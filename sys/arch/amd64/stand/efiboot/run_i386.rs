/*	$OpenBSD: run_i386.S,v 1.3 2022/12/08 01:25:44 guenther Exp $	*/
/*	$OpenBSD: run_i386.h,v 1.1 2015/09/02 01:52:26 yasuoka Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2015 YASUOKA Masahiko <yasuoka@yasuoka.net>
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
 * Copyright (c) 2015 YASUOKA Masahiko <yasuoka@yasuoka.net>
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
//! `run_i386`: the trampoline that leaves 64-bit mode for 32-bit protected mode without
//! paging and calls the kernel's 32-bit `start` with its arguments on a stack of its own
//! (`run_i386.S`); efiboot copies it to the heap and calls the copy.
//!
//! Upstream: sys/arch/amd64/stand/efiboot/run_i386.S @ 3ce1f3f79392,
//! sys/arch/amd64/stand/efiboot/run_i386.h @ 3ce1f3f79392

use core::arch::global_asm;

global_asm!(include_str!("run_i386.S"), options(att_syntax));

unsafe extern "C" {
    /// `run_i386_start`: the first byte of the trampoline (only its address is used).
    #[link_name = "run_i386_start"]
    static RUN_I386_START: u8;
    /// `run_i386_size`: the trampoline's size, its stack included.
    #[link_name = "run_i386_size"]
    static RUN_I386_SIZE: u32;
}

/// The address of `run_i386_start`.
pub fn run_i386_start() -> *const u8 {
    &raw const RUN_I386_START
}

/// `run_i386_size`.
pub fn run_i386_size() -> u32 {
    // SAFETY: a 32-bit constant the assembly defines.
    unsafe { RUN_I386_SIZE }
}
/* </CODE> */
