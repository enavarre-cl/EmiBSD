//! `<machine/db_machdep.h>` and `db_trace.c` as a trait: what `ddb(4)` needs from the machine.
//!
//! Milestone M2 ("ddb-lite") needs only a stack trace from the current frame, which
//! `db_stack_dump` (`ddb/db_output.rs`) prints at panic time. M4 adds the trap frame the
//! debugger was entered with (`ddb_regs`): the trace without an address starts there and
//! `PC_REGS(&ddb_regs)` is where the kernel stopped. Breakpoints and single-stepping come
//! with `db_run.c`.

use core::fmt;

use crate::machine::Machine;

/// The output function `db_stack_trace_print` prints through: `printf` from `db_stack_dump`,
/// `db_printf` from the debugger's `trace` command. Rust format arguments replace C's
/// `(const char *, ...)`.
pub type PrFn = fn(fmt::Arguments<'_>);

/// The debugger's machine-dependent entry points.
pub trait DbMachdep {
    /// `db_stack_trace_print`: walks the frame-pointer chain and prints one line per frame
    /// through `pr`. With `have_addr`, `addr` is the frame to start from (a `struct callframe`);
    /// without it the trace starts at the trap frame `ddb_regs`, which exists from M4. `count`
    /// bounds the number of frames; `modif` carries the `trace` command's modifiers (`t`: the
    /// address is a thread id, `u`: continue into user frames).
    fn db_stack_trace_print(addr: usize, have_addr: bool, count: usize, modif: &[u8], pr: PrFn);

    /// `__builtin_frame_address(0)`: the caller's frame pointer. Implementations are
    /// `#[inline(always)]`, so the frame is the one of the function that calls this.
    fn frame_address() -> usize;

    /// `db_enter` (`db_interface.c`): enters the debugger with a breakpoint instruction,
    /// which the trap handler hands to `db_ktrap`.
    fn db_enter();

    /// `PC_REGS(&ddb_regs)`: the program counter of the trap frame the debugger was entered
    /// with. Valid while `db_active`, after `db_ktrap` saved the frame.
    fn pc_regs() -> usize;
}

/// `db_enter` on the selected machine.
pub fn db_enter() {
    Machine::db_enter()
}

/// `db_stack_trace_print` on the selected machine.
pub fn db_stack_trace_print(addr: usize, have_addr: bool, count: usize, modif: &[u8], pr: PrFn) {
    Machine::db_stack_trace_print(addr, have_addr, count, modif, pr)
}

/// `PC_REGS(&ddb_regs)` on the selected machine.
pub fn pc_regs() -> usize {
    Machine::pc_regs()
}
