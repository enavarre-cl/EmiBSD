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

/* <CODE> */
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
//! - `PFLOGDEBUG` is not defined: `DPRINTF` expands to nothing and is not ported.
//! - `pflogoutput` and `pflogioctl` are `unsafe fn`s, the signatures of `if_output` and
//!   `if_ioctl` (`net/if_var.rs`).
//! - `pflog_packet` returns `bool` for the C's `int` 0 (`true`) or -1 (`false`, bad
//!   arguments); `rm` and `pd` are references, so only a missing kif or packet is bad.

use core::cell::Cell;
use core::ptr::{self, NonNull};

use crate::kern::kern_malloc::{free, malloc};
use crate::kern::subr_prf::{panic, snprintf};
use crate::kern::uipc_mbuf::m_freem;
use crate::net::bpf::{BPF_DIRECTION_OUT, DLT_PFLOG, bpf_mtap_hdr, bpfattach};
use crate::net::if_::{
    IFF_RUNNING, IFF_UP, IFNAMSIZ, IFXF_CLONED, if_alloc_sadl, if_attach, if_clone_attach,
    if_detach,
};
use crate::net::if_types::IFT_PFLOG;
use crate::net::if_var::{IfClone, Ifnet};
use crate::net::pf::{pf_addr_compare, pf_addrcpy, pf_socket_lookup};
use crate::net::pfvar::{PF_DROP, PF_LOG_USER, PFRES_MATCH, PfAddr, PfRule, PfRuleset};
use crate::net::pfvar_priv::{PfGlobal, PfLoc, PfPdesc};
use crate::net::route::Rtentry;
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_DEVBUF, M_WAITOK, M_ZERO};
use crate::sys::mbuf::{MHLEN, MLEN, Mbuf};
use crate::sys::proc::NO_PID;
use crate::sys::queue::{ListEntry, ListHead};
use crate::sys::socket::Sockaddr;
use crate::sys::sockio::SIOCSIFFLAGS;
use crate::sys::systm::{net_assert_locked, net_lock, net_unlock};
use crate::sys::types::{Pid, SaFamily, Uid};
use libkern::strlcpy;

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

    bpfattach(&pflogif.sc_if.if_bpf, ifp, DLT_PFLOG, PFLOG_HDRLEN as u32);

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
    pd: &mut PfPdesc,
    reason: u8,
    rm: &'static PfRule,
    am: Option<&'static PfRule>,
    ruleset: Option<&'static PfRuleset>,
    trigger: Option<&'static PfRule>,
) -> bool {
    let (Some(kif), Some(m)) = (pd.kif, pd.m) else {
        return false;
    };
    let trigger = trigger.unwrap_or(rm);
    let Some(pflogif) = pflog_getif(i32::from(trigger.logif)) else {
        return true;
    };
    let ifn = &pflogif.sc_if;
    let if_bpf = ifn.if_bpf.get();
    if if_bpf.is_null() {
        return true;
    }

    let mut hdr = Pfloghdr {
        length: PFLOG_REAL_HDRLEN as u8,
        // Default rule does not pass packets dropped for other reasons.
        action: if rm.nr.get() == u32::MAX && u16::from(reason) != PFRES_MATCH {
            PF_DROP
        } else {
            rm.action
        },
        reason,
        ifname: kif.pfik_name,
        ..Pfloghdr::default()
    };

    match am {
        None => {
            hdr.rulenr = rm.nr.get().to_be();
            hdr.subrulenr = u32::MAX;
        }
        Some(am) => {
            hdr.rulenr = am.nr.get().to_be();
            hdr.subrulenr = rm.nr.get().to_be();
            if let Some(anchor) = ruleset.and_then(|rs| rs.anchor.get()) {
                let _ = strlcpy(&mut hdr.ruleset, &anchor.name);
            }
        }
    }
    if trigger.log & PF_LOG_USER != 0 && pd.lookup.done == 0 {
        pd.lookup.done = i32::from(pf_socket_lookup(pd));
    }
    if trigger.log & PF_LOG_USER != 0 && pd.lookup.done > 0 {
        hdr.uid = pd.lookup.uid;
        hdr.pid = pd.lookup.pid;
    } else {
        hdr.uid = Uid::MAX;
        hdr.pid = NO_PID;
    }
    hdr.rule_uid = rm.cuid;
    hdr.rule_pid = rm.cpid;
    hdr.dir = pd.dir;
    hdr.af = pd.af;

    if !matches!(pd.src, PfLoc::None) && !matches!(pd.dst, PfLoc::None) {
        let (src, dst) = (pd.ld_addr(pd.src), pd.ld_addr(pd.dst));
        if pd.af != pd.naf
            || pf_addr_compare(&src, &pd.nsaddr, pd.naf) != 0
            || pf_addr_compare(&dst, &pd.ndaddr, pd.naf) != 0
            || pd.osport != pd.nsport
            || pd.odport != pd.ndport
        {
            hdr.rewritten = 1;
        }
    }
    hdr.naf = pd.naf;
    pf_addrcpy(&mut hdr.saddr, &pd.nsaddr, pd.naf);
    pf_addrcpy(&mut hdr.daddr, &pd.ndaddr, pd.naf);
    hdr.sport = pd.nsport;
    hdr.dport = pd.ndport;

    ifn.if_opackets().set(ifn.if_opackets().get() + 1);
    ifn.if_obytes()
        .set(ifn.if_obytes().get() + m.m_pkthdr().len.get() as u64);

    // SAFETY: `Pfloghdr` is `#[repr(C)]` plain data of `PFLOG_HDRLEN` bytes without holes
    // (`pad` is a member), so viewing it as bytes reads only initialised memory.
    let bytes = unsafe {
        core::slice::from_raw_parts(ptr::from_ref(&hdr).cast::<u8>(), size_of::<Pfloghdr>())
    };
    let _ = bpf_mtap_hdr(if_bpf, bytes, m, BPF_DIRECTION_OUT);

    true
}

const _: () = assert!(PFLOG_HDRLEN == 100);
const _: () = assert!(PFLOG_OLD_HDRLEN == 63);
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for `pflog(4)`: the `pfloghdr` layout and cloning `pflog` interfaces.

    use super::*;
    use crate::net::if_::tests::{setup_net, test_packet};
    use crate::net::if_::{if_clone_create, if_put, if_unit};

    #[test]
    fn pfloghdr_has_the_c_layout() {
        // sizeof(struct pfloghdr) and offsetof(..., pad) as clang computes them for amd64 and
        // arm64.
        assert_eq!(PFLOG_HDRLEN, 100);
        assert_eq!(PFLOG_REAL_HDRLEN, PFLOG_HDRLEN);
        assert_eq!(PFLOG_OLD_HDRLEN, 63);
        assert_eq!(core::mem::offset_of!(Pfloghdr, ifname), 4);
        assert_eq!(core::mem::offset_of!(Pfloghdr, ruleset), 20);
        assert_eq!(core::mem::offset_of!(Pfloghdr, rulenr), 36);
        assert_eq!(core::mem::offset_of!(Pfloghdr, rule_pid), 56);
        assert_eq!(core::mem::offset_of!(Pfloghdr, dir), 60);
        assert_eq!(core::mem::offset_of!(Pfloghdr, saddr), 64);
        assert_eq!(core::mem::offset_of!(Pfloghdr, daddr), 80);
        assert_eq!(core::mem::offset_of!(Pfloghdr, dport), 98);
    }

    #[test]
    fn pflog_interfaces_are_cloned() {
        let _g = setup_net();
        // The interface queues name their softnet task queue (no thread runs it here).
        crate::net::if_::softnet_init();
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| pflogattach(1));

        assert_eq!(pflog_clone_create(&PFLOG_CLONER, 0), Ok(()));
        let ifp = if_unit(b"pflog0").expect("pflog0");
        assert_eq!(ifp.if_type.get(), IFT_PFLOG);
        assert_eq!(ifp.if_mtu.get(), PFLOGMTU);
        assert_eq!(usize::from(ifp.if_hdrlen.get()), PFLOG_HDRLEN);
        assert_eq!(ifp.if_xflags.get() & IFXF_CLONED, IFXF_CLONED);
        if_put(ifp);

        net_lock();
        let sc = pflog_getif(0).expect("unit 0");
        assert!(ptr::eq(&sc.sc_if, ifp));
        assert!(pflog_getif(7).is_none());
        net_unlock();

        ifp.if_flags.set(ifp.if_flags.get() | IFF_UP);
        // SAFETY: SIOCSIFFLAGS reads no argument.
        let r = unsafe { pflogioctl(ifp, SIOCSIFFLAGS, ptr::null_mut()) };
        assert_eq!(r, Ok(()));
        assert_ne!(ifp.if_flags.get() & IFF_RUNNING, 0);
        ifp.if_flags.set(ifp.if_flags.get() & !IFF_UP);
        // SAFETY: as above.
        let r = unsafe { pflogioctl(ifp, SIOCSIFFLAGS, ptr::null_mut()) };
        assert_eq!(r, Ok(()));
        assert_eq!(ifp.if_flags.get() & IFF_RUNNING, 0);
        // SAFETY: as above.
        let r = unsafe { pflogioctl(ifp, 0, ptr::null_mut()) };
        assert_eq!(r, Err(Errno::ENOTTY));

        let m = test_packet(&[0x45, 0, 0, 20]);
        // SAFETY: `pflogoutput` reads no destination.
        let r = unsafe { pflogoutput(ifp, m, ptr::null(), None) };
        assert_eq!(r, Err(Errno::EAFNOSUPPORT));

        // The cloner makes more units, by name.
        assert_eq!(if_clone_create(b"pflog1", 0), Ok(()));
        assert_eq!(if_clone_create(b"pflog1", 0), Err(Errno::EEXIST));
        net_lock();
        assert_eq!(pflog_getif(1).map(|sc| sc.sc_unit.get()), Some(1));
        net_unlock();

        // pflog_clone_destroy runs if_detach, which sleeps on the interface's references and
        // task barriers: there is no process to sleep on the host, so destruction is left to
        // the kernel; here the softc is taken off the list as it does first.
        net_lock();
        // SAFETY: pflog_clone_create put the softc on pflog_ifs.
        unsafe { ListHead::<PflogIfs>::remove(sc) };
        assert!(pflog_getif(0).is_none());
        net_unlock();
    }
}
/* </TESTS> */
