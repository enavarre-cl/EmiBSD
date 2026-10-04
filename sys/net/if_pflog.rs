/* $OpenBSD: if_pflog.h,v 1.29 2021/01/13 09:13:30 mvs Exp $ */
/*	$OpenBSD: if_pflog.c,v 1.99 2025/07/07 02:28:50 jsg Exp $	*/
/* <LICENSES> */
/*
 * Copyright 2001 Niels Provos <provos@citi.umich.edu>
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
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT, INDIRECT,
 * INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT
 * NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
 * DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
 * THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF
 * THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */

/*
 * The authors of this code are John Ioannidis (ji@tla.org),
 * Angelos D. Keromytis (kermit@csd.uch.gr) and
 * Niels Provos (provos@physnet.uni-hamburg.de).
 *
 * This code was written by John Ioannidis for BSD/OS in Athens, Greece,
 * in November 1995.
 *
 * Ported to OpenBSD and NetBSD, with additional transforms, in December 1996,
 * by Angelos D. Keromytis.
 *
 * Additional transforms and features in 1997 and 1998 by Angelos D. Keromytis
 * and Niels Provos.
 *
 * Copyright (C) 1995, 1996, 1997, 1998 by John Ioannidis, Angelos D. Keromytis
 * and Niels Provos.
 * Copyright (c) 2001, Angelos D. Keromytis, Niels Provos.
 * Copyright (c) 2002 - 2010 Henning Brauer
 *
 * Permission to use, copy, and modify this software with or without fee
 * is hereby granted, provided that this entire notice is included in
 * all copies of any software which is or includes a copy or
 * modification of this software.
 * You may use this code under the GNU public license if you so wish. Please
 * contribute changes back to the authors under this freer than GPL license
 * so that we may further the use of strong encryption without limitations to
 * all.
 *
 * THIS SOFTWARE IS BEING PROVIDED "AS IS", WITHOUT ANY EXPRESS OR
 * IMPLIED WARRANTY. IN PARTICULAR, NONE OF THE AUTHORS MAKES ANY
 * REPRESENTATION OR WARRANTY OF ANY KIND CONCERNING THE
 * MERCHANTABILITY OF THIS SOFTWARE OR ITS FITNESS FOR ANY PARTICULAR
 * PURPOSE.
 */
/* </LICENSES> */

//! The packet filter logging interface, `pflog(4)`: `net/if_pflog.c` and `net/if_pflog.h`.
//! pf hands the packets of rules with `log` to `pflog_packet`, which passes them, behind a
//! `struct pfloghdr`, to the `bpf(4)` listeners of the rule's `pflogN` interface
//! (`pflogd(8)`, `tcpdump(8)`). The interfaces are cloned (`ifconfig pflog0 create`);
//! `pflogattach` (a pseudo-device in `pdevinit[]`) registers the cloner.
//!
//! Upstream: sys/net/if_pflog.c @ 3ce1f3f79392, sys/net/if_pflog.h @ 3ce1f3f79392
//!
//! `struct pfloghdr` is ABI: `pflogd(8)` writes it to its log files and `tcpdump(8)` reads
//! it back (`DLT_PFLOG`), so it keeps the C layout (`#[repr(C)]`, `PFLOG_HDRLEN` bytes).
//!
//! ## Deviations
//! - `bpf(4)` is not configured (`NBPFILTER` 0): `pflog_clone_create` attaches no bpf tap
//!   (`bpfattach(..., DLT_PFLOG, PFLOG_HDRLEN)`), and the whole body of `pflog_packet` (the
//!   `pfloghdr` it builds and `bpf_mtap_hdr`) is compiled out in the C, so `pflog_packet`
//!   only returns success, as the C does. Both are comments at their sites.
//! - `PFLOGDEBUG` is not defined: `DPRINTF` expands to nothing and is not ported.
//! - `pflogoutput` and `pflogioctl` are `unsafe fn`s, the signatures of `if_output` and
//!   `if_ioctl` (`net/if_var.rs`).
//! - `pflog_packet` returns `bool` for the C's `int` 0 (`true`) or -1 (`false`, bad
//!   arguments, which only the compiled-out bpf path can find).

use core::cell::Cell;
use core::ptr::{self, NonNull};

use crate::kern::kern_malloc::{free, malloc};
use crate::kern::subr_prf::{panic, snprintf};
use crate::kern::uipc_mbuf::m_freem;
use crate::net::if_::{
    IFF_RUNNING, IFF_UP, IFNAMSIZ, IFXF_CLONED, if_alloc_sadl, if_attach, if_clone_attach,
    if_detach,
};
use crate::net::if_types::IFT_PFLOG;
use crate::net::if_var::{IfClone, Ifnet};
use crate::net::pfvar::{PfAddr, PfRule, PfRuleset};
use crate::net::pfvar_priv::{PfGlobal, PfPdesc};
use crate::net::route::Rtentry;
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_DEVBUF, M_WAITOK, M_ZERO};
use crate::sys::mbuf::{MHLEN, MLEN, Mbuf};
use crate::sys::queue::{ListEntry, ListHead};
use crate::sys::socket::Sockaddr;
use crate::sys::sockio::SIOCSIFFLAGS;
use crate::sys::systm::{net_assert_locked, net_lock, net_unlock};
use crate::sys::types::{Pid, SaFamily, Uid};

/// `PFLOG_RULESET_NAME_SIZE`.
pub const PFLOG_RULESET_NAME_SIZE: usize = 16;

/// `struct pfloghdr`: what precedes a logged packet for `bpf(4)` (`DLT_PFLOG`).
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Pfloghdr {
    /// `length`: `PFLOG_REAL_HDRLEN`.
    pub length: u8,
    /// `af`.
    pub af: SaFamily,
    /// `action`: `PF_PASS`, `PF_DROP`, ...
    pub action: u8,
    /// `reason`: `PFRES_*`.
    pub reason: u8,
    /// `ifname`.
    pub ifname: [u8; IFNAMSIZ],
    /// `ruleset`: the anchor of the rule.
    pub ruleset: [u8; PFLOG_RULESET_NAME_SIZE],
    /// `rulenr`: network order.
    pub rulenr: u32,
    /// `subrulenr`: network order, -1 when not in an anchor.
    pub subrulenr: u32,
    /// `uid`.
    pub uid: Uid,
    /// `pid`.
    pub pid: Pid,
    /// `rule_uid`.
    pub rule_uid: Uid,
    /// `rule_pid`.
    pub rule_pid: Pid,
    /// `dir`.
    pub dir: u8,
    /// `rewritten`.
    pub rewritten: u8,
    /// `naf`.
    pub naf: SaFamily,
    /// `pad[1]`.
    pub pad: [u8; 1],
    /// `saddr`.
    pub saddr: PfAddr,
    /// `daddr`.
    pub daddr: PfAddr,
    /// `sport`.
    pub sport: u16,
    /// `dport`.
    pub dport: u16,
}

/// `PFLOG_HDRLEN`.
pub const PFLOG_HDRLEN: usize = size_of::<Pfloghdr>();
/// `PFLOG_REAL_HDRLEN`: used to be minus pad, also used as a signature.
pub const PFLOG_REAL_HDRLEN: usize = PFLOG_HDRLEN;
/// `PFLOG_OLD_HDRLEN`.
pub const PFLOG_OLD_HDRLEN: usize = core::mem::offset_of!(Pfloghdr, pad);

/// `struct pflog_softc`. The all-zero value is valid (`malloc(M_ZERO)`).
pub struct PflogSoftc {
    /// `sc_entry`: on `pflog_ifs`.
    pub sc_entry: ListEntry<PflogSoftc>,
    /// `sc_if`: the interface.
    pub sc_if: Ifnet,
    /// `sc_unit`.
    pub sc_unit: Cell<i32>,
}

// SAFETY: the list link changes under the net lock, `sc_unit` only before the softc is
// published, the interface as `Ifnet` documents.
unsafe impl Sync for PflogSoftc {}

crate::queue_adapter!(
    /// `LIST_HEAD(, pflog_softc)`.
    pub PflogIfs: PflogSoftc, sc_entry => ListEntry<PflogSoftc>
);

/// `PFLOGMTU`.
pub const PFLOGMTU: u32 = (32768 + MHLEN + MLEN) as u32;

/// `pflog_cloner`.
pub static PFLOG_CLONER: IfClone =
    IfClone::new(b"pflog", pflog_clone_create, Some(pflog_clone_destroy));

/// `pflog_ifs`: \[N\] the `pflog` interfaces.
pub static PFLOG_IFS: PfGlobal<ListHead<PflogIfs>> = PfGlobal(ListHead::new());

/// `pflogattach`: the pseudo-device attach function: registers the cloner.
pub fn pflogattach(_npflog: i32) {
    // SAFETY: `pflogattach` runs once, from `main`'s pseudo-device attach.
    unsafe { if_clone_attach(&PFLOG_CLONER) };
}

/// `pflog_clone_create`: creates `pflog<unit>`.
pub fn pflog_clone_create(_ifc: &'static IfClone, unit: i32) -> Result<(), Errno> {
    let Some(mem) = malloc(size_of::<PflogSoftc>(), M_DEVBUF, M_WAITOK | M_ZERO) else {
        panic(format_args!("pflog_clone_create: out of memory"));
    };
    // SAFETY: a zero-filled block of `sizeof(struct pflog_softc)` bytes, aligned for it
    // (malloc's chunks are aligned to their size); the all-zero softc is valid. It lives
    // until `pflog_clone_destroy`.
    let pflogif: &'static PflogSoftc = unsafe { &*mem.as_ptr().cast::<PflogSoftc>() };
    pflogif.sc_unit.set(unit);
    let ifp = &pflogif.sc_if;
    let mut xname = [0u8; IFNAMSIZ];
    let _ = snprintf(&mut xname, format_args!("pflog{unit}"));
    ifp.if_xname.set(xname);
    ifp.if_softc.set(ptr::from_ref(pflogif).cast_mut().cast());
    ifp.if_mtu.set(PFLOGMTU);
    ifp.if_ioctl.set(Some(pflogioctl));
    ifp.if_output.set(Some(pflogoutput));
    ifp.if_xflags.set(IFXF_CLONED);
    ifp.if_type.set(IFT_PFLOG);
    ifp.if_hdrlen.set(PFLOG_HDRLEN as u8);
    if_attach(ifp);
    if_alloc_sadl(ifp);

    // NBPFILTER > 0: bpfattach(&pflogif->sc_if.if_bpf, ifp, DLT_PFLOG, PFLOG_HDRLEN); not
    // configured.

    net_lock();
    // SAFETY: the new softc is on no list; it stays allocated until `pflog_clone_destroy`
    // takes it off.
    unsafe { PFLOG_IFS.insert_head(pflogif) };
    net_unlock();

    Ok(())
}

/// `pflog_clone_destroy`.
pub fn pflog_clone_destroy(ifp: &'static Ifnet) -> Result<(), Errno> {
    // SAFETY: `if_softc` of a `pflog` interface is its softc (`pflog_clone_create`), which
    // embeds the interface and lives until the `free` below.
    let pflogif: &'static PflogSoftc =
        unsafe { &*ifp.if_softc.get().cast::<PflogSoftc>().cast_const() };

    net_lock();
    // SAFETY: `pflog_clone_create` put the softc on `pflog_ifs`.
    unsafe { ListHead::<PflogIfs>::remove(pflogif) };
    net_unlock();

    if_detach(ifp);
    free(
        NonNull::from(pflogif).cast(),
        M_DEVBUF,
        size_of::<PflogSoftc>(),
    );

    Ok(())
}

/// `pflogoutput`: drops the packet.
///
/// # Safety
///
/// As for `if_output` (`IfOutputFn`).
pub unsafe fn pflogoutput(
    _ifp: &'static Ifnet,
    m: &'static Mbuf,
    _dst: *const Sockaddr,
    _rt: Option<&'static Rtentry>,
) -> Result<(), Errno> {
    m_freem(m); // drop packet
    Err(Errno::EAFNOSUPPORT)
}

/// `pflogioctl`: follows `IFF_UP` with `IFF_RUNNING`.
///
/// # Safety
///
/// As for `if_ioctl` (`IfIoctlFn`); `data` is not used.
pub unsafe fn pflogioctl(ifp: &'static Ifnet, cmd: u64, _data: *mut u8) -> Result<(), Errno> {
    match cmd {
        SIOCSIFFLAGS => {
            if ifp.if_flags.get() & IFF_UP != 0 {
                ifp.if_flags.set(ifp.if_flags.get() | IFF_RUNNING);
            } else {
                ifp.if_flags.set(ifp.if_flags.get() & !IFF_RUNNING);
            }
        }
        _ => return Err(Errno::ENOTTY),
    }

    Ok(())
}

/// `pflog_getif`: the `pflog` interface of unit `unit`.
pub fn pflog_getif(unit: i32) -> Option<&'static PflogSoftc> {
    net_assert_locked("pflog_getif");

    PFLOG_IFS.iter().find(|p| p.sc_unit.get() == unit)
}

/// `pflog_packet`: logs the packet `pd` describes, matched by rule `rm` (in anchor rule
/// `am` of `ruleset`), on the `pflog` interface of `trigger` (or of `rm`).
pub fn pflog_packet(
    _pd: &mut PfPdesc,
    _reason: u8,
    _rm: &'static PfRule,
    _am: Option<&'static PfRule>,
    _ruleset: Option<&'static PfRuleset>,
    _trigger: Option<&'static PfRule>,
) -> bool {
    // NBPFILTER > 0: the packet is handed to the bpf listeners of the pflog interface of
    // trigger->logif (trigger defaults to rm; -1 for a missing rule, descriptor, kif or
    // packet; 0 without the interface or a listener) behind a struct pfloghdr: length
    // PFLOG_REAL_HDRLEN; the action (PF_DROP for the default rule when the reason is not
    // PFRES_MATCH); the reason; the kif's name; the rule numbers (am's and rm's in network
    // order, or rm's and -1) and the ruleset's anchor name; the socket's uid/pid for
    // `log (user)` (pf_socket_lookup), else -1/NO_PID; the rule's creator uid/pid; the
    // direction and family; `rewritten` when the addresses or ports were translated; the
    // translated family, addresses and ports. The interface counts an output packet of the
    // packet's length, and bpf_mtap_hdr(if_bpf, &hdr, sizeof(hdr), pd->m,
    // BPF_DIRECTION_OUT) passes it on. bpf(4) is not configured.

    true
}

const _: () = assert!(PFLOG_HDRLEN == 100);
const _: () = assert!(PFLOG_OLD_HDRLEN == 63);

#[cfg(test)]
mod tests;
