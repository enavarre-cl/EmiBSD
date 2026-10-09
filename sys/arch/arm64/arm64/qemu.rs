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
//! Arm semihosting under QEMU (feature `qemu`): `SYS_EXIT` ends the emulator with the status
//! given in the parameter block. Not an OpenBSD file.

use core::arch::asm;

use super::super::Machine;
use crate::machine::{Cpu, ExitStatus};

/// Semihosting operation: exit the application.
const SYS_EXIT: u32 = 0x18;
/// Reason code: the application stopped on its own, with the exit code in the second word.
const ADP_STOPPED_APPLICATION_EXIT: u64 = 0x20026;

/// Ends the emulator so that its process exits with [`ExitStatus::qemu_status`].
pub fn exit(status: ExitStatus) -> ! {
    let block: [u64; 2] = [
        ADP_STOPPED_APPLICATION_EXIT,
        u64::from(status.qemu_status()),
    ];
    // SAFETY: `hlt #0xf000` is the AArch64 semihosting call. Under QEMU started with
    // `-semihosting-config enable=on,target=native` (what feature `qemu` promises) the emulator
    // handles it and exits; `block` lives on this stack frame for the whole call.
    unsafe {
        asm!(
            "hlt #0xf000",
            in("w0") SYS_EXIT,
            in("x1") block.as_ptr(),
            options(nostack, preserves_flags)
        );
    }
    Machine::halt()
}
/* </CODE> */
