//! Host tests of the extent manager: creation, placed and searched allocations (alignment,
//! skew, boundary, best and first fit), coalescing, freeing (the four cases and the
//! conflict-tolerant ones), fixed storage and caller descriptors.
//!
//! The descriptor pool is initialized once, by the first `extent_create`, over the memory
//! `setup_real_memory` loads; so the cases run in one test, under one setup.

use super::*;
use crate::kern::subr_pool::tests::setup_real_memory;
use crate::sys::extent::{
    EX_FAST, EX_NOALIGN, EX_NOBOUNDARY, EX_NOWAIT, EXF_NOCOALESCE, extent_fixed_storage_size,
};
use crate::sys::malloc::M_DEVBUF;
use std::boxed::Box;
use std::vec::Vec;

/// The extent's regions, in list order.
fn regions(ex: &Extent) -> Vec<(u64, u64)> {
    ex.ex_regions
        .iter()
        .map(|rp| (rp.er_start.get(), rp.er_end.get()))
        .collect()
}

fn new_extent(start: u64, end: u64, flags: i32) -> &'static Extent {
    extent_create(b"test", start, end, M_DEVBUF, None, EX_WAITOK | flags).expect("extent")
}

fn align_and_overflow_helpers() {
    assert_eq!(extent_align(0x1001, 0x1000, 0), 0x2000);
    assert_eq!(extent_align(0x1000, 0x1000, 0), 0x1000);
    // The skew shifts the grid.
    assert_eq!(extent_align(0x1001, 0x1000, 0x10), 0x1010);
    assert_eq!(extent_align(5, EX_NOALIGN, 0), 5);
    assert!(le_ov(1, 2, 3));
    assert!(!le_ov(2, 2, 3));
    assert!(!le_ov(u64::MAX, 2, u64::MAX));
}

fn create_empty_and_filled() {
    let ex = new_extent(0, 0xff, 0);
    assert!(regions(ex).is_empty());
    assert_eq!(ex.ex_flags.get(), 0);
    assert_eq!((ex.ex_start, ex.ex_end), (0, 0xff));

    let ex = new_extent(0, u64::MAX, EX_FILLED | EX_NOCOALESCE);
    assert_eq!(regions(ex), [(0, u64::MAX)]);
    assert_eq!(ex.ex_flags.get(), EXF_NOCOALESCE);
    // SAFETY: not used afterwards.
    unsafe { extent_destroy(ex) };
}

/// A filled extent with the host bridge's windows freed, as acpipci builds its memory
/// extent from `_CRS`.
fn filled_extent_freed_windows() {
    let ex = new_extent(0, u64::MAX, EX_FILLED);
    extent_free(ex, 0x1000_0000, 0x2eff_0000, EX_WAITOK).expect("free");
    extent_free(ex, 0x80_0000_0000, 0x80_0000_0000, EX_WAITOK).expect("free");
    assert_eq!(
        regions(ex),
        [
            (0, 0x0fff_ffff),
            (0x3eff_0000, 0x7f_ffff_ffff),
            (0x100_0000_0000, u64::MAX)
        ]
    );

    // A BAR in the low window, 64 KiB aligned.
    let a = extent_alloc(ex, 0x4000, 0x1_0000, 0, EX_NOBOUNDARY, EX_NOWAIT).expect("alloc");
    assert_eq!(a, 0x1000_0000);
    // It coalesced with the region before the window.
    assert_eq!(regions(ex)[0], (0, 0x1000_3fff));

    // Giving it back splits nothing: case 3 (end of a region).
    extent_free(ex, a, 0x4000, EX_NOWAIT).expect("free");
    assert_eq!(regions(ex)[0], (0, 0x0fff_ffff));
}

fn alloc_region_conflicts_and_coalescing() {
    let ex = new_extent(0, 0xff, 0);
    extent_alloc_region(ex, 0x10, 0x10, EX_NOWAIT).expect("alloc");
    extent_alloc_region(ex, 0x30, 0x10, EX_NOWAIT).expect("alloc");
    assert_eq!(regions(ex), [(0x10, 0x1f), (0x30, 0x3f)]);

    // Overlap: EAGAIN.
    assert_eq!(
        extent_alloc_region(ex, 0x18, 0x10, EX_NOWAIT),
        Err(Errno::EAGAIN)
    );
    // Outside the extent: EINVAL (no DIAGNOSTIC).
    #[cfg(not(feature = "diagnostic"))]
    assert_eq!(
        extent_alloc_region(ex, 0xf8, 0x10, EX_NOWAIT),
        Err(Errno::EINVAL)
    );

    // Exactly the gap: both neighbours merge into one region.
    extent_alloc_region(ex, 0x20, 0x10, EX_NOWAIT).expect("alloc");
    assert_eq!(regions(ex), [(0x10, 0x3f)]);
    // Before the first region, touching it: prepended.
    extent_alloc_region(ex, 0x0, 0x10, EX_NOWAIT).expect("alloc");
    assert_eq!(regions(ex), [(0x0, 0x3f)]);
    // Touching the next only.
    extent_alloc_region(ex, 0x50, 0x10, EX_NOWAIT).expect("alloc");
    extent_alloc_region(ex, 0x48, 0x8, EX_NOWAIT).expect("alloc");
    assert_eq!(regions(ex), [(0x0, 0x3f), (0x48, 0x5f)]);
}

fn alloc_region_conflictok() {
    let ex = new_extent(0, 0xff, 0);
    extent_alloc_region(ex, 0x10, 0x10, EX_NOWAIT).expect("alloc");
    extent_alloc_region(ex, 0x40, 0x10, EX_NOWAIT).expect("alloc");
    // Covers the first region's tail, the gap, the whole second region and more: the
    // whole range ends up allocated.
    extent_alloc_region(ex, 0x18, 0x48, EX_NOWAIT | EX_CONFLICTOK).expect("alloc");
    assert_eq!(regions(ex), [(0x10, 0x5f)]);
}

fn alloc_best_fit_and_first_fit() {
    let ex = new_extent(0, 0xff, 0);
    // Gaps: [0x10, 0x2f] (32), [0x40, 0x4f] (16), [0x60, 0xff] (160).
    extent_alloc_region(ex, 0x0, 0x10, EX_NOWAIT).expect("alloc");
    extent_alloc_region(ex, 0x30, 0x10, EX_NOWAIT).expect("alloc");
    extent_alloc_region(ex, 0x50, 0x10, EX_NOWAIT).expect("alloc");

    // Best fit: the 16-byte gap, an exact match.
    let a = extent_alloc(ex, 0x10, EX_NOALIGN, 0, EX_NOBOUNDARY, EX_NOWAIT).expect("alloc");
    assert_eq!(a, 0x40);
    // First fit takes the first gap that holds it.
    let b =
        extent_alloc(ex, 0x8, EX_NOALIGN, 0, EX_NOBOUNDARY, EX_NOWAIT | EX_FAST).expect("alloc");
    assert_eq!(b, 0x10);
    // Best fit for 8 bytes: the 24-byte rest of the first gap beats the tail.
    let c = extent_alloc(ex, 0x8, EX_NOALIGN, 0, EX_NOBOUNDARY, EX_NOWAIT).expect("alloc");
    assert_eq!(c, 0x18);
    // Too big: EAGAIN.
    assert_eq!(
        extent_alloc(ex, 0x100, EX_NOALIGN, 0, EX_NOBOUNDARY, EX_NOWAIT),
        Err(Errno::EAGAIN)
    );
}

fn alloc_alignment_skew_subregion() {
    let ex = new_extent(0, 0xffff, 0);
    let a = extent_alloc_subregion(ex, 0x101, 0xffff, 0x10, 0x100, 0, 0, EX_NOWAIT).expect("alloc");
    assert_eq!(a, 0x200);
    let b =
        extent_alloc_subregion(ex, 0x101, 0xffff, 0x10, 0x100, 0x8, 0, EX_NOWAIT).expect("alloc");
    assert_eq!(b, 0x108);
    // Only up to subend.
    assert_eq!(
        extent_alloc_subregion(ex, 0x0, 0xf, 0x20, 1, 0, 0, EX_NOWAIT),
        Err(Errno::EAGAIN)
    );
}

fn alloc_boundary() {
    let ex = new_extent(0, 0xffff, 0);
    extent_alloc_region(ex, 0, 0xf0, EX_NOWAIT).expect("alloc");
    // 0x20 bytes at 0xf0 would cross 0x100: moved to the boundary.
    let a = extent_alloc(ex, 0x20, EX_NOALIGN, 0, 0x100, EX_NOWAIT | EX_FAST).expect("alloc");
    assert_eq!(a, 0x100);
    // Without a boundary it fits right after the first region.
    let b =
        extent_alloc(ex, 0x10, EX_NOALIGN, 0, EX_NOBOUNDARY, EX_NOWAIT | EX_FAST).expect("alloc");
    assert_eq!(b, 0xf0);
}

fn free_cases() {
    let ex = new_extent(0, 0xff, 0);
    extent_alloc_region(ex, 0x10, 0x40, EX_NOWAIT).expect("alloc");
    // Case 4: the middle, the region splits.
    extent_free(ex, 0x20, 0x10, EX_NOWAIT).expect("free");
    assert_eq!(regions(ex), [(0x10, 0x1f), (0x30, 0x4f)]);
    // Case 2: the start.
    extent_free(ex, 0x30, 0x8, EX_NOWAIT).expect("free");
    assert_eq!(regions(ex), [(0x10, 0x1f), (0x38, 0x4f)]);
    // Case 3: the end.
    extent_free(ex, 0x48, 0x8, EX_NOWAIT).expect("free");
    assert_eq!(regions(ex), [(0x10, 0x1f), (0x38, 0x47)]);
    // Case 1: a whole region.
    extent_free(ex, 0x10, 0x10, EX_NOWAIT).expect("free");
    assert_eq!(regions(ex), [(0x38, 0x47)]);
    // Conflict-tolerant: across a region's start (2a) and past what is allocated.
    extent_alloc_region(ex, 0x60, 0x10, EX_NOWAIT).expect("alloc");
    extent_free(ex, 0x30, 0x10, EX_NOWAIT | EX_CONFLICTOK).expect("free");
    assert_eq!(regions(ex), [(0x40, 0x47), (0x60, 0x6f)]);
    // 1a and 2a: a range covering one region and the second's head.
    extent_free(ex, 0x3c, 0x2c, EX_NOWAIT | EX_CONFLICTOK).expect("free");
    assert_eq!(regions(ex), [(0x68, 0x6f)]);
    // Nothing there at all is fine with EX_CONFLICTOK.
    extent_free(ex, 0x0, 0x10, EX_NOWAIT | EX_CONFLICTOK).expect("free");
    assert_eq!(regions(ex), [(0x68, 0x6f)]);
}

fn nocoalesce_with_descr() {
    let ex = new_extent(0, 0xff, EX_NOCOALESCE);
    let r1: &'static ExtentRegion = Box::leak(Box::new(ExtentRegion::new()));
    let r2: &'static ExtentRegion = Box::leak(Box::new(ExtentRegion::new()));
    // SAFETY: fresh descriptors, in no list, leaked for good.
    unsafe {
        extent_alloc_region_with_descr(ex, 0x10, 0x10, EX_NOWAIT, r1).expect("alloc");
        let a = extent_alloc_with_descr(ex, 0x10, EX_NOALIGN, 0, EX_NOBOUNDARY, EX_FAST, r2)
            .expect("alloc");
        assert_eq!(a, 0);
    }
    // Neighbours stay apart.
    assert_eq!(regions(ex), [(0x0, 0xf), (0x10, 0x1f)]);
    assert_eq!(r1.er_flags.get(), ER_DISCARD);
    // Only whole regions can be freed; a caller's descriptor is not given to the pool.
    extent_free(ex, 0x10, 0x10, EX_NOWAIT).expect("free");
    assert_eq!(regions(ex), [(0x0, 0xf)]);
}

fn fixed_storage() {
    // Room for the extent and one descriptor after 7 bytes of padding: the storage starts
    // one byte past an 8-byte boundary, and is aligned up.
    let len = 7 + extent_fixed_storage_size(1);
    let words: &'static mut [u64] = Box::leak(std::vec![0xa5a5u64; len / 8 + 2].into_boxed_slice());
    // SAFETY: the words' bytes, leaked for good; u8 has no alignment.
    let bytes: &'static mut [u8] = unsafe {
        core::slice::from_raw_parts_mut(words.as_mut_ptr().cast::<u8>(), words.len() * 8)
    };
    let storage = &mut bytes[1..1 + len];
    let ex = extent_create(b"fixed", 0, 0xff, M_DEVBUF, Some(storage), EX_NOWAIT).expect("extent");
    assert_ne!(ex.ex_flags.get() & EXF_FIXED, 0);
    let fex = extent_fixed(ex);
    assert_eq!(fex.fex_freelist.iter().count(), 1);

    // One descriptor left after the padding: the second allocation has none.
    extent_alloc_region(ex, 0x0, 0x10, EX_NOWAIT).expect("alloc");
    assert_eq!(
        extent_alloc_region(ex, 0x80, 0x10, EX_NOWAIT),
        Err(Errno::ENOMEM)
    );
    // EX_MALLOCOK falls back to the pool.
    extent_alloc_region(ex, 0x80, 0x10, EX_NOWAIT | EX_MALLOCOK).expect("alloc");
    assert_eq!(regions(ex), [(0x0, 0xf), (0x80, 0x8f)]);
    // A storage descriptor goes back to the freelist; a pool one to the pool.
    extent_free(ex, 0x80, 0x10, EX_NOWAIT | EX_MALLOCOK).expect("free");
    extent_free(ex, 0x0, 0x10, EX_NOWAIT | EX_MALLOCOK).expect("free");
    assert!(regions(ex).is_empty());
    assert!(fex.fex_freelist.iter().count() >= 1);
}

fn registered_and_printed() {
    let ex = new_extent(0x100, 0x1ff, EX_FILLED);
    assert!(EXT_LIST.0.iter().any(|ep| ptr::eq(ep, ex)));
    extent_print(ex);
    // SAFETY: not used afterwards.
    unsafe { extent_destroy(ex) };
    assert!(!EXT_LIST.0.iter().any(|ep| ptr::eq(ep, ex)));
}

#[test]
fn extent_cases() {
    let _g = setup_real_memory();
    align_and_overflow_helpers();
    create_empty_and_filled();
    filled_extent_freed_windows();
    alloc_region_conflicts_and_coalescing();
    alloc_region_conflictok();
    alloc_best_fit_and_first_fit();
    alloc_alignment_skew_subregion();
    alloc_boundary();
    free_cases();
    nocoalesce_with_descr();
    fixed_storage();
    registered_and_printed();
}
