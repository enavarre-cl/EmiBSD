/* <LICENSES> */
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

/* <CODE> */
//! `PF_KEY` message validation: `net/pfkeyv2_parsemessage.c`. Checks a message from userland
//! (lengths, types, which extensions each message may and must carry, the contents of each
//! extension) and fills `headers[]` with a pointer per extension; also the tables of the
//! extensions a reply may and must carry.
//!
//! Upstream: sys/net/pfkeyv2_parsemessage.c @ 3ce1f3f79392
//!
//! Status: `ported` (M9c).
//!
//! ## Deviations
//! - The message is a byte slice: every extension is read with a bounds-checked unaligned
//!   copy at its offset, and the pointers stored in `headers[]` point into the slice (the C
//!   casts `void *` cursors).
//! - The `BITMAP_*` macros are constants, `sadb_exts_allowed_in`/`sadb_exts_required_in`
//!   (writable in C, never written) are constant tables like the `_out` ones.
//! - `max_alg` of the `SUPPORTED` extensions keeps the C's expression, whose second test is
//!   the constant `SADB_EXT_SUPPORTED_ENCRYPT` (always true): the compressor table is checked
//!   against `SADB_EALG_MAX`.
//! - `NPF` (pf(4)) is configured: the `SADB_X_EXT_TAG` and `SADB_X_EXT_TAP` checks.
//! - `INET6` is configured (feature `inet6`): the `AF_INET6` address checks of the ADDRESS
//!   extensions.

use core::mem::{offset_of, size_of};

use crate::machine::cpu::curproc;
use crate::net::pfkeyv2::{
    SADB_AALG_MAX, SADB_ADD, SADB_EALG_MAX, SADB_EXT_ADDRESS_DST, SADB_EXT_ADDRESS_PROXY,
    SADB_EXT_ADDRESS_SRC, SADB_EXT_IDENTITY_DST, SADB_EXT_IDENTITY_SRC, SADB_EXT_KEY_AUTH,
    SADB_EXT_KEY_ENCRYPT, SADB_EXT_LIFETIME_CURRENT, SADB_EXT_LIFETIME_HARD,
    SADB_EXT_LIFETIME_SOFT, SADB_EXT_MAX, SADB_EXT_PROPOSAL, SADB_EXT_SA, SADB_EXT_SENSITIVITY,
    SADB_EXT_SPIRANGE, SADB_EXT_SUPPORTED_AUTH, SADB_EXT_SUPPORTED_ENCRYPT, SADB_IDENTTYPE_MAX,
    SADB_MAX, SADB_SASTATE_DEAD, SADB_SASTATE_MATURE, SADB_SASTATE_MAX, SADB_UPDATE,
    SADB_X_CALG_MAX, SADB_X_EXT_COUNTER, SADB_X_EXT_DST_FLOW, SADB_X_EXT_DST_MASK, SADB_X_EXT_DST2,
    SADB_X_EXT_FLOW_TYPE, SADB_X_EXT_IFACE, SADB_X_EXT_LIFETIME_LASTUSE, SADB_X_EXT_MTU,
    SADB_X_EXT_POLICY, SADB_X_EXT_PROTOCOL, SADB_X_EXT_RDOMAIN, SADB_X_EXT_REPLAY, SADB_X_EXT_SA2,
    SADB_X_EXT_SATYPE2, SADB_X_EXT_SRC_FLOW, SADB_X_EXT_SRC_MASK, SADB_X_EXT_SUPPORTED_COMP,
    SADB_X_EXT_TAG, SADB_X_EXT_TAP, SADB_X_EXT_UDPENCAP, SADB_X_SATYPE_TCPSIGNATURE, SadbAddress,
    SadbAlg, SadbComb, SadbExt, SadbHeaders, SadbIdent, SadbKey, SadbLifetime, SadbMsg, SadbProp,
    SadbProtocol, SadbSa, SadbSens, SadbSpirange, SadbSupported, SadbXCounter, SadbXIface,
    SadbXPolicy, SadbXRdomain, SadbXReplay, SadbXTag, SadbXTap, SadbXUdpencap, padup, sadb_get,
    sadb_headers_new,
};
use crate::net::pfvar::PF_TAG_NAME_SIZE;
use crate::netinet::in_::SockaddrIn;
#[cfg(feature = "inet6")]
use crate::netinet6::in6::SockaddrIn6;
use crate::sys::errno::Errno;
#[cfg(feature = "inet6")]
use crate::sys::socket::AF_INET6;
use crate::sys::socket::{AF_INET, Sockaddr};

/// `DPRINTF` of this file.
macro_rules! dprintf {
    ($($arg:tt)*) => {
        crate::ipsec_dprintf!("pfkeyv2_parsemessage", $($arg)*)
    };
}

/// `BITMAP_SA`.
const BITMAP_SA: u64 = 1 << SADB_EXT_SA;
/// `BITMAP_LIFETIME_CURRENT`.
const BITMAP_LIFETIME_CURRENT: u64 = 1 << SADB_EXT_LIFETIME_CURRENT;
/// `BITMAP_LIFETIME_HARD`.
const BITMAP_LIFETIME_HARD: u64 = 1 << SADB_EXT_LIFETIME_HARD;
/// `BITMAP_LIFETIME_SOFT`.
const BITMAP_LIFETIME_SOFT: u64 = 1 << SADB_EXT_LIFETIME_SOFT;
/// `BITMAP_ADDRESS_SRC`.
const BITMAP_ADDRESS_SRC: u64 = 1 << SADB_EXT_ADDRESS_SRC;
/// `BITMAP_ADDRESS_DST`.
const BITMAP_ADDRESS_DST: u64 = 1 << SADB_EXT_ADDRESS_DST;
/// `BITMAP_ADDRESS_PROXY`.
const BITMAP_ADDRESS_PROXY: u64 = 1 << SADB_EXT_ADDRESS_PROXY;
/// `BITMAP_KEY_AUTH`.
const BITMAP_KEY_AUTH: u64 = 1 << SADB_EXT_KEY_AUTH;
/// `BITMAP_KEY_ENCRYPT`.
const BITMAP_KEY_ENCRYPT: u64 = 1 << SADB_EXT_KEY_ENCRYPT;
/// `BITMAP_IDENTITY_SRC`.
const BITMAP_IDENTITY_SRC: u64 = 1 << SADB_EXT_IDENTITY_SRC;
/// `BITMAP_IDENTITY_DST`.
const BITMAP_IDENTITY_DST: u64 = 1 << SADB_EXT_IDENTITY_DST;
/// `BITMAP_SENSITIVITY`.
#[allow(dead_code)] // the C defines it; no table uses it
const BITMAP_SENSITIVITY: u64 = 1 << SADB_EXT_SENSITIVITY;
/// `BITMAP_PROPOSAL`.
const BITMAP_PROPOSAL: u64 = 1 << SADB_EXT_PROPOSAL;
/// `BITMAP_SUPPORTED_AUTH`.
const BITMAP_SUPPORTED_AUTH: u64 = 1 << SADB_EXT_SUPPORTED_AUTH;
/// `BITMAP_SUPPORTED_ENCRYPT`.
const BITMAP_SUPPORTED_ENCRYPT: u64 = 1 << SADB_EXT_SUPPORTED_ENCRYPT;
/// `BITMAP_SPIRANGE`.
const BITMAP_SPIRANGE: u64 = 1 << SADB_EXT_SPIRANGE;
/// `BITMAP_LIFETIME`.
const BITMAP_LIFETIME: u64 = BITMAP_LIFETIME_CURRENT | BITMAP_LIFETIME_HARD | BITMAP_LIFETIME_SOFT;
/// `BITMAP_ADDRESS`.
const BITMAP_ADDRESS: u64 = BITMAP_ADDRESS_SRC | BITMAP_ADDRESS_DST;
/// `BITMAP_KEY`.
const BITMAP_KEY: u64 = BITMAP_KEY_AUTH | BITMAP_KEY_ENCRYPT;
/// `BITMAP_IDENTITY`.
const BITMAP_IDENTITY: u64 = BITMAP_IDENTITY_SRC | BITMAP_IDENTITY_DST;
/// `BITMAP_MSG`.
const BITMAP_MSG: u64 = 1;
/// `BITMAP_X_SRC_MASK`.
const BITMAP_X_SRC_MASK: u64 = 1 << SADB_X_EXT_SRC_MASK;
/// `BITMAP_X_DST_MASK`.
const BITMAP_X_DST_MASK: u64 = 1 << SADB_X_EXT_DST_MASK;
/// `BITMAP_X_PROTOCOL`.
const BITMAP_X_PROTOCOL: u64 = 1 << SADB_X_EXT_PROTOCOL;
/// `BITMAP_X_SRC_FLOW`.
const BITMAP_X_SRC_FLOW: u64 = 1 << SADB_X_EXT_SRC_FLOW;
/// `BITMAP_X_DST_FLOW`.
const BITMAP_X_DST_FLOW: u64 = 1 << SADB_X_EXT_DST_FLOW;
/// `BITMAP_X_FLOW_TYPE`.
const BITMAP_X_FLOW_TYPE: u64 = 1 << SADB_X_EXT_FLOW_TYPE;
/// `BITMAP_X_SA2`.
const BITMAP_X_SA2: u64 = 1 << SADB_X_EXT_SA2;
/// `BITMAP_X_DST2`.
const BITMAP_X_DST2: u64 = 1 << SADB_X_EXT_DST2;
/// `BITMAP_X_POLICY`.
const BITMAP_X_POLICY: u64 = 1 << SADB_X_EXT_POLICY;
/// `BITMAP_X_FLOW`.
const BITMAP_X_FLOW: u64 = BITMAP_X_SRC_MASK
    | BITMAP_X_DST_MASK
    | BITMAP_X_PROTOCOL
    | BITMAP_X_SRC_FLOW
    | BITMAP_X_DST_FLOW
    | BITMAP_X_FLOW_TYPE;
/// `BITMAP_X_SUPPORTED_COMP`.
const BITMAP_X_SUPPORTED_COMP: u64 = 1 << SADB_X_EXT_SUPPORTED_COMP;
/// `BITMAP_X_UDPENCAP`.
const BITMAP_X_UDPENCAP: u64 = 1 << SADB_X_EXT_UDPENCAP;
/// `BITMAP_X_LIFETIME_LASTUSE`.
const BITMAP_X_LIFETIME_LASTUSE: u64 = 1 << SADB_X_EXT_LIFETIME_LASTUSE;
/// `BITMAP_X_TAG`.
const BITMAP_X_TAG: u64 = 1 << SADB_X_EXT_TAG;
/// `BITMAP_X_TAP`.
const BITMAP_X_TAP: u64 = 1 << SADB_X_EXT_TAP;
/// `BITMAP_X_SATYPE2`.
const BITMAP_X_SATYPE2: u64 = 1 << SADB_X_EXT_SATYPE2;
/// `BITMAP_X_RDOMAIN`.
const BITMAP_X_RDOMAIN: u64 = 1 << SADB_X_EXT_RDOMAIN;
/// `BITMAP_X_COUNTER`.
const BITMAP_X_COUNTER: u64 = 1 << SADB_X_EXT_COUNTER;
/// `BITMAP_X_MTU`.
const BITMAP_X_MTU: u64 = 1 << SADB_X_EXT_MTU;
/// `BITMAP_X_REPLAY`.
const BITMAP_X_REPLAY: u64 = 1 << SADB_X_EXT_REPLAY;
/// `BITMAP_X_IFACE`.
const BITMAP_X_IFACE: u64 = 1 << SADB_X_EXT_IFACE;

/// `sadb_exts_allowed_in`: the extensions each message type may carry from userland.
pub static SADB_EXTS_ALLOWED_IN: [u64; SADB_MAX + 1] = [
    // RESERVED
    !0,
    // GETSPI
    BITMAP_ADDRESS_SRC | BITMAP_ADDRESS_DST | BITMAP_SPIRANGE,
    // UPDATE
    BITMAP_SA
        | BITMAP_LIFETIME
        | BITMAP_ADDRESS
        | BITMAP_ADDRESS_PROXY
        | BITMAP_KEY
        | BITMAP_IDENTITY
        | BITMAP_X_FLOW
        | BITMAP_X_UDPENCAP
        | BITMAP_X_TAG
        | BITMAP_X_TAP
        | BITMAP_X_RDOMAIN
        | BITMAP_X_COUNTER
        | BITMAP_X_REPLAY
        | BITMAP_X_IFACE,
    // ADD
    BITMAP_SA
        | BITMAP_LIFETIME
        | BITMAP_ADDRESS
        | BITMAP_KEY
        | BITMAP_IDENTITY
        | BITMAP_X_FLOW
        | BITMAP_X_UDPENCAP
        | BITMAP_X_LIFETIME_LASTUSE
        | BITMAP_X_TAG
        | BITMAP_X_TAP
        | BITMAP_X_RDOMAIN
        | BITMAP_X_COUNTER
        | BITMAP_X_REPLAY
        | BITMAP_X_IFACE,
    // DELETE
    BITMAP_SA | BITMAP_ADDRESS_SRC | BITMAP_ADDRESS_DST | BITMAP_X_RDOMAIN,
    // GET
    BITMAP_SA | BITMAP_ADDRESS_SRC | BITMAP_ADDRESS_DST | BITMAP_X_RDOMAIN,
    // ACQUIRE
    BITMAP_ADDRESS_SRC | BITMAP_ADDRESS_DST | BITMAP_IDENTITY | BITMAP_PROPOSAL,
    // REGISTER
    0,
    // EXPIRE
    BITMAP_SA | BITMAP_ADDRESS_SRC | BITMAP_ADDRESS_DST,
    // FLUSH
    0,
    // DUMP
    0,
    // X_PROMISC
    0,
    // X_ADDFLOW
    BITMAP_ADDRESS_SRC
        | BITMAP_ADDRESS_DST
        | BITMAP_IDENTITY_SRC
        | BITMAP_IDENTITY_DST
        | BITMAP_X_FLOW
        | BITMAP_X_RDOMAIN,
    // X_DELFLOW
    BITMAP_X_FLOW | BITMAP_X_RDOMAIN,
    // X_GRPSPIS
    BITMAP_SA
        | BITMAP_X_SA2
        | BITMAP_X_DST2
        | BITMAP_ADDRESS_DST
        | BITMAP_X_SATYPE2
        | BITMAP_X_RDOMAIN,
    // X_ASKPOLICY
    BITMAP_X_POLICY,
    // X_SPDDUMP
    0,
];

/// `sadb_exts_required_in`: the extensions each message type must carry from userland.
pub static SADB_EXTS_REQUIRED_IN: [u64; SADB_MAX + 1] = [
    // RESERVED
    0,
    // GETSPI
    BITMAP_ADDRESS_SRC | BITMAP_ADDRESS_DST | BITMAP_SPIRANGE,
    // UPDATE
    BITMAP_SA | BITMAP_ADDRESS_SRC | BITMAP_ADDRESS_DST,
    // ADD
    BITMAP_SA | BITMAP_ADDRESS_DST,
    // DELETE
    BITMAP_SA | BITMAP_ADDRESS_DST,
    // GET
    BITMAP_SA | BITMAP_ADDRESS_DST,
    // ACQUIRE
    0,
    // REGISTER
    0,
    // EXPIRE
    BITMAP_SA | BITMAP_ADDRESS_SRC | BITMAP_ADDRESS_DST,
    // FLUSH
    0,
    // DUMP
    0,
    // X_PROMISC
    0,
    // X_ADDFLOW
    BITMAP_X_SRC_MASK
        | BITMAP_X_DST_MASK
        | BITMAP_X_SRC_FLOW
        | BITMAP_X_DST_FLOW
        | BITMAP_X_FLOW_TYPE,
    // X_DELFLOW
    BITMAP_X_SRC_MASK
        | BITMAP_X_DST_MASK
        | BITMAP_X_SRC_FLOW
        | BITMAP_X_DST_FLOW
        | BITMAP_X_FLOW_TYPE,
    // X_GRPSPIS
    BITMAP_SA | BITMAP_X_SA2 | BITMAP_X_DST2 | BITMAP_ADDRESS_DST | BITMAP_X_SATYPE2,
    // X_ASKPOLICY
    BITMAP_X_POLICY,
    // X_SPDDUMP
    0,
];

/// `sadb_exts_allowed_out`: the extensions a reply of each type may carry.
pub static SADB_EXTS_ALLOWED_OUT: [u64; SADB_MAX + 1] = [
    // RESERVED
    !0,
    // GETSPI
    BITMAP_SA | BITMAP_ADDRESS_SRC | BITMAP_ADDRESS_DST,
    // UPDATE
    BITMAP_SA
        | BITMAP_LIFETIME
        | BITMAP_ADDRESS
        | BITMAP_ADDRESS_PROXY
        | BITMAP_IDENTITY
        | BITMAP_X_FLOW
        | BITMAP_X_UDPENCAP
        | BITMAP_X_TAG
        | BITMAP_X_TAP
        | BITMAP_X_RDOMAIN
        | BITMAP_X_IFACE,
    // ADD
    BITMAP_SA
        | BITMAP_LIFETIME
        | BITMAP_ADDRESS
        | BITMAP_IDENTITY
        | BITMAP_X_FLOW
        | BITMAP_X_UDPENCAP
        | BITMAP_X_TAG
        | BITMAP_X_TAP
        | BITMAP_X_RDOMAIN
        | BITMAP_X_IFACE,
    // DELETE
    BITMAP_SA | BITMAP_ADDRESS_SRC | BITMAP_ADDRESS_DST | BITMAP_X_RDOMAIN,
    // GET
    BITMAP_SA
        | BITMAP_LIFETIME
        | BITMAP_ADDRESS
        | BITMAP_KEY
        | BITMAP_IDENTITY
        | BITMAP_X_UDPENCAP
        | BITMAP_X_LIFETIME_LASTUSE
        | BITMAP_X_SRC_MASK
        | BITMAP_X_DST_MASK
        | BITMAP_X_PROTOCOL
        | BITMAP_X_FLOW_TYPE
        | BITMAP_X_SRC_FLOW
        | BITMAP_X_DST_FLOW
        | BITMAP_X_TAG
        | BITMAP_X_TAP
        | BITMAP_X_COUNTER
        | BITMAP_X_RDOMAIN
        | BITMAP_X_MTU
        | BITMAP_X_REPLAY
        | BITMAP_X_IFACE,
    // ACQUIRE
    BITMAP_ADDRESS_SRC | BITMAP_ADDRESS_DST | BITMAP_IDENTITY | BITMAP_PROPOSAL,
    // REGISTER
    BITMAP_SUPPORTED_AUTH | BITMAP_SUPPORTED_ENCRYPT | BITMAP_X_SUPPORTED_COMP,
    // EXPIRE
    BITMAP_SA | BITMAP_LIFETIME | BITMAP_ADDRESS,
    // FLUSH
    0,
    // DUMP
    BITMAP_SA | BITMAP_LIFETIME | BITMAP_ADDRESS | BITMAP_IDENTITY,
    // X_PROMISC
    0,
    // X_ADDFLOW
    BITMAP_ADDRESS_SRC
        | BITMAP_ADDRESS_DST
        | BITMAP_X_SRC_MASK
        | BITMAP_X_DST_MASK
        | BITMAP_X_PROTOCOL
        | BITMAP_X_SRC_FLOW
        | BITMAP_X_DST_FLOW
        | BITMAP_X_FLOW_TYPE
        | BITMAP_IDENTITY_SRC
        | BITMAP_IDENTITY_DST
        | BITMAP_X_RDOMAIN,
    // X_DELFLOW
    BITMAP_X_SRC_MASK
        | BITMAP_X_DST_MASK
        | BITMAP_X_PROTOCOL
        | BITMAP_X_SRC_FLOW
        | BITMAP_X_DST_FLOW
        | BITMAP_X_FLOW_TYPE
        | BITMAP_X_RDOMAIN,
    // X_GRPSPIS
    BITMAP_SA
        | BITMAP_X_SA2
        | BITMAP_X_DST2
        | BITMAP_ADDRESS_DST
        | BITMAP_X_SATYPE2
        | BITMAP_X_RDOMAIN,
    // X_ASKPOLICY
    BITMAP_X_SRC_FLOW
        | BITMAP_X_DST_FLOW
        | BITMAP_X_SRC_MASK
        | BITMAP_X_DST_MASK
        | BITMAP_X_FLOW_TYPE
        | BITMAP_X_POLICY,
    // X_SPDDUMP (the C's initializer stops at X_ASKPOLICY)
    0,
];

/// `sadb_exts_required_out`: the extensions a reply of each type must carry.
pub static SADB_EXTS_REQUIRED_OUT: [u64; SADB_MAX + 1] = [
    // RESERVED
    0,
    // GETSPI
    BITMAP_SA | BITMAP_ADDRESS_DST,
    // UPDATE
    BITMAP_SA | BITMAP_ADDRESS_DST,
    // ADD
    BITMAP_SA | BITMAP_ADDRESS_DST,
    // DELETE
    BITMAP_SA | BITMAP_ADDRESS_DST,
    // GET
    BITMAP_SA | BITMAP_LIFETIME_CURRENT | BITMAP_ADDRESS_DST,
    // ACQUIRE
    0,
    // REGISTER
    BITMAP_SUPPORTED_AUTH | BITMAP_SUPPORTED_ENCRYPT | BITMAP_X_SUPPORTED_COMP,
    // EXPIRE
    BITMAP_SA | BITMAP_ADDRESS_DST,
    // FLUSH
    0,
    // DUMP
    0,
    // X_PROMISC
    0,
    // X_ADDFLOW
    BITMAP_X_SRC_MASK
        | BITMAP_X_DST_MASK
        | BITMAP_X_SRC_FLOW
        | BITMAP_X_DST_FLOW
        | BITMAP_X_FLOW_TYPE,
    // X_DELFLOW
    BITMAP_X_SRC_MASK
        | BITMAP_X_DST_MASK
        | BITMAP_X_SRC_FLOW
        | BITMAP_X_DST_FLOW
        | BITMAP_X_FLOW_TYPE,
    // X_GRPSPIS
    BITMAP_SA | BITMAP_X_SA2 | BITMAP_X_DST2 | BITMAP_ADDRESS_DST | BITMAP_X_SATYPE2,
    // X_REPPOLICY
    BITMAP_X_SRC_FLOW
        | BITMAP_X_DST_FLOW
        | BITMAP_X_SRC_MASK
        | BITMAP_X_DST_MASK
        | BITMAP_X_FLOW_TYPE,
    // X_SPDDUMP (the C's initializer stops before it)
    0,
];

/// Reads a `T` at `off` of `msg`, `None` when it does not fit.
fn get<T: Copy>(msg: &[u8], off: usize) -> Option<T> {
    if off.checked_add(size_of::<T>())? > msg.len() {
        return None;
    }
    // SAFETY: `size_of::<T>()` bytes at `off` are inside `msg` (checked above); `T` is one of
    // the ABI structures, integers valid for any bytes.
    Some(unsafe { sadb_get(msg.as_ptr().add(off)) })
}

/// `pfkeyv2_parsemessage`: validate the message `p` and point `headers[]` at its header and
/// extensions.
pub fn pfkeyv2_parsemessage(p: &mut [u8], headers: &mut SadbHeaders) -> Result<(), Errno> {
    let len = p.len();
    let mut left = len;
    let mut seen: u64 = BITMAP_MSG;

    *headers = sadb_headers_new();

    let Some(sadb_msg) = get::<SadbMsg>(p, 0) else {
        dprintf!("message too short");
        return Err(Errno::EINVAL);
    };

    headers[0] = p.as_mut_ptr();

    if usize::from(sadb_msg.sadb_msg_len) * size_of::<u64>() != left {
        dprintf!("length not a multiple of 64");
        return Err(Errno::EINVAL);
    }

    let mut off = size_of::<SadbMsg>();
    left -= size_of::<SadbMsg>();

    if sadb_msg.sadb_msg_reserved != 0 {
        dprintf!("message header reserved field set");
        return Err(Errno::EINVAL);
    }

    if usize::from(sadb_msg.sadb_msg_type) > SADB_MAX {
        dprintf!("message type > {}", SADB_MAX);
        return Err(Errno::EINVAL);
    }

    if sadb_msg.sadb_msg_type == 0 {
        dprintf!("message type unset");
        return Err(Errno::EINVAL);
    }

    let pid = curproc().map_or(0, |cp| cp.process().ps_pid.get());
    if sadb_msg.sadb_msg_pid != pid as u32 {
        dprintf!("bad PID value");
        return Err(Errno::EINVAL);
    }

    if sadb_msg.sadb_msg_errno != 0 {
        dprintf!("errno set");
        return Err(Errno::EINVAL);
    }

    let allow = SADB_EXTS_ALLOWED_IN[usize::from(sadb_msg.sadb_msg_type)];

    while left > 0 {
        let Some(sadb_ext) = get::<SadbExt>(p, off) else {
            dprintf!("extension header too short");
            return Err(Errno::EINVAL);
        };

        let i = usize::from(sadb_ext.sadb_ext_len) * size_of::<u64>();
        if left < i {
            dprintf!("extension header exceeds message length");
            return Err(Errno::EINVAL);
        }
        let ext = &p[off..off + i];
        let t = sadb_ext.sadb_ext_type;

        if usize::from(t) > SADB_EXT_MAX {
            dprintf!("unknown extension header {}", t);
            return Err(Errno::EINVAL);
        }

        if t == 0 {
            dprintf!("unset extension header");
            return Err(Errno::EINVAL);
        }

        if allow & (1u64 << t) == 0 {
            dprintf!(
                "extension header {} not permitted on message type {}",
                t,
                sadb_msg.sadb_msg_type
            );
            return Err(Errno::EINVAL);
        }

        if !headers[usize::from(t)].is_null() {
            dprintf!("duplicate extension header {}", t);
            return Err(Errno::EINVAL);
        }

        seen |= 1u64 << t;

        parse_extension(&sadb_msg, t, ext)?;

        headers[usize::from(t)] = p[off..].as_mut_ptr();
        off += i;
        left -= i;
    }

    if left != 0 {
        dprintf!("message too long");
        return Err(Errno::EINVAL);
    }

    {
        let required = SADB_EXTS_REQUIRED_IN[usize::from(sadb_msg.sadb_msg_type)];

        if seen & required != required {
            dprintf!("required fields missing");
            return Err(Errno::EINVAL);
        }
    }

    match sadb_msg.sadb_msg_type {
        SADB_UPDATE | SADB_ADD => {
            // SADB_EXT_SA is required for both, so it is there.
            let sa_off = headers[usize::from(SADB_EXT_SA)] as usize - p.as_ptr() as usize;
            let state = get::<SadbSa>(p, sa_off).map_or(0, |s| s.sadb_sa_state);
            if state != SADB_SASTATE_MATURE {
                if sadb_msg.sadb_msg_type == SADB_UPDATE {
                    dprintf!("updating non-mature SA prohibited");
                } else {
                    dprintf!("adding non-mature SA prohibited");
                }
                return Err(Errno::EINVAL);
            }
        }
        _ => {}
    }

    Ok(())
}

/// The checks of one extension of type `t` (`ext`, its `sadb_ext_len` bytes).
fn parse_extension(sadb_msg: &SadbMsg, t: u16, ext: &[u8]) -> Result<(), Errno> {
    let i = ext.len();
    match t {
        SADB_EXT_SA | SADB_X_EXT_SA2 => {
            if i != size_of::<SadbSa>() {
                dprintf!("bad header length for SA extension header {}", t);
                return Err(Errno::EINVAL);
            }
            let Some(sadb_sa) = get::<SadbSa>(ext, 0) else {
                return Err(Errno::EINVAL);
            };

            if sadb_sa.sadb_sa_state > SADB_SASTATE_MAX {
                dprintf!(
                    "unknown SA state {} in SA extension header {}",
                    sadb_sa.sadb_sa_state,
                    t
                );
                return Err(Errno::EINVAL);
            }

            if sadb_sa.sadb_sa_state == SADB_SASTATE_DEAD {
                dprintf!("cannot set SA state to dead, SA extension header {}", t);
                return Err(Errno::EINVAL);
            }

            if sadb_sa.sadb_sa_encrypt > SADB_EALG_MAX {
                dprintf!(
                    "unknown encryption algorithm {} in SA extension header {}",
                    sadb_sa.sadb_sa_encrypt,
                    t
                );
                return Err(Errno::EINVAL);
            }

            if sadb_sa.sadb_sa_auth > SADB_AALG_MAX {
                dprintf!(
                    "unknown authentication algorithm {} in SA extension header {}",
                    sadb_sa.sadb_sa_auth,
                    t
                );
                return Err(Errno::EINVAL);
            }

            if sadb_sa.sadb_sa_replay > 64 {
                dprintf!(
                    "unsupported replay window size {} in SA extension header {}",
                    sadb_sa.sadb_sa_replay,
                    t
                );
                return Err(Errno::EINVAL);
            }
        }
        SADB_X_EXT_PROTOCOL | SADB_X_EXT_FLOW_TYPE | SADB_X_EXT_SATYPE2 => {
            if i != size_of::<SadbProtocol>() {
                dprintf!(
                    "bad PROTOCOL/FLOW/SATYPE2 header length in extension header {}",
                    t
                );
                return Err(Errno::EINVAL);
            }
        }
        SADB_X_EXT_POLICY => {
            if i != size_of::<SadbXPolicy>() {
                dprintf!("bad POLICY header length");
                return Err(Errno::EINVAL);
            }
        }
        SADB_EXT_LIFETIME_CURRENT
        | SADB_EXT_LIFETIME_HARD
        | SADB_EXT_LIFETIME_SOFT
        | SADB_X_EXT_LIFETIME_LASTUSE => {
            if i != size_of::<SadbLifetime>() {
                dprintf!("bad header length for LIFETIME extension header {}", t);
                return Err(Errno::EINVAL);
            }
        }
        SADB_EXT_ADDRESS_SRC
        | SADB_EXT_ADDRESS_DST
        | SADB_EXT_ADDRESS_PROXY
        | SADB_X_EXT_SRC_MASK
        | SADB_X_EXT_DST_MASK
        | SADB_X_EXT_SRC_FLOW
        | SADB_X_EXT_DST_FLOW
        | SADB_X_EXT_DST2 => {
            if i < size_of::<SadbAddress>() + size_of::<Sockaddr>() {
                dprintf!("bad ADDRESS extension header {} length", t);
                return Err(Errno::EINVAL);
            }
            let (Some(sadb_address), Some(sa)) = (
                get::<SadbAddress>(ext, 0),
                get::<Sockaddr>(ext, size_of::<SadbAddress>()),
            ) else {
                return Err(Errno::EINVAL);
            };

            if sadb_address.sadb_address_reserved != 0 {
                dprintf!("ADDRESS extension header {} reserved field set", t);
                return Err(Errno::EINVAL);
            }
            if sa.sa_len != 0 && i != size_of::<SadbAddress>() + padup(usize::from(sa.sa_len)) {
                dprintf!(
                    "bad sockaddr length field in ADDRESS extension header {}",
                    t
                );
                return Err(Errno::EINVAL);
            }

            match sa.sa_family {
                AF_INET => {
                    if size_of::<SadbAddress>() + padup(size_of::<SockaddrIn>()) != i {
                        dprintf!("invalid ADDRESS extension header {} length", t);
                        return Err(Errno::EINVAL);
                    }

                    if usize::from(sa.sa_len) != size_of::<SockaddrIn>() {
                        dprintf!("bad sockaddr_in length in ADDRESS extension header {}", t);
                        return Err(Errno::EINVAL);
                    }
                    let Some(sin) = get::<SockaddrIn>(ext, size_of::<SadbAddress>()) else {
                        return Err(Errno::EINVAL);
                    };

                    // Only check the right pieces
                    match t {
                        SADB_X_EXT_SRC_MASK | SADB_X_EXT_DST_MASK | SADB_X_EXT_SRC_FLOW
                        | SADB_X_EXT_DST_FLOW => {}
                        _ => {
                            if sin.sin_port != 0 {
                                dprintf!(
                                    "port field set in sockaddr_in of ADDRESS extension header {}",
                                    t
                                );
                                return Err(Errno::EINVAL);
                            }
                        }
                    }

                    if sin.sin_zero != [0; 8] {
                        dprintf!(
                            "reserved sockaddr_in field non-zero'ed in ADDRESS extension \
                             header {}",
                            t
                        );
                        return Err(Errno::EINVAL);
                    }
                }
                #[cfg(feature = "inet6")]
                AF_INET6 => {
                    if size_of::<SadbAddress>() + padup(size_of::<SockaddrIn6>()) != i {
                        dprintf!(
                            "invalid sockaddr_in6 length in ADDRESS extension header {}",
                            t
                        );
                        return Err(Errno::EINVAL);
                    }

                    if usize::from(sa.sa_len) != size_of::<SockaddrIn6>() {
                        dprintf!("bad sockaddr_in6 length in ADDRESS extension header {}", t);
                        return Err(Errno::EINVAL);
                    }
                    let Some(sin6) = get::<SockaddrIn6>(ext, size_of::<SadbAddress>()) else {
                        return Err(Errno::EINVAL);
                    };

                    if sin6.sin6_flowinfo != 0 {
                        dprintf!(
                            "flowinfo field set in sockaddr_in6 of ADDRESS extension header {}",
                            t
                        );
                        return Err(Errno::EINVAL);
                    }

                    // Only check the right pieces
                    match t {
                        SADB_X_EXT_SRC_MASK | SADB_X_EXT_DST_MASK | SADB_X_EXT_SRC_FLOW
                        | SADB_X_EXT_DST_FLOW => {}
                        _ => {
                            if sin6.sin6_port != 0 {
                                dprintf!(
                                    "port field set in sockaddr_in6 of ADDRESS extension header {}",
                                    t
                                );
                                return Err(Errno::EINVAL);
                            }
                        }
                    }
                }
                _ => {
                    if !(sadb_msg.sadb_msg_satype == SADB_X_SATYPE_TCPSIGNATURE
                        && sa.sa_family == 0)
                    {
                        dprintf!(
                            "unknown address family {} in ADDRESS extension header {}",
                            sa.sa_family,
                            t
                        );
                        return Err(Errno::EINVAL);
                    }
                }
            }
        }
        SADB_EXT_KEY_AUTH | SADB_EXT_KEY_ENCRYPT => {
            let Some(sadb_key) = get::<SadbKey>(ext, 0) else {
                dprintf!("bad header length in KEY extension header {}", t);
                return Err(Errno::EINVAL);
            };

            if sadb_key.sadb_key_bits == 0 {
                dprintf!("key length unset in KEY extension header {}", t);
                return Err(Errno::EINVAL);
            }

            if usize::from(sadb_key.sadb_key_bits).div_ceil(64) * size_of::<u64>()
                != i - size_of::<SadbKey>()
            {
                dprintf!("invalid key length in KEY extension header {}", t);
                return Err(Errno::EINVAL);
            }

            if sadb_key.sadb_key_reserved != 0 {
                dprintf!("reserved field set in KEY extension header {}", t);
                return Err(Errno::EINVAL);
            }
        }
        SADB_EXT_IDENTITY_SRC | SADB_EXT_IDENTITY_DST => {
            let Some(sadb_ident) = get::<SadbIdent>(ext, 0) else {
                dprintf!("bad header length of IDENTITY extension header {}", t);
                return Err(Errno::EINVAL);
            };

            if sadb_ident.sadb_ident_type > SADB_IDENTTYPE_MAX {
                dprintf!(
                    "unknown identity type {} in IDENTITY extension header {}",
                    sadb_ident.sadb_ident_type,
                    t
                );
                return Err(Errno::EINVAL);
            }

            if sadb_ident.sadb_ident_reserved != 0 {
                dprintf!("reserved field set in IDENTITY extension header {}", t);
                return Err(Errno::EINVAL);
            }

            if i > size_of::<SadbIdent>() {
                let c = &ext[size_of::<SadbIdent>()..];

                if ext[i - 1] != 0 {
                    dprintf!(
                        "non NUL-terminated identity in IDENTITY extension header {}",
                        t
                    );
                    return Err(Errno::EINVAL);
                }

                let strlen = c.iter().position(|&b| b == 0).unwrap_or(c.len());
                let j = padup(strlen + 1) + size_of::<SadbIdent>();

                if i != j {
                    dprintf!(
                        "actual identity length does not match expected length in identity \
                         extension header {}",
                        t
                    );
                    return Err(Errno::EINVAL);
                }
            }
        }
        SADB_EXT_SENSITIVITY => {
            let Some(sadb_sens) = get::<SadbSens>(ext, 0) else {
                dprintf!("bad header length for SENSITIVITY extension header");
                return Err(Errno::EINVAL);
            };

            if i != (usize::from(sadb_sens.sadb_sens_sens_len)
                + usize::from(sadb_sens.sadb_sens_integ_len))
                * size_of::<u64>()
                + size_of::<SadbSens>()
            {
                dprintf!("bad payload length for SENSITIVITY extension header");
                return Err(Errno::EINVAL);
            }
        }
        SADB_EXT_PROPOSAL => {
            let Some(sadb_prop) = get::<SadbProp>(ext, 0) else {
                dprintf!("bad PROPOSAL header length");
                return Err(Errno::EINVAL);
            };

            if sadb_prop.sadb_prop_reserved != 0 {
                dprintf!("reserved fieldset in PROPOSAL extension header");
                return Err(Errno::EINVAL);
            }

            if !(i - size_of::<SadbProp>()).is_multiple_of(size_of::<SadbComb>()) {
                dprintf!("bad proposal length");
                return Err(Errno::EINVAL);
            }

            // The C checks the first combination as many times as there are: its pointer is
            // never advanced.
            let n = (i - size_of::<SadbProp>()) / size_of::<SadbComb>();
            if n > 0
                && let Some(sadb_comb) = get::<SadbComb>(ext, size_of::<SadbProp>())
            {
                if sadb_comb.sadb_comb_auth > SADB_AALG_MAX {
                    dprintf!(
                        "unknown authentication algorithm {} in PROPOSAL",
                        sadb_comb.sadb_comb_auth
                    );
                    return Err(Errno::EINVAL);
                }

                if sadb_comb.sadb_comb_encrypt > SADB_EALG_MAX {
                    dprintf!(
                        "unknown encryption algorithm {} in PROPOSAL",
                        sadb_comb.sadb_comb_encrypt
                    );
                    return Err(Errno::EINVAL);
                }

                if sadb_comb.sadb_comb_reserved != 0 {
                    dprintf!("reserved field set in COMB header");
                    return Err(Errno::EINVAL);
                }
            }
        }
        SADB_EXT_SUPPORTED_AUTH | SADB_EXT_SUPPORTED_ENCRYPT | SADB_X_EXT_SUPPORTED_COMP => {
            let Some(sadb_supported) = get::<SadbSupported>(ext, 0) else {
                dprintf!("bad header length for SUPPORTED extension header {}", t);
                return Err(Errno::EINVAL);
            };

            if sadb_supported.sadb_supported_reserved != 0 {
                dprintf!("reserved field set in SUPPORTED extension header {}", t);
                return Err(Errno::EINVAL);
            }

            // `SADB_EXT_SUPPORTED_ENCRYPT ? SADB_EALG_MAX : SADB_X_CALG_MAX`: the constant is
            // true (see the deviations).
            let max_alg = if t == SADB_EXT_SUPPORTED_AUTH {
                SADB_AALG_MAX
            } else if SADB_EXT_SUPPORTED_ENCRYPT != 0 {
                SADB_EALG_MAX
            } else {
                SADB_X_CALG_MAX
            };

            let n = usize::from(sadb_supported.sadb_supported_len).saturating_sub(1);
            for j in 0..n {
                let Some(sadb_alg) =
                    get::<SadbAlg>(ext, size_of::<SadbSupported>() + j * size_of::<SadbAlg>())
                else {
                    return Err(Errno::EINVAL);
                };
                if sadb_alg.sadb_alg_id > max_alg {
                    dprintf!(
                        "unknown algorithm {} in SUPPORTED extension header {}",
                        sadb_alg.sadb_alg_id,
                        t
                    );
                    return Err(Errno::EINVAL);
                }

                if sadb_alg.sadb_alg_reserved != 0 {
                    dprintf!(
                        "reserved field set in supported algorithms header inside \
                         SUPPORTED extension header {}",
                        t
                    );
                    return Err(Errno::EINVAL);
                }
            }
        }
        SADB_EXT_SPIRANGE => {
            if i != size_of::<SadbSpirange>() {
                dprintf!("bad header length of SPIRANGE extension header");
                return Err(Errno::EINVAL);
            }
            let Some(sadb_spirange) = get::<SadbSpirange>(ext, 0) else {
                return Err(Errno::EINVAL);
            };

            if sadb_spirange.sadb_spirange_min > sadb_spirange.sadb_spirange_max {
                dprintf!("bad SPI range");
                return Err(Errno::EINVAL);
            }
        }
        SADB_X_EXT_UDPENCAP => {
            if i != size_of::<SadbXUdpencap>() {
                dprintf!("bad UDPENCAP header length");
                return Err(Errno::EINVAL);
            }
        }
        SADB_X_EXT_RDOMAIN => {
            if i != size_of::<SadbXRdomain>() {
                dprintf!("bad RDOMAIN header length");
                return Err(Errno::EINVAL);
            }
        }
        SADB_X_EXT_REPLAY => {
            if i != size_of::<SadbXReplay>() {
                dprintf!("bad REPLAY header length");
                return Err(Errno::EINVAL);
            }
        }
        SADB_X_EXT_COUNTER => {
            if i != size_of::<SadbXCounter>() {
                dprintf!("bad COUNTER header length");
                return Err(Errno::EINVAL);
            }
        }
        SADB_X_EXT_TAG => {
            if i < size_of::<SadbXTag>() {
                dprintf!("TAG extension header too small");
                return Err(Errno::EINVAL);
            }
            if i > size_of::<SadbXTag>() + PF_TAG_NAME_SIZE {
                dprintf!("TAG extension header too long");
                return Err(Errno::EINVAL);
            }
        }
        SADB_X_EXT_TAP => {
            if i < size_of::<SadbXTap>() {
                dprintf!("TAP extension header too small");
                return Err(Errno::EINVAL);
            }
            if i > size_of::<SadbXTap>() {
                dprintf!("TAP extension header too long");
                return Err(Errno::EINVAL);
            }
        }
        SADB_X_EXT_IFACE => {
            if i != size_of::<SadbXIface>() {
                dprintf!("bad IFACE header length");
                return Err(Errno::EINVAL);
            }
        }
        _ => {
            dprintf!("unknown extension header type {}", t);
            return Err(Errno::EINVAL);
        }
    }
    Ok(())
}

const _: () = assert!(offset_of!(SadbExt, sadb_ext_type) == 2);
/* </CODE> */
