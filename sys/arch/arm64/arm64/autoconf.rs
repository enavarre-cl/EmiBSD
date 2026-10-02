/*	$OpenBSD: autoconf.c,v 1.18 2026/06/23 11:45:54 kettenis Exp $	*/
/*
 * Copyright (c) 2009 Miodrag Vallat.
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

//! Setup the system to run on the current machine: `arch/arm64/arm64/autoconf.c`.
//!
//! Upstream: sys/arch/arm64/arm64/autoconf.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M4 ports `cpu_configure` as far as the interrupts go;
//! `diskconf`, `device_register` and the root-device search arrive with autoconfiguration
//! (M5). `cold` lives in `machdep.rs`, where the C defines it.
//!
//! ## Deviations
//! - `cpu_configure` has no `config_rootfound("mainbus")`: `bus_dma_init`, `unmap_startup`
//!   and `cpu_identify_cleanup` are reported; the interrupt controller attaches from the
//!   device tree in M4-b, part 2.

use core::sync::atomic::Ordering;

use crate::arch::arm64::arm64::machdep::COLD;
use crate::kern::kern_softintr::softintr_init;
use crate::machine::intr::{spl0, splhigh};
use crate::unported;

/// `cpu_configure`: determine i/o configuration for a machine.
pub fn cpu_configure() {
    splhigh();

    softintr_init();
    let _ = unported!("bus_dma_init (M7)");

    // config_rootfound("mainbus", NULL): autoconfiguration (M5).

    let _ = unported!("unmap_startup (M6)");

    let _ = unported!("cpu_identify_cleanup (M4-b)");

    // CRYPTO: not configured.

    COLD.store(false, Ordering::Relaxed);
    spl0();
}
