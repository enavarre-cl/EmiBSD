//! The machine-dependent interface: OpenBSD `<machine/*.h>` and `cpufunc.h` as traits.
//!
//! Generic code reaches architecture code ONLY through this module. The selected architecture is
//! re-exported as [`Machine`]; the block at the bottom proves at compile time that it implements
//! every trait in [`api`]. Adding a trait method therefore means implementing it for amd64, arm64
//! and the host test double in the same commit.

pub mod api;

pub use api::*;

/// The selected architecture's implementation of the machine interface.
pub use crate::arch::current::Machine;

// Compile-time proof that the selected architecture implements the whole contract.
const _: () = {
    const fn assert_impl<M: api::MachineInfo>() {}
    assert_impl::<Machine>();
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_machine_is_the_host_double_under_test() {
        assert_eq!(Machine::MACHINE, "host");
    }
}
