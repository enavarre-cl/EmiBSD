//! arm64 (aarch64) machine-dependent code: OpenBSD `sys/arch/arm64/`.
//!
//! Layout follows OpenBSD: `arm64/` for `.c`/`.S` ports (`locore`, `machdep`, `pmap`, `trap`),
//! `include/` for header ports, `dev/` for arch-only drivers (GIC, generic timer),
//! `conf/kernel.ld` for the linker script.

#[allow(clippy::module_inception)] // OpenBSD's layout: sys/arch/arm64/arm64/
pub mod arm64;
pub mod include;

use core::arch::asm;

use crate::machine::{BootInfo, Console, Cpu, Exit, ExitStatus, MachineInfo};

/// The arm64 implementation of the machine interface.
pub struct Machine;

impl MachineInfo for Machine {
    const MACHINE: &'static str = include::param::MACHINE;
    const MACHINE_ARCH: &'static str = include::param::MACHINE_ARCH;
}

impl Cpu for Machine {
    unsafe fn early_init(boot: &BootInfo) -> Result<(), &'static str> {
        // SAFETY: forwarded; the caller's guarantees are the same.
        unsafe { arm64::earlycons::init(boot) }
    }

    fn halt() -> ! {
        include::cpu::disable_irq_daif();
        loop {
            // SAFETY: `wfi` only waits for an event; with interrupts masked it just idles.
            unsafe { asm!("wfi", options(nomem, nostack, preserves_flags)) };
        }
    }
}

impl Console for Machine {
    fn putc(c: u8) {
        arm64::earlycons::putc(c);
    }
}

impl Exit for Machine {
    fn exit(status: ExitStatus) -> ! {
        #[cfg(feature = "qemu")]
        {
            arm64::qemu::exit(status)
        }
        #[cfg(not(feature = "qemu"))]
        {
            let _ = status;
            Self::halt()
        }
    }
}
