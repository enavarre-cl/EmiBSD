//! Host tests for the credentials and the id system calls: the reference counting of
//! `crget`/`crhold`/`crfree`/`crcopy`/`crdup`, `groupmember`, `suser`, and the permission
//! rules of the set*id calls against threads built by hand.

use std::boxed::Box;
use std::sync::MutexGuard;
use std::{assert, assert_eq, assert_ne};

use super::*;
use crate::kern::kern_proc::procinit;
use crate::kern::subr_pool::tests::setup_real_memory;
use crate::sys::proc::refreshcreds;
use crate::sys::user::User;

/// Real memory and the process pools (`ucred_pool` among them) and hashes.
fn setup() -> MutexGuard<'static, ()> {
    let guard = setup_real_memory();
    crate::machine::cons::consinit();
    procinit();
    guard
}

/// Credentials with one reference whose real, effective and saved ids are `uid`/`gid`.
fn cred(uid: Uid, gid: Gid) -> &'static Ucred {
    let cr = crget();
    cr.cr_uid.set(uid);
    cr.cr_ruid.set(uid);
    cr.cr_svuid.set(uid);
    cr.cr_gid.set(gid);
    cr.cr_rgid.set(gid);
    cr.cr_svgid.set(gid);
    cr
}

/// A thread of a fresh process, both holding `cr` (the thread `cr`'s reference, the process
/// one more), with a u-area for the TCB, charged to its real uid as `fork1` does.
fn thread(cr: &'static Ucred) -> &'static Proc {
    let pr: &'static Process = Box::leak(Box::new(Process::new()));
    let p: &'static Proc = Box::leak(Box::new(Proc::new()));
    p.p_p.set(pr);
    p.p_addr.set(Box::leak(Box::new(User::new())));
    pr.ps_mainproc.set(p);
    p.p_ucred.set(cr);
    pr.ps_ucred.set(crhold(cr));
    chgproccnt(cr.cr_ruid.get(), 1);
    p
}

/// The argument registers of a system call.
fn args(a: &[Register]) -> SysArgs {
    let mut v: SysArgs = [0; 6];
    v[..a.len()].copy_from_slice(a);
    v
}

/// `(uid_t)-1` in an argument register.
const NONE: Register = Uid::MAX as Register;

/// The real, effective and saved uids of `cr`.
fn uids(cr: &Ucred) -> (Uid, Uid, Uid) {
    (cr.cr_ruid.get(), cr.cr_uid.get(), cr.cr_svuid.get())
}

#[test]
fn crget_crhold_crfree_count_references() {
    let _g = setup();
    let nout = UCRED_POOL.pr_nout.get();

    let cr = crget();
    assert_eq!(cr.cr_refcnt.r_refs.load(Ordering::Relaxed), 1);
    assert_eq!(uids(cr), (0, 0, 0));
    assert_eq!(cr.cr_ngroups.get(), 0);
    assert_eq!(UCRED_POOL.pr_nout.get(), nout + 1);

    assert!(ptr::eq(crhold(cr), cr));
    assert_eq!(cr.cr_refcnt.r_refs.load(Ordering::Relaxed), 2);
    crfree(cr);
    assert_eq!(cr.cr_refcnt.r_refs.load(Ordering::Relaxed), 1);
    assert_eq!(UCRED_POOL.pr_nout.get(), nout + 1);
    crfree(cr);
    assert_eq!(UCRED_POOL.pr_nout.get(), nout);
}

#[test]
fn crcopy_copies_only_shared_credentials_and_crdup_always() {
    let _g = setup();

    let cr = cred(1000, 100);
    cr.cr_ngroups.set(2);
    cr.cr_groups[0].set(100);
    cr.cr_groups[1].set(20);
    assert!(ptr::eq(crcopy(cr), cr)); // the only reference: nothing to copy

    crhold(cr);
    let copy = crcopy(cr);
    assert!(!ptr::eq(copy, cr));
    assert_eq!(cr.cr_refcnt.r_refs.load(Ordering::Relaxed), 1);
    assert_eq!(copy.cr_refcnt.r_refs.load(Ordering::Relaxed), 1);
    assert_eq!(uids(copy), (1000, 1000, 1000));
    assert_eq!(copy.cr_svgid.get(), 100);
    assert_eq!(copy.cr_ngroups.get(), 2);
    assert_eq!(copy.cr_groups[1].get(), 20);

    let dup = crdup(cr);
    assert!(!ptr::eq(dup, cr));
    assert_eq!(cr.cr_refcnt.r_refs.load(Ordering::Relaxed), 1);
    assert_eq!(dup.cr_refcnt.r_refs.load(Ordering::Relaxed), 1);
    assert_eq!(uids(dup), (1000, 1000, 1000));
    for c in [cr, copy, dup] {
        crfree(c);
    }
}

#[test]
fn groupmember_suser_and_crfromxucred() {
    let _g = setup();

    let cr = cred(1000, 100);
    cr.cr_ngroups.set(2);
    cr.cr_groups[0].set(100);
    cr.cr_groups[1].set(20);
    cr.cr_groups[2].set(30); // beyond cr_ngroups
    assert!(groupmember(100, cr));
    assert!(groupmember(20, cr));
    assert!(!groupmember(30, cr));
    assert_eq!(suser_ucred(cr), Err(Errno::EPERM));
    cr.cr_uid.set(0);
    assert_eq!(suser_ucred(cr), Ok(()));

    let mut x = Xucred {
        cr_uid: 5,
        cr_gid: 6,
        cr_ngroups: 1,
        cr_groups: [7; NGROUPS_MAX],
    };
    let k = Ucred::new();
    assert_eq!(crfromxucred(&k, &x), Ok(()));
    assert_eq!(
        (k.cr_uid.get(), k.cr_gid.get(), k.cr_ngroups.get()),
        (5, 6, 1)
    );
    assert_eq!((k.cr_groups[0].get(), k.cr_groups[1].get()), (7, 0));
    x.cr_ngroups = NGROUPS_MAX as i16 + 1;
    assert_eq!(crfromxucred(&k, &x), Err(Errno::EINVAL));
    x.cr_ngroups = -1;
    assert_eq!(crfromxucred(&k, &x), Err(Errno::EINVAL));
    crfree(cr);
}

#[test]
fn setresuid_allows_the_current_ids_and_root_anything() {
    let _g = setup();
    let mut rv: [Register; 2] = [0; 2];

    // A user whose saved uid is still root may become root again, nothing else.
    let cr = cred(1000, 100);
    cr.cr_svuid.set(0);
    let p = thread(cr);
    let pr = p.process();
    assert_eq!(
        sys_setresuid(p, &args(&[NONE, NONE, NONE]), &mut rv),
        Ok(())
    );
    assert_eq!(pr.ps_flags.load(Ordering::Relaxed) & PS_SUGID, 0);
    assert_eq!(
        sys_setresuid(p, &args(&[NONE, 2000, NONE]), &mut rv),
        Err(Errno::EPERM)
    );
    assert!(ptr::eq(pr.ucred(), cr));
    assert_eq!(sys_setresuid(p, &args(&[NONE, 0, NONE]), &mut rv), Ok(()));

    // The process has new credentials; the thread keeps its own until it refreshes them.
    assert!(!ptr::eq(pr.ucred(), cr));
    assert_eq!(uids(pr.ucred()), (1000, 0, 0));
    assert_ne!(pr.ps_flags.load(Ordering::Relaxed) & PS_SUGID, 0);
    assert_eq!(uids(p.ucred()), (1000, 1000, 0));
    assert_eq!(cr.cr_refcnt.r_refs.load(Ordering::Relaxed), 1);
    refreshcreds(p);
    assert!(ptr::eq(p.ucred(), pr.ucred()));
    assert_eq!(p.ucred().cr_refcnt.r_refs.load(Ordering::Relaxed), 2);

    // Now root: any uid at all, and the process count moves to the new real uid.
    assert_eq!(
        sys_setresuid(p, &args(&[3000, 3000, 3000]), &mut rv),
        Ok(())
    );
    refreshcreds(p);
    assert_eq!(uids(p.ucred()), (3000, 3000, 3000));
    assert_eq!(
        sys_setresuid(p, &args(&[0, NONE, NONE]), &mut rv),
        Err(Errno::EPERM)
    );
}

#[test]
fn setreuid_resets_the_saved_uid_with_the_real_one() {
    let _g = setup();
    let mut rv: [Register; 2] = [0; 2];

    let p = thread(cred(0, 0));
    let pr = p.process();
    // Only the effective uid: the saved uid stays.
    assert_eq!(sys_setreuid(p, &args(&[NONE, 1000]), &mut rv), Ok(()));
    assert_eq!(uids(pr.ucred()), (0, 1000, 0));
    refreshcreds(p);
    // The real uid changes: the saved one follows it.
    assert_eq!(sys_setreuid(p, &args(&[1000, NONE]), &mut rv), Ok(()));
    assert_eq!(uids(pr.ucred()), (1000, 1000, 1000));
    refreshcreds(p);
    assert_eq!(
        sys_setreuid(p, &args(&[0, NONE]), &mut rv),
        Err(Errno::EPERM)
    );
}

#[test]
fn setuid_and_seteuid_of_a_user_with_a_root_saved_uid() {
    let _g = setup();
    let mut rv: [Register; 2] = [0; 2];

    let cr = cred(1000, 100);
    cr.cr_svuid.set(0);
    let p = thread(cr);
    let pr = p.process();
    // setuid(0) is allowed (0 is the saved uid) but, not being root, only the effective
    // uid changes.
    assert_eq!(sys_setuid(p, &args(&[0]), &mut rv), Ok(()));
    assert_eq!(uids(pr.ucred()), (1000, 0, 0));
    assert_eq!(sys_seteuid(p, &args(&[2000]), &mut rv), Err(Errno::EPERM));
    assert_eq!(sys_seteuid(p, &args(&[1000]), &mut rv), Ok(()));
    assert_eq!(uids(pr.ucred()), (1000, 1000, 0));
    refreshcreds(p);
    // setgid to a group the thread does not have needs root.
    assert_eq!(sys_setgid(p, &args(&[200]), &mut rv), Err(Errno::EPERM));
    assert_eq!(sys_setegid(p, &args(&[100]), &mut rv), Ok(()));
}

#[test]
fn the_get_calls_copy_out_and_the_groups_round_trip() {
    let _g = setup();
    let mut rv: [Register; 2] = [0; 2];

    let cr = cred(1000, 100);
    cr.cr_svuid.set(7);
    let p = thread(cred(0, 0));
    let (mut r, e, mut s): (Uid, Uid, Uid) = (9, 9, 9);
    let ptrs = |a: &mut Uid| a as *mut Uid as Register;
    // A root thread installs cr's groups, then reads them back.
    let groups: [Gid; 3] = [100, 20, 30];
    assert_eq!(
        sys_setgroups(p, &args(&[3, groups.as_ptr() as Register]), &mut rv),
        Ok(())
    );
    refreshcreds(p);
    assert_eq!(sys_getgroups(p, &args(&[0, 0]), &mut rv), Ok(()));
    assert_eq!(rv[0], 3);
    let mut back: [Gid; 4] = [0; 4];
    assert_eq!(
        sys_getgroups(p, &args(&[2, back.as_mut_ptr() as Register]), &mut rv),
        Err(Errno::EINVAL)
    );
    assert_eq!(
        sys_getgroups(p, &args(&[4, back.as_mut_ptr() as Register]), &mut rv),
        Ok(())
    );
    assert_eq!((rv[0], back), (3, [100, 20, 30, 0]));

    let q = thread(cr);
    assert_eq!(
        sys_getresuid(q, &args(&[ptrs(&mut r), 0, ptrs(&mut s)]), &mut rv),
        Ok(())
    );
    assert_eq!((r, e, s), (1000, 9, 7));
    assert_eq!(
        sys_setgroups(q, &args(&[1, groups.as_ptr() as Register]), &mut rv),
        Err(Errno::EPERM)
    );
}

#[test]
fn the_tcb_and_the_thread_name_round_trip() {
    let _g = setup();
    let mut rv: [Register; 2] = [0; 2];

    let p = thread(cred(0, 0));
    assert_eq!(sys___set_tcb(p, &args(&[0x1234_5000]), &mut rv), Ok(()));
    assert_eq!(sys___get_tcb(p, &args(&[]), &mut rv), Ok(()));
    assert_eq!(rv[0], 0x1234_5000);

    let name = b"worker\0";
    assert_eq!(
        sys_setthrname(p, &args(&[0, name.as_ptr() as Register]), &mut rv),
        Ok(())
    );
    assert_eq!((rv[0], p.name()), (0, &b"worker"[..]));
    let mut out = [0xffu8; 4];
    assert_eq!(
        sys_getthrname(p, &args(&[0, out.as_mut_ptr() as Register, 4]), &mut rv),
        Ok(())
    );
    assert_eq!(rv[0], Errno::ERANGE as i32 as Register);
    let mut out = [0xffu8; _MAXCOMLEN];
    assert_eq!(
        sys_getthrname(
            p,
            &args(&[0, out.as_mut_ptr() as Register, _MAXCOMLEN as Register]),
            &mut rv
        ),
        Ok(())
    );
    assert_eq!((rv[0], &out[..7]), (0, &name[..]));
    assert_eq!(
        sys_getthrname(p, &args(&[12345, out.as_mut_ptr() as Register, 4]), &mut rv),
        Err(Errno::ESRCH)
    );
}
