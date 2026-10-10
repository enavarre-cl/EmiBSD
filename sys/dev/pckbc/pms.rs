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

/*-
 * Copyright (c) 1994 Charles M. Hannum.
 * Copyright (c) 1992, 1993 Erik Forsberg.
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 *
 * THIS SOFTWARE IS PROVIDED BY ``AS IS'' AND ANY EXPRESS OR IMPLIED
 * WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.  IN
 * NO EVENT SHALL I BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL,
 * EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
 * PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR
 * PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF
 * LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING
 * NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
 * SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! `pms(4)`: the PS/2 mouse and touchpads on `pckbc(4)`'s auxiliary slot, feeding
//! `wsmouse(4)`: `dev/pckbc/pms.c`.
//!
//! Upstream: sys/dev/pckbc/pms.c @ 3ce1f3f79392
//!
//! The probe resets the device on the aux slot and wants `PMS_RSTDONE, 0` back. The attach
//! hooks [`pmsinput`] up as the slot's input handler, attaches a `wsmouse` child and walks
//! [`PMS_PROTOCOLS`] ([`pms_protocol_lookup`]): each protocol's `enable` sends its "knock"
//! (a sequence of resolution, scaling or rate commands the plain mouse ignores) and checks
//! the answer; the first one that answers wins, the plain PS/2 mouse is the fallback. The
//! Synaptics, ALPS and Elantech touchpads get a sub-softc of their own (`malloc`ed on the
//! first successful knock, kept afterwards), set themselves to absolute mode and describe
//! their hardware to `wsmouse`; those with a trackpoint (Synaptics pass-through, ALPS
//! Dualpoint, Elantech v4) attach a second `wsmouse` for it ([`PMS_SEC_ACCESSOPS`]). The
//! device stays disabled until a `wsmouse` opens: [`pms_change_state`], under
//! `sc_state_lock`, enables or disables the slot and the device and re-runs the protocol's
//! `enable` (or the lookup) on every enable and wake-up. Every byte reaches [`pmsinput`],
//! which collects a packet of the protocol's size, checks the protocol's sync bits and hands
//! the packet to its `proc`, and watches for the reset announcement (`AA 00`) a touchpad
//! sends after a power failure ([`pms_reset_detect`]): 100 ms later the `systq` task
//! disables and enables the device again.
//!
//! In QEMU the PS/2 mouse answers the IntelliMouse knock (id 3), so `pms0` runs the
//! IntelliMouse protocol (four-byte packets with a wheel); the touchpad knocks fail.
//!
//! ## Deviations
//! - The `pms_*` command helpers return the `pckbc` errno as `Result<(), Errno>`
//!   ([`pms_get_devid`] and [`synaptics_query`] return their answer in the `Ok`). Where the
//!   C returns -1 for an answer it does not accept (a knock that finds another device, an
//!   unknown model or firmware, a mode that did not take), the port returns `ENXIO`; every
//!   caller only tests for failure, as in the C.
//! - The protocols' `enable` returns `bool` (the C's 1 or 0) and `sync` returns `true` when
//!   the byte fits the packet (the C's 0); the table is a `static` array of [`PmsProtocol`],
//!   whose `enable`, `ioctl` and `disable` are `Option<fn>` (NULL in some entries; the C
//!   calls `sync` and `proc` without checking, so they are plain `fn`s). The ioctls return
//!   `Ok(true)`/`Ok(false)`/`Err` (`wsmousevar.rs`); [`pms_change_state`] returns
//!   `Result<(), Errno>` (its only errno is `EBUSY`), which `pms_enable` turns back into the
//!   C's `int` for `wsmouse`.
//! - The softc's fields are `Cell`s, touched at `spltty()` or under `sc_state_lock` and the
//!   kernel lock, as in the C; the packet is a `Cell<[u8; 8]>` and the protocol procs decode
//!   a copy of it. The sub-softcs (`SynapticsSoftc`, `AlpsSoftc`, `ElantechSoftc`) are
//!   `malloc`ed (`M_WAITOK | M_ZERO`), written with their `new()` value, and freed with
//!   `free` where the C frees them; the softc holds them as `Option<NonNull<_>>`.
//! - The C dereferences `sc_wsmousedev`, `sc_sec_wsmousedev`, `sc->protocol` and the
//!   sub-softcs without checking (its attach comment explains why they are set when used);
//!   the port checks the `Option`s: a missing `wsmouse` skips the report (or fails the
//!   hardware query that needs it), a missing sub-softc makes `sync` refuse the byte, and
//!   a missing protocol reads as the standard one. None of these arms runs once attached.
//! - `wsmouse_get_hw` is a guard over the mouse's input (`wsmouse.rs`); the port takes it
//!   only to write or copy the hardware description, never across a command, at the same
//!   points the C writes it.
//! - The Synaptics, ALPS and Elantech hardware queries keep the C's order of commands and
//!   writes; the read-back loops of `elantech_set_absolute_mode_v1..v3` keep the C's exit on
//!   a failed command, as if the register had been read (a slip of the C,
//!   docs/EXTERNAL_BUGS.md), with the answer buffer zeroed where the C's is uninitialised.
//! - The parity table of `pms_enable_elantech_v1` is filled by [`elantech_fill_parity`],
//!   the C's loop, kept as it is: it starts at `parity[0] ^= 1`, so every refill (each
//!   enable after the first) inverts the table (docs/EXTERNAL_BUGS.md).
//! - `mouse_has_softbtn` is `acpi(4)`'s flag on a PC (`MACHINE_PC`, where the C's
//!   `NACPI > 0`), and the driver's own never-set flag elsewhere (arm64, where the C does
//!   not include `acpi.h`). `SMALL_KERNEL` is not defined.
//! - The `#ifdef DIAGNOSTIC` messages (`pms_print_packet`, the out-of-sync and the device
//!   reset messages) follow the `diagnostic` cargo feature; the `DEBUG` ones (`DPRINTF`,
//!   the reset errors of `pms_reset` and `pmsprobe`) are not compiled, as in GENERIC.
//! - The decoding parts of the PS/2 packets ([`pms_ps2_motion`], shared by
//!   `pms_proc_mouse`, `synaptics_sec_proc` and `alps_sec_proc`, and [`pms_mouse_decode`])
//!   are functions of their own so the host tests can check them; the procs call them where
//!   the C computes inline. The `malloc`, `free` and `wsmouse_configure` the four
//!   `pms_enable_elantech_v*` repeat are three private helpers (`elantech_alloc`,
//!   `elantech_free`, `elantech_configure`), the parity loop is `elantech_fill_parity`, and
//!   the C's `goto err` is a labelled block.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI32, Ordering};

use crate::dev::ic::pckbc::{
    pckbc_enqueue_cmd, pckbc_flush, pckbc_poll_cmd, pckbc_set_inputhandler, pckbc_slot_enable,
};
use crate::dev::ic::pckbcvar::{PCKBC_AUX_SLOT, PckbcAttachArgs, PckbcTag};
use crate::dev::pckbc::pmsreg::*;
use crate::dev::wscons::wsconsio::{
    WSMOUSE_COMPAT, WSMOUSE_NATIVE, WSMOUSE_TYPE_ALPS, WSMOUSE_TYPE_ELANTECH, WSMOUSE_TYPE_PS2,
    WSMOUSE_TYPE_SYNAP_SBTN, WSMOUSE_TYPE_SYNAPTICS, WSMOUSECFG_PRESSURE_HI,
    WSMOUSECFG_PRESSURE_LO, WSMOUSECFG_SMOOTHING, WSMOUSEIO_GCALIBCOORDS, WSMOUSEIO_GTYPE,
    WSMOUSEIO_SETMODE, WSMOUSEIO_SRES, WsmouseCalibcoords, WsmouseParam,
};
use crate::dev::wscons::wsmouse::{
    wsmouse_buttons, wsmouse_configure, wsmouse_get_hw, wsmouse_input_sync, wsmouse_mtstate,
    wsmouse_set, wsmouse_set_mode, wsmousedevprint,
};
use crate::dev::wscons::wsmousevar::{
    WSMOUSE_DEFAULT_PRESSURE, WSMOUSE_MT_PRESSURE, WSMOUSE_MT_REL_X, WSMOUSE_MT_REL_Y,
    WSMOUSE_TOUCH_WIDTH, WSMOUSEHW_CLICKPAD, WSMOUSEHW_TOUCHPAD, WsmouseAccessops,
    WsmousedevAttachArgs,
};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_rwlock::{rw_enter_write, rw_exit_write, rw_init};
use crate::kern::kern_task::{SYSTQ, task_add, task_set};
use crate::kern::kern_timeout::{timeout_add_msec, timeout_set};
use crate::kern::subr_autoconf::{config_activate_children, config_found};
use crate::kern::subr_prf::printf;
use crate::machine::cpu::{MACHINE_PC, delay};
use crate::machine::intr::{spltty, splx};
use crate::sys::device::{
    CfMatch, Cfattach, Cfdriver, DV_DULL, DVACT_QUIESCE, DVACT_WAKEUP, Device, Softc,
};
use crate::sys::errno::Errno;
use crate::sys::ioctl::{ioctl_arg, ioctl_ret};
use crate::sys::malloc::{M_DEVBUF, M_WAITOK, M_ZERO};
use crate::sys::proc::Proc;
use crate::sys::rwlock::Rwlock;
use crate::sys::task::Task;
use crate::sys::timeout::Timeout;

/// `WSMOUSE_BUTTON(x)`: the `wsmouse` bit of button `x` (1-based).
pub const fn wsmouse_button(x: u32) -> u32 {
    1 << (x - 1)
}

/// `PMS_STANDARD`: the generic PS/2 mouse (`pms_protocol.type`).
pub const PMS_STANDARD: i32 = 0;
/// `PMS_INTELLI`: Microsoft IntelliMouse.
pub const PMS_INTELLI: i32 = 1;
/// `PMS_SYNAPTICS`.
pub const PMS_SYNAPTICS: i32 = 2;
/// `PMS_ALPS`.
pub const PMS_ALPS: i32 = 3;
/// `PMS_ELANTECH_V1`.
pub const PMS_ELANTECH_V1: i32 = 4;
/// `PMS_ELANTECH_V2`.
pub const PMS_ELANTECH_V2: i32 = 5;
/// `PMS_ELANTECH_V3`.
pub const PMS_ELANTECH_V3: i32 = 6;
/// `PMS_ELANTECH_V4`.
pub const PMS_ELANTECH_V4: i32 = 7;

/// `SYNAPTICS_MASK_NEWABS_STRICT` (`synaptics_softc.mask`).
pub const SYNAPTICS_MASK_NEWABS_STRICT: i32 = 0xc8;
/// `SYNAPTICS_MASK_NEWABS_RELAXED`.
pub const SYNAPTICS_MASK_NEWABS_RELAXED: i32 = 0xc0;
/// `SYNAPTICS_VALID_NEWABS_FIRST`.
pub const SYNAPTICS_VALID_NEWABS_FIRST: i32 = 0x80;
/// `SYNAPTICS_VALID_NEWABS_NEXT`.
pub const SYNAPTICS_VALID_NEWABS_NEXT: i32 = 0xc0;

/// `SYNAPTICS_PRESSURE_HI`.
pub const SYNAPTICS_PRESSURE_HI: i32 = 30;
/// `SYNAPTICS_PRESSURE_LO`.
pub const SYNAPTICS_PRESSURE_LO: i32 = 25;
/// `SYNAPTICS_PRESSURE`.
pub const SYNAPTICS_PRESSURE: i32 = SYNAPTICS_PRESSURE_HI;
/// `SYNAPTICS_SCALE`.
pub const SYNAPTICS_SCALE: i32 = 4;
/// `SYNAPTICS_MAX_FINGERS`.
pub const SYNAPTICS_MAX_FINGERS: i32 = 3;

/// `ALPS_GLIDEPOINT` (`alps_softc.model`).
pub const ALPS_GLIDEPOINT: i32 = 1 << 1;
/// `ALPS_DUALPOINT`.
pub const ALPS_DUALPOINT: i32 = 1 << 2;
/// `ALPS_PASSTHROUGH`.
pub const ALPS_PASSTHROUGH: i32 = 1 << 3;
/// `ALPS_INTERLEAVED`.
pub const ALPS_INTERLEAVED: i32 = 1 << 4;

/// `ALPS_PRESSURE`.
pub const ALPS_PRESSURE: i32 = 40;

/// `ELANTECH_F_REPORTS_PRESSURE` (`elantech_softc.flags`).
pub const ELANTECH_F_REPORTS_PRESSURE: i32 = 0x01;
/// `ELANTECH_F_HAS_ROCKER`.
pub const ELANTECH_F_HAS_ROCKER: i32 = 0x02;
/// `ELANTECH_F_2FINGER_PACKET`.
pub const ELANTECH_F_2FINGER_PACKET: i32 = 0x04;
/// `ELANTECH_F_HW_V1_OLD`.
pub const ELANTECH_F_HW_V1_OLD: i32 = 0x08;
/// `ELANTECH_F_CRC_ENABLED`.
pub const ELANTECH_F_CRC_ENABLED: i32 = 0x10;
/// `ELANTECH_F_TRACKPOINT`.
pub const ELANTECH_F_TRACKPOINT: i32 = 0x20;

/// `PMS_STATE_DISABLED` (`pms_softc.sc_state`).
pub const PMS_STATE_DISABLED: i32 = 0;
/// `PMS_STATE_ENABLED`.
pub const PMS_STATE_ENABLED: i32 = 1;
/// `PMS_STATE_SUSPENDED`.
pub const PMS_STATE_SUSPENDED: i32 = 2;

/// `PMS_DEV_IGNORE` (`pms_softc.sc_dev_enable`).
pub const PMS_DEV_IGNORE: i32 = 0x00;
/// `PMS_DEV_PRIMARY`.
pub const PMS_DEV_PRIMARY: i32 = 0x01;
/// `PMS_DEV_SECONDARY`.
pub const PMS_DEV_SECONDARY: i32 = 0x02;

/// `PMS_RST_COMMENCE` (`pms_softc.sc_rststate`).
pub const PMS_RST_COMMENCE: i32 = 0x01;
/// `PMS_RST_ANNOUNCED`.
pub const PMS_RST_ANNOUNCED: i32 = 0x02;

/// The type of a protocol's `ioctl`, as `wsmouse`'s ([`crate::dev::wscons::wsmousevar`]).
pub type PmsIoctlFn = fn(
    sc: &PmsSoftc,
    cmd: u64,
    data: &mut [u8],
    flag: i32,
    p: Option<&Proc>,
) -> Result<bool, Errno>;

/// `struct pms_protocol`: a protocol of the table [`PMS_PROTOCOLS`].
pub struct PmsProtocol {
    /// `type`: `PMS_STANDARD`, `PMS_INTELLI`, ...
    pub type_: i32,
    /// `packetsize`: the bytes of a packet.
    pub packetsize: usize,
    /// `enable`: the knock and the setup; `true` when the device speaks the protocol.
    pub enable: Option<fn(&PmsSoftc) -> bool>,
    /// `ioctl`.
    pub ioctl: Option<PmsIoctlFn>,
    /// `sync`: `true` when the byte fits the packet at `inputstate`.
    pub sync: fn(&PmsSoftc, i32) -> bool,
    /// `proc`: decode a complete packet and report it.
    pub proc_: fn(&PmsSoftc),
    /// `disable`.
    pub disable: Option<fn(&PmsSoftc)>,
}

/// `struct synaptics_softc`.
pub struct SynapticsSoftc {
    /// `identify`: the answer to `SYNAPTICS_QUE_IDENTIFY`.
    pub identify: Cell<i32>,
    /// `capabilities`.
    pub capabilities: Cell<i32>,
    /// `ext_capabilities`.
    pub ext_capabilities: Cell<i32>,
    /// `ext2_capabilities`.
    pub ext2_capabilities: Cell<i32>,
    /// `model`.
    pub model: Cell<i32>,
    /// `ext_model`.
    pub ext_model: Cell<i32>,
    /// `modes`.
    pub modes: Cell<i32>,
    /// `mode`: the mode last set.
    pub mode: Cell<i32>,
    /// `mask`: `SYNAPTICS_MASK_NEWABS_STRICT` or `_RELAXED`.
    pub mask: Cell<i32>,
    /// `sec_buttons`: the trackstick's buttons wired to the touchpad.
    pub sec_buttons: Cell<u32>,
}

impl SynapticsSoftc {
    /// The `M_ZERO` state.
    pub const fn new() -> Self {
        Self {
            identify: Cell::new(0),
            capabilities: Cell::new(0),
            ext_capabilities: Cell::new(0),
            ext2_capabilities: Cell::new(0),
            model: Cell::new(0),
            ext_model: Cell::new(0),
            modes: Cell::new(0),
            mode: Cell::new(0),
            mask: Cell::new(0),
            sec_buttons: Cell::new(0),
        }
    }
}

impl Default for SynapticsSoftc {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct alps_softc`.
pub struct AlpsSoftc {
    /// `model`: `ALPS_GLIDEPOINT`, `ALPS_DUALPOINT`, ...
    pub model: Cell<i32>,
    /// `mask`: the sync mask of a packet's first byte.
    pub mask: Cell<i32>,
    /// `version`.
    pub version: Cell<i32>,
    /// `gesture`.
    pub gesture: Cell<u32>,
    /// `sec_buttons`: trackpoint.
    pub sec_buttons: Cell<u32>,
    /// `old_x`.
    pub old_x: Cell<i32>,
    /// `old_y`.
    pub old_y: Cell<i32>,
}

impl AlpsSoftc {
    /// The `M_ZERO` state.
    pub const fn new() -> Self {
        Self {
            model: Cell::new(0),
            mask: Cell::new(0),
            version: Cell::new(0),
            gesture: Cell::new(0),
            sec_buttons: Cell::new(0),
            old_x: Cell::new(0),
            old_y: Cell::new(0),
        }
    }
}

impl Default for AlpsSoftc {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct elantech_softc`.
pub struct ElantechSoftc {
    /// `flags`: `ELANTECH_F_*`.
    pub flags: Cell<i32>,
    /// `fw_version`.
    pub fw_version: Cell<i32>,
    /// `mt_slots`: the v4 contacts down, as a mask.
    pub mt_slots: Cell<u32>,
    /// `width`.
    pub width: Cell<i32>,
    /// `parity`: the v1 parity bit of each byte.
    pub parity: [Cell<u8>; 256],
    /// `p1`.
    pub p1: Cell<u8>,
    /// `p2`.
    pub p2: Cell<u8>,
    /// `p3`.
    pub p3: Cell<u8>,
    /// `max_x`.
    pub max_x: Cell<i32>,
    /// `max_y`.
    pub max_y: Cell<i32>,
    /// `old_x`.
    pub old_x: Cell<i32>,
    /// `old_y`.
    pub old_y: Cell<i32>,
    /// `initial_pkt`.
    pub initial_pkt: Cell<i32>,
}

impl ElantechSoftc {
    /// The `M_ZERO` state.
    pub const fn new() -> Self {
        Self {
            flags: Cell::new(0),
            fw_version: Cell::new(0),
            mt_slots: Cell::new(0),
            width: Cell::new(0),
            parity: [const { Cell::new(0) }; 256],
            p1: Cell::new(0),
            p2: Cell::new(0),
            p3: Cell::new(0),
            max_x: Cell::new(0),
            max_y: Cell::new(0),
            old_x: Cell::new(0),
            old_y: Cell::new(0),
            initial_pkt: Cell::new(0),
        }
    }
}

impl Default for ElantechSoftc {
    fn default() -> Self {
        Self::new()
    }
}

/// `ELANTECH_IS_CLICKPAD(sc)`.
pub fn elantech_is_clickpad(elantech: &ElantechSoftc) -> bool {
    elantech.fw_version.get() & 0x1000 != 0
}

/// `struct pms_softc`: driver status information.
#[repr(C)]
pub struct PmsSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `sc_kbctag`.
    pub sc_kbctag: Cell<Option<PckbcTag>>,
    /// `sc_state`: `PMS_STATE_*`.
    pub sc_state: Cell<i32>,
    /// `sc_state_lock`: serialises the state changes ([`pms_change_state`]).
    pub sc_state_lock: Rwlock,
    /// `sc_dev_enable`: `PMS_DEV_PRIMARY` and `PMS_DEV_SECONDARY`, the `wsmouse`s open.
    pub sc_dev_enable: Cell<i32>,
    /// `sc_rsttask`: the reset task.
    pub sc_rsttask: Task,
    /// `sc_rsttimo`: the reset timeout.
    pub sc_rsttimo: Timeout,
    /// `sc_rststate`: `PMS_RST_*`.
    pub sc_rststate: Cell<i32>,
    /// `poll`: commands are polled (autoconfiguration, suspend).
    pub poll: Cell<i32>,
    /// `inputstate`: the index of the next byte of the packet.
    pub inputstate: Cell<usize>,
    /// `protocol`.
    pub protocol: Cell<Option<&'static PmsProtocol>>,
    /// `synaptics`.
    pub synaptics: Cell<Option<NonNull<SynapticsSoftc>>>,
    /// `alps`.
    pub alps: Cell<Option<NonNull<AlpsSoftc>>>,
    /// `elantech`.
    pub elantech: Cell<Option<NonNull<ElantechSoftc>>>,
    /// `packet`.
    pub packet: Cell<[u8; 8]>,
    /// `sc_wsmousedev`.
    pub sc_wsmousedev: Cell<Option<NonNull<Device>>>,
    /// `sc_sec_wsmousedev`: the trackpoint's `wsmouse`.
    pub sc_sec_wsmousedev: Cell<Option<NonNull<Device>>>,
}

impl PmsSoftc {
    /// `DEVNAME(sc)`.
    pub fn devname(&self) -> &str {
        self.sc_dev.xname()
    }

    /// `sc->protocol`, the standard one before the lookup sets it.
    pub fn protocol(&self) -> &'static PmsProtocol {
        self.protocol.get().unwrap_or(&PMS_PROTOCOLS[0])
    }

    /// `sc->synaptics`.
    pub fn synaptics(&self) -> Option<&SynapticsSoftc> {
        // SAFETY: the block is `malloc`ed and written by `pms_enable_synaptics`, and freed
        // only there, after the pointer is taken back; no reference from here is held
        // across that `free`.
        self.synaptics.get().map(|p| unsafe { p.as_ref() })
    }

    /// `sc->alps`.
    pub fn alps(&self) -> Option<&AlpsSoftc> {
        // SAFETY: as in `synaptics`, with `pms_enable_alps`.
        self.alps.get().map(|p| unsafe { p.as_ref() })
    }

    /// `sc->elantech`.
    pub fn elantech(&self) -> Option<&ElantechSoftc> {
        // SAFETY: as in `synaptics`, with the `pms_enable_elantech_v*` functions.
        self.elantech.get().map(|p| unsafe { p.as_ref() })
    }

    /// `sc->sc_wsmousedev`.
    pub fn wsmousedev(&self) -> Option<&Device> {
        // SAFETY: what `config_found` returned in `pmsattach`; never detached.
        self.sc_wsmousedev.get().map(|d| unsafe { d.as_ref() })
    }

    /// `sc->sc_sec_wsmousedev`.
    pub fn sec_wsmousedev(&self) -> Option<&Device> {
        // SAFETY: what `config_found` returned for the trackpoint; never detached.
        self.sc_sec_wsmousedev.get().map(|d| unsafe { d.as_ref() })
    }

    /// `sc` as the cookie of `wsmouse`, `pckbc`, the task and the timeout.
    fn cookie(&self) -> *mut c_void {
        ptr::from_ref(self).cast_mut().cast()
    }
}

// SAFETY: `#[repr(C)]` with the device first; the lock, the task and the timeout are valid
// all-zero (`sys/rwlock.rs`, `sys/task.rs`, `sys/timeout.rs`), and every other member is a
// `Cell` of an integer, a byte array or `None`.
unsafe impl Softc for PmsSoftc {}

/// `struct alps_model`.
pub struct AlpsModel {
    /// `version`.
    pub version: i32,
    /// `mask`.
    pub mask: i32,
    /// `model`.
    pub model: i32,
}

/// `mouse_has_softbtn` where the C defines it itself (no `acpi(4)`: not a PC): never set.
static MOUSE_HAS_SOFTBTN: AtomicI32 = AtomicI32::new(0);

/// `butmap`: the `wsmouse` buttons of the three PS/2 button bits.
pub static BUTMAP: [u32; 8] = [
    0,
    wsmouse_button(1),
    wsmouse_button(3),
    wsmouse_button(1) | wsmouse_button(3),
    wsmouse_button(2),
    wsmouse_button(1) | wsmouse_button(2),
    wsmouse_button(2) | wsmouse_button(3),
    wsmouse_button(1) | wsmouse_button(2) | wsmouse_button(3),
];

/// `alps_models`: the ALPS touchpads the driver knows, by version. The C's `#if 0` entries
/// (0x633b, a clitpad not compatible enough; 0x7326, an unknown v3 protocol; 0x7331, not
/// supported) are left out as there.
pub static ALPS_MODELS: [AlpsModel; 19] = [
    AlpsModel {
        version: 0x2021,
        mask: 0xf8,
        model: ALPS_DUALPOINT | ALPS_PASSTHROUGH,
    },
    AlpsModel {
        version: 0x2221,
        mask: 0xf8,
        model: ALPS_DUALPOINT | ALPS_PASSTHROUGH,
    },
    AlpsModel {
        version: 0x2222,
        mask: 0xff,
        model: ALPS_DUALPOINT | ALPS_PASSTHROUGH,
    },
    AlpsModel {
        version: 0x3222,
        mask: 0xf8,
        model: ALPS_DUALPOINT | ALPS_PASSTHROUGH,
    },
    AlpsModel {
        version: 0x5212,
        mask: 0xff,
        model: ALPS_DUALPOINT | ALPS_PASSTHROUGH | ALPS_INTERLEAVED,
    },
    AlpsModel {
        version: 0x5321,
        mask: 0xf8,
        model: ALPS_GLIDEPOINT,
    },
    AlpsModel {
        version: 0x5322,
        mask: 0xf8,
        model: ALPS_GLIDEPOINT,
    },
    AlpsModel {
        version: 0x603b,
        mask: 0xf8,
        model: ALPS_GLIDEPOINT,
    },
    AlpsModel {
        version: 0x6222,
        mask: 0xcf,
        model: ALPS_DUALPOINT | ALPS_PASSTHROUGH | ALPS_INTERLEAVED,
    },
    AlpsModel {
        version: 0x6321,
        mask: 0xf8,
        model: ALPS_GLIDEPOINT,
    },
    AlpsModel {
        version: 0x6322,
        mask: 0xf8,
        model: ALPS_GLIDEPOINT,
    },
    AlpsModel {
        version: 0x6323,
        mask: 0xf8,
        model: ALPS_GLIDEPOINT,
    },
    AlpsModel {
        version: 0x6324,
        mask: 0x8f,
        model: ALPS_GLIDEPOINT,
    },
    AlpsModel {
        version: 0x6325,
        mask: 0xef,
        model: ALPS_GLIDEPOINT,
    },
    AlpsModel {
        version: 0x6326,
        mask: 0xf8,
        model: ALPS_GLIDEPOINT,
    },
    AlpsModel {
        version: 0x7301,
        mask: 0xf8,
        model: ALPS_DUALPOINT,
    },
    AlpsModel {
        version: 0x7321,
        mask: 0xf8,
        model: ALPS_GLIDEPOINT,
    },
    AlpsModel {
        version: 0x7322,
        mask: 0xf8,
        model: ALPS_GLIDEPOINT,
    },
    AlpsModel {
        version: 0x7325,
        mask: 0xcf,
        model: ALPS_GLIDEPOINT,
    },
];

/// `synaptics_params`: the `wsmouse` configuration of a Synaptics touchpad.
static SYNAPTICS_PARAMS: [WsmouseParam; 2] = [
    WsmouseParam {
        key: WSMOUSECFG_PRESSURE_LO,
        value: SYNAPTICS_PRESSURE_LO,
    },
    WsmouseParam {
        key: WSMOUSECFG_PRESSURE_HI,
        value: SYNAPTICS_PRESSURE_HI,
    },
];

/// `alps_params`.
static ALPS_PARAMS: [WsmouseParam; 1] = [WsmouseParam {
    key: WSMOUSECFG_SMOOTHING,
    value: 3,
}];

/// `pms_ca`.
pub static PMS_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<PmsSoftc>(),
    ca_match: Some(pmsprobe),
    ca_attach: pmsattach,
    ca_detach: None,
    ca_activate: Some(pmsactivate),
};

/// `pms_cd`.
pub static PMS_CD: Cfdriver = Cfdriver::new(b"pms", DV_DULL, 0);

/// `pms_accessops`: the touchpad's or mouse's `wsmouse`.
pub static PMS_ACCESSOPS: WsmouseAccessops = WsmouseAccessops {
    enable: pms_enable,
    ioctl: pms_ioctl,
    disable: pms_disable,
};

/// `pms_sec_accessops`: the trackpoint's `wsmouse`.
pub static PMS_SEC_ACCESSOPS: WsmouseAccessops = WsmouseAccessops {
    enable: pms_sec_enable,
    ioctl: pms_sec_ioctl,
    disable: pms_sec_disable,
};

/// `pms_protocols`: the protocols, the generic PS/2 mouse first (the fallback), the
/// IntelliMouse last (its knock is the least specific).
pub static PMS_PROTOCOLS: [PmsProtocol; 8] = [
    // Generic PS/2 mouse
    PmsProtocol {
        type_: PMS_STANDARD,
        packetsize: 3,
        enable: None,
        ioctl: Some(pms_ioctl_mouse),
        sync: pms_sync_mouse,
        proc_: pms_proc_mouse,
        disable: None,
    },
    // Synaptics touchpad
    PmsProtocol {
        type_: PMS_SYNAPTICS,
        packetsize: 6,
        enable: Some(pms_enable_synaptics),
        ioctl: Some(pms_ioctl_synaptics),
        sync: pms_sync_synaptics,
        proc_: pms_proc_synaptics,
        disable: Some(pms_disable_synaptics),
    },
    // ALPS touchpad
    PmsProtocol {
        type_: PMS_ALPS,
        packetsize: 6,
        enable: Some(pms_enable_alps),
        ioctl: Some(pms_ioctl_alps),
        sync: pms_sync_alps,
        proc_: pms_proc_alps,
        disable: None,
    },
    // Elantech touchpad (hardware version 1)
    PmsProtocol {
        type_: PMS_ELANTECH_V1,
        packetsize: 4,
        enable: Some(pms_enable_elantech_v1),
        ioctl: Some(pms_ioctl_elantech),
        sync: pms_sync_elantech_v1,
        proc_: pms_proc_elantech_v1,
        disable: None,
    },
    // Elantech touchpad (hardware version 2)
    PmsProtocol {
        type_: PMS_ELANTECH_V2,
        packetsize: 6,
        enable: Some(pms_enable_elantech_v2),
        ioctl: Some(pms_ioctl_elantech),
        sync: pms_sync_elantech_v2,
        proc_: pms_proc_elantech_v2,
        disable: None,
    },
    // Elantech touchpad (hardware version 3)
    PmsProtocol {
        type_: PMS_ELANTECH_V3,
        packetsize: 6,
        enable: Some(pms_enable_elantech_v3),
        ioctl: Some(pms_ioctl_elantech),
        sync: pms_sync_elantech_v3,
        proc_: pms_proc_elantech_v3,
        disable: None,
    },
    // Elantech touchpad (hardware version 4)
    PmsProtocol {
        type_: PMS_ELANTECH_V4,
        packetsize: 6,
        enable: Some(pms_enable_elantech_v4),
        ioctl: Some(pms_ioctl_elantech),
        sync: pms_sync_elantech_v4,
        proc_: pms_proc_elantech_v4,
        disable: None,
    },
    // Microsoft IntelliMouse
    PmsProtocol {
        type_: PMS_INTELLI,
        packetsize: 4,
        enable: Some(pms_enable_intelli),
        ioctl: Some(pms_ioctl_mouse),
        sync: pms_sync_mouse,
        proc_: pms_proc_mouse,
        disable: None,
    },
];

/// `mouse_has_softbtn`: `acpi(4)`'s flag on a PC, the driver's own elsewhere.
fn mouse_has_softbtn() -> i32 {
    if MACHINE_PC {
        crate::dev::acpi::acpi::MOUSE_HAS_SOFTBTN.load(Ordering::Relaxed)
    } else {
        MOUSE_HAS_SOFTBTN.load(Ordering::Relaxed)
    }
}

/// The softc behind a cookie (the access operations', the input handler's, the task's and
/// the timeout's).
fn pms_cookie(v: *mut c_void) -> &'static PmsSoftc {
    // SAFETY: every cookie is the softc `pmsattach` handed out; it is never detached.
    unsafe { &*v.cast::<PmsSoftc>() }
}

/// `pms_cmd`: send `cmd` to the device and read `resp.len()` bytes of answer: polled while
/// `poll` is set, queued and waited for otherwise.
pub fn pms_cmd(sc: &PmsSoftc, cmd: &[u8], resp: &mut [u8]) -> Result<(), Errno> {
    let Some(tag) = sc.sc_kbctag.get() else {
        return Err(Errno::ENXIO);
    };
    let resplen = resp.len();
    let resp = if resplen == 0 { None } else { Some(resp) };

    if sc.poll.get() != 0 {
        pckbc_poll_cmd(tag, PCKBC_AUX_SLOT, cmd, resplen, resp, true)
    } else {
        pckbc_enqueue_cmd(tag, PCKBC_AUX_SLOT, cmd, resplen, true, resp)
    }
}

/// `pms_spec_cmd`: the Synaptics "special command" sequence that encodes `cmd` in four
/// resolution commands, two bits each, after a scaling command.
pub fn pms_spec_cmd(sc: &PmsSoftc, cmd: i32) -> Result<(), Errno> {
    pms_set_scaling(sc, 1)?;
    pms_set_resolution(sc, (cmd >> 6) & 0x03)?;
    pms_set_resolution(sc, (cmd >> 4) & 0x03)?;
    pms_set_resolution(sc, (cmd >> 2) & 0x03)?;
    pms_set_resolution(sc, cmd & 0x03)?;
    Ok(())
}

/// `pms_get_devid`: the device type byte.
pub fn pms_get_devid(sc: &PmsSoftc) -> Result<u8, Errno> {
    let mut resp = [0u8; 1];
    pms_cmd(sc, &[PMS_SEND_DEV_ID], &mut resp)?;
    Ok(resp[0])
}

/// `pms_get_status`: the three status bytes, into `resp` (left as it was on an error).
pub fn pms_get_status(sc: &PmsSoftc, resp: &mut [u8; 3]) -> Result<(), Errno> {
    pms_cmd(sc, &[PMS_SEND_DEV_STATUS], resp)
}

/// `pms_set_rate`: set the sampling rate.
pub fn pms_set_rate(sc: &PmsSoftc, value: u8) -> Result<(), Errno> {
    pms_cmd(sc, &[PMS_SET_SAMPLE, value], &mut [])
}

/// `pms_set_resolution`: set the resolution (0..3; the byte of `value`, as the C's
/// `u_char`).
pub fn pms_set_resolution(sc: &PmsSoftc, value: i32) -> Result<(), Errno> {
    pms_cmd(sc, &[PMS_SET_RES, value as u8], &mut [])
}

/// `pms_set_scaling`: scaling 2:1 for `scale == 2`, 1:1 otherwise.
pub fn pms_set_scaling(sc: &PmsSoftc, scale: i32) -> Result<(), Errno> {
    let cmd = match scale {
        2 => PMS_SET_SCALE21,
        _ => PMS_SET_SCALE11,
    };
    pms_cmd(sc, &[cmd], &mut [])
}

/// `pms_reset`: reset the device (the `DEBUG` message about a bad answer is not compiled).
pub fn pms_reset(sc: &PmsSoftc) -> Result<(), Errno> {
    let mut resp = [0u8; 2];
    pms_cmd(sc, &[PMS_RESET], &mut resp)
}

/// `pms_dev_enable`: turn the device's reports on.
pub fn pms_dev_enable(sc: &PmsSoftc) -> Result<(), Errno> {
    let res = pms_cmd(sc, &[PMS_DEV_ENABLE], &mut []);
    if res.is_err() {
        printf(format_args!("{}: enable error\n", sc.devname()));
    }
    res
}

/// `pms_dev_disable`: turn the device's reports off.
pub fn pms_dev_disable(sc: &PmsSoftc) -> Result<(), Errno> {
    let res = pms_cmd(sc, &[PMS_DEV_DISABLE], &mut []);
    if res.is_err() {
        printf(format_args!("{}: disable error\n", sc.devname()));
    }
    res
}

/// `pms_protocol_lookup`: the first protocol after the standard one whose `enable`
/// succeeds (after a reset each), the standard one if none does.
pub fn pms_protocol_lookup(sc: &PmsSoftc) {
    sc.protocol.set(Some(&PMS_PROTOCOLS[0]));
    for p in &PMS_PROTOCOLS[1..] {
        let _ = pms_reset(sc);
        if p.enable.is_some_and(|enable| enable(sc)) {
            sc.protocol.set(Some(p));
            break;
        }
    }

    // DPRINTF("%s: protocol type %d\n", ...): DEBUG, not compiled.
}

/// `pms_reset_detect`: detect the reset announcement (`0xaa, 0x00`). The sequence will be
/// sent as input on rare occasions when the touchpad was reset due to a power failure.
pub fn pms_reset_detect(sc: &PmsSoftc, data: i32) {
    match sc.sc_rststate.get() {
        PMS_RST_COMMENCE => {
            if data == 0x0 {
                sc.sc_rststate.set(PMS_RST_ANNOUNCED);
                timeout_add_msec(&sc.sc_rsttimo, 100);
            } else if data != i32::from(PMS_RSTDONE) {
                sc.sc_rststate.set(0);
            }
        }
        _ => {
            if data == i32::from(PMS_RSTDONE) {
                sc.sc_rststate.set(PMS_RST_COMMENCE);
            } else {
                sc.sc_rststate.set(0);
            }
        }
    }
}

/// `pms_reset_timo`: 100 ms after an announcement, schedule the reset task, unless the
/// reset was a false positive or the device already is disabled.
pub fn pms_reset_timo(v: *mut c_void) {
    let sc = pms_cookie(v);
    let s = spltty();

    // Do nothing if the reset was a false positive or if the device already is disabled.
    if sc.sc_rststate.get() == PMS_RST_ANNOUNCED && sc.sc_state.get() != PMS_STATE_DISABLED {
        task_add(SYSTQ, &sc.sc_rsttask);
    }

    splx(s);
}

/// `pms_reset_task`: disable and enable again the devices that are open.
pub fn pms_reset_task(v: *mut c_void) {
    let sc = pms_cookie(v);
    let s = spltty();

    if cfg!(feature = "diagnostic") {
        printf(format_args!(
            "{}: device reset (state = {})\n",
            sc.devname(),
            sc.sc_rststate.get()
        ));
    }

    rw_enter_write(&sc.sc_state_lock);

    if sc.sc_sec_wsmousedev.get().is_some() {
        let _ = pms_change_state(sc, PMS_STATE_DISABLED, PMS_DEV_SECONDARY);
    }
    let _ = pms_change_state(sc, PMS_STATE_DISABLED, PMS_DEV_PRIMARY);

    let _ = pms_change_state(sc, PMS_STATE_ENABLED, PMS_DEV_PRIMARY);
    if sc.sc_sec_wsmousedev.get().is_some() {
        let _ = pms_change_state(sc, PMS_STATE_ENABLED, PMS_DEV_SECONDARY);
    }

    rw_exit_write(&sc.sc_state_lock);
    splx(s);
}

/// `pms_enable_intelli`: the special sequence to enable the third button and the roller;
/// an IntelliMouse then reports id 3.
pub fn pms_enable_intelli(sc: &PmsSoftc) -> bool {
    pms_set_rate(sc, PMS_INTELLI_MAGIC1).is_ok()
        && pms_set_rate(sc, PMS_INTELLI_MAGIC2).is_ok()
        && pms_set_rate(sc, PMS_INTELLI_MAGIC3).is_ok()
        && pms_get_devid(sc) == Ok(PMS_INTELLI_ID)
}

/// `pms_ioctl_mouse`: the type and the resolution of a PS/2 mouse.
pub fn pms_ioctl_mouse(
    sc: &PmsSoftc,
    cmd: u64,
    data: &mut [u8],
    _flag: i32,
    _p: Option<&Proc>,
) -> Result<bool, Errno> {
    match cmd {
        WSMOUSEIO_GTYPE => ioctl_ret(data, &WSMOUSE_TYPE_PS2),
        WSMOUSEIO_SRES => {
            let i = (ioctl_arg::<u32>(data) as i32).wrapping_sub(12) / 25;
            // valid values are {0,1,2,3}
            let i = i.clamp(0, 3);

            if pms_set_resolution(sc, i).is_err() {
                printf(format_args!("{}: SET_RES command error\n", sc.devname()));
            }
        }
        _ => return Ok(false),
    }
    Ok(true)
}

/// `pms_sync_mouse`: the first byte of a standard packet has no overflow bits; the first
/// byte of an IntelliMouse packet has bit 3 set.
pub fn pms_sync_mouse(sc: &PmsSoftc, data: i32) -> bool {
    if sc.inputstate.get() != 0 {
        return true;
    }

    match sc.protocol().type_ {
        PMS_STANDARD => data & 0xc0 == 0,
        PMS_INTELLI => data & 0x08 == 0x08,
        _ => true,
    }
}

/// The buttons and the motion of a PS/2 packet's first three bytes: the status byte (its
/// buttons and the sign bits) and the X and Y bytes, as `pms_proc_mouse`,
/// `synaptics_sec_proc` and `alps_sec_proc` decode them.
pub fn pms_ps2_motion(status: u8, x: u8, y: u8) -> (u32, i32, i32) {
    let buttons = BUTMAP[usize::from(status & PMS_PS2_BUTTONSMASK)];
    let dx = if status & PMS_PS2_XNEG != 0 {
        i32::from(x) - 256
    } else {
        i32::from(x)
    };
    let dy = if status & PMS_PS2_YNEG != 0 {
        i32::from(y) - 256
    } else {
        i32::from(y)
    };
    (buttons, dx, dy)
}

/// The decoding of `pms_proc_mouse`: buttons, dx, dy and (IntelliMouse) the wheel.
pub fn pms_mouse_decode(sc: &PmsSoftc) -> (u32, i32, i32, i32) {
    let p = sc.packet.get();
    let (buttons, dx, dy) = pms_ps2_motion(p[0], p[1], p[2]);
    let dz = if sc.protocol().type_ == PMS_INTELLI {
        i32::from(p[3] as i8)
    } else {
        0
    };
    (buttons, dx, dy, dz)
}

/// `pms_proc_mouse`: report a mouse packet.
pub fn pms_proc_mouse(sc: &PmsSoftc) {
    let (buttons, dx, dy, dz) = pms_mouse_decode(sc);

    if let Some(dev) = sc.wsmousedev() {
        crate::WSMOUSE_INPUT!(dev, buttons, dx, dy, dz, 0);
    }
}

/// `pmsprobe`: a device on the aux slot that answers a reset with `PMS_RSTDONE, 0`.
pub fn pmsprobe(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: `pckbc_attach_slot` hands its children a `PckbcAttachArgs`.
    let pa = unsafe { &*aux.cast::<PckbcAttachArgs>() };
    let mut resp = [0u8; 2];

    if pa.pa_slot != PCKBC_AUX_SLOT {
        return 0;
    }

    // Flush any garbage.
    pckbc_flush(pa.pa_tag, pa.pa_slot);

    // reset the device
    let res = pckbc_poll_cmd(
        pa.pa_tag,
        pa.pa_slot,
        &[PMS_RESET],
        2,
        Some(&mut resp),
        true,
    );
    if res.is_err() || resp[0] != PMS_RSTDONE || resp[1] != 0 {
        // The DEBUG message ("pms: reset error ...") is not compiled.
        return 0;
    }

    1
}

/// `pmsattach`: hook the input handler up, attach the `wsmouse`, find the protocol and
/// leave the device disabled until the `wsmouse` opens.
pub fn pmsattach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `pms_ca` makes `PmsSoftc`s; `config_make_softc`'s allocation lives as long as
    // the device, which is never freed while attached.
    let sc: &'static PmsSoftc = unsafe { &*ptr::from_ref(self_.softc::<PmsSoftc>()) };
    // SAFETY: as in `pmsprobe`.
    let pa = unsafe { &*aux.cast::<PckbcAttachArgs>() };

    sc.sc_kbctag.set(Some(pa.pa_tag));

    // SAFETY: the softc is the device, alive while attached (see above).
    let name: &'static str = unsafe { &*ptr::from_ref(sc.devname()) };
    pckbc_set_inputhandler(
        pa.pa_tag,
        PCKBC_AUX_SLOT,
        Some(pmsinput),
        sc.cookie(),
        Some(name),
    );

    printf(format_args!("\n"));

    let mut a = WsmousedevAttachArgs {
        accessops: &PMS_ACCESSOPS,
        accesscookie: sc.cookie(),
    };

    rw_init(&sc.sc_state_lock, "pmsst");

    // Attach the wsmouse, saving a handle to it. Note that we don't need to check this
    // pointer against NULL here or in pmsintr, because if this fails pms_enable() will
    // never be called, so pmsinput() will never be called.
    sc.sc_wsmousedev.set(config_found(
        &sc.sc_dev,
        ptr::from_mut(&mut a).cast(),
        Some(wsmousedevprint),
    ));

    task_set(&sc.sc_rsttask, pms_reset_task, sc.cookie());
    timeout_set(&sc.sc_rsttimo, pms_reset_timo, sc.cookie());

    sc.poll.set(1);
    sc.sc_dev_enable.set(0);

    // See if the device understands an extended (touchpad) protocol.
    pms_protocol_lookup(sc);

    // no interrupts until enabled
    let _ = pms_change_state(sc, PMS_STATE_DISABLED, PMS_DEV_IGNORE);
}

/// `pmsactivate`: suspend an enabled device after its children quiesce, enable it again
/// before they wake up.
pub fn pmsactivate(self_: &Device, act: i32) -> Result<(), Errno> {
    // SAFETY: `pms_ca` makes `PmsSoftc`s.
    let sc = unsafe { self_.softc::<PmsSoftc>() };

    match act {
        DVACT_QUIESCE => {
            let rv = config_activate_children(self_, act);
            if sc.sc_state.get() == PMS_STATE_ENABLED {
                let _ = pms_change_state(sc, PMS_STATE_SUSPENDED, PMS_DEV_IGNORE);
            }
            rv
        }
        DVACT_WAKEUP => {
            if sc.sc_state.get() == PMS_STATE_SUSPENDED {
                let _ = pms_change_state(sc, PMS_STATE_ENABLED, PMS_DEV_IGNORE);
            }
            config_activate_children(self_, act)
        }
        _ => config_activate_children(self_, act),
    }
}

/// `pms_change_state`: move the device to `newstate` on behalf of `dev` (the primary or
/// the secondary `wsmouse`, or neither): it is enabled while either is open, and enabling
/// it re-runs the protocol's setup (or the lookup when that fails). `EBUSY` when `dev` is
/// enabled already.
pub fn pms_change_state(sc: &PmsSoftc, newstate: i32, dev: i32) -> Result<(), Errno> {
    if dev != PMS_DEV_IGNORE {
        match newstate {
            PMS_STATE_ENABLED => {
                if sc.sc_dev_enable.get() & dev != 0 {
                    return Err(Errno::EBUSY);
                }

                sc.sc_dev_enable.set(sc.sc_dev_enable.get() | dev);

                if sc.sc_state.get() == PMS_STATE_ENABLED {
                    return Ok(());
                }
            }
            PMS_STATE_DISABLED => {
                sc.sc_dev_enable.set(sc.sc_dev_enable.get() & !dev);

                if sc.sc_dev_enable.get() != 0 {
                    return Ok(());
                }
            }
            _ => {}
        }
    }

    let Some(tag) = sc.sc_kbctag.get() else {
        return Err(Errno::ENXIO);
    };

    match newstate {
        PMS_STATE_ENABLED => {
            sc.inputstate.set(0);
            sc.sc_rststate.set(0);

            pckbc_slot_enable(tag, PCKBC_AUX_SLOT, true);

            if sc.poll.get() != 0 {
                pckbc_flush(tag, PCKBC_AUX_SLOT);
            }

            let _ = pms_reset(sc);
            if let Some(enable) = sc.protocol().enable
                && !enable(sc)
            {
                pms_protocol_lookup(sc);
            }

            let _ = pms_dev_enable(sc);
        }
        PMS_STATE_DISABLED | PMS_STATE_SUSPENDED => {
            let _ = pms_dev_disable(sc);

            if let Some(disable) = sc.protocol().disable {
                disable(sc);
            }

            pckbc_slot_enable(tag, PCKBC_AUX_SLOT, false);
        }
        _ => {}
    }

    sc.sc_state.set(newstate);
    sc.poll.set(i32::from(newstate == PMS_STATE_SUSPENDED));

    Ok(())
}

/// `pms_enable`: the primary `wsmouse` opens; returns the errno.
pub fn pms_enable(v: *mut c_void) -> i32 {
    let sc = pms_cookie(v);

    rw_enter_write(&sc.sc_state_lock);
    let rv = pms_change_state(sc, PMS_STATE_ENABLED, PMS_DEV_PRIMARY);
    rw_exit_write(&sc.sc_state_lock);

    rv.map_or_else(|e| e as i32, |()| 0)
}

/// `pms_disable`: the primary `wsmouse` closes.
pub fn pms_disable(v: *mut c_void) {
    let sc = pms_cookie(v);

    rw_enter_write(&sc.sc_state_lock);
    let _ = pms_change_state(sc, PMS_STATE_DISABLED, PMS_DEV_PRIMARY);
    rw_exit_write(&sc.sc_state_lock);
}

/// `pms_ioctl`: the protocol's ioctl.
pub fn pms_ioctl(
    v: *mut c_void,
    cmd: u64,
    data: &mut [u8],
    flag: i32,
    p: Option<&Proc>,
) -> Result<bool, Errno> {
    let sc = pms_cookie(v);

    match sc.protocol().ioctl {
        Some(ioctl) => ioctl(sc, cmd, data, flag, p),
        None => Ok(false),
    }
}

/// `pms_sec_enable`: the trackpoint's `wsmouse` opens; returns the errno.
pub fn pms_sec_enable(v: *mut c_void) -> i32 {
    let sc = pms_cookie(v);

    rw_enter_write(&sc.sc_state_lock);
    let rv = pms_change_state(sc, PMS_STATE_ENABLED, PMS_DEV_SECONDARY);
    rw_exit_write(&sc.sc_state_lock);

    rv.map_or_else(|e| e as i32, |()| 0)
}

/// `pms_sec_disable`: the trackpoint's `wsmouse` closes.
pub fn pms_sec_disable(v: *mut c_void) {
    let sc = pms_cookie(v);

    rw_enter_write(&sc.sc_state_lock);
    let _ = pms_change_state(sc, PMS_STATE_DISABLED, PMS_DEV_SECONDARY);
    rw_exit_write(&sc.sc_state_lock);
}

/// `pms_sec_ioctl`: the trackpoint is a PS/2 mouse.
pub fn pms_sec_ioctl(
    _v: *mut c_void,
    cmd: u64,
    data: &mut [u8],
    _flag: i32,
    _p: Option<&Proc>,
) -> Result<bool, Errno> {
    match cmd {
        WSMOUSEIO_GTYPE => {
            ioctl_ret(data, &WSMOUSE_TYPE_PS2);
            Ok(true)
        }
        _ => Ok(false),
    }
}

/// `pms_print_packet` (`DIAGNOSTIC`): the packet so far, a bar after the byte at
/// `inputstate`.
fn pms_print_packet(sc: &PmsSoftc) {
    let state = sc.inputstate.get();
    let size = sc.protocol().packetsize;
    for (i, b) in sc.packet.get().iter().enumerate().take(size) {
        if i == state {
            printf(format_args!(" {b:02x} |"));
        } else {
            printf(format_args!(" {b:02x}"));
        }
    }
}

/// `pmsinput`: the input handler: collect a packet byte by byte, in sync with the
/// protocol, and hand a complete one to the protocol's `proc`.
pub fn pmsinput(vsc: *mut c_void, data: i32) {
    let sc = pms_cookie(vsc);

    if sc.sc_state.get() != PMS_STATE_ENABLED {
        // Interrupts are not expected.  Discard the byte.
        return;
    }

    let mut packet = sc.packet.get();
    if let Some(b) = packet.get_mut(sc.inputstate.get()) {
        *b = data as u8;
    }
    sc.packet.set(packet);
    pms_reset_detect(sc, data);
    let protocol = sc.protocol();
    if !(protocol.sync)(sc, data) {
        if cfg!(feature = "diagnostic") {
            printf(format_args!(
                "{}: not in sync yet, discard input (state = {},",
                sc.devname(),
                sc.inputstate.get()
            ));
            pms_print_packet(sc);
            printf(format_args!(")\n"));
        }

        sc.inputstate.set(0);
        return;
    }

    sc.inputstate.set(sc.inputstate.get() + 1);

    if sc.inputstate.get() != sc.protocol().packetsize {
        return;
    }

    sc.inputstate.set(0);
    (sc.protocol().proc_)(sc);
}

/// `synaptics_set_mode`: set the touchpad's mode (`rate` 0) or send `mode` as the
/// argument of the command `rate`.
pub fn synaptics_set_mode(sc: &PmsSoftc, mode: i32, rate: u8) -> Result<(), Errno> {
    pms_spec_cmd(sc, mode)?;
    pms_set_rate(
        sc,
        if rate == 0 {
            SYNAPTICS_CMD_SET_MODE
        } else {
            rate
        },
    )?;

    // Make sure that the set mode command has finished. Otherwise enabling the device
    // before that will make it fail.
    delay(10000);

    if rate == 0
        && let Some(syn) = sc.synaptics()
    {
        syn.mode.set(mode);
    }

    Ok(())
}

/// `synaptics_query`: the 24-bit answer to the Synaptics query `query`.
pub fn synaptics_query(sc: &PmsSoftc, query: i32) -> Result<i32, Errno> {
    let mut resp = [0u8; 3];

    pms_spec_cmd(sc, query)?;
    pms_get_status(sc, &mut resp)?;

    Ok((i32::from(resp[0]) << 16) | (i32::from(resp[1]) << 8) | i32::from(resp[2]))
}

/// `synaptics_get_hwinfo`: query the touchpad's identity, capabilities, model, resolution
/// and coordinate limits, and describe it to `wsmouse`.
pub fn synaptics_get_hwinfo(sc: &PmsSoftc) -> Result<(), Errno> {
    let (Some(syn), Some(dev)) = (sc.synaptics(), sc.wsmousedev()) else {
        return Err(Errno::ENXIO);
    };
    let mut resolution = 0;
    let mut max_coords = 0;
    let mut min_coords = 0;

    syn.identify
        .set(synaptics_query(sc, SYNAPTICS_QUE_IDENTIFY)?);
    syn.capabilities
        .set(synaptics_query(sc, SYNAPTICS_QUE_CAPABILITIES)?);
    syn.model.set(synaptics_query(sc, SYNAPTICS_QUE_MODEL)?);
    let queries = synaptics_cap_extended_queries(syn.capabilities.get());
    if queries >= 1 {
        syn.ext_model
            .set(synaptics_query(sc, SYNAPTICS_QUE_EXT_MODEL)?);
    }
    if queries >= 4 {
        syn.ext_capabilities
            .set(synaptics_query(sc, SYNAPTICS_QUE_EXT_CAPABILITIES)?);
    }
    if synaptics_id_major(syn.identify.get()) >= 4 {
        resolution = synaptics_query(sc, SYNAPTICS_QUE_RESOLUTION)?;
    }
    if queries >= 5 && syn.ext_capabilities.get() & SYNAPTICS_EXT_CAP_MAX_COORDS != 0 {
        max_coords = synaptics_query(sc, SYNAPTICS_QUE_EXT_MAX_COORDS)?;
    }
    if (queries >= 7 || synaptics_id_full(syn.identify.get()) == 0x801)
        && syn.ext_capabilities.get() & SYNAPTICS_EXT_CAP_MIN_COORDS != 0
    {
        min_coords = synaptics_query(sc, SYNAPTICS_QUE_EXT_MIN_COORDS)?;
    }

    if synaptics_id_full(syn.identify.get()) >= 0x705 {
        syn.modes.set(synaptics_query(sc, SYNAPTICS_QUE_MODES)?);
        if syn.modes.get() & SYNAPTICS_EXT2_CAP != 0 {
            syn.ext2_capabilities
                .set(synaptics_query(sc, SYNAPTICS_QUE_EXT2_CAPABILITIES)?);
        }
    }

    {
        let mut hw = wsmouse_get_hw(dev);
        let ext_caps = syn.ext_capabilities.get();

        hw.type_ = if ext_caps & SYNAPTICS_EXT_CAP_CLICKPAD != 0
            && syn.ext2_capabilities.get() & SYNAPTICS_EXT2_CAP_BUTTONS_STICK == 0
            && mouse_has_softbtn() != 0
        {
            WSMOUSE_TYPE_SYNAP_SBTN as i32
        } else {
            WSMOUSE_TYPE_SYNAPTICS as i32
        };

        hw.hw_type = if ext_caps & SYNAPTICS_EXT_CAP_CLICKPAD != 0 {
            WSMOUSEHW_CLICKPAD
        } else {
            WSMOUSEHW_TOUCHPAD
        };

        if resolution & SYNAPTICS_RESOLUTION_VALID != 0 {
            hw.h_res = synaptics_resolution_x(resolution);
            hw.v_res = synaptics_resolution_y(resolution);
        }

        hw.x_min = if min_coords != 0 {
            synaptics_x_limit(min_coords)
        } else {
            SYNAPTICS_XMIN_BEZEL
        };
        hw.y_min = if min_coords != 0 {
            synaptics_y_limit(min_coords)
        } else {
            SYNAPTICS_YMIN_BEZEL
        };
        hw.x_max = if max_coords != 0 {
            synaptics_x_limit(max_coords)
        } else {
            SYNAPTICS_XMAX_BEZEL
        };
        hw.y_max = if max_coords != 0 {
            synaptics_y_limit(max_coords)
        } else {
            SYNAPTICS_YMAX_BEZEL
        };

        hw.contacts_max = if syn.capabilities.get() & SYNAPTICS_CAP_MULTIFINGER != 0
            || synaptics_supports_agm(ext_caps) != 0
        {
            SYNAPTICS_MAX_FINGERS
        } else {
            1
        };
    }

    syn.sec_buttons.set(0);

    if synaptics_ext_model_buttons(syn.ext_model.get()) > 8 {
        syn.ext_model.set(syn.ext_model.get() & !0xf000);
    }

    if syn.model.get() & SYNAPTICS_MODEL_NEWABS == 0 {
        printf(format_args!(
            "{}: don't support Synaptics OLDABS\n",
            sc.devname()
        ));
        return Err(Errno::ENXIO);
    }

    let identify = syn.identify.get();
    syn.mask.set(
        if synaptics_id_major(identify) == 5 && synaptics_id_minor(identify) == 9 {
            SYNAPTICS_MASK_NEWABS_RELAXED
        } else {
            SYNAPTICS_MASK_NEWABS_STRICT
        },
    );

    Ok(())
}

/// `synaptics_sec_proc`: a pass-through packet of the trackpoint, a PS/2 packet in bytes 1,
/// 4 and 5.
pub fn synaptics_sec_proc(sc: &PmsSoftc) {
    let Some(syn) = sc.synaptics() else {
        return;
    };

    if sc.sc_dev_enable.get() & PMS_DEV_SECONDARY == 0 {
        return;
    }

    let p = sc.packet.get();
    let (buttons, dx, dy) = pms_ps2_motion(p[1], p[4], p[5]);
    let buttons = buttons | syn.sec_buttons.get();

    if let Some(dev) = sc.sec_wsmousedev() {
        crate::WSMOUSE_INPUT!(dev, buttons, dx, dy, 0, 0);
    }
}

/// `synaptics_knock`: four resolution 0 commands and a status request; a Synaptics
/// touchpad answers `SYNAPTICS_ID_MAGIC` in the middle byte.
pub fn synaptics_knock(sc: &PmsSoftc) -> Result<(), Errno> {
    let mut resp = [0u8; 3];

    pms_set_resolution(sc, 0)?;
    pms_set_resolution(sc, 0)?;
    pms_set_resolution(sc, 0)?;
    pms_set_resolution(sc, 0)?;
    pms_get_status(sc, &mut resp)?;
    if resp[1] != SYNAPTICS_ID_MAGIC {
        return Err(Errno::ENXIO);
    }

    Ok(())
}

/// `pms_enable_synaptics`: knock (retrying a known touchpad that does not resume quickly),
/// set the touchpad up the first time (sub-softc, hardware info, pass-through `wsmouse`,
/// `wsmouse` configuration) and put it in absolute and W mode (and the advanced gesture
/// mode when it has it).
pub fn pms_enable_synaptics(sc: &PmsSoftc) -> bool {
    let ok = 'err: {
        if synaptics_knock(sc).is_err() {
            if sc.synaptics.get().is_none() {
                break 'err false;
            }
            // Some synaptics touchpads don't resume quickly. Retry a few times.
            let mut i = 10;
            while i > 0 {
                printf(format_args!(
                    "{}: device not resuming, retrying\n",
                    sc.devname()
                ));
                let _ = pms_reset(sc);
                if synaptics_knock(sc).is_ok() {
                    break;
                }
                delay(100000);
                i -= 1;
            }
            if i == 0 {
                printf(format_args!("{}: lost device\n", sc.devname()));
                break 'err false;
            }
        }

        if sc.synaptics.get().is_none() {
            let size = size_of::<SynapticsSoftc>();
            let Some(mem) = malloc(size, M_DEVBUF, M_WAITOK | M_ZERO) else {
                printf(format_args!(
                    "{}: synaptics: not enough memory\n",
                    sc.devname()
                ));
                break 'err false;
            };
            let p: NonNull<SynapticsSoftc> = mem.cast();
            // SAFETY: a fresh block of `size_of::<SynapticsSoftc>()` bytes, aligned by
            // `malloc`, that nothing else references yet.
            unsafe { p.as_ptr().write(SynapticsSoftc::new()) };
            sc.synaptics.set(Some(p));

            if synaptics_get_hwinfo(sc).is_err() {
                sc.synaptics.set(None);
                free(mem, M_DEVBUF, size);
                break 'err false;
            }

            let Some(syn) = sc.synaptics() else {
                break 'err false;
            };

            // enable pass-through PS/2 port if supported
            if syn.capabilities.get() & SYNAPTICS_CAP_PASSTHROUGH != 0 {
                let mut a = WsmousedevAttachArgs {
                    accessops: &PMS_SEC_ACCESSOPS,
                    accesscookie: sc.cookie(),
                };
                sc.sc_sec_wsmousedev.set(config_found(
                    &sc.sc_dev,
                    ptr::from_mut(&mut a).cast(),
                    Some(wsmousedevprint),
                ));
            }

            let configured = sc
                .wsmousedev()
                .map(|dev| wsmouse_configure(dev, Some(&SYNAPTICS_PARAMS)));
            if !matches!(configured, Some(Ok(()))) {
                break 'err false;
            }

            let ext_caps = syn.ext_capabilities.get();
            printf(format_args!(
                "{}: Synaptics {}, firmware {}.{}, {:#x} {:#x} {:#x} {:#x} {:#x}\n",
                sc.devname(),
                if ext_caps & SYNAPTICS_EXT_CAP_CLICKPAD != 0 {
                    "clickpad"
                } else {
                    "touchpad"
                },
                synaptics_id_major(syn.identify.get()),
                synaptics_id_minor(syn.identify.get()),
                syn.model.get(),
                syn.ext_model.get(),
                syn.modes.get(),
                syn.capabilities.get(),
                ext_caps
            ));
        }

        let Some(syn) = sc.synaptics() else {
            break 'err false;
        };

        // Enable absolute mode, plain W-mode and "advanced gesture mode" (AGM), if
        // possible.  AGM, which seems to be a prerequisite for the extended W-mode, might
        // not always be necessary here, but at least some older Synaptics models do not
        // report finger counts without it.
        let mut mode = SYNAPTICS_ABSOLUTE_MODE | SYNAPTICS_HIGH_RATE;
        if syn.capabilities.get() & SYNAPTICS_CAP_EXTENDED != 0 {
            mode |= SYNAPTICS_W_MODE;
        } else if synaptics_id_major(syn.identify.get()) >= 4 {
            mode |= SYNAPTICS_DISABLE_GESTURE;
        }
        if synaptics_set_mode(sc, mode, 0).is_err() {
            break 'err false;
        }

        if synaptics_supports_agm(syn.ext_capabilities.get()) != 0
            && synaptics_set_mode(sc, SYNAPTICS_QUE_MODEL, SYNAPTICS_CMD_SET_ADV_GESTURE_MODE)
                .is_err()
        {
            break 'err false;
        }

        true
    };

    if !ok {
        // err:
        let _ = pms_reset(sc);
    }
    ok
}

/// `pms_ioctl_synaptics`: the type, the calibration (the coordinate limits and the
/// resolution) and the mode of a Synaptics touchpad.
pub fn pms_ioctl_synaptics(
    sc: &PmsSoftc,
    cmd: u64,
    data: &mut [u8],
    _flag: i32,
    _p: Option<&Proc>,
) -> Result<bool, Errno> {
    let Some(dev) = sc.wsmousedev() else {
        return Err(Errno::ENXIO);
    };
    let hw = *wsmouse_get_hw(dev);

    match cmd {
        WSMOUSEIO_GTYPE => ioctl_ret(data, &(hw.type_ as u32)),
        WSMOUSEIO_GCALIBCOORDS => {
            let mut wsmc: WsmouseCalibcoords = ioctl_arg(data);
            wsmc.minx = hw.x_min;
            wsmc.maxx = hw.x_max;
            wsmc.miny = hw.y_min;
            wsmc.maxy = hw.y_max;
            wsmc.swapxy = 0;
            wsmc.resx = hw.h_res;
            wsmc.resy = hw.v_res;
            ioctl_ret(data, &wsmc);
        }
        WSMOUSEIO_SETMODE => {
            let wsmode = ioctl_arg::<u32>(data) as i32;
            if wsmode != WSMOUSE_COMPAT && wsmode != WSMOUSE_NATIVE {
                return Err(Errno::EINVAL);
            }
            let _ = wsmouse_set_mode(dev, wsmode);
        }
        _ => return Ok(false),
    }
    Ok(true)
}

/// `pms_sync_synaptics`: the first and the fourth byte of a "new absolute" packet carry
/// fixed bits.
pub fn pms_sync_synaptics(sc: &PmsSoftc, data: i32) -> bool {
    let Some(syn) = sc.synaptics() else {
        return false;
    };
    let mask = syn.mask.get();

    match sc.inputstate.get() {
        0 => data & mask == SYNAPTICS_VALID_NEWABS_FIRST,
        3 => data & mask == SYNAPTICS_VALID_NEWABS_NEXT,
        _ => true,
    }
}

/// `pms_proc_synaptics`: report a touchpad packet (position, pressure, finger width and
/// count, buttons), or hand a pass-through packet to the trackpoint.
pub fn pms_proc_synaptics(sc: &PmsSoftc) {
    let Some(syn) = sc.synaptics() else {
        return;
    };
    let p = sc.packet.get();
    let (p0, p1, p3, p4, p5) = (
        i32::from(p[0]),
        i32::from(p[1]),
        i32::from(p[3]),
        i32::from(p[4]),
        i32::from(p[5]),
    );

    let mut w = ((p0 & 0x30) >> 2) | ((p0 & 0x04) >> 1) | ((p3 & 0x04) >> 2);
    let mut z = i32::from(p[2]);

    if syn.capabilities.get() & SYNAPTICS_CAP_EXTENDED == 0 {
        // Emulate W mode for models that don't provide it. Bit 3 of the w-input signals a
        // touch ("finger"), Bit 2 and the "gesture" bits 1-0 can be ignored.
        if w & 8 != 0 {
            w = 4;
        } else {
            z = 0;
            w = 0;
        }
    }

    if w == 3 {
        if syn.capabilities.get() & SYNAPTICS_CAP_PASSTHROUGH != 0 {
            synaptics_sec_proc(sc);
        }
        return;
    }

    if sc.sc_dev_enable.get() & PMS_DEV_PRIMARY == 0 {
        return;
    }

    if w == 2 {
        return; // EW-mode packets are not expected here.
    }

    let mut x = ((p3 & 0x10) << 8) | ((p1 & 0x0f) << 8) | p4;
    let mut y = ((p3 & 0x20) << 7) | ((p1 & 0xf0) << 4) | p5;

    let mut buttons = if (p0 & p3) & 0x01 != 0 {
        wsmouse_button(1)
    } else {
        0
    };
    if (p0 & p3) & 0x02 != 0 {
        buttons |= wsmouse_button(3);
    }

    if syn.ext_capabilities.get() & SYNAPTICS_EXT_CAP_CLICKPAD != 0 {
        if (p0 ^ p3) & 0x01 != 0 {
            buttons |= wsmouse_button(1);
        }
    } else if syn.capabilities.get() & SYNAPTICS_CAP_MIDDLE_BUTTON != 0 && (p0 ^ p3) & 0x01 != 0 {
        buttons |= wsmouse_button(2);
    }

    if syn.capabilities.get() & SYNAPTICS_CAP_FOUR_BUTTON != 0 {
        if (p0 ^ p3) & 0x01 != 0 {
            buttons |= wsmouse_button(4);
        }
        if (p0 ^ p3) & 0x02 != 0 {
            buttons |= wsmouse_button(5);
        }
    } else if synaptics_ext_model_buttons(syn.ext_model.get()) != 0 && (p0 ^ p3) & 0x02 != 0 {
        if syn.ext2_capabilities.get() & SYNAPTICS_EXT2_CAP_BUTTONS_STICK != 0 {
            // Trackstick buttons on this machine are wired to the trackpad as extra
            // buttons, so route the event through the trackstick interface as normal
            // buttons
            let mut sec = if p4 & 0x01 != 0 { wsmouse_button(1) } else { 0 };
            if p5 & 0x01 != 0 {
                sec |= wsmouse_button(3);
            }
            if p4 & 0x02 != 0 {
                sec |= wsmouse_button(2);
            }
            syn.sec_buttons.set(sec);
            if let Some(sdev) = sc.sec_wsmousedev() {
                wsmouse_buttons(sdev, sec);
                wsmouse_input_sync(sdev);
            }
            return;
        }

        for (n, (byte, bit)) in [
            (p4, 0x01),
            (p5, 0x01),
            (p4, 0x02),
            (p5, 0x02),
            (p4, 0x04),
            (p5, 0x04),
            (p4, 0x08),
            (p5, 0x08),
        ]
        .into_iter()
        .enumerate()
        {
            if byte & bit != 0 {
                buttons |= wsmouse_button(6 + n as u32);
            }
        }
        x &= !0x0f;
        y &= !0x0f;
    }

    let fingerwidth = if z != 0 {
        let fingerwidth = w.max(4);
        w = if w < 2 { w + 2 } else { 1 };
        fingerwidth
    } else {
        w = 0;
        0
    };
    if let Some(dev) = sc.wsmousedev() {
        wsmouse_set(dev, WSMOUSE_TOUCH_WIDTH, fingerwidth, 0);
        crate::WSMOUSE_TOUCH!(dev, buttons, x, y, z, w);
    }
}

/// `pms_disable_synaptics`: put a touchpad that can sleep to sleep.
pub fn pms_disable_synaptics(sc: &PmsSoftc) {
    let Some(syn) = sc.synaptics() else {
        return;
    };

    if syn.capabilities.get() & SYNAPTICS_CAP_SLEEP != 0 {
        let _ = synaptics_set_mode(sc, SYNAPTICS_SLEEP_MODE | SYNAPTICS_DISABLE_GESTURE, 0);
    }
}

/// `alps_sec_proc`: a trackpoint packet of a Dualpoint, a PS/2 packet in the first three
/// bytes or (interleaved) in the last three; `true` when the packet was the trackpoint's.
pub fn alps_sec_proc(sc: &PmsSoftc) -> bool {
    let Some(alps) = sc.alps() else {
        return false;
    };
    let p = sc.packet.get();

    let pos = if p[0] & PMS_ALPS_PS2_MASK == PMS_ALPS_PS2_VALID {
        // We need to keep buttons states because interleaved packets only signalize x/y
        // movements.
        alps.sec_buttons
            .set(BUTMAP[usize::from(p[0] & PMS_PS2_BUTTONSMASK)]);
        0
    } else if p[3] & PMS_ALPS_INTERLEAVED_MASK == PMS_ALPS_INTERLEAVED_VALID {
        sc.inputstate.set(3);
        3
    } else {
        return false;
    };

    if sc.sc_dev_enable.get() & PMS_DEV_SECONDARY == 0 {
        return true;
    }

    let (_, dx, dy) = pms_ps2_motion(p[pos], p[pos + 1], p[pos + 2]);

    if let Some(dev) = sc.sec_wsmousedev() {
        crate::WSMOUSE_INPUT!(dev, alps.sec_buttons.get(), dx, dy, 0, 0);
    }

    true
}

/// `alps_get_hwinfo`: the touchpad's version (an "E7" report), looked up in
/// [`ALPS_MODELS`]; a known model is described to `wsmouse`.
pub fn alps_get_hwinfo(sc: &PmsSoftc) -> Result<(), Errno> {
    let Some(alps) = sc.alps() else {
        return Err(Errno::ENXIO);
    };
    let mut resp = [0u8; 3];

    pms_set_resolution(sc, 0)?;
    pms_set_scaling(sc, 2)?;
    pms_set_scaling(sc, 2)?;
    pms_set_scaling(sc, 2)?;
    // DPRINTF("%s: alps: model query error\n", ...) on a failure: DEBUG, not compiled.
    pms_get_status(sc, &mut resp)?;

    alps.version
        .set((i32::from(resp[0]) << 8) | (i32::from(resp[1]) << 4) | (i32::from(resp[2]) / 20 + 1));

    for m in &ALPS_MODELS {
        if alps.version.get() == m.version {
            alps.model.set(m.model);
            alps.mask.set(m.mask);

            let Some(dev) = sc.wsmousedev() else {
                return Err(Errno::ENXIO);
            };
            let mut hw = wsmouse_get_hw(dev);
            hw.type_ = WSMOUSE_TYPE_ALPS as i32;
            hw.hw_type = WSMOUSEHW_TOUCHPAD;
            hw.x_min = ALPS_XMIN_BEZEL;
            hw.y_min = ALPS_YMIN_BEZEL;
            hw.x_max = ALPS_XMAX_BEZEL;
            hw.y_max = ALPS_YMAX_BEZEL;
            hw.contacts_max = 1;

            return Ok(());
        }
    }

    Err(Errno::ENXIO)
}

/// `pms_enable_alps`: knock (resolution 0, three scalings 1:1, a status request whose
/// answer is `0, 0, 10|80|100`), set the touchpad up the first time (sub-softc, model,
/// `wsmouse` configuration, the Dualpoint's trackpoint `wsmouse`) and switch it to
/// absolute mode (with the pass-through port of a Dualpoint on around it).
pub fn pms_enable_alps(sc: &PmsSoftc) -> bool {
    let ok = 'err: {
        let mut resp = [0u8; 3];

        if pms_set_resolution(sc, 0).is_err()
            || pms_set_scaling(sc, 1).is_err()
            || pms_set_scaling(sc, 1).is_err()
            || pms_set_scaling(sc, 1).is_err()
            || pms_get_status(sc, &mut resp).is_err()
            || resp[0] != PMS_ALPS_MAGIC1
            || resp[1] != PMS_ALPS_MAGIC2
            || (resp[2] != PMS_ALPS_MAGIC3_1
                && resp[2] != PMS_ALPS_MAGIC3_2
                && resp[2] != PMS_ALPS_MAGIC3_3)
        {
            break 'err false;
        }

        if sc.alps.get().is_none() {
            let size = size_of::<AlpsSoftc>();
            let Some(mem) = malloc(size, M_DEVBUF, M_WAITOK | M_ZERO) else {
                printf(format_args!("{}: alps: not enough memory\n", sc.devname()));
                break 'err false;
            };
            let p: NonNull<AlpsSoftc> = mem.cast();
            // SAFETY: a fresh block of `size_of::<AlpsSoftc>()` bytes, aligned by
            // `malloc`, that nothing else references yet.
            unsafe { p.as_ptr().write(AlpsSoftc::new()) };
            sc.alps.set(Some(p));

            if alps_get_hwinfo(sc).is_err() {
                sc.alps.set(None);
                free(mem, M_DEVBUF, size);
                break 'err false;
            }

            let configured = sc
                .wsmousedev()
                .map(|dev| wsmouse_configure(dev, Some(&ALPS_PARAMS)));
            if !matches!(configured, Some(Ok(()))) {
                sc.alps.set(None);
                free(mem, M_DEVBUF, size);
                printf(format_args!("{}: setup failed\n", sc.devname()));
                break 'err false;
            }

            let Some(alps) = sc.alps() else {
                break 'err false;
            };
            printf(format_args!(
                "{}: ALPS {}, version {:#06x}\n",
                sc.devname(),
                if alps.model.get() & ALPS_DUALPOINT != 0 {
                    "Dualpoint"
                } else {
                    "Glidepoint"
                },
                alps.version.get()
            ));

            if alps.model.get() & ALPS_DUALPOINT != 0 {
                let mut a = WsmousedevAttachArgs {
                    accessops: &PMS_SEC_ACCESSOPS,
                    accesscookie: sc.cookie(),
                };
                sc.sc_sec_wsmousedev.set(config_found(
                    &sc.sc_dev,
                    ptr::from_mut(&mut a).cast(),
                    Some(wsmousedevprint),
                ));
            }
        }

        let Some(alps) = sc.alps() else {
            break 'err false;
        };

        if alps.model.get() == 0 {
            break 'err false;
        }

        if alps.model.get() & ALPS_PASSTHROUGH != 0
            && (pms_set_scaling(sc, 2).is_err()
                || pms_set_scaling(sc, 2).is_err()
                || pms_set_scaling(sc, 2).is_err()
                || pms_dev_disable(sc).is_err())
        {
            // DPRINTF("%s: alps: passthrough on error\n", ...): DEBUG, not compiled.
            break 'err false;
        }

        if pms_dev_disable(sc).is_err()
            || pms_dev_disable(sc).is_err()
            || pms_set_rate(sc, 0x0a).is_err()
        {
            // DPRINTF("%s: alps: tapping error\n", ...): DEBUG, not compiled.
            break 'err false;
        }

        if pms_dev_disable(sc).is_err()
            || pms_dev_disable(sc).is_err()
            || pms_dev_disable(sc).is_err()
            || pms_dev_disable(sc).is_err()
            || pms_dev_enable(sc).is_err()
        {
            // DPRINTF("%s: alps: absolute mode error\n", ...): DEBUG, not compiled.
            break 'err false;
        }

        if alps.model.get() & ALPS_PASSTHROUGH != 0
            && (pms_set_scaling(sc, 1).is_err()
                || pms_set_scaling(sc, 1).is_err()
                || pms_set_scaling(sc, 1).is_err()
                || pms_dev_disable(sc).is_err())
        {
            // DPRINTF("%s: alps: passthrough off error\n", ...): DEBUG, not compiled.
            break 'err false;
        }

        alps.sec_buttons.set(0);

        true
    };

    if !ok {
        // err:
        let _ = pms_reset(sc);
    }
    ok
}

/// `pms_ioctl_alps`: the type, the calibration and the mode of an ALPS touchpad.
pub fn pms_ioctl_alps(
    sc: &PmsSoftc,
    cmd: u64,
    data: &mut [u8],
    _flag: i32,
    _p: Option<&Proc>,
) -> Result<bool, Errno> {
    match cmd {
        WSMOUSEIO_GTYPE => ioctl_ret(data, &WSMOUSE_TYPE_ALPS),
        WSMOUSEIO_GCALIBCOORDS => {
            let Some(dev) = sc.wsmousedev() else {
                return Err(Errno::ENXIO);
            };
            let hw = *wsmouse_get_hw(dev);
            let mut wsmc: WsmouseCalibcoords = ioctl_arg(data);
            wsmc.minx = hw.x_min;
            wsmc.maxx = hw.x_max;
            wsmc.miny = hw.y_min;
            wsmc.maxy = hw.y_max;
            wsmc.swapxy = 0;
            ioctl_ret(data, &wsmc);
        }
        WSMOUSEIO_SETMODE => {
            let wsmode = ioctl_arg::<u32>(data) as i32;
            if wsmode != WSMOUSE_COMPAT && wsmode != WSMOUSE_NATIVE {
                return Err(Errno::EINVAL);
            }
            if let Some(dev) = sc.wsmousedev() {
                let _ = wsmouse_set_mode(dev, wsmode);
            }
        }
        _ => return Ok(false),
    }
    Ok(true)
}

/// `pms_sync_alps`: a Dualpoint's PS/2 packet is three bytes long (the sync jumps to the
/// end of the six); the first byte of a touchpad packet matches the model's mask, the
/// others have bit 7 clear (bytes 4 and 5 of an interleaved model may not).
pub fn pms_sync_alps(sc: &PmsSoftc, data: i32) -> bool {
    let Some(alps) = sc.alps() else {
        return false;
    };
    let p0 = sc.packet.get()[0];

    if alps.model.get() & ALPS_DUALPOINT != 0 && p0 & PMS_ALPS_PS2_MASK == PMS_ALPS_PS2_VALID {
        if sc.inputstate.get() == 2 {
            sc.inputstate.set(sc.inputstate.get() + 3);
        }
        return true;
    }

    let mask = alps.mask.get();
    let valid = data & i32::from(PMS_ALPS_MASK) == i32::from(PMS_ALPS_VALID);
    match sc.inputstate.get() {
        0 => data & mask == mask,
        1..=3 => valid,
        4 | 5 => alps.model.get() & ALPS_INTERLEAVED != 0 || valid,
        _ => true,
    }
}

/// `pms_proc_alps`: report a touchpad packet (or a trackpoint one), turning the ALPS tap
/// and drag gestures into touches.
pub fn pms_proc_alps(sc: &PmsSoftc) {
    let Some(alps) = sc.alps() else {
        return;
    };

    if alps.model.get() & ALPS_DUALPOINT != 0 && alps_sec_proc(sc) {
        return;
    }

    let p = sc.packet.get();
    let x = i32::from(p[1]) | ((i32::from(p[2]) & 0x78) << 4);
    let mut y = i32::from(p[4]) | ((i32::from(p[3]) & 0x70) << 3);
    let z = i32::from(p[5]);

    let buttons = (if p[3] & 1 != 0 { wsmouse_button(1) } else { 0 })
        | (if p[3] & 2 != 0 { wsmouse_button(3) } else { 0 })
        | (if p[3] & 4 != 0 { wsmouse_button(2) } else { 0 });

    if sc.sc_dev_enable.get() & PMS_DEV_SECONDARY != 0 && z == ALPS_Z_MAGIC {
        let dx = if x > ALPS_XSEC_BEZEL / 2 {
            x - ALPS_XSEC_BEZEL
        } else {
            x
        };
        let dy = if y > ALPS_YSEC_BEZEL / 2 {
            y - ALPS_YSEC_BEZEL
        } else {
            y
        };

        if let Some(dev) = sc.sec_wsmousedev() {
            crate::WSMOUSE_INPUT!(dev, buttons, dx, dy, 0, 0);
        }

        return;
    }

    if sc.sc_dev_enable.get() & PMS_DEV_PRIMARY == 0 {
        return;
    }
    let Some(dev) = sc.wsmousedev() else {
        return;
    };

    // XXX The Y-axis is in the opposite direction compared to Synaptics touchpads and PS/2
    // mouses. It's why we need to translate the y value here for both NATIVE and COMPAT
    // modes.
    y = ALPS_YMAX_BEZEL - y + ALPS_YMIN_BEZEL;

    if alps.gesture.get() == ALPS_TAP {
        // Report a touch with the tap coordinates.
        crate::WSMOUSE_TOUCH!(
            dev,
            buttons,
            alps.old_x.get(),
            alps.old_y.get(),
            ALPS_PRESSURE,
            0
        );
        if z > 0 {
            // The hardware doesn't send a null pressure event when dragging starts.
            crate::WSMOUSE_TOUCH!(dev, buttons, alps.old_x.get(), alps.old_y.get(), 0, 0);
        }
    }

    let gesture = u32::from(p[2] & 0x03);
    if gesture != ALPS_TAP {
        crate::WSMOUSE_TOUCH!(dev, buttons, x, y, z, 0);
    }

    if alps.gesture.get() != ALPS_DRAG || gesture != ALPS_TAP {
        alps.gesture.set(gesture);
    }

    alps.old_x.set(x);
    alps.old_y.set(y);
}

/// `elantech_set_absolute_mode_v1`: enable absolute mode (magic numbers from Linux driver)
/// and read register 0x10 back to ensure hardware is ready.
pub fn elantech_set_absolute_mode_v1(sc: &PmsSoftc) -> Result<(), Errno> {
    let mut resp = [0u8; 3];

    // Enable absolute mode. Magic numbers from Linux driver.
    pms_spec_cmd(sc, i32::from(ELANTECH_CMD_WRITE_REG))?;
    pms_spec_cmd(sc, 0x10)?;
    pms_spec_cmd(sc, 0x16)?;
    pms_set_scaling(sc, 1)?;
    pms_spec_cmd(sc, i32::from(ELANTECH_CMD_WRITE_REG))?;
    pms_spec_cmd(sc, 0x11)?;
    pms_spec_cmd(sc, 0x8f)?;
    pms_set_scaling(sc, 1)?;

    // Read back reg 0x10 to ensure hardware is ready. (A failed command ends the loop as
    // the answer does, as in the C.)
    let mut i = 0;
    while i < 5 {
        if pms_spec_cmd(sc, i32::from(ELANTECH_CMD_READ_REG)).is_err()
            || pms_spec_cmd(sc, 0x10).is_err()
            || pms_get_status(sc, &mut resp).is_ok()
        {
            break;
        }
        delay(2000);
        i += 1;
    }
    if i == 5 {
        return Err(Errno::ENXIO);
    }

    if resp[0] & ELANTECH_ABSOLUTE_MODE == 0 {
        return Err(Errno::ENXIO);
    }

    Ok(())
}

/// `elantech_set_absolute_mode_v2`: as v1, through the "custom command" escape.
pub fn elantech_set_absolute_mode_v2(sc: &PmsSoftc) -> Result<(), Errno> {
    let mut resp = [0u8; 3];
    let fw_version = sc.elantech().map_or(0, |e| e.fw_version.get());
    let reg10: u8 = if fw_version == 0x20030 { 0x54 } else { 0xc4 };

    // Enable absolute mode. Magic numbers from Linux driver.
    for c in [
        ELANTECH_PS2_CUSTOM_COMMAND,
        ELANTECH_CMD_WRITE_REG,
        ELANTECH_PS2_CUSTOM_COMMAND,
        0x10,
        ELANTECH_PS2_CUSTOM_COMMAND,
        reg10,
    ] {
        elantech_ps2_cmd(sc, c)?;
    }
    pms_set_scaling(sc, 1)?;
    for c in [
        ELANTECH_PS2_CUSTOM_COMMAND,
        ELANTECH_CMD_WRITE_REG,
        ELANTECH_PS2_CUSTOM_COMMAND,
        0x11,
        ELANTECH_PS2_CUSTOM_COMMAND,
        0x88,
    ] {
        elantech_ps2_cmd(sc, c)?;
    }
    pms_set_scaling(sc, 1)?;

    // Read back reg 0x10 to ensure hardware is ready.
    let mut i = 0;
    while i < 5 {
        if elantech_ps2_cmd(sc, ELANTECH_PS2_CUSTOM_COMMAND).is_err()
            || elantech_ps2_cmd(sc, ELANTECH_CMD_READ_REG).is_err()
            || elantech_ps2_cmd(sc, ELANTECH_PS2_CUSTOM_COMMAND).is_err()
            || elantech_ps2_cmd(sc, 0x10).is_err()
            || pms_get_status(sc, &mut resp).is_ok()
        {
            break;
        }
        delay(2000);
        i += 1;
    }
    if i == 5 {
        return Err(Errno::ENXIO);
    }

    Ok(())
}

/// `elantech_set_absolute_mode_v3`.
pub fn elantech_set_absolute_mode_v3(sc: &PmsSoftc) -> Result<(), Errno> {
    let mut resp = [0u8; 3];

    // Enable absolute mode. Magic numbers from Linux driver.
    for c in [
        ELANTECH_PS2_CUSTOM_COMMAND,
        ELANTECH_CMD_READ_WRITE_REG,
        ELANTECH_PS2_CUSTOM_COMMAND,
        0x10,
        ELANTECH_PS2_CUSTOM_COMMAND,
        0x0b,
    ] {
        elantech_ps2_cmd(sc, c)?;
    }
    pms_set_scaling(sc, 1)?;

    // Read back reg 0x10 to ensure hardware is ready.
    let mut i = 0;
    while i < 5 {
        if elantech_ps2_cmd(sc, ELANTECH_PS2_CUSTOM_COMMAND).is_err()
            || elantech_ps2_cmd(sc, ELANTECH_CMD_READ_WRITE_REG).is_err()
            || elantech_ps2_cmd(sc, ELANTECH_PS2_CUSTOM_COMMAND).is_err()
            || elantech_ps2_cmd(sc, 0x10).is_err()
            || pms_get_status(sc, &mut resp).is_ok()
        {
            break;
        }
        delay(2000);
        i += 1;
    }
    if i == 5 {
        return Err(Errno::ENXIO);
    }

    Ok(())
}

/// `elantech_set_absolute_mode_v4`: v4 has no register 0x10 to read the answer from.
pub fn elantech_set_absolute_mode_v4(sc: &PmsSoftc) -> Result<(), Errno> {
    // Enable absolute mode. Magic numbers from Linux driver.
    for c in [
        ELANTECH_PS2_CUSTOM_COMMAND,
        ELANTECH_CMD_READ_WRITE_REG,
        ELANTECH_PS2_CUSTOM_COMMAND,
        0x07,
        ELANTECH_PS2_CUSTOM_COMMAND,
        ELANTECH_CMD_READ_WRITE_REG,
        ELANTECH_PS2_CUSTOM_COMMAND,
        0x01,
    ] {
        elantech_ps2_cmd(sc, c)?;
    }
    pms_set_scaling(sc, 1)?;

    // v4 has no register 0x10 to read response from

    Ok(())
}

/// `elantech_get_hwinfo_v1`: a version 1 firmware (below 0x20030, or 0x20600), its rocker,
/// absolute mode and the fixed axis ranges.
pub fn elantech_get_hwinfo_v1(sc: &PmsSoftc) -> Result<(), Errno> {
    let (Some(elantech), Some(dev)) = (sc.elantech(), sc.wsmousedev()) else {
        return Err(Errno::ENXIO);
    };
    let mut capabilities = [0u8; 3];

    let fw_version = synaptics_query(sc, ELANTECH_QUE_FW_VER)?;

    if fw_version < 0x20030 || fw_version == 0x20600 {
        if fw_version < 0x20000 {
            elantech
                .flags
                .set(elantech.flags.get() | ELANTECH_F_HW_V1_OLD);
        }
    } else {
        return Err(Errno::ENXIO);
    }

    elantech.fw_version.set(fw_version);

    pms_spec_cmd(sc, ELANTECH_QUE_CAPABILITIES)?;
    pms_get_status(sc, &mut capabilities)?;

    if capabilities[0] & ELANTECH_CAP_HAS_ROCKER != 0 {
        elantech
            .flags
            .set(elantech.flags.get() | ELANTECH_F_HAS_ROCKER);
    }

    elantech_set_absolute_mode_v1(sc)?;

    let mut hw = wsmouse_get_hw(dev);
    hw.type_ = WSMOUSE_TYPE_ELANTECH as i32;
    hw.hw_type = WSMOUSEHW_TOUCHPAD;
    hw.x_min = ELANTECH_V1_X_MIN;
    hw.x_max = ELANTECH_V1_X_MAX;
    hw.y_min = ELANTECH_V1_Y_MIN;
    hw.y_max = ELANTECH_V1_Y_MAX;

    Ok(())
}

/// `elantech_get_hwinfo_v2`: a version 2 firmware (IC version 2 or 4), absolute mode, and
/// the axis ranges (fixed for the older variants, which lack the ID query).
pub fn elantech_get_hwinfo_v2(sc: &PmsSoftc) -> Result<(), Errno> {
    let (Some(elantech), Some(dev)) = (sc.elantech(), sc.wsmousedev()) else {
        return Err(Errno::ENXIO);
    };
    let mut capabilities = [0u8; 3];
    let mut resp = [0u8; 3];

    let fw_version = synaptics_query(sc, ELANTECH_QUE_FW_VER)?;

    let ic_ver = (fw_version & 0x0f0000) >> 16;
    if ic_ver != 2 && ic_ver != 4 {
        return Err(Errno::ENXIO);
    }

    elantech.fw_version.set(fw_version);
    if fw_version >= 0x20800 {
        elantech
            .flags
            .set(elantech.flags.get() | ELANTECH_F_REPORTS_PRESSURE);
    }

    pms_spec_cmd(sc, ELANTECH_QUE_CAPABILITIES)?;
    pms_get_status(sc, &mut capabilities)?;

    elantech_set_absolute_mode_v2(sc)?;

    {
        let mut hw = wsmouse_get_hw(dev);
        hw.type_ = WSMOUSE_TYPE_ELANTECH as i32;
        hw.hw_type = WSMOUSEHW_TOUCHPAD;
    }

    let (cap1, cap2) = (i32::from(capabilities[1]), i32::from(capabilities[2]));
    let (x_max, y_max) = if fw_version == 0x20800 || fw_version == 0x20b00 || fw_version == 0x20030
    {
        (ELANTECH_V2_X_MAX, ELANTECH_V2_Y_MAX)
    } else {
        pms_spec_cmd(sc, ELANTECH_QUE_FW_ID)?;
        pms_get_status(sc, &mut resp)?;
        let fixed_dpi = resp[1] & 0x10;
        let i = if fw_version > 0x20800 && fw_version < 0x20900 {
            1
        } else {
            2
        };
        if (fw_version >> 16) == 0x14 && fixed_dpi != 0 {
            pms_spec_cmd(sc, ELANTECH_QUE_SAMPLE)?;
            pms_get_status(sc, &mut resp)?;
            (
                (cap1 - i) * i32::from(resp[1]) / 2,
                (cap2 - i) * i32::from(resp[2]) / 2,
            )
        } else if fw_version == 0x040216 {
            (819, 405)
        } else if fw_version == 0x040219 || fw_version == 0x040215 {
            (900, 500)
        } else {
            ((cap1 - i) * 64, (cap2 - i) * 64)
        }
    };

    let mut hw = wsmouse_get_hw(dev);
    hw.x_max = x_max;
    hw.y_max = y_max;

    Ok(())
}

/// `elantech_get_hwinfo_v3`: a version 3 firmware (IC version 5), absolute mode and the
/// axis ranges of the ID query.
pub fn elantech_get_hwinfo_v3(sc: &PmsSoftc) -> Result<(), Errno> {
    let (Some(elantech), Some(dev)) = (sc.elantech(), sc.wsmousedev()) else {
        return Err(Errno::ENXIO);
    };
    let mut resp = [0u8; 3];

    let fw_version = synaptics_query(sc, ELANTECH_QUE_FW_VER)?;

    if ((fw_version & 0x0f0000) >> 16) != 5 {
        return Err(Errno::ENXIO);
    }

    elantech.fw_version.set(fw_version);
    elantech
        .flags
        .set(elantech.flags.get() | ELANTECH_F_REPORTS_PRESSURE);

    if (fw_version & 0x4000) == 0x4000 {
        elantech
            .flags
            .set(elantech.flags.get() | ELANTECH_F_CRC_ENABLED);
    }

    elantech_set_absolute_mode_v3(sc)?;

    pms_spec_cmd(sc, ELANTECH_QUE_FW_ID)?;
    pms_get_status(sc, &mut resp)?;

    let (r0, r1, r2) = (i32::from(resp[0]), i32::from(resp[1]), i32::from(resp[2]));
    elantech.max_x.set(((r0 & 0x0f) << 8) | r1);
    elantech.max_y.set(((r0 & 0xf0) << 4) | r2);
    let mut hw = wsmouse_get_hw(dev);
    hw.x_max = elantech.max_x.get();
    hw.y_max = elantech.max_y.get();

    hw.type_ = WSMOUSE_TYPE_ELANTECH as i32;
    hw.hw_type = WSMOUSEHW_TOUCHPAD;

    Ok(())
}

/// `elantech_get_hwinfo_v4`: a version 4 firmware (IC version 6 or later), absolute mode,
/// the axis ranges, the trace count (the width of a trace) and the trackpoint.
pub fn elantech_get_hwinfo_v4(sc: &PmsSoftc) -> Result<(), Errno> {
    let (Some(elantech), Some(dev)) = (sc.elantech(), sc.wsmousedev()) else {
        return Err(Errno::ENXIO);
    };
    let mut capabilities = [0u8; 3];
    let mut resp = [0u8; 3];

    let fw_version = synaptics_query(sc, ELANTECH_QUE_FW_VER)?;

    if (fw_version & 0x0f0000) >> 16 < 6 {
        return Err(Errno::ENXIO);
    }

    elantech.fw_version.set(fw_version);
    elantech
        .flags
        .set(elantech.flags.get() | ELANTECH_F_REPORTS_PRESSURE);

    if (fw_version & 0x4000) == 0x4000 {
        elantech
            .flags
            .set(elantech.flags.get() | ELANTECH_F_CRC_ENABLED);
    }

    elantech_set_absolute_mode_v4(sc)?;

    pms_spec_cmd(sc, ELANTECH_QUE_CAPABILITIES)?;
    pms_get_status(sc, &mut capabilities)?;

    pms_spec_cmd(sc, ELANTECH_QUE_FW_ID)?;
    pms_get_status(sc, &mut resp)?;

    let (r0, r1, r2) = (i32::from(resp[0]), i32::from(resp[1]), i32::from(resp[2]));
    let mut hw = wsmouse_get_hw(dev);
    hw.x_max = ((r0 & 0x0f) << 8) | r1;
    hw.y_max = ((r0 & 0xf0) << 4) | r2;

    let cap1 = i32::from(capabilities[1]);
    if cap1 < 2 || cap1 > hw.x_max {
        return Err(Errno::ENXIO);
    }

    if capabilities[0] & ELANTECH_CAP_TRACKPOINT != 0 {
        elantech
            .flags
            .set(elantech.flags.get() | ELANTECH_F_TRACKPOINT);
    }

    hw.type_ = WSMOUSE_TYPE_ELANTECH as i32;
    hw.hw_type = if elantech_is_clickpad(elantech) {
        WSMOUSEHW_CLICKPAD
    } else {
        WSMOUSEHW_TOUCHPAD
    };
    hw.mt_slots = ELANTECH_MAX_FINGERS;

    elantech.width.set(hw.x_max / (cap1 - 1));

    Ok(())
}

/// `elantech_ps2_cmd`: one byte to the touchpad.
pub fn elantech_ps2_cmd(sc: &PmsSoftc, command: u8) -> Result<(), Errno> {
    pms_cmd(sc, &[command], &mut [])
}

/// `elantech_knock`: disable, three scalings 1:1 and a status request, which an Elantech
/// touchpad answers with `0x3c, 0x03, 0xc8|0x00`.
pub fn elantech_knock(sc: &PmsSoftc) -> Result<(), Errno> {
    let mut resp = [0u8; 3];

    pms_dev_disable(sc)?;
    pms_set_scaling(sc, 1)?;
    pms_set_scaling(sc, 1)?;
    pms_set_scaling(sc, 1)?;
    pms_get_status(sc, &mut resp)?;
    if resp[0] != PMS_ELANTECH_MAGIC1
        || resp[1] != PMS_ELANTECH_MAGIC2
        || (resp[2] != PMS_ELANTECH_MAGIC3_1 && resp[2] != PMS_ELANTECH_MAGIC3_2)
    {
        return Err(Errno::ENXIO);
    }

    Ok(())
}

/// The `malloc` of an Elantech sub-softc in the `pms_enable_elantech_v*` functions: the
/// new block, written and published in `sc->elantech`; `None` (the C's "not enough
/// memory" message printed) when there is no memory.
fn elantech_alloc(sc: &PmsSoftc) -> Option<NonNull<u8>> {
    let Some(mem) = malloc(size_of::<ElantechSoftc>(), M_DEVBUF, M_WAITOK | M_ZERO) else {
        printf(format_args!(
            "{}: elantech: not enough memory\n",
            sc.devname()
        ));
        return None;
    };
    let p: NonNull<ElantechSoftc> = mem.cast();
    // SAFETY: a fresh block of `size_of::<ElantechSoftc>()` bytes, aligned by `malloc`,
    // that nothing else references yet.
    unsafe { p.as_ptr().write(ElantechSoftc::new()) };
    sc.elantech.set(Some(p));
    Some(mem)
}

/// The `free` of an Elantech sub-softc in the `pms_enable_elantech_v*` functions, when its
/// hardware query or the `wsmouse` configuration failed: `sc->elantech` is cleared first.
fn elantech_free(sc: &PmsSoftc, mem: NonNull<u8>) {
    sc.elantech.set(None);
    free(mem, M_DEVBUF, size_of::<ElantechSoftc>());
}

/// `wsmouse_configure(sc->sc_wsmousedev, NULL, 0)`, failing without a `wsmouse`.
fn elantech_configure(sc: &PmsSoftc) -> Result<(), Errno> {
    match sc.wsmousedev() {
        Some(dev) => wsmouse_configure(dev, None),
        None => Err(Errno::ENXIO),
    }
}

/// The parity table loop at the end of `pms_enable_elantech_v1`, as the C has it: it starts
/// with `parity[0] ^= 1`, so the first fill gives each byte its odd parity bit and every
/// later fill inverts the table.
pub fn elantech_fill_parity(elantech: &ElantechSoftc) {
    for i in 0..elantech.parity.len() {
        let v = elantech.parity[i & i.wrapping_sub(1)].get() ^ 1;
        elantech.parity[i].set(v);
    }
}

/// `pms_enable_elantech_v1`: knock, set a version 1 touchpad up the first time (or switch
/// it to absolute mode again), and fill the parity table.
pub fn pms_enable_elantech_v1(sc: &PmsSoftc) -> bool {
    let ok = 'err: {
        if elantech_knock(sc).is_err() {
            break 'err false;
        }

        if sc.elantech.get().is_none() {
            let Some(mem) = elantech_alloc(sc) else {
                break 'err false;
            };

            if elantech_get_hwinfo_v1(sc).is_err() {
                elantech_free(sc, mem);
                break 'err false;
            }
            if elantech_configure(sc).is_err() {
                elantech_free(sc, mem);
                printf(format_args!("{}: elantech: setup failed\n", sc.devname()));
                break 'err false;
            }

            printf(format_args!(
                "{}: Elantech Touchpad, version {}, firmware {:#x}\n",
                sc.devname(),
                1,
                sc.elantech().map_or(0, |e| e.fw_version.get())
            ));
        } else if elantech_set_absolute_mode_v1(sc).is_err() {
            break 'err false;
        }

        if let Some(elantech) = sc.elantech() {
            elantech_fill_parity(elantech);
        }

        true
    };

    if !ok {
        // err:
        let _ = pms_reset(sc);
    }
    ok
}

/// `pms_enable_elantech_v2`.
pub fn pms_enable_elantech_v2(sc: &PmsSoftc) -> bool {
    let ok = 'err: {
        if elantech_knock(sc).is_err() {
            break 'err false;
        }

        if sc.elantech.get().is_none() {
            let Some(mem) = elantech_alloc(sc) else {
                break 'err false;
            };

            if elantech_get_hwinfo_v2(sc).is_err() {
                elantech_free(sc, mem);
                break 'err false;
            }
            if elantech_configure(sc).is_err() {
                elantech_free(sc, mem);
                printf(format_args!("{}: elantech: setup failed\n", sc.devname()));
                break 'err false;
            }

            printf(format_args!(
                "{}: Elantech Touchpad, version {}, firmware {:#x}\n",
                sc.devname(),
                2,
                sc.elantech().map_or(0, |e| e.fw_version.get())
            ));
        } else if elantech_set_absolute_mode_v2(sc).is_err() {
            break 'err false;
        }

        true
    };

    if !ok {
        // err:
        let _ = pms_reset(sc);
    }
    ok
}

/// `pms_enable_elantech_v3`.
pub fn pms_enable_elantech_v3(sc: &PmsSoftc) -> bool {
    let ok = 'err: {
        if elantech_knock(sc).is_err() {
            break 'err false;
        }

        if sc.elantech.get().is_none() {
            let Some(mem) = elantech_alloc(sc) else {
                break 'err false;
            };

            if elantech_get_hwinfo_v3(sc).is_err() {
                elantech_free(sc, mem);
                break 'err false;
            }
            if elantech_configure(sc).is_err() {
                elantech_free(sc, mem);
                printf(format_args!("{}: elantech: setup failed\n", sc.devname()));
                break 'err false;
            }

            printf(format_args!(
                "{}: Elantech Touchpad, version {}, firmware {:#x}\n",
                sc.devname(),
                3,
                sc.elantech().map_or(0, |e| e.fw_version.get())
            ));
        } else if elantech_set_absolute_mode_v3(sc).is_err() {
            break 'err false;
        }

        true
    };

    if !ok {
        // err:
        let _ = pms_reset(sc);
    }
    ok
}

/// `pms_enable_elantech_v4`: as v1..v3, and the trackpoint's `wsmouse` when it has one.
pub fn pms_enable_elantech_v4(sc: &PmsSoftc) -> bool {
    let ok = 'err: {
        if elantech_knock(sc).is_err() {
            break 'err false;
        }

        if sc.elantech.get().is_none() {
            let Some(mem) = elantech_alloc(sc) else {
                break 'err false;
            };

            if elantech_get_hwinfo_v4(sc).is_err() {
                elantech_free(sc, mem);
                break 'err false;
            }
            if elantech_configure(sc).is_err() {
                elantech_free(sc, mem);
                printf(format_args!("{}: elantech: setup failed\n", sc.devname()));
                break 'err false;
            }

            let Some(elantech) = sc.elantech() else {
                break 'err false;
            };
            printf(format_args!(
                "{}: Elantech {}, version 4, firmware {:#x}\n",
                sc.devname(),
                if elantech_is_clickpad(elantech) {
                    "Clickpad"
                } else {
                    "Touchpad"
                },
                elantech.fw_version.get()
            ));

            if elantech.flags.get() & ELANTECH_F_TRACKPOINT != 0 {
                let mut a = WsmousedevAttachArgs {
                    accessops: &PMS_SEC_ACCESSOPS,
                    accesscookie: sc.cookie(),
                };
                sc.sc_sec_wsmousedev.set(config_found(
                    &sc.sc_dev,
                    ptr::from_mut(&mut a).cast(),
                    Some(wsmousedevprint),
                ));
            }
        } else if elantech_set_absolute_mode_v4(sc).is_err() {
            break 'err false;
        }

        true
    };

    if !ok {
        // err:
        let _ = pms_reset(sc);
    }
    ok
}

/// `pms_ioctl_elantech`: the type, the calibration and the mode of an Elantech touchpad.
pub fn pms_ioctl_elantech(
    sc: &PmsSoftc,
    cmd: u64,
    data: &mut [u8],
    _flag: i32,
    _p: Option<&Proc>,
) -> Result<bool, Errno> {
    match cmd {
        WSMOUSEIO_GTYPE => ioctl_ret(data, &WSMOUSE_TYPE_ELANTECH),
        WSMOUSEIO_GCALIBCOORDS => {
            let Some(dev) = sc.wsmousedev() else {
                return Err(Errno::ENXIO);
            };
            let hw = *wsmouse_get_hw(dev);
            let mut wsmc: WsmouseCalibcoords = ioctl_arg(data);
            wsmc.minx = hw.x_min;
            wsmc.maxx = hw.x_max;
            wsmc.miny = hw.y_min;
            wsmc.maxy = hw.y_max;
            wsmc.swapxy = 0;
            wsmc.resx = hw.h_res;
            wsmc.resy = hw.v_res;
            ioctl_ret(data, &wsmc);
        }
        WSMOUSEIO_SETMODE => {
            let wsmode = ioctl_arg::<u32>(data) as i32;
            if wsmode != WSMOUSE_COMPAT && wsmode != WSMOUSE_NATIVE {
                return Err(Errno::EINVAL);
            }
            if let Some(dev) = sc.wsmousedev() {
                let _ = wsmouse_set_mode(dev, wsmode);
            }
        }
        _ => return Ok(false),
    }
    Ok(true)
}

/// `pms_sync_elantech_v1`: the first byte carries the parity bits of the other three
/// (firmware 0x20022 is not checked: it sends inverted parity bits on cold boot, returning
/// to normal after suspend & resume).
pub fn pms_sync_elantech_v1(sc: &PmsSoftc, data: i32) -> bool {
    let Some(elantech) = sc.elantech() else {
        return false;
    };

    let p = match sc.inputstate.get() {
        0 => {
            if elantech.flags.get() & ELANTECH_F_HW_V1_OLD != 0 {
                elantech.p1.set(((data & 0x20) >> 5) as u8);
                elantech.p2.set(((data & 0x10) >> 4) as u8);
            } else {
                elantech.p1.set(((data & 0x10) >> 4) as u8);
                elantech.p2.set(((data & 0x20) >> 5) as u8);
            }
            elantech.p3.set(((data & 0x04) >> 2) as u8);
            return true;
        }
        1 => elantech.p1.get(),
        2 => elantech.p2.get(),
        3 => elantech.p3.get(),
        _ => return false,
    };

    let Some(parity) = usize::try_from(data)
        .ok()
        .and_then(|d| elantech.parity.get(d))
    else {
        return false;
    };
    // FW 0x20022 sends inverted parity bits on cold boot, returning to normal after
    // suspend & resume, so the parity check is disabled for this one.
    elantech.fw_version.get() == 0x20022 || parity.get() == p
}

/// `pms_sync_elantech_v2`: the constant bits of the first and the fourth byte (which, for
/// the variants without pressure, differ for two-finger packets).
pub fn pms_sync_elantech_v2(sc: &PmsSoftc, data: i32) -> bool {
    let Some(elantech) = sc.elantech() else {
        return false;
    };
    let state = sc.inputstate.get();

    // Variants reporting pressure always have the same constant bits.
    if elantech.flags.get() & ELANTECH_F_REPORTS_PRESSURE != 0 {
        if state == 0 && (data & 0x0c) != 0x04 {
            return false;
        }
        if state == 3 && (data & 0x0f) != 0x02 {
            return false;
        }
        return true;
    }

    // For variants not reporting pressure, 1 and 3 finger touch packets have different
    // constant bits than 2 finger touch packets.
    let two = elantech.flags.get() & ELANTECH_F_2FINGER_PACKET != 0;
    match state {
        0 => {
            if (data & 0xc0) == 0x80 {
                if (data & 0x0c) != 0x0c {
                    return false;
                }
                elantech
                    .flags
                    .set(elantech.flags.get() | ELANTECH_F_2FINGER_PACKET);
            } else {
                if (data & 0x3c) != 0x3c {
                    return false;
                }
                elantech
                    .flags
                    .set(elantech.flags.get() & !ELANTECH_F_2FINGER_PACKET);
            }
            true
        }
        1 | 4 => two || (data & 0xf0) == 0x00,
        3 => {
            if two {
                (data & 0x0e) == 0x08
            } else {
                (data & 0x3e) == 0x38
            }
        }
        _ => true,
    }
}

/// `pms_sync_elantech_v3`: the constant bits of the first and the fourth byte (another
/// set with the CRC format).
pub fn pms_sync_elantech_v3(sc: &PmsSoftc, data: i32) -> bool {
    let Some(elantech) = sc.elantech() else {
        return false;
    };
    let crc = elantech.flags.get() & ELANTECH_F_CRC_ENABLED != 0;

    match sc.inputstate.get() {
        0 => crc || (data & 0x0c) == 0x04 || (data & 0x0c) == 0x0c,
        3 => {
            if crc {
                (data & 0x09) == 0x08 || (data & 0x09) == 0x09
            } else {
                (data & 0xcf) == 0x02 || (data & 0xce) == 0x0c
            }
        }
        _ => true,
    }
}

/// `elantech_packet_type`: extract the type bits from `packet[3]`.
pub fn elantech_packet_type(elantech: &ElantechSoftc, b: u8) -> i32 {
    // This looks dubious, but in the "crc-enabled" format bit 2 may be set even in MOTION
    // packets.
    if elantech.flags.get() & ELANTECH_F_TRACKPOINT != 0 && (b & 0x0f) == 0x06 {
        ELANTECH_PKT_TRACKPOINT
    } else {
        i32::from(b & 0x03)
    }
}

/// `pms_sync_elantech_v4`: bit 3 of the first byte is clear; the fourth byte fits its
/// packet type (a trackpoint packet repeats bits of the first three).
pub fn pms_sync_elantech_v4(sc: &PmsSoftc, data: i32) -> bool {
    let Some(elantech) = sc.elantech() else {
        return false;
    };

    match sc.inputstate.get() {
        0 => (data & 0x08) == 0,
        3 => match elantech_packet_type(elantech, data as u8) {
            ELANTECH_V4_PKT_STATUS | ELANTECH_V4_PKT_HEAD | ELANTECH_V4_PKT_MOTION => {
                if elantech.flags.get() & ELANTECH_F_CRC_ENABLED != 0 {
                    (data & 0x08) == 0
                } else {
                    (data & 0x1c) == 0x10
                }
            }
            ELANTECH_PKT_TRACKPOINT => {
                let p = sc.packet.get();
                (p[0] & 0xc8) == 0
                    && i32::from(p[1]) == ((data & 0x10) << 3)
                    && i32::from(p[2]) == ((data & 0x20) << 2)
                    && (data ^ i32::from(p[0] & 0x30)) == 0x36
            }
            _ => false,
        },
        _ => true,
    }
}

/// `pms_proc_elantech_v1`: report a version 1 packet (one finger position, or the finger
/// count; no pressure).
pub fn pms_proc_elantech_v1(sc: &PmsSoftc) {
    let Some(elantech) = sc.elantech() else {
        return;
    };
    let p = sc.packet.get();

    let mut buttons = BUTMAP[usize::from(p[0] & 3)];

    if elantech.flags.get() & ELANTECH_F_HAS_ROCKER != 0 {
        if p[0] & 0x40 != 0 {
            // up
            buttons |= wsmouse_button(4);
        }
        if p[0] & 0x80 != 0 {
            // down
            buttons |= wsmouse_button(5);
        }
    }

    let w = if elantech.flags.get() & ELANTECH_F_HW_V1_OLD != 0 {
        i32::from((p[1] & 0x80) >> 7) + i32::from((p[1] & 0x30) >> 4)
    } else {
        i32::from((p[0] & 0xc0) >> 6)
    };

    // Firmwares 0x20022 and 0x20600 have a bug, position data in the first two reports for
    // single-touch contacts may be corrupt.
    let fw = elantech.fw_version.get();
    if fw == 0x20022 || fw == 0x20600 {
        if w == 1 {
            if elantech.initial_pkt.get() < 2 {
                elantech.initial_pkt.set(elantech.initial_pkt.get() + 1);
                return;
            }
        } else if elantech.initial_pkt.get() != 0 {
            elantech.initial_pkt.set(0);
        }
    }

    // Hardware version 1 doesn't report pressure.
    let (x, y, z) = if w != 0 {
        (
            (i32::from(p[1] & 0x0c) << 6) | i32::from(p[2]),
            (i32::from(p[1] & 0x03) << 8) | i32::from(p[3]),
            SYNAPTICS_PRESSURE,
        )
    } else {
        (0, 0, 0)
    };

    if let Some(dev) = sc.wsmousedev() {
        crate::WSMOUSE_TOUCH!(dev, buttons, x, y, z, w);
    }
}

/// `pms_proc_elantech_v2`: report a version 2 packet (one or three fingers: a position;
/// two: the first finger's position at a coarser scale).
pub fn pms_proc_elantech_v2(sc: &PmsSoftc) {
    const DEBOUNCE_PKT: [u8; 6] = [0x84, 0xff, 0xff, 0x02, 0xff, 0xff];
    let Some(elantech) = sc.elantech() else {
        return;
    };
    let p = sc.packet.get();

    // The hardware sends this packet when in debounce state. The packet should be ignored.
    if p[..DEBOUNCE_PKT.len()] == DEBOUNCE_PKT {
        return;
    }

    let buttons = BUTMAP[usize::from(p[0] & 3)];

    let w = i32::from((p[0] & 0xc0) >> 6);
    let (x, y, z) = if w == 1 || w == 3 {
        (
            (i32::from(p[1] & 0x0f) << 8) | i32::from(p[2]),
            (i32::from(p[4] & 0x0f) << 8) | i32::from(p[5]),
            if elantech.flags.get() & ELANTECH_F_REPORTS_PRESSURE != 0 {
                i32::from(p[1] & 0xf0) | i32::from((p[4] & 0xf0) >> 4)
            } else {
                SYNAPTICS_PRESSURE
            },
        )
    } else if w == 2 {
        (
            ((i32::from(p[0] & 0x10) << 4) | i32::from(p[1])) << 2,
            ((i32::from(p[0] & 0x20) << 3) | i32::from(p[2])) << 2,
            SYNAPTICS_PRESSURE,
        )
    } else {
        (0, 0, 0)
    };

    if let Some(dev) = sc.wsmousedev() {
        crate::WSMOUSE_TOUCH!(dev, buttons, x, y, z, w);
    }
}

/// `pms_proc_elantech_v3`: report a version 3 packet (a two-finger touch sends a head and
/// a tail packet; the tail is ignored), keeping the cursor still on garbage.
pub fn pms_proc_elantech_v3(sc: &PmsSoftc) {
    const DEBOUNCE_PKT: [u8; 6] = [0xc4, 0xff, 0xff, 0x02, 0xff, 0xff];
    let Some(elantech) = sc.elantech() else {
        return;
    };
    let p = sc.packet.get();

    let buttons = BUTMAP[usize::from(p[0] & 3)];

    let mut x = (i32::from(p[1] & 0x0f) << 8) | i32::from(p[2]);
    let mut y = (i32::from(p[4] & 0x0f) << 8) | i32::from(p[5]);
    let mut z = 0;
    let w = i32::from((p[0] & 0xc0) >> 6);
    if w == 2 {
        // Two-finger touch causes two packets -- a head packet and a tail packet. We report
        // a single event and ignore the tail packet.
        if elantech.flags.get() & ELANTECH_F_CRC_ENABLED != 0 {
            if (p[3] & 0x09) != 0x08 {
                return;
            }
        } else {
            // The hardware sends this packet when in debounce state. The packet should be
            // ignored.
            if p[..DEBOUNCE_PKT.len()] == DEBOUNCE_PKT {
                return;
            }
            if (p[0] & 0x0c) != 0x04 && (p[3] & 0xcf) != 0x02 {
                // not the head packet -- ignore
                return;
            }
        }
    }

    // Prevent jumping cursor if pad isn't touched or reports garbage.
    let (old_x, old_y) = (elantech.old_x.get(), elantech.old_y.get());
    if w == 0
        || ((x == 0 || y == 0 || x == elantech.max_x.get() || y == elantech.max_y.get())
            && (x != old_x || y != old_y))
    {
        x = old_x;
        y = old_y;
    }

    if elantech.flags.get() & ELANTECH_F_REPORTS_PRESSURE != 0 {
        z = i32::from(p[1] & 0xf0) | i32::from((p[4] & 0xf0) >> 4);
    } else if w != 0 {
        z = SYNAPTICS_PRESSURE;
    }

    if let Some(dev) = sc.wsmousedev() {
        crate::WSMOUSE_TOUCH!(dev, buttons, x, y, z, w);
    }
    elantech.old_x.set(x);
    elantech.old_y.set(y);
}

/// `pms_proc_elantech_v4`: report a version 4 packet: a status packet (the contacts
/// down), a head packet (a contact's position and pressure), a motion packet (two
/// contacts' relative motion) or a trackpoint packet.
pub fn pms_proc_elantech_v4(sc: &PmsSoftc) {
    let Some(elantech) = sc.elantech() else {
        return;
    };
    let Some(dev) = sc.wsmousedev() else {
        return;
    };
    let p = sc.packet.get();

    match elantech_packet_type(elantech, p[3]) {
        ELANTECH_V4_PKT_STATUS => {
            let mut slots = elantech.mt_slots.get();
            elantech.mt_slots.set(u32::from(p[1] & 0x1f));
            slots &= !elantech.mt_slots.get();
            let mut id = 0;
            while slots != 0 {
                if slots & 1 != 0 {
                    wsmouse_mtstate(dev, id, 0, 0, 0);
                }
                id += 1;
                slots >>= 1;
            }
        }

        ELANTECH_V4_PKT_HEAD => {
            let id = i32::from((p[3] & 0xe0) >> 5) - 1;
            if id > -1 && id < ELANTECH_MAX_FINGERS {
                let x = (i32::from(p[1] & 0x0f) << 8) | i32::from(p[2]);
                let y = (i32::from(p[4] & 0x0f) << 8) | i32::from(p[5]);
                let z = i32::from(p[1] & 0xf0) | i32::from((p[4] & 0xf0) >> 4);
                wsmouse_mtstate(dev, id, x, y, z);
            }
        }

        ELANTECH_V4_PKT_MOTION => {
            let weight = if p[0] & 0x10 != 0 {
                ELANTECH_V4_WEIGHT_VALUE
            } else {
                1
            };
            for n in [0, 3] {
                let id = i32::from((p[n] & 0xe0) >> 5) - 1;
                if !(0..ELANTECH_MAX_FINGERS).contains(&id) {
                    continue;
                }
                let x = weight * i32::from(p[n + 1] as i8);
                let y = weight * i32::from(p[n + 2] as i8);
                let z = WSMOUSE_DEFAULT_PRESSURE;
                wsmouse_set(dev, WSMOUSE_MT_REL_X, x, id);
                wsmouse_set(dev, WSMOUSE_MT_REL_Y, y, id);
                wsmouse_set(dev, WSMOUSE_MT_PRESSURE, z, id);
            }
        }

        ELANTECH_PKT_TRACKPOINT => {
            if sc.sc_dev_enable.get() & PMS_DEV_SECONDARY != 0 {
                // This firmware misreport coordinates for trackpoint occasionally. Discard
                // packets outside of [-127, 127] range to prevent cursor jumps.
                if p[4] == 0x80 || p[5] == 0x80 || p[1] >> 7 == p[4] >> 7 || p[2] >> 7 == p[5] >> 7
                {
                    return;
                }

                let x = i32::from(p[4]) - 0x100 + (i32::from(p[1]) << 1);
                let y = i32::from(p[5]) - 0x100 + (i32::from(p[2]) << 1);
                let buttons = BUTMAP[usize::from(p[0] & 7)];
                if let Some(sdev) = sc.sec_wsmousedev() {
                    crate::WSMOUSE_INPUT!(sdev, buttons, x, y, 0, 0);
                }
            }
            return;
        }

        _ => {
            printf(format_args!(
                "{}: unknown packet type {:#x}\n",
                sc.devname(),
                p[3] & 0x1f
            ));
            return;
        }
    }

    let buttons = BUTMAP[usize::from(p[0] & 3)];
    wsmouse_buttons(dev, buttons);

    wsmouse_input_sync(dev);
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use std::boxed::Box;

    use super::*;
    use crate::kern::kern_timeout::timeout_del;
    use crate::sys::timeout::timeout_pending;

    /// A softc as `config_make_softc` makes it (all zero), enabled, speaking `protocol`,
    /// with its reset timeout set up and no `wsmouse` (the procs then report nothing).
    fn softc(protocol: usize) -> &'static PmsSoftc {
        // SAFETY: `PmsSoftc` is valid all-zero (its `Softc` impl).
        let sc: &'static PmsSoftc = Box::leak(Box::new(unsafe { core::mem::zeroed::<PmsSoftc>() }));
        timeout_set(&sc.sc_rsttimo, pms_reset_timo, sc.cookie());
        sc.protocol.set(Some(&PMS_PROTOCOLS[protocol]));
        sc.sc_state.set(PMS_STATE_ENABLED);
        sc
    }

    /// The index of `type_` in the protocol table.
    fn proto(type_: i32) -> usize {
        PMS_PROTOCOLS.iter().position(|p| p.type_ == type_).unwrap()
    }

    fn feed(sc: &PmsSoftc, bytes: &[u8]) {
        for &b in bytes {
            pmsinput(sc.cookie(), i32::from(b));
        }
    }

    /// The timeout wheel, fresh, for the tests that add the reset timeout
    /// (`kern_timeout.rs`'s tests share it).
    fn timeouts() -> std::sync::MutexGuard<'static, ()> {
        let g = crate::kern::kern_timeout::tests::LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::kern::kern_timeout::timeout_startup();
        g
    }

    fn with_synaptics(sc: &PmsSoftc) -> &SynapticsSoftc {
        sc.synaptics.set(Some(NonNull::from(Box::leak(Box::new(
            SynapticsSoftc::new(),
        )))));
        sc.synaptics().unwrap()
    }

    fn with_alps(sc: &PmsSoftc) -> &AlpsSoftc {
        sc.alps
            .set(Some(NonNull::from(Box::leak(Box::new(AlpsSoftc::new())))));
        sc.alps().unwrap()
    }

    fn with_elantech(sc: &PmsSoftc) -> &ElantechSoftc {
        sc.elantech.set(Some(NonNull::from(Box::leak(Box::new(
            ElantechSoftc::new(),
        )))));
        sc.elantech().unwrap()
    }

    #[test]
    fn the_table_and_the_button_map() {
        let types: std::vec::Vec<i32> = PMS_PROTOCOLS.iter().map(|p| p.type_).collect();
        assert_eq!(types, [0, 2, 3, 4, 5, 6, 7, 1]);
        let sizes: std::vec::Vec<usize> = PMS_PROTOCOLS.iter().map(|p| p.packetsize).collect();
        assert_eq!(sizes, [3, 6, 6, 4, 6, 6, 6, 4]);
        assert!(PMS_PROTOCOLS[0].enable.is_none());
        assert!(PMS_PROTOCOLS[1..].iter().all(|p| p.enable.is_some()));
        assert_eq!(BUTMAP, [0, 1, 4, 5, 2, 3, 6, 7]);
        assert_eq!(ALPS_MODELS.len(), 19);
        assert_eq!(wsmouse_button(13), 1 << 12);
    }

    #[test]
    fn ps2_packets_decode() {
        // Left button, +5 right, -5 down (Y sign bit).
        assert_eq!(pms_ps2_motion(0x29, 0x05, 0xfb), (wsmouse_button(1), 5, -5));
        // Middle and right, X negative.
        assert_eq!(
            pms_ps2_motion(0x1e, 0xff, 0x01),
            (wsmouse_button(2) | wsmouse_button(3), -1, 1)
        );
    }

    #[test]
    fn standard_packets_sync_and_decode() {
        let sc = softc(proto(PMS_STANDARD));
        // A first byte with an overflow bit is out of sync and dropped.
        feed(sc, &[0x48]);
        assert_eq!(sc.inputstate.get(), 0);
        feed(sc, &[0x29, 0x05]);
        assert_eq!(sc.inputstate.get(), 2);
        feed(sc, &[0xfb]);
        assert_eq!(sc.inputstate.get(), 0);
        assert_eq!(pms_mouse_decode(sc), (wsmouse_button(1), 5, -5, 0));

        // Disabled: every byte is discarded.
        sc.sc_state.set(PMS_STATE_DISABLED);
        feed(sc, &[0x08, 0x01]);
        assert_eq!(sc.inputstate.get(), 0);
        assert_eq!(sc.packet.get()[0], 0x29);
    }

    #[test]
    fn intellimouse_packets_carry_the_wheel() {
        let sc = softc(proto(PMS_INTELLI));
        // Bit 3 of the first byte must be set.
        feed(sc, &[0x02]);
        assert_eq!(sc.inputstate.get(), 0);
        feed(sc, &[0x0a, 0x10, 0x00, 0xff]);
        assert_eq!(sc.inputstate.get(), 0);
        assert_eq!(pms_mouse_decode(sc), (wsmouse_button(3), 16, 0, -1));
        feed(sc, &[0x08, 0x00, 0x00, 0x02]);
        assert_eq!(pms_mouse_decode(sc), (0, 0, 0, 2));
    }

    #[test]
    fn reset_announcement() {
        let _t = timeouts();
        let sc = softc(proto(PMS_STANDARD));
        pms_reset_detect(sc, 0x12);
        assert_eq!(sc.sc_rststate.get(), 0);
        pms_reset_detect(sc, i32::from(PMS_RSTDONE));
        assert_eq!(sc.sc_rststate.get(), PMS_RST_COMMENCE);
        // Another 0xaa keeps it commencing; anything but 0 or 0xaa ends it.
        pms_reset_detect(sc, i32::from(PMS_RSTDONE));
        assert_eq!(sc.sc_rststate.get(), PMS_RST_COMMENCE);
        pms_reset_detect(sc, 0x01);
        assert_eq!(sc.sc_rststate.get(), 0);
        assert!(!timeout_pending(&sc.sc_rsttimo));
        // 0xaa 0x00 is the announcement: the timeout runs in 100 ms.
        pms_reset_detect(sc, i32::from(PMS_RSTDONE));
        pms_reset_detect(sc, 0x00);
        assert_eq!(sc.sc_rststate.get(), PMS_RST_ANNOUNCED);
        assert!(timeout_pending(&sc.sc_rsttimo));
        timeout_del(&sc.sc_rsttimo);
        // The next byte starts over.
        pms_reset_detect(sc, 0x08);
        assert_eq!(sc.sc_rststate.get(), 0);
    }

    #[test]
    fn reset_announcement_through_the_input() {
        let _t = timeouts();
        let sc = softc(proto(PMS_STANDARD));
        // The announcement is watched for whatever the packet does with the bytes.
        feed(sc, &[0xaa, 0x00]);
        assert_eq!(sc.sc_rststate.get(), PMS_RST_ANNOUNCED);
        assert!(timeout_pending(&sc.sc_rsttimo));
        timeout_del(&sc.sc_rsttimo);
    }

    #[test]
    fn state_changes_count_the_open_wsmouses() {
        let sc = softc(proto(PMS_STANDARD));
        // Enabled already: the primary's open only records itself; twice is EBUSY.
        assert_eq!(
            pms_change_state(sc, PMS_STATE_ENABLED, PMS_DEV_PRIMARY),
            Ok(())
        );
        assert_eq!(
            pms_change_state(sc, PMS_STATE_ENABLED, PMS_DEV_PRIMARY),
            Err(Errno::EBUSY)
        );
        assert_eq!(
            pms_change_state(sc, PMS_STATE_ENABLED, PMS_DEV_SECONDARY),
            Ok(())
        );
        // The secondary closes while the primary is open: still enabled.
        assert_eq!(
            pms_change_state(sc, PMS_STATE_DISABLED, PMS_DEV_SECONDARY),
            Ok(())
        );
        assert_eq!(sc.sc_dev_enable.get(), PMS_DEV_PRIMARY);
        assert_eq!(sc.sc_state.get(), PMS_STATE_ENABLED);
    }

    #[test]
    fn synaptics_sync() {
        let sc = softc(proto(PMS_SYNAPTICS));
        let syn = with_synaptics(sc);
        syn.mask.set(SYNAPTICS_MASK_NEWABS_STRICT);
        assert!(pms_sync_synaptics(sc, 0x80));
        assert!(pms_sync_synaptics(sc, 0xb7));
        assert!(!pms_sync_synaptics(sc, 0x88));
        sc.inputstate.set(3);
        assert!(pms_sync_synaptics(sc, 0xc0));
        assert!(!pms_sync_synaptics(sc, 0x80));
        sc.inputstate.set(2);
        assert!(pms_sync_synaptics(sc, 0xff));
        // The relaxed mask of firmware 5.9 lets bit 3 through.
        syn.mask.set(SYNAPTICS_MASK_NEWABS_RELAXED);
        sc.inputstate.set(0);
        assert!(pms_sync_synaptics(sc, 0x88));
        // A full packet goes through; with no wsmouse the proc reports nothing.
        feed(sc, &[0x80, 0x12, 0x30, 0xc0, 0x34, 0x56]);
        assert_eq!(sc.inputstate.get(), 0);
        assert_eq!(sc.packet.get()[..6], [0x80, 0x12, 0x30, 0xc0, 0x34, 0x56]);
    }

    #[test]
    fn alps_sync() {
        let sc = softc(proto(PMS_ALPS));
        let alps = with_alps(sc);
        alps.model.set(ALPS_GLIDEPOINT);
        alps.mask.set(0xf8);
        assert!(pms_sync_alps(sc, 0xff));
        assert!(!pms_sync_alps(sc, 0x7f));
        for state in 1..=5 {
            sc.inputstate.set(state);
            assert!(pms_sync_alps(sc, 0x7f));
            assert!(!pms_sync_alps(sc, 0x80));
        }
        // An interleaved model accepts bit 7 in bytes 4 and 5.
        alps.model.set(ALPS_DUALPOINT | ALPS_INTERLEAVED);
        sc.inputstate.set(4);
        assert!(pms_sync_alps(sc, 0x80));

        // A Dualpoint's PS/2 packet: three bytes, then the sync jumps to the end.
        sc.packet.set([0x08, 0, 0, 0, 0, 0, 0, 0]);
        sc.inputstate.set(2);
        assert!(pms_sync_alps(sc, 0x80));
        assert_eq!(sc.inputstate.get(), 5);
    }

    #[test]
    fn alps_trackpoint_packets() {
        let sc = softc(proto(PMS_ALPS));
        let alps = with_alps(sc);
        alps.model.set(ALPS_DUALPOINT | ALPS_INTERLEAVED);
        // A PS/2 packet keeps its buttons.
        sc.packet.set([0x09, 0x05, 0x02, 0, 0, 0, 0, 0]);
        assert!(alps_sec_proc(sc));
        assert_eq!(alps.sec_buttons.get(), wsmouse_button(1));
        // An interleaved one is in the last three bytes, and the next packet starts at 3.
        sc.packet.set([0xff, 0, 0, 0x0f, 0x01, 0x01, 0, 0]);
        assert!(alps_sec_proc(sc));
        assert_eq!(sc.inputstate.get(), 3);
        // A touchpad packet is not the trackpoint's.
        sc.packet.set([0xff, 0, 0, 0x00, 0x01, 0x01, 0, 0]);
        assert!(!alps_sec_proc(sc));
    }

    #[test]
    fn elantech_parity_table() {
        let e = ElantechSoftc::new();
        elantech_fill_parity(&e);
        // The first fill: each byte's odd parity bit (1 when the byte has an even number
        // of bits set).
        for i in 0..256usize {
            assert_eq!(
                u32::from(e.parity[i].get()),
                (i.count_ones() + 1) % 2,
                "{i:#x}"
            );
        }
        // Every later fill inverts the table, as the C's loop does (it starts with
        // parity[0] ^= 1).
        elantech_fill_parity(&e);
        for i in 0..256usize {
            assert_eq!(u32::from(e.parity[i].get()), i.count_ones() % 2, "{i:#x}");
        }
    }

    #[test]
    fn elantech_v1_sync_checks_parity() {
        let sc = softc(proto(PMS_ELANTECH_V1));
        let e = with_elantech(sc);
        elantech_fill_parity(e);
        // p1 = bit 4, p2 = bit 5, p3 = bit 2 of the first byte.
        assert!(pms_sync_elantech_v1(sc, 0x14));
        assert_eq!((e.p1.get(), e.p2.get(), e.p3.get()), (1, 0, 1));
        sc.inputstate.set(1);
        assert!(pms_sync_elantech_v1(sc, 0x00)); // parity[0] is 1
        assert!(!pms_sync_elantech_v1(sc, 0x01));
        sc.inputstate.set(2);
        assert!(pms_sync_elantech_v1(sc, 0x01)); // parity[1] is 0
        sc.inputstate.set(4);
        assert!(!pms_sync_elantech_v1(sc, 0x00));
        // Firmware 0x20022 is not checked.
        e.fw_version.set(0x20022);
        sc.inputstate.set(1);
        assert!(pms_sync_elantech_v1(sc, 0x01));
        // The old hardware swaps p1 and p2.
        e.flags.set(ELANTECH_F_HW_V1_OLD);
        sc.inputstate.set(0);
        assert!(pms_sync_elantech_v1(sc, 0x10));
        assert_eq!((e.p1.get(), e.p2.get()), (0, 1));
    }

    #[test]
    fn elantech_v2_v3_sync() {
        let sc = softc(proto(PMS_ELANTECH_V2));
        let e = with_elantech(sc);
        e.flags.set(ELANTECH_F_REPORTS_PRESSURE);
        assert!(pms_sync_elantech_v2(sc, 0x04));
        assert!(!pms_sync_elantech_v2(sc, 0x0c));
        sc.inputstate.set(3);
        assert!(pms_sync_elantech_v2(sc, 0x12));
        assert!(!pms_sync_elantech_v2(sc, 0x13));
        // Without pressure: a two-finger first byte sets the flag, a one-finger one
        // clears it.
        e.flags.set(0);
        sc.inputstate.set(0);
        assert!(pms_sync_elantech_v2(sc, 0x8c));
        assert_ne!(e.flags.get() & ELANTECH_F_2FINGER_PACKET, 0);
        sc.inputstate.set(1);
        assert!(pms_sync_elantech_v2(sc, 0xf0));
        sc.inputstate.set(3);
        assert!(pms_sync_elantech_v2(sc, 0x08));
        sc.inputstate.set(0);
        assert!(pms_sync_elantech_v2(sc, 0x3c));
        assert_eq!(e.flags.get() & ELANTECH_F_2FINGER_PACKET, 0);
        sc.inputstate.set(1);
        assert!(!pms_sync_elantech_v2(sc, 0xf0));
        sc.inputstate.set(3);
        assert!(pms_sync_elantech_v2(sc, 0x38));

        e.flags.set(0);
        sc.inputstate.set(0);
        assert!(pms_sync_elantech_v3(sc, 0x04));
        assert!(pms_sync_elantech_v3(sc, 0x0c));
        assert!(!pms_sync_elantech_v3(sc, 0x08));
        sc.inputstate.set(3);
        assert!(pms_sync_elantech_v3(sc, 0x02));
        assert!(pms_sync_elantech_v3(sc, 0x0c));
        assert!(!pms_sync_elantech_v3(sc, 0x08));
        e.flags.set(ELANTECH_F_CRC_ENABLED);
        assert!(pms_sync_elantech_v3(sc, 0x08));
        assert!(!pms_sync_elantech_v3(sc, 0x01));
        sc.inputstate.set(0);
        assert!(pms_sync_elantech_v3(sc, 0x00));
    }

    #[test]
    fn elantech_v4_packet_types_and_sync() {
        let sc = softc(proto(PMS_ELANTECH_V4));
        let e = with_elantech(sc);
        assert_eq!(elantech_packet_type(e, 0x10), ELANTECH_V4_PKT_STATUS);
        assert_eq!(elantech_packet_type(e, 0x11), ELANTECH_V4_PKT_HEAD);
        assert_eq!(elantech_packet_type(e, 0x12), ELANTECH_V4_PKT_MOTION);
        assert_eq!(elantech_packet_type(e, 0x16), ELANTECH_V4_PKT_MOTION);
        e.flags.set(ELANTECH_F_TRACKPOINT);
        assert_eq!(elantech_packet_type(e, 0x16), ELANTECH_PKT_TRACKPOINT);

        assert!(pms_sync_elantech_v4(sc, 0x04));
        assert!(!pms_sync_elantech_v4(sc, 0x08));
        sc.inputstate.set(3);
        assert!(pms_sync_elantech_v4(sc, 0x11));
        assert!(!pms_sync_elantech_v4(sc, 0x19));
        assert!(!pms_sync_elantech_v4(sc, 0x03));
        // A trackpoint packet repeats the sign bits of bytes 1 and 2 in byte 3.
        sc.packet.set([0x00, 0x80, 0x80, 0, 0, 0, 0, 0]);
        assert!(pms_sync_elantech_v4(sc, 0x36));
        sc.packet.set([0x00, 0x00, 0x00, 0, 0, 0, 0, 0]);
        assert!(!pms_sync_elantech_v4(sc, 0x36));
        e.flags.set(ELANTECH_F_CRC_ENABLED);
        assert!(pms_sync_elantech_v4(sc, 0x01));
        sc.inputstate.set(2);
        assert!(pms_sync_elantech_v4(sc, 0xff));
    }

    #[test]
    fn ioctls_without_hardware() {
        let sc = softc(proto(PMS_STANDARD));
        let mut data = [0u8; 4];
        assert_eq!(
            pms_ioctl(sc.cookie(), WSMOUSEIO_GTYPE, &mut data, 0, None),
            Ok(true)
        );
        assert_eq!(u32::from_ne_bytes(data), WSMOUSE_TYPE_PS2);
        assert_eq!(
            pms_ioctl(sc.cookie(), WSMOUSEIO_SETMODE, &mut data, 0, None),
            Ok(false)
        );
        assert_eq!(
            pms_sec_ioctl(sc.cookie(), WSMOUSEIO_GTYPE, &mut data, 0, None),
            Ok(true)
        );
        let mut data = [0u8; 4];
        assert_eq!(
            pms_ioctl_alps(sc, WSMOUSEIO_GTYPE, &mut data, 0, None),
            Ok(true)
        );
        assert_eq!(u32::from_ne_bytes(data), WSMOUSE_TYPE_ALPS);
        let mut data = 7u32.to_ne_bytes();
        assert_eq!(
            pms_ioctl_elantech(sc, WSMOUSEIO_SETMODE, &mut data, 0, None),
            Err(Errno::EINVAL)
        );
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn the_drivers_own_defines_match_the_c() {
        let defs = crate::reftest::defines("sys/dev/pckbc/pms.c");
        let ours = crate::reftest::assert_defines!(defs;
            PMS_STANDARD, PMS_INTELLI, PMS_SYNAPTICS, PMS_ALPS, PMS_ELANTECH_V1,
            PMS_ELANTECH_V2, PMS_ELANTECH_V3, PMS_ELANTECH_V4, SYNAPTICS_MASK_NEWABS_STRICT,
            SYNAPTICS_MASK_NEWABS_RELAXED, SYNAPTICS_VALID_NEWABS_FIRST,
            SYNAPTICS_VALID_NEWABS_NEXT, SYNAPTICS_PRESSURE_HI, SYNAPTICS_PRESSURE_LO,
            SYNAPTICS_PRESSURE, SYNAPTICS_SCALE, SYNAPTICS_MAX_FINGERS, ALPS_GLIDEPOINT,
            ALPS_DUALPOINT, ALPS_PASSTHROUGH, ALPS_INTERLEAVED, ALPS_PRESSURE,
            ELANTECH_F_REPORTS_PRESSURE, ELANTECH_F_HAS_ROCKER, ELANTECH_F_2FINGER_PACKET,
            ELANTECH_F_HW_V1_OLD, ELANTECH_F_CRC_ENABLED, ELANTECH_F_TRACKPOINT,
            PMS_STATE_DISABLED, PMS_STATE_ENABLED, PMS_STATE_SUSPENDED, PMS_DEV_IGNORE,
            PMS_DEV_PRIMARY, PMS_DEV_SECONDARY, PMS_RST_COMMENCE, PMS_RST_ANNOUNCED,
        );
        for prefix in ["PMS_", "SYNAPTICS_", "ALPS_", "ELANTECH_"] {
            crate::reftest::assert_complete(&defs, prefix, &ours);
        }
    }

    /// `alps_models[]` against the C, entry by entry (the `#if 0` ones left out).
    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn alps_models_match_the_c() {
        let path = crate::reftest::openbsd_src().join("sys/dev/pckbc/pms.c");
        let text = std::fs::read_to_string(path).unwrap();
        let start = text.find("alps_models[] = {").unwrap();
        let body = &text[start..];
        let body = &body[..body.find("#if 0").unwrap()];
        let model = |s: &str| -> i32 {
            s.split('|')
                .map(|t| match t.trim() {
                    "ALPS_DUALPOINT" => ALPS_DUALPOINT,
                    "ALPS_PASSTHROUGH" => ALPS_PASSTHROUGH,
                    "ALPS_INTERLEAVED" => ALPS_INTERLEAVED,
                    "ALPS_GLIDEPOINT" => ALPS_GLIDEPOINT,
                    t => panic!("{t}"),
                })
                .fold(0, |a, b| a | b)
        };
        let mut c = std::vec::Vec::new();
        for line in body
            .lines()
            .map(str::trim)
            .filter(|l| l.starts_with("{ 0x"))
        {
            let inner = &line[1..line.find('}').unwrap()];
            let f: std::vec::Vec<&str> = inner.split(',').map(str::trim).collect();
            let hex = |s: &str| i32::from_str_radix(s.trim_start_matches("0x"), 16).unwrap();
            c.push((hex(f[0]), hex(f[1]), model(f[2])));
        }
        let ours: std::vec::Vec<(i32, i32, i32)> = ALPS_MODELS
            .iter()
            .map(|m| (m.version, m.mask, m.model))
            .collect();
        assert_eq!(ours, c);
    }
}
/* </TESTS> */
