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
/* </LICENSES> */

/* <CODE> */
//! QEMU's `isa-debug-exit` device (feature `qemu`): writing `v` to its port ends the emulator
//! with exit status `(v << 1) | 1`. Not an OpenBSD file.

use super::super::Machine;
use super::super::include::pio::outb;
use crate::machine::{Cpu, ExitStatus};

/// The port `xtask qemu` configures: `-device isa-debug-exit,iobase=0xf4,iosize=0x04`.
const ISA_DEBUG_EXIT_PORT: u16 = 0xf4;

/// Ends the emulator so that its process exits with [`ExitStatus::qemu_status`].
pub fn exit(status: ExitStatus) -> ! {
    // qemu_status is (v << 1) | 1 by construction; recover v.
    let v = (status.qemu_status() - 1) / 2;
    // SAFETY: the port exists only under QEMU started with the device above, which is what
    // feature `qemu` promises; the write's only effect is ending the emulator.
    unsafe { outb(ISA_DEBUG_EXIT_PORT, v as u8) };
    Machine::halt()
}
/* </CODE> */
