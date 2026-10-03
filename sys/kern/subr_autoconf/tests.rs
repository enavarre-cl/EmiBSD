//! Host tests for autoconfiguration over a test `ioconf`: a root bus `troot0` and a starred
//! child driver `tchild*` below it, attached, looked up, deferred and detached.

use std::boxed::Box;
use std::sync::MutexGuard;
use std::sync::atomic::AtomicUsize;
use std::vec::Vec;
use std::{assert, assert_eq};

use super::*;
use crate::kern::subr_pool::tests::setup_real_memory;
use crate::machine::Machine;
use crate::sys::device::{CD_INDIRECT, Cfattach, DV_DULL, QUIET, UNCONF};

/// What the child's attach arguments say: whether to match, and at which level.
struct ChildArgs {
    pri: i32,
}

static ROOT_ATTACHED: AtomicUsize = AtomicUsize::new(0);
static CHILD_ATTACHED: AtomicUsize = AtomicUsize::new(0);
static PRINTED: AtomicUsize = AtomicUsize::new(0);
static DEFERRED_RAN: AtomicUsize = AtomicUsize::new(0);
static ACTIVATE_FAIL_UNIT: AtomicI32 = AtomicI32::new(-1);
static RESUMED: AtomicUsize = AtomicUsize::new(0);

fn root_match(parent: Option<&Device>, _m: &CfMatch, _aux: *mut c_void) -> i32 {
    assert!(parent.is_none());
    1
}

fn root_attach(parent: Option<&Device>, _self: &Device, _aux: *mut c_void) {
    assert!(parent.is_none());
    ROOT_ATTACHED.fetch_add(1, Ordering::Relaxed);
}

fn child_match(parent: Option<&Device>, m: &CfMatch, aux: *mut c_void) -> i32 {
    assert!(parent.is_some());
    assert_eq!(m.cfdata().cf_driver.cd_name, b"tchild");
    // SAFETY: the tests always pass a `ChildArgs` as the child's aux.
    unsafe { (*aux.cast::<ChildArgs>()).pri }
}

fn child_attach(parent: Option<&Device>, self_: &Device, _aux: *mut c_void) {
    assert!(parent.is_some());
    assert!(ptr::eq(
        self_.parent().expect("a parent"),
        parent.expect("a parent")
    ));
    CHILD_ATTACHED.fetch_add(1, Ordering::Relaxed);
}

fn child_detach(_dev: &Device, _flags: i32) -> Result<(), Errno> {
    Ok(())
}

fn child_activate(dev: &Device, act: i32) -> Result<(), Errno> {
    if dev.dv_unit.get() == ACTIVATE_FAIL_UNIT.load(Ordering::Relaxed) && act == DVACT_SUSPEND {
        return Err(Errno::EBUSY);
    }
    if act == DVACT_RESUME {
        RESUMED.fetch_add(1, Ordering::Relaxed);
    }
    Ok(())
}

fn child_print(_aux: *mut c_void, pnp: Option<&[u8]>) -> i32 {
    PRINTED.fetch_add(1, Ordering::Relaxed);
    if pnp.is_some() { UNCONF } else { QUIET }
}

fn deferred(dev: &Device) {
    assert_eq!(dev.cfdata().cf_driver.cd_name, b"tchild");
    DEFERRED_RAN.fetch_add(1, Ordering::Relaxed);
}

/// A fresh test `ioconf` (state lives in the tables, so every test gets its own) with
/// `troot0 at root` and `tchild* at troot0`; `root_mode` is the root driver's `cd_mode`.
fn ioconf(root_mode: i32) -> (&'static [Cfdata], &'static Cfdriver, &'static Cfdriver) {
    let root_ca: &'static Cfattach = Box::leak(Box::new(Cfattach {
        ca_devsize: size_of::<Device>(),
        ca_match: Some(root_match),
        ca_attach: root_attach,
        ca_detach: None,
        ca_activate: None,
    }));
    let root_cd: &'static Cfdriver =
        Box::leak(Box::new(Cfdriver::new(b"troot", DV_DULL, root_mode)));
    let child_ca: &'static Cfattach = Box::leak(Box::new(Cfattach {
        ca_devsize: size_of::<Device>() + 64,
        ca_match: Some(child_match),
        ca_attach: child_attach,
        ca_detach: Some(child_detach),
        ca_activate: Some(child_activate),
    }));
    let child_cd: &'static Cfdriver = Box::leak(Box::new(Cfdriver::new(b"tchild", DV_DULL, 0)));
    let table: &'static [Cfdata] = Box::leak(Box::new([
        Cfdata::new(root_ca, root_cd, 0, FSTATE_NOTFOUND, &[], 0, &[], 0, 0),
        Cfdata::new(child_ca, child_cd, 0, FSTATE_STAR, &[], 0, &[0], 0, 0),
    ]));
    (table, root_cd, child_cd)
}

/// Real memory for malloc(9), a fresh `ioconf` installed, the counters reset.
fn setup(
    root_mode: i32,
) -> (
    MutexGuard<'static, ()>,
    &'static [Cfdata],
    &'static Cfdriver,
    &'static Cfdriver,
) {
    let guard = setup_real_memory();
    let (table, root_cd, child_cd) = ioconf(root_mode);
    // SAFETY: `setup_real_memory`'s lock serialises the tests.
    unsafe { Machine::set_ioconf(table, &[0]) };
    config_init();
    for c in [
        &ROOT_ATTACHED,
        &CHILD_ATTACHED,
        &PRINTED,
        &DEFERRED_RAN,
        &RESUMED,
    ] {
        c.store(0, Ordering::Relaxed);
    }
    ACTIVATE_FAIL_UNIT.store(-1, Ordering::Relaxed);
    (guard, table, root_cd, child_cd)
}

fn name(dev: NonNull<Device>) -> Vec<u8> {
    // SAFETY: the tests only name attached devices.
    let n = unsafe { dev.as_ref() }.dv_xname.get();
    cstr(&n).to_vec()
}

fn aux(args: &mut ChildArgs) -> *mut c_void {
    ptr::from_mut(args).cast()
}

fn alldevs() -> Vec<Vec<u8>> {
    ALLDEVS
        .0
        .iter()
        .map(|d| cstr(&d.dv_xname.get()).to_vec())
        .collect()
}

#[test]
fn rootfound_attaches_the_named_root() {
    let (_g, table, root_cd, _) = setup(0);
    assert!(config_rootfound(b"nosuchroot", ptr::null_mut()).is_none());
    let root = config_rootfound(b"troot", ptr::null_mut()).expect("troot0");
    assert_eq!(name(root), b"troot0");
    assert_eq!(ROOT_ATTACHED.load(Ordering::Relaxed), 1);
    assert_eq!(table[0].cf_fstate.get(), FSTATE_FOUND);
    assert_eq!(root_cd.cd_dev(0), Some(root));
    assert_eq!(alldevs(), [b"troot0".to_vec()]);
    // SAFETY: just attached.
    let r = unsafe { root.as_ref() };
    assert_eq!(
        r.dv_ref.load(Ordering::Relaxed),
        2,
        "config_make_softc's and config_attach's"
    );
    assert!(r.dv_flags.get() & DVF_ACTIVE != 0);
    assert!(r.parent().is_none());
}

#[test]
fn starred_children_take_increasing_units() {
    let (_g, table, _, child_cd) = setup(0);
    let root = config_rootfound(b"troot", ptr::null_mut()).expect("troot0");
    // SAFETY: attached above.
    let r = unsafe { root.as_ref() };
    let mut yes = ChildArgs { pri: 1 };
    let a = config_found(r, aux(&mut yes), Some(child_print)).expect("tchild0");
    let b = config_found(r, aux(&mut yes), Some(child_print)).expect("tchild1");
    assert_eq!(name(a), b"tchild0");
    assert_eq!(name(b), b"tchild1");
    assert_eq!(table[1].cf_unit.get(), 2);
    assert_eq!(table[1].cf_fstate.get(), FSTATE_STAR);
    assert_eq!(CHILD_ATTACHED.load(Ordering::Relaxed), 2);
    assert_eq!(
        PRINTED.load(Ordering::Relaxed),
        2,
        "once each, with pnp == NULL"
    );
    assert_eq!(child_cd.cd_dev(1), Some(b));
    assert_eq!(alldevs().len(), 3);
}

#[test]
fn an_unmatched_device_is_printed_not_configured() {
    let (_g, _, _, _) = setup(0);
    let root = config_rootfound(b"troot", ptr::null_mut()).expect("troot0");
    // SAFETY: attached above.
    let r = unsafe { root.as_ref() };
    let mut no = ChildArgs { pri: 0 };
    assert!(config_found(r, aux(&mut no), Some(child_print)).is_none());
    assert_eq!(PRINTED.load(Ordering::Relaxed), 1);
    assert_eq!(CHILD_ATTACHED.load(Ordering::Relaxed), 0);
}

#[test]
fn indirect_parents_match_against_softcs() {
    let (_g, _, _, child_cd) = setup(CD_INDIRECT);
    let root = config_rootfound(b"troot", ptr::null_mut()).expect("troot0");
    // SAFETY: attached above.
    let r = unsafe { root.as_ref() };
    let mut yes = ChildArgs { pri: 3 };
    let m = config_search(None, r, aux(&mut yes)).expect("a match");
    assert!(matches!(m, CfMatch::Softc(_)));
    let child = config_attach(Some(r), m, aux(&mut yes), None);
    assert_eq!(name(child), b"tchild0");
    assert_eq!(child_cd.cd_dev(0), Some(child));
    // A losing softc is freed: nothing is left behind in cd_devs.
    let mut no = ChildArgs { pri: 0 };
    assert!(config_search(None, r, aux(&mut no)).is_none());
    assert!(child_cd.cd_dev(1).is_none());
}

#[test]
fn detach_frees_the_unit_for_reuse() {
    let (_g, table, _, child_cd) = setup(0);
    let root = config_rootfound(b"troot", ptr::null_mut()).expect("troot0");
    // SAFETY: attached above.
    let r = unsafe { root.as_ref() };
    let mut yes = ChildArgs { pri: 1 };
    let a = config_found(r, aux(&mut yes), None).expect("tchild0");
    let b = config_found(r, aux(&mut yes), None).expect("tchild1");
    let serial = AUTOCONF_SERIAL.load(Ordering::Relaxed);

    // SAFETY: attached, not used afterwards.
    assert_eq!(unsafe { config_detach(b, DETACH_QUIET) }, Ok(()));
    assert_eq!(table[1].cf_unit.get(), 1, "the last starred unit is reused");
    assert!(child_cd.cd_dev(1).is_none());
    assert_eq!(AUTOCONF_SERIAL.load(Ordering::Relaxed), serial + 1);
    let c = config_found(r, aux(&mut yes), None).expect("tchild1 again");
    assert_eq!(name(c), b"tchild1");

    // The root has no ca_detach: it fails after the deactivation, which the C keeps (and
    // which reached the children through config_activate_children).
    // SAFETY: attached; on failure it stays attached.
    let rv = unsafe { config_detach(root, DETACH_QUIET) };
    assert_eq!(rv, Err(Errno::EOPNOTSUPP));
    assert!(r.dv_flags.get() & DVF_ACTIVE == 0);
    // SAFETY: still attached.
    assert!(unsafe { a.as_ref() }.dv_flags.get() & DVF_ACTIVE == 0);

    assert_eq!(config_detach_children(r, DETACH_QUIET), Ok(()));
    assert_eq!(alldevs(), [b"troot0".to_vec()]);
    assert_eq!(
        child_cd.cd_ndevs.get(),
        0,
        "the empty cd_devs array is freed"
    );
}

#[test]
fn device_lookup_takes_a_reference_on_active_devices() {
    let (_g, _, _, child_cd) = setup(0);
    let root = config_rootfound(b"troot", ptr::null_mut()).expect("troot0");
    // SAFETY: attached above.
    let r = unsafe { root.as_ref() };
    let mut yes = ChildArgs { pri: 1 };
    let a = config_found(r, aux(&mut yes), None).expect("tchild0");
    // SAFETY: attached above.
    let d = unsafe { a.as_ref() };

    assert!(device_lookup(child_cd, 1).is_none());
    assert!(device_lookup(child_cd, -1).is_none());
    assert_eq!(device_lookup(child_cd, 0), Some(a));
    assert_eq!(d.dv_ref.load(Ordering::Relaxed), 3);
    // SAFETY: gives back the lookup's reference.
    unsafe { device_unref(a) };
    assert_eq!(d.dv_ref.load(Ordering::Relaxed), 2);

    assert_eq!(config_deactivate(d), Ok(()));
    assert!(device_lookup(child_cd, 0).is_none(), "inactive");
}

#[test]
fn deferred_configuration_runs_after_the_parent_attach() {
    let (_g, _, _, _) = setup(0);
    let root = config_rootfound(b"troot", ptr::null_mut()).expect("troot0");
    // SAFETY: attached above.
    let r = unsafe { root.as_ref() };
    let mut yes = ChildArgs { pri: 1 };
    let a = config_found(r, aux(&mut yes), None).expect("tchild0");
    // SAFETY: attached above.
    let d = unsafe { a.as_ref() };

    config_defer(d, deferred);
    assert_eq!(CONFIG_PENDING.load(Ordering::Relaxed), 1);
    config_process_deferred_children(d);
    assert_eq!(
        DEFERRED_RAN.load(Ordering::Relaxed),
        0,
        "not a child of itself"
    );
    config_process_deferred_children(r);
    assert_eq!(DEFERRED_RAN.load(Ordering::Relaxed), 1);
    assert_eq!(CONFIG_PENDING.load(Ordering::Relaxed), 0);

    config_mountroot(d, deferred);
    assert_eq!(
        DEFERRED_RAN.load(Ordering::Relaxed),
        1,
        "no root file system yet"
    );
    config_process_deferred_mountroot();
    assert_eq!(DEFERRED_RAN.load(Ordering::Relaxed), 2);
}

#[test]
fn a_refused_suspend_resumes_the_earlier_siblings() {
    let (_g, _, _, _) = setup(0);
    let root = config_rootfound(b"troot", ptr::null_mut()).expect("troot0");
    // SAFETY: attached above.
    let r = unsafe { root.as_ref() };
    let mut yes = ChildArgs { pri: 1 };
    for _ in 0..3 {
        config_found(r, aux(&mut yes), None).expect("a child");
    }
    ACTIVATE_FAIL_UNIT.store(2, Ordering::Relaxed);
    assert_eq!(config_suspend(r, DVACT_SUSPEND), Err(Errno::EBUSY));
    assert_eq!(
        RESUMED.load(Ordering::Relaxed),
        2,
        "tchild1 and tchild0 resumed"
    );
    ACTIVATE_FAIL_UNIT.store(-1, Ordering::Relaxed);
    assert_eq!(config_suspend(r, DVACT_SUSPEND), Ok(()));
}
