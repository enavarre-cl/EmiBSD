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
//! Different parameters, structures and functions of the debugger: `<ddb/db_var.h>`.
//!
//! Upstream: sys/ddb/db_var.h @ 3ce1f3f79392
//!
//! The header's `extern` tunables live in the files that define them, as in C: `db_radix`,
//! `db_max_width`, `db_tab_stop_width` and `db_max_line` in `ddb/db_output.rs`, `db_panic` and
//! `db_console` in `kern/subr_prf.rs`, `db_log`, `db_profile` and `db_suspend` in
//! `ddb/db_usrreq.rs`, which also has `ddb_sysctl`.
//!
//! ## Deviations
//! - `CTL_DDB_NAMES` is a [`Ctlname`] array, as the other `CTL_*_NAMES` are.

use crate::sys::sysctl::{CTLTYPE_INT, Ctlname};

/// `DB_HISTORY_SIZE`: the size of `db_input.c`'s command history ring.
pub const DB_HISTORY_SIZE: usize = 4000;

/// `DBCTL_RADIX`: `ddb.radix`.
pub const DBCTL_RADIX: i32 = 1;
/// `DBCTL_MAXWIDTH`: `ddb.max_width`.
pub const DBCTL_MAXWIDTH: i32 = 2;
/// `DBCTL_MAXLINE`: `ddb.max_line`.
pub const DBCTL_MAXLINE: i32 = 3;
/// `DBCTL_TABSTOP`: `ddb.tab_stop_width`.
pub const DBCTL_TABSTOP: i32 = 4;
/// `DBCTL_PANIC`: `ddb.panic`.
pub const DBCTL_PANIC: i32 = 5;
/// `DBCTL_CONSOLE`: `ddb.console`.
pub const DBCTL_CONSOLE: i32 = 6;
/// `DBCTL_LOG`: `ddb.log`.
pub const DBCTL_LOG: i32 = 7;
/// `DBCTL_TRIGGER`: `ddb.trigger`.
pub const DBCTL_TRIGGER: i32 = 8;
/// `DBCTL_PROFILE`: `ddb.profile`.
pub const DBCTL_PROFILE: i32 = 9;
/// `DBCTL_SUSPEND`: `ddb.suspend`.
pub const DBCTL_SUSPEND: i32 = 10;
/// `DBCTL_MAXID`: number of `ddb` names.
pub const DBCTL_MAXID: usize = 11;

/// `CTL_DDB_NAMES`: the names of the `ddb` sysctl level.
pub const CTL_DDB_NAMES: [Ctlname; DBCTL_MAXID] = [
    Ctlname::NONE,
    Ctlname::new(b"radix", CTLTYPE_INT),
    Ctlname::new(b"max_width", CTLTYPE_INT),
    Ctlname::new(b"max_line", CTLTYPE_INT),
    Ctlname::new(b"tab_stop_width", CTLTYPE_INT),
    Ctlname::new(b"panic", CTLTYPE_INT),
    Ctlname::new(b"console", CTLTYPE_INT),
    Ctlname::new(b"log", CTLTYPE_INT),
    Ctlname::new(b"trigger", CTLTYPE_INT),
    Ctlname::new(b"profile", CTLTYPE_INT),
    Ctlname::new(b"suspend", CTLTYPE_INT),
];
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/ddb/db_var.h");
        let ours: &[(&str, i64)] = &[
            ("DB_HISTORY_SIZE", DB_HISTORY_SIZE as i64),
            ("DBCTL_RADIX", DBCTL_RADIX.into()),
            ("DBCTL_MAXWIDTH", DBCTL_MAXWIDTH.into()),
            ("DBCTL_MAXLINE", DBCTL_MAXLINE.into()),
            ("DBCTL_TABSTOP", DBCTL_TABSTOP.into()),
            ("DBCTL_PANIC", DBCTL_PANIC.into()),
            ("DBCTL_CONSOLE", DBCTL_CONSOLE.into()),
            ("DBCTL_LOG", DBCTL_LOG.into()),
            ("DBCTL_TRIGGER", DBCTL_TRIGGER.into()),
            ("DBCTL_PROFILE", DBCTL_PROFILE.into()),
            ("DBCTL_SUSPEND", DBCTL_SUSPEND.into()),
            ("DBCTL_MAXID", DBCTL_MAXID as i64),
        ];
        for (name, value) in ours {
            assert_eq!(crate::reftest::int(&defs, name), Some(*value), "{name}");
        }
        assert_eq!(
            CTL_DDB_NAMES[DBCTL_TABSTOP as usize].ctl_name,
            Some(&b"tab_stop_width"[..])
        );
    }
}
/* </TESTS> */
