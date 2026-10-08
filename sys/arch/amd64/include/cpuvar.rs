/*	$OpenBSD: cpuvar.h,v 1.14 2024/10/22 10:14:49 jsg Exp $	*/
/* 	$NetBSD: cpuvar.h,v 1.1 2003/03/01 18:29:28 fvdl Exp $ */
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

/*
 * Copyright (c) 1999 Stefan Grefen
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
 *      This product includes software developed by the NetBSD
 *      Foundation, Inc. and its contributors.
 * 4. Neither the name of The NetBSD Foundation nor the names of its
 *    contributors may be used to endorse or promote products derived
 *    from this software without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY AUTHOR AND CONTRIBUTORS ``AS IS'' AND ANY
 * EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR AND CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! amd64 `<machine/cpuvar.h>`: how a CPU is attached (`struct cpu_attach_args`).
//!
//! Upstream: sys/arch/amd64/include/cpuvar.h @ 3ce1f3f79392
//!
//! Status: `wip`. The attach arguments, the roles and `struct cpu_functions` are here;
//! `identifycpu` and `cpu_init` (declared by the header) are `cpu.c`/`identcpu.c` and not
//! ported; `mp_cpu_funcs`, `x86_ipi` and the TSC synchronisation are `MULTIPROCESSOR`.
//!
//! ## Deviations
//! - `caa_name` is a byte string; it stays the first member (`#[repr(C)]`), which is what
//!   `mainbus_print` reads from any of mainbus's attach arguments.

use crate::arch::amd64::include::cpu::CpuInfo;

/// `CPU_ROLE_SP`: the only processor of a uniprocessor machine.
pub const CPU_ROLE_SP: i32 = 0;
/// `CPU_ROLE_BP`: the boot processor of a multiprocessor machine.
pub const CPU_ROLE_BP: i32 = 1;
/// `CPU_ROLE_AP`: an application processor.
pub const CPU_ROLE_AP: i32 = 2;

/// `struct cpu_functions`: how to start, stop and clean up after a CPU.
pub struct CpuFunctions {
    /// `start`.
    pub start: Option<fn(&CpuInfo) -> i32>,
    /// `stop`.
    pub stop: Option<fn(&CpuInfo) -> i32>,
    /// `cleanup`.
    pub cleanup: Option<fn(&CpuInfo)>,
}

/// `struct cpu_attach_args`: what `mainbus`, `mpbios` and `acpimadt` hand the `cpu` driver.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CpuAttachArgs {
    /// `caa_name`: "cpu".
    pub caa_name: &'static [u8],
    /// `cpu_apicid`.
    pub cpu_apicid: i32,
    /// `cpu_acpi_proc_id`.
    pub cpu_acpi_proc_id: i32,
    /// `cpu_role` (`CPU_ROLE_*`).
    pub cpu_role: i32,
    /// `cpu_func`.
    pub cpu_func: Option<&'static CpuFunctions>,
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/arch/amd64/include/cpuvar.h");
        for (name, value) in [
            ("CPU_ROLE_SP", CPU_ROLE_SP),
            ("CPU_ROLE_BP", CPU_ROLE_BP),
            ("CPU_ROLE_AP", CPU_ROLE_AP),
        ] {
            assert_eq!(
                crate::reftest::int(&defs, name),
                Some(value.into()),
                "{name}"
            );
        }
    }
}
/* </TESTS> */
