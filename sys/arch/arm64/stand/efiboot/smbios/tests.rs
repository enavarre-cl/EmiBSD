//! SMBIOS parsing on synthetic tables.

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
