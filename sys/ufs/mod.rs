//! The UNIX file system: OpenBSD `sys/ufs/`.
//!
//! `ufs` is the layer the UFS-like file systems share (inodes, directories, the vnode
//! operations); `ffs` is the fast file system on it. Both are compiled with feature `ffs`
//! (`option FFS`), as `conf/files` builds them for `ffs | mfs`; `mfs` is the memory file
//! system on `ffs`, compiled with feature `mfs` (`option MFS`); `ext2fs` is not ported.

pub mod ffs;
#[cfg(feature = "mfs")]
pub mod mfs;
#[allow(clippy::module_inception)] // OpenBSD's sys/ufs/ufs
pub mod ufs;
