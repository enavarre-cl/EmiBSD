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
//! The machine-dependent interface: OpenBSD `<machine/*.h>` and `cpufunc.h` as traits.
//!
//! Generic code reaches architecture code ONLY through this module. One module per OpenBSD header
//! ([`param`], [`vmparam`], [`cpu`], [`cons`], [`bus`], [`pmap`], [`intr`], [`db_machdep`],
//! [`disklabel`], [`fdt`], [`proc`], [`signal`], [`tcb`], [`pci_machdep`], [`isa_machdep`], [`atomic`];
//! [`acpi_machdep`] is the machine half of `<dev/acpi/acpivar.h>` (each arch's `acpi_machdep.c`)
//! and [`pciide_machdep`] that of `<dev/pci/pciidevar.h>` (each arch's `pciide_machdep.c`);
//! [`autoconf`] is what `ioconf.c` and the machine's `autoconf.c` give `subr_autoconf.c`;
//! [`conf`] is the device switch the machine's `conf.c` fills (`bdevsw[]`, `cdevsw[]`); [`bootinfo`]
//! is the record the boot glue hands over), all re-exported here. `pci_chipset` (cfg
//! `machine_pci_chipset`, arm64 only) is not a contract but the machine items the device-tree
//! PCI host bridges use directly, as their C does; it is reached by its path, as is `x86`
//! (cfg `machine_x86`, amd64 only), the x86 items `dev/acpi/acpidmar.c` and
//! `dev/acpi/acpicpu_x86.c` use directly. The selected
//! architecture is re-exported as [`Machine`]; the block at the
//! bottom proves at compile time that it implements every trait. Adding a trait method therefore
//! means implementing it for amd64, arm64 and the host test double in the same commit.

pub mod acpi_machdep;
pub mod atomic;
pub mod autoconf;
pub mod bootinfo;
pub mod bus;
pub mod conf;
pub mod cons;
pub mod copy;
pub mod cpu;
pub mod db_machdep;
pub mod disklabel;
pub mod exec;
pub mod fdt;
pub mod intr;
pub mod isa_machdep;
pub mod mpconfig;
pub mod param;
#[cfg(machine_pci_chipset)]
pub mod pci_chipset;
pub mod pci_machdep;
pub mod pciide_machdep;
pub mod pmap;
pub mod proc;
pub mod signal;
pub mod tcb;
pub mod vmparam;
#[cfg(machine_x86)]
pub mod x86;

pub use acpi_machdep::*;
pub use atomic::*;
pub use autoconf::*;
pub use bootinfo::*;
pub use bus::*;
pub use conf::*;
pub use cons::*;
pub use copy::*;
pub use cpu::*;
pub use db_machdep::*;
pub use disklabel::*;
pub use exec::*;
pub use fdt::*;
pub use intr::*;
pub use isa_machdep::*;
pub use mpconfig::*;
pub use param::*;
pub use pci_machdep::*;
pub use pciide_machdep::*;
pub use pmap::*;
pub use proc::*;
pub use signal::*;
pub use tcb::*;
pub use vmparam::*;

/// The selected architecture's implementation of the machine interface.
pub use crate::arch::current::Machine;

// Compile-time proof that the selected architecture implements the whole contract.
const _: () = {
    const fn assert_impl<
        M: MachineInfo
            + AcpiMachdep
            + Atomic
            + Autoconf
            + MachineParam
            + VmParam
            + Cpu
            + Console
            + Exit
            + Conf
            + BusSpace
            + BusDma
            + PciMachdep
            + PciideMachdep
            + DbMachdep
            + MachineDisklabel
            + Pmap
            + Intr
            + Fdt
            + IsaMachdep
            + MpConfig
            + MachineProc
            + UserCopy
            + MachineExec
            + MachineSignal
            + Tcb,
    >() {
    }
    assert_impl::<Machine>();
};
/* </CODE> */

/* <TESTS> */
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
/* </TESTS> */
