/*	$OpenBSD: i2c_scan.c,v 1.147 2024/09/04 07:54:52 mglocker Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2005 Theo de Raadt <deraadt@openbsd.org>
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
//! I2C bus scanning: `iic_scan` and the probe heuristics that name the chips on a bus.
//!
//! Upstream: sys/dev/i2c/i2c_scan.c @ 3ce1f3f79392
//!
//! The scan sends a RECEIVE BYTE command to every address of two lists, the sensor addresses
//! (`0x18..=0x1f`, `0x20..=0x2f`, `0x48..=0x4e`; `0x4f` is skipped because probing it seems
//! to crash at least one Sony VAIO laptop) and the EEPROM ones (`0x50..=0x57`). A device that
//! answers is identified by the probe of its list, from the values of registers it reads
//! (never writes): a chip name is handed to `config_found`, which finds the driver that
//! claims it or prints the device as not configured. Sensors nobody names get a register
//! dump (`I2C_VERBOSE` is defined).
//!
//! ## Deviations
//! - The probe's state (`probe_ic`, `probe_addr`, `probe_val[256]` and `skip_fc`, file-level
//!   statics in the C) is the [`Probe`] one scan step makes and passes down, so the heuristics
//!   run on any controller: the host tests feed them fake register files. `skip_fc` is
//!   cleared by the sensor probe, and set by the probes that recognise a Maxim 1617 clone, as
//!   in C; as nothing else reads it, living in the `Probe` is equivalent.
//! - The probes return `Option<&'static str>` (the chip name or NULL) and take the `Probe`
//!   instead of `(struct device *self, u_int8_t addr)`: `self` is unused by them and the
//!   `Probe` carries the address.
//! - `ignore_addrs[MAX_IGNORE]` is an array of atomics.
//! - `I2C_DEBUG` (undefined in the C) and the `#if 0` ds1624/ds1631/ds1721 probe are not
//!   configured; `I2C_VERBOSE` (defined) is, and its `#ifndef` branch is not.

use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{AtomicU8, Ordering};

use crate::dev::i2c::i2c::iic_print;
use crate::dev::i2c::i2c_exec::iic_exec;
use crate::dev::i2c::i2c_io::{I2C_OP_READ_WITH_STOP, I2cAddr};
use crate::dev::i2c::i2cvar::{
    I2cAttachArgs, I2cTag, I2cbusAttachArgs, iic_acquire_bus, iic_release_bus,
};
use crate::dev::ipmi::IPMI_ENABLED;
use crate::kern::subr_autoconf::config_found;
use crate::kern::subr_prf::printf;
use crate::sys::device::Device;

/// `MAX_IGNORE`: how many addresses `iic_ignore_addr` can remember.
const MAX_IGNORE: usize = 8;

/// `ignore_addrs[MAX_IGNORE]`: addresses a driver claimed while the scan runs; the scan skips
/// them. 0 is a free slot.
static IGNORE_ADDRS: [AtomicU8; MAX_IGNORE] = [const { AtomicU8::new(0) }; MAX_IGNORE];

/// `struct iicprobelist`: an inclusive range of addresses.
struct IicProbeList {
    start: u8,
    end: u8,
}

/// `probe_addrs_sensor[]`: addresses at which to probe for sensors. Skip address 0x4f, since
/// probing it seems to crash at least one Sony VAIO laptop. Only a few chips can actually sit
/// at that address, and vendors seem to place those at other addresses, so this isn't a big
/// loss.
static PROBE_ADDRS_SENSOR: [IicProbeList; 3] = [
    IicProbeList {
        start: 0x18,
        end: 0x1f,
    },
    IicProbeList {
        start: 0x20,
        end: 0x2f,
    },
    IicProbeList {
        start: 0x48,
        end: 0x4e,
    },
];

/// `probe_addrs_eeprom[]`: addresses at which to probe for eeprom devices.
static PROBE_ADDRS_EEPROM: [IicProbeList; 1] = [IicProbeList {
    start: 0x50,
    end: 0x57,
}];

/// `PFLAG_SENSOR`.
const PFLAG_SENSOR: i32 = 1;

/// One entry of `probes[]`.
struct ProbeSet {
    pl: &'static [IicProbeList],
    probe: fn(&mut Probe) -> Option<&'static str>,
    flags: i32,
}

/// `probes[]`.
static PROBES: [ProbeSet; 2] = [
    ProbeSet {
        pl: &PROBE_ADDRS_SENSOR,
        probe: iic_probe_sensor,
        flags: PFLAG_SENSOR,
    },
    ProbeSet {
        pl: &PROBE_ADDRS_EEPROM,
        probe: iic_probe_eeprom,
        flags: 0,
    },
];

/// The state of the probe of one address: the C's `probe_ic`, `probe_addr`, `probe_val[]` and
/// `skip_fc`.
///
/// Some Maxim 1617 clones MAY NOT even read cmd 0xfc! When it is read, they will
/// power-on-reset. Their default condition (control register bit 0x80) therefore will be that
/// they assert /ALERT for the 5 potential errors that may occur. One of those errors is that
/// the external temperature diode is missing. This is unfortunately a common choice of system
/// designers, except suddenly now we get a /ALERT, which may on some chipsets cause us to
/// receive an entirely unexpected SMI .. and then an NMI.
///
/// As we probe each device, if we hit something which looks suspiciously like it may
/// potentially be a 1617 or clone, we immediately set `skip_fc` to avoid reading that
/// register offset.
pub struct Probe {
    probe_ic: I2cTag,
    probe_addr: u8,
    probe_val: [u8; 256],
    skip_fc: bool,
}

impl Probe {
    /// `iicprobeinit`: starts the probe of the device at `addr` on the bus of `iba`.
    pub fn new(iba: &I2cbusAttachArgs, addr: u8) -> Probe {
        Probe::on(iba.iba_tag, addr)
    }

    /// `iicprobeinit` over a bare controller.
    pub fn on(ic: I2cTag, addr: u8) -> Probe {
        Probe {
            probe_ic: ic,
            probe_addr: addr,
            probe_val: [0xff; 256],
            skip_fc: false,
        }
    }

    /// `iicprobenc`: reads register `cmd` without the cache.
    pub fn iicprobenc(&mut self, cmd: u8) -> u8 {
        // If we think we are talking to an evil Maxim 1617 or clone, avoid accessing this
        // register because it is death.
        if self.skip_fc && cmd == 0xfc {
            return 0xff;
        }
        let mut data = [0u8; 1];
        iic_acquire_bus(self.probe_ic, 0);
        if iic_exec(
            self.probe_ic,
            I2C_OP_READ_WITH_STOP,
            I2cAddr::from(self.probe_addr),
            &[cmd],
            &mut data,
            0,
        ) != 0
        {
            data[0] = 0xff;
        }
        iic_release_bus(self.probe_ic, 0);
        data[0]
    }

    /// `iicprobew`: reads the big-endian word at register `cmd`.
    pub fn iicprobew(&mut self, cmd: u8) -> u16 {
        // If we think we are talking to an evil Maxim 1617 or clone, avoid accessing this
        // register because it is death.
        if self.skip_fc && cmd == 0xfc {
            return 0xffff;
        }
        let mut data = [0u8; 2];
        iic_acquire_bus(self.probe_ic, 0);
        if iic_exec(
            self.probe_ic,
            I2C_OP_READ_WITH_STOP,
            I2cAddr::from(self.probe_addr),
            &[cmd],
            &mut data,
            0,
        ) != 0
        {
            data = [0xff, 0xff];
        }
        iic_release_bus(self.probe_ic, 0);
        // betoh16 of the two bytes as they arrived.
        u16::from_be_bytes(data)
    }

    /// `iicprobe`: reads register `cmd` once (0xff, which is also the failure value, is read
    /// again).
    pub fn iicprobe(&mut self, cmd: u8) -> u8 {
        if self.probe_val[usize::from(cmd)] != 0xff {
            return self.probe_val[usize::from(cmd)];
        }
        let v = self.iicprobenc(cmd);
        self.probe_val[usize::from(cmd)] = v;
        v
    }
}

const LM75TEMP: u8 = 0x00;
const LM75CONF: u8 = 0x01;
const LM75THYST: u8 = 0x02;
const LM75TOS: u8 = 0x03;
const LM77TLOW: u8 = 0x04;
const LM77THIGH: u8 = 0x05;
/// `LM75TMASK`: 9 bits in temperature registers.
const LM75TMASK: u16 = 0xff80;
/// `LM77TMASK`: 13 bits in temperature registers.
const LM77TMASK: u16 = 0xfff8;

/// `lm75probe`: the LM75/LM75A/LM77 family are very hard to detect. Thus, we check for all
/// other possible chips first. These chips do not have an ID register. They do have a few
/// quirks though:
/// -  on the LM75 and LM77, registers 0x06 and 0x07 return whatever value was read before
/// -  the LM75 lacks registers 0x04 and 0x05, so those act as above
/// -  the LM75A returns 0xffff for registers 0x04, 0x05, 0x06 and 0x07
/// -  the chip registers loop every 8 registers
///
/// The downside is that we must read almost every register to guess if this is an LM75, LM75A
/// or LM77.
pub fn lm75probe(p: &mut Probe) -> Option<&'static str> {
    let mut mask = LM75TMASK;

    let mut temp = p.iicprobew(LM75TEMP);

    // Sometimes the other probes can upset the chip, if we get 0xffff the first time, try it
    // once more.
    if temp == 0xffff {
        temp = p.iicprobew(LM75TEMP);
    }

    let conf = p.iicprobenc(LM75CONF);
    let mut thyst = p.iicprobew(LM75THYST);
    let mut tos = p.iicprobew(LM75TOS);

    // totally bogus data
    if conf == 0xff && temp == 0xffff && thyst == 0xffff {
        return None;
    }

    temp &= mask;
    thyst &= mask;
    tos &= mask;

    // All values the same? Very unlikely
    if temp == thyst && thyst == tos {
        return None;
    }

    // (The C's `#if notsure` check of the aliasing of the temperature and configuration
    // registers is not configured.)

    // LM77/LM75 registers 6, 7 echo whatever was read just before them from reg 0, 1, or 2
    //
    // LM75A doesn't appear to do this, but does appear to reliably return 0xffff
    let mut echocount = 2;
    let mut ffffcount = 0;
    for i in 6u8..=7 {
        if (p.iicprobew(LM75TEMP) & mask) != (p.iicprobew(i) & mask)
            || (p.iicprobew(LM75THYST) & mask) != (p.iicprobew(i) & mask)
            || (p.iicprobew(LM75TOS) & mask) != (p.iicprobew(i) & mask)
        {
            echocount -= 1;
        }
        if p.iicprobew(i) == 0xffff {
            ffffcount += 1;
        }
    }

    // Make sure either both registers echo, or neither does
    if echocount == 1 || ffffcount == 1 {
        return None;
    }

    let echoreg67 = i32::from(echocount != 0);
    let ffffreg67 = i32::from(ffffcount != 0);

    // LM75 has no registers 4 or 5, and they will act as echos too
    //
    // LM75A doesn't appear to do this either, but does appear to reliably return 0xffff
    let mut echocount = 2;
    let mut ffffcount = 0;
    for i in 4u8..=5 {
        if (p.iicprobew(LM75TEMP) & mask) != (p.iicprobew(i) & mask)
            || (p.iicprobew(LM75THYST) & mask) != (p.iicprobew(i) & mask)
            || (p.iicprobew(LM75TOS) & mask) != (p.iicprobew(i) & mask)
        {
            echocount -= 1;
        }
        if p.iicprobew(i) == 0xffff {
            ffffcount += 1;
        }
    }

    // Make sure either both registers echo, or neither does
    if echocount == 1 || ffffcount == 1 {
        return None;
    }

    let echoreg45 = i32::from(echocount != 0);
    let ffffreg45 = i32::from(ffffcount != 0);

    // If we find that 4 and 5 are not echos, and don't return 0xffff then based on whether the
    // echo test of registers 6 and 7 succeeded or not, we may have an LM77
    let mut tlow = 0;
    let mut thigh = 0;
    if echoreg45 == 0 && ffffreg45 == 0 && echoreg67 == 1 {
        mask = LM77TMASK;

        // mask size changed, must re-read for the next checks
        thyst = p.iicprobew(LM75THYST) & mask;
        tos = p.iicprobew(LM75TOS) & mask;
        tlow = p.iicprobew(LM77TLOW) & mask;
        thigh = p.iicprobew(LM77THIGH) & mask;
    }

    // a real LM75/LM75A/LM77 repeats its registers....
    for i in (0x08u8..=0xf8).step_by(8) {
        if conf != p.iicprobenc(LM75CONF + i)
            || thyst != (p.iicprobew(LM75THYST + i) & mask)
            || tos != (p.iicprobew(LM75TOS + i) & mask)
        {
            return None;
        }

        // Check that the repeated registers 0x06 and 0x07 still either echo or return 0xffff
        if echoreg67 == 1 {
            tos = p.iicprobew(LM75TOS) & mask;
            if tos != (p.iicprobew(0x06 + i) & mask) || tos != (p.iicprobew(0x07 + i) & mask) {
                return None;
            }
        } else if ffffreg67 == 1
            && (p.iicprobew(0x06 + i) != 0xffff || p.iicprobew(0x07 + i) != 0xffff)
        {
            return None;
        }

        // Check that the repeated registers 0x04 and 0x05 still either echo or return 0xffff.
        // If they do neither, and registers 0x06 and 0x07 echo, then we will be probing for an
        // LM77, so make sure those still repeat
        if echoreg45 == 1 {
            tos = p.iicprobew(LM75TOS) & mask;
            if tos != (p.iicprobew(LM77TLOW + i) & mask)
                || tos != (p.iicprobew(LM77THIGH + i) & mask)
            {
                return None;
            }
        } else if ffffreg45 == 1 {
            if p.iicprobew(LM77TLOW + i) != 0xffff || p.iicprobew(LM77THIGH + i) != 0xffff {
                return None;
            }
        } else if echoreg67 == 1
            && (tlow != (p.iicprobew(LM77TLOW + i) & mask)
                || thigh != (p.iicprobew(LM77THIGH + i) & mask))
        {
            return None;
        }
    }

    // Given that we now know how the first eight registers behave and that this behaviour is
    // consistently repeated, we can now use the following table:
    //
    // echoreg67 | echoreg45 | ffffreg67 | ffffreg45 | chip
    // ----------+-----------+-----------+-----------+------
    //     1     |     1     |     0     |     0     | LM75
    //     1     |     0     |     0     |     0     | LM77
    //     0     |     0     |     1     |     1     | LM75A

    // Convert the various flags into a single score
    let score = (echoreg67 << 3) + (echoreg45 << 2) + (ffffreg67 << 1) + ffffreg45;

    match score {
        12 => Some("lm75"),
        8 => Some("lm77"),
        3 => Some("lm75a"),
        _ => None,
    }
}

/// `adm1032cloneprobe`.
pub fn adm1032cloneprobe(p: &mut Probe, addr: u8) -> Option<&'static str> {
    if addr == 0x18 || addr == 0x1a || addr == 0x29 || addr == 0x2b || addr == 0x4c || addr == 0x4e
    {
        let mut zero = 0;
        let mut copy = 0;

        let val = p.iicprobe(0x00);
        for reg in 0x00u8..0x09 {
            if p.iicprobe(reg) == 0xff {
                return None;
            }
            if p.iicprobe(reg) == 0x00 {
                zero += 1;
            }
            if val == p.iicprobe(reg) {
                copy += 1;
            }
        }
        if zero > 6 || copy > 6 {
            return None;
        }
        let val = p.iicprobe(0x09);
        for reg in 0x0au8..0xfc {
            if p.iicprobe(reg) != val {
                return None;
            }
        }
        // 0xfe may be Maxim, or some other vendor
        if p.iicprobe(0xfe) == 0x4d {
            return Some("max1617");
        }
        // "xeontemp" is the name we choose for clone chips which have all sorts of buggy bus
        // interactions, such as those we just probed. Why? Intel is partly to blame for this
        // situation.
        return Some("xeontemp");
    }
    None
}

/// `iic_ignore_addr`: a driver that claims an address while the scan runs asks the scan to
/// leave it alone from now on.
pub fn iic_ignore_addr(addr: u8) {
    for slot in &IGNORE_ADDRS {
        if slot
            .compare_exchange(0, addr, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            return;
        }
    }
}

/// `iic_dump` (`I2C_VERBOSE`): prints the registers of a sensor nobody claimed.
pub fn iic_dump(dv: &Device, p: &mut Probe, name: Option<&str>) {
    let mut iicvalcnt = [0u8; 256];
    let mut cnt = 0;

    // Don't bother printing the most often repeated register value, since it is often weird
    // devices that respond incorrectly, busted controller driver, or in the worst case, it in
    // mosts cases, the value 0xff.
    let first = p.iicprobe(0);
    // (`u_int8_t` counters: 256 equal registers wrap to 0, as in C.)
    iicvalcnt[usize::from(first)] = iicvalcnt[usize::from(first)].wrapping_add(1);
    for i in 1..=0xffu8 {
        let val2 = p.iicprobe(i);
        iicvalcnt[usize::from(val2)] = iicvalcnt[usize::from(val2)].wrapping_add(1);
        if first == val2 {
            cnt += 1;
        }
    }

    let mut val = 0u8;
    let mut max = 0u8;
    for i in 0..=0xffu8 {
        if max < iicvalcnt[usize::from(i)] {
            max = iicvalcnt[usize::from(i)];
            val = i;
        }
    }

    if cnt == 255 {
        return;
    }

    printf(format_args!("{}: addr 0x{:x}", dv.xname(), p.probe_addr));
    for i in 0..=0xffu8 {
        let v = p.iicprobe(i);
        if v != val {
            printf(format_args!(" {i:02x}={v:02x}"));
        }
    }
    printf(format_args!(" words"));
    for i in 0..8u8 {
        let w = p.iicprobew(i);
        printf(format_args!(" {i:02x}={w:04x}"));
    }
    if let Some(name) = name {
        printf(format_args!(": {name}"));
    }
    printf(format_args!("\n"));
}

/// `iic_probe_sensor`: names the sensor chip at the probe's address, if the registers say
/// which it is.
pub fn iic_probe_sensor(p: &mut Probe) -> Option<&'static str> {
    let addr = p.probe_addr;
    let mut name: Option<&'static str> = None;

    p.skip_fc = false;

    // `iicprobe(reg)` and `iicprobew(reg)` of the C, as `r!` and `w!`.
    macro_rules! r {
        ($reg:expr) => {
            p.iicprobe($reg)
        };
    }
    macro_rules! w {
        ($reg:expr) => {
            p.iicprobew($reg)
        };
    }

    // Many I2C/SMBus devices use register 0x3e as a vendor ID register.
    match r!(0x3e) {
        0x01 => {
            // National Semiconductor
            //
            // Some newer National products use a vendor code at 0x3e of 0x01, and then 0x3f
            // contains a product code. But some older products are missing a product code, and
            // contain who knows what in that register. We assume that some employee was smart
            // enough to keep the numbers unique.
            if (addr == 0x2c || addr == 0x2d || addr == 0x2e)
                && (r!(0x3f) == 0x73 || r!(0x3f) == 0x72)
                && r!(0x00) == 0x00
            {
                name = Some("lm93"); // product 0x72 is the prototype
            } else if (addr == 0x2c || addr == 0x2d || addr == 0x2e) && r!(0x3f) == 0x68 {
                name = Some("lm96000"); // adt7460 compat?
            } else if (addr == 0x2c || addr == 0x2d || addr == 0x2e)
                && (r!(0x3f) == 0x60 || r!(0x3f) == 0x62)
            {
                name = Some("lm85"); // lm85C/B == adt7460 compat
            } else if (addr & 0x7c) == 0x2c // addr 0b01011xx
                && r!(0x48) == addr
                && (r!(0x3f) == 0x03 || r!(0x3f) == 0x04)
                && (r!(0x40) & 0x80) == 0x00
            {
                name = Some("lm81");
            }
        }
        0x02 => {
            // National Semiconductor?
            if (r!(0x3f) & 0xfc) == 0x04 {
                name = Some("lm87"); // complete check
            }
        }
        0x23 => {
            // Analog Devices?
            if r!(0x48) == addr && (r!(0x40) & 0x80) == 0x00 && (addr & 0x7c) == 0x2c {
                name = Some("adm9240"); // lm87 clone
            }
        }
        0x41 => {
            // Analog Devices
            //
            // Newer chips have a valid 0x3d product number, while older ones sometimes
            // encoded the product into the upper half of the "step register" at 0x3f.
            if (addr == 0x2c || addr == 0x2e || addr == 0x2f) && r!(0x3d) == 0x70 {
                name = Some("adt7470");
            } else if (addr == 0x2c || addr == 0x2d || addr == 0x2e) && r!(0x3d) == 0x76 {
                name = Some("adt7476"); // or adt7476a
            } else if addr == 0x2e && r!(0x3d) == 0x75 {
                name = Some("adt7475");
            } else if r!(0x3d) == 0x27 && (r!(0x3f) == 0x60 || r!(0x3f) == 0x6a) {
                name = Some("adm1027"); // or adt7463
            } else if r!(0x3d) == 0x27 && (r!(0x3f) == 0x62 || r!(0x3f) == 0x6a) {
                name = Some("adt7460"); // complete check
            } else if (addr == 0x2c || addr == 0x2e) && r!(0x3d) == 0x62 && r!(0x3f) == 0x04 {
                name = Some("adt7462");
            } else if addr == 0x4c && r!(0x3d) == 0x66 && r!(0x3f) == 0x02 {
                name = Some("adt7466");
            } else if addr == 0x2e && r!(0x3d) == 0x68 && (r!(0x3f) & 0xf0) == 0x70 {
                name = Some("adt7467"); // or adt7468
            } else if r!(0x3d) == 0x33 && r!(0x3f) == 0x02 {
                name = Some("adm1033");
            } else if r!(0x3d) == 0x34 && r!(0x3f) == 0x02 {
                name = Some("adm1034");
            } else if (addr == 0x2c || addr == 0x2d || addr == 0x2e)
                && r!(0x3d) == 0x30
                && (r!(0x01) & 0x80) == 0x00
                && (r!(0x0d) & 0x70) == 0x00
                && (r!(0x0e) & 0x70) == 0x00
            {
                // Revision 3 seems to be an adm1031 with remote diode 2 shorted. Therefore we
                // cannot assume the reserved/unused bits of register 0x03 and 0x06 are set to
                // zero.
                name = Some("adm1030"); // complete check
            } else if (addr == 0x2c || addr == 0x2d || addr == 0x2e)
                && r!(0x3d) == 0x31
                && (r!(0x01) & 0x80) == 0x00
                && (r!(0x0d) & 0x70) == 0x00
                && (r!(0x0e) & 0x70) == 0x00
                && (r!(0x0f) & 0x70) == 0x00
            {
                name = Some("adm1031"); // complete check
            } else if (addr & 0x7c) == 0x2c // addr 0b01011xx
                && (r!(0x3f) & 0xf0) == 0x20
                && (r!(0x40) & 0x80) == 0x00
                && (r!(0x41) & 0xc0) == 0x00
                && (r!(0x42) & 0xbc) == 0x00
            {
                name = Some("adm1025"); // complete check
            } else if (addr & 0x7c) == 0x2c // addr 0b01011xx
                && (r!(0x3f) & 0xf0) == 0x10
                && (r!(0x40) & 0x80) == 0x00
            {
                name = Some("adm1024"); // complete check
            } else if (r!(0xff) & 0xf0) == 0x30 {
                name = Some("adm1023");
            } else if addr == 0x2e && (r!(0x3f) & 0xf0) == 0xd0 && (r!(0x40) & 0x80) == 0x00 {
                name = Some("adm1028"); // adm1022 clone?
            } else if (addr == 0x2c || addr == 0x2e || addr == 0x2f)
                && (r!(0x3f) & 0xf0) == 0xc0
                && (r!(0x40) & 0x80) == 0x00
            {
                name = Some("adm1022");
            }
        }
        0x49 => {
            // Texas Instruments
            if (addr == 0x2c || addr == 0x2e || addr == 0x2f)
                && (r!(0x3f) & 0xf0) == 0xc0
                && (r!(0x40) & 0x80) == 0x00
            {
                name = Some("thmc50"); // adm1022 clone
            }
        }
        0x55 => {
            // SMSC
            if (addr & 0x7c) == 0x2c // addr 0b01011xx
                && r!(0x3f) == 0x20
                && (r!(0x47) & 0x70) == 0x00
                && (r!(0x49) & 0xfe) == 0x80
            {
                name = Some("47m192"); // adm1025 compat
            }
        }
        0x5c => {
            // SMSC
            if (addr == 0x2c || addr == 0x2d || addr == 0x2e) && r!(0x3f) == 0x69 {
                name = Some("sch5027");
            } else if (addr == 0x2c || addr == 0x2d || addr == 0x2e) && (r!(0x3f) & 0xf0) == 0x60 {
                name = Some("emc6d100"); // emc6d101, emc6d102, emc6d103
            } else if (addr == 0x2c || addr == 0x2d || addr == 0x2e) && (r!(0x3f) & 0xf0) == 0x80 {
                name = Some("sch5017");
            } else if (addr == 0x2c || addr == 0x2d || addr == 0x2e) && (r!(0x3f) & 0xf0) == 0xb0 {
                name = Some("emc6w201");
            }
        }
        0x61 => {
            // Andigilog
            if (addr == 0x2c || addr == 0x2d || addr == 0x2e)
                && r!(0x3f) == 0x69
                && r!(0x22) >= 0xaf // Vdd
                && (r!(0x09) & 0xbf) == 0x00
                && r!(0x0f) == 0x00
                && (r!(0x40) & 0xf0) == 0x00
            {
                name = Some("asc7611");
            } else if (addr == 0x2c || addr == 0x2d || addr == 0x2e)
                && r!(0x3f) == 0x6c
                && r!(0x22) >= 0xae
            // Vdd
            {
                name = Some("asc7621");
            }
        }
        0xa1 => {
            // Philips
            if (r!(0x3f) & 0xf0) == 0x20
                && (r!(0x40) & 0x80) == 0x00
                && (r!(0x41) & 0xc0) == 0x00
                && (r!(0x42) & 0xbc) == 0x00
            {
                name = Some("ne1619"); // adm1025 compat
            }
        }
        // Dallas Semiconductor
        0xda if r!(0x3f) == 0x01 && r!(0x48) == addr && (r!(0x40) & 0x80) == 0x00 => {
            name = Some("ds1780"); // lm87 clones
        }
        _ => {}
    }

    if r!(0x4e) == 0x41 {
        // Analog Devices
        if (addr == 0x48 || addr == 0x4a || addr == 0x4b)
            && (r!(0x4d) == 0x03 || r!(0x4d) == 0x08 || r!(0x4d) == 0x07)
        {
            name = Some("adt7516"); // adt7517, adt7519
        }
    }

    match r!(0xfe) {
        0x01 => {
            // National Semiconductor
            if addr == 0x4c
                && r!(0xff) == 0x41
                && (r!(0x03) & 0x18) == 0
                && r!(0x04) <= 0x0f
                && (r!(0xbf) & 0xf8) == 0
            {
                name = Some("lm63");
            } else if addr == 0x4c
                && r!(0xff) == 0x11
                && (r!(0x03) & 0x2a) == 0
                && r!(0x04) <= 0x09
                && (r!(0xbf) & 0xf8) == 0
            {
                name = Some("lm86");
            } else if addr == 0x4c
                && r!(0xff) == 0x31
                && (r!(0x03) & 0x2a) == 0
                && r!(0x04) <= 0x09
                && (r!(0xbf) & 0xf8) == 0
            {
                name = Some("lm89"); // or lm99
            } else if addr == 0x4d
                && r!(0xff) == 0x34
                && (r!(0x03) & 0x2a) == 0
                && r!(0x04) <= 0x09
                && (r!(0xbf) & 0xf8) == 0
            {
                name = Some("lm89-1"); // or lm99-1
            } else if addr == 0x4c
                && r!(0xff) == 0x21
                && (r!(0x03) & 0x2a) == 0
                && r!(0x04) <= 0x09
                && (r!(0xbf) & 0xf8) == 0
            {
                name = Some("lm90");
            }
        }
        0x23 => {
            // Genesys Logic?
            if addr == 0x4c && (r!(0x03) & 0x3f) == 0x00 && r!(0x04) <= 0x08 {
                // Genesys Logic doesn't make the datasheet for the GL523SM publicly
                // available, so the checks above are nothing more than a (conservative)
                // educated guess.
                name = Some("gl523sm");
            }
        }
        0x41 => {
            // Analog Devices
            if (addr == 0x4c || addr == 0x4d)
                && r!(0xff) == 0x51
                && (r!(0x03) & 0x1f) == 0x04
                && r!(0x04) <= 0x0a
            {
                // If not in adm1032 compatibility mode.
                name = Some("adt7461");
            } else if (addr == 0x18
                || addr == 0x19
                || addr == 0x1a
                || addr == 0x29
                || addr == 0x2a
                || addr == 0x2b
                || addr == 0x4c
                || addr == 0x4d
                || addr == 0x4e)
                && (r!(0xff) & 0xf0) == 0x00
                && (r!(0x03) & 0x3f) == 0x00
                && r!(0x04) <= 0x07
            {
                name = Some("adm1021");
                p.skip_fc = true;
            } else if (addr == 0x18
                || addr == 0x19
                || addr == 0x1a
                || addr == 0x29
                || addr == 0x2a
                || addr == 0x2b
                || addr == 0x4c
                || addr == 0x4d
                || addr == 0x4e)
                && (r!(0xff) & 0xf0) == 0x30
                && (r!(0x03) & 0x3f) == 0x00
                && r!(0x04) <= 0x07
            {
                name = Some("adm1023"); // or adm1021a
                p.skip_fc = true;
            } else if (addr == 0x4c || addr == 0x4d || addr == 0x4e)
                && (r!(0x03) & 0x3f) == 0x00
                && r!(0x04) <= 0x0a
            {
                name = Some("adm1032"); // or adm1020
                p.skip_fc = true;
            }
        }
        0x47 => {
            // Global Mixed-mode Technology
            if addr == 0x4c && r!(0xff) == 0x01 && (r!(0x03) & 0x3f) == 0x00 && r!(0x04) <= 0x08 {
                name = Some("g781");
            }
            if addr == 0x4d && r!(0xff) == 0x03 && (r!(0x03) & 0x3f) == 0x00 && r!(0x04) <= 0x08 {
                name = Some("g781-1");
            }
        }
        0x4d => {
            // Maxim
            if (addr == 0x18
                || addr == 0x19
                || addr == 0x1a
                || addr == 0x29
                || addr == 0x2a
                || addr == 0x2b
                || addr == 0x4c
                || addr == 0x4d
                || addr == 0x4e)
                && r!(0xff) == 0x08
                && (r!(0x02) & 0x03) == 0
                && (r!(0x03) & 0x07) == 0
                && r!(0x04) <= 0x08
            {
                name = Some("max6690");
            } else if (addr == 0x4c || addr == 0x4d || addr == 0x4e)
                && r!(0xff) == 0x59
                && (r!(0x03) & 0x1f) == 0
                && r!(0x04) <= 0x07
            {
                name = Some("max6646"); // max6647/8/9, max6692
            } else if (addr == 0x4c || addr == 0x4d || addr == 0x4e)
                && (r!(0x02) & 0x2b) == 0
                && (r!(0x03) & 0x0f) == 0
                && r!(0x04) <= 0x09
            {
                name = Some("max6657"); // max6658, max6659
                p.skip_fc = true;
            } else if (0x48..=0x4f).contains(&addr)
                && (r!(0x02) & 0x2b) == 0
                && (r!(0x03) & 0x0f) == 0
            {
                name = Some("max6642");
            }
        }
        0x55 => {
            // Texas Instruments
            if addr == 0x4c
                && r!(0xff) == 0x11
                && (r!(0x03) & 0x1b) == 0x00
                && (r!(0x04) & 0xf0) == 0x00
                && (r!(0x10) & 0x0f) == 0x00
                && (r!(0x13) & 0x0f) == 0x00
                && (r!(0x14) & 0x0f) == 0x00
                && (r!(0x15) & 0x0f) == 0x00
                && (r!(0x16) & 0x0f) == 0x00
                && (r!(0x17) & 0x0f) == 0x00
            {
                name = Some("tmp401");
            }
        }
        0xa1 if (0x48..=0x4f).contains(&addr)
            && r!(0xff) == 0x00
            && (r!(0x03) & 0xf8) == 0x00
            && r!(0x04) <= 0x09 =>
        {
            name = Some("sa56004x"); // NXP sa56004x
            p.skip_fc = true;
        }
        _ => {}
    }

    // `addr * 2` of the C is an `int`.
    let addr2 = i32::from(addr) * 2;

    if addr == r!(0x48)
        && ((r!(0x4f) == 0x5c && (r!(0x4e) & 0x80) != 0)
            || (r!(0x4f) == 0xa3 && (r!(0x4e) & 0x80) == 0))
    {
        // We could toggle 0x4e bit 0x80, then re-read 0x4f to see if the value changes to 0xa3
        // (indicating Winbond). But we are trying to avoid writes.
        if (r!(0x4e) & 0x07) == 0 {
            match r!(0x58) {
                0x10 | 0x11 => name = Some("w83781d"), // rev 2?
                0x21 => name = Some("w83627hf"),
                0x30 => name = Some("w83782d"),
                0x31 => name = Some("as99127f"), // rev 2
                0x40 => name = Some("w83783s"),
                0x71 => name = Some("w83791d"),
                0x72 => name = Some("w83791sd"),
                0x7a => name = Some("w83792d"),
                0xc1 => name = Some("w83627dhg"),
                _ => {}
            }
        } else {
            // The BIOS left the chip in a non-zero register bank. Assume it's a W83781D and let
            // lm(4) sort out the real model.
            name = Some("w83781d");
        }
    } else if addr == (r!(0xfc) & 0x7f)
        && r!(0xfe) == 0x79
        && r!(0xfb) == 0x51
        && ((r!(0xfd) == 0x5c && (r!(0x00) & 0x80) != 0)
            || (r!(0xfd) == 0xa3 && (r!(0x00) & 0x80) == 0))
    {
        // We could toggle 0x00 bit 0x80, then re-read 0xfd to see if the value changes to 0xa3
        // (indicating Nuvoton). But we are trying to avoid writes.
        name = Some("w83795g");
    } else if addr == r!(0x4a) && r!(0x4e) == 0x50 && r!(0x4c) == 0xa3 && r!(0x4d) == 0x5c {
        name = Some("w83l784r");
    } else if addr == 0x2d && r!(0x4e) == 0x60 && r!(0x4c) == 0xa3 && r!(0x4d) == 0x5c {
        name = Some("w83l785r");
    } else if addr == 0x2e && r!(0x4e) == 0x70 && r!(0x4c) == 0xa3 && r!(0x4d) == 0x5c {
        name = Some("w83l785ts-l");
    } else if (0x2c..=0x2f).contains(&addr)
        && ((r!(0x00) & 0x07) != 0x0
            || ((r!(0x00) & 0x07) == 0x0
                && addr2 == i32::from(r!(0x0b))
                && (r!(0x0c) & 0x40) != 0
                && (r!(0x0c) & 0x04) == 0))
        && r!(0x0e) == 0x7b
        && (r!(0x0f) & 0xf0) == 0x10
        && ((r!(0x0d) == 0x5c && (r!(0x00) & 0x80) != 0)
            || (r!(0x0d) == 0xa3 && (r!(0x00) & 0x80) == 0))
    {
        name = Some("w83793g");
    } else if (0x28..=0x2f).contains(&addr) && r!(0x4f) == 0x12 && (r!(0x4e) & 0x80) != 0 {
        // We could toggle 0x4e bit 0x80, then re-read 0x4f to see if the value changes to 0xc3
        // (indicating ASUS). But we are trying to avoid writes.
        if r!(0x58) == 0x31 {
            name = Some("as99127f"); // rev 1
        }
    } else if (addr == 0x2d || addr == 0x2e)
        && addr2 == i32::from(r!(0x04))
        && r!(0x5d) == 0x19
        && r!(0x5e) == 0x34
        && r!(0x5a) == 0x03
        && r!(0x5b) == 0x06
    {
        name = Some("f75375"); // Fintek
    } else if addr == 0x2d
        && ((r!(0x4f) == 0x06 && (r!(0x4e) & 0x80) != 0)
            || (r!(0x4f) == 0x94 && (r!(0x4e) & 0x80) == 0))
    {
        // We could toggle 0x4e bit 0x80, then re-read 0x4f to see if the value changes to 0x94
        // (indicating ASUS). But we are trying to avoid writes.
        //
        // NB. we won't match if the BIOS has selected a non-zero register bank (set via 0x4e).
        // We could select bank 0 so we see the right registers, but that would require a write.
        // In general though, we bet no BIOS would leave us in the wrong state.
        if (r!(0x58) & 0x7f) == 0x31 && (r!(0x4e) & 0xf) == 0x00 {
            name = Some("asb100");
        }
    } else if (addr == 0x2c || addr == 0x2d)
        && r!(0x00) == 0x80
        && (r!(0x01) == 0x00 || r!(0x01) == 0x80)
        && r!(0x02) == 0x00
        && (r!(0x03) & 0x83) == 0x00
        && (r!(0x0f) & 0x07) == 0x00
        && (r!(0x11) & 0x80) == 0x00
        && (r!(0x12) & 0x80) == 0x00
    {
        // The GL518SM is really crappy. It has both byte and word registers, and reading a word
        // register with a byte read command will make the device crap out and hang the bus.
        // This has nasty consequences on some machines, like preventing warm reboots. The word
        // registers are 0x07 through 0x0c, so make sure the checks above don't access those
        // registers. We don't want to do this check right up front though since this chip is
        // somewhat hard to detect (which is why we check for every single fixed bit it has).
        name = Some("gl518sm");
    } else if (addr == 0x2c || addr == 0x2d || addr == 0x2e)
        && r!(0x16) == 0x41
        && (r!(0x17) & 0xf0) == 0x40
    {
        name = Some("adm1026");
    } else if (addr & 0x78) == 0x18 && w!(0x06) == 0x1131 && (w!(0x07) & 0xfffc) == 0xa200 {
        name = Some("se97"); // or se97b
    } else if (addr & 0x78) == 0x18
        && w!(0x06) == 0x1131
        && (w!(0x07) & 0xfffc) == 0xa100
        && (w!(0x00) & 0xfff0) == 0x0010
    {
        name = Some("se98");
    } else if (addr & 0x78) == 0x18
        && w!(0x06) == 0x004d
        && w!(0x07) == 0x3e00
        && (w!(0x00) & 0xffe0) == 0x0000
    {
        name = Some("max6604");
    } else if (addr & 0x78) == 0x18
        && w!(0x06) == 0x0054
        && (w!(0x07) & 0xfffc) == 0x0200
        && (w!(0x00) & 0xffe0) == 0x0000
    {
        name = Some("mcp9804");
    } else if (addr & 0x78) == 0x18
        && w!(0x06) == 0x0054
        && (w!(0x07) & 0xff00) == 0x0000
        && (w!(0x00) & 0xffe0) == 0x0000
    {
        name = Some("mcp9805"); // or mcp9843
    } else if (addr & 0x78) == 0x18
        && w!(0x06) == 0x0054
        && (w!(0x07) & 0xfffc) == 0x2000
        && (w!(0x00) & 0xffe0) == 0x0000
    {
        name = Some("mcp98242");
    } else if (addr & 0x78) == 0x18
        && w!(0x06) == 0x0054
        && (w!(0x07) & 0xff00) == 0x2100
        && (w!(0x00) & 0xff00) == 0x0000
    {
        name = Some("mcp98243");
    } else if (addr & 0x78) == 0x18
        && w!(0x06) == 0x0054
        && (w!(0x07) & 0xfffc) == 0x2200
        && (w!(0x00) & 0xff00) == 0x0000
    {
        name = Some("mcp98244");
    } else if (addr & 0x78) == 0x18
        && w!(0x06) == 0x11d4
        && w!(0x07) == 0x0800
        && w!(0x00) == 0x001d
    {
        name = Some("adt7408");
    } else if (addr & 0x78) == 0x18
        && w!(0x06) == 0x104a
        && (w!(0x07) & 0xfffe) == 0x0000
        && (w!(0x00) == 0x002d || w!(0x00) == 0x002f)
    {
        name = Some("stts424e02");
    } else if (addr & 0x78) == 0x18
        && w!(0x06) == 0x104a
        && (w!(0x07) & 0xfffe) == 0x0300
        && w!(0x00) == 0x006f
    {
        name = Some("stts2002");
    } else if (addr & 0x78) == 0x18
        && w!(0x06) == 0x104a
        && w!(0x07) == 0x2201
        && w!(0x00) == 0x00ef
    {
        name = Some("stts2004");
    } else if (addr & 0x78) == 0x18
        && w!(0x06) == 0x104a
        && w!(0x07) == 0x0200
        && w!(0x00) == 0x006f
    {
        name = Some("stts3000");
    } else if (addr & 0x78) == 0x18
        && w!(0x06) == 0x104a
        && w!(0x07) == 0x0101
        && (w!(0x00) == 0x002d || w!(0x00) == 0x002f)
    {
        name = Some("stts424");
    } else if (addr & 0x78) == 0x18
        && w!(0x06) == 0x1b09
        && (w!(0x07) & 0xffe0) == 0x0800
        && (w!(0x00) & 0x001f) == 0x001f
    {
        name = Some("cat34ts02"); // or cat6095, prod 0x0813
    } else if (addr & 0x78) == 0x18
        && w!(0x06) == 0x1b09
        && w!(0x07) == 0x0a00
        && (w!(0x00) & 0x001f) == 0x001f
    {
        name = Some("cat34ts02c");
    } else if (addr & 0x78) == 0x18
        && w!(0x06) == 0x1b09
        && w!(0x07) == 0x2200
        && w!(0x00) == 0x007f
    {
        name = Some("cat34ts04");
    } else if (addr & 0x78) == 0x18
        && w!(0x06) == 0x00b3
        && w!(0x07) == 0x2903
        && w!(0x00) == 0x004f
    {
        name = Some("ts3000b3"); // or tse2002b3
    } else if (addr & 0x78) == 0x18
        && w!(0x06) == 0x00b3
        && w!(0x07) == 0x2912
        && w!(0x00) == 0x006f
    {
        name = Some("ts3000gb2"); // or tse2002gb2
    } else if (addr & 0x78) == 0x18
        && w!(0x06) == 0x00b3
        && w!(0x07) == 0x2913
        && w!(0x00) == 0x0077
    {
        name = Some("ts3000gb0");
    } else if (addr & 0x78) == 0x18
        && w!(0x06) == 0x00b3
        && w!(0x07) == 0x3001
        && w!(0x00) == 0x006f
    {
        name = Some("ts3001gb2");
    } else if (addr & 0x78) == 0x18
        && w!(0x06) == 0x00b3
        && w!(0x07) == 0x2214
        && w!(0x00) == 0x00ff
    {
        name = Some("tse2004gb2");
    } else if (addr & 0x78) == 0x18
        && w!(0x06) == 0x001f
        && w!(0x07) == 0x8201
        && (w!(0x00) & 0xff00) == 0x0000
    {
        name = Some("at30ts00"); // or at30tse002
    } else if (addr & 0x78) == 0x18
        && w!(0x06) == 0x1114
        && w!(0x07) == 0x2200
        && (w!(0x00) & 0xff00) == 0x0000
    {
        name = Some("at30tse004");
    } else if (addr & 0x78) == 0x18
        && w!(0x06) == 0x1c68
        && w!(0x07) == 0x2201
        && (w!(0x00) & 0xff00) == 0x0000
    {
        name = Some("gt30ts00");
    } else if (addr & 0x78) == 0x18
        && w!(0x06) == 0x132d
        && w!(0x07) == 0x3300
        && (w!(0x00) & 0x001f) == 0x001f
    {
        name = Some("gt34ts02");
    } else if (addr & 0x7e) == 0x1c
        && r!(0x0f) == 0x3b
        && (r!(0x21) & 0x60) == 0x00
        && r!(0x0f) == r!(0x8f) // registers address is 7 bits
        && r!(0x20) == r!(0xa0)
        && r!(0x21) == r!(0xa1)
        && r!(0x22) == r!(0xa2)
        && r!(0x07) == 0x00
    // 0x00 to 0x0e are reserved
    {
        name = Some("lis331dl");
    } else if name.is_none() && (addr & 0x78) == 0x48 {
        // addr 0b1001xxx
        name = lm75probe(p);
    }

    // (The C's `#if 0` ds1624/ds1631/ds1721 probe stays out: "This probe needs to be improved;
    // the driver does some dangerous writes.")

    if name.is_none()
        && (addr & 0xf8) == 0x28
        && r!(0x48) == addr
        && (r!(0x00) & 0x90) == 0x10
        && r!(0x58) == 0x90
    {
        if r!(0x5b) == 0x12 {
            name = Some("it8712");
        } else if r!(0x5b) == 0x00 {
            name = Some("it8712f-a"); // sis950 too
        }
    }

    if name.is_none() && r!(0x48) == addr && (r!(0x40) & 0x80) == 0x00 && r!(0x58) == 0xac {
        name = Some("mtp008");
    }

    if name.is_none() {
        name = adm1032cloneprobe(p, addr);
        if name.is_some() {
            p.skip_fc = true;
        }
    }

    name
}

/// `iic_probe_eeprom`: an EEPROM is anything whose register 2 holds a memory type SPD
/// knows; the drivers match further.
pub fn iic_probe_eeprom(p: &mut Probe) -> Option<&'static str> {
    let ty = p.iicprobe(0x02);
    // limit to SPD types seen in the wild
    if !(4..=16).contains(&ty) {
        return None;
    }

    // more matching in driver(s)
    Some("eeprom")
}

/// `iic_scan`: scans the bus for known device signatures and attaches what answers.
pub fn iic_scan(self_: &Device, iba: &I2cbusAttachArgs) {
    let ic = iba.iba_tag;
    let cmd = [0u8; 1];

    for slot in &IGNORE_ADDRS {
        slot.store(0, Ordering::Relaxed);
    }

    for set in &PROBES {
        if set.flags & PFLAG_SENSOR != 0 && IPMI_ENABLED.load(Ordering::Relaxed) != 0 {
            printf(format_args!(
                "{}: skipping sensors to avoid ipmi0 interactions\n",
                self_.xname()
            ));
            continue;
        }
        for pl in set.pl {
            for addr in pl.start..=pl.end {
                if IGNORE_ADDRS
                    .iter()
                    .any(|a| a.load(Ordering::Relaxed) == addr)
                {
                    continue;
                }

                // Perform RECEIVE BYTE command
                iic_acquire_bus(ic, 0);
                if iic_exec(
                    ic,
                    I2C_OP_READ_WITH_STOP,
                    I2cAddr::from(addr),
                    &cmd,
                    &mut [],
                    0,
                ) == 0
                {
                    iic_release_bus(ic, 0);

                    // Some device exists
                    let mut p = Probe::new(iba, addr);
                    let name = (set.probe)(&mut p);
                    if let Some(name) = name {
                        let mut ia = I2cAttachArgs::new(
                            iba.iba_tag,
                            I2cAddr::from(addr),
                            1,
                            name.as_bytes(),
                        );
                        if config_found(
                            self_,
                            ptr::from_mut(&mut ia).cast::<c_void>(),
                            Some(iic_print),
                        )
                        .is_some()
                        {
                            continue;
                        }
                    }
                    if set.flags & PFLAG_SENSOR != 0 {
                        iic_dump(self_, &mut p, name);
                    }
                } else {
                    iic_release_bus(ic, 0);
                }
            }
        }
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::dev::i2c::i2c_io::I2cOp;
    use crate::dev::i2c::i2cvar::I2cController;
    use core::cell::Cell;
    use std::boxed::Box;
    use std::vec::Vec;

    /// How the fake chip answers the registers it was not given.
    enum Mode {
        /// Byte registers from `regs`, words from `words` or the two bytes at `reg`, `reg + 1`.
        Plain,
        /// An LM75: registers 0..3 (temperature, configuration, Thyst, Tos) repeat every 8;
        /// 4..7 echo the last word read from 0, 2 or 3.
        Lm75 {
            conf: u8,
            temp: u16,
            thyst: u16,
            tos: u16,
            last: Cell<u16>,
        },
        /// An LM75A: as the LM75, but 4..7 read 0xffff.
        Lm75a {
            conf: u8,
            temp: u16,
            thyst: u16,
            tos: u16,
        },
    }

    struct FakeChip {
        addr: u8,
        regs: [u8; 256],
        words: Vec<(u8, u16)>,
        mode: Mode,
        reads: Cell<usize>,
    }

    impl FakeChip {
        fn plain(addr: u8, fill: u8, set: &[(u8, u8)]) -> FakeChip {
            let mut regs = [fill; 256];
            for &(r, v) in set {
                regs[usize::from(r)] = v;
            }
            FakeChip {
                addr,
                regs,
                words: Vec::new(),
                mode: Mode::Plain,
                reads: Cell::new(0),
            }
        }

        fn word(&self, reg: u8) -> u16 {
            match &self.mode {
                Mode::Plain => self
                    .words
                    .iter()
                    .find(|&&(r, _)| r == reg)
                    .map(|&(_, w)| w)
                    .unwrap_or_else(|| {
                        u16::from_be_bytes([
                            self.regs[usize::from(reg)],
                            self.regs[usize::from(reg.wrapping_add(1))],
                        ])
                    }),
                Mode::Lm75 {
                    conf,
                    temp,
                    thyst,
                    tos,
                    last,
                } => match reg & 7 {
                    0 => {
                        last.set(*temp);
                        *temp
                    }
                    1 => u16::from(*conf) << 8,
                    2 => {
                        last.set(*thyst);
                        *thyst
                    }
                    3 => {
                        last.set(*tos);
                        *tos
                    }
                    _ => last.get(),
                },
                Mode::Lm75a {
                    conf,
                    temp,
                    thyst,
                    tos,
                } => match reg & 7 {
                    0 => *temp,
                    1 => u16::from(*conf) << 8,
                    2 => *thyst,
                    3 => *tos,
                    _ => 0xffff,
                },
            }
        }

        fn byte(&self, reg: u8) -> u8 {
            match &self.mode {
                Mode::Plain => self.regs[usize::from(reg)],
                Mode::Lm75 { conf, .. } | Mode::Lm75a { conf, .. } if reg & 7 == 1 => *conf,
                _ => (self.word(reg) >> 8) as u8,
            }
        }
    }

    fn exec(
        cookie: *mut core::ffi::c_void,
        _op: I2cOp,
        addr: I2cAddr,
        cmd: &[u8],
        buf: &mut [u8],
        _flags: i32,
    ) -> i32 {
        // SAFETY: the tests pass a leaked `FakeChip` as the cookie.
        let chip = unsafe { &*cookie.cast::<FakeChip>() };
        if addr != I2cAddr::from(chip.addr) || cmd.len() != 1 {
            return 1;
        }
        chip.reads.set(chip.reads.get() + 1);
        match buf.len() {
            0 => {}
            1 => buf[0] = chip.byte(cmd[0]),
            2 => buf.copy_from_slice(&chip.word(cmd[0]).to_be_bytes()),
            _ => return 1,
        }
        0
    }

    fn bus(chip: FakeChip) -> (I2cTag, &'static FakeChip) {
        let chip: &'static FakeChip = Box::leak(Box::new(chip));
        let ic: &'static I2cController = Box::leak(Box::new(I2cController::new()));
        ic.ic_cookie.set(ptr::from_ref(chip).cast_mut().cast());
        ic.ic_exec.set(Some(exec));
        (ic, chip)
    }

    fn probe_of(chip: FakeChip) -> Probe {
        let addr = chip.addr;
        Probe::on(bus(chip).0, addr)
    }

    fn sensor(addr: u8, fill: u8, set: &[(u8, u8)]) -> Option<&'static str> {
        iic_probe_sensor(&mut probe_of(FakeChip::plain(addr, fill, set)))
    }

    #[test]
    fn the_address_lists_are_the_c_tables() {
        let ranges = |pl: &[IicProbeList]| pl.iter().map(|p| (p.start, p.end)).collect::<Vec<_>>();
        assert_eq!(
            ranges(&PROBE_ADDRS_SENSOR),
            [(0x18, 0x1f), (0x20, 0x2f), (0x48, 0x4e)]
        );
        assert_eq!(ranges(&PROBE_ADDRS_EEPROM), [(0x50, 0x57)]);
        assert_eq!(PROBES.len(), 2);
        assert_eq!(PROBES[0].flags, PFLAG_SENSOR);
        assert_eq!(PROBES[1].flags, 0);
    }

    #[test]
    fn a_register_read_that_fails_reads_as_ones() {
        let mut p = Probe::on(bus(FakeChip::plain(0x50, 0, &[])).0, 0x51);
        assert_eq!(p.iicprobenc(0x02), 0xff);
        assert_eq!(p.iicprobew(0x02), 0xffff);
    }

    #[test]
    fn words_come_big_endian() {
        let mut p = probe_of(FakeChip::plain(0x48, 0, &[(0x06, 0x11), (0x07, 0x31)]));
        assert_eq!(p.iicprobew(0x06), 0x1131);
    }

    #[test]
    fn registers_are_read_once_unless_they_read_as_ones() {
        let (ic, chip) = bus(FakeChip::plain(0x50, 0xff, &[(0x05, 0x42)]));
        let mut p = Probe::on(ic, 0x50);
        assert_eq!(p.iicprobe(0x05), 0x42);
        assert_eq!(p.iicprobe(0x05), 0x42);
        assert_eq!(chip.reads.get(), 1);
        // 0xff is the "not read" marker of the cache, so it is read again, as in C.
        assert_eq!(p.iicprobe(0x06), 0xff);
        assert_eq!(p.iicprobe(0x06), 0xff);
        assert_eq!(chip.reads.get(), 3);
    }

    #[test]
    fn register_0xfc_is_never_read_once_a_maxim_1617_is_suspected() {
        let (ic, chip) = bus(FakeChip::plain(0x4c, 0x11, &[]));
        let mut p = Probe::on(ic, 0x4c);
        p.skip_fc = true;
        assert_eq!(p.iicprobenc(0xfc), 0xff);
        assert_eq!(p.iicprobew(0xfc), 0xffff);
        assert_eq!(chip.reads.get(), 0);
        assert_eq!(p.iicprobenc(0xfb), 0x11);
        p.skip_fc = false;
        assert_eq!(p.iicprobenc(0xfc), 0x11);
    }

    #[test]
    fn eeprom_types_four_to_sixteen_are_eeproms() {
        for ty in 0..=0xffu8 {
            let got = iic_probe_eeprom(&mut probe_of(FakeChip::plain(0x50, 0xff, &[(0x02, ty)])));
            let want = (4..=16).contains(&ty).then_some("eeprom");
            assert_eq!(got, want, "type {ty:#x}");
        }
        // An SPD DDR2 (type 8) and a DDR4 (type 12): what QEMU's SPD EEPROMs hold.
        assert_eq!(
            iic_probe_eeprom(&mut probe_of(FakeChip::plain(0x51, 0, &[(0x02, 0x0c)]))),
            Some("eeprom")
        );
    }

    #[test]
    fn vendor_registers_name_the_sensors() {
        // National Semiconductor: LM93 at 0x2c..0x2e, product 0x73, register 0 zero.
        assert_eq!(
            sensor(0x2c, 0xff, &[(0x3e, 0x01), (0x3f, 0x73), (0x00, 0x00)]),
            Some("lm93")
        );
        // National Semiconductor: LM87, any address, product 0x04..0x07 in 0x3f.
        assert_eq!(
            sensor(0x2e, 0xff, &[(0x3e, 0x02), (0x3f, 0x05)]),
            Some("lm87")
        );
        // Analog Devices: ADT7460 (0x3d = 0x27, step 0x62) and ADM1027 (step 0x60).
        assert_eq!(
            sensor(0x2e, 0xff, &[(0x3e, 0x41), (0x3d, 0x27), (0x3f, 0x62)]),
            Some("adt7460")
        );
        assert_eq!(
            sensor(0x2e, 0xff, &[(0x3e, 0x41), (0x3d, 0x27), (0x3f, 0x60)]),
            Some("adm1027")
        );
        // The same vendor register at an address the chip cannot have.
        assert_eq!(sensor(0x2d, 0xff, &[(0x3e, 0x41), (0x3d, 0x70)]), None);
        assert_eq!(
            sensor(0x2c, 0xff, &[(0x3e, 0x41), (0x3d, 0x70)]),
            Some("adt7470")
        );
        // Winbond: register 0x48 holds the address, 0x4f the vendor, 0x58 the revision.
        assert_eq!(
            sensor(
                0x2d,
                0xff,
                &[(0x48, 0x2d), (0x4f, 0xa3), (0x4e, 0x00), (0x58, 0x10)]
            ),
            Some("w83781d")
        );
        assert_eq!(
            sensor(
                0x2d,
                0xff,
                &[(0x48, 0x2d), (0x4f, 0xa3), (0x4e, 0x00), (0x58, 0x71)]
            ),
            Some("w83791d")
        );
        // A bank other than 0: assume a W83781D.
        assert_eq!(
            sensor(
                0x2d,
                0xff,
                &[(0x48, 0x2d), (0x4f, 0x5c), (0x4e, 0x81), (0x58, 0x71)]
            ),
            Some("w83781d")
        );
        // Maxim: MAX6657 at 0x4c, vendor 0x4d in 0xfe.
        assert_eq!(
            sensor(
                0x4c,
                0x00,
                &[
                    (0xfe, 0x4d),
                    (0xff, 0x77),
                    (0x02, 0x00),
                    (0x03, 0x00),
                    (0x04, 0x05)
                ]
            ),
            Some("max6657")
        );
    }

    #[test]
    fn the_word_registered_sensors_are_told_by_their_ids() {
        let mut chip = FakeChip::plain(0x18, 0xff, &[]);
        chip.words = std::vec![(0x06, 0x1131), (0x07, 0xa201), (0x00, 0x0010)];
        assert_eq!(iic_probe_sensor(&mut probe_of(chip)), Some("se97"));

        let mut chip = FakeChip::plain(0x1a, 0xff, &[]);
        chip.words = std::vec![(0x06, 0x104a), (0x07, 0x2201), (0x00, 0x00ef)];
        assert_eq!(iic_probe_sensor(&mut probe_of(chip)), Some("stts2004"));

        // The same ID at an address outside 0x18..=0x1f is no memory module sensor.
        let mut chip = FakeChip::plain(0x2a, 0xff, &[]);
        chip.words = std::vec![(0x06, 0x104a), (0x07, 0x2201), (0x00, 0x00ef)];
        assert_eq!(iic_probe_sensor(&mut probe_of(chip)), None);
    }

    #[test]
    fn a_chip_that_answers_with_ones_is_nothing() {
        for addr in [0x1a, 0x2c, 0x48, 0x4a, 0x4e] {
            assert_eq!(sensor(addr, 0xff, &[]), None, "{addr:#x}");
        }
    }

    #[test]
    fn adm1032_clones_are_max1617_or_xeontemp() {
        let regs = |fe: u8| {
            let mut set: Vec<(u8, u8)> = (0..9u8).map(|r| (r, 0x20 + r)).collect();
            set.push((0x09, 0x55));
            set.extend((0x0a..0xfcu8).map(|r| (r, 0x55)));
            set.push((0xfe, fe));
            set
        };
        let probe = |addr: u8, fe: u8| {
            adm1032cloneprobe(&mut probe_of(FakeChip::plain(addr, 0x55, &regs(fe))), addr)
        };
        assert_eq!(probe(0x4c, 0x4d), Some("max1617"));
        assert_eq!(probe(0x4c, 0x00), Some("xeontemp"));
        // Only six addresses can hold one.
        assert_eq!(probe(0x4d, 0x4d), None);
        // A register that reads 0xff is not a clone, and a mostly zero file is not either.
        let mut p = probe_of(FakeChip::plain(0x4c, 0x00, &[]));
        assert_eq!(adm1032cloneprobe(&mut p, 0x4c), None);
        let mut p = probe_of(FakeChip::plain(0x4c, 0xff, &[]));
        assert_eq!(adm1032cloneprobe(&mut p, 0x4c), None);
    }

    fn lm75(addr: u8, mode: Mode) -> Option<&'static str> {
        let chip = FakeChip {
            addr,
            regs: [0xff; 256],
            words: Vec::new(),
            mode,
            reads: Cell::new(0),
        };
        lm75probe(&mut probe_of(chip))
    }

    #[test]
    fn lm75_and_lm75a_are_told_apart_by_their_aliased_registers() {
        let (conf, temp, thyst, tos) = (0x00, 0x1a80, 0x4b00, 0x5000);
        assert_eq!(
            lm75(
                0x48,
                Mode::Lm75 {
                    conf,
                    temp,
                    thyst,
                    tos,
                    last: Cell::new(0)
                }
            ),
            Some("lm75")
        );
        assert_eq!(
            lm75(
                0x49,
                Mode::Lm75a {
                    conf,
                    temp,
                    thyst,
                    tos
                }
            ),
            Some("lm75a")
        );
        // The scan reaches the same answer for both, through iic_probe_sensor.
        let chip = FakeChip {
            addr: 0x4a,
            regs: [0xff; 256],
            words: Vec::new(),
            mode: Mode::Lm75 {
                conf,
                temp,
                thyst,
                tos,
                last: Cell::new(0),
            },
            reads: Cell::new(0),
        };
        assert_eq!(iic_probe_sensor(&mut probe_of(chip)), Some("lm75"));
    }

    #[test]
    fn lm75_look_alikes_are_rejected() {
        // All values the same: very unlikely a real LM75.
        let mode = Mode::Lm75 {
            conf: 0,
            temp: 0x1a80,
            thyst: 0x1a80,
            tos: 0x1a80,
            last: Cell::new(0),
        };
        assert_eq!(lm75(0x48, mode), None);
        // Totally bogus data.
        let mode = Mode::Lm75a {
            conf: 0xff,
            temp: 0xffff,
            thyst: 0xffff,
            tos: 0x5000,
        };
        assert_eq!(lm75(0x48, mode), None);
        // Registers that do not repeat every 8 are not an LM75.
        let chip = FakeChip::plain(0x48, 0xff, &[(0x01, 0x00), (0x02, 0x4b), (0x03, 0x00)]);
        assert_eq!(lm75probe(&mut probe_of(chip)), None);
    }

    #[test]
    fn the_scan_skips_the_addresses_a_driver_asked_to_ignore() {
        for slot in &IGNORE_ADDRS {
            slot.store(0, Ordering::Relaxed);
        }
        for a in 0x41..0x4b {
            iic_ignore_addr(a);
        }
        let held: Vec<u8> = IGNORE_ADDRS
            .iter()
            .map(|s| s.load(Ordering::Relaxed))
            .collect();
        // Eight slots: the first eight are kept, the rest are dropped.
        assert_eq!(held, [0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48]);
        for slot in &IGNORE_ADDRS {
            slot.store(0, Ordering::Relaxed);
        }
    }
}
/* </TESTS> */
