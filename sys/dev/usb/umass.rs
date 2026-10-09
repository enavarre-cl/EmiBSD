/* <LICENSES> */
/*
 * Copyright (c) 2026 Emilio Navarrete Lineros <enavarre@outlook.com>
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

/*
 * Copyright (c) 2003 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Charles M. Hannum.
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

/*-
 * Copyright (c) 1999 MAEKAWA Masahide <bishop@rr.iij4u.or.jp>,
 *		      Nick Hibma <n_hibma@freebsd.org>
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
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *     $FreeBSD: src/sys/dev/usb/umass.c,v 1.13 2000/03/26 01:39:12 n_hibma Exp $
 */
/* </LICENSES> */

/* <CODE> */
//! umass(4): USB Mass Storage, over the SCSI midlayer.
//!
//! Upstream: sys/dev/usb/umass.c @ 3ce1f3f79392
//!
//! Universal Serial Bus Mass Storage Class specs:
//! <https://www.usb.org/sites/default/files/Mass_Storage_Specification_Overview_v1.4_2-19-2010.pdf>,
//! <https://www.usb.org/sites/default/files/usbmassbulk_10.pdf>,
//! <https://www.usb.org/sites/default/files/usb_msc_cbi_1.1.pdf>,
//! <https://www.usb.org/sites/default/files/usbmass-ufi10.pdf>.
//! Ported to NetBSD by Lennart Augustsson; parts of the code written by Jason R. Thorpe.
//!
//! The driver handles three wire protocols: Command/Bulk/Interrupt (CBI), CBI with Command
//! Completion Interrupt (CBI_I), and Bulk-Only (BBB: Bulk/Bulk/Bulk for the command, data
//! and status phases). Over them it carries SCSI, 8070i (ATAPI, the commands padded to 12
//! bytes) and UFI (USB floppies); `umass_adjust_transfer` does the little the command
//! protocols need here, and `umass_scsi.c` attaches the SCSI bus.
//!
//! Each protocol is a state machine (`umass_bbb_state`, `umass_cbi_state`), the callback of
//! every xfer: each state first handles the error of the previous transfer, then starts the
//! next one and returns. A transfer starts in `umass_*_transfer`, a reset recovery in
//! `umass_*_reset`; both end in the caller's `umass_callback`. This avoids sleeping in
//! interrupt context after a failed transfer. When the bus polls (the SCSI probe's
//! `SCSI_POLL` commands), `umass_polled_transfer` turns the callbacks' recursion into
//! iteration: a transfer started from a callback is kept and run when the current one is
//! over.
//!
//! ## Deviations
//! - `UMASS_DEBUG` is not in GENERIC: `umassdebug`, `states[]`, the `DPRINTF`/`DIF` calls
//!   and `umass_bbb_dump_cbw`, `umass_bbb_dump_csw`, `umass_dump_buffer` are absent, as in a
//!   kernel built without it. The `#if 0` `umass_reset` and residue check are not ported.
//! - The command is a slice (`cmd`, `cmdlen`); one longer than `CBWCDBLENGTH` is cut to it
//!   where the C overruns `CBWCDB`.
//! - The copies between the caller's buffer and the data xfer's DMA buffer are bounded by
//!   `transfer_datalen` and `UMASS_MAX_TRANSFER_SIZE`; a Bulk-Only residue larger than the
//!   transfer copies nothing, where the C's `memcpy` gets a negative length.
//! - The C's fall-through `switch` of the state machines is a `loop` over a `match` whose
//!   arms set the next case and `continue` where the C falls through.
//! - `dCBWtag`, a function `static` in C, is the `AtomicU32` [`DCBWTAG`].
//! - `umass_detach` returns `Result`; `umass_scsi_attach`'s 0 is `Ok(())`.

use core::ffi::c_void;
use core::mem::offset_of;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicU32, Ordering};

use crate::dev::usb::umass_quirks::umass_lookup;
use crate::dev::usb::umass_scsi::{umass_scsi_attach, umass_scsi_detach};
use crate::dev::usb::umassvar::{
    CBWCDBLENGTH, CBWFLAGS_IN, CBWFLAGS_OUT, CBWSIGNATURE, CSWSIGNATURE, CSWSIGNATURE_OLYMPUS_C1,
    CSWSTATUS_FAILED, CSWSTATUS_PHASE, DIR_IN, DIR_NONE, DIR_OUT, IDB_TYPE_CCI, IDB_VALUE_FAIL,
    IDB_VALUE_PASS, IDB_VALUE_PERSISTENT, IDB_VALUE_STATUS_MASK, STATUS_CMD_FAILED, STATUS_CMD_OK,
    STATUS_CMD_UNKNOWN, STATUS_WIRE_FAILED, TSTATE_BBB_COMMAND, TSTATE_BBB_DATA, TSTATE_BBB_DCLEAR,
    TSTATE_BBB_RESET1, TSTATE_BBB_RESET2, TSTATE_BBB_RESET3, TSTATE_BBB_SCLEAR, TSTATE_BBB_STATUS1,
    TSTATE_BBB_STATUS2, TSTATE_CBI_COMMAND, TSTATE_CBI_DATA, TSTATE_CBI_DCLEAR, TSTATE_CBI_RESET1,
    TSTATE_CBI_RESET2, TSTATE_CBI_RESET3, TSTATE_CBI_SCLEAR, TSTATE_CBI_STATUS, TSTATE_IDLE,
    UFI_COMMAND_LENGTH, UMASS_BBB_CBW_SIZE, UMASS_BBB_CSW_SIZE, UMASS_BULKIN, UMASS_BULKOUT,
    UMASS_CPROTO_ATAPI, UMASS_CPROTO_ISD_ATA, UMASS_CPROTO_RBC, UMASS_CPROTO_SCSI,
    UMASS_CPROTO_UFI, UMASS_CPROTO_UNSPEC, UMASS_INTRIN, UMASS_MAX_TRANSFER_SIZE,
    UMASS_QUIRK_IGNORE_RESIDUE, UMASS_QUIRK_WRONG_CSWSIG, UMASS_QUIRK_WRONG_CSWTAG,
    UMASS_WPROTO_BBB, UMASS_WPROTO_CBI, UMASS_WPROTO_CBI_I, UMASS_WPROTO_UNSPEC,
    UR_BBB_GET_MAX_LUN, UR_BBB_RESET, UR_CBI_ADSC, UmassBbbCbw, UmassCallback, UmassCbiSbl,
    UmassSoftc, UmassWireMethods, XFER_BBB_CBW, XFER_BBB_CSW1, XFER_BBB_CSW2, XFER_BBB_DATA,
    XFER_BBB_DCLEAR, XFER_BBB_RESET1, XFER_BBB_RESET2, XFER_BBB_RESET3, XFER_BBB_SCLEAR,
    XFER_CBI_CB, XFER_CBI_DATA, XFER_CBI_DCLEAR, XFER_CBI_RESET1, XFER_CBI_RESET2, XFER_CBI_RESET3,
    XFER_CBI_SCLEAR, XFER_CBI_STATUS, XFER_NR,
};
use crate::dev::usb::usb::{
    UE_BULK, UE_DIR_IN, UE_DIR_OUT, UE_INTERRUPT, UF_ENDPOINT_HALT, UICLASS_MASS, UIPROTO_MASS_BBB,
    UIPROTO_MASS_BBB_OLD, UIPROTO_MASS_CBI, UIPROTO_MASS_CBI_I, UISUBCLASS_QIC157, UISUBCLASS_RBC,
    UISUBCLASS_SCSI, UISUBCLASS_SFF8020I, UISUBCLASS_SFF8070I, UISUBCLASS_UFI, UR_CLEAR_FEATURE,
    USBD_SHORT_XFER_OK, UT_READ_CLASS_INTERFACE, UT_WRITE_CLASS_INTERFACE, UT_WRITE_ENDPOINT,
    UsbDeviceRequest, ue_get_dir, ue_get_xfertype, ugetdw, usetdw, usetw,
};
use crate::dev::usb::usb_subr::usbd_errstr;
use crate::dev::usb::usbdi::{
    UMATCH_IFACECLASS, UMATCH_IFACECLASS_IFACESUBCLASS, UMATCH_IFACECLASS_IFACESUBCLASS_IFACEPROTO,
    UMATCH_NONE, USBD_DEFAULT_TIMEOUT, USBD_EXCLUSIVE_USE, USBD_IN_PROGRESS, USBD_IOERROR,
    USBD_NO_COPY, USBD_NORMAL_COMPLETION, USBD_STALLED, UsbAttachArg, UsbdStatus, splusb,
    usbd_abort_pipe, usbd_alloc_buffer, usbd_alloc_xfer, usbd_clear_endpoint_toggle,
    usbd_close_pipe, usbd_do_request_flags, usbd_free_xfer, usbd_get_interface_descriptor,
    usbd_get_xfer_status, usbd_interface2endpoint_descriptor, usbd_is_dying, usbd_open_pipe,
    usbd_setup_default_xfer, usbd_setup_xfer, usbd_transfer,
};
use crate::dev::usb::usbdi_util::usb_detach_wait;
use crate::dev::usb::usbdivar::{UsbdPipe, UsbdXfer};
use crate::kern::subr_prf::{panic, printf};
use crate::machine::intr::splx;
use crate::scsi::scsi_all::{
    INQUIRY, MODE_SENSE_BIG, REQUEST_SENSE, SID_SCSI2_ALEN, SID_SCSI2_HDRLEN,
};
use crate::sys::device::{CfMatch, Cfattach, Cfdriver, DV_DULL, Device};
use crate::sys::errno::Errno;

/// `umass_cd`.
pub static UMASS_CD: Cfdriver = Cfdriver::new(b"umass", DV_DULL, 0);

/// `umass_ca`.
pub static UMASS_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<UmassSoftc>(),
    ca_match: Some(umass_match),
    ca_attach: umass_attach,
    ca_detach: Some(umass_detach),
    ca_activate: None,
};

/// `umass_bbb_methods`: Bulk-Only.
pub static UMASS_BBB_METHODS: UmassWireMethods = UmassWireMethods {
    wire_xfer: umass_bbb_transfer,
    wire_reset: umass_bbb_reset,
    wire_state: umass_bbb_state,
};

/// `umass_cbi_methods`: CBI and CBI with CCI.
pub static UMASS_CBI_METHODS: UmassWireMethods = UmassWireMethods {
    wire_xfer: umass_cbi_transfer,
    wire_reset: umass_cbi_reset,
    wire_state: umass_cbi_state,
};

/// `dCBWtag` of `umass_bbb_transfer`: unique for CBW of transfer.
pub static DCBWTAG: AtomicU32 = AtomicU32::new(42);

/// `(struct umass_softc *)self`.
fn umass_softc(self_: &Device) -> &'static UmassSoftc {
    // SAFETY: only called with devices `umass_ca` made (umass's own entry points), whose
    // softc is a `UmassSoftc`; attached devices live until `config_detach` frees them after
    // `umass_detach`.
    unsafe { &*ptr::from_ref(self_.softc::<UmassSoftc>()) }
}

/// `(struct umass_softc *)priv`: the softc an xfer's callback gets back.
fn umass_priv_softc(priv_: *mut c_void) -> &'static UmassSoftc {
    if priv_.is_null() {
        panic(format_args!("umass: xfer without its softc"));
    }
    // SAFETY: umass sets up its xfers with its own softc as `priv`
    // (`umass_setup_transfer`, `umass_setup_ctrl_transfer`); the softc outlives them.
    unsafe { &*priv_.cast::<UmassSoftc>().cast_const() }
}

/// `(void *)sc`: the softc as an xfer's `priv`.
fn umass_priv(sc: &'static UmassSoftc) -> *mut c_void {
    ptr::from_ref(sc).cast_mut().cast()
}

/// `&sc->cbw`, the CBW buffer of the command xfer.
fn umass_cbw_ptr(sc: &UmassSoftc) -> *mut u8 {
    sc.cbw.as_ptr().cast()
}

/// `sc->cbw.CBWCDB`, the command buffer of the CBI ADSC request.
fn umass_cbwcdb_ptr(sc: &UmassSoftc) -> *mut u8 {
    umass_cbw_ptr(sc).wrapping_add(offset_of!(UmassBbbCbw, CBWCDB))
}

/// `sc->transfer_cb(sc, sc->transfer_priv, residue, status)`.
fn umass_transfer_cb(sc: &'static UmassSoftc, residue: i32, status: i32) {
    match sc.transfer_cb.get() {
        Some(cb) => cb(sc, sc.transfer_priv.get(), residue, status),
        None => panic(format_args!(
            "{}: transfer without a callback",
            sc.sc_dev.xname()
        )),
    }
}

/// The bytes `memcpy` may move between `transfer_data` and `data_buffer`: `len`, within
/// the transfer and the DMA buffer.
fn umass_copy_len(sc: &UmassSoftc, len: i32) -> usize {
    let max = sc.transfer_datalen.get().max(0) as usize;
    (len.max(0) as usize).min(max).min(UMASS_MAX_TRANSFER_SIZE)
}

/// `memcpy(sc->data_buffer, sc->transfer_data, len)`: the data to send.
fn umass_copy_to_dma(sc: &UmassSoftc, len: i32) {
    let n = umass_copy_len(sc, len);
    let (src, dst) = (sc.transfer_data.get(), sc.data_buffer.get());
    if n == 0 || src.is_null() || dst.is_null() {
        return;
    }
    // SAFETY: `data_buffer` is the data xfer's DMA buffer of `UMASS_MAX_TRANSFER_SIZE` bytes
    // (`umass_attach`); `transfer_data` is valid for `transfer_datalen` bytes until the
    // callback runs (`umass_wire_xfer`'s contract); `n` is within both, which do not
    // overlap.
    unsafe { ptr::copy_nonoverlapping(src.cast_const(), dst, n) };
}

/// `memcpy(sc->transfer_data, sc->data_buffer, len)`: the data received.
fn umass_copy_from_dma(sc: &UmassSoftc, len: i32) {
    let n = umass_copy_len(sc, len);
    let (src, dst) = (sc.data_buffer.get(), sc.transfer_data.get());
    if n == 0 || src.is_null() || dst.is_null() {
        return;
    }
    // SAFETY: as in `umass_copy_to_dma`, the other way.
    unsafe { ptr::copy_nonoverlapping(src.cast_const(), dst, n) };
}

/*
 * USB device probe/attach/detach
 */

/// `umass_match`.
pub fn umass_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: `usbd_probe_and_attach` hands its `usb_attach_arg`, valid during the match.
    let uaa = unsafe { &*aux.cast::<UsbAttachArg>() };

    let Some(iface) = uaa.iface else {
        return UMATCH_NONE;
    };

    if let Some(quirk) = umass_lookup(uaa.vendor as u16, uaa.product as u16) {
        return quirk.uq_match;
    }

    let Some(id) = usbd_get_interface_descriptor(iface) else {
        return UMATCH_NONE;
    };
    if id.bInterfaceClass != UICLASS_MASS {
        return UMATCH_NONE;
    }

    match id.bInterfaceSubClass {
        UISUBCLASS_RBC | UISUBCLASS_SFF8020I | UISUBCLASS_QIC157 | UISUBCLASS_UFI
        | UISUBCLASS_SFF8070I | UISUBCLASS_SCSI => {}
        _ => return UMATCH_IFACECLASS,
    }

    match id.bInterfaceProtocol {
        UIPROTO_MASS_CBI_I | UIPROTO_MASS_CBI | UIPROTO_MASS_BBB_OLD | UIPROTO_MASS_BBB => {}
        _ => return UMATCH_IFACECLASS_IFACESUBCLASS,
    }

    UMATCH_IFACECLASS_IFACESUBCLASS_IFACEPROTO
}

/// `umass_attach`.
pub fn umass_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    let sc = umass_softc(self_);
    // SAFETY: as in `umass_match`, valid during the attach.
    let uaa = unsafe { &*aux.cast::<UsbAttachArg>() };

    sc.sc_udev.set(uaa.device);
    sc.sc_iface.set(uaa.iface);
    sc.sc_ifaceno.set(uaa.ifaceno);

    let quirk = umass_lookup(uaa.vendor as u16, uaa.product as u16);
    if let Some(quirk) = quirk {
        sc.sc_wire.set(quirk.uq_wire);
        sc.sc_cmd.set(quirk.uq_cmd);
        sc.sc_quirks.set(quirk.uq_flags);
        sc.sc_busquirks.set(quirk.uq_busquirks);

        if let Some(fixup) = quirk.uq_fixup {
            fixup(sc);
        }
    } else {
        sc.sc_wire.set(UMASS_WPROTO_UNSPEC);
        sc.sc_cmd.set(UMASS_CPROTO_UNSPEC);
        sc.sc_quirks.set(0);
        sc.sc_busquirks.set(0);
    }

    let Some(id) = usbd_get_interface_descriptor(sc.iface()) else {
        return;
    };

    if sc.sc_wire.get() == UMASS_WPROTO_UNSPEC {
        match id.bInterfaceProtocol {
            UIPROTO_MASS_CBI => sc.sc_wire.set(UMASS_WPROTO_CBI),
            UIPROTO_MASS_CBI_I => sc.sc_wire.set(UMASS_WPROTO_CBI_I),
            UIPROTO_MASS_BBB | UIPROTO_MASS_BBB_OLD => sc.sc_wire.set(UMASS_WPROTO_BBB),
            // Unsupported wire protocol.
            _ => return,
        }
    }

    if sc.sc_cmd.get() == UMASS_CPROTO_UNSPEC {
        match id.bInterfaceSubClass {
            UISUBCLASS_SCSI => sc.sc_cmd.set(UMASS_CPROTO_SCSI),
            UISUBCLASS_UFI => sc.sc_cmd.set(UMASS_CPROTO_UFI),
            UISUBCLASS_SFF8020I | UISUBCLASS_SFF8070I | UISUBCLASS_QIC157 => {
                sc.sc_cmd.set(UMASS_CPROTO_ATAPI)
            }
            UISUBCLASS_RBC => sc.sc_cmd.set(UMASS_CPROTO_RBC),
            // Unsupported command protocol.
            _ => return,
        }
    }

    let s_wire = match sc.sc_wire.get() {
        UMASS_WPROTO_CBI => "CBI",
        UMASS_WPROTO_CBI_I => "CBI with CCI",
        UMASS_WPROTO_BBB => "Bulk-Only",
        _ => "unknown",
    };

    let s_command = match sc.sc_cmd.get() {
        UMASS_CPROTO_RBC => "RBC",
        UMASS_CPROTO_SCSI => "SCSI",
        UMASS_CPROTO_UFI => "UFI",
        UMASS_CPROTO_ATAPI => "ATAPI",
        UMASS_CPROTO_ISD_ATA => "ISD-ATA",
        _ => "unknown",
    };

    printf(format_args!(
        "{}: using {} over {}\n",
        sc.sc_dev.xname(),
        s_command,
        s_wire
    ));

    if let Some(init) = quirk.and_then(|q| q.uq_init)
        && init(sc).is_err()
    {
        umass_disco(sc);
        return;
    }

    // In addition to the Control endpoint the following endpoints are required:
    // a) bulk-in endpoint.
    // b) bulk-out endpoint.
    // and for Control/Bulk/Interrupt with CCI (CBI_I)
    // c) intr-in
    //
    // The endpoint addresses are not fixed, so we have to read them from the device
    // descriptors of the current interface.
    for i in 0..id.bNumEndpoints {
        let Some(ed) = usbd_interface2endpoint_descriptor(sc.iface(), i) else {
            printf(format_args!(
                "{}: could not read endpoint descriptor\n",
                sc.sc_dev.xname()
            ));
            return;
        };
        let dir = ue_get_dir(ed.bEndpointAddress);
        let xfertype = ue_get_xfertype(ed.bmAttributes);
        if dir == UE_DIR_IN && xfertype == UE_BULK {
            sc.sc_epaddr[UMASS_BULKIN].set(ed.bEndpointAddress);
        } else if dir == UE_DIR_OUT && xfertype == UE_BULK {
            sc.sc_epaddr[UMASS_BULKOUT].set(ed.bEndpointAddress);
        } else if sc.sc_wire.get() == UMASS_WPROTO_CBI_I
            && dir == UE_DIR_IN
            && xfertype == UE_INTERRUPT
        {
            sc.sc_epaddr[UMASS_INTRIN].set(ed.bEndpointAddress);
        }
    }

    // check whether we found all the endpoints we need
    if sc.sc_epaddr[UMASS_BULKIN].get() == 0
        || sc.sc_epaddr[UMASS_BULKOUT].get() == 0
        || (sc.sc_wire.get() == UMASS_WPROTO_CBI_I && sc.sc_epaddr[UMASS_INTRIN].get() == 0)
    {
        return;
    }

    // Get the maximum LUN supported by the device.
    if sc.sc_wire.get() == UMASS_WPROTO_BBB {
        sc.maxlun.set(umass_bbb_get_max_lun(sc));
    } else {
        sc.maxlun.set(0);
    }

    // Open the bulk-in and -out pipe
    match usbd_open_pipe(
        sc.iface(),
        sc.sc_epaddr[UMASS_BULKOUT].get(),
        USBD_EXCLUSIVE_USE,
    ) {
        Ok(p) => sc.sc_pipe[UMASS_BULKOUT].set(Some(p)),
        Err(_) => {
            umass_disco(sc);
            return;
        }
    }
    match usbd_open_pipe(
        sc.iface(),
        sc.sc_epaddr[UMASS_BULKIN].get(),
        USBD_EXCLUSIVE_USE,
    ) {
        Ok(p) => sc.sc_pipe[UMASS_BULKIN].set(Some(p)),
        Err(_) => {
            umass_disco(sc);
            return;
        }
    }
    // Open the intr-in pipe if the protocol is CBI with CCI.
    // Note: early versions of the Zip drive do have an interrupt pipe, but this pipe is
    // unused
    //
    // We do not open the interrupt pipe as an interrupt pipe, but as a normal bulk endpoint.
    // We send an IN transfer down the wire at the appropriate time, because we know exactly
    // when to expect data on that endpoint. This saves bandwidth, but more important, makes
    // the code for handling the data on that endpoint simpler. No data arriving
    // concurrently.
    if sc.sc_wire.get() == UMASS_WPROTO_CBI_I {
        match usbd_open_pipe(
            sc.iface(),
            sc.sc_epaddr[UMASS_INTRIN].get(),
            USBD_EXCLUSIVE_USE,
        ) {
            Ok(p) => sc.sc_pipe[UMASS_INTRIN].set(Some(p)),
            Err(_) => {
                umass_disco(sc);
                return;
            }
        }
    }

    // initialisation of generic part
    sc.transfer_state.set(TSTATE_IDLE);

    // request a sufficient number of xfer handles
    for i in 0..XFER_NR {
        let x = usbd_alloc_xfer(uaa.device());
        sc.transfer_xfer[i].set(x);
        if x.is_none() {
            umass_disco(sc);
            return;
        }
    }
    // Allocate buffer for data transfer (it's huge).
    let bno = match sc.sc_wire.get() {
        UMASS_WPROTO_BBB => Some(XFER_BBB_DATA),
        UMASS_WPROTO_CBI | UMASS_WPROTO_CBI_I => Some(XFER_CBI_DATA),
        _ => None,
    };
    if let Some(bno) = bno {
        match usbd_alloc_buffer(sc.xfer(bno), UMASS_MAX_TRANSFER_SIZE as u32) {
            Some(b) => sc.data_buffer.set(b.as_ptr()),
            None => {
                umass_disco(sc);
                return;
            }
        }
    }

    // Initialise the wire protocol specific methods
    match sc.sc_wire.get() {
        UMASS_WPROTO_BBB => sc.sc_methods.set(Some(&UMASS_BBB_METHODS)),
        UMASS_WPROTO_CBI | UMASS_WPROTO_CBI_I => sc.sc_methods.set(Some(&UMASS_CBI_METHODS)),
        _ => {
            umass_disco(sc);
            return;
        }
    }

    let error = match sc.sc_cmd.get() {
        UMASS_CPROTO_RBC | UMASS_CPROTO_SCSI | UMASS_CPROTO_UFI | UMASS_CPROTO_ATAPI => {
            umass_scsi_attach(sc)
        }
        UMASS_CPROTO_ISD_ATA => {
            printf(format_args!(
                "{}: isdata not configured\n",
                sc.sc_dev.xname()
            ));
            Ok(())
        }
        cmd => {
            printf(format_args!(
                "{}: command protocol=0x{:x} not supported\n",
                sc.sc_dev.xname(),
                cmd
            ));
            umass_disco(sc);
            return;
        }
    };
    if error.is_err() {
        printf(format_args!("{}: bus attach failed\n", sc.sc_dev.xname()));
        umass_disco(sc);
    }
}

/// `umass_detach`.
pub fn umass_detach(self_: &Device, flags: i32) -> Result<(), Errno> {
    let sc = umass_softc(self_);

    // Abort the pipes to wake up any waiting processes.
    for pipe in &sc.sc_pipe {
        if let Some(p) = pipe.get() {
            usbd_abort_pipe(p);
        }
    }

    // Do we really need reference counting?  Perhaps in ioctl()
    let s = splusb();
    sc.sc_refcnt.set(sc.sc_refcnt.get() - 1);
    if sc.sc_refcnt.get() >= 0 {
        #[cfg(feature = "diagnostic")]
        printf(format_args!("{}: waiting for refcnt\n", sc.sc_dev.xname()));
        // Wait for processes to go away.
        usb_detach_wait(&sc.sc_dev);
    }

    // Free the buffers via callback.
    if sc.transfer_state.get() != TSTATE_IDLE && !sc.transfer_priv.get().is_null() {
        sc.transfer_state.set(TSTATE_IDLE);
        umass_transfer_cb(sc, sc.transfer_datalen.get(), STATUS_WIRE_FAILED);
        sc.transfer_priv.set(ptr::null_mut());
    }
    splx(s);

    umass_scsi_detach(sc, flags)?;

    umass_disco(sc);

    Ok(())
}

/// `umass_disco`: closes the pipes and frees the xfers.
pub fn umass_disco(sc: &'static UmassSoftc) {
    // Remove all the pipes.
    for pipe in &sc.sc_pipe {
        if let Some(p) = pipe.take() {
            // SAFETY: the pipe was opened by `umass_attach` and nothing uses it once it is
            // off the softc.
            unsafe { usbd_close_pipe(NonNull::from(p)) };
        }
    }

    // Make sure there is no stuck control transfer left.
    if let Some(p) = sc.udev().default_pipe.get() {
        usbd_abort_pipe(p);
    }

    // Free the xfers.
    for xfer in &sc.transfer_xfer {
        if let Some(x) = xfer.take() {
            // SAFETY: allocated by `umass_attach`; the pipes are closed (their queues
            // aborted) and the default pipe aborted, so the xfer is not queued, and nothing
            // uses it once it is off the softc.
            unsafe { usbd_free_xfer(NonNull::from(x)) };
        }
    }
    sc.data_buffer.set(ptr::null_mut());
}

/*
 * Generic functions to handle transfers
 */

/// `umass_polled_transfer`: runs `xfer` while the bus polls; a transfer started from a
/// callback meanwhile is kept and run after the current one completes.
pub fn umass_polled_transfer(sc: &'static UmassSoftc, xfer: &'static UsbdXfer) -> UsbdStatus {
    if usbd_is_dying(sc.udev()) {
        return USBD_IOERROR;
    }

    // If a polled transfer is already in progress, preserve the new struct usbd_xfer and
    // run it after the running one completes. This converts the recursive calls into the
    // umass_*_state callbacks into iteration, preventing us from running out of stack under
    // error conditions.
    if sc.polling_depth.get() != 0 {
        if let Some(pending) = sc.next_polled_xfer.get() {
            panic(format_args!(
                "{}: got polled xfer {:p}, but {:p} already pending\n",
                sc.sc_dev.xname(),
                xfer,
                pending
            ));
        }

        sc.next_polled_xfer.set(Some(xfer));

        return USBD_IN_PROGRESS;
    }

    sc.polling_depth.set(sc.polling_depth.get() + 1);

    let mut xfer = xfer;
    loop {
        // start_next_xfer:
        let err = usbd_transfer(xfer);
        if err.is_err() && err != USBD_IN_PROGRESS && sc.next_polled_xfer.get().is_none() {
            sc.polling_depth.set(sc.polling_depth.get() - 1);
            return err;
        }

        match sc.next_polled_xfer.take() {
            Some(next) => xfer = next,
            None => break,
        }
    }

    sc.polling_depth.set(sc.polling_depth.get() - 1);

    USBD_NORMAL_COMPLETION
}

/// `umass_setup_transfer`: sets up `xfer` on `pipe` with the state machine as callback and
/// starts it.
///
/// # Safety
///
/// `buffer` is valid for `buflen` bytes (reads for an OUT pipe, writes for an IN one) until
/// the transfer completes; with `USBD_NO_COPY` it is the xfer's own DMA buffer.
pub unsafe fn umass_setup_transfer(
    sc: &'static UmassSoftc,
    pipe: &'static UsbdPipe,
    buffer: *mut u8,
    buflen: u32,
    flags: u16,
    xfer: &'static UsbdXfer,
) -> UsbdStatus {
    if usbd_is_dying(sc.udev()) {
        return USBD_IOERROR;
    }

    // Initialise a USB transfer and then schedule it

    // SAFETY: the caller's contract on `buffer`.
    unsafe {
        usbd_setup_xfer(
            xfer,
            pipe,
            umass_priv(sc),
            buffer,
            buflen,
            flags | sc.sc_xfer_flags.get(),
            sc.timeout.get(),
            Some(sc.methods().wire_state),
        )
    };

    let err = if sc.udev().bus().use_polling.get() != 0 {
        umass_polled_transfer(sc, xfer)
    } else {
        usbd_transfer(xfer)
    };
    if err.is_err() && err != USBD_IN_PROGRESS {
        return err;
    }

    USBD_NORMAL_COMPLETION
}

/// `umass_setup_ctrl_transfer`: sets up `xfer` as the control request `req` on the default
/// pipe with the state machine as callback and starts it.
///
/// # Safety
///
/// As for [`umass_setup_transfer`]; `buffer` may be null when `buflen` is 0.
pub unsafe fn umass_setup_ctrl_transfer(
    sc: &'static UmassSoftc,
    req: &UsbDeviceRequest,
    buffer: *mut u8,
    buflen: u32,
    flags: u16,
    xfer: &'static UsbdXfer,
) -> UsbdStatus {
    if usbd_is_dying(sc.udev()) {
        return USBD_IOERROR;
    }

    // Initialise a USB control transfer and then schedule it

    // SAFETY: the caller's contract on `buffer`.
    unsafe {
        usbd_setup_default_xfer(
            xfer,
            sc.udev(),
            umass_priv(sc),
            USBD_DEFAULT_TIMEOUT,
            req,
            buffer,
            buflen,
            flags | sc.sc_xfer_flags.get(),
            Some(sc.methods().wire_state),
        )
    };

    let err = if sc.udev().bus().use_polling.get() != 0 {
        umass_polled_transfer(sc, xfer)
    } else {
        usbd_transfer(xfer)
    };
    if err.is_err() && err != USBD_IN_PROGRESS {
        // do not reset, as this would make us loop
        return err;
    }

    USBD_NORMAL_COMPLETION
}

/// `umass_adjust_transfer`: the command protocols' changes to the command block (12-byte
/// commands; UFI's shorter INQUIRY, MODE SENSE and REQUEST SENSE data).
pub fn umass_adjust_transfer(sc: &UmassSoftc) {
    let mut cbw = sc.cbw.get();
    match sc.sc_cmd.get() {
        UMASS_CPROTO_UFI => {
            cbw.bCDBLength = UFI_COMMAND_LENGTH;
            // Adjust the length field in certain scsi commands.
            const INQUIRY_MAX: i32 = (SID_SCSI2_HDRLEN + SID_SCSI2_ALEN) as i32;
            let datalen = sc.transfer_datalen.get();
            match cbw.CBWCDB[0] {
                INQUIRY if datalen > INQUIRY_MAX => {
                    sc.transfer_datalen.set(INQUIRY_MAX);
                    cbw.CBWCDB[4] = INQUIRY_MAX as u8;
                }
                MODE_SENSE_BIG if datalen > 8 => {
                    sc.transfer_datalen.set(8);
                    cbw.CBWCDB[7] = 0;
                    cbw.CBWCDB[8] = 8;
                }
                REQUEST_SENSE if datalen > 18 => {
                    sc.transfer_datalen.set(18);
                    cbw.CBWCDB[4] = 18;
                }
                _ => {}
            }
        }
        UMASS_CPROTO_ATAPI => cbw.bCDBLength = UFI_COMMAND_LENGTH,
        _ => {}
    }
    sc.cbw.set(cbw);
}

/// `umass_clear_endpoint_stall`: clears the halt of endpoint `endpt` (`UMASS_BULKIN`, ...)
/// with `xfer`.
pub fn umass_clear_endpoint_stall(sc: &'static UmassSoftc, endpt: usize, xfer: &'static UsbdXfer) {
    if usbd_is_dying(sc.udev()) {
        return;
    }

    usbd_clear_endpoint_toggle(sc.pipe(endpt));

    let mut req = UsbDeviceRequest {
        bmRequestType: UT_WRITE_ENDPOINT,
        bRequest: UR_CLEAR_FEATURE,
        ..UsbDeviceRequest::default()
    };
    usetw(&mut req.wValue, UF_ENDPOINT_HALT);
    usetw(&mut req.wIndex, u16::from(sc.sc_epaddr[endpt].get()));
    usetw(&mut req.wLength, 0);
    sc.sc_req.set(req);
    // SAFETY: no data stage.
    let _ = unsafe { umass_setup_ctrl_transfer(sc, &req, ptr::null_mut(), 0, 0, xfer) };
}

/*
 * Bulk protocol specific functions
 */

/// `umass_bbb_reset`: Bulk-Only reset recovery, ending with `status`.
pub fn umass_bbb_reset(sc: &'static UmassSoftc, status: i32) {
    if usbd_is_dying(sc.udev()) {
        return;
    }

    // Reset recovery (5.3.4 in Universal Serial Bus Mass Storage Class)
    //
    // For Reset Recovery the host shall issue in the following order:
    // a) a Bulk-Only Mass Storage Reset
    // b) a Clear Feature HALT to the Bulk-In endpoint
    // c) a Clear Feature HALT to the Bulk-Out endpoint
    //
    // This is done in 3 steps, states:
    // TSTATE_BBB_RESET1
    // TSTATE_BBB_RESET2
    // TSTATE_BBB_RESET3
    //
    // If the reset doesn't succeed, the device should be port reset.

    sc.transfer_state.set(TSTATE_BBB_RESET1);
    sc.transfer_status.set(status);

    // reset is a class specific interface write
    let mut req = UsbDeviceRequest {
        bmRequestType: UT_WRITE_CLASS_INTERFACE,
        bRequest: UR_BBB_RESET,
        ..UsbDeviceRequest::default()
    };
    usetw(&mut req.wValue, 0);
    usetw(&mut req.wIndex, sc.sc_ifaceno.get() as u16);
    usetw(&mut req.wLength, 0);
    sc.sc_req.set(req);
    // SAFETY: no data stage.
    let _ = unsafe {
        umass_setup_ctrl_transfer(sc, &req, ptr::null_mut(), 0, 0, sc.xfer(XFER_BBB_RESET1))
    };
}

/// `umass_bbb_transfer`: a Bulk-Only command.
///
/// # Safety
///
/// `umass_wire_xfer`'s contract.
#[allow(clippy::too_many_arguments)] // the C's signature
pub unsafe fn umass_bbb_transfer(
    sc: &'static UmassSoftc,
    lun: i32,
    cmd: &[u8],
    data: *mut u8,
    datalen: i32,
    dir: i32,
    timeout: u32,
    cb: UmassCallback,
    priv_: *mut c_void,
) {
    if usbd_is_dying(sc.udev()) {
        sc.polled_xfer_status.set(USBD_IOERROR);
        return;
    }

    // Be a little generous.
    sc.timeout.set(timeout.wrapping_add(USBD_DEFAULT_TIMEOUT));

    // Do a Bulk-Only transfer with cmdlen bytes from cmd, possibly a data phase of datalen
    // bytes from/to the device and finally a csw read phase. If the data direction was
    // inbound a maximum of datalen bytes is stored in the buffer pointed to by data.
    //
    // umass_bbb_transfer initialises the transfer and lets the state machine in
    // umass_bbb_state handle the completion. It uses the following states:
    // TSTATE_BBB_COMMAND
    //   -> TSTATE_BBB_DATA
    //   -> TSTATE_BBB_STATUS
    //   -> TSTATE_BBB_STATUS2
    //   -> TSTATE_BBB_IDLE
    //
    // An error in any of those states will invoke umass_bbb_reset.

    // Determine the direction of the data transfer and the length.
    //
    // dCBWDataTransferLength (datalen): this field indicates the number of bytes of data
    // that the host intends to transfer on the IN or OUT Bulk endpoint (as indicated by the
    // Direction bit) during the execution of this command. If this field is set to 0, the
    // device will expect that no data will be transferred IN or OUT during this command,
    // regardless of the value of the Direction bit defined in dCBWFlags.
    //
    // dCBWFlags (dir): bits 0-6 reserved; bit 7 Direction (ignored if
    // dCBWDataTransferLength is zero): 0 = data Out from host to device, 1 = data In from
    // device to host.

    // Fill in the Command Block Wrapper
    let mut cbw = sc.cbw.get();
    usetdw(&mut cbw.dCBWSignature, CBWSIGNATURE);
    usetdw(&mut cbw.dCBWTag, DCBWTAG.fetch_add(1, Ordering::Relaxed));
    usetdw(&mut cbw.dCBWDataTransferLength, datalen as u32);
    // DIR_NONE is treated as DIR_OUT (0x00)
    cbw.bCBWFlags = if dir == DIR_IN {
        CBWFLAGS_IN
    } else {
        CBWFLAGS_OUT
    };
    cbw.bCBWLUN = lun as u8;
    let cmdlen = cmd.len().min(CBWCDBLENGTH);
    cbw.bCDBLength = cmdlen as u8;
    cbw.CBWCDB = [0; CBWCDBLENGTH];
    cbw.CBWCDB[..cmdlen].copy_from_slice(&cmd[..cmdlen]);
    sc.cbw.set(cbw);

    // store the details for the data transfer phase
    sc.transfer_dir.set(dir);
    sc.transfer_data.set(data);
    sc.transfer_datalen.set(datalen);
    sc.transfer_actlen.set(0);
    sc.transfer_cb.set(Some(cb));
    sc.transfer_priv.set(priv_);
    sc.transfer_status.set(STATUS_CMD_OK);

    // move from idle to the command state
    sc.transfer_state.set(TSTATE_BBB_COMMAND);

    // Send the CBW from host to device via bulk-out endpoint.
    umass_adjust_transfer(sc);
    // SAFETY: the CBW lives in the softc, which outlives the transfer; the stack copies it
    // into the xfer's DMA buffer when it starts.
    let err = unsafe {
        umass_setup_transfer(
            sc,
            sc.pipe(UMASS_BULKOUT),
            umass_cbw_ptr(sc),
            UMASS_BBB_CBW_SIZE,
            0,
            sc.xfer(XFER_BBB_CBW),
        )
    };
    if err.is_err() {
        umass_bbb_reset(sc, STATUS_WIRE_FAILED);
    }

    if sc.udev().bus().use_polling.get() != 0 {
        sc.polled_xfer_status.set(err);
    }
}

/// Starts the data phase of a command (`TSTATE_*_DATA`) on `xfer`; `false` when it could
/// not be started.
fn umass_start_data(sc: &'static UmassSoftc, xfer: &'static UsbdXfer) -> bool {
    let datalen = sc.transfer_datalen.get().max(0) as u32;
    let err = if sc.transfer_dir.get() == DIR_IN {
        // SAFETY: `data_buffer` is this xfer's own DMA buffer of `UMASS_MAX_TRANSFER_SIZE`
        // bytes (`umass_attach`), and `transfer_datalen` is no more (`umass_scsi_cmd`).
        unsafe {
            umass_setup_transfer(
                sc,
                sc.pipe(UMASS_BULKIN),
                sc.data_buffer.get(),
                datalen,
                USBD_SHORT_XFER_OK | USBD_NO_COPY,
                xfer,
            )
        }
    } else {
        umass_copy_to_dma(sc, sc.transfer_datalen.get());
        // SAFETY: as above.
        unsafe {
            umass_setup_transfer(
                sc,
                sc.pipe(UMASS_BULKOUT),
                sc.data_buffer.get(),
                datalen,
                USBD_NO_COPY, // fixed length transfer
                xfer,
            )
        }
    };
    !err.is_err()
}

/// `umass_bbb_state`: the Bulk-Only state machine, every Bulk-Only xfer's callback.
pub fn umass_bbb_state(xfer: &'static UsbdXfer, priv_: *mut c_void, err: UsbdStatus) {
    let sc = umass_priv_softc(priv_);

    if usbd_is_dying(sc.udev()) {
        return;
    }

    // State handling for BBB transfers.
    //
    // The subroutine is rather long. It steps through the states given in Annex A of the
    // Bulk-Only specification. Each state first does the error handling of the previous
    // transfer and then prepares the next transfer. Each transfer is done asynchronously
    // so after the request/transfer has been submitted you will find a 'return;'.

    let mut case = sc.transfer_state.get();
    loop {
        match case {
            /* Bulk Transfer */
            TSTATE_BBB_COMMAND => {
                // Command transport phase, error handling
                if err.is_err() {
                    // If the device detects that the CBW is invalid, then the device may
                    // STALL both bulk endpoints and require a Bulk-Reset
                    umass_bbb_reset(sc, STATUS_WIRE_FAILED);
                    return;
                }

                // Data transport phase, setup transfer
                sc.transfer_state.set(TSTATE_BBB_DATA);
                if sc.transfer_dir.get() == DIR_IN || sc.transfer_dir.get() == DIR_OUT {
                    if !umass_start_data(sc, sc.xfer(XFER_BBB_DATA)) {
                        umass_bbb_reset(sc, STATUS_WIRE_FAILED);
                    }
                    return;
                }
                // no data phase

                // FALLTHROUGH if no data phase, err == 0
                case = TSTATE_BBB_DATA;
            }
            TSTATE_BBB_DATA => {
                // Command transport phase error handling (ignored if no data phase
                // (fallthrough from previous state))
                if sc.transfer_dir.get() != DIR_NONE {
                    // retrieve the length of the transfer that was done
                    let mut actlen = 0u32;
                    usbd_get_xfer_status(xfer, None, None, Some(&mut actlen), None);
                    sc.transfer_actlen.set(actlen as i32);

                    if err.is_err() {
                        if err == USBD_STALLED {
                            sc.transfer_state.set(TSTATE_BBB_DCLEAR);
                            let endpt = if sc.transfer_dir.get() == DIR_IN {
                                UMASS_BULKIN
                            } else {
                                UMASS_BULKOUT
                            };
                            umass_clear_endpoint_stall(sc, endpt, sc.xfer(XFER_BBB_DCLEAR));
                        } else {
                            // Unless the error is a pipe stall the error is fatal.
                            umass_bbb_reset(sc, STATUS_WIRE_FAILED);
                        }
                        return;
                    }
                }

                // FALLTHROUGH, err == 0 (no data phase or successful)
                case = TSTATE_BBB_DCLEAR;
            }
            TSTATE_BBB_DCLEAR => {
                // stall clear after data phase

                // FALLTHROUGH, err == 0 (no data phase or successful)
                case = TSTATE_BBB_SCLEAR;
            }
            TSTATE_BBB_SCLEAR => {
                // stall clear after status phase

                // Reading of CSW after bulk stall condition in data phase (TSTATE_BBB_DATA2)
                // or bulk-in stall condition after reading CSW (TSTATE_BBB_SCLEAR). In the
                // case of no data phase or successful data phase, err == 0 and the
                // following if block is passed.
                if err.is_err() {
                    // should not occur
                    umass_bbb_reset(sc, STATUS_WIRE_FAILED);
                    return;
                }

                // Status transport phase, setup transfer
                let state = sc.transfer_state.get();
                let next_xfer = if state == TSTATE_BBB_COMMAND
                    || state == TSTATE_BBB_DATA
                    || state == TSTATE_BBB_DCLEAR
                {
                    // After no data phase, successful data phase and after clearing
                    // bulk-in/-out stall condition
                    sc.transfer_state.set(TSTATE_BBB_STATUS1);
                    sc.xfer(XFER_BBB_CSW1)
                } else {
                    // After first attempt of fetching CSW
                    sc.transfer_state.set(TSTATE_BBB_STATUS2);
                    sc.xfer(XFER_BBB_CSW2)
                };

                // Read the Command Status Wrapper via bulk-in endpoint.
                // SAFETY: the CSW lives in the softc, which outlives the transfer; the
                // stack copies the answer into it when the transfer completes.
                let e = unsafe {
                    umass_setup_transfer(
                        sc,
                        sc.pipe(UMASS_BULKIN),
                        sc.csw.as_ptr().cast(),
                        UMASS_BBB_CSW_SIZE,
                        0,
                        next_xfer,
                    )
                };
                if e.is_err() {
                    umass_bbb_reset(sc, STATUS_WIRE_FAILED);
                }
                return;
            }
            TSTATE_BBB_STATUS1 | TSTATE_BBB_STATUS2 => {
                // first attempt, second attempt
                umass_bbb_status(sc, err);
                return;
            }

            /* Bulk Reset */
            TSTATE_BBB_RESET1 => {
                // (UMASS_DEBUG: "BBB reset failed" on err)
                sc.transfer_state.set(TSTATE_BBB_RESET2);
                umass_clear_endpoint_stall(sc, UMASS_BULKIN, sc.xfer(XFER_BBB_RESET2));
                return;
            }
            TSTATE_BBB_RESET2 => {
                // (UMASS_DEBUG: "BBB bulk-in clear stall failed" on err, should not occur)
                // no error recovery, otherwise we end up in a loop
                sc.transfer_state.set(TSTATE_BBB_RESET3);
                umass_clear_endpoint_stall(sc, UMASS_BULKOUT, sc.xfer(XFER_BBB_RESET3));
                return;
            }
            TSTATE_BBB_RESET3 => {
                // (UMASS_DEBUG: "BBB bulk-out clear stall failed" on err, should not occur)
                // no error recovery, otherwise we end up in a loop
                sc.transfer_state.set(TSTATE_IDLE);
                if !sc.transfer_priv.get().is_null() {
                    umass_transfer_cb(sc, sc.transfer_datalen.get(), sc.transfer_status.get());
                }
                return;
            }

            /* Default */
            state => panic(format_args!(
                "{}: Unknown state {}",
                sc.sc_dev.xname(),
                state
            )),
        }
    }
}

/// The `TSTATE_BBB_STATUS1`/`TSTATE_BBB_STATUS2` case of `umass_bbb_state`: checks the CSW
/// and ends the command.
fn umass_bbb_status(sc: &'static UmassSoftc, err: UsbdStatus) {
    // Status transfer, error handling
    if err.is_err() {
        // If this was the first attempt at fetching the CSW retry it, otherwise fail.
        if sc.transfer_state.get() == TSTATE_BBB_STATUS1 {
            sc.transfer_state.set(TSTATE_BBB_SCLEAR);
            umass_clear_endpoint_stall(sc, UMASS_BULKIN, sc.xfer(XFER_BBB_SCLEAR));
        } else {
            umass_bbb_reset(sc, STATUS_WIRE_FAILED);
        }
        return;
    }

    let cbw = sc.cbw.get();
    let mut csw = sc.csw.get();

    // Translate weird command-status signatures.
    if sc.sc_quirks.get() & UMASS_QUIRK_WRONG_CSWSIG != 0
        && ugetdw(csw.dCSWSignature) == CSWSIGNATURE_OLYMPUS_C1
    {
        usetdw(&mut csw.dCSWSignature, CSWSIGNATURE);
    }

    // Translate invalid command-status tags
    if sc.sc_quirks.get() & UMASS_QUIRK_WRONG_CSWTAG != 0 {
        usetdw(&mut csw.dCSWTag, ugetdw(cbw.dCBWTag));
    }
    sc.csw.set(csw);

    // Check CSW and handle any error
    if ugetdw(csw.dCSWSignature) != CSWSIGNATURE {
        // Invalid CSW: Wrong signature or wrong tag might indicate that the device is
        // confused -> reset it.
        printf(format_args!(
            "{}: Invalid CSW: sig 0x{:08x} should be 0x{:08x}\n",
            sc.sc_dev.xname(),
            ugetdw(csw.dCSWSignature),
            CSWSIGNATURE
        ));

        umass_bbb_reset(sc, STATUS_WIRE_FAILED);
    } else if ugetdw(csw.dCSWTag) != ugetdw(cbw.dCBWTag) {
        printf(format_args!(
            "{}: Invalid CSW: tag {} should be {}\n",
            sc.sc_dev.xname(),
            ugetdw(csw.dCSWTag) as i32,
            ugetdw(cbw.dCBWTag) as i32
        ));

        umass_bbb_reset(sc, STATUS_WIRE_FAILED);

    // CSW is valid here
    } else if csw.bCSWStatus > CSWSTATUS_PHASE {
        printf(format_args!(
            "{}: Invalid CSW: status {} > {}\n",
            sc.sc_dev.xname(),
            csw.bCSWStatus,
            CSWSTATUS_PHASE
        ));

        umass_bbb_reset(sc, STATUS_WIRE_FAILED);
    } else if csw.bCSWStatus == CSWSTATUS_PHASE {
        printf(format_args!(
            "{}: Phase Error, residue = {}\n",
            sc.sc_dev.xname(),
            ugetdw(csw.dCSWDataResidue) as i32
        ));

        umass_bbb_reset(sc, STATUS_WIRE_FAILED);
    } else if sc.transfer_actlen.get() > sc.transfer_datalen.get() {
        // Buffer overrun! Don't let this go by unnoticed
        panic(format_args!(
            "{}: transferred {} bytes instead of {} bytes",
            sc.sc_dev.xname(),
            sc.transfer_actlen.get(),
            sc.transfer_datalen.get()
        ));
    } else if csw.bCSWStatus == CSWSTATUS_FAILED {
        // SCSI command failed but transfer was successful
        sc.transfer_state.set(TSTATE_IDLE);
        umass_transfer_cb(sc, ugetdw(csw.dCSWDataResidue) as i32, STATUS_CMD_FAILED);
    } else {
        // success
        let residue = ugetdw(csw.dCSWDataResidue);
        sc.transfer_state.set(TSTATE_IDLE);
        if sc.transfer_dir.get() == DIR_IN {
            if residue == sc.transfer_datalen.get() as u32 {
                if cbw.CBWCDB[0] == INQUIRY {
                    sc.sc_quirks
                        .set(sc.sc_quirks.get() | UMASS_QUIRK_IGNORE_RESIDUE);
                }
                if sc.sc_quirks.get() & UMASS_QUIRK_IGNORE_RESIDUE != 0 {
                    usetdw(&mut csw.dCSWDataResidue, 0);
                    sc.csw.set(csw);
                }
            }
            sc.transfer_actlen.set(
                sc.transfer_datalen
                    .get()
                    .wrapping_sub(ugetdw(csw.dCSWDataResidue) as i32),
            );
            umass_copy_from_dma(sc, sc.transfer_actlen.get());
        }
        umass_transfer_cb(sc, ugetdw(csw.dCSWDataResidue) as i32, STATUS_CMD_OK);
    }
}

/*
 * Command/Bulk/Interrupt (CBI) specific functions
 */

/// `umass_cbi_adsc`: the Accept Device-Specific Command request carrying `buflen` bytes at
/// `buffer`, on `xfer`.
///
/// # Safety
///
/// `buffer` is valid for `buflen` bytes until the transfer completes (the stack copies
/// them into the xfer's DMA buffer when it starts).
pub unsafe fn umass_cbi_adsc(
    sc: &'static UmassSoftc,
    buffer: *mut u8,
    buflen: u32,
    xfer: &'static UsbdXfer,
) -> UsbdStatus {
    let mut req = UsbDeviceRequest {
        bmRequestType: UT_WRITE_CLASS_INTERFACE,
        bRequest: UR_CBI_ADSC,
        ..UsbDeviceRequest::default()
    };
    usetw(&mut req.wValue, 0);
    usetw(&mut req.wIndex, sc.sc_ifaceno.get() as u16);
    usetw(&mut req.wLength, buflen as u16);
    sc.sc_req.set(req);
    // SAFETY: the caller's contract on `buffer`.
    unsafe { umass_setup_ctrl_transfer(sc, &req, buffer, buflen, 0, xfer) }
}

/// `SEND_DIAGNOSTIC_CMDLEN` of `umass_cbi_reset`.
const SEND_DIAGNOSTIC_CMDLEN: usize = 12;

/// `umass_cbi_reset`: the Command Block Reset Protocol, ending with `status`.
pub fn umass_cbi_reset(sc: &'static UmassSoftc, status: i32) {
    if usbd_is_dying(sc.udev()) {
        return;
    }

    // Command Block Reset Protocol
    //
    // First send a reset request to the device. Then clear any possibly stalled bulk
    // endpoints.
    //
    // This is done in 3 steps, states:
    // TSTATE_CBI_RESET1
    // TSTATE_CBI_RESET2
    // TSTATE_CBI_RESET3
    //
    // If the reset doesn't succeed, the device should be port reset.

    sc.transfer_state.set(TSTATE_CBI_RESET1);
    sc.transfer_status.set(status);

    // The 0x1d code is the SEND DIAGNOSTIC command. To distinguish between the two the last
    // 10 bytes of the cbl is filled with 0xff (section 2.2 of the CBI spec).
    let mut cbl = sc.cbl.get();
    cbl[0] = 0x1d; // Command Block Reset
    cbl[1] = 0x04;
    cbl[2..SEND_DIAGNOSTIC_CMDLEN].fill(0xff);
    sc.cbl.set(cbl);

    // SAFETY: the command block lives in the softc, which outlives the transfer.
    let _ = unsafe {
        umass_cbi_adsc(
            sc,
            sc.cbl.as_ptr().cast(),
            SEND_DIAGNOSTIC_CMDLEN as u32,
            sc.xfer(XFER_CBI_RESET1),
        )
    };
    // XXX if the command fails we should reset the port on the bub
}

/// `umass_cbi_transfer`: a CBI command.
///
/// # Safety
///
/// `umass_wire_xfer`'s contract.
#[allow(clippy::too_many_arguments)] // the C's signature
pub unsafe fn umass_cbi_transfer(
    sc: &'static UmassSoftc,
    lun: i32,
    cmd: &[u8],
    data: *mut u8,
    datalen: i32,
    dir: i32,
    timeout: u32,
    cb: UmassCallback,
    priv_: *mut c_void,
) {
    let _ = lun;

    if usbd_is_dying(sc.udev()) {
        sc.polled_xfer_status.set(USBD_IOERROR);
        return;
    }

    // Be a little generous.
    sc.timeout.set(timeout.wrapping_add(USBD_DEFAULT_TIMEOUT));

    // Do a CBI transfer with cmdlen bytes from cmd, possibly a data phase of datalen bytes
    // from/to the device and finally a csw read phase. If the data direction was inbound a
    // maximum of datalen bytes is stored in the buffer pointed to by data.
    //
    // umass_cbi_transfer initialises the transfer and lets the state machine in
    // umass_cbi_state handle the completion. It uses the following states:
    // TSTATE_CBI_COMMAND
    //   -> XXX fill in
    //
    // An error in any of those states will invoke umass_cbi_reset.

    // store the details for the data transfer phase
    sc.transfer_dir.set(dir);
    sc.transfer_data.set(data);
    sc.transfer_datalen.set(datalen);
    sc.transfer_actlen.set(0);
    sc.transfer_cb.set(Some(cb));
    sc.transfer_priv.set(priv_);
    sc.transfer_status.set(STATUS_CMD_OK);

    // move from idle to the command state
    sc.transfer_state.set(TSTATE_CBI_COMMAND);

    // Send the Command Block from host to device via control endpoint.
    let mut cbw = sc.cbw.get();
    let cmdlen = cmd.len().min(CBWCDBLENGTH);
    cbw.bCDBLength = cmdlen as u8;
    cbw.CBWCDB = [0; CBWCDBLENGTH];
    cbw.CBWCDB[..cmdlen].copy_from_slice(&cmd[..cmdlen]);
    sc.cbw.set(cbw);
    umass_adjust_transfer(sc);
    // SAFETY: the command block lives in the softc, which outlives the transfer.
    let err = unsafe {
        umass_cbi_adsc(
            sc,
            umass_cbwcdb_ptr(sc),
            u32::from(sc.cbw.get().bCDBLength),
            sc.xfer(XFER_CBI_CB),
        )
    };
    if err.is_err() {
        umass_cbi_reset(sc, STATUS_WIRE_FAILED);
    }

    if sc.udev().bus().use_polling.get() != 0 {
        sc.polled_xfer_status.set(err);
    }
}

/// `umass_cbi_state`: the CBI state machine, every CBI xfer's callback.
pub fn umass_cbi_state(xfer: &'static UsbdXfer, priv_: *mut c_void, err: UsbdStatus) {
    let sc = umass_priv_softc(priv_);

    if usbd_is_dying(sc.udev()) {
        return;
    }

    // State handling for CBI transfers.

    let mut case = sc.transfer_state.get();
    loop {
        match case {
            /* CBI Transfer */
            TSTATE_CBI_COMMAND => {
                if err == USBD_STALLED {
                    // Status transport by control pipe (section 2.3.2.1). The command
                    // contained in the command block failed.
                    //
                    // The control pipe has already been unstalled by the USB stack.
                    // Section 2.4.3.1.1 states that the bulk in endpoints should not
                    // stalled at this point.

                    sc.transfer_state.set(TSTATE_IDLE);
                    umass_transfer_cb(sc, sc.transfer_datalen.get(), STATUS_CMD_FAILED);

                    return;
                } else if err.is_err() {
                    umass_cbi_reset(sc, STATUS_WIRE_FAILED);
                    return;
                }

                // Data transport phase, setup transfer
                sc.transfer_state.set(TSTATE_CBI_DATA);
                if sc.transfer_dir.get() == DIR_IN || sc.transfer_dir.get() == DIR_OUT {
                    if !umass_start_data(sc, sc.xfer(XFER_CBI_DATA)) {
                        umass_cbi_reset(sc, STATUS_WIRE_FAILED);
                    }
                    return;
                }
                // no data phase

                // FALLTHROUGH if no data phase, err == 0
                case = TSTATE_CBI_DATA;
            }
            TSTATE_CBI_DATA => {
                // Command transport phase error handling (ignored if no data phase
                // (fallthrough from previous state))
                if sc.transfer_dir.get() != DIR_NONE {
                    // retrieve the length of the transfer that was done
                    let mut actlen = 0u32;
                    usbd_get_xfer_status(xfer, None, None, Some(&mut actlen), None);
                    sc.transfer_actlen.set(actlen as i32);

                    if err.is_err() {
                        if err == USBD_STALLED {
                            sc.transfer_state.set(TSTATE_CBI_DCLEAR);
                            let endpt = if sc.transfer_dir.get() == DIR_IN {
                                UMASS_BULKIN
                            } else {
                                UMASS_BULKOUT
                            };
                            umass_clear_endpoint_stall(sc, endpt, sc.xfer(XFER_CBI_DCLEAR));
                        } else {
                            // Unless the error is a pipe stall the error is fatal.
                            umass_cbi_reset(sc, STATUS_WIRE_FAILED);
                        }
                        return;
                    }
                }

                if sc.transfer_dir.get() == DIR_IN {
                    umass_copy_from_dma(sc, sc.transfer_actlen.get());
                }

                // Status phase
                if sc.sc_wire.get() == UMASS_WPROTO_CBI_I {
                    sc.transfer_state.set(TSTATE_CBI_STATUS);
                    sc.sbl.set(UmassCbiSbl::default());
                    // SAFETY: the status block lives in the softc, which outlives the
                    // transfer; the stack copies the answer into it at the end.
                    let e = unsafe {
                        umass_setup_transfer(
                            sc,
                            sc.pipe(UMASS_INTRIN),
                            sc.sbl.as_ptr().cast(),
                            size_of::<UmassCbiSbl>() as u32,
                            0, // fixed length transfer
                            sc.xfer(XFER_CBI_STATUS),
                        )
                    };
                    if e.is_err() {
                        umass_cbi_reset(sc, STATUS_WIRE_FAILED);
                    }
                } else {
                    // No command completion interrupt. Request sense to get status of
                    // command.
                    sc.transfer_state.set(TSTATE_IDLE);
                    umass_transfer_cb(
                        sc,
                        sc.transfer_datalen.get() - sc.transfer_actlen.get(),
                        STATUS_CMD_UNKNOWN,
                    );
                }
                return;
            }
            TSTATE_CBI_STATUS => {
                umass_cbi_status(sc, xfer, err);
                return;
            }
            TSTATE_CBI_DCLEAR => {
                if err.is_err() {
                    // should not occur
                    printf(format_args!(
                        "{}: CBI bulk-in/out stall clear failed, {}\n",
                        sc.sc_dev.xname(),
                        usbd_errstr(err)
                    ));
                    umass_cbi_reset(sc, STATUS_WIRE_FAILED);
                } else {
                    sc.transfer_state.set(TSTATE_IDLE);
                    umass_transfer_cb(sc, sc.transfer_datalen.get(), STATUS_CMD_FAILED);
                }
                return;
            }
            TSTATE_CBI_SCLEAR => {
                if err.is_err() {
                    // should not occur
                    printf(format_args!(
                        "{}: CBI intr-in stall clear failed, {}\n",
                        sc.sc_dev.xname(),
                        usbd_errstr(err)
                    ));
                    umass_cbi_reset(sc, STATUS_WIRE_FAILED);
                } else {
                    sc.transfer_state.set(TSTATE_IDLE);
                    umass_transfer_cb(sc, sc.transfer_datalen.get(), STATUS_CMD_FAILED);
                }
                return;
            }

            /* CBI Reset */
            TSTATE_CBI_RESET1 => {
                if err.is_err() {
                    printf(format_args!(
                        "{}: CBI reset failed, {}\n",
                        sc.sc_dev.xname(),
                        usbd_errstr(err)
                    ));
                }

                sc.transfer_state.set(TSTATE_CBI_RESET2);
                umass_clear_endpoint_stall(sc, UMASS_BULKIN, sc.xfer(XFER_CBI_RESET2));

                return;
            }
            TSTATE_CBI_RESET2 => {
                if err.is_err() {
                    // should not occur
                    printf(format_args!(
                        "{}: CBI bulk-in stall clear failed, {}\n",
                        sc.sc_dev.xname(),
                        usbd_errstr(err)
                    ));
                }
                // no error recovery, otherwise we end up in a loop

                sc.transfer_state.set(TSTATE_CBI_RESET3);
                umass_clear_endpoint_stall(sc, UMASS_BULKOUT, sc.xfer(XFER_CBI_RESET3));

                return;
            }
            TSTATE_CBI_RESET3 => {
                if err.is_err() {
                    // should not occur
                    printf(format_args!(
                        "{}: CBI bulk-out stall clear failed, {}\n",
                        sc.sc_dev.xname(),
                        usbd_errstr(err)
                    ));
                }
                // no error recovery, otherwise we end up in a loop

                sc.transfer_state.set(TSTATE_IDLE);
                if !sc.transfer_priv.get().is_null() {
                    umass_transfer_cb(sc, sc.transfer_datalen.get(), sc.transfer_status.get());
                }

                return;
            }

            /* Default */
            state => panic(format_args!(
                "{}: Unknown state {}",
                sc.sc_dev.xname(),
                state
            )),
        }
    }
}

/// The `TSTATE_CBI_STATUS` case of `umass_cbi_state`: dissects the interrupt status block
/// and ends the command.
fn umass_cbi_status(sc: &'static UmassSoftc, xfer: &'static UsbdXfer, err: UsbdStatus) {
    if err.is_err() {
        // Status transport by interrupt pipe (section 2.3.2.2).

        if err == USBD_STALLED {
            sc.transfer_state.set(TSTATE_CBI_SCLEAR);
            umass_clear_endpoint_stall(sc, UMASS_INTRIN, sc.xfer(XFER_CBI_SCLEAR));
        } else {
            umass_cbi_reset(sc, STATUS_WIRE_FAILED);
        }
        return;
    }

    // Dissect the information in the buffer

    let mut actlen = 0u32;
    usbd_get_xfer_status(xfer, None, None, Some(&mut actlen), None);
    if actlen != 2 {
        return;
    }

    let sbl = sc.sbl.get();
    let residue = sc.transfer_datalen.get() - sc.transfer_actlen.get();
    if sc.sc_cmd.get() == UMASS_CPROTO_UFI {
        // Section 3.4.3.1.3 specifies that the UFI command protocol returns an ASC and ASCQ
        // in the interrupt data block.

        let status = if (sbl.ufi_asc() == 0 && sbl.ufi_ascq() == 0) || sc.sc_sense.get() != 0 {
            STATUS_CMD_OK
        } else {
            STATUS_CMD_FAILED
        };

        // No autosense, command successful
        sc.transfer_state.set(TSTATE_IDLE);
        umass_transfer_cb(sc, residue, status);
    } else {
        // Command Interrupt Data Block

        if sbl.common_type() == IDB_TYPE_CCI {
            let status = match sbl.common_value() & IDB_VALUE_STATUS_MASK {
                IDB_VALUE_PASS => STATUS_CMD_OK,
                IDB_VALUE_FAIL | IDB_VALUE_PERSISTENT => STATUS_CMD_FAILED,
                // IDB_VALUE_PHASE and anything else
                _ => STATUS_WIRE_FAILED,
            };

            sc.transfer_state.set(TSTATE_IDLE);
            umass_transfer_cb(sc, residue, status);
        }
    }
}

/// `umass_bbb_get_max_lun`: the Get Max LUN request; 0 when the device does not support
/// it.
pub fn umass_bbb_get_max_lun(sc: &'static UmassSoftc) -> u8 {
    let mut buf = [0u8; 1];

    // The Get Max Lun command is a class-specific request.
    let mut req = UsbDeviceRequest {
        bmRequestType: UT_READ_CLASS_INTERFACE,
        bRequest: UR_BBB_GET_MAX_LUN,
        ..UsbDeviceRequest::default()
    };
    usetw(&mut req.wValue, 0);
    usetw(&mut req.wIndex, sc.sc_ifaceno.get() as u16);
    usetw(&mut req.wLength, 1);

    let err = usbd_do_request_flags(
        sc.udev(),
        &req,
        &mut buf,
        USBD_SHORT_XFER_OK,
        None,
        USBD_DEFAULT_TIMEOUT,
    );

    match err {
        USBD_NORMAL_COMPLETION => buf[0],
        // XXX Should we port_reset the device?
        _ => 0,
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use std::boxed::Box;

    fn softc(cmd: u8) -> &'static UmassSoftc {
        // SAFETY: all-zero is a valid `UmassSoftc` (the `Softc` contract).
        let sc: &'static UmassSoftc = Box::leak(Box::new(unsafe { core::mem::zeroed() }));
        sc.sc_cmd.set(cmd);
        sc
    }

    #[test]
    fn ufi_trims_the_inquiry_and_pads_the_command() {
        let sc = softc(UMASS_CPROTO_UFI);
        let mut cbw = UmassBbbCbw::default();
        cbw.bCDBLength = 6;
        cbw.CBWCDB[0] = INQUIRY;
        cbw.CBWCDB[4] = 255;
        sc.cbw.set(cbw);
        sc.transfer_datalen.set(255);
        umass_adjust_transfer(sc);
        assert_eq!(sc.transfer_datalen.get(), 36);
        assert_eq!(sc.cbw.get().CBWCDB[4], 36);
        assert_eq!(sc.cbw.get().bCDBLength, UFI_COMMAND_LENGTH);
    }

    #[test]
    fn atapi_pads_and_scsi_keeps_the_command() {
        let sc = softc(UMASS_CPROTO_ATAPI);
        sc.cbw.set(UmassBbbCbw {
            bCDBLength: 10,
            ..UmassBbbCbw::default()
        });
        umass_adjust_transfer(sc);
        assert_eq!(sc.cbw.get().bCDBLength, 12);
        let sc = softc(UMASS_CPROTO_SCSI);
        sc.cbw.set(UmassBbbCbw {
            bCDBLength: 10,
            ..UmassBbbCbw::default()
        });
        umass_adjust_transfer(sc);
        assert_eq!(sc.cbw.get().bCDBLength, 10);
    }

    #[test]
    fn copies_stay_within_the_transfer() {
        let sc = softc(UMASS_CPROTO_SCSI);
        sc.transfer_datalen.set(512);
        assert_eq!(umass_copy_len(sc, 100), 100);
        assert_eq!(umass_copy_len(sc, 4096), 512);
        assert_eq!(umass_copy_len(sc, -1), 0);
        assert_eq!(offset_of!(UmassBbbCbw, CBWCDB), 15);
    }
}
/* </TESTS> */
