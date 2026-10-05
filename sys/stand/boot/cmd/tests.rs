//! boot(8)'s command line: words, abbreviations, `set` variables, flags and the shortcut
//! boot of a bare kernel name.

use super::*;
use crate::boot::{BootMd, boot_md_register};
use crate::vars::bootparse;

static MD: BootMd = BootMd {
    machine: "test",
    version: "0",
    machdep: || {},
    devboot: |_, _| {},
    run_loadfile: |_, _, _| {},
    getsecs: || 0,
    cmd_machine: None,
    check_skip_conf: None,
    mdrandom: None,
    fwrandom: None,
    bootdev_has_hibernate: None,
    stty: None,
};

/// A state with boot device `hd0a` and image `/bsd`, and `line` in the buffer.
fn state(line: &str) -> CmdState {
    boot_md_register(&MD);
    let mut cmd = CmdState::new();
    strlcpy(&mut cmd.bootdev, b"hd0a");
    strlcpy(&mut cmd.image, b"/bsd");
    cmd.buf[..line.len()].copy_from_slice(line.as_bytes());
    cmd
}

#[test]
fn nextword_splits_on_blanks() {
    let mut buf = *b"ls \t /etc\0\0";
    assert_eq!(nextword(&mut buf, 0), Some(5));
    assert_eq!(&buf[..3], b"ls\0");
    assert_eq!(nextword(&mut buf, 5), None);
}

#[test]
fn set_variables_and_flags() {
    let mut cmd = state("set timeout 7");
    assert_eq!(docmd(&mut cmd), 0);
    assert_eq!(cmd.timeout, 7);

    let mut cmd = state("se im /bsd.rd");
    assert_eq!(docmd(&mut cmd), 0);
    assert_eq!(cstr(&cmd.image), b"/bsd.rd");

    let mut cmd = state("set howto -sd");
    assert_eq!(docmd(&mut cmd), 0);
    assert_eq!(
        cmd.boothowto,
        libsa::hdr::reboot::RB_SINGLE | libsa::hdr::reboot::RB_KDB
    );

    let mut cmd = state("set nothing");
    assert_eq!(docmd(&mut cmd), 0); // "set: syntax error"

    let mut cmd = state("# a comment");
    assert_eq!(docmd(&mut cmd), 0);
}

#[test]
fn boot_and_its_shortcut() {
    // `b` abbreviates `boot`, which composes the path from the device and the image
    let mut cmd = state("b -c");
    assert_eq!(docmd(&mut cmd), 1);
    assert_eq!(cstr(&cmd.path), b"hd0a:/bsd");
    assert_eq!(cmd.boothowto, libsa::hdr::reboot::RB_CONFIG);

    // an unknown first word boots it, once
    let mut cmd = state("hd1a:/bsd.sp -s");
    assert_eq!(docmd(&mut cmd), 1);
    assert_eq!(cstr(&cmd.path), b"hd1a:/bsd.sp");
    assert_eq!(cmd.boothowto, libsa::hdr::reboot::RB_SINGLE);
    let mut cmd = state("/bsd");
    assert_eq!(docmd(&mut cmd), 0);

    let mut cmd = state("boot /bsd -x");
    assert_eq!(docmd(&mut cmd), 0); // "howto: bad option: x"
    assert_eq!(bootparse(&mut state("boot -a"), 1), 0);
}
