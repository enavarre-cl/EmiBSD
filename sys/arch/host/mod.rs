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

use core::cell::Cell;
use std::collections::BTreeMap;
use std::eprintln;
use std::io::Write;
use std::sync::Mutex;
use std::vec;

use crate::dev::cons::{CN_LOWPRI, Consdev, set_cn_tab};
use crate::machine::bus::{BusAddr, BusSize, BusSpace};
use crate::machine::db_machdep::{DbMachdep, PrFn};
use crate::machine::proc::MachineProc;
use crate::machine::{
    BootInfo, Console, Cpu, Exit, ExitStatus, Intr, MachineInfo, MachineParam, Pmap, VmParam,
};
use crate::sys::clockintr::Clockqueue;
use crate::sys::errno::Errno;
use crate::sys::param::NODEV;
use crate::sys::proc::Proc;
use crate::sys::sched::SchedstatePercpu;
use crate::sys::types::{Dev, Paddr, Vaddr, Vsize};
use crate::sys::user::User;
use crate::uvm::uvm_extern::{UvmConstraintRange, VmProt};
use crate::uvm::uvm_page::{
    PHYS_TO_VM_PAGE, VM_PSTRAT_BIGFIRST, VmPage, uvm_page_physsteal, vm_page_to_phys,
};

/// The host implementation of the machine interface.
pub struct Machine;

/// The host's one bus space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostBusSpace;

/// A host bus space handle: the address that was "mapped".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostBusSpaceHandle(usize);

/// The host's `struct pmap`: the kernel mappings, page by page.
pub struct HostPmap {
    mappings: Mutex<BTreeMap<usize, usize>>,
}

/// Where the host pretends kernel virtual space starts (amd64's `VM_MIN_KERNEL_ADDRESS`).
const HOST_KVA_START: usize = 0xffff_8000_0000_0000;
/// Where it ends (amd64's `VM_MAX_KERNEL_ADDRESS`).
const HOST_KVA_END: usize = 0xffff_8080_0000_0000;

/// The host's kernel pmap.
static HOST_PMAP: HostPmap = HostPmap {
    mappings: Mutex::new(BTreeMap::new()),
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

/// The host's `struct pcb`: nothing to switch.
#[derive(Default)]
pub struct HostPcb;

impl MachineProc for Machine {
    type Mdproc = HostMdproc;
    const MDPROC_INIT: HostMdproc = HostMdproc;
    type Pcb = HostPcb;
    const PCB_INIT: HostPcb = HostPcb;
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
}

impl Pmap for Machine {
    type VmPageMd = ();
    type Pmap = HostPmap;

    const VM_MDPAGE_INIT: () = ();
    const HAVE_PMAP_DIRECT: bool = true;
    const PMAP_STEAL_MEMORY: bool = true;
    const UVM_MD_CONSTRAINTS: &'static [&'static UvmConstraintRange] =
        &[&ISA_CONSTRAINT, &DMA_CONSTRAINT];
    const DMA_CONSTRAINT: &'static UvmConstraintRange = &DMA_CONSTRAINT;

    fn pmap_kernel() -> &'static HostPmap {
        &HOST_PMAP
    }

    /// Page contents are not modelled.
    fn pmap_zero_page(_pg: &VmPage) {}

    /// Page contents are not modelled.
    fn pmap_copy_page(_src: &VmPage, _dst: &VmPage) {}

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
}

/// The host double's interrupt priority level.
static HOST_IPL: core::sync::atomic::AtomicI32 = core::sync::atomic::AtomicI32::new(0);

/// The host has no device tree.
impl crate::machine::fdt::Fdt for Machine {
    fn fdt_find_cons(_name: &[u8]) -> crate::dev::ofw::fdt::FdtNode {
        core::ptr::null()
    }

    fn stdout_node() -> i32 {
        0
    }

    fn fdt_cons_bs_tag() -> crate::machine::bus::BusSpaceTag {
        HostBusSpace
    }
}
