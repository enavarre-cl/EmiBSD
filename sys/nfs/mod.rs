//! The Network File System: OpenBSD `sys/nfs/` (`option NFSCLIENT`, feature `nfsclient`;
//! `option NFSSERVER`, feature `nfsserver`).
//!
//! Headers (types): `nfs` (`nfs.h`), `nfsproto` (`nfsproto.h`), `rpcv2` (`rpcv2.h`),
//! `xdr_subs` (`xdr_subs.h`), `nfsm_subs` (`nfsm_subs.h`), `nfsnode` (`nfsnode.h`),
//! `nfsmount` (`nfsmount.h`), `nfsrvcache` (`nfsrvcache.h`), `nfs_var` (`nfs_var.h`).
//! Files (functions): `nfs_subs`.

#[allow(clippy::module_inception)] // OpenBSD's layout: sys/nfs/nfs.h
pub mod nfs;
pub mod nfs_subs;
pub mod nfs_var;
pub mod nfsm_subs;
pub mod nfsmount;
pub mod nfsnode;
pub mod nfsproto;
pub mod nfsrvcache;
pub mod rpcv2;
pub mod xdr_subs;
