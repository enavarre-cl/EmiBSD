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
/* </LICENSES> */

/* <CODE> */
//! The ISA bus: OpenBSD `sys/dev/isa/`: the bus itself (`isa.c`, `isavar.h`), the register
//! map amd64's timer code needs (`isareg.h`) and `com(4)`'s attachment (`com_isa.c`),
//! `pckbc(4)`'s (`pckbc_isa.c`).

pub mod com_isa;
#[allow(clippy::module_inception)] // OpenBSD's layout: sys/dev/isa/isa.c
pub mod isa;
pub mod isareg;
pub mod isavar;
pub mod lpt_isa;
pub mod pckbc_isa;
pub mod pcppi;
pub mod pcppireg;
pub mod pcppivar;
pub mod spkr;
pub mod spkrio;
pub mod vga_isa;
/* </CODE> */
