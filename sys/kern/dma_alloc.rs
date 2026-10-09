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
 * Copyright (c) 2010 Theo de Raadt <deraadt@openbsd.org>
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
//! `dma_alloc(9)`: small physically contiguous, DMA-reachable buffers from a set of pools,
//! one per power of two from 16 bytes to 64 KB.
//!
//! Upstream: sys/kern/dma_alloc.c @ 3ce1f3f79392
//!
//! `dma_alloc_init` (from `uvm_init`, after `kmeminit`) makes the pools `dma16` ..
//! `dma65536`, each backed by `kp_dma_contig`; `dma_alloc` takes an item from the smallest
//! pool that fits and `dma_free` gives it back to the same pool.
//!
//! ## Deviations
//! - `dmanames` is a table of `&'static str` written out at compile time ([`DMANAMES`]) where
//!   the C `snprintf`s each name into a `char[10]` at `dma_alloc_init` time: `pool_init`
//!   keeps a `&'static str` as the pool's wait channel name.
//! - `dma_alloc_index` returns `Option<usize>` (the C's `-1` is `None`); `DEBUG` is not
//!   configured, so its "too large" message is not printed, as in the C.
//! - `dma_alloc` returns `Option<NonNull<u8>>` (NULL is `None`); `dma_free` takes the
//!   `NonNull` back.

use core::ptr::NonNull;

use crate::kern::subr_pool::{pool_get, pool_init, pool_put, pool_set_constraints};
use crate::machine::intr::IPL_VM;
use crate::sys::pool::Pool;
use crate::uvm::uvm_km::KP_DMA_CONTIG;

/// `DMA_PAGE_SHIFT`: the largest pool's items are `1 << DMA_PAGE_SHIFT` bytes.
pub const DMA_PAGE_SHIFT: usize = 16;
/// `DMA_BUCKET_OFFSET`: the smallest pool's items are `1 << DMA_BUCKET_OFFSET` bytes.
pub const DMA_BUCKET_OFFSET: usize = 4;
/// `nitems(dmapools)`.
const NDMAPOOLS: usize = DMA_PAGE_SHIFT - DMA_BUCKET_OFFSET + 1;

/// `dmanames`: the pools' names, "dma%d" of each pool's item size.
static DMANAMES: [&str; NDMAPOOLS] = [
    "dma16", "dma32", "dma64", "dma128", "dma256", "dma512", "dma1024", "dma2048", "dma4096",
    "dma8192", "dma16384", "dma32768", "dma65536",
];

/// `dmapools`: objects sized 2^4 to 2^16.
pub static DMAPOOLS: [Pool; NDMAPOOLS] = [const { Pool::new() }; NDMAPOOLS];

/// `dma_alloc_init`: creates the pools.
pub fn dma_alloc_init() {
    for (i, pp) in DMAPOOLS.iter().enumerate() {
        pool_init(
            pp,
            1 << (i + DMA_BUCKET_OFFSET),
            0,
            IPL_VM,
            0,
            DMANAMES[i],
            None,
        );
        pool_set_constraints(pp, &KP_DMA_CONTIG);
        // XXX need pool_setlowat(&dmapools[i], dmalowat);
    }
}

/// `dma_alloc_index`: the smallest pool whose items hold `sz` bytes.
#[inline]
fn dma_alloc_index(sz: usize) -> Option<usize> {
    (0..NDMAPOOLS).find(|&b| sz <= 1 << (b + DMA_BUCKET_OFFSET))
}

/// `dma_alloc`: `size` bytes of DMA-reachable, physically contiguous memory; `prflags` are
/// `pool_get`'s (`PR_WAITOK`/`PR_NOWAIT`, `PR_ZERO`). `None` when `size` is over 64 KB or the
/// pool has nothing.
pub fn dma_alloc(size: usize, prflags: i32) -> Option<NonNull<u8>> {
    let pi = dma_alloc_index(size)?;
    pool_get(&DMAPOOLS[pi], prflags)
}

/// `dma_free`: gives back what `dma_alloc(size, ..)` returned.
pub fn dma_free(m: NonNull<u8>, size: usize) {
    let Some(pi) = dma_alloc_index(size) else {
        return;
    };
    pool_put(&DMAPOOLS[pi], m);
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests of dma_alloc(9): the bucket choice, and items from the pools over real memory.

    use core::ptr;
    use std::{assert, assert_eq};

    use super::*;
    use crate::kern::subr_pool::tests::setup_real_memory;
    use crate::sys::pool::{PR_NOWAIT, PR_ZERO};

    #[test]
    fn index_is_the_smallest_bucket_that_fits() {
        assert_eq!(dma_alloc_index(0), Some(0));
        assert_eq!(dma_alloc_index(1), Some(0));
        assert_eq!(dma_alloc_index(16), Some(0));
        assert_eq!(dma_alloc_index(17), Some(1));
        assert_eq!(dma_alloc_index(1024), Some(6));
        assert_eq!(dma_alloc_index(65536), Some(12));
        assert_eq!(dma_alloc_index(65537), None);
        for (i, name) in DMANAMES.iter().enumerate() {
            assert_eq!(*name, std::format!("dma{}", 1 << (i + DMA_BUCKET_OFFSET)));
        }
    }

    #[test]
    fn pools_are_dma_contiguous_powers_of_two() {
        // The host's test memory lies above the DMA constraint (4 GB), so no item can be
        // taken here; the pools' set-up is what is checked.
        let _g = setup_real_memory();
        dma_alloc_init();
        for (i, pp) in DMAPOOLS.iter().enumerate() {
            assert_eq!(pp.pr_size.get() as usize, 1 << (i + DMA_BUCKET_OFFSET));
            assert_eq!(pp.pr_wchan.get(), DMANAMES[i]);
            assert!(
                pp.pr_crange
                    .get()
                    .is_some_and(|m| ptr::eq(m, &KP_DMA_CONTIG))
            );
        }
        assert!(dma_alloc(65537, PR_NOWAIT | PR_ZERO).is_none());
    }
}
/* </TESTS> */
