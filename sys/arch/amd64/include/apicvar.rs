/*	$OpenBSD: apicvar.h,v 1.4 2025/09/05 16:57:48 kettenis Exp $	*/
/* 	$NetBSD: apicvar.h,v 1.1 2003/02/26 21:26:10 fvdl Exp $ */
/* <LICENSES> */
/*-
 * Copyright (c) 2000 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by RedBack Networks Inc.
 *
 * Author: Bill Sommerfeld
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
 * THIS SOFTWARE IS PROVIDED BY THE NETBSD FOUNDATION, INC. AND CONTRIBUTORS
 * ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE FOUNDATION OR CONTRIBUTORS
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! amd64 `<machine/apicvar.h>`: what a local or I/O APIC is attached with.
//!
//! Upstream: sys/arch/amd64/include/apicvar.h @ 3ce1f3f79392
//!
//! `apic_format_redir` is `amd64/apic.rs`.

use crate::arch::amd64::amd64::bus_space::X86BusSpace;
use crate::machine::bus::BusAddr;

/// `IOAPIC_PICMODE`: the I/O APIC starts behind the 8259s (the IMCR routes them).
pub const IOAPIC_PICMODE: i32 = 0x01;
/// `IOAPIC_VWIRE`: virtual wire mode.
pub const IOAPIC_VWIRE: i32 = 0x02;

/// `struct apic_attach_args`.
#[repr(C)]
pub struct ApicAttachArgs {
    /// `aaa_name`: first, as in every mainbus child's arguments (`mainbus_print` reads it).
    pub aaa_name: &'static [u8],
    /// `apic_id`.
    pub apic_id: i32,
    /// `apic_version`.
    pub apic_version: i32,
    /// `flags`: `IOAPIC_PICMODE`, `IOAPIC_VWIRE`.
    pub flags: i32,
    /// `apic_memt`.
    pub apic_memt: X86BusSpace,
    /// `apic_address`.
    pub apic_address: BusAddr,
    /// `apic_vecbase`: the first global interrupt, -1 when the table gives none.
    pub apic_vecbase: i32,
}
/* </CODE> */
