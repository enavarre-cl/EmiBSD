/* $OpenBSD: fusebuf.c,v 1.19 2026/06/17 13:29:01 helg Exp $ */
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

//! The FUSE request buffers: `fb_setup` makes a fusebuf for an operation, `fb_queue` hands
//! it to the daemon and waits for the reply, `fb_delete` frees it.
//!
//! Upstream: sys/miscfs/fuse/fusebuf.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `fb_setup` returns the fusebuf as a `&'static Fusebuf` (a `fusefs_fbuf_pool` item, the
//!   `struct mbuf *` idiom); `fb_delete` takes it back and accepts `None` for the C's NULL.
//!   The operation is a `u32`, the type of the header's `opcode` it is stored in (an `int`
//!   argument in C).
//! - `fb_queue` returns the daemon's error as a `Result`; an error number that is not an
//!   [`Errno`] (the daemon chooses it) becomes `EIO`, where the C returns the number as it is.
//! - The host tests (`cfg(test)`) have no context switch: between queueing the fusebuf and
//!   sleeping on it, `fb_queue` runs the tests' simulated daemon (`TEST_DAEMON`), which reads
//!   the request from the device and writes its reply as the daemon process would; on the
//!   host the sleep then returns at once (`cold`). The kernel build has no such call.

use core::ptr::{self, NonNull};

use crate::dev::rnd::arc4random_buf;
use crate::kassert;
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_synch::tsleep_nsec;
use crate::kern::subr_pool::{pool_get, pool_put};
use crate::kern::subr_prf::panic;
use crate::miscfs::fuse::fuse_device::fuse_device_queue_fbuf;
use crate::miscfs::fuse::fuse_vfsops::FUSEFS_FBUF_POOL;
use crate::sys::errno::Errno;
use crate::sys::fusebuf::{
    FUSE_FLUSH, FUSE_FORGET, FUSE_FSYNC, FUSE_GETATTR, FUSE_INIT, FUSE_LINK, FUSE_LOOKUP,
    FUSE_MKDIR, FUSE_MKNOD, FUSE_OPEN, FUSE_OPENDIR, FUSE_READ, FUSE_READDIR, FUSE_READLINK,
    FUSE_RELEASE, FUSE_RELEASEDIR, FUSE_RENAME, FUSE_SETATTR, FUSE_STATFS, FUSE_SYMLINK,
    FUSE_WRITE, FUSEBUFMAXSIZE, FuseAttrOut, FuseEntryOut, FuseFlushIn, FuseForgetIn, FuseFsyncIn,
    FuseGetattrIn, FuseInHeader, FuseInitIn, FuseInitOut, FuseLinkIn, FuseMkdirIn, FuseMknodIn,
    FuseOpenIn, FuseOpenOut, FuseReadIn, FuseReleaseIn, FuseRenameIn, FuseSetattrIn, FuseStatfsOut,
    FuseWriteIn, FuseWriteOut, Fusebuf,
};
use crate::sys::malloc::{M_FUSEFS, M_WAITOK, M_ZERO};
use crate::sys::param::PWAIT;
use crate::sys::pool::{PR_WAITOK, PR_ZERO};
use crate::sys::proc::{Proc, THREAD_PID_OFFSET};
use crate::sys::systm::INFSLP;
use crate::sys::types::{Dev, Ino};

#[cfg(test)]
std::thread_local! {
    /// The host tests' simulated daemon: called by `fb_queue` with the device after the
    /// fusebuf is queued (see the module's deviations).
    pub(crate) static TEST_DAEMON: core::cell::Cell<Option<fn(Dev)>> =
        const { core::cell::Cell::new(None) };
}

/// `fb_setup`: a zeroed fusebuf for operation `op` on inode `ino`, issued by `p`, with a
/// zeroed data buffer of `len` bytes (none when `len` is 0). The sizes of the operation's
/// input and output structures, and whether a data reply is expected, follow from `op`.
pub fn fb_setup(len: usize, ino: Ino, op: u32, p: &Proc) -> &'static Fusebuf {
    kassert!(len <= FUSEBUFMAXSIZE);

    let Some(mem) = pool_get(&FUSEFS_FBUF_POOL, PR_WAITOK | PR_ZERO) else {
        panic(format_args!("fb_setup: pool_get(PR_WAITOK) failed"));
    };
    let fbuf_ptr = mem.cast::<Fusebuf>();
    // SAFETY: a fresh `fusefs_fbuf_pool` item of `size_of::<Fusebuf>()` bytes (the pool's
    // item size), aligned by the pool; nothing else refers to it yet, and it lives until
    // `fb_delete` puts it back.
    let fbuf: &'static Fusebuf = unsafe {
        fbuf_ptr.as_ptr().write(Fusebuf::new());
        fbuf_ptr.as_ref()
    };

    let (op_in_len, op_out_len, op_out_buf) = match op {
        FUSE_GETATTR => (size_of::<FuseGetattrIn>(), size_of::<FuseAttrOut>(), 0),
        FUSE_SETATTR => (size_of::<FuseSetattrIn>(), size_of::<FuseAttrOut>(), 0),
        FUSE_READLINK => (0, 0, 1),
        FUSE_FLUSH => (size_of::<FuseFlushIn>(), 0, 0),
        FUSE_INIT => (size_of::<FuseInitIn>(), size_of::<FuseInitOut>(), 0),
        FUSE_OPEN | FUSE_OPENDIR => (size_of::<FuseOpenIn>(), size_of::<FuseOpenOut>(), 0),
        FUSE_READ | FUSE_READDIR => (size_of::<FuseReadIn>(), 0, 1),
        FUSE_MKDIR => (size_of::<FuseMkdirIn>(), size_of::<FuseEntryOut>(), 0),
        FUSE_MKNOD => (size_of::<FuseMknodIn>(), size_of::<FuseEntryOut>(), 0),
        FUSE_FSYNC => (size_of::<FuseFsyncIn>(), 0, 0),
        FUSE_RENAME => (size_of::<FuseRenameIn>(), 0, 0),
        FUSE_LINK => (size_of::<FuseLinkIn>(), size_of::<FuseEntryOut>(), 0),
        FUSE_SYMLINK => (0, size_of::<FuseEntryOut>(), 0),
        FUSE_RELEASE | FUSE_RELEASEDIR => (size_of::<FuseReleaseIn>(), 0, 0),
        FUSE_WRITE => (size_of::<FuseWriteIn>(), size_of::<FuseWriteOut>(), 0),
        FUSE_FORGET => (size_of::<FuseForgetIn>(), 0, 0),
        FUSE_LOOKUP => (0, size_of::<FuseEntryOut>(), 0),
        FUSE_STATFS => (0, size_of::<FuseStatfsOut>(), 0),
        _ => (0, 0, 0),
    };
    fbuf.op_in_len.set(op_in_len);
    fbuf.op_out_len.set(op_out_len);
    fbuf.op_out_buf.set(op_out_buf);

    let mut uuid = [0u8; 8];
    arc4random_buf(&mut uuid);
    let cred = p.ucred();
    fbuf.update_hdr(|h| {
        h.len = (size_of::<FuseInHeader>() + op_in_len + len) as u32;
        h.unique = u64::from_ne_bytes(uuid);
        h.opcode = op;
        h.nodeid = ino;
        // When exposed to userspace, thread IDs have THREAD_PID_OFFSET added to keep them
        // from overlapping the PID range.
        h.pid = (p.p_tid.get() + THREAD_PID_OFFSET) as u32;
        h.uid = cred.cr_uid.get();
        h.gid = cred.cr_gid.get();
    });
    fbuf.set_fb_len(len as u64);
    fbuf.set_fb_err(0);
    if len == 0 {
        fbuf.set_fb_dat(ptr::null_mut());
    } else {
        let Some(dat) = malloc(len, M_FUSEFS, M_WAITOK | M_ZERO) else {
            panic(format_args!("fb_setup: malloc(M_WAITOK) failed"));
        };
        fbuf.set_fb_dat(dat.as_ptr());
    }

    fbuf
}

/// `fb_queue`: puts the fbuf on the queue and waits for the file system to process it. The
/// current process will block indefinitely and cannot be interrupted or killed. This is
/// consistent with how VFS system calls should behave. nfs supports the `-ointr` or `-i`
/// mount option and FUSE can too but this is non-trivial. The file system daemon must be
/// multi-threaded and also support being interrupted. Note that libfuse currently only
/// supports single-threaded daemons.
///
/// Why not timeout similar to `mount_nfs -osoft`? It introduces another point of failure and
/// a possible mount option (to specify the timeout) that users need to understand and tune
/// to avoid premature timeouts for slow file systems. More complexity, less reliability.
///
/// In the case where the daemon has become unresponsive the daemon will have to be killed
/// in order for the current process to wakeup. The FUSE device is automatically closed when
/// the daemon terminates and any waiting fbuf is woken up.
pub fn fb_queue(dev: Dev, fbuf: &'static Fusebuf) -> Result<(), Errno> {
    fuse_device_queue_fbuf(dev, fbuf);
    #[cfg(test)]
    if let Some(daemon) = TEST_DAEMON.with(|d| d.get()) {
        daemon(dev);
    }
    let _ = tsleep_nsec(ptr::from_ref(fbuf), PWAIT, "fuse", INFSLP);

    match fbuf.fb_err() {
        0 => Ok(()),
        e => Err(Errno::from_raw(e).unwrap_or(Errno::EIO)),
    }
}

/// `fb_delete`: frees the fusebuf and its data buffer; `None` (the C's NULL) is ignored.
pub fn fb_delete(fbuf: impl Into<Option<&'static Fusebuf>>) {
    if let Some(fbuf) = fbuf.into() {
        if let Some(dat) = NonNull::new(fbuf.fb_dat()) {
            free(dat, M_FUSEFS, fbuf.fb_len() as usize);
        }
        pool_put(&FUSEFS_FBUF_POOL, NonNull::from(fbuf).cast());
    }
}
