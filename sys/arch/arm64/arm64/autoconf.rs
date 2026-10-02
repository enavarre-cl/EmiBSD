/*	$OpenBSD: autoconf.c,v 1.18 2026/06/23 11:45:54 kettenis Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2009 Miodrag Vallat.
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */
/* </LICENSES> */

//! Setup the system to run on the current machine: `arch/arm64/arm64/autoconf.c`.
//!
//! Upstream: sys/arch/arm64/arm64/autoconf.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M4 ports `cpu_configure` as far as the interrupts go;
//! `diskconf`, `device_register` and the root-device search arrive with autoconfiguration
//! (M5). `cold` lives in `machdep.rs`, where the C defines it.
//!
//! ## Deviations
//! - `cpu_configure` has no `config_rootfound("mainbus")`: `bus_dma_init`, `unmap_startup`
//!   and `cpu_identify_cleanup` are reported, and the interrupt controller is attached by
//!   `attach_interrupt_controller`, which finds the GIC's node and builds its
//!   `fdt_attach_args` as `simplebus` would.

use core::sync::atomic::Ordering;

use crate::arch::arm64::arm64::bus_space::ARM64_BS_TAG;
use crate::arch::arm64::arm64::intr::arm_intr_init_fdt;
use crate::arch::arm64::arm64::machdep::COLD;
use crate::arch::arm64::dev::ampintc::{ampintc_attach, ampintc_match};
use crate::arch::arm64::include::fdt::FdtAttachArgs;
use crate::dev::ofw::fdt::{FdtReg, fdt_get_reg, of_fdt_node};
use crate::dev::ofw::openfirm::{OF_child, OF_getpropint, OF_peer};
use crate::kern::kern_softintr::softintr_init;
use crate::kern::subr_prf::printf;
use crate::machine::intr::{spl0, splhigh};
use crate::unported;

/// `cpu_configure`: determine i/o configuration for a machine.
pub fn cpu_configure() {
    splhigh();

    softintr_init();
    let _ = unported!("bus_dma_init (M7)");

    // config_rootfound("mainbus", NULL): autoconfiguration (M5). Of what mainbus and
    // simplebus would attach, the interrupt controller:
    arm_intr_init_fdt();
    attach_interrupt_controller();

    let _ = unported!("unmap_startup (M6)");

    let _ = unported!("cpu_identify_cleanup (M4-b)");

    // CRYPTO: not configured.

    COLD.store(false, Ordering::Relaxed);
    spl0();
}

/// What `mainbus`/`simplebus` do for the interrupt controller until autoconfiguration
/// (M5): find the GIC's node, build its attach arguments from `reg`, attach it.
fn attach_interrupt_controller() {
    let mut node = OF_child(OF_peer(0));
    while node != 0 {
        let mut regs = [FdtReg::default(); 2];
        let mut faa = FdtAttachArgs {
            fa_name: b"",
            fa_node: node,
            fa_iot: &ARM64_BS_TAG,
            fa_reg: &[],
            fa_intr: &[],
            fa_acells: OF_getpropint(OF_peer(0), b"#address-cells", 1) as i32,
            fa_scells: OF_getpropint(OF_peer(0), b"#size-cells", 1) as i32,
        };
        if ampintc_match(&faa) {
            let fnode = of_fdt_node(node);
            if fdt_get_reg(fnode, 0, &mut regs[0]).is_err()
                || fdt_get_reg(fnode, 1, &mut regs[1]).is_err()
            {
                printf(format_args!("ampintc0: no registers\n"));
                return;
            }
            faa.fa_reg = &regs;
            ampintc_attach(&faa);
            return;
        }
        node = OF_peer(node);
    }
    printf(format_args!("no interrupt controller in the device tree\n"));
}
