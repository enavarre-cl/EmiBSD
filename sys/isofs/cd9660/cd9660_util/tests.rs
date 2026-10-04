//! Host tests for ISO 9660 name handling: one character of a plain or Joliet name, the
//! comparison of a path component with a recorded name, and the translation shown to users.

use std::vec::Vec;

use super::*;

/// `isofntrans` into a fresh buffer: the name and the length it reports.
fn trans(infn: &[u8], original: bool, assoc: bool, joliet_level: i32) -> (Vec<u8>, u16) {
    let mut out = [0u8; 300];
    let mut len = 0u16;
    isofntrans(infn, &mut out, &mut len, original, assoc, joliet_level);
    (out[..usize::from(len)].to_vec(), len)
}

/// A Joliet (UCS-2 big-endian) name of ASCII characters.
fn ucs2(s: &[u8]) -> Vec<u8> {
    s.iter().flat_map(|&c| [0, c]).collect()
}

#[test]
fn isochar_reads_plain_and_joliet_characters() {
    let mut c = 0u8;
    assert_eq!(isochar(b"AB", 2, 0, &mut c), 1);
    assert_eq!(c, b'A');
    // Joliet: two bytes, the high one zero for ASCII, '?' otherwise (no Unicode support)
    assert_eq!(isochar(&[0x00, b'x'], 2, 1, &mut c), 2);
    assert_eq!(c, b'x');
    assert_eq!(isochar(&[0x30, 0x42], 2, 2, &mut c), 2);
    assert_eq!(c, b'?');
    // (00) and (01) are one byte in Joliet, too
    assert_eq!(isochar(&[0x01], 1, 1, &mut c), 1);
    assert_eq!(c, 1);
    // past the buffer reads as zero
    assert_eq!(isochar(&[], 0, 0, &mut c), 1);
    assert_eq!(c, 0);
}

#[test]
fn isofncmp_matches_names_without_case_or_version() {
    assert_eq!(isofncmp(b"m10c_iso.txt", b"M10C_ISO.TXT;1", 0), 0);
    assert_eq!(isofncmp(b"M10C_ISO.TXT", b"M10C_ISO.TXT;1", 0), 0);
    // an explicit version must match
    assert_eq!(isofncmp(b"foo.txt;1", b"FOO.TXT;1", 0), 0);
    assert_eq!(isofncmp(b"foo.txt;12", b"FOO.TXT;1", 0), 11);
    assert_eq!(isofncmp(b"foo.txt;x", b"FOO.TXT;1", 0), -1);
    assert_eq!(isofncmp(b"foo.txt.", b"FOO.TXT;1", 0), i32::from(b'.'));
    // a name without extension is recorded with a trailing dot
    assert_eq!(isofncmp(b"foo", b"FOO.;1", 0), 0);
    assert_eq!(isofncmp(b"foo", b"FOO.TXT;1", 0), -1);
    assert_eq!(isofncmp(b"foo", b"FOO", 0), 0);
}

#[test]
fn isofncmp_orders_mismatches_like_the_c() {
    // lowercase input against an uppercase record compares case-insensitively
    assert_eq!(
        isofncmp(b"fop", b"FOO;1", 0),
        i32::from(b'p' - 32) - i32::from(b'O')
    );
    // anything else compares the bytes
    assert_eq!(
        isofncmp(b"F_A", b"F-A;1", 0),
        i32::from(b'_') - i32::from(b'-')
    );
    assert_eq!(isofncmp(b"A", b"b;1", 0), i32::from(b'A') - i32::from(b'b'));
    // the recorded name ran out first: the next input character
    assert_eq!(isofncmp(b"foo.txt", b"FOO", 0), i32::from(b'.'));
    // the input ran out first: minus the next recorded character
    assert_eq!(isofncmp(b"fo", b"FOX;1", 0), -i32::from(b'X'));
}

#[test]
fn isofncmp_reads_joliet_names() {
    assert_eq!(isofncmp(b"foo.txt", &ucs2(b"Foo.txt;1"), 1), 0);
    assert_eq!(isofncmp(b"foo.txt;1", &ucs2(b"foo.txt;1"), 3), 0);
    assert_ne!(isofncmp(b"foo.txu", &ucs2(b"foo.txt;1"), 2), 0);
}

#[test]
fn isofntrans_strips_versions_unless_original() {
    assert_eq!(
        trans(b"M10C_ISO.TXT;1", false, false, 0),
        (b"M10C_ISO.TXT".to_vec(), 12)
    );
    // the dot before the version of a name without extension is dropped too
    assert_eq!(
        trans(b"README.;1", false, false, 0),
        (b"README".to_vec(), 6)
    );
    assert_eq!(
        trans(b"M10C_ISO.TXT;1", true, false, 0),
        (b"M10C_ISO.TXT;1".to_vec(), 14)
    );
    // associated files get a leading '='
    assert_eq!(trans(b"ICON.;1", false, true, 0), (b"=ICON".to_vec(), 5));
    // Joliet
    assert_eq!(
        trans(&ucs2(b"Mixed.case;1"), false, false, 1),
        (b"Mixed.case".to_vec(), 10)
    );
    // no byte is written past the output
    let mut out = [0u8; 4];
    let mut len = 0u16;
    isofntrans(b"LONGNAME", &mut out, &mut len, true, false, 0);
    assert_eq!((&out, len), (b"LONG", 8));
}
