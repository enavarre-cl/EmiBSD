//! Host test double for the machine interface. Not an OpenBSD architecture.
//!
//! Compiled whenever `target_os != "none"` so that `cargo test` runs on macOS/Linux and the
//! compiler proves the `machine` contract is complete. It prints to stdout, has no-op SPL and
//! fakes hardware with std collections. It must not grow logic: behaviour belongs in `kern/`.
//!
//! Its machine parameters mirror amd64's `<machine/param.h>` and `<machine/_types.h>`, so tests
//! see the page geometry of a real architecture (`just test-ref` checks that they stay equal).

use crate::machine::api::{MachineInfo, MachineParam};

/// The host implementation of the machine interface.
pub struct Machine;

impl MachineInfo for Machine {
    const MACHINE: &'static str = "host";
    const MACHINE_ARCH: &'static str = "host";
}

impl MachineParam for Machine {
    const PAGE_SHIFT: usize = 12;
    const PAGE_SIZE: usize = 1 << Self::PAGE_SHIFT;
    const PAGE_MASK: usize = Self::PAGE_SIZE - 1;
    const KERNBASE: usize = 0xffff_ffff_8000_0000;
    const UPAGES: usize = 6;
    const USPACE: usize = Self::UPAGES * Self::PAGE_SIZE;
    const USPACE_ALIGN: usize = 0;
    const HAVE_USPACE_GUARD: bool = true;
    const NMBCLUSTERS: usize = 256 * 1024;
    const MSGBUFSIZE: usize = 32 * Self::PAGE_SIZE;
    const HAVE_ACPI: bool = true;
    const HAVE_FDT: bool = false;
    const ALIGNBYTES: usize = core::mem::size_of::<usize>() - 1;
    const STACKALIGNBYTES: usize = 15;
    const MAX_PAGE_SHIFT: usize = 12;

    fn aligned_pointer<T>(_p: usize) -> bool {
        true
    }
}
