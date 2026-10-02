//! Traits every architecture implements.
//!
//! Grows with the milestones: M0 adds [`Cpu`], [`Console`] and [`Exit`]; M1 adds
//! [`MachineParam`] (`<machine/param.h>` and the alignment rules of `<machine/_types.h>`);
//! M3 adds `Pmap`; M4 adds `Intr`/`Spl` and trap frames. Keep each trait small and named after
//! the OpenBSD header or `(9)` interface it stands in for.

use crate::machine::bootinfo::BootInfo;

/// Identity of the running architecture: `MACHINE` and `MACHINE_ARCH` of `<machine/param.h>`.
pub trait MachineInfo {
    /// The architecture name as OpenBSD spells it: `"amd64"` or `"arm64"`; `"host"` for the
    /// test double.
    const MACHINE: &'static str;
    /// The CPU architecture name: `"amd64"` or `"aarch64"`; `"host"` for the test double.
    const MACHINE_ARCH: &'static str;
}

/// Machine-dependent parameters: `<machine/param.h>` plus the alignment rules of
/// `<machine/_types.h>`.
///
/// Generic code reads these through `sys::param` (`PAGE_SIZE`, `ALIGNBYTES`, ...), exactly as C
/// reaches them through `<sys/param.h>`. Every value is a plain constant in the architecture's
/// `include/param.rs` or `include/_types.rs`; the trait only proves that each architecture
/// defines the whole set.
pub trait MachineParam {
    /// `PAGE_SHIFT`: log2 of the page size.
    const PAGE_SHIFT: usize;
    /// `PAGE_SIZE`: bytes per page.
    const PAGE_SIZE: usize;
    /// `PAGE_MASK`: byte offset mask within a page.
    const PAGE_MASK: usize;
    /// `KERNBASE`: start of kernel virtual address space.
    const KERNBASE: usize;
    /// `UPAGES`: pages of u-area (per-thread kernel stack and PCB).
    const UPAGES: usize;
    /// `USPACE`: total size of the u-area.
    const USPACE: usize;
    /// `USPACE_ALIGN`: u-area alignment, 0 for none.
    const USPACE_ALIGN: usize;
    /// `__HAVE_USPACE_GUARD`: the u-area carries a guard page.
    const HAVE_USPACE_GUARD: bool;
    /// `NMBCLUSTERS`: maximum number of mbuf clusters.
    const NMBCLUSTERS: usize;
    /// `MSGBUFSIZE`: default kernel message buffer size.
    const MSGBUFSIZE: usize;
    /// `__HAVE_ACPI`: the architecture boots with ACPI tables.
    const HAVE_ACPI: bool;
    /// `__HAVE_FDT`: the architecture boots with a flattened device tree.
    const HAVE_FDT: bool;
    /// `_ALIGNBYTES`: rounding mask that aligns an address for every data type.
    const ALIGNBYTES: usize;
    /// `_STACKALIGNBYTES`: rounding mask for the stack pointer.
    const STACKALIGNBYTES: usize;
    /// `_MAX_PAGE_SHIFT`: the largest page shift the architecture can use.
    const MAX_PAGE_SHIFT: usize;
    /// `_ALIGNED_POINTER(p, t)`: whether a value of type `T` may be fetched from address `p`.
    /// This reflects possibility, not optimal alignment.
    fn aligned_pointer<T>(p: usize) -> bool;
}

/// The boot CPU, from the bootloader's hand-off until `cpu_startup` exists (milestone M5).
pub trait Cpu {
    /// Earliest machine setup, called once by the boot glue before anything prints: whatever the
    /// polled console needs (on arm64, a temporary mapping of the device). Nothing else is
    /// touched. The error is a fixed message because there is nowhere to print it yet; the glue
    /// turns it into a failure exit.
    ///
    /// # Safety
    ///
    /// Call exactly once, on the boot CPU, with the machine in the state the Limine protocol
    /// specifies at entry, and `boot` describing the image that was just loaded.
    unsafe fn early_init(boot: &BootInfo) -> Result<(), &'static str>;

    /// Masks interrupts and parks the CPU forever.
    fn halt() -> !;
}

/// The polled early console: what `cnputc(9)` becomes once `dev/cons.c` is ported.
pub trait Console {
    /// Writes one byte, blocking until the device accepts it.
    fn putc(c: u8);
}

/// How the kernel leaves the machine.
pub trait Exit {
    /// Leaves with `status`: under QEMU (feature `qemu`) the emulator exits with
    /// [`ExitStatus::qemu_status`], which `xtask smoke` checks; without it the CPU is halted.
    fn exit(status: ExitStatus) -> !;
}

/// Outcome reported through [`Exit::exit`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitStatus {
    /// Everything the run set out to do happened.
    Success,
    /// The kernel gave up; the serial transcript says why when a console was available.
    Failure,
}

impl ExitStatus {
    /// The process exit status QEMU reports for this outcome. Odd numbers, because amd64's
    /// `isa-debug-exit` device can only produce `(v << 1) | 1`; arm64 passes the same values
    /// through semihosting so `xtask smoke` checks one number on both.
    pub const fn qemu_status(self) -> u32 {
        match self {
            ExitStatus::Success => 33,
            ExitStatus::Failure => 35,
        }
    }
}
