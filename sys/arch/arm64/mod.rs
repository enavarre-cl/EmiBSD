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
use crate::machine::{BootInfo, Console, Cpu, Exit, ExitStatus, Intr, MachineInfo, Pmap, VmParam};
use crate::sys::errno::Errno;
use crate::sys::types::{Paddr, Vaddr, Vsize};
use crate::uvm::uvm_extern::{UvmConstraintRange, VmProt};
use crate::uvm::uvm_page::VmPage;

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

    fn cpu_startup() {
        arm64::machdep::cpu_startup()
    }

    fn curcpu_ptr() -> *const () {
        core::ptr::from_ref(include::cpu::curcpu()).cast()
    }

    fn curcpu_mutex_level_add(delta: i32) {
        let ci = include::cpu::curcpu();
        ci.ci_mutex_level.set(ci.ci_mutex_level.get() + delta);
    }

    fn cpu_configure() {
        arm64::autoconf::cpu_configure()
    }
}

impl VmParam for Machine {
    const VM_MIN_ADDRESS: usize = include::vmparam::VM_MIN_ADDRESS;
    const VM_MAXUSER_ADDRESS: usize = include::vmparam::VM_MAXUSER_ADDRESS;
    const VM_MAX_ADDRESS: usize = include::vmparam::VM_MAX_ADDRESS;
    const VM_MIN_KERNEL_ADDRESS: usize = include::vmparam::VM_MIN_KERNEL_ADDRESS;
    const VM_MAX_KERNEL_ADDRESS: usize = include::vmparam::VM_MAX_KERNEL_ADDRESS;
    const VM_PHYSSEG_MAX: usize = include::vmparam::VM_PHYSSEG_MAX;
    const VM_PHYSSEG_STRAT: i32 = include::vmparam::VM_PHYSSEG_STRAT;
    const VM_PHYSSEG_NOADD: bool = include::vmparam::VM_PHYSSEG_NOADD;
}

impl Pmap for Machine {
    type VmPageMd = include::pmap::VmPageMd;
    type Pmap = include::pmap::Pmap;

    #[allow(clippy::declare_interior_mutable_const)] // an initializer, copied into every vm_page
    const VM_MDPAGE_INIT: Self::VmPageMd = include::pmap::VM_MDPAGE_INIT;
    /// The bootloader's direct map, until the kernel owns its page tables
    /// (`arm64/pmap.rs`, deviations).
    const HAVE_PMAP_DIRECT: bool = true;
    const PMAP_STEAL_MEMORY: bool = true;
    const UVM_MD_CONSTRAINTS: &'static [&'static UvmConstraintRange] =
        &arm64::machdep::UVM_MD_CONSTRAINTS;
    const DMA_CONSTRAINT: &'static UvmConstraintRange = &arm64::machdep::DMA_CONSTRAINT;

    fn pmap_kernel() -> &'static Self::Pmap {
        arm64::pmap::pmap_kernel()
    }

    fn pmap_zero_page(pg: &VmPage) {
        arm64::pmap::pmap_zero_page(pg)
    }

    fn pmap_copy_page(src: &VmPage, dst: &VmPage) {
        arm64::pmap::pmap_copy_page(src, dst)
    }

    unsafe fn pmap_steal_memory(
        size: Vsize,
        start: Option<&mut Vaddr>,
        end: Option<&mut Vaddr>,
    ) -> Vaddr {
        // SAFETY: forwarded.
        unsafe { arm64::pmap::pmap_steal_memory(size, start, end) }
    }

    fn pmap_virtual_space(start: &mut Vaddr, end: &mut Vaddr) {
        arm64::pmap::pmap_virtual_space(start, end)
    }

    unsafe fn pmap_kenter_pa(va: Vaddr, pa: Paddr, prot: VmProt) {
        // SAFETY: forwarded.
        unsafe { arm64::pmap::pmap_kenter_pa(va, pa, prot) }
    }

    unsafe fn pmap_kremove(va: Vaddr, len: Vsize) {
        // SAFETY: forwarded.
        unsafe { arm64::pmap::pmap_kremove(va, len) }
    }

    fn pmap_extract(pmap: &Self::Pmap, va: Vaddr) -> Option<Paddr> {
        arm64::pmap::pmap_extract(pmap, va)
    }

    /// `pmap_update`: nothing, as the C.
    fn pmap_update(_pmap: &Self::Pmap) {}

    fn pmap_growkernel(maxkvaddr: Vaddr) -> Vaddr {
        arm64::pmap::pmap_growkernel(maxkvaddr)
    }

    fn pmap_init() {
        arm64::pmap::pmap_init()
    }

    fn pmap_map_direct(pg: &VmPage) -> Vaddr {
        arm64::pmap::pmap_map_direct(pg)
    }

    fn pmap_unmap_direct(va: Vaddr) -> Option<&'static VmPage> {
        arm64::pmap::pmap_unmap_direct(va)
    }
}

impl Console for Machine {
    fn consinit() {
        arm64::machdep::consinit()
    }

    fn cn_rx_intr_establish(_sink: fn(u8)) -> Result<(), Errno> {
        // pluart_fdt's arm_intr_establish_fdt needs the interrupt controller (M4-b, part 2).
        Err(crate::unported!(
            "arm_intr_establish_fdt for the console (ampintc, M4-b part 2)"
        ))
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
    fn pc_regs() -> usize {
        // SAFETY: a read of ddb_regs while the debugger is active, after db_ktrap wrote it.
        include::db_machdep::pc_regs(unsafe { arm64::db_interface::DDB_REGS.get() })
    }
}

impl Intr for Machine {
    const IPL_NONE: i32 = include::intr::IPL_NONE;
    const IPL_SOFTCLOCK: i32 = include::intr::IPL_SOFTCLOCK;
    const IPL_SOFTNET: i32 = include::intr::IPL_SOFTNET;
    const IPL_SOFTTTY: i32 = include::intr::IPL_SOFTTTY;
    const IPL_BIO: i32 = include::intr::IPL_BIO;
    const IPL_NET: i32 = include::intr::IPL_NET;
    const IPL_TTY: i32 = include::intr::IPL_TTY;
    const IPL_VM: i32 = include::intr::IPL_VM;
    const IPL_AUDIO: i32 = include::intr::IPL_AUDIO;
    const IPL_CLOCK: i32 = include::intr::IPL_CLOCK;
    const IPL_SCHED: i32 = include::intr::IPL_SCHED;
    const IPL_STATCLOCK: i32 = include::intr::IPL_STATCLOCK;
    const IPL_HIGH: i32 = include::intr::IPL_HIGH;
    const IPL_IPI: i32 = include::intr::IPL_IPI;
    const IPL_MPFLOOR: i32 = include::intr::IPL_MPFLOOR;
    const IPL_MPSAFE: i32 = include::intr::IPL_MPSAFE;
    const IPL_WAKEUP: i32 = include::intr::IPL_WAKEUP;

    fn splraise(ipl: i32) -> i32 {
        arm64::intr::splraise(ipl)
    }

    fn spllower(ipl: i32) -> i32 {
        arm64::intr::spllower(ipl)
    }

    fn splx(s: i32) {
        arm64::intr::splx(s)
    }

    fn softintr(si: i32) {
        arm64::intr::softintr(si)
    }

    fn splassert_check(wantipl: i32, func: &str) {
        arm64::intr::arm_splassert_check(wantipl, func)
    }
}
