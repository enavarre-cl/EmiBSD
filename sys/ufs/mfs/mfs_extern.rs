/*	$OpenBSD: mfs_extern.h,v 1.22 2021/10/02 08:51:41 semarie Exp $	*/
/*	$NetBSD: mfs_extern.h,v 1.4 1996/02/09 22:31:27 christos Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1991, 1993
 *	The Regents of the University of California.  All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the University nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE REGENTS AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *	@(#)mfs_extern.h	8.2 (Berkeley) 6/16/94
 */
/* </LICENSES> */

//! `<ufs/mfs/mfs_extern.h>`: the prototypes and the operation table of `ufs/mfs`.
//!
//! Upstream: sys/ufs/mfs/mfs_extern.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The prototypes and `mfs_vops` are the items of `mfs_vfsops.rs` and `mfs_vnops.rs`,
//!   re-exported here (the header's forward declarations of structures need no Rust).

pub use crate::ufs::mfs::mfs_vfsops::{mfs_checkexp, mfs_init, mfs_mount, mfs_start};
pub use crate::ufs::mfs::mfs_vnops::{
    MFS_VOPS, mfs_close, mfs_doio, mfs_inactive, mfs_ioctl, mfs_open, mfs_print, mfs_reclaim,
    mfs_strategy,
};
