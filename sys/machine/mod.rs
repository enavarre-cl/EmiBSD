//! The machine-dependent interface: OpenBSD `<machine/*.h>` and `cpufunc.h` as traits.
//!
//! Generic code reaches architecture code ONLY through this module. One module per OpenBSD header
//! ([`param`], [`vmparam`], [`cpu`], [`cons`], [`bus`], [`pmap`], [`intr`], [`db_machdep`],
//! [`fdt`], [`proc`], [`tcb`]; [`bootinfo`]
//! is the record the boot glue hands over), all re-exported here. The selected architecture is re-exported as [`Machine`]; the block at the
//! bottom proves at compile time that it implements every trait. Adding a trait method therefore
//! means implementing it for amd64, arm64 and the host test double in the same commit.

pub mod bootinfo;
pub mod bus;
pub mod cons;
pub mod copy;
pub mod cpu;
pub mod db_machdep;
pub mod exec;
pub mod fdt;
pub mod intr;
pub mod param;
pub mod pmap;
pub mod proc;
pub mod tcb;
pub mod vmparam;

pub use bootinfo::*;
pub use bus::*;
pub use cons::*;
pub use copy::*;
pub use cpu::*;
pub use db_machdep::*;
pub use exec::*;
pub use fdt::*;
pub use intr::*;
pub use param::*;
pub use pmap::*;
pub use proc::*;
pub use tcb::*;
pub use vmparam::*;

/// The selected architecture's implementation of the machine interface.
pub use crate::arch::current::Machine;

// Compile-time proof that the selected architecture implements the whole contract.
const _: () = {
    const fn assert_impl<
        M: MachineInfo
            + MachineParam
            + VmParam
            + Cpu
            + Console
            + Exit
            + BusSpace
            + DbMachdep
            + Pmap
            + Intr
            + Fdt
            + MachineProc
            + UserCopy
            + MachineExec
            + Tcb,
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
