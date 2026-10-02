//! Host test double for the machine interface. Not an OpenBSD architecture.
//!
//! Compiled whenever `target_os != "none"` so that `cargo test` runs on macOS/Linux and the
//! compiler proves the `machine` contract is complete. It prints to stdout, has no-op SPL and
//! fakes hardware with std collections. It must not grow logic: behaviour belongs in `kern/`.
//!
//! Its machine parameters mirror amd64's `<machine/param.h>` and `<machine/_types.h>`, so tests
//! see the page geometry of a real architecture (`just test-ref` checks that they stay equal).

use std::io::Write;

use crate::machine::api::{Console, Cpu, Exit, ExitStatus, MachineInfo, MachineParam};
use crate::machine::bootinfo::BootInfo;

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

impl Cpu for Machine {
    unsafe fn early_init(_boot: &BootInfo) -> Result<(), &'static str> {
        Ok(())
    }

    /// The host has no CPU to park: the process ends instead.
    fn halt() -> ! {
        std::process::exit(0)
    }
}

impl Console for Machine {
    fn putc(c: u8) {
        // A failed write to stdout has nowhere to be reported; the console is best effort.
        let _ = std::io::stdout().write_all(&[c]);
    }
}

impl Exit for Machine {
    fn exit(status: ExitStatus) -> ! {
        let _ = std::io::stdout().flush();
        std::process::exit(status.qemu_status() as i32)
    }
}
