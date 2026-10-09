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
 * Copyright (c) 2001 Daniel Hartmeier
 * Copyright (c) 2002 - 2018 Henning Brauer <henning@openbsd.org>
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 *
 *    - Redistributions of source code must retain the above copyright
 *      notice, this list of conditions and the following disclaimer.
 *    - Redistributions in binary form must reproduce the above
 *      copyright notice, this list of conditions and the following
 *      disclaimer in the documentation and/or other materials provided
 *      with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
 * "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
 * LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS
 * FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE
 * COPYRIGHT HOLDERS OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT,
 * INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING,
 * BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
 * LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
 * CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN
 * ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 *
 * Effort sponsored in part by the Defense Advanced Research Projects
 * Agency (DARPA) and Air Force Research Laboratory, Air Force
 * Materiel Command, USAF, under agreement number F30602-01-2-0537.
 *
 */
/* </LICENSES> */

/* <CODE> */
//! `pf(4)`'s control device, `/dev/pf`: the `ioctl(2)` interface `pfctl(8)` drives, rule set
//! transactions, tags and queue names, the queue manager, the limiter configuration, and
//! `pfattach`, which sets pf up at boot.
//!
//! Upstream: sys/net/pf_ioctl.c @ 3ce1f3f79392
//!
//! The ioctl arguments arrive as `sys_ioctl`'s kernel copy (`data`). The structures that are
//! plain integers (`AbiPod`) are read with `ioctl_arg` and stored back with `ioctl_ret`; those
//! that embed pf's kernel objects (`struct pfioc_rule`, `struct pfioc_table`, `struct
//! pf_status`, ...) implement `PfAbi` and are moved into a `Box` with `pf_abi_read`, worked on,
//! and stored back with `pf_abi_write` (`net/pfvar.rs`). A kernel rule, queue or tag is built
//! in a `PfPoolNew` (an item that is not published yet, whose plain members can still be
//! written) and published as a `&'static` object once its plain members are final; the
//! members pf changes later are `Cell`s.
//!
//! ## Deviations
//! - `kstat(4)` (`NKSTAT`) is not configured: the limiter kstats (`pf_statelim_kstat_*`,
//!   `pf_sourcelim_kstat_*`) are a comment at the end of the file and at their call sites.
//! - `pf_anchor_stack` is `net/pf.rs`'s static (no `cpumem`: only `pf_lock`'s holder uses
//!   it, see `net/pf.rs`); `pfattach` sets its bottom frame through `pf_anchor_stack_init`.
//! - `pf_default_rule`'s plain members (`action` = `PF_PASS`, `rtableid` = -1) are set by its
//!   static initialiser, the members that are `Cell`s by `pfattach` as in the C.
//!   `pf_default_rule_new = pf_default_rule` copies the `Cell` members (`pf_default_rule_copy`):
//!   the plain ones are the same constants in both, and only the timeouts are read from
//!   `pf_default_rule_new`.
//! - The default rule's "never garbage collected" mark (`entries.tqe_prev` pointing at its own
//!   `tqe_next`) and the test for a linked rule (`entries.tqe_prev != NULL`) use
//!   `TailqEntry::set_prev_self`/`is_linked`/`clear_prev`.
//! - `pfioctl`'s `switch` is `pfioctl_locked`, called between `rw_enter_write(&pfioctl_rw)`
//!   and `rw_exit_write`; its `goto fail` are returns. A case that leaves a result in the
//!   argument and jumps to `fail` without an error stores the argument back before it returns.
//! - `DIOCADDRULE` and `DIOCCHANGERULE` write `cuid`/`cpid` into the rule after the ruleset
//!   checks, as in the C, but before the rule is published (they are plain members).
//! - `pf_create_queues`' `malloc`ed list of `struct pf_queue_if` is a `Vec` searched from its
//!   end (the C prepends to its list and searches from the head).
//! - `pfclose` collects the closing descriptor's transactions in a `Vec` (freed last found
//!   first, as from the head of the C's local list); `pf_statelim_commit`'s and
//!   `pf_sourcelim_commit`'s local list `l` is a static (only they use it, under `pf_lock`),
//!   so that its elements are `'static` objects.
//! - `DIOCIGETIFACES` copies out at most the buffer it allocated: `pfi_get_ifaces` returns the
//!   number of matching interfaces, which may exceed the room the caller gave, and the C then
//!   copies out past the end of its buffer.
//! - `pf_sourcelim_add`'s IPv4 mask for a prefix length of 0 is 0 (the C shifts a 32-bit
//!   value by 32, which is undefined).
//! - Functions returning 0/-1 (not an errno) return `bool`, `true` for the C's 0:
//!   `pf_rtlabel_add`. The predicates `pf_validate_range` and `pf_chk_limiter_action` return
//!   `true` where the C returns 1 (a bad range, an unknown action).
//! - `pf_addr_copyout` is `unsafe`: it reads the kernel pointer a byte copy of a rule's
//!   operand still holds (`pfi_dynaddr_copyout`, `pf_tbladdr_copyout`).
//! - `pf_statelim_get`, `pf_sourcelim_get` and `pf_source_get` take the C's lookup function
//!   (`RBT_FIND` or `RBT_NFIND`) as a `fn` pointer, as the C does.

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::cell::Cell;
use core::ffi::c_void;
use core::mem::{offset_of, size_of};
use core::ptr::{self, NonNull};
use core::sync::atomic::Ordering as AtomicOrdering;

use crate::conf::param::NMBCLUST;
use crate::crypto::md5::{MD5Final, MD5Init, MD5Update, Md5Ctx};
use crate::dev::rnd::arc4random;
use crate::kassert;
use crate::kern::kern_lock::{mtx_enter, mtx_leave, pc_lock_init};
use crate::kern::kern_malloc::{free, malloc, mallocarray};
use crate::kern::kern_rwlock::{
    rw_assert_anylock, rw_assert_wrlock, rw_enter, rw_enter_write, rw_exit, rw_exit_write,
};
use crate::kern::kern_sysctl::{SECURELEVEL, sysctl_rdstruct};
use crate::kern::kern_task::{SYSTQMP, task_add};
use crate::kern::kern_tc::{getnsecuptime, gettime, getuptime};
use crate::kern::kern_timeout::{timeout_add_sec, timeout_del};
use crate::kern::subr_pool::{pool_init, pool_sethardlimit};
use crate::kern::subr_prf::log;
use crate::machine::copy::{AbiPod, copyin, copyout, copyout_obj};
use crate::machine::intr::IPL_SOFTNET;
use crate::net::fq_codel::{IFQ_FQCODEL_OPS, PFQ_FQCODEL_OPS};
use crate::net::hfsc::{IFQ_HFSC_OPS, PFQ_HFSC_OPS, hfsc_initialize};
use crate::net::if_::{IFNAMSIZ, IFQ_MAXPRIO};
use crate::net::if_var::Ifnet;
use crate::net::ifq::{IFQ_PRIQ_OPS, IfqOps, ifq_attach};
use crate::net::pf::*;
use crate::net::pf_if::{
    pfi_all, pfi_clear_flags, pfi_dynaddr_copyout, pfi_dynaddr_remove, pfi_dynaddr_setup,
    pfi_get_ifaces, pfi_initialize, pfi_kif_alloc, pfi_kif_free, pfi_kif_get, pfi_kif_ref,
    pfi_kif_unref, pfi_set_flags, pfi_update_status, pfi_xcommit,
};
use crate::net::pf_norm::pf_normalize_init;
use crate::net::pf_osfp::{pf_osfp_add, pf_osfp_flush, pf_osfp_get, pf_osfp_initialize};
use crate::net::pf_ruleset::{
    PF_ANCHOR_PL, PF_ANCHORS, PF_MAIN_ANCHOR, pf_anchor_copyout, pf_anchor_rele, pf_anchor_setup,
    pf_anchor_take, pf_find_or_create_ruleset, pf_find_ruleset, pf_init_ruleset, pf_main_ruleset,
    pf_remove_anchor, pf_remove_if_empty_ruleset,
};
use crate::net::pf_syncookies::{
    pf_syncookies_getwats, pf_syncookies_init, pf_syncookies_setmode, pf_syncookies_setwats,
};
use crate::net::pf_table::{
    PfrBuf, pfr_add_addrs, pfr_add_tables, pfr_attach_table, pfr_clr_addrs, pfr_clr_astats,
    pfr_clr_tables, pfr_clr_tstats, pfr_del_addrs, pfr_del_tables, pfr_detach_table, pfr_get_addrs,
    pfr_get_astats, pfr_get_tables, pfr_get_tstats, pfr_ina_begin, pfr_ina_commit, pfr_ina_define,
    pfr_ina_rollback, pfr_initialize, pfr_set_addrs, pfr_set_tflags, pfr_tst_addrs,
};
use crate::net::pfvar::*;
use crate::net::pfvar_priv::*;
use crate::net::route::{rtlabel_id2name, rtlabel_name2id, rtlabel_unref};
use crate::net::rtable::rtable_exists;
use crate::netinet::in_::{IPPROTO_TCP, IPPROTO_UDP};
use crate::netinet::ip_icmp::ICMP_MAXTYPE;
use crate::sys::errno::Errno;
use crate::sys::fcntl::FWRITE;
use crate::sys::ioctl::{ioctl_arg, ioctl_ret};
use crate::sys::malloc::{M_CANFAIL, M_PF, M_WAITOK, M_ZERO};
use crate::sys::pool::{PR_LIMITFAIL, PR_NOWAIT, PR_WAITOK, Pool};
use crate::sys::proc::Proc;
use crate::sys::queue::{ListHead, TailqEntry, TailqHead};
use crate::sys::rwlock::{RW_INTR, RW_READ, Rwlock};
use crate::sys::select::NBBY;
use crate::sys::socket::{AF_INET, AF_INET6, RT_TABLEID_MAX};
use crate::sys::specdev::{CLONE_MAPSZ, CLONE_SHIFT};
use crate::sys::syslog::{LOG_ERR, LOG_NOTICE};
use crate::sys::systm::{
    PHYSMEM, net_assert_locked, net_lock, net_lock_shared, net_unlock, net_unlock_shared,
};
use crate::sys::time::sec_to_nsec;
use crate::sys::tree::{RbEntry, RbtHead};
use crate::sys::types::{Dev, SaFamily, minor};
use crate::uvm::uvm_param::atop;

/// `NPF`: the count `config(8)` writes into `pf.h` for `pseudo-device pf`.
pub const NPF: i32 = 1;

/// `PF_ORDER_HOST`: a port range in host byte order.
const PF_ORDER_HOST: i32 = 0;
/// `PF_ORDER_NET`: a port range in network byte order.
const PF_ORDER_NET: i32 = 1;

/// `PF_TSET_STATUSIF`.
const PF_TSET_STATUSIF: u32 = 0x01;
/// `PF_TSET_DEBUG`.
const PF_TSET_DEBUG: u32 = 0x02;
/// `PF_TSET_HOSTID`.
const PF_TSET_HOSTID: u32 = 0x04;
/// `PF_TSET_REASS`.
const PF_TSET_REASS: u32 = 0x08;

/// `TAGID_MAX`.
const TAGID_MAX: u32 = 50000;

/// `CACHELINESIZE` (`<sys/percpu.h>`, not ported): the alignment of the state pools.
const CACHELINESIZE: u32 = 64;

/// `pf_trans_set`: the settings a `DIOCX*` transaction stages until `DIOCXCOMMIT`.
struct PfTransSet {
    /// `statusif`.
    statusif: Cell<[u8; IFNAMSIZ]>,
    /// `debug`.
    debug: Cell<u32>,
    /// `hostid`.
    hostid: Cell<u32>,
    /// `reass`.
    reass: Cell<u32>,
    /// `mask`: `PF_TSET_*`.
    mask: Cell<u32>,
}

impl PfTransSet {
    /// `memset(&pf_trans_set, 0, sizeof(pf_trans_set))`.
    fn clear(&self) {
        self.statusif.set([0; IFNAMSIZ]);
        self.debug.set(0);
        self.hostid.set(0);
        self.reass.set(0);
        self.mask.set(0);
    }
}

crate::queue_adapter!(
    /// `TAILQ_HEAD(pf_tags, pf_tagname)`.
    pub PfTags: PfTagname, entries => TailqEntry<PfTagname>
);

// SAFETY: a queue link, a byte array and `Cell`s of integers: all zero is a valid value.
unsafe impl PfPoolItem for PfTagname {}

/// One root queue's interface while `pf_create_queues` builds the disciplines (`struct
/// pf_queue_if`).
struct PfQueueIf {
    /// `ifp`.
    ifp: &'static Ifnet,
    /// `ifqops`.
    ifqops: &'static IfqOps,
    /// `pfqops`.
    pfqops: &'static PfqOps,
    /// `disc`.
    disc: *mut c_void,
}

/// The lookup `pf_statelim_get` makes: `RBT_FIND` or `RBT_NFIND`.
pub type PfStatelimRbOp =
    fn(&'static RbtHead<PfStatelimIdTree>, &PfStatelim) -> Option<&'static PfStatelim>;

/// The lookup `pf_sourcelim_get` makes: `RBT_FIND` or `RBT_NFIND`.
pub type PfSourcelimRbOp =
    fn(&'static RbtHead<PfSourcelimIdTree>, &PfSourcelim) -> Option<&'static PfSourcelim>;

/// The lookup `pf_source_get` makes: `RBT_FIND` or `RBT_NFIND`.
pub type PfSourceRbOp =
    fn(&'static RbtHead<PfSourceIocTree>, &PfSource) -> Option<&'static PfSource>;

/// `pf_tag_pl`.
pub static PF_TAG_PL: Pool = Pool::new();

/// `pf_default_rule`: the rule a packet matches when no rule does, and the default timeouts.
pub static PF_DEFAULT_RULE: PfRule = pf_default_rule_init();
/// `pf_default_rule_new`: the default rule a transaction stages (its timeouts).
pub static PF_DEFAULT_RULE_NEW: PfRule = pf_default_rule_init();

/// `pf_trans_set`.
static PF_TRANS_SET: PfGlobal<PfTransSet> = PfGlobal(PfTransSet {
    statusif: Cell::new([0; IFNAMSIZ]),
    debug: Cell::new(0),
    hostid: Cell::new(0),
    reass: Cell::new(0),
    mask: Cell::new(0),
});

/// `pf_tags`: the tag names in use, sorted by tag.
pub static PF_TAGS: PfGlobal<TailqHead<PfTags>> = PfGlobal(TailqHead::new());
/// `pf_qids`: the queue names in use, sorted by id.
pub static PF_QIDS: PfGlobal<TailqHead<PfTags>> = PfGlobal(TailqHead::new());

/// `pf_lock`: protects the consistency of the pf data structures that do not have a lock of
/// their own yet: the rules, the radix tables and the source nodes. Every caller takes it
/// exclusively.
pub static PF_LOCK: Rwlock = Rwlock::new("pf_lock");
/// `pf_state_lock`: protects the state table. Packets that look states up take it for
/// reading; a packet that creates a state takes `pf_lock` first and then this lock for
/// writing.
pub static PF_STATE_LOCK: Rwlock = Rwlock::new("pf_state_lock");
/// `pfioctl_rw`: serialises the ioctls and the transactions list.
pub static PFIOCTL_RW: Rwlock = Rwlock::new("pfioctl_rw");

/// `pf_ioctl_trans`: the open ioctl transactions (`DIOCGETRULES` walks).
pub static PF_IOCTL_TRANS: PfGlobal<ListHead<PfTransList>> = PfGlobal(ListHead::new());

/// `pf_statelim_commit`'s local list `l` (the old active state limiters), a static so that
/// its elements are `'static`; only `pf_statelim_commit` uses it, under `pf_lock`, and leaves
/// it empty.
static PF_STATELIM_COMMIT_L: PfGlobal<TailqHead<PfStatelimList>> = PfGlobal(TailqHead::new());
/// `pf_sourcelim_commit`'s local list `l`, as `PF_STATELIM_COMMIT_L`.
static PF_SOURCELIM_COMMIT_L: PfGlobal<TailqHead<PfSourcelimList>> = PfGlobal(TailqHead::new());

/// `pf_tcount`: the transactions each clone of the device has open.
static PF_TCOUNT: PfGlobal<[Cell<u32>; CLONE_MAPSZ * NBBY]> =
    PfGlobal([const { Cell::new(0) }; CLONE_MAPSZ * NBBY]);

/// `pf_unit2idx(unit)`: the clone index of a minor number.
const fn pf_unit2idx(unit: u32) -> usize {
    (unit >> CLONE_SHIFT) as usize
}

/// The static image of `pf_default_rule`: zero, but for the plain members `pfattach` sets
/// (`action`, `rtableid`), which a static cannot change later.
const fn pf_default_rule_init() -> PfRule {
    let mut r = PfRule::zeroed();
    r.action = PF_PASS;
    r.rtableid = -1;
    r
}

/// `*dst = *src` for the default rules: copies the members that are `Cell`s (the plain ones
/// are the constants of `pf_default_rule_init` in both).
fn pf_default_rule_copy(dst: &PfRule, src: &PfRule) {
    for (d, s) in dst.timeout.iter().zip(src.timeout.iter()) {
        d.set(s.get());
    }
    dst.nr.set(src.nr.get());
    dst.evaluations.set(src.evaluations.get());
    for i in 0..2 {
        dst.packets[i].set(src.packets[i].get());
        dst.bytes[i].set(src.bytes[i].get());
    }
    dst.states_cur.set(src.states_cur.get());
    dst.states_tot.set(src.states_tot.get());
    dst.src_nodes.set(src.src_nodes.get());
    dst.rule_flag.set(src.rule_flag.get());
    dst.exptime.set(src.exptime.get());
    dst.src.addr.type_.set(src.src.addr.type_.get());
    dst.dst.addr.type_.set(src.dst.addr.type_.get());
    dst.rdr.addr.type_.set(src.rdr.addr.type_.get());
    dst.nat.addr.type_.set(src.nat.addr.type_.get());
    dst.route.addr.type_.set(src.route.addr.type_.get());
}

/// `strnlen(s, sizeof(s)) >= sizeof(s)`: the array holds no NUL.
fn pf_name_too_long(s: &[u8]) -> bool {
    libkern::strnlen(s, s.len()) >= s.len()
}

/// `pfattach`: sets pf up at boot (`pseudo-device pf`): the pools, the tables, the
/// interfaces, the fingerprints, the syncookies, the main ruleset, the queues, the default
/// rule and its timeouts, the status and the anchor stack.
pub fn pfattach(_num: i32) {
    pf_pool_init::<PfRule>(&PF_RULE_PL, IPL_SOFTNET, 0, "pfrule");
    pf_pool_init::<PfSrcNode>(&PF_SRC_TREE_PL, IPL_SOFTNET, 0, "pfsrctr");
    pf_pool_init::<PfSnItem>(&PF_SN_ITEM_PL, IPL_SOFTNET, 0, "pfsnitem");
    pool_init(
        &PF_STATE_PL,
        size_of::<PfState>(),
        CACHELINESIZE,
        IPL_SOFTNET,
        0,
        "pfstate",
        None,
    );
    pool_init(
        &PF_STATE_KEY_PL,
        size_of::<PfStateKey>(),
        CACHELINESIZE,
        IPL_SOFTNET,
        0,
        "pfstkey",
        None,
    );
    pf_pool_init::<PfStateItem>(&PF_STATE_ITEM_PL, IPL_SOFTNET, 0, "pfstitem");
    pf_pool_init::<PfRuleItem>(&PF_RULE_ITEM_PL, IPL_SOFTNET, 0, "pfruleitem");
    pf_pool_init::<PfQueuespec>(&PF_QUEUE_PL, IPL_SOFTNET, 0, "pfqueue");
    pf_pool_init::<PfTagname>(&PF_TAG_PL, IPL_SOFTNET, 0, "pftag");
    pf_pool_init::<PfPktdelay>(&PF_PKTDELAY_PL, IPL_SOFTNET, 0, "pfpktdelay");
    pool_init(
        &PF_ANCHOR_PL,
        size_of::<PfAnchor>(),
        core::mem::align_of::<PfAnchor>() as u32,
        IPL_SOFTNET,
        0,
        "pfanchor",
        None,
    );

    pf_pool_init::<PfStatelim>(&PF_STATELIM_PL, IPL_SOFTNET, 0, "pfstlim");
    pf_pool_init::<PfSourcelim>(&PF_SOURCELIM_PL, IPL_SOFTNET, 0, "pfsrclim");
    pf_pool_init::<PfSource>(&PF_SOURCE_PL, IPL_SOFTNET, 0, "pfsrc");
    pf_pool_init::<PfStateLink>(&PF_STATE_LINK_PL, IPL_SOFTNET, 0, "pfslink");

    hfsc_initialize();
    pfr_initialize();
    pfi_initialize();
    pf_osfp_initialize();
    pf_syncookies_init();

    for i in [PF_LIMIT_STATES, PF_LIMIT_ANCHORS] {
        if let Some(pp) = PF_POOL_LIMITS[i].pp {
            // The C ignores the result: the pools are empty.
            let _ = pool_sethardlimit(pp, PF_POOL_LIMITS[i].limit.get());
        }
    }

    if PHYSMEM.load(AtomicOrdering::Relaxed) <= atop(100 * 1024 * 1024) {
        PF_POOL_LIMITS[PF_LIMIT_TABLE_ENTRIES]
            .limit
            .set(PFR_KENTRY_HIWAT_SMALL);
    }

    TREE_SRC_TRACKING.init();
    PF_ANCHORS.init();
    pf_init_ruleset(pf_main_ruleset());
    PF_QUEUES[0].init();
    PF_QUEUES[1].init();
    PF_QUEUES_ACTIVE.set(0);
    PF_QUEUES_INACTIVE.set(1);

    // default rule should never be garbage collected
    // SAFETY: the default rule is in no queue and never goes into one.
    unsafe { PF_DEFAULT_RULE.entries.set_prev_self() };
    // `action` (PF_PASS) and `rtableid` (-1) are set by the static's initialiser.
    PF_DEFAULT_RULE.nr.set(u32::MAX);

    // initialize default timeouts
    let timeout = &PF_DEFAULT_RULE.timeout;
    timeout[PFTM_TCP_FIRST_PACKET].set(PFTM_TCP_FIRST_PACKET_VAL);
    timeout[PFTM_TCP_OPENING].set(PFTM_TCP_OPENING_VAL);
    timeout[PFTM_TCP_ESTABLISHED].set(PFTM_TCP_ESTABLISHED_VAL);
    timeout[PFTM_TCP_CLOSING].set(PFTM_TCP_CLOSING_VAL);
    timeout[PFTM_TCP_FIN_WAIT].set(PFTM_TCP_FIN_WAIT_VAL);
    timeout[PFTM_TCP_CLOSED].set(PFTM_TCP_CLOSED_VAL);
    timeout[PFTM_UDP_FIRST_PACKET].set(PFTM_UDP_FIRST_PACKET_VAL);
    timeout[PFTM_UDP_SINGLE].set(PFTM_UDP_SINGLE_VAL);
    timeout[PFTM_UDP_MULTIPLE].set(PFTM_UDP_MULTIPLE_VAL);
    timeout[PFTM_ICMP_FIRST_PACKET].set(PFTM_ICMP_FIRST_PACKET_VAL);
    timeout[PFTM_ICMP_ERROR_REPLY].set(PFTM_ICMP_ERROR_REPLY_VAL);
    timeout[PFTM_OTHER_FIRST_PACKET].set(PFTM_OTHER_FIRST_PACKET_VAL);
    timeout[PFTM_OTHER_SINGLE].set(PFTM_OTHER_SINGLE_VAL);
    timeout[PFTM_OTHER_MULTIPLE].set(PFTM_OTHER_MULTIPLE_VAL);
    timeout[PFTM_FRAG].set(PFTM_FRAG_VAL);
    timeout[PFTM_INTERVAL].set(PFTM_INTERVAL_VAL);
    timeout[PFTM_SRC_NODE].set(PFTM_SRC_NODE_VAL);
    timeout[PFTM_TS_DIFF].set(PFTM_TS_DIFF_VAL);
    timeout[PFTM_ADAPTIVE_START].set(PFSTATE_ADAPT_START);
    timeout[PFTM_ADAPTIVE_END].set(PFSTATE_ADAPT_END);

    PF_DEFAULT_RULE.src.addr.type_.set(PF_ADDR_ADDRMASK);
    PF_DEFAULT_RULE.dst.addr.type_.set(PF_ADDR_ADDRMASK);
    PF_DEFAULT_RULE.rdr.addr.type_.set(PF_ADDR_NONE);
    PF_DEFAULT_RULE.nat.addr.type_.set(PF_ADDR_NONE);
    PF_DEFAULT_RULE.route.addr.type_.set(PF_ADDR_NONE);

    pf_normalize_init();
    pf_status_init();

    pf_default_rule_copy(&PF_DEFAULT_RULE_NEW, &PF_DEFAULT_RULE);

    // We waste two stack frames as meta-data: frame[0] always presents a top, which can not
    // be used for data; frame[PF_ANCHOR_STACK_MAX] denotes the bottom of the stack and keeps
    // the pointer to the currently used stack frame.
    pf_anchor_stack_init();
}

/// `pfopen`: only the clone device's first minor of each clone may be opened.
pub fn pfopen(dev: Dev, _flags: i32, _fmt: i32, _p: &Proc) -> Result<(), Errno> {
    let unit = minor(dev);

    if unit & ((1 << CLONE_SHIFT) - 1) != 0 {
        return Err(Errno::ENXIO);
    }

    Ok(())
}

/// `pfclose`: frees the transactions the closing descriptor left open.
pub fn pfclose(dev: Dev, _flags: i32, _fmt: i32, _p: Option<&Proc>) -> Result<(), Errno> {
    let mut tmp_list: Vec<&'static PfTrans> = Vec::new();
    let unit = minor(dev);

    rw_enter_write(&PFIOCTL_RW);
    for w in PF_IOCTL_TRANS.iter() {
        if w.pft_unit.get() == unit {
            // SAFETY: `w` is on `pf_ioctl_trans` (we are iterating it under `pfioctl_rw`).
            unsafe { ListHead::<PfTransList>::remove(w) };
            tmp_list.push(w);
        }
    }
    rw_exit_write(&PFIOCTL_RW);

    // The C moved them to the head of its local list: the last found first.
    while let Some(w) = tmp_list.pop() {
        pf_free_trans(w);
    }

    Ok(())
}

/// `pf_rule_free`: frees a rule that was never put in a ruleset (`pf_rule_copyin`'s
/// interfaces and the rule itself).
pub fn pf_rule_free(rule: Option<&'static PfRule>) {
    let Some(rule) = rule else {
        return;
    };

    pfi_kif_free(rule.kif());
    pfi_kif_free(rule.rcv_kif());
    pfi_kif_free(rule.rdr.kif());
    pfi_kif_free(rule.nat.kif());
    pfi_kif_free(rule.route.kif());

    pf_pool_put(&PF_RULE_PL, rule);
}

/// `pf_rm_rule`: takes `rule` out of `rulequeue` (if any) and frees it once no state and no
/// source node uses it.
pub fn pf_rm_rule(rulequeue: Option<&'static TailqHead<PfRulequeue>>, rule: &'static PfRule) {
    if let Some(rulequeue) = rulequeue {
        if rule.states_cur.get() == 0 && rule.src_nodes.get() == 0 {
            // XXX - we need to remove the table *before* detaching the rule to make sure the
            // table code does not delete the anchor under our feet.
            pf_tbladdr_remove(&rule.src.addr);
            pf_tbladdr_remove(&rule.dst.addr);
            pf_tbladdr_remove(&rule.rdr.addr);
            pf_tbladdr_remove(&rule.nat.addr);
            pf_tbladdr_remove(&rule.route.addr);
            if let Some(t) = rule.overload_tbl() {
                pfr_detach_table(t);
            }
        }
        // SAFETY: the caller's rule is on `rulequeue`; once removed it is in no queue.
        unsafe {
            rulequeue.remove(rule);
            rule.entries.clear_prev();
        }
        rule.nr.set(u32::MAX);
    }

    if rule.states_cur.get() > 0 || rule.src_nodes.get() > 0 || rule.entries.is_linked() {
        return;
    }
    pf_tag_unref(rule.tag);
    pf_tag_unref(rule.match_tag);
    pf_rtlabel_remove(&rule.src.addr);
    pf_rtlabel_remove(&rule.dst.addr);
    pfi_dynaddr_remove(&rule.src.addr);
    pfi_dynaddr_remove(&rule.dst.addr);
    pfi_dynaddr_remove(&rule.rdr.addr);
    pfi_dynaddr_remove(&rule.nat.addr);
    pfi_dynaddr_remove(&rule.route.addr);
    if rulequeue.is_none() {
        pf_tbladdr_remove(&rule.src.addr);
        pf_tbladdr_remove(&rule.dst.addr);
        pf_tbladdr_remove(&rule.rdr.addr);
        pf_tbladdr_remove(&rule.nat.addr);
        pf_tbladdr_remove(&rule.route.addr);
        if let Some(t) = rule.overload_tbl() {
            pfr_detach_table(t);
        }
    }
    pfi_kif_unref(rule.rcv_kif(), PFI_KIF_REF_RULE);
    pfi_kif_unref(rule.kif(), PFI_KIF_REF_RULE);
    pfi_kif_unref(rule.rdr.kif(), PFI_KIF_REF_RULE);
    pfi_kif_unref(rule.nat.kif(), PFI_KIF_REF_RULE);
    pfi_kif_unref(rule.route.kif(), PFI_KIF_REF_RULE);
    pf_remove_anchor(rule);
    pf_pool_put(&PF_RULE_PL, rule);
}

/// `tagname2tag`: the number of tag `tagname` in `head`, referenced once more; with `create`
/// a new number (the lowest free one) for a new name. 0 when there is none.
pub fn tagname2tag(head: &TailqHead<PfTags>, tagname: &[u8], create: bool) -> u16 {
    let name = pf_cstr(tagname);

    for tag in head.iter() {
        if pf_cstr(&tag.name) == name {
            tag.ref_.set(tag.ref_.get() + 1);
            return tag.tag.get();
        }
    }

    if !create {
        return 0;
    }

    // To avoid fragmentation, we do a linear search from the beginning and take the first
    // free slot we find. If there is none or the list is empty, append a new entry at the
    // end.

    // new entry
    let mut new_tagid: u32 = 1;
    let mut p = None;
    for t in head.iter() {
        if u32::from(t.tag.get()) != new_tagid {
            p = Some(t);
            break;
        }
        new_tagid = u32::from(t.tag.get()) + 1;
    }

    if new_tagid > TAGID_MAX {
        return 0;
    }

    // allocate and fill new struct pf_tagname
    let Some(mut tag) = PfPoolNew::<PfTagname>::get(&PF_TAG_PL, PR_NOWAIT) else {
        return 0;
    };
    pf_strlcpy(&mut tag.get_mut().name, tagname);
    let tag = tag.publish();
    tag.tag.set(new_tagid as u16);
    tag.ref_.set(tag.ref_.get() + 1);

    match p {
        // insert new entry before p
        // SAFETY: `p` is on `head`; the new tag is in no list and lives until `tag_unref`
        // unlinks it.
        Some(p) => unsafe { TailqHead::<PfTags>::insert_before(p, tag) },
        // either list empty or no free slot in between
        // SAFETY: as above; `head` is a static.
        None => unsafe { head.insert_tail(tag) },
    }

    tag.tag.get()
}

/// `tag2tagname`: copies the name of tag `tagid` in `head` into `p` (unchanged when there is
/// no such tag).
pub fn tag2tagname(head: &TailqHead<PfTags>, tagid: u16, p: &mut [u8]) {
    for tag in head.iter() {
        if tag.tag.get() == tagid {
            let n = p.len().min(PF_TAG_NAME_SIZE);
            pf_strlcpy(&mut p[..n], &tag.name);
            return;
        }
    }
}

/// `tag_unref`: drops a reference on tag `tag` in `head`, freeing the name with the last.
pub fn tag_unref(head: &TailqHead<PfTags>, tag: u16) {
    if tag == 0 {
        return;
    }

    for p in head.iter() {
        if tag == p.tag.get() {
            p.ref_.set(p.ref_.get() - 1);
            if p.ref_.get() == 0 {
                // SAFETY: `p` is on `head`; nothing else points at a tag name.
                unsafe { head.remove(p) };
                pf_pool_put(&PF_TAG_PL, p);
            }
            break;
        }
    }
}

/// `pf_tagname2tag`.
pub fn pf_tagname2tag(tagname: &[u8], create: bool) -> u16 {
    tagname2tag(&PF_TAGS, tagname, create)
}

/// `pf_tag2tagname`.
pub fn pf_tag2tagname(tagid: u16, p: &mut [u8]) {
    tag2tagname(&PF_TAGS, tagid, p);
}

/// `pf_tag_ref`: one more reference on tag `tag`.
pub fn pf_tag_ref(tag: u16) {
    if let Some(t) = PF_TAGS.iter().find(|t| t.tag.get() == tag) {
        t.ref_.set(t.ref_.get() + 1);
    }
}

/// `pf_tag_unref`.
pub fn pf_tag_unref(tag: u16) {
    tag_unref(&PF_TAGS, tag);
}

/// `pf_rtlabel_add`: resolves a `PF_ADDR_RTLABEL` operand's name to its label id (taking a
/// reference). `false` (the C's -1) when the label cannot be registered.
pub fn pf_rtlabel_add(a: &PfAddrWrap) -> bool {
    if a.type_.get() == PF_ADDR_RTLABEL {
        let mut v = a.v.get();
        let id = rtlabel_name2id(v.rtlabelname());
        v.set_rtlabel(u32::from(id));
        a.v.set(v);
        if id == 0 {
            return false;
        }
    }
    true
}

/// `pf_rtlabel_remove`: drops a `PF_ADDR_RTLABEL` operand's label reference.
pub fn pf_rtlabel_remove(a: &PfAddrWrap) {
    if a.type_.get() == PF_ADDR_RTLABEL {
        rtlabel_unref(a.v.get().rtlabel() as u16);
    }
}

/// `pf_rtlabel_copyout`: puts the label's name back into a copy of an operand for user
/// space ("?" when the label is gone).
pub fn pf_rtlabel_copyout(a: &PfAddrWrap) {
    let mut v = a.v.get();
    if a.type_.get() == PF_ADDR_RTLABEL && v.rtlabel() != 0 {
        let id = v.rtlabel() as u16;
        if rtlabel_id2name(id, v.rtlabelname_mut()).is_none() {
            pf_strlcpy(v.rtlabelname_mut(), b"?");
        }
        a.v.set(v);
    }
}

/// `pf_qname2qid`.
pub fn pf_qname2qid(qname: &[u8], create: bool) -> u16 {
    tagname2tag(&PF_QIDS, qname, create)
}

/// `pf_qid2qname`.
pub fn pf_qid2qname(qid: u16, p: &mut [u8]) {
    tag2tagname(&PF_QIDS, qid, p);
}

/// `pf_qid_unref`.
pub fn pf_qid_unref(qid: u16) {
    tag_unref(&PF_QIDS, qid);
}

/// `pf_begin_rules`: opens a transaction on the inactive rules of ruleset `anchor` (created
/// if needed), emptying them; the new version is the ticket.
pub fn pf_begin_rules(version: &mut u32, anchor: &[u8]) -> Result<(), Errno> {
    let Some(rs) = pf_find_or_create_ruleset(anchor) else {
        return Err(Errno::EINVAL);
    };
    while let Some(rule) = rs.inactive_ptr().first() {
        pf_rm_rule(Some(rs.inactive_ptr()), rule);
        rs.inactive.rcount.set(rs.inactive.rcount.get() - 1);
    }
    rs.inactive
        .version
        .set(rs.inactive.version.get().wrapping_add(1));
    *version = rs.inactive.version.get();
    rs.inactive.open.set(1);
    Ok(())
}

/// `pf_rollback_rules`: abandons the transaction `version` on ruleset `anchor`.
pub fn pf_rollback_rules(version: u32, anchor: &[u8]) {
    let rs = pf_find_ruleset(anchor);
    let Some(rs) =
        rs.filter(|rs| rs.inactive.open.get() != 0 && rs.inactive.version.get() == version)
    else {
        return;
    };
    while let Some(rule) = rs.inactive_ptr().first() {
        pf_rm_rule(Some(rs.inactive_ptr()), rule);
        rs.inactive.rcount.set(rs.inactive.rcount.get() - 1);
    }
    rs.inactive.open.set(0);

    // queue defs only in the main ruleset
    if anchor.first().is_some_and(|&c| c != 0) {
        return;
    }

    pf_statelim_rollback();
    pf_sourcelim_rollback();
    pf_free_queues(pf_queues_inactive());
}

/// `pf_free_queues`: frees every queue of `where_`.
pub fn pf_free_queues(where_: &'static TailqHead<PfQueuehead>) {
    for q in where_.iter() {
        // SAFETY: `q` is on `where_`; it is freed right after.
        unsafe { where_.remove(q) };
        pfi_kif_unref(q.kif(), PFI_KIF_REF_RULE);
        pf_pool_put(&PF_QUEUE_PL, q);
    }
}

/// `pf_remove_queues`: puts the interfaces of the active root queues back in normal
/// queueing mode.
pub fn pf_remove_queues() {
    // put back interfaces in normal queueing mode
    for q in pf_queues_active().iter() {
        if q.parent_qid != 0 {
            continue;
        }

        let Some(ifp) = q.kif().and_then(|k| k.pfik_ifp()) else {
            continue;
        };

        ifq_attach(&ifp.if_snd, IFQ_PRIQ_OPS, ptr::null_mut());
    }
}

/// `pf_ifp2q`: the entry of `ifp` in `list` (the latest added first, as the C's list).
fn pf_ifp2q<'a>(list: &'a [PfQueueIf], ifp: &Ifnet) -> Option<&'a PfQueueIf> {
    list.iter().rev().find(|qif| ptr::eq(qif.ifp, ifp))
}

/// `pf_create_queues`: builds the disciplines of the active queues and attaches them to
/// their interfaces; the interfaces that lost their root queue go back to normal queueing.
pub fn pf_create_queues() -> Result<(), Errno> {
    let mut list: Vec<PfQueueIf> = Vec::new();

    // Find root queues and allocate traffic conditioner private data for these interfaces.
    for q in pf_queues_active().iter() {
        if q.parent_qid != 0 {
            continue;
        }

        let Some(ifp) = q.kif().and_then(|k| k.pfik_ifp()) else {
            continue;
        };

        let (ifqops, pfqops) = if q.flags & PFQS_ROOTCLASS != 0 {
            (IFQ_HFSC_OPS, PFQ_HFSC_OPS)
        } else {
            (IFQ_FQCODEL_OPS, PFQ_FQCODEL_OPS)
        };

        let disc = (pfqops.pfq_alloc)(ifp);

        list.push(PfQueueIf {
            ifp,
            ifqops,
            pfqops,
            disc,
        });
    }

    // and now everything
    let mut error = Ok(());
    for q in pf_queues_active().iter() {
        let Some(ifp) = q.kif().and_then(|k| k.pfik_ifp()) else {
            continue;
        };

        let qif = pf_ifp2q(&list, ifp);
        kassert!(qif.is_some());
        let Some(qif) = qif else {
            continue;
        };

        error = (qif.pfqops.pfq_addqueue)(qif.disc, q);
        if error.is_err() {
            break;
        }
    }

    if let Err(e) = error {
        while let Some(qif) = list.pop() {
            (qif.pfqops.pfq_free)(qif.disc);
        }
        return Err(e);
    }

    // find root queues in old list to disable them if necessary
    for q in pf_queues_inactive().iter() {
        if q.parent_qid != 0 {
            continue;
        }

        let Some(ifp) = q.kif().and_then(|k| k.pfik_ifp()) else {
            continue;
        };

        if pf_ifp2q(&list, ifp).is_some() {
            continue;
        }

        ifq_attach(&ifp.if_snd, IFQ_PRIQ_OPS, ptr::null_mut());
    }

    // commit the new queues
    while let Some(qif) = list.pop() {
        ifq_attach(&qif.ifp.if_snd, qif.ifqops, qif.disc);
    }

    Ok(())
}

/// `pf_commit_queues`: makes the inactive queues the active ones.
pub fn pf_commit_queues() -> Result<(), Errno> {
    // swap
    let qswap = PF_QUEUES_ACTIVE.get();
    PF_QUEUES_ACTIVE.set(PF_QUEUES_INACTIVE.get());
    PF_QUEUES_INACTIVE.set(qswap);

    if let Err(e) = pf_create_queues() {
        PF_QUEUES_INACTIVE.set(PF_QUEUES_ACTIVE.get());
        PF_QUEUES_ACTIVE.set(qswap);
        return Err(e);
    }

    pf_free_queues(pf_queues_inactive());

    Ok(())
}

/// `pf_queue_manager`: the discipline of a queue that is not an HFSC class (FQ-CoDel for a
/// flow queue, else none: `pfq_default_ops`).
pub fn pf_queue_manager(q: &PfQueuespec) -> Option<&'static PfqOps> {
    if q.flags & PFQS_FLOWQUEUE != 0 {
        return Some(PFQ_FQCODEL_OPS);
    }
    None // pfq_default_ops
}

/// `pf_hash_rule_addr`: hashes the members of an operand that identify it.
pub fn pf_hash_rule_addr(ctx: &mut Md5Ctx, pfr: &PfRuleAddr) {
    let v = pfr.addr.v.get();
    MD5Update(ctx, &[pfr.addr.type_.get()]);
    match pfr.addr.type_.get() {
        PF_ADDR_DYNIFTL => {
            MD5Update(ctx, v.ifname());
            MD5Update(ctx, &[pfr.addr.iflags.get()]);
        }
        PF_ADDR_TABLE => {
            if !v.tblname().starts_with(PF_OPTIMIZER_TABLE_PFX) {
                MD5Update(ctx, v.tblname());
            }
        }
        PF_ADDR_ADDRMASK => {
            // XXX ignore af?
            MD5Update(ctx, &v.addr().addr8);
            MD5Update(ctx, &v.mask().addr8);
        }
        PF_ADDR_RTLABEL => MD5Update(ctx, v.rtlabelname()),
        _ => {}
    }

    MD5Update(ctx, &pfr.port[0].get().to_ne_bytes());
    MD5Update(ctx, &pfr.port[1].get().to_ne_bytes());
    MD5Update(ctx, &[pfr.neg.get()]);
    MD5Update(ctx, &[pfr.port_op.get()]);
}

/// `pf_hash_rule`: hashes the members of a rule that identify it (`PF_MD5_UPD`: the bytes as
/// stored; `PF_MD5_UPD_STR`: the string; `PF_MD5_UPD_HTONL`/`HTONS`: network order).
pub fn pf_hash_rule(ctx: &mut Md5Ctx, rule: &PfRule) {
    pf_hash_rule_addr(ctx, &rule.src);
    pf_hash_rule_addr(ctx, &rule.dst);
    MD5Update(ctx, pf_cstr(&rule.label));
    MD5Update(ctx, pf_cstr(&rule.ifname));
    MD5Update(ctx, pf_cstr(&rule.rcv_ifname));
    MD5Update(ctx, pf_cstr(&rule.match_tagname));
    MD5Update(ctx, &rule.match_tag.to_be_bytes()); // dup?
    MD5Update(ctx, &rule.os_fingerprint.to_be_bytes());
    MD5Update(ctx, &rule.prob.to_be_bytes());
    MD5Update(ctx, &rule.uid.uid[0].to_be_bytes());
    MD5Update(ctx, &rule.uid.uid[1].to_be_bytes());
    MD5Update(ctx, &[rule.uid.op]);
    MD5Update(ctx, &rule.gid.gid[0].to_be_bytes());
    MD5Update(ctx, &rule.gid.gid[1].to_be_bytes());
    MD5Update(ctx, &[rule.gid.op]);
    MD5Update(ctx, &rule.rule_flag.get().to_be_bytes());
    MD5Update(ctx, &[rule.action]);
    MD5Update(ctx, &[rule.direction]);
    MD5Update(ctx, &[rule.af]);
    MD5Update(ctx, &[rule.quick]);
    MD5Update(ctx, &[rule.ifnot]);
    MD5Update(ctx, &[rule.rcvifnot]);
    MD5Update(ctx, &[rule.match_tag_not]);
    MD5Update(ctx, &[rule.keep_state]);
    MD5Update(ctx, &[rule.proto]);
    MD5Update(ctx, &rule.type_.to_ne_bytes());
    MD5Update(ctx, &rule.code.to_ne_bytes());
    MD5Update(ctx, &[rule.flags]);
    MD5Update(ctx, &[rule.flagset]);
    MD5Update(ctx, &[rule.allow_opts]);
    MD5Update(ctx, &[rule.rt]);
    MD5Update(ctx, &[rule.tos]);
}

/// `pf_commit_rules`: makes the inactive rules of ruleset `anchor` (transaction `version`)
/// the active ones and frees the old; for the main ruleset, also the limiters and queues.
pub fn pf_commit_rules(version: u32, anchor: &[u8]) -> Result<(), Errno> {
    pf_assert_locked();

    let rs = pf_find_ruleset(anchor);
    let Some(rs) =
        rs.filter(|rs| rs.inactive.open.get() != 0 && version == rs.inactive.version.get())
    else {
        return Err(Errno::EBUSY);
    };

    if ptr::eq(rs, pf_main_ruleset()) {
        pf_sourcelim_check()?;
        pf_calc_chksum(rs);
    }

    // Swap rules, keep the old.
    let old_rules = rs.active.ptr.get();
    let old_rcount = rs.active.rcount.get();

    rs.active.ptr.set(rs.inactive.ptr.get());
    rs.active.rcount.set(rs.inactive.rcount.get());
    rs.inactive.ptr.set(old_rules);
    rs.inactive.rcount.set(old_rcount);

    rs.active.version.set(rs.inactive.version.get());
    pf_calc_skip_steps(rs.active_ptr());

    // Purge the old rule list.
    let old_rules = &rs.queues[old_rules];
    while let Some(rule) = old_rules.first() {
        pf_rm_rule(Some(old_rules), rule);
    }
    rs.inactive.rcount.set(0);
    rs.inactive.open.set(0);
    pf_remove_if_empty_ruleset(rs);

    // statelim/sourcelim/queue defs only in the main ruleset
    if anchor.first().is_some_and(|&c| c != 0) {
        return Ok(());
    }

    pf_statelim_commit();
    pf_sourcelim_commit();

    pf_commit_queues()
}

/// `pf_calc_chksum`: the MD5 of the inactive rules of `rs`, stored as `pf_status.pf_chksum`.
pub fn pf_calc_chksum(rs: &PfRuleset) {
    let mut ctx = Md5Ctx::default();
    let mut digest = [0u8; PF_MD5_DIGEST_LENGTH];

    MD5Init(&mut ctx);

    if rs.inactive.rcount.get() != 0 {
        for rule in rs.inactive_ptr().iter() {
            pf_hash_rule(&mut ctx, rule);
        }
    }

    MD5Final(&mut digest, &mut ctx);
    PF_STATUS.pf_chksum.set(digest);
}

/// `pf_addr_setup`: resolves an operand of a new rule: its interface's addresses, its
/// table, its route label.
pub fn pf_addr_setup(
    ruleset: &'static PfRuleset,
    addr: &'static PfAddrWrap,
    af: SaFamily,
) -> Result<(), Errno> {
    if !pfi_dynaddr_setup(addr, af, PR_WAITOK)
        || !pf_tbladdr_setup(ruleset, addr, PR_WAITOK)
        || !pf_rtlabel_add(addr)
    {
        return Err(Errno::EINVAL);
    }

    Ok(())
}

/// `pf_kif_setup`: trades the interface `pf_rule_copyin` allocated for the one of that name
/// (the allocation becomes it when it is new), with a rule reference.
pub fn pf_kif_setup(kif_buf: Option<&'static PfiKif>) -> Option<&'static PfiKif> {
    let buf = kif_buf?;
    let mut kif_buf = Some(buf);

    kassert!(buf.pfik_name[0] != 0);

    let kif = pfi_kif_get(&buf.pfik_name, Some(&mut kif_buf));
    if kif_buf.is_some() {
        pfi_kif_free(kif_buf);
    }
    if let Some(k) = kif {
        pfi_kif_ref(k, PFI_KIF_REF_RULE);
    }

    kif
}

/// `pf_addr_copyout`: turns the kernel pointers of a copy of an operand for user space into
/// what user space reads (address counts, the route label's name).
///
/// # Safety
///
/// `addr` is a kernel operand or a byte copy of one made under `pf_lock`, which is still
/// held: its `p` is null or points at the live dynamic address or table the kernel operand
/// has (`pfi_dynaddr_copyout`, `pf_tbladdr_copyout`).
pub unsafe fn pf_addr_copyout(addr: &PfAddrWrap) {
    // SAFETY: the caller's contract is the callees'.
    unsafe {
        pfi_dynaddr_copyout(addr);
        pf_tbladdr_copyout(addr);
    }
    pf_rtlabel_copyout(addr);
}

/// `pf_statelim_add`: `DIOCADDSTATELIM`, a state limiter for the open main ruleset
/// transaction.
pub fn pf_statelim_add(ioc: &PfiocStatelim) -> Result<(), Errno> {
    if pf_name_too_long(&ioc.name) {
        return Err(Errno::ENAMETOOLONG);
    }

    if ioc.id < PF_STATELIM_ID_MIN || ioc.id > PF_STATELIM_ID_MAX {
        return Err(Errno::EINVAL);
    }

    if ioc.limit < PF_STATELIM_LIMIT_MIN || ioc.limit > PF_STATELIM_LIMIT_MAX {
        return Err(Errno::EINVAL);
    }

    if (ioc.rate.limit == 0) != (ioc.rate.seconds == 0) {
        return Err(Errno::EINVAL);
    }

    let Some(pfstlim) = pf_pool_get::<PfStatelim>(&PF_STATELIM_PL, PR_WAITOK) else {
        return Err(Errno::ENOMEM);
    };

    pfstlim.pfstlim_id.set(ioc.id);
    let mut nm = [0u8; PF_STATELIM_NAME_LEN];
    pf_strlcpy(&mut nm, &ioc.name);
    pfstlim.pfstlim_nm.set(nm);
    pfstlim.pfstlim_limit.set(ioc.limit);
    pfstlim.pfstlim_rate.limit.set(ioc.rate.limit);
    pfstlim.pfstlim_rate.seconds.set(ioc.rate.seconds);

    if pfstlim.pfstlim_rate.limit.get() != 0 {
        let bucket = sec_to_nsec(u64::from(pfstlim.pfstlim_rate.seconds.get()));

        pfstlim
            .pfstlim_rate_ts
            .set(getnsecuptime().wrapping_sub(bucket));
        pfstlim
            .pfstlim_rate_token
            .set(bucket / u64::from(pfstlim.pfstlim_rate.limit.get()));
        pfstlim.pfstlim_rate_bucket.set(bucket);
    }

    pfstlim.pfstlim_states.init();
    pc_lock_init(&pfstlim.pfstlim_lock);

    net_lock();
    pf_lock();
    let error = 'unlock: {
        if ioc.ticket != pf_main_ruleset().inactive.version.get() {
            break 'unlock Errno::EBUSY;
        }

        // SAFETY: a fresh limiter, in no tree; the inactive trees and list are pf_lock's.
        if unsafe { PF_STATELIM_ID_TREE_INACTIVE.insert(pfstlim) }.is_some() {
            break 'unlock Errno::EBUSY;
        }

        // SAFETY: as above.
        if unsafe { PF_STATELIM_NM_TREE_INACTIVE.insert(pfstlim) }.is_some() {
            // SAFETY: inserted in the id tree just above.
            unsafe { PF_STATELIM_ID_TREE_INACTIVE.remove(pfstlim) };
            break 'unlock Errno::EBUSY;
        }

        // SAFETY: as above; the limiter is on no list.
        unsafe { PF_STATELIM_LIST_INACTIVE.insert_head(pfstlim) };

        pf_unlock();
        net_unlock();

        return Ok(());
    };

    pf_unlock();
    net_unlock();
    pf_pool_put(&PF_STATELIM_PL, pfstlim);

    Err(error)
}

/// `pf_statelim_unlink`: detaches the states of a limiter that goes away; their links go to
/// `garbage`.
fn pf_statelim_unlink(pfstlim: &'static PfStatelim, garbage: &TailqHead<PfStateLinkList>) {
    pf_state_enter_write();

    // unwire the links
    for pfl in pfstlim.pfstlim_states.iter() {
        if let Some(s) = pfl.pfl_state.get() {
            // if !rmst
            s.statelim.set(0);
            // SAFETY: a link on a limiter's list is on its state's linkage.
            unsafe { s.linkage.remove(pfl) };
        }
    }

    // take the list away
    // SAFETY: `garbage` is the caller's local, in place until it is emptied.
    unsafe { garbage.concat(&pfstlim.pfstlim_states) };
    pfstlim.pfstlim_inuse.set(0);

    pf_state_exit_write();
}

/// `pf_statelim_commit`: merges the new state limiters into the active set: the ones that
/// exist take the new configuration, the others are added, the ones not configured any more
/// are freed.
pub fn pf_statelim_commit() {
    let l: &'static TailqHead<PfStatelimList> = &PF_STATELIM_COMMIT_L;
    let garbage: TailqHead<PfStateLinkList> = TailqHead::new();

    pf_assert_locked();
    net_assert_locked("pf_statelim_commit");

    // merge the new statelims into the current set

    // start with an empty active list
    // SAFETY: `l` is a static, empty between calls.
    unsafe { l.concat(&PF_STATELIM_LIST_ACTIVE) };

    // beware, the inactive bits gets messed up here

    // try putting pending statelims into the active tree
    for mut pfstlim in PF_STATELIM_LIST_INACTIVE.iter() {
        // SAFETY: the limiter's id tree entry is the inactive tree's, which is abandoned and
        // re-initialised below without being followed again.
        if let Some(opfstlim) = unsafe { PF_STATELIM_ID_TREE_ACTIVE.insert(pfstlim) } {
            // this statelim already exists, merge
            opfstlim.pfstlim_limit.set(pfstlim.pfstlim_limit.get());
            opfstlim
                .pfstlim_rate
                .limit
                .set(pfstlim.pfstlim_rate.limit.get());
            opfstlim
                .pfstlim_rate
                .seconds
                .set(pfstlim.pfstlim_rate.seconds.get());

            opfstlim.pfstlim_rate_ts.set(pfstlim.pfstlim_rate_ts.get());
            opfstlim
                .pfstlim_rate_token
                .set(pfstlim.pfstlim_rate_token.get());
            opfstlim
                .pfstlim_rate_bucket
                .set(pfstlim.pfstlim_rate_bucket.get());

            opfstlim.pfstlim_nm.set(pfstlim.pfstlim_nm.get());

            // use the existing statelim instead
            pf_pool_put(&PF_STATELIM_PL, pfstlim);
            // SAFETY: an active limiter is on the old active list, now `l`.
            unsafe { l.remove(opfstlim) };
            pfstlim = opfstlim;
        }

        // SAFETY: the limiter is on no list that is followed again: it left the old active
        // list just above, or its inactive list link is abandoned (the iteration read the
        // next element already, and the list is re-initialised below).
        unsafe { PF_STATELIM_LIST_ACTIVE.insert_tail(pfstlim) };

        // NKSTAT: pf_statelim_kstat_attach(pfstlim); not configured.
    }

    // clean up the now unused statelims from the old set
    for pfstlim in l.iter() {
        pf_statelim_unlink(pfstlim, &garbage);

        // SAFETY: an old active limiter is in the active tree.
        unsafe { PF_STATELIM_ID_TREE_ACTIVE.remove(pfstlim) };

        // NKSTAT: pf_statelim_kstat_detach(pfstlim); not configured.
        pf_pool_put(&PF_STATELIM_PL, pfstlim);
    }

    // `l` was the C's local: forget the freed limiters.
    l.init();

    // fix up the inactive tree
    PF_STATELIM_ID_TREE_INACTIVE.init();
    PF_STATELIM_NM_TREE_INACTIVE.init();
    PF_STATELIM_LIST_INACTIVE.init();

    for pfl in garbage.iter() {
        pf_pool_put(&PF_STATE_LINK_PL, pfl);
    }
}

/// `pf_sourcelim_unlink`: frees the sources of a source limiter that goes away and detaches
/// their states; the links go to `garbage`.
fn pf_sourcelim_unlink(pfsrlim: &'static PfSourcelim, garbage: &TailqHead<PfStateLinkList>) {
    pf_state_enter_write();

    while let Some(pfsr) = pfsrlim.pfsrlim_sources.root() {
        // SAFETY: a source of the limiter is in both of its trees, and on `pf_source_gc` when
        // unused.
        unsafe {
            pfsrlim.pfsrlim_sources.remove(pfsr);
            pfsrlim.pfsrlim_ioc_sources.remove(pfsr);
            if pfsr.pfsr_inuse.get() == 0 {
                PF_SOURCE_GC.remove(pfsr);
            }
        }

        // unwire the links
        for pfl in pfsr.pfsr_states.iter() {
            if let Some(s) = pfl.pfl_state.get() {
                // if !rmst
                s.sourcelim.set(0);
                // SAFETY: a link on a source's list is on its state's linkage.
                unsafe { s.linkage.remove(pfl) };
            }
        }

        // take the list away
        // SAFETY: `garbage` is the caller's local, in place until it is emptied.
        unsafe { garbage.concat(&pfsr.pfsr_states) };

        pf_pool_put(&PF_SOURCE_PL, pfsr);
    }

    pf_state_exit_write();
}

/// `pf_sourcelim_check`: whether the new source limiters can be merged into the active ones
/// that track sources (same overload table and prefix lengths); `EBUSY` if not.
pub fn pf_sourcelim_check() -> Result<(), Errno> {
    pf_assert_locked();
    net_assert_locked("pf_sourcelim_check");

    // check if we can merge

    for pfsrlim in PF_SOURCELIM_LIST_INACTIVE.iter() {
        // new config, no conflict
        let Some(npfsrlim) = PF_SOURCELIM_ID_TREE_ACTIVE.find(pfsrlim) else {
            continue;
        };

        // nothing is tracked at the moment, no conflict
        if npfsrlim.pfsrlim_sources.is_empty() {
            continue;
        }

        if pf_cstr(&npfsrlim.pfsrlim_overload.name.get())
            != pf_cstr(&pfsrlim.pfsrlim_overload.name.get())
        {
            return Err(Errno::EBUSY);
        }

        // we should allow the prefixlens to get shorter and merge pf_source entries.

        if npfsrlim.pfsrlim_ipv4_prefix.get() != pfsrlim.pfsrlim_ipv4_prefix.get()
            || npfsrlim.pfsrlim_ipv6_prefix.get() != pfsrlim.pfsrlim_ipv6_prefix.get()
        {
            return Err(Errno::EBUSY);
        }
    }

    Ok(())
}

/// `pf_sourcelim_commit`: merges the new source limiters into the active set, as
/// `pf_statelim_commit` does for the state limiters.
pub fn pf_sourcelim_commit() {
    let l: &'static TailqHead<PfSourcelimList> = &PF_SOURCELIM_COMMIT_L;
    let garbage: TailqHead<PfStateLinkList> = TailqHead::new();

    pf_assert_locked();
    net_assert_locked("pf_sourcelim_commit");

    // merge the new sourcelims into the current set

    // start with an empty active list
    // SAFETY: `l` is a static, empty between calls.
    unsafe { l.concat(&PF_SOURCELIM_LIST_ACTIVE) };

    // beware, the inactive bits gets messed up here

    // try putting pending sourcelims into the active tree
    for mut pfsrlim in PF_SOURCELIM_LIST_INACTIVE.iter() {
        // SAFETY: as in `pf_statelim_commit`.
        if let Some(opfsrlim) = unsafe { PF_SOURCELIM_ID_TREE_ACTIVE.insert(pfsrlim) } {
            // this sourcelim already exists, merge
            opfsrlim.pfsrlim_entries.set(pfsrlim.pfsrlim_entries.get());
            opfsrlim.pfsrlim_limit.set(pfsrlim.pfsrlim_limit.get());
            opfsrlim
                .pfsrlim_ipv4_prefix
                .set(pfsrlim.pfsrlim_ipv4_prefix.get());
            opfsrlim
                .pfsrlim_ipv6_prefix
                .set(pfsrlim.pfsrlim_ipv6_prefix.get());
            opfsrlim
                .pfsrlim_rate
                .limit
                .set(pfsrlim.pfsrlim_rate.limit.get());
            opfsrlim
                .pfsrlim_rate
                .seconds
                .set(pfsrlim.pfsrlim_rate.seconds.get());

            opfsrlim
                .pfsrlim_ipv4_mask
                .set(pfsrlim.pfsrlim_ipv4_mask.get());
            opfsrlim
                .pfsrlim_ipv6_mask
                .set(pfsrlim.pfsrlim_ipv6_mask.get());

            // keep the existing pfstlim_rate_ts

            opfsrlim
                .pfsrlim_rate_token
                .set(pfsrlim.pfsrlim_rate_token.get());
            opfsrlim
                .pfsrlim_rate_bucket
                .set(pfsrlim.pfsrlim_rate_bucket.get());

            if let Some(t) = opfsrlim.pfsrlim_overload.table.get() {
                pfr_detach_table(t);
            }

            let mut name = [0u8; PF_TABLE_NAME_SIZE];
            pf_strlcpy(&mut name, &pfsrlim.pfsrlim_overload.name.get());
            opfsrlim.pfsrlim_overload.name.set(name);
            opfsrlim
                .pfsrlim_overload
                .hwm
                .set(pfsrlim.pfsrlim_overload.hwm.get());
            opfsrlim
                .pfsrlim_overload
                .lwm
                .set(pfsrlim.pfsrlim_overload.lwm.get());
            opfsrlim
                .pfsrlim_overload
                .table
                .set(pfsrlim.pfsrlim_overload.table.get());

            opfsrlim.pfsrlim_nm.set(pfsrlim.pfsrlim_nm.get());

            // use the existing sourcelim instead
            pf_pool_put(&PF_SOURCELIM_PL, pfsrlim);
            // SAFETY: an active limiter is on the old active list, now `l`.
            unsafe { l.remove(opfsrlim) };
            pfsrlim = opfsrlim;
        }

        // SAFETY: as in `pf_statelim_commit`.
        unsafe { PF_SOURCELIM_LIST_ACTIVE.insert_tail(pfsrlim) };

        // NKSTAT: pf_sourcelim_kstat_attach(pfsrlim); not configured.
    }

    // clean up the now unused sourcelims from the old set
    for pfsrlim in l.iter() {
        pf_sourcelim_unlink(pfsrlim, &garbage);

        // SAFETY: an old active limiter is in the active tree.
        unsafe { PF_SOURCELIM_ID_TREE_ACTIVE.remove(pfsrlim) };

        if let Some(t) = pfsrlim.pfsrlim_overload.table.get() {
            pfr_detach_table(t);
        }

        // NKSTAT: pf_sourcelim_kstat_detach(pfsrlim); not configured.

        pf_pool_put(&PF_SOURCELIM_PL, pfsrlim);
    }

    // `l` was the C's local: forget the freed limiters.
    l.init();

    // fix up the inactive tree
    PF_SOURCELIM_ID_TREE_INACTIVE.init();
    PF_SOURCELIM_NM_TREE_INACTIVE.init();
    PF_SOURCELIM_LIST_INACTIVE.init();

    for pfl in garbage.iter() {
        pf_pool_put(&PF_STATE_LINK_PL, pfl);
    }
}

/// `pf_statelim_rollback`: frees the state limiters of an abandoned transaction.
pub fn pf_statelim_rollback() {
    pf_assert_locked();
    net_assert_locked("pf_statelim_rollback");

    for pfstlim in PF_STATELIM_LIST_INACTIVE.iter() {
        pf_pool_put(&PF_STATELIM_PL, pfstlim);
    }

    PF_STATELIM_LIST_INACTIVE.init();
    PF_STATELIM_ID_TREE_INACTIVE.init();
    PF_STATELIM_NM_TREE_INACTIVE.init();
}

/// `pf_statelim_rb_find`.
fn pf_statelim_rb_find(
    tree: &'static RbtHead<PfStatelimIdTree>,
    key: &PfStatelim,
) -> Option<&'static PfStatelim> {
    tree.find(key)
}

/// `pf_statelim_rb_nfind`.
fn pf_statelim_rb_nfind(
    tree: &'static RbtHead<PfStatelimIdTree>,
    key: &PfStatelim,
) -> Option<&'static PfStatelim> {
    tree.nfind(key)
}

/// `pf_statelim_get`: `DIOCGETSTATELIM` (the limiter `ioc->id`) and `DIOCGETNSTATELIM` (the
/// first from `ioc->id` on): its configuration and counters.
pub fn pf_statelim_get(ioc: &mut PfiocStatelim, rbt_op: PfStatelimRbOp) -> Result<(), Errno> {
    let key = PfStatelim::key(ioc.id);

    net_lock();
    pf_lock();

    let error = match rbt_op(&PF_STATELIM_ID_TREE_ACTIVE, &key) {
        None => Err(Errno::ENOENT),
        Some(pfstlim) => {
            ioc.id = pfstlim.pfstlim_id.get();
            ioc.limit = pfstlim.pfstlim_limit.get();
            ioc.rate.limit = pfstlim.pfstlim_rate.limit.get();
            ioc.rate.seconds = pfstlim.pfstlim_rate.seconds.get();
            ioc.name = pfstlim.pfstlim_nm.get();

            ioc.inuse = pfstlim.pfstlim_inuse.get();
            ioc.admitted = pfstlim.pfstlim_counters.admitted.get();
            ioc.hardlimited = pfstlim.pfstlim_counters.hardlimited.get();
            ioc.ratelimited = pfstlim.pfstlim_counters.ratelimited.get();
            Ok(())
        }
    };

    pf_unlock();
    net_unlock();

    error
}

/// `pf_sourcelim_add`: `DIOCADDSOURCELIM`, a source limiter for the open main ruleset
/// transaction.
pub fn pf_sourcelim_add(ioc: &PfiocSourcelim) -> Result<(), Errno> {
    if pf_name_too_long(&ioc.name) {
        return Err(Errno::ENAMETOOLONG);
    }
    if pf_name_too_long(&ioc.overload_tblname) {
        return Err(Errno::ENAMETOOLONG);
    }

    if ioc.id < PF_SOURCELIM_ID_MIN || ioc.id > PF_SOURCELIM_ID_MAX {
        return Err(Errno::EINVAL);
    }

    if ioc.entries < 1 {
        return Err(Errno::EINVAL);
    }

    if ioc.limit < 1 {
        return Err(Errno::EINVAL);
    }

    if (ioc.rate.limit == 0) != (ioc.rate.seconds == 0) {
        return Err(Errno::EINVAL);
    }

    if ioc.inet_prefix > 32 {
        return Err(Errno::EINVAL);
    }
    if ioc.inet6_prefix > 128 {
        return Err(Errno::EINVAL);
    }

    if ioc.overload_tblname[0] != 0 {
        if ioc.overload_hwm == 0 {
            return Err(Errno::EINVAL);
        }

        if ioc.overload_hwm < ioc.overload_lwm {
            return Err(Errno::EINVAL);
        }
    }

    let Some(pfsrlim) = pf_pool_get::<PfSourcelim>(&PF_SOURCELIM_PL, PR_WAITOK) else {
        return Err(Errno::ENOMEM);
    };

    pfsrlim.pfsrlim_id.set(ioc.id);
    pfsrlim.pfsrlim_entries.set(ioc.entries);
    pfsrlim.pfsrlim_limit.set(ioc.limit);
    pfsrlim.pfsrlim_ipv4_prefix.set(ioc.inet_prefix);
    pfsrlim.pfsrlim_ipv6_prefix.set(ioc.inet6_prefix);
    pfsrlim.pfsrlim_rate.limit.set(ioc.rate.limit);
    pfsrlim.pfsrlim_rate.seconds.set(ioc.rate.seconds);
    let mut name = [0u8; PF_TABLE_NAME_SIZE];
    pf_strlcpy(&mut name, &ioc.overload_tblname);
    pfsrlim.pfsrlim_overload.name.set(name);
    pfsrlim.pfsrlim_overload.hwm.set(ioc.overload_hwm);
    pfsrlim.pfsrlim_overload.lwm.set(ioc.overload_lwm);
    let mut nm = [0u8; PF_SOURCELIM_NAME_LEN];
    pf_strlcpy(&mut nm, &ioc.name);
    pfsrlim.pfsrlim_nm.set(nm);

    if pfsrlim.pfsrlim_rate.limit.get() != 0 {
        let bucket = u64::from(pfsrlim.pfsrlim_rate.seconds.get()) * 1_000_000_000;

        pfsrlim
            .pfsrlim_rate_token
            .set(bucket / u64::from(pfsrlim.pfsrlim_rate.limit.get()));
        pfsrlim.pfsrlim_rate_bucket.set(bucket);
    }

    let mut mask4 = PfAddr::zeroed();
    let bits = 0xffff_ffffu32
        .checked_shl(32 - pfsrlim.pfsrlim_ipv4_prefix.get())
        .unwrap_or(0);
    mask4.set_addr32(0, bits.to_be());
    pfsrlim.pfsrlim_ipv4_mask.set(mask4);

    let mut mask6 = PfAddr::zeroed();
    let mut prefix = pfsrlim.pfsrlim_ipv6_prefix.get();
    for i in 0..4 {
        if prefix == 0 {
            // the memory is already zeroed
            break;
        }
        if prefix < 32 {
            mask6.set_addr32(i, (0xffff_ffffu32 << (32 - prefix)).to_be());
            break;
        }

        mask6.set_addr32(i, 0xffff_ffffu32.to_be());
        prefix -= 32;
    }
    pfsrlim.pfsrlim_ipv6_mask.set(mask6);

    pfsrlim.pfsrlim_sources.init();
    pc_lock_init(&pfsrlim.pfsrlim_lock);

    net_lock();
    pf_lock();
    let error = 'unlock: {
        if ioc.ticket != pf_main_ruleset().inactive.version.get() {
            break 'unlock Errno::EBUSY;
        }

        if pfsrlim.pfsrlim_overload.name.get()[0] != 0 {
            let t = pfr_attach_table(
                pf_main_ruleset(),
                &pfsrlim.pfsrlim_overload.name.get(),
                PR_WAITOK,
            );
            pfsrlim.pfsrlim_overload.table.set(t);
            if t.is_none() {
                break 'unlock Errno::EINVAL;
            }
        }

        // SAFETY: a fresh limiter, in no tree; the inactive trees and list are pf_lock's.
        if unsafe { PF_SOURCELIM_ID_TREE_INACTIVE.insert(pfsrlim) }.is_some() {
            break 'unlock Errno::EBUSY;
        }

        // SAFETY: as above.
        if unsafe { PF_SOURCELIM_NM_TREE_INACTIVE.insert(pfsrlim) }.is_some() {
            // SAFETY: inserted in the id tree just above.
            unsafe { PF_SOURCELIM_ID_TREE_INACTIVE.remove(pfsrlim) };
            break 'unlock Errno::EBUSY;
        }

        // SAFETY: as above; the limiter is on no list.
        unsafe { PF_SOURCELIM_LIST_INACTIVE.insert_head(pfsrlim) };

        pf_unlock();
        net_unlock();

        return Ok(());
    };

    if let Some(t) = pfsrlim.pfsrlim_overload.table.get() {
        pfr_detach_table(t);
    }
    pf_unlock();
    net_unlock();
    pf_pool_put(&PF_SOURCELIM_PL, pfsrlim);

    Err(error)
}

/// `pf_sourcelim_rollback`: frees the source limiters of an abandoned transaction.
pub fn pf_sourcelim_rollback() {
    pf_assert_locked();
    net_assert_locked("pf_sourcelim_rollback");

    for pfsrlim in PF_SOURCELIM_LIST_INACTIVE.iter() {
        if let Some(t) = pfsrlim.pfsrlim_overload.table.get() {
            pfr_detach_table(t);
        }

        pf_pool_put(&PF_SOURCELIM_PL, pfsrlim);
    }

    PF_SOURCELIM_LIST_INACTIVE.init();
    PF_SOURCELIM_ID_TREE_INACTIVE.init();
    PF_SOURCELIM_NM_TREE_INACTIVE.init();
}

/// `pf_sourcelim_rb_find`.
fn pf_sourcelim_rb_find(
    tree: &'static RbtHead<PfSourcelimIdTree>,
    key: &PfSourcelim,
) -> Option<&'static PfSourcelim> {
    tree.find(key)
}

/// `pf_sourcelim_rb_nfind`.
fn pf_sourcelim_rb_nfind(
    tree: &'static RbtHead<PfSourcelimIdTree>,
    key: &PfSourcelim,
) -> Option<&'static PfSourcelim> {
    tree.nfind(key)
}

/// `pf_sourcelim_get`: `DIOCGETSOURCELIM` and `DIOCGETNSOURCELIM`.
pub fn pf_sourcelim_get(ioc: &mut PfiocSourcelim, rbt_op: PfSourcelimRbOp) -> Result<(), Errno> {
    let key = PfSourcelim::key(ioc.id);

    net_lock();
    pf_lock();

    let error = match rbt_op(&PF_SOURCELIM_ID_TREE_ACTIVE, &key) {
        None => Err(Errno::ESRCH),
        Some(pfsrlim) => {
            ioc.id = pfsrlim.pfsrlim_id.get();
            ioc.entries = pfsrlim.pfsrlim_entries.get();
            ioc.limit = pfsrlim.pfsrlim_limit.get();
            ioc.inet_prefix = pfsrlim.pfsrlim_ipv4_prefix.get();
            ioc.inet6_prefix = pfsrlim.pfsrlim_ipv6_prefix.get();
            ioc.rate.limit = pfsrlim.pfsrlim_rate.limit.get();
            ioc.rate.seconds = pfsrlim.pfsrlim_rate.seconds.get();

            ioc.overload_tblname = pfsrlim.pfsrlim_overload.name.get();
            ioc.overload_hwm = pfsrlim.pfsrlim_overload.hwm.get();
            ioc.overload_lwm = pfsrlim.pfsrlim_overload.lwm.get();

            ioc.name = pfsrlim.pfsrlim_nm.get();

            // XXX overload table thing

            ioc.nentries = pfsrlim.pfsrlim_nsources.get();

            ioc.inuse = pfsrlim.pfsrlim_counters.inuse.get() as u32;
            ioc.addrallocs = pfsrlim.pfsrlim_counters.addrallocs.get();
            ioc.addrnomem = pfsrlim.pfsrlim_counters.addrnomem.get();
            ioc.admitted = pfsrlim.pfsrlim_counters.admitted.get();
            ioc.addrlimited = pfsrlim.pfsrlim_counters.addrlimited.get();
            ioc.hardlimited = pfsrlim.pfsrlim_counters.hardlimited.get();
            ioc.ratelimited = pfsrlim.pfsrlim_counters.ratelimited.get();
            Ok(())
        }
    };

    pf_unlock();
    net_unlock();

    error
}

/// `pf_source_rb_find`.
fn pf_source_rb_find(
    tree: &'static RbtHead<PfSourceIocTree>,
    key: &PfSource,
) -> Option<&'static PfSource> {
    tree.find(key)
}

/// `pf_source_rb_nfind`.
fn pf_source_rb_nfind(
    tree: &'static RbtHead<PfSourceIocTree>,
    key: &PfSource,
) -> Option<&'static PfSource> {
    tree.nfind(key)
}

/// `pf_source_get`: `DIOCGETSOURCE` and `DIOCGETNSOURCE`: copies out the sources of limiter
/// `ioc->id` from the one `ioc->key` names (or the next) on, as many as `ioc->entrieslen`
/// bytes hold.
pub fn pf_source_get(ioc: &mut PfiocSource, rbt_op: PfSourceRbOp) -> Result<(), Errno> {
    let plkey = PfSourcelim::key(ioc.id);
    let esize = size_of::<PfiocSourceEntry>();
    let len = ioc.entrieslen;
    let mut used = 0usize;

    if pf_name_too_long(&ioc.name) {
        return Err(Errno::ENAMETOOLONG);
    }
    if ioc.entry_size != esize {
        return Err(Errno::EINVAL);
    }
    if len < esize {
        return Err(Errno::EMSGSIZE);
    }

    let mut e = crate::machine::copy::copyin_obj::<PfiocSourceEntry>(ioc.key)?;

    net_lock();
    pf_lock();

    let error = 'unlock: {
        let Some(pfsrlim) = pf_sourcelim_rb_find(&PF_SOURCELIM_ID_TREE_ACTIVE, &plkey) else {
            break 'unlock Err(Errno::ESRCH);
        };

        let key = PfSource::zeroed();
        key.pfsr_af.set(e.af);
        key.pfsr_rdomain.set(e.rdomain as u16);
        key.pfsr_addr.set(e.addr);
        let Some(mut pfsr) = rbt_op(&pfsrlim.pfsrlim_ioc_sources, &key) else {
            break 'unlock Err(Errno::ENOENT);
        };

        e = PfiocSourceEntry::default();

        let mut uentry = ioc.entries;
        loop {
            e.af = pfsr.pfsr_af.get();
            e.rdomain = u32::from(pfsr.pfsr_rdomain.get());
            e.addr = pfsr.pfsr_addr.get();

            e.inuse = pfsr.pfsr_inuse.get();
            e.admitted = pfsr.pfsr_counters.admitted.get();
            e.hardlimited = pfsr.pfsr_counters.hardlimited.get();
            e.ratelimited = pfsr.pfsr_counters.ratelimited.get();

            if let Err(err) = copyout_obj(&e, uentry) {
                break 'unlock Err(err);
            }

            used += esize;
            if used == len {
                break;
            }

            let Some(next) = RbtHead::<PfSourceIocTree>::next(pfsr) else {
                break;
            };
            pfsr = next;

            if (len - used) < esize {
                break 'unlock Err(Errno::EMSGSIZE);
            }

            uentry += esize;
        }

        ioc.inet_prefix = pfsrlim.pfsrlim_ipv4_prefix.get();
        ioc.inet6_prefix = pfsrlim.pfsrlim_ipv6_prefix.get();
        ioc.limit = pfsrlim.pfsrlim_limit.get();

        ioc.entrieslen = used;
        Ok(())
    };

    pf_unlock();
    net_unlock();

    error
}

/// `pf_source_clr`: `DIOCCLRSOURCE`, forgets one address of a source limiter (detaching its
/// states from the limiter).
pub fn pf_source_clr(ioc: &PfiocSourceKill) -> Result<(), Errno> {
    let plkey = PfSourcelim::key(ioc.id);
    let skey = PfSource::zeroed();
    skey.pfsr_af.set(ioc.af);
    skey.pfsr_rdomain.set(ioc.rdomain as u16);
    skey.pfsr_addr.set(ioc.addr);

    if pf_name_too_long(&ioc.name) {
        return Err(Errno::ENAMETOOLONG);
    }

    if ioc.rmstates != 0 {
        // XXX userland wants the states removed too
        return Err(Errno::EOPNOTSUPP);
    }

    net_lock();
    pf_lock();

    let Some(pfsrlim) = pf_sourcelim_rb_find(&PF_SOURCELIM_ID_TREE_ACTIVE, &plkey) else {
        pf_unlock();
        net_unlock();
        return Err(Errno::ESRCH);
    };

    let Some(pfsr) = pf_source_rb_find(&pfsrlim.pfsrlim_ioc_sources, &skey) else {
        pf_unlock();
        net_unlock();
        return Err(Errno::ENOENT);
    };

    // SAFETY: a source of the limiter is in both of its trees, and on `pf_source_gc` when
    // unused.
    unsafe {
        pfsrlim.pfsrlim_sources.remove(pfsr);
        pfsrlim.pfsrlim_ioc_sources.remove(pfsr);
        if pfsr.pfsr_inuse.get() == 0 {
            PF_SOURCE_GC.remove(pfsr);
        }
    }

    let gen_ = pf_sourcelim_enter(pfsrlim);
    pfsrlim
        .pfsrlim_nsources
        .set(pfsrlim.pfsrlim_nsources.get().wrapping_sub(1));
    pfsrlim.pfsrlim_counters.inuse.set(
        pfsrlim
            .pfsrlim_counters
            .inuse
            .get()
            .wrapping_sub(u64::from(pfsr.pfsr_inuse.get())),
    );
    pf_sourcelim_leave(pfsrlim, gen_);

    // unwire the links
    for pfl in pfsr.pfsr_states.iter() {
        if let Some(st) = pfl.pfl_state.get() {
            // if !rmst
            st.sourcelim.set(0);
            // SAFETY: a link on a source's list is on its state's linkage.
            unsafe { st.linkage.remove(pfl) };
        }
    }

    pf_unlock();
    net_unlock();

    for pfl in pfsr.pfsr_states.iter() {
        pf_pool_put(&PF_STATE_LINK_PL, pfl);
    }

    pf_pool_put(&PF_SOURCE_PL, pfsr);

    Ok(())
}

/// `pf_states_clr`: `DIOCCLRSTATES`, removes every state (or those of interface
/// `psk_ifname`); the count in `psk_killed`.
pub fn pf_states_clr(psk: &mut PfiocStateKill) -> Result<(), Errno> {
    let mut killed: u32 = 0;

    if pf_name_too_long(&psk.psk_ifname) {
        return Err(Errno::ENAMETOOLONG);
    }
    if pf_name_too_long(&psk.psk_label) {
        return Err(Errno::ENAMETOOLONG);
    }

    net_lock();

    // lock against the gc removing an item from the list
    let error = rw_enter(&PF_STATE_LIST.pfs_rwl, RW_READ | RW_INTR);
    if error.is_ok() {
        // get a snapshot view of the ends of the list to traverse between
        mtx_enter(&PF_STATE_LIST.pfs_mtx);
        let head = PF_STATE_LIST.pfs_list.first();
        let tail = PF_STATE_LIST.pfs_list.last();
        mtx_leave(&PF_STATE_LIST.pfs_mtx);

        let mut st: Option<&'static PfState> = None;
        let mut nextst = head;

        pf_lock();
        pf_state_enter_write();

        while !opt_eq(st, tail) {
            st = nextst;
            let Some(s) = st else {
                break;
            };
            nextst = TailqHead::<PfStateQueue>::next(s);

            if usize::from(s.timeout.get()) == PFTM_UNLINKED {
                continue;
            }

            if psk.psk_ifname[0] == 0 || pf_cstr(&psk.psk_ifname) == kif_name_bytes(s.kif.get()) {
                // don't send out individual delete messages
                s.state_flags.set(s.state_flags.get() | PFSTATE_NOSYNC);
                pf_remove_state(s);
                killed += 1;
            }
        }

        pf_state_exit_write();
        pf_unlock();
        rw_exit(&PF_STATE_LIST.pfs_rwl);

        psk.psk_killed = killed;

        crate::net::if_pfsync::pfsync_clear_states(PF_STATUS.hostid.get(), &psk.psk_ifname);
    }
    net_unlock();

    error
}

/// The name of a state's interface (`st->kif->pfik_name`), empty without one.
fn kif_name_bytes(kif: Option<&PfiKif>) -> &[u8] {
    kif.map_or(&[][..], |k| pf_cstr(&k.pfik_name))
}

/// `pf_states_get`: `DIOCGETSTATES`, copies the states out to `ps_buf` (as many as `ps_len`
/// bytes hold), or reports the space they need when `ps_len` is 0.
pub fn pf_states_get(ps: &mut PfiocStates) -> Result<(), Errno> {
    let mut nr: u32 = 0;
    let psize = size_of::<PfsyncState>();

    if ps.ps_len == 0 {
        nr = PF_STATUS.states.get();
        ps.ps_len = psize * nr as usize;
        return Ok(());
    }

    let mut p = ps.ps_buf;

    // lock against the gc removing an item from the list
    rw_enter(&PF_STATE_LIST.pfs_rwl, RW_READ | RW_INTR)?;

    // get a snapshot view of the ends of the list to traverse between
    mtx_enter(&PF_STATE_LIST.pfs_mtx);
    let head = PF_STATE_LIST.pfs_list.first();
    let tail = PF_STATE_LIST.pfs_list.last();
    mtx_leave(&PF_STATE_LIST.pfs_mtx);

    let mut st: Option<&'static PfState> = None;
    let mut nextst = head;
    let mut error = Ok(());
    let mut pstore = PfsyncState::default();

    'fail: {
        while !opt_eq(st, tail) {
            st = nextst;
            let Some(s) = st else {
                break;
            };
            nextst = TailqHead::<PfStateQueue>::next(s);

            if usize::from(s.timeout.get()) == PFTM_UNLINKED {
                continue;
            }

            if (nr as usize + 1) * psize > ps.ps_len {
                break;
            }

            pf_state_export(&mut pstore, s);
            error = copyout_obj(&pstore, p);
            if error.is_err() {
                break 'fail;
            }

            p += psize;
            nr += 1;
        }
        ps.ps_len = psize * nr as usize;
    }

    rw_exit(&PF_STATE_LIST.pfs_rwl);

    error
}

/// The `pfrio_flags` of a `struct pfioc_table` argument (the securelevel and write checks).
fn pfioc_table_flags(data: &[u8]) -> i32 {
    let off = offset_of!(PfiocTable, pfrio_flags);
    data.get(off..off + 4)
        .map_or(0, |b| i32::from_ne_bytes([b[0], b[1], b[2], b[3]]))
}

/// The `action` of a `struct pfioc_rule` argument (the write check).
fn pfioc_rule_action(data: &[u8]) -> u32 {
    let off = offset_of!(PfiocRule, action);
    data.get(off..off + 4)
        .map_or(0, |b| u32::from_ne_bytes([b[0], b[1], b[2], b[3]]))
}

/// `pfioctl`: the `/dev/pf` ioctls. `data` is `sys_ioctl`'s kernel copy of the argument.
pub fn pfioctl(dev: Dev, cmd: u64, data: &mut [u8], flags: i32, p: &Proc) -> Result<(), Errno> {
    let mut flags = flags;

    // XXX keep in sync with switch() below
    if SECURELEVEL.load(AtomicOrdering::Relaxed) > 1 {
        match cmd {
            DIOCGETRULES | DIOCGETRULE | DIOCGETSTATE | DIOCSETSTATUSIF | DIOCGETSTATUS
            | DIOCCLRSTATUS | DIOCNATLOOK | DIOCSETDEBUG | DIOCGETSTATES | DIOCGETTIMEOUT
            | DIOCGETLIMIT | DIOCGETSTATELIM | DIOCGETNSTATELIM | DIOCGETSOURCELIM
            | DIOCGETNSOURCELIM | DIOCGETSOURCE | DIOCGETNSOURCE | DIOCGETRULESETS
            | DIOCGETRULESET | DIOCGETQUEUES | DIOCGETQUEUE | DIOCGETQSTATS | DIOCRGETTABLES
            | DIOCRGETTSTATS | DIOCRCLRTSTATS | DIOCRCLRADDRS | DIOCRADDADDRS | DIOCRDELADDRS
            | DIOCRSETADDRS | DIOCRGETADDRS | DIOCRGETASTATS | DIOCRCLRASTATS | DIOCRTSTADDRS
            | DIOCOSFPGET | DIOCGETSRCNODES | DIOCCLRSRCNODES | DIOCIGETIFACES | DIOCSETIFFLAG
            | DIOCCLRIFFLAG | DIOCGETSYNFLWATS => {}
            DIOCRCLRTABLES | DIOCRADDTABLES | DIOCRDELTABLES | DIOCRSETTFLAGS => {
                if pfioc_table_flags(data) & PFR_FLAG_DUMMY == 0 {
                    return Err(Errno::EPERM);
                }
                // dummy operation ok
            }
            _ => return Err(Errno::EPERM),
        }
    }

    if flags & FWRITE == 0 {
        match cmd {
            DIOCGETRULES | DIOCGETSTATE | DIOCGETSTATUS | DIOCGETSTATES | DIOCGETTIMEOUT
            | DIOCGETLIMIT | DIOCGETSTATELIM | DIOCGETNSTATELIM | DIOCGETSOURCELIM
            | DIOCGETNSOURCELIM | DIOCGETSOURCE | DIOCGETNSOURCE | DIOCGETRULESETS
            | DIOCGETRULESET | DIOCGETQUEUES | DIOCGETQUEUE | DIOCGETQSTATS | DIOCNATLOOK
            | DIOCRGETTABLES | DIOCRGETTSTATS | DIOCRGETADDRS | DIOCRGETASTATS | DIOCRTSTADDRS
            | DIOCOSFPGET | DIOCGETSRCNODES | DIOCIGETIFACES | DIOCGETSYNFLWATS | DIOCXEND => {}
            DIOCRCLRTABLES | DIOCRADDTABLES | DIOCRDELTABLES | DIOCRCLRTSTATS | DIOCRCLRADDRS
            | DIOCRADDADDRS | DIOCRDELADDRS | DIOCRSETADDRS | DIOCRSETTFLAGS => {
                if pfioc_table_flags(data) & PFR_FLAG_DUMMY == 0 {
                    return Err(Errno::EACCES);
                }
                flags |= FWRITE; // need write lock for dummy
                // dummy operation ok
            }
            DIOCGETRULE => {
                if pfioc_rule_action(data) == PF_GET_CLR_CNTR {
                    return Err(Errno::EACCES);
                }
            }
            _ => return Err(Errno::EACCES),
        }
    }
    let _ = flags;

    rw_enter_write(&PFIOCTL_RW);
    let error = pfioctl_locked(dev, cmd, data, p);
    rw_exit_write(&PFIOCTL_RW);

    error
}

/// The `switch (cmd)` of `pfioctl`, under `pfioctl_rw`; a return is the C's `goto fail`.
fn pfioctl_locked(dev: Dev, cmd: u64, data: &mut [u8], p: &Proc) -> Result<(), Errno> {
    match cmd {
        DIOCSTART => {
            let mut error = Ok(());
            net_lock();
            pf_lock();
            if PF_STATUS.running.get() != 0 {
                error = Err(Errno::EEXIST);
            } else {
                PF_STATUS.running.set(1);
                PF_STATUS.since.set(getuptime());
                if PF_STATUS.stateid.get() == 0 {
                    PF_STATUS.stateid.set(gettime() as u64);
                    PF_STATUS.stateid.set(PF_STATUS.stateid.get() << 32);
                }
                timeout_add_sec(&PF_PURGE_STATES_TO, 1);
                timeout_add_sec(&PF_PURGE_TO, 1);
                // The C ignores the result.
                let _ = pf_create_queues();
                crate::dpfprintf!(LOG_NOTICE, "pf: started");
            }
            pf_unlock();
            net_unlock();
            error
        }

        DIOCSTOP => {
            let mut error = Ok(());
            net_lock();
            pf_lock();
            if PF_STATUS.running.get() == 0 {
                error = Err(Errno::ENOENT);
            } else {
                PF_STATUS.running.set(0);
                PF_STATUS.since.set(getuptime());
                pf_remove_queues();
                crate::dpfprintf!(LOG_NOTICE, "pf: stopped");
            }
            pf_unlock();
            net_unlock();
            error
        }

        DIOCGETQUEUES => {
            let mut pq = pf_abi_read::<PfiocQueue>(data);
            let mut nr: u32 = 0;

            pf_lock();
            pq.ticket = pf_main_ruleset().active.version.get();

            // save state to not run over them all each time?
            let mut qs = pf_queues_active().first();
            while let Some(q) = qs {
                qs = TailqHead::<PfQueuehead>::next(q);
                nr += 1;
            }
            pq.nr = nr;
            pf_unlock();
            pf_abi_write(data, &*pq);
            Ok(())
        }

        DIOCGETQUEUE => {
            let mut pq = pf_abi_read::<PfiocQueue>(data);
            let mut nr: u32 = 0;

            pf_lock();
            if pq.ticket != pf_main_ruleset().active.version.get() {
                pf_unlock();
                return Err(Errno::EBUSY);
            }

            // save state to not run over them all each time?
            let mut qs = pf_queues_active().first();
            while let Some(q) = qs {
                if nr >= pq.nr {
                    break;
                }
                nr += 1;
                qs = TailqHead::<PfQueuehead>::next(q);
            }
            let Some(qs) = qs else {
                pf_unlock();
                return Err(Errno::EBUSY);
            };
            pf_abi_copy_into(&mut pq.queue, qs);
            pf_unlock();
            pf_abi_write(data, &*pq);
            Ok(())
        }

        DIOCGETQSTATS => {
            let mut pq = pf_abi_read::<PfiocQstats>(data);

            net_lock();
            pf_lock();
            if pq.ticket != pf_main_ruleset().active.version.get() {
                pf_unlock();
                net_unlock();
                return Err(Errno::EBUSY);
            }
            let mut nbytes = pq.nbytes;
            let mut nr: u32 = 0;

            // save state to not run over them all each time?
            let mut qs = pf_queues_active().first();
            while let Some(q) = qs {
                if nr >= pq.nr {
                    break;
                }
                nr += 1;
                qs = TailqHead::<PfQueuehead>::next(q);
            }
            let Some(qs) = qs else {
                pf_unlock();
                net_unlock();
                return Err(Errno::EBUSY);
            };
            pf_abi_copy_into(&mut pq.queue, qs);
            // It's a root flow queue but is not an HFSC root class
            let error = if qs.flags & PFQS_FLOWQUEUE != 0
                && qs.parent_qid == 0
                && qs.flags & PFQS_ROOTCLASS == 0
            {
                (PFQ_FQCODEL_OPS.pfq_qstats)(qs, pq.buf, &mut nbytes)
            } else {
                (PFQ_HFSC_OPS.pfq_qstats)(qs, pq.buf, &mut nbytes)
            };
            if error.is_ok() {
                pq.nbytes = nbytes;
            }
            pf_unlock();
            net_unlock();
            pf_abi_write(data, &*pq);
            error
        }

        DIOCADDQUEUE => {
            let q = pf_abi_read::<PfiocQueue>(data);

            if pf_name_too_long(&q.queue.qname)
                || pf_name_too_long(&q.queue.parent)
                || pf_name_too_long(&q.queue.ifname)
            {
                return Err(Errno::ENAMETOOLONG);
            }

            let Some(mut qs) =
                PfPoolNew::<PfQueuespec>::get(&PF_QUEUE_PL, PR_WAITOK | PR_LIMITFAIL)
            else {
                return Err(Errno::ENOMEM);
            };

            net_lock();
            pf_lock();
            let error = 'fail: {
                if q.ticket != pf_main_ruleset().inactive.version.get() {
                    break 'fail Errno::EBUSY;
                }
                let s = qs.get_mut();
                pf_abi_copy_into(s, &q.queue);
                s.qid = u32::from(pf_qname2qid(&s.qname, true));
                if s.qid == 0 {
                    break 'fail Errno::EBUSY;
                }
                if s.parent[0] != 0 {
                    s.parent_qid = u32::from(pf_qname2qid(&s.parent, false));
                    if s.parent_qid == 0 {
                        break 'fail Errno::ESRCH;
                    }
                }
                let kif = pfi_kif_get(&s.ifname, None);
                s.set_kif(kif);
                let Some(kif) = kif else {
                    break 'fail Errno::ESRCH;
                };
                // XXX resolve bw percentage specs
                pfi_kif_ref(kif, PFI_KIF_REF_RULE);

                let qs = qs.publish();
                // SAFETY: a fresh queue spec, on no list; the inactive queues are pf_lock's.
                unsafe { pf_queues_inactive().insert_tail(qs) };
                pf_unlock();
                net_unlock();

                return Ok(());
            };
            pf_unlock();
            net_unlock();
            qs.put(&PF_QUEUE_PL);
            Err(error)
        }

        DIOCADDSTATELIM => pf_statelim_add(&ioctl_arg::<PfiocStatelim>(data)),
        DIOCGETSTATELIM => {
            let mut ioc = ioctl_arg::<PfiocStatelim>(data);
            let error = pf_statelim_get(&mut ioc, pf_statelim_rb_find);
            ioctl_ret(data, &ioc);
            error
        }
        DIOCGETNSTATELIM => {
            let mut ioc = ioctl_arg::<PfiocStatelim>(data);
            let error = pf_statelim_get(&mut ioc, pf_statelim_rb_nfind);
            ioctl_ret(data, &ioc);
            error
        }

        DIOCADDSOURCELIM => pf_sourcelim_add(&ioctl_arg::<PfiocSourcelim>(data)),
        DIOCGETSOURCELIM => {
            let mut ioc = ioctl_arg::<PfiocSourcelim>(data);
            let error = pf_sourcelim_get(&mut ioc, pf_sourcelim_rb_find);
            ioctl_ret(data, &ioc);
            error
        }
        DIOCGETNSOURCELIM => {
            let mut ioc = ioctl_arg::<PfiocSourcelim>(data);
            let error = pf_sourcelim_get(&mut ioc, pf_sourcelim_rb_nfind);
            ioctl_ret(data, &ioc);
            error
        }

        DIOCGETSOURCE => {
            let mut ioc = ioctl_arg::<PfiocSource>(data);
            let error = pf_source_get(&mut ioc, pf_source_rb_find);
            ioctl_ret(data, &ioc);
            error
        }
        DIOCGETNSOURCE => {
            let mut ioc = ioctl_arg::<PfiocSource>(data);
            let error = pf_source_get(&mut ioc, pf_source_rb_nfind);
            ioctl_ret(data, &ioc);
            error
        }
        DIOCCLRSOURCE => pf_source_clr(&ioctl_arg::<PfiocSourceKill>(data)),

        DIOCADDRULE => {
            let pr = pf_abi_read::<PfiocRule>(data);

            if pf_name_too_long(&pr.anchor) || pf_name_too_long(&pr.anchor_call) {
                return Err(Errno::ENAMETOOLONG);
            }

            let Some(mut nrule) = PfPoolNew::<PfRule>::get(&PF_RULE_PL, PR_WAITOK | PR_LIMITFAIL)
            else {
                return Err(Errno::ENOMEM);
            };

            if let Err(e) = pf_rule_copyin(&pr.rule, nrule.get_mut()) {
                pf_rule_free(Some(nrule.publish()));
                return Err(e);
            }

            if pr.rule.return_icmp >> 8 > u16::from(ICMP_MAXTYPE) {
                pf_rule_free(Some(nrule.publish()));
                return Err(Errno::EINVAL);
            }
            if let Err(e) = pf_rule_checkaf(&nrule) {
                pf_rule_free(Some(nrule.publish()));
                return Err(e);
            }
            if nrule.src.addr.type_.get() == PF_ADDR_NONE
                || nrule.dst.addr.type_.get() == PF_ADDR_NONE
            {
                pf_rule_free(Some(nrule.publish()));
                return Err(Errno::EINVAL);
            }

            if nrule.rt != 0 && nrule.direction == 0 {
                pf_rule_free(Some(nrule.publish()));
                return Err(Errno::EINVAL);
            }

            net_lock();
            pf_lock();
            let Some(ruleset) = pf_find_ruleset(&pr.anchor) else {
                pf_unlock();
                net_unlock();
                pf_rule_free(Some(nrule.publish()));
                return Err(Errno::EINVAL);
            };
            if pr.ticket != ruleset.inactive.version.get() {
                pf_unlock();
                net_unlock();
                pf_rule_free(Some(nrule.publish()));
                return Err(Errno::EBUSY);
            }
            let r = nrule.get_mut();
            r.cuid = p.ucred().cr_ruid.get();
            r.cpid = p.process().ps_pid.get();
            let rule = nrule.publish();

            let tail = ruleset.inactive_ptr().last();
            rule.nr.set(tail.map_or(0, |t| t.nr.get().wrapping_add(1)));

            let error = pf_rule_setup(rule, ruleset, &pr.anchor_call);
            if let Err(e) = error {
                pf_rm_rule(None, rule);
                pf_unlock();
                net_unlock();
                return Err(e);
            }
            // SAFETY: a fresh rule, in no queue; the ruleset's queues are pf_lock's.
            unsafe { ruleset.inactive_ptr().insert_tail(rule) };
            ruleset
                .inactive
                .rcount
                .set(ruleset.inactive.rcount.get() + 1);
            pf_unlock();
            net_unlock();
            Ok(())
        }

        DIOCGETRULES => {
            let mut pr = pf_abi_read::<PfiocRule>(data);

            if pf_name_too_long(&pr.anchor) {
                // anchor_call[PATH_MAX] is not used here
                return Err(Errno::ENAMETOOLONG);
            }
            net_lock();
            pf_lock();
            let Some(ruleset) = pf_find_ruleset(&pr.anchor) else {
                pf_unlock();
                net_unlock();
                return Err(Errno::EINVAL);
            };
            pr.nr = ruleset
                .active_ptr()
                .last()
                .map_or(0, |r| r.nr.get().wrapping_add(1));
            let ruleset_version = ruleset.active.version.get();
            pf_anchor_take(ruleset.anchor.get());
            let rule = ruleset.active_ptr().first();
            pf_unlock();
            net_unlock();

            let Some(t) = pf_open_trans(minor(dev)) else {
                return Err(Errno::EBUSY);
            };
            pf_init_tgetrule(t, ruleset.anchor.get(), ruleset_version, rule);
            pr.ticket = t.pft_ticket.get() as u32;

            pf_abi_write(data, &*pr);
            Ok(())
        }

        DIOCGETRULE => {
            let mut pr = pf_abi_read::<PfiocRule>(data);

            let Some(t) = pf_find_trans(minor(dev), u64::from(pr.ticket)) else {
                return Err(Errno::ENXIO);
            };
            kassert!(t.pft_unit.get() == minor(dev));
            if t.pft_type.get() != PF_TRANS_GETRULE {
                return Err(Errno::EINVAL);
            }

            net_lock();
            pf_lock();
            let anchor = t.pftgr_anchor.get();
            kassert!(anchor.is_some());
            let ruleset: &'static PfRuleset = match anchor {
                Some(a) => &a.ruleset,
                None => pf_main_ruleset(),
            };
            if t.pftgr_version.get() != ruleset.active.version.get() {
                pf_unlock();
                net_unlock();
                return Err(Errno::EBUSY);
            }
            let Some(rule) = t.pftgr_rule.get() else {
                pf_unlock();
                net_unlock();
                return Err(Errno::ENOENT);
            };
            pf_abi_copy_into(&mut pr.rule, rule);
            pr.rule.entries = TailqEntry::new();
            pr.rule.kif.set(ptr::null());
            pr.rule.nat.kif.set(ptr::null());
            pr.rule.rdr.kif.set(ptr::null());
            pr.rule.route.kif.set(ptr::null());
            pr.rule.rcv_kif.set(ptr::null());
            pr.rule.anchor.set(ptr::null());
            pr.rule.overload_tbl.set(ptr::null());
            pr.rule
                .pktrate
                .limit
                .set(pr.rule.pktrate.limit.get() / PF_THRESHOLD_MULT);
            if !pf_anchor_copyout(ruleset, rule, &mut pr) {
                pf_unlock();
                net_unlock();
                return Err(Errno::EBUSY);
            }
            // SAFETY: `pr.rule` is a byte copy of the kernel rule `rule`, made under
            // pf_lock, which is still held.
            unsafe {
                pf_addr_copyout(&pr.rule.src.addr);
                pf_addr_copyout(&pr.rule.dst.addr);
                pf_addr_copyout(&pr.rule.rdr.addr);
                pf_addr_copyout(&pr.rule.nat.addr);
                pf_addr_copyout(&pr.rule.route.addr);
            }
            for i in 0..PF_SKIP_COUNT {
                match rule.skip[i].ptr() {
                    None => pr.rule.skip[i].set_nr(u32::MAX),
                    Some(s) => pr.rule.skip[i].set_nr(s.nr.get()),
                }
            }

            if pr.action == PF_GET_CLR_CNTR {
                rule.evaluations.set(0);
                rule.packets[0].set(0);
                rule.packets[1].set(0);
                rule.bytes[0].set(0);
                rule.bytes[1].set(0);
                rule.states_tot.set(0);
            }
            pr.nr = rule.nr.get();
            t.pftgr_rule.set(TailqHead::<PfRulequeue>::next(rule));
            pf_unlock();
            net_unlock();
            pf_abi_write(data, &*pr);
            Ok(())
        }

        DIOCCHANGERULE => {
            let mut pcr = pf_abi_read::<PfiocRule>(data);
            let mut newrule: Option<PfPoolNew<PfRule>> = None;

            if pf_name_too_long(&pcr.anchor) || pf_name_too_long(&pcr.anchor_call) {
                return Err(Errno::ENAMETOOLONG);
            }

            if pcr.action < PF_CHANGE_ADD_HEAD || pcr.action > PF_CHANGE_GET_TICKET {
                return Err(Errno::EINVAL);
            }

            if pcr.action == PF_CHANGE_GET_TICKET {
                net_lock();
                pf_lock();

                let error = match pf_find_ruleset(&pcr.anchor) {
                    None => Err(Errno::EINVAL),
                    Some(ruleset) => {
                        ruleset
                            .active
                            .version
                            .set(ruleset.active.version.get().wrapping_add(1));
                        pcr.ticket = ruleset.active.version.get();
                        Ok(())
                    }
                };

                pf_unlock();
                net_unlock();
                pf_abi_write(data, &*pcr);
                return error;
            }

            if pcr.action != PF_CHANGE_REMOVE {
                let Some(mut nr) = PfPoolNew::<PfRule>::get(&PF_RULE_PL, PR_WAITOK | PR_LIMITFAIL)
                else {
                    return Err(Errno::ENOMEM);
                };

                if pcr.rule.return_icmp >> 8 > u16::from(ICMP_MAXTYPE) {
                    nr.put(&PF_RULE_PL);
                    return Err(Errno::EINVAL);
                }
                if let Err(e) = pf_rule_copyin(&pcr.rule, nr.get_mut()) {
                    pf_rule_free(Some(nr.publish()));
                    return Err(e);
                }
                if let Err(e) = pf_rule_checkaf(&nr) {
                    pf_rule_free(Some(nr.publish()));
                    return Err(e);
                }
                if nr.rt != 0 && nr.direction == 0 {
                    pf_rule_free(Some(nr.publish()));
                    return Err(Errno::EINVAL);
                }
                newrule = Some(nr);
            }

            net_lock();
            pf_lock();
            let Some(ruleset) = pf_find_ruleset(&pcr.anchor) else {
                pf_unlock();
                net_unlock();
                pf_rule_free(newrule.map(PfPoolNew::publish));
                return Err(Errno::EINVAL);
            };

            if pcr.ticket != ruleset.active.version.get() {
                pf_unlock();
                net_unlock();
                pf_rule_free(newrule.map(PfPoolNew::publish));
                return Err(Errno::EINVAL);
            }

            let mut newrule: Option<&'static PfRule> = match newrule {
                None => None,
                Some(mut nr) => {
                    let r = nr.get_mut();
                    r.cuid = p.ucred().cr_ruid.get();
                    r.cpid = p.process().ps_pid.get();
                    Some(nr.publish())
                }
            };

            if pcr.action != PF_CHANGE_REMOVE {
                kassert!(newrule.is_some());
                if let Some(nr) = newrule
                    && let Err(e) = pf_rule_setup(nr, ruleset, &pcr.anchor_call)
                {
                    pf_rm_rule(None, nr);
                    pf_unlock();
                    net_unlock();
                    return Err(e);
                }
            }

            let oldrule = if pcr.action == PF_CHANGE_ADD_HEAD {
                ruleset.active_ptr().first()
            } else if pcr.action == PF_CHANGE_ADD_TAIL {
                ruleset.active_ptr().last()
            } else {
                let o = ruleset.active_ptr().iter().find(|r| r.nr.get() == pcr.nr);
                if o.is_none() {
                    if let Some(nr) = newrule.take() {
                        pf_rm_rule(None, nr);
                    }
                    pf_unlock();
                    net_unlock();
                    return Err(Errno::EINVAL);
                }
                o
            };

            if pcr.action == PF_CHANGE_REMOVE {
                if let Some(o) = oldrule {
                    pf_rm_rule(Some(ruleset.active_ptr()), o);
                }
                ruleset
                    .active
                    .rcount
                    .set(ruleset.active.rcount.get().wrapping_sub(1));
            } else if let Some(nr) = newrule {
                match oldrule {
                    // SAFETY: a fresh rule, in no queue; the ruleset's queues are pf_lock's.
                    None => unsafe { ruleset.active_ptr().insert_tail(nr) },
                    Some(o)
                        if pcr.action == PF_CHANGE_ADD_HEAD
                            || pcr.action == PF_CHANGE_ADD_BEFORE =>
                    {
                        // SAFETY: as above; `o` is on the active queue.
                        unsafe { TailqHead::<PfRulequeue>::insert_before(o, nr) }
                    }
                    // SAFETY: as above.
                    Some(o) => unsafe { ruleset.active_ptr().insert_after(o, nr) },
                }
                ruleset.active.rcount.set(ruleset.active.rcount.get() + 1);
            }

            for (nr, oldrule) in (0u32..).zip(ruleset.active_ptr().iter()) {
                oldrule.nr.set(nr);
            }

            ruleset
                .active
                .version
                .set(ruleset.active.version.get().wrapping_add(1));

            pf_calc_skip_steps(ruleset.active_ptr());
            pf_remove_if_empty_ruleset(ruleset);

            pf_unlock();
            net_unlock();
            Ok(())
        }

        DIOCCLRSTATES => {
            let mut psk = pf_abi_read::<PfiocStateKill>(data);
            let error = pf_states_clr(&mut psk);
            pf_abi_write(data, &*psk);
            error
        }

        DIOCKILLSTATES => {
            let mut psk = pf_abi_read::<PfiocStateKill>(data);
            let error = pfioctl_killstates(&mut psk);
            pf_abi_write(data, &*psk);
            error
        }

        DIOCADDSTATE => {
            let ps = ioctl_arg::<PfiocState>(data);
            let sp = &ps.state;

            if usize::from(sp.timeout) >= PFTM_MAX {
                return Err(Errno::EINVAL);
            }
            net_lock();
            pf_lock();
            let error = pf_state_import(sp, crate::net::if_pfsync::PFSYNC_SI_IOCTL);
            pf_unlock();
            net_unlock();
            error
        }

        DIOCGETSTATE => {
            let mut ps = ioctl_arg::<PfiocState>(data);
            let id_key = PfStateCmp {
                id: ps.state.id,
                creatorid: ps.state.creatorid,
                ..PfStateCmp::default()
            };

            net_lock();
            pf_state_enter_read();
            let st = pf_find_state_byid(&id_key).map(pf_state_ref);
            pf_state_exit_read();
            net_unlock();
            let Some(st) = st else {
                return Err(Errno::ENOENT);
            };

            pf_state_export(&mut ps.state, st);
            pf_state_unref(Some(st));
            ioctl_ret(data, &ps);
            Ok(())
        }

        DIOCGETSTATES => {
            let mut ps = ioctl_arg::<PfiocStates>(data);
            let error = pf_states_get(&mut ps);
            ioctl_ret(data, &ps);
            error
        }

        DIOCGETSTATUS => {
            let mut s = pf_abi_read::<PfStatus>(data);
            let error = pf_status_read(&mut s);
            pf_abi_write(data, &*s);
            error
        }

        DIOCSETSTATUSIF => {
            let pi = ioctl_arg::<PfiocIface>(data);

            if pf_name_too_long(&pi.pfiio_name) {
                return Err(Errno::ENAMETOOLONG);
            }

            net_lock();
            pf_lock();
            if pi.pfiio_name[0] == 0 {
                PF_STATUS.ifname.set([0; IFNAMSIZ]);
            } else {
                let mut statusif = [0u8; IFNAMSIZ];
                pf_strlcpy(&mut statusif, &pi.pfiio_name);
                PF_TRANS_SET.statusif.set(statusif);
                PF_TRANS_SET
                    .mask
                    .set(PF_TRANS_SET.mask.get() | PF_TSET_STATUSIF);
            }
            pf_unlock();
            net_unlock();
            Ok(())
        }

        DIOCCLRSTATUS => {
            let pi = ioctl_arg::<PfiocIface>(data);

            if pf_name_too_long(&pi.pfiio_name) {
                return Err(Errno::ENAMETOOLONG);
            }

            net_lock();
            pf_lock();
            // if ifname is specified, clear counters there only
            if pi.pfiio_name[0] != 0 {
                pfi_update_status(&pi.pfiio_name, None);
                pf_unlock();
                net_unlock();
                return Ok(());
            }

            pf_status_clear();
            for c in &PF_STATUS.counters {
                c.set(0);
            }
            for c in &PF_STATUS.scounters {
                c.set(0);
            }
            pf_frag_lock();
            for c in &PF_STATUS.ncounters {
                c.set(0);
            }
            pf_frag_unlock();
            PF_STATUS.since.set(getuptime());

            pf_unlock();
            net_unlock();
            Ok(())
        }

        DIOCNATLOOK => {
            let mut pnl = ioctl_arg::<PfiocNatlook>(data);
            let error = pfioctl_natlook(&mut pnl);
            ioctl_ret(data, &pnl);
            error
        }

        DIOCSETTIMEOUT => {
            let mut pt = ioctl_arg::<PfiocTm>(data);

            if pt.timeout < 0 || pt.timeout as usize >= PFTM_MAX || pt.seconds < 0 {
                return Err(Errno::EINVAL);
            }
            net_lock();
            pf_lock();
            if pt.timeout as usize == PFTM_INTERVAL && pt.seconds == 0 {
                pt.seconds = 1;
            }
            PF_DEFAULT_RULE_NEW.timeout[pt.timeout as usize].set(pt.seconds as u32);
            pt.seconds = PF_DEFAULT_RULE.timeout(pt.timeout as usize) as i32;
            pf_unlock();
            net_unlock();
            ioctl_ret(data, &pt);
            Ok(())
        }

        DIOCGETTIMEOUT => {
            let mut pt = ioctl_arg::<PfiocTm>(data);

            if pt.timeout < 0 || pt.timeout as usize >= PFTM_MAX {
                return Err(Errno::EINVAL);
            }
            pf_lock();
            pt.seconds = PF_DEFAULT_RULE.timeout(pt.timeout as usize) as i32;
            pf_unlock();
            ioctl_ret(data, &pt);
            Ok(())
        }

        DIOCGETLIMIT => {
            let mut pl = ioctl_arg::<PfiocLimit>(data);

            if pl.index < 0 || pl.index as usize >= PF_LIMIT_MAX {
                return Err(Errno::EINVAL);
            }
            pf_lock();
            pl.limit = PF_POOL_LIMITS[pl.index as usize].limit.get();
            pf_unlock();
            ioctl_ret(data, &pl);
            Ok(())
        }

        DIOCSETLIMIT => {
            let pl = ioctl_arg::<PfiocLimit>(data);

            pf_lock();
            if pl.index < 0 || pl.index as usize >= PF_LIMIT_MAX {
                pf_unlock();
                return Err(Errno::EINVAL);
            }
            let lim = &PF_POOL_LIMITS[pl.index as usize];
            let Some(pp) = lim.pp else {
                pf_unlock();
                return Err(Errno::EINVAL);
            };
            if pp.pr_nout.get() > pl.limit {
                pf_unlock();
                return Err(Errno::EBUSY);
            }
            // Fragments reference mbuf clusters.
            if pl.index as usize == PF_LIMIT_FRAGS
                && i64::from(pl.limit) > NMBCLUST.load(AtomicOrdering::Relaxed)
            {
                pf_unlock();
                return Err(Errno::EINVAL);
            }

            let error = pool_sethardlimit(pp, pl.limit);
            if error.is_ok() {
                lim.limit_new.set(pl.limit);
                lim.limit.set(pl.limit);
            }
            pf_unlock();
            error
        }

        DIOCSETDEBUG => {
            let level = ioctl_arg::<i32>(data) as u32;

            net_lock();
            pf_lock();
            PF_TRANS_SET.debug.set(level);
            PF_TRANS_SET
                .mask
                .set(PF_TRANS_SET.mask.get() | PF_TSET_DEBUG);
            pf_unlock();
            net_unlock();
            Ok(())
        }

        DIOCGETRULESETS => {
            let mut pr = ioctl_arg::<PfiocRuleset>(data);

            if pf_name_too_long(&pr.path) {
                return Err(Errno::ENAMETOOLONG);
            }

            pf_lock();
            let Some(ruleset) = pf_find_ruleset(&pr.path) else {
                pf_unlock();
                return Err(Errno::EINVAL);
            };
            pr.nr = 0;
            if ptr::eq(ruleset, pf_main_ruleset()) {
                // XXX kludge for pf_main_ruleset
                for anchor in PF_ANCHORS.iter() {
                    if anchor.parent.get().is_none() {
                        pr.nr += 1;
                    }
                }
            } else if let Some(a) = ruleset.anchor.get() {
                for _anchor in a.children.iter() {
                    pr.nr += 1;
                }
            }
            pf_unlock();
            ioctl_ret(data, &pr);
            Ok(())
        }

        DIOCGETRULESET => {
            let mut pr = ioctl_arg::<PfiocRuleset>(data);
            let mut nr: u32 = 0;

            if pf_name_too_long(&pr.path) || pf_name_too_long(&pr.name) {
                return Err(Errno::ENAMETOOLONG);
            }

            pf_lock();
            let Some(ruleset) = pf_find_ruleset(&pr.path) else {
                pf_unlock();
                return Err(Errno::EINVAL);
            };
            if ptr::eq(ruleset, pf_main_ruleset()) {
                // XXX kludge for pf_main_ruleset
                for anchor in PF_ANCHORS.iter() {
                    if anchor.parent.get().is_none() {
                        let n = nr;
                        nr += 1;
                        if n == pr.nr {
                            pf_strlcpy(&mut pr.name, &anchor.name);
                            break;
                        }
                    }
                }
            } else if let Some(a) = ruleset.anchor.get() {
                for anchor in a.children.iter() {
                    let n = nr;
                    nr += 1;
                    if n == pr.nr {
                        pf_strlcpy(&mut pr.name, &anchor.name);
                        break;
                    }
                }
            }
            pf_unlock();
            ioctl_ret(data, &pr);
            if pr.name[0] == 0 {
                return Err(Errno::EBUSY);
            }
            Ok(())
        }

        DIOCRCLRTABLES | DIOCRADDTABLES | DIOCRDELTABLES | DIOCRGETTABLES | DIOCRGETTSTATS
        | DIOCRCLRTSTATS | DIOCRSETTFLAGS | DIOCRCLRADDRS | DIOCRADDADDRS | DIOCRDELADDRS
        | DIOCRSETADDRS | DIOCRGETADDRS | DIOCRGETASTATS | DIOCRCLRASTATS | DIOCRTSTADDRS
        | DIOCRINADEFINE => {
            let mut io = pf_abi_read::<PfiocTable>(data);
            let error = pfioctl_table(cmd, &mut io);
            pf_abi_write(data, &*io);
            error
        }

        DIOCOSFPADD => pf_osfp_add(&ioctl_arg::<PfOsfpIoctl>(data)),

        DIOCOSFPGET => {
            let mut io = ioctl_arg::<PfOsfpIoctl>(data);
            let error = pf_osfp_get(&mut io);
            ioctl_ret(data, &io);
            error
        }

        DIOCXBEGIN => pfioctl_xbegin(&ioctl_arg::<PfiocTrans>(data)),
        DIOCXROLLBACK => pfioctl_xrollback(&ioctl_arg::<PfiocTrans>(data)),
        DIOCXCOMMIT => pfioctl_xcommit(&ioctl_arg::<PfiocTrans>(data)),

        DIOCXEND => {
            let ticket = ioctl_arg::<i32>(data) as u32;

            match pf_find_trans(minor(dev), u64::from(ticket)) {
                Some(t) => {
                    pf_rollback_trans(Some(t));
                    Ok(())
                }
                None => Err(Errno::ENXIO),
            }
        }

        DIOCGETSRCNODES => {
            let mut psn = ioctl_arg::<PfiocSrcNodes>(data);
            let error = pfioctl_getsrcnodes(&mut psn);
            ioctl_ret(data, &psn);
            error
        }

        DIOCCLRSRCNODES => {
            net_lock();
            pf_lock();
            pf_state_enter_write();
            for st in TREE_ID.iter() {
                pf_src_tree_remove_state(st);
            }
            pf_state_exit_write();
            for n in TREE_SRC_TRACKING.iter() {
                n.expire.set(1);
            }
            pf_purge_expired_src_nodes();
            pf_unlock();
            net_unlock();
            Ok(())
        }

        DIOCKILLSRCNODES => {
            let mut psnk = pf_abi_read::<PfiocSrcNodeKill>(data);
            let mut killed: u32 = 0;

            net_lock();
            pf_lock();
            for sn in TREE_SRC_TRACKING.iter() {
                let (s, d) = (psnk.psnk_src.addr.v.get(), psnk.psnk_dst.addr.v.get());
                if pf_match_addr(
                    psnk.psnk_src.neg.get(),
                    &s.addr(),
                    &s.mask(),
                    &sn.addr.get(),
                    sn.af.get(),
                ) && pf_match_addr(
                    psnk.psnk_dst.neg.get(),
                    &d.addr(),
                    &d.mask(),
                    &sn.raddr.get(),
                    sn.af.get(),
                ) {
                    // Handle state to src_node linkage
                    if sn.states.get() != 0 {
                        pf_assert_locked();
                        pf_state_enter_write();
                        for st in TREE_ID.iter() {
                            pf_state_rm_src_node(st, sn);
                        }
                        pf_state_exit_write();
                    }
                    sn.expire.set(1);
                    killed += 1;
                }
            }

            if killed > 0 {
                pf_purge_expired_src_nodes();
            }

            psnk.psnk_killed = killed;
            pf_unlock();
            net_unlock();
            pf_abi_write(data, &*psnk);
            Ok(())
        }

        DIOCSETHOSTID => {
            let hostid = ioctl_arg::<i32>(data) as u32;

            net_lock();
            pf_lock();
            if hostid == 0 {
                PF_TRANS_SET.hostid.set(arc4random());
            } else {
                PF_TRANS_SET.hostid.set(hostid);
            }
            PF_TRANS_SET
                .mask
                .set(PF_TRANS_SET.mask.get() | PF_TSET_HOSTID);
            pf_unlock();
            net_unlock();
            Ok(())
        }

        DIOCOSFPFLUSH => {
            pf_osfp_flush();
            Ok(())
        }

        DIOCIGETIFACES => {
            let mut io = ioctl_arg::<PfiocIface>(data);
            let error = pfioctl_igetifaces(&mut io);
            ioctl_ret(data, &io);
            error
        }

        DIOCSETIFFLAG => {
            let io = ioctl_arg::<PfiocIface>(data);

            if pf_name_too_long(&io.pfiio_name) {
                return Err(Errno::ENAMETOOLONG);
            }

            pf_lock();
            let error = pfi_set_flags(&io.pfiio_name, io.pfiio_flags);
            pf_unlock();
            error
        }

        DIOCCLRIFFLAG => {
            let io = ioctl_arg::<PfiocIface>(data);

            if pf_name_too_long(&io.pfiio_name) {
                return Err(Errno::ENAMETOOLONG);
            }

            pf_lock();
            let error = pfi_clear_flags(&io.pfiio_name, io.pfiio_flags);
            pf_unlock();
            error
        }

        DIOCSETREASS => {
            let reass = ioctl_arg::<i32>(data) as u32;

            net_lock();
            pf_lock();
            PF_TRANS_SET.reass.set(reass);
            PF_TRANS_SET
                .mask
                .set(PF_TRANS_SET.mask.get() | PF_TSET_REASS);
            pf_unlock();
            net_unlock();
            Ok(())
        }

        DIOCSETSYNFLWATS => {
            let io = ioctl_arg::<PfiocSynflwats>(data);

            net_lock();
            pf_lock();
            let error = pf_syncookies_setwats(io.hiwat, io.lowat);
            pf_unlock();
            net_unlock();
            error
        }

        DIOCGETSYNFLWATS => {
            let mut io = ioctl_arg::<PfiocSynflwats>(data);

            net_lock();
            pf_lock();
            let error = pf_syncookies_getwats(&mut io);
            pf_unlock();
            net_unlock();
            ioctl_ret(data, &io);
            error
        }

        DIOCSETSYNCOOKIES => {
            let mode = data.first().copied().unwrap_or(0);

            net_lock();
            pf_lock();
            let error = pf_syncookies_setmode(mode);
            pf_unlock();
            net_unlock();
            error
        }

        _ => Err(Errno::ENODEV),
    }
}

/// The part `DIOCADDRULE` and `DIOCCHANGERULE` share once the rule is numbered: its
/// interfaces, overload table, operands and anchor. `EINVAL` if any of them failed (the rule
/// is then the caller's to remove).
fn pf_rule_setup(
    rule: &'static PfRule,
    ruleset: &'static PfRuleset,
    anchor_call: &[u8],
) -> Result<(), Errno> {
    let mut error = Ok(());

    rule.set_kif(pf_kif_setup(rule.kif()));
    rule.set_rcv_kif(pf_kif_setup(rule.rcv_kif()));
    rule.rdr.set_kif(pf_kif_setup(rule.rdr.kif()));
    rule.nat.set_kif(pf_kif_setup(rule.nat.kif()));
    rule.route.set_kif(pf_kif_setup(rule.route.kif()));

    if rule.overload_tblname[0] != 0 {
        let t = pfr_attach_table(ruleset, &rule.overload_tblname, PR_WAITOK);
        rule.set_overload_tbl(t);
        match t {
            None => error = Err(Errno::EINVAL),
            Some(t) => t
                .pfrkt_flags()
                .set(t.pfrkt_flags().get() | PFR_TFLAG_ACTIVE),
        }
    }

    for aw in [
        &rule.src.addr,
        &rule.dst.addr,
        &rule.rdr.addr,
        &rule.nat.addr,
        &rule.route.addr,
    ] {
        if pf_addr_setup(ruleset, aw, rule.af).is_err() {
            error = Err(Errno::EINVAL);
        }
    }
    if !pf_anchor_setup(rule, ruleset, anchor_call) {
        error = Err(Errno::EINVAL);
    }

    error
}

/// `DIOCKILLSTATES`: removes the state `psk_pfcmp` names, or the states matching the
/// addresses, ports, protocol, label and interface of `psk`; the count in `psk_killed`.
fn pfioctl_killstates(psk: &mut PfiocStateKill) -> Result<(), Errno> {
    let dirs = [PF_IN, PF_OUT];
    let mut killed: u32 = 0;

    if pf_name_too_long(&psk.psk_ifname) || pf_name_too_long(&psk.psk_label) {
        return Err(Errno::ENAMETOOLONG);
    }
    if psk.psk_pfcmp.id != 0 {
        if psk.psk_pfcmp.creatorid == 0 {
            psk.psk_pfcmp.creatorid = PF_STATUS.hostid.get();
        }
        net_lock();
        pf_lock();
        pf_state_enter_write();
        if let Some(st) = pf_find_state_byid(&psk.psk_pfcmp) {
            pf_remove_state(st);
            psk.psk_killed = 1;
        }
        pf_state_exit_write();
        pf_unlock();
        net_unlock();
        return Ok(());
    }

    let src = psk.psk_src.addr.v.get();
    let dst = psk.psk_dst.addr.v.get();

    if psk.psk_af != 0
        && psk.psk_proto != 0
        && psk.psk_src.port_op.get() == PF_OP_EQ
        && psk.psk_dst.port_op.get() == PF_OP_EQ
    {
        let mut key = PfStateKeyCmp {
            af: psk.psk_af,
            proto: psk.psk_proto as u8,
            rdomain: psk.psk_rdomain,
            ..PfStateKeyCmp::default()
        };

        net_lock();
        pf_lock();
        pf_state_enter_write();
        for &dir in &dirs {
            let (sidx, didx) = if dir == PF_IN { (0, 1) } else { (1, 0) };
            pf_addrcpy(&mut key.addr[sidx], &src.addr(), key.af);
            pf_addrcpy(&mut key.addr[didx], &dst.addr(), key.af);
            key.port[sidx] = psk.psk_src.port[0].get();
            key.port[didx] = psk.psk_dst.port[0].get();

            let Some(sk) = PF_STATETBL.find(&PfStateKey::from_cmp(&key)) else {
                continue;
            };

            for si in sk.sk_states.iter() {
                let Some(sist) = si.si_st.get() else {
                    continue;
                };
                let (Some(wire), Some(stack)) =
                    (sist.key[PF_SK_WIRE].get(), sist.key[PF_SK_STACK].get())
                else {
                    continue;
                };
                let same_af = wire.af.get() == stack.af.get();
                let keymatch = (same_af && ptr::eq(sk, if dir == PF_IN { wire } else { stack }))
                    || (!same_af && dir == PF_IN && (ptr::eq(sk, stack) || ptr::eq(sk, wire)));
                if keymatch
                    && (psk.psk_ifname[0] == 0
                        || (!opt_eq(sist.kif.get(), pfi_all())
                            && pf_cstr(&psk.psk_ifname) == kif_name_bytes(sist.kif.get())))
                {
                    pf_remove_state(sist);
                    killed += 1;
                }
            }
        }
        if killed != 0 {
            psk.psk_killed = killed;
        }
        pf_state_exit_write();
        pf_unlock();
        net_unlock();
        return Ok(());
    }

    net_lock();
    pf_lock();
    pf_state_enter_write();
    for st in TREE_ID.iter() {
        let (sk, srcaddr, dstaddr, srcport, dstport) = if st.direction.get() == PF_OUT {
            let Some(sk) = st.key[PF_SK_STACK].get() else {
                continue;
            };
            (
                sk,
                sk.addr[1].get(),
                sk.addr[0].get(),
                sk.port[1].get(),
                sk.port[0].get(),
            )
        } else {
            let Some(sk) = st.key[PF_SK_WIRE].get() else {
                continue;
            };
            (
                sk,
                sk.addr[0].get(),
                sk.addr[1].get(),
                sk.port[0].get(),
                sk.port[1].get(),
            )
        };
        let label_ok = psk.psk_label[0] == 0
            || st
                .rule
                .ptr()
                .is_some_and(|r| r.label[0] != 0 && pf_cstr(&psk.psk_label) == pf_cstr(&r.label));
        if (psk.psk_af == 0 || sk.af.get() == psk.psk_af)
            && (psk.psk_proto == 0 || psk.psk_proto == i32::from(sk.proto.get()))
            && psk.psk_rdomain == sk.rdomain.get()
            && pf_match_addr(
                psk.psk_src.neg.get(),
                &src.addr(),
                &src.mask(),
                &srcaddr,
                sk.af.get(),
            )
            && pf_match_addr(
                psk.psk_dst.neg.get(),
                &dst.addr(),
                &dst.mask(),
                &dstaddr,
                sk.af.get(),
            )
            && (psk.psk_src.port_op.get() == 0
                || pf_match_port(
                    psk.psk_src.port_op.get(),
                    psk.psk_src.port[0].get(),
                    psk.psk_src.port[1].get(),
                    srcport,
                ))
            && (psk.psk_dst.port_op.get() == 0
                || pf_match_port(
                    psk.psk_dst.port_op.get(),
                    psk.psk_dst.port[0].get(),
                    psk.psk_dst.port[1].get(),
                    dstport,
                ))
            && label_ok
            && (psk.psk_ifname[0] == 0 || pf_cstr(&psk.psk_ifname) == kif_name_bytes(st.kif.get()))
        {
            pf_remove_state(st);
            killed += 1;
        }
    }
    psk.psk_killed = killed;
    pf_state_exit_write();
    pf_unlock();
    net_unlock();
    Ok(())
}

/// `DIOCNATLOOK`: the addresses and ports a NATed connection has on the other side.
fn pfioctl_natlook(pnl: &mut PfiocNatlook) -> Result<(), Errno> {
    let direction = pnl.direction;
    let mut m: i32 = 0;

    match pnl.af {
        AF_INET => {}
        #[cfg(feature = "inet6")]
        AF_INET6 => {}
        _ => return Err(Errno::EAFNOSUPPORT),
    }

    // NATLOOK src and dst are reversed, so reverse sidx/didx
    let sidx = if direction == PF_IN { 1 } else { 0 };
    let didx = if direction == PF_IN { 0 } else { 1 };

    let proto = i32::from(pnl.proto);
    if pnl.proto == 0
        || pf_azero(&pnl.saddr, pnl.af)
        || pf_azero(&pnl.daddr, pnl.af)
        || ((proto == IPPROTO_TCP || proto == IPPROTO_UDP) && (pnl.dport == 0 || pnl.sport == 0))
        || u32::from(pnl.rdomain) > RT_TABLEID_MAX
    {
        return Err(Errno::EINVAL);
    }

    let mut key = PfStateKeyCmp {
        af: pnl.af,
        proto: pnl.proto,
        rdomain: pnl.rdomain,
        ..PfStateKeyCmp::default()
    };
    pf_addrcpy(&mut key.addr[sidx], &pnl.saddr, pnl.af);
    key.port[sidx] = pnl.sport;
    pf_addrcpy(&mut key.addr[didx], &pnl.daddr, pnl.af);
    key.port[didx] = pnl.dport;

    net_lock();
    pf_state_enter_read();
    let st = pf_find_state_all(&key, direction, Some(&mut m)).map(pf_state_ref);
    pf_state_exit_read();
    net_unlock();

    let error = if m > 1 {
        Err(Errno::E2BIG) // more than one state
    } else if let Some(sk) = st.and_then(|st| st.key[sidx].get()) {
        let sa = sk.addr[sidx].get();
        let da = sk.addr[didx].get();
        pf_addrcpy(&mut pnl.rsaddr, &sa, sk.af.get());
        pnl.rsport = sk.port[sidx].get();
        pf_addrcpy(&mut pnl.rdaddr, &da, sk.af.get());
        pnl.rdport = sk.port[didx].get();
        pnl.rrdomain = sk.rdomain.get();
        Ok(())
    } else {
        Err(Errno::ENOENT)
    };
    pf_state_unref(st);

    error
}

/// The table ioctls (`DIOCR*`): checks the element size and calls `pf_table.rs`.
fn pfioctl_table(cmd: u64, io: &mut PfiocTable) -> Result<(), Errno> {
    let tsize = size_of::<PfrTable>() as i32;
    let asize = size_of::<PfrAddr>() as i32;
    let flags = io.pfrio_flags | PFR_FLAG_USERIOCTL;

    let esize = match cmd {
        DIOCRCLRTABLES | DIOCRCLRADDRS => 0,
        DIOCRADDTABLES | DIOCRDELTABLES | DIOCRGETTABLES | DIOCRCLRTSTATS | DIOCRSETTFLAGS => tsize,
        DIOCRGETTSTATS => size_of::<PfrTstats>() as i32,
        DIOCRGETASTATS => size_of::<PfrAstats>() as i32,
        _ => asize,
    };
    if io.pfrio_esize != esize {
        return Err(Errno::ENODEV);
    }

    // DIOCRADDTABLES and DIOCRADDADDRS take the locks themselves (they copy in first).
    let locked = !matches!(cmd, DIOCRADDTABLES | DIOCRADDADDRS);
    if locked {
        net_lock();
        pf_lock();
    }
    let buf = io.pfrio_buffer;
    let error = match cmd {
        DIOCRCLRTABLES => pfr_clr_tables(&mut io.pfrio_table, Some(&mut io.pfrio_ndel), flags),
        DIOCRADDTABLES => pfr_add_tables(
            &PfrBuf::User(buf),
            io.pfrio_size,
            Some(&mut io.pfrio_nadd),
            flags,
        ),
        DIOCRDELTABLES => pfr_del_tables(
            &PfrBuf::User(buf),
            io.pfrio_size,
            Some(&mut io.pfrio_ndel),
            flags,
        ),
        DIOCRGETTABLES => pfr_get_tables(
            &mut io.pfrio_table,
            &mut PfrBuf::User(buf),
            &mut io.pfrio_size,
            flags,
        ),
        DIOCRGETTSTATS => pfr_get_tstats(
            &mut io.pfrio_table,
            &mut PfrBuf::User(buf),
            &mut io.pfrio_size,
            flags,
        ),
        // pfrio_nzero
        DIOCRCLRTSTATS => pfr_clr_tstats(
            &PfrBuf::User(buf),
            io.pfrio_size,
            Some(&mut io.pfrio_nadd),
            flags,
        ),
        // pfrio_setflag, pfrio_clrflag
        DIOCRSETTFLAGS => pfr_set_tflags(
            &PfrBuf::User(buf),
            io.pfrio_size,
            io.pfrio_size2,
            io.pfrio_nadd,
            Some(&mut io.pfrio_nchange),
            Some(&mut io.pfrio_ndel),
            flags,
        ),
        DIOCRCLRADDRS => pfr_clr_addrs(&mut io.pfrio_table, Some(&mut io.pfrio_ndel), flags),
        DIOCRADDADDRS => pfr_add_addrs(
            &mut io.pfrio_table,
            &mut PfrBuf::User(buf),
            io.pfrio_size,
            Some(&mut io.pfrio_nadd),
            flags,
        ),
        DIOCRDELADDRS => pfr_del_addrs(
            &mut io.pfrio_table,
            &mut PfrBuf::User(buf),
            io.pfrio_size,
            Some(&mut io.pfrio_ndel),
            flags,
        ),
        DIOCRSETADDRS => pfr_set_addrs(
            &mut io.pfrio_table,
            &mut PfrBuf::User(buf),
            io.pfrio_size,
            Some(&mut io.pfrio_size2),
            Some(&mut io.pfrio_nadd),
            Some(&mut io.pfrio_ndel),
            Some(&mut io.pfrio_nchange),
            flags,
            0,
        ),
        DIOCRGETADDRS => pfr_get_addrs(
            &mut io.pfrio_table,
            &mut PfrBuf::User(buf),
            &mut io.pfrio_size,
            flags,
        ),
        DIOCRGETASTATS => pfr_get_astats(
            &mut io.pfrio_table,
            &mut PfrBuf::User(buf),
            &mut io.pfrio_size,
            flags,
        ),
        // pfrio_nzero
        DIOCRCLRASTATS => pfr_clr_astats(
            &mut io.pfrio_table,
            &mut PfrBuf::User(buf),
            io.pfrio_size,
            Some(&mut io.pfrio_nadd),
            flags,
        ),
        // pfrio_nmatch
        DIOCRTSTADDRS => pfr_tst_addrs(
            &mut io.pfrio_table,
            &mut PfrBuf::User(buf),
            io.pfrio_size,
            Some(&mut io.pfrio_nadd),
            flags,
        ),
        // DIOCRINADEFINE: pfrio_naddr
        _ => pfr_ina_define(
            &mut io.pfrio_table,
            &PfrBuf::User(buf),
            io.pfrio_size,
            Some(&mut io.pfrio_nadd),
            Some(&mut io.pfrio_size2),
            io.pfrio_ticket,
            flags,
        ),
    };
    if locked {
        pf_unlock();
        net_unlock();
    }

    error
}

/// A zeroed `T` on the heap (the transaction elements are a kilobyte).
fn pod_box<T: AbiPod>() -> Box<T> {
    // SAFETY: `T: AbiPod`: every bit pattern, the zero one included, is a valid `T`.
    unsafe { Box::<T>::new_zeroed().assume_init() }
}

/// `copyin(uaddr, v, sizeof(*v))` into an existing heap `T`.
fn pod_copyin<T: AbiPod>(uaddr: usize, v: &mut T) -> Result<(), Errno> {
    // SAFETY: the bytes of `v`, a valid `T` that any bytes keep valid (`AbiPod`); the slice
    // borrows `v` mutably for the copy only.
    let buf =
        unsafe { core::slice::from_raw_parts_mut(ptr::from_mut(v).cast::<u8>(), size_of::<T>()) };
    copyin(uaddr, buf)
}

/// `copyout(v, uaddr, sizeof(*v))` of a heap `T`.
fn pod_copyout<T: AbiPod>(v: &T, uaddr: usize) -> Result<(), Errno> {
    copyout_obj(v, uaddr)
}

/// The `i`th element of a `struct pfioc_trans`'s array, read in (`EFAULT` on a fault,
/// `ENAMETOOLONG` without a NUL in the anchor).
fn pfioc_trans_e_in(io: &PfiocTrans, i: i32, ioe: &mut PfiocTransE) -> Result<(), Errno> {
    let uaddr = io.array + i as usize * size_of::<PfiocTransE>();
    if pod_copyin(uaddr, ioe).is_err() {
        return Err(Errno::EFAULT);
    }
    if libkern::strnlen(&ioe.anchor, ioe.anchor.len()) == ioe.anchor.len() {
        return Err(Errno::ENAMETOOLONG);
    }
    Ok(())
}

/// `DIOCXBEGIN`: opens a transaction on every ruleset and table set of the array.
fn pfioctl_xbegin(io: &PfiocTrans) -> Result<(), Errno> {
    if io.esize as usize != size_of::<PfiocTransE>() {
        return Err(Errno::ENODEV);
    }
    let mut ioe = pod_box::<PfiocTransE>();
    let mut table = pf_abi_zeroed::<PfrTable>();
    net_lock();
    pf_lock();
    pf_default_rule_copy(&PF_DEFAULT_RULE_NEW, &PF_DEFAULT_RULE);
    pf_unlock();
    net_unlock();
    PF_TRANS_SET.clear();
    for i in 0..io.size {
        pfioc_trans_e_in(io, i, &mut ioe)?;
        net_lock();
        pf_lock();
        let error = match ioe.type_ {
            PF_TRANS_TABLE => {
                *table = PfrTable::zeroed();
                pf_strlcpy(&mut table.pfrt_anchor, &ioe.anchor);
                pfr_ina_begin(&table, Some(&mut ioe.ticket), None, 0)
            }
            PF_TRANS_RULESET => pf_begin_rules(&mut ioe.ticket, &ioe.anchor),
            _ => Err(Errno::EINVAL),
        };
        pf_unlock();
        net_unlock();
        error?;
        let uaddr = io.array + i as usize * size_of::<PfiocTransE>();
        if pod_copyout(&*ioe, uaddr).is_err() {
            return Err(Errno::EFAULT);
        }
    }
    Ok(())
}

/// `DIOCXROLLBACK`: abandons the transactions of the array.
fn pfioctl_xrollback(io: &PfiocTrans) -> Result<(), Errno> {
    if io.esize as usize != size_of::<PfiocTransE>() {
        return Err(Errno::ENODEV);
    }
    let mut ioe = pod_box::<PfiocTransE>();
    let mut table = pf_abi_zeroed::<PfrTable>();
    for i in 0..io.size {
        pfioc_trans_e_in(io, i, &mut ioe)?;
        net_lock();
        pf_lock();
        let error = match ioe.type_ {
            PF_TRANS_TABLE => {
                *table = PfrTable::zeroed();
                pf_strlcpy(&mut table.pfrt_anchor, &ioe.anchor);
                pfr_ina_rollback(&table, ioe.ticket, None, 0) // really bad
            }
            PF_TRANS_RULESET => {
                pf_rollback_rules(ioe.ticket, &ioe.anchor);
                Ok(())
            }
            _ => Err(Errno::EINVAL), // really bad
        };
        pf_unlock();
        net_unlock();
        error?;
    }
    Ok(())
}

/// `DIOCXCOMMIT`: checks that every transaction of the array is still open, then commits
/// them all, the staged default timeouts, interface flags and `pf_trans_set`.
fn pfioctl_xcommit(io: &PfiocTrans) -> Result<(), Errno> {
    if io.esize as usize != size_of::<PfiocTransE>() {
        return Err(Errno::ENODEV);
    }
    let mut ioe = pod_box::<PfiocTransE>();
    let mut table = pf_abi_zeroed::<PfrTable>();
    // first makes sure everything will succeed
    for i in 0..io.size {
        pfioc_trans_e_in(io, i, &mut ioe)?;
        net_lock();
        pf_lock();
        let error = match ioe.type_ {
            PF_TRANS_TABLE => {
                let rs = pf_find_ruleset(&ioe.anchor);
                if rs.is_none_or(|rs| rs.topen.get() == 0 || ioe.ticket != rs.tticket.get()) {
                    Err(Errno::EBUSY)
                } else {
                    Ok(())
                }
            }
            PF_TRANS_RULESET => {
                let rs = pf_find_ruleset(&ioe.anchor);
                if rs.is_none_or(|rs| {
                    rs.inactive.open.get() == 0 || rs.inactive.version.get() != ioe.ticket
                }) {
                    Err(Errno::EBUSY)
                } else {
                    Ok(())
                }
            }
            _ => Err(Errno::EINVAL),
        };
        pf_unlock();
        net_unlock();
        error?;
    }
    net_lock();
    pf_lock();

    // now do the commit - no errors should happen here
    for i in 0..io.size {
        pf_unlock();
        net_unlock();
        pfioc_trans_e_in(io, i, &mut ioe)?;
        net_lock();
        pf_lock();
        let error = match ioe.type_ {
            PF_TRANS_TABLE => {
                *table = PfrTable::zeroed();
                pf_strlcpy(&mut table.pfrt_anchor, &ioe.anchor);
                pfr_ina_commit(&table, ioe.ticket, None, None, 0) // really bad
            }
            PF_TRANS_RULESET => pf_commit_rules(ioe.ticket, &ioe.anchor), // really bad
            _ => Err(Errno::EINVAL),                                      // really bad
        };
        if error.is_err() {
            pf_unlock();
            net_unlock();
            return error;
        }
    }
    for i in 0..PFTM_MAX {
        let old = PF_DEFAULT_RULE.timeout(i);

        PF_DEFAULT_RULE.timeout[i].set(PF_DEFAULT_RULE_NEW.timeout(i));
        // The C compares the new value (not the index) with PFTM_INTERVAL; kept.
        if PF_DEFAULT_RULE.timeout(i) == PFTM_INTERVAL as u32
            && PF_DEFAULT_RULE.timeout(i) < old
            && timeout_del(&PF_PURGE_TO)
        {
            task_add(SYSTQMP, &PF_PURGE_TASK);
        }
    }
    pfi_xcommit();
    pf_trans_set_commit();
    pf_unlock();
    net_unlock();
    Ok(())
}

/// `DIOCGETSRCNODES`: copies the source nodes out (as many as `psn_len` bytes hold), or
/// reports the space they need when `psn_len` is 0.
fn pfioctl_getsrcnodes(psn: &mut PfiocSrcNodes) -> Result<(), Errno> {
    let nsize = size_of::<PfSrcNode>();
    let space = psn.psn_len;
    let mut nr: u32 = 0;

    net_lock();
    pf_lock();
    if space == 0 {
        nr = TREE_SRC_TRACKING.iter().count() as u32;
        psn.psn_len = nsize * nr as usize;
        pf_unlock();
        net_unlock();
        return Ok(());
    }

    let mut p = psn.psn_buf;
    for n in TREE_SRC_TRACKING.iter() {
        let secs = getuptime() as i32;

        if (nr as usize + 1) * nsize > psn.psn_len {
            break;
        }

        let mut pstore = pf_abi_clone(n);
        pstore.entry = RbEntry::new();
        pstore.rule.set_ptr(None);
        pstore.kif.set(ptr::null());
        pstore
            .rule
            .set_nr(n.rule.ptr().map_or(u32::MAX, |r| r.nr.get()));
        pstore
            .creation
            .set(secs.wrapping_sub(pstore.creation.get()));
        if pstore.expire.get() > secs {
            pstore.expire.set(pstore.expire.get() - secs);
        } else {
            pstore.expire.set(0);
        }

        // adjust the connection rate estimate
        let diff = secs.wrapping_sub(n.conn_rate.last.get() as i32);
        if diff as u32 >= n.conn_rate.seconds.get() {
            pstore.conn_rate.count.set(0);
        } else {
            pstore
                .conn_rate
                .count
                .set(pstore.conn_rate.count.get().wrapping_sub(
                    n.conn_rate.count.get().wrapping_mul(diff as u32) / n.conn_rate.seconds.get(),
                ));
        }

        if let Err(e) = pf_abi_copyout(&*pstore, p) {
            pf_unlock();
            net_unlock();
            return Err(e);
        }
        p += nsize;
        nr += 1;
    }
    psn.psn_len = nsize * nr as usize;

    pf_unlock();
    net_unlock();
    Ok(())
}

/// `DIOCIGETIFACES`: copies out the interfaces (and groups) matching `pfiio_name`.
fn pfioctl_igetifaces(io: &mut PfiocIface) -> Result<(), Errno> {
    let ksize = size_of::<PfiKif>();
    let apfiio_size = io.pfiio_size;

    if pf_name_too_long(&io.pfiio_name) {
        return Err(Errno::ENAMETOOLONG);
    }
    if io.pfiio_esize as usize != ksize {
        return Err(Errno::ENODEV);
    }

    // A negative count wraps to a huge one, which mallocarray refuses (M_CANFAIL).
    let nmemb = apfiio_size as usize;
    let Some(kif_buf) = mallocarray(nmemb, ksize, M_PF, M_WAITOK | M_CANFAIL) else {
        return Err(Errno::EINVAL);
    };
    // SAFETY: `mallocarray` returned `nmemb * ksize` bytes (the product did not overflow),
    // owned here until the `free` below; bytes need no initialisation to be written.
    let buf = unsafe { core::slice::from_raw_parts_mut(kif_buf.as_ptr(), nmemb * ksize) };
    buf.fill(0);

    net_lock_shared();
    pf_lock();
    pfi_get_ifaces(&io.pfiio_name, buf, &mut io.pfiio_size);
    pf_unlock();
    net_unlock_shared();
    let n = (io.pfiio_size.max(0) as usize).min(nmemb) * ksize;
    let error = if copyout(&buf[..n], io.pfiio_buffer).is_err() {
        Err(Errno::EFAULT)
    } else {
        Ok(())
    };
    free(kif_buf, M_PF, nmemb * ksize);
    error
}

/// `pf_trans_set_commit`: applies the settings `DIOCSETSTATUSIF`, `DIOCSETDEBUG`,
/// `DIOCSETHOSTID` and `DIOCSETREASS` staged.
pub fn pf_trans_set_commit() {
    let mask = PF_TRANS_SET.mask.get();
    if mask & PF_TSET_STATUSIF != 0 {
        let mut ifname = [0u8; IFNAMSIZ];
        pf_strlcpy(&mut ifname, &PF_TRANS_SET.statusif.get());
        PF_STATUS.ifname.set(ifname);
    }
    if mask & PF_TSET_DEBUG != 0 {
        PF_STATUS.debug.set(PF_TRANS_SET.debug.get());
    }
    if mask & PF_TSET_HOSTID != 0 {
        PF_STATUS.hostid.set(PF_TRANS_SET.hostid.get());
    }
    if mask & PF_TSET_REASS != 0 {
        PF_STATUS.reass.set(PF_TRANS_SET.reass.get());
    }
}

/// `pf_pool_copyin`: a pool of a new rule from the user's copy, without its kernel pointers
/// (`kif`, `addr.p.tbl`).
pub fn pf_pool_copyin(from: &PfPool, to: &mut PfPool) {
    to.addr = from.addr.copy_scalars();
    to.key.set(from.key.get());
    to.counter.set(from.counter.get());
    to.ifname.set(from.ifname.get());
    to.kif.set(ptr::null());
    to.tblidx.set(from.tblidx.get());
    to.states.set(from.states.get());
    to.curweight.set(from.curweight.get());
    to.weight.set(from.weight.get());
    to.proxy_port[0].set(from.proxy_port[0].get());
    to.proxy_port[1].set(from.proxy_port[1].get());
    to.port_op.set(from.port_op.get());
    to.opts.set(from.opts.get());
}

/// `pf_validate_range`: whether the port range `port` (in `order`) is empty under `op`:
/// `true` where the C returns 1.
pub fn pf_validate_range(op: u8, port: [u16; 2], order: i32) -> bool {
    let a = if order == PF_ORDER_NET {
        u16::from_be(port[0])
    } else {
        port[0]
    };
    let b = if order == PF_ORDER_NET {
        u16::from_be(port[1])
    } else {
        port[1]
    };

    (op == PF_OP_RRG && a > b)  // 34:12,  i.e. none
        || (op == PF_OP_IRG && a >= b) // 34><12, i.e. none
        || (op == PF_OP_XRG && a > b) // 34<>22, i.e. all
}

/// `pf_chk_limiter_action`: whether `limiter_action` is not one pf knows (the C's 1).
pub fn pf_chk_limiter_action(limiter_action: i32) -> bool {
    !matches!(limiter_action, PF_LIMITER_NOMATCH | PF_LIMITER_BLOCK)
}

/// `pf_rule_copyin`: fills the new kernel rule `to` from the user's `from`, member by member
/// (never the pointers), allocating its interfaces and resolving its tags and queues.
pub fn pf_rule_copyin(from: &PfRule, to: &mut PfRule) -> Result<(), Errno> {
    if from.scrub_flags & PFSTATE_SETPRIO != 0
        && (u32::from(from.set_prio[0]) > IFQ_MAXPRIO || u32::from(from.set_prio[1]) > IFQ_MAXPRIO)
    {
        return Err(Errno::EINVAL);
    }

    to.src = from.src.copy_scalars();
    to.dst = from.dst.copy_scalars();

    if pf_validate_range(
        to.src.port_op.get(),
        [to.src.port[0].get(), to.src.port[1].get()],
        PF_ORDER_NET,
    ) {
        return Err(Errno::EINVAL);
    }
    if pf_validate_range(
        to.dst.port_op.get(),
        [to.dst.port[0].get(), to.dst.port[1].get()],
        PF_ORDER_NET,
    ) {
        return Err(Errno::EINVAL);
    }

    // XXX union skip[]

    if pf_name_too_long(&from.label)
        || pf_name_too_long(&from.ifname)
        || pf_name_too_long(&from.rcv_ifname)
        || pf_name_too_long(&from.qname)
        || pf_name_too_long(&from.pqname)
        || pf_name_too_long(&from.tagname)
        || pf_name_too_long(&from.match_tagname)
        || pf_name_too_long(&from.overload_tblname)
    {
        return Err(Errno::ENAMETOOLONG);
    }

    pf_strlcpy(&mut to.label, &from.label);
    pf_strlcpy(&mut to.ifname, &from.ifname);
    pf_strlcpy(&mut to.rcv_ifname, &from.rcv_ifname);
    pf_strlcpy(&mut to.qname, &from.qname);
    pf_strlcpy(&mut to.pqname, &from.pqname);
    pf_strlcpy(&mut to.tagname, &from.tagname);
    pf_strlcpy(&mut to.match_tagname, &from.match_tagname);
    pf_strlcpy(&mut to.overload_tblname, &from.overload_tblname);

    pf_pool_copyin(&from.nat, &mut to.nat);
    pf_pool_copyin(&from.rdr, &mut to.rdr);
    pf_pool_copyin(&from.route, &mut to.route);

    if pf_validate_range(
        to.rdr.port_op.get(),
        [to.rdr.proxy_port[0].get(), to.rdr.proxy_port[1].get()],
        PF_ORDER_HOST,
    ) {
        return Err(Errno::EINVAL);
    }

    let kif_alloc = |name: &[u8]| {
        if name[0] != 0 {
            pfi_kif_alloc(name, M_WAITOK)
        } else {
            None
        }
    };
    to.set_kif(kif_alloc(&to.ifname));
    to.set_rcv_kif(kif_alloc(&to.rcv_ifname));
    to.rdr.set_kif(kif_alloc(&to.rdr.ifname.get()));
    to.nat.set_kif(kif_alloc(&to.nat.ifname.get()));
    to.route.set_kif(kif_alloc(&to.route.ifname.get()));

    to.os_fingerprint = from.os_fingerprint;

    to.rtableid = from.rtableid;
    if to.rtableid >= 0 && !rtable_exists(to.rtableid as u32) {
        return Err(Errno::EBUSY);
    }
    to.onrdomain = from.onrdomain;
    if to.onrdomain != -1 && (to.onrdomain < 0 || to.onrdomain as u32 > RT_TABLEID_MAX) {
        return Err(Errno::EINVAL);
    }

    for i in 0..PFTM_MAX {
        to.timeout[i].set(from.timeout[i].get());
    }
    to.states_tot.set(from.states_tot.get());
    to.max_states = from.max_states;
    to.max_src_nodes = from.max_src_nodes;
    to.max_src_states = from.max_src_states;
    to.max_src_conn = from.max_src_conn;
    to.max_src_conn_rate.limit = from.max_src_conn_rate.limit;
    to.max_src_conn_rate.seconds = from.max_src_conn_rate.seconds;
    pf_init_threshold(
        &to.pktrate,
        from.pktrate.limit.get(),
        from.pktrate.seconds.get(),
    );

    if to.qname[0] != 0 {
        to.qid = u32::from(pf_qname2qid(&to.qname, false));
        if to.qid == 0 {
            return Err(Errno::EBUSY);
        }
        if to.pqname[0] != 0 {
            to.pqid = u32::from(pf_qname2qid(&to.pqname, false));
            if to.pqid == 0 {
                return Err(Errno::EBUSY);
            }
        } else {
            to.pqid = to.qid;
        }
    }
    to.rt_listid = from.rt_listid;
    to.prob = from.prob;
    to.return_icmp = from.return_icmp;
    to.return_icmp6 = from.return_icmp6;
    to.max_mss = from.max_mss;
    if to.tagname[0] != 0 {
        to.tag = pf_tagname2tag(&to.tagname, true);
        if to.tag == 0 {
            return Err(Errno::EBUSY);
        }
    }
    if to.match_tagname[0] != 0 {
        to.match_tag = pf_tagname2tag(&to.match_tagname, true);
        if to.match_tag == 0 {
            return Err(Errno::EBUSY);
        }
    }
    to.scrub_flags = from.scrub_flags;
    to.delay = from.delay;
    to.uid = from.uid;
    to.gid = from.gid;
    to.rule_flag.set(from.rule_flag.get());
    to.action = from.action;
    to.direction = from.direction;
    to.log = from.log;
    to.logif = from.logif;
    // NPFLOG > 0
    if to.log == 0 {
        to.logif = 0;
    }
    to.quick = from.quick;
    to.ifnot = from.ifnot;
    to.rcvifnot = from.rcvifnot;
    to.match_tag_not = from.match_tag_not;
    to.keep_state = from.keep_state;
    to.af = from.af;
    to.naf = from.naf;
    to.proto = from.proto;
    to.type_ = from.type_;
    to.code = from.code;
    to.flags = from.flags;
    to.flagset = from.flagset;
    to.min_ttl = from.min_ttl;
    to.allow_opts = from.allow_opts;
    to.rt = from.rt;
    to.return_ttl = from.return_ttl;
    to.tos = from.tos;
    to.set_tos = from.set_tos;
    to.anchor_relative.set(from.anchor_relative.get()); // XXX
    to.anchor_wildcard.set(from.anchor_wildcard.get()); // XXX
    to.flush = from.flush;
    to.divert.addr = from.divert.addr;
    to.divert.port = from.divert.port;
    to.divert.type_ = from.divert.type_;
    to.prio = from.prio;
    to.set_prio[0] = from.set_prio[0];
    to.set_prio[1] = from.set_prio[1];
    to.statelim = from.statelim;
    to.sourcelim = from.sourcelim;

    if pf_chk_limiter_action(to.statelim.limiter_action)
        || pf_chk_limiter_action(to.sourcelim.limiter_action)
    {
        return Err(Errno::EINVAL);
    }

    Ok(())
}

/// `pf_rule_checkaf`: the rule's address families are ones pf supports (`af-to` only from
/// one family to the other).
pub fn pf_rule_checkaf(r: &PfRule) -> Result<(), Errno> {
    let afto = r.rule_flag.get() & PFRULE_AFTO != 0;
    match r.af {
        0 => {
            if afto {
                return Err(Errno::EPFNOSUPPORT);
            }
        }
        AF_INET => {
            if afto && r.naf != AF_INET6 {
                return Err(Errno::EPFNOSUPPORT);
            }
        }
        #[cfg(feature = "inet6")]
        AF_INET6 => {
            if afto && r.naf != AF_INET {
                return Err(Errno::EPFNOSUPPORT);
            }
        }
        _ => return Err(Errno::EPFNOSUPPORT),
    }

    if !afto && r.naf != 0 {
        return Err(Errno::EPFNOSUPPORT);
    }

    Ok(())
}

/// `pf_sysctl`: `kern.pfstatus`, the status as `DIOCGETSTATUS` reports it.
pub fn pf_sysctl(
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    _newlen: usize,
) -> Result<(), Errno> {
    let mut pfs = pf_abi_zeroed::<PfStatus>();

    if pf_status_read(&mut pfs).is_err() {
        return Err(Errno::ENOENT);
    }
    sysctl_rdstruct(oldp, oldlenp, newp, pf_abi_bytes(&*pfs))
}

/// `pf_open_trans`: a new transaction of clone `unit`; `None` when the clone has too many.
pub fn pf_open_trans(unit: u32) -> Option<&'static PfTrans> {
    /// `ticket`: the static counter of the C's function.
    static TICKET: PfGlobal<Cell<u64>> = PfGlobal(Cell::new(1));

    rw_assert_wrlock(&PFIOCTL_RW);

    let idx = pf_unit2idx(unit);
    kassert!(idx < PF_TCOUNT.len());
    if PF_TCOUNT[idx].get() >= (PF_ANCHOR_STACK_MAX * 8) as u32 {
        return None;
    }

    let mem = malloc(size_of::<PfTrans>(), M_PF, M_WAITOK | M_ZERO)?;
    // SAFETY: `size_of::<PfTrans>()` zeroed bytes, aligned for any object (`malloc(9)`); all
    // zero is a valid `PfTrans` (a list link, `Cell`s of integers and of `Option`s of
    // references). The transaction lives until `pf_free_trans` frees it.
    let t: &'static PfTrans = unsafe { &*mem.as_ptr().cast::<PfTrans>() };
    t.pft_unit.set(unit);
    t.pft_ticket.set(TICKET.get());
    TICKET.set(TICKET.get() + 1);
    PF_TCOUNT[idx].set(PF_TCOUNT[idx].get() + 1);

    // SAFETY: a fresh transaction, on no list; the list is pfioctl_rw's, held.
    unsafe { PF_IOCTL_TRANS.insert_head(t) };

    Some(t)
}

/// `pf_find_trans`: the transaction `ticket` of clone `unit`.
pub fn pf_find_trans(unit: u32, ticket: u64) -> Option<&'static PfTrans> {
    rw_assert_anylock(&PFIOCTL_RW);

    PF_IOCTL_TRANS
        .iter()
        .find(|t| t.pft_ticket.get() == ticket && t.pft_unit.get() == unit)
}

/// `pf_init_tgetrule`: makes `t` a `DIOCGETRULE` walk over the rules of anchor `a` (the main
/// ruleset for none), version `rs_version`, from rule `r`.
pub fn pf_init_tgetrule(
    t: &PfTrans,
    a: Option<&'static PfAnchor>,
    rs_version: u32,
    r: Option<&'static PfRule>,
) {
    t.pft_type.set(PF_TRANS_GETRULE);
    match a {
        None => t.pftgr_anchor.set(Some(&PF_MAIN_ANCHOR)),
        Some(a) => t.pftgr_anchor.set(Some(a)),
    }

    t.pftgr_version.set(rs_version);
    t.pftgr_rule.set(r);
}

/// `pf_cleanup_tgetrule`: drops the anchor reference of a `DIOCGETRULE` walk.
pub fn pf_cleanup_tgetrule(t: &PfTrans) {
    kassert!(t.pft_type.get() == PF_TRANS_GETRULE);
    pf_anchor_rele(t.pftgr_anchor.get());
}

/// `pf_free_trans`: frees a transaction that is on no list.
pub fn pf_free_trans(t: &'static PfTrans) {
    match t.pft_type.get() {
        PF_TRANS_GETRULE => pf_cleanup_tgetrule(t),
        other => log(
            LOG_ERR,
            format_args!("pf_free_trans unknown transaction type: {}\n", other),
        ),
    }

    let idx = pf_unit2idx(t.pft_unit.get());
    kassert!(idx < PF_TCOUNT.len());
    kassert!(PF_TCOUNT[idx].get() >= 1);
    PF_TCOUNT[idx].set(PF_TCOUNT[idx].get() - 1);

    free(NonNull::from(t).cast::<u8>(), M_PF, size_of::<PfTrans>());
}

/// `pf_rollback_trans`: unlinks and frees transaction `t`.
pub fn pf_rollback_trans(t: Option<&'static PfTrans>) {
    if let Some(t) = t {
        rw_assert_wrlock(&PFIOCTL_RW);
        // SAFETY: an open transaction is on `pf_ioctl_trans` (pfioctl_rw is held).
        unsafe { ListHead::<PfTransList>::remove(t) };
        pf_free_trans(t);
    }
}

// NKSTAT: the state and source limiter kstats (`struct pf_statelim_kstat`,
// `pf_statelim_kstat_read`, `pf_statelim_kstat_attach`, `pf_statelim_kstat_detach`, `struct
// pf_sourcelim_kstat`, `pf_sourcelim_kstat_read`, `pf_sourcelim_kstat_attach`,
// `pf_sourcelim_kstat_detach`: a "pf" "state-limiter"/"source-limiter" kstat per limiter with
// its name, in-use, limit and admitted/hard-limited/rate-limited counters, read under the
// limiter's pc lock) are not configured.

// `#if (PF_QNAME_SIZE != PF_TAG_NAME_SIZE)`: the queue names share the tag name functions.
const _: () = assert!(PF_QNAME_SIZE == PF_TAG_NAME_SIZE);
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for `/dev/pf`: starting and stopping pf, a rule set transaction that installs a
    // `block in quick` rule and reads it back, the default timeouts and pool limits, tag names,
    // and the open and write-permission checks.

    use std::boxed::Box;
    use std::sync::MutexGuard;
    use std::vec;
    use std::vec::Vec;

    use super::*;
    use crate::kern::kern_proc::procinit;
    use crate::kern::kern_prot::{crget, crhold};
    use crate::net::if_::tests::setup_net;
    use crate::sys::fcntl::FREAD;
    use crate::sys::proc::Process;
    use crate::sys::ucred::Ucred;

    /// The network test lock with fresh memory, the process tables, and pf attached (every test
    /// resets the memory the pools draw from, so pf is attached again over it).
    fn setup() -> MutexGuard<'static, ()> {
        let guard = setup_net();
        procinit();
        crate::kern::kern_timeout::timeout_startup();
        pfattach(1);
        guard
    }

    /// A thread of a process with credentials, as `fork1` would leave it.
    fn thread() -> &'static Proc {
        let cr: &'static Ucred = crget();
        cr.cr_ruid.set(1000);
        let pr: &'static Process = Box::leak(Box::new(Process::new()));
        let p: &'static Proc = Box::leak(Box::new(Proc::new()));
        p.p_p.set(pr);
        p.p_ucred.set(cr);
        pr.ps_ucred.set(crhold(cr));
        pr.ps_mainproc.set(p);
        pr.ps_pid.set(42);
        p
    }

    /// The minor of the first clone of the device.
    const DEV: Dev = 0;

    /// `sys_ioctl`'s kernel copy of an `AbiPod` argument.
    fn pod<T: AbiPod>(v: &T) -> Vec<u8> {
        let mut data = vec![0u8; size_of::<T>()];
        ioctl_ret(&mut data, v);
        data
    }

    /// `sys_ioctl`'s kernel copy of a `PfAbi` argument.
    fn abi<T: PfAbi>(v: &T) -> Vec<u8> {
        pf_abi_bytes(v).to_vec()
    }

    fn ioctl(cmd: u64, data: &mut [u8], p: &Proc) -> Result<(), Errno> {
        pfioctl(DEV, cmd, data, FREAD | FWRITE, p)
    }

    fn status(p: &Proc) -> Box<PfStatus> {
        let mut data = abi(&*pf_abi_zeroed::<PfStatus>());
        assert_eq!(ioctl(DIOCGETSTATUS, &mut data, p), Ok(()));
        pf_abi_read::<PfStatus>(&data)
    }

    #[test]
    fn start_and_stop() {
        let _g = setup();
        let p = thread();

        assert_eq!(status(p).running.get(), 0);
        assert_eq!(ioctl(DIOCSTART, &mut [], p), Ok(()));
        let s = status(p);
        assert_eq!(s.running.get(), 1);
        assert_eq!(ioctl(DIOCSTART, &mut [], p), Err(Errno::EEXIST));

        assert_eq!(ioctl(DIOCSTOP, &mut [], p), Ok(()));
        assert_eq!(status(p).running.get(), 0);
        assert_eq!(ioctl(DIOCSTOP, &mut [], p), Err(Errno::ENOENT));

        // The purge timeouts DIOCSTART armed would outlive the test's timeout wheel.
        timeout_del(&PF_PURGE_STATES_TO);
        timeout_del(&PF_PURGE_TO);
    }

    /// A `pfioc_trans` of one element for the main ruleset; the element is returned so that it
    /// stays where `array` points.
    fn trans_main() -> (Box<PfiocTransE>, PfiocTrans) {
        let mut e = pod_box::<PfiocTransE>();
        e.type_ = PF_TRANS_RULESET;
        let io = PfiocTrans {
            size: 1,
            esize: size_of::<PfiocTransE>() as i32,
            array: ptr::from_mut(&mut *e) as usize,
        };
        (e, io)
    }

    #[test]
    fn a_committed_rule_is_read_back() {
        let _g = setup();
        let p = thread();

        // DIOCXBEGIN: the ticket of the main ruleset.
        let (e, io) = trans_main();
        let mut data = pod(&io);
        assert_eq!(ioctl(DIOCXBEGIN, &mut data, p), Ok(()));
        let ticket = e.ticket;
        assert_eq!(ticket, pf_main_ruleset().inactive.version.get());

        // DIOCADDRULE: block in quick.
        let mut pr = pf_abi_zeroed::<PfiocRule>();
        pr.ticket = ticket;
        pr.rule.action = PF_DROP;
        pr.rule.direction = PF_IN;
        pr.rule.quick = 1;
        pr.rule.rtableid = -1; // pfctl's "no rtable"
        pr.rule.onrdomain = -1;
        pr.rule.src.addr.type_.set(PF_ADDR_ADDRMASK);
        pr.rule.dst.addr.type_.set(PF_ADDR_ADDRMASK);
        let mut data = abi(&*pr);
        assert_eq!(ioctl(DIOCADDRULE, &mut data, p), Ok(()));
        assert_eq!(pf_main_ruleset().inactive.rcount.get(), 1);

        // A wrong ticket is refused.
        pr.ticket = ticket.wrapping_add(1);
        let mut data = abi(&*pr);
        assert_eq!(ioctl(DIOCADDRULE, &mut data, p), Err(Errno::EBUSY));

        // DIOCXCOMMIT: the rule becomes active, with its checksum.
        let mut data = pod(&io);
        assert_eq!(ioctl(DIOCXCOMMIT, &mut data, p), Ok(()));
        assert_eq!(pf_main_ruleset().active.rcount.get(), 1);
        assert_eq!(pf_main_ruleset().inactive.rcount.get(), 0);
        assert_ne!(PF_STATUS.pf_chksum.get(), [0; PF_MD5_DIGEST_LENGTH]);

        // DIOCGETRULES: one rule, and a ticket to walk them.
        let pr = pf_abi_zeroed::<PfiocRule>();
        let mut data = abi(&*pr);
        assert_eq!(ioctl(DIOCGETRULES, &mut data, p), Ok(()));
        let pr = pf_abi_read::<PfiocRule>(&data);
        assert_eq!(pr.nr, 1);
        let walk = pr.ticket;

        // DIOCGETRULE: the rule, without kernel pointers.
        let mut data = abi(&*pr);
        assert_eq!(ioctl(DIOCGETRULE, &mut data, p), Ok(()));
        let got = pf_abi_read::<PfiocRule>(&data);
        assert_eq!(got.nr, 0);
        assert_eq!(got.rule.action, PF_DROP);
        assert_eq!(got.rule.direction, PF_IN);
        assert_eq!(got.rule.quick, 1);
        assert_eq!(got.rule.cuid, 1000);
        assert_eq!(got.rule.cpid, 42);
        assert!(got.rule.kif.get().is_null());
        assert!(got.rule.overload_tbl.get().is_null());
        for s in &got.rule.skip {
            assert_eq!(s.nr(), u32::MAX, "the only rule skips to the end");
        }

        // The walk is over; DIOCXEND closes it.
        let mut data = abi(&*pr);
        assert_eq!(ioctl(DIOCGETRULE, &mut data, p), Err(Errno::ENOENT));
        let mut data = pod(&(walk as i32));
        assert_eq!(ioctl(DIOCXEND, &mut data, p), Ok(()));
        assert_eq!(ioctl(DIOCXEND, &mut data, p), Err(Errno::ENXIO));

        // A new transaction with no rule empties the active set.
        let (_e, io) = trans_main();
        let mut data = pod(&io);
        assert_eq!(ioctl(DIOCXBEGIN, &mut data, p), Ok(()));
        let mut data = pod(&io);
        assert_eq!(ioctl(DIOCXCOMMIT, &mut data, p), Ok(()));
        assert!(pf_main_ruleset().active_ptr().is_empty());
    }

    #[test]
    fn timeouts_are_staged_until_the_commit() {
        let _g = setup();
        let p = thread();

        let get = |t: usize| {
            let mut data = pod(&PfiocTm {
                timeout: t as i32,
                seconds: 0,
            });
            assert_eq!(ioctl(DIOCGETTIMEOUT, &mut data, p), Ok(()));
            ioctl_arg::<PfiocTm>(&data).seconds
        };
        assert_eq!(get(PFTM_TCP_ESTABLISHED), PFTM_TCP_ESTABLISHED_VAL as i32);

        let (_e, io) = trans_main();
        let mut data = pod(&io);
        assert_eq!(ioctl(DIOCXBEGIN, &mut data, p), Ok(()));

        // DIOCSETTIMEOUT returns the value in force.
        let mut data = pod(&PfiocTm {
            timeout: PFTM_TCP_ESTABLISHED as i32,
            seconds: 3600,
        });
        assert_eq!(ioctl(DIOCSETTIMEOUT, &mut data, p), Ok(()));
        assert_eq!(
            ioctl_arg::<PfiocTm>(&data).seconds,
            PFTM_TCP_ESTABLISHED_VAL as i32
        );
        assert_eq!(get(PFTM_TCP_ESTABLISHED), PFTM_TCP_ESTABLISHED_VAL as i32);

        let mut data = pod(&io);
        assert_eq!(ioctl(DIOCXCOMMIT, &mut data, p), Ok(()));
        assert_eq!(get(PFTM_TCP_ESTABLISHED), 3600);

        // Out of range.
        let mut data = pod(&PfiocTm {
            timeout: PFTM_MAX as i32,
            seconds: 1,
        });
        assert_eq!(ioctl(DIOCSETTIMEOUT, &mut data, p), Err(Errno::EINVAL));
        assert_eq!(ioctl(DIOCGETTIMEOUT, &mut data, p), Err(Errno::EINVAL));
    }

    #[test]
    fn pool_limits() {
        let _g = setup();
        let p = thread();

        let get = |i: usize| {
            let mut data = pod(&PfiocLimit {
                index: i as i32,
                limit: 0,
            });
            assert_eq!(ioctl(DIOCGETLIMIT, &mut data, p), Ok(()));
            ioctl_arg::<PfiocLimit>(&data).limit
        };
        assert_eq!(get(PF_LIMIT_STATES), PFSTATE_HIWAT);

        let mut data = pod(&PfiocLimit {
            index: PF_LIMIT_STATES as i32,
            limit: 5000,
        });
        assert_eq!(ioctl(DIOCSETLIMIT, &mut data, p), Ok(()));
        assert_eq!(get(PF_LIMIT_STATES), 5000);
        assert_eq!(PF_POOL_LIMITS[PF_LIMIT_STATES].limit_new.get(), 5000);

        let mut data = pod(&PfiocLimit {
            index: PF_LIMIT_MAX as i32,
            limit: 1,
        });
        assert_eq!(ioctl(DIOCSETLIMIT, &mut data, p), Err(Errno::EINVAL));
        assert_eq!(ioctl(DIOCGETLIMIT, &mut data, p), Err(Errno::EINVAL));

        // Restore the default for the other tests of this process.
        let mut data = pod(&PfiocLimit {
            index: PF_LIMIT_STATES as i32,
            limit: PFSTATE_HIWAT,
        });
        assert_eq!(ioctl(DIOCSETLIMIT, &mut data, p), Ok(()));
    }

    #[test]
    fn tag_names_round_trip() {
        let _g = setup();

        pf_lock();
        let a = pf_tagname2tag(b"pftest-a\0", true);
        let b = pf_tagname2tag(b"pftest-b\0", true);
        assert_ne!(a, 0);
        assert_ne!(b, 0);
        assert_ne!(a, b);
        assert_eq!(
            pf_tagname2tag(b"pftest-a\0", false),
            a,
            "found, referenced again"
        );
        assert_eq!(pf_tagname2tag(b"pftest-none\0", false), 0);

        let mut name = [0u8; PF_TAG_NAME_SIZE];
        pf_tag2tagname(b, &mut name);
        assert_eq!(pf_cstr(&name), b"pftest-b");

        // Two references on a, one on b: the names go with the last.
        pf_tag_unref(a);
        pf_tag_unref(b);
        assert_eq!(pf_tagname2tag(b"pftest-b\0", false), 0);
        pf_tag_ref(a);
        pf_tag_unref(a);
        assert_eq!(pf_tagname2tag(b"pftest-a\0", false), a);
        pf_tag_unref(a);
        pf_tag_unref(a);
        assert_eq!(pf_tagname2tag(b"pftest-a\0", false), 0);

        // The lowest free number is reused.
        let c = pf_tagname2tag(b"pftest-c\0", true);
        assert!(c <= a.min(b), "a freed slot is taken first");
        pf_tag_unref(c);
        pf_unlock();
    }

    #[test]
    fn open_and_permissions() {
        let _g = setup();
        let p = thread();

        assert_eq!(pfopen(0, 0, 0, p), Ok(()));
        assert_eq!(
            pfopen(1, 0, 0, p),
            Err(Errno::ENXIO),
            "not a clone's first minor"
        );
        assert_eq!(pfopen(1 << CLONE_SHIFT, 0, 0, p), Ok(()));

        // Read-only: reading is allowed, changing is not.
        let mut data = abi(&*pf_abi_zeroed::<PfStatus>());
        assert_eq!(pfioctl(DEV, DIOCGETSTATUS, &mut data, FREAD, p), Ok(()));
        assert_eq!(
            pfioctl(DEV, DIOCSTART, &mut [], FREAD, p),
            Err(Errno::EACCES)
        );

        // Unknown commands.
        assert_eq!(ioctl(0, &mut [], p), Err(Errno::ENODEV), "not a pf command");

        // DIOCADDSTATE (pfsync is configured): a state without a creator id is refused, one
        // with a timeout past PFTM_MAX too.
        let mut ps = PfiocState::default();
        assert_eq!(ioctl(DIOCADDSTATE, &mut pod(&ps), p), Err(Errno::EINVAL));
        ps.state.timeout = PFTM_MAX as u8;
        ps.state.creatorid = 1;
        assert_eq!(ioctl(DIOCADDSTATE, &mut pod(&ps), p), Err(Errno::EINVAL));

        assert_eq!(pfclose(DEV, 0, 0, Some(p)), Ok(()));
    }

    #[test]
    fn the_rule_checks() {
        let r = PfRule::zeroed();
        assert_eq!(pf_rule_checkaf(&r), Ok(()));
        r.rule_flag.set(PFRULE_AFTO);
        assert_eq!(pf_rule_checkaf(&r), Err(Errno::EPFNOSUPPORT));

        assert!(!pf_validate_range(
            PF_OP_RRG,
            [12u16.to_be(), 34u16.to_be()],
            PF_ORDER_NET
        ));
        assert!(pf_validate_range(
            PF_OP_RRG,
            [34u16.to_be(), 12u16.to_be()],
            PF_ORDER_NET
        ));
        assert!(pf_validate_range(PF_OP_IRG, [12, 12], PF_ORDER_HOST));
        assert!(!pf_chk_limiter_action(PF_LIMITER_BLOCK));
        assert!(pf_chk_limiter_action(7));
    }
}
/* </TESTS> */
