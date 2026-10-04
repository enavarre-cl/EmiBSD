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
