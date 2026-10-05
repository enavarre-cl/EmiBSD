/*	$OpenBSD: mpconfig.h,v 1.8 2011/10/21 20:48:11 kettenis Exp $	*/
/*	$NetBSD: mpconfig.h,v 1.2 2003/05/11 00:05:52 fvdl Exp $	*/
/* <LICENSES> */
/* </LICENSES> */

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
