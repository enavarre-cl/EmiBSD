/*	$OpenBSD: kern_sched.c,v 1.116 2026/04/09 01:30:02 jsg Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2007, 2008 Artur Grabowski <art@openbsd.org>
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

//! The run queues and CPU selection: `kern/kern_sched.c`.
//!
//! Upstream: sys/kern/kern_sched.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M5 (part a) ports `sched_init` and `sched_init_cpu` as far as
//! the clock interrupts: the four `clockintr_bind`s. The run queues, the idle thread
//! (`sched_kthreads_create`, `sched_idle`), `sched_chooseproc`, `setrunqueue`/`remrunqueue`,
//! the CPU sets and the rest arrive with `struct proc` (part b).

use core::ptr;

use crate::kern::kern_clock::statclock;
use crate::kern::kern_clockintr::clockintr_bind;
use crate::kern::kern_time::itimer_update;
use crate::kern::sched_bsd::roundrobin;
use crate::kern::subr_prof::profclock;
use crate::machine::Machine;
use crate::machine::cpu::{Cpu, CpuInfo};
use crate::unported;

/// `sched_init`.
pub fn sched_init() {
    // cpuset_add(&sched_all_cpus, curcpu()): the CPU sets (M5-b).
    let _ = unported!("sched_init: cpuset_add (M5-b)");
}

/// `sched_init_cpu`: called from `main()` for the boot cpu, then it's the responsibility
/// of the MD code to call it for all other cpus.
pub fn sched_init_cpu(ci: &'static CpuInfo) {
    let spc = Machine::ci_schedstate(ci);

    // TAILQ_INIT(&spc->spc_qs[i]), spc->spc_idleproc = NULL: struct proc (M5-b).

    clockintr_bind(&spc.spc_itimer, ci, itimer_update, ptr::null_mut());
    clockintr_bind(&spc.spc_profclock, ci, profclock, ptr::null_mut());
    clockintr_bind(&spc.spc_roundrobin, ci, roundrobin, ptr::null_mut());
    clockintr_bind(&spc.spc_statclock, ci, statclock, ptr::null_mut());

    // kthread_create_deferred(sched_kthreads_create, ci): the idle thread (M5-b).
    let _ = unported!("sched_init_cpu: kthread_create_deferred (sched_kthreads_create, M5-b)");

    // TAILQ_INIT(&spc->spc_deadproc), SIMPLEQ_INIT(&spc->spc_deferred): M5-b.

    // Slight hack here until the cpuset code handles cpu_info structures.
    let _ = unported!("sched_init_cpu: cpuset_init_cpu (M5-b)");
}
