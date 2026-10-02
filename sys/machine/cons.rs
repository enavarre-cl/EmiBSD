//! `consinit(9)`: the machine-dependent half of the console framework.
//!
//! `<dev/cons.h>` and `dev/cons.c` (ported as `dev/cons.rs`) are generic: `cn_tab`, `cnputc`,
//! `cngetc`. What each `machdep.c` (or `consinit.c`) provides is `consinit()`, declared in
//! `<sys/systm.h>`: find the console device and attach it, once. `main()` calls it early, and
//! the architectures call it even earlier, from their first C function, so a panic during boot
//! has somewhere to print.

use crate::machine::Machine;
use crate::sys::errno::Errno;

/// The console attach each architecture provides.
pub trait Console {
    /// `consinit()`: attaches the console device. Idempotent: the second and later calls do
    /// nothing, as in every OpenBSD `machdep.c`.
    fn consinit();

    /// Arms the console UART's receive interrupt: `sink` is called from the UART's
    /// interrupt handler with every byte received. This is what the console's bus attachment
    /// (`com_isa.c`, `pluart_fdt.c`) does with `*_intr_establish` when autoconfiguration
    /// attaches the port; until then (M5) the machine does it here, for the M4 self-test.
    fn cn_rx_intr_establish(sink: fn(u8)) -> Result<(), Errno>;
}

/// `cn_rx_intr_establish` on the selected machine.
pub fn cn_rx_intr_establish(sink: fn(u8)) -> Result<(), Errno> {
    Machine::cn_rx_intr_establish(sink)
}

/// `consinit()` on the selected machine.
pub fn consinit() {
    Machine::consinit()
}
