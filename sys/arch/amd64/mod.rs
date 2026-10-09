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
//! amd64 (x86_64) machine-dependent code: OpenBSD `sys/arch/amd64/`.
//!
//! Layout follows OpenBSD: `amd64/` for `.c`/`.S` ports (`locore`, `machdep`, `pmap`, `trap`),
//! `include/` for header ports, `isa/` for the ISA-side clock and RTC, `conf/kernel.ld` for the
//! linker script and `conf/ioconf.rs` for the autoconfiguration tables.

#[allow(clippy::module_inception)] // OpenBSD's layout: sys/arch/amd64/amd64/
pub mod amd64;
pub mod conf;
pub mod include;
pub mod isa;
pub mod pci;

use core::arch::asm;
use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::NonNull;

use self::include::bus;

use crate::dev::pci::pcivar::{PciAttachArgs, PcibusAttachArgs, Pcireg};
use crate::machine::bus::{BusAddr, BusDma, BusSize, BusSpace};
use crate::machine::bus::{BusSpaceHandle, BusSpaceTag};
use crate::machine::cpu::CpuInfo;
use crate::machine::db_machdep::{DbMachdep, PrFn};
use crate::machine::pci_machdep::{PciIntrFn, PciIntrStr, PciMachdep};
use crate::sys::device::Device;

use crate::machine::copy::UserCopy;
use crate::machine::exec::MachineExec;
use crate::machine::proc::MachineProc;
use crate::machine::signal::MachineSignal;
use crate::machine::tcb::Tcb;
use crate::machine::{BootInfo, Console, Cpu, Exit, ExitStatus, Intr, MachineInfo, Pmap, VmParam};
use crate::sys::clockintr::Clockqueue;
use crate::sys::errno::Errno;
use crate::sys::exec::{ExecPackage, PsStrings};
use crate::sys::mbuf::Mbuf;
use crate::sys::proc::{Proc, Process};
use crate::sys::sched::SchedstatePercpu;
use crate::sys::siginfo::Siginfo;
use crate::sys::signal::{Sig, Sigset};
use crate::sys::systm::SysArgs;
use crate::sys::types::{Off, Paddr, Register, Vaddr, Vsize};
use crate::sys::uio::Uio;
use crate::sys::user::User;
use crate::uvm::uvm_extern::{UvmConstraintRange, VmProt, Vmspace};
use crate::uvm::uvm_page::VmPage;

/// The amd64 implementation of the machine interface.
pub struct Machine;

impl MachineInfo for Machine {
    const MACHINE: &'static str = include::param::MACHINE;
    const MACHINE_ARCH: &'static str = include::param::MACHINE_ARCH;
}

impl Cpu for Machine {
    type CpuInfo = include::cpu::CpuInfo;
    type ClockFrame = include::cpu::Clockframe;
    const MAXCPUS: u32 = include::cpu::MAXCPUS;
    const CPU_CHR2BLK: Option<i32> = Some(include::cpu::CPU_CHR2BLK);

    unsafe fn early_init(boot: &BootInfo) -> Result<(), &'static str> {
        // SAFETY: forwarded; `_start` calls this once with the machine as Limine left it.
        unsafe { amd64::machdep::init_x86_64(boot) }
    }

    unsafe fn getbootinfo(arg: usize) -> Result<BootInfo, &'static str> {
        // SAFETY: forwarded; `bootarg_main` calls this once, from `locore0.S`'s `start`.
        unsafe { amd64::machdep::getbootinfo(arg) }
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

    fn cpu_startup() {
        amd64::machdep::cpu_startup()
    }

    fn curcpu() -> &'static include::cpu::CpuInfo {
        include::cpu::curcpu()
    }

    fn curcpu_ptr() -> *const () {
        core::ptr::from_ref(include::cpu::curcpu()).cast()
    }

    fn curcpu_mutex_level_add(delta: i32) {
        let ci = include::cpu::curcpu();
        ci.ci_mutex_level.set(ci.ci_mutex_level.get() + delta);
    }

    fn curcpu_mutex_level() -> i32 {
        include::cpu::curcpu().ci_mutex_level.get()
    }

    fn cpu_info_foreach(f: &mut dyn FnMut(&'static include::cpu::CpuInfo)) {
        let mut ci: *const include::cpu::CpuInfo = include::cpu::cpu_info_primary();
        // SAFETY: `cpu_info_list` links static cpu_infos (just the primary before MP).
        while let Some(info) = unsafe { ci.as_ref() } {
            f(info);
            ci = info.ci_next.get();
        }
    }

    fn cpu_is_primary(ci: &include::cpu::CpuInfo) -> bool {
        include::cpu::cpu_is_primary(ci)
    }

    fn cpu_info_unit(ci: &include::cpu::CpuInfo) -> u32 {
        include::cpu::cpu_info_unit(ci)
    }

    fn ci_queue(ci: &include::cpu::CpuInfo) -> &Clockqueue {
        &ci.ci_queue
    }

    fn ci_schedstate(ci: &include::cpu::CpuInfo) -> &SchedstatePercpu {
        &ci.ci_schedstate
    }

    fn ci_randseed(ci: &include::cpu::CpuInfo) -> &Cell<u32> {
        &ci.ci_randseed
    }

    fn ci_curproc(ci: &include::cpu::CpuInfo) -> *const Proc {
        ci.ci_curproc.get()
    }

    fn set_curproc(ci: &include::cpu::CpuInfo, p: *const Proc) {
        ci.ci_curproc.set(p);
    }

    fn proc0paddr() -> &'static User {
        amd64::machdep::proc0paddr()
    }

    fn ci_idepth(ci: &include::cpu::CpuInfo) -> u32 {
        ci.ci_idepth.get().max(0) as u32
    }

    fn clkf_usermode(frame: &include::cpu::Clockframe) -> bool {
        include::cpu::clkf_usermode(frame)
    }

    fn clkf_pc(frame: &include::cpu::Clockframe) -> usize {
        include::cpu::clkf_pc(frame)
    }

    fn clkf_intr(frame: &include::cpu::Clockframe) -> bool {
        include::cpu::clkf_intr(frame)
    }

    fn need_resched(ci: &include::cpu::CpuInfo) {
        amd64::machdep::need_resched(ci)
    }

    fn clear_resched(ci: &include::cpu::CpuInfo) {
        amd64::machdep::clear_resched(ci)
    }

    fn cpu_sysctl(
        name: &[i32],
        oldp: usize,
        oldlenp: &mut usize,
        newp: usize,
        newlen: usize,
        p: &crate::sys::proc::Proc,
    ) -> Result<(), crate::sys::errno::Errno> {
        amd64::machdep::cpu_sysctl(name, oldp, oldlenp, newp, newlen, p)
    }

    fn cpu_unidle(ci: &include::cpu::CpuInfo) {
        amd64::machdep::cpu_unidle(ci)
    }

    /// `cpu_idle_enter()`: nothing on amd64.
    fn cpu_idle_enter() {}

    fn cpu_idle_cycle() {
        amd64::machdep::cpu_idle_cycle()
    }

    /// `cpu_idle_leave()`: nothing on amd64.
    fn cpu_idle_leave() {}

    unsafe fn cpu_switchto(old: Option<&Proc>, new: &Proc) {
        // SAFETY: forwarded: the caller holds the scheduler lock with `old`/`new` as the
        // contract asks; the assembly only touches their pcbs, the stacks and `%cr3`.
        unsafe {
            amd64::locore::cpu_switchto(
                old.map_or(core::ptr::null(), |p| core::ptr::from_ref(p).cast()),
                core::ptr::from_ref(new).cast(),
            )
        }
    }

    fn cpu_exit(p: &Proc) {
        amd64::vm_machdep::cpu_exit(p)
    }

    fn cpu_fork(
        p1: &Proc,
        p2: &Proc,
        stack: *mut u8,
        tcb: *mut u8,
        func: fn(*mut c_void),
        arg: *mut c_void,
    ) {
        amd64::vm_machdep::cpu_fork(p1, p2, stack, tcb, func, arg)
    }

    fn vmapbuf(bp: &crate::sys::buf::Buf, len: usize) {
        amd64::vm_machdep::vmapbuf(bp, len)
    }

    fn vunmapbuf(bp: &crate::sys::buf::Buf, len: usize) {
        amd64::vm_machdep::vunmapbuf(bp, len)
    }

    fn setregs(p: &Proc, pack: &ExecPackage<'_>, stack: Vaddr, arginfo: &PsStrings) {
        amd64::machdep::setregs(p, pack, stack, arginfo)
    }

    fn signotify(p: &Proc) {
        amd64::machdep::signotify(p)
    }

    fn need_proftick(p: &Proc) {
        amd64::machdep::aston(p)
    }

    fn child_return(arg: *mut c_void) {
        amd64::trap::child_return(arg)
    }

    fn proc_pc(p: &Proc) -> usize {
        // SAFETY: `md_regs` is the thread's trap frame at the top of its u-area (`cpu_fork`),
        // set before the thread first runs in user mode; read without a reference kept.
        unsafe { (*p.p_md.md_regs.get()).tf_rip as usize }
    }

    fn proc_stack(p: &Proc) -> usize {
        // SAFETY: as in `proc_pc`.
        unsafe { (*p.p_md.md_regs.get()).tf_rsp as usize }
    }

    fn cpu_initclocks() {
        amd64::machdep::cpu_initclocks()
    }

    fn cpu_startclock() {
        amd64::machdep::cpu_startclock()
    }

    fn setstatclockrate(newhz: i32) {
        isa::clock::setstatclockrate(newhz)
    }

    fn cpu_configure() {
        amd64::autoconf::cpu_configure()
    }

    fn cpu_number() -> u32 {
        include::cpu::cpu_number()
    }

    fn ci_cpuid(ci: &include::cpu::CpuInfo) -> u32 {
        ci.ci_cpuid.get()
    }

    fn cpu_is_running(ci: &include::cpu::CpuInfo) -> bool {
        include::cpu::cpu_is_running(ci)
    }

    fn intr_disable() -> u64 {
        include::cpufunc::intr_disable()
    }

    unsafe fn intr_restore(s: u64) {
        // SAFETY: forwarded: `s` came from `intr_disable` on this CPU.
        unsafe { include::cpufunc::intr_restore(s) }
    }

    unsafe fn intr_enable() {
        // SAFETY: forwarded: the caller's guarantee.
        unsafe { include::cpufunc::intr_enable() }
    }

    fn kbd_reset() -> Option<&'static core::sync::atomic::AtomicI32> {
        Some(&amd64::machdep::KBD_RESET)
    }

    fn cpu_boot_secondary_processors() {
        amd64::cpu::cpu_boot_secondary_processors()
    }

    unsafe fn cpu_hatch(arg: usize) -> ! {
        // SAFETY: forwarded from the boot glue, with the `cpu_info` `cpu_start_secondary`
        // passed.
        unsafe { amd64::cpu::cpu_hatch_entry(arg) }
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
    const USRSTACK: usize = include::vmparam::USRSTACK;
    const MAXTSIZ: usize = include::vmparam::MAXTSIZ;
    const DFLDSIZ: usize = include::vmparam::DFLDSIZ;
    const MAXDSIZ: usize = include::vmparam::MAXDSIZ;
    const BRKSIZ: usize = include::vmparam::BRKSIZ;
    const DFLSSIZ: usize = include::vmparam::DFLSSIZ;
    const MAXSSIZ: usize = include::vmparam::MAXSSIZ;
    const STACKGAP_RANDOM: usize = include::vmparam::STACKGAP_RANDOM;
    const VM_MIN_STACK_ADDRESS: usize = include::vmparam::VM_MIN_STACK_ADDRESS;
}

impl Pmap for Machine {
    type VmPageMd = include::pmap::VmPageMd;
    type Pmap = include::pmap::Pmap;

    #[allow(clippy::declare_interior_mutable_const)] // an initializer, copied into every vm_page
    const VM_MDPAGE_INIT: Self::VmPageMd = include::pmap::VM_MDPAGE_INIT;
    const HAVE_PMAP_DIRECT: bool = true;
    const PMAP_STEAL_MEMORY: bool = true;
    const PMAP_WC: usize = include::pmap::PMAP_WC as usize;
    const PMAP_NOCACHE: usize = include::pmap::PMAP_NOCACHE as usize;
    const PMAP_NOMMU: bool = false;
    const UVM_MD_CONSTRAINTS: &'static [&'static UvmConstraintRange] =
        &amd64::machdep::UVM_MD_CONSTRAINTS;
    const DMA_CONSTRAINT: &'static UvmConstraintRange = &amd64::machdep::DMA_CONSTRAINT;

    fn pmap_kernel() -> &'static Self::Pmap {
        amd64::pmap::pmap_kernel()
    }

    fn pmap_zero_page(pg: &VmPage) {
        amd64::pmap::pmap_zero_page(pg)
    }

    fn pmap_copy_page(src: &VmPage, dst: &VmPage) {
        amd64::pmap::pmap_copy_page(src, dst)
    }

    fn pmap_page_protect(pg: &VmPage, prot: VmProt) {
        include::pmap::pmap_page_protect(pg, prot)
    }

    fn pmap_clear_modify(pg: &VmPage) -> bool {
        include::pmap::pmap_clear_modify(pg)
    }

    fn pmap_clear_reference(pg: &VmPage) -> bool {
        include::pmap::pmap_clear_reference(pg)
    }

    fn pmap_is_modified(pg: &VmPage) -> bool {
        include::pmap::pmap_is_modified(pg)
    }

    unsafe fn pmap_steal_memory(
        size: Vsize,
        start: Option<&mut Vaddr>,
        end: Option<&mut Vaddr>,
    ) -> Vaddr {
        // SAFETY: forwarded.
        unsafe { amd64::pmap::pmap_steal_memory(size, start, end) }
    }

    fn pmap_virtual_space(start: &mut Vaddr, end: &mut Vaddr) {
        amd64::pmap::pmap_virtual_space(start, end)
    }

    unsafe fn pmap_kenter_pa(va: Vaddr, pa: Paddr, prot: VmProt) {
        // SAFETY: forwarded.
        unsafe { amd64::pmap::pmap_kenter_pa(va, pa, prot) }
    }

    unsafe fn pmap_kremove(va: Vaddr, len: Vsize) {
        // SAFETY: forwarded.
        unsafe { amd64::pmap::pmap_kremove(va, len) }
    }

    fn pmap_extract(pmap: &Self::Pmap, va: Vaddr) -> Option<Paddr> {
        amd64::pmap::pmap_extract(pmap, va)
    }

    fn pmap_create() -> &'static Self::Pmap {
        amd64::pmap::pmap_create()
    }

    fn pmap_destroy(pmap: &'static Self::Pmap) {
        amd64::pmap::pmap_destroy(pmap)
    }

    fn pmap_reference(pmap: &Self::Pmap) {
        amd64::pmap::pmap_reference(pmap)
    }

    fn pmap_enter(
        pmap: &Self::Pmap,
        va: Vaddr,
        pa: Paddr,
        prot: VmProt,
        flags: i32,
    ) -> Result<(), Errno> {
        amd64::pmap::pmap_enter(pmap, va, pa, prot, flags)
    }

    fn pmap_remove(pmap: &Self::Pmap, sva: Vaddr, eva: Vaddr) {
        amd64::pmap::pmap_remove(pmap, sva, eva)
    }

    fn pmap_protect(pmap: &Self::Pmap, sva: Vaddr, eva: Vaddr, prot: VmProt) {
        include::pmap::pmap_protect(pmap, sva, eva, prot)
    }

    fn pmap_wired_count(pmap: &Self::Pmap) -> i64 {
        pmap.pm_stats.wired_count.get()
    }

    fn pmap_resident_count(pmap: &Self::Pmap) -> i64 {
        pmap.pm_stats.resident_count.get()
    }

    fn pmap_unwire(pmap: &Self::Pmap, va: Vaddr) {
        amd64::pmap::pmap_unwire(pmap, va)
    }

    fn pmap_remove_holes(vm: &Vmspace) {
        amd64::pmap::pmap_remove_holes(vm)
    }

    fn pmap_proc_iflush(pr: &Process, va: Vaddr, len: Vsize) {
        amd64::pmap::pmap_proc_iflush(pr, va, len)
    }

    /// `pmap_update`: nothing (yet), as the C macro.
    fn pmap_activate(p: &Proc) {
        amd64::pmap::pmap_activate(p)
    }

    fn pmap_deactivate(p: &Proc) {
        amd64::pmap::pmap_deactivate(p)
    }

    /// No `__HAVE_PMAP_PURGE` on amd64.
    fn pmap_purge(_p: &Proc) {}

    fn pmap_update(_pmap: &Self::Pmap) {}

    fn pmap_growkernel(maxkvaddr: Vaddr) -> Vaddr {
        amd64::pmap::pmap_growkernel(maxkvaddr)
    }

    fn pmap_init() {
        amd64::pmap::pmap_init()
    }

    fn pmap_map_direct(pg: &VmPage) -> Vaddr {
        amd64::pmap::pmap_map_direct(pg)
    }

    fn pmap_unmap_direct(va: Vaddr) -> Option<&'static VmPage> {
        amd64::pmap::pmap_unmap_direct(va)
    }
}

impl MachineExec for Machine {
    const LDPGSZ: usize = include::exec::LDPGSZ;
    const ARCH_ELFSIZE: usize = include::exec::ARCH_ELFSIZE;
    const ELF_TARG_CLASS: u8 = include::exec::ELF_TARG_CLASS;
    const ELF_TARG_DATA: u8 = include::exec::ELF_TARG_DATA;
    const ELF_TARG_MACH: u16 = include::exec::ELF_TARG_MACH;
    const HAVE_CPU_HWCAP: bool = false;
    const HAVE_CPU_HWCAP2: bool = false;
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

/// `<machine/atomic.h>`'s barriers (`arch/amd64/include/atomic.h`, the `__membar`
/// definitions).
impl crate::machine::atomic::Atomic for Machine {
    #[inline]
    fn virtio_membar_producer() {
        // __membar(""): x86 keeps stores in order; only the compiler must not reorder.
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    }

    #[inline]
    fn virtio_membar_consumer() {
        // __membar(""): x86 keeps loads in order.
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    }

    #[inline]
    fn virtio_membar_sync() {
        // SAFETY: `mfence` orders memory accesses and changes nothing else; no `nomem`, so
        // the compiler does not move accesses across it either.
        unsafe { core::arch::asm!("mfence", options(nostack, preserves_flags)) };
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

    fn bus_space_subregion(
        t: Self::Tag,
        h: Self::Handle,
        offset: BusSize,
        size: BusSize,
    ) -> Result<Self::Handle, Errno> {
        amd64::bus_space::bus_space_subregion(t, h, offset, size)
    }

    fn bus_space_vaddr(t: Self::Tag, h: Self::Handle) -> *mut u8 {
        amd64::bus_space::bus_space_vaddr(t, h)
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

    fn bus_space_copy_2(
        t: Self::Tag,
        h1: Self::Handle,
        o1: BusSize,
        h2: Self::Handle,
        o2: BusSize,
        count: usize,
    ) {
        amd64::bus_space::bus_space_copy_2(t, h1, o1, h2, o2, count)
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

impl BusDma for Machine {
    type DmaTag = &'static bus::BusDmaTag;
    type Dmamap = bus::BusDmamap;
    type DmaSegment = bus::BusDmaSegment;

    const BUS_DMA_WAITOK: i32 = bus::BUS_DMA_WAITOK;
    const BUS_DMA_NOWAIT: i32 = bus::BUS_DMA_NOWAIT;
    const BUS_DMA_ALLOCNOW: i32 = bus::BUS_DMA_ALLOCNOW;
    const BUS_DMA_COHERENT: i32 = bus::BUS_DMA_COHERENT;
    const BUS_DMA_BUS1: i32 = bus::BUS_DMA_BUS1;
    const BUS_DMA_BUS2: i32 = bus::BUS_DMA_BUS2;
    const BUS_DMA_STREAMING: i32 = bus::BUS_DMA_STREAMING;
    const BUS_DMA_READ: i32 = bus::BUS_DMA_READ;
    const BUS_DMA_WRITE: i32 = bus::BUS_DMA_WRITE;
    const BUS_DMA_NOCACHE: i32 = bus::BUS_DMA_NOCACHE;
    const BUS_DMA_ZERO: i32 = bus::BUS_DMA_ZERO;
    const BUS_DMA_64BIT: i32 = bus::BUS_DMA_64BIT;
    const BUS_DMASYNC_PREREAD: i32 = bus::BUS_DMASYNC_PREREAD;
    const BUS_DMASYNC_POSTREAD: i32 = bus::BUS_DMASYNC_POSTREAD;
    const BUS_DMASYNC_PREWRITE: i32 = bus::BUS_DMASYNC_PREWRITE;
    const BUS_DMASYNC_POSTWRITE: i32 = bus::BUS_DMASYNC_POSTWRITE;

    fn bus_dmamap_create(
        t: Self::DmaTag,
        size: BusSize,
        nsegments: i32,
        maxsegsz: BusSize,
        boundary: BusSize,
        flags: i32,
    ) -> Result<&'static Self::Dmamap, Errno> {
        (t._dmamap_create)(t, size, nsegments, maxsegsz, boundary, flags)
    }

    unsafe fn bus_dmamap_destroy(t: Self::DmaTag, map: NonNull<Self::Dmamap>) {
        // SAFETY: forwarded.
        unsafe { (t._dmamap_destroy)(t, map) }
    }

    unsafe fn bus_dmamap_load(
        t: Self::DmaTag,
        map: &Self::Dmamap,
        buf: *mut u8,
        buflen: BusSize,
        p: Option<&Proc>,
        flags: i32,
    ) -> Result<(), Errno> {
        // SAFETY: forwarded.
        unsafe { (t._dmamap_load)(t, map, buf, buflen, p, flags) }
    }

    unsafe fn bus_dmamap_load_mbuf(
        t: Self::DmaTag,
        map: &Self::Dmamap,
        m: &Mbuf,
        flags: i32,
    ) -> Result<(), Errno> {
        // SAFETY: forwarded.
        unsafe { (t._dmamap_load_mbuf)(t, map, m, flags) }
    }

    unsafe fn bus_dmamap_load_uio(
        t: Self::DmaTag,
        map: &Self::Dmamap,
        uio: &Uio<'_>,
        flags: i32,
    ) -> Result<(), Errno> {
        // SAFETY: forwarded.
        unsafe { (t._dmamap_load_uio)(t, map, uio, flags) }
    }

    unsafe fn bus_dmamap_load_raw(
        t: Self::DmaTag,
        map: &Self::Dmamap,
        segs: &[Self::DmaSegment],
        size: BusSize,
        flags: i32,
    ) -> Result<(), Errno> {
        // SAFETY: forwarded.
        unsafe { (t._dmamap_load_raw)(t, map, segs, size, flags) }
    }

    fn bus_dmamap_unload(t: Self::DmaTag, map: &Self::Dmamap) {
        (t._dmamap_unload)(t, map)
    }

    fn bus_dmamap_sync(
        t: Self::DmaTag,
        map: &Self::Dmamap,
        offset: BusAddr,
        len: BusSize,
        ops: i32,
    ) {
        (t._dmamap_sync)(t, map, offset, len, ops)
    }

    fn bus_dmamem_alloc(
        t: Self::DmaTag,
        size: BusSize,
        alignment: BusSize,
        boundary: BusSize,
        segs: &mut [Self::DmaSegment],
        flags: i32,
    ) -> Result<usize, Errno> {
        (t._dmamem_alloc)(t, size, alignment, boundary, segs, flags)
    }

    fn bus_dmamem_alloc_range(
        t: Self::DmaTag,
        size: BusSize,
        alignment: BusSize,
        boundary: BusSize,
        segs: &mut [Self::DmaSegment],
        flags: i32,
        low: BusAddr,
        high: BusAddr,
    ) -> Result<usize, Errno> {
        (t._dmamem_alloc_range)(t, size, alignment, boundary, segs, flags, low, high)
    }

    unsafe fn bus_dmamem_free(t: Self::DmaTag, segs: &[Self::DmaSegment]) {
        // SAFETY: forwarded.
        unsafe { (t._dmamem_free)(t, segs) }
    }

    fn bus_dmamem_map(
        t: Self::DmaTag,
        segs: &mut [Self::DmaSegment],
        size: usize,
        flags: i32,
    ) -> Result<NonNull<u8>, Errno> {
        (t._dmamem_map)(t, segs, size, flags)
    }

    unsafe fn bus_dmamem_unmap(t: Self::DmaTag, kva: NonNull<u8>, size: usize) {
        // SAFETY: forwarded.
        unsafe { (t._dmamem_unmap)(t, kva, size) }
    }

    fn bus_dmamem_mmap(
        t: Self::DmaTag,
        segs: &[Self::DmaSegment],
        off: Off,
        prot: i32,
        flags: i32,
    ) -> Option<Paddr> {
        (t._dmamem_mmap)(t, segs, off, prot, flags)
    }
}

impl PciMachdep for Machine {
    type PciChipsetTag = include::pci_machdep::PciChipsetTag;
    type Pcitag = include::pci_machdep::Pcitag;
    type PciIntrHandle = include::pci_machdep::PciIntrHandle;

    const PCI_MSI_PER_BRIDGE: bool = true;
    const PCI_IO_START: u64 = include::pci_machdep::PCI_IO_START;
    const PCI_IO_END: u64 = include::pci_machdep::PCI_IO_END;
    const PCI_MEM_START: u64 = include::pci_machdep::PCI_MEM_START;
    /// `ppb.c`'s default: amd64's `pci_machdep.h` sets no `PCI_MEM_END`.
    const PCI_MEM_END: u64 = 0xffff_ffff;

    fn pci_attach_hook(parent: &Device, self_: &Device, pba: &PcibusAttachArgs) {
        pci::pci_machdep::pci_attach_hook(parent, self_, pba)
    }

    fn pci_bus_maxdevs(pc: Self::PciChipsetTag, busno: i32) -> i32 {
        pci::pci_machdep::pci_bus_maxdevs(pc, busno)
    }

    fn pci_lookup_segment(segment: i32, bus: i32) -> Option<Self::PciChipsetTag> {
        Some(pci::pci_machdep::pci_lookup_segment(segment, bus))
    }

    fn pci_mcfg_init(iot: BusSpaceTag, addr: BusAddr, segment: i32, min_bus: i32, max_bus: i32) {
        pci::pci_machdep::pci_mcfg_init(iot, addr, segment, min_bus, max_bus)
    }

    fn pci_make_tag(pc: Self::PciChipsetTag, bus: i32, device: i32, function: i32) -> Self::Pcitag {
        pci::pci_machdep::pci_make_tag(pc, bus, device, function)
    }

    fn pci_decompose_tag(pc: Self::PciChipsetTag, tag: Self::Pcitag) -> (i32, i32, i32) {
        pci::pci_machdep::pci_decompose_tag(pc, tag)
    }

    /// No device tree on amd64 (`__HAVE_FDT` is not defined).
    fn pcitag_node(_tag: Self::Pcitag) -> i32 {
        0
    }

    fn pci_conf_size(pc: Self::PciChipsetTag, tag: Self::Pcitag) -> i32 {
        pci::pci_machdep::pci_conf_size(pc, tag)
    }

    fn pci_conf_read(pc: Self::PciChipsetTag, tag: Self::Pcitag, reg: i32) -> Pcireg {
        pci::pci_machdep::pci_conf_read(pc, tag, reg)
    }

    fn pci_conf_write(pc: Self::PciChipsetTag, tag: Self::Pcitag, reg: i32, data: Pcireg) {
        pci::pci_machdep::pci_conf_write(pc, tag, reg, data)
    }

    fn pci_probe_device_hook(pc: Self::PciChipsetTag, pa: &mut PciAttachArgs) -> i32 {
        pci::pci_machdep::pci_probe_device_hook(pc, pa)
    }

    fn pci_dev_postattach(dev: &Device, pa: &PciAttachArgs) {
        pci::pci_machdep::pci_dev_postattach(dev, pa)
    }

    fn pci_min_powerstate(pc: Self::PciChipsetTag, tag: Self::Pcitag) -> Pcireg {
        pci::pci_machdep::pci_min_powerstate(pc, tag)
    }

    fn pci_set_powerstate_md(pc: Self::PciChipsetTag, tag: Self::Pcitag, state: i32, pre: i32) {
        pci::pci_machdep::pci_set_powerstate_md(pc, tag, state, pre)
    }

    fn pci_msix_table_map(
        pc: Self::PciChipsetTag,
        tag: Self::Pcitag,
        memt: BusSpaceTag,
    ) -> Result<BusSpaceHandle, Errno> {
        pci::pci_machdep::pci_msix_table_map(pc, tag, memt)
    }

    fn pci_msix_table_unmap(
        pc: Self::PciChipsetTag,
        tag: Self::Pcitag,
        memt: BusSpaceTag,
        memh: BusSpaceHandle,
    ) {
        pci::pci_machdep::pci_msix_table_unmap(pc, tag, memt, memh)
    }

    fn pci_intr_enable_msivec(pa: &PciAttachArgs, num_vec: i32) -> bool {
        pci::pci_machdep::pci_intr_enable_msivec(pa, num_vec)
    }

    fn pci_intr_map_msi(pa: &PciAttachArgs) -> Option<Self::PciIntrHandle> {
        pci::pci_machdep::pci_intr_map_msi(pa)
    }

    fn pci_intr_map_msivec(pa: &PciAttachArgs, vec: i32) -> Option<Self::PciIntrHandle> {
        pci::pci_machdep::pci_intr_map_msivec(pa, vec)
    }

    fn pci_intr_map_msix(pa: &PciAttachArgs, vec: i32) -> Option<Self::PciIntrHandle> {
        pci::pci_machdep::pci_intr_map_msix(pa, vec)
    }

    fn pci_intr_map(pa: &PciAttachArgs) -> Option<Self::PciIntrHandle> {
        pci::pci_machdep::pci_intr_map(pa)
    }

    fn pci_intr_string(pc: Self::PciChipsetTag, ih: Self::PciIntrHandle) -> PciIntrStr {
        pci::pci_machdep::pci_intr_string(pc, ih)
    }

    fn pci_intr_establish_cpu(
        pc: Self::PciChipsetTag,
        ih: Self::PciIntrHandle,
        level: i32,
        ci: Option<&'static CpuInfo>,
        func: PciIntrFn,
        arg: *mut c_void,
        what: &'static str,
    ) -> Option<NonNull<c_void>> {
        pci::pci_machdep::pci_intr_establish_cpu(pc, ih, level, ci, func, arg, what)
            .map(NonNull::cast)
    }

    unsafe fn pci_intr_disestablish(pc: Self::PciChipsetTag, cookie: NonNull<c_void>) {
        // SAFETY: forwarded; the cookie is the Intrhand pci_intr_establish_cpu returned.
        unsafe { pci::pci_machdep::pci_intr_disestablish(pc, cookie.cast()) }
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
    fn pc_regs() -> usize {
        // SAFETY: a read of ddb_regs while the debugger is active, after db_ktrap wrote it.
        include::db_machdep::pc_regs(unsafe { amd64::db_interface::DDB_REGS.get() })
    }

    fn db_regs() -> &'static [crate::ddb::db_variables::DbVariable] {
        &amd64::db_trace::DB_REGS
    }

    const DB_MACHINE_COMMAND_TABLE: &'static [crate::ddb::db_command::DbCommand] =
        amd64::db_interface::DB_MACHINE_COMMAND_TABLE;

    fn db_machine_command_table() -> &'static [crate::ddb::db_command::DbCommand] {
        Self::DB_MACHINE_COMMAND_TABLE
    }

    fn db_machine_init() {
        amd64::db_interface::db_machine_init()
    }

    fn set_pc_regs(pc: usize) {
        // SAFETY: ddb_regs is only used by the CPU in the debugger, and no other reference
        // to it is live during this access.
        include::db_machdep::set_pc_regs(unsafe { amd64::db_interface::DDB_REGS.get_mut() }, pc)
    }

    fn fixup_pc_after_break() {
        // SAFETY: as in set_pc_regs.
        include::db_machdep::fixup_pc_after_break(unsafe {
            amd64::db_interface::DDB_REGS.get_mut()
        })
    }

    fn db_set_single_step() {
        // SAFETY: as in set_pc_regs.
        include::db_machdep::db_set_single_step(unsafe { amd64::db_interface::DDB_REGS.get_mut() })
    }

    fn db_clear_single_step() {
        // SAFETY: as in set_pc_regs.
        include::db_machdep::db_clear_single_step(unsafe {
            amd64::db_interface::DDB_REGS.get_mut()
        })
    }

    fn is_breakpoint_trap(type_: i32, code: i32) -> bool {
        include::db_machdep::is_breakpoint_trap(type_, code)
    }

    fn is_watchpoint_trap(type_: i32, code: i32) -> bool {
        include::db_machdep::is_watchpoint_trap(type_, code)
    }

    fn inst_trap_return(ins: i64) -> bool {
        include::db_machdep::inst_trap_return(ins)
    }

    fn inst_return(ins: i64) -> bool {
        include::db_machdep::inst_return(ins)
    }

    fn inst_call(ins: i64) -> bool {
        include::db_machdep::inst_call(ins)
    }
}

impl Intr for Machine {
    const IPL_NONE: i32 = include::intrdefs::IPL_NONE;
    const IPL_SOFTCLOCK: i32 = include::intrdefs::IPL_SOFTCLOCK;
    const IPL_SOFTNET: i32 = include::intrdefs::IPL_SOFTNET;
    const IPL_SOFTTTY: i32 = include::intrdefs::IPL_SOFTTTY;
    const IPL_BIO: i32 = include::intrdefs::IPL_BIO;
    const IPL_NET: i32 = include::intrdefs::IPL_NET;
    const IPL_TTY: i32 = include::intrdefs::IPL_TTY;
    const IPL_VM: i32 = include::intrdefs::IPL_VM;
    const IPL_AUDIO: i32 = include::intrdefs::IPL_AUDIO;
    const IPL_CLOCK: i32 = include::intrdefs::IPL_CLOCK;
    const IPL_SCHED: i32 = include::intrdefs::IPL_SCHED;
    const IPL_STATCLOCK: i32 = include::intrdefs::IPL_STATCLOCK;
    const IPL_HIGH: i32 = include::intrdefs::IPL_HIGH;
    const IPL_IPI: i32 = include::intrdefs::IPL_IPI;
    const IPL_MPFLOOR: i32 = include::intrdefs::IPL_MPFLOOR;
    const IPL_MPSAFE: i32 = include::intrdefs::IPL_MPSAFE;
    const IPL_WAKEUP: i32 = include::intrdefs::IPL_WAKEUP;

    fn splraise(ipl: i32) -> i32 {
        amd64::intr::splraise(ipl)
    }

    fn spllower(ipl: i32) -> i32 {
        amd64::intr::spllower(ipl)
    }

    fn splx(s: i32) {
        amd64::intr::spllower(s);
    }

    fn softintr(si: i32) {
        amd64::intr::softintr(si)
    }

    fn splassert_check(wantipl: i32, func: &str) {
        amd64::machdep::splassert_check(wantipl, func)
    }

    fn intr_barrier(cookie: NonNull<c_void>) {
        amd64::intr::intr_barrier(cookie.cast())
    }
}

/// The ISA bus behind the legacy 8259s (`arch/amd64/isa/isa_machdep.c`).
impl crate::machine::isa_machdep::IsaMachdep for Machine {
    type IsaChipsetTag = isa::isa_machdep::IsaChipsetTag;

    const IST_NONE: i32 = include::intrdefs::IST_NONE;
    const IST_PULSE: i32 = include::intrdefs::IST_PULSE;
    const IST_EDGE: i32 = include::intrdefs::IST_EDGE;
    const IST_LEVEL: i32 = include::intrdefs::IST_LEVEL;

    fn isa_attach_hook(
        _parent: Option<&crate::sys::device::Device>,
        _self: &crate::sys::device::Device,
    ) {
        isa::isa_machdep::isa_attach_hook()
    }

    fn isa_intr_check(ic: Self::IsaChipsetTag, irq: i32, type_: i32) -> i32 {
        isa::isa_machdep::isa_intr_check(ic, irq, type_)
    }

    fn isa_intr_establish(
        ic: Self::IsaChipsetTag,
        irq: i32,
        type_: i32,
        level: i32,
        ih_fun: fn(*mut c_void) -> i32,
        ih_arg: *mut c_void,
        ih_what: &'static str,
    ) -> Option<core::ptr::NonNull<c_void>> {
        isa::isa_machdep::isa_intr_establish(ic, irq, type_, level, ih_fun, ih_arg, ih_what)
            .map(|ih| ih.cast())
    }
}

/// amd64 has no device tree: ACPI describes the machine (M5).
impl crate::machine::acpi_machdep::AcpiMachdep for Machine {
    const ACPI_PRT: bool = true;
    const ACPI_SECTWO: bool = false;

    fn acpi_map(pa: Paddr, len: usize) -> Result<crate::dev::acpi::acpivar::AcpiMemMap, Errno> {
        amd64::acpi_machdep::acpi_map(pa, len)
    }

    fn acpi_unmap(handle: &crate::dev::acpi::acpivar::AcpiMemMap) {
        amd64::acpi_machdep::acpi_unmap(handle)
    }

    unsafe fn acpi_bus_space_map(
        t: BusSpaceTag,
        addr: BusAddr,
        size: BusSize,
        flags: i32,
    ) -> Result<BusSpaceHandle, Errno> {
        // SAFETY: forwarded.
        unsafe { amd64::acpi_machdep::acpi_bus_space_map(t, addr, size, flags) }
    }

    fn acpi_bus_space_unmap(t: BusSpaceTag, bsh: BusSpaceHandle, size: BusSize) {
        amd64::acpi_machdep::acpi_bus_space_unmap(t, bsh, size)
    }

    fn acpi_intr_establish(
        irq: i32,
        flags: i32,
        level: i32,
        handler: fn(*mut c_void) -> i32,
        arg: *mut c_void,
        what: &'static str,
    ) -> Option<NonNull<c_void>> {
        amd64::acpi_machdep::acpi_intr_establish(irq, flags, level, handler, arg, what)
    }

    unsafe fn acpi_intr_disestablish(cookie: NonNull<c_void>) {
        // SAFETY: forwarded.
        unsafe { amd64::acpi_machdep::acpi_intr_disestablish(cookie) }
    }

    fn acpi_attach_machdep(sc: &'static crate::dev::acpi::acpivar::AcpiSoftc) {
        amd64::acpi_machdep::acpi_attach_machdep(sc)
    }

    unsafe fn acpi_acquire_glk(lock: *mut u32) -> i32 {
        // SAFETY: forwarded.
        unsafe { amd64::acpi_machdep::acpi_acquire_glk(lock) }
    }

    unsafe fn acpi_release_glk(lock: *mut u32) -> i32 {
        // SAFETY: forwarded.
        unsafe { amd64::acpi_machdep::acpi_release_glk(lock) }
    }

    fn acpi_iommu_device_map(
        node: &crate::dev::acpi::amltypes::AmlNodeRef,
        dmat: Option<crate::machine::bus::BusDmaTag>,
    ) -> Option<crate::machine::bus::BusDmaTag> {
        amd64::acpi_machdep::acpi_iommu_device_map(node, dmat)
    }

    fn pwr_action() -> i32 {
        amd64::machdep::PWR_ACTION.load(core::sync::atomic::Ordering::Relaxed)
    }

    fn ci_acpi_proc_id(ci: &CpuInfo) -> u32 {
        ci.ci_acpi_proc_id.get()
    }

    fn cpu_suspended() -> &'static core::sync::atomic::AtomicI32 {
        &amd64::cpu::CPU_SUSPENDED
    }

    fn delay_init(f: fn(i32), fn_quality: i32) {
        amd64::machdep::delay_init(f, fn_quality)
    }

    fn delay_fini(f: fn(i32)) {
        amd64::machdep::delay_fini(f)
    }

    fn cpu_recalibrate_tsc(tc: &'static crate::sys::timetc::Timecounter) {
        amd64::tsc::cpu_recalibrate_tsc(tc)
    }
}

/// The MP configuration (`mainbus.c`'s `mp_*` globals), the local APIC and the I/O APICs
/// (`lapic.c`, `ioapic.c`) as the x86 ACPI drivers see them.
impl crate::machine::mpconfig::MpConfig for Machine {
    type Ioapic = include::i82093var::IoapicSoftc;

    const MPS_INTPO_DEF: i32 = include::mpbiosreg::MPS_INTPO_DEF;
    const MPS_INTPO_ACTHI: i32 = include::mpbiosreg::MPS_INTPO_ACTHI;
    const MPS_INTPO_ACTLO: i32 = include::mpbiosreg::MPS_INTPO_ACTLO;
    const MPS_INTPO_SHIFT: i32 = include::mpbiosreg::MPS_INTPO_SHIFT;
    const MPS_INTPO_MASK: i32 = include::mpbiosreg::MPS_INTPO_MASK;
    const MPS_INTTR_DEF: i32 = include::mpbiosreg::MPS_INTTR_DEF;
    const MPS_INTTR_EDGE: i32 = include::mpbiosreg::MPS_INTTR_EDGE;
    const MPS_INTTR_LEVEL: i32 = include::mpbiosreg::MPS_INTTR_LEVEL;
    const MPS_INTTR_SHIFT: i32 = include::mpbiosreg::MPS_INTTR_SHIFT;
    const MPS_INTTR_MASK: i32 = include::mpbiosreg::MPS_INTTR_MASK;
    const IOAPIC_REDLO_DEL_MASK: u32 = include::i82093reg::IOAPIC_REDLO_DEL_MASK;
    const IOAPIC_REDLO_DEL_SHIFT: u32 = include::i82093reg::IOAPIC_REDLO_DEL_SHIFT;
    const IOAPIC_REDLO_DEL_LOPRI: u32 = include::i82093reg::IOAPIC_REDLO_DEL_LOPRI;
    const IOAPIC_REDLO_DEL_NMI: u32 = include::i82093reg::IOAPIC_REDLO_DEL_NMI;
    const IOAPIC_REDLO_ACTLO: u32 = include::i82093reg::IOAPIC_REDLO_ACTLO;
    const IOAPIC_REDLO_LEVEL: u32 = include::i82093reg::IOAPIC_REDLO_LEVEL;
    const APIC_INT_VIA_APIC: i32 = include::i82093var::APIC_INT_VIA_APIC;
    const APIC_INT_APIC_SHIFT: i32 = include::i82093var::APIC_INT_APIC_SHIFT;
    const APIC_INT_PIN_SHIFT: i32 = include::i82093var::APIC_INT_PIN_SHIFT;
    const ICU_LEN: i32 = include::i8259::ICU_LEN;
    const NIOAPIC: bool = true;

    fn lapic_boot_init(lapic_base: Paddr) {
        amd64::lapic::lapic_boot_init(lapic_base)
    }

    fn lapic_cpu_number() -> u32 {
        amd64::lapic::lapic_cpu_number()
    }

    fn mp_attach_cpu(
        parent: &Device,
        apic_id: u32,
        acpi_proc_id: u32,
        bp: bool,
        print: crate::sys::device::CfprintT,
    ) {
        use include::cpuvar::{CPU_ROLE_AP, CPU_ROLE_BP, CpuAttachArgs};

        let mut caa = CpuAttachArgs {
            caa_name: b"cpu",
            cpu_apicid: apic_id as i32,
            cpu_acpi_proc_id: acpi_proc_id as i32,
            cpu_role: if bp { CPU_ROLE_BP } else { CPU_ROLE_AP },
            // MULTIPROCESSOR: caa.cpu_func = &mp_cpu_funcs
            #[cfg(feature = "multiprocessor")]
            cpu_func: Some(&amd64::cpu::MP_CPU_FUNCS),
            #[cfg(not(feature = "multiprocessor"))]
            cpu_func: None,
        };
        let _ = crate::kern::subr_autoconf::config_found(
            parent,
            core::ptr::from_mut(&mut caa).cast(),
            Some(print),
        );
    }

    fn mp_attach_ioapic(
        parent: &Device,
        memt: BusSpaceTag,
        apic_id: i32,
        address: BusAddr,
        vecbase: i32,
        print: crate::sys::device::CfprintT,
    ) {
        let mut aaa = include::apicvar::ApicAttachArgs {
            aaa_name: b"ioapic",
            apic_id,
            apic_version: 0,
            flags: 0,
            apic_memt: memt,
            apic_address: address,
            apic_vecbase: vecbase,
        };
        let _ = crate::kern::subr_autoconf::config_found(
            parent,
            core::ptr::from_mut(&mut aaa).cast(),
            Some(print),
        );
    }

    fn ioapic_find_bybase(vec: i32) -> Option<&'static Self::Ioapic> {
        amd64::ioapic::ioapic_find_bybase(vec)
    }

    fn ioapic_apicid(apic: &Self::Ioapic) -> i32 {
        apic.sc_apicid.get()
    }

    fn ioapic_vecbase(apic: &Self::Ioapic) -> i32 {
        apic.sc_apic_vecbase.get()
    }

    fn ioapic_set_ip_map(
        apic: &Self::Ioapic,
        pin: i32,
        map: &'static crate::machine::mpconfig::MpIntrMap,
    ) {
        if let Some(pp) = apic.pins().get(pin as usize) {
            pp.ip_map.set(Some(map));
        }
    }

    fn nioapics() -> i32 {
        amd64::ioapic::NIOAPICS.load(core::sync::atomic::Ordering::Relaxed)
    }

    fn mp_set_busses(
        busses: &'static [crate::machine::mpconfig::MpBus],
        isa: &'static crate::machine::mpconfig::MpBus,
    ) {
        amd64::mainbus::mp_set_busses(busses, isa)
    }

    fn mp_set_intrs(intrs: &'static [crate::machine::mpconfig::MpIntrMap]) {
        amd64::mainbus::mp_set_intrs(intrs)
    }

    fn mp_busses() -> Option<&'static [crate::machine::mpconfig::MpBus]> {
        amd64::mainbus::mp_busses()
    }
}

impl crate::machine::fdt::Fdt for Machine {
    type FdtAttachArgs<'a> = crate::machine::fdt::NoFdtAttachArgs<'a>;

    fn fdt_find_cons(_name: &[u8]) -> crate::dev::ofw::fdt::FdtNode {
        core::ptr::null()
    }

    fn stdout_node() -> i32 {
        0
    }

    fn fdt_cons_bs_tag() -> crate::machine::bus::BusSpaceTag {
        amd64::bus_space::X86_BUS_SPACE_IO
    }

    fn fdt_intr_establish(
        _node: i32,
        _level: i32,
        _func: crate::machine::intr::IntrFn,
        _arg: *mut c_void,
        _name: &'static str,
    ) -> Option<NonNull<c_void>> {
        None
    }

    fn fdt_intr_establish_imap_cpu(
        _node: i32,
        _reg: &[u32],
        _level: i32,
        _ci: Option<&'static crate::machine::cpu::CpuInfo>,
        _func: crate::machine::intr::IntrFn,
        _arg: *mut c_void,
        _name: &'static str,
    ) -> Option<NonNull<c_void>> {
        None
    }

    fn fdt_intr_establish_msi_cpu(
        _node: i32,
        _addr: &mut u64,
        _data: &mut u64,
        _level: i32,
        _ci: Option<&'static crate::machine::cpu::CpuInfo>,
        _func: crate::machine::intr::IntrFn,
        _arg: *mut c_void,
        _name: &'static str,
    ) -> Option<NonNull<c_void>> {
        None
    }

    /// No secure monitor: `PSCI_NOT_SUPPORTED` (-1); only `psci(4)` calls it.
    fn smc_call(_a0: u64, _a1: u64, _a2: u64, _a3: u64) -> u64 {
        u64::MAX
    }

    /// No hypervisor conduit: `PSCI_NOT_SUPPORTED` (-1).
    fn hvc_call(_a0: u64, _a1: u64, _a2: u64, _a3: u64) -> u64 {
        u64::MAX
    }

    /// No device-tree driver registers a reset function here (amd64 uses ACPI's `CPURESETFN`).
    fn set_cpuresetfn(_f: fn()) {}

    /// No device-tree driver registers a power-off function here.
    fn set_powerdownfn(_f: fn()) {}

    fn lid_action() -> i32 {
        amd64::machdep::LID_ACTION.load(core::sync::atomic::Ordering::Relaxed)
    }

    unsafe fn fdt_intr_disestablish(_cookie: NonNull<c_void>) {}
}

/// The autoconfiguration tables `config(8)` would generate (`conf/ioconf.rs`) and the
/// `autoconf.c` hook.
impl crate::machine::autoconf::Autoconf for Machine {
    fn cfdata() -> &'static [crate::sys::device::Cfdata] {
        // SAFETY: `user_config`, the one writer (through `ioconf_mut`), runs in `cpu_startup`
        // before anything reads the tables, and its borrow ends there.
        crate::machine::autoconf::ioconf_cfdata(unsafe { conf::ioconf::CFDATA.get() })
    }

    fn cfroots() -> &'static [i16] {
        // SAFETY: as for `cfdata`.
        unsafe { conf::ioconf::CFROOTS.get() }
    }

    unsafe fn ioconf_mut() -> crate::machine::autoconf::IoconfTables<'static> {
        // SAFETY: the caller's contract: no other reference to the tables exists yet, and
        // this one is dropped before anybody else reads them.
        unsafe {
            crate::machine::autoconf::IoconfTables {
                cfdata: conf::ioconf::CFDATA.get_mut(),
                cfroots: conf::ioconf::CFROOTS.get_mut(),
                pdevinit: conf::ioconf::PDEVINIT.get_mut(),
                pdevnames: &conf::ioconf::PDEVNAMES,
                locnames: &conf::ioconf::LOCNAMES,
                locnamp: &conf::ioconf::LOCNAMP,
            }
        }
    }

    fn mainbus_cd() -> &'static crate::sys::device::Cfdriver {
        &amd64::mainbus::MAINBUS_CD
    }

    fn device_register(dev: &crate::sys::device::Device, aux: *mut c_void) {
        amd64::autoconf::device_register(dev, aux)
    }

    fn pdevinit() -> &'static [crate::sys::device::Pdevinit] {
        // SAFETY: as for `cfdata`.
        unsafe { conf::ioconf::PDEVINIT.get() }
    }

    fn nam2blk() -> &'static [crate::sys::device::Nam2blk] {
        &amd64::autoconf::NAM2BLK
    }

    fn diskconf() {
        amd64::autoconf::diskconf()
    }
}

/// The disk label location (`<machine/disklabel.h>`) and label I/O (`disksubr.c`).
impl crate::machine::disklabel::MachineDisklabel for Machine {
    const LABELSECTOR: u64 = include::disklabel::LABELSECTOR;
    const LABELOFFSET: usize = include::disklabel::LABELOFFSET;
    const MAXPARTITIONS: usize = include::disklabel::MAXPARTITIONS;

    fn readdisklabel(
        dev: crate::sys::types::Dev,
        strat: crate::sys::conf::DevTypeStrategy,
        lp: &mut crate::sys::disklabel::Disklabel,
        spoofonly: bool,
    ) -> Result<(), crate::sys::errno::Errno> {
        amd64::disksubr::readdisklabel(dev, strat, lp, spoofonly)
    }

    fn writedisklabel(
        dev: crate::sys::types::Dev,
        strat: crate::sys::conf::DevTypeStrategy,
        lp: &mut crate::sys::disklabel::Disklabel,
    ) -> Result<(), crate::sys::errno::Errno> {
        amd64::disksubr::writedisklabel(dev, strat, lp)
    }
}

/// The device switch tables (`amd64/amd64/conf.c`).
impl crate::machine::conf::Conf for Machine {
    fn nchrdev() -> u32 {
        amd64::conf::CDEVSW.len() as u32
    }

    fn cdevsw(maj: u32) -> Option<crate::sys::conf::Cdevsw> {
        amd64::conf::CDEVSW.get(maj)
    }

    fn cdevsw_set(maj: u32, sw: crate::sys::conf::Cdevsw) {
        amd64::conf::CDEVSW.set(maj, sw)
    }

    fn nblkdev() -> u32 {
        amd64::conf::BDEVSW.len() as u32
    }

    fn bdevsw(maj: u32) -> Option<crate::sys::conf::Bdevsw> {
        amd64::conf::BDEVSW.get(maj)
    }

    fn chrtoblktbl() -> &'static [crate::sys::types::Dev] {
        &amd64::conf::CHRTOBLKTBL
    }

    fn swapdev() -> crate::sys::types::Dev {
        amd64::conf::SWAPDEV
    }

    fn mem_no() -> u32 {
        amd64::conf::MEM_NO
    }

    fn iskmemdev(dev: crate::sys::types::Dev) -> bool {
        amd64::conf::iskmemdev(dev)
    }

    fn iszerodev(dev: crate::sys::types::Dev) -> bool {
        amd64::conf::iszerodev(dev)
    }

    fn getnulldev() -> crate::sys::types::Dev {
        amd64::conf::getnulldev()
    }
}

impl MachineProc for Machine {
    type Mdproc = include::proc::Mdproc;
    const MDPROC_INIT: include::proc::Mdproc = include::proc::Mdproc::new();
    type Pcb = include::pcb::Pcb;
    const PCB_INIT: include::pcb::Pcb = include::pcb::Pcb::new();
}

impl Tcb for Machine {
    fn tcb_get(p: &Proc) -> usize {
        amd64::vm_machdep::tcb_get(p)
    }

    fn tcb_set(p: &Proc, addr: usize) {
        amd64::vm_machdep::tcb_set(p, addr)
    }

    fn tcb_invalid(addr: usize) -> bool {
        include::tcb::tcb_invalid(addr)
    }
}

impl UserCopy for Machine {
    fn copyin(uaddr: usize, kbuf: &mut [u8]) -> Result<(), Errno> {
        amd64::copy::copyin(uaddr, kbuf)
    }

    fn copyout(kbuf: &[u8], uaddr: usize) -> Result<(), Errno> {
        amd64::copy::copyout(kbuf, uaddr)
    }

    fn copyinstr(uaddr: usize, kbuf: &mut [u8]) -> Result<usize, Errno> {
        amd64::copy::copyinstr(uaddr, kbuf)
    }

    fn copyin32(uaddr: usize) -> Result<u32, Errno> {
        amd64::machdep::copyin32(uaddr)
    }

    fn copyoutstr(kbuf: &[u8], uaddr: usize) -> Result<usize, Errno> {
        amd64::copy::copyoutstr(kbuf, uaddr)
    }

    unsafe fn kcopy(src: *const u8, dst: *mut u8, len: usize) -> Result<(), Errno> {
        // SAFETY: forwarded.
        unsafe { amd64::copy::kcopy(src, dst, len) }
    }
}

impl MachineSignal for Machine {
    type Sigcontext = include::signal::Sigcontext;

    fn sendsig(
        catcher: Sig,
        sig: i32,
        mask: Sigset,
        ksip: &Siginfo,
        info: bool,
        onstack: bool,
    ) -> Result<(), Errno> {
        amd64::machdep::sendsig(catcher, sig, mask, ksip, info, onstack)
    }

    fn sys_sigreturn(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
        amd64::machdep::sys_sigreturn(p, v, retval)
    }

    fn sigcode() -> &'static [u8] {
        amd64::locore::sigcode_bytes()
    }

    fn sigcoderet() -> usize {
        amd64::locore::sigcoderet_offset()
    }

    fn sigcodecall() -> usize {
        amd64::locore::sigcodecall_offset()
    }

    fn sigfill() -> &'static [u8] {
        amd64::locore::sigfill_bytes()
    }
}
/* </CODE> */
