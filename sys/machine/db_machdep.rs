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
//! `<machine/db_machdep.h>` and `db_trace.c` as a trait: what `ddb(4)` needs from the machine.
//!
//! Milestone M2 ("ddb-lite") needs only a stack trace from the current frame, which
//! `db_stack_dump` (`ddb/db_output.rs`) prints at panic time. M4 adds the trap frame the
//! debugger was entered with (`ddb_regs`): the trace without an address starts there and
//! `PC_REGS(&ddb_regs)` is where the kernel stopped. M11c adds what the command loop needs:
//! `db_regs[]`, the machine command table and `db_machine_init` (`db_interface.c`), and for
//! `db_run.c` the `ddb_regs` accessors (`SET_PC_REGS`, `FIXUP_PC_AFTER_BREAK`,
//! `db_set_single_step`, `db_clear_single_step`), the trap tests and the `inst_*`
//! classifiers of the header. `db_expr_t` is [`DbExpr`], an address [`DbAddr`].

use core::fmt;

pub use crate::arch::current::include::db_machdep::{DbAddr, DbExpr};
use crate::ddb::db_command::DbCommand;
use crate::ddb::db_variables::DbVariable;
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

    /// `db_regs[]`: the machine registers as debugger variables (`$rax`, `$x0`, ...), read
    /// and written in `ddb_regs`.
    fn db_regs() -> &'static [DbVariable];

    /// `db_machine_command_table[]`: the `machine` commands (`DB_MACHINE_COMMANDS`), as a
    /// constant so `db_command.c`'s static table can name it.
    const DB_MACHINE_COMMAND_TABLE: &'static [DbCommand];

    /// `db_machine_command_table[]`, the same table through a function.
    fn db_machine_command_table() -> &'static [DbCommand];

    /// `db_machine_init`: machine-dependent debugger set-up, called before `boot -d` enters
    /// the debugger.
    fn db_machine_init();

    /// `SET_PC_REGS(&ddb_regs, pc)`.
    fn set_pc_regs(pc: usize);

    /// `FIXUP_PC_AFTER_BREAK(&ddb_regs)`: moves the PC back onto the breakpoint instruction
    /// where the trap leaves it past it; nothing where the C does not define the macro.
    fn fixup_pc_after_break();

    /// `db_set_single_step(&ddb_regs)`: trap after the next instruction.
    fn db_set_single_step();

    /// `db_clear_single_step(&ddb_regs)`.
    fn db_clear_single_step();

    /// `IS_BREAKPOINT_TRAP(type, code)`.
    fn is_breakpoint_trap(type_: i32, code: i32) -> bool;

    /// `IS_WATCHPOINT_TRAP(type, code)`.
    fn is_watchpoint_trap(type_: i32, code: i32) -> bool;

    /// `inst_trap_return(ins)`: whether `ins` returns from a trap.
    fn inst_trap_return(ins: DbExpr) -> bool;

    /// `inst_return(ins)`: whether `ins` returns from a call.
    fn inst_return(ins: DbExpr) -> bool;

    /// `inst_call(ins)`: whether `ins` is a call.
    fn inst_call(ins: DbExpr) -> bool;
}

/// `db_machine_command_table[]` of the selected machine.
pub const DB_MACHINE_COMMAND_TABLE: &[DbCommand] = <Machine as DbMachdep>::DB_MACHINE_COMMAND_TABLE;

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

/// `db_regs[]` on the selected machine.
pub fn db_regs() -> &'static [DbVariable] {
    Machine::db_regs()
}

/// `db_machine_command_table[]` on the selected machine.
pub fn db_machine_command_table() -> &'static [DbCommand] {
    Machine::db_machine_command_table()
}

/// `db_machine_init` on the selected machine.
pub fn db_machine_init() {
    Machine::db_machine_init()
}

/// `SET_PC_REGS(&ddb_regs, pc)` on the selected machine.
pub fn set_pc_regs(pc: usize) {
    Machine::set_pc_regs(pc)
}

/// `FIXUP_PC_AFTER_BREAK(&ddb_regs)` on the selected machine.
pub fn fixup_pc_after_break() {
    Machine::fixup_pc_after_break()
}

/// `db_set_single_step(&ddb_regs)` on the selected machine.
pub fn db_set_single_step() {
    Machine::db_set_single_step()
}

/// `db_clear_single_step(&ddb_regs)` on the selected machine.
pub fn db_clear_single_step() {
    Machine::db_clear_single_step()
}

/// `IS_BREAKPOINT_TRAP(type, code)` on the selected machine.
pub fn is_breakpoint_trap(type_: i32, code: i32) -> bool {
    Machine::is_breakpoint_trap(type_, code)
}

/// `IS_WATCHPOINT_TRAP(type, code)` on the selected machine.
pub fn is_watchpoint_trap(type_: i32, code: i32) -> bool {
    Machine::is_watchpoint_trap(type_, code)
}

/// `inst_trap_return(ins)` on the selected machine.
pub fn inst_trap_return(ins: DbExpr) -> bool {
    Machine::inst_trap_return(ins)
}

/// `inst_return(ins)` on the selected machine.
pub fn inst_return(ins: DbExpr) -> bool {
    Machine::inst_return(ins)
}

/// `inst_call(ins)` on the selected machine.
pub fn inst_call(ins: DbExpr) -> bool {
    Machine::inst_call(ins)
}
/* </CODE> */
