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
 * Copyright (c) 2002,2003 Henning Brauer
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
//! Anchors and rulesets: the tree of named rulesets `pf(4)` evaluates (`anchor "a/b"`), shared
//! with `pfctl(8)`.
//!
//! Upstream: sys/net/pf_ruleset.c @ 3ce1f3f79392
//!
//! An anchor is a pool item (`pf_anchor_pl`) linked into the global tree `pf_anchors`, ordered by
//! path, and into its parent's `children`. Its ruleset holds the rules; the main ruleset is the
//! ruleset of the static anchor `pf_main_anchor`, which is in no tree. An anchor lives while
//! rules call it (`refcnt`), tables or a transaction use its ruleset, it has children or rules,
//! and transactions hold it (`ref`); `pf_remove_if_empty_ruleset` frees what became empty,
//! climbing to the parents.
//!
//! Paths and names are C strings: the functions take byte slices and read them up to their
//! first NUL (or their end), as `strlcpy` does.
//!
//! ## Deviations
//! - Only the `_KERNEL` half of the file is here. The `!_KERNEL` half (`rs_malloc` as `calloc`,
//!   `rs_pool_get_anchor` as `calloc`, no `refcnt(9)`) is the build `pfctl(8)` makes of the
//!   same C file; userland compiles it from the C source.
//! - `pf_anchor_compare` and the `RB_GENERATE` of `pf_anchor_global` and `pf_anchor_node` are in
//!   `net/pfvar.rs` (the comparator and the tree adapters `PfAnchorGlobal`, `PfAnchorNode`),
//!   beside the trees' types.
//! - `pf_main_ruleset` (`#define pf_main_ruleset pf_main_anchor.ruleset`) is the function
//!   [`pf_main_ruleset`].
//! - `rs_malloc`/`rs_free` hand out an [`RsBuf`], whose `Drop` is `rs_free`; the C's explicit
//!   `rs_free` calls stay at their sites.
//! - `pf_find_anchor` does not allocate a key anchor: it descends `pf_anchors` comparing the
//!   path as `pf_anchor_compare` would (the C's `RB_FIND` with a `malloc`ed key, whose
//!   allocation could fail and make the lookup miss).
//! - `pf_get_leaf_ruleset`'s `char **path_remainder` is the returned offset into `path`.
//! - `pf_anchor_setup` and `pf_anchor_copyout` return `bool`: `true` for the C's 0 (success),
//!   `false` for its 1 (failure): a caller's `if (pf_anchor_setup(...))` is
//!   `if !pf_anchor_setup(...)`.
//! - A ruleset other than the main one whose `anchor` is NULL (the C would dereference it)
//!   ends `pf_remove_if_empty_ruleset` and makes `pf_anchor_copyout` take its path as empty.

use core::cmp::Ordering;
use core::ops::{Deref, DerefMut};
use core::ptr::{self, NonNull};

use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_synch::{refcnt_init, refcnt_rele, refcnt_take};
use crate::kern::subr_pool::{pool_get, pool_put};
use crate::kern::subr_prf::Str;
use crate::net::pfvar::{
    PF_ANCHOR_MAXPATH, PF_ANCHOR_NAME_SIZE, PfAnchor, PfAnchorGlobal, PfRule, PfRuleset, PfiocRule,
    pf_cstr,
};
use crate::net::pfvar_priv::PfGlobal;
use crate::sys::malloc::{M_CANFAIL, M_PF, M_WAITOK, M_ZERO};
use crate::sys::param::MAXPATHLEN;
use crate::sys::pool::{PR_LIMITFAIL, PR_WAITOK, PR_ZERO, Pool};
use crate::sys::syslimits::PATH_MAX;
use crate::sys::syslog::LOG_NOTICE;
use crate::sys::tree::RbHead;
use libkern::{strlcat, strlcpy};

/// A buffer from `rs_malloc`, zeroed, given back by `rs_free` (or when dropped).
pub struct RsBuf {
    p: NonNull<u8>,
    len: usize,
}

impl Deref for RsBuf {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        // SAFETY: `p` is a live `malloc` block of `len` bytes, zeroed by `M_ZERO`, owned by
        // this buffer until it is dropped.
        unsafe { core::slice::from_raw_parts(self.p.as_ptr(), self.len) }
    }
}

impl DerefMut for RsBuf {
    fn deref_mut(&mut self) -> &mut [u8] {
        // SAFETY: as for `deref`; `&mut self` makes the access unique.
        unsafe { core::slice::from_raw_parts_mut(self.p.as_ptr(), self.len) }
    }
}

impl Drop for RsBuf {
    fn drop(&mut self) {
        free(self.p, M_PF, self.len);
    }
}

/// `pf_anchor_pl`: the anchors; `pfattach` (`net/pf_ioctl.rs`) initialises it.
pub static PF_ANCHOR_PL: Pool = Pool::new();

/// `pf_anchors`: every anchor but the main one, by path.
pub static PF_ANCHORS: PfGlobal<RbHead<PfAnchorGlobal>> = PfGlobal(RbHead::new());

/// `pf_main_anchor`: the anchor of the main ruleset; in no tree, never freed.
pub static PF_MAIN_ANCHOR: PfAnchor = PfAnchor::new();

/// `pf_main_ruleset`: `pf_main_anchor.ruleset`.
pub fn pf_main_ruleset() -> &'static PfRuleset {
    &PF_MAIN_ANCHOR.ruleset
}

/// `rs_malloc(x)`: `malloc(x, M_PF, M_WAITOK|M_CANFAIL|M_ZERO)`.
fn rs_malloc(x: usize) -> Option<RsBuf> {
    let p = malloc(x, M_PF, M_WAITOK | M_CANFAIL | M_ZERO)?;
    Some(RsBuf { p, len: x })
}

/// `rs_free(x, siz)`: `free(x, M_PF, siz)`; the size is the buffer's.
fn rs_free(x: RsBuf) {
    drop(x);
}

/// `rs_pool_get_anchor()`: a zeroed item of `pf_anchor_pl`, not yet shared, so that the
/// caller may fill in the members that are not `Cell`s (`name`, `path`).
fn rs_pool_get_anchor() -> Option<NonNull<PfAnchor>> {
    pool_get(&PF_ANCHOR_PL, PR_WAITOK | PR_LIMITFAIL | PR_ZERO).map(NonNull::cast)
}

/// `rs_pool_put_anchor(x)`.
fn rs_pool_put_anchor(x: &PfAnchor) {
    pool_put(&PF_ANCHOR_PL, NonNull::from(x).cast());
}

/// `strrchr(s, '/')` within the C string at the start of `s`: the offset of the last slash.
fn strrchr_slash(s: &[u8]) -> Option<usize> {
    pf_cstr(s).iter().rposition(|&c| c == b'/')
}

/// `pf_init_ruleset`: empties `ruleset` (`memset` 0), its queues empty, `queues[0]` active
/// and `queues[1]` inactive.
pub fn pf_init_ruleset(ruleset: &PfRuleset) {
    ruleset.queues[0].init();
    ruleset.queues[1].init();
    for (rules, ptr) in [(&ruleset.active, 0), (&ruleset.inactive, 1)] {
        rules.ptr.set(ptr);
        rules.rcount.set(0);
        rules.version.set(0);
        rules.open.set(0);
    }
    ruleset.anchor.set(None);
    ruleset.tticket.set(0);
    ruleset.tables.set(0);
    ruleset.topen.set(0);
}

/// `pf_find_anchor`: the anchor whose path is `path`.
pub fn pf_find_anchor(path: &[u8]) -> Option<&'static PfAnchor> {
    // The C copies the path into a key anchor with strlcpy, which truncates it.
    let path = pf_cstr(path);
    let key = &path[..path.len().min(PATH_MAX - 1)];
    let mut n = PF_ANCHORS.root();
    while let Some(a) = n {
        n = match key.cmp(pf_cstr(&a.path)) {
            Ordering::Less => RbHead::<PfAnchorGlobal>::left(a),
            Ordering::Greater => RbHead::<PfAnchorGlobal>::right(a),
            Ordering::Equal => return Some(a),
        };
    }
    None
}

/// `pf_find_ruleset`: the ruleset of the anchor `path` (leading slashes ignored), the main
/// ruleset for an empty path.
pub fn pf_find_ruleset(path: &[u8]) -> Option<&'static PfRuleset> {
    let mut path = pf_cstr(path);
    while let [b'/', rest @ ..] = path {
        path = rest;
    }
    if path.is_empty() {
        return Some(pf_main_ruleset());
    }
    pf_find_anchor(path).map(|anchor| &anchor.ruleset)
}

/// `pf_get_leaf_ruleset`: the ruleset of the longest existing prefix of the anchor path in
/// `path` (the main ruleset when none exists), and the offset in `path` of the remainder: the
/// components still to be created, after a slash or (when no prefix exists) at the start.
/// `path` is changed while searching and restored.
pub fn pf_get_leaf_ruleset(path: &mut [u8]) -> (&'static PfRuleset, usize) {
    let mut i = 0;

    let mut p = 0;
    while path.get(p) == Some(&b'/') {
        p += 1;
    }

    let mut ruleset = pf_find_ruleset(&path[p..]);
    let mut leaf = p;
    let ruleset = loop {
        if let Some(rs) = ruleset {
            break rs;
        }
        if let Some(slash) = strrchr_slash(&path[p..]) {
            leaf = p + slash;
            path[leaf] = 0;
            i += 1;
            ruleset = pf_find_ruleset(&path[p..]);
        } else {
            leaf = 0;
            // if no path component exists, then main ruleset is our parent.
            ruleset = Some(pf_main_ruleset());
        }
    };

    let path_remainder = leaf;

    // restore slashes in path.
    while i != 0 {
        while leaf < path.len() && path[leaf] != 0 {
            leaf += 1;
        }
        if leaf < path.len() {
            path[leaf] = b'/';
        }
        i -= 1;
    }

    (ruleset, path_remainder)
}

/// `pf_create_anchor`: a new anchor `aname` under `parent` (top level without one), in the
/// global tree and the parent's children, with an empty ruleset. `None` when the name is empty
/// or too long, the parent's path too long, memory ran out, or the anchor exists.
pub fn pf_create_anchor(
    parent: Option<&'static PfAnchor>,
    aname: &[u8],
) -> Option<&'static PfAnchor> {
    let aname = pf_cstr(aname);
    if aname.is_empty()
        || aname.len() >= PF_ANCHOR_NAME_SIZE
        || parent.is_some_and(|parent| pf_cstr(&parent.path).len() >= PF_ANCHOR_MAXPATH)
    {
        return None;
    }

    let a = rs_pool_get_anchor()?.as_ptr();
    // SAFETY: `a` is a fresh, zeroed item of `pf_anchor_pl`, sized and aligned for a
    // `PfAnchor` (`pfattach`'s `pool_init`), and all-zero is a valid `PfAnchor` (integers,
    // `Cell`s of them and of `Option<&T>`, tree links and heads, a `Refcnt`). Nothing else
    // refers to it yet, so `name` and `path` (not `Cell`s) are written through the raw
    // pointer before the first shared reference is made; it stays allocated until
    // `rs_pool_put_anchor`.
    let anchor: &'static PfAnchor = unsafe {
        let name = &mut *ptr::addr_of_mut!((*a).name);
        let path = &mut *ptr::addr_of_mut!((*a).path);
        strlcpy(name, aname);
        if let Some(parent) = parent {
            // Make sure path for levels 2, 3, ... is terminated by '/':
            //	1/2/3/...
            strlcpy(path, &parent.path);
            strlcat(path, b"/");
        }
        strlcat(path, name);
        &*a
    };
    anchor.children.init();

    // SAFETY: the new anchor is in no tree; it stays in place until `rs_pool_put_anchor`,
    // which only happens once it is out of the trees again.
    if let Some(dup) = unsafe { PF_ANCHORS.insert(anchor) } {
        crate::dpfprintf!(
            LOG_NOTICE,
            "{}: RB_INSERT to global '{}' '{}' collides with '{}' '{}'",
            "pf_create_anchor",
            Str(&anchor.path),
            Str(&anchor.name),
            Str(&dup.path),
            Str(&dup.name)
        );
        rs_pool_put_anchor(anchor);
        return None;
    }

    if let Some(parent) = parent {
        anchor.parent.set(Some(parent));
        // SAFETY: as above, for the parent's children.
        if let Some(dup) = unsafe { parent.children.insert(anchor) } {
            crate::dpfprintf!(
                LOG_NOTICE,
                "{}: RB_INSERT to parent '{}' '{}' collides with '{}' '{}'",
                "pf_create_anchor",
                Str(&anchor.path),
                Str(&anchor.name),
                Str(&dup.path),
                Str(&dup.name)
            );
            // SAFETY: inserted into the global tree just above.
            unsafe { PF_ANCHORS.remove(anchor) };
            rs_pool_put_anchor(anchor);
            return None;
        }
    }

    pf_init_ruleset(&anchor.ruleset);
    anchor.ruleset.anchor.set(Some(anchor));
    refcnt_init(&anchor.ref_);

    Some(anchor)
}

/// `pf_find_or_create_ruleset`: the ruleset of the anchor `path`, creating the anchors of its
/// missing components. `None` when an anchor cannot be created.
pub fn pf_find_or_create_ruleset(path: &[u8]) -> Option<&'static PfRuleset> {
    let mut path = pf_cstr(path);
    if path.is_empty() {
        return Some(pf_main_ruleset());
    }

    while let [b'/', rest @ ..] = path {
        path = rest;
    }

    if let Some(ruleset) = pf_find_ruleset(path) {
        return Some(ruleset);
    }

    let mut p = rs_malloc(MAXPATHLEN)?;
    strlcpy(&mut p, path);

    let (ruleset, mut aname) = pf_get_leaf_ruleset(&mut p);
    let mut anchor = ruleset.anchor.get();

    while p.get(aname) == Some(&b'/') {
        aname += 1;
    }
    // aname is a path remainder, which contains nodes we must create. We process the aname
    // path from left to right, effectively descending from parents to children.
    loop {
        let rest = pf_cstr(&p[aname..]);
        let r = rest.iter().position(|&c| c == b'/');
        if r.is_none() && rest.is_empty() {
            break;
        }
        let component = &rest[..r.unwrap_or(rest.len())];

        anchor = pf_create_anchor(anchor, component);
        if anchor.is_none() {
            rs_free(p);
            return None;
        }

        match r {
            None => break,
            Some(r) => aname += r + 1,
        }
    }

    rs_free(p);
    anchor.map(|anchor| &anchor.ruleset)
}

/// `pf_remove_if_empty_ruleset`: frees the anchor of `ruleset` if nothing uses it any more,
/// then its parent's, and so on up.
pub fn pf_remove_if_empty_ruleset(ruleset: &'static PfRuleset) {
    let mut ruleset = ruleset;
    loop {
        if ptr::eq(ruleset, pf_main_ruleset()) {
            return;
        }
        let Some(anchor) = ruleset.anchor.get() else {
            return;
        };
        if !anchor.children.is_empty()
            || anchor.refcnt.get() > 0
            || ruleset.tables.get() > 0
            || ruleset.topen.get() != 0
        {
            return;
        }
        if !ruleset.active_ptr().is_empty()
            || !ruleset.inactive_ptr().is_empty()
            || ruleset.inactive.open.get() != 0
        {
            return;
        }
        // SAFETY: an anchor with a ruleset other than the main one is in the global tree
        // (`pf_create_anchor`), and in its parent's children when it has a parent.
        unsafe { PF_ANCHORS.remove(anchor) };
        let parent = anchor.parent.get();
        if let Some(parent) = parent {
            // SAFETY: as above.
            unsafe { parent.children.remove(anchor) };
        }
        pf_anchor_rele(Some(anchor));
        let Some(parent) = parent else {
            return;
        };
        ruleset = &parent.ruleset;
    }
}

/// `pf_anchor_setup`: points the rule `r` of ruleset `s` at the anchor `name` calls (absolute
/// with a leading slash, else relative to `s`'s anchor, with `../` steps and a trailing `/*`
/// wildcard), creating it if needed, and takes a reference on it. `true` (the C's 0) on
/// success, `false` (the C's 1) without memory, for `..` beyond the root, or without a ruleset.
pub fn pf_anchor_setup(r: &PfRule, s: &PfRuleset, name: &[u8]) -> bool {
    let mut name = pf_cstr(name);

    r.set_anchor(None);
    r.anchor_relative.set(0);
    r.anchor_wildcard.set(0);
    if name.is_empty() {
        return true;
    }
    let Some(mut path) = rs_malloc(MAXPATHLEN) else {
        return false;
    };
    if name[0] == b'/' {
        strlcpy(&mut path, &name[1..]);
    } else {
        // relative path
        r.anchor_relative.set(1);
        match s.anchor.get() {
            Some(anchor) if anchor.path[0] != 0 => {
                strlcpy(&mut path, &anchor.path);
            }
            _ => path[0] = 0,
        }
        while name.starts_with(b"../") {
            if path[0] == 0 {
                crate::dpfprintf!(LOG_NOTICE, "pf_anchor_setup: .. beyond root");
                rs_free(path);
                return false;
            }
            match strrchr_slash(&path) {
                Some(p) => path[p] = 0,
                None => path[0] = 0,
            }
            r.anchor_relative
                .set(r.anchor_relative.get().wrapping_add(1));
            name = &name[3..];
        }
        if path[0] != 0 {
            strlcat(&mut path, b"/");
        }
        strlcat(&mut path, name);
    }
    if let Some(p) = strrchr_slash(&path)
        && pf_cstr(&path[p..]) == b"/*"
    {
        r.anchor_wildcard.set(1);
        path[p] = 0;
    }
    let ruleset = pf_find_or_create_ruleset(&path);
    rs_free(path);
    let Some(ruleset) = ruleset.filter(|rs| !ptr::eq(*rs, pf_main_ruleset())) else {
        crate::dpfprintf!(LOG_NOTICE, "pf_anchor_setup: ruleset");
        return false;
    };
    let anchor = ruleset.anchor.get();
    r.set_anchor(anchor);
    if let Some(anchor) = anchor {
        anchor.refcnt.set(anchor.refcnt.get() + 1);
    }
    true
}

/// `pf_anchor_copyout`: writes into `pr.anchor_call` the anchor call of rule `r` of ruleset
/// `rs`, as `pf_anchor_setup` was given it (absolute, or relative with `../` steps, and the
/// wildcard). `true` (the C's 0) on success, `false` (the C's 1) without memory or when the
/// anchor is not under the ruleset's.
pub fn pf_anchor_copyout(rs: &'static PfRuleset, r: &'static PfRule, pr: &mut PfiocRule) -> bool {
    pr.anchor_call[0] = 0;
    let Some(anchor) = r.anchor() else {
        return true;
    };
    if r.anchor_relative.get() == 0 {
        strlcpy(&mut pr.anchor_call, b"/");
        strlcat(&mut pr.anchor_call, &anchor.path);
    } else {
        let Some(mut a) = rs_malloc(MAXPATHLEN) else {
            return false;
        };
        if ptr::eq(rs, pf_main_ruleset()) {
            a[0] = 0;
        } else if let Some(rsa) = rs.anchor.get() {
            strlcpy(&mut a, &rsa.path);
        }
        for _ in 1..r.anchor_relative.get() {
            let p = strrchr_slash(&a).unwrap_or(0);
            a[p] = 0;
            strlcat(&mut pr.anchor_call, b"../");
        }
        let alen = pf_cstr(&a).len();
        let path = pf_cstr(&anchor.path);
        if !path.starts_with(pf_cstr(&a)) {
            crate::dpfprintf!(
                LOG_NOTICE,
                "pf_anchor_copyout: '{}' '{}'",
                Str(&a),
                Str(&anchor.path)
            );
            rs_free(a);
            return false;
        }
        if path.len() > alen {
            let skip = if a[0] != 0 { alen + 1 } else { 0 };
            strlcat(&mut pr.anchor_call, &path[skip..]);
        }
        rs_free(a);
    }
    if r.anchor_wildcard.get() != 0 {
        let wild: &[u8] = if pr.anchor_call[0] != 0 { b"/*" } else { b"*" };
        strlcat(&mut pr.anchor_call, wild);
    }
    true
}

/// `pf_remove_anchor`: drops the reference rule `r` holds on its anchor, freeing the anchor's
/// ruleset if it became empty.
pub fn pf_remove_anchor(r: &'static PfRule) {
    let Some(anchor) = r.anchor() else {
        return;
    };
    if anchor.refcnt.get() <= 0 {
        crate::dpfprintf!(LOG_NOTICE, "pf_remove_anchor: broken refcount");
    } else {
        anchor.refcnt.set(anchor.refcnt.get() - 1);
        if anchor.refcnt.get() == 0 {
            pf_remove_if_empty_ruleset(&anchor.ruleset);
        }
    }
    r.set_anchor(None);
}

/// `pf_anchor_rele`: drops a reference on `anchor` (`refcnt(9)`), freeing it with the last.
/// The main anchor is never freed.
pub fn pf_anchor_rele(anchor: Option<&'static PfAnchor>) {
    let Some(anchor) = anchor else {
        return;
    };
    if ptr::eq(anchor, &PF_MAIN_ANCHOR) {
        return;
    }

    if refcnt_rele(&anchor.ref_) {
        rs_pool_put_anchor(anchor);
    }
}

/// `pf_anchor_take`: takes a reference on `anchor` (none for the main anchor) and returns it.
pub fn pf_anchor_take(anchor: Option<&'static PfAnchor>) -> Option<&'static PfAnchor> {
    if let Some(a) = anchor
        && !ptr::eq(a, &PF_MAIN_ANCHOR)
    {
        refcnt_take(&a.ref_);
    }
    anchor
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
pub(crate) mod tests {
    // Host tests for anchors and rulesets: anchor paths creating their chain of anchors, lookups,
    // the leaf ruleset of a partly existing path, `pf_anchor_setup` with absolute, relative and
    // wildcard calls (and `pf_anchor_copyout` giving them back), and the removal of empty
    // anchors.

    use std::boxed::Box;
    use std::sync::MutexGuard;

    use super::*;
    use crate::kern::subr_pool::pool_init;
    use crate::kern::subr_pool::tests::setup_real_memory;
    use crate::machine::intr::IPL_SOFTNET;
    use crate::net::pfvar::{PfiocRule, pf_abi_zeroed};

    /// Fresh memory, `pf_anchor_pl` initialised as `pfattach` does, and empty anchor trees.
    pub(crate) fn setup() -> MutexGuard<'static, ()> {
        let guard = setup_real_memory();
        crate::machine::cons::consinit();
        pool_init(
            &PF_ANCHOR_PL,
            size_of::<PfAnchor>(),
            align_of::<PfAnchor>() as u32,
            IPL_SOFTNET,
            0,
            "pfanchor",
            None,
        );
        PF_ANCHORS.init();
        pf_init_ruleset(pf_main_ruleset());
        guard
    }

    fn rule() -> &'static PfRule {
        Box::leak(pf_abi_zeroed::<PfRule>())
    }

    fn path(a: &PfAnchor) -> &[u8] {
        pf_cstr(&a.path)
    }

    fn anchor_of(rs: &PfRuleset) -> &'static PfAnchor {
        rs.anchor.get().expect("an anchored ruleset")
    }

    #[test]
    fn find_or_create_builds_the_chain() {
        let _g = setup();

        assert!(ptr::eq(
            pf_find_or_create_ruleset(b"").unwrap(),
            pf_main_ruleset()
        ));
        assert!(ptr::eq(pf_find_ruleset(b"///").unwrap(), pf_main_ruleset()));

        let rs = pf_find_or_create_ruleset(b"a/b/c\0junk").expect("created");
        let c = anchor_of(rs);
        assert_eq!(path(c), b"a/b/c");
        assert_eq!(pf_cstr(&c.name), b"c");
        let b = c.parent.get().expect("parent b");
        assert_eq!(path(b), b"a/b");
        let a = b.parent.get().expect("parent a");
        assert_eq!(path(a), b"a");
        assert!(a.parent.get().is_none());
        assert!(ptr::eq(a.ruleset.anchor.get().unwrap(), a));

        // Found again, with or without leading slashes; the intermediate ones exist too.
        assert!(ptr::eq(pf_find_ruleset(b"a/b/c").unwrap(), rs));
        assert!(ptr::eq(pf_find_ruleset(b"//a/b/c").unwrap(), rs));
        assert!(ptr::eq(pf_find_or_create_ruleset(b"/a/b/c").unwrap(), rs));
        assert!(ptr::eq(pf_find_ruleset(b"a/b").unwrap(), &b.ruleset));
        assert!(ptr::eq(pf_find_anchor(b"a").unwrap(), a));
        assert!(pf_find_ruleset(b"a/x").is_none());
        assert!(pf_find_anchor(b"b").is_none());

        // The global tree holds the three, each parent its child.
        assert_eq!(PF_ANCHORS.iter().count(), 3);
        assert!(ptr::eq(a.children.root().unwrap(), b));
        assert!(ptr::eq(b.children.root().unwrap(), c));

        // A sibling shares the existing prefix.
        let d = anchor_of(pf_find_or_create_ruleset(b"a/d").unwrap());
        assert!(ptr::eq(d.parent.get().unwrap(), a));
        assert_eq!(PF_ANCHORS.iter().count(), 4);

        // Slashes before the components to create are skipped, as in C.
        let e = anchor_of(pf_find_or_create_ruleset(b"a//e").unwrap());
        assert_eq!(path(e), b"a/e");
        // Empty and over-long names are refused (the C keeps the anchors made before).
        assert!(pf_find_or_create_ruleset(b"x//y").is_none());
        assert!(pf_find_anchor(b"x").is_some());
        let long = [b'n'; PF_ANCHOR_NAME_SIZE];
        assert!(pf_create_anchor(None, &long).is_none());
        assert!(pf_create_anchor(None, b"").is_none());
        // An existing anchor collides.
        assert!(pf_create_anchor(None, b"a").is_none());
    }

    #[test]
    fn leaf_ruleset_of_a_partial_path() {
        let _g = setup();
        let ab = pf_find_or_create_ruleset(b"a/b").unwrap();

        let mut buf = [0u8; 32];
        buf[..7].copy_from_slice(b"a/b/x/y");
        let (rs, rem) = pf_get_leaf_ruleset(&mut buf);
        assert!(ptr::eq(rs, ab));
        assert_eq!(pf_cstr(&buf[rem..]), b"/x/y");
        assert_eq!(pf_cstr(&buf), b"a/b/x/y", "slashes restored");

        let mut buf = [0u8; 32];
        buf[..6].copy_from_slice(b"//a/b/");
        let (rs, rem) = pf_get_leaf_ruleset(&mut buf);
        assert!(ptr::eq(rs, ab));
        assert_eq!(pf_cstr(&buf[rem..]), b"/");
        assert_eq!(pf_cstr(&buf), b"//a/b/");

        let mut buf = [0u8; 32];
        buf[..3].copy_from_slice(b"q/r");
        let (rs, rem) = pf_get_leaf_ruleset(&mut buf);
        assert!(ptr::eq(rs, pf_main_ruleset()));
        assert_eq!(rem, 0);
        assert_eq!(pf_cstr(&buf), b"q/r");
    }

    #[test]
    fn anchor_setup_and_copyout() {
        let _g = setup();
        let ab = pf_find_or_create_ruleset(b"a/b").unwrap();
        let mut pr = pf_abi_zeroed::<PfiocRule>();

        // Relative, one level up.
        let r = rule();
        assert!(pf_anchor_setup(r, ab, b"../x"));
        let x = r.anchor().expect("anchor set");
        assert_eq!(path(x), b"a/x");
        assert_eq!(r.anchor_relative.get(), 2);
        assert_eq!(r.anchor_wildcard.get(), 0);
        assert_eq!(x.refcnt.get(), 1);
        assert!(pf_anchor_copyout(ab, r, &mut pr));
        assert_eq!(pf_cstr(&pr.anchor_call), b"../x");

        // Relative, below the ruleset's anchor.
        let r2 = rule();
        assert!(pf_anchor_setup(r2, ab, b"c"));
        assert_eq!(path(r2.anchor().unwrap()), b"a/b/c");
        assert_eq!(r2.anchor_relative.get(), 1);
        assert!(pf_anchor_copyout(ab, r2, &mut pr));
        assert_eq!(pf_cstr(&pr.anchor_call), b"c");

        // Absolute.
        let r3 = rule();
        assert!(pf_anchor_setup(r3, ab, b"/abs/y"));
        assert_eq!(path(r3.anchor().unwrap()), b"abs/y");
        assert_eq!(r3.anchor_relative.get(), 0);
        assert!(pf_anchor_copyout(ab, r3, &mut pr));
        assert_eq!(pf_cstr(&pr.anchor_call), b"/abs/y");

        // Wildcard from the main ruleset.
        let r4 = rule();
        assert!(pf_anchor_setup(r4, pf_main_ruleset(), b"a/*"));
        let a = r4.anchor().unwrap();
        assert_eq!(path(a), b"a");
        assert_eq!(r4.anchor_wildcard.get(), 1);
        assert_eq!(r4.anchor_relative.get(), 1);
        assert!(pf_anchor_copyout(pf_main_ruleset(), r4, &mut pr));
        assert_eq!(pf_cstr(&pr.anchor_call), b"a/*");

        // No call at all.
        let r5 = rule();
        assert!(pf_anchor_setup(r5, ab, b""));
        assert!(r5.anchor().is_none());
        assert!(pf_anchor_copyout(ab, r5, &mut pr));
        assert_eq!(pf_cstr(&pr.anchor_call), b"");

        // Failures: `..` beyond the root, and the main ruleset itself.
        let r6 = rule();
        assert!(!pf_anchor_setup(r6, pf_main_ruleset(), b"../x"));
        assert!(!pf_anchor_setup(r6, pf_main_ruleset(), b"/"));
        assert!(r6.anchor().is_none());

        // pf_remove_anchor drops the reference and frees the now unused anchor.
        pf_remove_anchor(r);
        assert!(r.anchor().is_none());
        assert!(pf_find_anchor(b"a/x").is_none());
        assert!(pf_find_anchor(b"a").is_some(), "a still has children");
    }

    #[test]
    fn remove_if_empty_climbs_to_the_root() {
        let _g = setup();
        let rs = pf_find_or_create_ruleset(b"a/b/c").unwrap();
        let c = anchor_of(rs);
        let b = c.parent.get().unwrap();

        // A reference on b keeps it (and a) once c is gone.
        b.refcnt.set(1);
        pf_remove_if_empty_ruleset(rs);
        assert!(pf_find_anchor(b"a/b/c").is_none());
        assert!(pf_find_anchor(b"a/b").is_some());
        assert!(b.children.is_empty());

        // Tables keep a ruleset too.
        b.refcnt.set(0);
        b.ruleset.tables.set(1);
        pf_remove_if_empty_ruleset(&b.ruleset);
        assert!(pf_find_anchor(b"a/b").is_some());

        b.ruleset.tables.set(0);
        pf_remove_if_empty_ruleset(&b.ruleset);
        assert!(pf_find_anchor(b"a/b").is_none());
        assert!(pf_find_anchor(b"a").is_none());
        assert!(PF_ANCHORS.is_empty());

        // The main ruleset is never removed.
        pf_remove_if_empty_ruleset(pf_main_ruleset());
        pf_anchor_rele(Some(&PF_MAIN_ANCHOR));
        assert!(pf_anchor_take(Some(&PF_MAIN_ANCHOR)).is_some());
    }

    #[test]
    fn transaction_reference_outlives_removal() {
        let _g = setup();
        let rs = pf_find_or_create_ruleset(b"t").unwrap();
        let t = anchor_of(rs);
        // A transaction holds the anchor: removal unlinks it, the last rele frees it.
        assert!(ptr::eq(pf_anchor_take(Some(t)).unwrap(), t));
        pf_remove_if_empty_ruleset(rs);
        assert!(pf_find_anchor(b"t").is_none());
        assert_eq!(PF_ANCHOR_PL.pr_nout.get(), 1);
        pf_anchor_rele(Some(t));
        assert_eq!(PF_ANCHOR_PL.pr_nout.get(), 0);
    }
}
/* </TESTS> */
