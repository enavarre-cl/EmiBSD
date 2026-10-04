/*	$OpenBSD: db_machdep.h,v 1.19 2021/08/30 08:11:12 jasper Exp $	*/
/*	$NetBSD: db_machdep.h,v 1.2 2003/04/29 17:06:04 scw Exp $	*/
/* <LICENSES> */
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

//! amd64 `<machine/db_machdep.h>`: machine-dependent defines for new kernel debugger.
//!
//! Upstream: sys/arch/amd64/include/db_machdep.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M4 ports `db_regs_t`, `PC_REGS`/`SET_PC_REGS`, the breakpoint
//! instruction, `FIXUP_PC_AFTER_BREAK`, the single-step bit helpers and the
//! `IS_BREAKPOINT_TRAP`/`IS_WATCHPOINT_TRAP` tests. `db_expr_t`, the `inst_*` classifiers
//! (`db_run.c`), the `DDB_STATE_*` values and `DB_MACHINE_COMMANDS` come with the command
//! loop and the multiprocessor entry. The entry points this header declares (`db_ktrap`,
//! `db_machine_init`, ...) live in `amd64/db_interface.rs`; what `ddb/` itself needs is the
//! `machine::DbMachdep` contract.

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
    }
}
