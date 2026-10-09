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
 * Copyright (c) 2001 Michael Shalayeff
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
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL THE AUTHOR OR HIS RELATIVES BE LIABLE FOR ANY DIRECT,
 * INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
 * (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
 * SERVICES; LOSS OF MIND, USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT,
 * STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING
 * IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF
 * THE POSSIBILITY OF SUCH DAMAGE.
 */

/*
 * Copyright (c) 2008 David Gwynne <dlg@openbsd.org>
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
 * Copyright (c) 2002 Michael Shalayeff
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
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL THE AUTHOR OR HIS RELATIVES BE LIABLE FOR ANY DIRECT,
 * INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
 * (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
 * SERVICES; LOSS OF MIND, USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT,
 * STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING
 * IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF
 * THE POSSIBILITY OF SUCH DAMAGE.
 */

/*
 * Copyright (c) 2009, 2022, 2023 David Gwynne <dlg@openbsd.org>
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
//! The pf state table synchronisation interface, `pfsync(4)`: `<net/if_pfsync.h>` and
//! `net/if_pfsync.c`. pf calls the hooks (`pfsync_insert_state`, `pfsync_update_state`,
//! `pfsync_delete_state`, ...) as states come and go; pfsync queues the changes in per-slice
//! queues and sends them, in protocol 240 datagrams (`IPPROTO_PFSYNC`, TTL 255), to the sync
//! peer (the `224.0.0.240` group by default) out of the sync interface (`syncdev`). A peer's
//! datagrams come in through `pfsync_input4` (the `inetsw[]` entry) and are applied to the
//! local state table. A fresh `pfsync0` asks its peers for a bulk update of their states.
//!
//! Upstream: sys/net/if_pfsync.h @ 3ce1f3f79392
//! Upstream: sys/net/if_pfsync.c @ 3ce1f3f79392
//!
//! The wire structures (`struct pfsync_header`, the subheader and the messages) are `__packed`
//! in C and `#[repr(C, packed)]` here; they are copied in and out of mbuf data as bytes. The
//! softc is `malloc(M_ZERO)`ed, so the all-zero value of each of its members is valid, and
//! members that change behind the shared softc are `Cell`s under the locks the C names.
//!
//! ## Deviations
//! - The header and the file share this module. `PFSYNCCTL_NAMES` (a `struct ctlname` table)
//!   comes with `<sys/sysctl.h>`.
//! - `pfsyncif` is an `AtomicPtr` (`SMR_PTR_GET`/`SMR_PTR_SET_LOCKED`), and `pfsync_down`
//!   gives SMR's reference back with `smr_call(&smr, refcnt_rele_wake, &sc->sc_refs)` on its
//!   own stack, as in C (M11e). The read sections are explicit where the C's ends with its
//!   block; in `pfsync_defer` and `pfsync_input` the C's section spans early returns and is
//!   left implicit: `smr_read_enter`/`smr_read_leave` only count under `DIAGNOSTIC`, and a
//!   grace period waits for every CPU to switch, which code that does not sleep (both
//!   sections, as in C) never does.
//! - `pfsynccounters` (a `cpumem`) is a static array of atomics (`docs/C_TO_RUST.md`), so
//!   `pfsyncattach` allocates nothing.
//! - `kstat(4)` is not configured (`NKSTAT` 0): the slices have no `s_kstat`, and
//!   `pfsync_kstat_data`, `pfsync_kstat_tpl` and `pfsync_kstat_copy` are compiled out. The
//!   slices keep their `s_stat_*` counters, which the C keeps either way.
//! - `carp(4)` is not configured (`NCARP` 0): the `carp_group_demote_adj` calls and
//!   `if_addgroup(ifp, "carp")` are comments at their sites. `bpf(4)` is configured:
//!   `pfsync_clone_create` attaches a `DLT_PFSYNC` tap and `pfsync_sendout` taps each frame.
//! - `PFSYNC_DEBUG` is not defined: its `KASSERT` in `pfsync_slice_drop` is not ported.
//! - `struct pfsync_slice`'s `__aligned(CACHELINESIZE)` is left out: it keeps the slices'
//!   mutexes on separate cache lines (a performance matter), and `<machine/param.h>`'s
//!   `CACHELINESIZE` is not ported.
//! - The message writers (`struct pfsync_q`'s `write`) and readers (`struct pfsync_act`'s
//!   `in`) take the message bytes as a slice of the mbuf data instead of a `caddr_t`; the
//!   readers get all `count` messages of a subheader at once, as in C.
//! - `pfsync_clear_states` takes the interface name as bytes (NUL-terminated or not);
//!   `pfsync_state_in_use`, `pfsync_defer` and `pfsync_is_up` answer `bool`; `pfsync_up`,
//!   `pfsync_down`, the ioctls and the cloner return `Result`.
//! - `pfsync_bulk_snd_states` stops at a state without a successor before the tail (the C
//!   would dereference the NULL), leaving the rest of the bulk to `pfsync_bulk_snd_tmo`,
//!   which then ends it.

use core::cell::Cell;
use core::ffi::c_void;
use core::mem::{offset_of, size_of};
use core::ptr::{self, NonNull};
use core::slice;
use core::sync::atomic::{AtomicPtr, AtomicU64, Ordering};

use crate::kassert;
use crate::kern::kern_lock::{mtx_enter, mtx_enter_try, mtx_init_flags, mtx_leave};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_prot::suser;
use crate::kern::kern_rwlock::{
    rw_enter_read, rw_enter_write, rw_exit_read, rw_exit_write, rw_init,
};
use crate::kern::kern_smr::{smr_read_enter, smr_read_leave};
use crate::kern::kern_synch::{
    msleep_nsec, refcnt_finalize, refcnt_init, refcnt_rele_wake, refcnt_take, wakeup_one,
};
use crate::kern::kern_sysctl::sysctl_rdstruct;
use crate::kern::kern_task::{task_add, task_del, task_set};
use crate::kern::kern_tc::{getnsecuptime, getuptime};
use crate::kern::kern_timeout::{
    timeout_add_msec, timeout_add_nsec, timeout_add_sec, timeout_barrier, timeout_del, timeout_set,
    timeout_set_proc,
};
use crate::kern::subr_pool::pool_init;
use crate::kern::subr_prf::{Str, panic, printf, snprintf};
use crate::kern::uipc_mbuf::{
    MAX_LINKHDR, m_adj, m_align, m_freem, m_gethdr, m_prepend, m_pullup, ml_dequeue, ml_init,
    mq_delist, mq_enqueue, mq_init, mq_purge,
};
use crate::machine::copy::{AbiPod, copyin_obj, copyout_obj};
use crate::machine::cpu::curproc;
use crate::machine::intr::{IPL_MPFLOOR, IPL_SOFTNET};
use crate::net::bpf::{BPF_DIRECTION_OUT, DLT_PFSYNC, bpf_mtap, bpfattach};
use crate::net::if_::{
    IFF_DEBUG, IFF_MULTICAST, IFF_RUNNING, IFF_UP, IFNAMSIZ, IFXF_CLONED, IFXF_MPSAFE, IfParent,
    Ifreq, counters_inc, counters_pkt, if_alloc_sadl, if_attach, if_clone_attach,
    if_counters_alloc, if_detach, if_detachhook_add, if_detachhook_del, if_down, if_get,
    if_linkstatehook_add, if_linkstatehook_del, if_put, if_unit, link_state_is_up, net_tq,
    net_tq_barriers, unhandled_af,
};
use crate::net::if_types::IFT_PFSYNC;
use crate::net::if_var::{IfClone, IfCounters, Ifnet, Netstack};
use crate::net::ifq::{Ifqueue, ifq_purge};
use crate::net::pf::{
    PF_POOL_LIMITS, PF_STATE_LIST, PF_STATUS, pf_find_state_byid, pf_remove_state, pf_route,
    pf_setup_pdesc, pf_state_export, pf_state_import, pf_state_peer_hton, pf_state_peer_ntoh,
    pf_state_ref, pf_state_unref,
};
use crate::net::pf_if::pfi_kif_find;
use crate::net::pf_norm::{pf_state_scrub_get, pf_state_scrub_put};
use crate::net::pfvar::{
    PF_LIMIT_STATES, PF_OUT, PF_PASS, PF_ROUTETO, PF_SK_WIRE, PF_TCPS_PROXY_DST, PF_TCPS_PROXY_SRC,
    PFSTATE_ACK, PFSTATE_NOSYNC, PFTM_MAX, PFTM_UNLINKED, PfPoolItem, PfStateCmp, PfiKif,
    PfsyncState, PfsyncStatePeer, pf_pool_get, pf_pool_put,
};
use crate::net::pfvar_priv::{
    PfPdesc, PfState, PfStateKey, PfStateQueue, PfStateSyncQueue, pf_lock, pf_state_enter_read,
    pf_state_enter_write, pf_state_exit_read, pf_state_exit_write, pf_unlock,
};
use crate::net::route::Rtentry;
use crate::netinet::if_ether::ETHERMTU;
use crate::netinet::in_::{
    INADDR_ANY, INADDR_BROADCAST, INADDR_PFSYNC_GROUP, IPPROTO_DONE, IPPROTO_PFSYNC, IPPROTO_TCP,
    InAddr, in_addmulti, in_delmulti, in_multicast,
};
use crate::netinet::in_var::InMulti;
use crate::netinet::ip::{IP_DF, IPTOS_LOWDELAY, IPVERSION, Ip};
use crate::netinet::ip_id::ip_randomid;
use crate::netinet::ip_ipsp::{
    SPI_RESERVED_MAX, SockaddrUnion, TDBF_PFSYNC, TDBF_PFSYNC_RPL, TDBF_PFSYNC_SNAPPED, Tdb,
    gettdb, tdb_unref,
};
use crate::netinet::ip_output::ip_output;
use crate::netinet::ip_var::{IP_RAWOUTPUT, IpMoptions};
use crate::netinet::tcp_seq::seq_gt;
use crate::sys::endian::{betoh64, htobe64, htonl, htons, ntohl, ntohs};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_CANFAIL, M_DEVBUF, M_WAITOK, M_ZERO};
use crate::sys::mbuf::{
    M_BCAST, M_DONTWAIT, M_EXT, M_MCAST, MHLEN, MT_DATA, Mbuf, MbufList, MbufQueue,
    PF_TAG_GENERATED, mclgetl, ml_empty, mtod,
};
use crate::sys::mutex::{Mutex, mutex_assert_locked, mutex_assert_unlocked};
use crate::sys::param::PWAIT;
use crate::sys::pool::{PR_NOWAIT, Pool};
use crate::sys::queue::{TailqEntry, TailqHead};
use crate::sys::rwlock::Rwlock;
use crate::sys::smr::{SmrEntry, smr_call, smr_init};
use crate::sys::socket::{AF_INET, AF_INET6, Sockaddr};
use crate::sys::sockio::{
    SIOCDIFPARENT, SIOCGETPFSYNC, SIOCGIFPARENT, SIOCSETPFSYNC, SIOCSIFADDR, SIOCSIFFLAGS,
    SIOCSIFMTU, SIOCSIFPARENT,
};
use crate::sys::syslog::LOG_WARNING;
use crate::sys::systm::{INFSLP, net_assert_locked, net_lock, net_unlock};
use crate::sys::task::{Task, Taskq};
use crate::sys::timeout::{Timeout, timeout_pending};

/// `PFSYNC_VERSION`.
pub const PFSYNC_VERSION: u8 = 6;
/// `PFSYNC_DFLTTL`.
pub const PFSYNC_DFLTTL: u8 = 255;

/// `PFSYNC_ACT_CLR`: clear all states.
pub const PFSYNC_ACT_CLR: u8 = 0;
/// `PFSYNC_ACT_OINS`: old insert state.
pub const PFSYNC_ACT_OINS: u8 = 1;
/// `PFSYNC_ACT_INS_ACK`: ack of inserted state.
pub const PFSYNC_ACT_INS_ACK: u8 = 2;
/// `PFSYNC_ACT_OUPD`: old update state.
pub const PFSYNC_ACT_OUPD: u8 = 3;
/// `PFSYNC_ACT_UPD_C`: "compressed" update state.
pub const PFSYNC_ACT_UPD_C: u8 = 4;
/// `PFSYNC_ACT_UPD_REQ`: request "uncompressed" state.
pub const PFSYNC_ACT_UPD_REQ: u8 = 5;
/// `PFSYNC_ACT_DEL`: delete state.
pub const PFSYNC_ACT_DEL: u8 = 6;
/// `PFSYNC_ACT_DEL_C`: "compressed" delete state.
pub const PFSYNC_ACT_DEL_C: u8 = 7;
/// `PFSYNC_ACT_INS_F`: insert fragment.
pub const PFSYNC_ACT_INS_F: u8 = 8;
/// `PFSYNC_ACT_DEL_F`: delete fragments.
pub const PFSYNC_ACT_DEL_F: u8 = 9;
/// `PFSYNC_ACT_BUS`: bulk update status.
pub const PFSYNC_ACT_BUS: u8 = 10;
/// `PFSYNC_ACT_OTDB`: old TDB replay counter update.
pub const PFSYNC_ACT_OTDB: u8 = 11;
/// `PFSYNC_ACT_EOF`: end of frame - DEPRECATED.
pub const PFSYNC_ACT_EOF: u8 = 12;
/// `PFSYNC_ACT_INS`: insert state.
pub const PFSYNC_ACT_INS: u8 = 13;
/// `PFSYNC_ACT_UPD`: update state.
pub const PFSYNC_ACT_UPD: u8 = 14;
/// `PFSYNC_ACT_TDB`: TDB replay counter update.
pub const PFSYNC_ACT_TDB: u8 = 15;
/// `PFSYNC_ACT_MAX`.
pub const PFSYNC_ACT_MAX: u8 = 16;

/// `PFSYNC_ACTIONS`: the actions' names, by number.
pub const PFSYNC_ACTIONS: [&str; PFSYNC_ACT_MAX as usize] = [
    "CLR ST",
    "INS ST OLD",
    "INS ST ACK",
    "UPD ST OLD",
    "UPD ST COMP",
    "UPD ST REQ",
    "DEL ST",
    "DEL ST COMP",
    "INS FR",
    "DEL FR",
    "BULK UPD STAT",
    "UPD TDB OLD",
    "EOF",
    "INS ST",
    "UPD ST",
    "UPD TDB",
];

// A pfsync frame is built from a header followed by several sections which are all prefixed
// with their own subheaders:
//
// | ...                    |
// | IP header              |
// +========================+
// | pfsync_header          |
// +------------------------+
// | pfsync_subheader       |
// +------------------------+
// | first action fields    |
// | ...                    |
// +------------------------+
// | pfsync_subheader       |
// +------------------------+
// | second action fields   |
// | ...                    |
// +========================+

/// `struct pfsync_header` (`__packed`): the frame header.
#[repr(C, packed)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct PfsyncHeader {
    /// `version`.
    pub version: u8,
    /// `_pad`.
    pub _pad: u8,
    /// `len`: in bytes, network order.
    pub len: u16,
    /// `spare[16]`.
    pub spare: [u8; 16],
}

/// `struct pfsync_subheader` (`__packed`): the frame region subheader.
#[repr(C, packed)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct PfsyncSubheader {
    /// `action`.
    pub action: u8,
    /// `len`: in dwords.
    pub len: u8,
    /// `count`: network order.
    pub count: u16,
}

/// `struct pfsync_clr` (`__packed`): CLR.
#[repr(C, packed)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct PfsyncClr {
    /// `ifname`.
    pub ifname: [u8; IFNAMSIZ],
    /// `creatorid`.
    pub creatorid: u32,
}

// OINS, OUPD: these messages are deprecated.
// INS, UPD, DEL: these use struct pfsync_state in pfvar.h.

/// `struct pfsync_ins_ack` (`__packed`): INS_ACK.
#[repr(C, packed)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct PfsyncInsAck {
    /// `id`.
    pub id: u64,
    /// `creatorid`.
    pub creatorid: u32,
}

/// `struct pfsync_upd_c` (`__packed`): UPD_C.
#[repr(C, packed)]
#[derive(Clone, Copy, Default)]
pub struct PfsyncUpdC {
    /// `id`.
    pub id: u64,
    /// `src`.
    pub src: PfsyncStatePeer,
    /// `dst`.
    pub dst: PfsyncStatePeer,
    /// `creatorid`.
    pub creatorid: u32,
    /// `expire`.
    pub expire: u32,
    /// `timeout`.
    pub timeout: u8,
    /// `state_flags`.
    pub state_flags: u8,
    /// `_pad[2]`.
    pub _pad: [u8; 2],
}

/// `struct pfsync_upd_req` (`__packed __aligned(4)`): UPD_REQ. Packed here (alignment 1, the
/// same 12 bytes): it is only ever copied in and out of mbuf data.
#[repr(C, packed)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct PfsyncUpdReq {
    /// `id`.
    pub id: u64,
    /// `creatorid`.
    pub creatorid: u32,
}

/// `struct pfsync_del_c` (`__packed`): DEL_C.
#[repr(C, packed)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct PfsyncDelC {
    /// `id`.
    pub id: u64,
    /// `creatorid`.
    pub creatorid: u32,
}

// INS_F, DEL_F: not implemented (yet).

/// `struct pfsync_bus` (`__packed`): BUS.
#[repr(C, packed)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct PfsyncBus {
    /// `creatorid`.
    pub creatorid: u32,
    /// `endtime`: network order.
    pub endtime: u32,
    /// `status`: `PFSYNC_BUS_START` or `PFSYNC_BUS_END`.
    pub status: u8,
    /// `_pad[3]`.
    pub _pad: [u8; 3],
}

/// `PFSYNC_BUS_START`.
pub const PFSYNC_BUS_START: u8 = 1;
/// `PFSYNC_BUS_END`.
pub const PFSYNC_BUS_END: u8 = 2;

/// `struct pfsync_tdb` (`__packed`): TDB. `dst` is the bytes of a `union sockaddr_union`
/// (whose 4-byte alignment a packed structure cannot hold).
#[repr(C, packed)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct PfsyncTdb {
    /// `spi`.
    pub spi: u32,
    /// `dst`.
    pub dst: [u8; size_of::<SockaddrUnion>()],
    /// `rpl`: network order.
    pub rpl: u64,
    /// `cur_bytes`: network order.
    pub cur_bytes: u64,
    /// `sproto`.
    pub sproto: u8,
    /// `updates`.
    pub updates: u8,
    /// `rdomain`: network order.
    pub rdomain: u16,
}

// EOF: this message is deprecated.

// SAFETY: packed integers and arrays of integers: no padding, any bit pattern valid.
unsafe impl AbiPod for PfsyncHeader {}
// SAFETY: as above.
unsafe impl AbiPod for PfsyncSubheader {}
// SAFETY: as above.
unsafe impl AbiPod for PfsyncClr {}
// SAFETY: as above.
unsafe impl AbiPod for PfsyncInsAck {}
// SAFETY: as above; the peers are packed integers too.
unsafe impl AbiPod for PfsyncUpdC {}
// SAFETY: as above.
unsafe impl AbiPod for PfsyncUpdReq {}
// SAFETY: as above.
unsafe impl AbiPod for PfsyncDelC {}
// SAFETY: as above.
unsafe impl AbiPod for PfsyncBus {}
// SAFETY: as above.
unsafe impl AbiPod for PfsyncTdb {}

/// `struct pfsync_subh_clr` (`__packed __aligned(4)`): the subheader and the CLR message.
#[repr(C, packed)]
#[derive(Clone, Copy, Default)]
struct PfsyncSubhClr {
    subh: PfsyncSubheader,
    clr: PfsyncClr,
}

// SAFETY: packed wire structures: no padding, any bit pattern valid.
unsafe impl AbiPod for PfsyncSubhClr {}

/// `struct pfsync_subh_bus` (`__packed __aligned(4)`): the subheader and the BUS message.
#[repr(C, packed)]
#[derive(Clone, Copy, Default)]
struct PfsyncSubhBus {
    subh: PfsyncSubheader,
    bus: PfsyncBus,
}

// SAFETY: packed wire structures: no padding, any bit pattern valid.
unsafe impl AbiPod for PfsyncSubhBus {}

/// `PFSYNC_HDRLEN`.
pub const PFSYNC_HDRLEN: usize = size_of::<PfsyncHeader>();

/// `PFSYNCCTL_STATS`: PFSYNC stats.
pub const PFSYNCCTL_STATS: i32 = 1;
/// `PFSYNCCTL_MAXID`.
pub const PFSYNCCTL_MAXID: i32 = 2;

/// `struct pfsyncstats`: `net.inet.pfsync.stats`.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Pfsyncstats {
    /// `pfsyncs_ipackets`: total input packets, IPv4.
    pub pfsyncs_ipackets: u64,
    /// `pfsyncs_ipackets6`: total input packets, IPv6.
    pub pfsyncs_ipackets6: u64,
    /// `pfsyncs_badif`: not the right interface.
    pub pfsyncs_badif: u64,
    /// `pfsyncs_badttl`: TTL is not `PFSYNC_DFLTTL`.
    pub pfsyncs_badttl: u64,
    /// `pfsyncs_hdrops`: packets shorter than hdr.
    pub pfsyncs_hdrops: u64,
    /// `pfsyncs_badver`: bad (incl unsupp) version.
    pub pfsyncs_badver: u64,
    /// `pfsyncs_badact`: bad action.
    pub pfsyncs_badact: u64,
    /// `pfsyncs_badlen`: data length does not match.
    pub pfsyncs_badlen: u64,
    /// `pfsyncs_badauth`: bad authentication.
    pub pfsyncs_badauth: u64,
    /// `pfsyncs_stale`: stale state.
    pub pfsyncs_stale: u64,
    /// `pfsyncs_badval`: bad values.
    pub pfsyncs_badval: u64,
    /// `pfsyncs_badstate`: insert/lookup failed.
    pub pfsyncs_badstate: u64,
    /// `pfsyncs_opackets`: total output packets, IPv4.
    pub pfsyncs_opackets: u64,
    /// `pfsyncs_opackets6`: total output packets, IPv6.
    pub pfsyncs_opackets6: u64,
    /// `pfsyncs_onomem`: no memory for an mbuf.
    pub pfsyncs_onomem: u64,
    /// `pfsyncs_oerrors`: ip output error.
    pub pfsyncs_oerrors: u64,
}

/// `struct pfsyncreq`: the configuration `SIOCSETPFSYNC` and `SIOCGETPFSYNC` carry.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Pfsyncreq {
    /// `pfsyncr_syncdev`.
    pub pfsyncr_syncdev: [u8; IFNAMSIZ],
    /// `pfsyncr_syncpeer`.
    pub pfsyncr_syncpeer: InAddr,
    /// `pfsyncr_maxupdates`.
    pub pfsyncr_maxupdates: i32,
    /// `pfsyncr_defer`.
    pub pfsyncr_defer: i32,
}

// SAFETY: `#[repr(C)]` integers and a byte array, 28 bytes without padding (checked below).
unsafe impl AbiPod for Pfsyncreq {}

/// `enum pfsync_counters`: the indices of `pfsynccounters`.
#[repr(usize)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PfsyncCounters {
    /// `pfsyncs_ipackets`.
    PfsyncsIpackets,
    /// `pfsyncs_ipackets6`.
    PfsyncsIpackets6,
    /// `pfsyncs_badif`.
    PfsyncsBadif,
    /// `pfsyncs_badttl`.
    PfsyncsBadttl,
    /// `pfsyncs_hdrops`.
    PfsyncsHdrops,
    /// `pfsyncs_badver`.
    PfsyncsBadver,
    /// `pfsyncs_badact`.
    PfsyncsBadact,
    /// `pfsyncs_badlen`.
    PfsyncsBadlen,
    /// `pfsyncs_badauth`.
    PfsyncsBadauth,
    /// `pfsyncs_stale`.
    PfsyncsStale,
    /// `pfsyncs_badval`.
    PfsyncsBadval,
    /// `pfsyncs_badstate`.
    PfsyncsBadstate,
    /// `pfsyncs_opackets`.
    PfsyncsOpackets,
    /// `pfsyncs_opackets6`.
    PfsyncsOpackets6,
    /// `pfsyncs_onomem`.
    PfsyncsOnomem,
    /// `pfsyncs_oerrors`.
    PfsyncsOerrors,
    /// `pfsyncs_ncounters`.
    PfsyncsNcounters,
}

/// `pfsyncs_ncounters`.
pub const PFSYNCS_NCOUNTERS: usize = PfsyncCounters::PfsyncsNcounters as usize;

// This shows where a pf state is with respect to the syncing.
/// `PFSYNC_S_IACK`.
pub const PFSYNC_S_IACK: u8 = 0x00;
/// `PFSYNC_S_UPD_C`.
pub const PFSYNC_S_UPD_C: u8 = 0x01;
/// `PFSYNC_S_DEL`.
pub const PFSYNC_S_DEL: u8 = 0x02;
/// `PFSYNC_S_INS`.
pub const PFSYNC_S_INS: u8 = 0x03;
/// `PFSYNC_S_UPD`.
pub const PFSYNC_S_UPD: u8 = 0x04;
/// `PFSYNC_S_COUNT`.
pub const PFSYNC_S_COUNT: usize = 0x05;

/// `PFSYNC_S_NONE`.
pub const PFSYNC_S_NONE: u8 = 0xd0;
/// `PFSYNC_S_SYNC`.
pub const PFSYNC_S_SYNC: u8 = 0xd1;
/// `PFSYNC_S_PFSYNC`.
pub const PFSYNC_S_PFSYNC: u8 = 0xd2;
/// `PFSYNC_S_DEAD`.
pub const PFSYNC_S_DEAD: u8 = 0xde;

/// `PFSYNC_SI_IOCTL`.
pub const PFSYNC_SI_IOCTL: i32 = 0x01;
/// `PFSYNC_SI_CKSUM`.
pub const PFSYNC_SI_CKSUM: i32 = 0x02;
/// `PFSYNC_SI_ACK`.
pub const PFSYNC_SI_ACK: i32 = 0x04;
/// `PFSYNC_SI_PFSYNC`.
pub const PFSYNC_SI_PFSYNC: i32 = 0x08;

/// `PFSYNC_MINPKT`: an IP header and a pfsync header.
const PFSYNC_MINPKT: usize = size_of::<Ip>() + size_of::<PfsyncHeader>();

/// `struct pfsync_deferral`: a packet held back until the peer acknowledges its new state.
pub struct PfsyncDeferral {
    /// `pd_entry`.
    pub pd_entry: TailqEntry<PfsyncDeferral>,
    /// `pd_st`: the state, with a reference.
    pub pd_st: Cell<Option<&'static PfState>>,
    /// `pd_m`: the packet.
    pub pd_m: Cell<Option<&'static Mbuf>>,
    /// `pd_deadline`: when it goes out anyway (`getnsecuptime`).
    pub pd_deadline: Cell<u64>,
}

// SAFETY: links, `Option` references, a raw-pointer-free `u64` in `Cell`s: all-zero is a
// valid deferral (nothing linked, no state, no packet).
unsafe impl PfPoolItem for PfsyncDeferral {}

// SAFETY: changed under the slice's `s_mtx`, as in C.
unsafe impl Sync for PfsyncDeferral {}

crate::queue_adapter!(
    /// `TAILQ_HEAD(pfsync_deferrals, pfsync_deferral)`.
    pub PfsyncDeferrals: PfsyncDeferral, pd_entry => TailqEntry<PfsyncDeferral>
);

crate::queue_adapter!(
    /// The slices' `TAILQ_HEAD(, tdb)` through `tdb_sync_entry`.
    pub TdbSyncQueue: Tdb, tdb_sync_entry => TailqEntry<Tdb>
);

/// `PFSYNC_DEFER_NSEC`.
const PFSYNC_DEFER_NSEC: u64 = 20_000_000;
/// `PFSYNC_DEFER_LIMIT`.
const PFSYNC_DEFER_LIMIT: u32 = 128;
/// `PFSYNC_BULK_SND_IVAL_MS`.
const PFSYNC_BULK_SND_IVAL_MS: u64 = 20;

/// `enum pfsync_bulk_req_state`.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PfsyncBulkReqState {
    /// `PFSYNC_BREQ_S_NONE`.
    None,
    /// `PFSYNC_BREQ_S_START`.
    Start,
    /// `PFSYNC_BREQ_S_SENT`.
    Sent,
    /// `PFSYNC_BREQ_S_BULK`.
    Bulk,
    /// `PFSYNC_BREQ_S_DONE`.
    Done,
}

/// `pfsync_bulk_req_state_names[]`.
const PFSYNC_BULK_REQ_STATE_NAMES: [&str; 5] = ["none", "start", "sent", "bulk", "done"];

/// `enum pfsync_bulk_req_event`.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PfsyncBulkReqEvent {
    /// `PFSYNC_BREQ_EVT_UP`.
    Up,
    /// `PFSYNC_BREQ_EVT_DOWN`.
    Down,
    /// `PFSYNC_BREQ_EVT_TMO`.
    Tmo,
    /// `PFSYNC_BREQ_EVT_LINK`.
    Link,
    /// `PFSYNC_BREQ_EVT_BUS_START`.
    BusStart,
    /// `PFSYNC_BREQ_EVT_BUS_END`.
    BusEnd,
}

/// `pfsync_bulk_req_event_names[]`.
const PFSYNC_BULK_REQ_EVENT_NAMES: [&str; 6] =
    ["up", "down", "timeout", "link", "bus-start", "bus-end"];

/// `struct pfsync_slice`: one queue of pending state changes and its sender.
pub struct PfsyncSlice {
    /// `s_pfsync`: the softc.
    pub s_pfsync: Cell<Option<&'static PfsyncSoftc>>,
    /// `s_mtx`: protects the rest.
    pub s_mtx: Mutex,

    /// `s_qs[PFSYNC_S_COUNT]`: the states to send, per `PFSYNC_S_*` message.
    pub s_qs: [TailqHead<PfStateSyncQueue>; PFSYNC_S_COUNT],
    /// `s_tdb_q`: the TDBs to send.
    pub s_tdb_q: TailqHead<TdbSyncQueue>,
    /// `s_len`: the length of the packet the queues make.
    pub s_len: Cell<usize>,
    /// `s_ml`.
    pub s_ml: MbufList,

    /// `s_softnet`.
    pub s_softnet: Cell<Option<&'static Taskq>>,
    /// `s_task`: writes and sends the queues.
    pub s_task: Task,
    /// `s_tmo`.
    pub s_tmo: Timeout,

    /// `s_sendq`: packets written when the queues filled up.
    pub s_sendq: MbufQueue,
    /// `s_send`: sends them.
    pub s_send: Task,

    /// `s_deferrals`.
    pub s_deferrals: TailqHead<PfsyncDeferrals>,
    /// `s_deferred`: how many.
    pub s_deferred: Cell<u32>,
    /// `s_deferrals_task`.
    pub s_deferrals_task: Task,
    /// `s_deferrals_tmo`.
    pub s_deferrals_tmo: Timeout,

    /// `s_stat_locks`.
    pub s_stat_locks: Cell<u64>,
    /// `s_stat_contended`.
    pub s_stat_contended: Cell<u64>,
    /// `s_stat_write_nop`.
    pub s_stat_write_nop: Cell<u64>,
    /// `s_stat_task_add`.
    pub s_stat_task_add: Cell<u64>,
    /// `s_stat_task_run`.
    pub s_stat_task_run: Cell<u64>,
    /// `s_stat_enqueue`.
    pub s_stat_enqueue: Cell<u64>,
    /// `s_stat_dequeue`.
    pub s_stat_dequeue: Cell<u64>,

    /// `s_stat_defer_add`.
    pub s_stat_defer_add: Cell<u64>,
    /// `s_stat_defer_ack`.
    pub s_stat_defer_ack: Cell<u64>,
    /// `s_stat_defer_run`.
    pub s_stat_defer_run: Cell<u64>,
    /// `s_stat_defer_overlimit`.
    pub s_stat_defer_overlimit: Cell<u64>,
    // NKSTAT > 0: s_kstat; not configured.
}

/// `PFSYNC_SLICE_BITS`.
const PFSYNC_SLICE_BITS: u32 = 1;
/// `PFSYNC_NSLICES`.
const PFSYNC_NSLICES: usize = 1 << PFSYNC_SLICE_BITS;

/// `sc_bulk_req`: the bulk update this side asks for.
pub struct PfsyncBulkReq {
    /// `req_lock`.
    pub req_lock: Rwlock,
    /// `req_tmo`.
    pub req_tmo: Timeout,
    /// `req_state`.
    pub req_state: Cell<PfsyncBulkReqState>,
    /// `req_tries`.
    pub req_tries: Cell<u32>,
    /// `req_demoted`.
    pub req_demoted: Cell<u32>,
}

/// `sc_bulk_snd`: the bulk update this side sends.
pub struct PfsyncBulkSnd {
    /// `snd_lock`.
    pub snd_lock: Rwlock,
    /// `snd_tmo`.
    pub snd_tmo: Timeout,
    /// `snd_requested`.
    pub snd_requested: Cell<i64>,

    /// `snd_next`.
    pub snd_next: Cell<Option<&'static PfState>>,
    /// `snd_tail`.
    pub snd_tail: Cell<Option<&'static PfState>>,
    /// `snd_again`.
    pub snd_again: Cell<u32>,
}

/// `struct pfsync_softc`. The interface comes first: `if_softc` and the softc are the same
/// address.
#[repr(C)]
pub struct PfsyncSoftc {
    /// `sc_if`.
    pub sc_if: Ifnet,
    /// `sc_dead`.
    pub sc_dead: Cell<u32>,
    /// `sc_up`.
    pub sc_up: Cell<u32>,
    /// `sc_refs`: "owned" by `IFF_RUNNING`.
    pub sc_refs: crate::sys::refcnt::Refcnt,

    // config
    /// `sc_syncpeer`.
    pub sc_syncpeer: Cell<InAddr>,
    /// `sc_maxupdates`.
    pub sc_maxupdates: Cell<u32>,
    /// `sc_defer`.
    pub sc_defer: Cell<u32>,

    // operation
    /// `sc_sync_ifidx`.
    pub sc_sync_ifidx: Cell<u32>,
    /// `sc_sync_if_down`.
    pub sc_sync_if_down: Cell<u32>,
    /// `sc_inm`: the sync peer group's membership.
    pub sc_inm: Cell<Option<&'static InMulti>>,
    /// `sc_ltask`.
    pub sc_ltask: Task,
    /// `sc_dtask`.
    pub sc_dtask: Task,
    /// `sc_template`: the IP header of every frame.
    pub sc_template: Cell<Ip>,

    /// `sc_slices[PFSYNC_NSLICES]`.
    pub sc_slices: [PfsyncSlice; PFSYNC_NSLICES],

    /// `sc_bulk_req`.
    pub sc_bulk_req: PfsyncBulkReq,

    /// `sc_bulk_snd`.
    pub sc_bulk_snd: PfsyncBulkSnd,
}

// SAFETY: the members change under the locks and in the contexts the C names (`NET_LOCK`, the
// slices' mutexes, the bulk locks); the interface as `Ifnet` documents.
unsafe impl Sync for PfsyncSoftc {}

impl PfsyncSoftc {
    /// The softc behind a `void *` argument (`timeout(9)`, `task_add(9)`).
    fn of_arg(arg: *mut c_void) -> &'static Self {
        // SAFETY: every timeout and task that names the softc as its argument is set up by
        // `pfsync_clone_create` and stopped before `pfsync_clone_destroy` frees it.
        unsafe { &*arg.cast::<Self>().cast_const() }
    }

    /// The softc of a `pfsync` interface.
    fn of_ifp(ifp: &Ifnet) -> &'static Self {
        // SAFETY: `if_softc` of a `pfsync` interface is its softc (`pfsync_clone_create`),
        // which lives until `pfsync_clone_destroy`.
        unsafe { &*ifp.if_softc.get().cast::<Self>().cast_const() }
    }

    /// The softc as a timeout or task argument.
    fn arg(&self) -> *mut c_void {
        ptr::from_ref(self).cast_mut().cast()
    }
}

impl PfsyncSlice {
    /// The slice behind a `void *` argument.
    fn of_arg(arg: *mut c_void) -> &'static Self {
        // SAFETY: the slice's timeouts and tasks are set up with the slice, part of a softc
        // that `pfsync_clone_create` made, and stopped before the softc is freed.
        unsafe { &*arg.cast::<Self>().cast_const() }
    }

    /// The slice as a timeout or task argument.
    fn arg(&self) -> *mut c_void {
        ptr::from_ref(self).cast_mut().cast()
    }

    /// `s->s_pfsync`.
    fn sc(&self) -> &'static PfsyncSoftc {
        match self.s_pfsync.get() {
            Some(sc) => sc,
            None => panic(format_args!("pfsync: slice without a softc")),
        }
    }

    /// `s->s_softnet`.
    fn softnet(&self) -> &'static Taskq {
        match self.s_softnet.get() {
            Some(tq) => tq,
            None => panic(format_args!("pfsync: slice without a softnet queue")),
        }
    }
}

/// `pfsync_deferrals_pool`.
pub static PFSYNC_DEFERRALS_POOL: Pool = Pool::new();

/// `pfsyncif`: the running pfsync interface (SMR protected in C).
static PFSYNCIF: AtomicPtr<PfsyncSoftc> = AtomicPtr::new(ptr::null_mut());

/// `pfsynccounters`.
pub static PFSYNCCOUNTERS: [AtomicU64; PFSYNCS_NCOUNTERS] =
    [const { AtomicU64::new(0) }; PFSYNCS_NCOUNTERS];

/// `PFSYNC_MAX_BULKTRIES`.
const PFSYNC_MAX_BULKTRIES: u32 = 12;

/// `pfsync_cloner`.
pub static PFSYNC_CLONER: IfClone =
    IfClone::new(b"pfsync", pfsync_clone_create, Some(pfsync_clone_destroy));

/// `struct pfsync_q`: how a `PFSYNC_S_*` queue goes out.
struct PfsyncQ {
    /// `write`: the message of a state.
    write: fn(&'static PfState, &mut [u8]),
    /// `len`: its size.
    len: usize,
    /// `action`.
    action: u8,
}

/// `pfsync_qs[]`: we have one of these for every `PFSYNC_S_`.
static PFSYNC_QS: [PfsyncQ; PFSYNC_S_COUNT] = [
    PfsyncQ {
        write: pfsync_out_iack,
        len: size_of::<PfsyncInsAck>(),
        action: PFSYNC_ACT_INS_ACK,
    },
    PfsyncQ {
        write: pfsync_out_upd_c,
        len: size_of::<PfsyncUpdC>(),
        action: PFSYNC_ACT_UPD_C,
    },
    PfsyncQ {
        write: pfsync_out_del,
        len: size_of::<PfsyncDelC>(),
        action: PFSYNC_ACT_DEL_C,
    },
    PfsyncQ {
        write: pfsync_out_state,
        len: size_of::<PfsyncState>(),
        action: PFSYNC_ACT_INS,
    },
    PfsyncQ {
        write: pfsync_out_state,
        len: size_of::<PfsyncState>(),
        action: PFSYNC_ACT_UPD,
    },
];

/// The reader of a message type: the softc, the messages, the size of one, their count.
type PfsyncInFn = fn(&'static PfsyncSoftc, &[u8], usize, usize);

/// `struct pfsync_act`: how a message type comes in.
struct PfsyncAct {
    /// `in`: NULL for the types nothing reads.
    in_: Option<PfsyncInFn>,
    /// `len`: the size of one message.
    len: usize,
}

/// `pfsync_acts[]`, by `PFSYNC_ACT_*`.
static PFSYNC_ACTS: [PfsyncAct; PFSYNC_ACT_MAX as usize] = [
    // PFSYNC_ACT_CLR
    PfsyncAct {
        in_: Some(pfsync_in_clr),
        len: size_of::<PfsyncClr>(),
    },
    // PFSYNC_ACT_OINS
    PfsyncAct { in_: None, len: 0 },
    // PFSYNC_ACT_INS_ACK
    PfsyncAct {
        in_: Some(pfsync_in_iack),
        len: size_of::<PfsyncInsAck>(),
    },
    // PFSYNC_ACT_OUPD
    PfsyncAct { in_: None, len: 0 },
    // PFSYNC_ACT_UPD_C
    PfsyncAct {
        in_: Some(pfsync_in_upd_c),
        len: size_of::<PfsyncUpdC>(),
    },
    // PFSYNC_ACT_UPD_REQ
    PfsyncAct {
        in_: Some(pfsync_in_ureq),
        len: size_of::<PfsyncUpdReq>(),
    },
    // PFSYNC_ACT_DEL
    PfsyncAct {
        in_: Some(pfsync_in_del),
        len: size_of::<PfsyncState>(),
    },
    // PFSYNC_ACT_DEL_C
    PfsyncAct {
        in_: Some(pfsync_in_del_c),
        len: size_of::<PfsyncDelC>(),
    },
    // PFSYNC_ACT_INS_F
    PfsyncAct { in_: None, len: 0 },
    // PFSYNC_ACT_DEL_F
    PfsyncAct { in_: None, len: 0 },
    // PFSYNC_ACT_BUS
    PfsyncAct {
        in_: Some(pfsync_in_bus),
        len: size_of::<PfsyncBus>(),
    },
    // PFSYNC_ACT_OTDB
    PfsyncAct { in_: None, len: 0 },
    // PFSYNC_ACT_EOF
    PfsyncAct { in_: None, len: 0 },
    // PFSYNC_ACT_INS
    PfsyncAct {
        in_: Some(pfsync_in_ins),
        len: size_of::<PfsyncState>(),
    },
    // PFSYNC_ACT_UPD
    PfsyncAct {
        in_: Some(pfsync_in_upd),
        len: size_of::<PfsyncState>(),
    },
    // PFSYNC_ACT_TDB
    PfsyncAct {
        in_: Some(pfsync_in_tdb),
        len: size_of::<PfsyncTdb>(),
    },
];

/// The bytes of a wire structure.
fn wire_bytes<T: AbiPod>(v: &T) -> &[u8] {
    // SAFETY: `T: AbiPod` has no padding, so its `size_of::<T>()` bytes are initialised and
    // may be read as `u8`s while `v` is borrowed.
    unsafe { slice::from_raw_parts(ptr::from_ref(v).cast::<u8>(), size_of::<T>()) }
}

/// Writes the wire structure `v` at the start of `buf`.
fn wire_put<T: AbiPod>(buf: &mut [u8], v: &T) {
    buf[..size_of::<T>()].copy_from_slice(wire_bytes(v));
}

/// Reads a wire structure from the start of `buf`.
fn wire_get<T: AbiPod>(buf: &[u8]) -> T {
    let b = &buf[..size_of::<T>()];
    // SAFETY: `b` holds `size_of::<T>()` bytes and every bit pattern is a valid `T`
    // (`AbiPod`); the read does not need alignment.
    unsafe { ptr::read_unaligned(b.as_ptr().cast::<T>()) }
}

/// The `len` bytes of `m`'s data from `off`, which the caller made contiguous (`m_pullup`).
///
/// # Safety
///
/// `off + len` bytes from `mtod(m)` are inside `m`'s data area, and nothing writes them while
/// the slice lives.
unsafe fn mbuf_bytes(m: &Mbuf, off: usize, len: usize) -> &[u8] {
    // SAFETY: the caller's contract.
    unsafe { slice::from_raw_parts(mtod::<u8>(m).add(off), len) }
}

/// The bytes of an IP header.
fn ip_bytes(ip: &Ip) -> [u8; size_of::<Ip>()] {
    let mut b = [0u8; size_of::<Ip>()];
    // SAFETY: `b` holds `size_of::<Ip>()` bytes; the unaligned write copies the header's
    // bytes (`Ip` is `#[repr(C)]` without padding: 20 bytes).
    unsafe { ptr::write_unaligned(b.as_mut_ptr().cast::<Ip>(), *ip) };
    b
}

/// The first `len` bytes of an mbuf's data, which a frame writer fills in (the C writes
/// through pointers into `mtod(m, caddr_t)`).
struct MbufWriter<'a> {
    m: &'a Mbuf,
    len: usize,
}

impl<'a> MbufWriter<'a> {
    /// A writer of the `len` bytes at `mtod(m)`.
    ///
    /// # Safety
    ///
    /// `len` bytes from `mtod(m)` are inside `m`'s data area (`m_align`, `m_prepend` made
    /// them), and nothing else reads or writes them while the writer lives.
    unsafe fn new(m: &'a Mbuf, len: usize) -> Self {
        Self { m, len }
    }

    /// Copies `bytes` to offset `off`.
    fn copyin(&self, off: usize, bytes: &[u8]) {
        if off
            .checked_add(bytes.len())
            .is_none_or(|end| end > self.len)
        {
            panic(format_args!(
                "pfsync: frame write of {} at {} past {}",
                bytes.len(),
                off,
                self.len
            ));
        }
        // SAFETY: the range was checked against `len`, which `new`'s contract puts inside
        // the data area and gives to this writer alone.
        unsafe {
            ptr::copy_nonoverlapping(bytes.as_ptr(), mtod::<u8>(self.m).add(off), bytes.len());
        }
    }

    /// `memset(ptr, 0, len)`.
    fn fill_zero(&self) {
        for off in (0..self.len).step_by(64) {
            self.copyin(off, &[0u8; 64][..(self.len - off).min(64)]);
        }
    }

    /// Writes the wire structure `v` at offset `off`.
    fn put<T: AbiPod>(&self, off: usize, v: &T) {
        self.copyin(off, wire_bytes(v));
    }

    /// Writes the IP header `ip` at offset `off`.
    fn put_ip(&self, off: usize, ip: &Ip) {
        self.copyin(off, &ip_bytes(ip));
    }
}

/// `pfsyncstat_inc`.
fn pfsyncstat_inc(c: PfsyncCounters) {
    PFSYNCCOUNTERS[c as usize].fetch_add(1, Ordering::Relaxed);
}

/// `pfsync_if`: the running pfsync interface (`SMR_PTR_GET(&pfsyncif)`).
fn pfsync_if() -> Option<&'static PfsyncSoftc> {
    // SAFETY: `pfsync_up` publishes a softc made by `pfsync_clone_create` and `pfsync_down`
    // withdraws it before the softc can be freed.
    unsafe { PFSYNCIF.load(Ordering::Acquire).as_ref() }
}

/// `counters_pkt(ifp->if_counters, ...)`.
fn ifp_counters_pkt(ifp: &Ifnet, pc: IfCounters, bc: IfCounters, v: u64) {
    if let Some(c) = ifp.if_counters.get() {
        counters_pkt(c, pc, bc, v);
    }
}

/// `counters_inc(ifp->if_counters, c)`.
fn ifp_counters_inc(ifp: &Ifnet, c: IfCounters) {
    if let Some(ctrs) = ifp.if_counters.get() {
        counters_inc(ctrs, c);
    }
}

/// `pfsyncattach`: the pseudo-device attach function: registers the cloner.
pub fn pfsyncattach(_npfsync: i32) {
    // pfsynccounters = counters_alloc(pfsyncs_ncounters): a static (see the deviations).

    // SAFETY: `pfsyncattach` runs once, from `main`'s pseudo-device attach.
    unsafe { if_clone_attach(&PFSYNC_CLONER) };
}

/// `pfsync_clone_create`: creates `pfsync0`, the only unit.
pub fn pfsync_clone_create(_ifc: &'static IfClone, unit: i32) -> Result<(), Errno> {
    if unit != 0 {
        return Err(Errno::ENXIO);
    }

    if PFSYNC_DEFERRALS_POOL.pr_size.get() == 0 {
        pool_init(
            &PFSYNC_DEFERRALS_POOL,
            size_of::<PfsyncDeferral>(),
            align_of::<PfsyncDeferral>() as u32,
            IPL_MPFLOOR,
            0,
            "pfdefer",
            None,
        );
        // pool_cache_init(&pfsync_deferrals_pool);
    }

    let Some(mem) = malloc(
        size_of::<PfsyncSoftc>(),
        M_DEVBUF,
        M_WAITOK | M_ZERO | M_CANFAIL,
    ) else {
        return Err(Errno::ENOMEM);
    };
    // SAFETY: a zero-filled block of `sizeof(struct pfsync_softc)` bytes, aligned for it
    // (malloc's chunks are aligned to their size); the all-zero softc is valid (see the
    // module's docs). It lives until `pfsync_clone_destroy`.
    let sc: &'static PfsyncSoftc = unsafe { &*mem.as_ptr().cast::<PfsyncSoftc>() };

    // sc_refs is "owned" by IFF_RUNNING

    sc.sc_syncpeer.set(InAddr {
        s_addr: INADDR_PFSYNC_GROUP,
    });
    sc.sc_maxupdates.set(128);
    sc.sc_defer.set(0);

    task_set(&sc.sc_ltask, pfsync_syncif_link, sc.arg());
    task_set(&sc.sc_dtask, pfsync_syncif_detach, sc.arg());

    rw_init(&sc.sc_bulk_req.req_lock, "pfsyncbreq");
    // need process context to take net lock to call ip_output
    timeout_set_proc(&sc.sc_bulk_req.req_tmo, pfsync_bulk_req_tmo, sc.arg());

    rw_init(&sc.sc_bulk_snd.snd_lock, "pfsyncbsnd");
    // need process context to take net lock to call ip_output
    timeout_set_proc(&sc.sc_bulk_snd.snd_tmo, pfsync_bulk_snd_tmo, sc.arg());

    let ifp = &sc.sc_if;
    let mut xname = [0u8; IFNAMSIZ];
    let _ = snprintf(&mut xname, format_args!("pfsync{unit}"));
    ifp.if_xname.set(xname);
    ifp.if_softc.set(sc.arg());
    ifp.if_ioctl.set(Some(pfsync_ioctl));
    ifp.if_output.set(Some(pfsync_output));
    ifp.if_qstart.set(Some(pfsync_start));
    ifp.if_type.set(IFT_PFSYNC);
    ifp.if_hdrlen.set(size_of::<PfsyncHeader>() as u8);
    ifp.if_mtu.set(ETHERMTU as u32);
    ifp.if_xflags.set(IFXF_CLONED | IFXF_MPSAFE);

    for (i, s) in sc.sc_slices.iter().enumerate() {
        s.s_pfsync.set(Some(sc));

        mtx_init_flags(&s.s_mtx, IPL_SOFTNET, Some("pfslice"), 0);
        s.s_softnet.set(net_tq(i as u32));
        timeout_set(&s.s_tmo, pfsync_slice_tmo, s.arg());
        task_set(&s.s_task, pfsync_slice_task, s.arg());

        mq_init(&s.s_sendq, 16, IPL_SOFTNET);
        task_set(&s.s_send, pfsync_slice_sendq, s.arg());

        s.s_len.set(PFSYNC_MINPKT);
        ml_init(&s.s_ml);

        for q in &s.s_qs {
            q.init();
        }
        s.s_tdb_q.init();

        // stupid NET_LOCK
        timeout_set(&s.s_deferrals_tmo, pfsync_deferrals_tmo, s.arg());
        task_set(&s.s_deferrals_task, pfsync_deferrals_task, s.arg());
        s.s_deferrals.init();

        // NKSTAT > 0: kstat_create(ifp->if_xname, 0, "pfsync-slice", i, KSTAT_T_KV, 0) with
        // pfsync_kstat_copy over the slice's counters; kstat is not configured.
    }

    if_counters_alloc(ifp);
    if_attach(ifp);
    if_alloc_sadl(ifp);

    // NCARP > 0: if_addgroup(ifp, "carp"); carp(4) is not configured.

    bpfattach(&sc.sc_if.if_bpf, ifp, DLT_PFSYNC, PFSYNC_HDRLEN as u32);

    Ok(())
}

/// `pfsync_clone_destroy`.
pub fn pfsync_clone_destroy(ifp: &'static Ifnet) -> Result<(), Errno> {
    let sc = PfsyncSoftc::of_ifp(ifp);

    net_lock();
    sc.sc_dead.set(1);

    if ifp.if_flags.get() & IFF_RUNNING != 0 {
        let _ = pfsync_down(sc);
    }
    net_unlock();

    if_detach(ifp);

    // NKSTAT > 0: kstat_destroy(s->s_kstat) of each slice; not configured.

    free(NonNull::from(sc).cast(), M_DEVBUF, size_of::<PfsyncSoftc>());

    Ok(())
}

/// `pfsync_dprintf`: a message about the interface when it has `IFF_DEBUG`.
fn pfsync_dprintf(sc: &PfsyncSoftc, args: core::fmt::Arguments<'_>) {
    let ifp = &sc.sc_if;

    if ifp.if_flags.get() & IFF_DEBUG == 0 {
        return;
    }

    let xname = ifp.if_xname.get();
    printf(format_args!("{}: {}\n", Str(&xname), args));
}

/// `pfsync_syncif_link`: the sync interface's link state changed (`sc_ltask`).
fn pfsync_syncif_link(arg: *mut c_void) {
    let sc = PfsyncSoftc::of_arg(arg);
    let mut sync_if_down = 1;

    let ifp0 = if_get(sc.sc_sync_ifidx.get());
    if let Some(ifp0) = ifp0
        && link_state_is_up(ifp0.if_link_state.get())
    {
        pfsync_bulk_req_evt(sc, PfsyncBulkReqEvent::Link);
        sync_if_down = 0;
    }
    if_put(ifp0);

    // NCARP > 0: carp_group_demote_adj(&sc->sc_if, sync_if_down ? 1 : -1, "pfsync link")
    // when sc_sync_if_down changes; carp(4) is not configured.

    sc.sc_sync_if_down.set(sync_if_down);
}

/// `pfsync_syncif_detach`: the sync interface is going away (`sc_dtask`).
fn pfsync_syncif_detach(arg: *mut c_void) {
    let sc = PfsyncSoftc::of_arg(arg);
    let ifp = &sc.sc_if;

    if ifp.if_flags.get() & IFF_RUNNING != 0 {
        let _ = pfsync_down(sc);
        if_down(ifp);
    }

    sc.sc_sync_ifidx.set(0);
}

/// `pfsync_output`: drops the packet.
///
/// # Safety
///
/// As for `if_output` (`IfOutputFn`).
unsafe fn pfsync_output(
    _ifp: &'static Ifnet,
    m: &'static Mbuf,
    _dst: *const Sockaddr,
    _rt: Option<&'static Rtentry>,
) -> Result<(), Errno> {
    m_freem(m); // drop packet
    Err(Errno::EAFNOSUPPORT)
}

/// `pfsync_ioctl`.
///
/// # Safety
///
/// As for `if_ioctl` (`IfIoctlFn`): `data` is the command's argument (a `struct ifreq`, or a
/// `struct if_parent` for `SIOC[SG]IFPARENT`), aligned.
unsafe fn pfsync_ioctl(ifp: &'static Ifnet, cmd: u64, data: *mut u8) -> Result<(), Errno> {
    let sc = PfsyncSoftc::of_ifp(ifp);

    let error = match cmd {
        SIOCSIFADDR => Err(Errno::EOPNOTSUPP),

        SIOCSIFFLAGS => {
            if ifp.if_flags.get() & IFF_UP != 0 {
                if ifp.if_flags.get() & IFF_RUNNING == 0 {
                    pfsync_up(sc)
                } else {
                    Err(Errno::ENETRESET)
                }
            } else if ifp.if_flags.get() & IFF_RUNNING != 0 {
                pfsync_down(sc)
            } else {
                Err(Errno::ENOTTY)
            }
        }

        SIOCSIFMTU => {
            // SAFETY: the caller's contract: `SIOCSIFMTU` takes a `struct ifreq`.
            let ifr = unsafe { &*data.cast::<Ifreq>() };
            pfsync_set_mtu(sc, ifr.ifr_mtu() as u32)
        }

        // SAFETY: the caller's contract: these take a `struct if_parent`.
        SIOCSIFPARENT => pfsync_set_parent(sc, unsafe { &*data.cast::<IfParent>() }),
        // SAFETY: as above.
        SIOCGIFPARENT => pfsync_get_parent(sc, unsafe { &mut *data.cast::<IfParent>() }),
        SIOCDIFPARENT => pfsync_del_parent(sc),

        // SAFETY: the caller's contract: these take a `struct ifreq`.
        SIOCSETPFSYNC => pfsync_set_ioc(sc, unsafe { &*data.cast::<Ifreq>() }),
        // SAFETY: as above.
        SIOCGETPFSYNC => pfsync_get_ioc(sc, unsafe { &*data.cast::<Ifreq>() }),

        _ => Err(Errno::ENOTTY),
    };

    if error == Err(Errno::ENETRESET) {
        return Ok(());
    }

    error
}

/// `pfsync_set_mtu`.
fn pfsync_set_mtu(sc: &PfsyncSoftc, mtu: u32) -> Result<(), Errno> {
    let ifp = &sc.sc_if;

    let Some(ifp0) = if_get(sc.sc_sync_ifidx.get()) else {
        return Err(Errno::EINVAL);
    };

    let error = if mtu as usize <= PFSYNC_MINPKT || mtu > ifp0.if_mtu.get() {
        Err(Errno::EINVAL)
    } else {
        // commit
        ifp.if_mtu.set(mtu);
        Ok(())
    };

    if_put(ifp0);
    error
}

/// `pfsync_set_parent`.
fn pfsync_set_parent(sc: &PfsyncSoftc, p: &IfParent) -> Result<(), Errno> {
    let ifp = &sc.sc_if;

    let Some(ifp0) = if_unit(&p.ifp_parent) else {
        return Err(Errno::ENXIO);
    };

    let error = if ifp0.if_index.get() == sc.sc_sync_ifidx.get() {
        Ok(())
    } else if ifp.if_flags.get() & IFF_RUNNING != 0 {
        Err(Errno::EBUSY)
    } else {
        // commit
        sc.sc_sync_ifidx.set(ifp0.if_index.get());
        Ok(())
    };

    if_put(ifp0);
    error
}

/// `pfsync_get_parent`.
fn pfsync_get_parent(sc: &PfsyncSoftc, p: &mut IfParent) -> Result<(), Errno> {
    let ifp0 = if_get(sc.sc_sync_ifidx.get());
    let error = match ifp0 {
        None => Err(Errno::EADDRNOTAVAIL),
        Some(ifp0) => {
            libkern::strlcpy(&mut p.ifp_parent, &ifp0.if_xname.get());
            Ok(())
        }
    };
    if_put(ifp0);

    error
}

/// `pfsync_del_parent`.
fn pfsync_del_parent(sc: &PfsyncSoftc) -> Result<(), Errno> {
    let ifp = &sc.sc_if;

    if ifp.if_flags.get() & IFF_RUNNING != 0 {
        return Err(Errno::EBUSY);
    }

    // commit
    sc.sc_sync_ifidx.set(0);

    Ok(())
}

/// `pfsync_get_ioc`: `SIOCGETPFSYNC`.
fn pfsync_get_ioc(sc: &PfsyncSoftc, ifr: &Ifreq) -> Result<(), Errno> {
    let mut pfsyncr = Pfsyncreq::default();

    let ifp0 = if_get(sc.sc_sync_ifidx.get());
    if let Some(ifp0) = ifp0 {
        libkern::strlcpy(&mut pfsyncr.pfsyncr_syncdev, &ifp0.if_xname.get());
    }
    if_put(ifp0);

    pfsyncr.pfsyncr_syncpeer = sc.sc_syncpeer.get();
    pfsyncr.pfsyncr_maxupdates = sc.sc_maxupdates.get() as i32;
    pfsyncr.pfsyncr_defer = sc.sc_defer.get() as i32;

    copyout_obj(&pfsyncr, ifr.ifr_data() as usize)
}

/// `pfsync_set_ioc`: `SIOCSETPFSYNC`.
fn pfsync_set_ioc(sc: &PfsyncSoftc, ifr: &Ifreq) -> Result<(), Errno> {
    let ifp = &sc.sc_if;
    let mut sync_ifidx = sc.sc_sync_ifidx.get();
    let mut wantdown = false;

    let Some(p) = curproc() else {
        return Err(Errno::EPERM);
    };
    suser(p)?;

    let mut pfsyncr: Pfsyncreq = copyin_obj(ifr.ifr_data() as usize)?;

    if pfsyncr.pfsyncr_maxupdates > 255 {
        return Err(Errno::EINVAL);
    }

    if pfsyncr.pfsyncr_syncdev[0] != 0 {
        // set
        let Some(ifp0) = if_unit(&pfsyncr.pfsyncr_syncdev) else {
            return Err(Errno::ENXIO);
        };

        if ifp0.if_index.get() != sync_ifidx {
            wantdown = true;
        }

        sync_ifidx = ifp0.if_index.get();
        if_put(ifp0);
    } else {
        // del
        wantdown = true;
        sync_ifidx = 0;
    }

    if pfsyncr.pfsyncr_syncpeer.s_addr == INADDR_ANY {
        pfsyncr.pfsyncr_syncpeer.s_addr = INADDR_PFSYNC_GROUP;
    }
    if pfsyncr.pfsyncr_syncpeer.s_addr != sc.sc_syncpeer.get().s_addr {
        wantdown = true;
    }

    if wantdown && ifp.if_flags.get() & IFF_RUNNING != 0 {
        return Err(Errno::EBUSY);
    }

    // commit
    sc.sc_sync_ifidx.set(sync_ifidx);
    sc.sc_syncpeer.set(pfsyncr.pfsyncr_syncpeer);
    sc.sc_maxupdates.set(pfsyncr.pfsyncr_maxupdates as u32);
    sc.sc_defer.set(pfsyncr.pfsyncr_defer as u32);

    Ok(())
}

/// `pfsync_up`: joins the sync peer's group, publishes the softc to pf and starts the bulk
/// update request. Called with the net lock held.
fn pfsync_up(sc: &'static PfsyncSoftc) -> Result<(), Errno> {
    let ifp = &sc.sc_if;
    let mut inm: Option<&'static InMulti> = None;

    net_assert_locked("pfsync_up");
    kassert!(ifp.if_flags.get() & IFF_RUNNING == 0);

    if sc.sc_dead.get() != 0 {
        return Err(Errno::ENXIO);
    }

    // coordinate with pfsync_down(). if sc_up is still up and we're here then something else
    // is tearing pfsync down.
    if sc.sc_up.get() != 0 {
        return Err(Errno::EBUSY);
    }

    let syncpeer = sc.sc_syncpeer.get();
    if syncpeer.s_addr == INADDR_ANY || syncpeer.s_addr == INADDR_BROADCAST {
        return Err(Errno::EDESTADDRREQ);
    }

    let Some(ifp0) = if_get(sc.sc_sync_ifidx.get()) else {
        return Err(Errno::ENXIO);
    };

    let error = 'put: {
        if in_multicast(syncpeer.s_addr) {
            if ifp0.if_flags.get() & IFF_MULTICAST == 0 {
                break 'put Err(Errno::ENODEV);
            }
            inm = in_addmulti(&syncpeer, ifp0);
            if inm.is_none() {
                break 'put Err(Errno::ECONNABORTED);
            }
        }

        sc.sc_up.set(1);

        let mut ip = Ip::default();
        ip.set_ip_v(IPVERSION);
        ip.set_ip_hl((size_of::<Ip>() >> 2) as u8);
        ip.ip_tos = IPTOS_LOWDELAY;
        // len and id are set later
        ip.ip_off = htons(IP_DF);
        ip.ip_ttl = PFSYNC_DFLTTL;
        ip.ip_p = IPPROTO_PFSYNC as u8;
        ip.ip_src.s_addr = INADDR_ANY;
        ip.ip_dst.s_addr = syncpeer.s_addr;
        sc.sc_template.set(ip);

        // commit
        refcnt_init(&sc.sc_refs); // IFF_RUNNING kind of owns this

        // NCARP > 0: sc_sync_if_down = 1; carp_group_demote_adj(&sc->sc_if, 1, "pfsync up");
        // carp(4) is not configured.

        // SAFETY: the tasks are part of the softc, on no list (`pfsync_down` takes them off),
        // and the softc outlives the hooks: `pfsync_down` runs before it is freed, and the
        // detach hook runs (and so ends) before the sync interface goes.
        unsafe {
            if_linkstatehook_add(ifp0, &sc.sc_ltask);
            if_detachhook_add(ifp0, &sc.sc_dtask);
        }

        sc.sc_inm.set(inm);
        ifp.if_flags.set(ifp.if_flags.get() | IFF_RUNNING);

        pfsync_bulk_req_evt(sc, PfsyncBulkReqEvent::Up);

        refcnt_take(&sc.sc_refs); // give one to SMR
        PFSYNCIF.store(ptr::from_ref(sc).cast_mut(), Ordering::Release);

        pfsync_syncif_link(sc.arg()); // try and push the bulk req state forward

        Ok(())
    };

    if_put(ifp0);
    error
}

/// `pfsync_encap`: puts the IP and pfsync headers in front of the messages in `m`.
fn pfsync_encap(sc: &PfsyncSoftc, m: &'static Mbuf) -> Option<&'static Mbuf> {
    const HLEN: usize = size_of::<Ip>() + size_of::<PfsyncHeader>();
    let mut mlen = m.m_pkthdr().len.get() as usize;

    let m = m_prepend(m, HLEN as i32, M_DONTWAIT)?;

    // SAFETY: `m_prepend` made `HLEN` contiguous bytes at the start of `m`'s data.
    let h = unsafe { MbufWriter::new(m, HLEN) };
    h.fill_zero();

    mlen += size_of::<PfsyncHeader>();
    let ph = PfsyncHeader {
        version: PFSYNC_VERSION,
        len: htons(mlen as u16),
        // h->ph.spare is all zero
        ..PfsyncHeader::default()
    };
    h.put(size_of::<Ip>(), &ph);

    mlen += size_of::<Ip>();
    let mut ip = sc.sc_template.get();
    ip.ip_len = htons(mlen as u16);
    ip.ip_id = htons(ip_randomid());
    h.put_ip(0, &ip);

    Some(m)
}

/// `pfsync_bulk_req_send`: asks the peers for a bulk update (an `UPD_REQ` for id 0).
fn pfsync_bulk_req_send(sc: &PfsyncSoftc) {
    const HLEN: usize = size_of::<PfsyncSubheader>() + size_of::<PfsyncUpdReq>();
    let mlen = MAX_LINKHDR.load(Ordering::Relaxed) as usize
        + size_of::<Ip>()
        + size_of::<PfsyncHeader>()
        + HLEN;

    'fail: {
        let Some(m) = m_gethdr(M_DONTWAIT, MT_DATA) else {
            break 'fail;
        };

        if mlen > MHLEN {
            let _ = mclgetl(m, M_DONTWAIT, mlen as u32);
            if m.m_flags().get() & M_EXT == 0 {
                // drop:
                m_freem(m);
                break 'fail;
            }
        }

        m_align(m, HLEN as i32);
        m.m_len().set(HLEN as u32);
        m.m_pkthdr().len.set(HLEN as i32);

        // SAFETY: `m_align` placed `HLEN` bytes of `m`'s data area at `mtod(m)`.
        let h = unsafe { MbufWriter::new(m, HLEN) };
        h.fill_zero();

        let subh = PfsyncSubheader {
            action: PFSYNC_ACT_UPD_REQ,
            len: (size_of::<PfsyncUpdReq>() >> 2) as u8,
            count: htons(1),
        };
        h.put(0, &subh);

        let ur = PfsyncUpdReq {
            id: htobe64(0),
            creatorid: htonl(0),
        };
        h.put(size_of::<PfsyncSubheader>(), &ur);

        let Some(m) = pfsync_encap(sc, m) else {
            break 'fail;
        };

        pfsync_sendout(sc, m);
        return;
    }

    let xname = sc.sc_if.if_xname.get();
    printf(format_args!(
        "{}: unable to request bulk update\n",
        Str(&xname)
    ));
}

/// `pfsync_bulk_req_nstate`: moves the bulk request to `nstate`, with a timeout of `seconds`
/// (none when 0).
fn pfsync_bulk_req_nstate(sc: &PfsyncSoftc, nstate: PfsyncBulkReqState, seconds: i32) {
    sc.sc_bulk_req.req_state.set(nstate);
    if seconds > 0 {
        let _ = timeout_add_sec(&sc.sc_bulk_req.req_tmo, seconds);
    } else {
        let _ = timeout_del(&sc.sc_bulk_req.req_tmo);
    }
}

/// `pfsync_bulk_req_invstate`: an event the state does not expect.
fn pfsync_bulk_req_invstate(sc: &PfsyncSoftc, evt: PfsyncBulkReqEvent) -> ! {
    let xname = sc.sc_if.if_xname.get();
    panic(format_args!(
        "{}: unexpected event {} in state {}",
        Str(&xname),
        PFSYNC_BULK_REQ_EVENT_NAMES[evt as usize],
        PFSYNC_BULK_REQ_STATE_NAMES[sc.sc_bulk_req.req_state.get() as usize]
    ));
}

/// `pfsync_bulk_req_nstate_bulk`: the peer started a bulk update; allow it four times the
/// time its packets should take.
fn pfsync_bulk_req_nstate_bulk(sc: &PfsyncSoftc) {
    // calculate the number of packets we expect
    let per_pkt = (sc.sc_if.if_mtu.get() as usize - PFSYNC_MINPKT) / size_of::<PfsyncState>();
    let mut t = (PF_POOL_LIMITS[PF_LIMIT_STATES].limit.get() as usize / per_pkt) as i32;

    // turn it into seconds
    t /= (1000 / PFSYNC_BULK_SND_IVAL_MS) as i32;

    if t == 0 {
        t = 1;
    }

    pfsync_bulk_req_nstate(sc, PfsyncBulkReqState::Bulk, t * 4);
}

/// `pfsync_bulk_req_nstate_done`.
fn pfsync_bulk_req_nstate_done(sc: &PfsyncSoftc) {
    pfsync_bulk_req_nstate(sc, PfsyncBulkReqState::Done, 0);

    kassert!(sc.sc_bulk_req.req_demoted.get() == 1);
    sc.sc_bulk_req.req_demoted.set(0);

    // NCARP > 0: carp_group_demote_adj(&sc->sc_if, -32, "pfsync done"); not configured.
}

/// `pfsync_bulk_req_evt`: the bulk request's state machine.
fn pfsync_bulk_req_evt(sc: &PfsyncSoftc, evt: PfsyncBulkReqEvent) {
    use PfsyncBulkReqEvent as E;
    use PfsyncBulkReqState as S;

    let br = &sc.sc_bulk_req;

    rw_enter_write(&br.req_lock);
    pfsync_dprintf(
        sc,
        format_args!(
            "pfsync_bulk_req_evt state {} evt {}",
            PFSYNC_BULK_REQ_STATE_NAMES[br.req_state.get() as usize],
            PFSYNC_BULK_REQ_EVENT_NAMES[evt as usize]
        ),
    );

    if evt == E::Down {
        // unconditionally move down
        br.req_tries.set(0);
        pfsync_bulk_req_nstate(sc, S::None, 0);

        if br.req_demoted.get() != 0 {
            br.req_demoted.set(0);
            // NCARP > 0: carp_group_demote_adj(&sc->sc_if, -32, "pfsync down"); not
            // configured.
        }
    } else {
        match br.req_state.get() {
            S::None => match evt {
                E::Up => {
                    kassert!(br.req_demoted.get() == 0);
                    br.req_demoted.set(1);
                    // NCARP > 0: carp_group_demote_adj(&sc->sc_if, 32, "pfsync start"); not
                    // configured.
                    pfsync_bulk_req_nstate(sc, S::Start, 30);
                }
                _ => pfsync_bulk_req_invstate(sc, evt),
            },

            S::Start => match evt {
                E::Link => {
                    pfsync_bulk_req_send(sc);
                    pfsync_bulk_req_nstate(sc, S::Sent, 2);
                }
                E::Tmo => {
                    pfsync_dprintf(sc, format_args!("timeout waiting for link"));
                    pfsync_bulk_req_nstate_done(sc);
                }
                E::BusStart => pfsync_bulk_req_nstate_bulk(sc),
                E::BusEnd => {
                    // ignore this
                }
                _ => pfsync_bulk_req_invstate(sc, evt),
            },

            S::Sent => match evt {
                E::BusStart => pfsync_bulk_req_nstate_bulk(sc),
                E::BusEnd | E::Link => {
                    // ignore this
                }
                E::Tmo => {
                    br.req_tries.set(br.req_tries.get() + 1);
                    if br.req_tries.get() < PFSYNC_MAX_BULKTRIES {
                        pfsync_bulk_req_send(sc);
                        pfsync_bulk_req_nstate(sc, S::Sent, 2);
                    } else {
                        pfsync_dprintf(sc, format_args!("timeout waiting for bulk transfer start"));
                        pfsync_bulk_req_nstate_done(sc);
                    }
                }
                _ => pfsync_bulk_req_invstate(sc, evt),
            },

            S::Bulk => match evt {
                E::BusStart | E::Link => {
                    // ignore this
                }
                E::BusEnd => pfsync_bulk_req_nstate_done(sc),
                E::Tmo => {
                    br.req_tries.set(br.req_tries.get() + 1);
                    if br.req_tries.get() < PFSYNC_MAX_BULKTRIES {
                        pfsync_bulk_req_send(sc);
                        pfsync_bulk_req_nstate(sc, S::Sent, 2);
                    }

                    pfsync_dprintf(sc, format_args!("timeout waiting for bulk transfer end"));
                    pfsync_bulk_req_nstate_done(sc);
                }
                _ => pfsync_bulk_req_invstate(sc, evt),
            },

            // pfsync is up and running
            S::Done => match evt {
                E::BusStart | E::BusEnd | E::Link => {
                    // nops
                }
                _ => pfsync_bulk_req_invstate(sc, evt),
            },
        }
    }
    rw_exit_write(&br.req_lock);
}

/// `pfsync_bulk_req_tmo`: `req_tmo`.
fn pfsync_bulk_req_tmo(arg: *mut c_void) {
    let sc = PfsyncSoftc::of_arg(arg);

    net_lock();
    pfsync_bulk_req_evt(sc, PfsyncBulkReqEvent::Tmo);
    net_unlock();
}

/// `(void (*)(void *))refcnt_rele_wake`: `pfsync_down`'s deferred release of the reference
/// it gave SMR when `pfsync_up` published the softc.
fn pfsync_refcnt_rele_wake(refs: *mut c_void) {
    // SAFETY: `pfsync_down` passes its softc's `sc_refs`, which it keeps alive in
    // `refcnt_finalize` until this release has run.
    refcnt_rele_wake(unsafe { &*refs.cast::<crate::sys::refcnt::Refcnt>() });
}

/// `pfsync_down`: withdraws the softc from pf, waits for every context still running pfsync
/// and drops what is queued; the deferred packets go out. Called with the net lock held,
/// which it lets go of while it waits.
fn pfsync_down(sc: &'static PfsyncSoftc) -> Result<(), Errno> {
    let ifp = &sc.sc_if;
    let mut sndbar = false;
    let pds: TailqHead<PfsyncDeferrals> = TailqHead::new();

    net_assert_locked("pfsync_down");
    kassert!(ifp.if_flags.get() & IFF_RUNNING != 0);

    // tearing down pfsync involves waiting for pfsync to stop running in various contexts
    // including softnet taskqs. this thread cannot hold netlock while waiting for a barrier
    // in softnet because softnet might be waiting for the netlock. sc->sc_up is used to
    // coordinate with pfsync_up.

    ifp.if_flags.set(ifp.if_flags.get() & !IFF_RUNNING);

    let ifp0 = if_get(sc.sc_sync_ifidx.get());
    if let Some(ifp0) = ifp0 {
        // SAFETY: `pfsync_up` added both tasks to this interface (the sync interface cannot
        // change while running: `pfsync_set_parent` and `pfsync_set_ioc` refuse).
        unsafe {
            if_linkstatehook_del(ifp0, &sc.sc_ltask);
            if_detachhook_del(ifp0, &sc.sc_dtask);
        }
    }
    if_put(ifp0);

    // NCARP > 0: carp_group_demote_adj(&sc->sc_if, -1, "pfsync down") when sc_sync_if_down;
    // not configured.

    net_unlock();

    let cur = PFSYNCIF.load(Ordering::Acquire);
    if !ptr::eq(cur, sc) {
        panic(format_args!(
            "pfsyncif {cur:p} != sc {:p}",
            ptr::from_ref(sc)
        ));
    }
    PFSYNCIF.store(ptr::null_mut(), Ordering::Release);
    let smr = SmrEntry::new();
    smr_init(&smr);
    // SAFETY: `smr` stays on this frame until `refcnt_finalize` below returns, which is after
    // the SMR thread called `refcnt_rele_wake` through it, its last use of the entry: for that
    // long it may be lent as `'static` (as `smr_barrier` lends its own).
    let smr_static: &'static SmrEntry = unsafe { &*ptr::from_ref(&smr) };
    smr_call(
        smr_static,
        pfsync_refcnt_rele_wake,
        ptr::from_ref(&sc.sc_refs).cast_mut().cast(),
    );

    // stop pf producing work before cleaning up the timeouts and tasks
    refcnt_finalize(&sc.sc_refs, "pfsyncfini");

    pfsync_bulk_req_evt(sc, PfsyncBulkReqEvent::Down);

    let bs = &sc.sc_bulk_snd;
    rw_enter_read(&PF_STATE_LIST.pfs_rwl);
    rw_enter_write(&bs.snd_lock);
    if bs.snd_tail.get().is_some() {
        sndbar = !timeout_del(&bs.snd_tmo);

        bs.snd_again.set(0);
        bs.snd_next.set(None);
        bs.snd_tail.set(None);
    }
    rw_exit_write(&bs.snd_lock);
    rw_exit_read(&PF_STATE_LIST.pfs_rwl);

    // do a single barrier for all the timeouts. because the timeouts in each slice are
    // configured the same way, the barrier for one will work for all of them.
    for s in &sc.sc_slices {
        let _ = timeout_del(&s.s_tmo);
        let _ = task_del(s.softnet(), &s.s_task);
        let _ = task_del(s.softnet(), &s.s_send);

        let _ = timeout_del(&s.s_deferrals_tmo);
        let _ = task_del(s.softnet(), &s.s_deferrals_task);
    }
    timeout_barrier(&sc.sc_slices[0].s_tmo);
    timeout_barrier(&sc.sc_bulk_req.req_tmo); // XXX proc
    if sndbar {
        // technically the preceding barrier does the same job
        timeout_barrier(&bs.snd_tmo);
    }
    net_tq_barriers("pfsyncbar");

    // pfsync is no longer running

    let inm = sc.sc_inm.take();

    for s in &sc.sc_slices {
        pfsync_slice_drop(sc, s);
        let _ = mq_purge(&s.s_sendq);

        while let Some(pd) = s.s_deferrals.first() {
            // SAFETY: `pd` is on the slice's list; it moves to the local list, which lives
            // until every deferral is taken off it below.
            unsafe {
                s.s_deferrals.remove(pd);
            }

            if let Some(st) = pd.pd_st.get() {
                st.sync_defer.set(None);
            }

            // SAFETY: as above.
            unsafe { pds.insert_tail(pd) };
        }
        s.s_deferred.set(0);
    }

    net_lock();
    sc.sc_up.set(0);

    if let Some(inm) = inm {
        in_delmulti(inm);
    }

    while let Some(pd) = pds.first() {
        // SAFETY: on the local list, put there above.
        unsafe { pds.remove(pd) };

        pfsync_defer_output(pd_static(pd));
    }

    Ok(())
}

/// A deferral as the `&'static` pool item it is.
fn pd_static(pd: &PfsyncDeferral) -> &'static PfsyncDeferral {
    // SAFETY: deferrals are pool items (`pfsync_defer`), valid until `pfsync_defer_output`
    // gives them back.
    unsafe { &*ptr::from_ref(pd) }
}

/// `pfsync_is_up`: whether a pfsync interface is running.
pub fn pfsync_is_up() -> bool {
    smr_read_enter();
    let rv = pfsync_if().is_some();
    smr_read_leave();
    rv
}

/// `pfsync_start`: nothing is sent through the interface's queue.
fn pfsync_start(ifq: &'static Ifqueue) {
    let _ = ifq_purge(ifq);
}

/// `pfsync_slice_enter`: the slice of `st`, locked.
fn pfsync_slice_enter(sc: &'static PfsyncSoftc, st: &PfState) -> &'static PfsyncSlice {
    let hash = st.key[0].get().map_or(0, |k| usize::from(k.hash.get()));
    let idx = hash % PFSYNC_NSLICES;
    let s = &sc.sc_slices[idx];

    if !mtx_enter_try(&s.s_mtx) {
        mtx_enter(&s.s_mtx);
        s.s_stat_contended.set(s.s_stat_contended.get() + 1);
    }
    s.s_stat_locks.set(s.s_stat_locks.get() + 1);

    s
}

/// `pfsync_slice_leave`.
fn pfsync_slice_leave(_sc: &PfsyncSoftc, s: &PfsyncSlice) {
    mtx_leave(&s.s_mtx);
}

/// `pfsync_out_state`: INS and UPD.
fn pfsync_out_state(st: &'static PfState, buf: &mut [u8]) {
    let mut sp = PfsyncState::default();

    mtx_enter(&st.mtx);
    pf_state_export(&mut sp, st);
    mtx_leave(&st.mtx);

    wire_put(buf, &sp);
}

/// `pfsync_out_iack`.
fn pfsync_out_iack(st: &'static PfState, buf: &mut [u8]) {
    let iack = PfsyncInsAck {
        id: st.id.get(),
        creatorid: st.creatorid.get(),
    };
    wire_put(buf, &iack);
}

/// `pfsync_out_upd_c`.
fn pfsync_out_upd_c(st: &'static PfState, buf: &mut [u8]) {
    let mut up = PfsyncUpdC {
        id: st.id.get(),
        creatorid: st.creatorid.get(),
        ..PfsyncUpdC::default()
    };

    let mut src = PfsyncStatePeer::default();
    let mut dst = PfsyncStatePeer::default();
    mtx_enter(&st.mtx);
    pf_state_peer_hton(&st.src, &mut src);
    pf_state_peer_hton(&st.dst, &mut dst);
    up.timeout = st.timeout.get();
    mtx_leave(&st.mtx);
    up.src = src;
    up.dst = dst;

    wire_put(buf, &up);
}

/// `pfsync_out_del`.
fn pfsync_out_del(st: &'static PfState, buf: &mut [u8]) {
    let dp = PfsyncDelC {
        id: st.id.get(),
        creatorid: st.creatorid.get(),
    };
    wire_put(buf, &dp);

    st.sync_state.set(PFSYNC_S_DEAD);
}

/// `pfsync_tdb_enter`.
fn pfsync_tdb_enter(tdb: &Tdb) {
    mtx_enter(&tdb.tdb_mtx);
}

/// `pfsync_tdb_leave`: wakes `pfsync_delete_tdb` up when the TDB is being freed.
fn pfsync_tdb_leave(tdb: &Tdb) {
    let snapped = tdb.tdb_flags.get() & TDBF_PFSYNC_SNAPPED != 0;
    mtx_leave(&tdb.tdb_mtx);
    if snapped {
        wakeup_one(ptr::from_ref(&tdb.tdb_updates));
    }
}

/// `pfsync_slice_drop`: empties the slice's queues.
fn pfsync_slice_drop(_sc: &PfsyncSoftc, s: &PfsyncSlice) {
    for q in &s.s_qs {
        if q.is_empty() {
            continue;
        }

        while let Some(st) = q.first() {
            // SAFETY: `st` is on this queue.
            unsafe { q.remove(st) };
            // PFSYNC_DEBUG: KASSERT(st->sync_state == q); not defined.
            st.sync_state.set(PFSYNC_S_NONE);
            pf_state_unref(Some(state_static(st)));
        }
    }

    while let Some(tdb) = s.s_tdb_q.first() {
        // SAFETY: `tdb` is on this queue.
        unsafe { s.s_tdb_q.remove(tdb) };

        pfsync_tdb_enter(tdb);
        kassert!(tdb.tdb_flags.get() & TDBF_PFSYNC != 0);
        tdb.tdb_flags.set(tdb.tdb_flags.get() & !TDBF_PFSYNC);
        pfsync_tdb_leave(tdb);
    }

    let _ = timeout_del(&s.s_tmo);
    s.s_len.set(PFSYNC_MINPKT);
}

/// A state on a pfsync queue as the `&'static` pool item it is.
fn state_static(st: &PfState) -> &'static PfState {
    // SAFETY: states are pool items; one on a pfsync queue holds a reference (`pfsync_q_ins`)
    // that keeps it allocated.
    unsafe { &*ptr::from_ref(st) }
}

/// `pfsync_slice_write`: the slice's queues as one frame; the queues are emptied. Called with
/// the slice's mutex held.
fn pfsync_slice_write(s: &PfsyncSlice) -> Option<&'static Mbuf> {
    let sc = s.sc();
    let len = s.s_len.get();
    let mlen = MAX_LINKHDR.load(Ordering::Relaxed) as usize + len;

    mutex_assert_locked(&s.s_mtx, "pfsync_slice_write");
    if len == PFSYNC_MINPKT {
        s.s_stat_write_nop.set(s.s_stat_write_nop.get() + 1);
        return None;
    }

    let _ = task_del(s.softnet(), &s.s_task);

    let m = 'drop: {
        let Some(m) = m_gethdr(M_DONTWAIT, MT_DATA) else {
            break 'drop None;
        };

        if mlen > MHLEN {
            let _ = mclgetl(m, M_DONTWAIT, mlen as u32);
            if m.m_flags().get() & M_EXT == 0 {
                break 'drop Some(m);
            }
        }

        m_align(m, len as i32);
        m.m_len().set(len as u32);
        m.m_pkthdr().len.set(len as i32);

        // SAFETY: `m_align` placed `len` bytes of `m`'s data area at `mtod(m)`; the frame is
        // written into them and nothing else sees `m` yet.
        let buf = unsafe { MbufWriter::new(m, len) };
        let mut off = 0;

        let mut ip = sc.sc_template.get();
        ip.ip_len = htons(len as u16);
        ip.ip_id = htons(ip_randomid());
        buf.put_ip(off, &ip);
        off += size_of::<Ip>();

        let ph = PfsyncHeader {
            version: PFSYNC_VERSION,
            len: htons((len - size_of::<Ip>()) as u16),
            ..PfsyncHeader::default()
        };
        buf.put(off, &ph);
        off += size_of::<PfsyncHeader>();

        for (q, psq) in s.s_qs.iter().enumerate() {
            if psq.is_empty() {
                continue;
            }

            let subh_off = off;
            off += size_of::<PfsyncSubheader>();

            let mut count: u16 = 0;
            while let Some(st) = psq.first() {
                // SAFETY: `st` is on this queue.
                unsafe { psq.remove(st) };
                count += 1;

                kassert!(usize::from(st.sync_state.get()) == q);
                // the write handler below may override this
                st.sync_state.set(PFSYNC_S_NONE);

                let st = state_static(st);
                let mut msg = [0u8; size_of::<PfsyncState>()];
                (PFSYNC_QS[q].write)(st, &mut msg[..PFSYNC_QS[q].len]);
                buf.copyin(off, &msg[..PFSYNC_QS[q].len]);
                off += PFSYNC_QS[q].len;

                pf_state_unref(Some(st));
            }

            let subh = PfsyncSubheader {
                action: PFSYNC_QS[q].action,
                len: (PFSYNC_QS[q].len >> 2) as u8,
                count: htons(count),
            };
            buf.put(subh_off, &subh);
        }

        if !s.s_tdb_q.is_empty() {
            let subh_off = off;
            off += size_of::<PfsyncSubheader>();

            let mut count: u16 = 0;
            while let Some(tdb) = s.s_tdb_q.first() {
                // SAFETY: `tdb` is on this queue.
                unsafe { s.s_tdb_q.remove(tdb) };
                count += 1;

                pfsync_tdb_enter(tdb);
                kassert!(tdb.tdb_flags.get() & TDBF_PFSYNC != 0);

                // get a consistent view of the counters
                let mut msg = [0u8; size_of::<PfsyncTdb>()];
                pfsync_out_tdb(tdb, &mut msg);
                buf.copyin(off, &msg);

                tdb.tdb_flags.set(tdb.tdb_flags.get() & !TDBF_PFSYNC);
                pfsync_tdb_leave(tdb);

                off += size_of::<PfsyncTdb>();
            }

            let subh = PfsyncSubheader {
                action: PFSYNC_ACT_TDB,
                len: (size_of::<PfsyncTdb>() >> 2) as u8,
                count: htons(count),
            };
            buf.put(subh_off, &subh);
        }

        let _ = timeout_del(&s.s_tmo);
        s.s_len.set(PFSYNC_MINPKT);

        return Some(m);
    };

    // drop:
    m_freem(m);
    pfsyncstat_inc(PfsyncCounters::PfsyncsOnomem);
    pfsync_slice_drop(sc, s);
    None
}

/// `pfsync_sendout`: sends a frame to the sync peer out of the sync interface.
fn pfsync_sendout(sc: &PfsyncSoftc, m: &'static Mbuf) {
    let len = m.m_pkthdr().len.get() as u64;
    let if_bpf = sc.sc_if.if_bpf.get();
    if !if_bpf.is_null() {
        let _ = bpf_mtap(if_bpf, m, BPF_DIRECTION_OUT);
    }

    let imo = IpMoptions {
        imo_membership: ptr::null_mut(),
        imo_ifidx: sc.sc_sync_ifidx.get() as u16,
        imo_ttl: PFSYNC_DFLTTL,
        imo_loop: 0,
        imo_num_memberships: 0,
        imo_max_memberships: 0,
    };
    m.m_pkthdr().ph_rtableid.set(sc.sc_if.if_rdomain.get());

    if ip_output(m, None, None, IP_RAWOUTPUT, Some(&imo), None, 0).is_ok() {
        ifp_counters_pkt(
            &sc.sc_if,
            IfCounters::IfcOpackets,
            IfCounters::IfcObytes,
            len,
        );
        pfsyncstat_inc(PfsyncCounters::PfsyncsOpackets);
    } else {
        ifp_counters_inc(&sc.sc_if, IfCounters::IfcOerrors);
        pfsyncstat_inc(PfsyncCounters::PfsyncsOerrors);
    }
}

/// `pfsync_slice_tmo`: `s_tmo`.
fn pfsync_slice_tmo(arg: *mut c_void) {
    let s = PfsyncSlice::of_arg(arg);

    let _ = task_add(s.softnet(), &s.s_task);
}

/// `pfsync_slice_sched`: send the slice's queues soon.
fn pfsync_slice_sched(s: &'static PfsyncSlice) {
    s.s_stat_task_add.set(s.s_stat_task_add.get() + 1);
    let _ = task_add(s.softnet(), &s.s_task);
}

/// `pfsync_slice_task`: `s_task`.
fn pfsync_slice_task(arg: *mut c_void) {
    let s = PfsyncSlice::of_arg(arg);

    mtx_enter(&s.s_mtx);
    s.s_stat_task_run.set(s.s_stat_task_run.get() + 1);

    let m = pfsync_slice_write(s);
    mtx_leave(&s.s_mtx);
    if let Some(m) = m {
        net_lock();
        pfsync_sendout(s.sc(), m);
        net_unlock();
    }
}

/// `pfsync_slice_sendq`: `s_send`.
fn pfsync_slice_sendq(arg: *mut c_void) {
    let s = PfsyncSlice::of_arg(arg);
    let ml = MbufList::new();

    mq_delist(&s.s_sendq, &ml);
    if ml_empty(&ml) {
        return;
    }

    mtx_enter(&s.s_mtx);
    s.s_stat_dequeue.set(s.s_stat_dequeue.get() + 1);
    mtx_leave(&s.s_mtx);

    net_lock();
    while let Some(m) = ml_dequeue(&ml) {
        pfsync_sendout(s.sc(), m);
    }
    net_unlock();
}

/// `pfsync_q_ins`: queues `st` for message `q`; a full frame goes out first.
fn pfsync_q_ins(s: &'static PfsyncSlice, st: &'static PfState, q: u8) {
    let qi = usize::from(q);
    let mut nlen = PFSYNC_QS[qi].len;

    mutex_assert_locked(&s.s_mtx, "pfsync_q_ins");
    kassert!(st.sync_state.get() == PFSYNC_S_NONE);
    kassert!(s.s_len.get() >= PFSYNC_MINPKT);

    if s.s_qs[qi].is_empty() {
        nlen += size_of::<PfsyncSubheader>();
    }

    if s.s_len.get() + nlen > s.sc().sc_if.if_mtu.get() as usize {
        let m = pfsync_slice_write(s);
        if let Some(m) = m {
            s.s_stat_enqueue.set(s.s_stat_enqueue.get() + 1);
            if !mq_enqueue(&s.s_sendq, m) {
                let _ = task_add(s.softnet(), &s.s_send);
            }
        }

        nlen = size_of::<PfsyncSubheader>() + PFSYNC_QS[qi].len;
    }

    s.s_len.set(s.s_len.get() + nlen);
    let st = pf_state_ref(st);
    // SAFETY: a state with `PFSYNC_S_NONE` is on no pfsync queue; the reference just taken
    // keeps it allocated until `pfsync_q_del`, `pfsync_slice_write` or `pfsync_slice_drop`
    // takes it off.
    unsafe { s.s_qs[qi].insert_tail(st) };
    st.sync_state.set(q);

    if !timeout_pending(&s.s_tmo) {
        let _ = timeout_add_sec(&s.s_tmo, 1);
    }
}

/// `pfsync_q_del`: takes `st` off its queue.
fn pfsync_q_del(s: &PfsyncSlice, st: &'static PfState) {
    let q = usize::from(st.sync_state.get());

    mutex_assert_locked(&s.s_mtx, "pfsync_q_del");
    kassert!(st.sync_state.get() < PFSYNC_S_NONE);

    st.sync_state.set(PFSYNC_S_NONE);
    // SAFETY: a state's `sync_state` below `PFSYNC_S_NONE` names the queue it is on.
    unsafe { s.s_qs[q].remove(st) };
    pf_state_unref(Some(st));
    s.s_len.set(s.s_len.get() - PFSYNC_QS[q].len);

    if s.s_qs[q].is_empty() {
        s.s_len.set(s.s_len.get() - size_of::<PfsyncSubheader>());
    }
}

// the pfsync hooks that pf calls

/// `pfsync_init_state`: the sync state of a new state, before `pf_state_insert`.
pub fn pfsync_init_state(
    st: &PfState,
    skw: Option<&PfStateKey>,
    _sks: Option<&PfStateKey>,
    flags: i32,
) {
    // this is called before pf_state_insert

    if skw.is_some_and(|k| i32::from(k.proto.get()) == IPPROTO_PFSYNC) {
        st.state_flags.set(st.state_flags.get() | PFSTATE_NOSYNC);
    }

    if st.state_flags.get() & PFSTATE_NOSYNC != 0 {
        st.sync_state.set(PFSYNC_S_DEAD);
        return;
    }

    if flags & PFSYNC_SI_IOCTL != 0 {
        // all good
        return;
    }

    // state came off the wire
    if flags & PFSYNC_SI_PFSYNC != 0 {
        if st.state_flags.get() & PFSTATE_ACK != 0 {
            st.state_flags.set(st.state_flags.get() & !PFSTATE_ACK);

            // peer wants an iack, not an insert
            st.sync_state.set(PFSYNC_S_SYNC);
        } else {
            st.sync_state.set(PFSYNC_S_PFSYNC);
        }
    }
}

/// `pfsync_insert_state`: a state was inserted.
pub fn pfsync_insert_state(st: &'static PfState) {
    mutex_assert_unlocked(&st.mtx, "pfsync_insert_state");

    if st.state_flags.get() & PFSTATE_NOSYNC != 0 || st.sync_state.get() == PFSYNC_S_DEAD {
        return;
    }

    smr_read_enter();
    if let Some(sc) = pfsync_if() {
        let s = pfsync_slice_enter(sc, st);

        match st.sync_state.get() {
            PFSYNC_S_UPD_C | PFSYNC_S_NONE => {
                if st.sync_state.get() == PFSYNC_S_UPD_C {
                    // we must have lost a race after insert
                    pfsync_q_del(s, st);
                }
                // FALLTHROUGH
                pfsync_q_ins(s, st, PFSYNC_S_INS);
            }
            PFSYNC_S_SYNC => {
                st.sync_state.set(PFSYNC_S_NONE); // gross
                pfsync_q_ins(s, st, PFSYNC_S_IACK);
                pfsync_slice_sched(s); // the peer is waiting
            }
            PFSYNC_S_PFSYNC => {
                // state was just inserted by pfsync
                st.sync_state.set(PFSYNC_S_NONE);
            }
            ss => panic(format_args!(
                "pfsync_insert_state: state {:p} unexpected sync_state {}",
                ptr::from_ref(st),
                ss
            )),
        }

        pfsync_slice_leave(sc, s);
    }
    smr_read_leave();
}

/// `pfsync_update_state`: a state changed.
pub fn pfsync_update_state(st: &'static PfState) {
    mutex_assert_unlocked(&st.mtx, "pfsync_update_state");

    if st.state_flags.get() & PFSTATE_NOSYNC != 0 || st.sync_state.get() == PFSYNC_S_DEAD {
        return;
    }

    smr_read_enter();
    if let Some(sc) = pfsync_if() {
        let s = pfsync_slice_enter(sc, st);
        let mut sync = false;

        match st.sync_state.get() {
            PFSYNC_S_UPD_C | PFSYNC_S_UPD => {
                // we're already handling it
                if st.key[PF_SK_WIRE]
                    .get()
                    .is_some_and(|k| i32::from(k.proto.get()) == IPPROTO_TCP)
                {
                    st.sync_updates.set(st.sync_updates.get().wrapping_add(1));
                    if u32::from(st.sync_updates.get()) >= sc.sc_maxupdates.get() {
                        sync = true;
                    }
                }
                // FALLTHROUGH
            }
            PFSYNC_S_INS | PFSYNC_S_DEL | PFSYNC_S_DEAD => {}

            PFSYNC_S_IACK | PFSYNC_S_NONE => {
                if st.sync_state.get() == PFSYNC_S_IACK {
                    pfsync_q_del(s, st);
                }
                // FALLTHROUGH
                pfsync_q_ins(s, st, PFSYNC_S_UPD_C);
                st.sync_updates.set(0);
            }
            ss => panic(format_args!(
                "pfsync_update_state: state {:p} unexpected sync_state {}",
                ptr::from_ref(st),
                ss
            )),
        }

        if !sync && getuptime() - i64::from(st.pfsync_time.get()) < 2 {
            sync = true;
        }

        if sync {
            pfsync_slice_sched(s);
        }
        pfsync_slice_leave(sc, s);
    }
    smr_read_leave();
}

/// `pfsync_delete_state`: a state is being removed.
pub fn pfsync_delete_state(st: &'static PfState) {
    mutex_assert_unlocked(&st.mtx, "pfsync_delete_state");

    if st.state_flags.get() & PFSTATE_NOSYNC != 0 || st.sync_state.get() == PFSYNC_S_DEAD {
        return;
    }

    smr_read_enter();
    if let Some(sc) = pfsync_if() {
        let s = pfsync_slice_enter(sc, st);

        match st.sync_state.get() {
            PFSYNC_S_INS => {
                // let's pretend this never happened
                pfsync_q_del(s, st);
            }

            PFSYNC_S_UPD_C | PFSYNC_S_UPD | PFSYNC_S_IACK | PFSYNC_S_NONE => {
                if st.sync_state.get() != PFSYNC_S_NONE {
                    pfsync_q_del(s, st);
                }
                // FALLTHROUGH
                pfsync_q_ins(s, st, PFSYNC_S_DEL);
                st.sync_updates.set(0);
            }
            PFSYNC_S_DEL | PFSYNC_S_DEAD => {
                // XXX we should count this
            }
            ss => panic(format_args!(
                "pfsync_delete_state: state {:p} unexpected sync_state {}",
                ptr::from_ref(st),
                ss
            )),
        }

        pfsync_slice_leave(sc, s);
    }
    smr_read_leave();
}

/// `pfsync_clear_states`: tells the peers to clear the states of `creatorid` (on interface
/// `ifname`, or all of them when it is empty).
pub fn pfsync_clear_states(creatorid: u32, ifname: &[u8]) {
    smr_read_enter();
    let sc = pfsync_if();
    if let Some(sc) = sc {
        refcnt_take(&sc.sc_refs);
    }
    smr_read_leave();

    let Some(sc) = sc else {
        return;
    };

    const H: usize = size_of::<PfsyncSubhClr>();
    let hlen = size_of::<Ip>() + size_of::<PfsyncHeader>() + H;

    let mlen = MAX_LINKHDR.load(Ordering::Relaxed) as usize + hlen;

    'leave: {
        let Some(m) = m_gethdr(M_DONTWAIT, MT_DATA) else {
            // count error
            break 'leave;
        };

        if mlen > MHLEN {
            let _ = mclgetl(m, M_DONTWAIT, mlen as u32);
            if m.m_flags().get() & M_EXT == 0 {
                m_freem(m);
                break 'leave;
            }
        }

        m_align(m, H as i32);

        let mut h = PfsyncSubhClr {
            subh: PfsyncSubheader {
                action: PFSYNC_ACT_CLR,
                len: (size_of::<PfsyncClr>() >> 2) as u8,
                count: htons(1),
            },
            ..PfsyncSubhClr::default()
        };
        let mut name = [0u8; IFNAMSIZ];
        libkern::strlcpy(&mut name, ifname);
        h.clr.ifname = name;
        h.clr.creatorid = creatorid;
        // SAFETY: `m_align` placed `H` bytes of `m`'s data area at `mtod(m)`.
        unsafe { MbufWriter::new(m, H) }.put(0, &h);

        m.m_len().set(H as u32);
        m.m_pkthdr().len.set(H as i32);
        let Some(m) = pfsync_encap(sc, m) else {
            break 'leave;
        };

        pfsync_sendout(sc, m);
    }
    refcnt_rele_wake(&sc.sc_refs);
}

/// `pfsync_state_in_use`: whether a bulk send still points at `st`, which pf must not free.
pub fn pfsync_state_in_use(st: &PfState) -> bool {
    let mut rv = false;

    smr_read_enter();
    if let Some(sc) = pfsync_if() {
        // pfsync bulk sends run inside rw_enter_read(&pf_state_list.pfs_rwl), and this code
        // (pfsync_state_in_use) is only called from the purge code inside
        // rw_enter_write(&pf_state_list.pfs_rwl). therefore, those two sections are exclusive
        // so we can safely look at the bulk send pointers.
        // rw_assert_wrlock(&pf_state_list.pfs_rwl);
        let bs = &sc.sc_bulk_snd;
        if bs.snd_next.get().is_some_and(|n| ptr::eq(n, st))
            || bs.snd_tail.get().is_some_and(|t| ptr::eq(t, st))
        {
            rv = true;
        }
    }
    smr_read_leave();

    rv
}

/// `pfsync_defer`: holds the first packet of a new state back until the peer has the state
/// (or `PFSYNC_DEFER_NSEC` passed); `true` when `m` was taken.
pub fn pfsync_defer(st: &'static PfState, m: &'static Mbuf) -> bool {
    if st.state_flags.get() & PFSTATE_NOSYNC != 0 || m.m_flags().get() & (M_BCAST | M_MCAST) != 0 {
        return false;
    }

    // smr_read_enter(): the read section is implicit here (see the module's deviations).
    let Some(sc) = pfsync_if() else {
        return false;
    };
    if sc.sc_defer.get() == 0 {
        return false;
    }

    let Some(pd) = pf_pool_get::<PfsyncDeferral>(&PFSYNC_DEFERRALS_POOL, PR_NOWAIT) else {
        return false;
    };

    let s = pfsync_slice_enter(sc, st);
    s.s_stat_defer_add.set(s.s_stat_defer_add.get() + 1);

    pd.pd_st.set(Some(pf_state_ref(st)));
    pd.pd_m.set(Some(m));
    pd.pd_deadline.set(getnsecuptime() + PFSYNC_DEFER_NSEC);

    let pf = &m.m_pkthdr().pf;
    pf.flags.set(pf.flags.get() | PF_TAG_GENERATED);
    st.sync_defer.set(Some(pd));

    let sched = s.s_deferred.get();
    s.s_deferred.set(sched + 1);
    // SAFETY: a fresh pool item on no list; it stays allocated until `pfsync_defer_output`.
    unsafe { s.s_deferrals.insert_tail(pd) };

    if sched == 0 {
        let _ = timeout_add_nsec(&s.s_deferrals_tmo, PFSYNC_DEFER_NSEC);
    } else if sched >= PFSYNC_DEFER_LIMIT {
        s.s_stat_defer_overlimit
            .set(s.s_stat_defer_overlimit.get() + 1);
        let _ = timeout_del(&s.s_deferrals_tmo);
        let _ = task_add(s.softnet(), &s.s_deferrals_task);
    }

    pfsync_slice_sched(s);
    pfsync_slice_leave(sc, s);
    true
}

/// `pfsync_deferred`: the peer acknowledged `st`; its deferred packet goes out.
fn pfsync_deferred(sc: &'static PfsyncSoftc, st: &'static PfState) {
    let s = pfsync_slice_enter(sc, st);

    let pd = st.sync_defer.get();
    if let Some(pd) = pd {
        s.s_stat_defer_ack.set(s.s_stat_defer_ack.get() + 1);

        // SAFETY: a state's deferral is on its slice's list.
        unsafe { s.s_deferrals.remove(pd) };
        s.s_deferred.set(s.s_deferred.get() - 1);

        if let Some(st) = pd.pd_st.get() {
            st.sync_defer.set(None);
        }
    }
    pfsync_slice_leave(sc, s);

    if let Some(pd) = pd {
        pfsync_defer_output(pd);
    }
}

/// `pfsync_deferrals_tmo`: `s_deferrals_tmo`.
fn pfsync_deferrals_tmo(arg: *mut c_void) {
    let s = PfsyncSlice::of_arg(arg);

    if s.s_deferred.get() > 0 {
        let _ = task_add(s.softnet(), &s.s_deferrals_task);
    }
}

/// `pfsync_deferrals_task`: sends the deferred packets whose time is up (all of them past
/// `PFSYNC_DEFER_LIMIT`).
fn pfsync_deferrals_task(arg: *mut c_void) {
    let s = PfsyncSlice::of_arg(arg);
    let mut nsec: u64 = 0;
    let pds: TailqHead<PfsyncDeferrals> = TailqHead::new();

    let now = getnsecuptime();

    mtx_enter(&s.s_mtx);
    s.s_stat_defer_run.set(s.s_stat_defer_run.get() + 1); // maybe move this into the loop
    while let Some(pd) = s.s_deferrals.first() {
        if s.s_deferred.get() < PFSYNC_DEFER_LIMIT && now < pd.pd_deadline.get() {
            nsec = pd.pd_deadline.get() - now;
            break;
        }

        // SAFETY: `pd` is on the slice's list; it moves to the local one, which is emptied
        // below before it goes out of scope.
        unsafe { s.s_deferrals.remove(pd) };
        s.s_deferred.set(s.s_deferred.get() - 1);

        // detach the pd from the state. the pd still refers to the state though.
        if let Some(st) = pd.pd_st.get() {
            st.sync_defer.set(None);
        }

        // SAFETY: as above.
        unsafe { pds.insert_tail(pd) };
    }
    mtx_leave(&s.s_mtx);

    if nsec > 0 {
        // we were looking at a pd, but it wasn't old enough
        let _ = timeout_add_nsec(&s.s_deferrals_tmo, nsec);
    }

    if pds.is_empty() {
        return;
    }

    net_lock();
    while let Some(pd) = pds.first() {
        // SAFETY: on the local list.
        unsafe { pds.remove(pd) };

        pfsync_defer_output(pd_static(pd));
    }
    net_unlock();
}

/// `pfsync_defer_output`: sends a deferred packet as the state would have and gives the
/// deferral back.
fn pfsync_defer_output(pd: &'static PfsyncDeferral) {
    let Some(st) = pd.pd_st.get() else {
        return;
    };
    let af = st.key[PF_SK_WIRE].get().map_or(0, |k| k.af.get());

    if st.rt.get() == PF_ROUTETO {
        let mut pdesc = PfPdesc::new();
        let mut reason: u16 = 0;
        let Some(m) = pd.pd_m.get() else {
            return;
        };
        if pf_setup_pdesc(&mut pdesc, af, st.direction.get(), None, m, &mut reason) != PF_PASS {
            // As in C, the deferral is not given back.
            return;
        }
        match af {
            AF_INET => pf_route(&mut pdesc, st),
            #[cfg(feature = "inet6")]
            AF_INET6 => crate::net::pf::pf_route6(&mut pdesc, st),
            _ => unhandled_af(i32::from(af)),
        }
        pd.pd_m.set(pdesc.m);
    } else {
        match af {
            AF_INET => {
                if let Some(m) = pd.pd_m.get() {
                    let _ = ip_output(m, None, None, 0, None, None, 0);
                }
            }
            #[cfg(feature = "inet6")]
            AF_INET6 => {
                if let Some(m) = pd.pd_m.get() {
                    let _ = crate::netinet6::ip6_output::ip6_output(m, None, None, 0, None, None);
                }
            }
            _ => unhandled_af(i32::from(af)),
        }

        pd.pd_m.set(None);
    }

    pf_state_unref(Some(st));
    m_freem(pd.pd_m.get());
    pf_pool_put(&PFSYNC_DEFERRALS_POOL, pd);
}

/// The writer of a bulk send frame `m`, `m_align`ed for `space` bytes.
fn bulk_buf(m: &Mbuf, space: usize) -> MbufWriter<'_> {
    // SAFETY: the bulk frames are `m_align`ed for `space` bytes (`pfsync_bulk_mbuf`) and are
    // written by one bulk send function at a time, under `snd_lock`.
    unsafe { MbufWriter::new(m, space) }
}

/// `pfsync_bulk_snd_bus`: appends a BUS message with `status`, if it fits.
fn pfsync_bulk_snd_bus(m: &Mbuf, space: usize, endtime: u32, status: u8) -> bool {
    let len = m.m_len().get() as usize;
    let nlen = len + size_of::<PfsyncSubhBus>();
    if space < nlen {
        return false;
    }

    let h = PfsyncSubhBus {
        subh: PfsyncSubheader {
            action: PFSYNC_ACT_BUS,
            len: (size_of::<PfsyncBus>() >> 2) as u8,
            count: htons(1),
        },
        bus: PfsyncBus {
            creatorid: PF_STATUS.hostid.get(),
            endtime: htonl(endtime),
            status,
            _pad: [0; 3],
        },
    };
    bulk_buf(m, space).put(len, &h);

    m.m_len().set(nlen as u32);

    true
}

/// `pfsync_bulk_snd_states`: fills the frame with the next states of the bulk send, from
/// offset `len`; returns how many went in.
fn pfsync_bulk_snd_states(sc: &PfsyncSoftc, m: &Mbuf, space: usize, len: usize) -> u32 {
    let bs = &sc.sc_bulk_snd;
    let mut len = len;
    let mut count = 0;

    let mut st = bs.snd_next.get();

    while let Some(s) = st {
        let nlen = len + size_of::<PfsyncState>();
        if space < nlen {
            break;
        }

        let mut sp = PfsyncState::default();
        mtx_enter(&s.mtx);
        pf_state_export(&mut sp, s);
        mtx_leave(&s.mtx);
        bulk_buf(m, space).put(len, &sp);

        // commit
        count += 1;
        len = nlen;
        m.m_len().set(len as u32);

        if bs.snd_tail.get().is_some_and(|t| ptr::eq(t, s)) {
            if !pfsync_bulk_snd_bus(m, space, 0, PFSYNC_BUS_END) {
                // couldn't fit the BUS
                st = None;
                break;
            }

            // this BUS is done
            pfsync_dprintf(sc, format_args!("bulk send done (pfsync_bulk_snd_states)"));
            bs.snd_again.set(0); // XXX
            bs.snd_next.set(None);
            bs.snd_tail.set(None);
            return count;
        }

        st = TailqHead::<PfStateQueue>::next(s).map(state_static);
    }

    // there's still work to do
    bs.snd_next.set(st);
    let _ = timeout_add_msec(&bs.snd_tmo, PFSYNC_BULK_SND_IVAL_MS);

    count
}

/// `pfsync_bulk_snd_sub`: an UPD section of states.
fn pfsync_bulk_snd_sub(sc: &PfsyncSoftc, m: &Mbuf, space: usize) -> u32 {
    let len = m.m_len().get() as usize;
    let nlen = len + size_of::<PfsyncSubheader>();
    if nlen > space {
        return 0;
    }

    // pfsync_bulk_snd_states only updates m->m_len after filling in a state after the
    // offset we gave it.
    let count = pfsync_bulk_snd_states(sc, m, space, nlen);
    if count == 0 {
        return 0;
    }

    let subh = PfsyncSubheader {
        action: PFSYNC_ACT_UPD,
        len: (size_of::<PfsyncState>() >> 2) as u8,
        count: htons(count as u16),
    };
    bulk_buf(m, space).put(len, &subh);

    count
}

/// A bulk send frame: a header mbuf with a cluster for the interface's MTU, `m_align`ed for
/// `space` bytes and empty.
fn pfsync_bulk_mbuf(sc: &PfsyncSoftc, space: usize) -> Option<&'static Mbuf> {
    let m = m_gethdr(M_DONTWAIT, MT_DATA)?;

    let _ = mclgetl(
        m,
        M_DONTWAIT,
        MAX_LINKHDR.load(Ordering::Relaxed) as u32 + sc.sc_if.if_mtu.get(),
    );
    if m.m_flags().get() & M_EXT == 0 {
        // some error++
        m_freem(m); // drop
        return None;
    }

    m_align(m, space as i32);
    m.m_len().set(0);

    Some(m)
}

/// `pfsync_bulk_snd_start`: a peer asked for a bulk update: send it all our states.
fn pfsync_bulk_snd_start(sc: &PfsyncSoftc) {
    let space = sc.sc_if.if_mtu.get() as usize - (size_of::<Ip>() + size_of::<PfsyncHeader>());
    let bs = &sc.sc_bulk_snd;

    rw_enter_read(&PF_STATE_LIST.pfs_rwl);

    rw_enter_write(&bs.snd_lock);
    'leave: {
        if bs.snd_next.get().is_some() {
            bs.snd_again.set(1);
            break 'leave;
        }

        mtx_enter(&PF_STATE_LIST.pfs_mtx);
        bs.snd_next.set(PF_STATE_LIST.pfs_list.first());
        bs.snd_tail.set(PF_STATE_LIST.pfs_list.last());
        mtx_leave(&PF_STATE_LIST.pfs_mtx);

        let Some(m) = pfsync_bulk_mbuf(sc, space) else {
            break 'leave;
        };

        if bs.snd_tail.get().is_none() {
            pfsync_dprintf(sc, format_args!("bulk send empty (pfsync_bulk_snd_start)"));

            // list is empty
            if !pfsync_bulk_snd_bus(m, space, 0, PFSYNC_BUS_END) {
                panic(format_args!("pfsync_bulk_snd_start: mtu is too low"));
            }
        } else {
            pfsync_dprintf(sc, format_args!("bulk send start (pfsync_bulk_snd_start)"));

            // start a bulk update.
            if !pfsync_bulk_snd_bus(m, space, 0, PFSYNC_BUS_START) {
                panic(format_args!("pfsync_bulk_snd_start: mtu is too low"));
            }

            // fill it up with state updates.
            let _ = pfsync_bulk_snd_sub(sc, m, space);
        }

        // encap:
        m.m_pkthdr().len.set(m.m_len().get() as i32);
        let Some(m) = pfsync_encap(sc, m) else {
            break 'leave;
        };

        pfsync_sendout(sc, m);
    }
    rw_exit_write(&bs.snd_lock);

    rw_exit_read(&PF_STATE_LIST.pfs_rwl);
}

/// `pfsync_bulk_snd_tmo`: `snd_tmo`: the next frame of the bulk send.
fn pfsync_bulk_snd_tmo(arg: *mut c_void) {
    let sc = PfsyncSoftc::of_arg(arg);
    let space = sc.sc_if.if_mtu.get() as usize - (size_of::<Ip>() + size_of::<PfsyncHeader>());
    let bs = &sc.sc_bulk_snd;

    let Some(m) = pfsync_bulk_mbuf(sc, space) else {
        // some error++
        // retry later
        let _ = timeout_add_msec(&bs.snd_tmo, PFSYNC_BULK_SND_IVAL_MS);
        return;
    };

    rw_enter_read(&PF_STATE_LIST.pfs_rwl);
    rw_enter_write(&bs.snd_lock);

    if bs.snd_next.get().is_none() {
        // there was no space in the previous packet for a BUS END

        if !pfsync_bulk_snd_bus(m, space, 0, PFSYNC_BUS_END) {
            panic(format_args!("pfsync_bulk_snd_tmo: mtu is too low"));
        }

        // this bulk is done
        pfsync_dprintf(sc, format_args!("bulk send done (pfsync_bulk_snd_tmo)"));
        bs.snd_again.set(0); // XXX
        bs.snd_tail.set(None);
    } else {
        pfsync_dprintf(sc, format_args!("bulk send again (pfsync_bulk_snd_tmo)"));

        // fill it up with state updates.
        let _ = pfsync_bulk_snd_sub(sc, m, space);
    }

    m.m_pkthdr().len.set(m.m_len().get() as i32);
    let m = pfsync_encap(sc, m);

    rw_exit_write(&bs.snd_lock);
    rw_exit_read(&PF_STATE_LIST.pfs_rwl);

    if let Some(m) = m {
        net_lock();
        pfsync_sendout(sc, m);
        net_unlock();
    }
}

/// `pfsync_update_state_req`: a peer asked for `st`; queue its full state.
fn pfsync_update_state_req(sc: &'static PfsyncSoftc, st: &'static PfState) {
    let s = pfsync_slice_enter(sc, st);

    match st.sync_state.get() {
        PFSYNC_S_UPD_C | PFSYNC_S_IACK | PFSYNC_S_NONE => {
            if st.sync_state.get() != PFSYNC_S_NONE {
                pfsync_q_del(s, st);
            }
            // FALLTHROUGH
            pfsync_q_ins(s, st, PFSYNC_S_UPD);
        }

        PFSYNC_S_INS | PFSYNC_S_UPD | PFSYNC_S_DEL => {
            // we're already handling it
        }
        ss => panic(format_args!(
            "pfsync_update_state_req: state {:p} unexpected sync_state {}",
            ptr::from_ref(st),
            ss
        )),
    }

    pfsync_slice_sched(s);
    pfsync_slice_leave(sc, s);
}

/// `RPL_INCR`: what an outbound TDB's replay counter is bumped by for the peer.
const RPL_INCR: u64 = 16384;

/// `pfsync_out_tdb`: the TDB message. Called with the TDB's mutex held.
fn pfsync_out_tdb(tdb: &Tdb, buf: &mut [u8]) {
    mutex_assert_locked(&tdb.tdb_mtx, "pfsync_out_tdb");

    let mut ut = PfsyncTdb {
        spi: tdb.tdb_spi.get(),
        dst: *tdb.tdb_dst.get().as_bytes(),
        ..PfsyncTdb::default()
    };
    // When a failover happens, the master's rpl is probably above what we see here (we may
    // be up to a second late), so increase it a bit for outbound tdbs to manage most such
    // situations.
    //
    // For now, just add an offset that is likely to be larger than the number of packets we
    // can see in one second. The RFC just says the next packet must have a higher seq value.
    //
    // XXX What is a good algorithm for this? We could use a rate-determined increase, but to
    // know it, we would have to extend struct tdb.
    // XXX pt->rpl can wrap over MAXINT, but if so the real tdb will soon be replaced anyway.
    // For now, just don't handle this edge case.
    let incr = if tdb.tdb_flags.get() & TDBF_PFSYNC_RPL != 0 {
        RPL_INCR
    } else {
        0
    };
    ut.rpl = htobe64(tdb.tdb_rpl.get().wrapping_add(incr));
    ut.cur_bytes = htobe64(tdb.tdb_cur_bytes.get());
    ut.sproto = tdb.tdb_sproto.get();
    ut.rdomain = htons(tdb.tdb_rdomain.get() as u16);

    wire_put(buf, &ut);
}

/// `pfsync_slice_enter_tdb`: the slice of a TDB, locked.
fn pfsync_slice_enter_tdb(sc: &'static PfsyncSoftc, _t: &Tdb) -> &'static PfsyncSlice {
    // just use the first slice for all ipsec (for now) until it's more obvious what property
    // (eg, spi) we can distribute tdbs over slices with.
    let s = &sc.sc_slices[0];

    if !mtx_enter_try(&s.s_mtx) {
        mtx_enter(&s.s_mtx);
        s.s_stat_contended.set(s.s_stat_contended.get() + 1);
    }
    s.s_stat_locks.set(s.s_stat_locks.get() + 1);

    s
}

/// `pfsync_tdb_ins`: queues the TDB; a full frame goes out first.
fn pfsync_tdb_ins(s: &'static PfsyncSlice, tdb: &Tdb) {
    let mut nlen = size_of::<PfsyncTdb>();

    kassert!(s.s_len.get() >= PFSYNC_MINPKT);

    mutex_assert_locked(&s.s_mtx, "pfsync_tdb_ins");
    mutex_assert_unlocked(&tdb.tdb_mtx, "pfsync_tdb_ins");

    if s.s_tdb_q.is_empty() {
        nlen += size_of::<PfsyncSubheader>();
    }

    if s.s_len.get() + nlen > s.sc().sc_if.if_mtu.get() as usize {
        let m = pfsync_slice_write(s);
        if let Some(m) = m {
            s.s_stat_enqueue.set(s.s_stat_enqueue.get() + 1);
            if !mq_enqueue(&s.s_sendq, m) {
                let _ = task_add(s.softnet(), &s.s_send);
            }
        }

        nlen = size_of::<PfsyncSubheader>() + size_of::<PfsyncTdb>();
    }

    s.s_len.set(s.s_len.get() + nlen);
    // SAFETY: a TDB without `TDBF_PFSYNC` is on no slice's queue; `tdb_free` waits in
    // `pfsync_delete_tdb` until pfsync let go of it (`TDBF_PFSYNC` cleared), so it stays
    // valid while queued.
    unsafe { s.s_tdb_q.insert_tail(tdb) };
    tdb.tdb_updates.set(0);

    if !timeout_pending(&s.s_tmo) {
        let _ = timeout_add_sec(&s.s_tmo, 1);
    }
}

/// `pfsync_tdb_del`.
fn pfsync_tdb_del(s: &PfsyncSlice, tdb: &Tdb) {
    mutex_assert_locked(&s.s_mtx, "pfsync_tdb_del");
    mutex_assert_unlocked(&tdb.tdb_mtx, "pfsync_tdb_del");

    // SAFETY: the TDB has `TDBF_PFSYNC`, so it is on this (the first) slice's queue.
    unsafe { s.s_tdb_q.remove(tdb) };

    s.s_len.set(s.s_len.get() - size_of::<PfsyncTdb>());
    if s.s_tdb_q.is_empty() {
        s.s_len.set(s.s_len.get() - size_of::<PfsyncSubheader>());
    }
}

// the reference that pfsync has to a tdb is accounted for by the TDBF_PFSYNC flag, not by
// tdb_ref/tdb_unref. tdb_delete_tdb() is called after all other references to a tdb are
// dropped (with tdb_unref) as part of the tdb_free().
//
// tdb_free() needs to wait for pfsync to let go of the tdb though, which would be best
// handled by a reference count, but tdb_free needs the NET_LOCK which pfsync is already
// fighting with. instead use the TDBF_PFSYNC_SNAPPED flag to coordinate the pfsync
// write/drop with tdb_free.

/// `pfsync_update_tdb`: a TDB's replay counter moved (`output` for an outbound SA).
pub fn pfsync_update_tdb(tdb: &Tdb, output: bool) {
    mutex_assert_unlocked(&tdb.tdb_mtx, "pfsync_update_tdb");
    let _ = output;

    smr_read_enter();
    if let Some(sc) = pfsync_if() {
        let s = pfsync_slice_enter_tdb(sc, tdb);

        // TDBF_PFSYNC is only changed while the slice mtx is held
        if tdb.tdb_flags.get() & TDBF_PFSYNC == 0 {
            mtx_enter(&tdb.tdb_mtx);
            tdb.tdb_flags.set(tdb.tdb_flags.get() | TDBF_PFSYNC);
            mtx_leave(&tdb.tdb_mtx);

            pfsync_tdb_ins(s, tdb);
        } else {
            tdb.tdb_updates.set(tdb.tdb_updates.get() + 1);
            if tdb.tdb_updates.get() >= sc.sc_maxupdates.get() {
                pfsync_slice_sched(s);
            }
        }

        // XXX no sync timestamp on tdbs to check

        pfsync_slice_leave(sc, s);
    }
    smr_read_leave();
}

/// `pfsync_delete_tdb`: the TDB is being freed: take it off the queue, and wait for a
/// write or drop in progress to let go of it.
pub fn pfsync_delete_tdb(tdb: &Tdb) {
    mutex_assert_unlocked(&tdb.tdb_mtx, "pfsync_delete_tdb");

    smr_read_enter();
    if let Some(sc) = pfsync_if() {
        let s = pfsync_slice_enter_tdb(sc, tdb);

        // TDBF_PFSYNC is only changed while the slice mtx is held
        if tdb.tdb_flags.get() & TDBF_PFSYNC != 0 {
            pfsync_tdb_del(s, tdb);

            mtx_enter(&tdb.tdb_mtx);
            tdb.tdb_flags.set(tdb.tdb_flags.get() & !TDBF_PFSYNC);
            mtx_leave(&tdb.tdb_mtx);
        }

        pfsync_slice_leave(sc, s);
    }
    smr_read_leave();

    // handle pfsync_slice_drop being called from pfsync_down and the smr/slice access above
    // won't work.

    mtx_enter(&tdb.tdb_mtx);
    tdb.tdb_flags.set(tdb.tdb_flags.get() | TDBF_PFSYNC_SNAPPED); // like a thanos snap
    while tdb.tdb_flags.get() & TDBF_PFSYNC != 0 {
        let _ = msleep_nsec(
            ptr::from_ref(&tdb.tdb_updates),
            &tdb.tdb_mtx,
            PWAIT,
            "tdbfree",
            INFSLP,
        );
    }
    mtx_leave(&tdb.tdb_mtx);
}

/// `pfsync_in_skip`: a message type nothing reads.
fn pfsync_in_skip(_sc: &'static PfsyncSoftc, _buf: &[u8], _mlen: usize, _count: usize) {
    // nop
}

/// `pfsync_input`: applies a peer's frame (`m` from its IP header of `hlen` bytes, with TTL
/// `ttl`). Returns what is left of `m` for the caller to free.
fn pfsync_input(m: &'static Mbuf, ttl: u8, hlen: usize) -> Option<&'static Mbuf> {
    let mut m = m;

    pfsyncstat_inc(PfsyncCounters::PfsyncsIpackets);

    if PF_STATUS.running.get() == 0 {
        return Some(m);
    }

    // pfsyncif is only set if it is up and running correctly.
    // smr_read_enter(): the read section is implicit here (see the module's deviations).
    let Some(sc) = pfsync_if() else {
        return Some(m);
    };

    if sc.sc_sync_ifidx.get() != m.m_pkthdr().ph_ifidx.get() {
        pfsyncstat_inc(PfsyncCounters::PfsyncsBadif);
        return Some(m);
    }

    // verify that the IP TTL is 255.
    if ttl != PFSYNC_DFLTTL {
        pfsyncstat_inc(PfsyncCounters::PfsyncsBadttl);
        return Some(m);
    }

    m_adj(m, hlen as i32);

    let phlen = size_of::<PfsyncHeader>();
    if (m.m_pkthdr().len.get() as usize) < phlen {
        pfsyncstat_inc(PfsyncCounters::PfsyncsHdrops);
        return Some(m);
    }
    if (m.m_len().get() as usize) < phlen {
        m = m_pullup(m, phlen as i32)?;
    }

    // SAFETY: `m_pullup` (or the length check) made the header contiguous at `mtod(m)`.
    let ph: PfsyncHeader = wire_get(unsafe { mbuf_bytes(m, 0, phlen) });
    if ph.version != PFSYNC_VERSION {
        pfsyncstat_inc(PfsyncCounters::PfsyncsBadver);
        return Some(m);
    }

    let len = usize::from(ntohs(ph.len));
    if (m.m_pkthdr().len.get() as usize) < len {
        pfsyncstat_inc(PfsyncCounters::PfsyncsBadlen);
        return Some(m);
    }
    if m.m_pkthdr().len.get() as usize > len {
        m.m_pkthdr().len.set(len as i32);
    }

    // ok, it's serious now
    refcnt_take(&sc.sc_refs);
    // smr_read_leave(): implicit, as above.

    ifp_counters_pkt(
        &sc.sc_if,
        IfCounters::IfcIpackets,
        IfCounters::IfcIbytes,
        len as u64,
    );

    m_adj(m, phlen as i32);

    let subhlen = size_of::<PfsyncSubheader>();
    let rest = 'rele: {
        while m.m_pkthdr().len.get() as usize >= subhlen {
            if (m.m_len().get() as usize) < subhlen {
                match m_pullup(m, subhlen as i32) {
                    Some(n) => m = n,
                    None => break 'rele None,
                }
            }
            // SAFETY: as for the header above.
            let subh: PfsyncSubheader = wire_get(unsafe { mbuf_bytes(m, 0, subhlen) });

            let action = usize::from(subh.action);
            let mlen = usize::from(subh.len) << 2;
            let count = usize::from(ntohs(subh.count));

            let in_: PfsyncInFn = if action >= usize::from(PFSYNC_ACT_MAX)
                || action >= PFSYNC_ACTS.len()
                || mlen < PFSYNC_ACTS[action].len
            {
                // subheaders are always followed by at least one message, so if the peer is
                // new enough to tell us how big its messages are then we know enough to skip
                // them.
                if count == 0 || mlen == 0 {
                    pfsyncstat_inc(PfsyncCounters::PfsyncsBadact);
                    break 'rele Some(m);
                }

                pfsync_in_skip
            } else {
                PFSYNC_ACTS[action].in_.unwrap_or(pfsync_in_skip)
            };

            m_adj(m, subhlen as i32);
            let len = mlen * count;
            if len > m.m_pkthdr().len.get() as usize {
                pfsyncstat_inc(PfsyncCounters::PfsyncsBadlen);
                break 'rele Some(m);
            }
            if (m.m_len().get() as usize) < len {
                match m_pullup(m, len as i32) {
                    Some(n) => m = n,
                    None => break 'rele None,
                }
            }

            // SAFETY: `m_pullup` (or the length check) made the `len` bytes of the messages
            // contiguous at `mtod(m)`; the readers only read them.
            in_(sc, unsafe { mbuf_bytes(m, 0, len) }, mlen, count);
            m_adj(m, len as i32);
        }
        Some(m)
    };

    // rele:
    refcnt_rele_wake(&sc.sc_refs);
    rest
}

/// `pfsync_in_clr`: CLR: removes the states of a creator (on an interface).
fn pfsync_in_clr(_sc: &'static PfsyncSoftc, buf: &[u8], mlen: usize, count: usize) {
    rw_enter_read(&PF_STATE_LIST.pfs_rwl);

    // get a view of the state list
    mtx_enter(&PF_STATE_LIST.pfs_mtx);
    let head = PF_STATE_LIST.pfs_list.first().map(state_static);
    let tail = PF_STATE_LIST.pfs_list.last().map(state_static);
    mtx_leave(&PF_STATE_LIST.pfs_mtx);

    pf_lock();
    for i in 0..count {
        let clr: PfsyncClr = wire_get(&buf[i * mlen..]);

        let creatorid = clr.creatorid;
        let kif: Option<&'static PfiKif> = if clr.ifname[0] == 0 {
            None
        } else {
            match pfi_kif_find(&clr.ifname) {
                Some(kif) => Some(kif),
                None => continue,
            }
        };

        let mut st: Option<&'static PfState> = None;
        let mut next = head;

        pf_state_enter_write();
        while !opt_ptr_eq(st, tail) {
            st = next;
            let Some(s) = st else {
                break;
            };
            next = TailqHead::<PfStateQueue>::next(s).map(state_static);

            if creatorid != s.creatorid.get() {
                continue;
            }
            if kif.is_some() && !opt_ptr_eq(kif, s.kif.get()) {
                continue;
            }

            mtx_enter(&s.mtx);
            s.state_flags.set(s.state_flags.get() | PFSTATE_NOSYNC);
            mtx_leave(&s.mtx);
            pf_remove_state(s);
        }
        pf_state_exit_write();
    }
    pf_unlock();

    rw_exit_read(&PF_STATE_LIST.pfs_rwl);
}

/// Pointer equality of two optional references (`a == b` of two C pointers).
fn opt_ptr_eq<T>(a: Option<&T>, b: Option<&T>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => ptr::eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

/// `pfsync_in_ins`: INS: new states.
fn pfsync_in_ins(_sc: &'static PfsyncSoftc, buf: &[u8], mlen: usize, count: usize) {
    pf_lock();
    for i in 0..count {
        let sp: PfsyncState = wire_get(&buf[mlen * i..]);
        let af1 = sp.key[0].af;
        let af2 = sp.key[1].af;

        // check for invalid values
        if usize::from(sp.timeout) >= PFTM_MAX
            || sp.src.state > PF_TCPS_PROXY_DST
            || sp.dst.state > PF_TCPS_PROXY_DST
            || sp.direction > PF_OUT
            || ((af1 != 0 || af2 != 0)
                && ((af1 != AF_INET && af1 != AF_INET6) || (af2 != AF_INET && af2 != AF_INET6)))
            || (sp.af != AF_INET && sp.af != AF_INET6)
        {
            pfsyncstat_inc(PfsyncCounters::PfsyncsBadval);
            continue;
        }

        if pf_state_import(&sp, PFSYNC_SI_PFSYNC) == Err(Errno::ENOMEM) {
            // drop out, but process the rest of the actions
            break;
        }
    }
    pf_unlock();
}

/// The state with the id and creator of a message, with a reference.
fn pfsync_find_state(id: u64, creatorid: u32) -> Option<&'static PfState> {
    let id_key = PfStateCmp {
        id,
        creatorid,
        ..PfStateCmp::default()
    };

    pf_state_enter_read();
    let st = pf_find_state_byid(&id_key).map(pf_state_ref);
    pf_state_exit_read();

    st
}

/// `pfsync_in_iack`: INS_ACK: the peer has our state; its deferred packet may go.
fn pfsync_in_iack(sc: &'static PfsyncSoftc, buf: &[u8], mlen: usize, count: usize) {
    for i in 0..count {
        let ia: PfsyncInsAck = wire_get(&buf[mlen * i..]);

        let Some(st) = pfsync_find_state(ia.id, ia.creatorid) else {
            continue;
        };

        if st.sync_defer.get().is_some() {
            pfsync_deferred(sc, st);
        }

        pf_state_unref(Some(st));
    }
}

/// `pfsync_upd_tcp`: takes the peer's TCP peers unless ours are further along; returns how
/// many of the two were stale.
fn pfsync_upd_tcp(st: &PfState, src: &PfsyncStatePeer, dst: &PfsyncStatePeer) -> u32 {
    let mut sync = 0;

    // The state should never go backwards except for syn-proxy states. Neither should the
    // sequence window slide backwards.
    let (ss, ds) = (st.src.state.get(), src.state);
    if (ss > ds && (ss < PF_TCPS_PROXY_SRC || ds >= PF_TCPS_PROXY_SRC))
        || (ss == ds && seq_gt(st.src.seqlo.get(), ntohl(src.seqlo)))
    {
        sync += 1;
    } else {
        pf_state_peer_ntoh(src, &st.src);
    }

    let (ss, ds) = (st.dst.state.get(), dst.state);
    if ss > ds || (ss == ds && seq_gt(st.dst.seqlo.get(), ntohl(dst.seqlo))) {
        sync += 1;
    } else {
        pf_state_peer_ntoh(dst, &st.dst);
    }

    sync
}

/// `pfsync_in_updates`: applies a peer's update to `st`; ours goes back if it is newer.
fn pfsync_in_updates(
    sc: &'static PfsyncSoftc,
    st: &'static PfState,
    src: &PfsyncStatePeer,
    dst: &PfsyncStatePeer,
    timeout: u8,
) {
    let mut sscrub = None;
    let mut dscrub = None;

    'out: {
        if src.scrub.scrub_flag != 0 && st.src.scrub.get().is_none() {
            sscrub = pf_state_scrub_get();
            if sscrub.is_none() {
                // inc error?
                break 'out;
            }
        }
        if dst.scrub.scrub_flag != 0 && st.dst.scrub.get().is_none() {
            dscrub = pf_state_scrub_get();
            if dscrub.is_none() {
                // inc error?
                break 'out;
            }
        }

        if st.sync_defer.get().is_some() {
            pfsync_deferred(sc, st);
        }

        mtx_enter(&st.mtx);

        // attach the scrub memory if needed
        if sscrub.is_some() && st.src.scrub.get().is_none() {
            st.src.scrub.set(sscrub.take());
        }
        if dscrub.is_some() && st.dst.scrub.get().is_none() {
            st.dst.scrub.set(dscrub.take());
        }

        let sync = if st.key[PF_SK_WIRE]
            .get()
            .is_some_and(|k| i32::from(k.proto.get()) == IPPROTO_TCP)
        {
            pfsync_upd_tcp(st, src, dst)
        } else {
            let mut sync = 0;

            // Non-TCP protocol state machine always go forwards
            if st.src.state.get() > src.state {
                sync += 1;
            } else {
                pf_state_peer_ntoh(src, &st.src);
            }

            if st.dst.state.get() > dst.state {
                sync += 1;
            } else {
                pf_state_peer_ntoh(dst, &st.dst);
            }
            sync
        };

        st.pfsync_time.set(getuptime() as i32);
        if sync < 2 {
            st.expire.set(st.pfsync_time.get());
            if usize::from(st.timeout.get()) != PFTM_UNLINKED {
                st.timeout.set(timeout);
            }
        }

        mtx_leave(&st.mtx);

        if sync != 0 {
            pfsyncstat_inc(PfsyncCounters::PfsyncsStale);
            pfsync_update_state(st);
        }
    }

    // out:
    if let Some(s) = sscrub {
        pf_state_scrub_put(s);
    }
    if let Some(s) = dscrub {
        pf_state_scrub_put(s);
    }
}

/// `pfsync_in_upd`: UPD: full state updates; unknown states are inserted.
fn pfsync_in_upd(sc: &'static PfsyncSoftc, buf: &[u8], mlen: usize, count: usize) {
    for i in 0..count {
        let sp: PfsyncState = wire_get(&buf[mlen * i..]);

        // check for invalid values
        if usize::from(sp.timeout) >= PFTM_MAX
            || sp.src.state > PF_TCPS_PROXY_DST
            || sp.dst.state > PF_TCPS_PROXY_DST
        {
            pfsyncstat_inc(PfsyncCounters::PfsyncsBadval);
            continue;
        }

        let Some(st) = pfsync_find_state(sp.id, sp.creatorid) else {
            // insert the update
            pf_lock();
            if pf_state_import(&sp, PFSYNC_SI_PFSYNC).is_err() {
                pfsyncstat_inc(PfsyncCounters::PfsyncsBadstate);
            }
            pf_unlock();
            continue;
        };

        let (src, dst) = (sp.src, sp.dst);
        pfsync_in_updates(sc, st, &src, &dst, sp.timeout);

        pf_state_unref(Some(st));
    }
}

/// `pfsync_upd_req_init`: an empty frame for up to `count` update requests, built from the
/// end.
fn pfsync_upd_req_init(_sc: &PfsyncSoftc, count: usize) -> Option<&'static Mbuf> {
    let Some(m) = m_gethdr(M_DONTWAIT, MT_DATA) else {
        pfsyncstat_inc(PfsyncCounters::PfsyncsOnomem);
        return None;
    };

    let mlen = MAX_LINKHDR.load(Ordering::Relaxed) as usize
        + size_of::<Ip>()
        + size_of::<PfsyncHeader>()
        + size_of::<PfsyncSubheader>()
        + size_of::<PfsyncUpdReq>() * count;

    if mlen > MHLEN {
        let _ = mclgetl(m, M_DONTWAIT, mlen as u32);
        if m.m_flags().get() & M_EXT == 0 {
            m_freem(m);
            return None;
        }
    }

    m_align(m, 0);
    m.m_len().set(0);

    Some(m)
}

/// `pfsync_in_upd_c`: UPD_C: compressed updates; unknown states are asked for.
fn pfsync_in_upd_c(sc: &'static PfsyncSoftc, buf: &[u8], mlen: usize, count: usize) {
    let mut m: Option<&'static Mbuf> = None;
    let mut rcount: u16 = 0;

    for i in 0..count {
        let up: PfsyncUpdC = wire_get(&buf[mlen * i..]);

        // check for invalid values
        if usize::from(up.timeout) >= PFTM_MAX
            || up.src.state > PF_TCPS_PROXY_DST
            || up.dst.state > PF_TCPS_PROXY_DST
        {
            pfsyncstat_inc(PfsyncCounters::PfsyncsBadval);
            continue;
        }

        let Some(st) = pfsync_find_state(up.id, up.creatorid) else {
            // We don't have this state. Ask for it.
            let mm = match m {
                Some(mm) => mm,
                None => match pfsync_upd_req_init(sc, count) {
                    Some(mm) => mm,
                    None => {
                        pfsyncstat_inc(PfsyncCounters::PfsyncsOnomem);
                        continue;
                    }
                },
            };

            let Some(mm) = m_prepend(mm, size_of::<PfsyncUpdReq>() as i32, M_DONTWAIT) else {
                m = None;
                pfsyncstat_inc(PfsyncCounters::PfsyncsOnomem);
                continue;
            };
            m = Some(mm);

            let ur = PfsyncUpdReq {
                id: up.id,
                creatorid: up.creatorid,
            };
            // SAFETY: `m_prepend` made the request's bytes contiguous at `mtod(mm)`.
            unsafe { MbufWriter::new(mm, size_of::<PfsyncUpdReq>()) }.put(0, &ur);
            rcount += 1;

            continue;
        };

        let (src, dst) = (up.src, up.dst);
        pfsync_in_updates(sc, st, &src, &dst, up.timeout);

        pf_state_unref(Some(st));
    }

    if let Some(mm) = m {
        let Some(mm) = m_prepend(mm, size_of::<PfsyncSubheader>() as i32, M_DONTWAIT) else {
            pfsyncstat_inc(PfsyncCounters::PfsyncsOnomem);
            return;
        };

        let subh = PfsyncSubheader {
            action: PFSYNC_ACT_UPD_REQ,
            len: (size_of::<PfsyncUpdReq>() >> 2) as u8,
            count: htons(rcount),
        };
        // SAFETY: `m_prepend` made the subheader's bytes contiguous at `mtod(mm)`.
        unsafe { MbufWriter::new(mm, size_of::<PfsyncSubheader>()) }.put(0, &subh);

        let Some(mm) = pfsync_encap(sc, mm) else {
            pfsyncstat_inc(PfsyncCounters::PfsyncsOnomem);
            return;
        };

        pfsync_sendout(sc, mm);
    }
}

/// `pfsync_in_ureq`: UPD_REQ: the peer asks for states (all of them, a bulk update, for id
/// 0 and creator 0).
fn pfsync_in_ureq(sc: &'static PfsyncSoftc, buf: &[u8], mlen: usize, count: usize) {
    for i in 0..count {
        let ur: PfsyncUpdReq = wire_get(&buf[mlen * i..]);

        let id_key = PfStateCmp {
            id: ur.id,
            creatorid: ur.creatorid,
            ..PfStateCmp::default()
        };

        if id_key.id == 0 && id_key.creatorid == 0 {
            pfsync_bulk_snd_start(sc);
            continue;
        }

        pf_state_enter_read();
        let st = pf_find_state_byid(&id_key).filter(|st| {
            usize::from(st.timeout.get()) < PFTM_MAX && st.state_flags.get() & PFSTATE_NOSYNC == 0
        });
        let st = st.map(pf_state_ref);
        pf_state_exit_read();
        let Some(st) = st else {
            pfsyncstat_inc(PfsyncCounters::PfsyncsBadstate);
            continue;
        };

        pfsync_update_state_req(sc, st);

        pf_state_unref(Some(st));
    }
}

/// Removes the state of id `id` from `creatorid` without telling the peers.
fn pfsync_in_remove(id: u64, creatorid: u32) {
    let id_key = PfStateCmp {
        id,
        creatorid,
        ..PfStateCmp::default()
    };

    let Some(st) = pf_find_state_byid(&id_key) else {
        pfsyncstat_inc(PfsyncCounters::PfsyncsBadstate);
        return;
    };

    mtx_enter(&st.mtx);
    st.state_flags.set(st.state_flags.get() | PFSTATE_NOSYNC);
    mtx_leave(&st.mtx);
    pf_remove_state(st);
}

/// `pfsync_in_del`: DEL: full deletes.
fn pfsync_in_del(_sc: &'static PfsyncSoftc, buf: &[u8], mlen: usize, count: usize) {
    pf_lock();
    pf_state_enter_write();
    for i in 0..count {
        let sp: PfsyncState = wire_get(&buf[mlen * i..]);

        pfsync_in_remove(sp.id, sp.creatorid);
    }
    pf_state_exit_write();
    pf_unlock();
}

/// `pfsync_in_del_c`: DEL_C: compressed deletes.
fn pfsync_in_del_c(_sc: &'static PfsyncSoftc, buf: &[u8], mlen: usize, count: usize) {
    pf_lock();
    pf_state_enter_write();
    for i in 0..count {
        let sp: PfsyncDelC = wire_get(&buf[mlen * i..]);

        pfsync_in_remove(sp.id, sp.creatorid);
    }
    pf_state_exit_write();
    pf_unlock();
}

/// `pfsync_in_bus`: BUS: the peer's bulk update starts or ends.
fn pfsync_in_bus(sc: &'static PfsyncSoftc, buf: &[u8], _len: usize, _count: usize) {
    let bus: PfsyncBus = wire_get(buf);

    match bus.status {
        PFSYNC_BUS_START => pfsync_bulk_req_evt(sc, PfsyncBulkReqEvent::BusStart),

        PFSYNC_BUS_END => pfsync_bulk_req_evt(sc, PfsyncBulkReqEvent::BusEnd),

        _ => {}
    }
}

/// `pfsync_update_net_tdb`: updates an in-kernel tdb. Silently fail if no tdb is found.
fn pfsync_update_net_tdb(pt: &PfsyncTdb) {
    net_assert_locked("pfsync_update_net_tdb");

    let mut dst = SockaddrUnion::new();
    dst.as_bytes_mut().copy_from_slice(&pt.dst);

    // check for invalid values
    if ntohl(pt.spi) <= SPI_RESERVED_MAX
        || (dst.sa_family() != AF_INET && dst.sa_family() != AF_INET6)
    {
        // bad:
        crate::dpfprintf!(
            LOG_WARNING,
            "pfsync_insert: PFSYNC_ACT_TDB_UPD: invalid value"
        );
        pfsyncstat_inc(PfsyncCounters::PfsyncsBadstate);
        return;
    }

    if let Some(tdb) = gettdb(u32::from(ntohs(pt.rdomain)), pt.spi, &dst, pt.sproto) {
        let rpl = betoh64(pt.rpl);
        let cur_bytes = betoh64(pt.cur_bytes);

        // Neither replay nor byte counter should ever decrease.
        mtx_enter(&tdb.tdb_mtx);
        if rpl >= tdb.tdb_rpl.get() && cur_bytes >= tdb.tdb_cur_bytes.get() {
            tdb.tdb_rpl.set(rpl);
            tdb.tdb_cur_bytes.set(cur_bytes);
        }
        mtx_leave(&tdb.tdb_mtx);

        tdb_unref(Some(tdb));
    }
}

/// `pfsync_in_tdb`: TDB: replay counter updates.
fn pfsync_in_tdb(_sc: &'static PfsyncSoftc, buf: &[u8], len: usize, count: usize) {
    for i in 0..count {
        let tp: PfsyncTdb = wire_get(&buf[len * i..]);
        pfsync_update_net_tdb(&tp);
    }
}

/// `pfsync_input4`: the `inetsw[]` input of `IPPROTO_PFSYNC`.
pub fn pfsync_input4(
    mp: &mut Option<&'static Mbuf>,
    _offp: &mut i32,
    _proto: i32,
    _af: i32,
    _ns: Option<&Netstack>,
) -> i32 {
    let Some(m) = mp.take() else {
        return IPPROTO_DONE;
    };

    // SAFETY: `ip_input` hands the packet with its IP header contiguous at `mtod(m)`.
    let ip: Ip = unsafe { ptr::read_unaligned(mtod::<Ip>(m)) };

    let m = pfsync_input(m, ip.ip_ttl, usize::from(ip.ip_hl()) << 2);

    m_freem(m);
    *mp = None;

    IPPROTO_DONE
}

/// `pfsync_sysctl_pfsyncstat`: `net.inet.pfsync.stats`.
fn pfsync_sysctl_pfsyncstat(oldp: usize, oldlenp: &mut usize, newp: usize) -> Result<(), Errno> {
    const _: () = assert!(size_of::<Pfsyncstats>() == PFSYNCS_NCOUNTERS * size_of::<u64>());
    let mut bytes = [0u8; PFSYNCS_NCOUNTERS * size_of::<u64>()];
    for (i, c) in PFSYNCCOUNTERS.iter().enumerate() {
        bytes[i * 8..i * 8 + 8].copy_from_slice(&c.load(Ordering::Relaxed).to_ne_bytes());
    }
    sysctl_rdstruct(oldp, oldlenp, newp, &bytes)
}

/// `pfsync_sysctl`: `net.inet.pfsync`.
pub fn pfsync_sysctl(
    name: &[i32],
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    _newlen: usize,
) -> Result<(), Errno> {
    // All sysctl names at this level are terminal.
    if name.len() != 1 {
        return Err(Errno::ENOTDIR);
    }

    match name[0] {
        PFSYNCCTL_STATS => pfsync_sysctl_pfsyncstat(oldp, oldlenp, newp),
        _ => Err(Errno::ENOPROTOOPT),
    }
}

const _: () = {
    assert!(size_of::<PfsyncHeader>() == 20);
    assert!(size_of::<PfsyncSubheader>() == 4);
    assert!(size_of::<PfsyncClr>() == 20);
    assert!(size_of::<PfsyncInsAck>() == 12);
    assert!(size_of::<PfsyncUpdC>() == 84);
    assert!(size_of::<PfsyncUpdReq>() == 12);
    assert!(size_of::<PfsyncDelC>() == 12);
    assert!(size_of::<PfsyncBus>() == 12);
    assert!(size_of::<PfsyncTdb>() == 52);
    assert!(size_of::<PfsyncState>() == 264);
    assert!(size_of::<Pfsyncreq>() == 28);
    assert!(offset_of!(PfsyncSoftc, sc_if) == 0);
    // Every message is a whole number of dwords (`subh->len = len >> 2`).
    assert!(
        size_of::<PfsyncUpdC>().is_multiple_of(4) && size_of::<PfsyncState>().is_multiple_of(4)
    );
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for `pfsync(4)`: the wire layout, the constants against the C header, the
    // interface, the sync state hooks and a frame's round trip through `pfsync_input`.

    use super::*;
    use std::sync::MutexGuard;

    use crate::net::if_::tests::setup_net;
    use crate::net::pf::PF_STATE_PL;
    use crate::net::pf_ioctl::pfattach;
    use crate::reftest::{assert_complete, assert_defines};

    /// The network test lock with fresh memory and pf attached.
    fn setup() -> MutexGuard<'static, ()> {
        let guard = setup_net();
        crate::kern::kern_timeout::timeout_startup();
        pfattach(1);
        guard
    }

    /// A zeroed state from pf's pool.
    fn state() -> &'static PfState {
        pf_pool_get::<PfState>(&PF_STATE_PL, PR_NOWAIT).expect("a state")
    }

    #[test]
    fn wire_structures_have_the_c_layout() {
        // The sizes clang gives the __packed structures of <net/if_pfsync.h>.
        assert_eq!(PFSYNC_HDRLEN, 20);
        assert_eq!(size_of::<PfsyncSubheader>(), 4);
        assert_eq!(size_of::<PfsyncClr>(), 20);
        assert_eq!(size_of::<PfsyncInsAck>(), 12);
        assert_eq!(size_of::<PfsyncUpdC>(), 84);
        assert_eq!(offset_of!(PfsyncUpdC, creatorid), 72);
        assert_eq!(size_of::<PfsyncUpdReq>(), 12);
        assert_eq!(size_of::<PfsyncDelC>(), 12);
        assert_eq!(size_of::<PfsyncBus>(), 12);
        assert_eq!(offset_of!(PfsyncBus, status), 8);
        assert_eq!(size_of::<PfsyncTdb>(), 52);
        assert_eq!(offset_of!(PfsyncTdb, rpl), 32);
        assert_eq!(offset_of!(PfsyncTdb, rdomain), 50);
        assert_eq!(size_of::<Pfsyncreq>(), 28);
        assert_eq!(size_of::<Pfsyncstats>(), 16 * 8);
        assert_eq!(PFSYNC_MINPKT, 40);
    }

    #[test]
    fn messages_round_trip_through_the_wire_helpers() {
        let mut buf = [0u8; 16];
        let ia = PfsyncInsAck {
            id: 0x0102_0304_0506_0708,
            creatorid: 0xaabb_ccdd,
        };
        wire_put(&mut buf[1..], &ia);
        assert_eq!(buf[0], 0);
        let back: PfsyncInsAck = wire_get(&buf[1..]);
        assert_eq!(back, ia);
        assert_eq!(wire_bytes(&ia).len(), 12);
    }

    #[test]
    fn every_action_has_its_reader() {
        assert_eq!(PFSYNC_ACTS.len(), usize::from(PFSYNC_ACT_MAX));
        for a in [
            PFSYNC_ACT_OINS,
            PFSYNC_ACT_OUPD,
            PFSYNC_ACT_INS_F,
            PFSYNC_ACT_DEL_F,
            PFSYNC_ACT_OTDB,
            PFSYNC_ACT_EOF,
        ] {
            assert!(PFSYNC_ACTS[usize::from(a)].in_.is_none(), "{a}");
        }
        assert_eq!(PFSYNC_ACTS[usize::from(PFSYNC_ACT_UPD_C)].len, 84);
        assert_eq!(
            PFSYNC_ACTS[usize::from(PFSYNC_ACT_INS)].len,
            size_of::<PfsyncState>()
        );
        assert_eq!(
            PFSYNC_QS[usize::from(PFSYNC_S_DEL)].action,
            PFSYNC_ACT_DEL_C
        );
        assert_eq!(
            PFSYNC_QS[usize::from(PFSYNC_S_IACK)].action,
            PFSYNC_ACT_INS_ACK
        );
        assert_eq!(PFSYNC_ACTIONS[usize::from(PFSYNC_ACT_TDB)], "UPD TDB");
    }

    #[test]
    fn init_state_follows_the_flags() {
        let _g = setup();
        let st = state();
        pfsync_init_state(&st, None, None, PFSYNC_SI_PFSYNC);
        assert_eq!(st.sync_state.get(), PFSYNC_S_PFSYNC);

        st.state_flags.set(PFSTATE_ACK);
        pfsync_init_state(&st, None, None, PFSYNC_SI_PFSYNC);
        assert_eq!(st.sync_state.get(), PFSYNC_S_SYNC);
        assert_eq!(st.state_flags.get() & PFSTATE_ACK, 0);

        st.sync_state.set(PFSYNC_S_NONE);
        pfsync_init_state(&st, None, None, PFSYNC_SI_IOCTL);
        assert_eq!(st.sync_state.get(), PFSYNC_S_NONE);

        st.state_flags.set(PFSTATE_NOSYNC);
        pfsync_init_state(&st, None, None, 0);
        assert_eq!(st.sync_state.get(), PFSYNC_S_DEAD);
    }

    #[test]
    fn nothing_is_synced_without_an_interface() {
        let _g = setup();
        assert!(!pfsync_is_up());
        let st = state();
        assert!(!pfsync_state_in_use(&st));
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/net/if_pfsync.h");
        let act = assert_defines!(defs;
        PFSYNC_VERSION, PFSYNC_DFLTTL, PFSYNC_ACT_CLR, PFSYNC_ACT_OINS, PFSYNC_ACT_INS_ACK,
        PFSYNC_ACT_OUPD, PFSYNC_ACT_UPD_C, PFSYNC_ACT_UPD_REQ, PFSYNC_ACT_DEL, PFSYNC_ACT_DEL_C,
        PFSYNC_ACT_INS_F, PFSYNC_ACT_DEL_F, PFSYNC_ACT_BUS, PFSYNC_ACT_OTDB, PFSYNC_ACT_EOF,
        PFSYNC_ACT_INS, PFSYNC_ACT_UPD, PFSYNC_ACT_TDB, PFSYNC_ACT_MAX, PFSYNC_BUS_START,
        PFSYNC_BUS_END, PFSYNCCTL_STATS, PFSYNCCTL_MAXID, PFSYNC_S_IACK, PFSYNC_S_UPD_C,
        PFSYNC_S_DEL, PFSYNC_S_INS, PFSYNC_S_UPD, PFSYNC_S_COUNT, PFSYNC_S_NONE, PFSYNC_S_SYNC,
        PFSYNC_S_PFSYNC, PFSYNC_S_DEAD, PFSYNC_SI_IOCTL, PFSYNC_SI_CKSUM, PFSYNC_SI_ACK,
        PFSYNC_SI_PFSYNC);
        assert_complete(
            &defs,
            "PFSYNC",
            &[
                &act[..],
                &["PFSYNC_ACTIONS", "PFSYNC_HDRLEN", "PFSYNCCTL_NAMES"],
            ]
            .concat(),
        );
    }

    /// A state as a peer `creatorid` would send it: UDP 10.0.0.1:1000 -> 10.0.0.2:53, on `all`.
    fn peer_state(id: u64, creatorid: u32) -> PfsyncState {
        let mut sp = PfsyncState {
            id,
            creatorid,
            af: AF_INET,
            proto: crate::netinet::in_::IPPROTO_UDP as u8,
            direction: PF_OUT,
            timeout: crate::net::pfvar::PFTM_UDP_FIRST_PACKET as u8,
            expire: htonl(30),
            rule: u32::MAX,
            anchor: u32::MAX,
            ..PfsyncState::default()
        };
        let mut ifname = [0u8; IFNAMSIZ];
        ifname[..3].copy_from_slice(b"all");
        sp.ifname = ifname;
        let mut key = crate::net::pfvar::PfsyncStateKey::default();
        key.addr[0].set_addr32(0, htonl(0x0a00_0001));
        key.addr[1].set_addr32(0, htonl(0x0a00_0002));
        key.port = [htons(1000), htons(53)];
        key.af = AF_INET;
        sp.key = [key, key];
        sp
    }

    /// A pfsync softc that is up on the interface `ifp0`, without `pfsync_up` (whose bulk request
    /// would go through `ip_output`).
    fn running_softc(ifp0: &'static Ifnet) -> &'static PfsyncSoftc {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| pfsyncattach(1));
        assert_eq!(
            pfsync_clone_create(&PFSYNC_CLONER, 1),
            Err(Errno::ENXIO),
            "only unit 0"
        );
        assert_eq!(pfsync_clone_create(&PFSYNC_CLONER, 0), Ok(()));
        let ifp = if_unit(b"pfsync0").expect("pfsync0");
        if_put(ifp);
        assert_eq!(ifp.if_type.get(), IFT_PFSYNC);
        assert_eq!(ifp.if_mtu.get(), ETHERMTU as u32);
        let sc = PfsyncSoftc::of_ifp(ifp);
        assert_eq!(sc.sc_syncpeer.get().s_addr, INADDR_PFSYNC_GROUP);
        assert_eq!(sc.sc_maxupdates.get(), 128);

        sc.sc_sync_ifidx.set(ifp0.if_index.get());
        let mut ip = Ip::default();
        ip.set_ip_v(IPVERSION);
        ip.set_ip_hl(5);
        ip.ip_ttl = PFSYNC_DFLTTL;
        ip.ip_p = IPPROTO_PFSYNC as u8;
        sc.sc_template.set(ip);
        refcnt_init(&sc.sc_refs);
        ifp.if_flags.set(ifp.if_flags.get() | IFF_UP | IFF_RUNNING);
        PFSYNCIF.store(ptr::from_ref(sc).cast_mut(), Ordering::Release);
        sc
    }

    #[test]
    fn a_state_goes_out_as_ins_and_a_peer_del_c_removes_it() {
        let _g = setup();
        crate::net::if_::softnet_init();
        // The IP ids pfsync's frames take (ip_init does this at boot).
        crate::netinet::ip_id::ip_randomid_init();
        let ifp0 = crate::net::if_::tests::test_ifnet(b"tsync0");
        crate::net::if_::if_attach(ifp0);
        let sc = running_softc(ifp0);
        assert!(pfsync_is_up());

        // A state from DIOCADDSTATE: pfsync queues its insert.
        let sp = peer_state(0x1122_3344_5566_7788, htonl(0xc0ff_ee00));
        net_lock();
        pf_lock();
        assert_eq!(pf_state_import(&sp, PFSYNC_SI_IOCTL), Ok(()));
        pf_unlock();
        net_unlock();
        let st = pfsync_find_state(sp.id, sp.creatorid).expect("imported");
        assert_eq!(st.sync_state.get(), PFSYNC_S_INS);
        let s = sc
            .sc_slices
            .iter()
            .find(|s| !s.s_qs[usize::from(PFSYNC_S_INS)].is_empty())
            .expect("a slice holds the insert");
        let flen = PFSYNC_MINPKT + size_of::<PfsyncSubheader>() + size_of::<PfsyncState>();
        assert_eq!(s.s_len.get(), flen);

        // The frame: IP (protocol 240, TTL 255), the pfsync header, one INS.
        mtx_enter(&s.s_mtx);
        let m = pfsync_slice_write(s).expect("a frame");
        mtx_leave(&s.s_mtx);
        assert_eq!(s.s_len.get(), PFSYNC_MINPKT);
        assert_eq!(st.sync_state.get(), PFSYNC_S_NONE);
        let mut f = std::vec![0u8; flen];
        crate::kern::uipc_mbuf::m_copydata(m, 0, &mut f);
        assert_eq!(m.m_pkthdr().len.get() as usize, flen);
        assert_eq!((f[9], f[8]), (IPPROTO_PFSYNC as u8, PFSYNC_DFLTTL));
        assert_eq!(u16::from_be_bytes([f[2], f[3]]) as usize, flen);
        let ph: PfsyncHeader = wire_get(&f[20..]);
        assert_eq!(ph.version, PFSYNC_VERSION);
        assert_eq!(usize::from(ntohs(ph.len)), flen - 20);
        let subh: PfsyncSubheader = wire_get(&f[40..]);
        assert_eq!((subh.action, ntohs(subh.count)), (PFSYNC_ACT_INS, 1));
        assert_eq!(usize::from(subh.len) << 2, size_of::<PfsyncState>());
        let out: PfsyncState = wire_get(&f[44..]);
        assert_eq!((out.id, out.creatorid), (sp.id, sp.creatorid));
        m_freem(m);

        // The peer deletes it: a DEL_C frame on the sync interface.
        let mut b = std::vec::Vec::new();
        let mut ip = Ip::default();
        ip.set_ip_v(IPVERSION);
        ip.set_ip_hl(5);
        ip.ip_ttl = PFSYNC_DFLTTL;
        ip.ip_p = IPPROTO_PFSYNC as u8;
        b.extend_from_slice(&ip_bytes(&ip));
        let ph = PfsyncHeader {
            version: PFSYNC_VERSION,
            len: htons((PFSYNC_HDRLEN + 4 + 12) as u16),
            ..PfsyncHeader::default()
        };
        b.extend_from_slice(wire_bytes(&ph));
        let subh = PfsyncSubheader {
            action: PFSYNC_ACT_DEL_C,
            len: 3,
            count: htons(1),
        };
        b.extend_from_slice(wire_bytes(&subh));
        let del = PfsyncDelC {
            id: sp.id,
            creatorid: sp.creatorid,
        };
        b.extend_from_slice(wire_bytes(&del));
        let m = crate::net::if_::tests::test_packet(&b);
        m.m_pkthdr().ph_ifidx.set(ifp0.if_index.get());

        PF_STATUS.running.set(1);
        let mut mp = Some(m);
        let mut off = 20;
        net_lock();
        assert_eq!(
            pfsync_input4(&mut mp, &mut off, IPPROTO_PFSYNC, 2, None),
            IPPROTO_DONE
        );
        net_unlock();
        assert!(mp.is_none());
        assert!(pfsync_find_state(sp.id, sp.creatorid).is_none(), "removed");
        assert_eq!(
            PFSYNCCOUNTERS[PfsyncCounters::PfsyncsBadstate as usize].load(Ordering::Relaxed),
            0
        );
        pf_state_unref(Some(st));

        // A frame from another interface is not applied.
        let m = crate::net::if_::tests::test_packet(&b);
        m.m_pkthdr().ph_ifidx.set(ifp0.if_index.get() + 7);
        let before = PFSYNCCOUNTERS[PfsyncCounters::PfsyncsBadif as usize].load(Ordering::Relaxed);
        let mut mp = Some(m);
        let _ = pfsync_input4(&mut mp, &mut off, IPPROTO_PFSYNC, 2, None);
        assert_eq!(
            PFSYNCCOUNTERS[PfsyncCounters::PfsyncsBadif as usize].load(Ordering::Relaxed),
            before + 1
        );

        // net.inet.pfsync.stats is the counters.
        let mut len = 0;
        assert_eq!(pfsync_sysctl(&[PFSYNCCTL_STATS], 0, &mut len, 0, 0), Ok(()));
        assert_eq!(len, size_of::<Pfsyncstats>());
        assert_eq!(
            pfsync_sysctl(&[9], 0, &mut len, 0, 0),
            Err(Errno::ENOPROTOOPT)
        );
        assert_eq!(
            pfsync_sysctl(&[1, 1], 0, &mut len, 0, 0),
            Err(Errno::ENOTDIR)
        );

        PF_STATUS.running.set(0);
        PFSYNCIF.store(ptr::null_mut(), Ordering::Release);
    }
}
/* </TESTS> */
