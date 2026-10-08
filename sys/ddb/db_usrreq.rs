/*	$OpenBSD: db_usrreq.c,v 1.23 2025/05/19 21:48:28 kettenis Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1996 Michael Shalayeff.  All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT, INDIRECT,
 * INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT
 * NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
 * DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
 * THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF
 * THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! The `ddb` sysctl node and its tunables: `ddb/db_usrreq.c`.
//!
//! Upstream: sys/ddb/db_usrreq.c @ 3ce1f3f79392
//!
//! `ddb.radix`, `ddb.max_width`, `ddb.tab_stop_width`, `ddb.max_line`, `ddb.log` and
//! `ddb.suspend` are bounded integers (`ddb_vars`); `ddb.panic` and `ddb.console` can only be
//! lowered once the securelevel is raised; writing `ddb.trigger` enters the debugger, when
//! `ddb.console` allows it and the caller is on the console (or the securelevel is below 1).
//!
//! ## Deviations
//! - `db_panic` stays an `AtomicBool` (`kern/subr_prf.rs`; the arch `db_ktrap` reads it as
//!   one): `ddb.panic` is read and written through an `int` copy.
//! - `DDBPROF` is not configured: `ddb.profile` is not a name here, as in GENERIC
//!   (`DBCTL_PROFILE` falls to the bounded table and is `EOPNOTSUPP`).
//! - The trigger's console check takes the caller from `ddb_sysctl`'s `p` (C uses `curproc`,
//!   the same thread).

use core::sync::atomic::{AtomicI32, Ordering};

use crate::ddb::db_output::{DB_MAX_LINE_VAR, DB_MAX_WIDTH_VAR, DB_RADIX, DB_TAB_STOP_WIDTH};
use crate::ddb::db_var::{
    DBCTL_CONSOLE, DBCTL_LOG, DBCTL_MAXLINE, DBCTL_MAXWIDTH, DBCTL_PANIC, DBCTL_RADIX,
    DBCTL_SUSPEND, DBCTL_TABSTOP, DBCTL_TRIGGER,
};
use crate::dev::cons::cn_tab;
use crate::kern::kern_sysctl::{
    SECURELEVEL, sysctl_bounded_arr, sysctl_int_bounded, sysctl_int_lower, sysctl_rdint,
};
use crate::kern::subr_prf::{DB_CONSOLE, DB_PANIC};
use crate::machine::db_machdep::db_enter;
use crate::sys::errno::Errno;
use crate::sys::proc::{PS_CONTROLT, Proc};
use crate::sys::sysctl::SysctlBoundedArgs;

/// `db_log`: copy `ddb(4)` output into the kernel message buffer (0 or 1).
pub static DB_LOG: AtomicI32 = AtomicI32::new(1);
/// `db_profile`: allow dynamic profiling (`DDBPROF`).
pub static DB_PROFILE: AtomicI32 = AtomicI32::new(0);
/// `db_suspend`: allow suspend from ddb.
pub static DB_SUSPEND: AtomicI32 = AtomicI32::new(0);

/// `ddb_vars[]`: the bounded integers of the `ddb` node.
static DDB_VARS: [SysctlBoundedArgs; 6] = [
    SysctlBoundedArgs {
        mib: DBCTL_RADIX,
        var: &DB_RADIX,
        minimum: 8,
        maximum: 16,
    },
    SysctlBoundedArgs {
        mib: DBCTL_MAXWIDTH,
        var: &DB_MAX_WIDTH_VAR,
        minimum: 0,
        maximum: i32::MAX,
    },
    SysctlBoundedArgs {
        mib: DBCTL_TABSTOP,
        var: &DB_TAB_STOP_WIDTH,
        minimum: 1,
        maximum: 16,
    },
    SysctlBoundedArgs {
        mib: DBCTL_MAXLINE,
        var: &DB_MAX_LINE_VAR,
        minimum: 0,
        maximum: i32::MAX,
    },
    SysctlBoundedArgs {
        mib: DBCTL_LOG,
        var: &DB_LOG,
        minimum: 0,
        maximum: 1,
    },
    SysctlBoundedArgs {
        mib: DBCTL_SUSPEND,
        var: &DB_SUSPEND,
        minimum: 0,
        maximum: 1,
    },
];

/// `ddb.panic`, `ddb.console` and `ddb.profile`: once the securelevel is raised the value can
/// only be lowered, before it is 0 or 1.
fn ddb_sysctl_secure_int(
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
    var: &AtomicI32,
) -> Result<(), Errno> {
    if SECURELEVEL.load(Ordering::Relaxed) > 0 {
        sysctl_int_lower(oldp, oldlenp, newp, newlen, var)
    } else {
        sysctl_int_bounded(oldp, oldlenp, newp, newlen, var, 0, 1)
    }
}

/// Whether `p`'s process has the console as its controlling terminal: `pr->ps_flags &
/// PS_CONTROLT && cn_tab && cn_tab->cn_dev == pr->ps_session->s_ttyp->t_dev`.
fn ddb_on_console(p: &Proc) -> bool {
    let pr = p.process();
    if pr.ps_flags.load(Ordering::Relaxed) & PS_CONTROLT == 0 {
        return false;
    }
    let Some(cn) = cn_tab() else {
        return false;
    };
    // SAFETY: a process's session lives while the process is in it.
    let Some(s) = (unsafe { pr.session().as_ref() }) else {
        return false;
    };
    // SAFETY: with PS_CONTROLT the session has its terminal, which the driver keeps until it
    // detaches.
    let Some(tp) = (unsafe { s.s_ttyp.get().as_ref() }) else {
        return false;
    };
    cn.cn_dev.get() == tp.t_dev.get()
}

/// `ddb_sysctl`: the `ddb` sysctl level. All names at this level are terminal.
pub fn ddb_sysctl(
    name: &[i32],
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
    p: &Proc,
) -> Result<(), Errno> {
    // All sysctl names at this level are terminal.
    let [mib] = name else {
        return Err(Errno::ENOTDIR);
    };

    match *mib {
        DBCTL_PANIC => {
            let v = AtomicI32::new(i32::from(DB_PANIC.load(Ordering::Relaxed)));
            let r = ddb_sysctl_secure_int(oldp, oldlenp, newp, newlen, &v);
            DB_PANIC.store(v.load(Ordering::Relaxed) != 0, Ordering::Relaxed);
            r
        }
        DBCTL_CONSOLE => ddb_sysctl_secure_int(oldp, oldlenp, newp, newlen, &DB_CONSOLE),
        DBCTL_TRIGGER => {
            let mut newp = newp;
            if newp != 0 && DB_CONSOLE.load(Ordering::Relaxed) != 0 {
                if SECURELEVEL.load(Ordering::Relaxed) < 1 || ddb_on_console(p) {
                    db_enter();
                    newp = 0;
                } else {
                    return Err(Errno::ENODEV);
                }
            }
            sysctl_rdint(oldp, oldlenp, newp, 0)
        }
        // DBCTL_PROFILE: DDBPROF is not configured.
        _ => sysctl_bounded_arr(&DDB_VARS, name, oldp, oldlenp, newp, newlen),
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_sizes() {
        let p = Proc::new();
        let mut oldlen = 0;
        assert_eq!(
            ddb_sysctl(&[DBCTL_RADIX, 1], 0, &mut oldlen, 0, 0, &p),
            Err(Errno::ENOTDIR)
        );
        assert_eq!(ddb_sysctl(&[DBCTL_RADIX], 0, &mut oldlen, 0, 0, &p), Ok(()));
        assert_eq!(oldlen, size_of::<i32>());
        oldlen = 0;
        assert_eq!(
            ddb_sysctl(&[DBCTL_TRIGGER], 0, &mut oldlen, 0, 0, &p),
            Ok(())
        );
        assert_eq!(oldlen, size_of::<i32>());
        assert_eq!(ddb_sysctl(&[DBCTL_PANIC], 0, &mut oldlen, 0, 0, &p), Ok(()));
        assert_eq!(
            ddb_sysctl(&[DBCTL_CONSOLE], 0, &mut oldlen, 0, 0, &p),
            Ok(())
        );
        // DDBPROF is not configured
        let profile = crate::ddb::db_var::DBCTL_PROFILE;
        assert_eq!(
            ddb_sysctl(&[profile], 0, &mut oldlen, 0, 0, &p),
            Err(Errno::EOPNOTSUPP)
        );
    }
}
/* </TESTS> */
