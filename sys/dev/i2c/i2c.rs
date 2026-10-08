/*	$OpenBSD: i2c.c,v 1.19 2022/04/06 18:59:28 naddy Exp $	*/
/*	$NetBSD: i2c.c,v 1.1 2003/09/30 00:35:31 thorpej Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2003 Wasabi Systems, Inc.
 * All rights reserved.
 *
 * Written by Jason R. Thorpe for Wasabi Systems, Inc.
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
 *      This product includes software developed for the NetBSD Project by
 *      Wasabi Systems, Inc.
 * 4. The name of Wasabi Systems, Inc. may not be used to endorse
 *    or promote products derived from this software without specific prior
 *    written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY WASABI SYSTEMS, INC. ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL WASABI SYSTEMS, INC
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! `iic*`: the i2c bus (`iic* at piixpm?`, `iic* at ichiic?`), see `iic(4)`.
//!
//! Upstream: sys/dev/i2c/i2c.c @ 3ce1f3f79392
//!
//! A controller driver attaches the bus with an [`I2cbusAttachArgs`]; `iic` attaches the
//! devices the kernel configuration names (`iic_search`: the entries with an `addr` locator)
//! and then scans the bus for known device signatures (`iic_scan` in `i2c_scan.rs`, or the
//! controller's own `iba_bus_scan`). A device found that no driver claims prints
//! `"name" at iic0 addr 0x50 not configured`, through [`iic_print`].
//!
//! ## Deviations
//! - `iic_search` reads `cf_loc[IICCF_ADDR]` as a `get`: an entry without locators is skipped
//!   (the C would read past its `loc[]`). No entry of this kernel's tables is a child of
//!   `iic` yet (`spdmem`, `lm`, ... are not ported), so the search finds none.
//! - `iic_is_compatible` returns a `bool` and takes `name` as a `&str`.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr;

use crate::dev::i2c::i2c_scan::iic_scan;
use crate::dev::i2c::i2cvar::{I2cAttachArgs, I2cTag, I2cbusAttachArgs};
use crate::kern::subr_autoconf::{config_attach, config_search};
use crate::kern::subr_prf::{Str, printf};
use crate::sys::device::{
    CD_SKIPHIBERNATE, CfMatch, Cfattach, Cfdriver, DV_DULL, Device, Softc, UNCONF,
};

/// `IICCF_ADDR`: the index of the `addr` locator.
pub const IICCF_ADDR: usize = 0;
/// `IICCF_SIZE`: the index of the `size` locator.
pub const IICCF_SIZE: usize = 1;

/// `struct iic_softc`.
#[repr(C)]
pub struct IicSoftc {
    /// `sc_dev`: generic device glue.
    pub sc_dev: Device,
    /// `sc_tag`: the controller, set by the attach.
    pub sc_tag: Cell<Option<I2cTag>>,
}

// SAFETY: `#[repr(C)]` with the device first; the other member is a `Cell` of an `Option` of
// a reference, valid as zero bits.
unsafe impl Softc for IicSoftc {}

/// `iic_ca`.
pub static IIC_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<IicSoftc>(),
    ca_match: Some(iic_match),
    ca_attach: iic_attach,
    ca_detach: None,
    ca_activate: None,
};

/// `iic_cd`.
pub static IIC_CD: Cfdriver = Cfdriver::new(b"iic", DV_DULL, CD_SKIPHIBERNATE);

/// `iicbus_print`: the print function a controller attaches its bus with.
pub fn iicbus_print(aux: *mut c_void, pnp: Option<&[u8]>) -> i32 {
    // SAFETY: the controller attaches the bus with an `i2cbus_attach_args`.
    let iba = unsafe { &*aux.cast::<I2cbusAttachArgs>() };

    if let Some(pnp) = pnp {
        printf(format_args!("{} at {}", iba.iba_name, Str(pnp)));
    }

    UNCONF
}

/// `iic_print`: the print function of the devices on the bus.
pub fn iic_print(aux: *mut c_void, pnp: Option<&[u8]>) -> i32 {
    // SAFETY: the devices on the bus are attached with an `i2c_attach_args`.
    let ia = unsafe { &*aux.cast::<I2cAttachArgs<'_>>() };

    if let Some(pnp) = pnp {
        printf(format_args!("\"{}\" at {}", Str(ia.ia_name), Str(pnp)));
    }
    printf(format_args!(" addr 0x{:x}", ia.ia_addr));

    UNCONF
}

/// `iic_search`: attaches the devices the kernel configuration file describes by address.
pub fn iic_search(parent: Option<&Device>, match_: &CfMatch, aux: *mut c_void) -> i32 {
    let Some(parent) = parent else {
        return 0;
    };
    // SAFETY: `iic_search` is only used as the submatch of `iic_attach`, whose parent is an
    // `iic*` made for `IIC_CA`.
    let sc = unsafe { parent.softc::<IicSoftc>() };
    let cf = match_.cfdata();
    let loc = |i: usize| cf.cf_loc.get(i).copied().unwrap_or(-1);

    if loc(IICCF_ADDR) != -1
        && let Some(tag) = sc.sc_tag.get()
    {
        let mut ia = I2cAttachArgs::new(
            tag,
            loc(IICCF_ADDR) as u16,
            loc(IICCF_SIZE) as i32,
            b"unknown",
        );
        let ia_ptr = ptr::from_mut(&mut ia).cast::<c_void>();

        if let Some(ca_match) = cf.cf_attach.ca_match
            && ca_match(Some(parent), match_, ia_ptr) > 0
        {
            config_attach(Some(parent), CfMatch::Cfdata(cf), ia_ptr, Some(iic_print));
        }
    }
    let _ = aux;
    0
}

/// `iic_match`: just make sure we're looking for i2c.
pub fn iic_match(_parent: Option<&Device>, match_: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: the controller attaches the bus with an `i2cbus_attach_args`.
    let iba = unsafe { &*aux.cast::<I2cbusAttachArgs>() };
    let cf = match_.cfdata();

    i32::from(iba.iba_name.as_bytes() == cf.cf_driver.cd_name)
}

/// `iic_attach`.
pub fn iic_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `self_` was made for `iic_ca`, whose softc is an `IicSoftc`.
    let sc = unsafe { self_.softc::<IicSoftc>() };
    // SAFETY: the controller attaches the bus with an `i2cbus_attach_args`.
    let iba = unsafe { &*aux.cast::<I2cbusAttachArgs>() };

    sc.sc_tag.set(Some(iba.iba_tag));

    printf(format_args!("\n"));

    // Attach all i2c devices described in the kernel configuration file.
    let _ = config_search(Some(iic_search), self_, ptr::null_mut());

    // Scan for known device signatures.
    match iba.iba_bus_scan {
        Some(scan) => scan(self_, iba, iba.iba_bus_scan_arg),
        None => iic_scan(self_, iba),
    }
}

/// `iic_is_compatible`: whether the device's name, or one of the names `ia_name`
/// concatenates, is `name`.
pub fn iic_is_compatible(ia: &I2cAttachArgs<'_>, name: &str) -> bool {
    let name = name.as_bytes();

    if ia.ia_namelen > 0 {
        // ia_name points to a concatenation of strings.
        let all = &ia.ia_name[..ia.ia_namelen.min(ia.ia_name.len())];
        let mut rest = all;
        while !rest.is_empty() {
            let len = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
            if &rest[..len] == name {
                return true;
            }
            rest = &rest[(len + 1).min(rest.len())..];
        }
    } else {
        // ia_name points to a string.
        let len = ia
            .ia_name
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(ia.ia_name.len());
        if &ia.ia_name[..len] == name {
            return true;
        }
    }

    false
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::dev::i2c::i2cvar::I2cController;
    use std::boxed::Box;

    fn tag() -> I2cTag {
        Box::leak(Box::new(I2cController::new()))
    }

    #[test]
    fn a_single_name_is_compared_whole() {
        let ia = I2cAttachArgs::new(tag(), 0x50, 1, b"eeprom");
        assert!(iic_is_compatible(&ia, "eeprom"));
        assert!(!iic_is_compatible(&ia, "eepro"));
        assert!(!iic_is_compatible(&ia, "eeprom2"));
        assert!(!iic_is_compatible(&ia, "lm75"));
    }

    #[test]
    fn a_concatenation_is_walked_name_by_name() {
        let names = b"atmel,24c32\0at24\0spd\0";
        let mut ia = I2cAttachArgs::new(tag(), 0x50, 1, names);
        ia.ia_namelen = names.len();
        assert!(iic_is_compatible(&ia, "atmel,24c32"));
        assert!(iic_is_compatible(&ia, "at24"));
        assert!(iic_is_compatible(&ia, "spd"));
        assert!(!iic_is_compatible(&ia, "atmel"));
        assert!(!iic_is_compatible(&ia, "24c32"));
        assert!(!iic_is_compatible(&ia, ""));
    }

    #[test]
    fn a_concatenation_without_the_last_nul_still_ends() {
        let names = b"a\0bc";
        let mut ia = I2cAttachArgs::new(tag(), 0x50, 1, names);
        ia.ia_namelen = names.len();
        assert!(iic_is_compatible(&ia, "bc"));
        assert!(!iic_is_compatible(&ia, "b"));
    }

    #[test]
    fn locator_indices_follow_files_i2c() {
        // define i2c {[addr = -1], [size = -1]}
        assert_eq!((IICCF_ADDR, IICCF_SIZE), (0, 1));
    }
}
/* </TESTS> */
