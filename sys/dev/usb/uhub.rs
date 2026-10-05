/*	$OpenBSD: uhub.c,v 1.99 2026/02/20 12:32:34 sthen Exp $ */
/*	$NetBSD: uhub.c,v 1.64 2003/02/08 03:32:51 ichiro Exp $	*/
/*	$FreeBSD: src/sys/dev/usb/uhub.c,v 1.18 1999/11/17 22:33:43 n_hibma Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1998 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Lennart Augustsson (lennart@augustsson.net) at
 * Carlstedt Research & Technology.
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
/* </LICENSES> */

//! uhub(4): the USB hub driver, for root hubs (`uhub* at usb?`) and external hubs
//! (`uhub* at uhub?`).
//!
//! Upstream: sys/dev/usb/uhub.c @ 3ce1f3f79392
//!
//! The attach powers every port and opens the hub's interrupt pipe; the interrupt callback
//! records which ports changed and asks the USB task thread to explore. `uhub_explore` reads
//! the changed ports' status, clears the change bits, resets a newly connected port and
//! enumerates its device (`usbd_new_device`), detaches the device of a port that lost it,
//! and recurses into the hubs below.
//!
//! ## Deviations
//! - `UHUB_DEBUG` is not in GENERIC: the `DPRINTF`s and the debug attach line are absent.
//! - The port array and the transaction translators are allocated zeroed (`M_ZERO`): the C
//!   leaves members it does not set (`usbd_port.status`) uninitialised, which Rust may not
//!   read.
//! - The status bits are kept with checked shifts: the C's `1 << port` and `buffer[i] << (i *
//!   8)` are undefined beyond 31 bits (a hub of more than 31 ports); such bits are dropped.
//! - `uhub_explore` returns the errno as an `i32` (`usbd_hub.explore`'s type);
//!   `uhub_port_connect` returns `false` for the C's -1.
//! - As in C, a failure after the interrupt pipe is open (the TT allocation) frees the status
//!   buffer without closing the pipe.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};

use crate::dev::usb::usb::usb_needs_explore;
use crate::dev::usb::usb::{
    UDCLASS_HUB, UDPROTO_FSHUB, UDPROTO_HSHUBSTT, UE_INTERRUPT, UHD_PWRON_FACTOR, UHD_TT_THINK,
    UHF_C_BH_PORT_RESET, UHF_C_PORT_CONNECTION, UHF_C_PORT_ENABLE, UHF_C_PORT_LINK_STATE,
    UHF_C_PORT_RESET, UHF_PORT_ENABLE, UHF_PORT_POWER, UPS_C_BH_PORT_RESET, UPS_C_CONNECT_STATUS,
    UPS_C_PORT_ENABLED, UPS_C_PORT_LINK_STATE, UPS_C_PORT_RESET, UPS_CURRENT_CONNECT_STATUS,
    UPS_HIGH_SPEED, UPS_LOW_SPEED, UPS_PORT_ENABLED, UPS_PORT_POWER, UPS_PORT_POWER_SS,
    USB_EXTRA_POWER_UP_TIME, USB_HUB_MAX_DEPTH, USB_MAX_POWER, USB_MIN_POWER,
    USB_PORT_POWERUP_DELAY, USB_POWER_DOWN_TIME, USB_SPEED_FULL, USB_SPEED_HIGH, USB_SPEED_LOW,
    USB_SPEED_SUPER, USBD_SHORT_XFER_OK, UsbHubDescriptor, UsbHubSsDescriptor, UsbPortStatus,
    UsbWire, ue_get_xfertype, ugetw,
};
use crate::dev::usb::usb_subr::{
    usbd_delay_ms, usbd_detach, usbd_errstr, usbd_new_device, usbd_reset_port,
};
use crate::dev::usb::usbdi::{
    UMATCH_DEVCLASS_DEVSUBCLASS, UMATCH_NONE, USBD_NORMAL_COMPLETION, USBD_STALLED, UsbAttachArg,
    UsbdStatus, usbd_clear_endpoint_stall_async, usbd_close_pipe, usbd_get_device_descriptor,
    usbd_interface2endpoint_descriptor, usbd_is_dying, usbd_open_pipe_intr,
};
use crate::dev::usb::usbdi_util::{
    usbd_clear_port_feature, usbd_get_hub_descriptor, usbd_get_hub_ss_descriptor,
    usbd_get_port_status, usbd_set_hub_depth, usbd_set_port_feature,
};
use crate::dev::usb::usbdivar::{
    USBD_RESTART_MAX, UsbdDevice, UsbdHub, UsbdPipe, UsbdPort, UsbdTt, UsbdXfer,
};
use crate::kern::kern_malloc::{free, malloc, mallocarray};
use crate::kern::subr_prf::{panic, printf};
use crate::sys::device::{CfMatch, Cfattach, Cfdriver, DV_DULL, Device, Softc};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_NOWAIT, M_USBDEV, M_ZERO};

/// `UHUB_INTR_INTERVAL`, in ms.
const UHUB_INTR_INTERVAL: i32 = 255;

/// `struct uhub_softc`.
#[repr(C)]
pub struct UhubSoftc {
    /// `sc_dev`: base device.
    pub sc_dev: Device,
    /// `sc_hub`: USB device.
    pub sc_hub: Cell<Option<&'static UsbdDevice>>,
    /// `sc_ipipe`: interrupt pipe.
    pub sc_ipipe: Cell<Option<&'static UsbdPipe>>,

    /// `sc_status`: status from last interrupt.
    pub sc_status: Cell<u32>,
    /// `sc_statusbuf`: per port status buffer.
    pub sc_statusbuf: Cell<*mut u8>,
    /// `sc_statuslen`: status bufferlen.
    pub sc_statuslen: Cell<usize>,

    /// `sc_running`.
    pub sc_running: Cell<u8>,
}

impl UhubSoftc {
    /// `sc->sc_hub`. Panics before the attach set it.
    fn hub_dev(&self) -> &'static UsbdDevice {
        match self.sc_hub.get() {
            Some(d) => d,
            None => panic(format_args!("{}: no hub device", self.sc_dev.xname())),
        }
    }
}

// SAFETY: `#[repr(C)]` with the device first; the other members are `Cell`s of `Option`s of
// references, integers and a raw pointer: all valid as zero bits.
unsafe impl Softc for UhubSoftc {}

/// `UHUB_PROTO(sc)`.
fn uhub_proto(sc: &UhubSoftc) -> u8 {
    sc.hub_dev().ddesc.get().bDeviceProtocol
}

/// `UHUB_IS_HIGH_SPEED(sc)`.
fn uhub_is_high_speed(sc: &UhubSoftc) -> bool {
    uhub_proto(sc) != UDPROTO_FSHUB
}

/// `UHUB_IS_SINGLE_TT(sc)`.
fn uhub_is_single_tt(sc: &UhubSoftc) -> bool {
    uhub_proto(sc) == UDPROTO_HSHUBSTT
}

/// `uhub_cd`.
pub static UHUB_CD: Cfdriver = Cfdriver::new(b"uhub", DV_DULL, 0);

/// `uhub_ca`: hub to usb.
pub static UHUB_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<UhubSoftc>(),
    ca_match: Some(uhub_match),
    ca_attach: uhub_attach,
    ca_detach: Some(uhub_detach),
    ca_activate: None,
};

/// `uhub_uhub_ca`: hub to hub.
pub static UHUB_UHUB_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<UhubSoftc>(),
    ca_match: Some(uhub_match),
    ca_attach: uhub_attach,
    ca_detach: Some(uhub_detach),
    ca_activate: None,
};

/// `(struct uhub_softc *)self`.
fn uhub_softc(self_: &Device) -> &'static UhubSoftc {
    // SAFETY: only called with devices `uhub_ca`/`uhub_uhub_ca` made (uhub's own entry
    // points); attached devices live until `config_detach` frees them after `uhub_detach`.
    unsafe { &*ptr::from_ref(self_.softc::<UhubSoftc>()) }
}

/// `uhub_match`: a device of the hub class (any subclass: hubs report 0 or 1).
pub fn uhub_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: `usbd_probe_and_attach` hands its `usb_attach_arg`, valid during the match.
    let uaa = unsafe { &*aux.cast::<UsbAttachArg>() };
    let dd = usbd_get_device_descriptor(uaa.device());

    if uaa.iface.is_none() {
        return UMATCH_NONE;
    }

    // The subclass for hubs seems to be 0 for some and 1 for others, so we just ignore the
    // subclass.
    if dd.bDeviceClass == UDCLASS_HUB {
        return UMATCH_DEVCLASS_DEVSUBCLASS;
    }
    UMATCH_NONE
}

/// `uhub_attach`.
pub fn uhub_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    let sc = uhub_softc(self_);
    // SAFETY: as in `uhub_match`, valid during the attach.
    let uaa = unsafe { &*aux.cast::<UsbAttachArg>() };
    let dev = uaa.device();
    let iface = uaa.iface;
    let xname = sc.sc_dev.xname();
    let mut ttthink: u16 = 0;

    sc.sc_hub.set(Some(dev));

    if dev.depth.get() > USB_HUB_MAX_DEPTH {
        printf(format_args!(
            "{}: hub depth ({}) exceeded, hub ignored\n",
            xname, USB_HUB_MAX_DEPTH
        ));
        return;
    }

    // Super-Speed hubs need to know their depth to be able to parse the bits of the
    // route-string that correspond to their downstream port number.
    //
    // This does not apply to root hubs.
    if dev.depth.get() != 0
        && dev.speed.get() == USB_SPEED_SUPER
        && usbd_set_hub_depth(dev, i32::from(dev.depth.get()) - 1).is_err()
    {
        printf(format_args!("{}: unable to set HUB depth\n", xname));
        return;
    }

    // Get hub descriptor.
    let (err, nports, powerdelay) = if dev.speed.get() == USB_SPEED_SUPER {
        let mut ss = UsbHubSsDescriptor::zeroed();
        let err = usbd_get_hub_ss_descriptor(dev, &mut ss, 1);
        let nports = ss.bNbrPorts;
        let powerdelay = i32::from(ss.bPwrOn2PwrGood) * UHD_PWRON_FACTOR;
        if !err.is_err() && nports > 7 {
            let _ = usbd_get_hub_ss_descriptor(dev, &mut ss, nports);
        }
        (err, nports, powerdelay)
    } else {
        let mut hs = UsbHubDescriptor::zeroed();
        let err = usbd_get_hub_descriptor(dev, &mut hs, 1);
        let nports = hs.bNbrPorts;
        let powerdelay = i32::from(hs.bPwrOn2PwrGood) * UHD_PWRON_FACTOR;
        ttthink = ugetw(hs.wHubCharacteristics) & UHD_TT_THINK;
        if !err.is_err() && nports > 7 {
            let _ = usbd_get_hub_descriptor(dev, &mut hs, nports);
        }
        (err, nports, powerdelay)
    };

    if err.is_err() {
        return;
    }

    let nports = usize::from(nports);
    let mut hub: Option<&'static UsbdHub> = None;

    'bad: {
        if nports == 0 {
            printf(format_args!("{}: no ports, hub ignored\n", xname));
            break 'bad;
        }

        let Some(hmem) = malloc(size_of::<UsbdHub>(), M_USBDEV, M_NOWAIT) else {
            return;
        };
        let Some(ports) = mallocarray(nports, size_of::<UsbdPort>(), M_USBDEV, M_NOWAIT | M_ZERO)
        else {
            free(hmem, M_USBDEV, size_of::<UsbdHub>());
            return;
        };
        let hp = hmem.cast::<UsbdHub>();
        // SAFETY: a fresh allocation of a `UsbdHub`'s size (malloc aligns for any type),
        // written whole before anybody sees it; it lives until `uhub_detach` or the `bad`
        // path below frees it after clearing `dev->hub`.
        let h: &'static UsbdHub = unsafe {
            hp.write(UsbdHub {
                explore: Cell::new(Some(uhub_explore)),
                hubsoftc: Cell::new(ptr::from_ref(sc).cast_mut().cast()),
                ports: Cell::new(ports.as_ptr().cast()),
                nports: Cell::new(nports as i32),
                powerdelay: Cell::new(powerdelay as u8),
                ttthink: Cell::new((ttthink >> 5) as u8),
                multi: Cell::new(u8::from(!uhub_is_single_tt(sc))),
            });
            &*hp.as_ptr()
        };
        hub = Some(h);
        dev.hub.set(Some(h));

        if dev.self_powered.get() == 0
            && let Some(src) = dev.powersrc.get()
            && let Some(parent) = src.parent.get()
            && parent.self_powered.get() == 0
        {
            printf(format_args!(
                "{}: bus powered hub connected to bus powered hub, ignored\n",
                xname
            ));
            break 'bad;
        }

        // Set up interrupt pipe.
        let Some(iface) = iface else {
            printf(format_args!("{}: no endpoint descriptor\n", xname));
            break 'bad;
        };
        let Some(ed) = usbd_interface2endpoint_descriptor(iface, 0) else {
            printf(format_args!("{}: no endpoint descriptor\n", xname));
            break 'bad;
        };
        if ue_get_xfertype(ed.bmAttributes) != UE_INTERRUPT {
            printf(format_args!("{}: bad interrupt endpoint\n", xname));
            break 'bad;
        }

        sc.sc_statuslen.set((nports + 1).div_ceil(8));
        let Some(buf) = malloc(sc.sc_statuslen.get(), M_USBDEV, M_NOWAIT) else {
            break 'bad;
        };
        sc.sc_statusbuf.set(buf.as_ptr());

        // SAFETY: the status buffer is `sc_statuslen` bytes, freed only after the pipe is
        // closed (`uhub_detach`).
        let r = unsafe {
            usbd_open_pipe_intr(
                iface,
                ed.bEndpointAddress,
                USBD_SHORT_XFER_OK as u8,
                ptr::from_ref(sc).cast_mut().cast(),
                buf.as_ptr(),
                sc.sc_statuslen.get() as u32,
                uhub_intr,
                UHUB_INTR_INTERVAL,
            )
        };
        match r {
            Ok(p) => sc.sc_ipipe.set(Some(p)),
            Err(_) => {
                printf(format_args!("{}: cannot open interrupt pipe\n", xname));
                break 'bad;
            }
        }

        // Wait with power off for a while.
        usbd_delay_ms(dev, USB_POWER_DOWN_TIME);

        // To have the best chance of success we do things in the exact same order as
        // Windoze98. This should not be necessary, but some devices do not follow the USB
        // specs to the letter.
        //
        // These are the events on the bus when a hub is attached:
        //  Get device and config descriptors (see attach code)
        //  Get hub descriptor (see above)
        //  For all ports
        //     turn on power
        //     wait for power to become stable
        // (all below happens in explore code)
        //  For all ports
        //     clear C_PORT_CONNECTION
        //  For all ports
        //     get port status
        //     if device connected
        //        wait 100 ms
        //        turn on reset
        //        wait
        //        clear C_PORT_RESET
        //        get port status
        //        proceed with device attachment

        let mut tts: *mut UsbdTt = ptr::null_mut();
        if uhub_is_high_speed(sc) {
            let n = if uhub_is_single_tt(sc) { 1 } else { nports };
            let Some(t) = mallocarray(n, size_of::<UsbdTt>(), M_USBDEV, M_NOWAIT | M_ZERO) else {
                break 'bad;
            };
            tts = t.as_ptr().cast();
        }
        // Set up data structures
        for (p, up) in h.ports().iter().enumerate() {
            up.device.set(None);
            up.parent.set(Some(dev));
            up.portno.set((p + 1) as u8);
            if dev.self_powered.get() != 0 {
                // Self powered hub, give ports maximum current.
                up.power.set(USB_MAX_POWER);
            } else {
                up.power.set(USB_MIN_POWER);
            }
            up.restartcnt.set(0);
            up.reattach.set(0);
            if uhub_is_high_speed(sc) {
                let i = if uhub_is_single_tt(sc) { 0 } else { p };
                // SAFETY: `tts` holds `nports` zeroed TTs (one when single), valid as zero
                // bits, freed by `uhub_detach` only.
                let tt: &'static UsbdTt = unsafe { &*tts.add(i) };
                up.tt.set(Some(tt));
                tt.hub.set(Some(h));
                tt.hcpriv.set(ptr::null_mut());
            } else {
                up.tt.set(None);
            }
        }

        for port in 1..=nports as i32 {
            // Turn the power on.
            let err = usbd_set_port_feature(dev, port, UHF_PORT_POWER);
            if err.is_err() {
                printf(format_args!(
                    "{}: port {} power on failed, {}\n",
                    xname,
                    port,
                    usbd_errstr(err)
                ));
            }
            // Make sure we check the port status at least once.
            sc.sc_status
                .set(sc.sc_status.get() | 1u32.checked_shl(port as u32).unwrap_or(0));
        }

        // Wait for stable power.
        if dev.powersrc.get().and_then(|p| p.parent.get()).is_some() {
            usbd_delay_ms(dev, (powerdelay + USB_EXTRA_POWER_UP_TIME as i32) as u32);
        }

        // The usual exploration will finish the setup.

        sc.sc_running.set(1);

        return;
    }

    // bad:
    if let Some(b) = NonNull::new(sc.sc_statusbuf.get()) {
        free(b, M_USBDEV, sc.sc_statuslen.get());
        sc.sc_statusbuf.set(ptr::null_mut());
    }
    if let Some(h) = hub {
        let n = h.nports.get() as usize;
        if let Some(p) = NonNull::new(h.ports.get()) {
            free(p.cast(), M_USBDEV, n * size_of::<UsbdPort>());
        }
        dev.hub.set(None);
        free(NonNull::from(h).cast(), M_USBDEV, size_of::<UsbdHub>());
    }
    dev.hub.set(None);
}

/// `uhub_explore`: the hub's `explore`, run by the USB task thread.
pub fn uhub_explore(dev: &'static UsbdDevice) -> i32 {
    let hub = match dev.hub.get() {
        Some(h) => h,
        None => panic(format_args!("uhub_explore: not a hub")),
    };
    // SAFETY: `uhub_attach` points `hubsoftc` at its softc, which outlives the hub structure
    // (`uhub_detach` frees the structure).
    let sc: &'static UhubSoftc = unsafe { &*hub.hubsoftc.get().cast::<UhubSoftc>() };
    let xname = sc.sc_dev.xname();
    let shub = sc.hub_dev();

    if usbd_is_dying(shub) {
        return Errno::EIO as i32;
    }

    if sc.sc_running.get() == 0 {
        return Errno::ENXIO as i32;
    }

    // Ignore hubs that are too deep.
    if shub.depth.get() > USB_HUB_MAX_DEPTH {
        return Errno::EOPNOTSUPP as i32;
    }

    let ports = match shub.hub.get() {
        Some(h) => h.ports(),
        None => &[],
    };
    for (i, up) in ports.iter().enumerate() {
        let port = i as i32 + 1;
        let bit = 1u32.checked_shl(port as u32).unwrap_or(0);

        let mut change: u16 = 0;
        let mut status: u16 = 0;

        if (sc.sc_status.get() & bit) != 0 || up.reattach.get() != 0 {
            sc.sc_status.set(sc.sc_status.get() & !bit);

            let mut ps = UsbPortStatus::zeroed();
            if usbd_get_port_status(dev, port, &mut ps).is_err() {
                continue;
            }
            up.status.set(ps);

            status = ugetw(ps.wPortStatus);
            change = ugetw(ps.wPortChange);
        }

        if up.reattach.get() != 0 {
            change |= UPS_C_CONNECT_STATUS;
            up.reattach.set(0);
        }

        if change & UPS_C_PORT_ENABLED != 0 {
            let _ = usbd_clear_port_feature(shub, port, UHF_C_PORT_ENABLE);
            if change & UPS_C_CONNECT_STATUS != 0 {
                // Ignore the port error if the device vanished.
            } else if status & UPS_PORT_ENABLED != 0 {
                printf(format_args!(
                    "{}: illegal enable change, port {}\n",
                    xname, port
                ));
            } else {
                // Port error condition.
                if up.restartcnt.get() != 0 {
                    // no message first time
                    printf(format_args!(
                        "{}: port error, restarting port {}\n",
                        xname, port
                    ));
                }

                let cnt = up.restartcnt.get();
                up.restartcnt.set(cnt.wrapping_add(1));
                if cnt < USBD_RESTART_MAX {
                    change |= UPS_C_CONNECT_STATUS;
                } else {
                    printf(format_args!(
                        "{}: port error, giving up port {}\n",
                        xname, port
                    ));
                }
            }
        }

        if change & UPS_C_PORT_RESET != 0 {
            let _ = usbd_clear_port_feature(shub, port, UHF_C_PORT_RESET);
            change |= UPS_C_CONNECT_STATUS;
        }

        if change & UPS_C_BH_PORT_RESET != 0 && shub.speed.get() == USB_SPEED_SUPER {
            let _ = usbd_clear_port_feature(shub, port, UHF_C_BH_PORT_RESET);
        }

        if change & UPS_C_CONNECT_STATUS != 0 {
            if !uhub_port_connect(sc, port, status) {
                continue;
            }

            // The port set up succeeded, reset error count.
            up.restartcnt.set(0);
        }

        if change & UPS_C_PORT_LINK_STATE != 0 {
            let _ = usbd_clear_port_feature(shub, port, UHF_C_PORT_LINK_STATE);
        }

        // Recursive explore.
        if let Some(d) = up.device.get()
            && let Some(h) = d.hub.get()
        {
            h.explore(d);
        }
    }

    0
}

/// `uhub_detach`: called from process context when the hub is gone; detaches all devices
/// on active ports.
pub fn uhub_detach(self_: &Device, _flags: i32) -> Result<(), Errno> {
    let sc = uhub_softc(self_);
    let Some(shub) = sc.sc_hub.get() else {
        return Ok(());
    };
    let Some(hub) = shub.hub.get() else {
        // Must be partially working
        return Ok(());
    };

    if let Some(p) = sc.sc_ipipe.get() {
        // SAFETY: the pipe `uhub_attach` opened; nothing uses it after this.
        unsafe { usbd_close_pipe(NonNull::from(p)) };
        sc.sc_ipipe.set(None);
    }

    for rup in hub.ports() {
        if let Some(d) = rup.device.get() {
            let _ = usbd_detach(d, self_);
            rup.device.set(None);
        }
    }

    let nports = hub.nports.get() as usize;
    if let Some(tt) = hub.ports().first().and_then(|p| p.tt.get()) {
        let n = if uhub_is_single_tt(sc) { 1 } else { nports };
        free(NonNull::from(tt).cast(), M_USBDEV, n * size_of::<UsbdTt>());
    }
    if let Some(b) = NonNull::new(sc.sc_statusbuf.get()) {
        free(b, M_USBDEV, sc.sc_statuslen.get());
        sc.sc_statusbuf.set(ptr::null_mut());
    }
    shub.hub.set(None);
    if let Some(p) = NonNull::new(hub.ports.get()) {
        free(p.cast(), M_USBDEV, nports * size_of::<UsbdPort>());
    }
    free(NonNull::from(hub).cast(), M_USBDEV, size_of::<UsbdHub>());

    Ok(())
}

/// `uhub_intr`: an indication that some port has changed status. Remember the ports that
/// need attention and notify the USB task thread that we need to be explored again.
pub fn uhub_intr(xfer: &'static UsbdXfer, addr: *mut c_void, status: UsbdStatus) {
    // SAFETY: the pipe's callback argument is the softc (`uhub_attach`), alive until the
    // pipe is closed.
    let sc: &UhubSoftc = unsafe { &*addr.cast::<UhubSoftc>() };
    let mut stats: u32 = 0;

    if usbd_is_dying(sc.hub_dev()) {
        return;
    }

    if status == USBD_STALLED {
        if let Some(p) = sc.sc_ipipe.get() {
            let _ = usbd_clear_endpoint_stall_async(p);
        }
    } else if status == USBD_NORMAL_COMPLETION {
        let buf = xfer.buffer.get();
        let n = (xfer.actlen.get() as usize).min(sc.sc_statuslen.get());
        for i in 0..n {
            // SAFETY: the xfer's buffer is the softc's status buffer of `sc_statuslen`
            // bytes (`usbd_open_pipe_intr` in `uhub_attach`), and `i` is below it.
            let b = u32::from(unsafe { buf.add(i).read() });
            stats |= b.checked_shl((i * 8) as u32).unwrap_or(0);
        }
        sc.sc_status.set(sc.sc_status.get() | stats);

        usb_needs_explore(sc.hub_dev(), false);
    }
}

/// `uhub_port_connect`: handles a connect status change on `port`; `false` for the C's -1
/// (the port needs another look).
fn uhub_port_connect(sc: &'static UhubSoftc, port: i32, status: u16) -> bool {
    let shub = sc.hub_dev();
    let xname = sc.sc_dev.xname();
    let Some(up) = shub
        .hub
        .get()
        .and_then(|h| h.ports().get((port - 1) as usize))
    else {
        return false;
    };
    let mut status = status;

    // We have a connect status change, handle it.
    let _ = usbd_clear_port_feature(shub, port, UHF_C_PORT_CONNECTION);

    // If there is already a device on the port the change status must mean that is has
    // disconnected. Looking at the current connect status is not enough to figure this out
    // since a new unit may have been connected before we handle the disconnect.
    if let Some(d) = up.device.get() {
        // Disconnected
        let _ = usbd_detach(d, &sc.sc_dev);
        up.device.set(None);
    }

    // Nothing connected, just ignore it.
    if status & UPS_CURRENT_CONNECT_STATUS == 0 {
        return true;
    }

    // Connected
    if status & (UPS_PORT_POWER | UPS_PORT_POWER_SS) == 0 {
        printf(format_args!(
            "{}: connected port {} has no power\n",
            xname, port
        ));
        return false;
    }

    // Wait for maximum device power up time.
    usbd_delay_ms(shub, USB_PORT_POWERUP_DELAY);

    // Reset port, which implies enabling it.
    if usbd_reset_port(shub, port).is_err() {
        printf(format_args!("{}: port {} reset failed\n", xname, port));
        return false;
    }
    // Get port status again, it might have changed during reset
    let mut ps = UsbPortStatus::zeroed();
    if usbd_get_port_status(shub, port, &mut ps).is_err() {
        return false;
    }
    up.status.set(ps);

    status = ugetw(ps.wPortStatus);

    // Nothing connected, just ignore it.
    if status & UPS_CURRENT_CONNECT_STATUS == 0 {
        return false;
    }

    // Figure out device speed. This is a bit tricky because UPS_PORT_POWER_SS and
    // UPS_LOW_SPEED share the same bit.
    if status & UPS_PORT_POWER == 0 {
        status &= !UPS_PORT_POWER_SS;
    }

    let mut speed = if status & UPS_HIGH_SPEED != 0 {
        USB_SPEED_HIGH
    } else if status & UPS_LOW_SPEED != 0 {
        USB_SPEED_LOW
    } else if status & UPS_PORT_POWER != 0 {
        // If there is no power bit set, it is certainly a Super Speed device, so use the
        // speed of its parent hub.
        USB_SPEED_FULL
    } else {
        shub.speed.get()
    };

    // Reduce the speed, otherwise we won't setup the proper transfer methods.
    if speed > shub.speed.get() {
        speed = shub.speed.get();
    }

    // Get device info and set its address.
    if usbd_new_device(
        &sc.sc_dev,
        shub.bus(),
        i32::from(shub.depth.get()) + 1,
        i32::from(speed),
        port,
        up,
    )
    .is_err()
    {
        // The unit refused to accept a new address, or had some other serious problem.
        // Since we cannot leave at 0 we have to disable the port instead.
        printf(format_args!(
            "{}: device problem, disabling port {}\n",
            xname, port
        ));
        let _ = usbd_clear_port_feature(shub, port, UHF_PORT_ENABLE);

        return false;
    }

    true
}
