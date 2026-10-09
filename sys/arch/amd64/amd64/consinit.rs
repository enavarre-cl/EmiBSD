/* <LICENSES> */
/*
 * Copyright (c) 1998
 *	Matthias Drochner.  All rights reserved.
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
 *
 */
/* </LICENSES> */

/* <CODE> */
//! amd64 `consinit()`: `arch/amd64/amd64/consinit.c`.
//!
//! Upstream: sys/arch/amd64/amd64/consinit.c @ 3ce1f3f79392
//!
//! Status: `wip`. The console's receive interrupt is `com_isa`'s since M8 (`isa0 at
//! mainbus0`, `com0 at isa?`).
//!
//! ## Deviations
//! - In C the function is empty: `init_x86_64` already ran `cninit()`, the `constab[]` probe
//!   loop of `dev/cons.c`, which picks the best of the consoles `conf.c` lists (`pc`, `com`).
//!   `constab` and `cninit` are not ported yet, so this attaches `com(4)` at `CONADDR` directly,
//!   once, with the defaults `comcnprobe`/`comcninit` would use. The machine code that reads the
//!   bootloader's console description (`comconsrate` and friends) arrives with M4.

use core::sync::atomic::{AtomicBool, Ordering};

use crate::arch::amd64::amd64::bus_space::X86_BUS_SPACE_IO;
use crate::dev::ic::com::{COMCONSCFLAG, COMCONSRATE, comcnattach};
use crate::dev::ic::comreg::{COM_FREQ, CONADDR};

/// `consinit`: attaches the console, once.
pub fn consinit() {
    static CALLED: AtomicBool = AtomicBool::new(false);

    if CALLED.swap(true, Ordering::Relaxed) {
        return;
    }
    // SAFETY: CONADDR is COM1 of the PC platform, which QEMU's q35 provides, and nothing
    // else drives it. TODO(M4): take the console from the bootloader/ACPI description.
    let attached = unsafe {
        comcnattach(
            X86_BUS_SPACE_IO,
            CONADDR,
            COMCONSRATE.load(Ordering::Relaxed),
            COM_FREQ,
            COMCONSCFLAG.load(Ordering::Relaxed),
        )
    };
    // A failure leaves the kernel without a console, which is also what the C's silent
    // `cninit` does when no constab entry probes; there is nowhere to report it.
    let _ = attached;
}
/* </CODE> */
