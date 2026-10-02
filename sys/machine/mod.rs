//! The machine-dependent interface: OpenBSD `<machine/*.h>` and `cpufunc.h` as traits.
//!
//! Generic code reaches architecture code ONLY through this module. The selected architecture is
//! re-exported as [`Machine`]; the block at the bottom proves at compile time that it implements
//! every trait in [`api`]. Adding a trait method therefore means implementing it for amd64, arm64
//! and the host test double in the same commit. [`bootinfo`] is the bootloader-neutral record the
//! boot glue hands to [`Cpu::early_init`].

pub mod api;
pub mod bootinfo;

pub use api::*;
pub use bootinfo::*;

/// The selected architecture's implementation of the machine interface.
pub use crate::arch::current::Machine;

// Compile-time proof that the selected architecture implements the whole contract.
const _: () = {
    const fn assert_impl<
        M: api::MachineInfo + api::MachineParam + api::Cpu + api::Console + api::Exit,
    >() {
    }
    assert_impl::<Machine>();
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_machine_is_the_host_double_under_test() {
        assert_eq!(Machine::MACHINE, "host");
        assert_eq!(Machine::MACHINE_ARCH, "host");
    }

    #[test]
    fn host_page_geometry_is_consistent() {
        assert_eq!(Machine::PAGE_SIZE, 1 << Machine::PAGE_SHIFT);
        assert_eq!(Machine::PAGE_MASK, Machine::PAGE_SIZE - 1);
        assert_eq!(Machine::USPACE, Machine::UPAGES * Machine::PAGE_SIZE);
        assert!(Machine::MAX_PAGE_SHIFT >= Machine::PAGE_SHIFT);
    }

    #[test]
    fn qemu_exit_statuses_are_distinct_and_odd() {
        assert_ne!(
            ExitStatus::Success.qemu_status(),
            ExitStatus::Failure.qemu_status()
        );
        assert_eq!(ExitStatus::Success.qemu_status() % 2, 1);
        assert_eq!(ExitStatus::Failure.qemu_status() % 2, 1);
    }
}
