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

/*++

Copyright (c)  1999 - 2002 Intel Corporation. All rights reserved
This software and associated documentation (if any) is furnished
under a license and may only be used or copied in accordance
with the terms of the license. Except as permitted by such
license, no part of this software or documentation may be
reproduced, stored in a retrieval system, or transmitted in any
form or by any means without the express written consent of
Intel Corporation.

Module Name:

    efifs.h

Abstract:

    EFI File System structures



Revision History

--*/
/* </LICENSES> */

/* <CODE> */
//! EFI native file system: partition header, file header and logical block address lists.
//!
//! Upstream: sys/stand/efi/include/efifs.h @ 3ce1f3f79392
//!
//! These are the on-disk structures of the original EFI file system, which no firmware
//! implements any more; the header is ported complete for fidelity.
//!
//! ## Deviations
//! - Type names are the C typedefs in CamelCase (`EFI_PARTITION_HEADER` ->
//!   `EfiPartitionHeader`, `EFI_RL` -> `EfiRl`, ...). The misspelt `SecutiryFile` field keeps
//!   its spelling.
//! - `EFI_FILE_LBAL`, `EFI_LBAL_ARRAY_SIZE` and `EFI_LBAL_RL` are the unsafe fns
//!   [`efi_file_lbal`], [`efi_lbal_array_size`] and [`efi_lbal_rl`], as they dereference.
//! - The signature constants are `UINT64`.

#![allow(non_snake_case, non_upper_case_globals, non_camel_case_types)]
// EFI names are the specification's (Hdr, LBALOffset, SecutiryFile, ...), kept verbatim for grep-ability

use core::mem::size_of;

use super::amd64::efibind::{UINT32, UINT64, UINTN};
use super::efiapi::EfiTableHeader;
use super::efidef::{CHAR16, EfiGuid, EfiLba, EfiTime};

/// `EFI_PARTITION_SIGNATURE`.
pub const EFI_PARTITION_SIGNATURE: UINT64 = 0x5053595320494249;
/// `EFI_PARTITION_REVISION`.
pub const EFI_PARTITION_REVISION: UINT32 = 0x00010001;
/// `MIN_EFI_PARTITION_BLOCK_SIZE`.
pub const MIN_EFI_PARTITION_BLOCK_SIZE: UINTN = 512;
/// `EFI_PARTITION_LBA`: the partition header normally starts in LBA 1.
pub const EFI_PARTITION_LBA: EfiLba = 1;

/// `EFI_FILE_HEADER_SIGNATURE`.
pub const EFI_FILE_HEADER_SIGNATURE: UINT64 = 0x454c494620494249;
/// `EFI_FILE_HEADER_REVISION`.
pub const EFI_FILE_HEADER_REVISION: UINT32 = 0x00010000;
/// `EFI_FILE_STRING_SIZE`.
pub const EFI_FILE_STRING_SIZE: UINTN = 260;

/// `EFI_FILE_CLASS_FREE_SPACE`.
pub const EFI_FILE_CLASS_FREE_SPACE: UINT32 = 1;
/// `EFI_FILE_CLASS_EMPTY`.
pub const EFI_FILE_CLASS_EMPTY: UINT32 = 2;
/// `EFI_FILE_CLASS_NORMAL`.
pub const EFI_FILE_CLASS_NORMAL: UINT32 = 3;

/// `EFI_LBAL_SIGNATURE`.
pub const EFI_LBAL_SIGNATURE: UINT64 = 0x4c41424c20494249;
/// `EFI_LBAL_REVISION`.
pub const EFI_LBAL_REVISION: UINT32 = 0x00010000;

/// `EFI_PARTITION_HEADER`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EfiPartitionHeader {
    /// Table header.
    pub Hdr: EfiTableHeader,
    /// Directory allocation number.
    pub DirectoryAllocationNumber: UINT32,
    /// Block size.
    pub BlockSize: UINT32,
    /// First usable LBA.
    pub FirstUsableLba: EfiLba,
    /// Last usable LBA.
    pub LastUsableLba: EfiLba,
    /// Unusable space.
    pub UnusableSpace: EfiLba,
    /// Free space.
    pub FreeSpace: EfiLba,
    /// Root file.
    pub RootFile: EfiLba,
    /// Security file (spelt `SecutiryFile` in the C).
    pub SecutiryFile: EfiLba,
}

/// `EFI_FILE_HEADER`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EfiFileHeader {
    /// Table header.
    pub Hdr: EfiTableHeader,
    /// `EFI_FILE_CLASS_*`.
    pub Class: UINT32,
    /// Offset of the first [`EfiLbal`] from the start of the header.
    pub LBALOffset: UINT32,
    /// Parent.
    pub Parent: EfiLba,
    /// File size.
    pub FileSize: UINT64,
    /// File attributes.
    pub FileAttributes: UINT64,
    /// Creation time.
    pub FileCreateTime: EfiTime,
    /// Modification time.
    pub FileModificationTime: EfiTime,
    /// Vendor GUID.
    pub VendorGuid: EfiGuid,
    /// File name.
    pub FileString: [CHAR16; EFI_FILE_STRING_SIZE],
}

/// `EFI_LBAL`: a logical block address list, the fundamental block description structure.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EfiLbal {
    /// Table header.
    pub Hdr: EfiTableHeader,
    /// Class.
    pub Class: UINT32,
    /// Parent.
    pub Parent: EfiLba,
    /// Next list.
    pub Next: EfiLba,
    /// Array size.
    pub ArraySize: UINT32,
    /// Array count.
    pub ArrayCount: UINT32,
}

/// `EFI_RL`: a logical block run length.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EfiRl {
    /// First block.
    pub Start: EfiLba,
    /// Length.
    pub Length: UINT64,
}

/// `EFI_FILE_LBAL(a)`: the file's first LBAL, in the same logical block as the file header.
///
/// # Safety
/// `a` must point at a valid, readable [`EfiFileHeader`] whose `LBALOffset` stays within the
/// block that holds it.
pub unsafe fn efi_file_lbal(a: *const EfiFileHeader) -> *mut EfiLbal {
    // SAFETY: the caller guarantees `a` is readable and `LBALOffset` stays within its block.
    unsafe { a.cast::<u8>().add((*a).LBALOffset as usize) as *mut EfiLbal }
}

/// `EFI_LBAL_ARRAY_SIZE(lbal, offs, blks)`: how many run lengths fit in a block of `blks`
/// whose LBAL starts at offset `offs`. Unsigned arithmetic wraps as in C.
///
/// # Safety
/// `lbal` must point at a valid, readable [`EfiLbal`].
pub unsafe fn efi_lbal_array_size(lbal: *const EfiLbal, offs: UINTN, blks: UINTN) -> UINTN {
    // SAFETY: the caller guarantees `lbal` is readable.
    let header_size = unsafe { (*lbal).Hdr.HeaderSize } as UINTN;
    blks.wrapping_sub(offs).wrapping_sub(header_size) / size_of::<EfiRl>()
}

/// `EFI_LBAL_RL(a)`: the run-length array that follows an LBAL header.
///
/// # Safety
/// `a` must point at a valid, readable [`EfiLbal`] whose `Hdr.HeaderSize` stays within the
/// block that holds it.
pub unsafe fn efi_lbal_rl(a: *const EfiLbal) -> *mut EfiRl {
    // SAFETY: the caller guarantees `a` is readable and `HeaderSize` stays within its block.
    unsafe { a.cast::<u8>().add((*a).Hdr.HeaderSize as usize) as *mut EfiRl }
}

const _: () = assert!(size_of::<EfiPartitionHeader>() == 24 + 8 + 6 * 8);
const _: () = assert!(size_of::<EfiRl>() == 16);
/* </CODE> */
