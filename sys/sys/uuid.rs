/*	$OpenBSD: uuid.h,v 1.5 2025/07/11 19:12:49 krw Exp $	*/
/*	$NetBSD: uuid.h,v 1.5 2008/11/18 14:01:03 joerg Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2002 Marcel Moolenaar
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 *
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
 *
 * $FreeBSD: /repoman/r/ncvs/src/sys/sys/uuid.h,v 1.3 2003/05/31 16:47:07 phk Exp $
 */
/* </LICENSES> */

//! `<sys/uuid.h>`: a DCE 1.1 compatible source representation of UUIDs.
//!
//! Upstream: sys/sys/uuid.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `uuid_snprintf` and `uuid_printf` (`kern/kern_uuid.c`) are not ported: nothing calls
//!   them yet.
//! - [`Uuid::from_bytes`]/[`Uuid::as_bytes`] move a UUID through its in-memory bytes (the
//!   C's `memcpy`/`memcmp`), in the host's byte order, as the C's struct is.

/// `_UUID_NODE_LEN`: length of a node address (an IEEE 802 address).
pub const UUID_NODE_LEN: usize = 6;

/// `_UUID_BUF_LEN`: length of a printed UUID.
pub const UUID_BUF_LEN: usize = 38;

/// `struct uuid`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Uuid {
    /// `time_low`.
    pub time_low: u32,
    /// `time_mid`.
    pub time_mid: u16,
    /// `time_hi_and_version`.
    pub time_hi_and_version: u16,
    /// `clock_seq_hi_and_reserved`.
    pub clock_seq_hi_and_reserved: u8,
    /// `clock_seq_low`.
    pub clock_seq_low: u8,
    /// `node`.
    pub node: [u8; UUID_NODE_LEN],
}

impl Uuid {
    /// The UUID whose in-memory bytes are `b`.
    pub const fn from_bytes(b: [u8; 16]) -> Self {
        Self {
            time_low: u32::from_ne_bytes([b[0], b[1], b[2], b[3]]),
            time_mid: u16::from_ne_bytes([b[4], b[5]]),
            time_hi_and_version: u16::from_ne_bytes([b[6], b[7]]),
            clock_seq_hi_and_reserved: b[8],
            clock_seq_low: b[9],
            node: [b[10], b[11], b[12], b[13], b[14], b[15]],
        }
    }

    /// The UUID's in-memory bytes.
    pub const fn as_bytes(&self) -> [u8; 16] {
        let a = self.time_low.to_ne_bytes();
        let m = self.time_mid.to_ne_bytes();
        let h = self.time_hi_and_version.to_ne_bytes();
        let n = self.node;
        [
            a[0],
            a[1],
            a[2],
            a[3],
            m[0],
            m[1],
            h[0],
            h[1],
            self.clock_seq_hi_and_reserved,
            self.clock_seq_low,
            n[0],
            n[1],
            n[2],
            n[3],
            n[4],
            n[5],
        ]
    }
}

const _: () = assert!(size_of::<Uuid>() == 16);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_round_trip() {
        let b: [u8; 16] = core::array::from_fn(|i| i as u8 * 7 + 1);
        assert_eq!(Uuid::from_bytes(b).as_bytes(), b);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/sys/uuid.h");
        assert_eq!(
            crate::reftest::int(&defs, "_UUID_NODE_LEN"),
            Some(UUID_NODE_LEN as i64)
        );
        assert_eq!(
            crate::reftest::int(&defs, "_UUID_BUF_LEN"),
            Some(UUID_BUF_LEN as i64)
        );
    }
}
