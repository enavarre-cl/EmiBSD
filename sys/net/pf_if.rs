/*	$OpenBSD: pf_if.c,v 1.114 2026/09/08 18:42:14 bluhm Exp $ */
/* <LICENSES> */

/*
 * Copyright 2005 Henning Brauer <henning@openbsd.org>
 * Copyright 2005 Ryan McBride <mcbride@openbsd.org>
 * Copyright (c) 2001 Daniel Hartmeier
 * Copyright (c) 2003 Cedric Berger
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
 */
/* </LICENSES> */

//! pf's interface layer (`net/pf_if.c`): the `pfi_kif`s, pf's view of every interface and
//! interface group, kept in the `pfi_ifs` tree by name; the dynamic addresses (`(ifname)`
//! operands) and the tables that hold them; and the `set skip` flags.
//!
//! Upstream: sys/net/pf_if.c @ 3ce1f3f79392
//!
//! A kif exists for every attached interface (`pfi_attach_ifnet`) and group
//! (`pfi_attach_ifgroup`), and for every name a rule, state, route, source node or `set skip`
//! refers to, attached or not. The references are counted per kind (`pfi_kif_ref`); a kif
//! that is neither attached nor referenced is freed by the `pfi_kif_unref` that drops its
//! last reference. Kifs are `malloc(9)`'d (`M_PF`), as in C; a published kif is a
//! `&'static PfiKif` until that free.
//!
//! An interface's `if_pf_kif` and a group's `ifg_pf_kif` hold their kif's address (a raw
//! word, as in C); the kif points back through `pfik_ifp`/`pfik_group`. The interface's
//! address hook (`if_addrhook_add`) runs `pfi_kifaddr_update`, which brings the dynamic
//! addresses of the kif and of its groups up to date.
//!
//! ## Deviations
//! - `struct pfi_kif_cmp` (the name-only key the C casts to a `struct pfi_kif` for
//!   `RB_FIND`) is a zeroed `PfiKif` on the stack holding the name (`pfi_kif_key`).
//! - The C dereferences NULL in a few places that cannot be reached when pf is attached to
//!   every interface and group: a rule kif with a group matched against a packet kif without
//!   an interface (`pfi_kif_match`), an interface or group whose `if_pf_kif`/`ifg_pf_kif` is
//!   NULL (`pfi_kif_update`, `pfi_update_status`), a dynamic address with more than one
//!   address but no table (`pfi_match_addr`), and a NULL broadcast or peer address
//!   (`pfi_address_add`). Here they skip the object (or do not match) instead of faulting.
//! - `pfi_buffer`, `pfi_buffer_cnt` and `pfi_buffer_max` are the members of one static,
//!   `PFI_BUFFER` (a static `PFI_BUFFER_MAX` would collide with the C macro of that name).
//!   The buffer is a raw `malloc(9)`'d array whose kernel address is handed to
//!   `pfr_set_addrs` without `PFR_FLAG_USERIOCTL`, as in C.
//! - `pfi_get_ifaces` copies the kifs into the caller's kernel buffer (byte slices of
//!   `sizeof(struct pfi_kif)`), as in C; `pf_ioctl.c`'s `DIOCIGETIFACES` copies that out.
//! - `pfi_dynaddr_copyout` takes the operand of a user-bound copy of a rule, whose `p` still
//!   holds the kernel's `pfi_dynaddr` address (`DIOCGETRULE` copies the rule byte for byte),
//!   and is `unsafe` for that reason.
//! - C functions returning `int` 0/1 (not an errno) return `bool`: `pfi_kif_match`,
//!   `pfi_match_addr` and `pfi_skip_if` return `true` for 1; `pfi_dynaddr_setup` returns
//!   `true` for the C's 0 (success), `false` for 1 (`docs/C_TO_RUST.md`: 0/-1 status).

use core::cell::Cell;
use core::cmp::Ordering;
use core::ffi::c_void;
use core::ptr::{self, NonNull};

use crate::kassert;
use crate::kern::kern_malloc::{free, malloc, mallocarray};
use crate::kern::kern_tc::gettime;
use crate::kern::subr_prf::{Str, panic, snprintf};
use crate::machine::intr::IPL_SOFTNET;
use crate::net::if_::{
    IFF_BROADCAST, IFF_LOOPBACK, IFF_POINTOPOINT, IFG_ALL, IFNAMSIZ, if_addrhook_add,
    if_addrhook_del,
};
use crate::net::if_var::{IfgGroup, Ifnet};
use crate::net::pf::pf_match_addr;
use crate::net::pf_ruleset::{pf_find_or_create_ruleset, pf_remove_if_empty_ruleset};
use crate::net::pf_table::{
    PfrBuf, pfr_attach_table, pfr_detach_table, pfr_dynaddr_update, pfr_match_addr, pfr_set_addrs,
};
use crate::net::pfvar::{
    PF_ADDR_DYNIFTL, PF_RESERVED_ANCHOR, PF_TABLE_NAME_SIZE, PFI_AFLAG_BROADCAST,
    PFI_AFLAG_NETWORK, PFI_AFLAG_NOALIAS, PFI_AFLAG_PEER, PFI_IFLAG_ANY, PFI_IFLAG_SKIP,
    PFI_KIF_REF_FLAG, PFI_KIF_REF_NONE, PFI_KIF_REF_ROUTE, PFI_KIF_REF_RULE, PFI_KIF_REF_SRCNODE,
    PFI_KIF_REF_STATE, PFR_TFLAG_ACTIVE, PFR_TFLAG_ALLMASK, PfAddr, PfAddrWrap, PfPoolItem,
    PfStatus, PfiDynaddr, PfiIfhead, PfiKif, PfiKifRefs, PfrAddr, PfrKtable, pf_abi_bytes, pf_cstr,
    pf_pool_get, pf_pool_init, pf_pool_put, pf_strlcpy,
};
use crate::net::pfvar_priv::{PfGlobal, pf_assert_locked, pf_lock, pf_unlock};
use crate::netinet::in_::SockaddrIn;
use crate::netinet6::in6::{In6Addr, in6_is_addr_linklocal, in6_is_scope_embed, satosin6_const};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_NOWAIT, M_PF, M_WAITOK, M_ZERO};
use crate::sys::pool::{PR_LIMITFAIL, Pool};
use crate::sys::socket::{AF_INET, AF_INET6, Sockaddr};
use crate::sys::syslog::LOG_ERR;
use crate::sys::systm::net_assert_locked;
use crate::sys::task::Task;
use crate::sys::tree::RbHead;
use crate::sys::types::SaFamily;

/// `PFI_BUFFER_MAX`: the most addresses `pfi_buffer` grows to.
pub const PFI_BUFFER_MAX: i32 = 0x10000;
/// `PFI_MTYPE`: the `malloc(9)` type of kifs, hook tasks and `pfi_buffer`.
pub const PFI_MTYPE: i32 = M_PF;

/// The C globals `pfi_buffer`, `pfi_buffer_cnt` and `pfi_buffer_max` (see the module's
/// deviations).
pub struct PfiBuffer {
    /// `pfi_buffer`: a `malloc(9)`'d array of `pfi_buffer_max` addresses.
    pub pfi_buffer: Cell<*mut PfrAddr>,
    /// `pfi_buffer_cnt`: the entries in use.
    pub pfi_buffer_cnt: Cell<i32>,
    /// `pfi_buffer_max`: the entries there is room for.
    pub pfi_buffer_max: Cell<i32>,
}

impl PfiBuffer {
    /// An empty buffer (before `pfi_initialize`).
    pub const fn new() -> Self {
        Self {
            pfi_buffer: Cell::new(ptr::null_mut()),
            pfi_buffer_cnt: Cell::new(0),
            pfi_buffer_max: Cell::new(0),
        }
    }
}

impl Default for PfiBuffer {
    fn default() -> Self {
        Self::new()
    }
}

/// `pfi_all`: the kif of the group `all`, set by `pfi_initialize`.
pub static PFI_ALL: PfGlobal<Cell<Option<&'static PfiKif>>> = PfGlobal(Cell::new(None));
/// `pfi_addr_pl`: the pool of `struct pfi_dynaddr`.
pub static PFI_ADDR_PL: Pool = Pool::new();
/// `pfi_ifs`: every kif, by name.
pub static PFI_IFS: PfGlobal<RbHead<PfiIfhead>> = PfGlobal(RbHead::new());
/// `pfi_update`: bumped on every interface or address change; a table whose `pfrkt_larg`
/// differs is out of date (`pfi_dynaddr_update`).
pub static PFI_UPDATE: PfGlobal<Cell<i64>> = PfGlobal(Cell::new(1));
/// `pfi_buffer`, `pfi_buffer_cnt` and `pfi_buffer_max`: the addresses `pfi_table_update`
/// collects.
pub static PFI_BUFFER: PfGlobal<PfiBuffer> = PfGlobal(PfiBuffer::new());

// SAFETY: `PfiDynaddr` is a queue link, `Cell`s of addresses and integers, `Option<&T>`s and a
// raw pointer: all zero is a valid value (`pool_get(PR_ZERO)`).
unsafe impl PfPoolItem for PfiDynaddr {}

/// `pfi_all`.
pub fn pfi_all() -> Option<&'static PfiKif> {
    PFI_ALL.get()
}

/// `pfi_update++`.
fn pfi_update_inc() {
    PFI_UPDATE.set(PFI_UPDATE.get().wrapping_add(1));
}

/// `isupper(c)`.
fn isupper(c: u8) -> bool {
    c.is_ascii_uppercase()
}

/// `islower(c)`.
fn islower(c: u8) -> bool {
    c.is_ascii_lowercase()
}

/// `isalpha(c)`.
fn isalpha(c: u8) -> bool {
    isupper(c) || islower(c)
}

/// The kif an interface's `if_pf_kif` or a group's `ifg_pf_kif` holds.
fn kif_of(word: *mut u8) -> Option<&'static PfiKif> {
    // SAFETY: the word is null or was set by `pfi_attach_ifnet`/`pfi_attach_ifgroup` to a kif
    // that stays allocated while the interface or group points at it: the matching detach
    // clears the word before the kif can be freed.
    unsafe { word.cast::<PfiKif>().cast_const().as_ref() }
}

/// `struct pfi_kif_cmp`: a key for `RB_FIND` in `pfi_ifs`, holding only the name.
fn pfi_kif_key(kif_name: &[u8]) -> PfiKif {
    // SAFETY: `PfiKif: PfAbi`: all-zero bytes are a valid value.
    let mut s: PfiKif = unsafe { core::mem::MaybeUninit::zeroed().assume_init() };
    pf_strlcpy(&mut s.pfik_name, kif_name);
    s
}

/// `pfi_kif_alloc`: a new kif named `kif_name`, `None` when `malloc` fails (`M_NOWAIT`).
pub fn pfi_kif_alloc(kif_name: &[u8], mflags: i32) -> Option<&'static PfiKif> {
    let mem = malloc(size_of::<PfiKif>(), PFI_MTYPE, mflags | M_ZERO)?;
    let p = mem.as_ptr().cast::<PfiKif>();
    // SAFETY: a zero-filled block of `sizeof(struct pfi_kif)` bytes, aligned for it
    // (malloc's chunks are aligned to their size); the all-zero kif is valid (`PfAbi`). The
    // name is written before the kif is published.
    let kif: &'static PfiKif = unsafe {
        pf_strlcpy(&mut (*p).pfik_name, kif_name);
        &*p
    };
    kif.pfik_tzero.set(gettime());
    kif.pfik_dynaddrs.init();

    if pf_cstr(&kif.pfik_name) == b"any" {
        // both so it works in the ioctl and the regular case
        kif.pfik_flags.set(kif.pfik_flags.get() | PFI_IFLAG_ANY);
        kif.pfik_flags_new
            .set(kif.pfik_flags_new.get() | PFI_IFLAG_ANY);
    }

    Some(kif)
}

/// `pfi_kif_free`: frees a kif that was never published (a rule's copy-in buffer).
pub fn pfi_kif_free(kif: Option<&'static PfiKif>) {
    let Some(kif) = kif else {
        return;
    };

    if kif.pfik_rules.get() != 0
        || kif.pfik_states.get() != 0
        || kif.pfik_routes.get() != 0
        || kif.pfik_srcnodes.get() != 0
        || kif.pfik_flagrefs.get() != 0
    {
        panic(format_args!("kif is still alive"));
    }

    free(NonNull::from(kif).cast(), PFI_MTYPE, size_of::<PfiKif>());
}

/// `pfi_initialize`: the address pool, `pfi_buffer` and the kif of `all`, once.
pub fn pfi_initialize() {
    // The first time we arrive here is during kernel boot, when if_attachsetup() for the
    // first time. No locking is needed in this case, because it's granted there is a single
    // thread, which sets pfi_all global var.
    if PFI_ALL.get().is_some() {
        // already initialized
        return;
    }

    pf_pool_init::<PfiDynaddr>(&PFI_ADDR_PL, IPL_SOFTNET, 0, "pfiaddrpl");
    PFI_BUFFER.pfi_buffer_max.set(64);
    let Some(buf) = mallocarray(
        PFI_BUFFER.pfi_buffer_max.get() as usize,
        size_of::<PfrAddr>(),
        PFI_MTYPE,
        M_WAITOK,
    ) else {
        panic(format_args!("pfi_initialize: out of memory"));
    };
    PFI_BUFFER.pfi_buffer.set(buf.as_ptr().cast());

    let Some(all) = pfi_kif_alloc(IFG_ALL, M_WAITOK) else {
        panic(format_args!("pfi_initialize: out of memory"));
    };
    PFI_ALL.set(Some(all));

    // SAFETY: `all` is new and in no tree; it is never freed (`pfi_kif_unref` keeps
    // `pfi_all`).
    if unsafe { PFI_IFS.insert(all) }.is_some() {
        panic(format_args!("IFG_ALL kif found already"));
    }
}

/// `pfi_kif_find`: the kif named `kif_name`.
pub fn pfi_kif_find(kif_name: &[u8]) -> Option<&'static PfiKif> {
    pf_assert_locked();

    let s = pfi_kif_key(kif_name);
    PFI_IFS.find(&s)
}

/// `pfi_kif_get`: the kif named `kif_name`, created (from `*prealloc` when the caller has
/// one, which is then taken) if there is none.
pub fn pfi_kif_get(
    kif_name: &[u8],
    prealloc: Option<&mut Option<&'static PfiKif>>,
) -> Option<&'static PfiKif> {
    pf_assert_locked();

    if let Some(kif) = pfi_kif_find(kif_name) {
        return Some(kif);
    }

    // create new one
    let kif = match prealloc {
        Some(pre) if pre.is_some() => pre.take()?,
        _ => pfi_kif_alloc(kif_name, M_NOWAIT)?,
    };

    // SAFETY: `kif` is new (or the caller's unpublished buffer) and in no tree; it stays
    // allocated until `pfi_kif_unref` takes it out.
    let _ = unsafe { PFI_IFS.insert(kif) };
    Some(kif)
}

/// `pfi_kif_ref`: counts a reference of kind `what`.
pub fn pfi_kif_ref(kif: &PfiKif, what: PfiKifRefs) {
    pf_assert_locked();

    let c = match what {
        PFI_KIF_REF_RULE => &kif.pfik_rules,
        PFI_KIF_REF_STATE => &kif.pfik_states,
        PFI_KIF_REF_ROUTE => &kif.pfik_routes,
        PFI_KIF_REF_SRCNODE => &kif.pfik_srcnodes,
        PFI_KIF_REF_FLAG => &kif.pfik_flagrefs,
        _ => panic(format_args!("pfi_kif_ref with unknown type")),
    };
    c.set(c.get() + 1);
}

/// `pfi_kif_unref`: drops a reference of kind `what` (`PFI_KIF_REF_NONE`: none, only the
/// check below); frees the kif when it is neither attached nor referenced any more.
pub fn pfi_kif_unref(kif: Option<&'static PfiKif>, what: PfiKifRefs) {
    let Some(kif) = kif else {
        return;
    };

    pf_assert_locked();

    let name = Str(pf_cstr(&kif.pfik_name));
    let (c, kind) = match what {
        PFI_KIF_REF_NONE => (None, ""),
        PFI_KIF_REF_RULE => (Some(&kif.pfik_rules), "rules"),
        PFI_KIF_REF_STATE => (Some(&kif.pfik_states), "state"),
        PFI_KIF_REF_ROUTE => (Some(&kif.pfik_routes), "route"),
        PFI_KIF_REF_SRCNODE => (Some(&kif.pfik_srcnodes), "src-node"),
        PFI_KIF_REF_FLAG => (Some(&kif.pfik_flagrefs), "flags"),
        _ => panic(format_args!("pfi_kif_unref ({name}) with unknown type")),
    };
    if let Some(c) = c {
        if c.get() <= 0 {
            crate::dpfprintf!(LOG_ERR, "pfi_kif_unref ({}): {} refcount <= 0", name, kind);
            return;
        }
        c.set(c.get() - 1);
    }

    if kif.pfik_ifp().is_some()
        || kif.pfik_group().is_some()
        || pfi_all().is_some_and(|all| ptr::eq(kif, all))
    {
        return;
    }

    if kif.pfik_rules.get() != 0
        || kif.pfik_states.get() != 0
        || kif.pfik_routes.get() != 0
        || kif.pfik_srcnodes.get() != 0
        || kif.pfik_flagrefs.get() != 0
    {
        return;
    }

    // SAFETY: every kif that reaches here was published by `pfi_kif_get` or
    // `pfi_initialize` and is in `pfi_ifs`.
    unsafe { PFI_IFS.remove(kif) };
    free(NonNull::from(kif).cast(), PFI_MTYPE, size_of::<PfiKif>());
}

/// `pfi_kif_match`: whether a rule bound to `rule_kif` (none: any interface) applies to a
/// packet on `packet_kif`: the same kif, a group the packet's interface is in, or `any` and
/// the interface is not a loopback.
pub fn pfi_kif_match(
    rule_kif: Option<&'static PfiKif>,
    packet_kif: Option<&'static PfiKif>,
) -> bool {
    let Some(rule_kif) = rule_kif else {
        return true;
    };
    if packet_kif.is_some_and(|p| ptr::eq(rule_kif, p)) {
        return true;
    }
    let packet_ifp = packet_kif.and_then(|p| p.pfik_ifp());

    if let Some(group) = rule_kif.pfik_group()
        && let Some(ifp) = packet_ifp
        && ifp.if_groups.iter().any(|p| ptr::eq(p.ifgl_group, group))
    {
        return true;
    }

    if rule_kif.pfik_flags.get() & PFI_IFLAG_ANY != 0
        && let Some(ifp) = packet_ifp
        && ifp.if_flags.get() & IFF_LOOPBACK == 0
    {
        return true;
    }

    false
}

/// `pfi_attach_ifnet`: the interface's kif (created if needed), linked both ways, and the
/// address hook that keeps its dynamic addresses current.
pub fn pfi_attach_ifnet(ifp: &'static Ifnet) {
    pf_lock();
    pfi_initialize();
    pfi_update_inc();
    let Some(kif) = pfi_kif_get(&ifp.if_xname.get(), None) else {
        panic(format_args!("pfi_attach_ifnet: pfi_kif_get failed"));
    };

    kif.set_pfik_ifp(Some(ifp));
    ifp.if_pf_kif.set(ptr::from_ref(kif).cast_mut().cast());

    let Some(mem) = malloc(size_of::<Task>(), PFI_MTYPE, M_WAITOK) else {
        panic(format_args!("pfi_attach_ifnet: out of memory"));
    };
    let t = mem.as_ptr().cast::<Task>();
    // SAFETY: a fresh block of `sizeof(struct task)` bytes, aligned for it; `task_set` on
    // the new task. It lives until `pfi_detach_ifnet` frees it.
    let t: &'static Task = unsafe {
        t.write(Task::new(
            pfi_kifaddr_update,
            ptr::from_ref(kif).cast_mut().cast(),
        ));
        &*t
    };
    // SAFETY: the task is new, on no list, and stays allocated until `pfi_detach_ifnet`
    // removes it from the hooks.
    unsafe { if_addrhook_add(ifp, t) };
    kif.pfik_ah_cookie.set(ptr::from_ref(t).cast_mut().cast());

    pfi_kif_update(kif);
    pf_unlock();
}

/// `pfi_detach_ifnet`: removes the address hook and unlinks the interface from its kif,
/// which goes away unless something refers to it.
pub fn pfi_detach_ifnet(ifp: &'static Ifnet) {
    let Some(kif) = kif_of(ifp.if_pf_kif.get()) else {
        return;
    };

    pf_lock();
    pfi_update_inc();
    let t = kif.pfik_ah_cookie.get().cast::<Task>();
    kif.pfik_ah_cookie.set(ptr::null_mut());
    if let Some(t) = NonNull::new(t) {
        // SAFETY: `pfik_ah_cookie` is the task `pfi_attach_ifnet` added to this interface's
        // hooks and allocated; nothing else frees it.
        unsafe { if_addrhook_del(ifp, t.as_ref()) };
        free(t.cast(), PFI_MTYPE, size_of::<Task>());
    }

    pfi_kif_update(kif);

    kif.set_pfik_ifp(None);
    ifp.if_pf_kif.set(ptr::null_mut());
    pfi_kif_unref(Some(kif), PFI_KIF_REF_NONE);
    pf_unlock();
}

/// `pfi_attach_ifgroup`: the group's kif, linked both ways.
pub fn pfi_attach_ifgroup(ifg: &'static IfgGroup) {
    pf_lock();
    pfi_initialize();
    pfi_update_inc();
    let Some(kif) = pfi_kif_get(&ifg.ifg_group, None) else {
        panic(format_args!("pfi_attach_ifgroup: pfi_kif_get failed"));
    };

    kif.set_pfik_group(Some(ifg));
    ifg.ifg_pf_kif.set(ptr::from_ref(kif).cast_mut().cast());
    pf_unlock();
}

/// `pfi_detach_ifgroup`.
pub fn pfi_detach_ifgroup(ifg: &'static IfgGroup) {
    let Some(kif) = kif_of(ifg.ifg_pf_kif.get()) else {
        return;
    };

    pf_lock();
    pfi_update_inc();

    kif.set_pfik_group(None);
    ifg.ifg_pf_kif.set(ptr::null_mut());
    pfi_kif_unref(Some(kif), PFI_KIF_REF_NONE);
    pf_unlock();
}

/// `pfi_group_change`: a group's members changed; its dynamic addresses are refreshed.
pub fn pfi_group_change(group: &[u8]) {
    pfi_update_inc();
    let Some(kif) = pfi_kif_get(group, None) else {
        panic(format_args!("pfi_group_change: pfi_kif_get failed"));
    };

    pfi_kif_update(kif);
}

/// `pfi_group_delmember`.
pub fn pfi_group_delmember(group: &[u8]) {
    pf_lock();
    pfi_group_change(group);
    pfi_xcommit();
    pf_unlock();
}

/// `pfi_group_addmember`.
pub fn pfi_group_addmember(group: &[u8]) {
    pf_lock();
    pfi_group_change(group);
    pfi_xcommit();
    pf_unlock();
}

/// `pfi_match_addr`: whether `a` is one of the dynamic address's addresses: none, the one
/// kept in the descriptor, or one of the table's.
pub fn pfi_match_addr(dyn_: &PfiDynaddr, a: &PfAddr, af: SaFamily) -> bool {
    match af {
        AF_INET => match dyn_.pfid_acnt4.get() {
            0 => false,
            1 => pf_match_addr(
                0,
                &dyn_.pfid_addr4.get(),
                &dyn_.pfid_mask4.get(),
                a,
                AF_INET,
            ),
            _ => dyn_
                .pfid_kt
                .get()
                .is_some_and(|kt| pfr_match_addr(kt, a, AF_INET)),
        },
        #[cfg(feature = "inet6")]
        AF_INET6 => match dyn_.pfid_acnt6.get() {
            0 => false,
            1 => pf_match_addr(
                0,
                &dyn_.pfid_addr6.get(),
                &dyn_.pfid_mask6.get(),
                a,
                AF_INET6,
            ),
            _ => dyn_
                .pfid_kt
                .get()
                .is_some_and(|kt| pfr_match_addr(kt, a, AF_INET6)),
        },
        _ => false,
    }
}

/// `pfi_dynaddr_setup`: makes `aw`, an `(ifname)` operand, a dynamic address: a
/// `pfi_dynaddr` on the interface's kif with a table of its addresses in the reserved
/// anchor. `true` (the C's 0) for it or any other operand; `false` (the C's 1) when something
/// cannot be had.
pub fn pfi_dynaddr_setup(aw: &'static PfAddrWrap, af: SaFamily, wait: i32) -> bool {
    if aw.type_.get() != PF_ADDR_DYNIFTL {
        return true;
    }
    let Some(dyn_) = pf_pool_get::<PfiDynaddr>(&PFI_ADDR_PL, wait | PR_LIMITFAIL) else {
        return false;
    };

    let v = aw.v.get();
    if pf_cstr(v.ifname()) == b"self" {
        dyn_.pfid_kif.set(pfi_kif_get(IFG_ALL, None));
    } else {
        dyn_.pfid_kif.set(pfi_kif_get(v.ifname(), None));
    }

    let mut ruleset = None;
    let ok = 'setup: {
        let Some(kif) = dyn_.pfid_kif.get() else {
            break 'setup false;
        };
        pfi_kif_ref(kif, PFI_KIF_REF_RULE);

        dyn_.pfid_net.set(pfi_unmask(&v.mask()));
        if af == AF_INET && dyn_.pfid_net.get() == 32 {
            dyn_.pfid_net.set(128);
        }
        let mut tblname = [0u8; PF_TABLE_NAME_SIZE];
        pf_strlcpy(&mut tblname, v.ifname());
        let iflags = aw.iflags.get();
        if iflags & PFI_AFLAG_NETWORK != 0 {
            libkern::strlcat(&mut tblname, b":network");
        }
        if iflags & PFI_AFLAG_BROADCAST != 0 {
            libkern::strlcat(&mut tblname, b":broadcast");
        }
        if iflags & PFI_AFLAG_PEER != 0 {
            libkern::strlcat(&mut tblname, b":peer");
        }
        if iflags & PFI_AFLAG_NOALIAS != 0 {
            libkern::strlcat(&mut tblname, b":0");
        }
        if dyn_.pfid_net.get() != 128 {
            let n = pf_cstr(&tblname).len();
            let _ = snprintf(&mut tblname[n..], format_args!("/{}", dyn_.pfid_net.get()));
        }
        ruleset = pf_find_or_create_ruleset(PF_RESERVED_ANCHOR);
        let Some(rs) = ruleset else {
            break 'setup false;
        };

        dyn_.pfid_kt.set(pfr_attach_table(rs, &tblname, wait));
        let Some(kt) = dyn_.pfid_kt.get() else {
            break 'setup false;
        };

        kt.pfrkt_flags()
            .set(kt.pfrkt_flags().get() | PFR_TFLAG_ACTIVE);
        dyn_.pfid_iflags.set(iflags);
        dyn_.pfid_af.set(af);

        // SAFETY: the new dynamic address is on no list; it stays allocated until
        // `pfi_dynaddr_remove` takes it off.
        unsafe { kif.pfik_dynaddrs.insert_tail(dyn_) };
        aw.set_dyn(Some(dyn_));
        pfi_kif_update(kif);
        true
    };
    if ok {
        return true;
    }

    // _bad:
    if let Some(kt) = dyn_.pfid_kt.get() {
        pfr_detach_table(kt);
    }
    if let Some(rs) = ruleset {
        pf_remove_if_empty_ruleset(rs);
    }
    if let Some(kif) = dyn_.pfid_kif.get() {
        pfi_kif_unref(Some(kif), PFI_KIF_REF_RULE);
    }
    pf_pool_put(&PFI_ADDR_PL, dyn_);
    false
}

/// `pfi_kif_update`: brings the dynamic addresses of the kif, and of every group its
/// interface is in, up to date.
pub fn pfi_kif_update(kif: &'static PfiKif) {
    // update all dynaddr
    for p in kif.pfik_dynaddrs.iter() {
        pfi_dynaddr_update(p);
    }

    // again for all groups kif is member of
    if let Some(ifp) = kif.pfik_ifp() {
        for ifgl in ifp.if_groups.iter() {
            if let Some(gkif) = kif_of(ifgl.ifgl_group.ifg_pf_kif.get()) {
                pfi_kif_update(gkif);
            }
        }
    }
}

/// `pfi_dynaddr_update`: refills the dynamic address's table if the interfaces changed
/// since, then takes the address count (and the single address) from the table.
pub fn pfi_dynaddr_update(dyn_: &'static PfiDynaddr) {
    let (Some(kif), Some(kt)) = (dyn_.pfid_kif.get(), dyn_.pfid_kt.get()) else {
        panic(format_args!("pfi_dynaddr_update"));
    };

    if kt.pfrkt_larg.get() != PFI_UPDATE.get() {
        // this table needs to be brought up-to-date
        pfi_table_update(
            kt,
            kif,
            dyn_.pfid_net.get() as u8,
            i32::from(dyn_.pfid_iflags.get()),
        );
        kt.pfrkt_larg.set(PFI_UPDATE.get());
    }
    pfr_dynaddr_update(kt, dyn_);
}

/// `pfi_table_update`: replaces the table's addresses with the addresses of the kif's
/// interface or of every member of its group, as `net` and `flags` select them.
pub fn pfi_table_update(kt: &'static PfrKtable, kif: &'static PfiKif, net: u8, flags: i32) {
    let mut size2 = 0;

    PFI_BUFFER.pfi_buffer_cnt.set(0);

    if let Some(ifp) = kif.pfik_ifp() {
        pfi_instance_add(Some(ifp), net, flags);
    } else if let Some(group) = kif.pfik_group() {
        for ifgm in group.ifg_members.iter() {
            pfi_instance_add(Some(ifgm.ifgm_ifp), net, flags);
        }
    }

    let cnt = PFI_BUFFER.pfi_buffer_cnt.get();
    let buf: &mut [PfrAddr] = match NonNull::new(PFI_BUFFER.pfi_buffer.get()) {
        // SAFETY: `pfi_buffer` is the `malloc`'d array of `pfi_buffer_max` addresses
        // (`pfi_initialize`, `pfi_address_add` grows it), of which the first `cnt` are
        // filled; nothing else touches it until `pfr_set_addrs` returns (net lock).
        Some(p) => unsafe { core::slice::from_raw_parts_mut(p.as_ptr(), cnt.max(0) as usize) },
        None => &mut [],
    };
    // The table's anchor is already fixed, so `pfr_set_addrs`'s validation of this copy
    // changes nothing (the C passes `&kt->pfrkt_t`).
    let mut t = crate::net::pfvar::pf_abi_clone(kt.pfrkt_t());
    if let Err(e) = pfr_set_addrs(
        &mut t,
        &mut PfrBuf::Kernel(buf),
        cnt,
        Some(&mut size2),
        None,
        None,
        None,
        0,
        PFR_TFLAG_ALLMASK,
    ) {
        crate::dpfprintf!(
            LOG_ERR,
            "pfi_table_update: cannot set {} new addresses into table {}: {}",
            PFI_BUFFER.pfi_buffer_cnt.get(),
            Str(pf_cstr(kt.pfrkt_name())),
            e as i32
        );
    }
}

/// `((struct sockaddr_in6 *)sa)->sin6_addr`.
///
/// # Safety
///
/// `sa` points at a readable `struct sockaddr_in6`.
unsafe fn sin6_addr(sa: *const Sockaddr) -> In6Addr {
    // SAFETY: the caller's contract; a generic sockaddr pointer has a smaller alignment, so
    // the structure is read unaligned.
    unsafe { ptr::read_unaligned(satosin6_const(sa)).sin6_addr }
}

/// `sin_addr` of the `struct sockaddr_in` at `sa`, as a `pf_addr`.
///
/// # Safety
///
/// `sa` points at a readable `struct sockaddr_in`.
unsafe fn sin_pfaddr(sa: *const Sockaddr) -> PfAddr {
    let sin = sa.cast::<SockaddrIn>();
    // SAFETY: the caller's contract.
    PfAddr::from_v4(unsafe { ptr::read_unaligned(ptr::addr_of!((*sin).sin_addr)) })
}

/// `pfi_instance_add`: adds the interface's addresses to `pfi_buffer`: its own, broadcast or
/// peer addresses (`flags`), the first of each family only (`PFI_AFLAG_NOALIAS`), as hosts
/// or as networks of `net` bits (or of their netmask, `PFI_AFLAG_NETWORK`).
pub fn pfi_instance_add(ifp: Option<&'static Ifnet>, net: u8, flags: i32) {
    let Some(ifp) = ifp else {
        return;
    };
    let flags_has = |f: u8| flags & i32::from(f) != 0;
    let mut got4 = false;
    let mut got6 = false;
    for ifa in ifp.if_addrlist.iter() {
        let sa = ifa.ifa_addr.get();
        if sa.is_null() {
            continue;
        }
        // SAFETY: an address on the list points at a valid socket address.
        let af = unsafe { (*sa).sa_family };
        if af != AF_INET && af != AF_INET6 {
            continue;
        }
        if flags_has(PFI_AFLAG_BROADCAST) && af == AF_INET6 {
            continue;
        }
        if flags_has(PFI_AFLAG_BROADCAST) && ifp.if_flags.get() & IFF_BROADCAST == 0 {
            continue;
        }
        if flags_has(PFI_AFLAG_PEER) && ifp.if_flags.get() & IFF_POINTOPOINT == 0 {
            continue;
        }
        if flags_has(PFI_AFLAG_NETWORK) && af == AF_INET6 {
            // SAFETY: an `AF_INET6` address is a `struct sockaddr_in6`.
            let a6 = unsafe { sin6_addr(sa) };
            if in6_is_addr_linklocal(&a6) {
                continue;
            }
        }
        if flags_has(PFI_AFLAG_NOALIAS) {
            if af == AF_INET && got4 {
                continue;
            }
            if af == AF_INET6 && got6 {
                continue;
            }
        }
        if af == AF_INET {
            got4 = true;
        } else if af == AF_INET6 {
            got6 = true;
        }
        let mut net2 = i32::from(net);
        if net2 == 128 && flags_has(PFI_AFLAG_NETWORK) {
            let mask = ifa.ifa_netmask.get();
            if !mask.is_null() {
                if af == AF_INET {
                    // SAFETY: the netmask of an `AF_INET` address is a `sockaddr_in`.
                    net2 = pfi_unmask(&unsafe { sin_pfaddr(mask) });
                } else if af == AF_INET6 {
                    // SAFETY: the netmask of an `AF_INET6` address is a `sockaddr_in6`.
                    net2 = pfi_unmask(&PfAddr::from_v6(unsafe { sin6_addr(mask) }));
                }
            }
        }
        if af == AF_INET && net2 > 32 {
            net2 = 32;
        }
        let a = if flags_has(PFI_AFLAG_BROADCAST) {
            ifa.ifa_broadaddr().get()
        } else if flags_has(PFI_AFLAG_PEER) {
            ifa.ifa_dstaddr.get()
        } else {
            sa
        };
        // SAFETY: the interface's address, broadcast or peer address is null or a socket
        // address of the interface address's family.
        unsafe { pfi_address_add(a, af, net2 as u8) };
    }
}

/// `pfi_address_add`: appends `sa` as a network of `net` bits to `pfi_buffer`, growing it
/// (up to `PFI_BUFFER_MAX` entries) when it is full.
///
/// # Safety
///
/// `sa` is null or points at a readable socket address of family `af` (`struct
/// sockaddr_in` for `AF_INET`, `struct sockaddr_in6` for `AF_INET6`).
pub unsafe fn pfi_address_add(sa: *const Sockaddr, af: SaFamily, mut net: u8) {
    if sa.is_null() {
        return;
    }
    if PFI_BUFFER.pfi_buffer_cnt.get() >= PFI_BUFFER.pfi_buffer_max.get() {
        let new_max = PFI_BUFFER.pfi_buffer_max.get() * 2;

        if new_max > PFI_BUFFER_MAX {
            crate::dpfprintf!(
                LOG_ERR,
                "pfi_address_add: address buffer full ({}/{})",
                PFI_BUFFER.pfi_buffer_cnt.get(),
                PFI_BUFFER_MAX
            );
            return;
        }
        let Some(p) = mallocarray(new_max as usize, size_of::<PfrAddr>(), PFI_MTYPE, M_NOWAIT)
        else {
            crate::dpfprintf!(
                LOG_ERR,
                "pfi_address_add: no memory to grow buffer ({}/{})",
                PFI_BUFFER.pfi_buffer_cnt.get(),
                PFI_BUFFER_MAX
            );
            return;
        };
        let old = PFI_BUFFER.pfi_buffer.get();
        let old_max = PFI_BUFFER.pfi_buffer_max.get() as usize;
        // SAFETY: the old buffer holds `pfi_buffer_max` entries, the new one twice as many;
        // they are distinct allocations.
        unsafe { ptr::copy_nonoverlapping(old, p.as_ptr().cast::<PfrAddr>(), old_max) };
        // no need to zero buffer
        if let Some(old) = NonNull::new(old) {
            free(old.cast(), PFI_MTYPE, old_max * size_of::<PfrAddr>());
        }
        PFI_BUFFER.pfi_buffer.set(p.as_ptr().cast());
        PFI_BUFFER.pfi_buffer_max.set(new_max);
    }
    if af == AF_INET && net > 32 {
        net = 128;
    }
    let mut p = PfrAddr {
        pfra_af: af,
        pfra_net: net,
        ..PfrAddr::default()
    };
    if af == AF_INET {
        // SAFETY: the caller's contract: an `AF_INET` address is a `sockaddr_in`.
        p.pfra_u = unsafe { sin_pfaddr(sa) };
    } else if af == AF_INET6 {
        // SAFETY: the caller's contract: an `AF_INET6` address is a `sockaddr_in6`.
        let mut a6 = unsafe { sin6_addr(sa) };
        if in6_is_scope_embed(&a6) {
            a6.set_s6_addr16(1, 0);
        }
        p.pfra_u.set_v6(a6);
    }
    // mask network address bits
    let b = &mut p.pfra_u.addr8;
    let bits = usize::from(p.pfra_net);
    if bits < 128 {
        b[bits / 8] &= !(0xffu8 >> (bits % 8));
    }
    for byte in b.iter_mut().skip(bits.div_ceil(8)) {
        *byte = 0;
    }

    let i = PFI_BUFFER.pfi_buffer_cnt.get();
    PFI_BUFFER.pfi_buffer_cnt.set(i + 1);
    // SAFETY: `i < pfi_buffer_max` (grown above), so the entry is within the buffer.
    unsafe { PFI_BUFFER.pfi_buffer.get().add(i as usize).write(p) };
}

/// `pfi_dynaddr_remove`: undoes `pfi_dynaddr_setup`.
pub fn pfi_dynaddr_remove(aw: &'static PfAddrWrap) {
    if aw.type_.get() != PF_ADDR_DYNIFTL {
        return;
    }
    let Some(dyn_) = aw.dyn_() else {
        return;
    };
    let (Some(kif), Some(kt)) = (dyn_.pfid_kif.get(), dyn_.pfid_kt.get()) else {
        return;
    };

    // SAFETY: `pfi_dynaddr_setup` put the dynamic address on its kif's list.
    unsafe { kif.pfik_dynaddrs.remove(dyn_) };
    pfi_kif_unref(Some(kif), PFI_KIF_REF_RULE);
    dyn_.pfid_kif.set(None);
    pfr_detach_table(kt);
    dyn_.pfid_kt.set(None);
    pf_pool_put(&PFI_ADDR_PL, dyn_);
    aw.set_dyn(None);
}

/// `pfi_dynaddr_copyout`: replaces the dynamic address in a copy of an operand for user
/// space with its address count (`p.dyncnt`).
///
/// # Safety
///
/// `aw` is a kernel operand or a byte copy of one made under `pf_lock`, which is still held:
/// for `PF_ADDR_DYNIFTL`, its `p` is null or the address of a live `pfi_dynaddr`.
pub unsafe fn pfi_dynaddr_copyout(aw: &PfAddrWrap) {
    if aw.type_.get() != PF_ADDR_DYNIFTL {
        return;
    }
    // SAFETY: the caller's contract.
    let Some(dyn_) = (unsafe { (aw.p.get() as *const PfiDynaddr).as_ref() }) else {
        return;
    };
    if dyn_.pfid_kif.get().is_none() {
        return;
    }
    aw.set_cnt(dyn_.pfid_acnt4.get() + dyn_.pfid_acnt6.get());
}

/// `pfi_kifaddr_update`: the interface address hook (`task(9)` function, `v` the kif).
pub fn pfi_kifaddr_update(v: *mut c_void) {
    // SAFETY: `pfi_attach_ifnet` made the hook with its kif as the argument, and removes the
    // hook before the kif can go.
    let Some(kif) = (unsafe { v.cast::<PfiKif>().cast_const().as_ref() }) else {
        return;
    };

    net_assert_locked("pfi_kifaddr_update");

    pf_lock();
    pfi_update_inc();
    pfi_kif_update(kif);
    pf_unlock();
}

/// `pfi_if_compare`: kifs are ordered by name.
pub fn pfi_if_compare(p: &PfiKif, q: &PfiKif) -> Ordering {
    pf_cstr(&p.pfik_name).cmp(pf_cstr(&q.pfik_name))
}

/// `pfi_update_status`: with an empty `name` and no `pfs`, clears every kif's counters;
/// otherwise sums the counters of the interface `name` (or of the members of the group
/// `name`) into `pfs`, or clears theirs when there is no `pfs`.
pub fn pfi_update_status(name: &[u8], pfs: Option<&mut PfStatus>) {
    let name = pf_cstr(name);
    let pfs = pfs.as_deref();

    if name.is_empty() && pfs.is_none() {
        for p in PFI_IFS.iter() {
            pfi_kif_clear_counters(p);
        }
        return;
    }

    let key = pfi_kif_key(name);
    let Some(p) = PFI_IFS.find(&key) else {
        return;
    };
    if let Some(pfs) = pfs {
        for c in pfs.pcounters.iter().flatten().flatten() {
            c.set(0);
        }
        for c in pfs.bcounters.iter().flatten() {
            c.set(0);
        }
    }

    let visit = |ifp: &'static Ifnet| {
        let Some(p) = kif_of(ifp.if_pf_kif.get()) else {
            return;
        };

        // just clear statistics
        let Some(pfs) = pfs else {
            pfi_kif_clear_counters(p);
            return;
        };
        for i in 0..2 {
            for j in 0..2 {
                for k in 0..2 {
                    let pc = &pfs.pcounters[i][j][k];
                    pc.set(pc.get().wrapping_add(p.pfik_packets[i][j][k].get()));
                    let bc = &pfs.bcounters[i][j];
                    bc.set(bc.get().wrapping_add(p.pfik_bytes[i][j][k].get()));
                }
            }
        }
    };
    if let Some(group) = p.pfik_group() {
        for ifgm in group.ifg_members.iter() {
            visit(ifgm.ifgm_ifp);
        }
    } else if let Some(ifp) = p.pfik_ifp() {
        // build a temporary list for p only
        visit(ifp);
    }
}

/// The `memset`s of a kif's counters and the new `pfik_tzero`.
fn pfi_kif_clear_counters(p: &PfiKif) {
    for c in p.pfik_packets.iter().flatten().flatten() {
        c.set(0);
    }
    for c in p.pfik_bytes.iter().flatten().flatten() {
        c.set(0);
    }
    p.pfik_tzero.set(gettime());
}

/// `pfi_get_ifaces`: copies the kifs `name` selects (`pfi_skip_if`), at most `*size` and as
/// many as fit, into `buf` as `struct pfi_kif`s; `*size` becomes the number copied.
pub fn pfi_get_ifaces(name: &[u8], buf: &mut [u8], size: &mut i32) {
    let mut n = 0;
    let (slots, _) = buf.as_chunks_mut::<{ size_of::<PfiKif>() }>();
    let mut slots = slots.iter_mut();

    for p in PFI_IFS.iter() {
        if pfi_skip_if(name, p) {
            continue;
        }
        if n >= *size {
            break;
        }
        let Some(slot) = slots.next() else {
            break;
        };
        n += 1;
        if p.pfik_tzero.get() == 0 {
            p.pfik_tzero.set(gettime());
        }
        slot.copy_from_slice(pf_abi_bytes(p));
    }
    *size = n;
}

/// `pfi_skip_if`: whether `p` is left out by the interface or group name `filter` (an empty
/// filter keeps everything).
pub fn pfi_skip_if(filter: &[u8], p: &'static PfiKif) -> bool {
    pf_assert_locked();

    let filter = pf_cstr(filter);
    if filter.is_empty() {
        return false;
    }
    if pf_cstr(&p.pfik_name) == filter {
        return false; // exact match
    }
    let n = filter.len();
    if !(1..IFNAMSIZ).contains(&n) {
        return true; // sanity check
    }
    if filter[n - 1].is_ascii_digit() {
        return true; // group names may not end in a digit
    }
    if let Some(ifp) = p.pfik_ifp()
        && ifp
            .if_groups
            .iter()
            .any(|i| pf_cstr(&i.ifgl_group.ifg_group) == filter)
    {
        return false; // iface is in group "filter"
    }
    true
}

/// `pfi_set_flags`: adds `flags` to the pending flags (`pfik_flags_new`) of the kif `name`,
/// created if needed, or of every kif; a kif created or newly skipped here holds a flag
/// reference.
pub fn pfi_set_flags(name: &[u8], flags: i32) -> Result<(), Errno> {
    pf_assert_locked();

    let name = pf_cstr(name);
    if !name.is_empty() {
        match pfi_kif_find(name) {
            None => {
                let n = name.len();
                if !(1..IFNAMSIZ).contains(&n) {
                    return Err(Errno::EINVAL);
                }

                if !isalpha(name[0]) {
                    return Err(Errno::EINVAL);
                }

                let Some(p) = pfi_kif_get(name, None) else {
                    panic(format_args!("pfi_set_flags pfi_kif_get() returned NULL\n"));
                };
                p.pfik_flags_new.set(p.pfik_flags.get() | flags);
                // We use pfik_flagrefs counter as an indication whether the kif has been
                // created on behalf of 'pfi_set_flags()' or not.
                kassert!(p.pfik_flagrefs.get() == 0);
                if p.pfik_flags_new.get() & PFI_IFLAG_SKIP != 0 {
                    pfi_kif_ref(p, PFI_KIF_REF_FLAG);
                }
            }
            Some(p) => {
                // pf.conf may accidentally contain two set skip on ... statements. For
                // example:
                //     set skip lo
                //     set skip lo
                // We need to grab reference only when skip flag is set to avoid tripping
                // assert pfi_clear_flags()
                if flags & PFI_IFLAG_SKIP != 0
                    && p.pfik_flags_new.get() & PFI_IFLAG_SKIP == 0
                    && p.pfik_flags.get() & PFI_IFLAG_SKIP == 0
                {
                    pfi_kif_ref(p, PFI_KIF_REF_FLAG);
                }

                p.pfik_flags_new.set(p.pfik_flags.get() | flags);
            }
        }
    } else {
        for p in PFI_IFS.iter() {
            p.pfik_flags_new.set(p.pfik_flags.get() | flags);
        }
    }

    Ok(())
}

/// `pfi_clear_flags`: removes `flags` from the pending flags of the kif `name` or of every
/// kif; a kif no longer skipped drops its flag reference.
pub fn pfi_clear_flags(name: &[u8], flags: i32) -> Result<(), Errno> {
    pf_assert_locked();

    let clear = |p: &'static PfiKif| {
        p.pfik_flags_new.set(p.pfik_flags.get() & !flags);

        kassert!(p.pfik_flagrefs.get() == 0 || p.pfik_flagrefs.get() == 1);

        if p.pfik_flags_new.get() & PFI_IFLAG_SKIP == 0 && p.pfik_flagrefs.get() == 1 {
            pfi_kif_unref(Some(p), PFI_KIF_REF_FLAG);
        }
    };

    let name = pf_cstr(name);
    if !name.is_empty() {
        let Some(p) = pfi_kif_find(name) else {
            return Err(Errno::ESRCH);
        };
        clear(p);
    } else {
        // RB_FOREACH_SAFE: `clear` may free the kif it is given.
        for p in PFI_IFS.iter() {
            clear(p);
        }
    }

    Ok(())
}

/// `pfi_xcommit`: makes the pending flags current; an interface also takes the pending
/// flags of its groups.
pub fn pfi_xcommit() {
    pf_assert_locked();

    for p in PFI_IFS.iter() {
        p.pfik_flags.set(p.pfik_flags_new.get());
        // if kif is backed by existing interface, then we must use skip flags found in
        // groups. We use pfik_flags_new, otherwise we would need to do two RB_FOREACH()
        // passes: the first to commit group changes the second to commit flag changes for
        // interfaces.
        if let Some(ifp) = p.pfik_ifp() {
            for g in ifp.if_groups.iter() {
                let gkif = kif_of(g.ifgl_group.ifg_pf_kif.get());
                kassert!(gkif.is_some());
                if let Some(gkif) = gkif {
                    p.pfik_flags
                        .set(p.pfik_flags.get() | gkif.pfik_flags_new.get());
                }
            }
        }
    }
}

/// `pfi_unmask`: the prefix length of the netmask `addr` (its leading one bits); from
/// `pf_print_state.c`.
pub fn pfi_unmask(addr: &PfAddr) -> i32 {
    let mut j = 0;
    let mut b = 0;

    while j < 4 && addr.addr32(j) == 0xffff_ffff {
        b += 32;
        j += 1;
    }
    if j < 4 {
        let tmp = u32::from_be(addr.addr32(j));
        b += tmp.leading_ones() as i32;
    }
    b
}

/// Host tests: pfi back to "never initialised" (`pfi_all` NULL, no kifs), as the old state
/// points into an earlier test's memory; the next `pfi_initialize` starts over.
#[cfg(test)]
pub(crate) fn pfi_test_reset() {
    PFI_ALL.set(None);
    PFI_IFS.init();
}

#[cfg(test)]
mod tests;
