/*	$OpenBSD: mfs_vnops.c,v 1.62 2024/10/18 05:52:33 miod Exp $	*/
/*	$NetBSD: mfs_vnops.c,v 1.8 1996/03/17 02:16:32 christos Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1989, 1993
 *	The Regents of the University of California.  All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the University nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE REGENTS AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *	@(#)mfs_vnops.c	8.5 (Berkeley) 7/28/94
 */
/* </LICENSES> */

/* <CODE> */
//! The memory based file system's vnode operations: `mfs_vops` (the operations of its
//! "device" vnode) and `mfs_open`, `mfs_ioctl`, `mfs_strategy`, `mfs_doio`, `mfs_close`,
//! `mfs_inactive`, `mfs_reclaim` and `mfs_print`.
//!
//! Upstream: sys/ufs/mfs/mfs_vnops.c @ 3ce1f3f79392
//!
//! The I/O model: the file system lives in the address space of the process that called
//! `mount(2)` (`mount_mfs(8)`'s child), which stays inside `mfs_start`. `mfs_strategy` queues
//! a request on the mfsnode and wakes that process, which copies the data in or out with
//! `mfs_doio`; when the process itself asks (it unmounts on a signal), the copy is done at once.
//!
//! ## Deviations
//! - `mfs_vops` has `Some(|_| vop_generic_badop())` where the C names `vop_generic_badop`
//!   (a function of no arguments here, see `vfs_default.rs`), and `nullop` slots as
//!   closures, as `spec_vops` does.
//! - `mfs_doio` runs the data through `copyin`/`copyout` over the buffer's `b_bcount`
//!   bytes; a request that starts beyond the end of the file system has `b_bcount` set to
//!   the (negative) remainder as in the C, which this reads as no bytes.
//! - `mfs_print` exists under features `diagnostic` or `debug` (the C's
//!   `DEBUG || DIAGNOSTIC`); without them it does nothing, as the C. `VFSLCKDEBUG` is not
//!   configured.

use core::ptr;
use core::sync::atomic::Ordering;

#[cfg(feature = "diagnostic")]
use crate::kern::kern_bufq::bufq_peek;
use crate::kern::kern_bufq::{bufq_dequeue, bufq_destroy, bufq_queue};
use crate::kern::kern_malloc::free;
use crate::kern::kern_synch::wakeup;
use crate::kern::spec_vnops::spec_fsync;
use crate::kern::subr_prf::panic;
use crate::kern::subr_xxx::nullop;
use crate::kern::vfs_bio::biodone;
use crate::kern::vfs_default::{
    vop_generic_badop, vop_generic_bmap, vop_generic_bwrite, vop_generic_revoke,
};
use crate::kern::vfs_subr::vinvalbuf;
use crate::kern::vfs_vops::VOP_UNLOCK;
use crate::machine::copy::{copyin, copyout};
use crate::machine::cpu::curproc;
use crate::machine::intr::{splbio, splx};
use crate::sys::buf::{B_ERROR, B_READ, Buf};
use crate::sys::errno::Errno;
use crate::sys::malloc::M_MFSNODE;
use crate::sys::param::DEV_BSHIFT;
use crate::sys::systm::INFSLP;
use crate::sys::vnode::{
    V_SAVE, VBLK, VopCloseArgs, VopInactiveArgs, VopIoctlArgs, VopOpenArgs, VopPrintArgs,
    VopReclaimArgs, VopStrategyArgs, Vops,
};
use crate::ufs::mfs::mfsnode::{Mfsnode, vtomfs};

/// `mfs_vops`: mfs vnode operations.
pub static MFS_VOPS: Vops = Vops {
    vop_lookup: Some(|_| vop_generic_badop()),
    vop_create: Some(|_| vop_generic_badop()),
    vop_mknod: Some(|_| vop_generic_badop()),
    vop_open: Some(mfs_open),
    vop_close: Some(mfs_close),
    vop_access: Some(|_| vop_generic_badop()),
    vop_getattr: Some(|_| vop_generic_badop()),
    vop_setattr: Some(|_| vop_generic_badop()),
    vop_read: Some(|_| vop_generic_badop()),
    vop_write: Some(|_| vop_generic_badop()),
    vop_ioctl: Some(mfs_ioctl),
    vop_kqfilter: Some(|_| vop_generic_badop()),
    vop_revoke: Some(vop_generic_revoke),
    vop_fsync: Some(spec_fsync),
    vop_remove: Some(|_| vop_generic_badop()),
    vop_link: Some(|_| vop_generic_badop()),
    vop_rename: Some(|_| vop_generic_badop()),
    vop_mkdir: Some(|_| vop_generic_badop()),
    vop_rmdir: Some(|_| vop_generic_badop()),
    vop_symlink: Some(|_| vop_generic_badop()),
    vop_readdir: Some(|_| vop_generic_badop()),
    vop_readlink: Some(|_| vop_generic_badop()),
    vop_abortop: Some(|_| vop_generic_badop()),
    vop_inactive: Some(mfs_inactive),
    vop_reclaim: Some(mfs_reclaim),
    vop_lock: Some(|_| nullop()),
    vop_unlock: Some(|_| nullop()),
    vop_islocked: Some(|_| 0), // nullop
    vop_bmap: Some(vop_generic_bmap),
    vop_strategy: Some(mfs_strategy),
    vop_print: Some(mfs_print),
    vop_pathconf: Some(|_| vop_generic_badop()),
    vop_advlock: Some(|_| vop_generic_badop()),
    vop_bwrite: Some(vop_generic_bwrite),
};

/// `mfs_open` (`vop_open`): open called to allow memory filesystem to initialize and validate
/// before actual IO. Record our process identifier so we can tell when we are doing I/O to
/// ourself.
pub fn mfs_open(ap: &mut VopOpenArgs<'_>) -> Result<(), Errno> {
    #[cfg(feature = "diagnostic")]
    if ap.a_vp.v_type.get() != VBLK {
        panic(format_args!("mfs_open not VBLK"));
    }
    #[cfg(not(feature = "diagnostic"))]
    let _ = ap;
    Ok(())
}

/// `mfs_ioctl` (`vop_ioctl`): ioctl operation.
pub fn mfs_ioctl(_ap: &mut VopIoctlArgs<'_>) -> Result<(), Errno> {
    Err(Errno::ENOTTY)
}

/// `mfs_strategy` (`vop_strategy`): pass I/O requests to the memory filesystem process.
pub fn mfs_strategy(ap: &mut VopStrategyArgs) -> Result<(), Errno> {
    let bp = ap.a_bp;
    let vp = ap.a_vp;

    if vp.v_type.get() != VBLK || vp.v_usecount.get() == 0 {
        panic(format_args!("mfs_strategy: bad dev"));
    }

    let mfsp = vtomfs(vp);
    if curproc().is_some_and(|p| p.p_tid.get() == mfsp.mfs_tid.get()) {
        mfs_doio(mfsp, bp);
    } else {
        bufq_queue(&mfsp.mfs_bufq, bp);
        wakeup(ptr::from_ref(vp));
    }
    Ok(())
}

/// `mfs_doio`: memory file system I/O. Runs in the file system's process: `mfs_baseoff` is an
/// address in its memory, which `copyin` (a read of the file system) and `copyout` (a write)
/// reach.
pub fn mfs_doio(mfsp: &Mfsnode, bp: &'static Buf) {
    let offset = bp.b_blkno.get() << DEV_BSHIFT;

    if bp.b_bcount.get() > mfsp.mfs_size.get() - offset {
        bp.b_bcount.set(mfsp.mfs_size.get() - offset);
    }

    let base = mfsp.mfs_baseoff.get().wrapping_add(offset as usize);
    // SAFETY: the buffer is busy for the requester, which sleeps in `biowait` until
    // `biodone` below (or is `mfs_close`/`mfs_start` finishing it); buffers handed to a
    // strategy routine are mapped; `data` covers `b_bcount` bytes (none when negative) and
    // nothing else holds a slice of them meanwhile.
    let data = unsafe { bp.data() };
    let error = if bp.isset(B_READ) {
        copyin(base, data)
    } else {
        copyout(data, base)
    };
    bp.b_error.set(error.err());
    if error.is_err() {
        bp.set(B_ERROR);
    } else {
        bp.b_resid.set(0);
    }
    let s = splbio();
    biodone(bp);
    splx(s);
}

/// `mfs_close` (`vop_close`): memory filesystem close routine.
pub fn mfs_close(ap: &mut VopCloseArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let mfsp = vtomfs(vp);

    // Finish any pending I/O requests.
    while let Some(bp) = bufq_dequeue(&mfsp.mfs_bufq) {
        mfs_doio(mfsp, bp);
        wakeup(ptr::from_ref(bp));
    }

    // On last close of a memory filesystem we must invalidate any in core blocks, so that we
    // can free up its vnode.
    vinvalbuf(vp, V_SAVE, ap.a_cred, ap.a_p, 0, INFSLP)?;

    // There should be no way to have any more buffers on this vnode.
    #[cfg(feature = "diagnostic")]
    if bufq_peek(&mfsp.mfs_bufq) {
        crate::kprintf!("mfs_close: dirty buffers\n");
    }

    // Send a request to the filesystem server to exit.
    mfsp.mfs_shutdown.store(1, Ordering::Relaxed);
    wakeup(ptr::from_ref(vp));
    Ok(())
}

/// `mfs_inactive` (`vop_inactive`): memory filesystem inactive routine.
pub fn mfs_inactive(ap: &mut VopInactiveArgs<'_>) -> Result<(), Errno> {
    #[cfg(feature = "diagnostic")]
    {
        let mfsp = vtomfs(ap.a_vp);
        if mfsp.mfs_shutdown.load(Ordering::Relaxed) != 0 && bufq_peek(&mfsp.mfs_bufq) {
            panic(format_args!("mfs_inactive: not inactive"));
        }
    }
    let _ = VOP_UNLOCK(ap.a_vp);
    Ok(())
}

/// `mfs_reclaim` (`vop_reclaim`): reclaim a memory filesystem devvp so that it can be reused.
pub fn mfs_reclaim(ap: &mut VopReclaimArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let mfsp = vtomfs(vp);

    bufq_destroy(&mfsp.mfs_bufq);

    if let Some(data) = ptr::NonNull::new(vp.v_data.get().cast::<u8>()) {
        free(data, M_MFSNODE, size_of::<Mfsnode>());
    }
    vp.v_data.set(ptr::null_mut());
    Ok(())
}

/// `mfs_print` (`vop_print`): print out the contents of an mfsnode.
pub fn mfs_print(ap: &mut VopPrintArgs) -> Result<(), Errno> {
    #[cfg(any(feature = "debug", feature = "diagnostic"))]
    {
        let mfsp = vtomfs(ap.a_vp);

        crate::kprintf!(
            "tag VT_MFS, tid {}, base {:#x}, size {}\n",
            mfsp.mfs_tid.get(),
            mfsp.mfs_baseoff.get(),
            mfsp.mfs_size.get()
        );
    }
    #[cfg(not(any(feature = "debug", feature = "diagnostic")))]
    let _ = ap;
    Ok(())
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for the memory file system's I/O: `mfs_doio` against a block of memory (reads,
    // writes, the clamp at the end of the file system, a bad address), and `mfs_strategy`'s two
    // paths (the file system's own process does the I/O at once, any other queues the request
    // for it).

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
            crate::kern::vfs_subr::checkalias(vp, crate::sys::types::makedev(255, 900), None)
                .is_none()
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
}
/* </TESTS> */
