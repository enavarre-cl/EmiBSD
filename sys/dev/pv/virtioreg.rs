/* <LICENSES> */
/*
 * Copyright (c) 2012 Stefan Fritsch.
 * Copyright (c) 2010 Minoura Makoto.
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

/*
 * Part of the file derived from `Virtio PCI Card Specification v0.8.6 DRAFT'
 * Appendix A.
 */
/* An interface for efficient virtio implementation.
 *
 * This header is BSD licensed so anyone can use the definitions
 * to implement compatible drivers/servers.
 *
 * Copyright 2007, 2009, IBM Corporation
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
 * 3. Neither the name of IBM nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 * THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL IBM OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! `<dev/pv/virtioreg.h>`: the virtio registers and ring layout, from the "Virtio PCI Card
//! Specification v0.8.6 DRAFT" Appendix A.
//!
//! Upstream: sys/dev/pv/virtioreg.h @ 3ce1f3f79392
//!
//! The standard layout for the ring is a continuous chunk of memory which looks like this.
//! We assume num is a power of 2.
//!
//! ```text
//! struct vring {
//!      // The actual descriptors (16 bytes each)
//!      struct vring_desc desc[num];
//!
//!      // A ring of available descriptor heads with free-running index.
//!      __u16 avail_flags;
//!      __u16 avail_idx;
//!      __u16 available[num];
//!      __u16 used_event_idx
//!
//!      // Padding to the next align boundary.
//!      char pad[];
//!
//!      // A ring of used descriptor heads with free-running index.
//!      __u16 used_flags;
//!      __u16 used_idx;
//!      struct vring_used_elem used[num];
//!      __u16 avail_event_idx;
//! };
//! ```
//! Note: for virtio PCI, align is 4096.
//!
//! ## Deviations
//! - The rings live in DMA memory the device reads and writes behind the compiler's back, so
//!   the structures are only ever reached through raw pointers with volatile accesses
//!   (`dev/pv/virtio.rs`), never through references; `ring[0]` flexible members are the
//!   constant offsets [`VRING_AVAIL_RING`] and [`VRING_USED_RING`].
//! - `VQ_USED_EVENT(vq)` and `VQ_AVAIL_EVENT(vq)` are methods of `struct virtqueue`
//!   (`Virtqueue::vq_used_event`, `Virtqueue::vq_avail_event`, `dev/pv/virtiovar.rs`), as
//!   they take one.

use core::mem::offset_of;

/// `PCI_PRODUCT_VIRTIO_NETWORK`: virtio product id (subsystem).
pub const PCI_PRODUCT_VIRTIO_NETWORK: i32 = 1;
/// `PCI_PRODUCT_VIRTIO_BLOCK`.
pub const PCI_PRODUCT_VIRTIO_BLOCK: i32 = 2;
/// `PCI_PRODUCT_VIRTIO_CONSOLE`.
pub const PCI_PRODUCT_VIRTIO_CONSOLE: i32 = 3;
/// `PCI_PRODUCT_VIRTIO_ENTROPY`.
pub const PCI_PRODUCT_VIRTIO_ENTROPY: i32 = 4;
/// `PCI_PRODUCT_VIRTIO_BALLOON`.
pub const PCI_PRODUCT_VIRTIO_BALLOON: i32 = 5;
/// `PCI_PRODUCT_VIRTIO_IOMEM`.
pub const PCI_PRODUCT_VIRTIO_IOMEM: i32 = 6;
/// `PCI_PRODUCT_VIRTIO_RPMSG`.
pub const PCI_PRODUCT_VIRTIO_RPMSG: i32 = 7;
/// `PCI_PRODUCT_VIRTIO_SCSI`.
pub const PCI_PRODUCT_VIRTIO_SCSI: i32 = 8;
/// `PCI_PRODUCT_VIRTIO_9P`.
pub const PCI_PRODUCT_VIRTIO_9P: i32 = 9;
/// `PCI_PRODUCT_VIRTIO_MAC80211`.
pub const PCI_PRODUCT_VIRTIO_MAC80211: i32 = 10;
/// `PCI_PRODUCT_VIRTIO_GPU`.
pub const PCI_PRODUCT_VIRTIO_GPU: i32 = 16;
/// `PCI_PRODUCT_VIRTIO_VMMCI`: private id.
pub const PCI_PRODUCT_VIRTIO_VMMCI: i32 = 65535;

// Device-independent feature bits.

/// `VIRTIO_F_NOTIFY_ON_EMPTY`.
pub const VIRTIO_F_NOTIFY_ON_EMPTY: u64 = 1 << 24;
/// `VIRTIO_F_ANY_LAYOUT`.
pub const VIRTIO_F_ANY_LAYOUT: u64 = 1 << 27;
/// `VIRTIO_F_RING_INDIRECT_DESC`.
pub const VIRTIO_F_RING_INDIRECT_DESC: u64 = 1 << 28;
/// `VIRTIO_F_RING_EVENT_IDX`.
pub const VIRTIO_F_RING_EVENT_IDX: u64 = 1 << 29;
/// `VIRTIO_F_BAD_FEATURE`.
pub const VIRTIO_F_BAD_FEATURE: u64 = 1 << 30;
/// `VIRTIO_F_VERSION_1`.
pub const VIRTIO_F_VERSION_1: u64 = 1 << 32;
/// `VIRTIO_F_ACCESS_PLATFORM`.
pub const VIRTIO_F_ACCESS_PLATFORM: u64 = 1 << 33;
/// `VIRTIO_F_RING_PACKED`.
pub const VIRTIO_F_RING_PACKED: u64 = 1 << 34;
/// `VIRTIO_F_IN_ORDER`.
pub const VIRTIO_F_IN_ORDER: u64 = 1 << 35;
/// `VIRTIO_F_ORDER_PLATFORM`.
pub const VIRTIO_F_ORDER_PLATFORM: u64 = 1 << 36;
/// `VIRTIO_F_SR_IOV`.
pub const VIRTIO_F_SR_IOV: u64 = 1 << 37;
/// `VIRTIO_F_NOTIFICATION_DATA`.
pub const VIRTIO_F_NOTIFICATION_DATA: u64 = 1 << 38;
/// `VIRTIO_F_NOTIF_CONFIG_DATA`.
pub const VIRTIO_F_NOTIF_CONFIG_DATA: u64 = 1 << 39;
/// `VIRTIO_F_RING_RESET`.
pub const VIRTIO_F_RING_RESET: u64 = 1 << 40;

// Device status bits.

/// `VIRTIO_CONFIG_DEVICE_STATUS_RESET`.
pub const VIRTIO_CONFIG_DEVICE_STATUS_RESET: i32 = 0;
/// `VIRTIO_CONFIG_DEVICE_STATUS_ACK`.
pub const VIRTIO_CONFIG_DEVICE_STATUS_ACK: i32 = 1;
/// `VIRTIO_CONFIG_DEVICE_STATUS_DRIVER`.
pub const VIRTIO_CONFIG_DEVICE_STATUS_DRIVER: i32 = 2;
/// `VIRTIO_CONFIG_DEVICE_STATUS_DRIVER_OK`.
pub const VIRTIO_CONFIG_DEVICE_STATUS_DRIVER_OK: i32 = 4;
/// `VIRTIO_CONFIG_DEVICE_STATUS_FEATURES_OK`.
pub const VIRTIO_CONFIG_DEVICE_STATUS_FEATURES_OK: i32 = 8;
/// `VIRTIO_CONFIG_DEVICE_STATUS_DEVICE_NEEDS_RESET`.
pub const VIRTIO_CONFIG_DEVICE_STATUS_DEVICE_NEEDS_RESET: i32 = 64;
/// `VIRTIO_CONFIG_DEVICE_STATUS_FAILED`.
pub const VIRTIO_CONFIG_DEVICE_STATUS_FAILED: i32 = 128;

// Virtqueue

/// `VRING_DESC_F_NEXT`: this marks a buffer as continuing via the next field.
pub const VRING_DESC_F_NEXT: u16 = 1;
/// `VRING_DESC_F_WRITE`: this marks a buffer as write-only (otherwise read-only).
pub const VRING_DESC_F_WRITE: u16 = 2;
/// `VRING_DESC_F_INDIRECT`: this means the buffer contains a list of buffer descriptors.
pub const VRING_DESC_F_INDIRECT: u16 = 4;

/// `VRING_USED_F_NO_NOTIFY`: the Host uses this in `used->flags` to advise the Guest: don't
/// kick me when you add a buffer. It's unreliable, so it's simply an optimization. Guest
/// will still kick if it's out of buffers.
pub const VRING_USED_F_NO_NOTIFY: u16 = 1;
/// `VRING_AVAIL_F_NO_INTERRUPT`: the Guest uses this in `avail->flags` to advise the Host:
/// don't interrupt me when you consume a buffer. It's unreliable, so it's simply an
/// optimization.
pub const VRING_AVAIL_F_NO_INTERRUPT: u16 = 1;

/// `VIRTIO_PAGE_SIZE`.
pub const VIRTIO_PAGE_SIZE: usize = 4096;

/// `struct vring_desc`: virtio ring descriptors, 16 bytes. These can chain together via
/// `next`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VringDesc {
    /// Address (guest-physical).
    pub addr: u64,
    /// Length.
    pub len: u32,
    /// The flags as indicated above.
    pub flags: u16,
    /// We chain unused descriptors via this, too.
    pub next: u16,
}

/// `struct vring_avail`: the driver's ring of available descriptor heads; `ring[]` follows
/// at [`VRING_AVAIL_RING`].
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VringAvail {
    /// `flags`.
    pub flags: u16,
    /// `idx`: free-running index of the next slot the driver fills.
    pub idx: u16,
}

/// `struct vring_used_elem`. `u32` is used here for ids for padding reasons.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VringUsedElem {
    /// Index of start of used descriptor chain.
    pub id: u32,
    /// Total length of the descriptor chain which was written to.
    pub len: u32,
}

/// `struct vring_used`: the device's ring of used descriptor chains; `ring[]` follows at
/// [`VRING_USED_RING`].
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VringUsed {
    /// `flags`.
    pub flags: u16,
    /// `idx`: free-running index of the next slot the device fills.
    pub idx: u16,
}

/// `offsetof(struct vring_avail, ring)`.
pub const VRING_AVAIL_RING: usize = size_of::<VringAvail>();
/// `offsetof(struct vring_used, ring)`.
pub const VRING_USED_RING: usize = size_of::<VringUsed>();

// The structures are __packed in C; the natural layout here has no padding, so it is the
// same.
const _: () = {
    assert!(size_of::<VringDesc>() == 16);
    assert!(offset_of!(VringDesc, len) == 8);
    assert!(offset_of!(VringDesc, flags) == 12);
    assert!(offset_of!(VringDesc, next) == 14);
    assert!(size_of::<VringAvail>() == 4);
    assert!(size_of::<VringUsedElem>() == 8);
    assert!(size_of::<VringUsed>() == 4);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/pv/virtioreg.h");
        crate::reftest::assert_defines!(defs;
            PCI_PRODUCT_VIRTIO_NETWORK, PCI_PRODUCT_VIRTIO_BLOCK, PCI_PRODUCT_VIRTIO_CONSOLE,
            PCI_PRODUCT_VIRTIO_ENTROPY, PCI_PRODUCT_VIRTIO_BALLOON, PCI_PRODUCT_VIRTIO_IOMEM,
            PCI_PRODUCT_VIRTIO_RPMSG, PCI_PRODUCT_VIRTIO_SCSI, PCI_PRODUCT_VIRTIO_9P,
            PCI_PRODUCT_VIRTIO_MAC80211, PCI_PRODUCT_VIRTIO_GPU, PCI_PRODUCT_VIRTIO_VMMCI,
            VIRTIO_F_NOTIFY_ON_EMPTY, VIRTIO_F_ANY_LAYOUT, VIRTIO_F_RING_INDIRECT_DESC,
            VIRTIO_F_RING_EVENT_IDX, VIRTIO_F_BAD_FEATURE, VIRTIO_F_VERSION_1,
            VIRTIO_F_ACCESS_PLATFORM, VIRTIO_F_RING_PACKED, VIRTIO_F_IN_ORDER,
            VIRTIO_F_ORDER_PLATFORM, VIRTIO_F_SR_IOV, VIRTIO_F_NOTIFICATION_DATA,
            VIRTIO_F_NOTIF_CONFIG_DATA, VIRTIO_F_RING_RESET,
            VIRTIO_CONFIG_DEVICE_STATUS_RESET, VIRTIO_CONFIG_DEVICE_STATUS_ACK,
            VIRTIO_CONFIG_DEVICE_STATUS_DRIVER, VIRTIO_CONFIG_DEVICE_STATUS_DRIVER_OK,
            VIRTIO_CONFIG_DEVICE_STATUS_FEATURES_OK,
            VIRTIO_CONFIG_DEVICE_STATUS_DEVICE_NEEDS_RESET, VIRTIO_CONFIG_DEVICE_STATUS_FAILED,
            VRING_DESC_F_NEXT, VRING_DESC_F_WRITE, VRING_DESC_F_INDIRECT,
            VRING_USED_F_NO_NOTIFY, VRING_AVAIL_F_NO_INTERRUPT, VIRTIO_PAGE_SIZE,
        );
    }

    #[test]
    fn feature_bits_are_the_spec_bit_numbers() {
        assert_eq!(VIRTIO_F_RING_INDIRECT_DESC.trailing_zeros(), 28);
        assert_eq!(VIRTIO_F_RING_EVENT_IDX.trailing_zeros(), 29);
        assert_eq!(VIRTIO_F_VERSION_1.trailing_zeros(), 32);
        assert_eq!(VIRTIO_F_RING_RESET.trailing_zeros(), 40);
    }
}
/* </TESTS> */
