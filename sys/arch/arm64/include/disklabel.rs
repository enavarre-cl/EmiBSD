/* $OpenBSD: disklabel.h,v 1.1 2016/12/17 23:38:33 patrick Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2014 Patrick Wildt <patrick@blueri.se>
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
//! arm64 `<machine/disklabel.h>`: where the disk label sits and how many partitions it may
//! describe.
//!
//! Upstream: sys/arch/arm64/include/disklabel.h @ 3ce1f3f79392
//!
//! Generic code reads these through `machine::disklabel::MachineDisklabel`, re-exported by
//! `sys::disklabel`.

/// `LABELSECTOR`: sector containing label.
pub const LABELSECTOR: u64 = 1;
/// `LABELOFFSET`: offset of label in sector.
pub const LABELOFFSET: usize = 0;
/// `MAXPARTITIONS`: number of partitions.
pub const MAXPARTITIONS: usize = 16;
/* </CODE> */
