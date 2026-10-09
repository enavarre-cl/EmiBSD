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
//! boot(8)'s machine-independent part (`sys/stand/boot`): the main loop, the `boot>` prompt
//! and `boot.conf`, the commands and variables, and the boot arguments handed to the kernel.
//!
//! OpenBSD compiles these files into every boot program (`.PATH: ${S}/stand/boot`); here
//! they are a crate the boot programs link, as libsa is. The program registers its
//! machine-dependent routines ([`boot::boot_md_register`]) and calls [`boot::boot`].

#![no_std]

extern crate alloc;
#[cfg(test)]
extern crate std;

pub mod boot;
pub mod bootarg;
pub mod cmd;
pub mod vars;
/* </CODE> */
