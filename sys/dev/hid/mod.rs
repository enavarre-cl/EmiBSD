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
//! HID class support: OpenBSD `sys/dev/hid/`.
//!
//! `hid` is the report descriptor parser with `<dev/hid/hid.h>`; `hidkbd` is the keyboard
//! logic `ukbd(4)` and `ikbd(4)` wrap, with `hidkbdsc.h` and `hidkbdvar.h`. `hidms` is the mouse, tablet and
//! touch panel logic `ums(4)` and `uwacom(4)` wrap, with `hidmsvar.h`. `hidmt` and `hidcc`
//! (multitouch, consumer control) are not ported yet.

#[allow(clippy::module_inception)] // OpenBSD's layout: dev/hid/hid.c
pub mod hid;
pub mod hidkbd;
pub mod hidms;
pub mod hidmsvar;
/* </CODE> */
