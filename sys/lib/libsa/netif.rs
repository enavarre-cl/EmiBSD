/* <LICENSES> */
/*
 * Copyright (c) 1993 Adam Glass
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
 *	This product includes software developed by Adam Glass.
 * 4. The name of the Author may not be used to endorse or promote products
 *    derived from this software without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY Adam Glass ``AS IS'' AND
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
 */
/* </LICENSES> */

/* <CODE> */
//! `netif.h` and `netif.c`: the generic network interface layer. A program supplies its
//! interface drivers; `netif_open()` picks the best interface for a hint, probes and
//! attaches it to a free socket, and the Ethernet layer sends and receives frames through
//! `netif_put()` and `netif_get()`.
//!
//! Upstream: sys/lib/libsa/netif.c @ 3ce1f3f79392, sys/lib/libsa/netif.h @ 3ce1f3f79392
//!
//! A program registers its drivers as `SaConf::netif_drivers` (the C's `netif_drivers[]`
//! and `n_netif_drivers`, "machdep", which the linker resolves), each a `'static`
//! [`NetifDriver`] whose `netif_ifs` are its units.
//!
//! ## Deviations
//! - The machine-dependent hint `netif_open()` passes to the drivers (`void *machdep_hint`)
//!   is a byte string: every boot program in the tree passes a device name (efiboot's
//!   `netif_open("efinet")`, luna88k's `devname`).
//! - The driver routines take references and slices: `netif_get` receives into the whole
//!   slice and `netif_put` sends the whole slice; both return `Result<usize, Errno>` where
//!   the C returns the count or -1 (the `Err`'s value is not looked at: the C's callers see
//!   -1 and keep `errno`).
//! - `netif_select()` returns the chosen interface by value where the C returns a pointer to
//!   its `static struct netif best_if`; each socket keeps its own copy (`IoDesc::io_netif`).
//! - `struct netif_dif`'s `dif_stats` is a `&'static NetifStats` whose counters are atomics
//!   and `dif_used` an `AtomicU64` (the C's `u_long` bitmask): the drivers are `static`s the
//!   layer updates in place. `netif_nifs` is the length of the `netif_ifs` slice.
//! - `sockets[SOPEN_MAX]` is the [`StaticCell`] [`SOCKETS`]; `socktodesc` and `netif_close`
//!   take a `usize` socket and fail with `EBADF` for one that is not open, where the C
//!   dereferences a NULL `io_netif`; `netif_get`/`netif_put` on a socket without an
//!   interface fail with `EBADF` too.
//! - `netif_debug` and the `NETIF_DEBUG`/`PARANOID` blocks are not ported: no efiboot
//!   Makefile defines them (the routines are `fn`s, never NULL).

use core::ffi::c_void;
use core::sync::atomic::{AtomicI32, AtomicU64, Ordering};

use libkern::staticcell::StaticCell;

use crate::dev::set_errno;
use crate::hdr::types::Time;
use crate::iodesc::IoDesc;
use crate::saerrno::Errno;
use crate::stand::{SOPEN_MAX, sa_conf};

/// `struct netif_driver`: a network interface driver.
#[derive(Debug)]
pub struct NetifDriver {
    /// `netif_bname`: the driver's name.
    pub netif_bname: &'static str,
    /// `netif_match`: how well the interface `nif` suits the hint (0: not at all; the best
    /// wins).
    pub netif_match: fn(nif: &mut Netif, machdep_hint: &[u8]) -> i32,
    /// `netif_probe`: 0 if the interface is there.
    pub netif_probe: fn(nif: &mut Netif, machdep_hint: &[u8]) -> i32,
    /// `netif_init`: bring the interface of `desc.io_netif` up and fill `desc` (`myea`,
    /// `myip`, `xid`...).
    pub netif_init: fn(desc: &mut IoDesc, machdep_hint: &[u8]),
    /// `netif_get`: receive a frame into `pkt`, waiting at most `timo` seconds; its length.
    pub netif_get: fn(desc: &mut IoDesc, pkt: &mut [u8], timo: Time) -> Result<usize, Errno>,
    /// `netif_put`: send the frame `pkt`; the length sent.
    pub netif_put: fn(desc: &mut IoDesc, pkt: &[u8]) -> Result<usize, Errno>,
    /// `netif_end`: shut the interface down.
    pub netif_end: fn(nif: &mut Netif),
    /// `netif_ifs`, `netif_nifs`: the driver's units.
    pub netif_ifs: &'static [NetifDif],
}

/// `struct netif_dif`: one unit of a driver.
#[derive(Debug)]
pub struct NetifDif {
    /// `dif_unit`.
    pub dif_unit: i32,
    /// `dif_nsel`: the number of selections (media...) the unit has.
    pub dif_nsel: i32,
    /// `dif_stats`: the unit's counters, zeroed when it is attached.
    pub dif_stats: &'static NetifStats,
    /// `dif_private`: the driver's own data.
    pub dif_private: *mut c_void,
    /// `dif_used`: the selections in use, one bit each (used internally by the netif
    /// layer).
    pub dif_used: AtomicU64,
}

impl NetifDif {
    /// A unit, none of its selections in use (the C's `{ unit, nsel, &stats, private, 0 }`).
    pub const fn new(
        dif_unit: i32,
        dif_nsel: i32,
        dif_stats: &'static NetifStats,
        dif_private: *mut c_void,
    ) -> Self {
        Self {
            dif_unit,
            dif_nsel,
            dif_stats,
            dif_private,
            dif_used: AtomicU64::new(0),
        }
    }
}

// SAFETY: the netif layer never dereferences `dif_private`; it belongs to the driver that
// set it, in a single-threaded standalone program. Every other member is immutable or
// atomic.
unsafe impl Sync for NetifDif {}

/// `struct netif_stats`: a unit's counters.
#[derive(Debug, Default)]
pub struct NetifStats {
    /// `collisions`.
    pub collisions: AtomicI32,
    /// `collision_error`.
    pub collision_error: AtomicI32,
    /// `missed`.
    pub missed: AtomicI32,
    /// `sent`.
    pub sent: AtomicI32,
    /// `received`.
    pub received: AtomicI32,
    /// `deferred`.
    pub deferred: AtomicI32,
    /// `overflow`.
    pub overflow: AtomicI32,
}

impl NetifStats {
    /// Zeroed counters.
    pub const fn new() -> Self {
        Self {
            collisions: AtomicI32::new(0),
            collision_error: AtomicI32::new(0),
            missed: AtomicI32::new(0),
            sent: AtomicI32::new(0),
            received: AtomicI32::new(0),
            deferred: AtomicI32::new(0),
            overflow: AtomicI32::new(0),
        }
    }

    /// `bzero(stats, sizeof(struct netif_stats))`.
    pub fn clear(&self) {
        for c in [
            &self.collisions,
            &self.collision_error,
            &self.missed,
            &self.sent,
            &self.received,
            &self.deferred,
            &self.overflow,
        ] {
            c.store(0, Ordering::Relaxed);
        }
    }
}

/// `struct netif`: an interface: a driver's unit and selection.
#[derive(Clone, Copy, Debug)]
pub struct Netif {
    /// `nif_driver`.
    pub nif_driver: &'static NetifDriver,
    /// `nif_unit`: the index of the unit in `netif_ifs`.
    pub nif_unit: i32,
    /// `nif_sel`: the selection.
    pub nif_sel: i32,
    /// `nif_devdata`: the driver's data for this interface.
    pub nif_devdata: *mut c_void,
}

impl Netif {
    /// `drv->netif_ifs[nif->nif_unit]`.
    fn dif(&self) -> Option<&'static NetifDif> {
        usize::try_from(self.nif_unit)
            .ok()
            .and_then(|u| self.nif_driver.netif_ifs.get(u))
    }
}

// SAFETY: `nif_devdata` is only dereferenced by the driver that set it, in a
// single-threaded standalone program; the rest is a shared `'static` driver and integers.
unsafe impl Send for Netif {}

/// `sockets[SOPEN_MAX]`: the network sockets.
pub static SOCKETS: StaticCell<[IoDesc; SOPEN_MAX]> =
    StaticCell::new([const { IoDesc::new() }; SOPEN_MAX]);

/// `netif_init()`: initialize the generic network interface layer: no selection of any
/// unit is in use.
pub fn netif_init() {
    for drv in sa_conf().netif_drivers {
        for dif in drv.netif_ifs {
            dif.dif_used.store(0, Ordering::Relaxed);
        }
    }
}

/// `netif_match(nif, machdep_hint)`.
fn netif_match(nif: &mut Netif, machdep_hint: &[u8]) -> i32 {
    (nif.nif_driver.netif_match)(nif, machdep_hint)
}

/// The bit of selection `s` in `dif_used`.
fn sel_bit(s: i32) -> u64 {
    u32::try_from(s)
        .ok()
        .and_then(|s| 1u64.checked_shl(s))
        .unwrap_or(0)
}

/// `netif_select(machdep_hint)`: the unused interface that matches the hint best, now
/// marked used; `None` if none matches.
pub fn netif_select(machdep_hint: &[u8]) -> Option<Netif> {
    let mut best_val = 0;
    let mut best_if: Option<Netif> = None;

    for &drv in sa_conf().netif_drivers {
        for (u, dif) in drv.netif_ifs.iter().enumerate() {
            for s in 0..dif.dif_nsel {
                if dif.dif_used.load(Ordering::Relaxed) & sel_bit(s) != 0 {
                    continue;
                }

                let mut cur_if = Netif {
                    nif_driver: drv,
                    nif_unit: u as i32,
                    nif_sel: s,
                    nif_devdata: core::ptr::null_mut(),
                };
                let val = netif_match(&mut cur_if, machdep_hint);
                if val > best_val {
                    best_val = val;
                    best_if = Some(cur_if);
                }
            }
        }
    }

    let best = best_if?;
    if let Some(dif) = best.dif() {
        dif.dif_used
            .fetch_or(sel_bit(best.nif_sel), Ordering::Relaxed);
    }
    Some(best)
}

/// `netif_probe(nif, machdep_hint)`: the driver's probe; 0 if the interface is there.
pub fn netif_probe(nif: &mut Netif, machdep_hint: &[u8]) -> i32 {
    (nif.nif_driver.netif_probe)(nif, machdep_hint)
}

/// `netif_attach(nif, desc, machdep_hint)`: give `desc` the interface and let the driver
/// initialize both.
pub fn netif_attach(nif: Netif, desc: &mut IoDesc, machdep_hint: &[u8]) {
    let drv = nif.nif_driver;
    desc.io_netif = Some(nif);
    (drv.netif_init)(desc, machdep_hint);
    if let Some(dif) = nif.dif() {
        dif.dif_stats.clear();
    }
}

/// `netif_detach(nif)`: the driver's end.
pub fn netif_detach(nif: &mut Netif) {
    (nif.nif_driver.netif_end)(nif);
}

/// `netif_get(desc, pkt, timo)`: receive a frame into `pkt`; its length.
pub fn netif_get(desc: &mut IoDesc, pkt: &mut [u8], timo: Time) -> Result<usize, Errno> {
    let drv = desc.io_netif.ok_or(Errno::EBADF)?.nif_driver;
    (drv.netif_get)(desc, pkt, timo)
}

/// `netif_put(desc, pkt)`: send the frame `pkt`; the length sent.
pub fn netif_put(desc: &mut IoDesc, pkt: &[u8]) -> Result<usize, Errno> {
    let drv = desc.io_netif.ok_or(Errno::EBADF)?.nif_driver;
    (drv.netif_put)(desc, pkt)
}

/// `socktodesc(sock)`: the I/O descriptor of a socket.
///
/// # Safety
///
/// No other reference to that socket may be live while the returned one is: the network
/// code takes it at an entry point (`tftp_open`, `tftp_read`...) and drops it before
/// returning, in a single-threaded standalone program.
pub unsafe fn socktodesc(sock: usize) -> Result<&'static mut IoDesc, Errno> {
    if sock >= SOPEN_MAX {
        set_errno(Errno::EBADF);
        return Err(Errno::EBADF);
    }
    // SAFETY: the caller excludes every other reference to this socket (the contract); the
    // elements of the array are disjoint.
    let sockets = unsafe { &mut *SOCKETS.as_ptr() };
    Ok(&mut sockets[sock])
}

/// `netif_open(machdep_hint)`: attach the interface that best matches the hint to a free
/// socket; the socket.
pub fn netif_open(machdep_hint: &[u8]) -> Result<usize, Errno> {
    // find a free socket
    // SAFETY: shared read of the table; no reference into it is live (the network code's
    // references live within its entry points, which do not call `netif_open`).
    let free = unsafe { SOCKETS.get() }
        .iter()
        .position(|s| s.io_netif.is_none());
    let Some(fd) = free else {
        set_errno(Errno::EMFILE);
        return Err(Errno::EMFILE);
    };
    // SAFETY: as above; this is the one reference to the socket until we return.
    let s = unsafe { socktodesc(fd) }?;

    *s = IoDesc::new();
    netif_init();
    let Some(mut nif) = netif_select(machdep_hint) else {
        crate::exit::panic(format_args!("netboot: no interfaces left untried"));
    };
    if netif_probe(&mut nif, machdep_hint) != 0 {
        crate::printf!(
            "netboot: couldn't probe {}{}\n",
            nif.nif_driver.netif_bname,
            nif.nif_unit
        );
        set_errno(Errno::EINVAL);
        return Err(Errno::EINVAL);
    }
    netif_attach(nif, s, machdep_hint);

    Ok(fd)
}

/// `netif_close(sock)`: detach the socket's interface and free the socket.
pub fn netif_close(sock: usize) -> Result<(), Errno> {
    // SAFETY: entry point; the one reference to the socket until we return.
    let s = unsafe { socktodesc(sock) }?;
    let Some(mut nif) = s.io_netif else {
        set_errno(Errno::EBADF);
        return Err(Errno::EBADF);
    };
    netif_detach(&mut nif);
    s.io_netif = None;

    Ok(())
}
/* </CODE> */
