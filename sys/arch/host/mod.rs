//! Host test double for the machine interface. Not an OpenBSD architecture.
//!
//! Compiled whenever `target_os != "none"` so that `cargo test` runs on macOS/Linux and the
//! compiler proves the `machine` contract is complete. It prints to stdout, has no-op SPL and
//! fakes hardware with std collections. It must not grow logic: behaviour belongs in `kern/`.
//!
//! Its machine parameters mirror amd64's `<machine/param.h>` and `<machine/_types.h>`, so tests
//! see the page geometry of a real architecture (`just test-ref` checks that they stay equal).
//! Its console is a `Consdev` that writes to stdout and reads nothing, so `kprintf!` in a test
//! shows up with `--nocapture`; its bus space accepts every map, reads 0 and drops writes.
//! Its pmap is a `BTreeMap` of kernel mappings; physical pages are numbers, not memory, so
//! zeroing and copying them do nothing, and boot memory comes from the host allocator.
//! No host device does DMA: its `bus_dma` types carry only the public members and every
//! operation fails with `EOPNOTSUPP` (the archs' `bus_dma` is exercised by the QEMU boot
//! self-test). It has no PCI bus either: configuration reads return all ones unless a test
//! installs a configuration space with `Machine::set_pci_conf`, and no interrupt maps.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::NonNull;
use std::boxed::Box;
use std::collections::BTreeMap;
use std::eprintln;
use std::io::Write;
use std::sync::Mutex;
use std::vec;

use crate::dev::cons::{CN_LOWPRI, Consdev, set_cn_tab};
use crate::dev::pci::pcivar::{PciAttachArgs, PcibusAttachArgs, Pcireg};
use crate::machine::autoconf::Autoconf;
use crate::machine::bus::{BusAddr, BusDma, BusSize, BusSpace};
use crate::machine::copy::UserCopy;
use crate::machine::db_machdep::{DbMachdep, PrFn};
use crate::machine::pci_machdep::{PciIntrFn, PciIntrStr, PciMachdep};
use crate::machine::proc::MachineProc;
use crate::machine::signal::MachineSignal;
use crate::machine::tcb::Tcb;
use crate::machine::{
    BootInfo, Console, Cpu, Exit, ExitStatus, Intr, MachineInfo, MachineParam, Pmap, VmParam,
};
use crate::sys::clockintr::Clockqueue;
use crate::sys::device::{Cfdata, Cfdriver, DV_DULL, Device};
use crate::sys::errno::Errno;
use crate::sys::exec::{ExecPackage, PsStrings};
use crate::sys::exec_elf::{ELFCLASS64, ELFDATA2LSB};
use crate::sys::mbuf::Mbuf;
use crate::sys::param::NODEV;
use crate::sys::proc::{Proc, Process};
use crate::sys::sched::SchedstatePercpu;
use crate::sys::siginfo::Siginfo;
use crate::sys::signal::{Sig, Sigset};
use crate::sys::systm::SysArgs;
use crate::sys::types::{Dev, Off, Paddr, Register, Vaddr, Vsize};
use crate::sys::uio::Uio;
use crate::sys::user::User;
use crate::uvm::uvm_extern::{UvmConstraintRange, VmProt, Vmspace};
use crate::uvm::uvm_page::{
    PHYS_TO_VM_PAGE, VM_PSTRAT_BIGFIRST, VmPage, uvm_page_physsteal, vm_page_to_phys,
};

/// The host implementation of the machine interface.
pub struct Machine;

/// The host's one bus space.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HostBusSpace;

/// A host bus space handle: the address that was "mapped".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostBusSpaceHandle(usize);

/// The host's DMA tag. No host device does DMA: every `bus_dma` operation fails with
/// `EOPNOTSUPP` (or does nothing), and `bus_dma` is exercised in QEMU by the boot self-test.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HostBusDmaTag;

/// The host's `bus_dma_segment_t`: the public members.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HostBusDmaSegment {
    /// `ds_addr`.
    pub ds_addr: BusAddr,
    /// `ds_len`.
    pub ds_len: BusSize,
}

/// The host's `struct bus_dmamap`: the public members, so generic code compiles against
/// the same names on every machine. The host never makes one.
pub struct HostBusDmamap {
    /// `dm_mapsize`.
    pub dm_mapsize: Cell<BusSize>,
    /// `dm_nsegs`.
    pub dm_nsegs: Cell<i32>,
    /// `dm_segs`.
    pub segs: [Cell<HostBusDmaSegment>; 1],
}

impl HostBusDmamap {
    /// `dm_segs`.
    pub fn dm_segs(&self) -> &[Cell<HostBusDmaSegment>] {
        &self.segs
    }
}

/// The host's `pci_chipset_tag_t`: there is no chipset state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HostPciChipset;

/// The host's `pcitag_t`: the three numbers, as they are.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HostPcitag {
    /// The bus.
    pub bus: i32,
    /// The device.
    pub device: i32,
    /// The function.
    pub function: i32,
}

/// The host's `pci_intr_handle_t`: there are no PCI interrupts on the host.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HostPciIntrHandle;

/// A configuration space a host test installs, as its read and write functions: the host
/// has no PCI bus, so `pci_conf_read` returns all ones (no device) unless a test puts
/// devices there.
pub type HostPciConf = (
    Box<dyn FnMut(HostPcitag, i32) -> u32 + Send>,
    Box<dyn FnMut(HostPcitag, i32, u32) + Send>,
);

/// The configuration space a test installed.
static HOST_PCI_CONF: Mutex<Option<HostPciConf>> = Mutex::new(None);

/// The host's `struct pmap`: the kernel mappings, page by page.
pub struct HostPmap {
    mappings: Mutex<BTreeMap<usize, usize>>,
    /// `pm_obj.uo_refs`: how many vmspaces hold the pmap.
    refs: core::sync::atomic::AtomicI32,
}

/// Where the host pretends kernel virtual space starts (amd64's `VM_MIN_KERNEL_ADDRESS`).
const HOST_KVA_START: usize = 0xffff_8000_0000_0000;
/// Where it ends (amd64's `VM_MAX_KERNEL_ADDRESS`).
const HOST_KVA_END: usize = 0xffff_8080_0000_0000;

/// The host's kernel pmap.
static HOST_PMAP: HostPmap = HostPmap {
    mappings: Mutex::new(BTreeMap::new()),
    refs: core::sync::atomic::AtomicI32::new(1),
};
/// amd64's `isa_constraint`, so the tests see two DMA ranges.
static ISA_CONSTRAINT: UvmConstraintRange = UvmConstraintRange {
    ucr_low: Paddr::new(0),
    ucr_high: Paddr::new(0x00ff_ffff),
};
/// amd64's `dma_constraint`.
static DMA_CONSTRAINT: UvmConstraintRange = UvmConstraintRange {
    ucr_low: Paddr::new(0),
    ucr_high: Paddr::new(0xffff_ffff),
};

fn host_cngetc(_dev: Dev) -> i32 {
    0
}

fn host_cnputc(_dev: Dev, c: i32) {
    // A failed write to stdout has nowhere to be reported; the console is best effort.
    let _ = std::io::stdout().write_all(&[c as u8]);
}

fn host_cnpollc(_dev: Dev, _on: bool) {}

/// The host console device.
static HOSTCONS: Consdev = Consdev {
    cn_probe: None,
    cn_init: None,
    cn_getc: host_cngetc,
    cn_putc: host_cnputc,
    cn_pollc: host_cnpollc,
    cn_bell: None,
    cn_dev: Cell::new(NODEV),
    cn_pri: Cell::new(CN_LOWPRI),
};

impl MachineInfo for Machine {
    const MACHINE: &'static str = "host";
    const MACHINE_ARCH: &'static str = "host";
}

impl MachineParam for Machine {
    const PAGE_SHIFT: usize = 12;
    const PAGE_SIZE: usize = 1 << Self::PAGE_SHIFT;
    const PAGE_MASK: usize = Self::PAGE_SIZE - 1;
    const KERNBASE: usize = 0xffff_ffff_8000_0000;
    const UPAGES: usize = 6;
    const USPACE: usize = Self::UPAGES * Self::PAGE_SIZE;
    const USPACE_ALIGN: usize = 0;
    const HAVE_USPACE_GUARD: bool = true;
    const NMBCLUSTERS: usize = 256 * 1024;
    const MSGBUFSIZE: usize = 32 * Self::PAGE_SIZE;
    const HAVE_ACPI: bool = true;
    const HAVE_FDT: bool = false;
    const ALIGNBYTES: usize = core::mem::size_of::<usize>() - 1;
    const STACKALIGNBYTES: usize = 15;
    const MAX_PAGE_SHIFT: usize = 12;

    fn aligned_pointer<T>(_p: usize) -> bool {
        true
    }
}

/// The host's `struct cpu_info`: what the generic clock and scheduler code reaches.
pub struct HostCpuInfo {
    /// `ci_queue`.
    pub ci_queue: Clockqueue,
    /// `ci_schedstate`.
    pub ci_schedstate: SchedstatePercpu,
    /// `ci_randseed`.
    pub ci_randseed: Cell<u32>,
    /// `ci_want_resched`.
    pub ci_want_resched: Cell<i32>,
    /// `ci_curproc`.
    pub ci_curproc: Cell<*const Proc>,
}

// SAFETY: the one host CPU; the tests that touch the queue serialise on their own lock.
unsafe impl Sync for HostCpuInfo {}

/// The host's one CPU.
static HOST_CPU_INFO: HostCpuInfo = HostCpuInfo {
    ci_queue: Clockqueue::new(),
    ci_schedstate: SchedstatePercpu::new(),
    ci_randseed: Cell::new(1),
    ci_want_resched: Cell::new(0),
    ci_curproc: Cell::new(core::ptr::null()),
};

/// The host's `proc0paddr`.
static HOST_PROC0PADDR: User = User::new();

/// The host's `struct mdproc`: nothing.
#[derive(Default)]
pub struct HostMdproc;

/// The host's `struct pcb`: nothing to switch; it only stores the TCB address `TCB_SET`
/// records.
#[derive(Default)]
pub struct HostPcb {
    /// The thread's TCB address (`pcb_tcb` on arm64, `pcb_fsbase` on amd64).
    pub pcb_tcb: Cell<usize>,
}

// SAFETY: a thread's pcb is written by that thread (`TCB_SET`) or before it runs; the tests
// that build threads serialise on their own lock.
unsafe impl Sync for HostPcb {}

impl MachineProc for Machine {
    type Mdproc = HostMdproc;
    const MDPROC_INIT: HostMdproc = HostMdproc;
    type Pcb = HostPcb;
    const PCB_INIT: HostPcb = HostPcb {
        pcb_tcb: Cell::new(0),
    };
}

impl Tcb for Machine {
    fn tcb_get(p: &Proc) -> usize {
        p.pcb().pcb_tcb.get()
    }

    fn tcb_set(p: &Proc, addr: usize) {
        p.pcb().pcb_tcb.set(addr);
    }

    fn tcb_invalid(_addr: usize) -> bool {
        false
    }
}

/// The host's `struct clockframe`: nothing to read.
pub struct HostClockFrame;

impl Cpu for Machine {
    type CpuInfo = HostCpuInfo;
    type ClockFrame = HostClockFrame;
    const MAXCPUS: u32 = 1;

    unsafe fn early_init(_boot: &BootInfo) -> Result<(), &'static str> {
        Ok(())
    }

    /// The host has no CPU to park: the process ends instead.
    fn halt() -> ! {
        std::process::exit(0)
    }

    /// The host has nothing to reboot: the process ends with a failure status.
    fn boot(howto: i32) -> ! {
        let _ = std::io::stdout().flush();
        eprintln!("host: boot(howto={howto:#x})");
        std::process::exit(1)
    }

    fn delay(usec: u32) {
        std::thread::sleep(std::time::Duration::from_micros(u64::from(usec)));
    }

    fn cpu_startup() {}

    fn curcpu() -> &'static HostCpuInfo {
        &HOST_CPU_INFO
    }

    fn curcpu_ptr() -> *const () {
        // Aligned, so bit 0 (the mutex waiter flag) is clear in its address.
        static HOST_CPU: u64 = 0;
        core::ptr::from_ref(&HOST_CPU).cast()
    }

    fn curcpu_mutex_level_add(_delta: i32) {}

    fn curcpu_mutex_level() -> i32 {
        0
    }

    fn cpu_info_foreach(f: &mut dyn FnMut(&'static HostCpuInfo)) {
        f(&HOST_CPU_INFO);
    }

    fn cpu_is_primary(_ci: &HostCpuInfo) -> bool {
        true
    }

    fn cpu_info_unit(_ci: &HostCpuInfo) -> u32 {
        0
    }

    fn ci_queue(ci: &HostCpuInfo) -> &Clockqueue {
        &ci.ci_queue
    }

    fn ci_schedstate(ci: &HostCpuInfo) -> &SchedstatePercpu {
        &ci.ci_schedstate
    }

    fn ci_randseed(ci: &HostCpuInfo) -> &Cell<u32> {
        &ci.ci_randseed
    }

    fn ci_curproc(ci: &HostCpuInfo) -> *const Proc {
        ci.ci_curproc.get()
    }

    fn set_curproc(ci: &HostCpuInfo, p: *const Proc) {
        ci.ci_curproc.set(p);
    }

    fn proc0paddr() -> &'static User {
        &HOST_PROC0PADDR
    }

    fn ci_idepth(_ci: &HostCpuInfo) -> u32 {
        0
    }

    fn clkf_usermode(_frame: &HostClockFrame) -> bool {
        false
    }

    fn clkf_pc(_frame: &HostClockFrame) -> usize {
        0
    }

    fn clkf_intr(_frame: &HostClockFrame) -> bool {
        false
    }

    fn need_resched(ci: &HostCpuInfo) {
        ci.ci_want_resched.set(1);
    }

    fn clear_resched(ci: &HostCpuInfo) {
        ci.ci_want_resched.set(0);
    }

    fn cpu_unidle(_ci: &HostCpuInfo) {}

    fn cpu_idle_enter() {}

    fn cpu_idle_cycle() {}

    fn cpu_idle_leave() {}

    /// The host has one thread of execution and no kernel stacks to switch between.
    unsafe fn cpu_switchto(_old: Option<&Proc>, _new: &Proc) {
        crate::kern::subr_prf::panic(format_args!("host: cpu_switchto has no context switch"))
    }

    fn cpu_exit(_p: &Proc) {}

    /// Nothing to set up: the host never switches to the thread.
    fn cpu_fork(
        _p1: &Proc,
        _p2: &Proc,
        _stack: *mut u8,
        _tcb: *mut u8,
        _func: fn(*mut core::ffi::c_void),
        _arg: *mut core::ffi::c_void,
    ) {
    }

    /// No user mode to return to.
    fn setregs(_p: &Proc, _pack: &ExecPackage<'_>, _stack: Vaddr, _arginfo: &PsStrings) {}

    /// No user mode, hence no AST to post.
    fn signotify(_p: &Proc) {}

    /// The host double has no user mode: no program counter to report.
    fn proc_pc(_p: &Proc) -> usize {
        0
    }

    /// The host double has no user mode: no stack pointer to report.
    fn proc_stack(_p: &Proc) -> usize {
        0
    }

    fn cpu_initclocks() {}

    fn cpu_startclock() {}

    fn setstatclockrate(_newhz: i32) {}

    fn cpu_configure() {}
}

impl VmParam for Machine {
    const VM_MIN_ADDRESS: usize = Self::PAGE_SIZE;
    const VM_MAXUSER_ADDRESS: usize = 0x0000_7f7f_ffff_c000;
    const VM_MAX_ADDRESS: usize = 0x0000_7fbf_dfef_f000;
    const VM_MIN_KERNEL_ADDRESS: usize = HOST_KVA_START;
    const VM_MAX_KERNEL_ADDRESS: usize = HOST_KVA_END;
    const VM_PHYSSEG_MAX: usize = 16;
    const VM_PHYSSEG_STRAT: i32 = VM_PSTRAT_BIGFIRST;
    const VM_PHYSSEG_NOADD: bool = true;
    const USRSTACK: usize = Self::VM_MAXUSER_ADDRESS;
    const MAXTSIZ: usize = 256 * 1024 * 1024;
    const DFLDSIZ: usize = 128 * 1024 * 1024;
    const MAXDSIZ: usize = 128 * 1024 * 1024 * 1024;
    const BRKSIZ: usize = 8 * 1024 * 1024 * 1024;
    const DFLSSIZ: usize = 2 * 1024 * 1024;
    const MAXSSIZ: usize = 32 * 1024 * 1024;
    const STACKGAP_RANDOM: usize = 256 * 1024;
    const VM_MIN_STACK_ADDRESS: usize = 0x0000_6000_0000_0000;
}

impl Pmap for Machine {
    type VmPageMd = ();
    type Pmap = HostPmap;

    const VM_MDPAGE_INIT: () = ();
    const HAVE_PMAP_DIRECT: bool = true;
    const PMAP_WC: usize = 0;
    const PMAP_NOMMU: bool = true;
    const PMAP_STEAL_MEMORY: bool = true;
    const UVM_MD_CONSTRAINTS: &'static [&'static UvmConstraintRange] =
        &[&ISA_CONSTRAINT, &DMA_CONSTRAINT];
    const DMA_CONSTRAINT: &'static UvmConstraintRange = &DMA_CONSTRAINT;

    fn pmap_kernel() -> &'static HostPmap {
        &HOST_PMAP
    }

    /// A user pmap is another map; it is leaked on destroy (a test double).
    fn pmap_create() -> &'static HostPmap {
        Box::leak(Box::new(HostPmap {
            mappings: Mutex::new(BTreeMap::new()),
            refs: core::sync::atomic::AtomicI32::new(1),
        }))
    }

    fn pmap_destroy(pmap: &'static HostPmap) {
        if pmap
            .refs
            .fetch_sub(1, core::sync::atomic::Ordering::Relaxed)
            == 1
        {
            pmap.mappings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clear();
        }
    }

    fn pmap_reference(pmap: &HostPmap) {
        pmap.refs
            .fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    }

    fn pmap_enter(
        pmap: &HostPmap,
        va: Vaddr,
        pa: Paddr,
        _prot: VmProt,
        _flags: i32,
    ) -> Result<(), Errno> {
        let mut map = pmap.mappings.lock().unwrap_or_else(|e| e.into_inner());
        map.insert(va.trunc_page().as_usize(), pa.trunc_page().as_usize());
        Ok(())
    }

    fn pmap_remove(pmap: &HostPmap, sva: Vaddr, eva: Vaddr) {
        let mut map = pmap.mappings.lock().unwrap_or_else(|e| e.into_inner());
        map.retain(|&va, _| va < sva.trunc_page().as_usize() || va >= eva.as_usize());
    }

    /// Mappings have no protection on the host; `PROT_NONE` removes them, as the C does.
    fn pmap_protect(pmap: &HostPmap, sva: Vaddr, eva: Vaddr, prot: VmProt) {
        if prot == crate::sys::mman::PROT_NONE {
            Self::pmap_remove(pmap, sva, eva);
        }
    }

    /// Nothing is wired on the host.
    fn pmap_wired_count(_pmap: &HostPmap) -> i64 {
        0
    }

    /// Every mapping is resident on the host.
    fn pmap_resident_count(pmap: &HostPmap) -> i64 {
        pmap.mappings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .len() as i64
    }

    /// Nothing is wired on the host.
    fn pmap_unwire(_pmap: &HostPmap, _va: Vaddr) {}

    fn pmap_remove_holes(_vm: &Vmspace) {}

    fn pmap_proc_iflush(_pr: &Process, _va: Vaddr, _len: Vsize) {}

    /// Page contents are not modelled.
    fn pmap_zero_page(_pg: &VmPage) {}

    /// Page contents are not modelled.
    fn pmap_copy_page(_src: &VmPage, _dst: &VmPage) {}

    /// Mappings have no protection on the host.
    fn pmap_page_protect(_pg: &VmPage, _prot: VmProt) {}

    /// Nothing is ever modified on the host.
    fn pmap_clear_modify(_pg: &VmPage) -> bool {
        false
    }

    /// Boot memory comes from the host allocator and is never returned; the frames it stands
    /// for leave `vm_physmem[]` through `uvm_page_physsteal`, as on a real machine, so
    /// `uvm_page_init`'s page count adds up.
    unsafe fn pmap_steal_memory(
        size: Vsize,
        start: Option<&mut Vaddr>,
        end: Option<&mut Vaddr>,
    ) -> Vaddr {
        let size = size.round_page().as_usize();
        // A missing frame is the C's "out of memory" panic; the tests load enough.
        let _ = uvm_page_physsteal(size / Self::PAGE_SIZE);
        let words = size.div_ceil(size_of::<u64>());
        let block: &'static mut [u64] = vec![0u64; words].leak();
        if let Some(start) = start {
            *start = Vaddr::new(HOST_KVA_START);
        }
        if let Some(end) = end {
            *end = Vaddr::new(HOST_KVA_END);
        }
        Vaddr::new(block.as_mut_ptr() as usize)
    }

    fn pmap_virtual_space(start: &mut Vaddr, end: &mut Vaddr) {
        *start = Vaddr::new(HOST_KVA_START);
        *end = Vaddr::new(HOST_KVA_END);
    }

    unsafe fn pmap_kenter_pa(va: Vaddr, pa: Paddr, _prot: VmProt) {
        let mut map = HOST_PMAP.mappings.lock().unwrap_or_else(|e| e.into_inner());
        map.insert(va.trunc_page().as_usize(), pa.trunc_page().as_usize());
    }

    unsafe fn pmap_kremove(va: Vaddr, len: Vsize) {
        let mut map = HOST_PMAP.mappings.lock().unwrap_or_else(|e| e.into_inner());
        let mut page = va.trunc_page().as_usize();
        let end = va.as_usize() + len.as_usize();
        while page < end {
            map.remove(&page);
            page += Self::PAGE_SIZE;
        }
    }

    fn pmap_extract(pmap: &HostPmap, va: Vaddr) -> Option<Paddr> {
        let map = pmap.mappings.lock().unwrap_or_else(|e| e.into_inner());
        map.get(&va.trunc_page().as_usize())
            .map(|pa| Paddr::new(pa + (va.as_usize() & Self::PAGE_MASK)))
    }

    fn pmap_activate(_p: &Proc) {}

    fn pmap_deactivate(_p: &Proc) {}

    fn pmap_update(_pmap: &HostPmap) {}

    /// The host's page tables never run out.
    fn pmap_growkernel(maxkvaddr: Vaddr) -> Vaddr {
        maxkvaddr
    }

    fn pmap_init() {}

    /// The "direct map" is the identity: a frame's address is its number.
    fn pmap_map_direct(pg: &VmPage) -> Vaddr {
        Vaddr::new(vm_page_to_phys(pg).as_usize())
    }

    fn pmap_unmap_direct(va: Vaddr) -> Option<&'static VmPage> {
        PHYS_TO_VM_PAGE(Paddr::new(va.as_usize()))
    }
}

impl crate::machine::exec::MachineExec for Machine {
    const LDPGSZ: usize = 4096;
    const ARCH_ELFSIZE: usize = 64;
    const ELF_TARG_CLASS: u8 = ELFCLASS64;
    const ELF_TARG_DATA: u8 = ELFDATA2LSB;
    #[cfg(target_arch = "aarch64")]
    const ELF_TARG_MACH: u16 = crate::sys::exec_elf::EM_AARCH64;
    #[cfg(not(target_arch = "aarch64"))]
    const ELF_TARG_MACH: u16 = crate::sys::exec_elf::EM_AMD64;
}

impl Console for Machine {
    fn consinit() {
        set_cn_tab(&HOSTCONS);
    }

    fn cn_rx_intr_establish(_sink: fn(u8)) -> Result<(), Errno> {
        Err(Errno::ENODEV)
    }
}

impl Exit for Machine {
    fn exit(status: ExitStatus) -> ! {
        let _ = std::io::stdout().flush();
        std::process::exit(status.qemu_status() as i32)
    }
}

/// The barriers of `<machine/atomic.h>`: the host's fences.
impl crate::machine::atomic::Atomic for Machine {
    fn virtio_membar_producer() {
        core::sync::atomic::fence(core::sync::atomic::Ordering::Release);
    }

    fn virtio_membar_consumer() {
        core::sync::atomic::fence(core::sync::atomic::Ordering::Acquire);
    }

    fn virtio_membar_sync() {
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
    }
}

impl BusSpace for Machine {
    type Tag = HostBusSpace;
    type Handle = HostBusSpaceHandle;

    const BUS_SPACE_MAP_CACHEABLE: u32 = 0x01;
    const BUS_SPACE_MAP_LINEAR: u32 = 0x02;
    const BUS_SPACE_MAP_PREFETCHABLE: u32 = 0x08;

    unsafe fn bus_space_map(
        _t: Self::Tag,
        addr: BusAddr,
        _size: BusSize,
        _flags: u32,
    ) -> Result<Self::Handle, Errno> {
        Ok(HostBusSpaceHandle(addr))
    }

    fn bus_space_unmap(_t: Self::Tag, _h: Self::Handle, _size: BusSize) {}

    fn bus_space_subregion(
        _t: Self::Tag,
        h: Self::Handle,
        offset: BusSize,
        _size: BusSize,
    ) -> Result<Self::Handle, Errno> {
        Ok(HostBusSpaceHandle(h.0 + offset))
    }

    fn bus_space_read_1(_t: Self::Tag, _h: Self::Handle, _offset: BusSize) -> u8 {
        0
    }

    fn bus_space_read_2(_t: Self::Tag, _h: Self::Handle, _offset: BusSize) -> u16 {
        0
    }

    fn bus_space_read_4(_t: Self::Tag, _h: Self::Handle, _offset: BusSize) -> u32 {
        0
    }

    fn bus_space_write_1(_t: Self::Tag, _h: Self::Handle, _offset: BusSize, _value: u8) {}

    fn bus_space_write_2(_t: Self::Tag, _h: Self::Handle, _offset: BusSize, _value: u16) {}

    fn bus_space_write_4(_t: Self::Tag, _h: Self::Handle, _offset: BusSize, _value: u32) {}

    fn bus_space_barrier(
        _t: Self::Tag,
        _h: Self::Handle,
        _offset: BusSize,
        _length: BusSize,
        _flags: u32,
    ) {
    }
}

impl BusDma for Machine {
    type DmaTag = HostBusDmaTag;
    type Dmamap = HostBusDmamap;
    type DmaSegment = HostBusDmaSegment;

    // amd64's values, like the rest of the host's parameters.
    const BUS_DMA_WAITOK: i32 = 0x0000;
    const BUS_DMA_NOWAIT: i32 = 0x0001;
    const BUS_DMA_ALLOCNOW: i32 = 0x0002;
    const BUS_DMA_COHERENT: i32 = 0x0004;
    const BUS_DMA_BUS1: i32 = 0x0010;
    const BUS_DMA_BUS2: i32 = 0x0020;
    const BUS_DMA_STREAMING: i32 = 0x0100;
    const BUS_DMA_READ: i32 = 0x0200;
    const BUS_DMA_WRITE: i32 = 0x0400;
    const BUS_DMA_NOCACHE: i32 = 0x0800;
    const BUS_DMA_ZERO: i32 = 0x1000;
    const BUS_DMA_64BIT: i32 = 0x2000;
    const BUS_DMASYNC_PREREAD: i32 = 0x01;
    const BUS_DMASYNC_POSTREAD: i32 = 0x02;
    const BUS_DMASYNC_PREWRITE: i32 = 0x04;
    const BUS_DMASYNC_POSTWRITE: i32 = 0x08;

    fn bus_dmamap_create(
        _t: Self::DmaTag,
        _size: BusSize,
        _nsegments: i32,
        _maxsegsz: BusSize,
        _boundary: BusSize,
        _flags: i32,
    ) -> Result<&'static Self::Dmamap, Errno> {
        Err(Errno::EOPNOTSUPP)
    }

    unsafe fn bus_dmamap_destroy(_t: Self::DmaTag, _map: NonNull<Self::Dmamap>) {}

    unsafe fn bus_dmamap_load(
        _t: Self::DmaTag,
        _map: &Self::Dmamap,
        _buf: *mut u8,
        _buflen: BusSize,
        _p: Option<&Proc>,
        _flags: i32,
    ) -> Result<(), Errno> {
        Err(Errno::EOPNOTSUPP)
    }

    unsafe fn bus_dmamap_load_mbuf(
        _t: Self::DmaTag,
        _map: &Self::Dmamap,
        _m: &Mbuf,
        _flags: i32,
    ) -> Result<(), Errno> {
        Err(Errno::EOPNOTSUPP)
    }

    unsafe fn bus_dmamap_load_uio(
        _t: Self::DmaTag,
        _map: &Self::Dmamap,
        _uio: &Uio<'_>,
        _flags: i32,
    ) -> Result<(), Errno> {
        Err(Errno::EOPNOTSUPP)
    }

    unsafe fn bus_dmamap_load_raw(
        _t: Self::DmaTag,
        _map: &Self::Dmamap,
        _segs: &[Self::DmaSegment],
        _size: BusSize,
        _flags: i32,
    ) -> Result<(), Errno> {
        Err(Errno::EOPNOTSUPP)
    }

    fn bus_dmamap_unload(_t: Self::DmaTag, _map: &Self::Dmamap) {}

    fn bus_dmamap_sync(
        _t: Self::DmaTag,
        _map: &Self::Dmamap,
        _offset: BusAddr,
        _len: BusSize,
        _ops: i32,
    ) {
    }

    fn bus_dmamem_alloc(
        _t: Self::DmaTag,
        _size: BusSize,
        _alignment: BusSize,
        _boundary: BusSize,
        _segs: &mut [Self::DmaSegment],
        _flags: i32,
    ) -> Result<usize, Errno> {
        Err(Errno::EOPNOTSUPP)
    }

    fn bus_dmamem_alloc_range(
        _t: Self::DmaTag,
        _size: BusSize,
        _alignment: BusSize,
        _boundary: BusSize,
        _segs: &mut [Self::DmaSegment],
        _flags: i32,
        _low: BusAddr,
        _high: BusAddr,
    ) -> Result<usize, Errno> {
        Err(Errno::EOPNOTSUPP)
    }

    unsafe fn bus_dmamem_free(_t: Self::DmaTag, _segs: &[Self::DmaSegment]) {}

    fn bus_dmamem_map(
        _t: Self::DmaTag,
        _segs: &mut [Self::DmaSegment],
        _size: usize,
        _flags: i32,
    ) -> Result<NonNull<u8>, Errno> {
        Err(Errno::EOPNOTSUPP)
    }

    unsafe fn bus_dmamem_unmap(_t: Self::DmaTag, _kva: NonNull<u8>, _size: usize) {}

    fn bus_dmamem_mmap(
        _t: Self::DmaTag,
        _segs: &[Self::DmaSegment],
        _off: Off,
        _prot: i32,
        _flags: i32,
    ) -> Option<Paddr> {
        None
    }
}

impl PciMachdep for Machine {
    type PciChipsetTag = HostPciChipset;
    type Pcitag = HostPcitag;
    type PciIntrHandle = HostPciIntrHandle;

    const PCI_MSI_PER_BRIDGE: bool = false;

    fn pci_attach_hook(_parent: &Device, _self: &Device, _pba: &PcibusAttachArgs) {}

    fn pci_bus_maxdevs(_pc: HostPciChipset, _busno: i32) -> i32 {
        32
    }

    fn pci_make_tag(_pc: HostPciChipset, bus: i32, device: i32, function: i32) -> HostPcitag {
        HostPcitag {
            bus,
            device,
            function,
        }
    }

    fn pci_decompose_tag(_pc: HostPciChipset, tag: HostPcitag) -> (i32, i32, i32) {
        (tag.bus, tag.device, tag.function)
    }

    fn pci_conf_size(_pc: HostPciChipset, _tag: HostPcitag) -> i32 {
        0x100
    }

    fn pci_conf_read(_pc: HostPciChipset, tag: HostPcitag, reg: i32) -> Pcireg {
        let mut conf = HOST_PCI_CONF.lock().unwrap_or_else(|e| e.into_inner());
        conf.as_mut()
            .map_or(0xffff_ffff, |(read, _)| read(tag, reg))
    }

    fn pci_conf_write(_pc: HostPciChipset, tag: HostPcitag, reg: i32, data: Pcireg) {
        let mut conf = HOST_PCI_CONF.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((_, write)) = conf.as_mut() {
            write(tag, reg, data);
        }
    }

    fn pci_probe_device_hook(_pc: HostPciChipset, _pa: &mut PciAttachArgs) -> i32 {
        0
    }

    fn pci_dev_postattach(_dev: &Device, _pa: &PciAttachArgs) {}

    fn pci_min_powerstate(_pc: HostPciChipset, _tag: HostPcitag) -> Pcireg {
        0
    }

    fn pci_set_powerstate_md(_pc: HostPciChipset, _tag: HostPcitag, _state: i32, _pre: i32) {}

    fn pci_msix_table_map(
        _pc: HostPciChipset,
        _tag: HostPcitag,
        _memt: HostBusSpace,
    ) -> Result<HostBusSpaceHandle, Errno> {
        Err(Errno::EOPNOTSUPP)
    }

    fn pci_msix_table_unmap(
        _pc: HostPciChipset,
        _tag: HostPcitag,
        _memt: HostBusSpace,
        _memh: HostBusSpaceHandle,
    ) {
    }

    fn pci_intr_enable_msivec(_pa: &PciAttachArgs, _num_vec: i32) -> bool {
        true
    }

    fn pci_intr_map_msi(_pa: &PciAttachArgs) -> Option<HostPciIntrHandle> {
        None
    }

    fn pci_intr_map_msivec(_pa: &PciAttachArgs, _vec: i32) -> Option<HostPciIntrHandle> {
        None
    }

    fn pci_intr_map_msix(_pa: &PciAttachArgs, _vec: i32) -> Option<HostPciIntrHandle> {
        None
    }

    fn pci_intr_map(_pa: &PciAttachArgs) -> Option<HostPciIntrHandle> {
        None
    }

    fn pci_intr_string(_pc: HostPciChipset, _ih: HostPciIntrHandle) -> PciIntrStr {
        PciIntrStr::new(format_args!("host"))
    }

    fn pci_intr_establish_cpu(
        _pc: HostPciChipset,
        _ih: HostPciIntrHandle,
        _level: i32,
        _ci: Option<&'static HostCpuInfo>,
        _func: PciIntrFn,
        _arg: *mut c_void,
        _what: &'static str,
    ) -> Option<NonNull<c_void>> {
        None
    }

    unsafe fn pci_intr_disestablish(_pc: HostPciChipset, _cookie: NonNull<c_void>) {}
}

impl DbMachdep for Machine {
    fn db_stack_trace_print(
        _addr: usize,
        _have_addr: bool,
        _count: usize,
        _modif: &[u8],
        pr: PrFn,
    ) {
        pr(format_args!("host: no stack trace\n"));
    }

    fn frame_address() -> usize {
        0
    }

    fn db_enter() {
        eprintln!("host: db_enter");
    }

    fn pc_regs() -> usize {
        0
    }
}

/// amd64's interrupt priority levels, so tests see a real machine's numbers.
impl Intr for Machine {
    const IPL_NONE: i32 = 0x0;
    const IPL_SOFTCLOCK: i32 = 0x1;
    const IPL_SOFTNET: i32 = 0x2;
    const IPL_SOFTTTY: i32 = 0x8;
    const IPL_BIO: i32 = 0x3;
    const IPL_NET: i32 = 0x4;
    const IPL_TTY: i32 = 0x9;
    const IPL_VM: i32 = 0xa;
    const IPL_AUDIO: i32 = 0xb;
    const IPL_CLOCK: i32 = 0xc;
    const IPL_SCHED: i32 = 0xc;
    const IPL_STATCLOCK: i32 = 0xc;
    const IPL_HIGH: i32 = 0xd;
    const IPL_IPI: i32 = 0xe;
    const IPL_MPFLOOR: i32 = 0x9;
    const IPL_MPSAFE: i32 = 0x100;
    const IPL_WAKEUP: i32 = 0x200;

    /// The host has no interrupts: the level is a number that is tracked and nothing more.
    fn splraise(ipl: i32) -> i32 {
        let old = HOST_IPL.load(core::sync::atomic::Ordering::Relaxed);
        HOST_IPL.store(old.max(ipl), core::sync::atomic::Ordering::Relaxed);
        old
    }

    fn spllower(ipl: i32) -> i32 {
        HOST_IPL.swap(ipl, core::sync::atomic::Ordering::Relaxed)
    }

    fn splx(s: i32) {
        HOST_IPL.store(s, core::sync::atomic::Ordering::Relaxed);
    }

    fn softintr(_si: i32) {}

    fn splassert_check(_wantipl: i32, _func: &str) {}

    fn intr_barrier(_cookie: NonNull<c_void>) {}
}

/// The host double's interrupt priority level.
static HOST_IPL: core::sync::atomic::AtomicI32 = core::sync::atomic::AtomicI32::new(0);

/// The host's `ioconf`: empty until a test installs one with `Machine::set_ioconf`.
static HOST_IOCONF: libkern::StaticCell<(&'static [Cfdata], &'static [i16])> =
    libkern::StaticCell::new((&[], &[]));

/// The host's `mainbus_cd`, for tests that attach a root named "mainbus".
static HOST_MAINBUS_CD: Cfdriver = Cfdriver::new(b"mainbus", DV_DULL, 0);

/// The host's autoconfiguration tables are whatever the test installed; `device_register`
/// does nothing, as on amd64 and arm64.
impl Autoconf for Machine {
    fn cfdata() -> &'static [Cfdata] {
        // SAFETY: written only by `set_ioconf`, under the test's lock.
        unsafe { HOST_IOCONF.read().0 }
    }

    fn cfroots() -> &'static [i16] {
        // SAFETY: as above.
        unsafe { HOST_IOCONF.read().1 }
    }

    fn mainbus_cd() -> &'static Cfdriver {
        &HOST_MAINBUS_CD
    }

    fn device_register(_dev: &Device, _aux: *mut c_void) {}

    fn pdevinit() -> &'static [crate::sys::device::Pdevinit] {
        &[]
    }
}

#[cfg(test)]
impl Machine {
    /// Installs a test's `cfdata[]` and `cfroots[]`.
    ///
    /// # Safety
    ///
    /// The caller holds the lock that serialises the tests using autoconfiguration.
    pub unsafe fn set_ioconf(cfdata: &'static [Cfdata], cfroots: &'static [i16]) {
        // SAFETY: the caller's lock excludes every reader.
        unsafe { HOST_IOCONF.write((cfdata, cfroots)) };
    }

    /// Installs (or, with `None`, removes) the configuration space `pci_conf_read` and
    /// `pci_conf_write` reach.
    pub fn set_pci_conf(conf: Option<HostPciConf>) {
        let mut slot = HOST_PCI_CONF.lock().unwrap_or_else(|e| e.into_inner());
        *slot = conf;
    }
}

/// The host has no device tree.
impl crate::machine::fdt::Fdt for Machine {
    type FdtAttachArgs<'a> = crate::machine::fdt::NoFdtAttachArgs<'a>;

    fn fdt_find_cons(_name: &[u8]) -> crate::dev::ofw::fdt::FdtNode {
        core::ptr::null()
    }

    fn stdout_node() -> i32 {
        0
    }

    fn fdt_cons_bs_tag() -> crate::machine::bus::BusSpaceTag {
        HostBusSpace
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

    unsafe fn fdt_intr_disestablish(_cookie: NonNull<c_void>) {}
}

/// The host has no user mode: no handler is ever entered and there is no trampoline.
impl MachineSignal for Machine {
    type Sigcontext = HostSigcontext;

    fn sendsig(
        _catcher: Sig,
        _sig: i32,
        _mask: Sigset,
        _ksip: &Siginfo,
        _info: bool,
        _onstack: bool,
    ) -> Result<(), Errno> {
        Ok(())
    }

    fn sys_sigreturn(_p: &Proc, _v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
        Err(Errno::ENOSYS)
    }

    fn sigcode() -> &'static [u8] {
        &[]
    }

    fn sigcoderet() -> usize {
        0
    }

    fn sigfill() -> &'static [u8] {
        &[]
    }
}

/// The host's `struct sigcontext`: nothing to save.
#[derive(Clone, Copy, Debug, Default)]
pub struct HostSigcontext;

/// The host has one address space: a "user" address is a pointer into the test's memory,
/// so the copies are plain byte copies and a null address is the one `EFAULT`.
impl UserCopy for Machine {
    fn copyin(uaddr: usize, kbuf: &mut [u8]) -> Result<(), Errno> {
        if uaddr == 0 {
            return Err(Errno::EFAULT);
        }
        // SAFETY: the test passed a pointer to `kbuf.len()` readable bytes.
        unsafe {
            core::ptr::copy_nonoverlapping(uaddr as *const u8, kbuf.as_mut_ptr(), kbuf.len())
        };
        Ok(())
    }

    fn copyout(kbuf: &[u8], uaddr: usize) -> Result<(), Errno> {
        if uaddr == 0 {
            return Err(Errno::EFAULT);
        }
        // SAFETY: the test passed a pointer to `kbuf.len()` writable bytes.
        unsafe { core::ptr::copy_nonoverlapping(kbuf.as_ptr(), uaddr as *mut u8, kbuf.len()) };
        Ok(())
    }

    fn copyinstr(uaddr: usize, kbuf: &mut [u8]) -> Result<usize, Errno> {
        if uaddr == 0 {
            return Err(Errno::EFAULT);
        }
        for (i, slot) in kbuf.iter_mut().enumerate() {
            // SAFETY: the test passed a pointer to a NUL-terminated string.
            let c = unsafe { (uaddr as *const u8).add(i).read() };
            *slot = c;
            if c == 0 {
                return Ok(i + 1);
            }
        }
        Err(Errno::ENAMETOOLONG)
    }

    fn copyoutstr(kbuf: &[u8], uaddr: usize) -> Result<usize, Errno> {
        if uaddr == 0 {
            return Err(Errno::EFAULT);
        }
        for (i, &c) in kbuf.iter().enumerate() {
            // SAFETY: the test passed a pointer to `kbuf.len()` writable bytes.
            unsafe { (uaddr as *mut u8).add(i).write(c) };
            if c == 0 {
                return Ok(i + 1);
            }
        }
        Err(Errno::ENAMETOOLONG)
    }

    unsafe fn kcopy(src: *const u8, dst: *mut u8, len: usize) -> Result<(), Errno> {
        // SAFETY: the caller's guarantee; the host has no faults to catch.
        unsafe { core::ptr::copy(src, dst, len) };
        Ok(())
    }
}
