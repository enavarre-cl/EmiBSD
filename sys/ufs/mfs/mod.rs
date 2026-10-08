/* <CODE> */
//! The memory file system: OpenBSD `sys/ufs/mfs/`, feature `mfs` (`option MFS`).
//!
//! Headers (types): `mfsnode`, `mfs_extern`. Files (functions): `mfs_vfsops`, `mfs_vnops`.
//! An MFS is an FFS (`ufs/ffs`) whose "disk" is a block of memory in the process that
//! mounted it (`mount_mfs(8)`), which stays in the kernel serving the file system's I/O.

pub mod mfs_extern;
pub mod mfs_vfsops;
pub mod mfs_vnops;
pub mod mfsnode;
/* </CODE> */
