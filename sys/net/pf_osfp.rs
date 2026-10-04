/*	$OpenBSD: pf_osfp.c,v 1.50 2026/09/13 03:27:17 deraadt Exp $ */
/* <LICENSES> */

/*
 * Copyright (c) 2003 Mike Frantzen <frantzen@w4g.org>
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
 *
 */
/* </LICENSES> */

//! Passive operating system fingerprinting (`net/pf_osfp.c`): the fingerprints `pfctl(8)`
//! loads from `pf.os(5)`, matched against the TCP SYNs of IPv4 packets for rules with
//! `os "name"`.
//!
//! Upstream: sys/net/pf_osfp.c @ 3ce1f3f79392
//!
//! The fingerprints are a list (`pf_osfp_list`), each with the list of operating systems it
//! identifies (`fp_oses`); both are pool items. A SYN's fingerprint (window, TTL, DF, packet
//! size, TCP options, MSS, window scale) is looked up with the TTL allowed to have dropped
//! by up to `PF_OSFP_MAXTTL_OFFSET` hops, and the rule's packed OS id is matched against
//! the entries found.
//!
//! The C file is shared with userland (`tcpdump(8)` lends its fingerprinting code): only the
//! `_KERNEL` side is here. The `#else` branch (the userland `pool_t`/`pool_get`/`pool_put`
//! over `malloc`, the empty `PF_LOCK` macros, `getnameinfo` for the source name) is not
//! compiled in the kernel and is not ported.
//!
//! ## Deviations
//! - `pf_osfp_fingerprint_hdr` takes no `struct ip6_hdr *`: its only use is under `#ifdef
//!   INET6` (not configured), so an IPv6 packet (no IPv4 header) gets NULL, as in the C
//!   without INET6: `pf_osfp_fingerprint` passes no IPv4 header for an `AF_INET6` packet.
//!   The TCP header and its options come as the bytes `pf_pull_hdr` copied (`th_off << 2` of
//!   them); options running past those bytes end the parse with no fingerprint.
//! - `inet_ntop(AF_INET, &ip->ip_src, ...)` (not ported) is a dotted quad formatter for the
//!   debug line (`InAddrFmt`).
//! - The fingerprint and entry are filled in (from the ioctl) when they are taken from their
//!   pools in `pf_osfp_add`, before they are published, rather than after the lookup; the
//!   unused one goes back to its pool as in C.
//! - `PFDEBUG` is not configured: the `pf_osfp_validate()` call in `pf_osfp_add` is a comment
//!   at its site (the function is ported).
//! - The C functions returning `int` 0/1 return `bool` (`pf_osfp_match`), and those
//!   returning an errno return `Result<(), Errno>` (`pf_osfp_add`, `pf_osfp_get`).

use core::fmt;

use crate::kassert;
use crate::kern::subr_pool::pool_get;
use crate::kern::subr_prf::Str;
use crate::machine::intr::IPL_NONE;
use crate::net::pf::pf_pull_hdr;
use crate::net::pfvar::{
    PF_OSFP_ANY, PF_OSFP_DF, PF_OSFP_MAXTTL_OFFSET, PF_OSFP_MSS_DC, PF_OSFP_MSS_MOD,
    PF_OSFP_PSIZE_DC, PF_OSFP_PSIZE_MOD, PF_OSFP_TCPOPT_BITS, PF_OSFP_TCPOPT_MSS,
    PF_OSFP_TCPOPT_NOP, PF_OSFP_TCPOPT_SACK, PF_OSFP_TCPOPT_TS, PF_OSFP_TCPOPT_WSCALE, PF_OSFP_TS0,
    PF_OSFP_UNKNOWN, PF_OSFP_WSCALE_DC, PF_OSFP_WSCALE_MOD, PF_OSFP_WSIZE_DC, PF_OSFP_WSIZE_MOD,
    PF_OSFP_WSIZE_MSS, PF_OSFP_WSIZE_MTU, PfOsFingerprint, PfOsfp, PfOsfpEnlist, PfOsfpEntry,
    PfOsfpIoctl, PfOsfpIoctlEntry, PfPoolItem, pf_cstr, pf_osfp_entry_eq, pf_osfp_unpack,
    pf_pool_init, pf_pool_put,
};
use crate::net::pfvar_priv::{PfGlobal, PfPdesc, pf_assert_locked, pf_lock, pf_unlock};
use crate::netinet::in_::{IPPROTO_TCP, InAddr};
use crate::netinet::ip::{IP_DF, IP_OFFMASK, Ip};
use crate::netinet::tcp::{
    TCPOLEN_MAXSEG, TCPOLEN_TIMESTAMP, TCPOLEN_WINDOW, TCPOPT_EOL, TCPOPT_MAXSEG, TCPOPT_NOP,
    TCPOPT_SACK_PERMITTED, TCPOPT_TIMESTAMP, TCPOPT_WINDOW, TH_ACK, TH_SYN, Tcphdr,
};
use crate::sys::errno::Errno;
use crate::sys::mbuf::mtod;
use crate::sys::pool::{PR_LIMITFAIL, PR_WAITOK, PR_ZERO, Pool};
use crate::sys::queue::{SlistEntry, SlistHead};
use crate::sys::socket::{AF_INET, AF_INET6};
use crate::sys::syslog::{LOG_DEBUG, LOG_INFO, LOG_NOTICE};

// Protection/ownership:
//	I	immutable after pf_osfp_initialize()
//	p	pf_lock

/// `SMART_MSS`: some "smart" NAT devices and DSL routers will tweak the MSS size and will
/// set it to whatever is suitable for the link type.
const SMART_MSS: u32 = 1460;
/// `MTUOFF`: `sizeof(struct ip) + sizeof(struct tcphdr)`.
const MTUOFF: u32 = (size_of::<Ip>() + size_of::<Tcphdr>()) as u32;
/// `SMART_MTU`.
const SMART_MTU: u32 = SMART_MSS + MTUOFF;

/// A network-order IPv4 address printed as a dotted quad (what `inet_ntop(AF_INET, ...)`
/// writes; see the module's deviations).
struct InAddrFmt(InAddr);

impl fmt::Display for InAddrFmt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let b = self.0.s_addr.to_ne_bytes();
        write!(f, "{}.{}.{}.{}", b[0], b[1], b[2], b[3])
    }
}

crate::queue_adapter!(
    /// `SLIST_HEAD(pf_osfp_list, pf_os_fingerprint)`.
    pub PfOsfpList: PfOsFingerprint, fp_next => SlistEntry<PfOsFingerprint>
);

/// `pf_osfp_list`: \[p\] the fingerprints.
pub static PF_OSFP_LIST: PfGlobal<SlistHead<PfOsfpList>> = PfGlobal(SlistHead::new());
/// `pf_osfp_entry_pl`: \[I\] the pool of `struct pf_osfp_entry`.
pub static PF_OSFP_ENTRY_PL: Pool = Pool::new();
/// `pf_osfp_pl`: \[I\] the pool of `struct pf_os_fingerprint`.
pub static PF_OSFP_PL: Pool = Pool::new();

// SAFETY: a list link, integers and byte arrays: all zero is a valid value.
unsafe impl PfPoolItem for PfOsfpEntry {}
// SAFETY: a list head, a list link and integers: all zero is a valid value.
unsafe impl PfPoolItem for PfOsFingerprint {}

/// `pool_get(pp, flags)` of an item that is `v` before anyone can see it.
fn pf_osfp_pool_get<T: PfPoolItem>(pp: &Pool, flags: i32, v: T) -> Option<&'static T> {
    let mem = pool_get(pp, flags)?;
    kassert!(pp.pr_size.get() as usize >= size_of::<T>());
    let p = mem.as_ptr().cast::<T>();
    // SAFETY: the pool's items are at least `size_of::<T>()` bytes with `T`'s alignment
    // (`pf_osfp_initialize`); the item is written whole before the reference is made, and
    // stays allocated until `pool_put`.
    unsafe {
        p.write(v);
        Some(&*p)
    }
}

/// The scalar members of a fingerprint, its lists empty (the C's `memcpy` of a fingerprint
/// into a local key).
fn pf_osfp_copy(f: &PfOsFingerprint) -> PfOsFingerprint {
    PfOsFingerprint {
        fp_tcpopts: f.fp_tcpopts,
        fp_wsize: f.fp_wsize,
        fp_psize: f.fp_psize,
        fp_mss: f.fp_mss,
        fp_flags: f.fp_flags,
        fp_optcnt: f.fp_optcnt,
        fp_wscale: f.fp_wscale,
        fp_ttl: f.fp_ttl,
        ..PfOsFingerprint::default()
    }
}

/// The `struct pf_osfp_entry` of an ioctl (`memcpy(entry, &fpioc->fp_os, sizeof(*entry))`;
/// the link the user passed is not kept: the entry is linked right after).
fn pf_osfp_entry_from_ioctl(e: &PfOsfpIoctlEntry) -> PfOsfpEntry {
    PfOsfpEntry {
        fp_entry: SlistEntry::new(),
        fp_os: e.fp_os,
        fp_enflags: e.fp_enflags,
        fp_class_nm: e.fp_class_nm,
        fp_version_nm: e.fp_version_nm,
        fp_subtype_nm: e.fp_subtype_nm,
    }
}

/// The names of the first OS of a fingerprint, for the debug lines.
fn pf_osfp_first_names(f: &PfOsFingerprint) -> (Str<'_>, Str<'_>, Str<'_>) {
    match f.fp_oses.first() {
        Some(e) => (
            Str(pf_cstr(&e.fp_class_nm)),
            Str(pf_cstr(&e.fp_version_nm)),
            Str(pf_cstr(&e.fp_subtype_nm)),
        ),
        None => (Str(b""), Str(b""), Str(b"")),
    }
}

/// `pf_osfp_fingerprint`: passively fingerprint the OS of the host (IPv4 TCP SYN packets
/// only). Returns the list of possible OSes.
pub fn pf_osfp_fingerprint(pd: &mut PfPdesc) -> Option<&'static SlistHead<PfOsfpEnlist>> {
    if i32::from(pd.proto) != IPPROTO_TCP {
        return None;
    }
    let m = pd.m?;

    let ip = match pd.af {
        // SAFETY: pf pulled the IPv4 header into the first mbuf (`pf_setup_pdesc`).
        AF_INET => Some(unsafe { mtod::<Ip>(m).read_unaligned() }),
        // ip6 = mtod(pd->m, struct ip6_hdr *): used under INET6 only (not configured).
        AF_INET6 => None,
        _ => None,
    };
    let mut hdr = [0u8; 60];
    let len = usize::from(pd.tcp().th_off()) << 2;
    if !pf_pull_hdr(m, pd.off as i32, &mut hdr[..len], None, pd.af) {
        return None;
    }

    pf_osfp_fingerprint_hdr(ip.as_ref(), &hdr[..len])
}

/// `pf_osfp_fingerprint_hdr`: the operating systems whose fingerprint matches the SYN with
/// IPv4 header `ip` (none: not IPv4, see the module's deviations) and TCP header and options
/// `tcp`.
pub fn pf_osfp_fingerprint_hdr(
    ip: Option<&Ip>,
    tcp: &[u8],
) -> Option<&'static SlistHead<PfOsfpEnlist>> {
    if tcp.len() < size_of::<Tcphdr>() {
        return None;
    }
    // SAFETY: `tcp` holds at least a TCP header; `Tcphdr` is plain integers.
    let th: Tcphdr = unsafe { tcp.as_ptr().cast::<Tcphdr>().read_unaligned() };

    if th.th_flags & (TH_SYN | TH_ACK) != TH_SYN {
        return None;
    }
    if let Some(ip) = ip
        && u16::from_be(ip.ip_off) & IP_OFFMASK != 0
    {
        return None;
    }

    let mut fp = PfOsFingerprint::default();

    // INET6: else if ip6, the IPv6 branch (fp_psize from ip6_plen, fp_ttl from ip6_hlim,
    // PF_OSFP_DF | PF_OSFP_INET6); not configured. Neither header: no fingerprint.
    let ip = ip?;
    fp.fp_psize = u16::from_be(ip.ip_len);
    fp.fp_ttl = ip.ip_ttl;
    if u16::from_be(ip.ip_off) & IP_DF != 0 {
        fp.fp_flags |= PF_OSFP_DF;
    }
    let srcname = InAddrFmt(ip.ip_src);
    fp.fp_wsize = u16::from_be(th.th_win);

    let hlen = usize::from(th.th_off()) << 2;
    let mut cnt = hlen as i32 - size_of::<Tcphdr>() as i32;
    let mut off = size_of::<Tcphdr>();
    while cnt > 0 {
        let opt = *tcp.get(off)?;
        if opt == TCPOPT_EOL {
            break;
        }

        fp.fp_optcnt = fp.fp_optcnt.wrapping_add(1);
        let mut optlen: i32;
        if opt == TCPOPT_NOP {
            fp.fp_tcpopts = (fp.fp_tcpopts << PF_OSFP_TCPOPT_BITS) | PF_OSFP_TCPOPT_NOP;
            optlen = 1;
        } else {
            if cnt < 2 {
                return None;
            }
            optlen = i32::from(*tcp.get(off + 1)?);
            if optlen > cnt || optlen < 2 {
                return None;
            }
            let o = tcp.get(off..off + optlen as usize)?;
            match opt {
                TCPOPT_MAXSEG => {
                    if optlen >= i32::from(TCPOLEN_MAXSEG) {
                        fp.fp_mss = u16::from_ne_bytes([o[2], o[3]]);
                    }
                    fp.fp_tcpopts = (fp.fp_tcpopts << PF_OSFP_TCPOPT_BITS) | PF_OSFP_TCPOPT_MSS;
                    fp.fp_mss = u16::from_be(fp.fp_mss);
                }
                TCPOPT_WINDOW => {
                    if optlen >= i32::from(TCPOLEN_WINDOW) {
                        fp.fp_wscale = o[2];
                    }
                    fp.fp_tcpopts = (fp.fp_tcpopts << PF_OSFP_TCPOPT_BITS) | PF_OSFP_TCPOPT_WSCALE;
                }
                TCPOPT_SACK_PERMITTED => {
                    fp.fp_tcpopts = (fp.fp_tcpopts << PF_OSFP_TCPOPT_BITS) | PF_OSFP_TCPOPT_SACK;
                }
                TCPOPT_TIMESTAMP => {
                    if optlen >= i32::from(TCPOLEN_TIMESTAMP) {
                        let ts = u32::from_ne_bytes([o[2], o[3], o[4], o[5]]);
                        if ts == 0 {
                            fp.fp_flags |= PF_OSFP_TS0;
                        }
                    }
                    fp.fp_tcpopts = (fp.fp_tcpopts << PF_OSFP_TCPOPT_BITS) | PF_OSFP_TCPOPT_TS;
                }
                _ => return None,
            }
        }
        optlen = optlen.max(1); // paranoia
        cnt -= optlen;
        off += optlen as usize;
    }

    crate::dpfprintf!(
        LOG_INFO,
        "fingerprinted {}:{}  {}:{}:{}:{}:{:x} ({}) (TS={},M={}{},W={}{})",
        srcname,
        u16::from_be(th.th_sport),
        fp.fp_wsize,
        fp.fp_ttl,
        u8::from(fp.fp_flags & PF_OSFP_DF != 0),
        fp.fp_psize,
        fp.fp_tcpopts,
        fp.fp_optcnt,
        if fp.fp_flags & PF_OSFP_TS0 != 0 {
            "0"
        } else {
            ""
        },
        mod_dc(fp.fp_flags, PF_OSFP_MSS_MOD, PF_OSFP_MSS_DC),
        fp.fp_mss,
        mod_dc(fp.fp_flags, PF_OSFP_WSCALE_MOD, PF_OSFP_WSCALE_DC),
        fp.fp_wscale
    );

    pf_osfp_find(&fp, PF_OSFP_MAXTTL_OFFSET).map(|f| &f.fp_oses)
}

/// The `%`/`*` prefix of a modulus or don't-care member in the debug lines.
fn mod_dc(flags: u16, mod_: u16, dc: u16) -> &'static str {
    if flags & mod_ != 0 {
        "%"
    } else if flags & dc != 0 {
        "*"
    } else {
        ""
    }
}

/// `pf_osfp_match`: match a fingerprint ID against a list of OSes.
pub fn pf_osfp_match(list: Option<&SlistHead<PfOsfpEnlist>>, os: PfOsfp) -> bool {
    if os == PF_OSFP_ANY {
        return true;
    }
    let Some(list) = list else {
        crate::dpfprintf!(LOG_INFO, "osfp no match against {:x}", os);
        return os == PF_OSFP_UNKNOWN;
    };
    let (os_class, os_version, os_subtype) = pf_osfp_unpack(os);
    for entry in list.iter() {
        let (en_class, en_version, en_subtype) = pf_osfp_unpack(entry.fp_os);
        if (os_class == PF_OSFP_ANY || en_class == os_class)
            && (os_version == PF_OSFP_ANY || en_version == os_version)
            && (os_subtype == PF_OSFP_ANY || en_subtype == os_subtype)
        {
            crate::dpfprintf!(
                LOG_INFO,
                "osfp matched {} {} {}  {:x}=={:x}",
                Str(pf_cstr(&entry.fp_class_nm)),
                Str(pf_cstr(&entry.fp_version_nm)),
                Str(pf_cstr(&entry.fp_subtype_nm)),
                os,
                entry.fp_os
            );
            return true;
        }
    }
    crate::dpfprintf!(LOG_INFO, "fingerprint 0x{:x} didn't match", os);
    false
}

/// `pf_osfp_initialize`: initialize the OS fingerprint system.
pub fn pf_osfp_initialize() {
    pf_pool_init::<PfOsfpEntry>(&PF_OSFP_ENTRY_PL, IPL_NONE, PR_WAITOK, "pfosfpen");
    pf_pool_init::<PfOsFingerprint>(&PF_OSFP_PL, IPL_NONE, PR_WAITOK, "pfosfp");
}

/// `pf_osfp_flush`: flush the fingerprint list.
pub fn pf_osfp_flush() {
    pf_lock();
    while let Some(fp) = PF_OSFP_LIST.first() {
        // SAFETY: the list is not empty.
        unsafe { PF_OSFP_LIST.remove_head() };
        while let Some(entry) = fp.fp_oses.first() {
            // SAFETY: the list is not empty.
            unsafe { fp.fp_oses.remove_head() };
            pf_pool_put(&PF_OSFP_ENTRY_PL, entry);
        }
        pf_pool_put(&PF_OSFP_PL, fp);
    }
    pf_unlock();
}

/// `pf_osfp_add`: add a fingerprint (`DIOCOSFPADD`).
pub fn pf_osfp_add(fpioc: &PfOsfpIoctl) -> Result<(), Errno> {
    let os = &fpioc.fp_os;
    for nm in [
        &os.fp_class_nm[..],
        &os.fp_version_nm[..],
        &os.fp_subtype_nm[..],
    ] {
        if libkern::strnlen(nm, nm.len()) >= nm.len() {
            return Err(Errno::ENAMETOOLONG);
        }
    }

    let fpadd = PfOsFingerprint {
        fp_tcpopts: fpioc.fp_tcpopts,
        fp_wsize: fpioc.fp_wsize,
        fp_psize: fpioc.fp_psize,
        fp_mss: fpioc.fp_mss,
        fp_flags: fpioc.fp_flags,
        fp_optcnt: fpioc.fp_optcnt,
        fp_wscale: fpioc.fp_wscale,
        fp_ttl: fpioc.fp_ttl,
        ..PfOsFingerprint::default()
    };

    let f = fpadd.fp_flags;
    crate::dpfprintf!(
        LOG_DEBUG,
        "adding osfp {} {} {} = {}{}:{}:{}:{}{}:0x{:x} {} (TS={},M={}{},W={}{}) {:x}",
        Str(pf_cstr(&os.fp_class_nm)),
        Str(pf_cstr(&os.fp_version_nm)),
        Str(pf_cstr(&os.fp_subtype_nm)),
        if f & PF_OSFP_WSIZE_MOD != 0 {
            "%"
        } else if f & PF_OSFP_WSIZE_MSS != 0 {
            "S"
        } else if f & PF_OSFP_WSIZE_MTU != 0 {
            "T"
        } else if f & PF_OSFP_WSIZE_DC != 0 {
            "*"
        } else {
            ""
        },
        fpadd.fp_wsize,
        fpadd.fp_ttl,
        u8::from(f & PF_OSFP_DF != 0),
        mod_dc(f, PF_OSFP_PSIZE_MOD, PF_OSFP_PSIZE_DC),
        fpadd.fp_psize,
        fpadd.fp_tcpopts,
        fpadd.fp_optcnt,
        if f & PF_OSFP_TS0 != 0 { "0" } else { "" },
        mod_dc(f, PF_OSFP_MSS_MOD, PF_OSFP_MSS_DC),
        fpadd.fp_mss,
        mod_dc(f, PF_OSFP_WSCALE_MOD, PF_OSFP_WSCALE_DC),
        fpadd.fp_wscale,
        os.fp_os
    );

    let Some(entry) = pf_osfp_pool_get(
        &PF_OSFP_ENTRY_PL,
        PR_WAITOK | PR_LIMITFAIL,
        pf_osfp_entry_from_ioctl(os),
    ) else {
        return Err(Errno::ENOMEM);
    };

    let Some(prealloc) = pf_osfp_pool_get(
        &PF_OSFP_PL,
        PR_WAITOK | PR_ZERO | PR_LIMITFAIL,
        pf_osfp_copy(&fpadd),
    ) else {
        pf_pool_put(&PF_OSFP_ENTRY_PL, entry);
        return Err(Errno::ENOMEM);
    };
    let mut fp_prealloc = Some(prealloc);

    pf_lock();
    let fp = match pf_osfp_find_exact(&fpadd) {
        Some(fp) => {
            if fp
                .fp_oses
                .iter()
                .any(|tentry| pf_osfp_entry_eq(tentry, entry))
            {
                pf_unlock();
                pf_pool_put(&PF_OSFP_ENTRY_PL, entry);
                pf_pool_put(&PF_OSFP_PL, prealloc);
                return Err(Errno::EEXIST);
            }
            fp
        }
        None => {
            let fp = prealloc;
            fp_prealloc = None;
            fp.fp_oses.init();
            pf_osfp_insert(fp);
            fp
        }
    };

    // SAFETY: the new entry is on no list; it stays allocated until `pf_osfp_flush`.
    unsafe { fp.fp_oses.insert_head(entry) };
    pf_unlock();

    // PFDEBUG: if ((fp = pf_osfp_validate())) DPFPRINTF(LOG_NOTICE, "Invalid fingerprint
    // list"); not configured.

    if let Some(p) = fp_prealloc {
        pf_pool_put(&PF_OSFP_PL, p);
    }

    Ok(())
}

/// `pf_osfp_find`: find a fingerprint in the list, the list's TTL at most `ttldiff` above
/// the packet's.
pub fn pf_osfp_find(find: &PfOsFingerprint, ttldiff: u8) -> Option<&'static PfOsFingerprint> {
    pf_assert_locked();

    // MATCH_INT: the member must be equal, or a multiple of the list's (modulus), or is not
    // looked at (don't care).
    let match_int = |f: &PfOsFingerprint, mod_: u16, dc: u16, fv: u16, findv: u16| -> bool {
        if f.fp_flags & dc == 0 {
            if f.fp_flags & mod_ == 0 {
                if fv != findv {
                    return false;
                }
            } else if fv == 0 || !findv.is_multiple_of(fv) {
                return false;
            }
        }
        true
    };

    'next: for f in PF_OSFP_LIST.iter() {
        if f.fp_tcpopts != find.fp_tcpopts
            || f.fp_optcnt != find.fp_optcnt
            || f.fp_ttl < find.fp_ttl
            || f.fp_ttl - find.fp_ttl > ttldiff
            || (f.fp_flags & (PF_OSFP_DF | PF_OSFP_TS0))
                != (find.fp_flags & (PF_OSFP_DF | PF_OSFP_TS0))
        {
            continue;
        }

        if !match_int(
            f,
            PF_OSFP_PSIZE_MOD,
            PF_OSFP_PSIZE_DC,
            f.fp_psize,
            find.fp_psize,
        ) || !match_int(f, PF_OSFP_MSS_MOD, PF_OSFP_MSS_DC, f.fp_mss, find.fp_mss)
            || !match_int(
                f,
                PF_OSFP_WSCALE_MOD,
                PF_OSFP_WSCALE_DC,
                u16::from(f.fp_wscale),
                u16::from(find.fp_wscale),
            )
        {
            continue;
        }
        if f.fp_flags & PF_OSFP_WSIZE_DC == 0 {
            let fw = u32::from(f.fp_wsize);
            let w = u32::from(find.fp_wsize);
            let mss = u32::from(find.fp_mss);
            if f.fp_flags & PF_OSFP_WSIZE_MSS != 0 {
                if mss == 0 {
                    continue 'next;
                }
                if (w % mss != 0 || w / mss != fw) && (w % SMART_MSS != 0 || w / SMART_MSS != fw) {
                    continue 'next;
                }
            } else if f.fp_flags & PF_OSFP_WSIZE_MTU != 0 {
                if mss == 0 {
                    continue 'next;
                }
                if (w % (mss + MTUOFF) != 0 || w / (mss + MTUOFF) != fw)
                    && (w % SMART_MTU != 0 || w / SMART_MTU != fw)
                {
                    continue 'next;
                }
            } else if f.fp_flags & PF_OSFP_WSIZE_MOD != 0 {
                if fw == 0 || w % fw != 0 {
                    continue 'next;
                }
            } else if fw != w {
                continue 'next;
            }
        }
        return Some(f);
    }

    None
}

/// `pf_osfp_find_exact`: find an exact fingerprint in the list.
pub fn pf_osfp_find_exact(find: &PfOsFingerprint) -> Option<&'static PfOsFingerprint> {
    pf_assert_locked();

    PF_OSFP_LIST.iter().find(|f| {
        f.fp_tcpopts == find.fp_tcpopts
            && f.fp_wsize == find.fp_wsize
            && f.fp_psize == find.fp_psize
            && f.fp_mss == find.fp_mss
            && f.fp_flags == find.fp_flags
            && f.fp_optcnt == find.fp_optcnt
            && f.fp_wscale == find.fp_wscale
            && f.fp_ttl == find.fp_ttl
    })
}

/// `pf_osfp_insert`: insert a fingerprint into the list, at its end.
pub fn pf_osfp_insert(ins: &'static PfOsFingerprint) {
    pf_assert_locked();

    // XXX need to go semi tree based.  can key on tcp options

    let prev = PF_OSFP_LIST.iter().last();
    // SAFETY: `ins` is new, on no list; it stays allocated until `pf_osfp_flush`.
    unsafe {
        match prev {
            Some(prev) => SlistHead::<PfOsfpList>::insert_after(prev, ins),
            None => PF_OSFP_LIST.insert_head(ins),
        }
    }
}

/// `pf_osfp_get`: fill a fingerprint by its number (from an ioctl, `DIOCOSFPGET`): the
/// `fp_getnum`th operating system over all fingerprints. `EBUSY` past the last.
pub fn pf_osfp_get(fpioc: &mut PfOsfpIoctl) -> Result<(), Errno> {
    let num = fpioc.fp_getnum;
    let mut i = 0;

    *fpioc = PfOsfpIoctl::default();
    pf_lock();
    for fp in PF_OSFP_LIST.iter() {
        for entry in fp.fp_oses.iter() {
            let this = i;
            i += 1;
            if this == num {
                fpioc.fp_mss = fp.fp_mss;
                fpioc.fp_wsize = fp.fp_wsize;
                fpioc.fp_flags = fp.fp_flags;
                fpioc.fp_psize = fp.fp_psize;
                fpioc.fp_ttl = fp.fp_ttl;
                fpioc.fp_wscale = fp.fp_wscale;
                fpioc.fp_getnum = num;
                fpioc.fp_os = PfOsfpIoctlEntry {
                    fp_entry: SlistHead::<PfOsfpEnlist>::next(entry)
                        .map_or(0, |n| core::ptr::from_ref(n) as usize),
                    fp_os: entry.fp_os,
                    fp_enflags: entry.fp_enflags,
                    fp_class_nm: entry.fp_class_nm,
                    fp_version_nm: entry.fp_version_nm,
                    fp_subtype_nm: entry.fp_subtype_nm,
                };
                pf_unlock();
                return Ok(());
            }
        }
    }
    pf_unlock();

    Err(Errno::EBUSY)
}

/// `pf_osfp_validate`: validate that each signature is reachable; the first that is not.
pub fn pf_osfp_validate() -> Option<&'static PfOsFingerprint> {
    pf_assert_locked();

    for f in PF_OSFP_LIST.iter() {
        let mut find = pf_osfp_copy(f);

        // We do a few MSS/th_win percolations to make things unique
        if find.fp_mss == 0 {
            find.fp_mss = 128;
        }
        let mss = u32::from(find.fp_mss);
        if f.fp_flags & PF_OSFP_WSIZE_MSS != 0 {
            find.fp_wsize = (u32::from(find.fp_wsize) * mss) as u16;
        } else if f.fp_flags & PF_OSFP_WSIZE_MTU != 0 {
            find.fp_wsize = (u32::from(find.fp_wsize) * (mss + 40)) as u16;
        } else if f.fp_flags & PF_OSFP_WSIZE_MOD != 0 {
            find.fp_wsize = find.fp_wsize.wrapping_mul(2);
        }
        let f2 = pf_osfp_find(&find, 0);
        if !f2.is_some_and(|f2| core::ptr::eq(f, f2)) {
            let (c, v, s) = pf_osfp_first_names(f);
            if let Some(f2) = f2 {
                let (c2, v2, s2) = pf_osfp_first_names(f2);
                crate::dpfprintf!(
                    LOG_NOTICE,
                    "Found \"{} {} {}\" instead of \"{} {} {}\"\n",
                    c2,
                    v2,
                    s2,
                    c,
                    v,
                    s
                );
            } else {
                crate::dpfprintf!(LOG_NOTICE, "Couldn't find \"{} {} {}\"\n", c, v, s);
            }
            return Some(f);
        }
    }
    None
}

#[cfg(test)]
mod tests;
