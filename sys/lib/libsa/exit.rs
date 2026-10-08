/*	$OpenBSD: exit.c,v 1.9 2004/01/03 14:08:53 espie Exp $	*/
/*	$NetBSD: exit.c,v 1.11 1996/12/01 20:22:19 pk Exp $	*/
/* <LICENSES> */
/*-
 *  Copyright (c) 1993 John Brezak
 *  All rights reserved.
 *
 *  Redistribution and use in source and binary forms, with or without
 *  modification, are permitted provided that the following conditions
 *  are met:
 *  1. Redistributions of source code must retain the above copyright
 *     notice, this list of conditions and the following disclaimer.
 *  2. Redistributions in binary form must reproduce the above copyright
 *     notice, this list of conditions and the following disclaimer in the
 *     documentation and/or other materials provided with the distribution.
 *  3. The name of the author may not be used to endorse or promote products
 *     derived from this software without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR `AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
 * WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
 * DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT,
 * INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
 * (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
 * SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT,
 * STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN
 * ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! `panic()` and `exit()`: close what is open, say why, and return to the firmware.
//!
//! Upstream: sys/lib/libsa/exit.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `panic(fmt, ...)` takes `format_args!` (see `printf.rs`); `static int paniced` is an
//!   atomic.

use core::fmt;
use core::sync::atomic::{AtomicBool, Ordering};

use crate::closeall::closeall;
use crate::printf;
use crate::stand::sa_conf;

/// `panic()`'s `static int paniced`: the files are closed once only.
static PANICED: AtomicBool = AtomicBool::new(false);

/// `panic(fmt, ...)`: close every file (the first time), print the message and `_rtt()`.
pub fn panic(args: fmt::Arguments<'_>) -> ! {
    if !PANICED.swap(true, Ordering::Relaxed) {
        closeall();
    }

    printf::vprintf(args);
    crate::printf!("\n");
    (sa_conf().rtt)()
}

/// `exit()`.
pub fn exit() -> ! {
    panic(format_args!("exit"))
}
/* </CODE> */
