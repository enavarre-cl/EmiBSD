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
 * Copyright (c) 1993, 1994 Charles Hannum.
 * Copyright (c) 1990 William F. Jolitz, TeleMuse
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *	This software is a component of "386BSD" developed by
 *	William F. Jolitz, TeleMuse.
 * 4. Neither the name of the developer nor the name "386BSD"
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS A COMPONENT OF 386BSD DEVELOPED BY WILLIAM F. JOLITZ
 * AND IS INTENDED FOR RESEARCH AND EDUCATIONAL PURPOSES ONLY. THIS
 * SOFTWARE SHOULD NOT BE CONSIDERED TO BE A COMMERCIAL PRODUCT.
 * THE DEVELOPER URGES THAT USERS WHO REQUIRE A COMMERCIAL PRODUCT
 * NOT MAKE USE OF THIS WORK.
 *
 * FOR USERS WHO WISH TO UNDERSTAND THE 386BSD SYSTEM DEVELOPED
 * BY WILLIAM F. JOLITZ, WE RECOMMEND THE USER STUDY WRITTEN
 * REFERENCES SUCH AS THE  "PORTING UNIX TO THE 386" SERIES
 * (BEGINNING JANUARY 1991 "DR. DOBBS JOURNAL", USA AND BEGINNING
 * JUNE 1991 "UNIX MAGAZIN", GERMANY) BY WILLIAM F. JOLITZ AND
 * LYNNE GREER JOLITZ, AS WELL AS OTHER BOOKS ON UNIX AND THE
 * ON-LINE 386BSD USER MANUAL BEFORE USE. A BOOK DISCUSSING THE INTERNALS
 * OF 386BSD ENTITLED "386BSD FROM THE INSIDE OUT" WILL BE AVAILABLE LATE 1992.
 *
 * THIS SOFTWARE IS PROVIDED BY THE DEVELOPER ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE DEVELOPER BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! `lpt(4)` on the ISA bus: `dev/isa/lpt_isa.c`.
//!
//! Upstream: sys/dev/isa/lpt_isa.c @ 3ce1f3f79392
//!
//! The probe's logic:
//!
//! 1. You should be able to write to and read back the same value to the data port. Do an
//!    alternating zeros, alternating ones, walking zero, and walking one test to check for
//!    stuck bits.
//! 2. You should be able to write to and read back the same value to the control port lower
//!    5 bits, the upper 3 bits are reserved per the IBM PC technical reference manuals and
//!    different boards do different things with them. Do an alternating zeros, alternating
//!    ones, walking zero, and walking one test to check for stuck bits. Some printers drag
//!    the strobe line down when the are powered off so this bit has been masked out of the
//!    control port test. (XXX Some printers may not like a fast pulse on init or strobe, I
//!    don't know at this point, if that becomes a problem these bits should be turned off in
//!    the mask byte for the control port test.)
//! 3. Set the data and control ports to a value of 0.
//!
//! (The C tests the data port only, as here.)
//!
//! ## Deviations
//! - `DEBUG`'s `ABORT` message is not carried over (`DEBUG` is not configured).
//! - `__NO_ISA_INTR_CHECK` is not defined on amd64, so the probe checks the IRQ with
//!   `isa_intr_check` through the parent `isa_softc`'s chipset tag, as the C does.

use core::ffi::c_void;
use core::ptr;

use crate::dev::ic::lpt::{lpt_activate, lpt_attach_common, lpt_port_test, lptintr};
use crate::dev::ic::lptreg::{LPT_NPORTS, lpt_control, lpt_data};
use crate::dev::ic::lptvar::{LPT_POLLED, LptSoftc};
use crate::dev::isa::isavar::{IRQUNK, IsaAttachArgs, IsaSoftc};
use crate::kern::subr_prf::{panic, printf};
use crate::machine::bus::{bus_space_map, bus_space_unmap, bus_space_write_1};
use crate::machine::intr::IPL_TTY;
use crate::machine::isa_machdep::{IST_EDGE, isa_intr_check, isa_intr_establish};
use crate::sys::device::{CfMatch, Cfattach, Device};

/// `CHAR_BIT`.
const CHAR_BIT: u32 = 8;

/// `lpt_isa_ca`.
pub static LPT_ISA_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<LptSoftc>(),
    ca_match: Some(lpt_isa_probe),
    ca_attach: lpt_isa_attach,
    ca_detach: None,
    ca_activate: Some(lpt_activate),
};

/// `lpt_isa_probe`: a parallel port whose data register keeps what is written.
pub fn lpt_isa_probe(parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: the ISA bus hands its children `isa_attach_args` (`isascan`).
    let ia = unsafe { &mut *aux.cast::<IsaAttachArgs>() };

    let iot = ia.ia_iot;
    let base = ia.ia_iobase() as usize;
    let iosz = if ia.ia_iosize() == 0x666 {
        LPT_NPORTS
    } else {
        ia.ia_iosize() as usize
    };
    // SAFETY: the GENERIC line's ISA port range, which only this probe touches.
    let Ok(ioh) = (unsafe { bus_space_map(iot, base, iosz, 0) }) else {
        return 0;
    };

    let mask = 0xff;
    let data_ok = |data: u8| lpt_port_test(iot, ioh, base, lpt_data, data, mask);

    // Alternating zeros, alternating ones, walking zero, walking one.
    let ok = data_ok(0x55)
        && data_ok(0xaa)
        && (0..CHAR_BIT).all(|i| data_ok(!(1u8 << i)))
        && (0..CHAR_BIT).all(|i| data_ok(1u8 << i));

    let rv = if ok {
        bus_space_write_1(iot, ioh, lpt_data, 0);
        bus_space_write_1(iot, ioh, lpt_control, 0);

        // Check if the specified IRQ is available.  If not revert to polled mode.
        let ic = parent.and_then(|p| {
            // SAFETY: lpt's parent is the ISA bus, whose softc is an `IsaSoftc`.
            unsafe { p.softc::<IsaSoftc>() }.sc_ic.get()
        });
        if let Some(ic) = ic
            && ia.ia_irq() != IRQUNK
            && isa_intr_check(ic, ia.ia_irq(), IST_EDGE) == 0
        {
            ia.set_ia_irq(IRQUNK);
        }
        ia.set_ia_msize(0);
        ia.set_ia_iosize(iosz as i32);

        1
    } else {
        0
    };

    // out:
    bus_space_unmap(iot, ioh, iosz);
    rv
}

/// `lpt_isa_attach`: map the ports, attach, and establish the interrupt unless the IRQ was
/// refused (then polled).
pub fn lpt_isa_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `lpt_isa_ca` makes `LptSoftc`s; `config_make_softc`'s allocation lives as long
    // as the device, which is never freed while attached.
    let sc: &'static LptSoftc = unsafe { &*ptr::from_ref(self_.softc::<LptSoftc>()) };
    // SAFETY: as in `lpt_isa_probe`.
    let ia = unsafe { &*aux.cast::<IsaAttachArgs>() };

    sc.sc_state.set(0);
    sc.sc_iot.set(Some(ia.ia_iot));
    // SAFETY: the port range the probe tested and recorded.
    match unsafe {
        bus_space_map(
            ia.ia_iot,
            ia.ia_iobase() as usize,
            ia.ia_iosize() as usize,
            0,
        )
    } {
        Ok(ioh) => sc.sc_ioh.set(Some(ioh)),
        Err(_) => panic(format_args!("lpt_isa_attach: couldn't map I/O ports")),
    }

    if ia.ia_irq() == IRQUNK {
        sc.sc_flags.set(sc.sc_flags.get() | LPT_POLLED);
        printf(format_args!(": polled"));
    }

    lpt_attach_common(sc);

    if ia.ia_irq() != IRQUNK {
        // SAFETY: the softc is the device, alive while attached (see above).
        let name: &'static str = unsafe { &*ptr::from_ref(sc.sc_dev.xname()) };
        let ih = isa_intr_establish(
            ia.ia_ic,
            ia.ia_irq(),
            IST_EDGE,
            IPL_TTY,
            lptintr,
            ptr::from_ref(sc).cast_mut().cast::<c_void>(),
            name,
        );
        sc.sc_ih.set(ih.map_or(ptr::null_mut(), |ih| ih.as_ptr()));
    }
}
/* </CODE> */
