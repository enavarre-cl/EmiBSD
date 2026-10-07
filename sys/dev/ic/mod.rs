//! Bus-independent chip drivers: OpenBSD `sys/dev/ic/`.

pub mod ac97;
pub mod ahci;
pub mod ahcireg;
pub mod ahcivar;
pub mod com;
pub mod comreg;
pub mod comvar;
pub mod i8253reg;
pub mod mc146818reg;
pub mod ns16550reg;
pub mod nvme;
pub mod nvmeio;
pub mod nvmereg;
pub mod nvmevar;
pub mod pluart;
pub mod re;
pub mod rtl81x9reg;
pub mod siop;
pub mod siop_common;
pub mod siopreg;
pub mod siopvar;
pub mod siopvar_common;
