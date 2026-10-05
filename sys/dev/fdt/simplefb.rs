/*	$OpenBSD: simplefb.c,v 1.23 2026/09/18 03:33:29 kettenis Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2016 Mark Kettenis
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

//! The firmware's frame buffer on the device tree: `dev/fdt/simplefb.c`, `simplefb`.
//!
//! Upstream: sys/dev/fdt/simplefb.c @ 3ce1f3f79392
//!
//! `simplefb* at fdt?` drives a `simple-framebuffer` node: a linear frame buffer the
//! firmware (or arm64's efiboot, from the UEFI GOP) set up, described by its `reg`,
//! `width`, `height`, `stride` and pixel `format`. arm64's mainbus attaches the nodes of
//! `/chosen` once the root is mounted (`mainbus_attach_framebuffer`), so that a real
//! display driver can claim the frame buffer first (`rasops_check_framebuffer`). As the
//! console (`simplefb_init_cons`, from arm64's `consinit`) it is drawn on before
//! autoconfiguration.
//!
//! Under Limine there is no efiboot: the boot glue adds the `/chosen/framebuffer` node
//! efiboot would from Limine's framebuffer response (`sys/stand/fdtfb.rs`,
//! `docs/ARCHITECTURE.md`, "Frame buffer under Limine"). QEMU's `virt` device tree has no
//! `simple-framebuffer` node of its own (`ramfb` is configured by the firmware through
//! fw_cfg), and neither does the tree EDK2 hands on.
//!
//! ## Deviations
//! - wsdisplay is not ported yet (the next M13 console step): `wsdisplay_cnattach` and the
//!   `config_found_sm` of the `wsdisplay` child (`wsemuldisplaydevprint`,
//!   `wsemuldisplaydevsubmatch`) are reported with `unported!` where the C calls them, and
//!   the attach arguments go, under feature `qemu`, to the frame buffer self-test
//!   (`kern/selftest.rs`, `selftest=fb`).
//! - `simplefb_activate`'s `SUSPEND` path (redraw after a hibernation wakeup, `sleep_mode`)
//!   waits for suspend and resume, not ported: a `DVACT_WAKEUP` is reported with
//!   `unported!`; the children are activated as in C.
//! - `simplefb_init` returns the unknown format as `Err` (the C returns the format string,
//!   NULL on success). `simplefb_init_cons` is ported but nothing calls it yet: arm64's
//!   `consinit` picks the framebuffer console only with wsdisplay.
//! - `ws_get_param`/`ws_set_param` are `wsdisplayvar.rs`'s cells (as in `efifb.rs`).
//! - The console's rasops descriptor, screen descriptor and backing store are statics
//!   written while cold, under the kernel lock afterwards; a softc's descriptor list lives in
//!   `Cell`s of the softc, as the C's `sc_wsd`/`sc_wsl`/`sc_scrlist`.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr;

use libkern::StaticCell;

use crate::dev::ofw::fdt::{FdtReg, OF_getprop, OF_getpropint, OF_is_compatible, fdt_get_reg};
use crate::dev::rasops::rasops::{
    RI_CENTER, RI_CLEAR, RI_FULLCLEAR, RI_VCONS, RI_WRONLY, RasopsInfo, rasops_alloc_screen,
    rasops_check_framebuffer, rasops_free_screen, rasops_getchar, rasops_init, rasops_list_font,
    rasops_load_font, rasops_scrollback, rasops_show_screen,
};
use crate::dev::wscons::wsconsio::{
    WSDISPLAY_TYPE_EFIFB, WSDISPLAYIO_DEPTH_15, WSDISPLAYIO_DEPTH_16, WSDISPLAYIO_DEPTH_24_24,
    WSDISPLAYIO_DEPTH_24_32, WSDISPLAYIO_DEPTH_30, WSDISPLAYIO_GETPARAM,
    WSDISPLAYIO_GETSUPPORTEDDEPTH, WSDISPLAYIO_GINFO, WSDISPLAYIO_GTYPE, WSDISPLAYIO_GVIDEO,
    WSDISPLAYIO_LINEBYTES, WSDISPLAYIO_SETPARAM, WSDISPLAYIO_SMODE, WSDISPLAYIO_SVIDEO,
    WsdisplayFbinfo, WsdisplayParam,
};
use crate::dev::wscons::wsdisplayvar::{
    WsParamFn, WsdisplayAccessops, WsdisplayCharcell, WsemuldisplaydevAttachArgs, WsscreenDescr,
    WsscreenList, ws_get_param, ws_set_param,
};
use crate::kern::subr_autoconf::config_activate_children;
use crate::kern::subr_prf::{Str, printf};
use crate::machine::Machine;
use crate::machine::bus::{BusSpace, BusSpaceHandle, BusSpaceTag, bus_space_map, bus_space_vaddr};
use crate::machine::fdt::{FdtAttachArgs, fdt_find_cons, stdout_node};
use crate::machine::pmap::Pmap;
use crate::sys::device::{CfMatch, Cfattach, Cfdriver, DV_DULL, DVACT_WAKEUP, Device, Softc};
use crate::sys::errno::Errno;
use crate::sys::ioctl::{ioctl_arg, ioctl_ret};
use crate::sys::param::PAGE_MASK;
use crate::sys::proc::Proc;
use crate::sys::types::{Paddr, Psize};
use crate::unported;

/// `SIMPLEFB_WIDTH`.
const SIMPLEFB_WIDTH: i32 = 160;
/// `SIMPLEFB_HEIGHT`.
const SIMPLEFB_HEIGHT: i32 = 50;

/// `struct simplefb_format`: a pixel format of the binding and its layout.
pub struct SimplefbFormat {
    /// `format`.
    pub format: &'static [u8],
    /// `depth`.
    pub depth: i32,
    /// `rpos`.
    pub rpos: u8,
    /// `rnum`.
    pub rnum: u8,
    /// `gpos`.
    pub gpos: u8,
    /// `gnum`.
    pub gnum: u8,
    /// `bpos`.
    pub bpos: u8,
    /// `bnum`.
    pub bnum: u8,
}

impl SimplefbFormat {
    /// A format whose layout is given.
    const fn new(format: &'static [u8], depth: i32, layout: [u8; 6]) -> Self {
        Self {
            format,
            depth,
            rpos: layout[0],
            rnum: layout[1],
            gpos: layout[2],
            gnum: layout[3],
            bpos: layout[4],
            bnum: layout[5],
        }
    }
}

/// `struct simplefb_softc`.
#[repr(C)]
pub struct SimplefbSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `sc_iot`.
    pub sc_iot: Cell<Option<BusSpaceTag>>,
    /// `sc_ioh`.
    pub sc_ioh: Cell<Option<BusSpaceHandle>>,
    /// `sc_ri`.
    pub sc_ri: RasopsInfo,
    /// `sc_wsd`.
    pub sc_wsd: Cell<WsscreenDescr>,
    /// `sc_wsl`.
    pub sc_wsl: Cell<WsscreenList>,
    /// `sc_scrlist`.
    pub sc_scrlist: Cell<[*const WsscreenDescr; 1]>,
    /// `sc_format`.
    pub sc_format: Cell<*const SimplefbFormat>,
    /// `sc_paddr`.
    pub sc_paddr: Cell<Paddr>,
    /// `sc_psize`.
    pub sc_psize: Cell<Psize>,
}

// SAFETY: `#[repr(C)]` with the `struct device` first; every other member is zero-valid
// (`None`, null pointers, zero numbers, a zeroed rasops descriptor).
unsafe impl Softc for SimplefbSoftc {}

/// The console's rasops descriptor (`simplefb_ri`).
pub struct SimplefbConsole(RasopsInfo);

// SAFETY: written while cold (`simplefb_init_cons`), under the kernel lock afterwards, as
// the C's global.
unsafe impl Sync for SimplefbConsole {}

/// Supported pixel formats. Layout omitted when it matches the rasops defaults.
pub static SIMPLEFB_FORMATS: [SimplefbFormat; 10] = [
    SimplefbFormat::new(b"r5g6b5", 16, [11, 5, 5, 6, 0, 5]),
    SimplefbFormat::new(b"x1r5g5b5", 15, [10, 5, 5, 5, 0, 5]),
    SimplefbFormat::new(b"a1r5g5b5", 15, [10, 5, 5, 5, 0, 5]),
    SimplefbFormat::new(b"r8g8b8", 24, [16, 8, 8, 8, 0, 8]),
    SimplefbFormat::new(b"x8r8g8b8", 32, [16, 8, 8, 8, 0, 8]),
    SimplefbFormat::new(b"a8r8g8b8", 32, [16, 8, 8, 8, 0, 8]),
    SimplefbFormat::new(b"x8b8g8r8", 32, [0; 6]),
    SimplefbFormat::new(b"a8b8g8r8", 32, [0; 6]),
    SimplefbFormat::new(b"x2r10g10b10", 32, [20, 10, 10, 10, 0, 10]),
    SimplefbFormat::new(b"a2r10g10b10", 32, [20, 10, 10, 10, 0, 10]),
];

/// `simplefb_burn_hook`: a board's backlight switch (none is ported).
pub static SIMPLEFB_BURN_HOOK: StaticCell<Option<fn(u32)>> = StaticCell::new(None);

/// `simplefb_ri`.
static SIMPLEFB_RI: SimplefbConsole = SimplefbConsole(RasopsInfo::new());

/// `simplefb_wsd`.
static SIMPLEFB_WSD: StaticCell<WsscreenDescr> = StaticCell::new(WsscreenDescr::new(b"std"));

/// `simplefb_bs`.
static SIMPLEFB_BS: StaticCell<[WsdisplayCharcell; (SIMPLEFB_WIDTH * SIMPLEFB_HEIGHT) as usize]> =
    StaticCell::new(
        [WsdisplayCharcell { uc: 0, attr: 0 }; (SIMPLEFB_WIDTH * SIMPLEFB_HEIGHT) as usize],
    );

/// `simplefb_ca`.
pub static SIMPLEFB_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<SimplefbSoftc>(),
    ca_match: Some(simplefb_match),
    ca_attach: simplefb_attach,
    ca_detach: None,
    ca_activate: Some(simplefb_activate),
};

/// `simplefb_cd`.
pub static SIMPLEFB_CD: Cfdriver = Cfdriver::new(b"simplefb", DV_DULL, 0);

/// `simplefb_accessops`.
pub static SIMPLEFB_ACCESSOPS: WsdisplayAccessops = WsdisplayAccessops {
    ioctl: Some(simplefb_wsioctl),
    mmap: Some(simplefb_wsmmap),
    alloc_screen: Some(simplefb_alloc_screen),
    free_screen: Some(rasops_free_screen),
    show_screen: Some(rasops_show_screen),
    getchar: Some(rasops_getchar),
    load_font: Some(rasops_load_font),
    list_font: Some(rasops_list_font),
    scrollback: Some(rasops_scrollback),
    burn_screen: Some(simplefb_burn_screen),
    ..WsdisplayAccessops::EMPTY
};

/// `simplefb_match`: a `simple-framebuffer` node with an address that no other driver
/// claimed.
pub fn simplefb_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: the device-tree buses hand their children `struct fdt_attach_args`.
    let faa = unsafe { &*aux.cast::<FdtAttachArgs<'_>>() };

    // Don't attach if it has no address space.
    let Some(reg) = faa.fa_reg.first() else {
        return 0;
    };
    if reg.size == 0 {
        return 0;
    }

    // Don't attach if another driver already claimed our framebuffer.
    if rasops_check_framebuffer(Paddr::new(reg.addr as usize)) {
        return 0;
    }

    i32::from(OF_is_compatible(faa.fa_node, b"simple-framebuffer"))
}

/// `simplefb_attach`: map the frame buffer, set rasops up with virtual screens and attach
/// the `wsdisplay` child.
pub fn simplefb_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: as in `simplefb_match`.
    let faa = unsafe { &*aux.cast::<FdtAttachArgs<'_>>() };
    // SAFETY: `simplefb_ca` makes `SimplefbSoftc`s, which live as long as the device (for
    // good).
    let sc: &'static SimplefbSoftc = unsafe { &*ptr::from_ref(self_.softc::<SimplefbSoftc>()) };
    let ri: &'static RasopsInfo = &sc.sc_ri;
    let mut console = 0;

    if let Err(format) = simplefb_init(faa.fa_node, ri) {
        printf(format_args!(": unsupported format \"{}\"\n", Str(&format)));
        return;
    }

    if faa.fa_node == stdout_node() {
        console = 1;
    }

    let Some(reg) = faa.fa_reg.first() else {
        return;
    };
    sc.sc_iot.set(Some(faa.fa_iot));
    sc.sc_paddr.set(Paddr::new(reg.addr as usize));
    sc.sc_psize.set(Psize::new(reg.size as usize));
    // SAFETY: the firmware's frame buffer, which only this driver drives.
    let ioh = match unsafe {
        bus_space_map(
            faa.fa_iot,
            reg.addr as usize,
            reg.size as usize,
            <Machine as BusSpace>::BUS_SPACE_MAP_LINEAR
                | <Machine as BusSpace>::BUS_SPACE_MAP_PREFETCHABLE,
        )
    } {
        Ok(ioh) => ioh,
        Err(_) => {
            printf(format_args!(": can't map framebuffer\n"));
            return;
        }
    };
    sc.sc_ioh.set(Some(ioh));

    ri.ri_bits.set(bus_space_vaddr(faa.fa_iot, ioh));
    ri.ri_hw.set(ptr::from_ref(sc).cast_mut().cast());

    if console != 0 {
        // Preserve contents.
        ri.ri_bs.set(SIMPLEFB_BS.as_ptr().cast());
        ri.ri_flg.set(ri.ri_flg.get() & !RI_CLEAR);
    }

    printf(format_args!(
        ": {}x{}, {}bpp\n",
        ri.ri_width.get(),
        ri.ri_height.get(),
        ri.ri_depth.get()
    ));

    ri.ri_flg.set(ri.ri_flg.get() | RI_VCONS);
    if rasops_init(ri, SIMPLEFB_HEIGHT, SIMPLEFB_WIDTH).is_err() {
        printf(format_args!("{}: rasops_init failed\n", self_.xname()));
        return;
    }

    let mut wsd = WsscreenDescr::new(b"std");
    ri.fill_descr(&mut wsd);
    sc.sc_wsd.set(wsd);

    sc.sc_scrlist.set([sc.sc_wsd.as_ptr().cast_const()]);
    sc.sc_wsl.set(WsscreenList {
        nscreens: 1,
        screens: sc.sc_scrlist.as_ptr().cast::<*const WsscreenDescr>(),
    });

    if console != 0 {
        // SAFETY: with RI_VCONS the emulops take the active screen as their cookie.
        let _defattr = unsafe { ri.ops_pack_attr(ri.ri_active.get().cast(), 0, 0, 0) };
        // wsdisplay_cnattach(&sc->sc_wsd, ri->ri_active, simplefb_ri.ri_ccol,
        //     simplefb_ri.ri_crow, defattr);
        let _ = (SIMPLEFB_RI.0.ri_ccol.get(), SIMPLEFB_RI.0.ri_crow.get());
        let _ = unported!("wsdisplay_cnattach (simplefb console)");
    }

    let waa = WsemuldisplaydevAttachArgs {
        console,
        primary: 0,
        scrdata: sc.sc_wsl.as_ptr(),
        accessops: &SIMPLEFB_ACCESSOPS,
        accesscookie: ri.cookie(),
        defaultscreens: 0,
    };

    // config_found_sm(self, &waa, wsemuldisplaydevprint, wsemuldisplaydevsubmatch);
    let _ = unported!("wsdisplay* at simplefb? (wsemuldisplaydevprint, wsemuldisplaydevsubmatch)");
    #[cfg(feature = "qemu")]
    crate::kern::selftest::fb_attached(self_, &waa);
    #[cfg(not(feature = "qemu"))]
    let _ = waa;
}

/// `simplefb_activate`.
pub fn simplefb_activate(self_: &Device, act: i32) -> Result<(), Errno> {
    // SUSPEND: after a hibernation, redraw the active screen.
    if act == DVACT_WAKEUP {
        let _ = unported!("simplefb_activate: DVACT_WAKEUP (SUSPEND, sleep_mode)");
    }

    config_activate_children(self_, act)
}

/// `simplefb_init`: the geometry and channel layout of node `node`; `Err` holds a format
/// the driver does not know.
pub fn simplefb_init(node: i32, ri: &RasopsInfo) -> Result<(), [u8; 16]> {
    let mut format = [0u8; 16];

    OF_getprop(node, b"format", &mut format);
    format[15] = 0;
    let len = format.iter().position(|&c| c == 0).unwrap_or(15);

    let Some(fmt) = SIMPLEFB_FORMATS.iter().find(|f| f.format == &format[..len]) else {
        return Err(format);
    };

    ri.ri_width.set(OF_getpropint(node, b"width", 0) as i32);
    ri.ri_height.set(OF_getpropint(node, b"height", 0) as i32);
    ri.ri_stride.set(OF_getpropint(node, b"stride", 0) as i32);
    ri.ri_depth.set(fmt.depth);
    ri.ri_rpos.set(fmt.rpos);
    ri.ri_rnum.set(fmt.rnum);
    ri.ri_gpos.set(fmt.gpos);
    ri.ri_gnum.set(fmt.gnum);
    ri.ri_bpos.set(fmt.bpos);
    ri.ri_bnum.set(fmt.bnum);
    ri.ri_flg
        .set(RI_CENTER | RI_CLEAR | RI_FULLCLEAR | RI_WRONLY);

    Ok(())
}

/// `simplefb_wsioctl`.
///
/// # Safety
///
/// `v` is an attached frame buffer's `RasopsInfo` (the access cookie, whose `ri_hw` is the
/// softc); `data` is the kernel copy of the command's argument.
pub unsafe fn simplefb_wsioctl(
    v: *mut c_void,
    cmd: u64,
    data: &mut [u8],
    _flag: i32,
    _p: &Proc,
) -> Result<bool, Errno> {
    // SAFETY: the caller's contract.
    let ri = unsafe { &*v.cast::<RasopsInfo>() };
    // SAFETY: `simplefb_attach` set `ri_hw` to its softc, which lives for good.
    let sc = unsafe { &*ri.ri_hw.get().cast::<SimplefbSoftc>() };

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
        WSDISPLAYIO_GTYPE => {
            ioctl_ret(data, &WSDISPLAY_TYPE_EFIFB);
            return Ok(true);
        }
        WSDISPLAYIO_GINFO => {
            let wdf = WsdisplayFbinfo {
                width: ri.ri_width.get() as u32,
                height: ri.ri_height.get() as u32,
                depth: ri.ri_depth.get() as u32,
                stride: ri.ri_stride.get() as u32,
                offset: (sc.sc_paddr.get().as_usize() & PAGE_MASK) as u32,
                cmsize: 0, // color map is unavailable
            };
            ioctl_ret(data, &wdf);
        }
        WSDISPLAYIO_LINEBYTES => ioctl_ret(data, &(ri.ri_stride.get() as u32)),
        WSDISPLAYIO_SMODE => {}
        WSDISPLAYIO_GETSUPPORTEDDEPTH => {
            let d = match ri.ri_depth.get() {
                32 if ri.ri_rnum.get() == 10 => WSDISPLAYIO_DEPTH_30,
                32 => WSDISPLAYIO_DEPTH_24_32,
                24 => WSDISPLAYIO_DEPTH_24_24,
                16 => WSDISPLAYIO_DEPTH_16,
                15 => WSDISPLAYIO_DEPTH_15,
                _ => return Ok(false),
            };
            ioctl_ret(data, &d);
        }
        WSDISPLAYIO_GVIDEO | WSDISPLAYIO_SVIDEO => {}
        _ => return Ok(false),
    }

    Ok(true)
}

/// `simplefb_wsmmap`: the physical page at `off` of the frame buffer, uncached.
///
/// # Safety
///
/// As for [`simplefb_wsioctl`].
pub unsafe fn simplefb_wsmmap(v: *mut c_void, off: i64, _prot: i32) -> Option<Paddr> {
    // SAFETY: the caller's contract.
    let ri = unsafe { &*v.cast::<RasopsInfo>() };
    // SAFETY: as in `simplefb_wsioctl`.
    let sc = unsafe { &*ri.ri_hw.get().cast::<SimplefbSoftc>() };
    let paddr = sc.sc_paddr.get().as_usize();

    if off < 0 || off as usize >= sc.sc_psize.get().as_usize() + (paddr & PAGE_MASK) {
        return None;
    }

    Some(Paddr::new(
        ((paddr & !PAGE_MASK) + off as usize) | <Machine as Pmap>::PMAP_NOCACHE,
    ))
}

/// `simplefb_alloc_screen`.
///
/// # Safety
///
/// As for `rasops_alloc_screen`.
pub unsafe fn simplefb_alloc_screen(
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

/// `simplefb_burn_screen`: the board's backlight hook, if any.
///
/// # Safety
///
/// None: the cookie is not used.
pub unsafe fn simplefb_burn_screen(_v: *mut c_void, on: u32, _flags: u32) {
    // SAFETY: the hook is set while cold, if ever, and only read afterwards.
    if let Some(hook) = unsafe { SIMPLEFB_BURN_HOOK.read() } {
        hook(on);
    }
}

/// `simplefb_init_cons`: the console's `simple-framebuffer` node as the console display,
/// and a USB keyboard as its keyboard (`NUKBD > 0`).
pub fn simplefb_init_cons(iot: BusSpaceTag) {
    let ri: &'static RasopsInfo = &SIMPLEFB_RI.0;
    let mut reg = FdtReg::default();

    let node = fdt_find_cons(b"simple-framebuffer");
    if node.is_null() {
        return;
    }

    if fdt_get_reg(node, 0, &mut reg).is_err() {
        return;
    }

    // SAFETY: the console frame buffer the device tree names, which only this driver drives.
    let Ok(ioh) = (unsafe {
        bus_space_map(
            iot,
            reg.addr as usize,
            reg.size as usize,
            <Machine as BusSpace>::BUS_SPACE_MAP_LINEAR
                | <Machine as BusSpace>::BUS_SPACE_MAP_PREFETCHABLE,
        )
    }) else {
        return;
    };

    ri.ri_bits.set(bus_space_vaddr(iot, ioh));

    if simplefb_init(stdout_node(), ri).is_err() {
        return;
    }

    ri.ri_bs.set(SIMPLEFB_BS.as_ptr().cast());
    if rasops_init(ri, SIMPLEFB_HEIGHT, SIMPLEFB_WIDTH).is_err() {
        return;
    }

    // SAFETY: cold, the boot CPU: nothing else holds the descriptor.
    ri.fill_descr(unsafe { SIMPLEFB_WSD.get_mut() });

    let _defattr = ri.ri_pack_attr(0, 0, 0);
    // wsdisplay_cnattach(&simplefb_wsd, ri, 0, 0, defattr);
    let _ = unported!("wsdisplay_cnattach (simplefb console)");

    // Allow USB keyboards to become the console input device.
    let _ = crate::dev::usb::ukbd::ukbd_cnattach();
}
