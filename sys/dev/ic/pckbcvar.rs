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
 * Copyright (c) 1998
 *	Matthias Drochner.  All rights reserved.
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
 *
 */
/* </LICENSES> */

/* <CODE> */
//! The interface of `pckbc(4)`, the PC keyboard controller, to its bus attachments and to the
//! devices on its slots (`pckbd(4)`, `pms(4)`): `<dev/ic/pckbcvar.h>`.
//!
//! Upstream: sys/dev/ic/pckbcvar.h @ 3ce1f3f79392
//!
//! The controller's state ([`PckbcInternal`], the C's `pckbc_tag_t`) lives apart from the
//! device ([`PckbcSoftc`]) because the console keyboard needs it before autoconfiguration
//! (`pckbc_cnattach` fills the static `pckbc_consdata`). A slot device gets the tag and its
//! slot in a [`PckbcAttachArgs`] and talks to its device through the functions of
//! `pckbc.rs`; its input reaches it through the handler it registers with
//! `pckbc_set_inputhandler`.
//!
//! ## Deviations
//! - `pckbc_tag_t` (`void *`) is [`PckbcTag`], a `&'static PckbcInternal`: the internal
//!   state is the static `PCKBC_CONSDATA` or a `malloc`ed block that is never freed, as in
//!   the C.
//! - The bus space tag and handles are `Option`s, `None` until the bus attachment maps the
//!   ports; `t_slotdata` and `t_sc` are `Option<NonNull<_>>`, the C's possibly NULL
//!   pointers. Every field is a `Cell`: the controller is touched at `spltty()` under the
//!   kernel lock, from process context and from its interrupt handler.
//! - The `extern` declarations and prototypes are `pckbc.c`'s, in `pckbc.rs`.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::NonNull;

use crate::dev::ic::pckbc::PckbcSlotdata;
use crate::machine::bus::{BusAddr, BusSpaceHandle, BusSpaceTag};
use crate::sys::device::{Device, Softc};
use crate::sys::timeout::Timeout;

/// `PCKBCCF_SLOT`: the index of the `slot` locator (`define pckbcslot {[slot = -1]}`).
pub const PCKBCCF_SLOT: usize = 0;
/// `PCKBCCF_SLOT_DEFAULT`: the `slot` locator left open.
pub const PCKBCCF_SLOT_DEFAULT: i64 = -1;

/// `PCKBC_KBD_SLOT`: the keyboard port.
pub const PCKBC_KBD_SLOT: PckbcSlotT = 0;
/// `PCKBC_AUX_SLOT`: the auxiliary (mouse) port.
pub const PCKBC_AUX_SLOT: PckbcSlotT = 1;
/// `PCKBC_NSLOTS`.
pub const PCKBC_NSLOTS: usize = 2;

/// `PCKBC_NEED_AUXWRITE`: need auxwrite command to find aux.
pub const PCKBC_NEED_AUXWRITE: i32 = 0x0001;
/// `PCKBC_FIXED_SET2`: can't translate to XT scancodes, stuck to set #2.
pub const PCKBC_FIXED_SET2: i32 = 0x0002;
/// `PCKBC_FIXED_SET3`: can't translate to XT scancodes, stuck to set #3.
pub const PCKBC_FIXED_SET3: i32 = 0x0004;
/// `PCKBC_CANT_TRANSLATE`.
pub const PCKBC_CANT_TRANSLATE: i32 = PCKBC_FIXED_SET2 | PCKBC_FIXED_SET3;

/// `PCKBCF_FORCE_KEYBOARD_PRESENT`: device configuration flag (`cf_flags`): attach the
/// keyboard even when it does not answer.
pub const PCKBCF_FORCE_KEYBOARD_PRESENT: i32 = 0x0001;

/// `pckbc_slot_t`.
pub type PckbcSlotT = i32;

/// `pckbc_tag_t`: the controller, as the slot devices see it.
pub type PckbcTag = &'static PckbcInternal;

/// `pckbc_inputfcn`: a slot device's input handler, called with its argument and the byte.
pub type PckbcInputfcn = fn(arg: *mut c_void, data: i32);

/// `struct pckbc_internal`: external representation (`pckbc_tag_t`), needed early for
/// console operation.
pub struct PckbcInternal {
    /// `t_iot`.
    pub t_iot: Cell<Option<BusSpaceTag>>,
    /// `t_ioh_d`: data port.
    pub t_ioh_d: Cell<Option<BusSpaceHandle>>,
    /// `t_ioh_c`: cmd port.
    pub t_ioh_c: Cell<Option<BusSpaceHandle>>,
    /// `t_addr`.
    pub t_addr: Cell<BusAddr>,
    /// `t_cmdbyte`: shadow of the controller's command byte.
    pub t_cmdbyte: Cell<u8>,
    /// `t_flags`: `PCKBC_NEED_AUXWRITE`, `PCKBC_FIXED_SET2`, `PCKBC_FIXED_SET3`.
    pub t_flags: Cell<i32>,
    /// `t_haveaux`: controller has an aux port.
    pub t_haveaux: Cell<i32>,
    /// `t_slotdata`: the command queues of each slot, once it has a device.
    pub t_slotdata: [Cell<Option<NonNull<PckbcSlotdata>>>; PCKBC_NSLOTS],
    /// `t_sc`: back pointer.
    pub t_sc: Cell<Option<NonNull<PckbcSoftc>>>,
    /// `t_cleanup`: the command timeout.
    pub t_cleanup: Timeout,
    /// `t_poll`: the keyboard's polling fallback.
    pub t_poll: Timeout,
}

impl PckbcInternal {
    /// A controller nobody has mapped yet (`bzero`ed, like the static `pckbc_consdata`).
    pub const fn new() -> Self {
        Self {
            t_iot: Cell::new(None),
            t_ioh_d: Cell::new(None),
            t_ioh_c: Cell::new(None),
            t_addr: Cell::new(0),
            t_cmdbyte: Cell::new(0),
            t_flags: Cell::new(0),
            t_haveaux: Cell::new(0),
            t_slotdata: [const { Cell::new(None) }; PCKBC_NSLOTS],
            t_sc: Cell::new(None),
            t_cleanup: Timeout::zeroed(),
            t_poll: Timeout::zeroed(),
        }
    }

    /// The tag and the data and command ports, once mapped.
    pub fn regs(&self) -> Option<(BusSpaceTag, BusSpaceHandle, BusSpaceHandle)> {
        Some((self.t_iot.get()?, self.t_ioh_d.get()?, self.t_ioh_c.get()?))
    }

    /// `t->t_slotdata[slot]`.
    pub fn slotdata(&self, slot: PckbcSlotT) -> Option<&PckbcSlotdata> {
        let p = self.t_slotdata.get(slot as usize)?.get()?;
        // SAFETY: a slot's data is the static `pckbc_cons_slotdata` or a `malloc`ed block
        // that is never freed (`pckbc_attach_slot`).
        Some(unsafe { p.as_ref() })
    }

    /// `t->t_sc`.
    pub fn sc(&self) -> Option<&PckbcSoftc> {
        // SAFETY: `t_sc` is the attached controller's softc, which is never detached.
        self.t_sc.get().map(|p| unsafe { p.as_ref() })
    }
}

impl Default for PckbcInternal {
    fn default() -> Self {
        Self::new()
    }
}

// SAFETY: the controller's state is touched at `spltty()` under the kernel lock (its
// interrupt handler is not `MPSAFE`), so no two CPUs reach the cells at once.
unsafe impl Sync for PckbcInternal {}

/// `struct pckbc_softc`: state per device.
#[repr(C)]
pub struct PckbcSoftc {
    /// `sc_dv`.
    pub sc_dv: Device,
    /// `id`: the controller.
    pub id: Cell<Option<PckbcTag>>,
    /// `inputhandler`: each slot device's input handler.
    pub inputhandler: [Cell<Option<PckbcInputfcn>>; PCKBC_NSLOTS],
    /// `inputarg`: their arguments.
    pub inputarg: [Cell<*mut c_void>; PCKBC_NSLOTS],
    /// `subname`: their device names.
    pub subname: [Cell<Option<&'static str>>; PCKBC_NSLOTS],
}

// SAFETY: `#[repr(C)]` with the device first; all-zero is `None`s and null pointers.
unsafe impl Softc for PckbcSoftc {}

/// `struct pckbc_attach_args`: what a slot device's match and attach get.
pub struct PckbcAttachArgs {
    /// `pa_tag`.
    pub pa_tag: PckbcTag,
    /// `pa_slot`.
    pub pa_slot: PckbcSlotT,
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_controller_has_no_ports_and_no_slots() {
        let t = PckbcInternal::new();
        assert!(t.regs().is_none());
        assert!(t.slotdata(PCKBC_KBD_SLOT).is_none());
        assert!(t.slotdata(PCKBC_AUX_SLOT).is_none());
        assert!(t.slotdata(2).is_none());
        assert!(t.sc().is_none());
        assert_eq!(PCKBC_CANT_TRANSLATE, 6);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/ic/pckbcvar.h");
        let ours: &[(&str, i64)] = &[
            ("PCKBCCF_SLOT", PCKBCCF_SLOT as i64),
            ("PCKBCCF_SLOT_DEFAULT", PCKBCCF_SLOT_DEFAULT),
            ("PCKBC_KBD_SLOT", PCKBC_KBD_SLOT.into()),
            ("PCKBC_AUX_SLOT", PCKBC_AUX_SLOT.into()),
            ("PCKBC_NSLOTS", PCKBC_NSLOTS as i64),
            ("PCKBC_NEED_AUXWRITE", PCKBC_NEED_AUXWRITE.into()),
            ("PCKBC_FIXED_SET2", PCKBC_FIXED_SET2.into()),
            ("PCKBC_FIXED_SET3", PCKBC_FIXED_SET3.into()),
            (
                "PCKBCF_FORCE_KEYBOARD_PRESENT",
                PCKBCF_FORCE_KEYBOARD_PRESENT.into(),
            ),
        ];
        for (name, value) in ours {
            assert_eq!(crate::reftest::int(&defs, name), Some(*value), "{name}");
        }
    }
}
/* </TESTS> */
