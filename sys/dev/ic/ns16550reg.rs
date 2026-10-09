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
 *	@(#)ns16550.h	7.1 (Berkeley) 5/9/91
 */
/* </LICENSES> */

/* <CODE> */
//! NS16550 (and above) UART registers: `<dev/ic/ns16550reg.h>`.
//!
//! Upstream: sys/dev/ic/ns16550reg.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The C names are lowercase (`com_data`); Rust constants are upper-case (`COM_DATA`).

use crate::machine::bus::BusSize;

/// Data register (R/W).
pub const COM_DATA: BusSize = 0;
/// Divisor latch low (W).
pub const COM_DLBL: BusSize = 0;
/// Divisor latch high (W).
pub const COM_DLBH: BusSize = 1;
/// Interrupt enable (W).
pub const COM_IER: BusSize = 1;
/// Interrupt identification (R).
pub const COM_IIR: BusSize = 2;
/// FIFO control (W).
pub const COM_FIFO: BusSize = 2;
/// Extended FIFO control (W).
pub const COM_FCTL: BusSize = 2;
/// Extended features register (W).
pub const COM_EFR: BusSize = 2;
/// Line control register (R/W).
pub const COM_LCTL: BusSize = 3;
/// Line control register (R/W).
pub const COM_CFCR: BusSize = 3;
/// Modem control register (R/W).
pub const COM_MCR: BusSize = 4;
/// Line status register (R/W).
pub const COM_LSR: BusSize = 5;
/// Modem status register (R/W).
pub const COM_MSR: BusSize = 6;
/// Scratch register (R/W).
pub const COM_SCRATCH: BusSize = 7;

// Synopsys DesignWare APB UART additional registers

/// UART status register (R).
pub const COM_USR: BusSize = 31;
/// Component parameter register (R).
pub const COM_CPR: BusSize = 61;
/* </CODE> */
