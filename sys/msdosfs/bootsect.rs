/*	$OpenBSD: bootsect.h,v 1.8 2025/08/01 13:00:18 jsg Exp $	*/
/*	$NetBSD: bootsect.h,v 1.8 1997/10/17 11:23:29 ws Exp $	*/
/* <LICENSES> */
/*
 * Written by Paul Popelka (paulp@uts.amdahl.com)
 *
 * You can do anything you want with this software, just don't say you wrote
 * it, and don't remove this notice.
 *
 * This software is provided "as is".
 *
 * The author supplies this software to be publicly redistributed on the
 * understanding that the author is not responsible for the correct
 * functioning of this software in any circumstances and is not liable for
 * any damages caused by this software.
 *
 * October 1992
 */
/* </LICENSES> */

//! `<msdosfs/bootsect.h>`: the format of a boot sector. This is the first sector on a DOS
//! floppy disk or the first sector of a partition on a hard disk. But, it is not the first
//! sector of a partitioned hard disk.
//!
//! Upstream: sys/msdosfs/bootsect.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The `int8_t` arrays are `u8` arrays (see `bpb.rs`: they are bytes read through
//!   `getushort`/`getulong` or copied), so every structure has the alignment of a byte and is
//!   viewed in place with `at`/`at_mut` (`byte_view!`).
//! - `union bootsector` is a `#[repr(C)]` union whose members are all 512 bytes of bytes; its
//!   accessors [`Bootsector::bs33`], [`Bootsector::bs50`] and [`Bootsector::bs710`] are safe,
//!   since any bytes are a valid value of every member.
//! - The `#if 0` block of `bsBPB` shorthands (`bsBytesPerSec`, ...) is not compiled in C and
//!   is left out.

use crate::msdosfs::bpb::byte_view;

/// `BOOTSIG0`: first byte of the boot sector signature.
pub const BOOTSIG0: u8 = 0x55;
/// `BOOTSIG1`: second byte of the boot sector signature.
pub const BOOTSIG1: u8 = 0xaa;
/// `BOOTSIG2`: FAT32's third signature byte.
pub const BOOTSIG2: u8 = 0;
/// `BOOTSIG3`: FAT32's fourth signature byte.
pub const BOOTSIG3: u8 = 0;
/// `EXBOOTSIG`: ext. boot signature (`exBootSignature`).
pub const EXBOOTSIG: u8 = 0x29;

/// `struct bootsector33`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(non_snake_case)] // the C field names
pub struct Bootsector33 {
    /// `bsJump`: jump inst E9xxxx or EBxx90.
    pub bsJump: [u8; 3],
    /// `bsOemName`: OEM name and version.
    pub bsOemName: [u8; 8],
    /// `bsBPB`: BIOS parameter block.
    pub bsBPB: [u8; 19],
    /// `bsDriveNumber`: drive number (0x80).
    pub bsDriveNumber: u8,
    /// `bsBootCode`: pad so struct is 512b.
    pub bsBootCode: [u8; 479],
    /// `bsBootSectSig0`.
    pub bsBootSectSig0: u8,
    /// `bsBootSectSig1`.
    pub bsBootSectSig1: u8,
}

byte_view!(Bootsector33);

/// `struct extboot`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(non_snake_case)] // the C field names
pub struct Extboot {
    /// `exDriveNumber`: drive number (0x80).
    pub exDriveNumber: u8,
    /// `exReserved1`: reserved.
    pub exReserved1: u8,
    /// `exBootSignature`: ext. boot signature (0x29).
    pub exBootSignature: u8,
    /// `exVolumeID`: volume ID number.
    pub exVolumeID: [u8; 4],
    /// `exVolumeLabel`: volume label.
    pub exVolumeLabel: [u8; 11],
    /// `exFileSysType`: fs type (FAT12 or FAT16).
    pub exFileSysType: [u8; 8],
}

byte_view!(Extboot);

/// `struct bootsector50`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(non_snake_case)] // the C field names
pub struct Bootsector50 {
    /// `bsJump`: jump inst E9xxxx or EBxx90.
    pub bsJump: [u8; 3],
    /// `bsOemName`: OEM name and version.
    pub bsOemName: [u8; 8],
    /// `bsBPB`: BIOS parameter block.
    pub bsBPB: [u8; 25],
    /// `bsExt`: Bootsector Extension.
    pub bsExt: [u8; 26],
    /// `bsBootCode`: pad so structure is 512b.
    pub bsBootCode: [u8; 448],
    /// `bsBootSectSig0`.
    pub bsBootSectSig0: u8,
    /// `bsBootSectSig1`.
    pub bsBootSectSig1: u8,
}

byte_view!(Bootsector50);

/// `struct bootsector710`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(non_snake_case)] // the C field names
pub struct Bootsector710 {
    /// `bsJump`: jump inst E9xxxx or EBxx90.
    pub bsJump: [u8; 3],
    /// `bsOEMName`: OEM name and version.
    pub bsOEMName: [u8; 8],
    /// `bsBPB`: BIOS parameter block.
    pub bsBPB: [u8; 53],
    /// `bsExt`: Bootsector Extension.
    pub bsExt: [u8; 26],
    /// `bsBootCode`: pad so structure is 512b.
    pub bsBootCode: [u8; 418],
    /// `bsBootSectSig2`: 2 & 3 are only defined for FAT32?
    pub bsBootSectSig2: u8,
    /// `bsBootSectSig3`.
    pub bsBootSectSig3: u8,
    /// `bsBootSectSig0`.
    pub bsBootSectSig0: u8,
    /// `bsBootSectSig1`.
    pub bsBootSectSig1: u8,
}

byte_view!(Bootsector710);

/// `union bootsector`.
#[repr(C)]
#[derive(Clone, Copy)]
pub union Bootsector {
    /// `bs33`.
    pub bs33: Bootsector33,
    /// `bs50`.
    pub bs50: Bootsector50,
    /// `bs710`.
    pub bs710: Bootsector710,
}

impl Bootsector {
    /// `bsp->bs33`.
    pub fn bs33(&self) -> &Bootsector33 {
        // SAFETY: every member is 512 bytes of `u8` (checked below), so the bytes of any
        // member are a valid `Bootsector33`.
        unsafe { &self.bs33 }
    }

    /// `bsp->bs50`.
    pub fn bs50(&self) -> &Bootsector50 {
        // SAFETY: as in `bs33`.
        unsafe { &self.bs50 }
    }

    /// `bsp->bs710`.
    pub fn bs710(&self) -> &Bootsector710 {
        // SAFETY: as in `bs33`.
        unsafe { &self.bs710 }
    }
}

byte_view!(Bootsector);

const _: () = {
    assert!(Bootsector33::SIZE == 512);
    assert!(Extboot::SIZE == 26);
    assert!(Bootsector50::SIZE == 512);
    assert!(Bootsector710::SIZE == 512);
    assert!(Bootsector::SIZE == 512);
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signatures_sit_at_the_end_of_the_sector() {
        let mut sector = [0u8; 512];
        sector[510] = BOOTSIG0;
        sector[511] = BOOTSIG1;
        let bsp = Bootsector::at(&sector, 0);
        assert_eq!(bsp.bs33().bsBootSectSig0, BOOTSIG0);
        assert_eq!(bsp.bs50().bsBootSectSig1, BOOTSIG1);
        assert_eq!(bsp.bs710().bsBootSectSig0, BOOTSIG0);
        assert_eq!(bsp.bs710().bsBootSectSig3, 0);
        assert_eq!(core::mem::offset_of!(Bootsector50, bsExt), 36);
        assert_eq!(core::mem::offset_of!(Bootsector710, bsExt), 64);
    }
}
