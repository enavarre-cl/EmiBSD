//! Host tests for `kern_exec.c`: the argument vectors copied in (`copy_strings`) and the
//! new stack `copyargs` lays out (the host double's `copyin`/`copyout` treat user
//! addresses as the test's own memory).

use std::vec::Vec;
use std::{assert_eq, vec};

use super::*;

/// The NUL-terminated string at the address `a`.
fn cstr_at(a: usize) -> Vec<u8> {
    let mut v = Vec::new();
    let mut i = 0;
    loop {
        // SAFETY: the test wrote a NUL-terminated string there.
        let c = unsafe { (a as *const u8).add(i).read() };
        if c == 0 {
            return v;
        }
        v.push(c);
        i += 1;
    }
}

/// The word at the address `a`.
fn word_at(a: usize) -> usize {
    // SAFETY: the test's buffer holds the word.
    unsafe { (a as *const usize).read_unaligned() }
}

#[test]
fn copy_strings_gathers_a_vector() {
    let strings = [&b"/sbin/init\0"[..], b"-s\0"];
    let vector: Vec<usize> = strings
        .iter()
        .map(|s| s.as_ptr() as usize)
        .chain([0])
        .collect();
    let mut argbuf = vec![0u8; ARG_MAX];
    let mut dp = 0;
    assert_eq!(
        copy_strings(vector.as_ptr() as usize, &mut argbuf, &mut dp),
        Ok(2)
    );
    assert_eq!(dp, 11 + 3);
    assert_eq!(&argbuf[..dp], b"/sbin/init\0-s\0");

    // an empty vector
    let empty = [0usize];
    let mut dp2 = dp;
    assert_eq!(
        copy_strings(empty.as_ptr() as usize, &mut argbuf, &mut dp2),
        Ok(0)
    );
    assert_eq!(dp2, dp);
}

#[test]
fn copyargs_lays_out_the_stack() {
    let args = b"init\0-s\0HOME=/\0";
    let mut stack = vec![0usize; 512];
    let base = stack.as_mut_ptr() as usize;
    let mut pack = ExecPackage::new(b"init");
    let mut arginfo = PsStrings {
        ps_nargvstr: 2,
        ps_nenvstr: 1,
        ..PsStrings::default()
    };

    assert!(copyargs(&mut pack, &mut arginfo, base, args));

    const W: usize = size_of::<usize>();
    // argc, then argv[0..2] and NULL, envp[0] and NULL, then the auxiliary vector's room
    assert_eq!(word_at(base), 2);
    assert_eq!(arginfo.ps_argvstr, base + W);
    assert_eq!(arginfo.ps_envstr, base + 4 * W);
    assert_eq!(word_at(base + 3 * W), 0);
    assert_eq!(word_at(base + 5 * W), 0);
    assert_eq!(pack.ep_auxinfo, base + 6 * W);

    // the strings follow the vectors and the ELF_AUX_WORDS words
    let strings = base + W + (2 + 1 + 2 + ELF_AUX_WORDS) * W;
    assert_eq!(word_at(base + W), strings);
    assert_eq!(cstr_at(word_at(base + W)), b"init");
    assert_eq!(cstr_at(word_at(base + 2 * W)), b"-s");
    assert_eq!(cstr_at(word_at(base + 4 * W)), b"HOME=/");
    assert_eq!(word_at(base + 4 * W), strings + 8);
}

#[test]
fn exec_comm_is_the_last_component() {
    let nid_path = b"/sbin/init\0";
    let p = std::boxed::Box::leak(std::boxed::Box::new(Proc::new()));
    let nid = ndinit(LOOKUP, NOFOLLOW, NiDirp::Sys(nid_path), p);
    // no lookup ran: the path's last component, as for a memory image
    assert_eq!(exec_comm(&nid, b"/sbin/init"), b"init");
    assert_eq!(exec_comm(&nid, b"init"), b"init");
}
