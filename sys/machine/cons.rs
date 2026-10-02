//! `consinit(9)`: the machine-dependent half of the console framework.
//!
//! `<dev/cons.h>` and `dev/cons.c` (ported as `dev/cons.rs`) are generic: `cn_tab`, `cnputc`,
//! `cngetc`. What each `machdep.c` (or `consinit.c`) provides is `consinit()`, declared in
//! `<sys/systm.h>`: find the console device and attach it, once. `main()` calls it early, and
//! the architectures call it even earlier, from their first C function, so a panic during boot
//! has somewhere to print.

use crate::machine::Machine;

/// The console attach each architecture provides.
pub trait Console {
    /// `consinit()`: attaches the console device. Idempotent: the second and later calls do
    /// nothing, as in every OpenBSD `machdep.c`.
    fn consinit();
}

/// `consinit()` on the selected machine.
pub fn consinit() {
    Machine::consinit()
}
