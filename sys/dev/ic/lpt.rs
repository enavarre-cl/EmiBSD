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

/*
 * Copyright (c) 1993, 1994 Charles Hannum.
 * Copyright (c) 1990 William F. Jolitz, TeleMuse
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *	This software is a component of "386BSD" developed by
 *	William F. Jolitz, TeleMuse.
 * 4. Neither the name of the developer nor the name "386BSD"
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS A COMPONENT OF 386BSD DEVELOPED BY WILLIAM F. JOLITZ
 * AND IS INTENDED FOR RESEARCH AND EDUCATIONAL PURPOSES ONLY. THIS
 * SOFTWARE SHOULD NOT BE CONSIDERED TO BE A COMMERCIAL PRODUCT.
 * THE DEVELOPER URGES THAT USERS WHO REQUIRE A COMMERCIAL PRODUCT
 * NOT MAKE USE OF THIS WORK.
 *
 * FOR USERS WHO WISH TO UNDERSTAND THE 386BSD SYSTEM DEVELOPED
 * BY WILLIAM F. JOLITZ, WE RECOMMEND THE USER STUDY WRITTEN
 * REFERENCES SUCH AS THE  "PORTING UNIX TO THE 386" SERIES
 * (BEGINNING JANUARY 1991 "DR. DOBBS JOURNAL", USA AND BEGINNING
 * JUNE 1991 "UNIX MAGAZIN", GERMANY) BY WILLIAM F. JOLITZ AND
 * LYNNE GREER JOLITZ, AS WELL AS OTHER BOOKS ON UNIX AND THE
 * ON-LINE 386BSD USER MANUAL BEFORE USE. A BOOK DISCUSSING THE INTERNALS
 * OF 386BSD ENTITLED "386BSD FROM THE INSIDE OUT" WILL BE AVAILABLE LATE 1992.
 *
 * THIS SOFTWARE IS PROVIDED BY THE DEVELOPER ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE DEVELOPER BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! `lpt(4)`: the AT parallel printer port (Device Driver for AT parallel printer port).
//!
//! Upstream: sys/dev/ic/lpt.c @ 3ce1f3f79392
//!
//! The bus front end (`lpt_isa.c`) maps the three registers and calls
//! [`lpt_attach_common`]. `lptopen` primes the printer and waits until it is selected and
//! not busy; `lptwrite` copies the user's bytes into a 1 kB buffer and pushes them out
//! ([`lptpushbytes`]): by the interrupt and a quarter-second wake-up tick that both run
//! [`lptintr`], one byte per "ready", or, for a minor with `LPT_NOINTR`, by polling the
//! status with an adaptive busy wait. Each byte is put on the data lines and strobed.
//!
//! ## Deviations
//! - `lptclose` and `lptwrite` look the softc up with `cd_dev`, which returns `ENXIO` for a
//!   unit with no device where the C indexes `lpt_cd.cd_devs` and dereferences what it
//!   finds (the open succeeded, so the device exists in practice).
//! - `LPRINTF` (`DEBUG` and `notdef`) is not carried over.
//! - `lpt_port_test` returns a `bool` (`temp == data`).
//! - The device's `NLPT` (`lpt.h`) is the arch's `conf.rs`'s: 1 on amd64 (`lpt0 at isa?`),
//!   0 on arm64.

use core::ffi::c_void;
use core::ptr;

use crate::dev::ic::lptreg::{
    LPC_AUTOLF, LPC_IENABLE, LPC_NINIT, LPC_SELECT, LPC_STROBE, LPS_NACK, LPS_NBSY, LPS_NERR,
    LPS_NOPAPER, LPS_SELECT, lpt_control, lpt_data, lpt_status,
};
use crate::dev::ic::lptvar::{
    LPT_AUTOLF, LPT_INIT, LPT_NOINTR, LPT_NOPRIME, LPT_OBUSY, LPT_OPEN, LPT_POLLED, LptSoftc,
};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_subr::uiomove;
use crate::kern::kern_synch::{tsleep_nsec, wakeup};
use crate::kern::kern_timeout::{timeout_add_msec, timeout_del, timeout_set};
#[cfg(feature = "diagnostic")]
use crate::kern::subr_prf::printf;
use crate::kern::subr_prf::{log, panic};
use crate::machine::bus::{
    BusAddr, BusSize, BusSpaceHandle, BusSpaceTag, bus_space_read_1, bus_space_write_1,
};
use crate::machine::cpu::delay;
use crate::machine::intr::{spltty, splx};
use crate::sys::device::{Cfdriver, DV_TTY, DVACT_RESUME, DVACT_SUSPEND, Device};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_DEVBUF, M_WAITOK, M_ZERO};
use crate::sys::param::{PCATCH, PZERO};
use crate::sys::proc::Proc;
use crate::sys::syslog::LOG_NOTICE;
use crate::sys::systm::INFSLP;
use crate::sys::time::msec_to_nsec;
use crate::sys::types::{Dev, minor};
use crate::sys::uio::Uio;

/// `TIMEOUT`: wait up to 16 seconds for a ready (milliseconds).
const TIMEOUT: i32 = 16000;
/// `STEP`: 1/4 seconds (milliseconds).
const STEP: i32 = 250;

/// `LPTPRI`.
const LPTPRI: i32 = PZERO + 8;
/// `LPT_BSIZE`.
const LPT_BSIZE: usize = 1024;

/// `LPS_INVERT`: the status bits that read 1 when the printer is fine.
const LPS_INVERT: u8 = LPS_SELECT | LPS_NERR | LPS_NBSY | LPS_NACK;
/// `LPS_MASK`.
const LPS_MASK: u8 = LPS_SELECT | LPS_NERR | LPS_NBSY | LPS_NACK | LPS_NOPAPER;

/// `lpt_cd`.
pub static LPT_CD: Cfdriver = Cfdriver::new(b"lpt", DV_TTY, 0);

/// `LPTUNIT(s)`.
const fn lptunit(dev: Dev) -> i32 {
    (minor(dev) & 0x1f) as i32
}

/// `LPTFLAGS(s)`: `LPT_AUTOLF`, `LPT_NOPRIME`, `LPT_NOINTR` from the minor.
const fn lptflags(dev: Dev) -> u8 {
    (minor(dev) & 0xe0) as u8
}

impl LptSoftc {
    /// `sc->sc_iot`, `sc->sc_ioh`.
    fn io(&self) -> (BusSpaceTag, BusSpaceHandle) {
        match (self.sc_iot.get(), self.sc_ioh.get()) {
            (Some(t), Some(h)) => (t, h),
            _ => panic(format_args!("lpt: registers not mapped")),
        }
    }

    /// `bus_space_read_1(sc->sc_iot, sc->sc_ioh, off)`.
    fn read(&self, off: usize) -> u8 {
        let (t, h) = self.io();
        bus_space_read_1(t, h, off)
    }

    /// `bus_space_write_1(sc->sc_iot, sc->sc_ioh, off, v)`.
    fn write(&self, off: usize, v: u8) {
        let (t, h) = self.io();
        bus_space_write_1(t, h, off, v);
    }

    /// `NOT_READY()`: the status bits that say the printer cannot take a byte.
    fn not_ready(&self) -> u8 {
        (self.read(lpt_status) ^ LPS_INVERT) & LPS_MASK
    }

    /// `NOT_READY_ERR()`: as `NOT_READY()`, logging what changed (`lpt_not_ready`).
    fn not_ready_err(&self) -> u8 {
        lpt_not_ready(self.read(lpt_status), self)
    }

    /// `*sc->sc_cp++`: the next byte of the buffer.
    fn next_byte(&self) -> u8 {
        let cp = self.sc_cp.get();
        // SAFETY: `sc_cp` walks `sc_inbuf` (`LPT_BSIZE` bytes, allocated by lptopen) and
        // `sc_count` bytes are left from it; callers take a byte only while `sc_count > 0`.
        let b = unsafe { *cp };
        self.sc_cp.set(cp.wrapping_add(1));
        b
    }

    /// The softc as the `void *` of the interrupt and the timeout.
    fn as_arg(&self) -> *mut c_void {
        ptr::from_ref(self).cast_mut().cast()
    }
}

/// The softc of unit `unit`, if one attached.
fn lpt_sc(unit: i32) -> Option<&'static LptSoftc> {
    let dv = LPT_CD.cd_dev(unit)?;
    // SAFETY: `lpt_cd`'s devices are made by the front ends' cfattach, whose softc is an
    // `LptSoftc`; a softc lives as long as its device, which is never freed while attached.
    Some(unsafe { &*ptr::from_ref(dv.as_ref().softc::<LptSoftc>()) })
}

/// `lpt_port_test`: internal routine to lptprobe to do port tests of one byte value.
pub fn lpt_port_test(
    iot: BusSpaceTag,
    ioh: BusSpaceHandle,
    _base: BusAddr,
    off: BusSize,
    data: u8,
    mask: u8,
) -> bool {
    let data = data & mask;
    bus_space_write_1(iot, ioh, off, data);
    let mut timeout = 1000;
    let mut temp;
    loop {
        delay(10);
        temp = bus_space_read_1(iot, ioh, off) & mask;
        if temp == data {
            break;
        }
        timeout -= 1;
        if timeout == 0 {
            break;
        }
    }
    temp == data
}

/// `lpt_attach_common`: ends the attach line, keeps the printer in INIT, sets up the
/// wake-up tick.
pub fn lpt_attach_common(sc: &'static LptSoftc) {
    crate::kern::subr_prf::printf(format_args!("\n"));

    sc.write(lpt_control, LPC_NINIT);

    timeout_set(&sc.sc_wakeup_tmo, lptwakeup, sc.as_arg());
}

/// `lptopen`: reset the printer, then wait until it's selected and not busy.
pub fn lptopen(dev: Dev, _flag: i32, _mode: i32, _p: &Proc) -> Result<(), Errno> {
    let unit = lptunit(dev);
    let flags = lptflags(dev);

    let sc = lpt_sc(unit).ok_or(Errno::ENXIO)?;

    sc.sc_flags.set((sc.sc_flags.get() & LPT_POLLED) | flags);
    if sc.sc_flags.get() & (LPT_POLLED | LPT_NOINTR) == LPT_POLLED {
        return Err(Errno::ENXIO);
    }

    #[cfg(feature = "diagnostic")]
    if sc.sc_state.get() != 0 {
        printf(format_args!(
            "{}: stat=0x{:x} not zero\n",
            sc.sc_dev.xname(),
            sc.sc_state.get()
        ));
    }

    if sc.sc_state.get() != 0 {
        return Err(Errno::EBUSY);
    }

    sc.sc_state.set(LPT_INIT);

    if flags & LPT_NOPRIME == 0 {
        // assert INIT for 100 usec to start up printer
        sc.write(lpt_control, LPC_SELECT);
        delay(100);
    }

    let mut control = LPC_SELECT | LPC_NINIT;
    sc.write(lpt_control, control);

    // wait till ready (printer running diagnostics)
    let mut spin = 0;
    while sc.not_ready_err() != 0 {
        if spin >= TIMEOUT {
            sc.sc_state.set(0);
            return Err(Errno::EBUSY);
        }

        // wait 1/4 second, give up if we get a signal
        let error = tsleep_nsec(
            ptr::from_ref(sc),
            LPTPRI | PCATCH,
            "lptopen",
            msec_to_nsec(STEP as u64),
        );
        if sc.sc_state.get() == 0 {
            return Err(Errno::EIO);
        }
        if error != Err(Errno::EWOULDBLOCK) {
            sc.sc_state.set(0);
            return error;
        }
        spin += STEP;
    }

    if flags & LPT_NOINTR == 0 {
        control |= LPC_IENABLE;
    }
    if flags & LPT_AUTOLF != 0 {
        control |= LPC_AUTOLF;
    }
    sc.sc_control.set(control);
    sc.write(lpt_control, control);

    let Some(buf) = malloc(LPT_BSIZE, M_DEVBUF, M_WAITOK | M_ZERO) else {
        panic(format_args!("lptopen: malloc(M_WAITOK) failed"));
    };
    sc.sc_inbuf.set(buf.as_ptr());
    sc.sc_count.set(0);
    sc.sc_state.set(LPT_OPEN);

    if sc.sc_flags.get() & LPT_NOINTR == 0 {
        lptwakeup(sc.as_arg());
    }

    Ok(())
}

/// `lpt_not_ready`: the not-ready bits of `status`, logging the conditions that newly
/// appeared.
pub fn lpt_not_ready(status: u8, sc: &LptSoftc) -> u8 {
    let status = (status ^ LPS_INVERT) & LPS_MASK;
    let new = status & !sc.sc_laststatus.get();
    sc.sc_laststatus.set(status);

    if new & LPS_SELECT != 0 {
        log(LOG_NOTICE, format_args!("{}: offline\n", sc.sc_dev.xname()));
    } else if new & LPS_NOPAPER != 0 {
        log(
            LOG_NOTICE,
            format_args!("{}: out of paper\n", sc.sc_dev.xname()),
        );
    } else if new & LPS_NERR != 0 {
        log(
            LOG_NOTICE,
            format_args!("{}: output error\n", sc.sc_dev.xname()),
        );
    }

    status
}

/// `lptwakeup`: the quarter-second tick that runs the interrupt handler while the device
/// is open.
fn lptwakeup(arg: *mut c_void) {
    // SAFETY: `lpt_attach_common` set the timeout with the softc, which outlives it.
    let sc = unsafe { &*arg.cast::<LptSoftc>() };

    let s = spltty();
    lptintr(arg);
    splx(s);

    if sc.sc_state.get() != 0 {
        timeout_add_msec(&sc.sc_wakeup_tmo, STEP as u64);
    }
}

/// `lptclose`: close the device, and free the local line buffer.
pub fn lptclose(dev: Dev, _flag: i32, _mode: i32, _p: Option<&Proc>) -> Result<(), Errno> {
    let sc = lpt_sc(lptunit(dev)).ok_or(Errno::ENXIO)?;

    if sc.sc_count.get() != 0 {
        let _ = lptpushbytes(sc);
    }

    if sc.sc_flags.get() & LPT_NOINTR == 0 {
        timeout_del(&sc.sc_wakeup_tmo);
    }

    sc.write(lpt_control, LPC_NINIT);
    sc.sc_state.set(0);
    sc.write(lpt_control, LPC_NINIT);
    if let Some(buf) = core::ptr::NonNull::new(sc.sc_inbuf.get()) {
        free(buf, M_DEVBUF, LPT_BSIZE);
    }

    Ok(())
}

/// `lptpushbytes`: sends the buffered bytes, by polling (`LPT_NOINTR`) or by sleeping
/// while the interrupt sends them.
pub fn lptpushbytes(sc: &LptSoftc) -> Result<(), Errno> {
    if sc.sc_flags.get() & LPT_NOINTR != 0 {
        let control = sc.sc_control.get();

        while sc.sc_count.get() > 0 {
            let mut spin = 0;
            if sc.sc_state.get() == 0 {
                return Err(Errno::EIO);
            }
            while sc.not_ready() != 0 {
                spin += 1;
                if spin < sc.sc_spinmax.get() {
                    continue;
                }
                let mut msecs: i32 = 0;
                // adapt busy-wait algorithm
                sc.sc_spinmax.set(sc.sc_spinmax.get() + 1);
                while sc.not_ready_err() != 0 {
                    // exponential backoff
                    msecs = msecs + msecs + 10;
                    if msecs > TIMEOUT {
                        msecs = TIMEOUT;
                    }
                    let mut error = tsleep_nsec(
                        ptr::from_ref(sc),
                        LPTPRI | PCATCH,
                        "lptpsh",
                        msec_to_nsec(msecs as u64),
                    );
                    if sc.sc_state.get() == 0 {
                        error = Err(Errno::EIO);
                    }
                    if error != Err(Errno::EWOULDBLOCK) {
                        return error;
                    }
                    if sc.sc_count.get() == 0 {
                        return Ok(());
                    }
                }
                break;
            }

            sc.write(lpt_data, sc.next_byte());
            sc.write(lpt_control, control | LPC_STROBE);
            sc.sc_count.set(sc.sc_count.get() - 1);
            sc.write(lpt_control, control);

            // adapt busy-wait algorithm
            if spin * 2 + 16 < sc.sc_spinmax.get() {
                sc.sc_spinmax.set(sc.sc_spinmax.get() - 1);
            }
        }
    } else {
        while sc.sc_count.get() > 0 {
            // if the printer is ready for a char, give it one
            if sc.sc_state.get() & LPT_OBUSY == 0 {
                let s = spltty();
                let _ = lptintr(sc.as_arg());
                splx(s);
            }
            if sc.sc_state.get() == 0 {
                return Err(Errno::EIO);
            }
            let mut error = tsleep_nsec(ptr::from_ref(sc), LPTPRI | PCATCH, "lptwrite2", INFSLP);
            if sc.sc_state.get() == 0 {
                error = Err(Errno::EIO);
            }
            error?;
        }
    }
    Ok(())
}

/// `lptwrite`: copy a line from user space to a local buffer, then call putc to get the
/// chars moved to the output queue.
pub fn lptwrite(dev: Dev, uio: &mut Uio<'_>, _flags: i32) -> Result<(), Errno> {
    let sc = lpt_sc(lptunit(dev)).ok_or(Errno::ENXIO)?;

    loop {
        let n = LPT_BSIZE.min(uio.uio_resid);
        if n == 0 {
            break;
        }
        let buf = sc.sc_inbuf.get();
        // SAFETY: `sc_inbuf` is the `LPT_BSIZE` bytes lptopen allocated (the device is
        // open), and nothing else uses them while no bytes are pending (`sc_count` is 0
        // between pushes).
        uiomove(unsafe { core::slice::from_raw_parts_mut(buf, n) }, uio)?;
        sc.sc_cp.set(buf);
        sc.sc_count.set(n);
        if let Err(error) = lptpushbytes(sc) {
            // Return accurate residual if interrupted or timed out.
            uio.uio_resid += sc.sc_count.get();
            sc.sc_count.set(0);
            return Err(error);
        }
    }
    Ok(())
}

/// `lptintr`: handle printer interrupts which occur when the printer is ready to accept
/// another char.
pub fn lptintr(arg: *mut c_void) -> i32 {
    // SAFETY: the front end established the interrupt (and lpt_attach_common the timeout)
    // with the softc, which outlives them.
    let sc = unsafe { &*arg.cast::<LptSoftc>() };

    if (sc.sc_state.get() & LPT_OPEN == 0 && sc.sc_count.get() == 0)
        || sc.sc_flags.get() & LPT_NOINTR != 0
    {
        return 0;
    }

    // is printer online and ready for output
    if sc.not_ready() != 0 && sc.not_ready_err() != 0 {
        return -1;
    }

    if sc.sc_count.get() != 0 {
        let control = sc.sc_control.get();
        // send char
        sc.write(lpt_data, sc.next_byte());
        delay(50);
        sc.write(lpt_control, control | LPC_STROBE);
        sc.sc_count.set(sc.sc_count.get() - 1);
        sc.write(lpt_control, control);
        sc.sc_state.set(sc.sc_state.get() | LPT_OBUSY);
    } else {
        sc.sc_state.set(sc.sc_state.get() & !LPT_OBUSY);
    }

    if sc.sc_count.get() == 0 {
        // none, wake up the top half to get more
        wakeup(ptr::from_ref(sc));
    }

    1
}

/// `lpt_activate`: stop the tick on suspend; on resume reinitialise the printer and, if
/// the device is open, wait for it again.
pub fn lpt_activate(self_: &Device, act: i32) -> Result<(), Errno> {
    // SAFETY: the front ends' cfattach make `LptSoftc`s.
    let sc = unsafe { self_.softc::<LptSoftc>() };

    match act {
        DVACT_SUSPEND => {
            timeout_del(&sc.sc_wakeup_tmo);
        }
        DVACT_RESUME => {
            sc.write(lpt_control, LPC_NINIT);

            if sc.sc_state.get() != 0 {
                if sc.sc_flags.get() & LPT_NOPRIME == 0 {
                    // assert INIT for 100 usec to start up printer
                    sc.write(lpt_control, LPC_SELECT);
                    delay(100);
                }

                sc.write(lpt_control, LPC_SELECT | LPC_NINIT);

                // wait till ready (printer running diagnostics)
                let mut spin = 0;
                let mut failed = false;
                while sc.not_ready_err() != 0 {
                    if spin >= TIMEOUT {
                        sc.sc_state.set(0);
                        failed = true;
                        break;
                    }

                    // wait 1/4 second, give up if we get a signal
                    delay((STEP * 1000) as u32);
                    spin += STEP;
                }

                if !failed {
                    sc.write(lpt_control, sc.sc_control.get());
                    wakeup(ptr::from_ref(sc));
                }
            }
        }
        _ => {}
    }

    Ok(())
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minor_bits() {
        assert_eq!(lptunit(0x21), 1);
        assert_eq!(lptflags(0x21), LPT_AUTOLF);
        assert_eq!(lptflags(0xe0), LPT_AUTOLF | LPT_NOPRIME | LPT_NOINTR);
    }

    #[test]
    fn status_inversion() {
        // QEMU's idle parallel port reads 0xdf (all fine, not busy): nothing is not ready.
        assert_eq!((0xdf ^ LPS_INVERT) & LPS_MASK, 0);
        // A printer that is offline, busy and out of paper.
        assert_eq!(
            (0x20 ^ LPS_INVERT) & LPS_MASK,
            LPS_SELECT | LPS_NERR | LPS_NBSY | LPS_NACK | LPS_NOPAPER
        );
    }
}
/* </TESTS> */
