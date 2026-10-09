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
 */
/* </LICENSES> */

/* <CODE> */
//! `pckbc(4)`: the PC keyboard controller (i8042) and the command queues of the devices on its
//! keyboard and auxiliary slots: `dev/ic/pckbc.c`.
//!
//! Upstream: sys/dev/ic/pckbc.c @ 3ce1f3f79392
//!
//! A bus attachment (`pckbc_isa.rs`) maps the data and command ports and calls
//! [`pckbc_attach`], which writes the command byte, attaches a device to the keyboard slot,
//! probes the auxiliary port with an echo and attaches a device there, then enables the
//! interrupts of the slots that have one. The slot devices (`pckbd(4)`, `pms(4)`) send
//! commands either by polling ([`pckbc_poll_cmd`], during autoconfiguration) or through the
//! slot's queue ([`pckbc_enqueue_cmd`]), whose responses [`pckbcintr`] matches before it
//! hands the other bytes to the slot's input handler.
//!
//! The console keyboard path, [`pckbc_cnattach`], is ported but nothing calls it: amd64's
//! `wscons_machdep.c` (the glass console's `wscn_input_init`), which would, is not ported, and
//! the kernel's console is the serial port; so `pckbc_console` stays 0.
//!
//! ## Deviations
//! - `pckbc_tag_t` is `&'static PckbcInternal` (`pckbcvar.rs`). The command descriptors
//!   are `Cell`s on `queue.rs` tail queues; a slot's queues live in its `PckbcSlotdata`,
//!   which is never freed (as in the C).
//! - `pckbc_send_cmd` and `pckbc_xt_translation` return `bool` (the C's 1 and 0, and 0 and
//!   -1, as success and failure); `pckbc_poll_data1` and `pckbc_poll_data` return
//!   `Option<u8>`, `None` where the C returns -1; `pckbc_poll_cmd` and
//!   `pckbc_enqueue_cmd` take the command as a slice and the response buffer as an
//!   `Option<&mut [u8]>`, and return `Result`; a descriptor's `status` is an
//!   `Option<Errno>`.
//! - The `#if defined(__i386__) || defined(__amd64__)` parts (the legacy-free PC's console
//!   release) are chosen by the machine's `MACHINE_PC` constant, not by `cfg(target_arch)`.
//!   The `#ifdef __sparc64__` part of `pckbc_xt_translation` (pckbc at ebus) is not
//!   compiled, as on amd64 and arm64.
//! - `pckbc_release_console`'s call of `wscn_input_init(1)` (`wscons_machdep.c`, not ported)
//!   is a visible `unported!`; it is unreachable while `pckbc_console` is 0 (see above).
//! - The `PCKBCDEBUG` messages (`DPRINTF` and the `#ifdef PCKBCDEBUG` lines) are not
//!   compiled, as in GENERIC; the `#if 0` keyboard port test of `pckbc_attach` neither.
//! - A slot without data where the C dereferences NULL (`pckbc_set_poll`,
//!   `pckbc_enqueue_cmd`) returns early; `pckbc_cmdresponse` without an active command
//!   panics under `diagnostic`, as the C, and otherwise returns 0.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI32, Ordering};

use crate::dev::ic::i8042reg::{
    K_LDCMDBYTE, K_RDCMDBYTE, KBC_AUXDISABLE, KBC_AUXECHO, KBC_AUXENABLE, KBC_AUXWRITE,
    KBC_KBDDISABLE, KBC_KBDENABLE, KBC_SELFTEST, KBDATAP, KBS_AUXDATA, KBS_DIB, KBS_IBF, KC8_CPU,
    KC8_KENABLE, KC8_MENABLE, KC8_TRANS,
};
use crate::dev::ic::pckbcvar::{
    PCKBC_AUX_SLOT, PCKBC_KBD_SLOT, PCKBC_NEED_AUXWRITE, PCKBC_NSLOTS, PCKBCCF_SLOT,
    PCKBCCF_SLOT_DEFAULT, PCKBCF_FORCE_KEYBOARD_PRESENT, PckbcAttachArgs, PckbcInputfcn,
    PckbcInternal, PckbcSlotT, PckbcSoftc, PckbcTag,
};
use crate::dev::pckbc::pckbdvar::pckbd_cnattach;
use crate::kern::kern_malloc::malloc;
use crate::kern::kern_synch::{tsleep_nsec, wakeup};
use crate::kern::kern_timeout::{timeout_add_sec, timeout_del, timeout_set};
use crate::kern::subr_autoconf::config_found_sm;
use crate::kern::subr_prf::{panic, printf};
use crate::machine::bus::{
    BusAddr, BusSize, BusSpaceHandle, BusSpaceTag, bus_space_map, bus_space_read_1,
    bus_space_unmap, bus_space_write_1,
};
use crate::machine::cpu::{MACHINE_PC, delay};
use crate::machine::intr::{spltty, splx};
use crate::sys::device::{CfMatch, Cfdriver, DV_DULL, Device, QUIET};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_DEVBUF, M_NOWAIT};
use crate::sys::queue::{TailqEntry, TailqHead};
use crate::sys::time::sec_to_nsec;
use crate::sys::timeout::timeout_pending;

/// `KBC_CMDFLAG_SYNC`: give descriptor back to caller.
const KBC_CMDFLAG_SYNC: i32 = 1;
/// `KBC_CMDFLAG_SLOW`: the device may take ten seconds to answer.
const KBC_CMDFLAG_SLOW: i32 = 2;
/// `KBC_CMDFLAG_QUEUED`: descriptor on cmdqueue.
const KBC_CMDFLAG_QUEUED: i32 = 4;

/// `NCMD`: command descriptors per slot.
pub const NCMD: usize = 5;

/// `KBC_DEVCMD_ACK`.
const KBC_DEVCMD_ACK: u8 = 0xfa;
/// `KBC_DEVCMD_RESEND`.
const KBC_DEVCMD_RESEND: u8 = 0xfe;
/// `KBC_DEVCMD_BAT_DONE`.
const KBC_DEVCMD_BAT_DONE: u8 = 0xaa;
/// `KBC_DEVCMD_BAT_FAIL`.
const KBC_DEVCMD_BAT_FAIL: u8 = 0xfc;

/// `struct pckbc_devcmd`: descriptor for one device command.
pub struct PckbcDevcmd {
    /// `next`: on the slot's `cmdqueue` or `freequeue`.
    next: TailqEntry<PckbcDevcmd>,
    /// `flags`: `KBC_CMDFLAG_*`.
    flags: Cell<i32>,
    /// `cmd`: the bytes to send.
    cmd: Cell<[u8; 4]>,
    /// `cmdlen`.
    cmdlen: Cell<i32>,
    /// `cmdidx`: the next byte to send.
    cmdidx: Cell<i32>,
    /// `retries`.
    retries: Cell<i32>,
    /// `response`: the bytes received.
    response: Cell<[u8; 4]>,
    /// `status`: 0 (`None`) or the error.
    status: Cell<Option<Errno>>,
    /// `responselen`.
    responselen: Cell<i32>,
    /// `responseidx`: the next byte to receive.
    responseidx: Cell<i32>,
}

impl PckbcDevcmd {
    /// A free descriptor (`bzero`).
    pub const fn new() -> Self {
        Self {
            next: TailqEntry::new(),
            flags: Cell::new(0),
            cmd: Cell::new([0; 4]),
            cmdlen: Cell::new(0),
            cmdidx: Cell::new(0),
            retries: Cell::new(0),
            response: Cell::new([0; 4]),
            status: Cell::new(None),
            responselen: Cell::new(0),
            responseidx: Cell::new(0),
        }
    }

    /// `bzero(nc, sizeof(*nc))` on a descriptor that is on no queue, then the command.
    fn setup(&self, cmd: &[u8], responselen: usize, flags: i32) {
        let mut c = [0u8; 4];
        c[..cmd.len()].copy_from_slice(cmd);
        self.flags.set(flags);
        self.cmd.set(c);
        self.cmdlen.set(cmd.len() as i32);
        self.cmdidx.set(0);
        self.retries.set(0);
        self.response.set([0; 4]);
        self.status.set(None);
        self.responselen.set(responselen as i32);
        self.responseidx.set(0);
    }

    /// `cmd->cmd[cmd->cmdidx]`.
    fn cur_byte(&self) -> u8 {
        self.cmd.get()[(self.cmdidx.get() as usize) & 3]
    }

    /// `cmd->response[cmd->responseidx++] = c`.
    fn push_response(&self, c: u8) {
        let mut r = self.response.get();
        r[(self.responseidx.get() as usize) & 3] = c;
        self.response.set(r);
        self.responseidx.set(self.responseidx.get() + 1);
    }
}

impl Default for PckbcDevcmd {
    fn default() -> Self {
        Self::new()
    }
}

crate::queue_adapter!(
    /// `TAILQ_ENTRY(pckbc_devcmd) next`: a slot's `cmdqueue` and `freequeue`.
    pub PckbcDevcmdNext: PckbcDevcmd, next => TailqEntry<PckbcDevcmd>
);

/// `struct pckbc_slotdata`: data per slave device.
pub struct PckbcSlotdata {
    /// `polling`: don't read data port in interrupt handler.
    polling: Cell<i32>,
    /// `cmdqueue`: active commands.
    cmdqueue: TailqHead<PckbcDevcmdNext>,
    /// `freequeue`: free commands.
    freequeue: TailqHead<PckbcDevcmdNext>,
    /// `cmds`.
    cmds: [PckbcDevcmd; NCMD],
}

impl PckbcSlotdata {
    /// Slot data before `pckbc_init_slotdata`.
    pub const fn new() -> Self {
        Self {
            polling: Cell::new(0),
            cmdqueue: TailqHead::new(),
            freequeue: TailqHead::new(),
            cmds: [const { PckbcDevcmd::new() }; NCMD],
        }
    }

    /// `CMD_IN_QUEUE(q)`.
    fn cmd_in_queue(&self) -> bool {
        !self.cmdqueue.is_empty()
    }

    /// `TAILQ_REMOVE(&q->cmdqueue, cmd, next)`.
    fn remove_cmd(&self, cmd: &PckbcDevcmd) {
        // SAFETY: `cmd` is one of `self.cmds`, on `cmdqueue` (the callers' discipline, the
        // C's); the descriptors and the heads live as long as the slot data, never freed.
        unsafe { self.cmdqueue.remove(cmd) };
    }

    /// `TAILQ_INSERT_TAIL(&q->freequeue, cmd, next)`.
    fn free_cmd(&self, cmd: &PckbcDevcmd) {
        // SAFETY: `cmd` is one of `self.cmds`, on no queue at this point (it was taken off
        // `cmdqueue` or `freequeue`); see `remove_cmd`.
        unsafe { self.freequeue.insert_tail(cmd) };
    }
}

impl Default for PckbcSlotdata {
    fn default() -> Self {
        Self::new()
    }
}

// SAFETY: as `PckbcInternal`: touched at `spltty()` under the kernel lock.
unsafe impl Sync for PckbcSlotdata {}

/// `struct pckbc_portcmd`: the enable and disable commands of a slot's port.
struct PckbcPortcmd {
    /// `cmd_en`.
    cmd_en: u8,
    /// `cmd_dis`.
    cmd_dis: u8,
}

/// `pckbc_consdata`: the console controller's state, set up by `pckbc_cnattach`.
pub static PCKBC_CONSDATA: PckbcInternal = PckbcInternal::new();

/// `pckbc_console_attached`: the console controller found its device.
pub static PCKBC_CONSOLE_ATTACHED: AtomicI32 = AtomicI32::new(0);

/// `pckbc_console`: the keyboard on the controller is the console's.
pub static PCKBC_CONSOLE: AtomicI32 = AtomicI32::new(0);

/// `pckbc_cons_slotdata`: the console keyboard slot's queues.
static PCKBC_CONS_SLOTDATA: PckbcSlotdata = PckbcSlotdata::new();

/// `pckbc_slot_names`.
pub static PCKBC_SLOT_NAMES: [&str; PCKBC_NSLOTS] = ["kbd", "aux"];

/// `pckbc_portcmd`.
static PCKBC_PORTCMD: [PckbcPortcmd; 2] = [
    PckbcPortcmd {
        cmd_en: KBC_KBDENABLE,
        cmd_dis: KBC_KBDDISABLE,
    },
    PckbcPortcmd {
        cmd_en: KBC_AUXENABLE,
        cmd_dis: KBC_AUXDISABLE,
    },
];

/// `pckbc_cd`.
pub static PCKBC_CD: Cfdriver = Cfdriver::new(b"pckbc", DV_DULL, 0);

/// The controller as the `void *` its timeouts are called with.
fn tag_arg(t: &PckbcInternal) -> *mut c_void {
    ptr::from_ref(t).cast_mut().cast()
}

/// `KBD_DELAY`.
fn kbd_delay() {
    delay(8);
}

/// `pckbc_wait_output`: wait for the controller's input buffer to empty.
#[inline]
fn pckbc_wait_output(iot: BusSpaceTag, ioh_c: BusSpaceHandle) -> bool {
    for _ in 0..100000u32 {
        if bus_space_read_1(iot, ioh_c, 0) & KBS_IBF == 0 {
            kbd_delay();
            return true;
        }
    }
    false
}

/// `pckbc_send_cmd`: send a command to the controller; `false` when it does not take it.
pub fn pckbc_send_cmd(iot: BusSpaceTag, ioh_c: BusSpaceHandle, val: u8) -> bool {
    if !pckbc_wait_output(iot, ioh_c) {
        return false;
    }
    bus_space_write_1(iot, ioh_c, 0, val);
    true
}

/// `pckbc_poll_data1`: polls for ~100ms for a byte from `slot` (the other slot's bytes, and
/// with `checkaux` the auxiliary ones the keyboard slot does not want, are lost).
pub fn pckbc_poll_data1(
    iot: BusSpaceTag,
    ioh_d: BusSpaceHandle,
    ioh_c: BusSpaceHandle,
    slot: PckbcSlotT,
    checkaux: bool,
) -> Option<u8> {
    for _ in 0..100 {
        let stat = bus_space_read_1(iot, ioh_c, 0);
        if stat & KBS_DIB != 0 {
            kbd_delay();
            core::hint::spin_loop(); // CPU_BUSY_CYCLE()
            let c = bus_space_read_1(iot, ioh_d, 0);
            let lost = if checkaux && stat & KBS_AUXDATA != 0 {
                // lost aux
                slot != PCKBC_AUX_SLOT
            } else {
                // lost kbd, or discard aux data
                slot == PCKBC_AUX_SLOT || stat & KBS_AUXDATA != 0
            };
            if !lost {
                return Some(c);
            }
        }
        delay(1000);
    }
    None
}

/// `pckbc_get8042cmd`: get the current command byte.
fn pckbc_get8042cmd(t: &PckbcInternal) -> bool {
    let Some((iot, ioh_d, ioh_c)) = t.regs() else {
        return false;
    };

    if !pckbc_send_cmd(iot, ioh_c, K_RDCMDBYTE) {
        return false;
    }
    let Some(data) = pckbc_poll_data1(iot, ioh_d, ioh_c, PCKBC_KBD_SLOT, t.t_haveaux.get() != 0)
    else {
        return false;
    };
    t.t_cmdbyte.set(data);
    true
}

/// `pckbc_put8042cmd`: pass command byte to keyboard controller (8042).
fn pckbc_put8042cmd(t: &PckbcInternal) -> bool {
    let Some((iot, ioh_d, ioh_c)) = t.regs() else {
        return false;
    };

    if !pckbc_send_cmd(iot, ioh_c, K_LDCMDBYTE) {
        return false;
    }
    if !pckbc_wait_output(iot, ioh_c) {
        return false;
    }
    bus_space_write_1(iot, ioh_d, 0, t.t_cmdbyte.get());
    true
}

/// `pckbc_send_devcmd`: send a byte to the device on `slot`.
fn pckbc_send_devcmd(t: &PckbcInternal, slot: PckbcSlotT, val: u8) -> bool {
    let Some((iot, ioh_d, ioh_c)) = t.regs() else {
        return false;
    };

    if slot == PCKBC_AUX_SLOT && !pckbc_send_cmd(iot, ioh_c, KBC_AUXWRITE) {
        return false;
    }
    if !pckbc_wait_output(iot, ioh_c) {
        return false;
    }
    bus_space_write_1(iot, ioh_d, 0, val);
    true
}

/// `pckbc_is_console`: the controller at `iot`/`addr` is the console's and not attached yet.
pub fn pckbc_is_console(iot: BusSpaceTag, addr: BusAddr) -> bool {
    PCKBC_CONSOLE.load(Ordering::Relaxed) != 0
        && PCKBC_CONSOLE_ATTACHED.load(Ordering::Relaxed) == 0
        && PCKBC_CONSDATA.t_iot.get() == Some(iot)
        && PCKBC_CONSDATA.t_addr.get() == addr
}

/// `pckbc_submatch_locators`: the `slot` locator.
pub fn pckbc_submatch_locators(
    _parent: Option<&Device>,
    match_: &CfMatch,
    aux: *mut c_void,
) -> i32 {
    let cf = match_.cfdata();
    // SAFETY: `pckbc_attach_slot` hands its children a `PckbcAttachArgs`.
    let pa = unsafe { &*aux.cast::<PckbcAttachArgs>() };

    let loc = cf
        .cf_loc
        .get(PCKBCCF_SLOT)
        .copied()
        .unwrap_or(PCKBCCF_SLOT_DEFAULT);
    if loc != PCKBCCF_SLOT_DEFAULT && loc != i64::from(pa.pa_slot) {
        return 0;
    }
    1
}

/// `pckbc_submatch`: the `slot` locator, then the driver's match.
pub fn pckbc_submatch(parent: Option<&Device>, match_: &CfMatch, aux: *mut c_void) -> i32 {
    let cf = match_.cfdata();

    if pckbc_submatch_locators(parent, match_, aux) == 0 {
        return 0;
    }
    match cf.cf_attach.ca_match {
        Some(ca_match) => ca_match(parent, match_, aux),
        None => 0,
    }
}

/// `pckbc_attach_slot`: attach a device on `slot` (`force`: whatever its match says); the aux
/// slot gets its queues, and a handler that drops its bytes, even without a device.
pub fn pckbc_attach_slot(sc: &PckbcSoftc, slot: PckbcSlotT, force: bool) -> bool {
    let Some(t) = sc.id.get() else {
        return false;
    };
    let mut pa = PckbcAttachArgs {
        pa_tag: t,
        pa_slot: slot,
    };
    let submatch = if force {
        pckbc_submatch_locators
    } else {
        pckbc_submatch
    };
    let mut found = config_found_sm(
        &sc.sc_dv,
        ptr::from_mut(&mut pa).cast(),
        Some(pckbcprint),
        Some(submatch),
    )
    .is_some();

    if (found || slot == PCKBC_AUX_SLOT) && t.slotdata(slot).is_none() {
        let Some(mem) = malloc(size_of::<PckbcSlotdata>(), M_DEVBUF, M_NOWAIT) else {
            return false;
        };
        let q: NonNull<PckbcSlotdata> = mem.cast();
        // SAFETY: a fresh block of `size_of::<PckbcSlotdata>()` bytes, aligned by `malloc`,
        // which nothing else references; it is never freed.
        unsafe { q.as_ptr().write(PckbcSlotdata::new()) };
        t.t_slotdata[slot as usize].set(Some(q));
        if let Some(q) = t.slotdata(slot) {
            pckbc_init_slotdata(q);
        }

        if !found && slot == PCKBC_AUX_SLOT {
            // Some machines don't handle disabling the aux slot completely and still
            // generate data when the mouse is moved, so setup a dummy interrupt handler to
            // discard this slot's data.
            pckbc_set_inputhandler(
                t,
                PCKBC_AUX_SLOT,
                None,
                ptr::from_ref(sc).cast_mut().cast(),
                None,
            );
            found = true;
        }
    }
    found
}

/// `pckbc_attach`: set the controller up, attach the slots' devices and enable their
/// interrupts; `flags` are the bus attachment's `cf_flags`.
pub fn pckbc_attach(sc: &PckbcSoftc, flags: i32) {
    let Some(t) = sc.id.get() else {
        return;
    };
    let Some((iot, ioh_d, ioh_c)) = t.regs() else {
        return;
    };
    let mut haskbd = false;
    let mut cmdbits = 0u8;

    if PCKBC_CONSOLE.load(Ordering::Relaxed) == 0 {
        timeout_set(&t.t_cleanup, pckbc_cleanup, tag_arg(t));
        timeout_set(&t.t_poll, pckbc_poll, tag_arg(t));
    }

    // flush
    let _ = pckbc_poll_data1(iot, ioh_d, ioh_c, PCKBC_KBD_SLOT, false);

    // set initial cmd byte
    if !pckbc_put8042cmd(t) {
        if MACHINE_PC && flags & PCKBCF_FORCE_KEYBOARD_PRESENT == 0 {
            pckbc_release_console();
            return;
        }
        printf(format_args!("kbc: cmd word write error\n"));
        return;
    }

    // XXX Don't check the keyboard port. There are broken keyboard controllers which don't
    // pass the test but work normally otherwise.
    if pckbc_attach_slot(sc, PCKBC_KBD_SLOT, false) {
        cmdbits |= KC8_KENABLE;
        haskbd = true;
    }

    'nomouse: {
        // Check aux port ok. Avoid KBC_AUXTEST because it hangs some older controllers (eg
        // UMC880?).
        if !pckbc_send_cmd(iot, ioh_c, KBC_AUXECHO) {
            printf(format_args!("kbc: aux echo error 1\n"));
            break 'nomouse;
        }
        if !pckbc_wait_output(iot, ioh_c) {
            printf(format_args!("kbc: aux echo error 2\n"));
            break 'nomouse;
        }
        bus_space_write_1(iot, ioh_d, 0, 0x5a); // a random value
        let mut res = pckbc_poll_data1(iot, ioh_d, ioh_c, PCKBC_AUX_SLOT, true);

        if t.t_flags.get() & PCKBC_NEED_AUXWRITE != 0 {
            // The following code is necessary to find the aux port on the oqo-1 machine,
            // among others. However if confuses old (non-ps/2) keyboard controllers (at
            // least UMC880x again).
            if res.is_none() {
                // Read of aux echo timed out, try again
                if !pckbc_send_cmd(iot, ioh_c, KBC_AUXWRITE) {
                    break 'nomouse;
                }
                if !pckbc_wait_output(iot, ioh_c) {
                    break 'nomouse;
                }
                bus_space_write_1(iot, ioh_d, 0, 0x5a);
                res = pckbc_poll_data1(iot, ioh_d, ioh_c, PCKBC_AUX_SLOT, true);
            }
        }

        if res.is_some() {
            // In most cases, the 0x5a gets echoed. Some old controllers (Gateway 2000 circa
            // 1993) return 0xfe here. We are satisfied if there is anything in the aux output
            // buffer.
            t.t_haveaux.set(1);
            if pckbc_attach_slot(sc, PCKBC_AUX_SLOT, false) {
                cmdbits |= KC8_MENABLE;
            }
        }

        if MACHINE_PC && !haskbd && flags & PCKBCF_FORCE_KEYBOARD_PRESENT == 0 {
            if t.t_haveaux.get() != 0 {
                if pckbc_attach_slot(sc, PCKBC_KBD_SLOT, true) {
                    cmdbits |= KC8_KENABLE;
                }
            } else {
                pckbc_release_console();
            }
        }
    }

    // nomouse: enable needed interrupts
    t.t_cmdbyte.set(t.t_cmdbyte.get() | cmdbits);
    if !pckbc_put8042cmd(t) {
        printf(format_args!("kbc: cmd word write error\n"));
    }
}

/// `pckbcprint`: " (kbd slot)" or " (aux slot)" after a slot device's name.
pub fn pckbcprint(aux: *mut c_void, pnp: Option<&[u8]>) -> i32 {
    // SAFETY: as in `pckbc_submatch_locators`.
    let pa = unsafe { &*aux.cast::<PckbcAttachArgs>() };

    if pnp.is_none() {
        let name = PCKBC_SLOT_NAMES
            .get(pa.pa_slot as usize)
            .copied()
            .unwrap_or("?");
        printf(format_args!(" ({} slot)", name));
    }
    QUIET
}

/// `pckbc_release_console`: if there is no keyboard present, yet we are the console, we might
/// be on a legacy-free PC where the PS/2 emulated keyboard was elected as console, but went
/// away as soon as the USB controller drivers attached.
///
/// In that case, we want to release ourselves from console duties, unless we have been able
/// to attach a mouse, which would mean this is a real PS/2 controller after all.
pub fn pckbc_release_console() {
    if MACHINE_PC && PCKBC_CONSOLE.load(Ordering::Relaxed) != 0 {
        PCKBC_CONSOLE.store(0, Ordering::Relaxed);
        // wscn_input_init(1);
        let _ = crate::unported!("wscn_input_init");
    }
}

/// `pckbc_init_slotdata`: every descriptor free, no command active, not polling.
pub fn pckbc_init_slotdata(q: &PckbcSlotdata) {
    q.cmdqueue.init();
    q.freequeue.init();

    for cmd in &q.cmds {
        q.free_cmd(cmd);
    }
    q.polling.set(0);
}

/// `pckbc_flush`: drop a byte waiting from `slot`.
pub fn pckbc_flush(t: PckbcTag, slot: PckbcSlotT) {
    if let Some((iot, ioh_d, ioh_c)) = t.regs() {
        let _ = pckbc_poll_data1(iot, ioh_d, ioh_c, slot, t.t_haveaux.get() != 0);
    }
}

/// `pckbc_poll_data`: a byte from `slot` by polling; one that answers the running command
/// goes to it instead.
pub fn pckbc_poll_data(t: PckbcTag, slot: PckbcSlotT) -> Option<u8> {
    let (iot, ioh_d, ioh_c) = t.regs()?;

    let c = pckbc_poll_data1(iot, ioh_d, ioh_c, slot, t.t_haveaux.get() != 0);
    if let (Some(c), Some(q)) = (c, t.slotdata(slot))
        && q.cmd_in_queue()
    {
        // we jumped into a running command - try to deliver the response
        if pckbc_cmdresponse(t, slot, c) {
            return None;
        }
    }
    c
}

/// `pckbc_xt_translation`: set scancode translation on; `false` when the controller would
/// not (the C's -1).
pub fn pckbc_xt_translation(t: PckbcTag, _table: &mut i32) -> bool {
    // #ifdef __sparc64__: the fixed set 2 and 3 controllers (pckbc at ebus); not here.

    if t.t_cmdbyte.get() & KC8_TRANS != 0 {
        return true;
    }

    t.t_cmdbyte.set(t.t_cmdbyte.get() | KC8_TRANS);
    if !pckbc_put8042cmd(t) {
        return false;
    }

    // read back to be sure
    if !pckbc_get8042cmd(t) {
        return false;
    }

    t.t_cmdbyte.get() & KC8_TRANS != 0
}

/// `pckbc_slot_enable`: enable or disable a slot's port; the keyboard's polling fallback
/// follows it.
pub fn pckbc_slot_enable(t: PckbcTag, slot: PckbcSlotT, on: bool) {
    let Some(cmd) = PCKBC_PORTCMD.get(slot as usize) else {
        return;
    };

    let sent = match (t.t_iot.get(), t.t_ioh_c.get()) {
        (Some(iot), Some(ioh_c)) => {
            pckbc_send_cmd(iot, ioh_c, if on { cmd.cmd_en } else { cmd.cmd_dis })
        }
        _ => false,
    };
    if !sent {
        printf(format_args!(
            "pckbc_slot_enable({}) failed\n",
            i32::from(on)
        ));
    }

    if slot == PCKBC_KBD_SLOT {
        if on {
            timeout_add_sec(&t.t_poll, 1);
        } else {
            timeout_del(&t.t_poll);
        }
    }
}

/// `pckbc_set_poll`: switch polling of `slot` on or off.
pub fn pckbc_set_poll(t: PckbcTag, slot: PckbcSlotT, on: bool) {
    let Some(q) = t.slotdata(slot) else {
        return;
    };
    q.polling.set(i32::from(on));

    if !on {
        // If disabling polling on a device that's been configured, make sure there are no
        // bytes left in the FIFO, holding up the interrupt line. Otherwise we won't get any
        // further interrupts.
        if let Some(sc) = t.sc() {
            let s = spltty();
            pckbcintr_internal(t, Some(sc));
            splx(s);
        }
    }
}

/// `pckbc_poll_cmd1`: pass command to device, poll for ACK and data. To be called at
/// `spltty()`.
fn pckbc_poll_cmd1(t: &PckbcInternal, slot: PckbcSlotT, cmd: &PckbcDevcmd) {
    let Some((iot, ioh_d, ioh_c)) = t.regs() else {
        cmd.status.set(Some(Errno::EIO));
        return;
    };
    let checkaux = t.t_haveaux.get() != 0;

    while cmd.cmdidx.get() < cmd.cmdlen.get() {
        if !pckbc_send_devcmd(t, slot, cmd.cur_byte()) {
            printf(format_args!("pckbc_cmd: send error\n"));
            cmd.status.set(Some(Errno::EIO));
            return;
        }
        let mut c = None;
        for _ in 0..10 {
            // 1s ???
            c = pckbc_poll_data1(iot, ioh_d, ioh_c, slot, checkaux);
            if c.is_some() {
                break;
            }
        }

        match c {
            Some(KBC_DEVCMD_ACK) => {
                cmd.cmdidx.set(cmd.cmdidx.get() + 1);
            }
            // Some legacy free PCs keep returning Basic Assurance Test (BAT) instead of
            // something usable, so fail gracefully.
            Some(KBC_DEVCMD_RESEND | KBC_DEVCMD_BAT_DONE | KBC_DEVCMD_BAT_FAIL) => {
                let retries = cmd.retries.get();
                cmd.retries.set(retries + 1);
                if retries < 5 {
                    continue;
                }
                cmd.status.set(Some(Errno::ENXIO));
                return;
            }
            None => {
                cmd.status.set(Some(Errno::EIO));
                return;
            }
            Some(_) => {} // lost
        }
    }

    while cmd.responseidx.get() < cmd.responselen.get() {
        let tries = if cmd.flags.get() & KBC_CMDFLAG_SLOW != 0 {
            100 // 10s ???
        } else {
            10 // 1s ???
        };
        let mut c = None;
        for _ in 0..tries {
            c = pckbc_poll_data1(iot, ioh_d, ioh_c, slot, checkaux);
            if c.is_some() {
                break;
            }
        }
        match c {
            None => {
                cmd.status.set(Some(Errno::ETIMEDOUT));
                return;
            }
            Some(c) => cmd.push_response(c),
        }
    }
}

/// `pckbc_poll_cmd`: send `cmd` to the device on `slot` and poll for `responselen` bytes of
/// answer (copied to `respbuf`); for use in autoconfiguration. `slow`: the device may take
/// ten seconds per byte.
pub fn pckbc_poll_cmd(
    t: PckbcTag,
    slot: PckbcSlotT,
    cmd: &[u8],
    responselen: usize,
    respbuf: Option<&mut [u8]>,
    slow: bool,
) -> Result<(), Errno> {
    if cmd.len() > 4 || responselen > 4 {
        return Err(Errno::EINVAL);
    }

    let nc = PckbcDevcmd::new();
    nc.setup(cmd, responselen, if slow { KBC_CMDFLAG_SLOW } else { 0 });

    pckbc_poll_cmd1(t, slot, &nc);

    match nc.status.get() {
        Some(e) => Err(e),
        None => {
            if let Some(respbuf) = respbuf {
                let n = responselen.min(respbuf.len());
                respbuf[..n].copy_from_slice(&nc.response.get()[..n]);
            }
            Ok(())
        }
    }
}

/// `pckbc_cleanqueue`: clean up a command queue, throw away everything.
pub fn pckbc_cleanqueue(q: &PckbcSlotdata) {
    while let Some(cmd) = q.cmdqueue.first() {
        q.remove_cmd(cmd);
        cmd.flags.set(cmd.flags.get() & !KBC_CMDFLAG_QUEUED);
        // A synchronous command on the cmdqueue is currently owned by a sleeping proc. The
        // same proc is responsible for putting it back on the freequeue once awake.
        if cmd.flags.get() & KBC_CMDFLAG_SYNC != 0 {
            continue;
        }

        q.free_cmd(cmd);
    }
}

/// `pckbc_cleanqueues`: both slots' queues.
pub fn pckbc_cleanqueues(t: &PckbcInternal) {
    if let Some(q) = t.slotdata(PCKBC_KBD_SLOT) {
        pckbc_cleanqueue(q);
    }
    if let Some(q) = t.slotdata(PCKBC_AUX_SLOT) {
        pckbc_cleanqueue(q);
    }
}

/// `pckbc_cleanup`: timeout error handler: clean queues and data port. XXX could be less
/// invasive.
pub fn pckbc_cleanup(self_: *mut c_void) {
    // SAFETY: the argument of `t_cleanup` (`timeout_set` in `pckbc_attach` and
    // `pckbc_cnattach`) and of the direct calls is the controller, never freed.
    let t = unsafe { &*self_.cast::<PckbcInternal>() };

    printf(format_args!("pckbc: command timeout\n"));

    let s = spltty();

    pckbc_cleanqueues(t);

    if let Some((iot, ioh_d, ioh_c)) = t.regs() {
        while bus_space_read_1(iot, ioh_c, 0) & KBS_DIB != 0 {
            kbd_delay();
            let _ = bus_space_read_1(iot, ioh_d, 0);
        }
    }

    // reset KBC?

    splx(s);
}

/// `pckbc_stop`: stop the keyboard controller when we are going to suspend.
pub fn pckbc_stop(sc: &PckbcSoftc) {
    let Some(t) = sc.id.get() else {
        return;
    };

    timeout_del(&t.t_poll);
    pckbc_cleanqueues(t);
    timeout_del(&t.t_cleanup);
}

/// `pckbc_reset`: reset the keyboard controller in a violent fashion; normally done after
/// suspend/resume when we do not trust the machine.
pub fn pckbc_reset(sc: &PckbcSoftc) {
    let Some(t) = sc.id.get() else {
        return;
    };
    let Some((iot, ioh_d, ioh_c)) = t.regs() else {
        return;
    };

    let _ = pckbc_poll_data1(iot, ioh_d, ioh_c, PCKBC_KBD_SLOT, false);
    // KBC selftest
    if !pckbc_send_cmd(iot, ioh_c, KBC_SELFTEST) {
        return;
    }
    let _ = pckbc_poll_data1(iot, ioh_d, ioh_c, PCKBC_KBD_SLOT, false);
    let _ = pckbc_put8042cmd(t);
    if let Some(tsc) = t.sc()
        && let Some(id) = tsc.id.get()
    {
        pckbcintr_internal(id, Some(tsc));
    }
}

/// `pckbc_start`: pass command to device during normal operation. To be called at
/// `spltty()`.
pub fn pckbc_start(t: &PckbcInternal, slot: PckbcSlotT) {
    let Some(q) = t.slotdata(slot) else {
        return;
    };
    let Some(mut cmd) = q.cmdqueue.first() else {
        return;
    };

    if q.polling.get() != 0 {
        loop {
            pckbc_poll_cmd1(t, slot, cmd);
            if cmd.status.get().is_some() {
                printf(format_args!("pckbc_start: command error\n"));
            }

            q.remove_cmd(cmd);
            cmd.flags.set(cmd.flags.get() & !KBC_CMDFLAG_QUEUED);
            if cmd.flags.get() & KBC_CMDFLAG_SYNC != 0 {
                wakeup(ptr::from_ref(cmd));
            } else {
                timeout_del(&t.t_cleanup);
                q.free_cmd(cmd);
            }
            match q.cmdqueue.first() {
                Some(c) => cmd = c,
                None => break,
            }
        }
        return;
    }

    if !pckbc_send_devcmd(t, slot, cmd.cur_byte()) {
        printf(format_args!("pckbc_start: send error\n"));
        // XXX what now?
    }
}

/// `pckbc_cmdresponse`: handle command responses coming in asynchronously; `true` if
/// `data` was a valid response. To be called at `spltty()`.
pub fn pckbc_cmdresponse(t: &PckbcInternal, slot: PckbcSlotT, data: u8) -> bool {
    let Some(q) = t.slotdata(slot) else {
        return false;
    };
    let Some(cmd) = q.cmdqueue.first() else {
        if cfg!(feature = "diagnostic") {
            panic(format_args!("pckbc_cmdresponse: no active command"));
        }
        return false;
    };

    let restart = if cmd.cmdidx.get() < cmd.cmdlen.get() {
        if data != KBC_DEVCMD_ACK && data != KBC_DEVCMD_RESEND {
            return false;
        }

        if data == KBC_DEVCMD_RESEND {
            let retries = cmd.retries.get();
            cmd.retries.set(retries + 1);
            if retries < 5 {
                // try again last command
                true
            } else {
                cmd.status.set(Some(Errno::ENXIO));
                // dequeue
                false
            }
        } else {
            cmd.cmdidx.set(cmd.cmdidx.get() + 1);
            if cmd.cmdidx.get() < cmd.cmdlen.get() {
                true
            } else if cmd.responselen.get() != 0 {
                return true;
            } else {
                // else dequeue
                false
            }
        }
    } else if cmd.responseidx.get() < cmd.responselen.get() {
        cmd.push_response(data);
        if cmd.responseidx.get() < cmd.responselen.get() {
            return true;
        }
        // else dequeue
        false
    } else {
        return false;
    };

    if !restart {
        // dequeue:
        q.remove_cmd(cmd);
        cmd.flags.set(cmd.flags.get() & !KBC_CMDFLAG_QUEUED);
        if cmd.flags.get() & KBC_CMDFLAG_SYNC != 0 {
            wakeup(ptr::from_ref(cmd));
        } else {
            timeout_del(&t.t_cleanup);
            q.free_cmd(cmd);
        }
        if q.cmdqueue.first().is_none() {
            return true;
        }
    }
    // restart:
    pckbc_start(t, slot);
    true
}

/// `pckbc_enqueue_cmd`: put command into the device's command queue; with `sync`, wait for
/// it (one second at most) and copy `responselen` bytes of answer to `respbuf`.
pub fn pckbc_enqueue_cmd(
    t: PckbcTag,
    slot: PckbcSlotT,
    cmd: &[u8],
    responselen: usize,
    sync: bool,
    respbuf: Option<&mut [u8]>,
) -> Result<(), Errno> {
    if cmd.len() > 4 || responselen > 4 {
        return Err(Errno::EINVAL);
    }
    let Some(q) = t.slotdata(slot) else {
        return Err(Errno::ENXIO);
    };

    let s = spltty();
    let nc = q.freequeue.first();
    if let Some(nc) = nc {
        // SAFETY: `nc` is the first of `freequeue`; see `PckbcSlotdata::remove_cmd`.
        unsafe { q.freequeue.remove(nc) };
    }
    splx(s);
    let Some(nc) = nc else {
        return Err(Errno::ENOMEM);
    };

    nc.setup(cmd, responselen, if sync { KBC_CMDFLAG_SYNC } else { 0 });

    let s = spltty();

    if q.polling.get() != 0 && sync {
        // XXX We should poll until the queue is empty. But we don't come here normally, so
        // make it simple and throw away everything.
        pckbc_cleanqueue(q);
    }

    let isactive = q.cmd_in_queue();
    nc.flags.set(nc.flags.get() | KBC_CMDFLAG_QUEUED);
    // SAFETY: `nc` was taken off `freequeue` above and is on no queue.
    unsafe { q.cmdqueue.insert_tail(nc) };
    if !isactive {
        pckbc_start(t, slot);
    }

    let mut res = Ok(());
    if q.polling.get() != 0 {
        if sync {
            res = nc.status.get().map_or(Ok(()), Err);
        }
    } else if sync {
        match tsleep_nsec(ptr::from_ref(nc), 0, "kbccmd", sec_to_nsec(1)) {
            Err(e) => {
                res = Err(e);
                pckbc_cleanup(tag_arg(t));
            }
            Ok(()) => {
                // Under certain circumstances, such as during suspend, tsleep() becomes a
                // no-op and the command is left on the cmdqueue.
                if nc.flags.get() & KBC_CMDFLAG_QUEUED != 0 {
                    q.remove_cmd(nc);
                    nc.flags.set(nc.flags.get() & !KBC_CMDFLAG_QUEUED);
                }
                res = nc.status.get().map_or(Ok(()), Err);
            }
        }
    } else {
        timeout_add_sec(&t.t_cleanup, 1);
    }

    if sync {
        if let Some(respbuf) = respbuf {
            let n = responselen.min(respbuf.len());
            respbuf[..n].copy_from_slice(&nc.response.get()[..n]);
        }
        q.free_cmd(nc);
    }

    splx(s);

    res
}

/// `pckbc_set_inputhandler`: the handler the bytes of `slot` go to (`None` drops them), its
/// argument and the device's name.
pub fn pckbc_set_inputhandler(
    t: PckbcTag,
    slot: PckbcSlotT,
    func: Option<PckbcInputfcn>,
    arg: *mut c_void,
    name: Option<&'static str>,
) {
    if slot < 0 || slot as usize >= PCKBC_NSLOTS {
        panic(format_args!("pckbc_set_inputhandler: bad slot {}", slot));
    }
    let Some(sc) = t.sc() else {
        return;
    };

    sc.inputhandler[slot as usize].set(func);
    sc.inputarg[slot as usize].set(arg);
    sc.subname[slot as usize].set(name);

    if PCKBC_CONSOLE.load(Ordering::Relaxed) != 0 && slot == PCKBC_KBD_SLOT {
        timeout_add_sec(&t.t_poll, 1);
    }
}

/// `pckbc_poll`: the `t_poll` timeout: run the interrupt handler, every second.
pub fn pckbc_poll(v: *mut c_void) {
    // SAFETY: as in `pckbc_cleanup`.
    let t = unsafe { &*v.cast::<PckbcInternal>() };

    let s = spltty();
    let _ = pckbcintr_internal(t, t.sc());
    timeout_add_sec(&t.t_poll, 1);
    splx(s);
}

/// `pckbcintr`: the controller's interrupt handler (both irqs).
pub fn pckbcintr(vsc: *mut c_void) -> i32 {
    // SAFETY: the bus attachment establishes the interrupts with the softc as argument; it
    // is never detached.
    let sc = unsafe { &*vsc.cast::<PckbcSoftc>() };

    match sc.id.get() {
        Some(t) => pckbcintr_internal(t, Some(sc)),
        None => 0,
    }
}

/// `pckbcintr_internal`: read every byte the controller holds, deliver command responses
/// and pass the others to the slots' input handlers; 1 if there was anything.
pub fn pckbcintr_internal(t: &PckbcInternal, sc: Option<&PckbcSoftc>) -> i32 {
    let mut served = 0;

    // reschedule timeout further into the idle times
    if timeout_pending(&t.t_poll) {
        timeout_add_sec(&t.t_poll, 1);
    }

    let Some((iot, ioh_d, ioh_c)) = t.regs() else {
        return 0;
    };

    loop {
        let stat = bus_space_read_1(iot, ioh_c, 0);
        if stat & KBS_DIB == 0 {
            break;
        }

        served = 1;

        let slot = if t.t_haveaux.get() != 0 && stat & KBS_AUXDATA != 0 {
            PCKBC_AUX_SLOT
        } else {
            PCKBC_KBD_SLOT
        };
        let Some(q) = t.slotdata(slot) else {
            // XXX do something for live insertion?
            kbd_delay();
            let _ = bus_space_read_1(iot, ioh_d, 0);
            continue;
        };

        if q.polling.get() != 0 {
            break; // pckbc_poll_data() will get it
        }

        kbd_delay();
        let data = bus_space_read_1(iot, ioh_d, 0);

        if q.cmd_in_queue() && pckbc_cmdresponse(t, slot, data) {
            continue;
        }

        if let Some(sc) = sc
            && let Some(handler) = sc.inputhandler[slot as usize].get()
        {
            handler(sc.inputarg[slot as usize].get(), i32::from(data));
        }
    }

    served
}

/// `pckbc_cnattach`: set the controller at `iot`/`addr` up as the console keyboard's
/// (`cmd_offset`: where its command port is) and attach the console keyboard.
pub fn pckbc_cnattach(
    iot: BusSpaceTag,
    addr: BusAddr,
    cmd_offset: BusSize,
    flags: i32,
) -> Result<(), Errno> {
    // SAFETY: the controller's data port, which only the console keyboard uses.
    let Ok(ioh_d) = (unsafe { bus_space_map(iot, addr + KBDATAP, 1, 0) }) else {
        return Err(Errno::ENXIO);
    };
    // SAFETY: as above, the command port.
    let Ok(ioh_c) = (unsafe { bus_space_map(iot, addr + cmd_offset, 1, 0) }) else {
        bus_space_unmap(iot, ioh_d, 1);
        return Err(Errno::ENXIO);
    };

    let t = &PCKBC_CONSDATA;
    t.t_iot.set(Some(iot));
    t.t_ioh_d.set(Some(ioh_d));
    t.t_ioh_c.set(Some(ioh_c));
    t.t_addr.set(addr);
    t.t_flags.set(flags);
    timeout_set(&t.t_cleanup, pckbc_cleanup, tag_arg(t));
    timeout_set(&t.t_poll, pckbc_poll, tag_arg(t));

    // flush
    let _ = pckbc_poll_data1(iot, ioh_d, ioh_c, PCKBC_KBD_SLOT, false);

    // selftest?

    // init cmd byte, enable ports
    t.t_cmdbyte.set(KC8_CPU);
    let mut res = Ok(());
    if !pckbc_put8042cmd(t) {
        printf(format_args!("kbc: cmd word write error\n"));
        res = Err(Errno::EIO);
    }

    if res.is_ok() {
        // NPCKBD > 0
        res = pckbd_cnattach(t);
    }

    if res.is_err() {
        bus_space_unmap(iot, ioh_d, 1);
        bus_space_unmap(iot, ioh_c, 1);
    } else {
        t.t_slotdata[PCKBC_KBD_SLOT as usize].set(Some(NonNull::from(&PCKBC_CONS_SLOTDATA)));
        pckbc_init_slotdata(&PCKBC_CONS_SLOTDATA);
        PCKBC_CONSOLE.store(1, Ordering::Relaxed);
    }

    res
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests of the command queues: the host's bus space reads 0 (input buffer empty,
    // no data), so commands are sent at once and never answered by the "hardware"; the
    // answers are fed through `pckbc_cmdresponse` as `pckbcintr` would.

    use std::boxed::Box;

    use super::*;

    /// The timeout wheel is global: hold its tests' lock while a controller's timeouts may be
    /// on it, and take them off when done.
    struct Wheel(
        &'static PckbcInternal,
        #[allow(dead_code)] std::sync::MutexGuard<'static, ()>,
    );

    impl Drop for Wheel {
        fn drop(&mut self) {
            timeout_del(&self.0.t_cleanup);
            timeout_del(&self.0.t_poll);
        }
    }

    /// A controller with mapped (host) ports and slot data on the keyboard slot.
    fn controller() -> (&'static PckbcInternal, Wheel) {
        let g = crate::kern::kern_timeout::tests::LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let t: &'static PckbcInternal = Box::leak(Box::new(PckbcInternal::new()));
        let q: &'static PckbcSlotdata = Box::leak(Box::new(PckbcSlotdata::new()));
        pckbc_init_slotdata(q);
        t.t_iot.set(Some(Default::default()));
        // SAFETY: the host's bus space maps nothing.
        t.t_ioh_d
            .set(unsafe { bus_space_map(Default::default(), 0x60, 1, 0) }.ok());
        // SAFETY: as above.
        t.t_ioh_c
            .set(unsafe { bus_space_map(Default::default(), 0x64, 1, 0) }.ok());
        t.t_slotdata[0].set(Some(NonNull::from(q)));
        timeout_set(&t.t_cleanup, pckbc_cleanup, tag_arg(t));
        timeout_set(&t.t_poll, pckbc_poll, tag_arg(t));
        (t, Wheel(t, g))
    }

    #[test]
    fn a_slot_starts_with_every_descriptor_free() {
        let (t, _w) = controller();
        let q = t.slotdata(PCKBC_KBD_SLOT).unwrap();
        assert_eq!(q.freequeue.iter().count(), NCMD);
        assert!(!q.cmd_in_queue());
    }

    #[test]
    fn too_long_commands_are_refused() {
        let (t, _w) = controller();
        assert_eq!(
            pckbc_enqueue_cmd(t, PCKBC_KBD_SLOT, &[1, 2, 3, 4, 5], 0, false, None),
            Err(Errno::EINVAL)
        );
        assert_eq!(
            pckbc_poll_cmd(t, PCKBC_KBD_SLOT, &[1], 5, None, false),
            Err(Errno::EINVAL)
        );
        assert_eq!(
            pckbc_enqueue_cmd(t, PCKBC_AUX_SLOT, &[1], 0, false, None),
            Err(Errno::ENXIO)
        );
    }

    #[test]
    fn an_async_command_is_acked_byte_by_byte_then_freed() {
        let (t, _w) = controller();
        let q = t.slotdata(PCKBC_KBD_SLOT).unwrap();

        // KBC_MODEIND and its argument, as pckbd_set_leds sends them.
        assert_eq!(
            pckbc_enqueue_cmd(t, PCKBC_KBD_SLOT, &[0xed, 0x02], 0, false, None),
            Ok(())
        );
        assert!(q.cmd_in_queue());
        assert_eq!(q.freequeue.iter().count(), NCMD - 1);

        // Not an ack: the byte is input, not a response.
        assert!(!pckbc_cmdresponse(t, PCKBC_KBD_SLOT, 0x1c));
        // The first ack sends the second byte; the second completes the command.
        assert!(pckbc_cmdresponse(t, PCKBC_KBD_SLOT, KBC_DEVCMD_ACK));
        assert!(q.cmd_in_queue());
        assert!(pckbc_cmdresponse(t, PCKBC_KBD_SLOT, KBC_DEVCMD_ACK));
        assert!(!q.cmd_in_queue());
        assert_eq!(q.freequeue.iter().count(), NCMD);
    }

    #[test]
    fn responses_are_collected_and_resends_retried() {
        let (t, _w) = controller();
        let q = t.slotdata(PCKBC_KBD_SLOT).unwrap();

        assert_eq!(
            pckbc_enqueue_cmd(t, PCKBC_KBD_SLOT, &[0xf2], 2, false, None),
            Ok(())
        );
        let cmd = q.cmdqueue.first().unwrap();
        for _ in 0..5 {
            assert!(pckbc_cmdresponse(t, PCKBC_KBD_SLOT, KBC_DEVCMD_RESEND));
            assert!(cmd.status.get().is_none());
        }
        assert!(pckbc_cmdresponse(t, PCKBC_KBD_SLOT, KBC_DEVCMD_ACK));
        assert!(pckbc_cmdresponse(t, PCKBC_KBD_SLOT, 0xab));
        assert!(q.cmd_in_queue());
        assert!(pckbc_cmdresponse(t, PCKBC_KBD_SLOT, 0x83));
        assert!(!q.cmd_in_queue());
        assert_eq!(cmd.response.get()[..2], [0xab, 0x83]);
        assert!(cmd.status.get().is_none());

        // A sixth resend gives up with ENXIO.
        assert_eq!(
            pckbc_enqueue_cmd(t, PCKBC_KBD_SLOT, &[0xf4], 0, false, None),
            Ok(())
        );
        let cmd = q.cmdqueue.first().unwrap();
        for _ in 0..6 {
            assert!(pckbc_cmdresponse(t, PCKBC_KBD_SLOT, KBC_DEVCMD_RESEND));
        }
        assert_eq!(cmd.status.get(), Some(Errno::ENXIO));
        assert!(!q.cmd_in_queue());
        assert_eq!(q.freequeue.iter().count(), NCMD);
    }

    #[test]
    fn the_queue_runs_out_of_descriptors_and_cleans_up() {
        let (t, _w) = controller();
        let q = t.slotdata(PCKBC_KBD_SLOT).unwrap();
        for _ in 0..NCMD {
            assert_eq!(
                pckbc_enqueue_cmd(t, PCKBC_KBD_SLOT, &[0xf4], 0, false, None),
                Ok(())
            );
        }
        assert_eq!(
            pckbc_enqueue_cmd(t, PCKBC_KBD_SLOT, &[0xf4], 0, false, None),
            Err(Errno::ENOMEM)
        );
        pckbc_cleanqueues(t);
        assert!(!q.cmd_in_queue());
        assert_eq!(q.freequeue.iter().count(), NCMD);
    }

    #[test]
    fn a_polled_command_times_out_without_an_answer() {
        let (t, _w) = controller();
        // The host's controller never has data: no ack.
        assert_eq!(
            pckbc_poll_cmd(t, PCKBC_KBD_SLOT, &[0xff], 1, None, false),
            Err(Errno::EIO)
        );
        assert_eq!(pckbc_poll_data(t, PCKBC_KBD_SLOT), None);
    }
}
/* </TESTS> */
