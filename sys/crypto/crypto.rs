/*	$OpenBSD: crypto.c,v 1.92 2021/10/24 14:50:42 tobhe Exp $	*/
/* <LICENSES> */
/*
 * The author of this code is Angelos D. Keromytis (angelos@cis.upenn.edu)
 *
 * This code was written by Angelos D. Keromytis in Athens, Greece, in
 * February 2000. Network Security Technologies Inc. (NSTI) kindly
 * supported the development of this code.
 *
 * Copyright (c) 2000, 2001 Angelos D. Keromytis
 *
 * Permission to use, copy, and modify this software with or without fee
 * is hereby granted, provided that this entire notice is included in
 * all source code copies of any software which is or includes a copy or
 * modification of this software.
 *
 * THIS SOFTWARE IS BEING PROVIDED "AS IS", WITHOUT ANY EXPRESS OR
 * IMPLIED WARRANTY. IN PARTICULAR, NONE OF THE AUTHORS MAKES ANY
 * REPRESENTATION OR WARRANTY OF ANY KIND CONCERNING THE
 * MERCHANTABILITY OF THIS SOFTWARE OR ITS FITNESS FOR ANY PARTICULAR
 * PURPOSE.
 */
/* </LICENSES> */

//! The kernel crypto framework (`crypto(9)`): the table of drivers, the choice of the driver
//! for a session ([`crypto_newsession`]), the dispatch of a request to the driver of its
//! session ([`crypto_invoke`]), and the allocation of requests ([`crypto_getreq`]). The only
//! driver in this tree is the software one (`cryptosoft.rs`).
//!
//! Upstream: sys/crypto/crypto.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `crypto_drivers`/`crypto_drivers_num` are one `Vec<Cryptocap>` (`crypto_drivers_num` is
//!   its length; "NULL until the first driver" is "empty"), in a `StaticCell` that the
//!   functions reach through a closure (`with_drivers`) so no borrow of it outlives a call into
//!   a driver. It is protected as in the C: by the kernel lock (asserted on entry) and
//!   `splvm`.
//! - The growth and zeroing of the table (`mallocarray` with `M_NOWAIT`) are `try_reserve` and
//!   `resize_with`; memory comes from the global allocator (`M_TEMP`) where the C charges
//!   `M_CRYPTO_DATA`.
//! - `crypto_newsession` returns the session id (`Result<u64, Errno>`) instead of filling
//!   `*sid`; `crypto_get_driverid` returns `Result<u32, Errno>` (`ENOMEM`, also for a full table)
//!   for the id or -1.
//! - `cryptop_pool` is not ported: a request is a value ([`Cryptop`]) that
//!   [`crypto_getreq`] returns and [`crypto_freereq`] drops, its descriptors in a `Vec` whose
//!   allocation failure is the `NULL` of `pool_get`/`mallocarray` with `PR_NOWAIT`.
//!   `crypto_init`, which only made the pool, therefore does nothing.
//! - The "migrate" of `crypto_invoke` (a session whose driver went away is made again on
//!   another from the descriptors' own initialisation data) chains the descriptors'
//!   `CRD_INI`s on the stack, last first, where the C links them in place. It leaves out the
//!   C's write of `NULL` into the `cri_next` of the element after the last descriptor (one
//!   past the array when it is full).
//! - `ERESTART` from a driver is `Err(Errno::ERESTART)`.

use alloc::vec::Vec;

use libkern::StaticCell;

use super::cryptodev::{
    CRYPTO_ALGORITHM_MAX, CRYPTO_DRIVERS_INITIAL, CRYPTO_DRIVERS_MAX, CRYPTOCAP_F_CLEANUP,
    CRYPTOCAP_F_SOFTWARE, Cryptocap, Cryptodesc, Cryptoini, Cryptop,
};
use crate::machine::intr::{splvm, splx};
use crate::sys::errno::Errno;
use crate::sys::systm::kernel_assert_locked;

/// `crypto_drivers`: array allocated by driver; [A] driver data and session count [K].
#[allow(non_upper_case_globals)] // the C name
pub static crypto_drivers: StaticCell<Vec<Cryptocap>> = StaticCell::new(Vec::new());

/// Runs `f` on the driver table. The table is only touched with the kernel lock held and at
/// `splvm`, and `f` never calls back into this module or into a driver.
fn with_drivers<R>(f: impl FnOnce(&mut Vec<Cryptocap>) -> R) -> R {
    // SAFETY: serialized by the kernel lock and splvm (the C's [K]); `f` is the only user of
    // the table while it runs, as none of its callers pass a closure that reaches the table or
    // calls a driver.
    f(unsafe { crypto_drivers.get_mut() })
}

/// `crypto_drivers_num`: the size of the driver array.
pub fn crypto_drivers_num() -> usize {
    with_drivers(|d| d.len())
}

/// Whether `cap` supports every algorithm of the chain `cri`.
fn supports_all(cap: &Cryptocap, cri: &Cryptoini<'_>) -> bool {
    let mut cr = Some(cri);
    while let Some(c) = cr {
        let supported = usize::try_from(c.cri_alg)
            .ok()
            .and_then(|a| cap.cc_alg.get(a))
            .is_some_and(|f| *f != 0);
        if !supported {
            return false;
        }
        cr = c.cri_next;
    }
    true
}

/// `crypto_newsession`: create a new session. The session id is the driver's index in the
/// high word and the driver's own session number in the low word. `hard` asks for hardware
/// drivers only.
pub fn crypto_newsession(cri: &Cryptoini<'_>, hard: i32) -> Result<u64, Errno> {
    let s = splvm();
    let r = crypto_newsession_locked(cri, hard);
    splx(s);
    r
}

fn crypto_newsession_locked(cri: &Cryptoini<'_>, hard: i32) -> Result<u64, Errno> {
    let num = crypto_drivers_num();
    let mut hid2: Option<usize> = None;
    let mut turn = 0;

    if num == 0 {
        return Err(Errno::EINVAL);
    }

    kernel_assert_locked();

    // The algorithm we use here is pretty stupid; just use the first driver that supports all
    // the algorithms we need. Do a double-pass over all the drivers, ignoring software ones at
    // first, to deal with cases of drivers that register after the software one(s) --- e.g.,
    // PCMCIA crypto cards.
    //
    // XXX We need more smarts here (in real life too, but that's another story altogether).
    loop {
        with_drivers(|drivers| {
            for hid in 0..num {
                let cpc = &drivers[hid];

                // If it's not initialized or has remaining sessions referencing it, skip.
                if cpc.cc_newsession.is_none() || (cpc.cc_flags & CRYPTOCAP_F_CLEANUP) != 0 {
                    continue;
                }

                if (cpc.cc_flags & CRYPTOCAP_F_SOFTWARE) != 0 {
                    // First round of search, ignore software drivers.
                    if turn == 0 {
                        continue;
                    }
                } else if turn == 1 {
                    // !CRYPTOCAP_F_SOFTWARE: second round of search, only software.
                    continue;
                }

                // See if all the algorithms are supported. If even one algorithm is not
                // supported, keep searching.
                if !supports_all(cpc, cri) {
                    continue;
                }

                // If we had a previous match, see how it compares to this one. Keep
                // "remembering" whichever is the best of the two.
                match hid2 {
                    Some(h2) => {
                        // Compare session numbers, pick the one with the lowest.
                        // XXX Need better metrics, this will XXX just do un-weighted
                        // round-robin.
                        if drivers[hid].cc_sessions <= drivers[h2].cc_sessions {
                            hid2 = Some(hid);
                        }
                    }
                    // Remember this one, for future comparisons.
                    None => hid2 = Some(hid),
                }
            }
        });

        // If we found something worth remembering, leave. The side-effect is that we will
        // always prefer a hardware driver over the software one.
        if hid2.is_some() {
            break;
        }

        turn += 1;

        // If we only want hardware drivers, don't do second pass.
        if !(turn <= 2 && hard == 0) {
            break;
        }
    }

    // Can't do everything in one session.
    //
    // XXX Fix this. We need to inject a "virtual" session XXX layer right about here.
    let Some(hid) = hid2 else {
        return Err(Errno::EINVAL);
    };

    // Call the driver initialization routine. Pass the driver ID.
    let newsession = with_drivers(|d| d[hid].cc_newsession);
    let mut lid = hid as u32;
    match newsession {
        Some(f) => f(&mut lid, cri)?,
        None => return Err(Errno::EINVAL),
    }
    let sid = (u64::from(hid as u32) << 32) | u64::from(lid);
    with_drivers(|d| d[hid].cc_sessions += 1);
    Ok(sid)
}

/// `crypto_freesession`: delete an existing session (or a reserved session on an unregistered
/// driver).
pub fn crypto_freesession(sid: u64) -> Result<(), Errno> {
    let s = splvm();
    let r = crypto_freesession_locked(sid);
    splx(s);
    r
}

fn crypto_freesession_locked(sid: u64) -> Result<(), Errno> {
    let num = crypto_drivers_num();

    if num == 0 {
        return Err(Errno::EINVAL);
    }

    // Determine two IDs.
    let hid = ((sid >> 32) & 0xffffffff) as usize;

    if hid >= num {
        return Err(Errno::ENOENT);
    }

    kernel_assert_locked();

    let freesession = with_drivers(|d| {
        if d[hid].cc_sessions != 0 {
            d[hid].cc_sessions -= 1;
        }
        d[hid].cc_freesession
    });

    // Call the driver cleanup routine, if available.
    let err = match freesession {
        Some(f) => f(sid),
        None => Ok(()),
    };

    // If this was the last session of a driver marked as invalid, make the entry available
    // for reuse.
    with_drivers(|d| {
        if (d[hid].cc_flags & CRYPTOCAP_F_CLEANUP) != 0 && d[hid].cc_sessions == 0 {
            d[hid] = Cryptocap::default();
        }
    });

    err
}

/// `crypto_get_driverid`: find an empty slot (called from attach routines).
pub fn crypto_get_driverid(flags: u8) -> Result<u32, Errno> {
    let s = splvm();
    let r = crypto_get_driverid_locked(flags);
    splx(s);
    r
}

fn crypto_get_driverid_locked(flags: u8) -> Result<u32, Errno> {
    kernel_assert_locked();

    with_drivers(|drivers| {
        if drivers.is_empty() {
            if drivers.try_reserve_exact(CRYPTO_DRIVERS_INITIAL).is_err() {
                return Err(Errno::ENOMEM);
            }
            drivers.resize_with(CRYPTO_DRIVERS_INITIAL, Cryptocap::default);
        }

        for (i, cap) in drivers.iter_mut().enumerate() {
            if cap.cc_process.is_none()
                && (cap.cc_flags & CRYPTOCAP_F_CLEANUP) == 0
                && cap.cc_sessions == 0
            {
                cap.cc_sessions = 1; // Mark
                cap.cc_flags = flags;
                return Ok(i as u32);
            }
        }

        // Out of entries, allocate some more.
        let num = drivers.len();
        if num >= CRYPTO_DRIVERS_MAX {
            return Err(Errno::ENOMEM);
        }
        if drivers.try_reserve_exact(num).is_err() {
            return Err(Errno::ENOMEM);
        }
        drivers.resize_with(2 * num, Cryptocap::default);

        drivers[num].cc_sessions = 1; // Mark
        drivers[num].cc_flags = flags;
        Ok(num as u32)
    })
}

/// `crypto_register`: register a crypto driver. It should be called once for each algorithm
/// supported by the driver.
pub fn crypto_register(
    driverid: u32,
    alg: &[i32; CRYPTO_ALGORITHM_MAX + 1],
    newses: super::cryptodev::CcNewsession,
    freeses: super::cryptodev::CcFreesession,
    process: super::cryptodev::CcProcess,
) -> Result<(), Errno> {
    if driverid as usize >= crypto_drivers_num() {
        return Err(Errno::EINVAL);
    }

    // called from attach routines
    kernel_assert_locked();

    let s = splvm();
    with_drivers(|drivers| {
        let cap = &mut drivers[driverid as usize];
        // XXX Do some performance testing to determine placing. We probably need an
        // auxiliary data structure that describes relative performances.
        cap.cc_alg = *alg;

        cap.cc_newsession = Some(newses);
        cap.cc_process = Some(process);
        cap.cc_freesession = Some(freeses);
        cap.cc_sessions = 0; // Unmark
    });
    splx(s);

    Ok(())
}

/// `crypto_unregister`: unregister a crypto driver. If there are pending sessions using it,
/// leave enough information around so that subsequent calls using those sessions will
/// correctly detect the driver being unregistered and reroute the request. `alg` is an
/// algorithm, or `CRYPTO_ALGORITHM_MAX + 1` for all of them.
pub fn crypto_unregister(driverid: u32, alg: i32) -> Result<(), Errno> {
    let s = splvm();
    let r = crypto_unregister_locked(driverid, alg);
    splx(s);
    r
}

fn crypto_unregister_locked(driverid: u32, alg: i32) -> Result<(), Errno> {
    let all = CRYPTO_ALGORITHM_MAX as i32 + 1;
    let mut i = all;

    // may be called from detach routines, but not used
    kernel_assert_locked();

    with_drivers(|drivers| {
        // Sanity checks.
        if driverid as usize >= drivers.len() || drivers.is_empty() || alg <= 0 || alg > all {
            return Err(Errno::EINVAL);
        }
        let cap = &mut drivers[driverid as usize];

        if alg != all {
            if cap.cc_alg[alg as usize] == 0 {
                return Err(Errno::EINVAL);
            }
            cap.cc_alg[alg as usize] = 0;

            // Was this the last algorithm ?
            i = 1;
            while i <= CRYPTO_ALGORITHM_MAX as i32 {
                if cap.cc_alg[i as usize] != 0 {
                    break;
                }
                i += 1;
            }
        }

        // If a driver unregistered its last algorithm or all of them
        // (alg == CRYPTO_ALGORITHM_MAX + 1), cleanup its entry.
        if i == all || alg == all {
            let ses = cap.cc_sessions;
            *cap = Cryptocap::default();
            if ses != 0 {
                // If there are pending sessions, just mark as invalid.
                cap.cc_flags |= CRYPTOCAP_F_CLEANUP;
                cap.cc_sessions = ses;
            }
        }
        Ok(())
    })
}

/// Makes a new session from the descriptors' own initialisation data, chained in order.
fn crypto_migrate(descs: &[Cryptodesc<'_>]) -> Result<u64, Errno> {
    fn chain(descs: &[Cryptodesc<'_>], next: Option<&Cryptoini<'_>>) -> Result<u64, Errno> {
        match descs.split_last() {
            None => match next {
                Some(first) => crypto_newsession(first, 0),
                None => Err(Errno::EINVAL),
            },
            Some((last, rest)) => {
                let mut ini = last.CRD_INI;
                ini.cri_next = next;
                chain(rest, Some(&ini))
            }
        }
    }
    chain(descs, None)
}

/// `crypto_invoke`: dispatch a crypto request to the appropriate crypto devices. `EAGAIN`
/// means the session was made again on another driver (`crp_sid` is the new one) and the
/// request is to be invoked again.
pub fn crypto_invoke(crp: &mut Cryptop<'_>) -> Result<(), Errno> {
    let s = splvm();
    let r = crypto_invoke_locked(crp);
    splx(s);
    r
}

fn crypto_invoke_locked(crp: &mut Cryptop<'_>) -> Result<(), Errno> {
    kernel_assert_locked();

    // Sanity checks.
    let num = crypto_drivers_num();
    if crp.crp_ndesc < 1 || num == 0 {
        return Err(Errno::EINVAL);
    }

    let hid = ((crp.crp_sid >> 32) & 0xffffffff) as usize;
    'migrate: {
        if hid >= num {
            break 'migrate;
        }

        let flags = with_drivers(|d| d[hid].cc_flags);
        if (flags & CRYPTOCAP_F_CLEANUP) != 0 {
            let _ = crypto_freesession(crp.crp_sid);
            break 'migrate;
        }

        let Some(process) = with_drivers(|d| d[hid].cc_process) else {
            break 'migrate;
        };

        with_drivers(|d| {
            d[hid].cc_operations += 1;
            d[hid].cc_bytes += crp.crp_ilen as u64;
        });

        match process(crp) {
            Err(Errno::ERESTART) => {
                // Unregister driver and migrate session.
                let _ = crypto_unregister(hid as u32, CRYPTO_ALGORITHM_MAX as i32 + 1);
            }
            r => return r,
        }
    }

    // Migrate session.
    let n = (crp.crp_ndesc as usize).min(crp.crp_desc.len());
    if let Ok(nid) = crypto_migrate(&crp.crp_desc[..n]) {
        crp.crp_sid = nid;
    }

    Err(Errno::EAGAIN)
}

/// `crypto_freereq`: release a set of crypto descriptors.
pub fn crypto_freereq(crp: Option<Cryptop<'_>>) {
    drop(crp);
}

/// `crypto_getreq`: acquire a set of `num` crypto descriptors; `None` when out of memory.
pub fn crypto_getreq<'a>(num: i32) -> Option<Cryptop<'a>> {
    let mut crp = Cryptop::default();

    if num < 0 {
        return None;
    }
    crp.crp_desc.try_reserve_exact(num as usize).ok()?;
    crp.crp_desc.resize_with(num as usize, Cryptodesc::default);
    crp.crp_ndescalloc = num;
    crp.crp_ndesc = num;

    Some(crp)
}

/// `crypto_init`: the C initialises `cryptop_pool` here; requests are plain values (see the
/// deviations), so there is nothing to do.
pub fn crypto_init() {}

/// Forgets every driver: the host tests start from an empty framework.
#[cfg(test)]
pub(crate) fn crypto_reset() {
    with_drivers(|d| d.clear());
}

#[cfg(test)]
mod tests;
