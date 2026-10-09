/* <LICENSES> */
/*
 * Copyright (c) 1996 Scott K. Stevens
 *
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
 *	From: db_interface.c,v 2.4 1991/02/05 17:11:13 mrt (CMU)
 */
/* </LICENSES> */

/* <CODE> */
//! Interface to new debugger: `arch/arm64/arm64/db_interface.c`.
//!
//! Upstream: sys/arch/arm64/arm64/db_interface.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M4 ports `ddb_regs`, `db_ktrap` and `db_enter` (ddb-lite: the
//! trap frame is saved and `db_trap` prints where the kernel stopped). M11c adds the register
//! table `db_regs[]`, the machine command table, `db_machine_init` and the `MULTIPROCESSOR`
//! entry and exit: `ddb_mp_mutex`, `ddb_state`, `ddb_active_cpu`,
//! `db_switch_cpu`/`db_switch_to_cpu`, `db_ktrap`'s `db_enter_ddb` loop, the `machine`
//! commands `cpuinfo`, `startcpu`, `stopcpu` and `ddbcpu`, `db_enter_ddb`, `db_startcpu` and
//! `db_stopcpu`, plus the `splhigh`/`splx` around `db_trap`. `db_validate_address`,
//! `db_read_bytes`/`db_write_bytes`, `db_write_text` and `db_branch_taken` (single-stepping
//! over branches) are not ported.
//!
//! ## Deviations
//! - A fault while a command loop runs (`db_recover`) cannot `longjmp` back into
//!   `db_command_loop`: `db_ktrap` prints `Faulted in DDB` and returns at once (before the
//!   `MULTIPROCESSOR` loop, where the C checks inside it), and `do_el1h_sync` panics.
//!   `db_active` is the boolean of `init_main.rs`, not a counter.
//! - With `MULTIPROCESSOR`, `db_ktrap` stays at `splhigh` for its whole `db_enter_ddb` loop,
//!   where the C drops back to the trapped level between iterations. A CPU that handed the
//!   debugger to another one (`machine ddbcpu`) waits in `db_enter_ddb` with interrupts on;
//!   at a low level it would run the console's interrupt and eat the active CPU's input.
//! - `db_regs[]`'s `x30` is `tf_lr`: the C's `tf_x[30]` is one past the end of `tf_x`. The body the C repeats inside its
//!   `MULTIPROCESSOR` loop is the private `db_ktrap_inddb`, so the uniprocessor kernel runs it
//!   once without the loop.
//! - `db_switch_cpu` is an `AtomicBool`, `ddb_active_cpu`/`db_switch_to_cpu` `AtomicU32`s (the
//!   C's `volatile` ints and longs); `cpu_info[]` is walked through `cpu_infos()`.

use core::sync::atomic::Ordering;
#[cfg(feature = "multiprocessor")]
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU32};

use libkern::StaticCell;

#[cfg(feature = "multiprocessor")]
use crate::arch::arm64::arm64::intr::arm_send_ipi;
#[cfg(feature = "multiprocessor")]
use crate::arch::arm64::arm64::machdep::CPU_INFO;
use crate::arch::arm64::include::armreg::{
    DBG_MDSCR_KDE, DBG_MDSCR_SS, EXCP_BRK, EXCP_SOFTSTP_EL1, EXCP_WATCHPT_EL1, PSR_D, PSR_SS,
    read_specialreg, write_specialreg,
};
#[cfg(feature = "multiprocessor")]
use crate::arch::arm64::include::cpu::{
    CI_DDB_ENTERDDB, CI_DDB_INDDB, CI_DDB_RUNNING, CI_DDB_SHOULDSTOP, CI_DDB_STOPPED, CpuInfo,
    MAXCPUS, cpu_info_unit, cpu_number, curcpu,
};
#[cfg(feature = "multiprocessor")]
use crate::arch::arm64::include::db_machdep::DbExpr;
use crate::arch::arm64::include::db_machdep::DbRegs;
#[cfg(feature = "multiprocessor")]
use crate::arch::arm64::include::db_machdep::{
    DDB_STATE_EXITING, DDB_STATE_NOT_RUNNING, DDB_STATE_RUNNING,
};
#[cfg(feature = "multiprocessor")]
use crate::arch::arm64::include::intr::ARM_IPI_DDB;
use crate::db_reg_var;
#[cfg(feature = "multiprocessor")]
use crate::ddb::db_command::{DB_CMD_LOOP_DONE, DbResult};
use crate::ddb::db_command::{DB_RECOVER, DbCommand};
use crate::ddb::db_trap::db_trap;
use crate::ddb::db_variables::DbVariable;
use crate::dev::cons::cnpollc;
use crate::kern::init_main::DB_ACTIVE;
#[cfg(feature = "multiprocessor")]
use crate::kern::kern_lock::{db_mtx_enter, db_mtx_leave};
use crate::kern::subr_prf::db_printf;
use crate::machine::intr::{splhigh, splx};
#[cfg(feature = "multiprocessor")]
use crate::sys::mutex::DbMutex;

/// `ddb_regs`: register state. Written by `db_ktrap` on the one CPU that is in the
/// debugger, read by `db_trace` and `db_trap` while it is.
pub static DDB_REGS: StaticCell<DbRegs> = StaticCell::new(DbRegs::new());

/// `db_regs[]`: the registers of `ddb_regs` as debugger variables. Each reads and writes its
/// field of the trap frame (`db_reg_var!`) where C points `valuep` at it; `x30` is `tf_lr`
/// (the C's `tf_x[30]` is one past the end of `tf_x`, which holds x0 to x29).
pub static DB_REGS: [DbVariable; 35] = [
    db_reg_var!(DDB_REGS, "x0", tf_x[0]),
    db_reg_var!(DDB_REGS, "x1", tf_x[1]),
    db_reg_var!(DDB_REGS, "x2", tf_x[2]),
    db_reg_var!(DDB_REGS, "x3", tf_x[3]),
    db_reg_var!(DDB_REGS, "x4", tf_x[4]),
    db_reg_var!(DDB_REGS, "x5", tf_x[5]),
    db_reg_var!(DDB_REGS, "x6", tf_x[6]),
    db_reg_var!(DDB_REGS, "x7", tf_x[7]),
    db_reg_var!(DDB_REGS, "x8", tf_x[8]),
    db_reg_var!(DDB_REGS, "x9", tf_x[9]),
    db_reg_var!(DDB_REGS, "x10", tf_x[10]),
    db_reg_var!(DDB_REGS, "x11", tf_x[11]),
    db_reg_var!(DDB_REGS, "x12", tf_x[12]),
    db_reg_var!(DDB_REGS, "x13", tf_x[13]),
    db_reg_var!(DDB_REGS, "x14", tf_x[14]),
    db_reg_var!(DDB_REGS, "x15", tf_x[15]),
    db_reg_var!(DDB_REGS, "x16", tf_x[16]),
    db_reg_var!(DDB_REGS, "x17", tf_x[17]),
    db_reg_var!(DDB_REGS, "x18", tf_x[18]),
    db_reg_var!(DDB_REGS, "x19", tf_x[19]),
    db_reg_var!(DDB_REGS, "x20", tf_x[20]),
    db_reg_var!(DDB_REGS, "x21", tf_x[21]),
    db_reg_var!(DDB_REGS, "x22", tf_x[22]),
    db_reg_var!(DDB_REGS, "x23", tf_x[23]),
    db_reg_var!(DDB_REGS, "x24", tf_x[24]),
    db_reg_var!(DDB_REGS, "x25", tf_x[25]),
    db_reg_var!(DDB_REGS, "x26", tf_x[26]),
    db_reg_var!(DDB_REGS, "x27", tf_x[27]),
    db_reg_var!(DDB_REGS, "x28", tf_x[28]),
    db_reg_var!(DDB_REGS, "x29", tf_x[29]),
    db_reg_var!(DDB_REGS, "x30", tf_lr),
    db_reg_var!(DDB_REGS, "sp", tf_sp),
    db_reg_var!(DDB_REGS, "spsr", tf_spsr),
    db_reg_var!(DDB_REGS, "elr", tf_elr),
    db_reg_var!(DDB_REGS, "lr", tf_lr),
];

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
    let ci = CPU_INFO.get(cpu as usize)?.get();
    // SAFETY: `cpu_info[]` holds null or a `cpu_info` that is never freed.
    unsafe { ci.as_ref() }
}

/// The attached CPUs with their numbers: the C's `for (i = 0; i < MAXCPUS; i++) if
/// (cpu_info[i] != NULL)`.
#[cfg(feature = "multiprocessor")]
fn cpu_infos() -> impl Iterator<Item = (u32, &'static CpuInfo)> {
    (0..MAXCPUS).filter_map(|i| cpu_info(i).map(|ci| (i, ci)))
}

/// `db_ktrap`: the debugger's entry from an exception of class `type_`; always returns
/// `true` (the kernel continues with `regs`).
pub fn db_ktrap(type_: i32, regs: &mut DbRegs) -> bool {
    let fault = !(type_ == EXCP_BRK as i32
        || type_ == EXCP_WATCHPT_EL1 as i32
        || type_ == EXCP_SOFTSTP_EL1 as i32
        || type_ == -1);
    if fault && DB_RECOVER.load(Ordering::Relaxed) {
        // db_error("Faulted in DDB; continuing...\n") longjmps back into the command loop in
        // C; without a longjmp the fault cannot be unwound (see the module's deviations).
        db_printf(format_args!("Faulted in DDB\n"));
        return true;
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
            db_ktrap_inddb(type_, regs);
            if !DB_SWITCH_CPU.load(Ordering::Relaxed) {
                DDB_STATE.store(DDB_STATE_EXITING, Ordering::Relaxed);
            }
        }
        splx(s);
    }
    #[cfg(not(feature = "multiprocessor"))]
    db_ktrap_inddb(type_, regs);

    // Enable debug exceptions in the kernel when needed.
    let mut mdscr = read_specialreg!("mdscr_el1");
    if regs.tf_spsr as u64 & PSR_SS != 0 {
        mdscr |= DBG_MDSCR_KDE | DBG_MDSCR_SS;
        regs.tf_spsr &= !(PSR_D as isize);
    } else {
        mdscr &= !(DBG_MDSCR_KDE | DBG_MDSCR_SS);
        regs.tf_spsr |= PSR_D as isize;
    }
    // SAFETY: MDSCR_EL1 only arms or disarms software-step debug events for the kernel,
    // matching the frame about to be restored.
    unsafe { write_specialreg!("mdscr_el1", mdscr) };

    true
}

/// The part of `db_ktrap` the C repeats inside its `MULTIPROCESSOR` loop: this CPU is the one
/// in the debugger, with the exception frame in `ddb_regs`, at `splhigh`, with the console
/// polled.
fn db_ktrap_inddb(type_: i32, regs: &mut DbRegs) {
    match type_ {
        // breakpoint, watchpoint, single-step, keyboard interrupt
        t if t == EXCP_BRK as i32
            || t == EXCP_WATCHPT_EL1 as i32
            || t == EXCP_SOFTSTP_EL1 as i32
            || t == -1 => {}
        _ => {
            // db_recover != 0: handled at the top of db_ktrap (see the module's deviations).
        }
    }

    // Should switch to kdb`s own stack here.

    // SAFETY: one CPU at a time runs this (the only one, or with MULTIPROCESSOR the one
    // `db_enter_ddb` let in under `ddb_mp_mutex`); the readers (db_trace, db_trap) run below on
    // this CPU, and it copies the frame back before another CPU is let in.
    unsafe { DDB_REGS.write(*regs) };

    let s = splhigh();
    DB_ACTIVE.store(true, Ordering::Relaxed);
    cnpollc(true);
    db_trap(type_, 0 /* code */);
    cnpollc(false);
    DB_ACTIVE.store(false, Ordering::Relaxed);
    splx(s);

    // SAFETY: as above; db_trap has returned and no other CPU is in the debugger yet.
    *regs = unsafe { DDB_REGS.read() };
}

/// `db_enter`: enters the debugger with `brk #0xf000`, which `do_el1h_sync` hands to
/// `db_ktrap` and then steps over.
pub fn db_enter() {
    // SAFETY: a breakpoint instruction; the exception handler returns past it.
    unsafe { core::arch::asm!("brk #0xf000", options(nomem, nostack, preserves_flags)) };
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
/// or leaves it (`false`). The first CPU in stops the others with `ARM_IPI_DDB`; the others
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
                arm_send_ipi(ci, ARM_IPI_DDB);
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

/// `db_stopcpu`: asks `cpu` to stop in ddb with `ARM_IPI_DDB`, unless it is stopped already.
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
            arm_send_ipi(ci, ARM_IPI_DDB);
        }
        _ => db_mtx_leave(&DDB_MP_MUTEX),
    }
}

/// `db_machine_command_table[]`: the `machine` commands, all of them `MULTIPROCESSOR` ones.
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
];

/// `db_machine_command_table[]` without `MULTIPROCESSOR`: empty.
#[cfg(not(feature = "multiprocessor"))]
pub const DB_MACHINE_COMMAND_TABLE: &[DbCommand] = &[];

/// `db_machine_init`: machine-dependent debugger set-up: every CPU starts out running
/// (`MULTIPROCESSOR`).
pub fn db_machine_init() {
    #[cfg(feature = "multiprocessor")]
    for (_, ci) in cpu_infos() {
        ci.ci_ddb_paused.store(CI_DDB_RUNNING, Ordering::Relaxed);
    }
}
/* </CODE> */
