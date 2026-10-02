/*	$OpenBSD: subr_prf.c,v 1.107 2026/09/16 19:53:45 jan Exp $	*/
/*	$NetBSD: subr_prf.c,v 1.45 1997/10/24 18:14:25 chuck Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1986, 1988, 1991, 1993
 *	The Regents of the University of California.  All rights reserved.
 * (c) UNIX System Laboratories, Inc.
 * All or some portions of this file are derived from material licensed
 * to the University of California by American Telephone and Telegraph
 * Co. or Unix System Laboratories, Inc. and are reproduced herein with
 * the permission of UNIX System Laboratories, Inc.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the University nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE REGENTS AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *	@(#)subr_prf.c	8.3 (Berkeley) 1/21/94
 */
/* </LICENSES> */

//! `printf(9)`, `panic(9)`, `log(9)` and the kernel's formatted output engine:
//! `kern/subr_prf.c`.
//!
//! Upstream: sys/kern/subr_prf.c @ 3ce1f3f79392
//!
//! Every message the kernel prints comes through [`kprintf`], which routes the characters by
//! flag: to the console (`TOCONS`, through `cnputc`), to the message buffer (`TOLOG`), to the
//! debugger's paginated output (`TODDB`) or into a caller's buffer (`TOBUFONLY`, with `TOCOUNT`
//! for the `snprintf` return value). The [`kprintf!`], [`kprintln!`], [`log!`] and
//! [`db_printf!`] macros are what callers write; `panic!` anywhere in the kernel lands in
//! [`panic`] through the crate's panic handler.
//!
//! Status: `wip`.
//!
//! ## Deviations
//! - The format engine is `core::fmt`: a `fmt::Arguments` replaces the `(fmt, va_list)` pair,
//!   so `printf`/`vprintf` and `snprintf`/`vsnprintf` are the same function twice. OpenBSD's
//!   `%b` is the [`Bitmask`] `Display` adaptor; its `%s` of a NUL-terminated byte string is
//!   [`Str`].
//! - `kprintf_mutex` and the `splhigh` in `log`/`addlog` arrive with M4/M5.
//! - `v_putc` is fixed to `cnputc`; nothing redirects the console yet.
//! - `kputchar` has no `tp` argument and `constty` is always NULL: `TOTTY` output has no tty to
//!   go to until M7, so `uprintf`, `ttyprintf`, `tprintf_open`, `tprintf_close` and `tprintf`
//!   wait with it.
//! - `panicstr` is behind [`panicstr`] (a flag); the first message is kept in a single
//!   `panicbuf`, per CPU from M5 (`ci_panicbuf`).
//! - `db_panic` defaults to 0: ddb-lite has no debugger to enter, so a panic prints the stack
//!   trace, as OpenBSD does with `ddb.panic=0`.
//! - `KASSERT`/`KDASSERT` (`libkern.h`) live here as [`kassert!`]/[`kdassert!`], next to the
//!   `__assert` they call; `libkern` is a leaf crate that cannot reach it.

use core::fmt::{self, Write};
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, Ordering};

use libkern::StaticCell;

use crate::ddb::db_output::{db_putchar, db_stack_dump};
use crate::ddb::db_usrreq::DB_LOG;
use crate::dev::cons::cnputc;
use crate::kern::init_main::DB_ACTIVE;
use crate::kern::kern_xxx::reboot;
use crate::kern::subr_log::{LOG_OPEN, logwakeup, msgbuf_putchar, msgbufmapped, msgbufp};
use crate::machine::db_machdep::db_enter;
use crate::sys::reboot::{RB_AUTOBOOT, RB_DUMP, RB_NOSYNC};
use crate::sys::syslog::LOG_ERR;

// flags for kprintf

/// To the console.
pub const TOCONS: i32 = 0x01;
/// To the process' tty.
pub const TOTTY: i32 = 0x02;
/// To the kernel message buffer.
pub const TOLOG: i32 = 0x04;
/// To the buffer (only) \[for snprintf\].
pub const TOBUFONLY: i32 = 0x08;
/// To ddb console.
pub const TODDB: i32 = 0x10;
/// Act like \[v\]snprintf.
pub const TOCOUNT: i32 = 0x20;

/// Max size buffer kprintf needs to print quad_t \[size in base 8 + \0\].
pub const KPRINTF_BUFSIZE: usize = (u64::BITS as usize) / 3 + 2;

/// `__KASSERTSTR`, as a Rust format string: the assertion kind, the expression, the file and
/// the line.
pub const KASSERTSTR: &str = "kernel {}assertion \"{}\" failed: file \"{}\", line {}";

/// `panicstr`: arg to first call to panic (used as a flag to indicate that panic has already
/// been called).
static PANICSTR: AtomicPtr<u8> = AtomicPtr::new(ptr::null_mut());
/// `ci_panicbuf`: the first panic message, NUL-terminated.
static PANICBUF: StaticCell<[u8; 512]> = StaticCell::new([0; 512]);
/// `db_panic`: enter ddb on panic.
pub static DB_PANIC: AtomicBool = AtomicBool::new(false);
/// `db_console`: whether a special key combination (machine dependent) enters ddb.
pub static DB_CONSOLE: AtomicBool = AtomicBool::new(false);
/// `splassert_ctl`: what an spl assertion failure does: 1 prints, 2 adds a stack trace, 3 enters
/// ddb, anything else panics; 0 stays quiet.
pub static SPLASSERT_CTL: AtomicI32 = AtomicI32::new(1);
/// `printf_flags`: where `printf` sends its output.
pub static PRINTF_FLAGS: AtomicI32 = AtomicI32::new(TOCONS | TOLOG);

/// `%s` of a NUL-terminated byte string: prints the bytes before the first NUL (or the whole
/// slice), non-ASCII bytes as `?`.
pub struct Str<'a>(pub &'a [u8]);

impl fmt::Display for Str<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for &b in self.0.iter().take_while(|&&b| b != 0) {
            f.write_char(if b.is_ascii() { char::from(b) } else { '?' })?;
        }
        Ok(())
    }
}

/// `%b`: a value followed by the set bits' names, from a descriptor string of the form
/// `"\x10\x01FLAG1\x02FLAG2..."`: the first byte is the base (8, 10 or 16) the value is printed
/// in, then each bit's number (either a byte below `' '`, counted from 1, or a byte with the
/// high bit set) followed by its name. `Bitmask(0x5, b"\x10\x01READ\x02WRITE\x03EXEC")` prints
/// `5<READ,EXEC>`.
pub struct Bitmask<'a>(pub u64, pub &'a [u8]);

impl fmt::Display for Bitmask<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Some((&base, mut b)) = self.1.split_first() else {
            return Ok(());
        };
        let v = self.0;
        match base {
            8 => write!(f, "{v:o}")?,
            10 => write!(f, "{v}")?,
            16 => write!(f, "{v:x}")?,
            _ => return Ok(()),
        }
        if v == 0 {
            return Ok(());
        }
        let mut any = false;
        while let Some((&bit, rest)) = b.split_first() {
            b = rest;
            if bit == 0 {
                break;
            }
            let n = if bit & 0x80 != 0 {
                bit & 0x7f
            } else if bit <= b' ' {
                bit - 1
            } else {
                bit
            };
            let name_len = b.iter().take_while(|&&c| c > b' ' && c & 0x80 == 0).count();
            let (name, rest) = b.split_at(name_len);
            b = rest;
            if n < 64 && v & (1u64 << n) != 0 {
                f.write_char(if any { ',' } else { '<' })?;
                for &c in name {
                    f.write_char(char::from(c))?;
                }
                any = true;
            }
        }
        if any {
            f.write_char('>')?;
        }
        Ok(())
    }
}

/// Where [`kprintf`] sends each character: `KPRINTF_PUTCHAR` in C.
struct Sink<'a> {
    oflags: i32,
    buf: Option<&'a mut [u8]>,
    pos: usize,
    ret: usize,
}

impl Sink<'_> {
    /// One output character. The error is the `TOBUFONLY` overflow stop (no `TOCOUNT`), the
    /// only way the C engine ends early.
    fn putchar(&mut self, c: u8) -> fmt::Result {
        self.ret += 1;
        if self.oflags & TOBUFONLY != 0 {
            if let Some(buf) = self.buf.as_deref_mut() {
                // The last byte is the terminator's (`tailp`): a character that would land on
                // it is dropped, and only counted with TOCOUNT.
                if self.pos + 1 >= buf.len() {
                    if self.oflags & TOCOUNT == 0 {
                        return Err(fmt::Error);
                    }
                } else {
                    buf[self.pos] = c;
                    self.pos += 1;
                }
            }
        } else {
            kputchar(i32::from(c), self.oflags);
        }
        Ok(())
    }
}

impl Write for Sink<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for b in s.bytes() {
            self.putchar(b)?;
        }
        Ok(())
    }
}

/// `__assert`: partial support (the failure case) of the assertion facility commonly found in
/// userland.
pub fn __assert(t: &str, f: &str, l: u32, e: &str) -> ! {
    panic(format_args!(
        "kernel {}assertion \"{}\" failed: file \"{}\", line {}",
        t, e, f, l
    ))
}

/// `tablefull`: warn that a system table is full.
pub fn tablefull(tab: &str) {
    log(LOG_ERR, format_args!("{tab}: table is full\n"));
}

/// `panicstr`: whether `panic` has been called.
pub fn panicstr() -> bool {
    !PANICSTR.load(Ordering::Acquire).is_null()
}

/// `atomic_cas_ptr(&panicstr, NULL, buf)`: the trap handlers' `fault()` claims `panicstr`
/// for their CPU's `ci_panicbuf` before the message is formatted, so the `panic()` that
/// follows counts as the second one (`RB_NOSYNC`).
pub fn panicstr_claim(buf: *mut u8) {
    let _ = PANICSTR.compare_exchange(ptr::null_mut(), buf, Ordering::AcqRel, Ordering::Acquire);
}

/// `panic`: handle an unresolvable fatal error. Prints "panic: \<message\>" and reboots. If
/// called twice (i.e. a recursive call) we avoid trying to sync the disk and just reboot (to
/// avoid recursive panics).
pub fn panic(args: fmt::Arguments<'_>) -> ! {
    let mut bootopt = RB_AUTOBOOT | RB_DUMP;
    let panicbuf = PANICBUF.as_ptr().cast::<u8>();
    if PANICSTR
        .compare_exchange(
            ptr::null_mut(),
            panicbuf,
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .is_err()
    {
        bootopt |= RB_NOSYNC;
    }

    // do not trigger assertions, we know that we are inconsistent
    SPLASSERT_CTL.store(0, Ordering::Relaxed);

    // All panic messages are printed, but only the first panic on a given CPU is written to its
    // panicbuf. The message is formatted on the stack and copied, so no reference to the
    // buffer is live while printing, which may panic again.
    // SAFETY: a volatile read of the first byte; the buffer is only ever written below, from
    // the one CPU that owns it.
    let first = unsafe { ptr::read_volatile(panicbuf) } == 0;
    if first {
        let mut msg = [0u8; 512];
        vsnprintf(&mut msg, args);
        // SAFETY: this CPU's buffer, written once (its first byte was 0), with no reader yet.
        unsafe { ptr::copy_nonoverlapping(msg.as_ptr(), panicbuf, msg.len()) };
        db_printf(format_args!("panic: {}\n", Str(&msg)));
    } else {
        db_printf(format_args!("panic: "));
        db_vprintf(args);
        db_printf(format_args!("\n"));
    }

    if DB_PANIC.load(Ordering::Relaxed) {
        db_enter();
    } else {
        db_stack_dump();
    }
    reboot(bootopt)
}

/// `splassert_fail`: reports an spl assertion failure. We print only the function name. The
/// file name is usually very long and would eat tons of space in the kernel.
pub fn splassert_fail(wantipl: i32, haveipl: i32, func: &str) {
    if panicstr() || DB_ACTIVE.load(Ordering::Relaxed) {
        return;
    }

    printf(format_args!(
        "splassert: {func}: want {wantipl} have {haveipl}\n"
    ));
    match SPLASSERT_CTL.load(Ordering::Relaxed) {
        1 => {}
        2 => db_stack_dump(),
        3 => {
            db_stack_dump();
            db_enter();
        }
        _ => panic(format_args!("spl assertion failure in {func}")),
    }
}

/// `log`: write to the log buffer. Will not sleep (so safe to call from interrupt); will log to
/// console if `/dev/klog` isn't open.
pub fn log(level: i32, args: fmt::Arguments<'_>) {
    // s = splhigh(): M4.
    logpri(level); // log the level first
    kprintf(args, TOLOG, None);
    if !LOG_OPEN.load(Ordering::Relaxed) {
        // mtx_enter(&kprintf_mutex): M5.
        kprintf(args, TOCONS, None);
    }
    logwakeup(); // wake up anyone waiting for log msgs
}

/// `logpri`: log the priority level to the klog.
pub fn logpri(level: i32) {
    let mut snbuf = [0u8; KPRINTF_BUFSIZE];

    kputchar(i32::from(b'<'), TOLOG);
    snprintf(&mut snbuf, format_args!("{level}"));
    for &p in snbuf.iter().take_while(|&&p| p != 0) {
        kputchar(i32::from(p), TOLOG);
    }
    kputchar(i32::from(b'>'), TOLOG);
}

/// `addlog`: add info to previous log message.
pub fn addlog(args: fmt::Arguments<'_>) {
    // s = splhigh(): M4.
    kprintf(args, TOLOG, None);
    if !LOG_OPEN.load(Ordering::Relaxed) {
        // mtx_enter(&kprintf_mutex): M5.
        kprintf(args, TOCONS, None);
    }
    logwakeup();
}

/// `kputchar`: print a single character on console or user terminal. Note that the `tp`
/// argument of the C (the tty) is not here yet (M7).
pub fn kputchar(c: i32, flags: i32) {
    // if (panicstr) constty = NULL; TOTTY -> tputchar(c, tp): no tty until M7.
    if flags & TOLOG != 0
        && c != 0
        && c != i32::from(b'\r')
        && c != 0o177
        && msgbufmapped()
        && let Some(mbp) = msgbufp()
    {
        msgbuf_putchar(mbp, c as u8);
    }
    if flags & TOCONS != 0 && c != 0 {
        // (constty == NULL || db_active): there is no constty yet.
        cnputc(c);
    }
    if flags & TODDB != 0 {
        db_putchar(c);
    }
}

/// `db_printf` / `db_vprintf`: `ddb(4)`'s `printf`, paginated through `db_putchar` and logged
/// when `db_log` is set.
pub fn db_printf(args: fmt::Arguments<'_>) -> usize {
    db_vprintf(args)
}

/// `db_vprintf`: the `va_list` form of [`db_printf`].
pub fn db_vprintf(args: fmt::Arguments<'_>) -> usize {
    let mut flags = TODDB;
    if DB_LOG.load(Ordering::Relaxed) {
        flags |= TOLOG;
    }
    kprintf(args, flags, None)
}

/// `printf(9)`: the normal kernel printf, to the console and the message buffer. Returns the
/// number of characters produced.
pub fn printf(args: fmt::Arguments<'_>) -> usize {
    // mtx_enter(&kprintf_mutex): M5.
    let retval = kprintf(args, PRINTF_FLAGS.load(Ordering::Relaxed), None);
    if !panicstr() {
        logwakeup();
    }
    retval
}

/// `vprintf`: the `va_list` form of [`printf`]; always to the console and the log.
pub fn vprintf(args: fmt::Arguments<'_>) -> usize {
    // mtx_enter(&kprintf_mutex): M5.
    let retval = kprintf(args, TOCONS | TOLOG, None);
    if !panicstr() {
        logwakeup();
    }
    retval
}

/// `snprintf`: formats into `buf`, NUL-terminated, and returns the length the whole message
/// would have had (the C contract: a result of `buf.len()` or more means truncation).
pub fn snprintf(buf: &mut [u8], args: fmt::Arguments<'_>) -> usize {
    vsnprintf(buf, args)
}

/// `vsnprintf`: the `va_list` form of [`snprintf`].
pub fn vsnprintf(buf: &mut [u8], args: fmt::Arguments<'_>) -> usize {
    let retval = kprintf(args, TOBUFONLY | TOCOUNT, Some(&mut *buf));
    if !buf.is_empty() {
        let end = retval.min(buf.len() - 1);
        buf[end] = 0; // null terminate
    }
    retval
}

/// `kprintf`: the engine behind every printf-like function. Formats `args` and routes each
/// character according to `oflags`; with `TOBUFONLY`, into `sbuf` (never its last byte, which
/// is left for the terminator). Returns the number of characters produced, counting the ones
/// `TOCOUNT` dropped.
pub fn kprintf(args: fmt::Arguments<'_>, oflags: i32, sbuf: Option<&mut [u8]>) -> usize {
    let mut sink = Sink {
        oflags,
        buf: sbuf,
        pos: 0,
        ret: 0,
    };
    // The only error is the TOBUFONLY overflow stop, which ends the output as the C's
    // `goto overflow` does.
    let _ = sink.write_fmt(args);
    sink.ret
}

/// `puts`: prints `s` and a newline.
pub fn puts(s: &[u8]) {
    printf(format_args!("{}\n", Str(s)));
}

/// `putchar`: prints the byte `c` and returns it.
pub fn putchar(c: i32) -> i32 {
    printf(format_args!("{}", Str(&[c as u8])));
    c
}

/// `printf(9)`: `kprintf!("fmt", args...)` prints through [`printf`].
#[macro_export]
macro_rules! kprintf {
    ($($arg:tt)*) => {
        $crate::kern::subr_prf::printf(::core::format_args!($($arg)*))
    };
}

/// `printf(9)` with a trailing newline.
#[macro_export]
macro_rules! kprintln {
    () => {
        $crate::kern::subr_prf::printf(::core::format_args!("\n"))
    };
    ($($arg:tt)*) => {
        $crate::kern::subr_prf::printf(::core::format_args!(
            "{}\n",
            ::core::format_args!($($arg)*)
        ))
    };
}

/// `log(9)`: `log!(LOG_ERR, "fmt", args...)`.
#[macro_export]
macro_rules! log {
    ($level:expr, $($arg:tt)*) => {
        $crate::kern::subr_prf::log($level, ::core::format_args!($($arg)*))
    };
}

/// `db_printf`: `ddb(4)` output.
#[macro_export]
macro_rules! db_printf {
    ($($arg:tt)*) => {
        $crate::kern::subr_prf::db_printf(::core::format_args!($($arg)*))
    };
}

/// `KASSERT(e)`: with feature `diagnostic`, panics through `__assert` when `e` is false;
/// otherwise nothing, not even the evaluation of `e` (the expression still type-checks, inside
/// a closure that is never called, so the names it uses do not become unused).
#[macro_export]
macro_rules! kassert {
    ($e:expr) => {
        #[cfg(feature = "diagnostic")]
        {
            if !$e {
                $crate::kern::subr_prf::__assert(
                    "diagnostic ",
                    ::core::file!(),
                    ::core::line!(),
                    ::core::stringify!($e),
                );
            }
        }
        #[cfg(not(feature = "diagnostic"))]
        {
            let _ = || -> bool { $e };
        }
    };
}

/// `KDASSERT(e)`: as [`kassert!`], behind feature `debug`.
#[macro_export]
macro_rules! kdassert {
    ($e:expr) => {
        #[cfg(feature = "debug")]
        {
            if !$e {
                $crate::kern::subr_prf::__assert(
                    "debugging ",
                    ::core::file!(),
                    ::core::line!(),
                    ::core::stringify!($e),
                );
            }
        }
        #[cfg(not(feature = "debug"))]
        {
            let _ = || -> bool { $e };
        }
    };
}

#[cfg(test)]
mod tests;
