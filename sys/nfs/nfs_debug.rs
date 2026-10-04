/*	$OpenBSD: nfs_debug.c,v 1.8 2026/05/23 22:13:17 kirill Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2009 Thordur I. Bjornsson. <thib@openbsd.org>
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

//! The NFS client's `ddb(4)` printers: `show all nfsreqs` and `show all nfsnodes` walk the
//! `nfsreqpl` and `nfs_node_pool` pools and print every request and every node in use;
//! `nfs_request_print` and `nfs_node_print` print one of them (`full` adds the second line).
//!
//! Upstream: sys/nfs/nfs_debug.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `db_show_all_nfsreqs` and `db_show_all_nfsnodes` are `ddb` commands
//!   (`ddb::db_command::DbCmdFn`: `db_expr_t` is `DbExpr`, the modifier string bytes, the
//!   result a `DbResult`); they print with `db_printf` as the C does. The printers take
//!   the printer as a `fn(fmt::Arguments) -> usize` (`db_printf`'s type) and, being the
//!   `func` of `pool_walk`, an item pointer: `nfs_request_print`/`nfs_node_print` read it as a
//!   `struct nfsreq`/`struct nfsnode`.
//! - `pool_walk` is `subr_pool.rs`'s, added with this file. `show all nfsreqs`,
//!   `show all nfsnodes`, `show nfsreq` and `show nfsnode` reach these from the `ddb>`
//!   prompt (`ddb/db_command.rs`, M11c).

use core::fmt;
use core::ptr;

use crate::ddb::db_command::DbResult;
use crate::kern::subr_pool::pool_walk;
use crate::kern::subr_prf::db_printf;
use crate::machine::db_machdep::DbExpr;
use crate::nfs::nfs::{NFS_NODE_POOL, NfsReq};
use crate::nfs::nfs_subs::NFSREQPL;
use crate::nfs::nfsnode::NfsNode;

/// The type of `db_printf`, which the printers write with.
pub type DbPrintf = fn(fmt::Arguments<'_>) -> usize;

/// `db_show_all_nfsreqs(expr, haddr, count, modif)`: `show all nfsreqs[/f]`: every request in
/// the `nfsreqpl` pool, in full with `/f`.
pub fn db_show_all_nfsreqs(_expr: DbExpr, _haddr: bool, _count: DbExpr, modif: &[u8]) -> DbResult {
    let full = modif.first() == Some(&b'f');

    pool_walk(&NFSREQPL, full, db_printf, nfs_request_print);
    Ok(())
}

/// `nfs_request_print(v, full, pr)`: prints the `struct nfsreq` at `v`.
pub fn nfs_request_print(v: *const u8, full: bool, pr: DbPrintf) {
    // SAFETY: `v` is an item of `nfsreqpl` that `pool_walk` found in use (or the address a
    // debugger user asked for): a whole `struct nfsreq`.
    let rep: &NfsReq = unsafe { &*v.cast::<NfsReq>() };
    pr(format_args!(
        "xid {:#x} flags {:#x} rexmit {} procnum {} proc {:p}\n",
        rep.r_xid.get(),
        rep.r_flags.get(),
        rep.r_rexmit.get(),
        rep.r_procnum.get(),
        rep.r_procp.get()
    ));

    if full {
        let mbuf =
            |m: Option<&'static crate::sys::mbuf::Mbuf>| m.map_or(ptr::null(), ptr::from_ref);
        pr(format_args!(
            "mreq {:p} mrep {:p} md {:p} nfsmount {:p} vnode {:p} timer {} rtt {}\n",
            mbuf(rep.r_mreq.get()),
            mbuf(rep.r_mrep.get()),
            mbuf(rep.r_md.get()),
            rep.r_nmp.get().map_or(ptr::null(), ptr::from_ref),
            rep.r_vp.get().map_or(ptr::null(), ptr::from_ref),
            rep.r_timer.get(),
            rep.r_rtt.get()
        ));
    }
}

/// `db_show_all_nfsnodes(expr, haddr, count, modif)`: `show all nfsnodes[/f]`: every node in
/// the `nfs_node_pool` pool, in full with `/f`.
pub fn db_show_all_nfsnodes(_expr: DbExpr, _haddr: bool, _count: DbExpr, modif: &[u8]) -> DbResult {
    let full = modif.first() == Some(&b'f');

    pool_walk(&NFS_NODE_POOL, full, db_printf, nfs_node_print);
    Ok(())
}

/// `nfs_node_print(v, full, pr)`: prints the `struct nfsnode` at `v`.
pub fn nfs_node_print(v: *const u8, full: bool, pr: DbPrintf) {
    // SAFETY: `v` is an item of `nfs_node_pool` that `pool_walk` found in use (or the address
    // a debugger user asked for): a whole `struct nfsnode`.
    let np: &NfsNode = unsafe { &*v.cast::<NfsNode>() };
    pr(format_args!(
        "size {} flag {} vnode {:p} accstamp {}\n",
        np.n_size.get(),
        np.n_flag.get(),
        np.n_vnode.get().map_or(ptr::null(), ptr::from_ref),
        np.n_accstamp.get()
    ));

    if full {
        pr(format_args!(
            "pushedlo {} pushedhi {} pushlo {} pushhi {}\n",
            np.n_pushedlo.get() as u64,
            np.n_pushedhi.get() as u64,
            np.n_pushlo.get() as u64,
            np.n_pushhi.get() as u64
        ));
        pr(format_args!("commitflags {}\n", np.n_commitflags.get()));
    }
}

#[cfg(test)]
mod tests;
