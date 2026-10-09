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
 * Copyright (c) 1997 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Jason R. Thorpe of the Numerical Aerospace Simulation Facility,
 * NASA Ames Research Center.
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
//! NE2000-compatible softc (`dev/ic/ne2000var.h`).
//!
//! Upstream: sys/dev/ic/ne2000var.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `struct ne2000_softc` is `#[repr(C)]` with the [`Dp8390Softc`] first (so the C's casts
//!   between the two are [`ne2000_softc`] in `ne2000.rs`), its bus space tag and handle are
//!   `Option`s in `Cell`s, and the anonymous `enum` of `sc_type` is [`Ne2000Type`], whose
//!   `NE2000_TYPE_UNKNOWN` (0) is the all-zero value of a fresh softc.
//! - The prototypes are `ne2000.rs`'s functions.

use core::cell::Cell;

use crate::dev::ic::dp8390var::Dp8390Softc;
use crate::machine::bus::{BusSpaceHandle, BusSpaceTag};
use crate::sys::device::Softc;

/// The `sc_type` enum of `struct ne2000_softc`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum Ne2000Type {
    /// `NE2000_TYPE_UNKNOWN`.
    Unknown = 0,
    /// `NE2000_TYPE_NE1000`.
    Ne1000 = 1,
    /// `NE2000_TYPE_NE2000`.
    Ne2000 = 2,
    /// `NE2000_TYPE_DL10019`.
    Dl10019 = 3,
    /// `NE2000_TYPE_DL10022`.
    Dl10022 = 4,
    /// `NE2000_TYPE_AX88190`.
    Ax88190 = 5,
    /// `NE2000_TYPE_AX88790`.
    Ax88790 = 6,
}

/// `struct ne2000_softc`.
#[repr(C)]
pub struct Ne2000Softc {
    /// `sc_dp8390`.
    pub sc_dp8390: Dp8390Softc,

    /// `sc_asict`: space tag for ASIC.
    pub sc_asict: Cell<Option<BusSpaceTag>>,
    /// `sc_asich`: space handle for ASIC.
    pub sc_asich: Cell<Option<BusSpaceHandle>>,

    /// `sc_type`.
    pub sc_type: Cell<Ne2000Type>,
    /// `sc_useword`.
    pub sc_useword: Cell<i32>,
}

// SAFETY: `#[repr(C)]` with the `Dp8390Softc` (a `Softc`, its device first) first; the other
// members are `Cell`s of `Option`s of a tag/handle, of an enum whose zero value exists, and
// of an integer, all-zero valid.
unsafe impl Softc for Ne2000Softc {}
/* </CODE> */
