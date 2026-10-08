/*	$OpenBSD: tprintf.h,v 1.6 2003/08/24 01:27:07 avsm Exp $	*/
/*	$NetBSD: tprintf.h,v 1.10 1996/04/09 20:55:43 cgd Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1990, 1993
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
 *	@(#)tprintf.h	8.1 (Berkeley) 6/2/93
 */
/* </LICENSES> */

/* <CODE> */
//! `<sys/tprintf.h>`: the handle `tprintf(9)` sends a message to a process's controlling
//! terminal through.
//!
//! Upstream: sys/sys/tprintf.h @ 3ce1f3f79392
//!
//! The functions the header declares are `kern/subr_prf.rs`'s (`tprintf_open`,
//! `tprintf_close`, `tprintf`), compiled with feature `nfsclient` or `nfsserver`, the only
//! users, as the C's `#if defined(NFSSERVER) || defined(NFSCLIENT)` does.
//!
//! ## Deviations
//! - `tpr_t` (`struct session *`, NULL when the process has no controlling terminal) is the
//!   type alias [`Tpr`], an `Option` of a session reference.

use crate::sys::proc::Session;

/// `tpr_t`: a `tprintf_open` handle, the session whose controlling terminal gets the message,
/// or `None` (the C's NULL) when the process cannot be printed to.
pub type Tpr = Option<&'static Session>;
/* </CODE> */
