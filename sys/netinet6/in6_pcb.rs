/*	$OpenBSD: in6_pcb.c,v 1.152 2025/09/16 09:19:16 florian Exp $	*/
/* <LICENSES> */
/*
 * Copyright (C) 1995, 1996, 1997, and 1998 WIDE Project.
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the project nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE PROJECT AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE PROJECT OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 */

/*
 *	@(#)COPYRIGHT	1.1 (NRL) 17 January 1995
 *
 * NRL grants permission for redistribution and use in source and binary
 * forms, with or without modification, of the software and documentation
 * created at NRL provided that the following conditions are met:
 *
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgements:
 *	This product includes software developed by the University of
 *	California, Berkeley and its contributors.
 *	This product includes software developed at the Information
 *	Technology Division, US Naval Research Laboratory.
 * 4. Neither the name of the NRL nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THE SOFTWARE PROVIDED BY NRL IS PROVIDED BY NRL AND CONTRIBUTORS ``AS
 * IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A
 * PARTICULAR PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL NRL OR
 * CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL,
 * EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
 * PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR
 * PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF
 * LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING
 * NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
 * SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 *
 * The views and conclusions contained in the software and documentation
 * are those of the authors and should not be interpreted as representing
 * official policies, either expressed or implied, of the US Naval
 * Research Laboratory (NRL).
 */

/*
 * Copyright (c) 1982, 1986, 1990, 1993, 1995
 *	Regents of the University of California.  All rights reserved.
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
 */
/* </LICENSES> */

//! IPv6 protocol control blocks: binding, connecting, lookups and notifications of
//! `INP_IPV6` pcbs: `netinet6/in6_pcb.c` (prototypes in `<netinet/in_pcb.h>`).
//!
//! Upstream: sys/netinet6/in6_pcb.c @ 3ce1f3f79392
//!
//! The licence block carries the NRL notice with its advertising clause, accepted as
//! BSD-4 (`.claude/rules/scope-and-stubs.md`).
//!
//! `in_pcb.c` is used for inet and inet6; this file only holds the special IPv6 cases: the
//! hash of an IPv6 quadruple, the checks of a local IPv6 address, connecting, the address
//! queries of `getsockname(2)`/`getpeername(2)`, ICMPv6 notifications and the exact and
//! listening lookups. A control block of an `AF_INET6` socket (`INP_IPV6`) uses the
//! `inp_faddr6`/`inp_laddr6` members of `struct inpcb`.
//!
//! ## Deviations
//! - The `struct sockaddr_in6 *` arguments the C modifies in place (`in6_pcbaddrisavail`'s,
//!   the copy `in6_pcbconnect` makes) are `&mut` locals; mbuf data is read with
//!   `read_unaligned` (no alignment guarantee).
//! - `in6_pcbselsrc` returns the address by value, so `in6_pcbconnect` keeps a copy where
//!   the C keeps a pointer.
//! - `in_pcblookup_local_lock` and `in_pcbbind_locked` (`netinet/in_pcb.rs`) still take an
//!   IPv4 local address (the C's `const void *laddr`): their `INET6` branches come with the
//!   INET6 integration of `in_pcb.c`. The two call sites go through
//!   `in_pcblookup_local_lock6` and `in_pcbbind_locked6`, which report the gap with
//!   `unported!` (`ENOSYS`): binding an IPv6 socket to a non-zero port in
//!   `in6_pcbaddrisavail_lock`, and the implicit bind of `in6_pcbconnect`, fail until then.
//! - `in6_pcbnotify` takes `cmdarg` as the C does and, as the C does, does not use it.
//! - `DIAGNOSTIC`'s `in_pcbnotifymiss` messages print the ports and routing domain (the C
//!   prints no addresses either).
//! - `NPF` and `NSTOEPLITZ` are configured: the divert keys of `in6_pcblookup_listen` and
//!   `inp_flowid`.

use core::ffi::c_void;
use core::mem::size_of;
use core::ptr;
#[cfg(feature = "diagnostic")]
use core::sync::atomic::Ordering;

use crate::crypto::siphash::{SipHash24_End, SipHash24_Init, SipHash24_Update, SiphashCtx};
use crate::kassert;
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::subr_prf::panic;
use crate::machine::cpu::curproc;
use crate::net::if_::ifa_ifwithaddr;
use crate::net::pf::pf_find_divert;
use crate::net::pfvar::{PF_DIVERT_REPLY, PF_DIVERT_TO};
use crate::net::route::{RTF_HOST, Rtentry, route6_mpath};
use crate::net::rtable::rtable_l2;
use crate::net::toeplitz::stoeplitz_ip6port;
use crate::netinet::in_pcb::{
    IN_PCBLOCK_GRAB, IN_PCBLOCK_HOLD, INP_IPV6, INPLOOKUP_IPV6, INPLOOKUP_WILDCARD, InpHash,
    InpNotifyFn, Inpcb, InpcbIterator, Inpcbtable, in_pcb_iterator, in_pcbref, in_pcbrehash,
    in_pcbrtchange, in_pcbsolock, in_pcbsounlock, in_pcbunref, sotoinpcb,
};
use crate::netinet::ip6::IPV6_FLOWLABEL_MASK;
use crate::netinet6::in6::{
    IN6ADDR_ANY, IN6ADDR_ANY_INIT, In6Addr, SA6_ANY, SockaddrIn6, ifatoia6, in6_are_addr_equal,
    in6_is_addr_multicast, in6_is_addr_unspecified, in6_is_addr_v4mapped, in6_nam2sin6,
    sin6tosa_const,
};
use crate::netinet6::in6_src::{in6_embedscope, in6_pcbselsrc, in6_recoverscope, in6_selecthlim};
use crate::netinet6::in6_var::{
    IN6_IFF_ANYCAST, IN6_IFF_DETACHED, IN6_IFF_DUPLICATED, IN6_IFF_TENTATIVE,
};
use crate::netinet6::ip6_id::ip6_randomflowlabel;
use crate::netinet6::ip6_input::INET6CTLERRMAP;
use crate::netinet6::ip6_var::{Ip6Moptions, Ip6Pktopts};
use crate::sys::endian::htonl;
use crate::sys::errno::Errno;
use crate::sys::mbuf::{Mbuf, PF_TAG_DIVERTED, PF_TAG_TRANSLATE_LOCALHOST, mtod};
use crate::sys::mutex::mutex_assert_locked;
use crate::sys::proc::Proc;
use crate::sys::protosw::{PRC_HOSTDEAD, PRC_NCMDS, prc_is_redirect};
use crate::sys::queue::ListHead;
use crate::sys::socket::{SO_BINDANY, SO_REUSEADDR, SO_REUSEPORT};
use crate::sys::socketvar::Socket;
use crate::sys::systm::net_assert_locked;

/// `zeroin6_addr`: the unspecified address.
pub static ZEROIN6_ADDR: In6Addr = IN6ADDR_ANY_INIT;

/// A control block found in a table's lists, for as long as the caller keeps it.
fn inp_static(inp: &Inpcb) -> &'static Inpcb {
    // SAFETY: a control block in a table's lists is an `inpcb_pool` item that stays
    // allocated until its last `in_pcbunref`; the callers hold the table mutex while they
    // look and a reference (`in_pcbref`) for as long as they keep it, as in C.
    unsafe { &*ptr::from_ref(inp) }
}

/// `inp->inp_outputopts6` as `ip6_output` and the source selection take it.
pub(crate) fn inp_outputopts6(inp: &Inpcb) -> Option<&Ip6Pktopts> {
    // SAFETY: the options are `ip6_pcbopt`'s allocation, owned by this control block and
    // replaced or freed only under the socket lock, which the callers hold.
    inp.inp_outputopts6.get().map(|o| unsafe { o.as_ref() })
}

/// `inp->inp_moptions6` as `ip6_output` and `in6_embedscope` take it.
pub(crate) fn inp_moptions6(inp: &Inpcb) -> Option<&Ip6Moptions> {
    // SAFETY: the options are `ip6_setmoptions`'s allocation, owned by this control block
    // and freed only under the socket lock, which the callers hold.
    inp.inp_moptions6.get().map(|o| unsafe { o.as_ref() })
}

/// `curproc`, which the socket requests run as.
fn curproc_or_panic(func: &str) -> &'static Proc {
    match curproc() {
        Some(p) => p,
        None => panic(format_args!("{}: no curproc", func)),
    }
}

/// `in6_pcbhash`: the hash of the complete IPv6 address quadruple in routing domain
/// `rdomain`.
pub fn in6_pcbhash(
    table: &Inpcbtable,
    rdomain: u32,
    faddr: &In6Addr,
    fport: u16,
    laddr: &In6Addr,
    lport: u16,
) -> u64 {
    let mut ctx = SiphashCtx::default();
    let nrdom = rdomain.to_be_bytes();

    SipHash24_Init(&mut ctx, &table.inpt_key.get());
    SipHash24_Update(&mut ctx, &nrdom);
    SipHash24_Update(&mut ctx, &faddr.s6_addr);
    SipHash24_Update(&mut ctx, &fport.to_ne_bytes());
    SipHash24_Update(&mut ctx, &laddr.s6_addr);
    SipHash24_Update(&mut ctx, &lport.to_ne_bytes());
    SipHash24_End(&mut ctx)
}

/// `in_pcblookup_local_lock` with an IPv6 local address (the C passes `&sin6->sin6_addr`
/// as its `const void *laddr`, with `INPLOOKUP_IPV6` in `flags`).
///
/// `netinet/in_pcb.rs` takes an `&InAddr` there until the INET6 integration of `in_pcb.c`
/// generalizes it; the gap is reported here.
fn in_pcblookup_local_lock6(
    table: &Inpcbtable,
    laddr: &In6Addr,
    lport: u16,
    flags: i32,
    rtable: u32,
    lock: i32,
) -> Result<Option<&'static Inpcb>, Errno> {
    let _ = (table, laddr, lport, flags, rtable, lock);
    Err(crate::unported!(
        "in_pcblookup_local_lock: IPv6 local address (in_pcb.c INET6)"
    ))
}

/// `in_pcbbind_locked(inp, NULL, laddr, p)` with an IPv6 local address (the C's
/// `const void *laddr`): picks a local port for `inp`.
///
/// `netinet/in_pcb.rs` takes an `&InAddr` there until the INET6 integration of `in_pcb.c`
/// generalizes it; the gap is reported here.
fn in_pcbbind_locked6(inp: &'static Inpcb, laddr: &In6Addr, p: &Proc) -> Result<(), Errno> {
    let _ = (inp, laddr, p);
    Err(crate::unported!(
        "in_pcbbind_locked: IPv6 local address (in_pcb.c INET6)"
    ))
}

/// `in6_pcbaddrisavail_lock`: whether `inp` may bind to `sin6` (an address of ours, not
/// anycast nor unusable, and a port nobody else has, as `wild` and the reuse options
/// allow); `lock` says whether the table mutex is held (`IN_PCBLOCK_HOLD`) or taken here
/// (`IN_PCBLOCK_GRAB`). The scope zone is embedded into `sin6`'s address and its scope id
/// cleared, as the C does in place.
pub fn in6_pcbaddrisavail_lock(
    inp: &Inpcb,
    sin6: &mut SockaddrIn6,
    wild: i32,
    p: &Proc,
    lock: i32,
) -> Result<(), Errno> {
    let so = inp.socket();
    let table = inp.table();
    let lport = sin6.sin6_port;
    let mut reuseport = so.so_options.get() & SO_REUSEPORT;
    let wild = wild | INPLOOKUP_IPV6;
    let _ = p;

    // KAME hack: embed scopeid
    let mut addr = sin6.sin6_addr;
    if in6_embedscope(&mut addr, sin6, inp_outputopts6(inp), inp_moptions6(inp)).is_err() {
        return Err(Errno::EINVAL);
    }
    sin6.sin6_addr = addr;
    // this must be cleared for ifa_ifwithaddr()
    sin6.sin6_scope_id = 0;
    // reject IPv4 mapped address, we have no support for it
    if in6_is_addr_v4mapped(&sin6.sin6_addr) {
        return Err(Errno::EADDRNOTAVAIL);
    }

    if in6_is_addr_multicast(&sin6.sin6_addr) {
        // Treat SO_REUSEADDR as SO_REUSEPORT for multicast; allow complete duplication of
        // binding if SO_REUSEPORT is set, or if SO_REUSEADDR is set and a multicast address
        // is bound on both new and duplicated sockets.
        if so.has_options(SO_REUSEADDR | SO_REUSEPORT) {
            reuseport = SO_REUSEADDR | SO_REUSEPORT;
        }
    } else if !in6_is_addr_unspecified(&sin6.sin6_addr) {
        // Cleared because ifa_ifwithaddr() compares the whole socket address, ports (and
        // flow) included.
        sin6.sin6_port = 0;
        sin6.sin6_flowinfo = 0;
        let mut ifa = None;
        if !so.has_options(SO_BINDANY) {
            // SAFETY: a local `sockaddr_in6`, read for the call.
            ifa = unsafe { ifa_ifwithaddr(sin6tosa_const(sin6), inp.inp_rtableid.get()) };
            if ifa.is_none() {
                return Err(Errno::EADDRNOTAVAIL);
            }
        }
        sin6.sin6_port = lport;

        // Binding to an anycast address might accidentally cause sending a packet with an
        // anycast source address, so we forbid it.
        //
        // We should allow to bind to a deprecated address, since the application dare to
        // use it. But, can we assume that they are careful enough to check if the address
        // is deprecated or not? Maybe, as a safeguard, we should have a setsockopt flag to
        // control the bind(2) behavior against deprecated addresses (default: forbid
        // bind(2)).
        if let Some(ifa) = ifa
            && ifatoia6(ifa).ia6_flags.get()
                & (IN6_IFF_ANYCAST | IN6_IFF_TENTATIVE | IN6_IFF_DUPLICATED | IN6_IFF_DETACHED)
                != 0
        {
            return Err(Errno::EADDRNOTAVAIL);
        }
    }
    if lport != 0 {
        if so.so_euid.get() != 0 && !in6_is_addr_multicast(&sin6.sin6_addr) {
            let t = in_pcblookup_local_lock6(
                table,
                &sin6.sin6_addr,
                lport,
                INPLOOKUP_WILDCARD | INPLOOKUP_IPV6,
                inp.inp_rtableid.get(),
                lock,
            )?;
            let error = t.is_some_and(|t| so.so_euid.get() != t.socket().so_euid.get());
            if lock == IN_PCBLOCK_GRAB {
                in_pcbunref(t);
            }
            if error {
                return Err(Errno::EADDRINUSE);
            }
        }
        let t = in_pcblookup_local_lock6(
            table,
            &sin6.sin6_addr,
            lport,
            wild,
            inp.inp_rtableid.get(),
            lock,
        )?;
        let error = t.is_some_and(|t| reuseport & t.socket().so_options.get() == 0);
        if lock == IN_PCBLOCK_GRAB {
            in_pcbunref(t);
        }
        if error {
            return Err(Errno::EADDRINUSE);
        }
    }
    Ok(())
}

/// `in6_pcbaddrisavail`: `in6_pcbaddrisavail_lock` taking the table mutex.
pub fn in6_pcbaddrisavail(
    inp: &Inpcb,
    sin6: &mut SockaddrIn6,
    wild: i32,
    p: &Proc,
) -> Result<(), Errno> {
    in6_pcbaddrisavail_lock(inp, sin6, wild, p, IN_PCBLOCK_GRAB)
}

/// `in6_pcbconnect`: connects `inp` to the `sockaddr_in6` in `nam`; both address and port
/// must be specified. If the socket has no local address yet, one is picked (and a local
/// port, if it has none either). Eventually, flow labels will have to be dealt with here,
/// as well.
pub fn in6_pcbconnect(inp: &'static Inpcb, nam: &Mbuf) -> Result<(), Errno> {
    let table = inp.table();

    kassert!(inp.has_flags(INP_IPV6));

    let sin6p = in6_nam2sin6(nam)?;
    // protect *sin6 from overwrites: a copy.
    // SAFETY: `in6_nam2sin6` checked the mbuf holds a whole `sockaddr_in6`; mbuf data need
    // not be aligned, so it is read unaligned.
    let mut sin6 = unsafe { sin6p.read_unaligned() };
    if sin6.sin6_port == 0 {
        return Err(Errno::EADDRNOTAVAIL);
    }
    // reject IPv4 mapped address, we have no support for it
    if in6_is_addr_v4mapped(&sin6.sin6_addr) {
        return Err(Errno::EADDRNOTAVAIL);
    }

    // KAME hack: embed scopeid
    let mut addr = sin6.sin6_addr;
    if in6_embedscope(&mut addr, &sin6, inp_outputopts6(inp), inp_moptions6(inp)).is_err() {
        return Err(Errno::EINVAL);
    }
    sin6.sin6_addr = addr;
    // this must be cleared for ifa_ifwithaddr()
    sin6.sin6_scope_id = 0;

    // Source address selection.
    // XXX: in6_selectsrc might replace the bound local address with the address specified
    // by setsockopt(IPV6_PKTINFO). Is it the intended behavior?
    let mut in6a = In6Addr::default();
    in6_pcbselsrc(&mut in6a, &sin6, inp, inp_outputopts6(inp))?;

    let mut ip6 = inp.inp_ipv6.get();
    ip6.ip6_hlim = in6_selecthlim(inp) as u8;
    inp.inp_ipv6.set(ip6);

    // keep lookup, modification, and rehash in sync
    mtx_enter(&table.inpt_mtx);

    let laddr = if in6_is_addr_unspecified(&inp.inp_laddr6.get()) {
        in6a
    } else {
        inp.inp_laddr6.get()
    };
    let t = in6_pcblookup_lock(
        table,
        &sin6.sin6_addr,
        sin6.sin6_port,
        &laddr,
        inp.inp_lport.get(),
        inp.inp_rtableid.get(),
        IN_PCBLOCK_HOLD,
    );
    if t.is_some() {
        mtx_leave(&table.inpt_mtx);
        return Err(Errno::EADDRINUSE);
    }

    kassert!(in6_is_addr_unspecified(&inp.inp_laddr6.get()) || inp.inp_lport.get() != 0);

    if in6_is_addr_unspecified(&inp.inp_laddr6.get()) {
        if inp.inp_lport.get() == 0 {
            if let Err(e) = in_pcbbind_locked6(inp, &in6a, curproc_or_panic("in6_pcbconnect")) {
                mtx_leave(&table.inpt_mtx);
                return Err(e);
            }
            let t = in6_pcblookup_lock(
                table,
                &sin6.sin6_addr,
                sin6.sin6_port,
                &in6a,
                inp.inp_lport.get(),
                inp.inp_rtableid.get(),
                IN_PCBLOCK_HOLD,
            );
            if t.is_some() {
                inp.inp_lport.set(0);
                mtx_leave(&table.inpt_mtx);
                return Err(Errno::EADDRINUSE);
            }
        }
        inp.inp_laddr6.set(in6a);
    }
    inp.inp_faddr6.set(sin6.sin6_addr);
    inp.inp_fport.set(sin6.sin6_port);
    in_pcbrehash(inp);

    mtx_leave(&table.inpt_mtx);

    in6_pcb_newflow(inp);
    Ok(())
}

/// A fresh random flow label for `inp`, and its flow id from the new addresses (the tail
/// `in6_pcbconnect` and `in6_pcbset_addr` share).
fn in6_pcb_newflow(inp: &Inpcb) {
    let flowinfo = (inp.inp_flowinfo() & !IPV6_FLOWLABEL_MASK)
        | (htonl(ip6_randomflowlabel()) & IPV6_FLOWLABEL_MASK);
    inp.set_inp_flowinfo(flowinfo);
    inp.inp_flowid.set(stoeplitz_ip6port(
        &inp.inp_faddr6.get().s6_addr,
        &inp.inp_laddr6.get().s6_addr,
        inp.inp_fport.get(),
        inp.inp_lport.get(),
    ));
}

/// Writes `sin6` as the address in `nam`.
fn set_nam6(nam: &Mbuf, sin6: SockaddrIn6) {
    nam.m_len().set(size_of::<SockaddrIn6>() as u32);
    // SAFETY: `nam` is an `MT_SONAME` mbuf of `MLEN` bytes, more than a `sockaddr_in6`;
    // written unaligned (mbuf data need not be aligned).
    unsafe { mtod::<SockaddrIn6>(nam).write_unaligned(sin6) };
}

/// `in6_setsockaddr`: the local address and port of `inp` into `nam`, as a
/// `sockaddr_in6`. This services the getsockname(2) call.
pub fn in6_setsockaddr(inp: &Inpcb, nam: &Mbuf) {
    let mut sin6 = SockaddrIn6 {
        sin6_port: inp.inp_lport.get(),
        ..SockaddrIn6::with_addr(inp.inp_laddr6.get())
    };
    // KAME hack: recover scopeid
    in6_recoverscope(&mut sin6, &inp.inp_laddr6.get());
    set_nam6(nam, sin6);
}

/// `in6_setpeeraddr`: the foreign address and port of `inp` into `nam`, as a
/// `sockaddr_in6`. This services the getpeername(2) call.
pub fn in6_setpeeraddr(inp: &Inpcb, nam: &Mbuf) {
    let mut sin6 = SockaddrIn6 {
        sin6_port: inp.inp_fport.get(),
        ..SockaddrIn6::with_addr(inp.inp_faddr6.get())
    };
    // KAME hack: recover scopeid
    in6_recoverscope(&mut sin6, &inp.inp_faddr6.get());
    set_nam6(nam, sin6);
}

/// `sotoinpcb(so)` of a socket the C knows to be attached.
fn inpcb_of(so: &Socket) -> &'static Inpcb {
    match sotoinpcb(so) {
        Some(inp) => inp,
        None => panic(format_args!("socket {:p}: no inpcb", so)),
    }
}

/// `in6_sockaddr`: the `pru_sockaddr` of the IPv6 protocols.
pub fn in6_sockaddr(so: &'static Socket, nam: &'static Mbuf) -> Result<(), Errno> {
    in6_setsockaddr(inpcb_of(so), nam);

    Ok(())
}

/// `in6_peeraddr`: the `pru_peeraddr` of the IPv6 protocols.
pub fn in6_peeraddr(so: &'static Socket, nam: &'static Mbuf) -> Result<(), Errno> {
    in6_setpeeraddr(inpcb_of(so), nam);

    Ok(())
}

/// `in6_pcbnotify`: passes control command `cmd` to every connection of `table` associated
/// with address `dst`. The local address and/or port numbers may be specified (`src`,
/// `lport_arg`, `fport_arg`) to limit the search; `src` is `None` for a notification of
/// local fragmentation. The "usual action" will be taken, depending on the ctlinput cmd:
/// redirects and dead-host indications go to every reference to the destination
/// (`in_pcbrtchange` invalidates the route cache of redirected ones). The caller must
/// filter any cmds that are uninteresting (e.g., no error in the map). `notify` reports the
/// errno of `inet6ctlerrmap` to each matching socket.
#[allow(clippy::too_many_arguments)] // the C's signature
pub fn in6_pcbnotify(
    table: &Inpcbtable,
    dst: &SockaddrIn6,
    fport_arg: u32,
    src: Option<&SockaddrIn6>,
    lport_arg: u32,
    rtable: u32,
    cmd: i32,
    cmdarg: *mut c_void,
    notify: Option<InpNotifyFn>,
) {
    let iter = InpcbIterator::new();
    let mut inp: Option<&'static Inpcb> = None;
    let mut fport = fport_arg as u16;
    let mut lport = lport_arg as u16;
    let mut notify = notify;
    // The command's argument is not used here (as in C): the protocols read theirs from
    // the `ip6ctlparam`.
    let _ = cmdarg;

    if cmd as u32 >= PRC_NCMDS as u32 {
        return;
    }

    if in6_is_addr_unspecified(&dst.sin6_addr) {
        return;
    }
    if in6_is_addr_v4mapped(&dst.sin6_addr) {
        #[cfg(feature = "diagnostic")]
        crate::kprintf!("in6_pcbnotify: Huh?  Thought we never got called with mapped!\n");
        return;
    }

    // note that src can be NULL when we get notify by local fragmentation.
    let mut sa6_src = src.copied().unwrap_or(SA6_ANY);
    let flowinfo = sa6_src.sin6_flowinfo;

    // Redirects go to all references to the destination, and use in_pcbrtchange to
    // invalidate the route cache. Dead host indications: also use in_pcbrtchange to
    // invalidate the cache, and deliver the error to all the sockets. Otherwise, if we have
    // knowledge of the local port and address, deliver only to that socket.
    if prc_is_redirect(cmd) || cmd == PRC_HOSTDEAD {
        fport = 0;
        lport = 0;
        sa6_src.sin6_addr = IN6ADDR_ANY;

        if cmd != PRC_HOSTDEAD {
            notify = Some(in_pcbrtchange);
        }
    }
    let errno = INET6CTLERRMAP[cmd as usize];
    let Some(notify) = notify else {
        return;
    };

    let rdomain = rtable_l2(rtable);
    mtx_enter(&table.inpt_mtx);
    // SAFETY: the mutex is held around every call; `iter` lives on this frame until the walk
    // ends with `None`.
    while let Some(i) = unsafe { in_pcb_iterator(table, inp, &iter) } {
        inp = Some(i);
        kassert!(i.has_flags(INP_IPV6));

        // Under the following condition, notify of redirects to the pcb, without making
        // address matches against inpcb: a redirect notification arrived, the inpcb is
        // unconnected, it caches a !RTF_HOST routing entry, and the ICMPv6 notification is
        // from the gateway cached in the inpcb (the nexthop the inpcb used very recently).
        // This improves the interaction between the redirect handling code and the inpcb
        // route cache: without the clause, a !RTF_HOST routing entry (which carries the
        // gateway used by the inpcb right before the ICMPv6 redirect) would be cached
        // forever in an unconnected inpcb. (Other systems clone RTF_HOST entries on output
        // instead, which exposes them to local DoS attacks; RFC 2461's "destination cache"
        // may imply that behavior. A hiwat/lowat on the number of cloned host routes, see
        // icmp6_mtudisc_update(), may be a good idea.)
        let do_notify = ((prc_is_redirect(cmd) || cmd == PRC_HOSTDEAD)
            && in6_is_addr_unspecified(&i.inp_laddr6.get())
            && i
                .inp_route
                .ro_rt
                .get()
                .is_some_and(|rt| rt.rt_flags.get() & RTF_HOST == 0)
            && in6_are_addr_equal(&i.inp_route.ro_dstsin6().sin6_addr, &dst.sin6_addr))
            // Detect if we should notify the error. If no source and destination ports are
            // specified, but non-zero flowinfo and local address match, notify the error.
            // This is the case when the error is delivered with an encrypted buffer by ESP.
            // Otherwise, just compare addresses and ports as usual.
            || (lport == 0
                && fport == 0
                && flowinfo != 0
                && flowinfo == (i.inp_flowinfo() & IPV6_FLOWLABEL_MASK)
                && in6_are_addr_equal(&i.inp_laddr6.get(), &sa6_src.sin6_addr))
            || !(!in6_are_addr_equal(&i.inp_faddr6.get(), &dst.sin6_addr)
                || rtable_l2(i.inp_rtableid.get()) != rdomain
                || (lport != 0 && i.inp_lport.get() != lport)
                || (!in6_is_addr_unspecified(&sa6_src.sin6_addr)
                    && !in6_are_addr_equal(&i.inp_laddr6.get(), &sa6_src.sin6_addr))
                || (fport != 0 && i.inp_fport.get() != fport));
        if !do_notify {
            continue;
        }

        mtx_leave(&table.inpt_mtx);
        let so = in_pcbsolock(i);
        if so.is_some() {
            notify(i, errno);
        }
        in_pcbsounlock(Some(i), so);
        mtx_enter(&table.inpt_mtx);
    }
    mtx_leave(&table.inpt_mtx);
}

/// `in6_pcbrtentry`: the route to the foreign address of `inp`, from its cache (refreshed
/// when stale).
pub fn in6_pcbrtentry(inp: &Inpcb) -> Option<&'static Rtentry> {
    if in6_is_addr_unspecified(&inp.inp_faddr6.get()) {
        return None;
    }
    let faddr = inp.inp_faddr6.get();
    let laddr = inp.inp_laddr6.get();
    route6_mpath(&inp.inp_route, &faddr, Some(&laddr), inp.inp_rtableid.get())
}

/// `in6_pcbhash_lookup`: the control block of `table` with exactly these addresses in hash
/// chain `hash`, moved to the head of its chain. The table mutex is held.
pub fn in6_pcbhash_lookup(
    table: &Inpcbtable,
    hash: u64,
    rdomain: u32,
    faddr: &In6Addr,
    fport: u16,
    laddr: &In6Addr,
    lport: u16,
) -> Option<&'static Inpcb> {
    net_assert_locked("in6_pcbhash_lookup");
    mutex_assert_locked(&table.inpt_mtx, "in6_pcbhash_lookup");

    let head: &ListHead<InpHash> =
        &table.inpt_hashtbl.get()[(hash & table.inpt_mask.get()) as usize];
    let inp = head
        .iter()
        .find(|inp| {
            kassert!(inp.has_flags(INP_IPV6));

            inp.inp_fport.get() == fport
                && inp.inp_lport.get() == lport
                && in6_are_addr_equal(&inp.inp_faddr6.get(), faddr)
                && in6_are_addr_equal(&inp.inp_laddr6.get(), laddr)
                && rtable_l2(inp.inp_rtableid.get()) == rdomain
        })
        .map(inp_static)?;
    // Move this PCB to the head of hash chain so that repeated accesses are quicker. This is
    // analogous to the historic single-entry PCB cache.
    if !head.first().is_some_and(|f| ptr::eq(f, inp)) {
        // SAFETY: the table mutex is held; `inp` is on this chain and stays on it.
        unsafe {
            ListHead::<InpHash>::remove(inp);
            head.insert_head(inp);
        }
    }
    Some(inp)
}

/// `in6_pcblookup_lock`: the connected control block for `faddr.fport <-> laddr.lport`; no
/// wildcard matching is done. With `IN_PCBLOCK_GRAB` the table mutex is taken here and the
/// result referenced; with `IN_PCBLOCK_HOLD` the caller holds it.
pub fn in6_pcblookup_lock(
    table: &Inpcbtable,
    faddr: &In6Addr,
    fport: u16,
    laddr: &In6Addr,
    lport: u16,
    rtable: u32,
    lock: i32,
) -> Option<&'static Inpcb> {
    let rdomain = rtable_l2(rtable);
    let hash = in6_pcbhash(table, rdomain, faddr, fport, laddr, lport);

    if lock == IN_PCBLOCK_GRAB {
        mtx_enter(&table.inpt_mtx);
    } else {
        kassert!(lock == IN_PCBLOCK_HOLD);
        mutex_assert_locked(&table.inpt_mtx, "in6_pcblookup_lock");
    }
    let inp = in6_pcbhash_lookup(table, hash, rdomain, faddr, fport, laddr, lport);
    if lock == IN_PCBLOCK_GRAB {
        in_pcbref(inp);
        mtx_leave(&table.inpt_mtx);
    }

    #[cfg(feature = "diagnostic")]
    if inp.is_none() && crate::netinet::in_pcb::IN_PCBNOTIFYMISS.load(Ordering::Relaxed) != 0 {
        crate::kprintf!(
            "in6_pcblookup_lock: faddr= fport={} laddr= lport={} rdom={}\n",
            u16::from_be(fport),
            u16::from_be(lport),
            rdomain
        );
    }
    inp
}

/// `in6_pcblookup`: `in6_pcblookup_lock` taking the table mutex; the result is referenced.
pub fn in6_pcblookup(
    table: &Inpcbtable,
    faddr: &In6Addr,
    fport: u16,
    laddr: &In6Addr,
    lport: u16,
    rtable: u32,
) -> Option<&'static Inpcb> {
    in6_pcblookup_lock(table, faddr, fport, laddr, lport, rtable, IN_PCBLOCK_GRAB)
}

/// `in6_pcblookup_listen`: the listening control block for `laddr.lport_arg`: unspecified
/// foreign address and port, bound to `laddr` or to the wildcard address (pf's divert-to
/// key, or the wildcard first for a connection redirected to localhost, from `m`). The
/// result is referenced.
pub fn in6_pcblookup_listen(
    table: &Inpcbtable,
    laddr: &In6Addr,
    lport_arg: u16,
    m: Option<&Mbuf>,
    rtable: u32,
) -> Option<&'static Inpcb> {
    let mut key1 = *laddr;
    let mut key2 = ZEROIN6_ADDR;
    let mut lport = lport_arg;

    if let Some(m) = m
        && m.m_pkthdr().pf.flags.get() & PF_TAG_DIVERTED != 0
    {
        let divert = pf_find_divert(m);
        kassert!(divert.is_some());
        let divert = divert?;
        match divert.type_ {
            PF_DIVERT_TO => {
                key1 = In6Addr::new(divert.addr.addr8);
                key2 = key1;
                lport = divert.port;
            }
            PF_DIVERT_REPLY => return None,
            t => panic(format_args!(
                "in6_pcblookup_listen: unknown divert type {t}, mbuf {m:p}"
            )),
        }
    } else if let Some(m) = m
        && m.m_pkthdr().pf.flags.get() & PF_TAG_TRANSLATE_LOCALHOST != 0
    {
        // Redirected connections should not be treated the same as connections directed to
        // ::1 since localhost can only be accessed from the host itself.
        key1 = ZEROIN6_ADDR;
        key2 = *laddr;
    }

    let rdomain = rtable_l2(rtable);
    let hash = in6_pcbhash(table, rdomain, &ZEROIN6_ADDR, 0, &key1, lport);

    mtx_enter(&table.inpt_mtx);
    let mut inp = in6_pcbhash_lookup(table, hash, rdomain, &ZEROIN6_ADDR, 0, &key1, lport);
    if inp.is_none() && !in6_are_addr_equal(&key1, &key2) {
        let hash = in6_pcbhash(table, rdomain, &ZEROIN6_ADDR, 0, &key2, lport);
        inp = in6_pcbhash_lookup(table, hash, rdomain, &ZEROIN6_ADDR, 0, &key2, lport);
    }
    in_pcbref(inp);
    mtx_leave(&table.inpt_mtx);

    #[cfg(feature = "diagnostic")]
    if inp.is_none() && crate::netinet::in_pcb::IN_PCBNOTIFYMISS.load(Ordering::Relaxed) != 0 {
        crate::kprintf!(
            "in6_pcblookup_listen: laddr= lport={} rdom={}\n",
            u16::from_be(lport),
            rdomain
        );
    }
    inp
}

/// `in6_pcbset_addr`: gives `inp` both addresses and ports at once (a connection accepted
/// from a listener) in routing table `rtableid`, unless another control block has them;
/// the IPv6 half of `in_pcbset_addr`.
pub fn in6_pcbset_addr(
    inp: &'static Inpcb,
    fsin6: &SockaddrIn6,
    lsin6: &SockaddrIn6,
    rtableid: u32,
) -> Result<(), Errno> {
    let table = inp.table();

    mtx_enter(&table.inpt_mtx);

    let t = in6_pcblookup_lock(
        table,
        &fsin6.sin6_addr,
        fsin6.sin6_port,
        &lsin6.sin6_addr,
        lsin6.sin6_port,
        rtableid,
        IN_PCBLOCK_HOLD,
    );
    if t.is_some() {
        mtx_leave(&table.inpt_mtx);
        return Err(Errno::EADDRINUSE);
    }

    inp.inp_rtableid.set(rtableid);
    inp.inp_laddr6.set(lsin6.sin6_addr);
    inp.inp_lport.set(lsin6.sin6_port);
    inp.inp_faddr6.set(fsin6.sin6_addr);
    inp.inp_fport.set(fsin6.sin6_port);
    in_pcbrehash(inp);

    mtx_leave(&table.inpt_mtx);

    in6_pcb_newflow(inp);
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests;
