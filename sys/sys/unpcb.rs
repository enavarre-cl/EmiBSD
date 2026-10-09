/* <LICENSES> */
/*
 * Copyright (c) 1982, 1986, 1989, 1993
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
 *	@(#)unpcb.h	8.1 (Berkeley) 6/2/93
 */
/* </LICENSES> */

/* <CODE> */
//! Protocol control block for an active instance of a UNIX internal protocol:
//! `<sys/unpcb.h>`.
//!
//! Upstream: sys/sys/unpcb.h @ 3ce1f3f79392
//!
//! A socket may be associated with a vnode in the file system. If so, the `unp_vnode`
//! pointer holds a reference count to this vnode, which should be `vrele`'d when the socket
//! goes away.
//!
//! A socket may be connected to another socket, in which case the control block of the
//! socket to which it is connected is given by `unp_conn`.
//!
//! A socket may be referenced by a number of sockets (e.g. several sockets may be connected
//! to a datagram socket.) These sockets are in a linked list starting with `unp_refs`,
//! linked through `unp_nextref` and null-terminated. Note that a socket may be referenced by
//! a number of other sockets and may also reference a socket (not necessarily one which is
//! referencing it). This generates the need for `unp_refs` and `unp_nextref` to be separate
//! fields.
//!
//! Stream sockets keep copies of receive sockbuf `sb_cc` and `sb_mbcnt` so that changes in
//! the sockbuf may be computed to modify back pressure on the sender accordingly.
//!
//! Locks used to protect struct members: \[I\] immutable after creation, \[G\]
//! `unp_gc_lock`, \[s\] socket lock.
//!
//! ## Deviations
//! - An `Unpcb` is an `unpcb_pool` item handed around as `&'static Unpcb` (valid until
//!   `unp_detach` gives it back); its links to other control blocks, the vnode, the file and
//!   the address mbuf are `Cell<Option<&'static T>>`, the counters `Cell`s.
//! - `struct fdpass` lives inside control mbufs, so it keeps the C layout: `fp` is a raw
//!   `*const File` the internalized message holds a reference through.
//! - `sotounpcb(so)` returns `Option<&'static Unpcb>` (the C's NULL `so_pcb` is `None`).
//! - The `pr_usrreqs` tables and the prototypes are `kern/uipc_usrreq.rs`'s.

use core::cell::Cell;

use crate::queue_adapter;
use crate::sys::file::File;
use crate::sys::mbuf::Mbuf;
use crate::sys::queue::{ListEntry, SlistEntry, SlistHead};
use crate::sys::refcnt::Refcnt;
use crate::sys::socket::Sockpeercred;
use crate::sys::socketvar::Socket;
use crate::sys::time::Timespec;
use crate::sys::types::Ino;
use crate::sys::vnode::Vnode;

// flag bits in unp_flags

/// `UNP_FEIDS`: `unp_connid` contains information.
pub const UNP_FEIDS: i32 = 0x01;
/// `UNP_FEIDSBIND`: `unp_connid` was set by a bind.
pub const UNP_FEIDSBIND: i32 = 0x02;
/// `UNP_BINDING`: unp is binding now.
pub const UNP_BINDING: i32 = 0x04;
/// `UNP_CONNECTING`: unp is connecting now.
pub const UNP_CONNECTING: i32 = 0x08;

// flag bits in unp_gcflags

/// `UNP_GCDEAD`: unp could be dead.
pub const UNP_GCDEAD: i32 = 0x01;

/// `struct unpcb`.
pub struct Unpcb {
    /// `unp_refcnt`: references to this pcb.
    pub unp_refcnt: Refcnt,
    /// \[I\] `unp_socket`: pointer back to socket.
    pub unp_socket: &'static Socket,
    /// \[s\] `unp_vnode`: if associated with file.
    pub unp_vnode: Cell<Option<&'static Vnode>>,
    /// \[G\] `unp_file`: backpointer for `unp_gc()`.
    pub unp_file: Cell<Option<&'static File>>,
    /// \[s\] `unp_conn`: control block of connected socket.
    pub unp_conn: Cell<Option<&'static Unpcb>>,
    /// \[s\] `unp_ino`: fake inode number.
    pub unp_ino: Cell<Ino>,
    /// \[s\] `unp_refs`: referencing socket linked list.
    pub unp_refs: SlistHead<UnpRefs>,
    /// \[s\] `unp_nextref`: link in `unp_refs` list.
    pub unp_nextref: SlistEntry<Unpcb>,
    /// \[s\] `unp_addr`: bound address of socket.
    pub unp_addr: Cell<Option<&'static Mbuf>>,
    /// \[G\] `unp_msgcount`: references from socket rcv buf.
    pub unp_msgcount: Cell<i64>,
    /// \[G\] `unp_gcrefs`: references from gc.
    pub unp_gcrefs: Cell<i64>,
    /// \[s\] `unp_flags`: this unpcb contains peer eids.
    pub unp_flags: Cell<i32>,
    /// \[G\] `unp_gcflags`: garbage collector flags.
    pub unp_gcflags: Cell<i32>,
    /// \[s\] `unp_connid`: id of peer process.
    pub unp_connid: Cell<Sockpeercred>,
    /// \[I\] `unp_ctime`: holds creation time.
    pub unp_ctime: Cell<Timespec>,
    /// \[G\] `unp_link`: link in per-AF list of sockets.
    pub unp_link: ListEntry<Unpcb>,
}

impl Unpcb {
    /// A zeroed control block of `so`, as `pool_get(PR_ZERO)` and `unp_socket = so` leave
    /// it.
    pub const fn new(so: &'static Socket) -> Self {
        Self {
            unp_refcnt: Refcnt::new(),
            unp_socket: so,
            unp_vnode: Cell::new(None),
            unp_file: Cell::new(None),
            unp_conn: Cell::new(None),
            unp_ino: Cell::new(0),
            unp_refs: SlistHead::new(),
            unp_nextref: SlistEntry::new(),
            unp_addr: Cell::new(None),
            unp_msgcount: Cell::new(0),
            unp_gcrefs: Cell::new(0),
            unp_flags: Cell::new(0),
            unp_gcflags: Cell::new(0),
            unp_connid: Cell::new(Sockpeercred {
                uid: 0,
                gid: 0,
                pid: 0,
            }),
            unp_ctime: Cell::new(Timespec::new(0, 0)),
            unp_link: ListEntry::new(),
        }
    }

    /// `unp->unp_flags & bits`.
    pub fn has_flags(&self, bits: i32) -> bool {
        self.unp_flags.get() & bits != 0
    }

    /// `unp->unp_flags |= bits`.
    pub fn set_flags(&self, bits: i32) {
        self.unp_flags.set(self.unp_flags.get() | bits);
    }

    /// `unp->unp_flags &= ~bits`.
    pub fn clear_flags(&self, bits: i32) {
        self.unp_flags.set(self.unp_flags.get() & !bits);
    }
}

queue_adapter!(
    /// `SLIST_HEAD(,unpcb) unp_refs`: the control blocks connected to this one, through
    /// `unp_nextref`.
    pub UnpRefs: Unpcb, unp_nextref => SlistEntry<Unpcb>
);

queue_adapter!(
    /// `LIST_HEAD(unp_head, unpcb)`: every UNIX domain control block, through `unp_link`.
    pub UnpHead: Unpcb, unp_link => ListEntry<Unpcb>
);

/// `sotounpcb(so)`: the UNIX domain control block of `so`, `None` once detached.
pub fn sotounpcb(so: &Socket) -> Option<&'static Unpcb> {
    // SAFETY: only `uipc_attach` sets the `so_pcb` of a UNIX domain socket, to an
    // `unpcb_pool` item, and `unp_detach` clears it before giving the item back; the caller
    // asks only about UNIX domain sockets, as the C's cast assumes.
    unsafe { so.so_pcb.get().cast::<Unpcb>().cast_const().as_ref() }
}

/// `struct fdpass`: a file in flight in an internalized `SCM_RIGHTS` message, with the
/// sender's `UF_PLEDGED` flag.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Fdpass {
    /// `fp`: the file; the message holds a reference to it.
    pub fp: *const File,
    /// `flags`.
    pub flags: i32,
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::reftest::{assert_complete, assert_defines};

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/sys/unpcb.h");
        let unp = assert_defines!(defs;
            UNP_FEIDS, UNP_FEIDSBIND, UNP_BINDING, UNP_CONNECTING, UNP_GCDEAD);
        assert_complete(&defs, "UNP_", &unp);
    }
}
/* </TESTS> */
