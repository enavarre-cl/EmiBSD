//! The polled early console as a trait: what `cnputc(9)` (`<dev/cons.h>`, `dev/cons.c`) becomes
//! once the console framework is ported (milestone M2).

/// The polled early console: what `cnputc(9)` becomes once `dev/cons.c` is ported.
pub trait Console {
    /// Writes one byte, blocking until the device accepts it.
    fn putc(c: u8);
}
