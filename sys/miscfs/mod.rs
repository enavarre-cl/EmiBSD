/* <CODE> */
//! Miscellaneous file systems: OpenBSD `sys/miscfs/`.
//!
//! `deadfs` holds the operations of revoked vnodes; `fuse` is FUSE (feature `fuse`,
//! `option FUSE`); `fifofs` is not ported.

pub mod deadfs;
#[cfg(feature = "fuse")]
pub mod fuse;
/* </CODE> */
