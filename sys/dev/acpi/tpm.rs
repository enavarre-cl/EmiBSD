/* $OpenBSD: tpm.c,v 1.20 2024/05/29 12:21:33 kettenis Exp $ */
/* <LICENSES> */
/*
 * Minimal interface to Trusted Platform Module chips implementing the
 * TPM Interface Spec 1.2, just enough to tell the TPM to save state before
 * a system suspend.
 *
 * Copyright (c) 2008, 2009 Michael Shalayeff
 * Copyright (c) 2009, 2010 Hans-Joerg Hoexer
 * Copyright (c) 2016 joshua stein <jcs@openbsd.org>
 * All rights reserved.
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF MIND, USE, DATA OR PROFITS, WHETHER IN
 * AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT
 * OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */
/* </LICENSES> */

/* <CODE> */
//! tpm(4): `dev/acpi/tpm.c`, a minimal driver for Trusted Platform Module chips behind the
//! TPM Interface Specification 1.2 (TIS) registers or TPM 2.0's Command Response Buffer
//! (CRB), just enough to tell the TPM to save its state before the machine suspends.
//!
//! Upstream: sys/dev/acpi/tpm.c @ 3ce1f3f79392
//!
//! tpm attaches to the ACPI devices of `tpm_hids` (`PNP0C31`, `MSFT0101`, ...) that have a
//! register window. A `MSFT0101` device is a TPM 2.0, and the `TPM2` table's start method
//! says whether its registers are TIS (6) or CRB (7); the others are TIS 1.2 chips. The
//! attachment maps the window, disables the TPM's interrupts (the driver polls), takes
//! locality 0 and names the chip. On `DVACT_SUSPEND` (with acpi0 leaving S0) it sends
//! `TPM_ORD_SaveState` (1.2) or `TPM2_Shutdown(TPM_SU_STATE)` (2.0) through the TIS FIFO or
//! the CRB command buffer and reads the response header back; `DVACT_WAKEUP` does nothing
//! (the firmware restores the state). amd64's ioconf has `tpm* at acpi?`, as its GENERIC;
//! arm64's GENERIC has no tpm.
//!
//! Under QEMU, `-device tpm-tis` (or `tpm-crb`) backed by `swtpm` is a `MSFT0101` device
//! with a `TPM2` table (xtask `--tpm tis|crb`, `smoke-tpm`).
//!
//! ## Deviations
//! - The functions return `Result<_, Errno>` where the C returns an int that is 0 or an
//!   errno. `tpm_waitfor_status` (and so `tpm_read_tis`/`tpm_write_tis`, which pass it on)
//!   fails with `ETIMEDOUT` where the C returns the status byte it saw, and `tpm_write_tis`
//!   with `EIO` where it returns the status that still expects data: no caller looks at the
//!   value. `tpm_read_tis` returns the byte count the C stores through `count`.
//! - `tpm_waitfor_status`'s and `tpm_waitfor`'s timeouts and the TIS FIFO accesses keep the
//!   C's polling with `DELAY(1)`/`DELAY(10)`; `DPRINTF` (`TPM_DEBUG`, not defined) is
//!   compiled out, so its messages are not here.
//! - The registers are read through the softc's tag and handle, which are `Cell`s set by
//!   the attachment (a zeroed softc has none); before that a read returns all ones and a
//!   write is dropped. Nothing reaches them earlier: `tpm_activate` checks `sc_enabled`.
//! - `strcmp(aaa_dev, "MSFT0101") == 0 || strcmp(aaa_cdev, "MSFT0101") == 0` is
//!   `acpi_matchhids` with `MSFT0101` alone, which is the same test (acpi0 always hands a
//!   `_HID` with a node).
//! - Nothing suspends yet: `subr_suspend.c` (`sleep_state`) and the S3 path of
//!   `acpi_x86.c` are not ported, so `config_suspend` never sends `DVACT_SUSPEND` or
//!   `DVACT_WAKEUP` and `tpm_suspend`/`tpm_resume` are ported but have no trigger
//!   (`acpi.rs` reports `sleep_state` where it is reached).
//! - [`tpm_selftest`], under feature `qemu`, is test plumbing and not in the C: the
//!   `selftest=tpm` hook (`kern/selftest.rs`) sends `TPM2_SelfTest` through the same
//!   `tpm_write_tis`/`tpm_read_tis` (or `_crb`) path `tpm_suspend` uses and prints the
//!   response code, so `smoke-tpm` sees the TPM (swtpm behind QEMU) answer.

use core::cell::{Cell, RefCell};
use core::ffi::c_void;
use core::ptr;

use super::acpi::{acpi_getsta, acpi_matchhids, q_table_bytes};
use super::acpidev::{STA_DEV_OK, STA_ENABLED, STA_PRESENT};
use super::acpireg::{ACPI_STATE_S0, AcpiTpm2, TPM2_SIG};
use super::acpivar::{AcpiAttachArgs, AcpiSoftc};
use super::amltypes::AmlNodeRef;
use super::dsdt::cstr;
use crate::kern::subr_prf::{Str, printf};
use crate::machine::bus::{
    BUS_SPACE_BARRIER_WRITE, BusAddr, BusSize, BusSpaceHandle, BusSpaceTag, bus_space_barrier,
    bus_space_map, bus_space_read_1, bus_space_read_4, bus_space_write_1, bus_space_write_4,
};
use crate::machine::cpu::delay;
use crate::sys::device::{
    CD_SKIPHIBERNATE, CfMatch, Cfattach, Cfdriver, DV_DULL, DVACT_SUSPEND, DVACT_WAKEUP, Device,
    Softc,
};
use crate::sys::errno::Errno;

/// `TPM_BUFSIZ`.
pub const TPM_BUFSIZ: usize = 1024;
/// `TPM_HDRSIZE`: a command or response header: tag, size, code.
pub const TPM_HDRSIZE: usize = 10;
/// `TPM_PARAM_SIZE`: `tpm_read_tis` flag: read `len` bytes, not one header's burst.
pub const TPM_PARAM_SIZE: i32 = 0x0001;

/// `TPM_ACCESS`: access register.
pub const TPM_ACCESS: BusSize = 0x0000;
/// `TPM_ACCESS_ESTABLISHMENT`: establishment.
pub const TPM_ACCESS_ESTABLISHMENT: u8 = 0x01;
/// `TPM_ACCESS_REQUEST_USE`: request using locality.
pub const TPM_ACCESS_REQUEST_USE: u8 = 0x02;
/// `TPM_ACCESS_REQUEST_PENDING`: pending request.
pub const TPM_ACCESS_REQUEST_PENDING: u8 = 0x04;
/// `TPM_ACCESS_SEIZE`: request locality seize.
pub const TPM_ACCESS_SEIZE: u8 = 0x08;
/// `TPM_ACCESS_SEIZED`: locality has been seized.
pub const TPM_ACCESS_SEIZED: u8 = 0x10;
/// `TPM_ACCESS_ACTIVE_LOCALITY`: locality is active.
pub const TPM_ACCESS_ACTIVE_LOCALITY: u8 = 0x20;
/// `TPM_ACCESS_VALID`: bits are valid.
pub const TPM_ACCESS_VALID: u8 = 0x80;
/// `TPM_ACCESS_BITS`: the `%b` description of the access register.
pub const TPM_ACCESS_BITS: &[u8] = b"\x10\x01EST\x02REQ\x03PEND\x04SEIZE\x05SEIZED\x06ACT\x08VALID";

/// `TPM_INTERRUPT_ENABLE`.
pub const TPM_INTERRUPT_ENABLE: BusSize = 0x0008;
/// `TPM_GLOBAL_INT_ENABLE`: enable ints.
pub const TPM_GLOBAL_INT_ENABLE: u32 = 0x8000_0000;
/// `TPM_CMD_READY_INT`: cmd ready enable.
pub const TPM_CMD_READY_INT: u32 = 0x0000_0080;
/// `TPM_INT_EDGE_FALLING`.
pub const TPM_INT_EDGE_FALLING: u32 = 0x0000_0018;
/// `TPM_INT_EDGE_RISING`.
pub const TPM_INT_EDGE_RISING: u32 = 0x0000_0010;
/// `TPM_INT_LEVEL_LOW`.
pub const TPM_INT_LEVEL_LOW: u32 = 0x0000_0008;
/// `TPM_INT_LEVEL_HIGH`.
pub const TPM_INT_LEVEL_HIGH: u32 = 0x0000_0000;
/// `TPM_LOCALITY_CHANGE_INT`: locality change enable.
pub const TPM_LOCALITY_CHANGE_INT: u32 = 0x0000_0004;
/// `TPM_STS_VALID_INT`: int on `TPM_STS_VALID` is set.
pub const TPM_STS_VALID_INT: u32 = 0x0000_0002;
/// `TPM_DATA_AVAIL_INT`: int on `TPM_STS_DATA_AVAIL` is set.
pub const TPM_DATA_AVAIL_INT: u32 = 0x0000_0001;
/// `TPM_INTERRUPT_ENABLE_BITS`.
pub const TPM_INTERRUPT_ENABLE_BITS: &[u8] = b"\x10\x20ENA\x08RDY\x03LOCH\x02STSV\x01DRDY";

/// `TPM_INT_VECTOR`: 8 bit reg for 4 bit irq vector.
pub const TPM_INT_VECTOR: BusSize = 0x000c;
/// `TPM_INT_STATUS`: bits are & 0x87 from `TPM_INTERRUPT_ENABLE`.
pub const TPM_INT_STATUS: BusSize = 0x0010;

/// `TPM_INTF_CAPABILITIES`: capability register.
pub const TPM_INTF_CAPABILITIES: BusSize = 0x0014;
/// `TPM_INTF_BURST_COUNT_STATIC`: `TPM_STS_BMASK` static.
pub const TPM_INTF_BURST_COUNT_STATIC: u32 = 0x0100;
/// `TPM_INTF_CMD_READY_INT`: int on ready supported.
pub const TPM_INTF_CMD_READY_INT: u32 = 0x0080;
/// `TPM_INTF_INT_EDGE_FALLING`: falling edge ints supported.
pub const TPM_INTF_INT_EDGE_FALLING: u32 = 0x0040;
/// `TPM_INTF_INT_EDGE_RISING`: rising edge ints supported.
pub const TPM_INTF_INT_EDGE_RISING: u32 = 0x0020;
/// `TPM_INTF_INT_LEVEL_LOW`: level-low ints supported.
pub const TPM_INTF_INT_LEVEL_LOW: u32 = 0x0010;
/// `TPM_INTF_INT_LEVEL_HIGH`: level-high ints supported.
pub const TPM_INTF_INT_LEVEL_HIGH: u32 = 0x0008;
/// `TPM_INTF_LOCALITY_CHANGE_INT`: locality-change int (mb 1).
pub const TPM_INTF_LOCALITY_CHANGE_INT: u32 = 0x0004;
/// `TPM_INTF_STS_VALID_INT`: `TPM_STS_VALID` int supported.
pub const TPM_INTF_STS_VALID_INT: u32 = 0x0002;
/// `TPM_INTF_DATA_AVAIL_INT`: `TPM_STS_DATA_AVAIL` int supported (mb 1).
pub const TPM_INTF_DATA_AVAIL_INT: u32 = 0x0001;
/// `TPM_CAPSREQ`: the capabilities a TIS chip must have.
pub const TPM_CAPSREQ: u32 =
    TPM_INTF_DATA_AVAIL_INT | TPM_INTF_LOCALITY_CHANGE_INT | TPM_INTF_INT_LEVEL_LOW;
/// `TPM_CAPBITS`.
pub const TPM_CAPBITS: &[u8] =
    b"\x10\x01IDRDY\x02ISTSV\x03ILOCH\x04IHIGH\x05ILOW\x06IEDGE\x07IFALL\x08IRDY\x09BCST";

/// `TPM_STS`: status register.
pub const TPM_STS: BusSize = 0x0018;
/// `TPM_STS_MASK`: status bits.
pub const TPM_STS_MASK: u32 = 0x0000_00ff;
/// `TPM_STS_BMASK`: ro io burst size.
pub const TPM_STS_BMASK: u32 = 0x00ff_ff00;
/// `TPM_STS_VALID`: ro other bits are valid.
pub const TPM_STS_VALID: u8 = 0x80;
/// `TPM_STS_CMD_READY`: rw chip/signal ready.
pub const TPM_STS_CMD_READY: u8 = 0x40;
/// `TPM_STS_GO`: wo start the command.
pub const TPM_STS_GO: u8 = 0x20;
/// `TPM_STS_DATA_AVAIL`: ro data available.
pub const TPM_STS_DATA_AVAIL: u8 = 0x10;
/// `TPM_STS_DATA_EXPECT`: ro more data to be written.
pub const TPM_STS_DATA_EXPECT: u8 = 0x08;
/// `TPM_STS_RESP_RETRY`: wo resend the response.
pub const TPM_STS_RESP_RETRY: u8 = 0x02;
/// `TPM_STS_BITS`.
pub const TPM_STS_BITS: &[u8] = b"\x10\x08VALID\x07RDY\x06GO\x05DRDY\x04EXPECT\x02RETRY";

/// `TPM_DATA`: the FIFO.
pub const TPM_DATA: BusSize = 0x0024;
/// `TPM_ID`: vendor and device id.
pub const TPM_ID: BusSize = 0x0f00;
/// `TPM_REV`: revision.
pub const TPM_REV: BusSize = 0x0f04;
/// `TPM_SIZE`: five pages of the above.
pub const TPM_SIZE: BusSize = 0x5000;

/// `TPM_ACCESS_TMO`: 2 s.
pub const TPM_ACCESS_TMO: i32 = 2000;
/// `TPM_READY_TMO`: 2 s.
pub const TPM_READY_TMO: i32 = 2000;
/// `TPM_READ_TMO`: 2 minutes.
pub const TPM_READ_TMO: i32 = 120_000;
/// `TPM_BURST_TMO`: 2 s.
pub const TPM_BURST_TMO: i32 = 2000;

/// `TPM2_START_METHOD_TIS`: the `TPM2` table's start method of a TIS (FIFO) interface.
pub const TPM2_START_METHOD_TIS: u32 = 6;
/// `TPM2_START_METHOD_CRB`: the start method of a Command Response Buffer.
pub const TPM2_START_METHOD_CRB: u32 = 7;

/// `TPM_CRB_LOC_STATE`.
pub const TPM_CRB_LOC_STATE: BusSize = 0x0;
/// `TPM_CRB_LOC_CTRL`.
pub const TPM_CRB_LOC_CTRL: BusSize = 0x8;
/// `TPM_LOC_STS`.
pub const TPM_LOC_STS: BusSize = 0xC;
/// `TPM_CRB_INTF_ID`.
pub const TPM_CRB_INTF_ID: BusSize = 0x30;
/// `TPM_CRB_CTRL_EXT`.
pub const TPM_CRB_CTRL_EXT: BusSize = 0x38;
/// `TPM_CRB_CTRL_REQ`.
pub const TPM_CRB_CTRL_REQ: BusSize = 0x40;
/// `TPM_CRB_CTRL_STS`.
pub const TPM_CRB_CTRL_STS: BusSize = 0x44;
/// `TPM_CRB_CTRL_CANCEL`.
pub const TPM_CRB_CTRL_CANCEL: BusSize = 0x48;
/// `TPM_CRB_CTRL_START`.
pub const TPM_CRB_CTRL_START: BusSize = 0x4C;
/// `TPM_CRB_CTRL_CMD_SIZE`.
pub const TPM_CRB_CTRL_CMD_SIZE: BusSize = 0x58;
/// `TPM_CRB_CTRL_CMD_LADDR`.
pub const TPM_CRB_CTRL_CMD_LADDR: BusSize = 0x5C;
/// `TPM_CRB_CTRL_CMD_HADDR`.
pub const TPM_CRB_CTRL_CMD_HADDR: BusSize = 0x60;
/// `TPM_CRB_CTRL_RSP_SIZE`.
pub const TPM_CRB_CTRL_RSP_SIZE: BusSize = 0x64;
/// `TPM_CRB_CTRL_RSP_LADDR`.
pub const TPM_CRB_CTRL_RSP_LADDR: BusSize = 0x68;
/// `TPM_CRB_CTRL_RSP_HADDR`.
pub const TPM_CRB_CTRL_RSP_HADDR: BusSize = 0x6c;
/// `TPM_CRB_DATA_BUFFER`.
pub const TPM_CRB_DATA_BUFFER: BusSize = 0x80;

/// `TPM_CRB_LOC_STATE_ESTB`.
pub const TPM_CRB_LOC_STATE_ESTB: u32 = 1 << 0;
/// `TPM_CRB_LOC_STATE_ASSIGNED`.
pub const TPM_CRB_LOC_STATE_ASSIGNED: u32 = 1 << 1;
/// `TPM_CRB_LOC_ACTIVE_MASK`.
pub const TPM_CRB_LOC_ACTIVE_MASK: u32 = 0x009c;
/// `TPM_CRB_LOC_VALID`.
pub const TPM_CRB_LOC_VALID: u32 = 1 << 7;

/// `TPM_CRB_LOC_REQUEST`.
pub const TPM_CRB_LOC_REQUEST: u32 = 1 << 0;
/// `TPM_CRB_LOC_RELEASE`.
pub const TPM_CRB_LOC_RELEASE: u32 = 1 << 1;

/// `TPM_CRB_CTRL_REQ_GO_READY`.
pub const TPM_CRB_CTRL_REQ_GO_READY: u32 = 1 << 0;
/// `TPM_CRB_CTRL_REQ_GO_IDLE`.
pub const TPM_CRB_CTRL_REQ_GO_IDLE: u32 = 1 << 1;

/// `TPM_CRB_CTRL_STS_ERR_BIT`.
pub const TPM_CRB_CTRL_STS_ERR_BIT: u32 = 1 << 0;
/// `TPM_CRB_CTRL_STS_IDLE_BIT`.
pub const TPM_CRB_CTRL_STS_IDLE_BIT: u32 = 1 << 1;

/// `TPM_CRB_CTRL_CANCEL_CMD`.
pub const TPM_CRB_CTRL_CANCEL_CMD: u32 = 0x1;
/// `TPM_CRB_CTRL_CANCEL_CLEAR`.
pub const TPM_CRB_CTRL_CANCEL_CLEAR: u32 = 0x0;

/// `TPM_CRB_CTRL_START_CMD`.
pub const TPM_CRB_CTRL_START_CMD: u32 = 1 << 0;
/// `TPM_CRB_INT_ENABLED_BIT`.
pub const TPM_CRB_INT_ENABLED_BIT: u32 = 1 << 31;

/// `TPM2_RC_SUCCESS`.
pub const TPM2_RC_SUCCESS: u32 = 0x0000;
/// `TPM2_RC_INITIALIZE`.
pub const TPM2_RC_INITIALIZE: u32 = 0x0100;
/// `TPM2_RC_FAILURE`.
pub const TPM2_RC_FAILURE: u32 = 0x0101;
/// `TPM2_RC_DISABLED`.
pub const TPM2_RC_DISABLED: u32 = 0x0120;
/// `TPM2_RC_RETRY`.
pub const TPM2_RC_RETRY: u32 = 0x0922;

/// `TPM_TIS`: `sc_tpm_mode` of a TIS (FIFO) interface.
pub const TPM_TIS: i32 = 0;
/// `TPM_CRB`: `sc_tpm_mode` of a Command Response Buffer.
pub const TPM_CRB: i32 = 1;

/// `struct tpm_softc`.
#[repr(C)]
pub struct TpmSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `sc_bt`.
    pub sc_bt: Cell<Option<BusSpaceTag>>,
    /// `sc_bh`.
    pub sc_bh: Cell<Option<BusSpaceHandle>>,
    /// `sc_bbase`: the window's physical address (the CRB buffers' addresses are absolute).
    pub sc_bbase: Cell<BusSize>,
    /// `sc_acpi`: acpi0.
    pub sc_acpi: Cell<*const AcpiSoftc>,
    /// `sc_devnode`.
    pub sc_devnode: RefCell<Option<AmlNodeRef>>,
    /// `sc_devid`: `TPM_ID` (TIS).
    pub sc_devid: Cell<u32>,
    /// `sc_rev`: `TPM_REV` (TIS).
    pub sc_rev: Cell<u32>,
    /// `sc_tpm20`: a TPM 2.0 (`MSFT0101`).
    pub sc_tpm20: Cell<i32>,
    /// `sc_tpm_mode`: [`TPM_TIS`] or [`TPM_CRB`].
    pub sc_tpm_mode: Cell<i32>,
    /// `sc_cmd_off`: the CRB command buffer, from the window's start.
    pub sc_cmd_off: Cell<BusSize>,
    /// `sc_rsp_off`: the CRB response buffer, from the window's start.
    pub sc_rsp_off: Cell<BusSize>,
    /// `sc_cmd_sz`.
    pub sc_cmd_sz: Cell<usize>,
    /// `sc_rsp_sz`.
    pub sc_rsp_sz: Cell<usize>,
    /// `sc_enabled`: the attachment went through.
    pub sc_enabled: Cell<i32>,
}

impl TpmSoftc {
    /// `bus_space_read_1(sc->sc_bt, sc->sc_bh, off)`.
    fn read_1(&self, off: BusSize) -> u8 {
        match (self.sc_bt.get(), self.sc_bh.get()) {
            (Some(t), Some(h)) => bus_space_read_1(t, h, off),
            _ => 0xff,
        }
    }

    /// `bus_space_read_4(sc->sc_bt, sc->sc_bh, off)`.
    fn read_4(&self, off: BusSize) -> u32 {
        match (self.sc_bt.get(), self.sc_bh.get()) {
            (Some(t), Some(h)) => bus_space_read_4(t, h, off),
            _ => 0xffff_ffff,
        }
    }

    /// `bus_space_write_1(sc->sc_bt, sc->sc_bh, off, v)`.
    fn write_1(&self, off: BusSize, v: u8) {
        if let (Some(t), Some(h)) = (self.sc_bt.get(), self.sc_bh.get()) {
            bus_space_write_1(t, h, off, v);
        }
    }

    /// `bus_space_write_4(sc->sc_bt, sc->sc_bh, off, v)`.
    fn write_4(&self, off: BusSize, v: u32) {
        if let (Some(t), Some(h)) = (self.sc_bt.get(), self.sc_bh.get()) {
            bus_space_write_4(t, h, off, v);
        }
    }

    /// `bus_space_barrier(sc->sc_bt, sc->sc_bh, off, len, BUS_SPACE_BARRIER_WRITE)`.
    fn barrier_write(&self, off: BusSize, len: BusSize) {
        if let (Some(t), Some(h)) = (self.sc_bt.get(), self.sc_bh.get()) {
            bus_space_barrier(t, h, off, len, BUS_SPACE_BARRIER_WRITE);
        }
    }

    /// `sc->sc_dev.dv_xname`.
    fn xname(&self) -> &str {
        self.sc_dev.xname()
    }

    /// `sc->sc_acpi`, set by the attachment.
    fn acpi(&self) -> Option<&'static AcpiSoftc> {
        // SAFETY: null or acpi0's softc, which is never freed.
        unsafe { self.sc_acpi.get().as_ref() }
    }
}

// SAFETY: `#[repr(C)]` with the device first; every other member is valid as zero bits: no
// tag or handle, null pointers, an unborrowed `RefCell` of `None`, zeros.
unsafe impl Softc for TpmSoftc {}

/// One row of `tpm_devs[]`.
struct TpmDev {
    /// `devid`: `TPM_ID`.
    devid: u32,
    /// `name`.
    name: &'static str,
}

/// `tpm_devs[]`: the TIS chips the attachment names (the C's `{ 0, "" }` end marker is the
/// slice's end).
static TPM_DEVS: [TpmDev; 8] = [
    TpmDev {
        devid: 0x000615d1,
        name: "Infineon SLD9630 1.1",
    },
    TpmDev {
        devid: 0x000b15d1,
        name: "Infineon SLB9635 1.2",
    },
    TpmDev {
        devid: 0x100214e4,
        name: "Broadcom BCM0102",
    },
    TpmDev {
        devid: 0x00fe1050,
        name: "WEC WPCT200",
    },
    TpmDev {
        devid: 0x687119fa,
        name: "SNS SSX35",
    },
    TpmDev {
        devid: 0x2e4d5453,
        name: "STM ST19WP18",
    },
    TpmDev {
        devid: 0x32021114,
        name: "Atmel 97SC3203",
    },
    TpmDev {
        devid: 0x10408086,
        name: "Intel INTC0102",
    },
];

/// `tpm_ca`.
pub static TPM_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<TpmSoftc>(),
    ca_match: Some(tpm_match),
    ca_attach: tpm_attach,
    ca_detach: None,
    ca_activate: Some(tpm_activate),
};

/// `tpm_cd`.
pub static TPM_CD: Cfdriver = Cfdriver::new(b"tpm", DV_DULL, CD_SKIPHIBERNATE); // XXX

/// `tpm_hids[]`.
pub static TPM_HIDS: [&str; 8] = [
    "PNP0C31", "ATM1200", "IFX0102", "BCM0101", "BCM0102", "NSC1200", "ICO0102", "MSFT0101",
];

/// `tpm_match(parent, match, aux)`: a device of `tpm_hids` with a register window.
pub fn tpm_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: acpi0 hands its children `struct acpi_attach_args`.
    let aa = unsafe { &*aux.cast_const().cast::<AcpiAttachArgs>() };

    if aa.aaa_naddr < 1 {
        return 0;
    }
    acpi_matchhids(aa, &TPM_HIDS, "tpm")
}

/// `tpm_attach(parent, self, aux)`: tells TIS from CRB, checks `_STA`, maps the registers
/// and initialises the interface.
pub fn tpm_attach(parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `tpm_ca` makes `TpmSoftc`s, never detached: the softc lives as long as the
    // kernel.
    let sc: &'static TpmSoftc = unsafe { &*ptr::from_ref(self_.softc::<TpmSoftc>()) };
    // SAFETY: as in `tpm_match`.
    let aaa = unsafe { &*aux.cast_const().cast::<AcpiAttachArgs>() };
    let Some(parent) = parent else {
        return;
    };
    // SAFETY: tpm attaches at acpi0 only (`tpm* at acpi?`), whose softc is an `AcpiSoftc`.
    let acpi: &'static AcpiSoftc = unsafe { &*ptr::from_ref(parent.softc::<AcpiSoftc>()) };

    sc.sc_acpi.set(ptr::from_ref(acpi));
    *sc.sc_devnode.borrow_mut() = aaa.aaa_node.clone();
    sc.sc_enabled.set(0);
    sc.sc_tpm_mode.set(TPM_TIS);

    if let Some(node) = aaa.aaa_node.as_ref() {
        printf(format_args!(" {}", Str(cstr(&node.name))));
    }

    if acpi_matchhids(aaa, &["MSFT0101"], "tpm") != 0 {
        sc.sc_tpm20.set(1);
        // Identify if using 1.2 TIS or 2.0's CRB methods
        let start_method = tpm2_start_method(acpi);
        match start_method {
            TPM2_START_METHOD_TIS => {} // Already default
            TPM2_START_METHOD_CRB => sc.sc_tpm_mode.set(TPM_CRB),
            _ => {
                printf(format_args!(
                    ": unsupported TPM2 start method {}\n",
                    start_method as i32
                ));
                return;
            }
        }
    }

    printf(format_args!(
        " {} ({})",
        if sc.sc_tpm20.get() != 0 { "2.0" } else { "1.2" },
        if sc.sc_tpm_mode.get() == TPM_TIS {
            "TIS"
        } else {
            "CRB"
        }
    ));

    let sta = acpi_getsta(acpi, aaa.aaa_node.as_ref());
    let want = i64::from(STA_PRESENT | STA_ENABLED | STA_DEV_OK);
    if sta & want != want {
        printf(format_args!(": not enabled\n"));
        return;
    }

    printf(format_args!(
        " addr 0x{:x}/0x{:x}",
        aaa.aaa_addr[0], aaa.aaa_size[0]
    ));
    sc.sc_bbase.set(aaa.aaa_addr[0] as BusSize);

    let Some(bt) = aaa.aaa_bst[0] else {
        printf(format_args!(": can't map registers\n"));
        return;
    };
    sc.sc_bt.set(Some(bt));
    // SAFETY: the TPM's register window from its `_CRS`, which only this driver drives.
    match unsafe {
        bus_space_map(
            bt,
            aaa.aaa_addr[0] as BusAddr,
            aaa.aaa_size[0] as BusSize,
            0,
        )
    } {
        Ok(bh) => sc.sc_bh.set(Some(bh)),
        Err(_) => {
            printf(format_args!(": can't map registers\n"));
            return;
        }
    }

    if sc.sc_tpm_mode.get() == TPM_TIS {
        if !tpm_probe(sc) {
            printf(format_args!(": probe failed\n"));
            return;
        }

        if tpm_init_tis(sc).is_err() {
            printf(format_args!(": init failed\n"));
            return;
        }
    } else if tpm_init_crb(sc).is_err() {
        printf(format_args!(": init failed\n"));
        return;
    }

    printf(format_args!("\n"));
    sc.sc_enabled.set(1);
}

/// `tpm_activate(self, act)`: saves the TPM's state on suspend; nothing on wakeup.
pub fn tpm_activate(self_: &Device, act: i32) -> Result<(), Errno> {
    // SAFETY: `tpm_ca`'s softc is a `TpmSoftc` (`ca_devsize`).
    let sc: &TpmSoftc = unsafe { self_.softc() };

    match act {
        DVACT_SUSPEND => {
            if sc.sc_enabled.get() == 0 {
                // DPRINTF: suspend, but not enabled
                return Ok(());
            }
            tpm_suspend(sc);
        }
        DVACT_WAKEUP => {
            if sc.sc_enabled.get() == 0 {
                // DPRINTF: wakeup, but not enabled
                return Ok(());
            }
            tpm_resume(sc);
        }
        _ => {}
    }

    Ok(())
}

/// The command `tpm_suspend` sends: `TPM_ORD_SaveState` to a 1.2 chip,
/// `TPM2_Shutdown(TPM_SU_STATE)` to a 2.0 one.
pub fn tpm_suspend_command(tpm20: bool) -> &'static [u8] {
    const COMMAND1: [u8; 10] = [
        0, 0xc1, // TPM_TAG_RQU_COMMAND
        0, 0, 0, 10, // Length in bytes
        0, 0, 0, 0x98, // TPM_ORD_SaveStates
    ];
    const COMMAND2: [u8; 12] = [
        0x80, 0x01, // TPM_ST_COMMAND_TAG
        0, 0, 0, 12, // Length in bytes
        0, 0, 0x01, 0x45, // TPM_CC_Shutdown
        0x00, 0x01,
    ];
    if tpm20 { &COMMAND2 } else { &COMMAND1 }
}

/// `tpm_suspend(sc)`: when acpi0 is leaving S0, tells the chip to save its state so the
/// firmware can restore it on resume, and reads the response header back.
pub fn tpm_suspend(sc: &TpmSoftc) -> i32 {
    let Some(acpi) = sc.acpi() else {
        return 0;
    };
    if acpi.sc_state.get() == i32::from(ACPI_STATE_S0) {
        return 0;
    }

    // DPRINTF: saving state preparing for suspend

    let mut command = [0u8; 12];
    let cmd = tpm_suspend_command(sc.sc_tpm20.get() != 0);
    let commandlen = cmd.len();
    command[..commandlen].copy_from_slice(cmd);
    let command = &mut command[..commandlen];

    // Tell the chip to save its state so the BIOS can then restore it upon resume.
    if sc.sc_tpm_mode.get() == TPM_TIS {
        let _ = tpm_write_tis(sc, command);
        command.fill(0);
        let _ = tpm_read_tis(sc, command, TPM_HDRSIZE as i32);
    } else {
        let _ = tpm_write_crb(sc, command);
        command.fill(0);
        let _ = tpm_read_crb(sc, command);
    }
    0
}

/// `tpm_resume(sc)`: nothing. The firmware should have restored the chip's state already,
/// but the chip should be told to self-test here (according to the Linux driver).
pub fn tpm_resume(_sc: &TpmSoftc) -> i32 {
    // DPRINTF: resume
    0
}

/// The start method of a `TPM2` table's bytes, `None` for another table.
pub fn tpm2_table_start_method(table: &[u8]) -> Option<u32> {
    if !table.starts_with(TPM2_SIG) || table.len() < size_of::<AcpiTpm2>() {
        return None;
    }
    // SAFETY: the table holds a whole `struct acpi_tpm2` (checked above); it is packed
    // integers, read unaligned.
    let tpm2 = unsafe { ptr::read_unaligned(table.as_ptr().cast::<AcpiTpm2>()) };
    Some(tpm2.start_method)
}

/// `tpm2_start_method(sc)`: the `TPM2` table's start method, 0 when there is no table.
pub fn tpm2_start_method(sc: &AcpiSoftc) -> u32 {
    for entry in sc.sc_tables.iter() {
        if let Some(method) = tpm2_table_start_method(q_table_bytes(entry)) {
            return method;
        }
    }

    // DPRINTF: ", no TPM2 table"
    0
}

/// `tpm_probe(bt, bh)`: waits for the access register to be valid, then checks that the
/// capability register reads as something. As in C a timeout prints a message but still
/// counts as found; only an all-ones capability register fails.
pub fn tpm_probe(sc: &TpmSoftc) -> bool {
    let mut tries = 10000;

    // wait for chip to settle
    loop {
        let t = tries;
        tries -= 1;
        if t == 0 {
            break;
        }
        if sc.read_1(TPM_ACCESS) & TPM_ACCESS_VALID != 0 {
            break;
        } else if tries == 0 {
            printf(format_args!(": timed out waiting for validity\n"));
            return true;
        }

        delay(10);
    }

    sc.read_4(TPM_INTF_CAPABILITIES) != 0xffff_ffff
}

/// Whether a TIS capability register has what the driver needs: `TPM_CAPSREQ` and rising
/// edge or low level interrupts.
pub fn tpm_caps_ok(r: u32) -> bool {
    r & TPM_CAPSREQ == TPM_CAPSREQ && r & (TPM_INTF_INT_EDGE_RISING | TPM_INTF_INT_LEVEL_LOW) != 0
}

/// The TIS interrupt enable register that acknowledges every interrupt and disables them
/// (the driver polls).
pub fn tpm_tis_intmask(intmask: u32) -> u32 {
    (intmask
        | TPM_INTF_CMD_READY_INT
        | TPM_INTF_LOCALITY_CHANGE_INT
        | TPM_INTF_DATA_AVAIL_INT
        | TPM_INTF_STS_VALID_INT)
        & !TPM_GLOBAL_INT_ENABLE
}

/// The name `tpm_devs[]` gives a TIS chip id.
pub fn tpm_devname(devid: u32) -> Option<&'static str> {
    TPM_DEVS.iter().find(|d| d.devid == devid).map(|d| d.name)
}

/// `printf(", %s rev 0x%x")` or `printf(", device 0x%08x rev 0x%x")`: names the chip.
fn tpm_print_dev(sc: &TpmSoftc) {
    let _ = match tpm_devname(sc.sc_devid.get()) {
        Some(name) => printf(format_args!(", {} rev 0x{:x}", name, sc.sc_rev.get())),
        None => printf(format_args!(
            ", device 0x{:08x} rev 0x{:x}",
            sc.sc_devid.get(),
            sc.sc_rev.get()
        )),
    };
}

/// `tpm_init_tis(sc)`: checks the capabilities, disables the interrupts, takes locality 0
/// and names the chip. Capabilities too low end it early but successfully, as in C.
pub fn tpm_init_tis(sc: &TpmSoftc) -> Result<(), Errno> {
    let r = sc.read_4(TPM_INTF_CAPABILITIES);
    if !tpm_caps_ok(r) {
        // DPRINTF: ": caps too low (caps=%b)\n"
        return Ok(());
    }

    // ack and disable all interrupts, we'll be using polling only
    let intmask = tpm_tis_intmask(sc.read_4(TPM_INTERRUPT_ENABLE));
    sc.write_4(TPM_INTERRUPT_ENABLE, intmask);

    if let Err(e) = tpm_request_locality_tis(sc, 0) {
        printf(format_args!(", requesting locality failed\n"));
        return Err(e);
    }

    sc.sc_devid.set(sc.read_4(TPM_ID));
    sc.sc_rev.set(u32::from(sc.read_1(TPM_REV)));

    tpm_print_dev(sc);

    Ok(())
}

/// A CRB buffer's offset in the register window: its 64-bit physical address (low and high
/// registers) less the window's base.
pub fn tpm_crb_offset(laddr: u32, haddr: u32, bbase: BusSize) -> BusSize {
    let addr = u64::from(laddr) | (u64::from(haddr) << 32);
    (addr as BusSize).wrapping_sub(bbase)
}

/// `tpm_init_crb(sc)`: takes locality 0, disables the interrupts, finds the command and
/// response buffers, gives the locality back and names the chip.
pub fn tpm_init_crb(sc: &TpmSoftc) -> Result<(), Errno> {
    if let Err(e) = tpm_request_locality_crb(sc, 0) {
        printf(format_args!(", request locality failed\n"));
        return Err(e);
    }

    // ack and disable all interrupts, we'll be using polling only
    let intmask = sc.read_4(TPM_INTERRUPT_ENABLE) & !TPM_CRB_INT_ENABLED_BIT;
    sc.write_4(TPM_INTERRUPT_ENABLE, intmask);

    // Identify command and response registers and sizes
    let bbase = sc.sc_bbase.get();
    sc.sc_cmd_off.set(tpm_crb_offset(
        sc.read_4(TPM_CRB_CTRL_CMD_LADDR),
        sc.read_4(TPM_CRB_CTRL_CMD_HADDR),
        bbase,
    ));
    sc.sc_cmd_sz.set(sc.read_4(TPM_CRB_CTRL_CMD_SIZE) as usize);
    sc.sc_rsp_off.set(tpm_crb_offset(
        sc.read_4(TPM_CRB_CTRL_RSP_LADDR),
        sc.read_4(TPM_CRB_CTRL_RSP_HADDR),
        bbase,
    ));
    sc.sc_rsp_sz.set(sc.read_4(TPM_CRB_CTRL_RSP_SIZE) as usize);

    // DPRINTF: ", cmd @ 0x%lx, %ld, rsp @ 0x%lx, %ld"

    tpm_release_locality_crb(sc);

    // If it's a unified buffer, the sizes must be the same.
    if sc.sc_cmd_off.get() == sc.sc_rsp_off.get() && sc.sc_cmd_sz.get() != sc.sc_rsp_sz.get() {
        printf(format_args!(", invalid buffer sizes\n"));
        return Err(Errno::EINVAL);
    }

    tpm_print_dev(sc);

    Ok(())
}

/// Whether a TIS access register says the locality is ours.
pub fn tpm_access_active(access: u8) -> bool {
    access & (TPM_ACCESS_VALID | TPM_ACCESS_ACTIVE_LOCALITY)
        == TPM_ACCESS_VALID | TPM_ACCESS_ACTIVE_LOCALITY
}

/// `tpm_request_locality_tis(sc, l)`: takes locality `l` (only 0) and waits up to
/// `TPM_ACCESS_TMO` ms for it.
pub fn tpm_request_locality_tis(sc: &TpmSoftc, l: i32) -> Result<(), Errno> {
    if l != 0 {
        return Err(Errno::EINVAL);
    }

    if tpm_access_active(sc.read_1(TPM_ACCESS)) {
        return Ok(());
    }

    sc.write_1(TPM_ACCESS, TPM_ACCESS_REQUEST_USE);

    let mut to = TPM_ACCESS_TMO * 100; // steps of 10 microseconds

    let mut r;
    loop {
        r = sc.read_1(TPM_ACCESS);
        if tpm_access_active(r) {
            break;
        }
        let t = to;
        to -= 1;
        if t == 0 {
            break;
        }
        delay(10);
    }

    if !tpm_access_active(r) {
        // DPRINTF: "%s: %s: access %b\n"
        return Err(Errno::EBUSY);
    }

    Ok(())
}

/// Whether a CRB locality state register says the locality is assigned.
pub fn tpm_crb_loc_assigned(state: u32) -> bool {
    let mask = TPM_CRB_LOC_STATE_ASSIGNED | TPM_CRB_LOC_VALID;
    state & mask == mask
}

/// `tpm_request_locality_crb(sc, l)`: requests locality `l` (only 0) and waits for it to
/// be assigned.
pub fn tpm_request_locality_crb(sc: &TpmSoftc, l: i32) -> Result<(), Errno> {
    if l != 0 {
        return Err(Errno::EINVAL);
    }

    let r = sc.read_4(TPM_CRB_LOC_CTRL);
    sc.write_4(TPM_CRB_LOC_CTRL, r | TPM_CRB_LOC_REQUEST);

    let mut to = TPM_ACCESS_TMO * 200;

    let mut r = sc.read_4(TPM_CRB_LOC_STATE);
    while !tpm_crb_loc_assigned(r) {
        let t = to;
        to -= 1;
        if t == 0 {
            break;
        }
        delay(10);
        r = sc.read_4(TPM_CRB_LOC_STATE);
    }

    if !tpm_crb_loc_assigned(r) {
        printf(format_args!(", CRB loc FAILED"));
        return Err(Errno::EBUSY);
    }

    Ok(())
}

/// `tpm_release_locality_tis(sc)`: gives locality 0 up when another one waits for it.
pub fn tpm_release_locality_tis(sc: &TpmSoftc) {
    let want = TPM_ACCESS_REQUEST_PENDING | TPM_ACCESS_VALID;
    if sc.read_1(TPM_ACCESS) & want == want {
        // DPRINTF: releasing locality
        sc.write_1(TPM_ACCESS, TPM_ACCESS_ACTIVE_LOCALITY);
    }
}

/// `tpm_release_locality_crb(sc)`.
pub fn tpm_release_locality_crb(sc: &TpmSoftc) {
    let r = sc.read_4(TPM_CRB_LOC_CTRL);
    sc.write_4(TPM_CRB_LOC_CTRL, r | TPM_CRB_LOC_RELEASE);
}

/// The burst count of the status register's bytes 1 and 2.
pub fn tpm_burst_count(sts1: u8, sts2: u8) -> i32 {
    i32::from(sts1) | (i32::from(sts2) << 8)
}

/// `tpm_getburst(sc)`: how many bytes the FIFO takes or gives without waiting, polled up to
/// `TPM_BURST_TMO` ms; 0 on timeout.
pub fn tpm_getburst(sc: &TpmSoftc) -> i32 {
    let mut to = TPM_BURST_TMO * 100; // steps of 10 microseconds

    loop {
        let t = to;
        to -= 1;
        if t == 0 {
            break;
        }
        // Burst count has to be read from bits 8 to 23 without touching any other bits,
        // eg. the actual status bits 0 to 7.
        let burst = tpm_burst_count(sc.read_1(TPM_STS + 1), sc.read_1(TPM_STS + 2));
        if burst != 0 {
            return burst;
        }

        delay(10);
    }

    // DPRINTF: getburst timed out

    0
}

/// `tpm_status(sc)`: the status bits of `TPM_STS`.
pub fn tpm_status(sc: &TpmSoftc) -> u8 {
    sc.read_1(TPM_STS) & TPM_STS_MASK as u8
}

/// `tpm_waitfor(sc, offset, mask, val, msecs)`: polls a 32-bit register every microsecond
/// until `(r & mask) == val`, for up to `msecs` ms.
pub fn tpm_waitfor(
    sc: &TpmSoftc,
    offset: BusSize,
    mask: u32,
    val: u32,
    msecs: i32,
) -> Result<(), Errno> {
    let mut usecs = msecs * 1000;

    if sc.read_4(offset) & mask == val {
        return Ok(());
    }

    while usecs > 0 {
        if sc.read_4(offset) & mask == val {
            return Ok(());
        }
        delay(1);
        usecs -= 1;
    }

    // DPRINTF: "%s: %s: timed out, status 0x%x != 0x%x\n"
    Err(Errno::ETIMEDOUT)
}

/// `tpm_waitfor_status(sc, mask, msecs)`: polls the status bits every microsecond until
/// all of `mask` are set, for up to `msecs` ms.
pub fn tpm_waitfor_status(sc: &TpmSoftc, mask: u8, msecs: i32) -> Result<(), Errno> {
    let mut usecs = msecs * 1000;

    while tpm_status(sc) & mask != mask {
        if usecs == 0 {
            // DPRINTF: "%s: %s: timed out, status 0x%x != 0x%x\n"
            return Err(Errno::ETIMEDOUT);
        }

        usecs -= 1;
        delay(1);
    }

    Ok(())
}

/// Whether `tpm_read_tis` stops after a burst: without `TPM_PARAM_SIZE` it reads a header's
/// worth (6 bytes: tag and size) and whatever the burst gave with it.
pub fn tpm_read_tis_done(flags: i32, cnt: usize) -> bool {
    flags & TPM_PARAM_SIZE == 0 && cnt >= 6
}

/// `tpm_read_tis(sc, buf, len, &count, flags)`: reads the response from the FIFO into
/// `buf`, burst by burst; returns how many bytes it read.
pub fn tpm_read_tis(sc: &TpmSoftc, buf: &mut [u8], flags: i32) -> Result<usize, Errno> {
    let mut len = buf.len();
    let mut cnt = 0;

    // DPRINTF: "%s: %s %d:"

    while len > 0 {
        tpm_waitfor_status(sc, TPM_STS_DATA_AVAIL | TPM_STS_VALID, TPM_READ_TMO)?;

        let bcnt = tpm_getburst(sc).max(0) as usize;
        let n = len.min(bcnt);

        for _ in 0..n {
            buf[cnt] = sc.read_1(TPM_DATA);
            cnt += 1;
            len -= 1;
        }

        if tpm_read_tis_done(flags, cnt) {
            break;
        }
    }

    Ok(cnt)
}

/// The size (bytes 2-5) and the response code (bytes 6-9) of a response header.
pub fn tpm_rsp_header(hdr: &[u8; TPM_HDRSIZE]) -> (u32, u32) {
    let sz = u32::from_be_bytes([hdr[2], hdr[3], hdr[4], hdr[5]]);
    let rc = u32::from_be_bytes([hdr[6], hdr[7], hdr[8], hdr[9]]);
    (sz, rc)
}

/// Whether a CRB response size is possible: at least a header, at most the buffer.
pub fn tpm_crb_rsp_size_ok(sz: u32, rsp_sz: usize) -> bool {
    sz as usize >= TPM_HDRSIZE && sz as usize <= rsp_sz
}

/// `tpm_read_crb(sc, buf, len)`: reads the response header from the CRB response buffer,
/// checks its size and code, sends the TPM idle and gives the locality back.
pub fn tpm_read_crb(sc: &TpmSoftc, buf: &mut [u8]) -> Result<(), Errno> {
    let len = buf.len();

    // DPRINTF: "%s: %s %d:"

    if len < TPM_HDRSIZE {
        printf(format_args!(
            "{}: tpm_read_crb buf len too small\n",
            sc.xname()
        ));
        return Err(Errno::EINVAL);
    }

    let mut hdr = [0u8; TPM_HDRSIZE];
    for (count, b) in hdr.iter_mut().enumerate() {
        *b = sc.read_1(sc.sc_rsp_off.get() + count);
    }
    buf[..TPM_HDRSIZE].copy_from_slice(&hdr);

    // Response length is bytes 2-5 in the response header.
    let (sz, rc) = tpm_rsp_header(&hdr);
    if !tpm_crb_rsp_size_ok(sz, sc.sc_rsp_sz.get()) {
        printf(format_args!(
            "{}: invalid response size {}\n",
            sc.xname(),
            sz as i32
        ));
        return Err(Errno::EIO);
    }
    if sz as usize > len {
        printf(format_args!(
            "{}: response size too large, truncated to {}\n",
            sc.xname(),
            len
        ));
    }

    // Response code is bytes 6-9.
    if rc != TPM2_RC_SUCCESS {
        printf(format_args!(
            "{}: command failed (0x{:04x})\n",
            sc.xname(),
            rc
        ));
        // Nothing we can do on failure. Still try to idle the tpm.
    }

    // Tell the device to go idle.
    let r = sc.read_4(TPM_CRB_CTRL_REQ);
    sc.write_4(TPM_CRB_CTRL_REQ, r | TPM_CRB_CTRL_REQ_GO_IDLE);

    let mask = TPM_CRB_CTRL_STS_IDLE_BIT;
    if tpm_waitfor(sc, TPM_CRB_CTRL_STS, mask, mask, 200).is_err() {
        printf(format_args!(
            "{}: failed to transition to idle state after read\n",
            sc.xname()
        ));
    }

    tpm_release_locality_crb(sc);

    // DPRINTF: "%s: %s completed\n"
    Ok(())
}

/// `tpm_write_tis(sc, buf, len)`: takes locality 0, makes the TPM ready (aborting what it
/// did), writes the command into the FIFO burst by burst, checks that the TPM expects no
/// more and starts it (`TPM_STS_GO`).
pub fn tpm_write_tis(sc: &TpmSoftc, buf: &[u8]) -> Result<(), Errno> {
    let len = buf.len();

    tpm_request_locality_tis(sc, 0)?;

    // DPRINTF: the command bytes

    // read status
    let status = tpm_status(sc);
    if status & TPM_STS_CMD_READY == 0 {
        // abort!
        sc.write_1(TPM_STS, TPM_STS_CMD_READY);
        // DPRINTF on failure: failed waiting for ready after abort
        tpm_waitfor_status(sc, TPM_STS_CMD_READY, TPM_READ_TMO)?;
    }

    let mut count = 0;
    while count + 1 < len {
        let mut r = tpm_getburst(sc);
        while r > 0 && count + 1 < len {
            sc.write_1(TPM_DATA, buf[count]);
            count += 1;
            r -= 1;
        }
        // DPRINTF on failure: failed waiting for next byte
        tpm_waitfor_status(sc, TPM_STS_VALID | TPM_STS_DATA_EXPECT, TPM_READ_TMO)?;
    }

    if let Some(&last) = buf.get(count) {
        sc.write_1(TPM_DATA, last);
    }

    // DPRINTF on failure: failed after last byte
    tpm_waitfor_status(sc, TPM_STS_VALID, TPM_READ_TMO)?;

    if tpm_status(sc) & TPM_STS_DATA_EXPECT != 0 {
        // DPRINTF: final status still expecting data
        return Err(Errno::EIO);
    }

    // DPRINTF: final status after write

    // XXX: are we ever sending non-command data?
    sc.write_1(TPM_STS, TPM_STS_GO);

    Ok(())
}

/// `tpm_write_crb(sc, buf, len)`: takes locality 0, clears a cancellation, moves the TPM
/// to idle (if needed) and then to ready, writes the command into the command buffer,
/// starts it and waits for the TPM to take it.
pub fn tpm_write_crb(sc: &TpmSoftc, buf: &[u8]) -> Result<(), Errno> {
    let len = buf.len();

    if len > sc.sc_cmd_sz.get() {
        printf(format_args!(
            "{}: requested write length larger than cmd buffer\n",
            sc.xname()
        ));
        return Err(Errno::EINVAL);
    }

    if sc.read_4(TPM_CRB_CTRL_STS) & TPM_CRB_CTRL_STS_ERR_BIT != 0 {
        printf(format_args!("{}: device error bit set\n", sc.xname()));
        return Err(Errno::EIO);
    }

    if tpm_request_locality_crb(sc, 0).is_err() {
        printf(format_args!("{}: failed to acquire locality\n", sc.xname()));
        return Err(Errno::EIO);
    }

    // Clear cancellation bit
    sc.write_4(TPM_CRB_CTRL_CANCEL, TPM_CRB_CTRL_CANCEL_CLEAR);

    // Toggle to idle state (if needed) and then to ready
    let r = sc.read_4(TPM_CRB_CTRL_STS);
    if r & TPM_CRB_CTRL_STS_IDLE_BIT == 0 {
        printf(format_args!("{}: asking device to idle\n", sc.xname()));
        let r = sc.read_4(TPM_CRB_CTRL_REQ);
        sc.write_4(TPM_CRB_CTRL_REQ, r | TPM_CRB_CTRL_REQ_GO_IDLE);

        let mask = TPM_CRB_CTRL_STS_IDLE_BIT;
        if tpm_waitfor(sc, TPM_CRB_CTRL_STS, mask, mask, 200).is_err() {
            printf(format_args!(
                "{}: failed to transition to idle state before write\n",
                sc.xname()
            ));
            return Err(Errno::EIO);
        }
    }
    let r = sc.read_4(TPM_CRB_CTRL_REQ);
    sc.write_4(TPM_CRB_CTRL_REQ, r | TPM_CRB_CTRL_REQ_GO_READY);
    // As in C, the value waited for is `!mask`, 0: the status register's bit 0 (its error
    // bit) clear.
    let mask = TPM_CRB_CTRL_REQ_GO_READY;
    if tpm_waitfor(sc, TPM_CRB_CTRL_STS, mask, 0, 200).is_err() {
        printf(format_args!(
            "{}: failed to transition to ready state\n",
            sc.xname()
        ));
        return Err(Errno::EIO);
    }

    // Write the command
    for (count, &b) in buf.iter().enumerate() {
        sc.write_1(sc.sc_cmd_off.get() + count, b);
    }
    sc.barrier_write(sc.sc_cmd_off.get(), len);
    // DPRINTF: "%s: %s wrote %lu bytes\n"

    // Send the Start Command request
    sc.write_4(TPM_CRB_CTRL_START, TPM_CRB_CTRL_START_CMD);
    sc.barrier_write(TPM_CRB_CTRL_START, 4);

    // Check if command was processed: the start register reads 0 again (`~mask`).
    if tpm_waitfor(sc, TPM_CRB_CTRL_START, !0, 0, 200).is_err() {
        printf(format_args!(
            "{}: timeout waiting for device to process command\n",
            sc.xname()
        ));
        return Err(Errno::EIO);
    }

    Ok(())
}

/// `TPM2_SelfTest(fullTest = YES)`: the command [`tpm_selftest`] sends (TPM 2.0 Part 3,
/// 10.2): tag `TPM_ST_NO_SESSIONS`, 11 bytes, `TPM_CC_SelfTest`.
pub const TPM2_SELFTEST_COMMAND: [u8; 11] = [
    0x80, 0x01, // TPM_ST_NO_SESSIONS
    0, 0, 0, 11, // Length in bytes
    0, 0, 0x01, 0x43, // TPM_CC_SelfTest
    0x01, // fullTest: YES
];

/// `selftest=tpm` (feature `qemu`, test plumbing, not in the C): sends
/// [`TPM2_SELFTEST_COMMAND`] to tpm0 through `tpm_write_tis`/`tpm_read_tis` or
/// `tpm_write_crb`/`tpm_read_crb`, the path `tpm_suspend` uses, and prints the response
/// header's size and code, which come from the TPM behind the interface (swtpm under
/// QEMU), not from the emulated registers.
#[cfg(feature = "qemu")]
pub fn tpm_selftest() {
    let Some(dv) = TPM_CD.cd_dev(0) else {
        printf(format_args!("selftest: tpm: no tpm0\n"));
        return;
    };
    // SAFETY: an attached unit of `tpm_cd`, never detached; its softc is a `TpmSoftc`.
    let sc: &TpmSoftc = unsafe { dv.as_ref().softc() };
    if sc.sc_enabled.get() == 0 {
        printf(format_args!("selftest: tpm: {} not enabled\n", sc.xname()));
        return;
    }

    let mut rsp = [0u8; TPM_HDRSIZE];
    let res = if sc.sc_tpm_mode.get() == TPM_TIS {
        tpm_write_tis(sc, &TPM2_SELFTEST_COMMAND)
            .and_then(|()| tpm_read_tis(sc, &mut rsp, TPM_HDRSIZE as i32).map(|_| ()))
    } else {
        tpm_write_crb(sc, &TPM2_SELFTEST_COMMAND).and_then(|()| tpm_read_crb(sc, &mut rsp))
    };
    if let Err(e) = res {
        printf(format_args!(
            "selftest: tpm: {}: TPM2_SelfTest failed: {:?}\n",
            sc.xname(),
            e
        ));
        return;
    }
    let (sz, rc) = tpm_rsp_header(&rsp);
    printf(format_args!(
        "selftest: tpm: {}: TPM2_SelfTest answered {} bytes, rc 0x{:x}\n",
        sc.xname(),
        sz,
        rc
    ));
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_bits_and_burst() {
        // Bit numbers in the %b strings match the masks (1-based).
        assert_eq!(TPM_STS_VALID, 1 << (8 - 1));
        assert_eq!(TPM_STS_CMD_READY, 1 << (7 - 1));
        assert_eq!(TPM_STS_GO, 1 << (6 - 1));
        assert_eq!(TPM_STS_DATA_AVAIL, 1 << (5 - 1));
        assert_eq!(TPM_STS_DATA_EXPECT, 1 << (4 - 1));
        assert_eq!(TPM_STS_RESP_RETRY, 1 << (2 - 1));
        assert_eq!(TPM_STS_MASK | TPM_STS_BMASK, 0x00ff_ffff);
        // The burst count is bytes 1 and 2 of TPM_STS, little endian.
        assert_eq!(tpm_burst_count(0x0a, 0x00), 10);
        assert_eq!(tpm_burst_count(0x00, 0x01), 256);
        assert_eq!(tpm_burst_count(0xff, 0xff), 0xffff);
        assert_eq!(tpm_burst_count(0, 0), 0);
    }

    #[test]
    fn access_register() {
        assert!(tpm_access_active(0xa1)); // VALID | ACT | EST
        assert!(!tpm_access_active(0x80)); // VALID only
        assert!(!tpm_access_active(0x20)); // ACT, but not valid
        assert_eq!(TPM_ACCESS_BITS[0], 16); // %b base
    }

    #[test]
    fn capabilities() {
        // QEMU's TIS 2.0 capabilities: every interrupt kind, burst count dynamic.
        assert!(tpm_caps_ok(0x3000_06ff | TPM_INTF_INT_LEVEL_LOW));
        assert!(tpm_caps_ok(TPM_CAPSREQ));
        assert!(tpm_caps_ok(
            TPM_INTF_DATA_AVAIL_INT | TPM_INTF_LOCALITY_CHANGE_INT | TPM_INTF_INT_LEVEL_LOW
        ));
        // Missing the locality change interrupt.
        assert!(!tpm_caps_ok(
            TPM_INTF_DATA_AVAIL_INT | TPM_INTF_INT_LEVEL_LOW | TPM_INTF_INT_EDGE_RISING
        ));
        assert!(!tpm_caps_ok(0));
    }

    #[test]
    fn interrupt_masks() {
        assert_eq!(tpm_tis_intmask(TPM_GLOBAL_INT_ENABLE | 0x08), 0x8f);
        assert_eq!(tpm_tis_intmask(0), 0x87);
        assert_eq!(TPM_CRB_INT_ENABLED_BIT, 0x8000_0000);
    }

    #[test]
    fn device_names() {
        assert_eq!(tpm_devname(0x000b15d1), Some("Infineon SLB9635 1.2"));
        assert_eq!(tpm_devname(0x10408086), Some("Intel INTC0102"));
        // QEMU's TIS: IBM's vendor id 0x1014, device 1.
        assert_eq!(tpm_devname(0x0001_1014), None);
        assert_eq!(tpm_devname(0), None);
    }

    #[test]
    fn crb_registers() {
        assert!(tpm_crb_loc_assigned(0x82)); // VALID | ASSIGNED
        assert!(tpm_crb_loc_assigned(0x83));
        assert!(!tpm_crb_loc_assigned(0x80));
        assert!(!tpm_crb_loc_assigned(0x02));
        // QEMU's CRB at 0xfed40000: the buffer at 0xfed40080, offset 0x80.
        assert_eq!(
            tpm_crb_offset(0xfed4_0080, 0, 0xfed4_0000),
            TPM_CRB_DATA_BUFFER
        );
        assert_eq!(
            tpm_crb_offset(0x1000, 0x1, 0x1_0000_0000),
            0x1000 as BusSize
        );
    }

    #[test]
    fn suspend_commands() {
        let c1 = tpm_suspend_command(false);
        assert_eq!(c1.len(), 10);
        assert_eq!(&c1[..2], &[0x00, 0xc1]);
        assert_eq!(u32::from_be_bytes([c1[2], c1[3], c1[4], c1[5]]), 10);
        assert_eq!(u32::from_be_bytes([c1[6], c1[7], c1[8], c1[9]]), 0x98);

        let c2 = tpm_suspend_command(true);
        assert_eq!(c2.len(), 12);
        assert_eq!(&c2[..2], &[0x80, 0x01]);
        assert_eq!(u32::from_be_bytes([c2[2], c2[3], c2[4], c2[5]]), 12);
        assert_eq!(u32::from_be_bytes([c2[6], c2[7], c2[8], c2[9]]), 0x145);
        assert_eq!(&c2[10..], &[0x00, 0x01]); // TPM_SU_STATE

        let st = TPM2_SELFTEST_COMMAND;
        assert_eq!(
            u32::from_be_bytes([st[2], st[3], st[4], st[5]]) as usize,
            st.len()
        );
        assert_eq!(u32::from_be_bytes([st[6], st[7], st[8], st[9]]), 0x143);
    }

    #[test]
    fn response_framing() {
        // TPM2_SelfTest's answer: TPM_ST_NO_SESSIONS, 10 bytes, TPM_RC_SUCCESS.
        let ok = [0x80, 0x01, 0, 0, 0, 10, 0, 0, 0, 0];
        assert_eq!(tpm_rsp_header(&ok), (10, TPM2_RC_SUCCESS));
        // TPM_RC_INITIALIZE: no TPM2_Startup yet.
        let init = [0x80, 0x01, 0, 0, 0, 10, 0, 0, 0x01, 0x00];
        assert_eq!(tpm_rsp_header(&init), (10, TPM2_RC_INITIALIZE));
        let big = [0x80, 0x01, 0x01, 0x02, 0x03, 0x04, 0x0a, 0x0b, 0x0c, 0x0d];
        assert_eq!(tpm_rsp_header(&big), (0x0102_0304, 0x0a0b_0c0d));

        assert!(tpm_crb_rsp_size_ok(10, 0xf80));
        assert!(tpm_crb_rsp_size_ok(0xf80, 0xf80));
        assert!(!tpm_crb_rsp_size_ok(9, 0xf80));
        assert!(!tpm_crb_rsp_size_ok(0xf81, 0xf80));
    }

    #[test]
    fn tis_read_stops_after_a_header() {
        assert!(!tpm_read_tis_done(TPM_HDRSIZE as i32, 5));
        assert!(tpm_read_tis_done(TPM_HDRSIZE as i32, 6));
        assert!(tpm_read_tis_done(0, 10));
        assert!(!tpm_read_tis_done(TPM_PARAM_SIZE, 10));
    }

    #[test]
    fn tpm2_table() {
        let mut t = [0u8; size_of::<AcpiTpm2>()];
        t[..4].copy_from_slice(TPM2_SIG);
        t[4..8].copy_from_slice(&(size_of::<AcpiTpm2>() as u32).to_le_bytes());
        // start_method after the header (36), reserved (4) and control_addr (8).
        t[48..52].copy_from_slice(&TPM2_START_METHOD_CRB.to_le_bytes());
        assert_eq!(tpm2_table_start_method(&t), Some(TPM2_START_METHOD_CRB));
        t[48..52].copy_from_slice(&TPM2_START_METHOD_TIS.to_le_bytes());
        assert_eq!(tpm2_table_start_method(&t), Some(TPM2_START_METHOD_TIS));
        assert_eq!(tpm2_table_start_method(&t[..40]), None);
        t[..4].copy_from_slice(b"HPET");
        assert_eq!(tpm2_table_start_method(&t), None);
    }

    #[test]
    fn unmapped_registers_read_as_ones() {
        // A zeroed softc has no tag or handle (autoconfiguration's state before the attach).
        let sc = core::mem::MaybeUninit::<TpmSoftc>::zeroed();
        // SAFETY: all-zero bits are a valid `TpmSoftc` (its `Softc` contract); the
        // `MaybeUninit` is never dropped.
        let sc = unsafe { sc.assume_init_ref() };
        assert_eq!(sc.read_1(TPM_ACCESS), 0xff);
        assert_eq!(sc.read_4(TPM_INTF_CAPABILITIES), 0xffff_ffff);
        assert!(!tpm_probe(sc));
        assert_eq!(tpm_request_locality_tis(sc, 1), Err(Errno::EINVAL));
        assert_eq!(tpm_request_locality_crb(sc, 1), Err(Errno::EINVAL));
    }
}
/* </TESTS> */
