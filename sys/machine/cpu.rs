//! `<machine/cpu.h>`, `<machine/cpufunc.h>`, `boot(9)` and `delay(9)` as traits.
//!
//! Milestone M0 needs only the earliest setup, a way to park the CPU and a way to leave the
//! machine; M2 adds `boot(9)` (the end of `panic`) and `delay(9)` (the polled console); M4 adds
//! interrupt masking (`spl(9)`); M5 adds `curcpu()` and the `struct cpu_info` members the
//! clock and scheduler code reach (`ci_queue`, `ci_schedstate`, `ci_randseed`, `ci_curproc`),
//! the `CLKF_*` macros over the architecture's `struct clockframe`, `need_resched` and the
//! clock entry points `cpu_initclocks`/`cpu_startclock`/`setstatclockrate`; M5-b adds
//! `curproc` (`ci_curproc`, `set_curproc`) and `proc0paddr`; context switching comes with
//! part b2.

use core::cell::Cell;

use crate::machine::Machine;
use crate::machine::bootinfo::BootInfo;
use crate::sys::clockintr::Clockqueue;
use crate::sys::proc::Proc;
use crate::sys::sched::SchedstatePercpu;
use crate::sys::user::User;

/// Outcome reported through [`Exit::exit`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitStatus {
    /// Everything the run set out to do happened.
    Success,
    /// The kernel gave up; the serial transcript says why when a console was available.
    Failure,
}

impl ExitStatus {
    /// The process exit status QEMU reports for this outcome. Odd numbers, because amd64's
    /// `isa-debug-exit` device can only produce `(v << 1) | 1`; arm64 passes the same values
    /// through semihosting so `xtask smoke` checks one number on both.
    pub const fn qemu_status(self) -> u32 {
        match self {
            ExitStatus::Success => 33,
            ExitStatus::Failure => 35,
        }
    }
}

/// The boot CPU, from the bootloader's hand-off through `cpu_startup`, and the per-CPU state.
pub trait Cpu {
    /// `struct cpu_info`: the architecture's per-CPU state. Generic code holds `&'static`
    /// references to it and reaches the members through the accessors below.
    type CpuInfo: 'static;

    /// `struct clockframe`: what the clock interrupt handlers get (`intrframe` on amd64,
    /// `trapframe` on arm64).
    type ClockFrame;

    /// `MAXCPUS`: the most CPUs this kernel supports.
    const MAXCPUS: u32;

    /// Earliest machine setup, called once by the boot glue before anything prints: OpenBSD's
    /// `init_x86_64` / `initarm`, as far as they are ported. It brings up the message buffer and
    /// the console (`consinit`), so everything after it can `printf`. The error is a fixed
    /// message because there is nowhere to print it yet; the glue turns it into a failure exit.
    ///
    /// # Safety
    ///
    /// Call exactly once, on the boot CPU, with the machine in the state the Limine protocol
    /// specifies at entry, and `boot` describing the image that was just loaded.
    unsafe fn early_init(boot: &BootInfo) -> Result<(), &'static str>;

    /// Masks interrupts and parks the CPU forever.
    fn halt() -> !;

    /// `boot(9)`: halts or reboots the machine according to the `RB_*` flags in `howto`
    /// (`sys/sys/reboot.rs`). `reboot()` in `kern/kern_xxx.rs` is its only caller.
    fn boot(howto: i32) -> !;

    /// `delay(9)`: busy-waits for at least `usec` microseconds.
    fn delay(usec: u32);

    /// `cpu_startup`: machine-dependent startup once the VM system is up; `main` calls it
    /// after `uvm_init`. It prints the memory sizes; the exec and physio maps, the buffer
    /// cache and the descriptor tables join it in later milestones.
    fn cpu_startup();

    /// `curcpu()`: this CPU's `cpu_info`.
    fn curcpu() -> &'static Self::CpuInfo;

    /// `curcpu()` as an opaque pointer: what lock owners and the soft interrupt runner
    /// record, compared for identity only.
    fn curcpu_ptr() -> *const ();

    /// `curcpu()->ci_mutex_level += delta` (`DIAGNOSTIC`): the mutex nesting counter.
    fn curcpu_mutex_level_add(delta: i32);

    /// `CPU_IS_PRIMARY(ci)`.
    fn cpu_is_primary(ci: &Self::CpuInfo) -> bool;

    /// `CPU_INFO_UNIT(ci)`: the CPU's device unit number.
    fn cpu_info_unit(ci: &Self::CpuInfo) -> u32;

    /// `ci->ci_queue`: the CPU's clock interrupt queue.
    fn ci_queue(ci: &Self::CpuInfo) -> &Clockqueue;

    /// `ci->ci_schedstate`: the CPU's scheduler state.
    fn ci_schedstate(ci: &Self::CpuInfo) -> &SchedstatePercpu;

    /// `ci->ci_randseed`: the seed of `random()` (`lib/libkern/random.c`).
    fn ci_randseed(ci: &Self::CpuInfo) -> &Cell<u32>;

    /// `ci->ci_curproc`: the thread running on the CPU, null before `proc0` is set up.
    fn ci_curproc(ci: &Self::CpuInfo) -> *const Proc;

    /// `ci->ci_curproc = p`: what `cpu_switchto` and `main`'s `curproc = &proc0` do.
    fn set_curproc(ci: &Self::CpuInfo, p: *const Proc);

    /// `proc0paddr`: the u-area of `proc0` (`locore` reserves it in C).
    fn proc0paddr() -> &'static User;

    /// `ci->ci_idepth`: the interrupt nesting depth.
    fn ci_idepth(ci: &Self::CpuInfo) -> u32;

    /// `CLKF_USERMODE(frame)`: whether the clock interrupt came from user mode.
    fn clkf_usermode(frame: &Self::ClockFrame) -> bool;

    /// `CLKF_PC(frame)`: the interrupted program counter.
    fn clkf_pc(frame: &Self::ClockFrame) -> usize;

    /// `CLKF_INTR(frame)`: whether the clock interrupt interrupted another interrupt handler.
    fn clkf_intr(frame: &Self::ClockFrame) -> bool;

    /// `need_resched(ci)`: asks `ci` to reschedule at the next opportunity.
    fn need_resched(ci: &Self::CpuInfo);

    /// `cpu_initclocks()`: the machine-dependent part of `initclocks`: picks the clock
    /// hardware, sets `stathz`/`profhz`, registers the timecounter.
    fn cpu_initclocks();

    /// `cpu_startclock()`: starts dispatching clock interrupts on the calling CPU.
    fn cpu_startclock();

    /// `setstatclockrate(newhz)`: changes the statistics clock's rate, where the hardware
    /// has a separate one.
    fn setstatclockrate(newhz: i32);

    /// `cpu_configure()` (`autoconf.c`): the machine-dependent part of autoconfiguration;
    /// ends with `spl0()` and `cold = 0`.
    fn cpu_configure();
}

/// `struct cpu_info` on the selected machine.
pub type CpuInfo = <Machine as Cpu>::CpuInfo;

/// `struct clockframe` on the selected machine.
pub type ClockFrame = <Machine as Cpu>::ClockFrame;

/// `MAXCPUS` on the selected machine.
pub const MAXCPUS: u32 = <Machine as Cpu>::MAXCPUS;

/// `curcpu()` on the selected machine.
pub fn curcpu() -> &'static CpuInfo {
    Machine::curcpu()
}

/// `curproc`: the thread running on this CPU, `None` before `proc0` is set up.
pub fn curproc() -> Option<&'static Proc> {
    // SAFETY: `ci_curproc` names a thread that is on the CPU, hence alive.
    unsafe { Machine::ci_curproc(Machine::curcpu()).as_ref() }
}

/// `cpu_configure` on the selected machine.
pub fn cpu_configure() {
    Machine::cpu_configure()
}

/// `cpu_startup` on the selected machine.
pub fn cpu_startup() {
    Machine::cpu_startup()
}

/// `cpu_initclocks` on the selected machine.
pub fn cpu_initclocks() {
    Machine::cpu_initclocks()
}

/// `cpu_startclock` on the selected machine.
pub fn cpu_startclock() {
    Machine::cpu_startclock()
}

/// `setstatclockrate` on the selected machine.
pub fn setstatclockrate(newhz: i32) {
    Machine::setstatclockrate(newhz)
}

/// `need_resched` on the selected machine.
pub fn need_resched(ci: &CpuInfo) {
    Machine::need_resched(ci)
}

/// `boot(9)` on the selected machine.
pub fn boot(howto: i32) -> ! {
    Machine::boot(howto)
}

/// `delay(9)` on the selected machine.
pub fn delay(usec: u32) {
    Machine::delay(usec)
}

/// How the kernel leaves the machine.
pub trait Exit {
    /// Leaves with `status`: under QEMU (feature `qemu`) the emulator exits with
    /// [`ExitStatus::qemu_status`], which `xtask smoke` checks; without it the CPU is halted.
    fn exit(status: ExitStatus) -> !;
}
