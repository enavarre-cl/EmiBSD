/*	$OpenBSD: db_machdep.h,v 1.8 2025/07/22 09:20:41 kettenis Exp $	*/
/*	$NetBSD: db_machdep.h,v 1.5 2001/11/22 18:00:00 thorpej Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1996 Scott K Stevens
 *
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

//! arm64 `<machine/db_machdep.h>`: machine-dependent defines for new kernel debugger.
//!
//! Upstream: sys/arch/arm64/include/db_machdep.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M4 ports `db_regs_t`, `PC_REGS`/`SET_PC_REGS`, the breakpoint
//! instruction, the single-step bit helpers and the `IS_BREAKPOINT_TRAP`/
//! `IS_WATCHPOINT_TRAP` tests; M11c `db_expr_t` and the `inst_*` classifiers (which the C
//! marks "ALL BROKEN!!!": they are never true). `DB_MACHINE_COMMANDS` is defined: the table
//! is `arm64/db_interface.rs`'s. The `DDB_STATE_*` values come with the multiprocessor
//! entry. The entry points this header declares
//! (`db_ktrap`, `db_machine_init`) live in `arm64/db_interface.rs`; what `ddb/` itself needs
//! is the `machine::DbMachdep` contract.

use crate::arch::arm64::include::armreg::{EXCP_BRK, EXCP_WATCHPT_EL1, INSN_SIZE, PSR_SS};
use crate::arch::arm64::include::frame::Trapframe;

/// `db_expr_t`: expression - signed (`long`).
pub type DbExpr = i64;

/// `db_addr_t`: an address the debugger works on (`vaddr_t`; the header has no `db_addr_t`
/// any more, ddb uses `vaddr_t`).
pub type DbAddr = usize;

/// `db_regs_t`: the register state the debugger works on, a trap frame.
pub type DbRegs = Trapframe;

/// `PC_REGS(regs)`: the program counter of `regs`.
pub const fn pc_regs(regs: &DbRegs) -> usize {
    regs.tf_elr as usize
}

/// `SET_PC_REGS(regs, value)`.
pub fn set_pc_regs(regs: &mut DbRegs, value: usize) {
    regs.tf_elr = value as isize;
}

/// `BKPT_INST`: breakpoint instruction (`brk #0`).
pub const BKPT_INST: u32 = 0xd420_0000;
/// `BKPT_SIZE`: size of breakpoint inst.
pub const BKPT_SIZE: usize = INSN_SIZE;
/// `BKPT_SET(inst)`.
pub const fn bkpt_set(_inst: u32) -> u32 {
    BKPT_INST
}

/// `db_clear_single_step(regs)`.
pub fn db_clear_single_step(regs: &mut DbRegs) {
    regs.tf_spsr &= !(PSR_SS as isize);
}

/// `db_set_single_step(regs)`.
pub fn db_set_single_step(regs: &mut DbRegs) {
    regs.tf_spsr |= PSR_SS as isize;
}

/// `IS_BREAKPOINT_TRAP(type, code)`.
pub const fn is_breakpoint_trap(type_: i32, _code: i32) -> bool {
    type_ == EXCP_BRK as i32
}

/// `IS_WATCHPOINT_TRAP(type, code)`.
pub const fn is_watchpoint_trap(type_: i32, _code: i32) -> bool {
    type_ == EXCP_WATCHPT_EL1 as i32
}

/// `inst_trap_return(ins)`: `((ins) == 0 && (ins) == 1)` in C ("ALL BROKEN!!!"), never true.
pub const fn inst_trap_return(_ins: DbExpr) -> bool {
    false
}

/// `inst_return(ins)`: never true, as in C.
pub const fn inst_return(_ins: DbExpr) -> bool {
    false
}

/// `inst_call(ins)`: never true, as in C.
pub const fn inst_call(_ins: DbExpr) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pc_and_single_step() {
        let mut regs = DbRegs::new();
        set_pc_regs(&mut regs, 0x4000);
        assert_eq!(pc_regs(&regs), 0x4000);
        db_set_single_step(&mut regs);
        assert_ne!(regs.tf_spsr & PSR_SS as isize, 0);
        db_clear_single_step(&mut regs);
        assert_eq!(regs.tf_spsr & PSR_SS as isize, 0);
        assert!(is_breakpoint_trap(EXCP_BRK as i32, 0));
        assert!(is_watchpoint_trap(EXCP_WATCHPT_EL1 as i32, 0));
        assert!(!is_breakpoint_trap(EXCP_WATCHPT_EL1 as i32, 0));
        assert_eq!(BKPT_SIZE, 4);
    }
}
