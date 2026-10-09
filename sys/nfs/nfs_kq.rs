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

/*-
 * Copyright (c) 2002 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Jaromir Dolecek.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE NETBSD FOUNDATION, INC. AND CONTRIBUTORS
 * ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE FOUNDATION OR CONTRIBUTORS
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! kqueue(2) on NFS files: the filters (`nfs_kqfilter`, `filt_nfsread`, `filt_nfswrite`,
//! `filt_nfsvnode`, `filt_nfsdetach`) and the poller thread `nfs_kqpoll`, which notices
//! changes made on the server to the watched files (`nfs_kqwatch`, `nfs_kqunwatch`).
//!
//! Upstream: sys/nfs/nfs_kq.c @ 3ce1f3f79392
//!
//! The poller periodically checks for server changes of any of the watched files every
//! `NFS_MINATTRTIMO/2` seconds. Only changes in size, modification time, change time and
//! nlinks are checked, everything else is ignored. It calls `VOP_GETATTR()` only when it is
//! likely to get new data, i.e. when the vnode expires from the attribute cache: the same
//! result as running stat(2) periodically from userland, with little CPU and network use,
//! and still proper kevent semantics. The thread is created when the first vnode is added
//! to the watch list and exits when the list is empty.
//!
//! ## Deviations
//! - `struct kevq` entries are `malloc(9)`ed `Kevq`s with `Cell` members, linked through
//!   `kev_link` (adapter `KevLink`); `kevlist` and `pnfskq` are statics (`KEVLIST`, an
//!   `AtomicPtr` `PNFSKQ`) under `nfskevq_lock`, as in C.
//! - `nfs_kqpoll` uses `curproc` (itself) for `VOP_GETATTR` where the C reads `pnfskq`: the
//!   same thread, and `kthread_create` here returns the new thread instead of storing it
//!   through a pointer before the thread can run.
//! - The `DEBUG` `printf`s are behind feature `debug`.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicPtr, Ordering};

use crate::kern::kern_event::{klist_insert_locked, klist_remove_locked};
use crate::kern::kern_kthread::{kthread_create, kthread_exit};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_rwlock::{rw_enter_write, rw_exit_write};
use crate::kern::kern_synch::{tsleep_nsec, wakeup};
use crate::kern::subr_prf::panic;
#[cfg(feature = "debug")]
use crate::kern::subr_prf::printf;
use crate::kern::vfs_vops::VOP_GETATTR;
#[cfg(feature = "debug")]
use crate::kern::vfs_vops::VOP_PRINT;
use crate::machine::cpu::curproc;
use crate::nfs::nfs::NFS_MINATTRTIMO;
use crate::nfs::nfs_subs::nfs_getattrcache;
use crate::nfs::nfsnode::{VTONFS, nfs_invalidate_attrcache};
use crate::queue_adapter;
use crate::sys::errno::Errno;
use crate::sys::event::{
    __EV_POLL, __EV_SELECT, EV_EOF, EV_ONESHOT, EVFILT_READ, EVFILT_VNODE, EVFILT_WRITE,
    FILTEROP_ISFD, Filterops, Knote, NOTE_ATTRIB, NOTE_DELETE, NOTE_EOF, NOTE_EXTEND, NOTE_LINK,
    NOTE_REVOKE, NOTE_TRUNCATE, NOTE_WRITE,
};
use crate::sys::file::foffset;
use crate::sys::malloc::{M_KEVENT, M_WAITOK};
use crate::sys::param::PSOCK;
use crate::sys::proc::Proc;
use crate::sys::queue::{SlistEntry, SlistHead};
use crate::sys::rwlock::Rwlock;
use crate::sys::systm::INFSLP;
use crate::sys::time::{Timespec, sec_to_nsec};
use crate::sys::types::Nlink;
use crate::sys::vnode::{VN_KNOTE, Vattr, Vnode, VopKqfilterArgs};

/// `KEVQ_BUSY`: currently being processed.
const KEVQ_BUSY: u32 = 0x01;
/// `KEVQ_WANT`: want to change this entry.
const KEVQ_WANT: u32 = 0x02;

/// `struct kevq`: a watched vnode and what the poller last saw of it.
struct Kevq {
    /// `kev_link`.
    kev_link: SlistEntry<Kevq>,
    /// `vp`.
    vp: &'static Vnode,
    /// `usecount`: the knotes watching `vp`.
    usecount: Cell<u32>,
    /// `flags`: `KEVQ_*`.
    flags: Cell<u32>,
    /// `omtime`: old modification time.
    omtime: Cell<Timespec>,
    /// `octime`: old change time.
    octime: Cell<Timespec>,
    /// `onlink`: old number of references to file.
    onlink: Cell<Nlink>,
}

queue_adapter!(
    /// `SLIST_HEAD(kevqlist, kevq)`: the watch list, through `kev_link`.
    KevLink: Kevq, kev_link => SlistEntry<Kevq>
);

/// `struct kevqlist`.
struct Kevqlist(SlistHead<KevLink>);

// SAFETY: the list and its entries are read and changed only under `NFSKEVQ_LOCK`; an entry
// marked `KEVQ_BUSY` stays linked and allocated while the poller works on it unlocked.
unsafe impl Sync for Kevqlist {}

/// `nfskevq_lock`.
static NFSKEVQ_LOCK: Rwlock = Rwlock::new("nfskqlk");
/// `pnfskq`: the poller thread, NULL when none runs (under `NFSKEVQ_LOCK`).
static PNFSKQ: AtomicPtr<Proc> = AtomicPtr::new(ptr::null_mut());
/// `kevlist`: the watched vnodes.
static KEVLIST: Kevqlist = Kevqlist(SlistHead::new());

/// `nfsread_filtops`.
static NFSREAD_FILTOPS: Filterops = Filterops {
    f_flags: FILTEROP_ISFD,
    f_attach: None,
    f_detach: Some(filt_nfsdetach),
    f_event: Some(filt_nfsread),
    f_modify: None,
    f_process: None,
};

/// `nfswrite_filtops`.
static NFSWRITE_FILTOPS: Filterops = Filterops {
    f_flags: FILTEROP_ISFD,
    f_attach: None,
    f_detach: Some(filt_nfsdetach),
    f_event: Some(filt_nfswrite),
    f_modify: None,
    f_process: None,
};

/// `nfsvnode_filtops`.
static NFSVNODE_FILTOPS: Filterops = Filterops {
    f_flags: FILTEROP_ISFD,
    f_attach: None,
    f_detach: Some(filt_nfsdetach),
    f_event: Some(filt_nfsvnode),
    f_modify: None,
    f_process: None,
};

/// `nfs_kqpoll(arg)`: the poller thread's body. It checks the watched files whose cached
/// attributes expired, posts the events their changes imply, and exits when nothing is
/// watched any more.
pub fn nfs_kqpoll(_arg: *mut c_void) {
    let Some(p) = curproc() else {
        panic(format_args!("nfs_kqpoll: no thread"));
    };

    loop {
        rw_enter_write(&NFSKEVQ_LOCK);
        let mut cur = KEVLIST.0.first();
        while let Some(ke) = cur {
            let np = VTONFS(ke.vp);
            let mut attr = Vattr::new();

            #[cfg(feature = "debug")]
            {
                printf(format_args!("nfs_kqpoll on: "));
                let _ = VOP_PRINT(ke.vp);
            }
            // skip if still in attrcache
            if nfs_getattrcache(ke.vp, &mut attr) != Err(Errno::ENOENT) {
                cur = SlistHead::<KevLink>::next(ke);
                continue;
            }

            // Mark entry busy, release lock and check for changes.
            ke.flags.set(ke.flags.get() | KEVQ_BUSY);
            rw_exit_write(&NFSKEVQ_LOCK);

            // save v_size, nfs_getattr() updates it
            let osize = np.n_size.get();

            'next: {
                if VOP_GETATTR(ke.vp, &mut attr, p.p_ucred.get(), p) == Err(Errno::ESTALE) {
                    nfs_invalidate_attrcache(np);
                    VN_KNOTE(ke.vp, NOTE_DELETE);
                    break 'next;
                }

                // following is a bit fragile, but about best we can get
                if attr.va_size != osize {
                    let mut flags = NOTE_WRITE;

                    if attr.va_size > osize {
                        flags |= NOTE_EXTEND;
                    } else {
                        flags |= NOTE_TRUNCATE;
                    }

                    VN_KNOTE(ke.vp, flags);
                    ke.omtime.set(attr.va_mtime);
                } else if attr.va_mtime != ke.omtime.get() {
                    VN_KNOTE(ke.vp, NOTE_WRITE);
                    ke.omtime.set(attr.va_mtime);
                }

                if attr.va_ctime != ke.octime.get() {
                    VN_KNOTE(ke.vp, NOTE_ATTRIB);
                    ke.octime.set(attr.va_ctime);
                }

                if attr.va_nlink != ke.onlink.get() {
                    VN_KNOTE(ke.vp, NOTE_LINK);
                    ke.onlink.set(attr.va_nlink);
                }
            }

            // next:
            rw_enter_write(&NFSKEVQ_LOCK);
            ke.flags.set(ke.flags.get() & !KEVQ_BUSY);
            if ke.flags.get() & KEVQ_WANT != 0 {
                ke.flags.set(ke.flags.get() & !KEVQ_WANT);
                wakeup(ptr::from_ref(ke));
            }
            cur = SlistHead::<KevLink>::next(ke);
        }

        if KEVLIST.0.is_empty() {
            // Nothing more to watch, exit
            PNFSKQ.store(ptr::null_mut(), Ordering::Relaxed);
            rw_exit_write(&NFSKEVQ_LOCK);
            kthread_exit(0);
        }
        rw_exit_write(&NFSKEVQ_LOCK);

        // wait a while before checking for changes again
        let _ = tsleep_nsec(
            PNFSKQ.load(Ordering::Relaxed).cast_const(),
            PSOCK,
            "nfskqpw",
            sec_to_nsec(NFS_MINATTRTIMO as u64) / 2,
        );
    }
}

/// `kn->kn_hook` of an NFS knote: its vnode.
fn kn_vnode(kn: &Knote) -> &'static Vnode {
    // SAFETY: `nfs_kqfilter` points `kn_hook` at the vnode, a `vnode_pool` item that is never
    // freed.
    match unsafe { kn.kn_hook.get().cast::<Vnode>().as_ref() } {
        Some(vp) => vp,
        None => panic(format_args!("knote {:p}: no vnode", kn)),
    }
}

/// `filt_nfsdetach`: unhook the knote from the vnode and stop watching the vnode for it.
pub fn filt_nfsdetach(kn: &Knote) {
    let vp = kn_vnode(kn);

    klist_remove_locked(&vp.v_klist, kn);

    // Remove the vnode from watch list
    if !kn.has_flags(__EV_POLL | __EV_SELECT) {
        nfs_kqunwatch(vp);
    }
}

/// `nfs_kqunwatch(vp)`: drop one watcher of `vp`; the last one takes it off the list.
pub fn nfs_kqunwatch(vp: &Vnode) {
    rw_enter_write(&NFSKEVQ_LOCK);
    let mut cur = KEVLIST.0.first();
    while let Some(ke) = cur {
        if ptr::eq(ke.vp, vp) {
            while ke.flags.get() & KEVQ_BUSY != 0 {
                ke.flags.set(ke.flags.get() | KEVQ_WANT);
                rw_exit_write(&NFSKEVQ_LOCK);
                let _ = tsleep_nsec(ptr::from_ref(ke), PSOCK, "nfskqdet", INFSLP);
                rw_enter_write(&NFSKEVQ_LOCK);
            }

            if ke.usecount.get() > 1 {
                // keep, other kevents need this
                ke.usecount.set(ke.usecount.get() - 1);
            } else {
                // last user, g/c
                // SAFETY: `ke` is on `kevlist` (found there under the lock, and only this
                // function unlinks entries, under the lock).
                unsafe { KEVLIST.0.remove(ke) };
                free(NonNull::from(ke).cast::<u8>(), M_KEVENT, size_of::<Kevq>());
            }
            break;
        }
        cur = SlistHead::<KevLink>::next(ke);
    }
    rw_exit_write(&NFSKEVQ_LOCK);
}

/// `filt_nfsread`: the bytes past the file offset; always ready for poll and select.
pub fn filt_nfsread(kn: &Knote, hint: i64) -> bool {
    let vp = kn_vnode(kn);
    let np = VTONFS(vp);

    // filesystem is gone, so set the EOF flag and schedule the knote for deletion.
    if hint == i64::from(NOTE_REVOKE) {
        kn.set_flags(EV_EOF | EV_ONESHOT);
        return true;
    }

    // The C subtracts in u_quad_t and keeps the int64_t.
    kn.kn_data()
        .set((np.n_size.get() as i64).wrapping_sub(foffset(kn.fp())));
    #[cfg(feature = "debug")]
    printf(format_args!("nfsread event. {}\n", kn.kn_data().get()));
    if kn.kn_data().get() == 0 && kn.kn_sfflags.get() & NOTE_EOF != 0 {
        kn.kn_fflags().set(kn.kn_fflags().get() | NOTE_EOF);
        return true;
    }

    if kn.has_flags(__EV_POLL | __EV_SELECT) {
        return true;
    }

    kn.kn_data().get() != 0
}

/// `filt_nfswrite`: an NFS file is always writable.
pub fn filt_nfswrite(kn: &Knote, hint: i64) -> bool {
    // filesystem is gone, so set the EOF flag and schedule the knote for deletion.
    if hint == i64::from(NOTE_REVOKE) {
        kn.set_flags(EV_EOF | EV_ONESHOT);
        return true;
    }

    kn.kn_data().set(0);
    true
}

/// `filt_nfsvnode`: records the vnode events (`NOTE_*`) the user asked for.
pub fn filt_nfsvnode(kn: &Knote, hint: i64) -> bool {
    let hint32 = hint as u32;
    if kn.kn_sfflags.get() & hint32 != 0 {
        kn.kn_fflags().set(kn.kn_fflags().get() | hint32);
    }
    if hint == i64::from(NOTE_REVOKE) {
        kn.set_flags(EV_EOF);
        return true;
    }
    kn.kn_fflags().get() != 0
}

/// `nfs_kqfilter` (`vop_kqfilter`): attach a knote to an NFS vnode and, unless it is for
/// poll(2) or select(2), put the vnode on the poller's watch list.
pub fn nfs_kqfilter(ap: &mut VopKqfilterArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let kn = ap.a_kn;

    #[cfg(feature = "debug")]
    {
        printf(format_args!("nfs_kqfilter({}) on: ", kn.kn_filter().get()));
        let _ = VOP_PRINT(vp);
    }

    match kn.kn_filter().get() {
        EVFILT_READ => kn.kn_fop.set(Some(&NFSREAD_FILTOPS)),
        EVFILT_WRITE => kn.kn_fop.set(Some(&NFSWRITE_FILTOPS)),
        EVFILT_VNODE => kn.kn_fop.set(Some(&NFSVNODE_FILTOPS)),
        _ => return Err(Errno::EINVAL),
    }

    kn.kn_hook.set(ptr::from_ref(vp).cast_mut().cast());

    // Put the vnode to watched list.
    if !kn.has_flags(__EV_POLL | __EV_SELECT) {
        nfs_kqwatch(vp)?;
    }

    klist_insert_locked(&vp.v_klist, kn);

    Ok(())
}

/// `nfs_kqwatch(vp)`: watch `vp`, starting the poller thread if it is not running.
pub fn nfs_kqwatch(vp: &'static Vnode) -> Result<(), Errno> {
    let Some(p) = curproc() else {
        // XXX
        panic(format_args!("nfs_kqwatch: no thread"));
    };

    // Fetch current attributes. It's only needed when the vnode is not watched yet, but we
    // need to do this without lock held. This is likely cheap due to attrcache, so do it now.
    let mut attr = Vattr::new();
    let _ = VOP_GETATTR(vp, &mut attr, p.p_ucred.get(), p);

    rw_enter_write(&NFSKEVQ_LOCK);

    let error = 'out: {
        // ensure the poller is running
        if PNFSKQ.load(Ordering::Relaxed).is_null() {
            match kthread_create(nfs_kqpoll, ptr::null_mut(), b"nfskqpoll") {
                Ok(np) => PNFSKQ.store(ptr::from_ref(np).cast_mut(), Ordering::Relaxed),
                Err(e) => break 'out Err(e),
            }
        }

        if let Some(ke) = KEVLIST.0.iter().find(|ke| ptr::eq(ke.vp, vp)) {
            // already watched, so just bump usecount
            ke.usecount.set(ke.usecount.get() + 1);
        } else {
            // need a new one
            let Some(mem) = malloc(size_of::<Kevq>(), M_KEVENT, M_WAITOK) else {
                panic(format_args!("nfs_kqwatch: malloc"));
            };
            let kp = mem.cast::<Kevq>();
            // SAFETY: a fresh `malloc(9)` block of `size_of::<Kevq>()` bytes, aligned by
            // malloc, written once before anything else sees it.
            unsafe {
                kp.as_ptr().write(Kevq {
                    kev_link: SlistEntry::new(),
                    vp,
                    usecount: Cell::new(1),
                    flags: Cell::new(0),
                    omtime: Cell::new(attr.va_mtime),
                    octime: Cell::new(attr.va_ctime),
                    onlink: Cell::new(attr.va_nlink),
                })
            };
            // SAFETY: as above; it stays in place until `nfs_kqunwatch` unlinks and frees it.
            let ke: &'static Kevq = unsafe { kp.as_ref() };
            // SAFETY: a new entry, on no list; changed under the lock.
            unsafe { KEVLIST.0.insert_head(ke) };
        }

        // kick the poller
        wakeup(PNFSKQ.load(Ordering::Relaxed).cast_const());
        Ok(())
    };

    rw_exit_write(&NFSKEVQ_LOCK);
    error
}

const _: () = assert!(!core::mem::needs_drop::<Kevq>());
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_filter_is_always_ready() {
        let kn = Knote::new();
        kn.kn_data().set(7);
        assert!(filt_nfswrite(&kn, 0));
        assert_eq!(kn.kn_data().get(), 0);
        assert!(filt_nfswrite(&kn, i64::from(NOTE_REVOKE)));
        assert!(kn.has_flags(EV_EOF) && kn.has_flags(EV_ONESHOT));
    }

    #[test]
    fn vnode_filter_records_requested_events() {
        let kn = Knote::new();
        kn.kn_sfflags.set(NOTE_WRITE | NOTE_ATTRIB);
        assert!(!filt_nfsvnode(&kn, i64::from(NOTE_LINK)));
        assert!(filt_nfsvnode(&kn, i64::from(NOTE_WRITE)));
        assert_eq!(kn.kn_fflags().get(), NOTE_WRITE);
        assert!(filt_nfsvnode(&kn, i64::from(NOTE_ATTRIB)));
        assert_eq!(kn.kn_fflags().get(), NOTE_WRITE | NOTE_ATTRIB);
        assert!(filt_nfsvnode(&kn, i64::from(NOTE_REVOKE)));
        assert!(kn.has_flags(EV_EOF) && !kn.has_flags(EV_ONESHOT));
    }
}
/* </TESTS> */
