/*	$OpenBSD: kern_subr.c,v 1.53 2024/10/08 11:57:59 claudio Exp $	*/
/*	$NetBSD: kern_subr.c,v 1.15 1996/04/09 17:21:56 ragge Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1982, 1986, 1991, 1993
 *	The Regents of the University of California.  All rights reserved.
 * (c) UNIX System Laboratories, Inc.
 * All or some portions of this file are derived from material licensed
 * to the University of California by American Telephone and Telegraph
 * Co. or Unix System Laboratories, Inc. and are reproduced herein with
 * the permission of UNIX System Laboratories, Inc.
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
 *	@(#)kern_subr.c	8.3 (Berkeley) 1/21/94
 */
/* </LICENSES> */

//! Kernel subroutines: `kern/kern_subr.c`.
//!
//! Upstream: sys/kern/kern_subr.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M5 ports `hashinit` and `hashfree`; `uiomove`, `ureadc`, the
//! hook lists (`hook_establish`, `dohooks`) and the rest arrive with the subsystems that use
//! them (M6, M7).
//!
//! ## Deviations
//! - `hashinit` returns the table as a slice (its length is `hashmask + 1`) instead of a
//!   pointer plus an out-parameter mask; callers mask with `len() - 1`.

use core::ptr::{self, NonNull};

use crate::kern::kern_malloc::{free, mallocarray};
use crate::kern::subr_prf::panic;
use crate::sys::queue::{ListAdapter, ListHead};

/// The power of two the hash table of `elements` rounds up to.
fn hashsize(elements: i32) -> usize {
    let elements = elements as usize;
    if elements & (elements - 1) == 0 {
        elements
    } else {
        let mut hashsize = 1;
        while hashsize < elements {
            hashsize <<= 1;
        }
        hashsize
    }
}

/// `hashinit`: a table of `elements` (rounded up to a power of two) empty lists, allocated
/// from `type_` with `flags`; `None` when the allocation failed.
pub fn hashinit<A: ListAdapter>(
    elements: i32,
    type_: i32,
    flags: i32,
) -> Option<&'static [ListHead<A>]> {
    if elements <= 0 {
        panic(format_args!("hashinit: bad cnt"));
    }
    let hashsize = hashsize(elements);
    let hashtbl =
        mallocarray(hashsize, size_of::<ListHead<A>>(), type_, flags)?.cast::<ListHead<A>>();
    for i in 0..hashsize {
        // SAFETY: `hashsize` heads were just allocated at `hashtbl`.
        unsafe { ptr::write(hashtbl.as_ptr().add(i), ListHead::new()) };
    }
    // SAFETY: the heads are initialised and the allocation is never freed while the table
    // is in use (`hashfree` takes it back).
    Some(unsafe { core::slice::from_raw_parts(hashtbl.as_ptr(), hashsize) })
}

/// `hashfree`: releases a table `hashinit` made for `elements`.
///
/// # Safety
///
/// `hash` came from `hashinit` with the same `elements` and `type_`, its lists are empty,
/// and nothing uses it afterwards.
pub unsafe fn hashfree<A: ListAdapter>(hash: &'static [ListHead<A>], elements: i32, type_: i32) {
    if elements <= 0 {
        panic(format_args!("hashfree: bad cnt"));
    }
    let hashsize = hashsize(elements);
    let Some(p) = NonNull::new(hash.as_ptr().cast_mut().cast::<u8>()) else {
        return;
    };
    free(p, type_, hashsize * size_of::<ListHead<A>>());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_round_up_to_powers_of_two() {
        assert_eq!(hashsize(1), 1);
        assert_eq!(hashsize(2), 2);
        assert_eq!(hashsize(3), 4);
        assert_eq!(hashsize(64), 64);
        assert_eq!(hashsize(65), 128);
    }
}
