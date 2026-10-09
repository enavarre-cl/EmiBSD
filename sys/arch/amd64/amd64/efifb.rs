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
 * Copyright (c) 2015 YASUOKA Masahiko <yasuoka@yasuoka.net>
 * Copyright (c) 2016 joshua stein <jcs@openbsd.org>
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
//! The EFI frame buffer: `arch/amd64/amd64/efifb.c`, `efifb(4)`.
//!
//! Upstream: sys/arch/amd64/amd64/efifb.c @ 3ce1f3f79392
//!
//! `efifb0 at mainbus?` drives the linear frame buffer the firmware's Graphics Output
//! Protocol set up before the kernel started (or, under coreboot, the one its tables
//! describe): no mode setting, only rasops drawing on the pixels. As the console
//! (`efifb_cnattach`, from `wscons_machdep.c`'s `wscn_video_init`) it is mapped early and
//! drawn on before autoconfiguration, then remapped (`efifb_cnremap`, from mainbus) and
//! attached; otherwise `efifb_attach` maps it and sets rasops up with virtual screens.
//!
//! ## Deviations
//! - `bios_efiinfo` (boot(8)'s `BOOTARG_EFIINFO`) is `machdep.rs`'s [`bios_efiinfo`], made
//!   from Limine's framebuffer response (`docs/ARCHITECTURE.md`, "Frame buffer under
//!   Limine").
//! - `efifb_early_map` is the bootloader's direct map: Limine maps the frame buffer memory in
//!   its higher-half direct map, which the kernel keeps (`pmap.rs`), so there is no
//!   `pmap_set_pml4_early` slot to borrow and `efifb_early_cleanup` has nothing to undo
//!   (`pmap_set_pml4_early`/`pmap_clear_pml4_early` are not ported).
//! - `efifb_cnattach` is `wscons_machdep.c`'s `wscn_video_init`'s, which is not ported: the
//!   kernel's console is the serial line (`consinit.rs`), as OpenBSD's with a serial console
//!   chosen by boot(8), so `wsdisplay0 at efifb0` attaches as a plain, non-console display.
//!   Under feature `qemu` the attach arguments are also handed to the frame buffer self-test
//!   (`kern/selftest.rs`, `selftest=fb`).
//! - `ws_get_param`/`ws_set_param` are `wsdisplayvar.rs`'s cells; their `int` result is
//!   0 (`Ok(true)`), -1 (`Ok(false)`, not ours) or an errno.
//! - `efifb_attach` checks `rasops_init`'s result: the C ignores it and would dereference
//!   the NULL font of a failed initialisation; here the attach stops with a message.
//! - The `struct efifb` of a non-console attach is malloc'd as in C; the console's and the
//!   backing store are statics written while cold, under the kernel lock afterwards.
//! - `fls`/`ffs` (libkern, not ported) are the bit-scan methods of `u32`.
//! - The coreboot table walk reads physical memory through the direct map
//!   (`PMAP_DIRECT_MAP`), unaligned, as the C's packed structures are; nothing calls it yet
//!   (`efifb_cb_cnattach` is `wscons_machdep.c`'s).

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{AtomicBool, Ordering};

use libkern::StaticCell;

use crate::arch::amd64::amd64::bus_space::{
    BUS_SPACE_MAP_LINEAR, BUS_SPACE_MAP_PREFETCHABLE, X86_BUS_SPACE_MEM, bus_space_map,
    bus_space_vaddr,
};
use crate::arch::amd64::amd64::machdep::bios_efiinfo;
use crate::arch::amd64::amd64::pmap::pmap_direct_map;
use crate::arch::amd64::include::efifbvar::EfifbAttachArgs;
use crate::arch::amd64::include::pmap::PMAP_WC;
use crate::dev::pci::pci_map::{pci_mapreg_info, pci_mapreg_probe};
use crate::dev::pci::pcireg::{
    PCI_MAPREG_END, PCI_MAPREG_MEM_TYPE_64BIT, PCI_MAPREG_START, PCI_MAPREG_TYPE_IO,
};
use crate::dev::pci::pcivar::PciAttachArgs;
use crate::dev::rasops::rasops::{
    RI_CENTER, RI_CLEAR, RI_VCONS, RI_WRONLY, RasopsInfo, rasops_alloc_screen, rasops_free_screen,
    rasops_getchar, rasops_init, rasops_list_font, rasops_load_font, rasops_scrollback,
    rasops_show_screen,
};
use crate::dev::wscons::wsconsio::{
    WSDISPLAY_TYPE_EFIFB, WSDISPLAYIO_DEPTH_8, WSDISPLAYIO_DEPTH_15, WSDISPLAYIO_DEPTH_16,
    WSDISPLAYIO_DEPTH_24_24, WSDISPLAYIO_DEPTH_24_32, WSDISPLAYIO_GETPARAM,
    WSDISPLAYIO_GETSUPPORTEDDEPTH, WSDISPLAYIO_GINFO, WSDISPLAYIO_GTYPE, WSDISPLAYIO_LINEBYTES,
    WSDISPLAYIO_SETPARAM, WSDISPLAYIO_SMODE, WsdisplayFbinfo, WsdisplayParam,
};
use crate::dev::wscons::wsdisplay::{
    wsdisplay_cnattach, wsemuldisplaydevprint, wsemuldisplaydevsubmatch,
};
use crate::dev::wscons::wsdisplayvar::{
    WsParamFn, WsdisplayAccessops, WsdisplayCharcell, WsemuldisplaydevAttachArgs, WsscreenDescr,
    WsscreenList, ws_get_param, ws_set_param,
};
use crate::kern::kern_malloc::malloc;
use crate::kern::subr_autoconf::config_found_sm;
use crate::kern::subr_prf::{panic, printf};
use crate::sys::device::{CfMatch, Cfattach, Cfdriver, DV_DULL, Device, Softc};
use crate::sys::errno::Errno;
use crate::sys::ioctl::{ioctl_arg, ioctl_ret};
use crate::sys::malloc::{M_DEVBUF, M_NOWAIT};
use crate::sys::proc::Proc;
use crate::sys::types::{Paddr, Psize};

/// `CB_TAG_VERSION`.
#[allow(dead_code)] // the C defines it; the table walk does not use it
const CB_TAG_VERSION: u32 = 0x0004;
/// `CB_TAG_FORWARD`.
const CB_TAG_FORWARD: u32 = 0x0011;
/// `CB_TAG_FRAMEBUFFER`.
const CB_TAG_FRAMEBUFFER: u32 = 0x0012;

/// `EFIFB_WIDTH`.
const EFIFB_WIDTH: i32 = 160;
/// `EFIFB_HEIGHT`.
const EFIFB_HEIGHT: i32 = 160;

/// `struct cb_header`: a coreboot table header (`"LBIO"`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct CbHeader {
    /// `signature`.
    signature: [u8; 4],
    /// `header_bytes`.
    header_bytes: u32,
    /// `header_checksum`.
    header_checksum: u32,
    /// `table_bytes`.
    table_bytes: u32,
    /// `table_checksum`.
    table_checksum: u32,
    /// `table_entries`.
    table_entries: u32,
}

/// `struct cb_framebuffer`: coreboot's frame buffer record.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct CbFramebuffer {
    /// `physical_address`.
    pub physical_address: u64,
    /// `x_resolution`.
    pub x_resolution: u32,
    /// `y_resolution`.
    pub y_resolution: u32,
    /// `bytes_per_line`.
    pub bytes_per_line: u32,
    /// `bits_per_pixel`.
    pub bits_per_pixel: u8,
    /// `red_mask_pos`.
    pub red_mask_pos: u8,
    /// `red_mask_size`.
    pub red_mask_size: u8,
    /// `green_mask_pos`.
    pub green_mask_pos: u8,
    /// `green_mask_size`.
    pub green_mask_size: u8,
    /// `blue_mask_pos`.
    pub blue_mask_pos: u8,
    /// `blue_mask_size`.
    pub blue_mask_size: u8,
    /// `reserved_mask_pos`.
    pub reserved_mask_pos: u8,
    /// `reserved_mask_size`.
    pub reserved_mask_size: u8,
}

/// `struct cb_entry`'s fixed part (the union follows it).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct CbEntry {
    /// `tag`.
    tag: u32,
    /// `size`.
    size: u32,
}

/// `struct efifb`.
pub struct Efifb {
    /// `rinfo`.
    pub rinfo: RasopsInfo,
    /// `depth`.
    pub depth: Cell<i32>,
    /// `paddr`.
    pub paddr: Cell<Paddr>,
    /// `psize`.
    pub psize: Cell<Psize>,
    /// `cb_table_fb`.
    pub cb_table_fb: Cell<CbFramebuffer>,
}

impl Efifb {
    /// A zeroed `struct efifb`.
    pub const fn new() -> Self {
        Self {
            rinfo: RasopsInfo::new(),
            depth: Cell::new(0),
            paddr: Cell::new(Paddr::new(0)),
            psize: Cell::new(Psize::new(0)),
            cb_table_fb: Cell::new(CbFramebuffer {
                physical_address: 0,
                x_resolution: 0,
                y_resolution: 0,
                bytes_per_line: 0,
                bits_per_pixel: 0,
                red_mask_pos: 0,
                red_mask_size: 0,
                green_mask_pos: 0,
                green_mask_size: 0,
                blue_mask_pos: 0,
                blue_mask_size: 0,
                reserved_mask_pos: 0,
                reserved_mask_size: 0,
            }),
        }
    }

    /// `memset(fb, 0, sizeof(*fb))` for the static console structure: everything but the
    /// rasops descriptor's list and task, which `rasops_init` sets again.
    fn clear(&self) {
        self.depth.set(0);
        self.paddr.set(Paddr::new(0));
        self.psize.set(Psize::new(0));
        self.cb_table_fb.set(CbFramebuffer::default());
        let ri = &self.rinfo;
        ri.ri_font.set(ptr::null_mut());
        ri.ri_bs.set(ptr::null_mut());
        ri.ri_bits.set(ptr::null_mut());
        ri.ri_flg.set(0);
        ri.ri_rnum.set(0);
    }
}

impl Default for Efifb {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct efifb_softc`.
#[repr(C)]
pub struct EfifbSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `sc_fb`.
    pub sc_fb: Cell<*const Efifb>,
}

// SAFETY: `#[repr(C)]` with the `struct device` first; a null `sc_fb` is the zero pattern.
unsafe impl Softc for EfifbSoftc {}

/// The console frame buffer and its screen descriptor, list and backing store.
pub struct EfifbStatics {
    /// `efifb_console`.
    console: Efifb,
}

// SAFETY: written while cold (console attachment, autoconfiguration) and afterwards only
// under the kernel lock, as the C's globals are.
unsafe impl Sync for EfifbStatics {}

/// `efifb_descrs[]`.
struct EfifbDescrs([*const WsscreenDescr; 1]);

// SAFETY: a constant pointer at a static.
unsafe impl Sync for EfifbDescrs {}

/// `efifb_ca`.
pub static EFIFB_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<EfifbSoftc>(),
    ca_match: Some(efifb_match),
    ca_attach: efifb_attach,
    ca_detach: None,
    ca_activate: None,
};

/// `efifb_std_descr`.
static EFIFB_STD_DESCR: StaticCell<WsscreenDescr> = StaticCell::new(WsscreenDescr::new(b"std"));

/// `efifb_descrs`.
static EFIFB_DESCRS: EfifbDescrs = EfifbDescrs([EFIFB_STD_DESCR.as_ptr().cast_const()]);

/// `efifb_screen_list`.
static EFIFB_SCREEN_LIST: StaticCell<WsscreenList> = StaticCell::new(WsscreenList {
    nscreens: 1,
    screens: EFIFB_DESCRS.0.as_ptr(),
});

/// `efifb_accessops`.
pub static EFIFB_ACCESSOPS: WsdisplayAccessops = WsdisplayAccessops {
    ioctl: Some(efifb_ioctl),
    mmap: Some(efifb_mmap),
    alloc_screen: Some(efifb_alloc_screen),
    free_screen: Some(rasops_free_screen),
    show_screen: Some(rasops_show_screen),
    getchar: Some(rasops_getchar),
    load_font: Some(rasops_load_font),
    list_font: Some(rasops_list_font),
    scrollback: Some(rasops_scrollback),
    ..WsdisplayAccessops::EMPTY
};

/// `efifb_cd`.
pub static EFIFB_CD: Cfdriver = Cfdriver::new(b"efifb", DV_DULL, 0);

/// `efifb_detached`.
static EFIFB_DETACHED: AtomicBool = AtomicBool::new(false);

/// `efifb_console`.
static EFIFB: EfifbStatics = EfifbStatics {
    console: Efifb::new(),
};

/// `efifb_bs`: the backing store of the console's characters.
static EFIFB_BS: StaticCell<[WsdisplayCharcell; (EFIFB_HEIGHT * EFIFB_WIDTH) as usize]> =
    StaticCell::new([WsdisplayCharcell { uc: 0, attr: 0 }; (EFIFB_HEIGHT * EFIFB_WIDTH) as usize]);

/// `efifb_console`.
fn efifb_console() -> &'static Efifb {
    &EFIFB.console
}

/// `fls`: the last (most significant) bit set, from 1; 0 for 0.
fn fls(x: u32) -> i32 {
    32 - x.leading_zeros() as i32
}

/// `ffs`: the first (least significant) bit set, from 1; 0 for 0.
fn ffs(x: u32) -> i32 {
    if x == 0 {
        0
    } else {
        x.trailing_zeros() as i32 + 1
    }
}

/// `efifb_match`: the `efifb` child of mainbus, when there is a console frame buffer or the
/// firmware reported one.
pub fn efifb_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: mainbus hands its `efifb` child a `struct efifb_attach_args` (`mba_eaa`).
    let eaa = unsafe { &*aux.cast::<EfifbAttachArgs>() };

    if EFIFB_DETACHED.load(Ordering::Relaxed) {
        return 0;
    }

    if eaa.eaa_name == EFIFB_CD.cd_name {
        if efifb_console().paddr.get().as_usize() != 0 {
            return 1;
        }
        if bios_efiinfo().is_some_and(|ei| ei.fb_addr != 0) {
            return 1;
        }
    }

    0
}

/// `efifb_attach`: the console's frame buffer, or a new one mapped here, and the `wsdisplay`
/// child.
pub fn efifb_attach(_parent: Option<&Device>, self_: &Device, _aux: *mut c_void) {
    // SAFETY: `efifb_ca` makes `EfifbSoftc`s, which live as long as the device (for good).
    let sc: &'static EfifbSoftc = unsafe { &*ptr::from_ref(self_.softc::<EfifbSoftc>()) };
    let mut console = 0;

    let fb: &'static Efifb = if efifb_console().paddr.get().as_usize() != 0 {
        console = 1;
        efifb_console()
    } else {
        let Some(mem) = malloc(size_of::<Efifb>(), M_DEVBUF, M_NOWAIT) else {
            return;
        };
        let p = mem.cast::<Efifb>().as_ptr();
        // SAFETY: a fresh allocation of one `struct efifb`, malloc(9)-aligned, kept for good
        // (M_ZERO in C: `Efifb::new` is the zeroed structure).
        let fb: &'static Efifb = unsafe {
            ptr::write(p, Efifb::new());
            &*p
        };

        efifb_efiinfo_init(fb);

        // SAFETY: the firmware's frame buffer, which only this driver drives.
        let ioh = match unsafe {
            bus_space_map(
                X86_BUS_SPACE_MEM,
                fb.paddr.get().as_usize(),
                fb.psize.get().as_usize(),
                BUS_SPACE_MAP_PREFETCHABLE | BUS_SPACE_MAP_LINEAR,
            )
        } {
            Ok(ioh) => ioh,
            Err(_) => {
                crate::kern::kern_malloc::free(mem, M_DEVBUF, size_of::<Efifb>());
                return;
            }
        };
        fb.rinfo
            .ri_bits
            .set(bus_space_vaddr(X86_BUS_SPACE_MEM, ioh));
        if efifb_rasops_init(fb, RI_VCONS).is_err() {
            printf(format_args!(": rasops_init failed\n"));
            return;
        }
        // SAFETY: autoconfiguration is single-threaded; nothing else holds the descriptor.
        fb.rinfo.fill_descr(unsafe { EFIFB_STD_DESCR.get_mut() });
        fb
    };
    let ri = &fb.rinfo;

    sc.sc_fb.set(fb);
    printf(format_args!(
        ": {}x{}, {}bpp\n",
        ri.ri_width.get(),
        ri.ri_height.get(),
        ri.ri_depth.get()
    ));

    if console != 0 {
        let ccol = ri.ri_ccol.get();
        let crow = ri.ri_crow.get();

        let _ = efifb_rasops_init(fb, RI_VCONS);

        // SAFETY: with RI_VCONS the emulops take the active screen as their cookie.
        let defattr = unsafe { ri.ops_pack_attr(ri.ri_active.get().cast(), 0, 0, 0) }.unwrap_or(0);
        // SAFETY: the descriptor was filled by the console attach and is only read from now
        // on; with RI_VCONS the active screen is the emulops' cookie, for good.
        unsafe {
            wsdisplay_cnattach(
                EFIFB_STD_DESCR.get(),
                ri.ri_active.get().cast(),
                ccol,
                crow,
                defattr,
            )
        };
    }

    ri.ri_hw.set(ptr::from_ref(sc).cast_mut().cast());
    let mut aa = WsemuldisplaydevAttachArgs {
        console,
        primary: 0,
        scrdata: EFIFB_SCREEN_LIST.as_ptr(),
        accessops: &EFIFB_ACCESSOPS,
        accesscookie: ri.cookie(),
        defaultscreens: 0,
    };

    #[cfg(feature = "qemu")]
    crate::kern::selftest::fb_attached(self_, &aa);

    let _ = config_found_sm(
        self_,
        ptr::from_mut(&mut aa).cast(),
        Some(wsemuldisplaydevprint),
        Some(wsemuldisplaydevsubmatch),
    );
}

/// `efifb_rasops_init`: the geometry and channel layout from coreboot's table or from
/// `bios_efiinfo`, then `rasops_init` for a grid of at most 160x160 characters.
fn efifb_rasops_init(fb: &'static Efifb, flags: i32) -> Result<(), Errno> {
    // bmnum, bmpos
    let bmnum = |x: u32| (fls(x) - ffs(x) + 1) as u8;
    let bmpos = |x: u32| (ffs(x) - 1) as u8;
    let ri = &fb.rinfo;

    let cb = efifb_console().cb_table_fb.get();
    if cb.x_resolution != 0 {
        ri.ri_width.set(cb.x_resolution as i32);
        ri.ri_height.set(cb.y_resolution as i32);
        ri.ri_depth.set(fb.depth.get());
        ri.ri_stride.set(cb.bytes_per_line as i32);
        ri.ri_rnum.set(cb.red_mask_size);
        ri.ri_rpos.set(cb.red_mask_pos);
        ri.ri_gnum.set(cb.green_mask_size);
        ri.ri_gpos.set(cb.green_mask_pos);
        ri.ri_bnum.set(cb.blue_mask_size);
        ri.ri_bpos.set(cb.blue_mask_pos);
    } else {
        let Some(ei) = bios_efiinfo() else {
            return Err(Errno::ENXIO);
        };
        ri.ri_width.set(ei.fb_width as i32);
        ri.ri_height.set(ei.fb_height as i32);
        ri.ri_depth.set(fb.depth.get());
        ri.ri_stride.set(ei.fb_pixpsl as i32 * (fb.depth.get() / 8));
        ri.ri_rnum.set(bmnum(ei.fb_red_mask));
        ri.ri_rpos.set(bmpos(ei.fb_red_mask));
        ri.ri_gnum.set(bmnum(ei.fb_green_mask));
        ri.ri_gpos.set(bmpos(ei.fb_green_mask));
        ri.ri_bnum.set(bmnum(ei.fb_blue_mask));
        ri.ri_bpos.set(bmpos(ei.fb_blue_mask));
    }
    ri.ri_bs.set(EFIFB_BS.as_ptr().cast());
    // if reinitializing, it is important to not clear all the flags
    ri.ri_flg.set(ri.ri_flg.get() & !RI_CLEAR);
    ri.ri_flg
        .set(ri.ri_flg.get() | flags | RI_CENTER | RI_WRONLY);
    rasops_init(&fb.rinfo, EFIFB_HEIGHT, EFIFB_WIDTH)
}

/// `efifb_ioctl`.
///
/// # Safety
///
/// `v` is the frame buffer's `RasopsInfo` (the access cookie); `data` is the kernel copy of
/// the command's argument.
pub unsafe fn efifb_ioctl(
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
        WSDISPLAYIO_GTYPE => ioctl_ret(data, &WSDISPLAY_TYPE_EFIFB),
        WSDISPLAYIO_GINFO => {
            let wdf = WsdisplayFbinfo {
                width: ri.ri_width.get() as u32,
                height: ri.ri_height.get() as u32,
                depth: ri.ri_depth.get() as u32,
                stride: ri.ri_stride.get() as u32,
                offset: 0,
                cmsize: 0, // color map is unavailable
            };
            ioctl_ret(data, &wdf);
        }
        WSDISPLAYIO_LINEBYTES => ioctl_ret(data, &(ri.ri_stride.get() as u32)),
        WSDISPLAYIO_SMODE => {}
        WSDISPLAYIO_GETSUPPORTEDDEPTH => {
            let d = match ri.ri_depth.get() {
                32 => WSDISPLAYIO_DEPTH_24_32,
                24 => WSDISPLAYIO_DEPTH_24_24,
                16 => WSDISPLAYIO_DEPTH_16,
                15 => WSDISPLAYIO_DEPTH_15,
                8 => WSDISPLAYIO_DEPTH_8,
                _ => return Ok(false),
            };
            ioctl_ret(data, &d);
        }
        _ => return Ok(false),
    }

    Ok(true)
}

/// `efifb_mmap`: the physical page at `off`, write-combining.
///
/// # Safety
///
/// `v` is an attached frame buffer's `RasopsInfo` (whose `ri_hw` is the softc).
pub unsafe fn efifb_mmap(v: *mut c_void, off: i64, _prot: i32) -> Option<Paddr> {
    // SAFETY: the caller's contract.
    let ri = unsafe { &*v.cast::<RasopsInfo>() };
    // SAFETY: `efifb_attach` set `ri_hw` to its softc, which lives for good.
    let sc = unsafe { &*ri.ri_hw.get().cast::<EfifbSoftc>() };
    // SAFETY: the softc's frame buffer is set before the attach arguments go out.
    let fb = unsafe { &*sc.sc_fb.get() };

    if off < 0 || off as usize >= fb.psize.get().as_usize() {
        return None;
    }

    Some(Paddr::new(
        (fb.paddr.get().as_usize() + off as usize) | PMAP_WC as usize,
    ))
}

/// `efifb_alloc_screen`.
///
/// # Safety
///
/// As for `rasops_alloc_screen`.
pub unsafe fn efifb_alloc_screen(
    v: *mut c_void,
    _descr: *const WsscreenDescr,
    cookiep: &mut *mut c_void,
    curxp: &mut i32,
    curyp: &mut i32,
    attrp: &mut u32,
) -> Result<(), Errno> {
    // SAFETY: forwarded.
    unsafe { rasops_alloc_screen(v, ptr::null(), cookiep, curxp, curyp, attrp) }
}

/// `efifb_cnattach`: the frame buffer `bios_efiinfo` describes as the console;
/// `Err(ENXIO)` (the C's -1) without one.
pub fn efifb_cnattach() -> Result<(), Errno> {
    if !bios_efiinfo().is_some_and(|ei| ei.fb_addr != 0) {
        return Err(Errno::ENXIO);
    }

    efifb_console().clear();
    efifb_efiinfo_init(efifb_console());
    efifb_cnattach_common();

    Ok(())
}

/// `efifb_efiinfo_init`: address, depth and size from `bios_efiinfo`.
fn efifb_efiinfo_init(fb: &Efifb) {
    let Some(ei) = bios_efiinfo() else {
        return;
    };
    fb.paddr.set(Paddr::new(ei.fb_addr as usize));
    let mut depth = fb.depth.get();
    depth = depth.max(fls(ei.fb_red_mask));
    depth = depth.max(fls(ei.fb_green_mask));
    depth = depth.max(fls(ei.fb_blue_mask));
    depth = depth.max(fls(ei.fb_reserved_mask));
    fb.depth.set(depth);
    fb.psize.set(Psize::new(
        ei.fb_height as usize * ei.fb_pixpsl as usize * (depth / 8) as usize,
    ));
}

/// `efifb_cnattach_common`: map the console early, clear it and hand it to wsdisplay.
fn efifb_cnattach_common() {
    let fb = efifb_console();
    let ri = &fb.rinfo;

    ri.ri_bits.set(efifb_early_map(fb.paddr.get()) as *mut u8);

    if efifb_rasops_init(fb, RI_CLEAR).is_err() {
        return;
    }

    // SAFETY: cold, the boot CPU: nothing else holds the descriptor.
    ri.fill_descr(unsafe { EFIFB_STD_DESCR.get_mut() });

    let defattr = ri.ri_pack_attr(0, 0, 0).unwrap_or(0);
    // SAFETY: cold, the boot CPU: the descriptor was just filled and is only read from now
    // on; the console's rasops_info is a static, the emulops' cookie for good.
    unsafe { wsdisplay_cnattach(EFIFB_STD_DESCR.get(), ri.cookie(), 0, 0, defattr) };
}

/// `efifb_cnremap`: map the console frame buffer for good once `bus_space_map` works
/// (mainbus, before the children attach).
pub fn efifb_cnremap() {
    let fb = efifb_console();
    let ri = &fb.rinfo;

    if fb.paddr.get().as_usize() == 0 {
        return;
    }

    // SAFETY: the console frame buffer, which only this driver drives.
    let Ok(ioh) = (unsafe {
        bus_space_map(
            X86_BUS_SPACE_MEM,
            fb.paddr.get().as_usize(),
            fb.psize.get().as_usize(),
            BUS_SPACE_MAP_PREFETCHABLE | BUS_SPACE_MAP_LINEAR,
        )
    }) else {
        panic(format_args!("can't remap framebuffer"));
    };
    ri.ri_origbits.set(bus_space_vaddr(X86_BUS_SPACE_MEM, ioh));

    let _ = efifb_rasops_init(fb, 0);

    efifb_early_cleanup();
}

/// Whether the console frame buffer (`paddr`, or with `efiinfo` also `bios_efiinfo`'s) lies
/// in one of the memory BARs of `pa`.
fn efifb_in_bars(pa: &PciAttachArgs, efiinfo: bool) -> bool {
    let (pc, tag) = (pa.pa_pc, pa.pa_tag);
    let console = efifb_console().paddr.get().as_usize();
    let fw = bios_efiinfo().map_or(0, |ei| ei.fb_addr as usize);

    let mut reg = PCI_MAPREG_START;
    while reg < PCI_MAPREG_END {
        let Some(type_) = pci_mapreg_probe(pc, tag, reg) else {
            reg += 4;
            continue;
        };

        if type_ == PCI_MAPREG_TYPE_IO {
            reg += 4;
            continue;
        }

        let Ok((base, size, _)) = pci_mapreg_info(pc, tag, reg, type_) else {
            reg += 4;
            continue;
        };

        if efiinfo && bios_efiinfo().is_some() && fw >= base && fw < base + size {
            return true;
        }

        if console >= base && console < base + size {
            return true;
        }

        if type_ & PCI_MAPREG_MEM_TYPE_64BIT != 0 {
            reg += 4;
        }
        reg += 4;
    }

    false
}

/// `efifb_is_console`: whether the console frame buffer is in a BAR of `pa` (a display
/// driver taking over from efifb).
pub fn efifb_is_console(pa: &PciAttachArgs) -> bool {
    efifb_in_bars(pa, false)
}

/// `efifb_is_primary`: whether the firmware's or the console frame buffer is in a BAR of
/// `pa`.
pub fn efifb_is_primary(pa: &PciAttachArgs) -> bool {
    efifb_in_bars(pa, true)
}

/// `efifb_detach`: a display driver took the frame buffer over.
pub fn efifb_detach() {
    EFIFB_DETACHED.store(true, Ordering::Relaxed);
}

/// `efifb_reattach`: a display driver gave the frame buffer back.
pub fn efifb_reattach() {
    EFIFB_DETACHED.store(false, Ordering::Relaxed);
    crate::arch::amd64::amd64::mainbus::mainbus_efifb_reattach();
}

/// `efifb_cb_cnattach`: the frame buffer of coreboot's tables as the console; `Err(ENXIO)`
/// (the C's -1) without one.
pub fn efifb_cb_cnattach() -> Result<(), Errno> {
    let Some(cb_fb) = cb_find_fb(Paddr::new(0)) else {
        return Err(Errno::ENXIO);
    };
    if cb_fb.x_resolution == 0 {
        return Err(Errno::ENXIO);
    }

    let fb = efifb_console();
    fb.clear();
    fb.cb_table_fb.set(cb_fb);

    fb.paddr.set(Paddr::new(cb_fb.physical_address as usize));
    fb.depth.set(i32::from(cb_fb.bits_per_pixel));
    fb.psize.set(Psize::new(
        cb_fb.y_resolution as usize * cb_fb.bytes_per_line as usize,
    ));

    efifb_cnattach_common();

    Ok(())
}

/// `efifb_cb_found`: whether the console came from coreboot's tables.
pub fn efifb_cb_found() -> bool {
    let fb = efifb_console();
    fb.paddr.get().as_usize() != 0 && fb.cb_table_fb.get().x_resolution != 0
}

/// `cb_checksum`: the IP checksum of `size` bytes at `addr`.
///
/// # Safety
///
/// `size` bytes at `addr` are mapped and readable.
unsafe fn cb_checksum(addr: *const u8, size: usize) -> u16 {
    let mut sum: u32 = 0;

    for i in 0..size / 2 {
        // SAFETY: inside the `size` bytes (the caller's contract).
        sum = sum.wrapping_add(u32::from(unsafe {
            ptr::read_unaligned(addr.add(i * 2).cast::<u16>())
        }));
    }

    sum = (sum >> 16) + (sum & 0xffff);
    sum += sum >> 16;
    sum = !sum & 0xffff;

    sum as u16
}

/// `cb_find_fb`: coreboot's frame buffer record, from the `"LBIO"` table in the 4 KiB at
/// physical `addr` (following forward records).
pub fn cb_find_fb(addr: Paddr) -> Option<CbFramebuffer> {
    let addr = addr.as_usize();
    for i in (0..4 * 1024).step_by(16) {
        let cbh_p = pmap_direct_map(Paddr::new(addr + i)).as_usize() as *const u8;
        // SAFETY: low physical memory through the direct map, read unaligned as the C's
        // packed table.
        let cbh: CbHeader = unsafe { ptr::read_unaligned(cbh_p.cast()) };
        if &cbh.signature != b"LBIO" {
            continue;
        }

        if cbh.header_bytes == 0 {
            continue;
        }

        // SAFETY: the header just read.
        if unsafe { cb_checksum(cbh_p, size_of::<CbHeader>()) } != 0 {
            return None;
        }

        let cbtable = pmap_direct_map(Paddr::new(addr + i + cbh.header_bytes as usize)).as_usize();

        let mut j = 0usize;
        while j < cbh.table_bytes as usize {
            let cbe_p = (cbtable + j) as *const u8;
            // SAFETY: a record of the table, through the direct map.
            let cbe: CbEntry = unsafe { ptr::read_unaligned(cbe_p.cast()) };
            // SAFETY: the record's union follows its tag and size.
            let u = unsafe { cbe_p.add(size_of::<CbEntry>()) };

            match cbe.tag {
                CB_TAG_FORWARD => {
                    // SAFETY: a forward record holds a 64-bit address.
                    let forward: u64 = unsafe { ptr::read_unaligned(u.cast()) };
                    return cb_find_fb(Paddr::new(forward as usize));
                }
                CB_TAG_FRAMEBUFFER => {
                    // SAFETY: a frame buffer record holds a `struct cb_framebuffer`.
                    return Some(unsafe { ptr::read_unaligned(u.cast()) });
                }
                _ => {}
            }
            if cbe.size == 0 {
                break;
            }
            j += cbe.size as usize;
        }
    }

    None
}

/// `efifb_stolen`: the bytes of the console frame buffer (what the DRM drivers keep).
pub fn efifb_stolen() -> Psize {
    efifb_console().psize.get()
}

/// `efifb_early_map`: the kernel virtual address of the frame buffer before `bus_space_map`
/// works: the bootloader's direct map (see the module's deviations).
fn efifb_early_map(pa: Paddr) -> usize {
    pmap_direct_map(pa).as_usize()
}

/// `efifb_early_cleanup`: nothing was borrowed (see the module's deviations).
fn efifb_early_cleanup() {}

const _: () = assert!(size_of::<CbHeader>() == 24);
/* </CODE> */
