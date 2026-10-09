/* <LICENSES> */
/*
 * Copyright (c) 2010 Reyk Floeter <reyk@vantronix.net>
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
 * Copyright (c) 2010 Reyk Floeter <reyk@vantronix.net>
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
//! The encapsulating interface, `enc(4)`: `<net/if_enc.h>` and `net/if_enc.c`. `enc0` is
//! made at boot (`encattach`, a pseudo-device of GENERIC: option IPSEC needs it); more are
//! cloned. Each routing domain has a default enc interface, which IPsec traffic is accounted
//! to and which gives routes to a `PF_KEY` destination their interface address.
//!
//! Upstream: sys/net/if_enc.h @ 3ce1f3f79392
//! Upstream: sys/net/if_enc.c @ 3ce1f3f79392
//!
//! Status: `ported` (M9c).
//!
//! ## Deviations
//! - `struct enc_softc` is `#[repr(C)]` with the `struct ifnet` first, `malloc(M_DEVBUF)`'d
//!   as in C; `if_softc` points back at it, and [`enc_softc`] casts back after checking
//!   `if_type` is `IFT_ENC`.
//! - `enc_ifps` (by routing domain) and `enc_allifps` (by unit), arrays grown with
//!   `mallocarray(M_DEVBUF)` in C, are `Vec`s of `Option<&'static Ifnet>` in a `StaticCell`,
//!   under the net lock; `enc_max_rdomain`/`enc_max_unit` are their lengths minus one.
//! - `enc_output` and `enc_ioctl` are `unsafe fn`s, the signatures of `if_output` and
//!   `if_ioctl` (`net/if_var.rs`).

use alloc::vec::Vec;
use core::cell::Cell;
use core::mem::size_of;
use core::ptr::{self, NonNull};

use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_synch::{refcnt_init_trace, refcnt_rele};
use crate::kern::subr_prf::{panic, snprintf};
use crate::kern::uipc_mbuf::m_freem;
use crate::net::bpf::{DLT_ENC, bpfattach};
use crate::net::if_::{
    IFF_RUNNING, IFF_UP, IFXF_CLONED, Ifreq, LINK_STATE_UNKNOWN, LINK_STATE_UP, if_addgroup,
    if_alloc_sadl, if_attach, if_clone_attach, if_detach,
};
use crate::net::if_dl::sdltosa;
use crate::net::if_types::IFT_ENC;
use crate::net::if_var::{IfClone, Ifaddr, Ifnet};
use crate::net::route::Rtentry;
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_DEVBUF, M_NOWAIT, M_ZERO};
use crate::sys::mbuf::Mbuf;
use crate::sys::refcnt::DT_REFCNT_IDX_IFADDR;
use crate::sys::socket::{RT_TABLEID_MAX, Sockaddr};
use crate::sys::sockio::{SIOCSIFADDR, SIOCSIFDSTADDR, SIOCSIFFLAGS, SIOCSIFRDOMAIN};
use crate::sys::systm::{net_assert_locked, net_lock, net_unlock};
use libkern::staticcell::StaticCell;

/// `ENCMTU`: XXX should be bigger, maybe `LOMTU`.
pub const ENCMTU: u32 = 1536;
/// `ENC_HDRLEN`.
pub const ENC_HDRLEN: usize = 12;

/// `struct enchdr`: the header `bpf(4)` sees in front of a packet on `enc(4)`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Enchdr {
    /// `af`, network order.
    pub af: u32,
    /// `spi`.
    pub spi: u32,
    /// `flags`: similar to mbuf `m_flags`.
    pub flags: u32,
}

impl Enchdr {
    /// The structure's bytes as the C lays it out (`(char *)&hdr`, `ENC_HDRLEN` bytes).
    pub fn to_bytes(&self) -> [u8; ENC_HDRLEN] {
        let mut b = [0u8; ENC_HDRLEN];
        b[0..4].copy_from_slice(&self.af.to_ne_bytes());
        b[4..8].copy_from_slice(&self.spi.to_ne_bytes());
        b[8..12].copy_from_slice(&self.flags.to_ne_bytes());
        b
    }
}

/// `struct enc_softc`.
#[repr(C)]
pub struct EncSoftc {
    /// `sc_if`: virtual interface.
    pub sc_if: Ifnet,
    /// `sc_unit`.
    pub sc_unit: Cell<u32>,
    /// `sc_ifa`: needed to attach rtentry.
    pub sc_ifa: Ifaddr,
}

// SAFETY: the members change under the net lock, as in C.
unsafe impl Sync for EncSoftc {}

/// `ENC_MAX_UNITS`: XXX n per rdomain.
const ENC_MAX_UNITS: i32 = 4096;

/// `enc_ifps` (rdomain-mapped enc ifs) and `enc_allifps` (unit-mapped enc ifs), under the
/// net lock.
struct EncIfs {
    /// `enc_ifps`.
    by_rdomain: Vec<Option<&'static Ifnet>>,
    /// `enc_allifps`.
    by_unit: Vec<Option<&'static Ifnet>>,
}

static ENC_IFS: StaticCell<EncIfs> = StaticCell::new(EncIfs {
    by_rdomain: Vec::new(),
    by_unit: Vec::new(),
});

/// The enc interface tables; the net lock is held.
fn enc_ifs() -> &'static mut EncIfs {
    // SAFETY: the net lock serialises every access, and no caller keeps the reference past
    // the function that took it.
    unsafe { ENC_IFS.get_mut() }
}

/// `enc_cloner`.
pub static ENC_CLONER: IfClone = IfClone::new(b"enc", enc_clone_create, Some(enc_clone_destroy));

/// `encattach`: creates `enc0` and registers the cloner.
pub fn encattach(_count: i32) {
    // Create enc0 by default
    let _ = enc_clone_create(&ENC_CLONER, 0);

    // SAFETY: `encattach` runs once, from `main`'s pseudo-device attach.
    unsafe { if_clone_attach(&ENC_CLONER) };
}

/// The softc of an `enc(4)` interface (`ifp->if_softc`).
pub fn enc_softc(ifp: &Ifnet) -> &'static EncSoftc {
    if ifp.if_type.get() != IFT_ENC {
        panic(format_args!("enc_softc: not an enc interface"));
    }
    // SAFETY: an `IFT_ENC` interface is the first member of the `EncSoftc` that
    // `enc_clone_create` allocated and pointed `if_softc` at; it lives until
    // `enc_clone_destroy`.
    unsafe { &*ifp.if_softc.get().cast::<EncSoftc>() }
}

/// `enc_clone_create`: creates `enc<unit>`.
pub fn enc_clone_create(ifc: &'static IfClone, unit: i32) -> Result<(), Errno> {
    if unit > ENC_MAX_UNITS {
        return Err(Errno::EINVAL);
    }

    let Some(mem) = malloc(size_of::<EncSoftc>(), M_DEVBUF, M_NOWAIT | M_ZERO) else {
        return Err(Errno::ENOBUFS);
    };
    // SAFETY: a zero-filled block of the softc's size, aligned for it (malloc's chunks are
    // aligned to their size); the all-zero `Ifnet` and `Ifaddr` are valid. It lives until
    // `enc_clone_destroy`.
    let sc: &'static EncSoftc = unsafe { &*mem.as_ptr().cast::<EncSoftc>() };

    sc.sc_unit.set(unit as u32);

    let ifp = &sc.sc_if;
    ifp.if_softc.set(mem.as_ptr().cast());
    ifp.if_type.set(IFT_ENC);
    ifp.if_xflags.set(IFXF_CLONED);
    ifp.if_output.set(Some(enc_output));
    ifp.if_ioctl.set(Some(enc_ioctl));
    ifp.if_hdrlen.set(ENC_HDRLEN as u8);

    let mut xname = [0u8; 16];
    let _ = snprintf(
        &mut xname,
        format_args!("{}{}", crate::kern::subr_prf::Str(ifc.ifc_name), unit),
    );
    ifp.if_xname.set(xname);

    if_attach(ifp);
    if unit == 0 {
        let _ = if_addgroup(ifp, ifc.ifc_name);
    }
    // enc(4) does not have a link-layer address but rtrequest() wants an ifa for every route
    // entry. So let's setup a fake and empty ifa of type AF_LINK for this purpose.
    if_alloc_sadl(ifp);
    refcnt_init_trace(&sc.sc_ifa.ifa_refcnt, DT_REFCNT_IDX_IFADDR);
    sc.sc_ifa.ifa_ifp.set(Some(ifp));
    sc.sc_ifa.ifa_addr.set(sdltosa(ifp.if_sadl.get()));
    sc.sc_ifa.ifa_netmask.set(ptr::null_mut());

    bpfattach(&ifp.if_bpf, ifp, DLT_ENC, ENC_HDRLEN as u32);
    net_lock();
    if let Err(error) = enc_setif(ifp, 0) {
        net_unlock();
        if_detach(ifp);
        free(mem, M_DEVBUF, size_of::<EncSoftc>());
        return Err(error);
    }

    let t = enc_ifs();
    let u = unit as usize;
    if t.by_unit.len() <= u {
        if t.by_unit.try_reserve(u + 1 - t.by_unit.len()).is_err() {
            net_unlock();
            return Err(Errno::ENOBUFS);
        }
        t.by_unit.resize(u + 1, None);
    }
    t.by_unit[u] = Some(ifp);
    net_unlock();

    Ok(())
}

/// `enc_clone_destroy`: destroys a cloned enc interface; `enc0` stays.
pub fn enc_clone_destroy(ifp: &'static Ifnet) -> Result<(), Errno> {
    let sc = enc_softc(ifp);

    // Protect users from removing enc0
    if sc.sc_unit.get() == 0 {
        return Err(Errno::EPERM);
    }

    net_lock();
    enc_ifs().by_unit[sc.sc_unit.get() as usize] = None;
    enc_unsetif(ifp);
    net_unlock();

    if_detach(ifp);
    if !refcnt_rele(&sc.sc_ifa.ifa_refcnt) {
        panic(format_args!(
            "enc_clone_destroy: ifa refcnt has {} refs",
            sc.sc_ifa
                .ifa_refcnt
                .r_refs
                .load(core::sync::atomic::Ordering::Relaxed)
        ));
    }
    free(NonNull::from(sc).cast(), M_DEVBUF, size_of::<EncSoftc>());

    Ok(())
}

/// `enc_output`: drop packet.
///
/// # Safety
///
/// As for `if_output` (`IfOutputFn`).
pub unsafe fn enc_output(
    _ifp: &'static Ifnet,
    m: &'static Mbuf,
    _sa: *const Sockaddr,
    _rt: Option<&'static Rtentry>,
) -> Result<(), Errno> {
    m_freem(m); // drop packet
    Err(Errno::EAFNOSUPPORT)
}

/// `enc_ioctl`.
///
/// # Safety
///
/// As for `if_ioctl` (`IfIoctlFn`): `data` is the kernel copy of the command's structure.
pub unsafe fn enc_ioctl(ifp: &'static Ifnet, cmd: u64, data: *mut u8) -> Result<(), Errno> {
    match cmd {
        SIOCSIFADDR | SIOCSIFDSTADDR | SIOCSIFFLAGS => {
            if ifp.if_flags.get() & IFF_UP != 0 {
                ifp.if_flags.set(ifp.if_flags.get() | IFF_RUNNING);
            } else {
                ifp.if_flags.set(ifp.if_flags.get() & !IFF_RUNNING);
            }
        }
        SIOCSIFRDOMAIN => {
            // SAFETY: `SIOCSIFRDOMAIN` carries a `struct ifreq` (the caller's contract).
            let ifr = unsafe { &*data.cast::<Ifreq>() };
            enc_setif(ifp, ifr.ifr_rdomainid() as u32)?;
            // FALLTHROUGH
            return Err(Errno::ENOTTY);
        }
        _ => return Err(Errno::ENOTTY),
    }

    Ok(())
}

/// `enc_getif`: the enc interface `unit` (if it is in `rdomain`), or the default one of
/// `rdomain` for unit 0.
pub fn enc_getif(rdomain: u32, unit: u32) -> Option<&'static Ifnet> {
    net_assert_locked("enc_getif");
    let t = enc_ifs();

    // Check if the caller wants to get a non-default enc interface
    if unit > 0 {
        let ifp = (*t.by_unit.get(unit as usize)?)?;
        if ifp.if_rdomain.get() != rdomain {
            return None;
        }
        return Some(ifp);
    }

    // Otherwise return the default enc interface for this rdomain
    if t.by_rdomain.is_empty() || rdomain > RT_TABLEID_MAX {
        return None;
    }
    *t.by_rdomain.get(rdomain as usize)?
}

/// `enc_getifa`: the fake link address of the enc interface `enc_getif` finds.
pub fn enc_getifa(rdomain: u32, unit: u32) -> Option<&'static Ifaddr> {
    let ifp = enc_getif(rdomain, unit)?;

    Some(&enc_softc(ifp).sc_ifa)
}

/// `enc_setif`: makes `ifp` the default enc interface of `rdomain` if it has none.
pub fn enc_setif(ifp: &'static Ifnet, rdomain: u32) -> Result<(), Errno> {
    net_assert_locked("enc_setif");

    enc_unsetif(ifp);

    // There can only be one default encif per rdomain - Don't overwrite the existing enc
    // iface that is stored for this rdomain, so only the first enc interface that was added
    // for this rdomain becomes the default.
    if enc_getif(rdomain, 0).is_some() {
        return Ok(());
    }

    if rdomain > RT_TABLEID_MAX {
        return Err(Errno::EINVAL);
    }

    let t = enc_ifs();
    let r = rdomain as usize;
    if t.by_rdomain.len() <= r {
        if t.by_rdomain
            .try_reserve(r + 1 - t.by_rdomain.len())
            .is_err()
        {
            return Err(Errno::ENOBUFS);
        }
        t.by_rdomain.resize(r + 1, None);
    }

    t.by_rdomain[r] = Some(ifp);

    // Indicate that this interface is the rdomain default
    ifp.if_link_state.set(LINK_STATE_UP);

    Ok(())
}

/// `enc_unsetif`: if `ifp` is its routing domain's default, the next enc interface of the
/// domain takes over.
pub fn enc_unsetif(ifp: &'static Ifnet) {
    let rdomain = ifp.if_rdomain.get();

    match enc_getif(rdomain, 0) {
        Some(oifp) if ptr::eq(oifp, ifp) => {}
        _ => return,
    }

    // Clear slot for this rdomain
    let t = enc_ifs();
    t.by_rdomain[rdomain as usize] = None;
    ifp.if_link_state.set(LINK_STATE_UNKNOWN);

    // Now find the next available encif to be the default interface for this rdomain.
    for nifp in t.by_unit.iter().flatten() {
        if ptr::eq(*nifp, ifp) || nifp.if_rdomain.get() != rdomain {
            continue;
        }

        t.by_rdomain[rdomain as usize] = Some(*nifp);
        nifp.if_link_state.set(LINK_STATE_UP);
        break;
    }
}

/// Host tests: forgets the enc interfaces of the previous test (its memory is gone).
#[cfg(test)]
pub(crate) fn enc_reset() {
    let t = enc_ifs();
    t.by_rdomain.clear();
    t.by_unit.clear();
}

// LP64 sizes of the C structures.
const _: () = assert!(size_of::<Enchdr>() == ENC_HDRLEN);
/* </CODE> */
