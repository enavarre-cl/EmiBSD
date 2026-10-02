//! `<machine/cpu.h>`, `<machine/cpufunc.h>`, `boot(9)` and `delay(9)` as traits.
//!
//! Milestone M0 needs only the earliest setup, a way to park the CPU and a way to leave the
//! machine; M2 adds `boot(9)` (the end of `panic`) and `delay(9)` (the polled console); M4 adds
//! interrupt masking (`spl(9)`), M5 context switching.

use crate::machine::Machine;
use crate::machine::bootinfo::BootInfo;

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

/// The boot CPU, from the bootloader's hand-off until `cpu_startup` exists (milestone M5).
pub trait Cpu {
    /// Earliest machine setup, called once by the boot glue before anything prints: OpenBSD's
    /// `init_x86_64` / `initarm`, as far as they are ported. It brings up the message buffer and
    /// the console (`consinit`), so everything after it can `printf`. The error is a fixed
    /// message because there is nowhere to print it yet; the glue turns it into a failure exit.
    ///
    /// # Safety
    ///
    /// Call exactly once, on the boot CPU, with the machine in the state the Limine protocol
    /// specifies at entry, and `boot` describing the image that was just loaded.
    unsafe fn early_init(boot: &BootInfo) -> Result<(), &'static str>;

    /// Masks interrupts and parks the CPU forever.
    fn halt() -> !;

    /// `boot(9)`: halts or reboots the machine according to the `RB_*` flags in `howto`
    /// (`sys/sys/reboot.rs`). `reboot()` in `kern/kern_xxx.rs` is its only caller.
    fn boot(howto: i32) -> !;

    /// `delay(9)`: busy-waits for at least `usec` microseconds.
    fn delay(usec: u32);
}

/// `boot(9)` on the selected machine.
pub fn boot(howto: i32) -> ! {
    Machine::boot(howto)
}

/// `delay(9)` on the selected machine.
pub fn delay(usec: u32) {
    Machine::delay(usec)
}

/// How the kernel leaves the machine.
pub trait Exit {
    /// Leaves with `status`: under QEMU (feature `qemu`) the emulator exits with
    /// [`ExitStatus::qemu_status`], which `xtask smoke` checks; without it the CPU is halted.
    fn exit(status: ExitStatus) -> !;
}
