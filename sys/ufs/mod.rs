//! The UNIX file system: OpenBSD `sys/ufs/`.
//!
//! `ufs` is the layer the UFS-like file systems share (inodes, directories, the vnode
//! operations); `ffs` is the fast file system on it. Both are compiled with feature `ffs`
//! (`option FFS`), as `conf/files` builds them for `ffs | mfs`; `mfs` and `ext2fs` are not
//! ported.

pub mod ffs;
#[allow(clippy::module_inception)] // OpenBSD's sys/ufs/ufs
pub mod ufs;
