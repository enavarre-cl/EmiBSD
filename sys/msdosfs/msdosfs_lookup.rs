/*	$OpenBSD: msdosfs_lookup.c,v 1.35 2022/08/23 20:37:16 cheloha Exp $	*/
/*	$NetBSD: msdosfs_lookup.c,v 1.34 1997/10/18 22:12:27 ws Exp $	*/
/* <LICENSES> */
/*-
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

//! Directory operations of the msdos file system: `msdosfs_lookup`, `createde`, `removede`,
//! `dosdirempty`, `doscheckpath`, `readep`, `readde`, `uniqdosname`.
//!
//! Upstream: sys/msdosfs/msdosfs_lookup.c @ 3ce1f3f79392
//!
//! Not ported yet (M10c): this module holds only the items `msdosfs_denode.rs` calls, as
//! visible stubs (`unported!`), each tagged `M10C-SHIM`, until the port of
//! `msdosfs_lookup.c` replaces them with the same signatures.
//!
//! ## Deviations
//! - `readep` and `readde` return the busy buffer and the offset of the entry in its data
//!   (the C's `bptoep`), where the C fills `*bpp` and `*epp`; on error the buffer is already
//!   released, as in the C (`*bpp = NULL`).

use crate::msdosfs::denode::Denode;
use crate::msdosfs::msdosfsmount::Msdosfsmount;
use crate::sys::buf::Buf;
use crate::sys::errno::Errno;
use crate::unported;

/// `readep(pmp, dirclust, diroffset, bpp, epp)`: read in the disk block containing the
/// directory entry (`dirclust`, `diroffset`) and return the buf header, and the offset of the
/// directory entry within the block.
// M10C-SHIM: msdosfs_lookup.c
pub fn readep(
    _pmp: &Msdosfsmount,
    _dirclust: u32,
    _diroffset: u32,
) -> Result<(&'static Buf, usize), Errno> {
    Err(unported!("readep"))
}

/// `readde(dep, bpp, epp)`: read in the disk block containing the directory entry `dep` came
/// from and return the buf header, and the offset of the directory entry within the block.
// M10C-SHIM: msdosfs_lookup.c
pub fn readde(_dep: &Denode) -> Result<(&'static Buf, usize), Errno> {
    Err(unported!("readde"))
}
