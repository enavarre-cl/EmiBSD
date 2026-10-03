//! Host tests of the virtio core: a queue built by hand over heap memory, a test transport
//! that counts kicks, and the device's side of the rings written by the test.

use super::*;

use core::cell::Cell;
use core::ffi::c_void;
use std::alloc::{Layout, alloc_zeroed};
use std::boxed::Box;
use std::thread_local;

use crate::dev::pv::virtiovar::{VirtioAttachArgs, VirtioOps};
use crate::machine::cpu::CpuInfo;
use crate::machine::intr::IntrFn;

thread_local! {
    static KICKS: Cell<u32> = const { Cell::new(0) };
    static DONE: Cell<u32> = const { Cell::new(0) };
}

fn t_kick(_sc: &VirtioSoftc, _idx: u16) {
    KICKS.with(|k| k.set(k.get() + 1));
}
fn t_r1(_sc: &VirtioSoftc, _o: i32) -> u8 {
    0
}
fn t_r2(_sc: &VirtioSoftc, _o: i32) -> u16 {
    0
}
fn t_r4(_sc: &VirtioSoftc, _o: i32) -> u32 {
    0
}
fn t_r8(_sc: &VirtioSoftc, _o: i32) -> u64 {
    0
}
fn t_w1(_sc: &VirtioSoftc, _o: i32, _v: u8) {}
fn t_w2(_sc: &VirtioSoftc, _o: i32, _v: u16) {}
fn t_w4(_sc: &VirtioSoftc, _o: i32, _v: u32) {}
fn t_w8(_sc: &VirtioSoftc, _o: i32, _v: u64) {}
fn t_qsize(_sc: &VirtioSoftc, _i: u16) -> u16 {
    QSIZE as u16
}
fn t_setup_queue(_sc: &VirtioSoftc, _vq: &Virtqueue, _addr: u64) {}
fn t_setup_intrs(_sc: &VirtioSoftc) {}
fn t_get_status(_sc: &VirtioSoftc) -> i32 {
    0
}
fn t_set_status(_sc: &VirtioSoftc, _s: i32) {}
fn t_neg(_sc: &VirtioSoftc, _n: Option<&[VirtioFeatureName]>) -> Result<(), Errno> {
    Ok(())
}
fn t_finish(_sc: &VirtioSoftc, _va: *mut VirtioAttachArgs) -> Result<(), Errno> {
    Ok(())
}
fn t_poll(_arg: *mut c_void) -> i32 {
    0
}
fn t_barrier(_sc: &VirtioSoftc) {}
fn t_establish(
    _sc: &VirtioSoftc,
    _va: *mut VirtioAttachArgs,
    _vec: i32,
    _ci: Option<&'static CpuInfo>,
    _f: IntrFn,
    _a: *mut c_void,
) -> Result<(), Errno> {
    Err(Errno::ENXIO)
}

static TEST_OPS: VirtioOps = VirtioOps {
    kick: t_kick,
    read_dev_cfg_1: t_r1,
    read_dev_cfg_2: t_r2,
    read_dev_cfg_4: t_r4,
    read_dev_cfg_8: t_r8,
    write_dev_cfg_1: t_w1,
    write_dev_cfg_2: t_w2,
    write_dev_cfg_4: t_w4,
    write_dev_cfg_8: t_w8,
    read_queue_size: t_qsize,
    setup_queue: t_setup_queue,
    setup_intrs: t_setup_intrs,
    get_status: t_get_status,
    set_status: t_set_status,
    neg_features: t_neg,
    attach_finish: t_finish,
    poll_intr: t_poll,
    intr_barrier: t_barrier,
    intr_establish: t_establish,
};

const QSIZE: usize = 8;
const MAXNSEGS: usize = 4;

/// A leaked, zero-filled `T` (all-zero is valid for the virtio structures).
fn zeroed<T>(n: usize) -> *mut T {
    let layout = Layout::array::<T>(n).unwrap();
    // SAFETY: a non-zero layout.
    let p = unsafe { alloc_zeroed(layout) };
    assert!(!p.is_null());
    p.cast()
}

fn map(addr: usize, len: usize) -> &'static BusDmamap {
    Box::leak(Box::new(BusDmamap {
        dm_mapsize: Cell::new(len),
        dm_nsegs: Cell::new(1),
        segs: [Cell::new(BusDmaSegment {
            ds_addr: addr,
            ds_len: len,
        })],
    }))
}

/// A softc with one queue of `QSIZE` slots laid out as `virtio_alloc_vq` lays it out
/// (event indexes possible, indirect tables of `MAXNSEGS` when `indirect`).
fn fixture(features: u64, indirect: bool) -> (&'static VirtioSoftc, &'static Virtqueue) {
    // SAFETY: leaked zeroed blocks; all-zero is a valid softc and queue.
    let sc: &'static VirtioSoftc = unsafe { &*zeroed::<VirtioSoftc>(1) };
    // SAFETY: as above.
    let vq: &'static Virtqueue = unsafe { &*zeroed::<Virtqueue>(1) };
    sc.sc_ops.set(Some(&TEST_OPS));
    sc.sc_dmat
        .set(Some(crate::machine::bus::BusDmaTag::default()));
    sc.sc_active_features.set(features);
    sc.sc_vqs.set(core::ptr::from_ref(vq).cast_mut());
    sc.sc_nvqs.set(1);

    let hdrlen = 3;
    let a1 = virtqueue_align(16 * QSIZE + 2 * (hdrlen + QSIZE));
    let a2 = virtqueue_align(2 * hdrlen + 8 * QSIZE);
    let a3 = if indirect { 16 * MAXNSEGS * QSIZE } else { 0 };
    let size = a1 + a2 + a3;
    let mem = zeroed::<u64>(size / 8).cast::<u8>();

    vq.vq_owner.set(core::ptr::from_ref(sc));
    vq.vq_num.set(QSIZE as u32);
    vq.vq_mask.set(QSIZE as u32 - 1);
    vq.vq_desc.set(mem.cast());
    vq.vq_availoffset.set((16 * QSIZE) as i32);
    // SAFETY: offsets inside the `size` bytes of `mem`.
    vq.vq_avail.set(unsafe { mem.add(16 * QSIZE) }.cast());
    vq.vq_usedoffset.set(a1 as i32);
    // SAFETY: as above.
    vq.vq_used.set(unsafe { mem.add(a1) }.cast());
    if indirect {
        vq.vq_indirectoffset.set((a1 + a2) as i32);
        // SAFETY: as above.
        vq.vq_indirect.set(unsafe { mem.add(a1 + a2) }.cast());
    }
    vq.vq_vaddr.set(mem);
    vq.vq_bytesize.set(size as u32);
    vq.vq_maxnsegs
        .set(if indirect { MAXNSEGS as i32 } else { 1 });
    vq.vq_dmamap.set(Some(map(0x10_0000, size)));
    vq.vq_entries.set(zeroed::<VqEntry>(QSIZE));
    virtio_init_vq(sc, vq);
    (sc, vq)
}

fn desc(vq: &Virtqueue, base: *mut VringDesc, i: usize) -> VringDesc {
    let _ = vq;
    // SAFETY: `base` is one of the fixture's descriptor tables and `i` inside it.
    unsafe { ptr::read_volatile(base.add(i)) }
}

/// The device consumed `slot` and wrote `len` bytes: the used ring's next element.
fn device_uses(vq: &Virtqueue, slot: u32, len: u32) {
    let u = vq.vq_used.get().cast::<u8>();
    // SAFETY: the fixture's used ring, `QSIZE` elements after the header.
    unsafe {
        let idx = ptr::read_volatile(u.add(2).cast::<u16>());
        let e = u
            .add(VRING_USED_RING + 8 * (idx as usize % QSIZE))
            .cast::<VringUsedElem>();
        ptr::write_volatile(e, VringUsedElem { id: slot, len });
        ptr::write_volatile(u.add(2).cast::<u16>(), idx.wrapping_add(1));
    }
}

fn free_slots(vq: &Virtqueue) -> usize {
    vq.vq_freelist.iter().count()
}

#[test]
fn init_puts_every_slot_on_the_free_list_in_order() {
    let (_sc, vq) = fixture(0, false);
    let order: std::vec::Vec<u16> = vq.vq_freelist.iter().map(|e| e.qe_index.get()).collect();
    assert_eq!(order, (0..QSIZE as u16).collect::<std::vec::Vec<_>>());
    assert_eq!(vq.vq_queued.get(), 1);
}

#[test]
fn a_direct_chain_is_enqueued_committed_and_dequeued() {
    let (sc, vq) = fixture(0, false);
    KICKS.with(|k| k.set(0));

    let slot = virtio_enqueue_prep(vq).unwrap();
    assert_eq!(slot, 0);
    virtio_enqueue_reserve(vq, slot, 3).unwrap();
    assert_eq!(free_slots(vq), QSIZE - 3);
    let d = vq.vq_desc.get();
    assert_eq!(desc(vq, d, 0).flags, VRING_DESC_F_NEXT);
    assert_eq!(desc(vq, d, 0).next, 1);
    assert_eq!(desc(vq, d, 1).flags, VRING_DESC_F_NEXT);
    assert_eq!(desc(vq, d, 1).next, 2);
    assert_eq!(desc(vq, d, 2).flags, 0);

    let hdr = map(0x2000, 12);
    let payload = map(0x3000, 1500);
    virtio_enqueue_p(vq, slot, hdr, 0, 12, true);
    virtio_enqueue(vq, slot, payload, true);
    let rx = map(0x4000, 2048);
    virtio_enqueue(vq, slot, rx, false);
    assert_eq!(desc(vq, d, 0).addr, 0x2000);
    assert_eq!(desc(vq, d, 0).len, 12);
    assert_eq!(desc(vq, d, 1).addr, 0x3000);
    assert_eq!(desc(vq, d, 2).flags, VRING_DESC_F_WRITE);

    virtio_enqueue_commit(sc, vq, slot, true);
    assert_eq!(vq.avail_ring(0), 0);
    assert_eq!(vq.avail_idx(), 1);
    assert_eq!(KICKS.with(Cell::get), 1);

    // Nothing used yet.
    assert_eq!(virtio_dequeue(sc, vq), Err(Errno::ENOENT));

    device_uses(vq, 0, 100);
    fn done(_vq: &Virtqueue) -> i32 {
        DONE.with(|d| d.set(d.get() + 1));
        1
    }
    vq.vq_done.set(Some(done));
    DONE.with(|d| d.set(0));
    assert_eq!(virtio_check_vqs(sc), 1);
    assert_eq!(DONE.with(Cell::get), 1);

    assert_eq!(virtio_dequeue(sc, vq), Ok((0, 100)));
    assert_eq!(virtio_dequeue_commit(vq, 0), 3);
    assert_eq!(free_slots(vq), QSIZE);
    assert_eq!(virtio_dequeue(sc, vq), Err(Errno::ENOENT));
}

#[test]
fn reserve_without_enough_slots_aborts_and_frees() {
    let (_sc, vq) = fixture(0, false);
    for _ in 0..6 {
        let s = virtio_enqueue_prep(vq).unwrap();
        virtio_enqueue_reserve(vq, s, 1).unwrap();
    }
    let s = virtio_enqueue_prep(vq).unwrap();
    assert_eq!(free_slots(vq), 1);
    assert_eq!(virtio_enqueue_reserve(vq, s, 3), Err(Errno::EAGAIN));
    // The head and the one extra slot it took are back.
    assert_eq!(free_slots(vq), 2);

    let s = virtio_enqueue_prep(vq).unwrap();
    virtio_enqueue_abort(vq, s);
    assert_eq!(free_slots(vq), 2);
}

#[test]
fn an_indirect_request_uses_one_ring_slot() {
    let (sc, vq) = fixture(0, true);
    // virtio_init_vq links every slot's indirect table.
    let ind = vq.vq_indirect.get();
    for j in 0..MAXNSEGS - 1 {
        assert_eq!(desc(vq, ind, MAXNSEGS + j).next, (j + 1) as u16);
    }

    let s0 = virtio_enqueue_prep(vq).unwrap();
    let slot = virtio_enqueue_prep(vq).unwrap();
    assert_eq!(slot, 1);
    virtio_enqueue_reserve(vq, slot, 3).unwrap();
    assert_eq!(free_slots(vq), QSIZE - 2);
    let d = desc(vq, vq.vq_desc.get(), 1);
    assert_eq!(d.flags, VRING_DESC_F_INDIRECT);
    assert_eq!(d.len, 48);
    assert_eq!(
        d.addr,
        0x10_0000 + vq.vq_indirectoffset.get() as u64 + (16 * MAXNSEGS) as u64
    );
    let base = vq.entry(1).qe_desc_base.get();
    assert_eq!(desc(vq, base, 0).flags, VRING_DESC_F_NEXT);
    assert_eq!(desc(vq, base, 1).flags, VRING_DESC_F_NEXT);
    assert_eq!(desc(vq, base, 2).flags, 0);

    virtio_enqueue_trim(vq, slot, 2);
    assert_eq!(desc(vq, vq.vq_desc.get(), 1).len, 32);
    assert_eq!(desc(vq, base, 1).flags, 0);

    virtio_enqueue_commit(sc, vq, slot, false);
    device_uses(vq, 1, 0);
    assert_eq!(virtio_dequeue(sc, vq), Ok((1, 0)));
    assert_eq!(virtio_dequeue_commit(vq, 1), 1);
    virtio_enqueue_abort(vq, s0);
    assert_eq!(free_slots(vq), QSIZE);
}

#[test]
fn event_index_decides_the_kick() {
    let (sc, vq) = fixture(VIRTIO_F_RING_EVENT_IDX, false);
    KICKS.with(|k| k.set(0));

    // The device asked to be notified when entry 0 is published (avail_event 0).
    let s = virtio_enqueue_prep(vq).unwrap();
    virtio_enqueue_reserve(vq, s, 1).unwrap();
    virtio_enqueue_commit(sc, vq, s, true);
    assert_eq!(KICKS.with(Cell::get), 1);

    // It has not moved its event index: publishing entry 1 needs no kick.
    let s = virtio_enqueue_prep(vq).unwrap();
    virtio_enqueue_reserve(vq, s, 1).unwrap();
    virtio_enqueue_commit(sc, vq, s, true);
    assert_eq!(KICKS.with(Cell::get), 1);
    assert_eq!(vq.avail_idx(), 2);
}

#[test]
fn interrupt_suppression_flags_and_event_indexes() {
    let (sc, vq) = fixture(0, false);
    virtio_stop_vq_intr(sc, vq);
    assert_eq!(
        vq.avail_flags() & VRING_AVAIL_F_NO_INTERRUPT,
        VRING_AVAIL_F_NO_INTERRUPT
    );
    assert!(!virtio_start_vq_intr(sc, vq));
    assert_eq!(vq.avail_flags() & VRING_AVAIL_F_NO_INTERRUPT, 0);
    device_uses(vq, 0, 0);
    assert!(virtio_start_vq_intr(sc, vq));

    let (sc, vq) = fixture(VIRTIO_F_RING_EVENT_IDX, false);
    virtio_stop_vq_intr(sc, vq);
    assert_eq!(vq.vq_used_event(), 0x8000);
    assert!(!virtio_start_vq_intr(sc, vq));
    assert_eq!(vq.vq_used_event(), 0);
    assert!(!virtio_postpone_intr(vq, 3));
    assert_eq!(vq.vq_used_event(), 3);
    device_uses(vq, 0, 0);
    device_uses(vq, 1, 0);
    assert_eq!(virtio_nused(vq), 2);
    assert!(virtio_postpone_intr(vq, 1));
}

#[test]
fn device_names() {
    assert_eq!(virtio_device_string(1), "Network");
    assert_eq!(virtio_device_string(2), "Block");
    assert_eq!(virtio_device_string(0), "Unknown (0)");
    assert_eq!(virtio_device_string(12), "(null)");
    assert_eq!(virtio_device_string(16), "GPU");
    assert_eq!(virtio_device_string(17), "Unknown");
    assert_eq!(virtio_device_string(-1), "Unknown");
}
