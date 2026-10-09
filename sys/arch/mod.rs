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
//! Machine-dependent code, one directory per architecture: OpenBSD `sys/arch/<arch>/`.
//!
//! Exactly one implementation is compiled in and re-exported as `current`. Nothing outside
//! `sys/arch/` and `sys/machine/` may name these modules directly.

#[cfg(all(target_os = "none", target_arch = "x86_64"))]
pub mod amd64;
#[cfg(all(target_os = "none", target_arch = "aarch64"))]
pub mod arm64;
#[cfg(not(target_os = "none"))]
pub mod host;

#[cfg(all(target_os = "none", target_arch = "x86_64"))]
pub use self::amd64 as current;
#[cfg(all(target_os = "none", target_arch = "aarch64"))]
pub use self::arm64 as current;
#[cfg(not(target_os = "none"))]
pub use self::host as current;
/* </CODE> */
