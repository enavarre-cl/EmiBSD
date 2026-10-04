//! Host tests for `/dev/pf`: starting and stopping pf, a rule set transaction that installs a
//! `block in quick` rule and reads it back, the default timeouts and pool limits, tag names,
//! and the open and write-permission checks.

use std::boxed::Box;
use std::sync::MutexGuard;
use std::vec;
use std::vec::Vec;

use super::*;
use crate::kern::kern_proc::procinit;
use crate::kern::kern_prot::{crget, crhold};
use crate::net::if_::tests::setup_net;
use crate::sys::fcntl::FREAD;
use crate::sys::proc::Process;
use crate::sys::ucred::Ucred;

/// The network test lock with fresh memory, the process tables, and pf attached (every test
/// resets the memory the pools draw from, so pf is attached again over it).
fn setup() -> MutexGuard<'static, ()> {
    let guard = setup_net();
    procinit();
    crate::kern::kern_timeout::timeout_startup();
    pfattach(1);
    guard
}

/// A thread of a process with credentials, as `fork1` would leave it.
fn thread() -> &'static Proc {
    let cr: &'static Ucred = crget();
    cr.cr_ruid.set(1000);
    let pr: &'static Process = Box::leak(Box::new(Process::new()));
    let p: &'static Proc = Box::leak(Box::new(Proc::new()));
    p.p_p.set(pr);
    p.p_ucred.set(cr);
    pr.ps_ucred.set(crhold(cr));
    pr.ps_mainproc.set(p);
    pr.ps_pid.set(42);
    p
}

/// The minor of the first clone of the device.
const DEV: Dev = 0;

/// `sys_ioctl`'s kernel copy of an `AbiPod` argument.
fn pod<T: AbiPod>(v: &T) -> Vec<u8> {
    let mut data = vec![0u8; size_of::<T>()];
    ioctl_ret(&mut data, v);
    data
}

/// `sys_ioctl`'s kernel copy of a `PfAbi` argument.
fn abi<T: PfAbi>(v: &T) -> Vec<u8> {
    pf_abi_bytes(v).to_vec()
}

fn ioctl(cmd: u64, data: &mut [u8], p: &Proc) -> Result<(), Errno> {
    pfioctl(DEV, cmd, data, FREAD | FWRITE, p)
}

fn status(p: &Proc) -> Box<PfStatus> {
    let mut data = abi(&*pf_abi_zeroed::<PfStatus>());
    assert_eq!(ioctl(DIOCGETSTATUS, &mut data, p), Ok(()));
    pf_abi_read::<PfStatus>(&data)
}

#[test]
fn start_and_stop() {
    let _g = setup();
    let p = thread();

    assert_eq!(status(p).running.get(), 0);
    assert_eq!(ioctl(DIOCSTART, &mut [], p), Ok(()));
    let s = status(p);
    assert_eq!(s.running.get(), 1);
    assert_eq!(ioctl(DIOCSTART, &mut [], p), Err(Errno::EEXIST));

    assert_eq!(ioctl(DIOCSTOP, &mut [], p), Ok(()));
    assert_eq!(status(p).running.get(), 0);
    assert_eq!(ioctl(DIOCSTOP, &mut [], p), Err(Errno::ENOENT));

    // The purge timeouts DIOCSTART armed would outlive the test's timeout wheel.
    timeout_del(&PF_PURGE_STATES_TO);
    timeout_del(&PF_PURGE_TO);
}

/// A `pfioc_trans` of one element for the main ruleset; the element is returned so that it
/// stays where `array` points.
fn trans_main() -> (Box<PfiocTransE>, PfiocTrans) {
    let mut e = pod_box::<PfiocTransE>();
    e.type_ = PF_TRANS_RULESET;
    let io = PfiocTrans {
        size: 1,
        esize: size_of::<PfiocTransE>() as i32,
        array: ptr::from_mut(&mut *e) as usize,
    };
    (e, io)
}

#[test]
fn a_committed_rule_is_read_back() {
    let _g = setup();
    let p = thread();

    // DIOCXBEGIN: the ticket of the main ruleset.
    let (e, io) = trans_main();
    let mut data = pod(&io);
    assert_eq!(ioctl(DIOCXBEGIN, &mut data, p), Ok(()));
    let ticket = e.ticket;
    assert_eq!(ticket, pf_main_ruleset().inactive.version.get());

    // DIOCADDRULE: block in quick.
    let mut pr = pf_abi_zeroed::<PfiocRule>();
    pr.ticket = ticket;
    pr.rule.action = PF_DROP;
    pr.rule.direction = PF_IN;
    pr.rule.quick = 1;
    pr.rule.rtableid = -1; // pfctl's "no rtable"
    pr.rule.onrdomain = -1;
    pr.rule.src.addr.type_.set(PF_ADDR_ADDRMASK);
    pr.rule.dst.addr.type_.set(PF_ADDR_ADDRMASK);
    let mut data = abi(&*pr);
    assert_eq!(ioctl(DIOCADDRULE, &mut data, p), Ok(()));
    assert_eq!(pf_main_ruleset().inactive.rcount.get(), 1);

    // A wrong ticket is refused.
    pr.ticket = ticket.wrapping_add(1);
    let mut data = abi(&*pr);
    assert_eq!(ioctl(DIOCADDRULE, &mut data, p), Err(Errno::EBUSY));

    // DIOCXCOMMIT: the rule becomes active, with its checksum.
    let mut data = pod(&io);
    assert_eq!(ioctl(DIOCXCOMMIT, &mut data, p), Ok(()));
    assert_eq!(pf_main_ruleset().active.rcount.get(), 1);
    assert_eq!(pf_main_ruleset().inactive.rcount.get(), 0);
    assert_ne!(PF_STATUS.pf_chksum.get(), [0; PF_MD5_DIGEST_LENGTH]);

    // DIOCGETRULES: one rule, and a ticket to walk them.
    let pr = pf_abi_zeroed::<PfiocRule>();
    let mut data = abi(&*pr);
    assert_eq!(ioctl(DIOCGETRULES, &mut data, p), Ok(()));
    let pr = pf_abi_read::<PfiocRule>(&data);
    assert_eq!(pr.nr, 1);
    let walk = pr.ticket;

    // DIOCGETRULE: the rule, without kernel pointers.
    let mut data = abi(&*pr);
    assert_eq!(ioctl(DIOCGETRULE, &mut data, p), Ok(()));
    let got = pf_abi_read::<PfiocRule>(&data);
    assert_eq!(got.nr, 0);
    assert_eq!(got.rule.action, PF_DROP);
    assert_eq!(got.rule.direction, PF_IN);
    assert_eq!(got.rule.quick, 1);
    assert_eq!(got.rule.cuid, 1000);
    assert_eq!(got.rule.cpid, 42);
    assert!(got.rule.kif.get().is_null());
    assert!(got.rule.overload_tbl.get().is_null());
    for s in &got.rule.skip {
        assert_eq!(s.nr(), u32::MAX, "the only rule skips to the end");
    }

    // The walk is over; DIOCXEND closes it.
    let mut data = abi(&*pr);
    assert_eq!(ioctl(DIOCGETRULE, &mut data, p), Err(Errno::ENOENT));
    let mut data = pod(&(walk as i32));
    assert_eq!(ioctl(DIOCXEND, &mut data, p), Ok(()));
    assert_eq!(ioctl(DIOCXEND, &mut data, p), Err(Errno::ENXIO));

    // A new transaction with no rule empties the active set.
    let (_e, io) = trans_main();
    let mut data = pod(&io);
    assert_eq!(ioctl(DIOCXBEGIN, &mut data, p), Ok(()));
    let mut data = pod(&io);
    assert_eq!(ioctl(DIOCXCOMMIT, &mut data, p), Ok(()));
    assert!(pf_main_ruleset().active_ptr().is_empty());
}

#[test]
fn timeouts_are_staged_until_the_commit() {
    let _g = setup();
    let p = thread();

    let get = |t: usize| {
        let mut data = pod(&PfiocTm {
            timeout: t as i32,
            seconds: 0,
        });
        assert_eq!(ioctl(DIOCGETTIMEOUT, &mut data, p), Ok(()));
        ioctl_arg::<PfiocTm>(&data).seconds
    };
    assert_eq!(get(PFTM_TCP_ESTABLISHED), PFTM_TCP_ESTABLISHED_VAL as i32);

    let (_e, io) = trans_main();
    let mut data = pod(&io);
    assert_eq!(ioctl(DIOCXBEGIN, &mut data, p), Ok(()));

    // DIOCSETTIMEOUT returns the value in force.
    let mut data = pod(&PfiocTm {
        timeout: PFTM_TCP_ESTABLISHED as i32,
        seconds: 3600,
    });
    assert_eq!(ioctl(DIOCSETTIMEOUT, &mut data, p), Ok(()));
    assert_eq!(
        ioctl_arg::<PfiocTm>(&data).seconds,
        PFTM_TCP_ESTABLISHED_VAL as i32
    );
    assert_eq!(get(PFTM_TCP_ESTABLISHED), PFTM_TCP_ESTABLISHED_VAL as i32);

    let mut data = pod(&io);
    assert_eq!(ioctl(DIOCXCOMMIT, &mut data, p), Ok(()));
    assert_eq!(get(PFTM_TCP_ESTABLISHED), 3600);

    // Out of range.
    let mut data = pod(&PfiocTm {
        timeout: PFTM_MAX as i32,
        seconds: 1,
    });
    assert_eq!(ioctl(DIOCSETTIMEOUT, &mut data, p), Err(Errno::EINVAL));
    assert_eq!(ioctl(DIOCGETTIMEOUT, &mut data, p), Err(Errno::EINVAL));
}

#[test]
fn pool_limits() {
    let _g = setup();
    let p = thread();

    let get = |i: usize| {
        let mut data = pod(&PfiocLimit {
            index: i as i32,
            limit: 0,
        });
        assert_eq!(ioctl(DIOCGETLIMIT, &mut data, p), Ok(()));
        ioctl_arg::<PfiocLimit>(&data).limit
    };
    assert_eq!(get(PF_LIMIT_STATES), PFSTATE_HIWAT);

    let mut data = pod(&PfiocLimit {
        index: PF_LIMIT_STATES as i32,
        limit: 5000,
    });
    assert_eq!(ioctl(DIOCSETLIMIT, &mut data, p), Ok(()));
    assert_eq!(get(PF_LIMIT_STATES), 5000);
    assert_eq!(PF_POOL_LIMITS[PF_LIMIT_STATES].limit_new.get(), 5000);

    let mut data = pod(&PfiocLimit {
        index: PF_LIMIT_MAX as i32,
        limit: 1,
    });
    assert_eq!(ioctl(DIOCSETLIMIT, &mut data, p), Err(Errno::EINVAL));
    assert_eq!(ioctl(DIOCGETLIMIT, &mut data, p), Err(Errno::EINVAL));

    // Restore the default for the other tests of this process.
    let mut data = pod(&PfiocLimit {
        index: PF_LIMIT_STATES as i32,
        limit: PFSTATE_HIWAT,
    });
    assert_eq!(ioctl(DIOCSETLIMIT, &mut data, p), Ok(()));
}

#[test]
fn tag_names_round_trip() {
    let _g = setup();

    pf_lock();
    let a = pf_tagname2tag(b"pftest-a\0", true);
    let b = pf_tagname2tag(b"pftest-b\0", true);
    assert_ne!(a, 0);
    assert_ne!(b, 0);
    assert_ne!(a, b);
    assert_eq!(
        pf_tagname2tag(b"pftest-a\0", false),
        a,
        "found, referenced again"
    );
    assert_eq!(pf_tagname2tag(b"pftest-none\0", false), 0);

    let mut name = [0u8; PF_TAG_NAME_SIZE];
    pf_tag2tagname(b, &mut name);
    assert_eq!(pf_cstr(&name), b"pftest-b");

    // Two references on a, one on b: the names go with the last.
    pf_tag_unref(a);
    pf_tag_unref(b);
    assert_eq!(pf_tagname2tag(b"pftest-b\0", false), 0);
    pf_tag_ref(a);
    pf_tag_unref(a);
    assert_eq!(pf_tagname2tag(b"pftest-a\0", false), a);
    pf_tag_unref(a);
    pf_tag_unref(a);
    assert_eq!(pf_tagname2tag(b"pftest-a\0", false), 0);

    // The lowest free number is reused.
    let c = pf_tagname2tag(b"pftest-c\0", true);
    assert!(c <= a.min(b), "a freed slot is taken first");
    pf_tag_unref(c);
    pf_unlock();
}

#[test]
fn open_and_permissions() {
    let _g = setup();
    let p = thread();

    assert_eq!(pfopen(0, 0, 0, p), Ok(()));
    assert_eq!(
        pfopen(1, 0, 0, p),
        Err(Errno::ENXIO),
        "not a clone's first minor"
    );
    assert_eq!(pfopen(1 << CLONE_SHIFT, 0, 0, p), Ok(()));

    // Read-only: reading is allowed, changing is not.
    let mut data = abi(&*pf_abi_zeroed::<PfStatus>());
    assert_eq!(pfioctl(DEV, DIOCGETSTATUS, &mut data, FREAD, p), Ok(()));
    assert_eq!(
        pfioctl(DEV, DIOCSTART, &mut [], FREAD, p),
        Err(Errno::EACCES)
    );

    // Unknown commands.
    assert_eq!(ioctl(0, &mut [], p), Err(Errno::ENODEV), "not a pf command");

    // DIOCADDSTATE (pfsync is configured): a state without a creator id is refused, one
    // with a timeout past PFTM_MAX too.
    let mut ps = PfiocState::default();
    assert_eq!(ioctl(DIOCADDSTATE, &mut pod(&ps), p), Err(Errno::EINVAL));
    ps.state.timeout = PFTM_MAX as u8;
    ps.state.creatorid = 1;
    assert_eq!(ioctl(DIOCADDSTATE, &mut pod(&ps), p), Err(Errno::EINVAL));

    assert_eq!(pfclose(DEV, 0, 0, Some(p)), Ok(()));
}

#[test]
fn the_rule_checks() {
    let r = PfRule::zeroed();
    assert_eq!(pf_rule_checkaf(&r), Ok(()));
    r.rule_flag.set(PFRULE_AFTO);
    assert_eq!(pf_rule_checkaf(&r), Err(Errno::EPFNOSUPPORT));

    assert!(!pf_validate_range(
        PF_OP_RRG,
        [12u16.to_be(), 34u16.to_be()],
        PF_ORDER_NET
    ));
    assert!(pf_validate_range(
        PF_OP_RRG,
        [34u16.to_be(), 12u16.to_be()],
        PF_ORDER_NET
    ));
    assert!(pf_validate_range(PF_OP_IRG, [12, 12], PF_ORDER_HOST));
    assert!(!pf_chk_limiter_action(PF_LIMITER_BLOCK));
    assert!(pf_chk_limiter_action(7));
}
