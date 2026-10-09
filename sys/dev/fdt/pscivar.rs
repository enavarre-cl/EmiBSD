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

/* Public Domain */
/* </LICENSES> */

/* <CODE> */
//! The PSCI function ids and return values `psci(4)` shares with its callers:
//! `dev/fdt/pscivar.h`. The functions it declares live in [`super::psci`].
//!
//! Upstream: sys/dev/fdt/pscivar.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - Only the LP64 values exist (`#ifdef __LP64__`): both ports are 64-bit.

/// `PSCI_SUCCESS`.
pub const PSCI_SUCCESS: i32 = 0;
/// `PSCI_NOT_SUPPORTED`.
pub const PSCI_NOT_SUPPORTED: i32 = -1;

/// `PSCI_METHOD_NONE`.
pub const PSCI_METHOD_NONE: i32 = 0;
/// `PSCI_METHOD_HVC`.
pub const PSCI_METHOD_HVC: i32 = 1;
/// `PSCI_METHOD_SMC`.
pub const PSCI_METHOD_SMC: i32 = 2;

/// `PSCI_VERSION`.
pub const PSCI_VERSION: u32 = 0x8400_0000;
/// `CPU_SUSPEND` (LP64).
pub const CPU_SUSPEND: u32 = 0xc400_0001;
/// `CPU_OFF`.
pub const CPU_OFF: u32 = 0x8400_0002;
/// `CPU_ON` (LP64).
pub const CPU_ON: u32 = 0xc400_0003;
/// `SYSTEM_OFF`.
pub const SYSTEM_OFF: u32 = 0x8400_0008;
/// `SYSTEM_RESET`.
pub const SYSTEM_RESET: u32 = 0x8400_0009;
/// `PSCI_FEATURES`.
pub const PSCI_FEATURES: u32 = 0x8400_000a;
/// `SYSTEM_SUSPEND` (LP64).
pub const SYSTEM_SUSPEND: u32 = 0xc400_000e;

/// `PSCI_FEATURE_POWER_STATE_EXT`.
pub const PSCI_FEATURE_POWER_STATE_EXT: u32 = 1 << 1;
/// `PSCI_POWER_STATE_POWERDOWN`.
pub const PSCI_POWER_STATE_POWERDOWN: u32 = 1 << 16;
/// `PSCI_POWER_STATE_EXT_POWERDOWN`.
pub const PSCI_POWER_STATE_EXT_POWERDOWN: u32 = 1 << 30;
/* </CODE> */
