/*	$OpenBSD: piixpm.c,v 1.44 2024/05/24 06:02:58 jsg Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2005, 2006 Alexander Yurchenko <grange@openbsd.org>
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

/* <CODE> */
//! `piixpm*`: Intel PIIX and compatible Power Management controller driver, the SMBus part
//! (`piixpm* at pci?`, `iic* at piixpm?`), see `piixpm(4)`.
//!
//! Upstream: sys/dev/pci/piixpm.c @ 3ce1f3f79392
//!
//! The SMBus host controller in the power management function of the PIIX4 (QEMU's i440fx
//! `pc` machine has it at 00:01.3, `PCI_PRODUCT_INTEL_82371AB_PM`) and of AMD's, ATI's,
//! ServerWorks' and SMSC's compatibles. It does the SMBus transactions of a command byte and up
//! to two data bytes itself (`ic_exec` only), by interrupt when the function is set to IRQ and
//! the PCI interrupt is routed, else by polling (SMI mode); the `cold` autoconfiguration scan
//! always polls. On AMD's SB800 and later (FCH) the registers are found through the
//! index/data pair at I/O ports 0xcd6/0xcd7 and the controller serves up to four (SB800) or
//! two (FCH) buses, each its own `iic*`, selected through the same pair when the bus is
//! acquired.
//!
//! ## Deviations
//! - The C's `struct piixpm_smbus` back pointer (`sb_sc`) is a `Cell<*const PiixpmSoftc>`, the
//!   cookie of each bus's `i2c_controller` is its `PiixpmSmbus`.
//! - `sc_i2c_xfer.error` is an atomic (the C's `volatile int`, written by the interrupt
//!   handler); `buf` is cleared when an exec call returns, and the interrupt handler reads
//!   no data when it is null: a late interrupt after a timeout cannot write into the caller's
//!   buffer (the C could).
//! - `piixpm_i2c_acquire_bus` returns the `Errno` of `rw_enter` as its positive `int`.
//! - The attach's "can't map i/o space" after the SB800 mapping failed prints no newline, as in C.
//! - `PIIXPM_DEBUG` (the `DPRINTF`s) is not configured.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI32, Ordering};

use crate::dev::i2c::i2c::iicbus_print;
use crate::dev::i2c::i2c_io::{I2cAddr, I2cOp};
use crate::dev::i2c::i2cvar::{I2C_F_POLL, I2cController, I2cbusAttachArgs};
use crate::dev::pci::pci::pci_matchbyid;
use crate::dev::pci::pcidevs::{
    PCI_PRODUCT_AMD_HUDSON2_SMB, PCI_PRODUCT_AMD_KERNCZ_SMB, PCI_PRODUCT_ATI_SB200_SMB,
    PCI_PRODUCT_ATI_SB300_SMB, PCI_PRODUCT_ATI_SB400_SMB, PCI_PRODUCT_ATI_SBX00_SMB,
    PCI_PRODUCT_INTEL_82371AB_PM, PCI_PRODUCT_INTEL_82440MX_PM, PCI_PRODUCT_RCC_CSB5,
    PCI_PRODUCT_RCC_CSB6, PCI_PRODUCT_RCC_HT_1000, PCI_PRODUCT_RCC_HT_1100, PCI_PRODUCT_RCC_OSB4,
    PCI_PRODUCT_SMSC_VICTORY66_PM, PCI_VENDOR_AMD, PCI_VENDOR_ATI, PCI_VENDOR_INTEL,
    PCI_VENDOR_RCC, PCI_VENDOR_SMSC,
};
use crate::dev::pci::pcireg::{pci_product, pci_revision, pci_vendor};
use crate::dev::pci::pcivar::{PciAttachArgs, PciMatchid};
use crate::dev::pci::piixreg::{
    AMDFCH41_PM_DECODE_EN, AMDFCH41_PM_PORT_INDEX, AMDFCH41_SMBUS_EN, PIIX_SMB_BASE,
    PIIX_SMB_BASE_MASK, PIIX_SMB_HC, PIIX_SMB_HC_CMD_BDATA, PIIX_SMB_HC_CMD_BYTE,
    PIIX_SMB_HC_CMD_WDATA, PIIX_SMB_HC_INTREN, PIIX_SMB_HC_KILL, PIIX_SMB_HC_START, PIIX_SMB_HCMD,
    PIIX_SMB_HD0, PIIX_SMB_HD1, PIIX_SMB_HOSTC, PIIX_SMB_HOSTC_HSTEN, PIIX_SMB_HOSTC_INTMASK,
    PIIX_SMB_HOSTC_IRQ, PIIX_SMB_HOSTC_SMI, PIIX_SMB_HS, PIIX_SMB_HS_BITS, PIIX_SMB_HS_BUSERR,
    PIIX_SMB_HS_BUSY, PIIX_SMB_HS_DEVERR, PIIX_SMB_HS_FAILED, PIIX_SMB_HS_INTR, PIIX_SMB_SIZE,
    PIIX_SMB_TXSLVA, PIIX_SMB_TXSLVA_READ, SB800_PMREG_BASE, SB800_PMREG_SIZE, SB800_PMREG_SMB0EN,
    SB800_PMREG_SMB0SEL, SB800_PMREG_SMB0SELEN, SB800_SMB_HOSTC, SB800_SMB_HOSTC_INTMASK,
    SB800_SMB_SIZE, SB800_SMB0EN_BASE_MASK, SB800_SMB0EN_EN, SB800_SMB0SELEN_EN,
    piix_smb_txslva_addr,
};
use crate::kern::kern_rwlock::{rw_enter, rw_exit, rw_init};
use crate::kern::kern_synch::{tsleep_nsec, wakeup};
use crate::kern::subr_autoconf::config_found;
use crate::kern::subr_prf::{Bitmask, panic, printf};
use crate::machine::bus::{
    BusAddr, BusSpaceHandle, BusSpaceTag, bus_space_map, bus_space_read_1, bus_space_unmap,
    bus_space_write_1,
};
use crate::machine::cpu::delay;
use crate::machine::intr::IPL_BIO;
use crate::machine::pci_machdep::{
    pci_conf_read, pci_intr_establish, pci_intr_map, pci_intr_string,
};
use crate::sys::device::{CfMatch, Cfattach, Cfdriver, DV_DULL, Device, Softc};
use crate::sys::param::PRIBIO;
use crate::sys::rwlock::{RW_INTR, RW_WRITE, Rwlock};
use crate::sys::systm::COLD;
use crate::sys::time::sec_to_nsec;

/// `PIIXPM_DELAY`: microseconds between two looks at the status register.
const PIIXPM_DELAY: u32 = 200;
/// `PIIXPM_TIMEOUT`: seconds an interrupt-driven transfer may take.
const PIIXPM_TIMEOUT: u64 = 1;

/// `struct piixpm_smbus`: one bus of the controller.
pub struct PiixpmSmbus {
    /// `sb_bus`: which bus (0 for a PIIX).
    pub sb_bus: Cell<i32>,
    /// `sb_sc`: the controller.
    pub sb_sc: Cell<*const PiixpmSoftc>,
}

/// The transfer in progress (the anonymous `sc_i2c_xfer` struct of the softc).
pub struct PiixpmXfer {
    /// `op`.
    pub op: Cell<I2cOp>,
    /// `buf`: the caller's data buffer (null when no transfer is waiting).
    pub buf: Cell<*mut u8>,
    /// `len`.
    pub len: Cell<usize>,
    /// `flags`.
    pub flags: Cell<i32>,
    /// `error`: set by the interrupt handler, read by the waiting exec.
    pub error: AtomicI32,
}

/// `struct piixpm_softc`.
#[repr(C)]
pub struct PiixpmSoftc {
    /// `sc_dev`: generic device glue.
    pub sc_dev: Device,
    /// `sc_iot`.
    pub sc_iot: Cell<Option<BusSpaceTag>>,
    /// `sc_ioh`.
    pub sc_ioh: Cell<Option<BusSpaceHandle>>,
    /// `sc_sb800_ioh`: the index/data pair of the SB800 and FCH.
    pub sc_sb800_ioh: Cell<Option<BusSpaceHandle>>,
    /// `sc_ih`.
    pub sc_ih: Cell<Option<NonNull<c_void>>>,
    /// `sc_poll`.
    pub sc_poll: Cell<i32>,
    /// `sc_is_sb800`.
    pub sc_is_sb800: Cell<i32>,
    /// `sc_is_fch`.
    pub sc_is_fch: Cell<i32>,
    /// `sc_busses`.
    pub sc_busses: [PiixpmSmbus; 4],
    /// `sc_i2c_tag`.
    pub sc_i2c_tag: [I2cController; 4],
    /// `sc_i2c_lock`.
    pub sc_i2c_lock: Rwlock,
    /// `sc_i2c_xfer`.
    pub sc_i2c_xfer: PiixpmXfer,
}

// SAFETY: `#[repr(C)]` with the device first; the other members are `Cell`s of integers,
// pointers and `Option`s of handles, arrays of the same, the all-zero `I2cController`s and
// `Rwlock` (`rw_init` names the lock) and atomics: all valid as zero bits.
unsafe impl Softc for PiixpmSoftc {}

/// The registers' tag and handle, as `bus_space_read_1`/`write_1` take them.
#[derive(Clone, Copy)]
struct Io(BusSpaceTag, BusSpaceHandle);

impl Io {
    fn r(self, reg: usize) -> u8 {
        bus_space_read_1(self.0, self.1, reg)
    }

    fn w(self, reg: usize, v: u8) {
        bus_space_write_1(self.0, self.1, reg, v);
    }
}

impl PiixpmSoftc {
    /// The mapped SMBus registers, once the attach has mapped them.
    fn io(&self) -> Option<Io> {
        Some(Io(self.sc_iot.get()?, self.sc_ioh.get()?))
    }

    /// The SB800/FCH index/data pair, once the attach has mapped it.
    fn pm_io(&self) -> Option<Io> {
        Some(Io(self.sc_iot.get()?, self.sc_sb800_ioh.get()?))
    }

    /// The softc behind an interrupt argument.
    ///
    /// # Safety
    ///
    /// `arg` is the `PiixpmSoftc` `piixpm_attach` registered it with.
    unsafe fn from_arg(arg: *mut c_void) -> &'static PiixpmSoftc {
        // SAFETY: the caller's contract; the softc lives as long as the device.
        unsafe { &*arg.cast::<PiixpmSoftc>() }
    }
}

/// The controller and bus behind a bus's cookie.
///
/// # Safety
///
/// `cookie` is a `PiixpmSmbus` `piixpm_attach` put in an `sc_i2c_tag` entry.
unsafe fn piixpm_bus(cookie: *mut c_void) -> (&'static PiixpmSoftc, &'static PiixpmSmbus) {
    // SAFETY: the caller's contract; the bus is a member of the softc, which lives as long as
    // the device, and `sb_sc` was set to that softc by the attach.
    unsafe {
        let smbus = &*cookie.cast::<PiixpmSmbus>();
        (&*smbus.sb_sc.get(), smbus)
    }
}

/// `piixpm_ca`.
pub static PIIXPM_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<PiixpmSoftc>(),
    ca_match: Some(piixpm_match),
    ca_attach: piixpm_attach,
    ca_detach: None,
    ca_activate: None,
};

/// `piixpm_cd`.
pub static PIIXPM_CD: Cfdriver = Cfdriver::new(b"piixpm", DV_DULL, 0);

/// `{ vendor, product }` of a `pci_matchid`.
const fn id(vendor: u32, product: u32) -> PciMatchid {
    PciMatchid {
        pm_vid: vendor as u16,
        pm_pid: product as u16,
    }
}

/// `piixpm_ids[]`.
static PIIXPM_IDS: [PciMatchid; 14] = [
    id(PCI_VENDOR_AMD, PCI_PRODUCT_AMD_HUDSON2_SMB),
    id(PCI_VENDOR_AMD, PCI_PRODUCT_AMD_KERNCZ_SMB),
    id(PCI_VENDOR_ATI, PCI_PRODUCT_ATI_SB200_SMB),
    id(PCI_VENDOR_ATI, PCI_PRODUCT_ATI_SB300_SMB),
    id(PCI_VENDOR_ATI, PCI_PRODUCT_ATI_SB400_SMB),
    id(PCI_VENDOR_ATI, PCI_PRODUCT_ATI_SBX00_SMB),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_82371AB_PM),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_82440MX_PM),
    id(PCI_VENDOR_RCC, PCI_PRODUCT_RCC_CSB5),
    id(PCI_VENDOR_RCC, PCI_PRODUCT_RCC_CSB6),
    id(PCI_VENDOR_RCC, PCI_PRODUCT_RCC_HT_1000),
    id(PCI_VENDOR_RCC, PCI_PRODUCT_RCC_HT_1100),
    id(PCI_VENDOR_RCC, PCI_PRODUCT_RCC_OSB4),
    id(PCI_VENDOR_SMSC, PCI_PRODUCT_SMSC_VICTORY66_PM),
];

/// `piixpm_match`.
pub fn piixpm_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: pci attaches its children with a `pci_attach_args`.
    let pa = unsafe { &*aux.cast::<PciAttachArgs>() };

    pci_matchbyid(pa, &PIIXPM_IDS)
}

/// `piixpm_attach`.
pub fn piixpm_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `self_` was made for `piixpm_ca`, whose softc is a `PiixpmSoftc`; softcs are
    // never freed while the device exists, so the softc may be borrowed for 'static.
    let sc: &'static PiixpmSoftc = unsafe { &*ptr::from_ref(self_.softc::<PiixpmSoftc>()) };
    // SAFETY: pci attaches its children with a `pci_attach_args` it owns for the duration of
    // the attach.
    let pa = unsafe { &*aux.cast::<PciAttachArgs>() };
    let cookie = ptr::from_ref(sc).cast_mut().cast::<c_void>();
    let iot = pa.pa_iot;
    let vendor = pci_vendor(pa.pa_id);
    let product = pci_product(pa.pa_id);
    let revision = pci_revision(pa.pa_class);

    sc.sc_iot.set(Some(iot));
    let mut numbusses = 1;
    let conf: u32;

    if (vendor == PCI_VENDOR_AMD && product == PCI_PRODUCT_AMD_HUDSON2_SMB)
        || (vendor == PCI_VENDOR_AMD && product == PCI_PRODUCT_AMD_KERNCZ_SMB)
        || (vendor == PCI_VENDOR_ATI && product == PCI_PRODUCT_ATI_SBX00_SMB && revision >= 0x40)
    {
        // On the AMD SB800+, the SMBus I/O registers are well hidden. We need to look at the
        // "SMBus0En" Power Management register to find out where they live. We use indirect IO
        // access through the index/data pair at 0xcd6/0xcd7 to access "SMBus0En".
        //
        // SAFETY: 0xcd6/0xcd7 is the SB800's PM index/data pair, which this branch only takes
        // on the AMD and ATI parts that have it (the PCI ID says which); nothing else owns it.
        let Ok(ioh) =
            (unsafe { bus_space_map(iot, SB800_PMREG_BASE as BusAddr, SB800_PMREG_SIZE, 0) })
        else {
            printf(format_args!(": can't map i/o space\n"));
            return;
        };
        let pm = Io(iot, ioh);

        let smb0en: u16;
        let base: BusAddr;

        // AMD Bolton matches PCI_PRODUCT_AMD_HUDSON2_SMB but uses old register layout.
        // Therefor check PCI_REVISION.
        if vendor == PCI_VENDOR_AMD
            && ((product == PCI_PRODUCT_AMD_HUDSON2_SMB && revision >= 0x1f)
                || product == PCI_PRODUCT_AMD_KERNCZ_SMB)
        {
            pm.w(0, AMDFCH41_PM_DECODE_EN);
            let val = u16::from(pm.r(1));
            smb0en = val & u16::from(AMDFCH41_SMBUS_EN);

            pm.w(0, AMDFCH41_PM_DECODE_EN + 1);
            let val = u16::from(pm.r(1)) << 8;
            base = BusAddr::from(val);

            sc.sc_is_fch.set(1);
            numbusses = 2;
        } else {
            // Read "SmBus0En"
            pm.w(0, SB800_PMREG_SMB0EN);
            let mut val = u16::from(pm.r(1));

            pm.w(0, SB800_PMREG_SMB0EN + 1);
            val |= u16::from(pm.r(1)) << 8;
            smb0en = val & SB800_SMB0EN_EN;
            base = BusAddr::from(val & SB800_SMB0EN_BASE_MASK);

            pm.w(0, SB800_PMREG_SMB0SELEN);
            let val = pm.r(1);
            if val & SB800_SMB0SELEN_EN != 0 {
                sc.sc_is_sb800.set(1);
                numbusses = 4;
            }
        }

        if smb0en == 0 {
            printf(format_args!(": SMBus disabled\n"));
            bus_space_unmap(iot, ioh, SB800_PMREG_SIZE);
            return;
        }

        sc.sc_sb800_ioh.set(Some(ioh));

        // Map I/O space
        //
        let mapped = if base == 0 {
            None
        } else {
            // SAFETY: `base` is the SMBus I/O window the chip's own SmBus0En register
            // announced.
            unsafe { bus_space_map(iot, base, SB800_SMB_SIZE, 0) }.ok()
        };
        let Some(smbioh) = mapped else {
            printf(format_args!(": can't map i/o space"));
            bus_space_unmap(iot, ioh, SB800_PMREG_SIZE);
            return;
        };
        sc.sc_ioh.set(Some(smbioh));

        // Read configuration
        conf = if Io(iot, smbioh).r(SB800_SMB_HOSTC) & SB800_SMB_HOSTC_INTMASK != 0 {
            PIIX_SMB_HOSTC_IRQ
        } else {
            PIIX_SMB_HOSTC_SMI
        };
    } else {
        // Read configuration
        conf = pci_conf_read(pa.pa_pc, pa.pa_tag, PIIX_SMB_HOSTC);

        if conf & PIIX_SMB_HOSTC_HSTEN == 0 {
            printf(format_args!(": SMBus disabled\n"));
            return;
        }

        // Map I/O space
        let base = BusAddr::try_from(
            pci_conf_read(pa.pa_pc, pa.pa_tag, PIIX_SMB_BASE) & PIIX_SMB_BASE_MASK,
        )
        .unwrap_or(0);
        let mapped = if base == 0 {
            None
        } else {
            // SAFETY: `base` is the SMBus I/O window the function's own base register
            // announced.
            unsafe { bus_space_map(iot, base, PIIX_SMB_SIZE, 0) }.ok()
        };
        let Some(ioh) = mapped else {
            printf(format_args!(": can't map i/o space\n"));
            return;
        };
        sc.sc_ioh.set(Some(ioh));
    }

    sc.sc_poll.set(1);
    if conf & PIIX_SMB_HOSTC_INTMASK == PIIX_SMB_HOSTC_SMI {
        // No PCI IRQ
        printf(format_args!(": SMI"));
    } else {
        if conf & PIIX_SMB_HOSTC_INTMASK == PIIX_SMB_HOSTC_IRQ {
            // Install interrupt handler
            if let Some(ih) = pci_intr_map(pa) {
                let intrstr = pci_intr_string(pa.pa_pc, ih);
                sc.sc_ih.set(pci_intr_establish(
                    pa.pa_pc,
                    ih,
                    IPL_BIO,
                    piixpm_intr,
                    cookie,
                    sc.sc_dev.xname(),
                ));
                if sc.sc_ih.get().is_some() {
                    printf(format_args!(": {intrstr}"));
                    sc.sc_poll.set(0);
                }
            }
        }
        if sc.sc_poll.get() != 0 {
            printf(format_args!(": polling"));
        }
    }

    printf(format_args!("\n"));

    // Attach I2C bus
    rw_init(&sc.sc_i2c_lock, "iiclk");
    for i in 0..numbusses {
        let bus = &sc.sc_busses[i];
        let tag = &sc.sc_i2c_tag[i];
        bus.sb_bus.set(i as i32);
        bus.sb_sc.set(ptr::from_ref(sc));
        tag.ic_cookie.set(ptr::from_ref(bus).cast_mut().cast());
        tag.ic_acquire_bus.set(Some(piixpm_i2c_acquire_bus));
        tag.ic_release_bus.set(Some(piixpm_i2c_release_bus));
        tag.ic_exec.set(Some(piixpm_i2c_exec));

        let iba = I2cbusAttachArgs::new("iic", tag);
        config_found(
            self_,
            ptr::from_ref(&iba).cast_mut().cast(),
            Some(iicbus_print),
        );
    }
}

/// `piixpm_i2c_acquire_bus`.
fn piixpm_i2c_acquire_bus(cookie: *mut c_void, flags: i32) -> i32 {
    // SAFETY: the cookie `piixpm_attach` put in the bus's `i2c_controller`.
    let (sc, smbus) = unsafe { piixpm_bus(cookie) };

    if !COLD.load(Ordering::Relaxed)
        && sc.sc_poll.get() == 0
        && flags & I2C_F_POLL == 0
        && let Err(rc) = rw_enter(&sc.sc_i2c_lock, RW_WRITE | RW_INTR)
    {
        return rc as i32;
    }

    if let Some(pm) = sc.pm_io() {
        if sc.sc_is_fch.get() != 0 {
            pm.w(0, AMDFCH41_PM_PORT_INDEX);
            pm.w(1, (smbus.sb_bus.get() << 3) as u8);
        } else if sc.sc_is_sb800.get() != 0 {
            pm.w(0, SB800_PMREG_SMB0SEL);
            pm.w(1, (smbus.sb_bus.get() << 1) as u8);
        }
    }

    0
}

/// `piixpm_i2c_release_bus`.
fn piixpm_i2c_release_bus(cookie: *mut c_void, flags: i32) {
    // SAFETY: the cookie `piixpm_attach` put in the bus's `i2c_controller`.
    let (sc, _smbus) = unsafe { piixpm_bus(cookie) };

    if let Some(pm) = sc.pm_io() {
        if sc.sc_is_fch.get() != 0 {
            pm.w(0, AMDFCH41_PM_PORT_INDEX);
            pm.w(1, 0);
        } else if sc.sc_is_sb800.get() != 0 {
            pm.w(0, SB800_PMREG_SMB0SEL);
            pm.w(1, 0);
        }
    }

    if COLD.load(Ordering::Relaxed) || sc.sc_poll.get() != 0 || flags & I2C_F_POLL != 0 {
        return;
    }
    rw_exit(&sc.sc_i2c_lock);
}

/// The `HC` value that starts a transaction of `len` data bytes: BYTE (none), BYTE DATA (one) or
/// WORD DATA (two), with interrupts unless polling.
fn piixpm_ctl(len: usize, flags: i32) -> u8 {
    // Set SMBus command
    let mut ctl = match len {
        0 => PIIX_SMB_HC_CMD_BYTE,
        1 => PIIX_SMB_HC_CMD_BDATA,
        2 => PIIX_SMB_HC_CMD_WDATA,
        _ => panic(format_args!("piixpm_i2c_exec: unexpected len {len}")),
    };

    if flags & I2C_F_POLL == 0 {
        ctl |= PIIX_SMB_HC_INTREN;
    }

    // Start transaction
    ctl | PIIX_SMB_HC_START
}

/// `piixpm_i2c_exec`.
fn piixpm_i2c_exec(
    cookie: *mut c_void,
    op: I2cOp,
    addr: I2cAddr,
    cmdbuf: &[u8],
    buf: &mut [u8],
    flags: i32,
) -> i32 {
    // SAFETY: the cookie `piixpm_attach` put in the bus's `i2c_controller`.
    let (sc, _smbus) = unsafe { piixpm_bus(cookie) };

    let error = piixpm_exec(sc, op, addr, cmdbuf, buf, flags);
    // The transfer is over: a late interrupt has no buffer to write to.
    sc.sc_i2c_xfer.buf.set(ptr::null_mut());
    error
}

/// The body of `piixpm_i2c_exec`.
fn piixpm_exec(
    sc: &'static PiixpmSoftc,
    op: I2cOp,
    addr: I2cAddr,
    cmdbuf: &[u8],
    buf: &mut [u8],
    mut flags: i32,
) -> i32 {
    let len = buf.len();
    let Some(io) = sc.io() else {
        return 1;
    };

    // Wait for bus to be idle
    let mut st = 0;
    for _ in 0..100 {
        st = io.r(PIIX_SMB_HS);
        if st & PIIX_SMB_HS_BUSY == 0 {
            break;
        }
        delay(PIIXPM_DELAY);
    }
    if st & PIIX_SMB_HS_BUSY != 0 {
        return 1;
    }

    if COLD.load(Ordering::Relaxed) || sc.sc_poll.get() != 0 {
        flags |= I2C_F_POLL;
    }

    if !op.stop_p() || cmdbuf.len() > 1 || len > 2 {
        return 1;
    }

    // Setup transfer
    sc.sc_i2c_xfer.op.set(op);
    sc.sc_i2c_xfer.buf.set(buf.as_mut_ptr());
    sc.sc_i2c_xfer.len.set(len);
    sc.sc_i2c_xfer.flags.set(flags);
    sc.sc_i2c_xfer.error.store(0, Ordering::Relaxed);

    // Set slave address and transfer direction
    io.w(
        PIIX_SMB_TXSLVA,
        piix_smb_txslva_addr(addr) | if op.read_p() { PIIX_SMB_TXSLVA_READ } else { 0 },
    );

    if let Some(&c) = cmdbuf.first() {
        // Set command byte
        io.w(PIIX_SMB_HCMD, c);
    }

    if op.write_p() {
        // Write data
        if let Some(&b) = buf.first() {
            io.w(PIIX_SMB_HD0, b);
        }
        if let Some(&b) = buf.get(1) {
            io.w(PIIX_SMB_HD1, b);
        }
    }

    io.w(PIIX_SMB_HC, piixpm_ctl(len, flags));

    let timed_out = if flags & I2C_F_POLL != 0 {
        // Poll for completion
        delay(PIIXPM_DELAY);
        for _ in 0..1000 {
            st = io.r(PIIX_SMB_HS);
            if st & PIIX_SMB_HS_BUSY == 0 {
                break;
            }
            delay(PIIXPM_DELAY);
        }
        if st & PIIX_SMB_HS_BUSY != 0 {
            true
        } else {
            piixpm_intr(ptr::from_ref(sc).cast_mut().cast());
            false
        }
    } else {
        // Wait for interrupt
        tsleep_nsec(
            ptr::from_ref(sc),
            PRIBIO,
            "piixpm",
            sec_to_nsec(PIIXPM_TIMEOUT),
        )
        .is_err()
    };

    if !timed_out {
        return i32::from(sc.sc_i2c_xfer.error.load(Ordering::Relaxed) != 0);
    }

    // timeout:
    // Transfer timeout. Kill the transaction and clear status bits.
    printf(format_args!(
        "{}: exec: op {}, addr 0x{:02x}, cmdlen {}, len {}, flags 0x{:02x}: timeout, status 0x{}\n",
        sc.sc_dev.xname(),
        op.0,
        addr,
        cmdbuf.len(),
        len,
        flags,
        Bitmask(u64::from(st), PIIX_SMB_HS_BITS)
    ));
    io.w(PIIX_SMB_HC, PIIX_SMB_HC_KILL);
    delay(PIIXPM_DELAY);
    st = io.r(PIIX_SMB_HS);
    if st & PIIX_SMB_HS_FAILED == 0 {
        printf(format_args!(
            "{}: abort failed, status 0x{}\n",
            sc.sc_dev.xname(),
            Bitmask(u64::from(st), PIIX_SMB_HS_BITS)
        ));
    }
    io.w(PIIX_SMB_HS, st);
    1
}

/// `piixpm_intr`.
fn piixpm_intr(arg: *mut c_void) -> i32 {
    // SAFETY: the argument `piixpm_attach` established the handler with (or `piixpm_exec`
    // passes in polling mode).
    let sc = unsafe { PiixpmSoftc::from_arg(arg) };
    let Some(io) = sc.io() else {
        return 0;
    };

    // Read status
    let st = io.r(PIIX_SMB_HS);
    if st & PIIX_SMB_HS_BUSY != 0
        || st & (PIIX_SMB_HS_INTR | PIIX_SMB_HS_DEVERR | PIIX_SMB_HS_BUSERR | PIIX_SMB_HS_FAILED)
            == 0
    {
        // Interrupt was not for us
        return 0;
    }

    // Clear status bits
    io.w(PIIX_SMB_HS, st);

    // Check for errors
    if st & (PIIX_SMB_HS_DEVERR | PIIX_SMB_HS_BUSERR | PIIX_SMB_HS_FAILED) != 0 {
        sc.sc_i2c_xfer.error.store(1, Ordering::Relaxed);
    } else if st & PIIX_SMB_HS_INTR != 0 && sc.sc_i2c_xfer.op.get().read_p() {
        // Read data
        let b = sc.sc_i2c_xfer.buf.get();
        let len = sc.sc_i2c_xfer.len.get();
        if !b.is_null() {
            if len > 0 {
                // SAFETY: `b` is the start of the `len` bytes `piixpm_exec` is waiting on (it
                // clears `buf` before the buffer goes away).
                unsafe { *b = io.r(PIIX_SMB_HD0) };
            }
            if len > 1 {
                // SAFETY: as above, `len` is at least 2.
                unsafe { *b.add(1) = io.r(PIIX_SMB_HD1) };
            }
        }
    }

    // done:
    if sc.sc_i2c_xfer.flags.get() & I2C_F_POLL == 0 {
        wakeup(ptr::from_ref(sc));
    }
    1
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_control_byte_follows_the_data_length() {
        assert_eq!(
            piixpm_ctl(0, 0),
            PIIX_SMB_HC_CMD_BYTE | PIIX_SMB_HC_INTREN | PIIX_SMB_HC_START
        );
        assert_eq!(
            piixpm_ctl(1, I2C_F_POLL),
            PIIX_SMB_HC_CMD_BDATA | PIIX_SMB_HC_START
        );
        assert_eq!(
            piixpm_ctl(2, I2C_F_POLL),
            PIIX_SMB_HC_CMD_WDATA | PIIX_SMB_HC_START
        );
        assert_eq!(piixpm_ctl(2, 0), 0xc | 0x1 | 0x40);
    }

    #[test]
    fn the_table_holds_the_i440fx_power_management_function() {
        // QEMU's PIIX4 PM: 8086:7113.
        let hits = PIIXPM_IDS
            .iter()
            .filter(|m| m.pm_vid == 0x8086 && m.pm_pid == 0x7113)
            .count();
        assert_eq!(hits, 1);
        assert_eq!(PIIXPM_IDS.len(), 14);
    }
}
/* </TESTS> */
