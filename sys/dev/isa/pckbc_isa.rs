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
 * Copyright (c) 1998
 *	Matthias Drochner.  All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT, INDIRECT,
 * INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT
 * NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
 * DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
 * THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF
 * THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! `pckbc(4)` on the ISA bus: `dev/isa/pckbc_isa.c`.
//!
//! Upstream: sys/dev/isa/pckbc_isa.c @ 3ce1f3f79392
//!
//! The match runs the controller's self test at `IO_KBD` (unless it is the console's) and
//! claims the five ports and the keyboard's and the mouse's interrupts, 1 and 12; the attach
//! establishes both, maps the ports (or takes the console's state) and hands over to
//! `pckbc_attach`. On QEMU's q35 and pc machines the i8042 is always there.
//!
//! ## Deviations
//! - The internal state is a `PckbcInternal` written into its `malloc`ed block (the C's
//!   `M_ZERO` block), never freed; the softc reaches `pckbc_attach` as `&'static`, as in
//!   `com_isa.rs`.
//! - The C's "couldn't map" panic keeps its text.

use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::Ordering;

use crate::dev::ic::i8042reg::{KBC_SELFTEST, KBCMDP, KBDATAP, KC8_CPU};
use crate::dev::ic::pckbc::{
    PCKBC_CONSDATA, PCKBC_CONSOLE_ATTACHED, pckbc_attach, pckbc_is_console, pckbc_poll_data1,
    pckbc_reset, pckbc_send_cmd, pckbc_stop, pckbcintr,
};
use crate::dev::ic::pckbcvar::{
    PCKBC_AUX_SLOT, PCKBC_KBD_SLOT, PCKBC_NSLOTS, PckbcInternal, PckbcSoftc,
};
use crate::dev::isa::isareg::IO_KBD;
use crate::dev::isa::isavar::{DRQUNK, IOBASEUNK, IRQUNK, IsaAttachArgs, MADDRUNK};
use crate::kern::kern_malloc::malloc;
use crate::kern::subr_autoconf::config_activate_children;
use crate::kern::subr_prf::{panic, printf};
use crate::machine::bus::{bus_space_map, bus_space_unmap};
use crate::machine::intr::IPL_TTY;
use crate::machine::isa_machdep::{IST_EDGE, isa_intr_establish};
use crate::sys::device::{CfMatch, Cfattach, DVACT_RESUME, DVACT_SUSPEND, Device};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_DEVBUF, M_WAITOK, M_ZERO};

/// `pckbc_isa_ca`.
pub static PCKBC_ISA_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<PckbcSoftc>(),
    ca_match: Some(pckbc_isa_match),
    ca_attach: pckbc_isa_attach,
    ca_detach: None,
    ca_activate: Some(pckbc_isa_activate),
};

/// `pckbc_isa_match`: a controller at `IO_KBD` that passes its self test.
pub fn pckbc_isa_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: the ISA bus hands its children `isa_attach_args` (`isascan`).
    let ia = unsafe { &mut *aux.cast::<IsaAttachArgs>() };
    let iot = ia.ia_iot;

    // If values are hardwired to something that they can't be, punt.
    if (ia.ia_iobase() != IOBASEUNK && ia.ia_iobase() != i32::from(IO_KBD))
        || ia.ia_maddr() != MADDRUNK
        || (ia.ia_irq() != IRQUNK && ia.ia_irq() != 1/* XXX */)
        || ia.ia_drq() != DRQUNK
    {
        return 0;
    }

    if !pckbc_is_console(iot, usize::from(IO_KBD)) {
        // SAFETY: the controller's data port, which only this probe touches.
        let Ok(ioh_d) = (unsafe { bus_space_map(iot, usize::from(IO_KBD) + KBDATAP, 1, 0) }) else {
            return 0;
        };
        // SAFETY: as above, its command port.
        let Ok(ioh_c) = (unsafe { bus_space_map(iot, usize::from(IO_KBD) + KBCMDP, 1, 0) }) else {
            // fail:
            bus_space_unmap(iot, ioh_d, 1);
            return 0;
        };

        // flush KBC
        let _ = pckbc_poll_data1(iot, ioh_d, ioh_c, PCKBC_KBD_SLOT, false);

        // KBC selftest
        let ok = 'test: {
            if !pckbc_send_cmd(iot, ioh_c, KBC_SELFTEST) {
                break 'test false;
            }
            let res = pckbc_poll_data1(iot, ioh_d, ioh_c, PCKBC_KBD_SLOT, false);
            if res != Some(0x55) {
                printf(format_args!(
                    "kbc selftest: {:x}\n",
                    res.map_or(-1, i32::from)
                ));
                break 'test false;
            }
            true
        };
        // fail2: / fail:
        bus_space_unmap(iot, ioh_c, 1);
        bus_space_unmap(iot, ioh_d, 1);
        if !ok {
            return 0;
        }
    }

    ia.set_ia_iobase(i32::from(IO_KBD));
    ia.set_ia_iosize(5);
    ia.set_ia_msize(0x0);
    ia.ipa_nirq = PCKBC_NSLOTS as u8;
    ia.ipa_irq[PCKBC_KBD_SLOT as usize].num = 1;
    ia.ipa_irq[PCKBC_AUX_SLOT as usize].num = 12;

    1
}

/// `pckbc_isa_activate`: stop the controller before its children suspend, reset it before
/// they resume.
pub fn pckbc_isa_activate(self_: &Device, act: i32) -> Result<(), Errno> {
    // SAFETY: `pckbc_isa_ca` makes `PckbcSoftc`s.
    let sc = unsafe { self_.softc::<PckbcSoftc>() };

    match act {
        DVACT_SUSPEND => {
            let rv = config_activate_children(self_, act);
            pckbc_stop(sc);
            rv
        }
        DVACT_RESUME => {
            pckbc_reset(sc);
            config_activate_children(self_, act)
        }
        _ => config_activate_children(self_, act),
    }
}

/// `pckbc_isa_attach`: establish the two interrupts, map the ports (or take the console's
/// controller) and attach the controller.
pub fn pckbc_isa_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `pckbc_isa_ca` makes `PckbcSoftc`s; `config_make_softc`'s allocation lives as
    // long as the device, which is never freed while attached.
    let sc: &'static PckbcSoftc = unsafe { &*ptr::from_ref(self_.softc::<PckbcSoftc>()) };
    let cf = self_.cfdata();
    // SAFETY: as in `pckbc_isa_match`.
    let ia = unsafe { &*aux.cast::<IsaAttachArgs>() };

    let iot = ia.ia_iot;

    printf(format_args!("\n"));

    // SAFETY: the softc is the device, alive while attached (see above).
    let name: &'static str = unsafe { &*ptr::from_ref(sc.sc_dv.xname()) };
    for slot in 0..PCKBC_NSLOTS {
        let irq = i32::from(ia.ipa_irq[slot].num);
        let rv = isa_intr_establish(
            ia.ia_ic,
            irq,
            IST_EDGE,
            IPL_TTY,
            pckbcintr,
            ptr::from_ref(sc).cast_mut().cast::<c_void>(),
            name,
        );
        if rv.is_none() {
            printf(format_args!(
                "{}: unable to establish interrupt for irq {}\n",
                name, irq
            ));
            // XXX fail attach?
        }
    }

    let t: &'static PckbcInternal = if pckbc_is_console(iot, usize::from(IO_KBD)) {
        PCKBC_CONSOLE_ATTACHED.store(1, Ordering::Relaxed);
        // t->t_cmdbyte was initialized by cnattach
        &PCKBC_CONSDATA
    } else {
        // SAFETY: the controller's ports, which the match probed and released.
        let ioh_d = unsafe { bus_space_map(iot, usize::from(IO_KBD) + KBDATAP, 1, 0) };
        // SAFETY: as above.
        let ioh_c = unsafe { bus_space_map(iot, usize::from(IO_KBD) + KBCMDP, 1, 0) };
        let (Ok(ioh_d), Ok(ioh_c)) = (ioh_d, ioh_c) else {
            panic(format_args!("pckbc_attach: couldn't map"));
        };

        let Some(mem) = malloc(size_of::<PckbcInternal>(), M_DEVBUF, M_WAITOK | M_ZERO) else {
            panic(format_args!("pckbc_attach: couldn't map"));
        };
        let p: NonNull<PckbcInternal> = mem.cast();
        // SAFETY: a fresh block of `size_of::<PckbcInternal>()` bytes, aligned by `malloc`,
        // that nothing else references; it is never freed.
        let t: &'static PckbcInternal = unsafe {
            p.as_ptr().write(PckbcInternal::new());
            &*p.as_ptr()
        };
        t.t_iot.set(Some(iot));
        t.t_ioh_d.set(Some(ioh_d));
        t.t_ioh_c.set(Some(ioh_c));
        t.t_addr.set(usize::from(IO_KBD));
        t.t_cmdbyte.set(KC8_CPU); // Enable ports
        t
    };

    t.t_sc.set(Some(NonNull::from(sc)));
    sc.id.set(Some(t));

    // Finish off the attach.
    pckbc_attach(sc, cf.cf_flags);
}
/* </CODE> */
