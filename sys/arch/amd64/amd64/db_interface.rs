/*	$OpenBSD: db_interface.c,v 1.40 2025/02/12 20:18:31 bluhm Exp $	*/
/*	$NetBSD: db_interface.c,v 1.1 2003/04/26 18:39:27 fvdl Exp $	*/
/* <LICENSES> */
/*
 * Mach Operating System
 * Copyright (c) 1991,1990 Carnegie Mellon University
 * All Rights Reserved.
 *
 * Permission to use, copy, modify and distribute this software and its
 * documentation is hereby granted, provided that both the copyright
 * notice and this permission notice appear in all copies of the
 * software, derivative works or modified versions, and any portions
 * thereof, and that both notices appear in supporting documentation.
 *
 * CARNEGIE MELLON ALLOWS FREE USE OF THIS SOFTWARE IN ITS "AS IS"
 * CONDITION.  CARNEGIE MELLON DISCLAIMS ANY LIABILITY OF ANY KIND FOR
 * ANY DAMAGES WHATSOEVER RESULTING FROM THE USE OF THIS SOFTWARE.
 *
 * Carnegie Mellon requests users of this software to return to
 *
 *  Software Distribution Coordinator  or  Software.Distribution@CS.CMU.EDU
 *  School of Computer Science
 *  Carnegie Mellon University
 *  Pittsburgh PA 15213-3890
 *
 * any improvements or extensions that they make and grant Carnegie the
 * rights to redistribute these changes.
 *
 *	db_interface.c,v 2.4 1991/02/05 17:11:13 mrt (CMU)
 */
/* </LICENSES> */

/* <CODE> */
//! Interface to new debugger: `arch/amd64/amd64/db_interface.c`.
//!
//! Upstream: sys/arch/amd64/amd64/db_interface.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M4 ports `ddb_regs`, `db_printtrap`, `db_ktrap` and `db_enter`
//! (ddb-lite: the trap frame is saved and `db_trap` prints where the kernel stopped). M11c
//! adds `db_sysregs_cmd`, the machine command table and `db_machine_init` (the register table
//! `db_regs[]` is `db_trace.rs`'s, as in C), and the `MULTIPROCESSOR` entry and exit:
//! `ddb_mp_mutex`, `ddb_state`, `ddb_active_cpu`, `db_switch_cpu`/`db_switch_to_cpu`,
//! `db_ktrap`'s `db_enter_ddb` loop, the `machine` commands `cpuinfo`, `startcpu`, `stopcpu`
//! and `ddbcpu`, `db_enter_ddb`, `db_startcpu`, `db_stopcpu` and `x86_ipi_db` (moved here from
//! `ipifuncs.rs`, where the C has it), plus the `splhigh`/`splx` around `db_trap`.
//! `db_read_bytes`/`db_write_bytes` are not ported (ddb reads memory directly).
//!
//! ## Deviations
//! - A fault while a command loop runs (`db_recover`) cannot `longjmp` back into
//!   `db_command_loop`: `db_ktrap` prints `Faulted in DDB` and returns 0, so `kerntrap`
//!   panics. `db_active` is the boolean of `init_main.rs`, not a counter.
//! - With `MULTIPROCESSOR`, `db_ktrap` stays at `splhigh` for its whole `db_enter_ddb` loop,
//!   where the C drops back to the trapped level between iterations. A CPU that handed the
//!   debugger to another one (`machine ddbcpu`) waits in `db_enter_ddb` with interrupts on;
//!   at a low level it would run the console's interrupt and eat the active CPU's input. The body the C repeats inside its
//!   `MULTIPROCESSOR` loop is the private `db_ktrap_inddb`, so the uniprocessor kernel runs it
//!   once without the loop.
//! - `db_switch_cpu` is an `AtomicBool`, `ddb_active_cpu`/`db_switch_to_cpu` `AtomicU32`s (the
//!   C's `volatile` ints and longs); `cpu_info[]` is walked through `cpu_infos()`.
//! - `db_panic` is 0 (`kern/subr_prf.rs`), so a fatal trap returns 0 from `db_ktrap` and
//!   `kerntrap` prints the trap and panics, which is the C's flow when ddb is told not to
//!   take panics.

use core::sync::atomic::Ordering;
#[cfg(feature = "multiprocessor")]
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU32};

use libkern::StaticCell;

#[cfg(feature = "multiprocessor")]
use crate::arch::amd64::amd64::cpu::CPU_INFO;
#[cfg(feature = "multiprocessor")]
use crate::arch::amd64::amd64::ipi::x86_send_ipi;
use crate::arch::amd64::amd64::trap::{TRAP_TYPE, TRAP_TYPES};
#[cfg(feature = "multiprocessor")]
use crate::arch::amd64::include::cpu::{
    CI_DDB_ENTERDDB, CI_DDB_INDDB, CI_DDB_RUNNING, CI_DDB_SHOULDSTOP, CI_DDB_STOPPED, CpuInfo,
    MAXCPUS, cpu_info_unit, cpu_number, curcpu,
};
use crate::arch::amd64::include::cpufunc::{breakpoint, rcr0, rcr2, rcr3, rcr4, rdmsr};
#[cfg(feature = "multiprocessor")]
use crate::arch::amd64::include::db_machdep::{
    DDB_STATE_EXITING, DDB_STATE_NOT_RUNNING, DDB_STATE_RUNNING,
};
use crate::arch::amd64::include::db_machdep::{DbExpr, DbRegs};
#[cfg(feature = "multiprocessor")]
use crate::arch::amd64::include::intrdefs::X86_IPI_DDB;
use crate::arch::amd64::include::segments::RegionDescriptor;
use crate::arch::amd64::include::specialreg::{MSR_GSBASE, MSR_KERNELGSBASE};
use crate::arch::amd64::include::trap::{T_BPTFLT, T_NMI, T_TRCTRAP};
#[cfg(feature = "multiprocessor")]
use crate::ddb::db_command::DB_CMD_LOOP_DONE;
use crate::ddb::db_command::{DB_RECOVER, DbCommand, DbResult};
use crate::ddb::db_trap::db_trap;
use crate::dev::cons::cnpollc;
use crate::kern::init_main::DB_ACTIVE;
#[cfg(feature = "multiprocessor")]
use crate::kern::kern_lock::{db_mtx_enter, db_mtx_leave};
use crate::kern::subr_prf::{DB_PANIC, db_printf};
use crate::machine::intr::{splhigh, splx};
#[cfg(feature = "multiprocessor")]
use crate::sys::mutex::DbMutex;

/// `ddb_regs`: register state. Written by `db_ktrap` on the one CPU that is in the
/// debugger, read by `db_trace` and `db_trap` while it is.
pub static DDB_REGS: StaticCell<DbRegs> = StaticCell::new(DbRegs {
    tf_rdi: 0,
    tf_rsi: 0,
    tf_rdx: 0,
    tf_r10: 0,
    tf_r8: 0,
    tf_r9: 0,
    tf_rcx: 0,
    tf_r11: 0,
    tf_r12: 0,
    tf_r13: 0,
    tf_r14: 0,
    tf_r15: 0,
    tf_err: 0,
    tf_rbx: 0,
    tf_rax: 0,
    tf_trapno: 0,
    tf_rbp: 0,
    tf_rip: 0,
    tf_cs: 0,
    tf_rflags: 0,
    tf_rsp: 0,
    tf_ss: 0,
});

/// `ddb_mp_mutex`: serialises the CPUs entering and leaving the debugger (`MULTIPROCESSOR`).
#[cfg(feature = "multiprocessor")]
pub static DDB_MP_MUTEX: DbMutex = DbMutex::new();

/// `ddb_state` (volatile): `DDB_STATE_*`, written under `ddb_mp_mutex`.
#[cfg(feature = "multiprocessor")]
pub static DDB_STATE: AtomicI32 = AtomicI32::new(DDB_STATE_NOT_RUNNING);

/// `ddb_active_cpu` (volatile): the CPU that runs the debugger while `DDB_STATE_RUNNING`.
#[cfg(feature = "multiprocessor")]
pub static DDB_ACTIVE_CPU: AtomicU32 = AtomicU32::new(0);

/// `db_switch_cpu`: `machine ddbcpu` asked to hand the debugger to `db_switch_to_cpu`.
#[cfg(feature = "multiprocessor")]
pub static DB_SWITCH_CPU: AtomicBool = AtomicBool::new(false);

/// `db_switch_to_cpu`: the CPU `machine ddbcpu` hands the debugger to.
#[cfg(feature = "multiprocessor")]
pub static DB_SWITCH_TO_CPU: AtomicU32 = AtomicU32::new(0);

/// `cpu_info[cpu]`, when that CPU is attached (`MULTIPROCESSOR`).
#[cfg(feature = "multiprocessor")]
fn cpu_info(cpu: u32) -> Option<&'static CpuInfo> {
    let ci = CPU_INFO.get(cpu as usize)?.load(Ordering::Acquire);
    // SAFETY: `cpu_info[]` holds null or a `cpu_info` that is never freed.
    unsafe { ci.as_ref() }
}

/// The attached CPUs with their numbers: the C's `for (i = 0; i < MAXCPUS; i++) if
/// (cpu_info[i] != NULL)`.
#[cfg(feature = "multiprocessor")]
fn cpu_infos() -> impl Iterator<Item = (u32, &'static CpuInfo)> {
    (0..MAXCPUS).filter_map(|i| cpu_info(i).map(|ci| (i, ci)))
}

/// `db_printtrap`: names the trap that brought the kernel into the debugger.
pub fn db_printtrap(type_: i32, code: i32) {
    db_printf(format_args!("kernel: "));
    if !(0..TRAP_TYPES).contains(&type_) {
        db_printf(format_args!("type {type_}"));
    } else {
        db_printf(format_args!("{}", TRAP_TYPE[type_ as usize]));
    }
    db_printf(format_args!(" trap, code={code:x}\n"));
}

/// `db_ktrap`: field a TRACE or BPT trap. Returns `true` when the debugger took the trap and
/// the kernel continues with `regs`, `false` when the caller should treat it as fatal.
pub fn db_ktrap(type_: i32, code: i32, regs: &mut DbRegs) -> bool {
    // wsdisplay_enter_ddb(): no wsdisplay.

    match type_ {
        // breakpoint, single_step, NMI, keyboard interrupt
        T_BPTFLT | T_TRCTRAP | T_NMI | -1 => {}
        _ => {
            if !DB_PANIC.load(Ordering::Relaxed) {
                return false;
            }

            db_printtrap(type_, code);
            if DB_RECOVER.load(Ordering::Relaxed) {
                // db_error("Faulted in DDB; continuing...\n") longjmps back into the command
                // loop in C; without a longjmp the fault cannot be unwound, so the caller
                // panics (see the module's deviations).
                db_printf(format_args!("Faulted in DDB\n"));
                return false;
            }
        }
    }

    #[cfg(feature = "multiprocessor")]
    {
        db_mtx_enter(&DDB_MP_MUTEX);
        if DDB_STATE.load(Ordering::Relaxed) == DDB_STATE_EXITING {
            DDB_STATE.store(DDB_STATE_NOT_RUNNING, Ordering::Relaxed);
        }
        db_mtx_leave(&DDB_MP_MUTEX);
        // Held across the loop, not only around db_trap (see the module's deviations).
        let s = splhigh();
        while db_enter_ddb() {
            db_ktrap_inddb(type_, code, regs);
            if !DB_SWITCH_CPU.load(Ordering::Relaxed) {
                DDB_STATE.store(DDB_STATE_EXITING, Ordering::Relaxed);
            }
        }
        splx(s);
    }
    #[cfg(not(feature = "multiprocessor"))]
    db_ktrap_inddb(type_, code, regs);

    true
}

/// The part of `db_ktrap` the C repeats inside its `MULTIPROCESSOR` loop: this CPU is the one
/// in the debugger, with the trap frame in `ddb_regs`, at `splhigh`, with the console polled.
fn db_ktrap_inddb(type_: i32, code: i32, regs: &mut DbRegs) {
    let mut saved = *regs;
    saved.tf_cs &= 0xffff;
    saved.tf_ss &= 0xffff;
    // SAFETY: one CPU at a time runs this (the only one, or with MULTIPROCESSOR the one
    // `db_enter_ddb` let in under `ddb_mp_mutex`); the readers (db_trace, db_trap) run below on
    // this CPU, and it copies the frame back before another CPU is let in.
    unsafe { DDB_REGS.write(saved) };

    let s = splhigh();
    DB_ACTIVE.store(true, Ordering::Relaxed);
    cnpollc(true);
    db_trap(type_, code);
    cnpollc(false);
    DB_ACTIVE.store(false, Ordering::Relaxed);
    splx(s);

    // SAFETY: as above; db_trap has returned and no other CPU is in the debugger yet.
    *regs = unsafe { DDB_REGS.read() };
}

/// `db_sysregs_cmd`: `machine sysregs`, prints the descriptor table registers, the control
/// registers and the GS bases.
pub fn db_sysregs_cmd(_addr: DbExpr, _have_addr: bool, _count: DbExpr, _modif: &[u8]) -> DbResult {
    let mut idtr = RegionDescriptor {
        rd_limit: 0,
        rd_base: 0,
    };
    let mut gdtr = idtr;
    let ldtr: u16;
    let tr: u16;

    // SAFETY: sidt stores the 10-byte IDT register into `idtr`, a packed region descriptor of
    // exactly that layout; it has no other effect.
    unsafe {
        core::arch::asm!("sidt [{}]", in(reg) &raw mut idtr, options(nostack, preserves_flags))
    };
    let (base, limit) = (idtr.rd_base, idtr.rd_limit);
    db_printf(format_args!("idtr:   {base:#010x}/{limit:04x}\n"));

    // SAFETY: as above, for the GDT register.
    unsafe {
        core::arch::asm!("sgdt [{}]", in(reg) &raw mut gdtr, options(nostack, preserves_flags))
    };
    let (base, limit) = (gdtr.rd_base, gdtr.rd_limit);
    db_printf(format_args!("gdtr:   {base:#010x}/{limit:04x}\n"));

    // SAFETY: sldt reads the LDT selector into a register; no memory, no flags.
    unsafe {
        core::arch::asm!("sldt {0:x}", out(reg) ldtr, options(nomem, nostack, preserves_flags))
    };
    db_printf(format_args!("ldtr:   {ldtr:#06x}\n"));

    // SAFETY: str reads the task register's selector into a register; no memory, no flags.
    unsafe { core::arch::asm!("str {0:x}", out(reg) tr, options(nomem, nostack, preserves_flags)) };
    db_printf(format_args!("tr:     {tr:#06x}\n"));

    db_printf(format_args!("cr0:    {:#018x}\n", rcr0()));
    db_printf(format_args!("cr2:    {:#018x}\n", rcr2()));
    db_printf(format_args!("cr3:    {:#018x}\n", rcr3()));
    db_printf(format_args!("cr4:    {:#018x}\n", rcr4()));

    // SAFETY: every amd64 CPU has the GS base MSRs (long mode requires them).
    let gsb = unsafe { rdmsr(MSR_GSBASE) };
    db_printf(format_args!("gsb:    {gsb:#018x}\n"));

    // SAFETY: as above.
    let gsb = unsafe { rdmsr(MSR_KERNELGSBASE) };
    db_printf(format_args!("kgsb:   {gsb:#018x}\n"));
    Ok(())
}

/// `cpu_info[addr]` for a ddb command's CPU argument: a valid, attached CPU other than this one.
#[cfg(feature = "multiprocessor")]
fn db_other_cpu(addr: DbExpr) -> Option<u32> {
    let cpu = u32::try_from(addr).ok().filter(|&c| c < MAXCPUS)?;
    (cpu_info(cpu).is_some() && cpu != cpu_number()).then_some(cpu)
}

/// `db_cpuinfo_cmd`: `machine cpuinfo`, every CPU's ddb state; `*` marks this one.
#[cfg(feature = "multiprocessor")]
pub fn db_cpuinfo_cmd(_addr: DbExpr, _have_addr: bool, _count: DbExpr, _modif: &[u8]) -> DbResult {
    for (i, ci) in cpu_infos() {
        db_printf(format_args!(
            "{}{:4}: ",
            if i == cpu_number() { '*' } else { ' ' },
            cpu_info_unit(ci)
        ));
        match ci.ci_ddb_paused.load(Ordering::Relaxed) {
            CI_DDB_RUNNING => db_printf(format_args!("running\n")),
            CI_DDB_SHOULDSTOP => db_printf(format_args!("stopping\n")),
            CI_DDB_STOPPED => db_printf(format_args!("stopped\n")),
            CI_DDB_ENTERDDB => db_printf(format_args!("entering ddb\n")),
            CI_DDB_INDDB => db_printf(format_args!("ddb\n")),
            paused => db_printf(format_args!("? ({paused})\n")),
        };
    }
    Ok(())
}

/// `db_startproc_cmd`: `machine startcpu [cpu]`, lets one held CPU (or all of them) run.
#[cfg(feature = "multiprocessor")]
pub fn db_startproc_cmd(addr: DbExpr, have_addr: bool, _count: DbExpr, _modif: &[u8]) -> DbResult {
    if have_addr {
        match db_other_cpu(addr) {
            Some(cpu) => db_startcpu(cpu),
            None => {
                db_printf(format_args!("Invalid cpu {}\n", addr as i32));
            }
        }
    } else {
        for (i, _) in cpu_infos() {
            if i != cpu_number() {
                db_startcpu(i);
            }
        }
    }
    Ok(())
}

/// `db_stopproc_cmd`: `machine stopcpu [cpu]`, holds one CPU (or all of them) in ddb.
#[cfg(feature = "multiprocessor")]
pub fn db_stopproc_cmd(addr: DbExpr, have_addr: bool, _count: DbExpr, _modif: &[u8]) -> DbResult {
    if have_addr {
        match db_other_cpu(addr) {
            Some(cpu) => db_stopcpu(cpu),
            None => {
                db_printf(format_args!("Invalid cpu {}\n", addr as i32));
            }
        }
    } else {
        for (i, _) in cpu_infos() {
            if i != cpu_number() {
                db_stopcpu(i);
            }
        }
    }
    Ok(())
}

/// `db_ddbproc_cmd`: `machine ddbcpu cpu`, hands the debugger to another CPU: it is stopped,
/// this command loop ends, and `db_enter_ddb` makes it the active CPU.
#[cfg(feature = "multiprocessor")]
pub fn db_ddbproc_cmd(addr: DbExpr, have_addr: bool, _count: DbExpr, _modif: &[u8]) -> DbResult {
    if have_addr {
        match db_other_cpu(addr) {
            Some(cpu) => {
                db_stopcpu(cpu);
                DB_SWITCH_TO_CPU.store(cpu, Ordering::Relaxed);
                DB_SWITCH_CPU.store(true, Ordering::Relaxed);
                DB_CMD_LOOP_DONE.store(true, Ordering::Relaxed);
            }
            None => {
                db_printf(format_args!("Invalid cpu {}\n", addr as i32));
            }
        }
    } else {
        db_printf(format_args!("CPU not specified\n"));
    }
    Ok(())
}

/// `db_enter_ddb`: decides, under `ddb_mp_mutex`, whether this CPU runs the debugger (`true`)
/// or leaves it (`false`). The first CPU in stops the others with `X86_IPI_DDB`; the others
/// wait here until ddb is handed to them (`machine ddbcpu`) or everyone resumes.
#[cfg(feature = "multiprocessor")]
pub fn db_enter_ddb() -> bool {
    let me = cpu_number();

    db_mtx_enter(&DDB_MP_MUTEX);

    // If we are first in, grab ddb and stop all other CPUs
    if DDB_STATE.load(Ordering::Relaxed) == DDB_STATE_NOT_RUNNING {
        DDB_ACTIVE_CPU.store(me, Ordering::Relaxed);
        DDB_STATE.store(DDB_STATE_RUNNING, Ordering::Relaxed);
        curcpu()
            .ci_ddb_paused
            .store(CI_DDB_INDDB, Ordering::Relaxed);
        db_mtx_leave(&DDB_MP_MUTEX);
        for (i, ci) in cpu_infos() {
            if i != me && ci.ci_ddb_paused.load(Ordering::Relaxed) != CI_DDB_STOPPED {
                ci.ci_ddb_paused.store(CI_DDB_SHOULDSTOP, Ordering::Relaxed);
                x86_send_ipi(ci, X86_IPI_DDB);
            }
        }
        return true;
    }

    // Leaving ddb completely.  Start all other CPUs and return 0
    if DDB_ACTIVE_CPU.load(Ordering::Relaxed) == me
        && DDB_STATE.load(Ordering::Relaxed) == DDB_STATE_EXITING
    {
        for (_, ci) in cpu_infos() {
            ci.ci_ddb_paused.store(CI_DDB_RUNNING, Ordering::Relaxed);
        }
        db_mtx_leave(&DDB_MP_MUTEX);
        return false;
    }

    // We're switching to another CPU.  db_ddbproc_cmd() has made sure
    // it is waiting for ddb, we just have to set ddb_active_cpu.
    if DDB_ACTIVE_CPU.load(Ordering::Relaxed) == me && DB_SWITCH_CPU.load(Ordering::Relaxed) {
        curcpu()
            .ci_ddb_paused
            .store(CI_DDB_SHOULDSTOP, Ordering::Relaxed);
        DB_SWITCH_CPU.store(false, Ordering::Relaxed);
        let to = DB_SWITCH_TO_CPU.load(Ordering::Relaxed);
        DDB_ACTIVE_CPU.store(to, Ordering::Relaxed);
        if let Some(ci) = cpu_info(to) {
            ci.ci_ddb_paused.store(CI_DDB_ENTERDDB, Ordering::Relaxed);
        }
    }

    // Wait until we should enter ddb or resume
    let ci = curcpu();
    let waiting = || {
        DDB_ACTIVE_CPU.load(Ordering::Relaxed) != me
            && ci.ci_ddb_paused.load(Ordering::Relaxed) != CI_DDB_RUNNING
    };
    while waiting() {
        if ci.ci_ddb_paused.load(Ordering::Relaxed) == CI_DDB_SHOULDSTOP {
            ci.ci_ddb_paused.store(CI_DDB_STOPPED, Ordering::Relaxed);
        }
        db_mtx_leave(&DDB_MP_MUTEX);

        // Busy wait without locking, we'll confirm with lock later
        while waiting() {
            core::hint::spin_loop(); // CPU_BUSY_CYCLE()
        }

        db_mtx_enter(&DDB_MP_MUTEX);
    }

    // Either enter ddb or exit
    if DDB_ACTIVE_CPU.load(Ordering::Relaxed) == me
        && DDB_STATE.load(Ordering::Relaxed) == DDB_STATE_RUNNING
    {
        ci.ci_ddb_paused.store(CI_DDB_INDDB, Ordering::Relaxed);
        db_mtx_leave(&DDB_MP_MUTEX);
        true
    } else {
        db_mtx_leave(&DDB_MP_MUTEX);
        false
    }
}

/// `db_startcpu`: lets `cpu`, held by ddb, run again.
#[cfg(feature = "multiprocessor")]
pub fn db_startcpu(cpu: u32) {
    if cpu != cpu_number()
        && let Some(ci) = cpu_info(cpu)
    {
        db_mtx_enter(&DDB_MP_MUTEX);
        ci.ci_ddb_paused.store(CI_DDB_RUNNING, Ordering::Relaxed);
        db_mtx_leave(&DDB_MP_MUTEX);
    }
}

/// `db_stopcpu`: asks `cpu` to stop in ddb with `X86_IPI_DDB`, unless it is stopped already.
#[cfg(feature = "multiprocessor")]
pub fn db_stopcpu(cpu: u32) {
    db_mtx_enter(&DDB_MP_MUTEX);
    match cpu_info(cpu) {
        Some(ci)
            if cpu != cpu_number()
                && ci.ci_ddb_paused.load(Ordering::Relaxed) != CI_DDB_STOPPED =>
        {
            ci.ci_ddb_paused.store(CI_DDB_SHOULDSTOP, Ordering::Relaxed);
            db_mtx_leave(&DDB_MP_MUTEX);
            x86_send_ipi(ci, X86_IPI_DDB);
        }
        _ => db_mtx_leave(&DDB_MP_MUTEX),
    }
}

/// `x86_ipi_db`: the `X86_IPI_DDB` handler: another CPU entered ddb and asks this one to stop,
/// which it does by entering the debugger itself (`db_ktrap` holds it in `db_enter_ddb`).
#[cfg(feature = "multiprocessor")]
pub fn x86_ipi_db(_ci: &CpuInfo) {
    db_enter();
}

/// `db_machine_command_table[]`: the `machine` commands. The `acpi` commands need
/// `NACPI > 0`, which this kernel does not configure, so they are left out as the C's `#if`
/// does.
#[cfg(feature = "multiprocessor")]
pub const DB_MACHINE_COMMAND_TABLE: &[DbCommand] = &[
    DbCommand {
        name: "cpuinfo",
        fcn: Some(db_cpuinfo_cmd),
        flag: 0,
        more: None,
    },
    DbCommand {
        name: "startcpu",
        fcn: Some(db_startproc_cmd),
        flag: 0,
        more: None,
    },
    DbCommand {
        name: "stopcpu",
        fcn: Some(db_stopproc_cmd),
        flag: 0,
        more: None,
    },
    DbCommand {
        name: "ddbcpu",
        fcn: Some(db_ddbproc_cmd),
        flag: 0,
        more: None,
    },
    DbCommand {
        name: "sysregs",
        fcn: Some(db_sysregs_cmd),
        flag: 0,
        more: None,
    },
];

/// `db_machine_command_table[]` without `MULTIPROCESSOR`: `sysregs` only.
#[cfg(not(feature = "multiprocessor"))]
pub const DB_MACHINE_COMMAND_TABLE: &[DbCommand] = &[DbCommand {
    name: "sysregs",
    fcn: Some(db_sysregs_cmd),
    flag: 0,
    more: None,
}];

/// `db_machine_init`: machine-dependent debugger set-up: every CPU starts out running
/// (`MULTIPROCESSOR`).
pub fn db_machine_init() {
    #[cfg(feature = "multiprocessor")]
    for (_, ci) in cpu_infos() {
        ci.ci_ddb_paused.store(CI_DDB_RUNNING, Ordering::Relaxed);
    }
}

/// `db_enter`: enters the debugger with a breakpoint instruction, which `kerntrap` hands to
/// `db_ktrap`.
pub fn db_enter() {
    breakpoint();
}
/* </CODE> */
