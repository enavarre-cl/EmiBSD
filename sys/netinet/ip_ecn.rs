/*	$KAME: ip_ecn.h,v 1.5 2000/03/27 04:58:38 sumikawa Exp $	*/
/*	$KAME: ip_ecn.c,v 1.9 2000/10/01 12:44:48 itojun Exp $	*/
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
 * Copyright (C) 1999 WIDE Project.
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
 *
 */

/*
 * Copyright (C) 1999 WIDE Project.
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
 *
 */
/* </LICENSES> */

/* <CODE> */
//! ECN consideration on tunnel ingress/egress operation
//! (<http://www.aciri.org/floyd/papers/draft-ipsec-ecn-00.txt>): `<netinet/ip_ecn.h>` and
//! `netinet/ip_ecn.c`.
//!
//! Upstream: sys/netinet/ip_ecn.h @ 3ce1f3f79392
//! Upstream: sys/netinet/ip_ecn.c @ 3ce1f3f79392
//!
//! ECN and TOS (or TCLASS) processing rules at tunnel encapsulation and decapsulation from
//! RFC3168:
//!
//! ```text
//!                      Outer Hdr at                 Inner Hdr at
//!                      Encapsulator                 Decapsulator
//!   Header fields:     --------------------         ------------
//!     DS Field         copied from inner hdr        no change
//!     ECN Field        constructed by (I)           constructed by (E)
//! ```
//!
//! `ECN_ALLOWED` (full functionality): (I) if the ECN field in the inner header is set to CE,
//! then set the ECN field in the outer header to ECT(0), otherwise copy the ECN field to the
//! outer header. (E) if the ECN field in the outer header is set to CE and the ECN field of
//! the inner header is not-ECT, drop the packet; if the ECN field in the inner header is set
//! to ECT(0) or ECT(1) and the ECN field in the outer header is set to CE, then copy CE to the
//! inner header; otherwise, make no change to the inner header.
//!
//! `ECN_FORBIDDEN` (limited functionality): (I) set the ECN field to not-ECT in the outer
//! header. (E) if the ECN field in the outer header is set to CE, drop the packet; otherwise,
//! make no change to the ECN field in the inner header.
//!
//! The drop rule is for backward compatibility and protection against erasure of CE.
//!
//! Status: `ported` (M9c).
//!
//! ## Deviations
//! - The `u_int8_t *outer, *inner` arguments are references, so the C's "NULL pointer passed"
//!   panics cannot happen; `ip_ecn_egress` answers `bool` (`true` for the C's 1, keep the
//!   packet).
//! - `ip_tos_patch` takes the IP header as a mutable copy (`ip_var.rs`'s `mtod_ip`), which the
//!   caller stores back.

use crate::netinet::ip::{IPTOS_ECN_CE, IPTOS_ECN_ECT1, IPTOS_ECN_MASK, IPTOS_ECN_NOTECT, Ip};
use crate::sys::endian::htons;

/// `ECN_ALLOWED_IPSEC`: ECN allowed.
pub const ECN_ALLOWED_IPSEC: i32 = 2;
/// `ECN_ALLOWED`: ECN allowed.
pub const ECN_ALLOWED: i32 = 1;
/// `ECN_FORBIDDEN`: ECN forbidden.
pub const ECN_FORBIDDEN: i32 = 0;
/// `ECN_NOCARE`: no consideration to ECN.
pub const ECN_NOCARE: i32 = -1;

/// `ip_ecn_ingress`: modify outer ECN (TOS) field on ingress operation (tunnel
/// encapsulation). Call it after you've done the default initialization/copy for the outer.
pub fn ip_ecn_ingress(mode: i32, outer: &mut u8, inner: &u8) {
    *outer = *inner;
    match mode {
        // ECN allowed
        ECN_ALLOWED | ECN_ALLOWED_IPSEC => {
            // full-functionality: if the inner is CE, set ECT(0) to the outer. otherwise,
            // copy the ECN field.
            if *inner & IPTOS_ECN_MASK == IPTOS_ECN_CE {
                *outer &= !IPTOS_ECN_ECT1;
            }
        }
        // ECN forbidden
        ECN_FORBIDDEN => {
            // limited-functionality: set not-ECT to the outer
            *outer &= !IPTOS_ECN_MASK;
        }
        // ECN_NOCARE: no consideration to ECN
        _ => {}
    }
}

/// `ip_ecn_egress`: modify inner ECN (TOS) field on egress operation (tunnel decapsulation).
/// Call it after you've done the default initialization/copy for the inner. The caller
/// should drop the packet if the return value is `false`.
pub fn ip_ecn_egress(mode: i32, outer: &u8, inner: &mut u8) -> bool {
    match mode {
        ECN_ALLOWED | ECN_ALLOWED_IPSEC => {
            // full-functionality: if the outer is CE and the inner is not-ECT, should drop
            // it. otherwise, copy CE. However, according to RFC4301, we should just leave the
            // inner as non-ECT for IPsec.
            if *outer & IPTOS_ECN_MASK == IPTOS_ECN_CE {
                if *inner & IPTOS_ECN_MASK == IPTOS_ECN_NOTECT {
                    return mode == ECN_ALLOWED_IPSEC;
                }
                *inner |= IPTOS_ECN_CE;
            }
        }
        // ECN forbidden: limited-functionality: if the outer is CE, should drop it.
        // otherwise, leave the inner.
        ECN_FORBIDDEN if *outer & IPTOS_ECN_MASK == IPTOS_ECN_CE => return false,
        // ECN_NOCARE: no consideration to ECN
        _ => {}
    }
    true
}

/// `ip_tos_patch`: patch the checksum with the difference between the old and new tos. The
/// patching is based on what `pf_patch_8()` and `pf_cksum_fixkup()` do, but they're in pf, so
/// we can't rely on them being available.
pub fn ip_tos_patch(ip: &mut Ip, tos: u8) {
    let old = htons(u16::from(ip.ip_tos));
    let new = htons(u16::from(tos));

    ip.ip_tos = tos;

    let x: u32 = u32::from(ip.ip_sum)
        .wrapping_add(u32::from(old))
        .wrapping_sub(u32::from(new));
    ip.ip_sum = x.wrapping_add(x >> 16) as u16;
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::netinet::ip::IPTOS_ECN_ECT0;

    #[test]
    fn ingress_and_egress() {
        let mut outer = 0;
        ip_ecn_ingress(ECN_ALLOWED, &mut outer, &(0x10 | IPTOS_ECN_CE));
        assert_eq!(outer, 0x10 | IPTOS_ECN_ECT0);
        ip_ecn_ingress(ECN_FORBIDDEN, &mut outer, &(0x10 | IPTOS_ECN_CE));
        assert_eq!(outer, 0x10);

        let mut inner = IPTOS_ECN_NOTECT;
        assert!(!ip_ecn_egress(ECN_ALLOWED, &IPTOS_ECN_CE, &mut inner));
        assert!(ip_ecn_egress(ECN_ALLOWED_IPSEC, &IPTOS_ECN_CE, &mut inner));
        assert_eq!(inner, IPTOS_ECN_NOTECT);
        let mut inner = IPTOS_ECN_ECT0;
        assert!(ip_ecn_egress(ECN_ALLOWED, &IPTOS_ECN_CE, &mut inner));
        assert_eq!(inner, IPTOS_ECN_CE);
        assert!(!ip_ecn_egress(ECN_FORBIDDEN, &IPTOS_ECN_CE, &mut inner));
    }

    /// The Internet checksum of a 20-byte header (0 when its `ip_sum` is right).
    fn in_cksum_buf(ip: &Ip) -> u16 {
        // SAFETY: `Ip` is 20 bytes of integers in a `#[repr(C)]` structure without padding.
        let b: [u8; 20] = unsafe { core::mem::transmute(*ip) };
        let mut sum: u32 = b
            .chunks(2)
            .map(|w| u32::from(u16::from_ne_bytes([w[0], w[1]])))
            .sum();
        while sum >> 16 != 0 {
            sum = (sum & 0xffff) + (sum >> 16);
        }
        !(sum as u16)
    }

    #[test]
    fn tos_patch_keeps_the_checksum_valid() {
        let mut ip = Ip {
            ip_tos: 0x10,
            ip_len: htons(20),
            ip_ttl: 64,
            ip_p: 1,
            ..Ip::default()
        };
        ip.set_ip_v(4);
        ip.set_ip_hl(5);
        ip.ip_sum = in_cksum_buf(&ip);
        ip_tos_patch(&mut ip, 0x11);
        assert_eq!(ip.ip_tos, 0x11);
        assert_eq!(in_cksum_buf(&ip), 0);
    }
}
/* </TESTS> */
