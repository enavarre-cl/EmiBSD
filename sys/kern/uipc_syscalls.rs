/*	$OpenBSD: uipc_syscalls.c,v 1.229 2026/09/04 02:13:45 dlg Exp $	*/
/*	$NetBSD: uipc_syscalls.c,v 1.19 1996/02/09 19:00:48 christos Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1982, 1986, 1989, 1990, 1993
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
 *	@(#)uipc_syscalls.c	8.4 (Berkeley) 2/21/94
 */
/* </LICENSES> */

//! The socket system calls: `kern/uipc_syscalls.c`.
//!
//! Upstream: sys/kern/uipc_syscalls.c @ 3ce1f3f79392
//!
//! `socket(2)`, `socketpair(2)`, `bind(2)`, `listen(2)`, `accept(2)`/`accept4(2)`,
//! `connect(2)`, the send calls (`sendto`, `sendmsg`, `sendmmsg`, all through `sendit`), the
//! receive calls (`recvfrom`, `recvmsg`, `recvmmsg`, through `recvit`), `shutdown(2)`,
//! `setsockopt(2)`/`getsockopt(2)`, `getsockname(2)`/`getpeername(2)`, the routing table of
//! the process (`setrtable(2)`, `getrtable(2)`) and `ypconnect(2)`. `sockargs` copies a
//! socket address or control data in from user space into an mbuf; `getsock` finds the
//! socket of a descriptor.
//!
//! ## Deviations
//! - `struct msghdr`'s `msg_iov` stays the user's pointer: `sendit` and `recvit` take the
//!   kernel copy of the iovecs as a separate slice (the C swaps the pointer back and forth
//!   around the call). The user's `struct msghdr`/`struct mmsghdr` are copied in and out
//!   field by field (they have padding).
//! - `pool_get(PR_WAITOK)`, `m_get(M_WAIT)` and `mallocarray(M_WAITOK)` can fail here (see
//!   `subr_pool.rs`): `ENOBUFS` for an mbuf, `ENOMEM` for an iovec array.
//! - `sys_ypconnect`'s binding file name is built on the stack (`MAXPATHLEN`), where the C
//!   takes a `namei_pool` buffer.
//! - `KTRACE` is not configured (`ktrsockaddr`, `ktrmsghdr`, `ktriovec`, `ktrgenio`,
//!   `ktrfds`, `ktrcmsghdr`); `INET6` is not configured (`dns_portcheck`'s `AF_INET6`).
//! - `KERNEL_LOCK()`/`KERNEL_UNLOCK()` are nothing without `MULTIPROCESSOR`.

use core::mem::offset_of;
use core::ptr::NonNull;
use core::slice;
use core::sync::atomic::Ordering;

use crate::kern::kern_descrip::{closef, falloc, fd_getfile, fdinsert, fdremove};
use crate::kern::kern_event::knote;
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_malloc::{free, mallocarray};
use crate::kern::kern_pledge::{pledge_fail, pledge_sendit, pledge_socket, pledge_sockopt};
use crate::kern::kern_prot::suser;
use crate::kern::kern_sig::ptsignal;
use crate::kern::kern_sysctl::{DOMAINNAME, DOMAINNAMELEN};
use crate::kern::kern_tc::getnanotime;
use crate::kern::sys_socket::{SOCKETOPS, fp_socket};
use crate::kern::uipc_mbuf::{m_free, m_freem, m_get, m_getclr};
use crate::kern::uipc_socket::{
    sobind, soclose, soconnect, soconnect2, socreate, sogetopt, solisten, soreceive, sosend,
    sosetopt, soshutdown,
};
use crate::kern::uipc_socket2::{
    solock, solock_nonet, solock_shared, soqremque, sosleep_nsec, sounlock, sounlock_nonet,
    sounlock_shared,
};
use crate::kern::vfs_lookup::{namei, ndinit};
use crate::kern::vfs_subr::vput;
use crate::kern::vfs_vops::{VOP_ADVLOCK, VOP_GETATTR, VOP_READ};
use crate::machine::copy::{copyin, copyin_obj, copyout};
use crate::net::rtable::rtable_exists;
use crate::netinet::in_::{IPPORT_RESERVED, InAddr, SockaddrIn};
use crate::netinet::in_pcb::{INP_LOWPORT, sotoinpcb};
use crate::sys::errno::Errno;
use crate::sys::fcntl::{F_GETLK, F_POSIX, F_UNLCK, F_WRLCK, FNONBLOCK, FREAD, FWRITE, Flock};
use crate::sys::file::{DTYPE_SOCKET, File, frele};
use crate::sys::filedesc::{UF_EXCLOSE, UF_FORKCLOSE, fdplock, fdpunlock};
use crate::sys::limits::{SSIZE_MAX, UCHAR_MAX};
use crate::sys::malloc::{M_IOV, M_WAITOK};
use crate::sys::mbuf::{
    M_EXT, M_WAIT, MCLBYTES, MLEN, MT_CONTROL, MT_SONAME, MT_SOOPTS, Mbuf, mclget, mtod,
};
use crate::sys::namei::{KERNELPATH, LOCKLEAF, NOFOLLOW, NiDirp};
use crate::sys::param::{MAXPATHLEN, PCATCH, PSOCK, align};
use crate::sys::pledge::{PLEDGE_DNS, PLEDGE_RPATH};
use crate::sys::proc::{PS_CHROOT, PS_PLEDGE, Proc};
use crate::sys::protosw::{pru_peeraddr, pru_sockaddr};
use crate::sys::signal::SIGPIPE;
use crate::sys::signalvar::SignalType;
use crate::sys::socket::{
    AF_INET, AF_INET6, Cmsghdr, MSG_CTRUNC, MSG_DONTWAIT, MSG_NOSIGNAL, MSG_OOB, MSG_WAITFORONE,
    Mmsghdr, Msghdr, SOCK_CLOEXEC, SOCK_CLOFORK, SOCK_DGRAM, SOCK_DNS, SOCK_NONBLOCK,
    SOCK_NONBLOCK_INHERIT, SOCK_STREAM, SOCK_TYPE_MASK, Sockaddr, cmsg_align,
};
use crate::sys::socketvar::{
    SS_CANTRCVMORE, SS_DNS, SS_ISCONNECTED, SS_ISCONNECTING, SS_YP, Socket,
};
use crate::sys::syscallargs::{
    SysAccept4Args, SysAcceptArgs, SysBindArgs, SysConnectArgs, SysGetpeernameArgs,
    SysGetsocknameArgs, SysGetsockoptArgs, SysListenArgs, SysRecvfromArgs, SysRecvmmsgArgs,
    SysRecvmsgArgs, SysSendmmsgArgs, SysSendmsgArgs, SysSendtoArgs, SysSetrtableArgs,
    SysSetsockoptArgs, SysShutdownArgs, SysSocketArgs, SysSocketpairArgs, SysYpconnectArgs,
};
use crate::sys::syslimits::IOV_MAX;
use crate::sys::systm::{INFSLP, SysArgs, sysargs};
use crate::sys::time::{Timespec, timespecadd, timespecsub};
use crate::sys::types::{Register, Socklen};
use crate::sys::uio::{Iovec, UIO_SMALLIOV, Uio, UioRw, UioSeg};
use crate::sys::unistd::SEEK_SET;
use crate::sys::vnode::{VREG, Vattr};

/// The size of `struct msghdr` in user space.
const MSGHDR_SIZE: usize = size_of::<Msghdr>();
/// The size of `struct mmsghdr` in user space.
const MMSGHDR_SIZE: usize = size_of::<Mmsghdr>();

/// `struct ypbinding`: what `ypbind(8)` writes to `/var/yp/binding/<domain>.2` (packed).
#[derive(Clone, Copy, Debug, Default)]
struct Ypbinding {
    /// `ypbind_port`.
    _ypbind_port: u16,
    /// `status`.
    _status: i32,
    /// `in`: the server's address, network order.
    r#in: u32,
    /// `ypserv_udp_port`: network order.
    ypserv_udp_port: u16,
    /// `garbage`.
    _garbage: u16,
    /// `ypserv_tcp_port`: network order.
    ypserv_tcp_port: u16,
}

impl Ypbinding {
    /// `sizeof data` (`__packed`).
    const SIZE: usize = 16;

    /// The structure from its packed bytes.
    fn from_bytes(b: &[u8; Self::SIZE]) -> Self {
        let u16_at = |o: usize| u16::from_ne_bytes([b[o], b[o + 1]]);
        let u32_at = |o: usize| u32::from_ne_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
        Self {
            _ypbind_port: u16_at(0),
            _status: u32_at(2) as i32,
            r#in: u32_at(6),
            ypserv_udp_port: u16_at(10),
            _garbage: u16_at(12),
            ypserv_tcp_port: u16_at(14),
        }
    }
}

/// A `struct msghdr` from its user-space bytes.
fn msghdr_from_bytes(b: &[u8]) -> Msghdr {
    let usize_at = |o: usize| {
        let mut w = [0u8; size_of::<usize>()];
        w.copy_from_slice(&b[o..o + size_of::<usize>()]);
        usize::from_ne_bytes(w)
    };
    let u32_at = |o: usize| u32::from_ne_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
    Msghdr {
        msg_name: core::ptr::without_provenance_mut(usize_at(offset_of!(Msghdr, msg_name))),
        msg_namelen: u32_at(offset_of!(Msghdr, msg_namelen)),
        msg_iov: core::ptr::without_provenance_mut(usize_at(offset_of!(Msghdr, msg_iov))),
        msg_iovlen: u32_at(offset_of!(Msghdr, msg_iovlen)),
        msg_control: core::ptr::without_provenance_mut(usize_at(offset_of!(Msghdr, msg_control))),
        msg_controllen: u32_at(offset_of!(Msghdr, msg_controllen)),
        msg_flags: u32_at(offset_of!(Msghdr, msg_flags)) as i32,
    }
}

/// The user-space bytes of a `struct msghdr`, the padding zeroed.
fn msghdr_to_bytes(m: &Msghdr, b: &mut [u8]) {
    b[..MSGHDR_SIZE].fill(0);
    let mut put = |o: usize, v: &[u8]| b[o..o + v.len()].copy_from_slice(v);
    put(
        offset_of!(Msghdr, msg_name),
        &(m.msg_name as usize).to_ne_bytes(),
    );
    put(
        offset_of!(Msghdr, msg_namelen),
        &m.msg_namelen.to_ne_bytes(),
    );
    put(
        offset_of!(Msghdr, msg_iov),
        &(m.msg_iov as usize).to_ne_bytes(),
    );
    put(offset_of!(Msghdr, msg_iovlen), &m.msg_iovlen.to_ne_bytes());
    put(
        offset_of!(Msghdr, msg_control),
        &(m.msg_control as usize).to_ne_bytes(),
    );
    put(
        offset_of!(Msghdr, msg_controllen),
        &m.msg_controllen.to_ne_bytes(),
    );
    put(offset_of!(Msghdr, msg_flags), &m.msg_flags.to_ne_bytes());
}

/// `copyin(uaddr, &msg, sizeof(msg))` of a `struct msghdr`.
fn msghdr_copyin(uaddr: usize) -> Result<Msghdr, Errno> {
    let mut b = [0u8; MSGHDR_SIZE];
    copyin(uaddr, &mut b)?;
    Ok(msghdr_from_bytes(&b))
}

/// `copyin` of a `struct mmsghdr`.
fn mmsghdr_copyin(uaddr: usize) -> Result<Mmsghdr, Errno> {
    let mut b = [0u8; MMSGHDR_SIZE];
    copyin(uaddr, &mut b)?;
    let off = offset_of!(Mmsghdr, msg_len);
    Ok(Mmsghdr {
        msg_hdr: msghdr_from_bytes(&b),
        msg_len: u32::from_ne_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]]),
    })
}

/// `copyout` of a `struct mmsghdr`.
fn mmsghdr_copyout(m: &Mmsghdr, uaddr: usize) -> Result<(), Errno> {
    let mut b = [0u8; MMSGHDR_SIZE];
    msghdr_to_bytes(&m.msg_hdr, &mut b);
    let off = offset_of!(Mmsghdr, msg_len);
    b[off..off + 4].copy_from_slice(&m.msg_len.to_ne_bytes());
    copyout(&b, uaddr)
}

/// The iovec array of a message: `aiov` when `n` fit (`UIO_SMALLIOV`), else a
/// `mallocarray(M_IOV)` array returned with its allocation for [`iovs_free`].
fn iovs_alloc(
    n: usize,
    aiov: &mut [Iovec; UIO_SMALLIOV],
) -> Result<(&mut [Iovec], Option<NonNull<u8>>), Errno> {
    if n <= UIO_SMALLIOV {
        return Ok((&mut aiov[..n], None));
    }
    let Some(mem) = mallocarray(n, size_of::<Iovec>(), M_IOV, M_WAITOK) else {
        return Err(Errno::ENOMEM);
    };
    let iov = mem.cast::<Iovec>().as_ptr();
    for i in 0..n {
        // SAFETY: a fresh allocation of `n` iovecs, suitably aligned.
        unsafe { iov.add(i).write(Iovec::new()) };
    }
    // SAFETY: as above, initialised; ours until `iovs_free`.
    Ok((unsafe { slice::from_raw_parts_mut(iov, n) }, Some(mem)))
}

/// Frees what [`iovs_alloc`] allocated for `n` iovecs.
fn iovs_free(mem: Option<NonNull<u8>>, n: usize) {
    if let Some(mem) = mem {
        free(mem, M_IOV, n * size_of::<Iovec>());
    }
}

/// `copyin(uiov, iov, n * sizeof(struct iovec))`.
fn iovs_copyin(uiov: usize, iov: &mut [Iovec]) -> Result<(), Errno> {
    for (i, slot) in iov.iter_mut().enumerate() {
        let mut bytes = [0u8; Iovec::SIZE];
        copyin(uiov + i * Iovec::SIZE, &mut bytes)?;
        *slot = Iovec::from_bytes(&bytes);
    }
    Ok(())
}

/// `socket(2)`.
pub fn sys_socket(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysSocketArgs = sysargs(v);
    let fdp = p.fd();
    let mut type_ = uap.r#type.get();
    let domain = uap.domain.get();
    let mut ss = 0;

    if type_ & SOCK_DNS != 0 && !(domain == i32::from(AF_INET) || domain == i32::from(AF_INET6)) {
        return Err(Errno::EINVAL);
    }

    if type_ & SOCK_DNS != 0 {
        ss |= SS_DNS;
    }
    pledge_socket(p, domain, ss)?;

    type_ &= !(SOCK_CLOEXEC | SOCK_CLOFORK | SOCK_NONBLOCK | SOCK_DNS);
    let fdflags = (if uap.r#type.get() & SOCK_CLOEXEC != 0 {
        UF_EXCLOSE
    } else {
        0
    }) | (if uap.r#type.get() & SOCK_CLOFORK != 0 {
        UF_FORKCLOSE
    } else {
        0
    });
    let nonblock = uap.r#type.get() & SOCK_NONBLOCK != 0;
    let fflag = FREAD | FWRITE | if nonblock { FNONBLOCK } else { 0 };

    let so = socreate(domain, type_, uap.protocol.get())?;

    fdplock(fdp);
    match falloc(p) {
        Err(error) => {
            fdpunlock(fdp);
            let _ = soclose(so, MSG_DONTWAIT);
            Err(error)
        }
        Ok((fp, fd)) => {
            fp.f_flag.store(fflag as u32, Ordering::SeqCst);
            fp.f_type.set(DTYPE_SOCKET);
            fp.f_ops.set(Some(&SOCKETOPS));
            so.set_state(ss);
            fp.f_data.set(core::ptr::from_ref(so).cast_mut().cast());
            fdinsert(fdp, fd, fdflags, fp);
            fdpunlock(fdp);
            let _ = frele(fp, p);
            retval[0] = fd as Register;
            Ok(())
        }
    }
}

/// `isdnssocket(so)`: created with `SOCK_DNS`.
fn isdnssocket(so: &Socket) -> bool {
    so.has_state(SS_DNS)
}

/// For `SS_DNS` sockets, only allow port DNS (port 53).
fn dns_portcheck(p: &Proc, so: &Socket, nam: &[u8], namelen: usize) -> Result<(), Errno> {
    let mut error = Err(Errno::EINVAL);

    if so.dom_family() == i32::from(AF_INET)
        && namelen >= size_of::<SockaddrIn>()
        && nam.get(offset_of!(SockaddrIn, sin_port)..offset_of!(SockaddrIn, sin_port) + 2)
            == Some(&53u16.to_be_bytes()[..])
    {
        error = Ok(());
    }
    // INET6: not configured.
    if error.is_err() && p.process().ps_flags.load(Ordering::Relaxed) & PS_PLEDGE != 0 {
        return Err(pledge_fail(p, Errno::EPERM, PLEDGE_DNS));
    }
    error
}

/// The bytes of an mbuf's data.
fn mbuf_bytes(m: &Mbuf) -> &[u8] {
    // SAFETY: an mbuf holds `m_len` bytes of data at `m_data`, valid while it is borrowed.
    unsafe { slice::from_raw_parts(mtod::<u8>(m), m.m_len().get() as usize) }
}

/// `bind(2)`.
pub fn sys_bind(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysBindArgs = sysargs(v);

    let fp = getsock(p, uap.s.get())?;
    let so = fp_socket(fp);
    let error = 'out: {
        if so.has_state(SS_YP) {
            break 'out Err(Errno::ENOTSOCK);
        }
        if let Err(e) = pledge_socket(p, so.dom_family(), so.so_state.get()) {
            break 'out Err(e);
        }
        let nam = match sockargs(
            uap.name.get() as usize,
            uap.namelen.get() as usize,
            MT_SONAME,
        ) {
            Ok(nam) => nam,
            Err(e) => break 'out Err(e),
        };
        // KTRACE: not configured.
        solock_shared(so);
        let error = sobind(so, nam, p);
        sounlock_shared(so);
        m_freem(nam);
        error
    };
    let _ = frele(fp, p);
    error
}

/// `listen(2)`.
pub fn sys_listen(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysListenArgs = sysargs(v);

    let fp = getsock(p, uap.s.get())?;
    let so = fp_socket(fp);
    let error = if so.has_state(SS_YP) {
        Err(Errno::ENOTSOCK)
    } else {
        solock_shared(so);
        let error = solisten(so, uap.backlog.get());
        sounlock_shared(so);
        error
    };
    let _ = frele(fp, p);
    error
}

/// `accept(2)`.
pub fn sys_accept(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysAcceptArgs = sysargs(v);

    doaccept(
        p,
        uap.s.get(),
        uap.name.get() as usize,
        uap.anamelen.get() as usize,
        SOCK_NONBLOCK_INHERIT,
        retval,
    )
}

/// `accept4(2)`.
pub fn sys_accept4(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysAccept4Args = sysargs(v);

    if uap.flags.get() & !(SOCK_CLOEXEC | SOCK_CLOFORK | SOCK_NONBLOCK) != 0 {
        return Err(Errno::EINVAL);
    }

    doaccept(
        p,
        uap.s.get(),
        uap.name.get() as usize,
        uap.anamelen.get() as usize,
        uap.flags.get(),
        retval,
    )
}

/// `doaccept(p, sock, name, anamelen, flags, retval)`: takes the first complete connection
/// off the listening socket `sock` (sleeping for one unless non-blocking), gives it a
/// descriptor and copies its peer's address out to `name`.
pub fn doaccept(
    p: &Proc,
    sock: i32,
    name: usize,
    anamelen: usize,
    flags: i32,
    retval: &mut [Register; 2],
) -> Result<(), Errno> {
    let fdp = p.fd();
    let mut namelen: Socklen = 0;

    let fdflags = (if flags & SOCK_CLOEXEC != 0 {
        UF_EXCLOSE
    } else {
        0
    }) | (if flags & SOCK_CLOFORK != 0 {
        UF_FORKCLOSE
    } else {
        0
    });

    if name != 0 {
        namelen = copyin_obj::<i32>(anamelen)? as Socklen;
    }
    let headfp = getsock(p, sock)?;

    fdplock(fdp);
    let alloc = falloc(p);
    fdpunlock(fdp);
    let (fp, tmpfd) = match alloc {
        Ok(r) => r,
        Err(error) => {
            let _ = frele(headfp, p);
            return Err(error);
        }
    };

    let nam = m_get(M_WAIT, MT_SONAME);

    let head = fp_socket(headfp);
    solock_shared(head);

    let error: Result<(), Errno> = 'out: {
        let error: Result<(), Errno> = 'out_unlock: {
            let Some(nam) = nam else {
                break 'out_unlock Err(Errno::ENOBUFS);
            };
            if isdnssocket(head) || !head.has_options(crate::sys::socket::SO_ACCEPTCONN) {
                break 'out_unlock Err(Errno::EINVAL);
            }
            if headfp.flag() & FNONBLOCK != 0 && head.so_qlen.get() == 0 {
                if head.so_rcv.has_state(SS_CANTRCVMORE) {
                    break 'out_unlock Err(Errno::ECONNABORTED);
                } else {
                    break 'out_unlock Err(Errno::EWOULDBLOCK);
                }
            }
            while head.so_qlen.get() == 0 && head.error().is_none() {
                if head.so_rcv.has_state(SS_CANTRCVMORE) {
                    head.set_error(Some(Errno::ECONNABORTED));
                    break;
                }
                if let Err(e) =
                    sosleep_nsec(head, head.timeo_chan(), PSOCK | PCATCH, "netacc", INFSLP)
                {
                    break 'out_unlock Err(e);
                }
            }
            if let Some(e) = head.error() {
                head.set_error(None);
                break 'out_unlock Err(e);
            }

            // Do not sleep after we have taken the socket out of the queue.
            let Some(so) = head.so_q.first() else {
                panic_accept();
            };
            // SAFETY: a socket on the accept queue is a live `socket_pool` item; the
            // listener's lock is held.
            let so: &'static Socket = unsafe { &*core::ptr::from_ref(so) };

            solock_nonet(so);

            if !soqremque(so, 1) {
                panic_accept();
            }

            // Figure out whether the new socket should be non-blocking.
            let nflag = if flags & SOCK_NONBLOCK_INHERIT != 0 {
                headfp.flag() & FNONBLOCK
            } else if flags & SOCK_NONBLOCK != 0 {
                FNONBLOCK
            } else {
                0
            };

            // connection has been removed from the listen queue
            knote(&head.so_rcv.sb_klist, 0);

            sounlock_nonet(head);

            fp.f_type.set(DTYPE_SOCKET);
            fp.f_flag
                .store((FREAD | FWRITE | nflag) as u32, Ordering::SeqCst);
            fp.f_ops.set(Some(&SOCKETOPS));
            fp.f_data.set(core::ptr::from_ref(so).cast_mut().cast());

            let error = crate::kern::uipc_socket::soaccept(so, nam);

            sounlock_shared(so);

            if let Err(e) = error {
                break 'out Err(e);
            }

            if name != 0
                && let Err(e) = copyaddrout(p, nam, name, namelen, anamelen)
            {
                break 'out Err(e);
            }

            fdplock(fdp);
            fdinsert(fdp, tmpfd, fdflags, fp);
            fdpunlock(fdp);
            let _ = frele(fp, p);
            retval[0] = tmpfd as Register;

            m_freem(nam);
            let _ = frele(headfp, p);

            return Ok(());
        };
        // out_unlock:
        sounlock_shared(head);
        error
    };
    // out:
    fdplock(fdp);
    fdremove(fdp, tmpfd);
    fdpunlock(fdp);
    let _ = closef(fp, p);

    m_freem(nam);
    let _ = frele(headfp, p);

    error
}

/// `panic("accept")`: the queue the listener counted is empty.
fn panic_accept() -> ! {
    crate::kern::subr_prf::panic(format_args!("accept"))
}

/// `connect(2)`.
pub fn sys_connect(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysConnectArgs = sysargs(v);
    let mut interrupted = false;

    let fp = getsock(p, uap.s.get())?;
    let so = fp_socket(fp);
    let error: Result<(), Errno> = 'out: {
        if so.has_state(SS_YP) {
            break 'out Err(Errno::ENOTSOCK);
        }
        if let Err(e) = pledge_socket(p, so.dom_family(), so.so_state.get()) {
            break 'out Err(e);
        }
        let nam = match sockargs(
            uap.name.get() as usize,
            uap.namelen.get() as usize,
            MT_SONAME,
        ) {
            Ok(nam) => nam,
            Err(e) => break 'out Err(e),
        };
        // KTRACE: not configured.
        solock_shared(so);
        let error: Result<(), Errno> = 'unlock: {
            if isdnssocket(so)
                && let Err(e) = dns_portcheck(p, so, mbuf_bytes(nam), nam.m_len().get() as usize)
            {
                break 'unlock Err(e);
            }
            if so.has_state(SS_ISCONNECTING) {
                break 'unlock Err(Errno::EALREADY);
            }
            let mut error = soconnect(so, nam);
            'bad: {
                if error.is_err() {
                    break 'bad;
                }
                if fp.flag() & FNONBLOCK != 0 && so.has_state(SS_ISCONNECTING) {
                    break 'unlock Err(Errno::EINPROGRESS);
                }
                while so.has_state(SS_ISCONNECTING) && so.error().is_none() {
                    if let Err(e) =
                        sosleep_nsec(so, so.timeo_chan(), PSOCK | PCATCH, "netcon", INFSLP)
                    {
                        if e == Errno::EINTR || e == Errno::ERESTART {
                            interrupted = true;
                        }
                        error = Err(e);
                        break;
                    }
                }
                if error.is_ok() {
                    if let Some(e) = so.error() {
                        error = Err(e);
                    }
                    so.set_error(None);
                }
            }
            // bad:
            if !interrupted {
                so.clear_state(SS_ISCONNECTING);
            }
            error
        };
        // unlock:
        sounlock_shared(so);
        m_freem(nam);
        error
    };
    // out:
    let _ = frele(fp, p);
    match error {
        Err(Errno::ERESTART) => Err(Errno::EINTR),
        error => error,
    }
}

/// `socketpair(2)`.
pub fn sys_socketpair(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysSocketpairArgs = sysargs(v);
    let fdp = p.fd();
    let utype = uap.r#type.get();

    let type_ = utype & !(SOCK_CLOEXEC | SOCK_CLOFORK | SOCK_NONBLOCK);
    let fdflags = (if utype & SOCK_CLOEXEC != 0 {
        UF_EXCLOSE
    } else {
        0
    }) | (if utype & SOCK_CLOFORK != 0 {
        UF_FORKCLOSE
    } else {
        0
    });
    let nonblock = utype & SOCK_NONBLOCK != 0;
    let fflag = (FREAD | FWRITE | if nonblock { FNONBLOCK } else { 0 }) as u32;

    let so1 = socreate(uap.domain.get(), type_, uap.protocol.get())?;
    let so2 = match socreate(uap.domain.get(), type_, uap.protocol.get()) {
        Ok(so2) => so2,
        Err(error) => {
            // free1:
            let _ = soclose(so1, 0);
            return Err(error);
        }
    };
    let mut so1 = Some(so1);
    let mut so2 = Some(so2);

    let error: Result<(), Errno> = 'free2: {
        let (Some(s1), Some(s2)) = (so1, so2) else {
            break 'free2 Err(Errno::EINVAL);
        };
        if let Err(e) = soconnect2(s1, s2) {
            break 'free2 Err(e);
        }

        if utype & SOCK_TYPE_MASK == SOCK_DGRAM {
            // Datagram socket connection is asymmetric.
            if let Err(e) = soconnect2(s2, s1) {
                break 'free2 Err(e);
            }
        }
        let mut fp1: Option<&'static File> = None;
        let mut fp2: Option<&'static File> = None;
        fdplock(fdp);
        let error: Result<(), Errno> = 'free3: {
            let (f1, sv0) = match falloc(p) {
                Ok(r) => r,
                Err(e) => break 'free3 Err(e),
            };
            fp1 = Some(f1);
            f1.f_flag.store(fflag, Ordering::SeqCst);
            f1.f_type.set(DTYPE_SOCKET);
            f1.f_ops.set(Some(&SOCKETOPS));
            f1.f_data.set(core::ptr::from_ref(s1).cast_mut().cast());
            let (f2, sv1) = match falloc(p) {
                Ok(r) => r,
                Err(e) => {
                    // free4:
                    fdremove(fdp, sv0);
                    break 'free3 Err(e);
                }
            };
            fp2 = Some(f2);
            f2.f_flag.store(fflag, Ordering::SeqCst);
            f2.f_type.set(DTYPE_SOCKET);
            f2.f_ops.set(Some(&SOCKETOPS));
            f2.f_data.set(core::ptr::from_ref(s2).cast_mut().cast());
            let mut sv = [0u8; 2 * size_of::<i32>()];
            sv[..4].copy_from_slice(&sv0.to_ne_bytes());
            sv[4..].copy_from_slice(&sv1.to_ne_bytes());
            match copyout(&sv, uap.rsv.get() as usize) {
                Ok(()) => {
                    fdinsert(fdp, sv0, fdflags, f1);
                    fdinsert(fdp, sv1, fdflags, f2);
                    fdpunlock(fdp);
                    // KTRACE: not configured.
                    let _ = frele(f1, p);
                    let _ = frele(f2, p);
                    return Ok(());
                }
                Err(e) => {
                    fdremove(fdp, sv1);
                    // free4:
                    fdremove(fdp, sv0);
                    break 'free3 Err(e);
                }
            }
        };
        // free3:
        fdpunlock(fdp);

        if let Some(f2) = fp2 {
            let _ = closef(f2, p);
            so2 = None;
        }
        if let Some(f1) = fp1 {
            let _ = closef(f1, p);
            so1 = None;
        }
        error
    };
    // free2:
    if let Some(so2) = so2 {
        let _ = soclose(so2, 0);
    }
    // free1:
    if let Some(so1) = so1 {
        let _ = soclose(so1, 0);
    }
    error
}

/// `sendto(2)`.
pub fn sys_sendto(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysSendtoArgs = sysargs(v);

    let msg = Msghdr {
        msg_name: uap.to.get().cast_mut(),
        msg_namelen: uap.tolen.get(),
        msg_iov: core::ptr::null_mut(),
        msg_iovlen: 1,
        msg_control: core::ptr::null_mut(),
        msg_controllen: 0,
        msg_flags: 0,
    };
    let mut aiov = [Iovec {
        iov_base: uap.buf.get().cast_mut(),
        iov_len: uap.len.get(),
    }];
    sendit(
        p,
        uap.s.get(),
        &msg,
        &mut aiov,
        uap.flags.get(),
        &mut retval[0],
    )
}

/// `sendmsg(2)`.
pub fn sys_sendmsg(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysSendmsgArgs = sysargs(v);
    let mut aiov = [Iovec::new(); UIO_SMALLIOV];

    let mut msg = msghdr_copyin(uap.msg.get() as usize)?;
    // KTRACE: not configured.

    let n = msg.msg_iovlen as usize;
    if n > IOV_MAX {
        return Err(Errno::EMSGSIZE);
    }
    let (iov, mem) = iovs_alloc(n, &mut aiov)?;
    let error = 'done: {
        if n != 0
            && let Err(e) = iovs_copyin(msg.msg_iov as usize, iov)
        {
            break 'done Err(e);
        }
        msg.msg_flags = 0;
        sendit(p, uap.s.get(), &msg, iov, uap.flags.get(), &mut retval[0])
    };
    // done:
    iovs_free(mem, n);
    error
}

/// `sendmmsg(2)`: at most 1024 datagrams; the count sent is returned, an error only when
/// none was.
pub fn sys_sendmmsg(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysSendmmsgArgs = sysargs(v);
    let mut aiov = [Iovec::new(); UIO_SMALLIOV];
    let mut mem: Option<NonNull<u8>> = None;
    let mut iovlen = UIO_SMALLIOV;
    let mut error = Ok(());

    let s = uap.s.get();
    let flags = uap.flags.get();

    // Arbitrarily capped at 1024 datagrams.
    let vlen = uap.vlen.get().min(1024);

    let mmsgp = uap.mmsg.get() as usize;
    let mut dgrams: u32 = 0;
    while dgrams < vlen {
        let mut mmsg = match mmsghdr_copyin(mmsgp + dgrams as usize * MMSGHDR_SIZE) {
            Ok(m) => m,
            Err(e) => {
                error = Err(e);
                break;
            }
        };

        // KTRACE: not configured.

        let n = mmsg.msg_hdr.msg_iovlen as usize;
        if n > IOV_MAX {
            error = Err(Errno::EMSGSIZE);
            break;
        }

        if n > iovlen {
            iovs_free(mem.take(), iovlen);

            iovlen = n;
            match mallocarray(iovlen, size_of::<Iovec>(), M_IOV, M_WAITOK) {
                Some(m) => mem = Some(m),
                None => {
                    iovlen = UIO_SMALLIOV;
                    error = Err(Errno::ENOMEM);
                    break;
                }
            }
        }
        let iov: &mut [Iovec] = match mem {
            Some(m) => {
                let base = m.cast::<Iovec>().as_ptr();
                for i in 0..n {
                    // SAFETY: the allocation holds `iovlen` >= `n` iovecs, suitably aligned.
                    unsafe { base.add(i).write(Iovec::new()) };
                }
                // SAFETY: as above, initialised; ours until freed below.
                unsafe { slice::from_raw_parts_mut(base, n) }
            }
            None => &mut aiov[..n],
        };

        if n > 0
            && let Err(e) = iovs_copyin(mmsg.msg_hdr.msg_iov as usize, iov)
        {
            error = Err(e);
            break;
        }

        // KTRACE: not configured.

        mmsg.msg_hdr.msg_flags = 0;

        let mut retsnd: Register = 0;
        if let Err(e) = sendit(p, s, &mmsg.msg_hdr, iov, flags, &mut retsnd) {
            error = Err(e);
            break;
        }

        mmsg.msg_len = retsnd as u32;

        if let Err(e) = mmsghdr_copyout(&mmsg, mmsgp + dgrams as usize * MMSGHDR_SIZE) {
            error = Err(e);
            break;
        }
        dgrams += 1;
    }

    iovs_free(mem, iovlen);

    retval[0] = dgrams as Register;

    if error.is_err() && dgrams > 0 {
        error = Ok(());
    }

    error
}

/// `sendit(p, s, mp, flags, retsize)`: sends the message `mp` (with `iov`, the kernel copy
/// of its iovecs) on the socket `s`; the byte count goes to `retsize`.
pub fn sendit(
    p: &Proc,
    s: i32,
    mp: &Msghdr,
    iov: &mut [Iovec],
    flags: i32,
    retsize: &mut Register,
) -> Result<(), Errno> {
    let mut to: Option<&'static Mbuf> = None;
    let mut flags = flags;

    let fp = getsock(p, s)?;
    let so = fp_socket(fp);
    if fp.flag() & FNONBLOCK != 0 {
        flags |= MSG_DONTWAIT;
    }

    let error: Result<(), Errno> = 'bad: {
        if let Err(e) = pledge_sendit(p, mp.msg_name) {
            break 'bad Err(e);
        }

        let mut resid: usize = 0;
        for v in iov.iter() {
            // Don't allow sum > SSIZE_MAX
            resid = resid.wrapping_add(v.iov_len);
            if v.iov_len > SSIZE_MAX as usize || resid > SSIZE_MAX as usize {
                break 'bad Err(Errno::EINVAL);
            }
        }
        let mut auio = Uio {
            uio_iov: iov,
            uio_offset: 0, // XXX
            uio_resid: resid,
            uio_segflg: UioSeg::UIO_USERSPACE,
            uio_rw: UioRw::UIO_WRITE,
            uio_procp: Some(p),
        };
        if !mp.msg_name.is_null() {
            let t = match sockargs(mp.msg_name as usize, mp.msg_namelen as usize, MT_SONAME) {
                Ok(t) => t,
                Err(e) => break 'bad Err(e),
            };
            to = Some(t);
            if isdnssocket(so)
                && let Err(e) = dns_portcheck(p, so, mbuf_bytes(t), mp.msg_namelen as usize)
            {
                break 'bad Err(e);
            }
            // KTRACE: not configured.
        }
        let control = if !mp.msg_control.is_null() {
            if (mp.msg_controllen as usize) < cmsg_align(size_of::<Cmsghdr>()) {
                break 'bad Err(Errno::EINVAL);
            }
            match sockargs(
                mp.msg_control as usize,
                mp.msg_controllen as usize,
                MT_CONTROL,
            ) {
                Ok(c) => Some(c),
                Err(e) => break 'bad Err(e),
            }
            // KTRACE: not configured.
        } else {
            None
        };
        // KTRACE: not configured.
        let len = auio.uio_resid;
        let mut error = sosend(so, to, Some(&mut auio), None, control, flags);
        if let Err(e) = error {
            if auio.uio_resid != len
                && (e == Errno::ERESTART || e == Errno::EINTR || e == Errno::EWOULDBLOCK)
            {
                error = Ok(());
            }
            if error == Err(Errno::EPIPE) && flags & MSG_NOSIGNAL == 0 {
                ptsignal(p, SIGPIPE, SignalType::STHREAD);
            }
        }
        if error.is_ok() {
            *retsize = (len - auio.uio_resid) as Register;
            mtx_enter(&fp.f_mtx);
            fp.f_wxfer.set(fp.f_wxfer.get() + 1);
            fp.f_wbytes.set(fp.f_wbytes.get() + *retsize as u64);
            mtx_leave(&fp.f_mtx);
        }
        // KTRACE: not configured.
        error
    };
    // bad:
    let _ = frele(fp, p);
    m_freem(to);
    error
}

/// `recvfrom(2)`.
pub fn sys_recvfrom(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysRecvfromArgs = sysargs(v);

    let fromlenaddr = uap.fromlenaddr.get() as usize;
    let namelen = if fromlenaddr != 0 {
        copyin_obj::<i32>(fromlenaddr)? as Socklen
    } else {
        0
    };
    let mut msg = Msghdr {
        msg_name: uap.from.get(),
        msg_namelen: namelen,
        msg_iov: core::ptr::null_mut(),
        msg_iovlen: 1,
        msg_control: core::ptr::null_mut(),
        msg_controllen: 0,
        msg_flags: uap.flags.get(),
    };
    let mut aiov = [Iovec {
        iov_base: uap.buf.get(),
        iov_len: uap.len.get(),
    }];
    recvit(
        p,
        uap.s.get(),
        &mut msg,
        &mut aiov,
        fromlenaddr,
        &mut retval[0],
    )
}

/// `recvmsg(2)`.
pub fn sys_recvmsg(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysRecvmsgArgs = sysargs(v);
    let mut aiov = [Iovec::new(); UIO_SMALLIOV];

    let mut msg = msghdr_copyin(uap.msg.get() as usize)?;

    let n = msg.msg_iovlen as usize;
    if n > IOV_MAX {
        return Err(Errno::EMSGSIZE);
    }
    let (iov, mem) = iovs_alloc(n, &mut aiov)?;
    msg.msg_flags = uap.flags.get();
    let error = 'done: {
        if n > 0
            && let Err(e) = iovs_copyin(msg.msg_iov as usize, iov)
        {
            break 'done Err(e);
        }
        recvit(p, uap.s.get(), &mut msg, iov, 0, &mut retval[0])?;
        // KTRACE: not configured.
        let mut b = [0u8; MSGHDR_SIZE];
        msghdr_to_bytes(&msg, &mut b);
        copyout(&b, uap.msg.get() as usize)
    };
    // done:
    iovs_free(mem, n);
    error
}

/// `recvmmsg(2)`: at most 1024 datagrams, until `timeout` passes; the count received is
/// returned, an error only when none was (the socket keeps it for the next call).
pub fn sys_recvmmsg(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysRecvmmsgArgs = sysargs(v);
    let mut aiov = [Iovec::new(); UIO_SMALLIOV];
    let mut mem: Option<NonNull<u8>> = None;
    let mut iovlen = UIO_SMALLIOV;
    let mut error = Ok(());
    let mut ts = Timespec::new(0, 0);

    let timeout = uap.timeout.get() as usize;
    if timeout != 0 {
        ts = copyin_obj::<Timespec>(timeout)?;
        // KTRACE: not configured.
        if !ts.is_valid() {
            return Err(Errno::EINVAL);
        }

        let now = getnanotime();
        ts = timespecadd(&now, &ts);
    }

    let s = uap.s.get();
    let mut flags = uap.flags.get();

    // Arbitrarily capped at 1024 datagrams.
    let vlen = uap.vlen.get().min(1024);

    let mmsgp = uap.mmsg.get() as usize;
    let mut dgrams: u32 = 0;
    while dgrams < vlen {
        let mut mmsg = match mmsghdr_copyin(mmsgp + dgrams as usize * MMSGHDR_SIZE) {
            Ok(m) => m,
            Err(e) => {
                error = Err(e);
                break;
            }
        };

        let n = mmsg.msg_hdr.msg_iovlen as usize;
        if n > IOV_MAX {
            error = Err(Errno::EMSGSIZE);
            break;
        }

        if n > iovlen {
            iovs_free(mem.take(), iovlen);

            iovlen = n;
            match mallocarray(iovlen, size_of::<Iovec>(), M_IOV, M_WAITOK) {
                Some(m) => mem = Some(m),
                None => {
                    iovlen = UIO_SMALLIOV;
                    error = Err(Errno::ENOMEM);
                    break;
                }
            }
        }
        let iov: &mut [Iovec] = match mem {
            Some(m) => {
                let base = m.cast::<Iovec>().as_ptr();
                for i in 0..n {
                    // SAFETY: the allocation holds `iovlen` >= `n` iovecs, suitably aligned.
                    unsafe { base.add(i).write(Iovec::new()) };
                }
                // SAFETY: as above, initialised; ours until freed below.
                unsafe { slice::from_raw_parts_mut(base, n) }
            }
            None => &mut aiov[..n],
        };

        if n > 0
            && let Err(e) = iovs_copyin(mmsg.msg_hdr.msg_iov as usize, iov)
        {
            error = Err(e);
            break;
        }

        mmsg.msg_hdr.msg_flags = flags & !MSG_WAITFORONE;

        let mut retrec: Register = 0;
        if let Err(e) = recvit(p, s, &mut mmsg.msg_hdr, iov, 0, &mut retrec) {
            error = if e == Errno::EAGAIN && dgrams > 0 {
                Ok(())
            } else {
                Err(e)
            };
            break;
        }

        if flags & MSG_WAITFORONE != 0 {
            flags |= MSG_DONTWAIT;
        }

        mmsg.msg_len = retrec as u32;
        // KTRACE: not configured.

        if let Err(e) = mmsghdr_copyout(&mmsg, mmsgp + dgrams as usize * MMSGHDR_SIZE) {
            error = Err(e);
            break;
        }

        dgrams += 1;
        if mmsg.msg_hdr.msg_flags & MSG_OOB != 0 {
            break;
        }

        if timeout != 0 {
            let now = getnanotime();
            let left = timespecsub(&now, &ts);
            if left.tv_sec > 0 {
                break;
            }
        }
    }

    iovs_free(mem, iovlen);

    retval[0] = dgrams as Register;

    // If we succeeded at least once, return 0, hopefully so->so_error will catch it next
    // time.
    if let Err(e) = error
        && dgrams > 0
    {
        if let Ok(fp) = getsock(p, s) {
            let so = fp_socket(fp);
            so.set_error(Some(e));

            let _ = frele(fp, p);
        }
        error = Ok(());
    }

    error
}

/// `recvit(p, s, mp, namelenp, retsize)`: receives a message on the socket `s` into `mp`
/// (with `iov`, the kernel copy of its iovecs), copying out the sender's address, its
/// length to the user address `namelenp` (0: none) and the control data.
pub fn recvit(
    p: &Proc,
    s: i32,
    mp: &mut Msghdr,
    iov: &mut [Iovec],
    namelenp: usize,
    retsize: &mut Register,
) -> Result<(), Errno> {
    let mut from: Option<&'static Mbuf> = None;
    let mut control: Option<&'static Mbuf> = None;

    let fp = getsock(p, s)?;

    let error: Result<(), Errno> = 'out: {
        let mut resid: usize = 0;
        for v in iov.iter() {
            // Don't allow sum > SSIZE_MAX
            resid = resid.wrapping_add(v.iov_len);
            if v.iov_len > SSIZE_MAX as usize || resid > SSIZE_MAX as usize {
                break 'out Err(Errno::EINVAL);
            }
        }
        let mut auio = Uio {
            uio_iov: iov,
            uio_offset: 0, // XXX
            uio_resid: resid,
            uio_segflg: UioSeg::UIO_USERSPACE,
            uio_rw: UioRw::UIO_READ,
            uio_procp: Some(p),
        };
        // KTRACE: not configured.
        let len = auio.uio_resid;
        if fp.flag() & FNONBLOCK != 0 {
            mp.msg_flags |= MSG_DONTWAIT;
        }
        let want_control = !mp.msg_control.is_null();
        let mut msg_flags = mp.msg_flags;
        let mut error = soreceive(
            fp_socket(fp),
            Some(&mut from),
            &mut auio,
            None,
            if want_control {
                Some(&mut control)
            } else {
                None
            },
            Some(&mut msg_flags),
            if want_control { mp.msg_controllen } else { 0 },
        );
        mp.msg_flags = msg_flags;
        if let Err(e) = error
            && auio.uio_resid != len
            && (e == Errno::ERESTART || e == Errno::EINTR || e == Errno::EWOULDBLOCK)
        {
            error = Ok(());
        }
        // KTRACE: not configured.
        if let Err(e) = error {
            break 'out Err(e);
        }
        *retsize = (len - auio.uio_resid) as Register;
        if !mp.msg_name.is_null() {
            let alen: Socklen = match from {
                None => 0,
                Some(f) => {
                    let alen = f.m_len().get();
                    let n = alen.min(mp.msg_namelen) as usize;
                    if let Err(e) = copyout(&mbuf_bytes(f)[..n], mp.msg_name as usize) {
                        break 'out Err(e);
                    }
                    // KTRACE: not configured.
                    alen
                }
            };
            mp.msg_namelen = alen;
            if namelenp != 0
                && let Err(e) = copyout(&alen.to_ne_bytes(), namelenp)
            {
                break 'out Err(e);
            }
        }
        let mut error = Ok(());
        if want_control {
            let mut clen = mp.msg_controllen as usize;
            match control {
                Some(first) if clen > 0 => {
                    let base = mp.msg_control as usize;
                    let mut cp = base;
                    let mut m = first;

                    loop {
                        let mut i = m.m_len().get() as usize;
                        if clen < i {
                            mp.msg_flags |= MSG_CTRUNC;
                            i = clen;
                        }
                        error = copyout(&mbuf_bytes(m)[..i], cp);
                        // KTRACE: not configured.
                        if m.m_next().get().is_some() {
                            i = align(i);
                            if clen < i {
                                mp.msg_flags |= MSG_CTRUNC;
                                i = clen;
                            }
                        }
                        cp += i;
                        clen -= i;
                        if error.is_err() || clen == 0 {
                            break;
                        }
                        match m.m_next().get() {
                            Some(next) => m = next,
                            None => break,
                        }
                    }
                    clen = cp - base;
                }
                _ => clen = 0,
            }
            mp.msg_controllen = clen as Socklen;
        }
        if error.is_ok() {
            mtx_enter(&fp.f_mtx);
            fp.f_rxfer.set(fp.f_rxfer.get() + 1);
            fp.f_rbytes.set(fp.f_rbytes.get() + *retsize as u64);
            mtx_leave(&fp.f_mtx);
        }
        error
    };
    // out:
    let _ = frele(fp, p);
    m_freem(from);
    m_freem(control);
    error
}

/// `shutdown(2)`.
pub fn sys_shutdown(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysShutdownArgs = sysargs(v);

    let fp = getsock(p, uap.s.get())?;
    let error = soshutdown(fp_socket(fp), uap.how.get());
    let _ = frele(fp, p);
    error
}

/// `setsockopt(2)`.
pub fn sys_setsockopt(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysSetsockoptArgs = sysargs(v);
    let mut m: Option<&'static Mbuf> = None;

    let fp = getsock(p, uap.s.get())?;
    let so = fp_socket(fp);
    let error = 'bad: {
        if let Err(e) = pledge_sockopt(p, true, so.so_proto, uap.level.get(), uap.name.get()) {
            break 'bad Err(e);
        }
        let valsize = uap.valsize.get() as usize;
        if valsize > MCLBYTES {
            break 'bad Err(Errno::EINVAL);
        }
        let val = uap.val.get() as usize;
        if val != 0 {
            let Some(mm) = m_get(M_WAIT, MT_SOOPTS) else {
                break 'bad Err(Errno::ENOBUFS);
            };
            m = Some(mm);
            if valsize > MLEN {
                mclget(mm, M_WAIT);
                if mm.m_flags().get() & M_EXT == 0 {
                    break 'bad Err(Errno::ENOBUFS);
                }
            }
            // SAFETY: the mbuf (or its cluster) holds at least `valsize` bytes (checked).
            let buf = unsafe { slice::from_raw_parts_mut(mtod::<u8>(mm), valsize) };
            if let Err(e) = copyin(val, buf) {
                break 'bad Err(e);
            }
            mm.m_len().set(valsize as u32);
        }
        sosetopt(so, uap.level.get(), uap.name.get(), m)
    };
    // bad:
    m_freem(m);
    let _ = frele(fp, p);
    error
}

/// `getsockopt(2)`.
pub fn sys_getsockopt(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysGetsockoptArgs = sysargs(v);

    let fp = getsock(p, uap.s.get())?;
    let so = fp_socket(fp);
    let error = 'out: {
        if let Err(e) = pledge_sockopt(p, false, so.so_proto, uap.level.get(), uap.name.get()) {
            break 'out Err(e);
        }
        let val = uap.val.get() as usize;
        let avalsize = uap.avalsize.get() as usize;
        let mut valsize: Socklen = 0;
        if val != 0 {
            match copyin_obj::<i32>(avalsize) {
                Ok(v) => valsize = v as Socklen,
                Err(e) => break 'out Err(e),
            }
        }
        let Some(m) = m_get(M_WAIT, MT_SOOPTS) else {
            break 'out Err(Errno::ENOBUFS);
        };
        let mut error = sogetopt(so, uap.level.get(), uap.name.get(), m);
        if error.is_ok() && val != 0 && valsize != 0 {
            if valsize > m.m_len().get() {
                valsize = m.m_len().get();
            }
            error = copyout(&mbuf_bytes(m)[..valsize as usize], val);
            if error.is_ok() {
                error = copyout(&valsize.to_ne_bytes(), avalsize);
            }
        }
        m_free(m);
        error
    };
    // out:
    let _ = frele(fp, p);
    error
}

/// Get socket name.
pub fn sys_getsockname(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysGetsocknameArgs = sysargs(v);
    let mut m: Option<&'static Mbuf> = None;

    let fp = getsock(p, uap.fdes.get())?;
    let so = fp_socket(fp);
    let error = 'out: {
        if so.has_state(SS_YP) {
            break 'out Err(Errno::ENOTSOCK);
        }
        let alen = uap.alen.get() as usize;
        let len = match copyin_obj::<i32>(alen) {
            Ok(len) => len as Socklen,
            Err(e) => break 'out Err(e),
        };
        let Some(mm) = m_getclr(M_WAIT, MT_SONAME) else {
            break 'out Err(Errno::ENOBUFS);
        };
        m = Some(mm);
        solock_shared(so);
        let error = pru_sockaddr(so, mm);
        sounlock_shared(so);
        if let Err(e) = error {
            break 'out Err(e);
        }
        copyaddrout(p, mm, uap.asa.get() as usize, len, alen)
    };
    // out:
    let _ = frele(fp, p);
    m_freem(m);
    error
}

/// Get name of peer for connected socket.
pub fn sys_getpeername(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysGetpeernameArgs = sysargs(v);
    let mut m: Option<&'static Mbuf> = None;

    let fp = getsock(p, uap.fdes.get())?;
    let so = fp_socket(fp);
    let error = 'bad: {
        if so.has_state(SS_YP) {
            break 'bad Err(Errno::ENOTSOCK);
        }
        if !so.has_state(SS_ISCONNECTED) {
            break 'bad Err(Errno::ENOTCONN);
        }
        let alen = uap.alen.get() as usize;
        let len = match copyin_obj::<i32>(alen) {
            Ok(len) => len as Socklen,
            Err(e) => break 'bad Err(e),
        };
        let Some(mm) = m_getclr(M_WAIT, MT_SONAME) else {
            break 'bad Err(Errno::ENOBUFS);
        };
        m = Some(mm);
        solock_shared(so);
        let error = pru_peeraddr(so, mm);
        sounlock_shared(so);
        if let Err(e) = error {
            break 'bad Err(e);
        }
        copyaddrout(p, mm, uap.asa.get() as usize, len, alen)
    };
    // bad:
    let _ = frele(fp, p);
    m_freem(m);
    error
}

/// `sockargs(mp, buf, buflen, type)`: copies a socket address (`MT_SONAME`, whose `sa_len`
/// is set to `buflen`) or control data in from the user address `buf`.
pub fn sockargs(buf: usize, buflen: usize, type_: i32) -> Result<&'static Mbuf, Errno> {
    // We can't allow socket names > UCHAR_MAX in length, since that will overflow sa_len.
    // Also, control data more than MCLBYTES in length is just too much. Memory for sa_len
    // and sa_family must exist.
    if buflen
        > (if type_ == MT_SONAME {
            usize::from(UCHAR_MAX)
        } else {
            MCLBYTES
        })
        || (type_ == MT_SONAME && buflen < offset_of!(Sockaddr, sa_data))
    {
        return Err(Errno::EINVAL);
    }

    // Allocate an mbuf to hold the arguments.
    let Some(m) = m_get(M_WAIT, type_) else {
        return Err(Errno::ENOBUFS);
    };
    if buflen > MLEN {
        mclget(m, M_WAITOK);
        if m.m_flags().get() & M_EXT == 0 {
            m_free(m);
            return Err(Errno::ENOBUFS);
        }
    }
    m.m_len().set(buflen as u32);
    // SAFETY: the mbuf (or its cluster) holds at least `buflen` bytes (checked above).
    let data = unsafe { slice::from_raw_parts_mut(mtod::<u8>(m), buflen) };
    if let Err(error) = copyin(buf, data) {
        let _ = m_free(m);
        return Err(error);
    }
    if type_ == MT_SONAME {
        data[0] = buflen as u8;
    }
    Ok(m)
}

/// `getsock(p, fdes, fpp)`: the socket file of the descriptor `fdes`, with a reference;
/// `ENOTSOCK` for another kind of file.
pub fn getsock(p: &Proc, fdes: i32) -> Result<&'static File, Errno> {
    let Some(fp) = fd_getfile(p.fd(), fdes) else {
        return Err(Errno::EBADF);
    };
    if fp.f_type.get() != DTYPE_SOCKET {
        let _ = frele(fp, p);
        return Err(Errno::ENOTSOCK);
    }

    Ok(fp)
}

/// `setrtable(2)`: moves the process to routing table `rtableid`; only root may leave a
/// table other than 0.
pub fn sys_setrtable(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysSetrtableArgs = sysargs(v);
    let ps_rtableid = p.process().ps_rtableid.load(Ordering::Relaxed);

    let rtableid = uap.rtableid.get();

    if i64::from(ps_rtableid) == i64::from(rtableid) {
        return Ok(());
    }
    if ps_rtableid != 0 {
        suser(p)?;
    }
    if rtableid < 0 || !rtable_exists(rtableid as u32) {
        return Err(Errno::EINVAL);
    }

    p.process()
        .ps_rtableid
        .store(rtableid as u32, Ordering::Relaxed);
    Ok(())
}

/// `getrtable(2)`: the process's routing table.
pub fn sys_getrtable(p: &Proc, _v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    retval[0] = p.process().ps_rtableid.load(Ordering::Relaxed) as i32 as Register;
    Ok(())
}

/// `copyaddrout(p, name, sa, buflen, outlen)`: copies the address in `name` out to `sa`
/// (at most `buflen` bytes) and its full length to `outlen`.
pub fn copyaddrout(
    _p: &Proc,
    name: &Mbuf,
    sa: usize,
    buflen: Socklen,
    outlen: usize,
) -> Result<(), Errno> {
    let namelen: Socklen = name.m_len().get();

    // SHOULD COPY OUT A CHAIN HERE
    copyout(&mbuf_bytes(name)[..buflen.min(namelen) as usize], sa)?;
    // KTRACE: not configured.
    copyout(&namelen.to_ne_bytes(), outlen)
}

/// `ypsockargs(mp, buf, buflen, type)`: `sockargs` from a kernel buffer.
fn ypsockargs(buf: &[u8], type_: i32) -> Result<&'static Mbuf, Errno> {
    let buflen = buf.len();

    // We can't allow socket names > UCHAR_MAX in length, since that will overflow sa_len.
    // Also, control data more than MCLBYTES in length is just too much. Memory for sa_len
    // and sa_family must exist.
    if buflen
        > (if type_ == MT_SONAME {
            usize::from(UCHAR_MAX)
        } else {
            MCLBYTES
        })
        || (type_ == MT_SONAME && buflen < offset_of!(Sockaddr, sa_data))
    {
        return Err(Errno::EINVAL);
    }

    // Allocate an mbuf to hold the arguments.
    let Some(m) = m_get(M_WAIT, type_) else {
        return Err(Errno::ENOBUFS);
    };
    if buflen > MLEN {
        mclget(m, M_WAITOK);
        if m.m_flags().get() & M_EXT == 0 {
            m_free(m);
            return Err(Errno::ENOBUFS);
        }
    }
    m.m_len().set(buflen as u32);
    // SAFETY: the mbuf (or its cluster) holds at least `buflen` bytes (checked above).
    let data = unsafe { slice::from_raw_parts_mut(mtod::<u8>(m), buflen) };
    data.copy_from_slice(buf);
    if type_ == MT_SONAME {
        data[0] = buflen as u8;
    }
    Ok(m)
}

/// `ypconnect(2)`: a socket connected to the YP server `ypbind(8)` found.
pub fn sys_ypconnect(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysYpconnectArgs = sysargs(v);
    let fdp = p.fd();
    let type_ = uap.r#type.get();

    // SAFETY: `domainname` is written only by `kern.domainname` under `sysctl_lock`; this is
    // a read of the bytes the C reads without a lock too.
    let domainname = unsafe { DOMAINNAME.get() };
    let dlen = (DOMAINNAMELEN.load(Ordering::Relaxed).max(0) as usize).min(domainname.len());
    let dname = &domainname[..dlen];
    let dname = &dname[..dname.iter().position(|&c| c == 0).unwrap_or(dname.len())];
    if dname.is_empty() || dname.contains(&b'/') {
        return Err(Errno::EAFNOSUPPORT);
    }

    match type_ {
        SOCK_STREAM | SOCK_DGRAM => {}
        _ => return Err(Errno::EAFNOSUPPORT),
    }

    if p.process().ps_flags.load(Ordering::Relaxed) & PS_CHROOT != 0 {
        return Err(Errno::EACCES);
    }
    // KERNEL_LOCK(): nothing without MULTIPROCESSOR.
    let mut name = [0u8; MAXPATHLEN];
    let prefix = b"/var/yp/binding/";
    let suffix = b".2";
    let mut n = 0;
    for &c in prefix.iter().chain(dname).chain(suffix) {
        if n + 1 >= name.len() {
            break;
        }
        name[n] = c;
        n += 1;
    }
    let mut nid = ndinit(
        0,
        NOFOLLOW | LOCKLEAF | KERNELPATH,
        NiDirp::Sys(&name[..n]),
        p,
    );
    nid.ni_pledge = PLEDGE_RPATH;

    namei(&mut nid)?;
    let Some(vp) = nid.ni_vp else {
        crate::kern::subr_prf::panic(format_args!("sys_ypconnect: namei returned no vnode"));
    };
    let mut data = [0u8; Ypbinding::SIZE];
    let error: Result<(), Errno> = 'verror: {
        let mut va = Vattr::new();
        if let Err(e) = VOP_GETATTR(vp, &mut va, p.p_ucred.get(), p) {
            break 'verror Err(e);
        }
        if vp.v_type.get() != VREG || va.va_size != Ypbinding::SIZE as u64 {
            break 'verror Err(Errno::EFTYPE);
        }

        // Check that a lock is held on the file (hopefully by ypbind), otherwise the file
        // might be old
        let mut fl = Flock {
            l_start: 0,
            l_len: 0,
            l_pid: 0,
            l_type: F_WRLCK,
            l_whence: SEEK_SET as i16,
        };
        if let Err(e) = VOP_ADVLOCK(
            vp,
            core::ptr::from_ref(fdp).cast(),
            F_GETLK,
            &mut fl,
            F_POSIX,
        ) {
            break 'verror Err(e);
        }
        if fl.l_type == F_UNLCK {
            break 'verror Err(Errno::EOWNERDEAD);
        }

        let mut iov = [Iovec {
            iov_base: data.as_mut_ptr().cast(),
            iov_len: data.len(),
        }];
        let mut uio = Uio {
            uio_iov: &mut iov,
            uio_offset: 0,
            uio_resid: Ypbinding::SIZE,
            uio_segflg: UioSeg::UIO_SYSSPACE,
            uio_rw: UioRw::UIO_READ,
            uio_procp: Some(p),
        };

        VOP_READ(vp, &mut uio, 0, p.p_ucred.get())
    };
    vput(vp);
    // KERNEL_UNLOCK()
    error?;
    let data = Ypbinding::from_bytes(&data);

    let mut ypsin = SockaddrIn {
        sin_len: size_of::<SockaddrIn>() as u8,
        sin_family: AF_INET,
        ..SockaddrIn::default()
    };
    ypsin.sin_port = if type_ == SOCK_STREAM {
        data.ypserv_tcp_port
    } else {
        data.ypserv_udp_port
    };
    let port = i32::from(u16::from_be(ypsin.sin_port));
    if port >= IPPORT_RESERVED || port == 20 {
        return Err(Errno::EPERM);
    }
    ypsin.sin_addr = InAddr { s_addr: data.r#in };

    let so = socreate(i32::from(AF_INET), type_, 0)?;

    // SAFETY: `SockaddrIn` is `#[repr(C)]` without padding: its bytes are initialised.
    let sin_bytes = unsafe {
        slice::from_raw_parts(
            core::ptr::from_ref(&ypsin).cast::<u8>(),
            size_of::<SockaddrIn>(),
        )
    };
    let nam = match ypsockargs(sin_bytes, MT_SONAME) {
        Ok(nam) => nam,
        Err(error) => {
            let _ = soclose(so, MSG_DONTWAIT);
            return Err(error);
        }
    };

    // KTRACE: not configured.
    solock(so);

    // Secure YP maps require reserved ports
    if suser(p).is_ok()
        && let Some(inp) = sotoinpcb(so)
    {
        inp.set_flags(INP_LOWPORT);
    }

    let mut error = soconnect(so, nam);
    while so.has_state(SS_ISCONNECTING) && so.error().is_none() {
        error = sosleep_nsec(so, so.timeo_chan(), PSOCK | PCATCH, "ypcon", INFSLP);
        if error.is_err() {
            break;
        }
    }
    m_freem(nam);
    so.set_state(SS_YP); // impose some restrictions
    sounlock(so);
    if let Err(error) = error {
        let _ = soclose(so, MSG_DONTWAIT);
        return Err(error);
    }

    fdplock(fdp);
    let (fp, fd) = match falloc(p) {
        Ok(r) => r,
        Err(error) => {
            fdpunlock(fdp);
            let _ = soclose(so, MSG_DONTWAIT);
            return Err(error);
        }
    };

    fp.f_flag
        .store((FREAD | FWRITE | FNONBLOCK) as u32, Ordering::SeqCst);
    fp.f_type.set(DTYPE_SOCKET);
    fp.f_ops.set(Some(&SOCKETOPS));
    fp.f_data.set(core::ptr::from_ref(so).cast_mut().cast());
    fdinsert(fdp, fd, UF_EXCLOSE, fp);
    fdpunlock(fdp);
    let _ = frele(fp, p);
    retval[0] = fd as Register;
    Ok(())
}
