/*	$OpenBSD: com.c,v 1.184 2026/08/31 15:40:34 deraadt Exp $	*/
/*	$NetBSD: com.c,v 1.82.4.1 1996/06/02 09:08:00 mrg Exp $	*/

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

//! `com(4)`: the NS16450/NS16550 serial port driver, based on the HP dca driver.
//!
//! Upstream: sys/dev/ic/com.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M2 ports the console path: `comspeed`, `comprobe1`, `cominit`,
//! `comcnprobe`, `comcninit`, `comcnattach`, `comcngetc`, `comcnputc`, `comcnpollc`,
//! `comcn_read_reg`, `comcn_write_reg` and the `comcons*` globals. The tty side (`comopen`
//! through `comintr`, `com_attach_subr`, `com_fifo_probe`, `com_read_reg`/`com_write_reg` on a
//! `struct com_softc`, whose type is `comvar.rs`) arrives with the tty milestone (M7).
//!
//! ## Deviations
//! - The globals are atomics, or [`StaticCell`]s for the bus tag and handle; all are written by
//!   the attach routines on the boot CPU before the console is used.
//! - `comspeed` returns `Result`: `Ok(0)` for a hangup (`speed == 0`), `Err(EINVAL)` where the C
//!   returns `-1`. `cominit` still programs the C's `-1` (`0xffff`) when asked for an impossible
//!   speed, as the original does.
//! - `splhigh`/`spltty` around the polled accesses arrive with `spl(9)` (M4).
//! - `comcnprobe` cannot look up `com`'s major number without `cdevsw` (M7): `commajor` stays 0.
//! - `comcn_read_reg` before an attach returns 0 instead of dereferencing an unset handle.

use core::cell::Cell;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU8, AtomicU32, AtomicUsize, Ordering};

use libkern::StaticCell;

use crate::dev::cons::{CN_HIGHPRI, CN_LOWPRI, Consdev, set_cn_tab};
use crate::dev::ic::comreg::*;
use crate::machine::bus::{
    BUS_SPACE_BARRIER_READ, BUS_SPACE_BARRIER_WRITE, BusAddr, BusSize, BusSpaceHandle, BusSpaceTag,
    bus_space_barrier, bus_space_map, bus_space_read_1, bus_space_read_4, bus_space_unmap,
    bus_space_write_1, bus_space_write_4,
};
use crate::machine::cpu::delay;
use crate::sys::errno::Errno;
use crate::sys::param::NODEV;
use crate::sys::termios::Tcflag;
use crate::sys::ttydefaults::{TTYDEF_CFLAG, TTYDEF_SPEED};
use crate::sys::types::{Dev, makedev};
use crate::unported;

/// `#define com_lcr com_cfcr`.
const COM_LCR: BusSize = COM_CFCR;

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

/// `cominit`: programs the UART at `ioh` for `rate` bits per second (8N1, FIFOs on,
/// interrupts off, DTR and RTS asserted).
pub fn cominit(iot: BusSpaceTag, ioh: BusSpaceHandle, rate: i32, frequency: i32) {
    // int s = splhigh(): spl(9) arrives with M4.
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

    // Locate the major number: `cdevsw` and `comopen` arrive with M7; 0 is what the C's loop
    // leaves in `commajor` when nothing matches.
    let _ = unported!("cdevsw lookup of comopen (comcnprobe)");
    COMMAJOR.store(0, Ordering::Relaxed);

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
        #[allow(clippy::panic)] // the C panics here too
        {
            panic!("comcninit: mapping failed");
        }
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
    // int s = splhigh(): M4.

    // Block until a character becomes available.
    while comcn_read_reg(COM_LSR) & LSR_RXRDY == 0 {
        core::hint::spin_loop();
    }

    let c = comcn_read_reg(COM_DATA);

    // Clear any interrupts generated by this transmission.
    let _stat = comcn_read_reg(COM_IIR);
    i32::from(c)
}

/// `comcnputc`: sends one character, waiting up to 2 ms for the transmitter before and after.
pub fn comcnputc(_dev: Dev, c: i32) {
    // int s = spltty(): M4.

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
}

/// `comcnpollc`: nothing to switch; the console is always polled.
pub fn comcnpollc(_dev: Dev, _on: bool) {}

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
}
