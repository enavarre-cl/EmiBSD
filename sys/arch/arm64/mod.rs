//! arm64 (aarch64) machine-dependent code: OpenBSD `sys/arch/arm64/`.
//!
//! Layout follows OpenBSD: `arm64/` for `.c`/`.S` ports (`locore`, `machdep`, `pmap`, `trap`),
//! `include/` for header ports, `dev/` for arch-only drivers (GIC, generic timer),
//! `conf/kernel.ld` for the linker script.

#[allow(clippy::module_inception)] // OpenBSD's layout: sys/arch/arm64/arm64/
pub mod arm64;
pub mod include;

use core::arch::asm;

use crate::machine::bus::{BusAddr, BusSize, BusSpace};
use crate::machine::db_machdep::{DbMachdep, PrFn};
use crate::machine::{BootInfo, Console, Cpu, Exit, ExitStatus, MachineInfo};
use crate::sys::errno::Errno;

/// The arm64 implementation of the machine interface.
pub struct Machine;

impl MachineInfo for Machine {
    const MACHINE: &'static str = include::param::MACHINE;
    const MACHINE_ARCH: &'static str = include::param::MACHINE_ARCH;
}

impl Cpu for Machine {
    unsafe fn early_init(boot: &BootInfo) -> Result<(), &'static str> {
        // SAFETY: forwarded; `_start` calls this once with the machine as Limine left it.
        unsafe { arm64::machdep::initarm(boot) }
    }

    fn halt() -> ! {
        include::cpu::disable_irq_daif();
        loop {
            // SAFETY: `wfi` only waits for an event; with interrupts masked it just idles.
            unsafe { asm!("wfi", options(nomem, nostack, preserves_flags)) };
        }
    }

    fn boot(howto: i32) -> ! {
        arm64::machdep::boot(howto)
    }

    fn delay(usec: u32) {
        arm64::intr::delay(usec)
    }
}

impl Console for Machine {
    fn consinit() {
        arm64::machdep::consinit()
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

impl BusSpace for Machine {
    type Tag = &'static include::bus::BusSpace;
    type Handle = include::bus::BusSpaceHandle;

    const BUS_SPACE_MAP_CACHEABLE: u32 = include::bus::BUS_SPACE_MAP_CACHEABLE;
    const BUS_SPACE_MAP_LINEAR: u32 = include::bus::BUS_SPACE_MAP_LINEAR;
    const BUS_SPACE_MAP_PREFETCHABLE: u32 = include::bus::BUS_SPACE_MAP_PREFETCHABLE;

    unsafe fn bus_space_map(
        t: Self::Tag,
        addr: BusAddr,
        size: BusSize,
        flags: u32,
    ) -> Result<Self::Handle, Errno> {
        // SAFETY: forwarded.
        unsafe { (t._space_map)(t, addr, size, flags) }
    }

    fn bus_space_unmap(t: Self::Tag, h: Self::Handle, size: BusSize) {
        (t._space_unmap)(t, h, size)
    }

    fn bus_space_read_1(t: Self::Tag, h: Self::Handle, offset: BusSize) -> u8 {
        (t._space_read_1)(t, h, offset)
    }

    fn bus_space_read_2(t: Self::Tag, h: Self::Handle, offset: BusSize) -> u16 {
        (t._space_read_2)(t, h, offset)
    }

    fn bus_space_read_4(t: Self::Tag, h: Self::Handle, offset: BusSize) -> u32 {
        (t._space_read_4)(t, h, offset)
    }

    fn bus_space_write_1(t: Self::Tag, h: Self::Handle, offset: BusSize, value: u8) {
        (t._space_write_1)(t, h, offset, value)
    }

    fn bus_space_write_2(t: Self::Tag, h: Self::Handle, offset: BusSize, value: u16) {
        (t._space_write_2)(t, h, offset, value)
    }

    fn bus_space_write_4(t: Self::Tag, h: Self::Handle, offset: BusSize, value: u32) {
        (t._space_write_4)(t, h, offset, value)
    }

    fn bus_space_barrier(
        t: Self::Tag,
        h: Self::Handle,
        offset: BusSize,
        length: BusSize,
        flags: u32,
    ) {
        include::bus::bus_space_barrier(t, h, offset, length, flags)
    }
}

impl DbMachdep for Machine {
    fn db_stack_trace_print(addr: usize, have_addr: bool, count: usize, modif: &[u8], pr: PrFn) {
        arm64::db_trace::db_stack_trace_print(addr, have_addr, count, modif, pr)
    }

    #[inline(always)]
    fn frame_address() -> usize {
        let fp: usize;
        // SAFETY: reads the frame pointer register; `force-frame-pointers=yes` keeps it a
        // real frame pointer in every function.
        unsafe { asm!("mov {}, x29", out(reg) fp, options(nomem, nostack, preserves_flags)) };
        fp
    }

    fn db_enter() {
        arm64::db_interface::db_enter()
    }
}
