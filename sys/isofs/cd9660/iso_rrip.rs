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

/*-
 * Copyright (c) 1993, 1994
 *	The Regents of the University of California.  All rights reserved.
 *
 * This code is derived from software contributed to Berkeley
 * by Pace Willisson (pace@blitz.com).  The Rock Ridge Extension
 * Support code is derived from software contributed to Berkeley
 * by Atsushi Murai (amurai@spec.co.jp).
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the University nor the names of its contributors
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
 *
 *	@(#)iso_rrip.h	8.2 (Berkeley) 1/23/94
 */
/* </LICENSES> */

/* <CODE> */
//! `<isofs/cd9660/iso_rrip.h>`: the Rock Ridge analysis flags (`ISO_SUSP_*`, which System
//! Use Sharing Protocol entries an analysis still looks for or found) and the analysis state
//! (`ISO_RRIP_ANALYZE`).
//!
//! Upstream: sys/isofs/cd9660/iso_rrip.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `ISO_RRIP_ANALYZE`'s name output (`outbuf`, a pointer that moves through the caller's
//!   buffer, and `outlen`, a pointer to the caller's length) is the whole buffer with the
//!   position `outpos` and the length `outlen` kept in the analysis; the caller reads
//!   `outlen` back when the analysis is done. `inump` is an optional `&mut`.
//! - The prototypes are the functions of `cd9660_rrip.rs`.

use crate::isofs::cd9660::cd9660_extern::IsoMnt;
use crate::isofs::cd9660::cd9660_node::IsoNode;
use crate::isofs::cd9660::iso::Cdino;
use crate::sys::types::{Daddr, Off};

/// `ISO_SUSP_ATTR`: analyze function flag (similar to RR field bits).
pub const ISO_SUSP_ATTR: i32 = 0x0001;
/// `ISO_SUSP_DEVICE`.
pub const ISO_SUSP_DEVICE: i32 = 0x0002;
/// `ISO_SUSP_SLINK`.
pub const ISO_SUSP_SLINK: i32 = 0x0004;
/// `ISO_SUSP_ALTNAME`.
pub const ISO_SUSP_ALTNAME: i32 = 0x0008;
/// `ISO_SUSP_CLINK`.
pub const ISO_SUSP_CLINK: i32 = 0x0010;
/// `ISO_SUSP_PLINK`.
pub const ISO_SUSP_PLINK: i32 = 0x0020;
/// `ISO_SUSP_RELDIR`.
pub const ISO_SUSP_RELDIR: i32 = 0x0040;
/// `ISO_SUSP_TSTAMP`.
pub const ISO_SUSP_TSTAMP: i32 = 0x0080;
/// `ISO_SUSP_IDFLAG`.
pub const ISO_SUSP_IDFLAG: i32 = 0x0100;
/// `ISO_SUSP_EXTREF`.
pub const ISO_SUSP_EXTREF: i32 = 0x0200;
/// `ISO_SUSP_CONT`.
pub const ISO_SUSP_CONT: i32 = 0x0400;
/// `ISO_SUSP_OFFSET`.
pub const ISO_SUSP_OFFSET: i32 = 0x0800;
/// `ISO_SUSP_STOP`.
pub const ISO_SUSP_STOP: i32 = 0x1000;
/// `ISO_SUSP_UNKNOWN`.
pub const ISO_SUSP_UNKNOWN: i32 = 0x8000;

/// `ISO_RRIP_ANALYZE`: the state of one walk over a directory record's SUSP entries.
pub struct IsoRripAnalyze<'a> {
    /// `inop`: the node whose attributes are filled in (`cd9660_rrip_analyze`).
    pub inop: Option<&'a IsoNode>,
    /// `fields`: interesting fields in this analysis.
    pub fields: i32,
    /// `iso_ce_blk`: block of continuation area.
    pub iso_ce_blk: Daddr,
    /// `iso_ce_off`: offset of continuation area.
    pub iso_ce_off: Off,
    /// `iso_ce_len`: length of continuation area.
    pub iso_ce_len: i32,
    /// `imp`: mount structure.
    pub imp: &'a IsoMnt,
    /// `inump`: inode number pointer.
    pub inump: Option<&'a mut Cdino>,
    /// `outbuf`: name/symbolic link output area (the whole of it; see the module's
    /// deviations).
    pub outbuf: &'a mut [u8],
    /// Where the C's `outbuf` points: an offset into [`Self::outbuf`].
    pub outpos: usize,
    /// `*outlen`: length of above.
    pub outlen: u16,
    /// `maxlen`: maximum length of above.
    pub maxlen: u16,
    /// `cont`: continuation of above.
    pub cont: i32,
}

impl<'a> IsoRripAnalyze<'a> {
    /// An analysis of `imp`'s records looking for `fields`, with no node, no output and
    /// no inode number.
    pub fn new(imp: &'a IsoMnt, fields: i32) -> Self {
        Self {
            inop: None,
            fields,
            iso_ce_blk: 0,
            iso_ce_off: 0,
            iso_ce_len: 0,
            imp,
            inump: None,
            outbuf: &mut [],
            outpos: 0,
            outlen: 0,
            maxlen: 0,
            cont: 0,
        }
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/isofs/cd9660/iso_rrip.h");
        for (name, value) in [
            ("ISO_SUSP_ATTR", ISO_SUSP_ATTR),
            ("ISO_SUSP_DEVICE", ISO_SUSP_DEVICE),
            ("ISO_SUSP_SLINK", ISO_SUSP_SLINK),
            ("ISO_SUSP_ALTNAME", ISO_SUSP_ALTNAME),
            ("ISO_SUSP_CLINK", ISO_SUSP_CLINK),
            ("ISO_SUSP_PLINK", ISO_SUSP_PLINK),
            ("ISO_SUSP_RELDIR", ISO_SUSP_RELDIR),
            ("ISO_SUSP_TSTAMP", ISO_SUSP_TSTAMP),
            ("ISO_SUSP_IDFLAG", ISO_SUSP_IDFLAG),
            ("ISO_SUSP_EXTREF", ISO_SUSP_EXTREF),
            ("ISO_SUSP_CONT", ISO_SUSP_CONT),
            ("ISO_SUSP_OFFSET", ISO_SUSP_OFFSET),
            ("ISO_SUSP_STOP", ISO_SUSP_STOP),
            ("ISO_SUSP_UNKNOWN", ISO_SUSP_UNKNOWN),
        ] {
            assert_eq!(
                crate::reftest::int(&defs, name),
                Some(i64::from(value)),
                "{name}"
            );
        }
    }
}
/* </TESTS> */
