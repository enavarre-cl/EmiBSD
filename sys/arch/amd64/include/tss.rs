/*	$OpenBSD: tss.h,v 1.6 2023/07/27 00:30:07 guenther Exp $	*/
/*	$NetBSD: tss.h,v 1.1 2003/04/26 18:39:48 fvdl Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2001 Wasabi Systems, Inc.
 * All rights reserved.
 *
 * Written by Frank van der Linden for Wasabi Systems, Inc.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *      This product includes software developed for the NetBSD Project by
 *      Wasabi Systems, Inc.
 * 4. The name of Wasabi Systems, Inc. may not be used to endorse
 *    or promote products derived from this software without specific prior
 *    written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY WASABI SYSTEMS, INC. ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL WASABI SYSTEMS, INC
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

//! amd64 `<machine/tss.h>`: the task state segment.
//!
//! Upstream: sys/arch/amd64/include/tss.h @ 3ce1f3f79392

/// `struct x86_64_tss`: the 64-bit TSS, as the hardware reads it.
#[repr(C, packed)]
pub struct X86_64Tss {
    /// Reserved.
    pub tss_reserved1: u32,
    /// The stack for ring 0.
    pub tss_rsp0: u64,
    /// The stack for ring 1.
    pub tss_rsp1: u64,
    /// The stack for ring 2.
    pub tss_rsp2: u64,
    /// Reserved.
    pub tss_reserved2: u32,
    /// Reserved.
    pub tss_reserved3: u32,
    /// The interrupt stacks (`gd_ist` 1 to 7).
    pub tss_ist: [u64; 7],
    /// Reserved.
    pub tss_reserved4: u32,
    /// Reserved.
    pub tss_reserved5: u32,
    /// Reserved.
    pub tss_reserved6: u16,
    /// The I/O permission bitmap's offset.
    pub tss_iobase: u16,
}

impl X86_64Tss {
    /// A zeroed TSS.
    pub const fn new() -> Self {
        Self {
            tss_reserved1: 0,
            tss_rsp0: 0,
            tss_rsp1: 0,
            tss_rsp2: 0,
            tss_reserved2: 0,
            tss_reserved3: 0,
            tss_ist: [0; 7],
            tss_reserved4: 0,
            tss_reserved5: 0,
            tss_reserved6: 0,
            tss_iobase: 0,
        }
    }
}

impl Default for X86_64Tss {
    fn default() -> Self {
        Self::new()
    }
}

const _: () = assert!(size_of::<X86_64Tss>() == 104);
