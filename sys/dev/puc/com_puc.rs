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
/* </LICENSES> */

/* <CODE> */
//! `com(4)` on `puc(4)`: `dev/puc/com_puc.c`.
//!
//! Upstream: sys/dev/puc/com_puc.c @ 3ce1f3f79392
//!
//! A serial port of a PCI multi-port card: takes the interrupt `puc*` mapped, the registers it
//! sub-regioned for the port, and the clock of the card's UART (`puc_port_types`), then attaches
//! the generic `com(4)` driver.
//!
//! ## Deviations
//! - `intr_string` never fails here (`pci_intr_string` returns a string by value), so the C's
//!   `if (intrstr != NULL)` tests are gone.
//! - The softc reaches `com_attach_subr` as `&'static` and the interrupt is established under
//!   the device's name, as in `com_isa.rs`.
//! - `com_puc_detach` is `com_detach` as in the C; the interrupt is disestablished by the card
//!   (`puc_pci_detach`).

use core::ffi::c_void;
use core::ptr;

use crate::dev::ic::com::{com_activate, com_attach_subr, com_detach, comintr};
use crate::dev::ic::comreg::COM_FREQ;
use crate::dev::ic::comvar::{COM_UART_XR17V35X, ComSoftc};
use crate::dev::pci::pucvar::{PUC_PORT_COM_XR17V35X, PUC_PORT_TYPES, PucAttachArgs, puc_is_com};
use crate::kern::subr_prf::printf;
use crate::machine::intr::IPL_TTY;
use crate::sys::device::{CfMatch, Cfattach, Device};
use crate::sys::errno::Errno;

/// `com_puc_ca`.
pub static COM_PUC_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<ComSoftc>(),
    ca_match: Some(com_puc_match),
    ca_attach: com_puc_attach,
    ca_detach: Some(com_puc_detach),
    ca_activate: Some(com_activate),
};

/// `com_puc_match`: every port of a card that is not a parallel one.
pub fn com_puc_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: `puc_common_attach` hands its children a `puc_attach_args`.
    let pa = unsafe { &*aux.cast::<PucAttachArgs>() };

    i32::from(puc_is_com(pa.type_))
}

/// `com_puc_attach`: grab the card's interrupt, take the port's registers and attach the UART.
pub fn com_puc_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `com_puc_ca` makes `ComSoftc`s; `config_make_softc`'s allocation lives as long
    // as the device, which is never freed while attached.
    let sc: &'static ComSoftc = unsafe { &*ptr::from_ref(self_.softc::<ComSoftc>()) };
    // SAFETY: as in `com_puc_match`.
    let pa = unsafe { &*aux.cast::<PucAttachArgs>() };

    // Grab a PCI interrupt.
    let intrstr = (pa.intr_string)(pa);
    // SAFETY: the softc is the device, alive while attached (see above).
    let name: &'static str = unsafe { &*ptr::from_ref(sc.sc_dev.xname()) };
    let ih = (pa.intr_establish)(
        pa,
        IPL_TTY,
        comintr,
        ptr::from_ref(sc).cast_mut().cast::<c_void>(),
        name,
    );
    let Some(ih) = ih else {
        printf(format_args!(
            ": couldn't establish interrupt at {}\n",
            intrstr
        ));
        return;
    };
    sc.sc_ih.set(ih.as_ptr());
    printf(format_args!(" {}", intrstr));

    sc.sc_iot.set(pa.t);
    sc.sc_ioh.set(pa.h);
    sc.sc_iobase.set(pa.a);

    sc.sc_frequency.set(COM_FREQ);

    if let Some(t) = PUC_PORT_TYPES.iter().find(|t| t.type_ == pa.type_) {
        sc.sc_frequency.set(t.freq as i32);
    }

    if pa.type_ == PUC_PORT_COM_XR17V35X {
        sc.sc_uarttype.set(COM_UART_XR17V35X);
    }

    com_attach_subr(sc);
}

/// `com_puc_detach`.
pub fn com_puc_detach(self_: &Device, flags: i32) -> Result<(), Errno> {
    com_detach(self_, flags)
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::dev::pci::pucvar::{PUC_PORT_COM, PUC_PORT_LPT};

    #[test]
    fn a_port_clock_comes_from_its_type() {
        let clock = |ty: i32| {
            PUC_PORT_TYPES
                .iter()
                .find(|t| t.type_ == ty)
                .map(|t| t.freq)
        };

        assert_eq!(clock(PUC_PORT_COM), Some(COM_FREQ as u32));
        assert_eq!(clock(PUC_PORT_COM_XR17V35X), Some(125_000_000));
        assert!(puc_is_com(PUC_PORT_COM) && !puc_is_com(PUC_PORT_LPT));
    }
}
/* </TESTS> */
