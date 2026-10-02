//! `bsd`: the OpenBSD kernel, re-implemented in Rust.
//!
//! The module tree mirrors `reference/openbsd-src/sys/` one directory at a time:
//! `sys` (headers → types), `kern`, `uvm`, `dev`, `ddb`, `arch/<arch>`, with `machine` as the
//! `<machine/*.h>` contract between generic and architecture code. See `docs/ARCHITECTURE.md`.

#![no_std]

// The host build (tests and the `arch/host` double) links std; bare-metal never does.
#[cfg(not(target_os = "none"))]
extern crate std;

#[cfg(feature = "alloc")]
extern crate alloc;

pub mod arch;
pub mod ddb;
pub mod dev;
pub mod kern;
pub mod machine;
pub mod sys;
pub mod uvm;

#[cfg(test)]
pub(crate) mod reftest;

/// Kernel panic entry point for bare-metal targets.
///
/// Until `kern/subr_prf.rs` is ported (milestone M2) nothing can be printed, so the CPU is parked.
/// On the host, std's panic handler is used instead.
#[cfg(target_os = "none")]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
