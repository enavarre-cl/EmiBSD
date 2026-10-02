//! Milestone M0 bootstrap console: COM1, polled, through port I/O.
//!
//! Not an OpenBSD file. OpenBSD drives this UART with `com(4)` (`dev/ic/com.c`, register
//! definitions in `dev/ic/comreg.h`) and attaches the console from `machdep.c`'s `consinit()`.
//! Those arrive with milestone M2 and replace this module; until then the handful of 16550
//! registers it needs are spelled out here.

use super::super::include::pio::{inb, outb};

/// COM1's I/O base. TODO(M2): `comreg.rs`. TODO(M4): discover the port through the firmware.
const COM1: u16 = 0x3f8;

/// Transmitter holding register (write).
const THR: u16 = 0;
/// Divisor latch, low byte (with DLAB set).
const DLL: u16 = 0;
/// Interrupt enable register.
const IER: u16 = 1;
/// Divisor latch, high byte (with DLAB set).
const DLM: u16 = 1;
/// FIFO control register.
const FCR: u16 = 2;
/// Line control register.
const LCR: u16 = 3;
/// Modem control register.
const MCR: u16 = 4;
/// Line status register.
const LSR: u16 = 5;

/// LCR: divisor latch access bit.
const LCR_DLAB: u8 = 0x80;
/// LCR: 8 data bits, no parity, 1 stop bit.
const LCR_8N1: u8 = 0x03;
/// FCR: enable FIFOs, clear both, 14-byte receive trigger.
const FCR_ENABLE_CLEAR_TRIGGER14: u8 = 0xc7;
/// MCR: DTR, RTS and OUT2 (the interrupt gate on PC hardware) asserted.
const MCR_DTR_RTS_OUT2: u8 = 0x0b;
/// LSR: transmitter holding register empty.
const LSR_THRE: u8 = 0x20;

/// 115200 baud: divisor 1 of the 1.8432 MHz reference divided by 16.
const DIVISOR: u16 = 1;

/// Programs COM1 for 115200 baud, 8N1, FIFOs on, interrupts off.
///
/// # Safety
///
/// Call once, before [`putc`], as the only user of the COM1 port range.
pub unsafe fn init() {
    // SAFETY: the caller guarantees exclusive use of COM1; these are the 16550's documented
    // registers, written in the standard initialisation order.
    unsafe {
        outb(COM1 + IER, 0);
        outb(COM1 + LCR, LCR_DLAB);
        outb(COM1 + DLL, (DIVISOR & 0xff) as u8);
        outb(COM1 + DLM, (DIVISOR >> 8) as u8);
        outb(COM1 + LCR, LCR_8N1);
        outb(COM1 + FCR, FCR_ENABLE_CLEAR_TRIGGER14);
        outb(COM1 + MCR, MCR_DTR_RTS_OUT2);
    }
}

/// Writes one byte, after waiting for the transmitter holding register to empty.
pub fn putc(c: u8) {
    // SAFETY: COM1 was programmed by `init`; polling LSR and writing THR is the 16550's
    // documented transmit sequence and touches nothing else.
    unsafe {
        while inb(COM1 + LSR) & LSR_THRE == 0 {
            core::hint::spin_loop();
        }
        outb(COM1 + THR, c);
    }
}
