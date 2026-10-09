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
 * Copyright (c) 1997,1998 Michael Shalayeff
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
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
 * WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 */

/*
 * Copyright (c) 1996-1999 Michael Shalayeff
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
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL THE AUTHOR OR HIS RELATIVES BE LIABLE FOR ANY DIRECT,
 * INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
 * (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
 * SERVICES; LOSS OF MIND, USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT,
 * STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING
 * IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF
 * THE POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! The boot arguments: the list of typed records (`bootarg_t`) a boot program collects and
//! copies out for the kernel, which walks them until `BOOTARG_END`.
//!
//! Upstream: sys/stand/boot/bootarg.c @ 3ce1f3f79392, sys/stand/boot/bootarg.h @ 3ce1f3f79392
//!
//! The kernel reads the records in its own layout: `makebootargs` writes the native one
//! (`int ba_type; size_t ba_size; bootarg_t *ba_next; int ba_arg[]`, 64-bit: 28 bytes of
//! header), `makebootargs32` the 32-bit one amd64's kernel is handed by the 32-bit entry
//! (`int ba_type; int ba_size; int ba_nextX; char ba_arg[]`: 12 bytes of header, records
//! packed). `ba_size` covers the header and the argument; `ba_next` is not meaningful in the
//! copy. The list is copied newest first, as the C prepends.
//!
//! ## Deviations
//! - The list (`bootarg_list`, `alloc`ed records linked by `ba_next`) is a `Vec` of
//!   [`Bootarg`] in [`BOOTARG_LIST`], newest last; the copies are built byte by byte in the
//!   C's layouts. `addbootarg` takes the argument as bytes.
//! - `extern void *bootargv; extern int bootargc; extern bootarg_t *bootargp` are the
//!   kernel's (A2) and are not declared here.

use alloc::vec::Vec;

use libkern::staticcell::StaticCell;

/// `BAPIV_ANCIENT`: MD old i386 bootblocks.
pub const BAPIV_ANCIENT: i32 = 0x0000_0000;
/// `BAPIV_VARS`: MD structure w/ add info passed.
pub const BAPIV_VARS: i32 = 0x0000_0001;
/// `BAPIV_VECTOR`: MI vector of MD structures passed.
pub const BAPIV_VECTOR: i32 = 0x0000_0002;
/// `BAPIV_ENV`: MI environment vars vector.
pub const BAPIV_ENV: i32 = 0x0000_0004;
/// `BAPIV_BMEMMAP`: MI memory map passed is in bytes.
pub const BAPIV_BMEMMAP: i32 = 0x0000_0008;
/// `BOOTARG_APIVER`: what this boot program passes.
pub const BOOTARG_APIVER: i32 = BAPIV_VECTOR | BAPIV_ENV | BAPIV_BMEMMAP;

/// `BOOTARG_ENV`.
pub const BOOTARG_ENV: i32 = 0x1000;
/// `BOOTARG_END`: the record that ends the list.
pub const BOOTARG_END: i32 = -1;

/// `offsetof(bootarg_t, ba_arg)` on a 64-bit machine: `ba_type`, padding, `ba_size`,
/// `ba_next`.
pub const BOOTARG_HDR64: usize = 24;
/// `sizeof(bootarg_t) - sizeof(ba_arg)`: the 64-bit header as `ba_size` counts it (the
/// structure is 32 bytes with `int ba_arg[1]` and its padding).
pub const BOOTARG_SIZE64: usize = 32 - 4;
/// `sizeof(struct bootarg32)` (12 bytes and `char ba_arg[1]`, padded to 16).
pub const BOOTARG32_SIZE: usize = 16;
/// The 32-bit record's header (`ba_type`, `ba_size`, `ba_nextX`).
pub const BOOTARG32_HDR: usize = 12;

/// `bootarg_t`, as the list keeps it: the type and the argument.
pub struct Bootarg {
    /// `ba_type`.
    pub ba_type: i32,
    /// `ba_arg`.
    pub ba_arg: Vec<u8>,
}

impl Bootarg {
    /// `ba_size`: header and argument, as the C counts it.
    pub fn ba_size(&self) -> usize {
        BOOTARG_SIZE64 + self.ba_arg.len()
    }
}

/// `bootarg_list`, oldest first.
pub static BOOTARG_LIST: StaticCell<Vec<Bootarg>> = StaticCell::new(Vec::new());

/// The list.
///
/// # Safety
///
/// No other reference to [`BOOTARG_LIST`] may be live (boot programs are single-threaded).
unsafe fn list() -> &'static mut Vec<Bootarg> {
    // SAFETY: the caller's contract.
    unsafe { BOOTARG_LIST.get_mut() }
}

/// `addbootarg(t, l, p)`: add a record of type `t` holding `p`.
pub fn addbootarg(t: i32, p: &[u8]) {
    // SAFETY: entry point; no other reference to the list is live.
    unsafe { list() }.push(Bootarg {
        ba_type: t,
        ba_arg: p.to_vec(),
    });
}

/// `makebootargs(v, &len)`: copy the records, newest first, in the native 64-bit layout into
/// `v`, as many as fit with the end record; the length used.
pub fn makebootargs(v: &mut [u8]) -> usize {
    // SAFETY: entry point; no other reference to the list is live.
    let list = unsafe { list() };
    let mut l = BOOTARG_SIZE64 + 4; // sizeof(*p)
    let mut n = 0;
    for p in list.iter().rev() {
        if v.len() < l + p.ba_size() {
            break;
        }
        l += p.ba_size();
        n += 1;
    }
    let mut q = 0;
    for p in list.iter().rev().take(n) {
        let size = p.ba_size();
        v[q..q + 4].copy_from_slice(&p.ba_type.to_ne_bytes());
        v[q + 4..q + 8].fill(0);
        v[q + 8..q + 16].copy_from_slice(&(size as u64).to_ne_bytes());
        v[q + 16..q + 24].copy_from_slice(&0u64.to_ne_bytes());
        v[q + BOOTARG_HDR64..q + BOOTARG_HDR64 + p.ba_arg.len()].copy_from_slice(&p.ba_arg);
        q += size;
    }
    v[q..q + 4].copy_from_slice(&BOOTARG_END.to_ne_bytes());
    l
}

/// `makebootargs32(v, &len)`: as [`makebootargs`], in the 32-bit layout (`struct
/// bootarg32`), records packed one after the other; the length used.
pub fn makebootargs32(v: &mut [u8]) -> usize {
    // SAFETY: entry point; no other reference to the list is live.
    let list = unsafe { list() };
    let size32 = |p: &Bootarg| BOOTARG32_HDR + p.ba_arg.len();
    // get total size
    let mut l = BOOTARG32_SIZE;
    let mut n = 0;
    for p in list.iter().rev() {
        if v.len() < l + size32(p) {
            break;
        }
        l += size32(p);
        n += 1;
    }
    // copy them out
    let mut q = 0;
    for p in list.iter().rev().take(n) {
        if q + size32(p) > l - BOOTARG32_SIZE {
            break;
        }
        v[q..q + 4].copy_from_slice(&p.ba_type.to_ne_bytes());
        v[q + 4..q + 8].copy_from_slice(&(size32(p) as i32).to_ne_bytes());
        v[q + 8..q + 12].fill(0);
        v[q + BOOTARG32_HDR..q + size32(p)].copy_from_slice(&p.ba_arg);
        q += size32(p);
    }
    v[q..q + 4].copy_from_slice(&BOOTARG_END.to_ne_bytes());
    l
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_newest_first_in_both_layouts() {
        // SAFETY: this test is the only user of the list in this test binary.
        unsafe { list() }.clear();
        addbootarg(9, &[1, 2, 3, 4, 5, 6, 7, 8]);
        addbootarg(3, &16u32.to_ne_bytes());
        let mut v = [0xffu8; 64];
        let l = makebootargs32(&mut v);
        assert_eq!(l, 16 + 16 + 20);
        assert_eq!(v[..16], [3, 0, 0, 0, 16, 0, 0, 0, 0, 0, 0, 0, 16, 0, 0, 0]);
        assert_eq!(v[16..24], [9, 0, 0, 0, 20, 0, 0, 0]);
        assert_eq!(v[28..36], [1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(v[36..40], [0xff; 4]);
        let mut w = [0u8; 128];
        let l = makebootargs(&mut w);
        assert_eq!(l, 32 + 32 + 36);
        assert_eq!(w[8..16], 32u64.to_ne_bytes());
        assert_eq!(w[32 + 24..32 + 32], [1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(w[68..72], (-1i32).to_ne_bytes());
    }
}
/* </TESTS> */
