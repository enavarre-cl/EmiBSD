/*	$OpenBSD: pluart.c,v 1.14 2022/07/02 08:50:42 visa Exp $	*/
/*	$OpenBSD: pluartvar.h,v 1.5 2022/06/27 13:03:32 anton Exp $	*/
/* <LICENSES> */
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
/* </LICENSES> */

//! `pluart(4)`: the ARM PrimeCell PL011 UART, `dev/ic/pluart.c`, with the declarations of
//! `<dev/ic/pluartvar.h>`.
//!
//! Upstream: sys/dev/ic/pluart.c @ 3ce1f3f79392
//! Upstream: sys/dev/ic/pluartvar.h @ 3ce1f3f79392
//!
//! The console path (`pluartcnattach` and friends, polled) works from the first `printf`;
//! the port attaches later through `pluart_fdt` ([`pluart_attach_common`] and
//! [`pluart_intr`]), and opening its character device (com's slot 8, which the console
//! takes over: `pluartdev`) gives it a tty, fed by `pluart_intr`/[`pluart_softint`] and
//! drained by [`pluart_start`].
//!
//! ## Deviations
//! - The globals are atomics, or [`StaticCell`]s for the bus tag and handle; `pluartcnattach`
//!   writes them on the boot CPU before the console is used.
//! - `pluartcnattach`'s KLUDGE (`cdevsw[maj] = pluartdev`) goes through
//!   `machine::conf::cdevsw_set`; the major is found by comparing function addresses with
//!   `ptr::fn_addr_eq`, as `com` and the ptys do (`docs/C_TO_RUST.md`).
//! - [`PluartSoftc`] is a softc (`docs/C_TO_RUST.md`): the device first, `Cell`s, all-zero
//!   valid. The input ring's four pointers are the index of the buffer being filled and its
//!   fill count, as in `comvar.rs`; the high-water mark and the end are `UART_IHIGHWATER` and
//!   `UART_IBUFSIZE`.
//! - `pluart_cd.cd_devs[unit]` is [`pluart_sc`] (the C's `pluart_sc(dev)` and
//!   `pluart_cd.cd_devs[DEVUNIT(..)]`); the entry points answer `ENXIO` for a unit that is not
//!   there where the C (in `pluartclose`, `pluart_param`, `pluart_start`) would dereference
//!   NULL.
//! - `UART_DR_DATA(x)` and the other function-like macros are `const fn`s.
//! - `pluartcn_remap` is not in the C: it maps the console registers again when arm64's
//!   `pmap_init` drops the bootstrap identity map of the lower half (`arm64/pmap.rs`).
//! - The interrupt is established under the softc's name, read as a `'static` string from
//!   `dv_xname` ([`Device::xname`]).

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI32, AtomicU32, AtomicUsize, Ordering};

use libkern::StaticCell;

use crate::dev::cons::{CN_MIDPRI, Consdev, cn_tab, set_cn_tab};
use crate::dev::ic::com::comopen;
use crate::kern::kern_prot::suser;
use crate::kern::kern_softintr::{SoftintrHand, softintr_establish, softintr_schedule};
use crate::kern::kern_synch::wakeup;
use crate::kern::kern_timeout::{timeout_add_sec, timeout_set};
use crate::kern::subr_prf::{DB_CONSOLE, panic, printf};
use crate::kern::tty::{
    TTOPEN, ttioctl, ttsetwater, ttwakeupwr, ttychars, ttyclose, ttymalloc, ttysleep,
};
use crate::kern::tty_conf::linesw;
use crate::kern::tty_subr::getc;
use crate::machine::bus::{
    BusAddr, BusSize, BusSpaceHandle, BusSpaceTag, bus_space_map, bus_space_read_2,
    bus_space_read_4, bus_space_write_4,
};
use crate::machine::conf::{cdevsw, cdevsw_set, nchrdev};
use crate::machine::cpu::delay;
use crate::machine::db_machdep::db_enter;
use crate::machine::intr::{IPL_TTY, splhigh, spltty, splx};
use crate::sys::conf::{Cdevsw, DevTypeOpen, cdev_tty_init};
use crate::sys::device::{Cfdriver, DV_TTY, Device, Softc};
use crate::sys::errno::Errno;
use crate::sys::fcntl::O_NONBLOCK;
use crate::sys::param::{NODEV, PCATCH};
use crate::sys::proc::Proc;
use crate::sys::syslog::LOG_WARNING;
use crate::sys::termios::{
    B38400, CLOCAL, CRTSCTS, CS5, CS6, CS7, CS8, CSIZE, MDMBUF, Tcflag, Termios,
};
use crate::sys::timeout::Timeout;
use crate::sys::tty::{
    TS_BUSY, TS_CARR_ON, TS_FLUSH, TS_ISOPEN, TS_TIMEOUT, TS_TTSTOP, TS_WOPEN, TS_XCLUDE, TTIPRI,
    TTOPRI, Tty,
};
use crate::sys::ttycom::{
    TIOCCBRK, TIOCCDTR, TIOCGFLAGS, TIOCMBIC, TIOCMBIS, TIOCMGET, TIOCMSET, TIOCSBRK, TIOCSDTR,
    TIOCSFLAGS,
};
use crate::sys::ttydefaults::{TTYDEF_CFLAG, TTYDEF_IFLAG, TTYDEF_LFLAG, TTYDEF_OFLAG};
use crate::sys::types::{Dev, makedev, minor};
use crate::sys::uio::Uio;

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

// sc_hwflags

/// Never enable the interrupt gate.
pub const COM_HW_NOIEN: u8 = 0x01;
/// The FIFO works.
pub const COM_HW_FIFO: u8 = 0x02;
/// Infrared (SIR) port.
pub const COM_HW_SIR: u8 = 0x20;
/// This port is the console.
pub const COM_HW_CONSOLE: u8 = 0x40;
/// An SBSA generic UART (no FIFO level control).
pub const COM_HW_SBSA: u8 = 0x80;

// sc_swflags

/// Soft carrier: ignore DCD.
pub const COM_SW_SOFTCAR: u8 = 0x01;
/// Local line: CLOCAL by default.
pub const COM_SW_CLOCAL: u8 = 0x02;
/// Hardware flow control by default.
pub const COM_SW_CRTSCTS: u8 = 0x04;
/// DTR/DCD flow control by default.
pub const COM_SW_MDMBUF: u8 = 0x08;
/// Pulse-per-second input.
pub const COM_SW_PPS: u8 = 0x10;

/// Size of one input ring buffer, in characters.
pub const UART_IBUFSIZE: usize = 128;
/// Fill level at which input flow control would kick in.
pub const UART_IHIGHWATER: usize = 100;

/// `struct pluart_softc`: the state of one PL011.
#[repr(C)]
pub struct PluartSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `sc_iot`.
    pub sc_iot: Cell<Option<BusSpaceTag>>,
    /// `sc_ioh`.
    pub sc_ioh: Cell<Option<BusSpaceHandle>>,
    /// `sc_si`: the soft interrupt that drains the input ring.
    pub sc_si: Cell<Option<NonNull<SoftintrHand>>>,
    /// `sc_irq`: the interrupt handle.
    pub sc_irq: Cell<*mut c_void>,
    /// `sc_tty`: NULL until the first open.
    pub sc_tty: Cell<*const Tty>,
    /// `sc_diag_tmo`.
    pub sc_diag_tmo: Timeout,
    /// `sc_dtr_tmo`.
    pub sc_dtr_tmo: Timeout,
    /// `sc_overflows`.
    pub sc_overflows: Cell<i32>,
    /// `sc_floods`.
    pub sc_floods: Cell<i32>,
    /// `sc_errors`.
    pub sc_errors: Cell<i32>,
    /// `sc_halt`.
    pub sc_halt: Cell<i32>,
    /// `sc_ucr1`.
    pub sc_ucr1: Cell<u16>,
    /// `sc_ucr2`.
    pub sc_ucr2: Cell<u16>,
    /// `sc_ucr3`.
    pub sc_ucr3: Cell<u16>,
    /// `sc_ucr4`.
    pub sc_ucr4: Cell<u16>,
    /// `sc_hwflags`: `COM_HW_*`.
    pub sc_hwflags: Cell<u8>,
    /// `sc_hwrev`.
    pub sc_hwrev: Cell<u8>,
    /// `sc_swflags`: `COM_SW_*`.
    pub sc_swflags: Cell<u8>,
    /// `sc_fifolen`.
    pub sc_fifolen: Cell<i32>,
    /// `sc_imsc`: the interrupt mask shadow.
    pub sc_imsc: Cell<u32>,
    /// `sc_clkfreq`.
    pub sc_clkfreq: Cell<i32>,

    /// `sc_initialize`.
    pub sc_initialize: Cell<u8>,
    /// `sc_cua`.
    pub sc_cua: Cell<u8>,
    /// `sc_ibuf`: which of `sc_ibufs` is being filled (0 or 1).
    pub sc_ibuf: Cell<usize>,
    /// `sc_ibufp - sc_ibuf`: the characters in it.
    pub sc_ibufp: Cell<usize>,
    /// `sc_ibufs`: the two input ring buffers.
    pub sc_ibufs: [[Cell<u16>; UART_IBUFSIZE]; 2],
    // sc_clk: struct clk (ofw_clock.c, not ported).
}

// SAFETY: `repr(C)` with the device first; every member is a `Cell` of an integer, a raw
// pointer or an `Option` of a reference, handle or pointer, or a `Timeout`, all valid
// all-zero.
unsafe impl Softc for PluartSoftc {}

/// `pluart_cd`.
pub static PLUART_CD: Cfdriver = Cfdriver::new(b"pluart", DV_TTY, 0);

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

/// `pluartdev`: `cdev_tty_init(3/*XXX NUART */ ,pluart)`, the character device the console
/// installs in com's slot.
pub const PLUARTDEV: Cdevsw = cdev_tty_init(
    3, // XXX NUART
    pluartopen,
    pluartclose,
    pluartread,
    pluartwrite,
    pluartioctl,
    pluartstop,
    pluarttty,
);

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

/// `DEVUNIT(x)`.
const fn devunit(x: Dev) -> i32 {
    (minor(x) & 0x7f) as i32
}

/// `DEVCUA(x)`.
const fn devcua(x: Dev) -> bool {
    minor(x) & 0x80 != 0
}

/// The console's tag and handle, once `pluartcnattach` set them.
fn pluartcons_io() -> Option<(BusSpaceTag, BusSpaceHandle)> {
    // SAFETY: both cells are written by `pluartcnattach` on the boot CPU before the console is
    // used, and only read afterwards.
    unsafe { Some((PLUARTCONSIOT.read()?, PLUARTCONSIOH.read()?)) }
}

/// `bus_space_read_4(sc->sc_iot, sc->sc_ioh, reg)`; 0 before the registers are mapped.
fn read4(sc: &PluartSoftc, reg: BusSize) -> u32 {
    match (sc.sc_iot.get(), sc.sc_ioh.get()) {
        (Some(t), Some(h)) => bus_space_read_4(t, h, reg),
        _ => 0,
    }
}

/// `bus_space_write_4(sc->sc_iot, sc->sc_ioh, reg, v)`; nothing before the registers are
/// mapped.
fn write4(sc: &PluartSoftc, reg: BusSize, v: u32) {
    if let (Some(t), Some(h)) = (sc.sc_iot.get(), sc.sc_ioh.get()) {
        bus_space_write_4(t, h, reg, v);
    }
}

/// `pluart_attach_common`: what every attachment shares: the FIFO, the console's device
/// number, the timers and the soft interrupt, the receive interrupts unmasked.
pub fn pluart_attach_common(sc: &'static PluartSoftc, console: bool) {
    let fifolen;
    if sc.sc_hwflags.get() & COM_HW_SBSA == 0 {
        if sc.sc_hwrev.get() == 0 {
            sc.sc_hwrev.set(uart_pid2_rev(read4(sc, UART_PID2)) as u8);
        }
        fifolen = if sc.sc_hwrev.get() < 3 {
            UART_FIFO_SIZE
        } else {
            UART_FIFO_SIZE_R3
        };
        printf(format_args!(
            ": rev {}, {} byte fifo\n",
            sc.sc_hwrev.get(),
            fifolen
        ));
    } else {
        // The SBSA UART is PL011 r1p5 compliant which implies revision 3 with a 32 byte
        // FIFO. However, we cannot expect to configure RX/TX interrupt levels using the
        // UARTIFLS register making it impossible to make assumptions about the number of
        // available bytes in the FIFO. Therefore disable FIFO support for such devices.
        fifolen = 0;
        printf(format_args!("\n"));
    }

    if console {
        // Locate the major number.
        let n = nchrdev();
        let maj = (0..n)
            .find(|&maj| ptr::fn_addr_eq(cdevsw(maj).d_open, pluartopen as DevTypeOpen))
            .unwrap_or(n);
        if let Some(cn) = cn_tab() {
            cn.cn_dev.set(makedev(maj, sc.sc_dev.dv_unit.get() as u32));
        }

        printf(format_args!("{}: console\n", sc.sc_dev.xname()));
        sc.sc_hwflags.set(sc.sc_hwflags.get() | COM_HW_CONSOLE);
    }

    let arg = ptr::from_ref(sc).cast_mut().cast::<c_void>();
    timeout_set(&sc.sc_diag_tmo, pluart_diag, arg);
    timeout_set(&sc.sc_dtr_tmo, pluart_raisedtr, arg);
    sc.sc_si
        .set(softintr_establish(IPL_TTY, pluart_softint, arg));

    if sc.sc_si.get().is_none() {
        panic(format_args!(
            "{}: can't establish soft interrupt.",
            sc.sc_dev.xname()
        ));
    }

    // Flush transmit before enabling FIFO.
    loop {
        let fr = read4(sc, UART_FR);
        if fr & UART_FR_TXFE != 0 {
            break;
        }
        delay(100);
    }

    if fifolen > 0 {
        write4(
            sc,
            UART_IFLS,
            (UART_IFLS_3_4 << UART_IFLS_RX_SHIFT) | (UART_IFLS_1_4 << UART_IFLS_TX_SHIFT),
        );
    }
    sc.sc_imsc.set(UART_IMSC_RXIM | UART_IMSC_RTIM);
    write4(sc, UART_IMSC, sc.sc_imsc.get());
    write4(sc, UART_ICR, 0x7ff);

    let mut lcr = read4(sc, UART_LCR_H);
    if fifolen > 0 {
        lcr |= UART_LCR_H_FEN;
    } else {
        lcr &= !UART_LCR_H_FEN;
    }
    write4(sc, UART_LCR_H, lcr);
}

/// `pluart_intr`: the hard interrupt: restart output when the transmitter wants more, and
/// buffer what arrived for `pluart_softint`.
pub fn pluart_intr(arg: *mut c_void) -> i32 {
    // SAFETY: `pluart_fdt_attach` established the interrupt with its softc, which outlives
    // the handler.
    let sc = unsafe { &*arg.cast::<PluartSoftc>() };

    let is = read4(sc, UART_MIS);
    write4(sc, UART_ICR, is & !UART_IMSC_TXIM);

    // SAFETY: `sc_tty` is null or `ttymalloc`'s, never freed (the driver has no detach).
    let Some(tp) = (unsafe { sc.sc_tty.get().as_ref() }) else {
        return 0;
    };

    if is & UART_IMSC_RXIM == 0 && is & UART_IMSC_RTIM == 0 && is & UART_IMSC_TXIM == 0 {
        return 0;
    }

    if is & UART_IMSC_TXIM != 0 && tp.t_state_isset(TS_BUSY) {
        tp.t_state_clr(TS_BUSY | TS_FLUSH);
        if sc.sc_halt.get() > 0 {
            wakeup(ptr::from_ref(&tp.t_outq));
        }
        (linesw(tp).l_start)(tp);
    }

    let buf = &sc.sc_ibufs[sc.sc_ibuf.get()];
    let mut p = sc.sc_ibufp.get();

    while read4(sc, UART_FR) & UART_FR_RXFE == 0 {
        let mut c = match (sc.sc_iot.get(), sc.sc_ioh.get()) {
            (Some(t), Some(h)) => bus_space_read_2(t, h, UART_DR),
            _ => break,
        };
        if u32::from(c) & UART_DR_BE != 0 {
            // DDB
            if sc.sc_hwflags.get() & COM_HW_CONSOLE != 0 {
                if DB_CONSOLE.load(Ordering::Relaxed) {
                    db_enter();
                }
                continue;
            }
            c = 0;
        }
        if p >= UART_IBUFSIZE {
            sc.sc_floods.set(sc.sc_floods.get() + 1);
            let errors = sc.sc_errors.get();
            sc.sc_errors.set(errors + 1);
            if errors == 0 {
                timeout_add_sec(&sc.sc_diag_tmo, 60);
            }
        } else {
            buf[p].set(c);
            p += 1;
            // p == sc_ibufhigh && CRTSCTS: XXX, the C's IMXUART_CR3_DSR lines are commented
            // out.
        }
        // XXX - msr stuff ?
    }
    sc.sc_ibufp.set(p);

    if let Some(si) = sc.sc_si.get() {
        softintr_schedule(si);
    }

    1
}

/// `pluart_param`: program the line for `t`.
pub fn pluart_param(tp: &Tty, t: &Termios) -> Result<(), Errno> {
    let Some(sc) = pluart_sc(tp.t_dev.get()) else {
        return Err(Errno::ENXIO);
    };
    let ospeed = t.c_ospeed;

    if t.c_ospeed < 0 || (t.c_ispeed != 0 && t.c_ispeed != t.c_ospeed) {
        return Err(Errno::EINVAL);
    }

    match t.c_cflag & CSIZE {
        CS5 | CS6 => return Err(Errno::EINVAL),
        CS7 | CS8 => {
            // CLR/SET(sc->sc_ucr2, IMXUART_CR2_WS): commented out in the C.
        }
        _ => {}
    }

    // PARENB, STOPB - XXX; ospeed == 0: lower dtr (nothing in the C).

    if sc.sc_clkfreq.get() != 0 && ospeed != 0 && ospeed != tp.t_ospeed() {
        while tp.t_state_isset(TS_BUSY) {
            sc.sc_halt.set(sc.sc_halt.get() + 1);
            let error = ttysleep(
                tp,
                ptr::from_ref(&tp.t_outq).cast(),
                TTOPRI | PCATCH,
                "pluartprm",
            );
            sc.sc_halt.set(sc.sc_halt.get() - 1);
            if let Err(e) = error {
                pluart_start(tp);
                return Err(e);
            }
        }

        // Writes to IBRD and FBRD are made effective first when LCR_H is written.
        let lcr = read4(sc, UART_LCR_H);

        // The UART must be disabled while changing the baud rate.
        let cr = read4(sc, UART_CR);
        write4(sc, UART_CR, cr & !UART_CR_UARTEN);

        // The baud rate divisor is expressed relative to the UART clock frequency where
        // IBRD represents the quotient using 16 bits and FBRD the remainder using 6 bits.
        // The PL011 specification provides the following formula:
        //
        //	uartclk/(16 * baudrate)
        //
        // The formula can be estimated by scaling it with the precision 64 (2^6) and
        // letting the resulting upper 16 bits represents the quotient and the lower 6 bits
        // the remainder:
        //
        //	64 * uartclk/(16 * baudrate) = 4 * uartclk/baudrate
        let div = (4 * sc.sc_clkfreq.get() / ospeed) as u32;
        write4(sc, UART_IBRD, uart_ibrd_divint(div >> 6));
        write4(sc, UART_FBRD, uart_fbrd_divfrac(div));
        // Commit baud rate change.
        write4(sc, UART_LCR_H, lcr);
        // Enable UART.
        write4(sc, UART_CR, cr);
    }

    // setup fifo

    // When not using CRTSCTS, RTS follows DTR.
    // sc->sc_dtr = MCR_DTR;

    // and copy to tty
    tp.set_t_ispeed(t.c_ispeed);
    tp.set_t_ospeed(t.c_ospeed);
    let _oldcflag: Tcflag = tp.t_cflag();
    tp.set_t_cflag(t.c_cflag);

    // If DCD is off and MDMBUF is changed, ask the tty layer if we should stop the device.
    // XXX

    pluart_start(tp);

    Ok(())
}

/// `pluart_start`: start output: fill the transmit FIFO and enable its interrupt.
pub fn pluart_start(tp: &Tty) {
    let Some(sc) = pluart_sc(tp.t_dev.get()) else {
        return;
    };

    let s = spltty();
    'out: {
        if tp.t_state_isset(TS_BUSY) {
            break 'out;
        }
        let stopped = tp.t_state_isset(TS_TIMEOUT | TS_TTSTOP) || {
            ttwakeupwr(tp);
            tp.t_outq.c_cc.get() == 0
        };
        if stopped {
            // Disable transmit interrupt.
            if sc.sc_imsc.get() & UART_IMSC_TXIM != 0 {
                sc.sc_imsc.set(sc.sc_imsc.get() & !UART_IMSC_TXIM);
                write4(sc, UART_IMSC, sc.sc_imsc.get());
            }
            break 'out;
        }
        tp.t_state_set(TS_BUSY);

        // Enable transmit interrupt.
        if sc.sc_imsc.get() & UART_IMSC_TXIM == 0 {
            sc.sc_imsc.set(sc.sc_imsc.get() | UART_IMSC_TXIM);
            write4(sc, UART_IMSC, sc.sc_imsc.get());
        }

        while tp.t_outq.c_cc.get() > 0 {
            let fr = read4(sc, UART_FR);
            if fr & UART_FR_TXFF != 0 {
                break;
            }

            write4(sc, UART_DR, getc(&tp.t_outq) as u32);
        }
    }
    splx(s);
}

/// `pluart_diag`: the `sc_diag_tmo` timeout: report the overflows of the last minute.
pub fn pluart_diag(arg: *mut c_void) {
    // SAFETY: `pluart_attach_common` armed the timeout with its softc, which outlives it.
    let sc = unsafe { &*arg.cast::<PluartSoftc>() };

    let s = spltty();
    sc.sc_errors.set(0);
    let overflows = sc.sc_overflows.replace(0);
    let floods = sc.sc_floods.replace(0);
    splx(s);
    crate::log!(
        LOG_WARNING,
        "{}: {} silo overflow{}, {} ibuf overflow{}\n",
        sc.sc_dev.xname(),
        overflows,
        if overflows == 1 { "" } else { "s" },
        floods,
        if floods == 1 { "" } else { "s" }
    );
}

/// `pluart_raisedtr`: the `sc_dtr_tmo` timeout (its body is commented out in the C).
pub fn pluart_raisedtr(_arg: *mut c_void) {
    // SET(sc->sc_ucr3, IMXUART_CR3_DSR); /* XXX */
    // bus_space_write_4(sc->sc_iot, sc->sc_ioh, IMXUART_UCR3, sc->sc_ucr3);
}

/// `pluart_softint`: the soft interrupt: hand what `pluart_intr` buffered to the line
/// discipline.
pub fn pluart_softint(arg: *mut c_void) {
    // SAFETY: `pluart_attach_common` established the soft interrupt with its softc, which
    // outlives it.
    let Some(sc) = (unsafe { arg.cast::<PluartSoftc>().as_ref() }) else {
        return;
    };
    if sc.sc_ibufp.get() == 0 {
        return;
    }

    // SAFETY: as in `pluart_intr`.
    let tp = unsafe { sc.sc_tty.get().as_ref() };
    let s = spltty();

    let ibuf = sc.sc_ibuf.get();
    let ibufend = sc.sc_ibufp.get();

    let Some(tp) = tp.filter(|tp| ibufend != 0 && tp.t_state_isset(TS_ISOPEN)) else {
        splx(s);
        return;
    };

    sc.sc_ibuf.set(1 - ibuf);
    sc.sc_ibufp.set(0);

    // #if 0: the CRTSCTS IMXUART_CR3_DSR toggle.

    splx(s);

    for cell in &sc.sc_ibufs[ibuf][..ibufend] {
        let c = i32::from(cell.get());
        // The overrun and error bits (IMXUART_RX_*) are commented out in the C.
        let err = 0;
        let c = (c & 0xff) | err;
        (linesw(tp).l_rint)(c, tp);
    }
}

/// `pluartopen`: open a port; the first open gives it a tty.
pub fn pluartopen(dev: Dev, flag: i32, _mode: i32, p: &Proc) -> Result<(), Errno> {
    let unit = devunit(dev);

    if unit >= PLUART_CD.cd_ndevs.get() {
        return Err(Errno::ENXIO);
    }
    let Some(sc) = pluart_sc(dev) else {
        return Err(Errno::ENXIO);
    };

    let s = spltty();
    // SAFETY: as in `pluart_intr`.
    let tp: &'static Tty = match unsafe { sc.sc_tty.get().as_ref() } {
        Some(tp) => tp,
        None => {
            let tp = ttymalloc(0);
            sc.sc_tty.set(tp);
            tp
        }
    };

    splx(s);

    tp.t_oproc.set(Some(pluart_start));
    tp.t_param.set(Some(pluart_param));
    tp.t_dev.set(dev);

    let s = if !tp.t_state_isset(TS_ISOPEN) {
        tp.t_state_set(TS_WOPEN);
        ttychars(tp);
        tp.set_t_iflag(TTYDEF_IFLAG);
        tp.set_t_oflag(TTYDEF_OFLAG);

        if sc.sc_hwflags.get() & COM_HW_CONSOLE != 0 {
            tp.set_t_cflag(PLUARTCONSCFLAG.load(Ordering::Relaxed));
        } else {
            tp.set_t_cflag(TTYDEF_CFLAG);
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
        let rate = if sc.sc_hwflags.get() & COM_HW_CONSOLE != 0 {
            PLUARTCONSRATE.load(Ordering::Relaxed)
        } else {
            PLUARTDEFAULTRATE.load(Ordering::Relaxed)
        };
        tp.set_t_ispeed(rate);
        tp.set_t_ospeed(rate);

        let s = spltty();

        sc.sc_initialize.set(1);
        let _ = pluart_param(tp, &tp.t_termios.get());
        ttsetwater(tp);
        sc.sc_ibuf.set(0);
        sc.sc_ibufp.set(0);

        // #if 0: the IMXUART_UCR* setup.

        tp.t_state_set(TS_CARR_ON); // XXX
        s
    } else if tp.t_state_isset(TS_XCLUDE) && suser(p).is_err() {
        return Err(Errno::EBUSY);
    } else {
        spltty()
    };

    if devcua(dev) {
        if tp.t_state_isset(TS_ISOPEN) {
            splx(s);
            return Err(Errno::EBUSY);
        }
        sc.sc_cua.set(1);
    } else {
        // tty (not cua) device; wait for carrier if necessary
        if flag & O_NONBLOCK != 0 {
            if sc.sc_cua.get() != 0 {
                // Opening TTY non-blocking... but the CUA is busy
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
                    splx(s);
                    return Err(e);
                }
            }
        }
    }
    splx(s);
    (linesw(tp).l_open)(dev, tp, p)
}

/// `pluartclose`.
pub fn pluartclose(dev: Dev, flag: i32, _mode: i32, p: Option<&Proc>) -> Result<(), Errno> {
    let Some(sc) = pluart_sc(dev) else {
        return Err(Errno::ENXIO);
    };
    // SAFETY: as in `pluart_intr`.
    let Some(tp) = (unsafe { sc.sc_tty.get().as_ref() }) else {
        return Ok(());
    };

    // XXX This is for cons.c.
    if !tp.t_state_isset(TS_ISOPEN) {
        return Ok(());
    }

    let _ = (linesw(tp).l_close)(tp, flag, p);
    let s = spltty();
    if tp.t_state_isset(TS_WOPEN) {
        // tty device is waiting for carrier; drop dtr then re-raise
        timeout_add_sec(&sc.sc_dtr_tmo, 2);
    }
    tp.t_state_clr(TS_BUSY | TS_FLUSH);

    sc.sc_cua.set(0);
    splx(s);
    let _ = ttyclose(tp);

    Ok(())
}

/// `pluartread`.
pub fn pluartread(dev: Dev, uio: &mut Uio<'_>, flag: i32) -> Result<(), Errno> {
    let Some(tty) = pluarttty(dev) else {
        return Err(Errno::ENODEV);
    };

    (linesw(tty).l_read)(tty, uio, flag)
}

/// `pluartwrite`.
pub fn pluartwrite(dev: Dev, uio: &mut Uio<'_>, flag: i32) -> Result<(), Errno> {
    let Some(tty) = pluarttty(dev) else {
        return Err(Errno::ENODEV);
    };

    (linesw(tty).l_write)(tty, uio, flag)
}

/// `pluartioctl`.
pub fn pluartioctl(dev: Dev, cmd: u64, data: &mut [u8], flag: i32, p: &Proc) -> Result<(), Errno> {
    let Some(sc) = pluart_sc(dev) else {
        return Err(Errno::ENODEV);
    };

    // SAFETY: as in `pluart_intr`.
    let Some(tp) = (unsafe { sc.sc_tty.get().as_ref() }) else {
        return Err(Errno::ENXIO);
    };

    if (linesw(tp).l_ioctl)(tp, cmd, data, flag, p)? {
        return Ok(());
    }

    if ttioctl(tp, cmd, data, flag, p)? {
        return Ok(());
    }

    match cmd {
        TIOCSBRK | TIOCCBRK | TIOCSDTR | TIOCCDTR | TIOCMSET | TIOCMBIS | TIOCMBIC | TIOCMGET
        | TIOCGFLAGS => {}
        TIOCSFLAGS => {
            if suser(p).is_err() {
                return Err(Errno::EPERM);
            }
        }
        _ => return Err(Errno::ENOTTY),
    }

    Ok(())
}

/// `pluartstop`.
pub fn pluartstop(_tp: &Tty, _flag: i32) -> Result<(), Errno> {
    Ok(())
}

/// `pluarttty`.
pub fn pluarttty(dev: Dev) -> Option<&'static Tty> {
    let sc = pluart_sc(dev)?;
    // SAFETY: as in `pluart_intr`.
    unsafe { sc.sc_tty.get().as_ref() }
}

/// `pluart_sc`: the softc of the unit `dev` names.
pub fn pluart_sc(dev: Dev) -> Option<&'static PluartSoftc> {
    let unit = devunit(dev);
    let d = PLUART_CD.cd_dev(unit)?;
    // SAFETY: `pluart_cd`'s devices were made by `config_make_softc` from `pluart_fdt_ca`
    // (`ca_devsize` is `size_of::<PluartSoftc>()`), and live forever (no detach).
    Some(unsafe { d.as_ref().softc::<PluartSoftc>() })
}

// serial console

/// `pluartcnprobe`: nothing to probe; the console is attached explicitly.
pub fn pluartcnprobe(_cp: &Consdev) {}

/// `pluartcninit`: nothing to do; `pluartcnattach` did it.
pub fn pluartcninit(_cp: &Consdev) {}

/// `pluartcnattach`: makes the PL011 at `iobase` the console, in com's character device slot.
/// `ENOMEM` when its registers cannot be mapped, `ENXIO` when no `com` slot exists.
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

    // Look for major of com(4) to replace.
    let n = nchrdev();
    let Some(maj) = (0..n).find(|&maj| ptr::fn_addr_eq(cdevsw(maj).d_open, comopen as DevTypeOpen))
    else {
        return Err(Errno::ENXIO);
    };

    set_cn_tab(&PLUARTCONS);
    PLUARTCONS.cn_dev.set(makedev(maj, 0));
    cdevsw_set(maj, PLUARTDEV); // KLUDGE

    PLUARTCONSADDR.store(iobase, Ordering::Relaxed);
    PLUARTCONSCFLAG.store(cflag, Ordering::Relaxed);
    PLUARTCONSRATE.store(rate, Ordering::Relaxed);

    Ok(())
}

/// Maps the console's registers again through `bus_space_map` and switches the console to
/// the new handle: the arm64 `pmap_init` calls it when the bootstrap identity map of the
/// lower half goes away (not in the C, whose early console already lives in the kernel
/// half; see `arch/arm64/arm64/pmap.rs`).
pub fn pluartcn_remap() {
    let Some((iot, _)) = pluartcons_io() else {
        return;
    };
    let iobase = PLUARTCONSADDR.load(Ordering::Relaxed);
    // SAFETY: the same device registers `pluartcnattach` mapped, mapped once more.
    if let Ok(ioh) = unsafe { bus_space_map(iot, iobase, UART_SPACE, 0) } {
        // SAFETY: single writer, on the boot CPU, between two console writes (see
        // `pluartcons_io`).
        unsafe { PLUARTCONSIOH.write(Some(ioh)) };
    }
}

/// `pluartcngetc`: blocks until a character arrives and returns it.
pub fn pluartcngetc(_dev: Dev) -> i32 {
    let Some((iot, ioh)) = pluartcons_io() else {
        return 0;
    };
    let s = splhigh();
    while bus_space_read_4(iot, ioh, UART_FR) & UART_FR_RXFE != 0 {
        core::hint::spin_loop();
    }
    let c = bus_space_read_4(iot, ioh, UART_DR) as i32;
    splx(s);
    c
}

/// `pluartcnputc`: sends one character, waiting for room in the transmit FIFO.
pub fn pluartcnputc(_dev: Dev, c: i32) {
    let Some((iot, ioh)) = pluartcons_io() else {
        return;
    };
    let s = splhigh();
    while bus_space_read_4(iot, ioh, UART_FR) & UART_FR_TXFF != 0 {
        core::hint::spin_loop();
    }
    bus_space_write_4(iot, ioh, UART_DR, u32::from(c as u8));
    splx(s);
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
        assert_eq!(PLUARTDEV.d_type, crate::sys::conf::D_TTY);
    }
}
