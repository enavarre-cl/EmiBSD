/* <CODE> */
//! The UNIX file system: OpenBSD `sys/ufs/`.
//!
//! `ufs` is the layer the UFS-like file systems share (inodes, directories, the vnode
//! operations); `ffs` is the fast file system on it. Both are compiled with feature `ffs`
//! (`option FFS`), as `conf/files` builds them for `ffs | mfs`; `mfs` is the memory file
//! system on `ffs`, compiled with feature `mfs` (`option MFS`); `ext2fs` is the second extended
//! file system, compiled with feature `ext2fs` (`option EXT2FS`).

#[cfg(feature = "ext2fs")]
pub mod ext2fs;
pub mod ffs;
#[cfg(feature = "mfs")]
pub mod mfs;
#[allow(clippy::module_inception)] // OpenBSD's sys/ufs/ufs
pub mod ufs;
/* </CODE> */
