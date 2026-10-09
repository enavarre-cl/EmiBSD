/* <LICENSES> */
/*
 * Copyright (c) 1985, 1989, 1991, 1993
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
 *	@(#)namei.h	8.4 (Berkeley) 8/20/94
 */
/* </LICENSES> */

/* <CODE> */
//! `<sys/namei.h>`: the encapsulation of `namei` parameters (`struct nameidata` and the
//! `struct componentname` it hands to `VOP_LOOKUP`), the namei operations and flags, and the
//! name cache's `struct namecache` and statistics.
//!
//! Upstream: sys/sys/namei.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `ni_dirp` is a [`NiDirp`]: a user address or a kernel byte string; `ni_segflg` is kept
//!   and always agrees with it (`ndinitat` sets both). `NDINITAT`/`NDINIT` are `ndinitat`/
//!   `ndinit` (`kern/vfs_lookup.rs`), which return the structure instead of filling one.
//! - `cn_pnbuf`, `cn_rpbuf`, `cn_nameptr` and `ni_next` stay raw pointers, as in C: the
//!   lookup walks them with pointer arithmetic, and `cn_nameptr` may point at a string that
//!   is not in `cn_pnbuf` (`".."` in `vfs_getcwd_scandir`). Whoever sets `cn_nameptr` makes
//!   it point at `cn_namelen` readable bytes for the lookup's duration; [`Componentname::name`]
//!   relies on that, as the C code does.
//! - `cn_proc`, `cn_cred` are raw pointers with accessors that assert them non-null (the
//!   thread and its credentials outlive the lookup); `ni_unveil_match` is a raw pointer into
//!   the process's unveil table (`kern_unveil.rs`), NULL for no match.
//! - `struct namecache`'s members are `Cell`s; `nc_nlen` is a `u8` (a `char` holding a
//!   length of at most `NAMECACHE_MAXLEN`).
//! - `struct nchstats` is a structure of `AtomicU64`s (the C bumps them racily, as `uvmexp`);
//!   [`Nchstats::snapshot`] is what `kern.nchstats` copies out.
//! - The prototypes are their functions in `vfs_lookup.rs`, `vfs_cache.rs` and (the
//!   `unveil_*` ones) `kern_unveil.rs`; `namei_pool` is in `vfs_init.rs`.

use core::cell::Cell;
use core::ptr;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::kassert;
use crate::kern::kern_unveil::Unveil;
use crate::kern::vfs_cache::namecache_compare;
use crate::queue_adapter;
use crate::sys::proc::Proc;
use crate::sys::queue::TailqEntry;
use crate::sys::tree::RbtEntry;
use crate::sys::ucred::Ucred;
use crate::sys::uio::UioSeg;
use crate::sys::vnode::Vnode;
use crate::tree_adapter;

pub use crate::sys::fcntl::AT_FDCWD;

/// `ni_dirp`: where the pathname is.
#[derive(Clone, Copy)]
pub enum NiDirp<'a> {
    /// A user address (`UIO_USERSPACE`).
    User(usize),
    /// A kernel string (`UIO_SYSSPACE`); it ends at its first NUL or at the slice's end.
    Sys(&'a [u8]),
}

/// `struct componentname`: the subset of the `nameidata` structure that is passed through the
/// VOP interface.
pub struct Componentname {
    // Arguments to lookup.
    /// `cn_nameiop`: namei operation.
    pub cn_nameiop: u64,
    /// `cn_flags`: flags to namei.
    pub cn_flags: u64,
    /// `cn_proc`: process requesting lookup.
    pub cn_proc: *const Proc,
    /// `cn_cred`: credentials.
    pub cn_cred: *const Ucred,
    // Shared between lookup and commit routines.
    /// `cn_pnbuf`: pathname buffer (`MAXPATHLEN` bytes from `namei_pool`), or null.
    pub cn_pnbuf: *mut u8,
    /// `cn_rpbuf`: realpath buffer (`MAXPATHLEN` bytes), or null.
    pub cn_rpbuf: *mut u8,
    /// `cn_rpi`: realpath index.
    pub cn_rpi: usize,
    /// `cn_nameptr`: pointer to looked up name.
    pub cn_nameptr: *const u8,
    /// `cn_namelen`: length of looked up component.
    pub cn_namelen: i64,
    /// `cn_consume`: chars to consume in lookup().
    pub cn_consume: i64,
}

impl Componentname {
    /// A zeroed `struct componentname`.
    pub const fn new() -> Self {
        Self {
            cn_nameiop: 0,
            cn_flags: 0,
            cn_proc: ptr::null(),
            cn_cred: ptr::null(),
            cn_pnbuf: ptr::null_mut(),
            cn_rpbuf: ptr::null_mut(),
            cn_rpi: 0,
            cn_nameptr: ptr::null(),
            cn_namelen: 0,
            cn_consume: 0,
        }
    }

    /// `cnp->cn_proc`.
    pub fn proc(&self) -> &Proc {
        kassert!(!self.cn_proc.is_null());
        // SAFETY: set by `ndinitat` (or the caller that builds a componentname by hand) to
        // the thread doing the lookup, which outlives it.
        unsafe { &*self.cn_proc }
    }

    /// `cnp->cn_cred`.
    pub fn cred(&self) -> &Ucred {
        kassert!(!self.cn_cred.is_null());
        // SAFETY: `namei` sets it to the thread's `p_ucred`, which the thread holds for the
        // lookup's duration.
        unsafe { &*self.cn_cred }
    }

    /// The component being looked up: the `cn_namelen` bytes at `cn_nameptr`.
    pub fn name(&self) -> &[u8] {
        if self.cn_nameptr.is_null() || self.cn_namelen <= 0 {
            return &[];
        }
        // SAFETY: the module's deviations: whoever sets `cn_nameptr` makes it point at
        // `cn_namelen` readable bytes for the lookup's duration.
        unsafe { core::slice::from_raw_parts(self.cn_nameptr, self.cn_namelen as usize) }
    }
}

impl Default for Componentname {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct nameidata`: encapsulation of namei parameters.
pub struct Nameidata<'a> {
    // Arguments to namei/lookup.
    /// `ni_dirp`: pathname pointer.
    pub ni_dirp: NiDirp<'a>,
    /// `ni_dirfd`: dirfd from *at() functions.
    pub ni_dirfd: i32,
    /// `ni_segflg`: location of pathname.
    pub ni_segflg: UioSeg,
    // Arguments to lookup.
    /// `ni_startdir`: starting directory.
    pub ni_startdir: Option<&'static Vnode>,
    /// `ni_rootdir`: logical root directory.
    pub ni_rootdir: Option<&'static Vnode>,
    /// `ni_pledge`: expected pledge for namei.
    pub ni_pledge: u64,
    /// `ni_unveil`: required unveil flags for namei.
    pub ni_unveil: u8,
    // Results: returned from/manipulated by lookup.
    /// `ni_vp`: vnode of result.
    pub ni_vp: Option<&'static Vnode>,
    /// `ni_dvp`: vnode of intermediate directory.
    pub ni_dvp: Option<&'static Vnode>,
    // Shared between namei and lookup/commit routines.
    /// `ni_pathlen`: remaining chars in path.
    pub ni_pathlen: usize,
    /// `ni_next`: next location in pathname.
    pub ni_next: *const u8,
    /// `ni_loopcnt`: count of symlinks encountered.
    pub ni_loopcnt: u64,
    /// `ni_unveil_match`: last matching unveil component (a slot of the looking-up
    /// process's `ps_uvpaths`, or NULL).
    pub ni_unveil_match: *const Unveil,
    /// `ni_cnd`: lookup parameters.
    pub ni_cnd: Componentname,
}

/// `LOOKUP`: perform name lookup only.
pub const LOOKUP: u64 = 0;
/// `CREATE`: setup for file creation.
pub const CREATE: u64 = 1;
/// `DELETE`: setup for file deletion.
pub const DELETE: u64 = 2;
/// `RENAME`: setup for file renaming.
pub const RENAME: u64 = 3;
/// `OPMASK`: mask for operation.
pub const OPMASK: u64 = 3;

/// `LOCKLEAF`: lock inode on return.
pub const LOCKLEAF: u64 = 0x0004;
/// `LOCKPARENT`: want parent vnode returned locked.
pub const LOCKPARENT: u64 = 0x0008;
/// `WANTPARENT`: want parent vnode returned unlocked.
pub const WANTPARENT: u64 = 0x0010;
/// `NOCACHE`: name must not be left in cache.
pub const NOCACHE: u64 = 0x0020;
/// `FOLLOW`: follow symbolic links.
pub const FOLLOW: u64 = 0x0040;
/// `NOFOLLOW`: do not follow symbolic links (pseudo).
pub const NOFOLLOW: u64 = 0x0000;
/// `MODMASK`: mask of operational modifiers.
pub const MODMASK: u64 = 0x00fc;

/// `NOCROSSMOUNT`: do not cross mount points.
pub const NOCROSSMOUNT: u64 = 0x000100;
/// `RDONLY`: lookup with read-only semantics.
pub const RDONLY: u64 = 0x000200;
/// `HASBUF`: has allocated pathname buffer.
pub const HASBUF: u64 = 0x000400;
/// `SAVENAME`: save pathname buffer.
pub const SAVENAME: u64 = 0x000800;
/// `SAVESTART`: save starting directory.
pub const SAVESTART: u64 = 0x001000;
/// `ISDOTDOT`: current component name is `..`.
pub const ISDOTDOT: u64 = 0x002000;
/// `MAKEENTRY`: entry is to be added to name cache.
pub const MAKEENTRY: u64 = 0x004000;
/// `ISLASTCN`: this is last component of pathname.
pub const ISLASTCN: u64 = 0x008000;
/// `ISSYMLINK`: symlink needs interpretation.
pub const ISSYMLINK: u64 = 0x010000;
/// `REALPATH`: save pathname buffer for realpath.
pub const REALPATH: u64 = 0x020000;
/// `BPU_LOCALTIME`: /etc/localtime may cross 1 symlink.
pub const BPU_LOCALTIME: u64 = 0x040000;
/// `REQUIREDIR`: must be a directory.
pub const REQUIREDIR: u64 = 0x080000;
/// `STRIPSLASHES`: strip trailing slashes.
pub const STRIPSLASHES: u64 = 0x100000;
/// `PDIRUNLOCK`: vfs_lookup() unlocked parent dir.
pub const PDIRUNLOCK: u64 = 0x200000;
/// `BYPASSUNVEIL`: bypass pledgepath check.
pub const BYPASSUNVEIL: u64 = 0x400000;
/// `KERNELPATH`: access file as kernel, not process.
pub const KERNELPATH: u64 = 0x800000;
/// `BPU_ZONEINFO`: /usr/share/zoneinfo prohibits symlinks.
pub const BPU_ZONEINFO: u64 = 0x1000000;
/// `EXECPATH`: like `REALPATH`, but can give up.
pub const EXECPATH: u64 = 0x2000000;

/// `NAMECACHE_MAXLEN`: maximum name segment length we bother with.
pub const NAMECACHE_MAXLEN: usize = 31;

/// `struct namecache`: an element in the cache of recent names looked up by namei.
pub struct Namecache {
    /// `nc_lru`: Regular Entry LRU chain.
    pub nc_lru: TailqEntry<Namecache>,
    /// `nc_neg`: Negative Entry LRU chain.
    pub nc_neg: TailqEntry<Namecache>,
    /// `n_rbcache`: Namecache rb tree from vnode.
    pub n_rbcache: RbtEntry,
    /// `nc_me`: ncp's referring to me.
    pub nc_me: TailqEntry<Namecache>,
    /// `nc_dvp`: vnode of parent of name.
    pub nc_dvp: Cell<Option<&'static Vnode>>,
    /// `nc_dvpid`: capability number of `nc_dvp`.
    pub nc_dvpid: Cell<u64>,
    /// `nc_vp`: vnode the name refers to (NULL for a negative entry).
    pub nc_vp: Cell<Option<&'static Vnode>>,
    /// `nc_vpid`: capability number of `nc_vp`.
    pub nc_vpid: Cell<u64>,
    /// `nc_nlen`: length of name.
    pub nc_nlen: Cell<u8>,
    /// `nc_name`: segment name.
    pub nc_name: Cell<[u8; NAMECACHE_MAXLEN]>,
}

impl Namecache {
    /// A zeroed entry, as `pool_get(&nch_pool, PR_ZERO)` returns it.
    pub const fn new() -> Self {
        Self {
            nc_lru: TailqEntry::new(),
            nc_neg: TailqEntry::new(),
            n_rbcache: RbtEntry::new(),
            nc_me: TailqEntry::new(),
            nc_dvp: Cell::new(None),
            nc_dvpid: Cell::new(0),
            nc_vp: Cell::new(None),
            nc_vpid: Cell::new(0),
            nc_nlen: Cell::new(0),
            nc_name: Cell::new([0; NAMECACHE_MAXLEN]),
        }
    }

    /// The `nc_nlen` bytes of `nc_name`.
    pub fn name(&self) -> ([u8; NAMECACHE_MAXLEN], usize) {
        (self.nc_name.get(), self.nc_nlen.get() as usize)
    }
}

impl Default for Namecache {
    fn default() -> Self {
        Self::new()
    }
}

tree_adapter!(
    /// `RBT_HEAD(namecache_rb_cache, namecache)`: a directory's cached names, through
    /// `n_rbcache`, ordered by `namecache_compare`.
    pub NamecacheRbCache: Namecache, n_rbcache => RbtEntry, namecache_compare
);

queue_adapter!(
    /// The regular entries' LRU chain (`nclruhead`), through `nc_lru`.
    pub NcLru: Namecache, nc_lru => TailqEntry<Namecache>
);

queue_adapter!(
    /// The negative entries' LRU chain (`nclruneghead`), through `nc_neg`.
    pub NcNeg: Namecache, nc_neg => TailqEntry<Namecache>
);

queue_adapter!(
    /// `TAILQ_HEAD(, namecache) v_cache_dst`: the entries naming a vnode, through `nc_me`.
    pub NcMe: Namecache, nc_me => TailqEntry<Namecache>
);

/// `struct nchstats`: stats on usefulness of namei caches.
pub struct Nchstats {
    /// `ncs_goodhits`: hits that we can really use.
    pub ncs_goodhits: AtomicU64,
    /// `ncs_neghits`: negative hits that we can use.
    pub ncs_neghits: AtomicU64,
    /// `ncs_badhits`: hits we must drop.
    pub ncs_badhits: AtomicU64,
    /// `ncs_falsehits`: hits with id mismatch.
    pub ncs_falsehits: AtomicU64,
    /// `ncs_miss`: misses.
    pub ncs_miss: AtomicU64,
    /// `ncs_long`: long names that ignore cache.
    pub ncs_long: AtomicU64,
    /// `ncs_pass2`: names found with passes == 2.
    pub ncs_pass2: AtomicU64,
    /// `ncs_2passes`: number of times we attempt it.
    pub ncs_2passes: AtomicU64,
    /// `ncs_revhits`: reverse-cache hits.
    pub ncs_revhits: AtomicU64,
    /// `ncs_revmiss`: reverse-cache misses.
    pub ncs_revmiss: AtomicU64,
    /// `ncs_dothits`: hits on '.' lookups.
    pub ncs_dothits: AtomicU64,
    /// `ncs_dotdothits`: hits on '..' lookups.
    pub ncs_dotdothits: AtomicU64,
}

impl Nchstats {
    /// All counters zero.
    pub const fn new() -> Self {
        Self {
            ncs_goodhits: AtomicU64::new(0),
            ncs_neghits: AtomicU64::new(0),
            ncs_badhits: AtomicU64::new(0),
            ncs_falsehits: AtomicU64::new(0),
            ncs_miss: AtomicU64::new(0),
            ncs_long: AtomicU64::new(0),
            ncs_pass2: AtomicU64::new(0),
            ncs_2passes: AtomicU64::new(0),
            ncs_revhits: AtomicU64::new(0),
            ncs_revmiss: AtomicU64::new(0),
            ncs_dothits: AtomicU64::new(0),
            ncs_dotdothits: AtomicU64::new(0),
        }
    }

    /// The counters in the C's member order, as the `u_int64_t`s user space reads.
    pub fn snapshot(&self) -> [u64; 12] {
        [
            &self.ncs_goodhits,
            &self.ncs_neghits,
            &self.ncs_badhits,
            &self.ncs_falsehits,
            &self.ncs_miss,
            &self.ncs_long,
            &self.ncs_pass2,
            &self.ncs_2passes,
            &self.ncs_revhits,
            &self.ncs_revmiss,
            &self.ncs_dothits,
            &self.ncs_dotdothits,
        ]
        .map(|c| c.load(Ordering::Relaxed))
    }
}

impl Default for Nchstats {
    fn default() -> Self {
        Self::new()
    }
}

/// `KERN_NCHSTATS_GOODHITS`.
pub const KERN_NCHSTATS_GOODHITS: i32 = 1;
/// `KERN_NCHSTATS_NEGHITS`.
pub const KERN_NCHSTATS_NEGHITS: i32 = 2;
/// `KERN_NCHSTATS_BADHITS`.
pub const KERN_NCHSTATS_BADHITS: i32 = 3;
/// `KERN_NCHSTATS_FALSEHITS`.
pub const KERN_NCHSTATS_FALSEHITS: i32 = 4;
/// `KERN_NCHSTATS_MISS`.
pub const KERN_NCHSTATS_MISS: i32 = 5;
/// `KERN_NCHSTATS_LONG`.
pub const KERN_NCHSTATS_LONG: i32 = 6;
/// `KERN_NCHSTATS_PASS2`.
pub const KERN_NCHSTATS_PASS2: i32 = 7;
/// `KERN_NCHSTATS_2PASSES`.
pub const KERN_NCHSTATS_2PASSES: i32 = 8;
/// `KERN_NCHSTATS_REVHITS`.
pub const KERN_NCHSTATS_REVHITS: i32 = 9;
/// `KERN_NCHSTATS_REVMISS`.
pub const KERN_NCHSTATS_REVMISS: i32 = 10;
/// `KERN_NCHSTATS_DOTHITS`.
pub const KERN_NCHSTATS_DOTHITS: i32 = 11;
/// `KERN_NCHSTATS_DOTDOTHITS`.
pub const KERN_NCHSTATS_DOTDOTHITS: i32 = 12;
/// `KERN_NCHSTATS_MAXID`.
pub const KERN_NCHSTATS_MAXID: i32 = 13;

/// `UNVEIL_READ`.
pub const UNVEIL_READ: u8 = 0x01;
/// `UNVEIL_WRITE`.
pub const UNVEIL_WRITE: u8 = 0x02;
/// `UNVEIL_CREATE`.
pub const UNVEIL_CREATE: u8 = 0x04;
/// `UNVEIL_EXEC`.
pub const UNVEIL_EXEC: u8 = 0x08;
/// `UNVEIL_USERSET`.
pub const UNVEIL_USERSET: u8 = 0x10;
/// `UNVEIL_PLEDGEOPEN`.
pub const UNVEIL_PLEDGEOPEN: u8 = 0x20;
/// `UNVEIL_MASK`.
pub const UNVEIL_MASK: u8 = 0x0f;
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/sys/namei.h");
        for (name, value) in [
            ("LOCKLEAF", LOCKLEAF),
            ("WANTPARENT", WANTPARENT),
            ("FOLLOW", FOLLOW),
            ("MODMASK", MODMASK),
            ("NOCROSSMOUNT", NOCROSSMOUNT),
            ("SAVESTART", SAVESTART),
            ("ISSYMLINK", ISSYMLINK),
            ("STRIPSLASHES", STRIPSLASHES),
            ("PDIRUNLOCK", PDIRUNLOCK),
            ("KERNELPATH", KERNELPATH),
            ("EXECPATH", EXECPATH),
            ("NAMECACHE_MAXLEN", NAMECACHE_MAXLEN as u64),
        ] {
            assert_eq!(
                crate::reftest::int(&defs, name),
                Some(value as i64),
                "{name}"
            );
        }
        assert_eq!(
            crate::reftest::int(&defs, "AT_FDCWD"),
            Some(i64::from(AT_FDCWD))
        );
        assert_eq!(
            crate::reftest::int(&defs, "UNVEIL_PLEDGEOPEN"),
            Some(i64::from(UNVEIL_PLEDGEOPEN))
        );
    }
}
/* </TESTS> */
