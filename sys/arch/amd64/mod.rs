//! amd64 (x86_64) machine-dependent code: OpenBSD `sys/arch/amd64/`.
//!
//! Layout follows OpenBSD: `amd64/` for `.c`/`.S` ports (`locore`, `machdep`, `pmap`, `trap`),
//! `include/` for header ports, `isa/` for the ISA-side clock and RTC, `conf/kernel.ld` for the
//! linker script.

#[allow(clippy::module_inception)] // OpenBSD's layout: sys/arch/amd64/amd64/
pub mod amd64;
pub mod include;
pub mod isa;

use core::arch::asm;

use crate::machine::bus::{BusAddr, BusSize, BusSpace};
use crate::machine::db_machdep::{DbMachdep, PrFn};
use crate::machine::{BootInfo, Console, Cpu, Exit, ExitStatus, MachineInfo};
use crate::sys::errno::Errno;

/// The amd64 implementation of the machine interface.
pub struct Machine;

impl MachineInfo for Machine {
    const MACHINE: &'static str = include::param::MACHINE;
    const MACHINE_ARCH: &'static str = include::param::MACHINE_ARCH;
}

impl Cpu for Machine {
    unsafe fn early_init(boot: &BootInfo) -> Result<(), &'static str> {
        // SAFETY: forwarded; `_start` calls this once with the machine as Limine left it.
        unsafe { amd64::machdep::init_x86_64(boot) }
    }

    /// What `cpu_idle_cycle_hlt` in `machdep.c` does, forever and with interrupts off.
    fn halt() -> ! {
        let _ = include::cpufunc::intr_disable();
        loop {
            // SAFETY: with interrupts disabled `hlt` parks the CPU; nothing else is touched.
            unsafe { asm!("hlt", options(nomem, nostack, preserves_flags)) };
        }
    }

    fn boot(howto: i32) -> ! {
        amd64::machdep::boot(howto)
    }

    fn delay(usec: u32) {
        amd64::machdep::delay(usec)
    }
}

impl Console for Machine {
    fn consinit() {
        amd64::consinit::consinit()
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

impl BusSpace for Machine {
    type Tag = amd64::bus_space::X86BusSpace;
    type Handle = amd64::bus_space::BusSpaceHandle;

    const BUS_SPACE_MAP_CACHEABLE: u32 = amd64::bus_space::BUS_SPACE_MAP_CACHEABLE;
    const BUS_SPACE_MAP_LINEAR: u32 = amd64::bus_space::BUS_SPACE_MAP_LINEAR;
    const BUS_SPACE_MAP_PREFETCHABLE: u32 = amd64::bus_space::BUS_SPACE_MAP_PREFETCHABLE;

    unsafe fn bus_space_map(
        t: Self::Tag,
        addr: BusAddr,
        size: BusSize,
        flags: u32,
    ) -> Result<Self::Handle, Errno> {
        // SAFETY: forwarded.
        unsafe { amd64::bus_space::bus_space_map(t, addr, size, flags) }
    }

    fn bus_space_unmap(t: Self::Tag, h: Self::Handle, size: BusSize) {
        amd64::bus_space::bus_space_unmap(t, h, size)
    }

    fn bus_space_read_1(t: Self::Tag, h: Self::Handle, offset: BusSize) -> u8 {
        amd64::bus_space::bus_space_read_1(t, h, offset)
    }

    fn bus_space_read_2(t: Self::Tag, h: Self::Handle, offset: BusSize) -> u16 {
        amd64::bus_space::bus_space_read_2(t, h, offset)
    }

    fn bus_space_read_4(t: Self::Tag, h: Self::Handle, offset: BusSize) -> u32 {
        amd64::bus_space::bus_space_read_4(t, h, offset)
    }

    fn bus_space_write_1(t: Self::Tag, h: Self::Handle, offset: BusSize, value: u8) {
        amd64::bus_space::bus_space_write_1(t, h, offset, value)
    }

    fn bus_space_write_2(t: Self::Tag, h: Self::Handle, offset: BusSize, value: u16) {
        amd64::bus_space::bus_space_write_2(t, h, offset, value)
    }

    fn bus_space_write_4(t: Self::Tag, h: Self::Handle, offset: BusSize, value: u32) {
        amd64::bus_space::bus_space_write_4(t, h, offset, value)
    }

    fn bus_space_barrier(
        t: Self::Tag,
        h: Self::Handle,
        offset: BusSize,
        length: BusSize,
        flags: u32,
    ) {
        amd64::bus_space::bus_space_barrier(t, h, offset, length, flags)
    }
}

impl DbMachdep for Machine {
    fn db_stack_trace_print(addr: usize, have_addr: bool, count: usize, modif: &[u8], pr: PrFn) {
        amd64::db_trace::db_stack_trace_print(addr, have_addr, count, modif, pr)
    }

    #[inline(always)]
    fn frame_address() -> usize {
        let fp: usize;
        // SAFETY: reads the frame pointer register; `force-frame-pointers=yes` keeps it a
        // real frame pointer in every function.
        unsafe { asm!("mov {}, rbp", out(reg) fp, options(nomem, nostack, preserves_flags)) };
        fp
    }

    fn db_enter() {
        amd64::db_interface::db_enter()
    }
}
