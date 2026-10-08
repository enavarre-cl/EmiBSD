/*	$OpenBSD: cons.c,v 1.14 2010/05/09 15:30:28 jsg Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1988 University of Utah.
 * Copyright (c) 1990, 1993
 *	The Regents of the University of California.  All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * the Systems Programming Group of the University of Utah Computer
 * Science Department.
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
 * form: OpenBSD: cons.c,v 1.7 1996/04/21 22:19:48
 * from: OpenBSD: cninit.c,v 1.2 1996/03/30 02:03:45
 * from: Utah $Hdr: cons.c 1.7 92/01/21$
 *
 *	@(#)cons.c	8.2 (Berkeley) 1/12/94
 */
/* </LICENSES> */

/* <CODE> */
//! The standalone console: pick the best console of `constab[]` and do character I/O on it.
//!
//! Upstream: sys/lib/libsa/cons.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `constab[]` is the program's [`SaConf::constab`](crate::stand::SaConf); `cn_tab`, which
//!   the program's `conf.c` defines and points at `constab[0]`, is [`CN_TAB`] here, an index
//!   into it (`usize::MAX` for the C's NULL), starting at 0 as in C.

use core::sync::atomic::{AtomicUsize, Ordering};

use crate::hdr::cons::{CN_DEAD, ConsDev};
use crate::hdr::types::{Dev, major};
use crate::stand::sa_conf;

/// `cn_tab`: the index of the console in `constab[]`, `usize::MAX` for none.
pub static CN_TAB: AtomicUsize = AtomicUsize::new(0);

/// `cn_tab`: the console, if there is one.
pub fn cn_tab() -> Option<&'static ConsDev> {
    sa_conf().constab.get(CN_TAB.load(Ordering::Relaxed))
}

/// `cninit()`: collect information about all possible consoles, choose the one with the
/// highest priority and turn it on.
pub fn cninit() {
    let constab = sa_conf().constab;
    for (i, cp) in constab.iter().enumerate() {
        (cp.cn_probe)(cp);
        if cp.pri() != CN_DEAD && cn_tab().is_none_or(|tab| cp.pri() > tab.pri()) {
            CN_TAB.store(i, Ordering::Relaxed);
        }
    }
    // No console, we can handle it
    let Some(cp) = cn_tab() else {
        return;
    };
    // Turn on console
    (cp.cn_init)(cp);
}

/// `cnset(dev)`: switch to the console `dev`; `false` (the C's 1) if there is none.
pub fn cnset(dev: Dev) -> bool {
    let constab = sa_conf().constab;
    for (i, cp) in constab.iter().enumerate() {
        if major(cp.dev()) == major(dev) {
            // short-circuit noop
            if CN_TAB.load(Ordering::Relaxed) == i && cp.dev() == dev {
                return true;
            }
            if cp.pri() != CN_DEAD {
                CN_TAB.store(i, Ordering::Relaxed);
                cp.set_dev(dev);
                // Turn it on.
                (cp.cn_init)(cp);
                return true;
            }
            break;
        }
    }
    false
}

/// `cngetc()`: the next character typed, 0 without a console.
pub fn cngetc() -> i32 {
    match cn_tab() {
        Some(cp) => (cp.cn_getc)(cp.dev()),
        None => 0,
    }
}

/// `cnputc(c)`: write a character; a newline is followed by a carriage return.
pub fn cnputc(c: i32) {
    if let Some(cp) = cn_tab()
        && c != 0
    {
        (cp.cn_putc)(cp.dev(), c);
        if c == i32::from(b'\n') {
            (cp.cn_putc)(cp.dev(), i32::from(b'\r'));
        }
    }
}

/// `cnischar()`: non-zero if a character is waiting.
pub fn cnischar() -> i32 {
    match cn_tab() {
        Some(cp) => (cp.cn_getc)(cp.dev() | 0x80),
        None => 0,
    }
}
/* </CODE> */
