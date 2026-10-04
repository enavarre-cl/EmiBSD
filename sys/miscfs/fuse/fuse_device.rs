/* $OpenBSD: fuse_device.c,v 1.51 2026/06/20 13:45:13 helg Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2012-2013 Sylvestre Gallon <ccna.syl@gmail.com>
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */
/* </LICENSES> */

//! The fuse(4) device: `/dev/fuse0`, the character device (`pseudo-device fuse`, major 92,
//! cloning) through which the userland file system daemon reads the kernel's requests and
//! writes its replies.
//!
//! Upstream: sys/miscfs/fuse/fuse_device.c @ 3ce1f3f79392
//!
//! Each open of the device makes a `struct fuse_d` for its (clone) minor. A request
//! (`fusebuf`) queued by `fb_queue` waits on `fd_fbufs_in` until the daemon reads it
//! (`fuseread`), then on `fd_fbufs_wait` until the daemon writes the reply with the same ID
//! (`fusewrite`), which wakes the requesting thread. kqueue's `EVFILT_READ` tells the daemon
//! that a request is ready.
//!
//! Locks used to protect struct members and global data: \[l\] `fd_lock`.
//!
//! ## Deviations
//! - `struct fuse_d` is `malloc(M_DEVBUF)`ed by `fuseopen` and freed by `fuseclose`; its
//!   members are `Cell`s and the ported queue, lock and klist types. `NFUSE` (config(8)'s
//!   `fuse.h`) is 1, GENERIC's `pseudo-device fuse`.
//! - `fuse_device_cleanup` empties each queue by removing its head until it is empty. The C
//!   walks the queue keeping the removed element as `lprev` and calls
//!   `SIMPLEQ_REMOVE_AFTER(lprev)` on it, which unlinks nothing from the queue once two or
//!   more fusebufs are queued (the head keeps pointing at the second); the comment says
//!   every message is cleared, which the port does. As in C, a cleared `FUSE_FORGET` or
//!   `FUSE_INIT` fusebuf, which nobody waits for, is not freed.
//! - `fusewrite` returns `ENODEV` (and the requester gets `ENXIO`) when the device has no
//!   mount any more where the C reads `fd_fmp->max_read` or `fd_fmp->max_write` through the
//!   NULL pointer; that only happens if the unmount ran between the read and the reply.
//! - `fusewrite` negates the reply's error with `wrapping_neg` (`INT_MIN` stays `INT_MIN`,
//!   the C's overflow); the reply's data length is computed with wrapping arithmetic as the
//!   C's `size_t` expression is, so a short `len` fails the `max_read` check.
//! - The kqueue filters reach the device through `kn_hook` (`kn_fused`), as in C.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI32, Ordering};

use crate::kern::kern_event::{
    klist_init_rwlock, klist_insert, klist_remove, knote_locked, seltrue_kqfilter,
};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_rwlock::{rw_assert_wrlock, rw_enter_write, rw_exit_write, rw_init};
use crate::kern::kern_subr::uiomove;
use crate::kern::kern_synch::{
    refcnt_finalize, refcnt_init, refcnt_rele, refcnt_rele_wake, refcnt_take, rwsleep_nsec, wakeup,
    wakeup_one,
};
use crate::kern::subr_prf::panic;
use crate::miscfs::fuse::fusebuf::fb_delete;
use crate::miscfs::fuse::fusefs::FusefsMnt;
use crate::queue_adapter;
use crate::sys::errno::Errno;
use crate::sys::event::{
    EVFILT_READ, EVFILT_WRITE, FILTEROP_ISFD, FILTEROP_MPSAFE, Filterops, Kevent, Klist, Knote,
    knote_modify, knote_process,
};
use crate::sys::fcntl::O_EXCL;
use crate::sys::fusebuf::{
    FUSE_FORGET, FUSE_INIT, FUSE_KERNEL_VERSION, FUSEBUFMAXSIZE, FbNext, FuseInHeader, FuseInitOut,
    FuseOutHeader, Fusebuf, abi_bytes_mut,
};
use crate::sys::malloc::{M_DEVBUF, M_FUSEFS, M_WAITOK, M_ZERO};
use crate::sys::param::{PCATCH, PWAIT};
use crate::sys::proc::Proc;
use crate::sys::queue::{ListEntry, ListHead, SimpleqHead};
use crate::sys::refcnt::Refcnt;
use crate::sys::rwlock::Rwlock;
use crate::sys::systm::INFSLP;
use crate::sys::types::{Dev, minor};
use crate::sys::uio::Uio;
use crate::sys::vnode::IO_NDELAY;

/// `NFUSE`: the number of fuse pseudo-devices (`pseudo-device fuse` in GENERIC: 1).
pub const NFUSE: i32 = 1;

/// `SIMPLEQ_HEAD(fusebuf_head, fusebuf)`.
pub type FusebufHead = SimpleqHead<FbNext>;

/// `struct fuse_d`: an open fuse(4) device.
pub struct FuseD {
    /// `fd_lock`.
    pub fd_lock: Rwlock,
    /// `fd_refcnt`: held by the lookups (`fuse_lookup`) while they use the device.
    pub fd_refcnt: Refcnt,
    /// `fd_fmp`: the mount the device serves, `None` before the mount and after the unmount.
    pub fd_fmp: Cell<Option<&'static FusefsMnt>>,
    /// `fd_unit`: the minor number.
    pub fd_unit: Cell<i32>,
    /// \[l\] `fd_fbufs_in`: the requests the daemon has not read yet.
    pub fd_fbufs_in: FusebufHead,
    /// `fd_fbufs_wait`: the requests the daemon read and has not answered yet.
    pub fd_fbufs_wait: FusebufHead,
    /// \[l\] `fd_rklist`: kq fields.
    pub fd_rklist: Klist,
    /// `fd_list`: the link on `fuse_d_list`.
    pub fd_list: ListEntry<FuseD>,
}

// SAFETY: the queues and the klist are changed under `fd_lock`, the rest under the kernel
// lock, as in C.
unsafe impl Sync for FuseD {}

queue_adapter!(
    /// `fd_list`: the link of a device on `fuse_d_list`.
    pub FuseDList: FuseD, fd_list => ListEntry<FuseD>
);

impl FuseD {
    /// A zeroed device, as `malloc(M_ZERO)` returns it, before `fuseopen` initialises it.
    pub const fn new() -> Self {
        Self {
            fd_lock: Rwlock::new("fusedlk"),
            fd_refcnt: Refcnt::new(),
            fd_fmp: Cell::new(None),
            fd_unit: Cell::new(0),
            fd_fbufs_in: SimpleqHead::new(),
            fd_fbufs_wait: SimpleqHead::new(),
            fd_rklist: Klist::new(),
            fd_list: ListEntry::new(),
        }
    }
}

impl Default for FuseD {
    fn default() -> Self {
        Self::new()
    }
}

/// `fuse_d_list`'s type.
pub struct FuseDListHead(ListHead<FuseDList>);

// SAFETY: changed only under the kernel lock, as in C.
unsafe impl Sync for FuseDListHead {}

impl core::ops::Deref for FuseDListHead {
    type Target = ListHead<FuseDList>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// `stat_fbufs_in`: the fusebufs waiting to be read by a daemon (`vfs.fuse.fusefs_fbufs_in`).
pub static STAT_FBUFS_IN: AtomicI32 = AtomicI32::new(0);
/// `stat_fbufs_wait`: the fusebufs waiting for a reply (`vfs.fuse.fusefs_fbufs_wait`).
pub static STAT_FBUFS_WAIT: AtomicI32 = AtomicI32::new(0);
/// `stat_opened_fusedev`: the open devices (`vfs.fuse.fusefs_open_devices`).
pub static STAT_OPENED_FUSEDEV: AtomicI32 = AtomicI32::new(0);

/// `fuse_d_list`: the open devices.
pub static FUSE_D_LIST: FuseDListHead = FuseDListHead(ListHead::new());

/// `fuse_rd_filtops`.
pub static FUSE_RD_FILTOPS: Filterops = Filterops {
    f_flags: FILTEROP_ISFD | FILTEROP_MPSAFE,
    f_attach: None,
    f_detach: Some(filt_fuse_rdetach),
    f_event: Some(filt_fuse_read),
    f_modify: Some(filt_fuse_modify),
    f_process: Some(filt_fuse_process),
};

/// `fuse_lookup`: the open device of `unit`, with a reference taken (`refcnt_rele` it).
pub fn fuse_lookup(unit: i32) -> Option<&'static FuseD> {
    let fd = FUSE_D_LIST.iter().find(|fd| fd.fd_unit.get() == unit)?;
    refcnt_take(&fd.fd_refcnt);
    Some(fd)
}

/// The device of `dev`'s minor (`fuse_lookup(minor(dev))`).
fn fuse_lookup_dev(dev: Dev) -> Option<&'static FuseD> {
    fuse_lookup(minor(dev) as i32)
}

/// Unlinks the first fusebuf of `q` and fails it with `ENXIO`, waking the VFS syscall
/// waiting on it; `false` when the queue is empty.
fn fuse_device_fail_head(q: &'static FusebufHead) -> bool {
    let Some(f) = q.first() else {
        return false;
    };
    // SAFETY: the queue is not empty (`f` is its head).
    unsafe { q.remove_head() };
    f.set_fb_err(Errno::ENXIO as i32);
    // Wakeup up VFS syscall waiting on this fbuf, it will fail
    wakeup(ptr::from_ref(f));
    true
}

/// `fuse_device_cleanup`: cleanup all msgs from `fd_fbufs_in` and `fd_fbufs_wait`.
pub fn fuse_device_cleanup(dev: Dev) {
    let Some(fd) = fuse_lookup_dev(dev) else {
        return;
    };

    // clear FIFO IN
    rw_enter_write(&fd.fd_lock);
    while fuse_device_fail_head(&fd.fd_fbufs_in) {
        // DPRINTF("cleanup unprocessed msg in sc_fbufs_in\n");
        STAT_FBUFS_IN.fetch_sub(1, Ordering::Relaxed);
    }
    knote_locked(&fd.fd_rklist, 0);
    rw_exit_write(&fd.fd_lock);

    // clear FIFO WAIT
    while fuse_device_fail_head(&fd.fd_fbufs_wait) {
        // DPRINTF("umount unprocessed msg in sc_fbufs_wait\n");
        STAT_FBUFS_WAIT.fetch_sub(1, Ordering::Relaxed);
    }

    refcnt_rele_wake(&fd.fd_refcnt);
}

/// `fuse_device_queue_fbuf`: puts a request on the device's input queue and lets the
/// daemon know. Nothing happens when the device is not open.
pub fn fuse_device_queue_fbuf(dev: Dev, fbuf: &'static Fusebuf) {
    let Some(fd) = fuse_lookup_dev(dev) else {
        return;
    };

    rw_enter_write(&fd.fd_lock);
    // SAFETY: a request is queued once (fresh from `fb_setup`), and stays valid until it is
    // unlinked: its requester sleeps on it until `fusewrite` or `fuse_device_cleanup` took
    // it off, and a request nobody waits for (`FUSE_FORGET`, `FUSE_INIT`) is freed only
    // after it was taken off.
    unsafe { fd.fd_fbufs_in.insert_tail(fbuf) };
    knote_locked(&fd.fd_rklist, 0);
    rw_exit_write(&fd.fd_lock);
    STAT_FBUFS_IN.fetch_add(1, Ordering::Relaxed);

    // Let file system daemons know there is a request ready to process
    wakeup_one(ptr::from_ref(&fd.fd_fbufs_in));

    refcnt_rele_wake(&fd.fd_refcnt);
}

/// `fuse_device_set_fmp`: attaches the mount to its device (`set`) or detaches it.
pub fn fuse_device_set_fmp(fmp: &'static FusefsMnt, set: bool) {
    let Some(fd) = fuse_lookup_dev(fmp.dev) else {
        return;
    };

    if set {
        fd.fd_fmp.set(Some(fmp));
    } else {
        fd.fd_fmp.set(None);

        // Let file system daemons know the device is dead
        wakeup(ptr::from_ref(&fd.fd_fbufs_in));
    }

    refcnt_rele_wake(&fd.fd_refcnt);
}

/// `fuseattach`: the pseudo-device's attach function.
pub fn fuseattach(_num: i32) {
    FUSE_D_LIST.init();
}

/// `fuseopen`: makes the device of the minor; one open per minor (the device clones).
pub fn fuseopen(dev: Dev, flags: i32, _fmt: i32, _p: &Proc) -> Result<(), Errno> {
    let unit = minor(dev) as i32;

    if flags & O_EXCL != 0 {
        return Err(Errno::EBUSY); // No exclusive opens
    }

    if let Some(fd) = fuse_lookup(unit) {
        refcnt_rele_wake(&fd.fd_refcnt);
        return Err(Errno::EBUSY);
    }

    let Some(mem) = malloc(size_of::<FuseD>(), M_DEVBUF, M_WAITOK | M_ZERO) else {
        panic(format_args!("fuseopen: malloc(M_WAITOK) failed"));
    };
    let fd_ptr = mem.cast::<FuseD>();
    // SAFETY: a fresh block of `size_of::<FuseD>()` bytes, aligned by `malloc`; nothing else
    // refers to it yet, and it lives until `fuseclose` frees it.
    let fd: &'static FuseD = unsafe {
        fd_ptr.as_ptr().write(FuseD::new());
        fd_ptr.as_ref()
    };
    fd.fd_unit.set(unit);
    fd.fd_fbufs_in.init();
    fd.fd_fbufs_wait.init();
    rw_init(&fd.fd_lock, "fusedlk");
    // SAFETY: `fd_lock` is a member of the same device, which outlives its klist.
    unsafe { klist_init_rwlock(&fd.fd_rklist, &fd.fd_lock) };
    refcnt_init(&fd.fd_refcnt);

    // SAFETY: a new device, on no list; it stays in place until `fuseclose` unlinks it.
    unsafe { FUSE_D_LIST.insert_head(fd) };

    STAT_OPENED_FUSEDEV.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// `fuseclose`: fails the pending requests, detaches the mount and frees the device.
pub fn fuseclose(dev: Dev, _flags: i32, _fmt: i32, _p: Option<&Proc>) -> Result<(), Errno> {
    let Some(fd) = fuse_lookup_dev(dev) else {
        return Err(Errno::EBADF);
    };

    fuse_device_cleanup(dev);

    // Let fusefs_unmount know the device is closed so it doesn't try and send FBT_DESTROY
    // to a dead file system daemon.
    if let Some(fmp) = fd.fd_fmp.get() {
        fmp.sess_init.set(0);
        fuse_device_set_fmp(fmp, false);
    }

    // SAFETY: `fuseopen` put `fd` on `fuse_d_list`.
    unsafe { ListHead::<FuseDList>::remove(fd) };

    refcnt_rele(&fd.fd_refcnt);
    refcnt_finalize(&fd.fd_refcnt, "fusedfd");
    free(NonNull::from(fd).cast(), M_DEVBUF, size_of::<FuseD>());
    STAT_OPENED_FUSEDEV.fetch_sub(1, Ordering::Relaxed);
    Ok(())
}

/// `fuseread`: the daemon reads the oldest request: its header, its input structure and its
/// data, all or nothing. Sleeps for one unless `IO_NDELAY`.
pub fn fuseread(dev: Dev, uio: &mut Uio<'_>, ioflag: i32) -> Result<(), Errno> {
    let Some(fd) = fuse_lookup_dev(dev) else {
        return Err(Errno::ENODEV);
    };

    if fd.fd_fmp.get().is_none() {
        refcnt_rele(&fd.fd_refcnt);
        return Err(Errno::ENODEV);
    }

    rw_enter_write(&fd.fd_lock);

    let error = 'end: {
        // Loop to avoid a race condition with multithreaded daemons.
        let mut next = fd.fd_fbufs_in.first();
        let fbuf: &'static Fusebuf = loop {
            if let Some(f) = next {
                break f;
            }
            if ioflag & IO_NDELAY != 0 {
                break 'end Err(Errno::EAGAIN);
            }

            let error = rwsleep_nsec(
                ptr::from_ref(&fd.fd_fbufs_in),
                &fd.fd_lock,
                PWAIT | PCATCH,
                "fusedr",
                INFSLP,
            );

            // check for unmount during sleep
            if fd.fd_fmp.get().is_none() {
                break 'end Err(Errno::ENODEV);
            }
            if matches!(error, Err(Errno::EINTR) | Err(Errno::ERESTART)) {
                break 'end Err(Errno::EINTR);
            }

            next = fd.fd_fbufs_in.first();
        };

        // We get the whole fusebuf or nothing
        let op_in_len = fbuf.op_in_len.get();
        if (uio.uio_resid as u64) < (size_of::<FuseInHeader>() + op_in_len) as u64 + fbuf.fb_len() {
            break 'end Err(Errno::EINVAL);
        }

        let mut hdr = fbuf.hdr.get();
        if let Err(e) = uiomove(abi_bytes_mut(&mut hdr), uio) {
            break 'end Err(e);
        }
        let mut op = fbuf.op.get();
        if let Err(e) = uiomove(&mut op.bytes[..op_in_len], uio) {
            break 'end Err(e);
        }
        if fbuf.fb_len() > 0 {
            // SAFETY: the request is on the device's queue and the device holds `fd_lock`:
            // its data is the device's to read now (`fb_dat_slice`'s contract).
            if let Err(e) = uiomove(unsafe { fbuf.fb_dat_slice() }, uio) {
                break 'end Err(e);
            }
        }

        if let Some(dat) = NonNull::new(fbuf.fb_dat()) {
            free(dat, M_FUSEFS, fbuf.fb_len() as usize);
        }
        fbuf.set_fb_dat(ptr::null_mut());

        // Move the fbuf to the wait queue
        // SAFETY: `fbuf` is the head of the input queue.
        unsafe { fd.fd_fbufs_in.remove_head() };
        STAT_FBUFS_IN.fetch_sub(1, Ordering::Relaxed);

        // FUSE_FORGET has no response
        if fbuf.fb_type() == FUSE_FORGET {
            fb_delete(fbuf);
            break 'end Ok(());
        }

        // SAFETY: just unlinked from the input queue, so on no queue; it stays valid until
        // `fusewrite` or `fuse_device_cleanup` unlinks it (its requester sleeps on it).
        unsafe { fd.fd_fbufs_wait.insert_tail(fbuf) };
        STAT_FBUFS_WAIT.fetch_add(1, Ordering::Relaxed);

        Ok(())
    };

    rw_exit_write(&fd.fd_lock);
    refcnt_rele_wake(&fd.fd_refcnt);
    error
}

/// `fusewrite`: the daemon answers a request: a `struct fuse_out_header` with the request's
/// ID, then the operation's output structure and its data. Wakes the requester (a
/// `FUSE_INIT` reply has none: it starts the session).
pub fn fusewrite(dev: Dev, uio: &mut Uio<'_>, _ioflag: i32) -> Result<(), Errno> {
    let Some(fd) = fuse_lookup_dev(dev) else {
        return Err(Errno::ENODEV);
    };

    let error = 'out: {
        // Check for sanity - must receive at least the header
        if uio.uio_resid < size_of::<FuseOutHeader>() {
            break 'out Err(Errno::EINVAL);
        }

        // Read the header
        let mut hdr = FuseOutHeader::default();
        if let Err(e) = uiomove(abi_bytes_mut(&mut hdr), uio) {
            break 'out Err(e);
        }

        // A unique value of zero means daemon is notifying us and hdr.error contains
        // notification type. Currently unsupported.
        if hdr.unique == 0 {
            break 'out Ok(());
        }

        // looking for uuid in fd_fbufs_wait
        let mut lastfbuf: Option<&'static Fusebuf> = None;
        let mut found = None;
        for f in fd.fd_fbufs_wait.iter() {
            if f.fb_uuid() == hdr.unique {
                found = Some(f);
                break;
            }
            lastfbuf = Some(f);
        }
        let Some(fbuf) = found else {
            break 'out Err(Errno::ENOENT);
        };

        let error = fusewrite_reply(fd, fbuf, &hdr, uio);

        // Remove the fbuf from the wait queue
        match lastfbuf {
            // SAFETY: `fbuf` is the head of the wait queue.
            None => unsafe { fd.fd_fbufs_wait.remove_head() },
            // SAFETY: `lastfbuf` is the element before `fbuf` on the wait queue.
            Some(prev) => unsafe { fd.fd_fbufs_wait.remove_after(prev) },
        }
        STAT_FBUFS_WAIT.fetch_sub(1, Ordering::Relaxed);

        // FBT_INIT doesn't expect a response. Otherwise let the VFS syscall that is waiting
        // on this fbuf know the reponse is ready.
        if fbuf.fb_type() == FUSE_INIT {
            fb_delete(fbuf);
        } else {
            wakeup(ptr::from_ref(fbuf));
        }

        error
    };

    refcnt_rele_wake(&fd.fd_refcnt);
    error
}

/// The part of `fusewrite` between finding the request and its `end:` label: fills the
/// fusebuf from the reply.
fn fusewrite_reply(
    fd: &FuseD,
    fbuf: &'static Fusebuf,
    hdr: &FuseOutHeader,
    uio: &mut Uio<'_>,
) -> Result<(), Errno> {
    // Update fb_hdr
    fbuf.set_fb_err(hdr.error.wrapping_neg());

    // Don't expect out struct or data if there was an error
    if fbuf.fb_err() != 0 {
        if uio.uio_resid > 0 {
            fbuf.set_fb_err(Errno::EIO as i32);
            return Err(Errno::EINVAL);
        }
        return Ok(());
    }

    // get operation output
    let op_out_len = fbuf.op_out_len.get();
    if op_out_len > 0 {
        let mut op = fbuf.op.get();
        let r = uiomove(&mut op.bytes[..op_out_len], uio);
        fbuf.op.set(op);
        if let Err(e) = r {
            fbuf.set_fb_err(e as i32);
            return Err(e);
        }
    }

    // Calculate the length of the data buffer to expect
    if fbuf.op_out_buf.get() != 0 {
        let len = (hdr.len as usize)
            .wrapping_sub(size_of::<FuseOutHeader>())
            .wrapping_sub(op_out_len) as u64;
        fbuf.set_fb_len(len);
        let Some(fmp) = fd.fd_fmp.get() else {
            fbuf.set_fb_err(Errno::ENXIO as i32);
            return Err(Errno::ENODEV);
        };
        if len > fmp.max_read as u64 {
            // DPRINTF("invalid fusebuf read size: %llu opcode=%d\n", fb_len, fb_type);
            fbuf.set_fb_err(Errno::EIO as i32);
            return Err(Errno::EINVAL);
        }
    } else {
        fbuf.set_fb_len(0);
    }

    // validate remaining data
    if uio.uio_resid as u64 != fbuf.fb_len() {
        fbuf.set_fb_err(Errno::EIO as i32);
        return Err(Errno::EINVAL);
    }

    if fbuf.fb_len() > 0 {
        let len = fbuf.fb_len() as usize;
        let Some(dat) = malloc(len, M_FUSEFS, M_WAITOK) else {
            panic(format_args!("fusewrite: malloc(M_WAITOK) failed"));
        };
        fbuf.set_fb_dat(dat.as_ptr());
        // SAFETY: the buffer was just allocated with `fb_len` bytes and the request is on
        // the wait queue: its data is the device's to fill now.
        if let Err(e) = uiomove(unsafe { fbuf.fb_dat_slice() }, uio) {
            free(dat, M_FUSEFS, len);
            fbuf.set_fb_dat(ptr::null_mut());
            fbuf.set_fb_err(e as i32);
            return Err(e);
        }
    }

    if fbuf.fb_type() == FUSE_INIT && fbuf.fb_err() == 0 {
        let init: FuseInitOut = fbuf.op_get();
        // We don't support userspace with a smaller major version and it's up to userspace
        // implementations to fall back to our version if they are capable of a later
        // version.
        if init.major != FUSE_KERNEL_VERSION {
            // DPRINTF("unsupported major version: %d.%d\n", major, minor);
            return Err(Errno::EINVAL);
        }
        // If the major versions match then both shall use the smallest of the two minor
        // versions for communication. 7.9 is the smallest version less than what we support
        // where the ABI has not changed. Supporting an earlier version would require
        // conditional handling of some FUSE input arguments. If the daemon supports a later
        // version then it must fall back to ours.
        if init.minor < 9 {
            // DPRINTF("unsupported minor version: %d.%d\n", major, minor);
            return Err(Errno::EINVAL);
        }
        let Some(fmp) = fd.fd_fmp.get() else {
            return Err(Errno::ENODEV);
        };
        // max_write determines the size of buffer to send to the file system daemon when
        // writing so ensure that it's sane.
        let max_write = (init.max_write as usize).min(FUSEBUFMAXSIZE) as i32;
        fmp.max_write.set(if max_write == 0 {
            FUSEBUFMAXSIZE as i32
        } else {
            max_write
        });
        fmp.sess_init.set(1);
    }

    Ok(())
}

/// `fusekqfilter`: `EVFILT_READ` fires while a request waits to be read; the device is
/// always writable.
pub fn fusekqfilter(dev: Dev, kn: &Knote) -> Result<(), Errno> {
    let Some(fd) = fuse_lookup_dev(dev) else {
        return Err(Errno::EINVAL);
    };

    let error = 'end: {
        let klist = match kn.kn_filter().get() {
            EVFILT_READ => {
                kn.kn_fop.set(Some(&FUSE_RD_FILTOPS));
                &fd.fd_rklist
            }
            EVFILT_WRITE => break 'end seltrue_kqfilter(dev, kn),
            _ => break 'end Err(Errno::EINVAL),
        };

        kn.kn_hook
            .set(ptr::from_ref(fd).cast_mut().cast::<c_void>());

        klist_insert(klist, kn);
        Ok(())
    };

    refcnt_rele_wake(&fd.fd_refcnt);

    error
}

/// `kn->kn_hook` of a fuse(4) knote: its device.
fn kn_fused(kn: &Knote) -> &'static FuseD {
    // SAFETY: `fusekqfilter` points `kn_hook` at the device, which lives until `fuseclose`;
    // the descriptor's knotes are removed before its last close reaches `fuseclose`.
    match unsafe { kn.kn_hook.get().cast::<FuseD>().as_ref() } {
        Some(fd) => fd,
        None => panic(format_args!("knote {:p}: no fuse device", kn)),
    }
}

/// `filt_fuse_rdetach`.
pub fn filt_fuse_rdetach(kn: &Knote) {
    let fd = kn_fused(kn);

    klist_remove(&fd.fd_rklist, kn);
}

/// `filt_fuse_read`: a request is waiting to be read.
pub fn filt_fuse_read(kn: &Knote, _hint: i64) -> bool {
    let fd = kn_fused(kn);

    rw_assert_wrlock(&fd.fd_lock);

    !fd.fd_fbufs_in.is_empty()
}

/// `filt_fuse_modify`.
pub fn filt_fuse_modify(kev: &mut Kevent, kn: &Knote) -> bool {
    let fd = kn_fused(kn);

    rw_enter_write(&fd.fd_lock);
    let active = knote_modify(kev, kn);
    rw_exit_write(&fd.fd_lock);

    active
}

/// `filt_fuse_process`.
pub fn filt_fuse_process(kn: &Knote, kev: Option<&mut Kevent>) -> bool {
    let fd = kn_fused(kn);

    rw_enter_write(&fd.fd_lock);
    let active = knote_process(kn, kev);
    rw_exit_write(&fd.fd_lock);

    active
}

#[cfg(test)]
pub(crate) mod tests;
