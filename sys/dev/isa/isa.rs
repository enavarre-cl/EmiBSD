/*	$OpenBSD: isa.c,v 1.52 2025/09/16 12:18:10 hshoexer Exp $	*/
/*	$NetBSD: isa.c,v 1.85 1996/05/14 00:31:04 thorpej Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1997, Jason Downs.  All rights reserved.
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
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR(S) ``AS IS'' AND ANY EXPRESS
 * OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
 * WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
 * DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR(S) BE LIABLE FOR ANY DIRECT,
 * INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
 * (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
 * SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
 * CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 */

/*-
 * Copyright (c) 1993, 1994 Charles Hannum.  All rights reserved.
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
 *	This product includes software developed by Charles Hannum.
 * 4. The name of the author may not be used to endorse or promote products
 *    derived from this software without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT, INDIRECT,
 * INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT
 * NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
 * DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
 * THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF
 * THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

//! The ISA bus: `dev/isa/isa.c`.
//!
//! Upstream: sys/dev/isa/isa.c @ 3ce1f3f79392
//!
//! `isa` is an indirect-configuration bus (`CD_INDIRECT`): `isascan` gets a softc already made
//! for every `foo at isa?` line, fills the attach arguments from the line's locators (`port`,
//! `irq`, ...) and attaches the driver if its probe answers.
//!
//! ## Deviations
//! - `NISADMA` and `NISAPNP` are 0 (`isavar.rs`): `isaattach` maps the delay port alone,
//!   `isascan` allocates no DRQ, and `isapnp_isa_attach_hook` is not called.
//! - `NACPI` is 0 (`acpi.c` is not ported), so `acpi_legacy_free` is never consulted.
//! - `isa_intr_typename` returns a `&'static str`.

use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::Ordering;

use crate::dev::isa::isareg::IO_DMAPG;
use crate::dev::isa::isavar::{
    DRQUNK, IRQUNK, ISAPNP_MAX_DEVCLASS, ISAPNP_MAX_IDENT, IsaAttachArgs, IsaSoftc,
    IsabusAttachArgs, cf_drq, cf_drq2, cf_iobase, cf_irq, cf_maddr, cf_msize,
};
use crate::kern::init_main::BOOTHOWTO;
use crate::kern::kern_malloc::free;
use crate::kern::subr_autoconf::{AUTOCONF_VERBOSE, config_attach, config_make_softc, config_scan};
use crate::kern::subr_prf::{Str, panic, printf};
use crate::machine::bus::bus_space_map;
use crate::machine::isa_machdep::{
    IST_EDGE, IST_LEVEL, IST_NONE, IST_PULSE, isa_attach_hook, isa_intr_check,
};
use crate::sys::device::{
    CD_COCOVM, CD_INDIRECT, CfMatch, Cfattach, Cfdata, Cfdriver, DV_DULL, Device, FSTATE_STAR,
    SoftcMatch, UNCONF,
};
use crate::sys::malloc::M_DEVBUF;
use crate::sys::reboot::RB_COCOVM;

/// `isa_ca`.
pub static ISA_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<IsaSoftc>(),
    ca_match: Some(isamatch),
    ca_attach: isaattach,
    ca_detach: None,
    ca_activate: None,
};

/// `isa_cd`.
pub static ISA_CD: Cfdriver = Cfdriver::new(b"isa", DV_DULL, CD_INDIRECT | CD_COCOVM);

/// `isamatch`: the bus whose name the attach arguments carry.
pub fn isamatch(_parent: Option<&Device>, match_: &CfMatch, aux: *mut c_void) -> i32 {
    let cf = match_.cfdata();
    // SAFETY: an `isa` parent hands its children `isabus_attach_args` (`mainbus`'s `mba_iba`).
    let iba = unsafe { &*aux.cast::<IsabusAttachArgs>() };

    if iba.iba_busname != cf.cf_driver.cd_name {
        return 0;
    }

    // NACPI > 0: acpi_legacy_free (not configured).
    1
}

/// `isaattach`: record the bus's tags, map the delay port and attach the children.
pub fn isaattach(parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `isa_ca`'s devices are `IsaSoftc`s.
    let sc = unsafe { self_.softc::<IsaSoftc>() };
    // SAFETY: as in `isamatch`.
    let iba = unsafe { &*aux.cast::<IsabusAttachArgs>() };

    isa_attach_hook(parent, self_);
    printf(format_args!("\n"));

    sc.sc_iot.set(Some(iba.iba_iot));
    sc.sc_memt.set(Some(iba.iba_memt));
    // NISADMA > 0: sc_dmat (not configured).
    sc.sc_ic.set(Some(iba.iba_ic));

    // NISAPNP > 0: isapnp_isa_attach_hook(sc) (not configured).

    // NISADMA == 0: map port 0x84, which causes a 1.25us delay when read.
    // SAFETY: the DMA page register block of the PC platform; this port is only ever read.
    match unsafe { bus_space_map(iba.iba_iot, usize::from(IO_DMAPG) + 0x4, 0x1, 0) } {
        Ok(h) => sc.sc_delaybah.set(Some(h)),
        Err(_) => panic(format_args!("isaattach: can't map `delay port'")), // XXX
    }

    sc.sc_subdevs.init();
    config_scan(isascan, self_);
}

/// `isaprint`: print the resources of a child that found no driver.
pub fn isaprint(aux: *mut c_void, _isa: Option<&[u8]>) -> i32 {
    // SAFETY: `isascan` hands its children `isa_attach_args`.
    let ia = unsafe { &*aux.cast::<IsaAttachArgs>() };

    if ia.ia_iosize() != 0 {
        printf(format_args!(" port 0x{:x}", ia.ia_iobase()));
    }
    if ia.ia_iosize() > 1 {
        printf(format_args!("/{}", ia.ia_iosize()));
    }

    if ia.ia_msize() != 0 {
        printf(format_args!(" iomem 0x{:x}", ia.ia_maddr()));
    }
    if ia.ia_msize() > 1 {
        printf(format_args!("/{}", ia.ia_msize()));
    }

    let mut nirq = usize::from(ia.ipa_nirq);
    if nirq > ia.ipa_irq.len() {
        nirq = 1;
    }
    for pin in &ia.ipa_irq[..nirq] {
        if i32::from(pin.num) != IRQUNK {
            printf(format_args!(" irq {}", pin.num));
        }
    }

    let mut ndma = usize::from(ia.ipa_ndrq);
    if ndma > ia.ipa_drq.len() {
        ndma = 2;
    }
    for (dma, pin) in ia.ipa_drq[..ndma].iter().enumerate() {
        if i32::from(pin.num) != DRQUNK {
            if dma == 0 {
                printf(format_args!(" drq"));
            } else {
                printf(format_args!(" drq{}", dma + 1));
            }
            printf(format_args!(" {}", pin.num));
        }
    }

    UNCONF
}

/// Frees a softc `config_scan`/`config_make_softc` made for a probe that did not attach
/// (`free(dev, M_DEVBUF, cf->cf_attach->ca_devsize)`).
fn free_softc(dev: SoftcMatch, cf: &Cfdata) {
    free(dev.into_raw().cast(), M_DEVBUF, cf.cf_attach.ca_devsize);
}

/// `isascan`: probe and attach one `foo at isa?` line (a starred line as many times as its
/// probe answers).
pub fn isascan(parent: &Device, match_: CfMatch) {
    // SAFETY: `isa_ca`'s devices are `IsaSoftc`s.
    let sc = unsafe { parent.softc::<IsaSoftc>() };
    let CfMatch::Softc(mut dev) = match_ else {
        // isa is CD_INDIRECT: config_scan always hands a softc.
        return;
    };
    let cf: &'static Cfdata = dev.device().cfdata();

    let (Some(iot), Some(memt), Some(ic)) = (sc.sc_iot.get(), sc.sc_memt.get(), sc.sc_ic.get())
    else {
        free_softc(dev, cf);
        return;
    };
    let mut ia = IsaAttachArgs {
        ia_isa: ptr::null_mut(),
        ia_iot: iot,
        ia_memt: memt,
        ia_delaybah: sc.sc_delaybah.get(),
        ia_ic: ic,
        ipa_sibling: ptr::null_mut(),
        ipa_child: ptr::null_mut(),
        ipa_devident: [0; ISAPNP_MAX_IDENT],
        ipa_devlogic: [0; ISAPNP_MAX_DEVCLASS],
        ipa_devcompat: [0; ISAPNP_MAX_DEVCLASS],
        ipa_devclass: [0; ISAPNP_MAX_DEVCLASS],
        ipa_pref: 0,
        ipa_devnum: 0,
        ipa_nio: 0,
        ipa_nirq: 0,
        ipa_ndrq: 0,
        ipa_nmem: 0,
        ipa_nmem32: 0,
        ipa_io: Default::default(),
        ipa_mem: Default::default(),
        ipa_mem32: Default::default(),
        ipa_irq: Default::default(),
        ipa_drq: Default::default(),
        ia_aux: ptr::null_mut(),
    };
    ia.set_ia_iobase(cf_iobase(cf) as i32);
    ia.set_ia_iosize(0x666);
    ia.set_ia_maddr(cf_maddr(cf) as i32);
    ia.set_ia_msize(cf_msize(cf) as i32);
    ia.set_ia_irq(if cf_irq(cf) == 2 {
        9
    } else {
        cf_irq(cf) as i32
    });
    ia.ipa_nirq = if ia.ia_irq() == IRQUNK { 0 } else { 1 };
    ia.set_ia_drq(cf_drq(cf) as i32);
    ia.set_ia_drq2(cf_drq2(cf) as i32);
    ia.ipa_ndrq = 2;

    if BOOTHOWTO.load(Ordering::Relaxed) & RB_COCOVM != 0 && cf.cf_driver.cd_mode & CD_COCOVM == 0 {
        free_softc(dev, cf);
        return;
    }

    let verbose = AUTOCONF_VERBOSE.load(Ordering::Relaxed) != 0;
    let Some(ca_match) = cf.cf_attach.ca_match else {
        free_softc(dev, cf);
        return;
    };

    if cf.cf_fstate.get() == FSTATE_STAR {
        let mut ia2 = ia;

        if verbose {
            printf(format_args!(
                ">>> probing for {}*\n",
                Str(cf.cf_driver.cd_name)
            ));
        }
        loop {
            let m = CfMatch::Softc(dev);
            let pri = ca_match(Some(parent), &m, ptr::from_mut(&mut ia2).cast());
            let CfMatch::Softc(d) = m else {
                return;
            };
            dev = d;
            if pri <= 0 {
                break;
            }
            // !__NO_ISA_INTR_CHECK
            if ia2.ia_irq() != IRQUNK && isa_intr_check(ia2.ia_ic, ia2.ia_irq(), IST_EDGE) == 0 {
                printf(format_args!(
                    "{}{}: irq {} already in use\n",
                    Str(cf.cf_driver.cd_name),
                    cf.cf_unit.get(),
                    ia2.ia_irq()
                ));
                break;
            }

            if verbose {
                printf(format_args!(
                    ">>> probe for {}* clone into {}{}\n",
                    Str(cf.cf_driver.cd_name),
                    Str(cf.cf_driver.cd_name),
                    cf.cf_unit.get()
                ));
            }
            if ia2.ia_iosize() == 0x666 {
                printf(format_args!(
                    "{}: iosize not repaired by driver\n",
                    sc.sc_dev.xname()
                ));
                ia2.set_ia_iosize(0);
            }
            let _ = config_attach(
                Some(parent),
                CfMatch::Softc(dev),
                ptr::from_mut(&mut ia2).cast(),
                Some(isaprint),
            );
            // SAFETY: a fresh softc, owned by this loop until it attaches or is freed.
            dev = unsafe { SoftcMatch::new(config_make_softc(Some(parent), cf)) };
            // NISADMA > 0: ISA_DRQ_ALLOC (not configured).
            ia2 = ia;
        }
        if verbose {
            printf(format_args!(
                ">>> probing for {}* finished\n",
                Str(cf.cf_driver.cd_name)
            ));
        }
        free_softc(dev, cf);
        return;
    }

    if verbose {
        printf(format_args!(
            ">>> probing for {}{}\n",
            Str(cf.cf_driver.cd_name),
            cf.cf_unit.get()
        ));
    }
    let m = CfMatch::Softc(dev);
    let pri = ca_match(Some(parent), &m, ptr::from_mut(&mut ia).cast());
    let CfMatch::Softc(dev) = m else {
        return;
    };
    if pri > 0 {
        // !__NO_ISA_INTR_CHECK
        if ia.ia_irq() != IRQUNK && isa_intr_check(ia.ia_ic, ia.ia_irq(), IST_EDGE) == 0 {
            printf(format_args!(
                "{}{}: irq {} already in use\n",
                Str(cf.cf_driver.cd_name),
                cf.cf_unit.get(),
                ia.ia_irq()
            ));
            free_softc(dev, cf);
        } else {
            if verbose {
                printf(format_args!(
                    ">>> probing for {}{} succeeded\n",
                    Str(cf.cf_driver.cd_name),
                    cf.cf_unit.get()
                ));
            }
            let _ = config_attach(
                Some(parent),
                CfMatch::Softc(dev),
                ptr::from_mut(&mut ia).cast(),
                Some(isaprint),
            );
            // NISADMA > 0: ISA_DRQ_ALLOC (not configured).
        }
    } else {
        if verbose {
            printf(format_args!(
                ">>> probing for {}{} failed\n",
                Str(cf.cf_driver.cd_name),
                cf.cf_unit.get()
            ));
        }
        free_softc(dev, cf);
    }
}

/// `isa_intr_typename`: the name of an interrupt sharing type.
pub fn isa_intr_typename(type_: i32) -> &'static str {
    match type_ {
        IST_NONE => "none",
        IST_PULSE => "pulsed",
        IST_EDGE => "edge-triggered",
        IST_LEVEL => "level-triggered",
        _ => panic(format_args!("isa_intr_typename: invalid type {type_}")),
    }
}
