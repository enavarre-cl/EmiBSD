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
//! arm64 (aarch64) machine-dependent code: OpenBSD `sys/arch/arm64/`.
//!
//! Layout follows OpenBSD: `arm64/` for `.c`/`.S` ports (`locore`, `machdep`, `pmap`, `trap`),
//! `include/` for header ports, `dev/` for arch-only drivers (GIC, generic timer),
//! `conf/kernel.ld` for the linker script and `conf/ioconf.rs` for the autoconfiguration
//! tables.

#[allow(clippy::module_inception)] // OpenBSD's layout: sys/arch/arm64/arm64/
pub mod arm64;
pub mod conf;
pub mod dev;
pub mod include;

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

/// The arm64 implementation of the machine interface.
pub struct Machine;

impl MachineInfo for Machine {
    const MACHINE: &'static str = include::param::MACHINE;
    const MACHINE_ARCH: &'static str = include::param::MACHINE_ARCH;
}

impl Cpu for Machine {
    type CpuInfo = include::cpu::CpuInfo;
    type ClockFrame = include::cpu::Clockframe;
    const MAXCPUS: u32 = include::cpu::MAXCPUS;
    const CPU_ID_AA64ISAR0: Option<i32> = Some(include::cpu::CPU_ID_AA64ISAR0);
    const CPU_ID_AA64ISAR1: Option<i32> = Some(include::cpu::CPU_ID_AA64ISAR1);

    unsafe fn early_init(boot: &BootInfo) -> Result<(), &'static str> {
        // SAFETY: forwarded; `_start` calls this once with the machine as Limine left it.
        unsafe { arm64::machdep::initarm(boot) }
    }

    /// arm64's entry from efiboot: `locore0.S`'s `_start` passes its `arm64_bootparams`
    /// (`machdep.rs`'s `getbootinfo`, the `/chosen` half of `initarm`).
    unsafe fn getbootinfo(arg: usize) -> Result<BootInfo, &'static str> {
        // SAFETY: forwarded; `bootarg_main` calls this once with locore0.S's parameters.
        unsafe { arm64::machdep::getbootinfo(arg) }
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
        arm64::machdep::proc0paddr()
    }

    fn cpu_rnd_messybits() -> u32 {
        include::cpu::cpu_rnd_messybits()
    }

    fn etext() -> usize {
        arm64::machdep::etext()
    }

    fn ci_idepth(ci: &include::cpu::CpuInfo) -> u32 {
        ci.ci_idepth.get()
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
        arm64::machdep::need_resched(ci)
    }

    fn clear_resched(ci: &include::cpu::CpuInfo) {
        arm64::machdep::clear_resched(ci)
    }

    fn cpu_sysctl(
        name: &[i32],
        oldp: usize,
        oldlenp: &mut usize,
        newp: usize,
        newlen: usize,
        p: &crate::sys::proc::Proc,
    ) -> Result<(), crate::sys::errno::Errno> {
        arm64::machdep::cpu_sysctl(name, oldp, oldlenp, newp, newlen, p)
    }

    fn cpu_unidle(ci: &include::cpu::CpuInfo) {
        arm64::cpu::cpu_unidle(ci)
    }

    fn cpu_idle_enter() {
        arm64::machdep::cpu_idle_enter()
    }

    fn cpu_idle_cycle() {
        arm64::machdep::cpu_idle_cycle()
    }

    fn cpu_idle_leave() {
        arm64::machdep::cpu_idle_leave()
    }

    unsafe fn cpu_switchto(old: Option<&Proc>, new: &Proc) {
        // SAFETY: forwarded: the caller holds the scheduler lock with `old`/`new` as the
        // contract asks.
        unsafe { arm64::machdep::cpu_switchto(old, new) }
    }

    fn cpu_exit(p: &Proc) {
        arm64::vm_machdep::cpu_exit(p)
    }

    fn cpu_fork(
        p1: &Proc,
        p2: &Proc,
        stack: *mut u8,
        tcb: *mut u8,
        func: fn(*mut c_void),
        arg: *mut c_void,
    ) {
        arm64::vm_machdep::cpu_fork(p1, p2, stack, tcb, func, arg)
    }

    fn vmapbuf(bp: &crate::sys::buf::Buf, len: usize) {
        arm64::vm_machdep::vmapbuf(bp, len)
    }

    fn vunmapbuf(bp: &crate::sys::buf::Buf, len: usize) {
        arm64::vm_machdep::vunmapbuf(bp, len)
    }

    fn setregs(p: &Proc, pack: &ExecPackage<'_>, stack: Vaddr, arginfo: &PsStrings) {
        arm64::machdep::setregs(p, pack, stack, arginfo)
    }

    fn signotify(p: &Proc) {
        arm64::machdep::signotify(p)
    }

    fn need_proftick(p: &Proc) {
        arm64::machdep::aston(p)
    }

    fn child_return(arg: *mut c_void) {
        arm64::syscall::child_return(arg)
    }

    fn proc_pc(p: &Proc) -> usize {
        // SAFETY: `pcb_tf` is the thread's trap frame at the top of its u-area (`cpu_fork`),
        // set before the thread first runs in user mode; read without a reference kept.
        unsafe { (*p.pcb().pcb_tf.get()).tf_elr as usize }
    }

    fn proc_stack(p: &Proc) -> usize {
        // SAFETY: as in `proc_pc`.
        unsafe { (*p.pcb().pcb_tf.get()).tf_sp as usize }
    }

    fn cpu_initclocks() {
        arm64::intr::cpu_initclocks()
    }

    fn cpu_startclock() {
        arm64::intr::cpu_startclock()
    }

    fn setstatclockrate(newhz: i32) {
        arm64::intr::setstatclockrate(newhz)
    }

    fn cpu_configure() {
        arm64::autoconf::cpu_configure()
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

    fn ci_smt_id(ci: &include::cpu::CpuInfo) -> u32 {
        ci.ci_smt_id.get()
    }

    fn intr_disable() -> u64 {
        include::cpu::intr_disable()
    }

    unsafe fn intr_restore(s: u64) {
        // SAFETY: forwarded: `s` came from `intr_disable` on this CPU.
        unsafe { include::cpu::intr_restore(s) }
    }

    unsafe fn intr_enable() {
        // SAFETY: forwarded: the caller's guarantee.
        unsafe { include::cpu::intr_enable() }
    }

    fn kbd_reset() -> Option<&'static core::sync::atomic::AtomicI32> {
        None
    }

    fn cpu_boot_secondary_processors() {
        arm64::cpu::cpu_boot_secondary_processors()
    }

    unsafe fn cpu_hatch(arg: usize) -> ! {
        // SAFETY: forwarded from the boot glue, with the `cpu_info` `cpu_start_secondary`
        // passed.
        unsafe { arm64::cpu::cpu_hatch_entry(arg) }
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
    /// The bootloader's direct map, until the kernel owns its page tables
    /// (`arm64/pmap.rs`, deviations).
    const HAVE_PMAP_DIRECT: bool = true;
    const PMAP_STEAL_MEMORY: bool = true;
    const PMAP_WC: usize = include::pmap::PMAP_WC as usize;
    const PMAP_NOCACHE: usize = include::pmap::PMAP_NOCACHE as usize;
    const PMAP_NOMMU: bool = false;
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

    fn pmap_page_protect(pg: &VmPage, prot: VmProt) {
        arm64::pmap::pmap_page_protect(pg, prot)
    }

    fn pmap_clear_modify(pg: &VmPage) -> bool {
        arm64::pmap::pmap_clear_modify(pg)
    }

    fn pmap_clear_reference(pg: &VmPage) -> bool {
        arm64::pmap::pmap_clear_reference(pg)
    }

    fn pmap_is_modified(pg: &VmPage) -> bool {
        arm64::pmap::pmap_is_modified(pg)
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

    fn pmap_create() -> &'static Self::Pmap {
        arm64::pmap::pmap_create()
    }

    fn pmap_destroy(pmap: &'static Self::Pmap) {
        arm64::pmap::pmap_destroy(pmap)
    }

    fn pmap_reference(pmap: &Self::Pmap) {
        arm64::pmap::pmap_reference(pmap)
    }

    fn pmap_enter(
        pmap: &Self::Pmap,
        va: Vaddr,
        pa: Paddr,
        prot: VmProt,
        flags: i32,
    ) -> Result<(), Errno> {
        arm64::pmap::pmap_enter(pmap, va, pa, prot, flags)
    }

    fn pmap_remove(pmap: &Self::Pmap, sva: Vaddr, eva: Vaddr) {
        arm64::pmap::pmap_remove(pmap, sva, eva)
    }

    fn pmap_protect(pmap: &Self::Pmap, sva: Vaddr, eva: Vaddr, prot: VmProt) {
        arm64::pmap::pmap_protect(pmap, sva, eva, prot)
    }

    fn pmap_wired_count(pmap: &Self::Pmap) -> i64 {
        pmap.pm_stats.wired_count.get()
    }

    fn pmap_resident_count(pmap: &Self::Pmap) -> i64 {
        pmap.pm_stats.resident_count.get()
    }

    fn pmap_unwire(pmap: &Self::Pmap, va: Vaddr) {
        arm64::pmap::pmap_unwire(pmap, va)
    }

    fn pmap_remove_holes(vm: &Vmspace) {
        arm64::pmap::pmap_remove_holes(vm)
    }

    fn pmap_proc_iflush(pr: &Process, va: Vaddr, len: Vsize) {
        arm64::pmap::pmap_proc_iflush(pr, va, len)
    }

    /// `pmap_update`: nothing, as the C.
    fn pmap_activate(p: &Proc) {
        arm64::pmap::pmap_activate(p)
    }

    fn pmap_deactivate(p: &Proc) {
        arm64::pmap::pmap_deactivate(p)
    }

    fn pmap_purge(p: &Proc) {
        arm64::pmap::pmap_purge(p)
    }

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

impl MachineExec for Machine {
    const LDPGSZ: usize = include::exec::LDPGSZ;
    const ARCH_ELFSIZE: usize = include::exec::ARCH_ELFSIZE;
    const ELF_TARG_CLASS: u8 = include::exec::ELF_TARG_CLASS;
    const ELF_TARG_DATA: u8 = include::exec::ELF_TARG_DATA;
    const ELF_TARG_MACH: u16 = include::exec::ELF_TARG_MACH;
    const HAVE_CPU_HWCAP: bool = true;
    const HAVE_CPU_HWCAP2: bool = true;
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

/// `<machine/atomic.h>`'s barriers (`arch/arm64/include/atomic.h`: "virtio needs MP membars
/// even on SP kernels").
impl crate::machine::atomic::Atomic for Machine {
    #[inline]
    fn virtio_membar_producer() {
        // SAFETY: a data memory barrier orders memory accesses and changes nothing else; no
        // `nomem`, so the compiler does not move accesses across it either.
        unsafe { asm!("dmb st", options(nostack, preserves_flags)) };
    }

    #[inline]
    fn virtio_membar_consumer() {
        // SAFETY: as for `virtio_membar_producer`.
        unsafe { asm!("dmb ld", options(nostack, preserves_flags)) };
    }

    #[inline]
    fn virtio_membar_sync() {
        // SAFETY: as for `virtio_membar_producer`.
        unsafe { asm!("dmb sy", options(nostack, preserves_flags)) };
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

    fn bus_space_subregion(
        t: Self::Tag,
        h: Self::Handle,
        offset: BusSize,
        size: BusSize,
    ) -> Result<Self::Handle, Errno> {
        (t._space_subregion)(t, h, offset, size)
    }

    fn bus_space_vaddr(t: Self::Tag, h: Self::Handle) -> *mut u8 {
        (t._space_vaddr)(t, h)
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

    fn bus_space_copy_2(
        t: Self::Tag,
        h1: Self::Handle,
        o1: BusSize,
        h2: Self::Handle,
        o2: BusSize,
        count: usize,
    ) {
        include::bus::bus_space_copy_2(t, h1, o1, h2, o2, count)
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

    const PCI_MSI_PER_BRIDGE: bool = false;
    // `ppb.c`'s defaults: arm64's `pci_machdep.h` sets none of the four.
    const PCI_IO_START: u64 = 0;
    const PCI_IO_END: u64 = 0xffff_ffff;
    const PCI_MEM_START: u64 = 0;
    const PCI_MEM_END: u64 = 0xffff_ffff;

    fn pci_attach_hook(parent: &Device, self_: &Device, pba: &PcibusAttachArgs) {
        (pba.pba_pc.pc_attach_hook)(parent, self_, pba)
    }

    fn pci_bus_maxdevs(pc: Self::PciChipsetTag, busno: i32) -> i32 {
        (pc.pc_bus_maxdevs)(pc.pc_conf_v, busno)
    }

    fn pci_lookup_segment(segment: i32, bus: i32) -> Option<Self::PciChipsetTag> {
        Some(dev::acpipci::pci_lookup_segment(segment, bus))
    }

    fn pci_mcfg_init(iot: BusSpaceTag, addr: BusAddr, segment: i32, min_bus: i32, max_bus: i32) {
        dev::acpipci::pci_mcfg_init(iot, addr, segment, min_bus, max_bus)
    }

    fn pci_make_tag(pc: Self::PciChipsetTag, bus: i32, device: i32, function: i32) -> Self::Pcitag {
        (pc.pc_make_tag)(pc.pc_conf_v, bus, device, function)
    }

    fn pci_decompose_tag(pc: Self::PciChipsetTag, tag: Self::Pcitag) -> (i32, i32, i32) {
        (pc.pc_decompose_tag)(pc.pc_conf_v, tag)
    }

    fn pcitag_node(tag: Self::Pcitag) -> i32 {
        include::pci_machdep::pcitag_node(tag) as i32
    }

    fn pci_conf_size(pc: Self::PciChipsetTag, tag: Self::Pcitag) -> i32 {
        (pc.pc_conf_size)(pc.pc_conf_v, tag)
    }

    fn pci_conf_read(pc: Self::PciChipsetTag, tag: Self::Pcitag, reg: i32) -> Pcireg {
        (pc.pc_conf_read)(pc.pc_conf_v, tag, reg)
    }

    fn pci_conf_write(pc: Self::PciChipsetTag, tag: Self::Pcitag, reg: i32, data: Pcireg) {
        (pc.pc_conf_write)(pc.pc_conf_v, tag, reg, data)
    }

    fn pci_probe_device_hook(pc: Self::PciChipsetTag, pa: &mut PciAttachArgs) -> i32 {
        (pc.pc_probe_device_hook)(pc.pc_conf_v, pa)
    }

    fn pci_dev_postattach(_dev: &Device, _pa: &PciAttachArgs) {}

    fn pci_min_powerstate(_pc: Self::PciChipsetTag, _tag: Self::Pcitag) -> Pcireg {
        crate::dev::pci::pcireg::PCI_PMCSR_STATE_D3
    }

    fn pci_set_powerstate_md(_pc: Self::PciChipsetTag, _tag: Self::Pcitag, _s: i32, _p: i32) {}

    fn pci_msix_table_map(
        pc: Self::PciChipsetTag,
        tag: Self::Pcitag,
        memt: BusSpaceTag,
    ) -> Result<BusSpaceHandle, Errno> {
        dev::pci_machdep::pci_msix_table_map(pc, tag, memt)
    }

    fn pci_msix_table_unmap(
        pc: Self::PciChipsetTag,
        tag: Self::Pcitag,
        memt: BusSpaceTag,
        memh: BusSpaceHandle,
    ) {
        dev::pci_machdep::pci_msix_table_unmap(pc, tag, memt, memh)
    }

    fn pci_intr_enable_msivec(pa: &PciAttachArgs, num_vec: i32) -> bool {
        dev::pci_machdep::pci_intr_enable_msivec(pa, num_vec)
    }

    fn pci_intr_map_msi(pa: &PciAttachArgs) -> Option<Self::PciIntrHandle> {
        (pa.pa_pc.pc_intr_map_msi)(pa)
    }

    fn pci_intr_map_msivec(pa: &PciAttachArgs, vec: i32) -> Option<Self::PciIntrHandle> {
        (pa.pa_pc.pc_intr_map_msivec)(pa, vec)
    }

    fn pci_intr_map_msix(pa: &PciAttachArgs, vec: i32) -> Option<Self::PciIntrHandle> {
        (pa.pa_pc.pc_intr_map_msix)(pa, vec)
    }

    fn pci_intr_map(pa: &PciAttachArgs) -> Option<Self::PciIntrHandle> {
        (pa.pa_pc.pc_intr_map)(pa)
    }

    fn pci_intr_string(pc: Self::PciChipsetTag, ih: Self::PciIntrHandle) -> PciIntrStr {
        (pc.pc_intr_string)(pc.pc_intr_v, ih)
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
        (pc.pc_intr_establish)(pc.pc_intr_v, ih, level, ci, func, arg, what)
    }

    unsafe fn pci_intr_disestablish(pc: Self::PciChipsetTag, cookie: NonNull<c_void>) {
        // SAFETY: forwarded to the bridge that established it.
        unsafe { (pc.pc_intr_disestablish)(pc.pc_intr_v, cookie) }
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

    fn db_regs() -> &'static [crate::ddb::db_variables::DbVariable] {
        &arm64::db_interface::DB_REGS
    }

    const DB_MACHINE_COMMAND_TABLE: &'static [crate::ddb::db_command::DbCommand] =
        arm64::db_interface::DB_MACHINE_COMMAND_TABLE;

    fn db_machine_command_table() -> &'static [crate::ddb::db_command::DbCommand] {
        Self::DB_MACHINE_COMMAND_TABLE
    }

    fn db_machine_init() {
        arm64::db_interface::db_machine_init()
    }

    fn set_pc_regs(pc: usize) {
        // SAFETY: ddb_regs is only used by the CPU in the debugger, and no other reference
        // to it is live during this access.
        include::db_machdep::set_pc_regs(unsafe { arm64::db_interface::DDB_REGS.get_mut() }, pc)
    }

    fn fixup_pc_after_break() {
        // The arm64 header defines no FIXUP_PC_AFTER_BREAK: `brk` traps with the PC on it.
    }

    fn db_set_single_step() {
        // SAFETY: as in set_pc_regs.
        include::db_machdep::db_set_single_step(unsafe { arm64::db_interface::DDB_REGS.get_mut() })
    }

    fn db_clear_single_step() {
        // SAFETY: as in set_pc_regs.
        include::db_machdep::db_clear_single_step(unsafe {
            arm64::db_interface::DDB_REGS.get_mut()
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

    fn intr_barrier(cookie: NonNull<c_void>) {
        arm64::intr::intr_barrier(cookie.cast())
    }
}

/// arm64 has no `pciide_machdep.c` (its GENERIC has no pciide) and no ISA bus: a
/// compatibility-mode PCI IDE channel gets no interrupt.
impl crate::machine::pciide_machdep::PciideMachdep for Machine {
    fn pciide_machdep_compat_intr_establish(
        _dev: &'static crate::sys::device::Device,
        _pa: &crate::dev::pci::pcivar::PciAttachArgs,
        _chan: i32,
        _func: fn(*mut c_void) -> i32,
        _arg: *mut c_void,
    ) -> Option<NonNull<c_void>> {
        None
    }

    unsafe fn pciide_machdep_compat_intr_disestablish(
        _pc: crate::machine::pci_machdep::PciChipsetTag,
        _cookie: NonNull<c_void>,
    ) {
    }
}

/// arm64 has no ISA bus: no interrupt line is free and none can be established.
impl crate::machine::isa_machdep::IsaMachdep for Machine {
    type IsaChipsetTag = *const c_void;

    const IST_NONE: i32 = 0;
    const IST_PULSE: i32 = 1;
    const IST_EDGE: i32 = 2;
    const IST_LEVEL: i32 = 3;

    fn isa_attach_hook(
        _parent: Option<&crate::sys::device::Device>,
        _self: &crate::sys::device::Device,
    ) {
    }

    fn isa_intr_check(_ic: Self::IsaChipsetTag, _irq: i32, _type: i32) -> i32 {
        0
    }

    fn isa_intr_establish(
        _ic: Self::IsaChipsetTag,
        _irq: i32,
        _type: i32,
        _level: i32,
        _ih_fun: fn(*mut c_void) -> i32,
        _ih_arg: *mut c_void,
        _ih_what: &'static str,
    ) -> Option<core::ptr::NonNull<c_void>> {
        None
    }
}

// arm64's acpi_machdep.c (arch/arm64/arm64/acpi_machdep.rs, M14): acpi0 at the device tree
// node efiboot makes from the UEFI configuration table.
impl crate::machine::acpi_machdep::AcpiMachdep for Machine {
    const ACPI_PRT: bool = false;
    const ACPI_SECTWO: bool = true;

    fn acpi_map(pa: Paddr, len: usize) -> Result<crate::dev::acpi::acpivar::AcpiMemMap, Errno> {
        arm64::acpi_machdep::acpi_map(pa, len)
    }

    fn acpi_unmap(handle: &crate::dev::acpi::acpivar::AcpiMemMap) {
        arm64::acpi_machdep::acpi_unmap(handle)
    }

    unsafe fn acpi_bus_space_map(
        t: BusSpaceTag,
        addr: BusAddr,
        size: BusSize,
        flags: i32,
    ) -> Result<BusSpaceHandle, Errno> {
        // SAFETY: the caller's guarantee, forwarded.
        unsafe { arm64::acpi_machdep::acpi_bus_space_map(t, addr, size, flags) }
    }

    fn acpi_bus_space_unmap(t: BusSpaceTag, bsh: BusSpaceHandle, size: BusSize) {
        arm64::acpi_machdep::acpi_bus_space_unmap(t, bsh, size)
    }

    fn acpi_intr_establish(
        irq: i32,
        flags: i32,
        level: i32,
        handler: fn(*mut c_void) -> i32,
        arg: *mut c_void,
        what: &'static str,
    ) -> Option<core::ptr::NonNull<c_void>> {
        arm64::acpi_machdep::acpi_intr_establish(irq, flags, level, handler, arg, what)
    }

    unsafe fn acpi_intr_disestablish(cookie: core::ptr::NonNull<c_void>) {
        // SAFETY: the caller's guarantee, forwarded.
        unsafe { arm64::acpi_machdep::acpi_intr_disestablish(cookie) }
    }

    fn acpi_attach_machdep(sc: &'static crate::dev::acpi::acpivar::AcpiSoftc) {
        arm64::acpi_machdep::acpi_attach_machdep(sc)
    }

    unsafe fn acpi_acquire_glk(lock: *mut u32) -> i32 {
        arm64::acpi_machdep::acpi_acquire_glk(lock)
    }

    unsafe fn acpi_release_glk(lock: *mut u32) -> i32 {
        arm64::acpi_machdep::acpi_release_glk(lock)
    }

    fn acpi_iommu_device_map(
        node: &crate::dev::acpi::amltypes::AmlNodeRef,
        dmat: Option<crate::machine::bus::BusDmaTag>,
    ) -> Option<crate::machine::bus::BusDmaTag> {
        arm64::acpi_machdep::acpi_iommu_device_map(node, dmat)
    }

    fn pwr_action() -> i32 {
        arm64::acpi_machdep::PWR_ACTION
    }

    fn ci_acpi_proc_id(ci: &CpuInfo) -> u32 {
        ci.ci_acpi_proc_id.get()
    }

    fn cpu_suspended() -> &'static core::sync::atomic::AtomicI32 {
        // arm64 cpu.c's cpu_suspended, here until its suspend code (cpu_suspend_primary) is
        // ported; nothing sets it yet.
        static CPU_SUSPENDED: core::sync::atomic::AtomicI32 = core::sync::atomic::AtomicI32::new(0);
        &CPU_SUSPENDED
    }

    fn delay_init(_f: fn(i32), _fn_quality: i32) {
        // arm64 has no delay_init: its delay(9) is agtimer's, and acpitimer/acpihpet, the
        // callers, are amd64/i386 drivers that arm64 GENERIC does not configure.
        let _ = crate::unported!("delay_init (not on arm64: acpitimer/acpihpet are x86 drivers)");
    }

    fn delay_fini(_f: fn(i32)) {
        let _ = crate::unported!("delay_fini (not on arm64: acpitimer/acpihpet are x86 drivers)");
    }

    fn cpu_recalibrate_tsc(_tc: &'static crate::sys::timetc::Timecounter) {
        // #if defined(__amd64__) in the drivers: nothing to recalibrate on arm64.
    }
}

/// No I/O APIC and no MP configuration tables: the x86 ACPI drivers (`acpimadt`,
/// `acpiprt`) are not configured here, so nothing calls this; it answers "none".
impl crate::machine::mpconfig::MpConfig for Machine {
    type Ioapic = crate::machine::mpconfig::NoIoapic;

    const MPS_INTPO_DEF: i32 = 0;
    const MPS_INTPO_ACTHI: i32 = 0;
    const MPS_INTPO_ACTLO: i32 = 0;
    const MPS_INTPO_SHIFT: i32 = 0;
    const MPS_INTPO_MASK: i32 = 0;
    const MPS_INTTR_DEF: i32 = 0;
    const MPS_INTTR_EDGE: i32 = 0;
    const MPS_INTTR_LEVEL: i32 = 0;
    const MPS_INTTR_SHIFT: i32 = 0;
    const MPS_INTTR_MASK: i32 = 0;
    const IOAPIC_REDLO_DEL_MASK: u32 = 0;
    const IOAPIC_REDLO_DEL_SHIFT: u32 = 0;
    const IOAPIC_REDLO_DEL_LOPRI: u32 = 0;
    const IOAPIC_REDLO_DEL_NMI: u32 = 0;
    const IOAPIC_REDLO_ACTLO: u32 = 0;
    const IOAPIC_REDLO_LEVEL: u32 = 0;
    const APIC_INT_VIA_APIC: i32 = 0;
    const APIC_INT_APIC_SHIFT: i32 = 0;
    const APIC_INT_PIN_SHIFT: i32 = 0;
    const ICU_LEN: i32 = 0;
    const NIOAPIC: bool = false;

    fn lapic_boot_init(_lapic_base: crate::sys::types::Paddr) {}

    fn lapic_cpu_number() -> u32 {
        0
    }

    fn mp_attach_cpu(
        _parent: &crate::sys::device::Device,
        _apic_id: u32,
        _acpi_proc_id: u32,
        _bp: bool,
        _print: crate::sys::device::CfprintT,
    ) {
    }

    fn mp_attach_ioapic(
        _parent: &crate::sys::device::Device,
        _memt: crate::machine::bus::BusSpaceTag,
        _apic_id: i32,
        _address: crate::machine::bus::BusAddr,
        _vecbase: i32,
        _print: crate::sys::device::CfprintT,
    ) {
    }

    fn ioapic_find_bybase(_vec: i32) -> Option<&'static Self::Ioapic> {
        None
    }

    fn ioapic_apicid(apic: &Self::Ioapic) -> i32 {
        match *apic {}
    }

    fn ioapic_vecbase(apic: &Self::Ioapic) -> i32 {
        match *apic {}
    }

    fn ioapic_set_ip_map(
        apic: &Self::Ioapic,
        _pin: i32,
        _map: &'static crate::machine::mpconfig::MpIntrMap,
    ) {
        match *apic {}
    }

    fn nioapics() -> i32 {
        0
    }

    fn mp_set_busses(
        _busses: &'static [crate::machine::mpconfig::MpBus],
        _isa: &'static crate::machine::mpconfig::MpBus,
    ) {
    }

    fn mp_set_intrs(_intrs: &'static [crate::machine::mpconfig::MpIntrMap]) {}

    fn mp_busses() -> Option<&'static [crate::machine::mpconfig::MpBus]> {
        None
    }
}

impl crate::machine::fdt::Fdt for Machine {
    type FdtAttachArgs<'a> = include::fdt::FdtAttachArgs<'a>;

    fn fdt_find_cons(name: &[u8]) -> crate::dev::ofw::fdt::FdtNode {
        arm64::machdep::fdt_find_cons(name)
    }

    fn stdout_node() -> i32 {
        arm64::machdep::STDOUT_NODE.load(core::sync::atomic::Ordering::Relaxed)
    }

    fn fdt_cons_bs_tag() -> crate::machine::bus::BusSpaceTag {
        arm64::bus_space::FDT_CONS_BS_TAG
    }

    fn fdt_intr_establish(
        node: i32,
        level: i32,
        func: crate::machine::intr::IntrFn,
        arg: *mut c_void,
        name: &'static str,
    ) -> Option<NonNull<c_void>> {
        include::fdt::fdt_intr_establish(node, level, func, arg, name).map(NonNull::cast)
    }

    fn fdt_intr_establish_imap_cpu(
        node: i32,
        reg: &[u32],
        level: i32,
        ci: Option<&'static CpuInfo>,
        func: crate::machine::intr::IntrFn,
        arg: *mut c_void,
        name: &'static str,
    ) -> Option<NonNull<c_void>> {
        include::fdt::fdt_intr_establish_imap_cpu(node, reg, level, ci, func, arg, name)
            .map(NonNull::cast)
    }

    fn fdt_intr_establish_msi_cpu(
        node: i32,
        addr: &mut u64,
        data: &mut u64,
        level: i32,
        ci: Option<&'static CpuInfo>,
        func: crate::machine::intr::IntrFn,
        arg: *mut c_void,
        name: &'static str,
    ) -> Option<NonNull<c_void>> {
        include::fdt::fdt_intr_establish_msi_cpu(node, addr, data, level, ci, func, arg, name)
            .map(NonNull::cast)
    }

    fn smc_call(a0: u64, a1: u64, a2: u64, a3: u64) -> u64 {
        arm64::cpufunc::smc_call(a0, a1, a2, a3)
    }

    fn hvc_call(a0: u64, a1: u64, a2: u64, a3: u64) -> u64 {
        arm64::cpufunc::hvc_call(a0, a1, a2, a3)
    }

    fn set_cpuresetfn(f: fn()) {
        // SAFETY: called by a driver's attach on the boot CPU during autoconfiguration, before
        // any other CPU or `boot(9)` reads it.
        unsafe { arm64::machdep::CPURESETFN.write(Some(f)) };
    }

    fn set_powerdownfn(f: fn()) {
        // SAFETY: as for `set_cpuresetfn`.
        unsafe { arm64::machdep::POWERDOWNFN.write(Some(f)) };
    }

    fn lid_action() -> i32 {
        arm64::machdep::LID_ACTION.load(core::sync::atomic::Ordering::Relaxed)
    }

    unsafe fn fdt_intr_disestablish(cookie: NonNull<c_void>) {
        // SAFETY: the caller's guarantee: the cookie is a `MachineIntrHandle` from
        // `fdt_intr_establish*`.
        unsafe { include::fdt::fdt_intr_disestablish(cookie.cast()) }
    }
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
        &dev::mainbus::MAINBUS_CD
    }

    fn device_register(dev: &crate::sys::device::Device, aux: *mut c_void) {
        arm64::autoconf::device_register(dev, aux)
    }

    fn pdevinit() -> &'static [crate::sys::device::Pdevinit] {
        // SAFETY: as for `cfdata`.
        unsafe { conf::ioconf::PDEVINIT.get() }
    }

    fn nam2blk() -> &'static [crate::sys::device::Nam2blk] {
        &arm64::autoconf::NAM2BLK
    }

    fn diskconf() {
        arm64::autoconf::diskconf()
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
        arm64::disksubr::readdisklabel(dev, strat, lp, spoofonly)
    }

    fn writedisklabel(
        dev: crate::sys::types::Dev,
        strat: crate::sys::conf::DevTypeStrategy,
        lp: &mut crate::sys::disklabel::Disklabel,
    ) -> Result<(), crate::sys::errno::Errno> {
        arm64::disksubr::writedisklabel(dev, strat, lp)
    }
}

/// The device switch tables (`arm64/arm64/conf.c`).
impl crate::machine::conf::Conf for Machine {
    fn nchrdev() -> u32 {
        arm64::conf::CDEVSW.len() as u32
    }

    fn cdevsw(maj: u32) -> Option<crate::sys::conf::Cdevsw> {
        arm64::conf::CDEVSW.get(maj)
    }

    fn cdevsw_set(maj: u32, sw: crate::sys::conf::Cdevsw) {
        arm64::conf::CDEVSW.set(maj, sw)
    }

    fn nblkdev() -> u32 {
        arm64::conf::BDEVSW.len() as u32
    }

    fn bdevsw(maj: u32) -> Option<crate::sys::conf::Bdevsw> {
        arm64::conf::BDEVSW.get(maj)
    }

    fn chrtoblktbl() -> &'static [crate::sys::types::Dev] {
        &arm64::conf::CHRTOBLKTBL
    }

    fn swapdev() -> crate::sys::types::Dev {
        arm64::conf::SWAPDEV
    }

    fn mem_no() -> u32 {
        arm64::conf::MEM_NO
    }

    fn iskmemdev(dev: crate::sys::types::Dev) -> bool {
        arm64::conf::iskmemdev(dev)
    }

    fn iszerodev(dev: crate::sys::types::Dev) -> bool {
        arm64::conf::iszerodev(dev)
    }

    fn getnulldev() -> crate::sys::types::Dev {
        arm64::conf::getnulldev()
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
        include::tcb::tcb_get(p)
    }

    fn tcb_set(p: &Proc, addr: usize) {
        include::tcb::tcb_set(p, addr)
    }

    fn tcb_invalid(_addr: usize) -> bool {
        // arm64's <machine/tcb.h> has no TCB_INVALID.
        false
    }
}

impl UserCopy for Machine {
    fn copyin(uaddr: usize, kbuf: &mut [u8]) -> Result<(), Errno> {
        arm64::copy::copyin(uaddr, kbuf)
    }

    fn copyout(kbuf: &[u8], uaddr: usize) -> Result<(), Errno> {
        arm64::copy::copyout(kbuf, uaddr)
    }

    fn copyinstr(uaddr: usize, kbuf: &mut [u8]) -> Result<usize, Errno> {
        arm64::copystr::copyinstr(uaddr, kbuf)
    }

    fn copyin32(uaddr: usize) -> Result<u32, Errno> {
        arm64::copy::copyin32(uaddr)
    }

    fn copyoutstr(kbuf: &[u8], uaddr: usize) -> Result<usize, Errno> {
        arm64::copystr::copyoutstr(kbuf, uaddr)
    }

    unsafe fn kcopy(src: *const u8, dst: *mut u8, len: usize) -> Result<(), Errno> {
        // SAFETY: forwarded.
        unsafe { arm64::copy::kcopy(src, dst, len) }
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
        arm64::sig_machdep::sendsig(catcher, sig, mask, ksip, info, onstack)
    }

    fn sys_sigreturn(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
        arm64::sig_machdep::sys_sigreturn(p, v, retval)
    }

    fn sigcode() -> &'static [u8] {
        arm64::locore::sigcode_bytes()
    }

    fn sigcoderet() -> usize {
        arm64::locore::sigcoderet_offset()
    }

    fn sigcodecall() -> usize {
        arm64::locore::sigcodecall_offset()
    }

    fn sigfill() -> &'static [u8] {
        arm64::locore::sigfill_bytes()
    }
}
/* </CODE> */
