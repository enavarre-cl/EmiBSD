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
 * Copyright (c) 1997 - 1999, Jason Downs.  All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR(S) ``AS IS'' AND ANY EXPRESS
 * OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
 * WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
 * DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR(S) BE LIABLE FOR ANY DIRECT,
 * INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
 * (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
 * SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
 * CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 */
/*-
 * Copyright (c) 1993, 1994, 1995, 1996
 *	Charles M. Hannum.  All rights reserved.
 * Copyright (c) 1991 The Regents of the University of California.
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
 *	@(#)com.c	7.5 (Berkeley) 5/16/91
 */
/* </LICENSES> */

/* <CODE> */
//! `com(4)`: the NS16450/NS16550 serial port driver, based on the HP dca driver.
//!
//! Upstream: sys/dev/ic/com.c @ 3ce1f3f79392
//!
//! The console path (`comcnattach` and friends, polled, on the `comcons*` registers) works
//! from the first `printf`; the port attaches later through its bus front-end (`com_isa`),
//! which calls [`com_attach_subr`] and establishes [`comintr`]; opening the port's character
//! device (`cdevsw[8]`, or `/dev/console` through `cnopen`) gives it a tty, fed by
//! `comintr`/[`comsoft`] and drained by [`comstart`].
//!
//! ## Deviations
//! - The globals are atomics, or [`StaticCell`]s for the bus tag and handle; all are written by
//!   the attach routines on the boot CPU before the console is used.
//! - `comspeed` returns `Result`: `Ok(0)` for a hangup (`speed == 0`), `Err(EINVAL)` where the C
//!   returns `-1`. `cominit` still programs the C's `-1` (`0xffff`) when asked for an impossible
//!   speed, as the original does.
//! - `comcn_read_reg` before an attach returns 0 instead of dereferencing an unset handle; so
//!   does `com_read_reg` on a softc whose registers are not mapped.
//! - `com_cd.cd_devs[unit]` is [`com_sc`]: the softc of an attached unit, `None` otherwise;
//!   the entry points answer `ENXIO` for a unit that is not there, where the C (except
//!   `comopen`) would dereference NULL.
//! - The input ring is an index and a fill count (`comvar.rs`); `comintr` stores the data and
//!   line status bytes in pairs, as the C.
//! - The major number lookups (`cdevsw[maj].d_open == comopen`) compare function addresses
//!   with `ptr::fn_addr_eq` (`docs/C_TO_RUST.md`).
//! - `com_detach` frees the tty with `ttyfree` (which is `unsafe`: the tty must be idle),
//!   after `vdevgone` revoked every vnode of the port, as the C.
//! - The interrupt is established under the softc's name, read as a `'static` string from
//!   `dv_xname` ([`Device::xname`]).

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU8, AtomicU32, AtomicUsize, Ordering};

use libkern::StaticCell;

use crate::dev::cons::{CN_HIGHPRI, CN_LOWPRI, Consdev, cn_tab, set_cn_tab};
use crate::dev::ic::comreg::*;
use crate::dev::ic::comvar::*;
use crate::kern::kern_prot::suser;
use crate::kern::kern_softintr::{softintr_disestablish, softintr_establish, softintr_schedule};
use crate::kern::kern_synch::wakeup;
use crate::kern::kern_timeout::{timeout_add_sec, timeout_del, timeout_set};
use crate::kern::subr_prf::{DB_CONSOLE, Str, panic, printf};
use crate::kern::tty::{
    TTOPEN, ttioctl, ttsetwater, ttwakeupwr, ttychars, ttyclose, ttyfree, ttymalloc, ttysleep,
    ttytstamp,
};
use crate::kern::tty_conf::linesw;
use crate::kern::tty_subr::{getc, q_to_b};
use crate::kern::vfs_subr::vdevgone;
use crate::machine::bus::{
    BUS_SPACE_BARRIER_READ, BUS_SPACE_BARRIER_WRITE, BusAddr, BusSize, BusSpaceHandle, BusSpaceTag,
    bus_space_barrier, bus_space_map, bus_space_read_1, bus_space_read_4, bus_space_unmap,
    bus_space_write_1, bus_space_write_4,
};
use crate::machine::conf::{cdevsw, nchrdev};
use crate::machine::cpu::delay;
use crate::machine::db_machdep::db_enter;
use crate::machine::intr::{IPL_TTY, splhigh, spltty, splx};
use crate::sys::conf::DevTypeOpen;
use crate::sys::device::{
    CD_COCOVM, Cfdriver, DV_TTY, DVACT_DEACTIVATE, DVACT_RESUME, DVACT_SUSPEND, Device,
};
use crate::sys::errno::Errno;
use crate::sys::fcntl::O_NONBLOCK;
use crate::sys::ioctl::{ioctl_arg, ioctl_ret};
use crate::sys::param::{NODEV, PCATCH};
use crate::sys::proc::Proc;
use crate::sys::syslog::LOG_WARNING;
use crate::sys::termios::{
    CLOCAL, CRTSCTS, CS5, CS6, CS7, CS8, CSIZE, CSTOPB, HUPCL, MDMBUF, PARENB, PARODD, Tcflag,
    Termios,
};
use crate::sys::tty::{
    TS_BUSY, TS_CARR_ON, TS_FLUSH, TS_ISOPEN, TS_TIMEOUT, TS_TTSTOP, TS_WOPEN, TS_XCLUDE, TTIPRI,
    TTOPRI, TTY_FE, TTY_PE, Tty,
};
use crate::sys::ttycom::{
    TIOCCBRK, TIOCCDTR, TIOCFLAG_CLOCAL, TIOCFLAG_CRTSCTS, TIOCFLAG_MDMBUF, TIOCFLAG_PPS,
    TIOCFLAG_SOFTCAR, TIOCGFLAGS, TIOCM_CD, TIOCM_CTS, TIOCM_DSR, TIOCM_DTR, TIOCM_LE, TIOCM_RI,
    TIOCM_RTS, TIOCMBIC, TIOCMBIS, TIOCMGET, TIOCMSET, TIOCSBRK, TIOCSDTR, TIOCSFLAGS,
};
use crate::sys::ttydefaults::{
    TTYDEF_CFLAG, TTYDEF_IFLAG, TTYDEF_LFLAG, TTYDEF_OFLAG, TTYDEF_SPEED,
};
use crate::sys::types::{Dev, makedev, minor};
use crate::sys::uio::Uio;
use crate::sys::vnode::VCHR;

/// `#define com_lcr com_cfcr`.
const COM_LCR: BusSize = COM_CFCR;

/// `com_cd`.
pub static COM_CD: Cfdriver = Cfdriver::new(b"com", DV_TTY, CD_COCOVM);

/// `comdefaultrate`: the speed a port opens at.
pub static COMDEFAULTRATE: AtomicI32 = AtomicI32::new(TTYDEF_SPEED as i32);
/// `comconsfreq`: the console UART's clock, 0 until the machine code or `comcninit` sets it.
pub static COMCONSFREQ: AtomicI32 = AtomicI32::new(0);
/// `comconsrate`: the console speed.
pub static COMCONSRATE: AtomicI32 = AtomicI32::new(TTYDEF_SPEED as i32);
/// `comconsaddr`: the console UART's bus address, 0 when there is none. Only machine-dependent
/// code sets it, from the firmware's description of the console or the configured `CONADDR`.
pub static COMCONSADDR: AtomicUsize = AtomicUsize::new(0);
/// `comconsattached`: whether `com_attach_subr` has claimed the console port.
pub static COMCONSATTACHED: AtomicBool = AtomicBool::new(false);
/// `comconsiot`: the console UART's bus space tag.
static COMCONSIOT: StaticCell<Option<BusSpaceTag>> = StaticCell::new(None);
/// `comconsioh`: the console UART's mapped registers.
static COMCONSIOH: StaticCell<Option<BusSpaceHandle>> = StaticCell::new(None);
/// `comconsunit`: the unit number of the console port.
pub static COMCONSUNIT: AtomicI32 = AtomicI32::new(0);
/// `comconscflag`: the console's `c_cflag`.
pub static COMCONSCFLAG: AtomicU32 = AtomicU32::new(TTYDEF_CFLAG);
/// `comcons_reg_width`: register width in bytes (4 for memory-mapped 32-bit registers).
pub static COMCONS_REG_WIDTH: AtomicU8 = AtomicU8::new(0);
/// `comcons_reg_shift`: register stride, as a shift count.
pub static COMCONS_REG_SHIFT: AtomicU8 = AtomicU8::new(0);
/// `commajor`: `com`'s character device major number.
pub static COMMAJOR: AtomicI32 = AtomicI32::new(0);

/// `comcons`: the console device `comcnattach` installs.
static COMCONS: Consdev = Consdev {
    cn_probe: None,
    cn_init: None,
    cn_getc: comcngetc,
    cn_putc: comcnputc,
    cn_pollc: comcnpollc,
    cn_bell: None,
    cn_dev: Cell::new(NODEV),
    cn_pri: Cell::new(CN_LOWPRI),
};

/// `DEVUNIT(x)`.
const fn devunit(x: Dev) -> i32 {
    (minor(x) & 0x7f) as i32
}

/// `DEVCUA(x)`.
const fn devcua(x: Dev) -> bool {
    minor(x) & 0x80 != 0
}

/// `com_cd.cd_devs[unit]`: the softc of an attached port.
pub fn com_sc(unit: i32) -> Option<&'static ComSoftc> {
    let dev = COM_CD.cd_dev(unit)?;
    // SAFETY: `com_cd`'s devices were made by `config_make_softc` from a `com` attachment
    // (`com_isa_ca`: `ca_devsize` is `size_of::<ComSoftc>()`), and live until detached.
    Some(unsafe { dev.as_ref().softc::<ComSoftc>() })
}

/// The softc and tty of an open port (`sc->sc_tty` is set by `comopen`).
fn com_sc_tty(dev: Dev) -> Result<(&'static ComSoftc, &'static Tty), Errno> {
    let sc = com_sc(devunit(dev)).ok_or(Errno::ENXIO)?;
    // SAFETY: `sc_tty` is null or `ttymalloc`'s, freed only by `com_detach`.
    let tp = unsafe { sc.sc_tty.get().as_ref() }.ok_or(Errno::ENXIO)?;
    Ok((sc, tp))
}

/// The major number of `com` (`cdevsw[maj].d_open == comopen`), `nchrdev` if none.
fn com_major() -> u32 {
    let n = nchrdev();
    (0..n)
        .find(|&maj| ptr::fn_addr_eq(cdevsw(maj).d_open, comopen as DevTypeOpen))
        .unwrap_or(n)
}

/// The console's tag and handle, once an attach set them.
fn comcons_io() -> Option<(BusSpaceTag, BusSpaceHandle)> {
    // SAFETY: both cells are written by `comcnattach`/`comcninit` on the boot CPU before the
    // console is used, and only read afterwards.
    unsafe { Some((COMCONSIOT.read()?, COMCONSIOH.read()?)) }
}

/// Records the console's tag and handle.
fn set_comcons_io(iot: BusSpaceTag, ioh: BusSpaceHandle) {
    // SAFETY: as for `comcons_io`; this is the single writer, on the boot CPU.
    unsafe {
        COMCONSIOT.write(Some(iot));
        COMCONSIOH.write(Some(ioh));
    }
}

/// `comconsiot`, for the bus front-ends that compare it with their tag.
pub fn comconsiot() -> Option<BusSpaceTag> {
    comcons_io().map(|(t, _)| t)
}

/// `comconsioh`, for the bus front-ends that reuse the console's mapping.
pub fn comconsioh() -> Option<BusSpaceHandle> {
    comcons_io().map(|(_, h)| h)
}

/// `comspeed`: the divisor for `speed` bits per second from a `freq` Hz clock, rounded;
/// `Ok(0)` for a hangup, `Err(EINVAL)` for a negative speed or one the clock cannot produce
/// within `COM_TOLERANCE`.
pub fn comspeed(freq: i64, speed: i64) -> Result<i32, Errno> {
    /// Divide and round off.
    fn divrnd(n: i64, q: i64) -> i64 {
        (n * 2 / q + 1) / 2
    }

    if speed == 0 {
        return Ok(0);
    }
    if speed < 0 {
        return Err(Errno::EINVAL);
    }
    let x = divrnd(freq / 16, speed);
    if x <= 0 {
        return Err(Errno::EINVAL);
    }
    let err = (divrnd(freq * 1000 / 16, speed * x) - 1000).abs();
    if err > i64::from(COM_TOLERANCE) {
        return Err(Errno::EINVAL);
    }
    Ok(x as i32)
}

/// `comprobe1`: whether a UART answers at `ioh`: the line control register reads back and the
/// interrupt identification register has no reserved bits set, within 32 tries.
pub fn comprobe1(iot: BusSpaceTag, ioh: BusSpaceHandle) -> bool {
    // force access to id reg
    bus_space_write_1(iot, ioh, COM_LCR, LCR_8BITS);
    bus_space_write_1(iot, ioh, COM_IIR, 0);
    for _ in 0..32 {
        if bus_space_read_1(iot, ioh, COM_LCR) != LCR_8BITS
            || bus_space_read_1(iot, ioh, COM_IIR) & 0x38 != 0
        {
            bus_space_read_1(iot, ioh, COM_DATA); // cleanup
        } else {
            return true;
        }
    }
    false
}

/// `com_detach`: the port is going away: revoke its vnodes, stop its timers and soft
/// interrupt, free its tty.
pub fn com_detach(self_: &Device, _flags: i32) -> Result<(), Errno> {
    // SAFETY: `com`'s attachments make `ComSoftc`s.
    let sc = unsafe { self_.softc::<ComSoftc>() };

    sc.sc_swflags.set(sc.sc_swflags.get() | COM_SW_DEAD);

    // Locate the major number.
    let maj = com_major();

    // Nuke the vnodes for any open instances.
    let mut mn = self_.dv_unit.get() as u32;
    vdevgone(maj, mn, mn, VCHR);

    // XXX a symbolic constant for the cua bit would be nicer.
    mn |= 0x80;
    vdevgone(maj, mn, mn, VCHR);

    timeout_del(&sc.sc_dtr_tmo);
    timeout_del(&sc.sc_diag_tmo);
    if let Some(si) = sc.sc_si.take() {
        // SAFETY: the handle `com_attach_subr` established, used by nothing after this.
        unsafe { softintr_disestablish(si) };
    }

    // Detach and free the tty.
    if let Some(tp) = NonNull::new(sc.sc_tty.get().cast_mut()) {
        sc.sc_tty.set(ptr::null());
        // SAFETY: the vnodes are gone, the soft interrupt is disestablished and the port is
        // marked dead, so nothing uses the tty any more.
        unsafe { ttyfree(tp) };
    }

    Ok(())
}

/// `com_activate`: suspend, resume or deactivate the port.
pub fn com_activate(self_: &Device, act: i32) -> Result<(), Errno> {
    // SAFETY: as for `com_detach`.
    let sc = unsafe { self_.softc::<ComSoftc>() };
    let mut rv = Ok(());

    match act {
        DVACT_SUSPEND => {
            if timeout_del(&sc.sc_dtr_tmo) {
                // Make sure DTR gets raised upon resume.
                sc.sc_mcr.set(sc.sc_mcr.get() | MCR_DTR | MCR_RTS);
            }
            timeout_del(&sc.sc_diag_tmo);
        }
        DVACT_RESUME => com_resume(sc),
        DVACT_DEACTIVATE => {
            if sc.sc_hwflags.get() & COM_HW_CONSOLE != 0 {
                rv = Err(Errno::EBUSY);
            } else {
                let s = spltty();
                if let Some(disable) = sc.disable.get()
                    && sc.enabled.get() != 0
                {
                    disable(sc);
                    sc.enabled.set(0);
                }
                splx(s);
            }
        }
        _ => {}
    }
    rv
}

/// The FIFO setup `comopen` and `com_resume` share: (re)enable and drain the FIFOs, the
/// threshold from the receive speed.
fn com_fifo_setup(sc: &ComSoftc, ispeed: i32) {
    let mut fifo = FIFO_ENABLE | FIFO_RCV_RST | FIFO_XMT_RST;
    let mut lcr = 0;

    if ispeed <= 1200 {
        fifo |= FIFO_TRIGGER_1;
    } else if ispeed <= 38400 {
        fifo |= FIFO_TRIGGER_4;
    } else {
        fifo |= FIFO_TRIGGER_8;
    }
    if sc.sc_uarttype.get() == COM_UART_TI16750 {
        fifo |= FIFO_ENABLE_64BYTE;
        lcr = com_read_reg(sc, COM_LCR);
        com_write_reg(sc, COM_LCR, lcr | LCR_DLAB);
    }

    // (Re)enable and drain FIFOs.
    //
    // Certain SMC chips cause problems if the FIFOs are enabled while input is ready. Turn
    // off the FIFO if necessary to clear the input. Test the input ready bit after enabling
    // the FIFOs to handle races between enabling and fresh input.
    //
    // Set the FIFO threshold based on the receive speed.
    loop {
        com_write_reg(sc, COM_FIFO, 0);
        delay(100);
        let _ = com_read_reg(sc, COM_DATA);
        com_write_reg(sc, COM_FIFO, fifo | FIFO_RCV_RST | FIFO_XMT_RST);
        delay(100);
        if com_read_reg(sc, COM_LSR) & LSR_RXRDY == 0 {
            break;
        }
    }
    if sc.sc_uarttype.get() == COM_UART_TI16750 {
        com_write_reg(sc, COM_LCR, lcr);
    }
}

/// Wake up the sleepy heads: take the UARTs that have one out of their sleep mode.
fn com_wakeup_uart(sc: &ComSoftc) {
    match sc.sc_uarttype.get() {
        COM_UART_ST16650 | COM_UART_ST16650V2 => {
            com_write_reg(sc, COM_LCR, LCR_EFR);
            com_write_reg(sc, COM_EFR, EFR_ECB);
            com_write_reg(sc, COM_IER, 0);
            com_write_reg(sc, COM_EFR, 0);
            com_write_reg(sc, COM_LCR, 0);
        }
        COM_UART_TI16750 => com_write_reg(sc, COM_IER, 0),
        COM_UART_XR17V35X => com_write_reg(sc, UART_EXAR_SLEEP, 0),
        COM_UART_PXA2X0 => com_write_reg(sc, COM_IER, IER_EUART),
        _ => {}
    }
}

/// `comopen`: open a port; the first open gives it a tty and programs the UART.
pub fn comopen(dev: Dev, flag: i32, _mode: i32, p: &Proc) -> Result<(), Errno> {
    let unit = devunit(dev);

    if unit >= COM_CD.cd_ndevs.get() {
        return Err(Errno::ENXIO);
    }
    let Some(sc) = com_sc(unit) else {
        return Err(Errno::ENXIO);
    };

    let s = spltty();
    // SAFETY: as for `com_sc_tty`.
    let tp: &'static Tty = match unsafe { sc.sc_tty.get().as_ref() } {
        Some(tp) => tp,
        None => {
            let tp = ttymalloc(1_000_000);
            sc.sc_tty.set(tp);
            tp
        }
    };
    splx(s);

    tp.t_oproc.set(Some(comstart));
    tp.t_param.set(Some(comparam));
    tp.t_dev.set(dev);
    let s = if !tp.t_state_isset(TS_ISOPEN) {
        tp.t_state_set(TS_WOPEN);
        ttychars(tp);
        tp.set_t_iflag(TTYDEF_IFLAG);
        tp.set_t_oflag(TTYDEF_OFLAG);
        if sc.sc_hwflags.get() & COM_HW_CONSOLE != 0 {
            tp.set_t_cflag(COMCONSCFLAG.load(Ordering::Relaxed));
            tp.set_t_ispeed(COMCONSRATE.load(Ordering::Relaxed));
            tp.set_t_ospeed(COMCONSRATE.load(Ordering::Relaxed));
        } else {
            tp.set_t_cflag(TTYDEF_CFLAG);
            tp.set_t_ispeed(COMDEFAULTRATE.load(Ordering::Relaxed));
            tp.set_t_ospeed(COMDEFAULTRATE.load(Ordering::Relaxed));
        }
        if sc.sc_swflags.get() & COM_SW_CLOCAL != 0 {
            tp.set_t_cflag(tp.t_cflag() | CLOCAL);
        }
        if sc.sc_swflags.get() & COM_SW_CRTSCTS != 0 {
            tp.set_t_cflag(tp.t_cflag() | CRTSCTS);
        }
        if sc.sc_swflags.get() & COM_SW_MDMBUF != 0 {
            tp.set_t_cflag(tp.t_cflag() | MDMBUF);
        }
        tp.set_t_lflag(TTYDEF_LFLAG);

        let s = spltty();

        sc.sc_initialize.set(1);
        let _ = comparam(tp, &tp.t_termios.get());
        ttsetwater(tp);

        sc.sc_ibuf.set(0);
        sc.sc_ibufp.set(0);

        // Wake up the sleepy heads.
        if sc.sc_hwflags.get() & COM_HW_CONSOLE == 0 {
            com_wakeup_uart(sc);
        }

        if sc.sc_hwflags.get() & COM_HW_FIFO != 0 {
            com_fifo_setup(sc, tp.t_ispeed());
        }

        // Flush any pending I/O.
        while com_read_reg(sc, COM_LSR) & LSR_RXRDY != 0 {
            let _ = com_read_reg(sc, COM_DATA);
        }

        // You turn me on, baby!
        sc.sc_mcr.set(MCR_DTR | MCR_RTS);
        if sc.sc_hwflags.get() & COM_HW_NOIEN == 0 {
            sc.sc_mcr.set(sc.sc_mcr.get() | MCR_IENABLE);
        }
        com_write_reg(sc, COM_MCR, sc.sc_mcr.get());
        sc.sc_ier.set(IER_ERXRDY | IER_ERLS | IER_EMSC);
        if sc.sc_uarttype.get() == COM_UART_PXA2X0 {
            sc.sc_ier.set(sc.sc_ier.get() | IER_EUART | IER_ERXTOUT);
        }
        com_write_reg(sc, COM_IER, sc.sc_ier.get());

        sc.sc_msr.set(com_read_reg(sc, COM_MSR));
        if sc.sc_swflags.get() & COM_SW_SOFTCAR != 0
            || devcua(dev)
            || sc.sc_msr.get() & MSR_DCD != 0
            || tp.t_cflag() & MDMBUF != 0
        {
            tp.t_state_set(TS_CARR_ON);
        } else {
            tp.t_state_clr(TS_CARR_ON);
        }
        s
    } else if tp.t_state_isset(TS_XCLUDE) && suser(p).is_err() {
        return Err(Errno::EBUSY);
    } else {
        spltty()
    };

    if devcua(dev) {
        if tp.t_state_isset(TS_ISOPEN) {
            // Ah, but someone already is dialed in...
            splx(s);
            return Err(Errno::EBUSY);
        }
        sc.sc_cua.set(1); // We go into CUA mode.
    } else {
        // tty (not cua) device; wait for carrier if necessary.
        if flag & O_NONBLOCK != 0 {
            if sc.sc_cua.get() != 0 {
                // Opening TTY non-blocking... but the CUA is busy.
                splx(s);
                return Err(Errno::EBUSY);
            }
        } else {
            while sc.sc_cua.get() != 0
                || (tp.t_cflag() & CLOCAL == 0 && !tp.t_state_isset(TS_CARR_ON))
            {
                tp.t_state_set(TS_WOPEN);
                let error = ttysleep(
                    tp,
                    ptr::from_ref(&tp.t_rawq).cast(),
                    TTIPRI | PCATCH,
                    TTOPEN,
                );
                // If TS_WOPEN has been reset, that means the cua device has been closed. We
                // don't want to fail in that case, so just go around again.
                if let Err(e) = error
                    && tp.t_state_isset(TS_WOPEN)
                {
                    tp.t_state_clr(TS_WOPEN);
                    if sc.sc_cua.get() == 0 && !tp.t_state_isset(TS_ISOPEN) {
                        compwroff(sc);
                    }
                    splx(s);
                    return Err(e);
                }
            }
        }
    }
    splx(s);

    (linesw(tp).l_open)(dev, tp, p)
}

/// `comclose`: close a port; the last close powers the UART down.
pub fn comclose(dev: Dev, flag: i32, _mode: i32, p: Option<&Proc>) -> Result<(), Errno> {
    let (sc, tp) = com_sc_tty(dev)?;

    // XXX This is for cons.c.
    if !tp.t_state_isset(TS_ISOPEN) {
        return Ok(());
    }

    if sc.sc_swflags.get() & COM_SW_DEAD != 0 {
        return Ok(());
    }

    let _ = (linesw(tp).l_close)(tp, flag, p);
    let s = spltty();
    if tp.t_state_isset(TS_WOPEN) {
        // tty device is waiting for carrier; drop dtr then re-raise
        sc.sc_mcr.set(sc.sc_mcr.get() & !(MCR_DTR | MCR_RTS));
        com_write_reg(sc, COM_MCR, sc.sc_mcr.get());
        timeout_add_sec(&sc.sc_dtr_tmo, 2);
    } else {
        // no one else waiting; turn off the uart
        compwroff(sc);
    }
    tp.t_state_clr(TS_BUSY | TS_FLUSH);
    sc.sc_cua.set(0);
    splx(s);
    let _ = ttyclose(tp);

    // #ifdef notyet: free the console's tty here.
    Ok(())
}

/// `compwroff`: power the UART down after the last close.
pub fn compwroff(sc: &ComSoftc) {
    // SAFETY: `compwroff` runs on a port that has a tty (`comopen` made it).
    let Some(tp) = (unsafe { sc.sc_tty.get().as_ref() }) else {
        return;
    };

    sc.sc_lcr.set(sc.sc_lcr.get() & !LCR_SBREAK);
    com_write_reg(sc, COM_LCR, sc.sc_lcr.get());
    let mut ier = 0;
    if sc.sc_uarttype.get() == COM_UART_PXA2X0 {
        ier |= IER_EUART;
    }
    com_write_reg(sc, COM_IER, ier);
    if tp.t_cflag() & HUPCL != 0 && sc.sc_swflags.get() & COM_SW_SOFTCAR == 0 {
        // XXX perhaps only clear DTR
        sc.sc_mcr.set(0);
        com_write_reg(sc, COM_MCR, sc.sc_mcr.get());
    }

    let mut timo = 10000;
    while com_read_reg(sc, COM_LSR) & LSR_TSRE == 0 && {
        timo -= 1;
        timo != 0
    } {
        delay(1);
    }

    // Turn FIFO off; enter sleep mode if possible.
    com_write_reg(sc, COM_FIFO, 0);
    delay(100);
    if com_read_reg(sc, COM_LSR) & LSR_RXRDY != 0 {
        let _ = com_read_reg(sc, COM_DATA);
    }
    delay(100);
    com_write_reg(sc, COM_FIFO, FIFO_RCV_RST | FIFO_XMT_RST);

    if sc.sc_hwflags.get() & COM_HW_CONSOLE == 0 {
        match sc.sc_uarttype.get() {
            COM_UART_ST16650 | COM_UART_ST16650V2 => {
                com_write_reg(sc, COM_LCR, LCR_EFR);
                com_write_reg(sc, COM_EFR, EFR_ECB);
                com_write_reg(sc, COM_IER, IER_SLEEP);
                com_write_reg(sc, COM_LCR, 0);
            }
            COM_UART_TI16750 => com_write_reg(sc, COM_IER, IER_SLEEP),
            COM_UART_XR17V35X => com_write_reg(sc, UART_EXAR_SLEEP, 0xff),
            COM_UART_PXA2X0 => com_write_reg(sc, COM_IER, 0),
            _ => {}
        }
    }
}

/// `com_resume`: reprogram the UART after a suspend.
pub fn com_resume(sc: &ComSoftc) {
    // SAFETY: as for `com_sc_tty`.
    let tp = unsafe { sc.sc_tty.get().as_ref() };

    let Some(tp) = tp.filter(|tp| tp.t_state_isset(TS_ISOPEN)) else {
        if sc.sc_hwflags.get() & COM_HW_CONSOLE != 0
            && let Some((iot, ioh)) = comcons_io()
        {
            cominit(
                iot,
                ioh,
                COMCONSRATE.load(Ordering::Relaxed),
                COMCONSFREQ.load(Ordering::Relaxed),
            );
        }
        return;
    };

    // Wake up the sleepy heads.
    if sc.sc_hwflags.get() & COM_HW_CONSOLE == 0 {
        com_wakeup_uart(sc);
    }

    let ospeed = comspeed(i64::from(sc.sc_frequency.get()), i64::from(tp.t_ospeed())).unwrap_or(-1);

    if ospeed != 0 {
        com_write_reg(sc, COM_LCR, sc.sc_lcr.get() | LCR_DLAB);
        com_write_reg(sc, COM_DLBL, ospeed as u8);
        com_write_reg(sc, COM_DLBH, (ospeed >> 8) as u8);
        com_write_reg(sc, COM_LCR, sc.sc_lcr.get());
    } else {
        com_write_reg(sc, COM_LCR, sc.sc_lcr.get());
    }

    if sc.sc_hwflags.get() & COM_HW_FIFO != 0 {
        com_fifo_setup(sc, tp.t_ispeed());
    }

    // You turn me on, baby!
    com_write_reg(sc, COM_MCR, sc.sc_mcr.get());
    com_write_reg(sc, COM_IER, sc.sc_ier.get());
}

/// `com_raisedtr`: the `sc_dtr_tmo` timeout: raise DTR again after a close.
pub fn com_raisedtr(arg: *mut c_void) {
    // SAFETY: `com_attach_subr` armed the timeout with its softc, which outlives it
    // (`com_detach` deletes it).
    let sc = unsafe { &*arg.cast::<ComSoftc>() };

    sc.sc_mcr.set(sc.sc_mcr.get() | MCR_DTR | MCR_RTS);
    com_write_reg(sc, COM_MCR, sc.sc_mcr.get());
}

/// `comread`.
pub fn comread(dev: Dev, uio: &mut Uio<'_>, flag: i32) -> Result<(), Errno> {
    let (_sc, tp) = com_sc_tty(dev)?;

    (linesw(tp).l_read)(tp, uio, flag)
}

/// `comwrite`.
pub fn comwrite(dev: Dev, uio: &mut Uio<'_>, flag: i32) -> Result<(), Errno> {
    let (_sc, tp) = com_sc_tty(dev)?;

    (linesw(tp).l_write)(tp, uio, flag)
}

/// `comtty`.
pub fn comtty(dev: Dev) -> Option<&'static Tty> {
    com_sc_tty(dev).ok().map(|(_, tp)| tp)
}

/// `tiocm_xxx2mcr`: the MCR bits of `TIOCM_DTR`/`TIOCM_RTS`.
fn tiocm_xxx2mcr(data: i32) -> u8 {
    let mut m = 0;

    if data & TIOCM_DTR != 0 {
        m |= MCR_DTR;
    }
    if data & TIOCM_RTS != 0 {
        m |= MCR_RTS;
    }
    m
}

/// `comioctl`.
pub fn comioctl(dev: Dev, cmd: u64, data: &mut [u8], flag: i32, p: &Proc) -> Result<(), Errno> {
    let (sc, tp) = com_sc_tty(dev)?;

    if (linesw(tp).l_ioctl)(tp, cmd, data, flag, p)? {
        return Ok(());
    }
    if ttioctl(tp, cmd, data, flag, p)? {
        return Ok(());
    }

    match cmd {
        TIOCSBRK => {
            sc.sc_lcr.set(sc.sc_lcr.get() | LCR_SBREAK);
            com_write_reg(sc, COM_LCR, sc.sc_lcr.get());
        }
        TIOCCBRK => {
            sc.sc_lcr.set(sc.sc_lcr.get() & !LCR_SBREAK);
            com_write_reg(sc, COM_LCR, sc.sc_lcr.get());
        }
        TIOCSDTR => {
            sc.sc_mcr.set(sc.sc_mcr.get() | sc.sc_dtr.get());
            com_write_reg(sc, COM_MCR, sc.sc_mcr.get());
        }
        TIOCCDTR => {
            sc.sc_mcr.set(sc.sc_mcr.get() & !sc.sc_dtr.get());
            com_write_reg(sc, COM_MCR, sc.sc_mcr.get());
        }
        TIOCMSET | TIOCMBIS => {
            if cmd == TIOCMSET {
                sc.sc_mcr.set(sc.sc_mcr.get() & !(MCR_DTR | MCR_RTS));
            }
            sc.sc_mcr
                .set(sc.sc_mcr.get() | tiocm_xxx2mcr(ioctl_arg::<i32>(data)));
            com_write_reg(sc, COM_MCR, sc.sc_mcr.get());
        }
        TIOCMBIC => {
            sc.sc_mcr
                .set(sc.sc_mcr.get() & !tiocm_xxx2mcr(ioctl_arg::<i32>(data)));
            com_write_reg(sc, COM_MCR, sc.sc_mcr.get());
        }
        TIOCMGET => {
            let mut bits = 0;

            let m = sc.sc_mcr.get();
            if m & MCR_DTR != 0 {
                bits |= TIOCM_DTR;
            }
            if m & MCR_RTS != 0 {
                bits |= TIOCM_RTS;
            }
            let m = sc.sc_msr.get();
            if m & MSR_DCD != 0 {
                bits |= TIOCM_CD;
            }
            if m & MSR_CTS != 0 {
                bits |= TIOCM_CTS;
            }
            if m & MSR_DSR != 0 {
                bits |= TIOCM_DSR;
            }
            if m & (MSR_RI | MSR_TERI) != 0 {
                bits |= TIOCM_RI;
            }
            if com_read_reg(sc, COM_IER) != 0 {
                bits |= TIOCM_LE;
            }
            ioctl_ret(data, &bits);
        }
        TIOCGFLAGS => {
            let driverbits = sc.sc_swflags.get();
            let mut userbits = 0;
            if driverbits & COM_SW_SOFTCAR != 0 {
                userbits |= TIOCFLAG_SOFTCAR;
            }
            if driverbits & COM_SW_CLOCAL != 0 {
                userbits |= TIOCFLAG_CLOCAL;
            }
            if driverbits & COM_SW_CRTSCTS != 0 {
                userbits |= TIOCFLAG_CRTSCTS;
            }
            if driverbits & COM_SW_MDMBUF != 0 {
                userbits |= TIOCFLAG_MDMBUF;
            }
            if driverbits & COM_SW_PPS != 0 {
                userbits |= TIOCFLAG_PPS;
            }

            ioctl_ret(data, &userbits);
        }
        TIOCSFLAGS => {
            if suser(p).is_err() {
                return Err(Errno::EPERM);
            }

            let userbits = ioctl_arg::<i32>(data);
            let mut driverbits = 0;
            if userbits & TIOCFLAG_SOFTCAR != 0 || sc.sc_hwflags.get() & COM_HW_CONSOLE != 0 {
                driverbits |= COM_SW_SOFTCAR;
            }
            if userbits & TIOCFLAG_CLOCAL != 0 {
                driverbits |= COM_SW_CLOCAL;
            }
            if userbits & TIOCFLAG_CRTSCTS != 0 {
                driverbits |= COM_SW_CRTSCTS;
            }
            if userbits & TIOCFLAG_MDMBUF != 0 {
                driverbits |= COM_SW_MDMBUF;
            }
            if userbits & TIOCFLAG_PPS != 0 {
                driverbits |= COM_SW_PPS;
            }

            sc.sc_swflags.set(driverbits);
        }
        _ => return Err(Errno::ENOTTY),
    }

    Ok(())
}

/// `comparam`: program the line for `t` (already called at spltty).
pub fn comparam(tp: &Tty, t: &Termios) -> Result<(), Errno> {
    let Some(sc) = com_sc(devunit(tp.t_dev.get())) else {
        return Err(Errno::ENXIO);
    };
    let ospeed = comspeed(i64::from(sc.sc_frequency.get()), i64::from(t.c_ospeed));

    // Check requested parameters.
    let ospeed = match ospeed {
        Ok(o) if t.c_ispeed == 0 || t.c_ispeed == t.c_ospeed => o,
        _ => return Err(Errno::EINVAL),
    };

    let mut lcr = sc.sc_lcr.get() & LCR_SBREAK;

    match t.c_cflag & CSIZE {
        CS5 => lcr |= LCR_5BITS,
        CS6 => lcr |= LCR_6BITS,
        CS7 => lcr |= LCR_7BITS,
        CS8 => lcr |= LCR_8BITS,
        _ => {}
    }
    if t.c_cflag & PARENB != 0 {
        lcr |= LCR_PENAB;
        if t.c_cflag & PARODD == 0 {
            lcr |= LCR_PEVEN;
        }
    }
    if t.c_cflag & CSTOPB != 0 {
        lcr |= LCR_STOPB;
    }

    sc.sc_lcr.set(lcr);

    if ospeed == 0 {
        sc.sc_mcr.set(sc.sc_mcr.get() & !MCR_DTR);
        com_write_reg(sc, COM_MCR, sc.sc_mcr.get());
    }

    // Set the FIFO threshold based on the receive speed, if we are changing it.
    if sc.sc_initialize.get() != 0 || tp.t_ispeed() != t.c_ispeed {
        sc.sc_initialize.set(0);

        if ospeed != 0 {
            // Make sure the transmit FIFO is empty before proceeding. If we don't do this,
            // some revisions of the UART will hang. Interestingly enough, even if we do this
            // while the last character is still being pushed out, they don't hang. This
            // seems good enough.
            while tp.t_state_isset(TS_BUSY) {
                sc.sc_halt.set(sc.sc_halt.get() + 1);
                let error = ttysleep(
                    tp,
                    ptr::from_ref(&tp.t_outq).cast(),
                    TTOPRI | PCATCH,
                    "comprm",
                );
                sc.sc_halt.set(sc.sc_halt.get() - 1);
                if let Err(e) = error {
                    comstart(tp);
                    return Err(e);
                }
            }

            com_write_reg(sc, COM_LCR, lcr | LCR_DLAB);
            com_write_reg(sc, COM_DLBL, ospeed as u8);
            com_write_reg(sc, COM_DLBH, (ospeed >> 8) as u8);
            com_write_reg(sc, COM_LCR, lcr);
            sc.sc_mcr.set(sc.sc_mcr.get() | MCR_DTR);
            com_write_reg(sc, COM_MCR, sc.sc_mcr.get());
        } else {
            com_write_reg(sc, COM_LCR, lcr);
        }

        if sc.sc_hwflags.get() & COM_HW_FIFO != 0 {
            let trigger = if t.c_ispeed <= 1200 {
                FIFO_TRIGGER_1
            } else {
                FIFO_TRIGGER_8
            };
            if sc.sc_uarttype.get() == COM_UART_TI16750 {
                com_write_reg(sc, COM_LCR, lcr | LCR_DLAB);
                com_write_reg(sc, COM_FIFO, FIFO_ENABLE | FIFO_ENABLE_64BYTE | trigger);
                com_write_reg(sc, COM_LCR, lcr);
            } else {
                com_write_reg(sc, COM_FIFO, FIFO_ENABLE | trigger);
            }
        }
    } else {
        com_write_reg(sc, COM_LCR, lcr);
    }

    // When not using CRTSCTS, RTS follows DTR.
    if t.c_cflag & CRTSCTS == 0 {
        if sc.sc_mcr.get() & MCR_DTR != 0 {
            if sc.sc_mcr.get() & MCR_RTS == 0 {
                sc.sc_mcr.set(sc.sc_mcr.get() | MCR_RTS);
                com_write_reg(sc, COM_MCR, sc.sc_mcr.get());
            }
        } else if sc.sc_mcr.get() & MCR_RTS != 0 {
            sc.sc_mcr.set(sc.sc_mcr.get() & !MCR_RTS);
            com_write_reg(sc, COM_MCR, sc.sc_mcr.get());
        }
        sc.sc_dtr.set(MCR_DTR | MCR_RTS);
    } else {
        sc.sc_dtr.set(MCR_DTR);
    }

    // and copy to tty
    tp.set_t_ispeed(t.c_ispeed);
    tp.set_t_ospeed(t.c_ospeed);
    let oldcflag: Tcflag = tp.t_cflag();
    tp.set_t_cflag(t.c_cflag);

    // If DCD is off and MDMBUF is changed, ask the tty layer if we should stop the device.
    if sc.sc_msr.get() & MSR_DCD == 0
        && sc.sc_swflags.get() & COM_SW_SOFTCAR == 0
        && (oldcflag & MDMBUF) != (tp.t_cflag() & MDMBUF)
        && (linesw(tp).l_modem)(tp, 0) == 0
    {
        sc.sc_mcr.set(sc.sc_mcr.get() & !sc.sc_dtr.get());
        com_write_reg(sc, COM_MCR, sc.sc_mcr.get());
    }

    // Just to be sure...
    comstart(tp);
    Ok(())
}

/// `comstart`: start output: fill the transmitter (its FIFO) from the output queue and
/// enable the transmit interrupt.
pub fn comstart(tp: &Tty) {
    let Some(sc) = com_sc(devunit(tp.t_dev.get())) else {
        return;
    };

    let s = spltty();
    'out: {
        if tp.t_state_isset(TS_BUSY) {
            break 'out;
        }
        let stopped = tp.t_state_isset(TS_TIMEOUT | TS_TTSTOP)
            || sc.sc_halt.get() > 0
            || (tp.t_cflag() & CRTSCTS != 0 && sc.sc_msr.get() & MSR_CTS == 0)
            || {
                ttwakeupwr(tp);
                tp.t_outq.c_cc.get() == 0
            };
        if stopped {
            if sc.sc_ier.get() & IER_ETXRDY != 0 {
                sc.sc_ier.set(sc.sc_ier.get() & !IER_ETXRDY);
                com_write_reg(sc, COM_IER, sc.sc_ier.get());
            }
            break 'out;
        }
        tp.t_state_set(TS_BUSY);

        // Enable transmit completion interrupts.
        if sc.sc_ier.get() & IER_ETXRDY == 0 {
            sc.sc_ier.set(sc.sc_ier.get() | IER_ETXRDY);
            com_write_reg(sc, COM_IER, sc.sc_ier.get());
        }

        if sc.sc_hwflags.get() & COM_HW_FIFO != 0 {
            let mut buffer = [0u8; 256]; // largest fifo

            let len = (sc.sc_fifolen.get().max(0) as usize).min(buffer.len());
            let n = q_to_b(&tp.t_outq, &mut buffer[..len]);
            for &c in &buffer[..n] {
                com_write_reg(sc, COM_DATA, c);
            }
            buffer[..n].fill(0);
        } else if tp.t_outq.c_cc.get() != 0 {
            com_write_reg(sc, COM_DATA, getc(&tp.t_outq) as u8);
        }
    }
    splx(s);
}

/// `comstop`: stop output on a line.
pub fn comstop(tp: &Tty, _flag: i32) -> Result<(), Errno> {
    let s = spltty();
    if tp.t_state_isset(TS_BUSY) && !tp.t_state_isset(TS_TTSTOP) {
        tp.t_state_set(TS_FLUSH);
    }
    splx(s);
    Ok(())
}

/// `comdiag`: the `sc_diag_tmo` timeout: report the overflows of the last minute.
pub fn comdiag(arg: *mut c_void) {
    // SAFETY: as for `com_raisedtr`.
    let sc = unsafe { &*arg.cast::<ComSoftc>() };

    let s = spltty();
    sc.sc_errors.set(0);
    let overflows = sc.sc_overflows.replace(0);
    let floods = sc.sc_floods.replace(0);
    splx(s);
    crate::log!(
        LOG_WARNING,
        "{}: {} silo overflow{}, {} ibuf overflow{}\n",
        Str(sc.sc_dev.xname().as_bytes()),
        overflows,
        if overflows == 1 { "" } else { "s" },
        floods,
        if floods == 1 { "" } else { "s" }
    );
}

/// `comsoft`: the soft interrupt: hand what `comintr` buffered to the line discipline.
pub fn comsoft(arg: *mut c_void) {
    const LSRMAP: [i32; 8] = [
        0,
        TTY_PE,
        TTY_FE,
        TTY_PE | TTY_FE,
        TTY_FE,
        TTY_PE | TTY_FE,
        TTY_FE,
        TTY_PE | TTY_FE,
    ];

    // SAFETY: `com_attach_subr` established the soft interrupt with its softc, which
    // outlives it (`com_detach` disestablishes it).
    let Some(sc) = (unsafe { arg.cast::<ComSoftc>().as_ref() }) else {
        return;
    };
    if sc.sc_ibufp.get() == 0 {
        return;
    }

    // SAFETY: as for `com_sc_tty`.
    let tp = unsafe { sc.sc_tty.get().as_ref() };

    let s = spltty();

    let ibuf = sc.sc_ibuf.get();
    let ibufend = sc.sc_ibufp.get();

    if ibufend == 0 {
        splx(s);
        return;
    }

    sc.sc_ibuf.set(1 - ibuf);
    sc.sc_ibufp.set(0);

    let Some(tp) = tp.filter(|tp| tp.t_state_isset(TS_ISOPEN)) else {
        splx(s);
        return;
    };

    if tp.t_cflag() & CRTSCTS != 0 && sc.sc_mcr.get() & MCR_RTS == 0 {
        // XXX
        sc.sc_mcr.set(sc.sc_mcr.get() | MCR_RTS);
        com_write_reg(sc, COM_MCR, sc.sc_mcr.get());
    }

    splx(s);

    let buf = &sc.sc_ibufs[ibuf];
    let mut i = 0;
    while i < ibufend {
        let mut c = i32::from(buf[i].get());
        let lsr = buf[i + 1].get();
        i += 2;
        if lsr & LSR_OE != 0 {
            sc.sc_overflows.set(sc.sc_overflows.get() + 1);
            let errors = sc.sc_errors.get();
            sc.sc_errors.set(errors + 1);
            if errors == 0 {
                timeout_add_sec(&sc.sc_diag_tmo, 60);
            }
        }
        // This is ugly, but fast.
        c |= LSRMAP[usize::from((lsr & (LSR_BI | LSR_FE | LSR_PE)) >> 2)];
        (linesw(tp).l_rint)(c, tp);
    }
}

/// `comintr`: the hard interrupt: buffer received characters, follow the modem lines,
/// restart output when the transmitter is empty.
pub fn comintr(arg: *mut c_void) -> i32 {
    // SAFETY: the bus front-end established the interrupt with its softc, which outlives the
    // handler.
    let sc = unsafe { &*arg.cast::<ComSoftc>() };

    // SAFETY: as for `com_sc_tty`.
    let Some(tp) = (unsafe { sc.sc_tty.get().as_ref() }) else {
        return 0; // Can't do squat.
    };

    if com_read_reg(sc, COM_IIR) & IIR_NOPEND != 0 {
        return 0;
    }

    loop {
        let mut lsr = com_read_reg(sc, COM_LSR);

        if lsr & LSR_RXRDY != 0 {
            let buf = &sc.sc_ibufs[sc.sc_ibuf.get()];
            let mut p = sc.sc_ibufp.get();

            if let Some(si) = sc.sc_si.get() {
                softintr_schedule(si);
            }
            loop {
                let mut data = com_read_reg(sc, COM_DATA);
                let mut skip = false;
                if lsr & LSR_BI != 0 {
                    // DDB
                    if sc.sc_hwflags.get() & COM_HW_CONSOLE != 0 {
                        if DB_CONSOLE.load(Ordering::Relaxed) != 0 {
                            db_enter();
                        }
                        skip = true; // goto next
                    } else {
                        data = 0;
                    }
                }
                if !skip {
                    if p >= COM_IBUFSIZE {
                        sc.sc_floods.set(sc.sc_floods.get() + 1);
                        let errors = sc.sc_errors.get();
                        sc.sc_errors.set(errors + 1);
                        if errors == 0 {
                            timeout_add_sec(&sc.sc_diag_tmo, 60);
                        }
                    } else {
                        buf[p].set(data);
                        buf[p + 1].set(lsr);
                        p += 2;
                        if p == COM_IHIGHWATER && tp.t_cflag() & CRTSCTS != 0 {
                            // XXX
                            sc.sc_mcr.set(sc.sc_mcr.get() & !MCR_RTS);
                            com_write_reg(sc, COM_MCR, sc.sc_mcr.get());
                        }
                    }
                }
                // next:
                lsr = com_read_reg(sc, COM_LSR);
                if lsr & LSR_RXRDY == 0 {
                    break;
                }
            }

            sc.sc_ibufp.set(p);
        }
        let msr = com_read_reg(sc, COM_MSR);

        if msr != sc.sc_msr.get() {
            let delta = msr ^ sc.sc_msr.get();

            ttytstamp(
                tp,
                i32::from(sc.sc_msr.get() & MSR_CTS),
                i32::from(msr & MSR_CTS),
                i32::from(sc.sc_msr.get() & MSR_DCD),
                i32::from(msr & MSR_DCD),
            );

            sc.sc_msr.set(msr);
            if delta & MSR_DCD != 0
                && sc.sc_swflags.get() & COM_SW_SOFTCAR == 0
                && (linesw(tp).l_modem)(tp, i32::from(msr & MSR_DCD)) == 0
            {
                sc.sc_mcr.set(sc.sc_mcr.get() & !sc.sc_dtr.get());
                com_write_reg(sc, COM_MCR, sc.sc_mcr.get());
            }
            if (delta & msr) & MSR_CTS != 0 && tp.t_cflag() & CRTSCTS != 0 {
                // the line is up and we want to do rts/cts flow control
                (linesw(tp).l_start)(tp);
            }
        }

        if lsr & LSR_TXRDY != 0 && tp.t_state_isset(TS_BUSY) {
            tp.t_state_clr(TS_BUSY | TS_FLUSH);
            if sc.sc_halt.get() > 0 {
                wakeup(ptr::from_ref(&tp.t_outq));
            }
            (linesw(tp).l_start)(tp);
        }

        if com_read_reg(sc, COM_IIR) & IIR_NOPEND != 0 {
            return 1;
        }
    }
}

/// `cominit`: programs the UART at `ioh` for `rate` bits per second (8N1, FIFOs on,
/// interrupts off, DTR and RTS asserted).
pub fn cominit(iot: BusSpaceTag, ioh: BusSpaceHandle, rate: i32, frequency: i32) {
    let s = splhigh();
    bus_space_write_1(iot, ioh, COM_LCR, LCR_DLAB);
    let rate = comspeed(i64::from(frequency), i64::from(rate)).unwrap_or(-1); // XXX not comdefaultrate?
    bus_space_write_1(iot, ioh, COM_DLBL, rate as u8);
    bus_space_write_1(iot, ioh, COM_DLBH, (rate >> 8) as u8);
    bus_space_write_1(iot, ioh, COM_LCR, LCR_8BITS);
    bus_space_write_1(iot, ioh, COM_MCR, MCR_DTR | MCR_RTS);
    bus_space_write_1(iot, ioh, COM_IER, 0); // Make sure they are off
    bus_space_write_1(
        iot,
        ioh,
        COM_FIFO,
        FIFO_ENABLE | FIFO_RCV_RST | FIFO_XMT_RST | FIFO_TRIGGER_1,
    );
    let _stat = bus_space_read_1(iot, ioh, COM_IIR);
    splx(s);
}

/// `comcnprobe`: the `constab[]` probe: whether the UART at `comconsaddr` answers, and if so
/// which device and priority the console gets.
pub fn comcnprobe(cp: &Consdev) {
    let addr = COMCONSADDR.load(Ordering::Relaxed);
    if addr == 0 {
        return;
    }
    // SAFETY: `comconsiot` is set by machine-dependent code together with `comconsaddr`.
    let Some(iot) = (unsafe { COMCONSIOT.read() }) else {
        return;
    };
    // SAFETY: `comconsaddr` names the console UART the machine code found (see its doc).
    let Ok(ioh) = (unsafe { bus_space_map(iot, addr, COM_NPORTS, 0) }) else {
        return;
    };
    // XXX Some com@acpi devices will fail the comprobe1() check
    let found = COMCONS_REG_WIDTH.load(Ordering::Relaxed) == 4 || comprobe1(iot, ioh);
    bus_space_unmap(iot, ioh, COM_NPORTS);
    if !found {
        return;
    }

    // Locate the major number.
    COMMAJOR.store(com_major() as i32, Ordering::Relaxed);

    // Initialize required fields.
    cp.cn_dev.set(makedev(
        COMMAJOR.load(Ordering::Relaxed) as u32,
        COMCONSUNIT.load(Ordering::Relaxed) as u32,
    ));
    cp.cn_pri.set(CN_HIGHPRI);
}

/// `comcninit`: the `constab[]` init: maps and programs the console UART found by
/// [`comcnprobe`].
pub fn comcninit(_cp: &Consdev) {
    // SAFETY: `comconsiot` is set by machine-dependent code together with `comconsaddr`.
    let iot = unsafe { COMCONSIOT.read() };
    let addr = COMCONSADDR.load(Ordering::Relaxed);
    // SAFETY: `comconsaddr` names the console UART the machine code found (see its doc).
    let ioh = iot.and_then(|iot| unsafe { bus_space_map(iot, addr, COM_NPORTS, 0) }.ok());
    let (Some(iot), Some(ioh)) = (iot, ioh) else {
        panic(format_args!("comcninit: mapping failed"));
    };
    set_comcons_io(iot, ioh);

    if COMCONSFREQ.load(Ordering::Relaxed) == 0 {
        COMCONSFREQ.store(COM_FREQ, Ordering::Relaxed);
    }

    cominit(
        iot,
        ioh,
        COMCONSRATE.load(Ordering::Relaxed),
        COMCONSFREQ.load(Ordering::Relaxed),
    );
}

/// `comcnattach`: makes the UART at `iobase` the console, programmed for `rate` bits per
/// second from a `frequency` Hz clock. `ENOMEM` when the registers cannot be mapped.
///
/// # Safety
///
/// `iobase` must be a 16x50 UART this kernel owns, as the firmware or the configured console
/// address guarantees (the `bus_space_map` contract).
pub unsafe fn comcnattach(
    iot: BusSpaceTag,
    iobase: BusAddr,
    rate: i32,
    frequency: i32,
    cflag: Tcflag,
) -> Result<(), Errno> {
    // SAFETY: forwarded from the caller.
    let ioh = unsafe { bus_space_map(iot, iobase, COM_NPORTS, 0) }.map_err(|_| Errno::ENOMEM)?;
    set_comcons_io(iot, ioh);

    cominit(iot, ioh, rate, frequency);

    set_cn_tab(&COMCONS);

    COMCONSADDR.store(iobase, Ordering::Relaxed);
    COMCONSCFLAG.store(cflag, Ordering::Relaxed);
    COMCONSFREQ.store(frequency, Ordering::Relaxed);
    COMCONSRATE.store(rate, Ordering::Relaxed);

    Ok(())
}

/// `comcngetc`: blocks until a character arrives and returns it.
pub fn comcngetc(_dev: Dev) -> i32 {
    let s = splhigh();

    // Block until a character becomes available.
    while comcn_read_reg(COM_LSR) & LSR_RXRDY == 0 {
        core::hint::spin_loop();
    }

    let c = comcn_read_reg(COM_DATA);

    // Clear any interrupts generated by this transmission.
    let _stat = comcn_read_reg(COM_IIR);
    splx(s);
    i32::from(c)
}

/// `comcnputc`: console kernel output character routine; waits up to 2 ms for the
/// transmitter before and after.
pub fn comcnputc(_dev: Dev, c: i32) {
    let s = spltty();

    // Wait for any pending transmission to finish.
    let mut timo = 2000;
    while comcn_read_reg(COM_LSR) & LSR_TXRDY == 0 && {
        timo -= 1;
        timo != 0
    } {
        delay(1);
    }

    comcn_write_reg(COM_DATA, (c & 0xff) as u8);
    if let Some((iot, ioh)) = comcons_io() {
        bus_space_barrier(
            iot,
            ioh,
            0,
            COM_NPORTS << COMCONS_REG_SHIFT.load(Ordering::Relaxed),
            BUS_SPACE_BARRIER_READ | BUS_SPACE_BARRIER_WRITE,
        );
    }

    // Wait for this transmission to complete.
    let mut timo = 2000;
    while comcn_read_reg(COM_LSR) & LSR_TXRDY == 0 && {
        timo -= 1;
        timo != 0
    } {
        delay(1);
    }

    splx(s);
}

/// `comcnpollc`: nothing to switch; the console is always polled.
pub fn comcnpollc(_dev: Dev, _on: bool) {}

/// `com_enable_debugport`: turn on line break interrupt, set carrier.
pub fn com_enable_debugport(sc: &ComSoftc) {
    let s = splhigh();
    sc.sc_mcr
        .set(sc.sc_mcr.get() | MCR_DTR | MCR_RTS | MCR_IENABLE);
    com_write_reg(sc, COM_MCR, sc.sc_mcr.get());

    splx(s);
}

/// `com_attach_subr`: what every bus front-end's attach shares: identify the UART and its
/// FIFO, claim the console, set up the timers and the soft interrupt.
pub fn com_attach_subr(sc: &'static ComSoftc) {
    let mut probe = false;

    sc.sc_ier.set(0);
    if sc.sc_uarttype.get() == COM_UART_PXA2X0 {
        sc.sc_ier.set(sc.sc_ier.get() | IER_EUART);
    }
    // disable interrupts
    com_write_reg(sc, COM_IER, sc.sc_ier.get());

    if sc.sc_iot.get().is_some()
        && sc.sc_iot.get() == comconsiot()
        && sc.sc_iobase.get() == COMCONSADDR.load(Ordering::Relaxed)
    {
        COMCONSATTACHED.store(true, Ordering::Relaxed);
        sc.sc_hwflags.set(sc.sc_hwflags.get() | COM_HW_CONSOLE);
        sc.sc_swflags.set(sc.sc_swflags.get() | COM_SW_SOFTCAR);
    }

    if sc.sc_hwflags.get() & COM_HW_CONSOLE != 0 {
        // wait for output to finish
        wait_tsre(sc);
    }

    // Probe for all known forms of UART.
    let lcr = com_read_reg(sc, COM_LCR);
    com_write_reg(sc, COM_LCR, LCR_EFR);
    com_write_reg(sc, COM_EFR, 0);
    com_write_reg(sc, COM_LCR, 0);

    com_write_reg(sc, COM_FIFO, FIFO_ENABLE);
    delay(100);

    // Skip specific probes if attachment code knows it already.
    if sc.sc_uarttype.get() == COM_UART_UNKNOWN {
        sc.sc_uarttype.set(match com_read_reg(sc, COM_IIR) >> 6 {
            0 => COM_UART_16450,
            2 => COM_UART_16550,
            3 => COM_UART_16550A,
            _ => COM_UART_UNKNOWN,
        });
        probe = true;
    }

    // Probe for ST16650s
    if probe && sc.sc_uarttype.get() == COM_UART_16550A {
        com_write_reg(sc, COM_LCR, lcr | LCR_DLAB);
        if com_read_reg(sc, COM_EFR) == 0 {
            com_write_reg(sc, COM_EFR, EFR_CTS);
            if com_read_reg(sc, COM_EFR) != 0 {
                sc.sc_uarttype.set(COM_UART_ST16650);
            }
            com_write_reg(sc, COM_EFR, 0);
        } else {
            com_write_reg(sc, COM_LCR, LCR_EFR);
            if com_read_reg(sc, COM_EFR) == 0 {
                sc.sc_uarttype.set(COM_UART_ST16650V2);
            }
        }
    }

    // #if 0: the XR16850 probe (until com works with large FIFOs).

    // Probe for TI16750s
    if probe && sc.sc_uarttype.get() == COM_UART_16550A {
        com_write_reg(sc, COM_LCR, lcr | LCR_DLAB);
        com_write_reg(sc, COM_FIFO, FIFO_ENABLE | FIFO_ENABLE_64BYTE);
        if (com_read_reg(sc, COM_IIR) >> 5) == 7 {
            sc.sc_uarttype.set(COM_UART_TI16750);
        }
        com_write_reg(sc, COM_FIFO, FIFO_ENABLE);
    }

    // Reset the LCR (latch access is probably enabled).
    com_write_reg(sc, COM_LCR, lcr);

    // Probe for 8250
    if probe && sc.sc_uarttype.get() == COM_UART_16450 {
        let scr0 = com_read_reg(sc, COM_SCRATCH);
        com_write_reg(sc, COM_SCRATCH, 0xa5);
        let scr1 = com_read_reg(sc, COM_SCRATCH);
        com_write_reg(sc, COM_SCRATCH, 0x5a);
        let scr2 = com_read_reg(sc, COM_SCRATCH);
        com_write_reg(sc, COM_SCRATCH, scr0);

        if scr1 != 0xa5 || scr2 != 0x5a {
            sc.sc_uarttype.set(COM_UART_8250);
        }
    }

    // Print UART type and initialize ourself.
    let set_fifo = |len: i32| {
        sc.sc_hwflags.set(sc.sc_hwflags.get() | COM_HW_FIFO);
        sc.sc_fifolen.set(len);
    };
    match sc.sc_uarttype.get() {
        COM_UART_UNKNOWN => {
            printf(format_args!(": unknown uart\n"));
        }
        COM_UART_8250 => {
            printf(format_args!(": ns8250, no fifo\n"));
        }
        COM_UART_16450 => {
            printf(format_args!(": ns16450, no fifo\n"));
        }
        COM_UART_16550 => {
            printf(format_args!(": ns16550, no working fifo\n"));
        }
        COM_UART_16550A => {
            if sc.sc_fifolen.get() == 0 {
                sc.sc_fifolen.set(16);
            }
            printf(format_args!(
                ": ns16550a, {} byte fifo\n",
                sc.sc_fifolen.get()
            ));
            sc.sc_hwflags.set(sc.sc_hwflags.get() | COM_HW_FIFO);
        }
        COM_UART_ST16650 => {
            printf(format_args!(": st16650, no working fifo\n"));
        }
        COM_UART_ST16650V2 => {
            if sc.sc_fifolen.get() == 0 {
                sc.sc_fifolen.set(32);
            }
            printf(format_args!(
                ": st16650, {} byte fifo\n",
                sc.sc_fifolen.get()
            ));
            sc.sc_hwflags.set(sc.sc_hwflags.get() | COM_HW_FIFO);
        }
        COM_UART_ST16C654 => {
            printf(format_args!(": st16c654, 64 byte fifo\n"));
            set_fifo(64);
        }
        COM_UART_TI16750 => {
            printf(format_args!(": ti16750, 64 byte fifo\n"));
            set_fifo(64);
        }
        // #if 0: COM_UART_XR16850, COM_UART_OX16C950.
        COM_UART_XR17V35X => {
            printf(format_args!(": xr17v35x, 256 byte fifo\n"));
            set_fifo(256);
        }
        COM_UART_DW_APB => {
            printf(format_args!(": dw16550"));
            sc.sc_hwflags.set(sc.sc_hwflags.get() | COM_HW_FIFO);
            let cpr = match (sc.sc_iot.get(), sc.sc_ioh.get()) {
                (Some(t), Some(h)) => bus_space_read_4(t, h, COM_CPR << 2),
                _ => 0,
            };
            sc.sc_fifolen.set((cpr_fifo_mode(cpr) * 16) as i32);
            if sc.sc_fifolen.get() != 0 {
                printf(format_args!(", {} byte fifo\n", sc.sc_fifolen.get()));
            } else {
                printf(format_args!("\n"));
                // The DW-APB configuration on the Allwinner H6 SoC does not provide the CPR
                // register and will be detected as having no FIFO. But it does have a
                // 256-byte FIFO and with the FIFO disabled the LSR_RXRDY bit remains set even
                // if the input buffer is empty. As a workaround, treat as a 1-byte FIFO.
                sc.sc_fifolen.set(1);
            }
        }
        COM_UART_PXA2X0 => {
            printf(format_args!(": pxa2x0, 32 byte fifo\n"));
            set_fifo(32);
        }
        _ => panic(format_args!("comattach: bad fifo type")),
    }

    if sc.sc_hwflags.get() & COM_HW_CONSOLE == 0 && sc.sc_fifolen.get() < 256 {
        com_fifo_probe(sc);
    }

    if sc.sc_fifolen.get() == 0 {
        sc.sc_hwflags.set(sc.sc_hwflags.get() & !COM_HW_FIFO);
        sc.sc_fifolen.set(1);
    }

    if sc.sc_hwflags.get() & COM_HW_CONSOLE != 0 {
        // wait for output to finish
        wait_tsre(sc);
    }

    // clear and disable fifo
    // DW-APB UART cannot turn off FIFO here (ddb will not work)
    let fifo = if sc.sc_uarttype.get() == COM_UART_DW_APB {
        FIFO_ENABLE | FIFO_TRIGGER_1
    } else {
        0
    };
    com_write_reg(sc, COM_FIFO, fifo | FIFO_RCV_RST | FIFO_XMT_RST);
    if com_read_reg(sc, COM_LSR) & LSR_RXRDY != 0 {
        let _ = com_read_reg(sc, COM_DATA);
    }
    com_write_reg(sc, COM_FIFO, fifo);

    sc.sc_mcr.set(0);
    com_write_reg(sc, COM_MCR, sc.sc_mcr.get());

    if sc.sc_hwflags.get() & COM_HW_CONSOLE != 0 {
        // locate the major number
        let maj = com_major();

        crate::kassert!(maj < nchrdev());
        if let Some(cn) = cn_tab() {
            cn.cn_dev.set(makedev(maj, sc.sc_dev.dv_unit.get() as u32));
        }

        printf(format_args!(
            "{}: console\n",
            Str(sc.sc_dev.xname().as_bytes())
        ));
    }

    let arg = ptr::from_ref(sc).cast_mut().cast::<c_void>();
    timeout_set(&sc.sc_diag_tmo, comdiag, arg);
    timeout_set(&sc.sc_dtr_tmo, com_raisedtr, arg);
    sc.sc_si.set(softintr_establish(IPL_TTY, comsoft, arg));
    if sc.sc_si.get().is_none() {
        panic(format_args!(
            "{}: can't establish soft interrupt",
            Str(sc.sc_dev.xname().as_bytes())
        ));
    }

    // If there are no enable/disable functions, assume the device is always enabled.
    if sc.enable.get().is_none() {
        sc.enabled.set(1);
    }

    if sc.sc_hwflags.get() & COM_HW_CONSOLE != 0 {
        com_enable_debugport(sc);
    }
}

/// `while (!ISSET(com_read_reg(sc, com_lsr), LSR_TSRE) && --timo) delay(1);` with
/// `timo = 10000`: wait for output to finish.
fn wait_tsre(sc: &ComSoftc) {
    let mut timo = 10000;
    while com_read_reg(sc, COM_LSR) & LSR_TSRE == 0 && {
        timo -= 1;
        timo != 0
    } {
        delay(1);
    }
}

/// `com_fifo_probe`: measure the FIFO in loopback mode; trust the smaller of the probed and
/// the expected depth.
pub fn com_fifo_probe(sc: &ComSoftc) {
    if sc.sc_hwflags.get() & COM_HW_FIFO == 0 {
        return;
    }

    let mut ier = 0;
    if sc.sc_uarttype.get() == COM_UART_PXA2X0 {
        ier |= IER_EUART;
    }
    com_write_reg(sc, COM_IER, ier);
    com_write_reg(sc, COM_LCR, LCR_DLAB);
    com_write_reg(sc, COM_DLBL, 3);
    com_write_reg(sc, COM_DLBH, 0);
    com_write_reg(sc, COM_LCR, LCR_PNONE | LCR_8BITS);
    com_write_reg(sc, COM_MCR, MCR_LOOPBACK);

    let mut fifo = FIFO_ENABLE | FIFO_RCV_RST | FIFO_XMT_RST;
    if sc.sc_uarttype.get() == COM_UART_TI16750 {
        fifo |= FIFO_ENABLE_64BYTE;
    }

    com_write_reg(sc, COM_FIFO, fifo);

    /// Waits up to 2 ms for `bit` in the line status register; whether it came.
    fn wait_lsr(sc: &ComSoftc, bit: u8) -> bool {
        let mut timo = 2000;
        while com_read_reg(sc, COM_LSR) & bit == 0 && {
            timo -= 1;
            timo != 0
        } {
            delay(1);
        }
        timo != 0
    }

    let mut len = 0;
    while len < 256 {
        com_write_reg(sc, COM_DATA, (len + 1) as u8);
        if !wait_lsr(sc, LSR_TXRDY) {
            break;
        }
        len += 1;
    }

    delay(100);

    len = 0;
    while len < 256 {
        if !wait_lsr(sc, LSR_RXRDY) || i32::from(com_read_reg(sc, COM_DATA)) != (len + 1) & 0xff {
            break;
        }
        len += 1;
    }

    // For safety, always use the smaller value.
    if sc.sc_fifolen.get() > len {
        printf(format_args!(
            "{}: probed fifo depth: {} bytes\n",
            Str(sc.sc_dev.xname().as_bytes()),
            len
        ));
        sc.sc_fifolen.set(len);
    }
}

/// `com_read_reg`: reads port register `reg`, honouring the register width and stride.
pub fn com_read_reg(sc: &ComSoftc, reg: BusSize) -> u8 {
    let reg = reg << sc.sc_reg_shift.get();
    let (Some(iot), Some(ioh)) = (sc.sc_iot.get(), sc.sc_ioh.get()) else {
        return 0;
    };

    if sc.sc_reg_width.get() == 4 {
        bus_space_read_4(iot, ioh, reg) as u8
    } else {
        bus_space_read_1(iot, ioh, reg)
    }
}

/// `com_write_reg`: writes port register `reg`, honouring the register width and stride.
pub fn com_write_reg(sc: &ComSoftc, reg: BusSize, value: u8) {
    let reg = reg << sc.sc_reg_shift.get();
    let (Some(iot), Some(ioh)) = (sc.sc_iot.get(), sc.sc_ioh.get()) else {
        return;
    };

    if sc.sc_reg_width.get() == 4 {
        bus_space_write_4(iot, ioh, reg, u32::from(value));
    } else {
        bus_space_write_1(iot, ioh, reg, value);
    }
}

/// `comcn_read_reg`: reads console register `reg`, honouring the register width and stride.
pub fn comcn_read_reg(reg: BusSize) -> u8 {
    let reg = reg << COMCONS_REG_SHIFT.load(Ordering::Relaxed);
    let Some((iot, ioh)) = comcons_io() else {
        return 0;
    };
    if COMCONS_REG_WIDTH.load(Ordering::Relaxed) == 4 {
        bus_space_read_4(iot, ioh, reg) as u8
    } else {
        bus_space_read_1(iot, ioh, reg)
    }
}

/// `comcn_write_reg`: writes console register `reg`, honouring the register width and stride.
pub fn comcn_write_reg(reg: BusSize, value: u8) {
    let reg = reg << COMCONS_REG_SHIFT.load(Ordering::Relaxed);
    let Some((iot, ioh)) = comcons_io() else {
        return;
    };
    if COMCONS_REG_WIDTH.load(Ordering::Relaxed) == 4 {
        bus_space_write_4(iot, ioh, reg, u32::from(value));
    } else {
        bus_space_write_1(iot, ioh, reg, value);
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn divisors() {
        assert_eq!(comspeed(i64::from(COM_FREQ), 115_200), Ok(1));
        assert_eq!(comspeed(i64::from(COM_FREQ), 9600), Ok(12));
        assert_eq!(comspeed(i64::from(COM_FREQ), 0), Ok(0));
        assert_eq!(comspeed(i64::from(COM_FREQ), -1), Err(Errno::EINVAL));
        // 1843200 / 16 / 100000 rounds to 1, which is 115200 baud: 15% off, out of tolerance.
        assert_eq!(comspeed(i64::from(COM_FREQ), 100_000), Err(Errno::EINVAL));
        assert_eq!(
            comspeed(i64::from(COM_FREQ), 10_000_000),
            Err(Errno::EINVAL)
        );
    }

    #[test]
    fn minor_bits() {
        assert_eq!(devunit(makedev(8, 0x81)), 1);
        assert!(devcua(makedev(8, 0x81)));
        assert!(!devcua(makedev(8, 1)));
        assert_eq!(tiocm_xxx2mcr(TIOCM_DTR | TIOCM_RTS), MCR_DTR | MCR_RTS);
    }
}
/* </TESTS> */
