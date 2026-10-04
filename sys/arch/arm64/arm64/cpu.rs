/*	$OpenBSD: cpu.c,v 1.154 2026/09/09 22:15:49 tobhe Exp $	*/

/* <LICENSES> */
/*
 * Copyright (c) 2016 Dale Rahn <drahn@dalerahn.com>
 * Copyright (c) 2017 Mark Kettenis <kettenis@openbsd.org>
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

//! arm64 CPU identification, attachment and the application processors:
//! `arch/arm64/arm64/cpu.c`.
//!
//! Upstream: sys/arch/arm64/arm64/cpu.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M11a starts the port with the two entry points of the machine
//! contract, `cpu_boot_secondary_processors` and the application processor's entry from the
//! boot glue; the rest of the file follows in the same milestone.
//!
//! ## Deviations
//! - The application processors are started through the Limine MP request
//!   (`machine::BootMp`), not PSCI `CPU_ON` with `cpu_hatch` in `locore.S`
//!   (`docs/ARCHITECTURE.md`); `cpu_hatch_entry` is where the boot glue enters.

use crate::machine::Machine;
use crate::machine::cpu::Cpu;
#[cfg(feature = "multiprocessor")]
use crate::unported;

/// `cpu_boot_secondary_processors`: without `MULTIPROCESSOR` there are no application
/// processors to start.
pub fn cpu_boot_secondary_processors() {
    #[cfg(feature = "multiprocessor")]
    let _ = unported!("cpu_boot_secondary_processors (arm64 cpu.c, M11a)");
}

/// The application processor's entry from the boot glue (`Cpu::cpu_hatch`); `arg` is its
/// `struct cpu_info`.
///
/// # Safety
///
/// Called once per application processor by the boot glue, with the `arg` the boot processor
/// passed to `BootMp::start`.
pub unsafe fn cpu_hatch_entry(_arg: usize) -> ! {
    // Nothing starts an application processor yet (`cpu_start_secondary` is not ported), and
    // one that got here could not print: its `curcpu()` is not set up.
    Machine::halt()
}
