/*	$OpenBSD: sys_pipe.c,v 1.149 2025/08/04 04:59:31 guenther Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1996 John S. Dyson
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice immediately at the beginning of the file, without modification,
 *    this list of conditions, and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Absolutely no warranty of function or purpose is made by the author
 *    John S. Dyson.
 * 4. Modifications may be freely made to this file if the above conditions
 *    are met.
 */
/* </LICENSES> */

//! `sys_pipe.c`: pipes, a high-performance replacement for the socket-based pipes scheme
//! originally used in FreeBSD/4.4Lite. It does not support all features of sockets, but
//! does do everything that pipes normally do.
//!
//! Upstream: sys/kern/sys_pipe.c @ 3ce1f3f79392
//!
//! `pipe(2)`/`pipe2(2)` allocate a [`PipePair`] from `pipe_pair_pool`: two `struct pipe`s,
//! one per direction, that share one rwlock (`pp_lock`), each with a circular buffer of
//! `PIPE_SIZE` bytes of pageable kernel memory (`km_alloc(kv_any, kp_pageable)`, faulted in
//! by `uvm_fault` on `kernel_map`). The descriptor `fds[0]` reads the pair's `pp_rpipe`,
//! `fds[1]` reads `pp_wpipe`; a write goes into the buffer of the file's peer. The rwlock
//! guards the pipes' state; `PIPE_LOCK` (`pipe_iolock`) gives one reader or writer at a time
//! the buffer, so that `uiomove` runs with the rwlock released. A write of at most
//! `PIPE_BUF` bytes is atomic; a write larger than `PIPE_SIZE` into an empty buffer grows it
//! to `BIG_PIPE_SIZE` (at most `LIMITBIGPIPES` such pipes). A write to a pipe whose reader is
//! gone fails with `EPIPE`, on which `dofilewritev` posts `SIGPIPE`.
//!
//! ## Deviations
//! - Pipes are reached as `&Pipe` through their file's `f_data` ([`fp_pipe`] checks
//!   `DTYPE_PIPE`) and through `pipe_peer`; the pair is a pool item that
//!   [`pipe_pair_destroy`] frees when its second pipe is destroyed, so `pipe_destroy` and
//!   `pipe_pair_destroy` are `unsafe` and take `NonNull` pointers (the `taskq_destroy`
//!   idiom of `docs/C_TO_RUST.md`).
//! - `pipe_pair_create` returns `None` when the pool is empty: `pool_get` cannot sleep
//!   for `PR_WAITOK` yet, where the C waits and never fails; `dopipe` answers `ENOMEM`, as
//!   for a failed buffer allocation.
//! - `pipe_rundown` returns `bool` (the C's "non-zero if a rundown is ongoing").
//! - `kern_event.c` is not ported, so there are no knotes and no `select(2)`/`poll(2)`:
//!   `pipe_wakeup` reports its `knote_locked` (the call that wakes kqueue, select and poll
//!   waiters) and still posts `SIGIO`; `pipe_kqfilter` takes the lock, finds the peer and
//!   reports the registration (`struct knote`); `klist_init_rwlock`, `klist_insert_locked`,
//!   `klist_remove` and `klist_free` are comments where the C calls them (`pipe_klist` is
//!   left out of `struct pipe`). The filter bodies are ported as [`filt_piperead`],
//!   [`filt_pipewrite`] and [`filt_pipeexcept`] over the pipe and the knote's `__EV_POLL`
//!   flag, returning a [`PipeFilter`] (whether the event is active, `kn_data`, `EV_EOF`,
//!   `__EV_HUP`) for `kern_event.c` to apply; `filt_pipedetach`, `filt_pipemodify` and
//!   `filt_pipeprocess` report `klist_remove`/`knote_modify`/`knote_process`. The three
//!   `struct filterops` wait for `<sys/event.h>`. The filters read the pipe of the knote's
//!   file (`kn->kn_fp->f_data`), which callers pass.
//! - `fo_ioctl`'s `data` is the kernel copy of the argument (`sys_ioctl`), at least an `int`
//!   wide; the `int` is read and written in native byte order.
//! - `KTRACE` is not configured (`ktrfds`).

use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::slice;
use core::sync::atomic::{AtomicU32, Ordering};

use crate::kassert;
use crate::kern::kern_descrip::{closef, falloc, fdinsert, fdrelease, fdremove};
use crate::kern::kern_rwlock::{
    rw_assert_anylock, rw_assert_wrlock, rw_enter_read, rw_enter_write, rw_exit_read,
    rw_exit_write, rw_init,
};
use crate::kern::kern_sig::{pgsigio, sigio_free, sigio_getown, sigio_setown};
use crate::kern::kern_subr::uiomove;
use crate::kern::kern_synch::{rwsleep_nsec, wakeup};
use crate::kern::kern_tc::getnanotime;
use crate::kern::subr_pool::{pool_get, pool_init, pool_put};
use crate::kern::subr_prf::panic;
use crate::machine::copy::copyout;
use crate::machine::intr::IPL_MPFLOOR;
use crate::sys::errno::Errno;
use crate::sys::event::Knote;
use crate::sys::fcntl::{FNONBLOCK, FREAD, FWRITE, O_CLOEXEC, O_CLOFORK};
use crate::sys::file::{DTYPE_PIPE, File, Fileops, frele};
use crate::sys::filedesc::{UF_EXCLOSE, UF_FORKCLOSE, fdplock, fdpunlock};
use crate::sys::filio::{FIOASYNC, FIOGETOWN, FIONREAD, FIOSETOWN};
use crate::sys::param::{PCATCH, PRIBIO};
use crate::sys::pipe::{
    BIG_PIPE_SIZE, PIPE_ASYNC, PIPE_EOF, PIPE_LOCK, PIPE_LWANT, PIPE_SIZE, PIPE_WANTD, PIPE_WANTR,
    PIPE_WANTW, Pipe,
};
use crate::sys::pool::{PR_WAITOK, PR_ZERO, Pool};
use crate::sys::proc::Proc;
use crate::sys::rwlock::Rwlock;
use crate::sys::sigio::sigio_init;
use crate::sys::signal::SIGIO;
use crate::sys::sockio::{SIOCGPGRP, SIOCSPGRP};
use crate::sys::stat::{S_IFIFO, Stat};
use crate::sys::syscallargs::{SysPipe2Args, SysPipeArgs};
use crate::sys::syslimits::PIPE_BUF;
use crate::sys::systm::{INFSLP, SysArgs, sysargs};
use crate::sys::ttycom::{TIOCGPGRP, TIOCSPGRP};
use crate::sys::types::{Blkcnt, Blksize, Off, Register};
use crate::sys::uio::Uio;
use crate::unported;
use crate::uvm::uvm_km::{KD_WAITOK, KP_PAGEABLE, KV_ANY, km_alloc, km_free};

/// `MINPIPESIZE`: below this many buffered bytes a reader wakes a blocked writer (write
/// blocking hysteresis).
const MINPIPESIZE: usize = PIPE_SIZE / 3;

/// `LIMITBIGPIPES`: limit the number of "big" pipes.
const LIMITBIGPIPES: u32 = 32;

/// `struct pipe_pair`: the storage of both directions of a pipe and the lock they share.
pub struct PipePair {
    /// `pp_wpipe`: the pipe `fds[1]` reads.
    pub pp_wpipe: Pipe,
    /// `pp_rpipe`: the pipe `fds[0]` reads.
    pub pp_rpipe: Pipe,
    /// `pp_lock`: one lock is used per pipe pair in order to obtain exclusive access to the
    /// pipe pair.
    pub pp_lock: Rwlock,
}

impl PipePair {
    /// An unlinked pair, as `pool_get(PR_ZERO)` returns it before `pipe_pair_create`.
    pub const fn new() -> Self {
        Self {
            pp_wpipe: Pipe::new(),
            pp_rpipe: Pipe::new(),
            pp_lock: Rwlock::new("pipelk"),
        }
    }
}

impl Default for PipePair {
    fn default() -> Self {
        Self::new()
    }
}

/// What a pipe filter reports through its knote, for `kern_event.c` to apply once it exists:
/// the filter's return value, `kn_data`, and whether it sets `EV_EOF` and `__EV_HUP` in
/// `kn_flags`. Not a C type (see the module's deviations).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PipeFilter {
    /// The filter's result: the event is active.
    pub active: bool,
    /// `kn->kn_data`, when the filter sets it.
    pub kn_data: Option<i64>,
    /// `kn->kn_flags |= EV_EOF`.
    pub eof: bool,
    /// `kn->kn_flags |= __EV_HUP`.
    pub hup: bool,
}

/// `pipeops`: the interfaces to the outside world.
static PIPEOPS: Fileops = Fileops {
    fo_read: pipe_read,
    fo_write: pipe_write,
    fo_ioctl: pipe_ioctl,
    fo_kqfilter: pipe_kqfilter,
    fo_stat: pipe_stat,
    fo_close: pipe_close,
    fo_seek: None,
};

/// `nbigpipe`: the number of pipes whose buffer is `BIG_PIPE_SIZE`.
pub static NBIGPIPE: AtomicU32 = AtomicU32::new(0);

/// `amountpipekva`: the kernel memory held by pipe buffers.
static AMOUNTPIPEKVA: AtomicU32 = AtomicU32::new(0);

/// `pipe_pair_pool`.
pub static PIPE_PAIR_POOL: Pool = Pool::new();

/// `fp->f_data` of a pipe file.
pub fn fp_pipe(fp: &File) -> &Pipe {
    if fp.f_type.get() != DTYPE_PIPE {
        panic(format_args!("file {:p}: not a pipe", fp));
    }
    // SAFETY: `dopipe` points the `f_data` of every `DTYPE_PIPE` file at one pipe of a live
    // pair, which stays allocated until that file's `pipe_close` (which clears `f_data`)
    // and its peer's have both run; the file outlives the borrow.
    match unsafe { fp.f_data.get().cast::<Pipe>().as_ref() } {
        Some(pipe) => pipe,
        None => panic(format_args!("file {:p}: pipe already closed", fp)),
    }
}

/// The int an `ioctl` argument holds.
fn int_arg(data: &[u8]) -> i32 {
    data.first_chunk::<4>()
        .map_or(0, |b| i32::from_ne_bytes(*b))
}

/// Stores an int into an `ioctl` argument.
fn set_int_arg(data: &mut [u8], v: i32) {
    if let Some(b) = data.first_chunk_mut::<4>() {
        *b = v.to_ne_bytes();
    }
}

/// The pipe system call for the `DTYPE_PIPE` type of pipes.
pub fn sys_pipe(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysPipeArgs = sysargs(v);

    dopipe(p, uap.fdp.get() as usize, 0)
}

/// `pipe2(2)`: `pipe(2)` with `O_CLOEXEC`, `O_CLOFORK` and `O_NONBLOCK`.
pub fn sys_pipe2(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysPipe2Args = sysargs(v);
    let flags = uap.flags.get();

    if flags & !(O_CLOEXEC | O_CLOFORK | FNONBLOCK) != 0 {
        return Err(Errno::EINVAL);
    }

    dopipe(p, uap.fdp.get() as usize, flags)
}

/// `dopipe(p, ufds, flags)`: creates a pipe pair, installs its two files at the lowest free
/// descriptors and copies them out to the two `int`s at `ufds`.
pub fn dopipe(p: &Proc, ufds: usize, flags: i32) -> Result<(), Errno> {
    let fdp = p.fd();

    let fdflags = (if flags & O_CLOEXEC != 0 {
        UF_EXCLOSE
    } else {
        0
    }) | (if flags & O_CLOFORK != 0 {
        UF_FORKCLOSE
    } else {
        0
    });

    let Some(pp) = pipe_pair_create() else {
        return Err(Errno::ENOMEM);
    };
    let wpipe = NonNull::from(&pp.pp_wpipe);
    let rpipe = NonNull::from(&pp.pp_rpipe);
    let fflag = (FREAD | FWRITE | (flags & FNONBLOCK)) as u32;

    fdplock(fdp);

    let (rf, fd0) = match falloc(p) {
        Ok(r) => r,
        Err(error) => {
            // free2:
            fdpunlock(fdp);
            // SAFETY: the pair is new and no file refers to it; neither pipe is used again.
            unsafe {
                pipe_destroy(Some(wpipe));
                pipe_destroy(Some(rpipe));
            }
            return Err(error);
        }
    };
    rf.f_flag.store(fflag, Ordering::SeqCst);
    rf.f_type.set(DTYPE_PIPE);
    rf.f_data.set(rpipe.as_ptr().cast::<c_void>());
    rf.f_ops.set(Some(&PIPEOPS));

    let (wf, fd1) = match falloc(p) {
        Ok(r) => r,
        Err(error) => {
            // free3:
            fdremove(fdp, fd0);
            // The last reference to `rf` closes it, which destroys `rpipe`.
            let _ = closef(rf, p);
            // free2:
            fdpunlock(fdp);
            // SAFETY: no file refers to `wpipe`; it is not used again.
            unsafe { pipe_destroy(Some(wpipe)) };
            return Err(error);
        }
    };
    wf.f_flag.store(fflag, Ordering::SeqCst);
    wf.f_type.set(DTYPE_PIPE);
    wf.f_data.set(wpipe.as_ptr().cast::<c_void>());
    wf.f_ops.set(Some(&PIPEOPS));

    fdinsert(fdp, fd0, fdflags, rf);
    fdinsert(fdp, fd1, fdflags, wf);

    let mut fds = [0u8; 2 * size_of::<i32>()];
    fds[..4].copy_from_slice(&fd0.to_ne_bytes());
    fds[4..].copy_from_slice(&fd1.to_ne_bytes());
    let error = copyout(&fds, ufds);
    if error.is_ok() {
        fdpunlock(fdp);
        // KTRACE (ktrfds): not configured.
    } else {
        // fdrelease() unlocks fdp.
        let _ = fdrelease(p, fd0);
        fdplock(fdp);
        let _ = fdrelease(p, fd1);
    }

    let _ = frele(rf, p);
    let _ = frele(wf, p);
    error
}

/// Allocate kva for pipe circular buffer, the space is pageable. This routine will 'realloc'
/// the size of a pipe safely, if it fails it will retain the old buffer. If it fails it will
/// return `ENOMEM`.
pub fn pipe_buffer_realloc(cpipe: &Pipe, size: usize) -> Result<(), Errno> {
    let pb = &cpipe.pipe_buffer;

    // buffer uninitialized or pipe locked
    kassert!(pb.buffer.get().is_null() || cpipe.has_state(PIPE_LOCK));

    // buffer should be empty
    kassert!(pb.cnt.get() == 0);

    let Some(buffer) = km_alloc(size, &KV_ANY, &KP_PAGEABLE, &KD_WAITOK) else {
        return Err(Errno::ENOMEM);
    };

    // free old resources if we are resizing
    pipe_buffer_free(cpipe);

    pb.buffer.set(buffer.as_ptr());
    pb.size.set(size as u32);
    pb.r#in.set(0);
    pb.out.set(0);

    AMOUNTPIPEKVA.fetch_add(pb.size.get(), Ordering::SeqCst);

    Ok(())
}

/// Initialize and allocate VM and memory for pipe.
pub fn pipe_create(cpipe: &Pipe) -> Result<(), Errno> {
    pipe_buffer_realloc(cpipe, PIPE_SIZE)?;

    sigio_init(&cpipe.pipe_sigio);

    let now = getnanotime();
    cpipe.pipe_ctime.set(now);
    cpipe.pipe_atime.set(now);
    cpipe.pipe_mtime.set(now);

    Ok(())
}

/// `pipe_peer(cpipe)`: the other direction, or `None` once it is gone or at EOF.
pub fn pipe_peer(cpipe: &Pipe) -> Option<&Pipe> {
    rw_assert_anylock(cpipe.lock());

    // SAFETY: the peer is the other pipe of the same pair, and `pipe_destroy` clears this
    // link (under the pair's lock, held here) before the pair can be freed.
    let peer = unsafe { cpipe.pipe_peer.get().as_ref() }?;
    if peer.has_state(PIPE_EOF) {
        return None;
    }
    Some(peer)
}

/// Lock a pipe for exclusive I/O access.
pub fn pipe_iolock(cpipe: &Pipe) -> Result<(), Errno> {
    let lock = cpipe.lock();
    rw_assert_wrlock(lock);

    while cpipe.has_state(PIPE_LOCK) {
        cpipe.set_state(PIPE_LWANT);
        rwsleep_nsec(
            ptr::from_ref(cpipe),
            lock,
            PRIBIO | PCATCH,
            "pipeiolk",
            INFSLP,
        )?;
    }
    cpipe.set_state(PIPE_LOCK);
    Ok(())
}

/// Unlock a pipe I/O lock.
pub fn pipe_iounlock(cpipe: &Pipe) {
    rw_assert_wrlock(cpipe.lock());
    kassert!(cpipe.has_state(PIPE_LOCK));

    cpipe.clear_state(PIPE_LOCK);
    if cpipe.has_state(PIPE_LWANT) {
        cpipe.clear_state(PIPE_LWANT);
        wakeup(ptr::from_ref(cpipe));
    }
}

/// Unlock the pipe I/O lock and go to sleep. Returns `Ok` on success and the I/O lock is
/// relocked. Otherwise if a signal was caught, the error is returned and the I/O lock is not
/// locked.
///
/// Any caller must obtain a reference to the pipe by incrementing `pipe_busy` before calling
/// this function in order ensure that the same pipe is not destroyed while sleeping.
pub fn pipe_iosleep(cpipe: &Pipe, wmesg: &'static str) -> Result<(), Errno> {
    pipe_iounlock(cpipe);
    rwsleep_nsec(
        ptr::from_ref(cpipe),
        cpipe.lock(),
        PRIBIO | PCATCH,
        wmesg,
        INFSLP,
    )?;
    pipe_iolock(cpipe)
}

/// `pipe_wakeup(cpipe)`: tells the pipe's waiters (knotes, and `SIGIO` for `FIOASYNC`)
/// that its state changed.
pub fn pipe_wakeup(cpipe: &Pipe) {
    rw_assert_wrlock(cpipe.lock());

    // knote_locked(&cpipe->pipe_klist, 0): the kqueue, select and poll waiters.
    let _ = unported!("pipe_wakeup: knote_locked (kern_event.c)");

    if cpipe.has_state(PIPE_ASYNC) {
        pgsigio(&cpipe.pipe_sigio, SIGIO, false);
    }
}

/// `fo_read` of a pipe: copies out what is buffered, waiting for data unless the file is
/// non-blocking; 0 bytes at EOF.
pub fn pipe_read(fp: &File, uio: &mut Uio<'_>, _fflags: i32) -> Result<(), Errno> {
    let rpipe = fp_pipe(fp);
    let lock = rpipe.lock();
    let pb = &rpipe.pipe_buffer;
    let mut nread: usize = 0;

    rw_enter_write(lock);
    rpipe.pipe_busy.set(rpipe.pipe_busy.get() + 1);
    if let Err(error) = pipe_iolock(rpipe) {
        rpipe.pipe_busy.set(rpipe.pipe_busy.get() - 1);
        pipe_rundown(rpipe);
        rw_exit_write(lock);
        return Err(error);
    }

    let mut error = Ok(());
    // `false` when a sleep failed and the I/O lock is not held (the C's
    // `goto unlocked_error`).
    let iolocked = 'io: {
        while uio.uio_resid > 0 {
            if pb.cnt.get() > 0 {
                // Normal pipe buffer receive.
                let out = pb.out.get() as usize;
                let size = (pb.size.get() as usize - out)
                    .min(pb.cnt.get() as usize)
                    .min(uio.uio_resid);
                let buffer = pb.buffer.get();
                rw_exit_write(lock);
                // SAFETY: `buffer` is the pipe's `size`-byte buffer and `out + size` stays
                // inside it. The I/O lock (PIPE_LOCK) is ours, so no other thread reads,
                // writes, resizes or frees the buffer meanwhile (`pipe_destroy` waits for
                // `pipe_busy`).
                let bytes = unsafe { slice::from_raw_parts_mut(buffer.add(out), size) };
                let result = uiomove(bytes, uio);
                rw_enter_write(lock);
                if let Err(e) = result {
                    error = Err(e);
                    break;
                }
                let mut out = pb.out.get() as usize + size;
                if out >= pb.size.get() as usize {
                    out = 0;
                }
                pb.out.set(out as u32);

                pb.cnt.set(pb.cnt.get() - size as u32);
                // If there is no more to read in the pipe, reset its pointers to the
                // beginning. This improves cache hit stats.
                if pb.cnt.get() == 0 {
                    pb.r#in.set(0);
                    pb.out.set(0);
                }
                nread += size;
            } else {
                // detect EOF condition
                // read returns 0 on EOF, no need to set error
                if rpipe.has_state(PIPE_EOF) {
                    break;
                }

                // If the "write-side" has been blocked, wake it up.
                if rpipe.has_state(PIPE_WANTW) {
                    rpipe.clear_state(PIPE_WANTW);
                    wakeup(ptr::from_ref(rpipe));
                }

                // Break if some data was read.
                if nread > 0 {
                    break;
                }

                // Handle non-blocking mode operation.
                if fp.flag() & FNONBLOCK != 0 {
                    error = Err(Errno::EAGAIN);
                    break;
                }

                // Wait for more data.
                rpipe.set_state(PIPE_WANTR);
                if let Err(e) = pipe_iosleep(rpipe, "piperd") {
                    error = Err(e);
                    break 'io false;
                }
            }
        }
        true
    };
    if iolocked {
        pipe_iounlock(rpipe);

        if error.is_ok() {
            rpipe.pipe_atime.set(getnanotime());
        }
    }
    // unlocked_error:
    rpipe.pipe_busy.set(rpipe.pipe_busy.get() - 1);

    if !pipe_rundown(rpipe) && (pb.cnt.get() as usize) < MINPIPESIZE {
        // Handle write blocking hysteresis.
        if rpipe.has_state(PIPE_WANTW) {
            rpipe.clear_state(PIPE_WANTW);
            wakeup(ptr::from_ref(rpipe));
        }
    }

    if (pb.size.get() - pb.cnt.get()) as usize >= PIPE_BUF {
        pipe_wakeup(rpipe);
    }

    rw_exit_write(lock);
    error
}

/// `fo_write` of a pipe: copies into the peer's buffer, waiting for space unless the file is
/// non-blocking; `EPIPE` once the reader is gone. Writes of at most `PIPE_BUF` bytes are
/// atomic.
pub fn pipe_write(fp: &File, uio: &mut Uio<'_>, _fflags: i32) -> Result<(), Errno> {
    let rpipe = fp_pipe(fp);
    let lock = rpipe.lock();

    rw_enter_write(lock);
    let wpipe = pipe_peer(rpipe);

    // Detect loss of pipe read side, issue SIGPIPE if lost.
    let Some(wpipe) = wpipe else {
        rw_exit_write(lock);
        return Err(Errno::EPIPE);
    };
    let pb = &wpipe.pipe_buffer;

    wpipe.pipe_busy.set(wpipe.pipe_busy.get() + 1);
    if let Err(error) = pipe_iolock(wpipe) {
        wpipe.pipe_busy.set(wpipe.pipe_busy.get() - 1);
        pipe_rundown(wpipe);
        rw_exit_write(lock);
        return Err(error);
    }

    // If it is advantageous to resize the pipe buffer, do so.
    if uio.uio_resid > PIPE_SIZE && pb.size.get() as usize <= PIPE_SIZE && pb.cnt.get() == 0 {
        let npipe = NBIGPIPE.fetch_add(1, Ordering::SeqCst) + 1;
        if npipe > LIMITBIGPIPES || pipe_buffer_realloc(wpipe, BIG_PIPE_SIZE).is_err() {
            NBIGPIPE.fetch_sub(1, Ordering::SeqCst);
        }
    }

    let orig_resid = uio.uio_resid;

    let mut error = Ok(());
    // `false` when a sleep failed and the I/O lock is not held (the C's
    // `goto unlocked_error`).
    let iolocked = 'io: {
        while uio.uio_resid > 0 {
            if wpipe.has_state(PIPE_EOF) {
                error = Err(Errno::EPIPE);
                break;
            }

            let mut space = (pb.size.get() - pb.cnt.get()) as usize;

            // Writes of size <= PIPE_BUF must be atomic.
            if space < uio.uio_resid && orig_resid <= PIPE_BUF {
                space = 0;
            }

            if space > 0 {
                // Transfer size is minimum of uio transfer and free space in pipe buffer.
                let size = space.min(uio.uio_resid);
                // First segment to transfer is minimum of transfer size and contiguous
                // space in pipe buffer. If first segment to transfer is less than the
                // transfer size, we've got a wraparound in the buffer.
                let bufsize = pb.size.get() as usize;
                let r#in = pb.r#in.get() as usize;
                let segsize = (bufsize - r#in).min(size);
                let buffer = pb.buffer.get();

                // Transfer first segment
                rw_exit_write(lock);
                // SAFETY: `buffer` is the peer's `bufsize`-byte buffer and
                // `in + segsize <= bufsize`. The peer's I/O lock (PIPE_LOCK) is ours, so no
                // other thread reads, writes, resizes or frees the buffer meanwhile
                // (`pipe_destroy` waits for `pipe_busy`).
                let bytes = unsafe { slice::from_raw_parts_mut(buffer.add(r#in), segsize) };
                let mut result = uiomove(bytes, uio);
                rw_enter_write(lock);

                if result.is_ok() && segsize < size {
                    // Transfer remaining part now, to support atomic writes. Wraparound
                    // happened.
                    #[cfg(feature = "diagnostic")]
                    if pb.r#in.get() as usize + segsize != pb.size.get() as usize {
                        panic(format_args!("Expected pipe buffer wraparound disappeared"));
                    }

                    rw_exit_write(lock);
                    // SAFETY: as above; `size - segsize <= bufsize - segsize`, the free
                    // bytes at the start of the buffer.
                    let bytes = unsafe { slice::from_raw_parts_mut(buffer, size - segsize) };
                    result = uiomove(bytes, uio);
                    rw_enter_write(lock);
                }
                if result.is_ok() {
                    let mut r#in = pb.r#in.get() as usize + size;
                    if r#in >= pb.size.get() as usize {
                        #[cfg(feature = "diagnostic")]
                        if r#in != size - segsize + pb.size.get() as usize {
                            panic(format_args!("Expected wraparound bad"));
                        }
                        r#in = size - segsize;
                    }
                    pb.r#in.set(r#in as u32);

                    pb.cnt.set(pb.cnt.get() + size as u32);
                    #[cfg(feature = "diagnostic")]
                    if pb.cnt.get() > pb.size.get() {
                        panic(format_args!("Pipe buffer overflow"));
                    }
                }
                if let Err(e) = result {
                    error = Err(e);
                    break;
                }
            } else {
                // If the "read-side" has been blocked, wake it up.
                if wpipe.has_state(PIPE_WANTR) {
                    wpipe.clear_state(PIPE_WANTR);
                    wakeup(ptr::from_ref(wpipe));
                }

                // Don't block on non-blocking I/O.
                if fp.flag() & FNONBLOCK != 0 {
                    error = Err(Errno::EAGAIN);
                    break;
                }

                // We have no more space and have something to offer, wake up select/poll.
                pipe_wakeup(wpipe);

                wpipe.set_state(PIPE_WANTW);
                if let Err(e) = pipe_iosleep(wpipe, "pipewr") {
                    error = Err(e);
                    break 'io false;
                }

                // If read side wants to go away, we just issue a signal to ourselves.
                if wpipe.has_state(PIPE_EOF) {
                    error = Err(Errno::EPIPE);
                    break;
                }
            }
        }
        true
    };
    if iolocked {
        pipe_iounlock(wpipe);
    }

    // unlocked_error:
    wpipe.pipe_busy.set(wpipe.pipe_busy.get() - 1);

    if !pipe_rundown(wpipe) && pb.cnt.get() > 0 {
        // If we have put any characters in the buffer, we wake up the reader.
        if wpipe.has_state(PIPE_WANTR) {
            wpipe.clear_state(PIPE_WANTR);
            wakeup(ptr::from_ref(wpipe));
        }
    }

    // Don't return EPIPE if I/O was successful.
    if pb.cnt.get() == 0 && uio.uio_resid == 0 && error == Err(Errno::EPIPE) {
        error = Ok(());
    }

    if error.is_ok() {
        wpipe.pipe_mtime.set(getnanotime());
    }
    // We have something to offer, wake up select/poll.
    if pb.cnt.get() != 0 {
        pipe_wakeup(wpipe);
    }

    rw_exit_write(lock);
    error
}

/// We implement a very minimal set of ioctls for compatibility with sockets.
pub fn pipe_ioctl(fp: &File, cmd: u64, data: &mut [u8], _p: &Proc) -> Result<(), Errno> {
    let mpipe = fp_pipe(fp);

    match cmd {
        FIOASYNC => {
            rw_enter_write(mpipe.lock());
            if int_arg(data) != 0 {
                mpipe.set_state(PIPE_ASYNC);
            } else {
                mpipe.clear_state(PIPE_ASYNC);
            }
            rw_exit_write(mpipe.lock());
            Ok(())
        }

        FIONREAD => {
            rw_enter_read(mpipe.lock());
            set_int_arg(data, mpipe.pipe_buffer.cnt.get() as i32);
            rw_exit_read(mpipe.lock());
            Ok(())
        }

        FIOSETOWN | SIOCSPGRP | TIOCSPGRP => sigio_setown(&mpipe.pipe_sigio, cmd, &int_arg(data)),

        FIOGETOWN | SIOCGPGRP | TIOCGPGRP => {
            let mut pgid = 0;
            sigio_getown(&mpipe.pipe_sigio, cmd, &mut pgid);
            set_int_arg(data, pgid);
            Ok(())
        }

        _ => Err(Errno::ENOTTY),
    }
}

/// `fo_stat` of a pipe: a FIFO whose size is what is buffered.
pub fn pipe_stat(fp: &File, ub: &mut Stat, _p: &Proc) -> Result<(), Errno> {
    let pipe = fp_pipe(fp);

    *ub = Stat::default();

    rw_enter_read(pipe.lock());
    ub.st_mode = S_IFIFO;
    ub.st_blksize = pipe.pipe_buffer.size.get() as Blksize;
    ub.st_size = Off::from(pipe.pipe_buffer.cnt.get());
    ub.st_blocks = (ub.st_size + Blkcnt::from(ub.st_blksize) - 1) / Blkcnt::from(ub.st_blksize);
    ub.st_atim = pipe.pipe_atime.get();
    ub.st_mtim = pipe.pipe_mtime.get();
    ub.st_ctim = pipe.pipe_ctime.get();
    // SAFETY: every open file holds a reference to its credentials (`fnew`).
    let cred = unsafe { &*fp.f_cred.get() };
    ub.st_uid = cred.cr_uid.get();
    ub.st_gid = cred.cr_gid.get();
    rw_exit_read(pipe.lock());
    // Left as 0: st_dev, st_ino, st_nlink, st_rdev, st_flags, st_gen.
    // XXX (st_dev, st_ino) should be unique.
    Ok(())
}

/// `fo_close` of a pipe: the file's last reference is gone; destroy its pipe.
pub fn pipe_close(fp: &File, _p: Option<&Proc>) -> Result<(), Errno> {
    let cpipe = NonNull::new(fp.f_data.get().cast::<Pipe>());

    fp.f_ops.set(None);
    fp.f_data.set(ptr::null_mut());
    // SAFETY: `fdrop` calls this once, when the last reference to the file goes; the pipe
    // was this file's alone and nothing reaches it through the file any more.
    unsafe { pipe_destroy(cpipe) };
    Ok(())
}

/// Free kva for pipe circular buffer. No pipe lock check as only called from
/// `pipe_buffer_realloc()` and `pipeclose()`.
pub fn pipe_buffer_free(cpipe: &Pipe) {
    let pb = &cpipe.pipe_buffer;

    let Some(buffer) = NonNull::new(pb.buffer.get()) else {
        return;
    };

    let size = pb.size.get();

    km_free(buffer, size as usize, &KV_ANY, &KP_PAGEABLE);

    pb.buffer.set(ptr::null_mut());

    AMOUNTPIPEKVA.fetch_sub(size, Ordering::SeqCst);
    if size as usize > PIPE_SIZE {
        NBIGPIPE.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Shutdown the pipe, and free resources.
///
/// # Safety
///
/// `cpipe` is `None` or one pipe of a live pair that no file refers to any more (its file's
/// last reference is gone, or it never had one), destroyed only once. The caller does not
/// use it afterwards: the pair goes back to the pool when its second pipe is destroyed.
pub unsafe fn pipe_destroy(cpipe: Option<NonNull<Pipe>>) {
    let Some(cpipe) = cpipe else {
        return;
    };
    // SAFETY: the caller's contract: the pair is live until the end of this function.
    let cpipe = unsafe { cpipe.as_ref() };
    let lock = cpipe.lock();

    rw_enter_write(lock);

    pipe_wakeup(cpipe);
    sigio_free(&cpipe.pipe_sigio);

    // If the other side is blocked, wake it up saying that we want to close it down.
    cpipe.set_state(PIPE_EOF);
    while cpipe.pipe_busy.get() != 0 {
        wakeup(ptr::from_ref(cpipe));
        cpipe.set_state(PIPE_WANTD);
        let _ = rwsleep_nsec(ptr::from_ref(cpipe), lock, PRIBIO, "pipecl", INFSLP);
    }

    // Disconnect from peer.
    let ppipe = cpipe.pipe_peer.get();
    // SAFETY: the peer is the other pipe of the same pair, alive while this one is.
    if let Some(peer) = unsafe { ppipe.as_ref() } {
        pipe_wakeup(peer);

        peer.set_state(PIPE_EOF);
        wakeup(ppipe);
        peer.pipe_peer.set(ptr::null());
    }

    pipe_buffer_free(cpipe);

    rw_exit_write(lock);

    if ppipe.is_null() {
        let pp = cpipe.pipe_pair.get();
        if let Some(pp) = NonNull::new(pp.cast_mut()) {
            // SAFETY: both pipes of the pair are destroyed (the peer cleared our link when
            // it went), so nothing refers to the pair any more.
            unsafe { pipe_pair_destroy(pp) };
        }
    }
}

/// Returns `true` if a rundown is currently ongoing.
pub fn pipe_rundown(cpipe: &Pipe) -> bool {
    rw_assert_wrlock(cpipe.lock());

    if cpipe.pipe_busy.get() > 0 || !cpipe.has_state(PIPE_WANTD) {
        return false;
    }

    // Only wakeup pipe_destroy() once the pipe is no longer busy.
    cpipe.clear_state(PIPE_WANTD | PIPE_WANTR | PIPE_WANTW);
    wakeup(ptr::from_ref(cpipe));
    true
}

/// `fo_kqfilter` of a pipe: attaches a read, write or (poll's) except knote.
pub fn pipe_kqfilter(fp: &File, _kn: &Knote) -> Result<(), Errno> {
    let rpipe = fp_pipe(fp);
    let lock = rpipe.lock();

    rw_enter_write(lock);
    let _wpipe = pipe_peer(rpipe);

    // switch (kn->kn_filter): EVFILT_READ hooks the knote on `rpipe`; EVFILT_WRITE on the
    // peer, or on `rpipe` once the other end is closed (the filter then always reports an
    // event); EVFILT_EXCEPT only for poll (EPERM for select, EINVAL for kevent(2)); others
    // EINVAL. kn_fop is pipe_rfiltops/pipe_wfiltops/pipe_efiltops, kn_hook the pipe, and
    // klist_insert_locked(&pipe->pipe_klist, kn): struct knote (kern_event.c).
    let error = Err(unported!("pipe_kqfilter: struct knote (kern_event.c)"));

    rw_exit_write(lock);

    error
}

/// `filt_pipedetach(kn)`: unhooks the knote from `kn->kn_hook`'s list.
pub fn filt_pipedetach(_cpipe: &Pipe) {
    // klist_remove(&cpipe->pipe_klist, kn): kern_event.c.
    let _ = unported!("filt_pipedetach: klist_remove (kern_event.c)");
}

/// `filt_piperead(kn, hint)` for the pipe of the knote's file; `poll` is
/// `kn->kn_flags & __EV_POLL`.
pub fn filt_piperead(rpipe: &Pipe, poll: bool) -> PipeFilter {
    rw_assert_wrlock(rpipe.lock());

    let wpipe = pipe_peer(rpipe);

    let kn_data = i64::from(rpipe.pipe_buffer.cnt.get());

    if rpipe.has_state(PIPE_EOF) || wpipe.is_none() {
        return PipeFilter {
            active: true,
            kn_data: Some(kn_data),
            eof: true,
            hup: poll,
        };
    }

    PipeFilter {
        active: kn_data > 0,
        kn_data: Some(kn_data),
        ..PipeFilter::default()
    }
}

/// `filt_pipewrite(kn, hint)` for the pipe of the knote's file; `poll` is
/// `kn->kn_flags & __EV_POLL`.
pub fn filt_pipewrite(rpipe: &Pipe, poll: bool) -> PipeFilter {
    rw_assert_wrlock(rpipe.lock());

    let Some(wpipe) = pipe_peer(rpipe) else {
        return PipeFilter {
            active: true,
            kn_data: Some(0),
            eof: true,
            hup: poll,
        };
    };
    let kn_data = i64::from(wpipe.pipe_buffer.size.get() - wpipe.pipe_buffer.cnt.get());

    PipeFilter {
        active: kn_data >= PIPE_BUF as i64,
        kn_data: Some(kn_data),
        ..PipeFilter::default()
    }
}

/// `filt_pipeexcept(kn, hint)` for the pipe of the knote's file; `poll` is
/// `kn->kn_flags & __EV_POLL`.
pub fn filt_pipeexcept(rpipe: &Pipe, poll: bool) -> PipeFilter {
    let mut active = PipeFilter::default();

    rw_assert_wrlock(rpipe.lock());

    let wpipe = pipe_peer(rpipe);

    if poll && (rpipe.has_state(PIPE_EOF) || wpipe.is_none()) {
        active.hup = true;
        active.active = true;
    }

    active
}

/// `filt_pipemodify(kev, kn)`: `knote_modify` under the pipe's lock.
pub fn filt_pipemodify(rpipe: &Pipe) -> bool {
    rw_enter_write(rpipe.lock());
    let _ = unported!("filt_pipemodify: knote_modify (kern_event.c)");
    rw_exit_write(rpipe.lock());

    false
}

/// `filt_pipeprocess(kn, kev)`: `knote_process` under the pipe's lock.
pub fn filt_pipeprocess(rpipe: &Pipe) -> bool {
    rw_enter_write(rpipe.lock());
    let _ = unported!("filt_pipeprocess: knote_process (kern_event.c)");
    rw_exit_write(rpipe.lock());

    false
}

/// `pipe_init`: the pool of pipe pairs.
pub fn pipe_init() {
    pool_init(
        &PIPE_PAIR_POOL,
        size_of::<PipePair>(),
        0,
        IPL_MPFLOOR,
        PR_WAITOK,
        "pipepl",
        None,
    );
}

/// `pipe_pair_create()`: a new pair, linked and with both buffers, or `None`.
pub fn pipe_pair_create() -> Option<&'static PipePair> {
    // PR_WAITOK cannot sleep yet (see the module's deviations).
    let mem = pool_get(&PIPE_PAIR_POOL, PR_WAITOK | PR_ZERO)?;
    let raw = mem.cast::<PipePair>().as_ptr();
    // SAFETY: a fresh, suitably aligned pool item of `size_of::<PipePair>()` bytes, written
    // once before anything else sees it.
    unsafe { raw.write(PipePair::new()) };
    // SAFETY: as above; the item stays allocated until `pipe_pair_destroy`.
    let pp: &'static PipePair = unsafe { &*raw };

    pp.pp_wpipe.pipe_pair.set(raw);
    pp.pp_rpipe.pipe_pair.set(raw);
    pp.pp_wpipe.pipe_peer.set(&pp.pp_rpipe);
    pp.pp_rpipe.pipe_peer.set(&pp.pp_wpipe);
    // One lock is used per pipe pair in order to obtain exclusive access to the pipe pair.
    rw_init(&pp.pp_lock, "pipelk");
    pp.pp_wpipe.pipe_lock.set(&pp.pp_lock);
    pp.pp_rpipe.pipe_lock.set(&pp.pp_lock);

    // klist_init_rwlock(&pp->pp_wpipe.pipe_klist, &pp->pp_lock) and the same for
    // pp_rpipe: kern_event.c.

    if pipe_create(&pp.pp_wpipe).is_err() || pipe_create(&pp.pp_rpipe).is_err() {
        // err:
        // SAFETY: the pair is new and no file refers to it; neither pipe is used again.
        unsafe {
            pipe_destroy(Some(NonNull::from(&pp.pp_wpipe)));
            pipe_destroy(Some(NonNull::from(&pp.pp_rpipe)));
        }
        return None;
    }
    Some(pp)
}

/// `pipe_pair_destroy(pp)`: returns the pair to the pool.
///
/// # Safety
///
/// `pp` came from `pipe_pair_create`, both of its pipes are destroyed, and nothing refers
/// to it any more.
pub unsafe fn pipe_pair_destroy(pp: NonNull<PipePair>) {
    // klist_free(&pp->pp_wpipe.pipe_klist) and the same for pp_rpipe: kern_event.c.
    pool_put(&PIPE_PAIR_POOL, pp.cast());
}

#[cfg(test)]
mod tests;
