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
//! Media-independent interface PHYs: OpenBSD `sys/dev/mii/`.
//!
//! The mii(4) layer (`mii.c`, `mii_physubr.c`, `<dev/mii/mii.h>`, `<dev/mii/miivar.h>`), the
//! PHY drivers the GENERICs attach at `mii?` that are ported (`inphy`, `rlphy`, `rgephy`, `ukphy` with
//! `ukphy_subr`, `lxtphy`, `dcphy`), the ids they match (`miidevs.h`), `rgephyreg`, `inphyreg`
//! and `lxtphyreg`.

pub mod dcphy;
pub mod inphy;
pub mod inphyreg;
pub mod lxtphy;
pub mod lxtphyreg;
#[allow(clippy::module_inception)] // OpenBSD's layout: sys/dev/mii/mii.c
pub mod mii;
pub mod mii_physubr;
pub mod miidevs;
pub mod miivar;
pub mod rgephy;
pub mod rgephyreg;
pub mod rlphy;
pub mod ukphy;
pub mod ukphy_subr;
/* </CODE> */
