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
 * Copyright (c) 2001 Wasabi Systems, Inc.
 * All rights reserved.
 *
 * Written by Frank van der Linden for Wasabi Systems, Inc.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *	This product includes software developed for the NetBSD Project by
 *	Wasabi Systems, Inc.
 * 4. The name of Wasabi Systems, Inc. may not be used to endorse
 *    or promote products derived from this software without specific prior
 *    written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY WASABI SYSTEMS, INC. ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL WASABI SYSTEMS, INC
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */
/*
 * Copyright (c) 1995, 1996 Carnegie-Mellon University.
 * All rights reserved.
 *
 * Author: Chris G. Demetriou
 *
 * Permission to use, copy, modify and distribute this software and
 * its documentation is hereby granted, provided that both the copyright
 * notice and this permission notice appear in all copies of the
 * software, derivative works or modified versions, and any portions
 * thereof, and that both notices appear in supporting documentation.
 *
 * CARNEGIE MELLON ALLOWS FREE USE OF THIS SOFTWARE IN ITS "AS IS"
 * CONDITION.  CARNEGIE MELLON DISCLAIMS ANY LIABILITY OF ANY KIND
 * FOR ANY DAMAGES WHATSOEVER RESULTING FROM THE USE OF THIS SOFTWARE.
 *
 * Carnegie Mellon requests users of this software to return to
 *
 *  Software Distribution Coordinator  or  Software.Distribution@CS.CMU.EDU
 *  School of Computer Science
 *  Carnegie Mellon University
 *  Pittsburgh PA 15213-3890
 *
 * any improvements or extensions that they make and grant Carnegie the
 * rights to redistribute these changes.
 */
/* </LICENSES> */

/* <CODE> */
//! `vga(4)` at `pci`: a VGA-class PCI function whose I/O and memory decoding the firmware
//! left on and that answers at the legacy VGA addresses (or is the console), driven by
//! `vga.c`.
//!
//! Upstream: sys/dev/pci/vga_pci.c @ 3ce1f3f79392
//!
//! Under UEFI the firmware leaves the VGA in a graphics mode: on QEMU's q35 with OVMF (the
//! `just smoke` machine) the std VGA's legacy text memory does not answer the probe, and
//! OpenBSD prints `"Bochs VGA" rev 0x02 at pci0 dev 1 function 0 not configured` and
//! attaches `efifb0` instead (an OpenBSD 8.0 snapshot's dmesg on that machine,
//! `just diff-openbsd`). This port makes the same decision.
//!
//! ## Deviations
//! - `X86EMU` (amd64 GENERIC's `option X86EMU`): `vga_post_init` and `vga_post_call`
//!   (`arch/amd64/pci/vga_post.c` over `dev/x86emu`, which emulates the card's BIOS to
//!   re-POST it on resume) are not ported: `vga_pci_attach` reports the gap with
//!   `unported!` and keeps no handle, and a resume that would re-POST reports it too. Only
//!   the Intel GMA 500/600/3600 and Medfield chips of [`VGA_DEVS`] are re-POSTed.
//! - `RAMDISK_HOOKS` (bsd.rd's option; `vga_aperture_needed`, `vga_pci_common.c`) is not
//!   configured by any EmiBSD kernel, so its "aperture needed" message is not compiled, as
//!   in a kernel without the option.
//! - `vga_pci_activate` and the `vga_*_state` functions are compiled always (`NACPI > 0`
//!   and not `SMALL_KERNEL`, as amd64 GENERIC); they run only on suspend and resume, which
//!   EmiBSD does not do yet.
//! - `vga_pci_ioctl`'s result follows the accessops convention (`wsdisplayvar.rs`): the
//!   hooks' 0 is `Ok(true)`, their -1 `Ok(false)`, `ENOTTY` an error as in C.

use core::ffi::c_void;
use core::mem::offset_of;
use core::ptr;
use core::sync::atomic::{AtomicI32, Ordering};

use crate::dev::ic::mc6845reg::RegMc6845;
use crate::dev::ic::pcdisplayvar::{_pcdisplay_6845_read, _pcdisplay_6845_write};
use crate::dev::ic::vga::{
    vga_cnattach, vga_common_attach, vga_common_probe, vga_is_console, vga_restore_fonts,
    vga_restore_palette, vga_save_palette,
};
use crate::dev::ic::vgareg::{RegVgaattr, RegVgagdc, RegVgats};
use crate::dev::ic::vgavar::{
    _vga_attr_read, _vga_attr_write, _vga_gdc_read, _vga_gdc_write, _vga_ts_read, _vga_ts_write,
    vga_6845_read, vga_6845_write, vga_ts_write,
};
use crate::dev::pci::pcidevs::{
    PCI_PRODUCT_INTEL_GMA600_0, PCI_PRODUCT_INTEL_GMA600_8, PCI_PRODUCT_INTEL_GMA3600_0,
    PCI_PRODUCT_INTEL_MDFLD_IGD_0, PCI_PRODUCT_INTEL_US15L_IGD, PCI_PRODUCT_INTEL_US15W_IGD,
    PCI_VENDOR_INTEL,
};
use crate::dev::pci::pcireg::{
    PCI_COMMAND_IO_ENABLE, PCI_COMMAND_MASTER_ENABLE, PCI_COMMAND_MEM_ENABLE,
    PCI_COMMAND_STATUS_REG, PCI_SUBSYS_ID_REG, pci_product, pci_vendor,
};
use crate::dev::pci::pcivar::PciAttachArgs;
use crate::dev::pci::vga_pcivar::{VgaPciSoftc, device_is_vga_pci};
use crate::dev::wscons::wsconsio::{
    WSDISPLAY_TYPE_PCIVGA, WSDISPLAYIO_GETPARAM, WSDISPLAYIO_SETPARAM, WsdisplayParam,
};
use crate::dev::wscons::wsdisplayvar::{WsParamFn, ws_get_param, ws_set_param};
use crate::kern::subr_autoconf::config_activate_children;
use crate::kern::subr_prf::printf;
use crate::machine::bus::{BusSpaceTag, bus_space_read_region_2, bus_space_write_region_2};
use crate::machine::pci_machdep::{PciChipsetTag, pci_conf_read, pci_conf_write};
use crate::sys::device::{CfMatch, Cfattach, DVACT_RESUME, DVACT_SUSPEND, Device};
use crate::sys::errno::Errno;
use crate::sys::ioctl::{ioctl_arg, ioctl_ret};
use crate::sys::proc::Proc;

/// `struct vga_device_description`.
struct VgaDeviceDescription {
    /// `rval[4]`: vendor, product, subvendor, subproduct.
    rval: [u16; 4],
    /// `rmask[4]`: the masks of `rval`.
    rmask: [u16; 4],
    /// `vga_pci_post`: re-POST through the emulated BIOS on resume.
    vga_pci_post: i32,
}

/// `vga_pci_ca`.
pub static VGA_PCI_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<VgaPciSoftc>(),
    ca_match: Some(vga_pci_match),
    ca_attach: vga_pci_attach,
    ca_detach: None,
    ca_activate: Some(vga_pci_activate),
};

/// `vga_pci_do_post`.
static VGA_PCI_DO_POST: AtomicI32 = AtomicI32::new(0);

/// `vga_devs[]`.
///
/// Header description: first entry is a list of the pci video information in the
/// following order: VENDOR, PRODUCT, SUBVENDOR, SUBPRODUCT. The next entry is a list of
/// corresponding masks. Finally the last value indicates if we should repost via vga_pci
/// (i.e. the x86emulator) bios.
static VGA_DEVS: [VgaDeviceDescription; 6] = [
    // All machines with GMA500/Poulsbo
    VgaDeviceDescription {
        rval: [
            PCI_VENDOR_INTEL as u16,
            PCI_PRODUCT_INTEL_US15W_IGD as u16,
            0x0000,
            0x0000,
        ],
        rmask: [0xffff, 0xffff, 0x0000, 0x0000],
        vga_pci_post: 1,
    },
    // All machines with GMA500/Poulsbo
    VgaDeviceDescription {
        rval: [
            PCI_VENDOR_INTEL as u16,
            PCI_PRODUCT_INTEL_US15L_IGD as u16,
            0x0000,
            0x0000,
        ],
        rmask: [0xffff, 0xffff, 0x0000, 0x0000],
        vga_pci_post: 1,
    },
    // All machines with GMA600/Oaktrail, 0x4100:4107
    VgaDeviceDescription {
        rval: [
            PCI_VENDOR_INTEL as u16,
            PCI_PRODUCT_INTEL_GMA600_0 as u16,
            0x0000,
            0x0000,
        ],
        rmask: [0xffff, 0xfff8, 0x0000, 0x0000],
        vga_pci_post: 1,
    },
    // All machines with GMA600/Oaktrail, 0x4108
    VgaDeviceDescription {
        rval: [
            PCI_VENDOR_INTEL as u16,
            PCI_PRODUCT_INTEL_GMA600_8 as u16,
            0x0000,
            0x0000,
        ],
        rmask: [0xffff, 0xffff, 0x0000, 0x0000],
        vga_pci_post: 1,
    },
    // All machines with Medfield, 0x0130:0x0137
    VgaDeviceDescription {
        rval: [
            PCI_VENDOR_INTEL as u16,
            PCI_PRODUCT_INTEL_MDFLD_IGD_0 as u16,
            0x0000,
            0x0000,
        ],
        rmask: [0xffff, 0xfff8, 0x0000, 0x0000],
        vga_pci_post: 1,
    },
    // All machines with GMA36x0/Cedartrail, 0x0be0:0x0bef
    VgaDeviceDescription {
        rval: [
            PCI_VENDOR_INTEL as u16,
            PCI_PRODUCT_INTEL_GMA3600_0 as u16,
            0x0000,
            0x0000,
        ],
        rmask: [0xffff, 0xfff0, 0x0000, 0x0000],
        vga_pci_post: 1,
    },
];

/// The softc of a `vga` at `pci`.
fn sc_of(dev: &Device) -> &VgaPciSoftc {
    // SAFETY: `vga_pci_ca` makes `VgaPciSoftc`s, which live as long as the device.
    unsafe { dev.softc::<VgaPciSoftc>() }
}

/// `vga_pci_match`: a VGA-class function with I/O and memory decoding on that is the
/// console or answers `vga_common_probe`.
pub fn vga_pci_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: pci attaches its children with a `pci_attach_args`.
    let pa = unsafe { &*aux.cast::<PciAttachArgs>() };

    if !device_is_vga_pci(pa.pa_class) {
        return 0;
    }

    // check whether it is disabled by firmware
    if pci_conf_read(pa.pa_pc, pa.pa_tag, PCI_COMMAND_STATUS_REG)
        & (PCI_COMMAND_IO_ENABLE | PCI_COMMAND_MEM_ENABLE)
        != (PCI_COMMAND_IO_ENABLE | PCI_COMMAND_MEM_ENABLE)
    {
        return 0;
    }

    // If it's the console, we have a winner!
    if vga_is_console(pa.pa_iot, WSDISPLAY_TYPE_PCIVGA as i32) {
        return 1;
    }

    // If we might match, make sure that the card actually looks OK.
    if !vga_common_probe(pa.pa_iot, pa.pa_memt) {
        return 0;
    }

    1
}

/// `vga_pci_attach`.
pub fn vga_pci_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: pci attaches its children with a `pci_attach_args` it owns for the attach.
    let pa = unsafe { &*aux.cast::<PciAttachArgs>() };
    let sc = sc_of(self_);

    // Enable bus master; X might need this for accelerated graphics.
    let mut reg = pci_conf_read(pa.pa_pc, pa.pa_tag, PCI_COMMAND_STATUS_REG);
    reg |= PCI_COMMAND_MASTER_ENABLE;
    pci_conf_write(pa.pa_pc, pa.pa_tag, PCI_COMMAND_STATUS_REG, reg);

    sc.sc_type.set(WSDISPLAY_TYPE_PCIVGA as i32);

    printf(format_args!("\n"));

    // !SMALL_KERNEL && NACPI > 0

    // X86EMU: vga_post_init(pa_bus, pa_device, pa_function), or "couldn't set up vga POST
    // handler"; not ported (see the module's deviations).
    let _ = crate::unported!("vga_post_init (arch/amd64/pci/vga_post.c, option X86EMU)");
    sc.sc_posth.set(ptr::null_mut());

    let vend = pci_vendor(pa.pa_id);
    let prod = pci_product(pa.pa_id);
    let subid = pci_conf_read(pa.pa_pc, pa.pa_tag, PCI_SUBSYS_ID_REG);
    let subvend = pci_vendor(subid);
    let subprod = pci_product(subid);

    for d in &VGA_DEVS {
        if vend & u32::from(d.rmask[0]) == u32::from(d.rval[0])
            && prod & u32::from(d.rmask[1]) == u32::from(d.rval[1])
            && subvend & u32::from(d.rmask[2]) == u32::from(d.rval[2])
            && subprod & u32::from(d.rmask[3]) == u32::from(d.rval[3])
        {
            VGA_PCI_DO_POST.store(d.vga_pci_post, Ordering::Relaxed);
            break;
        }
    }

    // RAMDISK_HOOKS: "aperture needed" (not configured).

    let vc = vga_common_attach(self_, pa.pa_iot, pa.pa_memt, sc.sc_type.get());
    sc.sc_vc.set(vc.map_or(ptr::null(), ptr::from_ref));
}

/// `vga_pci_activate`: save the VGA's registers on suspend, restore them (after
/// re-POSTing the chips that need it) on resume.
pub fn vga_pci_activate(self_: &Device, act: i32) -> Result<(), Errno> {
    let sc = sc_of(self_);

    match act {
        DVACT_SUSPEND => {
            let rv = config_activate_children(self_, act);
            // Save the common vga state. This should theoretically only be necessary if we
            // intend to POST, but it is preferable to do it unconditionally, as many
            // systems do not restore this state correctly upon resume.
            vga_save_state(sc);
            rv
        }
        DVACT_RESUME => {
            // X86EMU
            if VGA_PCI_DO_POST.load(Ordering::Relaxed) != 0 {
                let _ =
                    crate::unported!("vga_post_call (arch/amd64/pci/vga_post.c, option X86EMU)");
            }
            vga_restore_state(sc);
            config_activate_children(self_, act)
        }
        _ => config_activate_children(self_, act),
    }
}

/// `vga_pci_cnattach`: the PCI VGA as the console.
pub fn vga_pci_cnattach(
    iot: BusSpaceTag,
    memt: BusSpaceTag,
    _pc: PciChipsetTag,
    _bus: i32,
    _device: i32,
    _function: i32,
) -> Result<(), Errno> {
    vga_cnattach(iot, memt, WSDISPLAY_TYPE_PCIVGA as i32, false)
}

/// `vga_pci_ioctl`: the brightness and backlight parameters, through the hooks a
/// backlight driver installs; `ENOTTY` for anything else (and without a hook).
pub fn vga_pci_ioctl(
    _v: *mut c_void,
    cmd: u64,
    addr: &mut [u8],
    _flag: i32,
    _pb: Option<&Proc>,
) -> Result<bool, Errno> {
    let param = |f: Option<WsParamFn>, data: &mut [u8]| {
        let Some(f) = f else {
            return Err(Errno::ENOTTY);
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
        WSDISPLAYIO_GETPARAM => param(ws_get_param(), addr),
        WSDISPLAYIO_SETPARAM => param(ws_set_param(), addr),
        _ => Err(Errno::ENOTTY),
    }
}

/// `vga_save_state`: the sequencer, CRTC, attribute and graphics registers, the palette
/// and, for a chip re-POSTed on resume, the active screen's cells.
pub fn vga_save_state(sc: &VgaPciSoftc) {
    let Some(vc) = sc.sc_vc() else {
        return;
    };

    let vh = &vc.hdl;

    // Save sequencer registers
    vga_ts_write!(vh, syncreset, 1); // stop sequencer
    let mut ts = [0u8; size_of::<RegVgats>()];
    for (i, b) in ts.iter_mut().enumerate().skip(1) {
        *b = _vga_ts_read(vh, i);
    }
    vga_ts_write!(vh, syncreset, 3); // start sequencer
    // pretend screen is not blanked
    let mode = offset_of!(RegVgats, mode);
    ts[mode] &= !0x20;
    ts[mode] |= 0x80;
    sc.sc_save_ts.set(ts);

    // Save CRTC registers
    let mut crtc = [0u8; size_of::<RegMc6845>()];
    for (i, b) in crtc.iter_mut().enumerate() {
        *b = _pcdisplay_6845_read(&vh.vh_ph, i);
    }
    sc.sc_save_crtc.set(crtc);

    // Save ATC registers
    let mut atc = [0u8; size_of::<RegVgaattr>()];
    for (i, b) in atc.iter_mut().enumerate() {
        *b = _vga_attr_read(vh, i);
    }
    sc.sc_save_atc.set(atc);

    // Save GDC registers
    let mut gdc = [0u8; size_of::<RegVgagdc>()];
    for (i, b) in gdc.iter_mut().enumerate() {
        *b = _vga_gdc_read(vh, i);
    }
    sc.sc_save_gdc.set(gdc);

    vga_save_palette(vc);

    // XXX should also save font data

    // Save current screen contents if we have backing store for it, and intend to POST on
    // resume.
    // XXX Since we don't allocate backing store unless the second VT is created, we could
    // XXX theoretically have no backing store available at this point.
    if VGA_PCI_DO_POST.load(Ordering::Relaxed) != 0
        && let Some(scr) = vc.active()
        && scr.pcs.active.get() != 0
    {
        let t = scr.pcs.type_();
        let mem = scr.pcs.mem();
        let n = ((t.ncols * t.nrows) as usize).min(mem.len());
        bus_space_read_region_2(
            vh.vh_memt(),
            vh.vh_memh(),
            scr.pcs.dispoffset.get() as usize,
            &mem[..n],
        );
    }
}

/// `vga_restore_state`: what `vga_save_state` saved, back into the VGA.
pub fn vga_restore_state(sc: &VgaPciSoftc) {
    let Some(vc) = sc.sc_vc() else {
        return;
    };

    let vh = &vc.hdl;

    // Restore sequencer registers
    vga_ts_write!(vh, syncreset, 1); // stop sequencer
    for (i, &b) in sc.sc_save_ts.get().iter().enumerate().skip(1) {
        _vga_ts_write(vh, i, b);
    }
    vga_ts_write!(vh, syncreset, 3); // start sequencer

    // Restore CRTC registers
    // unprotect registers 00-07
    vga_6845_write!(vh, vsynce, vga_6845_read!(vh, vsynce) & !0x80);
    for (i, &b) in sc.sc_save_crtc.get().iter().enumerate() {
        _pcdisplay_6845_write(&vh.vh_ph, i, b);
    }

    // Restore ATC registers
    for (i, &b) in sc.sc_save_atc.get().iter().enumerate() {
        _vga_attr_write(vh, i, b);
    }

    // Restore GDC registers
    for (i, &b) in sc.sc_save_gdc.get().iter().enumerate() {
        _vga_gdc_write(vh, i, b);
    }

    vga_restore_fonts(vc);
    vga_restore_palette(vc);

    // Restore current screen contents if we have backing store for it, and have POSTed on
    // resume.
    // XXX Since we don't allocate backing store unless the second VT is created, we could
    // XXX theoretically have no backing store available at this point.
    if VGA_PCI_DO_POST.load(Ordering::Relaxed) != 0
        && let Some(scr) = vc.active()
        && scr.pcs.active.get() != 0
    {
        let t = scr.pcs.type_();
        let mem = scr.pcs.mem();
        let n = ((t.ncols * t.nrows) as usize).min(mem.len());
        bus_space_write_region_2(
            vh.vh_memt(),
            vh.vh_memh(),
            scr.pcs.dispoffset.get() as usize,
            &mem[..n],
        );
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repost_table_matches_the_gma_chips() {
        let hit = |vend: u32, prod: u32| {
            VGA_DEVS.iter().any(|d| {
                vend & u32::from(d.rmask[0]) == u32::from(d.rval[0])
                    && prod & u32::from(d.rmask[1]) == u32::from(d.rval[1])
            })
        };
        assert!(hit(0x8086, 0x8108)); // US15W
        assert!(hit(0x8086, 0x4105)); // GMA600, 0x4100:4107
        assert!(hit(0x8086, 0x0bef)); // GMA36x0
        assert!(!hit(0x8086, 0x4109));
        assert!(!hit(0x1234, 0x1111)); // QEMU's std VGA
    }

    #[test]
    fn ioctl_without_hooks_is_not_ours() {
        let mut data = [0u8; size_of::<WsdisplayParam>()];
        assert_eq!(
            vga_pci_ioctl(ptr::null_mut(), WSDISPLAYIO_GETPARAM, &mut data, 0, None),
            Err(Errno::ENOTTY)
        );
        assert_eq!(
            vga_pci_ioctl(ptr::null_mut(), 0, &mut data, 0, None),
            Err(Errno::ENOTTY)
        );
    }
}
/* </TESTS> */
