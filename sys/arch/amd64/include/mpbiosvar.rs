/* <LICENSES> */
/*-
 * Copyright (c) 2000 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by RedBack Networks Inc.
 *
 * Author: Bill Sommerfeld
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
//! `<machine/mpbiosvar.h>`: where the application processors' real-mode trampoline lives.
//!
//! Upstream: sys/arch/amd64/include/mpbiosvar.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `mpbios_probe`, `mpbios_intr_fixup` and `mpbios_scanned` belong to `mpbios.c` (the MP
//!   tables), which is not ported; `<machine/mpbiosreg.h>` and `<machine/mpconfig.h>` come
//!   with it.

use crate::arch::amd64::include::param::PAGE_SIZE;

/// `MP_TRAMPOLINE`: the page `mptramp.S`'s real-mode code is copied to; the STARTUP IPI's
/// vector is its page number.
pub const MP_TRAMPOLINE: usize = 16 * PAGE_SIZE;
/// `MP_TRAMP_DATA`: the page of the trampoline's data and stack.
pub const MP_TRAMP_DATA: usize = 17 * PAGE_SIZE;
/* </CODE> */
