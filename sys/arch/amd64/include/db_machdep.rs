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

/*
 * Mach Operating System
 * Copyright (c) 1991,1990 Carnegie Mellon University
 * All Rights Reserved.
 *
 * Permission to use, copy, modify and distribute this software and its
 * documentation is hereby granted, provided that both the copyright
 * notice and this permission notice appear in all copies of the
 * software, derivative works or modified versions, and any portions
 * thereof, and that both notices appear in supporting documentation.
 *
 * CARNEGIE MELLON ALLOWS FREE USE OF THIS SOFTWARE IN ITS "AS IS"
 * CONDITION.  CARNEGIE MELLON DISCLAIMS ANY LIABILITY OF ANY KIND FOR
 * ANY DAMAGES WHATSOEVER RESULTING FROM THE USE OF THIS SOFTWARE.
 *
 * Carnegie Mellon requests users of this software to return to
 *
 *  Software Distribution Coordinator  or  Software.Distribution@CS.CMU.EDU
 *  School of Computer Science
 *  Carnegie Mellon University
 *  Pittsburgh PA 15213-3890
 *
 * any improvements or extensions that they make and grant Carnegie Mellon
 * the rights to redistribute these changes.
 */
/* </LICENSES> */

/* <CODE> */
//! amd64 `<machine/db_machdep.h>`: machine-dependent defines for new kernel debugger.
//!
//! Upstream: sys/arch/amd64/include/db_machdep.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M4 ports `db_regs_t`, `PC_REGS`/`SET_PC_REGS`, the breakpoint
//! instruction, `FIXUP_PC_AFTER_BREAK`, the single-step bit helpers and the
//! `IS_BREAKPOINT_TRAP`/`IS_WATCHPOINT_TRAP` tests; M11c `db_expr_t` and the `inst_*`
//! classifiers (`db_run.c`). `DB_MACHINE_COMMANDS` is defined: the table is
//! `amd64/db_interface.rs`'s. The `DDB_STATE_*` values are here.
//! The entry points this header declares (`db_ktrap`, `db_machine_init`, ...) live in
//! `amd64/db_interface.rs`; what `ddb/` itself needs is the `machine::DbMachdep` contract.

use crate::arch::amd64::include::frame::Trapframe;
use crate::arch::amd64::include::psl::PSL_T;
use crate::arch::amd64::include::trap::{T_BPTFLT, T_TRCTRAP};

/// `db_expr_t`: expression - signed (`long`).
pub type DbExpr = i64;

/// `db_addr_t`: an address the debugger works on (`vaddr_t`; the header has no `db_addr_t`
/// any more, ddb uses `vaddr_t`).
pub type DbAddr = usize;

/// `db_regs_t`: the register state the debugger works on, a trap frame.
pub type DbRegs = Trapframe;

/// `PC_REGS(regs)`: the program counter of `regs`.
pub const fn pc_regs(regs: &DbRegs) -> usize {
    regs.tf_rip as usize
}

/// `SET_PC_REGS(regs, value)`.
pub fn set_pc_regs(regs: &mut DbRegs, value: usize) {
    regs.tf_rip = value as i64;
}

/// `BKPT_ADDR(addr)`: breakpoint address.
pub const fn bkpt_addr(addr: usize) -> usize {
    addr
}
/// `BKPT_INST`: breakpoint instruction (`int3`).
pub const BKPT_INST: u8 = 0xcc;
/// `BKPT_SIZE`: size of breakpoint inst.
pub const BKPT_SIZE: usize = 1;
/// `BKPT_SET(inst)`.
pub const fn bkpt_set(_inst: u8) -> u8 {
    BKPT_INST
}

/// `SSF_INST`: `pushq %rbp`, the first instruction of a function with a frame.
pub const SSF_INST: u8 = 0x55;
/// `SSF_SIZE`.
pub const SSF_SIZE: usize = 1;

/// `FIXUP_PC_AFTER_BREAK(regs)`: `int3` is a trap, so the saved `rip` is past it.
pub fn fixup_pc_after_break(regs: &mut DbRegs) {
    regs.tf_rip -= BKPT_SIZE as i64;
}

/// `db_clear_single_step(regs)`.
pub fn db_clear_single_step(regs: &mut DbRegs) {
    regs.tf_rflags &= !(PSL_T as i64);
}

/// `db_set_single_step(regs)`.
pub fn db_set_single_step(regs: &mut DbRegs) {
    regs.tf_rflags |= PSL_T as i64;
}

/// `IS_BREAKPOINT_TRAP(type, code)`.
pub const fn is_breakpoint_trap(type_: i32, _code: i32) -> bool {
    type_ == T_BPTFLT
}

/// `IS_WATCHPOINT_TRAP(type, code)`: a debug trap with one of the `DR6.B0-B3` bits.
pub const fn is_watchpoint_trap(type_: i32, code: i32) -> bool {
    type_ == T_TRCTRAP && (code & 15) != 0
}

/// `I_CALL`: `call rel32`.
pub const I_CALL: i64 = 0xe8;
/// `I_CALLI`: the `0xff` group, a call through a register or memory with `/2`.
pub const I_CALLI: i64 = 0xff;
/// `I_RET`: `ret`.
pub const I_RET: i64 = 0xc3;
/// `I_IRET`: `iret`.
pub const I_IRET: i64 = 0xcf;

/// `inst_trap_return(ins)`.
pub const fn inst_trap_return(ins: DbExpr) -> bool {
    (ins & 0xff) == I_IRET
}

/// `inst_return(ins)`.
pub const fn inst_return(ins: DbExpr) -> bool {
    (ins & 0xff) == I_RET
}

/// `inst_call(ins)`: `call rel32`, or `0xff /2` (the ModRM reg field is 2).
pub const fn inst_call(ins: DbExpr) -> bool {
    (ins & 0xff) == I_CALL || ((ins & 0xff) == I_CALLI && (ins & 0x3800) == 0x1000)
}

/// `DDB_STATE_NOT_RUNNING`: no CPU is in ddb (`MULTIPROCESSOR`).
pub const DDB_STATE_NOT_RUNNING: i32 = 0;
/// `DDB_STATE_RUNNING`: one CPU runs ddb, the others are held.
pub const DDB_STATE_RUNNING: i32 = 1;
/// `DDB_STATE_EXITING`: the ddb CPU is leaving; the others resume.
pub const DDB_STATE_EXITING: i32 = 2;
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn breakpoint_fixup_and_tests() {
        let mut regs = DbRegs {
            tf_rip: 0x1001,
            ..DbRegs::default()
        };
        fixup_pc_after_break(&mut regs);
        assert_eq!(pc_regs(&regs), 0x1000);
        set_pc_regs(&mut regs, 0x2000);
        assert_eq!(regs.tf_rip, 0x2000);
        db_set_single_step(&mut regs);
        assert_ne!(regs.tf_rflags & PSL_T as i64, 0);
        db_clear_single_step(&mut regs);
        assert_eq!(regs.tf_rflags & PSL_T as i64, 0);
        assert!(is_breakpoint_trap(T_BPTFLT, 0));
        assert!(!is_breakpoint_trap(T_TRCTRAP, 0));
        assert!(is_watchpoint_trap(T_TRCTRAP, 2));
        assert!(!is_watchpoint_trap(T_TRCTRAP, 0x4000));
        assert!(inst_call(0xe8) && inst_call(0x10ff) && !inst_call(0x20ff));
        assert!(inst_return(0x12c3) && inst_trap_return(0xcf) && !inst_return(0xcf));
    }
}
/* </TESTS> */
