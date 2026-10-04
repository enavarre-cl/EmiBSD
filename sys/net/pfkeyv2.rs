/* $OpenBSD: pfkeyv2.h,v 1.95 2024/05/13 01:15:53 jsg Exp $ */
/* $OpenBSD: pfkeyv2.c,v 1.273 2026/02/10 20:24:34 tobhe Exp $ */
/* <LICENSES> */
/*
 *	@(#)COPYRIGHT	1.1 (NRL) January 1998
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
 * Copyright (c) 1995, 1996, 1997, 1998, 1999 Craig Metz. All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the author nor the names of any contributors
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
 */
/* </LICENSES> */

//! `PF_KEY` version 2 (RFC 2367 with OpenBSD's extensions): `<net/pfkeyv2.h>` (the message
//! and extension structures, the constants) and `net/pfkeyv2.c` (the `pfkeydomain` socket
//! domain, the message handler `pfkeyv2_dosend` that adds, updates, deletes and dumps SAs and
//! flows, and the messages the kernel sends on its own: `SADB_ACQUIRE`, `SADB_EXPIRE`).
//! `ipsecctl(8)` and `isakmpd(8)` talk to the kernel through it.
//!
//! Upstream: sys/net/pfkeyv2.h @ 3ce1f3f79392
//! Upstream: sys/net/pfkeyv2.c @ 3ce1f3f79392
//!
//! A message is a `struct sadb_msg` followed by extensions, each a multiple of 8 bytes whose
//! first two fields are its length (in 8-byte units) and type. The handler and the export
//! functions keep the C's `void *headers[SADB_EXT_MAX + 1]`: a pointer per extension type
//! into the message or into buffers the kernel builds, NULL when absent.
//!
//! Status: `ported` (M9c).
//!
//! ## Deviations
//! - The ABI structures are `#[repr(C)]` with the C's layout (pinned below); they are read
//!   and written in message buffers with unaligned copies ([`sadb_get`], [`sadb_put`]), so a
//!   message needs no alignment.
//! - `void *headers[]` is [`SadbHeaders`], raw pointers; the functions that read or write
//!   through them (and the `void **p` cursors of the exports) are `unsafe fn`s whose
//!   contract is the C's: every non-NULL header points at a whole extension, and a cursor at
//!   enough room.
//! - The message buffers (`malloc(M_PFKEY)` in C) are `Vec<u8>`s allocated with
//!   `try_reserve` (the C's `M_NOWAIT` failure is the same `ENOMEM`) and wiped before they
//!   are dropped where the C wipes them. The temporary TDB of `SADB_GETSPI` (`malloc(M_ZERO)`)
//!   is a local `Tdb`.
//! - `union sockaddr_union *` pointers into a message become [`SockaddrUnion`] copies of the
//!   address's `sa_len` bytes (the C's `bcopy` of the whole union may read past a 16-byte
//!   `sockaddr_in`; the bytes after `sa_len` are never compared).
//! - `struct pkpcb` is a `pkpcb_pool` item reached from `so_pcb`; `pkptable`'s list is a
//!   `TailqHead` under its rwlock; `pfkeyv2_seq`, `nregistered` and `npromisc` are atomics
//!   changed under `pfkeyv2_mtx`, as in C.
//! - The walkers' `void *` state (`struct dump_state`, `struct pfkeyv2_sysctl_walk`, the
//!   satype of `pfkeyv2_sa_flush`) is what the closures given to `tdb_walk` and
//!   `spd_table_walk` capture; `w_where` is a user address.
//! - `pfkeyv2_sysctl` follows `pr_sysctl`'s calling convention; the process it checks and
//!   whose routing table it uses is `curproc`, as in C.
//! - `NPF` (pf(4)) is configured: the `SADB_X_EXT_TAG`/`TAP` extensions.
//! - `INET6` is configured (feature `inet6`): the `SENT_IP6` flows and `AF_INET6` addresses
//!   of `pfkeyv2_policy`, `pfkeyv2_get` and `pfkeyv2_dump_policy`. Not configured, each a
//!   comment at its site: `IPSEC` and `TCP_SIGNATURE`
//!   (`SADB_X_SATYPE_TCPSIGNATURE`, `XF_TCPSIGNATURE`; M9+) are.

use alloc::vec::Vec;
use core::cell::Cell;
use core::mem::{offset_of, size_of};
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI32, AtomicU32, Ordering};

use crate::crypto::blf::BLF_MAXKEYLEN;
use crate::kassert;
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_prot::suser;
use crate::kern::kern_rwlock::{rw_enter_read, rw_enter_write, rw_exit_read, rw_exit_write};
use crate::kern::kern_synch::refcnt_init;
use crate::kern::subr_pool::{pool_get, pool_init, pool_put};
use crate::kern::subr_prf::panic;
use crate::kern::uipc_mbuf::{m_copydata, m_devget, m_dup_pkt, m_freem};
use crate::kern::uipc_socket::sorwakeup;
use crate::kern::uipc_socket2::{
    sbappendaddr, soassertlocked, socantsendmore, soisconnected, soisdisconnected, solock,
    soreserve, sounlock,
};
use crate::machine::copy::copyout;
use crate::machine::cpu::curproc;
use crate::machine::intr::{IPL_MPFLOOR, IPL_SOFTNET};
use crate::net::pfkeyv2_convert::{
    export_address, export_counter, export_flow, export_identities, export_iface, export_key,
    export_lifetime, export_mtu, export_rdomain, export_replay, export_sa, export_satype,
    export_tag, export_tap, export_udpencap, import_address, import_flow, import_identities,
    import_iface, import_key, import_lifetime, import_rdomain, import_sa, import_tag, import_tap,
    import_udpencap,
};
use crate::net::pfkeyv2_parsemessage::{
    SADB_EXTS_ALLOWED_OUT, SADB_EXTS_REQUIRED_OUT, pfkeyv2_parsemessage,
};
use crate::net::pfvar::PF_TAG_NAME_SIZE;
use crate::net::radix::{rn_addroute, rn_init, rn_match};
use crate::net::rtable::{rtable_exists, rtable_l2};
use crate::netinet::in_::{IPPROTO_AH, IPPROTO_ESP, IPPROTO_IPCOMP, IPPROTO_IPIP, SockaddrIn};
#[cfg(feature = "inet6")]
use crate::netinet::ip_ipsp::SENT_IP6;
use crate::netinet::ip_ipsp::{
    IPSEC_AUTH_HMAC_RIPEMD160, IPSEC_AUTH_HMAC_SHA1, IPSEC_AUTH_MD5, IPSEC_AUTH_SHA2_256,
    IPSEC_AUTH_SHA2_384, IPSEC_AUTH_SHA2_512, IPSEC_COMP_DEFLATE, IPSEC_ENC_3DES, IPSEC_ENC_AES,
    IPSEC_ENC_AESCTR, IPSEC_ENC_BLOWFISH, IPSEC_ENC_CAST128, IPSEC_IN_USE, IPSEC_POLICY_HEAD,
    IPSP_DENY, IPSP_DIRECTION_IN, IPSP_DIRECTION_OUT, IPSP_IPSEC_ACQUIRE, IPSP_IPSEC_DONTACQ,
    IPSP_IPSEC_REQUIRE, IPSP_IPSEC_USE, IPSP_IPSEC_USE as IPSP_IPSEC_USE_FLOW, IPSP_PERMIT,
    IPSP_POLICY_STATIC, IpsecAcquire, IpsecInit, IpsecPolicy, SENT_IP4, SockaddrEncap,
    SockaddrUnion, TDB_SADB_MTX, TDBF_IFACE, TDBF_INVALID, Tdb, XF_AH, XF_ESP, XF_IP4, XF_IPCOMP,
    XF_TCPSIGNATURE, gettdb, ipsp_ids_free, puttdb, puttdb_locked, reserve_spi, tdb_addtimeouts,
    tdb_alloc, tdb_delete, tdb_init, tdb_unlink_locked, tdb_unref, tdb_walk,
};
use crate::netinet::ip_spd::{
    IPO_TDB_MTX, IPSEC_ACQUIRE_POOL, IPSEC_POLICY_POOL, ipsec_delete_policy, ipsec_get_acquire,
    ipsec_policy_of, ipsec_unref_acquire, spd_table_add, spd_table_get, spd_table_walk,
};
use crate::netinet::ipsec_input::{
    AH_ENABLE, ESP_ENABLE, IPCOMP_ENABLE, IPSEC_DEF_AUTH, IPSEC_DEF_COMP, IPSEC_DEF_ENC,
    IPSEC_EXP_ALLOCATIONS, IPSEC_EXP_BYTES, IPSEC_EXP_FIRST_USE, IPSEC_EXP_TIMEOUT,
    IPSEC_REQUIRE_PFS, IPSEC_SOFT_ALLOCATIONS, IPSEC_SOFT_BYTES, IPSEC_SOFT_FIRST_USE,
    IPSEC_SOFT_TIMEOUT,
};
use crate::netinet::ipsec_output::UDPENCAP_ENABLE;
#[cfg(feature = "inet6")]
use crate::netinet6::in6::{In6Addr, SockaddrIn6};
use crate::queue_adapter;
use crate::sys::domain::Domain;
use crate::sys::errno::Errno;
use crate::sys::mbuf::{M_PKTHDR, M_WAIT, M_ZEROIZE, Mbuf, mtod};
use crate::sys::mutex::Mutex;
use crate::sys::pool::{PR_NOWAIT, PR_WAITOK, PR_ZERO, Pool};
use crate::sys::proc::Proc;
use crate::sys::protosw::{PR_ADDR, PR_ATOMIC, PrUsrreqs, Protosw};
use crate::sys::queue::{TailqEntry, TailqHead};
use crate::sys::rwlock::Rwlock;
#[cfg(feature = "inet6")]
use crate::sys::socket::AF_INET6;
use crate::sys::socket::{
    AF_INET, NET_KEY_SADB_DUMP, NET_KEY_SPD_DUMP, PF_KEY, SO_USELOOPBACK, SOCK_RAW,
};
use crate::sys::socketvar::{SS_NOFDREF, SS_PRIV, Socket};
use crate::sys::systm::{
    net_assert_locked, net_lock, net_lock_shared, net_unlock, net_unlock_shared,
};
use crate::sys::types::Pid;
use libkern::explicit_bzero;

/// `PF_KEY_V2`.
pub const PF_KEY_V2: u8 = 2;
/// `PFKEYV2_REVISION`.
pub const PFKEYV2_REVISION: i64 = 199806;

/// `_OPENBSD_IPSEC_API_VERSION`: this should be updated whenever the API is altered.
pub const _OPENBSD_IPSEC_API_VERSION: i32 = 2;

/// `SADB_RESERVED`.
pub const SADB_RESERVED: u8 = 0;
/// `SADB_GETSPI`.
pub const SADB_GETSPI: u8 = 1;
/// `SADB_UPDATE`.
pub const SADB_UPDATE: u8 = 2;
/// `SADB_ADD`.
pub const SADB_ADD: u8 = 3;
/// `SADB_DELETE`.
pub const SADB_DELETE: u8 = 4;
/// `SADB_GET`.
pub const SADB_GET: u8 = 5;
/// `SADB_ACQUIRE`.
pub const SADB_ACQUIRE: u8 = 6;
/// `SADB_REGISTER`.
pub const SADB_REGISTER: u8 = 7;
/// `SADB_EXPIRE`.
pub const SADB_EXPIRE: u8 = 8;
/// `SADB_FLUSH`.
pub const SADB_FLUSH: u8 = 9;
/// `SADB_DUMP`.
pub const SADB_DUMP: u8 = 10;
/// `SADB_X_PROMISC`.
pub const SADB_X_PROMISC: u8 = 11;
/// `SADB_X_ADDFLOW`.
pub const SADB_X_ADDFLOW: u8 = 12;
/// `SADB_X_DELFLOW`.
pub const SADB_X_DELFLOW: u8 = 13;
/// `SADB_X_GRPSPIS`.
pub const SADB_X_GRPSPIS: u8 = 14;
/// `SADB_X_ASKPOLICY`.
pub const SADB_X_ASKPOLICY: u8 = 15;
/// `SADB_X_SPDDUMP`.
pub const SADB_X_SPDDUMP: u8 = 16;
/// `SADB_MAX`.
pub const SADB_MAX: usize = 16;

/// `struct sadb_msg`: the message header.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SadbMsg {
    /// `sadb_msg_version`.
    pub sadb_msg_version: u8,
    /// `sadb_msg_type`.
    pub sadb_msg_type: u8,
    /// `sadb_msg_errno`.
    pub sadb_msg_errno: u8,
    /// `sadb_msg_satype`.
    pub sadb_msg_satype: u8,
    /// `sadb_msg_len`: in 64-bit words.
    pub sadb_msg_len: u16,
    /// `sadb_msg_reserved`.
    pub sadb_msg_reserved: u16,
    /// `sadb_msg_seq`.
    pub sadb_msg_seq: u32,
    /// `sadb_msg_pid`.
    pub sadb_msg_pid: u32,
}

/// `struct sadb_ext`: the header of every extension.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SadbExt {
    /// `sadb_ext_len`: in 64-bit words.
    pub sadb_ext_len: u16,
    /// `sadb_ext_type`.
    pub sadb_ext_type: u16,
}

/// `struct sadb_sa`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SadbSa {
    /// `sadb_sa_len`.
    pub sadb_sa_len: u16,
    /// `sadb_sa_exttype`.
    pub sadb_sa_exttype: u16,
    /// `sadb_sa_spi`: network order.
    pub sadb_sa_spi: u32,
    /// `sadb_sa_replay`.
    pub sadb_sa_replay: u8,
    /// `sadb_sa_state`.
    pub sadb_sa_state: u8,
    /// `sadb_sa_auth`.
    pub sadb_sa_auth: u8,
    /// `sadb_sa_encrypt`.
    pub sadb_sa_encrypt: u8,
    /// `sadb_sa_flags`.
    pub sadb_sa_flags: u32,
}

/// `struct sadb_lifetime`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SadbLifetime {
    /// `sadb_lifetime_len`.
    pub sadb_lifetime_len: u16,
    /// `sadb_lifetime_exttype`.
    pub sadb_lifetime_exttype: u16,
    /// `sadb_lifetime_allocations`.
    pub sadb_lifetime_allocations: u32,
    /// `sadb_lifetime_bytes`.
    pub sadb_lifetime_bytes: u64,
    /// `sadb_lifetime_addtime`.
    pub sadb_lifetime_addtime: u64,
    /// `sadb_lifetime_usetime`.
    pub sadb_lifetime_usetime: u64,
}

/// `struct sadb_address`: a socket address follows it.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SadbAddress {
    /// `sadb_address_len`.
    pub sadb_address_len: u16,
    /// `sadb_address_exttype`.
    pub sadb_address_exttype: u16,
    /// `sadb_address_reserved`.
    pub sadb_address_reserved: u32,
}

/// `struct sadb_key`: `sadb_key_bits` of key follow it.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SadbKey {
    /// `sadb_key_len`.
    pub sadb_key_len: u16,
    /// `sadb_key_exttype`.
    pub sadb_key_exttype: u16,
    /// `sadb_key_bits`.
    pub sadb_key_bits: u16,
    /// `sadb_key_reserved`.
    pub sadb_key_reserved: u16,
}

/// `struct sadb_ident`: a NUL-terminated identity may follow it.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SadbIdent {
    /// `sadb_ident_len`.
    pub sadb_ident_len: u16,
    /// `sadb_ident_exttype`.
    pub sadb_ident_exttype: u16,
    /// `sadb_ident_type`.
    pub sadb_ident_type: u16,
    /// `sadb_ident_reserved`.
    pub sadb_ident_reserved: u16,
    /// `sadb_ident_id`.
    pub sadb_ident_id: u64,
}

/// `struct sadb_sens`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SadbSens {
    /// `sadb_sens_len`.
    pub sadb_sens_len: u16,
    /// `sadb_sens_exttype`.
    pub sadb_sens_exttype: u16,
    /// `sadb_sens_dpd`.
    pub sadb_sens_dpd: u32,
    /// `sadb_sens_sens_level`.
    pub sadb_sens_sens_level: u8,
    /// `sadb_sens_sens_len`.
    pub sadb_sens_sens_len: u8,
    /// `sadb_sens_integ_level`.
    pub sadb_sens_integ_level: u8,
    /// `sadb_sens_integ_len`.
    pub sadb_sens_integ_len: u8,
    /// `sadb_sens_reserved`.
    pub sadb_sens_reserved: u32,
}

/// `struct sadb_prop`: `sadb_comb`s follow it.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SadbProp {
    /// `sadb_prop_len`.
    pub sadb_prop_len: u16,
    /// `sadb_prop_exttype`.
    pub sadb_prop_exttype: u16,
    /// `sadb_prop_num`.
    pub sadb_prop_num: u8,
    /// `sadb_prop_replay`.
    pub sadb_prop_replay: u8,
    /// `sadb_prop_reserved`.
    pub sadb_prop_reserved: u16,
}

/// `struct sadb_comb`: one proposal.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SadbComb {
    /// `sadb_comb_auth`.
    pub sadb_comb_auth: u8,
    /// `sadb_comb_encrypt`.
    pub sadb_comb_encrypt: u8,
    /// `sadb_comb_flags`.
    pub sadb_comb_flags: u16,
    /// `sadb_comb_auth_minbits`.
    pub sadb_comb_auth_minbits: u16,
    /// `sadb_comb_auth_maxbits`.
    pub sadb_comb_auth_maxbits: u16,
    /// `sadb_comb_encrypt_minbits`.
    pub sadb_comb_encrypt_minbits: u16,
    /// `sadb_comb_encrypt_maxbits`.
    pub sadb_comb_encrypt_maxbits: u16,
    /// `sadb_comb_reserved`.
    pub sadb_comb_reserved: u32,
    /// `sadb_comb_soft_allocations`.
    pub sadb_comb_soft_allocations: u32,
    /// `sadb_comb_hard_allocations`.
    pub sadb_comb_hard_allocations: u32,
    /// `sadb_comb_soft_bytes`.
    pub sadb_comb_soft_bytes: u64,
    /// `sadb_comb_hard_bytes`.
    pub sadb_comb_hard_bytes: u64,
    /// `sadb_comb_soft_addtime`.
    pub sadb_comb_soft_addtime: u64,
    /// `sadb_comb_hard_addtime`.
    pub sadb_comb_hard_addtime: u64,
    /// `sadb_comb_soft_usetime`.
    pub sadb_comb_soft_usetime: u64,
    /// `sadb_comb_hard_usetime`.
    pub sadb_comb_hard_usetime: u64,
}

/// `struct sadb_supported`: `sadb_alg`s follow it.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SadbSupported {
    /// `sadb_supported_len`.
    pub sadb_supported_len: u16,
    /// `sadb_supported_exttype`.
    pub sadb_supported_exttype: u16,
    /// `sadb_supported_reserved`.
    pub sadb_supported_reserved: u32,
}

/// `struct sadb_alg`: one supported algorithm.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SadbAlg {
    /// `sadb_alg_id`.
    pub sadb_alg_id: u8,
    /// `sadb_alg_ivlen`.
    pub sadb_alg_ivlen: u8,
    /// `sadb_alg_minbits`.
    pub sadb_alg_minbits: u16,
    /// `sadb_alg_maxbits`.
    pub sadb_alg_maxbits: u16,
    /// `sadb_alg_reserved`.
    pub sadb_alg_reserved: u16,
}

/// `struct sadb_spirange`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SadbSpirange {
    /// `sadb_spirange_len`.
    pub sadb_spirange_len: u16,
    /// `sadb_spirange_exttype`.
    pub sadb_spirange_exttype: u16,
    /// `sadb_spirange_min`.
    pub sadb_spirange_min: u32,
    /// `sadb_spirange_max`.
    pub sadb_spirange_max: u32,
    /// `sadb_spirange_reserved`.
    pub sadb_spirange_reserved: u32,
}

/// `struct sadb_protocol`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SadbProtocol {
    /// `sadb_protocol_len`.
    pub sadb_protocol_len: u16,
    /// `sadb_protocol_exttype`.
    pub sadb_protocol_exttype: u16,
    /// `sadb_protocol_proto`.
    pub sadb_protocol_proto: u8,
    /// `sadb_protocol_direction`.
    pub sadb_protocol_direction: u8,
    /// `sadb_protocol_flags`.
    pub sadb_protocol_flags: u8,
    /// `sadb_protocol_reserved2`.
    pub sadb_protocol_reserved2: u8,
}

/// `struct sadb_x_policy`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SadbXPolicy {
    /// `sadb_x_policy_len`.
    pub sadb_x_policy_len: u16,
    /// `sadb_x_policy_exttype`.
    pub sadb_x_policy_exttype: u16,
    /// `sadb_x_policy_seq`.
    pub sadb_x_policy_seq: u32,
}

/// `struct sadb_x_udpencap`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SadbXUdpencap {
    /// `sadb_x_udpencap_len`.
    pub sadb_x_udpencap_len: u16,
    /// `sadb_x_udpencap_exttype`.
    pub sadb_x_udpencap_exttype: u16,
    /// `sadb_x_udpencap_port`.
    pub sadb_x_udpencap_port: u16,
    /// `sadb_x_udpencap_reserved`.
    pub sadb_x_udpencap_reserved: u16,
}

/// `struct sadb_x_tag`: the tag name follows it.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SadbXTag {
    /// `sadb_x_tag_len`.
    pub sadb_x_tag_len: u16,
    /// `sadb_x_tag_exttype`.
    pub sadb_x_tag_exttype: u16,
    /// `sadb_x_tag_taglen`.
    pub sadb_x_tag_taglen: u32,
}

/// `struct sadb_x_replay`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SadbXReplay {
    /// `sadb_x_replay_len`.
    pub sadb_x_replay_len: u16,
    /// `sadb_x_replay_exttype`.
    pub sadb_x_replay_exttype: u16,
    /// `sadb_x_replay_reserved`.
    pub sadb_x_replay_reserved: u32,
    /// `sadb_x_replay_count`.
    pub sadb_x_replay_count: u64,
}

/// `struct sadb_x_rdomain`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SadbXRdomain {
    /// `sadb_x_rdomain_len`.
    pub sadb_x_rdomain_len: u16,
    /// `sadb_x_rdomain_exttype`.
    pub sadb_x_rdomain_exttype: u16,
    /// `sadb_x_rdomain_dom1`.
    pub sadb_x_rdomain_dom1: u16,
    /// `sadb_x_rdomain_dom2`.
    pub sadb_x_rdomain_dom2: u16,
}

/// `struct sadb_x_tap`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SadbXTap {
    /// `sadb_x_tap_len`.
    pub sadb_x_tap_len: u16,
    /// `sadb_x_tap_exttype`.
    pub sadb_x_tap_exttype: u16,
    /// `sadb_x_tap_unit`.
    pub sadb_x_tap_unit: u32,
}

/// `struct sadb_x_counter`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SadbXCounter {
    /// `sadb_x_counter_len`.
    pub sadb_x_counter_len: u16,
    /// `sadb_x_counter_exttype`.
    pub sadb_x_counter_exttype: u16,
    /// `sadb_x_counter_pad`.
    pub sadb_x_counter_pad: u32,
    /// Input IPsec packets.
    pub sadb_x_counter_ipackets: u64,
    /// Output IPsec packets.
    pub sadb_x_counter_opackets: u64,
    /// Input bytes.
    pub sadb_x_counter_ibytes: u64,
    /// Output bytes.
    pub sadb_x_counter_obytes: u64,
    /// Dropped on input.
    pub sadb_x_counter_idrops: u64,
    /// Dropped on output.
    pub sadb_x_counter_odrops: u64,
    /// Input bytes, decompressed.
    pub sadb_x_counter_idecompbytes: u64,
    /// Output bytes, uncompressed.
    pub sadb_x_counter_ouncompbytes: u64,
}

/// `struct sadb_x_mtu`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SadbXMtu {
    /// `sadb_x_mtu_len`.
    pub sadb_x_mtu_len: u16,
    /// `sadb_x_mtu_exttype`.
    pub sadb_x_mtu_exttype: u16,
    /// `sadb_x_mtu_mtu`.
    pub sadb_x_mtu_mtu: u32,
}

/// `struct sadb_x_iface`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SadbXIface {
    /// `sadb_x_iface_len`.
    pub sadb_x_iface_len: u16,
    /// `sadb_x_iface_exttype`.
    pub sadb_x_iface_exttype: u16,
    /// `sadb_x_iface_unit`.
    pub sadb_x_iface_unit: u32,
    /// `sadb_x_iface_direction`.
    pub sadb_x_iface_direction: u8,
    /// `sadb_x_iface_reserved`.
    pub sadb_x_iface_reserved: [u8; 7],
}

/// `SADB_X_GETSPROTO(x)`: the IPsec protocol of SA type `x`.
#[allow(non_snake_case)] // the C macro's name
pub const fn SADB_X_GETSPROTO(x: u8) -> u8 {
    (match x {
        SADB_SATYPE_AH => IPPROTO_AH,
        SADB_SATYPE_ESP => IPPROTO_ESP,
        SADB_X_SATYPE_TCPSIGNATURE => crate::netinet::in_::IPPROTO_TCP,
        SADB_X_SATYPE_IPCOMP => IPPROTO_IPCOMP,
        _ => IPPROTO_IPIP,
    }) as u8
}

/// `SADB_EXT_RESERVED`.
pub const SADB_EXT_RESERVED: u16 = 0;
/// `SADB_EXT_SA`.
pub const SADB_EXT_SA: u16 = 1;
/// `SADB_EXT_LIFETIME_CURRENT`.
pub const SADB_EXT_LIFETIME_CURRENT: u16 = 2;
/// `SADB_EXT_LIFETIME_HARD`.
pub const SADB_EXT_LIFETIME_HARD: u16 = 3;
/// `SADB_EXT_LIFETIME_SOFT`.
pub const SADB_EXT_LIFETIME_SOFT: u16 = 4;
/// `SADB_EXT_ADDRESS_SRC`.
pub const SADB_EXT_ADDRESS_SRC: u16 = 5;
/// `SADB_EXT_ADDRESS_DST`.
pub const SADB_EXT_ADDRESS_DST: u16 = 6;
/// `SADB_EXT_ADDRESS_PROXY`.
pub const SADB_EXT_ADDRESS_PROXY: u16 = 7;
/// `SADB_EXT_KEY_AUTH`.
pub const SADB_EXT_KEY_AUTH: u16 = 8;
/// `SADB_EXT_KEY_ENCRYPT`.
pub const SADB_EXT_KEY_ENCRYPT: u16 = 9;
/// `SADB_EXT_IDENTITY_SRC`.
pub const SADB_EXT_IDENTITY_SRC: u16 = 10;
/// `SADB_EXT_IDENTITY_DST`.
pub const SADB_EXT_IDENTITY_DST: u16 = 11;
/// `SADB_EXT_SENSITIVITY`.
pub const SADB_EXT_SENSITIVITY: u16 = 12;
/// `SADB_EXT_PROPOSAL`.
pub const SADB_EXT_PROPOSAL: u16 = 13;
/// `SADB_EXT_SUPPORTED_AUTH`.
pub const SADB_EXT_SUPPORTED_AUTH: u16 = 14;
/// `SADB_EXT_SUPPORTED_ENCRYPT`.
pub const SADB_EXT_SUPPORTED_ENCRYPT: u16 = 15;
/// `SADB_EXT_SPIRANGE`.
pub const SADB_EXT_SPIRANGE: u16 = 16;
/// `SADB_X_EXT_SRC_MASK`.
pub const SADB_X_EXT_SRC_MASK: u16 = 17;
/// `SADB_X_EXT_DST_MASK`.
pub const SADB_X_EXT_DST_MASK: u16 = 18;
/// `SADB_X_EXT_PROTOCOL`.
pub const SADB_X_EXT_PROTOCOL: u16 = 19;
/// `SADB_X_EXT_FLOW_TYPE`.
pub const SADB_X_EXT_FLOW_TYPE: u16 = 20;
/// `SADB_X_EXT_SRC_FLOW`.
pub const SADB_X_EXT_SRC_FLOW: u16 = 21;
/// `SADB_X_EXT_DST_FLOW`.
pub const SADB_X_EXT_DST_FLOW: u16 = 22;
/// `SADB_X_EXT_SA2`.
pub const SADB_X_EXT_SA2: u16 = 23;
/// `SADB_X_EXT_DST2`.
pub const SADB_X_EXT_DST2: u16 = 24;
/// `SADB_X_EXT_POLICY`.
pub const SADB_X_EXT_POLICY: u16 = 25;
/// `SADB_X_EXT_LOCAL_CREDENTIALS`.
pub const SADB_X_EXT_LOCAL_CREDENTIALS: u16 = 26;
/// `SADB_X_EXT_REMOTE_CREDENTIALS`.
pub const SADB_X_EXT_REMOTE_CREDENTIALS: u16 = 27;
/// `SADB_X_EXT_LOCAL_AUTH`.
pub const SADB_X_EXT_LOCAL_AUTH: u16 = 28;
/// `SADB_X_EXT_REMOTE_AUTH`.
pub const SADB_X_EXT_REMOTE_AUTH: u16 = 29;
/// `SADB_X_EXT_SUPPORTED_COMP`.
pub const SADB_X_EXT_SUPPORTED_COMP: u16 = 30;
/// `SADB_X_EXT_UDPENCAP`.
pub const SADB_X_EXT_UDPENCAP: u16 = 31;
/// `SADB_X_EXT_LIFETIME_LASTUSE`.
pub const SADB_X_EXT_LIFETIME_LASTUSE: u16 = 32;
/// `SADB_X_EXT_TAG`.
pub const SADB_X_EXT_TAG: u16 = 33;
/// `SADB_X_EXT_TAP`.
pub const SADB_X_EXT_TAP: u16 = 34;
/// `SADB_X_EXT_SATYPE2`.
pub const SADB_X_EXT_SATYPE2: u16 = 35;
/// `SADB_X_EXT_COUNTER`.
pub const SADB_X_EXT_COUNTER: u16 = 36;
/// `SADB_X_EXT_RDOMAIN`.
pub const SADB_X_EXT_RDOMAIN: u16 = 37;
/// `SADB_X_EXT_MTU`.
pub const SADB_X_EXT_MTU: u16 = 38;
/// `SADB_X_EXT_REPLAY`.
pub const SADB_X_EXT_REPLAY: u16 = 39;
/// `SADB_X_EXT_IFACE`.
pub const SADB_X_EXT_IFACE: u16 = 40;
/// `SADB_EXT_MAX`.
pub const SADB_EXT_MAX: usize = 40;

// Fix pfkeyv2.c struct pfkeyv2_socket if SATYPE_MAX > 31

/// `SADB_SATYPE_UNSPEC`.
pub const SADB_SATYPE_UNSPEC: u8 = 0;
/// `SADB_SATYPE_AH`.
pub const SADB_SATYPE_AH: u8 = 1;
/// `SADB_SATYPE_ESP`.
pub const SADB_SATYPE_ESP: u8 = 2;
/// `SADB_SATYPE_RSVP`.
pub const SADB_SATYPE_RSVP: u8 = 3;
/// `SADB_SATYPE_OSPFV2`.
pub const SADB_SATYPE_OSPFV2: u8 = 4;
/// `SADB_SATYPE_RIPV2`.
pub const SADB_SATYPE_RIPV2: u8 = 5;
/// `SADB_SATYPE_MIP`.
pub const SADB_SATYPE_MIP: u8 = 6;
/// `SADB_X_SATYPE_IPIP`.
pub const SADB_X_SATYPE_IPIP: u8 = 7;
/// `SADB_X_SATYPE_TCPSIGNATURE`.
pub const SADB_X_SATYPE_TCPSIGNATURE: u8 = 8;
/// `SADB_X_SATYPE_IPCOMP`.
pub const SADB_X_SATYPE_IPCOMP: u8 = 9;
/// `SADB_SATYPE_MAX`.
pub const SADB_SATYPE_MAX: u8 = 9;

/// `SADB_SASTATE_LARVAL`.
pub const SADB_SASTATE_LARVAL: u8 = 0;
/// `SADB_SASTATE_MATURE`.
pub const SADB_SASTATE_MATURE: u8 = 1;
/// `SADB_SASTATE_DYING`.
pub const SADB_SASTATE_DYING: u8 = 2;
/// `SADB_SASTATE_DEAD`.
pub const SADB_SASTATE_DEAD: u8 = 3;
/// `SADB_SASTATE_MAX`.
pub const SADB_SASTATE_MAX: u8 = 3;

/// `SADB_AALG_NONE`.
pub const SADB_AALG_NONE: u8 = 0;
/// `SADB_AALG_MD5HMAC`.
pub const SADB_AALG_MD5HMAC: u8 = 2;
/// `SADB_AALG_SHA1HMAC`.
pub const SADB_AALG_SHA1HMAC: u8 = 3;
/// `SADB_X_AALG_SHA2_256`.
pub const SADB_X_AALG_SHA2_256: u8 = 5;
/// `SADB_X_AALG_SHA2_384`.
pub const SADB_X_AALG_SHA2_384: u8 = 6;
/// `SADB_X_AALG_SHA2_512`.
pub const SADB_X_AALG_SHA2_512: u8 = 7;
/// `SADB_X_AALG_RIPEMD160HMAC`.
pub const SADB_X_AALG_RIPEMD160HMAC: u8 = 8;
/// `SADB_X_AALG_AES128GMAC`.
pub const SADB_X_AALG_AES128GMAC: u8 = 9;
/// `SADB_X_AALG_AES192GMAC`.
pub const SADB_X_AALG_AES192GMAC: u8 = 10;
/// `SADB_X_AALG_AES256GMAC`.
pub const SADB_X_AALG_AES256GMAC: u8 = 11;
/// `SADB_X_AALG_CHACHA20POLY1305`.
pub const SADB_X_AALG_CHACHA20POLY1305: u8 = 12;
/// `SADB_AALG_MAX`.
pub const SADB_AALG_MAX: u8 = 12;

/// `SADB_EALG_NONE`.
pub const SADB_EALG_NONE: u8 = 0;
/// `SADB_EALG_3DESCBC`.
pub const SADB_EALG_3DESCBC: u8 = 3;
/// `SADB_X_EALG_CAST`.
pub const SADB_X_EALG_CAST: u8 = 6;
/// `SADB_X_EALG_BLF`.
pub const SADB_X_EALG_BLF: u8 = 7;
/// `SADB_EALG_NULL`.
pub const SADB_EALG_NULL: u8 = 11;
/// `SADB_X_EALG_AES`.
pub const SADB_X_EALG_AES: u8 = 12;
/// `SADB_X_EALG_AESCTR`.
pub const SADB_X_EALG_AESCTR: u8 = 13;
/// `SADB_X_EALG_AESGCM8`.
pub const SADB_X_EALG_AESGCM8: u8 = 18;
/// `SADB_X_EALG_AESGCM12`.
pub const SADB_X_EALG_AESGCM12: u8 = 19;
/// `SADB_X_EALG_AESGCM16`.
pub const SADB_X_EALG_AESGCM16: u8 = 20;
/// `SADB_X_EALG_AESGMAC`.
pub const SADB_X_EALG_AESGMAC: u8 = 21;
/// `SADB_X_EALG_CHACHA20POLY1305`.
pub const SADB_X_EALG_CHACHA20POLY1305: u8 = 22;
/// `SADB_EALG_MAX`.
pub const SADB_EALG_MAX: u8 = 22;

/// `SADB_X_CALG_NONE`.
pub const SADB_X_CALG_NONE: u8 = 0;
/// `SADB_X_CALG_OUI`.
pub const SADB_X_CALG_OUI: u8 = 1;
/// `SADB_X_CALG_DEFLATE`.
pub const SADB_X_CALG_DEFLATE: u8 = 2;
/// `SADB_X_CALG_MAX`.
pub const SADB_X_CALG_MAX: u8 = 2;

/// `SADB_SAFLAGS_PFS`: perfect forward secrecy.
pub const SADB_SAFLAGS_PFS: u32 = 0x001;
/// `SADB_X_SAFLAGS_TUNNEL`: force tunneling.
pub const SADB_X_SAFLAGS_TUNNEL: u32 = 0x004;
/// `SADB_X_SAFLAGS_CHAINDEL`: delete whole SA chain.
pub const SADB_X_SAFLAGS_CHAINDEL: u32 = 0x008;
/// `SADB_X_SAFLAGS_UDPENCAP`: ESP in UDP.
pub const SADB_X_SAFLAGS_UDPENCAP: u32 = 0x200;
/// `SADB_X_SAFLAGS_ESN`: Extended Sequence Number.
pub const SADB_X_SAFLAGS_ESN: u32 = 0x400;

/// `SADB_X_POLICYFLAGS_POLICY`: this is a static policy.
pub const SADB_X_POLICYFLAGS_POLICY: u8 = 0x0001;

/// `SADB_IDENTTYPE_RESERVED`.
pub const SADB_IDENTTYPE_RESERVED: u16 = 0;
/// `SADB_IDENTTYPE_PREFIX`.
pub const SADB_IDENTTYPE_PREFIX: u16 = 1;
/// `SADB_IDENTTYPE_FQDN`.
pub const SADB_IDENTTYPE_FQDN: u16 = 2;
/// `SADB_IDENTTYPE_USERFQDN`.
pub const SADB_IDENTTYPE_USERFQDN: u16 = 3;
/// `SADB_IDENTTYPE_ASN1_DN`.
pub const SADB_IDENTTYPE_ASN1_DN: u16 = 4;
/// `SADB_IDENTTYPE_MAX`.
pub const SADB_IDENTTYPE_MAX: u16 = 4;

/// `SADB_KEY_FLAGS_MAX`.
pub const SADB_KEY_FLAGS_MAX: u32 = 0;

/// `PFKEYV2_LIFETIME_HARD`.
pub const PFKEYV2_LIFETIME_HARD: i32 = 0;
/// `PFKEYV2_LIFETIME_SOFT`.
pub const PFKEYV2_LIFETIME_SOFT: i32 = 1;
/// `PFKEYV2_LIFETIME_CURRENT`.
pub const PFKEYV2_LIFETIME_CURRENT: i32 = 2;
/// `PFKEYV2_LIFETIME_LASTUSE`.
pub const PFKEYV2_LIFETIME_LASTUSE: i32 = 3;

/// `PFKEYV2_IDENTITY_SRC`.
pub const PFKEYV2_IDENTITY_SRC: i32 = 0;
/// `PFKEYV2_IDENTITY_DST`.
pub const PFKEYV2_IDENTITY_DST: i32 = 1;

/// `PFKEYV2_ENCRYPTION_KEY`.
pub const PFKEYV2_ENCRYPTION_KEY: i32 = 0;
/// `PFKEYV2_AUTHENTICATION_KEY`.
pub const PFKEYV2_AUTHENTICATION_KEY: i32 = 1;

/// `PFKEYV2_SOCKETFLAGS_REGISTERED`.
pub const PFKEYV2_SOCKETFLAGS_REGISTERED: i32 = 1;
/// `PFKEYV2_SOCKETFLAGS_PROMISC`.
pub const PFKEYV2_SOCKETFLAGS_PROMISC: i32 = 2;

/// `PFKEYV2_SENDMESSAGE_UNICAST`.
pub const PFKEYV2_SENDMESSAGE_UNICAST: i32 = 1;
/// `PFKEYV2_SENDMESSAGE_REGISTERED`.
pub const PFKEYV2_SENDMESSAGE_REGISTERED: i32 = 2;
/// `PFKEYV2_SENDMESSAGE_BROADCAST`.
pub const PFKEYV2_SENDMESSAGE_BROADCAST: i32 = 3;

/// `SADB_X_FLOW_TYPE_USE`.
pub const SADB_X_FLOW_TYPE_USE: u8 = 1;
/// `SADB_X_FLOW_TYPE_ACQUIRE`.
pub const SADB_X_FLOW_TYPE_ACQUIRE: u8 = 2;
/// `SADB_X_FLOW_TYPE_REQUIRE`.
pub const SADB_X_FLOW_TYPE_REQUIRE: u8 = 3;
/// `SADB_X_FLOW_TYPE_BYPASS`.
pub const SADB_X_FLOW_TYPE_BYPASS: u8 = 4;
/// `SADB_X_FLOW_TYPE_DENY`.
pub const SADB_X_FLOW_TYPE_DENY: u8 = 5;
/// `SADB_X_FLOW_TYPE_DONTACQ`.
pub const SADB_X_FLOW_TYPE_DONTACQ: u8 = 6;

/// `void *headers[SADB_EXT_MAX + 1]`: the message header (index 0) and each extension, by
/// type; null when absent.
pub type SadbHeaders = [*mut u8; SADB_EXT_MAX + 1];

/// An empty `headers[]` (`bzero(headers, sizeof(headers))`).
pub const fn sadb_headers_new() -> SadbHeaders {
    [ptr::null_mut(); SADB_EXT_MAX + 1]
}

/// `PADUP(x)`: `x` rounded up to a multiple of 8.
pub const fn padup(x: usize) -> usize {
    (x + size_of::<u64>() - 1) & !(size_of::<u64>() - 1)
}

/// Reads a `T` at `p` in a message buffer.
///
/// # Safety
///
/// `p` points at `size_of::<T>()` readable bytes, and `T` is a `#[repr(C)]` structure of
/// integers (valid for any bytes).
pub unsafe fn sadb_get<T: Copy>(p: *const u8) -> T {
    // SAFETY: the caller's contract.
    unsafe { ptr::read_unaligned(p.cast::<T>()) }
}

/// Writes `v` at `p` in a message buffer.
///
/// # Safety
///
/// `p` points at `size_of::<T>()` writable bytes, and `T` has no padding.
pub unsafe fn sadb_put<T>(p: *mut u8, v: T) {
    // SAFETY: the caller's contract.
    unsafe { ptr::write_unaligned(p.cast::<T>(), v) };
}

/// `EXTLEN(x)`: the length in bytes of the extension at `x`.
///
/// # Safety
///
/// `x` points at an extension header.
pub unsafe fn extlen(x: *const u8) -> usize {
    // SAFETY: the caller's contract.
    usize::from(unsafe { sadb_get::<SadbExt>(x) }.sadb_ext_len) * size_of::<u64>()
}

/// The extension `headers[i]` as a `T`, `None` when absent.
///
/// # Safety
///
/// A non-null `headers[i]` points at an extension of at least `size_of::<T>()` bytes.
pub unsafe fn sadb_ext<T: Copy>(headers: &SadbHeaders, i: u16) -> Option<T> {
    let p = headers[usize::from(i)];
    // SAFETY: the caller's contract.
    (!p.is_null()).then(|| unsafe { sadb_get::<T>(p) })
}

/// The socket address after the `struct sadb_address` at `p`, as a union (its `sa_len`
/// bytes, at most the union's and the extension's).
///
/// # Safety
///
/// `p` points at a whole address extension.
pub unsafe fn sadb_address_sunion(p: *const u8) -> SockaddrUnion {
    let mut su = SockaddrUnion::new();
    // SAFETY: the caller's contract: an extension of `extlen` bytes.
    unsafe {
        let avail = extlen(p).saturating_sub(size_of::<SadbAddress>());
        let sa = p.add(size_of::<SadbAddress>());
        let len = if avail > 0 { usize::from(*sa) } else { 0 };
        let n = len.min(avail).min(su.as_bytes().len());
        ptr::copy_nonoverlapping(sa, su.as_bytes_mut().as_mut_ptr(), n);
    }
    su
}

/// A zeroed message buffer of `n` bytes (`malloc(n, M_PFKEY, M_NOWAIT | M_ZERO)`).
pub fn pfkey_alloc(n: usize) -> Result<Vec<u8>, Errno> {
    let mut v = Vec::new();
    v.try_reserve_exact(n).map_err(|_| Errno::ENOMEM)?;
    v.resize(n, 0);
    Ok(v)
}

/// Wipes and frees a message buffer (`explicit_bzero` then `free`).
pub fn pfkey_free(mut v: Vec<u8>) {
    explicit_bzero(&mut v);
    drop(v);
}

/// `PFKEYSNDQ`.
const PFKEYSNDQ: u64 = 8192;
/// `PFKEYRCVQ`.
const PFKEYRCVQ: u64 = 8192;

/// `SadbAlg` constructor for the static tables.
const fn alg(id: u8, ivlen: u8, minbits: u16, maxbits: u16) -> SadbAlg {
    SadbAlg {
        sadb_alg_id: id,
        sadb_alg_ivlen: ivlen,
        sadb_alg_minbits: minbits,
        sadb_alg_maxbits: maxbits,
        sadb_alg_reserved: 0,
    }
}

/// `ealgs[]`: the ciphers `SADB_REGISTER` advertises.
static EALGS: [SadbAlg; 9] = [
    alg(SADB_EALG_NULL, 0, 0, 0),
    alg(SADB_EALG_3DESCBC, 64, 192, 192),
    alg(SADB_X_EALG_BLF, 64, 40, (BLF_MAXKEYLEN * 8) as u16),
    alg(SADB_X_EALG_CAST, 64, 40, 128),
    alg(SADB_X_EALG_AES, 128, 128, 256),
    alg(SADB_X_EALG_AESCTR, 128, 128 + 32, 256 + 32),
    alg(SADB_X_EALG_AESGCM16, 64, 128 + 32, 256 + 32),
    alg(SADB_X_EALG_AESGMAC, 64, 128 + 32, 256 + 32),
    alg(SADB_X_EALG_CHACHA20POLY1305, 64, 256 + 32, 256 + 32),
];

/// `aalgs[]`: the authenticators.
static AALGS: [SadbAlg; 6] = [
    alg(SADB_AALG_SHA1HMAC, 0, 160, 160),
    alg(SADB_AALG_MD5HMAC, 0, 128, 128),
    alg(SADB_X_AALG_RIPEMD160HMAC, 0, 160, 160),
    alg(SADB_X_AALG_SHA2_256, 0, 256, 256),
    alg(SADB_X_AALG_SHA2_384, 0, 384, 384),
    alg(SADB_X_AALG_SHA2_512, 0, 512, 512),
];

/// `calgs[]`: the compressors.
static CALGS: [SadbAlg; 1] = [alg(SADB_X_CALG_DEFLATE, 0, 0, 0)];

/// `pkpcb_pool`.
static PKPCB_POOL: Pool = Pool::new();
/// `PFKEY_MSG_MAXSZ`.
const PFKEY_MSG_MAXSZ: i32 = 4096;
/// `pfkey_addr`: `{ 2, PF_KEY, }`.
static PFKEY_ADDR: [u8; 2] = [2, PF_KEY];

/// `struct pkpcb`: the pfkey PCB.
///
/// Locks used to protect struct members: I immutable after creation; l `pkptable`'s lock;
/// s socket lock.
pub struct Pkpcb {
    /// \[I\] `kcb_socket`: associated socket.
    pub kcb_socket: &'static Socket,
    /// \[l\] `kcb_list`.
    pub kcb_list: TailqEntry<Pkpcb>,
    /// \[s\] `kcb_flags`.
    pub kcb_flags: AtomicI32,
    /// \[s\] `kcb_reg`: inc if `SATYPE_MAX > 31`.
    pub kcb_reg: AtomicU32,
    /// \[I\] `kcb_pid`.
    pub kcb_pid: Pid,
    /// \[I\] `kcb_rdomain`: routing domain.
    pub kcb_rdomain: u32,
}

// SAFETY: the members change under the locks their docs name, or are atomic.
unsafe impl Sync for Pkpcb {}

queue_adapter!(
    /// `TAILQ_HEAD(, pkpcb)` through `kcb_list`.
    pub PkpcbList: Pkpcb, kcb_list => TailqEntry<Pkpcb>
);

/// `struct pkptable`.
pub struct Pkptable {
    /// `pkp_list`.
    pub pkp_list: TailqHead<PkpcbList>,
    /// `pkp_lk`.
    pub pkp_lk: Rwlock,
}

// SAFETY: the list changes under `pkp_lk`.
unsafe impl Sync for Pkptable {}

/// `pkptable`.
static PKPTABLE: Pkptable = Pkptable {
    pkp_list: TailqHead::new(),
    pkp_lk: Rwlock::new("pfkey"),
};

/// `pfkeyv2_mtx`.
static PFKEYV2_MTX: Mutex = Mutex::new(IPL_MPFLOOR);
/// `pfkeyv2_seq`.
static PFKEYV2_SEQ: AtomicU32 = AtomicU32::new(1);
/// `nregistered`.
static NREGISTERED: AtomicI32 = AtomicI32::new(0);
/// `npromisc`.
static NPROMISC: AtomicI32 = AtomicI32::new(0);

/// `sotokeycb(so)`: the PCB of a pfkey socket, `None` once detached.
fn sotokeycb(so: &Socket) -> Option<&'static Pkpcb> {
    // SAFETY: a pfkey socket's `so_pcb` is NULL or the `pkpcb_pool` item `pfkeyv2_attach`
    // wrote, which stays allocated until `pfkeyv2_detach` clears the pointer.
    unsafe { so.so_pcb.get().cast::<Pkpcb>().as_ref() }
}

/// `keylock(kp)`.
fn keylock(kp: &Pkpcb) {
    solock(kp.kcb_socket);
}

/// `keyunlock(kp)`.
fn keyunlock(kp: &Pkpcb) {
    sounlock(kp.kcb_socket);
}

/// `curproc`, which the requests and the sysctl run as.
fn curproc_or_panic(func: &str) -> &'static Proc {
    match curproc() {
        Some(p) => p,
        None => panic(format_args!("{}: no curproc", func)),
    }
}

/// `pfdatatopacket`: wrapper around `m_devget()`; copy data from contiguous buffer to mbuf
/// chain.
pub fn pfdatatopacket(data: &[u8]) -> Result<&'static Mbuf, Errno> {
    let packet = m_devget(data, 0).ok_or(Errno::ENOMEM)?;

    // Make sure, all data gets zeroized on free
    packet.m_flags().set(packet.m_flags().get() | M_ZEROIZE);

    Ok(packet)
}

/// `pfkeyv2_usrreqs`.
pub static PFKEYV2_USRREQS: PrUsrreqs = PrUsrreqs {
    pru_attach: Some(pfkeyv2_attach),
    pru_detach: Some(pfkeyv2_detach),
    pru_disconnect: Some(pfkeyv2_disconnect),
    pru_shutdown: Some(pfkeyv2_shutdown),
    pru_send: Some(pfkeyv2_send),
    pru_sockaddr: Some(pfkeyv2_sockaddr),
    pru_peeraddr: Some(pfkeyv2_peeraddr),
    ..PrUsrreqs::NONE
};

/// `pfkeysw[]`.
pub static PFKEYSW: [Protosw; 1] = [Protosw {
    pr_type: SOCK_RAW as i16,
    pr_protocol: PF_KEY_V2 as i16,
    pr_flags: PR_ATOMIC | PR_ADDR,
    pr_usrreqs: Some(&PFKEYV2_USRREQS),
    pr_sysctl: Some(pfkeyv2_sysctl),
    ..Protosw::new(&PFKEYDOMAIN)
}];

/// `pfkeydomain`.
pub static PFKEYDOMAIN: Domain = Domain {
    dom_family: PF_KEY as i32,
    dom_name: b"pfkey",
    dom_init: Some(pfkey_init),
    dom_externalize: None,
    dom_dispose: None,
    dom_protosw: &PFKEYSW,
    dom_sasize: 0,
    dom_rtoffset: 0,
    dom_maxplen: 0,
};

/// `pfkey_init`: the radix trees for `struct sockaddr_encap` keys and the pools.
pub fn pfkey_init() {
    rn_init(SockaddrEncap::new().as_bytes().len() as u32);
    PKPTABLE.pkp_list.init();
    pool_init(
        &PKPCB_POOL,
        size_of::<Pkpcb>(),
        0,
        IPL_SOFTNET,
        PR_WAITOK,
        "pkpcb",
        None,
    );
    pool_init(
        &IPSEC_POLICY_POOL,
        size_of::<IpsecPolicy>(),
        0,
        IPL_SOFTNET,
        0,
        "ipsec policy",
        None,
    );
    pool_init(
        &IPSEC_ACQUIRE_POOL,
        size_of::<IpsecAcquire>(),
        0,
        IPL_SOFTNET,
        0,
        "ipsec acquire",
        None,
    );
}

/// `pfkeyv2_attach`: attach a new PF_KEYv2 socket.
pub fn pfkeyv2_attach(so: &'static Socket, _proto: i32, wait: i32) -> Result<(), Errno> {
    if !so.has_state(SS_PRIV) {
        return Err(Errno::EACCES);
    }

    soreserve(so, PFKEYSNDQ, PFKEYRCVQ)?;

    let Some(mem) = pool_get(
        &PKPCB_POOL,
        (if wait == M_WAIT { PR_WAITOK } else { PR_NOWAIT }) | PR_ZERO,
    ) else {
        return Err(Errno::ENOBUFS);
    };
    let p = curproc_or_panic("pfkeyv2_attach");
    let raw = mem.cast::<Pkpcb>().as_ptr();
    // SAFETY: a fresh, suitably aligned `pkpcb_pool` item, written once before anything else
    // sees it; it stays allocated until `pfkeyv2_detach` gives it back.
    unsafe {
        raw.write(Pkpcb {
            kcb_socket: so,
            kcb_list: TailqEntry::new(),
            kcb_flags: AtomicI32::new(0),
            kcb_reg: AtomicU32::new(0),
            kcb_pid: p.process().ps_pid.get(),
            kcb_rdomain: rtable_l2(p.process().ps_rtableid.load(Ordering::Relaxed)),
        })
    };
    // SAFETY: as above.
    let kp: &'static Pkpcb = unsafe { &*raw };
    so.so_pcb.set(raw.cast());

    so.so_options.set(so.so_options.get() | SO_USELOOPBACK);
    soisconnected(so);

    rw_enter_write(&PKPTABLE.pkp_lk);
    // SAFETY: `pkp_lk` is held; the PCB is new and stays in place until `pfkeyv2_detach`.
    unsafe { PKPTABLE.pkp_list.insert_tail(kp) };
    rw_exit_write(&PKPTABLE.pkp_lk);

    Ok(())
}

/// `pfkeyv2_detach`: close a PF_KEYv2 socket.
pub fn pfkeyv2_detach(so: &'static Socket) -> Result<(), Errno> {
    soassertlocked(so);

    let Some(kp) = sotokeycb(so) else {
        return Err(Errno::ENOTCONN);
    };

    let flags = kp.kcb_flags.load(Ordering::Relaxed);
    if flags & (PFKEYV2_SOCKETFLAGS_REGISTERED | PFKEYV2_SOCKETFLAGS_PROMISC) != 0 {
        mtx_enter(&PFKEYV2_MTX);
        if flags & PFKEYV2_SOCKETFLAGS_REGISTERED != 0 {
            NREGISTERED.fetch_sub(1, Ordering::Relaxed);
        }

        if flags & PFKEYV2_SOCKETFLAGS_PROMISC != 0 {
            NPROMISC.fetch_sub(1, Ordering::Relaxed);
        }
        mtx_leave(&PFKEYV2_MTX);
    }

    rw_enter_write(&PKPTABLE.pkp_lk);
    // SAFETY: `pkp_lk` is held; `pfkeyv2_attach` linked the PCB.
    unsafe { PKPTABLE.pkp_list.remove(kp) };
    rw_exit_write(&PKPTABLE.pkp_lk);

    so.so_pcb.set(ptr::null_mut());
    kassert!(!so.has_state(SS_NOFDREF));
    pool_put(&PKPCB_POOL, NonNull::from(kp).cast());

    Ok(())
}

/// `pfkeyv2_disconnect`.
pub fn pfkeyv2_disconnect(so: &'static Socket) -> Result<(), Errno> {
    soisdisconnected(so);
    Ok(())
}

/// `pfkeyv2_shutdown`.
pub fn pfkeyv2_shutdown(so: &'static Socket) -> Result<(), Errno> {
    socantsendmore(so);
    Ok(())
}

/// `pfkeyv2_send`: a message from userland.
pub fn pfkeyv2_send(
    so: &'static Socket,
    m: Option<&'static Mbuf>,
    nam: Option<&'static Mbuf>,
    control: Option<&'static Mbuf>,
) -> Result<(), Errno> {
    soassertlocked(so);

    let error = 'out: {
        if control.is_some_and(|c| c.m_len().get() != 0) {
            break 'out Err(Errno::EOPNOTSUPP);
        }

        if nam.is_some() {
            break 'out Err(Errno::EISCONN);
        }

        let Some(m) = m else {
            break 'out Err(Errno::EINVAL);
        };
        m_freem(control);
        return pfkeyv2_output(m, so);
    };

    // out:
    m_freem(control);
    m_freem(m);

    error
}

/// `pfkeyv2_sockaddr`.
pub fn pfkeyv2_sockaddr(_so: &'static Socket, _nam: &'static Mbuf) -> Result<(), Errno> {
    Err(Errno::EINVAL)
}

/// `pfkeyv2_peeraddr`: minimal support, just implement a fake peer address.
pub fn pfkeyv2_peeraddr(_so: &'static Socket, nam: &'static Mbuf) -> Result<(), Errno> {
    // SAFETY: `nam` is an address mbuf of `MLEN` bytes, more than the two of `pfkey_addr`.
    unsafe { ptr::copy_nonoverlapping(PFKEY_ADDR.as_ptr(), mtod::<u8>(nam), PFKEY_ADDR.len()) };
    nam.m_len().set(PFKEY_ADDR.len() as u32);
    Ok(())
}

/// `pfkeyv2_output`: copies the message out of the packet and handles it without the socket
/// lock.
fn pfkeyv2_output(mbuf: &'static Mbuf, so: &'static Socket) -> Result<(), Errno> {
    let error = 'ret: {
        #[cfg(feature = "diagnostic")]
        if mbuf.m_flags().get() & M_PKTHDR == 0 {
            break 'ret Err(Errno::EINVAL);
        }
        let _ = M_PKTHDR;

        let len = mbuf.m_pkthdr().len.get();
        if len > PFKEY_MSG_MAXSZ {
            break 'ret Err(Errno::EMSGSIZE);
        }

        let mut message = match pfkey_alloc(len as usize) {
            Ok(v) => v,
            Err(e) => break 'ret Err(e),
        };

        m_copydata(mbuf, 0, &mut message);

        // The socket can't be closed concurrently because the file descriptor reference is
        // still held.

        sounlock(so);
        let error = pfkeyv2_dosend(so, message);
        solock(so);
        error
    };

    // ret:
    m_freem(mbuf);
    error
}

/// `pfkey_sendup`: queues `m0` (a copy of it with `more`) on the PCB's socket.
fn pfkey_sendup(kp: &Pkpcb, m0: &'static Mbuf, more: bool) -> Result<(), Errno> {
    let so = kp.kcb_socket;

    let m = if more {
        m_dup_pkt(m0, 0, crate::sys::mbuf::M_DONTWAIT).ok_or(Errno::ENOMEM)?
    } else {
        m0
    };

    mtx_enter(&so.so_rcv.sb_mtx);
    let ret = sbappendaddr(&so.so_rcv, &PFKEY_ADDR, Some(m), None);
    mtx_leave(&so.so_rcv.sb_mtx);

    if !ret {
        m_freem(m);
        return Err(Errno::ENOBUFS);
    }

    sorwakeup(so);
    Ok(())
}

/// Writes a PF_KEY message header at the start of `buf` (an encapsulating `SADB_X_PROMISC`
/// header around a message of `len` bytes after it).
fn promisc_header(buf: &mut [u8], len: usize, seq: u32) {
    buf[..size_of::<SadbMsg>()].fill(0);
    let smsg = SadbMsg {
        sadb_msg_version: PF_KEY_V2,
        sadb_msg_type: SADB_X_PROMISC,
        sadb_msg_len: ((size_of::<SadbMsg>() + len) / size_of::<u64>()) as u16,
        sadb_msg_seq: seq,
        ..SadbMsg::default()
    };
    // SAFETY: `buf` holds at least a message header (the callers allocate it with one).
    unsafe { sadb_put(buf.as_mut_ptr(), smsg) };
}

/// `pfkeyv2_sendmessage`: send a PFKEYv2 message, possibly to many receivers, based on the
/// satype of the socket (which is set by the REGISTER message), and the third argument.
///
/// # Safety
///
/// `headers[0]` points at a message header and every other non-null header at a whole
/// extension (`EXTLEN` bytes); the extensions' type fields are written.
pub unsafe fn pfkeyv2_sendmessage(
    headers: &SadbHeaders,
    mode: i32,
    so: Option<&'static Socket>,
    satype: u8,
    _count: i32,
    rdomain: u32,
) -> Result<(), Errno> {
    let msz = size_of::<SadbMsg>();

    // Find out how much space we'll need...
    let mut j = msz;

    for &h in &headers[1..] {
        if !h.is_null() {
            // SAFETY: the caller's contract.
            j += unsafe { extlen(h) };
        }
    }

    // ...and allocate it
    let mut buffer = pfkey_alloc(j + msz)?;

    let rval = 'ret: {
        // SAFETY: the caller's contract: `headers[0]` is a message header.
        let mut smsg: SadbMsg = unsafe { sadb_get(headers[0]) };
        smsg.sadb_msg_len = (j / size_of::<u64>()) as u16;
        // SAFETY: the buffer holds two headers and the extensions.
        unsafe { sadb_put(buffer.as_mut_ptr().add(msz), smsg) };
        let mut p = 2 * msz;

        // Copy payloads in the packet
        for (i, &h) in headers.iter().enumerate().skip(1) {
            if !h.is_null() {
                // SAFETY: the caller's contract: a whole extension, its type field writable.
                unsafe {
                    let mut e: SadbExt = sadb_get(h);
                    e.sadb_ext_type = i as u16;
                    sadb_put(h, e);
                    let n = extlen(h);
                    ptr::copy_nonoverlapping(h, buffer.as_mut_ptr().add(p), n);
                    p += n;
                }
            }
        }

        let packet = match pfdatatopacket(&buffer[msz..msz + j]) {
            Ok(p) => p,
            Err(e) => break 'ret Err(e),
        };

        match mode {
            PFKEYV2_SENDMESSAGE_UNICAST => {
                // Send message to the specified socket, plus all promiscuous listeners.
                if let Some(kp) = so.and_then(sotokeycb) {
                    let _ = pfkey_sendup(kp, packet, false);
                } else {
                    m_freem(packet);
                }

                // Promiscuous messages contain the original message encapsulated in another
                // sadb_msg header.
                promisc_header(&mut buffer, j, 0);

                // Copy to mbuf chain
                let packet = match pfdatatopacket(&buffer[..msz + j]) {
                    Ok(p) => p,
                    Err(e) => break 'ret Err(e),
                };

                // Search for promiscuous listeners, skipping the original destination.
                rw_enter_read(&PKPTABLE.pkp_lk);
                for kp in PKPTABLE.pkp_list.iter() {
                    if so.is_some_and(|s| ptr::eq(kp.kcb_socket, s)) || kp.kcb_rdomain != rdomain {
                        continue;
                    }

                    if kp.kcb_flags.load(Ordering::Relaxed) & PFKEYV2_SOCKETFLAGS_PROMISC != 0 {
                        let _ = pfkey_sendup(kp, packet, true);
                    }
                }
                rw_exit_read(&PKPTABLE.pkp_lk);
                m_freem(packet);
            }

            PFKEYV2_SENDMESSAGE_REGISTERED => {
                // Send the message to all registered sockets that match the specified satype
                // (e.g., all IPSEC-ESP negotiators)
                rw_enter_read(&PKPTABLE.pkp_lk);
                for kp in PKPTABLE.pkp_list.iter() {
                    if kp.kcb_rdomain != rdomain {
                        continue;
                    }

                    if kp.kcb_flags.load(Ordering::Relaxed) & PFKEYV2_SOCKETFLAGS_REGISTERED != 0 {
                        if satype == 0 {
                            // Just send to everyone registered
                            let _ = pfkey_sendup(kp, packet, true);
                        } else {
                            let kcb_reg = kp.kcb_reg.load(Ordering::Relaxed);
                            // Check for specified satype
                            if (1u32 << satype) & kcb_reg != 0 {
                                let _ = pfkey_sendup(kp, packet, true);
                            }
                        }
                    }
                }
                rw_exit_read(&PKPTABLE.pkp_lk);
                // Free last/original copy of the packet
                m_freem(packet);

                // Encapsulate the original message "inside" an sadb_msg header
                promisc_header(&mut buffer, j, 0);

                // Convert to mbuf chain
                let packet = match pfdatatopacket(&buffer[..msz + j]) {
                    Ok(p) => p,
                    Err(e) => break 'ret Err(e),
                };

                // Send to all registered promiscuous listeners
                rw_enter_read(&PKPTABLE.pkp_lk);
                for kp in PKPTABLE.pkp_list.iter() {
                    let flags = kp.kcb_flags.load(Ordering::Relaxed);

                    if kp.kcb_rdomain != rdomain {
                        continue;
                    }

                    if flags & PFKEYV2_SOCKETFLAGS_PROMISC != 0
                        && flags & PFKEYV2_SOCKETFLAGS_REGISTERED == 0
                    {
                        let _ = pfkey_sendup(kp, packet, true);
                    }
                }
                rw_exit_read(&PKPTABLE.pkp_lk);
                m_freem(packet);
            }

            PFKEYV2_SENDMESSAGE_BROADCAST => {
                // Send message to all sockets
                rw_enter_read(&PKPTABLE.pkp_lk);
                for kp in PKPTABLE.pkp_list.iter() {
                    if kp.kcb_rdomain != rdomain {
                        continue;
                    }

                    let _ = pfkey_sendup(kp, packet, true);
                }
                rw_exit_read(&PKPTABLE.pkp_lk);
                m_freem(packet);
            }
            _ => {
                m_freem(packet);
            }
        }
        Ok(())
    };

    // ret:
    pfkey_free(buffer);

    rval
}

/// A `sockaddr_in` union of `addr`/`port` (the cases of `pfkeyv2_policy`).
fn sunion_in(addr: crate::netinet::in_::InAddr, port: u16) -> SockaddrUnion {
    SockaddrUnion::from_sin(&SockaddrIn {
        sin_len: size_of::<SockaddrIn>() as u8,
        sin_family: AF_INET,
        sin_port: port,
        sin_addr: addr,
        ..SockaddrIn::default()
    })
}

/// A `sockaddr_in6` union of `addr`/`port` (the `SENT_IP6` cases of `pfkeyv2_policy`).
#[cfg(feature = "inet6")]
fn sunion_in6(addr: In6Addr, port: u16) -> SockaddrUnion {
    SockaddrUnion::from_sin6(&SockaddrIn6 {
        sin6_len: size_of::<SockaddrIn6>() as u8,
        sin6_family: AF_INET6,
        sin6_port: port,
        sin6_addr: addr,
        ..SockaddrIn6::default()
    })
}

/// The union `pfkeyv2_policy` exports for one half of a flow: the source (`src`) or
/// destination address and port of `e`, as a `sockaddr_in6` for an `SENT_IP6` flow (`v6`).
fn policy_sunion(e: &SockaddrEncap, v6: bool, src: bool) -> SockaddrUnion {
    #[cfg(feature = "inet6")]
    if v6 {
        return if src {
            sunion_in6(e.sen_ip6_src(), e.sen_ip6_sport())
        } else {
            sunion_in6(e.sen_ip6_dst(), e.sen_ip6_dport())
        };
    }
    let _ = v6;
    if src {
        sunion_in(e.sen_ip_src(), e.sen_sport())
    } else {
        sunion_in(e.sen_ip_dst(), e.sen_dport())
    }
}

/// `pfkeyv2_policy`: get SPD information for an ACQUIRE. We setup the message such that the
/// SRC/DST payloads are relative to us (regardless of whether the SPD rule was for incoming
/// or outgoing packets). The extensions are written in the returned buffer.
///
/// # Safety
///
/// `headers` is the message's header array; the new headers point into the returned buffer,
/// which the caller keeps until the message is sent.
pub unsafe fn pfkeyv2_policy(
    ipa: &IpsecAcquire,
    headers: &mut SadbHeaders,
) -> Result<Vec<u8>, Errno> {
    // Find out how big a buffer we need
    let mut i = 4 * size_of::<SadbAddress>() + size_of::<SadbProtocol>();

    let (dir, v6) = match ipa.ipa_info.sen_type() {
        SENT_IP4 => {
            i += 4 * padup(size_of::<SockaddrIn>());
            (ipa.ipa_info.sen_direction(), false)
        }
        #[cfg(feature = "inet6")]
        SENT_IP6 => {
            i += 4 * padup(size_of::<SockaddrIn6>());
            (ipa.ipa_info.sen_ip6_direction(), true)
        }
        _ => return Err(Errno::EINVAL),
    };

    let mut buffer = pfkey_alloc(i)?;
    let mut p = buffer.as_mut_ptr();

    let (a, b) = if dir == IPSP_DIRECTION_OUT {
        (SADB_X_EXT_SRC_FLOW, SADB_X_EXT_DST_FLOW)
    } else {
        (SADB_X_EXT_DST_FLOW, SADB_X_EXT_SRC_FLOW)
    };
    let (am, bm) = if dir == IPSP_DIRECTION_OUT {
        (SADB_X_EXT_SRC_MASK, SADB_X_EXT_DST_MASK)
    } else {
        (SADB_X_EXT_DST_MASK, SADB_X_EXT_SRC_MASK)
    };

    // SAFETY: the buffer was sized for these four addresses and the flow type.
    unsafe {
        headers[usize::from(a)] = p;
        let su = policy_sunion(&ipa.ipa_info, v6, true);
        export_address(&mut p, su.as_sockaddr_ptr());

        headers[usize::from(am)] = p;
        let su = policy_sunion(&ipa.ipa_mask, v6, true);
        export_address(&mut p, su.as_sockaddr_ptr());

        headers[usize::from(b)] = p;
        let su = policy_sunion(&ipa.ipa_info, v6, false);
        export_address(&mut p, su.as_sockaddr_ptr());

        headers[usize::from(bm)] = p;
        let su = policy_sunion(&ipa.ipa_mask, v6, false);
        export_address(&mut p, su.as_sockaddr_ptr());

        headers[usize::from(SADB_X_EXT_FLOW_TYPE)] = p;
        let mut sp = SadbProtocol {
            sadb_protocol_len: (size_of::<SadbProtocol>() / size_of::<u64>()) as u16,
            ..SadbProtocol::default()
        };
        #[cfg(feature = "inet6")]
        if v6 {
            if ipa.ipa_mask.sen_ip6_proto() != 0 {
                sp.sadb_protocol_proto = ipa.ipa_info.sen_ip6_proto();
            }
            sp.sadb_protocol_direction = ipa.ipa_info.sen_ip6_direction();
        } else {
            if ipa.ipa_mask.sen_proto() != 0 {
                sp.sadb_protocol_proto = ipa.ipa_info.sen_proto();
            }
            sp.sadb_protocol_direction = ipa.ipa_info.sen_direction();
        }
        #[cfg(not(feature = "inet6"))]
        {
            if ipa.ipa_mask.sen_proto() != 0 {
                sp.sadb_protocol_proto = ipa.ipa_info.sen_proto();
            }
            sp.sadb_protocol_direction = ipa.ipa_info.sen_direction();
        }
        sadb_put(p, sp);
    }

    Ok(buffer)
}

/// `pfkeyv2_get`: get all the information contained in an SA to a PFKEYV2 message. With
/// `buffer` `None` only the length is computed (`*lenp`); otherwise the extensions are written
/// in a new buffer, `headers` point into it, and `*lenused` is what was written.
///
/// # Safety
///
/// `headers` is the message's header array; the new headers point into `*buffer`, which the
/// caller keeps until the message is sent.
pub unsafe fn pfkeyv2_get(
    tdb: &Tdb,
    headers: &mut SadbHeaders,
    buffer: Option<&mut Vec<u8>>,
    lenp: Option<&mut usize>,
    lenused: Option<&mut usize>,
) -> Result<(), Errno> {
    net_assert_locked("pfkeyv2_get");

    // Find how much space we need
    let mut i = size_of::<SadbSa>() + size_of::<SadbLifetime>() + size_of::<SadbXCounter>();

    if tdb.tdb_soft_allocations.get() != 0
        || tdb.tdb_soft_bytes.get() != 0
        || tdb.tdb_soft_timeout.get() != 0
        || tdb.tdb_soft_first_use.get() != 0
    {
        i += size_of::<SadbLifetime>();
    }

    if tdb.tdb_exp_allocations.get() != 0
        || tdb.tdb_exp_bytes.get() != 0
        || tdb.tdb_exp_timeout.get() != 0
        || tdb.tdb_exp_first_use.get() != 0
    {
        i += size_of::<SadbLifetime>();
    }

    if tdb.tdb_last_used.get() != 0 {
        i += size_of::<SadbLifetime>();
    }

    i += size_of::<SadbAddress>() + padup(usize::from(tdb.tdb_src.get().sa_len()));
    i += size_of::<SadbAddress>() + padup(usize::from(tdb.tdb_dst.get().sa_len()));

    if let Some(ids) = tdb.tdb_ids.get() {
        i += size_of::<SadbIdent>() + padup(ids.id_local().len as usize);
        i += size_of::<SadbIdent>() + padup(ids.id_remote().len as usize);
    }

    if !tdb.tdb_amxkey.get().is_null() {
        i += size_of::<SadbKey>() + padup(usize::from(tdb.tdb_amxkeylen.get()));
    }

    if !tdb.tdb_emxkey.get().is_null() {
        i += size_of::<SadbKey>() + padup(usize::from(tdb.tdb_emxkeylen.get()));
    }

    let filter = tdb.tdb_filter.get();
    if filter.sen_type() != 0 {
        i += 2 * size_of::<SadbProtocol>();

        // We'll need four of them: src, src mask, dst, dst mask.
        match filter.sen_type() {
            SENT_IP4 => {
                i += 4 * padup(size_of::<SockaddrIn>());
                i += 4 * size_of::<SadbAddress>();
            }
            #[cfg(feature = "inet6")]
            SENT_IP6 => {
                i += 4 * padup(size_of::<SockaddrIn6>());
                i += 4 * size_of::<SadbAddress>();
            }
            _ => return Err(Errno::EINVAL),
        }
    }

    if let Some(onext) = tdb.tdb_onext.get() {
        i += size_of::<SadbSa>();
        i += size_of::<SadbAddress>() + padup(usize::from(onext.tdb_dst.get().sa_len()));
        i += size_of::<SadbProtocol>();
    }

    if tdb.tdb_udpencap_port.get() != 0 {
        i += size_of::<SadbXUdpencap>();
    }

    i += size_of::<SadbXReplay>();

    if tdb.tdb_mtu.get() > 0 {
        i += size_of::<SadbXMtu>();
    }

    if tdb.tdb_rdomain.get() != tdb.tdb_rdomain_post.get() {
        i += size_of::<SadbXRdomain>();
    }

    if tdb.tdb_tag.get() != 0 {
        i += size_of::<SadbXTag>() + padup(PF_TAG_NAME_SIZE);
    }
    if tdb.tdb_tap.get() != 0 {
        i += size_of::<SadbXTap>();
    }

    if tdb.has_flags(TDBF_IFACE) {
        i += size_of::<SadbXIface>();
    }

    if let Some(lenp) = lenp {
        *lenp = i;
    }

    let Some(buffer) = buffer else {
        return Ok(());
    };

    *buffer = pfkey_alloc(i)?;
    let start = buffer.as_mut_ptr();
    let mut p = start;

    // SAFETY: the buffer was sized above for exactly these extensions.
    unsafe {
        headers[usize::from(SADB_EXT_SA)] = p;

        export_sa(&mut p, tdb); // Export SA information (mostly flags)

        // Export lifetimes where applicable
        headers[usize::from(SADB_EXT_LIFETIME_CURRENT)] = p;
        export_lifetime(&mut p, tdb, PFKEYV2_LIFETIME_CURRENT);

        if tdb.tdb_soft_allocations.get() != 0
            || tdb.tdb_soft_bytes.get() != 0
            || tdb.tdb_soft_first_use.get() != 0
            || tdb.tdb_soft_timeout.get() != 0
        {
            headers[usize::from(SADB_EXT_LIFETIME_SOFT)] = p;
            export_lifetime(&mut p, tdb, PFKEYV2_LIFETIME_SOFT);
        }

        if tdb.tdb_exp_allocations.get() != 0
            || tdb.tdb_exp_bytes.get() != 0
            || tdb.tdb_exp_first_use.get() != 0
            || tdb.tdb_exp_timeout.get() != 0
        {
            headers[usize::from(SADB_EXT_LIFETIME_HARD)] = p;
            export_lifetime(&mut p, tdb, PFKEYV2_LIFETIME_HARD);
        }

        if tdb.tdb_last_used.get() != 0 {
            headers[usize::from(SADB_X_EXT_LIFETIME_LASTUSE)] = p;
            export_lifetime(&mut p, tdb, PFKEYV2_LIFETIME_LASTUSE);
        }

        // Export TDB source address
        headers[usize::from(SADB_EXT_ADDRESS_SRC)] = p;
        export_address(&mut p, tdb.tdb_src.get().as_sockaddr_ptr());

        // Export TDB destination address
        headers[usize::from(SADB_EXT_ADDRESS_DST)] = p;
        export_address(&mut p, tdb.tdb_dst.get().as_sockaddr_ptr());

        // Export source/destination identities, if present
        if let Some(ids) = tdb.tdb_ids.get() {
            export_identities(&mut p, ids, tdb.tdb_ids_swapped.get() != 0, headers);
        }

        // Export authentication key, if present
        if !tdb.tdb_amxkey.get().is_null() {
            headers[usize::from(SADB_EXT_KEY_AUTH)] = p;
            export_key(&mut p, tdb, PFKEYV2_AUTHENTICATION_KEY);
        }

        // Export encryption key, if present
        if !tdb.tdb_emxkey.get().is_null() {
            headers[usize::from(SADB_EXT_KEY_ENCRYPT)] = p;
            export_key(&mut p, tdb, PFKEYV2_ENCRYPTION_KEY);
        }

        // Export flow/filter, if present
        if filter.sen_type() != 0 {
            export_flow(
                &mut p,
                IPSP_IPSEC_USE_FLOW,
                &filter,
                &tdb.tdb_filtermask.get(),
                headers,
            );
        }

        if let Some(onext) = tdb.tdb_onext.get() {
            headers[usize::from(SADB_X_EXT_SA2)] = p;
            export_sa(&mut p, onext);
            headers[usize::from(SADB_X_EXT_DST2)] = p;
            export_address(&mut p, onext.tdb_dst.get().as_sockaddr_ptr());
            headers[usize::from(SADB_X_EXT_SATYPE2)] = p;
            export_satype(&mut p, onext);
        }

        // Export UDP encapsulation port, if present
        if tdb.tdb_udpencap_port.get() != 0 {
            headers[usize::from(SADB_X_EXT_UDPENCAP)] = p;
            export_udpencap(&mut p, tdb);
        }

        headers[usize::from(SADB_X_EXT_REPLAY)] = p;
        export_replay(&mut p, tdb);

        if tdb.tdb_mtu.get() > 0 {
            headers[usize::from(SADB_X_EXT_MTU)] = p;
            export_mtu(&mut p, tdb);
        }

        // Export rdomain switch, if present
        if tdb.tdb_rdomain.get() != tdb.tdb_rdomain_post.get() {
            headers[usize::from(SADB_X_EXT_RDOMAIN)] = p;
            export_rdomain(&mut p, tdb);
        }

        // Export tag information, if present
        if tdb.tdb_tag.get() != 0 {
            headers[usize::from(SADB_X_EXT_TAG)] = p;
            export_tag(&mut p, tdb);
        }

        // Export tap enc(4) device information, if present
        if tdb.tdb_tap.get() != 0 {
            headers[usize::from(SADB_X_EXT_TAP)] = p;
            export_tap(&mut p, tdb);
        }

        // Export sec(4) interface information, if present
        if tdb.has_flags(TDBF_IFACE) {
            headers[usize::from(SADB_X_EXT_IFACE)] = p;
            export_iface(&mut p, tdb);
        }

        headers[usize::from(SADB_X_EXT_COUNTER)] = p;
        export_counter(&mut p, tdb);

        if let Some(lenused) = lenused {
            *lenused = p.offset_from(start) as usize;
        }
    }

    Ok(())
}

/// `pfkeyv2_dump_walker`: dump a TDB to `so` (`struct dump_state`: the request `smsg` and the
/// socket).
fn pfkeyv2_dump_walker(
    tdb: &Tdb,
    smsg_p: *mut u8,
    so: &'static Socket,
    last: bool,
) -> Result<(), Errno> {
    // SAFETY: `smsg_p` is the request's message header (`pfkeyv2_dosend`).
    let mut smsg: SadbMsg = unsafe { sadb_get(smsg_p) };

    // If not satype was specified, dump all TDBs
    if smsg.sadb_msg_satype == 0 || tdb.tdb_satype.get() == smsg.sadb_msg_satype {
        let mut headers = sadb_headers_new();
        headers[0] = smsg_p;

        let mut buffer = Vec::new();
        let mut buflen = 0;
        // Get the information from the TDB to a PFKEYv2 message
        // SAFETY: the headers point into the request and into `buffer`, kept until sent.
        unsafe {
            pfkeyv2_get(
                tdb,
                &mut headers,
                Some(&mut buffer),
                Some(&mut buflen),
                None,
            )
        }?;

        if last {
            smsg.sadb_msg_seq = 0;
            // SAFETY: as above.
            unsafe { sadb_put(smsg_p, smsg) };
        }

        // Send the message to the specified socket
        // SAFETY: every header points at a whole extension.
        let rval = unsafe {
            pfkeyv2_sendmessage(
                &headers,
                PFKEYV2_SENDMESSAGE_UNICAST,
                Some(so),
                0,
                0,
                tdb.tdb_rdomain.get(),
            )
        };

        pfkey_free(buffer);
        rval?;
    }

    Ok(())
}

/// `pfkeyv2_sa_flush`: delete an SA of `satype` (any for 0).
fn pfkeyv2_sa_flush(tdb: &Tdb, satype: u8, _last: bool) -> Result<(), Errno> {
    if satype == 0 || tdb.tdb_satype.get() == satype {
        tdb_delete(tdb);
    }
    Ok(())
}

/// `pfkeyv2_get_proto_alg`: convert between SATYPEs and IPsec protocols, taking into
/// consideration sysctl variables enabling/disabling ESP/AH and the presence of the old IPsec
/// transforms. Returns the protocol and, with `alg`, the transform.
pub fn pfkeyv2_get_proto_alg(satype: u8, alg: Option<&mut u16>) -> Result<u8, Errno> {
    let (sproto, xf) = match satype {
        SADB_SATYPE_AH => {
            if AH_ENABLE.load(Ordering::Relaxed) == 0 {
                return Err(Errno::EOPNOTSUPP);
            }
            (IPPROTO_AH, XF_AH)
        }
        SADB_SATYPE_ESP => {
            if ESP_ENABLE.load(Ordering::Relaxed) == 0 {
                return Err(Errno::EOPNOTSUPP);
            }
            (IPPROTO_ESP, XF_ESP)
        }
        SADB_X_SATYPE_IPIP => (IPPROTO_IPIP, XF_IP4),
        SADB_X_SATYPE_IPCOMP => {
            if IPCOMP_ENABLE.load(Ordering::Relaxed) == 0 {
                return Err(Errno::EOPNOTSUPP);
            }
            (IPPROTO_IPCOMP, XF_IPCOMP)
        }
        // TCP_SIGNATURE
        SADB_X_SATYPE_TCPSIGNATURE => (crate::netinet::in_::IPPROTO_TCP, XF_TCPSIGNATURE),
        _ => return Err(Errno::EOPNOTSUPP), // Nothing else supported
    };

    if let Some(alg) = alg {
        *alg = xf;
    }

    Ok(sproto as u8)
}

/// Whether all or none of the flow extensions are present ("either all or none of the flow
/// must be included").
fn flow_all_or_none(headers: &SadbHeaders) -> bool {
    let f = [
        SADB_X_EXT_SRC_FLOW,
        SADB_X_EXT_PROTOCOL,
        SADB_X_EXT_FLOW_TYPE,
        SADB_X_EXT_DST_FLOW,
        SADB_X_EXT_SRC_MASK,
        SADB_X_EXT_DST_MASK,
    ];
    let any = f.iter().any(|&i| !headers[usize::from(i)].is_null());
    let all = f.iter().all(|&i| !headers[usize::from(i)].is_null());
    !any || all
}

/// The import part of `SADB_ADD` and `SADB_UPDATE` of a reserved SA: a new TDB set up from the
/// message.
///
/// # Safety
///
/// `headers` is a parsed message (`pfkeyv2_parsemessage`).
unsafe fn pfkeyv2_newsa(
    headers: &mut SadbHeaders,
    smsg: &SadbMsg,
    rdomain: u32,
    swapped: bool,
) -> Result<&'static Tdb, Errno> {
    // Create new TDB
    let newsa = tdb_alloc(rdomain);
    newsa.tdb_satype.set(smsg.sadb_msg_satype);

    let mut alg = 0u16;
    match pfkeyv2_get_proto_alg(newsa.tdb_satype.get(), Some(&mut alg)) {
        Ok(sproto) => newsa.tdb_sproto.set(sproto),
        Err(e) => {
            tdb_unref(Some(newsa));
            return Err(e);
        }
    }

    // Initialize SA
    let mut ii = IpsecInit::default();
    // SAFETY: the caller's contract: the headers are whole, checked extensions.
    unsafe {
        import_sa(
            newsa,
            sadb_ext::<SadbSa>(headers, SADB_EXT_SA),
            Some(&mut ii),
        );
        let mut su = newsa.tdb_src.get();
        import_address(&mut su, headers[usize::from(SADB_EXT_ADDRESS_SRC)]);
        newsa.tdb_src.set(su);
        let mut su = newsa.tdb_dst.get();
        import_address(&mut su, headers[usize::from(SADB_EXT_ADDRESS_DST)]);
        newsa.tdb_dst.set(su);

        import_lifetime(
            newsa,
            sadb_ext::<SadbLifetime>(headers, SADB_EXT_LIFETIME_CURRENT),
            PFKEYV2_LIFETIME_CURRENT,
        );
        import_lifetime(
            newsa,
            sadb_ext::<SadbLifetime>(headers, SADB_EXT_LIFETIME_SOFT),
            PFKEYV2_LIFETIME_SOFT,
        );
        import_lifetime(
            newsa,
            sadb_ext::<SadbLifetime>(headers, SADB_EXT_LIFETIME_HARD),
            PFKEYV2_LIFETIME_HARD,
        );

        import_key(
            &mut ii,
            headers[usize::from(SADB_EXT_KEY_AUTH)],
            PFKEYV2_AUTHENTICATION_KEY,
        );
        import_key(
            &mut ii,
            headers[usize::from(SADB_EXT_KEY_ENCRYPT)],
            PFKEYV2_ENCRYPTION_KEY,
        );

        if swapped {
            newsa.tdb_ids_swapped.set(1); // only on TDB_UPDATE
        }
        newsa.tdb_ids.set(import_identities(
            newsa.tdb_ids_swapped.get() != 0,
            headers[usize::from(SADB_EXT_IDENTITY_SRC)],
            headers[usize::from(SADB_EXT_IDENTITY_DST)],
        ));

        let mut filter = newsa.tdb_filter.get();
        let mut filtermask = newsa.tdb_filtermask.get();
        let r = import_flow(
            &mut filter,
            &mut filtermask,
            headers[usize::from(SADB_X_EXT_SRC_FLOW)],
            headers[usize::from(SADB_X_EXT_SRC_MASK)],
            headers[usize::from(SADB_X_EXT_DST_FLOW)],
            headers[usize::from(SADB_X_EXT_DST_MASK)],
            sadb_ext::<SadbProtocol>(headers, SADB_X_EXT_PROTOCOL),
            sadb_ext::<SadbProtocol>(headers, SADB_X_EXT_FLOW_TYPE),
        );
        newsa.tdb_filter.set(filter);
        newsa.tdb_filtermask.set(filtermask);
        if let Err(e) = r {
            tdb_unref(Some(newsa));
            return Err(e);
        }
        import_udpencap(
            newsa,
            sadb_ext::<SadbXUdpencap>(headers, SADB_X_EXT_UDPENCAP),
        );
        import_rdomain(newsa, sadb_ext::<SadbXRdomain>(headers, SADB_X_EXT_RDOMAIN));
        import_tag(newsa, headers[usize::from(SADB_X_EXT_TAG)]);
        import_tap(newsa, sadb_ext::<SadbXTap>(headers, SADB_X_EXT_TAP));
        import_iface(newsa, sadb_ext::<SadbXIface>(headers, SADB_X_EXT_IFACE));
    }

    // Exclude sensitive data from reply message.
    headers[usize::from(SADB_EXT_KEY_AUTH)] = ptr::null_mut();
    headers[usize::from(SADB_EXT_KEY_ENCRYPT)] = ptr::null_mut();
    headers[usize::from(SADB_X_EXT_LOCAL_AUTH)] = ptr::null_mut();
    headers[usize::from(SADB_X_EXT_REMOTE_AUTH)] = ptr::null_mut();

    newsa.tdb_seq.set(smsg.sadb_msg_seq);

    if tdb_init(newsa, alg, &mut ii).is_err() {
        tdb_unref(Some(newsa));
        return Err(Errno::EINVAL);
    }

    Ok(newsa)
}

/// The buffers `pfkeyv2_dosend` keeps until the reply is sent (`freeme`, `freeme2`,
/// `freeme3`).
#[derive(Default)]
struct Freeme {
    /// `freeme`: wiped before it is freed.
    freeme: Option<Vec<u8>>,
    /// `freeme2`.
    freeme2: Option<Vec<u8>>,
    /// `freeme3`.
    freeme3: Option<Vec<u8>>,
}

/// A `struct sadb_supported` and the algorithm table after it (`SADB_REGISTER`).
fn supported(algs: &[SadbAlg]) -> Result<Vec<u8>, Errno> {
    let sz = size_of::<SadbSupported>() + size_of_val(algs);
    let mut v = pfkey_alloc(sz)?;
    let ssup = SadbSupported {
        sadb_supported_len: (sz / size_of::<u64>()) as u16,
        ..SadbSupported::default()
    };
    // SAFETY: the buffer holds the header and the table.
    unsafe {
        sadb_put(v.as_mut_ptr(), ssup);
        for (i, a) in algs.iter().enumerate() {
            sadb_put(
                v.as_mut_ptr()
                    .add(size_of::<SadbSupported>() + i * size_of::<SadbAlg>()),
                *a,
            );
        }
    }
    Ok(v)
}

/// `pfkeyv2_dosend`: handle all messages from userland to kernel. `message` is the copy of
/// the user's message; it is wiped and freed here.
pub fn pfkeyv2_dosend(so: &'static Socket, mut message: Vec<u8>) -> Result<(), Errno> {
    let len = message.len();
    let mut mode = PFKEYV2_SENDMESSAGE_BROADCAST;
    let mut headers = sadb_headers_new();
    let mut fm = Freeme::default();
    let mut sa2: Option<&'static Tdb> = None;
    let mut smsg_p: *mut u8 = ptr::null_mut();

    mtx_enter(&PFKEYV2_MTX);
    let promisc = NPROMISC.load(Ordering::Relaxed);
    mtx_leave(&PFKEYV2_MTX);

    // `ret:` decides between `realret` (no reply) and the reply.
    let (rval, reply) = 'ret: {
        // Verify that we received this over a legitimate pfkeyv2 socket
        let Some(kp) = sotokeycb(so) else {
            break 'ret (Err(Errno::EINVAL), None);
        };

        let mut rdomain = kp.kcb_rdomain;

        // Validate message format
        if let Err(e) = pfkeyv2_parsemessage(&mut message, &mut headers) {
            break 'ret (Err(e), Some(kp));
        }
        smsg_p = headers[0];

        // If we have any promiscuous listeners, send them a copy of the message
        if promisc != 0 {
            let freeme_sz = size_of::<SadbMsg>() + len;
            let mut freeme = match pfkey_alloc(freeme_sz) {
                Ok(v) => v,
                Err(e) => break 'ret (Err(e), Some(kp)),
            };

            // Initialize encapsulating header
            let pid = curproc_or_panic("pfkeyv2_dosend").process().ps_pid.get();
            promisc_header(&mut freeme, len, pid as u32);

            freeme[size_of::<SadbMsg>()..].copy_from_slice(&message);

            // Convert to mbuf chain
            let packet = match pfdatatopacket(&freeme) {
                Ok(p) => p,
                Err(e) => {
                    pfkey_free(freeme);
                    break 'ret (Err(e), Some(kp));
                }
            };

            // Send to all promiscuous listeners
            rw_enter_read(&PKPTABLE.pkp_lk);
            for bkp in PKPTABLE.pkp_list.iter() {
                if bkp.kcb_rdomain != kp.kcb_rdomain {
                    continue;
                }

                if bkp.kcb_flags.load(Ordering::Relaxed) & PFKEYV2_SOCKETFLAGS_PROMISC != 0 {
                    let _ = pfkey_sendup(bkp, packet, true);
                }
            }
            rw_exit_read(&PKPTABLE.pkp_lk);

            m_freem(packet);

            // Paranoid
            pfkey_free(freeme);
        }

        // use specified rdomain
        // SAFETY: the headers are whole, checked extensions.
        let srdomain = unsafe { sadb_ext::<SadbXRdomain>(&headers, SADB_X_EXT_RDOMAIN) };
        if let Some(sr) = srdomain {
            if !rtable_exists(u32::from(sr.sadb_x_rdomain_dom1))
                || !rtable_exists(u32::from(sr.sadb_x_rdomain_dom2))
            {
                break 'ret (Err(Errno::EINVAL), Some(kp));
            }
            rdomain = u32::from(sr.sadb_x_rdomain_dom1);
        }

        // SAFETY: `headers[0]` is the message header.
        let mut smsg: SadbMsg = unsafe { sadb_get(smsg_p) };
        // SAFETY: (for the whole match) the headers are whole extensions of the parsed
        // message, of the types `pfkeyv2_parsemessage` checked; the buffers the reply points
        // into are kept in `fm` until it is sent.
        let r: Result<(), Errno> = unsafe {
            match smsg.sadb_msg_type {
                SADB_GETSPI => 'c: {
                    // Reserve an SPI
                    let sa1 = Tdb::new();

                    sa1.tdb_satype.set(smsg.sadb_msg_satype);
                    match pfkeyv2_get_proto_alg(sa1.tdb_satype.get(), None) {
                        Ok(p) => sa1.tdb_sproto.set(p),
                        Err(e) => break 'c Err(e),
                    }

                    let mut su = SockaddrUnion::new();
                    import_address(&mut su, headers[usize::from(SADB_EXT_ADDRESS_SRC)]);
                    sa1.tdb_src.set(su);
                    let mut su = SockaddrUnion::new();
                    import_address(&mut su, headers[usize::from(SADB_EXT_ADDRESS_DST)]);
                    sa1.tdb_dst.set(su);

                    // Find an unused SA identifier
                    let Some(sprng) = sadb_ext::<SadbSpirange>(&headers, SADB_EXT_SPIRANGE) else {
                        break 'c Err(Errno::EINVAL);
                    };
                    net_lock();
                    match reserve_spi(
                        rdomain,
                        sprng.sadb_spirange_min,
                        sprng.sadb_spirange_max,
                        &sa1.tdb_src.get(),
                        &sa1.tdb_dst.get(),
                        sa1.tdb_sproto.get(),
                    ) {
                        Ok(spi) => sa1.tdb_spi.set(spi),
                        Err(e) => {
                            net_unlock();
                            break 'c Err(e);
                        }
                    }

                    // Send a message back telling what the SA (the SPI really) is
                    let mut freeme = match pfkey_alloc(size_of::<SadbSa>()) {
                        Ok(v) => v,
                        Err(e) => {
                            net_unlock();
                            break 'c Err(e);
                        }
                    };

                    headers[usize::from(SADB_EXT_SPIRANGE)] = ptr::null_mut();
                    headers[usize::from(SADB_EXT_SA)] = freeme.as_mut_ptr();
                    let mut bckptr = freeme.as_mut_ptr();

                    // We really only care about the SPI, but we'll export the SA
                    export_sa(&mut bckptr, &sa1);
                    fm.freeme = Some(freeme);
                    net_unlock();
                    Ok(())
                }

                SADB_UPDATE => 'c: {
                    let Some(ssa) = sadb_ext::<SadbSa>(&headers, SADB_EXT_SA) else {
                        break 'c Err(Errno::EINVAL);
                    };
                    let sunionp = sadb_address_sunion(headers[usize::from(SADB_EXT_ADDRESS_DST)]);

                    // Either all or none of the flow must be included
                    if !flow_all_or_none(&headers) {
                        break 'c Err(Errno::EINVAL);
                    }
                    // UDP encap has to be enabled and is only supported for ESP
                    if !headers[usize::from(SADB_X_EXT_UDPENCAP)].is_null()
                        && (UDPENCAP_ENABLE.load(Ordering::Relaxed) == 0
                            || smsg.sadb_msg_satype != SADB_SATYPE_ESP)
                    {
                        break 'c Err(Errno::EINVAL);
                    }

                    // Find TDB
                    net_lock();
                    sa2 = gettdb(
                        rdomain,
                        ssa.sadb_sa_spi,
                        &sunionp,
                        SADB_X_GETSPROTO(smsg.sadb_msg_satype),
                    );

                    // If there's no such SA, we're done
                    let Some(s2) = sa2 else {
                        net_unlock();
                        break 'c Err(Errno::ESRCH);
                    };

                    // If this is a reserved SA
                    if s2.has_flags(TDBF_INVALID) {
                        let newsa = match pfkeyv2_newsa(&mut headers, &smsg, rdomain, true) {
                            Ok(t) => t,
                            Err(e) => {
                                net_unlock();
                                break 'c Err(e);
                            }
                        };

                        newsa.tdb_cur_allocations.set(s2.tdb_cur_allocations.get());

                        // Delete old version of the SA, insert new one
                        tdb_delete(s2);

                        tdb_addtimeouts(newsa);

                        puttdb(newsa);
                    } else {
                        // The SA is already initialized, so we're only allowed to change
                        // lifetimes and some other information; we're not allowed to change
                        // keys, addresses or identities.
                        if !headers[usize::from(SADB_EXT_KEY_AUTH)].is_null()
                            || !headers[usize::from(SADB_EXT_KEY_ENCRYPT)].is_null()
                            || !headers[usize::from(SADB_EXT_IDENTITY_SRC)].is_null()
                            || !headers[usize::from(SADB_EXT_IDENTITY_DST)].is_null()
                            || !headers[usize::from(SADB_EXT_SENSITIVITY)].is_null()
                        {
                            net_unlock();
                            break 'c Err(Errno::EINVAL);
                        }

                        import_sa(s2, sadb_ext::<SadbSa>(&headers, SADB_EXT_SA), None);
                        import_lifetime(
                            s2,
                            sadb_ext::<SadbLifetime>(&headers, SADB_EXT_LIFETIME_CURRENT),
                            PFKEYV2_LIFETIME_CURRENT,
                        );
                        import_lifetime(
                            s2,
                            sadb_ext::<SadbLifetime>(&headers, SADB_EXT_LIFETIME_SOFT),
                            PFKEYV2_LIFETIME_SOFT,
                        );
                        import_lifetime(
                            s2,
                            sadb_ext::<SadbLifetime>(&headers, SADB_EXT_LIFETIME_HARD),
                            PFKEYV2_LIFETIME_HARD,
                        );
                        import_udpencap(
                            s2,
                            sadb_ext::<SadbXUdpencap>(&headers, SADB_X_EXT_UDPENCAP),
                        );
                        import_tag(s2, headers[usize::from(SADB_X_EXT_TAG)]);
                        import_tap(s2, sadb_ext::<SadbXTap>(&headers, SADB_X_EXT_TAP));
                        import_iface(s2, sadb_ext::<SadbXIface>(&headers, SADB_X_EXT_IFACE));

                        tdb_addtimeouts(s2);

                        if !headers[usize::from(SADB_EXT_ADDRESS_SRC)].is_null()
                            || !headers[usize::from(SADB_EXT_ADDRESS_PROXY)].is_null()
                        {
                            mtx_enter(&TDB_SADB_MTX);
                            tdb_unlink_locked(s2);
                            let mut su = s2.tdb_src.get();
                            import_address(&mut su, headers[usize::from(SADB_EXT_ADDRESS_SRC)]);
                            s2.tdb_src.set(su);
                            let mut su = s2.tdb_dst.get();
                            import_address(&mut su, headers[usize::from(SADB_EXT_ADDRESS_PROXY)]);
                            s2.tdb_dst.set(su);
                            puttdb_locked(s2);
                            mtx_leave(&TDB_SADB_MTX);
                        }
                    }
                    net_unlock();
                    Ok(())
                }

                SADB_ADD => 'c: {
                    let Some(ssa) = sadb_ext::<SadbSa>(&headers, SADB_EXT_SA) else {
                        break 'c Err(Errno::EINVAL);
                    };
                    let sunionp = sadb_address_sunion(headers[usize::from(SADB_EXT_ADDRESS_DST)]);

                    // Either all or none of the flow must be included
                    if !flow_all_or_none(&headers) {
                        break 'c Err(Errno::EINVAL);
                    }
                    // UDP encap has to be enabled and is only supported for ESP
                    if !headers[usize::from(SADB_X_EXT_UDPENCAP)].is_null()
                        && (UDPENCAP_ENABLE.load(Ordering::Relaxed) == 0
                            || smsg.sadb_msg_satype != SADB_SATYPE_ESP)
                    {
                        break 'c Err(Errno::EINVAL);
                    }

                    net_lock();
                    sa2 = gettdb(
                        rdomain,
                        ssa.sadb_sa_spi,
                        &sunionp,
                        SADB_X_GETSPROTO(smsg.sadb_msg_satype),
                    );

                    // We can't add an existing SA!
                    if sa2.is_some() {
                        net_unlock();
                        break 'c Err(Errno::EEXIST);
                    }

                    // We can only add "mature" SAs
                    if ssa.sadb_sa_state != SADB_SASTATE_MATURE {
                        net_unlock();
                        break 'c Err(Errno::EINVAL);
                    }

                    let newsa = match pfkeyv2_newsa(&mut headers, &smsg, rdomain, false) {
                        Ok(t) => t,
                        Err(e) => {
                            net_unlock();
                            break 'c Err(e);
                        }
                    };

                    tdb_addtimeouts(newsa);

                    // Add TDB in table
                    puttdb(newsa);
                    net_unlock();
                    Ok(())
                }

                SADB_DELETE => 'c: {
                    let Some(ssa) = sadb_ext::<SadbSa>(&headers, SADB_EXT_SA) else {
                        break 'c Err(Errno::EINVAL);
                    };
                    let sunionp = sadb_address_sunion(headers[usize::from(SADB_EXT_ADDRESS_DST)]);

                    net_lock();
                    sa2 = gettdb(
                        rdomain,
                        ssa.sadb_sa_spi,
                        &sunionp,
                        SADB_X_GETSPROTO(smsg.sadb_msg_satype),
                    );
                    let Some(s2) = sa2 else {
                        net_unlock();
                        break 'c Err(Errno::ESRCH);
                    };

                    tdb_delete(s2);
                    net_unlock();
                    Ok(())
                }

                SADB_X_ASKPOLICY => 'c: {
                    // Get the relevant policy
                    let Some(pol) = sadb_ext::<SadbXPolicy>(&headers, SADB_X_EXT_POLICY) else {
                        break 'c Err(Errno::EINVAL);
                    };
                    net_lock();
                    let Some(ipa) = ipsec_get_acquire(pol.sadb_x_policy_seq) else {
                        net_unlock();
                        break 'c Err(Errno::ESRCH);
                    };

                    let r = pfkeyv2_policy(ipa, &mut headers);
                    net_unlock();
                    ipsec_unref_acquire(ipa);
                    match r {
                        Ok(v) => {
                            fm.freeme = Some(v);
                            Ok(())
                        }
                        Err(e) => {
                            mode = PFKEYV2_SENDMESSAGE_UNICAST;
                            Err(e)
                        }
                    }
                }

                SADB_GET => 'c: {
                    let Some(ssa) = sadb_ext::<SadbSa>(&headers, SADB_EXT_SA) else {
                        break 'c Err(Errno::EINVAL);
                    };
                    let sunionp = sadb_address_sunion(headers[usize::from(SADB_EXT_ADDRESS_DST)]);

                    net_lock();
                    sa2 = gettdb(
                        rdomain,
                        ssa.sadb_sa_spi,
                        &sunionp,
                        SADB_X_GETSPROTO(smsg.sadb_msg_satype),
                    );
                    let Some(s2) = sa2 else {
                        net_unlock();
                        break 'c Err(Errno::ESRCH);
                    };

                    let mut buffer = Vec::new();
                    let r = pfkeyv2_get(s2, &mut headers, Some(&mut buffer), None, None);
                    net_unlock();
                    fm.freeme = Some(buffer);
                    if r.is_err() {
                        mode = PFKEYV2_SENDMESSAGE_UNICAST;
                    }
                    r
                }

                SADB_REGISTER => 'c: {
                    keylock(kp);
                    if kp.kcb_flags.load(Ordering::Relaxed) & PFKEYV2_SOCKETFLAGS_REGISTERED == 0 {
                        kp.kcb_flags
                            .fetch_or(PFKEYV2_SOCKETFLAGS_REGISTERED, Ordering::Relaxed);
                        mtx_enter(&PFKEYV2_MTX);
                        NREGISTERED.fetch_add(1, Ordering::Relaxed);
                        mtx_leave(&PFKEYV2_MTX);
                    }
                    keyunlock(kp);

                    let mut freeme = match supported(&EALGS) {
                        Ok(v) => v,
                        Err(e) => break 'c Err(e),
                    };
                    headers[usize::from(SADB_EXT_SUPPORTED_ENCRYPT)] = freeme.as_mut_ptr();
                    fm.freeme = Some(freeme);

                    let mut freeme2 = match supported(&AALGS) {
                        Ok(v) => v,
                        Err(e) => break 'c Err(e),
                    };

                    // Keep track what this socket has registered for
                    keylock(kp);
                    kp.kcb_reg
                        .fetch_or(1u32 << u32::from(smsg.sadb_msg_satype), Ordering::Relaxed);
                    keyunlock(kp);

                    headers[usize::from(SADB_EXT_SUPPORTED_AUTH)] = freeme2.as_mut_ptr();
                    fm.freeme2 = Some(freeme2);

                    let mut freeme3 = match supported(&CALGS) {
                        Ok(v) => v,
                        Err(e) => break 'c Err(e),
                    };
                    headers[usize::from(SADB_X_EXT_SUPPORTED_COMP)] = freeme3.as_mut_ptr();
                    fm.freeme3 = Some(freeme3);
                    Ok(())
                }

                // Nothing to handle
                SADB_ACQUIRE | SADB_EXPIRE => Ok(()),

                SADB_FLUSH => {
                    let mut r = Ok(());

                    net_lock();
                    let satype = smsg.sadb_msg_satype;
                    match satype {
                        SADB_SATYPE_UNSPEC
                        | SADB_SATYPE_AH
                        | SADB_SATYPE_ESP
                        | SADB_X_SATYPE_IPIP
                        | SADB_X_SATYPE_IPCOMP
                        | SADB_X_SATYPE_TCPSIGNATURE => {
                            if satype == SADB_SATYPE_UNSPEC {
                                let _ = spd_table_walk(rdomain, pfkeyv2_policy_flush);
                                // FALLTHROUGH
                            }
                            let _ =
                                tdb_walk(rdomain, |tdb, last| pfkeyv2_sa_flush(tdb, satype, last));
                        }
                        _ => r = Err(Errno::EINVAL), // Unknown/unsupported type
                    }
                    net_unlock();
                    r
                }

                SADB_DUMP => {
                    net_lock();
                    let r = tdb_walk(rdomain, |tdb, last| {
                        pfkeyv2_dump_walker(tdb, smsg_p, so, last)
                    });
                    net_unlock();
                    match r {
                        Ok(()) => break 'ret (Ok(()), None),
                        Err(Errno::ENOMEM | Errno::ENOBUFS) => Ok(()),
                        Err(e) => Err(e),
                    }
                }

                SADB_X_GRPSPIS => 'c: {
                    let (Some(ssa), Some(ssa2), Some(sa_proto)) = (
                        sadb_ext::<SadbSa>(&headers, SADB_EXT_SA),
                        sadb_ext::<SadbSa>(&headers, SADB_X_EXT_SA2),
                        sadb_ext::<SadbProtocol>(&headers, SADB_X_EXT_SATYPE2),
                    ) else {
                        break 'c Err(Errno::EINVAL);
                    };
                    let sunionp = sadb_address_sunion(headers[usize::from(SADB_EXT_ADDRESS_DST)]);

                    net_lock();
                    let Some(tdb1) = gettdb(
                        rdomain,
                        ssa.sadb_sa_spi,
                        &sunionp,
                        SADB_X_GETSPROTO(smsg.sadb_msg_satype),
                    ) else {
                        net_unlock();
                        break 'c Err(Errno::ESRCH);
                    };

                    let sunionp2 = sadb_address_sunion(headers[usize::from(SADB_X_EXT_DST2)]);

                    // optionally fetch tdb2 from rdomain2
                    let Some(tdb2) = gettdb(
                        srdomain.map_or(rdomain, |s| u32::from(s.sadb_x_rdomain_dom2)),
                        ssa2.sadb_sa_spi,
                        &sunionp2,
                        SADB_X_GETSPROTO(sa_proto.sadb_protocol_proto),
                    ) else {
                        tdb_unref(Some(tdb1));
                        net_unlock();
                        break 'c Err(Errno::ESRCH);
                    };

                    // Detect cycles
                    let mut tdb3 = Some(tdb2);
                    while let Some(t3) = tdb3 {
                        if ptr::eq(t3, tdb1) {
                            tdb_unref(Some(tdb1));
                            tdb_unref(Some(tdb2));
                            net_unlock();
                            break 'c Err(Errno::ESRCH);
                        }
                        tdb3 = t3.tdb_onext.get();
                    }

                    // Maintenance
                    if let Some(on) = tdb1.tdb_onext.get()
                        && on.tdb_inext.get().is_some_and(|i| ptr::eq(i, tdb1))
                    {
                        tdb_unref(on.tdb_inext.get());
                        on.tdb_inext.set(None);
                    }

                    if let Some(inx) = tdb2.tdb_inext.get()
                        && inx.tdb_onext.get().is_some_and(|o| ptr::eq(o, tdb2))
                    {
                        tdb_unref(inx.tdb_onext.get());
                        inx.tdb_onext.set(None);
                    }

                    // Link them
                    tdb1.tdb_onext.set(Some(tdb2));
                    tdb2.tdb_inext.set(Some(tdb1));
                    net_unlock();
                    Ok(())
                }

                SADB_X_DELFLOW | SADB_X_ADDFLOW => {
                    let delflag = smsg.sadb_msg_type == SADB_X_DELFLOW;
                    pfkeyv2_flow(&mut headers, &mut smsg, smsg_p, rdomain, delflag)
                }

                SADB_X_PROMISC => 'c: {
                    if len >= 2 * size_of::<SadbMsg>() {
                        let packet = match pfdatatopacket(&message) {
                            Ok(p) => p,
                            Err(e) => break 'c Err(e),
                        };

                        rw_enter_read(&PKPTABLE.pkp_lk);
                        for bkp in PKPTABLE.pkp_list.iter() {
                            if ptr::eq(bkp, kp) || bkp.kcb_rdomain != kp.kcb_rdomain {
                                continue;
                            }

                            if smsg.sadb_msg_seq == 0 || smsg.sadb_msg_seq == kp.kcb_pid as u32 {
                                let _ = pfkey_sendup(bkp, packet, true);
                            }
                        }
                        rw_exit_read(&PKPTABLE.pkp_lk);

                        m_freem(packet);
                    } else {
                        if len != size_of::<SadbMsg>() {
                            break 'c Err(Errno::EINVAL);
                        }

                        keylock(kp);
                        let i =
                            kp.kcb_flags.load(Ordering::Relaxed) & PFKEYV2_SOCKETFLAGS_PROMISC != 0;
                        let j = smsg.sadb_msg_satype != 0;

                        if i != j {
                            if j {
                                kp.kcb_flags
                                    .fetch_or(PFKEYV2_SOCKETFLAGS_PROMISC, Ordering::Relaxed);
                                mtx_enter(&PFKEYV2_MTX);
                                NPROMISC.fetch_add(1, Ordering::Relaxed);
                                mtx_leave(&PFKEYV2_MTX);
                            } else {
                                kp.kcb_flags
                                    .fetch_and(!PFKEYV2_SOCKETFLAGS_PROMISC, Ordering::Relaxed);
                                mtx_enter(&PFKEYV2_MTX);
                                NPROMISC.fetch_sub(1, Ordering::Relaxed);
                                mtx_leave(&PFKEYV2_MTX);
                            }
                        }
                        keyunlock(kp);
                    }
                    Ok(())
                }

                _ => Err(Errno::EINVAL),
            }
        };
        let _ = &mut smsg;
        (r, Some(kp))
    };

    // ret:
    let rval = 'realret: {
        let Some(kp) = reply else {
            break 'realret rval;
        };
        if smsg_p.is_null() {
            break 'realret rval;
        }
        // SAFETY: `smsg_p` is the parsed message's header.
        let mut smsg: SadbMsg = unsafe { sadb_get(smsg_p) };
        match rval {
            Err(Errno::EINVAL | Errno::ENOMEM | Errno::ENOBUFS) => break 'realret rval,
            Err(e) => {
                for h in &mut headers[1..] {
                    *h = ptr::null_mut();
                }

                smsg.sadb_msg_errno = (e as i32).unsigned_abs() as u8;
                // SAFETY: as above.
                unsafe { sadb_put(smsg_p, smsg) };
            }
            Ok(()) => {
                let mut seen: u64 = 0;

                for (i, h) in headers.iter().enumerate().skip(1) {
                    if !h.is_null() {
                        seen |= 1u64 << i;
                    }
                }

                let t = usize::from(smsg.sadb_msg_type);
                if seen & SADB_EXTS_ALLOWED_OUT[t] != seen {
                    break 'realret Err(Errno::EPERM);
                }

                if seen & SADB_EXTS_REQUIRED_OUT[t] != SADB_EXTS_REQUIRED_OUT[t] {
                    break 'realret Err(Errno::EPERM);
                }
            }
        }

        // SAFETY: the headers point into the message and the kept buffers.
        unsafe { pfkeyv2_sendmessage(&headers, mode, Some(so), 0, 0, kp.kcb_rdomain) }
    };

    // realret:
    if let Some(v) = fm.freeme.take() {
        pfkey_free(v);
    }
    drop(fm.freeme2.take());
    drop(fm.freeme3.take());

    pfkey_free(message);

    net_lock();
    tdb_unref(sa2);
    net_unlock();

    rval
}

/// The `SADB_X_ADDFLOW`/`SADB_X_DELFLOW` case of `pfkeyv2_dosend`: adds, updates or deletes
/// an SPD entry.
///
/// # Safety
///
/// `headers` is a parsed message whose header is at `smsg_p`.
unsafe fn pfkeyv2_flow(
    headers: &mut SadbHeaders,
    smsg: &mut SadbMsg,
    smsg_p: *mut u8,
    rdomain: u32,
    delflag: bool,
) -> Result<(), Errno> {
    let mut encapdst = SockaddrEncap::new();
    let mut encapnetmask = SockaddrEncap::new();
    let mut exists = false;

    net_lock();
    let Some(rnh) = spd_table_add(rdomain) else {
        net_unlock();
        return Err(Errno::ENOMEM);
    };

    // SAFETY: (for the function) the caller's contract.
    let Some(sab) = (unsafe { sadb_ext::<SadbProtocol>(headers, SADB_X_EXT_FLOW_TYPE) }) else {
        net_unlock();
        return Err(Errno::EINVAL);
    };

    if sab.sadb_protocol_direction != IPSP_DIRECTION_IN
        && sab.sadb_protocol_direction != IPSP_DIRECTION_OUT
    {
        net_unlock();
        return Err(Errno::EINVAL);
    }

    // If the security protocol wasn't specified, pretend it was ESP
    if smsg.sadb_msg_satype == 0 {
        smsg.sadb_msg_satype = SADB_SATYPE_ESP;
        // SAFETY: the message header.
        unsafe { sadb_put(smsg_p, *smsg) };
    }

    let dstp = headers[usize::from(SADB_EXT_ADDRESS_DST)];
    // SAFETY: a whole address extension.
    let sunionp = (!dstp.is_null()).then(|| unsafe { sadb_address_sunion(dstp) });

    let srcp = headers[usize::from(SADB_EXT_ADDRESS_SRC)];
    // SAFETY: a whole address extension.
    let ssrc = (!srcp.is_null()).then(|| unsafe { sadb_address_sunion(srcp) });

    // SAFETY: whole, checked extensions.
    if let Err(e) = unsafe {
        import_flow(
            &mut encapdst,
            &mut encapnetmask,
            headers[usize::from(SADB_X_EXT_SRC_FLOW)],
            headers[usize::from(SADB_X_EXT_SRC_MASK)],
            headers[usize::from(SADB_X_EXT_DST_FLOW)],
            headers[usize::from(SADB_X_EXT_DST_MASK)],
            sadb_ext::<SadbProtocol>(headers, SADB_X_EXT_PROTOCOL),
            sadb_ext::<SadbProtocol>(headers, SADB_X_EXT_FLOW_TYPE),
        )
    } {
        net_unlock();
        return Err(e);
    }

    // Determine whether the exact same SPD entry already exists.
    let mut ipo_ptr: *mut IpsecPolicy = ptr::null_mut();
    // SAFETY: `encapdst` is a `SENT_LEN`-byte key whose first byte is its length.
    if let Some(rn) = unsafe { rn_match(encapdst.as_bytes().as_ptr(), rnh) } {
        // SAFETY: a leaf `rn_match` returned from an SPD tree.
        let ipo = unsafe { ipsec_policy_of(rn) };
        let p = ptr::from_ref(ipo).cast_mut();

        // Verify that the entry is identical
        if ipo.ipo_addr.get() == encapdst && ipo.ipo_mask.get() == encapnetmask {
            exists = true;
            ipo_ptr = p;
        }
    }

    // If the existing policy is static, only delete or update it if the new one is also
    // static.
    if exists {
        // SAFETY: as above.
        let ipo = unsafe { &*ipo_ptr };
        if ipo.ipo_flags.get() & IPSP_POLICY_STATIC != 0
            && sab.sadb_protocol_flags & SADB_X_POLICYFLAGS_POLICY == 0
        {
            net_unlock();
            return Ok(());
        }
    }

    // Delete ?
    if delflag {
        if exists {
            // SAFETY: as above.
            let r = ipsec_delete_policy(unsafe { &*ipo_ptr });
            net_unlock();
            return r;
        }

        // If we were asked to delete something non-existent, error.
        net_unlock();
        return Err(Errno::ESRCH);
    }

    if !exists {
        // Allocate policy entry
        let Some(mem) = pool_get(&IPSEC_POLICY_POOL, PR_NOWAIT | PR_ZERO) else {
            net_unlock();
            return Err(Errno::ENOMEM);
        };
        ipo_ptr = mem.cast::<IpsecPolicy>().as_ptr();
        // SAFETY: a fresh, suitably aligned `ipsec_policy_pool` item, written once before
        // anything else sees it; it stays allocated until `ipsec_delete_policy`.
        unsafe { ipo_ptr.write(IpsecPolicy::new()) };
    }
    // SAFETY: a live policy (new, or found in the tree).
    let ipo: &'static IpsecPolicy = unsafe { &*ipo_ptr };

    let ipo_type = match sab.sadb_protocol_proto {
        SADB_X_FLOW_TYPE_USE => IPSP_IPSEC_USE,
        SADB_X_FLOW_TYPE_ACQUIRE => IPSP_IPSEC_ACQUIRE,
        SADB_X_FLOW_TYPE_REQUIRE => IPSP_IPSEC_REQUIRE,
        SADB_X_FLOW_TYPE_DENY => IPSP_DENY,
        SADB_X_FLOW_TYPE_BYPASS => IPSP_PERMIT,
        SADB_X_FLOW_TYPE_DONTACQ => IPSP_IPSEC_DONTACQ,
        _ => {
            if !exists {
                pool_put(&IPSEC_POLICY_POOL, NonNull::from(ipo).cast());
            } else {
                let _ = ipsec_delete_policy(ipo);
            }

            net_unlock();
            return Err(Errno::EINVAL);
        }
    };
    ipo.ipo_type.set(ipo_type);

    if sab.sadb_protocol_flags & SADB_X_POLICYFLAGS_POLICY != 0 {
        ipo.ipo_flags.set(ipo.ipo_flags.get() | IPSP_POLICY_STATIC);
    }

    ipo.ipo_dst.set(sunionp.unwrap_or_default());
    ipo.ipo_src.set(ssrc.unwrap_or_default());

    ipo.ipo_sproto.set(SADB_X_GETSPROTO(smsg.sadb_msg_satype));

    if let Some(ids) = ipo.ipo_ids.get() {
        ipsp_ids_free(Some(ids));
        ipo.ipo_ids.set(None);
    }

    let sid = headers[usize::from(SADB_EXT_IDENTITY_SRC)];
    let did = headers[usize::from(SADB_EXT_IDENTITY_DST)];
    if !sid.is_null() && !did.is_null() {
        // SAFETY: whole identity extensions.
        ipo.ipo_ids
            .set(unsafe { import_identities(false, sid, did) });
        if ipo.ipo_ids.get().is_none() {
            if exists {
                let _ = ipsec_delete_policy(ipo);
            } else {
                pool_put(&IPSEC_POLICY_POOL, NonNull::from(ipo).cast());
            }
            net_unlock();
            return Err(Errno::ENOBUFS);
        }
    }

    // Flow type
    if !exists {
        // Initialize policy entry
        ipo.ipo_addr.set(encapdst);
        ipo.ipo_mask.set(encapnetmask);

        ipo.ipo_acquires.init();
        ipo.ipo_rdomain.set(rdomain);
        refcnt_init(&ipo.ipo_refcnt);

        // Add SPD entry
        let added = spd_table_get(rdomain).and_then(|rnh| {
            // SAFETY: the key and mask are the policy's `SENT_LEN`-byte encap addresses and
            // the nodes its `ipo_nodes`, which stay in place while it is in the tree.
            unsafe {
                rn_addroute(
                    ipo.ipo_addr_key(),
                    ipo.ipo_mask_key(),
                    rnh,
                    &ipo.ipo_nodes,
                    0,
                )
            }
        });
        if added.is_none() {
            // Remove from linked list of policies on TDB
            mtx_enter(&IPO_TDB_MTX);
            if let Some(t) = ipo.ipo_tdb.get() {
                // SAFETY: `ipo_tdb_mtx` is held; the policy is on that TDB's list.
                unsafe { t.tdb_policy_head.remove(ipo) };
                tdb_unref(Some(t));
                ipo.ipo_tdb.set(None);
            }
            mtx_leave(&IPO_TDB_MTX);
            if let Some(ids) = ipo.ipo_ids.get() {
                ipsp_ids_free(Some(ids));
            }
            pool_put(&IPSEC_POLICY_POOL, NonNull::from(ipo).cast());
            net_unlock();
            return Ok(());
        }
        // SAFETY: the exclusive net lock is held; the policy is new, on no list.
        unsafe { IPSEC_POLICY_HEAD.0.insert_head(ipo) };
        IPSEC_IN_USE.fetch_add(1, Ordering::Relaxed);
    } else {
        ipo.ipo_last_searched.set(0);
        ipo.ipo_flags.set(0);
    }
    net_unlock();

    Ok(())
}

/// `pfkeyv2_acquire`: send an ACQUIRE message to key management, to get a new SA. `seq`
/// gets the message's sequence number.
pub fn pfkeyv2_acquire(
    ipo: &IpsecPolicy,
    gw: &SockaddrUnion,
    laddr: Option<&SockaddrUnion>,
    seq: &Cell<u32>,
    _ddst: &SockaddrEncap,
) -> Result<(), Errno> {
    let require_pfs_local = IPSEC_REQUIRE_PFS.load(Ordering::Relaxed);

    let def_enc_local = IPSEC_DEF_ENC.load(Ordering::Relaxed);
    let def_comp_local = IPSEC_DEF_COMP.load(Ordering::Relaxed);
    let def_auth_local = IPSEC_DEF_AUTH.load(Ordering::Relaxed);

    let soft_allocations_local = IPSEC_SOFT_ALLOCATIONS.load(Ordering::Relaxed);
    let exp_allocations_local = IPSEC_EXP_ALLOCATIONS.load(Ordering::Relaxed);

    let soft_bytes_local = IPSEC_SOFT_BYTES.load(Ordering::Relaxed);
    let exp_bytes_local = IPSEC_EXP_BYTES.load(Ordering::Relaxed);

    let soft_timeout_local = IPSEC_SOFT_TIMEOUT.load(Ordering::Relaxed);
    let exp_timeout_local = IPSEC_EXP_TIMEOUT.load(Ordering::Relaxed);

    let soft_first_use_local = IPSEC_SOFT_FIRST_USE.load(Ordering::Relaxed);
    let exp_first_use_local = IPSEC_EXP_FIRST_USE.load(Ordering::Relaxed);

    mtx_enter(&PFKEYV2_MTX);
    seq.set(PFKEYV2_SEQ.fetch_add(1, Ordering::Relaxed));

    let registered = NREGISTERED.load(Ordering::Relaxed);
    mtx_leave(&PFKEYV2_MTX);

    if registered == 0 {
        return Err(Errno::ESRCH);
    }

    // How large a buffer do we need... XXX we only do one proposal for now
    let mut i = size_of::<SadbMsg>()
        + laddr.map_or(0, |_| {
            size_of::<SadbAddress>() + padup(usize::from(ipo.ipo_src.get().sa_len()))
        })
        + size_of::<SadbAddress>()
        + padup(usize::from(gw.sa_len()))
        + size_of::<SadbProp>()
        + size_of::<SadbComb>();

    if let Some(ids) = ipo.ipo_ids.get() {
        i += size_of::<SadbIdent>() + padup(ids.id_local().len as usize);
        i += size_of::<SadbIdent>() + padup(ids.id_remote().len as usize);
    }

    // Allocate
    let mut buffer = pfkey_alloc(i)?;
    let mut headers = sadb_headers_new();

    let base = buffer.as_mut_ptr();
    headers[0] = base;

    let mut smsg = SadbMsg {
        sadb_msg_version: PF_KEY_V2,
        sadb_msg_type: SADB_ACQUIRE,
        sadb_msg_len: (i / size_of::<u64>()) as u16,
        sadb_msg_seq: seq.get(),
        ..SadbMsg::default()
    };

    let sproto = i32::from(ipo.ipo_sproto.get());
    if sproto == IPPROTO_ESP {
        smsg.sadb_msg_satype = SADB_SATYPE_ESP;
    } else if sproto == IPPROTO_AH {
        smsg.sadb_msg_satype = SADB_SATYPE_AH;
    } else if sproto == IPPROTO_IPCOMP {
        smsg.sadb_msg_satype = SADB_X_SATYPE_IPCOMP;
    }

    // SAFETY: the buffer was sized above for every extension written.
    let rval = unsafe {
        sadb_put(base, smsg);
        let mut p = base.add(size_of::<SadbMsg>());

        let mut addr = |p: &mut *mut u8, which: u16, su: &SockaddrUnion| {
            headers[usize::from(which)] = *p;
            let sadd = SadbAddress {
                sadb_address_len: (size_of::<SadbAddress>() + usize::from(su.sa_len()))
                    .div_ceil(size_of::<u64>()) as u16,
                ..SadbAddress::default()
            };
            sadb_put(*p, sadd);
            ptr::copy_nonoverlapping(
                su.as_bytes().as_ptr(),
                p.add(size_of::<SadbAddress>()),
                su.sa_bytes().len(),
            );
            *p = p.add(size_of::<SadbAddress>() + padup(usize::from(su.sa_len())));
        };

        if let Some(laddr) = laddr {
            addr(&mut p, SADB_EXT_ADDRESS_SRC, laddr);
        }

        addr(&mut p, SADB_EXT_ADDRESS_DST, gw);

        if let Some(ids) = ipo.ipo_ids.get() {
            export_identities(&mut p, ids, false, &mut headers);
        }

        headers[usize::from(SADB_EXT_PROPOSAL)] = p;
        let sa_prop = SadbProp {
            sadb_prop_num: 1, // XXX One proposal only
            sadb_prop_len: ((size_of::<SadbProp>() + size_of::<SadbComb>()) / size_of::<u64>())
                as u16,
            ..SadbProp::default()
        };
        sadb_put(p, sa_prop);
        p = p.add(size_of::<SadbProp>());

        // XXX Should actually ask the crypto layer what's supported
        for _ in 0..sa_prop.sadb_prop_num {
            let mut comb = SadbComb::default();
            if require_pfs_local != 0 {
                comb.sadb_comb_flags |= SADB_SAFLAGS_PFS as u16;
            }

            if sproto == IPPROTO_ESP {
                // Set the encryption algorithm
                let (e, min, max) = match def_enc_local {
                    IPSEC_ENC_AES => (SADB_X_EALG_AES, 128, 256),
                    IPSEC_ENC_AESCTR => (SADB_X_EALG_AESCTR, 128 + 32, 256 + 32),
                    IPSEC_ENC_3DES => (SADB_EALG_3DESCBC, 192, 192),
                    IPSEC_ENC_BLOWFISH => (SADB_X_EALG_BLF, 40, (BLF_MAXKEYLEN * 8) as u16),
                    IPSEC_ENC_CAST128 => (SADB_X_EALG_CAST, 40, 128),
                    _ => (0, 0, 0),
                };
                if e != 0 {
                    comb.sadb_comb_encrypt = e;
                    comb.sadb_comb_encrypt_minbits = min;
                    comb.sadb_comb_encrypt_maxbits = max;
                }
            } else if sproto == IPPROTO_IPCOMP {
                // Set the compression algorithm
                if def_comp_local == IPSEC_COMP_DEFLATE {
                    comb.sadb_comb_encrypt = SADB_X_CALG_DEFLATE;
                    comb.sadb_comb_encrypt_minbits = 0;
                    comb.sadb_comb_encrypt_maxbits = 0;
                }
            }

            // Set the authentication algorithm
            let (a, min, max) = match def_auth_local {
                IPSEC_AUTH_HMAC_SHA1 => (SADB_AALG_SHA1HMAC, 160, 160),
                IPSEC_AUTH_HMAC_RIPEMD160 => (SADB_X_AALG_RIPEMD160HMAC, 160, 160),
                IPSEC_AUTH_MD5 => (SADB_AALG_MD5HMAC, 128, 128),
                IPSEC_AUTH_SHA2_256 => (SADB_X_AALG_SHA2_256, 256, 256),
                IPSEC_AUTH_SHA2_384 => (SADB_X_AALG_SHA2_384, 384, 384),
                IPSEC_AUTH_SHA2_512 => (SADB_X_AALG_SHA2_512, 512, 512),
                _ => (0, 0, 0),
            };
            if a != 0 {
                comb.sadb_comb_auth = a;
                comb.sadb_comb_auth_minbits = min;
                comb.sadb_comb_auth_maxbits = max;
            }

            comb.sadb_comb_soft_allocations = soft_allocations_local as u32;
            comb.sadb_comb_hard_allocations = exp_allocations_local as u32;

            comb.sadb_comb_soft_bytes = soft_bytes_local as u64;
            comb.sadb_comb_hard_bytes = exp_bytes_local as u64;

            comb.sadb_comb_soft_addtime = soft_timeout_local as u64;
            comb.sadb_comb_hard_addtime = exp_timeout_local as u64;

            comb.sadb_comb_soft_usetime = soft_first_use_local as u64;
            comb.sadb_comb_hard_usetime = exp_first_use_local as u64;
            sadb_put(p, comb);
            p = p.add(size_of::<SadbComb>());
        }

        // Send the ACQUIRE message to all compliant registered listeners.
        pfkeyv2_sendmessage(
            &headers,
            PFKEYV2_SENDMESSAGE_REGISTERED,
            None,
            smsg.sadb_msg_satype,
            0,
            ipo.ipo_rdomain.get(),
        )
    };

    pfkey_free(buffer);

    rval
}

/// `pfkeyv2_expire`: notify key management that an expiration went off. `type_` specifies
/// the type of expiration (`SADB_EXT_LIFETIME_SOFT` or `_HARD`).
pub fn pfkeyv2_expire(tdb: &Tdb, type_: u16) -> Result<(), Errno> {
    net_assert_locked("pfkeyv2_expire");

    match i32::from(tdb.tdb_sproto.get()) {
        // TCP_SIGNATURE: IPPROTO_TCP
        IPPROTO_AH
        | IPPROTO_ESP
        | IPPROTO_IPIP
        | IPPROTO_IPCOMP
        | crate::netinet::in_::IPPROTO_TCP => {}
        _ => return Err(Errno::EOPNOTSUPP),
    }

    let i = size_of::<SadbMsg>()
        + size_of::<SadbSa>()
        + 2 * size_of::<SadbLifetime>()
        + size_of::<SadbAddress>()
        + padup(usize::from(tdb.tdb_src.get().sa_len()))
        + size_of::<SadbAddress>()
        + padup(usize::from(tdb.tdb_dst.get().sa_len()));

    let mut buffer = pfkey_alloc(i)?;
    let mut headers = sadb_headers_new();

    let base = buffer.as_mut_ptr();
    headers[0] = base;

    mtx_enter(&PFKEYV2_MTX);
    let seq = PFKEYV2_SEQ.fetch_add(1, Ordering::Relaxed);
    mtx_leave(&PFKEYV2_MTX);

    let smsg = SadbMsg {
        sadb_msg_version: PF_KEY_V2,
        sadb_msg_type: SADB_EXPIRE,
        sadb_msg_satype: tdb.tdb_satype.get(),
        sadb_msg_len: (i / size_of::<u64>()) as u16,
        sadb_msg_seq: seq,
        ..SadbMsg::default()
    };

    // SAFETY: the buffer was sized above for every extension written.
    let rval = unsafe {
        sadb_put(base, smsg);
        let mut p = base.add(size_of::<SadbMsg>());

        headers[usize::from(SADB_EXT_SA)] = p;
        export_sa(&mut p, tdb);

        headers[usize::from(SADB_EXT_LIFETIME_CURRENT)] = p;
        export_lifetime(&mut p, tdb, PFKEYV2_LIFETIME_CURRENT);

        headers[usize::from(type_)] = p;
        export_lifetime(
            &mut p,
            tdb,
            if type_ == SADB_EXT_LIFETIME_SOFT {
                PFKEYV2_LIFETIME_SOFT
            } else {
                PFKEYV2_LIFETIME_HARD
            },
        );

        headers[usize::from(SADB_EXT_ADDRESS_SRC)] = p;
        export_address(&mut p, tdb.tdb_src.get().as_sockaddr_ptr());

        headers[usize::from(SADB_EXT_ADDRESS_DST)] = p;
        export_address(&mut p, tdb.tdb_dst.get().as_sockaddr_ptr());

        let mut r = pfkeyv2_sendmessage(
            &headers,
            PFKEYV2_SENDMESSAGE_BROADCAST,
            None,
            0,
            0,
            tdb.tdb_rdomain.get(),
        );
        // XXX
        if r.is_ok() && tdb.tdb_rdomain.get() != tdb.tdb_rdomain_post.get() {
            r = pfkeyv2_sendmessage(
                &headers,
                PFKEYV2_SENDMESSAGE_BROADCAST,
                None,
                0,
                0,
                tdb.tdb_rdomain_post.get(),
            );
        }
        r
    };

    pfkey_free(buffer);

    rval
}

/// `struct pfkeyv2_sysctl_walk`.
struct Pfkeyv2SysctlWalk {
    /// `w_where`: the user address written to, 0 to only count.
    w_where: usize,
    /// `w_len`.
    w_len: usize,
    /// `w_op`.
    w_op: i32,
    /// `w_satype`.
    w_satype: u8,
}

/// Copies out a message header of `type_`/`satype` for `len` bytes of extensions, then the
/// extensions (with their types set), as the sysctl walkers do.
///
/// # Safety
///
/// `headers` point into `buffer`, at whole extensions.
unsafe fn sysctl_copyout(
    w: &mut Pfkeyv2SysctlWalk,
    type_: u8,
    satype: u8,
    headers: &SadbHeaders,
    buffer: &[u8],
) -> Result<(), Errno> {
    // prepend header
    let msg = SadbMsg {
        sadb_msg_version: PF_KEY_V2,
        sadb_msg_satype: satype,
        sadb_msg_type: type_,
        sadb_msg_len: ((size_of::<SadbMsg>() + buffer.len()) / size_of::<u64>()) as u16,
        ..SadbMsg::default()
    };
    let mut mb = [0u8; size_of::<SadbMsg>()];
    // SAFETY: `mb` is a message header's size.
    unsafe { sadb_put(mb.as_mut_ptr(), msg) };
    copyout(&mb, w.w_where)?;
    w.w_where += size_of::<SadbMsg>();
    w.w_len -= size_of::<SadbMsg>();
    // set extension type
    for (i, &h) in headers.iter().enumerate().skip(1) {
        if !h.is_null() {
            // SAFETY: the caller's contract.
            unsafe {
                let mut e: SadbExt = sadb_get(h);
                e.sadb_ext_type = i as u16;
                sadb_put(h, e);
            }
        }
    }
    copyout(buffer, w.w_where)?;
    w.w_where += buffer.len();
    w.w_len -= buffer.len();
    Ok(())
}

/// `pfkeyv2_sysctl_walker`: one SA of `net.key.sadb_dump`.
fn pfkeyv2_sysctl_walker(tdb: &Tdb, w: &mut Pfkeyv2SysctlWalk, _last: bool) -> Result<(), Errno> {
    if w.w_satype != SADB_SATYPE_UNSPEC && w.w_satype != tdb.tdb_satype.get() {
        return Ok(());
    }

    let mut headers = sadb_headers_new();
    if w.w_where != 0 {
        let mut buffer = Vec::new();
        let mut usedlen = 0;
        // SAFETY: the headers point into `buffer`, which lives until the end.
        let r = unsafe {
            pfkeyv2_get(
                tdb,
                &mut headers,
                Some(&mut buffer),
                None,
                Some(&mut usedlen),
            )
        };
        let r = r.and_then(|()| {
            if w.w_len < size_of::<SadbMsg>() + usedlen {
                return Err(Errno::ENOMEM);
            }
            // SAFETY: as above.
            unsafe {
                sysctl_copyout(
                    w,
                    SADB_DUMP,
                    tdb.tdb_satype.get(),
                    &headers,
                    &buffer[..usedlen],
                )
            }
        });
        pfkey_free(buffer);
        r
    } else {
        let mut buflen = 0;
        // SAFETY: no buffer: only the length is computed.
        unsafe { pfkeyv2_get(tdb, &mut headers, None, Some(&mut buflen), None) }?;
        w.w_len += buflen;
        w.w_len += size_of::<SadbMsg>();
        Ok(())
    }
}

/// `pfkeyv2_dump_policy`: a flow as PF_KEY extensions; with `buffer` `None` only the length
/// (`*lenp`).
///
/// # Safety
///
/// As for [`pfkeyv2_get`].
pub unsafe fn pfkeyv2_dump_policy(
    ipo: &IpsecPolicy,
    headers: &mut SadbHeaders,
    buffer: Option<&mut Vec<u8>>,
    lenp: Option<&mut usize>,
) -> Result<(), Errno> {
    // Find how much space we need.
    let mut i = 2 * size_of::<SadbProtocol>();

    // We'll need four of them: src, src mask, dst, dst mask.
    match ipo.ipo_addr.get().sen_type() {
        SENT_IP4 => {
            i += 4 * padup(size_of::<SockaddrIn>());
            i += 4 * size_of::<SadbAddress>();
        }
        #[cfg(feature = "inet6")]
        SENT_IP6 => {
            i += 4 * padup(size_of::<SockaddrIn6>());
            i += 4 * size_of::<SadbAddress>();
        }
        _ => return Err(Errno::EINVAL),
    }

    // Local address, might be zeroed.
    match ipo.ipo_src.get().sa_family() {
        0 => {}
        AF_INET => {
            i += padup(size_of::<SockaddrIn>());
            i += size_of::<SadbAddress>();
        }
        #[cfg(feature = "inet6")]
        AF_INET6 => {
            i += padup(size_of::<SockaddrIn6>());
            i += size_of::<SadbAddress>();
        }
        _ => return Err(Errno::EINVAL),
    }

    // Remote address, might be zeroed. XXX ???
    match ipo.ipo_dst.get().sa_family() {
        0 => {}
        AF_INET => {
            i += padup(size_of::<SockaddrIn>());
            i += size_of::<SadbAddress>();
        }
        #[cfg(feature = "inet6")]
        AF_INET6 => {
            i += padup(size_of::<SockaddrIn6>());
            i += size_of::<SadbAddress>();
        }
        _ => return Err(Errno::EINVAL),
    }

    if let Some(ids) = ipo.ipo_ids.get() {
        i += size_of::<SadbIdent>() + padup(ids.id_local().len as usize);
        i += size_of::<SadbIdent>() + padup(ids.id_remote().len as usize);
    }

    if let Some(lenp) = lenp {
        *lenp = i;
    }

    let Some(buffer) = buffer else {
        return Ok(());
    };

    *buffer = pfkey_alloc(i)?;
    let mut p = buffer.as_mut_ptr();

    // SAFETY: the buffer was sized above for every extension written.
    unsafe {
        // Local address.
        if ipo.ipo_src.get().sa_family() != 0 {
            headers[usize::from(SADB_EXT_ADDRESS_SRC)] = p;
            export_address(&mut p, ipo.ipo_src.get().as_sockaddr_ptr());
        }

        // Remote address.
        if ipo.ipo_dst.get().sa_family() != 0 {
            headers[usize::from(SADB_EXT_ADDRESS_DST)] = p;
            export_address(&mut p, ipo.ipo_dst.get().as_sockaddr_ptr());
        }

        // Get actual flow.
        export_flow(
            &mut p,
            ipo.ipo_type.get(),
            &ipo.ipo_addr.get(),
            &ipo.ipo_mask.get(),
            headers,
        );

        // Add ids only when we are root.
        let perm = suser(curproc_or_panic("pfkeyv2_dump_policy"));
        if perm.is_ok()
            && let Some(ids) = ipo.ipo_ids.get()
        {
            export_identities(&mut p, ids, false, headers);
        }
    }

    Ok(())
}

/// `pfkeyv2_sysctl_policydumper`: one flow of `net.key.spd_dump`.
fn pfkeyv2_sysctl_policydumper(
    ipo: &IpsecPolicy,
    w: &mut Pfkeyv2SysctlWalk,
    _tableid: u32,
) -> Result<(), Errno> {
    let mut headers = sadb_headers_new();
    if w.w_where != 0 {
        let mut buffer = Vec::new();
        let mut buflen = 0;
        // SAFETY: the headers point into `buffer`, which lives until the end.
        let r =
            unsafe { pfkeyv2_dump_policy(ipo, &mut headers, Some(&mut buffer), Some(&mut buflen)) };
        let r = r.and_then(|()| {
            if w.w_len < buflen {
                return Err(Errno::ENOMEM);
            }
            let sproto = i32::from(ipo.ipo_sproto.get());
            let satype = if sproto == IPPROTO_ESP {
                SADB_SATYPE_ESP
            } else if sproto == IPPROTO_AH {
                SADB_SATYPE_AH
            } else if sproto == IPPROTO_IPCOMP {
                SADB_X_SATYPE_IPCOMP
            } else if sproto == IPPROTO_IPIP {
                SADB_X_SATYPE_IPIP
            } else {
                0
            };
            // SAFETY: as above.
            unsafe { sysctl_copyout(w, SADB_X_SPDDUMP, satype, &headers, &buffer[..buflen]) }
        });
        drop(buffer);
        r
    } else {
        let mut buflen = 0;
        // SAFETY: no buffer: only the length is computed.
        unsafe { pfkeyv2_dump_policy(ipo, &mut headers, None, Some(&mut buflen)) }?;
        w.w_len += buflen;
        w.w_len += size_of::<SadbMsg>();
        Ok(())
    }
}

/// `pfkeyv2_policy_flush`: deletes a flow; `EAGAIN` makes the walk start over (the tree
/// changed).
fn pfkeyv2_policy_flush(ipo: &'static IpsecPolicy, _tableid: u32) -> Result<(), Errno> {
    ipsec_delete_policy(ipo)?;
    Err(Errno::EAGAIN)
}

/// `pfkeyv2_sysctl`: `net.key`: `NET_KEY_SADB_DUMP` (root only) and `NET_KEY_SPD_DUMP`.
pub fn pfkeyv2_sysctl(
    name: &[i32],
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    _newlen: usize,
) -> Result<(), Errno> {
    if newp != 0 {
        return Err(Errno::EPERM);
    }
    let Some(&op) = name.first() else {
        return Err(Errno::EINVAL);
    };
    let mut w = Pfkeyv2SysctlWalk {
        w_op: op,
        w_satype: if name.len() >= 2 {
            name[1] as u8
        } else {
            SADB_SATYPE_UNSPEC
        },
        w_where: oldp,
        w_len: if oldp != 0 { *oldlenp } else { 0 },
    };

    let p = curproc_or_panic("pfkeyv2_sysctl");
    let tableid = if name.len() == 3 {
        let t = name[2] as u32;
        if !rtable_exists(t) {
            return Err(Errno::ENOENT);
        }
        t
    } else {
        p.process().ps_rtableid.load(Ordering::Relaxed)
    };
    let rdomain = rtable_l2(tableid);

    let mut error = Err(Errno::EINVAL);
    match w.w_op {
        NET_KEY_SADB_DUMP => {
            suser(p)?;
            net_lock();
            error = tdb_walk(rdomain, |tdb, last| {
                pfkeyv2_sysctl_walker(tdb, &mut w, last)
            });
            net_unlock();
            *oldlenp = if oldp != 0 { w.w_where - oldp } else { w.w_len };
        }

        NET_KEY_SPD_DUMP => {
            net_lock_shared();
            error = spd_table_walk(rdomain, |ipo, tableid| {
                pfkeyv2_sysctl_policydumper(ipo, &mut w, tableid)
            });
            net_unlock_shared();
            *oldlenp = if oldp != 0 { w.w_where - oldp } else { w.w_len };
        }
        _ => {}
    }

    error
}

/// Forgets every pfkey socket and counter: the host tests start from an empty table.
#[cfg(test)]
pub(crate) fn pfkey_reset() {
    PKPTABLE.pkp_list.init();
    NREGISTERED.store(0, Ordering::Relaxed);
    NPROMISC.store(0, Ordering::Relaxed);
}

// LP64 sizes of the ABI structures.
const _: () = {
    assert!(size_of::<SadbMsg>() == 16);
    assert!(size_of::<SadbExt>() == 4);
    assert!(size_of::<SadbSa>() == 16);
    assert!(size_of::<SadbLifetime>() == 32);
    assert!(size_of::<SadbAddress>() == 8);
    assert!(size_of::<SadbKey>() == 8);
    assert!(size_of::<SadbIdent>() == 16);
    assert!(size_of::<SadbSens>() == 16);
    assert!(size_of::<SadbProp>() == 8);
    assert!(size_of::<SadbComb>() == 72);
    assert!(size_of::<SadbSupported>() == 8);
    assert!(size_of::<SadbAlg>() == 8);
    assert!(size_of::<SadbSpirange>() == 16);
    assert!(size_of::<SadbProtocol>() == 8);
    assert!(size_of::<SadbXPolicy>() == 8);
    assert!(size_of::<SadbXUdpencap>() == 8);
    assert!(size_of::<SadbXTag>() == 8);
    assert!(size_of::<SadbXReplay>() == 16);
    assert!(size_of::<SadbXRdomain>() == 8);
    assert!(size_of::<SadbXTap>() == 8);
    assert!(size_of::<SadbXCounter>() == 72);
    assert!(size_of::<SadbXMtu>() == 8);
    assert!(size_of::<SadbXIface>() == 16);
    assert!(offset_of!(SadbMsg, sadb_msg_seq) == 8);
};

#[cfg(test)]
mod tests;
