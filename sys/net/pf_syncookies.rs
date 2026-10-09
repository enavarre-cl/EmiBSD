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

/* Copyright (c) 2016,2017 Henning Brauer <henning@openbsd.org>
 * Copyright (c) 2016 Alexandr Nedvedicky <sashan@openbsd.org>
 *
 * syncookie parts based on FreeBSD sys/netinet/tcp_syncache.c
 *
 * Copyright (c) 2001 McAfee, Inc.
 * Copyright (c) 2006,2013 Andre Oppermann, Internet Business Solutions AG
 * All rights reserved.
 *
 * This software was developed for the FreeBSD Project by Jonathan Lemon
 * and McAfee Research, the Security Research Division of McAfee, Inc. under
 * DARPA/SPAWAR contract N66001-01-C-8035 ("CBOSS"), as part of the
 * DARPA CHATS research program. [2001 McAfee, Inc.]
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
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! pf's SYN cookies: under a SYN flood pf answers SYNs itself with a cookie as the initial
//! sequence number instead of creating states, and creates the state only when the client's
//! ACK carries a valid cookie back.
//!
//! Upstream: sys/net/pf_syncookies.c @ 3ce1f3f79392
//!
//! When we're under synflood, we use syncookies to prevent state table exhaustion. Trigger
//! for the synflood mode is the number of half-open connections in the state table. We leave
//! synflood mode when the number of half-open states - including in-flight syncookies - drops
//! far enough again.
//!
//! A syncookie enabled Initial Sequence Number is a 24 bit MAC, a 3 bit WSCALE index, a 3 bit
//! MSS index, 1 bit SACK permitted and 1 bit odd/even secret. References: RFC4987 TCP SYN
//! Flooding Attacks and Common Mitigations; <http://cr.yp.to/syncookies.html> (overview);
//! <http://cr.yp.to/syncookies/archive> (details).
//!
//! ## Deviations
//! - `union pf_syncookie` is [`PfSyncookie`], the byte with accessors for the bit-fields; the
//!   bits are where clang puts them on both little-endian targets (amd64, arm64): `oddeven`
//!   bit 0, `sack_ok` bit 1, `wscale_idx` bits 2-4, `mss_idx` bits 5-7.
//! - The file-static `pf_syncookie_status` is [`PF_SYNCOOKIE_STATUS`], its members `Cell`s
//!   (and an atomic for the `volatile` `oddeven`).
//! - `tcp_mssdflt` (netinet's `TCP_MSSDFLT`, an atomic) is read through `net/pf.rs`'s
//!   `tcp_mssdflt()`.

use core::cell::Cell;
use core::ffi::c_void;
use core::sync::atomic::{AtomicU32, Ordering};

use crate::crypto::siphash::{SIPHASH_KEY_LENGTH, SipHash24Ctx, SiphashKey};
use crate::dev::rnd::arc4random_buf;
use crate::kassert;
use crate::kern::kern_timeout::{timeout_add_sec, timeout_set};
use crate::kern::subr_prf::panic;
use crate::net::pf::{PF_STATUS, pf_build_tcp, pf_get_mss, pf_get_wscale, pf_inc, pf_send_tcp};
use crate::net::pfvar::{
    LCNT_SYNCOOKIES_SENT, LCNT_SYNCOOKIES_VALID, LCNT_SYNFLOODS, PF_SYNCOOKIES_ADAPTIVE,
    PF_SYNCOOKIES_ALWAYS, PF_SYNCOOKIES_HIWATPCT, PF_SYNCOOKIES_LOWATPCT, PF_SYNCOOKIES_MODE_MAX,
    PF_SYNCOOKIES_NEVER, PFSTATE_HIWAT, PfiocSynflwats,
};
use crate::net::pfvar_priv::PfPdesc;
use crate::netinet::in_::IPPROTO_TCP;
use crate::netinet::tcp::{TH_ACK, TH_SYN};
use crate::sys::errno::Errno;
use crate::sys::mbuf::{Mbuf, PF_TAG_SYNCOOKIE_RECREATED};
use crate::sys::socket::{AF_INET, AF_INET6};
use crate::sys::syslog::LOG_WARNING;
use crate::sys::timeout::Timeout;

/// `union pf_syncookie`: the cookie byte and its bit-fields (see the module's deviations).
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct PfSyncookie {
    /// `cookie`.
    pub cookie: u8,
}

impl PfSyncookie {
    /// `flags.oddeven`: which of the two secrets.
    pub const fn oddeven(self) -> u8 {
        self.cookie & 0x1
    }

    /// `flags.oddeven = v`.
    pub fn set_oddeven(&mut self, v: u8) {
        self.cookie = (self.cookie & !0x1) | (v & 0x1);
    }

    /// `flags.sack_ok`.
    pub const fn sack_ok(self) -> u8 {
        (self.cookie >> 1) & 0x1
    }

    /// `flags.sack_ok = v`.
    pub fn set_sack_ok(&mut self, v: u8) {
        self.cookie = (self.cookie & !0x2) | ((v & 0x1) << 1);
    }

    /// `flags.wscale_idx`.
    pub const fn wscale_idx(self) -> u8 {
        (self.cookie >> 2) & 0x7
    }

    /// `flags.wscale_idx = v`.
    pub fn set_wscale_idx(&mut self, v: u8) {
        self.cookie = (self.cookie & !0x1c) | ((v & 0x7) << 2);
    }

    /// `flags.mss_idx`.
    pub const fn mss_idx(self) -> u8 {
        (self.cookie >> 5) & 0x7
    }

    /// `flags.mss_idx = v`.
    pub fn set_mss_idx(&mut self, v: u8) {
        self.cookie = (self.cookie & !0xe0) | ((v & 0x7) << 5);
    }
}

/// `PF_SYNCOOKIE_SECRET_SIZE`.
const PF_SYNCOOKIE_SECRET_SIZE: usize = SIPHASH_KEY_LENGTH;
/// `PF_SYNCOOKIE_SECRET_LIFETIME`: seconds.
const PF_SYNCOOKIE_SECRET_LIFETIME: i32 = 15;

/// The type of `pf_syncookie_status`.
pub struct PfSyncookieStatus {
    /// `keytimeout`.
    pub keytimeout: Timeout,
    /// `oddeven` (`volatile`).
    pub oddeven: AtomicU32,
    /// `key[2]`.
    pub key: [Cell<SiphashKey>; 2],
    /// `hiwat`: absolute; # of states.
    pub hiwat: Cell<u32>,
    /// `lowat`.
    pub lowat: Cell<u32>,
}

// SAFETY: changed under the net lock and `pf_lock`, and by the key timeout, which writes the
// key slot `oddeven` (an atomic) does not select before it flips it, as the C does.
unsafe impl Sync for PfSyncookieStatus {}

/// `pf_syncookie_status`.
pub static PF_SYNCOOKIE_STATUS: PfSyncookieStatus = PfSyncookieStatus {
    keytimeout: Timeout::zeroed(),
    oddeven: AtomicU32::new(0),
    key: [
        Cell::new(SiphashKey { k0: 0, k1: 0 }),
        Cell::new(SiphashKey { k0: 0, k1: 0 }),
    ],
    hiwat: Cell::new(0),
    lowat: Cell::new(0),
};

/// `pf_syncookie_msstab`: distribution and probability of certain MSS values. Those in
/// between are rounded down to the next lower one. \[An Analysis of TCP Maximum Segment
/// Sizes, S. Alcock and R. Nelson, 2011\]: .2% .3% 5% 7% 7% 20% 15% 45%.
static PF_SYNCOOKIE_MSSTAB: [i32; 8] = [216, 536, 1200, 1360, 1400, 1440, 1452, 1460];

/// `pf_syncookie_wstab`: distribution and probability of certain WSCALE values. The absence
/// of the WSCALE option is encoded with index zero. \[WSCALE values histograms, Allman,
/// 2012\]: X 10 10 35 5 6 14 10% by host, X 11 4 5 5 18 49 3% by connections.
static PF_SYNCOOKIE_WSTAB: [i32; 8] = [0, 0, 1, 2, 4, 6, 7, 8];

/// The current `pf_syncookie_status.oddeven`.
fn oddeven() -> usize {
    PF_SYNCOOKIE_STATUS.oddeven.load(Ordering::Relaxed) as usize
}

/// `pf_syncookies_init`.
pub fn pf_syncookies_init() {
    timeout_set(
        &PF_SYNCOOKIE_STATUS.keytimeout,
        pf_syncookie_rotate,
        core::ptr::null_mut(),
    );
    PF_SYNCOOKIE_STATUS
        .hiwat
        .set(PFSTATE_HIWAT * PF_SYNCOOKIES_HIWATPCT / 100);
    PF_SYNCOOKIE_STATUS
        .lowat
        .set(PFSTATE_HIWAT * PF_SYNCOOKIES_LOWATPCT / 100);
    let _ = pf_syncookies_setmode(PF_SYNCOOKIES_NEVER);
}

/// `pf_syncookies_setmode`.
pub fn pf_syncookies_setmode(mode: u8) -> Result<(), Errno> {
    if mode > PF_SYNCOOKIES_MODE_MAX {
        return Err(Errno::EINVAL);
    }

    if PF_STATUS.syncookies_mode.get() == mode {
        return Ok(());
    }

    PF_STATUS.syncookies_mode.set(mode);
    if PF_STATUS.syncookies_mode.get() == PF_SYNCOOKIES_ALWAYS {
        pf_syncookie_newkey();
        PF_STATUS.syncookies_active.set(1);
    }
    Ok(())
}

/// `pf_syncookies_setwats`.
pub fn pf_syncookies_setwats(hiwat: u32, lowat: u32) -> Result<(), Errno> {
    if lowat > hiwat {
        return Err(Errno::EINVAL);
    }

    PF_SYNCOOKIE_STATUS.hiwat.set(hiwat);
    PF_SYNCOOKIE_STATUS.lowat.set(lowat);
    Ok(())
}

/// `pf_syncookies_getwats`.
pub fn pf_syncookies_getwats(wats: &mut PfiocSynflwats) -> Result<(), Errno> {
    wats.hiwat = PF_SYNCOOKIE_STATUS.hiwat.get();
    wats.lowat = PF_SYNCOOKIE_STATUS.lowat.get();
    Ok(())
}

/// `pf_synflood_check`: whether SYNs are answered with cookies (the mode, or in adaptive
/// mode whether a flood is on, which this call may detect).
pub fn pf_synflood_check(pd: &mut PfPdesc) -> bool {
    kassert!(i32::from(pd.proto) == IPPROTO_TCP);

    if let Some(m) = pd.m
        && m.m_pkthdr().pf.tag.get() & u16::from(PF_TAG_SYNCOOKIE_RECREATED) != 0
    {
        return false;
    }

    if PF_STATUS.syncookies_mode.get() != PF_SYNCOOKIES_ADAPTIVE {
        return PF_STATUS.syncookies_mode.get() != 0;
    }

    if PF_STATUS.syncookies_active.get() == 0
        && PF_STATUS.states_halfopen.get() > PF_SYNCOOKIE_STATUS.hiwat.get()
    {
        pf_syncookie_newkey();
        PF_STATUS.syncookies_active.set(1);
        crate::dpfprintf!(LOG_WARNING, "synflood detected, enabling syncookies");
        pf_inc(&PF_STATUS.lcounters[LCNT_SYNFLOODS]);
    }

    PF_STATUS.syncookies_active.get() != 0
}

/// `pf_syncookie_send`: answers the SYN with a SYN|ACK carrying a cookie.
pub fn pf_syncookie_send(pd: &mut PfPdesc, reason: &mut u16) {
    let mssdflt = crate::net::pf::tcp_mssdflt();
    let mss = core::cmp::max(pf_get_mss(pd, mssdflt), mssdflt);
    let iss = pf_syncookie_generate(pd, mss);
    let (src, dst) = (pd.ld_addr(pd.src), pd.ld_addr(pd.dst));
    let (sport, dport) = (pd.ld16(pd.sport), pd.ld16(pd.dport));
    pf_send_tcp(
        None,
        pd.af,
        &dst,
        &src,
        dport,
        sport,
        iss,
        u32::from_be(pd.tcp().th_seq).wrapping_add(1),
        TH_SYN | TH_ACK,
        0,
        mss,
        0,
        true,
        0,
        u32::from(pd.rdomain),
        Some(reason),
    );
    pf_inc(&PF_STATUS.syncookies_inflight[oddeven()]);
    pf_inc(&PF_STATUS.lcounters[LCNT_SYNCOOKIES_SENT]);
}

/// `pf_syncookie_validate`: 1 when the ACK carries a cookie pf sent, 0 otherwise.
pub fn pf_syncookie_validate(pd: &mut PfPdesc) -> u8 {
    kassert!(i32::from(pd.proto) == IPPROTO_TCP);

    let seq = u32::from_be(pd.tcp().th_seq).wrapping_sub(1);
    let ack = u32::from_be(pd.tcp().th_ack).wrapping_sub(1);
    let cookie = PfSyncookie {
        cookie: ((ack & 0xff) ^ (ack >> 24)) as u8,
    };

    // we don't know oddeven before setting the cookie (union)
    let inflight = &PF_STATUS.syncookies_inflight[usize::from(cookie.oddeven())];
    if inflight.get() == 0 {
        return 0;
    }

    let hash = pf_syncookie_mac(pd, cookie, seq);
    if (ack & !0xff) != (hash & !0xff) {
        return 0;
    }

    inflight.set(inflight.get() - 1);
    pf_inc(&PF_STATUS.lcounters[LCNT_SYNCOOKIES_VALID]);
    1
}

/*
 * all following functions private
 */

/// `pf_syncookie_rotate`: the key timeout. Leaves synflood mode when the flood is over and
/// drops the keys once no cookie is in flight; otherwise makes a new key.
pub fn pf_syncookie_rotate(_arg: *mut c_void) {
    let st = &PF_STATUS;
    // do we want to disable syncookies?
    if st.syncookies_active.get() != 0
        && ((st.syncookies_mode.get() == PF_SYNCOOKIES_ADAPTIVE
            && u64::from(st.states_halfopen.get())
                + st.syncookies_inflight[0].get()
                + st.syncookies_inflight[1].get()
                < u64::from(PF_SYNCOOKIE_STATUS.lowat.get()))
            || st.syncookies_mode.get() == PF_SYNCOOKIES_NEVER)
    {
        st.syncookies_active.set(0);
        crate::dpfprintf!(LOG_WARNING, "syncookies disabled");
    }

    // nothing in flight any more? delete keys and return
    if st.syncookies_active.get() == 0
        && st.syncookies_inflight[0].get() == 0
        && st.syncookies_inflight[1].get() == 0
    {
        PF_SYNCOOKIE_STATUS.key[0].set(SiphashKey { k0: 0, k1: 0 });
        PF_SYNCOOKIE_STATUS.key[1].set(SiphashKey { k0: 0, k1: 0 });
        return;
    }

    // new key, including timeout
    pf_syncookie_newkey();
}

/// `pf_syncookie_newkey`: switches to the other secret, made afresh, and restarts the key
/// timeout.
pub fn pf_syncookie_newkey() {
    let oe = (PF_SYNCOOKIE_STATUS.oddeven.load(Ordering::Relaxed) + 1) & 0x1;
    PF_SYNCOOKIE_STATUS.oddeven.store(oe, Ordering::Relaxed);
    PF_STATUS.syncookies_inflight[oe as usize].set(0);
    let mut secret = [0u8; PF_SYNCOOKIE_SECRET_SIZE];
    arc4random_buf(&mut secret);
    let mut k0 = [0u8; 8];
    let mut k1 = [0u8; 8];
    k0.copy_from_slice(&secret[..8]);
    k1.copy_from_slice(&secret[8..]);
    PF_SYNCOOKIE_STATUS.key[oe as usize].set(SiphashKey {
        k0: u64::from_ne_bytes(k0),
        k1: u64::from_ne_bytes(k1),
    });
    timeout_add_sec(
        &PF_SYNCOOKIE_STATUS.keytimeout,
        PF_SYNCOOKIE_SECRET_LIFETIME,
    );
}

/// `pf_syncookie_mac`: the keyed hash of the connection, the client's sequence number and the
/// cookie byte.
pub fn pf_syncookie_mac(pd: &mut PfPdesc, cookie: PfSyncookie, seq: u32) -> u32 {
    kassert!(i32::from(pd.proto) == IPPROTO_TCP);

    let mut ctx = SipHash24Ctx::new(&PF_SYNCOOKIE_STATUS.key[usize::from(cookie.oddeven())].get());

    let alen = match pd.af {
        AF_INET => 4,
        AF_INET6 => 16,
        _ => panic(format_args!("unknown address family")),
    };
    let src = pd.ld_addr_n(pd.src, alen);
    let dst = pd.ld_addr_n(pd.dst, alen);
    ctx.update(&src.addr8[..alen]);
    ctx.update(&dst.addr8[..alen]);

    let sport = pd.ld16(pd.sport);
    let dport = pd.ld16(pd.dport);
    ctx.update(&sport.to_ne_bytes());
    ctx.update(&dport.to_ne_bytes());
    ctx.update(&seq.to_ne_bytes());
    ctx.update(&[cookie.cookie]);
    let siphash = ctx.finalize();

    let w0 = u32::from_ne_bytes([siphash[0], siphash[1], siphash[2], siphash[3]]);
    let w1 = u32::from_ne_bytes([siphash[4], siphash[5], siphash[6], siphash[7]]);
    w0 ^ w1
}

/// `pf_syncookie_generate`: the initial sequence number of the SYN|ACK, a cookie for the SYN
/// in `pd` with the MSS `mss`.
pub fn pf_syncookie_generate(pd: &mut PfPdesc, mss: u16) -> u32 {
    let mut cookie = PfSyncookie { cookie: 0 };

    // map MSS
    let mut i = PF_SYNCOOKIE_MSSTAB.len() - 1;
    while PF_SYNCOOKIE_MSSTAB[i] > i32::from(mss) && i > 0 {
        i -= 1;
    }
    cookie.set_mss_idx(i as u8);

    // map WSCALE
    let wscale = pf_get_wscale(pd);
    let mut i = PF_SYNCOOKIE_WSTAB.len() - 1;
    while PF_SYNCOOKIE_WSTAB[i] > i32::from(wscale) && i > 0 {
        i -= 1;
    }
    cookie.set_wscale_idx(i as u8);
    cookie.set_sack_ok(0); // XXX

    cookie.set_oddeven(oddeven() as u8);
    let seq = u32::from_be(pd.tcp().th_seq);
    let hash = pf_syncookie_mac(pd, cookie, seq);

    // Put the flags into the hash and XOR them to get better ISS number variance. This
    // doesn't enhance the cryptographic strength and is done to prevent the 8 cookie bits
    // from showing up directly on the wire.
    let mut iss = hash & !0xff;
    iss |= u32::from(cookie.cookie) ^ (hash >> 24);

    iss
}

/// `pf_syncookie_recreate_syn`: the SYN the client sent, rebuilt from the cookie its ACK
/// carries, to create the state with.
pub fn pf_syncookie_recreate_syn(pd: &mut PfPdesc, reason: &mut u16) -> Option<&'static Mbuf> {
    let seq = u32::from_be(pd.tcp().th_seq).wrapping_sub(1);
    let ack = u32::from_be(pd.tcp().th_ack).wrapping_sub(1);
    let cookie = PfSyncookie {
        cookie: ((ack & 0xff) ^ (ack >> 24)) as u8,
    };

    if usize::from(cookie.mss_idx()) >= PF_SYNCOOKIE_MSSTAB.len()
        || usize::from(cookie.wscale_idx()) >= PF_SYNCOOKIE_WSTAB.len()
    {
        return None;
    }

    let mss = PF_SYNCOOKIE_MSSTAB[usize::from(cookie.mss_idx())] as u16;
    let wscale = PF_SYNCOOKIE_WSTAB[usize::from(cookie.wscale_idx())] as u8;

    let (src, dst) = (pd.ld_addr(pd.src), pd.ld_addr(pd.dst));
    let (sport, dport) = (pd.ld16(pd.sport), pd.ld16(pd.dport));
    pf_build_tcp(
        None,
        pd.af,
        &src,
        &dst,
        sport,
        dport,
        seq,
        0,
        TH_SYN,
        u16::from(wscale),
        mss,
        pd.ttl,
        false,
        u16::from(PF_TAG_SYNCOOKIE_RECREATED),
        cookie.sack_ok() != 0,
        u32::from(pd.rdomain),
        Some(reason),
    )
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for the SYN cookies: a cookie made for a SYN validates on the matching ACK and
    // not on another; the mode and watermark ioctls check their bounds.

    use std::sync::{Mutex as StdMutex, MutexGuard};
    use std::{assert, assert_eq};

    use super::*;
    use crate::net::pfvar_priv::PfLoc;

    /// Serialises the tests: they share `pf_status` and the secrets.
    static LOCK: StdMutex<()> = StdMutex::new(());

    fn lock() -> MutexGuard<'static, ()> {
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The client's and the server's addresses, side by side.
    struct Addrs([u8; 8]);

    /// A TCP descriptor from 192.168.1.5:40000 to 10.0.0.1:80 with `seq`, `ack` and `flags`;
    /// its addresses are in `addrs`, which must outlive it.
    fn tcp_pd(addrs: &mut Addrs, seq: u32, ack: u32, flags: u8) -> PfPdesc {
        addrs.0 = [192, 168, 1, 5, 10, 0, 0, 1];
        let mut pd = PfPdesc::new();
        pd.af = AF_INET;
        pd.proto = IPPROTO_TCP as u8;
        // SAFETY: `addrs` outlives the descriptor in every test and is not touched while it is
        // used.
        pd.src = unsafe { PfLoc::local(addrs.0.as_mut_ptr()) };
        pd.dst = pd.src.offset(4);
        pd.sport = PfLoc::Hdr(0);
        pd.dport = PfLoc::Hdr(2);
        let th = pd.tcp_mut();
        th.th_sport = 40000u16.to_be();
        th.th_dport = 80u16.to_be();
        th.th_seq = seq.to_be();
        th.th_ack = ack.to_be();
        th.th_flags = flags;
        th.set_th_off(5);
        pd
    }

    /// Fixed secrets, the even one current.
    fn keys() {
        PF_SYNCOOKIE_STATUS.oddeven.store(0, Ordering::Relaxed);
        PF_SYNCOOKIE_STATUS.key[0].set(SiphashKey {
            k0: 0x0123_4567_89ab_cdef,
            k1: 0xfedc_ba98_7654_3210,
        });
        PF_SYNCOOKIE_STATUS.key[1].set(SiphashKey { k0: 1, k1: 2 });
    }

    #[test]
    fn cookie_layout() {
        let mut c = PfSyncookie { cookie: 0 };
        c.set_oddeven(1);
        c.set_sack_ok(1);
        c.set_wscale_idx(5);
        c.set_mss_idx(7);
        assert_eq!(c.cookie, 0b1111_0111);
        assert_eq!(
            (c.oddeven(), c.sack_ok(), c.wscale_idx(), c.mss_idx()),
            (1, 1, 5, 7)
        );
    }

    #[test]
    fn generated_cookie_validates() {
        let _g = lock();
        keys();
        let mut a1 = Addrs([0; 8]);
        let mut syn = tcp_pd(&mut a1, 1000, 0, TH_SYN);
        let iss = pf_syncookie_generate(&mut syn, 1460);
        // MSS 1460 is the last entry of the table; no window scale option (0) stops the C's
        // downward search at index 1, the first entry not above it.
        let cookie = PfSyncookie {
            cookie: ((iss & 0xff) ^ (iss >> 24)) as u8,
        };
        assert_eq!(cookie.mss_idx(), 7);
        assert_eq!(cookie.wscale_idx(), 1);
        assert_eq!(cookie.oddeven(), 0);

        PF_STATUS.syncookies_inflight[0].set(1);
        let mut a2 = Addrs([0; 8]);
        let mut ack = tcp_pd(&mut a2, 1001, iss.wrapping_add(1), TH_ACK);
        assert_eq!(pf_syncookie_validate(&mut ack), 1);
        assert_eq!(PF_STATUS.syncookies_inflight[0].get(), 0);
    }

    #[test]
    fn wrong_ack_does_not_validate() {
        let _g = lock();
        keys();
        let mut a1 = Addrs([0; 8]);
        let mut syn = tcp_pd(&mut a1, 5000, 0, TH_SYN);
        let iss = pf_syncookie_generate(&mut syn, 536);

        PF_STATUS.syncookies_inflight[0].set(1);
        PF_STATUS.syncookies_inflight[1].set(1);
        // The MAC bits differ.
        let mut a2 = Addrs([0; 8]);
        let mut ack = tcp_pd(&mut a2, 5001, (iss ^ 0x0100_0000).wrapping_add(1), TH_ACK);
        assert_eq!(pf_syncookie_validate(&mut ack), 0);
        // The sequence number differs.
        let mut a3 = Addrs([0; 8]);
        let mut ack = tcp_pd(&mut a3, 7001, iss.wrapping_add(1), TH_ACK);
        assert_eq!(pf_syncookie_validate(&mut ack), 0);
        // Nothing in flight under the cookie's secret.
        PF_STATUS.syncookies_inflight[0].set(0);
        let mut a4 = Addrs([0; 8]);
        let mut ack = tcp_pd(&mut a4, 5001, iss.wrapping_add(1), TH_ACK);
        assert_eq!(pf_syncookie_validate(&mut ack), 0);
        PF_STATUS.syncookies_inflight[1].set(0);
    }

    #[test]
    fn setmode_and_watermarks_check_bounds() {
        let _g = lock();
        assert_eq!(
            pf_syncookies_setmode(PF_SYNCOOKIES_MODE_MAX + 1),
            Err(Errno::EINVAL)
        );
        assert_eq!(pf_syncookies_setmode(PF_SYNCOOKIES_ADAPTIVE), Ok(()));
        assert_eq!(PF_STATUS.syncookies_mode.get(), PF_SYNCOOKIES_ADAPTIVE);
        assert_eq!(pf_syncookies_setmode(PF_SYNCOOKIES_NEVER), Ok(()));
        assert_eq!(PF_STATUS.syncookies_mode.get(), PF_SYNCOOKIES_NEVER);

        assert_eq!(pf_syncookies_setwats(10, 20), Err(Errno::EINVAL));
        assert_eq!(pf_syncookies_setwats(2500, 1250), Ok(()));
        let mut w = PfiocSynflwats::default();
        assert_eq!(pf_syncookies_getwats(&mut w), Ok(()));
        assert_eq!((w.hiwat, w.lowat), (2500, 1250));
        assert!(pf_syncookies_setwats(7, 7).is_ok());
    }

    #[test]
    fn synflood_check_follows_the_mode() {
        let _g = lock();
        let mut a1 = Addrs([0; 8]);
        let mut syn = tcp_pd(&mut a1, 1, 0, TH_SYN);
        PF_STATUS.syncookies_mode.set(PF_SYNCOOKIES_NEVER);
        assert!(!pf_synflood_check(&mut syn));
        PF_STATUS.syncookies_mode.set(PF_SYNCOOKIES_ALWAYS);
        assert!(pf_synflood_check(&mut syn));
        // Adaptive below the high watermark: no flood.
        PF_STATUS.syncookies_mode.set(PF_SYNCOOKIES_ADAPTIVE);
        PF_STATUS.syncookies_active.set(0);
        PF_SYNCOOKIE_STATUS.hiwat.set(100);
        PF_STATUS.states_halfopen.set(10);
        assert!(!pf_synflood_check(&mut syn));
        PF_STATUS.syncookies_mode.set(PF_SYNCOOKIES_NEVER);
        PF_STATUS.states_halfopen.set(0);
    }
}
/* </TESTS> */
