/*	$OpenBSD: kern_xxx.c,v 1.42 2025/06/16 20:21:33 kettenis Exp $	*/
/*	$NetBSD: kern_xxx.c,v 1.32 1996/04/22 01:38:41 christos Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1982, 1986, 1989, 1993
 *	The Regents of the University of California.  All rights reserved.
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
 *	@(#)kern_xxx.c	8.2 (Berkeley) 11/14/93
 */
/* </LICENSES> */

//! Odds and ends: `kern/kern_xxx.c`.
//!
//! Upstream: sys/kern/kern_xxx.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M2 ports `reboot()` and `rebooting`, the tail of `panic(9)`;
//! M8 `sys_reboot` (root only, then `reboot`) and `scdebug_call`/`scdebug_ret` (option
//! `SYSCALL_DEBUG`, feature `syscall_debug`). `__stack_smash_handler` arrives with its
//! subsystem. M11a: `sys_reboot` stops the secondary CPUs (`MULTIPROCESSOR`).
//!
//! ## Deviations
//! - `KASSERT((howto & RB_NOSYNC) || curproc != NULL)`: `curproc` arrives with M5; the
//!   assertion returns with it.

#[cfg(feature = "syscall_debug")]
use core::sync::atomic::AtomicI32;
use core::sync::atomic::{AtomicBool, Ordering};

use crate::kern::kern_prot::suser;
use crate::kern::kern_time::stop_periodic_resettodr;
#[cfg(feature = "syscall_debug")]
use crate::kern::subr_prf::printf;
use crate::machine::cpu::boot;
use crate::sys::errno::Errno;
use crate::sys::proc::Proc;
use crate::sys::syscallargs::SysRebootArgs;
use crate::sys::systm::{SysArgs, sysargs};
use crate::sys::types::Register;

/// `rebooting`: set once the system started to go down, for the benefit of code that must not
/// sleep any more.
pub static REBOOTING: AtomicBool = AtomicBool::new(false);

/// `reboot(2)`: root only; goes down with the `RB_*` flags `opt` (`<sys/reboot.h>`).
pub fn sys_reboot(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysRebootArgs = sysargs(v);

    suser(p)?;

    #[cfg(feature = "multiprocessor")]
    {
        crate::kern::kern_sched::sched_stop_secondary_cpus();
        crate::kassert!(
            <crate::machine::Machine as crate::machine::cpu::Cpu>::cpu_is_primary(
                crate::machine::cpu::curcpu()
            )
        );
    }
    reboot(uap.opt.get())
    // NOTREACHED
}

/// `reboot`: stops the clock bookkeeping and hands over to the machine's `boot(9)`.
pub fn reboot(howto: i32) -> ! {
    stop_periodic_resettodr();

    REBOOTING.store(true, Ordering::Relaxed);

    boot(howto)
}

/// `SCDEBUG_CALLS`: show calls.
#[cfg(feature = "syscall_debug")]
pub const SCDEBUG_CALLS: i32 = 0x0001;
/// `SCDEBUG_RETURNS`: show returns.
#[cfg(feature = "syscall_debug")]
pub const SCDEBUG_RETURNS: i32 = 0x0002;
/// `SCDEBUG_ALL`: even syscalls that are implemented.
#[cfg(feature = "syscall_debug")]
pub const SCDEBUG_ALL: i32 = 0x0004;
/// `SCDEBUG_SHOWARGS`: show arguments to calls.
#[cfg(feature = "syscall_debug")]
pub const SCDEBUG_SHOWARGS: i32 = 0x0008;

/// `scdebug`: what `scdebug_call`/`scdebug_ret` show (`ddb` or a debugger may change it).
#[cfg(feature = "syscall_debug")]
pub static SCDEBUG: AtomicI32 = AtomicI32::new(SCDEBUG_CALLS | SCDEBUG_RETURNS | SCDEBUG_SHOWARGS);

/// Whether `scdebug` asks about system call `code`: every call with `SCDEBUG_ALL`, else only
/// the out-of-range and unimplemented (`sys_nosys`) ones.
#[cfg(feature = "syscall_debug")]
fn scdebug_wanted(code: Register) -> bool {
    use crate::kern::init_sysent::SYSENT;
    SCDEBUG.load(Ordering::Relaxed) & SCDEBUG_ALL != 0
        || usize::try_from(code).map_or(true, |c| {
            SYSENT.get(c).is_none_or(|e| {
                e.sy_call as *const () == crate::kern::kern_sig::sys_nosys as *const ()
            })
        })
}

/// `scdebug_call`: prints a system call (`SYSCALL_DEBUG`).
#[cfg(feature = "syscall_debug")]
pub fn scdebug_call(p: &Proc, code: Register, args: &crate::sys::systm::SysArgs) {
    use crate::kern::init_sysent::SYSENT;
    use crate::kern::subr_prf::Str;
    use crate::kern::syscalls::SYSCALLNAMES;

    let scdebug = SCDEBUG.load(Ordering::Relaxed);
    if scdebug & SCDEBUG_CALLS == 0 || !scdebug_wanted(code) {
        return;
    }
    let pr = p.process();
    let _ = printf(format_args!(
        "proc {} ({}): num ",
        pr.ps_pid.get(),
        Str(pr.comm())
    ));
    match usize::try_from(code).ok().filter(|&c| c < SYSENT.len()) {
        None => {
            let _ = printf(format_args!("OUT OF RANGE ({})", code as i64));
        }
        Some(c) => {
            let _ = printf(format_args!("{c} call: {}", SYSCALLNAMES[c]));
            if scdebug & SCDEBUG_SHOWARGS != 0 {
                let _ = printf(format_args!("("));
                let n = SYSENT[c].sy_argsize as usize / size_of::<Register>();
                for (i, a) in args.iter().take(n).enumerate() {
                    let sep = if i == 0 { "" } else { ", " };
                    let _ = printf(format_args!("{sep}{a:#x}"));
                }
                let _ = printf(format_args!(")"));
            }
        }
    }
    let _ = printf(format_args!("\n"));
}

/// `scdebug_ret`: prints a system call's return (`SYSCALL_DEBUG`).
#[cfg(feature = "syscall_debug")]
pub fn scdebug_ret(p: &Proc, code: Register, error: i32, retval: &[Register; 2]) {
    use crate::kern::init_sysent::SYSENT;

    if SCDEBUG.load(Ordering::Relaxed) & SCDEBUG_RETURNS == 0 || !scdebug_wanted(code) {
        return;
    }
    let pr = p.process();
    let _ = printf(format_args!(
        "proc {} ({}): num ",
        pr.ps_pid.get(),
        crate::kern::subr_prf::Str(pr.comm())
    ));
    match usize::try_from(code).ok().filter(|&c| c < SYSENT.len()) {
        None => {
            let _ = printf(format_args!("OUT OF RANGE ({})", code as i64));
        }
        // An off_t fits the one 64-bit register on both architectures.
        Some(c) => {
            let _ = printf(format_args!(
                "{c} ret: err = {error}, rv = {:#x}",
                retval[0]
            ));
        }
    }
    let _ = printf(format_args!("\n"));
}
