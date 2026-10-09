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
 * Copyright (c) 2003 Wasabi Systems, Inc.
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

/*
 * Copyright (c) 1998 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Charles M. Hannum.
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
 * THIS SOFTWARE IS PROVIDED BY THE NETBSD FOUNDATION, INC. AND CONTRIBUTORS
 * ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE FOUNDATION OR CONTRIBUTORS
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! The spl loops of `arch/amd64/amd64/spl.S`, pulled in from the `.S` file next to this
//! module (the file keeps OpenBSD's licence blocks and layout; `NAME` between braces is what
//! `assym.h` provides in C).
//!
//! Upstream: sys/arch/amd64/amd64/spl.S @ 3ce1f3f79392
//!
//! Status: `ported`. `Xspllower` and `Xdoreti`; the `#if 0` profiling `splhigh`/`splx` are
//! dead code in C too.
//!
//! ## Deviations
//! - No `RETGUARD_*` (an OpenBSD compiler feature), no `_PROF_PROLOGUE` (no `GPROF`), and the
//!   `retpoline_rax` thunk (a `CODEPATCH`ed Spectre mitigation) is a plain `jmp *%rax`:
//!   `codepatch.c` is not ported.
//! - `intr_user_exit` is `locore.S`'s stub until user mode (M6).
//! - AT&T syntax, as the C file, so the two can be diffed.

use core::arch::global_asm;
use core::mem::offset_of;

use crate::arch::amd64::include::cpu::CpuInfo;
use crate::arch::amd64::include::frame::{Intrframe, Trapframe};
use crate::arch::amd64::include::intr::Intrsource;
use crate::arch::amd64::include::segments::SEL_RPL;

global_asm!(
    include_str!("spl.S"),
    CI_IUNMASK = const offset_of!(CpuInfo, ci_iunmask),
    CI_IPENDING = const offset_of!(CpuInfo, ci_ipending),
    CI_ILEVEL = const offset_of!(CpuInfo, ci_ilevel),
    CI_ISOURCES = const offset_of!(CpuInfo, ci_isources),
    CI_IDEPTH = const offset_of!(CpuInfo, ci_idepth),
    IS_RECURSE = const offset_of!(Intrsource, is_recurse),
    IS_RESUME = const offset_of!(Intrsource, is_resume),
    IF_PPL = const offset_of!(Intrframe, if_ppl),
    TF_CS = const offset_of!(Trapframe, tf_cs),
    SEL_RPL = const SEL_RPL,
    options(att_syntax)
);

unsafe extern "C" {
    /// `Xspllower(nlevel)`: lowers the IPL to `nlevel`, running the pending interrupts the
    /// new level unmasks; returns with interrupts enabled.
    ///
    /// # Safety
    ///
    /// Call with interrupts disabled, from kernel context, with `nlevel` a valid IPL.
    pub fn Xspllower(nlevel: i32);
}
/* </CODE> */
