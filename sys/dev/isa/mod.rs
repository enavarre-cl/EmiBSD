//! The ISA bus: OpenBSD `sys/dev/isa/`: the bus itself (`isa.c`, `isavar.h`), the register
//! map amd64's timer code needs (`isareg.h`) and `com(4)`'s attachment (`com_isa.c`).

pub mod com_isa;
#[allow(clippy::module_inception)] // OpenBSD's layout: sys/dev/isa/isa.c
pub mod isa;
pub mod isareg;
pub mod isavar;
