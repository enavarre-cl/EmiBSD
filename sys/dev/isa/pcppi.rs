/* <LICENSES> */
/*
 * Copyright (c) 2026 Emilio Navarrete Lineros <enavarre@outlook.com>
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */

/*
 * Copyright (c) 1996 Carnegie-Mellon University.
 * All rights reserved.
 *
 * Author: Chris G. Demetriou
 *
 * Permission to use, copy, modify and distribute this software and
 * its documentation is hereby granted, provided that both the copyright
 * notice and this permission notice appear in all copies of the
 * software, derivative works or modified versions, and any portions
 * thereof, and that both notices appear in supporting documentation.
 *
 * CARNEGIE MELLON ALLOWS FREE USE OF THIS SOFTWARE IN ITS "AS IS"
 * CONDITION.  CARNEGIE MELLON DISCLAIMS ANY LIABILITY OF ANY KIND
 * FOR ANY DAMAGES WHATSOEVER RESULTING FROM THE USE OF THIS SOFTWARE.
 *
 * Carnegie Mellon requests users of this software to return to
 *
 *  Software Distribution Coordinator  or  Software.Distribution@CS.CMU.EDU
 *  School of Computer Science
 *  Carnegie Mellon University
 *  Pittsburgh PA 15213-3890
 *
 * any improvements or extensions that they make and grant Carnegie the
 * rights to redistribute these changes.
 */
/* </LICENSES> */

/* <CODE> */
//! `pcppi(4)`: the PC's Programmable Peripheral Interface port B and the i8254's counter 2,
//! that is, the PC speaker (`pcppi0 at isa?`). It provides the bell: the keyboards' (`pckbd`,
//! `hidkbd`) through `pcppi_kbd_bell`, and spkr(4)'s, its child, through [`pcppi_bell`].
//!
//! Upstream: sys/dev/isa/pcppi.c @ 3ce1f3f79392
//!
//! A tone is counter 2 programmed as a square wave of the pitch (`TIMER_DIV(pitch)`), gated
//! to the speaker by `PIT_SPKR` in port B; a timeout (or a busy wait for a polled bell)
//! clears the bits after the period.
//!
//! ## Deviations
//! - The bell's sleep channel, `pcppi_bell_stop`'s address in the C, is the function's
//!   address here too.
//! - `NPCKBD` and `NHIDKBD` are both greater than 0 (amd64's GENERIC has `pckbd*` and
//!   `ukbd*`), so both keyboards' bells are hooked up.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr;

use crate::dev::hid::hidkbd::hidkbd_hookup_bell;
use crate::dev::ic::i8253reg::{
    TIMER_16BIT, TIMER_CNTR2, TIMER_FREQ, TIMER_MODE, TIMER_SEL2, TIMER_SQWAVE, timer_div,
};
use crate::dev::isa::isareg::{IO_PPI, IO_TIMER1};
use crate::dev::isa::isavar::{DRQUNK, IOBASEUNK, IRQUNK, IsaAttachArgs, MADDRUNK};
use crate::dev::isa::pcppireg::PIT_SPKR;
use crate::dev::isa::pcppivar::{PCPPI_BELL_POLL, PCPPI_BELL_SLEEP, PcppiAttachArgs, PcppiTag};
use crate::dev::pckbc::pckbd::pckbd_hookup_bell;
use crate::kern::kern_synch::{tsleep_nsec, wakeup};
use crate::kern::kern_timeout::{timeout_add_msec, timeout_del, timeout_set};
use crate::kern::subr_autoconf::config_found;
use crate::kern::subr_prf::{panic, printf};
use crate::machine::bus::{
    BusSpaceHandle, BusSpaceTag, bus_space_map, bus_space_read_1, bus_space_unmap,
    bus_space_write_1,
};
use crate::machine::cpu::delay;
use crate::machine::intr::{splhigh, spltty, splx};
use crate::sys::device::{CfMatch, Cfattach, Cfdriver, DV_DULL, Device, Softc};
use crate::sys::param::{PCATCH, PZERO};
use crate::sys::systm::INFSLP;
use crate::sys::timeout::Timeout;

/// `PCPPIPRI`.
const PCPPIPRI: i32 = PZERO - 1;

/// `struct pcppi_softc`. Allocated zeroed by autoconf, so every member is valid as zero.
#[repr(C)]
pub struct PcppiSoftc {
    /// `sc_dv`.
    sc_dv: Device,

    /// `sc_iot`.
    sc_iot: Cell<Option<BusSpaceTag>>,
    /// `sc_ppi_ioh`.
    sc_ppi_ioh: Cell<Option<BusSpaceHandle>>,
    /// `sc_pit1_ioh`.
    sc_pit1_ioh: Cell<Option<BusSpaceHandle>>,

    /// `sc_bell_timeout`.
    sc_bell_timeout: Timeout,

    /// `sc_bellactive`.
    sc_bellactive: Cell<i32>,
    /// `sc_bellpitch`.
    sc_bellpitch: Cell<i32>,
    /// `sc_slp`.
    sc_slp: Cell<i32>,
    /// `sc_timeout`.
    sc_timeout: Cell<i32>,
}

// SAFETY: `#[repr(C)]` with the device first; the other members are `Cell`s of integers and
// `Option`s of bus handles, and the timeout, all valid as zero bits.
unsafe impl Softc for PcppiSoftc {}

impl PcppiSoftc {
    /// `sc->sc_iot`.
    fn iot(&self) -> BusSpaceTag {
        match self.sc_iot.get() {
            Some(t) => t,
            None => panic(format_args!("pcppi: no bus tag")),
        }
    }

    /// Port B (`sc_ppi_ioh`, offset 0).
    fn ppi(&self) -> BusSpaceHandle {
        match self.sc_ppi_ioh.get() {
            Some(h) => h,
            None => panic(format_args!("pcppi: PPI not mapped")),
        }
    }

    /// The i8254 (`sc_pit1_ioh`).
    fn pit1(&self) -> BusSpaceHandle {
        match self.sc_pit1_ioh.get() {
            Some(h) => h,
            None => panic(format_args!("pcppi: timer not mapped")),
        }
    }
}

/// `pcppi_ca`.
pub static PCPPI_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<PcppiSoftc>(),
    ca_match: Some(pcppi_match),
    ca_attach: pcppi_attach,
    ca_detach: None,
    ca_activate: None,
};

/// `pcppi_cd`.
pub static PCPPI_CD: Cfdriver = Cfdriver::new(b"pcppi", DV_DULL, 0);

/// The bell's sleep channel: `pcppi_bell_stop`'s address, as in the C.
fn bell_chan() -> *const () {
    pcppi_bell_stop as fn(*mut c_void) as *const ()
}

/// `pcppi_match`: port B's speaker data bit can be toggled.
pub fn pcppi_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: the ISA bus hands its children `isa_attach_args` (`isascan`).
    let ia = unsafe { &mut *aux.cast::<IsaAttachArgs>() };

    // If values are hardwired to something that they can't be, punt.
    if (ia.ia_iobase() != IOBASEUNK && ia.ia_iobase() != i32::from(IO_PPI))
        || ia.ia_maddr() != MADDRUNK
        || ia.ia_msize() != 0
        || ia.ia_irq() != IRQUNK
        || ia.ia_drq() != DRQUNK
    {
        return 0;
    }

    let iot = ia.ia_iot;
    let mut rv = 0;

    // SAFETY: the i8254's four ports, which nothing maps for good (the clock maps them for
    // its own accesses the same way).
    let pit1 = unsafe { bus_space_map(iot, usize::from(IO_TIMER1), 4, 0) }.ok();
    let ppi = if pit1.is_some() {
        // SAFETY: port B, which only pcppi uses.
        unsafe { bus_space_map(iot, usize::from(IO_PPI), 1, 0) }.ok()
    } else {
        None
    };

    if let Some(ppi_ioh) = ppi {
        // Check for existence of PPI.  Realistically, this is either going to be here or
        // nothing is going to be here.
        //
        // We don't want to have any chance of changing speaker output (which this test
        // might, if it crashes in the middle, or something; normally it's too quick to
        // produce anything audible), but many "combo chip" mock-PPI's don't seem to support
        // the top bit of Port B as a settable bit.  The bottom bit has to be settable, since
        // the speaker driver hardware still uses it.
        let v = bus_space_read_1(iot, ppi_ioh, 0); // XXX
        bus_space_write_1(iot, ppi_ioh, 0, v ^ 0x01); // XXX
        let nv = bus_space_read_1(iot, ppi_ioh, 0); // XXX
        if (nv ^ v) & 0x01 == 0x01 {
            rv = 1;
        }
        bus_space_write_1(iot, ppi_ioh, 0, v); // XXX
        let nv = bus_space_read_1(iot, ppi_ioh, 0); // XXX
        if (nv ^ v) & 0x01 != 0x00 {
            rv = 0;
        }

        // We assume that the programmable interval timer is there.
    }

    // lose:
    if let Some(h) = pit1 {
        bus_space_unmap(iot, h, 4);
    }
    if let Some(h) = ppi {
        bus_space_unmap(iot, h, 1);
    }
    if rv != 0 {
        ia.set_ia_iobase(i32::from(IO_PPI));
        ia.set_ia_iosize(0x1);
        ia.set_ia_msize(0x0);
    }
    rv
}

/// `pcppi_attach`: map the timer and port B, hook the keyboards' bells up, and attach the
/// children (spkr).
pub fn pcppi_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `pcppi_ca` makes `PcppiSoftc`s, never freed while the device exists.
    let sc: &'static PcppiSoftc = unsafe { &*ptr::from_ref(self_.softc::<PcppiSoftc>()) };
    // SAFETY: as in `pcppi_match`.
    let ia = unsafe { &*aux.cast::<IsaAttachArgs>() };
    let arg: *mut c_void = ptr::from_ref(sc).cast_mut().cast();

    timeout_set(&sc.sc_bell_timeout, pcppi_bell_stop, arg);

    let iot = ia.ia_iot;
    sc.sc_iot.set(Some(iot));

    // SAFETY: as in `pcppi_match`; the mappings stay for the device's life.
    let pit1 = unsafe { bus_space_map(iot, usize::from(IO_TIMER1), 4, 0) };
    // SAFETY: as above.
    let ppi = unsafe { bus_space_map(iot, usize::from(IO_PPI), 1, 0) };
    match (pit1, ppi) {
        (Ok(pit1), Ok(ppi)) => {
            sc.sc_pit1_ioh.set(Some(pit1));
            sc.sc_ppi_ioh.set(Some(ppi));
        }
        _ => panic(format_args!("pcppi_attach: couldn't map")),
    }

    printf(format_args!("\n"));

    sc.sc_bellactive.set(0);
    sc.sc_bellpitch.set(0);
    sc.sc_slp.set(0);

    // Provide a beeper for the keyboard, if there isn't one already.
    pckbd_hookup_bell(pcppi_kbd_bell, arg);
    hidkbd_hookup_bell(pcppi_kbd_bell, arg);

    let mut pa = PcppiAttachArgs { pa_cookie: arg };
    while config_found(self_, ptr::from_mut(&mut pa).cast(), None).is_some() {}
}

/// `pcppi_bell`: sound `pitch` Hz for `period_ms`, sleeping (`PCPPI_BELL_SLEEP`) or busy
/// waiting (`PCPPI_BELL_POLL`) until the end, or returning at once; a pitch or period of 0
/// stops the bell.
pub fn pcppi_bell(self_: PcppiTag, pitch: i32, period_ms: i32, slp: i32) {
    // SAFETY: `self_` is the `pa_cookie` pcppi_attach gave its children (or the keyboards'
    // bell argument): its softc, which is never freed.
    let sc = unsafe { &*self_.cast::<PcppiSoftc>() };

    let pitch = pitch.clamp(0, i32::MAX - TIMER_FREQ);
    let period_ms = period_ms.clamp(0, i32::MAX / 1000);

    let s1 = spltty(); // ???
    if sc.sc_bellactive.get() != 0 {
        if sc.sc_timeout.get() != 0 {
            sc.sc_timeout.set(0);
            timeout_del(&sc.sc_bell_timeout);
        }
        if sc.sc_slp.get() != 0 {
            wakeup(bell_chan());
        }
    }
    if pitch == 0 || period_ms == 0 {
        pcppi_bell_stop(self_);
        sc.sc_bellpitch.set(0);
        splx(s1);
        return;
    }
    let iot = sc.iot();
    if sc.sc_bellactive.get() == 0 || sc.sc_bellpitch.get() != pitch {
        let s2 = splhigh();
        let pit1 = sc.pit1();
        let div = timer_div(pitch);
        bus_space_write_1(
            iot,
            pit1,
            usize::from(TIMER_MODE),
            TIMER_SEL2 | TIMER_16BIT | TIMER_SQWAVE,
        );
        bus_space_write_1(iot, pit1, usize::from(TIMER_CNTR2), (div % 256) as u8);
        bus_space_write_1(iot, pit1, usize::from(TIMER_CNTR2), (div / 256) as u8);
        splx(s2);
        // enable speaker
        let ppi = sc.ppi();
        bus_space_write_1(iot, ppi, 0, bus_space_read_1(iot, ppi, 0) | PIT_SPKR);
    }
    sc.sc_bellpitch.set(pitch);

    sc.sc_bellactive.set(1);

    if slp & PCPPI_BELL_POLL != 0 {
        delay((period_ms * 1000) as u32);
        pcppi_bell_stop(self_);
    } else {
        sc.sc_timeout.set(1);
        timeout_add_msec(&sc.sc_bell_timeout, period_ms as u64);
        if slp & PCPPI_BELL_SLEEP != 0 {
            sc.sc_slp.set(1);
            let _ = tsleep_nsec(bell_chan(), PCPPIPRI | PCATCH, "bell", INFSLP);
            sc.sc_slp.set(0);
        }
    }
    splx(s1);
}

/// `pcppi_bell_stop`: gate the speaker off.
fn pcppi_bell_stop(arg: *mut c_void) {
    // SAFETY: the bell's timeout and pcppi_bell pass the softc, which is never freed.
    let sc = unsafe { &*arg.cast::<PcppiSoftc>() };

    let s = spltty(); // ???
    sc.sc_timeout.set(0);

    // disable bell
    let (iot, ppi) = (sc.iot(), sc.ppi());
    bus_space_write_1(iot, ppi, 0, bus_space_read_1(iot, ppi, 0) & !PIT_SPKR);
    sc.sc_bellactive.set(0);
    if sc.sc_slp.get() != 0 {
        wakeup(bell_chan());
    }
    splx(s);
}

/// `pcppi_kbd_bell`: the keyboards' bell (wskbd's `WSKBDIO_BELL`); NB: volume ignored but
/// for 0, which silences it.
pub fn pcppi_kbd_bell(arg: *mut c_void, pitch: u32, period: u32, volume: u32, poll: i32) {
    pcppi_bell(
        arg,
        if volume != 0 { pitch as i32 } else { 0 },
        period as i32,
        if poll != 0 { PCPPI_BELL_POLL } else { 0 },
    );
}
/* </CODE> */
