/*	$OpenBSD: ichiic.c,v 1.58 2025/08/21 03:06:20 jsg Exp $	*/
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
//! `ichiic*`: Intel ICH SMBus controller driver (`ichiic* at pci?`, `iic* at ichiic?`), see
//! `ichiic(4)`.
//!
//! Upstream: sys/dev/pci/ichiic.c @ 3ce1f3f79392
//!
//! The SMBus host controller of the ICH family's LPC/SMBus function (QEMU's `q35` has the
//! ICH9 one at 00:1f.3). It does the SMBus transactions of a command byte and up to two data
//! bytes itself (`ic_exec` only; no byte-level hooks), by interrupt when the PCI interrupt is
//! routed and the BIOS did not leave the controller on SMI, else by polling; the `cold`
//! autoconfiguration scan always polls. The attach hangs an `iic*` bus off the controller,
//! whose scan (`i2c_scan.rs`) identifies what answers. UEFI firmware (EDK2/OVMF on q35)
//! leaves the host controller disabled, where a BIOS enables it: there the attach prints
//! `SMBus disabled` and stops, as OpenBSD 8.0 does on the same machine.
//!
//! ## Deviations
//! - The attach arguments' `pa_iot` is not used: `pci_mapreg_map` returns the tag and handle.
//! - `sc_i2c_xfer.error` is an atomic (the C's `volatile int`, written by the interrupt
//!   handler); `buf` is cleared when an exec call returns, and the interrupt handler reads
//!   no data when it is null: a late interrupt after a timeout cannot write into the caller's
//!   buffer (the C could).
//! - `ichiic_i2c_acquire_bus` returns the `Errno` of `rw_enter` as its positive `int`.
//! - `ICHIIC_DEBUG` (the `DPRINTF`s) is not configured.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI32, Ordering};

use crate::dev::i2c::i2c::iicbus_print;
use crate::dev::i2c::i2c_io::{I2cAddr, I2cOp};
use crate::dev::i2c::i2cvar::{I2C_F_POLL, I2cController, I2cbusAttachArgs};
use crate::dev::pci::ichreg::{
    ICH_SMB_BASE, ICH_SMB_HC, ICH_SMB_HC_CMD_BDATA, ICH_SMB_HC_CMD_BYTE, ICH_SMB_HC_CMD_WDATA,
    ICH_SMB_HC_INTREN, ICH_SMB_HC_KILL, ICH_SMB_HC_START, ICH_SMB_HCMD, ICH_SMB_HD0, ICH_SMB_HD1,
    ICH_SMB_HOSTC, ICH_SMB_HOSTC_HSTEN, ICH_SMB_HOSTC_SMIEN, ICH_SMB_HS, ICH_SMB_HS_BDONE,
    ICH_SMB_HS_BITS, ICH_SMB_HS_BUSERR, ICH_SMB_HS_BUSY, ICH_SMB_HS_DEVERR, ICH_SMB_HS_FAILED,
    ICH_SMB_HS_INTR, ICH_SMB_TXSLVA, ICH_SMB_TXSLVA_READ, ich_smb_txslva_addr,
};
use crate::dev::pci::pci::pci_matchbyid;
use crate::dev::pci::pci_map::pci_mapreg_map;
use crate::dev::pci::pcidevs::{
    PCI_PRODUCT_INTEL_6SERIES_SMB, PCI_PRODUCT_INTEL_7SERIES_SMB, PCI_PRODUCT_INTEL_8SERIES_LP_SMB,
    PCI_PRODUCT_INTEL_8SERIES_SMB, PCI_PRODUCT_INTEL_9SERIES_LP_SMB, PCI_PRODUCT_INTEL_9SERIES_SMB,
    PCI_PRODUCT_INTEL_100SERIES_LP_SMB, PCI_PRODUCT_INTEL_100SERIES_SMB,
    PCI_PRODUCT_INTEL_200SERIES_SMB, PCI_PRODUCT_INTEL_300SERIES_SMB,
    PCI_PRODUCT_INTEL_300SERIES_U_SMB, PCI_PRODUCT_INTEL_400SERIES_LP_SMB,
    PCI_PRODUCT_INTEL_400SERIES_SMB, PCI_PRODUCT_INTEL_400SERIES_V_SMB,
    PCI_PRODUCT_INTEL_495SERIES_LP_SMB, PCI_PRODUCT_INTEL_500SERIES_LP_SMB,
    PCI_PRODUCT_INTEL_500SERIES_SMB, PCI_PRODUCT_INTEL_600SERIES_LP_SMB,
    PCI_PRODUCT_INTEL_600SERIES_SMB, PCI_PRODUCT_INTEL_700SERIES_SMB, PCI_PRODUCT_INTEL_3400_SMB,
    PCI_PRODUCT_INTEL_6300ESB_SMB, PCI_PRODUCT_INTEL_6321ESB_SMB, PCI_PRODUCT_INTEL_82801AA_SMB,
    PCI_PRODUCT_INTEL_82801AB_SMB, PCI_PRODUCT_INTEL_82801BA_SMB, PCI_PRODUCT_INTEL_82801CA_SMB,
    PCI_PRODUCT_INTEL_82801DB_SMB, PCI_PRODUCT_INTEL_82801E_SMB, PCI_PRODUCT_INTEL_82801EB_SMB,
    PCI_PRODUCT_INTEL_82801FB_SMB, PCI_PRODUCT_INTEL_82801GB_SMB, PCI_PRODUCT_INTEL_82801H_SMB,
    PCI_PRODUCT_INTEL_82801I_SMB, PCI_PRODUCT_INTEL_82801JD_SMB, PCI_PRODUCT_INTEL_82801JI_SMB,
    PCI_PRODUCT_INTEL_ADL_N_SMB, PCI_PRODUCT_INTEL_APOLLOLAKE_SMB, PCI_PRODUCT_INTEL_ARL_U_SMB,
    PCI_PRODUCT_INTEL_ATOMC2000_PCU_SMB, PCI_PRODUCT_INTEL_BAYTRAIL_SMB,
    PCI_PRODUCT_INTEL_BRASWELL_SMB, PCI_PRODUCT_INTEL_C600_SMB, PCI_PRODUCT_INTEL_C600_SMB_IDF_1,
    PCI_PRODUCT_INTEL_C600_SMB_IDF_2, PCI_PRODUCT_INTEL_C600_SMB_IDF_3,
    PCI_PRODUCT_INTEL_C610_MS_SMB_1, PCI_PRODUCT_INTEL_C610_MS_SMB_2,
    PCI_PRODUCT_INTEL_C610_MS_SMB_3, PCI_PRODUCT_INTEL_C610_SMB, PCI_PRODUCT_INTEL_C620_SMB,
    PCI_PRODUCT_INTEL_C740_SMB, PCI_PRODUCT_INTEL_C3000_SMB_2, PCI_PRODUCT_INTEL_DH8900_SMB,
    PCI_PRODUCT_INTEL_EHL_SMB, PCI_PRODUCT_INTEL_EP80579_SMBUS, PCI_PRODUCT_INTEL_GLK_SMB,
    PCI_PRODUCT_INTEL_JSL_SMB, PCI_PRODUCT_INTEL_LNL_SMB, PCI_PRODUCT_INTEL_MTL_SMB,
    PCI_VENDOR_INTEL,
};
use crate::dev::pci::pcireg::PCI_MAPREG_TYPE_IO;
use crate::dev::pci::pcivar::{PciAttachArgs, PciMatchid};
use crate::kern::kern_rwlock::{rw_enter, rw_exit, rw_init};
use crate::kern::kern_synch::{tsleep_nsec, wakeup};
use crate::kern::subr_autoconf::config_found;
use crate::kern::subr_prf::{Bitmask, panic, printf};
use crate::machine::bus::{BusSpaceHandle, BusSpaceTag, bus_space_read_1, bus_space_write_1};
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

/// `ICHIIC_DELAY`: microseconds between two looks at the status register.
const ICHIIC_DELAY: u32 = 100;
/// `ICHIIC_TIMEOUT`: seconds an interrupt-driven transfer may take.
const ICHIIC_TIMEOUT: u64 = 1;

/// The transfer in progress (the anonymous `sc_i2c_xfer` struct of the softc).
pub struct IchiicXfer {
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

/// `struct ichiic_softc`.
#[repr(C)]
pub struct IchiicSoftc {
    /// `sc_dev`: generic device glue.
    pub sc_dev: Device,
    /// `sc_iot`.
    pub sc_iot: Cell<Option<BusSpaceTag>>,
    /// `sc_ioh`.
    pub sc_ioh: Cell<Option<BusSpaceHandle>>,
    /// `sc_ih`.
    pub sc_ih: Cell<Option<NonNull<c_void>>>,
    /// `sc_poll`.
    pub sc_poll: Cell<i32>,
    /// `sc_i2c_tag`.
    pub sc_i2c_tag: I2cController,
    /// `sc_i2c_lock`.
    pub sc_i2c_lock: Rwlock,
    /// `sc_i2c_xfer`.
    pub sc_i2c_xfer: IchiicXfer,
}

// SAFETY: `#[repr(C)]` with the device first; the other members are `Cell`s of integers,
// pointers, `Option`s of handles, the all-zero `I2cController` and `Rwlock` (`rw_init` names the
// lock) and atomics: all valid as zero bits.
unsafe impl Softc for IchiicSoftc {}

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

impl IchiicSoftc {
    /// The mapped registers, once the attach has mapped them.
    fn io(&self) -> Option<Io> {
        Some(Io(self.sc_iot.get()?, self.sc_ioh.get()?))
    }

    /// The softc behind a controller cookie or an interrupt argument.
    ///
    /// # Safety
    ///
    /// `arg` is the `IchiicSoftc` `ichiic_attach` registered it with.
    unsafe fn from_arg(arg: *mut c_void) -> &'static IchiicSoftc {
        // SAFETY: the caller's contract; the softc lives as long as the device.
        unsafe { &*arg.cast::<IchiicSoftc>() }
    }
}

/// `ichiic_ca`.
pub static ICHIIC_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<IchiicSoftc>(),
    ca_match: Some(ichiic_match),
    ca_attach: ichiic_attach,
    ca_detach: None,
    ca_activate: None,
};

/// `ichiic_cd`.
pub static ICHIIC_CD: Cfdriver = Cfdriver::new(b"ichiic", DV_DULL, 0);

/// `{ vendor, product }` of a `pci_matchid`.
const fn id(vendor: u32, product: u32) -> PciMatchid {
    PciMatchid {
        pm_vid: vendor as u16,
        pm_pid: product as u16,
    }
}

/// `ichiic_ids[]`.
static ICHIIC_IDS: [PciMatchid; 60] = [
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_3400_SMB),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_6SERIES_SMB),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_6300ESB_SMB),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_6321ESB_SMB),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_7SERIES_SMB),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_8SERIES_SMB),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_8SERIES_LP_SMB),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_9SERIES_SMB),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_9SERIES_LP_SMB),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_82801AA_SMB),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_82801AB_SMB),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_82801BA_SMB),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_82801CA_SMB),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_82801DB_SMB),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_82801E_SMB),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_82801EB_SMB),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_82801FB_SMB),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_82801GB_SMB),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_82801H_SMB),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_82801I_SMB),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_82801JD_SMB),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_82801JI_SMB),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_APOLLOLAKE_SMB),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_ATOMC2000_PCU_SMB),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_C3000_SMB_2),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_BAYTRAIL_SMB),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_BRASWELL_SMB),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_C600_SMB),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_C600_SMB_IDF_1),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_C600_SMB_IDF_2),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_C600_SMB_IDF_3),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_C610_SMB),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_C610_MS_SMB_1),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_C610_MS_SMB_2),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_C610_MS_SMB_3),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_C620_SMB),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_C740_SMB),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_DH8900_SMB),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_EP80579_SMBUS),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_GLK_SMB),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_100SERIES_SMB),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_100SERIES_LP_SMB),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_200SERIES_SMB),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_300SERIES_SMB),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_300SERIES_U_SMB),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_400SERIES_SMB),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_400SERIES_LP_SMB),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_400SERIES_V_SMB),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_495SERIES_LP_SMB),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_500SERIES_SMB),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_500SERIES_LP_SMB),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_600SERIES_SMB),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_600SERIES_LP_SMB),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_700SERIES_SMB),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_JSL_SMB),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_EHL_SMB),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_ADL_N_SMB),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_MTL_SMB),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_LNL_SMB),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_ARL_U_SMB),
];

/// `ichiic_match`.
pub fn ichiic_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: pci attaches its children with a `pci_attach_args`.
    let pa = unsafe { &*aux.cast::<PciAttachArgs>() };

    pci_matchbyid(pa, &ICHIIC_IDS)
}

/// `ichiic_attach`.
pub fn ichiic_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `self_` was made for `ichiic_ca`, whose softc is an `IchiicSoftc`; softcs are
    // never freed while the device exists, so the softc may be borrowed for 'static.
    let sc: &'static IchiicSoftc = unsafe { &*ptr::from_ref(self_.softc::<IchiicSoftc>()) };
    // SAFETY: pci attaches its children with a `pci_attach_args` it owns for the duration of
    // the attach.
    let pa = unsafe { &*aux.cast::<PciAttachArgs>() };
    let cookie = ptr::from_ref(sc).cast_mut().cast::<c_void>();

    // Read configuration
    let conf = pci_conf_read(pa.pa_pc, pa.pa_tag, ICH_SMB_HOSTC);

    if conf & ICH_SMB_HOSTC_HSTEN == 0 {
        printf(format_args!(": SMBus disabled\n"));
        return;
    }

    // Map I/O space
    let Ok((iot, ioh, _base, _iosize)) = pci_mapreg_map(pa, ICH_SMB_BASE, PCI_MAPREG_TYPE_IO, 0, 0)
    else {
        printf(format_args!(": can't map i/o space\n"));
        return;
    };
    sc.sc_iot.set(Some(iot));
    sc.sc_ioh.set(Some(ioh));

    sc.sc_poll.set(1);
    if conf & ICH_SMB_HOSTC_SMIEN != 0 {
        // No PCI IRQ
        printf(format_args!(": SMI"));
    } else {
        // Install interrupt handler
        if let Some(ih) = pci_intr_map(pa) {
            let intrstr = pci_intr_string(pa.pa_pc, ih);
            sc.sc_ih.set(pci_intr_establish(
                pa.pa_pc,
                ih,
                IPL_BIO,
                ichiic_intr,
                cookie,
                sc.sc_dev.xname(),
            ));
            if sc.sc_ih.get().is_some() {
                printf(format_args!(": {intrstr}"));
                sc.sc_poll.set(0);
            }
        }
        if sc.sc_poll.get() != 0 {
            printf(format_args!(": polling"));
        }
    }

    printf(format_args!("\n"));

    // Attach I2C bus
    rw_init(&sc.sc_i2c_lock, "iiclk");
    sc.sc_i2c_tag.ic_cookie.set(cookie);
    sc.sc_i2c_tag
        .ic_acquire_bus
        .set(Some(ichiic_i2c_acquire_bus));
    sc.sc_i2c_tag
        .ic_release_bus
        .set(Some(ichiic_i2c_release_bus));
    sc.sc_i2c_tag.ic_exec.set(Some(ichiic_i2c_exec));

    let iba = I2cbusAttachArgs::new("iic", &sc.sc_i2c_tag);
    config_found(
        self_,
        ptr::from_ref(&iba).cast_mut().cast(),
        Some(iicbus_print),
    );
}

/// `ichiic_i2c_acquire_bus`.
fn ichiic_i2c_acquire_bus(cookie: *mut c_void, flags: i32) -> i32 {
    // SAFETY: the cookie `ichiic_attach` put in `sc_i2c_tag`.
    let sc = unsafe { IchiicSoftc::from_arg(cookie) };

    if COLD.load(Ordering::Relaxed) || sc.sc_poll.get() != 0 || flags & I2C_F_POLL != 0 {
        return 0;
    }

    match rw_enter(&sc.sc_i2c_lock, RW_WRITE | RW_INTR) {
        Ok(()) => 0,
        Err(e) => e as i32,
    }
}

/// `ichiic_i2c_release_bus`.
fn ichiic_i2c_release_bus(cookie: *mut c_void, flags: i32) {
    // SAFETY: the cookie `ichiic_attach` put in `sc_i2c_tag`.
    let sc = unsafe { IchiicSoftc::from_arg(cookie) };

    if COLD.load(Ordering::Relaxed) || sc.sc_poll.get() != 0 || flags & I2C_F_POLL != 0 {
        return;
    }

    rw_exit(&sc.sc_i2c_lock);
}

/// The `HC` value that starts a transaction of `len` data bytes: BYTE (none), BYTE DATA (one) or
/// WORD DATA (two), with interrupts unless polling.
fn ichiic_ctl(len: usize, flags: i32) -> u8 {
    // Set SMBus command
    let mut ctl = match len {
        0 => ICH_SMB_HC_CMD_BYTE,
        1 => ICH_SMB_HC_CMD_BDATA,
        2 => ICH_SMB_HC_CMD_WDATA,
        _ => panic(format_args!("ichiic_i2c_exec: unexpected len {len}")),
    };

    if flags & I2C_F_POLL == 0 {
        ctl |= ICH_SMB_HC_INTREN;
    }

    // Start transaction
    ctl | ICH_SMB_HC_START
}

/// `ichiic_i2c_exec`.
fn ichiic_i2c_exec(
    cookie: *mut c_void,
    op: I2cOp,
    addr: I2cAddr,
    cmdbuf: &[u8],
    buf: &mut [u8],
    flags: i32,
) -> i32 {
    // SAFETY: the cookie `ichiic_attach` put in `sc_i2c_tag`.
    let sc = unsafe { IchiicSoftc::from_arg(cookie) };

    let error = ichiic_exec(sc, op, addr, cmdbuf, buf, flags);
    // The transfer is over: a late interrupt has no buffer to write to.
    sc.sc_i2c_xfer.buf.set(ptr::null_mut());
    error
}

/// The body of `ichiic_i2c_exec`.
fn ichiic_exec(
    sc: &'static IchiicSoftc,
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
        st = io.r(ICH_SMB_HS);
        if st & ICH_SMB_HS_BUSY == 0 {
            break;
        }
        delay(ICHIIC_DELAY);
    }
    if st & ICH_SMB_HS_BUSY != 0 {
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
        ICH_SMB_TXSLVA,
        ich_smb_txslva_addr(addr) | if op.read_p() { ICH_SMB_TXSLVA_READ } else { 0 },
    );

    if let Some(&c) = cmdbuf.first() {
        // Set command byte
        io.w(ICH_SMB_HCMD, c);
    }

    if op.write_p() {
        // Write data
        if let Some(&b) = buf.first() {
            io.w(ICH_SMB_HD0, b);
        }
        if let Some(&b) = buf.get(1) {
            io.w(ICH_SMB_HD1, b);
        }
    }

    io.w(ICH_SMB_HC, ichiic_ctl(len, flags));

    let timed_out = if flags & I2C_F_POLL != 0 {
        // Poll for completion
        delay(ICHIIC_DELAY);
        for _ in 0..1000 {
            st = io.r(ICH_SMB_HS);
            if st & ICH_SMB_HS_BUSY == 0 {
                break;
            }
            delay(ICHIIC_DELAY);
        }
        if st & ICH_SMB_HS_BUSY != 0 {
            true
        } else {
            ichiic_intr(ptr::from_ref(sc).cast_mut().cast());
            false
        }
    } else {
        // Wait for interrupt
        tsleep_nsec(
            ptr::from_ref(sc),
            PRIBIO,
            "ichiic",
            sec_to_nsec(ICHIIC_TIMEOUT),
        )
        .is_err()
    };

    if !timed_out {
        return i32::from(sc.sc_i2c_xfer.error.load(Ordering::Relaxed) != 0);
    }

    // timeout:
    // Transfer timeout. Kill the transaction and clear status bits.
    io.w(ICH_SMB_HC, ICH_SMB_HC_KILL);
    delay(ICHIIC_DELAY);
    st = io.r(ICH_SMB_HS);
    if st & ICH_SMB_HS_FAILED == 0 {
        printf(format_args!(
            "{}: abort failed, status 0x{}\n",
            sc.sc_dev.xname(),
            Bitmask(u64::from(st), ICH_SMB_HS_BITS)
        ));
    }
    io.w(ICH_SMB_HS, st);
    1
}

/// `ichiic_intr`.
fn ichiic_intr(arg: *mut c_void) -> i32 {
    // SAFETY: the argument `ichiic_attach` established the handler with (or `ichiic_exec`
    // passes in polling mode).
    let sc = unsafe { IchiicSoftc::from_arg(arg) };
    let Some(io) = sc.io() else {
        return 0;
    };

    // Read status
    let st = io.r(ICH_SMB_HS);

    // Clear status bits
    io.w(ICH_SMB_HS, st);

    // XXX Ignore SMBALERT# for now
    if st & ICH_SMB_HS_BUSY != 0
        || st
            & (ICH_SMB_HS_INTR
                | ICH_SMB_HS_DEVERR
                | ICH_SMB_HS_BUSERR
                | ICH_SMB_HS_FAILED
                | ICH_SMB_HS_BDONE)
            == 0
    {
        // Interrupt was not for us
        return 0;
    }

    // Check for errors
    if st & (ICH_SMB_HS_DEVERR | ICH_SMB_HS_BUSERR | ICH_SMB_HS_FAILED) != 0 {
        sc.sc_i2c_xfer.error.store(1, Ordering::Relaxed);
    } else if st & ICH_SMB_HS_INTR != 0 && sc.sc_i2c_xfer.op.get().read_p() {
        // Read data
        let b = sc.sc_i2c_xfer.buf.get();
        let len = sc.sc_i2c_xfer.len.get();
        if !b.is_null() {
            if len > 0 {
                // SAFETY: `b` is the start of the `len` bytes `ichiic_exec` is waiting on (it
                // clears `buf` before the buffer goes away).
                unsafe { *b = io.r(ICH_SMB_HD0) };
            }
            if len > 1 {
                // SAFETY: as above, `len` is at least 2.
                unsafe { *b.add(1) = io.r(ICH_SMB_HD1) };
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
    use crate::dev::pci::pcidevs::PCI_PRODUCT_INTEL_82801I_SMB;
    use crate::dev::pci::pcireg::PciProductId;

    #[test]
    fn the_control_byte_follows_the_data_length() {
        assert_eq!(
            ichiic_ctl(0, 0),
            ICH_SMB_HC_CMD_BYTE | ICH_SMB_HC_INTREN | ICH_SMB_HC_START
        );
        assert_eq!(
            ichiic_ctl(1, I2C_F_POLL),
            ICH_SMB_HC_CMD_BDATA | ICH_SMB_HC_START
        );
        assert_eq!(
            ichiic_ctl(2, I2C_F_POLL),
            ICH_SMB_HC_CMD_WDATA | ICH_SMB_HC_START
        );
        assert_eq!(ichiic_ctl(2, 0), 0xc | 0x1 | 0x40);
    }

    #[test]
    fn the_table_holds_q35s_ich9_smbus_once() {
        let ich9 = 0x2930 as PciProductId;
        assert_eq!(u32::from(ich9), PCI_PRODUCT_INTEL_82801I_SMB);
        let hits = ICHIIC_IDS
            .iter()
            .filter(|m| m.pm_vid == 0x8086 && m.pm_pid == ich9)
            .count();
        assert_eq!(hits, 1);
        assert_eq!(ICHIIC_IDS.len(), 60);
        assert!(ICHIIC_IDS.iter().all(|m| m.pm_vid == 0x8086));
    }
}
/* </TESTS> */
