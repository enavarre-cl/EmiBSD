/* <LICENSES> */
/*
 * Copyright (c) 1997-1999 Michael Shalayeff
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
 * 3. The name of the author may not be used to endorse or promote products
 *    derived from this software without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL THE AUTHOR OR HIS RELATIVES BE LIABLE FOR ANY DIRECT,
 * INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
 * (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
 * SERVICES; LOSS OF MIND, USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT,
 * STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING
 * IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF
 * THE POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! `<machine/biosvar.h>`: the boot arguments amd64's boot programs hand the kernel (memory
//! map, disks, console, EFI information...), and the BIOS constants beside them.
//!
//! Upstream: sys/arch/amd64/include/biosvar.h @ 3ce1f3f79392
//!
//! The file is self-contained (only `core`): efiboot (`sys/arch/amd64/stand/efiboot`)
//! includes it with `#[path]`, as the C includes the header in both the boot program and the
//! kernel; the kernel's amd64 bootarg entry (M14 track A2) will take it the same way.
//!
//! ## Deviations
//! - The `__packed` structures are `#[repr(C, packed)]` with `as_bytes()` for
//!   `addbootarg`. `struct BIOS_regs`, `DOINT`, `struct bios_attach_args` and the kernel's
//!   `extern` declarations (`bios_sysctl`, `bios_memmap`...) are left to the kernel's use of
//!   this file; `CTL_BIOS_NAMES` is the kernel's sysctl table, not declared here.

#![allow(dead_code)] // a header: each includer uses part of it

/// `BOOTARG_OFF`: some boxes put the APM data segment in the 2nd page.
pub const BOOTARG_OFF: u64 = 4096 * 2;
/// `BOOTARG_LEN`.
pub const BOOTARG_LEN: usize = 4096;
/// `BOOTBIOS_ADDR`.
pub const BOOTBIOS_ADDR: u64 = 0x7c00;
/// `BOOTBIOS_MAXSEC`.
pub const BOOTBIOS_MAXSEC: u32 = (1 << 28) - 1;

/// `BIOSF_BIOS32`.
pub const BIOSF_BIOS32: u32 = 0x0001;
/// `BIOSF_PCIBIOS`.
pub const BIOSF_PCIBIOS: u32 = 0x0002;
/// `BIOSF_PROMSCAN`.
pub const BIOSF_PROMSCAN: u32 = 0x0004;
/// `BIOSF_SMBIOS`.
pub const BIOSF_SMBIOS: u32 = 0x0008;

/// `BIOS_MAP_END`: end of array XXX - special.
pub const BIOS_MAP_END: u32 = 0x00;
/// `BIOS_MAP_FREE`: usable memory.
pub const BIOS_MAP_FREE: u32 = 0x01;
/// `BIOS_MAP_RES`: reserved memory.
pub const BIOS_MAP_RES: u32 = 0x02;
/// `BIOS_MAP_ACPI`: ACPI reclaim memory.
pub const BIOS_MAP_ACPI: u32 = 0x03;
/// `BIOS_MAP_NVS`: ACPI NVS memory.
pub const BIOS_MAP_NVS: u32 = 0x04;

/// `BIOS32_MAKESIG(a, b, c, d)`: a four-character signature as the little-endian word it
/// reads as.
pub const fn bios32_makesig(a: u8, b: u8, c: u8, d: u8) -> u32 {
    (a as u32) | ((b as u32) << 8) | ((c as u32) << 16) | ((d as u32) << 24)
}
/// `SMBIOS_SIGNATURE`: `_SM_`, the anchor of the SMBIOS 2 entry point.
pub const SMBIOS_SIGNATURE: u32 = bios32_makesig(b'_', b'S', b'M', b'_');

/// `BIOS_DEV`, `BIOS_DISKINFO`, `BIOS_CKSUMLEN`, `BIOS_MAXID`: the `CTL_BIOS` sysctl ids.
pub const BIOS_DEV: i32 = 1;
/// `BIOS_DISKINFO`.
pub const BIOS_DISKINFO: i32 = 2;
/// `BIOS_CKSUMLEN`.
pub const BIOS_CKSUMLEN: i32 = 3;
/// `BIOS_MAXID`.
pub const BIOS_MAXID: i32 = 4;

/// Implements `as_bytes()` for the packed boot argument structures.
macro_rules! bootarg_bytes {
    ($($t:ty),*) => {$(
        impl $t {
            /// The structure's bytes, as `addbootarg` copies them.
            pub fn as_bytes(&self) -> &[u8] {
                // SAFETY: the structure is `#[repr(C, packed)]` integers and arrays of
                // integers: no padding, every byte initialised; the slice borrows `self`.
                unsafe {
                    core::slice::from_raw_parts(
                        (self as *const Self).cast::<u8>(),
                        core::mem::size_of::<Self>(),
                    )
                }
            }
        }
    )*};
}

/// `BOOTARG_MEMMAP`.
pub const BOOTARG_MEMMAP: i32 = 0;

/// `bios_memmap_t`: one block of the memory map.
#[repr(C, packed)]
#[derive(Clone, Copy, Default, Debug)]
pub struct BiosMemmap {
    /// `addr`: beginning of block.
    pub addr: u64,
    /// `size`: size of block.
    pub size: u64,
    /// `type`: type of block (`BIOS_MAP_*`).
    pub r#type: u32,
}

/// `BOOTARG_DISKINFO`.
pub const BOOTARG_DISKINFO: i32 = 1;

/// `BDI_INVALID`: I/O error during checksumming.
pub const BDI_INVALID: u32 = 0x0000_0001;
/// `BDI_GOODLABEL`: had SCSI or ST506/ESDI disklabel.
pub const BDI_GOODLABEL: u32 = 0x0000_0002;
/// `BDI_BADLABEL`: had another disklabel.
pub const BDI_BADLABEL: u32 = 0x0000_0004;
/// `BDI_EL_TORITO`: 2,048-byte sectors.
pub const BDI_EL_TORITO: u32 = 0x0000_0008;
/// `BDI_HIBVALID`: hibernate signature valid.
pub const BDI_HIBVALID: u32 = 0x0000_0010;
/// `BDI_PICKED`: kernel-only: cksum matched.
pub const BDI_PICKED: u32 = 0x8000_0000;

/// `bios_diskinfo_t`: what the boot program knows of a disk, and its BSD device.
#[repr(C, packed)]
#[derive(Clone, Copy, Default, Debug)]
pub struct BiosDiskinfo {
    /// `bios_number`: BIOS number of drive (or -1).
    pub bios_number: i32,
    /// `bios_cylinders`.
    pub bios_cylinders: u32,
    /// `bios_heads`.
    pub bios_heads: u32,
    /// `bios_sectors`.
    pub bios_sectors: u32,
    /// `bios_edd`: EDD support.
    pub bios_edd: i32,
    /// `bsd_dev`: BSD device.
    pub bsd_dev: i32,
    /// `checksum`: checksum for drive.
    pub checksum: u32,
    /// `flags`: `BDI_*`.
    pub flags: u32,
}

/// `BOOTARG_APMINFO`.
pub const BOOTARG_APMINFO: i32 = 2;
/// `BOOTARG_CKSUMLEN` (`uint32_t`).
pub const BOOTARG_CKSUMLEN: i32 = 3;
/// `BOOTARG_PCIINFO`.
pub const BOOTARG_PCIINFO: i32 = 4;
/// `BOOTARG_CONSDEV`.
pub const BOOTARG_CONSDEV: i32 = 5;

/// `BCD_MMIO`: memory mapped I/O.
pub const BCD_MMIO: u32 = 0x0000_0001;

/// `bios_consdev_t`: the console.
#[repr(C, packed)]
#[derive(Clone, Copy, Default, Debug)]
pub struct BiosConsdev {
    /// `consdev`.
    pub consdev: i32,
    /// `conspeed`.
    pub conspeed: i32,
    /// `consaddr`.
    pub consaddr: u64,
    /// `consfreq`.
    pub consfreq: i32,
    /// `flags`: `BCD_*`.
    pub flags: u32,
    /// `reg_width`.
    pub reg_width: i32,
    /// `reg_shift`.
    pub reg_shift: i32,
}

/// `BOOTARG_BOOTMAC`.
pub const BOOTARG_BOOTMAC: i32 = 7;

/// `bios_bootmac_t`.
#[repr(C, packed)]
#[derive(Clone, Copy, Default, Debug)]
pub struct BiosBootmac {
    /// `mac`.
    pub mac: [u8; 6],
}

/// `BOOTARG_DDB`.
pub const BOOTARG_DDB: i32 = 8;

/// `bios_ddb_t`.
#[repr(C, packed)]
#[derive(Clone, Copy, Default, Debug)]
pub struct BiosDdb {
    /// `db_console`.
    pub db_console: i32,
}

/// `BOOTARG_BOOTDUID`.
pub const BOOTARG_BOOTDUID: i32 = 9;

/// `bios_bootduid_t`.
#[repr(C, packed)]
#[derive(Clone, Copy, Default, Debug)]
pub struct BiosBootduid {
    /// `duid`.
    pub duid: [u8; 8],
}

/// `BOOTARG_BOOTSR`.
pub const BOOTARG_BOOTSR: i32 = 10;
/// `BOOTSR_UUID_MAX`.
pub const BOOTSR_UUID_MAX: usize = 16;
/// `BOOTSR_CRYPTO_MAXKEYBYTES`.
pub const BOOTSR_CRYPTO_MAXKEYBYTES: usize = 32;

/// `bios_bootsr_t`.
#[repr(C, packed)]
#[derive(Clone, Copy, Default, Debug)]
pub struct BiosBootsr {
    /// `uuid`.
    pub uuid: [u8; BOOTSR_UUID_MAX],
    /// `maskkey`.
    pub maskkey: [u8; BOOTSR_CRYPTO_MAXKEYBYTES],
}

/// `BOOTARG_EFIINFO`.
pub const BOOTARG_EFIINFO: i32 = 11;

/// `BEI_64BIT`: 64-bit EFI implementation.
pub const BEI_64BIT: u32 = 0x0000_0001;
/// `BEI_ESRT`: ESRT table.
pub const BEI_ESRT: u32 = 0x0000_0002;

/// `bios_efiinfo_t`: the EFI system and configuration tables, the frame buffer and the EFI
/// memory map.
#[repr(C, packed)]
#[derive(Clone, Copy, Default, Debug)]
pub struct BiosEfiinfo {
    /// `config_acpi`.
    pub config_acpi: u64,
    /// `config_smbios`.
    pub config_smbios: u64,
    /// `fb_addr`.
    pub fb_addr: u64,
    /// `fb_size`.
    pub fb_size: u64,
    /// `fb_height`.
    pub fb_height: u32,
    /// `fb_width`.
    pub fb_width: u32,
    /// `fb_pixpsl`: pixels per scan line.
    pub fb_pixpsl: u32,
    /// `fb_red_mask`.
    pub fb_red_mask: u32,
    /// `fb_green_mask`.
    pub fb_green_mask: u32,
    /// `fb_blue_mask`.
    pub fb_blue_mask: u32,
    /// `fb_reserved_mask`.
    pub fb_reserved_mask: u32,
    /// `flags`: `BEI_*`.
    pub flags: u32,
    /// `mmap_desc_ver`.
    pub mmap_desc_ver: u32,
    /// `mmap_desc_size`.
    pub mmap_desc_size: u32,
    /// `mmap_size`.
    pub mmap_size: u32,
    /// `mmap_start`.
    pub mmap_start: u64,
    /// `system_table`.
    pub system_table: u64,
    /// `config_esrt`.
    pub config_esrt: u64,
}

/// `BOOTARG_UCODE`.
pub const BOOTARG_UCODE: i32 = 12;

/// `bios_ucode_t`: a CPU microcode update.
#[repr(C, packed)]
#[derive(Clone, Copy, Default, Debug)]
pub struct BiosUcode {
    /// `uc_addr`.
    pub uc_addr: u64,
    /// `uc_size`.
    pub uc_size: u64,
}

bootarg_bytes!(
    BiosMemmap,
    BiosDiskinfo,
    BiosConsdev,
    BiosBootmac,
    BiosDdb,
    BiosBootduid,
    BiosBootsr,
    BiosEfiinfo,
    BiosUcode
);

const _: () = {
    assert!(core::mem::size_of::<BiosMemmap>() == 20);
    assert!(core::mem::size_of::<BiosDiskinfo>() == 32);
    assert!(core::mem::size_of::<BiosConsdev>() == 32);
    assert!(core::mem::size_of::<BiosEfiinfo>() == 100);
    assert!(core::mem::size_of::<BiosUcode>() == 16);
};
/* </CODE> */
