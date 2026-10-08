/*	$OpenBSD: efirng.c,v 1.3 2021/06/06 23:56:55 krw Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2018 Mark Kettenis <kettenis@openbsd.org>
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
/* </LICENSES> */

/* <CODE> */
//! `fwrandom()`: mix the firmware's random number generator (`EFI_RNG_PROTOCOL`) into the
//! seed.
//!
//! Upstream: sys/arch/arm64/stand/efiboot/efirng.c @ 3ce1f3f79392

#![allow(non_snake_case)] // the UEFI specification's member names

use alloc::vec;
use core::ffi::c_void;
use core::ptr;

use efi::include::efi::*;
use libsa::printf;

use crate::efiboot::bs;

/// `EFI_RNG_PROTOCOL_GUID`.
const EFI_RNG_PROTOCOL_GUID: EfiGuid = EfiGuid::new(
    0x3152_bca5,
    0xeade,
    0x433d,
    [0x86, 0x2e, 0xc0, 0x1c, 0xdc, 0x29, 0x1f, 0x44],
);

/// `EFI_RNG_ALGORITHM`.
type EfiRngAlgorithm = EfiGuid;

/// `EFI_RNG_GET_INFO`.
type EfiRngGetInfo = unsafe extern "efiapi" fn(
    This: *mut EfiRngProtocol,
    RNGAlgorithmListSize: *mut UINTN,
    RNGAlgorithmList: *mut EfiRngAlgorithm,
) -> EfiStatus;

/// `EFI_RNG_GET_RNG`.
type EfiRngGetRng = unsafe extern "efiapi" fn(
    This: *mut EfiRngProtocol,
    RNGAlgorithm: *mut EfiRngAlgorithm,
    RNGValueLength: UINTN,
    RNGValue: *mut UINT8,
) -> EfiStatus;

/// `EFI_RNG_PROTOCOL`.
#[repr(C)]
struct EfiRngProtocol {
    GetInfo: EfiRngGetInfo,
    GetRNG: EfiRngGetRng,
}

/// `fwrandom(buf, buflen)`: XOR the firmware's random bytes into `buf`; 0 when it did.
pub fn fwrandom(buf: &mut [u8]) -> i32 {
    let mut guid = EFI_RNG_PROTOCOL_GUID;
    let mut rng: *mut c_void = ptr::null_mut();
    // SAFETY: a boot service with valid pointers.
    let status = unsafe { (bs().LocateProtocol)(&mut guid, ptr::null_mut(), &mut rng) };
    if rng.is_null() || efi_error(status) {
        return -1;
    }
    let rng = rng.cast::<EfiRngProtocol>();

    let mut random = vec![0u8; buf.len()];

    // SAFETY: the firmware's RNG protocol and a buffer of `buflen` bytes.
    let status =
        unsafe { ((*rng).GetRNG)(rng, ptr::null_mut(), random.len(), random.as_mut_ptr()) };
    if efi_error(status) {
        printf!("RNG GetRNG() failed ({})\n", status as i32);
        return -1;
    }

    for (b, r) in buf.iter_mut().zip(&random) {
        *b ^= r;
    }

    0
}
/* </CODE> */
