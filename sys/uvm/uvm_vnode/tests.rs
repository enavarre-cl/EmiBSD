//! Host tests for the vnode pager over `memfs`, a vnode whose `VOP_READ`/`VOP_WRITE` work on
//! an in-memory file: attaching, paging in (with the tail of the last page zeroed), cleaning
//! a dirty page back to the file through `uvm_vnp_sync`, persisting after the last detach,
//! shrinking with `uvm_vnp_setsize`, `uvm_vnp_uncache` and `uvm_vnp_terminate`.

use std::sync::{Mutex, MutexGuard};
use std::vec::Vec;
use std::{assert, assert_eq, vec};

use super::*;
use crate::kern::kern_rwlock::rw_obj_init;
use crate::kern::kern_subr::uiomove;
use crate::kern::subr_xxx::nullop;
use crate::kern::vfs_subr::getnewvnode;
use crate::machine::Machine;
use crate::machine::cpu::Cpu;
use crate::machine::pmap::pmap_map_direct;
use crate::sys::vnode::{
    VREG, VT_NON, VopGetattrArgs, VopInactiveArgs, VopReadArgs, VopWriteArgs, Vops,
};

/// The file's bytes.
static FILE: Mutex<Vec<u8>> = Mutex::new(Vec::new());

fn file<R>(f: impl FnOnce(&mut Vec<u8>) -> R) -> R {
    f(&mut FILE.lock().unwrap_or_else(|e| e.into_inner()))
}

fn memfs_getattr(ap: &mut VopGetattrArgs<'_>) -> Result<(), Errno> {
    *ap.a_vap = Vattr::new();
    ap.a_vap.va_type = VREG;
    ap.a_vap.va_size = file(|f| f.len()) as u64;
    Ok(())
}

fn memfs_read(ap: &mut VopReadArgs<'_, '_>) -> Result<(), Errno> {
    let uio = &mut *ap.a_uio;
    let mut data = file(|f| f.clone());
    let off = (uio.uio_offset as usize).min(data.len());
    let n = uio.uio_resid.min(data.len() - off);
    uiomove(&mut data[off..off + n], uio)
}

fn memfs_write(ap: &mut VopWriteArgs<'_, '_>) -> Result<(), Errno> {
    let uio = &mut *ap.a_uio;
    let off = uio.uio_offset as usize;
    let mut buf = vec![0u8; uio.uio_resid];
    uiomove(&mut buf, uio)?;
    file(|f| {
        if f.len() < off + buf.len() {
            f.resize(off + buf.len(), 0);
        }
        f[off..off + buf.len()].copy_from_slice(&buf);
    });
    Ok(())
}

fn memfs_inactive(ap: &mut VopInactiveArgs<'_>) -> Result<(), Errno> {
    VOP_UNLOCK(ap.a_vp)
}

/// `vops` of `memfs`: no locking, the file in memory.
static MEMFS_VOPS: Vops = Vops {
    vop_lock: Some(|_| nullop()),
    vop_unlock: Some(|_| nullop()),
    vop_islocked: Some(|_| 0),
    vop_inactive: Some(memfs_inactive),
    vop_reclaim: Some(|_| nullop()),
    vop_getattr: Some(memfs_getattr),
    vop_read: Some(memfs_read),
    vop_write: Some(memfs_write),
    ..Vops::EMPTY
};

/// Memory, the vnode table, the pager, a file of `len` bytes (`i % 251`) and its vnode.
fn setup(len: usize) -> (MutexGuard<'static, ()>, &'static Vnode) {
    let (g, p) = crate::kern::vfs_subr::tests::setup();
    rw_obj_init();
    uvn_init();
    Machine::set_curproc(Machine::curcpu(), p);
    file(|f| *f = (0..len).map(|i| (i % 251) as u8).collect());
    let vp = getnewvnode(VT_NON, None, &MEMFS_VOPS).expect("a vnode");
    vp.v_type.set(VREG);
    (g, vp)
}

fn teardown() {
    Machine::set_curproc(Machine::curcpu(), ptr::null());
}

/// The page's bytes through the direct map.
fn bytes(pg: &VmPage) -> &'static mut [u8] {
    let va = pmap_map_direct(pg).as_usize();
    // SAFETY: the test owns the busy page; the host's direct map is the page itself.
    unsafe { core::slice::from_raw_parts_mut(va as *mut u8, PAGE_SIZE) }
}

/// `pgo_get` of the page at `off` without the fault's locks: busy, resident.
fn get(uobj: &UvmObject, off: Voff) -> &'static VmPage {
    let mut pps = [ptr::null::<VmPage>(); 1];
    let mut npages = 1;
    let _ = rw_enter(uobj.vmobjlock(), RW_WRITE);
    let r = uvn_get(uobj, off, &mut pps, &mut npages, 0, PROT_READ, 0, 0);
    assert_eq!(r, VM_PAGER_OK);
    pps_page(pps[0])
}

#[test]
fn attach_get_dirty_sync_and_persist() {
    let (_g, vp) = setup(PAGE_SIZE + 100);
    let use0 = vp.v_usecount.get();

    let uobj = uvn_attach(vp, PROT_READ | PROT_WRITE).expect("attached");
    let uvn = uvn(uobj);
    assert!(vp.v_uvm.get().is_some_and(|u| ptr::eq(u, uvn)));
    assert_eq!(uobj.uo_refs.get(), 1);
    assert_eq!(uvn.u_size.get(), (PAGE_SIZE + 100) as Voff);
    assert!(uvn.u_flags.get() & UVM_VNODE_VALID != 0);
    assert!(uvn.u_flags.get() & UVM_VNODE_WRITEABLE != 0);
    assert_eq!(vp.v_usecount.get(), use0 + 1);
    // A second attach shares the object.
    let again = uvn_attach(vp, PROT_READ).expect("attached");
    assert!(ptr::eq(again, uobj));
    assert_eq!(uobj.uo_refs.get(), 2);
    uvn_detach(uobj);

    // With the fault's locks nothing is resident yet.
    let mut pps = [ptr::null::<VmPage>(); 2];
    let mut npages = 2;
    let _ = rw_enter(uobj.vmobjlock(), RW_WRITE);
    let r = uvn_get(uobj, 0, &mut pps, &mut npages, 0, PROT_READ, 0, PGO_LOCKED);
    rw_exit(uobj.vmobjlock());
    assert_eq!((r, npages), (VM_PAGER_UNLOCK, 0));

    // Paging in reads the file; the second page holds 100 bytes and zeroes.
    let pg0 = get(uobj, 0);
    assert!(pg0.flags() & PG_BUSY != 0 && pg0.flags() & PG_FAKE == 0);
    assert_eq!(bytes(pg0)[..8], [0, 1, 2, 3, 4, 5, 6, 7]);
    let pg1 = get(uobj, PAGE_SIZE as Voff);
    assert_eq!(bytes(pg1)[99], ((PAGE_SIZE + 99) % 251) as u8);
    assert!(bytes(pg1)[100..].iter().all(|&b| b == 0));
    assert_eq!(uobj.uo_npages.get(), 2);

    // Dirty the first page and let the sync write it back.
    bytes(pg0)[..4].copy_from_slice(b"EMI!");
    pg0.clear_bits(PG_CLEAN | PG_BUSY);
    pg1.clear_bits(PG_BUSY);
    uvm_vnp_sync(None);
    assert_eq!(file(|f| f[..4].to_vec()), b"EMI!");
    assert_eq!(file(|f| f.len()), PAGE_SIZE + 100);
    assert!(pg0.flags() & PG_CLEAN != 0 && pg0.flags() & PG_BUSY == 0);
    assert_eq!(uobj.uo_refs.get(), 1);

    // The last detach keeps the object (it persists) and drops the vnode reference.
    uvn_detach(uobj);
    assert_eq!(uobj.uo_refs.get(), 0);
    assert!(uvn.u_flags.get() & UVM_VNODE_VALID != 0);
    assert_eq!(uobj.uo_npages.get(), 2);
    assert_eq!(vp.v_usecount.get(), use0);

    // Re-attaching finds the cached pages.
    let uobj = uvn_attach(vp, PROT_READ).expect("attached");
    assert_eq!(uobj.uo_refs.get(), 1);
    assert!(uvm_pagelookup(uobj, 0).is_some_and(|p| ptr::eq(p, pg0)));

    // Shrinking tosses the pages from the one holding the new end (the C truncates the start
    // to a page).
    uvm_vnp_setsize(vp, PAGE_SIZE as Voff + 10);
    assert_eq!(uobj.uo_npages.get(), 1);
    assert!(uvm_pagelookup(uobj, PAGE_SIZE as Voff).is_none());
    assert_eq!(uvn.u_size.get(), PAGE_SIZE as Voff + 10);

    // Uncaching an active object only stops it persisting.
    assert!(!uvm_vnp_uncache(vp));
    assert!(uvn.u_flags.get() & UVM_VNODE_CANPERSIST == 0);
    uvn_detach(uobj);
    // Without CANPERSIST the last detach kills the object and frees its pages.
    assert_eq!(uvn.u_flags.get(), 0);
    assert_eq!(uobj.uo_npages.get(), 0);
    assert_eq!(vp.v_usecount.get(), use0);
    teardown();
}

#[test]
fn terminate_frees_a_persisting_object() {
    let (_g, vp) = setup(3 * PAGE_SIZE);
    let uobj = uvn_attach(vp, PROT_READ).expect("attached");
    let pg = get(uobj, PAGE_SIZE as Voff);
    pg.clear_bits(PG_BUSY);
    uvn_detach(uobj);
    let uvn = uvn(uobj);
    assert!(uvn.u_flags.get() & UVM_VNODE_VALID != 0);
    assert_eq!(uobj.uo_npages.get(), 1);

    uvm_vnp_terminate(vp);
    assert_eq!(uvn.u_flags.get(), 0);
    assert_eq!(uobj.uo_npages.get(), 0);
    // A dead object is not synced and cannot be uncached.
    uvm_vnp_sync(None);
    assert!(uvm_vnp_uncache(vp));
    teardown();
}

#[test]
fn a_page_past_the_end_is_bad() {
    let (_g, vp) = setup(100);
    let uobj = uvn_attach(vp, PROT_READ).expect("attached");
    let mut pps = [ptr::null::<VmPage>(); 1];
    let mut npages = 1;
    let _ = rw_enter(uobj.vmobjlock(), RW_WRITE);
    let r = uvn_get(
        uobj,
        PAGE_SIZE as Voff,
        &mut pps,
        &mut npages,
        0,
        PROT_READ,
        0,
        0,
    );
    // uvn_io refuses an offset past the end; uvn_get freed the page and dropped the lock.
    assert_eq!(r, VM_PAGER_BAD);
    assert_eq!(uobj.uo_npages.get(), 0);
    let mut lo = 0;
    let mut hi = 0;
    let _ = rw_enter(uobj.vmobjlock(), RW_WRITE);
    uvn_cluster(uobj, 0, &mut lo, &mut hi);
    rw_exit(uobj.vmobjlock());
    assert_eq!((lo, hi), (0, PAGE_SIZE as Voff));
    uvn_detach(uobj);
    teardown();
}
