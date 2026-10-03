//! `bsd`: the OpenBSD kernel, re-implemented in Rust.
//!
//! The module tree mirrors `reference/openbsd-src/sys/` one directory at a time:
//! `sys` (headers → types), `kern`, `uvm`, `dev`, `ddb`, `net`, `netinet`, `arch/<arch>`, with
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
pub mod ddb;
pub mod dev;
pub mod kern;
pub mod machine;
pub mod miscfs;
pub mod net;
pub mod netinet;
pub mod sys;
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
