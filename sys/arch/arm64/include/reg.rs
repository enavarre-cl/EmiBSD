/* $OpenBSD: reg.h,v 1.4 2024/03/30 09:17:51 kettenis Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2016 Dale Rahn <drahn@dalerahn.com>
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

//! arm64 `<machine/reg.h>`: the register sets `ptrace(2)` and the pcb use.
//!
//! Upstream: sys/arch/arm64/include/reg.h @ 3ce1f3f79392
//!
//! Status: `ported` (M5).

/// `struct reg`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Reg {
    /// `r_reg`.
    pub r_reg: [u64; 30],
    /// `r_lr`.
    pub r_lr: u64,
    /// `r_sp`.
    pub r_sp: u64,
    /// `r_pc`.
    pub r_pc: u64,
    /// `r_spsr`.
    pub r_spsr: u64,
    /// `r_tpidr`.
    pub r_tpidr: u64,
}

/// `struct fpreg`: the floating-point state.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Fpreg {
    /// `fp_reg`.
    pub fp_reg: [u128; 32],
    /// `fp_sr`.
    pub fp_sr: u32,
    /// `fp_cr`.
    pub fp_cr: u32,
}

impl Fpreg {
    /// All zero.
    pub const fn zeroed() -> Self {
        Self {
            fp_reg: [0; 32],
            fp_sr: 0,
            fp_cr: 0,
        }
    }
}
