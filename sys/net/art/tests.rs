//! Host tests for the ART: insertion, longest-prefix matching, perfect lookups, deletion with
//! the next most specific route taking over, and iteration, on IPv4-sized keys.

use std::sync::MutexGuard;
use std::vec;
use std::vec::Vec;

use super::*;

fn setup() -> (MutexGuard<'static, ()>, &'static Art) {
    let guard = crate::kern::subr_pool::tests::setup_real_memory();
    art_boot();
    let art = art_alloc(32).expect("art");
    (guard, art)
}

/// A node for `a.b.c.d/plen`, inserted; the node `art_insert` answered.
fn insert(art: &Art, addr: [u8; 4], plen: u32) -> &'static ArtNode {
    let an = art_get(&addr, plen).expect("node");
    let got = art_insert(art, an).expect("insert");
    assert!(ptr::eq(got, an), "{addr:?}/{plen} was new");
    got
}

fn matched(art: &Art, addr: [u8; 4]) -> Option<(Vec<u8>, u32)> {
    art_match(art, &addr).map(|an| (an.an_addr.get()[..4].to_vec(), an.an_plen.get()))
}

#[test]
fn bindex_matches_the_papers_formula() {
    // First level, stride 8: a /8 of 10 is index 10 + 256; a /4 of 10 (0000 1010) is 0 + 16.
    assert_eq!(art_bindex(0, 8, &[10, 0, 2, 15], 8), 10 + 256);
    assert_eq!(art_bindex(0, 8, &[10, 0, 2, 15], 4), 16);
    assert_eq!(art_bindex(0, 8, &[0xff, 0, 0, 0], 1), 1 + 2);
    // Second level (offset 8, stride 4): the high nibble of the second byte.
    assert_eq!(art_bindex(8, 4, &[10, 0xa5, 0, 0], 12), 0xa + 16);
    // Last level (offset 28): the low nibble of the fourth byte.
    assert_eq!(art_bindex(28, 4, &[10, 0, 2, 0x0f], 32), 0xf + 16);
}

#[test]
fn longest_prefix_wins() {
    let (_g, art) = setup();
    assert!(art_is_empty(art));
    assert!(art_match(art, &[10, 0, 2, 2]).is_none());

    insert(art, [0, 0, 0, 0], 0);
    insert(art, [10, 0, 0, 0], 8);
    insert(art, [10, 0, 2, 0], 24);
    insert(art, [10, 0, 2, 2], 32);
    insert(art, [192, 168, 0, 0], 16);
    assert!(!art_is_empty(art));

    assert_eq!(matched(art, [10, 0, 2, 2]), Some((vec![10, 0, 2, 2], 32)));
    assert_eq!(matched(art, [10, 0, 2, 3]), Some((vec![10, 0, 2, 0], 24)));
    assert_eq!(matched(art, [10, 9, 9, 9]), Some((vec![10, 0, 0, 0], 8)));
    assert_eq!(
        matched(art, [192, 168, 7, 1]),
        Some((vec![192, 168, 0, 0], 16))
    );
    assert_eq!(matched(art, [8, 8, 8, 8]), Some((vec![0, 0, 0, 0], 0)));

    // Perfect lookups only find the exact prefix.
    assert!(art_lookup(art, &[10, 0, 2, 0], 24).is_some());
    assert!(art_lookup(art, &[10, 0, 2, 0], 23).is_none());
    assert!(art_lookup(art, &[10, 0, 0, 0], 8).is_some());
    assert!(art_lookup(art, &[0, 0, 0, 0], 0).is_some());
    assert!(art_lookup(art, &[10, 0, 2, 2], 32).is_some());
    assert!(art_lookup(art, &[10, 0, 2, 3], 32).is_none());

    // A second node for an existing prefix gets the existing one back.
    let dup = art_get(&[10, 0, 2, 0], 24).expect("node");
    let got = art_insert(art, dup).expect("insert");
    assert!(!ptr::eq(got, dup));
    assert_eq!(got.an_plen.get(), 24);
}

#[test]
fn deletion_hands_the_range_to_the_next_most_specific_prefix() {
    let (_g, art) = setup();
    insert(art, [10, 0, 0, 0], 8);
    insert(art, [10, 0, 2, 0], 24);
    insert(art, [10, 0, 2, 2], 32);

    let an = art_delete(art, &[10, 0, 2, 0], 24).expect("deleted");
    assert_eq!(an.an_plen.get(), 24);
    assert!(
        art_delete(art, &[10, 0, 2, 0], 24).is_none(),
        "already gone"
    );
    assert_eq!(matched(art, [10, 0, 2, 7]), Some((vec![10, 0, 0, 0], 8)));
    assert_eq!(matched(art, [10, 0, 2, 2]), Some((vec![10, 0, 2, 2], 32)));

    art_delete(art, &[10, 0, 2, 2], 32).expect("deleted");
    assert_eq!(matched(art, [10, 0, 2, 2]), Some((vec![10, 0, 0, 0], 8)));

    art_delete(art, &[10, 0, 0, 0], 8).expect("deleted");
    assert!(art_match(art, &[10, 0, 2, 2]).is_none());
    assert!(art_is_empty(art), "the last reference freed every table");
}

#[test]
fn iteration_visits_every_prefix_once() {
    let (_g, art) = setup();
    let prefixes: [([u8; 4], u32); 7] = [
        ([0, 0, 0, 0], 0),
        ([10, 0, 0, 0], 8),
        ([10, 0, 0, 0], 12),
        ([10, 0, 2, 0], 24),
        ([10, 0, 2, 2], 32),
        ([10, 0, 2, 15], 32),
        ([172, 16, 0, 0], 12),
    ];
    for (addr, plen) in prefixes {
        insert(art, addr, plen);
    }

    let mut seen = Vec::new();
    let mut ai = ArtIter::new();
    art_foreach(art, &mut ai, |an| {
        seen.push((an.an_addr.get()[..4].to_vec(), an.an_plen.get()));
        true
    });
    assert_eq!(seen.len(), prefixes.len());
    for (addr, plen) in prefixes {
        assert!(
            seen.contains(&(addr.to_vec(), plen)),
            "{addr:?}/{plen} seen"
        );
    }
    assert!(ai.ai_table.is_null(), "a finished walk holds no table");

    // Stopping early closes the iterator.
    let mut n = 0;
    let mut ai = ArtIter::new();
    art_foreach(art, &mut ai, |_| {
        n += 1;
        n < 3
    });
    assert_eq!(n, 3);
    assert!(ai.ai_table.is_null());

    // The references the walks took are all given back: deleting everything empties the tree.
    for (addr, plen) in prefixes {
        art_delete(art, &addr, plen).expect("deleted");
    }
    assert!(art_is_empty(art));
}
