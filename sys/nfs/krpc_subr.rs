/*	$OpenBSD: krpc_subr.c,v 1.40 2025/02/16 16:05:07 bluhm Exp $	*/
/*	$NetBSD: krpc_subr.c,v 1.12.4.1 1996/06/07 00:52:26 cgd Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1995 Gordon Ross, Adam Glass
 * Copyright (c) 1992 Regents of the University of California.
 * All rights reserved.
 *
 * This software was developed by the Computer Systems Engineering group
 * at Lawrence Berkeley Laboratory under DARPA contract BG 91-66 and
 * contributed to Berkeley.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *	This product includes software developed by the University of
 *	California, Lawrence Berkeley Laboratory and its contributors.
 * 4. Neither the name of the University nor the names of its contributors
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
 * partially based on:
 *      libnetboot/rpc.c
 *               @(#) Header: rpc.c,v 1.12 93/09/28 08:31:56 leres Exp  (LBL)
 */
/* </LICENSES> */

/* <CODE> */
//! `nfs/krpc_subr.c` and `nfs/krpc.h`: kernel support for Sun RPC, used for bootstrapping in
//! NFS diskless configurations: a one-shot UDP RPC client (`krpc_call`, with the
//! portmapper lookup `krpc_portmap` on top of it) and the XDR encoders and decoders of the
//! boot protocols (strings and Internet addresses). `krpc.h` is the prototypes of these
//! functions, plus the portmapper and bootparamd program numbers, which are here.
//!
//! Upstream: sys/nfs/krpc_subr.c @ 3ce1f3f79392
//! Upstream: sys/nfs/krpc.h @ 3ce1f3f79392 (the constants and the prototypes)
//!
//! The file is compiled only with `option NFSCLIENT` (`sys/conf/files`): the module is behind
//! the `nfsclient` feature.
//!
//! ## Deviations
//! - `krpc_portmap` returns the port (network order, as the C stores it through `portp`);
//!   `xdr_string_encode`/`xdr_inaddr_encode` return `None` for the C's `NULL` (too big, no
//!   memory: the C's `M_WAIT` allocations cannot fail, here the pools cannot sleep).
//! - `krpc_call` takes the request body out of `*data` (it is always freed, the C leaves it
//!   to the caller on the failures before the header is made and leaves `*data` dangling on
//!   the later ones) and puts the reply there on success; `sa` is a `&SockaddrIn`; the
//!   broadcast address `from_p` is an `Option<&mut Option<&'static Mbuf>>`. A failed
//!   `socreate` returns at once: the C goes to `out:` and `soclose`s the uninitialised
//!   `so`. The reply mbuf is freed on the error paths after the reply arrived, where the C
//!   leaks it. `retries` of -1 loops "forever" as in C.
//! - `xdr_string_encode(str, len)` takes the string as a slice (`len = str.len()`) and zeroes
//!   the XDR padding (the C leaves the mbuf's old bytes there). `xdr_string_decode(m, str,
//!   len_p)` copies at most `min(*len_p, str.len() - 1)` bytes and NUL-terminates, so it
//!   cannot write past `str`.
//! - The wire structures (`struct rpc_call`, the reply header) are read and built as arrays of
//!   raw words through `m_copydata` and a `#[repr(C)]` `RpcCall`; the reply union's members
//!   are word offsets.
//! - `krpc_get_xid`'s `static struct idgen32_ctx` is a `StaticCell` under a private mutex
//!   (`krpc_xid_mtx`) and `called` an `AtomicBool`.
//! - `krpc_call`'s `static u_int32_t xid` is a local (the C only ever assigns it right before
//!   use).

use core::sync::atomic::{AtomicBool, Ordering};
use core::{mem, ptr};

use libkern::StaticCell;

use crate::crypto::idgen::{Idgen32Ctx, idgen32, idgen32_init};
use crate::kassert;
use crate::kern::init_main::PROC0;
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::subr_prf::{panic, printf};
use crate::kern::uipc_mbuf::{
    m_adj, m_calchdrlen, m_copydata, m_copym, m_free, m_freem, m_get, m_gethdr, m_pullup,
};
use crate::kern::uipc_socket::{sobind, soclose, socreate, soreceive, sosend, sosetopt};
use crate::kern::uipc_socket2::{solock_shared, sounlock_shared};
use crate::machine::copy::AbiPod;
use crate::machine::intr::IPL_NONE;
use crate::netinet::in_::{
    INADDR_ANY, IP_PORTRANGE, IP_PORTRANGE_DEFAULT, IP_PORTRANGE_LOW, IPPROTO_IP, IPPROTO_UDP,
    InAddr, SockaddrIn,
};
use crate::nfs::rpcv2::{RPC_REPLY, RPCAUTH_MAXSIZ, RPCAUTH_UNIX};
use crate::nfs::xdr_subs::{fxdr_unsigned, txdr_unsigned, xdr_bytes};
use crate::sys::errno::Errno;
use crate::sys::mbuf::{
    M_COPYALL, M_EXT, M_PKTHDR, M_WAIT, MCLBYTES, MLEN, MT_DATA, MT_SONAME, MT_SOOPTS, Mbuf,
    mclget, mtod,
};
use crate::sys::mutex::Mutex;
use crate::sys::socket::{AF_INET, SO_BROADCAST, SO_RCVTIMEO, SOCK_DGRAM, SOL_SOCKET};
use crate::sys::time::Timeval;
use crate::sys::uio::{Uio, UioRw, UioSeg};

/// `PMAPPORT`: RPC definitions for the portmapper.
pub const PMAPPORT: u16 = 111;
/// `PMAPPROG`.
pub const PMAPPROG: u32 = 100000;
/// `PMAPVERS`.
pub const PMAPVERS: u32 = 2;
/// `PMAPPROC_NULL`.
pub const PMAPPROC_NULL: u32 = 0;
/// `PMAPPROC_SET`.
pub const PMAPPROC_SET: u32 = 1;
/// `PMAPPROC_UNSET`.
pub const PMAPPROC_UNSET: u32 = 2;
/// `PMAPPROC_GETPORT`.
pub const PMAPPROC_GETPORT: u32 = 3;
/// `PMAPPROC_DUMP`.
pub const PMAPPROC_DUMP: u32 = 4;
/// `PMAPPROC_CALLIT`.
pub const PMAPPROC_CALLIT: u32 = 5;

/// `BOOTPARAM_PROG`: RPC definitions for bootparamd.
pub const BOOTPARAM_PROG: u32 = 100026;
/// `BOOTPARAM_VERS`.
pub const BOOTPARAM_VERS: u32 = 1;
/// `BOOTPARAM_WHOAMI`.
pub const BOOTPARAM_WHOAMI: u32 = 1;
/// `BOOTPARAM_GETFILE`.
pub const BOOTPARAM_GETFILE: u32 = 2;

/// `MIN_REPLY_HDR`: xid, dir, astat, errno.
const MIN_REPLY_HDR: usize = 16;

/// `MAX_RESEND_DELAY`: what is the longest we will wait before re-sending a request? Note
/// this is also the frequency of "RPC timeout" messages. The re-send loop count sup linearly
/// to this maximum, so the first complaint will happen after (1+2+3+4+5)=15 seconds.
const MAX_RESEND_DELAY: i32 = 5; // seconds

/// `struct rpc_call`: the generic RPC call header, with AUTH_UNIX credentials (`struct
/// auth_unix`: time, a null hostname, uid, gid and a null gid list) and an AUTH_NULL verifier,
/// every member a raw XDR word.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct RpcCall {
    /// `rp_xid`: request transaction id.
    rp_xid: u32,
    /// `rp_direction`: call direction (0).
    rp_direction: u32,
    /// `rp_rpcvers`: rpc version (2).
    rp_rpcvers: u32,
    /// `rp_prog`: program.
    rp_prog: u32,
    /// `rp_vers`: version.
    rp_vers: u32,
    /// `rp_proc`: procedure.
    rp_proc: u32,
    /// `rpc_auth.authtype`: auth type.
    auth_authtype: u32,
    /// `rpc_auth.authlen`: auth length.
    auth_authlen: u32,
    /// `rpc_unix`: `ua_time`, `ua_hostname`, `ua_uid`, `ua_gid`, `ua_gidlist`, all zero.
    rpc_unix: [u32; 5],
    /// `rpc_verf.authtype`.
    verf_authtype: u32,
    /// `rpc_verf.authlen`.
    verf_authlen: u32,
}

// SAFETY: `#[repr(C)]` of `u32`s only: no padding, every bit pattern valid.
unsafe impl AbiPod for RpcCall {}

/// The words of `struct rpc_reply`: `rp_xid`, `rp_direction`, `rp_astatus`, then the union:
/// `rp_errno`, or `rp_auth.authtype`, `rp_auth.authlen` and `rp_status`.
const REPLY_XID: usize = 0;
const REPLY_DIRECTION: usize = 1;
const REPLY_ASTATUS: usize = 2;
const REPLY_ERRNO: usize = 3;
const REPLY_AUTHTYPE: usize = 3;
const REPLY_AUTHLEN: usize = 4;
const REPLY_STATUS: usize = 5;
/// `sizeof(struct rpc_reply)`.
const RPC_REPLY_SIZE: usize = 6 * 4;

/// `struct xdr_inaddr`: an address type word and four words, one per byte.
const XDR_INADDR_SIZE: usize = 5 * 4;

/// The raw XDR word `i` at the start of the chain `m` (`m_copydata`: the chain must hold it).
fn reply_word(m: &Mbuf, i: usize) -> u32 {
    let mut b = [0u8; 4];
    m_copydata(m, (4 * i) as i32, &mut b);
    u32::from_ne_bytes(b)
}

/// `krpc_xid_ctx`: the state of the transaction id generator, touched only under
/// [`KRPC_XID_MTX`].
static KRPC_XID_CTX: StaticCell<Idgen32Ctx> = StaticCell::new(Idgen32Ctx::zeroed());
/// Protects [`KRPC_XID_CTX`] (the module's deviations).
static KRPC_XID_MTX: Mutex = Mutex::new(IPL_NONE);
/// `called`: the generator has been initialised.
static KRPC_XID_CALLED: AtomicBool = AtomicBool::new(false);

/// `krpc_get_xid()`: returns an unpredictable XID.
pub fn krpc_get_xid() -> u32 {
    mtx_enter(&KRPC_XID_MTX);
    // SAFETY: KRPC_XID_CTX is touched only here, with KRPC_XID_MTX held; no other reference
    // to it exists while this one lives.
    let ctx = unsafe { KRPC_XID_CTX.get_mut() };
    if !KRPC_XID_CALLED.swap(true, Ordering::Relaxed) {
        idgen32_init(ctx);
    }
    let xid = idgen32(ctx);
    mtx_leave(&KRPC_XID_MTX);
    xid
}

/// `krpc_portmap(sin, prog, vers, portp)`: calls the portmapper at `sin` to look up the port
/// number (network order) of a particular RPC program, over UDP. The portmapper's own port is
/// fixed and needs no call. `sin_port` is set to the portmapper's port.
pub fn krpc_portmap(sin: &mut SockaddrIn, prog: u32, vers: u32) -> Result<u16, Errno> {
    // The portmapper port is fixed.
    if prog == PMAPPROG {
        return Ok(PMAPPORT.to_be());
    }

    let Some(m) = m_get(M_WAIT, MT_DATA) else {
        return Err(Errno::ENOBUFS);
    };
    // struct sdata: the program, the version, the protocol and a port (unused), all words.
    let sdata: [u32; 4] = [
        txdr_unsigned(prog),
        txdr_unsigned(vers),
        txdr_unsigned(IPPROTO_UDP as u32),
        0,
    ];
    // SAFETY: a fresh mbuf's data area holds `MLEN >= 16` bytes.
    unsafe { ptr::copy_nonoverlapping(sdata.as_ptr().cast::<u8>(), mtod::<u8>(m), 16) };
    m.m_len().set(16);

    // Do the RPC to get it.
    sin.sin_port = PMAPPORT.to_be();
    let mut data = Some(m);
    krpc_call(
        sin,
        PMAPPROG,
        PMAPVERS,
        PMAPPROC_GETPORT,
        &mut data,
        None,
        -1,
    )?;
    let Some(mut m) = data else {
        return Err(Errno::EBADRPC);
    };

    // struct rdata: a 16-bit pad and the 16-bit port (the XDR word of the port).
    if (m.m_len().get() as usize) < 4 {
        m = m_pullup(m, 4).ok_or(Errno::ENOBUFS)?;
    }
    let mut rdata = [0u8; 4];
    m_copydata(m, 0, &mut rdata);
    let port = u16::from_ne_bytes([rdata[2], rdata[3]]);

    m_freem(m);
    Ok(port)
}

/// `krpc_call(sa, prog, vers, func, data, from_p, retries)`: does a remote procedure call
/// (RPC) over UDP and waits for its reply. The request body is `*data` (taken); on success
/// `*data` is the reply body, after the RPC header. If `from_p` is given, then we are doing
/// broadcast, and the address from whence the response came is saved there.
///
/// The request is sent from a reserved port (some NFS servers refuse requests from
/// non-privileged ports), and sent again after an increasing delay, up to `retries` times
/// (`-1`: until it is answered); after `MAX_RESEND_DELAY` seconds of delay it complains
/// about each retry. `ETIMEDOUT` when the retries run out.
pub fn krpc_call(
    sa: &SockaddrIn,
    prog: u32,
    vers: u32,
    func: u32,
    data: &mut Option<&'static Mbuf>,
    from_p: Option<&mut Option<&'static Mbuf>>,
    retries: i32,
) -> Result<(), Errno> {
    let body = data.take();

    // Validate address family. Sorry, this is INET specific...
    if sa.sin_family != AF_INET {
        m_freem(body);
        return Err(Errno::EAFNOSUPPORT);
    }

    // Create socket and set its receive timeout.
    let so = match socreate(i32::from(AF_INET), SOCK_DGRAM, 0) {
        Ok(so) => so,
        Err(e) => {
            m_freem(body);
            return Err(e);
        }
    };

    // Free at end if not null. The header mbuf is made first so that a failure anywhere
    // frees the body with it.
    let mut nam: Option<&'static Mbuf> = None;
    let mut from: Option<&'static Mbuf> = None;
    let mhead = match m_gethdr(M_WAIT, MT_DATA) {
        Some(m) => {
            m.m_next().set(body);
            Some(m)
        }
        None => {
            // The pool cannot sleep (the module's deviations).
            m_freem(body);
            None
        }
    };

    let broadcast = from_p.is_some();
    let r = krpc_exchange(
        so, sa, prog, vers, func, broadcast, retries, &mut nam, mhead, &mut from,
    );

    let ret = match r {
        Ok(reply) => {
            // result
            *data = Some(reply);
            if let Some(from_p) = from_p {
                *from_p = from.take();
            }
            Ok(())
        }
        Err(e) => Err(e),
    };

    // out:
    m_freem(nam);
    m_freem(mhead);
    m_freem(from);
    let _ = soclose(so, 0);
    ret
}

/// An mbuf (type `MT_SOOPTS`) holding `v`, for `sosetopt`.
fn sockopt_mbuf<T: Copy>(v: T) -> Result<&'static Mbuf, Errno> {
    let m = m_get(M_WAIT, MT_SOOPTS).ok_or(Errno::ENOBUFS)?;
    kassert!(size_of::<T>() <= MLEN);
    // SAFETY: a fresh mbuf's data area holds `MLEN >= size_of::<T>()` bytes; the write is
    // unaligned.
    unsafe { ptr::write_unaligned(mtod::<T>(m), v) };
    m.m_len().set(size_of::<T>() as u32);
    Ok(m)
}

/// `sosetopt` with a value, the option mbuf freed after (`m_freem(m)` in the C).
fn setopt<T: Copy>(
    so: &'static crate::sys::socketvar::Socket,
    level: i32,
    name: i32,
    v: T,
) -> Result<(), Errno> {
    let m = sockopt_mbuf(v)?;
    let r = sosetopt(so, level, name, Some(m));
    m_freem(m);
    r
}

/// The body of `krpc_call` between creating the socket and `out:`: sets the socket up, sends
/// the request, repeatedly, and returns the reply with its RPC header stripped.
#[allow(clippy::too_many_arguments)] // krpc_call's state, split
fn krpc_exchange(
    so: &'static crate::sys::socketvar::Socket,
    sa: &SockaddrIn,
    prog: u32,
    vers: u32,
    func: u32,
    broadcast: bool,
    retries: i32,
    nam: &mut Option<&'static Mbuf>,
    mhead: Option<&'static Mbuf>,
    from: &mut Option<&'static Mbuf>,
) -> Result<&'static Mbuf, Errno> {
    let Some(mhead) = mhead else {
        return Err(Errno::ENOBUFS);
    };

    setopt(
        so,
        SOL_SOCKET,
        SO_RCVTIMEO,
        Timeval {
            tv_sec: 1,
            tv_usec: 0,
        },
    )?;

    // Enable broadcast if necessary.
    if broadcast {
        setopt(so, SOL_SOCKET, SO_BROADCAST, 1i32)?;
    }

    // Bind the local endpoint to a reserved port, because some NFS servers refuse requests
    // from non-reserved (non-privileged) ports.
    setopt(so, IPPROTO_IP, IP_PORTRANGE, IP_PORTRANGE_LOW)?;

    let m = m_get(M_WAIT, MT_SONAME).ok_or(Errno::ENOBUFS)?;
    let any = SockaddrIn {
        sin_len: size_of::<SockaddrIn>() as u8,
        sin_family: AF_INET,
        sin_port: 0u16.to_be(),
        sin_addr: InAddr { s_addr: INADDR_ANY },
        ..SockaddrIn::default()
    };
    // SAFETY: a fresh mbuf's data area holds `MLEN >= size_of::<SockaddrIn>()` bytes.
    unsafe { ptr::write_unaligned(mtod::<SockaddrIn>(m), any) };
    m.m_len().set(u32::from(any.sin_len));
    solock_shared(so);
    let error = sobind(so, m, &PROC0);
    sounlock_shared(so);
    m_freem(m);
    if let Err(e) = error {
        printf(format_args!("bind failed\n"));
        return Err(e);
    }

    setopt(so, IPPROTO_IP, IP_PORTRANGE, IP_PORTRANGE_DEFAULT)?;

    // Setup socket address for the server.
    let n = m_get(M_WAIT, MT_SONAME).ok_or(Errno::ENOBUFS)?;
    *nam = Some(n);
    let len = usize::from(sa.sin_len).min(size_of::<SockaddrIn>());
    // SAFETY: a fresh mbuf's data area holds `MLEN >= size_of::<SockaddrIn>()` bytes; the
    // first `len <= size_of::<SockaddrIn>()` of them are the address.
    unsafe {
        mtod::<u8>(n).write_bytes(0, size_of::<SockaddrIn>());
        ptr::copy_nonoverlapping(ptr::from_ref(sa).cast::<u8>(), mtod::<u8>(n), len);
    }
    n.m_len().set(len as u32);

    // Prepend RPC message header.
    let xid = krpc_get_xid();
    let call = RpcCall {
        rp_xid: txdr_unsigned(xid),
        // rp_direction = 0
        rp_rpcvers: txdr_unsigned(2),
        rp_prog: txdr_unsigned(prog),
        rp_vers: txdr_unsigned(vers),
        rp_proc: txdr_unsigned(func),
        // rpc_auth part (auth_unix as root)
        auth_authtype: txdr_unsigned(RPCAUTH_UNIX),
        auth_authlen: txdr_unsigned(size_of::<[u32; 5]>() as u32),
        // rpc_verf part (auth_null) is zero.
        ..RpcCall::default()
    };
    let call_bytes = xdr_bytes(&call);
    // SAFETY: a header mbuf's data area holds `MHLEN >= size_of::<RpcCall>()` bytes.
    unsafe {
        ptr::copy_nonoverlapping(call_bytes.as_ptr(), mtod::<u8>(mhead), call_bytes.len());
    }
    mhead.m_len().set(call_bytes.len() as u32);

    // Setup packet header
    m_calchdrlen(mhead);
    mhead.m_pkthdr().ph_ifidx.set(0);

    // Send it, repeatedly, until a reply is received, but delay each re-send by an
    // increasing amount. If the delay hits the maximum, start complaining.
    let mut retries = retries;
    let mut timo = 0;
    let mut m: Option<&'static Mbuf> = None;
    while retries != 0 {
        // Send RPC request (or re-send).
        let Some(copy) = m_copym(mhead, 0, M_COPYALL, M_WAIT) else {
            return Err(Errno::ENOBUFS);
        };
        if let Err(e) = sosend(so, *nam, None, Some(copy), None, 0) {
            printf(format_args!("krpc_call: sosend: {}\n", e.as_i32()));
            return Err(e);
        }

        // Determine new timeout.
        if timo < MAX_RESEND_DELAY {
            timo += 1;
        } else {
            let a = sa.sin_addr.s_addr;
            let b = a.to_ne_bytes();
            printf(format_args!(
                "RPC timeout for server {}.{}.{}.{} (0x{:x}) prog {}\n",
                b[0],
                b[1],
                b[2],
                b[3],
                u32::from_be(a),
                prog
            ));
        }

        // Wait for up to timo seconds for a reply. The socket receive timeout was set to 1
        // second.
        let mut secs = timo;
        while secs > 0 {
            m_freem(from.take());
            m_freem(m.take());

            let mut auio = Uio {
                uio_iov: &mut [],
                uio_offset: 0,
                uio_resid: 1 << 16,
                uio_segflg: UioSeg::UIO_SYSSPACE,
                uio_rw: UioRw::UIO_READ,
                uio_procp: None,
            };
            let len = auio.uio_resid;
            let mut rcvflg = 0;
            match soreceive(
                so,
                Some(&mut *from),
                &mut auio,
                Some(&mut m),
                None,
                Some(&mut rcvflg),
                0,
            ) {
                Err(Errno::EWOULDBLOCK) => {
                    secs -= 1;
                    continue;
                }
                Err(e) => {
                    m_freem(m.take());
                    return Err(e);
                }
                Ok(()) => {}
            }
            let len = len - auio.uio_resid;
            let Some(rm) = m else {
                continue;
            };

            // Does the reply contain at least a header?
            if len < MIN_REPLY_HDR {
                continue;
            }
            if (rm.m_len().get() as usize) < MIN_REPLY_HDR {
                continue;
            }

            // Is it the right reply?
            if reply_word(rm, REPLY_DIRECTION) != txdr_unsigned(RPC_REPLY) {
                continue;
            }

            if reply_word(rm, REPLY_XID) != txdr_unsigned(xid) {
                continue;
            }

            // Was RPC accepted? (authorization OK)
            if reply_word(rm, REPLY_ASTATUS) != 0 {
                let e = fxdr_unsigned(reply_word(rm, REPLY_ERRNO));
                printf(format_args!("rpc denied, error={}\n", e as i32));
                continue;
            }

            // Did the call succeed? (A reply too short to have the status word is not
            // looked into: `strip_reply` refuses it.)
            if rm.m_pkthdr().len.get() as usize >= RPC_REPLY_SIZE
                && reply_word(rm, REPLY_STATUS) != 0
            {
                let e = fxdr_unsigned(reply_word(rm, REPLY_STATUS));
                printf(format_args!("rpc denied, status={}\n", e as i32));
                continue;
            }

            // gotreply:
            return strip_reply(m.take());
        } // while secs
        retries = retries.wrapping_sub(1);
    } // forever send/receive

    m_freem(m.take());
    Err(Errno::ETIMEDOUT)
}

/// `gotreply:` of `krpc_call`: gets the RPC reply header into the first mbuf, gets its length
/// and strips it off. Frees the reply on failure.
fn strip_reply(m: Option<&'static Mbuf>) -> Result<&'static Mbuf, Errno> {
    let Some(mut m) = m else {
        return Err(Errno::EBADRPC);
    };
    let mut len = RPC_REPLY_SIZE as i64;
    kassert!(m.m_flags().get() & M_PKTHDR != 0);
    if i64::from(m.m_pkthdr().len.get()) < len {
        m_freem(m);
        return Err(Errno::EBADRPC);
    }
    if (m.m_len().get() as usize) < RPC_REPLY_SIZE {
        m = m_pullup(m, RPC_REPLY_SIZE as i32).ok_or(Errno::ENOBUFS)?;
    }
    if reply_word(m, REPLY_AUTHTYPE) != 0 {
        let authlen = fxdr_unsigned(reply_word(m, REPLY_AUTHLEN)) as i32;
        if authlen < 0 || authlen as usize > RPCAUTH_MAXSIZ {
            m_freem(m);
            return Err(Errno::EBADRPC);
        }
        len += i64::from((authlen + 3) & !3); // XXX?
    }
    if len < 0 || i64::from(m.m_pkthdr().len.get()) < len {
        m_freem(m);
        return Err(Errno::EBADRPC);
    }
    m_adj(m, len as i32);
    Ok(m)
}

/// `xdr_string_encode(str, len)`: the XDR representation of a string for an RPC: its length
/// (without NUL or padding) and its bytes padded to a word boundary, in a new mbuf (a
/// cluster when it does not fit); `None` when it is too big for a cluster or there is no
/// memory.
pub fn xdr_string_encode(str: &[u8]) -> Option<&'static Mbuf> {
    let len = str.len();
    let dlen = (len + 3) & !3; // padded string length
    let mlen = dlen + 4; // message length

    if mlen > MCLBYTES {
        // If too big, we just can't do it.
        return None;
    }

    let m = m_get(M_WAIT, MT_DATA)?;
    if mlen > MLEN {
        mclget(m, M_WAIT);
        if m.m_flags().get() & M_EXT == 0 {
            let _ = m_free(m); // There can be only one.
            return None;
        }
    }
    let p = mtod::<u8>(m);
    m.m_len().set(mlen as u32);
    // SAFETY: `m` holds `mlen` bytes of data area (an mbuf's `MLEN`, or a cluster's
    // `MCLBYTES`, checked above); the length word, the string and its padding (written
    // zero) are exactly those `mlen` bytes.
    unsafe {
        p.cast::<u32>().write_unaligned(txdr_unsigned(len as u32));
        ptr::copy_nonoverlapping(str.as_ptr(), p.add(4), len);
        p.add(4 + len).write_bytes(0, dlen - len);
    }
    Some(m)
}

/// `xdr_string_decode(m, str, len_p)`: takes the XDR string at the head of the chain `m` (a
/// packet header chain) into `str` as a NUL-terminated string, at most `*len_p` bytes of it
/// (and at most what `str` holds), sets `*len_p` to its length and returns the chain with the
/// string, and its padding, trimmed off. `None` (the chain freed) when the chain is too short
/// or the length is absurd.
pub fn xdr_string_decode(
    m: &'static Mbuf,
    str: &mut [u8],
    len_p: &mut usize,
) -> Option<&'static Mbuf> {
    let mut m = m;
    let mut mlen = size_of::<u32>() as i64; // message length
    kassert!(m.m_flags().get() & M_PKTHDR != 0);
    if i64::from(m.m_pkthdr().len.get()) < mlen {
        m_freem(m);
        return None;
    }
    if (m.m_len().get() as i64) < mlen {
        m = m_pullup(m, mlen as i32)?;
    }
    let slen = i64::from(fxdr_unsigned(reply_word(m, 0)) as i32); // string length
    if slen < 0 || slen > i64::from(i32::MAX) - 3 - mlen {
        m_freem(m);
        return None;
    }
    mlen += (slen + 3) & !3;

    if str.is_empty() {
        panic(format_args!("xdr_string_decode: no room for a string"));
    }
    let mut slen = slen as usize;
    slen = slen.min(*len_p).min(str.len() - 1);
    if i64::from(m.m_pkthdr().len.get()) < mlen {
        m_freem(m);
        return None;
    }
    m_copydata(m, 4, &mut str[..slen]);
    m_adj(m, mlen as i32);

    str[slen] = 0;
    *len_p = slen;

    Some(m)
}

/// `xdr_inaddr_encode(ia)`: the XDR representation of an Internet address in an RPC message
/// (really four ints, NOT chars; blech): the type word 1 and one word per byte of the
/// address.
pub fn xdr_inaddr_encode(ia: &InAddr) -> Option<&'static Mbuf> {
    let m = m_get(M_WAIT, MT_DATA)?;
    let b = ia.s_addr.to_ne_bytes();
    let words: [u32; 5] = [
        txdr_unsigned(1),
        txdr_unsigned(u32::from(b[0])),
        txdr_unsigned(u32::from(b[1])),
        txdr_unsigned(u32::from(b[2])),
        txdr_unsigned(u32::from(b[3])),
    ];
    // SAFETY: a fresh mbuf's data area holds `MLEN >= XDR_INADDR_SIZE` bytes.
    unsafe {
        ptr::copy_nonoverlapping(words.as_ptr().cast::<u8>(), mtod::<u8>(m), XDR_INADDR_SIZE);
    }
    m.m_len().set(XDR_INADDR_SIZE as u32);
    Some(m)
}

/// `xdr_inaddr_decode(m, ia)`: takes the XDR Internet address at the head of the chain `m`
/// into `ia` (`INADDR_ANY` if the type word is not 1) and returns the chain with it trimmed
/// off; `None` (the chain freed) when the chain is too short.
pub fn xdr_inaddr_decode(m: &'static Mbuf, ia: &mut InAddr) -> Option<&'static Mbuf> {
    let mut m = m;
    if (m.m_len().get() as usize) < XDR_INADDR_SIZE {
        m = m_pullup(m, XDR_INADDR_SIZE as i32)?;
    }
    if reply_word(m, 0) != txdr_unsigned(1) {
        ia.s_addr = INADDR_ANY;
    } else {
        let mut b = [0u8; 4];
        for (i, byte) in b.iter_mut().enumerate() {
            *byte = fxdr_unsigned(reply_word(m, 1 + i)) as u8;
        }
        ia.s_addr = u32::from_ne_bytes(b);
    }
    m_adj(m, XDR_INADDR_SIZE as i32);
    Some(m)
}

const _: () = {
    assert!(size_of::<RpcCall>() == 15 * 4);
    assert!(size_of::<RpcCall>() <= crate::sys::mbuf::MHLEN);
    assert!(mem::align_of::<RpcCall>() == 4);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
pub(crate) mod tests {
    // Host tests for `krpc_subr.c`: the XDR string and address encoders and decoders (including
    // across mbuf boundaries), the portmapper shortcut, `krpc_get_xid` and the early failures of
    // `krpc_call`.

    use std::vec::Vec;

    use super::*;
    use crate::kern::uipc_mbuf::m_copyback;
    use crate::kern::uipc_mbuf::tests::setup;
    use crate::nfs::nfs_subs::tests::{bytes, lens};
    use crate::sys::mbuf::M_DONTWAIT;

    /// A packet header chain holding `parts`, one mbuf each.
    pub(crate) fn pkt(parts: &[&[u8]]) -> &'static Mbuf {
        let mut head: Option<&'static Mbuf> = None;
        let mut tail: Option<&'static Mbuf> = None;
        for p in parts {
            let m = if head.is_none() {
                m_gethdr(M_DONTWAIT, MT_DATA)
            } else {
                m_get(M_DONTWAIT, MT_DATA)
            }
            .expect("an mbuf");
            m.m_len().set(p.len() as u32);
            m_copyback(m, 0, p, M_DONTWAIT).expect("copyback");
            match tail {
                Some(t) => t.m_next().set(Some(m)),
                None => head = Some(m),
            }
            tail = Some(m);
        }
        let head = head.expect("at least one part");
        m_calchdrlen(head);
        head
    }

    #[test]
    fn string_encode_pads_to_a_word() {
        let _g = setup();
        let m = xdr_string_encode(b"/export/root").expect("mbuf");
        assert_eq!(
            bytes(m),
            [
                0, 0, 0, 12, b'/', b'e', b'x', b'p', b'o', b'r', b't', b'/', b'r', b'o', b'o', b't'
            ]
        );
        m_freem(m);

        let m = xdr_string_encode(b"abcde").expect("mbuf");
        assert_eq!(
            bytes(m),
            [0, 0, 0, 5, b'a', b'b', b'c', b'd', b'e', 0, 0, 0]
        );
        m_freem(m);

        let m = xdr_string_encode(b"").expect("mbuf");
        assert_eq!(bytes(m), [0, 0, 0, 0]);
        m_freem(m);
    }

    #[test]
    fn string_encode_uses_a_cluster_when_it_does_not_fit_and_refuses_when_too_big() {
        let _g = setup();
        let long = [b'x'; 300];
        let m = xdr_string_encode(&long).expect("mbuf");
        assert!(m.m_flags().get() & M_EXT != 0);
        assert_eq!(lens(m), [304]);
        assert_eq!(&bytes(m)[..4], [0, 0, 1, 44]);
        m_freem(m);

        // The largest that fits a cluster, and one more.
        let max = std::vec![b'y'; MCLBYTES - 4];
        let m = xdr_string_encode(&max).expect("mbuf");
        assert_eq!(lens(m), [MCLBYTES]);
        m_freem(m);
        let big = std::vec![b'y'; MCLBYTES - 3];
        assert!(xdr_string_encode(&big).is_none());
    }

    #[test]
    fn string_decode_takes_the_string_and_trims_the_chain() {
        let _g = setup();
        // "abcde" padded, then "tail", split across mbufs in the middle of the string.
        let m = pkt(&[
            &[0, 0, 0, 5, b'a', b'b'],
            &[b'c', b'd', b'e', 0, 0, 0, b't', b'a', b'i', b'l'],
        ]);
        let mut buf = [0xffu8; 16];
        let mut len = 15;
        let m = xdr_string_decode(m, &mut buf, &mut len).expect("decoded");
        assert_eq!(len, 5);
        assert_eq!(&buf[..6], b"abcde\0");
        assert_eq!(bytes(m), b"tail");
        assert_eq!(m.m_pkthdr().len.get(), 4);
        m_freem(m);

        // A string longer than the room: truncated and terminated, but the whole string is
        // skipped.
        let m = pkt(&[&[0, 0, 0, 5, b'a', b'b', b'c', b'd', b'e', 0, 0, 0, 9]]);
        let mut buf = [0xffu8; 16];
        let mut len = 3;
        let m = xdr_string_decode(m, &mut buf, &mut len).expect("decoded");
        assert_eq!((len, &buf[..4]), (3, &b"abc\0"[..]));
        assert_eq!(bytes(m), [9]);
        m_freem(m);

        // The buffer's own size bounds it too.
        let m = pkt(&[&[0, 0, 0, 5, b'a', b'b', b'c', b'd', b'e', 0, 0, 0]]);
        let mut buf = [0xffu8; 3];
        let mut len = 100;
        let m = xdr_string_decode(m, &mut buf, &mut len).expect("decoded");
        assert_eq!((len, &buf[..]), (2, &b"ab\0"[..]));
        m_freem(m);
    }

    #[test]
    fn string_decode_refuses_short_and_absurd_chains() {
        let _g = setup();
        let mut buf = [0u8; 8];
        let mut len = 7;
        // Not even a length word.
        assert!(xdr_string_decode(pkt(&[&[0, 0]]), &mut buf, &mut len).is_none());
        // A length that runs past the end of the chain.
        assert!(xdr_string_decode(pkt(&[&[0, 0, 0, 9, b'a', b'b']]), &mut buf, &mut len).is_none());
        // A length that overflows an int.
        assert!(
            xdr_string_decode(
                pkt(&[&[0xff, 0xff, 0xff, 0xfe, 1, 2, 3, 4]]),
                &mut buf,
                &mut len
            )
            .is_none()
        );
        assert!(
            xdr_string_decode(
                pkt(&[&[0x7f, 0xff, 0xff, 0xff, 1, 2, 3, 4]]),
                &mut buf,
                &mut len
            )
            .is_none()
        );
        // The length word split across two mbufs is pulled up.
        let m = pkt(&[&[0, 0], &[0, 2, b'h', b'i', 0, 0]]);
        let m = xdr_string_decode(m, &mut buf, &mut len).expect("decoded");
        assert_eq!((len, &buf[..3]), (2, &b"hi\0"[..]));
        assert_eq!(m.m_pkthdr().len.get(), 0);
        m_freem(m);
    }

    #[test]
    fn inaddr_encode_is_the_type_and_one_word_per_byte() {
        let _g = setup();
        let m = xdr_inaddr_encode(&InAddr {
            s_addr: u32::from_ne_bytes([10, 0, 2, 15]),
        })
        .expect("mbuf");
        assert_eq!(
            bytes(m),
            [0, 0, 0, 1, 0, 0, 0, 10, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 15]
        );
        m_freem(m);
    }

    #[test]
    fn inaddr_decode_round_trips_and_trims() {
        let _g = setup();
        let ia = InAddr {
            s_addr: u32::from_ne_bytes([192, 168, 1, 77]),
        };
        let enc = xdr_inaddr_encode(&ia).expect("mbuf");
        // More data behind it, in another mbuf, and the address itself split in two.
        let mut v = bytes(enc);
        m_freem(enc);
        let rest = v.split_off(9);
        let m = pkt(&[&v, &rest, &[7, 7]]);
        let mut out = InAddr::default();
        let m = xdr_inaddr_decode(m, &mut out).expect("decoded");
        assert_eq!(out, ia);
        assert_eq!(bytes(m), [7, 7]);
        m_freem(m);

        // Another address type is INADDR_ANY.
        let mut w: Vec<u8> = Vec::new();
        for x in [2u32, 1, 2, 3, 4] {
            w.extend_from_slice(&txdr_unsigned(x).to_ne_bytes());
        }
        let m = pkt(&[&w]);
        let mut out = InAddr { s_addr: 5 };
        let m = xdr_inaddr_decode(m, &mut out).expect("decoded");
        assert_eq!(out.s_addr, INADDR_ANY);
        assert_eq!(lens(m), [0]);
        m_freem(m);

        // Too short: refused (the chain is freed).
        let mut out = InAddr::default();
        assert!(xdr_inaddr_decode(pkt(&[&w[..12]]), &mut out).is_none());
    }

    #[test]
    fn xids_are_not_repeated_and_never_zero() {
        let _g = setup();
        let ids: Vec<u32> = (0..64).map(|_| krpc_get_xid()).collect();
        assert!(ids.iter().all(|&x| x != 0));
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len());
    }

    #[test]
    fn portmap_of_the_portmapper_needs_no_call_and_krpc_call_checks_the_family() {
        let _g = setup();
        let mut sin = SockaddrIn {
            sin_len: 16,
            sin_family: AF_INET,
            ..SockaddrIn::default()
        };
        assert_eq!(
            krpc_portmap(&mut sin, PMAPPROG, PMAPVERS),
            Ok(111u16.to_be())
        );
        assert_eq!(sin.sin_port, 0, "untouched: no call was made");

        // A non-INET address is refused and the body freed.
        let body = pkt(&[&[1, 2, 3, 4]]);
        let mut data = Some(body);
        sin.sin_family = 24;
        assert_eq!(
            krpc_call(&sin, 100003, 2, 0, &mut data, None, 1),
            Err(Errno::EAFNOSUPPORT)
        );
        assert!(data.is_none());
    }

    #[test]
    fn wire_structures_have_the_c_sizes() {
        // struct rpc_call: 6 words + 2 (auth) + 5 (auth_unix) + 2 (verf).
        assert_eq!(size_of::<RpcCall>(), 60);
        let call = RpcCall {
            rp_xid: txdr_unsigned(0x0102_0304),
            rp_rpcvers: txdr_unsigned(2),
            auth_authtype: txdr_unsigned(RPCAUTH_UNIX),
            auth_authlen: txdr_unsigned(20),
            ..RpcCall::default()
        };
        let b = xdr_bytes(&call);
        assert_eq!(&b[..12], [1, 2, 3, 4, 0, 0, 0, 0, 0, 0, 0, 2]);
        assert_eq!(&b[24..32], [0, 0, 0, 1, 0, 0, 0, 20]);
        assert!(b[32..].iter().all(|&x| x == 0));
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn constants_match_the_c_header() {
        let defs = crate::reftest::defines("sys/nfs/krpc.h");
        for (name, value) in [
            ("PMAPPORT", i64::from(PMAPPORT)),
            ("PMAPPROG", i64::from(PMAPPROG)),
            ("PMAPVERS", i64::from(PMAPVERS)),
            ("PMAPPROC_NULL", i64::from(PMAPPROC_NULL)),
            ("PMAPPROC_SET", i64::from(PMAPPROC_SET)),
            ("PMAPPROC_UNSET", i64::from(PMAPPROC_UNSET)),
            ("PMAPPROC_GETPORT", i64::from(PMAPPROC_GETPORT)),
            ("PMAPPROC_DUMP", i64::from(PMAPPROC_DUMP)),
            ("PMAPPROC_CALLIT", i64::from(PMAPPROC_CALLIT)),
            ("BOOTPARAM_PROG", i64::from(BOOTPARAM_PROG)),
            ("BOOTPARAM_VERS", i64::from(BOOTPARAM_VERS)),
            ("BOOTPARAM_WHOAMI", i64::from(BOOTPARAM_WHOAMI)),
            ("BOOTPARAM_GETFILE", i64::from(BOOTPARAM_GETFILE)),
        ] {
            assert_eq!(crate::reftest::int(&defs, name), Some(value), "{name}");
        }
    }
}
/* </TESTS> */
