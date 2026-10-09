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
 *	@(#)nfs_syscalls.c	8.5 (Berkeley) 3/30/95
 */
/* </LICENSES> */

/* <CODE> */
//! The NFS system calls: `nfssvc(2)` (`sys_nfssvc`), which adds a socket to the server's list
//! (`NFSSVC_ADDSOCK`, `nfssvc_addsock`) or turns the calling process into an `nfsd`
//! (`NFSSVC_NFSD`, `nfssvc_nfsd`), the server's initialisation and socket bookkeeping
//! (`nfsrv_init`, `nfsrv_zapsock`, `nfsrv_slpderef`, `nfsrv_getslp`), and the client's
//! asynchronous I/O threads (`nfssvc_iod`, `nfs_getset_niothreads`).
//!
//! Upstream: sys/nfs/nfs_syscalls.c @ 3ce1f3f79392
//!
//! `nfssvc_nfsd` loops getting RPC requests from the sockets `nfssvc_addsock` registered,
//! runs each through the procedure table `nfsrv3_procs[]`, saves the reply in the request
//! cache and sends it, until it is killed by a signal. The `nfsiod`s drain `nfs_bufq`, the
//! queue `nfs_asyncio` fills with the buffers of read-ahead and write-behind I/O.
//!
//! ## Deviations
//! - `sys_nfssvc` returns `ENOSYS` after the privilege check without `nfsserver`, as the C
//!   without `NFSSERVER`; the `NFSSVC_NFSD` argument is copied in only to check that it is
//!   readable (the C never looks at it again).
//! - `nfsrv_zapsock` also clears the sockets' mbuf pointers (`ns_nam`, `ns_raw`, `ns_rec`
//!   and their tails) and counters after freeing them, where the C leaves them dangling;
//!   `nfsrv_slpderef` never frees the datagram socket `nfs_udpsock`, which `nfssvc_addsock`
//!   reuses (the C frees it and reads it back, until `nfsrv_init(1)` makes a new one).
//! - An `m_prepend` that cannot get an mbuf makes the reply fail with `ENOBUFS` (the request
//!   is dropped), where the C dereferences NULL.
//! - `nfssvc_checknam` answers a `bool`; `nfssvc_nfsd`'s `nfsrv_dorec` returns the descriptor
//!   (`NonNull`) instead of filling `*ndp`; the thread's `struct proc *` entries of
//!   `nfs_asyncdaemon[]` are `AtomicPtr`s.
//! - `nfssvc_iod`'s `kthread_exit(error)` takes the errno number.

use core::ffi::c_void;
#[cfg(feature = "nfsclient")]
use core::ptr;
#[cfg(feature = "nfsserver")]
use core::ptr::NonNull;
#[cfg(feature = "nfsserver")]
use core::sync::atomic::AtomicI32;
#[cfg(feature = "nfsclient")]
use core::sync::atomic::AtomicPtr;
use core::sync::atomic::Ordering::Relaxed;

use crate::kern::kern_prot::suser;
use crate::kern::kern_synch::{tsleep_nsec, wakeup};
use crate::kern::subr_prf::panic;
use crate::sys::errno::Errno;
use crate::sys::proc::Proc;
use crate::sys::systm::{INFSLP, SysArgs};
use crate::sys::types::Register;

#[cfg(feature = "nfsserver")]
use crate::kern::kern_descrip::closef;
#[cfg(feature = "nfsserver")]
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
#[cfg(feature = "nfsserver")]
use crate::kern::kern_malloc::{free, malloc};
#[cfg(feature = "nfsserver")]
use crate::kern::subr_pool::{pool_init, pool_put};
#[cfg(feature = "nfsserver")]
use crate::kern::sys_socket::fp_socket;
#[cfg(feature = "nfsserver")]
use crate::kern::uipc_mbuf::{m_free, m_freem, m_get, m_prepend};
#[cfg(feature = "nfsserver")]
use crate::kern::uipc_socket::{sosetopt, soshutdown};
#[cfg(feature = "nfsserver")]
use crate::kern::uipc_socket2::{solock_shared, soreserve, sounlock_shared};
#[cfg(feature = "nfsserver")]
use crate::kern::uipc_syscalls::{getsock, sockargs};
#[cfg(feature = "nfsserver")]
use crate::machine::copy::{copyin, copyin_obj};
#[cfg(feature = "nfsserver")]
use crate::machine::intr::IPL_NONE;
#[cfg(feature = "nfsserver")]
use crate::netinet::in_::{IPPORT_RESERVED, IPPROTO_TCP, IPPROTO_UDP, in_nam2sin};
#[cfg(feature = "nfsserver")]
use crate::netinet::tcp::TCP_NODELAY;
#[cfg(feature = "nfsserver")]
use crate::nfs::nfs::{
    NFSD_CHECKSLP, NFSD_HEAD, NFSD_HEAD_FLAG, NFSD_REQINPROG, NFSD_WAITING, Nfsd, NfsdArgs,
    NfsdSrvargs, NfsrvDescript, NfssvcSock, NsChain, SLP_ALLFLAGS, SLP_DISCONN, SLP_DOREC,
    SLP_NEEDQ, SLP_VALID,
};
#[cfg(feature = "nfsserver")]
use crate::nfs::nfs::{NFSSVC_ADDSOCK, NFSSVC_NFSD};
#[cfg(feature = "nfsserver")]
use crate::nfs::nfs_serv::{self, NfsrvProc};
#[cfg(feature = "nfsserver")]
use crate::nfs::nfs_socket::{
    nfs_send, nfs_sndlock, nfs_sndunlock, nfsrv_dorec, nfsrv_rcv, nfsrv_wakenfsd,
};
#[cfg(feature = "nfsserver")]
use crate::nfs::nfs_srvcache::{nfsrv_cleancache, nfsrv_getcache, nfsrv_updatecache};
#[cfg(feature = "nfsserver")]
use crate::nfs::nfs_subs::NFSSTATS;
#[cfg(feature = "nfsserver")]
use crate::nfs::nfsproto::{NFS_MAXPACKET, NFS_NPROCS};
#[cfg(feature = "nfsserver")]
use crate::nfs::nfsrvcache::{RC_DOIT, RC_DROPIT, RC_REPLY};
#[cfg(feature = "nfsserver")]
use crate::sys::file::{File, fref, frele};
#[cfg(feature = "nfsserver")]
use crate::sys::malloc::{M_NFSD, M_NFSSVC, M_WAITOK, M_ZERO};
#[cfg(feature = "nfsserver")]
use crate::sys::mbuf::{M_WAIT, MT_SONAME, MT_SOOPTS, Mbuf, mtod};
use crate::sys::param::PCATCH;
#[cfg(feature = "nfsserver")]
use crate::sys::param::PSOCK;
#[cfg(feature = "nfsserver")]
use crate::sys::pool::{PR_WAITOK, Pool};
#[cfg(feature = "nfsserver")]
use crate::sys::protosw::PR_CONNREQUIRED;
#[cfg(feature = "nfsserver")]
use crate::sys::queue::TailqHead;
#[cfg(feature = "nfsserver")]
use crate::sys::socket::{AF_INET, SHUT_RDWR, SO_KEEPALIVE, SOCK_DGRAM, SOCK_STREAM, SOL_SOCKET};
#[cfg(feature = "nfsserver")]
use crate::sys::socketvar::SB_NOINTR;
#[cfg(feature = "nfsserver")]
use crate::sys::syscallargs::SysNfssvcArgs;
#[cfg(feature = "nfsserver")]
use crate::sys::systm::sysargs;

#[cfg(feature = "nfsclient")]
use crate::kern::kern_kthread::{kthread_create, kthread_exit};
#[cfg(feature = "nfsclient")]
use crate::kern::kern_sig::psignal;
#[cfg(feature = "nfsclient")]
use crate::kern::kern_synch::wakeup_one;
#[cfg(feature = "nfsclient")]
use crate::kern::vfs_bio::{BCSTATS, buf_undirty, bufcache_take};
#[cfg(feature = "nfsclient")]
use crate::kern::vfs_biomem::buf_acquire;
#[cfg(feature = "nfsclient")]
use crate::machine::cpu::curproc;
#[cfg(feature = "nfsclient")]
use crate::machine::intr::{splbio, splx};
#[cfg(feature = "nfsclient")]
use crate::nfs::nfs::{NFS_MAXASYNCDAEMON, nfs_niothreads};
#[cfg(feature = "nfsclient")]
use crate::nfs::nfs_bio::nfs_doio;
#[cfg(feature = "nfsclient")]
use crate::nfs::nfs_vnops::NFS_NUMASYNC;
#[cfg(feature = "nfsclient")]
use crate::nfs::nfsnode::{NFS_BUFQ, NFS_BUFQLEN, NFS_BUFQMAX};
#[cfg(feature = "nfsclient")]
use crate::sys::buf::{
    B_ASYNC, B_BUSY, B_DELWRI, B_DONE, B_ERROR, B_NEEDCOMMIT, B_NOCACHE, B_READ,
};
#[cfg(feature = "nfsclient")]
use crate::sys::param::PWAIT;
#[cfg(feature = "nfsclient")]
use crate::sys::signal::SIGKILL;

/// `SLP_INIT`: NFS data undergoing initialization.
#[cfg(feature = "nfsserver")]
const SLP_INIT: i32 = 0x01;
/// `SLP_WANTINIT`: thread waiting on NFS initialization.
#[cfg(feature = "nfsserver")]
const SLP_WANTINIT: i32 = 0x02;

/// `nfsrv_descript_pl`: the pool of the descriptors of the requests being served
/// (`nfsrv_dorec` takes them, `nfssvc_nfsd` gives them back).
#[cfg(feature = "nfsserver")]
pub static NFSRV_DESCRIPT_PL: Pool = Pool::new();

/// `struct nfssvc_sockhead`: the server's sockets, through `ns_chain` (kernel lock).
#[cfg(feature = "nfsserver")]
pub struct Nfssvcsockhead(pub TailqHead<NsChain>);

// SAFETY: the list is changed under the kernel lock, as in C.
#[cfg(feature = "nfsserver")]
unsafe impl Sync for Nfssvcsockhead {}

/// `nfssvc_sockhead`: every socket the server serves, the datagram one first.
#[cfg(feature = "nfsserver")]
pub static NFSSVC_SOCKHEAD: Nfssvcsockhead = Nfssvcsockhead(TailqHead::new());

/// `nfssvc_sockhead_flag`: `SLP_INIT` and `SLP_WANTINIT`.
#[cfg(feature = "nfsserver")]
static NFSSVC_SOCKHEAD_FLAG: AtomicI32 = AtomicI32::new(0);

/// `nfs_udpsock`: the datagram socket's `nfssvc_sock`, always on the list.
#[cfg(feature = "nfsserver")]
static NFS_UDPSOCK: core::sync::atomic::AtomicPtr<NfssvcSock> =
    core::sync::atomic::AtomicPtr::new(core::ptr::null_mut());

/// `nfsd_waiting`: the number of nfsds sleeping for a socket with work.
#[cfg(feature = "nfsserver")]
static NFSD_WAITING_COUNT: AtomicI32 = AtomicI32::new(0);

/// `nfs_numnfsd`: the number of running nfsds.
#[cfg(feature = "nfsserver")]
static NFS_NUMNFSD: AtomicI32 = AtomicI32::new(0);

/// `nfs_asyncdaemon[NFS_MAXASYNCDAEMON]`: the threads of the running nfsiods (null: free).
#[cfg(feature = "nfsclient")]
static NFS_ASYNCDAEMON: [AtomicPtr<Proc>; NFS_MAXASYNCDAEMON as usize] =
    [const { AtomicPtr::new(ptr::null_mut()) }; NFS_MAXASYNCDAEMON as usize];

/// `nfsd_waiting`'s value, for the nfsds' own bookkeeping and debugging.
#[cfg(feature = "nfsserver")]
pub fn nfsd_waiting() -> i32 {
    NFSD_WAITING_COUNT.load(Relaxed)
}

/// `nfsrv3_procs[NFS_NPROCS]`: the server procedure of each NFS version 3 procedure number
/// (`nd_procnum`; `nfs_getreq` maps version 2 numbers onto these), from `nfs_serv.rs`.
#[cfg(feature = "nfsserver")]
pub(crate) static NFSRV3_PROCS: [NfsrvProc; NFS_NPROCS] = [
    nfs_serv::nfsrv_null,
    nfs_serv::nfsrv_getattr,
    nfs_serv::nfsrv_setattr,
    nfs_serv::nfsrv_lookup,
    nfs_serv::nfsrv3_access,
    nfs_serv::nfsrv_readlink,
    nfs_serv::nfsrv_read,
    nfs_serv::nfsrv_write,
    nfs_serv::nfsrv_create,
    nfs_serv::nfsrv_mkdir,
    nfs_serv::nfsrv_symlink,
    nfs_serv::nfsrv_mknod,
    nfs_serv::nfsrv_remove,
    nfs_serv::nfsrv_rmdir,
    nfs_serv::nfsrv_rename,
    nfs_serv::nfsrv_link,
    nfs_serv::nfsrv_readdir,
    nfs_serv::nfsrv_readdirplus,
    nfs_serv::nfsrv_statfs,
    nfs_serv::nfsrv_fsinfo,
    nfs_serv::nfsrv_pathconf,
    nfs_serv::nfsrv_commit,
    nfs_serv::nfsrv_noop,
];

/// `sys_nfssvc(p, v, retval)`: NFS server pseudo system call for the nfsds. Based on the
/// flag value it either:
/// - adds a socket to the selection list
/// - remains in the kernel as an nfsd
pub fn sys_nfssvc(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    // Must be super user.
    suser(p)?;

    #[cfg(not(feature = "nfsserver"))]
    {
        let _ = v;
        Err(Errno::ENOSYS)
    }
    #[cfg(feature = "nfsserver")]
    {
        let uap: &SysNfssvcArgs = sysargs(v);
        let flags = uap.flag.get();

        while NFSSVC_SOCKHEAD_FLAG.load(Relaxed) & SLP_INIT != 0 {
            NFSSVC_SOCKHEAD_FLAG.fetch_or(SLP_WANTINIT, Relaxed);
            let _ = tsleep_nsec(
                core::ptr::from_ref(&NFSSVC_SOCKHEAD),
                PSOCK,
                "nfsd init",
                INFSLP,
            );
        }

        let error = match flags {
            NFSSVC_ADDSOCK => nfssvc_addsock_args(p, uap.argp.get() as usize),
            NFSSVC_NFSD => {
                // The `struct nfsd_srvargs` is only checked for being readable.
                let mut nsd = [0u8; NfsdSrvargs::SIZE];
                match copyin(uap.argp.get() as usize, &mut nsd) {
                    Err(e) => Err(e),
                    Ok(()) => {
                        let Some(mem) = malloc(size_of::<Nfsd>(), M_NFSD, M_WAITOK | M_ZERO) else {
                            return Err(Errno::ENOMEM);
                        };
                        let nfsd = mem.cast::<Nfsd>();
                        // SAFETY: a fresh allocation of `size_of::<Nfsd>()` bytes (malloc aligns
                        // to the bucket size, at least 16), written once; `nfssvc_nfsd` frees
                        // it when the nfsd is done.
                        let nfsd: &'static Nfsd = unsafe {
                            nfsd.as_ptr().write(Nfsd::new());
                            nfsd.as_ref()
                        };
                        nfsd.nfsd_procp.set(core::ptr::from_ref(p));
                        nfsd.nfsd_slp.set(None);

                        nfssvc_nfsd(nfsd)
                    }
                }
            }
            _ => Err(Errno::EINVAL),
        };

        match error {
            Err(Errno::EINTR | Errno::ERESTART) => Ok(()),
            error => error,
        }
    }
}

/// The `NFSSVC_ADDSOCK` case of `sys_nfssvc`: copies in the `struct nfsd_args`, finds the
/// socket and the client address (connected sockets) and adds it.
#[cfg(feature = "nfsserver")]
fn nfssvc_addsock_args(p: &Proc, argp: usize) -> Result<(), Errno> {
    let nfsdarg: NfsdArgs = copyin_obj(argp)?;

    let fp = getsock(p, nfsdarg.sock)?;

    // Get the client address for connected sockets.
    let nam = if nfsdarg.name == 0 || nfsdarg.namelen == 0 {
        None
    } else {
        match sockargs(nfsdarg.name, nfsdarg.namelen as usize, MT_SONAME) {
            Ok(nam) => Some(nam),
            Err(e) => {
                let _ = frele(fp, p);
                return Err(e);
            }
        }
    };
    let error = nfssvc_addsock(fp, nam);
    let _ = frele(fp, p);
    error
}

/// `nfssvc_addsock(fp, mynam)`: adds a socket to the list for servicing by nfsds.
#[cfg(feature = "nfsserver")]
pub fn nfssvc_addsock(fp: &'static File, mynam: Option<&'static Mbuf>) -> Result<(), Errno> {
    let so = fp_socket(fp);
    let mut tslp: Option<&'static NfssvcSock> = None;

    // Add it to the list, as required.
    if i32::from(so.so_proto.pr_protocol) == IPPROTO_UDP {
        let udp = nfs_udpsock();
        if udp.ns_flag.get() & SLP_VALID != 0 {
            m_freem(mynam);
            return Err(Errno::EPERM);
        }
        tslp = Some(udp);
    }
    // Allow only IPv4 UDP and TCP sockets.
    let sotype = so.so_type.get();
    if (sotype != SOCK_STREAM && sotype != SOCK_DGRAM) || so.dom_family() != i32::from(AF_INET) {
        m_freem(mynam);
        return Err(Errno::EINVAL);
    }

    let siz = if sotype == SOCK_STREAM {
        NFS_MAXPACKET + size_of::<u64>() // sizeof(u_long)
    } else {
        NFS_MAXPACKET
    };
    solock_shared(so);
    let error = soreserve(so, siz as u64, siz as u64);
    sounlock_shared(so);
    if let Err(e) = error {
        m_freem(mynam);
        return Err(e);
    }

    // Set protocol specific options { for now TCP only } and reserve some space. For datagram
    // sockets, this can get called repeatedly for the same socket, but that isn't harmful.
    if sotype == SOCK_STREAM {
        let Some(m) = m_get(M_WAIT, MT_SOOPTS) else {
            m_freem(mynam);
            return Err(Errno::ENOBUFS);
        };
        set_mtod_int(m, 1);
        let _ = sosetopt(so, SOL_SOCKET, SO_KEEPALIVE, Some(m));
        m_freem(Some(m));
    }
    if so.dom_family() == i32::from(AF_INET) && i32::from(so.so_proto.pr_protocol) == IPPROTO_TCP {
        let Some(m) = m_get(M_WAIT, MT_SOOPTS) else {
            m_freem(mynam);
            return Err(Errno::ENOBUFS);
        };
        set_mtod_int(m, 1);
        let _ = sosetopt(so, IPPROTO_TCP, TCP_NODELAY, Some(m));
        m_freem(Some(m));
    }
    solock_shared(so);
    mtx_enter(&so.so_rcv.sb_mtx);
    so.so_rcv
        .sb_flags
        .set(so.so_rcv.sb_flags.get() & !SB_NOINTR);
    so.so_rcv.sb_timeo_nsecs.set(INFSLP);
    mtx_leave(&so.so_rcv.sb_mtx);
    mtx_enter(&so.so_snd.sb_mtx);
    so.so_snd
        .sb_flags
        .set(so.so_snd.sb_flags.get() & !SB_NOINTR);
    so.so_snd.sb_timeo_nsecs.set(INFSLP);
    mtx_leave(&so.so_snd.sb_mtx);
    sounlock_shared(so);

    let slp = match tslp {
        Some(slp) => slp,
        None => {
            let Some(slp) = nfssvc_sock_alloc() else {
                m_freem(mynam);
                return Err(Errno::ENOMEM);
            };
            // SAFETY: a fresh `nfssvc_sock` on no list; it stays in place until
            // `nfsrv_slpderef` unlinks and frees it.
            unsafe { NFSSVC_SOCKHEAD.0.insert_tail(slp) };
            slp
        }
    };
    slp.ns_so.set(Some(so));
    slp.ns_nam.set(mynam);
    fref(fp);
    slp.ns_fp.set(Some(fp));
    so.so_upcallarg
        .set(core::ptr::from_ref(slp).cast_mut().cast::<c_void>());
    so.so_upcall.set(Some(nfsrv_rcv));
    slp.ns_flag.set(SLP_VALID | SLP_NEEDQ);
    nfsrv_wakenfsd(slp);
    Ok(())
}

/// `*mtod(m, int32_t *) = v; m->m_len = sizeof(int32_t)`: an option value for `sosetopt`.
#[cfg(feature = "nfsserver")]
fn set_mtod_int(m: &Mbuf, v: i32) {
    // SAFETY: an mbuf's storage holds `MLEN` bytes at least; the data may be unaligned.
    unsafe { mtod::<i32>(m).write_unaligned(v) };
    m.m_len().set(size_of::<i32>() as u32);
}

/// `malloc(sizeof(struct nfssvc_sock), M_NFSSVC, M_WAITOK|M_ZERO)`.
#[cfg(feature = "nfsserver")]
fn nfssvc_sock_alloc() -> Option<&'static NfssvcSock> {
    let mem = malloc(size_of::<NfssvcSock>(), M_NFSSVC, M_WAITOK | M_ZERO)?;
    let slp = mem.cast::<NfssvcSock>();
    // SAFETY: a fresh allocation of `size_of::<NfssvcSock>()` bytes (malloc aligns to the
    // bucket size, at least 16), written once; `nfsrv_slpderef` (or `nfsrv_init`) frees it.
    Some(unsafe {
        slp.as_ptr().write(NfssvcSock::new());
        &*slp.as_ptr()
    })
}

/// `free(slp, M_NFSSVC, sizeof(*slp))`.
#[cfg(feature = "nfsserver")]
fn nfssvc_sock_free(slp: &'static NfssvcSock) {
    free(
        NonNull::from(slp).cast::<u8>(),
        M_NFSSVC,
        size_of::<NfssvcSock>(),
    );
}

/// `nfs_udpsock`: the datagram socket's `nfssvc_sock` (made by `nfsrv_init`).
#[cfg(feature = "nfsserver")]
fn nfs_udpsock() -> &'static NfssvcSock {
    let p = NFS_UDPSOCK.load(Relaxed);
    if p.is_null() {
        panic(format_args!("nfs_udpsock: nfsrv_init has not run"));
    }
    // SAFETY: only `nfsrv_init` stores here, a live `nfssvc_sock` that stays on
    // `nfssvc_sockhead` (`nfsrv_slpderef` never frees it) until the next `nfsrv_init(1)`.
    unsafe { &*p }
}

/// `nfssvc_checknam(nam)`: whether the client address is acceptable: an IPv4 address with a
/// reserved source port.
#[cfg(feature = "nfsserver")]
fn nfssvc_checknam(nam: Option<&Mbuf>) -> bool {
    let Some(nam) = nam else {
        return false;
    };
    let Ok(sin) = in_nam2sin(nam) else {
        return false;
    };
    // SAFETY: `in_nam2sin` checked that `nam` holds a whole `struct sockaddr_in`; the data
    // may be unaligned.
    let port = unsafe { core::ptr::addr_of!((*sin).sin_port).read_unaligned() };
    i32::from(u16::from_be(port)) < IPPORT_RESERVED
}

/// `nfssvc_nfsd(nfsd)`: called by `nfssvc()` for nfsds. Just loops around servicing rpc
/// requests until it is killed by a signal.
#[cfg(feature = "nfsserver")]
fn nfssvc_nfsd(nfsd: &'static Nfsd) -> Result<(), Errno> {
    let mut nd: Option<NonNull<NfsrvDescript>> = None;
    let mut error: Result<(), Errno> = Ok(());

    // SAFETY: `nfsd` is on no list; it stays in place until the `done` below unlinks it.
    unsafe { NFSD_HEAD.0.insert_tail(nfsd) };
    NFS_NUMNFSD.fetch_add(1, Relaxed);

    // Loop getting rpc requests until SIGKILL.
    'done: loop {
        let slp: &'static NfssvcSock;
        if nfsd.nfsd_flag.get() & NFSD_REQINPROG == 0 {
            // attach an nfssvc_sock to nfsd
            if let Err(e) = nfsrv_getslp(nfsd) {
                error = Err(e);
                break 'done;
            }

            let Some(s) = nfsd.nfsd_slp.get() else {
                panic(format_args!("nfssvc_nfsd: no socket for the nfsd"));
            };
            slp = s;

            if slp.ns_flag.get() & SLP_VALID != 0 {
                if slp.ns_flag.get() & (SLP_DISCONN | SLP_NEEDQ) == SLP_NEEDQ {
                    slp.ns_flag.set(slp.ns_flag.get() & !SLP_NEEDQ);
                    let _ = nfs_sndlock(&slp.ns_solock, None);
                    if let Some(so) = slp.ns_so.get() {
                        nfsrv_rcv(
                            so,
                            core::ptr::from_ref(slp).cast_mut().cast::<c_void>(),
                            M_WAIT,
                        );
                    }
                    nfs_sndunlock(&slp.ns_solock);
                }
                if slp.ns_flag.get() & SLP_DISCONN != 0 {
                    nfsrv_zapsock(slp);
                }

                error = match nfsrv_dorec(slp, nfsd) {
                    Ok(n) => {
                        nd = Some(n);
                        Ok(())
                    }
                    Err(e) => Err(e),
                };
                nfsd.nfsd_flag.set(nfsd.nfsd_flag.get() | NFSD_REQINPROG);
            }
        } else {
            error = Ok(());
            let Some(s) = nfsd.nfsd_slp.get() else {
                panic(format_args!("nfssvc_nfsd: request in progress, no socket"));
            };
            slp = s;
        }

        if error.is_err() || slp.ns_flag.get() & SLP_VALID == 0 {
            if let Some(n) = nd.take() {
                pool_put(&NFSRV_DESCRIPT_PL, n.cast::<u8>());
            }
            nfsd.nfsd_slp.set(None);
            nfsd.nfsd_flag.set(nfsd.nfsd_flag.get() & !NFSD_REQINPROG);
            nfsrv_slpderef(slp);
            continue;
        }

        let Some(so) = slp.ns_so.get() else {
            panic(format_args!("nfssvc_nfsd: a valid socket without a socket"));
        };
        let sotype = so.so_type.get();
        let solockp = if so.so_proto.pr_flags & PR_CONNREQUIRED != 0 {
            Some(&slp.ns_solock)
        } else {
            None
        };

        let Some(ndp) = nd else {
            panic(format_args!("nfssvc_nfsd: no request descriptor"));
        };
        // SAFETY: `nfsrv_dorec` made the descriptor an `nfsrv_descript_pl` item owned by this
        // nfsd until it puts it back (`nfsd_nd` is only a record of it); nothing else
        // reaches it.
        let ndr: &mut NfsrvDescript = unsafe { &mut *ndp.as_ptr() };
        ndr.nd_nam = if ndr.nd_nam2.is_some() {
            ndr.nd_nam2
        } else {
            slp.ns_nam.get()
        };

        let mut mreq: Option<&'static Mbuf> = None;
        let mut cacherep = nfsrv_getcache(ndr, slp, &mut mreq);
        let mut send_reply = false;
        match cacherep {
            RC_DOIT => 'doit: {
                // Unless this is a null request (server ping), make sure that the client is
                // using a reserved source port.
                if ndr.nd_procnum != 0 && !nfssvc_checknam(ndr.nd_nam) {
                    // drop it
                    m_freem(ndr.nd_mrep.take());
                    m_freem(ndr.nd_nam2);
                    break 'doit;
                }
                let proc_fn = match NFSRV3_PROCS.get(ndr.nd_procnum) {
                    Some(f) => *f,
                    None => panic(format_args!(
                        "nfssvc_nfsd: procedure {} out of range",
                        ndr.nd_procnum
                    )),
                };
                let Some(procp) = (
                    // SAFETY: `nfsd_procp` is the process that entered the kernel as this nfsd
                    // (`sys_nfssvc`); it is running this very code.
                    unsafe { nfsd.nfsd_procp.get().as_ref() }
                ) else {
                    panic(format_args!("nfssvc_nfsd: nfsd without a process"));
                };
                let r = proc_fn(ndr, slp, procp, &mut mreq);
                if mreq.is_none() {
                    m_freem(ndr.nd_nam2);
                    m_freem(ndr.nd_mrep.take());
                    break 'doit;
                }
                if r.is_err() {
                    NFSSTATS.srv_errs.fetch_add(1, Relaxed);
                    nfsrv_updatecache(ndr, false, mreq);
                    m_freem(ndr.nd_nam2);
                    break 'doit;
                }
                NFSSTATS.srvrpccnt[ndr.nd_procnum].fetch_add(1, Relaxed);
                nfsrv_updatecache(ndr, true, mreq);
                ndr.nd_mrep = None;

                // FALLTHROUGH
                cacherep = RC_REPLY;
                send_reply = true;
            }
            RC_REPLY => send_reply = true,
            RC_DROPIT => {
                m_freem(ndr.nd_mrep.take());
                m_freem(ndr.nd_nam2);
            }
            _ => {}
        }

        if send_reply {
            let _ = cacherep;
            let mut siz = 0usize;
            let mut m = mreq;
            while let Some(mm) = m {
                siz += mm.m_len().get() as usize;
                m = mm.m_next().get();
            }

            if siz == 0 || siz > NFS_MAXPACKET {
                panic(format_args!("bad nfs svc reply, siz = {}", siz));
            }

            let mut m = mreq;
            if let Some(mm) = m {
                mm.m_pkthdr().len.set(siz as i32);
                mm.m_pkthdr().ph_ifidx.set(0);

                // For stream protocols, prepend a Sun RPC Record Mark.
                if sotype == SOCK_STREAM {
                    m = m_prepend(mm, size_of::<u32>() as i32, M_WAIT);
                    if let Some(mp) = m {
                        let mark = (0x8000_0000u32 | siz as u32).to_be();
                        // SAFETY: `m_prepend` made the first four bytes of `mp` its data;
                        // they may be unaligned.
                        unsafe { mtod::<u32>(mp).write_unaligned(mark) };
                    }
                }
            }

            if let Some(l) = solockp {
                let _ = nfs_sndlock(l, None);
            }

            let err = match m {
                Some(m) if slp.ns_flag.get() & SLP_VALID != 0 => nfs_send(so, ndr.nd_nam2, m, None),
                Some(m) => {
                    m_freem(Some(m));
                    Err(Errno::EPIPE)
                }
                // `m_prepend` freed the reply.
                None => Err(Errno::ENOBUFS),
            };
            m_freem(ndr.nd_nam2);
            m_freem(ndr.nd_mrep.take());
            if err == Err(Errno::EPIPE) {
                nfsrv_zapsock(slp);
            }
            if let Some(l) = solockp {
                nfs_sndunlock(l);
            }
            error = err;
            if matches!(err, Err(Errno::EINTR | Errno::ERESTART)) {
                pool_put(&NFSRV_DESCRIPT_PL, ndp.cast::<u8>());
                nfsrv_slpderef(slp);
                break 'done;
            }
        }

        // The descriptor goes back to its pool; the next record, if any, gets a new one.
        if let Some(n) = nd.take() {
            pool_put(&NFSRV_DESCRIPT_PL, n.cast::<u8>());
        }

        match nfsrv_dorec(slp, nfsd) {
            Ok(n) => nd = Some(n),
            Err(_) => {
                nfsd.nfsd_flag.set(nfsd.nfsd_flag.get() & !NFSD_REQINPROG);
                nfsd.nfsd_slp.set(None);
                nfsrv_slpderef(slp);
            }
        }
    }

    // done:
    // SAFETY: the nfsd was linked above and is still on the list.
    unsafe { NFSD_HEAD.0.remove(nfsd) };
    free(NonNull::from(nfsd).cast::<u8>(), M_NFSD, size_of::<Nfsd>());
    if NFS_NUMNFSD.fetch_sub(1, Relaxed) == 1 {
        nfsrv_init(1); // Reinitialize everything
    }
    error
}

/// `nfsrv_zapsock(slp)`: shut down a socket associated with an nfssvc_sock structure. Should
/// be called with the send lock set, if required. The trick here is to increment the sref at
/// the start, so that the nfsds will stop using it and clear ns_flag at the end so that it
/// will not be reassigned during cleanup.
#[cfg(feature = "nfsserver")]
pub fn nfsrv_zapsock(slp: &NfssvcSock) {
    slp.ns_flag.set(slp.ns_flag.get() & !SLP_ALLFLAGS);
    if let Some(fp) = slp.ns_fp.get() {
        fref(fp);
        slp.ns_fp.set(None);
        if let Some(so) = slp.ns_so.get() {
            so.so_upcall.set(None);
            let _ = soshutdown(so, SHUT_RDWR);
        }
        let _ = closef(fp, None::<&Proc>);
        if let Some(nam) = slp.ns_nam.take() {
            let _ = m_free(nam);
        }
        m_freem(slp.ns_raw.take());
        slp.ns_rawend.set(None);
        slp.ns_cc.set(0);
        slp.ns_reclen.set(0);
        let mut m = slp.ns_rec.take();
        slp.ns_recend.set(None);
        while let Some(mm) = m {
            let n = mm.m_nextpkt().get();
            m_freem(Some(mm));
            m = n;
        }
    }
}

/// `nfsrv_slpderef(slp)`: dereference a server socket structure. If it has no more
/// references and is no longer valid, you can throw it away.
#[cfg(feature = "nfsserver")]
pub fn nfsrv_slpderef(slp: &'static NfssvcSock) {
    let sref = slp.ns_sref.get().wrapping_sub(1);
    slp.ns_sref.set(sref);
    if sref == 0
        && slp.ns_flag.get() & SLP_VALID == 0
        && !core::ptr::eq(slp, NFS_UDPSOCK.load(Relaxed))
    {
        // SAFETY: a socket with no references is on the list (every socket is linked on
        // creation and unlinked only here and in `nfsrv_init`).
        unsafe { NFSSVC_SOCKHEAD.0.remove(slp) };
        nfssvc_sock_free(slp);
    }
}

/// `nfsrv_init(terminating)`: initialize the data structures for the server. Handshake with
/// any new nfsds starting up to avoid any chance of corruption.
#[cfg(feature = "nfsserver")]
pub fn nfsrv_init(terminating: i32) {
    if NFSSVC_SOCKHEAD_FLAG.load(Relaxed) & SLP_INIT != 0 {
        panic(format_args!("nfsd init"));
    }
    NFSSVC_SOCKHEAD_FLAG.fetch_or(SLP_INIT, Relaxed);
    if terminating != 0 {
        let mut slp = NFSSVC_SOCKHEAD.0.first();
        while let Some(s) = slp {
            let nslp = TailqHead::<NsChain>::next(s);
            if s.ns_flag.get() & SLP_VALID != 0 {
                nfsrv_zapsock(s);
            }
            // SAFETY: `s` is on the list (found by walking it) and is freed right away.
            unsafe { NFSSVC_SOCKHEAD.0.remove(s) };
            // SAFETY: every socket on the list is a `nfssvc_sock_alloc` allocation that lives
            // for `'static` until freed here or in `nfsrv_slpderef`.
            nfssvc_sock_free(unsafe { &*core::ptr::from_ref(s) });
            slp = nslp;
        }
        nfsrv_cleancache(); // And clear out server cache
    }

    NFSSVC_SOCKHEAD.0.init();
    NFSSVC_SOCKHEAD_FLAG.fetch_and(!SLP_INIT, Relaxed);
    if NFSSVC_SOCKHEAD_FLAG.load(Relaxed) & SLP_WANTINIT != 0 {
        NFSSVC_SOCKHEAD_FLAG.fetch_and(!SLP_WANTINIT, Relaxed);
        wakeup(core::ptr::from_ref(&NFSSVC_SOCKHEAD));
    }

    NFSD_HEAD.0.init();
    NFSD_HEAD_FLAG.fetch_and(!NFSD_CHECKSLP, Relaxed);

    let Some(udp) = nfssvc_sock_alloc() else {
        panic(format_args!(
            "nfsrv_init: no memory for the datagram socket"
        ));
    };
    NFS_UDPSOCK.store(core::ptr::from_ref(udp).cast_mut(), Relaxed);
    // SAFETY: a fresh `nfssvc_sock` on no list; it stays in place until `nfsrv_init(1)`.
    unsafe { NFSSVC_SOCKHEAD.0.insert_head(udp) };

    if terminating == 0 {
        pool_init(
            &NFSRV_DESCRIPT_PL,
            size_of::<NfsrvDescript>(),
            0,
            IPL_NONE,
            PR_WAITOK,
            "ndscpl",
            None,
        );
    }
}

/// `nfssvc_iod(arg)`: asynchronous I/O threads for client nfs. They do read-ahead and
/// write-behind operations on the block I/O cache. Never returns unless it fails or gets
/// killed.
#[cfg(feature = "nfsclient")]
pub fn nfssvc_iod(_arg: *mut c_void) {
    let Some(p) = curproc() else {
        panic(format_args!("nfssvc_iod: no curproc"));
    };
    let kvaslots = BCSTATS.kvaslots.load(Relaxed);
    let numbufs = BCSTATS.numbufs.load(Relaxed);
    let mut bufcount = 256.min(kvaslots / 8);
    bufcount = bufcount.min(numbufs / 8);

    // Assign my position or return error if too many already running.
    let mut myiod = None;
    for (i, slot) in NFS_ASYNCDAEMON.iter().enumerate() {
        if slot.load(Relaxed).is_null() {
            myiod = Some(i);
            break;
        }
    }
    let Some(myiod) = myiod else {
        kthread_exit(Errno::EBUSY.as_i32());
    };

    NFS_ASYNCDAEMON[myiod].store(ptr::from_ref(p).cast_mut(), Relaxed);
    NFS_NUMASYNC.fetch_add(1, Relaxed);

    // Upper limit on how many bufs we'll queue up for this iod.
    if i64::from(NFS_BUFQMAX.load(Relaxed)) > kvaslots / 4 {
        NFS_BUFQMAX.store((kvaslots / 4) as u32, Relaxed);
        bufcount = 0;
    }
    if i64::from(NFS_BUFQMAX.load(Relaxed)) > numbufs / 4 {
        NFS_BUFQMAX.store((numbufs / 4) as u32, Relaxed);
        bufcount = 0;
    }

    NFS_BUFQMAX.fetch_add(bufcount as u32, Relaxed);
    wakeup(ptr::from_ref(&NFS_BUFQLEN)); // wake up anyone waiting for room to enqueue IO

    // Just loop around doin our stuff until SIGKILL.
    let mut error: Result<(), Errno> = Ok(());
    loop {
        while NFS_BUFQ.0.first().is_none() && error.is_ok() {
            error = tsleep_nsec(ptr::from_ref(&NFS_BUFQ), PWAIT | PCATCH, "nfsidl", INFSLP);
        }
        while let Some(first) = NFS_BUFQ.0.first() {
            // Take one off the front of the list.
            // SAFETY: `first` is the head of the queue `nfs_asyncio` linked it on.
            unsafe { NFS_BUFQ.0.remove(first) };
            NFS_BUFQLEN.fetch_sub(1, Relaxed);
            wakeup_one(ptr::from_ref(&NFS_BUFQLEN));
            let mut bp = first;
            if bp.b_flags.get() & B_READ != 0 {
                let _ = nfs_doio(bp, None);
            } else {
                loop {
                    // Look for a delayed write for the same vnode, so I can do it now. We
                    // must grab it before calling nfs_doio() to avoid any risk of the vnode
                    // getting vclean()'d while we are doing the write rpc.
                    let Some(vp) = bp.b_vp.get() else {
                        panic(format_args!("nfssvc_iod: buffer without a vnode"));
                    };
                    let s = splbio();
                    let mut nbp = None;
                    for b in vp.v_dirtyblkhd.iter() {
                        if b.b_flags.get() & (B_BUSY | B_DELWRI | B_NEEDCOMMIT | B_NOCACHE)
                            != B_DELWRI
                        {
                            continue;
                        }
                        b.b_flags.set(b.b_flags.get() | B_ASYNC);
                        bufcache_take(b);
                        buf_acquire(b);
                        nbp = Some(b);
                        break;
                    }
                    // For the delayed write, do the first part of nfs_bwrite() up to, but not
                    // including nfs_strategy().
                    if let Some(nbp) = nbp {
                        nbp.b_flags
                            .set(nbp.b_flags.get() & !(B_READ | B_DONE | B_ERROR));
                        buf_undirty(nbp);
                        if let Some(nvp) = nbp.b_vp.get() {
                            nvp.v_numoutput.set(nvp.v_numoutput.get() + 1);
                        }
                    }
                    splx(s);

                    let _ = nfs_doio(bp, None);
                    match nbp {
                        Some(n) => bp = n,
                        None => break,
                    }
                }
            }
        }
        if let Err(e) = error {
            NFS_ASYNCDAEMON[myiod].store(ptr::null_mut(), Relaxed);
            NFS_NUMASYNC.fetch_sub(1, Relaxed);
            NFS_BUFQMAX.fetch_sub(bufcount as u32, Relaxed);
            kthread_exit(e.as_i32());
        }
    }
}

/// `nfs_getset_niothreads(set)`: reads (`set` false: `nfs_niothreads` becomes the number of
/// running nfsiods) or sets (starts or kills nfsiods until there are `nfs_niothreads`).
#[cfg(feature = "nfsclient")]
pub fn nfs_getset_niothreads(set: bool) {
    let have = NFS_ASYNCDAEMON
        .iter()
        .filter(|slot| !slot.load(Relaxed).is_null())
        .count() as i32;

    if set {
        // clamp to sane range
        let n = nfs_niothreads.load(Relaxed).clamp(0, NFS_MAXASYNCDAEMON);
        nfs_niothreads.store(n, Relaxed);

        let mut start = n - have;

        while start > 0 {
            let _ = kthread_create(nfssvc_iod, ptr::null_mut(), b"nfsio");
            start -= 1;
        }

        for slot in NFS_ASYNCDAEMON.iter() {
            if start >= 0 {
                break;
            }
            let p = slot.load(Relaxed);
            if !p.is_null() {
                // SAFETY: an nfsiod's thread stays valid until it clears its slot just before
                // it exits (`nfssvc_iod`), under the kernel lock like this code.
                psignal(unsafe { &*p }, SIGKILL);
                start += 1;
            }
        }
    } else if nfs_niothreads.load(Relaxed) >= 0 {
        nfs_niothreads.store(have, Relaxed);
    }
}

/// `nfsrv_getslp(nfsd)`: find an nfssrv_sock for nfsd, sleeping if needed.
#[cfg(feature = "nfsserver")]
pub fn nfsrv_getslp(nfsd: &'static Nfsd) -> Result<(), Errno> {
    loop {
        while nfsd.nfsd_slp.get().is_none() && NFSD_HEAD_FLAG.load(Relaxed) & NFSD_CHECKSLP == 0 {
            nfsd.nfsd_flag.set(nfsd.nfsd_flag.get() | NFSD_WAITING);
            NFSD_WAITING_COUNT.fetch_add(1, Relaxed);
            let error = tsleep_nsec(core::ptr::from_ref(nfsd), PSOCK | PCATCH, "nfsd", INFSLP);
            NFSD_WAITING_COUNT.fetch_sub(1, Relaxed);
            error?;
        }

        if nfsd.nfsd_slp.get().is_none() && NFSD_HEAD_FLAG.load(Relaxed) & NFSD_CHECKSLP != 0 {
            let mut found = false;
            for slp in NFSSVC_SOCKHEAD.0.iter() {
                if slp.ns_flag.get() & (SLP_VALID | SLP_DOREC) == (SLP_VALID | SLP_DOREC) {
                    slp.ns_flag.set(slp.ns_flag.get() & !SLP_DOREC);
                    slp.ns_sref.set(slp.ns_sref.get() + 1);
                    // SAFETY: a socket on the list is a `'static` allocation (see
                    // `nfsrv_init`), alive while an nfsd holds a reference.
                    nfsd.nfsd_slp
                        .set(Some(unsafe { &*core::ptr::from_ref(slp) }));
                    found = true;
                    break;
                }
            }
            if !found {
                NFSD_HEAD_FLAG.fetch_and(!NFSD_CHECKSLP, Relaxed);
            }
        }

        if nfsd.nfsd_slp.get().is_some() {
            return Ok(());
        }
        // again:
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for `nfs_syscalls.c`: the client address check of `nfssvc_checknam`, the server's
    // socket list (`nfsrv_init`, `nfsrv_slpderef`, `nfsrv_zapsock`, `nfsrv_getslp`) and the nfsiod
    // bookkeeping.

    use std::boxed::Box;

    use super::*;
    use crate::kern::uipc_mbuf::tests::setup;
    use crate::netinet::in_::{InAddr, SockaddrIn};
    use crate::sys::mbuf::{M_WAIT, MT_SONAME};
    use crate::sys::socket::AF_INET;

    /// An `MT_SONAME` mbuf holding an IPv4 address with the given port (host order).
    fn client(port: u16, len: u8) -> &'static Mbuf {
        let sin = SockaddrIn {
            sin_len: len,
            sin_family: AF_INET,
            sin_port: port.to_be(),
            sin_addr: InAddr {
                s_addr: u32::from_be_bytes([10, 0, 0, 1]).to_be(),
            },
            sin_zero: [0; 8],
        };
        let m = m_get(M_WAIT, MT_SONAME).expect("an mbuf");
        // SAFETY: an mbuf's data area holds `MLEN` bytes, more than a `sockaddr_in`; the write is
        // unaligned-safe.
        unsafe { mtod::<SockaddrIn>(m).write_unaligned(sin) };
        m.m_len().set(u32::from(len));
        m
    }

    #[cfg(feature = "nfsserver")]
    #[test]
    fn client_address_must_be_ipv4_with_a_reserved_port() {
        let _g = setup();
        let sin_len = size_of::<SockaddrIn>() as u8;

        assert!(nfssvc_checknam(Some(client(700, sin_len))));
        assert!(nfssvc_checknam(Some(client(1023, sin_len))));
        // An unprivileged port, no address at all and a malformed one are refused.
        assert!(!nfssvc_checknam(Some(client(1024, sin_len))));
        assert!(!nfssvc_checknam(Some(client(40000, sin_len))));
        assert!(!nfssvc_checknam(None));
        assert!(!nfssvc_checknam(Some(client(700, sin_len - 1))));
    }

    #[cfg(feature = "nfsserver")]
    #[test]
    fn server_socket_list_bookkeeping() {
        let _g = setup();
        // The server cache is global and `setup` replaced the memory under it: entries another
        // test left there (nfs_srvcache's) belong to the old memory, and `nfsrv_init(1)` below
        // frees whatever the cache holds. Start from an empty cache in this test's memory.
        crate::nfs::nfs_srvcache::NUMNFSRVCACHE.store(0, Relaxed);
        crate::nfs::nfs_srvcache::nfsrv_initcache();
        nfsrv_init(0);

        // The datagram socket is always the first on the list.
        let udp = nfs_udpsock();
        assert!(core::ptr::eq(
            NFSSVC_SOCKHEAD.0.first().expect("the list is not empty"),
            udp
        ));
        assert_eq!(udp.ns_flag.get(), 0);
        assert!(NFSD_HEAD.0.is_empty());

        // A stream socket: linked after it.
        let tcp = nfssvc_sock_alloc().expect("memory");
        // SAFETY: a fresh socket on no list, which stays in place until `nfsrv_slpderef` frees it.
        unsafe { NFSSVC_SOCKHEAD.0.insert_tail(tcp) };
        assert_eq!(NFSSVC_SOCKHEAD.0.iter().count(), 2);

        // Valid or referenced: dereferencing keeps it.
        tcp.ns_flag.set(SLP_VALID);
        tcp.ns_sref.set(2);
        nfsrv_slpderef(tcp);
        assert_eq!(tcp.ns_sref.get(), 1);
        nfsrv_slpderef(tcp);
        assert_eq!(tcp.ns_sref.get(), 0);
        assert_eq!(NFSSVC_SOCKHEAD.0.iter().count(), 2);
        // No longer valid and the last reference gone: unlinked and freed.
        tcp.ns_flag.set(0);
        tcp.ns_sref.set(1);
        nfsrv_slpderef(tcp);
        assert_eq!(NFSSVC_SOCKHEAD.0.iter().count(), 1);

        // The datagram socket is reused, never freed.
        udp.ns_sref.set(1);
        nfsrv_slpderef(udp);
        assert!(core::ptr::eq(NFS_UDPSOCK.load(Relaxed), udp));
        assert_eq!(NFSSVC_SOCKHEAD.0.iter().count(), 1);

        // An nfsd looks for a socket with records (NFSD_CHECKSLP): finds the valid one that has
        // `SLP_DOREC`, takes a reference and clears the flag; with none left it clears NFSD_CHECKSLP.
        let nfsd: &'static Nfsd = Box::leak(Box::new(Nfsd::new()));
        udp.ns_sref.set(0);
        udp.ns_flag.set(SLP_VALID | SLP_DOREC);
        NFSD_HEAD_FLAG.fetch_or(NFSD_CHECKSLP, Relaxed);
        assert_eq!(nfsrv_getslp(nfsd), Ok(()));
        assert!(nfsd.nfsd_slp.get().is_some_and(|s| core::ptr::eq(s, udp)));
        assert_eq!(udp.ns_sref.get(), 1);
        assert_eq!(udp.ns_flag.get(), SLP_VALID);
        // One that already has its socket is served at once.
        assert_eq!(nfsrv_getslp(nfsd), Ok(()));
        assert_eq!(udp.ns_sref.get(), 1);

        // Zapping a socket without a file only clears its flags.
        udp.ns_flag.set(SLP_VALID | SLP_NEEDQ | SLP_DOREC);
        nfsrv_zapsock(udp);
        assert_eq!(udp.ns_flag.get(), 0);

        // Terminating: every socket goes, a new datagram socket is made, the server's lists restart.
        nfsrv_init(1);
        assert_eq!(NFSSVC_SOCKHEAD.0.iter().count(), 1);
        assert_eq!(nfs_udpsock().ns_flag.get(), 0);
        assert!(NFSD_HEAD.0.is_empty());
        assert_eq!(NFSD_HEAD_FLAG.load(Relaxed) & NFSD_CHECKSLP, 0);
    }

    #[cfg(feature = "nfsserver")]
    #[test]
    fn the_temporary_procedure_table_covers_every_procedure() {
        assert_eq!(NFSRV3_PROCS.len(), NFS_NPROCS);
    }

    #[cfg(feature = "nfsclient")]
    #[test]
    fn nfsiod_threads_are_counted_and_the_sysctl_value_follows() {
        // Nothing runs: the slots are free and the count is zero.
        assert!(NFS_ASYNCDAEMON.iter().all(|p| p.load(Relaxed).is_null()));
        assert_eq!(NFS_NUMASYNC.load(Relaxed), 0);

        // Reading (`set` false) makes `nfs_niothreads` the number of running iods, but only once
        // it has been initialised (-1 means "not yet").
        let saved = nfs_niothreads.swap(-1, Relaxed);
        nfs_getset_niothreads(false);
        assert_eq!(nfs_niothreads.load(Relaxed), -1);
        nfs_niothreads.store(4, Relaxed);
        nfs_getset_niothreads(false);
        assert_eq!(nfs_niothreads.load(Relaxed), 0);
        nfs_niothreads.store(saved, Relaxed);
    }
}
/* </TESTS> */
