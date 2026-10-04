/*	$OpenBSD: msdosfs_vnops.c,v 1.143 2024/10/18 05:52:32 miod Exp $	*/
/*	$NetBSD: msdosfs_vnops.c,v 1.63 1997/10/17 11:24:19 ws Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (C) 2005 Thomas Wang.
 * Copyright (C) 1994, 1995, 1997 Wolfgang Solfrank.
 * Copyright (C) 1994, 1995, 1997 TooLs GmbH.
 * All rights reserved.
 * Original code by Paul Popelka (paulp@uts.amdahl.com) (see below).
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *	This product includes software developed by TooLs GmbH.
 * 4. The name of TooLs GmbH may not be used to endorse or promote products
 *    derived from this software without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY TOOLS GMBH ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL TOOLS GMBH BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
 * SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
 * PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS;
 * OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
 * WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR
 * OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF
 * ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */
/*
 * Written by Paul Popelka (paulp@uts.amdahl.com)
 *
 * You can do anything you want with this software, just don't say you wrote
 * it, and don't remove this notice.
 *
 * This software is provided "as is".
 *
 * The author supplies this software to be publicly redistributed on the
 * understanding that the author is not responsible for the correct
 * functioning of this software in any circumstances and is not liable for
 * any damages caused by this software.
 *
 * October 1992
 */
/* </LICENSES> */

//! The vnode operations of the msdos file system (`msdosfs_vops`) and their functions.
//!
//! Upstream: sys/msdosfs/msdosfs_vnops.c @ 3ce1f3f79392
//!
//! Not ported yet (M10c): this module holds only `MSDOSFS_VOPS`, which `deget` hands to
//! `getnewvnode`, tagged `M10C-SHIM`. It fills the two operations that exist so far
//! (`msdosfs_inactive` and `msdosfs_reclaim`, which the C defines in `msdosfs_denode.c`);
//! every other slot is `None` (`EOPNOTSUPP`) until the port of `msdosfs_vnops.c` replaces the
//! table with the C's.

use crate::msdosfs::msdosfs_denode::{msdosfs_inactive, msdosfs_reclaim};
use crate::sys::vnode::Vops;

/// `msdosfs_vops`: the vnode operations vector of msdos vnodes.
// M10C-SHIM: msdosfs_vnops.c
pub static MSDOSFS_VOPS: Vops = Vops {
    vop_inactive: Some(msdosfs_inactive),
    vop_reclaim: Some(msdosfs_reclaim),
    ..Vops::EMPTY
};
