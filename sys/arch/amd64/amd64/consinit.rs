/*	$OpenBSD: consinit.c,v 1.7 2017/10/14 04:44:43 jsg Exp $	*/
/*	$NetBSD: consinit.c,v 1.2 2003/03/02 18:27:14 fvdl Exp $	*/

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

//! amd64 `consinit()`: `arch/amd64/amd64/consinit.c`.
//!
//! Upstream: sys/arch/amd64/amd64/consinit.c @ 3ce1f3f79392
//!
//! Status: `wip`. M4 adds `cn_rx_intr_establish`, the console's receive interrupt until
//! `com_isa` attaches the port (M5).
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
use libkern::StaticCell;

use crate::arch::amd64::include::intrdefs::{IPL_TTY, IST_EDGE};
use crate::arch::amd64::isa::isa_machdep::isa_intr_establish;
use crate::dev::ic::com::{com_enable_debugport_cn, comcn_read_reg};
use crate::dev::ic::comreg::{
    COM_DATA, COM_FREQ, COM_IIR, COM_LSR, CONADDR, IIR_NOPEND, LSR_RXRDY,
};
use crate::sys::errno::Errno;

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

/// The byte sink of the console's receive interrupt.
static CN_RX_SINK: StaticCell<Option<fn(u8)>> = StaticCell::new(None);

/// The console port's receive interrupt handler: drains the UART into the sink. What
/// `comintr` does for a port with a tty; the tty is M7.
fn cn_rx_intr(_arg: *mut core::ffi::c_void) -> i32 {
    if comcn_read_reg(COM_IIR) & IIR_NOPEND != 0 {
        return 0;
    }
    // SAFETY: written once by `cn_rx_intr_establish` before the interrupt is unmasked.
    let sink = unsafe { CN_RX_SINK.read() };
    while comcn_read_reg(COM_LSR) & LSR_RXRDY != 0 {
        let data = comcn_read_reg(COM_DATA);
        if let Some(sink) = sink {
            sink(data);
        }
    }
    1
}

/// Arms COM1's receive interrupt (IRQ 4, as `com0 at isa` is configured): the handler on
/// the i8259 through `isa_intr_establish`, and the UART's `IER`/`MCR` as
/// `com_enable_debugport` sets them.
pub fn cn_rx_intr_establish(sink: fn(u8)) -> Result<(), Errno> {
    // SAFETY: once, before the interrupt is established below.
    unsafe { CN_RX_SINK.write(Some(sink)) };
    // TODO(M5): the IRQ comes from the isa attach args (com_isa.c), 4 for COM1.
    let ih = isa_intr_establish(
        core::ptr::null(),
        4,
        IST_EDGE,
        IPL_TTY,
        cn_rx_intr,
        core::ptr::null_mut(),
        "com0",
    );
    if ih.is_none() {
        return Err(Errno::ENXIO);
    }
    com_enable_debugport_cn();
    Ok(())
}
