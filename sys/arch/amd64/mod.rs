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

    fn setregs(p: &Proc, pack: &ExecPackage<'_>, stack: Vaddr, arginfo: &PsStrings) {
        amd64::machdep::setregs(p, pack, stack, arginfo)
    }

    fn signotify(p: &Proc) {
        amd64::machdep::signotify(p)
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
}

impl Console for Machine {
    fn consinit() {
        amd64::consinit::consinit()
    }

    fn cn_rx_intr_establish(sink: fn(u8)) -> Result<(), Errno> {
        amd64::consinit::cn_rx_intr_establish(sink)
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

    fn pci_attach_hook(parent: &Device, self_: &Device, pba: &PcibusAttachArgs) {
        pci::pci_machdep::pci_attach_hook(parent, self_, pba)
    }

    fn pci_bus_maxdevs(pc: Self::PciChipsetTag, busno: i32) -> i32 {
        pci::pci_machdep::pci_bus_maxdevs(pc, busno)
    }

    fn pci_make_tag(pc: Self::PciChipsetTag, bus: i32, device: i32, function: i32) -> Self::Pcitag {
        pci::pci_machdep::pci_make_tag(pc, bus, device, function)
    }

    fn pci_decompose_tag(pc: Self::PciChipsetTag, tag: Self::Pcitag) -> (i32, i32, i32) {
        pci::pci_machdep::pci_decompose_tag(pc, tag)
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
}

/// amd64 has no device tree: ACPI describes the machine (M5).
impl crate::machine::fdt::Fdt for Machine {
    fn fdt_find_cons(_name: &[u8]) -> crate::dev::ofw::fdt::FdtNode {
        core::ptr::null()
    }

    fn stdout_node() -> i32 {
        0
    }

    fn fdt_cons_bs_tag() -> crate::machine::bus::BusSpaceTag {
        amd64::bus_space::X86_BUS_SPACE_IO
    }
}

/// The autoconfiguration tables `config(8)` would generate (`conf/ioconf.rs`) and the
/// `autoconf.c` hook.
impl crate::machine::autoconf::Autoconf for Machine {
    fn cfdata() -> &'static [crate::sys::device::Cfdata] {
        &conf::ioconf::CFDATA
    }

    fn cfroots() -> &'static [i16] {
        &conf::ioconf::CFROOTS
    }

    fn mainbus_cd() -> &'static crate::sys::device::Cfdriver {
        &amd64::mainbus::MAINBUS_CD
    }

    fn device_register(dev: &crate::sys::device::Device, aux: *mut c_void) {
        amd64::autoconf::device_register(dev, aux)
    }

    fn pdevinit() -> &'static [crate::sys::device::Pdevinit] {
        &conf::ioconf::PDEVINIT
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

    fn sigfill() -> &'static [u8] {
        amd64::locore::sigfill_bytes()
    }
}
