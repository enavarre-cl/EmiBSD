/* <LICENSES> */
/*
 * Mach Operating System
 * Copyright (c) 1993,1992,1991,1990 Carnegie Mellon University
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
 * any improvements or extensions that they make and grant Carnegie Mellon
 * the rights to redistribute these changes.
 */
/*
 * Mach Operating System
 * Copyright (c) 1993,1992,1991,1990 Carnegie Mellon University
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
 * any improvements or extensions that they make and grant Carnegie Mellon
 * the rights to redistribute these changes.
 *
 *	Author: David B. Golub, Carnegie Mellon University
 *	Date:	7/90
 */
/* </LICENSES> */

/* <CODE> */
//! Command dispatcher of the debugger: `ddb/db_command.c` and `<ddb/db_command.h>`.
//!
//! Upstream: sys/ddb/db_command.c @ 3ce1f3f79392
//! Upstream: sys/ddb/db_command.h @ 3ce1f3f79392
//!
//! `db_command_loop` prints the prompt (`ddb> `, `ddb{N}> ` with `MULTIPROCESSOR`), reads a
//! line and hands it to `db_command`, which looks the first word up in `db_command_table`
//! (a prefix is enough; `show`, `show all`, `boot` and `machine` have a second level) and
//! parses the standard syntax `command[/modifier] [addr][,count]` before calling the
//! command. An empty line repeats the last command at `db_next`. `continue` and the other
//! run commands end the loop.
//!
//! ## Deviations
//! - `db_error` does not `longjmp` to `db_recover`: it returns a [`DbError`], which every
//!   function that can reach it propagates as the `Err` of a [`DbResult`] up to
//!   `db_command_loop`, where the C's `setjmp` was (`docs/C_TO_RUST.md`). Commands are
//!   [`DbCmdFn`]s and return a `DbResult` too. [`DB_RECOVER`] says whether a loop runs (the
//!   C's `db_recover != NULL`).
//! - Tables are `&'static [DbCommand]` slices without the NULL terminator, `const` so they
//!   can name each other; `fcn` and `more` are `Option`s. The `machine` table is the
//!   machine's `DB_MACHINE_COMMAND_TABLE` (`machine::db_machdep`). With `CS_OWN` the command
//!   gets 0, 0 and an empty modifier where C passes uninitialised ones.
//! - `db_command_loop` clears `DB_QUIT` (`db_output.rs`: `db_more`'s `q`) before each prompt.
//! - Commands whose code is not ported stay in the tables as visible stubs: they say so,
//!   report the gap once (`unported!`) and flush the line: the `db_examine.c` ones (`print`,
//!   `examine`, `search`), breakpoints and watchpoints (`db_break.c`, `db_watch.c`),
//!   `pprint` and `show struct` (`db_ctf.c`), `hangman` (`db_hangman.c`), `kill`, `stop`,
//!   `ps`/`show all procs` and `proc_printit` (`kern_proc.c`), `callout` (`kern_timeout.c`),
//!   `show all clockintr` (`kern_clockintr.c`), `show all pools`/`show pool`
//!   (`subr_pool.c`), the `vfs_subr.c` printers (`show buf`, `vnode`, `mount`,
//!   `all mounts`, `all vnodes`, `all bufs`), the `uvm` ones (`map`, `object`, `page`,
//!   `swap`), `malloc`, `extents`, `bcstats`. `show mbuf`, `socket`, `route`,
//!   `all routes`, `tdb`, `all tdbs`, `uvmexp`, `nfsreq`, `nfsnode`, `all nfsreqs` and
//!   `all nfsnodes` call the ported printers. `WITNESS` is not configured: its commands are
//!   left out, as the C's `#ifdef` does.
//! - `db_access.c`, `db_sym.c` and `db_examine.c` are not ported: [`db_get_value`],
//!   [`db_put_value`], [`db_printsym`] and [`db_print_loc_and_inst`] are visible stand-ins
//!   (the first two fail the command, the last two print the address as a number);
//!   `db_find_xtrn_sym_and_offset` finds no symbol, as the C does with an empty table.
//! - `show panic` reads `panicstr`'s message (`subr_prf.rs` keeps one panic buffer, not one
//!   per CPU) and names the CPU running the debugger.
//! - `db_fncall` refuses the address 0 ("Bad function"), which Rust cannot call; any other
//!   address is called as C does, with ten `long` arguments.

use core::fmt;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use libkern::StaticCell;

use crate::ddb::db_expr::db_expression;
use crate::ddb::db_lex::{
    TOK_STRING_SIZE, db_flush_lex, db_read_line, db_read_token, db_tok_string, db_unread_token,
    tCOMMA, tEOL, tEXCL, tIDENT, tLPAREN, tRPAREN, tSLASH,
};
use crate::ddb::db_output::{
    DB_FORMAT_BUF_SIZE, DB_FORMAT_N, DB_FORMAT_R, DB_OUTPUT_LINE, DB_QUIT, db_end_line, db_format,
    db_print_position, db_putchar,
};
use crate::ddb::db_run::{
    db_continue_cmd, db_single_step_cmd, db_trace_until_call_cmd, db_trace_until_matching_cmd,
};
use crate::ddb::db_variables::{DB_MAXOFF, db_read_variable, db_set_cmd};
use crate::kern::init_main::PROC0;
use crate::kern::kern_proc::tfind;
use crate::kern::kern_xxx::reboot;
use crate::kern::subr_log::msgbufp;
use crate::kern::subr_pool::{PoolWalkPr, pool_walk};
use crate::kern::subr_prf::{Str, db_printf, panicstr_message};
use crate::kern::uipc_mbuf::{m_print, m_print_chain, m_print_packet};
use crate::kern::uipc_socket::so_print;
use crate::machine::Machine;
use crate::machine::cpu::{Cpu, boot, curcpu, curproc};
use crate::machine::db_machdep::{
    DB_MACHINE_COMMAND_TABLE, DbAddr, DbExpr, PrFn, db_regs, db_stack_trace_print, pc_regs,
};
use crate::machine::intr::spl0;
use crate::net::route::{db_show_rtable, db_show_rtentry};
use crate::netinet::ip_ipsp::{TDB_POOL, Tdb, tdb_printit};
#[cfg(feature = "nfsclient")]
use crate::nfs::nfs_debug::{
    db_show_all_nfsnodes, db_show_all_nfsreqs, nfs_node_print, nfs_request_print,
};
use crate::sys::mbuf::Mbuf;
use crate::sys::msgbuf::MSG_MAGIC;
use crate::sys::reboot::{
    RB_AUTOBOOT, RB_DUMP, RB_HALT, RB_NOSYNC, RB_POWERDOWN, RB_RESET, RB_TIMEBAD, RB_USERREQ,
};
use crate::sys::socket::{AF_INET, AF_INET6};
use crate::sys::socketvar::Socket;
use crate::unported;
use crate::uvm::uvm_meter::uvmexp_print;

/// `CS_OWN`: non-standard syntax.
pub const CS_OWN: i32 = 0x1;
/// `CS_MORE`: standard syntax, but may have other words at end.
pub const CS_MORE: i32 = 0x2;
/// `CS_SET_DOT`: set dot after command.
pub const CS_SET_DOT: i32 = 0x100;

/// `MAXARGS`: the arguments `call` takes at most.
const MAXARGS: usize = 11;

/// "`db_error` was called": its message is printed and the lexer flushed; the command is
/// abandoned and control goes back to the command loop (C's `longjmp(db_recover)`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DbError;

/// The result of a debugger function that can reach `db_error`.
pub type DbResult<T = ()> = Result<T, DbError>;

/// A command: `fcn(addr, have_addr, count, modif)`.
pub type DbCmdFn = fn(addr: DbExpr, have_addr: bool, count: DbExpr, modif: &[u8]) -> DbResult;

/// `struct db_command`: an entry of a command table.
pub struct DbCommand {
    /// `name`: command name.
    pub name: &'static str,
    /// `fcn`: function to call (`None` for an entry that only has a `more` level).
    pub fcn: Option<DbCmdFn>,
    /// `flag`: extra info: `CS_OWN`, `CS_MORE`, `CS_SET_DOT`.
    pub flag: i32,
    /// `more`: another level of command.
    pub more: Option<&'static [DbCommand]>,
}

impl DbCommand {
    /// An entry, as the C's `{ name, fcn, flag, more }` initialiser.
    pub const fn new(
        name: &'static str,
        fcn: Option<DbCmdFn>,
        flag: i32,
        more: Option<&'static [DbCommand]>,
    ) -> Self {
        Self {
            name,
            fcn,
            flag,
            more,
        }
    }
}

/// The results of `db_cmd_search`.
#[derive(Clone, Copy)]
enum CmdSearch {
    /// `CMD_UNIQUE`: the whole name matched.
    Unique(&'static DbCommand),
    /// `CMD_FOUND`: a command starts with the name.
    Found(&'static DbCommand),
    /// `CMD_NONE`: no command starts with the name.
    None,
    /// `CMD_AMBIGUOUS`: two commands in a row start with the name.
    Ambiguous,
}

/// `db_cmd_loop_done`: set by the run commands (and the machine's `ddbcpu`) to leave the
/// command loop.
pub static DB_CMD_LOOP_DONE: AtomicBool = AtomicBool::new(false);
/// `db_recover != NULL`: a command loop runs, and `db_error` returns to it.
pub static DB_RECOVER: AtomicBool = AtomicBool::new(false);
/// `db_ed_style`: if 'ed' style, 'dot' is set at start of last item printed, and '+' points
/// to next line. Otherwise 'dot' points to next item, '..' points to last.
pub static DB_ED_STYLE: AtomicBool = AtomicBool::new(true);
/// `db_dot`: current location.
pub static DB_DOT: AtomicUsize = AtomicUsize::new(0);
/// `db_last_addr`: last explicit address typed.
pub static DB_LAST_ADDR: AtomicUsize = AtomicUsize::new(0);
/// `db_prev`: last address examined or written.
pub static DB_PREV: AtomicUsize = AtomicUsize::new(0);
/// `db_next`: next address to be examined or written.
pub static DB_NEXT: AtomicUsize = AtomicUsize::new(0);
/// `db_last_command`: the command an empty line repeats. Only the CPU in the debugger
/// touches it, from `db_command_loop`.
static DB_LAST_COMMAND: StaticCell<Option<&'static DbCommand>> = StaticCell::new(None);

/// `db_printf` as a [`PrFn`], for the printers that take one.
fn db_pr(args: fmt::Arguments<'_>) {
    db_printf(args);
}

/// `db_stub!("what (file.c)")`: the body of a command whose code is not ported: says so on
/// the ddb console, reports the gap once and drops the rest of the line.
macro_rules! db_stub {
    ($what:literal) => {{
        db_printf(format_args!(concat!($what, ": not ported\n")));
        let _ = unported!($what);
        db_flush_lex();
        Ok(())
    }};
}

/// `db_skip_to_eol`: utility routine - discard tokens through end-of-line.
pub fn db_skip_to_eol() -> DbResult {
    loop {
        if db_read_token()? == tEOL {
            return Ok(());
        }
    }
}

/// `db_cmd_search`: search for command prefix.
fn db_cmd_search(name: &[u8], table: &'static [DbCommand]) -> CmdSearch {
    let mut result = CmdSearch::None;

    for cmd in table {
        let cname = cmd.name.as_bytes();
        if name == cname {
            // complete match
            return CmdSearch::Unique(cmd);
        }
        if cname.starts_with(name) {
            // end of name, not end of command - partial match
            if let CmdSearch::Found(_) = result {
                // but keep looking for a full match - this lets us match single letters
                result = CmdSearch::Ambiguous;
            } else {
                result = CmdSearch::Found(cmd);
            }
        }
    }
    result
}

/// `db_cmd_list`: prints the names of `table`'s commands in columns.
fn db_cmd_list(table: &[DbCommand]) {
    for cmd in table {
        db_printf(format_args!("{:<12}", cmd.name));
        db_end_line(12);
    }
}

/// `db_command`: reads and runs one command of `cmd_table`; `last_cmdp` is the command an
/// empty line repeats.
pub fn db_command(
    last_cmdp: &mut Option<&'static DbCommand>,
    mut cmd_table: &'static [DbCommand],
) -> DbResult {
    let mut modif = [0u8; TOK_STRING_SIZE];
    let mut modif_len = 0;
    let addr: DbExpr;
    let count: DbExpr;
    let mut have_addr = false;
    let cmd: Option<&'static DbCommand>;

    let mut t = db_read_token()?;
    if t == tEOL {
        // empty line repeats last command, at 'next'
        cmd = *last_cmdp;
        addr = DB_NEXT.load(Ordering::Relaxed) as DbExpr;
        count = 1;
    } else if t == tEXCL {
        return db_fncall(0, false, 0, b"");
    } else if t != tIDENT {
        db_printf(format_args!("?\n"));
        db_flush_lex();
        return Ok(());
    } else {
        // Search for command
        let mut found: &'static DbCommand;
        loop {
            match db_cmd_search(db_tok_string().as_bytes(), cmd_table) {
                CmdSearch::None => {
                    db_printf(format_args!("No such command\n"));
                    db_flush_lex();
                    return Ok(());
                }
                CmdSearch::Ambiguous => {
                    db_printf(format_args!("Ambiguous\n"));
                    db_flush_lex();
                    return Ok(());
                }
                CmdSearch::Unique(c) | CmdSearch::Found(c) => found = c,
            }
            let Some(more) = found.more else {
                break;
            };
            cmd_table = more;
            t = db_read_token()?;
            if t != tIDENT {
                db_cmd_list(cmd_table);
                db_flush_lex();
                return Ok(());
            }
        }
        cmd = Some(found);

        if (found.flag & CS_OWN) == 0 {
            // Standard syntax: command [/modifier] [addr] [,count]
            t = db_read_token()?;
            if t == tSLASH {
                t = db_read_token()?;
                if t != tIDENT {
                    db_printf(format_args!("Bad modifier\n"));
                    db_flush_lex();
                    return Ok(());
                }
                // db_strlcpy(modif, db_tok_string, sizeof(modif))
                let tok = db_tok_string();
                let s = tok.as_bytes();
                modif_len = s.len().min(modif.len() - 1);
                modif[..modif_len].copy_from_slice(&s[..modif_len]);
            } else {
                db_unread_token(t);
            }

            if let Some(a) = db_expression()? {
                addr = a;
                DB_DOT.store(a as DbAddr, Ordering::Relaxed);
                DB_LAST_ADDR.store(a as DbAddr, Ordering::Relaxed);
                have_addr = true;
            } else {
                addr = DB_DOT.load(Ordering::Relaxed) as DbExpr;
            }
            t = db_read_token()?;
            if t == tCOMMA {
                let Some(c) = db_expression()? else {
                    db_printf(format_args!("Count missing\n"));
                    db_flush_lex();
                    return Ok(());
                };
                count = c;
            } else {
                db_unread_token(t);
                count = -1;
            }
            if (found.flag & CS_MORE) == 0 {
                db_skip_to_eol()?;
            }
        } else {
            addr = 0;
            count = 0;
        }
    }
    *last_cmdp = cmd;
    if let Some(cmd) = cmd {
        // Execute the command.
        if let Some(fcn) = cmd.fcn {
            fcn(addr, have_addr, count, &modif[..modif_len])?;
        }

        if cmd.flag & CS_SET_DOT != 0 {
            // If command changes dot, set dot to previous address displayed (if 'ed' style).
            if DB_ED_STYLE.load(Ordering::Relaxed) {
                DB_DOT.store(DB_PREV.load(Ordering::Relaxed), Ordering::Relaxed);
            } else {
                DB_DOT.store(DB_NEXT.load(Ordering::Relaxed), Ordering::Relaxed);
            }
        }
    } else {
        // If command does not change dot, set 'next' location to be the same.
        DB_NEXT.store(DB_DOT.load(Ordering::Relaxed), Ordering::Relaxed);
    }
    Ok(())
}

/// The address a printer command was given, as a reference to a `T`; `None` for 0.
///
/// # Safety
///
/// `addr` is 0 or the address of a live `T`: the debugger's user vouches for it, as in C.
unsafe fn db_addr_as<'a, T>(addr: DbExpr) -> Option<&'a T> {
    // SAFETY: the caller's guarantee.
    unsafe { (addr as usize as *const T).as_ref() }
}

/// `db_buf_print_cmd`: `show buf[/f] addr`.
fn db_buf_print_cmd(_addr: DbExpr, _have_addr: bool, _count: DbExpr, _modif: &[u8]) -> DbResult {
    db_stub!("vfs_buf_print (vfs_subr.c)")
}

/// `db_map_print_cmd`: `show map[/f] addr`.
fn db_map_print_cmd(_addr: DbExpr, _have_addr: bool, _count: DbExpr, _modif: &[u8]) -> DbResult {
    db_stub!("uvm_map_printit (uvm_map.c)")
}

/// `db_malloc_print_cmd`: `show malloc`.
fn db_malloc_print_cmd(_addr: DbExpr, _have: bool, _count: DbExpr, _modif: &[u8]) -> DbResult {
    db_stub!("malloc_printit (kern_malloc.c)")
}

/// `db_mbuf_print_cmd`: `show mbuf[/cp] addr`: one mbuf, its chain (`c`) or its packet
/// (`p`; `cp` deep).
fn db_mbuf_print_cmd(addr: DbExpr, _have_addr: bool, _count: DbExpr, modif: &[u8]) -> DbResult {
    // SAFETY: the user named an mbuf (db_addr_as).
    let m: Option<&Mbuf> = unsafe { db_addr_as(addr) };
    match modif {
        [b'c', b'p', ..] | [b'p', b'c', ..] => m_print_packet(m, true, db_pr),
        [b'c', ..] => m_print_chain(m, false, db_pr),
        [b'p', ..] => m_print_packet(m, false, db_pr),
        _ => {
            if let Some(m) = m {
                m_print(m, db_pr);
            }
        }
    }
    Ok(())
}

/// `db_socket_print_cmd`: `show socket addr`.
fn db_socket_print_cmd(addr: DbExpr, _have: bool, _count: DbExpr, _modif: &[u8]) -> DbResult {
    // SAFETY: the user named a socket (db_addr_as).
    if let Some(so) = unsafe { db_addr_as::<Socket>(addr) } {
        so_print(so, db_pr);
    }
    Ok(())
}

/// `db_mount_print_cmd`: `show mount[/f] addr`.
fn db_mount_print_cmd(_addr: DbExpr, _have: bool, _count: DbExpr, _modif: &[u8]) -> DbResult {
    db_stub!("vfs_mount_print (vfs_subr.c)")
}

/// `db_show_all_mounts`: `show all mounts[/f]`.
fn db_show_all_mounts(_addr: DbExpr, _have: bool, _count: DbExpr, _modif: &[u8]) -> DbResult {
    db_stub!("vfs_mount_print (vfs_subr.c)")
}

/// `db_show_all_vnodes`: `show all vnodes[/f]`.
fn db_show_all_vnodes(_addr: DbExpr, _have: bool, _count: DbExpr, _modif: &[u8]) -> DbResult {
    db_stub!("vfs_vnode_print (vfs_subr.c)")
}

/// `db_show_all_bufs`: `show all bufs[/f]`.
fn db_show_all_bufs(_addr: DbExpr, _have: bool, _count: DbExpr, _modif: &[u8]) -> DbResult {
    db_stub!("vfs_buf_print (vfs_subr.c)")
}

/// `pool_walk`'s printer for `show all tdbs`: `tdb_printit` on an item of `tdb_pool`.
fn db_tdb_walk(v: *const u8, full: bool, _pr: PoolWalkPr) {
    // SAFETY: pool_walk hands over the items of tdb_pool that are in use: whole TDBs.
    let tdb = unsafe { &*v.cast::<Tdb>() };
    tdb_printit(tdb, full, db_pr);
}

/// `db_show_all_tdbs`: `show all tdbs[/f]`.
fn db_show_all_tdbs(_addr: DbExpr, _have: bool, _count: DbExpr, modif: &[u8]) -> DbResult {
    let full = modif.first() == Some(&b'f');

    pool_walk(&TDB_POOL, full, db_printf, db_tdb_walk);
    Ok(())
}

/// `db_show_all_routes`: `show all routes[/iI] [rtableid][,count]`: the IPv4 and IPv6 trees
/// (`i`: IPv4 only, `I`: IPv6 only) of `count` routing tables.
fn db_show_all_routes(addr: DbExpr, have_addr: bool, count: DbExpr, modif: &[u8]) -> DbResult {
    let mut rtableid: u32 = 0;

    if have_addr {
        rtableid = addr as u32;
    }
    let mut count = if count == -1 { 1 } else { count };

    while count != 0 {
        count -= 1;
        if modif.first() != Some(&b'I') {
            let _ = db_show_rtable(AF_INET, rtableid);
        }
        if modif.first() != Some(&b'i') {
            let _ = db_show_rtable(AF_INET6, rtableid);
        }
        rtableid = rtableid.wrapping_add(1);
    }
    Ok(())
}

/// `db_show_route`: `show route addr`.
fn db_show_route(addr: DbExpr, _have_addr: bool, _count: DbExpr, _modif: &[u8]) -> DbResult {
    // SAFETY: the user named a route (db_addr_as).
    if let Some(rt) = unsafe { db_addr_as(addr) } {
        let _ = db_show_rtentry(rt, u32::MAX);
    }
    Ok(())
}

/// `db_object_print_cmd`: `show object[/f] addr`.
fn db_object_print_cmd(_addr: DbExpr, _have: bool, _count: DbExpr, _modif: &[u8]) -> DbResult {
    db_stub!("uvm_object_printit (uvm_object.c)")
}

/// `db_page_print_cmd`: `show page[/f] addr`.
fn db_page_print_cmd(_addr: DbExpr, _have: bool, _count: DbExpr, _modif: &[u8]) -> DbResult {
    db_stub!("uvm_page_printit (uvm_page.c)")
}

/// `db_vnode_print_cmd`: `show vnode[/f] addr`.
fn db_vnode_print_cmd(_addr: DbExpr, _have: bool, _count: DbExpr, _modif: &[u8]) -> DbResult {
    db_stub!("vfs_vnode_print (vfs_subr.c)")
}

/// `db_nfsreq_print_cmd`: `show nfsreq[/f] addr`.
#[cfg(feature = "nfsclient")]
fn db_nfsreq_print_cmd(addr: DbExpr, _have: bool, _count: DbExpr, modif: &[u8]) -> DbResult {
    let full = modif.first() == Some(&b'f');

    // nfs_request_print reads a whole struct nfsreq at the address: the user vouches for it,
    // as in C, except for 0, which Rust may not read through.
    if addr != 0 {
        nfs_request_print(addr as usize as *const u8, full, db_printf);
    }
    Ok(())
}

/// `db_nfsnode_print_cmd`: `show nfsnode[/f] addr`.
#[cfg(feature = "nfsclient")]
fn db_nfsnode_print_cmd(addr: DbExpr, _have: bool, _count: DbExpr, modif: &[u8]) -> DbResult {
    let full = modif.first() == Some(&b'f');

    // As in db_nfsreq_print_cmd, for a struct nfsnode.
    if addr != 0 {
        nfs_node_print(addr as usize as *const u8, full, db_printf);
    }
    Ok(())
}

/// `db_swap_print_cmd`: `show swap`.
fn db_swap_print_cmd(_addr: DbExpr, _have: bool, _count: DbExpr, _modif: &[u8]) -> DbResult {
    db_stub!("swap_print_all (uvm_swap.c)")
}

/// `db_show_panic_cmd`: `show panic`: the panic message, `*` before the panicking CPU.
fn db_show_panic_cmd(_addr: DbExpr, _have: bool, _count: DbExpr, _modif: &[u8]) -> DbResult {
    let mut msg = [0u8; 512];
    if panicstr_message(&mut msg) {
        db_printf(format_args!(
            "*cpu{}: {}\n",
            Machine::cpu_info_unit(curcpu()),
            Str(&msg)
        ));
    } else {
        db_printf(format_args!("the kernel did not panic\n")); // yet
    }
    Ok(())
}

/// `db_extent_print_cmd`: `show extents`.
fn db_extent_print_cmd(_addr: DbExpr, _have: bool, _count: DbExpr, _modif: &[u8]) -> DbResult {
    db_stub!("extent_print_all (subr_extent.c)")
}

/// `db_pool_print_cmd`: `show pool[/clp] addr`.
fn db_pool_print_cmd(_addr: DbExpr, _have: bool, _count: DbExpr, _modif: &[u8]) -> DbResult {
    db_stub!("pool_printit (subr_pool.c)")
}

/// `db_proc_print_cmd`: `show proc[/t] [addr]`: the current thread, the one at `addr`, or
/// with `t` the one whose thread id is `addr`.
fn db_proc_print_cmd(addr: DbExpr, have_addr: bool, _count: DbExpr, modif: &[u8]) -> DbResult {
    let mut addr = addr;
    if !have_addr {
        addr = curproc().map_or(0, |p| core::ptr::from_ref(p) as DbExpr);
    }
    if modif.first() == Some(&b't') {
        addr = tfind(addr as i32).map_or(0, |p| core::ptr::from_ref(p) as DbExpr);
        if addr == 0 {
            db_printf(format_args!("not found\n"));
            return Ok(());
        }
    }

    db_stub!("proc_printit (kern_proc.c)")
}

/// `db_tdb_print_cmd`: `show tdb[/f] addr`.
fn db_tdb_print_cmd(addr: DbExpr, _have_addr: bool, _count: DbExpr, modif: &[u8]) -> DbResult {
    let full = modif.first() == Some(&b'f');

    // SAFETY: the user named a TDB (db_addr_as).
    if let Some(tdb) = unsafe { db_addr_as::<Tdb>(addr) } {
        tdb_printit(tdb, full, db_pr);
    }
    Ok(())
}

/// `db_uvmexp_print_cmd`: `show uvmexp`.
fn db_uvmexp_print_cmd(_addr: DbExpr, _have: bool, _count: DbExpr, _modif: &[u8]) -> DbResult {
    uvmexp_print(db_pr);
    Ok(())
}

/// `db_bcstats_print_cmd`: `show bcstats`.
fn db_bcstats_print_cmd(_addr: DbExpr, _have: bool, _count: DbExpr, _modif: &[u8]) -> DbResult {
    db_stub!("bcstats_print (vfs_bio.c)")
}

/// `db_show_all_procs` (`kern_proc.c`): `ps[/anowt]`, `show all procs`.
pub fn db_show_all_procs(_addr: DbExpr, _have: bool, _count: DbExpr, _modif: &[u8]) -> DbResult {
    db_stub!("db_show_all_procs (kern_proc.c)")
}

/// `db_show_callout` (`kern_timeout.c`): `callout`, `show all callout`.
fn db_show_callout(_addr: DbExpr, _have: bool, _count: DbExpr, _modif: &[u8]) -> DbResult {
    db_stub!("db_show_callout (kern_timeout.c)")
}

/// `db_show_all_clockintr` (`kern_clockintr.c`): `show all clockintr`.
fn db_show_all_clockintr(_addr: DbExpr, _have: bool, _count: DbExpr, _modif: &[u8]) -> DbResult {
    db_stub!("db_show_all_clockintr (kern_clockintr.c)")
}

/// `db_show_all_pools` (`subr_pool.c`): `show all pools`.
fn db_show_all_pools(_addr: DbExpr, _have: bool, _count: DbExpr, _modif: &[u8]) -> DbResult {
    db_stub!("db_show_all_pools (subr_pool.c)")
}

/// `db_listbreak_cmd` (`db_break.c`): `show breaks`.
fn db_listbreak_cmd(_addr: DbExpr, _have: bool, _count: DbExpr, _modif: &[u8]) -> DbResult {
    db_stub!("db_listbreak_cmd (db_break.c)")
}

/// `db_listwatch_cmd` (`db_watch.c`): `show watches`.
fn db_listwatch_cmd(_addr: DbExpr, _have: bool, _count: DbExpr, _modif: &[u8]) -> DbResult {
    db_stub!("db_listwatch_cmd (db_watch.c)")
}

/// `db_ctf_show_struct` (`db_ctf.c`): `show struct`.
fn db_ctf_show_struct(_addr: DbExpr, _have: bool, _count: DbExpr, _modif: &[u8]) -> DbResult {
    db_stub!("db_ctf_show_struct (db_ctf.c)")
}

/// 'show all' commands.
const DB_SHOW_ALL_CMDS: &[DbCommand] = &[
    DbCommand::new("procs", Some(db_show_all_procs), 0, None),
    DbCommand::new("callout", Some(db_show_callout), 0, None),
    DbCommand::new("clockintr", Some(db_show_all_clockintr), 0, None),
    DbCommand::new("pools", Some(db_show_all_pools), 0, None),
    DbCommand::new("mounts", Some(db_show_all_mounts), 0, None),
    DbCommand::new("vnodes", Some(db_show_all_vnodes), 0, None),
    DbCommand::new("bufs", Some(db_show_all_bufs), 0, None),
    DbCommand::new("routes", Some(db_show_all_routes), 0, None),
    #[cfg(feature = "nfsclient")]
    DbCommand::new("nfsreqs", Some(db_show_all_nfsreqs), 0, None),
    #[cfg(feature = "nfsclient")]
    DbCommand::new("nfsnodes", Some(db_show_all_nfsnodes), 0, None),
    DbCommand::new("tdbs", Some(db_show_all_tdbs), 0, None),
];

/// 'show' commands.
const DB_SHOW_CMDS: &[DbCommand] = &[
    DbCommand::new("all", None, 0, Some(DB_SHOW_ALL_CMDS)),
    DbCommand::new("bcstats", Some(db_bcstats_print_cmd), 0, None),
    DbCommand::new("breaks", Some(db_listbreak_cmd), 0, None),
    DbCommand::new("buf", Some(db_buf_print_cmd), 0, None),
    DbCommand::new("extents", Some(db_extent_print_cmd), 0, None),
    DbCommand::new("malloc", Some(db_malloc_print_cmd), 0, None),
    DbCommand::new("map", Some(db_map_print_cmd), 0, None),
    DbCommand::new("mbuf", Some(db_mbuf_print_cmd), 0, None),
    DbCommand::new("mount", Some(db_mount_print_cmd), 0, None),
    #[cfg(feature = "nfsclient")]
    DbCommand::new("nfsreq", Some(db_nfsreq_print_cmd), 0, None),
    #[cfg(feature = "nfsclient")]
    DbCommand::new("nfsnode", Some(db_nfsnode_print_cmd), 0, None),
    DbCommand::new("object", Some(db_object_print_cmd), 0, None),
    DbCommand::new("page", Some(db_page_print_cmd), 0, None),
    DbCommand::new("panic", Some(db_show_panic_cmd), 0, None),
    DbCommand::new("pool", Some(db_pool_print_cmd), 0, None),
    DbCommand::new("proc", Some(db_proc_print_cmd), 0, None),
    DbCommand::new("registers", Some(db_show_regs), 0, None),
    DbCommand::new("route", Some(db_show_route), 0, None),
    DbCommand::new("socket", Some(db_socket_print_cmd), 0, None),
    DbCommand::new("struct", Some(db_ctf_show_struct), CS_OWN, None),
    DbCommand::new("swap", Some(db_swap_print_cmd), 0, None),
    DbCommand::new("tdb", Some(db_tdb_print_cmd), 0, None),
    DbCommand::new("uvmexp", Some(db_uvmexp_print_cmd), 0, None),
    DbCommand::new("vnode", Some(db_vnode_print_cmd), 0, None),
    DbCommand::new("watches", Some(db_listwatch_cmd), 0, None),
];

/// 'boot' commands.
const DB_BOOT_CMDS: &[DbCommand] = &[
    DbCommand::new("sync", Some(db_boot_sync_cmd), 0, None),
    DbCommand::new("crash", Some(db_boot_crash_cmd), 0, None),
    DbCommand::new("dump", Some(db_boot_dump_cmd), 0, None),
    DbCommand::new("halt", Some(db_boot_halt_cmd), 0, None),
    DbCommand::new("reboot", Some(db_boot_reboot_cmd), 0, None),
    DbCommand::new("poweroff", Some(db_boot_poweroff_cmd), 0, None),
];

/// `db_kill_cmd` (`kern_proc.c`): `kill pid`.
fn db_kill_cmd(_addr: DbExpr, _have: bool, _count: DbExpr, _modif: &[u8]) -> DbResult {
    db_stub!("db_kill_cmd (kern_proc.c)")
}

/// `db_stop_cmd` (`kern_proc.c`): `stop pid`.
fn db_stop_cmd(_addr: DbExpr, _have: bool, _count: DbExpr, _modif: &[u8]) -> DbResult {
    db_stub!("db_stop_cmd (kern_proc.c)")
}

/// `db_print_cmd` (`db_examine.c`): `print[/axzodurc] expr`.
fn db_print_cmd(_addr: DbExpr, _have: bool, _count: DbExpr, _modif: &[u8]) -> DbResult {
    db_stub!("db_print_cmd (db_examine.c)")
}

/// `db_ctf_pprint_cmd` (`db_ctf.c`): `pprint`.
fn db_ctf_pprint_cmd(_addr: DbExpr, _have: bool, _count: DbExpr, _modif: &[u8]) -> DbResult {
    db_stub!("db_ctf_pprint_cmd (db_ctf.c)")
}

/// `db_examine_cmd` (`db_examine.c`): `examine[/modifiers] addr[,count]`.
fn db_examine_cmd(_addr: DbExpr, _have: bool, _count: DbExpr, _modif: &[u8]) -> DbResult {
    db_stub!("db_examine_cmd (db_examine.c)")
}

/// `db_search_cmd` (`db_examine.c`): `search[/bhl] addr value [mask] [,count]`.
fn db_search_cmd(_addr: DbExpr, _have: bool, _count: DbExpr, _modif: &[u8]) -> DbResult {
    db_stub!("db_search_cmd (db_examine.c)")
}

/// `db_delete_cmd` (`db_break.c`): `delete addr`.
fn db_delete_cmd(_addr: DbExpr, _have: bool, _count: DbExpr, _modif: &[u8]) -> DbResult {
    db_stub!("db_delete_cmd (db_break.c)")
}

/// `db_breakpoint_cmd` (`db_break.c`): `break addr[,count]`.
fn db_breakpoint_cmd(_addr: DbExpr, _have: bool, _count: DbExpr, _modif: &[u8]) -> DbResult {
    db_stub!("db_breakpoint_cmd (db_break.c)")
}

/// `db_deletewatch_cmd` (`db_watch.c`): `dwatch addr`.
fn db_deletewatch_cmd(_addr: DbExpr, _have: bool, _count: DbExpr, _modif: &[u8]) -> DbResult {
    db_stub!("db_deletewatch_cmd (db_watch.c)")
}

/// `db_watchpoint_cmd` (`db_watch.c`): `watch addr[,size]`.
fn db_watchpoint_cmd(_addr: DbExpr, _have: bool, _count: DbExpr, _modif: &[u8]) -> DbResult {
    db_stub!("db_watchpoint_cmd (db_watch.c)")
}

/// `db_hangman` (`db_hangman.c`): `hangman`.
fn db_hangman(_addr: DbExpr, _have: bool, _count: DbExpr, _modif: &[u8]) -> DbResult {
    db_stub!("db_hangman (db_hangman.c)")
}

/// `db_command_table[]`: the top-level commands; `machine` (`DB_MACHINE_COMMANDS`) must be
/// the first entry.
pub const DB_COMMAND_TABLE: &[DbCommand] = &[
    DbCommand::new("machine", None, 0, Some(DB_MACHINE_COMMAND_TABLE)),
    DbCommand::new("kill", Some(db_kill_cmd), 0, None),
    DbCommand::new("stop", Some(db_stop_cmd), 0, None),
    DbCommand::new("print", Some(db_print_cmd), 0, None),
    DbCommand::new("p", Some(db_print_cmd), 0, None),
    DbCommand::new("pprint", Some(db_ctf_pprint_cmd), CS_OWN, None),
    DbCommand::new("examine", Some(db_examine_cmd), CS_SET_DOT, None),
    DbCommand::new("x", Some(db_examine_cmd), CS_SET_DOT, None),
    DbCommand::new("search", Some(db_search_cmd), CS_OWN | CS_SET_DOT, None),
    DbCommand::new("set", Some(db_set_cmd), CS_OWN, None),
    DbCommand::new("write", Some(db_write_cmd), CS_MORE | CS_SET_DOT, None),
    DbCommand::new("w", Some(db_write_cmd), CS_MORE | CS_SET_DOT, None),
    DbCommand::new("delete", Some(db_delete_cmd), 0, None),
    DbCommand::new("d", Some(db_delete_cmd), 0, None),
    DbCommand::new("break", Some(db_breakpoint_cmd), 0, None),
    DbCommand::new("dwatch", Some(db_deletewatch_cmd), 0, None),
    DbCommand::new("watch", Some(db_watchpoint_cmd), CS_MORE, None),
    DbCommand::new("step", Some(db_single_step_cmd), 0, None),
    DbCommand::new("s", Some(db_single_step_cmd), 0, None),
    DbCommand::new("continue", Some(db_continue_cmd), 0, None),
    DbCommand::new("c", Some(db_continue_cmd), 0, None),
    DbCommand::new("until", Some(db_trace_until_call_cmd), 0, None),
    DbCommand::new("next", Some(db_trace_until_matching_cmd), 0, None),
    DbCommand::new("match", Some(db_trace_until_matching_cmd), 0, None),
    DbCommand::new("trace", Some(db_stack_trace_cmd), 0, None),
    DbCommand::new("bt", Some(db_stack_trace_cmd), 0, None),
    DbCommand::new("call", Some(db_fncall), CS_OWN, None),
    DbCommand::new("ps", Some(db_show_all_procs), 0, None),
    DbCommand::new("callout", Some(db_show_callout), 0, None),
    DbCommand::new("reboot", Some(db_boot_reboot_cmd), 0, None),
    DbCommand::new("show", None, 0, Some(DB_SHOW_CMDS)),
    DbCommand::new("boot", None, 0, Some(DB_BOOT_CMDS)),
    DbCommand::new("help", Some(db_help_cmd), 0, None),
    DbCommand::new("hangman", Some(db_hangman), 0, None),
    DbCommand::new("dmesg", Some(db_dmesg_cmd), 0, None),
];

/// `db_help_cmd`: `help`, the list of commands.
fn db_help_cmd(_addr: DbExpr, _haddr: bool, _count: DbExpr, _modif: &[u8]) -> DbResult {
    db_cmd_list(DB_COMMAND_TABLE);
    Ok(())
}

/// `db_command_loop`: the debugger's read-eval loop, until a command sets
/// `db_cmd_loop_done`.
pub fn db_command_loop() {
    // Initialize 'prev' and 'next' to dot.
    let dot = DB_DOT.load(Ordering::Relaxed);
    DB_PREV.store(dot, Ordering::Relaxed);
    DB_NEXT.store(dot, Ordering::Relaxed);

    DB_CMD_LOOP_DONE.store(false, Ordering::Relaxed);

    // savejmp = db_recover; db_recover = &db_jmpbuf; setjmp: a DbError ends up here.
    let savejmp = DB_RECOVER.swap(true, Ordering::Relaxed);

    while !DB_CMD_LOOP_DONE.load(Ordering::Relaxed) {
        DB_QUIT.store(false, Ordering::Relaxed);
        if db_print_position() != 0 {
            db_printf(format_args!("\n"));
        }
        DB_OUTPUT_LINE.store(0, Ordering::Relaxed);

        #[cfg(feature = "multiprocessor")]
        db_printf(format_args!(
            "ddb{{{}}}> ",
            Machine::cpu_info_unit(curcpu())
        ));
        #[cfg(not(feature = "multiprocessor"))]
        db_printf(format_args!("ddb> "));
        let _ = db_read_line();

        // SAFETY: only the CPU in the debugger runs the command loop, and nothing a command
        // does touches db_last_command.
        let mut last = unsafe { DB_LAST_COMMAND.read() };
        // An Err is a db_error, already printed: back to the prompt, as the longjmp did.
        let _ = db_command(&mut last, DB_COMMAND_TABLE);
        // SAFETY: as above.
        unsafe { DB_LAST_COMMAND.write(last) };
    }

    DB_RECOVER.store(savejmp, Ordering::Relaxed);
}

/// `db_error`: prints `s` (if any), flushes the lexer and returns the [`DbError`] the caller
/// propagates back to the command loop: `return Err(db_error(Some("...")))`.
pub fn db_error(s: Option<&str>) -> DbError {
    if let Some(s) = s {
        db_printf(format_args!("{s}"));
    }
    db_flush_lex();
    DbError
}

/// The type `db_fncall` calls a function as: ten `long` arguments, a `long` result.
type DbFncallFn = extern "C" fn(
    DbExpr,
    DbExpr,
    DbExpr,
    DbExpr,
    DbExpr,
    DbExpr,
    DbExpr,
    DbExpr,
    DbExpr,
    DbExpr,
) -> DbExpr;

/// `db_fncall`: call random function: `!expr(arg,arg,arg)` or `call expr(...)`.
fn db_fncall(_addr: DbExpr, _have_addr: bool, _count: DbExpr, _modif: &[u8]) -> DbResult {
    let mut args: [DbExpr; MAXARGS] = [0; MAXARGS];
    let mut nargs = 0;

    let Some(fn_addr) = db_expression()?.filter(|&a| a != 0) else {
        db_printf(format_args!("Bad function\n"));
        db_flush_lex();
        return Ok(());
    };

    let mut t = db_read_token()?;
    if t == tLPAREN {
        if let Some(a) = db_expression()? {
            args[0] = a;
            nargs += 1;
            loop {
                t = db_read_token()?;
                if t != tCOMMA {
                    break;
                }
                if nargs == MAXARGS {
                    db_printf(format_args!("Too many arguments\n"));
                    db_flush_lex();
                    return Ok(());
                }
                let Some(a) = db_expression()? else {
                    db_printf(format_args!("Argument missing\n"));
                    db_flush_lex();
                    return Ok(());
                };
                args[nargs] = a;
                nargs += 1;
            }
            db_unread_token(t);
        }
        if db_read_token()? != tRPAREN {
            db_printf(format_args!("?\n"));
            db_flush_lex();
            return Ok(());
        }
    }
    db_skip_to_eol()?;

    // SAFETY: the debugger's user names a kernel function taking up to ten `long`s, as in C;
    // ddb trusts its operator. The address is not 0 (checked above), so it is a valid
    // function pointer value.
    let func = unsafe { core::mem::transmute::<usize, DbFncallFn>(fn_addr as usize) };
    let retval = func(
        args[0], args[1], args[2], args[3], args[4], args[5], args[6], args[7], args[8], args[9],
    );
    let mut tmpfmt = [0u8; DB_FORMAT_BUF_SIZE];
    db_printf(format_args!(
        "{}\n",
        Str(db_format(&mut tmpfmt, retval, DB_FORMAT_N, true, 0))
    ));
    Ok(())
}

/// `db_reboot`: reboots with `howto` from the debugger.
fn db_reboot(howto: i32) -> ! {
    spl0();
    if curproc().is_none() {
        Machine::set_curproc(curcpu(), &PROC0);
    }
    reboot(howto)
}

/// `db_boot_sync_cmd`: `boot sync`.
fn db_boot_sync_cmd(_addr: DbExpr, _haddr: bool, _count: DbExpr, _modif: &[u8]) -> DbResult {
    db_reboot(RB_AUTOBOOT | RB_TIMEBAD | RB_USERREQ)
}

/// `db_boot_crash_cmd`: `boot crash`.
fn db_boot_crash_cmd(_addr: DbExpr, _haddr: bool, _count: DbExpr, _modif: &[u8]) -> DbResult {
    db_reboot(RB_NOSYNC | RB_DUMP | RB_TIMEBAD | RB_USERREQ)
}

/// `db_boot_dump_cmd`: `boot dump`.
fn db_boot_dump_cmd(_addr: DbExpr, _haddr: bool, _count: DbExpr, _modif: &[u8]) -> DbResult {
    db_reboot(RB_DUMP | RB_TIMEBAD | RB_USERREQ)
}

/// `db_boot_halt_cmd`: `boot halt`.
fn db_boot_halt_cmd(_addr: DbExpr, _haddr: bool, _count: DbExpr, _modif: &[u8]) -> DbResult {
    db_reboot(RB_NOSYNC | RB_HALT | RB_TIMEBAD | RB_USERREQ)
}

/// `db_boot_reboot_cmd`: `boot reboot`, `reboot`.
fn db_boot_reboot_cmd(_addr: DbExpr, _haddr: bool, _count: DbExpr, _modif: &[u8]) -> DbResult {
    boot(RB_RESET | RB_AUTOBOOT | RB_NOSYNC | RB_TIMEBAD | RB_USERREQ)
}

/// `db_boot_poweroff_cmd`: `boot poweroff`.
fn db_boot_poweroff_cmd(_addr: DbExpr, _haddr: bool, _count: DbExpr, _modif: &[u8]) -> DbResult {
    db_reboot(RB_NOSYNC | RB_HALT | RB_POWERDOWN | RB_TIMEBAD | RB_USERREQ)
}

/// `db_dmesg_cmd`: `dmesg`, the kernel message buffer from its oldest byte.
fn db_dmesg_cmd(_addr: DbExpr, _haddr: bool, _count: DbExpr, _modif: &[u8]) -> DbResult {
    let Some(mbp) = msgbufp() else {
        return Ok(());
    };
    if mbp.magic() != MSG_MAGIC {
        return Ok(());
    }
    let bufs = usize::try_from(mbp.bufs()).unwrap_or(0);
    let bufc = mbp.bufc();
    let mut off = usize::try_from(mbp.bufx()).unwrap_or(0);
    if off > bufs {
        off = 0;
    }
    let mut p = off;
    for _ in 0..bufs {
        if p >= bufs {
            p = 0;
        }
        let c = bufc[p].get();
        if c != 0 {
            db_putchar(i32::from(c));
        }
        p += 1;
    }
    db_putchar(i32::from(b'\n'));
    Ok(())
}

/// `db_stack_trace_cmd`: `trace[/tu] [addr][,count]`, `bt`.
fn db_stack_trace_cmd(addr: DbExpr, have_addr: bool, count: DbExpr, modif: &[u8]) -> DbResult {
    // A count of -1 (none given) is "no limit", as the C's signed counter makes it.
    db_stack_trace_print(addr as usize, have_addr, count as usize, modif, db_pr);
    Ok(())
}

/// `db_find_xtrn_sym_and_offset` (`db_sym.c`): the symbol at or before `value` and the
/// offset from it. Not ported (`db_show_regs` reports the gap): no symbol table, so no
/// symbol, as the C with an empty one.
fn db_find_xtrn_sym_and_offset(_value: DbAddr) -> Option<(&'static str, DbExpr)> {
    None
}

/// `db_show_regs`: `show registers`, the registers of `ddb_regs` and where the PC is.
fn db_show_regs(_addr: DbExpr, _have_addr: bool, _count: DbExpr, _modif: &[u8]) -> DbResult {
    let mut tmpfmt = [0u8; DB_FORMAT_BUF_SIZE];

    // Reported before the listing, so the console line does not land inside it.
    let _ = unported!("db_find_xtrn_sym_and_offset (db_sym.c)");

    for regp in db_regs() {
        let mut value: DbExpr = 0;
        db_read_variable(regp, &mut value);
        db_printf(format_args!(
            "{:<12}{}",
            regp.name,
            Str(db_format(
                &mut tmpfmt,
                value,
                DB_FORMAT_N,
                true,
                size_of::<i64>() * 3
            ))
        ));
        if let Some((name, offset)) = db_find_xtrn_sym_and_offset(value as DbAddr)
            && offset <= DbExpr::from(DB_MAXOFF.load(Ordering::Relaxed))
            && offset != value
        {
            db_printf(format_args!("\t{name}"));
            if offset != 0 {
                db_printf(format_args!(
                    "+{}",
                    Str(db_format(&mut tmpfmt, offset, DB_FORMAT_R, true, 0))
                ));
            }
        }
        db_printf(format_args!("\n"));
    }
    db_print_loc_and_inst(pc_regs());
    Ok(())
}

/// `db_write_cmd`: `write[/bhlq] addr expr [expr ...]`, write to memory.
fn db_write_cmd(address: DbExpr, _have_addr: bool, _count: DbExpr, modif: &[u8]) -> DbResult {
    let mut addr = address as DbAddr;
    let mut wrote_one = false;
    let mut tmpfmt = [0u8; DB_FORMAT_BUF_SIZE];

    let size: usize = match modif.first() {
        Some(b'b') => 1,
        Some(b'h') => 2,
        Some(b'l') | None => 4,
        Some(b'q') => 8,
        _ => return Err(db_error(Some("Unknown size\n"))),
    };

    while let Some(new_value) = db_expression()? {
        let old_value = db_get_value(addr, size, false)?;
        db_printsym(addr, db_pr);
        db_printf(format_args!(
            "\t\t{}\t",
            Str(db_format(&mut tmpfmt, old_value, DB_FORMAT_N, false, 8))
        ));
        db_printf(format_args!(
            "=\t{}\n",
            Str(db_format(&mut tmpfmt, new_value, DB_FORMAT_N, false, 8))
        ));
        db_put_value(addr, size, new_value)?;
        addr = addr.wrapping_add(size);

        wrote_one = true;
    }

    if !wrote_one {
        return Err(db_error(Some("Nothing written.\n")));
    }

    DB_NEXT.store(addr, Ordering::Relaxed);
    DB_PREV.store(addr.wrapping_sub(size), Ordering::Relaxed);

    db_skip_to_eol()
}

// Stand-ins for the ddb files that are not ported yet (db_access.c, db_sym.c, db_examine.c).
// Each is a visible stub: it reports the gap, and fails the command where the C would have
// read or written memory.

/// `db_get_value` (`db_access.c`): reads `size` bytes at `addr`. Not ported: fails the
/// command.
pub fn db_get_value(addr: DbAddr, size: usize, is_signed: bool) -> DbResult<DbExpr> {
    let _ = (addr, size, is_signed);
    let _ = unported!("db_get_value (db_access.c)");
    Err(db_error(Some(
        "db_get_value: memory access is not ported\n",
    )))
}

/// `db_put_value` (`db_access.c`): writes `size` bytes of `value` at `addr`. Not ported:
/// fails the command.
pub fn db_put_value(addr: DbAddr, size: usize, value: DbExpr) -> DbResult {
    let _ = (addr, size, value);
    let _ = unported!("db_put_value (db_access.c)");
    Err(db_error(Some(
        "db_put_value: memory access is not ported\n",
    )))
}

/// `db_printsym(off, DB_STGY_ANY, pr)` (`db_sym.c`): the symbol for `off`. Not ported: no
/// symbol table, so the address as a number, as the C prints an address without a symbol.
pub fn db_printsym(off: DbAddr, pr: PrFn) {
    let _ = unported!("db_printsym (db_sym.c)");
    let mut buf = [0u8; DB_FORMAT_BUF_SIZE];
    pr(format_args!(
        "{}",
        Str(db_format(&mut buf, off as DbExpr, DB_FORMAT_N, true, 0))
    ));
}

/// `db_print_loc_and_inst` (`db_examine.c`): the symbol and the disassembled instruction at
/// `loc`. Not ported: prints the address and ends the line.
pub fn db_print_loc_and_inst(loc: DbAddr) {
    db_printf(format_args!("{loc:#x}\n"));
    let _ = unported!("db_print_loc_and_inst (db_examine.c: symbols, disassembler)");
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ddb::db_lex::{db_set_line, db_test_lock};
    use crate::ddb::db_output::DB_TAB_STOP_WIDTH;

    /// The name of the command `db_cmd_search` found for `name` in `table`, with its kind.
    fn search(name: &str, table: &'static [DbCommand]) -> (&'static str, &'static str) {
        match db_cmd_search(name.as_bytes(), table) {
            CmdSearch::Unique(c) => ("unique", c.name),
            CmdSearch::Found(c) => ("found", c.name),
            CmdSearch::None => ("none", ""),
            CmdSearch::Ambiguous => ("ambiguous", ""),
        }
    }

    #[test]
    fn search_prefixes() {
        // a whole name wins even when longer names start with it
        assert_eq!(search("c", DB_COMMAND_TABLE), ("unique", "c"));
        assert_eq!(search("continue", DB_COMMAND_TABLE), ("unique", "continue"));
        // a unique prefix
        assert_eq!(search("cont", DB_COMMAND_TABLE), ("found", "continue"));
        assert_eq!(search("he", DB_COMMAND_TABLE), ("found", "help"));
        assert_eq!(search("reg", DB_SHOW_CMDS), ("found", "registers"));
        // two prefixes in a row are ambiguous ...
        assert_eq!(search("tr", DB_COMMAND_TABLE), ("found", "trace"));
        assert_eq!(search("wa", DB_COMMAND_TABLE), ("found", "watch"));
        assert_eq!(search("st", DB_COMMAND_TABLE), ("ambiguous", ""));
        // ... and, as in C, a third one makes it "found" again (break, bt, boot: boot)
        assert_eq!(search("b", DB_COMMAND_TABLE), ("found", "boot"));
        // not found
        assert_eq!(search("frobnicate", DB_COMMAND_TABLE), ("none", ""));
        assert_eq!(search("machine", DB_COMMAND_TABLE), ("unique", "machine"));
    }

    #[test]
    fn run_commands() {
        let _g = db_test_lock();
        let mut last = None;

        // `set` runs with its own syntax
        db_set_line(b"set $tabstops = 4\n");
        assert_eq!(db_command(&mut last, DB_COMMAND_TABLE), Ok(()));
        assert_eq!(DB_TAB_STOP_WIDTH.load(Ordering::Relaxed), 4);
        assert_eq!(last.map(|c| c.name), Some("set"));
        db_set_line(b"set $tabstops 8\n");
        assert_eq!(db_command(&mut last, DB_COMMAND_TABLE), Ok(()));
        assert_eq!(DB_TAB_STOP_WIDTH.load(Ordering::Relaxed), 8);

        // the standard syntax sets dot and the last address; errors come back as DbError
        db_set_line(b"help 0x1234\n");
        assert_eq!(db_command(&mut last, DB_COMMAND_TABLE), Ok(()));
        assert_eq!(DB_DOT.load(Ordering::Relaxed), 0x1234);
        assert_eq!(DB_LAST_ADDR.load(Ordering::Relaxed), 0x1234);
        db_set_line(b"help 4/0\n");
        assert_eq!(db_command(&mut last, DB_COMMAND_TABLE), Err(DbError));
        db_set_line(b"write/z 1 2\n");
        assert_eq!(db_command(&mut last, DB_COMMAND_TABLE), Err(DbError));

        // unknown, ambiguous and bad lines print and return
        for line in [
            &b"frobnicate\n"[..],
            b"st\n",
            b"5\n",
            b"show\n",
            b"help/5\n",
        ] {
            db_set_line(line);
            assert_eq!(db_command(&mut last, DB_COMMAND_TABLE), Ok(()));
        }

        // continue ends the loop; an empty line repeats it
        DB_CMD_LOOP_DONE.store(false, Ordering::Relaxed);
        db_set_line(b"c\n");
        assert_eq!(db_command(&mut last, DB_COMMAND_TABLE), Ok(()));
        assert!(DB_CMD_LOOP_DONE.load(Ordering::Relaxed));
        DB_CMD_LOOP_DONE.store(false, Ordering::Relaxed);
        db_set_line(b"\n");
        assert_eq!(db_command(&mut last, DB_COMMAND_TABLE), Ok(()));
        assert!(DB_CMD_LOOP_DONE.load(Ordering::Relaxed));
        DB_CMD_LOOP_DONE.store(false, Ordering::Relaxed);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/ddb/db_command.h");
        for (name, value) in [
            ("CS_OWN", CS_OWN),
            ("CS_MORE", CS_MORE),
            ("CS_SET_DOT", CS_SET_DOT),
        ] {
            assert_eq!(
                crate::reftest::int(&defs, name),
                Some(i64::from(value)),
                "{name}"
            );
        }
    }
}
/* </TESTS> */
