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

/* <CODE> */
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
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
pub(crate) mod tests {
    // Host tests for the fuse(4) device: opens and closes, a request queued by the kernel read
    // back by the daemon in libfuse's layout (header, input structure, data), replies (data,
    // errors, bad lengths, unknown IDs, notifications), `FUSE_FORGET` and `FUSE_INIT`,
    // `fb_queue` returning the daemon's answer, the cleanup of both queues, close detaching the
    // mount, and the read filter. The vfs setup is `vfs_subr.rs`'s; its `vfsinit` runs
    // `fusefs_init` (the fusebuf pool, the inode hash).

    use std::boxed::Box;
    use std::sync::MutexGuard;
    use std::vec::Vec;
    use std::{assert, assert_eq};

    use super::*;
    use crate::kern::kern_rwlock::rw_obj_init;
    use crate::machine::Machine;
    use crate::machine::copy::AbiPod;
    use crate::machine::cpu::Cpu;
    use crate::miscfs::fuse::fuse_vfsops::PENDING;
    use crate::miscfs::fuse::fusebuf::{TEST_DAEMON, fb_queue, fb_setup};
    use crate::sys::fcntl::{FREAD, FWRITE};
    use crate::sys::fusebuf::{
        FUSE_GETATTR, FUSE_LOOKUP, FUSE_OPEN, FUSE_READ, FuseAttrOut, FuseEntryOut, FuseForgetIn,
        FuseInitIn, FuseOpenOut, FuseReadIn, abi_bytes,
    };
    use crate::sys::mount::Mount;
    use crate::sys::proc::THREAD_PID_OFFSET;
    use crate::sys::types::makedev;
    use crate::sys::uio::{Iovec, UioRw, UioSeg};

    /// fuse(4)'s character major (`cdevsw[92]`).
    pub(crate) const FUSE_MAJOR: u32 = 92;

    /// Memory, the vfs (whose `vfsinit` runs `fusefs_init`), the thread as `curproc`, an empty
    /// device list and counters, no simulated daemon.
    pub(crate) fn setup() -> (MutexGuard<'static, ()>, &'static Proc) {
        let (g, p) = crate::kern::vfs_subr::tests::setup();
        Machine::set_curproc(Machine::curcpu(), p);
        rw_obj_init();
        fuseattach(NFUSE);
        STAT_FBUFS_IN.store(0, Ordering::Relaxed);
        STAT_FBUFS_WAIT.store(0, Ordering::Relaxed);
        STAT_OPENED_FUSEDEV.store(0, Ordering::Relaxed);
        TEST_DAEMON.with(|d| d.set(None));
        (g, p)
    }

    /// `/dev/fuse<unit>`.
    pub(crate) fn fusedev(unit: u32) -> Dev {
        makedev(FUSE_MAJOR, unit)
    }

    /// A mount structure for the device as `fusefs_mount` makes it, on a mount that is not a
    /// real one (the device only reads `max_read`, `max_write` and `sess_init`).
    pub(crate) fn fake_fmp(dev: Dev, max_read: i32) -> &'static FusefsMnt {
        let mp: &'static Mount = Box::leak(Box::new(Mount::new()));
        Box::leak(Box::new(FusefsMnt {
            mp,
            undef_op: Cell::new(0),
            max_read,
            max_write: Cell::new(4096),
            sess_init: Cell::new(PENDING),
            allow_other: 0,
            dev,
        }))
    }

    /// `read(fd, buf, len)` on the device: the bytes read.
    pub(crate) fn dev_read(dev: Dev, buf: &mut [u8], ioflag: i32) -> Result<usize, Errno> {
        let len = buf.len();
        let mut iov = [Iovec {
            iov_base: buf.as_mut_ptr().cast(),
            iov_len: len,
        }];
        let mut uio = Uio {
            uio_iov: &mut iov,
            uio_offset: 0,
            uio_resid: len,
            uio_segflg: UioSeg::UIO_SYSSPACE,
            uio_rw: UioRw::UIO_READ,
            uio_procp: None,
        };
        fuseread(dev, &mut uio, ioflag)?;
        Ok(len - uio.uio_resid)
    }

    /// `writev(fd, ...)` of `parts` on the device, one iovec each, as libfuse writes a reply.
    pub(crate) fn dev_write(dev: Dev, parts: &[&[u8]]) -> Result<(), Errno> {
        let mut bufs: Vec<Vec<u8>> = parts.iter().map(|p| p.to_vec()).collect();
        let mut iov: Vec<Iovec> = bufs
            .iter_mut()
            .map(|b| Iovec {
                iov_base: b.as_mut_ptr().cast(),
                iov_len: b.len(),
            })
            .collect();
        let resid = parts.iter().map(|p| p.len()).sum();
        let mut uio = Uio {
            uio_iov: &mut iov,
            uio_offset: 0,
            uio_resid: resid,
            uio_segflg: UioSeg::UIO_SYSSPACE,
            uio_rw: UioRw::UIO_WRITE,
            uio_procp: None,
        };
        fusewrite(dev, &mut uio, 0)
    }

    /// The size of an operation's input structure, as libfuse knows it (`<sys/fusebuf.h>`'s
    /// userland table): independent of `fb_setup`'s.
    pub(crate) fn op_in_len(opcode: u32) -> usize {
        use crate::sys::fusebuf::*;
        match opcode {
            FUSE_GETATTR => size_of::<FuseGetattrIn>(),
            FUSE_SETATTR => size_of::<FuseSetattrIn>(),
            FUSE_MKNOD => size_of::<FuseMknodIn>(),
            FUSE_MKDIR => size_of::<FuseMkdirIn>(),
            FUSE_RENAME => size_of::<FuseRenameIn>(),
            FUSE_LINK => size_of::<FuseLinkIn>(),
            FUSE_OPEN | FUSE_OPENDIR => size_of::<FuseOpenIn>(),
            FUSE_READ | FUSE_READDIR => size_of::<FuseReadIn>(),
            FUSE_WRITE => size_of::<FuseWriteIn>(),
            FUSE_RELEASE | FUSE_RELEASEDIR => size_of::<FuseReleaseIn>(),
            FUSE_FSYNC => size_of::<FuseFsyncIn>(),
            FUSE_FLUSH => size_of::<FuseFlushIn>(),
            FUSE_INIT => size_of::<FuseInitIn>(),
            FUSE_FORGET => size_of::<FuseForgetIn>(),
            _ => 0, // LOOKUP, READLINK, SYMLINK, UNLINK, RMDIR, STATFS, DESTROY
        }
    }

    /// A request as the daemon reads it: the header, the input structure's bytes, the data.
    pub(crate) struct Req {
        pub(crate) hdr: FuseInHeader,
        pub(crate) op: Vec<u8>,
        pub(crate) data: Vec<u8>,
    }

    impl Req {
        /// The input structure `T`.
        pub(crate) fn op<T: AbiPod + Default>(&self) -> T {
            let mut v = T::default();
            abi_bytes_mut(&mut v).copy_from_slice(&self.op[..size_of::<T>()]);
            v
        }
    }

    /// Reads the next request from the device (`None` when there is none, `IO_NDELAY`).
    pub(crate) fn read_req(dev: Dev) -> Option<Req> {
        let mut buf = std::vec![0u8; 64 * 1024];
        let n = match dev_read(dev, &mut buf, IO_NDELAY) {
            Ok(n) => n,
            Err(Errno::EAGAIN) => return None,
            Err(e) => std::panic!("fuseread: {e:?}"),
        };
        let mut hdr = FuseInHeader::default();
        abi_bytes_mut(&mut hdr).copy_from_slice(&buf[..size_of::<FuseInHeader>()]);
        assert_eq!(hdr.len as usize, n, "hdr.len is the request's length");
        let op_end = size_of::<FuseInHeader>() + op_in_len(hdr.opcode);
        Some(Req {
            hdr,
            op: buf[size_of::<FuseInHeader>()..op_end].to_vec(),
            data: buf[op_end..n].to_vec(),
        })
    }

    /// Writes a reply to request `unique`: `error` (negated errno), output structure, data.
    pub(crate) fn reply(
        dev: Dev,
        unique: u64,
        error: i32,
        out: &[u8],
        data: &[u8],
    ) -> Result<(), Errno> {
        let hdr = FuseOutHeader {
            len: (size_of::<FuseOutHeader>() + out.len() + data.len()) as u32,
            error,
            unique,
        };
        dev_write(dev, &[abi_bytes(&hdr), out, data])
    }

    /// `fuseopen` of `/dev/fuse<unit>` for reading and writing.
    pub(crate) fn open_dev(p: &Proc, unit: u32) -> Dev {
        let dev = fusedev(unit);
        fuseopen(dev, FREAD | FWRITE, 0, p).expect("fuseopen");
        dev
    }

    #[test]
    fn open_is_exclusive_per_minor_and_close_frees() {
        let (_g, p) = setup();
        let dev0 = open_dev(p, 0);
        assert_eq!(STAT_OPENED_FUSEDEV.load(Ordering::Relaxed), 1);
        assert_eq!(fuseopen(dev0, FREAD, 0, p), Err(Errno::EBUSY));
        assert_eq!(
            fuseopen(fusedev(1), FREAD | O_EXCL, 0, p),
            Err(Errno::EBUSY),
            "no exclusive opens"
        );
        let dev1 = open_dev(p, 1 << 8); // a clone's minor
        assert_eq!(STAT_OPENED_FUSEDEV.load(Ordering::Relaxed), 2);
        assert!(fuse_lookup_dev(dev1).is_some_and(|fd| {
            refcnt_rele(&fd.fd_refcnt);
            fd.fd_unit.get() == 1 << 8
        }));

        fuseclose(dev0, FREAD, 0, Some(p)).expect("close");
        fuseclose(dev1, FREAD, 0, Some(p)).expect("close");
        assert_eq!(STAT_OPENED_FUSEDEV.load(Ordering::Relaxed), 0);
        assert_eq!(fuseclose(dev0, FREAD, 0, Some(p)), Err(Errno::EBADF));
        assert_eq!(
            fuseread(dev0, &mut empty_uio(&mut [0u8; 1]), 0),
            Err(Errno::ENODEV)
        );
    }

    /// A one-byte read uio over `b`, for the error paths that never touch it.
    fn empty_uio(b: &mut [u8; 1]) -> Uio<'static> {
        let iov: &'static mut [Iovec] = Box::leak(Box::new([Iovec {
            iov_base: b.as_mut_ptr().cast(),
            iov_len: 1,
        }]));
        Uio {
            uio_iov: iov,
            uio_offset: 0,
            uio_resid: 1,
            uio_segflg: UioSeg::UIO_SYSSPACE,
            uio_rw: UioRw::UIO_READ,
            uio_procp: None,
        }
    }

    #[test]
    fn read_needs_a_mount_and_honours_ndelay() {
        let (_g, p) = setup();
        let dev = open_dev(p, 0);
        let mut buf = [0u8; 256];
        assert_eq!(dev_read(dev, &mut buf, IO_NDELAY), Err(Errno::ENODEV));
        let fmp = fake_fmp(dev, FUSEBUFMAXSIZE as i32);
        fuse_device_set_fmp(fmp, true);
        assert_eq!(dev_read(dev, &mut buf, IO_NDELAY), Err(Errno::EAGAIN));
        fuseclose(dev, 0, 0, Some(p)).expect("close");
        assert_eq!(
            fmp.sess_init.get(),
            0,
            "close tells the mount the daemon is gone"
        );
    }

    #[test]
    fn a_queued_request_is_read_in_libfuse_layout_and_answered() {
        let (_g, p) = setup();
        let dev = open_dev(p, 0);
        fuse_device_set_fmp(fake_fmp(dev, FUSEBUFMAXSIZE as i32), true);

        let fbuf = fb_setup(6, 7, FUSE_LOOKUP, p);
        // SAFETY: a fresh fusebuf, not queued yet.
        unsafe { fbuf.fb_dat_slice() }.copy_from_slice(b"hello\0");
        fuse_device_queue_fbuf(dev, fbuf);
        assert_eq!(STAT_FBUFS_IN.load(Ordering::Relaxed), 1);

        // We get the whole fusebuf or nothing.
        let mut small = [0u8; 45];
        assert_eq!(dev_read(dev, &mut small, IO_NDELAY), Err(Errno::EINVAL));
        assert_eq!(STAT_FBUFS_IN.load(Ordering::Relaxed), 1);

        let req = read_req(dev).expect("a request");
        assert_eq!(req.hdr.len, 46);
        assert_eq!(req.hdr.opcode, FUSE_LOOKUP);
        assert_eq!(req.hdr.unique, fbuf.fb_uuid());
        assert_eq!(req.hdr.nodeid, 7);
        assert_eq!(req.hdr.uid, p.ucred().cr_uid.get());
        assert_eq!(req.hdr.gid, p.ucred().cr_gid.get());
        assert_eq!(req.hdr.pid, (p.p_tid.get() + THREAD_PID_OFFSET) as u32);
        assert!(req.op.is_empty());
        assert_eq!(req.data, b"hello\0");
        assert!(fbuf.fb_dat().is_null(), "the data is freed once read");
        assert_eq!(STAT_FBUFS_IN.load(Ordering::Relaxed), 0);
        assert_eq!(STAT_FBUFS_WAIT.load(Ordering::Relaxed), 1);
        assert!(read_req(dev).is_none());

        let mut entry = FuseEntryOut {
            nodeid: 9,
            ..FuseEntryOut::default()
        };
        entry.attr.mode = 0o100644;
        entry.attr.size = 13;
        reply(dev, req.hdr.unique, 0, abi_bytes(&entry), &[]).expect("reply");
        assert_eq!(fbuf.fb_err(), 0);
        assert_eq!(fbuf.op_get::<FuseEntryOut>(), entry);
        assert_eq!(fbuf.fb_len(), 0);
        assert_eq!(STAT_FBUFS_WAIT.load(Ordering::Relaxed), 0);
        // The reply's ID is not waited for any more.
        assert_eq!(
            reply(dev, req.hdr.unique, 0, abi_bytes(&entry), &[]),
            Err(Errno::ENOENT)
        );
        fb_delete(fbuf);
        fuseclose(dev, 0, 0, Some(p)).expect("close");
    }

    #[test]
    fn replies_with_errors_bad_lengths_and_notifications() {
        let (_g, p) = setup();
        let dev = open_dev(p, 0);
        let fmp = fake_fmp(dev, 8);
        fuse_device_set_fmp(fmp, true);

        // Too short for a header; a notification (unique 0) is accepted and ignored.
        assert_eq!(dev_write(dev, &[&[0u8; 15]]), Err(Errno::EINVAL));
        reply(dev, 0, -1, &[], &[]).expect("notification");

        // An error reply carries nothing else.
        let a = fb_setup(0, 1, FUSE_GETATTR, p);
        let b = fb_setup(0, 1, FUSE_GETATTR, p);
        fuse_device_queue_fbuf(dev, a);
        fuse_device_queue_fbuf(dev, b);
        let ra = read_req(dev).expect("a");
        let rb = read_req(dev).expect("b");
        assert_eq!(ra.op.len(), size_of::<crate::sys::fusebuf::FuseGetattrIn>());
        // b is answered first: a is then the element before it on the wait queue.
        reply(dev, rb.hdr.unique, -(Errno::ENOENT as i32), &[], &[]).expect("error reply");
        assert_eq!(b.fb_err(), Errno::ENOENT as i32);
        assert_eq!(
            reply(dev, ra.hdr.unique, -(Errno::EACCES as i32), &[], &[1, 2]),
            Err(Errno::EINVAL)
        );
        assert_eq!(
            a.fb_err(),
            Errno::EIO as i32,
            "an error reply with data fails"
        );
        assert_eq!(STAT_FBUFS_WAIT.load(Ordering::Relaxed), 0);
        fb_delete(a);
        fb_delete(b);

        // A data reply: its length comes from hdr.len, bounded by max_read (8 here).
        let r = fb_setup(0, 2, FUSE_READ, p);
        r.op_set(&FuseReadIn {
            size: 8,
            ..FuseReadIn::default()
        });
        fuse_device_queue_fbuf(dev, r);
        let rr = read_req(dev).expect("read");
        assert_eq!(rr.op::<FuseReadIn>().size, 8);
        reply(dev, rr.hdr.unique, 0, &[], b"abcdef").expect("data");
        assert_eq!(r.fb_len(), 6);
        // SAFETY: the reply is in; the fusebuf is off the queues.
        assert_eq!(unsafe { r.fb_dat_slice() }, b"abcdef");
        fb_delete(r);

        let r = fb_setup(0, 2, FUSE_READ, p);
        fuse_device_queue_fbuf(dev, r);
        let rr = read_req(dev).expect("read");
        assert_eq!(
            reply(dev, rr.hdr.unique, 0, &[], b"123456789"),
            Err(Errno::EINVAL)
        );
        assert_eq!(r.fb_err(), Errno::EIO as i32, "more than max_read");
        fb_delete(r);

        // hdr.len and the bytes written disagree.
        let o = fb_setup(0, 2, FUSE_OPEN, p);
        fuse_device_queue_fbuf(dev, o);
        let ro = read_req(dev).expect("open");
        let hdr = FuseOutHeader {
            len: 16,
            error: 0,
            unique: ro.hdr.unique,
        };
        let out = FuseOpenOut::default();
        assert_eq!(
            dev_write(dev, &[abi_bytes(&hdr), abi_bytes(&out), &[0]]),
            Err(Errno::EINVAL)
        );
        assert_eq!(o.fb_err(), Errno::EIO as i32);
        fb_delete(o);

        fuseclose(dev, 0, 0, Some(p)).expect("close");
    }

    #[test]
    fn forget_and_init_need_no_requester() {
        let (_g, p) = setup();
        let dev = open_dev(p, 0);
        let fmp = fake_fmp(dev, FUSEBUFMAXSIZE as i32);
        fuse_device_set_fmp(fmp, true);

        let f = fb_setup(0, 5, FUSE_FORGET, p);
        f.op_set(&FuseForgetIn { nlookup: 3 });
        fuse_device_queue_fbuf(dev, f);
        let rf = read_req(dev).expect("forget");
        assert_eq!(rf.hdr.opcode, FUSE_FORGET);
        assert_eq!(rf.op::<FuseForgetIn>().nlookup, 3);
        assert_eq!(
            STAT_FBUFS_WAIT.load(Ordering::Relaxed),
            0,
            "no reply is expected"
        );

        // A daemon of an older major version is refused: the session stays pending.
        let i = fb_setup(0, 0, FUSE_INIT, p);
        i.op_set(&FuseInitIn {
            major: FUSE_KERNEL_VERSION,
            minor: 19,
            ..FuseInitIn::default()
        });
        fuse_device_queue_fbuf(dev, i);
        let ri = read_req(dev).expect("init");
        assert_eq!(ri.op::<FuseInitIn>().major, 7);
        let old = FuseInitOut {
            major: 6,
            minor: 30,
            ..FuseInitOut::default()
        };
        assert_eq!(
            reply(dev, ri.hdr.unique, 0, abi_bytes(&old), &[]),
            Err(Errno::EINVAL)
        );
        assert_eq!(fmp.sess_init.get(), PENDING);

        let i = fb_setup(0, 0, FUSE_INIT, p);
        fuse_device_queue_fbuf(dev, i);
        let ri = read_req(dev).expect("init");
        let ok = FuseInitOut {
            major: 7,
            minor: 26,
            max_write: 0,
            ..FuseInitOut::default()
        };
        reply(dev, ri.hdr.unique, 0, abi_bytes(&ok), &[]).expect("init reply");
        assert_eq!(fmp.sess_init.get(), 1);
        assert_eq!(
            fmp.max_write.get(),
            FUSEBUFMAXSIZE as i32,
            "0 means the most"
        );
        assert_eq!(STAT_FBUFS_WAIT.load(Ordering::Relaxed), 0);

        fuseclose(dev, 0, 0, Some(p)).expect("close");
    }

    /// A simulated daemon answering every queued request with `FuseOpenOut { fh: 77 }`, or
    /// with `EACCES` for `FUSE_GETATTR`.
    fn open_daemon(dev: Dev) {
        while let Some(req) = read_req(dev) {
            if req.hdr.opcode == FUSE_GETATTR {
                reply(dev, req.hdr.unique, -(Errno::EACCES as i32), &[], &[]).expect("reply");
            } else {
                let out = FuseOpenOut {
                    fh: 77,
                    ..FuseOpenOut::default()
                };
                reply(dev, req.hdr.unique, 0, abi_bytes(&out), &[]).expect("reply");
            }
        }
    }

    #[test]
    fn fb_queue_returns_the_daemons_answer() {
        let (_g, p) = setup();
        let dev = open_dev(p, 0);
        fuse_device_set_fmp(fake_fmp(dev, FUSEBUFMAXSIZE as i32), true);
        TEST_DAEMON.with(|d| d.set(Some(open_daemon)));

        let o = fb_setup(0, 3, FUSE_OPEN, p);
        assert_eq!(fb_queue(dev, o), Ok(()));
        assert_eq!(o.op_get::<FuseOpenOut>().fh, 77);
        fb_delete(o);

        let g = fb_setup(0, 3, FUSE_GETATTR, p);
        assert_eq!(fb_queue(dev, g), Err(Errno::EACCES));
        assert_eq!(g.op_get::<FuseAttrOut>(), FuseAttrOut::default());
        fb_delete(g);
        assert_eq!(STAT_FBUFS_IN.load(Ordering::Relaxed), 0);
        assert_eq!(STAT_FBUFS_WAIT.load(Ordering::Relaxed), 0);

        TEST_DAEMON.with(|d| d.set(None));
        fuseclose(dev, 0, 0, Some(p)).expect("close");
    }

    #[test]
    fn cleanup_fails_every_queued_request() {
        let (_g, p) = setup();
        let dev = open_dev(p, 0);
        fuse_device_set_fmp(fake_fmp(dev, FUSEBUFMAXSIZE as i32), true);

        let fbufs: Vec<&'static Fusebuf> =
            (0..5).map(|i| fb_setup(0, i, FUSE_GETATTR, p)).collect();
        for f in &fbufs {
            fuse_device_queue_fbuf(dev, f);
        }
        // Two of them read: on the wait queue.
        read_req(dev).expect("one");
        read_req(dev).expect("two");
        assert_eq!(STAT_FBUFS_IN.load(Ordering::Relaxed), 3);
        assert_eq!(STAT_FBUFS_WAIT.load(Ordering::Relaxed), 2);

        fuse_device_cleanup(dev);
        for f in &fbufs {
            assert_eq!(f.fb_err(), Errno::ENXIO as i32);
        }
        assert_eq!(STAT_FBUFS_IN.load(Ordering::Relaxed), 0);
        assert_eq!(STAT_FBUFS_WAIT.load(Ordering::Relaxed), 0);
        let fd = fuse_lookup_dev(dev).expect("open");
        assert!(fd.fd_fbufs_in.is_empty() && fd.fd_fbufs_wait.is_empty());
        refcnt_rele(&fd.fd_refcnt);
        assert!(read_req(dev).is_none());
        for f in fbufs {
            fb_delete(f);
        }
        fuseclose(dev, 0, 0, Some(p)).expect("close");
    }

    #[test]
    fn the_read_filter_fires_while_a_request_waits() {
        let (_g, p) = setup();
        let dev = open_dev(p, 0);
        fuse_device_set_fmp(fake_fmp(dev, FUSEBUFMAXSIZE as i32), true);

        let kn: &'static Knote = Box::leak(Box::new(Knote::new()));
        kn.kn_filter().set(EVFILT_READ);
        fusekqfilter(dev, kn).expect("kqfilter");
        assert!(
            kn.kn_fop
                .get()
                .is_some_and(|f| ptr::eq(f, &FUSE_RD_FILTOPS))
        );
        let fd = kn_fused(kn);

        let event = || {
            rw_enter_write(&fd.fd_lock);
            let e = filt_fuse_read(kn, 0);
            rw_exit_write(&fd.fd_lock);
            e
        };
        assert!(!event());
        filt_fuse_rdetach(kn);
        assert!(fd.fd_rklist.kl_list.is_empty());

        // Queueing activates the knotes on the list (`knote_locked`), which needs a kqueue: the
        // knote is attached again once the request waits.
        let f = fb_setup(0, 1, FUSE_GETATTR, p);
        fuse_device_queue_fbuf(dev, f);
        fusekqfilter(dev, kn).expect("kqfilter");
        assert!(event());
        read_req(dev).expect("the request");
        assert!(!event());
        filt_fuse_rdetach(kn);
        reply(dev, f.fb_uuid(), 0, abi_bytes(&FuseAttrOut::default()), &[]).expect("reply");
        fb_delete(f);

        let wk: &'static Knote = Box::leak(Box::new(Knote::new()));
        wk.kn_filter().set(EVFILT_WRITE);
        fusekqfilter(dev, wk).expect("write filter: always true");
        let bad: &'static Knote = Box::leak(Box::new(Knote::new()));
        bad.kn_filter().set(crate::sys::event::EVFILT_VNODE);
        assert_eq!(fusekqfilter(dev, bad), Err(Errno::EINVAL));

        fuseclose(dev, 0, 0, Some(p)).expect("close");
    }
}
/* </TESTS> */
