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
 * National Semiconductor DS8390 NIC register definitions.
 *
 * Copyright (C) 1993, David Greenman.  This software may be used, modified,
 * copied, distributed, and sold, in both source and binary form provided that
 * the above copyright and these terms are retained.  Under no circumstances is
 * the author responsible for the proper functioning of this software, nor does
 * the author assume any responsibility for damages incurred with its use.
 */
/* </LICENSES> */

/* <CODE> */
//! Register group offsets of the NE2000-compatible boards (`dev/ic/ne2000reg.h`).
//!
//! Upstream: sys/dev/ic/ne2000reg.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - Offsets and port counts are `usize` (`BusSize`).

/// `NE2000_NIC_OFFSET`.
pub const NE2000_NIC_OFFSET: usize = 0x00;
/// `NE2000_ASIC_OFFSET`.
pub const NE2000_ASIC_OFFSET: usize = 0x10;
/// `NE2000_NIC_NPORTS`.
pub const NE2000_NIC_NPORTS: usize = 0x10;
/// `NE2000_ASIC_NPORTS`.
pub const NE2000_ASIC_NPORTS: usize = 0x10;
/// `NE2000_NPORTS`.
pub const NE2000_NPORTS: usize = 0x20;
/// `NE2000_ASIC_DATA`: remote DMA/data register.
pub const NE2000_ASIC_DATA: usize = 0x00;
/// `NE2000_ASIC_RESET`: reset on read.
pub const NE2000_ASIC_RESET: usize = 0x0f;
/* </CODE> */
