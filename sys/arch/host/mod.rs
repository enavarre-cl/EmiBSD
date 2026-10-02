//! Host test double for the machine interface. Not an OpenBSD architecture.
//!
//! Compiled whenever `target_os != "none"` so that `cargo test` runs on macOS/Linux and the
//! compiler proves the `machine` contract is complete. It prints to stdout, has no-op SPL and
//! fakes hardware with std collections. It must not grow logic: behaviour belongs in `kern/`.

/// The host implementation of the machine interface.
pub struct Machine;

impl crate::machine::api::MachineInfo for Machine {
    const MACHINE: &'static str = "host";
}
