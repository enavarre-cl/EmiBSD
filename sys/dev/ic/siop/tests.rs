//! Host tests for `siop.rs`: the script memory management (the script copied and patched by
//! `siop_reset`, a target's lun switch, its reselect entry and SCNTL3/SXFER restore, the lun
//! entries with and without a tag switch) on an adapter whose script lives in a page of host
//! memory, as without on-board RAM; the host's `bus_space` reads 0 and drops writes.

use std::alloc::{Layout, alloc_zeroed};
use std::assert_eq;
use std::boxed::Box;

use super::*;
use crate::kern::subr_pool::tests::setup_real_memory;
use crate::machine::bus::{BusDmamap, BusSpaceTag, bus_space_map};

/// The script's bus address in these tests.
const SCRIPTADDR: u32 = 0x1000_0000;

/// A zeroed `T` (an `M_ZERO` allocation), leaked.
///
/// # Safety
///
/// All-zero must be a valid `T`.
unsafe fn leak_zeroed<T>() -> &'static T {
    // SAFETY: a fresh zeroed allocation of `T`'s layout; the caller vouches for zero.
    unsafe { &*alloc_zeroed(Layout::new::<T>()).cast::<T>() }
}

/// The PCI front-end's reset, which has nothing to do here.
fn no_pci_reset(_sc: &SiopCommonSoftc) {}

/// A wide adapter whose script is a page of host memory at bus address `SCRIPTADDR`, reset
/// by `siop_reset`.
fn adapter() -> &'static SiopSoftc {
    // SAFETY: the softc is `Cell`s, queue heads, the iopool and a `Device`, valid as zero
    // bytes; the host's map is `Cell`s of integers and segments.
    let (sc, map) = unsafe { (leak_zeroed::<SiopSoftc>(), leak_zeroed::<BusDmamap>()) };
    let c = &sc.sc_c;
    c.features.set(SF_BUS_WIDE);
    c.ram_size.set(PAGE_SIZE as i32);
    c.clock_div.set(3);
    let page: &'static mut [u32] = Box::leak(std::vec![0u32; PAGE_SIZE / 4].into_boxed_slice());
    c.sc_script.set(page.as_mut_ptr());
    c.sc_scriptaddr.set(SCRIPTADDR as usize);
    c.sc_scriptdma.set(Some(map));
    c.sc_dmat.set(Some(Default::default()));
    c.sc_reset.set(Some(no_pci_reset));
    c.sc_rt.set(Some(BusSpaceTag::default()));
    // SAFETY: the host's bus space is a test double; nothing is mapped.
    c.sc_rh.set(Some(
        unsafe { bus_space_map(BusSpaceTag::default(), 0, 0x100, 0) }.unwrap(),
    ));
    siop_reset(sc);
    sc
}

#[test]
fn reset_copies_and_patches_the_script() {
    let sc = adapter();
    assert_eq!(SIOP_NCMDPB, PAGE_SIZE / 384);
    for (i, &w) in siop_script.iter().enumerate() {
        if !E_abs_msgin_Used.contains(&(i as u32)) {
            assert_eq!(siop_script_read(sc, i as u32), w, "word {i}");
        }
    }
    for &j in &E_abs_msgin_Used {
        assert_eq!(siop_script_read(sc, j), SCRIPTADDR + Ent_msgin_space);
    }
    assert_eq!(sc.script_free_lo.get(), siop_script.len() as u32);
    assert_eq!(sc.script_free_hi.get(), (PAGE_SIZE / 4) as u32);
    assert_eq!(sc.sc_ntargets.get(), 0);
}

#[test]
fn target_and_lun_switches() {
    // siop_get_lunsw malloc(9)s the lun switch.
    let _g = setup_real_memory();
    let sc = adapter();
    let free_lo = sc.script_free_lo.get();

    // siop_scsiprobe's part: a target with its lun switch, in the reselect switch.
    // SAFETY: a target is `Cell`s of integers and `Option`s, valid as zero bytes.
    let target = unsafe { leak_zeroed::<SiopTarget>() };
    target.target_c.id.set((3 << 24) | (2 << 16));
    target.lunsw.set(siop_get_lunsw(sc));
    let lunsw = target.lunsw();
    assert_eq!(lunsw.lunsw_off.get(), free_lo);
    assert_eq!(lunsw.lunsw_size.get(), lun_switch.len() as u32);
    assert_eq!(sc.script_free_lo.get(), free_lo + 12);
    assert_eq!(
        siop_script_read(sc, free_lo + E_abs_lunsw_return_Used[0]),
        SCRIPTADDR + Ent_lunsw_return
    );
    sc.sc_c.targets[2].set(Some(NonNull::from(&target.target_c)));
    siop_add_reselsw(sc, 2);
    let reseloff = Ent_resel_targ0 / 4;
    assert_eq!(target.reseloff.get(), reseloff);
    assert_eq!(siop_script_read(sc, reseloff), 0x800c_0082);
    assert_eq!(
        siop_script_read(sc, reseloff + 1),
        SCRIPTADDR + free_lo * 4 + Ent_lun_switch_entry
    );
    assert_eq!(sc.sc_ntargets.get(), 1);
    // The restore of SCNTL3 (3) and SXFER (0) at the head of the lun switch.
    assert_eq!(siop_script_read(sc, free_lo), 0x7803_0300);
    assert_eq!(siop_script_read(sc, free_lo + 2), 0x7805_0000);

    // lun 0, untagged: its JUMP replaces the switch's trailing INT, which moves down.
    // SAFETY: a lun is `Cell`s, valid as zero bytes.
    let lun0 = unsafe { leak_zeroed::<SiopLun>() };
    target.siop_lun[0].set(Some(NonNull::from(lun0)));
    siop_add_dev(sc, 2, 0);
    let lo = free_lo + 12;
    assert_eq!(lun0.reseloff.get(), lo - 2);
    assert_eq!(lun0.siop_tag[0].reseloff.get(), lo - 2);
    assert_eq!(siop_script_read(sc, lo - 2), 0x800c_0000);
    assert_eq!(siop_script_read(sc, lo), 0x9808_0000);
    assert_eq!(siop_script_read(sc, lo + 1), A_int_resellun);
    assert_eq!(sc.script_free_lo.get(), lo + 2);
    assert_eq!(lunsw.lunsw_size.get(), 14);
    // Twice is harmless.
    siop_add_dev(sc, 2, 0);
    assert_eq!(sc.script_free_lo.get(), lo + 2);

    // lun 1 of a tagged target: a tag switch at the top of the memory.
    target
        .target_c
        .flags
        .set(target.target_c.flags.get() | TARF_TAG);
    // SAFETY: as for lun 0.
    let lun1 = unsafe { leak_zeroed::<SiopLun>() };
    target.siop_lun[1].set(Some(NonNull::from(lun1)));
    siop_add_dev(sc, 2, 1);
    let hi = (PAGE_SIZE / 4) as u32 - tag_switch.len() as u32;
    assert_eq!(sc.script_free_hi.get(), hi);
    assert_eq!(lun1.reseloff.get(), lo);
    assert_eq!(siop_script_read(sc, lo), 0x800c_0001);
    assert_eq!(
        siop_script_read(sc, lo + 1),
        SCRIPTADDR + hi * 4 + Ent_tag_switch_entry
    );
    for (i, t) in lun1.siop_tag.iter().enumerate() {
        assert_eq!(t.reseloff.get(), hi + Ent_resel_tag0 / 4 + i as u32 * 2);
    }
    assert_eq!(siop_script_read(sc, hi + 2), tag_switch[2]);
}
