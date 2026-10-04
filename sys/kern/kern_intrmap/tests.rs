//! Host tests for interrupt maps: the count and grid choice over CPU sets of several sizes
//! (built here, as `intrmap_cpus_get` would on a machine with that many cores), and
//! `intrmap_create` over the host's one CPU.

use core::mem::MaybeUninit;
use std::vec;
use std::vec::Vec;

use super::*;
use crate::kern::subr_pool::tests::setup_real_memory;
use crate::machine::cpu::curcpu;

/// A CPU set of `count` entries (all the host's CPU), holding the global's reference.
fn fake_cpus(count: u32) -> NonNull<IntrmapCpus> {
    let cpumap =
        intrmap_mallocarray(count as usize, size_of::<&CpuInfo>(), 0).cast::<&'static CpuInfo>();
    for i in 0..count as usize {
        // SAFETY: inside the array just allocated.
        unsafe { cpumap.as_ptr().add(i).write(curcpu()) };
    }
    let ic = intrmap_malloc(size_of::<IntrmapCpus>(), 0).cast::<IntrmapCpus>();
    // SAFETY: a fresh block of the struct's size.
    unsafe {
        ic.as_ptr().write(IntrmapCpus {
            ic_refs: Refcnt::new(),
            ic_count: count,
            ic_cpumap: cpumap,
        })
    };
    ic
}

/// `intrmap_create` over a set of `count` CPUs for unit `unit`: the count, grid and map.
fn create(count: u32, unit: u32, nintrs: u32, maxintrs: u32, flags: u32) -> (u32, u32, Vec<u32>) {
    let ic = fake_cpus(count);
    // SAFETY: the set is alive; the map takes this second reference.
    refcnt_take(&unsafe { ic.as_ref() }.ic_refs);
    let im = intrmap_create_cpus(ic, unit, nintrs, maxintrs, flags);
    let got = (intrmap_count(im), im.im_grid, im.cpumap().to_vec());
    for ring in 0..intrmap_count(im) {
        assert!(ptr::eq(intrmap_cpu(im, ring), curcpu()));
    }
    // SAFETY: made just above and not used afterwards.
    unsafe { intrmap_destroy(im) };
    // SAFETY: the global's reference keeps the set alive.
    let ic_ref = unsafe { ic.as_ref() };
    assert_eq!(ic_ref.ic_refs.r_refs.load(Ordering::Relaxed), 1);
    // SAFETY: the global's reference, the last one: the set is freed.
    unsafe { intrmap_cpus_put(ic.as_ptr()) };
    got
}

#[test]
fn rings_spread_over_the_cpus_by_unit() {
    let _g = setup_real_memory();
    // One ring per device: unit n on CPU n.
    assert_eq!(create(4, 0, 1, 8, 0), (1, 1, vec![0]));
    assert_eq!(create(4, 1, 1, 8, 0), (1, 1, vec![1]));
    // As many rings as CPUs: every unit uses them all.
    assert_eq!(create(4, 0, 4, 8, 0), (4, 4, vec![0, 1, 2, 3]));
    assert_eq!(create(8, 1, 16, 16, 0), (8, 8, (0..8).collect()));
    // Two rings: groups of two CPUs, unit 1 takes the second group.
    assert_eq!(create(4, 1, 2, 8, 0), (2, 2, vec![2, 3]));
    assert_eq!(create(6, 1, 2, 8, 0), (2, 2, vec![2, 3]));
    // Not a divisor: the grid is the whole set.
    assert_eq!(create(4, 0, 0, 3, 0), (3, 4, vec![0, 1, 2]));
}

#[test]
fn counts_are_bounded_and_rounded() {
    let _g = setup_real_memory();
    // 0 asks for maxintrs; never more than the CPUs.
    assert_eq!(create(1, 5, 0, 4, 0), (1, 1, vec![0]));
    assert_eq!(create(2, 0, 9, 4, 0), (2, 2, vec![0, 1]));
    // INTRMAP_POWEROF2 rounds 3 down to 2.
    assert_eq!(create(4, 0, 0, 3, INTRMAP_POWEROF2), (2, 2, vec![0, 1]));
    assert_eq!(create(4, 3, 0, 3, INTRMAP_POWEROF2), (2, 2, vec![2, 3]));
}

#[test]
fn intrmap_create_uses_the_running_cpus() {
    let _g = setup_real_memory();
    // SAFETY: a `struct device` is valid as all-zero bits (`config_make_softc` zeroes it).
    let dv: Device = unsafe { MaybeUninit::zeroed().assume_init() };
    dv.dv_unit.set(2);
    let im = intrmap_create(&dv, 0, 8, 0);
    assert_eq!(intrmap_count(im), NCPUS.load(Ordering::Relaxed) as u32);
    assert!(ptr::eq(intrmap_cpu(im, 0), curcpu()));
    let ic = im.im_cpus;
    // The same set again while ncpus has not changed.
    let im2 = intrmap_create(&dv, 1, 1, 0);
    assert_eq!(im2.im_cpus, ic);
    // SAFETY: both made above and not used afterwards.
    unsafe {
        intrmap_destroy(im);
        intrmap_destroy(im2);
    }
}
