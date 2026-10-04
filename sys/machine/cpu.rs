//! `<machine/cpu.h>`, `<machine/cpufunc.h>`, `boot(9)` and `delay(9)` as traits.
//!
//! Milestone M0 needs only the earliest setup, a way to park the CPU and a way to leave the
//! machine; M2 adds `boot(9)` (the end of `panic`) and `delay(9)` (the polled console); M4 adds
//! interrupt masking (`spl(9)`); M5 adds `curcpu()` and the `struct cpu_info` members the
//! clock and scheduler code reach (`ci_queue`, `ci_schedstate`, `ci_randseed`, `ci_curproc`),
//! the `CLKF_*` macros over the architecture's `struct clockframe`, `need_resched` and the
//! clock entry points `cpu_initclocks`/`cpu_startclock`/`setstatclockrate`; M5-b adds
//! `curproc` (`ci_curproc`, `set_curproc`), `proc0paddr`, the context switch
//! (`cpu_switchto`, `cpu_fork`), `clear_resched`, `cpu_unidle`, the idle loop hooks
//! (`cpu_idle_enter`/`cpu_idle_cycle`/`cpu_idle_leave`), `CPU_INFO_FOREACH` and the mutex
//! nesting counter.

use core::cell::Cell;
use core::ffi::c_void;

use crate::machine::Machine;
use crate::machine::bootinfo::BootInfo;
use crate::sys::clockintr::Clockqueue;
use crate::sys::exec::{ExecPackage, PsStrings};
use crate::sys::proc::Proc;
use crate::sys::sched::SchedstatePercpu;
use crate::sys::types::Vaddr;
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

    /// `CPU_CHR2BLK` (`<machine/cpu.h>`): the `machdep` sysctl converting a character major
    /// into a block one, where the machine defines it (`pledge_sysctl`'s `#ifdef`).
    const CPU_CHR2BLK: Option<i32> = None;

    /// `CPU_SSE` (`<machine/cpu.h>`): the `machdep` sysctl i386's libm reads, where defined.
    const CPU_SSE: Option<i32> = None;

    /// `CPU_ID_AA64ISAR0` (`<machine/cpu.h>`): arm64's instruction set attribute register 0
    /// sysctl, where defined.
    const CPU_ID_AA64ISAR0: Option<i32> = None;

    /// `CPU_ID_AA64ISAR1` (`<machine/cpu.h>`): arm64's instruction set attribute register 1
    /// sysctl, where defined.
    const CPU_ID_AA64ISAR1: Option<i32> = None;

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

    /// `curcpu()->ci_mutex_level`: how many mutexes this CPU holds (`assertwaitok`).
    fn curcpu_mutex_level() -> i32;

    /// `CPU_INFO_FOREACH(cii, ci)`: calls `f` for every CPU, the boot CPU first.
    fn cpu_info_foreach(f: &mut dyn FnMut(&'static Self::CpuInfo));

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

    /// `clear_resched(ci)`: `ci->ci_want_resched = 0`, once a switch happened.
    fn clear_resched(ci: &Self::CpuInfo);

    /// `cpu_unidle(ci)`: kicks an idle CPU that just got work (an IPI with
    /// `MULTIPROCESSOR`; nothing on one CPU, whose idle loop sees the run queue itself).
    fn cpu_unidle(ci: &Self::CpuInfo);

    /// `cpu_idle_enter()`: what the idle thread does before checking the run queues.
    fn cpu_idle_enter();

    /// `cpu_idle_cycle()`: waits for an interrupt (`hlt`, `wfi`) with nothing to run.
    fn cpu_idle_cycle();

    /// `cpu_idle_leave()`: the idle thread found work.
    fn cpu_idle_leave();

    /// `cpu_switchto(old, new)` (`locore.S`, `cpuswitch.S`): saves `old`'s kernel context
    /// into its pcb (none to save when `old` is `None`: the thread is dead), makes `new`
    /// `curproc` with `p_stat = SONPROC` and `p_cpu = curcpu()`, loads its pcb, stack and
    /// address space and resumes it. Returns on `old`'s stack once `old` is switched back
    /// to.
    ///
    /// # Safety
    ///
    /// Called with the scheduler lock held, from `mi_switch`/`sched_toidle` only: `new` is
    /// runnable and off every queue, `old` (when given) is the running thread, and both
    /// have a kernel stack and a pcb set up by `cpu_fork` or `locore`.
    unsafe fn cpu_switchto(old: Option<&Proc>, new: &Proc);

    /// `cpu_exit(p)` (`vm_machdep.c`): the machine-dependent part of a thread's exit, before
    /// its address space goes (nothing on amd64 and arm64).
    fn cpu_exit(p: &Proc);

    /// `cpu_fork(p1, p2, stack, tcb, func, arg)` (`vm_machdep.c`): finish a fork operation,
    /// with process `p2` nearly set up. Copy and update the kernel stack and pcb, making the
    /// child ready to run, and marking it so that it can return differently than the
    /// parent: the first time `p2` is switched to it runs `proc_trampoline`, which calls
    /// `proc_trampoline_mi` and then `func(arg)`. A non-null `stack`/`tcb` give a user
    /// thread its own stack and TCB.
    fn cpu_fork(
        p1: &Proc,
        p2: &Proc,
        stack: *mut u8,
        tcb: *mut u8,
        func: fn(*mut c_void),
        arg: *mut c_void,
    );

    /// `setregs(p, pack, stack, arginfo)` (`machdep.c`): clear registers on exec: `p` will
    /// return to user mode at `pack.ep_entry` with the stack pointer at `stack`, every other
    /// register zero and the machine state of a fresh thread.
    fn setregs(p: &Proc, pack: &ExecPackage<'_>, stack: Vaddr, arginfo: &PsStrings);

    /// `need_proftick(p)`: an interval timer of the thread `p` expired; it handles that on
    /// its way back to user mode (an AST).
    fn need_proftick(p: &Proc);

    /// `signotify(p)`: notify the thread `p` that it has a signal pending, to be processed
    /// as soon as possible (an AST on its way back to user mode).
    fn signotify(p: &Proc);

    /// `PROC_PC(p)`: the user program counter of `p` (its trap frame's).
    fn proc_pc(p: &Proc) -> usize;

    /// `PROC_STACK(p)`: the user stack pointer of `p` (its trap frame's).
    fn proc_stack(p: &Proc) -> usize;

    /// `child_return(arg)` (`trap.c`/`syscall.c`): the first thing a forked user thread runs
    /// (`arg` is the thread): a system call return of 0 to user mode. `fork1` and
    /// `thread_fork` hand it to `cpu_fork`.
    fn child_return(arg: *mut c_void);

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

/// `CPU_CHR2BLK` on the selected machine (`None`: not defined there).
pub const CPU_CHR2BLK: Option<i32> = <Machine as Cpu>::CPU_CHR2BLK;

/// `CPU_SSE` on the selected machine (`None`: not defined there).
pub const CPU_SSE: Option<i32> = <Machine as Cpu>::CPU_SSE;

/// `CPU_ID_AA64ISAR0` on the selected machine (`None`: not defined there).
pub const CPU_ID_AA64ISAR0: Option<i32> = <Machine as Cpu>::CPU_ID_AA64ISAR0;

/// `CPU_ID_AA64ISAR1` on the selected machine (`None`: not defined there).
pub const CPU_ID_AA64ISAR1: Option<i32> = <Machine as Cpu>::CPU_ID_AA64ISAR1;

/// `curcpu()` on the selected machine.
pub fn curcpu() -> &'static CpuInfo {
    Machine::curcpu()
}

/// `child_return` on the selected machine.
pub fn child_return(arg: *mut c_void) {
    Machine::child_return(arg)
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

/// `CPU_INFO_FOREACH` on the selected machine.
pub fn cpu_info_foreach(f: &mut dyn FnMut(&'static CpuInfo)) {
    Machine::cpu_info_foreach(f)
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
