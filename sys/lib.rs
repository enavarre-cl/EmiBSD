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
//! `bsd`: the OpenBSD kernel, re-implemented in Rust.
//!
//! The module tree mirrors `reference/openbsd-src/sys/` one directory at a time:
//! `sys` (headers → types), `kern`, `uvm`, `dev`, `ddb`, `net`, `netinet`, `netinet6`, `arch/<arch>`, with
//! `machine` as the `<machine/*.h>` contract between generic and architecture code. See
//! `docs/ARCHITECTURE.md`.

#![no_std]

// The host build (tests and the `arch/host` double) links std; bare-metal never does.
#[cfg(not(target_os = "none"))]
extern crate std;

#[cfg(feature = "alloc")]
extern crate alloc;

pub mod arch;
pub mod conf;
pub mod crypto;
pub mod ddb;
pub mod dev;
pub mod isofs;
pub mod kern;
pub mod machine;
pub mod miscfs;
#[cfg(feature = "msdosfs")]
pub mod msdosfs;
pub mod net;
pub mod netinet;
pub mod netinet6;
#[cfg(any(feature = "nfsclient", feature = "nfsserver"))]
pub mod nfs;
#[cfg(option_ntfs)]
pub mod ntfs;
pub mod scsi;
pub mod sys;
#[cfg(feature = "tmpfs")]
pub mod tmpfs;
#[cfg(feature = "ffs")]
pub mod ufs;
pub mod uvm;

#[cfg(test)]
pub(crate) mod reftest;

/// Kernel panic entry point for bare-metal targets: `panic!("...")` anywhere in the kernel is
/// OpenBSD's `panic(9)`, in `kern/subr_prf.rs`. The message is the one `panic!` was given; the
/// Rust source location is left out, as the C prints only the message. On the host, std's panic
/// handler is used instead.
#[cfg(target_os = "none")]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    kern::subr_prf::panic(format_args!("{}", info.message()))
}
/* </CODE> */
