/*	$OpenBSD: intrmap.h,v 1.4 2025/06/13 09:48:45 jsg Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2020 David Gwynne <dlg@openbsd.org>
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
//! `<sys/intrmap.h>`: spreading a device's interrupts (its rings) over the CPUs. The
//! functions and the opaque `struct intrmap` live in `kern/kern_intrmap.rs`.
//!
//! Upstream: sys/sys/intrmap.h @ 3ce1f3f79392

/// `INTRMAP_POWEROF2`: round the number of interrupts down to a power of two.
pub const INTRMAP_POWEROF2: u32 = 1 << 0;
/* </CODE> */
