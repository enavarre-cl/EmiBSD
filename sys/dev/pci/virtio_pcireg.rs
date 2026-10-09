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
 * Copyright (c) 2019 Stefan Fritsch <sf@openbsd.org>
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
//! `<dev/pci/virtio_pcireg.h>`: the virtio PCI registers, the 0.9 I/O BAR layout and the
//! 1.0 capabilities.
//!
//! Upstream: sys/dev/pci/virtio_pcireg.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The structures are `__packed` in C; their natural layout has no padding, so `#[repr(C)]`
//!   gives the same offsets (checked at compile time). Capabilities are read out of
//!   configuration space a register at a time, so they are built from the 32-bit words
//!   (`VirtioPciCap::from_regs`) instead of through the C's union with `pcireg_t reg[]`.

use core::mem::offset_of;

use crate::dev::pci::pcivar::Pcireg;

// Virtio 0.9 config space

/// `VIRTIO_CONFIG_DEVICE_FEATURES`: 32bit.
pub const VIRTIO_CONFIG_DEVICE_FEATURES: usize = 0;
/// `VIRTIO_CONFIG_GUEST_FEATURES`: 32bit.
pub const VIRTIO_CONFIG_GUEST_FEATURES: usize = 4;
/// `VIRTIO_CONFIG_QUEUE_ADDRESS`: 32bit.
pub const VIRTIO_CONFIG_QUEUE_ADDRESS: usize = 8;
/// `VIRTIO_CONFIG_QUEUE_SIZE`: 16bit.
pub const VIRTIO_CONFIG_QUEUE_SIZE: usize = 12;
/// `VIRTIO_CONFIG_QUEUE_SELECT`: 16bit.
pub const VIRTIO_CONFIG_QUEUE_SELECT: usize = 14;
/// `VIRTIO_CONFIG_QUEUE_NOTIFY`: 16bit.
pub const VIRTIO_CONFIG_QUEUE_NOTIFY: usize = 16;
/// `VIRTIO_CONFIG_DEVICE_STATUS`: 8bit.
pub const VIRTIO_CONFIG_DEVICE_STATUS: usize = 18;
/// `VIRTIO_CONFIG_ISR_STATUS`: 8bit.
pub const VIRTIO_CONFIG_ISR_STATUS: usize = 19;
/// `VIRTIO_CONFIG_ISR_CONFIG_CHANGE`.
pub const VIRTIO_CONFIG_ISR_CONFIG_CHANGE: u8 = 2;
/// `VIRTIO_CONFIG_DEVICE_CONFIG_NOMSI`.
pub const VIRTIO_CONFIG_DEVICE_CONFIG_NOMSI: u32 = 20;
// Only if MSIX is enabled:
/// `VIRTIO_MSI_CONFIG_VECTOR`: 16bit, optional.
pub const VIRTIO_MSI_CONFIG_VECTOR: usize = 20;
/// `VIRTIO_MSI_QUEUE_VECTOR`: 16bit, optional.
pub const VIRTIO_MSI_QUEUE_VECTOR: usize = 22;
/// `VIRTIO_CONFIG_DEVICE_CONFIG_MSI`.
pub const VIRTIO_CONFIG_DEVICE_CONFIG_MSI: u32 = 24;

/// `VIRTIO_MSI_NO_VECTOR`.
pub const VIRTIO_MSI_NO_VECTOR: u16 = 0xffff;

// Virtio 1.0 specific

/// `VIRTIO_PCI_CAP_COMMON_CFG`: common configuration.
pub const VIRTIO_PCI_CAP_COMMON_CFG: u8 = 1;
/// `VIRTIO_PCI_CAP_NOTIFY_CFG`: notifications.
pub const VIRTIO_PCI_CAP_NOTIFY_CFG: u8 = 2;
/// `VIRTIO_PCI_CAP_ISR_CFG`: ISR status.
pub const VIRTIO_PCI_CAP_ISR_CFG: u8 = 3;
/// `VIRTIO_PCI_CAP_DEVICE_CFG`: device specific configuration.
pub const VIRTIO_PCI_CAP_DEVICE_CFG: u8 = 4;
/// `VIRTIO_PCI_CAP_PCI_CFG`: PCI configuration access.
pub const VIRTIO_PCI_CAP_PCI_CFG: u8 = 5;

/// `struct virtio_pci_cap`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VirtioPciCap {
    /// Generic PCI field: `PCI_CAP_ID_VNDR`.
    pub cap_vndr: u8,
    /// Generic PCI field: next ptr.
    pub cap_next: u8,
    /// Generic PCI field: capability length.
    pub cap_len: u8,
    /// Identifies the structure.
    pub cfg_type: u8,
    /// Where to find it.
    pub bar: u8,
    /// Pad to full dword.
    pub padding: [u8; 3],
    /// Offset within bar.
    pub offset: u32,
    /// Length of the structure, in bytes.
    pub length: u32,
}

impl VirtioPciCap {
    /// The capability as configuration space holds it: four little-endian registers.
    pub fn from_regs(reg: &[Pcireg; 4]) -> Self {
        let [b0, b1, b2, b3] = reg[0].to_le_bytes();
        let [bar, p0, p1, p2] = reg[1].to_le_bytes();
        Self {
            cap_vndr: b0,
            cap_next: b1,
            cap_len: b2,
            cfg_type: b3,
            bar,
            padding: [p0, p1, p2],
            offset: reg[2],
            length: reg[3],
        }
    }
}

/// `struct virtio_pci_notify_cap`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VirtioPciNotifyCap {
    /// `cap`.
    pub cap: VirtioPciCap,
    /// Multiplier for `queue_notify_off`.
    pub notify_off_multiplier: u32,
}

/// `struct virtio_pci_cfg_cap`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VirtioPciCfgCap {
    /// `cap`.
    pub cap: VirtioPciCap,
    /// Data for BAR access.
    pub pci_cfg_data: [u8; 4],
}

/// `struct virtio_pci_common_cfg`: the layout of the common configuration region.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VirtioPciCommonCfg {
    // About the whole device.
    /// read-write
    pub device_feature_select: u32,
    /// read-only for driver
    pub device_feature: u32,
    /// read-write
    pub driver_feature_select: u32,
    /// read-write
    pub driver_feature: u32,
    /// read-write
    pub config_msix_vector: u16,
    /// read-only for driver
    pub num_queues: u16,
    /// read-write
    pub device_status: u8,
    /// read-only for driver
    pub config_generation: u8,

    // About a specific virtqueue.
    /// read-write
    pub queue_select: u16,
    /// read-write, power of 2, or 0.
    pub queue_size: u16,
    /// read-write
    pub queue_msix_vector: u16,
    /// read-write
    pub queue_enable: u16,
    /// read-only for driver
    pub queue_notify_off: u16,
    /// read-write
    pub queue_desc: u64,
    /// read-write
    pub queue_avail: u64,
    /// read-write
    pub queue_used: u64,
}

// The C's `__packed` offsets.
const _: () = {
    assert!(size_of::<VirtioPciCap>() == 16);
    assert!(offset_of!(VirtioPciCap, offset) == 8);
    assert!(size_of::<VirtioPciNotifyCap>() == 20);
    assert!(size_of::<VirtioPciCfgCap>() == 20);
    assert!(offset_of!(VirtioPciCommonCfg, config_msix_vector) == 16);
    assert!(offset_of!(VirtioPciCommonCfg, device_status) == 20);
    assert!(offset_of!(VirtioPciCommonCfg, queue_select) == 22);
    assert!(offset_of!(VirtioPciCommonCfg, queue_notify_off) == 30);
    assert!(offset_of!(VirtioPciCommonCfg, queue_desc) == 32);
    assert!(offset_of!(VirtioPciCommonCfg, queue_used) == 48);
    assert!(size_of::<VirtioPciCommonCfg>() == 56);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/pci/virtio_pcireg.h");
        crate::reftest::assert_defines!(defs;
            VIRTIO_CONFIG_DEVICE_FEATURES, VIRTIO_CONFIG_GUEST_FEATURES,
            VIRTIO_CONFIG_QUEUE_ADDRESS, VIRTIO_CONFIG_QUEUE_SIZE, VIRTIO_CONFIG_QUEUE_SELECT,
            VIRTIO_CONFIG_QUEUE_NOTIFY, VIRTIO_CONFIG_DEVICE_STATUS, VIRTIO_CONFIG_ISR_STATUS,
            VIRTIO_CONFIG_ISR_CONFIG_CHANGE, VIRTIO_CONFIG_DEVICE_CONFIG_NOMSI,
            VIRTIO_MSI_CONFIG_VECTOR, VIRTIO_MSI_QUEUE_VECTOR, VIRTIO_CONFIG_DEVICE_CONFIG_MSI,
            VIRTIO_MSI_NO_VECTOR, VIRTIO_PCI_CAP_COMMON_CFG, VIRTIO_PCI_CAP_NOTIFY_CFG,
            VIRTIO_PCI_CAP_ISR_CFG, VIRTIO_PCI_CAP_DEVICE_CFG, VIRTIO_PCI_CAP_PCI_CFG,
        );
    }

    #[test]
    fn a_capability_is_read_from_its_registers() {
        // QEMU's common configuration capability: vendor-specific (9), next 0x70, length 16,
        // type 1, BAR 4, offset 0, length 0x1000.
        let cap = VirtioPciCap::from_regs(&[0x0110_7009, 0x0000_0004, 0, 0x1000]);
        assert_eq!(cap.cap_vndr, 9);
        assert_eq!(cap.cap_next, 0x70);
        assert_eq!(cap.cap_len, 16);
        assert_eq!(cap.cfg_type, VIRTIO_PCI_CAP_COMMON_CFG);
        assert_eq!(cap.bar, 4);
        assert_eq!(cap.offset, 0);
        assert_eq!(cap.length, 0x1000);
    }
}
/* </TESTS> */
