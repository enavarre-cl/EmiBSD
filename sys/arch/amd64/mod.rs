//! amd64 (x86_64) machine-dependent code: OpenBSD `sys/arch/amd64/`.
//!
//! Layout follows OpenBSD: `amd64/` for `.c`/`.S` ports (`locore`, `machdep`, `pmap`, `trap`),
//! `include/` for header ports, `conf/kernel.ld` for the linker script.

#[allow(clippy::module_inception)] // OpenBSD's layout: sys/arch/amd64/amd64/
pub mod amd64;
pub mod include;

use core::arch::asm;

use crate::machine::{BootInfo, Console, Cpu, Exit, ExitStatus, MachineInfo};

/// The amd64 implementation of the machine interface.
pub struct Machine;

impl MachineInfo for Machine {
    const MACHINE: &'static str = include::param::MACHINE;
    const MACHINE_ARCH: &'static str = include::param::MACHINE_ARCH;
}

impl Cpu for Machine {
    unsafe fn early_init(_boot: &BootInfo) -> Result<(), &'static str> {
        // SAFETY: forwarded; `_start` calls this once with COM1 untouched.
        unsafe { amd64::earlycons::init() };
        Ok(())
    }

    /// What `cpu_idle_cycle_hlt` in `machdep.c` does, forever and with interrupts off.
    fn halt() -> ! {
        let _ = include::cpufunc::intr_disable();
        loop {
            // SAFETY: with interrupts disabled `hlt` parks the CPU; nothing else is touched.
            unsafe { asm!("hlt", options(nomem, nostack, preserves_flags)) };
        }
    }
}

impl Console for Machine {
    fn putc(c: u8) {
        amd64::earlycons::putc(c);
    }
}

impl Exit for Machine {
    fn exit(status: ExitStatus) -> ! {
        #[cfg(feature = "qemu")]
        {
            amd64::qemu::exit(status)
        }
        #[cfg(not(feature = "qemu"))]
        {
            let _ = status;
            Self::halt()
        }
    }
}
