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
//! The I2C bus: OpenBSD `sys/dev/i2c/`.
//!
//! `i2c_io` and `i2cvar` are the headers (operations, the controller interface, the attach
//! arguments); `i2c` the bus driver (`iic* at piixpm?`, `iic* at ichiic?`), `i2c_exec` the
//! scripted client interface (`iic_exec`, the SMBus operations) and `i2c_scan` the bus scan
//! with its probe heuristics (M16e). The controllers are `ichiic(4)` and `piixpm(4)` in
//! `dev/pci/`; the chip drivers on the bus (`spdmem`, `lm`, ...) are not ported.

#[allow(clippy::module_inception)] // OpenBSD's layout: sys/dev/i2c/i2c.c
pub mod i2c;
pub mod i2c_exec;
pub mod i2c_io;
pub mod i2c_scan;
pub mod i2cvar;
/* </CODE> */
