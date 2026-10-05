use super::*;

fn parse(hdr: &[u8]) -> Result<(&[u8], Option<&[u8]>), Errno> {
    exec_script_parse(hdr).map(|l| (l.shellname, l.shellarg))
}

#[test]
fn plain_interpreter() {
    assert_eq!(parse(b"#!/bin/sh\necho hi\n"), Ok((&b"/bin/sh"[..], None)));
}

#[test]
fn blanks_around_the_name_are_skipped() {
    assert_eq!(parse(b"#! \t/bin/sh \t\n"), Ok((&b"/bin/sh"[..], None)));
}

#[test]
fn everything_after_the_name_is_one_argument() {
    assert_eq!(
        parse(b"#!/usr/bin/env  perl -w \nprint 1;\n"),
        Ok((&b"/usr/bin/env"[..], Some(&b"perl -w "[..])))
    );
    assert_eq!(
        parse(b"#!/bin/ksh\t-e\n"),
        Ok((&b"/bin/ksh"[..], Some(&b"-e"[..])))
    );
}

#[test]
fn bare_magic_names_an_empty_interpreter() {
    assert_eq!(parse(b"#!\n"), Ok((&b""[..], None)));
    assert_eq!(parse(b"#!   \n"), Ok((&b""[..], None)));
}

#[test]
fn a_nul_ends_the_line() {
    assert_eq!(parse(b"#!/bin/sh\0 -x\n"), Ok((&b"/bin/sh"[..], None)));
    assert_eq!(
        parse(b"#!/bin/sh -x\0y\n"),
        Ok((&b"/bin/sh"[..], Some(&b"-x"[..])))
    );
}

#[test]
fn not_a_script() {
    assert_eq!(parse(b""), Err(Errno::ENOEXEC));
    assert_eq!(parse(b"#"), Err(Errno::ENOEXEC));
    assert_eq!(parse(b"\x7fELF\x02\x01\x01"), Err(Errno::ENOEXEC));
    assert_eq!(parse(b"# !/bin/sh\n"), Err(Errno::ENOEXEC));
}

#[test]
fn the_newline_must_come_within_maxinterp_bytes() {
    // No newline in the valid header.
    assert_eq!(parse(b"#!/bin/sh"), Err(Errno::ENOEXEC));
    // The newline as the last byte the C looks at (index MAXINTERP - 1) ...
    let mut hdr = std::vec![b'a'; MAXINTERP + 4];
    hdr[..3].copy_from_slice(b"#!/");
    hdr[MAXINTERP - 1] = b'\n';
    let (name, arg) = parse(&hdr).unwrap();
    assert_eq!(name.len(), MAXINTERP - 1 - EXEC_SCRIPT_MAGICLEN);
    assert_eq!(arg, None);
    // ... and one byte later: too long.
    hdr[MAXINTERP - 1] = b'a';
    hdr[MAXINTERP] = b'\n';
    assert_eq!(parse(&hdr), Err(Errno::ENOEXEC));
}

#[test]
fn the_header_is_large_enough_for_a_script() {
    assert!(crate::sys::exec::exec_maxhdrsz() >= crate::sys::exec_script::EXEC_SCRIPT_HDRSZ);
}
