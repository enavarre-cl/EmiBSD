//! Tests of `subr_tree.rs` through the `RBT_*` API of `sys::tree`; see there.

use core::cell::Cell;
use core::cmp::Ordering;
use core::ptr;
use std::vec::Vec;

use crate::sys::tree::{RB_BLACK, RB_RED, RbtAdapter, RbtEntry, RbtHead};
use crate::tree_adapter;

struct Node {
    key: i32,
    /// Nodes in the subtree rooted here, maintained by the augment hook of `Aug`.
    size: Cell<usize>,
    rbt: RbtEntry,
}

impl Node {
    const fn new(key: i32) -> Self {
        Self {
            key,
            // a node not yet in a tree is a subtree of one
            size: Cell::new(1),
            rbt: RbtEntry::new(),
        }
    }
}

fn by_key(a: &Node, b: &Node) -> Ordering {
    a.key.cmp(&b.key)
}

fn subtree_size(n: &Node) {
    let size = |c: Option<&Node>| c.map_or(0, |c| c.size.get());
    n.size
        .set(1 + size(RbtHead::<Aug>::left(n)) + size(RbtHead::<Aug>::right(n)));
}

tree_adapter!(Plain: Node, rbt => RbtEntry, by_key);
tree_adapter!(Aug: Node, rbt => RbtEntry, by_key, augment = subtree_size);

/// What a user of the augment hook does after an insert or a remove (`uvm_map_addr_augment`):
/// the tree augments the parent and whatever it rotated, the caller walks the rest of the way
/// up.
fn augment_up(mut node: Option<&Node>) {
    while let Some(n) = node {
        subtree_size(n);
        node = RbtHead::<Aug>::parent(n);
    }
}

const KEYS: [i32; 15] = [50, 20, 70, 10, 30, 60, 80, 25, 35, 65, 5, 15, 90, 1, 99];

fn nodes() -> [Node; 15] {
    core::array::from_fn(|i| Node::new(KEYS[i]))
}

fn sorted() -> Vec<i32> {
    let mut v = KEYS.to_vec();
    v.sort_unstable();
    v
}

fn keys<'a>(it: impl Iterator<Item = &'a Node>) -> Vec<i32> {
    it.map(|n| n.key).collect()
}

fn key(n: Option<&Node>) -> Option<i32> {
    n.map(|n| n.key)
}

/// Checks the red-black invariants and returns the black height; with `sizes`, also that every
/// node's `size` is its subtree's node count.
fn check<A: RbtAdapter<Elem = Node>>(t: &RbtHead<A>, sizes: bool) -> usize {
    fn walk<A: RbtAdapter<Elem = Node>>(
        node: Option<&Node>,
        parent_red: bool,
        sizes: bool,
    ) -> (usize, usize) {
        let Some(n) = node else { return (1, 0) };
        let red = A::entry(n).color() == RB_RED;
        assert!(
            !(red && parent_red),
            "red node {} under a red parent",
            n.key
        );
        let (l, r) = (RbtHead::<A>::left(n), RbtHead::<A>::right(n));
        if let Some(l) = l {
            assert!(l.key < n.key);
            assert!(ptr::eq(RbtHead::<A>::parent(l).unwrap(), n));
        }
        if let Some(r) = r {
            assert!(r.key > n.key);
            assert!(ptr::eq(RbtHead::<A>::parent(r).unwrap(), n));
        }
        let ((lh, lc), (rh, rc)) = (walk::<A>(l, red, sizes), walk::<A>(r, red, sizes));
        assert_eq!(lh, rh, "black height differs under {}", n.key);
        let count = 1 + lc + rc;
        if sizes {
            assert_eq!(n.size.get(), count, "size of {}", n.key);
        }
        (lh + usize::from(!red), count)
    }
    if let Some(root) = t.root() {
        assert_eq!(A::entry(root).color(), RB_BLACK);
        assert!(RbtHead::<A>::parent(root).is_none());
    }
    walk::<A>(t.root(), false, sizes).0
}

#[test]
fn insert_find_iterate() {
    let n = nodes();
    let t = RbtHead::<Plain>::new();
    assert!(t.is_empty() && t.root().is_none() && t.min().is_none());
    // SAFETY: the nodes outlive the tree and start unlinked.
    unsafe {
        for node in &n {
            assert!(t.insert(node).is_none());
            check(&t, false);
        }
    }
    assert_eq!(keys(t.iter()), sorted());
    let mut rev = sorted();
    rev.reverse();
    assert_eq!(keys(t.iter_reverse()), rev);
    assert_eq!(key(t.min()), Some(1));
    assert_eq!(key(t.max()), Some(99));
    assert_eq!(key(t.find(&Node::new(25))), Some(25));
    assert!(t.find(&Node::new(26)).is_none());
    assert_eq!(key(t.nfind(&Node::new(26))), Some(30));
    assert_eq!(key(t.nfind(&Node::new(1))), Some(1));
    assert!(t.nfind(&Node::new(100)).is_none());

    let mut chain = Vec::new();
    let mut cur = t.min();
    while let Some(c) = cur {
        chain.push(c.key);
        cur = RbtHead::<Plain>::next(c);
    }
    assert_eq!(chain, sorted());
    chain.clear();
    cur = t.max();
    while let Some(c) = cur {
        chain.push(c.key);
        cur = RbtHead::<Plain>::prev(c);
    }
    assert_eq!(chain, rev);

    let dup = Node::new(10);
    // SAFETY: `dup` is unlinked; the tree refuses it.
    assert!(ptr::eq(unsafe { t.insert(&dup) }.unwrap(), &n[3]));
}

#[test]
fn remove_keeps_invariants() {
    let n = nodes();
    let t = RbtHead::<Plain>::new();
    // SAFETY: the nodes outlive the tree; every removed node is linked.
    unsafe {
        for node in &n {
            t.insert(node);
        }
        let mut expect = sorted();
        for &k in &[50, 1, 99, 30, 65, 20, 70, 10, 80, 25, 35, 60, 5, 15, 90] {
            let node = n.iter().find(|x| x.key == k).unwrap();
            assert!(ptr::eq(t.remove(node), node));
            expect.retain(|&x| x != k);
            assert_eq!(keys(t.iter()), expect, "after removing {k}");
            check(&t, false);
        }
    }
    assert!(t.is_empty());
    t.init();
    assert!(t.is_empty());
}

#[test]
fn remove_while_iterating_in_reverse() {
    let n = nodes();
    let t = RbtHead::<Plain>::new();
    // SAFETY: the nodes outlive the tree and start unlinked.
    unsafe {
        for node in &n {
            t.insert(node);
        }
    }
    for node in t.iter_reverse() {
        if node.key > 60 {
            // SAFETY: `node` is in the tree; the iterator already read its predecessor.
            unsafe { t.remove(node) };
        }
    }
    let expect: Vec<i32> = sorted().into_iter().filter(|&k| k <= 60).collect();
    assert_eq!(keys(t.iter()), expect);
    check(&t, false);
}

#[test]
fn augment_tracks_subtree_sizes() {
    let n = nodes();
    let t = RbtHead::<Aug>::new();
    // SAFETY: the nodes outlive the tree; every removed node is linked.
    unsafe {
        for (i, node) in n.iter().enumerate() {
            t.insert(node);
            augment_up(Some(node));
            check(&t, true);
            assert_eq!(t.root().unwrap().size.get(), i + 1);
        }
        let mut left = n.len();
        for &k in &[50, 1, 99, 30, 65, 20, 70, 10] {
            let node = n.iter().find(|x| x.key == k).unwrap();
            let parent = RbtHead::<Aug>::parent(node);
            t.remove(node);
            augment_up(parent);
            left -= 1;
            check(&t, true);
            assert_eq!(t.root().unwrap().size.get(), left);
        }
    }
}

#[test]
fn poison_and_check() {
    let n = Node::new(1);
    RbtHead::<Plain>::poison(&n, 0xdead_beef);
    assert!(RbtHead::<Plain>::check(&n, 0xdead_beef));
    assert!(!RbtHead::<Plain>::check(&n, 0xdead_beee));
    let t = RbtHead::<Plain>::new();
    // SAFETY: `n` outlives the tree and is unlinked (poison is not a link).
    unsafe { t.insert(&n) };
    assert!(!RbtHead::<Plain>::check(&n, 0xdead_beef));
}

#[test]
fn set_links_by_hand() {
    let n = nodes();
    let t = RbtHead::<Plain>::new();
    // SAFETY: the nodes outlive the tree and start unlinked.
    unsafe {
        for node in &n[..3] {
            t.insert(node);
        }
    }
    let root = t.root().unwrap();
    let left = RbtHead::<Plain>::left(root).unwrap();
    // SAFETY: detaching and re-attaching the same child leaves the tree as it was.
    unsafe {
        RbtHead::<Plain>::set_left(root, None);
        assert!(RbtHead::<Plain>::left(root).is_none());
        RbtHead::<Plain>::set_parent(left, None);
        assert!(RbtHead::<Plain>::parent(left).is_none());
        RbtHead::<Plain>::set_left(root, Some(left));
        RbtHead::<Plain>::set_parent(left, Some(root));
        RbtHead::<Plain>::set_right(root, RbtHead::<Plain>::right(root));
    }
    assert_eq!(keys(t.iter()), [20, 50, 70]);
    check(&t, false);
}
