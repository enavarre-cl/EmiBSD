/*	$OpenBSD: openfirm.h,v 1.23 2026/06/22 21:12:12 kettenis Exp $	*/
/*	$NetBSD: openfirm.h,v 1.1 1996/09/30 16:35:10 ws Exp $	*/
/*
 * Copyright (C) 1995, 1996 Wolfgang Solfrank.
 * Copyright (C) 1995, 1996 TooLs GmbH.
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

//! `<dev/ofw/openfirm.h>`: the OpenFirmware interface, which `dev/ofw/fdt.c` implements over
//! the device tree blob on this architecture.
//!
//! Upstream: sys/dev/ofw/openfirm.h @ 3ce1f3f79392
//!
//! Status: `wip`. The `OF_*` functions live in `dev/ofw/fdt.rs`, as the C implements them
//! there; this module re-exports them and `OFMAXPARAM`. `struct ofprobe`, `struct ofb_softc`
//! (autoconfiguration, M5) and the real-OpenFirmware calls (`OF_call_method`, `OF_open`,
//! `OF_boot`, ...) are for machines with a firmware this kernel does not run on.

pub use crate::dev::ofw::fdt::{
    OF_child, OF_finddevice, OF_getindex, OF_getnodebyname, OF_getnodebyphandle, OF_getprop,
    OF_getpropbool, OF_getpropint, OF_getpropint64, OF_getpropint64array, OF_getpropintarray,
    OF_getproplen, OF_is_compatible, OF_is_enabled, OF_parent, OF_peer, OFMAXPARAM,
};
