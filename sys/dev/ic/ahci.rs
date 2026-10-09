/* <LICENSES> */
/*
 * Copyright (c) 2006 David Gwynne <dlg@openbsd.org>
 * Copyright (c) 2010 Conformal Systems LLC <info@conformal.com>
 * Copyright (c) 2010 Jonathan Matthew <jonathan@d14n.org>
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
//! `ahci(4)`: the Advanced Host Controller Interface for Serial ATA, presented to the system
//! through atascsi as a SCSI adapter whose targets are the controller's ports (`scsibus* at
//! scsi?`, `sd* at scsibus?`, `cd* at scsibus?`).
//!
//! Upstream: sys/dev/ic/ahci.c @ 3ce1f3f79392
//!
//! The bus front-end (`ahci_pci`) maps the registers, establishes the interrupt and calls
//! `ahci_attach`, which resets the controller, allocates every implemented port (received FIS
//! area, command list, command tables and one ccb per command slot), resets the ports and
//! waits for their devices, detects port multipliers, and attaches atascsi. Commands are
//! issued by `ahci_start`, which keeps up to two standard commands, or any number of NCQ
//! commands of one port multiplier port, on the chip; `ahci_port_intr` completes them from the
//! interrupt or from `ahci_poll`, and recovers from task file errors (soft reset, port reset,
//! the NCQ error log page). `ahci_ata_cmd_timeout` aborts a command that took too long.
//!
//! ## Deviations
//! - `HIBERNATE` is not configured: `ahci_hibernate_io_start`, `ahci_hibernate_io_poll`,
//!   `ahci_hibernate_load_prdt` and `ahci_hibernate_io` are not compiled, as in a kernel
//!   without that option (nvme.rs does the same). `AHCI_DEBUG` is not defined: the `DPRINTF`s
//!   and the verbose capability report of `ahci_attach` are comments. `AHCI_COALESCE` is not
//!   defined: command coalescing (`ahci_attach`, `ahci_enable_interrupts`,
//!   `ahci_default_port_start`, `ahci_port_stop`, `ahci_intr`) is not compiled. `DIAGNOSTIC`
//!   is feature `diagnostic`.
//! - The functions that return the C's 0/1 or an errno (`ahci_attach`, `ahci_init`,
//!   `ahci_port_alloc`, `ahci_port_init`, `ahci_port_clo`, `ahci_port_softreset`,
//!   `ahci_pmp_port_softreset`, `ahci_pmp_port_portreset`, `ahci_port_portreset`,
//!   `ahci_port_portreset_poll`, `ahci_port_portreset_finish`, `ahci_port_detect_pmp`,
//!   `ahci_load_prdt`, `ahci_poll`, `ahci_port_read_ncq_error`, `ahci_wait_ne`,
//!   `ahci_pwait_eq`, `ahci_pmp_read`, `ahci_pmp_write`, `ahci_pmp_phy_status`,
//!   `ahci_pmp_identify`, `sc_port_start`) return `Result<(), Errno>` (`Result<u32, Errno>`
//!   for the PMP reads), the C's 1 being `EIO` (`ETIMEDOUT` for the wait loops).
//!   `ahci_port_stop` returns `Err("CR")` or `Err("FR")` for the C's 1 and 2, the register its
//!   callers name. `ahci_port_read_ncq_error` returns the errored slot instead of writing it
//!   through `err_slotp`.
//! - `ahci_poll`'s `timeout_fn` is an `Option<TimeoutFn>` called with the ccb as its
//!   argument, as the C. `ccb_done` is a `Cell` of a function pointer.
//! - `dma_alloc(9)` is not ported: `ap_err_scratch` is `malloc(9)`ed (`M_DEVBUF`).
//! - `ahci_port_intr`'s `goto failall` and `goto fatal`, which jump into the body of an `if`,
//!   are a labelled block that yields how the error path ends, followed by that body.
//! - The `KASSERT`s are `kassert!` (feature `diagnostic`); the C's `ffs(x) - 1` is
//!   `trailing_zeros`.
//! - A slot the controller or the NCQ log page names is looked up through
//!   [`AhciPort::ccb`], which panics past `sc_ncmds` where the C would index past `ap_ccbs`.

use core::ffi::c_void;
use core::mem::size_of;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicU32, Ordering};

use crate::dev::ata::atascsi::{
    ASAA_CAP_NCQ, ASAA_CAP_PMP_NCQ, ATA_C_READ_LOG_EXT, ATA_C_READ_PM, ATA_C_WRITE_PM,
    ATA_F_GET_RFIS, ATA_F_NCQ, ATA_F_NOWAIT, ATA_F_PACKET, ATA_F_PIO, ATA_F_POLL, ATA_F_READ,
    ATA_F_WRITE, ATA_FIS_CONTROL_4BIT, ATA_FIS_CONTROL_SRST, ATA_FIS_TYPE_D2H, ATA_FIS_TYPE_H2D,
    ATA_H2D_DEVICE_LBA, ATA_H2D_FLAGS_CMD, ATA_LOG_10H_TYPE_NOTQUEUED, ATA_LOG_10H_TYPE_TAG_MASK,
    ATA_PORT_T_ATAPI, ATA_PORT_T_DISK, ATA_PORT_T_NONE, ATA_PORT_T_PM, ATA_S_COMPLETE, ATA_S_ERROR,
    ATA_S_ONCHIP, ATA_S_PENDING, ATA_S_PUT, ATA_S_SETUP, ATA_S_TIMEOUT, AtaFisH2d, AtaLogPage10h,
    AtaXfer, AtascsiAttachArgs, AtascsiMethods, SATA_SIGNATURE_ATAPI,
    SATA_SIGNATURE_PORT_MULTIPLIER, ata_complete, atascsi_attach, atascsi_detach,
};
use crate::dev::ata::pmreg::{
    SATA_PFMT_PM_FEA, SATA_PFMT_PM_REV, SATA_PMP_CONTROL_PORT, SATA_PMREG_FEA, SATA_PMREG_FEAEN,
    SATA_PMREG_SCTL, SATA_PMREG_SERR, SATA_PMREG_SSTS,
};
use crate::dev::ic::ahcireg::{
    AHCI_CMD_LIST_FLAG_A, AHCI_CMD_LIST_FLAG_C, AHCI_CMD_LIST_FLAG_PMP,
    AHCI_CMD_LIST_FLAG_PMP_SHIFT, AHCI_CMD_LIST_FLAG_R, AHCI_CMD_LIST_FLAG_W, AHCI_MAX_PORTS,
    AHCI_MAX_PRDT, AHCI_PFMT_IS, AHCI_PFMT_TFD_STS, AHCI_PORT_SIZE, AHCI_PRDT_FLAG_INTR,
    AHCI_PREG_CI, AHCI_PREG_CI_ALL_SLOTS, AHCI_PREG_CLB, AHCI_PREG_CLBU, AHCI_PREG_CMD,
    AHCI_PREG_CMD_CLO, AHCI_PREG_CMD_CR, AHCI_PREG_CMD_FR, AHCI_PREG_CMD_FRE, AHCI_PREG_CMD_ICC,
    AHCI_PREG_CMD_ICC_ACTIVE, AHCI_PREG_CMD_PMA, AHCI_PREG_CMD_POD, AHCI_PREG_CMD_ST,
    AHCI_PREG_CMD_SUD, AHCI_PREG_FB, AHCI_PREG_FBU, AHCI_PREG_IE, AHCI_PREG_IE_DHRE,
    AHCI_PREG_IE_DPE, AHCI_PREG_IE_HBFE, AHCI_PREG_IE_IFE, AHCI_PREG_IE_IPME, AHCI_PREG_IE_OFE,
    AHCI_PREG_IE_SDBE, AHCI_PREG_IE_TFEE, AHCI_PREG_IE_UFE, AHCI_PREG_IS, AHCI_PREG_IS_DHRS,
    AHCI_PREG_IS_HBFS, AHCI_PREG_IS_IFS, AHCI_PREG_IS_IPMS, AHCI_PREG_IS_OFS, AHCI_PREG_IS_TFES,
    AHCI_PREG_IS_UFS, AHCI_PREG_SACT, AHCI_PREG_SCTL, AHCI_PREG_SCTL_DET, AHCI_PREG_SCTL_DET_INIT,
    AHCI_PREG_SCTL_DET_NONE, AHCI_PREG_SCTL_IPM_DISABLED, AHCI_PREG_SCTL_SPD_ANY,
    AHCI_PREG_SCTL_SPD_GEN1, AHCI_PREG_SERR, AHCI_PREG_SERR_DIAG_X, AHCI_PREG_SIG, AHCI_PREG_SNTF,
    AHCI_PREG_SSTS, AHCI_PREG_SSTS_DET, AHCI_PREG_SSTS_DET_DEV, AHCI_PREG_SSTS_DET_DEV_NE,
    AHCI_PREG_SSTS_DET_PHYOFFLINE, AHCI_PREG_SSTS_SPD, AHCI_PREG_SSTS_SPD_GEN1,
    AHCI_PREG_SSTS_SPD_GEN2, AHCI_PREG_SSTS_SPD_GEN3, AHCI_PREG_TFD, AHCI_PREG_TFD_STS_BSY,
    AHCI_PREG_TFD_STS_DRQ, AHCI_PREG_TFD_STS_ERR, AHCI_REG_CAP, AHCI_REG_CAP_SCLO,
    AHCI_REG_CAP_SMPS, AHCI_REG_CAP_SNCQ, AHCI_REG_CAP_SPM, AHCI_REG_CAP_SSNTF, AHCI_REG_CAP_SSS,
    AHCI_REG_GHC, AHCI_REG_GHC_AE, AHCI_REG_GHC_HR, AHCI_REG_GHC_IE, AHCI_REG_IS, AHCI_REG_PI,
    AHCI_REG_VS, AHCI_REG_VS_0_95, AHCI_REG_VS_1_0, AHCI_REG_VS_1_1, AHCI_REG_VS_1_2,
    AHCI_REG_VS_1_3, AHCI_REG_VS_1_3_1, AhciCmdHdr, AhciCmdTable, AhciPrdt, AhciRfis,
    ahci_port_region, ahci_preg_cmd_ccs, ahci_reg_cap_ncs,
};
use crate::dev::ic::ahcivar::{
    AHCI_F_IPMS_PROBE, AHCI_F_NO_NCQ, AHCI_F_NO_PMP, AP_S_ERROR_RECOVERY, AP_S_FATAL_ERROR,
    AP_S_NORMAL, AP_S_PMP_PORT_PROBE, AP_S_PMP_PROBE, AhciCcb, AhciDmamem, AhciPort, AhciSoftc,
    ahci_dma_dva, ahci_dma_kva, ahci_dma_map, ahci_port_start,
};
use crate::kassert;
use crate::kern::kern_lock::{mtx_enter, mtx_init, mtx_leave};
use crate::kern::kern_malloc::{free, malloc, mallocarray};
use crate::kern::kern_timeout::{timeout_add_msec, timeout_del, timeout_set};
use crate::kern::subr_autoconf::config_activate_children;
use crate::kern::subr_prf::{Bitmask, panic, printf};
use crate::machine::bus::{
    BUS_DMA_ALLOCNOW, BUS_DMA_COHERENT, BUS_DMA_NOWAIT, BUS_DMA_WAITOK, BUS_DMA_ZERO,
    BUS_DMASYNC_POSTREAD, BUS_DMASYNC_POSTWRITE, BUS_DMASYNC_PREREAD, BUS_DMASYNC_PREWRITE,
    BUS_SPACE_BARRIER_READ, BUS_SPACE_BARRIER_WRITE, BusDmaSegment, BusSize, BusSpaceHandle,
    BusSpaceTag, bus_dmamap_create, bus_dmamap_destroy, bus_dmamap_load, bus_dmamap_sync,
    bus_dmamap_unload, bus_dmamem_alloc, bus_dmamem_free, bus_dmamem_map, bus_dmamem_unmap,
    bus_space_barrier, bus_space_read_4, bus_space_subregion, bus_space_write_4,
};
use crate::machine::cpu::delay;
use crate::machine::intr::{IPL_BIO, splassert, splbio, splx};
use crate::sys::device::{Cfdriver, DV_DULL, DVACT_POWERDOWN, DVACT_RESUME, Device};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_DEVBUF, M_NOWAIT, M_ZERO};
use crate::sys::param::{DEV_BSIZE, MAXPHYS, PAGE_SIZE};
use crate::sys::queue::TailqEntry;
use crate::sys::timeout::TimeoutFn;

/// `ahci_cd`.
pub static AHCI_CD: Cfdriver = Cfdriver::new(b"ahci", DV_DULL, 0);

/// `ahci_atascsi_methods`: what atascsi calls.
pub static AHCI_ATASCSI_METHODS: AtascsiMethods = AtascsiMethods {
    ata_probe: ahci_ata_probe,
    ata_free: ahci_ata_free,
    ata_get_xfer: ahci_ata_get_xfer,
    ata_put_xfer: ahci_ata_put_xfer,
    ata_cmd: ahci_ata_cmd,
};

/// `ahci_pwait_clr(_ap, _r, _b, _n)`: waits for all bits in `b` to be cleared.
#[inline]
fn ahci_pwait_clr(ap: &AhciPort, r: BusSize, b: u32, n: i32) -> Result<(), Errno> {
    ahci_pwait_eq(ap, r, b, 0, n)
}

/// The softc as the `void *` cookie of atascsi and the interrupt handler.
fn ahci_cookie(sc: &AhciSoftc) -> *mut c_void {
    ptr::from_ref(sc).cast_mut().cast()
}

/// A ccb as the `void *` argument of its timeout functions.
fn ahci_ccb_cookie(ccb: &AhciCcb) -> *mut c_void {
    ptr::from_ref(ccb).cast_mut().cast()
}

/// `(struct ahci_ccb *)xa`: the ccb an `ata_xfer` is the first member of.
///
/// # Safety
///
/// `xa` is the `ccb_xa` of an [`AhciCcb`]: it came from [`ahci_ata_get_xfer`] (atascsi
/// calls ahci's methods only with the xfers ahci handed out).
unsafe fn ahci_xa_ccb(xa: &'static AtaXfer) -> &'static AhciCcb {
    // SAFETY: the caller's guarantee; `AhciCcb` is `#[repr(C)]` with `ccb_xa` first, so the
    // xfer's address is the ccb's.
    unsafe { &*ptr::from_ref(xa).cast::<AhciCcb>() }
}

/// `1 << slot` of a ccb.
#[inline]
fn ahci_slot_bit(ccb: &AhciCcb) -> u32 {
    1u32 << ccb.ccb_slot
}

/// `ahci_attach`: brings up a controller whose registers and interrupt the bus front-end
/// set up.
pub fn ahci_attach(sc: &'static AhciSoftc) -> Result<(), Errno> {
    if sc.sc_port_start.get().is_none() {
        sc.sc_port_start.set(Some(ahci_default_port_start));
    }

    'unmap: {
        if ahci_init(sc).is_err() {
            // error already printed by ahci_init
            break 'unmap;
        }

        printf(format_args!("\n"));

        sc.sc_cap.set(ahci_read(sc, AHCI_REG_CAP));
        sc.sc_ncmds.set(ahci_reg_cap_ncs(sc.sc_cap.get()));
        // AHCI_DEBUG (not defined): the capabilities, port and command counts and the
        // interface speed generation, and the extended capabilities, with AHCI_D_VERBOSE.

        let pi = ahci_read(sc, AHCI_REG_PI);
        // DPRINTF(AHCI_D_VERBOSE, "%s: ports implemented: 0x%08x\n", DEVNAME(sc), pi);

        // AHCI_COALESCE (not defined): naive coalescing support for all ports.

        // Given that ahci_port_alloc() will grab one CCB for error recovery in the NCQ case
        // from the pool of CCBs sized based on sc->sc_ncmds pretend at least 2 command slots
        // for devices without NCQ support. That way, also at least 1 slot is made available
        // for atascsi(4).
        sc.sc_ncmds.set(sc.sc_ncmds.get().max(2));
        'freeports: {
            for i in 0..AHCI_MAX_PORTS {
                if pi & (1u32 << i) == 0 {
                    // don't allocate stuff if the port isn't implemented
                    continue;
                }

                if ahci_port_alloc(sc, i) == Err(Errno::ENOMEM) {
                    break 'freeports;
                }

                if let Some(ap) = sc.port(i) {
                    ahci_port_portreset_start(ap);
                }
            }

            // Poll for device detection until all ports report a device, or one second has
            // elapsed.
            for _ in 0..1000 {
                let mut done = true;
                for j in 0..AHCI_MAX_PORTS {
                    let Some(ap) = sc.port(j) else {
                        continue;
                    };

                    if ahci_port_portreset_poll(ap).is_err() {
                        done = false;
                    }
                }

                if done {
                    break;
                }

                delay(1000);
            }

            // Finish device detection on all ports that initialized.
            for i in 0..AHCI_MAX_PORTS {
                if sc.port(i).is_some() {
                    ahci_port_detect(sc, i);
                }
            }

            let mut aaa = AtascsiAttachArgs {
                aaa_cookie: ahci_cookie(sc),
                aaa_methods: &AHCI_ATASCSI_METHODS,
                aaa_minphys: None,
                aaa_nports: AHCI_MAX_PORTS as i32,
                aaa_ncmds: sc.sc_ncmds.get() as i32 - 1,
                aaa_capability: 0,
            };
            if sc.sc_flags.get() & AHCI_F_NO_NCQ == 0
                && sc.sc_ncmds.get() > 2
                && sc.sc_cap.get() & AHCI_REG_CAP_SNCQ != 0
            {
                aaa.aaa_capability |= ASAA_CAP_NCQ | ASAA_CAP_PMP_NCQ;
            }

            sc.sc_atascsi.set(Some(atascsi_attach(&sc.sc_dev, &aaa)));

            // Flush all residual bits of the interrupt status register
            ahci_write(sc, AHCI_REG_IS, ahci_read(sc, AHCI_REG_IS));

            // Enable interrupts
            ahci_write(sc, AHCI_REG_GHC, AHCI_REG_GHC_AE | AHCI_REG_GHC_IE);

            return Ok(());
        }

        // freeports:
        for i in 0..AHCI_MAX_PORTS {
            if sc.port(i).is_some() {
                ahci_port_free(sc, i);
            }
        }
    }

    // unmap:
    // Disable controller
    ahci_write(sc, AHCI_REG_GHC, 0);
    Err(Errno::EIO)
}

/// `ahci_detach`.
pub fn ahci_detach(sc: &'static AhciSoftc, flags: i32) -> Result<(), Errno> {
    if let Some(as_) = sc.sc_atascsi.get() {
        // SAFETY: the atascsi ahci_attach made; on success it is gone and forgotten here.
        unsafe { atascsi_detach(as_, flags) }?;
        sc.sc_atascsi.set(None);
    }

    for i in 0..AHCI_MAX_PORTS {
        if sc.port(i).is_some() {
            ahci_port_free(sc, i);
        }
    }

    Ok(())
}

/// `ahci_activate`.
pub fn ahci_activate(self_: &Device, act: i32) -> Result<(), Errno> {
    // SAFETY: the front-ends' softcs begin with the `struct ahci_softc`, as the C's cast
    // assumes; softcs are never freed while the device exists.
    let sc: &'static AhciSoftc = unsafe { &*ptr::from_ref(self_).cast::<AhciSoftc>() };

    match act {
        DVACT_RESUME => {
            // enable ahci (global interrupts disabled)
            ahci_write(sc, AHCI_REG_GHC, AHCI_REG_GHC_AE);

            // restore BIOS initialised parameters
            ahci_write(sc, AHCI_REG_CAP, sc.sc_cap.get());

            for i in 0..AHCI_MAX_PORTS {
                if sc.port(i).is_some() {
                    let _ = ahci_port_init(sc, i);
                }
            }

            // Enable interrupts
            ahci_write(sc, AHCI_REG_GHC, AHCI_REG_GHC_AE | AHCI_REG_GHC_IE);

            config_activate_children(self_, act)
        }
        DVACT_POWERDOWN => {
            let rv = config_activate_children(self_, act);
            for i in 0..AHCI_MAX_PORTS {
                if let Some(ap) = sc.port(i) {
                    let _ = ahci_port_stop(ap, true);
                }
            }
            rv
        }
        _ => config_activate_children(self_, act),
    }
}

/// `ahci_init`: resets the controller (keeping the firmware's capabilities and ports) and
/// enables AHCI mode; prints the revision.
pub fn ahci_init(sc: &AhciSoftc) -> Result<(), Errno> {
    // DPRINTF(AHCI_D_VERBOSE, " GHC 0x%b", ahci_read(sc, AHCI_REG_GHC), AHCI_FMT_GHC);

    // save BIOS initialised parameters, enable staggered spin up
    let mut cap = ahci_read(sc, AHCI_REG_CAP);
    cap &= AHCI_REG_CAP_SMPS;
    cap |= AHCI_REG_CAP_SSS;
    let pi = ahci_read(sc, AHCI_REG_PI);

    if ahci_read(sc, AHCI_REG_GHC) & AHCI_REG_GHC_AE != 0 {
        // reset the controller
        ahci_write(sc, AHCI_REG_GHC, AHCI_REG_GHC_HR);
        if ahci_wait_ne(sc, AHCI_REG_GHC, AHCI_REG_GHC_HR, AHCI_REG_GHC_HR).is_err() {
            printf(format_args!(" unable to reset controller\n"));
            return Err(Errno::EIO);
        }
    }

    // enable ahci (global interrupts disabled)
    ahci_write(sc, AHCI_REG_GHC, AHCI_REG_GHC_AE);

    // restore parameters
    ahci_write(sc, AHCI_REG_CAP, cap);
    ahci_write(sc, AHCI_REG_PI, pi);

    // check the revision
    let reg = ahci_read(sc, AHCI_REG_VS);
    let Some(revision) = ahci_revision(reg) else {
        printf(format_args!(" unsupported AHCI revision 0x{reg:08x}\n"));
        return Err(Errno::EIO);
    };

    printf(format_args!(" AHCI {revision}"));

    Ok(())
}

/// The revision `ahci_init` prints for a version register, `None` for one it does not know.
pub fn ahci_revision(vs: u32) -> Option<&'static str> {
    match vs {
        AHCI_REG_VS_0_95 => Some("0.95"),
        AHCI_REG_VS_1_0 => Some("1.0"),
        AHCI_REG_VS_1_1 => Some("1.1"),
        AHCI_REG_VS_1_2 => Some("1.2"),
        AHCI_REG_VS_1_3 => Some("1.3"),
        AHCI_REG_VS_1_3_1 => Some("1.3.1"),
        _ => None,
    }
}

/// `ahci_enable_interrupts`.
pub fn ahci_enable_interrupts(ap: &AhciPort) {
    let sc = ap.sc();
    ahci_pwrite(
        ap,
        AHCI_PREG_IE,
        AHCI_PREG_IE_TFEE
            | AHCI_PREG_IE_HBFE
            | AHCI_PREG_IE_IFE
            | AHCI_PREG_IE_OFE
            | AHCI_PREG_IE_DPE
            | AHCI_PREG_IE_UFE
            | if sc.sc_cap.get() & AHCI_REG_CAP_SSNTF != 0 {
                AHCI_PREG_IE_IPME
            } else {
                0
            }
            // AHCI_COALESCE (not defined): no SDBE/DHRE on coalesced ports.
            | AHCI_PREG_IE_SDBE
            | AHCI_PREG_IE_DHRE,
    );
}

/// `ahci_port_alloc`: allocates port `port` and its DMA memory and ccbs, and enables FIS
/// reception.
pub fn ahci_port_alloc(sc: &'static AhciSoftc, port: usize) -> Result<(), Errno> {
    let Some(mem) = malloc(size_of::<AhciPort>(), M_DEVBUF, M_NOWAIT | M_ZERO) else {
        printf(format_args!(
            "{}: unable to allocate memory for port {}\n",
            sc.sc_dev.xname(),
            port
        ));
        return Err(Errno::ENOMEM);
    };
    let app = mem.cast::<AhciPort>();
    // SAFETY: a fresh allocation of `size_of::<AhciPort>()` bytes (malloc aligns it), written
    // whole before any use; it lives until ahci_port_free.
    let ap: &'static AhciPort = unsafe {
        app.as_ptr().write(AhciPort::new());
        &*app.as_ptr()
    };

    let Some(scratch) = malloc(DEV_BSIZE, M_DEVBUF, M_NOWAIT | M_ZERO) else {
        printf(format_args!(
            "{}: unable to allocate DMA scratch buf for port {}\n",
            sc.sc_dev.xname(),
            port
        ));
        free(app.cast(), M_DEVBUF, size_of::<AhciPort>());
        return Err(Errno::ENOMEM);
    };
    ap.ap_err_scratch.set(scratch.as_ptr());

    // AHCI_DEBUG (not defined): ap_name.
    ap.ap_port.set(port as i32);
    sc.sc_ports[port].set(Some(app));

    let rc = ahci_port_alloc_regs(sc, ap, port);

    // freeport:
    if rc.is_err() {
        ahci_port_free(sc, port);
    }
    rc
}

/// The part of `ahci_port_alloc` between the port's allocation and `freeport`.
fn ahci_port_alloc_regs(
    sc: &'static AhciSoftc,
    ap: &'static AhciPort,
    port: usize,
) -> Result<(), Errno> {
    let (iot, ioh) = sc.regs();
    let dmat = sc.dmat();

    let Ok(pioh) = bus_space_subregion(iot, ioh, ahci_port_region(port), AHCI_PORT_SIZE) else {
        printf(format_args!(
            "{}: unable to create register window for port {}\n",
            sc.sc_dev.xname(),
            port
        ));
        return Err(Errno::ENOMEM);
    };
    ap.ap_ioh.set(Some(pioh));

    ap.ap_sc.set(Some(sc));
    // AHCI_COALESCE (not defined): ap_num.
    ap.ap_ccb_free.init();
    ap.ap_ccb_pending.init();
    mtx_init(&ap.ap_ccb_mtx, IPL_BIO);

    // Disable port interrupts
    ahci_pwrite(ap, AHCI_PREG_IE, 0);

    // Sec 10.1.2 - deinitialise port if it is already running
    ahci_port_deinit(sc, ap, port)?;

    let nomem = || {
        printf(format_args!(
            "{}: unable to allocate DMA memory for port {}\n",
            sc.sc_dev.xname(),
            port
        ));
        Err(Errno::ENOMEM)
    };

    // Allocate RFIS
    let Some(rfis) = ahci_dmamem_alloc(sc, size_of::<AhciRfis>()) else {
        return nomem();
    };
    ap.ap_dmamem_rfis.set(Some(rfis));

    // Setup RFIS base address
    ahci_port_set_fis_base(ap, rfis);

    // Enable FIS reception and activate port.
    ahci_port_activate(ap)?;

    // Allocate a CCB for each command slot
    let ncmds = sc.sc_ncmds.get() as usize;
    let Some(ccbs) = mallocarray(ncmds, size_of::<AhciCcb>(), M_DEVBUF, M_NOWAIT | M_ZERO) else {
        printf(format_args!(
            "{}: unable to allocate command list for port {}\n",
            sc.sc_dev.xname(),
            port
        ));
        return Err(Errno::ENOMEM);
    };
    let ccbs = ccbs.cast::<AhciCcb>();
    ap.ap_ccbs.set(ccbs.as_ptr());

    // Command List Structures and Command Tables
    let cmd_list = ahci_dmamem_alloc(sc, ncmds * size_of::<AhciCmdHdr>());
    ap.ap_dmamem_cmd_list.set(cmd_list);
    let cmd_table = ahci_dmamem_alloc(sc, ncmds * size_of::<AhciCmdTable>());
    ap.ap_dmamem_cmd_table.set(cmd_table);
    let (Some(cmd_list), Some(cmd_table)) = (cmd_list, cmd_table) else {
        return nomem();
    };

    // Setup command list base address
    let dva = ahci_dma_dva(cmd_list);
    ahci_pwrite(ap, AHCI_PREG_CLBU, (dva >> 32) as u32);
    ahci_pwrite(ap, AHCI_PREG_CLB, dva as u32);

    // Split CCB allocation into CCBs and assign to command header/table
    let hdr = ahci_dma_kva(cmd_list).cast::<AhciCmdHdr>();
    let table = ahci_dma_kva(cmd_table).cast::<AhciCmdTable>();
    for i in 0..ncmds {
        let Ok(map) = bus_dmamap_create(
            dmat,
            MAXPHYS,
            AHCI_MAX_PRDT as i32,
            4 * 1024 * 1024,
            0,
            BUS_DMA_NOWAIT | BUS_DMA_ALLOCNOW,
        ) else {
            printf(format_args!(
                "{}: unable to create dmamap for port {} ccb {}\n",
                sc.sc_dev.xname(),
                port,
                i
            ));
            return Err(Errno::ENOMEM);
        };

        // SAFETY: `i` is below `sc_ncmds`, the length of the ccb array and of both DMA
        // areas; the ccb is written whole before anything reads it, and lives until
        // ahci_port_free.
        let ccb: &'static AhciCcb = unsafe {
            let hdr_i = hdr.add(i);
            let table_i = table.add(i);
            let p = ccbs.as_ptr().add(i);
            p.write(AhciCcb {
                ccb_xa: AtaXfer::new(
                    ptr::addr_of_mut!((*table_i).cfis).cast::<AtaFisH2d>(),
                    ptr::addr_of_mut!((*table_i).acmd).cast::<u8>(),
                    i as u8,
                ),
                ccb_slot: i as i32,
                ccb_port: ap,
                ccb_dmamap: map,
                ccb_cmd_hdr: hdr_i,
                ccb_cmd_table: table_i,
                ccb_done: core::cell::Cell::new(ahci_empty_done),
                ccb_entry: TailqEntry::new(),
            });
            &*p
        };
        ccb.set_hdr_ctba(ahci_dma_dva(cmd_table) + (i * size_of::<AhciCmdTable>()) as u64);

        ccb.ccb_xa.set_state(ATA_S_COMPLETE);
        ahci_put_ccb(ccb);
    }

    // grab a ccb for use during error recovery
    let err = ap.ccb(sc.sc_ncmds.get() - 1);
    ap.ap_ccb_err.set(Some(err));
    mtx_enter(&ap.ap_ccb_mtx);
    // SAFETY: every ccb was just put on the free list.
    unsafe { ap.ap_ccb_free.remove(err) };
    mtx_leave(&ap.ap_ccb_mtx);
    err.ccb_xa.set_state(ATA_S_COMPLETE);

    // Wait for ICC change to complete
    let _ = ahci_pwait_clr(ap, AHCI_PREG_CMD, AHCI_PREG_CMD_ICC, 1);

    Ok(())
}

/// Sec 10.1.2 of `ahci_port_alloc` and `ahci_port_init`: deinitialise the port if it is
/// already running.
fn ahci_port_deinit(sc: &AhciSoftc, ap: &AhciPort, port: usize) -> Result<(), Errno> {
    let cmd = ahci_pread(ap, AHCI_PREG_CMD);
    if cmd & (AHCI_PREG_CMD_ST | AHCI_PREG_CMD_CR | AHCI_PREG_CMD_FRE | AHCI_PREG_CMD_FR) != 0
        || ahci_pread(ap, AHCI_PREG_SCTL) & AHCI_PREG_SCTL_DET != 0
    {
        if let Err(r) = ahci_port_stop(ap, true) {
            printf(format_args!(
                "{}: unable to disable {}, ignoring port {}\n",
                sc.sc_dev.xname(),
                r,
                port
            ));
            return Err(Errno::ENXIO);
        }

        // Write DET to zero
        ahci_pwrite(ap, AHCI_PREG_SCTL, 0);
    }
    Ok(())
}

/// "Setup RFIS base address" of `ahci_port_alloc` and `ahci_port_init`.
fn ahci_port_set_fis_base(ap: &AhciPort, rfis: &AhciDmamem) {
    ap.ap_rfis.set(ahci_dma_kva(rfis).cast::<AhciRfis>());
    let dva = ahci_dma_dva(rfis);
    ahci_pwrite(ap, AHCI_PREG_FBU, (dva >> 32) as u32);
    ahci_pwrite(ap, AHCI_PREG_FB, dva as u32);
}

/// "Enable FIS reception and activate port" of `ahci_port_alloc` and `ahci_port_init`:
/// `ENXIO` when the port does not take FRE.
fn ahci_port_activate(ap: &AhciPort) -> Result<(), Errno> {
    let mut cmd = ahci_pread(ap, AHCI_PREG_CMD) & !AHCI_PREG_CMD_ICC;
    cmd |= AHCI_PREG_CMD_FRE | AHCI_PREG_CMD_POD | AHCI_PREG_CMD_SUD;
    ahci_pwrite(ap, AHCI_PREG_CMD, cmd | AHCI_PREG_CMD_ICC_ACTIVE);

    // Check whether port activated. Skip it if not.
    let cmd = ahci_pread(ap, AHCI_PREG_CMD) & !AHCI_PREG_CMD_ICC;
    if cmd & AHCI_PREG_CMD_FRE == 0 {
        return Err(Errno::ENXIO);
    }
    Ok(())
}

/// The reset results `ahci_port_detect` and `ahci_port_init` print: `Err` when the port is
/// to be given up.
fn ahci_port_reset_result(
    sc: &AhciSoftc,
    ap: &'static AhciPort,
    port: usize,
    rc: Result<(), Errno>,
) -> Result<(), Errno> {
    match rc {
        Err(Errno::ENODEV) => {
            match ahci_pread(ap, AHCI_PREG_SSTS) & AHCI_PREG_SSTS_DET {
                AHCI_PREG_SSTS_DET_DEV_NE => {
                    printf(format_args!(
                        "{}: device not communicating on port {}\n",
                        sc.sc_dev.xname(),
                        port
                    ));
                }
                AHCI_PREG_SSTS_DET_PHYOFFLINE => {
                    printf(format_args!(
                        "{}: PHY offline on port {}\n",
                        sc.sc_dev.xname(),
                        port
                    ));
                }
                _ => {
                    // DPRINTF(AHCI_D_VERBOSE, "%s: no device detected on port %d\n", ...);
                }
            }
            Err(Errno::ENODEV)
        }

        Err(Errno::EBUSY) => {
            printf(format_args!(
                "{}: device on port {} didn't come ready, TFD: 0x{}\n",
                sc.sc_dev.xname(),
                port,
                Bitmask(u64::from(ahci_pread(ap, AHCI_PREG_TFD)), AHCI_PFMT_TFD_STS)
            ));

            // Try a soft reset to clear busy
            let rc = ahci_port_softreset(ap);
            if rc.is_err() {
                printf(format_args!(
                    "{}: unable to communicate with device on port {}\n",
                    sc.sc_dev.xname(),
                    port
                ));
            }
            rc
        }

        _ => Ok(()),
    }
}

/// `ahci_port_detect`: finishes the port reset `ahci_attach` started, and starts the port
/// when a device answered; the port is freed otherwise.
pub fn ahci_port_detect(sc: &'static AhciSoftc, port: usize) {
    let Some(ap) = sc.port(port) else {
        return;
    };

    let rc = ahci_port_portreset_finish(ap, true);
    let mut rc = ahci_port_reset_result(sc, ap, port, rc);

    if rc.is_ok() {
        // DPRINTF(AHCI_D_VERBOSE, "%s: detected device on port %d; %d\n", ...);

        // Read current link speed
        let speed = match ahci_pread(ap, AHCI_PREG_SSTS) & AHCI_PREG_SSTS_SPD {
            AHCI_PREG_SSTS_SPD_GEN1 => Some("1.5Gb/s"),
            AHCI_PREG_SSTS_SPD_GEN2 => Some("3.0Gb/s"),
            AHCI_PREG_SSTS_SPD_GEN3 => Some("6.0Gb/s"),
            _ => None,
        };
        if let Some(speed) = speed {
            printf(format_args!(
                "{}: port {}: {}\n",
                ap.portname(),
                port,
                speed
            ));
        }

        // Enable command transfers on port
        if ahci_port_start(ap, false).is_err() {
            printf(format_args!(
                "{}: failed to start command DMA on port {}, disabling\n",
                sc.sc_dev.xname(),
                port
            ));
            rc = Err(Errno::ENXIO); // couldn't start port
        }

        // Flush interrupts for port
        ahci_pwrite(ap, AHCI_PREG_IS, ahci_pread(ap, AHCI_PREG_IS));
        ahci_write(sc, AHCI_REG_IS, 1 << port);

        ahci_enable_interrupts(ap);
    }

    // freeport:
    if rc.is_err() {
        ahci_port_free(sc, port);
    }
}

/// `ahci_port_free`: disables the port and frees it.
pub fn ahci_port_free(sc: &AhciSoftc, port: usize) {
    let Some(app) = sc.sc_ports[port].get() else {
        return;
    };
    // SAFETY: a port ahci_port_alloc allocated and wrote; freed below.
    let ap: &AhciPort = unsafe { app.as_ref() };

    // Ensure port is disabled and its interrupts are flushed
    if ap.ap_sc.get().is_some() {
        ahci_pwrite(ap, AHCI_PREG_CMD, 0);
        ahci_pwrite(ap, AHCI_PREG_IE, 0);
        ahci_pwrite(ap, AHCI_PREG_IS, ahci_pread(ap, AHCI_PREG_IS));
        ahci_write(sc, AHCI_REG_IS, 1 << port);
    }

    if let Some(err) = ap.ap_ccb_err.get() {
        ahci_put_ccb(err);
    }

    let ccbs = ap.ap_ccbs.get();
    if let Some(ccbs) = NonNull::new(ccbs) {
        while let Some(ccb) = ahci_get_ccb(ap) {
            // SAFETY: the ccb's own map, which nothing else holds.
            unsafe { bus_dmamap_destroy(sc.dmat(), NonNull::from(ccb.ccb_dmamap)) };
        }
        free(
            ccbs.cast(),
            M_DEVBUF,
            sc.sc_ncmds.get() as usize * size_of::<AhciCcb>(),
        );
    }

    if let Some(adm) = ap.ap_dmamem_cmd_list.get() {
        // SAFETY: the port's command list, which the stopped controller no longer uses.
        unsafe { ahci_dmamem_free(sc, adm) };
    }
    if let Some(adm) = ap.ap_dmamem_rfis.get() {
        // SAFETY: as above, the received FIS area.
        unsafe { ahci_dmamem_free(sc, adm) };
    }
    if let Some(adm) = ap.ap_dmamem_cmd_table.get() {
        // SAFETY: as above, the command tables.
        unsafe { ahci_dmamem_free(sc, adm) };
    }
    if let Some(scratch) = NonNull::new(ap.ap_err_scratch.get()) {
        free(scratch, M_DEVBUF, DEV_BSIZE);
    }

    // bus_space(9) says we dont free the subregions handle

    sc.sc_ports[port].set(None);
    free(app.cast(), M_DEVBUF, size_of::<AhciPort>());
}

/// `ahci_port_init`: brings a port back after a resume.
pub fn ahci_port_init(sc: &'static AhciSoftc, port: usize) -> Result<(), Errno> {
    let Some(ap) = sc.port(port) else {
        return Err(Errno::ENOMEM);
    };
    // AHCI_DEBUG (not defined): ap_name.

    // Disable port interrupts
    ahci_pwrite(ap, AHCI_PREG_IE, 0);

    // Sec 10.1.2 - deinitialise port if it is already running
    ahci_port_deinit(sc, ap, port)?;

    // Setup RFIS base address
    if let Some(rfis) = ap.ap_dmamem_rfis.get() {
        ahci_port_set_fis_base(ap, rfis);
    }

    // Enable FIS reception and activate port.
    ahci_port_activate(ap)?;

    // Setup command list base address
    if let Some(cmd_list) = ap.ap_dmamem_cmd_list.get() {
        let dva = ahci_dma_dva(cmd_list);
        ahci_pwrite(ap, AHCI_PREG_CLBU, (dva >> 32) as u32);
        ahci_pwrite(ap, AHCI_PREG_CLB, dva as u32);
    }

    // Wait for ICC change to complete
    let _ = ahci_pwait_clr(ap, AHCI_PREG_CMD, AHCI_PREG_CMD_ICC, 1);

    // Reset port
    let rc = ahci_port_portreset(ap, true);
    let mut rc = ahci_port_reset_result(sc, ap, port, rc);
    rc?;
    // DPRINTF(AHCI_D_VERBOSE, "%s: detected device on port %d\n", DEVNAME(sc), port);

    if ap.ap_pmp_ports.get() > 0 {
        for p in 0..ap.ap_pmp_ports.get() {
            // might need to do a portreset first here?

            // softreset the port
            if ahci_pmp_port_softreset(ap, p).is_err() {
                printf(format_args!(
                    "{}.{}: unable to probe PMP port due to softreset failure\n",
                    ap.portname(),
                    p
                ));
                continue;
            }

            let sig = ahci_port_signature(ap);
            printf(format_args!(
                "{}.{}: port signature returned {}\n",
                ap.portname(),
                p,
                sig
            ));
        }
    }

    // Enable command transfers on port
    if ahci_port_start(ap, false).is_err() {
        printf(format_args!(
            "{}: failed to start command DMA on port {}, disabling\n",
            sc.sc_dev.xname(),
            port
        ));
        rc = Err(Errno::ENXIO); // couldn't start port
    }

    // Flush interrupts for port
    ahci_pwrite(ap, AHCI_PREG_IS, ahci_pread(ap, AHCI_PREG_IS));
    ahci_write(sc, AHCI_REG_IS, 1 << port);

    ahci_enable_interrupts(ap);

    rc
}

/// `ahci_default_port_start`: turns on FRE (and ST).
pub fn ahci_default_port_start(ap: &AhciPort, fre_only: bool) -> Result<(), Errno> {
    // Turn on FRE (and ST)
    let mut r = ahci_pread(ap, AHCI_PREG_CMD) & !AHCI_PREG_CMD_ICC;
    r |= AHCI_PREG_CMD_FRE;
    if !fre_only {
        r |= AHCI_PREG_CMD_ST;
    }
    ahci_pwrite(ap, AHCI_PREG_CMD, r);

    // AHCI_COALESCE (not defined): (re-)enable coalescing on the port.

    Ok(())
}

/// `ahci_port_stop`: turns off ST (and FRE) and waits for the engines to stop; the error
/// names the engine that did not (the C's 1, "CR", and 2, "FR").
pub fn ahci_port_stop(ap: &AhciPort, stop_fis_rx: bool) -> Result<(), &'static str> {
    // AHCI_COALESCE (not defined): disable coalescing on the port while it is stopped.

    // Turn off ST (and FRE)
    let mut r = ahci_pread(ap, AHCI_PREG_CMD) & !AHCI_PREG_CMD_ICC;
    r &= !AHCI_PREG_CMD_ST;
    if stop_fis_rx {
        r &= !AHCI_PREG_CMD_FRE;
    }
    ahci_pwrite(ap, AHCI_PREG_CMD, r);

    // Wait for CR to go off
    if ahci_pwait_clr(ap, AHCI_PREG_CMD, AHCI_PREG_CMD_CR, 1).is_err() {
        return Err("CR");
    }

    // Wait for FR to go off
    if stop_fis_rx && ahci_pwait_clr(ap, AHCI_PREG_CMD, AHCI_PREG_CMD_FR, 1).is_err() {
        return Err("FR");
    }

    Ok(())
}

/// `ahci_port_clo`: AHCI command list override -> forcibly clear TFD.STS.{BSY,DRQ}.
pub fn ahci_port_clo(ap: &AhciPort) -> Result<(), Errno> {
    let sc = ap.sc();

    // Only attempt CLO if supported by controller
    if ahci_read(sc, AHCI_REG_CAP) & AHCI_REG_CAP_SCLO == 0 {
        return Err(Errno::EIO);
    }

    // Issue CLO
    let cmd = ahci_pread(ap, AHCI_PREG_CMD) & !AHCI_PREG_CMD_ICC;
    #[cfg(feature = "diagnostic")]
    if cmd & AHCI_PREG_CMD_ST != 0 {
        printf(format_args!(
            "{}: CLO requested while port running\n",
            ap.portname()
        ));
    }
    ahci_pwrite(ap, AHCI_PREG_CMD, cmd | AHCI_PREG_CMD_CLO);

    // Wait for completion
    if ahci_pwait_clr(ap, AHCI_PREG_CMD, AHCI_PREG_CMD_CLO, 1).is_err() {
        printf(format_args!("{}: CLO did not complete\n", ap.portname()));
        return Err(Errno::EIO);
    }

    Ok(())
}

/// `ahci_port_softreset`: AHCI soft reset, Section 10.4.1.
pub fn ahci_port_softreset(ap: &'static AhciPort) -> Result<(), Errno> {
    let mut ccb = None;
    let mut rc = Err(Errno::EIO);

    // DPRINTF(AHCI_D_VERBOSE, "%s: soft reset\n", PORTNAME(ap));

    let s = splbio();
    let oldstate = ap.ap_state.get();
    ap.ap_state.set(AP_S_ERROR_RECOVERY);

    // Save previous command register state
    let cmd = ahci_pread(ap, AHCI_PREG_CMD) & !AHCI_PREG_CMD_ICC;

    'err: {
        // Idle port
        if ahci_port_stop(ap, false).is_err() {
            printf(format_args!(
                "{}: failed to stop port, cannot softreset\n",
                ap.portname()
            ));
            break 'err;
        }

        // Request CLO if device appears hung
        if ahci_pread(ap, AHCI_PREG_TFD) & (AHCI_PREG_TFD_STS_BSY | AHCI_PREG_TFD_STS_DRQ) != 0 {
            let _ = ahci_port_clo(ap);
        }

        // Clear port errors to permit TFD transfer
        ahci_pwrite(ap, AHCI_PREG_SERR, ahci_pread(ap, AHCI_PREG_SERR));

        // Restart port
        if ahci_port_start(ap, false).is_err() {
            printf(format_args!(
                "{}: failed to start port, cannot softreset\n",
                ap.portname()
            ));
            break 'err;
        }

        // Check whether CLO worked
        if ahci_pwait_clr(
            ap,
            AHCI_PREG_TFD,
            AHCI_PREG_TFD_STS_BSY | AHCI_PREG_TFD_STS_DRQ,
            1,
        )
        .is_err()
        {
            printf(format_args!(
                "{}: CLO {}, need port reset\n",
                ap.portname(),
                if ahci_read(ap.sc(), AHCI_REG_CAP) & AHCI_REG_CAP_SCLO != 0 {
                    "failed"
                } else {
                    "unsupported"
                }
            ));
            rc = Err(Errno::EBUSY);
            break 'err;
        }

        // Prep first D2H command with SRST feature & clear busy/reset flags
        let c = ahci_get_err_ccb(ap);
        ccb = Some(c);
        c.zero_cmd_table();

        c.set_cfis(0, ATA_FIS_TYPE_H2D);
        c.set_cfis(15, ATA_FIS_CONTROL_SRST);

        c.set_hdr_prdtl(0);
        c.set_hdr_flags(
            5 // FIS length: 5 DWORDS
                | AHCI_CMD_LIST_FLAG_C
                | AHCI_CMD_LIST_FLAG_R
                | AHCI_CMD_LIST_FLAG_W,
        );

        c.ccb_xa.set_state(ATA_S_PENDING);
        if ahci_poll(c, 1000, None).is_err() {
            break 'err;
        }

        // Prep second D2H command to read status and complete reset sequence
        c.set_cfis(0, ATA_FIS_TYPE_H2D);
        c.set_cfis(15, 0);

        c.set_hdr_prdtl(0);
        c.set_hdr_flags(5 | AHCI_CMD_LIST_FLAG_W);

        c.ccb_xa.set_state(ATA_S_PENDING);
        if ahci_poll(c, 1000, None).is_err() {
            break 'err;
        }

        if ahci_pwait_clr(
            ap,
            AHCI_PREG_TFD,
            AHCI_PREG_TFD_STS_BSY | AHCI_PREG_TFD_STS_DRQ | AHCI_PREG_TFD_STS_ERR,
            1,
        )
        .is_err()
        {
            printf(format_args!(
                "{}: device didn't come ready after reset, TFD: 0x{}\n",
                ap.portname(),
                Bitmask(u64::from(ahci_pread(ap, AHCI_PREG_TFD)), AHCI_PFMT_TFD_STS)
            ));
            rc = Err(Errno::EBUSY);
            break 'err;
        }

        rc = Ok(());
    }

    // err:
    if let Some(c) = ccb {
        // Abort our command, if it failed, by stopping command DMA.
        if rc.is_err() && ap.ap_active.load(Ordering::Relaxed) & ahci_slot_bit(c) != 0 {
            printf(format_args!(
                "{}: stopping the port, softreset slot {} was still active.\n",
                ap.portname(),
                c.ccb_slot
            ));
            let _ = ahci_port_stop(ap, false);
        }
        c.ccb_xa.set_state(ATA_S_ERROR);
        ahci_put_err_ccb(c);
    }

    // Restore saved CMD register state
    ahci_pwrite(ap, AHCI_PREG_CMD, cmd);
    ap.ap_state.set(oldstate);

    splx(s);

    rc
}

/// `ahci_pmp_port_softreset`: soft reset of a port multiplier's port.
pub fn ahci_pmp_port_softreset(ap: &'static AhciPort, pmp_port: i32) -> Result<(), Errno> {
    let mut ccb: Option<&'static AhciCcb> = None;
    let mut rc = Ok(());

    let s = splbio();
    // ignore spurious IFS errors while resetting
    // DPRINTF(AHCI_D_VERBOSE, "%s: now ignoring IFS\n", PORTNAME(ap));
    ap.ap_pmp_ignore_ifs.set(1);

    let pmp_flag = (pmp_port as u16) << AHCI_CMD_LIST_FLAG_PMP_SHIFT;
    let mut count = 2;
    loop {
        if let Some(c) = ccb.take() {
            ahci_put_pmp_ccb(c);
        }

        if ahci_pmp_phy_status(ap, pmp_port).is_err() {
            printf(format_args!(
                "{}.{}: unable to clear PHY status\n",
                ap.portname(),
                pmp_port
            ));
        }
        ahci_pwrite(ap, AHCI_PREG_SERR, u32::MAX);
        // maybe don't do this on the first loop:
        ahci_pwrite(ap, AHCI_PREG_IS, AHCI_PREG_IS_IFS);
        let _ = ahci_pmp_write(ap, pmp_port, SATA_PMREG_SERR, u32::MAX);

        // send first softreset FIS
        let c = ahci_get_pmp_ccb(ap); // Always returns non-NULL.
        ccb = Some(c);
        c.zero_cmd_table();

        c.set_cfis(0, ATA_FIS_TYPE_H2D);
        c.set_cfis(1, pmp_port as u8);
        c.set_cfis(15, ATA_FIS_CONTROL_SRST | ATA_FIS_CONTROL_4BIT);

        c.set_hdr_prdtl(0);
        c.set_hdr_flags(
            5 // FIS length: 5 DWORDS
                | AHCI_CMD_LIST_FLAG_C
                | AHCI_CMD_LIST_FLAG_R
                | pmp_flag,
        );

        c.ccb_xa.set_state(ATA_S_PENDING);

        // DPRINTF(AHCI_D_VERBOSE, "%s.%d: sending PMP softreset cmd\n", ...);
        let cont = if ahci_poll(c, 1000, Some(ahci_pmp_probe_timeout)).is_err() {
            printf(format_args!(
                "{}.{}: PMP port softreset cmd failed\n",
                ap.portname(),
                pmp_port
            ));
            rc = Err(Errno::EBUSY);
            // probably delay a while to allow it to settle down?
            true
        } else {
            // send signature FIS
            c.zero_cmd_table();
            c.set_cfis(0, ATA_FIS_TYPE_H2D);
            c.set_cfis(1, pmp_port as u8);
            c.set_cfis(15, ATA_FIS_CONTROL_4BIT);

            c.set_hdr_prdtl(0);
            c.set_hdr_flags(5 /* FIS length: 5 DWORDS */ | pmp_flag);

            // DPRINTF(AHCI_D_VERBOSE, "%s.%d: sending PMP probe status cmd\n", ...);
            c.ccb_xa.set_state(ATA_S_PENDING);
            if ahci_poll(c, 5000, Some(ahci_pmp_probe_timeout)).is_err() {
                // DPRINTF(AHCI_D_VERBOSE, "%s.%d: PMP probe status cmd failed\n", ...);
                rc = Err(Errno::EBUSY);
                // sleep a while?
                true
            } else {
                c.set_cfis(15, 0);
                false
            }
        };
        if !cont {
            break;
        }

        // } while (count--);
        if count == 0 {
            break;
        }
        count -= 1;
    }

    if let Some(c) = ccb.take() {
        ahci_put_pmp_ccb(c);
    }

    // clean up a bit
    let _ = ahci_pmp_write(ap, pmp_port, SATA_PMREG_SERR, u32::MAX);
    ahci_pwrite(ap, AHCI_PREG_SERR, u32::MAX);
    ahci_pwrite(ap, AHCI_PREG_IS, AHCI_PREG_IS_IFS);
    ap.ap_pmp_ignore_ifs.set(0);
    // DPRINTF(AHCI_D_VERBOSE, "%s: no longer ignoring IFS\n", PORTNAME(ap));
    splx(s);

    rc
}

/// `ahci_pmp_port_probe`: the `ATA_PORT_T_*` of a port multiplier's port.
pub fn ahci_pmp_port_probe(ap: &'static AhciPort, pmp_port: i32) -> i32 {
    ap.ap_state.set(AP_S_PMP_PORT_PROBE);

    // DPRINTF(AHCI_D_VERBOSE, "%s.%d: probing pmp port\n", PORTNAME(ap), pmp_port);
    if ahci_pmp_port_portreset(ap, pmp_port).is_err() {
        printf(format_args!(
            "{}.{}: unable to probe PMP port; portreset failed\n",
            ap.portname(),
            pmp_port
        ));
        ap.ap_state.set(AP_S_NORMAL);
        return ATA_PORT_T_NONE;
    }

    if ahci_pmp_port_softreset(ap, pmp_port).is_err() {
        printf(format_args!(
            "{}.{}: unable to probe PMP port due to softreset failure\n",
            ap.portname(),
            pmp_port
        ));
        ap.ap_state.set(AP_S_NORMAL);
        return ATA_PORT_T_NONE;
    }

    let sig = ahci_port_signature(ap);
    // DPRINTF(AHCI_D_VERBOSE, "%s.%d: port signature returned %d\n", ...);
    ap.ap_state.set(AP_S_NORMAL);
    sig
}

/// `ahci_flush_tfd`: clears SERR.DIAG.X so the task file can update.
pub fn ahci_flush_tfd(ap: &AhciPort) {
    let r = ahci_pread(ap, AHCI_PREG_SERR);
    if r & AHCI_PREG_SERR_DIAG_X != 0 {
        ahci_pwrite(ap, AHCI_PREG_SERR, AHCI_PREG_SERR_DIAG_X);
    }
}

/// `ahci_active_mask`: the slots the chip still runs.
pub fn ahci_active_mask(ap: &AhciPort) -> u32 {
    let mut mask = ahci_pread(ap, AHCI_PREG_CI);
    if ap.sc().sc_cap.get() & AHCI_REG_CAP_SNCQ != 0 {
        mask |= ahci_pread(ap, AHCI_PREG_SACT);
    }
    mask
}

/// `ahci_pmp_probe_timeout`: `ahci_poll`'s timeout function for the PMP probe commands.
pub fn ahci_pmp_probe_timeout(cookie: *mut c_void) {
    // SAFETY: ahci_poll passes its ccb, one of a live port's.
    let ccb: &'static AhciCcb = unsafe { &*cookie.cast::<AhciCcb>().cast_const() };
    let ap = ccb.ccb_port;

    // DPRINTF(AHCI_D_VERBOSE, "%s: PMP probe cmd timed out\n", PORTNAME(ap));
    match ccb.ccb_xa.state() {
        ATA_S_PENDING => {
            // SAFETY: a pending ccb is on the pending list (ahci_start put it there).
            unsafe { ap.ap_ccb_pending.remove(ccb) };
            ccb.ccb_xa.set_state(ATA_S_TIMEOUT);
        }

        // ATA_S_ERROR: currently mostly here for the ATI SBx00 quirk
        ATA_S_ONCHIP | ATA_S_ERROR => {
            // clear the command on-chip
            kassert!(
                ap.ap_active.load(Ordering::Relaxed) == ahci_slot_bit(ccb)
                    && ap.ap_sactive.load(Ordering::Relaxed) == 0
            );
            let _ = ahci_port_stop(ap, false);
            let _ = ahci_port_start(ap, false);

            if ahci_active_mask(ap) != 0 {
                let _ = ahci_port_stop(ap, false);
                let _ = ahci_port_start(ap, false);
                let mask = ahci_active_mask(ap);
                if mask != 0 {
                    printf(format_args!(
                        "{}: ahci_pmp_probe_timeout: failed to clear active cmds: {:08x}\n",
                        ap.portname(),
                        mask
                    ));
                }
            }

            ccb.ccb_xa.set_state(ATA_S_TIMEOUT);
            ap.ap_active
                .fetch_and(!ahci_slot_bit(ccb), Ordering::Relaxed);
            kassert!(ap.ap_active_cnt.load(Ordering::Relaxed) > 0);
            ap.ap_active_cnt.fetch_sub(1, Ordering::Relaxed);
            // DPRINTF(AHCI_D_VERBOSE, "%s: timed out %d, active %x, active_cnt %d\n", ...);
        }

        s => panic(format_args!(
            "{}: ahci_pmp_probe_timeout: ccb in bad state {}",
            ap.portname(),
            s
        )),
    }
}

/// `ahci_port_signature`: the `ATA_PORT_T_*` the port's signature register names.
pub fn ahci_port_signature(ap: &AhciPort) -> i32 {
    ahci_signature_type(ahci_pread(ap, AHCI_PREG_SIG))
}

/// The `ATA_PORT_T_*` of a device signature.
pub fn ahci_signature_type(sig: u32) -> i32 {
    if (sig & 0xffff0000) == (SATA_SIGNATURE_ATAPI & 0xffff0000) {
        ATA_PORT_T_ATAPI
    } else if (sig & 0xffff0000) == (SATA_SIGNATURE_PORT_MULTIPLIER & 0xffff0000) {
        ATA_PORT_T_PM
    } else {
        ATA_PORT_T_DISK
    }
}

/// The SControl value of a COMRESET: `AHCI_PREG_SCTL_SPD_GEN1` when the controller's config
/// flags force GEN1.
fn ahci_sctl_comreset(ap: &AhciPort) -> u32 {
    let mut r = AHCI_PREG_SCTL_IPM_DISABLED | AHCI_PREG_SCTL_DET_INIT;
    if ap.sc().sc_dev.cfdata().cf_flags & 0x01 != 0 {
        // DPRINTF(AHCI_D_VERBOSE, "%s: forcing GEN1\n", PORTNAME(ap));
        r |= AHCI_PREG_SCTL_SPD_GEN1;
    } else {
        r |= AHCI_PREG_SCTL_SPD_ANY;
    }
    r
}

/// `ahci_pmp_port_portreset`: COMRESET of a port multiplier's port.
pub fn ahci_pmp_port_portreset(ap: &'static AhciPort, pmp_port: i32) -> Result<(), Errno> {
    let mut rc = Err(Errno::EIO);

    let s = splbio();
    // DPRINTF(AHCI_D_VERBOSE, "%s.%d: PMP port reset\n", PORTNAME(ap), pmp_port);

    // Save previous command register state
    let cmd = ahci_pread(ap, AHCI_PREG_CMD) & !AHCI_PREG_CMD_ICC;

    'err: {
        // turn off power management and disable the PHY
        let data = AHCI_PREG_SCTL_IPM_DISABLED;
        // maybe add AHCI_PREG_SCTL_DET_DISABLE
        if ahci_pmp_write(ap, pmp_port, SATA_PMREG_SERR, u32::MAX).is_err() {
            break 'err;
        }
        if ahci_pmp_write(ap, pmp_port, SATA_PMREG_SCTL, data).is_err() {
            break 'err;
        }
        delay(10000);

        // start COMRESET
        let data = ahci_sctl_comreset(ap);

        if ahci_pmp_write(ap, pmp_port, SATA_PMREG_SCTL, data).is_err() {
            break 'err;
        }

        // give it a while to settle down
        delay(100000);

        if ahci_pmp_phy_status(ap, pmp_port).is_err() {
            printf(format_args!(
                "{}.{}: cannot clear PHY status\n",
                ap.portname(),
                pmp_port
            ));
        }

        // start trying to negotiate
        let _ = ahci_pmp_write(ap, pmp_port, SATA_PMREG_SERR, u32::MAX);
        let data = AHCI_PREG_SCTL_IPM_DISABLED | AHCI_PREG_SCTL_DET_NONE;
        if ahci_pmp_write(ap, pmp_port, SATA_PMREG_SCTL, data).is_err() {
            break 'err;
        }

        // give it a while to detect
        let mut detected = false;
        for _ in 0..3 {
            let Ok(data) = ahci_pmp_read(ap, pmp_port, SATA_PMREG_SSTS) else {
                break 'err;
            };
            if data & AHCI_PREG_SSTS_DET != 0 {
                detected = true;
                break;
            }
            delay(100000);
        }
        if !detected {
            printf(format_args!(
                "{}.{}: port is unplugged\n",
                ap.portname(),
                pmp_port
            ));
            break 'err;
        }

        // give it even longer to fully negotiate
        let mut negotiated = false;
        for _ in 0..30 {
            let Ok(data) = ahci_pmp_read(ap, pmp_port, SATA_PMREG_SSTS) else {
                break 'err;
            };
            if (data & AHCI_PREG_SSTS_DET) == AHCI_PREG_SSTS_DET_DEV {
                negotiated = true;
                break;
            }
            delay(100000);
        }

        if !negotiated {
            printf(format_args!(
                "{}.{}: device is not negotiating\n",
                ap.portname(),
                pmp_port
            ));
            break 'err;
        }

        // device detected
        // DPRINTF(AHCI_D_VERBOSE, "%s.%d: device detected\n", PORTNAME(ap), pmp_port);

        // clean up a bit
        delay(100000);
        let _ = ahci_pmp_write(ap, pmp_port, SATA_PMREG_SERR, u32::MAX);
        ahci_pwrite(ap, AHCI_PREG_SERR, u32::MAX);
        ahci_pwrite(ap, AHCI_PREG_IS, AHCI_PREG_IS_IFS);

        rc = Ok(());
    }

    // err:
    // Restore preserved port state
    ahci_pwrite(ap, AHCI_PREG_CMD, cmd);
    splx(s);
    rc
}

/// `ahci_port_comreset`: AHCI port reset, Section 10.4.2.
pub fn ahci_port_comreset(ap: &AhciPort) {
    let mut r = ahci_sctl_comreset(ap);
    ahci_pwrite(ap, AHCI_PREG_SCTL, r);
    delay(10000); // wait at least 1ms for COMRESET to be sent
    r &= !AHCI_PREG_SCTL_DET_INIT;
    r |= AHCI_PREG_SCTL_DET_NONE;
    ahci_pwrite(ap, AHCI_PREG_SCTL, r);
    delay(10000);
}

/// `ahci_port_portreset_start`: stops the port and sends a COMRESET.
pub fn ahci_port_portreset_start(ap: &AhciPort) {
    let s = splbio();
    // DPRINTF(AHCI_D_VERBOSE, "%s: port reset\n", PORTNAME(ap));

    // Save previous command register state
    ap.ap_saved_cmd
        .set(ahci_pread(ap, AHCI_PREG_CMD) & !AHCI_PREG_CMD_ICC);

    // Clear ST, ignoring failure
    let _ = ahci_port_stop(ap, false);

    // Perform device detection
    ahci_pwrite(ap, AHCI_PREG_SCTL, 0);
    delay(10000);
    ahci_port_comreset(ap);
    splx(s);
}

/// `ahci_port_portreset_poll`: `EAGAIN` until a device is present and communicating.
pub fn ahci_port_portreset_poll(ap: &AhciPort) -> Result<(), Errno> {
    if (ahci_pread(ap, AHCI_PREG_SSTS) & AHCI_PREG_SSTS_DET) != AHCI_PREG_SSTS_DET_DEV {
        return Err(Errno::EAGAIN);
    }
    Ok(())
}

/// `ahci_port_portreset_wait`: polls for up to a second.
pub fn ahci_port_portreset_wait(ap: &AhciPort) {
    for _ in 0..1000 {
        if ahci_port_portreset_poll(ap).is_ok() {
            break;
        }
        delay(1000);
    }
}

/// `ahci_port_portreset_finish`: waits for the device to become ready and, when `pmp`,
/// looks for a port multiplier.
pub fn ahci_port_portreset_finish(ap: &'static AhciPort, pmp: bool) -> Result<(), Errno> {
    let mut pmp = pmp;
    let mut retries = 0;
    let rc;

    let s = splbio();
    // retry:
    'retry: loop {
        let rc_now;
        if ahci_port_portreset_poll(ap).is_err() {
            rc_now = Err(Errno::ENODEV);
            if ahci_pread(ap, AHCI_PREG_SSTS) & AHCI_PREG_SSTS_DET != 0 {
                // this may be a port multiplier with no device on port 0, so still do the
                // pmp check if requested.
            } else {
                rc = rc_now;
                break 'retry;
            }
        } else {
            // Clear SERR (incl X bit), so TFD can update
            ahci_pwrite(ap, AHCI_PREG_SERR, ahci_pread(ap, AHCI_PREG_SERR));

            // Wait for device to become ready
            if ahci_pwait_clr(
                ap,
                AHCI_PREG_TFD,
                AHCI_PREG_TFD_STS_BSY | AHCI_PREG_TFD_STS_DRQ | AHCI_PREG_TFD_STS_ERR,
                3,
            )
            .is_err()
            {
                // even if the device doesn't wake up, check if there's a port multiplier
                // there
                if retries == 0 {
                    retries = 1;
                    ahci_port_comreset(ap);
                    ahci_port_portreset_wait(ap);
                    continue 'retry;
                }
                rc_now = Err(Errno::EBUSY);
            } else {
                rc_now = Ok(());
            }
        }

        if pmp && ahci_port_detect_pmp(ap).is_err() {
            // reset again without pmp support
            pmp = false;
            retries = 0;
            ahci_port_comreset(ap);
            ahci_port_portreset_wait(ap);
            continue 'retry;
        }

        rc = rc_now;
        break;
    }

    // err:
    // Restore preserved port state
    ahci_pwrite(ap, AHCI_PREG_CMD, ap.ap_saved_cmd.get());
    ap.ap_saved_cmd.set(0);
    splx(s);

    rc
}

/// `ahci_port_portreset`.
pub fn ahci_port_portreset(ap: &'static AhciPort, pmp: bool) -> Result<(), Errno> {
    ahci_port_portreset_start(ap);
    ahci_port_portreset_wait(ap);
    ahci_port_portreset_finish(ap, pmp)
}

/// `ahci_port_detect_pmp`: looks for a port multiplier (and counts its ports); without one
/// the port is reset to work without PMA.
pub fn ahci_port_detect_pmp(ap: &'static AhciPort) -> Result<(), Errno> {
    let sc = ap.sc();
    let mut ccb: Option<&'static AhciCcb> = None;

    if sc.sc_flags.get() & AHCI_F_NO_PMP != 0 || ahci_read(sc, AHCI_REG_CAP) & AHCI_REG_CAP_SPM == 0
    {
        return Ok(());
    }

    let mut rc = Ok(());
    let mut pmp_rc = Ok(());
    let mut count = 2;
    loop {
        // DPRINTF(AHCI_D_VERBOSE, "%s: PMP probe %d\n", PORTNAME(ap), count);
        if let Some(c) = ccb.take() {
            ahci_put_pmp_ccb(c);
        }
        let _ = ahci_port_stop(ap, false);
        ap.ap_state.set(AP_S_PMP_PROBE);

        // set PMA in cmd reg
        let mut cmd = ahci_pread(ap, AHCI_PREG_CMD) & !AHCI_PREG_CMD_ICC;
        if (cmd & AHCI_PREG_CMD_PMA) == 0 {
            cmd |= AHCI_PREG_CMD_PMA;
            ahci_pwrite(ap, AHCI_PREG_CMD, cmd);
        }

        // Flush errors and request CLO unconditionally, then start the port
        let r = ahci_pread(ap, AHCI_PREG_SERR);
        if r & AHCI_PREG_SERR_DIAG_X != 0 {
            ahci_pwrite(ap, AHCI_PREG_SERR, AHCI_PREG_SERR_DIAG_X);
        }

        // Request CLO
        let _ = ahci_port_clo(ap);

        // Clear port errors to permit TFD transfer
        let r = ahci_pread(ap, AHCI_PREG_SERR);
        ahci_pwrite(ap, AHCI_PREG_SERR, r);

        // Restart port
        if ahci_port_start(ap, false).is_err() {
            rc = Err(Errno::EBUSY);
            printf(format_args!(
                "{}: failed to start port, cannot probe PMP\n",
                ap.portname()
            ));
            break;
        }

        // Check whether CLO worked
        if ahci_pwait_clr(
            ap,
            AHCI_PREG_TFD,
            AHCI_PREG_TFD_STS_BSY | AHCI_PREG_TFD_STS_DRQ,
            1,
        )
        .is_err()
        {
            let cap = ahci_read(sc, AHCI_REG_CAP);
            printf(format_args!(
                "{}: CLO {}, need port reset\n",
                ap.portname(),
                if cap & AHCI_REG_CAP_SCLO != 0 {
                    "failed"
                } else {
                    "unsupported"
                }
            ));
            pmp_rc = Err(Errno::EBUSY);
            break;
        }

        // Prep first command with SRST feature & clear busy/reset flags
        let c = ahci_get_pmp_ccb(ap); // Always returns non-NULL.
        ccb = Some(c);
        c.zero_cmd_table();

        c.set_cfis(0, ATA_FIS_TYPE_H2D);
        c.set_cfis(1, SATA_PMP_CONTROL_PORT as u8);
        c.set_cfis(15, ATA_FIS_CONTROL_SRST | ATA_FIS_CONTROL_4BIT);

        c.set_hdr_prdtl(0);
        c.set_hdr_flags(
            5 // FIS length: 5 DWORDS
                | AHCI_CMD_LIST_FLAG_C
                | AHCI_CMD_LIST_FLAG_R
                | AHCI_CMD_LIST_FLAG_PMP,
        );

        // DPRINTF(AHCI_D_VERBOSE, "%s: sending PMP reset cmd\n", PORTNAME(ap));
        c.ccb_xa.set_state(ATA_S_PENDING);
        let ok = if ahci_poll(c, 1000, Some(ahci_pmp_probe_timeout)).is_err() {
            // DPRINTF(AHCI_D_VERBOSE, "%s: PMP reset cmd failed\n", PORTNAME(ap));
            pmp_rc = Err(Errno::EBUSY);
            false
        } else {
            if ahci_pwait_clr(
                ap,
                AHCI_PREG_TFD,
                AHCI_PREG_TFD_STS_BSY | AHCI_PREG_TFD_STS_DRQ,
                1,
            )
            .is_err()
            {
                printf(format_args!(
                    "{}: port busy after first PMP probe FIS\n",
                    ap.portname()
                ));
            }

            // clear errors in case the device didn't reset cleanly
            ahci_flush_tfd(ap);
            let r = ahci_pread(ap, AHCI_PREG_SERR);
            ahci_pwrite(ap, AHCI_PREG_SERR, r);

            // Prep second command to read status and complete reset sequence
            c.zero_cmd_table();
            c.set_cfis(0, ATA_FIS_TYPE_H2D);
            c.set_cfis(1, SATA_PMP_CONTROL_PORT as u8);
            c.set_cfis(15, ATA_FIS_CONTROL_4BIT);

            c.set_hdr_prdtl(0);
            c.set_hdr_flags(5 /* FIS length: 5 DWORDS */ | AHCI_CMD_LIST_FLAG_PMP);

            // DPRINTF(AHCI_D_VERBOSE, "%s: sending PMP probe status cmd\n", PORTNAME(ap));
            c.ccb_xa.set_state(ATA_S_PENDING);
            if ahci_poll(c, 5000, Some(ahci_pmp_probe_timeout)).is_err() {
                // DPRINTF(AHCI_D_VERBOSE, "%s: PMP probe status cmd failed\n", ...);
                pmp_rc = Err(Errno::EBUSY);
                false
            } else {
                true
            }
        };

        if ok {
            // apparently we need to retry at least once to get the right signature
            c.set_cfis(15, 0);
            pmp_rc = Ok(());
        }

        // } while (--count);
        count -= 1;
        if count == 0 {
            break;
        }
    }

    if let Some(c) = ccb.take() {
        ahci_put_pmp_ccb(c);
    }

    if ap.ap_state.get() == AP_S_PMP_PROBE {
        ap.ap_state.set(AP_S_NORMAL);
    }

    if pmp_rc.is_ok() {
        if ahci_port_signature(ap) != ATA_PORT_T_PM {
            // DPRINTF(AHCI_D_VERBOSE, "%s: device is not a PMP\n", PORTNAME(ap));
            pmp_rc = Err(Errno::EBUSY);
        } else {
            // DPRINTF(AHCI_D_VERBOSE, "%s: PMP found\n", PORTNAME(ap));
        }
    }

    if pmp_rc.is_ok() {
        match ahci_pmp_identify(ap) {
            Ok(nports) => {
                ap.ap_pmp_ports.set(nports);
                rc = Ok(());
            }
            Err(_) => pmp_rc = Err(Errno::EBUSY),
        }
    }

    // if PMP detection failed, so turn off the PMA bit and reset the port again
    if pmp_rc.is_err() {
        // DPRINTF(AHCI_D_VERBOSE, "%s: no PMP found, resetting the port\n", PORTNAME(ap));
        let _ = ahci_port_stop(ap, false);
        let _ = ahci_port_clo(ap);
        let mut cmd = ahci_pread(ap, AHCI_PREG_CMD) & !AHCI_PREG_CMD_ICC;
        cmd &= !AHCI_PREG_CMD_PMA;
        ahci_pwrite(ap, AHCI_PREG_CMD, cmd);

        ahci_pwrite(ap, AHCI_PREG_IE, 0);
        let _ = ahci_port_stop(ap, false);
        if sc.sc_cap.get() & AHCI_REG_CAP_SSNTF != 0 {
            ahci_pwrite(ap, AHCI_PREG_SNTF, u32::MAX);
        }
        ahci_flush_tfd(ap);
        ahci_pwrite(ap, AHCI_PREG_SERR, u32::MAX);

        ahci_pwrite(ap, AHCI_PREG_IS, u32::MAX);

        ahci_enable_interrupts(ap);

        rc = pmp_rc;
    }

    rc
}

/// `ahci_load_prdt_seg`: one PRDT entry for `len` bytes at `addr`.
pub fn ahci_load_prdt_seg(addr: u64, len: u32, flags: u32) -> AhciPrdt {
    let flags = flags | len.wrapping_sub(1);

    AhciPrdt {
        dba: addr.to_le(),
        reserved: 0,
        flags: flags.to_le(),
    }
}

/// `ahci_load_prdt`: loads the command's data into its DMA map and PRDT.
pub fn ahci_load_prdt(ccb: &AhciCcb) -> Result<(), Errno> {
    let ap = ccb.ccb_port;
    let sc = ap.sc();
    let xa = &ccb.ccb_xa;
    let dmap = ccb.ccb_dmamap;
    let dmat = sc.dmat();

    if xa.datalen.get() == 0 {
        ccb.set_hdr_prdtl(0);
        return Ok(());
    }

    // SAFETY: the issuer (atascsi, or the NCQ error path with the port's scratch buffer)
    // made `data` valid for `datalen` bytes and reserved for this command until it completes,
    // which unloads the map (ahci_unload_prdt).
    let loaded = unsafe {
        bus_dmamap_load(
            dmat,
            dmap,
            xa.data.get(),
            xa.datalen.get(),
            None,
            if xa.flags.get() & ATA_F_NOWAIT != 0 {
                BUS_DMA_NOWAIT
            } else {
                BUS_DMA_WAITOK
            },
        )
    };
    if let Err(error) = loaded {
        printf(format_args!(
            "{}: error {} loading dmamap\n",
            ap.portname(),
            error as i32
        ));
        return Err(Errno::EIO);
    }

    let nsegs = dmap.dm_nsegs.get().max(1) as usize;
    let segs = dmap.dm_segs();
    for (i, seg) in segs.iter().enumerate().take(nsegs - 1) {
        let seg = seg.get();
        ccb.set_prdt(
            i,
            ahci_load_prdt_seg(seg.ds_addr as u64, seg.ds_len as u32, 0),
        );
    }

    let i = nsegs - 1;
    let seg = segs[i].get();
    ccb.set_prdt(
        i,
        ahci_load_prdt_seg(
            seg.ds_addr as u64,
            seg.ds_len as u32,
            if xa.flags.get() & ATA_F_PIO != 0 {
                AHCI_PRDT_FLAG_INTR
            } else {
                0
            },
        ),
    );

    ccb.set_hdr_prdtl(dmap.dm_nsegs.get() as u16);

    bus_dmamap_sync(
        dmat,
        dmap,
        0,
        dmap.dm_mapsize.get(),
        if xa.flags.get() & ATA_F_READ != 0 {
            BUS_DMASYNC_PREREAD
        } else {
            BUS_DMASYNC_PREWRITE
        },
    );

    Ok(())
}

/// `ahci_unload_prdt`: unloads the command's data and computes its residual.
pub fn ahci_unload_prdt(ccb: &AhciCcb) {
    let ap = ccb.ccb_port;
    let sc = ap.sc();
    let xa = &ccb.ccb_xa;
    let dmap = ccb.ccb_dmamap;
    let dmat = sc.dmat();

    if xa.datalen.get() != 0 {
        bus_dmamap_sync(
            dmat,
            dmap,
            0,
            dmap.dm_mapsize.get(),
            if xa.flags.get() & ATA_F_READ != 0 {
                BUS_DMASYNC_POSTREAD
            } else {
                BUS_DMASYNC_POSTWRITE
            },
        );

        bus_dmamap_unload(dmat, dmap);

        if xa.flags.get() & ATA_F_NCQ != 0 {
            xa.resid.set(0);
        } else {
            xa.resid
                .set(xa.datalen.get().wrapping_sub(ccb.hdr_prdbc() as usize));
        }
    }
}

/// `ahci_poll`: starts a command and polls the port until it completes, errors (running
/// `timeout_fn`) or `timeout` milliseconds pass (running `timeout_fn` at `splbio`).
pub fn ahci_poll(
    ccb: &'static AhciCcb,
    timeout: i32,
    timeout_fn: Option<TimeoutFn>,
) -> Result<(), Errno> {
    let ap = ccb.ccb_port;
    let mut timeout = timeout;

    let s = splbio();
    ahci_start(ccb);
    loop {
        if ahci_port_intr(ap, AHCI_PREG_CI_ALL_SLOTS) & ahci_slot_bit(ccb) != 0 {
            splx(s);
            return Ok(());
        }
        if ccb.ccb_xa.state() == ATA_S_ERROR {
            // DPRINTF(AHCI_D_VERBOSE, "%s: ccb in slot %d errored\n", ...);
            // pretend it timed out?
            if let Some(f) = timeout_fn {
                f(ahci_ccb_cookie(ccb));
            }
            splx(s);
            return Err(Errno::EIO);
        }

        delay(1000);
        timeout -= 1;
        if timeout <= 0 {
            break;
        }
    }

    // Run timeout while at splbio, otherwise ahci_intr could interfere.
    if let Some(f) = timeout_fn {
        f(ahci_ccb_cookie(ccb));
    }

    splx(s);

    Err(Errno::EIO)
}

/// `ahci_start`: issues a command, or queues it behind the commands it cannot run beside.
pub fn ahci_start(ccb: &'static AhciCcb) {
    let ap = ccb.ccb_port;
    let sc = ap.sc();
    let dmat = sc.dmat();
    let slot = ccb.ccb_slot as usize;
    let bit = ahci_slot_bit(ccb);

    // Zero transferred byte count before transfer
    ccb.set_hdr_prdbc(0);

    // Sync command list entry and corresponding command table entry
    if let Some(cmd_list) = ap.ap_dmamem_cmd_list.get() {
        bus_dmamap_sync(
            dmat,
            ahci_dma_map(cmd_list),
            slot * size_of::<AhciCmdHdr>(),
            size_of::<AhciCmdHdr>(),
            BUS_DMASYNC_PREWRITE,
        );
    }
    if let Some(cmd_table) = ap.ap_dmamem_cmd_table.get() {
        bus_dmamap_sync(
            dmat,
            ahci_dma_map(cmd_table),
            slot * size_of::<AhciCmdTable>(),
            size_of::<AhciCmdTable>(),
            BUS_DMASYNC_PREWRITE,
        );
    }

    // Prepare RFIS area for write by controller
    if let Some(rfis) = ap.ap_dmamem_rfis.get() {
        bus_dmamap_sync(
            dmat,
            ahci_dma_map(rfis),
            0,
            size_of::<AhciRfis>(),
            BUS_DMASYNC_PREREAD,
        );
    }

    let active = ap.ap_active.load(Ordering::Relaxed);
    let sactive = ap.ap_sactive.load(Ordering::Relaxed);
    let active_cnt = ap.ap_active_cnt.load(Ordering::Relaxed);
    let pmp_port = ccb.ccb_xa.pmp_port.get() as u32;
    if ccb.ccb_xa.flags.get() & ATA_F_NCQ != 0 {
        // Issue NCQ commands only when there are no outstanding standard commands.
        if active != 0
            || !ap.ap_ccb_pending.is_empty()
            || (sactive != 0 && ap.ap_pmp_ncq_port.load(Ordering::Relaxed) != pmp_port)
        {
            // SAFETY: the ccb is on no list (it was taken off the free list and is not
            // pending), and lives as long as the port.
            unsafe { ap.ap_ccb_pending.insert_tail(ccb) };
        } else {
            kassert!(active_cnt == 0);
            ap.ap_sactive.store(sactive | bit, Ordering::Relaxed);
            ccb.ccb_xa.set_state(ATA_S_ONCHIP);
            ahci_pwrite(ap, AHCI_PREG_SACT, bit);
            ahci_pwrite(ap, AHCI_PREG_CI, bit);
            ap.ap_pmp_ncq_port.store(pmp_port, Ordering::Relaxed);
        }
    } else {
        // Wait for all NCQ commands to finish before issuing standard command.
        if sactive != 0 || active_cnt == 2 {
            // SAFETY: as above.
            unsafe { ap.ap_ccb_pending.insert_tail(ccb) };
        } else if active_cnt < 2 {
            ap.ap_active.store(active | bit, Ordering::Relaxed);
            ccb.ccb_xa.set_state(ATA_S_ONCHIP);
            ahci_pwrite(ap, AHCI_PREG_CI, bit);
            ap.ap_active_cnt.store(active_cnt + 1, Ordering::Relaxed);
        }
    }
}

/// `ahci_issue_pending_ncq_commands`: starts the NCQ commands at the head of the pending
/// list (of one port multiplier port).
pub fn ahci_issue_pending_ncq_commands(ap: &AhciPort) {
    let mut sact_change: u32 = 0;

    kassert!(ap.ap_active_cnt.load(Ordering::Relaxed) == 0);

    let Some(mut nextccb) = ap.ap_ccb_pending.first() else {
        return;
    };
    if nextccb.ccb_xa.flags.get() & ATA_F_NCQ == 0 {
        return;
    }

    // Start all the NCQ commands at the head of the pending list. If a port multiplier is
    // attached to the port, we can only issue commands for one of its ports at a time.
    if ap.ap_sactive.load(Ordering::Relaxed) != 0
        && ap.ap_pmp_ncq_port.load(Ordering::Relaxed) != nextccb.ccb_xa.pmp_port.get() as u32
    {
        return;
    }

    ap.ap_pmp_ncq_port
        .store(nextccb.ccb_xa.pmp_port.get() as u32, Ordering::Relaxed);
    loop {
        // SAFETY: `nextccb` is the head of the pending list.
        unsafe { ap.ap_ccb_pending.remove(nextccb) };
        sact_change |= ahci_slot_bit(nextccb);
        nextccb.ccb_xa.set_state(ATA_S_ONCHIP);
        match ap.ap_ccb_pending.first() {
            Some(n)
                if n.ccb_xa.flags.get() & ATA_F_NCQ != 0
                    && n.ccb_xa.pmp_port.get() as u32
                        == ap.ap_pmp_ncq_port.load(Ordering::Relaxed) =>
            {
                nextccb = n;
            }
            _ => break,
        }
    }

    ap.ap_sactive.fetch_or(sact_change, Ordering::Relaxed);
    ahci_pwrite(ap, AHCI_PREG_SACT, sact_change);
    ahci_pwrite(ap, AHCI_PREG_CI, sact_change);
}

/// `ahci_issue_pending_commands`: after a command finished, starts what it held back.
pub fn ahci_issue_pending_commands(ap: &AhciPort, last_was_ncq: bool) {
    let nextccb = ap.ap_ccb_pending.first();
    match nextccb {
        Some(n) if n.ccb_xa.flags.get() & ATA_F_NCQ != 0 => {
            if last_was_ncq {
                kassert!(
                    n.ccb_xa.pmp_port.get() as u32 != ap.ap_pmp_ncq_port.load(Ordering::Relaxed)
                );
                // otherwise it should have been started already
            } else {
                ap.ap_active_cnt.fetch_sub(1, Ordering::Relaxed);
            }

            // Issue NCQ commands only when there are no outstanding standard commands, and
            // previous NCQ commands for other PMP ports have finished.
            if ap.ap_active.load(Ordering::Relaxed) == 0 {
                ahci_issue_pending_ncq_commands(ap);
            } else {
                kassert!(ap.ap_active_cnt.load(Ordering::Relaxed) == 1);
            }
        }
        Some(mut n) => {
            if ap.ap_sactive.load(Ordering::Relaxed) != 0 || last_was_ncq {
                kassert!(ap.ap_active_cnt.load(Ordering::Relaxed) == 0);
            }

            // Wait for all NCQ commands to finish before issuing standard command.
            if ap.ap_sactive.load(Ordering::Relaxed) != 0 {
                return;
            }

            // Keep up to 2 standard commands on-chip at a time.
            loop {
                // SAFETY: `n` is the head of the pending list.
                unsafe { ap.ap_ccb_pending.remove(n) };
                ap.ap_active.fetch_or(ahci_slot_bit(n), Ordering::Relaxed);
                n.ccb_xa.set_state(ATA_S_ONCHIP);
                ahci_pwrite(ap, AHCI_PREG_CI, ahci_slot_bit(n));
                if last_was_ncq {
                    ap.ap_active_cnt.fetch_add(1, Ordering::Relaxed);
                }
                if ap.ap_active_cnt.load(Ordering::Relaxed) == 2 {
                    break;
                }
                kassert!(ap.ap_active_cnt.load(Ordering::Relaxed) == 1);
                match ap.ap_ccb_pending.first() {
                    Some(next) if next.ccb_xa.flags.get() & ATA_F_NCQ == 0 => n = next,
                    _ => break,
                }
            }
        }
        None if !last_was_ncq => {
            kassert!(matches!(ap.ap_active_cnt.load(Ordering::Relaxed), 1 | 2));

            // Standard command finished, none waiting to start.
            ap.ap_active_cnt.fetch_sub(1, Ordering::Relaxed);
        }
        None => {
            kassert!(ap.ap_active_cnt.load(Ordering::Relaxed) == 0);

            // NCQ command finished.
        }
    }
}

/// `ahci_intr`: the controller's interrupt handler.
pub fn ahci_intr(arg: *mut c_void) -> i32 {
    // SAFETY: the front-end establishes the interrupt with its softc, which begins with the
    // `struct ahci_softc` and lives while the interrupt is established.
    let sc: &AhciSoftc = unsafe { &*arg.cast::<AhciSoftc>().cast_const() };

    // Read global interrupt status
    let mut is = ahci_read(sc, AHCI_REG_IS);
    if is == 0 || is == 0xffffffff {
        return 0;
    }
    let ack = is;

    // AHCI_COALESCE (not defined): check coalescing interrupt first.

    // Process interrupts for each port
    while is != 0 {
        let port = is.trailing_zeros() as usize;
        if let Some(ap) = sc.port(port) {
            ahci_port_intr(ap, AHCI_PREG_CI_ALL_SLOTS);
        }
        is &= !(1 << port);
    }

    // Finally, acknowledge global interrupt
    ahci_write(sc, AHCI_REG_IS, ack);

    1
}

/// How `ahci_port_intr`'s error handling ends.
#[derive(Clone, Copy, PartialEq, Eq)]
enum AhciIntrFail {
    /// `goto failall`: fail every active command.
    All,
    /// `goto fatal`: give the port up and fail every active command.
    Fatal,
}

/// `ahci_port_intr`: completes the port's finished commands (of the slots in `ci_mask`) and
/// recovers from errors; returns the slots it completed.
pub fn ahci_port_intr(ap: &'static AhciPort, ci_mask: u32) -> u32 {
    let sc = ap.sc();
    let dmat = sc.dmat();
    let mut processed: u32 = 0;
    let mut need_restart = false;
    let mut process_error = false;

    let mut is = ahci_pread(ap, AHCI_PREG_IS);

    // Ack port interrupt only if checking all command slots.
    if ci_mask == AHCI_PREG_CI_ALL_SLOTS {
        ahci_pwrite(ap, AHCI_PREG_IS, is);
    }

    // if (is) DPRINTF(AHCI_D_INTR, "%s: interrupt: %b\n", PORTNAME(ap), is, AHCI_PFMT_IS);

    let mut ci_saved;
    let active: &AtomicU32 = if ap.ap_sactive.load(Ordering::Relaxed) != 0 {
        // Active NCQ commands - use SActive instead of CI
        kassert!(ap.ap_active.load(Ordering::Relaxed) == 0);
        kassert!(ap.ap_active_cnt.load(Ordering::Relaxed) == 0);
        ci_saved = ahci_pread(ap, AHCI_PREG_SACT);
        &ap.ap_sactive
    } else {
        // Save CI
        ci_saved = ahci_pread(ap, AHCI_PREG_CI);
        &ap.ap_active
    };

    if is & AHCI_PREG_IS_TFES != 0 {
        process_error = true;
    } else if is & AHCI_PREG_IS_DHRS != 0 {
        let tfd = ahci_pread(ap, AHCI_PREG_TFD);
        let cmd = ahci_pread(ap, AHCI_PREG_CMD);
        let _serr = ahci_pread(ap, AHCI_PREG_SERR);
        if (tfd & AHCI_PREG_TFD_STS_ERR) != 0 && (cmd & AHCI_PREG_CMD_CR) == 0 {
            // DPRINTF(AHCI_D_VERBOSE, "%s: DHRS error, TFD: %b, SERR: %b, DIAG: %b\n", ...);
            process_error = true;
        } else {
            // rfis copy back is in the normal execution path
            ahci_pwrite(ap, AHCI_PREG_IS, AHCI_PREG_IS_DHRS);
        }
    }

    let fail = 'fail: {
        // Command failed. See AHCI 1.1 spec 6.2.2.1 and 6.2.2.2.
        if process_error {
            let tfd = ahci_pread(ap, AHCI_PREG_TFD);
            let serr = ahci_pread(ap, AHCI_PREG_SERR);
            let mut ccb: Option<&'static AhciCcb> = None;

            let mut err_slot: i32 = if ap.ap_sactive.load(Ordering::Relaxed) == 0 {
                // Errored slot is easy to determine from CMD.
                let mut err_slot = ahci_preg_cmd_ccs(ahci_pread(ap, AHCI_PREG_CMD));

                if (ci_saved & (1 << err_slot)) == 0 {
                    // Hardware doesn't seem to report correct slot number. If there's only
                    // one outstanding command we can cope, otherwise fail all active
                    // commands.
                    if ap.ap_active_cnt.load(Ordering::Relaxed) == 1 {
                        err_slot = ap.ap_active.load(Ordering::Relaxed).trailing_zeros();
                    } else {
                        break 'fail Some(AhciIntrFail::All);
                    }
                }

                let c = ap.ccb(err_slot);
                ccb = Some(c);

                // Preserve received taskfile data from the RFIS.
                c.ccb_xa.rfis.set(ap.rfis());
                err_slot as i32
            } else {
                -1 // Must extract error from log page
            };

            // DPRINTF(AHCI_D_VERBOSE, "%s: errored slot %d, TFD: %b, SERR: %b, DIAG: %b\n",
            //     ...);

            // Turn off ST to clear CI and SACT.
            let _ = ahci_port_stop(ap, false);
            need_restart = true;

            // Clear SERR to enable capturing new errors.
            ahci_pwrite(ap, AHCI_PREG_SERR, serr);

            // Acknowledge the interrupts we can recover from.
            ahci_pwrite(ap, AHCI_PREG_IS, AHCI_PREG_IS_TFES | AHCI_PREG_IS_IFS);
            is = ahci_pread(ap, AHCI_PREG_IS);

            // If device hasn't cleared its busy status, try to idle it.
            if tfd & (AHCI_PREG_TFD_STS_BSY | AHCI_PREG_TFD_STS_DRQ) != 0 {
                let state = ap.ap_state.get();
                if state == AP_S_PMP_PORT_PROBE || state == AP_S_ERROR_RECOVERY {
                    // can't reset the port here, just make sure the operation fails and the
                    // port still works.
                } else if ap.ap_pmp_ports.get() != 0
                    && err_slot != -1
                    && let Some(c) = ccb
                {
                    let pmp_port = c.ccb_xa.pmp_port.get();
                    printf(format_args!(
                        "{}: error on PMP port {}, idling device\n",
                        ap.portname(),
                        pmp_port
                    ));
                    if ahci_pmp_port_softreset(ap, pmp_port).is_ok() {
                        printf(format_args!(
                            "{}: unable to softreset port {}\n",
                            ap.portname(),
                            pmp_port
                        ));
                        if ahci_pmp_port_portreset(ap, pmp_port).is_err() {
                            printf(format_args!(
                                "{}: failed to port  reset {}, giving up on it\n",
                                ap.portname(),
                                pmp_port
                            ));
                            break 'fail Some(AhciIntrFail::Fatal);
                        }
                    }
                } else {
                    printf(format_args!(
                        "{}: attempting to idle device\n",
                        ap.portname()
                    ));
                    if ahci_port_softreset(ap).is_err() {
                        printf(format_args!(
                            "{}: failed to soft reset device\n",
                            ap.portname()
                        ));
                        if ahci_port_portreset(ap, false).is_err() {
                            printf(format_args!(
                                "{}: failed to port reset device, give up on it\n",
                                ap.portname()
                            ));
                            break 'fail Some(AhciIntrFail::Fatal);
                        }
                    }
                }

                // Had to reset device, can't gather extended info.
            } else if ap.ap_sactive.load(Ordering::Relaxed) != 0 {
                // Recover the NCQ error from log page 10h. We can only have queued commands
                // active for one port at a time, so we know which device errored.
                let pmp_port = ap.ap_pmp_ncq_port.load(Ordering::Relaxed) as i32;
                err_slot = match ahci_port_read_ncq_error(ap, pmp_port) {
                    Ok(slot) => slot as i32,
                    Err(_) => -1,
                };
                if err_slot < 0 {
                    break 'fail Some(AhciIntrFail::All);
                }

                // DPRINTF(AHCI_D_VERBOSE, "%s: NCQ errored slot %d\n", ...);

                let c = ap.ccb(err_slot as u32);
                ccb = Some(c);
                if c.ccb_xa.state() != ATA_S_ONCHIP {
                    printf(format_args!(
                        "{}: NCQ errored slot {} is idle ({:08x} active)\n",
                        ap.portname(),
                        err_slot,
                        ci_saved
                    ));
                    break 'fail Some(AhciIntrFail::All);
                }
            } else {
                // Didn't reset, could gather extended info from log.
            }

            // If we couldn't determine the errored slot, reset the port and fail all the
            // active slots.
            let Some(c) = ccb.filter(|_| err_slot != -1) else {
                if ahci_port_softreset(ap).is_err() && ahci_port_portreset(ap, false).is_err() {
                    printf(format_args!(
                        "{}: couldn't reset after NCQ error, disabling device.\n",
                        ap.portname()
                    ));
                    break 'fail Some(AhciIntrFail::Fatal);
                }
                printf(format_args!(
                    "{}: couldn't recover NCQ error, failing all outstanding commands.\n",
                    ap.portname()
                ));
                break 'fail Some(AhciIntrFail::All);
            };

            // Clear the failed command in saved CI so completion runs.
            ci_saved &= !(1 << err_slot);

            // Note the error in the ata_xfer.
            kassert!(c.ccb_xa.state() == ATA_S_ONCHIP);
            c.ccb_xa.set_state(ATA_S_ERROR);

            // There may only be one outstanding standard command now.
            #[cfg(feature = "diagnostic")]
            if ap.ap_sactive.load(Ordering::Relaxed) == 0 {
                let mut tmp = ci_saved;
                if tmp != 0 {
                    let slot = tmp.trailing_zeros();
                    tmp &= !(1 << slot);
                    kassert!(tmp == 0);
                }
            }
        }

        // ATI SBx00 AHCI controllers respond to PMP probes with IPMS interrupts when
        // there's a normal SATA device attached.
        if ap.ap_state.get() == AP_S_PMP_PROBE
            && sc.sc_flags.get() & AHCI_F_IPMS_PROBE != 0
            && is & AHCI_PREG_IS_IPMS != 0
        {
            let slot = ahci_preg_cmd_ccs(ahci_pread(ap, AHCI_PREG_CMD));
            // DPRINTF(AHCI_D_INTR, "%s: slot %d received IPMS\n", PORTNAME(ap), slot);

            let c = ap.ccb(slot);
            c.ccb_xa.set_state(ATA_S_ERROR);

            ahci_pwrite(ap, AHCI_PREG_IS, AHCI_PREG_IS_IPMS);
            is &= !AHCI_PREG_IS_IPMS;
        }

        // ignore IFS errors while resetting a PMP port
        if is & AHCI_PREG_IS_IFS != 0
        /* && ap->ap_pmp_ignore_ifs */
        {
            // DPRINTF(AHCI_D_INTR, "%s: ignoring IFS while resetting PMP port\n", ...);

            need_restart = true;
            ahci_pwrite(ap, AHCI_PREG_SERR, u32::MAX);
            ahci_pwrite(ap, AHCI_PREG_IS, AHCI_PREG_IS_IFS);
            // is &= ~AHCI_PREG_IS_IFS; (the C's store, which nothing reads after failall)
            break 'fail Some(AhciIntrFail::All);
        }

        // Check for remaining errors - they are fatal.
        if is
            & (AHCI_PREG_IS_TFES
                | AHCI_PREG_IS_HBFS
                | AHCI_PREG_IS_IFS
                | AHCI_PREG_IS_OFS
                | AHCI_PREG_IS_UFS)
            != 0
        {
            printf(format_args!(
                "{}: unrecoverable errors (IS: {}), disabling port.\n",
                ap.portname(),
                Bitmask(u64::from(is), AHCI_PFMT_IS)
            ));

            // XXX try recovery first
            break 'fail Some(AhciIntrFail::Fatal);
        }

        // Fail all outstanding commands if we know the port won't recover.
        if ap.ap_state.get() == AP_S_FATAL_ERROR {
            break 'fail Some(AhciIntrFail::Fatal);
        }

        None
    };

    if let Some(fail) = fail {
        if fail == AhciIntrFail::Fatal {
            // fatal:
            ap.ap_state.set(AP_S_FATAL_ERROR);
        }
        // failall:

        // Ensure port is shut down.
        let _ = ahci_port_stop(ap, true);

        // Error all the active slots.
        let mut ci_masked = ci_saved & active.load(Ordering::Relaxed);
        while ci_masked != 0 {
            let slot = ci_masked.trailing_zeros();
            let c = ap.ccb(slot);
            ci_masked &= !(1 << slot);
            c.ccb_xa.set_state(ATA_S_ERROR);
        }

        // Run completion for all active slots.
        ci_saved &= !active.load(Ordering::Relaxed);

        // Don't restart the port if our problems were deemed fatal.
        if ap.ap_state.get() == AP_S_FATAL_ERROR {
            need_restart = false;
        }
    }

    // CCB completion is detected by noticing its slot's bit in CI has changed to zero some
    // time after we activated it. If we are polling, we may only be interested in particular
    // slot(s).
    let mut ci_masked = !ci_saved & active.load(Ordering::Relaxed) & ci_mask;
    while ci_masked != 0 {
        let slot = ci_masked.trailing_zeros();
        let ccb = ap.ccb(slot);
        ci_masked &= !(1 << slot);

        // DPRINTF(AHCI_D_INTR, "%s: slot %d is complete%s\n", ...);

        let off = ccb.ccb_slot as usize;
        if let Some(cmd_list) = ap.ap_dmamem_cmd_list.get() {
            bus_dmamap_sync(
                dmat,
                ahci_dma_map(cmd_list),
                off * size_of::<AhciCmdHdr>(),
                size_of::<AhciCmdHdr>(),
                BUS_DMASYNC_POSTWRITE,
            );
        }

        if let Some(cmd_table) = ap.ap_dmamem_cmd_table.get() {
            bus_dmamap_sync(
                dmat,
                ahci_dma_map(cmd_table),
                off * size_of::<AhciCmdTable>(),
                size_of::<AhciCmdTable>(),
                BUS_DMASYNC_POSTWRITE,
            );
        }

        if let Some(rfis) = ap.ap_dmamem_rfis.get() {
            bus_dmamap_sync(
                dmat,
                ahci_dma_map(rfis),
                0,
                size_of::<AhciRfis>(),
                BUS_DMASYNC_POSTREAD,
            );
        }

        active.fetch_and(!ahci_slot_bit(ccb), Ordering::Relaxed);
        // Copy the rfis into the ccb if we were asked for it
        if ccb.ccb_xa.state() == ATA_S_ONCHIP && ccb.ccb_xa.flags.get() & ATA_F_GET_RFIS != 0 {
            ccb.ccb_xa.rfis.set(ap.rfis());
        }

        processed |= ahci_slot_bit(ccb);

        (ccb.ccb_done.get())(ccb);
    }

    if need_restart {
        // Restart command DMA on the port
        let _ = ahci_port_start(ap, false);

        // Re-enable outstanding commands on port.
        if ci_saved != 0 {
            #[cfg(feature = "diagnostic")]
            {
                let mut tmp = ci_saved;
                while tmp != 0 {
                    let slot = tmp.trailing_zeros();
                    tmp &= !(1 << slot);
                    let c = ap.ccb(slot);
                    kassert!(c.ccb_xa.state() == ATA_S_ONCHIP);
                    kassert!(
                        (c.ccb_xa.flags.get() & ATA_F_NCQ != 0)
                            == (ap.ap_sactive.load(Ordering::Relaxed) != 0)
                    );
                }
            }
            // DPRINTF(AHCI_D_VERBOSE, "%s: ahci_port_intr re-enabling%s slots %08x\n", ...);

            if ap.ap_sactive.load(Ordering::Relaxed) != 0 {
                ahci_pwrite(ap, AHCI_PREG_SACT, ci_saved);
            }
            ahci_pwrite(ap, AHCI_PREG_CI, ci_saved);
        }
    }

    processed
}

/// `ahci_get_ccb`: a free ccb of the port.
pub fn ahci_get_ccb(ap: &AhciPort) -> Option<&'static AhciCcb> {
    mtx_enter(&ap.ap_ccb_mtx);
    let ccb = ap.ap_ccb_free.first().map(|c| {
        // SAFETY: the free list holds ccbs of `ap_ccbs`, which live until ahci_port_free.
        let c: &'static AhciCcb = unsafe { &*ptr::from_ref(c) };
        kassert!(c.ccb_xa.state() == ATA_S_PUT);
        // SAFETY: `c` is the head of the free list.
        unsafe { ap.ap_ccb_free.remove(c) };
        c.ccb_xa.set_state(ATA_S_SETUP);
        c
    });
    mtx_leave(&ap.ap_ccb_mtx);

    ccb
}

/// `ahci_put_ccb`: gives a ccb back to its port.
pub fn ahci_put_ccb(ccb: &'static AhciCcb) {
    let ap = ccb.ccb_port;

    #[cfg(feature = "diagnostic")]
    {
        let state = ccb.ccb_xa.state();
        if state != ATA_S_COMPLETE && state != ATA_S_TIMEOUT && state != ATA_S_ERROR {
            printf(format_args!(
                "{}: invalid ata_xfer state {:02x} in ahci_put_ccb, slot {}\n",
                ap.portname(),
                state,
                ccb.ccb_slot
            ));
        }
    }

    ccb.ccb_xa.set_state(ATA_S_PUT);
    mtx_enter(&ap.ap_ccb_mtx);
    // SAFETY: a ccb of the port that is on no list (it was handed out or is new).
    unsafe { ap.ap_ccb_free.insert_tail(ccb) };
    mtx_leave(&ap.ap_ccb_mtx);
}

/// Warns when SACT is not clear and asserts CI is, as the err and pmp ccb functions do.
fn ahci_check_idle(ap: &AhciPort, what: &str) {
    let sact = ahci_pread(ap, AHCI_PREG_SACT);
    if sact != 0 {
        printf(format_args!("{what} {sact:08x} != 0?\n"));
    }
    kassert!(ahci_pread(ap, AHCI_PREG_CI) == 0);
}

/// `ahci_get_err_ccb`: the ccb kept for error recovery, with the port's outstanding command
/// state set aside.
pub fn ahci_get_err_ccb(ap: &AhciPort) -> &'static AhciCcb {
    splassert(IPL_BIO, "ahci_get_err_ccb");

    // No commands may be active on the chip.
    ahci_check_idle(ap, "ahci_get_err_ccb but SACT");

    #[cfg(feature = "diagnostic")]
    {
        kassert!(ap.ap_err_busy.get() == 0);
        ap.ap_err_busy.set(1);
    }
    // Save outstanding command state.
    ap.ap_err_saved_active
        .set(ap.ap_active.load(Ordering::Relaxed));
    ap.ap_err_saved_active_cnt
        .set(ap.ap_active_cnt.load(Ordering::Relaxed));
    ap.ap_err_saved_sactive
        .set(ap.ap_sactive.load(Ordering::Relaxed));

    // Pretend we have no commands outstanding, so that completions won't run prematurely.
    ap.ap_active.store(0, Ordering::Relaxed);
    ap.ap_active_cnt.store(0, Ordering::Relaxed);
    ap.ap_sactive.store(0, Ordering::Relaxed);

    // Grab a CCB to use for error recovery. This should never fail, as we ask atascsi to
    // reserve one for us at init time.
    let Some(err_ccb) = ap.ap_ccb_err.get() else {
        panic(format_args!("{}: no error recovery ccb", ap.portname()));
    };
    err_ccb.ccb_xa.flags.set(0);
    err_ccb.ccb_xa.set_state(ATA_S_SETUP);
    err_ccb.ccb_done.set(ahci_empty_done);

    err_ccb
}

/// `ahci_put_err_ccb`: done with the error ccb; restores the outstanding command state.
pub fn ahci_put_err_ccb(ccb: &AhciCcb) {
    let ap = ccb.ccb_port;

    splassert(IPL_BIO, "ahci_put_err_ccb");

    #[cfg(feature = "diagnostic")]
    kassert!(ap.ap_err_busy.get() != 0);
    // No commands may be active on the chip
    ahci_check_idle(ap, "ahci_put_err_ccb but SACT");

    // Done with the CCB
    kassert!(ap.ap_ccb_err.get().is_some_and(|e| ptr::eq(e, ccb)));

    // Restore outstanding command state
    ap.ap_sactive
        .store(ap.ap_err_saved_sactive.get(), Ordering::Relaxed);
    ap.ap_active_cnt
        .store(ap.ap_err_saved_active_cnt.get(), Ordering::Relaxed);
    ap.ap_active
        .store(ap.ap_err_saved_active.get(), Ordering::Relaxed);

    #[cfg(feature = "diagnostic")]
    ap.ap_err_busy.set(0);
}

/// `ahci_get_pmp_ccb`: ccb 1, which some PMP commands need.
pub fn ahci_get_pmp_ccb(ap: &AhciPort) -> &'static AhciCcb {
    // some PMP commands need to be issued on slot 1, particularly the command that clears
    // SRST and fetches the device signature.
    //
    // ensure the chip is idle and ccb 1 is available.
    splassert(IPL_BIO, "ahci_get_pmp_ccb");

    let sact = ahci_pread(ap, AHCI_PREG_SACT);
    if sact != 0 {
        printf(format_args!("ahci_get_pmp_ccb; SACT {sact:08x} != 0\n"));
    }
    kassert!(ahci_pread(ap, AHCI_PREG_CI) == 0);

    let ccb = ap.ccb(1);
    kassert!(ccb.ccb_xa.state() == ATA_S_PUT);
    ccb.ccb_xa.flags.set(0);
    ccb.ccb_done.set(ahci_pmp_cmd_done);

    mtx_enter(&ap.ap_ccb_mtx);
    // SAFETY: ccb 1 is free (`ATA_S_PUT`), hence on the free list.
    unsafe { ap.ap_ccb_free.remove(ccb) };
    mtx_leave(&ap.ap_ccb_mtx);

    ccb
}

/// `ahci_put_pmp_ccb`.
pub fn ahci_put_pmp_ccb(ccb: &'static AhciCcb) {
    let ap = ccb.ccb_port;

    // make sure this is the right ccb
    kassert!(ccb.ccb_slot == 1);

    // No commands may be active on the chip
    ahci_check_idle(ap, "ahci_put_pmp_ccb but SACT");

    ccb.ccb_xa.set_state(ATA_S_PUT);
    mtx_enter(&ap.ap_ccb_mtx);
    // SAFETY: ccb 1, taken off the free list by ahci_get_pmp_ccb.
    unsafe { ap.ap_ccb_free.insert_tail(ccb) };
    mtx_leave(&ap.ap_ccb_mtx);
}

/// `ahci_port_read_ncq_error`: reads log page 10h for the errored NCQ command and returns
/// its slot, its registers copied into the slot's `rfis`.
pub fn ahci_port_read_ncq_error(ap: &'static AhciPort, pmp_port: i32) -> Result<u32, Errno> {
    let mut rc = Err(Errno::EIO);
    let mut err_slot = 0;

    // DPRINTF(AHCI_D_VERBOSE, "%s: read log page\n", PORTNAME(ap));
    let oldstate = ap.ap_state.get();
    ap.ap_state.set(AP_S_ERROR_RECOVERY);

    // Save command register state.
    let cmd = ahci_pread(ap, AHCI_PREG_CMD) & !AHCI_PREG_CMD_ICC;

    // Port should have been idled already. Start it.
    kassert!((cmd & AHCI_PREG_CMD_CR) == 0);
    let _ = ahci_port_start(ap, false);

    // Prep error CCB for READ LOG EXT, page 10h, 1 sector.
    let ccb = ahci_get_err_ccb(ap);
    ccb.ccb_xa.flags.set(ATA_F_NOWAIT | ATA_F_READ | ATA_F_POLL);
    ccb.ccb_xa.data.set(ap.ap_err_scratch.get());
    ccb.ccb_xa.datalen.set(512);
    ccb.zero_cmd_table();

    ccb.ccb_xa.with_fis(|fis| {
        fis.r#type = ATA_FIS_TYPE_H2D;
        fis.flags = ATA_H2D_FLAGS_CMD | pmp_port as u8;
        fis.command = ATA_C_READ_LOG_EXT;
        fis.lba_low = 0x10; // queued error log page (10h)
        fis.sector_count = 1; // number of sectors (1)
        fis.sector_count_exp = 0;
        fis.lba_mid = 0; // starting offset
        fis.lba_mid_exp = 0;
        fis.device = 0;
    });

    ccb.set_hdr_flags(
        5 /* FIS length: 5 DWORDS */ | ((pmp_port as u16) << AHCI_CMD_LIST_FLAG_PMP_SHIFT),
    );

    'err: {
        if ahci_load_prdt(ccb).is_err() {
            rc = Err(Errno::ENOMEM); // XXX caller must abort all commands
            break 'err;
        }

        ccb.ccb_xa.set_state(ATA_S_PENDING);
        if ahci_poll(ccb, 1000, None).is_err() || ccb.ccb_xa.state() == ATA_S_ERROR {
            break 'err;
        }

        rc = Ok(());
    }

    // err:
    // Abort our command, if it failed, by stopping command DMA.
    if rc.is_err() && ap.ap_active.load(Ordering::Relaxed) & ahci_slot_bit(ccb) != 0 {
        printf(format_args!(
            "{}: log page read failed, slot {} was still active.\n",
            ap.portname(),
            ccb.ccb_slot
        ));
        let _ = ahci_port_stop(ap, false);
    }

    // Done with the error CCB now.
    ahci_unload_prdt(ccb);
    ahci_put_err_ccb(ccb);

    // Extract failed register set and tags from the scratch space.
    if rc.is_ok() {
        // SAFETY: the scratch buffer is `DEV_BSIZE` (512) bytes the controller has written
        // and no longer touches (the map is unloaded); `AtaLogPage10h` has alignment 1 and
        // accepts any bytes.
        let log: AtaLogPage10h =
            unsafe { ptr::read(ap.ap_err_scratch.get().cast::<AtaLogPage10h>()) };
        if log.err_regs.r#type & ATA_LOG_10H_TYPE_NOTQUEUED != 0 {
            // Not queued bit was set - wasn't an NCQ error?
            printf(format_args!(
                "{}: read NCQ error page, but not an NCQ error?\n",
                ap.portname()
            ));
            rc = Err(Errno::ESRCH);
        } else {
            // Copy back the log record as a D2H register FIS.
            err_slot = u32::from(log.err_regs.r#type & ATA_LOG_10H_TYPE_TAG_MASK);

            let c = ap.ccb(err_slot);
            let mut rfis = log.err_regs;
            rfis.r#type = ATA_FIS_TYPE_D2H;
            rfis.flags = 0;
            c.ccb_xa.rfis.set(rfis);
        }
    }

    // Restore saved CMD register state
    ahci_pwrite(ap, AHCI_PREG_CMD, cmd);
    ap.ap_state.set(oldstate);

    rc.map(|()| err_slot)
}

/// `ahci_dmamem_alloc`: `size` bytes of zeroed, mapped and loaded DMA memory.
pub fn ahci_dmamem_alloc(sc: &AhciSoftc, size: usize) -> Option<&'static AhciDmamem> {
    let dmat = sc.dmat();

    let mem = malloc(size_of::<AhciDmamem>(), M_DEVBUF, M_NOWAIT | M_ZERO)?;

    let Ok(map) = bus_dmamap_create(dmat, size, 1, size, 0, BUS_DMA_NOWAIT | BUS_DMA_ALLOCNOW)
    else {
        // admfree:
        free(mem, M_DEVBUF, size_of::<AhciDmamem>());
        return None;
    };

    let destroy = |mem: NonNull<u8>| {
        // SAFETY: the map created above, unused.
        unsafe { bus_dmamap_destroy(dmat, NonNull::from(map)) };
        free(mem, M_DEVBUF, size_of::<AhciDmamem>());
    };

    let mut segs = [BusDmaSegment::default(); 1];
    let Ok(nsegs) = bus_dmamem_alloc(
        dmat,
        size,
        PAGE_SIZE,
        0,
        &mut segs,
        BUS_DMA_NOWAIT | BUS_DMA_ZERO,
    ) else {
        destroy(mem);
        return None;
    };

    let Ok(kva) = bus_dmamem_map(
        dmat,
        &mut segs[..nsegs],
        size,
        BUS_DMA_NOWAIT | BUS_DMA_COHERENT,
    ) else {
        // free:
        // SAFETY: the segment allocated above, not mapped.
        unsafe { bus_dmamem_free(dmat, &segs[..nsegs]) };
        destroy(mem);
        return None;
    };

    // SAFETY: `kva` maps `size` bytes allocated above, which stay until ahci_dmamem_free
    // unloads the map first.
    if unsafe { bus_dmamap_load(dmat, map, kva.as_ptr(), size, None, BUS_DMA_NOWAIT) }.is_err() {
        // unmap:
        // SAFETY: the mapping and the segment made above, unused.
        unsafe {
            bus_dmamem_unmap(dmat, kva, size);
            bus_dmamem_free(dmat, &segs[..nsegs]);
        }
        destroy(mem);
        return None;
    }

    let adm = mem.cast::<AhciDmamem>();
    // SAFETY: a fresh allocation of `size_of::<AhciDmamem>()` bytes (malloc aligns it),
    // written whole before any use; it lives until ahci_dmamem_free.
    unsafe {
        adm.as_ptr().write(AhciDmamem {
            adm_map: map,
            adm_seg: segs[0],
            adm_size: size,
            adm_kva: kva,
        });
        Some(&*adm.as_ptr())
    }
}

/// `ahci_dmamem_free`.
///
/// # Safety
///
/// `adm` came from [`ahci_dmamem_alloc`], the controller no longer uses it and nothing
/// references it afterwards.
pub unsafe fn ahci_dmamem_free(sc: &AhciSoftc, adm: &'static AhciDmamem) {
    let dmat = sc.dmat();

    bus_dmamap_unload(dmat, adm.adm_map);
    // SAFETY: the caller's guarantee: the area's own mapping, segment and map, unloaded.
    unsafe {
        bus_dmamem_unmap(dmat, adm.adm_kva, adm.adm_size);
        bus_dmamem_free(dmat, core::slice::from_ref(&adm.adm_seg));
        bus_dmamap_destroy(dmat, NonNull::from(adm.adm_map));
    }
    free(NonNull::from(adm).cast(), M_DEVBUF, size_of::<AhciDmamem>());
}

/// `ahci_read`: a global register.
pub fn ahci_read(sc: &AhciSoftc, r: BusSize) -> u32 {
    let (t, h) = sc.regs();
    bus_space_barrier(t, h, r, 4, BUS_SPACE_BARRIER_READ);
    bus_space_read_4(t, h, r)
}

/// `ahci_write`.
pub fn ahci_write(sc: &AhciSoftc, r: BusSize, v: u32) {
    let (t, h) = sc.regs();
    bus_space_write_4(t, h, r, v);
    bus_space_barrier(t, h, r, 4, BUS_SPACE_BARRIER_WRITE);
}

/// `ahci_wait_ne`: waits up to a second for `(reg & mask) != target`.
pub fn ahci_wait_ne(sc: &AhciSoftc, r: BusSize, mask: u32, target: u32) -> Result<(), Errno> {
    for _ in 0..1000 {
        if (ahci_read(sc, r) & mask) != target {
            return Ok(());
        }
        delay(1000);
    }

    Err(Errno::ETIMEDOUT)
}

/// The port's register window, which `ahci_port_alloc` creates before any access.
fn ahci_port_regs(ap: &AhciPort) -> (BusSpaceTag, BusSpaceHandle) {
    let (t, _) = ap.sc().regs();
    match ap.ap_ioh.get() {
        Some(h) => (t, h),
        None => panic(format_args!("{}: port registers not mapped", ap.portname())),
    }
}

/// `ahci_pread`: a port register.
pub fn ahci_pread(ap: &AhciPort, r: BusSize) -> u32 {
    let (t, h) = ahci_port_regs(ap);
    bus_space_barrier(t, h, r, 4, BUS_SPACE_BARRIER_READ);
    bus_space_read_4(t, h, r)
}

/// `ahci_pwrite`.
pub fn ahci_pwrite(ap: &AhciPort, r: BusSize, v: u32) {
    let (t, h) = ahci_port_regs(ap);
    bus_space_write_4(t, h, r, v);
    bus_space_barrier(t, h, r, 4, BUS_SPACE_BARRIER_WRITE);
}

/// `ahci_pwait_eq`: waits up to `n` seconds for `(reg & mask) == target`.
pub fn ahci_pwait_eq(
    ap: &AhciPort,
    r: BusSize,
    mask: u32,
    target: u32,
    n: i32,
) -> Result<(), Errno> {
    for _ in 0..n * 1000 {
        if (ahci_pread(ap, r) & mask) == target {
            return Ok(());
        }
        delay(1000);
    }

    Err(Errno::ETIMEDOUT)
}

/// The controller behind atascsi's cookie.
///
/// # Safety
///
/// `xsc` is the `aaa_cookie` ahci_attach gave atascsi: a live softc.
unsafe fn ahci_cookie_sc(xsc: *mut c_void) -> &'static AhciSoftc {
    // SAFETY: the caller's guarantee; softcs are never freed while attached.
    unsafe { &*xsc.cast::<AhciSoftc>().cast_const() }
}

/// `ahci_ata_probe`: atascsi's `ata_probe`.
///
/// # Safety
///
/// `xsc` is the `aaa_cookie` ahci_attach gave atascsi.
pub unsafe fn ahci_ata_probe(xsc: *mut c_void, port: i32, lun: i32) -> i32 {
    // SAFETY: the caller's guarantee.
    let sc = unsafe { ahci_cookie_sc(xsc) };
    let Some(ap) = sc.port(port as usize) else {
        return ATA_PORT_T_NONE;
    };

    if lun != 0 {
        let pmp_port = lun - 1;
        if pmp_port >= ap.ap_pmp_ports.get() {
            return ATA_PORT_T_NONE;
        }
        ahci_pmp_port_probe(ap, pmp_port)
    } else {
        ahci_port_signature(ap)
    }
}

/// `ahci_ata_free`: atascsi's `ata_free`; nothing to do.
///
/// # Safety
///
/// `xsc` is the `aaa_cookie` ahci_attach gave atascsi.
pub unsafe fn ahci_ata_free(_xsc: *mut c_void, _port: i32, _lun: i32) {}

/// `ahci_ata_get_xfer`: atascsi's `ata_get_xfer`: a free ccb of the port.
///
/// # Safety
///
/// `aaa_cookie` is the `aaa_cookie` ahci_attach gave atascsi.
pub unsafe fn ahci_ata_get_xfer(aaa_cookie: *mut c_void, port: i32) -> Option<&'static AtaXfer> {
    // SAFETY: the caller's guarantee.
    let sc = unsafe { ahci_cookie_sc(aaa_cookie) };
    let ap = sc.port(port as usize)?;

    let Some(ccb) = ahci_get_ccb(ap) else {
        // DPRINTF(AHCI_D_XFER, "%s: ahci_ata_get_xfer: NULL ccb\n", PORTNAME(ap));
        return None;
    };

    // DPRINTF(AHCI_D_XFER, "%s: ahci_ata_get_xfer got slot %d\n", ...);

    Some(&ccb.ccb_xa)
}

/// `ahci_ata_put_xfer`: atascsi's `ata_put_xfer`.
pub fn ahci_ata_put_xfer(xa: &'static AtaXfer) {
    // SAFETY: atascsi gives back only the xfers ahci_ata_get_xfer handed out.
    let ccb = unsafe { ahci_xa_ccb(xa) };

    // DPRINTF(AHCI_D_XFER, "ahci_ata_put_xfer slot %d\n", ccb->ccb_slot);

    ahci_put_ccb(ccb);
}

/// `ahci_ata_cmd`: atascsi's `ata_cmd`: loads and issues a command (polled with
/// `ATA_F_POLL`, else with a timeout).
pub fn ahci_ata_cmd(xa: &'static AtaXfer) {
    // SAFETY: atascsi issues only the xfers ahci_ata_get_xfer handed out.
    let ccb = unsafe { ahci_xa_ccb(xa) };

    'failcmd: {
        if ccb.ccb_port.ap_state.get() == AP_S_FATAL_ERROR {
            break 'failcmd;
        }

        ccb.ccb_done.set(ahci_ata_cmd_done);

        let mut flags: u16 = 5; // FIS length (in DWORDs)
        flags |= (xa.pmp_port.get() as u16) << AHCI_CMD_LIST_FLAG_PMP_SHIFT;

        if xa.flags.get() & ATA_F_WRITE != 0 {
            flags |= AHCI_CMD_LIST_FLAG_W;
        }

        if xa.flags.get() & ATA_F_PACKET != 0 {
            flags |= AHCI_CMD_LIST_FLAG_A;
        }

        ccb.set_hdr_flags(flags);

        if ahci_load_prdt(ccb).is_err() {
            break 'failcmd;
        }

        timeout_set(&xa.stimeout, ahci_ata_cmd_timeout, ahci_ccb_cookie(ccb));

        xa.set_state(ATA_S_PENDING);

        if xa.flags.get() & ATA_F_POLL != 0 {
            let _ = ahci_poll(ccb, xa.timeout.get() as i32, Some(ahci_ata_cmd_timeout));
        } else {
            let s = splbio();
            timeout_add_msec(&xa.stimeout, u64::from(xa.timeout.get()));
            ahci_start(ccb);
            splx(s);
        }

        return;
    }

    // failcmd:
    let s = splbio();
    xa.set_state(ATA_S_ERROR);
    ata_complete(xa);
    splx(s);
}

/// `ahci_pmp_cmd_done`: the completion of the PMP ccb.
pub fn ahci_pmp_cmd_done(ccb: &'static AhciCcb) {
    let xa = &ccb.ccb_xa;

    if xa.state() == ATA_S_ONCHIP || xa.state() == ATA_S_ERROR {
        ahci_issue_pending_commands(ccb.ccb_port, xa.flags.get() & ATA_F_NCQ != 0);
    }

    xa.set_state(ATA_S_COMPLETE);
}

/// `ahci_ata_cmd_done`: the completion of an atascsi command.
pub fn ahci_ata_cmd_done(ccb: &'static AhciCcb) {
    let xa = &ccb.ccb_xa;

    timeout_del(&xa.stimeout);

    if xa.state() == ATA_S_ONCHIP || xa.state() == ATA_S_ERROR {
        ahci_issue_pending_commands(ccb.ccb_port, xa.flags.get() & ATA_F_NCQ != 0);
    }

    ahci_unload_prdt(ccb);

    if xa.state() == ATA_S_ONCHIP {
        xa.set_state(ATA_S_COMPLETE);
    } else {
        #[cfg(feature = "diagnostic")]
        if xa.state() != ATA_S_ERROR && xa.state() != ATA_S_TIMEOUT {
            printf(format_args!(
                "{}: invalid ata_xfer state {:02x} in ahci_ata_cmd_done, slot {}\n",
                ccb.ccb_port.portname(),
                xa.state(),
                ccb.ccb_slot
            ));
        }
    }
    if xa.state() != ATA_S_TIMEOUT {
        ata_complete(xa);
    }
}

/// `ahci_ata_cmd_timeout`: a command took too long: complete it with a timeout and reset the
/// port to abort it.
pub fn ahci_ata_cmd_timeout(arg: *mut c_void) {
    // SAFETY: ahci_ata_cmd sets the timeout (and ahci_poll calls it) with the ccb, one of a
    // live port's.
    let ccb: &'static AhciCcb = unsafe { &*arg.cast::<AhciCcb>().cast_const() };
    let xa = &ccb.ccb_xa;
    let ap = ccb.ccb_port;
    let bit = ahci_slot_bit(ccb);

    let s = splbio();

    let ncq_cmd = xa.flags.get() & ATA_F_NCQ != 0;
    let active: &AtomicU32 = if ncq_cmd {
        &ap.ap_sactive
    } else {
        &ap.ap_active
    };

    'ret: {
        let ccb_was_started = if xa.state() == ATA_S_PENDING {
            // DPRINTF(AHCI_D_TIMEOUT, "%s: command for slot %d timed out before it got on
            //     chip\n", ...);
            // SAFETY: a pending ccb is on the pending list.
            unsafe { ap.ap_ccb_pending.remove(ccb) };
            false
        } else if xa.state() == ATA_S_ONCHIP && ahci_port_intr(ap, bit) != 0 {
            // DPRINTF(AHCI_D_TIMEOUT, "%s: final poll of port completed command in slot
            //     %d\n", ...);
            break 'ret;
        } else if xa.state() != ATA_S_ONCHIP {
            // DPRINTF(AHCI_D_TIMEOUT, "%s: command slot %d already handled%s\n", ...);
            break 'ret;
        } else if ahci_pread(
            ap,
            if ncq_cmd {
                AHCI_PREG_SACT
            } else {
                AHCI_PREG_CI
            },
        ) & bit
            == 0
            && active.load(Ordering::Relaxed) & bit != 0
        {
            // DPRINTF(AHCI_D_TIMEOUT, "%s: command slot %d completed but IRQ handler didn't
            //     detect it.  Why?\n", ...);
            active.fetch_and(!bit, Ordering::Relaxed);
            (ccb.ccb_done.get())(ccb);
            break 'ret;
        } else {
            true
        };

        // Complete the slot with a timeout error.
        xa.set_state(ATA_S_TIMEOUT);
        active.fetch_and(!bit, Ordering::Relaxed);
        // DPRINTF(AHCI_D_TIMEOUT, "%s: run completion (1)\n", PORTNAME(ap));
        (ccb.ccb_done.get())(ccb); // This won't issue pending commands or run the atascsi
        // completion.

        // Reset port to abort running command.
        if ccb_was_started {
            // DPRINTF(AHCI_D_TIMEOUT, "%s: resetting port to abort%s command in slot %d,
            //     pmp port %d, active %08x\n", ...);
            if ahci_port_softreset(ap).is_err() && ahci_port_portreset(ap, false).is_err() {
                printf(format_args!(
                    "{}: failed to reset port during timeout handling, disabling it\n",
                    ap.portname()
                ));
                ap.ap_state.set(AP_S_FATAL_ERROR);
            }

            // Restart any other commands that were aborted by the reset.
            let a = active.load(Ordering::Relaxed);
            if a != 0 {
                // DPRINTF(AHCI_D_TIMEOUT, "%s: re-enabling%s slots %08x\n", ...);
                if ncq_cmd {
                    ahci_pwrite(ap, AHCI_PREG_SACT, a);
                }
                ahci_pwrite(ap, AHCI_PREG_CI, a);
            }
        }

        // Issue any pending commands now.
        // DPRINTF(AHCI_D_TIMEOUT, "%s: issue pending\n", PORTNAME(ap));
        if ccb_was_started {
            ahci_issue_pending_commands(ap, ncq_cmd);
        } else if ap.ap_active.load(Ordering::Relaxed) == 0 {
            ahci_issue_pending_ncq_commands(ap);
        }

        // Complete the timed out ata_xfer I/O (may generate new I/O).
        // DPRINTF(AHCI_D_TIMEOUT, "%s: run completion (2)\n", PORTNAME(ap));
        ata_complete(xa);

        // DPRINTF(AHCI_D_TIMEOUT, "%s: splx\n", PORTNAME(ap));
    }

    // ret:
    splx(s);
}

/// `ahci_empty_done`: the completion of the error-recovery commands.
pub fn ahci_empty_done(ccb: &'static AhciCcb) {
    if ccb.ccb_xa.state() != ATA_S_ERROR {
        ccb.ccb_xa.set_state(ATA_S_COMPLETE);
    }
}

/// The FIS of a READ or WRITE PORT MULTIPLIER command for register `which` of port `target`
/// (`data` for a write).
fn ahci_pmp_fis(fis: &mut AtaFisH2d, command: u8, target: i32, which: i32, data: u32) {
    fis.r#type = ATA_FIS_TYPE_H2D;
    fis.flags = ATA_H2D_FLAGS_CMD | SATA_PMP_CONTROL_PORT as u8;
    fis.command = command;
    fis.features = which as u8;
    fis.device = target as u8 | ATA_H2D_DEVICE_LBA;
    if command == ATA_C_WRITE_PM {
        fis.sector_count = data as u8;
        fis.lba_low = (data >> 8) as u8;
        fis.lba_mid = (data >> 16) as u8;
        fis.lba_high = (data >> 24) as u8;
    }
    fis.control = ATA_FIS_CONTROL_4BIT;
}

/// `ahci_pmp_read`: reads register `which` of port multiplier port `target`.
pub fn ahci_pmp_read(ap: &'static AhciPort, target: i32, which: i32) -> Result<u32, Errno> {
    let ccb = ahci_get_pmp_ccb(ap); // Always returns non-NULL.
    ccb.ccb_xa.flags.set(ATA_F_POLL | ATA_F_GET_RFIS);
    ccb.ccb_xa.pmp_port.set(SATA_PMP_CONTROL_PORT);
    ccb.ccb_xa.set_state(ATA_S_PENDING);

    ccb.zero_cmd_table();
    ccb.ccb_xa
        .with_fis(|fis| ahci_pmp_fis(fis, ATA_C_READ_PM, target, which, 0));

    let rv = if ahci_poll(ccb, 1000, Some(ahci_pmp_probe_timeout)).is_err() {
        Err(Errno::EIO)
    } else {
        let rfis = ccb.ccb_xa.rfis.get();
        Ok(u32::from(rfis.sector_count)
            | (u32::from(rfis.lba_low) << 8)
            | (u32::from(rfis.lba_mid) << 16)
            | (u32::from(rfis.lba_high) << 24))
    };
    ahci_put_pmp_ccb(ccb);
    rv
}

/// `ahci_pmp_write`: writes register `which` of port multiplier port `target`.
pub fn ahci_pmp_write(
    ap: &'static AhciPort,
    target: i32,
    which: i32,
    data: u32,
) -> Result<(), Errno> {
    let ccb = ahci_get_pmp_ccb(ap); // Always returns non-NULL.
    ccb.ccb_xa.flags.set(ATA_F_POLL);
    ccb.ccb_xa.pmp_port.set(SATA_PMP_CONTROL_PORT);
    ccb.ccb_xa.set_state(ATA_S_PENDING);

    ccb.zero_cmd_table();
    ccb.ccb_xa
        .with_fis(|fis| ahci_pmp_fis(fis, ATA_C_WRITE_PM, target, which, data));

    let error = ahci_poll(ccb, 1000, Some(ahci_pmp_probe_timeout));
    ahci_put_pmp_ccb(ccb);
    error
}

/// `ahci_pmp_phy_status`: reads a port multiplier port's SStatus and clears its SError (the
/// C's `*datap`, 0 on failure, is not used by any caller beyond the error).
pub fn ahci_pmp_phy_status(ap: &'static AhciPort, target: i32) -> Result<u32, Errno> {
    let data = ahci_pmp_read(ap, target, SATA_PMREG_SSTS)?;
    ahci_pmp_write(ap, target, SATA_PMREG_SERR, u32::MAX)?;
    Ok(data)
}

/// `ahci_pmp_identify`: identifies the port multiplier and returns its number of ports.
pub fn ahci_pmp_identify(ap: &'static AhciPort) -> Result<i32, Errno> {
    let s = splbio();

    let regs = (|| {
        Ok::<_, Errno>((
            ahci_pmp_read(ap, 15, 0)?,
            ahci_pmp_read(ap, 15, 1)?,
            ahci_pmp_read(ap, 15, 2)?,
            ahci_pmp_read(ap, 15, SATA_PMREG_FEA)?,
            ahci_pmp_read(ap, 15, SATA_PMREG_FEAEN)?,
        ))
    })();
    let Ok((chipid, rev, nports, features, enabled)) = regs else {
        printf(format_args!(
            "{}: port multiplier identification failed\n",
            ap.portname()
        ));
        splx(s);
        return Err(Errno::EIO);
    };
    splx(s);

    let mut nports = nports & 0x0F;

    // ignore SEMB port on SiI3726 port multiplier chips
    if chipid == 0x37261095 {
        nports = nports.wrapping_sub(1);
    }

    printf(format_args!(
        "{}: port multiplier found: chip={:08x} rev=0x{} nports={}, features: 0x{}, enabled: 0x{}\n",
        ap.portname(),
        chipid,
        Bitmask(u64::from(rev), SATA_PFMT_PM_REV),
        nports as i32,
        Bitmask(u64::from(features), SATA_PFMT_PM_FEA),
        Bitmask(u64::from(enabled), SATA_PFMT_PM_FEA)
    ));

    Ok(nports as i32)
}

// HIBERNATE: not configured (ahci_hibernate_io_start, ahci_hibernate_io_poll,
// ahci_hibernate_load_prdt, ahci_hibernate_io).
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revisions_and_signatures() {
        // QEMU's ich9-ahci reports AHCI 1.0.
        assert_eq!(ahci_revision(0x0001_0000), Some("1.0"));
        assert_eq!(ahci_revision(0x0000_0905), Some("0.95"));
        assert_eq!(ahci_revision(0x0001_0301), Some("1.3.1"));
        assert_eq!(ahci_revision(0x0002_0000), None);
        assert_eq!(ahci_signature_type(0x0000_0101), ATA_PORT_T_DISK);
        assert_eq!(ahci_signature_type(0xeb14_0101), ATA_PORT_T_ATAPI);
        assert_eq!(ahci_signature_type(0x9669_0101), ATA_PORT_T_PM);
        // Only the high half counts.
        assert_eq!(ahci_signature_type(0xeb14_0000), ATA_PORT_T_ATAPI);
    }

    #[test]
    fn prdt_entries() {
        let prd = ahci_load_prdt_seg(0x1234_5000, 4096, 0);
        assert_eq!(u64::from_le(prd.dba), 0x1234_5000);
        assert_eq!(u32::from_le(prd.flags), 4095);
        let prd = ahci_load_prdt_seg(0x8000, 512, AHCI_PRDT_FLAG_INTR);
        assert_eq!(u32::from_le(prd.flags), 0x8000_01ff);
    }

    #[test]
    fn pmp_register_fis() {
        let mut fis = AtaFisH2d::default();
        ahci_pmp_fis(&mut fis, ATA_C_WRITE_PM, 3, SATA_PMREG_SCTL, 0x0403_0201);
        assert_eq!(fis.r#type, ATA_FIS_TYPE_H2D);
        assert_eq!(fis.flags, ATA_H2D_FLAGS_CMD | 0x0f);
        assert_eq!((fis.command, fis.features, fis.device), (0xe8, 2, 0x43));
        assert_eq!(
            [fis.sector_count, fis.lba_low, fis.lba_mid, fis.lba_high],
            [1, 2, 3, 4]
        );
        assert_eq!(fis.control, ATA_FIS_CONTROL_4BIT);
        let mut fis = AtaFisH2d::default();
        ahci_pmp_fis(&mut fis, ATA_C_READ_PM, 15, 2, 0xffff_ffff);
        assert_eq!((fis.command, fis.device, fis.sector_count), (0xe4, 0x4f, 0));
    }
}
/* </TESTS> */
