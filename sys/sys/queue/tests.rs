//! Tests of `queue.rs`; see there.

use std::vec::Vec;

use super::*;

/// An element that can sit in one list of every family at once.
struct Node {
    id: u32,
    sl: SlistEntry<Node>,
    li: ListEntry<Node>,
    sq: SimpleqEntry<Node>,
    xq: XsimpleqEntry<Node>,
    tq: TailqEntry<Node>,
    st: StailqEntry<Node>,
}

impl Node {
    const fn new(id: u32) -> Self {
        Self {
            id,
            sl: SlistEntry::new(),
            li: ListEntry::new(),
            sq: SimpleqEntry::new(),
            xq: XsimpleqEntry::new(),
            tq: TailqEntry::new(),
            st: StailqEntry::new(),
        }
    }
}

queue_adapter!(Sl: Node, sl => SlistEntry<Node>);
queue_adapter!(Li: Node, li => ListEntry<Node>);
queue_adapter!(Sq: Node, sq => SimpleqEntry<Node>);
queue_adapter!(Xq: Node, xq => XsimpleqEntry<Node>);
queue_adapter!(Tq: Node, tq => TailqEntry<Node>);
queue_adapter!(St: Node, st => StailqEntry<Node>);

fn nodes<const N: usize>() -> [Node; N] {
    core::array::from_fn(|i| Node::new(i as u32 + 1))
}

fn ids<'a>(it: impl Iterator<Item = &'a Node>) -> Vec<u32> {
    it.map(|n| n.id).collect()
}

fn id(n: Option<&Node>) -> Option<u32> {
    n.map(|n| n.id)
}

#[test]
fn adapter_offset_matches_the_field() {
    let n = Node::new(7);
    assert_eq!(Sl::OFFSET, core::mem::offset_of!(Node, sl));
    assert_eq!(Tq::OFFSET, core::mem::offset_of!(Node, tq));
    assert!(ptr::eq(Tq::entry(&n), &n.tq));
    // container_of inverts entry()
    let link: *const Cell<*const Node> = &n.tq.tqe_next;
    // SAFETY: `link` is the first cell of `n.tq`, the entry `Tq` names, inside `n`.
    let back = unsafe { container_of::<Tq>(link) };
    assert!(ptr::eq(back, &n));
}

#[test]
fn slist() {
    let n = nodes::<4>();
    let h = SlistHead::<Sl>::new();
    assert!(h.is_empty());
    assert_eq!(id(h.first()), None);
    // SAFETY: the nodes outlive the head and are linked into one slist each.
    unsafe {
        h.insert_head(&n[2]);
        h.insert_head(&n[0]);
        SlistHead::<Sl>::insert_after(&n[0], &n[1]);
        SlistHead::<Sl>::insert_after(&n[2], &n[3]);
    }
    assert_eq!(ids(h.iter()), [1, 2, 3, 4]);
    assert_eq!(id(h.first()), Some(1));
    assert_eq!(id(SlistHead::<Sl>::next(&n[1])), Some(3));
    assert_eq!(id(SlistHead::<Sl>::next(&n[3])), None);
    // SAFETY: each element is in the list as the operation requires.
    unsafe {
        h.remove(&n[2]); // middle, O(n) path
        assert_eq!(ids(h.iter()), [1, 2, 4]);
        h.remove(&n[0]); // first, remove_head path
        assert_eq!(ids(h.iter()), [2, 4]);
        SlistHead::<Sl>::remove_after(&n[1]);
        assert_eq!(ids(h.iter()), [2]);
        h.remove_head();
    }
    assert!(h.is_empty());
    h.init();
    assert!(h.is_empty());
}

#[test]
fn list() {
    let n = nodes::<5>();
    let h = ListHead::<Li>::new();
    // SAFETY: the nodes outlive the head; each operation's precondition holds.
    unsafe {
        h.insert_head(&n[2]);
        h.insert_head(&n[0]);
        ListHead::<Li>::insert_after(&n[0], &n[1]);
        ListHead::<Li>::insert_before(&n[2], &n[3]); // before an element in the middle
        ListHead::<Li>::insert_before(&n[0], &n[4]); // before the first: touches lh_first
    }
    assert_eq!(ids(h.iter()), [5, 1, 2, 4, 3]);
    assert_eq!(id(ListHead::<Li>::next(&n[3])), Some(3));
    // SAFETY: as above.
    unsafe {
        ListHead::<Li>::remove(&n[4]); // first
        ListHead::<Li>::remove(&n[2]); // last
        ListHead::<Li>::remove(&n[1]); // middle
    }
    assert_eq!(ids(h.iter()), [1, 4]);
    let fresh = Node::new(9);
    // SAFETY: `n[3]` is linked, `fresh` is not and outlives the head.
    unsafe { ListHead::<Li>::replace(&n[3], &fresh) };
    assert_eq!(ids(h.iter()), [1, 9]);
    // SAFETY: as above.
    unsafe {
        ListHead::<Li>::remove(&fresh);
        ListHead::<Li>::remove(&n[0]);
    }
    assert!(h.is_empty());
}

#[test]
fn list_remove_while_iterating() {
    let n = nodes::<6>();
    let h = ListHead::<Li>::new();
    // SAFETY: the nodes outlive the head and start unlinked.
    unsafe {
        for node in n.iter().rev() {
            h.insert_head(node);
        }
    }
    for node in h.iter() {
        if node.id % 2 == 0 {
            // SAFETY: `node` is in the list; the iterator already read its successor.
            unsafe { ListHead::<Li>::remove(node) };
        }
    }
    assert_eq!(ids(h.iter()), [1, 3, 5]);
}

#[test]
fn simpleq() {
    let n = nodes::<4>();
    let h = SimpleqHead::<Sq>::new();
    // SAFETY: the nodes outlive the head; each operation's precondition holds.
    unsafe {
        h.insert_tail(&n[1]); // tail into an empty queue uses the sentinel
        h.insert_head(&n[0]);
        h.insert_tail(&n[3]);
        h.insert_after(&n[1], &n[2]);
    }
    assert_eq!(ids(h.iter()), [1, 2, 3, 4]);
    assert_eq!(id(SimpleqHead::<Sq>::next(&n[0])), Some(2));
    // SAFETY: as above.
    unsafe {
        h.remove_head();
        assert_eq!(ids(h.iter()), [2, 3, 4]);
        h.remove_after(&n[2]); // removes the last: sqh_last must follow
        assert_eq!(ids(h.iter()), [2, 3]);
        h.insert_tail(&n[3]);
        assert_eq!(ids(h.iter()), [2, 3, 4]);
        h.remove_head();
        h.remove_head();
        h.remove_head();
    }
    assert!(h.is_empty());
    // SAFETY: the queue is empty again, so the tail insert goes through the sentinel.
    unsafe { h.insert_tail(&n[0]) };
    assert_eq!(ids(h.iter()), [1]);
}

#[test]
fn simpleq_concat() {
    let a = nodes::<2>();
    let b = nodes::<2>();
    let ha = SimpleqHead::<Sq>::new();
    let hb = SimpleqHead::<Sq>::new();
    // SAFETY: the nodes outlive the heads and start unlinked.
    unsafe {
        ha.insert_tail(&a[0]);
        ha.insert_tail(&a[1]);
        hb.insert_tail(&b[0]);
        hb.insert_tail(&b[1]);
        ha.concat(&hb);
    }
    assert_eq!(ids(ha.iter()), [1, 2, 1, 2]);
    assert!(hb.is_empty());
    // SAFETY: `hb` is empty, concat of an empty queue changes nothing; then tail insert works.
    unsafe {
        ha.concat(&hb);
        let c = Node::new(3);
        ha.insert_tail(&c);
        assert_eq!(ids(ha.iter()), [1, 2, 1, 2, 3]);
        ha.init();
    }
}

#[test]
fn xsimpleq_with_and_without_cookie() {
    for cookie in [0usize, 0xdead_beef_cafe_f00d, usize::MAX] {
        let n = nodes::<4>();
        let h = XsimpleqHead::<Xq>::new();
        h.init(cookie);
        assert!(h.is_empty());
        assert_eq!(id(h.first()), None);
        // SAFETY: the nodes outlive the head; each operation's precondition holds.
        unsafe {
            h.insert_tail(&n[1]);
            h.insert_head(&n[0]);
            h.insert_tail(&n[3]);
            h.insert_after(&n[1], &n[2]);
        }
        assert_eq!(ids(h.iter()), [1, 2, 3, 4], "cookie {cookie:#x}");
        assert_eq!(id(h.next(&n[0])), Some(2));
        assert_eq!(id(h.next(&n[3])), None);
        if cookie != 0 {
            // the stored words are not the raw pointers
            assert_ne!(n[0].xq.sqx_next.get(), &n[1] as *const Node as usize);
        }
        // SAFETY: as above.
        unsafe {
            h.remove_head();
            h.remove_after(&n[2]);
            assert_eq!(ids(h.iter()), [2, 3]);
            h.insert_tail(&n[3]);
            assert_eq!(ids(h.iter()), [2, 3, 4]);
            h.remove_head();
            h.remove_head();
            h.remove_head();
        }
        assert!(h.is_empty());
        // SAFETY: empty again; the tail insert goes through the sentinel.
        unsafe { h.insert_tail(&n[0]) };
        assert_eq!(ids(h.iter()), [1]);
    }
}

#[test]
fn tailq() {
    let n = nodes::<5>();
    let h = TailqHead::<Tq>::new();
    assert_eq!(id(h.last()), None);
    // SAFETY: the nodes outlive the head; each operation's precondition holds.
    unsafe {
        h.insert_tail(&n[2]); // into an empty queue
        h.insert_head(&n[0]);
        h.insert_after(&n[0], &n[1]);
        h.insert_tail(&n[4]);
        TailqHead::<Tq>::insert_before(&n[4], &n[3]);
    }
    assert_eq!(ids(h.iter()), [1, 2, 3, 4, 5]);
    assert_eq!(ids(h.iter_reverse()), [5, 4, 3, 2, 1]);
    assert_eq!(id(h.first()), Some(1));
    assert_eq!(id(h.last()), Some(5));
    assert_eq!(id(TailqHead::<Tq>::next(&n[1])), Some(3));
    assert_eq!(id(h.prev(&n[1])), Some(1));
    assert_eq!(id(h.prev(&n[0])), None);
    assert_eq!(id(TailqHead::<Tq>::next(&n[4])), None);
    // SAFETY: as above.
    unsafe {
        h.remove(&n[4]); // last: tqh_last moves back
        assert_eq!(id(h.last()), Some(4));
        h.remove(&n[0]); // first
        assert_eq!(id(h.first()), Some(2));
        assert_eq!(id(h.prev(&n[1])), None);
        h.remove(&n[2]); // middle
    }
    assert_eq!(ids(h.iter()), [2, 4]);
    assert_eq!(ids(h.iter_reverse()), [4, 2]);
    let fresh = Node::new(9);
    // SAFETY: `n[3]` is the last element, `fresh` is unlinked and outlives the head.
    unsafe { h.replace(&n[3], &fresh) };
    assert_eq!(ids(h.iter()), [2, 9]);
    assert_eq!(id(h.last()), Some(9));
    // SAFETY: as above.
    unsafe {
        h.remove(&fresh);
        h.remove(&n[1]);
    }
    assert!(h.is_empty());
    assert_eq!(id(h.last()), None);
    // SAFETY: empty again; both inserts go through the sentinel.
    unsafe {
        h.insert_tail(&n[0]);
        h.insert_tail(&n[1]);
    }
    assert_eq!(ids(h.iter_reverse()), [2, 1]);
}

#[test]
fn tailq_concat_and_reverse_removal() {
    let a = nodes::<3>();
    let b = nodes::<2>();
    let ha = TailqHead::<Tq>::new();
    let hb = TailqHead::<Tq>::new();
    // SAFETY: the nodes outlive the heads and start unlinked.
    unsafe {
        for node in &a {
            ha.insert_tail(node);
        }
        for node in &b {
            hb.insert_tail(node);
        }
        ha.concat(&hb);
    }
    assert_eq!(ids(ha.iter()), [1, 2, 3, 1, 2]);
    assert_eq!(ids(ha.iter_reverse()), [2, 1, 3, 2, 1]);
    assert!(hb.is_empty());
    assert!(ptr::eq(ha.last().map_or(ptr::null(), |e| e), &b[1]));
    for node in ha.iter_reverse() {
        if node.id == 1 {
            // SAFETY: `node` is in `ha`; the iterator already read its predecessor.
            unsafe { ha.remove(node) };
        }
    }
    assert_eq!(ids(ha.iter()), [2, 3, 2]);
}

#[test]
fn stailq() {
    let n = nodes::<4>();
    let h = StailqHead::<St>::new();
    assert_eq!(id(h.last()), None);
    // SAFETY: the nodes outlive the head; each operation's precondition holds.
    unsafe {
        h.insert_tail(&n[1]);
        h.insert_head(&n[0]);
        h.insert_tail(&n[3]);
        h.insert_after(&n[1], &n[2]);
    }
    assert_eq!(ids(h.iter()), [1, 2, 3, 4]);
    assert_eq!(id(h.last()), Some(4));
    assert_eq!(id(StailqHead::<St>::next(&n[2])), Some(4));
    // SAFETY: as above.
    unsafe {
        h.remove(&n[2]); // middle, O(n) path
        assert_eq!(ids(h.iter()), [1, 2, 4]);
        h.remove(&n[3]); // last: stqh_last moves back
        assert_eq!(id(h.last()), Some(2));
        h.remove(&n[0]); // first
        assert_eq!(ids(h.iter()), [2]);
        h.remove_head();
    }
    assert!(h.is_empty());
    assert_eq!(id(h.last()), None);
    let hb = StailqHead::<St>::new();
    // SAFETY: as above; `n[2]` and `n[3]` are unlinked again.
    unsafe {
        hb.insert_tail(&n[2]);
        hb.insert_tail(&n[3]);
        h.concat(&hb);
        h.insert_tail(&n[0]);
    }
    assert_eq!(ids(h.iter()), [3, 4, 1]);
    assert!(hb.is_empty());
}

#[test]
fn one_element_in_several_lists() {
    let n = nodes::<3>();
    let li = ListHead::<Li>::new();
    let tq = TailqHead::<Tq>::new();
    let sq = SimpleqHead::<Sq>::new();
    // SAFETY: the nodes outlive the heads; each node is in at most one list per family.
    unsafe {
        for node in &n {
            li.insert_head(node);
            tq.insert_tail(node);
            sq.insert_tail(node);
        }
        ListHead::<Li>::remove(&n[1]);
    }
    assert_eq!(ids(li.iter()), [3, 1]);
    assert_eq!(ids(tq.iter()), [1, 2, 3]);
    assert_eq!(ids(sq.iter()), [1, 2, 3]);
}

#[test]
fn empty_heads_can_be_moved_and_const_initialised() {
    // A `static` head needs a lock around it (`Cell` is not `Sync`); a `const` is fine.
    const LIST: ListHead<Li> = ListHead::new();
    const ENTRY: TailqEntry<Node> = TailqEntry::new();
    let moved = [SimpleqHead::<Sq>::new(), SimpleqHead::<Sq>::default()];
    assert!(LIST.is_empty());
    assert!(moved.iter().all(SimpleqHead::is_empty));
    assert!(ENTRY.tqe_next.get().is_null());
}

#[cfg(feature = "diagnostic")]
#[test]
fn removed_links_are_poisoned_under_diagnostic() {
    let n = nodes::<2>();
    let h = TailqHead::<Tq>::new();
    // SAFETY: the nodes outlive the head and start unlinked.
    unsafe {
        h.insert_tail(&n[0]);
        h.insert_tail(&n[1]);
        h.remove(&n[0]);
    }
    assert_eq!(n[0].tq.tqe_next.get() as usize, Q_INVALID);
    assert_eq!(n[0].tq.tqe_prev.get() as usize, Q_INVALID);
}
