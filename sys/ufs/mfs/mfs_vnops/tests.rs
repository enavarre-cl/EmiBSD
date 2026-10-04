//! Host tests for the memory file system's I/O: `mfs_doio` against a block of memory (reads,
//! writes, the clamp at the end of the file system, a bad address), and `mfs_strategy`'s two
//! paths (the file system's own process does the I/O at once, any other queues the request
//! for it).

use std::boxed::Box;
use std::vec;
use std::vec::Vec;

use super::*;
use crate::kern::kern_bufq::{bufq_init, bufq_peek};
use crate::kern::vfs_bio::getblk;
use crate::machine::Machine;
use crate::machine::cpu::Cpu;
use crate::sys::buf::{B_DONE, B_WRITEINPROG};
use crate::sys::vnode::VT_MFS;
use crate::ufs::ffs::ffs_vfsops::tests::{setup, teardown};

/// A leaked mfsnode over `mem` (a leaked buffer), as `mfs_mount` sets one up.
fn mfsnode_over(mem: &'static mut [u8]) -> &'static Mfsnode {
    let mfsp: &'static Mfsnode = Box::leak(Box::new(Mfsnode::new()));
    mfsp.mfs_baseoff.set(mem.as_mut_ptr() as usize);
    mfsp.mfs_size.set(mem.len() as i64);
    mfsp
}

/// A leaked buffer of `len` bytes at block `blkno`, for a read or a write.
fn buf_at(blkno: i64, len: usize, read: bool) -> (&'static Buf, &'static mut [u8]) {
    let data: &'static mut [u8] = Box::leak(vec![0u8; len].into_boxed_slice());
    let bp: &'static Buf = Box::leak(Box::new(Buf::new()));
    bp.b_data.set(data.as_mut_ptr());
    bp.b_bcount.set(len as i64);
    bp.b_blkno.set(blkno);
    bp.b_flags.set(if read { B_READ } else { B_WRITEINPROG });
    (bp, data)
}

#[test]
fn mfs_doio_reads_and_writes_memory() {
    let (_g, _p) = setup(Vec::new());
    let mem: &'static mut [u8] = Box::leak(
        (0..4096u32)
            .map(|i| (i * 7) as u8)
            .collect::<Vec<u8>>()
            .into_boxed_slice(),
    );
    let expect: Vec<u8> = mem.to_vec();
    let mfsp = mfsnode_over(mem);

    // A read of the second sector: the bytes at offset 512.
    let (bp, data) = buf_at(1, 1024, true);
    mfs_doio(mfsp, bp);
    assert_eq!(bp.b_error.get(), None);
    assert!(!bp.isset(B_ERROR));
    assert!(bp.isset(B_DONE));
    assert_eq!(bp.b_resid.get(), 0);
    assert_eq!(bp.b_bcount.get(), 1024);
    assert_eq!(&data[..], &expect[512..1536]);

    // A write of the last sector goes to offset 3584 and leaves the rest alone.
    let (bp, data) = buf_at(7, 512, false);
    data.fill(0xa5);
    mfs_doio(mfsp, bp);
    assert_eq!(bp.b_error.get(), None);
    assert!(bp.isset(B_DONE));
    // SAFETY: the memory is leaked, and nothing else reads it while this slice lives.
    let mem = unsafe {
        core::slice::from_raw_parts(
            mfsp.mfs_baseoff.get() as *const u8,
            mfsp.mfs_size.get() as usize,
        )
    };
    assert!(mem[3584..].iter().all(|&b| b == 0xa5));
    assert_eq!(&mem[..3584], &expect[..3584]);
    teardown();
}

#[test]
fn mfs_doio_clamps_at_the_end_and_reports_bad_addresses() {
    let (_g, _p) = setup(Vec::new());
    let mem: &'static mut [u8] = Box::leak(vec![0x11u8; 2048].into_boxed_slice());
    let mfsp = mfsnode_over(mem);

    // 1024 bytes asked at offset 1536: only 512 are left, and b_bcount says so.
    let (bp, data) = buf_at(3, 1024, true);
    mfs_doio(mfsp, bp);
    assert_eq!(bp.b_bcount.get(), 512);
    assert_eq!(bp.b_error.get(), None);
    assert!(data[..512].iter().all(|&b| b == 0x11));
    assert!(data[512..].iter().all(|&b| b == 0));

    // A request at the very end is empty.
    let (bp, _) = buf_at(4, 512, true);
    mfs_doio(mfsp, bp);
    assert_eq!(bp.b_bcount.get(), 0);
    assert_eq!(bp.b_error.get(), None);
    assert!(bp.isset(B_DONE));

    // The process's memory is gone (no such address): the buffer gets the error.
    mfsp.mfs_baseoff.set(0);
    let (bp, _) = buf_at(0, 512, true);
    mfs_doio(mfsp, bp);
    // The host's copyin refuses a null address; offset 0 is that address.
    assert_eq!(bp.b_error.get(), Some(Errno::EFAULT));
    assert!(bp.isset(B_ERROR));
    assert!(bp.isset(B_DONE));
    teardown();
}

#[test]
fn mfs_strategy_does_the_io_in_the_file_system_process_and_queues_for_others() {
    let (_g, p) = setup(Vec::new());
    Machine::set_curproc(Machine::curcpu(), p);
    let mem: &'static mut [u8] = Box::leak(
        (0..8192u32)
            .map(|i| (i % 251) as u8)
            .collect::<Vec<u8>>()
            .into_boxed_slice(),
    );
    let expect: Vec<u8> = mem.to_vec();
    let mfsp = mfsnode_over(mem);
    mfsp.mfs_tid.set(p.p_tid.get());
    let _ = bufq_init(&mfsp.mfs_bufq, crate::sys::buf::BUFQ_FIFO);

    // The "device" vnode, in use.
    let vp = crate::kern::vfs_subr::getnewvnode(VT_MFS, None, &MFS_VOPS).unwrap();
    vp.v_type.set(VBLK);
    assert!(
        crate::kern::vfs_subr::checkalias(vp, crate::sys::types::makedev(255, 900), None).is_none()
    );
    vp.v_data.set(core::ptr::from_ref(mfsp).cast_mut().cast());
    mfsp.mfs_vnode.set(Some(vp));

    // Our own process: done on the spot.
    let bp = getblk(vp, 2, 1024, 0, INFSLP).unwrap();
    bp.set(B_READ);
    mfs_strategy(&mut VopStrategyArgs { a_vp: vp, a_bp: bp }).unwrap();
    assert!(bp.isset(B_DONE));
    assert!(!bufq_peek(&mfsp.mfs_bufq));
    // SAFETY: the buffer is ours and mapped.
    assert_eq!(unsafe { bp.data() }, &expect[1024..2048]);

    // Another process: queued, and nothing is copied until the file system's process serves
    // the queue (what `mfs_start` does).
    mfsp.mfs_tid.set(p.p_tid.get() + 1000);
    let bp = getblk(vp, 4, 1024, 0, INFSLP).unwrap();
    bp.set(B_READ);
    mfs_strategy(&mut VopStrategyArgs { a_vp: vp, a_bp: bp }).unwrap();
    assert!(!bp.isset(B_DONE));
    assert!(bufq_peek(&mfsp.mfs_bufq));
    let queued = bufq_dequeue(&mfsp.mfs_bufq).unwrap();
    assert!(core::ptr::eq(queued, bp));
    mfs_doio(mfsp, queued);
    assert!(bp.isset(B_DONE));
    // SAFETY: as above.
    assert_eq!(unsafe { bp.data() }, &expect[2048..3072]);
    assert!(!bufq_peek(&mfsp.mfs_bufq));

    // The other vops: no ioctls, nothing to check on open, print is quiet.
    assert_eq!(
        mfs_ioctl(&mut VopIoctlArgs {
            a_vp: vp,
            a_command: 0,
            a_data: &mut [],
            a_fflag: 0,
            a_cred: core::ptr::null(),
            a_p: p,
        }),
        Err(Errno::ENOTTY)
    );
    assert_eq!(
        mfs_open(&mut VopOpenArgs {
            a_vp: vp,
            a_mode: 0,
            a_cred: core::ptr::null(),
            a_p: p,
        }),
        Ok(())
    );
    assert_eq!(mfs_print(&mut VopPrintArgs { a_vp: vp }), Ok(()));
    teardown();
}
