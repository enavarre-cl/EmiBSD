//! Host tests for `kern_sysctl.rs`: the helpers' C semantics on old and new buffers (sizes,
//! `ENOMEM`/`EPERM`/`EINVAL`, truncation, bounds), the bounded tables, the identity nodes,
//! `hw` nodes that need no hardware, `kern.proc` sizing and `fill_kproc` over a process built
//! by hand, and `sysctl(2)` itself through its argument registers.

use std::assert_eq;
use std::boxed::Box;
use std::sync::MutexGuard;

use super::*;
use crate::kern::kern_proc::procinit;
use crate::kern::kern_prot::{crget, crhold};
use crate::kern::subr_pool::tests::setup_real_memory;
use crate::sys::proc::{Pgrp, Session};
use crate::sys::time::{Clockinfo, Timeval};
use crate::sys::ucred::Ucred;

/// A user address for a buffer of the test.
fn ua<T>(b: &mut T) -> usize {
    b as *mut T as usize
}

/// A user address of a read-only value.
fn ra<T>(b: &T) -> usize {
    b as *const T as usize
}

#[test]
fn rdint_sizes_reads_and_refuses_writes() {
    let mut len = 0;
    assert_eq!(sysctl_rdint(0, &mut len, 0, 42), Ok(()));
    assert_eq!(len, 4);

    let mut out = 0i32;
    let mut len = 3;
    assert_eq!(
        sysctl_rdint(ua(&mut out), &mut len, 0, 42),
        Err(Errno::ENOMEM)
    );
    assert_eq!(len, 3);

    let mut len = 4;
    assert_eq!(sysctl_rdint(ua(&mut out), &mut len, 0, 42), Ok(()));
    assert_eq!(out, 42);

    let new = 7i32;
    assert_eq!(sysctl_rdint(0, &mut len, ra(&new), 42), Err(Errno::EPERM));
}

#[test]
fn int_swaps_the_old_value_out() {
    let var = AtomicI32::new(5);
    let mut out = 0i32;
    let mut len = 4;
    let new = -9i32;
    assert_eq!(
        sysctl_int(ua(&mut out), &mut len, ra(&new), 4, &var),
        Ok(())
    );
    assert_eq!((out, var.load(Ordering::Relaxed), len), (5, -9, 4));

    // A new value of the wrong size is EINVAL, and nothing changes.
    assert_eq!(
        sysctl_int(0, &mut len, ra(&new), 2, &var),
        Err(Errno::EINVAL)
    );
    // No old buffer: the value is just stored.
    let new = 11i32;
    assert_eq!(sysctl_int(0, &mut len, ra(&new), 4, &var), Ok(()));
    assert_eq!(var.load(Ordering::Relaxed), 11);
}

#[test]
fn int_bounded_checks_both_bounds_and_read_only() {
    let var = AtomicI32::new(50);
    let mut len = 4;
    let too_big = 101i32;
    let too_small = -1i32;
    let fine = 100i32;
    assert_eq!(
        sysctl_int_bounded(0, &mut len, ra(&too_big), 4, &var, 0, 100),
        Err(Errno::EINVAL)
    );
    assert_eq!(
        sysctl_int_bounded(0, &mut len, ra(&too_small), 4, &var, 0, 100),
        Err(Errno::EINVAL)
    );
    assert_eq!(var.load(Ordering::Relaxed), 50);
    assert_eq!(
        sysctl_int_bounded(0, &mut len, ra(&fine), 4, &var, 0, 100),
        Ok(())
    );
    assert_eq!(var.load(Ordering::Relaxed), 100);

    // minimum > maximum: read-only, refused before anything is looked at.
    let (min, max) = SYSCTL_INT_READONLY;
    let mut len = 0;
    assert_eq!(
        sysctl_int_bounded(0, &mut len, ra(&fine), 4, &var, min, max),
        Err(Errno::EPERM)
    );
    let mut out = 0i32;
    let mut len = 4;
    assert_eq!(
        sysctl_int_bounded(ua(&mut out), &mut len, 0, 0, &var, min, max),
        Ok(())
    );
    assert_eq!(out, 100);
}

#[test]
fn int_lower_never_raises() {
    let var = AtomicI32::new(3);
    let mut len = 4;
    let higher = 4i32;
    assert_eq!(
        sysctl_int_lower(0, &mut len, ra(&higher), 4, &var),
        Err(Errno::EPERM)
    );
    let lower = 1i32;
    let mut out = 0i32;
    assert_eq!(
        sysctl_int_lower(ua(&mut out), &mut len, ra(&lower), 4, &var),
        Ok(())
    );
    assert_eq!((out, var.load(Ordering::Relaxed)), (3, 1));
}

#[test]
fn rdquad_is_eight_bytes() {
    let mut out = 0i64;
    let mut len = 7;
    assert_eq!(
        sysctl_rdquad(ua(&mut out), &mut len, 0, 1 << 40),
        Err(Errno::ENOMEM)
    );
    let mut len = 8;
    assert_eq!(sysctl_rdquad(ua(&mut out), &mut len, 0, 1 << 40), Ok(()));
    assert_eq!(out, 1 << 40);
}

#[test]
fn strings_count_the_nul_and_refuse_short_buffers() {
    let mut out = [0xffu8; 16];
    let mut len = 0;
    assert_eq!(sysctl_rdstring(0, &mut len, 0, b"EmiBSD"), Ok(()));
    assert_eq!(len, 7);

    let mut len = 6;
    assert_eq!(
        sysctl_rdstring(ua(&mut out), &mut len, 0, b"EmiBSD"),
        Err(Errno::ENOMEM)
    );
    let mut len = out.len();
    assert_eq!(
        sysctl_rdstring(ua(&mut out), &mut len, 0, b"EmiBSD\0junk"),
        Ok(())
    );
    assert_eq!((len, &out[..8]), (7, &b"EmiBSD\0\xff"[..]));

    let new = *b"x\0";
    assert_eq!(
        sysctl_rdstring(0, &mut len, ra(&new), b"EmiBSD"),
        Err(Errno::EPERM)
    );
}

#[test]
fn string_reads_then_writes_and_terminates() {
    let mut var = [0u8; 8];
    var[..3].copy_from_slice(b"old");
    let new = *b"newname";
    let mut out = [0xffu8; 8];
    let mut len = out.len();
    assert_eq!(
        sysctl_string(ua(&mut out), &mut len, ra(&new), 4, &mut var),
        Ok(())
    );
    assert_eq!((len, &out[..4]), (4, &b"old\0"[..]));
    assert_eq!(&var[..5], b"newn\0");

    // newlen must leave room for the NUL.
    assert_eq!(
        sysctl_string(0, &mut len, ra(&new), 8, &mut var),
        Err(Errno::EINVAL)
    );
    // A short old buffer is ENOMEM for sysctl_string...
    let mut len = 2;
    assert_eq!(
        sysctl_string(ua(&mut out), &mut len, 0, 0, &mut var),
        Err(Errno::ENOMEM)
    );
}

#[test]
fn tstring_truncates_with_a_nul() {
    let mut var = [0u8; 16];
    var[..8].copy_from_slice(b"hostname");
    let mut out = [0xffu8; 8];
    let mut len = 5;
    assert_eq!(
        sysctl_tstring(ua(&mut out), &mut len, 0, 0, &mut var),
        Ok(())
    );
    assert_eq!((len, &out[..6]), (5, &b"host\0\xff"[..]));

    let mut len = 0;
    assert_eq!(
        sysctl_tstring(ua(&mut out), &mut len, 0, 0, &mut var),
        Err(Errno::ENOMEM)
    );
}

#[test]
fn structs_copy_whole_and_rdstruct_is_read_only() {
    let tv = Timeval {
        tv_sec: 1,
        tv_usec: 2,
    };
    let mut out = Timeval::default();
    let mut len = 15;
    assert_eq!(
        sysctl_rdstruct(ua(&mut out), &mut len, 0, tv.as_bytes()),
        Err(Errno::ENOMEM)
    );
    let mut len = 16;
    assert_eq!(
        sysctl_rdstruct(ua(&mut out), &mut len, 0, tv.as_bytes()),
        Ok(())
    );
    assert_eq!(out, tv);
    assert_eq!(
        sysctl_rdstruct(0, &mut len, ra(&tv), tv.as_bytes()),
        Err(Errno::EPERM)
    );

    let mut var = Clockinfo::default();
    let new = Clockinfo {
        hz: 100,
        tick: 10_000,
        stathz: 128,
        profhz: 1024,
    };
    let mut len = 0;
    assert_eq!(
        sysctl_struct(0, &mut len, ra(&new), 17, var.as_bytes_mut()),
        Err(Errno::EINVAL)
    );
    assert_eq!(
        sysctl_struct(0, &mut len, ra(&new), 16, var.as_bytes_mut()),
        Ok(())
    );
    assert_eq!(var, new);
}

#[test]
fn bounded_arr_finds_the_mib() {
    static A: AtomicI32 = AtomicI32::new(7);
    let table = [SysctlBoundedArgs::readonly(3, &A)];
    let mut out = 0i32;
    let mut len = 4;
    assert_eq!(
        sysctl_bounded_arr(&table, &[3, 1], 0, &mut len, 0, 0),
        Err(Errno::ENOTDIR)
    );
    assert_eq!(
        sysctl_bounded_arr(&table, &[4], 0, &mut len, 0, 0),
        Err(Errno::EOPNOTSUPP)
    );
    assert_eq!(
        sysctl_bounded_arr(&table, &[3], ua(&mut out), &mut len, 0, 0),
        Ok(())
    );
    assert_eq!(out, 7);
}

/// A thread of a fresh process with credentials `uid`, in its own session and process
/// group, as `fork1` would leave it.
fn thread(uid: u32) -> &'static Proc {
    let cr: &'static Ucred = crget();
    cr.cr_uid.set(uid);
    cr.cr_ruid.set(uid);
    let pr: &'static Process = Box::leak(Box::new(Process::new()));
    let p: &'static Proc = Box::leak(Box::new(Proc::new()));
    let pg: &'static Pgrp = Box::leak(Box::new(Pgrp::new()));
    let sess: &'static Session = Box::leak(Box::new(Session::new()));
    pg.pg_session.set(sess);
    pg.pg_id.set(42);
    sess.s_leader.set(pr);
    p.p_p.set(pr);
    p.p_ucred.set(cr);
    pr.ps_ucred.set(crhold(cr));
    pr.ps_mainproc.set(p);
    pr.ps_pgrp.set(pg);
    pr.ps_pid.set(42);
    pr.set_comm(b"test");
    p
}

fn setup() -> MutexGuard<'static, ()> {
    let guard = setup_real_memory();
    crate::machine::cons::consinit();
    procinit();
    guard
}

#[test]
fn the_kernel_reports_emibsd_7_8() {
    let _g = setup();
    let p = thread(1000);

    let mut out = [0u8; 32];
    let mut len = out.len();
    assert_eq!(
        kern_sysctl(&[KERN_OSTYPE], ua(&mut out), &mut len, 0, 0, p),
        Ok(())
    );
    assert_eq!(&out[..len], b"EmiBSD\0");

    let mut len = out.len();
    assert_eq!(
        kern_sysctl(&[KERN_OSRELEASE], ua(&mut out), &mut len, 0, 0, p),
        Ok(())
    );
    assert_eq!(&out[..len], b"8.0\0");

    let mut len = 0;
    assert_eq!(kern_sysctl(&[KERN_VERSION], 0, &mut len, 0, 0, p), Ok(()));
    assert_eq!(len, VERSION.len() + 1);

    // A read-only node refuses a new value; a missing node is EOPNOTSUPP.
    let new = *b"OpenBSD\0";
    assert_eq!(
        kern_sysctl(&[KERN_OSTYPE], 0, &mut len, ra(&new), 8, p),
        Err(Errno::EPERM)
    );
    let mut v = 0i32;
    let mut len = 4;
    assert_eq!(
        kern_sysctl(&[KERN_ARGMAX], ua(&mut v), &mut len, 0, 0, p),
        Ok(())
    );
    assert_eq!(v, 512 * 1024);
    assert_eq!(
        kern_sysctl(&[KERN_OSREV], ua(&mut v), &mut len, 0, 0, p),
        Ok(())
    );
    assert_eq!(v, OpenBSD as i32);
    assert_eq!(
        kern_sysctl(&[KERN_GLOBAL_PTRACE], ua(&mut v), &mut len, 0, 0, p),
        Err(Errno::EOPNOTSUPP)
    );
    let one = 1i32;
    assert_eq!(
        kern_sysctl(&[KERN_SAVED_IDS], 0, &mut len, ra(&one), 4, p),
        Err(Errno::EPERM)
    );
}

#[test]
fn hw_nodes_without_hardware() {
    let _g = setup();
    let p = thread(0);

    let mut out = [0u8; 16];
    let mut len = out.len();
    assert_eq!(
        hw_sysctl(&[HW_MACHINE], ua(&mut out), &mut len, 0, 0, p),
        Ok(())
    );
    assert_eq!(&out[..len], b"host\0");

    let mut v = 0i32;
    let mut len = 4;
    assert_eq!(
        hw_sysctl(&[HW_BYTEORDER], ua(&mut v), &mut len, 0, 0, p),
        Ok(())
    );
    assert_eq!(v, 1234);
    assert_eq!(
        hw_sysctl(&[HW_NCPU, 1], 0, &mut len, 0, 0, p),
        Err(Errno::ENOTDIR)
    );
    assert_eq!(
        hw_sysctl(&[HW_VENDOR], ua(&mut v), &mut len, 0, 0, p),
        Err(Errno::EOPNOTSUPP)
    );

    let mut q = 0i64;
    let mut len = 8;
    assert_eq!(
        hw_sysctl(&[HW_PHYSMEM64], ua(&mut q), &mut len, 0, 0, p),
        Ok(())
    );
    assert_eq!(q, (PHYSMEM.load(Ordering::Relaxed) * PAGE_SIZE) as i64);
}

#[test]
fn doproc_sizes_the_answer_and_checks_its_arguments() {
    let _g = setup();
    let size = size_of::<KinfoProc>() as i32;

    let mut len = 0;
    assert_eq!(
        sysctl_doproc(&[KERN_PROC_ALL, 0, size], 0, &mut len),
        Err(Errno::EINVAL)
    );
    assert_eq!(
        sysctl_doproc(&[KERN_PROC_ALL, 0, size + 1, 1], 0, &mut len),
        Err(Errno::EINVAL)
    );

    // No process has this pid: only the slop is asked for.
    assert_eq!(
        sysctl_doproc(&[KERN_PROC_PID, 999_999, size, 10], 0, &mut len),
        Ok(())
    );
    assert_eq!(len, KERN_PROCSLOP * size as usize);
}

#[test]
fn fill_kproc_reports_a_process() {
    let _g = setup();
    let p = thread(1000);
    let pr = p.process();

    let mut ki = KinfoProc::zeroed();
    fill_kproc(pr, &mut ki, None, false);
    assert_eq!((ki.p_pid, ki.p__pgid, ki.p_sid, ki.p_tid), (42, 42, 42, -1));
    assert_eq!((ki.p_uid, ki.p_ruid), (1000, 1000));
    assert_eq!(&ki.p_comm[..5], b"test\0");
    assert_eq!(&ki.p_emul[..7], b"native\0");
    assert_eq!((ki.p_tdev, ki.p_tpgid), (NODEV as u32, -1));
    assert_eq!((ki.p_cpuid, ki.p_uvalid, ki.p_paddr), (KI_NOCPU, 1, 0));
    assert_eq!(ki.p_stat, SIDL as i8);

    // As a thread: its tid and name.
    p.p_tid.set(7);
    p.set_name(b"worker");
    fill_kproc(pr, &mut ki, Some(p), true);
    assert_eq!(ki.p_tid, 7 + THREAD_PID_OFFSET);
    assert_eq!(&ki.p_name[..7], b"worker\0");
    assert_eq!(ki.p_paddr, p as *const Proc as u64);
}

#[test]
fn sysctl_2_reads_kern_ostype() {
    let _g = setup();
    let p = thread(1000);

    let name = [CTL_KERN, KERN_OSTYPE];
    let mut out = [0u8; 16];
    let mut oldlen = out.len();
    let mut retval = [0; 2];
    let args = [
        ra(&name) as Register,
        2,
        ua(&mut out) as Register,
        ua(&mut oldlen) as Register,
        0,
        0,
    ];
    assert_eq!(sys_sysctl(p, &args, &mut retval), Ok(()));
    assert_eq!((oldlen, &out[..7]), (7, &b"EmiBSD\0"[..]));

    // One component is not enough; a new value needs root.
    let args = [ra(&name) as Register, 1, 0, 0, 0, 0];
    assert_eq!(sys_sysctl(p, &args, &mut retval), Err(Errno::EINVAL));
    let new = 1i32;
    let args = [ra(&name) as Register, 2, 0, 0, ra(&new) as Register, 4];
    assert_eq!(sys_sysctl(p, &args, &mut retval), Err(Errno::EPERM));
    let name = [CTL_DEBUG, 0];
    let args = [ra(&name) as Register, 2, 0, 0, 0, 0];
    assert_eq!(sys_sysctl(p, &args, &mut retval), Err(Errno::EOPNOTSUPP));
}
