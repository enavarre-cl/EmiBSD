/* <LICENSES> */
/*
 * Copyright (c) 2026 Emilio Navarrete Lineros <enavarre@outlook.com>
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */
/* </LICENSES> */

/* <CODE> */
//! The Network File System: OpenBSD `sys/nfs/` (`option NFSCLIENT`, feature `nfsclient`;
//! `option NFSSERVER`, feature `nfsserver`).
//!
//! Headers (types): `nfs` (`nfs.h`), `nfsproto` (`nfsproto.h`), `rpcv2` (`rpcv2.h`),
//! `xdr_subs` (`xdr_subs.h`), `nfsm_subs` (`nfsm_subs.h`), `nfsnode` (`nfsnode.h`),
//! `nfsmount` (`nfsmount.h`), `nfsrvcache` (`nfsrvcache.h`), `nfs_var` (`nfs_var.h`).
//! Files (functions): `nfs_socket`, `nfs_subs`, `nfs_srvsubs`, `nfs_srvcache`, `krpc_subr`
//! (and `krpc.h`), `nfs_boot`, `nfs_syscalls`; server: `nfs_serv`; client: `nfs_bio`, `nfs_debug`, `nfs_kq`,
//! `nfs_node`, `nfs_vfsops`, `nfs_vnops`; `nfsdiskless` is a header.

#[cfg(feature = "nfsclient")]
pub mod krpc_subr;
#[allow(clippy::module_inception)] // OpenBSD's layout: sys/nfs/nfs.h
pub mod nfs;
#[cfg(feature = "nfsclient")]
pub mod nfs_bio;
pub mod nfs_boot;
#[cfg(feature = "nfsclient")]
pub mod nfs_debug;
#[cfg(feature = "nfsclient")]
pub mod nfs_kq;
#[cfg(feature = "nfsclient")]
pub mod nfs_node;
#[cfg(feature = "nfsserver")]
pub mod nfs_serv;
pub mod nfs_socket;
#[cfg(feature = "nfsserver")]
pub mod nfs_srvcache;
#[cfg(feature = "nfsserver")]
pub mod nfs_srvsubs;
pub mod nfs_subs;
pub mod nfs_syscalls;
pub mod nfs_var;
#[cfg(feature = "nfsclient")]
pub mod nfs_vfsops;
#[cfg(feature = "nfsclient")]
pub mod nfs_vnops;
pub mod nfsdiskless;
pub mod nfsm_subs;
pub mod nfsmount;
pub mod nfsnode;
pub mod nfsproto;
pub mod nfsrvcache;
pub mod rpcv2;
pub mod xdr_subs;
/* </CODE> */
