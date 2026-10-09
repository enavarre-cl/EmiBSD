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

/*-
 * Copyright (c) 1993, 1994, 1995 Charles Hannum.
 * Copyright (c) 1990 The Regents of the University of California.
 * All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * Don Ahn.
 *
 * Portions Copyright (c) 1993, 1994 by
 *  jc@irbs.UUCP (John Capo)
 *  vak@zebub.msk.su (Serge Vakulenko)
 *  ache@astral.msk.su (Andrew A. Chernov)
 *  joerg_wunsch@uriah.sax.de (Joerg Wunsch)
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
 *	@(#)fd.c	7.4 (Berkeley) 5/25/91
 */
/* </LICENSES> */

/* <CODE> */
//! fdc(4): the PC floppy disk controller, `dev/isa/fdc.c`.
//!
//! Upstream: sys/dev/isa/fdc.c @ 3ce1f3f79392
//!
//! `fdc0 at isa? port 0x3f0 irq 6 drq 2` probes a NEC 765 at the port (reset, then a
//! `specify` it must accept), maps its registers, establishes its interrupt and, from a
//! kernel thread created once threads may be (`kthread_create_deferred`), offers each of
//! the four drive numbers to fd(4) (`fd* at fdc?`), with the default type of the first two
//! read from the RTC's NVRAM diskette byte on the first controller. The 765 command and
//! result handshakes (`out_fdc`, `fdcresult`) and the status printer (`fdcstatus`) are
//! shared with fd(4), which runs the controller's state machine (`fdintr`).
//!
//! It exists where cfg `machine_x86` is set (amd64): the NVRAM read and `struct fd_type`
//! are x86 machine items (`machine::x86`), as the C's `#if defined(__i386__) ||
//! defined(__amd64__)` says.
//!
//! ## Deviations
//! - `out_fdc` and `fdcresult` return `Result`: the C's `-1` (the 765 did not get ready, or
//!   answered more than seven bytes) is `Err(EIO)`; `fdcresult`'s count is the `Ok` value.
//! - `fdcprobe` keeps the C's leak of the first mapping when the second fails.
//! - `fdc->sc_ih` is set from `isa_intr_establish` as the C (`None` when it fails).
//! - `DIAGNOSTIC`'s checks (`fdcstart` not idle, `fdcstatus`'s weird size) are under
//!   feature `diagnostic`.

use core::ffi::c_void;
use core::mem::size_of;
use core::ptr;

use crate::dev::isa::fd::{fd_nvtotype, fdintr};
use crate::dev::isa::fdlink::{DEVIDLE, FdcAttachArgs, FdcSoftc};
use crate::dev::isa::fdreg::{
    FDC_NPORT, FDCTL_NPORT, FDCTL_OFFSET, FDO_FRST, NE7_CB, NE7_DIO, NE7_RQM, NE7_ST0BITS,
    NE7_ST1BITS, NE7_ST2BITS, NE7CMD_SENSEI, NE7CMD_SPECIFY, fddata, fdout, fdsts,
};
use crate::dev::isa::isavar::IsaAttachArgs;
use crate::kern::kern_kthread::{kthread_create, kthread_create_deferred, kthread_exit};
use crate::kern::kern_timeout::timeout_set;
use crate::kern::subr_autoconf::config_found;
use crate::kern::subr_prf::{Bitmask, log, panic, printf};
use crate::machine::bus::{
    BusSpaceHandle, BusSpaceTag, bus_space_map, bus_space_read_1, bus_space_unmap,
    bus_space_write_1,
};
use crate::machine::cpu::delay;
use crate::machine::intr::{IPL_BIO, splbio, splx};
use crate::machine::isa_machdep::{IST_EDGE, isa_intr_establish};
use crate::machine::x86::{NVRAM_DISKETTE, mc146818_read};
use crate::sys::device::{CfMatch, Cfattach, Cfdriver, DV_DULL, Device, QUIET};
use crate::sys::errno::Errno;
use crate::sys::syslog::LOG_ERR;

/// `fdc_ca`.
pub static FDC_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<FdcSoftc>(),
    ca_match: Some(fdcprobe),
    ca_attach: fdcattach,
    ca_detach: None,
    ca_activate: None,
};

/// `fdc_cd`.
pub static FDC_CD: Cfdriver = Cfdriver::new(b"fdc", DV_DULL, 0);

/// `fdcprobe`: a 765 that takes a `specify` after a reset.
pub fn fdcprobe(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: isa's children are probed with an `isa_attach_args`.
    let ia = unsafe { &mut *aux.cast::<IsaAttachArgs>() };

    let iot = ia.ia_iot;
    let mut rv = 0;

    // Map the i/o space.
    let base = ia.ia_iobase() as usize;
    // SAFETY: the controller's ports named by the configuration line; probing is what the
    // ISA bus does with them.
    let Ok(ioh) = (unsafe { bus_space_map(iot, base, FDC_NPORT, 0) }) else {
        return 0;
    };
    // SAFETY: as above, the control register past the gap.
    let Ok(ioh_ctl) = (unsafe { bus_space_map(iot, base + FDCTL_OFFSET, FDCTL_NPORT, 0) }) else {
        return 0;
    };

    'out: {
        // reset
        bus_space_write_1(iot, ioh, fdout, 0);
        delay(100);
        bus_space_write_1(iot, ioh, fdout, FDO_FRST);

        // see if it can handle a command
        if out_fdc(iot, ioh, NE7CMD_SPECIFY).is_err() {
            break 'out;
        }
        let _ = out_fdc(iot, ioh, 0xdf);
        let _ = out_fdc(iot, ioh, 2);

        rv = 1;
        ia.set_ia_iosize(FDC_NPORT as i32);
        ia.set_ia_msize(0);
    }

    // out:
    bus_space_unmap(iot, ioh, FDC_NPORT);
    bus_space_unmap(iot, ioh_ctl, FDCTL_NPORT);
    rv
}

/// `fdcattach`.
pub fn fdcattach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `fdc_ca`'s devices are `FdcSoftc`s.
    let fdc = unsafe { self_.softc::<FdcSoftc>() };
    // SAFETY: as in `fdcprobe`.
    let ia = unsafe { &*aux.cast::<IsaAttachArgs>() };

    let iot = ia.ia_iot;
    let base = ia.ia_iobase() as usize;

    // Re-map the I/O space.
    // SAFETY: the ports fdcprobe found the controller at, this driver's from now on.
    let (ioh, ioh_ctl) = match unsafe {
        (
            bus_space_map(iot, base, FDC_NPORT, 0),
            bus_space_map(iot, base + FDCTL_OFFSET, FDCTL_NPORT, 0),
        )
    } {
        (Ok(ioh), Ok(ioh_ctl)) => (ioh, ioh_ctl),
        _ => panic(format_args!("fdcattach: couldn't map I/O ports")),
    };

    fdc.sc_iot.set(Some(iot));
    fdc.sc_ioh.set(Some(ioh));
    fdc.sc_ioh_ctl.set(Some(ioh_ctl));

    fdc.sc_drq.set(ia.ia_drq());
    fdc.sc_state.set(DEVIDLE);
    fdc.sc_link.sc_drives.init(); // XXX

    printf(format_args!("\n"));

    // SAFETY: the softc is the device, alive while attached (never detached).
    let name: &'static str = unsafe { &*ptr::from_ref(fdc.sc_dev.xname()) };
    fdc.sc_ih.set(isa_intr_establish(
        ia.ia_ic,
        ia.ia_irq(),
        IST_EDGE,
        IPL_BIO,
        fdcintr,
        ptr::from_ref(fdc).cast_mut().cast(),
        name,
    ));

    kthread_create_deferred(fdc_create_kthread, ptr::from_ref(fdc).cast_mut().cast());
}

/// `fdc_create_kthread`.
pub fn fdc_create_kthread(arg: *mut c_void) {
    // SAFETY: `fdcattach` passed its softc, which lives as long as the kernel.
    let sc = unsafe { &*arg.cast::<FdcSoftc>() };
    if kthread_create(fdcattach_deferred, arg, b"fdcattach").is_err() {
        printf(format_args!(
            "{}: failed to create kernel thread, disabled\n",
            sc.sc_dev.xname()
        ));
    }
}

/// `fdcattach_deferred`: offers the four drives to fd(4), then exits.
pub fn fdcattach_deferred(arg: *mut c_void) {
    // SAFETY: as in `fdc_create_kthread`.
    let fdc = unsafe { &*arg.cast::<FdcSoftc>() };

    // The NVRAM info only tells us about the first two disks on the `primary' floppy
    // controller.
    let type_: i32 = if fdc.sc_dev.dv_unit.get() == 0 {
        mc146818_read(NVRAM_DISKETTE) as i32 // XXX softc
    } else {
        -1
    };

    timeout_set(&fdc.fdcpseudointr_to, fdcpseudointr, arg);

    // physical limit: four drives per controller.
    for drive in 0..4 {
        let mut fa = FdcAttachArgs {
            fa_drive: drive,
            fa_flags: 0,
            fa_type: 0,
            // NFD > 0
            fa_deftype: if type_ >= 0 && drive < 2 {
                fd_nvtotype(fdc.sc_dev.xname(), type_, drive)
            } else {
                None // unknown
            },
        };
        let _ = config_found(&fdc.sc_dev, ptr::from_mut(&mut fa).cast(), Some(fddprint));
    }
    kthread_exit(0);
}

/// `fddprint`: print the location of a disk/tape drive (called just before attaching the
/// the drive). If `fdc` is not NULL, the drive was found but was not in the system config
/// file; print the drive name as well. Return QUIET (config_find ignores this if the device
/// was configured) to avoid printing `fdN not configured' messages.
pub fn fddprint(aux: *mut c_void, fdc: Option<&[u8]>) -> i32 {
    // SAFETY: `fdcattach_deferred` attaches its children with an `fdc_attach_args`.
    let fa = unsafe { &*aux.cast::<FdcAttachArgs>() };

    if fdc.is_none() {
        printf(format_args!(" drive {}", fa.fa_drive));
    }
    QUIET
}

/// `fdcresult`: reads the controller's result bytes into `sc_status`, their count as the
/// value.
pub fn fdcresult(fdc: &FdcSoftc) -> Result<usize, Errno> {
    let iot = fdc.iot();
    let ioh = fdc.ioh();
    let mut n = 0;

    for _ in 0..100000 {
        let i = bus_space_read_1(iot, ioh, fdsts) & (NE7_DIO | NE7_RQM | NE7_CB);
        if i == NE7_RQM {
            return Ok(n);
        }
        if i == (NE7_DIO | NE7_RQM | NE7_CB) {
            if n >= fdc.sc_status.len() {
                log(LOG_ERR, format_args!("fdcresult: overrun\n"));
                return Err(Errno::EIO);
            }
            fdc.sc_status[n].set(bus_space_read_1(iot, ioh, fddata));
            n += 1;
        }
        delay(10);
    }
    Err(Errno::EIO)
}

/// `out_fdc`: hands the controller one command byte once it wants it.
pub fn out_fdc(iot: BusSpaceTag, ioh: BusSpaceHandle, x: u8) -> Result<(), Errno> {
    let mut i: i32 = 100000;

    while bus_space_read_1(iot, ioh, fdsts) & NE7_DIO != 0 && {
        let more = i > 0;
        i -= 1;
        more
    } {}
    if i <= 0 {
        return Err(Errno::EIO);
    }
    while bus_space_read_1(iot, ioh, fdsts) & NE7_RQM == 0 && {
        let more = i > 0;
        i -= 1;
        more
    } {}
    if i <= 0 {
        return Err(Errno::EIO);
    }
    bus_space_write_1(iot, ioh, fddata, x);
    Ok(())
}

/// `fdcstart`: sets an idle controller going on its drive queue.
pub fn fdcstart(fdc: &FdcSoftc) {
    // only got here if controller's drive queue was inactive; should be in idle state
    #[cfg(feature = "diagnostic")]
    if fdc.sc_state.get() != DEVIDLE {
        printf(format_args!("fdcstart: not idle\n"));
        return;
    }
    let _ = fdcintr(ptr::from_ref(fdc).cast_mut().cast());
}

/// `fdcstatus`: prints `dv`'s message with the controller's status: the result of a sense
/// interrupt status when `n` is 0, else the `n` result bytes already read.
pub fn fdcstatus(dv: &Device, n: i32, s: &str) {
    let Some(parent) = dv.parent() else {
        return;
    };
    // SAFETY: an fd(4)'s parent is its fdc(4), an `FdcSoftc`.
    let fdc = unsafe { parent.softc::<FdcSoftc>() };
    let mut n = n;

    if n == 0 {
        let _ = out_fdc(fdc.iot(), fdc.ioh(), NE7CMD_SENSEI);
        let _ = fdcresult(fdc);
        n = 2;
    }

    printf(format_args!("{}: {}", dv.xname(), s));

    let st = |i: usize| u64::from(fdc.sc_status[i].get());
    match n {
        0 => {
            printf(format_args!("\n"));
        }
        2 => {
            printf(format_args!(
                " (st0 {} cyl {})\n",
                Bitmask(st(0), NE7_ST0BITS),
                st(1)
            ));
        }
        7 => {
            printf(format_args!(
                " (st0 {} st1 {} st2 {} cyl {} head {} sec {})\n",
                Bitmask(st(0), NE7_ST0BITS),
                Bitmask(st(1), NE7_ST1BITS),
                Bitmask(st(2), NE7_ST2BITS),
                st(3),
                st(4),
                st(5)
            ));
        }
        #[cfg(feature = "diagnostic")]
        _ => {
            printf(format_args!("\nfdcstatus: weird size"));
        }
        #[cfg(not(feature = "diagnostic"))]
        _ => {}
    }
}

/// `fdcpseudointr`: runs the interrupt handler from a timeout, at its level.
pub fn fdcpseudointr(arg: *mut c_void) {
    // Just ensure it has the right spl.
    let s = splbio();
    let _ = fdcintr(arg);
    splx(s);
}

/// `fdcintr`: the controller's interrupt, handed to its drives (`NFD > 0`).
pub fn fdcintr(arg: *mut c_void) -> i32 {
    // SAFETY: `fdcattach` established the interrupt (and the timeouts) with its softc.
    let fdc = unsafe { &*arg.cast::<FdcSoftc>() };

    // Will switch on device type, shortly.
    fdintr(fdc)
}
/* </CODE> */
