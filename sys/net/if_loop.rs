/*	$OpenBSD: if_loop.c,v 1.103 2025/09/09 09:16:18 bluhm Exp $	*/
/*	$NetBSD: if_loop.c,v 1.15 1996/05/07 02:40:33 thorpej Exp $	*/
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
 * Copyright (c) 1982, 1986, 1993
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
 *	@(#)if_loop.c	8.1 (Berkeley) 6/10/93
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
/* </LICENSES> */

/* <CODE> */
//! The loopback interface, `lo(4)`: `net/if_loop.c`, a driver for protocol testing and
//! timing. `lo0` is attached at boot (`loopattach`, a pseudo-device in `pdevinit[]`); more
//! loopbacks are cloned (`ifconfig lo1 create`), one per routing domain.
//!
//! Upstream: sys/net/if_loop.c @ 3ce1f3f79392
//!
//! A packet sent on a loopback interface (`looutput`) is queued on one of its input queues
//! (`if_output_local`) rather than handed up at once, to avoid stack overflow and make TCP
//! handshakes over loopback work; the softnet task then gives it to `loinput`, which passes
//! it to its protocol through `if_input_local`.
//!
//! Status: `ported` (M7b).
//!
//! ## Deviations
//! - `loioctl` and `looutput` are `unsafe fn`s, the signatures of `if_ioctl` and `if_output`
//!   (`net/if_var.rs`).

use core::ptr;
use core::sync::atomic::Ordering;

use crate::kern::kern_malloc::{free, malloc};
use crate::kern::subr_prf::{panic, snprintf};
use crate::kern::uipc_mbuf::m_freem;
use crate::net::bpf::{DLT_LOOP, bpfattach};
use crate::net::if_::{
    IFCAP_CSUM_IPv4, IFCAP_CSUM_TCPv4, IFCAP_CSUM_TCPv6, IFCAP_CSUM_UDPv4, IFCAP_CSUM_UDPv6,
    IFCAP_LRO, IFCAP_TSOv4, IFCAP_TSOv6, IFF_LOOPBACK, IFF_MULTICAST, IFF_RUNNING, IFXF_CLONED,
    IFXF_LRO, Ifreq, counters_inc, if_addgroup, if_alloc_sadl, if_attach, if_attach_iqueues,
    if_attach_queues, if_attachhead, if_clone_attach, if_counters_alloc, if_detach, if_input_local,
    if_output_local, if_up, softnet_count,
};
use crate::net::if_types::IFT_LOOP;
use crate::net::if_var::{IfClone, IfCounters, Ifnet, Netstack};
use crate::net::route::{RTF_BLACKHOLE, RTF_HOST, RTF_REJECT, Rtentry};
use crate::net::rtable::{rtable_l2set, rtable_loindex};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_DEVBUF, M_WAITOK, M_ZERO};
use crate::sys::mbuf::{M_PKTHDR, Mbuf};
use crate::sys::socket::Sockaddr;
use crate::sys::sockio::{SIOCADDMULTI, SIOCDELMULTI, SIOCSIFADDR, SIOCSIFFLAGS, SIOCSIFMTU};

/// `LOMTU`.
pub const LOMTU: u32 = 32768;

/// `loop_cloner`.
pub static LOOP_CLONER: IfClone = IfClone::new(b"lo", loop_clone_create, Some(loop_clone_destroy));

/// `loopattach`: the pseudo-device attach function: creates `lo0` and registers the cloner.
pub fn loopattach(_n: i32) {
    if loop_clone_create(&LOOP_CLONER, 0).is_err() {
        panic(format_args!("unable to create lo0"));
    }

    // SAFETY: `loopattach` runs once, from `main`'s pseudo-device attach.
    unsafe { if_clone_attach(&LOOP_CLONER) };
}

/// `loop_clone_create`: creates `lo<unit>`.
pub fn loop_clone_create(ifc: &'static IfClone, unit: i32) -> Result<(), Errno> {
    let Some(ifp) = malloc(size_of::<Ifnet>(), M_DEVBUF, M_WAITOK | M_ZERO) else {
        panic(format_args!("loop_clone_create: out of memory"));
    };
    // SAFETY: a zero-filled `Ifnet`-sized block, aligned for it (malloc's chunks are aligned
    // to their size); the all-zero `Ifnet` is valid. It lives until `loop_clone_destroy`.
    let ifp: &'static Ifnet = unsafe { &*ifp.as_ptr().cast::<Ifnet>() };

    let mut xname = [0u8; 16];
    let _ = snprintf(&mut xname, format_args!("lo{unit}"));
    ifp.if_xname.set(xname);
    ifp.if_softc.set(ptr::null_mut());
    ifp.if_mtu.set(LOMTU);
    ifp.if_flags.set(IFF_LOOPBACK | IFF_MULTICAST);
    ifp.if_xflags.set(IFXF_CLONED | IFXF_LRO);
    ifp.if_capabilities.set(
        IFCAP_CSUM_IPv4
            | IFCAP_CSUM_TCPv4
            | IFCAP_CSUM_UDPv4
            | IFCAP_CSUM_TCPv6
            | IFCAP_CSUM_UDPv6
            | IFCAP_LRO
            | IFCAP_TSOv4
            | IFCAP_TSOv6,
    );
    ifp.if_bpf_mtap.set(Some(lo_bpf_mtap));
    ifp.if_rtrequest.set(Some(lortrequest));
    ifp.if_ioctl.set(Some(loioctl));
    ifp.if_input.set(Some(loinput));
    ifp.if_output.set(Some(looutput));
    ifp.if_type.set(IFT_LOOP);
    ifp.if_hdrlen.set(size_of::<u32>() as u8);
    if_counters_alloc(ifp);
    if unit == 0 {
        if_attachhead(ifp);
        let _ = if_addgroup(ifp, ifc.ifc_name);
        rtable_l2set(0, 0, ifp.if_index.get());
    } else {
        if_attach(ifp);
    }
    if_attach_queues(ifp, softnet_count());
    if_attach_iqueues(ifp, softnet_count());
    if_alloc_sadl(ifp);
    bpfattach(&ifp.if_bpf, ifp, DLT_LOOP, size_of::<u32>() as u32);
    Ok(())
}

/// `loop_clone_destroy`: destroys a cloned loopback, unless it is a routing domain's.
pub fn loop_clone_destroy(ifp: &'static Ifnet) -> Result<(), Errno> {
    let mut rdomain = 0;

    if ifp.if_index.get() == rtable_loindex(ifp.if_rdomain.get()) {
        // rdomain 0 always needs a loopback
        if ifp.if_rdomain.get() == 0 {
            return Err(Errno::EPERM);
        }

        // if there is any other interface in this rdomain, deny
        crate::sys::systm::net_lock_shared();
        let busy = crate::net::if_::IFNETLIST.0.iter().any(|p| {
            p.if_rdomain.get() == ifp.if_rdomain.get() && p.if_index.get() != ifp.if_index.get()
        });
        crate::sys::systm::net_unlock_shared();
        if busy {
            return Err(Errno::EBUSY);
        }

        rdomain = ifp.if_rdomain.get();
    }

    if_detach(ifp);

    free(ptr::NonNull::from(ifp).cast(), M_DEVBUF, size_of::<Ifnet>());

    if rdomain != 0 {
        rtable_l2set(rdomain, 0, 0);
    }
    Ok(())
}

/// `lo_bpf_mtap`: loopback dumps on output, disable input bpf.
pub fn lo_bpf_mtap(_if_bpf: *mut u8, _m: &Mbuf, _dir: u32) -> bool {
    false
}

/// `loinput`: a looped packet, from the input queue, to its protocol.
pub fn loinput(ifp: &'static Ifnet, m: &'static Mbuf, ns: Option<&Netstack>) {
    if m.m_flags().get() & M_PKTHDR == 0 {
        panic(format_args!("loinput: no header mbuf"));
    }

    if if_input_local(ifp, m, m.m_pkthdr().ph_family.get(), ns).is_err()
        && let Some(c) = ifp.if_counters.get()
    {
        counters_inc(c, IfCounters::IfcIerrors);
    }
}

/// `looutput`: the loopback's `if_output`.
///
/// # Safety
///
/// As for `if_output` (`IfOutputFn`).
pub unsafe fn looutput(
    ifp: &'static Ifnet,
    m: &'static Mbuf,
    dst: *const Sockaddr,
    rt: Option<&'static Rtentry>,
) -> Result<(), Errno> {
    if m.m_flags().get() & M_PKTHDR == 0 {
        panic(format_args!("looutput: no header mbuf"));
    }

    if let Some(rt) = rt
        && rt.rt_flags.get() & (RTF_REJECT | RTF_BLACKHOLE) != 0
    {
        m_freem(m);
        let flags = rt.rt_flags.get();
        return if flags & RTF_BLACKHOLE != 0 {
            Ok(())
        } else if flags & RTF_HOST != 0 {
            Err(Errno::EHOSTUNREACH)
        } else {
            Err(Errno::ENETUNREACH)
        };
    }

    // Do not call if_input_local() directly. Queue the packet to avoid stack overflow and
    // make TCP handshake over loopback work.
    // SAFETY: `dst` is readable (the caller's contract).
    if_output_local(ifp, m, unsafe { (*dst).sa_family })
}

/// `lortrequest`: a route over the loopback gets the loopback's MTU.
pub fn lortrequest(_ifp: &'static Ifnet, _cmd: i32, rt: Option<&'static Rtentry>) {
    if let Some(rt) = rt {
        let _ = rt
            .rt_mtu()
            .compare_exchange(0, LOMTU, Ordering::Relaxed, Ordering::Relaxed);
    }
}

/// `loioctl`: process an ioctl request.
///
/// # Safety
///
/// As for `if_ioctl` (`IfIoctlFn`).
pub unsafe fn loioctl(ifp: &'static Ifnet, cmd: u64, data: *mut u8) -> Result<(), Errno> {
    match cmd {
        SIOCSIFFLAGS => {
            let caps = ifp.if_capabilities.get();
            if ifp.if_xflags.get() & IFXF_LRO != 0 {
                ifp.if_capabilities.set(caps | IFCAP_TSOv4 | IFCAP_TSOv6);
            } else {
                ifp.if_capabilities.set(caps & !(IFCAP_TSOv4 | IFCAP_TSOv6));
            }
            Ok(())
        }

        SIOCSIFADDR => {
            ifp.if_flags.set(ifp.if_flags.get() | IFF_RUNNING);
            if_up(ifp); // send up RTM_IFINFO
            // Everything else is done at a higher level.
            Ok(())
        }

        SIOCADDMULTI | SIOCDELMULTI => Ok(()),

        SIOCSIFMTU => {
            // SAFETY: the caller's contract: `SIOCSIFMTU` takes a `struct ifreq`.
            let ifr = unsafe { &*data.cast::<Ifreq>() };
            ifp.if_mtu.set(ifr.ifr_mtu() as u32);
            Ok(())
        }

        _ => Err(Errno::ENOTTY),
    }
}
/* </CODE> */
