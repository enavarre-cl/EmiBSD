/*	$OpenBSD: db_input.c,v 1.20 2026/04/23 01:15:07 dlg Exp $	*/
/*	$NetBSD: db_input.c,v 1.7 1996/02/05 01:57:02 christos Exp $	*/
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
 *	Author: David B. Golub, Carnegie Mellon University
 *	Date:	7/90
 */
/* </LICENSES> */

//! Character input and editing for the debugger: `ddb/db_input.c`.
//!
//! Upstream: sys/ddb/db_input.c @ 3ce1f3f79392
//!
//! `db_readline` reads a line from the console with `cngetc`, echoing with `cnputc`, and
//! edits it emacs style: `^B`/`^F` move back and forward, `^A`/`^E` to the start and the end,
//! `^H` and DEL erase backwards, `^D` forwards, `^W` a word, `^K` to the end, `^U` the line,
//! `^T` swaps the last two characters, `^R` redraws, and `^P`/`^N` walk the history of the
//! last `DB_HISTORY_SIZE` bytes of lines. We don't track output position while editing input,
//! since input always ends with a new-line; the position is reset at the end.
//!
//! ## Deviations
//! - The line buffer is a slice and `db_lbuf_start`, `db_lbuf_end`, `db_lc`, `db_le` are
//!   indices into it ([`DbLine`]); the history ring and its three cursors are one
//!   [`StaticCell`] (`DB_HISTORY`), `db_history_prev` an `Option` for the C's NULL.
//! - `db_readline` NUL-terminates the line only when the NUL fits: a full line plus its
//!   newline fill the buffer, and C writes the NUL one byte past it. `^P`/`^N` stop copying
//!   a history line at the end of the buffer (they never reach it: history lines come from
//!   buffers of the same size).
//! - `db_check_interrupt`, which older ddb had here, is not in the pinned file.

use libkern::StaticCell;

use crate::ddb::db_output::{db_force_whitespace, db_putchar};
use crate::ddb::db_var::DB_HISTORY_SIZE;
use crate::dev::cons::{cngetc, cnputc};

/// `CTRL(c)`: the control character of `c`.
const fn ctrl(c: u8) -> i32 {
    (c & 0x1f) as i32
}

/// `BLANK`.
const BLANK: i32 = b' ' as i32;
/// `BACKUP`.
const BACKUP: i32 = 0x08;

/// `DEL_FWD`: delete forwards.
const DEL_FWD: bool = false;
/// `DEL_BWD`: delete backwards.
const DEL_BWD: bool = true;

/// The line being edited: `db_lbuf_start` (index 0), `db_lbuf_end`, `db_lc` and `db_le`.
struct DbLine<'a> {
    /// The input line buffer.
    buf: &'a mut [u8],
    /// `db_lbuf_end`: end of input line buffer (one byte is kept for the newline).
    end: usize,
    /// `db_lc`: current character.
    lc: usize,
    /// `db_le`: one past last character.
    le: usize,
}

/// The history ring: `db_history`, `db_history_curr`, `db_history_last`, `db_history_prev`.
struct DbHistory {
    /// `db_history`: lines, each ended by a NUL, wrapping around.
    buf: [u8; DB_HISTORY_SIZE],
    /// `db_history_curr`: start of current line.
    curr: usize,
    /// `db_history_last`: start of last line.
    last: usize,
    /// `db_history_prev`: start of previous line, `None` before the first.
    prev: Option<usize>,
}

impl DbHistory {
    /// `INC_DB_CURR()`.
    fn inc_curr(&mut self) {
        self.curr += 1;
        if self.curr > DB_HISTORY_SIZE - 1 {
            self.curr = 0;
        }
    }

    /// `DEC_DB_CURR()`.
    fn dec_curr(&mut self) {
        if self.curr == 0 {
            self.curr = DB_HISTORY_SIZE - 1;
        } else {
            self.curr -= 1;
        }
    }
}

/// The history. Only the CPU in the debugger edits a line (`db_active`), and `db_readline`
/// is not re-entered while it runs.
static DB_HISTORY: StaticCell<DbHistory> = StaticCell::new(DbHistory {
    buf: [0; DB_HISTORY_SIZE],
    curr: 0,
    last: 0,
    prev: None,
});

/// `db_putstring`: echoes `s`.
fn db_putstring(s: &[u8]) {
    for &c in s {
        cnputc(i32::from(c));
    }
}

/// `db_putnchars`: echoes `c` `count` times.
fn db_putnchars(c: i32, count: usize) {
    for _ in 0..count {
        cnputc(c);
    }
}

impl DbLine<'_> {
    /// `db_delete`: delete N characters, forward or backward.
    fn db_delete(&mut self, n: usize, bwd: bool) {
        if bwd {
            self.lc -= n;
            db_putnchars(BACKUP, n);
        }
        for p in self.lc..self.le - n {
            self.buf[p] = self.buf[p + n];
            cnputc(i32::from(self.buf[p]));
        }
        db_putnchars(BLANK, n);
        db_putnchars(BACKUP, self.le - self.lc);
        self.le -= n;
    }

    /// `db_delete_line`: erases the whole line.
    fn db_delete_line(&mut self) {
        self.db_delete(self.le - self.lc, DEL_FWD);
        self.db_delete(self.lc, DEL_BWD);
        self.le = 0;
        self.lc = 0;
    }

    /// Copies the history line at `h.curr` into the buffer (the body of `^P` and `^N`).
    fn copy_history(&mut self, h: &DbHistory) {
        let mut p = h.curr;
        self.le = 0;
        while h.buf[p] != 0 && self.le < self.buf.len() {
            self.buf[self.le] = h.buf[p];
            self.le += 1;
            p += 1;
            if p == DB_HISTORY_SIZE {
                p = 0;
            }
        }
        self.lc = self.le;
    }

    /// `db_inputchar`: handles one input character. Returns `true` at end-of-line.
    fn db_inputchar(&mut self, h: &mut DbHistory, c: i32) -> bool {
        match c {
            c if c == ctrl(b'b') => {
                // back up one character
                if self.lc > 0 {
                    cnputc(BACKUP);
                    self.lc -= 1;
                }
            }
            c if c == ctrl(b'f') => {
                // forward one character
                if self.lc < self.le {
                    cnputc(i32::from(self.buf[self.lc]));
                    self.lc += 1;
                }
            }
            c if c == ctrl(b'a') => {
                // beginning of line
                while self.lc > 0 {
                    cnputc(BACKUP);
                    self.lc -= 1;
                }
            }
            c if c == ctrl(b'e') => {
                // end of line
                while self.lc < self.le {
                    cnputc(i32::from(self.buf[self.lc]));
                    self.lc += 1;
                }
            }
            c if c == ctrl(b'w') => {
                // erase trailing whitespace after the word
                while self.lc > 0 && i32::from(self.buf[self.lc - 1]) == BLANK {
                    self.db_delete(1, DEL_BWD);
                }
                // erase word back
                while self.lc > 0 && i32::from(self.buf[self.lc - 1]) != BLANK {
                    self.db_delete(1, DEL_BWD);
                }
            }
            c if c == ctrl(b'h') || c == 0o177 => {
                // erase previous character
                if self.lc > 0 {
                    self.db_delete(1, DEL_BWD);
                }
            }
            c if c == ctrl(b'd') => {
                // erase next character
                if self.lc < self.le {
                    self.db_delete(1, DEL_FWD);
                }
            }
            c if c == ctrl(b'k') => {
                // delete to end of line
                if self.lc < self.le {
                    self.db_delete(self.le - self.lc, DEL_FWD);
                }
            }
            c if c == ctrl(b'u') => {
                // delete line
                self.db_delete_line();
            }
            c if c == ctrl(b't') => {
                // twiddle last 2 characters
                if self.lc >= 2 {
                    self.buf.swap(self.lc - 2, self.lc - 1);
                    cnputc(BACKUP);
                    cnputc(BACKUP);
                    cnputc(i32::from(self.buf[self.lc - 2]));
                    cnputc(i32::from(self.buf[self.lc - 1]));
                }
            }
            c if c == ctrl(b'p') => {
                h.dec_curr();
                while h.curr != h.last {
                    h.dec_curr();
                    if h.buf[h.curr] == 0 {
                        break;
                    }
                }
                self.db_delete_line();
                if h.curr == h.last {
                    h.inc_curr();
                    self.le = 0;
                    self.lc = 0;
                } else {
                    h.inc_curr();
                    self.copy_history(h);
                }
                db_putstring(&self.buf[..self.le]);
            }
            c if c == ctrl(b'n') => {
                while h.curr != h.last {
                    if h.buf[h.curr] == 0 {
                        break;
                    }
                    h.inc_curr();
                }
                if h.curr != h.last {
                    h.inc_curr();
                    self.db_delete_line();
                    if h.curr != h.last {
                        self.copy_history(h);
                    }
                    db_putstring(&self.buf[..self.le]);
                }
            }
            c if c == ctrl(b'r') => {
                db_putstring(b"^R\n");
                if self.le > 0 {
                    db_putstring(&self.buf[..self.le]);
                    db_putnchars(BACKUP, self.le - self.lc);
                }
            }
            c if c == i32::from(b'\n') || c == i32::from(b'\r') => {
                // Check whether current line is the same as previous saved line. If it is,
                // don't save it.
                if Some(h.curr) == h.prev {
                    let mut pp = h.curr;
                    let mut pc = 0;
                    // Is it the same?
                    while pc != self.le && h.buf[pp] != 0 {
                        if h.buf[pp] != self.buf[pc] {
                            break;
                        }
                        pp += 1;
                        if pp == DB_HISTORY_SIZE {
                            pp = 0;
                        }
                        pc += 1;
                    }
                    if h.buf[pp] == 0 && pc == self.le {
                        // Repeated previous line. Don't save.
                        h.curr = h.last;
                        self.buf[self.le] = c as u8;
                        self.le += 1;
                        return true;
                    }
                }
                if self.le != 0 {
                    h.prev = Some(h.last);
                    for p in 0..self.le {
                        h.buf[h.last] = self.buf[p];
                        h.last += 1;
                        if h.last == DB_HISTORY_SIZE {
                            h.last = 0;
                        }
                    }
                    h.buf[h.last] = 0;
                    h.last += 1;
                    if h.last == DB_HISTORY_SIZE {
                        h.last = 0;
                    }
                }
                h.curr = h.last;
                self.buf[self.le] = c as u8;
                self.le += 1;
                return true;
            }
            _ => {
                if self.le == self.end {
                    cnputc(0o7);
                } else if (i32::from(b' ')..=i32::from(b'~')).contains(&c) {
                    self.buf.copy_within(self.lc..self.le, self.lc + 1);
                    self.buf[self.lc] = c as u8;
                    self.lc += 1;
                    self.le += 1;
                    cnputc(c);
                    db_putstring(&self.buf[self.lc..self.le]);
                    db_putnchars(BACKUP, self.le - self.lc);
                }
            }
        }
        false
    }
}

/// `db_readline`: reads and edits a line from the console into `lstart`. Returns its length,
/// the newline included.
pub fn db_readline(lstart: &mut [u8]) -> usize {
    db_readline_from(lstart, cngetc)
}

/// The body of `db_readline`, reading characters from `getc`.
fn db_readline_from(lstart: &mut [u8], mut getc: impl FnMut() -> i32) -> usize {
    db_force_whitespace(); // synch output position

    let end = lstart.len() - 1;
    let mut line = DbLine {
        buf: lstart,
        end,
        lc: 0,
        le: 0,
    };

    loop {
        let c = getc();
        // SAFETY: only the CPU in the debugger reads a line, and nothing below re-enters
        // db_readline, so this is the only reference to the history while it lives.
        let h = unsafe { DB_HISTORY.get_mut() };
        if line.db_inputchar(h, c) {
            break;
        }
    }

    db_putchar(i32::from(b'\n')); // synch output position

    if line.le < line.buf.len() {
        line.buf[line.le] = 0;
    }
    line.le
}

#[cfg(test)]
mod tests;
