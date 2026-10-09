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
 * Copyright (c) 2006 Gordon Willem Klok <gklok@cogeco.ca>
 * Copyright (c) 2019 Mark Kettenis <kettenis@openbsd.org>
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
//! SMBIOS: find the firmware's structure table from its entry point (SMBIOS 2 `_SM_` or 3
//! `_SM3_`) and read the BIOS date and the system's and the board's vendor, product, version
//! and serial strings; `efi_fdt` picks a device tree by vendor and product.
//!
//! Upstream: sys/arch/arm64/stand/efiboot/smbios.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The parts of arm64's `<machine/smbiosvar.h>` it reads (`struct smbios_entry`,
//!   `smbhdr`, `smb3hdr`, `smbtblhdr`, `smbtable`, the `SMBIOS_TYPE_*` it uses and the
//!   string indices of `smbios_struct_bios`, `smbios_sys`, `smbios_board`) are declared
//!   here: the kernel's header is not a dependency of the boot loader.
//! - `struct smbtable`'s pointers are offsets into the structure table, which is read as a
//!   byte slice of `smbios_entry.len` bytes (a string search stops at its end, where the C's
//!   `while (*va++)` reads on).
//! - `hw_vendor`, `hw_prod`, `hw_ver`, `hw_serial` are `Vec`s (the C `alloc()`s them); the
//!   fixed-size strings are `StaticCell`s.
//! - `DPRINTF` (`SMBIOSDEBUG`) is not defined, as in the C: the traces, and the
//!   `fixstring()` calls in their arguments, are not compiled; `smbios_uninfo[]` is read only
//!   by the `#if 0` part of `fixstring`, as in the C.

use alloc::vec::Vec;
use core::ptr;

use libkern::staticcell::StaticCell;

/// `SMBIOS_TYPE_BIOS`.
pub const SMBIOS_TYPE_BIOS: u8 = 0;
/// `SMBIOS_TYPE_SYSTEM`.
pub const SMBIOS_TYPE_SYSTEM: u8 = 1;
/// `SMBIOS_TYPE_BASEBOARD`.
pub const SMBIOS_TYPE_BASEBOARD: u8 = 2;
/// `SMBIOS_TYPE_EOT`.
pub const SMBIOS_TYPE_EOT: u8 = 127;

/// `sizeof(struct smbtblhdr)`.
const SMBTBLHDR_SIZE: usize = 4;

/// The string indices of `struct smbios_struct_bios` (offsets from `tblhdr`): `vendor`,
/// `version`, `release`.
const BIOS_VENDOR: usize = 0;
const BIOS_VERSION: usize = 1;
const BIOS_RELEASE: usize = 4;
/// The string indices of `struct smbios_sys` and `struct smbios_board`: `vendor`,
/// `product`, `version`, `serial`.
const SYS_VENDOR: usize = 0;
const SYS_PRODUCT: usize = 1;
const SYS_VERSION: usize = 2;
const SYS_SERIAL: usize = 3;

/// `smbios_uninfo[]`: what an uninformative string starts with (`fixstring`'s `#if 0`).
#[allow(dead_code)] // read only by the C's `#if 0` block, as in the C
const SMBIOS_UNINFO: [&str; 4] = ["System", "Not ", "To be", "SYS-"];

/// `struct smbios_entry`: the structure table.
#[derive(Clone, Copy)]
pub struct SmbiosEntry {
    /// `mjr`.
    pub mjr: u8,
    /// `min`.
    pub min: u8,
    /// `addr`.
    pub addr: *const u8,
    /// `len`.
    pub len: u16,
    /// `count`.
    pub count: u16,
}

// SAFETY: efiboot is single-threaded; the table is the firmware's, only read.
unsafe impl Send for SmbiosEntry {}

impl SmbiosEntry {
    /// The structure table's bytes.
    fn table(&self) -> &[u8] {
        if self.addr.is_null() {
            return &[];
        }
        // SAFETY: `addr` and `len` come from the firmware's entry point (smbios_init) or a
        // test's buffer: `len` readable bytes that stay in place.
        unsafe { core::slice::from_raw_parts(self.addr, usize::from(self.len)) }
    }
}

/// `struct smbhdr`: the SMBIOS 2 entry point.
#[repr(C, packed)]
#[derive(Clone, Copy)]
struct Smbhdr {
    sig: u32,
    checksum: u8,
    len: u8,
    majrev: u8,
    minrev: u8,
    mss: u16,
    epr: u8,
    fa: [u8; 5],
    sasig: [u8; 5],
    sachecksum: u8,
    size: u16,
    addr: u32,
    count: u16,
    rev: u8,
}

/// `struct smb3hdr`: the SMBIOS 3 entry point.
#[repr(C, packed)]
#[derive(Clone, Copy)]
struct Smb3hdr {
    sig: [u8; 5],
    checksum: u8,
    len: u8,
    majrev: u8,
    minrev: u8,
    docrev: u8,
    epr: u8,
    reserved: u8,
    size: u32,
    addr: u64,
}

/// `struct smbtable`: a structure found by `smbios_find_table`; the C's pointers are
/// offsets into the structure table.
#[derive(Clone, Copy, Default)]
pub struct Smbtable {
    /// `hdr`: the structure's header.
    pub hdr: usize,
    /// `tblhdr`: its formatted area, after the header.
    pub tblhdr: usize,
    /// `cookie`: where to go on looking for the same type.
    pub cookie: u32,
}

/// `smbios_entry`.
pub static SMBIOS_ENTRY: StaticCell<SmbiosEntry> = StaticCell::new(SmbiosEntry {
    mjr: 0,
    min: 0,
    addr: ptr::null(),
    len: 0,
    count: 0,
});

/// `smbios_bios_date[64]`.
pub static SMBIOS_BIOS_DATE: StaticCell<[u8; 64]> = StaticCell::new([0; 64]);
/// `smbios_board_vendor[64]`.
pub static SMBIOS_BOARD_VENDOR: StaticCell<[u8; 64]> = StaticCell::new([0; 64]);
/// `smbios_board_prod[64]`.
pub static SMBIOS_BOARD_PROD: StaticCell<[u8; 64]> = StaticCell::new([0; 64]);
/// `smbios_board_serial[64]`.
pub static SMBIOS_BOARD_SERIAL: StaticCell<[u8; 64]> = StaticCell::new([0; 64]);

/// `hw_vendor`.
pub static HW_VENDOR: StaticCell<Option<Vec<u8>>> = StaticCell::new(None);
/// `hw_prod`.
pub static HW_PROD: StaticCell<Option<Vec<u8>>> = StaticCell::new(None);
/// `hw_ver`.
pub static HW_VER: StaticCell<Option<Vec<u8>>> = StaticCell::new(None);
/// `hw_serial`.
pub static HW_SERIAL: StaticCell<Option<Vec<u8>>> = StaticCell::new(None);

/// `smbios_init(smbios)`: read the entry point at `smbios` (the firmware's
/// `SMBIOS_TABLE_GUID` or `SMBIOS3_TABLE_GUID` table, may be null) and the strings.
///
/// # Safety
///
/// `smbios` is null or the firmware's entry point, followed by the structure table it
/// names; both readable.
pub unsafe fn smbios_init(smbios: *const u8) {
    if smbios.is_null() {
        return;
    }

    // SAFETY: the caller's contract: an entry point (at least 5 bytes of signature).
    let sig = unsafe { core::slice::from_raw_parts(smbios, 5) };
    // SAFETY: single-threaded; the only reference.
    let entry = unsafe { SMBIOS_ENTRY.get_mut() };
    let addr: u64 = if sig[..4] == *b"_SM_" {
        // SAFETY: an SMBIOS 2 entry point (`len` is checked against its size below).
        let hdr = unsafe { smbios.cast::<Smbhdr>().read_unaligned() };

        if usize::from(hdr.len) != core::mem::size_of::<Smbhdr>() {
            return;
        }
        // SAFETY: `hdr.len` bytes of the entry point.
        let bytes = unsafe { core::slice::from_raw_parts(smbios, usize::from(hdr.len)) };
        if bytes.iter().fold(0u8, |a, &b| a.wrapping_add(b)) != 0 {
            return;
        }

        entry.len = hdr.size;
        entry.mjr = hdr.majrev;
        entry.min = hdr.minrev;
        entry.count = hdr.count;

        u64::from(hdr.addr)
    } else if sig == *b"_SM3_" {
        // SAFETY: an SMBIOS 3 entry point (`len` is checked against its size below).
        let hdr = unsafe { smbios.cast::<Smb3hdr>().read_unaligned() };

        if usize::from(hdr.len) != core::mem::size_of::<Smb3hdr>() || hdr.epr != 0x01 {
            return;
        }
        // SAFETY: `hdr.len` bytes of the entry point.
        let bytes = unsafe { core::slice::from_raw_parts(smbios, usize::from(hdr.len)) };
        if bytes.iter().fold(0u8, |a, &b| a.wrapping_add(b)) != 0 {
            return;
        }

        entry.len = hdr.size as u16;
        entry.mjr = hdr.majrev;
        entry.min = hdr.minrev;
        entry.count = 0xffff; // -1

        hdr.addr
    } else {
        // Unsupported SMBIOS entry point
        return;
    };

    entry.addr = addr as usize as *const u8;
    let entry = *entry;

    let mut bios = Smbtable::default();
    if smbios_find_table(&entry, SMBIOS_TYPE_BIOS, &mut bios) {
        let sb = bios.tblhdr;
        let mut scratch = [0u8; 64];
        let _ = smbios_get_string(&entry, &bios, field(&entry, sb + BIOS_VENDOR), &mut scratch);
        let _ = smbios_get_string(
            &entry,
            &bios,
            field(&entry, sb + BIOS_VERSION),
            &mut scratch,
        );
        if smbios_get_string(
            &entry,
            &bios,
            field(&entry, sb + BIOS_RELEASE),
            &mut scratch,
        ) && let Some(sminfop) = fixstring(&mut scratch)
        {
            // SAFETY: single-threaded; the only reference.
            libkern::strlcpy::strlcpy(unsafe { SMBIOS_BIOS_DATE.get_mut() }, sminfop);
        }

        smbios_info(&entry);
    }
}

/// The byte at `off` of the structure table (0 past its end).
fn field(entry: &SmbiosEntry, off: usize) -> u8 {
    entry.table().get(off).copied().unwrap_or(0)
}

/// `smbios_find_table(type, st)`: the next structure of type `ty` after the one `st`
/// found (the first when `st.cookie` is 0); true if there is one.
///
/// The cookie field of the smbtable structure is used to locate multiple instances of a
/// table of an arbitrary type. Following the successful location of a table, the type is
/// encoded as bits 0:7 of the cookie value, the offset in terms of the number of structures
/// preceding that referenced by the handle is encoded in bits 15:31.
pub fn smbios_find_table(entry: &SmbiosEntry, ty: u8, st: &mut Smbtable) -> bool {
    let t = entry.table();
    let end = t.len();
    let mut va = 0usize;
    let mut ret = false;
    let mut tcount: u32 = 1;
    let at = |i: usize| t.get(i).copied().unwrap_or(0);

    if (st.cookie & 0xfff) == u32::from(ty) && st.cookie >> 16 != 0 && st.hdr < end {
        let hdr = st.hdr;
        if at(hdr) == ty {
            va = hdr + usize::from(at(hdr + 1));
            while va + 1 < end {
                if at(va) == 0 && at(va + 1) == 0 {
                    break;
                }
                va += 1;
            }
            va += 2;
            tcount = st.cookie >> 16;
        }
    }
    while va + SMBTBLHDR_SIZE < end && tcount <= u32::from(entry.count) {
        let hdr = va;
        if at(hdr) == ty {
            ret = true;
            st.hdr = hdr;
            st.tblhdr = va + SMBTBLHDR_SIZE;
            st.cookie = ((tcount + 1) << 16) | u32::from(ty);
            break;
        }
        if at(hdr) == SMBIOS_TYPE_EOT {
            break;
        }
        va += usize::from(at(hdr + 1));
        while va + 1 < end {
            if at(va) == 0 && at(va + 1) == 0 {
                break;
            }
            va += 1;
        }
        va += 2;
        tcount += 1;
    }
    ret
}

/// `smbios_get_string(st, indx, dest, len)`: string `indx` of the structure `st` into
/// `dest` (the whole of it, NUL-terminated); false (the C's NULL) if there is none or it is
/// too near the end of the table.
pub fn smbios_get_string(entry: &SmbiosEntry, st: &Smbtable, indx: u8, dest: &mut [u8]) -> bool {
    let t = entry.table();
    let end = t.len();
    let len = dest.len();
    let mut va = st.hdr + usize::from(t.get(st.hdr + 1).copied().unwrap_or(0));
    let mut i = 1;
    while va < end && i < indx && t[va] != 0 {
        while va < end && t[va] != 0 {
            va += 1;
        }
        va += 1;
        i += 1;
    }
    if i == indx && va + len < end {
        dest.copy_from_slice(&t[va..va + len]);
        dest[len - 1] = 0;
        return true;
    }
    false
}

/// `fixstring(s)`: remove leading and trailing blanks, in place; the string, or `None` if
/// it was all blanks.
pub fn fixstring(s: &mut [u8]) -> Option<&[u8]> {
    let len = s.iter().position(|&c| c == 0).unwrap_or(s.len());
    let p = s[..len].iter().take_while(|&&c| c == b' ').count();
    // Special case entire string is whitespace
    if p == len {
        return None;
    }
    let mut e = len - 1;
    while e > 0 && s[e] == b' ' {
        e -= 1;
    }
    if p > 0 || e < len - 1 {
        s.copy_within(p..=e, 0);
        if e - p + 1 < s.len() {
            s[e - p + 1] = 0;
        }
    }
    let n = s.iter().position(|&c| c == 0).unwrap_or(s.len());
    Some(&s[..n])
}

/// The string `indx` of `st`, fixed, as a `Vec` (the C's `alloc()` and `strlcpy()`).
fn get_fixed(entry: &SmbiosEntry, st: &Smbtable, indx: u8) -> Option<Vec<u8>> {
    let mut sminfo = [0u8; 64];
    if !smbios_get_string(entry, st, indx, &mut sminfo) {
        return None;
    }
    fixstring(&mut sminfo).map(<[u8]>::to_vec)
}

/// `smbios_info()`: the system's (else the board's) vendor and product, its version and
/// serial, and the board's strings.
pub fn smbios_info(entry: &SmbiosEntry) {
    if entry.mjr < 2 {
        return;
    }
    // According to the spec the system table among others is required, if it is not we do
    // not bother with this smbios implementation.
    let (mut stbl, mut btbl) = (Smbtable::default(), Smbtable::default());
    if !smbios_find_table(entry, SMBIOS_TYPE_SYSTEM, &mut stbl) {
        return;
    }
    let havebb = smbios_find_table(entry, SMBIOS_TYPE_BASEBOARD, &mut btbl);

    let sys = stbl.tblhdr;
    let board = btbl.tblhdr;
    // SAFETY: single-threaded; each static is referenced once at a time below.
    unsafe {
        if havebb {
            if let Some(s) = get_fixed(entry, &btbl, field(entry, board + SYS_VENDOR)) {
                libkern::strlcpy::strlcpy(SMBIOS_BOARD_VENDOR.get_mut(), &s);
            }
            if let Some(s) = get_fixed(entry, &btbl, field(entry, board + SYS_PRODUCT)) {
                libkern::strlcpy::strlcpy(SMBIOS_BOARD_PROD.get_mut(), &s);
            }
            if let Some(s) = get_fixed(entry, &btbl, field(entry, board + SYS_SERIAL)) {
                libkern::strlcpy::strlcpy(SMBIOS_BOARD_SERIAL.get_mut(), &s);
            }
        }

        // Some smbios implementations have no system vendor or product strings, some have
        // very uninformative data which is harder to work around and we must rely upon
        // various heuristics to detect this. In both cases we attempt to fall back on the
        // base board information in the perhaps naive belief that motherboard vendors will
        // supply this information.
        let mut sminfop = get_fixed(entry, &stbl, field(entry, sys + SYS_VENDOR));
        if sminfop.is_none() && havebb {
            sminfop = get_fixed(entry, &btbl, field(entry, board + SYS_VENDOR));
        }
        if sminfop.is_some() {
            HW_VENDOR.write(sminfop);
        }
        let mut sminfop = get_fixed(entry, &stbl, field(entry, sys + SYS_PRODUCT));
        if sminfop.is_none() && havebb {
            sminfop = get_fixed(entry, &btbl, field(entry, board + SYS_PRODUCT));
        }
        if sminfop.is_some() {
            HW_PROD.write(sminfop);
        }
        if let Some(s) = get_fixed(entry, &stbl, field(entry, sys + SYS_VERSION)) {
            HW_VER.write(Some(s));
        }
        if let Some(s) = get_fixed(entry, &stbl, field(entry, sys + SYS_SERIAL)) {
            HW_SERIAL.write(Some(s));
        }
    }
}

const _: () = assert!(core::mem::size_of::<Smbhdr>() == 31);
const _: () = assert!(core::mem::size_of::<Smb3hdr>() == 24);
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // SMBIOS parsing on synthetic tables.

    use super::*;
    use std::vec;
    use std::vec::Vec;

    /// A structure: header (type, size, handle), the formatted area, the strings.
    fn structure(ty: u8, handle: u16, formatted: &[u8], strings: &[&str]) -> Vec<u8> {
        let mut s = vec![ty, (4 + formatted.len()) as u8];
        s.extend_from_slice(&handle.to_le_bytes());
        s.extend_from_slice(formatted);
        for st in strings {
            s.extend_from_slice(st.as_bytes());
            s.push(0);
        }
        if strings.is_empty() {
            s.push(0);
        }
        s.push(0);
        s
    }

    /// A table like QEMU's: BIOS, system, board, two memory devices, end of table, and the room
    /// `smbios_get_string`'s 64-byte copies need.
    fn table() -> Vec<u8> {
        let mut t = Vec::new();
        // vendor 1, version 2, start 0xe800, release 3
        t.extend(structure(
            0,
            0,
            &[1, 2, 0x00, 0xe8, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            &["EFI Development Kit II / OVMF", "0.0.0", "  02/06/2015 "],
        ));
        // vendor 1, product 2, version 3, serial 4 (blank: no serial)
        t.extend(structure(
            1,
            0x100,
            &[
                1, 2, 3, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            ],
            &["QEMU", "QEMU Virtual Machine", "virt-9.2", "   "],
        ));
        t.extend(structure(
            2,
            0x200,
            &[1, 2, 3, 0],
            &[" Board Co", "B1 ", "1.0"],
        ));
        t.extend(structure(17, 0x1100, &[0; 8], &[]));
        t.extend(structure(17, 0x1101, &[1; 8], &[]));
        t.extend(structure(SMBIOS_TYPE_EOT, 0xfeff, &[], &[]));
        t.resize(t.len() + 64, 0);
        t
    }

    /// An SMBIOS 3 entry point for `t`.
    fn entry_point(t: &[u8]) -> Vec<u8> {
        let mut ep = b"_SM3_".to_vec();
        ep.extend_from_slice(&[0, 24, 3, 3, 0, 1, 0]); // checksum, len, 3.3.0, epr 1
        ep.extend_from_slice(&(t.len() as u32).to_le_bytes());
        ep.extend_from_slice(&(t.as_ptr() as u64).to_le_bytes());
        let sum = ep.iter().fold(0u8, |a, &b| a.wrapping_add(b));
        ep[5] = sum.wrapping_neg();
        ep
    }

    fn entry(t: &[u8]) -> SmbiosEntry {
        SmbiosEntry {
            mjr: 3,
            min: 3,
            addr: t.as_ptr(),
            len: t.len() as u16,
            count: 0xffff,
        }
    }

    #[test]
    fn find_table_walks_and_resumes() {
        let t = table();
        let e = entry(&t);
        let mut st = Smbtable::default();
        assert!(smbios_find_table(&e, 17, &mut st));
        assert_eq!(t[st.hdr + 2], 0x00);
        assert_eq!(st.tblhdr, st.hdr + 4);
        let first = st.hdr;
        assert!(smbios_find_table(&e, 17, &mut st));
        assert_eq!((t[st.hdr + 2], t[st.hdr + 3]), (0x01, 0x11));
        assert!(st.hdr > first);
        assert!(!smbios_find_table(&e, 17, &mut st));
        assert!(!smbios_find_table(&e, 4, &mut Smbtable::default()));
        // a count limit stops the walk
        let e = SmbiosEntry { count: 1, ..e };
        assert!(!smbios_find_table(
            &e,
            SMBIOS_TYPE_SYSTEM,
            &mut Smbtable::default()
        ));
    }

    #[test]
    fn strings_and_fixstring() {
        let t = table();
        let e = entry(&t);
        let mut st = Smbtable::default();
        assert!(smbios_find_table(&e, SMBIOS_TYPE_SYSTEM, &mut st));
        let mut s = [0u8; 64];
        assert!(smbios_get_string(&e, &st, 2, &mut s));
        assert!(s.starts_with(b"QEMU Virtual Machine\0"));
        // one past the last string is the empty string before the table's double NUL, as in C
        assert!(smbios_get_string(&e, &st, 5, &mut s));
        assert_eq!(s[0], 0);
        assert!(!smbios_get_string(&e, &st, 6, &mut s));
        let mut b = *b"  two words  \0xx";
        assert_eq!(fixstring(&mut b), Some(&b"two words"[..]));
        let mut b = *b"   \0";
        assert_eq!(fixstring(&mut b), None);
        let mut b = *b"\0";
        assert_eq!(fixstring(&mut b), None);
        let mut b = *b"plain\0";
        assert_eq!(fixstring(&mut b), Some(&b"plain"[..]));
    }

    #[test]
    fn init_reads_the_strings() {
        let t = table();
        let ep = entry_point(&t);
        let mut bad = ep.clone();
        bad[10] ^= 1; // checksum
        // SAFETY: entry points followed by `t`, which outlives the calls; this test alone
        // touches the statics.
        unsafe {
            smbios_init(bad.as_ptr());
            assert!(HW_VENDOR.get().is_none());
            smbios_init(ep.as_ptr());
            assert_eq!(SMBIOS_ENTRY.get().count, 0xffff);
            assert_eq!(HW_VENDOR.get().as_deref(), Some(&b"QEMU"[..]));
            assert_eq!(HW_PROD.get().as_deref(), Some(&b"QEMU Virtual Machine"[..]));
            assert_eq!(HW_VER.get().as_deref(), Some(&b"virt-9.2"[..]));
            assert_eq!(HW_SERIAL.get().as_deref(), None);
            assert!(SMBIOS_BIOS_DATE.get().starts_with(b"02/06/2015\0"));
            assert!(SMBIOS_BOARD_VENDOR.get().starts_with(b"Board Co\0"));
            assert!(SMBIOS_BOARD_PROD.get().starts_with(b"B1\0"));
        }
    }
}
/* </TESTS> */
