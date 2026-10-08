/* $OpenBSD: ip_spd.c,v 1.122 2025/07/08 00:47:41 jsg Exp $ */
/* <LICENSES> */
/*
 * The author of this code is Angelos D. Keromytis (angelos@cis.upenn.edu)
 *
 * Copyright (c) 2000-2001 Angelos D. Keromytis.
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
//! The IPsec security policy database (SPD): `netinet/ip_spd.c`. One radix tree of
//! [`IpsecPolicy`]s per routing domain, keyed by `struct sockaddr_encap` (the flow: direction,
//! addresses, protocol and ports), the policy lookup for a packet (`ipsp_spd_lookup`), which
//! finds or caches the SA a flow needs, and the acquire records sent to key management when
//! an SA is missing.
//!
//! Upstream: sys/netinet/ip_spd.c @ 3ce1f3f79392
//!
//! Status: `ported` (M9c).
//!
//! ## Deviations
//! - `ipsp_spd_lookup` and `ipsp_spd_inp` return `Result<(), SpdError>`: the C's `-EINVAL`
//!   ("silently drop the packet") is [`SpdError::Drop`], any other errno
//!   [`SpdError::Errno`]. `struct tdb **tdbout` is `Option<&mut Option<&'static Tdb>>`.
//! - `spd_tables` (`struct radix_node_head **` grown with `mallocarray(M_RTABLE)`) is a
//!   `Vec` of head pointers in a `StaticCell`, under the net lock; `spd_table_max` is its
//!   length minus one. The heads themselves come from `rn_inithead` (`malloc(M_RTABLE)`).
//! - `spd_table_walk` takes the walker as a closure; the radix walker's `struct radix_node *`
//!   is turned back into the policy that embeds it. A walker may free the policy it is given
//!   (`pfkeyv2_policy_flush`), as in C.
//! - The SPD radix key is `ipo_addr`, read through the pointer the policy's `Cell` gives:
//!   the key is written before the policy enters the tree and not changed while it is there.
//! - The ports of the flow key are copied out of the packet in network order, as the C's
//!   `m_copydata` into the `sockaddr_encap` does.
//! - `INET6` is configured (feature `inet6`): the `AF_INET6` flow key of the lookup, the
//!   IPv6 unspecified-address checks and the `SENT_IP6` acquire. A packet whose IPv6
//!   header is not in the first mbuf is read with `m_copydata`, as the C does.

use alloc::vec::Vec;
use core::ffi::c_void;
use core::mem::{offset_of, size_of};
use core::ptr::{self, NonNull};
use core::sync::atomic::Ordering;

use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_synch::{refcnt_init, refcnt_rele, refcnt_take};
use crate::kern::kern_tc::getuptime;
use crate::kern::kern_timeout::{timeout_add_sec, timeout_del, timeout_set};
use crate::kern::subr_pool::{pool_get, pool_put};
use crate::kern::uipc_mbuf::m_copydata;
use crate::machine::intr::IPL_SOFTNET;
use crate::net::pfkeyv2::pfkeyv2_acquire;
use crate::net::radix::{RadixNode, RadixNodeHead, rn_delete, rn_inithead, rn_match, rn_walktree};
use crate::net::rtable::rtable_l2;
use crate::netinet::in_::IPSEC_LEVEL_AVAIL;
use crate::netinet::in_::IPSEC_LEVEL_BYPASS;
use crate::netinet::in_::InAddr;
use crate::netinet::in_::{
    INADDR_ANY, INADDR_BROADCAST, IPPROTO_ESP, IPPROTO_IPCOMP, IPPROTO_TCP, IPPROTO_UDP, SockaddrIn,
};
use crate::netinet::ip::Ip;
#[cfg(feature = "inet6")]
use crate::netinet::ip_ipsp::SENT_IP6;
use crate::netinet::ip_ipsp::{
    IPSEC_IN_USE, IPSEC_LAST_ADDED, IPSEC_POLICY_HEAD, IPSP_DENY, IPSP_DIRECTION_IN,
    IPSP_DIRECTION_OUT, IPSP_IPSEC_ACQUIRE, IPSP_IPSEC_DONTACQ, IPSP_IPSEC_REQUIRE, IPSP_IPSEC_USE,
    IPSP_PERMIT, IpsecAcquire, IpsecAcquireHead, IpsecIds, IpsecLevel, IpsecPolicy, SENT_IP4,
    SENT_LEN, SockaddrEncap, SockaddrUnion, TDBF_DELETED, TDBF_INVALID, Tdb, gettdbbydst,
    gettdbbysrc, ipsp_aux_match, ipsp_ids_free, ipsp_ids_match, ipsp_is_unspecified, tdb_ref,
    tdb_unref,
};
#[cfg(feature = "inet6")]
use crate::netinet::ip6::Ip6Hdr;
use crate::netinet::ipsec_input::IPSEC_EXPIRE_ACQUIRE;
#[cfg(feature = "inet6")]
use crate::netinet6::in6::{IN6MASK128, In6Addr, SockaddrIn6, in6_is_addr_unspecified};
#[cfg(feature = "inet6")]
use crate::netinet6::in6_src::in6_recoverscope;
use crate::sys::errno::Errno;
use crate::sys::mbuf::Mbuf;
use crate::sys::mutex::{Mutex, mutex_assert_locked};
use crate::sys::pool::{PR_NOWAIT, PR_ZERO, Pool};
use crate::sys::queue::TailqHead;
#[cfg(feature = "inet6")]
use crate::sys::socket::AF_INET6;
use crate::sys::socket::{AF_INET, PF_KEY};
use crate::sys::systm::{net_assert_locked, net_assert_locked_exclusive};
use crate::sys::timeout::Timeout;
use libkern::staticcell::StaticCell;

/// What `ipsp_spd_lookup` answers instead of "no IPsec" or "do IPsec".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpdError {
    /// The C's `-EINVAL`: silently drop the packet.
    Drop,
    /// Drop the packet and return this error.
    Errno(Errno),
}

impl From<Errno> for SpdError {
    fn from(e: Errno) -> Self {
        SpdError::Errno(e)
    }
}

/// `ipsec_policy_pool`.
pub static IPSEC_POLICY_POOL: Pool = Pool::new();
/// `ipsec_acquire_pool`.
pub static IPSEC_ACQUIRE_POOL: Pool = Pool::new();

/// `ipo_tdb_mtx`: links of policies to TDBs (\[P\]). For `tdb_walk()` calling
/// `tdb_delete_locked()` we need lock order `tdb_sadb_mtx` before `ipo_tdb_mtx`.
pub static IPO_TDB_MTX: Mutex = Mutex::new(IPL_SOFTNET);

/// A radix head pointer in `spd_tables` (`rn_inithead`'s, never freed).
#[derive(Clone, Copy)]
struct SpdTable(Option<&'static RadixNodeHead>);

// SAFETY: the heads are reached under the net lock only.
unsafe impl Send for SpdTable {}

/// `spd_tables`/`spd_table_max`: the SPD of each routing domain. Protected by the
/// `NET_LOCK()`.
static SPD_TABLES: StaticCell<Vec<SpdTable>> = StaticCell::new(Vec::new());

/// The SPD tables to read; the net lock is held, shared or exclusive.
fn spd_tables() -> &'static Vec<SpdTable> {
    // SAFETY: the vector changes only in `spd_tables_mut`, under the exclusive net lock,
    // which no reader (holding the net lock) runs beside; no caller keeps the reference past
    // the function that took it.
    unsafe { SPD_TABLES.get() }
}

/// The SPD tables to change; the net lock is held exclusively.
fn spd_tables_mut() -> &'static mut Vec<SpdTable> {
    // SAFETY: the exclusive net lock keeps out every other reader and writer; no caller
    // keeps the reference past the function that took it.
    unsafe { SPD_TABLES.get_mut() }
}

/// `ipsec_acquire_mtx`: the acquire lists (\[A\]).
pub static IPSEC_ACQUIRE_MTX: Mutex = Mutex::new(IPL_SOFTNET);

/// `ipsec_acquire_head`: every pending acquire.
pub static IPSEC_ACQUIRE_HEAD: AcquireListHead = AcquireListHead(TailqHead::new());

/// The `ipsec_acquire_head` list head.
pub struct AcquireListHead(pub TailqHead<IpsecAcquireHead>);

// SAFETY: changed only under `ipsec_acquire_mtx`.
unsafe impl Sync for AcquireListHead {}

/// `spd_table_get`: the SPD of the routing domain of `rtableid`, if there is one.
pub fn spd_table_get(rtableid: u32) -> Option<&'static RadixNodeHead> {
    net_assert_locked("spd_table_get");

    let tables = spd_tables();
    if tables.is_empty() {
        return None;
    }

    let rdomain = rtable_l2(rtableid) as usize;
    tables.get(rdomain)?.0
}

/// `spd_table_add`: the SPD of the routing domain of `rtableid`, made if missing.
pub fn spd_table_add(rtableid: u32) -> Option<&'static RadixNodeHead> {
    net_assert_locked_exclusive("spd_table_add");

    let tables = spd_tables_mut();
    let rdomain = rtable_l2(rtableid) as usize;
    if tables.len() <= rdomain {
        if tables.try_reserve(rdomain + 1 - tables.len()).is_err() {
            return None;
        }
        tables.resize(rdomain + 1, SpdTable(None));
    }

    if tables[rdomain].0.is_none() {
        let mut rnh = None;
        if !rn_inithead(&mut rnh, offset_of_sen_type() as i32) {
            rnh = None;
        }
        tables[rdomain] = SpdTable(rnh);
    }

    tables[rdomain].0
}

/// `offsetof(struct sockaddr_encap, sen_type)`: where the radix comparison starts.
const fn offset_of_sen_type() -> usize {
    2
}

/// The policy whose `ipo_nodes[0]` is the SPD leaf `rn` (the C's cast of the
/// `struct radix_node *` to `struct ipsec_policy *`).
///
/// # Safety
///
/// `rn` is a leaf of an SPD tree (from `rn_match`, `rn_lookup`, `rn_delete` or
/// `rn_walktree`), not one of its root leaves.
pub(crate) unsafe fn ipsec_policy_of(rn: &'static RadixNode) -> &'static IpsecPolicy {
    // SAFETY: `IpsecPolicy` is `#[repr(C)]` with `ipo_nodes` first, and the only node pairs
    // `pfkeyv2_flow` adds to an SPD are policies' `ipo_nodes`; a policy stays allocated until
    // `ipsec_delete_policy` took it out of the tree.
    unsafe { &*ptr::from_ref(rn).cast::<IpsecPolicy>() }
}

/// `spd_table_walk`: calls `func(ipo, tableid)` on every policy of the SPD of `rtableid`.
/// `EAGAIN` from the walker means the tree changed: the walk starts over.
pub fn spd_table_walk(
    rtableid: u32,
    mut func: impl FnMut(&'static IpsecPolicy, u32) -> Result<(), Errno>,
) -> Result<(), Errno> {
    let Some(rnh) = spd_table_get(rtableid) else {
        return Ok(());
    };

    // EGAIN means the tree changed.
    loop {
        let r = rn_walktree(rnh, |rn, tableid| {
            // SAFETY: a leaf of an SPD tree, which `rn_walktree` hands over.
            let ipo = unsafe { ipsec_policy_of(rn) };
            func(ipo, tableid)
        });
        match r {
            Err(Errno::EAGAIN) => continue,
            r => return r,
        }
    }
}

/// Copies `n` bytes at offset `off` of the packet into a fresh array.
fn copy_out<const N: usize>(m: &Mbuf, off: usize) -> [u8; N] {
    let mut b = [0u8; N];
    m_copydata(m, off as i32, &mut b);
    b
}

/// `ipsp_spd_lookup`: lookup at the SPD based on the headers contained on the mbuf. `af`
/// indicates what protocol family the header at the beginning of the mbuf is. `hlen` is the
/// offset of the transport protocol header in the mbuf.
///
/// Return combinations (of return value and `*tdbout`):
/// - `Err(Drop)` -> silently drop the packet
/// - `Err(Errno)` -> drop packet and return error
/// - `Ok`/`None` -> no IPsec required on packet
/// - `Ok`/TDB -> do IPsec
///
/// In the case of incoming flows, only the first three combinations are returned.
#[allow(clippy::too_many_arguments)] // the C's prototype
pub fn ipsp_spd_lookup(
    m: &Mbuf,
    af: i32,
    hlen: i32,
    direction: u8,
    tdbin: Option<&'static Tdb>,
    seclevel: Option<&IpsecLevel>,
    mut tdbout: Option<&mut Option<&'static Tdb>>,
    ipsecflowinfo_ids: Option<&'static IpsecIds>,
) -> Result<(), SpdError> {
    let mut signore = false;
    let mut dignore = false;

    net_assert_locked("ipsp_spd_lookup");

    // If there are no flows in place, there's no point continuing with the SPD lookup.
    if IPSEC_IN_USE.load(Ordering::Relaxed) == 0 {
        return ipsp_spd_inp(m, seclevel, None, tdbout);
    }

    // If an input packet is destined to a BYPASS socket, just accept it.
    if let Some(sl) = seclevel
        && direction == IPSP_DIRECTION_IN
        && i32::from(sl.sl_esp_trans) == IPSEC_LEVEL_BYPASS
        && i32::from(sl.sl_esp_network) == IPSEC_LEVEL_BYPASS
        && i32::from(sl.sl_auth) == IPSEC_LEVEL_BYPASS
    {
        if let Some(out) = tdbout {
            *out = None;
        }
        return Ok(());
    }

    let mut dst = SockaddrEncap::new();
    let mut sdst = SockaddrUnion::new();
    let mut ssrc = SockaddrUnion::new();
    dst.set_sen_family(PF_KEY);
    dst.set_sen_len(SENT_LEN as u8);

    match af {
        x if x == i32::from(AF_INET) => {
            if (hlen as usize) < size_of::<Ip>() || m.m_pkthdr().len.get() < hlen {
                return Err(Errno::EINVAL.into());
            }

            dst.set_sen_direction(direction);
            dst.set_sen_type(SENT_IP4);

            let src_b: [u8; 4] = copy_out(m, offset_of!(Ip, ip_src));
            let dst_b: [u8; 4] = copy_out(m, offset_of!(Ip, ip_dst));
            let p_b: [u8; 1] = copy_out(m, offset_of!(Ip, ip_p));
            dst.set_sen_ip_src(InAddr {
                s_addr: u32::from_ne_bytes(src_b),
            });
            dst.set_sen_ip_dst(InAddr {
                s_addr: u32::from_ne_bytes(dst_b),
            });
            dst.set_sen_proto(p_b[0]);

            ssrc.set_sin(&SockaddrIn {
                sin_family: AF_INET,
                sin_len: size_of::<SockaddrIn>() as u8,
                sin_addr: dst.sen_ip_src(),
                ..SockaddrIn::default()
            });
            sdst.set_sin(&SockaddrIn {
                sin_family: AF_INET,
                sin_len: size_of::<SockaddrIn>() as u8,
                sin_addr: dst.sen_ip_dst(),
                ..SockaddrIn::default()
            });

            // If TCP/UDP, extract the port numbers to use in the lookup.
            match i32::from(dst.sen_proto()) {
                IPPROTO_UDP | IPPROTO_TCP => {
                    // Make sure there's enough data in the packet.
                    if m.m_pkthdr().len.get() < hlen + 2 * 2 {
                        return Err(Errno::EINVAL.into());
                    }

                    // Luckily, the offset of the src/dst ports in both the UDP and TCP
                    // headers is the same (first two 16-bit values in the respective
                    // headers), so we can just copy them.
                    let sp: [u8; 2] = copy_out(m, hlen as usize);
                    let dp: [u8; 2] = copy_out(m, hlen as usize + 2);
                    dst.set_sen_sport(u16::from_ne_bytes(sp));
                    dst.set_sen_dport(u16::from_ne_bytes(dp));
                }
                _ => {
                    dst.set_sen_sport(0);
                    dst.set_sen_dport(0);
                }
            }
        }
        #[cfg(feature = "inet6")]
        x if x == i32::from(AF_INET6) => {
            if (hlen as usize) < size_of::<Ip6Hdr>() || m.m_pkthdr().len.get() < hlen {
                return Err(Errno::EINVAL.into());
            }

            dst.set_sen_type(SENT_IP6);
            dst.set_sen_ip6_direction(direction);

            let src_b: [u8; 16] = copy_out(m, offset_of!(Ip6Hdr, ip6_src));
            let dst_b: [u8; 16] = copy_out(m, offset_of!(Ip6Hdr, ip6_dst));
            let nxt_b: [u8; 1] = copy_out(m, offset_of!(Ip6Hdr, ip6_nxt));
            dst.set_sen_ip6_src(In6Addr::new(src_b));
            dst.set_sen_ip6_dst(In6Addr::new(dst_b));
            dst.set_sen_ip6_proto(nxt_b[0]);

            let mut s6src = SockaddrIn6 {
                sin6_family: AF_INET6,
                sin6_len: size_of::<SockaddrIn6>() as u8,
                ..SockaddrIn6::default()
            };
            let mut s6dst = s6src;
            in6_recoverscope(&mut s6src, &dst.sen_ip6_src());
            in6_recoverscope(&mut s6dst, &dst.sen_ip6_dst());
            ssrc.set_sin6(&s6src);
            sdst.set_sin6(&s6dst);

            // If TCP/UDP, extract the port numbers to use in the lookup.
            match i32::from(dst.sen_ip6_proto()) {
                IPPROTO_UDP | IPPROTO_TCP => {
                    // Make sure there's enough data in the packet.
                    if m.m_pkthdr().len.get() < hlen + 2 * 2 {
                        return Err(Errno::EINVAL.into());
                    }

                    // Luckily, the offset of the src/dst ports in both the UDP and TCP
                    // headers is the same (first two 16-bit values in the respective
                    // headers), so we can just copy them.
                    let sp: [u8; 2] = copy_out(m, hlen as usize);
                    let dp: [u8; 2] = copy_out(m, hlen as usize + 2);
                    dst.set_sen_ip6_sport(u16::from_ne_bytes(sp));
                    dst.set_sen_ip6_dport(u16::from_ne_bytes(dp));
                }
                _ => {
                    dst.set_sen_ip6_sport(0);
                    dst.set_sen_ip6_dport(0);
                }
            }
        }
        _ => return Err(Errno::EAFNOSUPPORT.into()),
    }

    // Actual SPD lookup.
    let rdomain = rtable_l2(m.m_pkthdr().ph_rtableid.get());
    let rn = spd_table_get(rdomain).and_then(|rnh| {
        // SAFETY: `dst` is a `SENT_LEN`-byte key whose first byte is its length.
        unsafe { rn_match(dst.as_bytes().as_ptr(), rnh) }
    });
    let Some(rn) = rn else {
        // Return whatever the socket requirements are, there are no system-wide policies.
        return ipsp_spd_inp(m, seclevel, None, tdbout);
    };
    // SAFETY: `rn_match` never returns a root node; the SPD's leaves are the first nodes of
    // policies, alive while the net lock is held.
    let ipo: &'static IpsecPolicy = unsafe { ipsec_policy_of(rn) };

    match ipo.ipo_type.get() {
        IPSP_PERMIT => return ipsp_spd_inp(m, seclevel, Some(ipo), tdbout),
        IPSP_DENY => return Err(Errno::EHOSTUNREACH.into()),
        IPSP_IPSEC_USE | IPSP_IPSEC_ACQUIRE | IPSP_IPSEC_REQUIRE | IPSP_IPSEC_DONTACQ => {
            // Nothing more needed here.
        }
        _ => return Err(Errno::EINVAL.into()),
    }

    let ipo_dst = ipo.ipo_dst.get();
    let ipo_src = ipo.ipo_src.get();

    // Check for non-specific destination in the policy.
    match ipo_dst.sa_family() {
        AF_INET => {
            if ipo_dst.sin_addr().s_addr == INADDR_ANY
                || ipo_dst.sin_addr().s_addr == INADDR_BROADCAST
            {
                dignore = true;
            }
        }
        #[cfg(feature = "inet6")]
        AF_INET6
            if in6_is_addr_unspecified(&ipo_dst.sin6_addr())
                || ipo_dst.sin6_addr() == IN6MASK128 =>
        {
            dignore = true;
        }
        _ => {}
    }

    // Likewise for source.
    match ipo_src.sa_family() {
        AF_INET => {
            if ipo_src.sin_addr().s_addr == INADDR_ANY {
                signore = true;
            }
        }
        #[cfg(feature = "inet6")]
        AF_INET6 if in6_is_addr_unspecified(&ipo_src.sin6_addr()) => signore = true,
        _ => {}
    }

    // Do we have a cached entry ? If so, check if it's still valid.
    mtx_enter(&IPO_TDB_MTX);
    if let Some(t) = ipo.ipo_tdb.get()
        && t.has_flags(TDBF_INVALID)
    {
        // SAFETY: `ipo_tdb_mtx` is held; the policy is on its cached TDB's list.
        unsafe { t.tdb_policy_head.remove(ipo) };
        tdb_unref(Some(t));
        ipo.ipo_tdb.set(None);
    }
    mtx_leave(&IPO_TDB_MTX);

    let ids_for = ipsecflowinfo_ids.or(ipo.ipo_ids.get());
    let ipo_addr = ipo.ipo_addr.get();
    let ipo_mask = ipo.ipo_mask.get();

    // Outgoing packet policy check.
    if direction == IPSP_DIRECTION_OUT {
        // If the packet is destined for the policy-specified gateway/endhost, and the socket
        // has the BYPASS option set, skip IPsec processing.
        if let Some(sl) = seclevel
            && i32::from(sl.sl_esp_trans) == IPSEC_LEVEL_BYPASS
            && i32::from(sl.sl_esp_network) == IPSEC_LEVEL_BYPASS
            && i32::from(sl.sl_auth) == IPSEC_LEVEL_BYPASS
        {
            // Direct match.
            let n = sdst.sa_bytes().len();
            if dignore || sdst.as_bytes()[..n] == ipo_dst.as_bytes()[..n] {
                if let Some(out) = tdbout {
                    *out = None;
                }
                return Ok(());
            }
        }

        // Check that the cached TDB (if present), is appropriate.
        mtx_enter(&IPO_TDB_MTX);
        if let Some(t) = ipo.ipo_tdb.get() {
            let want = if dignore { &sdst } else { &ipo_dst };
            let tdst = t.tdb_dst.get();
            let n = tdst.sa_bytes().len();
            let good = ipo.ipo_last_searched.get() > IPSEC_LAST_ADDED.load(Ordering::Relaxed)
                && ipo.ipo_sproto.get() == t.tdb_sproto.get()
                && want.as_bytes()[..n] == tdst.as_bytes()[..n]
                && ipsp_aux_match(t, ids_for, Some(&ipo_addr), Some(&ipo_mask));
            if good {
                // Cached entry is good.
                let error = ipsp_spd_inp(m, seclevel, Some(ipo), tdbout);
                mtx_leave(&IPO_TDB_MTX);
                return error;
            }

            // nomatchout: Cached TDB was not good.
            // SAFETY: `ipo_tdb_mtx` is held; the policy is on its cached TDB's list.
            unsafe { t.tdb_policy_head.remove(ipo) };
            tdb_unref(Some(t));
            ipo.ipo_tdb.set(None);
            ipo.ipo_last_searched.set(0);
        }

        // If no SA has been added since the last time we did a lookup, there's no point
        // searching for one. However, if the destination gateway is left unspecified (or is
        // all-1's), always lookup since this is a generic-match rule (otherwise, we can have
        // situations where SAs to some destinations exist but are not used, possibly leading
        // to an explosion in the number of acquired SAs).
        if ipo.ipo_last_searched.get() <= IPSEC_LAST_ADDED.load(Ordering::Relaxed) {
            // "Touch" the entry.
            if !dignore {
                ipo.ipo_last_searched.set(getuptime() as u64);
            }

            // gettdb() takes tdb_sadb_mtx, preserve lock order
            mtx_leave(&IPO_TDB_MTX);
            // Find an appropriate SA from the existing ones.
            let mut tdbp_new = gettdbbydst(
                rdomain,
                if dignore { &sdst } else { &ipo_dst },
                ipo.ipo_sproto.get(),
                ids_for,
                Some(&ipo_addr),
                Some(&ipo_mask),
            );
            mtx_enter(&IPO_TDB_MTX);
            if let Some(t) = tdbp_new
                && t.has_flags(TDBF_DELETED)
            {
                // After tdb_delete() has released ipo_tdb_mtx in tdb_unlink(), never add a
                // new one. tdb_cleanspd() has to catch all of them.
                tdb_unref(Some(t));
                tdbp_new = None;
            }
            if let Some(t) = ipo.ipo_tdb.get() {
                // Remove cached TDB from parallel thread.
                // SAFETY: `ipo_tdb_mtx` is held; the policy is on that TDB's list.
                unsafe { t.tdb_policy_head.remove(ipo) };
                tdb_unref(Some(t));
            }
            ipo.ipo_tdb.set(tdbp_new);
            if let Some(t) = tdbp_new {
                // gettdbbydst() has already refcounted tdb
                // SAFETY: `ipo_tdb_mtx` is held; the policy is on no TDB's list now.
                unsafe { t.tdb_policy_head.insert_tail(ipo) };
                let error = ipsp_spd_inp(m, seclevel, Some(ipo), tdbout);
                mtx_leave(&IPO_TDB_MTX);
                return error;
            }
        }
        mtx_leave(&IPO_TDB_MTX);

        // So, we don't have an SA -- just a policy.
        let gw = if dignore { &sdst } else { &ipo_dst };
        let laddr = if signore { None } else { Some(&ipo_src) };
        match ipo.ipo_type.get() {
            IPSP_IPSEC_REQUIRE => {
                // Acquire SA through key management.
                if ipsp_acquire_sa(ipo, gw, laddr, &dst, Some(m)).is_err() {
                    return Err(Errno::EACCES.into());
                }
                // FALLTHROUGH
                return Err(SpdError::Drop); // Silently drop packet.
            }
            IPSP_IPSEC_DONTACQ => return Err(SpdError::Drop), // Silently drop packet.
            IPSP_IPSEC_ACQUIRE => {
                // Acquire SA through key management.
                let _ = ipsp_acquire_sa(ipo, gw, laddr, &dst, None);
                // FALLTHROUGH
                return ipsp_spd_inp(m, seclevel, Some(ipo), tdbout);
            }
            IPSP_IPSEC_USE => return ipsp_spd_inp(m, seclevel, Some(ipo), tdbout),
            _ => {}
        }
    } else {
        // IPSP_DIRECTION_IN
        if let Some(mut tdbin) = tdbin {
            'nomatchin: {
                // Special case for bundled IPcomp/ESP SAs:
                // 1) only IPcomp flows are loaded into kernel
                // 2) input processing processes ESP SA first
                // 3) then optional IPcomp processing happens
                // 4) we only update m_tag for ESP
                // => 'tdbin' is always set to ESP SA
                // => flow has ipo_proto for IPcomp
                // So if 'tdbin' points to an ESP SA and this 'tdbin' is bundled with an
                // IPcomp SA, then we replace 'tdbin' with the IPcomp SA at
                // tdbin->tdb_inext.
                if i32::from(ipo.ipo_sproto.get()) == IPPROTO_IPCOMP
                    && i32::from(tdbin.tdb_sproto.get()) == IPPROTO_ESP
                    && let Some(inext) = tdbin.tdb_inext.get()
                    && i32::from(inext.tdb_sproto.get()) == IPPROTO_IPCOMP
                {
                    tdbin = inext;
                }

                // Direct match in the cache.
                mtx_enter(&IPO_TDB_MTX);
                if ipo.ipo_tdb.get().is_some_and(|t| ptr::eq(t, tdbin)) {
                    let error = ipsp_spd_inp(m, seclevel, Some(ipo), tdbout);
                    mtx_leave(&IPO_TDB_MTX);
                    return error;
                }
                mtx_leave(&IPO_TDB_MTX);

                let tsrc = tdbin.tdb_src.get();
                let n = tsrc.sa_bytes().len();
                let want = if dignore { &ssrc } else { &ipo_dst };
                if want.as_bytes()[..n] != tsrc.as_bytes()[..n]
                    || ipo.ipo_sproto.get() != tdbin.tdb_sproto.get()
                {
                    break 'nomatchin;
                }

                // Match source/dest IDs.
                if let Some(ids) = ipo.ipo_ids.get() {
                    match tdbin.tdb_ids.get() {
                        Some(t) if ipsp_ids_match(ids, t) => {}
                        _ => break 'nomatchin,
                    }
                }

                // Add it to the cache.
                mtx_enter(&IPO_TDB_MTX);
                if let Some(t) = ipo.ipo_tdb.get() {
                    // SAFETY: `ipo_tdb_mtx` is held; the policy is on that TDB's list.
                    unsafe { t.tdb_policy_head.remove(ipo) };
                    tdb_unref(Some(t));
                }
                ipo.ipo_tdb.set(tdb_ref(Some(tdbin)));
                // SAFETY: `ipo_tdb_mtx` is held; the policy is on no TDB's list now.
                unsafe { tdbin.tdb_policy_head.insert_tail(ipo) };
                let error = ipsp_spd_inp(m, seclevel, Some(ipo), tdbout.as_deref_mut());
                mtx_leave(&IPO_TDB_MTX);
                return error;
            }
            // nomatchin: Nothing needed here, falling through
        }

        // Check whether cached entry applies.
        mtx_enter(&IPO_TDB_MTX);
        'skipinputsearch: {
            if let Some(t) = ipo.ipo_tdb.get() {
                // We only need to check that the correct security protocol and security
                // gateway are set; IDs will be the same since the cached entry is linked on
                // this policy.
                let tsrc = t.tdb_src.get();
                let n = tsrc.sa_bytes().len();
                let want = if dignore { &ssrc } else { &ipo_dst };
                if ipo.ipo_sproto.get() == t.tdb_sproto.get()
                    && tsrc.as_bytes()[..n] == want.as_bytes()[..n]
                {
                    break 'skipinputsearch;
                }

                // Not applicable, unlink.
                // SAFETY: `ipo_tdb_mtx` is held; the policy is on its cached TDB's list.
                unsafe { t.tdb_policy_head.remove(ipo) };
                tdb_unref(Some(t));
                ipo.ipo_tdb.set(None);
                ipo.ipo_last_searched.set(0);
            }

            // Find whether there exists an appropriate SA.
            if ipo.ipo_last_searched.get() <= IPSEC_LAST_ADDED.load(Ordering::Relaxed) {
                if !dignore {
                    ipo.ipo_last_searched.set(getuptime() as u64);
                }

                // gettdb() takes tdb_sadb_mtx, preserve lock order
                mtx_leave(&IPO_TDB_MTX);
                let mut tdbp_new = gettdbbysrc(
                    rdomain,
                    if dignore { &ssrc } else { &ipo_dst },
                    ipo.ipo_sproto.get(),
                    ipo.ipo_ids.get(),
                    Some(&ipo_addr),
                    Some(&ipo_mask),
                );
                mtx_enter(&IPO_TDB_MTX);
                if let Some(t) = tdbp_new
                    && t.has_flags(TDBF_DELETED)
                {
                    // After tdb_delete() has released ipo_tdb_mtx in tdb_unlink(), never add
                    // a new one. tdb_cleanspd() has to catch all of them.
                    tdb_unref(Some(t));
                    tdbp_new = None;
                }
                if let Some(t) = ipo.ipo_tdb.get() {
                    // Remove cached TDB from parallel thread.
                    // SAFETY: `ipo_tdb_mtx` is held; the policy is on that TDB's list.
                    unsafe { t.tdb_policy_head.remove(ipo) };
                    tdb_unref(Some(t));
                }
                ipo.ipo_tdb.set(tdbp_new);
                if let Some(t) = tdbp_new {
                    // gettdbbysrc() has already refcounted tdb
                    // SAFETY: `ipo_tdb_mtx` is held; the policy is on no TDB's list now.
                    unsafe { t.tdb_policy_head.insert_tail(ipo) };
                }
            }
        }
        // skipinputsearch:
        mtx_leave(&IPO_TDB_MTX);

        let gw = if dignore { &ssrc } else { &ipo_dst };
        let laddr = if signore { None } else { Some(&ipo_src) };
        match ipo.ipo_type.get() {
            IPSP_IPSEC_REQUIRE => {
                // If appropriate SA exists, don't acquire another.
                if ipo.ipo_tdb.get().is_some() {
                    return Err(SpdError::Drop); // Silently drop packet.
                }

                // Acquire SA through key management.
                ipsp_acquire_sa(ipo, gw, laddr, &dst, Some(m))?;

                // FALLTHROUGH
                return Err(SpdError::Drop); // Silently drop packet.
            }
            IPSP_IPSEC_DONTACQ => return Err(SpdError::Drop), // Silently drop packet.
            IPSP_IPSEC_ACQUIRE => {
                // If appropriate SA exists, don't acquire another.
                if ipo.ipo_tdb.get().is_some() {
                    return ipsp_spd_inp(m, seclevel, Some(ipo), tdbout);
                }

                // Acquire SA through key management.
                let _ = ipsp_acquire_sa(ipo, gw, laddr, &dst, None);

                // FALLTHROUGH
                return ipsp_spd_inp(m, seclevel, Some(ipo), tdbout);
            }
            IPSP_IPSEC_USE => return ipsp_spd_inp(m, seclevel, Some(ipo), tdbout),
            _ => {}
        }
    }

    // Shouldn't ever get this far.
    Err(Errno::EINVAL.into())
}

/// `ipsec_delete_policy`: delete a policy from the SPD (on the last reference).
pub fn ipsec_delete_policy(ipo: &'static IpsecPolicy) -> Result<(), Errno> {
    net_assert_locked_exclusive("ipsec_delete_policy");

    if !refcnt_rele(&ipo.ipo_refcnt) {
        return Ok(());
    }

    // Delete from SPD.
    let rnh = spd_table_get(ipo.ipo_rdomain.get()).ok_or(Errno::ESRCH)?;
    // SAFETY: the policy's key and mask are `SENT_LEN`-byte encap addresses; its nodes are in
    // this tree.
    let deleted = unsafe {
        rn_delete(
            ipo.ipo_addr_key(),
            ipo.ipo_mask_key(),
            rnh,
            Some(&ipo.ipo_nodes[0]),
        )
    };
    if deleted.is_none() {
        return Err(Errno::ESRCH);
    }

    mtx_enter(&IPO_TDB_MTX);
    if let Some(t) = ipo.ipo_tdb.get() {
        // SAFETY: `ipo_tdb_mtx` is held; the policy is on its cached TDB's list.
        unsafe { t.tdb_policy_head.remove(ipo) };
        tdb_unref(Some(t));
        ipo.ipo_tdb.set(None);
    }
    mtx_leave(&IPO_TDB_MTX);

    mtx_enter(&IPSEC_ACQUIRE_MTX);
    while let Some(ipa) = ipo.ipo_acquires.first() {
        // SAFETY: acquires are pool items alive while on the lists.
        ipsp_delete_acquire_locked(unsafe { &*ptr::from_ref(ipa) });
    }
    mtx_leave(&IPSEC_ACQUIRE_MTX);

    // SAFETY: the exclusive net lock is held; every policy in the SPD is on the list.
    unsafe { IPSEC_POLICY_HEAD.0.remove(ipo) };

    if let Some(ids) = ipo.ipo_ids.get() {
        ipsp_ids_free(Some(ids));
    }

    IPSEC_IN_USE.fetch_sub(1, Ordering::Relaxed);

    pool_put(&IPSEC_POLICY_POOL, NonNull::from(ipo).cast());

    Ok(())
}

/// The acquire record a timeout was set up with.
///
/// # Safety
///
/// `v` is the argument `ipsp_acquire_sa` gave the timeout: an acquire that holds a
/// reference for the pending timeout.
unsafe fn ipa_of(v: *mut c_void) -> &'static IpsecAcquire {
    // SAFETY: the caller's contract.
    unsafe { &*v.cast::<IpsecAcquire>() }
}

/// `ipsp_delete_acquire_timer`: the acquire expired.
fn ipsp_delete_acquire_timer(v: *mut c_void) {
    // SAFETY: set up by `ipsp_acquire_sa`; the pending timeout held a reference.
    let ipa = unsafe { ipa_of(v) };

    mtx_enter(&IPSEC_ACQUIRE_MTX);
    refcnt_rele(&ipa.ipa_refcnt);
    ipsp_delete_acquire_locked(ipa);
    mtx_leave(&IPSEC_ACQUIRE_MTX);
}

/// `ipsp_delete_acquire`: delete a pending IPsec acquire record.
pub fn ipsp_delete_acquire(ipa: &'static IpsecAcquire) {
    mtx_enter(&IPSEC_ACQUIRE_MTX);
    ipsp_delete_acquire_locked(ipa);
    mtx_leave(&IPSEC_ACQUIRE_MTX);
}

/// `ipsp_delete_acquire_locked`: `ipsp_delete_acquire` with `ipsec_acquire_mtx` held.
pub fn ipsp_delete_acquire_locked(ipa: &'static IpsecAcquire) {
    if timeout_del(&ipa.ipa_timeout) {
        refcnt_rele(&ipa.ipa_refcnt);
    }
    ipsp_unref_acquire_locked(ipa);
}

/// `ipsec_unref_acquire`: drops a reference `ipsec_get_acquire` took.
pub fn ipsec_unref_acquire(ipa: &'static IpsecAcquire) {
    mtx_enter(&IPSEC_ACQUIRE_MTX);
    ipsp_unref_acquire_locked(ipa);
    mtx_leave(&IPSEC_ACQUIRE_MTX);
}

/// `ipsp_unref_acquire_locked`: drops a reference; the last one unlinks and frees the record.
pub fn ipsp_unref_acquire_locked(ipa: &'static IpsecAcquire) {
    mutex_assert_locked(&IPSEC_ACQUIRE_MTX, "ipsp_unref_acquire_locked");

    if !refcnt_rele(&ipa.ipa_refcnt) {
        return;
    }
    // SAFETY: `ipsec_acquire_mtx` is held; a live acquire is on both lists.
    unsafe {
        IPSEC_ACQUIRE_HEAD.0.remove(ipa);
        if let Some(ipo) = ipa.ipa_policy.get() {
            ipo.ipo_acquires.remove(ipa);
        }
    }
    ipa.ipa_policy.set(None);

    pool_put(&IPSEC_ACQUIRE_POOL, NonNull::from(ipa).cast());
}

/// `ipsp_pending_acquire`: find out if there's an ACQUIRE pending. XXX Need a better
/// structure.
fn ipsp_pending_acquire(ipo: &IpsecPolicy, gw: &SockaddrUnion) -> bool {
    net_assert_locked("ipsp_pending_acquire");

    mtx_enter(&IPSEC_ACQUIRE_MTX);
    let n = gw.sa_bytes().len();
    let found = ipo
        .ipo_acquires
        .iter()
        .any(|ipa| gw.as_bytes()[..n] == ipa.ipa_addr.as_bytes()[..n]);
    mtx_leave(&IPSEC_ACQUIRE_MTX);

    found
}

/// `ipsp_acquire_sa`: signal key management that we need an SA. XXX For outgoing policies,
/// we could try to hold on to the mbuf.
pub fn ipsp_acquire_sa(
    ipo: &'static IpsecPolicy,
    gw: &SockaddrUnion,
    laddr: Option<&SockaddrUnion>,
    ddst: &SockaddrEncap,
    _m: Option<&Mbuf>,
) -> Result<(), Errno> {
    net_assert_locked("ipsp_acquire_sa");

    // Check whether request has been made already.
    if ipsp_pending_acquire(ipo, gw) {
        return Ok(());
    }

    let ipo_addr = ipo.ipo_addr.get();
    let ipo_mask = ipo.ipo_mask.get();

    let mut info = SockaddrEncap::new();
    let mut mask = SockaddrEncap::new();
    info.set_sen_len(SENT_LEN as u8);
    mask.set_sen_len(SENT_LEN as u8);
    info.set_sen_family(PF_KEY);
    mask.set_sen_family(PF_KEY);

    // Just copy the right information.
    match ipo_addr.sen_type() {
        SENT_IP4 => {
            info.set_sen_type(SENT_IP4);
            mask.set_sen_type(SENT_IP4);
            info.set_sen_direction(ipo_addr.sen_direction());
            mask.set_sen_direction(ipo_mask.sen_direction());

            if ipsp_is_unspecified(ipo.ipo_dst.get()) {
                info.set_sen_ip_src(ddst.sen_ip_src());
                mask.set_sen_ip_src(InAddr {
                    s_addr: INADDR_BROADCAST,
                });

                info.set_sen_ip_dst(ddst.sen_ip_dst());
                mask.set_sen_ip_dst(InAddr {
                    s_addr: INADDR_BROADCAST,
                });
            } else {
                info.set_sen_ip_src(ipo_addr.sen_ip_src());
                mask.set_sen_ip_src(ipo_mask.sen_ip_src());

                info.set_sen_ip_dst(ipo_addr.sen_ip_dst());
                mask.set_sen_ip_dst(ipo_mask.sen_ip_dst());
            }

            info.set_sen_proto(ipo_addr.sen_proto());
            mask.set_sen_proto(ipo_mask.sen_proto());

            if ipo_addr.sen_proto() != 0 {
                info.set_sen_sport(ipo_addr.sen_sport());
                mask.set_sen_sport(ipo_mask.sen_sport());

                info.set_sen_dport(ipo_addr.sen_dport());
                mask.set_sen_dport(ipo_mask.sen_dport());
            }
        }
        #[cfg(feature = "inet6")]
        SENT_IP6 => {
            info.set_sen_type(SENT_IP6);
            mask.set_sen_type(SENT_IP6);
            info.set_sen_ip6_direction(ipo_addr.sen_ip6_direction());
            mask.set_sen_ip6_direction(ipo_mask.sen_ip6_direction());

            if ipsp_is_unspecified(ipo.ipo_dst.get()) {
                info.set_sen_ip6_src(ddst.sen_ip6_src());
                mask.set_sen_ip6_src(IN6MASK128);

                info.set_sen_ip6_dst(ddst.sen_ip6_dst());
                mask.set_sen_ip6_dst(IN6MASK128);
            } else {
                info.set_sen_ip6_src(ipo_addr.sen_ip6_src());
                mask.set_sen_ip6_src(ipo_mask.sen_ip6_src());

                info.set_sen_ip6_dst(ipo_addr.sen_ip6_dst());
                mask.set_sen_ip6_dst(ipo_mask.sen_ip6_dst());
            }

            info.set_sen_ip6_proto(ipo_addr.sen_ip6_proto());
            mask.set_sen_ip6_proto(ipo_mask.sen_ip6_proto());

            if ipo_mask.sen_ip6_proto() != 0 {
                info.set_sen_ip6_sport(ipo_addr.sen_ip6_sport());
                mask.set_sen_ip6_sport(ipo_mask.sen_ip6_sport());

                info.set_sen_ip6_dport(ipo_addr.sen_ip6_dport());
                mask.set_sen_ip6_dport(ipo_mask.sen_ip6_dport());
            }
        }
        _ => return Ok(()),
    }

    // Add request in cache and proceed.
    let Some(mem) = pool_get(&IPSEC_ACQUIRE_POOL, PR_NOWAIT | PR_ZERO) else {
        return Err(Errno::ENOMEM);
    };
    let raw = mem.cast::<IpsecAcquire>().as_ptr();
    // SAFETY: a fresh, suitably aligned `ipsec_acquire_pool` item, written once before
    // anything else sees it; it stays allocated until `ipsp_unref_acquire_locked`.
    unsafe {
        raw.write(IpsecAcquire {
            ipa_addr: *gw,
            ipa_seq: core::cell::Cell::new(0),
            ipa_info: info,
            ipa_mask: mask,
            ipa_refcnt: crate::sys::refcnt::Refcnt::new(),
            ipa_timeout: Timeout::zeroed(),
            ipa_policy: core::cell::Cell::new(None),
            ipa_ipo_next: crate::sys::queue::TailqEntry::new(),
            ipa_next: crate::sys::queue::TailqEntry::new(),
        })
    };
    // SAFETY: as above.
    let ipa: &'static IpsecAcquire = unsafe { &*raw };

    refcnt_init(&ipa.ipa_refcnt);
    timeout_set(&ipa.ipa_timeout, ipsp_delete_acquire_timer, raw.cast());

    mtx_enter(&IPSEC_ACQUIRE_MTX);
    if timeout_add_sec(
        &ipa.ipa_timeout,
        IPSEC_EXPIRE_ACQUIRE.load(Ordering::Relaxed),
    ) {
        refcnt_take(&ipa.ipa_refcnt);
    }
    // SAFETY: `ipsec_acquire_mtx` is held; the record is new, on no list.
    unsafe {
        IPSEC_ACQUIRE_HEAD.0.insert_tail(ipa);
        ipo.ipo_acquires.insert_tail(ipa);
    }
    ipa.ipa_policy.set(Some(ipo));
    mtx_leave(&IPSEC_ACQUIRE_MTX);

    // PF_KEYv2 notification message.
    pfkeyv2_acquire(ipo, gw, laddr, &ipa.ipa_seq, ddst)
}

/// `ipsp_spd_inp`: deal with PCB security requirements.
fn ipsp_spd_inp(
    _m: &Mbuf,
    seclevel: Option<&IpsecLevel>,
    ipo: Option<&'static IpsecPolicy>,
    tdbout: Option<&mut Option<&'static Tdb>>,
) -> Result<(), SpdError> {
    // Sanity check.
    if let Some(sl) = seclevel {
        // We only support IPSEC_LEVEL_BYPASS or IPSEC_LEVEL_AVAIL
        let all = |l: i32| {
            i32::from(sl.sl_esp_trans) == l
                && i32::from(sl.sl_esp_network) == l
                && i32::from(sl.sl_auth) == l
        };
        if !all(IPSEC_LEVEL_BYPASS) && !all(IPSEC_LEVEL_AVAIL) {
            return Err(SpdError::Drop); // Silently drop packet.
        }
    }

    // justreturn:
    if let Some(out) = tdbout {
        *out = match ipo {
            Some(ipo) => tdb_ref(ipo.ipo_tdb.get()),
            None => None,
        };
    }
    Ok(())
}

/// `ipsec_get_acquire`: find a pending ACQUIRE record based on its sequence number, with a
/// reference. XXX Need to use a better data structure.
pub fn ipsec_get_acquire(seq: u32) -> Option<&'static IpsecAcquire> {
    net_assert_locked("ipsec_get_acquire");

    mtx_enter(&IPSEC_ACQUIRE_MTX);
    let ipa = IPSEC_ACQUIRE_HEAD
        .0
        .iter()
        .find(|ipa| ipa.ipa_seq.get() == seq)
        .map(|ipa| {
            refcnt_take(&ipa.ipa_refcnt);
            // SAFETY: acquires are pool items alive while referenced.
            unsafe { &*ptr::from_ref(ipa) }
        });
    mtx_leave(&IPSEC_ACQUIRE_MTX);

    ipa
}

/// Forgets every SPD table: the host tests start from an empty database.
#[cfg(test)]
pub(crate) fn spd_reset() {
    spd_tables_mut().clear();
    IPSEC_ACQUIRE_HEAD.0.init();
}

const _: () = assert!(offset_of_sen_type() == 2);
/* </CODE> */
