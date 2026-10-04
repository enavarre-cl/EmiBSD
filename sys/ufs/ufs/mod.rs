//! The UFS layer: OpenBSD `sys/ufs/ufs/`.
//!
//! Headers (types): `dinode`, `dir`, `dirhash`, `inode`, `quota`, `ufsmount`, `ufs_extern`.
//! Files (functions): `ufs_bmap`, `ufs_dirhash` (feature `ufs_dirhash`, `option
//! UFS_DIRHASH`), `ufs_ihash`, `ufs_inode`, `ufs_lookup`, `ufs_quota` (feature `quota`,
//! `option QUOTA`), `ufs_vfsops`, `ufs_vnops`. `ufs_quota_stub.c` is skipped (see `quota.rs`).

pub mod dinode;
pub mod dir;
pub mod dirhash;
pub mod inode;
pub mod quota;
pub mod ufs_bmap;
#[cfg(feature = "ufs_dirhash")]
pub mod ufs_dirhash;
pub mod ufs_extern;
pub mod ufs_ihash;
pub mod ufs_inode;
pub mod ufs_lookup;
#[cfg(feature = "quota")]
pub mod ufs_quota;
pub mod ufs_vfsops;
pub mod ufs_vnops;
pub mod ufsmount;
