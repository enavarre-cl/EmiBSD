//! QEMU's `isa-debug-exit` device (feature `qemu`): writing `v` to its port ends the emulator
//! with exit status `(v << 1) | 1`. Not an OpenBSD file.

use super::super::Machine;
use super::super::include::pio::outb;
use crate::machine::api::{Cpu, ExitStatus};

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
