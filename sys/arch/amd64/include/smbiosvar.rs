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
 * Copyright (c) 2006 Gordon Willem Klok <gklok@cogeco.ca>
 * Copyright (c) 2005 Jordan Hargrave
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
 * THIS SOFTWARE IS PROVIDED BY THE AUTHORS AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED. IN NO EVENT SHALL THE AUTHORS OR CONTRIBUTORS BE LIABLE FOR
 * ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! `<machine/smbiosvar.h>`: the SMBIOS (DMTF DSP0134) entry points and structures the
//! kernel reads, and `bios.c`'s handle on the structure table (`smbios_entry`,
//! `struct smbtable`).
//!
//! Upstream: sys/arch/amd64/include/smbiosvar.h @ 3ce1f3f79392
//!
//! The functions it declares, `smbios_find_table` and `smbios_get_string`, are `bios.c`'s
//! (`sys/arch/amd64/amd64/bios.rs`).
//!
//! ## Deviations
//! - `SMBIOS_UUID_REP` is the `printf` format the C hands `snprintf`; `bios.rs` writes the
//!   same text with `format_args!` (the constant is kept for reference).
//! - `struct smbtable`'s `hdr` and `tblhdr` are raw pointers into the structure table, as in
//!   the C; [`Smbtable::default`] is the C's zeroed one (`cookie = 0`).

#![allow(dead_code)] // a header: each includer uses part of it

use core::ffi::c_void;
use core::ptr;

/// `SMBIOS_START`: where the BIOS's entry point may be, in the ISA hole.
pub const SMBIOS_START: usize = 0xf0000;
/// `SMBIOS_END`.
pub const SMBIOS_END: usize = 0xfffff;

/// `SMBIOS_UUID_NPRESENT`.
pub const SMBIOS_UUID_NPRESENT: u32 = 0x1;
/// `SMBIOS_UUID_NSET`.
pub const SMBIOS_UUID_NSET: u32 = 0x2;

/// `SMBIOS_UUID_REP`: section 3.5 of "UUIDs and GUIDs" found at
/// <http://www.opengroup.org/dce/info/draft-leach-uuids-guids-01.txt> specifies the string
/// representation of a UUID.
pub const SMBIOS_UUID_REP: &str =
    "%02x%02x%02x%02x-%02x%02x-%02x%02x-%02x%02x-%02x%02x%02x%02x%02x%02x";
/// `SMBIOS_UUID_REPLEN`: 16 zero padded values, 4 hyphens, 1 null.
pub const SMBIOS_UUID_REPLEN: usize = 37;

/// `struct smbios_entry`: the structure table `bios_attach` mapped.
#[derive(Clone, Copy)]
pub struct SmbiosEntry {
    /// `mjr`: the specification's major revision.
    pub mjr: u8,
    /// `min`: its minor revision.
    pub min: u8,
    /// `addr`: the table's kernel virtual address (null before `bios_attach` found one).
    pub addr: *const u8,
    /// `len`: its length in bytes.
    pub len: u16,
    /// `count`: the number of structures in it.
    pub count: u16,
}

impl SmbiosEntry {
    /// The zeroed `smbios_entry` of a machine without SMBIOS.
    pub const fn new() -> Self {
        Self {
            mjr: 0,
            min: 0,
            addr: ptr::null(),
            len: 0,
            count: 0,
        }
    }
}

impl Default for SmbiosEntry {
    fn default() -> Self {
        Self::new()
    }
}

// SAFETY: `addr` points at the firmware's structure table, which bios0 maps read-only for
// good and nothing writes.
unsafe impl Send for SmbiosEntry {}

/// `struct smbhdr`: the SMBIOS 2 entry point.
#[repr(C, packed)]
#[derive(Clone, Copy, Default)]
pub struct Smbhdr {
    /// `sig`: "_SM_".
    pub sig: u32,
    /// `checksum`: entry point checksum.
    pub checksum: u8,
    /// `len`: entry point structure length.
    pub len: u8,
    /// `majrev`: specification major revision.
    pub majrev: u8,
    /// `minrev`: specification minor revision.
    pub minrev: u8,
    /// `mss`: maximum structure size.
    pub mss: u16,
    /// `epr`: entry point revision.
    pub epr: u8,
    /// `fa`: value determined by EPR.
    pub fa: [u8; 5],
    /// `sasig`: secondary anchor "_DMI_".
    pub sasig: [u8; 5],
    /// `sachecksum`: secondary checksum.
    pub sachecksum: u8,
    /// `size`: length of structure table in bytes.
    pub size: u16,
    /// `addr`: structure table address.
    pub addr: u32,
    /// `count`: number of SMBIOS structures.
    pub count: u16,
    /// `rev`: BCD revision.
    pub rev: u8,
}

/// `struct smb3hdr`: the SMBIOS 3 entry point.
#[repr(C, packed)]
#[derive(Clone, Copy, Default)]
pub struct Smb3hdr {
    /// `sig`: "_SM3_".
    pub sig: [u8; 5],
    /// `checksum`: entry point structure checksum.
    pub checksum: u8,
    /// `len`: entry point length.
    pub len: u8,
    /// `majrev`: SMBIOS major version.
    pub majrev: u8,
    /// `minrev`: SMBIOS minor version.
    pub minrev: u8,
    /// `docrev`: SMBIOS docrev.
    pub docrev: u8,
    /// `epr`: entry point revision.
    pub epr: u8,
    /// `reserved`.
    pub reserved: u8,
    /// `size`: structure table maximum size.
    pub size: u32,
    /// `addr`: structure table address.
    pub addr: u64,
}

/// `struct smbtblhdr`: the header of every structure.
#[repr(C, packed)]
#[derive(Clone, Copy, Default)]
pub struct Smbtblhdr {
    /// `type`.
    pub r#type: u8,
    /// `size`: the formatted area's length, this header included.
    pub size: u8,
    /// `handle`.
    pub handle: u16,
}

/// `struct smbtable`: a structure `smbios_find_table` found.
#[derive(Clone, Copy)]
pub struct Smbtable {
    /// `hdr`: its header.
    pub hdr: *const Smbtblhdr,
    /// `tblhdr`: its formatted area, after the header.
    pub tblhdr: *const c_void,
    /// `cookie`: where the next search for the same type goes on (0: from the start).
    pub cookie: u32,
}

impl Default for Smbtable {
    fn default() -> Self {
        Self {
            hdr: ptr::null(),
            tblhdr: ptr::null(),
            cookie: 0,
        }
    }
}

/// `SMBIOS_TYPE_BIOS`.
pub const SMBIOS_TYPE_BIOS: u8 = 0;
/// `SMBIOS_TYPE_SYSTEM`.
pub const SMBIOS_TYPE_SYSTEM: u8 = 1;
/// `SMBIOS_TYPE_BASEBOARD`.
pub const SMBIOS_TYPE_BASEBOARD: u8 = 2;
/// `SMBIOS_TYPE_ENCLOSURE`.
pub const SMBIOS_TYPE_ENCLOSURE: u8 = 3;
/// `SMBIOS_TYPE_PROCESSOR`.
pub const SMBIOS_TYPE_PROCESSOR: u8 = 4;
/// `SMBIOS_TYPE_MEMCTRL`.
pub const SMBIOS_TYPE_MEMCTRL: u8 = 5;
/// `SMBIOS_TYPE_MEMMOD`.
pub const SMBIOS_TYPE_MEMMOD: u8 = 6;
/// `SMBIOS_TYPE_CACHE`.
pub const SMBIOS_TYPE_CACHE: u8 = 7;
/// `SMBIOS_TYPE_PORT`.
pub const SMBIOS_TYPE_PORT: u8 = 8;
/// `SMBIOS_TYPE_SLOTS`.
pub const SMBIOS_TYPE_SLOTS: u8 = 9;
/// `SMBIOS_TYPE_OBD`.
pub const SMBIOS_TYPE_OBD: u8 = 10;
/// `SMBIOS_TYPE_OEM`.
pub const SMBIOS_TYPE_OEM: u8 = 11;
/// `SMBIOS_TYPE_SYSCONFOPT`.
pub const SMBIOS_TYPE_SYSCONFOPT: u8 = 12;
/// `SMBIOS_TYPE_BIOSLANG`.
pub const SMBIOS_TYPE_BIOSLANG: u8 = 13;
/// `SMBIOS_TYPE_GROUPASSOC`.
pub const SMBIOS_TYPE_GROUPASSOC: u8 = 14;
/// `SMBIOS_TYPE_SYSEVENTLOG`.
pub const SMBIOS_TYPE_SYSEVENTLOG: u8 = 15;
/// `SMBIOS_TYPE_PHYMEM`.
pub const SMBIOS_TYPE_PHYMEM: u8 = 16;
/// `SMBIOS_TYPE_MEMDEV`.
pub const SMBIOS_TYPE_MEMDEV: u8 = 17;
/// `SMBIOS_TYPE_ECCINFO32`.
pub const SMBIOS_TYPE_ECCINFO32: u8 = 18;
/// `SMBIOS_TYPE_MEMMAPARRAYADDR`.
pub const SMBIOS_TYPE_MEMMAPARRAYADDR: u8 = 19;
/// `SMBIOS_TYPE_MEMMAPDEVADDR`.
pub const SMBIOS_TYPE_MEMMAPDEVADDR: u8 = 20;
/// `SMBIOS_TYPE_INBUILTPOINT`.
pub const SMBIOS_TYPE_INBUILTPOINT: u8 = 21;
/// `SMBIOS_TYPE_PORTBATT`.
pub const SMBIOS_TYPE_PORTBATT: u8 = 22;
/// `SMBIOS_TYPE_SYSRESET`.
pub const SMBIOS_TYPE_SYSRESET: u8 = 23;
/// `SMBIOS_TYPE_HWSECUIRTY` (sic).
pub const SMBIOS_TYPE_HWSECUIRTY: u8 = 24;
/// `SMBIOS_TYPE_PWRCTRL`.
pub const SMBIOS_TYPE_PWRCTRL: u8 = 25;
/// `SMBIOS_TYPE_VOLTPROBE`.
pub const SMBIOS_TYPE_VOLTPROBE: u8 = 26;
/// `SMBIOS_TYPE_COOLING`.
pub const SMBIOS_TYPE_COOLING: u8 = 27;
/// `SMBIOS_TYPE_TEMPPROBE`.
pub const SMBIOS_TYPE_TEMPPROBE: u8 = 28;
/// `SMBIOS_TYPE_CURRENTPROBE`.
pub const SMBIOS_TYPE_CURRENTPROBE: u8 = 29;
/// `SMBIOS_TYPE_OOB_REMOTEACCESS`.
pub const SMBIOS_TYPE_OOB_REMOTEACCESS: u8 = 30;
/// `SMBIOS_TYPE_BIS`.
pub const SMBIOS_TYPE_BIS: u8 = 31;
/// `SMBIOS_TYPE_SBI`.
pub const SMBIOS_TYPE_SBI: u8 = 32;
/// `SMBIOS_TYPE_ECCINFO64`.
pub const SMBIOS_TYPE_ECCINFO64: u8 = 33;
/// `SMBIOS_TYPE_MGMTDEV`.
pub const SMBIOS_TYPE_MGMTDEV: u8 = 34;
/// `SMBIOS_TYPE_MGTDEVCOMP`.
pub const SMBIOS_TYPE_MGTDEVCOMP: u8 = 35;
/// `SMBIOS_TYPE_MGTDEVTHRESH`.
pub const SMBIOS_TYPE_MGTDEVTHRESH: u8 = 36;
/// `SMBIOS_TYPE_MEMCHANNEL`.
pub const SMBIOS_TYPE_MEMCHANNEL: u8 = 37;
/// `SMBIOS_TYPE_IPMIDEV`.
pub const SMBIOS_TYPE_IPMIDEV: u8 = 38;
/// `SMBIOS_TYPE_SPS`.
pub const SMBIOS_TYPE_SPS: u8 = 39;
/// `SMBIOS_TYPE_INACTIVE`.
pub const SMBIOS_TYPE_INACTIVE: u8 = 126;
/// `SMBIOS_TYPE_EOT`: end of table.
pub const SMBIOS_TYPE_EOT: u8 = 127;

/// `struct smbios_struct_bios`: SMBIOS structure type 0 "BIOS Information", DMTF
/// specification DSP0134 section 3.3.1 p.g. 34.
#[repr(C, packed)]
#[derive(Clone, Copy, Default)]
pub struct SmbiosStructBios {
    /// `vendor`: string.
    pub vendor: u8,
    /// `version`: string.
    pub version: u8,
    /// `startaddr`.
    pub startaddr: u16,
    /// `release`: string.
    pub release: u8,
    /// `romsize`.
    pub romsize: u8,
    /// `characteristics`.
    pub characteristics: u64,
    /// `charext`.
    pub charext: u32,
    /// `major_rel`.
    pub major_rel: u8,
    /// `minor_rel`.
    pub minor_rel: u8,
    /// `ecf_mjr_rel`: embedded controller firmware.
    pub ecf_mjr_rel: u8,
    /// `ecf_min_rel`: embedded controller firmware.
    pub ecf_min_rel: u8,
}

/// `struct smbios_sys`: SMBIOS structure type 1 "System Information", DMTF specification
/// DSP0134 section 3.3.2 p.g. 35.
#[repr(C, packed)]
#[derive(Clone, Copy, Default)]
pub struct SmbiosSys {
    // SMBIOS spec 2.0+
    /// `vendor`: string.
    pub vendor: u8,
    /// `product`: string.
    pub product: u8,
    /// `version`: string.
    pub version: u8,
    /// `serial`: string.
    pub serial: u8,
    // SMBIOS spec 2.1+
    /// `uuid`.
    pub uuid: [u8; 16],
    /// `wakeup`.
    pub wakeup: u8,
    // SMBIOS spec 2.4+
    /// `sku`: string.
    pub sku: u8,
    /// `family`: string.
    pub family: u8,
}

/// `struct smbios_board`: SMBIOS structure type 2 "Base Board (Module) Information", DMTF
/// specification DSP0134 section 3.3.3 p.g. 37.
#[repr(C, packed)]
#[derive(Clone, Copy, Default)]
pub struct SmbiosBoard {
    /// `vendor`: string.
    pub vendor: u8,
    /// `product`: string.
    pub product: u8,
    /// `version`: string.
    pub version: u8,
    /// `serial`: string.
    pub serial: u8,
    /// `asset`: string.
    pub asset: u8,
    /// `feature`: feature flags.
    pub feature: u8,
    /// `location`: location in chassis.
    pub location: u8,
    /// `handle`: chassis handle.
    pub handle: u16,
    /// `type`: board type.
    pub r#type: u8,
    /// `noc`: number of contained objects.
    pub noc: u8,
}

/// `struct smbios_enclosure`: SMBIOS structure type 3 "System Enclosure or Chassis", DMTF
/// specification DSP0134.
#[repr(C, packed)]
#[derive(Clone, Copy, Default)]
pub struct SmbiosEnclosure {
    // SMBIOS spec 2.0+
    /// `vendor`: string.
    pub vendor: u8,
    /// `type`.
    pub r#type: u8,
    /// `version`: string.
    pub version: u8,
    /// `serial`: string.
    pub serial: u8,
    /// `asset_tag`: string.
    pub asset_tag: u8,
    // SMBIOS spec 2.1+
    /// `boot_state`.
    pub boot_state: u8,
    /// `psu_state`.
    pub psu_state: u8,
    /// `thermal_state`.
    pub thermal_state: u8,
    /// `security_status`.
    pub security_status: u8,
    // SMBIOS spec 2.3+
    /// `oem_defined`.
    pub oem_defined: u16,
    /// `height`.
    pub height: u8,
    /// `no_power_cords`.
    pub no_power_cords: u8,
    /// `no_contained_element`.
    pub no_contained_element: u8,
    /// `reclen_contained_element`.
    pub reclen_contained_element: u8,
    /// `contained_elements`.
    pub contained_elements: u8,
    // SMBIOS spec 2.7+
    /// `sku`: string.
    pub sku: u8,
}

/// `SMBIOS_CPUST_POPULATED`: `cpu_status`, the socket is populated.
pub const SMBIOS_CPUST_POPULATED: u8 = 1 << 6;
/// `SMBIOS_CPUST_STATUSMASK`: `cpu_status`, the processor's status.
pub const SMBIOS_CPUST_STATUSMASK: u8 = 0x07;

/// `struct smbios_cpu`: SMBIOS structure type 4 "processor Information", DMTF specification
/// DSP0134 v2.5 section 3.3.5 p.g. 24.
#[repr(C, packed)]
#[derive(Clone, Copy, Default)]
pub struct SmbiosCpu {
    /// `cpu_socket_designation`: string.
    pub cpu_socket_designation: u8,
    /// `cpu_type`.
    pub cpu_type: u8,
    /// `cpu_family`.
    pub cpu_family: u8,
    /// `cpu_mfg`: string.
    pub cpu_mfg: u8,
    /// `cpu_id_eax`.
    pub cpu_id_eax: u32,
    /// `cpu_id_edx`.
    pub cpu_id_edx: u32,
    /// `cpu_version`: string.
    pub cpu_version: u8,
    /// `cpu_voltage`.
    pub cpu_voltage: u8,
    /// `cpu_clock`.
    pub cpu_clock: u16,
    /// `cpu_max_speed`.
    pub cpu_max_speed: u16,
    /// `cpu_current_speed`.
    pub cpu_current_speed: u16,
    /// `cpu_status`: [`SMBIOS_CPUST_POPULATED`], [`SMBIOS_CPUST_STATUSMASK`].
    pub cpu_status: u8,
    /// `cpu_upgrade`.
    pub cpu_upgrade: u8,
    /// `cpu_l1_handle`.
    pub cpu_l1_handle: u16,
    /// `cpu_l2_handle`.
    pub cpu_l2_handle: u16,
    /// `cpu_l3_handle`.
    pub cpu_l3_handle: u16,
    /// `cpu_serial`: string.
    pub cpu_serial: u8,
    /// `cpu_asset_tag`: string.
    pub cpu_asset_tag: u8,
    /// `cpu_part_nr`: string.
    pub cpu_part_nr: u8,
    // following fields were added in smbios 2.5
    /// `cpu_core_count`.
    pub cpu_core_count: u8,
    /// `cpu_core_enabled`.
    pub cpu_core_enabled: u8,
    /// `cpu_thread_count`.
    pub cpu_thread_count: u8,
    /// `cpu_characteristics`.
    pub cpu_characteristics: u16,
}

/// `struct smbios_ipmi`: SMBIOS structure type 38 "IPMI Information", DMTF specification
/// DSP0134 section 3.3.39 p.g. 91.
#[repr(C, packed)]
#[derive(Clone, Copy, Default)]
pub struct SmbiosIpmi {
    /// `smipmi_if_type`: IPMI interface type.
    pub smipmi_if_type: u8,
    /// `smipmi_if_rev`: BCD IPMI revision.
    pub smipmi_if_rev: u8,
    /// `smipmi_i2c_address`: I2C address of BMC.
    pub smipmi_i2c_address: u8,
    /// `smipmi_nvram_address`: I2C address of NVRAM storage.
    pub smipmi_nvram_address: u8,
    /// `smipmi_base_address`: base address of BMC (BAR format).
    pub smipmi_base_address: u64,
    /// `smipmi_base_flags`: bits 7:6 register spacing (00 byte, 01 dword, 02 word), bit 4
    /// lower bit BAR, bit 3 IRQ valid, bit 2 N/A, bit 1 interrupt polarity, bit 0 interrupt
    /// trigger.
    pub smipmi_base_flags: u8,
    /// `smipmi_irq`: IRQ if applicable.
    pub smipmi_irq: u8,
}

const _: () = {
    assert!(size_of::<Smbhdr>() == 0x1f);
    assert!(size_of::<Smb3hdr>() == 0x18);
    assert!(size_of::<Smbtblhdr>() == 4);
    assert!(size_of::<SmbiosStructBios>() == 0x16);
    assert!(size_of::<SmbiosSys>() == 0x17);
    assert!(size_of::<SmbiosBoard>() == 0xb);
    assert!(size_of::<SmbiosIpmi>() == 14);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/arch/amd64/include/smbiosvar.h");
        for (name, value) in [
            ("SMBIOS_START", SMBIOS_START as i64),
            ("SMBIOS_END", SMBIOS_END as i64),
            ("SMBIOS_UUID_NPRESENT", i64::from(SMBIOS_UUID_NPRESENT)),
            ("SMBIOS_UUID_NSET", i64::from(SMBIOS_UUID_NSET)),
            ("SMBIOS_UUID_REPLEN", SMBIOS_UUID_REPLEN as i64),
            ("SMBIOS_TYPE_BIOS", i64::from(SMBIOS_TYPE_BIOS)),
            ("SMBIOS_TYPE_SYSTEM", i64::from(SMBIOS_TYPE_SYSTEM)),
            ("SMBIOS_TYPE_BASEBOARD", i64::from(SMBIOS_TYPE_BASEBOARD)),
            ("SMBIOS_TYPE_ENCLOSURE", i64::from(SMBIOS_TYPE_ENCLOSURE)),
            ("SMBIOS_TYPE_PROCESSOR", i64::from(SMBIOS_TYPE_PROCESSOR)),
            ("SMBIOS_TYPE_MEMCTRL", i64::from(SMBIOS_TYPE_MEMCTRL)),
            ("SMBIOS_TYPE_MEMMOD", i64::from(SMBIOS_TYPE_MEMMOD)),
            ("SMBIOS_TYPE_CACHE", i64::from(SMBIOS_TYPE_CACHE)),
            ("SMBIOS_TYPE_PORT", i64::from(SMBIOS_TYPE_PORT)),
            ("SMBIOS_TYPE_SLOTS", i64::from(SMBIOS_TYPE_SLOTS)),
            ("SMBIOS_TYPE_OBD", i64::from(SMBIOS_TYPE_OBD)),
            ("SMBIOS_TYPE_OEM", i64::from(SMBIOS_TYPE_OEM)),
            ("SMBIOS_TYPE_SYSCONFOPT", i64::from(SMBIOS_TYPE_SYSCONFOPT)),
            ("SMBIOS_TYPE_BIOSLANG", i64::from(SMBIOS_TYPE_BIOSLANG)),
            ("SMBIOS_TYPE_GROUPASSOC", i64::from(SMBIOS_TYPE_GROUPASSOC)),
            (
                "SMBIOS_TYPE_SYSEVENTLOG",
                i64::from(SMBIOS_TYPE_SYSEVENTLOG),
            ),
            ("SMBIOS_TYPE_PHYMEM", i64::from(SMBIOS_TYPE_PHYMEM)),
            ("SMBIOS_TYPE_MEMDEV", i64::from(SMBIOS_TYPE_MEMDEV)),
            ("SMBIOS_TYPE_ECCINFO32", i64::from(SMBIOS_TYPE_ECCINFO32)),
            (
                "SMBIOS_TYPE_MEMMAPARRAYADDR",
                i64::from(SMBIOS_TYPE_MEMMAPARRAYADDR),
            ),
            (
                "SMBIOS_TYPE_MEMMAPDEVADDR",
                i64::from(SMBIOS_TYPE_MEMMAPDEVADDR),
            ),
            (
                "SMBIOS_TYPE_INBUILTPOINT",
                i64::from(SMBIOS_TYPE_INBUILTPOINT),
            ),
            ("SMBIOS_TYPE_PORTBATT", i64::from(SMBIOS_TYPE_PORTBATT)),
            ("SMBIOS_TYPE_SYSRESET", i64::from(SMBIOS_TYPE_SYSRESET)),
            ("SMBIOS_TYPE_HWSECUIRTY", i64::from(SMBIOS_TYPE_HWSECUIRTY)),
            ("SMBIOS_TYPE_PWRCTRL", i64::from(SMBIOS_TYPE_PWRCTRL)),
            ("SMBIOS_TYPE_VOLTPROBE", i64::from(SMBIOS_TYPE_VOLTPROBE)),
            ("SMBIOS_TYPE_COOLING", i64::from(SMBIOS_TYPE_COOLING)),
            ("SMBIOS_TYPE_TEMPPROBE", i64::from(SMBIOS_TYPE_TEMPPROBE)),
            (
                "SMBIOS_TYPE_CURRENTPROBE",
                i64::from(SMBIOS_TYPE_CURRENTPROBE),
            ),
            (
                "SMBIOS_TYPE_OOB_REMOTEACCESS",
                i64::from(SMBIOS_TYPE_OOB_REMOTEACCESS),
            ),
            ("SMBIOS_TYPE_BIS", i64::from(SMBIOS_TYPE_BIS)),
            ("SMBIOS_TYPE_SBI", i64::from(SMBIOS_TYPE_SBI)),
            ("SMBIOS_TYPE_ECCINFO64", i64::from(SMBIOS_TYPE_ECCINFO64)),
            ("SMBIOS_TYPE_MGMTDEV", i64::from(SMBIOS_TYPE_MGMTDEV)),
            ("SMBIOS_TYPE_MGTDEVCOMP", i64::from(SMBIOS_TYPE_MGTDEVCOMP)),
            (
                "SMBIOS_TYPE_MGTDEVTHRESH",
                i64::from(SMBIOS_TYPE_MGTDEVTHRESH),
            ),
            ("SMBIOS_TYPE_MEMCHANNEL", i64::from(SMBIOS_TYPE_MEMCHANNEL)),
            ("SMBIOS_TYPE_IPMIDEV", i64::from(SMBIOS_TYPE_IPMIDEV)),
            ("SMBIOS_TYPE_SPS", i64::from(SMBIOS_TYPE_SPS)),
            ("SMBIOS_TYPE_INACTIVE", i64::from(SMBIOS_TYPE_INACTIVE)),
            ("SMBIOS_TYPE_EOT", i64::from(SMBIOS_TYPE_EOT)),
            ("SMBIOS_CPUST_POPULATED", i64::from(SMBIOS_CPUST_POPULATED)),
            (
                "SMBIOS_CPUST_STATUSMASK",
                i64::from(SMBIOS_CPUST_STATUSMASK),
            ),
        ] {
            assert_eq!(crate::reftest::int(&defs, name), Some(value), "{name}");
        }
    }
}
/* </TESTS> */
