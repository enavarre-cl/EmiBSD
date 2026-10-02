/*	$OpenBSD: intr.h,v 1.26 2025/12/15 01:39:32 dlg Exp $ */

/*
 * Copyright (c) 2001-2004 Opsycon AB  (www.opsycon.se / www.opsycon.com)
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
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS
 * OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
 * WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY
 * DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 */

//! arm64 `<machine/intr.h>`: interrupt priority levels and the interrupt framework.
//!
//! Upstream: sys/arch/arm64/include/intr.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M3 ports the levels; the `spl*` functions, the interrupt handler
//! structures, the `arm_intr_func` vector and the soft interrupts arrive with M4.

/// `IPL_NONE`: nothing.
pub const IPL_NONE: i32 = 0;
/// `IPL_SOFTCLOCK`: soft clock interrupts.
pub const IPL_SOFTCLOCK: i32 = 2;
/// `IPL_SOFTNET`: soft network interrupts.
pub const IPL_SOFTNET: i32 = 3;
/// `IPL_SOFTTTY`: soft terminal interrupts.
pub const IPL_SOFTTTY: i32 = 4;
/// `IPL_BIO`: block I/O.
pub const IPL_BIO: i32 = 5;
/// `IPL_NET`: network.
pub const IPL_NET: i32 = 6;
/// `IPL_TTY`: terminal.
pub const IPL_TTY: i32 = 7;
/// `IPL_VM`: memory allocation.
pub const IPL_VM: i32 = 8;
/// `IPL_AUDIO`: audio.
pub const IPL_AUDIO: i32 = 9;
/// `IPL_CLOCK`: clock.
pub const IPL_CLOCK: i32 = 10;
/// `IPL_SCHED`.
pub const IPL_SCHED: i32 = IPL_CLOCK;
/// `IPL_STATCLOCK`.
pub const IPL_STATCLOCK: i32 = IPL_CLOCK;
/// `IPL_HIGH`: everything.
pub const IPL_HIGH: i32 = 11;
/// `IPL_IPI`: interprocessor interrupt.
pub const IPL_IPI: i32 = 12;

/// `IPL_MPFLOOR`.
pub const IPL_MPFLOOR: i32 = IPL_TTY;
/// `IPL_IRQMASK`: priority only.
pub const IPL_IRQMASK: i32 = 0xf;
/// `IPL_FLAGMASK`: flags only.
pub const IPL_FLAGMASK: i32 = 0xf00;
/// `IPL_MPSAFE`: 'mpsafe' interrupt, no kernel lock.
pub const IPL_MPSAFE: i32 = 0x100;
/// `IPL_WAKEUP`: 'wakeup' interrupt.
pub const IPL_WAKEUP: i32 = 0x200;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/arch/arm64/include/intr.h");
        let ours: &[(&str, i64)] = &[
            ("IPL_NONE", i64::from(IPL_NONE)),
            ("IPL_SOFTCLOCK", i64::from(IPL_SOFTCLOCK)),
            ("IPL_SOFTNET", i64::from(IPL_SOFTNET)),
            ("IPL_SOFTTTY", i64::from(IPL_SOFTTTY)),
            ("IPL_BIO", i64::from(IPL_BIO)),
            ("IPL_NET", i64::from(IPL_NET)),
            ("IPL_TTY", i64::from(IPL_TTY)),
            ("IPL_VM", i64::from(IPL_VM)),
            ("IPL_AUDIO", i64::from(IPL_AUDIO)),
            ("IPL_CLOCK", i64::from(IPL_CLOCK)),
            ("IPL_HIGH", i64::from(IPL_HIGH)),
            ("IPL_IPI", i64::from(IPL_IPI)),
            ("IPL_IRQMASK", i64::from(IPL_IRQMASK)),
            ("IPL_FLAGMASK", i64::from(IPL_FLAGMASK)),
            ("IPL_MPSAFE", i64::from(IPL_MPSAFE)),
            ("IPL_WAKEUP", i64::from(IPL_WAKEUP)),
        ];
        for (name, value) in ours {
            assert_eq!(crate::reftest::int(&defs, name), Some(*value), "{name}");
        }
    }
}
