/*	$OpenBSD: pf_lb.c,v 1.78 2026/05/12 09:34:00 henning Exp $ */
/* <LICENSES> */

/*
 * Copyright (c) 2001 Daniel Hartmeier
 * Copyright (c) 2002 - 2008 Henning Brauer
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 *
 *    - Redistributions of source code must retain the above copyright
 *      notice, this list of conditions and the following disclaimer.
 *    - Redistributions in binary form must reproduce the above
 *      copyright notice, this list of conditions and the following
 *      disclaimer in the documentation and/or other materials provided
 *      with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
 * "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
 * LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS
 * FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE
 * COPYRIGHT HOLDERS OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT,
 * INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING,
 * BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
 * LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
 * CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN
 * ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 *
 * Effort sponsored in part by the Defense Advanced Research Projects
 * Agency (DARPA) and Air Force Research Laboratory, Air Force
 * Materiel Command, USAF, under agreement number F30602-01-2-0537.
 *
 */
/* </LICENSES> */

/* <CODE> */
//! pf's address and port selection for translation and routing: the `nat-to`, `rdr-to` and
//! `route-to` pools (`bitmask`, `random`, `source-hash`, `round-robin`, `least-states`,
//! `sticky-address`) and the source port search of `nat-to`.
//!
//! Upstream: sys/net/pf_lb.c @ 3ce1f3f79392
//!
//! A pool's address is an address and mask, the addresses of an interface (`(ifname)`,
//! `struct pfi_dynaddr`) or a table; the round-robin position lives in the pool's `counter`.
//! The C points `raddr`/`rmask` at the pool, the dynamic address or the table entry that
//! `pfr_pool_get` picked; here they are copies of those addresses, which nothing changes
//! while `pf_map_addr` runs (net lock and `pf_lock`).
//!
//! ## Deviations
//! - The C's 0/-1 and 0/1 status returns (`pf_get_sport`, `pf_map_addr`,
//!   `pf_map_addr_sticky`, `pf_map_addr_states_increase`, `pf_get_transaddr`,
//!   `pf_postprocess_addr`, `pf_pool_states_decrease_addr`) are `bool`, `true` for the C's
//!   0 (`docs/C_TO_RUST.md`).
//! - `pfr_pool_get` returns the entry's address and mask (`net/pf_table.rs`) instead of
//!   pointing `raddr`/`rmask` at them (see above).

use crate::crypto::siphash::{SipHash24, SiphashKey};
use crate::dev::rnd::arc4random_uniform;
use crate::kern::subr_prf::{addlog, log};
use crate::net::if_::unhandled_af;
use crate::net::pf::{
    PF_STATUS, TREE_ID, TREE_SRC_TRACKING, pf_addr_inc, pf_addrcpy, pf_debug, pf_find_state_all,
    pf_inc, pf_insert_src_node, pf_match_addr, pf_pkt_hash, pf_poolmask, pf_print_host,
    pf_remove_src_node, pf_state_rm_src_node,
};
use crate::net::pf_table::{
    pfr_kentry_byaddr, pfr_ktable_select_active, pfr_pool_get, pfr_states_decrease,
    pfr_states_increase,
};
use crate::net::pfvar::{
    PF_ADDR_DYNIFTL, PF_ADDR_NONE, PF_ADDR_NOROUTE, PF_ADDR_TABLE, PF_IN, PF_OUT, PF_POOL_BITMASK,
    PF_POOL_LEASTSTATES, PF_POOL_NONE, PF_POOL_RANDOM, PF_POOL_ROUNDROBIN, PF_POOL_SRCHASH,
    PF_POOL_STICKYADDR, PF_POOL_TYPEMASK, PF_SK_STACK, PF_SK_WIRE, PF_SN_MAX, PF_SN_NAT, PF_SN_RDR,
    PfAddr, PfPool, PfPoolhashkey, PfRule, PfSnTypes, PfSrcNode, PfStateKeyCmp,
    SCNT_SRC_NODE_SEARCH, pf_aeq, pf_azero, pf_pool_dyntype,
};
use crate::net::pfvar_priv::{PfPdesc, PfState};
use crate::netinet::in_::{
    INADDR_BROADCAST, IPPROTO_ICMP, IPPROTO_ICMPV6, IPPROTO_TCP, IPPROTO_UDP,
};
use crate::netinet::in_pcb::in_baddynamic;
use crate::netinet::ip_icmp::ICMP_ECHO;
use crate::sys::socket::AF_INET;
#[cfg(feature = "inet6")]
use crate::sys::socket::AF_INET6;
use crate::sys::syslog::{LOG_DEBUG, LOG_ERR, LOG_INFO, LOG_NOTICE};
use crate::sys::types::SaFamily;

/// `pf_hash`: the keyed hash of `inaddr` under `key`, stored in `hash` and returned.
pub fn pf_hash(inaddr: &PfAddr, hash: &mut PfAddr, key: &PfPoolhashkey, af: SaFamily) -> u64 {
    // (SIPHASH_KEY *)key: the 16 key bytes as the two words.
    let mut k0 = [0u8; 8];
    let mut k1 = [0u8; 8];
    k0.copy_from_slice(&key.key8[..8]);
    k1.copy_from_slice(&key.key8[8..]);
    let skey = SiphashKey {
        k0: u64::from_ne_bytes(k0),
        k1: u64::from_ne_bytes(k1),
    };

    match af {
        AF_INET => {
            let res = SipHash24(&skey, &inaddr.addr8[..4]);
            hash.set_addr32(0, res as u32);
            res
        }
        #[cfg(feature = "inet6")]
        AF_INET6 => {
            let res = SipHash24(&skey, &inaddr.addr8);
            // The union of hash64 and hash32[2].
            let b = res.to_ne_bytes();
            let h0 = u32::from_ne_bytes([b[0], b[1], b[2], b[3]]);
            let h1 = u32::from_ne_bytes([b[4], b[5], b[6], b[7]]);
            hash.set_addr32(0, h0);
            hash.set_addr32(1, h1);
            // siphash isn't big enough, but flipping it around is good enough here.
            hash.set_addr32(2, !h1);
            hash.set_addr32(3, !h0);
            res
        }
        _ => unhandled_af(i32::from(af)),
    }
}

/// `pf_get_sport`: picks the translated source address (`naddr`) and a free source port
/// (`nport`, network order) in `low..=high` for `nat-to`; `true` (the C's 0) when found.
#[allow(clippy::too_many_arguments)]
pub fn pf_get_sport(
    pd: &mut PfPdesc,
    r: &'static PfRule,
    naddr: &mut PfAddr,
    nport: &mut u16,
    low: u16,
    high: u16,
    sn: &mut [Option<&'static PfSrcNode>; PF_SN_MAX],
) -> bool {
    let (mut low, mut high) = (low, high);
    let dir = if pd.dir == PF_IN { PF_OUT } else { PF_IN };
    let sidx = usize::from(pd.sidx);
    let didx = usize::from(pd.didx);

    let mut init_addr = PfAddr::zeroed();
    let nsaddr = pd.nsaddr;
    if !pf_map_addr(
        pd.naf,
        r,
        &nsaddr,
        naddr,
        Some(&mut init_addr),
        sn,
        &r.nat,
        PF_SN_NAT,
    ) {
        return false;
    }

    if i32::from(pd.proto) == IPPROTO_ICMP {
        if pd.ndport == u16::from(ICMP_ECHO).to_be() {
            low = 1;
            high = 65535;
        } else {
            return true; // Don't try to modify non-echo ICMP
        }
    }
    #[cfg(feature = "inet6")]
    if i32::from(pd.proto) == IPPROTO_ICMPV6 {
        if pd.ndport == u16::from(crate::netinet::icmp6::ICMP6_ECHO_REQUEST).to_be() {
            low = 1;
            high = 65535;
        } else {
            return true; // Don't try to modify non-echo ICMP
        }
    }

    loop {
        let mut key = PfStateKeyCmp {
            af: pd.naf,
            proto: pd.proto,
            rdomain: pd.rdomain,
            ..PfStateKeyCmp::default()
        };
        let nd = pd.ndaddr;
        pf_addrcpy(&mut key.addr[didx], &nd, key.af);
        pf_addrcpy(&mut key.addr[sidx], naddr, key.af);
        key.port[didx] = pd.ndport;

        // port search; start random, step;
        // similar 2 portloop in in_pcbbind
        let proto = i32::from(pd.proto);
        if !(proto == IPPROTO_TCP
            || proto == IPPROTO_UDP
            || proto == IPPROTO_ICMP
            || proto == IPPROTO_ICMPV6)
        {
            // XXX bug: icmp states dont use the id on both
            // XXX sides (traceroute -I through nat)
            key.port[sidx] = pd.nsport;
            key.hash = pf_pkt_hash(
                key.af,
                key.proto,
                &key.addr[0],
                &key.addr[1],
                key.port[0],
                key.port[1],
            );
            if pf_find_state_all(&key, dir, None).is_none() {
                *nport = pd.nsport;
                return true;
            }
        } else if low == 0 && high == 0 {
            key.port[sidx] = pd.nsport;
            key.hash = pf_pkt_hash(
                key.af,
                key.proto,
                &key.addr[0],
                &key.addr[1],
                key.port[0],
                key.port[1],
            );
            if pf_find_state_all(&key, dir, None).is_none() {
                *nport = pd.nsport;
                return true;
            }
        } else if low == high {
            key.port[sidx] = low.to_be();
            key.hash = pf_pkt_hash(
                key.af,
                key.proto,
                &key.addr[0],
                &key.addr[1],
                key.port[0],
                key.port[1],
            );
            if pf_find_state_all(&key, dir, None).is_none() {
                *nport = low.to_be();
                return true;
            }
        } else {
            if low > high {
                core::mem::swap(&mut low, &mut high);
            }
            // low < high
            let cut: u16 =
                (arc4random_uniform(1 + u32::from(high) - u32::from(low)) + u32::from(low)) as u16;
            // low <= cut <= high
            let mut tmp = u32::from(cut);
            while tmp <= u32::from(high) && tmp <= 0xffff {
                key.port[sidx] = (tmp as u16).to_be();
                key.hash = pf_pkt_hash(
                    key.af,
                    key.proto,
                    &key.addr[0],
                    &key.addr[1],
                    key.port[0],
                    key.port[1],
                );
                if pf_find_state_all(&key, dir, None).is_none()
                    && !in_baddynamic(tmp as u16, u16::from(pd.proto))
                {
                    *nport = (tmp as u16).to_be();
                    return true;
                }
                tmp += 1;
            }
            tmp = u32::from(cut).wrapping_sub(1);
            while tmp >= u32::from(low) && tmp <= 0xffff {
                key.port[sidx] = (tmp as u16).to_be();
                key.hash = pf_pkt_hash(
                    key.af,
                    key.proto,
                    &key.addr[0],
                    &key.addr[1],
                    key.port[0],
                    key.port[1],
                );
                if pf_find_state_all(&key, dir, None).is_none()
                    && !in_baddynamic(tmp as u16, u16::from(pd.proto))
                {
                    *nport = (tmp as u16).to_be();
                    return true;
                }
                tmp = tmp.wrapping_sub(1);
            }
        }

        match r.nat.opts.get() & PF_POOL_TYPEMASK {
            PF_POOL_RANDOM | PF_POOL_ROUNDROBIN | PF_POOL_LEASTSTATES => {
                // pick a different source address since we're out
                // of free port choices for the current one.
                let nsaddr = pd.nsaddr;
                if !pf_map_addr(
                    pd.naf,
                    r,
                    &nsaddr,
                    naddr,
                    Some(&mut init_addr),
                    sn,
                    &r.nat,
                    PF_SN_NAT,
                ) {
                    return false;
                }
            }
            // PF_POOL_NONE, PF_POOL_SRCHASH, PF_POOL_BITMASK and the rest
            _ => return false,
        }
        if pf_aeq(&init_addr, naddr, pd.naf) {
            break;
        }
    }
    false // none available
}

/// `pf_map_addr_sticky`: the address the source node of `saddr` remembers for `rpool`
/// (`sticky-address`); `true` (the C's 0) when it is still valid, with `naddr` set.
#[allow(clippy::too_many_arguments)]
pub fn pf_map_addr_sticky(
    af: SaFamily,
    r: &'static PfRule,
    saddr: &PfAddr,
    naddr: &mut PfAddr,
    sns: &mut [Option<&'static PfSrcNode>; PF_SN_MAX],
    rpool: &'static PfPool,
    type_: PfSnTypes,
) -> bool {
    let k = PfSrcNode::zeroed();
    k.af.set(af);
    k.type_.set(type_ as u8);
    let mut a = PfAddr::zeroed();
    pf_addrcpy(&mut a, saddr, af);
    k.addr.set(a);
    k.rule.set_ptr(Some(r));
    pf_inc(&PF_STATUS.scounters[SCNT_SRC_NODE_SEARCH]);
    sns[type_] = TREE_SRC_TRACKING.find(&k);
    let Some(sn) = sns[type_] else {
        return false;
    };

    // check if the cached entry is still valid
    let cached = sn.raddr.get();
    let valid = if pf_azero(&cached, af) {
        true
    } else if rpool.addr.type_.get() == PF_ADDR_DYNIFTL {
        rpool
            .addr
            .dyn_()
            .and_then(|d| d.pfid_kt.get())
            .is_some_and(|kt| pfr_kentry_byaddr(kt, &cached, af, false).is_some())
    } else if rpool.addr.type_.get() == PF_ADDR_TABLE {
        rpool
            .addr
            .tbl()
            .is_some_and(|kt| pfr_kentry_byaddr(kt, &cached, af, false).is_some())
    } else if rpool.addr.type_.get() != PF_ADDR_NOROUTE {
        let v = rpool.addr.v.get();
        pf_match_addr(0, &v.addr(), &v.mask(), &cached, af)
    } else {
        false
    };
    if !valid {
        if pf_debug(LOG_DEBUG) {
            log(
                LOG_DEBUG,
                format_args!("pf: pf_map_addr: stale src tracking ({type_}) "),
            );
            pf_print_host(&k.addr.get(), 0, af);
            addlog(format_args!(" to "));
            pf_print_host(&cached, 0, af);
            addlog(format_args!("\n"));
        }
        if sn.states.get() != 0 {
            // XXX expensive
            for s in TREE_ID.iter() {
                pf_state_rm_src_node(s, sn);
            }
        }
        sn.expire.set(1);
        pf_remove_src_node(sn);
        sns[type_] = None;
        return false;
    }

    if !pf_azero(&cached, af) {
        pf_addrcpy(naddr, &cached, af);
        if (rpool.opts.get() & PF_POOL_TYPEMASK) == PF_POOL_LEASTSTATES
            && !pf_map_addr_states_increase(af, rpool, &cached)
        {
            return false;
        }
    }
    if pf_debug(LOG_DEBUG) {
        log(
            LOG_DEBUG,
            format_args!("pf: pf_map_addr: src tracking ({type_}) maps "),
        );
        pf_print_host(&k.addr.get(), 0, af);
        addlog(format_args!(" to "));
        pf_print_host(naddr, 0, af);
        addlog(format_args!("\n"));
    }

    if let Some(kif) = sn.kif() {
        rpool.set_kif(Some(kif));
    }

    true
}

/// `pf_rand_addr`: a random host part under the (network order) mask word `mask`.
pub fn pf_rand_addr(mask: u32) -> u32 {
    let mask = !u32::from_be(mask);
    let addr = arc4random_uniform(mask.wrapping_add(1));

    addr.to_be()
}

/// Whether the pool's table (or the table of its dynamic address) has weighted entries
/// (`pfrkt_refcntcost > 0`), the condition `pf_map_addr` tests in several places.
fn pf_pool_weighted(rpool: &'static PfPool) -> bool {
    (rpool.addr.type_.get() == PF_ADDR_TABLE
        && rpool
            .addr
            .tbl()
            .is_some_and(|t| t.pfrkt_refcntcost.get() > 0))
        || (rpool.addr.type_.get() == PF_ADDR_DYNIFTL
            && rpool
                .addr
                .dyn_()
                .and_then(|d| d.pfid_kt.get())
                .is_some_and(|t| t.pfrkt_refcntcost.get() > 0))
}

/// The table of a table or dynamic-address pool (`rpool->addr.p.tbl` or
/// `rpool->addr.p.dyn->pfid_kt`).
fn pf_pool_table(rpool: &'static PfPool) -> Option<&'static crate::net::pfvar::PfrKtable> {
    if rpool.addr.type_.get() == PF_ADDR_TABLE {
        rpool.addr.tbl()
    } else {
        rpool.addr.dyn_().and_then(|d| d.pfid_kt.get())
    }
}

/// `!pfr_pool_get(rpool, &raddr, &rmask, af)`: the next address of the pool's table, in
/// `raddr` and `rmask`; `false` when the table has none.
fn pool_get_ok(
    rpool: &'static PfPool,
    raddr: &mut PfAddr,
    rmask: &mut PfAddr,
    af: SaFamily,
) -> bool {
    match pfr_pool_get(rpool, af) {
        Ok((a, m)) => {
            *raddr = a;
            *rmask = m;
            true
        }
        Err(_) => false,
    }
}

/// `pf_map_addr`: picks the address of `rpool` for a connection from `saddr` (the pool's
/// algorithm, or the sticky source node), in `naddr`; `init_addr` remembers the first pick
/// of a search. `true` (the C's 0) on success.
#[allow(clippy::too_many_arguments)]
pub fn pf_map_addr(
    af: SaFamily,
    r: &'static PfRule,
    saddr: &PfAddr,
    naddr: &mut PfAddr,
    init_addr: Option<&mut PfAddr>,
    sns: &mut [Option<&'static PfSrcNode>; PF_SN_MAX],
    rpool: &'static PfPool,
    type_: PfSnTypes,
) -> bool {
    let mut init_addr = init_addr;
    let v = rpool.addr.v.get();
    let mut raddr = v.addr();
    let mut rmask = v.mask();
    let mut states: u64 = 0;
    let mut weight: u16 = 0;

    if sns[type_].is_none()
        && rpool.opts.get() & PF_POOL_STICKYADDR != 0
        && (rpool.opts.get() & PF_POOL_TYPEMASK) != PF_POOL_NONE
        && pf_map_addr_sticky(af, r, saddr, naddr, sns, rpool, type_)
    {
        return true;
    }

    let atype = rpool.addr.type_.get();
    if atype == PF_ADDR_NOROUTE {
        return false;
    }
    if atype == PF_ADDR_DYNIFTL {
        let Some(d) = rpool.addr.dyn_() else {
            return false;
        };
        if af == AF_INET {
            if d.pfid_acnt4.get() < 1 && !pf_pool_dyntype(rpool.opts.get()) {
                return false;
            }
            raddr = d.pfid_addr4.get();
            rmask = d.pfid_mask4.get();
        } else {
            #[cfg(feature = "inet6")]
            if af == AF_INET6 {
                if d.pfid_acnt6.get() < 1 && !pf_pool_dyntype(rpool.opts.get()) {
                    return false;
                }
                raddr = d.pfid_addr6.get();
                rmask = d.pfid_mask6.get();
            } else {
                unhandled_af(i32::from(af));
            }
            #[cfg(not(feature = "inet6"))]
            unhandled_af(i32::from(af));
        }
    } else if atype == PF_ADDR_TABLE {
        if !pf_pool_dyntype(rpool.opts.get()) {
            return false; // unsupported
        }
    } else {
        raddr = v.addr();
        rmask = v.mask();
    }

    match rpool.opts.get() & PF_POOL_TYPEMASK {
        PF_POOL_NONE => {
            pf_addrcpy(naddr, &raddr, af);
        }
        PF_POOL_BITMASK => {
            pf_poolmask(naddr, &raddr, &rmask, saddr, af);
        }
        PF_POOL_RANDOM => {
            if atype == PF_ADDR_TABLE || atype == PF_ADDR_DYNIFTL {
                let Some(kt) = pf_pool_table(rpool).and_then(pfr_ktable_select_active) else {
                    return false;
                };

                let cnt = kt.pfrkt_cnt().get();
                if cnt == 0 {
                    rpool.tblidx.set(0);
                } else {
                    rpool.tblidx.set(arc4random_uniform(cnt as u32) as i32);
                }
                rpool.counter.set(PfAddr::zeroed());
                if !pool_get_ok(rpool, &mut raddr, &mut rmask, af) {
                    return false;
                }
                pf_addrcpy(naddr, &rpool.counter.get(), af);
            } else if let Some(ia) = init_addr.as_deref_mut()
                && pf_azero(ia, af)
            {
                if af == AF_INET {
                    let mut c = rpool.counter.get();
                    c.set_addr32(0, pf_rand_addr(rmask.addr32(0)));
                    rpool.counter.set(c);
                } else {
                    #[cfg(feature = "inet6")]
                    if af == AF_INET6 {
                        // A random host part in each word the mask leaves open, from the
                        // last, up to the first word the mask covers.
                        let mut c = rpool.counter.get();
                        for i in (0..4).rev() {
                            if rmask.addr32(i) == 0xffff_ffff {
                                break;
                            }
                            c.set_addr32(i, pf_rand_addr(rmask.addr32(i)));
                        }
                        rpool.counter.set(c);
                    } else {
                        unhandled_af(i32::from(af));
                    }
                    #[cfg(not(feature = "inet6"))]
                    unhandled_af(i32::from(af));
                }
                pf_poolmask(naddr, &raddr, &rmask, &rpool.counter.get(), af);
                pf_addrcpy(ia, naddr, af);
            } else {
                let mut c = rpool.counter.get();
                pf_addr_inc(&mut c, af);
                rpool.counter.set(c);
                pf_poolmask(naddr, &raddr, &rmask, &c, af);
            }
        }
        PF_POOL_SRCHASH => {
            let mut hash = PfAddr::zeroed();
            let hashidx = pf_hash(saddr, &mut hash, &rpool.key.get(), af);

            if atype == PF_ADDR_TABLE || atype == PF_ADDR_DYNIFTL {
                let Some(kt) = pf_pool_table(rpool).and_then(pfr_ktable_select_active) else {
                    return false;
                };

                let cnt = kt.pfrkt_cnt().get();
                if cnt == 0 {
                    rpool.tblidx.set(0);
                } else {
                    rpool.tblidx.set((hashidx % cnt as u64) as i32);
                }
                rpool.counter.set(PfAddr::zeroed());
                if !pool_get_ok(rpool, &mut raddr, &mut rmask, af) {
                    return false;
                }
                pf_addrcpy(naddr, &rpool.counter.get(), af);
            } else {
                pf_poolmask(naddr, &raddr, &rmask, &hash, af);
            }
        }
        PF_POOL_ROUNDROBIN => {
            'rr: {
                if atype == PF_ADDR_TABLE || atype == PF_ADDR_DYNIFTL {
                    if !pool_get_ok(rpool, &mut raddr, &mut rmask, af) {
                        // reset counter in case its value
                        // has been removed from the pool.
                        rpool.counter.set(PfAddr::zeroed());
                        if !pool_get_ok(rpool, &mut raddr, &mut rmask, af) {
                            return false;
                        }
                    }
                } else if pf_azero(&rpool.counter.get(), af) {
                    // fall back to POOL_NONE if there is a single host
                    // address in pool.
                    if af == AF_INET && rmask.addr32(0) == INADDR_BROADCAST {
                        pf_addrcpy(naddr, &raddr, af);
                        break 'rr;
                    }
                    #[cfg(feature = "inet6")]
                    if af == AF_INET6
                        && crate::netinet6::in6::in6_are_addr_equal(
                            &rmask.v6(),
                            &crate::netinet6::in6::IN6MASK128,
                        )
                    {
                        pf_addrcpy(naddr, &raddr, af);
                        break 'rr;
                    }
                } else if pf_match_addr(0, &raddr, &rmask, &rpool.counter.get(), af) {
                    return false;
                }

                // iterate over table if it contains entries which are weighted
                if pf_pool_weighted(rpool) {
                    loop {
                        if atype == PF_ADDR_TABLE || atype == PF_ADDR_DYNIFTL {
                            if !pool_get_ok(rpool, &mut raddr, &mut rmask, af) {
                                return false;
                            }
                        } else {
                            log(
                                LOG_ERR,
                                format_args!("pf: pf_map_addr: weighted RR failure"),
                            );
                            return false;
                        }
                        if i32::from(rpool.weight.get()) >= rpool.curweight.get() {
                            break;
                        }
                        let mut c = rpool.counter.get();
                        pf_addr_inc(&mut c, af);
                        rpool.counter.set(c);
                    }

                    weight = rpool.weight.get();
                }

                pf_poolmask(naddr, &raddr, &rmask, &rpool.counter.get(), af);
                if let Some(ia) = init_addr.as_deref_mut()
                    && pf_azero(ia, af)
                {
                    pf_addrcpy(ia, &rpool.counter.get(), af);
                }
                let mut c = rpool.counter.get();
                pf_addr_inc(&mut c, af);
                rpool.counter.set(c);
            }
        }
        PF_POOL_LEASTSTATES => {
            // retrieve an address first
            if atype == PF_ADDR_TABLE || atype == PF_ADDR_DYNIFTL {
                if !pool_get_ok(rpool, &mut raddr, &mut rmask, af) {
                    // see PF_POOL_ROUNDROBIN
                    rpool.counter.set(PfAddr::zeroed());
                    if !pool_get_ok(rpool, &mut raddr, &mut rmask, af) {
                        return false;
                    }
                }
            } else if pf_match_addr(0, &raddr, &rmask, &rpool.counter.get(), af) {
                return false;
            }

            states = rpool.states.get();
            weight = rpool.weight.get();
            let mut kif = rpool.kif();

            let mut load: u64 = if pf_pool_weighted(rpool) {
                (u64::from(u16::MAX) * rpool.states.get()) / u64::from(rpool.weight.get())
            } else {
                states
            };

            let mut faddr = PfAddr::zeroed();
            pf_addrcpy(&mut faddr, &rpool.counter.get(), af);

            pf_addrcpy(naddr, &rpool.counter.get(), af);
            if let Some(ia) = init_addr.as_deref_mut()
                && pf_azero(ia, af)
            {
                pf_addrcpy(ia, naddr, af);
            }

            // iterate *once* over whole table and find destination with
            // least connection
            loop {
                let mut c = rpool.counter.get();
                pf_addr_inc(&mut c, af);
                rpool.counter.set(c);
                if atype == PF_ADDR_TABLE || atype == PF_ADDR_DYNIFTL {
                    if !pool_get_ok(rpool, &mut raddr, &mut rmask, af) {
                        return false;
                    }
                } else if pf_match_addr(0, &raddr, &rmask, &rpool.counter.get(), af) {
                    return false;
                }

                let cload: u64 = if pf_pool_weighted(rpool) {
                    (u64::from(u16::MAX) * rpool.states.get()) / u64::from(rpool.weight.get())
                } else {
                    rpool.states.get()
                };

                // find lc minimum
                if cload < load {
                    states = rpool.states.get();
                    weight = rpool.weight.get();
                    kif = rpool.kif();
                    load = cload;

                    pf_addrcpy(naddr, &rpool.counter.get(), af);
                    if let Some(ia) = init_addr.as_deref_mut()
                        && pf_azero(ia, af)
                    {
                        pf_addrcpy(ia, naddr, af);
                    }
                }
                if !(pf_match_addr(1, &faddr, &rmask, &rpool.counter.get(), af) && states > 0) {
                    break;
                }
            }

            if !pf_map_addr_states_increase(af, rpool, naddr) {
                return false;
            }
            // revert the kif which was set by pfr_pool_get()
            rpool.set_kif(kif);
        }
        _ => {}
    }

    if rpool.opts.get() & PF_POOL_STICKYADDR != 0 {
        if let Some(sn) = sns[type_] {
            pf_remove_src_node(sn);
            sns[type_] = None;
        }
        if !pf_insert_src_node(
            &mut sns[type_],
            r,
            type_,
            af,
            saddr,
            Some(naddr),
            rpool.kif(),
        ) {
            return false;
        }
    }

    if pf_debug(LOG_INFO) && (rpool.opts.get() & PF_POOL_TYPEMASK) != PF_POOL_NONE {
        log(LOG_INFO, format_args!("pf: pf_map_addr: selected address "));
        pf_print_host(naddr, 0, af);
        if (rpool.opts.get() & PF_POOL_TYPEMASK) == PF_POOL_LEASTSTATES {
            addlog(format_args!(" with state count {states}"));
        }
        if pf_pool_weighted(rpool) {
            addlog(format_args!(" with weight {weight}"));
        }
        addlog(format_args!("\n"));
    }

    true
}

/// `pf_map_addr_states_increase`: counts a new state on the chosen table entry of a
/// `least-states` pool; `false` (the C's -1) when the entry is gone.
pub fn pf_map_addr_states_increase(af: SaFamily, rpool: &'static PfPool, naddr: &PfAddr) -> bool {
    let atype = rpool.addr.type_.get();
    if atype == PF_ADDR_TABLE || atype == PF_ADDR_DYNIFTL {
        let failed = pf_pool_table(rpool).is_none_or(|kt| pfr_states_increase(kt, naddr, af) == -1);
        if failed {
            if pf_debug(LOG_DEBUG) {
                log(
                    LOG_DEBUG,
                    format_args!("pf: pf_map_addr_states_increase: selected address "),
                );
                pf_print_host(naddr, 0, af);
                addlog(format_args!(". Failed to increase count!\n"));
            }
            return false;
        }
    }
    true
}

/// `pf_get_transaddr`: the translated addresses and ports of a `nat-to`/`rdr-to` rule, in
/// `pd->nsaddr`/`nsport` and `pd->ndaddr`/`ndport`; `nr` becomes the rule. `true` (the C's
/// 0) on success.
pub fn pf_get_transaddr(
    r: &'static PfRule,
    pd: &mut PfPdesc,
    sns: &mut [Option<&'static PfSrcNode>; PF_SN_MAX],
    nr: &mut Option<&'static PfRule>,
) -> bool {
    let mut naddr = PfAddr::zeroed();

    #[cfg(feature = "inet6")]
    if pd.af != pd.naf {
        return pf_get_transaddr_af(r, pd, sns);
    }

    if r.nat.addr.type_.get() != PF_ADDR_NONE {
        // XXX is this right? what if rtable is changed at the same
        // XXX time? where do I need to figure out the sport?
        let mut nport: u16 = 0;
        if !pf_get_sport(
            pd,
            r,
            &mut naddr,
            &mut nport,
            r.nat.proxy_port[0].get(),
            r.nat.proxy_port[1].get(),
            sns,
        ) {
            crate::dpfprintf!(
                LOG_NOTICE,
                "pf: NAT proxy port allocation ({}-{}) failed",
                r.nat.proxy_port[0].get(),
                r.nat.proxy_port[1].get()
            );
            return false;
        }
        // decrease least-connection state counter of the previous
        if let Some(prev) = *nr
            && prev.nat.addr.type_.get() != PF_ADDR_NONE
            && (prev.nat.opts.get() & PF_POOL_TYPEMASK) == PF_POOL_LEASTSTATES
        {
            pf_pool_states_decrease_addr(&prev.nat, i32::from(pd.af), &pd.nsaddr);
        }
        *nr = Some(r);
        let af = pd.af;
        pf_addrcpy(&mut pd.nsaddr, &naddr, af);
        pd.nsport = nport;
    }
    if r.rdr.addr.type_.get() != PF_ADDR_NONE {
        let nsaddr = pd.nsaddr;
        if !pf_map_addr(pd.af, r, &nsaddr, &mut naddr, None, sns, &r.rdr, PF_SN_RDR) {
            return false;
        }
        if (r.rdr.opts.get() & PF_POOL_TYPEMASK) == PF_POOL_BITMASK {
            let n = naddr;
            pf_poolmask(
                &mut naddr,
                &n,
                &r.rdr.addr.v.get().mask(),
                &pd.ndaddr,
                pd.af,
            );
        }

        let mut nport: u16 = 0;
        if r.rdr.proxy_port[1].get() != 0 {
            let mut div: u16 = r.rdr.proxy_port[1]
                .get()
                .wrapping_sub(r.rdr.proxy_port[0].get())
                .wrapping_add(1);
            div = if div == 0 { 1 } else { div };

            // The C's int arithmetic: the difference may be negative, and so the remainder.
            let mut tmp_nport: u32 = ((i32::from(u16::from_be(pd.ndport))
                - i32::from(u16::from_be(r.dst.port[0].get())))
                % i32::from(div)
                + i32::from(r.rdr.proxy_port[0].get())) as u32;

            // wrap around if necessary
            if tmp_nport > 65535 {
                tmp_nport = tmp_nport.wrapping_sub(65535);
            }
            nport = (tmp_nport as u16).to_be();
        } else if r.rdr.proxy_port[0].get() != 0 {
            nport = r.rdr.proxy_port[0].get().to_be();
        }
        // decrease least-connection state counter of the previous
        if let Some(prev) = *nr
            && prev.rdr.addr.type_.get() != PF_ADDR_NONE
            && (prev.rdr.opts.get() & PF_POOL_TYPEMASK) == PF_POOL_LEASTSTATES
        {
            pf_pool_states_decrease_addr(&prev.rdr, i32::from(pd.af), &pd.ndaddr);
        }
        *nr = Some(r);
        let af = pd.af;
        pf_addrcpy(&mut pd.ndaddr, &naddr, af);
        if nport != 0 {
            pd.ndport = nport;
        }
    }

    true
}

/// `pf_get_transaddr_af`: the `af-to` translation of `pd` into family `pd->naf`: the source
/// address and port from the nat pool, the ICMP and ICMPv6 echo types swapped, and the
/// destination mapped through `inet_nat46`/`inet_nat64` with the prefix of the rdr pool, the
/// rule's destination or the nat pool. `true` (the C's 0) on success.
#[cfg(feature = "inet6")]
pub fn pf_get_transaddr_af(
    r: &'static PfRule,
    pd: &mut PfPdesc,
    sns: &mut [Option<&'static PfSrcNode>; PF_SN_MAX],
) -> bool {
    use crate::kern::subr_prf::panic;
    use crate::netinet::icmp6::{ICMP6_ECHO_REPLY, ICMP6_ECHO_REQUEST};
    use crate::netinet::in_::{InAddr, in_mask2len};
    use crate::netinet::inet_nat64::{inet_nat46, inet_nat64};
    use crate::netinet::ip_icmp::ICMP_ECHOREPLY;
    use crate::netinet6::in6::in6_mask2len;

    let mut ndaddr = PfAddr::zeroed();
    let mut nsaddr = PfAddr::zeroed();
    let mut naddr = PfAddr::zeroed();
    let mut prefixlen: i32;

    let fam = |af: SaFamily| if af == AF_INET { "inet" } else { "inet6" };
    let kind = || {
        if r.rdr.addr.type_.get() == PF_ADDR_NONE {
            "nat"
        } else {
            "rdr"
        }
    };

    if pf_debug(LOG_INFO) {
        log(
            LOG_INFO,
            format_args!("pf: af-to {} {}, ", fam(pd.naf), kind()),
        );
        pf_print_host(&pd.nsaddr, pd.nsport, pd.af);
        addlog(format_args!(" -> "));
        pf_print_host(&pd.ndaddr, pd.ndport, pd.af);
        addlog(format_args!("\n"));
    }

    if r.nat.addr.type_.get() == PF_ADDR_NONE {
        panic(format_args!(
            "pf_get_transaddr_af: no nat pool for source address"
        ));
    }

    // get source address and port
    let mut nport: u16 = 0;
    if !pf_get_sport(
        pd,
        r,
        &mut nsaddr,
        &mut nport,
        r.nat.proxy_port[0].get(),
        r.nat.proxy_port[1].get(),
        sns,
    ) {
        crate::dpfprintf!(
            LOG_NOTICE,
            "pf: af-to NAT proxy port allocation ({}-{}) failed",
            r.nat.proxy_port[0].get(),
            r.nat.proxy_port[1].get()
        );
        return false;
    }
    pd.nsport = nport;

    // The echo type in the port of the destination (inbound) or source (outbound).
    let swap = |p: u16, from: (u8, u8), to: (u8, u8)| -> u16 {
        let h = u16::from_be(p);
        let h = if h == u16::from(from.0) {
            u16::from(to.0)
        } else if h == u16::from(from.1) {
            u16::from(to.1)
        } else {
            h
        };
        h.to_be()
    };
    let proto = i32::from(pd.proto);
    if proto == IPPROTO_ICMPV6 && pd.naf == AF_INET {
        let (f, t) = (
            (ICMP6_ECHO_REQUEST, ICMP6_ECHO_REPLY),
            (ICMP_ECHO, ICMP_ECHOREPLY),
        );
        if pd.dir == PF_IN {
            pd.ndport = swap(pd.ndport, f, t);
        } else {
            pd.nsport = swap(pd.nsport, f, t);
        }
    } else if proto == IPPROTO_ICMP && pd.naf == AF_INET6 {
        let (f, t) = (
            (ICMP_ECHO, ICMP_ECHOREPLY),
            (ICMP6_ECHO_REQUEST, ICMP6_ECHO_REPLY),
        );
        if pd.dir == PF_IN {
            pd.ndport = swap(pd.ndport, f, t);
        } else {
            pd.nsport = swap(pd.nsport, f, t);
        }
    }

    // get the destination address and port
    if r.rdr.addr.type_.get() != PF_ADDR_NONE {
        if !pf_map_addr(pd.naf, r, &nsaddr, &mut naddr, None, sns, &r.rdr, PF_SN_RDR) {
            return false;
        }
        if r.rdr.proxy_port[0].get() != 0 {
            pd.ndport = r.rdr.proxy_port[0].get().to_be();
        }

        let mask = r.rdr.addr.v.get().mask();
        if pd.naf == AF_INET {
            // The prefix is the IPv4 rdr address
            prefixlen = in_mask2len(&InAddr {
                s_addr: mask.addr32(0),
            });
            let _ = inet_nat46(
                i32::from(pd.naf),
                &pd.ndaddr.addr8,
                &mut ndaddr.addr8,
                &naddr.addr8,
                prefixlen as u8,
            );
        } else {
            // The prefix is the IPv6 rdr address
            prefixlen = in6_mask2len(&mask.v6(), None);
            let _ = inet_nat64(
                i32::from(pd.naf),
                &pd.ndaddr.addr8,
                &mut ndaddr.addr8,
                &naddr.addr8,
                prefixlen as u8,
            );
        }
    } else if pd.naf == AF_INET {
        // The prefix is the IPv6 dst address
        prefixlen = in6_mask2len(&r.dst.addr.v.get().mask().v6(), None);
        if prefixlen < 32 {
            prefixlen = 96;
        }
        let nd = pd.ndaddr;
        let _ = inet_nat64(
            i32::from(pd.naf),
            &nd.addr8,
            &mut ndaddr.addr8,
            &nd.addr8,
            prefixlen as u8,
        );
    } else {
        // The prefix is the IPv6 nat address (that was stored in pd->nsaddr)
        prefixlen = in6_mask2len(&r.nat.addr.v.get().mask().v6(), None);
        if prefixlen > 96 {
            prefixlen = 96;
        }
        let _ = inet_nat64(
            i32::from(pd.naf),
            &pd.ndaddr.addr8,
            &mut ndaddr.addr8,
            &nsaddr.addr8,
            prefixlen as u8,
        );
    }

    let naf = pd.naf;
    pf_addrcpy(&mut pd.nsaddr, &nsaddr, naf);
    pf_addrcpy(&mut pd.ndaddr, &ndaddr, naf);

    if pf_debug(LOG_INFO) {
        log(
            LOG_INFO,
            format_args!(
                "pf: af-to {} {} done, prefixlen {}, ",
                fam(pd.naf),
                kind(),
                prefixlen
            ),
        );
        pf_print_host(&pd.nsaddr, pd.nsport, pd.naf);
        addlog(format_args!(" -> "));
        pf_print_host(&pd.ndaddr, pd.ndport, pd.naf);
        addlog(format_args!("\n"));
    }

    true
}

/// `pf_postprocess_addr`: takes the state's connection off the `least-states` counters of
/// the pools that chose its addresses (`nat-to`, `rdr-to`, `route-to`); `true` (the C's 0)
/// unless a counter could not be decreased.
pub fn pf_postprocess_addr(cur: &'static PfState) -> bool {
    let mut natpl: Option<&'static PfPool> = None;
    let mut rdrpl: Option<&'static PfPool> = None;
    let mut ret = true;

    // decrease the states counter in pool for "least-states"
    if let Some(nat) = cur.natrule.ptr() {
        // this is the final
        if nat.nat.addr.type_.get() != PF_ADDR_NONE {
            natpl = Some(&nat.nat);
        }
        if nat.rdr.addr.type_.get() != PF_ADDR_NONE {
            rdrpl = Some(&nat.rdr);
        }
    }
    if natpl.is_none() || rdrpl.is_none() {
        // nat or rdr might be done by a previous "match"
        for ri in cur.match_rules.iter() {
            let rr = ri.r();
            // first match since the list order is reversed
            if natpl.is_none() && rr.nat.addr.type_.get() != PF_ADDR_NONE {
                natpl = Some(&rr.nat);
            }
            if rdrpl.is_none() && rr.rdr.addr.type_.get() != PF_ADDR_NONE {
                rdrpl = Some(&rr.rdr);
            }
            if natpl.is_some() && rdrpl.is_some() {
                break;
            }
        }
    }
    if let Some(natpl) = natpl
        && (natpl.opts.get() & PF_POOL_TYPEMASK) == PF_POOL_LEASTSTATES
    {
        // nat-to <table> least-state
        let (sks, i) = if cur.direction.get() == PF_IN {
            (cur.key[PF_SK_STACK].get(), 0)
        } else {
            (cur.key[PF_SK_WIRE].get(), 1)
        };
        if let Some(sks) = sks
            && !pf_pool_states_decrease_addr(natpl, i32::from(sks.af.get()), &sks.addr[i].get())
        {
            ret = false;
        }
    }
    if let Some(rdrpl) = rdrpl
        && (rdrpl.opts.get() & PF_POOL_TYPEMASK) == PF_POOL_LEASTSTATES
    {
        // rdr <table> least-state
        let (sks, i) = if cur.direction.get() == PF_IN {
            (cur.key[PF_SK_STACK].get(), 1)
        } else {
            (cur.key[PF_SK_WIRE].get(), 0)
        };
        if let Some(sks) = sks
            && !pf_pool_states_decrease_addr(rdrpl, i32::from(sks.af.get()), &sks.addr[i].get())
        {
            ret = false;
        }
    }
    if let Some(rule) = cur.rule.ptr() {
        // route-to <table> least-state
        let rpool = &rule.route;
        if rpool.addr.type_.get() != PF_ADDR_NONE
            && (rpool.opts.get() & PF_POOL_TYPEMASK) == PF_POOL_LEASTSTATES
        {
            let sks = cur.key[if cur.direction.get() == PF_IN {
                PF_SK_STACK
            } else {
                PF_SK_WIRE
            }]
            .get();
            crate::kassert!(sks.is_some());
            if let Some(sks) = sks
                && !pf_pool_states_decrease_addr(rpool, i32::from(sks.af.get()), &cur.rt_addr.get())
            {
                ret = false;
            }
        }
    }

    ret
}

/// `pf_pool_states_decrease_addr`: takes a state off the counter of `addr`'s entry in the
/// pool's table; `false` (the C's 1) when the entry is gone.
pub fn pf_pool_states_decrease_addr(rpool: &'static PfPool, af: i32, addr: &PfAddr) -> bool {
    let mut slbcount: i32 = -1;
    let af8 = af as SaFamily;

    let atype = rpool.addr.type_.get();
    if atype == PF_ADDR_TABLE || atype == PF_ADDR_DYNIFTL {
        slbcount = pf_pool_table(rpool).map_or(-1, |kt| pfr_states_decrease(kt, addr, af8));
        if slbcount == -1 {
            if pf_debug(LOG_DEBUG) {
                log(
                    LOG_DEBUG,
                    format_args!("pf: pf_pool_states_decrease_addr: selected address "),
                );
                pf_print_host(addr, 0, af8);
                addlog(format_args!(". Failed to decrease count!\n"));
            }
            return false;
        }
    }
    if slbcount > -1 && pf_debug(LOG_INFO) {
        log(
            LOG_INFO,
            format_args!("pf: pf_pool_states_decrease_addr: selected address "),
        );
        pf_print_host(addr, 0, af8);
        addlog(format_args!(" decreased state count to {slbcount}\n"));
    }

    true
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for the address pools (`none`, `bitmask`, `round-robin` over an address and
    // mask) and the source port search of `nat-to`.

    use std::boxed::Box;
    use std::{assert, assert_eq};

    use super::*;
    use crate::net::pfvar::{PF_ADDR_ADDRMASK, PfAddrWrapV};
    use crate::netinet::in_::InAddr;

    /// An IPv4 `PfAddr` from its dotted bytes.
    fn a4(b: [u8; 4]) -> PfAddr {
        PfAddr::from_v4(InAddr {
            s_addr: u32::from_ne_bytes(b),
        })
    }

    /// A kernel rule whose `nat` pool is `addr`/`mask` with the pool type `opts`.
    fn rule_with_nat(addr: [u8; 4], mask: [u8; 4], opts: u8) -> &'static PfRule {
        let r: &'static PfRule = Box::leak(Box::new(PfRule::zeroed()));
        let mut v = PfAddrWrapV::default();
        v.set_addr(&a4(addr));
        v.set_mask(&a4(mask));
        r.nat.addr.v.set(v);
        r.nat.addr.type_.set(PF_ADDR_ADDRMASK);
        r.nat.opts.set(opts);
        r
    }

    /// `pf_map_addr` of `saddr` through the rule's `nat` pool.
    fn map(r: &'static PfRule, saddr: [u8; 4]) -> Option<PfAddr> {
        let mut sns: [Option<&'static PfSrcNode>; PF_SN_MAX] = [None; PF_SN_MAX];
        let mut naddr = PfAddr::zeroed();
        pf_map_addr(
            AF_INET,
            r,
            &a4(saddr),
            &mut naddr,
            None,
            &mut sns,
            &r.nat,
            PF_SN_NAT,
        )
        .then_some(naddr)
    }

    #[test]
    fn pool_none_maps_to_the_pool_address() {
        let r = rule_with_nat([10, 0, 0, 1], [255, 255, 255, 255], PF_POOL_NONE);
        assert_eq!(map(r, [192, 168, 1, 5]), Some(a4([10, 0, 0, 1])));
        assert_eq!(map(r, [172, 16, 0, 9]), Some(a4([10, 0, 0, 1])));
    }

    #[test]
    fn pool_bitmask_keeps_the_host_part() {
        let r = rule_with_nat([10, 0, 0, 0], [255, 255, 255, 0], PF_POOL_BITMASK);
        assert_eq!(map(r, [192, 168, 1, 5]), Some(a4([10, 0, 0, 5])));
        assert_eq!(map(r, [172, 16, 3, 200]), Some(a4([10, 0, 0, 200])));
    }

    #[test]
    fn pool_roundrobin_walks_the_network() {
        let r = rule_with_nat([10, 0, 0, 0], [255, 255, 255, 252], PF_POOL_ROUNDROBIN);
        let s = [192, 168, 1, 5];
        assert_eq!(map(r, s), Some(a4([10, 0, 0, 0])));
        assert_eq!(map(r, s), Some(a4([10, 0, 0, 1])));
        assert_eq!(map(r, s), Some(a4([10, 0, 0, 2])));
        assert_eq!(map(r, s), Some(a4([10, 0, 0, 3])));
        // The counter's host part wraps within the mask.
        assert_eq!(map(r, s), Some(a4([10, 0, 0, 0])));
        // A counter that matches the pool's network makes the C's pf_match_addr check fail.
        r.nat.counter.set(a4([10, 0, 0, 1]));
        assert_eq!(map(r, s), None);
    }

    #[test]
    fn pool_roundrobin_single_host_falls_back_to_none() {
        let r = rule_with_nat([10, 0, 0, 7], [255, 255, 255, 255], PF_POOL_ROUNDROBIN);
        assert_eq!(map(r, [1, 2, 3, 4]), Some(a4([10, 0, 0, 7])));
        assert_eq!(map(r, [1, 2, 3, 4]), Some(a4([10, 0, 0, 7])));
    }

    #[test]
    fn noroute_pool_fails() {
        let r = rule_with_nat([10, 0, 0, 7], [255, 255, 255, 255], PF_POOL_NONE);
        r.nat.addr.type_.set(PF_ADDR_NOROUTE);
        assert_eq!(map(r, [1, 2, 3, 4]), None);
    }

    /// An outbound UDP descriptor from 192.168.1.5:1234 to 8.8.8.8:53.
    fn udp_pd() -> PfPdesc {
        let mut pd = PfPdesc::new();
        pd.af = AF_INET;
        pd.naf = AF_INET;
        pd.proto = IPPROTO_UDP as u8;
        pd.dir = PF_OUT;
        pd.sidx = 1;
        pd.didx = 0;
        pd.nsaddr = a4([192, 168, 1, 5]);
        pd.ndaddr = a4([8, 8, 8, 8]);
        pd.nsport = 1234u16.to_be();
        pd.ndport = 53u16.to_be();
        pd
    }

    #[test]
    fn get_sport_picks_a_port_in_the_range() {
        let r = rule_with_nat([10, 0, 0, 1], [255, 255, 255, 255], PF_POOL_NONE);
        let mut pd = udp_pd();
        let mut sns: [Option<&'static PfSrcNode>; PF_SN_MAX] = [None; PF_SN_MAX];
        for _ in 0..16 {
            let mut naddr = PfAddr::zeroed();
            let mut nport = 0u16;
            assert!(pf_get_sport(
                &mut pd, r, &mut naddr, &mut nport, 50000, 50010, &mut sns
            ));
            assert_eq!(naddr, a4([10, 0, 0, 1]));
            let p = u16::from_be(nport);
            assert!((50000..=50010).contains(&p), "port {p}");
        }

        // A one-port range gives that port; reversed bounds are swapped.
        let mut naddr = PfAddr::zeroed();
        let mut nport = 0u16;
        assert!(pf_get_sport(
            &mut pd, r, &mut naddr, &mut nport, 40000, 40000, &mut sns
        ));
        assert_eq!(u16::from_be(nport), 40000);
        assert!(pf_get_sport(
            &mut pd, r, &mut naddr, &mut nport, 60010, 60000, &mut sns
        ));
        assert!((60000..=60010).contains(&u16::from_be(nport)));

        // No range: the packet's own port.
        assert!(pf_get_sport(
            &mut pd, r, &mut naddr, &mut nport, 0, 0, &mut sns
        ));
        assert_eq!(u16::from_be(nport), 1234);
    }

    #[test]
    fn get_sport_leaves_non_echo_icmp_alone() {
        let r = rule_with_nat([10, 0, 0, 1], [255, 255, 255, 255], PF_POOL_NONE);
        let mut pd = udp_pd();
        pd.proto = IPPROTO_ICMP as u8;
        pd.ndport = 3u16.to_be(); // unreachable, not an echo
        let mut sns: [Option<&'static PfSrcNode>; PF_SN_MAX] = [None; PF_SN_MAX];
        let mut naddr = PfAddr::zeroed();
        let mut nport = 0xbeefu16;
        assert!(pf_get_sport(
            &mut pd, r, &mut naddr, &mut nport, 50000, 50010, &mut sns
        ));
        assert_eq!(nport, 0xbeef);
        assert_eq!(naddr, a4([10, 0, 0, 1]));
    }

    #[test]
    fn rand_addr_stays_in_the_host_part() {
        let mask = u32::from_ne_bytes([255, 255, 255, 0]);
        for _ in 0..32 {
            let a = pf_rand_addr(mask);
            assert_eq!(a & mask, 0);
        }
    }
}
/* </TESTS> */
