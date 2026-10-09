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
//! amd64 `<machine/mpconfig.h>`: definitions originally from the mpbios code, but now used
//! for ACPI MP config as well.
//!
//! Upstream: sys/arch/amd64/include/mpconfig.h @ 3ce1f3f79392
//!
//! The original file carries no licence text, only its `$OpenBSD$` and `$NetBSD$` lines
//! (kept above); every notice in the reference tree is accepted (the user's rule of
//! 2026-10-04).
//!
//! ## Deviations
//! - `struct mp_bus` and `struct mp_intr_map` are defined by the machine contract
//!   (`sys/machine/mpconfig.rs`), because the machine-independent `dev/acpi` drivers build
//!   them; this module re-exports them. `mb_intr_print`/`mb_intr_cfg` wait for `mpbios.c`.
//! - The `extern` globals (`mp_verbose`, `mp_busses`, `mp_nbusses`, `mp_intrs`, `mp_nintrs`,
//!   `mp_isa_bus`, `mp_eisa_bus`) are defined, as in C, by `amd64/mainbus.rs`.

pub use crate::machine::mpconfig::{MpBus, MpIntrMap};
/* </CODE> */
