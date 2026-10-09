/* <LICENSES> */
/*
 * Copyright (c) 2017-2019 Bob Beck <beck@openbsd.org>
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

/* <CODE> */
//! `unveil(2)`'s kernel side: the per-process table of unveiled directory vnodes, each with
//! a red-black tree of the names unveiled beneath it, and the checks `namei` makes against
//! it (`unveil_start_relative`, `unveil_check_component`, `unveil_check_final`). The system
//! call itself, `sys_unveil`, is `vfs_syscalls.c`'s.
//!
//! Upstream: sys/kern/kern_unveil.c @ 3ce1f3f79392
//!
//! A process's table is `ps_uvpaths`: `UNVEIL_MAX_VNODES` [`Unveil`] slots from
//! `mallocarray(M_PROC)`, of which the first `ps_uvvcount` are in use. Each slot holds a
//! referenced directory vnode (`uv_vp`, also counted in the vnode's `v_uvcount`), the slot
//! index of the unveil covering it from above (`uv_cover`, -1 for none), its own flags
//! (`uv_flags`, 0 while only names beneath it were unveiled) and the names (`uv_names`).
//! The table is freed only by `unveil_destroy`, which exit, exec and `pledge(2)` call with the
//! process single-threaded, so the references this module hands out (and the pointer a lookup
//! keeps in `ni_unveil_match`) stay valid for the system call that obtained them.
//!
//! ## Deviations
//! - `DEBUG_UNVEIL` is not defined upstream: the `DPRINTF`s are not carried over.
//! - `malloc(M_WAITOK)` cannot fail in C; here it can return NULL in one corner (`KMEMSTATS`
//!   over `ks_limit`, which cannot sleep yet). The allocations panic then, rather than letting
//!   a process run with fewer unveils than its parent or than it asked for.
//! - `unvname_new` keeps the C's NUL-terminated copy (`un_namesize` counts the NUL); the
//!   names passed around are byte slices without it. `unvname_compare` compares sizes, then
//!   bytes, as the C's `memcmp` does.
//! - `unveil_parsepermissions` returns the flags or `EINVAL` (the C returns -1 and fills an
//!   out-parameter, and `unveil_add` turns the -1 into `EINVAL`).
//! - `unveil_find_cover`: at the root of a file system mounted on nothing (`mnt_vnodecovered`
//!   NULL) that is not the process's root, the C's inner loop would spin forever; it stops
//!   instead, and the `..` lookup there returns the root itself, which ends the walk with -1.
//! - `unveil_add_vnode` re-checks the covers of the other slots; a slot whose vnode
//!   `unveil_removevnode` zapped has no vnode to walk up from (the C would follow NULL) and
//!   gets cover -1.

use core::cell::Cell;
use core::cmp::Ordering as CmpOrdering;
use core::mem::size_of;
use core::ptr::{self, NonNull};
use core::slice;

use crate::kassert;
use crate::kern::kern_malloc::{free, malloc, mallocarray};
use crate::kern::kern_proc::ALLPROCESS;
use crate::kern::kern_rwlock::{
    rw_enter_read, rw_enter_write, rw_exit_read, rw_exit_write, rw_init,
};
use crate::kern::subr_prf::panic;
use crate::kern::vfs_init::rootvnode;
use crate::kern::vfs_subr::{vget, vput, vref, vrele};
use crate::kern::vfs_vops::VOP_LOOKUP;
use crate::sys::acct::AUNVEIL;
use crate::sys::errno::Errno;
use crate::sys::lock::{LK_EXCLUSIVE, LK_RETRY};
use crate::sys::malloc::{M_PROC, M_WAITOK, M_ZERO};
use crate::sys::namei::{
    BYPASSUNVEIL, Componentname, HASBUF, ISDOTDOT, ISLASTCN, LOOKUP, Nameidata, PDIRUNLOCK, RDONLY,
    UNVEIL_CREATE, UNVEIL_EXEC, UNVEIL_MASK, UNVEIL_READ, UNVEIL_USERSET, UNVEIL_WRITE,
};
use crate::sys::param::MAXPATHLEN;
use crate::sys::pledge::PLEDGE_UNVEIL;
use crate::sys::proc::{Proc, Process};
use crate::sys::rwlock::Rwlock;
use crate::sys::tree::{RbtEntry, RbtHead};
use crate::sys::vnode::{VDIR, VROOT, Vnode};
use crate::tree_adapter;

/// `UNVEIL_MAX_VNODES`: the slots of a process's unveil table.
pub const UNVEIL_MAX_VNODES: usize = 128;
/// `UNVEIL_MAX_NAMES`: the names a process may unveil beneath its directories.
pub const UNVEIL_MAX_NAMES: usize = 128;

/// `struct unvname`: a name unveiled beneath a directory, with its flags.
pub struct Unvname {
    /// `un_name`: the name and its NUL (`un_namesize` bytes from `malloc(M_PROC)`).
    un_name: *mut u8,
    /// `un_namesize`: the bytes of `un_name`, NUL included.
    un_namesize: usize,
    /// `un_flags`: `UNVEIL_*` (written under the tree's `uv_lock`).
    pub un_flags: Cell<u8>,
    /// `un_rbt`: the links of `uv_names`.
    pub un_rbt: RbtEntry,
}

impl Unvname {
    /// The name, without its NUL.
    pub fn name(&self) -> &[u8] {
        if self.un_name.is_null() || self.un_namesize == 0 {
            return &[];
        }
        // SAFETY: `un_name` points at `un_namesize` bytes: the `malloc`ed copy of
        // `unvname_new`, or the caller's slice for a lookup key (`unveil_namelookup`), both
        // alive as long as the `Unvname`.
        unsafe { slice::from_raw_parts(self.un_name, self.un_namesize - 1) }
    }
}

tree_adapter!(
    /// `RBT_HEAD(unvname_rbt, unvname)`: the names beneath one unveiled directory, through
    /// `un_rbt`, ordered by `unvname_compare`.
    pub UnvnameRbt: Unvname, un_rbt => RbtEntry, unvname_compare
);

/// `struct unveil`: one unveiled directory vnode.
pub struct Unveil {
    /// `uv_vp`: the directory, referenced; NULL once `unveil_removevnode` zapped it.
    pub uv_vp: Cell<Option<&'static Vnode>>,
    /// `uv_cover`: the slot of the unveil covering this one from above, -1 for none.
    pub uv_cover: Cell<isize>,
    /// `uv_names`: the names unveiled beneath the directory (`uv_lock`).
    pub uv_names: RbtHead<UnvnameRbt>,
    /// `uv_lock`: protects `uv_names`.
    pub uv_lock: Rwlock,
    /// `uv_flags`: `UNVEIL_*` for the directory itself; 0 while only names beneath it are.
    pub uv_flags: Cell<u8>,
}

impl Unveil {
    /// A zeroed slot, as `mallocarray(M_ZERO)` leaves it (the lock is named by `rw_init`).
    pub const fn new() -> Self {
        Self {
            uv_vp: Cell::new(None),
            uv_cover: Cell::new(0),
            uv_names: RbtHead::new(),
            uv_lock: Rwlock::new("unveil"),
            uv_flags: Cell::new(0),
        }
    }
}

impl Default for Unveil {
    fn default() -> Self {
        Self::new()
    }
}

/// `pr->ps_uvpaths` as the slice of its `UNVEIL_MAX_VNODES` slots, or `None` while NULL.
fn uvpaths(pr: &Process) -> Option<&[Unveil]> {
    let paths = pr.ps_uvpaths.get();
    if paths.is_null() {
        return None;
    }
    // SAFETY: a non-null `ps_uvpaths` is the array `unveil_alloc` made: `UNVEIL_MAX_VNODES`
    // initialised slots. Only `unveil_destroy` frees it, with the process single-threaded
    // and outside any lookup (see the module's documentation), so no reference made here
    // outlives it.
    Some(unsafe { slice::from_raw_parts(paths, UNVEIL_MAX_VNODES) })
}

/// `mallocarray(UNVEIL_MAX_VNODES, sizeof(struct unveil), M_PROC, M_WAITOK|M_ZERO)`.
fn unveil_alloc() -> *mut Unveil {
    let Some(mem) = mallocarray(
        UNVEIL_MAX_VNODES,
        size_of::<Unveil>(),
        M_PROC,
        M_WAITOK | M_ZERO,
    ) else {
        panic(format_args!("unveil: out of memory (M_WAITOK)"));
    };
    let paths = mem.cast::<Unveil>().as_ptr();
    for i in 0..UNVEIL_MAX_VNODES {
        // SAFETY: a fresh allocation of `UNVEIL_MAX_VNODES` slots, aligned by `malloc` (at
        // least 16 bytes); each slot is written once before anything reads it.
        unsafe { paths.add(i).write(Unveil::new()) };
    }
    paths
}

/// The `ni_unveil_match` of a lookup by process `pr`.
fn unveil_match<'a>(ni: &Nameidata<'_>, _pr: &'a Process) -> Option<&'a Unveil> {
    // SAFETY: the hooks of this module set `ni_unveil_match` only to NULL or to a slot of
    // `pr->ps_uvpaths` (the lookup's own process), which stays allocated for the lookup (see
    // `uvpaths`).
    unsafe { ni.ni_unveil_match.as_ref() }
}

/// `a == b` for two optional vnodes.
fn vp_eq(a: Option<&Vnode>, b: Option<&Vnode>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => ptr::eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

/// The process's root for the walks up: its `chroot(2)` directory, else `rootvnode`.
fn proc_root(p: &Proc) -> Option<&'static Vnode> {
    p.fd().fd_rdir.get().or_else(rootvnode)
}

/// The NUL-terminated name at `cn_nameptr`, without its NUL: what `strlen(cn_nameptr)`
/// measures in the C.
fn nameptr_str(cnp: &Componentname) -> &[u8] {
    let base = cnp.cn_pnbuf;
    let name = cnp.cn_nameptr;
    if base.is_null() || name.is_null() {
        return cnp.name();
    }
    // SAFETY: both point into or at the pathname buffer; the offset is checked below.
    let off = unsafe { name.offset_from(base) };
    if off < 0 || off as usize >= MAXPATHLEN {
        return cnp.name();
    }
    // SAFETY: `cn_pnbuf` is the lookup's `MAXPATHLEN`-byte `namei_pool` item (kept by
    // `SAVENAME`, or still held while `namei` checks the result), and `cn_nameptr` points
    // into it at `off`; nobody writes it meanwhile.
    let rest = unsafe { slice::from_raw_parts(name, MAXPATHLEN - off as usize) };
    let len = rest.iter().position(|&c| c == 0).unwrap_or(rest.len());
    &rest[..len]
}

/// `unvname_compare(n1, n2)`: by size, then by the bytes of the name.
pub fn unvname_compare(n1: &Unvname, n2: &Unvname) -> CmpOrdering {
    if n1.un_namesize == n2.un_namesize {
        n1.name().cmp(n2.name())
    } else {
        n1.un_namesize.cmp(&n2.un_namesize)
    }
}

/// `unvname_new(name, size, flags)`: a name for a tree; `name` is without its NUL (`size` in
/// the C is `strlen(name) + 1`).
pub fn unvname_new(name: &[u8], flags: u8) -> &'static Unvname {
    let size = name.len() + 1;
    let (Some(ret), Some(buf)) = (
        malloc(size_of::<Unvname>(), M_PROC, M_WAITOK),
        malloc(size, M_PROC, M_WAITOK),
    ) else {
        panic(format_args!("unveil: out of memory (M_WAITOK)"));
    };
    // SAFETY: `buf` is a fresh `size`-byte allocation; `name` is `size - 1` bytes elsewhere.
    unsafe {
        ptr::copy_nonoverlapping(name.as_ptr(), buf.as_ptr(), name.len());
        buf.as_ptr().add(name.len()).write(0);
    }
    let ret = ret.cast::<Unvname>();
    // SAFETY: a fresh allocation of `size_of::<Unvname>()` bytes, aligned by `malloc` (at
    // least 16 bytes); it lives until `unvname_delete`.
    unsafe {
        ret.as_ptr().write(Unvname {
            un_name: buf.as_ptr(),
            un_namesize: size,
            un_flags: Cell::new(flags),
            un_rbt: RbtEntry::new(),
        });
        &*ret.as_ptr()
    }
}

/// `unvname_delete(name)`: frees a name that is in no tree.
pub fn unvname_delete(name: &Unvname) {
    let (buf, size) = (name.un_name, name.un_namesize);
    if let Some(buf) = NonNull::new(buf) {
        free(buf, M_PROC, size);
    }
    free(
        NonNull::from(name).cast::<u8>(),
        M_PROC,
        size_of::<Unvname>(),
    );
}

/// `unveil_delete_names(uv)`: frees every name beneath `uv`; returns how many there were.
pub fn unveil_delete_names(uv: &Unveil) -> usize {
    let mut ret = 0;

    rw_enter_write(&uv.uv_lock);
    for unvn in uv.uv_names.iter() {
        // SAFETY: `unvn` is in this tree; the iterator has already read its successor.
        unsafe { uv.uv_names.remove(unvn) };
        unvname_delete(unvn);
        ret += 1;
    }
    rw_exit_write(&uv.uv_lock);

    ret
}

/// `unveil_add_name_unlocked(uv, name, flags)`: adds `name` beneath `uv` unless it is there
/// already; true when it was added. The caller holds `uv_lock` (or owns `uv` alone).
pub fn unveil_add_name_unlocked(uv: &Unveil, name: &[u8], flags: u8) -> bool {
    let unvn = unvname_new(name, flags);
    // SAFETY: a new name in no tree; it stays allocated until `unvname_delete`, which runs
    // only after its removal.
    if unsafe { uv.uv_names.insert(unvn) }.is_some() {
        // Name already present.
        unvname_delete(unvn);
        return false;
    }

    true
}

/// `unveil_add_name(uv, name, flags)`: `unveil_add_name_unlocked` under `uv_lock`.
pub fn unveil_add_name(uv: &Unveil, name: &[u8], flags: u8) -> bool {
    rw_enter_write(&uv.uv_lock);
    let ret = unveil_add_name_unlocked(uv, name, flags);
    rw_exit_write(&uv.uv_lock);
    ret
}

/// `unveil_namelookup(uv, name)`: the entry for `name` beneath `uv`.
pub fn unveil_namelookup<'a>(uv: &'a Unveil, name: &[u8]) -> Option<&'a Unvname> {
    rw_enter_read(&uv.uv_lock);

    kassert!(uv.uv_vp.get().is_some());

    let n = Unvname {
        un_name: name.as_ptr().cast_mut(),
        un_namesize: name.len() + 1,
        un_flags: Cell::new(0),
        un_rbt: RbtEntry::new(),
    };
    let ret = uv.uv_names.find(&n);

    rw_exit_read(&uv.uv_lock);

    ret
}

/// `unveil_destroy(ps)`: drops every unveil of the process, with the vnode references and
/// the names, and frees the table.
pub fn unveil_destroy(ps: &Process) {
    if let Some(paths) = uvpaths(ps) {
        let count = ps.ps_uvvcount.get().max(0) as usize;
        for uv in &paths[..count] {
            // skip any vnodes zapped by unveil_removevnode
            if let Some(vp) = uv.uv_vp.get() {
                vp.v_uvcount.set(vp.v_uvcount.get() - 1);
                vrele(vp);
            }
            ps.ps_uvncount
                .set(ps.ps_uvncount.get() - unveil_delete_names(uv));
            uv.uv_vp.set(None);
            uv.uv_flags.set(0);
        }
    }

    kassert!(ps.ps_uvncount.get() == 0);
    if let Some(paths) = NonNull::new(ps.ps_uvpaths.get()) {
        free(
            paths.cast::<u8>(),
            M_PROC,
            UNVEIL_MAX_VNODES * size_of::<Unveil>(),
        );
    }
    ps.ps_uvvcount.set(0);
    ps.ps_uvpaths.set(ptr::null_mut());
}

/// `unveil_copy(parent, child)`: gives a new process its parent's unveils (`fork1`).
pub fn unveil_copy(parent: &Process, child: &Process) {
    child.ps_uvdone.set(parent.ps_uvdone.get());
    if parent.ps_uvvcount.get() == 0 {
        return;
    }

    child.ps_uvpaths.set(unveil_alloc());
    let Some(to_paths) = uvpaths(child) else {
        return;
    };

    child.ps_uvncount.set(0);
    if let Some(from_paths) = uvpaths(parent) {
        let count = parent.ps_uvvcount.get().max(0) as usize;
        for (from, to) in from_paths[..count].iter().zip(to_paths) {
            to.uv_vp.set(from.uv_vp.get());
            if let Some(vp) = to.uv_vp.get() {
                vref(vp);
                vp.v_uvcount.set(vp.v_uvcount.get() + 1);
            }
            rw_init(&to.uv_lock, "unveil");
            to.uv_names.init();
            rw_enter_read(&from.uv_lock);
            for unvn in from.uv_names.iter() {
                if unveil_add_name_unlocked(to, unvn.name(), unvn.un_flags.get()) {
                    child.ps_uvncount.set(child.ps_uvncount.get() + 1);
                }
            }
            rw_exit_read(&from.uv_lock);
            to.uv_flags.set(from.uv_flags.get());
            to.uv_cover.set(from.uv_cover.get());
        }
    }
    child.ps_uvvcount.set(parent.ps_uvvcount.get());
}

/// Walk up from vnode `dp`, until we find a matching unveil, or the root vnode. Returns -1 if
/// no unveil is to be found above `dp` or if `dp` is the root vnode.
pub fn unveil_find_cover(dp: &'static Vnode, p: &Proc) -> isize {
    let mut ret: isize = -1;

    // use the correct root to stop at, chrooted or not..
    let root = proc_root(p);
    let mut vp = dp;

    while !vp_eq(Some(vp), root) {
        let mut cn = Componentname::new();
        cn.cn_nameiop = LOOKUP;
        cn.cn_flags = ISLASTCN | ISDOTDOT | RDONLY;
        cn.cn_proc = p;
        cn.cn_cred = p.p_ucred.get();
        cn.cn_pnbuf = ptr::null_mut();
        cn.cn_nameptr = b"..".as_ptr();
        cn.cn_namelen = 2;
        cn.cn_consume = 0;

        // If we are at the root of a filesystem, and we are still mounted somewhere, take
        // the .. in the above filesystem.
        while !vp_eq(Some(vp), root) && vp.v_flag.get() & VROOT != 0 {
            let Some(mp) = vp.v_mount.get() else {
                return -1;
            };
            match mp.mnt_vnodecovered.get() {
                Some(covered) => vp = covered,
                // The C would spin here (see the module's deviations).
                None => break,
            }
        }

        if vget(vp, LK_EXCLUSIVE | LK_RETRY).is_err() {
            return -1;
        }
        // Get parent vnode of vp using lookup of '..'
        // This returns with vp unlocked but ref'ed
        let mut parent: Option<&'static Vnode> = None;
        if VOP_LOOKUP(vp, &mut parent, &mut cn).is_err() {
            if cn.cn_flags & PDIRUNLOCK == 0 {
                vput(vp);
            } else {
                // This corner case should not happen because we have not set LOCKPARENT
                // in the flags
                vrele(vp);
            }
            break;
        }
        let Some(parent) = parent else {
            panic(format_args!("unveil_find_cover: no vnode found"));
        };

        vrele(vp);
        let _ = unveil_lookup(parent, p.process(), Some(&mut ret));
        vput(parent);

        if ret >= 0 {
            break;
        }

        if ptr::eq(vp, parent) {
            ret = -1;
            break;
        }
        vp = parent;
    }
    ret
}

/// `unveil_lookup(vp, pr, position)`: the process's unveil of directory `vp`, and its slot
/// in `*position` (-1 for none).
pub fn unveil_lookup<'a>(
    vp: &Vnode,
    pr: &'a Process,
    mut position: Option<&mut isize>,
) -> Option<&'a Unveil> {
    if let Some(pos) = position.as_deref_mut() {
        *pos = -1;
    }

    if vp.v_uvcount.get() == 0 {
        return None;
    }

    let uv = uvpaths(pr)?;
    let count = pr.ps_uvvcount.get().max(0) as usize;
    for (i, u) in uv[..count].iter().enumerate() {
        if vp_eq(Some(vp), u.uv_vp.get()) {
            kassert!(vp.v_uvcount.get() > 0);
            kassert!(vp.v_usecount.get() > 0);
            if let Some(pos) = position {
                *pos = i as isize;
            }
            return Some(u);
        }
    }
    None
}

/// `unveil_parsepermissions(permissions, perms)`: the `UNVEIL_*` flags of a permission
/// string (`r`, `w`, `x`, `c`; it ends at its first NUL or at the slice's end), with
/// `UNVEIL_USERSET`; `EINVAL` for any other character.
pub fn unveil_parsepermissions(permissions: &[u8]) -> Result<u8, Errno> {
    let mut perms = UNVEIL_USERSET;
    for &c in permissions.iter().take_while(|&&c| c != 0) {
        match c {
            b'r' => perms |= UNVEIL_READ,
            b'w' => perms |= UNVEIL_WRITE,
            b'x' => perms |= UNVEIL_EXEC,
            b'c' => perms |= UNVEIL_CREATE,
            _ => return Err(Errno::EINVAL),
        }
    }
    Ok(perms)
}

/// `unveil_add_vnode(p, vp)`: a new slot for directory `vp` (referenced by the caller), with
/// its cover, re-checking the covers it may have interposed itself under.
pub fn unveil_add_vnode<'a>(p: &'a Proc, vp: &'static Vnode) -> &'a Unveil {
    let pr = p.process();
    let Some(paths) = uvpaths(pr) else {
        panic(format_args!("unveil_add_vnode: no unveil table"));
    };

    kassert!(pr.ps_uvvcount.get() < UNVEIL_MAX_VNODES as isize);

    let idx = pr.ps_uvvcount.get() as usize;
    pr.ps_uvvcount.set(pr.ps_uvvcount.get() + 1);
    let uv = &paths[idx];
    rw_init(&uv.uv_lock, "unveil");
    uv.uv_names.init();
    uv.uv_vp.set(Some(vp));
    uv.uv_flags.set(0);

    // find out what we are covered by
    uv.uv_cover.set(unveil_find_cover(vp, p));

    // Find anyone covered by what we are covered by and re-check what covers them (we could
    // have interposed a cover)
    for other in &paths[..idx] {
        if other.uv_cover.get() == uv.uv_cover.get() {
            let cover = match other.uv_vp.get() {
                Some(ovp) => unveil_find_cover(ovp, p),
                // zapped by unveil_removevnode (see the module's deviations)
                None => -1,
            };
            other.uv_cover.set(cover);
        }
    }

    uv
}

/// `unveil_add(p, ndp, permissions)`: unveils what `ndp` (a `SAVENAME` lookup by
/// `sys_unveil`) found: the directory itself, or the last name beneath its parent.
pub fn unveil_add(p: &Proc, ndp: &Nameidata<'_>, permissions: &[u8]) -> Result<(), Errno> {
    let pr = p.process();

    kassert!(ndp.ni_cnd.cn_flags & HASBUF != 0); // must have SAVENAME

    let flags = unveil_parsepermissions(permissions)?;

    if pr.ps_uvpaths.get().is_null() {
        pr.ps_uvpaths.set(unveil_alloc());
    }

    if pr.ps_uvvcount.get() >= UNVEIL_MAX_VNODES as isize
        || pr.ps_uvncount.get() >= UNVEIL_MAX_NAMES
    {
        return Err(Errno::E2BIG);
    }

    // Are we a directory? or something else
    let directory_add = ndp.ni_vp.is_some_and(|vp| vp.v_type.get() == VDIR);

    let vp = if directory_add { ndp.ni_vp } else { ndp.ni_dvp };
    let Some(vp) = vp else {
        panic(format_args!("unveil_add: no directory vnode"));
    };
    let name = nameptr_str(&ndp.ni_cnd);

    kassert!(vp.v_type.get() == VDIR);
    vref(vp);
    vp.v_uvcount.set(vp.v_uvcount.get() + 1);
    let uv = match unveil_lookup(vp, pr, None) {
        Some(uv) => {
            // We already have unveiled this directory vnode
            vp.v_uvcount.set(vp.v_uvcount.get() - 1);
            vrele(vp);

            // If we are adding a directory which was already unveiled containing only
            // specific terminals, unrestrict it.
            if directory_add {
                uv.uv_flags.set(flags);
                return Ok(());
            }

            // If we are adding a terminal that is already unveiled, just replace the flags
            // and we are done
            if let Some(tname) = unveil_namelookup(uv, name) {
                tname.un_flags.set(flags);
                return Ok(());
            }
            uv
        }
        // New unveil involving this directory vnode.
        None => unveil_add_vnode(p, vp),
    };

    // At this stage with have a unveil in uv with a vnode for a directory. If the component
    // we are adding is a directory, we are done. Otherwise, we add the component name the
    // name list in uv.
    if directory_add {
        uv.uv_flags.set(flags);
        return Ok(());
    }

    if unveil_add_name(uv, name, flags) {
        pr.ps_uvncount.set(pr.ps_uvncount.get() + 1);
    }

    Ok(())
}

/// `unveil_flagmatch(ni, flags)`: whether `flags` allow every access the lookup asks for in
/// `ni_unveil`; no access at all for flags 0.
pub fn unveil_flagmatch(ni: &Nameidata<'_>, flags: u8) -> bool {
    if flags == 0 {
        // All operations forbidden for 0 flags
        return false;
    }
    [UNVEIL_READ, UNVEIL_WRITE, UNVEIL_EXEC, UNVEIL_CREATE]
        .into_iter()
        .all(|f| ni.ni_unveil & f == 0 || flags & f != 0)
}

/// When traversing up towards the root figure out the proper unveil for the parent
/// directory.
pub fn unveil_covered<'a>(
    uv: Option<&'a Unveil>,
    dvp: Option<&Vnode>,
    p: &'a Proc,
) -> Option<&'a Unveil> {
    let u = uv?;
    if vp_eq(u.uv_vp.get(), dvp) {
        // if at the root, chrooted or not, return the current uv
        if vp_eq(dvp, proc_root(p)) {
            return uv;
        }
        let cover = u.uv_cover.get();
        if cover >= 0 {
            let pr = p.process();
            kassert!(cover < pr.ps_uvvcount.get());
            return uvpaths(pr).map(|paths| &paths[cover as usize]);
        }
        return None;
    }
    uv
}

/// Start a relative path lookup. Ensure we find whatever unveil covered where we start from,
/// either by having a saved current working directory unveil, or by walking up and finding a
/// cover the hard way if we are doing a non `AT_FDCWD` relative lookup.
pub fn unveil_start_relative(p: &Proc, ni: &mut Nameidata<'_>, dp: &'static Vnode) {
    let pr = p.process();

    if pr.ps_uvpaths.get().is_null() {
        return;
    }

    let mut uv = unveil_lookup(dp, pr, None);
    if uv.is_none() {
        let uvi = unveil_find_cover(dp, p);
        if uvi >= 0 {
            kassert!(uvi < pr.ps_uvvcount.get());
            uv = uvpaths(pr).map(|paths| &paths[uvi as usize]);
        }
    }

    // Store this match for later use. Flags are checked at the end.
    if let Some(uv) = uv {
        ni.ni_unveil_match = uv;
    }
}

/// unveil checking - for component directories in a namei lookup.
pub fn unveil_check_component(p: &Proc, ni: &mut Nameidata<'_>, dp: &'static Vnode) {
    let pr = p.process();

    if ni.ni_pledge == PLEDGE_UNVEIL || pr.ps_uvpaths.get().is_null() {
        return;
    }
    if ni.ni_cnd.cn_flags & BYPASSUNVEIL != 0 {
        return;
    }

    let uv = if ni.ni_cnd.cn_flags & ISDOTDOT != 0 {
        // adjust unveil match as necessary
        let cur = unveil_match(ni, pr);
        let uv = unveil_covered(cur, Some(dp), p);

        // clear the match when we DOTDOT above it
        if cur.is_some_and(|m| vp_eq(m.uv_vp.get(), Some(dp))) {
            ni.ni_unveil_match = ptr::null();
        }
        uv
    } else {
        unveil_lookup(dp, pr, None)
    };

    if let Some(uv) = uv {
        // update match
        ni.ni_unveil_match = uv;
    }
}

/// The refusal of a `UNVEIL_USERSET` unveil whose flags do not match: `EACCES` when it allows
/// something, `ENOENT` when it hides everything (`unveil(path, "")`).
fn unveil_refuse(pr: &Process, flags: u8) -> Errno {
    pr.ps_acflag.set(pr.ps_acflag.get() | AUNVEIL);
    if flags & UNVEIL_MASK != 0 {
        Errno::EACCES
    } else {
        Errno::ENOENT
    }
}

/// unveil checking - only done after namei lookup has succeeded on the last component of a
/// namei lookup.
pub fn unveil_check_final(p: &Proc, ni: &mut Nameidata<'_>) -> Result<(), Errno> {
    let pr = p.process();

    if ni.ni_pledge == PLEDGE_UNVEIL || pr.ps_uvpaths.get().is_null() {
        return Ok(());
    }

    if ni.ni_cnd.cn_flags & BYPASSUNVEIL != 0 {
        return Ok(());
    }

    'done: {
        if let Some(vp) = ni.ni_vp
            && vp.v_type.get() == VDIR
        {
            // We are matching a directory terminal component
            let uv = unveil_lookup(vp, pr, None);
            let Some(uv) = uv.filter(|uv| uv.uv_flags.get() & UNVEIL_USERSET != 0) else {
                if let Some(uv) = uv {
                    ni.ni_unveil_match = uv;
                }
                break 'done;
            };
            if !unveil_flagmatch(ni, uv.uv_flags.get()) {
                return Err(unveil_refuse(pr, uv.uv_flags.get()));
            }
            // directory and flags match, success
            return Ok(());
        }

        // Otherwise, we are matching a non-terminal component
        let Some(uv) = ni.ni_dvp.and_then(|dvp| unveil_lookup(dvp, pr, None)) else {
            break 'done;
        };
        let Some(tname) = unveil_namelookup(uv, nameptr_str(&ni.ni_cnd)) else {
            // no specific name, so check unveil directory flags
            if !unveil_flagmatch(ni, uv.uv_flags.get()) {
                // If dir has user set restrictions fail with EACCES or ENOENT. Otherwise,
                // use any covering match that we found above this dir.
                if uv.uv_flags.get() & UNVEIL_USERSET != 0 {
                    return Err(unveil_refuse(pr, uv.uv_flags.get()));
                }
                // start backtrack from this node
                ni.ni_unveil_match = uv;
                break 'done;
            }
            // directory flags match, success
            return Ok(());
        };
        if !unveil_flagmatch(ni, tname.un_flags.get()) {
            // do flags match for matched name
            pr.ps_acflag.set(pr.ps_acflag.get() | AUNVEIL);
            return Err(Errno::EACCES);
        }
        // name and flags match. success
        return Ok(());
    }

    // done: last component did not match, check previous matches if access is allowed or
    // not.
    let mut uv = unveil_match(ni, pr);
    while let Some(u) = uv {
        if unveil_flagmatch(ni, u.uv_flags.get()) {
            return Ok(());
        }
        // if node has any flags set then this is an access violation
        if u.uv_flags.get() & UNVEIL_USERSET != 0 {
            return Err(unveil_refuse(pr, u.uv_flags.get()));
        }

        let nuv = unveil_covered(Some(u), u.uv_vp.get(), p);
        if nuv.is_some_and(|n| ptr::eq(n, u)) {
            break;
        }
        uv = nuv;
    }
    pr.ps_acflag.set(pr.ps_acflag.get() | AUNVEIL);
    Err(Errno::ENOENT)
}

/// Scan all active processes to see if any of them have a unveil to this vnode. If so, NULL
/// the vnode in their unveil list, vrele, drop the reference, and mark their unveil list as
/// needing to have the hole shrunk the next time the process uses it for lookup.
pub fn unveil_removevnode(vp: &'static Vnode) {
    if vp.v_uvcount.get() == 0 {
        return;
    }

    vref(vp); // make sure it is held till we are done

    for pr in ALLPROCESS.0.iter() {
        if let Some(uv) = unveil_lookup(vp, pr, None)
            && uv.uv_vp.get().is_some()
        {
            uv.uv_vp.set(None);
            uv.uv_flags.set(0);

            if vp.v_uvcount.get() > 0 {
                vrele(vp);
                vp.v_uvcount.set(vp.v_uvcount.get() - 1);
            } else {
                panic(format_args!(
                    "vp {:p}, v_uvcount of {} should be 0",
                    vp,
                    vp.v_uvcount.get()
                ));
            }
        }
    }
    kassert!(vp.v_uvcount.get() == 0);

    vrele(vp); // release our ref
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for `kern_unveil.c`: the permission strings, the flag matching, the name tree,
    // and `unveil(2)` end to end over `testfs` (`vfs_subr.rs`): `sys_unveil` adds
    // directories and names, and `namei` then allows, refuses (`EACCES`) or hides (`ENOENT`)
    // what lies under them, through covers, `..`, symbolic links and relative lookups; fork's
    // copy, exec's and exit's destroy and unmount's `unveil_removevnode` give back every vnode
    // reference they took.

    use std::{assert, assert_eq, boxed::Box, vec::Vec};

    use super::*;
    use crate::kern::vfs_lookup::{namei, ndinit};
    use crate::kern::vfs_subr::tests::testfs::{self, A, C, LONG, N, ROOT, any_locked, usecounts};
    use crate::kern::vfs_syscalls::sys_unveil;
    use crate::machine::Machine;
    use crate::machine::cpu::Cpu;
    use crate::sys::namei::{FOLLOW, NiDirp};
    use crate::sys::proc::ProcessList;
    use crate::sys::queue::ListHead;
    use crate::sys::systm::SysArgs;
    use crate::sys::types::Register;

    #[test]
    fn parsepermissions_takes_rwxc_and_nothing_else() {
        assert_eq!(unveil_parsepermissions(b""), Ok(UNVEIL_USERSET));
        assert_eq!(
            unveil_parsepermissions(b"r"),
            Ok(UNVEIL_USERSET | UNVEIL_READ)
        );
        assert_eq!(
            unveil_parsepermissions(b"cxwr"),
            Ok(UNVEIL_USERSET | UNVEIL_READ | UNVEIL_WRITE | UNVEIL_EXEC | UNVEIL_CREATE)
        );
        assert_eq!(
            unveil_parsepermissions(b"rr"),
            Ok(UNVEIL_USERSET | UNVEIL_READ)
        );
        // The string ends at its NUL, as copyinstr leaves it.
        assert_eq!(
            unveil_parsepermissions(b"w\0x"),
            Ok(UNVEIL_USERSET | UNVEIL_WRITE)
        );
        for bad in [&b"rz"[..], b"R", b" ", b"rwxc-"] {
            assert_eq!(unveil_parsepermissions(bad), Err(Errno::EINVAL));
        }
    }

    #[test]
    fn flagmatch_requires_every_requested_bit() {
        let p: &'static Proc = Box::leak(Box::new(Proc::new()));
        let mut ni = ndinit(LOOKUP, 0, NiDirp::Sys(b"x"), p);
        let all = UNVEIL_READ | UNVEIL_WRITE | UNVEIL_EXEC | UNVEIL_CREATE;
        for want in 0..=all {
            ni.ni_unveil = want;
            // Flags 0 forbid everything, even a lookup that asks for nothing.
            assert!(!unveil_flagmatch(&ni, 0));
            for have in 0..=all {
                let flags = have | UNVEIL_USERSET;
                assert_eq!(
                    unveil_flagmatch(&ni, flags),
                    want & !have == 0,
                    "want {want:#x} have {have:#x}"
                );
            }
        }
    }

    #[test]
    fn the_name_tree_orders_by_size_then_bytes() {
        let (_g, _p, _mp) = testfs::setup_root();
        let uv: &'static Unveil = Box::leak(Box::new(Unveil::new()));
        // unveil_namelookup asserts a vnode under DIAGNOSTIC; any directory will do here.
        uv.uv_vp.set(rootvnode());
        for name in [&b"zz"[..], b"b", b"a", b"abc", b"ab"] {
            assert!(unveil_add_name(uv, name, UNVEIL_READ));
        }
        // Already there: not added again, the flags untouched.
        assert!(!unveil_add_name(uv, b"ab", UNVEIL_WRITE));
        let names: Vec<&[u8]> = uv.uv_names.iter().map(Unvname::name).collect();
        assert_eq!(names, [&b"a"[..], b"b", b"ab", b"zz", b"abc"]);

        let n1 = unvname_new(b"ab", 0);
        let n2 = unvname_new(b"b", 0);
        assert_eq!(unvname_compare(n1, n2), CmpOrdering::Greater);
        assert_eq!(unvname_compare(n2, n1), CmpOrdering::Less);
        unvname_delete(n1);
        unvname_delete(n2);

        assert_eq!(
            unveil_namelookup(uv, b"ab").map(|n| n.un_flags.get()),
            Some(UNVEIL_READ)
        );
        assert!(unveil_namelookup(uv, b"abd").is_none());
        assert!(unveil_namelookup(uv, b"").is_none());
        assert_eq!(unveil_delete_names(uv), 5);
        assert!(uv.uv_names.is_empty());
    }

    /// `unveil(path, permissions)` through the system call, `None` for a NULL argument.
    fn unveil(p: &Proc, path: Option<&[u8]>, perms: Option<&[u8]>) -> Result<(), Errno> {
        let cstr = |s: Option<&[u8]>| -> Register {
            s.map_or(0, |s| {
                let mut v = s.to_vec();
                v.push(0);
                Box::leak(v.into_boxed_slice()).as_ptr() as Register
            })
        };
        let mut v: SysArgs = [0; 6];
        v[0] = cstr(path);
        v[1] = cstr(perms);
        let mut retval = [0; 2];
        sys_unveil(p, &v, &mut retval)
    }

    /// `namei` of `path` asking for `want` (`ni_unveil`): `Ok` with the vnode released, or the
    /// error.
    fn access(p: &Proc, path: &[u8], want: u8) -> Result<(), Errno> {
        let path: &'static [u8] = Box::leak(path.to_vec().into_boxed_slice());
        let mut nd = ndinit(LOOKUP, FOLLOW, NiDirp::Sys(path), p);
        nd.ni_unveil = want;
        namei(&mut nd)?;
        if let Some(vp) = nd.ni_vp {
            vrele(vp);
        }
        Ok(())
    }

    /// The test's lock, and `curproc` cleared again when the test ends.
    struct Guard {
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            Machine::set_curproc(Machine::curcpu(), ptr::null());
        }
    }

    /// testfs as root, with the thread as `curproc` and counted as its process's one thread
    /// (what `single_thread_set` expects).
    fn setup_unveil() -> (Guard, &'static Proc, &'static crate::sys::mount::Mount) {
        let (g, p, mp) = testfs::setup_root();
        p.process().ps_threadcnt.set(1);
        Machine::set_curproc(Machine::curcpu(), p);
        (Guard { _lock: g }, p, mp)
    }

    /// Every node's `v_uvcount`.
    fn uvcounts() -> [u32; N] {
        core::array::from_fn(|i| testfs::vnode_of(i).map_or(0, |vp| vp.v_uvcount.get()))
    }

    #[test]
    fn an_unveiled_directory_hides_the_rest() {
        let (_g, p, _mp) = setup_unveil();
        let pr = p.process();
        let before = usecounts();

        // Nothing unveiled: everything is visible.
        assert_eq!(access(p, b"/a/b", UNVEIL_READ | UNVEIL_WRITE), Ok(()));

        assert_eq!(unveil(p, Some(b"/a/c"), Some(b"r")), Ok(()));
        assert!(!pr.ps_uvpaths.get().is_null());
        assert_eq!(pr.ps_uvvcount.get(), 1);
        assert_eq!(uvcounts()[C], 1);

        assert_eq!(access(p, b"/a/c", UNVEIL_READ), Ok(()));
        assert_eq!(access(p, b"/a/c/.", UNVEIL_READ), Ok(()));
        // Through a symbolic link, which is looked up component by component.
        assert_eq!(access(p, b"/l", UNVEIL_READ), Ok(()));
        // Unveiled read-only: EACCES for more.
        assert_eq!(access(p, b"/a/c", UNVEIL_WRITE), Err(Errno::EACCES));
        assert_eq!(pr.ps_acflag.get() & AUNVEIL, AUNVEIL);
        // Everything else is gone.
        assert_eq!(access(p, b"/a/b", UNVEIL_READ), Err(Errno::ENOENT));
        assert_eq!(access(p, b"/", UNVEIL_READ), Err(Errno::ENOENT));
        assert_eq!(access(p, b"/a", UNVEIL_READ), Err(Errno::ENOENT));
        assert_eq!(access(p, b"/a/c/..", UNVEIL_READ), Err(Errno::ENOENT));
        assert_eq!(access(p, b"/a/c/../b", UNVEIL_READ), Err(Errno::ENOENT));
        // A lookup that asks for nothing (stat-like) is still refused outside.
        assert_eq!(access(p, b"/a/b", 0), Err(Errno::ENOENT));
        assert!(!any_locked());

        // Exec (no execpromises) and exit drop the table and every reference.
        unveil_destroy(pr);
        assert!(pr.ps_uvpaths.get().is_null());
        assert_eq!(pr.ps_uvvcount.get(), 0);
        assert_eq!(uvcounts(), [0; N]);
        assert_eq!(usecounts(), before);
        assert_eq!(access(p, b"/a/b", UNVEIL_READ), Ok(()));
    }

    #[test]
    fn names_beneath_a_directory_and_covers() {
        let (_g, p, _mp) = setup_unveil();
        let pr = p.process();
        let before = usecounts();

        // A file: its directory gets a slot with flags 0, the name its own flags.
        assert_eq!(unveil(p, Some(b"/a/b"), Some(b"rw")), Ok(()));
        assert_eq!(pr.ps_uvvcount.get(), 1);
        assert_eq!(pr.ps_uvncount.get(), 1);
        assert_eq!(uvcounts()[A], 1);
        assert_eq!(access(p, b"/a/b", UNVEIL_READ | UNVEIL_WRITE), Ok(()));
        assert_eq!(access(p, b"/a/b", UNVEIL_EXEC), Err(Errno::EACCES));
        // A sibling name was not unveiled, and nothing covers /a.
        let long: Vec<u8> = [&b"/a/"[..], testfs::NODES[LONG].name].concat();
        assert_eq!(access(p, &long, UNVEIL_READ), Err(Errno::ENOENT));
        assert_eq!(access(p, b"/a", UNVEIL_READ), Err(Errno::ENOENT));

        // Re-unveiling the name replaces its flags; no new name.
        assert_eq!(unveil(p, Some(b"/a/b"), Some(b"x")), Ok(()));
        assert_eq!(pr.ps_uvncount.get(), 1);
        assert_eq!(access(p, b"/a/b", UNVEIL_EXEC), Ok(()));
        assert_eq!(access(p, b"/a/b", UNVEIL_READ), Err(Errno::EACCES));

        // A name that does not exist yet can be unveiled for creation.
        assert_eq!(unveil(p, Some(b"/a/new"), Some(b"c")), Ok(()));
        assert_eq!(pr.ps_uvncount.get(), 2);
        assert_eq!(uvcounts()[A], 1);

        // Unveiling the root read-only covers /a: its other names become readable, not writable.
        assert_eq!(unveil(p, Some(b"/"), Some(b"r")), Ok(()));
        assert_eq!(pr.ps_uvvcount.get(), 2);
        assert_eq!(uvcounts()[ROOT], 1);
        assert_eq!(access(p, &long, UNVEIL_READ), Ok(()));
        assert_eq!(access(p, &long, UNVEIL_WRITE), Err(Errno::EACCES));
        assert_eq!(access(p, b"/a/c", UNVEIL_READ), Ok(()));
        assert_eq!(access(p, b"/a/c/..", UNVEIL_READ), Ok(()));
        // The name keeps its own flags.
        assert_eq!(access(p, b"/a/b", UNVEIL_READ), Err(Errno::EACCES));

        // Unveiling /a itself (a directory already in the table) sets its flags in place.
        assert_eq!(unveil(p, Some(b"/a"), Some(b"rwc")), Ok(()));
        assert_eq!(pr.ps_uvvcount.get(), 2);
        assert_eq!(uvcounts()[A], 1);
        assert_eq!(access(p, &long, UNVEIL_WRITE), Ok(()));
        // ... and "" hides it: ENOENT, not EACCES.
        assert_eq!(unveil(p, Some(b"/a"), Some(b"")), Ok(()));
        assert_eq!(access(p, b"/a", UNVEIL_READ), Err(Errno::ENOENT));
        assert!(!any_locked());

        unveil_destroy(pr);
        assert_eq!(pr.ps_uvncount.get(), 0);
        assert_eq!(uvcounts(), [0; N]);
        assert_eq!(usecounts(), before);
    }

    #[test]
    fn relative_lookups_start_from_the_cover_of_the_directory() {
        let (_g, p, mp) = setup_unveil();
        let pr = p.process();

        assert_eq!(unveil(p, Some(b"/a"), Some(b"r")), Ok(()));
        // chdir /a/c, which is not itself unveiled: covered by /a.
        let cdir = testfs::vget_node(mp, C).unwrap();
        let _ = crate::kern::vfs_vops::VOP_UNLOCK(cdir);
        let old = p.fd().fd_cdir.replace(Some(cdir));
        assert_eq!(access(p, b".", UNVEIL_READ), Ok(()));
        assert_eq!(access(p, b"../b", UNVEIL_READ), Ok(()));
        assert_eq!(access(p, b"../b", UNVEIL_WRITE), Err(Errno::EACCES));
        // Above the cover nothing is visible.
        assert_eq!(access(p, b"../..", UNVEIL_READ), Err(Errno::ENOENT));
        assert_eq!(access(p, b"../../l", UNVEIL_READ), Ok(())); // l -> a/c
        p.fd().fd_cdir.set(old);
        vrele(cdir);
        assert!(!any_locked());
        unveil_destroy(pr);
        assert_eq!(uvcounts(), [0; N]);
    }

    #[test]
    fn the_system_call_checks_its_arguments_and_locks() {
        let (_g, p, _mp) = setup_unveil();
        let pr = p.process();

        assert_eq!(unveil(p, Some(b"/a"), Some(b"rq")), Err(Errno::EINVAL));
        assert_eq!(unveil(p, Some(b""), Some(b"r")), Err(Errno::EINVAL));
        assert_eq!(
            unveil(p, Some(b"/"), Some(b"rwxcr")),
            Err(Errno::ENAMETOOLONG)
        );
        assert_eq!(unveil(p, Some(b"/nope/x"), Some(b"r")), Err(Errno::ENOENT));
        assert_eq!(unveil(p, None, Some(b"r")), Err(Errno::EFAULT));
        assert_eq!(pr.ps_single.get(), ptr::null());

        // E2BIG once the names are used up.
        pr.ps_uvncount.set(UNVEIL_MAX_NAMES);
        assert_eq!(unveil(p, Some(b"/a/b"), Some(b"r")), Err(Errno::E2BIG));
        pr.ps_uvncount.set(0);

        // unveil(NULL, NULL) locks the table.
        assert_eq!(unveil(p, None, None), Ok(()));
        assert_eq!(pr.ps_uvdone.get(), 1);
        assert_eq!(unveil(p, Some(b"/"), Some(b"r")), Err(Errno::EPERM));
        unveil_destroy(pr);
        assert_eq!(uvcounts(), [0; N]);
    }

    #[test]
    fn fork_copies_and_unmount_zaps() {
        let (_g, p, _mp) = setup_unveil();
        let pr = p.process();
        let before = usecounts();

        assert_eq!(unveil(p, Some(b"/a/c"), Some(b"r")), Ok(()));
        assert_eq!(unveil(p, Some(b"/a/b"), Some(b"w")), Ok(()));
        pr.ps_uvdone.set(1);

        let child: &'static Process = Box::leak(Box::new(Process::new()));
        unveil_copy(pr, child);
        assert_eq!(child.ps_uvdone.get(), 1);
        assert_eq!(child.ps_uvvcount.get(), 2);
        assert_eq!(child.ps_uvncount.get(), 1);
        assert_eq!(uvcounts()[C], 2);
        assert_eq!(uvcounts()[A], 2);
        let cuv = unveil_lookup(testfs::vnode_of(A).unwrap(), child, None).unwrap();
        assert_eq!(
            unveil_namelookup(cuv, b"b").map(|n| n.un_flags.get()),
            Some(UNVEIL_USERSET | UNVEIL_WRITE)
        );

        // A process with nothing unveiled copies nothing.
        let empty: &'static Process = Box::leak(Box::new(Process::new()));
        let grandchild: &'static Process = Box::leak(Box::new(Process::new()));
        unveil_copy(empty, grandchild);
        assert!(grandchild.ps_uvpaths.get().is_null());

        // Both processes on allprocess: unmount drops their unveils of /a/c.
        // SAFETY: two leaked processes in no list, taken off again below.
        unsafe {
            ALLPROCESS.0.insert_head(pr);
            ALLPROCESS.0.insert_head(child);
        }
        let cvp = testfs::vnode_of(C).unwrap();
        unveil_removevnode(cvp);
        assert_eq!(cvp.v_uvcount.get(), 0);
        let mut pos = 0;
        assert!(unveil_lookup(cvp, pr, Some(&mut pos)).is_none());
        assert_eq!(pos, -1);
        assert_eq!(pr.ps_uvvcount.get(), 2); // the slot stays, zapped
        // SAFETY: both are on the list (inserted above).
        unsafe {
            ListHead::<ProcessList>::remove(child);
            ListHead::<ProcessList>::remove(pr);
        }

        unveil_destroy(child);
        unveil_destroy(pr);
        assert_eq!(uvcounts(), [0; N]);
        assert_eq!(usecounts(), before);
    }

    #[test]
    fn find_cover_walks_up_to_the_nearest_unveil() {
        let (_g, p, _mp) = setup_unveil();
        let pr = p.process();
        let root = rootvnode().unwrap();
        assert_eq!(unveil(p, Some(b"/"), Some(b"r")), Ok(()));
        assert_eq!(unveil(p, Some(b"/a/c"), Some(b"rw")), Ok(()));
        let cvp = testfs::vnode_of(C).unwrap();
        // The root has no cover; /a/c is covered by the root's slot (0).
        assert_eq!(unveil_find_cover(root, p), -1);
        assert_eq!(unveil_find_cover(cvp, p), 0);
        assert_eq!(unveil_lookup(cvp, pr, None).unwrap().uv_cover.get(), 0);
        // Interposing /a re-checks /a/c's cover: slot 2.
        assert_eq!(unveil(p, Some(b"/a"), Some(b"r")), Ok(()));
        assert_eq!(unveil_lookup(cvp, pr, None).unwrap().uv_cover.get(), 2);
        assert_eq!(unveil_find_cover(testfs::vnode_of(A).unwrap(), p), 0);
        assert!(!any_locked());
        unveil_destroy(pr);
        assert_eq!(uvcounts(), [0; N]);
    }
}
/* </TESTS> */
