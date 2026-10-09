/* <LICENSES> */
/*
 * Copyright (c) 2026 Emilio Navarrete Lineros <enavarre@outlook.com>
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */
/* </LICENSES> */

/* <CODE> */
//! What machine-independent autoconfiguration needs from the machine: the tables
//! `config(8)` generates into `ioconf.c` and the hooks each `arch/<arch>/<arch>/autoconf.c`
//! defines.
//!
//! OpenBSD's `subr_autoconf.c` reads `cfdata[]` and `cfroots[]` (from the kernel
//! configuration, compiled per machine), names `mainbus_cd` (each machine's root bus) and
//! calls `device_register()` (each machine's `autoconf.c`). Here `config(8)` is not ported:
//! each architecture writes its `ioconf` by hand in `sys/arch/<arch>/conf/ioconf.rs`, listing
//! the GENERIC entries whose drivers exist (`docs/ARCHITECTURE.md`, "Deviations").
//!
//! `boot -c` (`kern/subr_userconf.c`, UKC) edits those tables before autoconfiguration reads
//! them: the machine keeps them in `StaticCell`s, hands them to `user_config` once through
//! [`Autoconf::ioconf_mut`] and only then to everybody else. As `config(8)` does, `cfdata[]`
//! ends in free slots ([`Cfdata::free`]) for UKC's `add`; [`ioconf_cfdata`] cuts them off, as
//! the C's loops stop at the first entry without an attachment.

use core::ffi::c_void;

use crate::machine::Machine;
use crate::sys::device::{Cfdata, Cfdriver, Device, Nam2blk, Pdevinit};

/// The `ioconf.c` tables as UKC (`user_config`) edits them: the whole of `cfdata[]` with its
/// free slots, `cfroots[]`, `pdevinit[]`, and the names `config(8)` writes beside them.
pub struct IoconfTables<'a> {
    /// `cfdata[]`, free slots included.
    pub cfdata: &'a mut [Cfdata],
    /// `cfroots[]` (no terminating `-1`).
    pub cfroots: &'a mut [i16],
    /// `pdevinit[]` (no terminating entry).
    pub pdevinit: &'a mut [Pdevinit],
    /// `pdevnames[]`: the pseudo-devices' names, in `pdevinit[]`'s order.
    pub pdevnames: &'a [&'a [u8]],
    /// `locnames[]`: every locator name, once.
    pub locnames: &'a [&'a [u8]],
    /// `locnamp[]`: runs of indices into `locnames[]`, each ended by `-1`; an entry's
    /// `cf_locnames` is the start of its run.
    pub locnamp: &'a [i16],
}

/// The machine's autoconfiguration tables and hooks.
pub trait Autoconf {
    /// `cfdata[]`: every device the kernel configuration knows, in `ioconf.c`'s order (no
    /// terminating entry).
    fn cfdata() -> &'static [Cfdata];

    /// `cfroots[]`: the indices in `cfdata[]` of the root devices (no terminating `-1`).
    fn cfroots() -> &'static [i16];

    /// The tables for `user_config` to edit.
    ///
    /// # Safety
    ///
    /// Called once, by `user_config` on the boot CPU in `cpu_startup`, before anything has
    /// read `cfdata`, `cfroots` or `pdevinit`; the tables it returns are dropped before
    /// autoconfiguration starts.
    unsafe fn ioconf_mut() -> IoconfTables<'static>;

    /// `mainbus_cd`: the root bus's driver, which `device_mainbus()` reads.
    fn mainbus_cd() -> &'static Cfdriver;

    /// `device_register(dev, aux)`: the machine's look at every device before it attaches
    /// (to find the boot device).
    fn device_register(dev: &Device, aux: *mut c_void);

    /// `pdevinit[]`: the pseudo-devices `main` attaches, in `ioconf.c`'s order (no
    /// terminating entry).
    fn pdevinit() -> &'static [Pdevinit];
    /// `nam2blk[]`: the disk drivers' names and block majors, from the machine's
    /// `autoconf.c` (`findblkmajor`, `findblkname`; no terminating entry).
    fn nam2blk() -> &'static [Nam2blk];

    /// `diskconf()`: finds the boot device and configures the root, swap and dump devices
    /// (`setroot`); `main` calls it once autoconfiguration is done.
    fn diskconf();
}

/// `cfdata` on the selected machine.
pub fn cfdata() -> &'static [Cfdata] {
    Machine::cfdata()
}

/// `cfdata[]` up to its first free slot: the entries the kernel configuration has, as the C's
/// loops (`for (cf = cfdata; cf->cf_driver; cf++)`) see them.
pub fn ioconf_cfdata(table: &[Cfdata]) -> &[Cfdata] {
    let n = table
        .iter()
        .position(Cfdata::is_free)
        .unwrap_or(table.len());
    &table[..n]
}

/// The tables `user_config` edits, on the selected machine.
///
/// # Safety
///
/// As for [`Autoconf::ioconf_mut`].
pub unsafe fn ioconf_mut() -> IoconfTables<'static> {
    // SAFETY: the caller's contract is the trait method's.
    unsafe { Machine::ioconf_mut() }
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

/// `diskconf` on the selected machine.
pub fn diskconf() {
    Machine::diskconf()
}

/// `nam2blk` on the selected machine.
pub fn nam2blk() -> &'static [Nam2blk] {
    Machine::nam2blk()
}
/* </CODE> */
