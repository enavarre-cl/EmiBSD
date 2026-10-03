//! The fast file system: OpenBSD `sys/ufs/ffs/`.
//!
//! Headers (types): `fs`, `ffs_extern`. Files (functions): `ffs_alloc`, `ffs_balloc`,
//! `ffs_inode`, `ffs_subr`, `ffs_tables`, `ffs_vfsops`, `ffs_vnops`. FFS2 support is
//! feature `ffs2` (`option FFS2`). OpenBSD no longer has soft updates (`ffs_softdep.c`).

pub mod ffs_alloc;
pub mod ffs_balloc;
pub mod ffs_extern;
pub mod ffs_inode;
pub mod ffs_subr;
pub mod ffs_tables;
pub mod ffs_vfsops;
pub mod ffs_vnops;
pub mod fs;
