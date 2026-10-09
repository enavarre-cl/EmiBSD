/* <LICENSES> */
/*
 * Copyright (c) 2026 Emilio Navarrete Lineros <enavarre@outlook.com>
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

/*
 * Copyright (c) 1989, 1993, 1995
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
 *	@(#)nfs.h	8.4 (Berkeley) 5/1/95
 */
/* </LICENSES> */

/* <CODE> */
//! `<nfs/nfs.h>`: the NFS tunables, the `nfssvc(2)` arguments, the statistics, and the
//! in-kernel structures shared by the client and the server: the outstanding request
//! (`struct nfsreq`), the server socket (`struct nfssvc_sock`), the server thread
//! (`struct nfsd`) and the server's request descriptor (`struct nfsrv_descript`).
//!
//! Upstream: sys/nfs/nfs.h @ 3ce1f3f79392
//!
//! Requests, server sockets and server threads are pool or `malloc(9)` items that live until
//! the C frees them; they are handed around as `&'static T` with `Cell` members, the
//! `docs/C_TO_RUST.md` idiom for objects the C mutates through shared pointers, and they link
//! into their lists through `queue.rs` entries ([`RChain`], [`NsChain`], [`NfsdChain`]). The
//! server's request descriptor is owned by the `nfsd` that dequeued it and is passed as
//! `&mut NfsrvDescript`, because its dissection cursor (`nd_md`, `nd_dpos`) is what
//! `nfsd_dissect` reads through (`nfsm_subs.rs`).
//!
//! ## Deviations
//! - `NFS_HZ`, `NFS_TIMEO`, `NFS_MINTIMEO`, `NFS_MAXTIMEO`, `NFS_MINRTO`, `NFS_MAXRTO` and
//!   `NFS_INITRTT` depend on `hz` and `nfs_ticks`, so they are functions ([`nfs_hz`], ...).
//!   `NFS_CMPFH`, `NFS_ISV3`, `NFS_SRVMAXDATA`, `NFSIGNORE_SOERROR` are functions with
//!   lowercase names; `nfs_cmpfh` compares against the node's `n_fh` (`nfsnode.rs`).
//! - `struct nfsstats` is ABI (`fs.nfs.nfsstats`): `#[repr(C)]` with `AtomicU64` members
//!   (the C bumps them racily from everywhere, the `uvmexp` idiom); [`Nfsstats::snapshot`]
//!   reads them into the plain words `sysctl` copies out. The instance is `nfs_subs.rs`'s
//!   `NFSSTATS`, where the C defines it.
//! - `struct nfsd_args` and `struct nfsd_srvargs` (the `nfssvc(2)` ABI) name the holes the
//!   C compiler leaves on LP64 (`_pad*`); user addresses (`name`, `nsd_authstr`,
//!   `nsd_verfstr`) and the kernel's `nsd_nfsd` cookie are `usize`.
//! - `union nethostaddr` is [`Nethostaddr`] with both members side by side; the flag that
//!   says which one is valid stays the C's (`RC_INETADDR`/`RC_NAM`).
//! - `enum nfs_rto_timers` is a set of `usize` constants: the values index `nm_srtt` and
//!   `nm_sdrtt`.
//! - Procedure numbers (`r_procnum`, `nd_procnum`) are `usize` (see `nfsproto.rs`).
//! - The `struct proc *` of a request (`r_procp`) and of a server thread (`nfsd_procp`)
//!   are `Cell<*const Proc>`, the long-lived-pointer idiom; `nfsd_nd` is a `NonNull` to the
//!   descriptor the thread owns.
//! - The globals the header declares for other files are declared here, each with the file
//!   that defines it in C: `nfs_node_pool` (`nfs_node.c`), `nfsd_head`, `nfsd_head_flag`
//!   and `nfs_niothreads` (`nfs_syscalls.c`; lowercase, since `NFS_NIOTHREADS` is the sysctl
//!   identifier). `nfsreqpl` lives in `nfs_subs.rs`.
//! - `B_INVAFTERWRITE` is `B_INVAL`'s `i64` like every `B_*` flag.

use core::cell::Cell;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI32, AtomicU64, Ordering};

use crate::conf::param::HZ;
use crate::kern::subr_prf::panic;
use crate::nfs::nfs_subs::NFS_TICKS;
use crate::nfs::nfsmount::VFSTONFS;
use crate::nfs::nfsnode::NfsNode;
use crate::nfs::nfsproto::{NFS_MAXDATA, NFS_MAXDGRAMDATA, NFS_NPROCS, NFS_V2MAXDATA};
use crate::queue_adapter;
use crate::sys::buf::B_INVAL;
use crate::sys::errno::Errno;
use crate::sys::file::File;
use crate::sys::mbuf::Mbuf;
use crate::sys::mount::NFSMNT_NFSV3;
use crate::sys::pool::Pool;
use crate::sys::proc::Proc;
use crate::sys::protosw::PR_CONNREQUIRED;
use crate::sys::queue::{TailqEntry, TailqHead};
use crate::sys::signal::{SIGHUP, SIGINT, SIGKILL, SIGQUIT, SIGTERM, Sigset, sigmask};
use crate::sys::socketvar::Socket;
use crate::sys::sysctl::{CTLTYPE_INT, CTLTYPE_STRUCT, Ctlname};
use crate::sys::time::Timeval;
use crate::sys::types::Uid;
use crate::sys::ucred::{Ucred, Xucred};
use crate::sys::vnode::Vnode;

/// `NFS_TICKINTVL`: desired time for a tick (msec).
pub const NFS_TICKINTVL: i32 = 5;
/// `NFS_TIMEOUTMUL`: timeout/delay multiplier.
pub const NFS_TIMEOUTMUL: i32 = 2;
/// `NFS_MAXREXMIT`: stop counting after this many.
pub const NFS_MAXREXMIT: i32 = 100;
/// `NFS_RETRANS`: num of retrans for soft mounts.
pub const NFS_RETRANS: i32 = 10;
/// `NFS_MAXGRPS`: max. size of groups list.
pub const NFS_MAXGRPS: i32 = 16;
/// `NFS_MINATTRTIMO`: attribute cache timeout in sec.
pub const NFS_MINATTRTIMO: i32 = 5;
/// `NFS_MAXATTRTIMO`.
pub const NFS_MAXATTRTIMO: i32 = 60;
/// `NFS_WSIZE`: def. write data size <= 8192.
pub const NFS_WSIZE: i32 = 8192;
/// `NFS_RSIZE`: def. read data size <= 8192.
pub const NFS_RSIZE: i32 = 8192;
/// `NFS_READDIRSIZE`: def. readdir size.
pub const NFS_READDIRSIZE: i32 = 8192;
/// `NFS_DEFRAHEAD`: def. read ahead # blocks.
pub const NFS_DEFRAHEAD: i32 = 1;
/// `NFS_MAXRAHEAD`: max. read ahead # blocks.
pub const NFS_MAXRAHEAD: i32 = 4;
/// `NFS_MAXASYNCDAEMON`: max. number async_daemons runable.
pub const NFS_MAXASYNCDAEMON: i32 = 20;

/// `NFS_DIRBLKSIZ`: must be a multiple of `DIRBLKSIZ`. Ideally it should be bigger, but I've
/// seen servers with broken NFS/ethernet drivers that won't work with anything bigger
/// (Linux..).
pub const NFS_DIRBLKSIZ: i32 = 1024;
/// `NFS_READDIRBLKSIZ`: size of read dir blocks. XXX
pub const NFS_READDIRBLKSIZ: i32 = 512;

/// `B_INVAFTERWRITE`: whatever is required by the buffer cache code to say "invalidate the
/// block after it is written back".
pub const B_INVAFTERWRITE: i64 = B_INVAL;

/// `NFSSVC_NFSD`: flags for the `nfssvc()` system call.
pub const NFSSVC_NFSD: i32 = 0x004;
/// `NFSSVC_ADDSOCK`.
pub const NFSSVC_ADDSOCK: i32 = 0x008;

/// `NFS_NFSSTATS`: `fs.nfs` sysctl(3) identifiers; struct: `struct nfsstats`.
pub const NFS_NFSSTATS: i32 = 1;
/// `NFS_NIOTHREADS`: number of i/o threads.
pub const NFS_NIOTHREADS: i32 = 2;
/// `NFS_MAXID`.
pub const NFS_MAXID: i32 = 3;

/// `FS_NFS_NAMES`.
pub const FS_NFS_NAMES: [Ctlname; NFS_MAXID as usize] = [
    Ctlname::NONE,
    Ctlname::new(b"nfsstats", CTLTYPE_STRUCT),
    Ctlname::new(b"iothreads", CTLTYPE_INT),
];

/// `NFSINT_SIGMASK`: the set of signals that interrupt an I/O in progress for `NFSMNT_INT`
/// mounts. What should be in this set is open to debate, but I believe that since I/O system
/// calls on ufs are never interrupted by signals the set should be minimal. My reasoning is
/// that many current programs that use signals such as SIGALRM will not expect file I/O
/// system calls to be interrupted by them and break.
pub const NFSINT_SIGMASK: Sigset =
    sigmask(SIGINT) | sigmask(SIGTERM) | sigmask(SIGKILL) | sigmask(SIGHUP) | sigmask(SIGQUIT);

/// `R_TIMING`: timing request (in mntp); flag values for `r_flags`.
pub const R_TIMING: i32 = 0x01;
/// `R_SENT`: request has been sent.
pub const R_SENT: i32 = 0x02;
/// `R_SOFTTERM`: soft mnt, too many retries.
pub const R_SOFTTERM: i32 = 0x04;
/// `R_INTR`: intr mnt, signal pending.
pub const R_INTR: i32 = 0x08;
/// `R_SOCKERR`: fatal error on socket.
pub const R_SOCKERR: i32 = 0x10;
/// `R_TPRINTFMSG`: did a tprintf msg.
pub const R_TPRINTFMSG: i32 = 0x20;
/// `R_MUSTRESEND`: must resend request.
pub const R_MUSTRESEND: i32 = 0x40;

/// `NFS_DEFAULT_TIMER` (`enum nfs_rto_timers`).
pub const NFS_DEFAULT_TIMER: usize = 0;
/// `NFS_GETATTR_TIMER`.
pub const NFS_GETATTR_TIMER: usize = 1;
/// `NFS_LOOKUP_TIMER`.
pub const NFS_LOOKUP_TIMER: usize = 2;
/// `NFS_READ_TIMER`.
pub const NFS_READ_TIMER: usize = 3;
/// `NFS_WRITE_TIMER`.
pub const NFS_WRITE_TIMER: usize = 4;
/// `NFS_MAX_TIMER`.
pub const NFS_MAX_TIMER: usize = NFS_WRITE_TIMER;

/// `SLP_VALID`: connection is usable; bits for `ns_flag`.
pub const SLP_VALID: i32 = 0x01;
/// `SLP_DOREC`: receive operation required.
pub const SLP_DOREC: i32 = 0x02;
/// `SLP_NEEDQ`: connection has data to queue from socket.
pub const SLP_NEEDQ: i32 = 0x04;
/// `SLP_DISCONN`: connection is closed.
pub const SLP_DISCONN: i32 = 0x08;
/// `SLP_GETSTREAM`: extracting RPC from TCP connection.
pub const SLP_GETSTREAM: i32 = 0x10;
/// `SLP_LASTFRAG`: last fragment received on TCP connection.
pub const SLP_LASTFRAG: i32 = 0x20;
/// `SLP_ALLFLAGS`: convenience.
pub const SLP_ALLFLAGS: i32 = 0xff;

/// `NFSD_WAITING`: bits for `nfsd_flag`.
pub const NFSD_WAITING: i32 = 0x01;
/// `NFSD_REQINPROG`.
pub const NFSD_REQINPROG: i32 = 0x02;

/// `ND_NFSV3`: bits for `nd_flag`.
pub const ND_NFSV3: i32 = 0x08;

/// `NFSD_CHECKSLP`: a bit of `nfsd_head_flag`.
pub const NFSD_CHECKSLP: i32 = 0x01;

/// `struct nfsd_args`: the `NFSSVC_ADDSOCK` argument of `nfssvc(2)`. Not that anyone besides
/// nfsd(8) should ever use it.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NfsdArgs {
    /// `sock`: socket to serve.
    pub sock: i32,
    /// The hole before `name`.
    pub _pad0: i32,
    /// `name`: client addr for connection based sockets (a user address).
    pub name: usize,
    /// `namelen`: length of name.
    pub namelen: i32,
    /// The hole at the end.
    pub _pad1: i32,
}

// SAFETY: `#[repr(C)]` integers with the holes named, no implicit padding.
unsafe impl crate::machine::copy::AbiPod for NfsdArgs {}

/// `struct nfsd_srvargs`: the `NFSSVC_NFSD` argument of `nfssvc(2)`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct NfsdSrvargs {
    /// `nsd_nfsd`: pointer to in kernel nfsd struct (opaque to userland).
    pub nsd_nfsd: usize,
    /// `nsd_uid`: effective uid mapped to cred.
    pub nsd_uid: Uid,
    /// `nsd_haddr`: IP address of client.
    pub nsd_haddr: u32,
    /// `nsd_cr`: cred. uid maps to.
    pub nsd_cr: Xucred,
    /// `nsd_authlen`: length of auth string (ret).
    pub nsd_authlen: i32,
    /// `nsd_authstr`: auth string (ret), a user address.
    pub nsd_authstr: usize,
    /// `nsd_verflen`: and the verifier.
    pub nsd_verflen: i32,
    /// The hole before `nsd_verfstr`.
    pub _pad0: i32,
    /// `nsd_verfstr`, a user address.
    pub nsd_verfstr: usize,
    /// `nsd_timestamp`: timestamp from verifier.
    pub nsd_timestamp: Timeval,
    /// `nsd_ttl`: credential ttl (sec).
    pub nsd_ttl: u32,
    /// The hole at the end.
    pub _pad1: u32,
}

impl NfsdSrvargs {
    /// `sizeof(struct nfsd_srvargs)` on LP64.
    pub const SIZE: usize = size_of::<NfsdSrvargs>();
}

/// `struct nfsstats`: stats structure, the `fs.nfs.nfsstats` sysctl(3) ABI.
#[repr(C)]
pub struct Nfsstats {
    /// `attrcache_hits`.
    pub attrcache_hits: AtomicU64,
    /// `attrcache_misses`.
    pub attrcache_misses: AtomicU64,
    /// `lookupcache_hits`.
    pub lookupcache_hits: AtomicU64,
    /// `lookupcache_misses`.
    pub lookupcache_misses: AtomicU64,
    /// `direofcache_hits`.
    pub direofcache_hits: AtomicU64,
    /// `direofcache_misses`.
    pub direofcache_misses: AtomicU64,
    /// `biocache_reads`.
    pub biocache_reads: AtomicU64,
    /// `read_bios`.
    pub read_bios: AtomicU64,
    /// `read_physios`.
    pub read_physios: AtomicU64,
    /// `biocache_writes`.
    pub biocache_writes: AtomicU64,
    /// `write_bios`.
    pub write_bios: AtomicU64,
    /// `write_physios`.
    pub write_physios: AtomicU64,
    /// `biocache_readlinks`.
    pub biocache_readlinks: AtomicU64,
    /// `readlink_bios`.
    pub readlink_bios: AtomicU64,
    /// `biocache_readdirs`.
    pub biocache_readdirs: AtomicU64,
    /// `readdir_bios`.
    pub readdir_bios: AtomicU64,
    /// `rpccnt`: client RPCs by procedure.
    pub rpccnt: [AtomicU64; NFS_NPROCS],
    /// `rpcretries`.
    pub rpcretries: AtomicU64,
    /// `srvrpccnt`: server RPCs by procedure.
    pub srvrpccnt: [AtomicU64; NFS_NPROCS],
    /// `srvrpc_errs`.
    pub srvrpc_errs: AtomicU64,
    /// `srv_errs`.
    pub srv_errs: AtomicU64,
    /// `rpcrequests`.
    pub rpcrequests: AtomicU64,
    /// `rpctimeouts`.
    pub rpctimeouts: AtomicU64,
    /// `rpcunexpected`.
    pub rpcunexpected: AtomicU64,
    /// `rpcinvalid`.
    pub rpcinvalid: AtomicU64,
    /// `srvcache_inproghits`.
    pub srvcache_inproghits: AtomicU64,
    /// `srvcache_idemdonehits`.
    pub srvcache_idemdonehits: AtomicU64,
    /// `srvcache_nonidemdonehits`.
    pub srvcache_nonidemdonehits: AtomicU64,
    /// `srvcache_misses`.
    pub srvcache_misses: AtomicU64,
    /// `forcedsync`.
    pub forcedsync: AtomicU64,
    /// `srvnqnfs_leases`.
    pub srvnqnfs_leases: AtomicU64,
    /// `srvnqnfs_maxleases`.
    pub srvnqnfs_maxleases: AtomicU64,
    /// `srvnqnfs_getleases`.
    pub srvnqnfs_getleases: AtomicU64,
    /// `srvvop_writes`.
    pub srvvop_writes: AtomicU64,
}

impl Nfsstats {
    /// The number of `uint64_t` words in the structure.
    pub const NWORDS: usize = size_of::<Nfsstats>() / size_of::<u64>();

    /// All counters zero.
    pub const fn new() -> Self {
        Self {
            attrcache_hits: AtomicU64::new(0),
            attrcache_misses: AtomicU64::new(0),
            lookupcache_hits: AtomicU64::new(0),
            lookupcache_misses: AtomicU64::new(0),
            direofcache_hits: AtomicU64::new(0),
            direofcache_misses: AtomicU64::new(0),
            biocache_reads: AtomicU64::new(0),
            read_bios: AtomicU64::new(0),
            read_physios: AtomicU64::new(0),
            biocache_writes: AtomicU64::new(0),
            write_bios: AtomicU64::new(0),
            write_physios: AtomicU64::new(0),
            biocache_readlinks: AtomicU64::new(0),
            readlink_bios: AtomicU64::new(0),
            biocache_readdirs: AtomicU64::new(0),
            readdir_bios: AtomicU64::new(0),
            rpccnt: [const { AtomicU64::new(0) }; NFS_NPROCS],
            rpcretries: AtomicU64::new(0),
            srvrpccnt: [const { AtomicU64::new(0) }; NFS_NPROCS],
            srvrpc_errs: AtomicU64::new(0),
            srv_errs: AtomicU64::new(0),
            rpcrequests: AtomicU64::new(0),
            rpctimeouts: AtomicU64::new(0),
            rpcunexpected: AtomicU64::new(0),
            rpcinvalid: AtomicU64::new(0),
            srvcache_inproghits: AtomicU64::new(0),
            srvcache_idemdonehits: AtomicU64::new(0),
            srvcache_nonidemdonehits: AtomicU64::new(0),
            srvcache_misses: AtomicU64::new(0),
            forcedsync: AtomicU64::new(0),
            srvnqnfs_leases: AtomicU64::new(0),
            srvnqnfs_maxleases: AtomicU64::new(0),
            srvnqnfs_getleases: AtomicU64::new(0),
            srvvop_writes: AtomicU64::new(0),
        }
    }

    /// The counters in the order of the C structure: what `sysctl` copies out as
    /// `struct nfsstats`.
    pub fn snapshot(&self) -> [u64; Self::NWORDS] {
        // SAFETY: `Nfsstats` is `#[repr(C)]` and made only of `AtomicU64`s (alone or in
        // arrays), which have the size and alignment of `u64` and no padding between them:
        // the structure is `NWORDS` atomics in a row, borrowed for the call.
        let words = unsafe {
            core::slice::from_raw_parts(ptr::from_ref(self).cast::<AtomicU64>(), Self::NWORDS)
        };
        core::array::from_fn(|i| words[i].load(Ordering::Relaxed))
    }

    /// Overwrites the counters with `values`, in the order of the C structure: what the
    /// `fs.nfs.nfsstats` sysctl does when it is written (`nfs_sysctl`'s `copyin`).
    pub fn restore(&self, values: &[u64; Self::NWORDS]) {
        // SAFETY: as in `snapshot`: the structure is `NWORDS` atomics in a row.
        let words = unsafe {
            core::slice::from_raw_parts(ptr::from_ref(self).cast::<AtomicU64>(), Self::NWORDS)
        };
        for (word, value) in words.iter().zip(values) {
            word.store(*value, Ordering::Relaxed);
        }
    }
}

impl Default for Nfsstats {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct nfsreq`: NFS outstanding request list element, a `nfsreqpl` item.
pub struct NfsReq {
    /// `r_chain`: the link in the mount's `nm_reqsq`.
    pub r_chain: TailqEntry<NfsReq>,
    /// `r_mreq`: the request.
    pub r_mreq: Cell<Option<&'static Mbuf>>,
    /// `r_mrep`: the reply.
    pub r_mrep: Cell<Option<&'static Mbuf>>,
    /// `r_md`: the reply's dissection mbuf.
    pub r_md: Cell<Option<&'static Mbuf>>,
    /// `r_dpos`: the reply's dissection position, inside `r_md`'s data.
    pub r_dpos: Cell<*mut u8>,
    /// `r_nmp`: the mount.
    pub r_nmp: Cell<Option<&'static crate::nfs::nfsmount::NfsMount>>,
    /// `r_vp`: the vnode.
    pub r_vp: Cell<Option<&'static Vnode>>,
    /// `r_xid`: the transaction id (a raw XDR word).
    pub r_xid: Cell<u32>,
    /// `r_flags`: flags on request, see `R_*`.
    pub r_flags: Cell<i32>,
    /// `r_rexmit`: current retrans count.
    pub r_rexmit: Cell<i32>,
    /// `r_timer`: tick counter on reply.
    pub r_timer: Cell<i32>,
    /// `r_procnum`: NFS procedure number.
    pub r_procnum: Cell<usize>,
    /// `r_rtt`: RTT for rpc.
    pub r_rtt: Cell<i32>,
    /// `r_procp`: proc that did I/O system call (NULL for none).
    pub r_procp: Cell<*const Proc>,
}

// SAFETY: requests are changed under the kernel lock at `splsoftnet` (`nfs_timer` and the
// requesting thread), as in C.
unsafe impl Sync for NfsReq {}

impl NfsReq {
    /// A request with every member cleared (`pool_get(&nfsreqpl, PR_ZERO)`).
    pub const fn new() -> Self {
        Self {
            r_chain: TailqEntry::new(),
            r_mreq: Cell::new(None),
            r_mrep: Cell::new(None),
            r_md: Cell::new(None),
            r_dpos: Cell::new(ptr::null_mut()),
            r_nmp: Cell::new(None),
            r_vp: Cell::new(None),
            r_xid: Cell::new(0),
            r_flags: Cell::new(0),
            r_rexmit: Cell::new(0),
            r_timer: Cell::new(0),
            r_procnum: Cell::new(0),
            r_rtt: Cell::new(0),
            r_procp: Cell::new(ptr::null()),
        }
    }
}

impl Default for NfsReq {
    fn default() -> Self {
        Self::new()
    }
}

queue_adapter!(
    /// `TAILQ_HEAD(reqs, nfsreq)`: a mount's outstanding requests, through `r_chain`.
    pub RChain: NfsReq, r_chain => TailqEntry<NfsReq>
);

/// `union nethostaddr`: network address hash list element. Which member is valid is the
/// holder's flag (`RC_INETADDR` or `RC_NAM` in `nfsrvcache.rs`).
#[derive(Clone, Copy, Default)]
pub struct Nethostaddr {
    /// `had_inetaddr`.
    pub had_inetaddr: u32,
    /// `had_nam`.
    pub had_nam: Option<&'static Mbuf>,
}

/// `struct nfssvc_sock`: a socket the server receives requests on.
pub struct NfssvcSock {
    /// `ns_chain`: list of all `nfssvc_sock`s.
    pub ns_chain: TailqEntry<NfssvcSock>,
    /// `ns_fp`: fp from the...
    pub ns_fp: Cell<Option<&'static File>>,
    /// `ns_so`: ...socket this struct wraps.
    pub ns_so: Cell<Option<&'static Socket>>,
    /// `ns_nam`: `MT_SONAME` of client.
    pub ns_nam: Cell<Option<&'static Mbuf>>,
    /// `ns_raw`: head of unpeeked mbufs.
    pub ns_raw: Cell<Option<&'static Mbuf>>,
    /// `ns_rawend`: tail of unpeeked mbufs.
    pub ns_rawend: Cell<Option<&'static Mbuf>>,
    /// `ns_rec`: queued RPC records.
    pub ns_rec: Cell<Option<&'static Mbuf>>,
    /// `ns_recend`: last queued RPC record.
    pub ns_recend: Cell<Option<&'static Mbuf>>,
    /// `ns_frag`: end of record fragment.
    pub ns_frag: Cell<Option<&'static Mbuf>>,
    /// `ns_flag`: socket status flags (`SLP_*`).
    pub ns_flag: Cell<i32>,
    /// `ns_solock`: lock for connected socket.
    pub ns_solock: Cell<i32>,
    /// `ns_cc`: actual chars queued.
    pub ns_cc: Cell<i32>,
    /// `ns_reclen`: length of first queued record.
    pub ns_reclen: Cell<i32>,
    /// `ns_sref`: # of refs to this struct.
    pub ns_sref: Cell<u32>,
}

// SAFETY: server sockets are changed under the kernel lock, as in C.
unsafe impl Sync for NfssvcSock {}

impl NfssvcSock {
    /// A socket with every member cleared (`malloc(M_ZERO)`).
    pub const fn new() -> Self {
        Self {
            ns_chain: TailqEntry::new(),
            ns_fp: Cell::new(None),
            ns_so: Cell::new(None),
            ns_nam: Cell::new(None),
            ns_raw: Cell::new(None),
            ns_rawend: Cell::new(None),
            ns_rec: Cell::new(None),
            ns_recend: Cell::new(None),
            ns_frag: Cell::new(None),
            ns_flag: Cell::new(0),
            ns_solock: Cell::new(0),
            ns_cc: Cell::new(0),
            ns_reclen: Cell::new(0),
            ns_sref: Cell::new(0),
        }
    }
}

impl Default for NfssvcSock {
    fn default() -> Self {
        Self::new()
    }
}

queue_adapter!(
    /// `TAILQ_HEAD(, nfssvc_sock)`: the server's sockets, through `ns_chain`.
    pub NsChain: NfssvcSock, ns_chain => TailqEntry<NfssvcSock>
);

/// `struct nfsd`: one of these structures is allocated for each nfsd.
pub struct Nfsd {
    /// `nfsd_chain`: list of all nfsd's.
    pub nfsd_chain: TailqEntry<Nfsd>,
    /// `nfsd_flag`: `NFSD_` flags.
    pub nfsd_flag: Cell<i32>,
    /// `nfsd_slp`: current socket.
    pub nfsd_slp: Cell<Option<&'static NfssvcSock>>,
    /// `nfsd_procp`: proc ptr.
    pub nfsd_procp: Cell<*const Proc>,
    /// `nfsd_nd`: associated `nfsrv_descript`, owned by this nfsd while it serves it.
    pub nfsd_nd: Cell<Option<NonNull<NfsrvDescript>>>,
}

// SAFETY: server threads are changed under the kernel lock, as in C.
unsafe impl Sync for Nfsd {}

impl Nfsd {
    /// An nfsd with every member cleared (`malloc(M_ZERO)`).
    pub const fn new() -> Self {
        Self {
            nfsd_chain: TailqEntry::new(),
            nfsd_flag: Cell::new(0),
            nfsd_slp: Cell::new(None),
            nfsd_procp: Cell::new(ptr::null()),
            nfsd_nd: Cell::new(None),
        }
    }
}

impl Default for Nfsd {
    fn default() -> Self {
        Self::new()
    }
}

queue_adapter!(
    /// `TAILQ_HEAD(nfsdhead, nfsd)`: every nfsd, through `nfsd_chain`.
    pub NfsdChain: Nfsd, nfsd_chain => TailqEntry<Nfsd>
);

/// `struct nfsdhead`: the list of nfsds.
pub struct Nfsdhead(pub TailqHead<NfsdChain>);

// SAFETY: the list is changed under the kernel lock, as in C.
unsafe impl Sync for Nfsdhead {}

/// `struct nfsrv_descript`: used by the server for describing each request. The nfsd that
/// dequeued it owns it (`&mut`); `nd_md`/`nd_dpos` are the dissection cursor of
/// `nfsd_dissect` and friends (`nfsm_subs.rs`).
pub struct NfsrvDescript {
    /// `nd_mrep`: request mbuf list.
    pub nd_mrep: Option<&'static Mbuf>,
    /// `nd_md`: current dissect mbuf.
    pub nd_md: Option<&'static Mbuf>,
    /// `nd_nam`: and socket addr.
    pub nd_nam: Option<&'static Mbuf>,
    /// `nd_nam2`: return socket addr.
    pub nd_nam2: Option<&'static Mbuf>,
    /// `nd_dpos`: current dissect pos, inside `nd_md`'s data.
    pub nd_dpos: *mut u8,
    /// `nd_procnum`: RPC #.
    pub nd_procnum: usize,
    /// `nd_flag`: `ND_*`.
    pub nd_flag: i32,
    /// `nd_repstat`: reply status (an NFS status number).
    pub nd_repstat: i32,
    /// `nd_retxid`: reply xid, in host order (`nfs_getreq` converts it, `nfs_rephead` converts back).
    pub nd_retxid: u32,
    /// `nd_cr`: credentials.
    pub nd_cr: Ucred,
}

impl NfsrvDescript {
    /// A descriptor with every member cleared.
    pub const fn new() -> Self {
        Self {
            nd_mrep: None,
            nd_md: None,
            nd_nam: None,
            nd_nam2: None,
            nd_dpos: ptr::null_mut(),
            nd_procnum: 0,
            nd_flag: 0,
            nd_repstat: 0,
            nd_retxid: 0,
            nd_cr: Ucred::new(),
        }
    }
}

impl Default for NfsrvDescript {
    fn default() -> Self {
        Self::new()
    }
}

/// `nfs_node_pool`: the `struct nfsnode` pool (defined in `nfs_node.c`).
pub static NFS_NODE_POOL: Pool = Pool::new();

/// `nfsd_head`: every nfsd (defined in `nfs_syscalls.c`).
pub static NFSD_HEAD: Nfsdhead = Nfsdhead(TailqHead::new());

/// `nfsd_head_flag`: `NFSD_CHECKSLP` (defined in `nfs_syscalls.c`).
pub static NFSD_HEAD_FLAG: AtomicI32 = AtomicI32::new(0);

/// `nfs_niothreads`: the number of NFS I/O threads, `-1` before they are started (defined
/// in `nfs_syscalls.c`).
#[allow(non_upper_case_globals)] // the C name: `NFS_NIOTHREADS` is the sysctl identifier
pub static nfs_niothreads: AtomicI32 = AtomicI32::new(-1);

/// `NFS_HZ`: ticks per second of the NFS timer.
pub fn nfs_hz() -> i32 {
    HZ.load(Ordering::Relaxed) / NFS_TICKS.load(Ordering::Relaxed)
}

/// `NFS_TIMEO`: default timeout = 1 second.
pub fn nfs_timeo() -> i32 {
    nfs_hz()
}

/// `NFS_MINTIMEO`: min timeout to use.
pub fn nfs_mintimeo() -> i32 {
    nfs_hz()
}

/// `NFS_MAXTIMEO`: max timeout to backoff to.
pub fn nfs_maxtimeo() -> i32 {
    60 * nfs_hz()
}

/// `NFS_MINRTO`: on fast networks, the estimator will try to reduce the timeout lower than
/// the latency of the server's disks, which results in too many timeouts, so cap the lower
/// bound.
pub fn nfs_minrto() -> i32 {
    nfs_hz() >> 2
}

/// `NFS_MAXRTO`: keep the RTO from increasing to unreasonably large values when a server is
/// not responding.
pub fn nfs_maxrto() -> i32 {
    20 * nfs_hz()
}

/// `NFS_INITRTT`.
pub fn nfs_initrtt() -> i32 {
    nfs_hz() << 3
}

/// `NFS_CMPFH(n, f, s)`: whether node `n` has the file handle `f` (of `f.len()` bytes).
pub fn nfs_cmpfh(n: &NfsNode, f: &[u8]) -> bool {
    n.n_fhsize.get() as usize == f.len() && n.n_fh.get().fh_bytes[..f.len()] == *f
}

/// `NFS_ISV3(v)`: whether the vnode's mount speaks NFS version 3.
pub fn nfs_isv3(vp: &Vnode) -> bool {
    let Some(mp) = vp.v_mount.get() else {
        panic(format_args!("NFS_ISV3: vnode {:p} has no mount", vp));
    };
    VFSTONFS(mp).nm_flag.get() & NFSMNT_NFSV3 != 0
}

/// `NFS_SRVMAXDATA(n)`: the largest data a server reply may carry.
pub fn nfs_srvmaxdata(n: &NfsrvDescript) -> usize {
    if n.nd_flag & ND_NFSV3 != 0 {
        if n.nd_nam2.is_some() {
            NFS_MAXDGRAMDATA
        } else {
            NFS_MAXDATA
        }
    } else {
        NFS_V2MAXDATA
    }
}

/// `NFSIGNORE_SOERROR(s, e)`: socket errors ignored for connectionless sockets?? For now,
/// ignore them all. `s` is the protocol's `pr_flags`.
pub fn nfsignore_soerror(s: i32, e: Errno) -> bool {
    e != Errno::EINTR
        && e != Errno::ERESTART
        && e != Errno::EWOULDBLOCK
        && s & i32::from(PR_CONNREQUIRED) == 0
}

const _: () = {
    assert!(size_of::<NfsdArgs>() == 24);
    assert!(NfsdSrvargs::SIZE == 144);
    assert!(core::mem::offset_of!(NfsdSrvargs, nsd_authstr) == 96);
    assert!(core::mem::offset_of!(NfsdSrvargs, nsd_timestamp) == 120);
    assert!(Nfsstats::NWORDS == 78);
    assert!(size_of::<Nfsstats>() == 78 * size_of::<u64>());
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for `<nfs/nfs.h>`: the statistics snapshot, the helpers and, against the C
    // header, the constants.

    use std::string::String;

    use super::*;

    #[test]
    fn snapshot_is_in_c_order() {
        let st = Nfsstats::new();
        st.attrcache_hits.store(1, Ordering::Relaxed);
        st.rpccnt[0].store(2, Ordering::Relaxed);
        st.rpccnt[NFS_NPROCS - 1].store(3, Ordering::Relaxed);
        st.rpcretries.store(4, Ordering::Relaxed);
        st.srvvop_writes.store(5, Ordering::Relaxed);
        let w = st.snapshot();
        assert_eq!(w[0], 1);
        assert_eq!(w[16], 2);
        assert_eq!(w[16 + NFS_NPROCS - 1], 3);
        assert_eq!(w[16 + NFS_NPROCS], 4);
        assert_eq!(w[Nfsstats::NWORDS - 1], 5);
    }

    #[test]
    fn server_reply_limits_and_ignored_errors() {
        let mut nd = NfsrvDescript::new();
        assert_eq!(nfs_srvmaxdata(&nd), NFS_V2MAXDATA);
        nd.nd_flag = ND_NFSV3;
        assert_eq!(nfs_srvmaxdata(&nd), NFS_MAXDATA);
        assert!(nfsignore_soerror(0, Errno::ECONNREFUSED));
        assert!(!nfsignore_soerror(0, Errno::EINTR));
        assert!(!nfsignore_soerror(
            i32::from(PR_CONNREQUIRED),
            Errno::ECONNREFUSED
        ));
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn constants_match_the_c_header() {
        let defs = crate::reftest::defines("sys/nfs/nfs.h");
        let ours: &[(&str, i64)] = &[
            ("NFS_TICKINTVL", NFS_TICKINTVL.into()),
            ("NFS_TIMEOUTMUL", NFS_TIMEOUTMUL.into()),
            ("NFS_MAXREXMIT", NFS_MAXREXMIT.into()),
            ("NFS_RETRANS", NFS_RETRANS.into()),
            ("NFS_MAXGRPS", NFS_MAXGRPS.into()),
            ("NFS_MINATTRTIMO", NFS_MINATTRTIMO.into()),
            ("NFS_MAXATTRTIMO", NFS_MAXATTRTIMO.into()),
            ("NFS_WSIZE", NFS_WSIZE.into()),
            ("NFS_RSIZE", NFS_RSIZE.into()),
            ("NFS_READDIRSIZE", NFS_READDIRSIZE.into()),
            ("NFS_DEFRAHEAD", NFS_DEFRAHEAD.into()),
            ("NFS_MAXRAHEAD", NFS_MAXRAHEAD.into()),
            ("NFS_MAXASYNCDAEMON", NFS_MAXASYNCDAEMON.into()),
            ("NFS_DIRBLKSIZ", NFS_DIRBLKSIZ.into()),
            ("NFS_READDIRBLKSIZ", NFS_READDIRBLKSIZ.into()),
            ("NFSSVC_NFSD", NFSSVC_NFSD.into()),
            ("NFSSVC_ADDSOCK", NFSSVC_ADDSOCK.into()),
            ("NFS_NFSSTATS", NFS_NFSSTATS.into()),
            ("NFS_NIOTHREADS", NFS_NIOTHREADS.into()),
            ("NFS_MAXID", NFS_MAXID.into()),
            ("R_TIMING", R_TIMING.into()),
            ("R_SENT", R_SENT.into()),
            ("R_SOFTTERM", R_SOFTTERM.into()),
            ("R_INTR", R_INTR.into()),
            ("R_SOCKERR", R_SOCKERR.into()),
            ("R_TPRINTFMSG", R_TPRINTFMSG.into()),
            ("R_MUSTRESEND", R_MUSTRESEND.into()),
            ("SLP_VALID", SLP_VALID.into()),
            ("SLP_DOREC", SLP_DOREC.into()),
            ("SLP_NEEDQ", SLP_NEEDQ.into()),
            ("SLP_DISCONN", SLP_DISCONN.into()),
            ("SLP_GETSTREAM", SLP_GETSTREAM.into()),
            ("SLP_LASTFRAG", SLP_LASTFRAG.into()),
            ("SLP_ALLFLAGS", SLP_ALLFLAGS.into()),
            ("NFSD_WAITING", NFSD_WAITING.into()),
            ("NFSD_REQINPROG", NFSD_REQINPROG.into()),
            ("ND_NFSV3", ND_NFSV3.into()),
            ("NFSD_CHECKSLP", NFSD_CHECKSLP.into()),
        ];
        for (name, value) in ours {
            assert_eq!(crate::reftest::int(&defs, name), Some(*value), "{name}");
        }
        assert_eq!(
            defs.get("NFS_MAX_TIMER").map(String::as_str),
            Some("(NFS_WRITE_TIMER)")
        );
        assert_eq!(
            defs.get("B_INVAFTERWRITE").map(String::as_str),
            Some("B_INVAL")
        );
    }
}
/* </TESTS> */
