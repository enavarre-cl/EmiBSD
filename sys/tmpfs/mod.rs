/* <CODE> */
//! The efficient memory file system: OpenBSD `sys/tmpfs/` (`option TMPFS`, feature `tmpfs`).
//!
//! Headers (types): `tmpfs` (`tmpfs.h`), `tmpfs_vnops` (`tmpfs_vnops.h`, with
//! `tmpfs_vnops.c`). Files (functions): `tmpfs_mem`, `tmpfs_subr`, `tmpfs_vfsops`,
//! `tmpfs_specops`, `tmpfs_fifoops`, `tmpfs_vnops`. File data lives in an anonymous UVM
//! object per regular file (`uvm_aobj.rs`'s `uao_grow`/`uao_shrink`, also behind the
//! feature).

#[allow(clippy::module_inception)] // OpenBSD's layout: sys/tmpfs/tmpfs.h
pub mod tmpfs;
pub mod tmpfs_fifoops;
pub mod tmpfs_mem;
pub mod tmpfs_specops;
pub mod tmpfs_subr;
pub mod tmpfs_vfsops;
pub mod tmpfs_vnops;
/* </CODE> */
