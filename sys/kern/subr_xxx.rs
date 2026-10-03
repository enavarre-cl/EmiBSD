/*	$OpenBSD: subr_xxx.c,v 1.20 2026/04/22 01:51:37 jsg Exp $	*/
/*	$NetBSD: subr_xxx.c,v 1.10 1996/02/04 02:16:51 christos Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1982, 1986, 1991, 1993
 *	The Regents of the University of California.  All rights reserved.
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
 *	@(#)subr_xxx.c	8.1 (Berkeley) 6/10/93
 */
/* </LICENSES> */

//! Miscellaneous trivial functions, including many that are often inline-expanded or done
//! in assembler: `kern/subr_xxx.c`.
//!
//! Upstream: sys/kern/subr_xxx.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M5 (part b2) ports `enodev`, `enxio`, `eopnotsupp`, `nullop`
//! and `assertwaitok`; `bdevsw_lookup`, `chrtoblk` and `blktochr` need the device switch
//! tables (`conf.h`, M7).
//!
//! ## Deviations
//! - The error stubs return `Result<(), Errno>` like every other error path.
//! - `SMR_ASSERT_NONCRITICAL()` in `assertwaitok` waits for `kern_smr.c` (M7).

use core::sync::atomic::Ordering;

use crate::kern::init_main::DB_ACTIVE;
use crate::kern::subr_prf::panicstr;
use crate::machine::Machine;
use crate::machine::cpu::Cpu;
use crate::machine::intr::{IPL_NONE, splassert};
use crate::sys::errno::Errno;

/// `enodev`: unsupported device function (e.g. writing to read-only device).
pub fn enodev() -> Result<(), Errno> {
    Err(Errno::ENODEV)
}

/// `enxio`: unconfigured device function; driver not configured.
pub fn enxio() -> Result<(), Errno> {
    Err(Errno::ENXIO)
}

/// `eopnotsupp`: return error for operation not supported on a specific object or file type.
pub fn eopnotsupp() -> Result<(), Errno> {
    Err(Errno::EOPNOTSUPP)
}

/// `nullop`: generic null operation, always returns success.
pub fn nullop() -> Result<(), Errno> {
    Ok(())
}

// bdevsw_lookup, chrtoblk, blktochr: the device switch tables (conf.h, M7).

/// `assertwaitok`: check that we're in a context where it's okay to sleep.
pub fn assertwaitok() {
    if panicstr() || DB_ACTIVE.load(Ordering::Relaxed) {
        return;
    }

    splassert(IPL_NONE, "assertwaitok");
    // SMR_ASSERT_NONCRITICAL(): kern_smr.c (M7).
    #[cfg(feature = "diagnostic")]
    if Machine::curcpu_mutex_level() != 0 {
        crate::kern::subr_prf::panic(format_args!(
            "assertwaitok: non-zero mutex count: {}",
            Machine::curcpu_mutex_level()
        ));
    }
    #[cfg(not(feature = "diagnostic"))]
    let _ = Machine::curcpu_mutex_level;
}
