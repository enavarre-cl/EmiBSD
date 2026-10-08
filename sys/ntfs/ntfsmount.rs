/*	$OpenBSD: ntfsmount.h,v 1.3 2006/05/29 20:40:58 miod Exp $	*/
/*	$NetBSD: ntfsmount.h,v 1.1 2002/12/23 17:38:34 jdolecek Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1998, 1999 Semen Ustimenko
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *	Id: ntfsmount.h,v 1.4 1999/05/12 09:43:09 semenu Exp
 */
/* </LICENSES> */

/* <CODE> */
//! `<ntfs/ntfsmount.h>`: the NTFS mount options (`ntfs_args.flag`, kept in `ntm_flag`).
//!
//! Upstream: sys/ntfs/ntfsmount.h @ 3ce1f3f79392
//!
//! `<sys/mount.h>` defines the same two flags beside `struct ntfs_args` (`sys/sys/mount.rs`).

/// `NTFS_MFLAG_CASEINS`: case-insensitive name lookups.
pub const NTFS_MFLAG_CASEINS: u64 = 0x0000_0001;
/// `NTFS_MFLAG_ALLNAMES`: list the DOS names too.
pub const NTFS_MFLAG_ALLNAMES: u64 = 0x0000_0002;
/* </CODE> */
