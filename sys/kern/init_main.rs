/*	$OpenBSD: init_main.c,v 1.331 2026/01/01 07:00:57 jsg Exp $	*/
/*	$NetBSD: init_main.c,v 1.84.4.1 1996/06/02 09:08:06 mrg Exp $	*/

/*
 * Copyright (c) 1995 Christopher G. Demetriou.  All rights reserved.
 * Copyright (c) 1982, 1986, 1989, 1991, 1992, 1993
 *	The Regents of the University of California.  All rights reserved.
 * (c) UNIX System Laboratories, Inc.
 * All or some portions of this file are derived from material licensed
 * to the University of California by American Telephone and Telegraph
 * Co. or Unix System Laboratories, Inc. and are reproduced herein with
 * the permission of UNIX System Laboratories, Inc.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the University nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE REGENTS AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *	@(#)init_main.c	8.9 (Berkeley) 1/21/94
 */

//! System startup: `kern/init_main.c`. Initialize the world, create process 0, mount root
//! filesystem, and fork to create init and pagedaemon. Most of the hard work is done in the
//! lower-level initialization routines including `startup()`, which does memory initialization
//! and autoconfiguration.
//!
//! Upstream: sys/kern/init_main.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M2 ports the skeleton of `main()`: the console comes up, the
//! copyright prints, and every later step is called in the C's order, each one reporting itself
//! as unported until its subsystem lands. `copyright`, `boothowto`, `db_active`, `ncpus` and
//! `ncpusfound` are here; `proc0` and its process, `start_init`, `check_console` and the
//! kernel threads arrive with M5.
//!
//! ## Deviations
//! - `main()` takes no `framep` (unused in C) and never returns, as the C's loop never does.
//! - Under feature `qemu` the run ends where proc0 would go back to sleep: `tsleep_nsec` needs
//!   the scheduler (M5), and the emulator exits with the success status that `xtask smoke`
//!   checks.

use core::sync::atomic::{AtomicBool, AtomicI32};

use crate::kprintf;
use crate::machine::Machine;
use crate::machine::cons::consinit;
use crate::machine::cpu::cpu_startup;
use crate::unported;
use crate::uvm::uvm_init::uvm_init;

#[cfg(not(feature = "qemu"))]
use crate::machine::Cpu;
#[cfg(feature = "qemu")]
use crate::machine::{Exit, ExitStatus};

/// `copyright`: printed right after the console comes up.
pub const COPYRIGHT: &str = "Copyright (c) 1982, 1986, 1989, 1991, 1993\n\
\tThe Regents of the University of California.  All rights reserved.\n\
Copyright (c) 1995-2026 OpenBSD. All rights reserved.  https://www.OpenBSD.org\n";

/// `boothowto`: the `RB_*` flags the bootloader passed (`sys/sys/reboot.rs`).
pub static BOOTHOWTO: AtomicI32 = AtomicI32::new(0);
/// `db_active`: running currently inside `ddb(4)`.
pub static DB_ACTIVE: AtomicBool = AtomicBool::new(false);
/// `ncpus`: number of CPUs running the kernel.
pub static NCPUS: AtomicI32 = AtomicI32::new(1);
/// `ncpusfound`: number of CPUs we find.
pub static NCPUSFOUND: AtomicI32 = AtomicI32::new(1);

/// `main`: the machine-independent entry point, called by each architecture's early init once
/// the machine is set up.
pub fn main() -> ! {
    // Initialize the current process pointer (curproc) before any possible traps/probes to
    // simplify trap processing: proc0 and curcpu arrive with M5.
    let _ = unported!("proc0 / curproc setup (main)");

    // Initialize timeouts.
    let _ = unported!("timeout_startup");

    // Attempt to find console and initialize in case of early panic or other messages.
    let _ = unported!("config_init"); // init autoconfiguration data structures
    consinit();

    kprintf!("{}\n", COPYRIGHT);

    // KUBSAN and WITNESS are kernel options this configuration does not have.

    let _ = unported!("KERNEL_LOCK_INIT");
    let _ = unported!("SCHED_LOCK_INIT");

    let _ = unported!("rw_obj_init");
    uvm_init();
    #[cfg(feature = "qemu")]
    {
        crate::kern::selftest::pmap_kernel_mapping();
        crate::kern::selftest::malloc_pool_stress();
        if crate::kern::selftest::trap_requested() {
            crate::kern::selftest::trap_bad_access();
        }
    }
    let _ = unported!("disk_init"); // must come before autoconfiguration
    let _ = unported!("tty_init"); // initialise tty's
    cpu_startup();

    let _ = unported!("random_start"); // Start the flow

    // Initialize mbuf's. Do this now because we might attempt to allocate mbufs or mbuf
    // clusters during autoconfiguration.
    let _ = unported!("mbinit");

    // NSTOEPLITZ: not configured.

    // Initialize sockets.
    let _ = unported!("soinit");

    // Initialize SRP subsystem.
    let _ = unported!("srp_startup");

    // Initialize SMR subsystem.
    let _ = unported!("smr_startup");

    // Initialize process and pgrp structures.
    let _ = unported!("procinit");

    // Initialize file locking.
    let _ = unported!("lf_init");

    // Initialize filedescriptors.
    let _ = unported!("filedesc_init");

    // Initialize pipes.
    let _ = unported!("pipe_init");

    // Initialize kqueues.
    let _ = unported!("kqueue_init");

    // Initialize futexes.
    let _ = unported!("futex_init");
    let _ = unported!("tslp_init");

    // Create credentials.
    let _ = unported!("crget (proc0 credentials)");

    // Create process 0 (the swapper).
    let _ = unported!("process0 / pgrp0 / session0 setup");

    // Init timeouts, signal state, file descriptor table, limits and the prototype map of
    // process 0.
    let _ = unported!("signal_init / siginit");
    let _ = unported!("fdinit");
    let _ = unported!("lim_startup");
    let _ = unported!("uvmspace_init (vmspace0)");

    // Charge root for one process.
    let _ = unported!("chgproccnt");

    // Initialize run queues
    let _ = unported!("sched_init");
    let _ = unported!("sleep_queue_init");
    let _ = unported!("clockqueue_init");
    let _ = unported!("sched_init_cpu");

    // Initialize timeouts in process context.
    let _ = unported!("timeout_proc_init");

    // Initialize task queues
    let _ = unported!("taskq_init");

    // Initialize the interface/address trees
    let _ = unported!("ifinit");
    let _ = unported!("softnet_init");

    // Lock the kernel on behalf of proc0.
    let _ = unported!("KERNEL_LOCK");

    // NMPATH: not configured.

    // Configure the devices
    let _ = unported!("cpu_configure");

    // Configure virtual memory system, set vm rlimits.
    let _ = unported!("uvm_init_limits");

    // Per CPU memory allocation
    let _ = unported!("percpu_init");

    // Reduce softnet threads to number of CPU
    let _ = unported!("softnet_percpu");

    // Initialize the file systems. NFSSERVER / NFSCLIENT: not configured.
    let _ = unported!("vfsinit");

    // Start real time and statistics clocks.
    let _ = unported!("initclocks");

    // SYSVSHM / SYSVSEM / SYSVMSG: not configured.

    // Create default routing table before attaching lo0.
    let _ = unported!("rtable_init");

    // Attach pseudo-devices.
    let _ = unported!("pdevinit (pseudo-device attach)");

    // CRYPTO: not configured.

    // Initialize protocols.
    let _ = unported!("domaininit");

    crate::kern::subr_log::initconsbuf();

    // GPROF / DDBPROF: not configured.

    // Enable per-CPU data.
    let _ = unported!("mbcpuinit");
    let _ = unported!("kqueue_init_percpu");
    let _ = unported!("pmap_init_percpu");
    let _ = unported!("uvm_init_percpu");
    let _ = unported!("evcount_init_percpu");

    // init exec
    let _ = unported!("init_exec");

    // Start the scheduler
    let _ = unported!("scheduler_start");

    // Create process 1 (init(8)). We do this now, as Unix has historically had init be
    // process 1, and changing this would probably upset a lot of people.
    let _ = unported!("fork1 (init(8))");

    // Create any kernel threads whose creation was deferred because initprocess had not yet
    // been created.
    let _ = unported!("kthread_run_deferred_queue");

    // Now that device driver threads have been created, wait for them to finish any deferred
    // autoconfiguration.
    let _ = unported!("config_pending wait");

    let _ = unported!("dostartuphooks");

    // NVSCSI / NSOFTRAID: not configured.

    // Configure root/swap devices
    let _ = unported!("diskconf");

    // Make debug symbols available in ddb.
    let _ = unported!("db_ctf_init");

    let _ = unported!("mountroot");

    // Get the vnode for '/'. Set p->p_fd->fd_cdir to reference it.
    let _ = unported!("VFS_ROOT (rootvnode)");

    // Now can look at time, having had a chance to verify the time from the file system.
    let _ = unported!("nanouptime (process start times)");

    let _ = unported!("uvm_swap_init");

    // Create the pageout, reaper, cleaner, update, aiodone and page zeroing kernel threads.
    let _ = unported!("kthread_create (pagedaemon, reaper, cleaner, update, aiodoned, zerothread)");

    // MULTIPROCESSOR: not configured.

    // Now that all CPUs partake in scheduling, start SMR thread.
    let _ = unported!("smr_startup_thread");

    let _ = unported!("config_process_deferred_mountroot");

    // Okay, now we can let init(8) exec! It's off to userland!
    let _ = unported!("start_init_exec wakeup");

    let _ = unported!("start_periodic_resettodr");

    // proc0: nothing to do, back to sleep
    #[cfg(feature = "qemu")]
    {
        Machine::exit(ExitStatus::Success)
    }
    #[cfg(not(feature = "qemu"))]
    {
        let _ = unported!("tsleep_nsec (proc0 loop)");
        Machine::halt()
    }
}
