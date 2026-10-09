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
/* </LICENSES> */

/* <CODE> */
//! `<sys/param.h>` and `<sys/syslimits.h>` for libsa: the sizes and the rounding macros.

/// `NBBY`: bits per byte.
pub const NBBY: usize = 8;
/// `DEV_BSHIFT` (`_DEV_BSHIFT`): log2 of [`DEV_BSIZE`].
pub const DEV_BSHIFT: u32 = 9;
/// `DEV_BSIZE`: the unit of disk addresses.
pub const DEV_BSIZE: usize = 1 << DEV_BSHIFT;
/// `MAXBSIZE`: the largest file system block.
pub const MAXBSIZE: usize = 64 * 1024;
/// `MAXPATHLEN` (`PATH_MAX`): the longest path name, NUL included.
pub const MAXPATHLEN: usize = 1024;
/// `MAXSYMLINKS` (`SYMLOOP_MAX`): symbolic links followed in one lookup.
pub const MAXSYMLINKS: u32 = 32;
/// `PAGE_SIZE` of amd64 and arm64.
pub const PAGE_SIZE: u64 = 4096;

/// `btodb(x)`: bytes to [`DEV_BSIZE`] blocks.
pub const fn btodb(x: i64) -> i64 {
    x >> DEV_BSHIFT
}

/// `roundup(x, y)`: `x` rounded up to a multiple of `y`.
pub const fn roundup(x: u64, y: u64) -> u64 {
    x.div_ceil(y) * y
}

/// `howmany(x, y)`: how many `y`s hold `x`.
pub const fn howmany(x: u64, y: u64) -> u64 {
    x.div_ceil(y)
}
/* </CODE> */
