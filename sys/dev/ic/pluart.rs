/*	$OpenBSD: pluart.c,v 1.14 2022/07/02 08:50:42 visa Exp $	*/
/*	$OpenBSD: pluartvar.h,v 1.5 2022/06/27 13:03:32 anton Exp $	*/
/*
 * Copyright (c) 2014 Patrick Wildt <patrick@blueri.se>
 * Copyright (c) 2005 Dale Rahn <drahn@dalerahn.com>
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

//! `pluart(4)`: the ARM PrimeCell PL011 UART, `dev/ic/pluart.c`, with the declarations of
//! `<dev/ic/pluartvar.h>`.
//!
//! Upstream: sys/dev/ic/pluart.c @ 3ce1f3f79392
//! Upstream: sys/dev/ic/pluartvar.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M2 ports the register map and the console path: `pluartcnprobe`,
//! `pluartcninit`, `pluartcnattach`, `pluartcngetc`, `pluartcnputc`, `pluartcnpollc` and the
//! `pluartcons*` globals. `struct pluart_softc`, `pluart_attach_common`, `pluart_intr`, the
//! tty entry points and the `cdevsw` take-over of `com`'s entries arrive with M7.
//!
//! ## Deviations
//! - The globals are atomics, or [`StaticCell`]s for the bus tag and handle; `pluartcnattach`
//!   writes them on the boot CPU before the console is used.
//! - `pluartcnattach` cannot find `com`'s major number nor replace its `cdevsw` entry (M7); the
//!   console's `cn_dev` stays `NODEV` and the `ENXIO` path is not reachable.
//! - `UART_DR_DATA(x)` and the other function-like macros are `const fn`s.
//! - `splhigh` around the polled accesses arrives with `spl(9)` (M4).

use core::cell::Cell;
use core::sync::atomic::{AtomicI32, AtomicU32, AtomicUsize, Ordering};

use libkern::StaticCell;

use crate::dev::cons::{CN_MIDPRI, Consdev, set_cn_tab};
use crate::machine::bus::{
    BusAddr, BusSize, BusSpaceHandle, BusSpaceTag, bus_space_map, bus_space_read_4,
    bus_space_write_4,
};
use crate::sys::errno::Errno;
use crate::sys::param::NODEV;
use crate::sys::termios::{B38400, Tcflag};
use crate::sys::ttydefaults::TTYDEF_CFLAG;
use crate::sys::types::Dev;
use crate::unported;

/// Data register.
pub const UART_DR: BusSize = 0x00;
/// `UART_DR_DATA(x)`: the data bits of a DR read.
pub const fn uart_dr_data(x: u32) -> u32 {
    x & 0xf
}
/// Framing error.
pub const UART_DR_FE: u32 = 1 << 8;
/// Parity error.
pub const UART_DR_PE: u32 = 1 << 9;
/// Break error.
pub const UART_DR_BE: u32 = 1 << 10;
/// Overrun error.
pub const UART_DR_OE: u32 = 1 << 11;
/// Receive status register.
pub const UART_RSR: BusSize = 0x04;
/// Framing error.
pub const UART_RSR_FE: u32 = 1 << 0;
/// Parity error.
pub const UART_RSR_PE: u32 = 1 << 1;
/// Break error.
pub const UART_RSR_BE: u32 = 1 << 2;
/// Overrun error.
pub const UART_RSR_OE: u32 = 1 << 3;
/// Error clear register.
pub const UART_ECR: BusSize = 0x04;
/// Framing error.
pub const UART_ECR_FE: u32 = 1 << 0;
/// Parity error.
pub const UART_ECR_PE: u32 = 1 << 1;
/// Break error.
pub const UART_ECR_BE: u32 = 1 << 2;
/// Overrun error.
pub const UART_ECR_OE: u32 = 1 << 3;
/// Flag register.
pub const UART_FR: BusSize = 0x18;
/// Clear to send.
pub const UART_FR_CTS: u32 = 1 << 0;
/// Data set ready.
pub const UART_FR_DSR: u32 = 1 << 1;
/// Data carrier detect.
pub const UART_FR_DCD: u32 = 1 << 2;
/// UART busy.
pub const UART_FR_BUSY: u32 = 1 << 3;
/// Receive FIFO empty.
pub const UART_FR_RXFE: u32 = 1 << 4;
/// Transmit FIFO full.
pub const UART_FR_TXFF: u32 = 1 << 5;
/// Receive FIFO full.
pub const UART_FR_RXFF: u32 = 1 << 6;
/// Transmit FIFO empty.
pub const UART_FR_TXFE: u32 = 1 << 7;
/// Ring indicator.
pub const UART_FR_RI: u32 = 1 << 8;
/// IrDA low-power counter register.
pub const UART_ILPR: BusSize = 0x20;
/// `UART_ILPR_ILPDVSR`: IrDA low-power divisor.
pub const fn uart_ilpr_ilpdvsr(x: u32) -> u32 {
    x & 0xf
}
/// Integer baud rate register.
pub const UART_IBRD: BusSize = 0x24;
/// `UART_IBRD_DIVINT(x)`: integer baud rate divisor.
pub const fn uart_ibrd_divint(x: u32) -> u32 {
    x & 0xffff
}
/// Fractional baud rate register.
pub const UART_FBRD: BusSize = 0x28;
/// `UART_FBRD_DIVFRAC(x)`: fractional baud rate divisor.
pub const fn uart_fbrd_divfrac(x: u32) -> u32 {
    x & 0x3f
}
/// Line control register.
pub const UART_LCR_H: BusSize = 0x2c;
/// Send break.
pub const UART_LCR_H_BRK: u32 = 1 << 0;
/// Parity enable.
pub const UART_LCR_H_PEN: u32 = 1 << 1;
/// Even parity select.
pub const UART_LCR_H_EPS: u32 = 1 << 2;
/// Two stop bits select.
pub const UART_LCR_H_STP2: u32 = 1 << 3;
/// Enable FIFOs.
pub const UART_LCR_H_FEN: u32 = 1 << 4;
/// Word length: 5 bits.
pub const UART_LCR_H_WLEN5: u32 = 0x0 << 5;
/// Word length: 6 bits.
pub const UART_LCR_H_WLEN6: u32 = 0x1 << 5;
/// Word length: 7 bits.
pub const UART_LCR_H_WLEN7: u32 = 0x2 << 5;
/// Word length: 8 bits.
pub const UART_LCR_H_WLEN8: u32 = 0x3 << 5;
/// Stick parity select.
pub const UART_LCR_H_SPS: u32 = 1 << 7;
/// Control register.
pub const UART_CR: BusSize = 0x30;
/// UART enable.
pub const UART_CR_UARTEN: u32 = 1 << 0;
/// SIR enable.
pub const UART_CR_SIREN: u32 = 1 << 1;
/// IrDA SIR low power mode.
pub const UART_CR_SIRLP: u32 = 1 << 2;
/// Loop back enable.
pub const UART_CR_LBE: u32 = 1 << 7;
/// Transmit enable.
pub const UART_CR_TXE: u32 = 1 << 8;
/// Receive enable.
pub const UART_CR_RXE: u32 = 1 << 9;
/// Data transmit enable.
pub const UART_CR_DTR: u32 = 1 << 10;
/// Request to send.
pub const UART_CR_RTS: u32 = 1 << 11;
/// Out 1.
pub const UART_CR_OUT1: u32 = 1 << 12;
/// Out 2.
pub const UART_CR_OUT2: u32 = 1 << 13;
/// CTS hardware flow control enable.
pub const UART_CR_CTSE: u32 = 1 << 14;
/// RTS hardware flow control enable.
pub const UART_CR_RTSE: u32 = 1 << 15;
/// Interrupt FIFO level select register.
pub const UART_IFLS: BusSize = 0x34;
/// RX level in bits [5:3].
pub const UART_IFLS_RX_SHIFT: u32 = 3;
/// TX level in bits [2:0].
pub const UART_IFLS_TX_SHIFT: u32 = 0;
/// FIFO 1/8 full.
pub const UART_IFLS_1_8: u32 = 0;
/// FIFO 1/4 full.
pub const UART_IFLS_1_4: u32 = 1;
/// FIFO 1/2 full.
pub const UART_IFLS_1_2: u32 = 2;
/// FIFO 3/4 full.
pub const UART_IFLS_3_4: u32 = 3;
/// FIFO 7/8 full.
pub const UART_IFLS_7_8: u32 = 4;
/// Interrupt mask set/clear register.
pub const UART_IMSC: BusSize = 0x38;
/// Ring indicator modem interrupt mask.
pub const UART_IMSC_RIMIM: u32 = 1 << 0;
/// CTS modem interrupt mask.
pub const UART_IMSC_CTSMIM: u32 = 1 << 1;
/// DCD modem interrupt mask.
pub const UART_IMSC_DCDMIM: u32 = 1 << 2;
/// DSR modem interrupt mask.
pub const UART_IMSC_DSRMIM: u32 = 1 << 3;
/// Receive interrupt mask.
pub const UART_IMSC_RXIM: u32 = 1 << 4;
/// Transmit interrupt mask.
pub const UART_IMSC_TXIM: u32 = 1 << 5;
/// Receive timeout interrupt mask.
pub const UART_IMSC_RTIM: u32 = 1 << 6;
/// Framing error interrupt mask.
pub const UART_IMSC_FEIM: u32 = 1 << 7;
/// Parity error interrupt mask.
pub const UART_IMSC_PEIM: u32 = 1 << 8;
/// Break error interrupt mask.
pub const UART_IMSC_BEIM: u32 = 1 << 9;
/// Overrun error interrupt mask.
pub const UART_IMSC_OEIM: u32 = 1 << 10;
/// Raw interrupt status register.
pub const UART_RIS: BusSize = 0x3c;
/// Masked interrupt status register.
pub const UART_MIS: BusSize = 0x40;
/// Interrupt clear register.
pub const UART_ICR: BusSize = 0x44;
/// DMA control register.
pub const UART_DMACR: BusSize = 0x48;
/// Peripheral identification register 0.
pub const UART_PID0: BusSize = 0xfe0;
/// Peripheral identification register 1.
pub const UART_PID1: BusSize = 0xfe4;
/// Peripheral identification register 2.
pub const UART_PID2: BusSize = 0xfe8;
/// `UART_PID2_REV(x)`: the revision field of PID2.
pub const fn uart_pid2_rev(x: u32) -> u32 {
    (x & 0xf0) >> 4
}
/// Peripheral identification register 3.
pub const UART_PID3: BusSize = 0xfec;
/// Size of the register window.
pub const UART_SPACE: BusSize = 0x100;

/// FIFO depth.
pub const UART_FIFO_SIZE: usize = 16;
/// FIFO depth from revision 3 on.
pub const UART_FIFO_SIZE_R3: usize = 32;

/// `pluartdefaultrate`: the speed a port opens at.
pub static PLUARTDEFAULTRATE: AtomicI32 = AtomicI32::new(B38400 as i32);
/// `pluartconsrate`: the console speed.
pub static PLUARTCONSRATE: AtomicI32 = AtomicI32::new(B38400 as i32);
/// `pluartconsiot`: the console UART's bus space tag.
static PLUARTCONSIOT: StaticCell<Option<BusSpaceTag>> = StaticCell::new(None);
/// `pluartconsioh`: the console UART's mapped registers.
static PLUARTCONSIOH: StaticCell<Option<BusSpaceHandle>> = StaticCell::new(None);
/// `pluartconsaddr`: the console UART's bus address.
pub static PLUARTCONSADDR: AtomicUsize = AtomicUsize::new(0);
/// `pluartconscflag`: the console's `c_cflag`.
pub static PLUARTCONSCFLAG: AtomicU32 = AtomicU32::new(TTYDEF_CFLAG);

/// `pluartcons`: the console device `pluartcnattach` installs.
static PLUARTCONS: Consdev = Consdev {
    cn_probe: None,
    cn_init: None,
    cn_getc: pluartcngetc,
    cn_putc: pluartcnputc,
    cn_pollc: pluartcnpollc,
    cn_bell: None,
    cn_dev: Cell::new(NODEV),
    cn_pri: Cell::new(CN_MIDPRI),
};

/// The console's tag and handle, once `pluartcnattach` set them.
fn pluartcons_io() -> Option<(BusSpaceTag, BusSpaceHandle)> {
    // SAFETY: both cells are written by `pluartcnattach` on the boot CPU before the console is
    // used, and only read afterwards.
    unsafe { Some((PLUARTCONSIOT.read()?, PLUARTCONSIOH.read()?)) }
}

/// `pluartcnprobe`: nothing to probe; the console is attached explicitly.
pub fn pluartcnprobe(_cp: &Consdev) {}

/// `pluartcninit`: nothing to do; `pluartcnattach` did it.
pub fn pluartcninit(_cp: &Consdev) {}

/// `pluartcnattach`: makes the PL011 at `iobase` the console. `ENOMEM` when its registers
/// cannot be mapped.
///
/// # Safety
///
/// `iobase` must be a PL011 this kernel owns, as the device tree guarantees (the
/// `bus_space_map` contract).
pub unsafe fn pluartcnattach(
    iot: BusSpaceTag,
    iobase: BusAddr,
    rate: i32,
    cflag: Tcflag,
) -> Result<(), Errno> {
    // SAFETY: forwarded from the caller.
    let ioh = unsafe { bus_space_map(iot, iobase, UART_SPACE, 0) }.map_err(|_| Errno::ENOMEM)?;
    // SAFETY: single writer, on the boot CPU, before the console is used (see `pluartcons_io`).
    unsafe {
        PLUARTCONSIOT.write(Some(iot));
        PLUARTCONSIOH.write(Some(ioh));
    }

    // Disable FIFO.
    bus_space_write_4(
        iot,
        ioh,
        UART_LCR_H,
        bus_space_read_4(iot, ioh, UART_LCR_H) & !UART_LCR_H_FEN,
    );

    // Look for major of com(4) to replace: cdevsw arrives with M7, and with it the KLUDGE that
    // installs `pluartdev` in com's slot; until then the console has no device number.
    let _ = unported!("cdevsw lookup of comopen (pluartcnattach)");

    set_cn_tab(&PLUARTCONS);

    PLUARTCONSADDR.store(iobase, Ordering::Relaxed);
    PLUARTCONSCFLAG.store(cflag, Ordering::Relaxed);
    PLUARTCONSRATE.store(rate, Ordering::Relaxed);

    Ok(())
}

/// `pluartcngetc`: blocks until a character arrives and returns it.
pub fn pluartcngetc(_dev: Dev) -> i32 {
    // s = splhigh(): M4.
    let Some((iot, ioh)) = pluartcons_io() else {
        return 0;
    };
    while bus_space_read_4(iot, ioh, UART_FR) & UART_FR_RXFE != 0 {
        core::hint::spin_loop();
    }
    bus_space_read_4(iot, ioh, UART_DR) as i32
}

/// `pluartcnputc`: sends one character, waiting for room in the transmit FIFO.
pub fn pluartcnputc(_dev: Dev, c: i32) {
    // s = splhigh(): M4.
    let Some((iot, ioh)) = pluartcons_io() else {
        return;
    };
    while bus_space_read_4(iot, ioh, UART_FR) & UART_FR_TXFF != 0 {
        core::hint::spin_loop();
    }
    bus_space_write_4(iot, ioh, UART_DR, u32::from(c as u8));
}

/// `pluartcnpollc`: nothing to switch; the console is always polled.
pub fn pluartcnpollc(_dev: Dev, _on: bool) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_helpers() {
        assert_eq!(uart_dr_data(0x1ff), 0xf);
        assert_eq!(uart_ibrd_divint(0x12345), 0x2345);
        assert_eq!(uart_fbrd_divfrac(0x7f), 0x3f);
        assert_eq!(uart_pid2_rev(0x34), 3);
        assert_eq!(uart_ilpr_ilpdvsr(0xff), 0xf);
        assert_eq!(UART_LCR_H_WLEN8, 0x60);
    }
}
