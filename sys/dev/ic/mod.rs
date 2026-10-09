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
//! Bus-independent chip drivers: OpenBSD `sys/dev/ic/`.

pub mod ac97;
pub mod ahci;
pub mod ahcireg;
pub mod ahcivar;
pub mod am79900reg;
pub mod ax88190reg;
pub mod com;
pub mod comreg;
pub mod comvar;
pub mod dc;
pub mod dcreg;
pub mod dp8390;
pub mod dp8390reg;
pub mod dp8390var;
pub mod fxp;
pub mod fxpreg;
pub mod fxpvar;
pub mod i8042reg;
pub mod i8237reg;
pub mod i8253reg;
pub mod lancereg;
pub mod lpt;
pub mod lptreg;
pub mod lptvar;
pub mod mc146818reg;
pub mod mc6845reg;
pub mod mpi;
pub mod mpireg;
pub mod mpivar;
pub mod ne2000;
pub mod ne2000reg;
pub mod ne2000var;
pub mod nec765reg;
pub mod ns16550reg;
pub mod nvme;
pub mod nvmeio;
pub mod nvmereg;
pub mod nvmevar;
pub mod pcdisplay;
pub mod pcdisplay_chars;
pub mod pcdisplay_subr;
pub mod pcdisplayvar;
pub mod pckbc;
pub mod pckbcvar;
pub mod pluart;
pub mod re;
pub mod rtl80x9;
pub mod rtl80x9reg;
pub mod rtl81x9reg;
pub mod siop;
pub mod siop_common;
pub mod siopreg;
pub mod siopvar;
pub mod siopvar_common;
pub mod vga;
pub mod vga_subr;
pub mod vgareg;
pub mod vgavar;
pub mod wdc;
pub mod wdcevent;
pub mod wdcreg;
pub mod wdcvar;
/* </CODE> */
