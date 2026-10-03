//! Bus-independent chip drivers: OpenBSD `sys/dev/ic/`.

pub mod com;
pub mod comreg;
pub mod comvar;
pub mod i8253reg;
pub mod mc146818reg;
pub mod ns16550reg;
pub mod pluart;
