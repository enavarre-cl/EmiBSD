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
 * Copyright (c) 2009 Dariusz Swiderski <sfires@sfires.net>
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
//! em(4) on the EP80579 (Tolapai) SoC (`<dev/pci/if_em_soc.h>` and `if_em_soc.c`): finding
//! the GCU (the unit that owns the MDIO bus of the SoC's MACs) and the PHY register access
//! through it.
//!
//! Upstream: sys/dev/pci/if_em_soc.h @ 3ce1f3f79392, sys/dev/pci/if_em_soc.c @ 3ce1f3f79392
//!
//! The header's prototypes and constants and the file's functions share this module. gcu(4)
//! is configured only by i386 GENERIC: on amd64 and arm64 `NGCU` is 0, so
//! [`em_lookup_gcu`] finds none, `hw.gcu` stays `None`, and the MDIO functions return at
//! once, as the C does with a NULL `hw->gcu`.
//!
//! ## Deviations
//! - `em_lookup_gcu` returns `Option<NonNull<GcuSoftc>>` (the C's `void *`); it is the
//!   `NGCU == 0` branch, the only one this kernel configures.
//! - `DEVNAME(sc)` (the em softc's name, reached through `osdep->dev`) is the name of
//!   `osdep.dev`, the same device; `"em"` when the device is not set.
//! - The command-and-poll sequence both MDIO functions write out is `gcu_mdio_command`.
//! - The `struct device *self` arguments of `em_lookup_gcu` and `em_attach_miibus` are
//!   `&Device`; neither function reads it.

use core::ptr::NonNull;

use crate::dev::pci::gcu_reg::{
    MDIO_COMMAND_GO_MASK, MDIO_COMMAND_GO_OFFSET, MDIO_COMMAND_OPER_MASK,
    MDIO_COMMAND_PHY_ADDR_OFFSET, MDIO_COMMAND_PHY_REG_OFFSET, MDIO_COMMAND_REG,
    MDIO_STATUS_READ_DATA_MASK, MDIO_STATUS_REG, MDIO_STATUS_STATUS_MASK,
};
use crate::dev::pci::gcu_var::GcuSoftc;
use crate::dev::pci::if_em_hw::EmHw;
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::subr_prf::printf;
use crate::machine::bus::{bus_space_read_4, bus_space_write_4};
use crate::machine::cpu::delay;
use crate::sys::device::Device;

/// `GCU_MAX_ATTEMPTS`: polls of the MDIO command register before giving up.
pub const GCU_MAX_ATTEMPTS: i32 = 64;
/// `GCU_CMD_DELAY`: microseconds between two polls.
pub const GCU_CMD_DELAY: u32 = 50;
/// `RTL8211_E_PHY_ID`: the PHY id of the Realtek RTL8211 on EP80579 boards.
pub const RTL8211_E_PHY_ID: u32 = 0x001CC912;

/// `em_lookup_gcu`: the GCU softc (unit 0), when gcu(4) is configured; it is not
/// (`NGCU == 0`), so there is none.
pub fn em_lookup_gcu(_self: &Device) -> Option<NonNull<GcuSoftc>> {
    None
}

/// `em_attach_miibus`: nothing to attach; 0.
pub fn em_attach_miibus(_self: &Device) -> i32 {
    0
}

/// The GCU `hw.gcu` points at, if any.
fn em_gcu(hw: &EmHw) -> Option<&GcuSoftc> {
    // SAFETY: `hw.gcu` is only ever set (by if_em.c) from `em_lookup_gcu`, to an attached
    // gcu(4) softc; autoconf never frees a softc while the system runs, and the members read
    // here (the mapping and the mutex) are written only at the GCU's attach.
    hw.gcu.map(|g| unsafe { &*g.as_ptr() })
}

/// The em device's name for messages (the C's `DEVNAME(sc)`).
fn em_devname(hw: &EmHw) -> &str {
    match hw.osdep().dev {
        // SAFETY: if_em.c points `osdep.dev` at its softc's device before any PHY access;
        // the softc outlives the shared code's use of it.
        Some(d) => unsafe { d.as_ref() }.xname(),
        None => "em",
    }
}

/// Starts an MDIO command and polls until the GCU clears its GO bit; true if it did within
/// `GCU_MAX_ATTEMPTS` polls.
fn gcu_mdio_command(gcu: &GcuSoftc, command: u32) -> bool {
    let mut done = false;
    let mut i = 0;

    mtx_enter(&gcu.mdio_mtx);
    bus_space_write_4(gcu.tag, gcu.handle, MDIO_COMMAND_REG, command);

    while !done && {
        let at = i;
        i += 1;
        at < GCU_MAX_ATTEMPTS
    } {
        delay(GCU_CMD_DELAY);
        let data = bus_space_read_4(gcu.tag, gcu.handle, MDIO_COMMAND_REG);
        done = (data & MDIO_COMMAND_GO_MASK) >> MDIO_COMMAND_GO_OFFSET == 0;
    }
    mtx_leave(&gcu.mdio_mtx);

    i < GCU_MAX_ATTEMPTS
}

/// `gcu_miibus_readreg`: reads PHY register `reg` of PHY `phy` through the GCU; 0 when there
/// is no GCU or the read fails (a message says which).
pub fn gcu_miibus_readreg(hw: &EmHw, phy: i32, reg: i32) -> i32 {
    let Some(gcu) = em_gcu(hw) else {
        return 0;
    };

    // Format the data to be written to MDIO_COMMAND_REG.
    let mut data: u32 = 0;
    data |= (reg as u32) << MDIO_COMMAND_PHY_REG_OFFSET;
    data |= (phy as u32) << MDIO_COMMAND_PHY_ADDR_OFFSET;
    data |= MDIO_COMMAND_GO_MASK;

    if !gcu_mdio_command(gcu, data) {
        printf(format_args!(
            "{}: phy read timeout: phy {}, reg {}\n",
            em_devname(hw),
            phy,
            reg
        ));
        return 0;
    }

    mtx_enter(&gcu.mdio_mtx);
    let data = bus_space_read_4(gcu.tag, gcu.handle, MDIO_STATUS_REG);
    mtx_leave(&gcu.mdio_mtx);

    if data & MDIO_STATUS_STATUS_MASK != 0 {
        printf(format_args!(
            "{}: unable to read phy {} reg {}\n",
            em_devname(hw),
            phy,
            reg
        ));
        return 0;
    }
    i32::from((data & MDIO_STATUS_READ_DATA_MASK) as u16)
}

/// `gcu_miibus_writereg`: writes `val` to PHY register `reg` of PHY `phy` through the GCU
/// (nothing when there is no GCU; a timeout is reported).
pub fn gcu_miibus_writereg(hw: &EmHw, phy: i32, reg: i32, val: i32) {
    let Some(gcu) = em_gcu(hw) else {
        return;
    };

    // Format the data to be written to the MDIO_COMMAND_REG.
    let mut data = val as u32;
    data |= (reg as u32) << MDIO_COMMAND_PHY_REG_OFFSET;
    data |= (phy as u32) << MDIO_COMMAND_PHY_ADDR_OFFSET;
    data |= MDIO_COMMAND_OPER_MASK | MDIO_COMMAND_GO_MASK;

    if !gcu_mdio_command(gcu, data) {
        printf(format_args!(
            "{}: phy read timeout: phy {}, reg {}\n",
            em_devname(hw),
            phy,
            reg
        ));
    }
}
/* </CODE> */
