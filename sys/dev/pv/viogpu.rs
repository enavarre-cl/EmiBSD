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
 * Copyright (c) 2021-2023 joshua stein <jcs@openbsd.org>
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
 * Virtio GPU Device
 *
 * Copyright Red Hat, Inc. 2013-2014
 *
 * Authors:
 *     Dave Airlie <airlied@redhat.com>
 *     Gerd Hoffmann <kraxel@redhat.com>
 *
 * This header is BSD licensed so anyone can use the definitions
 * to implement compatible drivers/servers:
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
 * THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
 * ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
 * LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS
 * FOR A PARTICULAR PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL IBM OR
 * CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
 * SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
 * LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF
 * USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND
 * ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
 * OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT
 * OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! `viogpu(4)`: the virtio GPU (`viogpu* at virtio?`) as a `wsdisplay(4)` frame buffer, and
//! `<dev/pv/viogpu.h>`, the virtio GPU device's protocol.
//!
//! Upstream: sys/dev/pv/viogpu.c @ 3ce1f3f79392
//! Upstream: sys/dev/pv/viogpu.h @ 3ce1f3f79392
//!
//! The driver needs a virtio 1.x device (QEMU's `virtio-gpu-pci`; the legacy virtio-mmio GPU
//! is refused). It asks the device for its display size, allocates a frame buffer of that
//! size in DMA memory, makes it resource 1 (`RESOURCE_CREATE_2D`, B8G8R8X8), backs the
//! resource with it (`RESOURCE_ATTACH_BACKING`, one entry) and shows it on scanout 0
//! (`SET_SCANOUT`). rasops draws into the frame buffer, and every 10 ms a timeout copies it to
//! the host and flushes it (`TRANSFER_TO_HOST_2D`, `RESOURCE_FLUSH`). Every command is sent on
//! the control queue from one page of DMA memory and polled to completion. The display always
//! claims the console (`wsdisplay_cnattach`), as the C does.
//!
//! ## Deviations
//! - The protocol structures are `#[repr(C, packed)]` with their C names in CamelCase; the
//!   header's `__le*` fields are native integers, as the C's typedefs make them (the kernel
//!   runs little-endian only). They go to and from the command page through [`GpuWire`]
//!   (bytes copied, the C's `memcpy`); their sizes are compile-time assertions.
//!   `virtio_gpu_resp_capset`'s flexible `capset_data[]` is not a member: the structure is its
//!   header.
//! - The enums `virtio_gpu_ctrl_type`, `virtio_gpu_shm_id` and `virtio_gpu_formats` are
//!   `u32` constants.
//! - The softc is `#[repr(C)]` with the device first; its members are `Cell`s, a rasops
//!   descriptor and a timeout, all-zero valid. `sc_cmd` and `sc_fb_dma_kva` are the mapped
//!   addresses (`*mut u8`), the maps `Option`s.
//! - The attach's error labels (`fb_unmap` .. `err`) are one [`ViogpuUnwind`] level; the
//!   cleanup runs every step at or below it, as the C falls through its labels. The C passes
//!   the address of the pointer (`(caddr_t)&sc->sc_fb_dma_kva`, `&sc->sc_cmd`) to
//!   `bus_dmamem_unmap`; the port passes the mapped address itself.
//! - `softintr_establish(IPL_TTY, viogpu_rx_soft, vsc)`'s handle is dropped as in C (nothing
//!   schedules it); `viogpu_rx_soft` is ported all the same.
//! - The return codes of the command functions (0 or 1) are `Result<(), Errno>` (`EIO` for 1);
//!   `viogpu_send_cmd` always succeeds or panics, so it returns nothing.
//! - `viogpu_wsioctl` returns `Ok(false)` where the C returns -1 (not handled), as
//!   `simplefb.rs`; `viogpu_wsmmap` returns `None` for the C's -1.
//! - Under feature `qemu` the attach arguments of the `wsdisplay` child also go to the frame
//!   buffer self-tests (`kern/selftest.rs`, `selftest=fb` and `selftest=wscons`), as in
//!   `simplefb.rs` and `efifb.rs`.
//! - `DPRINTF` and the `VIRTIO_DEBUG` messages are compiled behind `if VIRTIO_DEBUG > 0`; the
//!   `#if 0` `viogpu_fb_probe` (a drm(4) path) is not ported, as the C does not compile it.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};

use crate::dev::pv::virtio::{
    virtio_alloc_vq, virtio_attach_finish, virtio_check_vq, virtio_dequeue, virtio_dequeue_commit,
    virtio_enqueue_commit, virtio_enqueue_p, virtio_enqueue_prep, virtio_enqueue_reserve,
};
use crate::dev::pv::virtioreg::PCI_PRODUCT_VIRTIO_GPU;
use crate::dev::pv::virtiovar::{
    VIRTIO_CHILD_ERROR, VIRTIO_DEBUG, VirtioAttachArgs, VirtioFeatureName, VirtioSoftc, Virtqueue,
    virtio_negotiate_features,
};
use crate::dev::rasops::rasops::{
    RI_CENTER, RI_CLEAR, RI_VCONS, RI_WRONLY, RasopsInfo, rasops_alloc_screen, rasops_free_screen,
    rasops_getchar, rasops_init, rasops_list_font, rasops_load_font, rasops_scrollback,
    rasops_show_screen,
};
use crate::dev::wscons::wsconsio::{
    WSDISPLAY_TYPE_VIOGPU, WSDISPLAYIO_DEPTH_24_32, WSDISPLAYIO_GETPARAM,
    WSDISPLAYIO_GETSUPPORTEDDEPTH, WSDISPLAYIO_GINFO, WSDISPLAYIO_GTYPE, WSDISPLAYIO_GVIDEO,
    WSDISPLAYIO_LINEBYTES, WSDISPLAYIO_SETPARAM, WSDISPLAYIO_SMODE, WSDISPLAYIO_SVIDEO,
    WsdisplayFbinfo, WsdisplayParam,
};
use crate::dev::wscons::wsdisplay::{
    wsdisplay_cnattach, wsemuldisplaydevprint, wsemuldisplaydevsubmatch,
};
use crate::dev::wscons::wsdisplayvar::{
    WsParamFn, WsdisplayAccessops, WsemuldisplaydevAttachArgs, WsscreenDescr, WsscreenList,
    ws_get_param, ws_set_param,
};
use crate::kern::kern_softintr::softintr_establish;
use crate::kern::kern_timeout::{timeout_add_msec, timeout_set};
use crate::kern::subr_autoconf::config_found_sm;
use crate::kern::subr_prf::{panic, printf};
use crate::machine::bus::{
    BUS_DMA_ALLOCNOW, BUS_DMA_NOWAIT, BUS_DMA_WAITOK, BUS_DMA_ZERO, BUS_DMASYNC_POSTREAD,
    BUS_DMASYNC_POSTWRITE, BUS_DMASYNC_PREWRITE, BusDmaSegment, BusDmamap, bus_dmamap_create,
    bus_dmamap_destroy, bus_dmamap_load, bus_dmamap_sync, bus_dmamem_alloc, bus_dmamem_free,
    bus_dmamem_map, bus_dmamem_mmap, bus_dmamem_unmap,
};
use crate::machine::intr::{IPL_TTY, spltty, splx};
use crate::sys::device::{CfMatch, Cfattach, Cfdriver, DV_DULL, Device, Softc};
use crate::sys::errno::Errno;
use crate::sys::ioctl::{ioctl_arg, ioctl_ret};
use crate::sys::param::NBPG;
use crate::sys::proc::Proc;
use crate::sys::timeout::Timeout;
use crate::sys::types::Paddr;

// ---- <dev/pv/viogpu.h> ----

/// `VIRTIO_GPU_F_VIRGL`: `VIRTIO_GPU_CMD_CTX_*`, `VIRTIO_GPU_CMD_*_3D`.
pub const VIRTIO_GPU_F_VIRGL: u64 = 1 << 0;
/// `VIRTIO_GPU_F_EDID`: `VIRTIO_GPU_CMD_GET_EDID`.
pub const VIRTIO_GPU_F_EDID: u64 = 1 << 1;
/// `VIRTIO_GPU_F_RESOURCE_UUID`: `VIRTIO_GPU_CMD_RESOURCE_ASSIGN_UUID`.
pub const VIRTIO_GPU_F_RESOURCE_UUID: u64 = 1 << 2;
/// `VIRTIO_GPU_F_RESOURCE_BLOB`: `VIRTIO_GPU_CMD_RESOURCE_CREATE_BLOB`.
pub const VIRTIO_GPU_F_RESOURCE_BLOB: u64 = 1 << 3;

// enum virtio_gpu_ctrl_type
/// `VIRTIO_GPU_UNDEFINED`.
pub const VIRTIO_GPU_UNDEFINED: u32 = 0;
// 2d commands
/// `VIRTIO_GPU_CMD_GET_DISPLAY_INFO`.
pub const VIRTIO_GPU_CMD_GET_DISPLAY_INFO: u32 = 0x0100;
/// `VIRTIO_GPU_CMD_RESOURCE_CREATE_2D`.
pub const VIRTIO_GPU_CMD_RESOURCE_CREATE_2D: u32 = 0x0101;
/// `VIRTIO_GPU_CMD_RESOURCE_UNREF`.
pub const VIRTIO_GPU_CMD_RESOURCE_UNREF: u32 = 0x0102;
/// `VIRTIO_GPU_CMD_SET_SCANOUT`.
pub const VIRTIO_GPU_CMD_SET_SCANOUT: u32 = 0x0103;
/// `VIRTIO_GPU_CMD_RESOURCE_FLUSH`.
pub const VIRTIO_GPU_CMD_RESOURCE_FLUSH: u32 = 0x0104;
/// `VIRTIO_GPU_CMD_TRANSFER_TO_HOST_2D`.
pub const VIRTIO_GPU_CMD_TRANSFER_TO_HOST_2D: u32 = 0x0105;
/// `VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING`.
pub const VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING: u32 = 0x0106;
/// `VIRTIO_GPU_CMD_RESOURCE_DETACH_BACKING`.
pub const VIRTIO_GPU_CMD_RESOURCE_DETACH_BACKING: u32 = 0x0107;
/// `VIRTIO_GPU_CMD_GET_CAPSET_INFO`.
pub const VIRTIO_GPU_CMD_GET_CAPSET_INFO: u32 = 0x0108;
/// `VIRTIO_GPU_CMD_GET_CAPSET`.
pub const VIRTIO_GPU_CMD_GET_CAPSET: u32 = 0x0109;
/// `VIRTIO_GPU_CMD_GET_EDID`.
pub const VIRTIO_GPU_CMD_GET_EDID: u32 = 0x010a;
/// `VIRTIO_GPU_CMD_RESOURCE_ASSIGN_UUID`.
pub const VIRTIO_GPU_CMD_RESOURCE_ASSIGN_UUID: u32 = 0x010b;
/// `VIRTIO_GPU_CMD_RESOURCE_CREATE_BLOB`.
pub const VIRTIO_GPU_CMD_RESOURCE_CREATE_BLOB: u32 = 0x010c;
/// `VIRTIO_GPU_CMD_SET_SCANOUT_BLOB`.
pub const VIRTIO_GPU_CMD_SET_SCANOUT_BLOB: u32 = 0x010d;
// 3d commands
/// `VIRTIO_GPU_CMD_CTX_CREATE`.
pub const VIRTIO_GPU_CMD_CTX_CREATE: u32 = 0x0200;
/// `VIRTIO_GPU_CMD_CTX_DESTROY`.
pub const VIRTIO_GPU_CMD_CTX_DESTROY: u32 = 0x0201;
/// `VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE`.
pub const VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE: u32 = 0x0202;
/// `VIRTIO_GPU_CMD_CTX_DETACH_RESOURCE`.
pub const VIRTIO_GPU_CMD_CTX_DETACH_RESOURCE: u32 = 0x0203;
/// `VIRTIO_GPU_CMD_RESOURCE_CREATE_3D`.
pub const VIRTIO_GPU_CMD_RESOURCE_CREATE_3D: u32 = 0x0204;
/// `VIRTIO_GPU_CMD_TRANSFER_TO_HOST_3D`.
pub const VIRTIO_GPU_CMD_TRANSFER_TO_HOST_3D: u32 = 0x0205;
/// `VIRTIO_GPU_CMD_TRANSFER_FROM_HOST_3D`.
pub const VIRTIO_GPU_CMD_TRANSFER_FROM_HOST_3D: u32 = 0x0206;
/// `VIRTIO_GPU_CMD_SUBMIT_3D`.
pub const VIRTIO_GPU_CMD_SUBMIT_3D: u32 = 0x0207;
/// `VIRTIO_GPU_CMD_RESOURCE_MAP_BLOB`.
pub const VIRTIO_GPU_CMD_RESOURCE_MAP_BLOB: u32 = 0x0208;
/// `VIRTIO_GPU_CMD_RESOURCE_UNMAP_BLOB`.
pub const VIRTIO_GPU_CMD_RESOURCE_UNMAP_BLOB: u32 = 0x0209;
// cursor commands
/// `VIRTIO_GPU_CMD_UPDATE_CURSOR`.
pub const VIRTIO_GPU_CMD_UPDATE_CURSOR: u32 = 0x0300;
/// `VIRTIO_GPU_CMD_MOVE_CURSOR`.
pub const VIRTIO_GPU_CMD_MOVE_CURSOR: u32 = 0x0301;
// success responses
/// `VIRTIO_GPU_RESP_OK_NODATA`.
pub const VIRTIO_GPU_RESP_OK_NODATA: u32 = 0x1100;
/// `VIRTIO_GPU_RESP_OK_DISPLAY_INFO`.
pub const VIRTIO_GPU_RESP_OK_DISPLAY_INFO: u32 = 0x1101;
/// `VIRTIO_GPU_RESP_OK_CAPSET_INFO`.
pub const VIRTIO_GPU_RESP_OK_CAPSET_INFO: u32 = 0x1102;
/// `VIRTIO_GPU_RESP_OK_CAPSET`.
pub const VIRTIO_GPU_RESP_OK_CAPSET: u32 = 0x1103;
/// `VIRTIO_GPU_RESP_OK_EDID`.
pub const VIRTIO_GPU_RESP_OK_EDID: u32 = 0x1104;
/// `VIRTIO_GPU_RESP_OK_RESOURCE_UUID`.
pub const VIRTIO_GPU_RESP_OK_RESOURCE_UUID: u32 = 0x1105;
/// `VIRTIO_GPU_RESP_OK_MAP_INFO`.
pub const VIRTIO_GPU_RESP_OK_MAP_INFO: u32 = 0x1106;
// error responses
/// `VIRTIO_GPU_RESP_ERR_UNSPEC`.
pub const VIRTIO_GPU_RESP_ERR_UNSPEC: u32 = 0x1200;
/// `VIRTIO_GPU_RESP_ERR_OUT_OF_MEMORY`.
pub const VIRTIO_GPU_RESP_ERR_OUT_OF_MEMORY: u32 = 0x1201;
/// `VIRTIO_GPU_RESP_ERR_INVALID_SCANOUT_ID`.
pub const VIRTIO_GPU_RESP_ERR_INVALID_SCANOUT_ID: u32 = 0x1202;
/// `VIRTIO_GPU_RESP_ERR_INVALID_RESOURCE_ID`.
pub const VIRTIO_GPU_RESP_ERR_INVALID_RESOURCE_ID: u32 = 0x1203;
/// `VIRTIO_GPU_RESP_ERR_INVALID_CONTEXT_ID`.
pub const VIRTIO_GPU_RESP_ERR_INVALID_CONTEXT_ID: u32 = 0x1204;
/// `VIRTIO_GPU_RESP_ERR_INVALID_PARAMETER`.
pub const VIRTIO_GPU_RESP_ERR_INVALID_PARAMETER: u32 = 0x1205;

// enum virtio_gpu_shm_id
/// `VIRTIO_GPU_SHM_ID_UNDEFINED`.
pub const VIRTIO_GPU_SHM_ID_UNDEFINED: u32 = 0;
/// `VIRTIO_GPU_SHM_ID_HOST_VISIBLE`: `VIRTIO_GPU_CMD_RESOURCE_MAP_BLOB`,
/// `VIRTIO_GPU_CMD_RESOURCE_UNMAP_BLOB`.
pub const VIRTIO_GPU_SHM_ID_HOST_VISIBLE: u32 = 1;

/// `VIRTIO_GPU_FLAG_FENCE`.
pub const VIRTIO_GPU_FLAG_FENCE: u32 = 1 << 0;

/// `VIRTIO_GPU_MAX_SCANOUTS`.
pub const VIRTIO_GPU_MAX_SCANOUTS: usize = 16;

/// `VIRTIO_GPU_RESOURCE_FLAG_Y_0_TOP`.
pub const VIRTIO_GPU_RESOURCE_FLAG_Y_0_TOP: u32 = 1 << 0;

/// `VIRTIO_GPU_CAPSET_VIRGL`.
pub const VIRTIO_GPU_CAPSET_VIRGL: u32 = 1;
/// `VIRTIO_GPU_CAPSET_VIRGL2`.
pub const VIRTIO_GPU_CAPSET_VIRGL2: u32 = 2;

/// `VIRTIO_GPU_EVENT_DISPLAY`.
pub const VIRTIO_GPU_EVENT_DISPLAY: u32 = 1 << 0;

// enum virtio_gpu_formats: simple formats for fbcon/X use
/// `VIRTIO_GPU_FORMAT_B8G8R8A8_UNORM`.
pub const VIRTIO_GPU_FORMAT_B8G8R8A8_UNORM: u32 = 1;
/// `VIRTIO_GPU_FORMAT_B8G8R8X8_UNORM`.
pub const VIRTIO_GPU_FORMAT_B8G8R8X8_UNORM: u32 = 2;
/// `VIRTIO_GPU_FORMAT_A8R8G8B8_UNORM`.
pub const VIRTIO_GPU_FORMAT_A8R8G8B8_UNORM: u32 = 3;
/// `VIRTIO_GPU_FORMAT_X8R8G8B8_UNORM`.
pub const VIRTIO_GPU_FORMAT_X8R8G8B8_UNORM: u32 = 4;
/// `VIRTIO_GPU_FORMAT_R8G8B8A8_UNORM`.
pub const VIRTIO_GPU_FORMAT_R8G8B8A8_UNORM: u32 = 67;
/// `VIRTIO_GPU_FORMAT_X8B8G8R8_UNORM`.
pub const VIRTIO_GPU_FORMAT_X8B8G8R8_UNORM: u32 = 68;
/// `VIRTIO_GPU_FORMAT_A8B8G8R8_UNORM`.
pub const VIRTIO_GPU_FORMAT_A8B8G8R8_UNORM: u32 = 121;
/// `VIRTIO_GPU_FORMAT_R8G8B8X8_UNORM`.
pub const VIRTIO_GPU_FORMAT_R8G8B8X8_UNORM: u32 = 134;

/// `VIRTIO_GPU_BLOB_MEM_GUEST`.
pub const VIRTIO_GPU_BLOB_MEM_GUEST: u32 = 0x0001;
/// `VIRTIO_GPU_BLOB_MEM_HOST3D`.
pub const VIRTIO_GPU_BLOB_MEM_HOST3D: u32 = 0x0002;
/// `VIRTIO_GPU_BLOB_MEM_HOST3D_GUEST`.
pub const VIRTIO_GPU_BLOB_MEM_HOST3D_GUEST: u32 = 0x0003;
/// `VIRTIO_GPU_BLOB_FLAG_USE_MAPPABLE`.
pub const VIRTIO_GPU_BLOB_FLAG_USE_MAPPABLE: u32 = 0x0001;
/// `VIRTIO_GPU_BLOB_FLAG_USE_SHAREABLE`.
pub const VIRTIO_GPU_BLOB_FLAG_USE_SHAREABLE: u32 = 0x0002;
/// `VIRTIO_GPU_BLOB_FLAG_USE_CROSS_DEVICE`.
pub const VIRTIO_GPU_BLOB_FLAG_USE_CROSS_DEVICE: u32 = 0x0004;

/// `VIRTIO_GPU_MAP_CACHE_MASK`.
pub const VIRTIO_GPU_MAP_CACHE_MASK: u32 = 0x0f;
/// `VIRTIO_GPU_MAP_CACHE_NONE`.
pub const VIRTIO_GPU_MAP_CACHE_NONE: u32 = 0x00;
/// `VIRTIO_GPU_MAP_CACHE_CACHED`.
pub const VIRTIO_GPU_MAP_CACHE_CACHED: u32 = 0x01;
/// `VIRTIO_GPU_MAP_CACHE_UNCACHED`.
pub const VIRTIO_GPU_MAP_CACHE_UNCACHED: u32 = 0x02;
/// `VIRTIO_GPU_MAP_CACHE_WC`.
pub const VIRTIO_GPU_MAP_CACHE_WC: u32 = 0x03;

// ---- viogpu.c ----

/// `VIOGPU_HEIGHT`: the rows rasops is asked for.
const VIOGPU_HEIGHT: i32 = 160;
/// `VIOGPU_WIDTH`: the columns rasops is asked for.
const VIOGPU_WIDTH: i32 = 160;

/// `VQCTRL`: the control queue.
pub const VQCTRL: usize = 0;
/// `VQCURS`: the cursor queue.
pub const VQCURS: usize = 1;

/// The protocol structures: plain bytes on the wire.
///
/// # Safety
///
/// The implementing type is `#[repr(C, packed)]`, made of integers and arrays of them only
/// (every bit pattern is a value, no padding).
pub unsafe trait GpuWire: Copy {
    /// The structure's bytes, as `memcpy` sends them.
    fn wire_bytes(&self) -> &[u8] {
        // SAFETY: the trait's contract: no padding, so every byte is initialised.
        unsafe { core::slice::from_raw_parts(ptr::from_ref(self).cast::<u8>(), size_of::<Self>()) }
    }

    /// The structure read from `b`'s first bytes, as `memcpy` reads them.
    fn from_wire(b: &[u8]) -> Self {
        if b.len() < size_of::<Self>() {
            panic(format_args!("viogpu: short reply"));
        }
        // SAFETY: `b` holds the structure's bytes, and any bytes are a value (the trait's
        // contract); `read_unaligned` needs no alignment.
        unsafe { ptr::read_unaligned(b.as_ptr().cast::<Self>()) }
    }
}

/// Declares packed wire structures, `Default` (all zero) and [`GpuWire`].
macro_rules! gpu_wire {
    ($($(#[$m:meta])* pub struct $name:ident { $($(#[$fm:meta])* pub $f:ident: $t:ty,)* })*) => {
        $(
            $(#[$m])*
            #[repr(C, packed)]
            #[derive(Clone, Copy, Debug, Default)]
            pub struct $name { $($(#[$fm])* pub $f: $t,)* }

            // SAFETY: `#[repr(C, packed)]` of integers and arrays of integers or of other
            // such structures.
            unsafe impl GpuWire for $name {}
        )*
    };
}

gpu_wire! {
    /// `struct virtio_gpu_ctrl_hdr`.
    pub struct VirtioGpuCtrlHdr {
        /// `type`.
        pub r#type: u32,
        /// `flags`.
        pub flags: u32,
        /// `fence_id`.
        pub fence_id: u64,
        /// `ctx_id`.
        pub ctx_id: u32,
        /// `padding`.
        pub padding: u32,
    }

    /// `struct virtio_gpu_cursor_pos`: data passed in the cursor vq.
    pub struct VirtioGpuCursorPos {
        /// `scanout_id`.
        pub scanout_id: u32,
        /// `x`.
        pub x: u32,
        /// `y`.
        pub y: u32,
        /// `padding`.
        pub padding: u32,
    }

    /// `struct virtio_gpu_update_cursor`: `VIRTIO_GPU_CMD_UPDATE_CURSOR`,
    /// `VIRTIO_GPU_CMD_MOVE_CURSOR`.
    pub struct VirtioGpuUpdateCursor {
        /// `hdr`.
        pub hdr: VirtioGpuCtrlHdr,
        /// `pos`: update & move.
        pub pos: VirtioGpuCursorPos,
        /// `resource_id`: update only.
        pub resource_id: u32,
        /// `hot_x`: update only.
        pub hot_x: u32,
        /// `hot_y`: update only.
        pub hot_y: u32,
        /// `padding`.
        pub padding: u32,
    }

    /// `struct virtio_gpu_rect`: data passed in the control vq, 2d related.
    pub struct VirtioGpuRect {
        /// `x`.
        pub x: u32,
        /// `y`.
        pub y: u32,
        /// `width`.
        pub width: u32,
        /// `height`.
        pub height: u32,
    }

    /// `struct virtio_gpu_resource_unref`: `VIRTIO_GPU_CMD_RESOURCE_UNREF`.
    pub struct VirtioGpuResourceUnref {
        /// `hdr`.
        pub hdr: VirtioGpuCtrlHdr,
        /// `resource_id`.
        pub resource_id: u32,
        /// `padding`.
        pub padding: u32,
    }

    /// `struct virtio_gpu_resource_create_2d`: `VIRTIO_GPU_CMD_RESOURCE_CREATE_2D`, create a
    /// 2d resource with a format.
    pub struct VirtioGpuResourceCreate2d {
        /// `hdr`.
        pub hdr: VirtioGpuCtrlHdr,
        /// `resource_id`.
        pub resource_id: u32,
        /// `format`.
        pub format: u32,
        /// `width`.
        pub width: u32,
        /// `height`.
        pub height: u32,
    }

    /// `struct virtio_gpu_set_scanout`: `VIRTIO_GPU_CMD_SET_SCANOUT`.
    pub struct VirtioGpuSetScanout {
        /// `hdr`.
        pub hdr: VirtioGpuCtrlHdr,
        /// `r`.
        pub r: VirtioGpuRect,
        /// `scanout_id`.
        pub scanout_id: u32,
        /// `resource_id`.
        pub resource_id: u32,
    }

    /// `struct virtio_gpu_resource_flush`: `VIRTIO_GPU_CMD_RESOURCE_FLUSH`.
    pub struct VirtioGpuResourceFlush {
        /// `hdr`.
        pub hdr: VirtioGpuCtrlHdr,
        /// `r`.
        pub r: VirtioGpuRect,
        /// `resource_id`.
        pub resource_id: u32,
        /// `padding`.
        pub padding: u32,
    }

    /// `struct virtio_gpu_transfer_to_host_2d`: `VIRTIO_GPU_CMD_TRANSFER_TO_HOST_2D`, simple
    /// transfer to_host.
    pub struct VirtioGpuTransferToHost2d {
        /// `hdr`.
        pub hdr: VirtioGpuCtrlHdr,
        /// `r`.
        pub r: VirtioGpuRect,
        /// `offset`.
        pub offset: u64,
        /// `resource_id`.
        pub resource_id: u32,
        /// `padding`.
        pub padding: u32,
    }

    /// `struct virtio_gpu_mem_entry`.
    pub struct VirtioGpuMemEntry {
        /// `addr`.
        pub addr: u64,
        /// `length`.
        pub length: u32,
        /// `padding`.
        pub padding: u32,
    }

    /// `struct virtio_gpu_resource_attach_backing`: `VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING`.
    pub struct VirtioGpuResourceAttachBacking {
        /// `hdr`.
        pub hdr: VirtioGpuCtrlHdr,
        /// `resource_id`.
        pub resource_id: u32,
        /// `nr_entries`.
        pub nr_entries: u32,
    }

    /// `struct virtio_gpu_resource_detach_backing`: `VIRTIO_GPU_CMD_RESOURCE_DETACH_BACKING`.
    pub struct VirtioGpuResourceDetachBacking {
        /// `hdr`.
        pub hdr: VirtioGpuCtrlHdr,
        /// `resource_id`.
        pub resource_id: u32,
        /// `padding`.
        pub padding: u32,
    }

    /// `struct virtio_gpu_display_one`.
    pub struct VirtioGpuDisplayOne {
        /// `r`.
        pub r: VirtioGpuRect,
        /// `enabled`.
        pub enabled: u32,
        /// `flags`.
        pub flags: u32,
    }

    /// `struct virtio_gpu_resp_display_info`: `VIRTIO_GPU_RESP_OK_DISPLAY_INFO`.
    pub struct VirtioGpuRespDisplayInfo {
        /// `hdr`.
        pub hdr: VirtioGpuCtrlHdr,
        /// `pmodes`.
        pub pmodes: [VirtioGpuDisplayOne; VIRTIO_GPU_MAX_SCANOUTS],
    }

    /// `struct virtio_gpu_box`: data passed in the control vq, 3d related.
    pub struct VirtioGpuBox {
        /// `x`.
        pub x: u32,
        /// `y`.
        pub y: u32,
        /// `z`.
        pub z: u32,
        /// `w`.
        pub w: u32,
        /// `h`.
        pub h: u32,
        /// `d`.
        pub d: u32,
    }

    /// `struct virtio_gpu_transfer_host_3d`: `VIRTIO_GPU_CMD_TRANSFER_TO_HOST_3D`,
    /// `VIRTIO_GPU_CMD_TRANSFER_FROM_HOST_3D`.
    pub struct VirtioGpuTransferHost3d {
        /// `hdr`.
        pub hdr: VirtioGpuCtrlHdr,
        /// `box`.
        pub r#box: VirtioGpuBox,
        /// `offset`.
        pub offset: u64,
        /// `resource_id`.
        pub resource_id: u32,
        /// `level`.
        pub level: u32,
        /// `stride`.
        pub stride: u32,
        /// `layer_stride`.
        pub layer_stride: u32,
    }

    /// `struct virtio_gpu_resource_create_3d`: `VIRTIO_GPU_CMD_RESOURCE_CREATE_3D`.
    pub struct VirtioGpuResourceCreate3d {
        /// `hdr`.
        pub hdr: VirtioGpuCtrlHdr,
        /// `resource_id`.
        pub resource_id: u32,
        /// `target`.
        pub target: u32,
        /// `format`.
        pub format: u32,
        /// `bind`.
        pub bind: u32,
        /// `width`.
        pub width: u32,
        /// `height`.
        pub height: u32,
        /// `depth`.
        pub depth: u32,
        /// `array_size`.
        pub array_size: u32,
        /// `last_level`.
        pub last_level: u32,
        /// `nr_samples`.
        pub nr_samples: u32,
        /// `flags`.
        pub flags: u32,
        /// `padding`.
        pub padding: u32,
    }

    /// `struct virtio_gpu_ctx_destroy`: `VIRTIO_GPU_CMD_CTX_DESTROY`.
    pub struct VirtioGpuCtxDestroy {
        /// `hdr`.
        pub hdr: VirtioGpuCtrlHdr,
    }

    /// `struct virtio_gpu_ctx_resource`: `VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE`,
    /// `VIRTIO_GPU_CMD_CTX_DETACH_RESOURCE`.
    pub struct VirtioGpuCtxResource {
        /// `hdr`.
        pub hdr: VirtioGpuCtrlHdr,
        /// `resource_id`.
        pub resource_id: u32,
        /// `padding`.
        pub padding: u32,
    }

    /// `struct virtio_gpu_cmd_submit`: `VIRTIO_GPU_CMD_SUBMIT_3D`.
    pub struct VirtioGpuCmdSubmit {
        /// `hdr`.
        pub hdr: VirtioGpuCtrlHdr,
        /// `size`.
        pub size: u32,
        /// `padding`.
        pub padding: u32,
    }

    /// `struct virtio_gpu_get_capset_info`: `VIRTIO_GPU_CMD_GET_CAPSET_INFO`.
    pub struct VirtioGpuGetCapsetInfo {
        /// `hdr`.
        pub hdr: VirtioGpuCtrlHdr,
        /// `capset_index`.
        pub capset_index: u32,
        /// `padding`.
        pub padding: u32,
    }

    /// `struct virtio_gpu_resp_capset_info`: `VIRTIO_GPU_RESP_OK_CAPSET_INFO`.
    pub struct VirtioGpuRespCapsetInfo {
        /// `hdr`.
        pub hdr: VirtioGpuCtrlHdr,
        /// `capset_id`.
        pub capset_id: u32,
        /// `capset_max_version`.
        pub capset_max_version: u32,
        /// `capset_max_size`.
        pub capset_max_size: u32,
        /// `padding`.
        pub padding: u32,
    }

    /// `struct virtio_gpu_get_capset`: `VIRTIO_GPU_CMD_GET_CAPSET`.
    pub struct VirtioGpuGetCapset {
        /// `hdr`.
        pub hdr: VirtioGpuCtrlHdr,
        /// `capset_id`.
        pub capset_id: u32,
        /// `capset_version`.
        pub capset_version: u32,
    }

    /// `struct virtio_gpu_resp_capset`: `VIRTIO_GPU_RESP_OK_CAPSET`; its flexible
    /// `capset_data[]` follows the header.
    pub struct VirtioGpuRespCapset {
        /// `hdr`.
        pub hdr: VirtioGpuCtrlHdr,
    }

    /// `struct virtio_gpu_cmd_get_edid`: `VIRTIO_GPU_CMD_GET_EDID`.
    pub struct VirtioGpuCmdGetEdid {
        /// `hdr`.
        pub hdr: VirtioGpuCtrlHdr,
        /// `scanout`.
        pub scanout: u32,
        /// `padding`.
        pub padding: u32,
    }

    /// `struct virtio_gpu_config`.
    pub struct VirtioGpuConfig {
        /// `events_read`.
        pub events_read: u32,
        /// `events_clear`.
        pub events_clear: u32,
        /// `num_scanouts`.
        pub num_scanouts: u32,
        /// `num_capsets`.
        pub num_capsets: u32,
    }

    /// `struct virtio_gpu_resource_assign_uuid`: `VIRTIO_GPU_CMD_RESOURCE_ASSIGN_UUID`.
    pub struct VirtioGpuResourceAssignUuid {
        /// `hdr`.
        pub hdr: VirtioGpuCtrlHdr,
        /// `resource_id`.
        pub resource_id: u32,
        /// `padding`.
        pub padding: u32,
    }

    /// `struct virtio_gpu_resp_resource_uuid`: `VIRTIO_GPU_RESP_OK_RESOURCE_UUID`.
    pub struct VirtioGpuRespResourceUuid {
        /// `hdr`.
        pub hdr: VirtioGpuCtrlHdr,
        /// `uuid`.
        pub uuid: [u8; 16],
    }

    /// `struct virtio_gpu_resource_create_blob`: `VIRTIO_GPU_CMD_RESOURCE_CREATE_BLOB`;
    /// `nr_entries` `struct virtio_gpu_mem_entry`s follow.
    pub struct VirtioGpuResourceCreateBlob {
        /// `hdr`.
        pub hdr: VirtioGpuCtrlHdr,
        /// `resource_id`.
        pub resource_id: u32,
        /// `blob_mem`: zero is invalid blob mem.
        pub blob_mem: u32,
        /// `blob_flags`.
        pub blob_flags: u32,
        /// `nr_entries`.
        pub nr_entries: u32,
        /// `blob_id`.
        pub blob_id: u64,
        /// `size`.
        pub size: u64,
    }

    /// `struct virtio_gpu_set_scanout_blob`: `VIRTIO_GPU_CMD_SET_SCANOUT_BLOB`.
    pub struct VirtioGpuSetScanoutBlob {
        /// `hdr`.
        pub hdr: VirtioGpuCtrlHdr,
        /// `r`.
        pub r: VirtioGpuRect,
        /// `scanout_id`.
        pub scanout_id: u32,
        /// `resource_id`.
        pub resource_id: u32,
        /// `width`.
        pub width: u32,
        /// `height`.
        pub height: u32,
        /// `format`.
        pub format: u32,
        /// `padding`.
        pub padding: u32,
        /// `strides`.
        pub strides: [u32; 4],
        /// `offsets`.
        pub offsets: [u32; 4],
    }

    /// `struct virtio_gpu_resource_map_blob`: `VIRTIO_GPU_CMD_RESOURCE_MAP_BLOB`.
    pub struct VirtioGpuResourceMapBlob {
        /// `hdr`.
        pub hdr: VirtioGpuCtrlHdr,
        /// `resource_id`.
        pub resource_id: u32,
        /// `padding`.
        pub padding: u32,
        /// `offset`.
        pub offset: u64,
    }

    /// `struct virtio_gpu_resp_map_info`: `VIRTIO_GPU_RESP_OK_MAP_INFO`.
    pub struct VirtioGpuRespMapInfo {
        /// `hdr`.
        pub hdr: VirtioGpuCtrlHdr,
        /// `map_info`.
        pub map_info: u32,
        /// `padding`.
        pub padding: u32,
    }

    /// `struct virtio_gpu_resource_unmap_blob`: `VIRTIO_GPU_CMD_RESOURCE_UNMAP_BLOB`.
    pub struct VirtioGpuResourceUnmapBlob {
        /// `hdr`.
        pub hdr: VirtioGpuCtrlHdr,
        /// `resource_id`.
        pub resource_id: u32,
        /// `padding`.
        pub padding: u32,
    }

    /// `struct virtio_gpu_resource_attach_backing_entries`: `viogpu_attach_backing`'s
    /// command, the header with its one entry.
    pub struct VirtioGpuResourceAttachBackingEntries {
        /// `hdr`.
        pub hdr: VirtioGpuCtrlHdr,
        /// `resource_id`.
        pub resource_id: u32,
        /// `nr_entries`.
        pub nr_entries: u32,
        /// `entries`.
        pub entries: [VirtioGpuMemEntry; 1],
    }
}

/// `struct virtio_gpu_ctx_create`: `VIRTIO_GPU_CMD_CTX_CREATE`.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct VirtioGpuCtxCreate {
    /// `hdr`.
    pub hdr: VirtioGpuCtrlHdr,
    /// `nlen`.
    pub nlen: u32,
    /// `padding`.
    pub padding: u32,
    /// `debug_name`.
    pub debug_name: [u8; 64],
}

// SAFETY: `#[repr(C, packed)]` of integers and byte arrays.
unsafe impl GpuWire for VirtioGpuCtxCreate {}

/// `struct virtio_gpu_resp_edid`: `VIRTIO_GPU_RESP_OK_EDID`.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct VirtioGpuRespEdid {
    /// `hdr`.
    pub hdr: VirtioGpuCtrlHdr,
    /// `size`.
    pub size: u32,
    /// `padding`.
    pub padding: u32,
    /// `edid`.
    pub edid: [u8; 1024],
}

// SAFETY: `#[repr(C, packed)]` of integers and byte arrays.
unsafe impl GpuWire for VirtioGpuRespEdid {}

/// The attach's error labels, deepest first: the cleanup runs every step at or below the
/// level it starts from, as the C falls through `fb_unmap:` .. `err:`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ViogpuUnwind {
    /// `err:`: the child failed.
    Err,
    /// `errdma:`: ": DMA setup failed".
    Errdma,
    /// `destroy:`: the command map.
    Destroy,
    /// `free:`: the command page.
    Free,
    /// `unmap:`: the command page's mapping.
    Unmap,
    /// `fb_destroy:`: the frame buffer map.
    FbDestroy,
    /// `fb_free:`: the frame buffer memory.
    FbFree,
    /// `fb_unmap:`: the frame buffer's mapping.
    FbUnmap,
}

/// `struct viogpu_softc`.
#[repr(C)]
pub struct ViogpuSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `sc_virtio`: the parent.
    pub sc_virtio: Cell<Option<&'static VirtioSoftc>>,
    /// `sc_vqs`: [`VQCTRL`] and [`VQCURS`].
    pub sc_vqs: [Virtqueue; 2],

    /// `sc_dma_seg`: the command page.
    pub sc_dma_seg: [Cell<BusDmaSegment>; 1],
    /// `sc_dma_map`.
    pub sc_dma_map: Cell<Option<&'static BusDmamap>>,
    /// `sc_dma_size`.
    pub sc_dma_size: Cell<usize>,
    /// `sc_cmd`: the command page, mapped.
    pub sc_cmd: Cell<*mut u8>,
    /// `sc_fence_id`.
    pub sc_fence_id: Cell<i32>,

    /// `sc_fb_width`.
    pub sc_fb_width: Cell<i32>,
    /// `sc_fb_height`.
    pub sc_fb_height: Cell<i32>,
    /// `sc_fb_dma_seg`.
    pub sc_fb_dma_seg: [Cell<BusDmaSegment>; 1],
    /// `sc_fb_dma_map`.
    pub sc_fb_dma_map: Cell<Option<&'static BusDmamap>>,
    /// `sc_fb_dma_size`.
    pub sc_fb_dma_size: Cell<usize>,
    /// `sc_fb_dma_kva`: the frame buffer, mapped.
    pub sc_fb_dma_kva: Cell<*mut u8>,

    /// `sc_ri`.
    pub sc_ri: RasopsInfo,
    /// `sc_wsd`.
    pub sc_wsd: Cell<WsscreenDescr>,
    /// `sc_wsl`.
    pub sc_wsl: Cell<WsscreenList>,
    /// `sc_scrlist`.
    pub sc_scrlist: Cell<[*const WsscreenDescr; 1]>,
    /// `console`.
    pub console: Cell<i32>,
    /// `primary`.
    pub primary: Cell<i32>,

    /// `sc_timo`: the repaint.
    pub sc_timo: Timeout,
}

impl ViogpuSoftc {
    /// `sc->sc_virtio`, set by the attach.
    fn virtio(&self) -> &'static VirtioSoftc {
        match self.sc_virtio.get() {
            Some(vsc) => vsc,
            None => panic(format_args!("viogpu: no virtio softc")),
        }
    }

    /// `sc->sc_dma_map`, which a running device has.
    fn dma_map(&self) -> &'static BusDmamap {
        match self.sc_dma_map.get() {
            Some(map) => map,
            None => panic(format_args!("viogpu: no command map")),
        }
    }

    /// `sc->sc_fb_dma_map`, which a running device has.
    fn fb_dma_map(&self) -> &'static BusDmamap {
        match self.sc_fb_dma_map.get() {
            Some(map) => map,
            None => panic(format_args!("viogpu: no frame buffer map")),
        }
    }

    /// `sc->sc_cmd[off .. off + len]`.
    ///
    /// # Safety
    ///
    /// No other reference to the command page is live (commands are sent one at a time, at
    /// spltty under the kernel lock).
    #[allow(clippy::mut_from_ref)] // the command page is DMA memory, not part of `self`
    unsafe fn cmd_bytes(&self, off: usize, len: usize) -> &mut [u8] {
        let base = self.sc_cmd.get();
        if base.is_null() || off + len > self.sc_dma_size.get() {
            panic(format_args!(
                "{}: command of {} bytes past the command page",
                self.sc_dev.xname(),
                off + len
            ));
        }
        // SAFETY: the mapped command page holds `sc_dma_size` bytes; the caller's guarantee
        // for exclusivity.
        unsafe { core::slice::from_raw_parts_mut(base.add(off), len) }
    }
}

// SAFETY: `#[repr(C)]` with the device first; the virtqueues, the rasops descriptor and the
// timeout are all-zero valid (`dev/pv/virtiovar.rs`, `dev/rasops/rasops.rs`,
// `sys/timeout.rs`), and every other member is a `Cell` of an integer, a raw pointer, a DMA
// segment, a screen descriptor or list (zero-valid, as in `simplefb.rs`) or an `Option` of a
// reference.
unsafe impl Softc for ViogpuSoftc {}

/// `viogpu_feature_names[]` with `VIRTIO_DEBUG`.
static VIOGPU_FEATURE_NAMES_DEBUG: [VirtioFeatureName; 2] = [
    VirtioFeatureName {
        bit: VIRTIO_GPU_F_VIRGL,
        name: "VirGL",
    },
    VirtioFeatureName {
        bit: VIRTIO_GPU_F_EDID,
        name: "EDID",
    },
];

/// `viogpu_accessops`.
pub static VIOGPU_ACCESSOPS: WsdisplayAccessops = WsdisplayAccessops {
    ioctl: Some(viogpu_wsioctl),
    mmap: Some(viogpu_wsmmap),
    alloc_screen: Some(viogpu_alloc_screen),
    free_screen: Some(rasops_free_screen),
    show_screen: Some(rasops_show_screen),
    getchar: Some(rasops_getchar),
    load_font: Some(rasops_load_font),
    list_font: Some(rasops_list_font),
    scrollback: Some(rasops_scrollback),
    ..WsdisplayAccessops::EMPTY
};

/// `viogpu_ca`.
pub static VIOGPU_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<ViogpuSoftc>(),
    ca_match: Some(viogpu_match),
    ca_attach: viogpu_attach,
    ca_detach: None,
    ca_activate: None,
};

/// `viogpu_cd`.
pub static VIOGPU_CD: Cfdriver = Cfdriver::new(b"viogpu", DV_DULL, 0);

/// `viogpu_feature_names`: the table `virtio_negotiate_features` is given, empty unless
/// `VIRTIO_DEBUG`.
fn viogpu_feature_names() -> &'static [VirtioFeatureName] {
    if VIRTIO_DEBUG > 0 {
        &VIOGPU_FEATURE_NAMES_DEBUG
    } else {
        &[]
    }
}

/// `(struct viogpu_softc *)vsc->sc_child`.
fn viogpu_child(vsc: &VirtioSoftc) -> &'static ViogpuSoftc {
    let c = vsc.sc_child.get();
    if c.is_null() || c == VIRTIO_CHILD_ERROR {
        panic(format_args!("viogpu: virtio without its child"));
    }
    // SAFETY: viogpu_attach stored its own device, the head of a `ViogpuSoftc` that lives as
    // long as the kernel; only viogpu's handlers call this.
    unsafe { &*c.cast::<ViogpuSoftc>() }
}

/// `viogpu_match`: the virtio GPU.
pub fn viogpu_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: virtio transports attach their children with arguments that begin with a
    // `virtio_attach_args`.
    let va = unsafe { &*aux.cast::<VirtioAttachArgs>() };
    i32::from(va.va_devid == PCI_PRODUCT_VIRTIO_GPU)
}

/// One `bus_dmamap_create` + `bus_dmamem_alloc` + `bus_dmamem_map` + `bus_dmamap_load` of
/// the attach, each step's failure the level to unwind from.
struct DmaArea {
    /// The map, once made.
    map: Option<&'static BusDmamap>,
    /// The mapped memory, once mapped.
    kva: Option<NonNull<u8>>,
}

/// `viogpu_attach`.
pub fn viogpu_attach(parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `self_` was made for `viogpu_ca`, whose softc is a `ViogpuSoftc`; softcs are
    // never freed while the device exists, so the softc may be borrowed for 'static.
    let sc: &'static ViogpuSoftc = unsafe { &*ptr::from_ref(self_.softc::<ViogpuSoftc>()) };
    let Some(parent) = parent else {
        return;
    };
    // SAFETY: `viogpu* at virtio?`: the parent is a virtio transport, whose softc begins with
    // the `struct virtio_softc`; it outlives its child.
    let vsc: &'static VirtioSoftc = unsafe { &*ptr::from_ref(parent.softc::<VirtioSoftc>()) };
    let va = aux.cast::<VirtioAttachArgs>();
    let ri: &'static RasopsInfo = &sc.sc_ri;

    if !vsc.sc_child.get().is_null() {
        printf(format_args!(
            ": child already attached for {}\n",
            parent.xname()
        ));
        return;
    }
    vsc.sc_child.set(ptr::from_ref(self_));

    let Err(level) = viogpu_attach_setup(sc, vsc, va) else {
        // The frame buffer is up: rasops, the console and the wsdisplay child.
        viogpu_attach_display(sc, self_, ri);
        return;
    };

    let dmat = vsc.dmat();
    if level >= ViogpuUnwind::FbUnmap
        && let Some(kva) = NonNull::new(sc.sc_fb_dma_kva.get())
    {
        // SAFETY: mapped by `bus_dmamem_map` below and no longer used (the device never got
        // the frame buffer: its setup failed).
        unsafe { bus_dmamem_unmap(dmat, kva, sc.sc_fb_dma_size.get()) };
    }
    if level >= ViogpuUnwind::FbFree {
        // SAFETY: the segment `bus_dmamem_alloc` gave, unmapped above.
        unsafe { bus_dmamem_free(dmat, &[sc.sc_fb_dma_seg[0].get()]) };
    }
    if level >= ViogpuUnwind::FbDestroy
        && let Some(map) = sc.sc_fb_dma_map.take()
    {
        // SAFETY: made by `bus_dmamap_create` below; nothing uses it any more.
        unsafe { bus_dmamap_destroy(dmat, NonNull::from(map)) };
    }
    if level >= ViogpuUnwind::Unmap
        && let Some(kva) = NonNull::new(sc.sc_cmd.get())
    {
        // SAFETY: the command page's mapping; no command is in flight after a failure.
        unsafe { bus_dmamem_unmap(dmat, kva, sc.sc_dma_size.get()) };
    }
    if level >= ViogpuUnwind::Free {
        // SAFETY: the command page's segment, unmapped above.
        unsafe { bus_dmamem_free(dmat, &[sc.sc_dma_seg[0].get()]) };
    }
    if level >= ViogpuUnwind::Destroy
        && let Some(map) = sc.sc_dma_map.take()
    {
        // SAFETY: made by `bus_dmamap_create`; nothing uses it any more.
        unsafe { bus_dmamap_destroy(dmat, NonNull::from(map)) };
    }
    if level >= ViogpuUnwind::Errdma {
        printf(format_args!(": DMA setup failed\n"));
    }
    // err:
    vsc.sc_child.set(VIRTIO_CHILD_ERROR);
}

/// Makes one DMA area of `size` bytes aligned to `align`: map, memory, mapping and load,
/// recording the map and the segment in `map_cell`/`seg`. The errors are the level to unwind
/// from, `base` being the level that undoes nothing of this area, and the message the C
/// prints at each step (`msgs`, `None` for none).
fn viogpu_dma_area(
    vsc: &VirtioSoftc,
    size: usize,
    align: usize,
    map_cell: &Cell<Option<&'static BusDmamap>>,
    seg: &Cell<BusDmaSegment>,
    levels: [ViogpuUnwind; 4],
    msgs: [Option<&str>; 4],
) -> Result<DmaArea, ViogpuUnwind> {
    let dmat = vsc.dmat();
    let fail = |i: usize| {
        if let Some(m) = msgs[i] {
            printf(format_args!("{m}"));
        }
        levels[i]
    };
    let map = bus_dmamap_create(dmat, size, 1, size, 0, BUS_DMA_NOWAIT | BUS_DMA_ALLOCNOW)
        .map_err(|_| fail(0))?;
    map_cell.set(Some(map));
    let mut segs = [BusDmaSegment::default()];
    let nsegs = bus_dmamem_alloc(
        dmat,
        size,
        align,
        0,
        &mut segs,
        BUS_DMA_NOWAIT | BUS_DMA_ZERO,
    )
    .map_err(|_| fail(1))?;
    seg.set(segs[0]);
    let kva =
        bus_dmamem_map(dmat, &mut segs[..nsegs], size, BUS_DMA_NOWAIT).map_err(|_| fail(2))?;
    // SAFETY: `kva` maps `size` bytes of DMA memory that stays allocated until the error path
    // or for good.
    unsafe { bus_dmamap_load(dmat, map, kva.as_ptr(), size, None, BUS_DMA_NOWAIT) }
        .map_err(|_| fail(3))?;
    Ok(DmaArea {
        map: Some(map),
        kva: Some(kva),
    })
}

/// The part of `viogpu_attach` that can fail: features, queues, the command page, the display
/// size, the frame buffer and the scanout.
fn viogpu_attach_setup(
    sc: &'static ViogpuSoftc,
    vsc: &'static VirtioSoftc,
    va: *mut VirtioAttachArgs,
) -> Result<(), ViogpuUnwind> {
    use ViogpuUnwind as U;

    if virtio_negotiate_features(vsc, Some(viogpu_feature_names())).is_err() {
        return Err(U::Err);
    }
    if vsc.sc_version_1.get() == 0 {
        printf(format_args!(": requires virtio version 1\n"));
        return Err(U::Err);
    }

    vsc.sc_ipl.set(IPL_TTY);
    let _ = softintr_establish(
        IPL_TTY,
        viogpu_rx_soft,
        ptr::from_ref(vsc).cast_mut().cast(),
    );
    sc.sc_virtio.set(Some(vsc));

    // allocate command and cursor virtqueues
    vsc.sc_vqs.set(sc.sc_vqs.as_ptr().cast_mut());
    if virtio_alloc_vq(vsc, &sc.sc_vqs[VQCTRL], VQCTRL as i32, 1, "control").is_err() {
        printf(format_args!(": alloc_vq failed\n"));
        return Err(U::Err);
    }
    sc.sc_vqs[VQCTRL].vq_done.set(Some(viogpu_vq_done));

    if virtio_alloc_vq(vsc, &sc.sc_vqs[VQCURS], VQCURS as i32, 1, "cursor").is_err() {
        printf(format_args!(": alloc_vq failed\n"));
        return Err(U::Err);
    }
    vsc.sc_nvqs.set(sc.sc_vqs.len() as i32);

    // setup DMA space for sending commands
    sc.sc_dma_size.set(NBPG);
    let cmd = viogpu_dma_area(
        vsc,
        sc.sc_dma_size.get(),
        16,
        &sc.sc_dma_map,
        &sc.sc_dma_seg[0],
        [U::Errdma, U::Destroy, U::Free, U::Unmap],
        [
            Some(": create failed"),
            Some(": alloc failed"),
            Some(": map failed"),
            Some(": load failed"),
        ],
    )?;
    sc.sc_cmd
        .set(cmd.kva.map_or(ptr::null_mut(), NonNull::as_ptr));

    if virtio_attach_finish(vsc, va).is_err() {
        return Err(U::Unmap);
    }

    if viogpu_get_display_info(sc).is_err() {
        return Err(U::Unmap);
    }

    // setup DMA space for actual framebuffer
    sc.sc_fb_dma_size
        .set(sc.sc_fb_width.get() as usize * sc.sc_fb_height.get() as usize * 4);
    let fb = viogpu_dma_area(
        vsc,
        sc.sc_fb_dma_size.get(),
        1024,
        &sc.sc_fb_dma_map,
        &sc.sc_fb_dma_seg[0],
        [U::Unmap, U::FbDestroy, U::FbFree, U::FbUnmap],
        [None; 4],
    )?;
    sc.sc_fb_dma_kva
        .set(fb.kva.map_or(ptr::null_mut(), NonNull::as_ptr));
    let _ = (cmd.map, fb.map);

    if viogpu_create_2d(sc, 1, sc.sc_fb_width.get(), sc.sc_fb_height.get()).is_err() {
        return Err(U::FbUnmap);
    }

    if viogpu_attach_backing(sc, 1, sc.fb_dma_map()).is_err() {
        return Err(U::FbUnmap);
    }

    if viogpu_set_scanout(sc, 0, 1, sc.sc_fb_width.get(), sc.sc_fb_height.get()).is_err() {
        return Err(U::FbUnmap);
    }

    Ok(())
}

/// The rest of `viogpu_attach`: rasops over the frame buffer, the console, the first repaint
/// and the `wsdisplay` child.
fn viogpu_attach_display(sc: &'static ViogpuSoftc, self_: &Device, ri: &'static RasopsInfo) {
    sc.console.set(1);

    ri.ri_hw.set(ptr::from_ref(sc).cast_mut().cast());
    ri.ri_bits.set(sc.sc_fb_dma_kva.get());
    ri.ri_flg.set(RI_VCONS | RI_CENTER | RI_CLEAR | RI_WRONLY);
    ri.ri_depth.set(32);
    ri.ri_width.set(sc.sc_fb_width.get());
    ri.ri_height.set(sc.sc_fb_height.get());
    ri.ri_stride.set(ri.ri_width.get() * ri.ri_depth.get() / 8);
    ri.ri_bpos.set(0); // B8G8R8X8
    ri.ri_bnum.set(8);
    ri.ri_gpos.set(8);
    ri.ri_gnum.set(8);
    ri.ri_rpos.set(16);
    ri.ri_rnum.set(8);
    // The C ignores rasops_init's result.
    let _ = rasops_init(ri, VIOGPU_HEIGHT, VIOGPU_WIDTH);

    let mut wsd = WsscreenDescr::new(b"std");
    ri.fill_descr(&mut wsd);
    sc.sc_wsd.set(wsd);

    sc.sc_scrlist.set([sc.sc_wsd.as_ptr().cast_const()]);
    sc.sc_wsl.set(WsscreenList {
        nscreens: 1,
        screens: sc.sc_scrlist.as_ptr().cast::<*const WsscreenDescr>(),
    });

    printf(format_args!(
        ": {}x{}, {}bpp\n",
        ri.ri_width.get(),
        ri.ri_height.get(),
        ri.ri_depth.get()
    ));

    timeout_set(
        &sc.sc_timo,
        viogpu_repaint,
        ptr::from_ref(sc).cast_mut().cast(),
    );
    viogpu_repaint(ptr::from_ref(sc).cast_mut().cast());

    if sc.console.get() != 0 {
        // SAFETY: with RI_VCONS the emulops take the active screen as their cookie.
        let defattr = unsafe { ri.ops_pack_attr(ri.ri_active.get().cast(), 0, 0, 0) }.unwrap_or(0);
        // SAFETY: `sc_wsd` was filled above and is only read from now on, in a softc that
        // lives for good; with RI_VCONS the active screen is the emulops' cookie.
        unsafe {
            wsdisplay_cnattach(
                &*sc.sc_wsd.as_ptr(),
                ri.ri_active.get().cast(),
                0,
                0,
                defattr,
            )
        };
    }

    let mut waa = WsemuldisplaydevAttachArgs {
        console: sc.console.get(),
        primary: 0,
        scrdata: sc.sc_wsl.as_ptr(),
        accessops: &VIOGPU_ACCESSOPS,
        accesscookie: ri.cookie(),
        defaultscreens: 0,
    };

    #[cfg(feature = "qemu")]
    crate::kern::selftest::fb_attached(self_, &waa);

    let _ = config_found_sm(
        self_,
        ptr::from_mut(&mut waa).cast(),
        Some(wsemuldisplaydevprint),
        Some(wsemuldisplaydevsubmatch),
    );
}

/// `viogpu_repaint`: copies the frame buffer to the host and shows it, every 10 ms.
pub fn viogpu_repaint(arg: *mut c_void) {
    // SAFETY: the attach set the timeout (and calls this) with its softc, never freed.
    let sc = unsafe { &*arg.cast_const().cast::<ViogpuSoftc>() };

    let s = spltty();

    let _ = viogpu_transfer_to_host_2d(
        sc,
        1,
        sc.sc_fb_width.get() as u32,
        sc.sc_fb_height.get() as u32,
    );
    let _ = viogpu_flush_resource(
        sc,
        1,
        sc.sc_fb_width.get() as u32,
        sc.sc_fb_height.get() as u32,
    );

    timeout_add_msec(&sc.sc_timo, 10);
    splx(s);
}

/// `viogpu_vq_done`: the control queue's `vq_done`: one finished command.
pub fn viogpu_vq_done(vq: &Virtqueue) -> i32 {
    let vsc = vq.owner();
    let sc = viogpu_child(vsc);

    let Ok((slot, _len)) = virtio_dequeue(vsc, vq) else {
        return 0;
    };

    bus_dmamap_sync(
        vsc.dmat(),
        sc.dma_map(),
        0,
        sc.sc_dma_size.get(),
        BUS_DMASYNC_POSTREAD,
    );

    virtio_dequeue_commit(vq, slot);

    1
}

/// `viogpu_rx_soft`: drains the control queue (established, never scheduled, as in C).
pub fn viogpu_rx_soft(arg: *mut c_void) {
    // SAFETY: established with the parent's `virtio_softc`, which lives for good.
    let vsc = unsafe { &*arg.cast_const().cast::<VirtioSoftc>() };
    let sc = viogpu_child(vsc);
    let vq = &sc.sc_vqs[VQCTRL];

    while let Ok((slot, len)) = virtio_dequeue(vsc, vq) {
        bus_dmamap_sync(
            vsc.dmat(),
            sc.dma_map(),
            slot as usize,
            len as usize,
            BUS_DMASYNC_POSTREAD,
        );
        virtio_dequeue_commit(vq, slot);
    }
}

/// `viogpu_send_cmd`: sends `cmd` on the control queue, fenced, and polls for the device's
/// reply, which it returns as `R`.
pub fn viogpu_send_cmd<C: GpuWire, R: GpuWire>(sc: &ViogpuSoftc, cmd: &C) -> R {
    let vsc = sc.virtio();
    let vq = vsc.vq(VQCTRL);
    let cmd_size = size_of::<C>();
    let ret_size = size_of::<R>();

    {
        // SAFETY: one command at a time (spltty, the kernel lock); nothing else holds the
        // page.
        let page = unsafe { sc.cmd_bytes(0, cmd_size + ret_size) };
        page[..cmd_size].copy_from_slice(cmd.wire_bytes());
        page[cmd_size..].fill(0);

        if VIRTIO_DEBUG >= 3 {
            printf(format_args!(
                "viogpu_send_cmd: [{cmd_size} -> {ret_size}]: "
            ));
            for b in &page[..cmd_size] {
                printf(format_args!(" {b:02x}"));
            }
            printf(format_args!("\n"));
        }

        let mut hdr = VirtioGpuCtrlHdr::from_wire(page);
        hdr.flags |= VIRTIO_GPU_FLAG_FENCE;
        sc.sc_fence_id.set(sc.sc_fence_id.get().wrapping_add(1));
        hdr.fence_id = sc.sc_fence_id.get() as u64;
        page[..size_of::<VirtioGpuCtrlHdr>()].copy_from_slice(hdr.wire_bytes());
    }

    let Ok(slot) = virtio_enqueue_prep(vq) else {
        panic(format_args!("{}: control vq busy", sc.sc_dev.xname()));
    };

    // SAFETY: the command page is mapped for good.
    if unsafe {
        bus_dmamap_load(
            vsc.dmat(),
            sc.dma_map(),
            sc.sc_cmd.get(),
            cmd_size + ret_size,
            None,
            BUS_DMA_NOWAIT,
        )
    }
    .is_err()
    {
        panic(format_args!("{}: dmamap load failed", sc.sc_dev.xname()));
    }

    if virtio_enqueue_reserve(vq, slot, sc.dma_map().dm_nsegs.get() + 1).is_err() {
        panic(format_args!("{}: control vq busy", sc.sc_dev.xname()));
    }

    bus_dmamap_sync(vsc.dmat(), sc.dma_map(), 0, cmd_size, BUS_DMASYNC_PREWRITE);

    virtio_enqueue_p(vq, slot, sc.dma_map(), 0, cmd_size, true);
    virtio_enqueue_p(vq, slot, sc.dma_map(), cmd_size, ret_size, false);
    virtio_enqueue_commit(vsc, vq, slot, true);

    while virtio_check_vq(vsc, vq) == 0 {
        core::hint::spin_loop();
    }

    bus_dmamap_sync(vsc.dmat(), sc.dma_map(), 0, cmd_size, BUS_DMASYNC_POSTWRITE);
    bus_dmamap_sync(
        vsc.dmat(),
        sc.dma_map(),
        cmd_size,
        ret_size,
        BUS_DMASYNC_POSTREAD,
    );

    // SAFETY: as above; the device is done with the page (the command completed). The reply
    // was written by DMA: read it through a volatile copy.
    let ret: R = unsafe {
        let page = sc.cmd_bytes(cmd_size, ret_size);
        ptr::read_volatile(page.as_ptr().cast::<R>())
    };
    let ret_hdr = VirtioGpuCtrlHdr::from_wire(ret.wire_bytes());

    let fence = ret_hdr.fence_id;
    if fence != sc.sc_fence_id.get() as u64 {
        printf(format_args!(
            "viogpu_send_cmd: return fence id not right ({:#x} != {:#x})\n",
            fence,
            sc.sc_fence_id.get()
        ));
    }

    ret
}

/// `viogpu_get_display_info`: the size of scanout 0, into `sc_fb_width`/`sc_fb_height`.
pub fn viogpu_get_display_info(sc: &ViogpuSoftc) -> Result<(), Errno> {
    let hdr = VirtioGpuCtrlHdr {
        r#type: VIRTIO_GPU_CMD_GET_DISPLAY_INFO,
        ..Default::default()
    };

    let info: VirtioGpuRespDisplayInfo = viogpu_send_cmd(sc, &hdr);

    let t = info.hdr.r#type;
    if t != VIRTIO_GPU_RESP_OK_DISPLAY_INFO {
        printf(format_args!(
            "{}: failed getting display info\n",
            sc.sc_dev.xname()
        ));
        return Err(Errno::EIO);
    }

    let pmodes = info.pmodes;
    let enabled = pmodes[0].enabled;
    if enabled == 0 {
        printf(format_args!(
            "{}: pmodes[0] is not enabled\n",
            sc.sc_dev.xname()
        ));
        return Err(Errno::EIO);
    }

    let r = pmodes[0].r;
    sc.sc_fb_width.set(r.width as i32);
    sc.sc_fb_height.set(r.height as i32);

    Ok(())
}

/// The `type` of a reply that must be `VIRTIO_GPU_RESP_OK_NODATA`; prints `what` failed.
fn viogpu_check_nodata(sc: &ViogpuSoftc, resp: &VirtioGpuCtrlHdr, what: &str) -> Result<(), Errno> {
    let t = resp.r#type;
    if t != VIRTIO_GPU_RESP_OK_NODATA {
        printf(format_args!(
            "{}: failed {}: {}\n",
            sc.sc_dev.xname(),
            what,
            t as i32
        ));
        return Err(Errno::EIO);
    }
    Ok(())
}

/// `viogpu_create_2d`: makes resource `resource_id`, `width` x `height` in B8G8R8X8.
pub fn viogpu_create_2d(
    sc: &ViogpuSoftc,
    resource_id: i32,
    width: i32,
    height: i32,
) -> Result<(), Errno> {
    let res = VirtioGpuResourceCreate2d {
        hdr: VirtioGpuCtrlHdr {
            r#type: VIRTIO_GPU_CMD_RESOURCE_CREATE_2D,
            ..Default::default()
        },
        resource_id: resource_id as u32,
        format: VIRTIO_GPU_FORMAT_B8G8R8X8_UNORM,
        width: width as u32,
        height: height as u32,
    };

    let resp: VirtioGpuCtrlHdr = viogpu_send_cmd(sc, &res);

    viogpu_check_nodata(sc, &resp, "CREATE_2D")
}

/// `viogpu_set_scanout`: shows resource `resource_id` on scanout `scanout_id`.
pub fn viogpu_set_scanout(
    sc: &ViogpuSoftc,
    scanout_id: i32,
    resource_id: i32,
    width: i32,
    height: i32,
) -> Result<(), Errno> {
    let ss = VirtioGpuSetScanout {
        hdr: VirtioGpuCtrlHdr {
            r#type: VIRTIO_GPU_CMD_SET_SCANOUT,
            ..Default::default()
        },
        r: VirtioGpuRect {
            width: width as u32,
            height: height as u32,
            ..Default::default()
        },
        scanout_id: scanout_id as u32,
        resource_id: resource_id as u32,
    };

    let resp: VirtioGpuCtrlHdr = viogpu_send_cmd(sc, &ss);

    viogpu_check_nodata(sc, &resp, "SET_SCANOUT")
}

/// `viogpu_attach_backing`: backs resource `resource_id` with `dmamap`'s first segment.
pub fn viogpu_attach_backing(
    sc: &ViogpuSoftc,
    resource_id: i32,
    dmamap: &BusDmamap,
) -> Result<(), Errno> {
    let seg = dmamap
        .dm_segs()
        .first()
        .map(|s| s.get())
        .unwrap_or_default();
    let backing = VirtioGpuResourceAttachBackingEntries {
        hdr: VirtioGpuCtrlHdr {
            r#type: VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING,
            ..Default::default()
        },
        resource_id: resource_id as u32,
        nr_entries: 1,
        entries: [VirtioGpuMemEntry {
            addr: seg.ds_addr as u64,
            length: seg.ds_len as u32,
            padding: 0,
        }],
    };

    if dmamap.dm_nsegs.get() > 1 {
        printf(format_args!(
            "viogpu_attach_backing: TODO: send all {} segs\n",
            dmamap.dm_nsegs.get()
        ));
    }

    if VIRTIO_DEBUG > 0 {
        let (addr, length) = (backing.entries[0].addr, backing.entries[0].length);
        printf(format_args!(
            "viogpu_attach_backing: backing addr {addr:#x} length {length}\n"
        ));
    }

    let resp: VirtioGpuCtrlHdr = viogpu_send_cmd(sc, &backing);

    viogpu_check_nodata(sc, &resp, "ATTACH_BACKING")
}

/// `viogpu_transfer_to_host_2d`: copies the frame buffer into resource `resource_id`.
pub fn viogpu_transfer_to_host_2d(
    sc: &ViogpuSoftc,
    resource_id: i32,
    width: u32,
    height: u32,
) -> Result<(), Errno> {
    let vsc = sc.virtio();
    let tth = VirtioGpuTransferToHost2d {
        hdr: VirtioGpuCtrlHdr {
            r#type: VIRTIO_GPU_CMD_TRANSFER_TO_HOST_2D,
            ..Default::default()
        },
        r: VirtioGpuRect {
            width,
            height,
            ..Default::default()
        },
        resource_id: resource_id as u32,
        ..Default::default()
    };

    bus_dmamap_sync(
        vsc.dmat(),
        sc.fb_dma_map(),
        0,
        sc.sc_fb_dma_size.get(),
        BUS_DMASYNC_PREWRITE,
    );

    let resp: VirtioGpuCtrlHdr = viogpu_send_cmd(sc, &tth);

    viogpu_check_nodata(sc, &resp, "TRANSFER_TO_HOST")?;

    bus_dmamap_sync(
        vsc.dmat(),
        sc.fb_dma_map(),
        0,
        sc.sc_fb_dma_size.get(),
        BUS_DMASYNC_POSTWRITE,
    );

    Ok(())
}

/// `viogpu_flush_resource`: shows what changed in resource `resource_id`.
pub fn viogpu_flush_resource(
    sc: &ViogpuSoftc,
    resource_id: i32,
    width: u32,
    height: u32,
) -> Result<(), Errno> {
    let flush = VirtioGpuResourceFlush {
        hdr: VirtioGpuCtrlHdr {
            r#type: VIRTIO_GPU_CMD_RESOURCE_FLUSH,
            ..Default::default()
        },
        r: VirtioGpuRect {
            width,
            height,
            ..Default::default()
        },
        resource_id: resource_id as u32,
        padding: 0,
    };

    let resp: VirtioGpuCtrlHdr = viogpu_send_cmd(sc, &flush);

    viogpu_check_nodata(sc, &resp, "RESOURCE_FLUSH")
}

/// `viogpu_wsioctl`.
///
/// # Safety
///
/// `v` is an attached viogpu's `RasopsInfo` (the access cookie); `data` is the kernel copy of
/// the command's argument.
pub unsafe fn viogpu_wsioctl(
    v: *mut c_void,
    cmd: u64,
    data: &mut [u8],
    _flag: i32,
    _p: Option<&Proc>,
) -> Result<bool, Errno> {
    // SAFETY: the caller's contract.
    let ri = unsafe { &*v.cast::<RasopsInfo>() };

    let param = |f: Option<WsParamFn>, data: &mut [u8]| {
        let Some(f) = f else {
            return Ok(false);
        };
        let mut dp: WsdisplayParam = ioctl_arg(data);
        let r = f(&mut dp);
        ioctl_ret(data, &dp);
        match r {
            0 => Ok(true),
            -1 => Ok(false),
            e => Err(Errno::from_raw(e).unwrap_or(Errno::EINVAL)),
        }
    };

    match cmd {
        WSDISPLAYIO_GETPARAM => return param(ws_get_param(), data),
        WSDISPLAYIO_SETPARAM => return param(ws_set_param(), data),
        WSDISPLAYIO_GTYPE => ioctl_ret(data, &WSDISPLAY_TYPE_VIOGPU),
        WSDISPLAYIO_GINFO => {
            let wdf = WsdisplayFbinfo {
                width: ri.ri_width.get() as u32,
                height: ri.ri_height.get() as u32,
                depth: ri.ri_depth.get() as u32,
                stride: ri.ri_stride.get() as u32,
                cmsize: 0,
                offset: 0,
            };
            ioctl_ret(data, &wdf);
        }
        WSDISPLAYIO_LINEBYTES => ioctl_ret(data, &(ri.ri_stride.get() as u32)),
        WSDISPLAYIO_SMODE => {}
        WSDISPLAYIO_GETSUPPORTEDDEPTH => ioctl_ret(data, &WSDISPLAYIO_DEPTH_24_32),
        WSDISPLAYIO_GVIDEO | WSDISPLAYIO_SVIDEO => {}
        _ => return Ok(false),
    }

    Ok(true)
}

/// `viogpu_wsmmap`: the physical page at `off` of the frame buffer.
///
/// # Safety
///
/// As for [`viogpu_wsioctl`].
pub unsafe fn viogpu_wsmmap(v: *mut c_void, off: i64, prot: i32) -> Option<Paddr> {
    // SAFETY: the caller's contract.
    let ri = unsafe { &*v.cast::<RasopsInfo>() };
    // SAFETY: the attach set `ri_hw` to its softc, which lives for good.
    let sc = unsafe { &*ri.ri_hw.get().cast::<ViogpuSoftc>() };
    let vsc = sc.virtio();
    let size = sc.sc_fb_dma_size.get();
    let segs = [sc.sc_fb_dma_seg[0].get()];

    if off < 0 || off as usize >= size {
        return None;
    }

    bus_dmamem_mmap(vsc.dmat(), &segs, off, prot, BUS_DMA_WAITOK)
}

/// `viogpu_alloc_screen`.
///
/// # Safety
///
/// As for `rasops_alloc_screen`.
pub unsafe fn viogpu_alloc_screen(
    v: *mut c_void,
    _type: *const WsscreenDescr,
    cookiep: &mut *mut c_void,
    curxp: &mut i32,
    curyp: &mut i32,
    attrp: &mut u32,
) -> Result<(), Errno> {
    // SAFETY: forwarded.
    unsafe { rasops_alloc_screen(v, ptr::null(), cookiep, curxp, curyp, attrp) }
}

// The wire sizes of `viogpu.h`'s packed structures.
const _: () = {
    assert!(size_of::<VirtioGpuCtrlHdr>() == 24);
    assert!(size_of::<VirtioGpuRect>() == 16);
    assert!(size_of::<VirtioGpuUpdateCursor>() == 56);
    assert!(size_of::<VirtioGpuResourceCreate2d>() == 40);
    assert!(size_of::<VirtioGpuSetScanout>() == 48);
    assert!(size_of::<VirtioGpuResourceFlush>() == 48);
    assert!(size_of::<VirtioGpuTransferToHost2d>() == 56);
    assert!(size_of::<VirtioGpuMemEntry>() == 16);
    assert!(size_of::<VirtioGpuResourceAttachBacking>() == 32);
    assert!(size_of::<VirtioGpuRespDisplayInfo>() == 24 + 24 * VIRTIO_GPU_MAX_SCANOUTS);
    assert!(size_of::<VirtioGpuTransferHost3d>() == 72);
    assert!(size_of::<VirtioGpuResourceCreate3d>() == 72);
    assert!(size_of::<VirtioGpuCtxCreate>() == 96);
    assert!(size_of::<VirtioGpuRespEdid>() == 1056);
    assert!(size_of::<VirtioGpuResourceCreateBlob>() == 56);
    assert!(size_of::<VirtioGpuSetScanoutBlob>() == 96);
    assert!(size_of::<VirtioGpuResourceAttachBackingEntries>() == 48);
    // One command and its reply fit the command page.
    assert!(size_of::<VirtioGpuCtrlHdr>() + size_of::<VirtioGpuRespDisplayInfo>() <= NBPG);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests of viogpu(4): the wire images of the commands the driver sends, and the
    // order of the attach's error labels.

    use std::{assert, assert_eq};

    use super::*;

    #[test]
    fn create_2d_wire_image() {
        let res = VirtioGpuResourceCreate2d {
            hdr: VirtioGpuCtrlHdr {
                r#type: VIRTIO_GPU_CMD_RESOURCE_CREATE_2D,
                flags: VIRTIO_GPU_FLAG_FENCE,
                fence_id: 3,
                ..Default::default()
            },
            resource_id: 1,
            format: VIRTIO_GPU_FORMAT_B8G8R8X8_UNORM,
            width: 1280,
            height: 800,
        };
        let b = res.wire_bytes();
        assert_eq!(b.len(), 40);
        assert_eq!(&b[0..4], &0x0101u32.to_le_bytes());
        assert_eq!(&b[4..8], &1u32.to_le_bytes());
        assert_eq!(&b[8..16], &3u64.to_le_bytes());
        assert_eq!(&b[24..28], &1u32.to_le_bytes());
        assert_eq!(&b[28..32], &2u32.to_le_bytes());
        assert_eq!(&b[32..36], &1280u32.to_le_bytes());
        assert_eq!(&b[36..40], &800u32.to_le_bytes());
    }

    #[test]
    fn display_info_reply_decodes() {
        let mut b = [0u8; size_of::<VirtioGpuRespDisplayInfo>()];
        b[0..4].copy_from_slice(&VIRTIO_GPU_RESP_OK_DISPLAY_INFO.to_le_bytes());
        // pmodes[0]: r = {0, 0, 1280, 800}, enabled = 1
        b[24 + 8..24 + 12].copy_from_slice(&1280u32.to_le_bytes());
        b[24 + 12..24 + 16].copy_from_slice(&800u32.to_le_bytes());
        b[24 + 16..24 + 20].copy_from_slice(&1u32.to_le_bytes());
        let info = VirtioGpuRespDisplayInfo::from_wire(&b);
        let (t, p) = (info.hdr.r#type, info.pmodes);
        let (w, h, en) = (p[0].r.width, p[0].r.height, p[0].enabled);
        assert_eq!((t, w, h, en), (0x1101, 1280, 800, 1));
    }

    #[test]
    fn attach_backing_carries_one_entry() {
        let backing = VirtioGpuResourceAttachBackingEntries {
            hdr: VirtioGpuCtrlHdr {
                r#type: VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING,
                ..Default::default()
            },
            resource_id: 1,
            nr_entries: 1,
            entries: [VirtioGpuMemEntry {
                addr: 0x4000_0000,
                length: 1280 * 800 * 4,
                padding: 0,
            }],
        };
        let b = backing.wire_bytes();
        assert_eq!(&b[0..4], &0x0106u32.to_le_bytes());
        assert_eq!(&b[28..32], &1u32.to_le_bytes());
        assert_eq!(&b[32..40], &0x4000_0000u64.to_le_bytes());
        assert_eq!(&b[40..44], &(1280u32 * 800 * 4).to_le_bytes());
    }

    #[test]
    fn unwind_levels_fall_through_in_label_order() {
        use ViogpuUnwind as U;
        let order = [
            U::Err,
            U::Errdma,
            U::Destroy,
            U::Free,
            U::Unmap,
            U::FbDestroy,
            U::FbFree,
            U::FbUnmap,
        ];
        for w in order.windows(2) {
            assert!(w[0] < w[1]);
        }
    }
}
/* </TESTS> */
