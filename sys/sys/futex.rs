/*	$OpenBSD: futex.h,v 1.3 2025/05/07 00:39:09 dlg Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2016 Martin Pieuchot
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
//! `<sys/futex.h>`: the operations and flags of `futex(2)`.
//!
//! Upstream: sys/sys/futex.h @ 3ce1f3f79392
//!
//! Status: `ported`. The user-space prototype is libc's.

/// `FUTEX_OP_MASK`.
pub const FUTEX_OP_MASK: i32 = 0x007f;
/// `FUTEX_WAIT`.
pub const FUTEX_WAIT: i32 = 1;
/// `FUTEX_WAKE`.
pub const FUTEX_WAKE: i32 = 2;
/// `FUTEX_REQUEUE`.
pub const FUTEX_REQUEUE: i32 = 3;
/// `FUTEX_FLAG_MASK`.
pub const FUTEX_FLAG_MASK: i32 = 0xff80;
/// `FUTEX_PRIVATE_FLAG`.
pub const FUTEX_PRIVATE_FLAG: i32 = 0x0080;
/// `FUTEX_WAIT_PRIVATE`.
pub const FUTEX_WAIT_PRIVATE: i32 = FUTEX_WAIT | FUTEX_PRIVATE_FLAG;
/// `FUTEX_WAKE_PRIVATE`.
pub const FUTEX_WAKE_PRIVATE: i32 = FUTEX_WAKE | FUTEX_PRIVATE_FLAG;
/// `FUTEX_REQUEUE_PRIVATE`.
pub const FUTEX_REQUEUE_PRIVATE: i32 = FUTEX_REQUEUE | FUTEX_PRIVATE_FLAG;
/* </CODE> */
