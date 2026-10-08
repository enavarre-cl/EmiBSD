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
