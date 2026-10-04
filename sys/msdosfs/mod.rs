//! The MS-DOS FAT file system: OpenBSD `sys/msdosfs/` (feature `msdosfs`, `option MSDOSFS`).
//!
//! Headers (types): `bootsect`, `bpb`, `denode`, `direntry`, `fat`, `msdosfsmount`. Files
//! (functions): `msdosfs_conv`, `msdosfs_denode`, `msdosfs_fat`; `msdosfs_lookup` and
//! `msdosfs_vnops` hold only the items the others call (`M10C-SHIM`) until their ports;
//! `msdosfs_vfsops` joins with its port.

pub mod bootsect;
pub mod bpb;
pub mod denode;
pub mod direntry;
pub mod fat;
pub mod msdosfs_conv;
pub mod msdosfs_denode;
pub mod msdosfs_fat;
pub mod msdosfs_lookup;
pub mod msdosfs_vnops;
pub mod msdosfsmount;
