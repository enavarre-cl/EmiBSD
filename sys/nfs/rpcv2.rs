/*	$OpenBSD: rpcv2.h,v 1.8 2008/07/04 16:49:28 blambert Exp $	*/
/*	$NetBSD: rpcv2.h,v 1.8 1996/02/18 11:54:11 fvdl Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1989, 1993
 *	The Regents of the University of California.  All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * Rick Macklem at The University of Guelph.
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
 *	@(#)rpcv2.h	8.2 (Berkeley) 3/30/95
 */
/* </LICENSES> */

/* <CODE> */
//! `<nfs/rpcv2.h>`: definitions for Sun RPC version 2, from "RPC: Remote Procedure Call
//! Protocol Specification" (RFC 1057): the message, authentication and reply constants and the
//! MOUNT program's numbers.
//!
//! Upstream: sys/nfs/rpcv2.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - None: constants only. They are `u32` (the XDR words they become through
//!   `txdr_unsigned`), the sizes `usize`.

/// `RPC_VER2`: version number.
pub const RPC_VER2: u32 = 2;

/// `RPCAUTH_NULL`: authentication flavours.
pub const RPCAUTH_NULL: u32 = 0;
/// `RPCAUTH_UNIX`.
pub const RPCAUTH_UNIX: u32 = 1;
/// `RPCAUTH_SHORT`.
pub const RPCAUTH_SHORT: u32 = 2;
/// `RPCAUTH_KERB4`.
pub const RPCAUTH_KERB4: u32 = 4;
/// `RPCAUTH_MAXSIZ`.
pub const RPCAUTH_MAXSIZ: usize = 400;
/// `RPCVERF_MAXSIZ`: for Kerb, can actually be 400.
pub const RPCVERF_MAXSIZ: usize = 12;
/// `RPCAUTH_UNIXGIDS`.
pub const RPCAUTH_UNIXGIDS: usize = 16;

/// `RPCAKN_FULLNAME`: constants associated with authentication flavours.
pub const RPCAKN_FULLNAME: u32 = 0;
/// `RPCAKN_NICKNAME`.
pub const RPCAKN_NICKNAME: u32 = 1;

/// `RPC_CALL`: RPC constants.
pub const RPC_CALL: u32 = 0;
/// `RPC_REPLY`.
pub const RPC_REPLY: u32 = 1;
/// `RPC_MSGACCEPTED`.
pub const RPC_MSGACCEPTED: u32 = 0;
/// `RPC_MSGDENIED`.
pub const RPC_MSGDENIED: u32 = 1;
/// `RPC_PROGUNAVAIL`.
pub const RPC_PROGUNAVAIL: u32 = 1;
/// `RPC_PROGMISMATCH`.
pub const RPC_PROGMISMATCH: u32 = 2;
/// `RPC_PROCUNAVAIL`.
pub const RPC_PROCUNAVAIL: u32 = 3;
/// `RPC_GARBAGE`: I like this one.
pub const RPC_GARBAGE: u32 = 4;
/// `RPC_MISMATCH`.
pub const RPC_MISMATCH: u32 = 0;
/// `RPC_AUTHERR`.
pub const RPC_AUTHERR: u32 = 1;

/// `AUTH_BADCRED`: authentication failures.
pub const AUTH_BADCRED: u32 = 1;
/// `AUTH_REJECTCRED`.
pub const AUTH_REJECTCRED: u32 = 2;
/// `AUTH_BADVERF`.
pub const AUTH_BADVERF: u32 = 3;
/// `AUTH_REJECTVERF`.
pub const AUTH_REJECTVERF: u32 = 4;
/// `AUTH_TOOWEAK`: give em wheaties.
pub const AUTH_TOOWEAK: u32 = 5;

/// `RPC_SIZ`: sizes of RPC header parts.
pub const RPC_SIZ: usize = 24;
/// `RPC_REPLYSIZ`.
pub const RPC_REPLYSIZ: usize = 28;

/// `RPCPROG_MNT`: RPC program definitions.
pub const RPCPROG_MNT: u32 = 100005;
/// `RPCMNT_VER1`.
pub const RPCMNT_VER1: u32 = 1;
/// `RPCMNT_VER3`.
pub const RPCMNT_VER3: u32 = 3;
/// `RPCMNT_MOUNT`.
pub const RPCMNT_MOUNT: u32 = 1;
/// `RPCMNT_DUMP`.
pub const RPCMNT_DUMP: u32 = 2;
/// `RPCMNT_UMOUNT`.
pub const RPCMNT_UMOUNT: u32 = 3;
/// `RPCMNT_UMNTALL`.
pub const RPCMNT_UMNTALL: u32 = 4;
/// `RPCMNT_EXPORT`.
pub const RPCMNT_EXPORT: u32 = 5;
/// `RPCMNT_NAMELEN`.
pub const RPCMNT_NAMELEN: usize = 255;
/// `RPCMNT_PATHLEN`.
pub const RPCMNT_PATHLEN: usize = 1024;
/// `RPCPROG_NFS`.
pub const RPCPROG_NFS: u32 = 100003;
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn constants_match_the_c_header() {
        let defs = crate::reftest::defines("sys/nfs/rpcv2.h");
        let ours: &[(&str, i64)] = &[
            ("RPC_VER2", RPC_VER2.into()),
            ("RPCAUTH_NULL", RPCAUTH_NULL.into()),
            ("RPCAUTH_UNIX", RPCAUTH_UNIX.into()),
            ("RPCAUTH_SHORT", RPCAUTH_SHORT.into()),
            ("RPCAUTH_KERB4", RPCAUTH_KERB4.into()),
            ("RPCAUTH_MAXSIZ", RPCAUTH_MAXSIZ as i64),
            ("RPCVERF_MAXSIZ", RPCVERF_MAXSIZ as i64),
            ("RPCAUTH_UNIXGIDS", RPCAUTH_UNIXGIDS as i64),
            ("RPCAKN_FULLNAME", RPCAKN_FULLNAME.into()),
            ("RPCAKN_NICKNAME", RPCAKN_NICKNAME.into()),
            ("RPC_CALL", RPC_CALL.into()),
            ("RPC_REPLY", RPC_REPLY.into()),
            ("RPC_MSGACCEPTED", RPC_MSGACCEPTED.into()),
            ("RPC_MSGDENIED", RPC_MSGDENIED.into()),
            ("RPC_PROGUNAVAIL", RPC_PROGUNAVAIL.into()),
            ("RPC_PROGMISMATCH", RPC_PROGMISMATCH.into()),
            ("RPC_PROCUNAVAIL", RPC_PROCUNAVAIL.into()),
            ("RPC_GARBAGE", RPC_GARBAGE.into()),
            ("RPC_MISMATCH", RPC_MISMATCH.into()),
            ("RPC_AUTHERR", RPC_AUTHERR.into()),
            ("AUTH_BADCRED", AUTH_BADCRED.into()),
            ("AUTH_REJECTCRED", AUTH_REJECTCRED.into()),
            ("AUTH_BADVERF", AUTH_BADVERF.into()),
            ("AUTH_REJECTVERF", AUTH_REJECTVERF.into()),
            ("AUTH_TOOWEAK", AUTH_TOOWEAK.into()),
            ("RPC_SIZ", RPC_SIZ as i64),
            ("RPC_REPLYSIZ", RPC_REPLYSIZ as i64),
            ("RPCPROG_MNT", RPCPROG_MNT.into()),
            ("RPCMNT_VER1", RPCMNT_VER1.into()),
            ("RPCMNT_VER3", RPCMNT_VER3.into()),
            ("RPCMNT_MOUNT", RPCMNT_MOUNT.into()),
            ("RPCMNT_DUMP", RPCMNT_DUMP.into()),
            ("RPCMNT_UMOUNT", RPCMNT_UMOUNT.into()),
            ("RPCMNT_UMNTALL", RPCMNT_UMNTALL.into()),
            ("RPCMNT_EXPORT", RPCMNT_EXPORT.into()),
            ("RPCMNT_NAMELEN", RPCMNT_NAMELEN as i64),
            ("RPCMNT_PATHLEN", RPCMNT_PATHLEN as i64),
            ("RPCPROG_NFS", RPCPROG_NFS.into()),
        ];
        for (name, value) in ours {
            assert_eq!(crate::reftest::int(&defs, name), Some(*value), "{name}");
        }
    }
}
/* </TESTS> */
