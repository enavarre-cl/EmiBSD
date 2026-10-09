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

/*
 * Copyright (c) 1997 Charles D. Cranor and Washington University.
 * Copyright (c) 1982, 1986, 1989, 1993
 *      The Regents of the University of California.
 *
 * All rights reserved.
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
 *      @(#)vm_meter.c  8.4 (Berkeley) 1/4/94
 * from: Id: uvm_meter.c,v 1.1.2.1 1997/08/14 19:10:35 chuck Exp
 */
/* </LICENSES> */

/* <CODE> */
//! UVM's statistics and its sysctl tree (`CTL_VM`): `uvm/uvm_meter.c`.
//!
//! Upstream: sys/uvm/uvm_meter.c @ 3ce1f3f79392
//!
//! `vm.loadavg`, `vm.vmmeter`, `vm.uvmexp`, `vm.nkmempages`, `vm.psstrings`,
//! `vm.anonmin`/`vtextmin`/`vnodemin`, `vm.maxslp`, `vm.uspace` and `vm.malloc_conf` are
//! served; `uvmexp_print` is the `ddb` printer.
//!
//! ## Deviations
//! - `UVM_SWAP_ENCRYPT` is not configured (as in the rest of the tree), so
//!   `vm.swapencrypt` is `EOPNOTSUPP` as the C's `#else` branch.
//! - `struct loadavg` and `struct vmtotal` have padding in C; they are copied out as the C's
//!   byte image, built member by member with the holes zeroed.
//! - `uvmexp_read` returns a [`UvmexpCopy`] (the plain `struct uvmexp`) instead of filling
//!   one through a pointer; `counters_read` reads the one counter array (no per-CPU copies
//!   until `percpu`).
//! - `maxslp` keeps its lowercase C name: `MAXSLP` is the constant it starts from.
//! - `malloc_conf` is a `StaticCell` buffer, written only under `sysctl_lock` (`sys_sysctl`
//!   holds it around every `CTL_VM` call).

use core::sync::atomic::{AtomicI32, Ordering};

use libkern::StaticCell;

use crate::kern::kern_malloc::nkmempages;
use crate::kern::kern_proc::ALLPROC;
use crate::kern::kern_sysctl::{sysctl_int, sysctl_rdint, sysctl_rdstruct, sysctl_string};
use crate::kern::sched_bsd::averunnable;
use crate::machine::Machine;
use crate::machine::cpu::Cpu;
use crate::machine::db_machdep::PrFn;
use crate::sys::errno::Errno;
use crate::sys::param::USPACE;
use crate::sys::proc::{Proc, SIDL, SONPROC, SRUN, SSLEEP, SSTOP};
use crate::sys::sysctl::SysctlPlain;
use crate::sys::vmmeter::Vmtotal;
use crate::uvm::uvm_init::{UVM, UVMEXP};
use crate::uvm::uvmexp::{
    UvmExpCounters, UvmexpCopy, VM_ANONMIN, VM_LOADAVG, VM_MALLOC_CONF, VM_MAXSLP, VM_METER,
    VM_NKMEMPAGES, VM_PSSTRINGS, VM_SWAPENCRYPT, VM_USPACE, VM_UVMEXP, VM_VNODEMIN, VM_VTEXTMIN,
    counters_read,
};

/// `MAXSLP`: the time for a process to be blocked before being very swappable. This is a
/// number of seconds which the system takes as being a non-trivial amount of real time. You
/// probably shouldn't change this; it is used in subtle ways (fractions and multiples of it
/// are, that is, like half of a "long time", almost a long time, etc.) It is related to human
/// patience and other factors which don't really change over time.
pub const MAXSLP: i32 = 20;

/// `maxslp`: patchable.
#[allow(non_upper_case_globals)] // the C's name; `MAXSLP` is the constant
pub static maxslp: AtomicI32 = AtomicI32::new(MAXSLP);

/// `malloc_conf`: the configuration string for userland `malloc(3)`. Protected by:
/// `sysctl_lock`.
#[allow(non_upper_case_globals)] // the C's name, an array beside `maxslp`
pub static malloc_conf: StaticCell<[u8; 16]> = StaticCell::new([0; 16]);

/// `uvm_sysctl`: the sysctl hook into UVM (`CTL_VM`).
pub fn uvm_sysctl(
    name: &[i32],
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
    p: &Proc,
) -> Result<(), Errno> {
    let pr = p.process();

    let Some(&top) = name.first() else {
        return Err(Errno::ENOTDIR);
    };
    match top {
        // UVM_SWAP_ENCRYPT is not configured.
        VM_SWAPENCRYPT => return Err(Errno::EOPNOTSUPP),
        _ => {
            // all sysctl names at this level are terminal
            if name.len() != 1 {
                return Err(Errno::ENOTDIR); // overloaded
            }
        }
    }

    match top {
        VM_LOADAVG => {
            let avg = averunnable();
            // struct loadavg: fixpt_t ldavg[3], 4 bytes of padding, long fscale.
            let mut b = [0u8; 24];
            b[..12].copy_from_slice(avg.ldavg.as_bytes());
            b[16..].copy_from_slice(avg.fscale.as_bytes());
            sysctl_rdstruct(oldp, oldlenp, newp, &b)
        }

        VM_METER => {
            let vmtotals = uvm_total();
            sysctl_rdstruct(oldp, oldlenp, newp, &vmtotal_bytes(&vmtotals))
        }

        VM_UVMEXP => {
            let uexp = uvmexp_read();
            sysctl_rdstruct(oldp, oldlenp, newp, uexp.as_bytes())
        }

        VM_NKMEMPAGES => sysctl_rdint(oldp, oldlenp, newp, nkmempages() as i32),

        VM_PSSTRINGS => {
            let ps_strings = pr.ps_strings.get() as u64;
            sysctl_rdstruct(oldp, oldlenp, newp, ps_strings.as_bytes())
        }

        VM_ANONMIN => uvm_minpct(
            &UVMEXP.anonminpct,
            &UVMEXP.anonmin,
            [&UVMEXP.vtextminpct, &UVMEXP.vnodeminpct],
            oldp,
            oldlenp,
            newp,
            newlen,
        ),

        VM_VTEXTMIN => uvm_minpct(
            &UVMEXP.vtextminpct,
            &UVMEXP.vtextmin,
            [&UVMEXP.anonminpct, &UVMEXP.vnodeminpct],
            oldp,
            oldlenp,
            newp,
            newlen,
        ),

        VM_VNODEMIN => uvm_minpct(
            &UVMEXP.vnodeminpct,
            &UVMEXP.vnodemin,
            [&UVMEXP.anonminpct, &UVMEXP.vtextminpct],
            oldp,
            oldlenp,
            newp,
            newlen,
        ),

        VM_MAXSLP => sysctl_rdint(oldp, oldlenp, newp, maxslp.load(Ordering::Relaxed)),

        VM_USPACE => sysctl_rdint(oldp, oldlenp, newp, USPACE as i32),

        VM_MALLOC_CONF => {
            // SAFETY: sys_sysctl holds sysctl_lock for writing around every CTL_VM call,
            // which serialises every access to malloc_conf.
            let conf = unsafe { malloc_conf.get_mut() };
            sysctl_string(oldp, oldlenp, newp, newlen, conf)
        }

        _ => Err(Errno::EOPNOTSUPP),
    }
}

/// The `VM_ANONMIN`, `VM_VTEXTMIN` and `VM_VNODEMIN` cases of `uvm_sysctl`: one of the three
/// minimum percentages, which together may not pass 95, and its threshold in 1/256ths.
#[allow(clippy::too_many_arguments)] // the C's arguments plus the three counters
fn uvm_minpct(
    pct: &AtomicI32,
    min: &AtomicI32,
    others: [&AtomicI32; 2],
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
) -> Result<(), Errno> {
    let t = AtomicI32::new(pct.load(Ordering::Relaxed));
    sysctl_int(oldp, oldlenp, newp, newlen, &t)?;
    let t = t.into_inner();
    let rest: i32 = others.iter().map(|o| o.load(Ordering::Relaxed)).sum();
    if t + rest > 95 || t < 0 {
        return Err(Errno::EINVAL);
    }
    pct.store(t, Ordering::Relaxed);
    min.store(t * 256 / 100, Ordering::Relaxed);
    Ok(())
}

/// `struct vmtotal` as the C lays it out: five `u_int16_t`, two bytes of padding, nine
/// `u_int32_t`.
fn vmtotal_bytes(t: &Vmtotal) -> [u8; 48] {
    let mut b = [0u8; 48];
    let shorts = [t.t_rq, t.t_dw, t.t_pw, t.t_sl, t.t_sw];
    for (i, v) in shorts.iter().enumerate() {
        b[2 * i..2 * i + 2].copy_from_slice(&v.to_ne_bytes());
    }
    let ints = [
        t.t_vm, t.t_avm, t.t_rm, t.t_arm, t.t_vmshr, t.t_avmshr, t.t_rmshr, t.t_armshr, t.t_free,
    ];
    for (i, v) in ints.iter().enumerate() {
        b[12 + 4 * i..16 + 4 * i].copy_from_slice(&v.to_ne_bytes());
    }
    b
}

/// `uvm_total`: calculate the current state of the system.
pub fn uvm_total() -> Vmtotal {
    let mut total = Vmtotal::default();

    // calculate process statistics
    for p in ALLPROC.0.iter() {
        match p.p_stat.get() {
            0 => continue,

            SSLEEP | SSTOP => total.t_sl += 1,
            s @ (SRUN | SONPROC | SIDL) => {
                if s != SIDL
                    && let Some(ci) = p.cpu()
                    && core::ptr::eq(Machine::ci_schedstate(ci).spc_idleproc.get(), p)
                {
                    continue;
                }
                total.t_rq += 1;
                if s == SIDL {
                    continue;
                }
            }
            _ => {}
        }
        // note active objects: #if 0 in C (XXXCDC: BOGUS! rethink this).
    }

    // Calculate object memory usage statistics.
    let free = UVMEXP.free.load(Ordering::Relaxed);
    let npages = UVMEXP.npages.load(Ordering::Relaxed);
    let swpginuse = UVMEXP.swpginuse.load(Ordering::Relaxed);
    let active = UVMEXP.active.load(Ordering::Relaxed);
    total.t_free = free as u32;
    total.t_vm = (npages - free + swpginuse) as u32;
    total.t_avm = (active + swpginuse) as u32; // XXX
    total.t_rm = (npages - free) as u32;
    total.t_arm = active as u32;
    total.t_vmshr = 0; // XXX
    total.t_avmshr = 0; // XXX
    total.t_rmshr = 0; // XXX
    total.t_armshr = 0; // XXX
    total
}

/// `uvmexp_read`: `uvmexp` with the per-CPU counters folded in.
pub fn uvmexp_read() -> UvmexpCopy {
    let mut uexp = UVMEXP.snapshot();

    let c = |which: UvmExpCounters| counters_read(which) as i32;

    // stat counters
    uexp.faults = c(UvmExpCounters::Faults);
    uexp.pageins = c(UvmExpCounters::Pageins);

    // fault subcounters
    uexp.fltnoram = c(UvmExpCounters::FltNoram);
    uexp.fltnoanon = c(UvmExpCounters::FltNoanon);
    uexp.fltnoamap = c(UvmExpCounters::FltNoamap);
    uexp.fltpgwait = c(UvmExpCounters::FltPgwait);
    uexp.fltpgrele = c(UvmExpCounters::FltPgrele);
    uexp.fltrelck = c(UvmExpCounters::FltRelck);
    uexp.fltnorelck = c(UvmExpCounters::FltNorelck);
    uexp.fltanget = c(UvmExpCounters::FltAnget);
    uexp.fltanretry = c(UvmExpCounters::FltAnretry);
    uexp.fltamcopy = c(UvmExpCounters::FltAmcopy);
    uexp.fltnamap = c(UvmExpCounters::FltNamap);
    uexp.fltnomap = c(UvmExpCounters::FltNomap);
    uexp.fltlget = c(UvmExpCounters::FltLget);
    uexp.fltget = c(UvmExpCounters::FltGet);
    uexp.flt_anon = c(UvmExpCounters::FltAnon);
    uexp.flt_acow = c(UvmExpCounters::FltAcow);
    uexp.flt_obj = c(UvmExpCounters::FltObj);
    uexp.flt_prcopy = c(UvmExpCounters::FltPrcopy);
    uexp.flt_przero = c(UvmExpCounters::FltPrzero);
    uexp.fltup = c(UvmExpCounters::FltUp);
    uexp.fltnoup = c(UvmExpCounters::FltNoup);
    uexp
}

/// `uvmexp_print`: `ddb` hook to print interesting uvm counters.
pub fn uvmexp_print(pr: PrFn) {
    let uexp = uvmexp_read();

    pr(format_args!("Current UVM status:\n"));
    pr(format_args!(
        "  pagesize={} (0x{:x}), pagemask=0x{:x}, pageshift={}\n",
        uexp.pagesize, uexp.pagesize, uexp.pagemask, uexp.pageshift
    ));
    pr(format_args!(
        "  {} VM pages: {} active, {} inactive, {} wired, {} free ({} zero)\n",
        uexp.npages, uexp.active, uexp.inactive, uexp.wired, uexp.free, uexp.zeropages
    ));
    pr(format_args!(
        "  freemin={}, free-target={}, inactive-target={}, wired-max={}\n",
        uexp.freemin, uexp.freetarg, uexp.inactarg, uexp.wiredmax
    ));
    pr(format_args!(
        "  faults={}, traps={}, intrs={}, ctxswitch={} fpuswitch={}\n",
        uexp.faults, uexp.traps, uexp.intrs, uexp.swtch, uexp.fpswtch
    ));
    pr(format_args!(
        "  softint={}, syscalls={}, kmapent={}\n",
        uexp.softs, uexp.syscalls, uexp.kmapent
    ));

    pr(format_args!("  fault counts:\n"));
    pr(format_args!(
        "    noram={}, noanon={}, noamap={}, pgwait={}, pgrele={}\n",
        uexp.fltnoram, uexp.fltnoanon, uexp.fltnoamap, uexp.fltpgwait, uexp.fltpgrele
    ));
    pr(format_args!(
        "    relocks={}({}), upgrades={}({}) anget(retries)={}({}), amapcopy={}\n",
        uexp.fltrelck,
        uexp.fltnorelck,
        uexp.fltup,
        uexp.fltnoup,
        uexp.fltanget,
        uexp.fltanretry,
        uexp.fltamcopy
    ));
    pr(format_args!(
        "    neighbor anon/obj pg={}/{}, gets(lock/unlock)={}/{}\n",
        uexp.fltnamap, uexp.fltnomap, uexp.fltlget, uexp.fltget
    ));
    pr(format_args!(
        "    cases: anon={}, anoncow={}, obj={}, prcopy={}, przero={}\n",
        uexp.flt_anon, uexp.flt_acow, uexp.flt_obj, uexp.flt_prcopy, uexp.flt_przero
    ));

    pr(format_args!("  daemon and swap counts:\n"));
    pr(format_args!(
        "    woke={}, revs={}, scans={}, obscans={}, anscans={}\n",
        uexp.pdwoke, uexp.pdrevs, uexp.pdscans, uexp.pdobscan, uexp.pdanscan
    ));
    pr(format_args!(
        "    busy={}, freed={}, reactivate={}, deactivate={}\n",
        uexp.pdbusy, uexp.pdfreed, uexp.pdreact, uexp.pddeact
    ));
    pr(format_args!(
        "    pageouts={}, pending={}, nswget={}\n",
        uexp.pdpageouts, uexp.pdpending, uexp.nswget
    ));
    pr(format_args!(
        "    nswapdev={}, swpskip={}\n",
        uexp.nswapdev, uexp.swpskip
    ));
    pr(format_args!(
        "    swpages={}, swpginuse={}, swpgonly={} paging={}\n",
        uexp.swpages, uexp.swpginuse, uexp.swpgonly, uexp.paging
    ));

    pr(format_args!("  kernel pointers:\n"));
    pr(format_args!(
        "    objs(kern)={:p}\n",
        UVM.kernel_object.get()
    ));
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vmtotal_has_the_c_layout() {
        let t = Vmtotal {
            t_rq: 1,
            t_sw: 5,
            t_vm: 6,
            t_free: 14,
            ..Vmtotal::default()
        };
        let b = vmtotal_bytes(&t);
        assert_eq!(u16::from_ne_bytes([b[0], b[1]]), 1);
        assert_eq!(u16::from_ne_bytes([b[8], b[9]]), 5);
        assert_eq!(&b[10..12], &[0, 0]);
        assert_eq!(u32::from_ne_bytes([b[12], b[13], b[14], b[15]]), 6);
        assert_eq!(u32::from_ne_bytes([b[44], b[45], b[46], b[47]]), 14);
    }

    #[test]
    fn uvmexp_read_folds_in_the_counters() {
        let before = uvmexp_read().fltanget;
        crate::uvm::uvmexp::counters_inc(UvmExpCounters::FltAnget);
        assert!(uvmexp_read().fltanget > before);
    }
}
/* </TESTS> */
