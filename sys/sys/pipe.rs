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
 * 4. This work was done expressly for inclusion into FreeBSD.  Other use
 *    is allowed if this notation is included.
 * 5. Modifications may be freely made to this file if the above conditions
 *    are met.
 */
/* </LICENSES> */

/* <CODE> */
//! `<sys/pipe.h>`: the per-direction pipe structure, `struct pipe`, its circular buffer
//! `struct pipebuf`, the buffer sizes and the `PIPE_*` state bits.
//!
//! Upstream: sys/sys/pipe.h @ 3ce1f3f79392
//!
//! Two `struct pipe`s, one per direction, live in one `struct pipe_pair` (a pool item defined
//! by `kern/sys_pipe.c`, here `kern::sys_pipe::PipePair`) and share its rwlock. A pipe is
//! reached through its file's `f_data` for as long as the file is open, and through its
//! peer's `pipe_peer` until it is destroyed; the pair goes back to the pool when its second
//! side is destroyed.
//!
//! ## Deviations
//! - The members the C guards with `pipe_lock` (`[p]`) are `Cell`s; `pipe_lock`,
//!   `pipe_peer` and `pipe_pair` are raw pointers into the pair (`Cell<*const T>`), read
//!   through [`Pipe::lock`] and the functions of `sys_pipe.c`.
//! - `struct pipebuf`'s member `in` is a Rust keyword: it is the raw identifier `r#in`.
//! - `PIPE_SIZE` and `BIG_PIPE_SIZE` are `usize` (they size `km_alloc` and compare with
//!   `uio_resid`); the C's `#ifndef` overrides are not configurable.

use core::cell::Cell;
use core::ptr;

use crate::kern::subr_prf::panic;
use crate::kern::sys_pipe::PipePair;
use crate::sys::event::Klist;
use crate::sys::rwlock::Rwlock;
use crate::sys::sigio::SigioRef;
use crate::sys::time::Timespec;

/// `PIPE_SIZE`: pipe buffer size, keep moderate in value, pipes take kva space.
pub const PIPE_SIZE: usize = 16384;

/// `BIG_PIPE_SIZE`: the size a pipe grows to for a large write.
pub const BIG_PIPE_SIZE: usize = 64 * 1024;

/// `PIPE_ASYNC`: async I/O.
pub const PIPE_ASYNC: u32 = 0x004;
/// `PIPE_WANTR`: reader wants some characters.
pub const PIPE_WANTR: u32 = 0x008;
/// `PIPE_WANTW`: writer wants space to put characters.
pub const PIPE_WANTW: u32 = 0x010;
/// `PIPE_WANTD`: pipe is wanted to be run-down.
pub const PIPE_WANTD: u32 = 0x020;
/// `PIPE_EOF`: pipe is in EOF condition.
pub const PIPE_EOF: u32 = 0x080;
/// `PIPE_LOCK`: thread has exclusive I/O access.
pub const PIPE_LOCK: u32 = 0x100;
/// `PIPE_LWANT`: thread wants exclusive I/O access.
pub const PIPE_LWANT: u32 = 0x200;

/// `struct pipebuf`: pipe buffer information. Separate in, out, cnt are used to simplify
/// calculations. Buffered write is active when the buffer.cnt field is set.
///
/// Protected by: the pipe's `pipe_lock`; the bytes of `buffer` by `PIPE_LOCK`.
pub struct Pipebuf {
    /// `cnt`: number of chars currently in buffer.
    pub cnt: Cell<u32>,
    /// `in`: in pointer.
    pub r#in: Cell<u32>,
    /// `out`: out pointer.
    pub out: Cell<u32>,
    /// `size`: size of buffer.
    pub size: Cell<u32>,
    /// `buffer`: kva of buffer (`km_alloc`), null when there is none.
    pub buffer: Cell<*mut u8>,
}

impl Pipebuf {
    /// An empty pipe buffer with no memory, as a zeroed pool item has it.
    pub const fn new() -> Self {
        Self {
            cnt: Cell::new(0),
            r#in: Cell::new(0),
            out: Cell::new(0),
            size: Cell::new(0),
            buffer: Cell::new(ptr::null_mut()),
        }
    }
}

impl Default for Pipebuf {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct pipe`: per-pipe data structure. Two of these are linked together to produce
/// bi-directional pipes.
///
/// Locking: \[I\] immutable after creation, \[S\] `sigio_lock`, \[p\] `pipe_lock`.
pub struct Pipe {
    /// \[I\] `pipe_lock`: the pair's `pp_lock`.
    pub pipe_lock: Cell<*const Rwlock>,
    /// \[p\] `pipe_buffer`: data storage.
    pub pipe_buffer: Pipebuf,
    /// \[p\] `pipe_klist`: list of knotes (locked by the pair's `pp_lock`).
    pub pipe_klist: Klist,
    /// \[p\] `pipe_atime`: time of last access.
    pub pipe_atime: Cell<Timespec>,
    /// \[p\] `pipe_mtime`: time of last modify.
    pub pipe_mtime: Cell<Timespec>,
    /// \[I\] `pipe_ctime`: time of status change.
    pub pipe_ctime: Cell<Timespec>,
    /// \[S\] `pipe_sigio`: async I/O registration.
    pub pipe_sigio: SigioRef,
    /// \[p\] `pipe_peer`: link with other direction, null once the peer is destroyed.
    pub pipe_peer: Cell<*const Pipe>,
    /// \[I\] `pipe_pair`: pipe storage.
    pub pipe_pair: Cell<*const PipePair>,
    /// \[p\] `pipe_state`: pipe status info (`PIPE_*`).
    pub pipe_state: Cell<u32>,
    /// \[p\] `pipe_busy`: # readers/writers.
    pub pipe_busy: Cell<i32>,
}

impl Pipe {
    /// A pipe with no lock, buffer or peer yet, as a zeroed pool item has it.
    pub const fn new() -> Self {
        Self {
            pipe_lock: Cell::new(ptr::null()),
            pipe_buffer: Pipebuf::new(),
            pipe_klist: Klist::new(),
            pipe_atime: Cell::new(Timespec::new(0, 0)),
            pipe_mtime: Cell::new(Timespec::new(0, 0)),
            pipe_ctime: Cell::new(Timespec::new(0, 0)),
            pipe_sigio: SigioRef::new(),
            pipe_peer: Cell::new(ptr::null()),
            pipe_pair: Cell::new(ptr::null()),
            pipe_state: Cell::new(0),
            pipe_busy: Cell::new(0),
        }
    }

    /// `cpipe->pipe_lock`: the rwlock of the pair this pipe belongs to.
    pub fn lock(&self) -> &Rwlock {
        // SAFETY: `pipe_pair_create` points `pipe_lock` at the `pp_lock` of the pair that
        // holds this pipe, and the pair is freed only after both of its pipes are destroyed,
        // so the lock lives at least as long as `self`.
        match unsafe { self.pipe_lock.get().as_ref() } {
            Some(lock) => lock,
            None => panic(format_args!("pipe {:p}: no pipe_lock", self)),
        }
    }

    /// `cpipe->pipe_state |= bits`.
    pub fn set_state(&self, bits: u32) {
        self.pipe_state.set(self.pipe_state.get() | bits);
    }

    /// `cpipe->pipe_state &= ~bits`.
    pub fn clear_state(&self, bits: u32) {
        self.pipe_state.set(self.pipe_state.get() & !bits);
    }

    /// `cpipe->pipe_state & bits`, as a test.
    pub fn has_state(&self, bits: u32) -> bool {
        self.pipe_state.get() & bits != 0
    }
}

impl Default for Pipe {
    fn default() -> Self {
        Self::new()
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn constants_match_the_c_header() {
        let defs = crate::reftest::defines("sys/sys/pipe.h");
        let ours = crate::reftest::assert_defines!(defs;
            PIPE_SIZE, BIG_PIPE_SIZE, PIPE_ASYNC, PIPE_WANTR, PIPE_WANTW, PIPE_WANTD, PIPE_EOF,
            PIPE_LOCK, PIPE_LWANT);
        crate::reftest::assert_complete(&defs, "PIPE_", &ours);
    }

    #[test]
    fn state_helpers() {
        let p = Pipe::new();
        p.set_state(PIPE_WANTR | PIPE_EOF);
        assert!(p.has_state(PIPE_EOF));
        p.clear_state(PIPE_WANTR);
        assert_eq!(p.pipe_state.get(), PIPE_EOF);
        assert!(!p.has_state(PIPE_WANTR | PIPE_LOCK));
    }
}
/* </TESTS> */
