/* <LICENSES> */
/*
 * Copyright (c) 1997, 1998, 1999, 2000-2003 Bill Paul <wpaul@windriver.com>
 * Copyright (c) 2003 Craig Boston
 * Copyright (c) 2004 Daniel Hartmeier
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
 *	This product includes software developed by Bill Paul.
 * 4. Neither the name of the author nor the names of any co-contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY Bill Paul AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL Bill Paul, THE VOICES IN HIS HEAD OR
 * THE CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL,
 * EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
 * PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS;
 * OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
 * WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR
 * OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF
 * ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! `<dev/usb/if_cdcereg.h>`: the cdce(4) softc, its transfer chains and the table entry type.
//!
//! Upstream: sys/dev/usb/if_cdcereg.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The softc's members the stack changes after attach are `Cell`s (the kernel lock at
//!   `splnet()`/`splusb()`, as in C); the USB handles (`cdce_udev`, `cdce_ctl_iface`,
//!   `cdce_data_iface`, the pipes, the xfers) are `Cell<Option<&'static T>>`
//!   (`docs/C_TO_RUST.md`, `usbd_device *`), `cdce_buf` the xfer's DMA buffer as a raw
//!   pointer, `cdce_mbuf` an `Option<&'static Mbuf>`, `cdce_sc` the softc as a raw pointer
//!   (the chain lives inside it).
//! - `GET_IFP(sc)` is [`CdceSoftc::get_ifp`]. The all-zero softc is valid (`config_make_softc`
//!   zeroes it): the C's `cdce_attached` and `cdce_flags` start at 0 as well.

use core::cell::Cell;

use crate::dev::usb::usbcdc::UsbCdcNotification;
use crate::dev::usb::usbdi::UsbDevno;
use crate::dev::usb::usbdivar::{UsbdDevice, UsbdInterface, UsbdPipe, UsbdXfer};
use crate::net::if_var::Ifnet;
use crate::netinet::if_ether::Arpcom;
use crate::sys::device::{Device, Softc};
use crate::sys::mbuf::Mbuf;

/// `CDCE_RX_LIST_CNT`.
pub const CDCE_RX_LIST_CNT: usize = 1;
/// `CDCE_TX_LIST_CNT`.
pub const CDCE_TX_LIST_CNT: usize = 1;
/// `CDCE_BUFSZ`.
pub const CDCE_BUFSZ: usize = 1542;

/// `CDCE_CRC32`: the device wants a 32-bit CRC appended to every frame.
pub const CDCE_CRC32: u16 = 1;
/// `CDCE_SWAPUNION`: the union descriptor's master and slave are swapped.
pub const CDCE_SWAPUNION: u16 = 2;

/// `struct cdce_type`: a device `cdce_devs[]` knows, with its quirks.
#[derive(Clone, Copy, Debug)]
pub struct CdceType {
    /// `cdce_dev`.
    pub cdce_dev: UsbDevno,
    /// `cdce_flags`: `CDCE_*`.
    pub cdce_flags: u16,
}

impl AsRef<UsbDevno> for CdceType {
    fn as_ref(&self) -> &UsbDevno {
        &self.cdce_dev
    }
}

/// `struct cdce_chain`: one transfer, its buffer and the mbuf it carries.
pub struct CdceChain {
    /// `cdce_sc`: the softc this chain belongs to (null until the list init).
    pub cdce_sc: Cell<*const CdceSoftc>,
    /// `cdce_xfer`.
    pub cdce_xfer: Cell<Option<&'static UsbdXfer>>,
    /// `cdce_buf`: the xfer's DMA buffer (`CDCE_BUFSZ` bytes).
    pub cdce_buf: Cell<*mut u8>,
    /// `cdce_mbuf`.
    pub cdce_mbuf: Cell<Option<&'static Mbuf>>,
    /// `cdce_accum`.
    pub cdce_accum: Cell<i32>,
    /// `cdce_idx`.
    pub cdce_idx: Cell<i32>,
}

/// `struct cdce_cdata`: the receive and transmit chains.
pub struct CdceCdata {
    /// `cdce_rx_chain`.
    pub cdce_rx_chain: [CdceChain; CDCE_RX_LIST_CNT],
    /// `cdce_tx_chain`.
    pub cdce_tx_chain: [CdceChain; CDCE_TX_LIST_CNT],
    /// `cdce_tx_prod`.
    pub cdce_tx_prod: Cell<i32>,
    /// `cdce_tx_cons`.
    pub cdce_tx_cons: Cell<i32>,
    /// `cdce_tx_cnt`.
    pub cdce_tx_cnt: Cell<i32>,
    /// `cdce_rx_prod`.
    pub cdce_rx_prod: Cell<i32>,
}

/// `struct cdce_softc`.
#[repr(C)]
pub struct CdceSoftc {
    /// `cdce_dev`: base device.
    pub cdce_dev: Device,
    /// `cdce_arpcom`: the Ethernet interface.
    pub cdce_arpcom: Arpcom,
    /// `cdce_udev`.
    pub cdce_udev: Cell<Option<&'static UsbdDevice>>,
    /// `cdce_ctl_iface`.
    pub cdce_ctl_iface: Cell<Option<&'static UsbdInterface>>,
    /// `cdce_intr_no`: the interrupt endpoint's address, -1 for none.
    pub cdce_intr_no: Cell<i32>,
    /// `cdce_intr_pipe`.
    pub cdce_intr_pipe: Cell<Option<&'static UsbdPipe>>,
    /// `cdce_intr_buf`: the notification the interrupt pipe reads into.
    pub cdce_intr_buf: Cell<UsbCdcNotification>,
    /// `cdce_intr_size`.
    pub cdce_intr_size: Cell<i32>,
    /// `cdce_data_iface`.
    pub cdce_data_iface: Cell<Option<&'static UsbdInterface>>,
    /// `cdce_bulkin_no`: the bulk-in endpoint's address, -1 for none.
    pub cdce_bulkin_no: Cell<i32>,
    /// `cdce_bulkin_pipe`.
    pub cdce_bulkin_pipe: Cell<Option<&'static UsbdPipe>>,
    /// `cdce_bulkout_no`: the bulk-out endpoint's address, -1 for none.
    pub cdce_bulkout_no: Cell<i32>,
    /// `cdce_bulkout_pipe`.
    pub cdce_bulkout_pipe: Cell<Option<&'static UsbdPipe>>,
    /// `cdce_cdata`.
    pub cdce_cdata: CdceCdata,
    /// `cdce_rxeof_errors`.
    pub cdce_rxeof_errors: Cell<i32>,
    /// `cdce_flags`: `CDCE_*`.
    pub cdce_flags: Cell<u16>,
    /// `cdce_attached`.
    pub cdce_attached: Cell<u8>,
}

impl CdceSoftc {
    /// `GET_IFP(sc)`.
    pub fn get_ifp(&self) -> &Ifnet {
        &self.cdce_arpcom.ac_if
    }
}

// SAFETY: `#[repr(C)]` with the device first; the arpcom is all-zero valid, and every other
// member is a `Cell` of an integer, a pointer, an `Option` of a reference or the notification
// (bytes), or a chain of such `Cell`s: all valid as zero bits.
unsafe impl Softc for CdceSoftc {}
/* </CODE> */
