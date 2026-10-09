/* <LICENSES> */
/*
 * Copyright (c) 1992, 1993
 *	The Regents of the University of California.  All rights reserved.
 *
 * This software was developed by the Computer Systems Engineering group
 * at Lawrence Berkeley Laboratory under DARPA contract BG 91-66 and
 * contributed to Berkeley.
 *
 * All advertising materials mentioning features or use of this software
 * must display the following acknowledgement:
 *	This product includes software developed by the University of
 *	California, Lawrence Berkeley Laboratory.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the University nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE REGENTS AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *	@(#)device.h	8.2 (Berkeley) 2/17/94
 */
/* </LICENSES> */

/* <CODE> */
//! Minimal device structures: `<sys/device.h>`, the types of autoconfiguration
//! (`autoconf(9)`, `config_attach(9)`).
//!
//! Upstream: sys/sys/device.h @ 3ce1f3f79392
//!
//! A driver is a [`Cfdriver`] (its name, class and array of attached units) plus one
//! [`Cfattach`] per bus it attaches to (its softc size and its match, attach, detach and
//! activate functions). `config(8)` would generate the [`Cfdata`] table that says which driver
//! may attach where; here each architecture writes it by hand (`sys/arch/<arch>/conf/ioconf.rs`,
//! reached through `machine::autoconf`). The functions declared by the header live where the C
//! defines them: `kern/subr_autoconf.rs` (`config_*`, `device_*`) and the machine's
//! `autoconf.rs` (`device_register`).
//!
//! A softc is a `#[repr(C)]` struct whose first field is the [`Device`], allocated by
//! `config_make_softc` with `malloc(ca_devsize, M_DEVBUF, M_NOWAIT|M_ZERO)` as in C; its type
//! implements [`Softc`] and the driver recovers it from the `&Device` it is handed with
//! [`Device::softc`], the C's `(struct foo_softc *)self`.
//!
//! ## Deviations
//! - Function pointers are Rust `fn` types; the `void *aux` of attach arguments stays an
//!   untyped pointer (`*mut c_void`) that each bus's children cast back to the bus's attach
//!   arguments, as in C. The `void *match` of `cfmatch_t`/`cfscan_t`, which is a `cfdata`
//!   or, below an indirect (`CD_INDIRECT`) parent, a preallocated softc, is the enum
//!   [`CfMatch`].
//! - `cfdata[]` and `cfroots[]` are slices without the C's terminating entry (a NULL
//!   `cf_driver`, a `-1` root); `cf_parents` and `cf_loc` are slices too. `config(8)`'s free
//!   `{0}` slots are [`Cfdata::free`] entries, whose attachment and driver are the statics
//!   [`CFATTACH_NULL`] and [`CFDRIVER_NULL`] instead of NULL.
//! - `cd_devs` holds `Option<NonNull<Device>>` slots (the C's `void *`), read through
//!   [`Cfdriver::cd_dev`]; `dv_ref` is atomic, as `atomic_inc_int` makes it in C.
//! - Fields autoconfiguration changes after boot (`cf_unit`, `cf_fstate`, `cd_devs`,
//!   `cd_ndevs`, every `struct device` member) are `Cell`s: the C changes them under the
//!   kernel lock, which a single CPU without preemption provides here.
//! - The prototypes of the suspend/resume (`request_sleep`, `sleep_state`, ...), root-device
//!   (`findblkmajor`, `setroot`, `parsedisk`, ...) and `loadfirmware` functions belong to files
//!   that are not ported (`subr_suspend.c`, `kern/subr_disk.c`, `dev/firmload.c`); only their
//!   types and constants are here.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::NonNull;
use core::sync::atomic::AtomicI32;

use crate::queue_adapter;
use crate::sys::errno::Errno;
use crate::sys::queue::{TailqEntry, TailqHead};

/// `DVACT_DEACTIVATE`: deactivate the device.
pub const DVACT_DEACTIVATE: i32 = 1;
/// `DVACT_QUIESCE`: warn the device about suspend.
pub const DVACT_QUIESCE: i32 = 2;
/// `DVACT_SUSPEND`: suspend the device.
pub const DVACT_SUSPEND: i32 = 3;
/// `DVACT_RESUME`: resume the device.
pub const DVACT_RESUME: i32 = 4;
/// `DVACT_WAKEUP`: tell device to recover after resume.
pub const DVACT_WAKEUP: i32 = 5;
/// `DVACT_POWERDOWN`: power device down.
pub const DVACT_POWERDOWN: i32 = 6;

/// `DVF_ACTIVE`: device is activated (`dv_flags`).
pub const DVF_ACTIVE: i32 = 0x0001;

/// `FSTATE_NOTFOUND`: has not been found.
pub const FSTATE_NOTFOUND: i16 = 0;
/// `FSTATE_FOUND`: has been found.
pub const FSTATE_FOUND: i16 = 1;
/// `FSTATE_STAR`: duplicable.
pub const FSTATE_STAR: i16 = 2;
/// `FSTATE_DNOTFOUND`: has not been found, and is disabled.
pub const FSTATE_DNOTFOUND: i16 = 3;
/// `FSTATE_DSTAR`: duplicable, and is disabled.
pub const FSTATE_DSTAR: i16 = 4;

/// `DETACH_FORCE`: force detachment; hardware gone (`config_detach`, `ca_detach`).
pub const DETACH_FORCE: i32 = 0x01;
/// `DETACH_QUIET`: don't print a notice.
pub const DETACH_QUIET: i32 = 0x02;

/// `CD_INDIRECT` (`cd_mode`): the children match against a preallocated softc.
pub const CD_INDIRECT: i32 = 1;
/// `CD_SKIPHIBERNATE`: not attached when unhibernating.
pub const CD_SKIPHIBERNATE: i32 = 2;
/// `CD_COCOVM`: allow a device on a VM employing confidential computing methods, e.g. AMD
/// SEV.
pub const CD_COCOVM: i32 = 4;

/// `QUIET`: print nothing (a `cfprint_t` result).
pub const QUIET: i32 = 0;
/// `UNCONF`: print " not configured\n".
pub const UNCONF: i32 = 1;
/// `UNSUPP`: print " not supported\n".
pub const UNSUPP: i32 = 2;

/// `SLEEP_RESUME` (`sleep_mode`).
pub const SLEEP_RESUME: i32 = 0;
/// `SLEEP_SUSPEND`.
pub const SLEEP_SUSPEND: i32 = 1;
/// `SLEEP_HIBERNATE`.
pub const SLEEP_HIBERNATE: i32 = 2;

/// `FIRMWARE_MAX`: the largest file `loadfirmware` reads.
pub const FIRMWARE_MAX: usize = 24 * 1024 * 1024;

/// `enum devclass`: minimal device classes. Note that all "system" device types are listed
/// here.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(non_camel_case_types)] // OpenBSD names, verbatim, for grep-ability
pub enum Devclass {
    /// Generic, no special info.
    DV_DULL = 0,
    /// CPU (carries resource utilization).
    DV_CPU = 1,
    /// Disk drive (label, etc).
    DV_DISK = 2,
    /// Network interface.
    DV_IFNET = 3,
    /// Tape device.
    DV_TAPE = 4,
    /// Serial line interface (???).
    DV_TTY = 5,
}

pub use Devclass::*;

/// `struct device`: what every softc starts with.
///
/// Allocated zeroed by `config_make_softc`, so every field is valid as all-zero bits:
/// `DV_DULL`, null links, `None`, 0.
#[repr(C)]
pub struct Device {
    /// `dv_class`: this device's classification.
    pub dv_class: Cell<Devclass>,
    /// `dv_list`: entry on list of all devices (`alldevs`).
    pub dv_list: TailqEntry<Device>,
    /// `dv_cfdata`: config data that found us.
    pub dv_cfdata: Cell<Option<&'static Cfdata>>,
    /// `dv_unit`: device unit number.
    pub dv_unit: Cell<i32>,
    /// `dv_xname`: external name (name + unit), NUL-terminated.
    pub dv_xname: Cell<[u8; 16]>,
    /// `dv_parent`: pointer to parent device; `None` for a root device.
    pub dv_parent: Cell<Option<NonNull<Device>>>,
    /// `dv_flags`: misc. flags (`DVF_*`).
    pub dv_flags: Cell<i32>,
    /// `dv_ref`: ref count.
    pub dv_ref: AtomicI32,
}

impl Device {
    /// `dv_cfdata`, which every device made by `config_make_softc` has.
    pub fn cfdata(&self) -> &'static Cfdata {
        match self.dv_cfdata.get() {
            Some(cf) => cf,
            None => crate::kern::subr_prf::panic(format_args!("device without cfdata")),
        }
    }

    /// `dv_parent`.
    pub fn parent(&self) -> Option<&Device> {
        // SAFETY: a device with children is never detached (`config_detach` refuses, and
        // panics under `diagnostic`, until `config_detach_children` has run), so a parent
        // outlives its children.
        self.dv_parent.get().map(|p| unsafe { p.as_ref() })
    }

    /// `(struct foo_softc *)self`: the softc this device is the head of.
    ///
    /// # Safety
    ///
    /// `self` was allocated by `config_make_softc` for a [`Cfattach`] whose `ca_devsize` is
    /// at least `size_of::<T>()`, which is the case inside that attachment's own functions.
    pub unsafe fn softc<T: Softc>(&self) -> &T {
        // SAFETY: `T` is `#[repr(C)]` with the device first (the `Softc` contract), so the
        // device's address is the softc's; the caller guarantees the allocation holds a `T`.
        unsafe { &*core::ptr::from_ref(self).cast::<T>() }
    }

    /// `dv_xname` as a string (`"com0"`), for the messages and interrupt names the C prints
    /// with `%s` from it.
    pub fn xname(&self) -> &str {
        // SAFETY: `config_attach` writes `dv_xname` before the driver sees the device and
        // never again, so no `set` can overlap this shared view of the cell's bytes.
        let bytes = unsafe { &*self.dv_xname.as_ptr() };
        let len = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        core::str::from_utf8(&bytes[..len]).unwrap_or("?")
    }
}

/// A driver's softc: `struct foo_softc { struct device sc_dev; ... }`.
///
/// # Safety
///
/// The implementing type is `#[repr(C)]`, its first field is a [`Device`], and the all-zero
/// bit pattern is a valid value of it (`config_make_softc` allocates with `M_ZERO`).
pub unsafe trait Softc: Sized {}

// SAFETY: a device is trivially its own head, and all-zero is valid (see `Device`).
unsafe impl Softc for Device {}

queue_adapter!(
    /// `TAILQ_ENTRY(device) dv_list`: `alldevs`.
    pub DeviceList: Device, dv_list => TailqEntry<Device>
);

/// `TAILQ_HEAD(devicelist, device)`, made `Sync`: the device lists are touched under the
/// kernel lock.
pub struct Devicelist(pub TailqHead<DeviceList>);

// SAFETY: see the type's doc; one CPU here, and autoconfiguration does not run from
// interrupts.
unsafe impl Sync for Devicelist {}

/// `struct cfdata`: configuration data (i.e., data placed in `ioconf.c`).
pub struct Cfdata {
    /// `cf_attach`: config attachment.
    pub cf_attach: &'static Cfattach,
    /// `cf_driver`: config driver.
    pub cf_driver: &'static Cfdriver,
    /// `cf_unit`: unit number (the next one, for a starred entry).
    pub cf_unit: Cell<i16>,
    /// `cf_fstate`: finding state (`FSTATE_*`).
    pub cf_fstate: Cell<i16>,
    /// `cf_loc`: locators (machine dependent).
    pub cf_loc: &'static [i64],
    /// `cf_flags`: flags from config.
    pub cf_flags: i32,
    /// `cf_parents`: potential parents, as indices into `cfdata[]`.
    pub cf_parents: &'static [i16],
    /// `cf_locnames`: start of names (in the `locnamp[]` of `ioconf.c`).
    pub cf_locnames: i32,
    /// `cf_starunit1`: 1st usable unit number by STAR.
    pub cf_starunit1: i16,
}

impl Cfdata {
    /// One `cfdata[]` entry, as `config(8)` writes it.
    #[allow(clippy::too_many_arguments)] // the C struct's nine members, in order
    pub const fn new(
        cf_attach: &'static Cfattach,
        cf_driver: &'static Cfdriver,
        cf_unit: i16,
        cf_fstate: i16,
        cf_loc: &'static [i64],
        cf_flags: i32,
        cf_parents: &'static [i16],
        cf_locnames: i32,
        cf_starunit1: i16,
    ) -> Self {
        Self {
            cf_attach,
            cf_driver,
            cf_unit: Cell::new(cf_unit),
            cf_fstate: Cell::new(cf_fstate),
            cf_loc,
            cf_flags,
            cf_parents,
            cf_locnames,
            cf_starunit1,
        }
    }

    /// `{0}`: a free slot, of the eight `config(8)` writes at the end of `cfdata[]` for UKC's
    /// `add` (`boot -c`, `kern/subr_userconf.c`). Its attachment is [`CFATTACH_NULL`], the C's
    /// NULL `cf_attach`.
    pub const fn free() -> Self {
        Self::new(&CFATTACH_NULL, &CFDRIVER_NULL, 0, 0, &[], 0, &[], 0, 0)
    }

    /// `cf_attach == NULL`: a free slot, where the C's walks of `cfdata[]` stop.
    pub fn is_free(&self) -> bool {
        core::ptr::eq(self.cf_attach, &CFATTACH_NULL)
    }
}

/// `cfdata[i+1] = cfdata[i]`: UKC's `add` moves and copies whole entries.
impl Clone for Cfdata {
    fn clone(&self) -> Self {
        Self::new(
            self.cf_attach,
            self.cf_driver,
            self.cf_unit.get(),
            self.cf_fstate.get(),
            self.cf_loc,
            self.cf_flags,
            self.cf_parents,
            self.cf_locnames,
            self.cf_starunit1,
        )
    }
}

// SAFETY: `cf_unit` and `cf_fstate` change under the kernel lock (`config_attach`,
// `config_detach`); one CPU here.
unsafe impl Sync for Cfdata {}

/// A softc preallocated for a match below an indirect parent (`CD_INDIRECT`).
///
/// Only `config_search`/`config_scan` make one, from `config_make_softc`; whoever ends up
/// with it attaches it (`config_attach`) or drops the claim, as the C's caller frees it.
pub struct SoftcMatch(NonNull<Device>);

impl SoftcMatch {
    /// Wraps a softc `config_make_softc` returned.
    ///
    /// # Safety
    ///
    /// `dev` is a live softc made by `config_make_softc` that nothing else will free while
    /// this value exists.
    pub(crate) unsafe fn new(dev: NonNull<Device>) -> Self {
        Self(dev)
    }

    /// The softc.
    pub fn device(&self) -> &Device {
        // SAFETY: `new`'s contract: the softc lives as long as this value.
        unsafe { self.0.as_ref() }
    }

    /// The softc's pointer, handing over the claim.
    pub(crate) fn into_raw(self) -> NonNull<Device> {
        self.0
    }
}

/// The `void *match` of `cfmatch_t` and `cfscan_t`.
pub enum CfMatch {
    /// The usual case: the `cfdata[]` entry being tried.
    Cfdata(&'static Cfdata),
    /// Below a `CD_INDIRECT` parent: a softc already made for the entry.
    Softc(SoftcMatch),
}

impl CfMatch {
    /// The `cfdata[]` entry: the match itself, or the softc's `dv_cfdata`.
    pub fn cfdata(&self) -> &'static Cfdata {
        match self {
            CfMatch::Cfdata(cf) => cf,
            CfMatch::Softc(sc) => sc.device().cfdata(),
        }
    }
}

/// `cfmatch_t`: returns a match level, 0 for no match; `parent` is `None` for a root.
pub type CfmatchT = fn(parent: Option<&Device>, match_: &CfMatch, aux: *mut c_void) -> i32;

/// `cfscan_t`: called for every potential child; it owns the match.
pub type CfscanT = fn(parent: &Device, match_: CfMatch);

/// The type of `ca_attach`: `void (*)(struct device *, struct device *, void *)`.
pub type CaAttachFn = fn(parent: Option<&Device>, self_: &Device, aux: *mut c_void);

/// The type of `ca_detach`: `int (*)(struct device *, int)`.
pub type CaDetachFn = fn(dev: &Device, flags: i32) -> Result<(), Errno>;

/// The type of `ca_activate`: `int (*)(struct device *, int)`.
pub type CaActivateFn = fn(dev: &Device, act: i32) -> Result<(), Errno>;

/// `struct cfattach`: the `configuration` attachment of a driver to one bus attribute (what
/// the machine-independent autoconf uses).
///
/// As devices are found, they are applied against all the potential matches. The one with the
/// best match is taken, and a device structure (plus any other data desired) is allocated.
/// Pointers to these are placed into an array of pointers. The array itself must be dynamic
/// since devices can be found long after the machine is up and running.
///
/// Devices can have multiple configuration attachments if they attach to different attributes
/// (busses, or whatever), to allow specification of multiple match and attach functions. There
/// is only one configuration driver per driver, so that things like unit numbers and the
/// device structure array will be shared.
pub struct Cfattach {
    /// `ca_devsize`: size of dev data (for malloc).
    pub ca_devsize: usize,
    /// `ca_match`: returns a match level.
    pub ca_match: Option<CfmatchT>,
    /// `ca_attach(parent, self, aux)`; `parent` is `None` for a root.
    pub ca_attach: CaAttachFn,
    /// `ca_detach(self, flags)`.
    pub ca_detach: Option<CaDetachFn>,
    /// `ca_activate(self, act)`.
    pub ca_activate: Option<CaActivateFn>,
}

/// `struct cfdriver`: one per driver.
pub struct Cfdriver {
    /// `cd_devs`: devices found, `cd_ndevs` slots indexed by unit (`mallocarray`ed).
    pub cd_devs: Cell<*mut Option<NonNull<Device>>>,
    /// `cd_name`: device name.
    pub cd_name: &'static [u8],
    /// `cd_class`: device classification.
    pub cd_class: Devclass,
    /// `cd_mode`: device type subclassification (`CD_*`).
    pub cd_mode: i32,
    /// `cd_ndevs`: size of `cd_devs` array.
    pub cd_ndevs: Cell<i32>,
}

impl Cfdriver {
    /// `{ NULL, "name", class, mode }`: a driver with no units yet.
    pub const fn new(cd_name: &'static [u8], cd_class: Devclass, cd_mode: i32) -> Self {
        Self {
            cd_devs: Cell::new(core::ptr::null_mut()),
            cd_name,
            cd_class,
            cd_mode,
            cd_ndevs: Cell::new(0),
        }
    }

    /// `cd->cd_devs[unit]`, `None` outside the array or for an empty slot.
    pub fn cd_dev(&self, unit: i32) -> Option<NonNull<Device>> {
        if unit < 0 || unit >= self.cd_ndevs.get() {
            return None;
        }
        // SAFETY: `cd_devs` has `cd_ndevs` initialised slots (`config_make_softc`).
        unsafe { *self.cd_devs.get().add(unit as usize) }
    }
}

// SAFETY: `cd_devs` and `cd_ndevs` change under the kernel lock (`config_make_softc`,
// `config_attach`, `config_detach`); one CPU here.
unsafe impl Sync for Cfdriver {}

/// `cfprint_t`: configuration printing function. The second argument is `None` if the
/// device was configured; otherwise it is the name of the parent device. The return value
/// (`QUIET`, `UNCONF`, `UNSUPP`) is ignored if the device was configured, so most functions
/// can return `UNCONF` unconditionally.
pub type CfprintT = fn(aux: *mut c_void, pnp: Option<&[u8]>) -> i32;

/// `struct pdevinit`: pseudo-device attach information (function + number of pseudo-devs).
pub struct Pdevinit {
    /// `pdev_attach`.
    pub pdev_attach: fn(i32),
    /// `pdev_count`.
    pub pdev_count: i32,
}

/// `struct nam2blk`: a disk driver's name and block major (`nam2blk[]` in each `autoconf.c`).
pub struct Nam2blk {
    /// `name`.
    pub name: &'static [u8],
    /// `maj`.
    pub maj: i32,
}

/// The attachment of a free `cfdata[]` slot ([`Cfdata::free`]): the C's NULL `cf_attach`.
/// Nothing attaches through it: every walk of `cfdata[]` ends before the first free slot
/// (`machine::autoconf::ioconf_cfdata`).
pub static CFATTACH_NULL: Cfattach = Cfattach {
    ca_devsize: 0,
    ca_match: None,
    ca_attach: |_, _, _| {},
    ca_detach: None,
    ca_activate: None,
};

/// The driver of a free `cfdata[]` slot: the C's NULL `cf_driver`, with an empty name.
pub static CFDRIVER_NULL: Cfdriver = Cfdriver::new(b"", DV_DULL, 0);
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for `<sys/device.h>`: the types and the constants (against the C header).

    use super::*;

    #[test]
    fn devclass_values_follow_the_c_enum() {
        assert_eq!(DV_DULL as i32, 0);
        assert_eq!(DV_TTY as i32, 5);
    }

    #[test]
    fn cfdriver_starts_without_units() {
        static CD: Cfdriver = Cfdriver::new(b"test", DV_DULL, 0);
        assert_eq!(CD.cd_ndevs.get(), 0);
        assert!(CD.cd_dev(0).is_none());
        assert!(CD.cd_dev(-1).is_none());
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/sys/device.h");
        let ours: &[(&str, i64)] = &[
            ("DVACT_DEACTIVATE", DVACT_DEACTIVATE.into()),
            ("DVACT_QUIESCE", DVACT_QUIESCE.into()),
            ("DVACT_SUSPEND", DVACT_SUSPEND.into()),
            ("DVACT_RESUME", DVACT_RESUME.into()),
            ("DVACT_WAKEUP", DVACT_WAKEUP.into()),
            ("DVACT_POWERDOWN", DVACT_POWERDOWN.into()),
            ("DVF_ACTIVE", DVF_ACTIVE.into()),
            ("FSTATE_NOTFOUND", FSTATE_NOTFOUND.into()),
            ("FSTATE_FOUND", FSTATE_FOUND.into()),
            ("FSTATE_STAR", FSTATE_STAR.into()),
            ("FSTATE_DNOTFOUND", FSTATE_DNOTFOUND.into()),
            ("FSTATE_DSTAR", FSTATE_DSTAR.into()),
            ("DETACH_FORCE", DETACH_FORCE.into()),
            ("DETACH_QUIET", DETACH_QUIET.into()),
            ("CD_INDIRECT", CD_INDIRECT.into()),
            ("CD_SKIPHIBERNATE", CD_SKIPHIBERNATE.into()),
            ("CD_COCOVM", CD_COCOVM.into()),
            ("QUIET", QUIET.into()),
            ("UNCONF", UNCONF.into()),
            ("UNSUPP", UNSUPP.into()),
            ("SLEEP_RESUME", SLEEP_RESUME.into()),
            ("SLEEP_SUSPEND", SLEEP_SUSPEND.into()),
            ("SLEEP_HIBERNATE", SLEEP_HIBERNATE.into()),
        ];
        for (name, value) in ours {
            assert_eq!(crate::reftest::int(&defs, name), Some(*value), "{name}");
        }
    }
}
/* </TESTS> */
