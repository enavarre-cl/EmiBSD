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
 *	California, Lawrence Berkeley Laboratories.
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
 * from: Header: subr_autoconf.c,v 1.12 93/02/01 19:31:48 torek Exp  (LBL)
 *
 *	@(#)subr_autoconf.c	8.1 (Berkeley) 6/10/93
 */
/* </LICENSES> */

/* <CODE> */
//! Autoconfiguration subroutines: `kern/subr_autoconf.c`, see `autoconf(9)`,
//! `config_attach(9)`, `config_defer(9)`, `config_found(9)`, `device_lookup(9)`.
//!
//! Upstream: sys/kern/subr_autoconf.c @ 3ce1f3f79392
//!
//! A bus driver's attach function describes each device it finds in an attach-arguments
//! structure and calls `config_found`; `config_search` walks `cfdata[]` for the entries whose
//! parents include the bus, runs their match functions and keeps the best; `config_attach`
//! makes the softc, names it (`dv_xname`), files it in `alldevs` and the driver's `cd_devs`,
//! prints `"<name> at <parent>"` and calls the driver's attach function. The machine starts it
//! all with `config_rootfound("mainbus", ...)` from `cpu_configure`.
//!
//! ## Deviations
//! - `cfdata[]`, `cfroots[]` and `mainbus_cd` come from the machine
//!   (`machine::autoconf`, written by hand in `sys/arch/<arch>/conf/ioconf.rs` instead of by
//!   `config(8)`); `device_register` is the machine's too, as in C.
//! - `config_attach` tells a preallocated softc from a `cfdata` by the [`CfMatch`] it is given
//!   instead of by the parent's `CD_INDIRECT`, which is what produced that variant.
//! - Devices are `NonNull<Device>` where the C may free them (`config_attach`'s result,
//!   `config_detach`, `device_unref`, `device_lookup`), so no reference outlives the softc;
//!   `config_detach` and `device_unref` are `unsafe` for that reason.
//! - `config_detach`'s last loop in C leaves `cf` at `cfdata[]`'s terminating entry and then
//!   writes `cf->cf_unit = 0` into it when the driver has no units left, a store nothing reads;
//!   without a terminating entry the store is not made.
//! - `hotplug(4)` is in both GENERICs (`NHOTPLUG > 0`) but `dev/hotplug.c` is not ported:
//!   `hotplug_device_attach`/`hotplug_device_detach` are reported when not `cold`.
//! - `NMPATH` is 0: `mpath(4)` is not in the machines' `ioconf.rs` (`scsi/mpath.c` is not
//!   ported), so `device_mpath` returns `None`, as the C compiles it with a count of 0.
//! - `config_mountroot` sees no root vnode (`rootvp`; there is no VFS until M10), so it always
//!   queues the function for `config_process_deferred_mountroot`, as the C does before the
//!   root is mounted.
//! - `autoconf_verbose` starts at 0 (`AUTOCONF_VERBOSE`, a kernel option not configured).

use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI32, Ordering};

use crate::kern::init_main::BOOTHOWTO;
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_malloc::{free, malloc, mallocarray};
use crate::kern::kern_synch::{msleep_nsec, wakeup};
use crate::kern::subr_prf::{Str, panic, printf, snprintf};
use crate::machine::autoconf::{cfdata, cfroots, device_register, mainbus_cd};
use crate::machine::intr::IPL_HIGH;
use crate::sys::device::{
    CD_COCOVM, CD_INDIRECT, CD_SKIPHIBERNATE, CfMatch, Cfdata, Cfdriver, CfmatchT, CfprintT,
    CfscanT, DETACH_FORCE, DETACH_QUIET, DV_IFNET, DV_TAPE, DVACT_DEACTIVATE, DVACT_POWERDOWN,
    DVACT_QUIESCE, DVACT_RESUME, DVACT_SUSPEND, DVACT_WAKEUP, DVF_ACTIVE, Device, DeviceList,
    Devicelist, FSTATE_DNOTFOUND, FSTATE_DSTAR, FSTATE_FOUND, FSTATE_NOTFOUND, FSTATE_STAR,
    SoftcMatch,
};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_DEVBUF, M_NOWAIT, M_ZERO, MINALLOCSIZE};
use crate::sys::mutex::Mutex;
use crate::sys::param::PWAIT;
use crate::sys::queue::{TailqEntry, TailqHead};
use crate::sys::reboot::{RB_COCOVM, RB_UNHIBERNATE};
use crate::sys::systm::{COLD, INFSLP};
use crate::{kassert, queue_adapter, unported};

/// `msgs[]`: what `config_found` prints after a `cfprint_t` returns `QUIET`, `UNCONF` or
/// `UNSUPP`.
const MSGS: [&str; 3] = ["", " not configured\n", " unsupported\n"];

/// `struct matchinfo`: the state of one search for the best match.
struct Matchinfo<'a> {
    /// `fn`: the submatch function, `None` for each candidate's `ca_match`.
    fn_: Option<CfmatchT>,
    /// `parent`; `None` (`ROOT`) for a root search.
    parent: Option<&'a Device>,
    /// `match`: the best match so far.
    match_: Option<CfMatch>,
    /// `aux`.
    aux: *mut c_void,
    /// `indirect`: the parent's driver is `CD_INDIRECT`.
    indirect: bool,
    /// `pri`: the best match level so far.
    pri: i32,
}

/// `struct deferred_config`: a device whose configuration waits for its parent's other
/// children, or for the root file system.
struct DeferredConfig {
    /// `dc_queue`.
    dc_queue: TailqEntry<DeferredConfig>,
    /// `dc_dev`.
    dc_dev: NonNull<Device>,
    /// `dc_func`.
    dc_func: fn(&Device),
}

queue_adapter!(
    /// `TAILQ_ENTRY(deferred_config) dc_queue`.
    DcQueue: DeferredConfig, dc_queue => TailqEntry<DeferredConfig>
);

/// A deferred-configuration queue head, made `Sync`: the queues are touched under the kernel
/// lock, from process context.
struct DeferredConfigQueue(TailqHead<DcQueue>);

// SAFETY: see the type's doc; one CPU here.
unsafe impl Sync for DeferredConfigQueue {}

/// `autoconf_verbose`: trace probe calls.
pub static AUTOCONF_VERBOSE: AtomicI32 = AtomicI32::new(0);

/// `deferred_config_queue`.
static DEFERRED_CONFIG_QUEUE: DeferredConfigQueue = DeferredConfigQueue(TailqHead::new());
/// `mountroot_config_queue`.
static MOUNTROOT_CONFIG_QUEUE: DeferredConfigQueue = DeferredConfigQueue(TailqHead::new());

/// `alldevs`: list of all devices.
pub static ALLDEVS: Devicelist = Devicelist(TailqHead::new());

/// `config_pending`: semaphore for mountroot.
pub static CONFIG_PENDING: AtomicI32 = AtomicI32::new(0);

/// `autoconf_attdet_mtx`: protects `autoconf_attdet` and `autoconf_serial`.
static AUTOCONF_ATTDET_MTX: Mutex = Mutex::new(IPL_HIGH);
/// `autoconf_attdet`: if > 0, devices are being attached and any thread which tries to detach
/// will sleep; if < 0 devices are being detached and any thread which tries to attach will
/// sleep. Protected by: `autoconf_attdet_mtx`.
static AUTOCONF_ATTDET: AtomicI32 = AtomicI32::new(0);

/// `autoconf_serial`: versioned state of the devices tree so that changes can be detected.
pub static AUTOCONF_SERIAL: AtomicI32 = AtomicI32::new(0);

/// A C string argument up to its first NUL.
fn cstr(s: &[u8]) -> &[u8] {
    &s[..s.iter().position(|&c| c == 0).unwrap_or(s.len())]
}

/// `dv_xname` as a printable string.
fn xname(dev: &Device) -> [u8; 16] {
    dev.dv_xname.get()
}

/// `config_init`: initialize autoconfiguration data structures. This occurs before console
/// initialization as that might require use of this subsystem. Furthermore this means that
/// malloc et al. isn't yet available.
pub fn config_init() {
    DEFERRED_CONFIG_QUEUE.0.init();
    MOUNTROOT_CONFIG_QUEUE.0.init();
    ALLDEVS.0.init();
}

/// Frees a softc preallocated for a match that lost (`free(match, M_DEVBUF, ca_devsize)`).
fn free_match(match_: CfMatch) {
    if let CfMatch::Softc(sc) = match_ {
        let size = sc.device().cfdata().cf_attach.ca_devsize;
        free(sc.into_raw().cast(), M_DEVBUF, size);
    }
}

/// `mapply`: apply the matching function and choose the best. This is used a few times and we
/// want to keep the code small.
fn mapply(m: &mut Matchinfo<'_>, cf: &'static Cfdata) {
    let match_ = if m.indirect {
        // SAFETY: a fresh softc, owned by this match until it wins or is freed below.
        CfMatch::Softc(unsafe { SoftcMatch::new(config_make_softc(m.parent, cf)) })
    } else {
        CfMatch::Cfdata(cf)
    };

    let verbose = AUTOCONF_VERBOSE.load(Ordering::Relaxed) != 0;
    if verbose {
        printf(format_args!(
            ">>> probing for {}",
            Str(cf.cf_driver.cd_name)
        ));
        if cf.cf_fstate.get() == FSTATE_STAR {
            printf(format_args!("*\n"));
        } else {
            printf(format_args!("{}\n", cf.cf_unit.get()));
        }
    }
    let pri = match m.fn_ {
        Some(f) => f(m.parent, &match_, m.aux),
        None => match cf.cf_attach.ca_match {
            Some(ca_match) => ca_match(m.parent, &match_, m.aux),
            None => panic(format_args!(
                "mapply: no match function for '{}' device",
                Str(cf.cf_driver.cd_name)
            )),
        },
    };
    if verbose {
        printf(format_args!(
            ">>> {} probe returned {}\n",
            Str(cf.cf_driver.cd_name),
            pri
        ));
    }

    if pri > m.pri {
        if let Some(old) = m.match_.take() {
            free_match(old);
        }
        m.match_ = Some(match_);
        m.pri = pri;
    } else {
        free_match(match_);
    }
}

/// Whether `parent`'s `cfdata` is `cfdata[p]` for one of `cf`'s potential parents `p`, once
/// per such parent (the C applies the match once per matching parent index).
fn parent_matches(parent: &Device, cf: &Cfdata) -> usize {
    let table = cfdata();
    let pcf = parent.dv_cfdata.get();
    cf.cf_parents
        .iter()
        .filter(|&&p| {
            table
                .get(p as usize)
                .is_some_and(|c| pcf.is_some_and(|pcf| ptr::eq(pcf, c)))
        })
        .count()
}

/// `config_search`: iterate over all potential children of some device, calling the given
/// function (default being the child's match function) for each one. Nonzero returns are
/// matches; the highest value returned is considered the best match. Return the "found child"
/// if we got a match, or `None` otherwise. The `aux` pointer is simply passed on through.
///
/// Note that this function is designed so that it can be used to apply an arbitrary function
/// to all potential children (its return value can be ignored).
pub fn config_search(fn_: Option<CfmatchT>, parent: &Device, aux: *mut c_void) -> Option<CfMatch> {
    let mut m = Matchinfo {
        fn_,
        parent: Some(parent),
        match_: None,
        aux,
        indirect: parent.cfdata().cf_driver.cd_mode & CD_INDIRECT != 0,
        pri: 0,
    };
    let boothowto = BOOTHOWTO.load(Ordering::Relaxed);

    for cf in cfdata() {
        // Skip cf if no longer eligible, otherwise scan through parents for one matching
        // `parent', and try match function.
        let fstate = cf.cf_fstate.get();
        if fstate == FSTATE_FOUND {
            continue;
        }
        if fstate == FSTATE_DNOTFOUND || fstate == FSTATE_DSTAR {
            continue;
        }
        let cd = cf.cf_driver;
        if boothowto & RB_UNHIBERNATE != 0 {
            if cd.cd_mode & CD_SKIPHIBERNATE != 0 {
                continue;
            }
            if cd.cd_class == DV_IFNET {
                continue;
            }
            if cd.cd_class == DV_TAPE {
                continue;
            }
        }
        if boothowto & RB_COCOVM != 0 && cd.cd_mode & CD_COCOVM == 0 {
            continue;
        }
        for _ in 0..parent_matches(parent, cf) {
            mapply(&mut m, cf);
        }
    }

    if AUTOCONF_VERBOSE.load(Ordering::Relaxed) != 0 {
        match &m.match_ {
            Some(found) => printf(format_args!(
                ">>> {} probe won\n",
                Str(found.cfdata().cf_driver.cd_name)
            )),
            None => printf(format_args!(">>> no winning probe\n")),
        };
    }
    m.match_
}

/// `config_scan`: iterate over all potential children of some device, calling the given
/// function for each one.
///
/// Note that this function is designed so that it can be used to apply an arbitrary function
/// to all potential children (its return value can be ignored).
pub fn config_scan(fn_: CfscanT, parent: &Device) {
    let indirect = parent.cfdata().cf_driver.cd_mode & CD_INDIRECT != 0;

    for cf in cfdata() {
        // Skip cf if no longer eligible, otherwise scan through parents for one matching
        // `parent', and try match function.
        let fstate = cf.cf_fstate.get();
        if fstate == FSTATE_FOUND {
            continue;
        }
        if fstate == FSTATE_DNOTFOUND || fstate == FSTATE_DSTAR {
            continue;
        }
        for _ in 0..parent_matches(parent, cf) {
            let match_ = if indirect {
                // SAFETY: a fresh softc, handed to `fn_`, which owns it from now on.
                CfMatch::Softc(unsafe { SoftcMatch::new(config_make_softc(Some(parent), cf)) })
            } else {
                CfMatch::Cfdata(cf)
            };
            fn_(parent, match_);
        }
    }
}

/// `config_rootsearch`: find the given root device. This is much like `config_search`, but
/// there is no parent.
pub fn config_rootsearch(
    fn_: Option<CfmatchT>,
    rootname: &[u8],
    aux: *mut c_void,
) -> Option<CfMatch> {
    let mut m = Matchinfo {
        fn_,
        parent: None,
        match_: None,
        aux,
        indirect: false,
        pri: 0,
    };
    let table = cfdata();
    // Look at root entries for matching name. We do not bother with found-state here since
    // only one instance of each possible root child should ever be searched.
    for &p in cfroots() {
        let Some(cf) = table.get(p as usize) else {
            continue;
        };
        let fstate = cf.cf_fstate.get();
        if fstate == FSTATE_DNOTFOUND || fstate == FSTATE_DSTAR {
            continue;
        }
        if cstr(cf.cf_driver.cd_name) == cstr(rootname) {
            mapply(&mut m, cf);
        }
    }
    m.match_
}

/// `config_found_sm`: the given `aux` argument describes a device that has been found on the
/// given parent, but not necessarily configured. Locate the configuration data for that
/// device (using the submatch function provided, or using candidates' `ca_match`
/// configuration driver functions) and attach it, and return it. If the device was not
/// configured, call the given `print` function and return `None`.
pub fn config_found_sm(
    parent: &Device,
    aux: *mut c_void,
    print: Option<CfprintT>,
    submatch: Option<CfmatchT>,
) -> Option<NonNull<Device>> {
    if let Some(match_) = config_search(submatch, parent, aux) {
        return Some(config_attach(Some(parent), match_, aux, print));
    }
    if let Some(print) = print {
        let name = xname(parent);
        let r = print(aux, Some(cstr(&name)));
        printf(format_args!(
            "{}",
            MSGS.get(r as usize).copied().unwrap_or("")
        ));
    }
    None
}

/// `config_found(d, a, p)`: `config_found_sm` without a submatch function (`<sys/device.h>`).
pub fn config_found(
    parent: &Device,
    aux: *mut c_void,
    print: Option<CfprintT>,
) -> Option<NonNull<Device>> {
    config_found_sm(parent, aux, print, None)
}

/// `config_rootfound`: as `config_found_sm`, but for root devices.
pub fn config_rootfound(rootname: &[u8], aux: *mut c_void) -> Option<NonNull<Device>> {
    if let Some(match_) = config_rootsearch(None, rootname, aux) {
        return Some(config_attach(None, match_, aux, None));
    }
    printf(format_args!(
        "root device {} not configured\n",
        Str(rootname)
    ));
    None
}

/// `config_attach`: attach a found device. Allocates memory for device variables.
pub fn config_attach(
    parent: Option<&Device>,
    match_: CfMatch,
    aux: *mut c_void,
    print: Option<CfprintT>,
) -> NonNull<Device> {
    mtx_enter(&AUTOCONF_ATTDET_MTX);
    while AUTOCONF_ATTDET.load(Ordering::Relaxed) < 0 {
        let _ = msleep_nsec(
            ptr::from_ref(&AUTOCONF_ATTDET),
            &AUTOCONF_ATTDET_MTX,
            PWAIT,
            "autoconf",
            INFSLP,
        );
    }
    AUTOCONF_ATTDET.fetch_add(1, Ordering::Relaxed);
    mtx_leave(&AUTOCONF_ATTDET_MTX);

    let (devp, cf) = match match_ {
        CfMatch::Softc(sc) => {
            let cf = sc.device().cfdata();
            (sc.into_raw(), cf)
        }
        CfMatch::Cfdata(cf) => (config_make_softc(parent, cf), cf),
    };
    // SAFETY: the softc was just made (or handed over by the search); it is freed only by
    // `config_detach`, which cannot run before this attach has finished (autoconf_attdet).
    let dev = unsafe { devp.as_ref() };

    let cd = cf.cf_driver;
    let ca = cf.cf_attach;
    let unit = dev.dv_unit.get();

    kassert!(!cd.cd_devs.get().is_null());
    kassert!(unit < cd.cd_ndevs.get());
    kassert!(cd.cd_dev(unit).is_none());
    set_cd_dev(cd, unit, Some(devp));

    // If this is a "STAR" device and we used the last unit, prepare for another one.
    if cf.cf_fstate.get() == FSTATE_STAR {
        if unit == i32::from(cf.cf_unit.get()) {
            cf.cf_unit.set(cf.cf_unit.get() + 1);
        }
    } else {
        cf.cf_fstate.set(FSTATE_FOUND);
    }

    // SAFETY: the device is in no list yet and stays in place until `config_detach` unlinks
    // it.
    unsafe { ALLDEVS.0.insert_tail(dev) };
    device_ref(dev);

    let name = xname(dev);
    match parent {
        None => {
            printf(format_args!("{} at root", Str(&name)));
        }
        Some(p) => {
            let pname = xname(p);
            printf(format_args!("{} at {}", Str(&name), Str(&pname)));
            if let Some(print) = print {
                let _ = print(aux, None);
            }
        }
    }

    // Before attaching, clobber any unfound devices that are otherwise identical, or bump the
    // unit number on all starred cfdata for this device.
    for c in cfdata() {
        if ptr::eq(c.cf_driver, cd) && i32::from(c.cf_unit.get()) == unit {
            if c.cf_fstate.get() == FSTATE_NOTFOUND {
                c.cf_fstate.set(FSTATE_FOUND);
            }
            if c.cf_fstate.get() == FSTATE_STAR {
                c.cf_unit.set(c.cf_unit.get() + 1);
            }
        }
    }
    device_register(dev, aux);
    (ca.ca_attach)(parent, dev, aux);
    config_process_deferred_children(dev);
    // NHOTPLUG > 0
    if !COLD.load(Ordering::Relaxed) {
        let _ = unported!("hotplug_device_attach (dev/hotplug.c)");
    }

    mtx_enter(&AUTOCONF_ATTDET_MTX);
    if AUTOCONF_ATTDET.fetch_sub(1, Ordering::Relaxed) == 1 {
        wakeup(ptr::from_ref(&AUTOCONF_ATTDET));
    }
    AUTOCONF_SERIAL.fetch_add(1, Ordering::Relaxed);
    mtx_leave(&AUTOCONF_ATTDET_MTX);
    devp
}

/// `cd->cd_devs[unit] = dev`.
fn set_cd_dev(cd: &Cfdriver, unit: i32, dev: Option<NonNull<Device>>) {
    kassert!(unit >= 0 && unit < cd.cd_ndevs.get());
    // SAFETY: `cd_devs` has `cd_ndevs` slots (`config_make_softc`), changed under the kernel
    // lock.
    unsafe { *cd.cd_devs.get().add(unit as usize) = dev };
}

/// `config_make_softc`: allocates the softc of a device `cf` describes below `parent`, picks
/// its unit, names it and makes room for it in the driver's `cd_devs`.
pub fn config_make_softc(parent: Option<&Device>, cf: &'static Cfdata) -> NonNull<Device> {
    let cd = cf.cf_driver;
    let ca = cf.cf_attach;
    if ca.ca_devsize < size_of::<Device>() {
        panic(format_args!("config_make_softc"));
    }

    // get memory for all device vars
    let Some(mem) = malloc(ca.ca_devsize, M_DEVBUF, M_NOWAIT | M_ZERO) else {
        panic(format_args!(
            "config_make_softc: allocation for device softc failed"
        ));
    };
    let devp = mem.cast::<Device>();
    kassert!(devp.as_ptr().is_aligned());
    // SAFETY: `ca_devsize` zeroed bytes, at least a `Device`, suitably aligned (malloc's
    // chunks are aligned to their power-of-two size); all-zero is a valid `Device`.
    let dev = unsafe { devp.as_ref() };

    dev.dv_class.set(cd.cd_class);
    dev.dv_cfdata.set(Some(cf));
    dev.dv_flags.set(DVF_ACTIVE); // always initially active

    // If this is a STAR device, search for a free unit number
    if cf.cf_fstate.get() == FSTATE_STAR {
        let mut unit = i32::from(cf.cf_starunit1);
        while unit < i32::from(cf.cf_unit.get()) {
            if cd.cd_ndevs.get() == 0 || unit >= cd.cd_ndevs.get() || cd.cd_dev(unit).is_none() {
                break;
            }
            unit += 1;
        }
        dev.dv_unit.set(unit);
    } else {
        dev.dv_unit.set(i32::from(cf.cf_unit.get()));
    }
    let unit = dev.dv_unit.get();

    // Build the device name into dv_xname.
    let mut name = [0u8; 16];
    if snprintf(&mut name, format_args!("{}{}", Str(cd.cd_name), unit)) >= name.len() {
        panic(format_args!("config_make_softc: device name too long"));
    }
    dev.dv_xname.set(name);
    dev.dv_parent.set(parent.map(NonNull::from));

    // put this device in the devices array
    if unit >= cd.cd_ndevs.get() {
        // Need to expand the array.
        let old = cd.cd_ndevs.get();
        let mut new = if old == 0 {
            (MINALLOCSIZE / size_of::<*const c_void>()) as i32
        } else {
            old * 2
        };
        while new <= unit {
            new *= 2;
        }
        let slot = size_of::<Option<NonNull<Device>>>();
        let Some(nsp) = mallocarray(new as usize, slot, M_DEVBUF, M_NOWAIT | M_ZERO) else {
            panic(format_args!(
                "config_make_softc: {}ing dev array",
                if old != 0 { "expand" } else { "creat" }
            ));
        };
        let nsp = nsp.cast::<Option<NonNull<Device>>>();
        if let Some(osp) = NonNull::new(cd.cd_devs.get()) {
            // SAFETY: the old array has `old` slots, the new one `new > old` zeroed ones;
            // distinct allocations.
            unsafe { ptr::copy_nonoverlapping(osp.as_ptr(), nsp.as_ptr(), old as usize) };
            free(osp.cast(), M_DEVBUF, old as usize * slot);
        }
        cd.cd_devs.set(nsp.as_ptr());
        cd.cd_ndevs.set(new);
    }
    if cd.cd_dev(unit).is_some() {
        panic(format_args!(
            "config_make_softc: duplicate {}",
            Str(&dev.dv_xname.get())
        ));
    }

    dev.dv_ref.store(1, Ordering::Relaxed);

    devp
}

/// `config_detach`: detach a device. Optionally forced (e.g. because of hardware removal) and
/// quiet. Returns `Ok` if successful, the error otherwise.
///
/// Note that this code wants to be run from a process context, so that the detach can sleep to
/// allow processes which have a device open to run and unwind their stacks.
///
/// # Safety
///
/// `dev` is an attached device. On success it has been freed (unless someone else holds a
/// reference from `device_lookup`/`device_ref`), so the caller must not use it again.
pub unsafe fn config_detach(dev: NonNull<Device>, flags: i32) -> Result<(), Errno> {
    mtx_enter(&AUTOCONF_ATTDET_MTX);
    while AUTOCONF_ATTDET.load(Ordering::Relaxed) > 0 {
        let _ = msleep_nsec(
            ptr::from_ref(&AUTOCONF_ATTDET),
            &AUTOCONF_ATTDET_MTX,
            PWAIT,
            "autoconf",
            INFSLP,
        );
    }
    AUTOCONF_ATTDET.fetch_sub(1, Ordering::Relaxed);
    mtx_leave(&AUTOCONF_ATTDET_MTX);

    // SAFETY: the caller's guarantee; the device stays allocated until the last
    // `device_unref` below.
    let d = unsafe { dev.as_ref() };
    let devname = xname(d);

    let cf = d.cfdata();
    #[cfg(feature = "diagnostic")]
    if cf.cf_fstate.get() != FSTATE_FOUND && cf.cf_fstate.get() != FSTATE_STAR {
        panic(format_args!("config_detach: bad device fstate"));
    }
    let ca = cf.cf_attach;
    let cd = cf.cf_driver;

    let rv = 'done: {
        // Ensure the device is deactivated. If the device has an activation entry point and
        // DVF_ACTIVE is still set, the device is busy, and the detach fails.
        let mut rv = config_deactivate(d);

        // Try to detach the device. If that's not possible, then we either panic() (for the
        // forced but failed case), or return an error.
        if rv.is_ok() {
            rv = match ca.ca_detach {
                Some(detach) => detach(d, flags),
                None => Err(Errno::EOPNOTSUPP),
            };
        }
        if let Err(e) = rv {
            if flags & DETACH_FORCE == 0 {
                break 'done rv;
            }
            panic(format_args!(
                "config_detach: forced detach of {} failed ({})",
                Str(&devname),
                e as i32
            ));
        }

        // The device has now been successfully detached.

        #[cfg(feature = "diagnostic")]
        {
            // Sanity: If you're successfully detached, you should have no children. (Note
            // that because children must be attached after parents, we only need to search
            // the latter part of the list.)
            let mut found = false;
            let mut c = TailqHead::<DeviceList>::next(d);
            while let Some(child) = c {
                if child.dv_parent.get() == Some(dev) {
                    printf(format_args!(
                        "config_detach: {} attached at {}\n",
                        Str(&xname(child)),
                        Str(&devname)
                    ));
                    found = true;
                }
                c = TailqHead::<DeviceList>::next(child);
            }
            if found {
                panic(format_args!(
                    "config_detach: detached device ({}) has children",
                    Str(&devname)
                ));
            }
        }

        // Mark cfdata to show that the unit can be reused, if possible. Note that we can only
        // re-use a starred unit number if the unit being detached had the last assigned unit
        // number.
        let unit = d.dv_unit.get();
        for c in cfdata() {
            if ptr::eq(c.cf_driver, cd) {
                if c.cf_fstate.get() == FSTATE_FOUND && i32::from(c.cf_unit.get()) == unit {
                    c.cf_fstate.set(FSTATE_NOTFOUND);
                }
                if c.cf_fstate.get() == FSTATE_STAR && i32::from(c.cf_unit.get()) == unit + 1 {
                    c.cf_unit.set(c.cf_unit.get() - 1);
                }
            }
        }

        // Unlink from device list.
        // SAFETY: an attached device is in `alldevs`.
        unsafe { ALLDEVS.0.remove(d) };
        // SAFETY: `config_attach` took this reference; `config_make_softc`'s is still held.
        unsafe { device_unref(dev) };

        // Remove from cfdriver's array, tell the world, and free softc.
        set_cd_dev(cd, unit, None);
        if flags & DETACH_QUIET == 0 {
            printf(format_args!("{} detached\n", Str(&devname)));
        }

        // SAFETY: the last reference autoconfiguration holds; `d` is not used after this.
        unsafe { device_unref(dev) };

        // If the device now has no units in use, deallocate its softc array.
        let ndevs = cd.cd_ndevs.get();
        if (0..ndevs).all(|i| cd.cd_dev(i).is_none()) {
            // nothing found; deallocate
            if let Some(sp) = NonNull::new(cd.cd_devs.get()) {
                free(
                    sp.cast(),
                    M_DEVBUF,
                    ndevs as usize * size_of::<Option<NonNull<Device>>>(),
                );
            }
            cd.cd_devs.set(ptr::null_mut());
            cd.cd_ndevs.set(0);
            // cf->cf_unit = 0: see the module's deviations.
        }

        // NHOTPLUG > 0
        if !COLD.load(Ordering::Relaxed) {
            let _ = unported!("hotplug_device_detach (dev/hotplug.c)");
        }

        // Return success.
        Ok(())
    };

    mtx_enter(&AUTOCONF_ATTDET_MTX);
    if AUTOCONF_ATTDET.fetch_add(1, Ordering::Relaxed) == -1 {
        wakeup(ptr::from_ref(&AUTOCONF_ATTDET));
    }
    AUTOCONF_SERIAL.fetch_add(1, Ordering::Relaxed);
    mtx_leave(&AUTOCONF_ATTDET_MTX);
    rv
}

/// `config_deactivate`: clears `DVF_ACTIVE` and tells the device (`DVACT_DEACTIVATE`); the
/// flag comes back if the device refuses.
pub fn config_deactivate(dev: &Device) -> Result<(), Errno> {
    let oflags = dev.dv_flags.get();
    let mut rv = Ok(());

    if oflags & DVF_ACTIVE != 0 {
        dev.dv_flags.set(oflags & !DVF_ACTIVE);
        rv = config_suspend(dev, DVACT_DEACTIVATE);
        if rv.is_err() {
            dev.dv_flags.set(oflags);
        }
    }
    rv
}

/// `config_defer`: defer the configuration of the specified device until all of its parent's
/// devices have been attached.
pub fn config_defer(dev: &Device, func: fn(&Device)) {
    if dev.dv_parent.get().is_none() {
        panic(format_args!(
            "config_defer: can't defer config of a root device"
        ));
    }

    #[cfg(feature = "diagnostic")]
    for dc in DEFERRED_CONFIG_QUEUE.0.iter() {
        if ptr::eq(dc.dc_dev.as_ptr(), dev) {
            panic(format_args!("config_defer: deferred twice"));
        }
    }

    let Some(mem) = malloc(size_of::<DeferredConfig>(), M_DEVBUF, M_NOWAIT) else {
        panic(format_args!("config_defer: can't allocate defer structure"));
    };
    let dc = mem.cast::<DeferredConfig>();
    // SAFETY: a fresh allocation the size of a `DeferredConfig`, aligned as malloc's chunks
    // are; written before use, linked until `config_process_deferred_children` frees it.
    unsafe {
        dc.write(DeferredConfig {
            dc_queue: TailqEntry::new(),
            dc_dev: NonNull::from(dev),
            dc_func: func,
        });
        DEFERRED_CONFIG_QUEUE.0.insert_tail(dc.as_ref());
    }
    config_pending_incr();
}

/// `rootvp`: the vnode of the root file system. There is no VFS yet (M10), so the root is
/// never mounted and this is `NULL`.
fn rootvp() -> Option<NonNull<c_void>> {
    None
}

/// `config_mountroot`: defer the configuration of the specified device until after root file
/// system is mounted.
pub fn config_mountroot(dev: &Device, func: fn(&Device)) {
    // No need to defer if root file system is already mounted.
    if rootvp().is_some() {
        func(dev);
        return;
    }

    #[cfg(feature = "diagnostic")]
    for dc in MOUNTROOT_CONFIG_QUEUE.0.iter() {
        if ptr::eq(dc.dc_dev.as_ptr(), dev) {
            panic(format_args!("config_mountroot: deferred twice"));
        }
    }

    let Some(mem) = malloc(size_of::<DeferredConfig>(), M_DEVBUF, M_NOWAIT) else {
        panic(format_args!(
            "config_mountroot: can't allocate defer structure"
        ));
    };
    let dc = mem.cast::<DeferredConfig>();
    // SAFETY: as in `config_defer`; freed by `config_process_deferred_mountroot`.
    unsafe {
        dc.write(DeferredConfig {
            dc_queue: TailqEntry::new(),
            dc_dev: NonNull::from(dev),
            dc_func: func,
        });
        MOUNTROOT_CONFIG_QUEUE.0.insert_tail(dc.as_ref());
    }
}

/// `config_process_deferred_children`: process the deferred configuration queue for a
/// device.
pub fn config_process_deferred_children(parent: &Device) {
    for dc in DEFERRED_CONFIG_QUEUE.0.iter() {
        let devp = dc.dc_dev;
        let func = dc.dc_func;
        // SAFETY: a deferred device stays attached until its configuration has run.
        let dev = unsafe { devp.as_ref() };
        if dev.dv_parent.get() == Some(NonNull::from(parent)) {
            // SAFETY: `dc` is in this queue; the iterator has already read its successor.
            unsafe { DEFERRED_CONFIG_QUEUE.0.remove(dc) };
            func(dev);
            free(
                NonNull::from(dc).cast(),
                M_DEVBUF,
                size_of::<DeferredConfig>(),
            );
            config_pending_decr();
        }
    }
}

/// `config_process_deferred_mountroot`: process the deferred configuration queue after the
/// root file system is mounted.
pub fn config_process_deferred_mountroot() {
    while let Some(dc) = MOUNTROOT_CONFIG_QUEUE.0.first() {
        let devp = dc.dc_dev;
        let func = dc.dc_func;
        // SAFETY: `dc` is the queue's first element.
        unsafe { MOUNTROOT_CONFIG_QUEUE.0.remove(dc) };
        // SAFETY: as in `config_process_deferred_children`.
        func(unsafe { devp.as_ref() });
        free(
            NonNull::from(dc).cast(),
            M_DEVBUF,
            size_of::<DeferredConfig>(),
        );
    }
}

/// `config_pending_incr`: manipulate the `config_pending` semaphore.
pub fn config_pending_incr() {
    CONFIG_PENDING.fetch_add(1, Ordering::Relaxed);
}

/// `config_pending_decr`.
pub fn config_pending_decr() {
    #[cfg(feature = "diagnostic")]
    if CONFIG_PENDING.load(Ordering::Relaxed) == 0 {
        panic(format_args!("config_pending_decr: config_pending == 0"));
    }
    if CONFIG_PENDING.fetch_sub(1, Ordering::Relaxed) == 1 {
        wakeup(ptr::from_ref(&CONFIG_PENDING));
    }
}

/// `config_detach_children`: detaches every child of `parent`, last attached first.
///
/// The `config_detach` routine may sleep, meaning devices may be added to the queue. However,
/// all devices will be added to the tail of the queue, the queue won't be re-organized, and
/// the subtree of parent here should be locked for purposes of adding/removing children.
///
/// Note that we can not afford trying to walk the device list once - our "next" device might
/// be a child of the device we are about to detach, so it would disappear. Just play it safe
/// and restart from the parent.
pub fn config_detach_children(parent: &Device, flags: i32) -> Result<(), Errno> {
    let parentp = NonNull::from(parent);
    let mut dev = ALLDEVS.0.last();
    while let Some(d) = dev {
        if d.dv_parent.get() == Some(parentp) {
            // SAFETY: `d` is attached (it is in `alldevs`) and not used after the detach.
            unsafe { config_detach(NonNull::from(d), flags)? };
            dev = ALLDEVS.0.last();
        } else {
            dev = ALLDEVS.0.prev(d);
        }
    }
    Ok(())
}

/// `config_suspend`: hands `act` to the device's `ca_activate`, or to its children when it
/// has none.
pub fn config_suspend(dev: &Device, act: i32) -> Result<(), Errno> {
    let ca = dev.cfdata().cf_attach;

    device_ref(dev);
    let r = match ca.ca_activate {
        Some(activate) => activate(dev, act),
        None => config_activate_children(dev, act),
    };
    // SAFETY: the caller's reference keeps `dv_ref` above the one taken here, so this does not
    // free the device.
    unsafe { device_unref(NonNull::from(dev)) };
    r
}

/// `config_suspend_all`: `config_suspend` on the `mpath` and `mainbus` trees, in the order
/// that suits `act`.
pub fn config_suspend_all(act: i32) -> Result<(), Errno> {
    let mainbus = device_mainbus();
    let mpath = device_mpath();
    // SAFETY: the root devices are never detached.
    let mainbus = mainbus.map(|d| unsafe { d.as_ref() });
    // SAFETY: as above.
    let mpath = mpath.map(|d| unsafe { d.as_ref() });
    let mut rv = Ok(());

    match act {
        DVACT_QUIESCE | DVACT_SUSPEND | DVACT_POWERDOWN => {
            if let Some(mpath) = mpath {
                config_suspend(mpath, act)?;
            }
            if let Some(mainbus) = mainbus {
                rv = config_suspend(mainbus, act);
            }
        }
        DVACT_RESUME | DVACT_WAKEUP => {
            if let Some(mainbus) = mainbus {
                config_suspend(mainbus, act)?;
            }
            if let Some(mpath) = mpath {
                rv = config_suspend(mpath, act);
            }
        }
        _ => {}
    }

    rv
}

/// `config_activate_children`: call the `ca_activate` for each of our children, letting each
/// decide whether they wish to do the same for their children and more.
pub fn config_activate_children(parent: &Device, act: i32) -> Result<(), Errno> {
    let parentp = NonNull::from(parent);
    let mut rv = Ok(());

    let mut d = TailqHead::<DeviceList>::next(parent);
    while let Some(dv) = d {
        if dv.dv_parent.get() != Some(parentp) {
            d = TailqHead::<DeviceList>::next(dv);
            continue;
        }
        match act {
            DVACT_QUIESCE | DVACT_SUSPEND | DVACT_RESUME | DVACT_WAKEUP | DVACT_POWERDOWN => {
                rv = config_suspend(dv, act);
            }
            DVACT_DEACTIVATE => rv = config_deactivate(dv),
            _ => {}
        }
        if rv.is_ok() {
            d = TailqHead::<DeviceList>::next(dv);
            continue;
        }

        // Found a device that refuses the action. If we were being asked to suspend, we can
        // try to resume all previous devices.
        #[cfg(feature = "diagnostic")]
        printf(format_args!(
            "config_activate_children: device {} failed {}\n",
            Str(&xname(dv)),
            act
        ));
        if act == DVACT_RESUME {
            printf(format_args!("failing resume cannot be handled\n"));
        }
        if act == DVACT_POWERDOWN {
            return rv;
        }
        if act != DVACT_SUSPEND {
            return rv;
        }

        let mut p = ALLDEVS.0.prev(dv);
        while let Some(pd) = p {
            if ptr::eq(pd, parent) {
                break;
            }
            if pd.dv_parent.get() == Some(parentp) {
                printf(format_args!("resume {}\n", Str(&xname(pd))));
                let _ = config_suspend(pd, DVACT_RESUME);
            }
            p = ALLDEVS.0.prev(pd);
        }
        return rv;
    }
    rv
}

/// `device_lookup`: lookup a device in the cfdriver device array. Does not return a device if
/// it is not active.
///
/// Increments ref count on the device by one, reflecting the new reference created on the
/// stack; the caller gives it back with `device_unref`.
///
/// Context: process only
pub fn device_lookup(cd: &Cfdriver, unit: i32) -> Option<NonNull<Device>> {
    let dv = cd.cd_dev(unit)?;
    // SAFETY: a unit in `cd_devs` is attached, hence allocated.
    let d = unsafe { dv.as_ref() };

    if d.dv_flags.get() & DVF_ACTIVE == 0 {
        return None;
    }

    device_ref(d);
    Some(dv)
}

/// `device_mainbus`: the root bus, once it is attached.
pub fn device_mainbus() -> Option<NonNull<Device>> {
    let cd = mainbus_cd();
    if cd.cd_ndevs.get() < 1 {
        return None;
    }
    cd.cd_dev(0)
}

/// `device_mpath`: the `mpath(4)` root; `NMPATH` is 0 (see the module's deviations).
pub fn device_mpath() -> Option<NonNull<Device>> {
    None
}

/// `device_ref`: increments the ref count on the device structure. The device structure is
/// freed when the ref count hits 0.
///
/// Context: process or interrupt
pub fn device_ref(dv: &Device) {
    dv.dv_ref.fetch_add(1, Ordering::Relaxed);
}

/// `device_unref`: decrement the ref count on the device structure; frees the structure when
/// the ref count hits zero.
///
/// Context: process or interrupt
///
/// # Safety
///
/// `dv` is a live device on which the caller holds a reference (from `config_make_softc`,
/// `device_ref` or `device_lookup`), which this gives up: if it was the last, the device is
/// freed and must not be used again.
pub unsafe fn device_unref(dv: NonNull<Device>) {
    // SAFETY: the caller's reference keeps the device allocated until the decrement.
    let d = unsafe { dv.as_ref() };
    if d.dv_ref.fetch_sub(1, Ordering::AcqRel) == 1 {
        let ca = d.cfdata().cf_attach;
        free(dv.cast(), M_DEVBUF, ca.ca_devsize);
    }
}

/// Host tests: no attach in progress after `setup_real_memory`. A test that panicked inside
/// `config_attach` leaves `autoconf_attdet` raised, and every later `config_detach` would
/// sleep on it for ever: the failure is reported by that test, not as a hung test run.
#[cfg(test)]
pub(crate) fn autoconf_test_reset() {
    AUTOCONF_ATTDET.store(0, Ordering::Relaxed);
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for autoconfiguration over a test `ioconf`: a root bus `troot0` and a starred
    // child driver `tchild*` below it, attached, looked up, deferred and detached.

    use std::boxed::Box;
    use std::sync::MutexGuard;
    use std::sync::atomic::AtomicUsize;
    use std::vec::Vec;
    use std::{assert, assert_eq};

    use super::*;
    use crate::kern::subr_pool::tests::setup_real_memory;
    use crate::machine::Machine;
    use crate::sys::device::{CD_INDIRECT, Cfattach, DV_DULL, QUIET, UNCONF};

    /// What the child's attach arguments say: whether to match, and at which level.
    struct ChildArgs {
        pri: i32,
    }

    static ROOT_ATTACHED: AtomicUsize = AtomicUsize::new(0);
    static CHILD_ATTACHED: AtomicUsize = AtomicUsize::new(0);
    static PRINTED: AtomicUsize = AtomicUsize::new(0);
    static DEFERRED_RAN: AtomicUsize = AtomicUsize::new(0);
    static ACTIVATE_FAIL_UNIT: AtomicI32 = AtomicI32::new(-1);
    static RESUMED: AtomicUsize = AtomicUsize::new(0);

    fn root_match(parent: Option<&Device>, _m: &CfMatch, _aux: *mut c_void) -> i32 {
        assert!(parent.is_none());
        1
    }

    fn root_attach(parent: Option<&Device>, _self: &Device, _aux: *mut c_void) {
        assert!(parent.is_none());
        ROOT_ATTACHED.fetch_add(1, Ordering::Relaxed);
    }

    fn child_match(parent: Option<&Device>, m: &CfMatch, aux: *mut c_void) -> i32 {
        assert!(parent.is_some());
        assert_eq!(m.cfdata().cf_driver.cd_name, b"tchild");
        // SAFETY: the tests always pass a `ChildArgs` as the child's aux.
        unsafe { (*aux.cast::<ChildArgs>()).pri }
    }

    fn child_attach(parent: Option<&Device>, self_: &Device, _aux: *mut c_void) {
        assert!(parent.is_some());
        assert!(ptr::eq(
            self_.parent().expect("a parent"),
            parent.expect("a parent")
        ));
        CHILD_ATTACHED.fetch_add(1, Ordering::Relaxed);
    }

    fn child_detach(_dev: &Device, _flags: i32) -> Result<(), Errno> {
        Ok(())
    }

    fn child_activate(dev: &Device, act: i32) -> Result<(), Errno> {
        if dev.dv_unit.get() == ACTIVATE_FAIL_UNIT.load(Ordering::Relaxed) && act == DVACT_SUSPEND {
            return Err(Errno::EBUSY);
        }
        if act == DVACT_RESUME {
            RESUMED.fetch_add(1, Ordering::Relaxed);
        }
        Ok(())
    }

    fn child_print(_aux: *mut c_void, pnp: Option<&[u8]>) -> i32 {
        PRINTED.fetch_add(1, Ordering::Relaxed);
        if pnp.is_some() { UNCONF } else { QUIET }
    }

    fn deferred(dev: &Device) {
        assert_eq!(dev.cfdata().cf_driver.cd_name, b"tchild");
        DEFERRED_RAN.fetch_add(1, Ordering::Relaxed);
    }

    /// A fresh test `ioconf` (state lives in the tables, so every test gets its own) with
    /// `troot0 at root` and `tchild* at troot0`; `root_mode` is the root driver's `cd_mode`.
    fn ioconf(root_mode: i32) -> (&'static [Cfdata], &'static Cfdriver, &'static Cfdriver) {
        let root_ca: &'static Cfattach = Box::leak(Box::new(Cfattach {
            ca_devsize: size_of::<Device>(),
            ca_match: Some(root_match),
            ca_attach: root_attach,
            ca_detach: None,
            ca_activate: None,
        }));
        let root_cd: &'static Cfdriver =
            Box::leak(Box::new(Cfdriver::new(b"troot", DV_DULL, root_mode)));
        let child_ca: &'static Cfattach = Box::leak(Box::new(Cfattach {
            ca_devsize: size_of::<Device>() + 64,
            ca_match: Some(child_match),
            ca_attach: child_attach,
            ca_detach: Some(child_detach),
            ca_activate: Some(child_activate),
        }));
        let child_cd: &'static Cfdriver = Box::leak(Box::new(Cfdriver::new(b"tchild", DV_DULL, 0)));
        let table: &'static [Cfdata] = Box::leak(Box::new([
            Cfdata::new(root_ca, root_cd, 0, FSTATE_NOTFOUND, &[], 0, &[], 0, 0),
            Cfdata::new(child_ca, child_cd, 0, FSTATE_STAR, &[], 0, &[0], 0, 0),
        ]));
        (table, root_cd, child_cd)
    }

    /// Real memory for malloc(9), a fresh `ioconf` installed, the counters reset.
    fn setup(
        root_mode: i32,
    ) -> (
        MutexGuard<'static, ()>,
        &'static [Cfdata],
        &'static Cfdriver,
        &'static Cfdriver,
    ) {
        let guard = setup_real_memory();
        let (table, root_cd, child_cd) = ioconf(root_mode);
        // SAFETY: `setup_real_memory`'s lock serialises the tests.
        unsafe { Machine::set_ioconf(table, &[0]) };
        config_init();
        for c in [
            &ROOT_ATTACHED,
            &CHILD_ATTACHED,
            &PRINTED,
            &DEFERRED_RAN,
            &RESUMED,
        ] {
            c.store(0, Ordering::Relaxed);
        }
        ACTIVATE_FAIL_UNIT.store(-1, Ordering::Relaxed);
        (guard, table, root_cd, child_cd)
    }

    fn name(dev: NonNull<Device>) -> Vec<u8> {
        // SAFETY: the tests only name attached devices.
        let n = unsafe { dev.as_ref() }.dv_xname.get();
        cstr(&n).to_vec()
    }

    fn aux(args: &mut ChildArgs) -> *mut c_void {
        ptr::from_mut(args).cast()
    }

    fn alldevs() -> Vec<Vec<u8>> {
        ALLDEVS
            .0
            .iter()
            .map(|d| cstr(&d.dv_xname.get()).to_vec())
            .collect()
    }

    #[test]
    fn rootfound_attaches_the_named_root() {
        let (_g, table, root_cd, _) = setup(0);
        assert!(config_rootfound(b"nosuchroot", ptr::null_mut()).is_none());
        let root = config_rootfound(b"troot", ptr::null_mut()).expect("troot0");
        assert_eq!(name(root), b"troot0");
        assert_eq!(ROOT_ATTACHED.load(Ordering::Relaxed), 1);
        assert_eq!(table[0].cf_fstate.get(), FSTATE_FOUND);
        assert_eq!(root_cd.cd_dev(0), Some(root));
        assert_eq!(alldevs(), [b"troot0".to_vec()]);
        // SAFETY: just attached.
        let r = unsafe { root.as_ref() };
        assert_eq!(
            r.dv_ref.load(Ordering::Relaxed),
            2,
            "config_make_softc's and config_attach's"
        );
        assert!(r.dv_flags.get() & DVF_ACTIVE != 0);
        assert!(r.parent().is_none());
    }

    #[test]
    fn starred_children_take_increasing_units() {
        let (_g, table, _, child_cd) = setup(0);
        let root = config_rootfound(b"troot", ptr::null_mut()).expect("troot0");
        // SAFETY: attached above.
        let r = unsafe { root.as_ref() };
        let mut yes = ChildArgs { pri: 1 };
        let a = config_found(r, aux(&mut yes), Some(child_print)).expect("tchild0");
        let b = config_found(r, aux(&mut yes), Some(child_print)).expect("tchild1");
        assert_eq!(name(a), b"tchild0");
        assert_eq!(name(b), b"tchild1");
        assert_eq!(table[1].cf_unit.get(), 2);
        assert_eq!(table[1].cf_fstate.get(), FSTATE_STAR);
        assert_eq!(CHILD_ATTACHED.load(Ordering::Relaxed), 2);
        assert_eq!(
            PRINTED.load(Ordering::Relaxed),
            2,
            "once each, with pnp == NULL"
        );
        assert_eq!(child_cd.cd_dev(1), Some(b));
        assert_eq!(alldevs().len(), 3);
    }

    #[test]
    fn an_unmatched_device_is_printed_not_configured() {
        let (_g, _, _, _) = setup(0);
        let root = config_rootfound(b"troot", ptr::null_mut()).expect("troot0");
        // SAFETY: attached above.
        let r = unsafe { root.as_ref() };
        let mut no = ChildArgs { pri: 0 };
        assert!(config_found(r, aux(&mut no), Some(child_print)).is_none());
        assert_eq!(PRINTED.load(Ordering::Relaxed), 1);
        assert_eq!(CHILD_ATTACHED.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn indirect_parents_match_against_softcs() {
        let (_g, _, _, child_cd) = setup(CD_INDIRECT);
        let root = config_rootfound(b"troot", ptr::null_mut()).expect("troot0");
        // SAFETY: attached above.
        let r = unsafe { root.as_ref() };
        let mut yes = ChildArgs { pri: 3 };
        let m = config_search(None, r, aux(&mut yes)).expect("a match");
        assert!(matches!(m, CfMatch::Softc(_)));
        let child = config_attach(Some(r), m, aux(&mut yes), None);
        assert_eq!(name(child), b"tchild0");
        assert_eq!(child_cd.cd_dev(0), Some(child));
        // A losing softc is freed: nothing is left behind in cd_devs.
        let mut no = ChildArgs { pri: 0 };
        assert!(config_search(None, r, aux(&mut no)).is_none());
        assert!(child_cd.cd_dev(1).is_none());
    }

    #[test]
    fn detach_frees_the_unit_for_reuse() {
        let (_g, table, _, child_cd) = setup(0);
        let root = config_rootfound(b"troot", ptr::null_mut()).expect("troot0");
        // SAFETY: attached above.
        let r = unsafe { root.as_ref() };
        let mut yes = ChildArgs { pri: 1 };
        let a = config_found(r, aux(&mut yes), None).expect("tchild0");
        let b = config_found(r, aux(&mut yes), None).expect("tchild1");
        let serial = AUTOCONF_SERIAL.load(Ordering::Relaxed);

        // SAFETY: attached, not used afterwards.
        assert_eq!(unsafe { config_detach(b, DETACH_QUIET) }, Ok(()));
        assert_eq!(table[1].cf_unit.get(), 1, "the last starred unit is reused");
        assert!(child_cd.cd_dev(1).is_none());
        assert_eq!(AUTOCONF_SERIAL.load(Ordering::Relaxed), serial + 1);
        let c = config_found(r, aux(&mut yes), None).expect("tchild1 again");
        assert_eq!(name(c), b"tchild1");

        // The root has no ca_detach: it fails after the deactivation, which the C keeps (and
        // which reached the children through config_activate_children).
        // SAFETY: attached; on failure it stays attached.
        let rv = unsafe { config_detach(root, DETACH_QUIET) };
        assert_eq!(rv, Err(Errno::EOPNOTSUPP));
        assert!(r.dv_flags.get() & DVF_ACTIVE == 0);
        // SAFETY: still attached.
        assert!(unsafe { a.as_ref() }.dv_flags.get() & DVF_ACTIVE == 0);

        assert_eq!(config_detach_children(r, DETACH_QUIET), Ok(()));
        assert_eq!(alldevs(), [b"troot0".to_vec()]);
        assert_eq!(
            child_cd.cd_ndevs.get(),
            0,
            "the empty cd_devs array is freed"
        );
    }

    #[test]
    fn device_lookup_takes_a_reference_on_active_devices() {
        let (_g, _, _, child_cd) = setup(0);
        let root = config_rootfound(b"troot", ptr::null_mut()).expect("troot0");
        // SAFETY: attached above.
        let r = unsafe { root.as_ref() };
        let mut yes = ChildArgs { pri: 1 };
        let a = config_found(r, aux(&mut yes), None).expect("tchild0");
        // SAFETY: attached above.
        let d = unsafe { a.as_ref() };

        assert!(device_lookup(child_cd, 1).is_none());
        assert!(device_lookup(child_cd, -1).is_none());
        assert_eq!(device_lookup(child_cd, 0), Some(a));
        assert_eq!(d.dv_ref.load(Ordering::Relaxed), 3);
        // SAFETY: gives back the lookup's reference.
        unsafe { device_unref(a) };
        assert_eq!(d.dv_ref.load(Ordering::Relaxed), 2);

        assert_eq!(config_deactivate(d), Ok(()));
        assert!(device_lookup(child_cd, 0).is_none(), "inactive");
    }

    #[test]
    fn deferred_configuration_runs_after_the_parent_attach() {
        let (_g, _, _, _) = setup(0);
        let root = config_rootfound(b"troot", ptr::null_mut()).expect("troot0");
        // SAFETY: attached above.
        let r = unsafe { root.as_ref() };
        let mut yes = ChildArgs { pri: 1 };
        let a = config_found(r, aux(&mut yes), None).expect("tchild0");
        // SAFETY: attached above.
        let d = unsafe { a.as_ref() };

        config_defer(d, deferred);
        assert_eq!(CONFIG_PENDING.load(Ordering::Relaxed), 1);
        config_process_deferred_children(d);
        assert_eq!(
            DEFERRED_RAN.load(Ordering::Relaxed),
            0,
            "not a child of itself"
        );
        config_process_deferred_children(r);
        assert_eq!(DEFERRED_RAN.load(Ordering::Relaxed), 1);
        assert_eq!(CONFIG_PENDING.load(Ordering::Relaxed), 0);

        config_mountroot(d, deferred);
        assert_eq!(
            DEFERRED_RAN.load(Ordering::Relaxed),
            1,
            "no root file system yet"
        );
        config_process_deferred_mountroot();
        assert_eq!(DEFERRED_RAN.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn a_refused_suspend_resumes_the_earlier_siblings() {
        let (_g, _, _, _) = setup(0);
        let root = config_rootfound(b"troot", ptr::null_mut()).expect("troot0");
        // SAFETY: attached above.
        let r = unsafe { root.as_ref() };
        let mut yes = ChildArgs { pri: 1 };
        for _ in 0..3 {
            config_found(r, aux(&mut yes), None).expect("a child");
        }
        ACTIVATE_FAIL_UNIT.store(2, Ordering::Relaxed);
        assert_eq!(config_suspend(r, DVACT_SUSPEND), Err(Errno::EBUSY));
        assert_eq!(
            RESUMED.load(Ordering::Relaxed),
            2,
            "tchild1 and tchild0 resumed"
        );
        ACTIVATE_FAIL_UNIT.store(-1, Ordering::Relaxed);
        assert_eq!(config_suspend(r, DVACT_SUSPEND), Ok(()));
    }
}
/* </TESTS> */
