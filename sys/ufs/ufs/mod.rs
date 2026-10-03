//! The UFS layer: OpenBSD `sys/ufs/ufs/`.
//!
//! Headers (types): `dinode`, `dir`, `inode`, `quota`, `ufsmount`, `ufs_extern`. Files
//! (functions): `ufs_bmap`, `ufs_ihash`, `ufs_inode`, `ufs_lookup`, `ufs_vfsops`,
//! `ufs_vnops`. `ufs_dirhash.c` (`option UFS_DIRHASH`) and `ufs_quota.c` (`option QUOTA`)
//! are not ported; `ufs_quota_stub.c` is skipped (see `quota.rs`).

pub mod dinode;
pub mod dir;
pub mod inode;
pub mod quota;
pub mod ufs_bmap;
pub mod ufs_extern;
pub mod ufs_ihash;
pub mod ufs_inode;
pub mod ufs_lookup;
pub mod ufs_vfsops;
pub mod ufs_vnops;
pub mod ufsmount;
