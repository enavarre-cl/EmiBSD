/*	$OpenBSD: db_variables.c,v 1.22 2023/03/08 04:43:07 guenther Exp $	*/
/*	$NetBSD: db_variables.c,v 1.8 1996/02/05 01:57:19 christos Exp $	*/
/*	$OpenBSD: db_variables.h,v 1.8 2016/01/25 14:30:30 mpi Exp $	*/
/*	$NetBSD: db_variables.h,v 1.5 1996/02/05 01:57:21 christos Exp $	*/
/* <LICENSES> */
/*
 * Mach Operating System
 * Copyright (c) 1993,1992,1991,1990 Carnegie Mellon University
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
/*
 * Mach Operating System
 * Copyright (c) 1993,1992,1991,1990 Carnegie Mellon University
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
 *
 * 	Author: David B. Golub, Carnegie Mellon University
 *	Date:	7/90
 */
/* </LICENSES> */

//! Debugger variables: `ddb/db_variables.c` and `<ddb/db_variables.h>`.
//!
//! Upstream: sys/ddb/db_variables.c @ 3ce1f3f79392
//! Upstream: sys/ddb/db_variables.h @ 3ce1f3f79392
//!
//! `$name` in an expression reads a debugger variable: one of `db_vars` (`radix`, `maxoff`,
//! `maxwidth`, `tabstops`, `lines`, `log`, the tunables of `db_output.c`, `db_sym.c` and
//! `db_usrreq.c`) or one of the machine's registers, `db_regs[]`, which live in the trap
//! frame the debugger was entered with. `set $name = value` writes one.
//!
//! ## Deviations
//! - `struct db_variable`'s `long *valuep` is an `Option<&AtomicI32>`: every C entry that
//!   has one points to an `int` read through `db_var_rw_int`. Registers have no pointer:
//!   their `fcn` reads and writes the field of `ddb_regs` ([`db_reg_var!`]), which C reaches
//!   through `valuep` and `FCN_NULL`.
//! - `db_regs[]` comes from the machine (`machine::db_machdep::db_regs()`); `db_evars` and
//!   `db_eregs` are the ends of the slices.
//! - `db_maxoff` belongs to `db_sym.c`, which is not ported; it lives here until it is.
//! - `db_find_variable` returns the variable, `db_get_variable` its value and
//!   `db_set_variable` nothing, in a [`DbResult`]: their C return value 0 is unreachable
//!   (`db_find_variable` calls `db_error` first).

use core::sync::atomic::{AtomicI32, Ordering};

use crate::ddb::db_command::{DbResult, db_error};
use crate::ddb::db_expr::db_expression;
use crate::ddb::db_lex::{
    db_read_token, db_tok_string, db_unread_token, tDOLLAR, tEOL, tEQ, tIDENT,
};
use crate::ddb::db_output::{DB_MAX_LINE_VAR, DB_MAX_WIDTH_VAR, DB_RADIX, DB_TAB_STOP_WIDTH};
use crate::ddb::db_usrreq::DB_LOG;
use crate::machine::db_machdep::{DbExpr, db_regs};

/// `DB_VAR_GET`: `fcn` reads the variable.
pub const DB_VAR_GET: i32 = 0;
/// `DB_VAR_SET`: `fcn` writes the variable.
pub const DB_VAR_SET: i32 = 1;

/// The function a variable is read and written through: `fcn(vp, valuep, DB_VAR_GET or
/// DB_VAR_SET)`.
pub type DbVarFn = fn(vp: &DbVariable, valuep: &mut DbExpr, op: i32) -> i32;

/// `struct db_variable`: a debugger variable.
pub struct DbVariable {
    /// `name`: name of variable.
    pub name: &'static str,
    /// `valuep`: value of variable (an `int`; `None` for registers).
    pub valuep: Option<&'static AtomicI32>,
    /// `fcn`: function to call when reading/writing; `None` is `FCN_NULL`.
    pub fcn: Option<DbVarFn>,
}

/// `db_reg_var!(DDB_REGS, "name", field)`: a [`DbVariable`] for one register of the trap
/// frame in the `StaticCell` `DDB_REGS` (an entry of the machine's `db_regs[]`), read and
/// written by value. `field` may be indexed: `tf_x[3]`.
#[macro_export]
macro_rules! db_reg_var {
    ($regs:path, $name:literal, $($field:tt)+) => {
        $crate::ddb::db_variables::DbVariable {
            name: $name,
            valuep: None,
            fcn: Some({
                fn rw(
                    _vp: &$crate::ddb::db_variables::DbVariable,
                    valuep: &mut $crate::machine::db_machdep::DbExpr,
                    op: i32,
                ) -> i32 {
                    // SAFETY: ddb_regs is only used by the CPU in the debugger, between
                    // db_ktrap's save and restore, and no other reference to it is live
                    // during this one access.
                    let regs = unsafe { $regs.get_mut() };
                    if op == $crate::ddb::db_variables::DB_VAR_SET {
                        regs.$($field)+ = *valuep as _;
                    } else {
                        *valuep = regs.$($field)+ as $crate::machine::db_machdep::DbExpr;
                    }
                    0
                }
                rw
            }),
        }
    };
}

/// `db_maxoff` (`db_sym.c`): like gdb's "max-symbolic-offset".
pub static DB_MAXOFF: AtomicI32 = AtomicI32::new(0x1000_0000);

/// `db_vars[]`: the debugger's own variables.
pub static DB_VARS: [DbVariable; 6] = [
    DbVariable {
        name: "radix",
        valuep: Some(&DB_RADIX),
        fcn: Some(db_var_rw_int),
    },
    DbVariable {
        name: "maxoff",
        valuep: Some(&DB_MAXOFF),
        fcn: Some(db_var_rw_int),
    },
    DbVariable {
        name: "maxwidth",
        valuep: Some(&DB_MAX_WIDTH_VAR),
        fcn: Some(db_var_rw_int),
    },
    DbVariable {
        name: "tabstops",
        valuep: Some(&DB_TAB_STOP_WIDTH),
        fcn: Some(db_var_rw_int),
    },
    DbVariable {
        name: "lines",
        valuep: Some(&DB_MAX_LINE_VAR),
        fcn: Some(db_var_rw_int),
    },
    DbVariable {
        name: "log",
        valuep: Some(&DB_LOG),
        fcn: Some(db_var_rw_int),
    },
];

/// `db_find_variable`: reads a variable name and finds it among `db_vars` and `db_regs`.
pub fn db_find_variable() -> DbResult<&'static DbVariable> {
    let t = db_read_token()?;
    if t == tIDENT {
        let tok = db_tok_string();
        let name = tok.as_bytes();
        if let Some(vp) = DB_VARS.iter().find(|vp| vp.name.as_bytes() == name) {
            return Ok(vp);
        }
        if let Some(vp) = db_regs().iter().find(|vp| vp.name.as_bytes() == name) {
            return Ok(vp);
        }
    }
    Err(db_error(Some("Unknown variable\n")))
}

/// `db_get_variable`: reads a variable name and the variable's value.
pub fn db_get_variable() -> DbResult<DbExpr> {
    let vp = db_find_variable()?;
    let mut value = 0;
    db_read_variable(vp, &mut value);
    Ok(value)
}

/// `db_set_variable`: reads a variable name and sets the variable to `value`.
pub fn db_set_variable(mut value: DbExpr) -> DbResult {
    let vp = db_find_variable()?;
    db_write_variable(vp, &mut value);
    Ok(())
}

/// `db_read_variable`: the value of `vp`.
pub fn db_read_variable(vp: &DbVariable, valuep: &mut DbExpr) {
    match (vp.fcn, vp.valuep) {
        (Some(func), _) => {
            func(vp, valuep, DB_VAR_GET);
        }
        (None, Some(v)) => *valuep = DbExpr::from(v.load(Ordering::Relaxed)),
        (None, None) => {}
    }
}

/// `db_write_variable`: sets `vp` to `*valuep`.
pub fn db_write_variable(vp: &DbVariable, valuep: &mut DbExpr) {
    match (vp.fcn, vp.valuep) {
        (Some(func), _) => {
            func(vp, valuep, DB_VAR_SET);
        }
        (None, Some(v)) => v.store(*valuep as i32, Ordering::Relaxed),
        (None, None) => {}
    }
}

/// `db_set_cmd`: `set $name [=] value`.
pub fn db_set_cmd(_addr: DbExpr, _have_addr: bool, _count: DbExpr, _modif: &[u8]) -> DbResult {
    let t = db_read_token()?;
    if t != tDOLLAR {
        return Err(db_error(Some("Unknown variable\n")));
    }
    let vp = db_find_variable()?;

    let t = db_read_token()?;
    if t != tEQ {
        db_unread_token(t);
    }

    let Some(mut value) = db_expression()? else {
        return Err(db_error(Some("No value\n")));
    };
    if db_read_token()? != tEOL {
        return Err(db_error(Some("?\n")));
    }

    db_write_variable(vp, &mut value);
    Ok(())
}

/// `db_var_rw_int`: reads or writes the `int` behind `var.valuep`.
pub fn db_var_rw_int(var: &DbVariable, expr: &mut DbExpr, mode: i32) -> i32 {
    if let Some(v) = var.valuep {
        if mode == DB_VAR_SET {
            v.store(*expr as i32, Ordering::Relaxed);
        } else {
            *expr = DbExpr::from(v.load(Ordering::Relaxed));
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ddb::db_lex::{db_set_line, db_test_lock};

    #[test]
    fn get_and_set() {
        let _g = db_test_lock();
        db_set_line(b"tabstops\n");
        assert_eq!(db_get_variable(), Ok(8));
        db_set_line(b"$tabstops = 4\n");
        assert_eq!(db_set_cmd(0, false, -1, b""), Ok(()));
        assert_eq!(DB_TAB_STOP_WIDTH.load(Ordering::Relaxed), 4);
        db_set_line(b"$tabstops 8\n");
        assert_eq!(db_set_cmd(0, false, -1, b""), Ok(()));
        assert_eq!(DB_TAB_STOP_WIDTH.load(Ordering::Relaxed), 8);
        db_set_line(b"nosuchvar\n");
        assert!(db_get_variable().is_err());
        db_set_line(b"radix\n");
        assert!(db_set_cmd(0, false, -1, b"").is_err());
        db_set_line(b"$radix\n");
        assert!(db_set_cmd(0, false, -1, b"").is_err());
    }
}
