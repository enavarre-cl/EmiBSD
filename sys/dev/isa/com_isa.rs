/*	$OpenBSD: com_isa.c,v 1.10 2022/04/06 18:59:28 naddy Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1997 - 1999, Jason Downs.  All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name(s) of the author(s) nor the name OpenBSD
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR(S) ``AS IS'' AND ANY EXPRESS
 * OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
 * WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
 * DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR(S) BE LIABLE FOR ANY DIRECT,
 * INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
 * (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
 * SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
 * CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 */

/*-
 * Copyright (c) 1993, 1994, 1995, 1996
 *	Charles M. Hannum.  All rights reserved.
 * Copyright (c) 1991 The Regents of the University of California.
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
 * 3. Neither the name of the University nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE REGENTS AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *	@(#)com.c	7.5 (Berkeley) 5/16/91
 */
/* </LICENSES> */

//! `com(4)` on the ISA bus: `dev/isa/com_isa.c`.
//!
//! Upstream: sys/dev/isa/com_isa.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The softc reaches `com_attach_subr` as `&'static`: `config_make_softc`'s allocation is
//!   never freed while the device is attached, and the interrupt is established under the
//!   device's name ([`Device::xname`]).

use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::Ordering;

use crate::dev::ic::com::{
    COMCONSADDR, COMCONSATTACHED, com_activate, com_attach_subr, comconsioh, comintr, comprobe1,
};
use crate::dev::ic::comreg::{COM_FREQ, COM_NPORTS};
use crate::dev::ic::comvar::ComSoftc;
use crate::dev::isa::isavar::{IRQUNK, IsaAttachArgs};
use crate::kern::subr_prf::panic;
use crate::machine::bus::{bus_space_map, bus_space_unmap};
use crate::machine::intr::IPL_TTY;
use crate::machine::isa_machdep::{IST_EDGE, isa_intr_establish};
use crate::sys::device::{CfMatch, Cfattach, Device};

/// `com_isa_ca`.
pub static COM_ISA_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<ComSoftc>(),
    ca_match: Some(com_isa_probe),
    ca_attach: com_isa_attach,
    ca_detach: None,
    ca_activate: Some(com_activate),
};

/// `com_isa_probe`: a UART at the line's port (the console's is taken without probing).
pub fn com_isa_probe(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: the ISA bus hands its children `isa_attach_args` (`isascan`).
    let ia = unsafe { &mut *aux.cast::<IsaAttachArgs>() };

    let iot = ia.ia_iot;
    let iobase = ia.ia_iobase();

    if !(iobase as usize == COMCONSADDR.load(Ordering::Relaxed)
        && !COMCONSATTACHED.load(Ordering::Relaxed))
    {
        // SAFETY: the GENERIC line's ISA port range, which only this probe touches.
        let Ok(ioh) = (unsafe { bus_space_map(iot, iobase as usize, COM_NPORTS, 0) }) else {
            return 0;
        };

        let rv = comprobe1(iot, ioh);

        bus_space_unmap(iot, ioh, COM_NPORTS);

        if !rv {
            return 0;
        }
    }

    // out:
    ia.set_ia_iosize(COM_NPORTS as i32);
    ia.set_ia_msize(0);
    1
}

/// `com_isa_attach`: map the port (the console's mapping is reused), attach the UART and
/// establish its interrupt.
pub fn com_isa_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `com_isa_ca` makes `ComSoftc`s; `config_make_softc`'s allocation lives as long
    // as the device, which is never freed while attached.
    let sc: &'static ComSoftc = unsafe { &*ptr::from_ref(self_.softc::<ComSoftc>()) };
    // SAFETY: as in `com_isa_probe`.
    let ia = unsafe { &*aux.cast::<IsaAttachArgs>() };

    sc.sc_hwflags.set(0);
    sc.sc_swflags.set(0);

    let iobase = ia.ia_iobase() as usize;
    let iot = ia.ia_iot;

    let ioh = if iobase != COMCONSADDR.load(Ordering::Relaxed) {
        // SAFETY: as in `com_isa_probe`.
        match unsafe { bus_space_map(iot, iobase, COM_NPORTS, 0) } {
            Ok(ioh) => ioh,
            Err(_) => panic(format_args!("com_isa_attach: mapping failed")),
        }
    } else {
        match comconsioh() {
            Some(ioh) => ioh,
            None => panic(format_args!("com_isa_attach: mapping failed")),
        }
    };

    sc.sc_iot.set(Some(iot));
    sc.sc_ioh.set(Some(ioh));
    sc.sc_iobase.set(iobase);
    sc.sc_frequency.set(COM_FREQ);

    com_attach_subr(sc);

    let irq = ia.ia_irq();
    if irq != IRQUNK {
        // SAFETY: the softc is the device, alive while attached (see above).
        let name: &'static str = unsafe { &*ptr::from_ref(sc.sc_dev.xname()) };
        let ih = isa_intr_establish(
            ia.ia_ic,
            irq,
            IST_EDGE,
            IPL_TTY,
            comintr,
            ptr::from_ref(sc).cast_mut().cast::<c_void>(),
            name,
        );
        sc.sc_ih.set(ih.map_or(ptr::null_mut(), |ih| ih.as_ptr()));
    }
}
