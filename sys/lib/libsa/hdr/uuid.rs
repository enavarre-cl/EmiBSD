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
/* </LICENSES> */

/* <CODE> */
//! `<sys/uuid.h>` for libsa: `struct uuid`.

/// `_UUID_NODE_LEN`.
pub const UUID_NODE_LEN: usize = 6;

/// `struct uuid`.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
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

const _: () = assert!(core::mem::size_of::<Uuid>() == 16);
/* </CODE> */
