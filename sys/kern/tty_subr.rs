/*	$OpenBSD: tty_subr.c,v 1.36 2022/08/14 01:58:28 jsg Exp $	*/
/*	$NetBSD: tty_subr.c,v 1.13 1996/02/09 19:00:43 christos Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1993, 1994 Theo de Raadt
 * All rights reserved.
 *
 * Per Lindqvist <pgd@compuram.bbt.se> supplied an almost fully working
 * set of true clist functions that this is very loosely based on.
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
 * IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT, INDIRECT,
 * INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT
 * NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
 * DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
 * THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF
 * THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

//! The character lists of the terminals: `kern/tty_subr.c`.
//!
//! Upstream: sys/kern/tty_subr.c @ 3ce1f3f79392
//!
//! Clists are really ring buffers. The `c_cc`, `c_cf`, `c_cl` fields have exactly the same
//! behaviour as in true clists. A queue allocated without quoting (`c_cq` NULL) has no
//! `TTY_QUOTE` functionality, which saves memory and CPU time: a line discipline that does
//! not need it (SLIP wanting a 32K ring) frees `c_cq` and sets it to NULL.
//!
//! ## Deviations
//! - The cursors are indices (see `sys/tty.rs`); `firstc`/`nextc` take and return
//!   `Option<usize>` where the C passes `u_char *` (NULL is `None`), and the `int *c`,
//!   `int *cc` out-parameters are `&mut i32`.
//! - The ring and the quote bitmap are reached as slices of `Cell<u8>` over the
//!   `clalloc` allocation ([`ring`], [`quote`]), so every index is bounds-checked; a bit
//!   past the bitmap (`ndqb`'s `isset(c_cq, i)` after the last character) reads as clear
//!   instead of reading past the allocation.
//! - `b_to_q`/`q_to_b` take the buffer as a slice: `cc`/`count` is its length; they return
//!   `usize`.
//! - `getc` and `unputc` return `-1` for an empty queue, as in C (a character is never
//!   negative, `TTY_QUOTE` included).
//! - `clalloc` panics if `malloc(M_WAITOK)` fails: `malloc(9)` cannot sleep yet (`M_WAITOK`
//!   cannot fail in C).

use core::cell::Cell;
use core::ptr::NonNull;
use core::slice;

use crate::kern::kern_malloc::{free, malloc};
use crate::kern::subr_prf::panic;
use crate::machine::intr::{spltty, splx};
use crate::sys::malloc::{M_TTYS, M_WAITOK, M_ZERO};
use crate::sys::tty::{Clist, TTY_QUOTE};

/// Bits per byte (`NBBY`, `<sys/select.h>`).
const NBBY: usize = u8::BITS as usize;

/// `QMEM(n)`: the bytes of a quote bitmap for `n` characters.
pub const fn qmem(n: usize) -> usize {
    ((n - 1) / NBBY) + 1
}

/// The ring buffer of `clp` (`c_cs` .. `c_ce`), empty before `clalloc`.
fn ring(clp: &Clist) -> &[Cell<u8>] {
    let p = clp.c_cs.get();
    if p.is_null() {
        return &[];
    }
    // SAFETY: `c_cs` is null or the `c_cn`-byte allocation `clalloc` made, alive until
    // `clfree` (which runs when the tty is freed, never while a queue operation runs);
    // `Cell<u8>` has the layout of `u8`, and every access to the ring is through these cells.
    unsafe { slice::from_raw_parts(p.cast::<Cell<u8>>(), clp.c_cn.get() as usize) }
}

/// The quote bitmap of `clp` (`c_cq`), if the queue has one.
fn quote(clp: &Clist) -> Option<&[Cell<u8>]> {
    let p = clp.c_cq.get();
    if p.is_null() {
        return None;
    }
    // SAFETY: as for `ring`: `clalloc` allocated `QMEM(c_cn)` bytes here.
    Some(unsafe { slice::from_raw_parts(p.cast::<Cell<u8>>(), qmem(clp.c_cn.get() as usize)) })
}

/// `isset(map, i)`; a bit past the map is clear.
fn qisset(map: &[Cell<u8>], i: usize) -> bool {
    map.get(i / NBBY)
        .is_some_and(|b| b.get() & (1 << (i % NBBY)) != 0)
}

/// `setbit(map, i)`.
fn qsetbit(map: &[Cell<u8>], i: usize) {
    let b = &map[i / NBBY];
    b.set(b.get() | (1 << (i % NBBY)));
}

/// `clrbit(map, i)`.
fn qclrbit(map: &[Cell<u8>], i: usize) {
    let b = &map[i / NBBY];
    b.set(b.get() & !(1 << (i % NBBY)));
}

/// `clalloc`: initialize a particular clist. Ok, they are really ring buffers, of the
/// specified length, with/without quoting support.
pub fn clalloc(clp: &Clist, size: i32, quot: bool) {
    let size_u = size as usize;
    let Some(cs) = malloc(size_u, M_TTYS, M_WAITOK | M_ZERO) else {
        panic(format_args!("clalloc: out of memory"));
    };
    clp.c_cs.set(cs.as_ptr());

    if quot {
        let Some(cq) = malloc(qmem(size_u), M_TTYS, M_WAITOK | M_ZERO) else {
            panic(format_args!("clalloc: out of memory"));
        };
        clp.c_cq.set(cq.as_ptr());
    } else {
        clp.c_cq.set(core::ptr::null_mut());
    }

    clp.c_cf.set(0);
    clp.c_cl.set(0);
    clp.c_cn.set(size);
    clp.c_cc.set(0);
}

/// `clfree`: releases a clist's ring and bitmap, scrubbed first.
pub fn clfree(clp: &Clist) {
    let cn = clp.c_cn.get() as usize;
    if let Some(cs) = NonNull::new(clp.c_cs.get()) {
        for b in ring(clp) {
            // explicit_bzero: a volatile store per byte, as `libkern::explicit_bzero` does.
            // SAFETY: `b` is a byte of the ring, valid for writes.
            unsafe { b.as_ptr().write_volatile(0) };
        }
        free(cs, M_TTYS, cn);
    }
    if let Some(cq) = NonNull::new(clp.c_cq.get()) {
        if let Some(map) = quote(clp) {
            for b in map {
                // SAFETY: as above, a byte of the bitmap.
                unsafe { b.as_ptr().write_volatile(0) };
            }
        }
        free(cq, M_TTYS, qmem(cn));
    }
    clp.c_cs.set(core::ptr::null_mut());
    clp.c_cq.set(core::ptr::null_mut());
}

/// `getc`: get a character from a clist; -1 when it is empty.
pub fn getc(clp: &Clist) -> i32 {
    let mut c = -1;

    let s = spltty();
    'out: {
        if clp.c_cc.get() == 0 {
            break 'out;
        }

        let ring = ring(clp);
        let cf = clp.c_cf.get();
        c = i32::from(ring[cf].get());
        ring[cf].set(0);
        if let Some(cq) = quote(clp) {
            if qisset(cq, cf) {
                c |= TTY_QUOTE;
            }
            qclrbit(cq, cf);
        }
        let mut cf = cf + 1;
        if cf == clp.c_cn.get() as usize {
            cf = 0;
        }
        clp.c_cf.set(cf);
        clp.c_cc.set(clp.c_cc.get() - 1);
        if clp.c_cc.get() == 0 {
            clp.c_cf.set(0);
            clp.c_cl.set(0);
        }
    }
    splx(s);
    c
}

/// `q_to_b`: copy clist to buffer. Return number of bytes moved.
pub fn q_to_b(clp: &Clist, cp: &mut [u8]) -> usize {
    let mut count = cp.len();
    let mut p = 0;

    let s = spltty();
    // optimize this while loop
    while count > 0 && clp.c_cc.get() > 0 {
        let cn = clp.c_cn.get() as usize;
        let cf = clp.c_cf.get();
        let cl = clp.c_cl.get();
        let mut cc = if cf >= cl { cn - cf } else { cl - cf };
        if cc > count {
            cc = count;
        }
        let ring = ring(clp);
        for (dst, src) in cp[p..p + cc].iter_mut().zip(&ring[cf..cf + cc]) {
            *dst = src.get();
            src.set(0);
        }
        if let Some(cq) = quote(clp) {
            clrbits(cq, cf, cc);
        }
        count -= cc;
        p += cc;
        clp.c_cc.set(clp.c_cc.get() - cc as i32);
        let mut cf = cf + cc;
        if cf == cn {
            cf = 0;
        }
        clp.c_cf.set(cf);
    }
    if clp.c_cc.get() == 0 {
        clp.c_cf.set(0);
        clp.c_cl.set(0);
    }
    splx(s);
    p
}

/// `ndqb`: return count of contiguous characters in clist. Stop counting if
/// `flag & character` is non-null.
pub fn ndqb(clp: &Clist, flag: i32) -> i32 {
    let mut count: i32 = 0;

    let s = spltty();
    'out: {
        let mut cc = clp.c_cc.get();
        if cc == 0 {
            break 'out;
        }

        let cn = clp.c_cn.get() as usize;
        if flag == 0 {
            count = clp.c_cl.get() as i32 - clp.c_cf.get() as i32;
            if count <= 0 {
                count = (cn - clp.c_cf.get()) as i32;
            }
            break 'out;
        }

        let ring = ring(clp);
        let mut i = clp.c_cf.get();
        if flag & TTY_QUOTE != 0 {
            // The quote bit tested is the one after the character, as the C's `i++` makes
            // it.
            let quoted = |i: usize| quote(clp).is_some_and(|cq| qisset(cq, i));
            while cc > 0 {
                cc -= 1;
                let ch = i32::from(ring[i].get());
                i += 1;
                if ch & (flag & !TTY_QUOTE) != 0 || quoted(i) {
                    break;
                }
                count += 1;
                if i == cn {
                    break;
                }
            }
        } else {
            while cc > 0 {
                cc -= 1;
                let ch = i32::from(ring[i].get());
                i += 1;
                if ch & flag != 0 {
                    break;
                }
                count += 1;
                if i == cn {
                    break;
                }
            }
        }
    }
    splx(s);
    count
}

/// `ndflush`: flush `count` bytes from clist.
pub fn ndflush(clp: &Clist, count: i32) {
    let mut count = count;

    let s = spltty();
    'out: {
        if count == clp.c_cc.get() {
            clp.c_cc.set(0);
            clp.c_cf.set(0);
            clp.c_cl.set(0);
            break 'out;
        }
        // optimize this while loop
        while count > 0 && clp.c_cc.get() > 0 {
            let cn = clp.c_cn.get() as usize;
            let cf = clp.c_cf.get();
            let cl = clp.c_cl.get();
            let mut cc = if cf >= cl { cn - cf } else { cl - cf } as i32;
            if cc > count {
                cc = count;
            }
            count -= cc;
            clp.c_cc.set(clp.c_cc.get() - cc);
            let mut cf = cf + cc as usize;
            if cf == cn {
                cf = 0;
            }
            clp.c_cf.set(cf);
        }
        if clp.c_cc.get() == 0 {
            clp.c_cf.set(0);
            clp.c_cl.set(0);
        }
    }
    splx(s);
}

/// `putc`: put a character into the output queue; -1 when it is full.
pub fn putc(c: i32, clp: &Clist) -> i32 {
    let s = spltty();
    if clp.c_cc.get() == clp.c_cn.get() {
        splx(s);
        return -1;
    }

    if clp.c_cc.get() == 0 {
        if clp.c_cs.get().is_null() {
            panic(format_args!("putc: tty has no clist"));
        }
        clp.c_cf.set(0);
        clp.c_cl.set(0);
    }

    let i = clp.c_cl.get();
    ring(clp)[i].set((c & 0xff) as u8);
    if let Some(cq) = quote(clp) {
        if c & TTY_QUOTE != 0 {
            qsetbit(cq, i);
        } else {
            qclrbit(cq, i);
        }
    }
    clp.c_cc.set(clp.c_cc.get() + 1);
    let mut cl = i + 1;
    if cl == clp.c_cn.get() as usize {
        cl = 0;
    }
    clp.c_cl.set(cl);
    splx(s);
    0
}

/// `clrbits`: optimized version of `for (i = 0; i < len; i++) clrbit(cp, off + i);`.
pub fn clrbits(cp: &[Cell<u8>], off: usize, len: usize) {
    if len == 1 {
        qclrbit(cp, off);
        return;
    }

    let mut sby = off / NBBY;
    let sbi = off % NBBY;
    let eby = (off + len) / NBBY;
    let ebi = (off + len) % NBBY;
    if sby == eby {
        let mask = (((1u32 << (ebi - sbi)) - 1) << sbi) as u8;
        cp[sby].set(cp[sby].get() & !mask);
    } else {
        let mask = ((1u32 << sbi) - 1) as u8;
        cp[sby].set(cp[sby].get() & mask);
        sby += 1;

        for b in &cp[sby..eby] {
            b.set(0x00);
        }

        let mask = ((1u32 << ebi) - 1) as u8;
        if mask != 0 {
            // if no mask, eby may be 1 too far
            cp[eby].set(cp[eby].get() & !mask);
        }
    }
}

/// `b_to_q`: copy buffer to clist. Return number of bytes not transferred.
pub fn b_to_q(cp: &[u8], clp: &Clist) -> usize {
    let mut count = cp.len();
    let mut p = 0;

    if count == 0 {
        return 0;
    }

    let s = spltty();
    'out: {
        if clp.c_cc.get() == clp.c_cn.get() {
            break 'out;
        }

        if clp.c_cc.get() == 0 {
            if clp.c_cs.get().is_null() {
                panic(format_args!("b_to_q: tty has no clist"));
            }
            clp.c_cf.set(0);
            clp.c_cl.set(0);
        }

        // optimize this while loop
        while count > 0 && clp.c_cc.get() < clp.c_cn.get() {
            let cn = clp.c_cn.get() as usize;
            let cf = clp.c_cf.get();
            let cl = clp.c_cl.get();
            let mut cc = if cf > cl { cf - cl } else { cn - cl };
            if cc > count {
                cc = count;
            }
            for (dst, &src) in ring(clp)[cl..cl + cc].iter().zip(&cp[p..p + cc]) {
                dst.set(src);
            }
            if let Some(cq) = quote(clp) {
                clrbits(cq, cl, cc);
            }
            p += cc;
            count -= cc;
            clp.c_cc.set(clp.c_cc.get() + cc as i32);
            let mut cl = cl + cc;
            if cl == cn {
                cl = 0;
            }
            clp.c_cl.set(cl);
        }
    }
    splx(s);
    count
}

/// `nextc`: given a cursor into the clist return the cursor of the next character in the
/// list, or `None` if no more chars; `*c` is set to that character.
///
/// Callers must not allow `getc`s to happen between `firstc`s and `nextc`s so that the
/// cursor becomes invalid. Note that interrupts are NOT masked.
pub fn nextc(clp: &Clist, cp: Option<usize>, c: &mut i32, ccp: &mut i32) -> Option<usize> {
    if clp.c_cc.get() != 0 && cp == Some(clp.c_cf.get()) {
        // First time initialization.
        *ccp = clp.c_cc.get();
    }
    let cp = cp?;
    if *ccp == 0 {
        return None;
    }
    *ccp -= 1;
    if *ccp == 0 {
        return None;
    }
    let mut cp = cp + 1;
    if cp == clp.c_cn.get() as usize {
        cp = 0;
    }
    *c = i32::from(ring(clp)[cp].get());
    if let Some(cq) = quote(clp)
        && qisset(cq, cp)
    {
        *c |= TTY_QUOTE;
    }
    Some(cp)
}

/// `firstc`: return the cursor of the first character in the list, or `None` if there is
/// none; `*c` is set to that character.
///
/// Callers must not allow `getc`s to happen between `firstc`s and `nextc`s so that the
/// cursor becomes invalid. Note that interrupts are NOT masked.
pub fn firstc(clp: &Clist, c: &mut i32, ccp: &mut i32) -> Option<usize> {
    *ccp = clp.c_cc.get();
    if *ccp == 0 {
        return None;
    }
    let cp = clp.c_cf.get();
    *c = i32::from(ring(clp)[cp].get());
    if let Some(cq) = quote(clp)
        && qisset(cq, cp)
    {
        *c |= TTY_QUOTE;
    }
    Some(cp)
}

/// `unputc`: remove the last character in the clist and return it; -1 when it is empty.
pub fn unputc(clp: &Clist) -> i32 {
    let mut c: i32 = -1;

    let s = spltty();
    'out: {
        if clp.c_cc.get() == 0 {
            break 'out;
        }

        let cl = if clp.c_cl.get() == 0 {
            clp.c_cn.get() as usize - 1
        } else {
            clp.c_cl.get() - 1
        };
        clp.c_cl.set(cl);
        clp.c_cc.set(clp.c_cc.get() - 1);

        let ring = ring(clp);
        c = i32::from(ring[cl].get());
        ring[cl].set(0);
        if let Some(cq) = quote(clp) {
            if qisset(cq, cl) {
                c |= TTY_QUOTE;
            }
            qclrbit(cq, cl);
        }
        if clp.c_cc.get() == 0 {
            clp.c_cf.set(0);
            clp.c_cl.set(0);
        }
    }
    splx(s);
    c
}

/// `catq`: put the chars in the `from` queue on the end of the `to` queue.
pub fn catq(from: &Clist, to: &Clist) {
    let s = spltty();
    if from.c_cc.get() == 0 {
        // nothing to move
        splx(s);
        return;
    }

    // If `to` queue is empty and the queues are the same max size, it is more efficient to
    // just swap the clist structures.
    if to.c_cc.get() == 0 && from.c_cn.get() == to.c_cn.get() {
        from.swap(to);
        splx(s);
        return;
    }
    splx(s);

    loop {
        let c = getc(from);
        if c == -1 {
            break;
        }
        putc(c, to);
    }
}

#[cfg(test)]
mod tests;
