//! What machine-independent autoconfiguration needs from the machine: the tables
//! `config(8)` generates into `ioconf.c` and the hooks each `arch/<arch>/<arch>/autoconf.c`
//! defines.
//!
//! OpenBSD's `subr_autoconf.c` reads `cfdata[]` and `cfroots[]` (from the kernel
//! configuration, compiled per machine), names `mainbus_cd` (each machine's root bus) and
//! calls `device_register()` (each machine's `autoconf.c`). Here `config(8)` is not ported:
//! each architecture writes its `ioconf` by hand in `sys/arch/<arch>/conf/ioconf.rs`, listing
//! the GENERIC entries whose drivers exist (`docs/ARCHITECTURE.md`, "Deviations").

use core::ffi::c_void;

use crate::machine::Machine;
use crate::sys::device::{Cfdata, Cfdriver, Device, Pdevinit};

/// The machine's autoconfiguration tables and hooks.
pub trait Autoconf {
    /// `cfdata[]`: every device the kernel configuration knows, in `ioconf.c`'s order (no
    /// terminating entry).
    fn cfdata() -> &'static [Cfdata];

    /// `cfroots[]`: the indices in `cfdata[]` of the root devices (no terminating `-1`).
    fn cfroots() -> &'static [i16];

    /// `mainbus_cd`: the root bus's driver, which `device_mainbus()` reads.
    fn mainbus_cd() -> &'static Cfdriver;

    /// `device_register(dev, aux)`: the machine's look at every device before it attaches
    /// (to find the boot device).
    fn device_register(dev: &Device, aux: *mut c_void);

    /// `pdevinit[]`: the pseudo-devices `main` attaches, in `ioconf.c`'s order (no
    /// terminating entry).
    fn pdevinit() -> &'static [Pdevinit];
}

/// `cfdata` on the selected machine.
pub fn cfdata() -> &'static [Cfdata] {
    Machine::cfdata()
}

/// `cfroots` on the selected machine.
pub fn cfroots() -> &'static [i16] {
    Machine::cfroots()
}

/// `mainbus_cd` on the selected machine.
pub fn mainbus_cd() -> &'static Cfdriver {
    Machine::mainbus_cd()
}

/// `pdevinit` on the selected machine.
pub fn pdevinit() -> &'static [Pdevinit] {
    Machine::pdevinit()
}

/// `device_register` on the selected machine.
pub fn device_register(dev: &Device, aux: *mut c_void) {
    Machine::device_register(dev, aux)
}
