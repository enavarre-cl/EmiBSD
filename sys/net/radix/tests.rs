//! Host tests for the radix trees, as pf's tables use them: IPv4 keys laid out as
//! `sockaddr_in`s (length, family, port, address) in a tree at the address's offset, routes
//! added with and without masks, longest-prefix matches, exact lookups, duplicated keys,
//! deletion and walks.

use std::boxed::Box;
use std::sync::MutexGuard;
use std::vec::Vec;

use super::*;
use crate::kern::subr_pool::tests::setup_real_memory;

/// `sizeof(struct sockaddr_in)`.
const SIN_LEN: u8 = 16;
/// `offsetof(struct sockaddr_in, sin_addr)`.
const SIN_ADDR_OFF: i32 = 4;
/// `AF_INET`.
const AF_INET: u8 = 2;

/// Fresh memory, and the radix globals back to "never initialised" (the old ones point into
/// the previous test's memory), then `rn_init` as `pfr_initialize` calls it
/// (`sizeof(struct sockaddr_in6)`).
fn setup() -> MutexGuard<'static, ()> {
    let guard = setup_real_memory();
    crate::machine::cons::consinit();
    MAX_KEYLEN.store(0, Ordering::Relaxed);
    MASK_RNHEAD.store(ptr::null_mut(), Ordering::Relaxed);
    RN_ZEROS.store(ptr::null_mut(), Ordering::Relaxed);
    RN_ONES.store(ptr::null_mut(), Ordering::Relaxed);
    rn_init(28);
    guard
}

/// A `sockaddr_in` for `a`, in a buffer longer than `max_keylen` (pf's `sockaddr_union`).
fn sin(a: [u8; 4]) -> [u8; 32] {
    let mut k = [0u8; 32];
    k[0] = SIN_LEN;
    k[1] = AF_INET;
    k[4..8].copy_from_slice(&a);
    k
}

/// The mask of a `plen`-bit prefix, as `pfr_prepare_network` builds it.
fn mask(plen: u32) -> [u8; 32] {
    let m = if plen == 0 {
        0
    } else {
        u32::MAX << (32 - plen)
    };
    sin(m.to_be_bytes())
}

/// What pf's `struct pfr_kentry` holds for the tree: the node pair and the key.
struct Entry {
    nodes: [RadixNode; 2],
    key: [u8; 32],
}

fn entry(a: [u8; 4]) -> &'static Entry {
    Box::leak(Box::new(Entry {
        nodes: [RadixNode::new(), RadixNode::new()],
        key: sin(a),
    }))
}

fn head() -> &'static RadixNodeHead {
    let mut h = None;
    assert!(rn_inithead(&mut h, SIN_ADDR_OFF));
    let h = h.expect("a head");
    let mut again = Some(h);
    assert!(
        rn_inithead(&mut again, SIN_ADDR_OFF),
        "an existing head is kept"
    );
    assert!(again.is_some_and(|a| ptr::eq(a, h)));
    h
}

fn add(h: &RadixNodeHead, e: &'static Entry, plen: Option<u32>) -> Option<&'static RadixNode> {
    let m = plen.map(mask);
    let mp = m.as_ref().map_or(ptr::null(), |m| m.as_ptr());
    // SAFETY: the key and the nodes are leaked, the mask is read during the call only.
    unsafe { rn_addroute(e.key.as_ptr(), mp, h, &e.nodes, 0) }
}

fn del(h: &RadixNodeHead, e: &'static Entry, plen: Option<u32>) -> Option<&'static RadixNode> {
    let m = plen.map(mask);
    let mp = m.as_ref().map_or(ptr::null(), |m| m.as_ptr());
    // SAFETY: as in `add`.
    unsafe { rn_delete(e.key.as_ptr(), mp, h, None) }
}

fn matched(h: &RadixNodeHead, a: [u8; 4]) -> Option<&'static RadixNode> {
    let k = sin(a);
    // SAFETY: a local key, read during the call.
    unsafe { rn_match(k.as_ptr(), h) }
}

fn lookup(h: &RadixNodeHead, a: [u8; 4], plen: Option<u32>) -> Option<&'static RadixNode> {
    let k = sin(a);
    let m = plen.map(mask);
    let mp = m.as_ref().map_or(ptr::null(), |m| m.as_ptr());
    // SAFETY: local key and mask, read during the call.
    unsafe { rn_lookup(k.as_ptr(), mp, h) }
}

fn is(rn: Option<&RadixNode>, e: &Entry) -> bool {
    rn.is_some_and(|rn| ptr::eq(rn, &e.nodes[0]))
}

/// The address of a leaf's key.
fn addr_of(rn: &RadixNode) -> [u8; 4] {
    let mut a = [0u8; 4];
    // SAFETY: leaves hold the tests' leaked 32-byte keys.
    a.copy_from_slice(unsafe { core::slice::from_raw_parts(rn.rn_key.get().add(4), 4) });
    a
}

#[test]
fn masks_refine_and_order() {
    let (m8, m16, m24) = (mask(8), mask(16), mask(24));
    // SAFETY: local masks.
    unsafe {
        assert!(rn_refines(m16.as_ptr(), m8.as_ptr()), "/16 is inside /8");
        assert!(!rn_refines(m8.as_ptr(), m16.as_ptr()));
        assert!(!rn_refines(m16.as_ptr(), m16.as_ptr()), "equal masks");
        assert!(rn_refines(m24.as_ptr(), m16.as_ptr()));
        let odd = sin([255, 0, 255, 0]);
        assert!(!rn_refines(odd.as_ptr(), m16.as_ptr()));
        assert!(rn_lexobetter(m24.as_ptr(), m16.as_ptr()));
        assert!(!rn_lexobetter(m16.as_ptr(), m24.as_ptr()));
        assert!(!rn_lexobetter(m16.as_ptr(), m16.as_ptr()));
    }
}

#[test]
fn longest_prefix_wins() {
    let _g = setup();
    let h = head();
    let net8 = entry([10, 0, 0, 0]);
    let net16 = entry([10, 1, 0, 0]);
    let net24 = entry([10, 1, 2, 0]);
    let host = entry([10, 1, 2, 3]);
    let other = entry([192, 168, 0, 0]);

    assert!(matched(h, [10, 1, 2, 3]).is_none(), "empty tree");
    assert!(is(add(h, net16, Some(16)), net16));
    assert!(is(add(h, net8, Some(8)), net8));
    assert!(is(add(h, host, None), host));
    assert!(is(add(h, net24, Some(24)), net24));
    assert!(is(add(h, other, Some(16)), other));

    assert!(is(matched(h, [10, 1, 2, 3]), host));
    assert!(is(matched(h, [10, 1, 2, 4]), net24));
    assert!(is(matched(h, [10, 1, 2, 255]), net24));
    assert!(is(matched(h, [10, 1, 3, 1]), net16));
    assert!(is(matched(h, [10, 1, 255, 255]), net16));
    assert!(is(matched(h, [10, 2, 0, 0]), net8));
    assert!(is(matched(h, [10, 255, 255, 255]), net8));
    assert!(is(matched(h, [192, 168, 77, 1]), other));
    assert!(matched(h, [192, 169, 0, 1]).is_none());
    assert!(matched(h, [11, 0, 0, 0]).is_none());
    assert!(
        matched(h, [0, 0, 0, 0]).is_none(),
        "the root leaf is not a route"
    );

    // A default route catches the rest; it is a duplicated key of the left root leaf.
    let dflt = entry([0, 0, 0, 0]);
    assert!(is(add(h, dflt, Some(0)), dflt));
    assert!(is(matched(h, [11, 0, 0, 0]), dflt));
    assert!(
        is(matched(h, [0, 0, 0, 0]), dflt),
        "explicitly asked for the default"
    );
    assert!(is(matched(h, [10, 1, 2, 3]), host));
}

#[test]
fn lookup_wants_the_exact_mask() {
    let _g = setup();
    let h = head();
    let net8 = entry([10, 0, 0, 0]);
    let net16 = entry([10, 1, 0, 0]);
    let net24 = entry([10, 1, 2, 0]);
    add(h, net8, Some(8)).expect("added");
    add(h, net16, Some(16)).expect("added");
    add(h, net24, Some(24)).expect("added");

    assert!(is(lookup(h, [10, 1, 0, 0], Some(16)), net16));
    assert!(is(lookup(h, [10, 0, 0, 0], Some(8)), net8));
    assert!(is(lookup(h, [10, 1, 2, 0], Some(24)), net24));
    assert!(
        lookup(h, [10, 1, 0, 0], Some(24)).is_none(),
        "a known mask, but not on that key"
    );
    assert!(
        lookup(h, [10, 1, 0, 0], Some(20)).is_none(),
        "a mask the mask tree does not have"
    );
    assert!(
        is(lookup(h, [10, 1, 9, 9], None), net16),
        "no mask is rn_match"
    );
    assert!(lookup(h, [12, 0, 0, 0], None).is_none());
}

#[test]
fn duplicated_keys_chain_most_specific_first() {
    let _g = setup();
    let h = head();
    let net16 = entry([10, 1, 0, 0]);
    assert!(is(add(h, net16, Some(16)), net16));

    let again = entry([10, 1, 0, 0]);
    assert!(add(h, again, Some(16)).is_none(), "same key and mask");

    let net24 = entry([10, 1, 0, 0]);
    let host = entry([10, 1, 0, 0]);
    let net8 = entry([10, 0, 0, 0]);
    let net8b = entry([10, 0, 0, 0]);
    assert!(is(add(h, net24, Some(24)), net24), "same key, longer mask");
    assert!(is(add(h, host, None), host), "same key, no mask");
    assert!(
        add(h, entry([10, 1, 0, 0]), None).is_none(),
        "a second host route"
    );
    assert!(is(add(h, net8, Some(8)), net8));
    assert!(add(h, net8b, Some(8)).is_none());

    // The chain starts with the host route, then the masks from the most specific.
    let mut chain = Vec::new();
    let mut rn = matched(h, [10, 1, 0, 0]);
    while let Some(n) = rn {
        chain.push(ptr::from_ref(n));
        // SAFETY: the chain's leaves are the tests' leaked nodes.
        rn = unsafe { n.rn_dupedkey.get().as_ref() };
    }
    assert_eq!(
        chain,
        [&host.nodes[0], &net24.nodes[0], &net16.nodes[0]].map(ptr::from_ref)
    );

    assert!(is(matched(h, [10, 1, 0, 0]), host));
    assert!(is(matched(h, [10, 1, 0, 7]), net24));
    assert!(is(matched(h, [10, 1, 7, 7]), net16));
    assert!(is(matched(h, [10, 7, 7, 7]), net8));
    assert!(is(lookup(h, [10, 1, 0, 0], Some(16)), net16));
    assert!(is(lookup(h, [10, 1, 0, 0], Some(24)), net24));

    // Deleting from the chain: the head (the leaf in the tree), then the rest.
    assert!(is(del(h, host, None), host));
    assert_eq!(host.nodes[0].rn_flags.get() & RNF_ACTIVE, 0);
    assert!(is(matched(h, [10, 1, 0, 0]), net24));
    assert!(is(del(h, net16, Some(16)), net16));
    assert!(is(matched(h, [10, 1, 7, 7]), net8));
    assert!(is(matched(h, [10, 1, 0, 9]), net24));
    assert!(is(del(h, net24, Some(24)), net24));
    assert!(is(matched(h, [10, 1, 0, 0]), net8));
    assert!(is(del(h, net8, Some(8)), net8));
    assert!(matched(h, [10, 1, 0, 0]).is_none());
    assert_eq!(walk(h).len(), 0);
}

#[test]
fn delete_unlinks_and_returns_the_pair() {
    let _g = setup();
    let h = head();
    let net8 = entry([10, 0, 0, 0]);
    let net16 = entry([10, 1, 0, 0]);
    let net24 = entry([10, 1, 2, 0]);
    let host = entry([10, 1, 2, 3]);
    let far = entry([172, 16, 0, 0]);
    add(h, net8, Some(8)).expect("added");
    add(h, net16, Some(16)).expect("added");
    add(h, net24, Some(24)).expect("added");
    add(h, host, None).expect("added");
    add(h, far, Some(12)).expect("added");

    assert!(
        del(h, entry([10, 9, 9, 9]), None).is_none(),
        "not in the tree"
    );
    assert!(
        del(h, net16, Some(24)).is_none(),
        "the key with another mask"
    );
    assert!(del(h, net16, Some(20)).is_none(), "an unknown mask");

    assert!(is(del(h, net24, Some(24)), net24));
    assert_eq!(net24.nodes[0].rn_flags.get() & RNF_ACTIVE, 0);
    assert_eq!(net24.nodes[1].rn_flags.get() & RNF_ACTIVE, 0);
    assert!(is(matched(h, [10, 1, 2, 4]), net16));
    assert!(is(matched(h, [10, 1, 2, 3]), host));
    assert!(del(h, net24, Some(24)).is_none(), "already gone");

    assert!(is(del(h, net8, Some(8)), net8));
    assert!(matched(h, [10, 2, 0, 0]).is_none());
    assert!(is(matched(h, [10, 1, 9, 9]), net16));
    assert!(is(matched(h, [172, 31, 1, 1]), far));

    // The pair can go back in once zeroed, as pf's `bzero(ke->pfrke_node)` does.
    net24.nodes[0].assign(&RadixNode::new());
    net24.nodes[1].assign(&RadixNode::new());
    assert!(is(add(h, net24, Some(24)), net24));
    assert!(is(matched(h, [10, 1, 2, 4]), net24));

    for (e, plen) in [
        (host, None),
        (net16, Some(16)),
        (far, Some(12)),
        (net24, Some(24)),
    ] {
        assert!(is(del(h, e, plen), e));
    }
    assert!(walk(h).is_empty());
    assert!(matched(h, [10, 1, 2, 3]).is_none());
}

#[test]
fn non_contiguous_masks_match_through_the_annotations() {
    let _g = setup();
    let h = head();
    let odd = entry([10, 0, 5, 0]);
    let near = entry([10, 0, 4, 0]);
    let m = sin([255, 0, 255, 0]);
    // SAFETY: leaked key and nodes; the mask is read during the calls only.
    unsafe {
        assert!(is(
            rn_addroute(odd.key.as_ptr(), m.as_ptr(), h, &odd.nodes, 0),
            odd
        ));
    }
    add(h, near, Some(24)).expect("added");
    assert!(is(matched(h, [10, 7, 5, 9]), odd));
    assert!(is(matched(h, [10, 0, 5, 1]), odd));
    assert!(is(matched(h, [10, 0, 4, 1]), near));
    assert!(matched(h, [10, 7, 6, 9]).is_none());
    assert!(matched(h, [11, 0, 5, 0]).is_none());
    // SAFETY: as above.
    unsafe {
        assert!(is(rn_lookup(odd.key.as_ptr(), m.as_ptr(), h), odd));
        assert!(is(rn_delete(odd.key.as_ptr(), m.as_ptr(), h, None), odd));
    }
    assert!(matched(h, [10, 7, 5, 9]).is_none());
}

/// The addresses `rn_walktree` visits, in order.
fn walk(h: &RadixNodeHead) -> Vec<[u8; 4]> {
    let mut seen = Vec::new();
    let r = rn_walktree(h, |rn, id| {
        assert_eq!(id, h.rnh_rtableid.get());
        seen.push(addr_of(rn));
        Ok(())
    });
    assert_eq!(r, Ok(()));
    seen
}

#[test]
fn walk_visits_every_leaf_in_key_order() {
    let _g = setup();
    let h = head();
    assert!(walk(h).is_empty(), "the root leaves are not routes");
    h.rnh_rtableid.set(3);

    let addrs = [
        ([192, 168, 1, 0], Some(24)),
        ([10, 0, 0, 0], Some(8)),
        ([10, 1, 0, 0], Some(16)),
        ([10, 1, 0, 0], Some(24)),
        ([10, 1, 2, 3], None),
        ([0, 0, 0, 0], Some(0)),
        ([255, 255, 255, 255], None),
        ([172, 16, 0, 0], Some(12)),
    ];
    let entries: Vec<&'static Entry> = addrs
        .iter()
        .map(|&(a, plen)| {
            let e = entry(a);
            assert!(is(add(h, e, plen), e));
            e
        })
        .collect();

    let seen = walk(h);
    assert_eq!(seen.len(), addrs.len(), "duplicated keys included");
    let mut sorted = seen.clone();
    sorted.sort_unstable();
    assert_eq!(seen, sorted, "in key order");
    for (a, _) in addrs {
        assert!(seen.contains(&a));
    }

    // The first error stops the walk and comes back.
    let mut calls = 0;
    let r = rn_walktree(h, |_, _| {
        calls += 1;
        if calls == 3 {
            Err(Errno::EINTR)
        } else {
            Ok(())
        }
    });
    assert_eq!(r, Err(Errno::EINTR));
    assert_eq!(calls, 3);

    // The function may delete the leaf it is given.
    let r = rn_walktree(h, |rn, _| {
        let (&e, &(_, plen)) = entries
            .iter()
            .zip(addrs.iter())
            .find(|(e, _)| ptr::eq(rn, &e.nodes[0]))
            .expect("one of ours");
        assert!(is(del(h, e, plen), e));
        Ok(())
    });
    assert_eq!(r, Ok(()));
    assert!(walk(h).is_empty());
}
