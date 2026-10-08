/*	$OpenBSD: plrtc.c,v 1.4 2022/10/17 19:09:46 kettenis Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2015 Jonathan Gray <jsg@openbsd.org>
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
//! The ARM PrimeCell PL031 real-time clock on the device tree: `dev/fdt/plrtc.c`.
//!
//! Upstream: sys/dev/fdt/plrtc.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The attach arguments are the machine's `struct fdt_attach_args`
//!   (`machine::fdt::FdtAttachArgs`).
//! - The chip handle is malloc'd as in C and never freed (the device is never detached).

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr;

use crate::dev::clock_subr::TodrChipHandle;
use crate::dev::ofw::fdt::OF_is_compatible;
use crate::kern::kern_malloc::malloc;
use crate::kern::kern_time::todr_attach;
use crate::kern::subr_prf::{panic, printf};
use crate::machine::bus::{
    BusSpaceHandle, BusSpaceTag, bus_space_map, bus_space_read_4, bus_space_write_4,
};
use crate::machine::fdt::FdtAttachArgs;
use crate::sys::device::{CfMatch, Cfattach, Cfdriver, DV_DULL, Device, Softc};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_DEVBUF, M_NOWAIT, M_ZERO};
use crate::sys::time::Timeval;

/// `RTCDR`: data register.
const RTCDR: usize = 0x00;
/// `RTCMR`: match register.
#[allow(dead_code)] // the C defines it; the driver does not use it
const RTCMR: usize = 0x04;
/// `RTCLR`: load register.
const RTCLR: usize = 0x08;
/// `RTCCR`: control register.
const RTCCR: usize = 0x0c;

/// `RTCCR_START`.
const RTCCR_START: u32 = 1 << 0;

/// `struct plrtc_softc`.
#[repr(C)]
pub struct PlrtcSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `sc_iot`.
    pub sc_iot: Cell<Option<BusSpaceTag>>,
    /// `sc_ioh`.
    pub sc_ioh: Cell<Option<BusSpaceHandle>>,
}

// SAFETY: `#[repr(C)]` with the `struct device` first, as `config_make_softc` requires.
unsafe impl Softc for PlrtcSoftc {}

/// `plrtc_ca`.
pub static PLRTC_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<PlrtcSoftc>(),
    ca_match: Some(plrtc_match),
    ca_attach: plrtc_attach,
    ca_detach: None,
    ca_activate: None,
};

/// `plrtc_cd`.
pub static PLRTC_CD: Cfdriver = Cfdriver::new(b"plrtc", DV_DULL, 0);

/// The softc a chip handle's cookie names.
fn handle_softc(handle: &TodrChipHandle) -> Option<(BusSpaceTag, BusSpaceHandle)> {
    // SAFETY: plrtc_attach sets the cookie to its softc, which lives as long as the device.
    let sc = unsafe { &*handle.cookie.cast::<PlrtcSoftc>() };
    Some((sc.sc_iot.get()?, sc.sc_ioh.get()?))
}

/// `plrtc_gettime`: the counter is the time in seconds.
pub fn plrtc_gettime(handle: &TodrChipHandle, tv: &mut Timeval) -> Result<(), Errno> {
    let (iot, ioh) = handle_softc(handle).ok_or(Errno::ENXIO)?;
    let tod = bus_space_read_4(iot, ioh, RTCDR);

    tv.tv_sec = i64::from(tod);
    tv.tv_usec = 0;

    Ok(())
}

/// `plrtc_settime`: load the counter.
pub fn plrtc_settime(handle: &TodrChipHandle, tv: &mut Timeval) -> Result<(), Errno> {
    let (iot, ioh) = handle_softc(handle).ok_or(Errno::ENXIO)?;
    bus_space_write_4(iot, ioh, RTCLR, tv.tv_sec as u32);

    Ok(())
}

/// `plrtc_match`: a node compatible with `arm,pl031`.
pub fn plrtc_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: the device-tree buses hand their children `struct fdt_attach_args`.
    let faa = unsafe { &*aux.cast::<FdtAttachArgs<'_>>() };

    i32::from(OF_is_compatible(faa.fa_node, b"arm,pl031"))
}

/// `plrtc_attach`: map the registers, hand the chip to `todr_attach` and start it.
pub fn plrtc_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: as in `plrtc_match`.
    let faa = unsafe { &*aux.cast::<FdtAttachArgs<'_>>() };
    // SAFETY: `plrtc_ca` makes `PlrtcSoftc`s; `config_make_softc`'s allocation lives as
    // long as the device, which is never detached.
    let sc: &'static PlrtcSoftc = unsafe { &*ptr::from_ref(self_.softc::<PlrtcSoftc>()) };

    let Some(reg) = faa.fa_reg.first() else {
        printf(format_args!(": no register data\n"));
        return;
    };

    sc.sc_iot.set(Some(faa.fa_iot));
    // SAFETY: the node's register window, which only this driver drives.
    match unsafe { bus_space_map(faa.fa_iot, reg.addr as usize, reg.size as usize, 0) } {
        Ok(ioh) => sc.sc_ioh.set(Some(ioh)),
        Err(_) => {
            printf(format_args!(": failed to map mem space\n"));
            return;
        }
    }

    let Some(mem) = malloc(size_of::<TodrChipHandle>(), M_DEVBUF, M_NOWAIT | M_ZERO) else {
        panic(format_args!("couldn't allocate todr_handle"));
    };
    let handle = mem.cast::<TodrChipHandle>().as_ptr();
    // SAFETY: a fresh allocation of the handle's size, malloc(9)-aligned (at least as
    // strictly as any kernel type), never freed: it becomes `&'static`.
    let handle: &'static TodrChipHandle = unsafe {
        ptr::write(
            handle,
            TodrChipHandle {
                cookie: ptr::from_ref(sc).cast_mut().cast::<c_void>(),
                bus_cookie: ptr::null_mut(),
                todr_quality: 0,
                todr_gettime: plrtc_gettime,
                todr_settime: plrtc_settime,
                todr_setwen: None,
            },
        );
        &*handle
    };
    todr_attach(handle);

    // enable the rtc
    if let (Some(iot), Some(ioh)) = (sc.sc_iot.get(), sc.sc_ioh.get()) {
        bus_space_write_4(iot, ioh, RTCCR, RTCCR_START);
    }

    printf(format_args!("\n"));
}
/* </CODE> */
