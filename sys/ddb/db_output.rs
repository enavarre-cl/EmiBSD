/*	$OpenBSD: db_output.c,v 1.37 2021/06/10 12:33:48 bluhm Exp $	*/
/*	$NetBSD: db_output.c,v 1.13 1996/04/01 17:27:14 christos Exp $	*/
/*	$OpenBSD: db_output.h,v 1.17 2021/02/09 14:37:13 jcs Exp $ */
/*	$NetBSD: db_output.h,v 1.9 1996/04/04 05:13:50 cgd Exp $	*/
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
 *
 * 	Author: David B. Golub, Carnegie Mellon University
 *	Date:	8/90
 */
/* </LICENSES> */

/* <CODE> */
//! Printf and character output for the debugger: `ddb/db_output.c` and `<ddb/db_output.h>`.
//!
//! Upstream: sys/ddb/db_output.c @ 3ce1f3f79392
//! Upstream: sys/ddb/db_output.h @ 3ce1f3f79392
//!
//! Character output tracks the position in the line. To do this correctly, we should know how
//! wide the output device is; then we could zero the line position when the output device
//! wraps around to the start of the next line. Instead, we count the number of spaces printed
//! since the last printing character so that we don't print trailing spaces. This avoids most
//! of the wraparounds.
//!
//! `db_printf` and `db_vprintf`, declared in the header, are implemented in
//! `kern/subr_prf.rs` as in C.
//!
//! Status: `wip`.
//!
//! ## Deviations
//! - The counters are atomics; ddb runs one CPU at a time.
//! - `db_stack_dump` tells a recursive traceback from a parallel one by `cpu_info`, which
//!   arrives with M5; with one CPU every second entry is "Faulted in traceback".
//! - `db_more`'s `q` calls `db_error(0)`, which cannot unwind out of `db_printf` here
//!   (`docs/C_TO_RUST.md`, the `db_error` row): it sets [`DB_QUIT`] instead, which drops
//!   every character `db_putchar` is given until `db_command_loop` clears it before its next
//!   prompt. The command runs to its end, silently.
//! - `db_format` returns the formatted bytes as a slice of `buf`; its `#` alternate form is
//!   Rust's (`0x`, `0o`), which differs from C's `%#lo` (`0`) for octal.

use core::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use crate::ddb::db_command::db_error;
use crate::dev::cons::{cngetc, cnputc};
use crate::kern::subr_prf::{printf, snprintf};
use crate::machine::Machine;
use crate::machine::db_machdep::{DbMachdep, db_stack_trace_print};

/// Maximum line.
pub const DB_MAX_LINE: i32 = 24;
/// Maximum width.
pub const DB_MAX_WIDTH: i32 = 80;

/// Minimum max width.
const DB_MIN_MAX_WIDTH: i32 = 20;
/// Minimum max line.
const DB_MIN_MAX_LINE: i32 = 3;

/// `CTRL(c)` as `db_output.c` defines it: `(c) & 0xff`, which on a byte is the byte itself.
#[allow(clippy::identity_op)] // the C expression, kept so the quirk stays visible
const fn ctrl(c: u8) -> u8 {
    c & 0xff
}

/// `DB_FORMAT_Z`: hexadecimal, whatever the radix.
pub const DB_FORMAT_Z: i32 = 1;
/// `DB_FORMAT_R`: in the current radix, signed.
pub const DB_FORMAT_R: i32 = 2;
/// `DB_FORMAT_N`: in the current radix, unsigned.
pub const DB_FORMAT_N: i32 = 3;
/// Should be plenty for all formats.
pub const DB_FORMAT_BUF_SIZE: usize = 64;

/// `db_output_position`: output column.
pub static DB_OUTPUT_POSITION: AtomicI32 = AtomicI32::new(0);
/// `db_output_line`: output line number.
pub static DB_OUTPUT_LINE: AtomicI32 = AtomicI32::new(0);
/// `db_last_non_space`: last non-space character.
pub static DB_LAST_NON_SPACE: AtomicI32 = AtomicI32::new(0);
/// `db_tab_stop_width`: how wide are tab stops?
pub static DB_TAB_STOP_WIDTH: AtomicI32 = AtomicI32::new(8);
/// `db_max_line`: output max lines.
pub static DB_MAX_LINE_VAR: AtomicI32 = AtomicI32::new(DB_MAX_LINE);
/// `db_max_width`: output line width.
pub static DB_MAX_WIDTH_VAR: AtomicI32 = AtomicI32::new(DB_MAX_WIDTH);
/// `db_radix`: output numbers radix.
pub static DB_RADIX: AtomicI32 = AtomicI32::new(16);
/// Set by `db_more`'s `q`: the output of the current command is dropped (C's `db_error(0)`
/// from `db_more`). `db_command_loop` clears it before each prompt.
pub static DB_QUIT: AtomicBool = AtomicBool::new(false);

/// `NEXT_TAB(i)`: the first tab stop after column `i`.
fn next_tab(i: i32) -> i32 {
    let w = DB_TAB_STOP_WIDTH.load(Ordering::Relaxed);
    ((i + w) / w) * w
}

/// `db_force_whitespace`: force pending whitespace.
pub fn db_force_whitespace() {
    let position = DB_OUTPUT_POSITION.load(Ordering::Relaxed);
    let mut last_print = DB_LAST_NON_SPACE.load(Ordering::Relaxed);
    while last_print < position {
        let next_tab = next_tab(last_print);
        if next_tab <= position {
            while last_print < next_tab {
                // DON'T send a tab!!!
                cnputc(i32::from(b' '));
                last_print += 1;
            }
        } else {
            cnputc(i32::from(b' '));
            last_print += 1;
        }
    }
    DB_LAST_NON_SPACE.store(position, Ordering::Relaxed);
}

/// `db_more`: pauses the output at the end of a page and waits for a key.
fn db_more() {
    for &p in b"--db_more--" {
        cnputc(i32::from(p));
    }
    let quit_output = match cngetc() as u8 {
        b' ' => {
            DB_OUTPUT_LINE.store(0, Ordering::Relaxed);
            false
        }
        c if c == b'q' || c == ctrl(b'c') => {
            DB_OUTPUT_LINE.store(0, Ordering::Relaxed);
            true
        }
        _ => {
            DB_OUTPUT_LINE.fetch_sub(1, Ordering::Relaxed);
            false
        }
    };
    for &p in b"\x08\x08\x08\x08\x08\x08\x08\x08\x08\x08\x08           \x08\x08\x08\x08\x08\x08\x08\x08\x08\x08\x08"
    {
        cnputc(i32::from(p));
    }
    if quit_output {
        let _ = db_error(None);
        DB_QUIT.store(true, Ordering::Relaxed);
    }
}

/// `db_putchar`: prints `c`, keeping track of the column and the line, paginating every
/// `db_max_line` lines and wrapping at `db_max_width`.
pub fn db_putchar(c: i32) {
    if DB_QUIT.load(Ordering::Relaxed) {
        return;
    }
    let max_line = DB_MAX_LINE_VAR.load(Ordering::Relaxed);
    if max_line >= DB_MIN_MAX_LINE && DB_OUTPUT_LINE.load(Ordering::Relaxed) >= max_line - 1 {
        db_more();
        if DB_QUIT.load(Ordering::Relaxed) {
            return;
        }
    }

    if c > i32::from(b' ') && c <= i32::from(b'~') {
        // Printing character. If we have spaces to print, print them first. Use tabs if
        // possible.
        db_force_whitespace();
        cnputc(c);
        let position = DB_OUTPUT_POSITION.fetch_add(1, Ordering::Relaxed) + 1;
        let max_width = DB_MAX_WIDTH_VAR.load(Ordering::Relaxed);
        if max_width >= DB_MIN_MAX_WIDTH && position >= max_width - 1 {
            // auto new line
            cnputc(i32::from(b'\n'));
            DB_OUTPUT_POSITION.store(0, Ordering::Relaxed);
            DB_LAST_NON_SPACE.store(0, Ordering::Relaxed);
            DB_OUTPUT_LINE.fetch_add(1, Ordering::Relaxed);
        }
        DB_LAST_NON_SPACE.store(
            DB_OUTPUT_POSITION.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
    } else if c == i32::from(b'\n') {
        // Return
        cnputc(c);
        DB_OUTPUT_POSITION.store(0, Ordering::Relaxed);
        DB_LAST_NON_SPACE.store(0, Ordering::Relaxed);
        DB_OUTPUT_LINE.fetch_add(1, Ordering::Relaxed);
    } else if c == i32::from(b'\t') {
        // assume tabs every 8 positions
        let position = DB_OUTPUT_POSITION.load(Ordering::Relaxed);
        DB_OUTPUT_POSITION.store(next_tab(position), Ordering::Relaxed);
    } else if c == i32::from(b' ') {
        // space
        DB_OUTPUT_POSITION.fetch_add(1, Ordering::Relaxed);
    } else if c == 0o7 {
        // bell
        cnputc(c);
    }
    // other characters are assumed non-printing
}

/// `db_print_position`: the current output column.
pub fn db_print_position() -> i32 {
    DB_OUTPUT_POSITION.load(Ordering::Relaxed)
}

/// `db_end_line`: ends the line unless `space` more columns fit.
pub fn db_end_line(space: i32) {
    if DB_OUTPUT_POSITION.load(Ordering::Relaxed)
        >= DB_MAX_WIDTH_VAR.load(Ordering::Relaxed) - space
    {
        crate::db_printf!("\n");
    }
}

/// `db_format`: a replacement for the non-standard `%z`, `%n` and `%r` printf formats in
/// `db_printf`. `val` is the value we want printed, `format` one of `DB_FORMAT_[ZRN]`, `alt`
/// whether to provide an "alternate" format (`#` in the printf format), `width` the field
/// width (0 is the same as no width specifier). Returns the formatted bytes, a prefix of `buf`.
pub fn db_format(buf: &mut [u8], val: i64, format: i32, alt: bool, width: usize) -> &[u8] {
    let radix = DB_RADIX.load(Ordering::Relaxed);
    // The leading '-' is a nasty (and beautiful) idea from NetBSD
    let (sign, val) = if val < 0 && format != DB_FORMAT_N {
        ("-", val.wrapping_neg() as u64)
    } else {
        ("", val as u64)
    };
    if format == DB_FORMAT_Z || radix == 16 {
        if alt {
            snprintf(buf, format_args!("{sign}{val:#width$x}"));
        } else {
            snprintf(buf, format_args!("{sign}{val:width$x}"));
        }
    } else if radix == 8 {
        if alt {
            snprintf(buf, format_args!("{sign}{val:#width$o}"));
        } else {
            snprintf(buf, format_args!("{sign}{val:width$o}"));
        }
    } else {
        snprintf(buf, format_args!("{sign}{val:width$}"));
    }
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    &buf[..end]
}

/// `db_stack_dump`: prints a stack trace of the current CPU through `printf`, guarding against
/// a fault while tracing.
pub fn db_stack_dump() {
    static INTRACE: AtomicBool = AtomicBool::new(false);

    fn pr(args: core::fmt::Arguments<'_>) {
        printf(args);
    }

    if INTRACE.swap(true, Ordering::AcqRel) {
        printf(format_args!("Faulted in traceback, aborting...\n"));
        return;
    }

    printf(format_args!("Starting stack trace...\n"));
    db_stack_trace_print(
        Machine::frame_address(),
        true,
        256, /* low limit */
        b"",
        pr,
    );
    printf(format_args!("End of stack trace.\n"));
    INTRACE.store(false, Ordering::Release);
}

/// `db_resize`: sets the page size.
pub fn db_resize(cols: i32, rows: i32) {
    DB_MAX_WIDTH_VAR.store(cols, Ordering::Relaxed);
    DB_MAX_LINE_VAR.store(rows, Ordering::Relaxed);
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabs_and_columns() {
        let _g = crate::ddb::db_lex::db_test_lock();
        DB_TAB_STOP_WIDTH.store(8, Ordering::Relaxed);
        assert_eq!(next_tab(0), 8);
        assert_eq!(next_tab(7), 8);
        assert_eq!(next_tab(8), 16);
        DB_OUTPUT_POSITION.store(0, Ordering::Relaxed);
        DB_LAST_NON_SPACE.store(0, Ordering::Relaxed);
        DB_OUTPUT_LINE.store(0, Ordering::Relaxed);
        db_putchar(i32::from(b'\t'));
        assert_eq!(db_print_position(), 8);
        db_putchar(i32::from(b' '));
        assert_eq!(db_print_position(), 9);
        db_putchar(i32::from(b'x'));
        assert_eq!(db_print_position(), 10);
        db_putchar(i32::from(b'\n'));
        assert_eq!(db_print_position(), 0);
        assert_eq!(DB_OUTPUT_LINE.load(Ordering::Relaxed), 1);
        db_end_line(0);
        db_resize(80, 24);
        assert_eq!(ctrl(b'c'), b'c');
    }

    #[test]
    fn format_variants() {
        let _g = crate::ddb::db_lex::db_test_lock();
        let mut buf = [0u8; DB_FORMAT_BUF_SIZE];
        DB_RADIX.store(16, Ordering::Relaxed);
        assert_eq!(db_format(&mut buf, 255, DB_FORMAT_R, false, 0), b"ff");
        assert_eq!(db_format(&mut buf, -255, DB_FORMAT_R, true, 0), b"-0xff");
        assert_eq!(
            db_format(&mut buf, -1, DB_FORMAT_N, false, 0),
            b"ffffffffffffffff"
        );
        assert_eq!(db_format(&mut buf, 5, DB_FORMAT_Z, false, 4), b"   5");
        DB_RADIX.store(8, Ordering::Relaxed);
        assert_eq!(db_format(&mut buf, 8, DB_FORMAT_R, false, 0), b"10");
        assert_eq!(db_format(&mut buf, 8, DB_FORMAT_Z, false, 0), b"8");
        DB_RADIX.store(10, Ordering::Relaxed);
        assert_eq!(db_format(&mut buf, -12, DB_FORMAT_R, false, 0), b"-12");
        DB_RADIX.store(16, Ordering::Relaxed);
    }
}
/* </TESTS> */
