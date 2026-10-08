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
