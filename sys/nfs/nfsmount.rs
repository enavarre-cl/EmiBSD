/*	$OpenBSD: nfsmount.h,v 1.28 2018/04/09 09:39:53 mpi Exp $	*/
/*	$NetBSD: nfsmount.h,v 1.10 1996/02/18 11:54:03 fvdl Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1989, 1993
 *	The Regents of the University of California.  All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * Rick Macklem at The University of Guelph.
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
 *	@(#)nfsmount.h	8.3 (Berkeley) 3/30/95
 */
/* </LICENSES> */

//! `<nfs/nfsmount.h>`: the NFS mount structure, one allocated on every NFS mount, holding
//! the NFS specific information for the mount, and `VFSTONFS`.
//!
//! Upstream: sys/nfs/nfsmount.h @ 3ce1f3f79392
//!
//! The structure is the `malloc(9)`ed object `nfs_mount` hangs from `mnt_data`; it lives
//! until `nfs_unmount` frees it, so it is handed around as `&'static NfsMount` with `Cell`
//! members, which the C changes through shared pointers (the RPC code at `splsoftnet`, the
//! vnode operations under their locks).
//!
//! The prototypes (`nfs_mount`, `nfs_fsinfo`, `nfs_init`) are the functions of
//! `nfs_vfsops.rs` and `nfs_subs.rs`.
//!
//! ## Deviations
//! - `nm_verf` is a `Cell` of the whole 8-byte array, read and written as a unit.
//! - `nm_so` is `Cell<Option<&'static Socket>>`, `nm_nam` an mbuf like every other.
//! - `VFSTONFS` checks that the mount is an NFS one and panics otherwise (the C casts).

use core::cell::Cell;

use crate::kern::subr_prf::panic;
use crate::nfs::nfs::{NFS_MAX_TIMER, RChain};
use crate::nfs::nfsnode::NfsNodetree;
use crate::nfs::nfsproto::NFSX_V3WRITEVERF;
use crate::sys::mbuf::Mbuf;
use crate::sys::mount::Mount;
use crate::sys::queue::TailqHead;
use crate::sys::socketvar::Socket;
use crate::sys::timeout::Timeout;
use crate::sys::tree::RbtHead;
use crate::sys::vnode::Vnode;

/// `struct nfsmount`: one allocated on every NFS mount.
pub struct NfsMount {
    /// `nm_ntree`: filehandle/node tree.
    pub nm_ntree: RbtHead<NfsNodetree>,
    /// `nm_reqsq`: request queue for this mount.
    pub nm_reqsq: TailqHead<RChain>,
    /// `nm_rtimeout`: timeout (scans/resends `nm_reqsq`).
    pub nm_rtimeout: Timeout,
    /// `nm_mountp`: VFS structure for this filesystem.
    pub nm_mountp: Cell<Option<&'static Mount>>,
    /// `nm_vnode`: vnode of root dir.
    pub nm_vnode: Cell<Option<&'static Vnode>>,
    /// `nm_flag`: flags for soft/hard... (`NFSMNT_*`).
    pub nm_flag: Cell<i32>,
    /// `nm_numgrps`: max. size of groupslist.
    pub nm_numgrps: Cell<i32>,
    /// `nm_so`: RPC socket.
    pub nm_so: Cell<Option<&'static Socket>>,
    /// `nm_sotype`: type of socket.
    pub nm_sotype: Cell<i32>,
    /// `nm_soproto`: and protocol.
    pub nm_soproto: Cell<i32>,
    /// `nm_soflags`: `pr_flags` for socket protocol.
    pub nm_soflags: Cell<i32>,
    /// `nm_nam`: addr of server.
    pub nm_nam: Cell<Option<&'static Mbuf>>,
    /// `nm_timeo`: init timer for `NFSMNT_DUMBTIMR`.
    pub nm_timeo: Cell<i32>,
    /// `nm_retry`: max retries.
    pub nm_retry: Cell<i32>,
    /// `nm_srtt`: RTT timers for RPCs.
    pub nm_srtt: [Cell<i32>; NFS_MAX_TIMER],
    /// `nm_sdrtt`.
    pub nm_sdrtt: [Cell<i32>; NFS_MAX_TIMER],
    /// `nm_sent`: request send count.
    pub nm_sent: Cell<i32>,
    /// `nm_cwnd`: request send window.
    pub nm_cwnd: Cell<i32>,
    /// `nm_timeouts`: request timeouts.
    pub nm_timeouts: Cell<i32>,
    /// `nm_rsize`: max size of read rpc.
    pub nm_rsize: Cell<i32>,
    /// `nm_wsize`: max size of write rpc.
    pub nm_wsize: Cell<i32>,
    /// `nm_readdirsize`: size of a readdir rpc.
    pub nm_readdirsize: Cell<i32>,
    /// `nm_readahead`: num. of blocks to readahead.
    pub nm_readahead: Cell<i32>,
    /// `nm_verf`: V3 write verifier.
    pub nm_verf: Cell<[u8; NFSX_V3WRITEVERF]>,
    /// `nm_acregmin`: attr cache file recently modified.
    pub nm_acregmin: Cell<u16>,
    /// `nm_acregmax`: ac file not recently modified.
    pub nm_acregmax: Cell<u16>,
    /// `nm_acdirmin`: ac for dir recently modified.
    pub nm_acdirmin: Cell<u16>,
    /// `nm_acdirmax`: ac for dir not recently modified.
    pub nm_acdirmax: Cell<u16>,
}

// SAFETY: the members are changed under the kernel lock (at `splsoftnet` for the RPC state),
// as in C.
unsafe impl Sync for NfsMount {}

impl NfsMount {
    /// A mount with every member cleared (`malloc(M_ZERO)`); `nfs_ninit` and
    /// `timeout_set` then initialise the tree and the timeout as in C.
    pub const fn new() -> Self {
        Self {
            nm_ntree: RbtHead::new(),
            nm_reqsq: TailqHead::new(),
            nm_rtimeout: Timeout::zeroed(),
            nm_mountp: Cell::new(None),
            nm_vnode: Cell::new(None),
            nm_flag: Cell::new(0),
            nm_numgrps: Cell::new(0),
            nm_so: Cell::new(None),
            nm_sotype: Cell::new(0),
            nm_soproto: Cell::new(0),
            nm_soflags: Cell::new(0),
            nm_nam: Cell::new(None),
            nm_timeo: Cell::new(0),
            nm_retry: Cell::new(0),
            nm_srtt: [const { Cell::new(0) }; NFS_MAX_TIMER],
            nm_sdrtt: [const { Cell::new(0) }; NFS_MAX_TIMER],
            nm_sent: Cell::new(0),
            nm_cwnd: Cell::new(0),
            nm_timeouts: Cell::new(0),
            nm_rsize: Cell::new(0),
            nm_wsize: Cell::new(0),
            nm_readdirsize: Cell::new(0),
            nm_readahead: Cell::new(0),
            nm_verf: Cell::new([0; NFSX_V3WRITEVERF]),
            nm_acregmin: Cell::new(0),
            nm_acregmax: Cell::new(0),
            nm_acdirmin: Cell::new(0),
            nm_acdirmax: Cell::new(0),
        }
    }
}

impl Default for NfsMount {
    fn default() -> Self {
        Self::new()
    }
}

/// `VFSTONFS(mp)`: convert mount ptr to nfsmount ptr.
#[allow(non_snake_case)] // the C name
pub fn VFSTONFS(mp: &Mount) -> &'static NfsMount {
    let data = mp.mnt_data.get();
    let nfs = mp.mnt_vfc.get().map(|vfc| vfc.name()) == Some(b"nfs".as_slice());
    if data.is_null() || !nfs {
        panic(format_args!(
            "VFSTONFS: mount {:p} is not a mounted NFS",
            mp
        ));
    }
    // SAFETY: an NFS mount's `mnt_data` (checked above) is the `struct nfsmount` that
    // `nfs_mount` allocated, which lives until `nfs_unmount` frees it and clears `mnt_data`;
    // the caller holds the mount busy or a vnode of it.
    unsafe { &*data.cast::<NfsMount>() }
}
