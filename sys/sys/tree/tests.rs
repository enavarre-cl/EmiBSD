//! Tests of `tree.rs` (splay trees and the classic `RB_*` API); see there.

use core::cmp::Ordering;
use std::vec::Vec;

use super::*;

struct Node {
    key: i32,
    sp: SplayEntry<Node>,
    rb: RbEntry<Node>,
}

impl Node {
    const fn new(key: i32) -> Self {
        Self {
            key,
            sp: SplayEntry::new(),
            rb: RbEntry::new(),
        }
    }
}

fn by_key(a: &Node, b: &Node) -> Ordering {
    a.key.cmp(&b.key)
}

crate::tree_adapter!(Sp: Node, sp => SplayEntry<Node>, by_key);
crate::tree_adapter!(Rb: Node, rb => RbEntry<Node>, by_key);

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

/// Checks the red-black invariants of a classic tree and returns its black height.
fn check_rb(t: &RbHead<Rb>) -> usize {
    fn walk(node: Option<&Node>, parent_red: bool) -> usize {
        let Some(n) = node else { return 1 };
        let red = RbHead::<Rb>::color(n) == RB_RED;
        assert!(
            !(red && parent_red),
            "red node {} under a red parent",
            n.key
        );
        let (l, r) = (RbHead::<Rb>::left(n), RbHead::<Rb>::right(n));
        if let Some(l) = l {
            assert!(l.key < n.key);
            assert!(ptr::eq(RbHead::<Rb>::parent(l).unwrap(), n));
        }
        if let Some(r) = r {
            assert!(r.key > n.key);
            assert!(ptr::eq(RbHead::<Rb>::parent(r).unwrap(), n));
        }
        let (lh, rh) = (walk(l, red), walk(r, red));
        assert_eq!(lh, rh, "black height differs under {}", n.key);
        lh + usize::from(!red)
    }
    if let Some(root) = t.root() {
        assert_eq!(RbHead::<Rb>::color(root), RB_BLACK);
        assert!(RbHead::<Rb>::parent(root).is_none());
    }
    walk(t.root(), false)
}

#[test]
fn splay_insert_find_iterate() {
    let n = nodes();
    let t = SplayHead::<Sp>::new();
    assert!(t.is_empty());
    assert!(t.min().is_none() && t.max().is_none());
    // SAFETY: the nodes outlive the tree and start unlinked.
    unsafe {
        for node in &n {
            assert!(t.insert(node).is_none());
        }
    }
    assert_eq!(keys(t.iter()), sorted());

    // find splays the hit to the root
    assert_eq!(key(t.find(&Node::new(65))), Some(65));
    assert_eq!(key(t.root()), Some(65));
    assert!(t.find(&Node::new(66)).is_none());
    assert_eq!(key(t.min()), Some(1));
    assert_eq!(key(t.root()), Some(1));
    assert_eq!(key(t.max()), Some(99));
    assert_eq!(key(t.root()), Some(99));

    let mut chain = Vec::new();
    let mut cur = t.min();
    while let Some(c) = cur {
        chain.push(c.key);
        cur = t.next(c);
    }
    assert_eq!(chain, sorted());

    // a duplicate key is refused and the existing element returned
    let dup = Node::new(30);
    // SAFETY: `dup` is unlinked; the tree refuses it, so it may go out of scope.
    let existing = unsafe { t.insert(&dup) };
    assert!(ptr::eq(existing.unwrap(), &n[4]));
    assert_eq!(keys(t.iter()), sorted());
}

#[test]
fn splay_remove() {
    let n = nodes();
    let t = SplayHead::<Sp>::new();
    // SAFETY: the nodes outlive the tree; each removed node is linked or already gone.
    unsafe {
        assert!(t.remove(&n[0]).is_none()); // empty tree
        for node in &n {
            t.insert(node);
        }
        let mut expect = sorted();
        for &k in &[50, 1, 99, 30, 65, 20] {
            let node = n.iter().find(|x| x.key == k).unwrap();
            assert!(ptr::eq(t.remove(node).unwrap(), node));
            expect.retain(|&x| x != k);
            assert_eq!(keys(t.iter()), expect);
        }
        assert!(t.remove(&Node::new(1234)).is_none());
        for node in &n {
            t.remove(node);
        }
    }
    assert!(t.is_empty());
    t.init();
    assert!(t.is_empty());
}

#[test]
fn rb_insert_find_iterate() {
    let n = nodes();
    let t = RbHead::<Rb>::new();
    assert!(t.is_empty());
    // SAFETY: the nodes outlive the tree and start unlinked.
    unsafe {
        for node in &n {
            assert!(t.insert(node).is_none());
            check_rb(&t);
        }
    }
    assert_eq!(keys(t.iter()), sorted());
    let mut rev = sorted();
    rev.reverse();
    assert_eq!(keys(t.iter_reverse()), rev);
    assert_eq!(key(t.min()), Some(1));
    assert_eq!(key(t.max()), Some(99));

    assert_eq!(key(t.find(&Node::new(35))), Some(35));
    assert!(t.find(&Node::new(36)).is_none());
    assert_eq!(key(t.nfind(&Node::new(66))), Some(70));
    assert_eq!(key(t.nfind(&Node::new(99))), Some(99));
    assert!(t.nfind(&Node::new(100)).is_none());
    assert_eq!(key(t.nfind(&Node::new(0))), Some(1));

    let mut chain = Vec::new();
    let mut cur = t.min();
    while let Some(c) = cur {
        chain.push(c.key);
        cur = RbHead::<Rb>::next(c);
    }
    assert_eq!(chain, sorted());
    chain.clear();
    cur = t.max();
    while let Some(c) = cur {
        chain.push(c.key);
        cur = RbHead::<Rb>::prev(c);
    }
    assert_eq!(chain, rev);

    let dup = Node::new(70);
    // SAFETY: `dup` is unlinked; the tree refuses it.
    let existing = unsafe { t.insert(&dup) };
    assert!(ptr::eq(existing.unwrap(), &n[2]));
}

#[test]
fn rb_remove_keeps_invariants() {
    let n = nodes();
    let t = RbHead::<Rb>::new();
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
            check_rb(&t);
        }
    }
    assert!(t.is_empty());
}

#[test]
fn rb_remove_while_iterating() {
    let n = nodes();
    let t = RbHead::<Rb>::new();
    // SAFETY: the nodes outlive the tree and start unlinked.
    unsafe {
        for node in &n {
            t.insert(node);
        }
    }
    for node in t.iter() {
        if node.key % 10 == 5 {
            // SAFETY: `node` is in the tree; the iterator already read its successor.
            unsafe { t.remove(node) };
        }
    }
    let expect: Vec<i32> = sorted().into_iter().filter(|k| k % 10 != 5).collect();
    assert_eq!(keys(t.iter()), expect);
    check_rb(&t);
}
