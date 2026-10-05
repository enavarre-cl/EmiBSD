//! Header ports: OpenBSD `sys/arch/amd64/include/*.h`.
//!
//! Constants, `#[repr(C)]` hardware structs and inline accessors only; never state.

pub mod _types;
pub mod apicvar;
pub mod biosvar;
pub mod bus;
pub mod cpu;
pub mod cpu_full;
pub mod cpufunc;
pub mod cpuvar;
pub mod db_machdep;
pub mod disklabel;
pub mod exec;
pub mod fpu;
pub mod frame;
pub mod i82093reg;
pub mod i82093var;
pub mod i82489reg;
pub mod i82489var;
pub mod i8259;
pub mod intr;
pub mod intrdefs;
pub mod mpbiosreg;
pub mod mpconfig;
pub mod mplock;
pub mod mutex;
pub mod param;
pub mod pcb;
pub mod pci_machdep;
pub mod pic;
pub mod pio;
pub mod pmap;
pub mod proc;
pub mod psl;
pub mod pte;
pub mod segments;
pub mod signal;
pub mod specialreg;
pub mod tcb;
pub mod timetc;
pub mod trap;
pub mod tss;
pub mod vmparam;
