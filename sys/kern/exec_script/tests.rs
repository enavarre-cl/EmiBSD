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

#[test]
fn a_carriage_return_is_part_of_the_name() {
    assert_eq!(parse(b"#!/bin/sh\r\n"), Ok((&b"/bin/sh\r"[..], None)));
}

#[test]
fn only_the_valid_part_of_the_header_counts() {
    // ep_hdrvalid bytes: a newline beyond them is not seen.
    let mut hdr = std::vec![b'a'; crate::sys::exec_script::EXEC_SCRIPT_HDRSZ];
    hdr[..3].copy_from_slice(b"#!/");
    hdr[20] = b'\n';
    assert_eq!(parse(&hdr[..20]), Err(Errno::ENOEXEC));
    assert!(parse(&hdr[..21]).is_ok());
    // A newline at the very end of the header is past MAXINTERP.
    hdr[20] = b'a';
    let last = hdr.len() - 1;
    hdr[last] = b'\n';
    assert_eq!(parse(&hdr), Err(Errno::ENOEXEC));
}

#[test]
fn scripts_come_before_elf_in_the_exec_switch() {
    use crate::kern::kern_exec::EXECSW;
    use crate::sys::exec::{ExecMakecmdsFcn, exec_maxhdrsz};
    // exec_conf.c's order: the script handler first, then ELF.
    assert_eq!(EXECSW.len(), 2);
    assert_eq!(
        EXECSW[0].es_hdrsz,
        crate::sys::exec_script::EXEC_SCRIPT_HDRSZ
    );
    assert!(core::ptr::fn_addr_eq(
        EXECSW[0].es_check,
        exec_script_makecmds as ExecMakecmdsFcn
    ));
    // init_exec: exec_maxhdrsz is the largest es_hdrsz.
    assert_eq!(
        Some(exec_maxhdrsz()),
        EXECSW.iter().map(|e| e.es_hdrsz).max()
    );
}
