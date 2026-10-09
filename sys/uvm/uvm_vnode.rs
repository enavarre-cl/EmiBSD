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
 *
 * Copyright (c) 1997 Charles D. Cranor and Washington University.
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
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *      This product includes software developed by Charles D. Cranor and
 *      Washington University.
 * 4. The name of the author may not be used to endorse or promote products
 *    derived from this software without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT, INDIRECT,
 * INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT
 * NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
 * DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
 * THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF
 * THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 *
 * from: Id: uvm_vnode.h,v 1.1.2.4 1997/10/03 21:18:24 chuck Exp
 */
/*
 * Copyright (c) 1997 Charles D. Cranor and Washington University.
 * Copyright (c) 1991, 1993
 *      The Regents of the University of California.
 * Copyright (c) 1990 University of Utah.
 *
 * All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * the Systems Programming Group of the University of Utah Computer
 * Science Department.
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
 *      @(#)vnode_pager.c       8.8 (Berkeley) 2/13/94
 * from: Id: uvm_vnode.c,v 1.1.2.26 1998/02/02 20:38:07 chuck Exp
 */
/* </LICENSES> */

/* <CODE> */
//! `uvm_vnode.c`: the vnode pager, the `uvm_object` behind a mapped file (`struct
//! uvm_vnode`, `<uvm/uvm_vnode.h>`). Its pages are read and written with `VOP_READ`/
//! `VOP_WRITE` (so through the file system and the buffer cache), one page at a time on the
//! way in and a cluster at a time on the way out; an object whose last mapping goes away
//! persists with its pages until the vnode is recycled (`uvm_vnp_terminate`).
//!
//! Upstream: sys/uvm/uvm_vnode.h @ 3ce1f3f79392
//! Upstream: sys/uvm/uvm_vnode.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - A `struct uvm_vnode` is a `uvm_vnode_pool` item that is never freed (the vnode keeps it
//!   in `v_uvm` across recycling, as in C): `&'static UvmVnode`, `#[repr(C)]` with `u_obj`
//!   first, reached from a `&UvmObject` through [`uvn`], which checks the pager first (C_TO_RUST
//!   row on the embedded first member).
//! - `uvn_attach` of a block device needs the device switch (`bdevsw[].d_type`, `DIOCGPART`
//!   through `d_ioctl`, `conf.c`): reported, and the attach fails (NULL) as for a device
//!   that is not a disk.
//! - The pager's page arrays are `&mut [*const VmPage]` (`uvm_pager.rs`); `uvn_io` takes a
//!   `UioRw` for the C's `int rw`.
//! - `uvm_vnp_sync(NULL)` is `uvm_vnp_sync(None)`; `uvm_vnp_uncache` returns `bool`.
//! - `UVM_PAGE_OWN` is not configured; the `DEBUG` printfs are compiled out as in C.
//! - The `ratecheck` timestamp of `uvn_flush`'s error message is a `StaticCell`, written
//!   under the kernel lock (every caller of `uvn_flush` holds it: `uvn_detach` takes it,
//!   `msync` and the vnode layer run locked), where the C's function-local `static` is
//!   unguarded.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr;

use libkern::StaticCell;

use crate::kern::kern_rwlock::{rw_enter, rw_enter_write, rw_exit, rw_exit_write, rw_init_flags};
use crate::kern::kern_synch::{rwsleep_nsec, tsleep_nsec, wakeup};
use crate::kern::kern_time::ratecheck;
use crate::kern::kern_xxx::REBOOTING;
use crate::kern::subr_pool::{pool_get, pool_init, pool_put};
use crate::kern::subr_prf::panic;
use crate::kern::vfs_subr::{vget, vref, vrele};
use crate::kern::vfs_vnops::vn_lock;
use crate::kern::vfs_vops::{VOP_GETATTR, VOP_READ, VOP_UNLOCK, VOP_WRITE};
use crate::machine::cpu::curproc;
use crate::machine::intr::IPL_NONE;
use crate::machine::pmap::{pmap_clear_modify, pmap_is_modified, pmap_page_protect};
use crate::net::if_::NETLOCK;
use crate::queue_adapter;
use crate::sys::errno::Errno;
use crate::sys::lock::{LK_EXCLUSIVE, LK_NOWAIT, LK_RECURSEFAIL, LK_RETRY};
use crate::sys::mman::{PROT_NONE, PROT_READ, PROT_WRITE};
use crate::sys::mount::Mount;
use crate::sys::param::{MAXBSIZE, PAGE_SHIFT, PAGE_SIZE, PVM};
use crate::sys::pool::{PR_WAITOK, PR_ZERO, Pool};
use crate::sys::queue::{ListEntry, ListHead, SimpleqEntry, SimpleqHead};
use crate::sys::rwlock::{RW_WRITE, RWL_IS_VNODE, Rwlock, rw_lock_held, rw_write_held};
use crate::sys::systm::{
    INFSLP, kernel_assert_locked, kernel_lock, kernel_unlock, net_lock, net_unlock,
};
use crate::sys::time::Timeval;
use crate::sys::uio::{Iovec, Uio, UioRw, UioSeg};
use crate::sys::vnode::{IO_NOCACHE, VBLK, VTEXT, Vattr, Vnode};
use crate::uvm::uvm_extern::{VmProt, Voff};
use crate::uvm::uvm_object::{UvmObject, uvm_obj_init, uvm_obj_is_vnode};
use crate::uvm::uvm_page::{
    PG_BUSY, PG_CLEAN, PG_CLEANCHK, PG_FAKE, PG_WANTED, PQ_ACTIVE, PQ_INACTIVE, VmPage,
    uvm_lock_pageq, uvm_pagealloc, uvm_pagedeactivate, uvm_pagedequeue, uvm_pagefree,
    uvm_pagelookup, uvm_pagewait, uvm_unlock_pageq,
};
use crate::uvm::uvm_pager::{
    PGO_ALLPAGES, PGO_CLEANIT, PGO_DEACTIVATE, PGO_DOACTCLUST, PGO_FREE, PGO_LOCKED, PGO_NOWAIT,
    PGO_PDFREECLUST, PGO_SYNCIO, UVMPAGER_MAPIN_READ, UVMPAGER_MAPIN_WAITOK, UVMPAGER_MAPIN_WRITE,
    UvmPagerops, VM_PAGER_AGAIN, VM_PAGER_BAD, VM_PAGER_ERROR, VM_PAGER_OK, VM_PAGER_PEND,
    VM_PAGER_UNLOCK, pgo_dontcare, uvm_mk_pcluster, uvm_pager_put, uvm_pagermapin, uvm_pagermapout,
};
use crate::uvm::uvm_param::{round_page, trunc_page};
use crate::uvm::uvm_pdaemon::uvm_wait;
use crate::{kassert, kprintf};

// u_flags values

/// `UVM_VNODE_VALID`: we are attached to the vnode.
pub const UVM_VNODE_VALID: i32 = 0x001;
/// `UVM_VNODE_CANPERSIST`: we can persist after ref == 0.
pub const UVM_VNODE_CANPERSIST: i32 = 0x002;
/// `UVM_VNODE_DYING`: final detach/terminate in progress.
pub const UVM_VNODE_DYING: i32 = 0x008;
/// `UVM_VNODE_RELKILL`: uvn should be killed by releasepg when final i/o is done.
pub const UVM_VNODE_RELKILL: i32 = 0x010;
/// `UVM_VNODE_WANTED`: someone is waiting for alock, dying, or relkill to clear.
pub const UVM_VNODE_WANTED: i32 = 0x020;
/// `UVM_VNODE_VNISLOCKED`: underlying vnode struct is locked (valid when DYING is true).
pub const UVM_VNODE_VNISLOCKED: i32 = 0x040;
/// `UVM_VNODE_IOSYNC`: I/O sync in progress ... setter sleeps on `&uvn->u_nio`.
pub const UVM_VNODE_IOSYNC: i32 = 0x080;
/// `UVM_VNODE_IOSYNCWANTED`: a process is waiting for the i/o sync to clear so it can do i/o.
pub const UVM_VNODE_IOSYNCWANTED: i32 = 0x100;
/// `UVM_VNODE_WRITEABLE`: uvn has pages that are writeable.
pub const UVM_VNODE_WRITEABLE: i32 = 0x200;

/// `UVM_VNODE_BLOCKED`: any condition that should new processes from touching the vnode
/// \[set WANTED and sleep to wait for it to clear\].
pub const UVM_VNODE_BLOCKED: i32 = UVM_VNODE_DYING | UVM_VNODE_RELKILL;

/// `struct uvm_vnode`: the vnode's handle into the VM system.
///
/// Locks: \[I\] immutable after creation, \[K\] kernel lock, \[S\] `uvn_sync_lock`, \[v\]
/// `u_obj`'s `vmobjlock`.
#[repr(C)]
pub struct UvmVnode {
    /// `u_obj`: the actual VM object.
    pub u_obj: UvmObject,
    /// \[I\] `u_vnode`: pointer back to vnode.
    pub u_vnode: Cell<Option<&'static Vnode>>,
    /// \[v\] `u_flags`: flags.
    pub u_flags: Cell<i32>,
    /// \[v\] `u_nio`: number of running I/O requests.
    pub u_nio: Cell<i32>,
    /// \[v\] `u_size`: size of object.
    pub u_size: Cell<Voff>,
    /// \[K\] `u_wlist`: list of writeable vnode objs.
    pub u_wlist: ListEntry<UvmVnode>,
    /// \[S\] `u_syncq`: vnode objs due for a "sync".
    pub u_syncq: SimpleqEntry<UvmVnode>,
}

// SAFETY: the members are changed under the locks their doc comments name, as in C.
unsafe impl Sync for UvmVnode {}

impl UvmVnode {
    /// A zeroed `uvm_vnode`, as `pool_get(PR_ZERO)` returns it.
    const fn new() -> Self {
        Self {
            u_obj: UvmObject::new(0),
            u_vnode: Cell::new(None),
            u_flags: Cell::new(0),
            u_nio: Cell::new(0),
            u_size: Cell::new(0),
            u_wlist: ListEntry::new(),
            u_syncq: SimpleqEntry::new(),
        }
    }

    /// `uvn->u_vnode`, set at attach.
    pub fn vnode(&self) -> &'static Vnode {
        match self.u_vnode.get() {
            Some(vp) => vp,
            None => panic(format_args!("uvm_vnode {:p} without a vnode", self)),
        }
    }
}

queue_adapter!(
    /// `LIST_HEAD(, uvm_vnode) uvn_wlist`, through `u_wlist`.
    pub UvnWlist: UvmVnode, u_wlist => ListEntry<UvmVnode>
);

queue_adapter!(
    /// `SIMPLEQ_HEAD(, uvm_vnode) uvn_sync_q`, through `u_syncq`.
    pub UvnSyncq: UvmVnode, u_syncq => SimpleqEntry<UvmVnode>
);

/// `uvn_wlist`'s type: a `Sync` static list.
pub struct UvnWlistHead(pub ListHead<UvnWlist>);

// SAFETY: changed and walked under the kernel lock, as in C (`uvn_attach` and
// `uvm_vnp_setsize` assert it, `uvn_detach` takes it, `uvm_vnp_sync` runs from the locked
// vnode layer).
unsafe impl Sync for UvnWlistHead {}

/// `uvn_sync_q`'s type: a `Sync` static queue.
pub struct UvnSyncqHead(pub SimpleqHead<UvnSyncq>);

// SAFETY: changed under `uvn_sync_lock`, as in C.
unsafe impl Sync for UvnSyncqHead {}

/// `uvm_vnode_pool`.
pub static UVM_VNODE_POOL: Pool = Pool::new();
/// `uvn_wlist`: \[K\] writeable uvns.
pub static UVN_WLIST: UvnWlistHead = UvnWlistHead(ListHead::new());
/// `uvn_sync_q`: \[S\] sync'ing uvns.
pub static UVN_SYNC_Q: UvnSyncqHead = UvnSyncqHead(SimpleqHead::new());
/// `uvn_sync_lock`: locks sync operation.
pub static UVN_SYNC_LOCK: Rwlock = Rwlock::new("uvnsync");

/// `lasttime` of `uvn_flush`'s pageout error message.
static UVN_FLUSH_LASTTIME: StaticCell<Timeval> = StaticCell::new(Timeval::new(0, 0));

/// `uvm_vnodeops`: master pager structure.
pub static UVM_VNODEOPS: UvmPagerops = UvmPagerops {
    pgo_init: Some(uvn_init),
    pgo_reference: Some(uvn_reference),
    pgo_detach: Some(uvn_detach),
    pgo_fault: None,
    pgo_flush: Some(uvn_flush),
    pgo_get: Some(uvn_get),
    pgo_put: Some(uvn_put),
    pgo_cluster: Some(uvn_cluster),
    // use generic version of this: see uvm_pager.c
    pgo_mk_pcluster: Some(uvm_mk_pcluster),
};

/// The page in a slot of a pager's page array, which the C dereferences: not `NULL`, not
/// `PGO_DONTCARE`.
fn pps_page(p: *const VmPage) -> &'static VmPage {
    if p.is_null() || pgo_dontcare(p) {
        panic(format_args!("uvm_vnode: no page in the slot"));
    }
    // SAFETY: the pager fills its arrays with pages of the object it holds busy; `vm_page`s
    // live forever in the page array.
    unsafe { &*p }
}

/// `(struct uvm_vnode *)uobj`: the vnode object `uobj` is the first member of.
pub fn uvn(uobj: &UvmObject) -> &UvmVnode {
    kassert!(uvm_obj_is_vnode(uobj));
    // SAFETY: an object whose pager is `uvm_vnodeops` was made by `uvn_attach` as the first
    // field of a `#[repr(C)]` `UvmVnode`, so the pointers coincide.
    unsafe { &*ptr::from_ref(uobj).cast::<UvmVnode>() }
}

/// The vnode's `uvm_vnode`, `vp->v_uvm`.
fn v_uvm(vp: &Vnode) -> Option<&'static UvmVnode> {
    vp.v_uvm.get()
}

/// `uvn_init`: init pager private data structures.
pub fn uvn_init() {
    pool_init(
        &UVM_VNODE_POOL,
        size_of::<UvmVnode>(),
        0,
        IPL_NONE,
        PR_WAITOK,
        "uvmvnodes",
        None,
    );
    UVN_WLIST.0.init();
    // note: uvn_sync_q init'd in uvm_vnp_sync()
    rw_init_flags(&UVN_SYNC_LOCK, "uvnsync", RWL_IS_VNODE);
}

/// uvn_attach
///
/// attach a vnode structure to a VM object. if the vnode is already attached, then just bump
/// the reference count by one and return the VM object. if not already attached, attach and
/// return the new VM obj. the "accessprot" tells the max access the attaching thread wants to
/// our pages.
///
/// => in fact, nothing should be locked so that we can sleep here.
pub fn uvn_attach(vp: &'static Vnode, accessprot: VmProt) -> Option<&'static UvmObject> {
    let Some(p) = curproc() else {
        panic(format_args!("uvn_attach: no curproc"));
    };

    // if we're mapping a BLK device, make sure it is a disk.
    if vp.v_type.get() == VBLK {
        // bdevsw[major(vp->v_rdev)].d_type != D_DISK, then the size from DIOCGPART through
        // d_ioctl (the disk label's d_secsize * DL_GETPSIZE): the device switch (conf.c).
        let _ = crate::unported!("uvn_attach: bdevsw[].d_type, DIOCGPART (conf.c)");
        return None;
    }

    let mut vattr = Vattr::new();
    VOP_GETATTR(vp, &mut vattr, p.p_ucred.get(), p).ok()?;
    let used_vnode_size = vattr.va_size;

    // make sure that the newsize fits within a vaddr_t
    // XXX: need to revise addressing data types

    if v_uvm(vp).is_none() {
        let Some(mem) = pool_get(&UVM_VNODE_POOL, PR_WAITOK | PR_ZERO) else {
            // PR_WAITOK cannot sleep yet (subr_pool.rs).
            return None;
        };
        let uvn = mem.cast::<UvmVnode>();
        // SAFETY: a fresh, suitably aligned pool item of `size_of::<UvmVnode>()` bytes,
        // written once before anything else sees it.
        unsafe { uvn.as_ptr().write(UvmVnode::new()) };
        // SAFETY: as above; the vnode keeps it for good (`v_uvm`).
        let uvn: &'static UvmVnode = unsafe { uvn.as_ref() };
        kernel_assert_locked();
        if v_uvm(vp).is_none() {
            uvm_obj_init(&uvn.u_obj, Some(&UVM_VNODEOPS), 0);
            uvn.u_vnode.set(Some(vp));
            vp.v_uvm.set(Some(uvn));
        } else {
            pool_put(&UVM_VNODE_POOL, mem);
        }
    }

    let uvn = v_uvm(vp)?;
    let lock = uvn.u_obj.vmobjlock();
    let _ = rw_enter(lock, RW_WRITE);
    while uvn.u_flags.get() & UVM_VNODE_BLOCKED != 0 {
        uvn.u_flags.set(uvn.u_flags.get() | UVM_VNODE_WANTED);
        let _ = rwsleep_nsec(ptr::from_ref(uvn), lock, PVM, "uvn_attach", INFSLP);
    }

    // now uvn must not be in a blocked state. first check to see if it is already active, in
    // which case we can bump the reference count.
    if uvn.u_flags.get() & UVM_VNODE_VALID != 0 {
        // already active?
        // regain vref if we were persisting
        if uvn.u_obj.uo_refs.get() == 0 {
            vref(vp);
        }
        uvn.u_obj.uo_refs.set(uvn.u_obj.uo_refs.get() + 1); // bump uvn ref!
    } else {
        // now set up the uvn.
        kassert!(uvn.u_obj.uo_refs.get() == 0);
        uvn.u_obj.uo_refs.set(uvn.u_obj.uo_refs.get() + 1);
        uvn.u_flags.set(UVM_VNODE_VALID | UVM_VNODE_CANPERSIST);
        uvn.u_nio.set(0);
        uvn.u_size.set(used_vnode_size as Voff);

        // this reference will stay as long as there is a valid mapping of the vnode. dropped
        // when the reference count goes to zero [and we either free or persist].
        vref(vp);
    }

    // if write access, we need to add it to the wlist
    if accessprot & PROT_WRITE != 0 && uvn.u_flags.get() & UVM_VNODE_WRITEABLE == 0 {
        uvn.u_flags.set(uvn.u_flags.get() | UVM_VNODE_WRITEABLE); // we are on wlist!
        kernel_assert_locked();
        // SAFETY: a uvn without `UVM_VNODE_WRITEABLE` is on no wlist; it is never freed.
        unsafe { UVN_WLIST.0.insert_head(uvn) };
    }

    if uvn.u_flags.get() & UVM_VNODE_WANTED != 0 {
        wakeup(ptr::from_ref(uvn));
    }
    rw_exit(lock);

    Some(&uvn.u_obj)
}

/// uvn_reference
///
/// duplicate a reference to a VM object. Note that the reference count must already be at
/// least one (the passed in reference) so there is no chance of the uvn being killed out
/// here.
///
/// => caller must be using the same accessprot as was used at attach time
pub fn uvn_reference(uobj: &UvmObject) {
    let _ = rw_enter(uobj.vmobjlock(), RW_WRITE);
    // DEBUG: the UVM_VNODE_VALID check.
    uobj.uo_refs.set(uobj.uo_refs.get() + 1);
    rw_exit(uobj.vmobjlock());
}

/// uvn_detach
///
/// remove a reference to a VM object.
///
/// => caller must call with map locked.
/// => this starts the detach process, but doesn't have to finish it (async i/o could still be
///    pending).
pub fn uvn_detach(uobj: &UvmObject) {
    let lock = uobj.vmobjlock();
    let _ = rw_enter(lock, RW_WRITE);
    uobj.uo_refs.set(uobj.uo_refs.get() - 1); // drop ref!
    if uobj.uo_refs.get() != 0 {
        // still more refs
        rw_exit(lock);
        return;
    }

    kernel_lock();
    // get other pointers ...
    let uvn = uvn(uobj);
    let vp = uvn.vnode();

    // clear VTEXT flag now that there are no mappings left (VTEXT is used to keep an active
    // text file from being overwritten).
    vp.v_flag.set(vp.v_flag.get() & !VTEXT);

    'out: {
        // we just dropped the last reference to the uvn. see if we can let it "stick
        // around".
        if uvn.u_flags.get() & UVM_VNODE_CANPERSIST != 0 {
            // won't block
            let _ = uvn_flush(uobj, 0, 0, PGO_DEACTIVATE | PGO_ALLPAGES);
            break 'out;
        }

        // its a goner!
        uvn.u_flags.set(uvn.u_flags.get() | UVM_VNODE_DYING);

        // even though we may unlock in flush, no one can gain a reference to us until we
        // clear the "dying" flag [because it blocks attaches]. we will not do that until
        // after we've disposed of all the pages with uvn_flush(). note that before the flush
        // the only pages that could be marked PG_BUSY are ones that are in async pageout by
        // the daemon. (there can't be any pending "get"'s because there are no references to
        // the object).
        let _ = uvn_flush(uobj, 0, 0, PGO_CLEANIT | PGO_FREE | PGO_ALLPAGES);

        // given the structure of this pager, the above flush request will create the
        // following state: all the pages that were in the object have either been free'd or
        // they are marked PG_BUSY and in the middle of an async io. If we still have pages
        // we set the "relkill" state, so that in the case the vnode gets terminated we know
        // to leave it alone. Otherwise we'll kill the vnode when it's empty.
        uvn.u_flags.set(uvn.u_flags.get() | UVM_VNODE_RELKILL);
        // wait on any outstanding io
        while uobj.uo_npages.get() != 0 && uvn.u_flags.get() & UVM_VNODE_RELKILL != 0 {
            uvn.u_flags.set(uvn.u_flags.get() | UVM_VNODE_IOSYNC);
            let _ = rwsleep_nsec(ptr::from_ref(&uvn.u_nio), lock, PVM, "uvn_term", INFSLP);
        }

        if uvn.u_flags.get() & UVM_VNODE_RELKILL == 0 {
            rw_exit(lock);
            kernel_unlock();
            return;
        }

        // kill object now. note that we can't be on the sync q because all references are
        // gone.
        if uvn.u_flags.get() & UVM_VNODE_WRITEABLE != 0 {
            // SAFETY: a uvn with `UVM_VNODE_WRITEABLE` is on the wlist.
            unsafe { ListHead::<UvnWlist>::remove(uvn) };
        }
        kassert!(uobj.memt.is_empty());
        let oldflags = uvn.u_flags.get();
        uvn.u_flags.set(0);

        // wake up any sleepers
        if oldflags & UVM_VNODE_WANTED != 0 {
            wakeup(ptr::from_ref(uvn));
        }
    }
    // out:
    rw_exit(lock);

    // drop our reference to the vnode.
    let _ = vrele(vp);
    kernel_unlock();
}

/// uvm_vnp_terminate: external hook to clear out a vnode's VM
///
/// called in two cases:
///  \[1\] when a persisting vnode vm object (i.e. one with a zero reference count) needs to
///      be freed so that a vnode can be reused. this happens under "getnewvnode" in
///      vfs_subr.c. if the vnode from the free list is still attached (i.e. not VBAD) then
///      vgone is called. as part of the vgone trace this should get called to free the vm
///      object. this is the common case.
///  \[2\] when a filesystem is being unmounted by force (MNT_FORCE, "umount -f") the vgone()
///      function is called on active vnodes on the mounted file systems to kill their data
///      (the vnodes become "dead" ones [see src/sys/miscfs/deadfs/...]). that results in a
///      call here (even if the uvn is still in use -- i.e. has a non-zero reference count).
///      this case happens at "umount -f" and during a "reboot/halt" operation.
///
/// => the caller must XLOCK and VOP_LOCK the vnode before calling us \[protects us from
///    getting a vnode that is already in the DYING state...\]
/// => in case \[2\] the uvn is still alive after this call, but all I/O ops will fail (due to
///    the backing vnode now being "dead"). this will prob. kill any process using the uvn due
///    to pgo_get failing.
pub fn uvm_vnp_terminate(vp: &'static Vnode) {
    let Some(uvn) = v_uvm(vp) else {
        return;
    };
    let uobj = &uvn.u_obj;
    let lock = uobj.vmobjlock();
    let _ = rw_enter(lock, RW_WRITE);
    if uvn.u_flags.get() & UVM_VNODE_VALID == 0 {
        rw_exit(lock);
        return;
    }

    // must be a valid uvn that is not already dying (because XLOCK protects us from that).
    // DEBUG: the "terminating active vnode" printf.

    // it is possible that the uvn was detached and is in the relkill state [i.e. waiting for
    // async i/o to finish]. we take over the vnode now and cancel the relkill. we want to know
    // when the i/o is done so we can recycle right away. note that a uvn can only be in the
    // RELKILL state if it has a zero reference count.
    if uvn.u_flags.get() & UVM_VNODE_RELKILL != 0 {
        uvn.u_flags.set(uvn.u_flags.get() & !UVM_VNODE_RELKILL); // cancel RELKILL
    }

    // block the uvn by setting the dying flag, and then flush the pages.
    //
    // also, note that we tell I/O that we are already VOP_LOCK'd so that uvn_io doesn't
    // attempt to VOP_LOCK again.
    //
    // XXXCDC: setting VNISLOCKED on an active uvn which is being terminated due to a forceful
    // unmount might not be a good idea. maybe we need a way to pass in this info to uvn_flush
    // through a pager-defined PGO_ constant [currently there are none].
    uvn.u_flags
        .set(uvn.u_flags.get() | UVM_VNODE_DYING | UVM_VNODE_VNISLOCKED);

    let _ = uvn_flush(uobj, 0, 0, PGO_CLEANIT | PGO_FREE | PGO_ALLPAGES);

    // as we just did a flush we expect all the pages to be gone or in the process of going.
    // sleep to wait for the rest to go [via iosync].
    while uobj.uo_npages.get() != 0 {
        // DEBUG: the busy-page and u_nio checks.
        uvn.u_flags.set(uvn.u_flags.get() | UVM_VNODE_IOSYNC);
        let _ = rwsleep_nsec(ptr::from_ref(&uvn.u_nio), lock, PVM, "uvn_term", INFSLP);
    }

    // done. now we free the uvn if its reference count is zero (true if we are zapping a
    // persisting uvn). however, if we are terminating a uvn with active mappings we let it
    // live ... future calls down to the vnode layer will fail.
    let oldflags = uvn.u_flags.get();
    if uobj.uo_refs.get() != 0 {
        // uvn must live on it is dead-vnode state until all references are gone. restore
        // flags. clear CANPERSIST state.
        uvn.u_flags.set(
            uvn.u_flags.get()
                & !(UVM_VNODE_DYING
                    | UVM_VNODE_VNISLOCKED
                    | UVM_VNODE_WANTED
                    | UVM_VNODE_CANPERSIST),
        );
    } else {
        // free the uvn now. note that the vref reference is already gone [it is dropped when
        // we enter the persist state].
        if uvn.u_flags.get() & UVM_VNODE_IOSYNCWANTED != 0 {
            panic(format_args!("uvm_vnp_terminate: io sync wanted bit set"));
        }

        if uvn.u_flags.get() & UVM_VNODE_WRITEABLE != 0 {
            // SAFETY: a uvn with `UVM_VNODE_WRITEABLE` is on the wlist.
            unsafe { ListHead::<UvnWlist>::remove(uvn) };
        }
        uvn.u_flags.set(0); // uvn is history, clear all bits
    }

    if oldflags & UVM_VNODE_WANTED != 0 {
        wakeup(ptr::from_ref(uvn));
    }

    rw_exit(lock);
}

// NOTE: currently we have to use VOP_READ/VOP_WRITE because they go through the buffer cache
// and allow I/O in any size. These VOPs use synchronous i/o. [vs. VOP_STRATEGY which can be
// async, but doesn't go through the buffer cache or allow I/O sizes larger than a block]. we
// will eventually want to change this.
//
// (The C's comment goes on to sketch an async aiodesc interface that was never written.)

/// The number of slots in `uvn_flush`'s cluster array (`MAXBSIZE >> PAGE_SHIFT`).
const FLUSH_PPS: usize = MAXBSIZE >> PAGE_SHIFT;

/// uvn_flush: flush pages out of a uvm object.
///
/// => if PGO_CLEANIT is set, we may block (due to I/O). thus, a caller might want to unlock
///    higher level resources (e.g. vm_map) before calling flush.
/// => if PGO_CLEANIT is not set, then we will not block
/// => if PGO_ALLPAGE is set, then all pages in the object are valid targets for flushing.
/// => NOTE: we are allowed to lock the page queues, so the caller must not be holding the
///    lock on them [e.g. pagedaemon had better not call us with the queues locked]
/// => we return TRUE unless we encountered some sort of I/O error
///
/// comment on "cleaning" object and PG_BUSY pages: this routine is holding the lock on the
/// object. the only time that it can run into a PG_BUSY page that it does not own is if some
/// other process has started I/O on the page (e.g. either a pagein, or a pageout). if the
/// PG_BUSY page is being paged in, then it can not be dirty (!PG_CLEAN) because no one has had
/// a chance to modify it yet. if the PG_BUSY page is being paged out then it means that
/// someone else has already started cleaning the page for us (how nice!). in this case, if we
/// have syncio specified, then after we make our pass through the object we need to wait for
/// the other PG_BUSY pages to clear off (i.e. we need to do an iosync). also note that once a
/// page is PG_BUSY it must stay in its object until it is un-busyed.
pub fn uvn_flush(uobj: &UvmObject, start: Voff, stop: Voff, mut flags: i32) -> bool {
    let uvn = uvn(uobj);
    let mut pps: [*const VmPage; FLUSH_PPS] = [ptr::null(); FLUSH_PPS];
    let lock = uobj.vmobjlock();
    let ps = PAGE_SIZE as Voff;

    kassert!(rw_write_held(lock));

    // get init vals and determine how we are going to traverse object
    let mut need_iosync = false;
    let mut retval = true; // return value
    let (start, stop) = if flags & PGO_ALLPAGES != 0 {
        (0, round_page(uvn.u_size.get() as usize) as Voff)
    } else {
        (
            trunc_page(start as usize) as Voff,
            (round_page(stop as usize) as Voff).min(round_page(uvn.u_size.get() as usize) as Voff),
        )
    };

    // PG_CLEANCHK: this bit is used by the pgo_mk_pcluster function as a _hint_ as to how up
    // to date the PG_CLEAN bit is. if the hint is wrong it will only prevent us from
    // clustering... it won't break anything. we clear all PG_CLEANCHK bits here, and
    // pgo_mk_pcluster will set them as it syncs PG_CLEAN. This is only an issue if we are
    // looking at non-inactive pages (because inactive page's PG_CLEAN bit is always up to date
    // since there are no mappings). [borrowed PG_CLEANCHK idea from FreeBSD VM]
    if flags & PGO_CLEANIT != 0 {
        kassert!(uobj.pgops().pgo_mk_pcluster.is_some());
        let mut curoff = start;
        while curoff < stop {
            if let Some(pp) = uvm_pagelookup(uobj, curoff) {
                pp.clear_bits(PG_CLEANCHK);
            }
            curoff += ps;
        }
    }

    uvm_lock_pageq();
    // locked: both page queues
    let mut curoff = start;
    while curoff < stop {
        let Some(pp) = uvm_pagelookup(uobj, curoff) else {
            curoff += ps;
            continue;
        };
        // handle case where we do not need to clean page (either because we are not clean or
        // because page is not dirty or is busy):
        //
        // NOTE: we are allowed to deactivate a non-wired active PG_BUSY page, but once a
        // PG_BUSY page is on the inactive queue it must stay put until it is !PG_BUSY (so as
        // not to confuse pagedaemon).
        let needs_clean = if flags & PGO_CLEANIT == 0 || pp.flags() & PG_BUSY != 0 {
            if pp.flags() & PG_BUSY != 0
                && flags & (PGO_CLEANIT | PGO_SYNCIO) == (PGO_CLEANIT | PGO_SYNCIO)
            {
                need_iosync = true;
            }
            false
        } else {
            // freeing: nuke all mappings so we can sync PG_CLEAN bit with no race
            if pp.flags() & PG_CLEAN != 0 && flags & PGO_FREE != 0 && pp.flags() & PQ_ACTIVE != 0 {
                pmap_page_protect(pp, PROT_NONE);
            }
            if pp.flags() & PG_CLEAN != 0 && pmap_is_modified(pp) {
                pp.clear_bits(PG_CLEAN);
            }
            pp.set_bits(PG_CLEANCHK);

            pp.flags() & PG_CLEAN == 0
        };

        // if we don't need a clean, deactivate/free pages then cont.
        if !needs_clean {
            if flags & PGO_DEACTIVATE != 0 {
                uvm_unlock_pageq();
                uvm_pagedeactivate(pp);
                uvm_lock_pageq();
            } else if flags & PGO_FREE != 0 {
                if pp.flags() & PG_BUSY != 0 {
                    uvm_unlock_pageq();
                    uvm_pagewait(pp, lock, "uvn_flsh");
                    let _ = rw_enter(lock, RW_WRITE);
                    uvm_lock_pageq();
                    continue; // the same offset again
                } else {
                    pmap_page_protect(pp, PROT_NONE);
                    // dequeue to prevent lock recursion
                    if pp.flags() & (PQ_ACTIVE | PQ_INACTIVE) != 0 {
                        uvm_pagedequeue(pp);
                    }
                    uvm_pagefree(pp);
                }
            }
            curoff += ps;
            continue;
        }

        // pp points to a page in the object that we are working on. if it is
        // !PG_CLEAN,!PG_BUSY and we asked for cleaning (PGO_CLEANIT). we clean it now.
        //
        // let uvm_pager_put attempted a clustered page out. note: locked: page queues.
        let mut ppsp = 0;
        let mut npages;
        let result = loop {
            // ReTry:
            pp.set_bits(PG_BUSY);
            // UVM_PAGE_OWN(pp, "uvn_flush"): not configured.
            pmap_page_protect(pp, PROT_READ);
            npages = FLUSH_PPS as i32;

            let result = uvm_pager_put(
                uobj,
                pp,
                &mut pps,
                &mut ppsp,
                &mut npages,
                flags | PGO_DOACTCLUST,
                start,
                stop,
            );

            // if we did an async I/O it is remotely possible for the async i/o to complete
            // and the page "pp" be freed or what not before we get a chance to relock the
            // object. Therefore, we only touch it when it won't be freed, RELEASED took care
            // of the rest.
            uvm_lock_pageq();

            // VM_PAGER_AGAIN: given the structure of this pager, this can only happen when we
            // are doing async I/O and can't map the pages into kernel memory (pager_map) due
            // to lack of vm space. if this happens we drop back to sync I/O.
            if result == VM_PAGER_AGAIN {
                kassert!(flags & PGO_SYNCIO == 0);
                flags |= PGO_SYNCIO;
                continue;
            }
            break result;
        };

        // the cleaning operation is now done. finish up. note that on error (!OK, !PEND)
        // uvm_pager_put drops the cluster for us. if success (OK, PEND) then uvm_pager_put
        // returns the cluster to us in ppsp/npages.

        // for pending async i/o if we are not deactivating we can move on to the next page.
        // aiodoned deals with the freeing case for us.
        if result == VM_PAGER_PEND && flags & PGO_DEACTIVATE == 0 {
            curoff += ps;
            continue;
        }

        // need to look at each page of the I/O operation, and do what we gotta do.
        for lcv in 0..npages as usize {
            let ptmp = pps_page(pps[ppsp + lcv]);
            // verify the page didn't get moved
            if result == VM_PAGER_PEND && !ptr::eq(ptmp.uobject.get(), uobj) {
                continue;
            }

            // unbusy the page if I/O is done. note that for pending I/O it is possible that
            // the I/O op finished (in which case the page is no longer busy).
            if result != VM_PAGER_PEND {
                if ptmp.flags() & PG_WANTED != 0 {
                    wakeup(ptr::from_ref(ptmp));
                }

                ptmp.clear_bits(PG_WANTED | PG_BUSY);
                // UVM_PAGE_OWN(ptmp, NULL): not configured.
                ptmp.set_bits(PG_CLEAN | PG_CLEANCHK);
                if flags & PGO_FREE == 0 {
                    let _ = pmap_clear_modify(ptmp);
                }
            }

            // dispose of page
            if flags & PGO_DEACTIVATE != 0 {
                uvm_unlock_pageq();
                uvm_pagedeactivate(ptmp);
                uvm_lock_pageq();
            } else if flags & PGO_FREE != 0 && result != VM_PAGER_PEND {
                if result != VM_PAGER_OK {
                    static INTERVAL: Timeval = Timeval::new(5, 0);
                    // SAFETY: the timestamp is only touched here, and every caller of
                    // `uvn_flush` holds the kernel lock (see the module's deviations).
                    let lasttime = unsafe { UVN_FLUSH_LASTTIME.get_mut() };
                    if ratecheck(lasttime, &INTERVAL) {
                        kprintf!(
                            "uvn_flush: obj={:p}, offset={:#x}.  error during pageout.\n",
                            pp.uobject.get(),
                            pp.offset.get()
                        );
                        kprintf!("uvn_flush: WARNING: changes to page may be lost!\n");
                    }
                    retval = false;
                }
                pmap_page_protect(ptmp, PROT_NONE);
                // dequeue first to prevent lock recursion
                if ptmp.flags() & (PQ_ACTIVE | PQ_INACTIVE) != 0 {
                    uvm_pagedequeue(ptmp);
                }
                uvm_pagefree(ptmp);
            }
        } // end of "lcv" for loop
        curoff += ps;
    } // end of "pp" for loop

    // done with pagequeues: unlock
    uvm_unlock_pageq();

    // now wait for all I/O if required.
    if need_iosync {
        while uvn.u_nio.get() != 0 {
            uvn.u_flags.set(uvn.u_flags.get() | UVM_VNODE_IOSYNC);
            let _ = rwsleep_nsec(ptr::from_ref(&uvn.u_nio), lock, PVM, "uvn_flush", INFSLP);
        }
        if uvn.u_flags.get() & UVM_VNODE_IOSYNCWANTED != 0 {
            wakeup(ptr::from_ref(&uvn.u_flags));
        }
        uvn.u_flags
            .set(uvn.u_flags.get() & !(UVM_VNODE_IOSYNC | UVM_VNODE_IOSYNCWANTED));
    }

    retval
}

/// uvn_cluster
///
/// we are about to do I/O in an object at offset. this function is called to establish a
/// range of offsets around "offset" in which we can cluster I/O.
pub fn uvn_cluster(uobj: &UvmObject, offset: Voff, loffset: &mut Voff, hoffset: &mut Voff) {
    let uvn = uvn(uobj);
    *loffset = offset;

    kassert!(rw_write_held(uobj.vmobjlock()));

    if *loffset >= uvn.u_size.get() {
        panic(format_args!("uvn_cluster: offset out of range"));
    }

    // XXX: old pager claims we could use VOP_BMAP to get maxcontig value.
    *hoffset = *loffset + MAXBSIZE as Voff;
    let end = round_page(uvn.u_size.get() as usize) as Voff;
    if *hoffset > end {
        // past end?
        *hoffset = end;
    }
}

/// uvn_put: flush page data to backing store.
///
/// => prefer map unlocked (not required)
/// => flags: PGO_SYNCIO -- use sync. I/O
/// => note: caller must set PG_CLEAN and pmap_clear_modify (if needed)
/// => XXX: currently we use VOP_READ/VOP_WRITE which are only sync. [thus we never do async
///    i/o! see iodone comment]
pub fn uvn_put(uobj: &UvmObject, pps: &mut [*const VmPage], npages: i32, flags: i32) -> i32 {
    let uvn = uvn(uobj);

    kassert!(rw_write_held(uobj.vmobjlock()));

    // Unless we're recycling this vnode, grab a reference to it to prevent it from being
    // recycled from under our feet. This also makes sure we can don't panic if we end up in
    // uvn_vnp_uncache() as a result of the I/O operation as that function assumes we hold a
    // reference.
    //
    // If the vnode is in the process of being recycled by someone else, grabbing a reference
    // will fail. In that case the pages will already be written out by whoever is cleaning
    // the vnode, so simply return VM_PAGER_AGAIN such that we skip these pages.
    let dying = uvn.u_flags.get() & UVM_VNODE_DYING != 0;
    if !dying && vget(uvn.vnode(), LK_NOWAIT).is_err() {
        return VM_PAGER_AGAIN;
    }

    let retval = uvn_io(uvn, pps, npages, flags, UioRw::UIO_WRITE);

    if !dying {
        let _ = vrele(uvn.vnode());
    }

    retval
}

/// uvn_get: get pages (synchronously) from backing store
///
/// => prefer map unlocked (not required)
/// => flags: PGO_ALLPAGES: get all of the pages
///           PGO_LOCKED: fault data structures are locked
/// => NOTE: offset is the offset of pps[0], _NOT_ pps[centeridx]
/// => NOTE: caller must check for released pages!!
#[allow(clippy::too_many_arguments)] // the pager interface's signature
pub fn uvn_get(
    uobj: &UvmObject,
    offset: Voff,
    pps: &mut [*const VmPage],
    npagesp: &mut i32,
    centeridx: i32,
    access_type: VmProt,
    _advice: i32,
    flags: i32,
) -> i32 {
    let lock = uobj.vmobjlock();
    let ps = PAGE_SIZE as Voff;

    kassert!(rw_lock_held(lock));
    kassert!(rw_write_held(lock) || (flags & PGO_LOCKED != 0 && access_type & PROT_WRITE == 0));

    // step 1: handled the case where fault data structures are locked.
    if flags & PGO_LOCKED != 0 {
        // gotpages is the current number of pages we've gotten (which we pass back up to
        // caller via *npagesp.
        let mut gotpages = 0;

        // step 1a: get pages that are already resident. only do this if the data structures
        // are locked (i.e. the first time through).
        let mut done = true; // be optimistic

        let mut current_offset = offset;
        for (lcv, slot) in pps.iter_mut().enumerate().take(*npagesp as usize) {
            let off = current_offset;
            current_offset += ps;
            // do we care about this page? if not, skip it
            if pgo_dontcare(*slot) {
                continue;
            }

            // lookup page; to be useful must get a non-busy page
            let Some(ptmp) = uvm_pagelookup(uobj, off).filter(|p| p.flags() & PG_BUSY == 0) else {
                if lcv as i32 == centeridx {
                    // need to do a wait or I/O!
                    done = false;
                }
                if flags & PGO_ALLPAGES != 0 {
                    done = false;
                    break;
                }
                continue;
            };

            // useful page: busy it and plug it in our result array
            *slot = ptmp;
            gotpages += 1;
        }

        // XXX: given the "advice", should we consider async read-ahead? XXX: fault current
        // does deactivate of pages behind us. is this good (other callers might now).
        //
        // XXX: read-ahead currently handled by buffer cache (bread) level. XXX: no async i/o
        // available. XXX: so we don't do anything now.

        // step 1c: now we've either done everything needed or we to unlock and do some
        // waiting or I/O.
        *npagesp = gotpages; // let caller know
        return if done { VM_PAGER_OK } else { VM_PAGER_UNLOCK };
    }

    // step 2: get non-resident or busy pages. data structures are unlocked.
    //
    // XXX: because we can't do async I/O at this level we get things page at a time
    // (otherwise we'd chunk). the VOP_READ() will do async-read-ahead for us at a lower level.
    let mut current_offset = offset;
    for (lcv, slot) in pps.iter_mut().enumerate().take(*npagesp as usize) {
        let off = current_offset;
        current_offset += ps;

        // skip over pages we've already gotten or don't want
        // skip over pages we don't _have_ to get
        if !slot.is_null() || (lcv as i32 != centeridx && flags & PGO_ALLPAGES == 0) {
            continue;
        }

        // we have yet to locate the current page (pps[lcv]). we first look for a page that is
        // already at the current offset. if we fine a page, we check to see if it is busy or
        // released. if that is the case, then we sleep on the page until it is no longer busy
        // or released and repeat the lookup. if the page we found is neither busy nor
        // released, then we busy it (so we own it) and plug it into pps[lcv]. this breaks the
        // following while loop and indicates we are ready to move on to the next page in the
        // "lcv" loop above.
        //
        // if we exit the while loop with pps[lcv] still set to NULL, then it means that we
        // allocated a new busy/fake/clean page ptmp in the object and we need to do I/O to
        // fill in the data.
        let ptmp: &'static VmPage = loop {
            // top of "pps" while loop
            // look for a current page
            let Some(ptmp) = uvm_pagelookup(uobj, off) else {
                // nope? allocate one now (if we can)
                let Some(ptmp) = uvm_pagealloc(Some(uobj), off, None, 0) else {
                    // out of RAM?
                    uvm_wait("uvn_getpage");
                    // goto top of pps while loop
                    continue;
                };
                // got new page ready for I/O. break pps while loop. pps[lcv] is still NULL.
                break ptmp;
            };

            // page is there, see if we need to wait on it
            if ptmp.flags() & PG_BUSY != 0 {
                uvm_pagewait(ptmp, lock, "uvn_get");
                let _ = rw_enter(lock, RW_WRITE);
                continue; // goto top of pps while loop
            }

            // if we get here then the page has become resident and unbusy between steps 1 and
            // 2. we busy it now (so we own it) and set pps[lcv] (so that we exit the while
            // loop).
            ptmp.set_bits(PG_BUSY);
            // UVM_PAGE_OWN(ptmp, "uvn_get2"): not configured.
            *slot = ptmp;
            // SAFETY: `vm_page`s live forever in the page array.
            break unsafe { &*ptr::from_ref(ptmp) };
        };

        // if we own the a valid page at the correct offset, pps[lcv] will point to it.
        // nothing more to do except go to the next page.
        if !slot.is_null() {
            continue; // next lcv
        }

        // we have a "fake/busy/clean" page that we just allocated. do I/O to fill it with
        // valid data.
        let mut one = [ptr::from_ref(ptmp)];
        let result = uvn_io(
            uvn(uobj),
            &mut one,
            1,
            PGO_SYNCIO | PGO_NOWAIT,
            UioRw::UIO_READ,
        );

        // I/O done. because we used syncio the result can not be PEND or AGAIN.
        if result != VM_PAGER_OK {
            if ptmp.flags() & PG_WANTED != 0 {
                wakeup(ptr::from_ref(ptmp));
            }

            ptmp.clear_bits(PG_WANTED | PG_BUSY);
            // UVM_PAGE_OWN(ptmp, NULL): not configured.
            uvm_pagefree(ptmp);
            rw_exit(lock);
            return result;
        }

        // we got the page! clear the fake flag (indicates valid data now in page) and plug
        // into our result array. note that page is still busy.
        //
        // it is the callers job to:
        // => check if the page is released
        // => unbusy the page
        // => activate the page

        // data is valid ...
        ptmp.clear_bits(PG_FAKE);
        let _ = pmap_clear_modify(ptmp); // ... and clean
        *slot = ptmp;
    }

    rw_exit(lock);
    VM_PAGER_OK
}

/// uvn_io: do I/O to a vnode
///
/// => prefer map unlocked (not required)
/// => flags: PGO_SYNCIO -- use sync. I/O
/// => XXX: currently we use VOP_READ/VOP_WRITE which are only sync. [thus we never do async
///    i/o! see iodone comment]
pub fn uvn_io(
    uvn: &UvmVnode,
    pps: &mut [*const VmPage],
    npages: i32,
    flags: i32,
    rw: UioRw,
) -> i32 {
    let uobj = &uvn.u_obj;
    let lock = uobj.vmobjlock();
    let lkflags = if flags & PGO_NOWAIT != 0 {
        LK_NOWAIT
    } else {
        0
    };
    let npages_u = npages as usize;

    kassert!(rw_write_held(lock));

    // init values
    let waitok = flags & PGO_SYNCIO != 0;
    let vn = uvn.vnode();
    let file_offset = pps_page(pps[0]).offset.get();

    // check for sync'ing I/O.
    while uvn.u_flags.get() & UVM_VNODE_IOSYNC != 0 {
        if !waitok {
            return VM_PAGER_AGAIN;
        }
        uvn.u_flags.set(uvn.u_flags.get() | UVM_VNODE_IOSYNCWANTED);
        let _ = rwsleep_nsec(ptr::from_ref(&uvn.u_flags), lock, PVM, "uvn_iosync", INFSLP);
    }

    // check size
    if file_offset >= uvn.u_size.get() {
        return VM_PAGER_BAD;
    }

    // first try and map the pages in (without waiting)
    let mapinflags = if rw == UioRw::UIO_READ {
        UVMPAGER_MAPIN_READ
    } else {
        UVMPAGER_MAPIN_WRITE
    };

    let mut kva = uvm_pagermapin(pps, npages_u, mapinflags);
    if kva == 0 && !waitok {
        return VM_PAGER_AGAIN;
    }

    // ok, now bump u_nio up. at this point we are done with uvn and can unlock it. if we
    // still don't have a kva, try again (this time with sleep ok).
    uvn.u_nio.set(uvn.u_nio.get() + 1); // we have an I/O in progress!
    let vnlocked = uvn.u_flags.get() & UVM_VNODE_VNISLOCKED != 0;
    let uvnsize = uvn.u_size.get();
    rw_exit(lock);
    if kva == 0 {
        kva = uvm_pagermapin(pps, npages_u, mapinflags | UVMPAGER_MAPIN_WAITOK);
    }

    // ok, mapped in. our pages are PG_BUSY so they are not going to get touched (so we can
    // look at "offset" without having to lock the object). set up for I/O.
    // fill out uio/iov
    let mut wanted = npages_u << PAGE_SHIFT;
    if file_offset + wanted as Voff > uvnsize {
        wanted = (uvnsize - file_offset) as usize; // XXX: needed?
    }
    let mut iov = [Iovec {
        iov_base: kva as *mut c_void,
        iov_len: wanted,
    }];
    let p = curproc();
    let mut uio = Uio {
        uio_iov: &mut iov,
        uio_offset: file_offset,
        uio_segflg: UioSeg::UIO_SYSSPACE,
        uio_rw: rw,
        uio_resid: wanted,
        uio_procp: p,
    };

    // This process may already have the NET_LOCK(), if we faulted in copyin() or copyout()
    // in the network stack.
    let netunlocked = if crate::kern::kern_rwlock::rw_status(&NETLOCK) == RW_WRITE {
        net_unlock();
        true
    } else {
        false
    };

    // do the I/O! (XXX: curproc?)
    //
    // This process may already have this vnode locked, if we faulted in copyin() or
    // copyout() on a region backed by this vnode while doing I/O to the vnode. If this is the
    // case, don't panic.. instead, return the error to the user.
    //
    // XXX this is a stopgap to prevent a panic. Ideally, this kind of operation *should* work.
    kernel_lock();
    let cred = p.map_or(ptr::null(), |p| p.p_ucred.get());
    let mut result = Ok(());
    if !vnlocked {
        result = vn_lock(vn, LK_EXCLUSIVE | LK_RECURSEFAIL | lkflags);
    }
    if result.is_ok() {
        // NOTE: vnode now locked!
        result = if rw == UioRw::UIO_READ {
            VOP_READ(vn, &mut uio, 0, cred)
        } else {
            let ioflag = if flags & PGO_PDFREECLUST != 0 {
                IO_NOCACHE
            } else {
                0
            };
            VOP_WRITE(vn, &mut uio, ioflag, cred)
        };

        if !vnlocked {
            let _ = VOP_UNLOCK(vn);
        }
    }
    kernel_unlock();

    if netunlocked {
        net_lock();
    }

    // NOTE: vnode now unlocked (unless vnislocked)
    //
    // result == unix style errno (0 == OK!)
    //
    // zero out rest of buffer (if needed)
    if result.is_ok() {
        let got = wanted - uio.uio_resid;

        if wanted != 0 && got == 0 {
            result = Err(Errno::EIO); // XXX: error?
        } else if got < PAGE_SIZE * npages_u && rw == UioRw::UIO_READ {
            // SAFETY: `kva` maps the `npages` busy pages of this I/O (uvm_pagermapin), so the
            // bytes from `got` to the end are the pages' own.
            unsafe { ptr::write_bytes((kva + got) as *mut u8, 0, (npages_u << PAGE_SHIFT) - got) };
        }
    }

    // now remove pager mapping
    uvm_pagermapout(kva, npages_u);

    // now clean up the object (i.e. drop I/O count)
    let _ = rw_enter(lock, RW_WRITE);
    uvn.u_nio.set(uvn.u_nio.get() - 1); // I/O DONE!
    if uvn.u_flags.get() & UVM_VNODE_IOSYNC != 0 && uvn.u_nio.get() == 0 {
        wakeup(ptr::from_ref(&uvn.u_nio));
    }

    match result {
        Ok(()) => VM_PAGER_OK,
        Err(Errno::EBUSY) => {
            kassert!(flags & PGO_NOWAIT != 0);
            VM_PAGER_AGAIN
        }
        Err(_) => {
            if REBOOTING.load(core::sync::atomic::Ordering::Relaxed) {
                kernel_lock();
                while REBOOTING.load(core::sync::atomic::Ordering::Relaxed) {
                    let _ = tsleep_nsec(ptr::from_ref(&REBOOTING), PVM, "uvndead", INFSLP);
                }
                kernel_unlock();
            }
            VM_PAGER_ERROR
        }
    }
}

/// uvm_vnp_uncache: disable "persisting" in a vnode... when last reference is gone we will
/// kill the object (flushing dirty pages back to the vnode if needed).
///
/// => returns TRUE if there was no uvm_object attached or if there was one and we killed it
///    [i.e. if there is no active uvn]
/// => called with the vnode VOP_LOCK'd [we will unlock it for I/O, if needed]
///
/// => XXX: given that we now kill uvn's when a vnode is recycled (without having to hold a
///    reference on the vnode) and given a working uvm_vnp_sync(), how does that effect the
///    need for this function? [XXXCDC: seems like it can die?]
///
/// => XXX: this function should DIE once we merge the VM and buffer cache.
///
/// research shows that this is called in the following places: ext2fs_truncate,
/// ffs_truncate, detrunc[msdosfs]: called when vnode changes sizes; ext2fs_write, WRITE
/// [ufs_readwrite], msdosfs_write: called when we are written to; ex2fs_chmod, ufs_chmod:
/// called if VTEXT vnode and the sticky bit is off; ffs_realloccg: when we can't extend the
/// current block and have to allocate a new one we call this [XXX: why?]; nfsrv_rename,
/// rename_files: called when the target filename is there and we want to remove it;
/// nfsrv_remove, sys_unlink: called on file we are removing; nfsrv_access: if VTEXT and we
/// want WRITE access and we don't uncache then return "text busy"; nfs_open: seems to uncache
/// any file opened with nfs; vn_writechk: if VTEXT vnode and can't uncache return "text
/// busy"; fusefs_open: uncaches any file that is opened; fusefs_write: uncaches on every
/// write.
pub fn uvm_vnp_uncache(vp: &'static Vnode) -> bool {
    let Some(uvn) = v_uvm(vp) else {
        return true;
    };
    let uobj = &uvn.u_obj;
    let lock = uobj.vmobjlock();
    let _ = rw_enter(lock, RW_WRITE);
    if uvn.u_flags.get() & UVM_VNODE_VALID == 0 || uvn.u_flags.get() & UVM_VNODE_BLOCKED != 0 {
        rw_exit(lock);
        return true;
    }

    // we have a valid, non-blocked uvn. clear persist flag. if uvn is currently active we can
    // return now.
    uvn.u_flags.set(uvn.u_flags.get() & !UVM_VNODE_CANPERSIST);
    if uobj.uo_refs.get() != 0 {
        rw_exit(lock);
        return false;
    }

    // uvn is currently persisting! we have to gain a reference to it so that we can call
    // uvn_detach to kill the uvn.
    vref(vp); // seems ok, even with VOP_LOCK
    uobj.uo_refs.set(uobj.uo_refs.get() + 1); // value is now 1
    rw_exit(lock);

    // VFSLCKDEBUG: the "vnode not locked" check is not configured.

    // now drop our reference to the vnode. if we have the sole reference to the vnode then
    // this will cause it to die [as we just cleared the persist flag]. we have to unlock the
    // vnode while we are doing this as it may trigger I/O.
    //
    // XXX: it might be possible for uvn to get reclaimed while we are unlocked causing us to
    // return TRUE when we should not. we ignore this as a false-positive return value doesn't
    // hurt us.
    let _ = VOP_UNLOCK(vp);
    uvn_detach(uobj);
    let _ = vn_lock(vp, LK_EXCLUSIVE | LK_RETRY);

    true
}

/// uvm_vnp_setsize: grow or shrink a vnode uvn
///
/// grow   => just update size value
/// shrink => toss un-needed pages
///
/// => we assume that the caller has a reference of some sort to the vnode in question so that
///    it will not be yanked out from under us.
///
/// called from:
///  => truncate fns (ext2fs_truncate, ffs_truncate, detrunc[msdos], fusefs_setattr)
///  => "write" fns (ext2fs_write, WRITE [ufs/ufs], msdosfs_write, nfs_write fusefs_write)
///  => ffs_balloc [XXX: why? doesn't WRITE handle?]
///  => NFS: nfs_loadattrcache, nfs_getattrcache, nfs_setattr
///  => union fs: union_newsize
pub fn uvm_vnp_setsize(vp: &'static Vnode, newsize: Voff) {
    kernel_assert_locked();
    let Some(uvn) = v_uvm(vp) else {
        return;
    };
    let uobj = &uvn.u_obj;
    let lock = uobj.vmobjlock();
    let _ = rw_enter(lock, RW_WRITE);
    if uvn.u_flags.get() & UVM_VNODE_VALID != 0 {
        // now check if the size has changed: if we shrink we had better toss some pages...
        if uvn.u_size.get() > newsize {
            let _ = uvn_flush(uobj, newsize, uvn.u_size.get(), PGO_FREE);
        }
        uvn.u_size.set(newsize);
    }
    rw_exit(lock);
}

/// uvm_vnp_sync: flush all dirty VM pages back to their backing vnodes.
///
/// => called from sys_sync with no VM structures locked
/// => only one process can do a sync at a time (because the uvn structure only has one queue
///    for sync'ing). we ensure this by holding the uvn_sync_lock while the sync is in
///    progress. other processes attempting a sync will sleep on this lock until we are done.
pub fn uvm_vnp_sync(mp: Option<&'static Mount>) {
    // step 1: ensure we are only ones using the uvn_sync_q by locking our lock...
    rw_enter_write(&UVN_SYNC_LOCK);

    // step 2: build up a simpleq of uvns of interest based on the write list. we gain a
    // reference to uvns of interest.
    UVN_SYNC_Q.0.init();
    for uvn in UVN_WLIST.0.iter() {
        let vp = uvn.vnode();
        if let Some(mp) = mp
            && !vp.v_mount.get().is_some_and(|m| ptr::eq(m, mp))
        {
            continue;
        }

        // If the vnode is "blocked" it means it must be dying, which in turn means its in the
        // process of being flushed out so we can safely skip it.
        if uvn.u_flags.get() & UVM_VNODE_BLOCKED != 0 {
            continue;
        }

        // gain reference. watch out for persisting uvns (need to regain vnode REF).
        if uvn.u_obj.uo_refs.get() == 0 {
            vref(vp);
        }
        uvn.u_obj.uo_refs.set(uvn.u_obj.uo_refs.get() + 1);

        // SAFETY: the sync queue was just emptied and only this function (under
        // `uvn_sync_lock`) links uvns into it; uvns are never freed.
        unsafe { UVN_SYNC_Q.0.insert_head(uvn) };
    }

    // step 3: we now have a list of uvn's that may need cleaning.
    for uvn in UVN_SYNC_Q.0.iter() {
        let lock = uvn.u_obj.vmobjlock();
        let _ = rw_enter(lock, RW_WRITE);
        // DEBUG: the "dying vnode on sync list" printf.
        let _ = uvn_flush(
            &uvn.u_obj,
            0,
            0,
            PGO_CLEANIT | PGO_ALLPAGES | PGO_DOACTCLUST,
        );

        // if we have the only reference and we just cleaned the uvn, then we can pull it out
        // of the UVM_VNODE_WRITEABLE state thus allowing us to avoid thinking about flushing
        // it again on later sync ops.
        if uvn.u_obj.uo_refs.get() == 1 && uvn.u_flags.get() & UVM_VNODE_WRITEABLE != 0 {
            // SAFETY: a uvn with `UVM_VNODE_WRITEABLE` is on the wlist.
            unsafe { ListHead::<UvnWlist>::remove(uvn) };
            uvn.u_flags.set(uvn.u_flags.get() & !UVM_VNODE_WRITEABLE);
        }
        rw_exit(lock);

        // now drop our reference to the uvn
        uvn_detach(&uvn.u_obj);
    }

    rw_exit_write(&UVN_SYNC_LOCK);
}

/// `(struct uvm_vnode *)` of an object whose pager may not be the vnode pager: `None` for
/// any other object.
pub fn uvn_of(uobj: &UvmObject) -> Option<&UvmVnode> {
    uvm_obj_is_vnode(uobj).then(|| uvn(uobj))
}

const _: () = assert!(core::mem::offset_of!(UvmVnode, u_obj) == 0);
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for the vnode pager over `memfs`, a vnode whose `VOP_READ`/`VOP_WRITE` work on
    // an in-memory file: attaching, paging in (with the tail of the last page zeroed), cleaning
    // a dirty page back to the file through `uvm_vnp_sync`, persisting after the last detach,
    // shrinking with `uvm_vnp_setsize`, `uvm_vnp_uncache` and `uvm_vnp_terminate`.

    use std::sync::{Mutex, MutexGuard};
    use std::vec::Vec;
    use std::{assert, assert_eq, vec};

    use super::*;
    use crate::kern::kern_rwlock::rw_obj_init;
    use crate::kern::kern_subr::uiomove;
    use crate::kern::subr_xxx::nullop;
    use crate::kern::vfs_subr::getnewvnode;
    use crate::machine::Machine;
    use crate::machine::cpu::Cpu;
    use crate::machine::pmap::pmap_map_direct;
    use crate::sys::vnode::{
        VREG, VT_NON, VopGetattrArgs, VopInactiveArgs, VopReadArgs, VopWriteArgs, Vops,
    };

    /// The file's bytes.
    static FILE: Mutex<Vec<u8>> = Mutex::new(Vec::new());

    fn file<R>(f: impl FnOnce(&mut Vec<u8>) -> R) -> R {
        f(&mut FILE.lock().unwrap_or_else(|e| e.into_inner()))
    }

    fn memfs_getattr(ap: &mut VopGetattrArgs<'_>) -> Result<(), Errno> {
        *ap.a_vap = Vattr::new();
        ap.a_vap.va_type = VREG;
        ap.a_vap.va_size = file(|f| f.len()) as u64;
        Ok(())
    }

    fn memfs_read(ap: &mut VopReadArgs<'_, '_>) -> Result<(), Errno> {
        let uio = &mut *ap.a_uio;
        let mut data = file(|f| f.clone());
        let off = (uio.uio_offset as usize).min(data.len());
        let n = uio.uio_resid.min(data.len() - off);
        uiomove(&mut data[off..off + n], uio)
    }

    fn memfs_write(ap: &mut VopWriteArgs<'_, '_>) -> Result<(), Errno> {
        let uio = &mut *ap.a_uio;
        let off = uio.uio_offset as usize;
        let mut buf = vec![0u8; uio.uio_resid];
        uiomove(&mut buf, uio)?;
        file(|f| {
            if f.len() < off + buf.len() {
                f.resize(off + buf.len(), 0);
            }
            f[off..off + buf.len()].copy_from_slice(&buf);
        });
        Ok(())
    }

    fn memfs_inactive(ap: &mut VopInactiveArgs<'_>) -> Result<(), Errno> {
        VOP_UNLOCK(ap.a_vp)
    }

    /// `vops` of `memfs`: no locking, the file in memory.
    static MEMFS_VOPS: Vops = Vops {
        vop_lock: Some(|_| nullop()),
        vop_unlock: Some(|_| nullop()),
        vop_islocked: Some(|_| 0),
        vop_inactive: Some(memfs_inactive),
        vop_reclaim: Some(|_| nullop()),
        vop_getattr: Some(memfs_getattr),
        vop_read: Some(memfs_read),
        vop_write: Some(memfs_write),
        ..Vops::EMPTY
    };

    /// Memory, the vnode table, the pager, a file of `len` bytes (`i % 251`) and its vnode.
    fn setup(len: usize) -> (MutexGuard<'static, ()>, &'static Vnode) {
        let (g, p) = crate::kern::vfs_subr::tests::setup();
        rw_obj_init();
        uvn_init();
        Machine::set_curproc(Machine::curcpu(), p);
        file(|f| *f = (0..len).map(|i| (i % 251) as u8).collect());
        let vp = getnewvnode(VT_NON, None, &MEMFS_VOPS).expect("a vnode");
        vp.v_type.set(VREG);
        (g, vp)
    }

    fn teardown() {
        Machine::set_curproc(Machine::curcpu(), ptr::null());
    }

    /// The page's bytes through the direct map.
    fn bytes(pg: &VmPage) -> &'static mut [u8] {
        let va = pmap_map_direct(pg).as_usize();
        // SAFETY: the test owns the busy page; the host's direct map is the page itself.
        unsafe { core::slice::from_raw_parts_mut(va as *mut u8, PAGE_SIZE) }
    }

    /// `pgo_get` of the page at `off` without the fault's locks: busy, resident.
    fn get(uobj: &UvmObject, off: Voff) -> &'static VmPage {
        let mut pps = [ptr::null::<VmPage>(); 1];
        let mut npages = 1;
        let _ = rw_enter(uobj.vmobjlock(), RW_WRITE);
        let r = uvn_get(uobj, off, &mut pps, &mut npages, 0, PROT_READ, 0, 0);
        assert_eq!(r, VM_PAGER_OK);
        pps_page(pps[0])
    }

    #[test]
    fn attach_get_dirty_sync_and_persist() {
        let (_g, vp) = setup(PAGE_SIZE + 100);
        let use0 = vp.v_usecount.get();

        let uobj = uvn_attach(vp, PROT_READ | PROT_WRITE).expect("attached");
        let uvn = uvn(uobj);
        assert!(vp.v_uvm.get().is_some_and(|u| ptr::eq(u, uvn)));
        assert_eq!(uobj.uo_refs.get(), 1);
        assert_eq!(uvn.u_size.get(), (PAGE_SIZE + 100) as Voff);
        assert!(uvn.u_flags.get() & UVM_VNODE_VALID != 0);
        assert!(uvn.u_flags.get() & UVM_VNODE_WRITEABLE != 0);
        assert_eq!(vp.v_usecount.get(), use0 + 1);
        // A second attach shares the object.
        let again = uvn_attach(vp, PROT_READ).expect("attached");
        assert!(ptr::eq(again, uobj));
        assert_eq!(uobj.uo_refs.get(), 2);
        uvn_detach(uobj);

        // With the fault's locks nothing is resident yet.
        let mut pps = [ptr::null::<VmPage>(); 2];
        let mut npages = 2;
        let _ = rw_enter(uobj.vmobjlock(), RW_WRITE);
        let r = uvn_get(uobj, 0, &mut pps, &mut npages, 0, PROT_READ, 0, PGO_LOCKED);
        rw_exit(uobj.vmobjlock());
        assert_eq!((r, npages), (VM_PAGER_UNLOCK, 0));

        // Paging in reads the file; the second page holds 100 bytes and zeroes.
        let pg0 = get(uobj, 0);
        assert!(pg0.flags() & PG_BUSY != 0 && pg0.flags() & PG_FAKE == 0);
        assert_eq!(bytes(pg0)[..8], [0, 1, 2, 3, 4, 5, 6, 7]);
        let pg1 = get(uobj, PAGE_SIZE as Voff);
        assert_eq!(bytes(pg1)[99], ((PAGE_SIZE + 99) % 251) as u8);
        assert!(bytes(pg1)[100..].iter().all(|&b| b == 0));
        assert_eq!(uobj.uo_npages.get(), 2);

        // Dirty the first page and let the sync write it back.
        bytes(pg0)[..4].copy_from_slice(b"EMI!");
        pg0.clear_bits(PG_CLEAN | PG_BUSY);
        pg1.clear_bits(PG_BUSY);
        uvm_vnp_sync(None);
        assert_eq!(file(|f| f[..4].to_vec()), b"EMI!");
        assert_eq!(file(|f| f.len()), PAGE_SIZE + 100);
        assert!(pg0.flags() & PG_CLEAN != 0 && pg0.flags() & PG_BUSY == 0);
        assert_eq!(uobj.uo_refs.get(), 1);

        // The last detach keeps the object (it persists) and drops the vnode reference.
        uvn_detach(uobj);
        assert_eq!(uobj.uo_refs.get(), 0);
        assert!(uvn.u_flags.get() & UVM_VNODE_VALID != 0);
        assert_eq!(uobj.uo_npages.get(), 2);
        assert_eq!(vp.v_usecount.get(), use0);

        // Re-attaching finds the cached pages.
        let uobj = uvn_attach(vp, PROT_READ).expect("attached");
        assert_eq!(uobj.uo_refs.get(), 1);
        assert!(uvm_pagelookup(uobj, 0).is_some_and(|p| ptr::eq(p, pg0)));

        // Shrinking tosses the pages from the one holding the new end (the C truncates the start
        // to a page).
        uvm_vnp_setsize(vp, PAGE_SIZE as Voff + 10);
        assert_eq!(uobj.uo_npages.get(), 1);
        assert!(uvm_pagelookup(uobj, PAGE_SIZE as Voff).is_none());
        assert_eq!(uvn.u_size.get(), PAGE_SIZE as Voff + 10);

        // Uncaching an active object only stops it persisting.
        assert!(!uvm_vnp_uncache(vp));
        assert!(uvn.u_flags.get() & UVM_VNODE_CANPERSIST == 0);
        uvn_detach(uobj);
        // Without CANPERSIST the last detach kills the object and frees its pages.
        assert_eq!(uvn.u_flags.get(), 0);
        assert_eq!(uobj.uo_npages.get(), 0);
        assert_eq!(vp.v_usecount.get(), use0);
        teardown();
    }

    #[test]
    fn terminate_frees_a_persisting_object() {
        let (_g, vp) = setup(3 * PAGE_SIZE);
        let uobj = uvn_attach(vp, PROT_READ).expect("attached");
        let pg = get(uobj, PAGE_SIZE as Voff);
        pg.clear_bits(PG_BUSY);
        uvn_detach(uobj);
        let uvn = uvn(uobj);
        assert!(uvn.u_flags.get() & UVM_VNODE_VALID != 0);
        assert_eq!(uobj.uo_npages.get(), 1);

        uvm_vnp_terminate(vp);
        assert_eq!(uvn.u_flags.get(), 0);
        assert_eq!(uobj.uo_npages.get(), 0);
        // A dead object is not synced and cannot be uncached.
        uvm_vnp_sync(None);
        assert!(uvm_vnp_uncache(vp));
        teardown();
    }

    #[test]
    fn a_page_past_the_end_is_bad() {
        let (_g, vp) = setup(100);
        let uobj = uvn_attach(vp, PROT_READ).expect("attached");
        let mut pps = [ptr::null::<VmPage>(); 1];
        let mut npages = 1;
        let _ = rw_enter(uobj.vmobjlock(), RW_WRITE);
        let r = uvn_get(
            uobj,
            PAGE_SIZE as Voff,
            &mut pps,
            &mut npages,
            0,
            PROT_READ,
            0,
            0,
        );
        // uvn_io refuses an offset past the end; uvn_get freed the page and dropped the lock.
        assert_eq!(r, VM_PAGER_BAD);
        assert_eq!(uobj.uo_npages.get(), 0);
        let mut lo = 0;
        let mut hi = 0;
        let _ = rw_enter(uobj.vmobjlock(), RW_WRITE);
        uvn_cluster(uobj, 0, &mut lo, &mut hi);
        rw_exit(uobj.vmobjlock());
        assert_eq!((lo, hi), (0, PAGE_SIZE as Voff));
        uvn_detach(uobj);
        teardown();
    }
}
/* </TESTS> */
