//! Host tests for anchors and rulesets: anchor paths creating their chain of anchors, lookups,
//! the leaf ruleset of a partly existing path, `pf_anchor_setup` with absolute, relative and
//! wildcard calls (and `pf_anchor_copyout` giving them back), and the removal of empty
//! anchors.

use std::boxed::Box;
use std::sync::MutexGuard;

use super::*;
use crate::kern::subr_pool::pool_init;
use crate::kern::subr_pool::tests::setup_real_memory;
use crate::machine::intr::IPL_SOFTNET;
use crate::net::pfvar::{PfiocRule, pf_abi_zeroed};

/// Fresh memory, `pf_anchor_pl` initialised as `pfattach` does, and empty anchor trees.
pub(crate) fn setup() -> MutexGuard<'static, ()> {
    let guard = setup_real_memory();
    crate::machine::cons::consinit();
    pool_init(
        &PF_ANCHOR_PL,
        size_of::<PfAnchor>(),
        align_of::<PfAnchor>() as u32,
        IPL_SOFTNET,
        0,
        "pfanchor",
        None,
    );
    PF_ANCHORS.init();
    pf_init_ruleset(pf_main_ruleset());
    guard
}

fn rule() -> &'static PfRule {
    Box::leak(pf_abi_zeroed::<PfRule>())
}

fn path(a: &PfAnchor) -> &[u8] {
    pf_cstr(&a.path)
}

fn anchor_of(rs: &PfRuleset) -> &'static PfAnchor {
    rs.anchor.get().expect("an anchored ruleset")
}

#[test]
fn find_or_create_builds_the_chain() {
    let _g = setup();

    assert!(ptr::eq(
        pf_find_or_create_ruleset(b"").unwrap(),
        pf_main_ruleset()
    ));
    assert!(ptr::eq(pf_find_ruleset(b"///").unwrap(), pf_main_ruleset()));

    let rs = pf_find_or_create_ruleset(b"a/b/c\0junk").expect("created");
    let c = anchor_of(rs);
    assert_eq!(path(c), b"a/b/c");
    assert_eq!(pf_cstr(&c.name), b"c");
    let b = c.parent.get().expect("parent b");
    assert_eq!(path(b), b"a/b");
    let a = b.parent.get().expect("parent a");
    assert_eq!(path(a), b"a");
    assert!(a.parent.get().is_none());
    assert!(ptr::eq(a.ruleset.anchor.get().unwrap(), a));

    // Found again, with or without leading slashes; the intermediate ones exist too.
    assert!(ptr::eq(pf_find_ruleset(b"a/b/c").unwrap(), rs));
    assert!(ptr::eq(pf_find_ruleset(b"//a/b/c").unwrap(), rs));
    assert!(ptr::eq(pf_find_or_create_ruleset(b"/a/b/c").unwrap(), rs));
    assert!(ptr::eq(pf_find_ruleset(b"a/b").unwrap(), &b.ruleset));
    assert!(ptr::eq(pf_find_anchor(b"a").unwrap(), a));
    assert!(pf_find_ruleset(b"a/x").is_none());
    assert!(pf_find_anchor(b"b").is_none());

    // The global tree holds the three, each parent its child.
    assert_eq!(PF_ANCHORS.iter().count(), 3);
    assert!(ptr::eq(a.children.root().unwrap(), b));
    assert!(ptr::eq(b.children.root().unwrap(), c));

    // A sibling shares the existing prefix.
    let d = anchor_of(pf_find_or_create_ruleset(b"a/d").unwrap());
    assert!(ptr::eq(d.parent.get().unwrap(), a));
    assert_eq!(PF_ANCHORS.iter().count(), 4);

    // Slashes before the components to create are skipped, as in C.
    let e = anchor_of(pf_find_or_create_ruleset(b"a//e").unwrap());
    assert_eq!(path(e), b"a/e");
    // Empty and over-long names are refused (the C keeps the anchors made before).
    assert!(pf_find_or_create_ruleset(b"x//y").is_none());
    assert!(pf_find_anchor(b"x").is_some());
    let long = [b'n'; PF_ANCHOR_NAME_SIZE];
    assert!(pf_create_anchor(None, &long).is_none());
    assert!(pf_create_anchor(None, b"").is_none());
    // An existing anchor collides.
    assert!(pf_create_anchor(None, b"a").is_none());
}

#[test]
fn leaf_ruleset_of_a_partial_path() {
    let _g = setup();
    let ab = pf_find_or_create_ruleset(b"a/b").unwrap();

    let mut buf = [0u8; 32];
    buf[..7].copy_from_slice(b"a/b/x/y");
    let (rs, rem) = pf_get_leaf_ruleset(&mut buf);
    assert!(ptr::eq(rs, ab));
    assert_eq!(pf_cstr(&buf[rem..]), b"/x/y");
    assert_eq!(pf_cstr(&buf), b"a/b/x/y", "slashes restored");

    let mut buf = [0u8; 32];
    buf[..6].copy_from_slice(b"//a/b/");
    let (rs, rem) = pf_get_leaf_ruleset(&mut buf);
    assert!(ptr::eq(rs, ab));
    assert_eq!(pf_cstr(&buf[rem..]), b"/");
    assert_eq!(pf_cstr(&buf), b"//a/b/");

    let mut buf = [0u8; 32];
    buf[..3].copy_from_slice(b"q/r");
    let (rs, rem) = pf_get_leaf_ruleset(&mut buf);
    assert!(ptr::eq(rs, pf_main_ruleset()));
    assert_eq!(rem, 0);
    assert_eq!(pf_cstr(&buf), b"q/r");
}

#[test]
fn anchor_setup_and_copyout() {
    let _g = setup();
    let ab = pf_find_or_create_ruleset(b"a/b").unwrap();
    let mut pr = pf_abi_zeroed::<PfiocRule>();

    // Relative, one level up.
    let r = rule();
    assert!(pf_anchor_setup(r, ab, b"../x"));
    let x = r.anchor().expect("anchor set");
    assert_eq!(path(x), b"a/x");
    assert_eq!(r.anchor_relative.get(), 2);
    assert_eq!(r.anchor_wildcard.get(), 0);
    assert_eq!(x.refcnt.get(), 1);
    assert!(pf_anchor_copyout(ab, r, &mut pr));
    assert_eq!(pf_cstr(&pr.anchor_call), b"../x");

    // Relative, below the ruleset's anchor.
    let r2 = rule();
    assert!(pf_anchor_setup(r2, ab, b"c"));
    assert_eq!(path(r2.anchor().unwrap()), b"a/b/c");
    assert_eq!(r2.anchor_relative.get(), 1);
    assert!(pf_anchor_copyout(ab, r2, &mut pr));
    assert_eq!(pf_cstr(&pr.anchor_call), b"c");

    // Absolute.
    let r3 = rule();
    assert!(pf_anchor_setup(r3, ab, b"/abs/y"));
    assert_eq!(path(r3.anchor().unwrap()), b"abs/y");
    assert_eq!(r3.anchor_relative.get(), 0);
    assert!(pf_anchor_copyout(ab, r3, &mut pr));
    assert_eq!(pf_cstr(&pr.anchor_call), b"/abs/y");

    // Wildcard from the main ruleset.
    let r4 = rule();
    assert!(pf_anchor_setup(r4, pf_main_ruleset(), b"a/*"));
    let a = r4.anchor().unwrap();
    assert_eq!(path(a), b"a");
    assert_eq!(r4.anchor_wildcard.get(), 1);
    assert_eq!(r4.anchor_relative.get(), 1);
    assert!(pf_anchor_copyout(pf_main_ruleset(), r4, &mut pr));
    assert_eq!(pf_cstr(&pr.anchor_call), b"a/*");

    // No call at all.
    let r5 = rule();
    assert!(pf_anchor_setup(r5, ab, b""));
    assert!(r5.anchor().is_none());
    assert!(pf_anchor_copyout(ab, r5, &mut pr));
    assert_eq!(pf_cstr(&pr.anchor_call), b"");

    // Failures: `..` beyond the root, and the main ruleset itself.
    let r6 = rule();
    assert!(!pf_anchor_setup(r6, pf_main_ruleset(), b"../x"));
    assert!(!pf_anchor_setup(r6, pf_main_ruleset(), b"/"));
    assert!(r6.anchor().is_none());

    // pf_remove_anchor drops the reference and frees the now unused anchor.
    pf_remove_anchor(r);
    assert!(r.anchor().is_none());
    assert!(pf_find_anchor(b"a/x").is_none());
    assert!(pf_find_anchor(b"a").is_some(), "a still has children");
}

#[test]
fn remove_if_empty_climbs_to_the_root() {
    let _g = setup();
    let rs = pf_find_or_create_ruleset(b"a/b/c").unwrap();
    let c = anchor_of(rs);
    let b = c.parent.get().unwrap();

    // A reference on b keeps it (and a) once c is gone.
    b.refcnt.set(1);
    pf_remove_if_empty_ruleset(rs);
    assert!(pf_find_anchor(b"a/b/c").is_none());
    assert!(pf_find_anchor(b"a/b").is_some());
    assert!(b.children.is_empty());

    // Tables keep a ruleset too.
    b.refcnt.set(0);
    b.ruleset.tables.set(1);
    pf_remove_if_empty_ruleset(&b.ruleset);
    assert!(pf_find_anchor(b"a/b").is_some());

    b.ruleset.tables.set(0);
    pf_remove_if_empty_ruleset(&b.ruleset);
    assert!(pf_find_anchor(b"a/b").is_none());
    assert!(pf_find_anchor(b"a").is_none());
    assert!(PF_ANCHORS.is_empty());

    // The main ruleset is never removed.
    pf_remove_if_empty_ruleset(pf_main_ruleset());
    pf_anchor_rele(Some(&PF_MAIN_ANCHOR));
    assert!(pf_anchor_take(Some(&PF_MAIN_ANCHOR)).is_some());
}

#[test]
fn transaction_reference_outlives_removal() {
    let _g = setup();
    let rs = pf_find_or_create_ruleset(b"t").unwrap();
    let t = anchor_of(rs);
    // A transaction holds the anchor: removal unlinks it, the last rele frees it.
    assert!(ptr::eq(pf_anchor_take(Some(t)).unwrap(), t));
    pf_remove_if_empty_ruleset(rs);
    assert!(pf_find_anchor(b"t").is_none());
    assert_eq!(PF_ANCHOR_PL.pr_nout.get(), 1);
    pf_anchor_rele(Some(t));
    assert_eq!(PF_ANCHOR_PL.pr_nout.get(), 0);
}
