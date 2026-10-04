/*	$OpenBSD: bpfdesc.h,v 1.50 2024/11/19 23:26:35 dlg Exp $	*/
/*	$NetBSD: bpfdesc.h,v 1.11 1995/09/27 18:30:42 thorpej Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1990, 1991, 1993
 *	The Regents of the University of California.  All rights reserved.
 *
 * This code is derived from the Stanford/CMU enet packet filter,
 * (net/enet.c) distributed as part of 4.3BSD, and code contributed
 * to Berkeley by Steven McCanne and Van Jacobson both of Lawrence
 * Berkeley Laboratory.
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
 *	@(#)bpfdesc.h	8.1 (Berkeley) 6/10/93
 */
/* </LICENSES> */

//! `<net/bpfdesc.h>`: the kernel's `bpf(4)` structures, the descriptor of an open
//! `/dev/bpf` (`struct bpf_d`) and the tap of an attached interface (`struct bpf_if`).
//!
//! Upstream: sys/net/bpfdesc.h @ 3ce1f3f79392
//!
//! Locks used to protect struct members in this file: \[m\] the per-descriptor mutex
//! (`bd_mtx`); the others the kernel lock, which is the single CPU here.
//!
//! ## Deviations
//! - SMR is not ported (as in `net/if_.rs`): `SMR_SLIST_ENTRY`/`SMR_SLIST_HEAD` are plain
//!   `SlistEntry`/`SlistHead`, `bd_rfilter`/`bd_wfilter` plain `Cell`s, and the `smr_entry`
//!   members (`bps_smr`, `bd_smr`) do not exist: `smr_call` runs its callback at once, which
//!   is sound on one CPU with no kernel preemption (a tap never runs inside the list update,
//!   which holds `bd_mtx` at `IPL_NET`).
//! - `struct bpf_program_smr` holds the kernel copy of the program as a pointer and a count
//!   (`bps_bf.bf_insns`, `bps_bf.bf_len`), handed out as a slice by [`BpfProgramSmr::insns`];
//!   the user-visible `struct bpf_program` (`net/bpf.rs`) carries a user address instead.
//! - `bif_driverp` (`struct bpf_if **` into the driver's softc) is a reference to the
//!   driver's `if_bpf` cell (`caddr_t`), which `bpf_attachd`/`bpf_detachd` set and clear.
//! - `bif_name` is a copy of the name (`IFNAMSIZ` bytes, NUL-padded) instead of a pointer
//!   to it: the names `bpfattach` passes (`if_xname`) never change after attach.
//! - `bd_rcount` is an `AtomicU64` (the C's `atomic_inc_long`).

use core::cell::Cell;
use core::ptr::{self, NonNull};
use core::sync::atomic::AtomicU64;

use crate::machine::intr::IPL_NONE;
use crate::net::bpf::BpfInsn;
use crate::net::if_::IFNAMSIZ;
use crate::net::if_var::Ifnet;
use crate::sys::event::Klist;
use crate::sys::mutex::Mutex;
use crate::sys::queue::{ListEntry, SlistEntry, SlistHead, TailqEntry};
use crate::sys::refcnt::Refcnt;
use crate::sys::sigio::SigioRef;
use crate::sys::task::Task;
use crate::sys::timeout::Timeout;

/// `struct bpf_program_smr`: a filter program the kernel validated and keeps.
pub struct BpfProgramSmr {
    /// `bps_bf.bf_len`: the number of instructions.
    pub bf_len: u32,
    /// `bps_bf.bf_insns`: the instructions, a `mallocarray(M_DEVBUF)` block of `bf_len`.
    pub bf_insns: NonNull<BpfInsn>,
}

impl BpfProgramSmr {
    /// The program's instructions.
    pub fn insns(&self) -> &[BpfInsn] {
        // SAFETY: `bf_insns` is the block `bpf_setf` allocated and filled with `bf_len`
        // instructions; it lives until `bpf_prog_smr` frees it with this structure.
        unsafe { core::slice::from_raw_parts(self.bf_insns.as_ptr(), self.bf_len as usize) }
    }
}

/// `struct bpf_d`: the descriptor associated with each open bpf file.
///
/// Buffer slots: two buffers hold the incoming packets. The model has three slots. Sbuf is
/// always occupied. sbuf (store): the receive path puts packets here. hbuf (hold): when sbuf
/// is full, put the buffer here and wake up read (replace sbuf with fbuf). fbuf (free): when
/// read is done, put the buffer here. On receiving, if sbuf is full and fbuf is 0, the packet
/// is dropped.
pub struct BpfD {
    /// `bd_next`: linked list of descriptors (the interface's `bif_dlist`).
    pub bd_next: SlistEntry<BpfD>,
    /// `bd_mtx`: protects the buffer slots below.
    pub bd_mtx: Mutex,
    /// \[m\] `bd_sbuf`: store slot.
    pub bd_sbuf: Cell<*mut u8>,
    /// \[m\] `bd_hbuf`: hold slot.
    pub bd_hbuf: Cell<*mut u8>,
    /// \[m\] `bd_fbuf`: free slot.
    pub bd_fbuf: Cell<*mut u8>,
    /// \[m\] `bd_slen`: current length of store buffer.
    pub bd_slen: Cell<i32>,
    /// \[m\] `bd_hlen`: current length of hold buffer.
    pub bd_hlen: Cell<i32>,
    /// `bd_bufsize`: absolute length of buffers.
    pub bd_bufsize: Cell<i32>,
    /// `bd_in_uiomove`: for debugging purpose.
    pub bd_in_uiomove: Cell<i32>,
    /// `bd_bif`: interface descriptor.
    pub bd_bif: Cell<Option<&'static BpfIf>>,
    /// \[m\] `bd_rtout`: read timeout in nanoseconds.
    pub bd_rtout: Cell<u64>,
    /// \[m\] `bd_wtout`: wait time in nanoseconds.
    pub bd_wtout: Cell<u64>,
    /// \[m\] `bd_nreaders`: number of threads asleep in `bpfread()`.
    pub bd_nreaders: Cell<u64>,
    /// `bd_rfilter`: read filter code.
    pub bd_rfilter: Cell<Option<&'static BpfProgramSmr>>,
    /// `bd_wfilter`: write filter code.
    pub bd_wfilter: Cell<Option<&'static BpfProgramSmr>>,
    /// `bd_rcount`: number of packets received.
    pub bd_rcount: AtomicU64,
    /// \[m\] `bd_dcount`: number of packets dropped.
    pub bd_dcount: Cell<u64>,
    /// `bd_promisc`: true if listening promiscuously.
    pub bd_promisc: Cell<u8>,
    /// \[m\] `bd_state`: idle, waiting, or timed out.
    pub bd_state: Cell<u8>,
    /// `bd_locked`: true if descriptor is locked.
    pub bd_locked: Cell<u8>,
    /// `bd_fildrop`: true if filtered packets will be dropped.
    pub bd_fildrop: Cell<u8>,
    /// `bd_dirfilt`: direction filter.
    pub bd_dirfilt: Cell<u8>,
    /// `bd_hdrcmplt`: false to fill in src lladdr automatically.
    pub bd_hdrcmplt: Cell<i32>,
    /// `bd_async`: non-zero if packet reception should generate signal.
    pub bd_async: Cell<i32>,
    /// `bd_sig`: signal to send upon packet reception.
    pub bd_sig: Cell<i32>,
    /// `bd_sigio`: async I/O registration.
    pub bd_sigio: SigioRef,
    /// `bd_refcnt`: reference count.
    pub bd_refcnt: Refcnt,
    /// `bd_klist`: list of knotes.
    pub bd_klist: Klist,
    /// `bd_unit`: logical unit number.
    pub bd_unit: Cell<i32>,
    /// `bd_list`: descriptor list.
    pub bd_list: ListEntry<BpfD>,
    /// `bd_wake_task`: defer `pgsigio()` and `selwakeup()`.
    pub bd_wake_task: Task,
    /// `bd_wait_tmo`: delay wakeup after catching pkt.
    pub bd_wait_tmo: Timeout,
}

impl BpfD {
    /// A descriptor with every member zero or empty, as `malloc(M_ZERO)` hands it to
    /// `bpfopen`, which initialises the rest.
    pub const fn new() -> Self {
        Self {
            bd_next: SlistEntry::new(),
            bd_mtx: Mutex::new(IPL_NONE),
            bd_sbuf: Cell::new(ptr::null_mut()),
            bd_hbuf: Cell::new(ptr::null_mut()),
            bd_fbuf: Cell::new(ptr::null_mut()),
            bd_slen: Cell::new(0),
            bd_hlen: Cell::new(0),
            bd_bufsize: Cell::new(0),
            bd_in_uiomove: Cell::new(0),
            bd_bif: Cell::new(None),
            bd_rtout: Cell::new(0),
            bd_wtout: Cell::new(0),
            bd_nreaders: Cell::new(0),
            bd_rfilter: Cell::new(None),
            bd_wfilter: Cell::new(None),
            bd_rcount: AtomicU64::new(0),
            bd_dcount: Cell::new(0),
            bd_promisc: Cell::new(0),
            bd_state: Cell::new(0),
            bd_locked: Cell::new(0),
            bd_fildrop: Cell::new(0),
            bd_dirfilt: Cell::new(0),
            bd_hdrcmplt: Cell::new(0),
            bd_async: Cell::new(0),
            bd_sig: Cell::new(0),
            bd_sigio: SigioRef::new(),
            bd_refcnt: Refcnt::new(),
            bd_klist: Klist::new(),
            bd_unit: Cell::new(0),
            bd_list: ListEntry::new(),
            bd_wake_task: Task::zeroed(),
            bd_wait_tmo: Timeout::zeroed(),
        }
    }
}

impl Default for BpfD {
    fn default() -> Self {
        Self::new()
    }
}

// SAFETY: the members change under `bd_mtx` (\[m\]) or the kernel lock, as in C.
unsafe impl Sync for BpfD {}

crate::queue_adapter!(
    /// `SMR_SLIST_HEAD(, bpf_d)` through `bd_next`: an interface's listeners.
    pub BpfDlist: BpfD, bd_next => SlistEntry<BpfD>
);

crate::queue_adapter!(
    /// `LIST_HEAD(, bpf_d)` through `bd_list`: every open descriptor (`bpf_d_list`).
    pub BpfDList: BpfD, bd_list => ListEntry<BpfD>
);

/// `struct bpf_if`: the descriptor associated with each attached hardware interface.
pub struct BpfIf {
    /// `bif_next`: list of all interfaces.
    pub bif_next: TailqEntry<BpfIf>,
    /// `bif_dlist`: descriptor list.
    pub bif_dlist: SlistHead<BpfDlist>,
    /// `bif_driverp`: the driver's `if_bpf` (pointer into softc).
    pub bif_driverp: &'static Cell<*mut u8>,
    /// `bif_dlt`: link layer type.
    pub bif_dlt: u32,
    /// `bif_hdrlen`: length of header (with padding).
    pub bif_hdrlen: u32,
    /// `bif_name`: name of "subsystem", NUL-padded.
    pub bif_name: [u8; IFNAMSIZ],
    /// `bif_ifp`: corresponding interface.
    pub bif_ifp: Cell<Option<&'static Ifnet>>,
}

// SAFETY: changed only under the kernel lock (the listener list also under each
// descriptor's `bd_mtx`), as in C.
unsafe impl Sync for BpfIf {}

crate::queue_adapter!(
    /// `TAILQ_HEAD(, bpf_if)` through `bif_next`: `bpf_iflist`.
    pub BpfIfList: BpfIf, bif_next => TailqEntry<BpfIf>
);
