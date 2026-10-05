/*	$OpenBSD: azalia.h,v 1.69 2019/10/14 02:04:35 jcs Exp $	*/
/*	$NetBSD: azalia.h,v 1.6 2006/01/16 14:15:26 kent Exp $	*/
/*	$OpenBSD: azalia.c,v 1.292 2026/04/04 09:01:13 jsg Exp $	*/
/*	$NetBSD: azalia.c,v 1.20 2006/05/07 08:31:44 kent Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 2005 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by TAMURA Kent
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
 * THIS SOFTWARE IS PROVIDED BY THE NETBSD FOUNDATION, INC. AND CONTRIBUTORS
 * ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE FOUNDATION OR CONTRIBUTORS
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

//! azalia(4): the Intel High Definition Audio controller (`azalia* at pci?`) and
//! `<dev/pci/azalia.h>`, its registers, codec verbs and codec types.
//!
//! Upstream: sys/dev/pci/azalia.c @ 3ce1f3f79392
//! Upstream: sys/dev/pci/azalia.h @ 3ce1f3f79392
//!
//! High Definition Audio Specification:
//! <http://www.intel.com/content/dam/www/public/us/en/documents/product-specifications/high-definition-audio-specification.pdf>
//!
//! The controller talks to its codecs through two DMA rings: commands go out on the CORB
//! ([`azalia_set_command`]) and responses come back on the RIRB ([`azalia_get_response`]),
//! both under `audio_lock`; unsolicited responses (jack sense, volume knob) are queued by
//! the interrupt handler and handed to the codec code by the `unsol_to` timeout. At attach
//! every codec on the link is walked ([`azalia_codec_init`]): its widgets, their
//! connections, the pins by priority, the DAC and ADC groups, the formats they can play
//! and record, and the mixer (`azalia_codec.c`). audio(4) then attaches on top through
//! [`AZALIA_HW_IF`]; playback and recording each use one stream descriptor whose buffer
//! descriptor list cuts audio(4)'s ring into blocks, with an interrupt per block
//! ([`azalia_stream_intr`] calls audio(4) back under `audio_lock`). The interrupt is
//! established `IPL_AUDIO | IPL_MPSAFE` as in C.
//!
//! TO DO (the C's): multiple codecs (needed?), multiple streams (needed?).
//!
//! ## Deviations
//! - AZALIA_DEBUG is not configured: the `DPRINTF`/`DPRINTFN` messages are not carried, and
//!   the `azalia_*_print_*` functions are their non-debug versions, empty.
//! - The C's `T *p; int n;` arrays of `codec_t` (`w`, `formats`, `opins`, `opins_d`,
//!   `ipins`, `ipins_d`, `mixers` with `nmixers`/`maxmixers`) and `widget_t`'s
//!   `connections` are `Vec`s, allocated fallibly (`try_reserve_exact`, the C's `M_NOWAIT`
//!   `ENOMEM` paths kept); the counts are their lengths (`nformats()`, `nopins()`, ...).
//!   Freeing a codec's widgets frees their connection lists too (the C leaks them).
//! - `az->codecs` is still a `mallocarray`ed array, with each `Codec` written in place
//!   ([`Codec::new`]) and dropped before the free, so that the zeroed softc stays valid.
//! - `widget_t`'s `union d` and `mixer_item_t`'s `union saved` are Rust unions of plain
//!   integers with safe accessor methods; `WidgetPin`'s `sequence`, `association`, `color`
//!   and `device` are `u32` (the configuration default's bit fields) and `Widget::type_` is
//!   `u32` (`COP_AWTYPE_*`); `enable` is a `bool`.
//! - The widget functions take the codec and the widget's nid (`azalia_widget_init(codec,
//!   nid)`) instead of a widget pointer into the codec plus the codec; `azalia_shutdown` takes
//!   the softc (the C's `void *`).
//! - [`azalia_comresp`] returns the response (`Result<u32, Errno>`) instead of storing it
//!   through `uint32_t *result`; where the C goes on with a `result` a failed command did
//!   not write, the previous value is kept (`azalia_codec_init`, `azalia_widget_init`) or 0
//!   is used (`azalia_codec_disconnect_stream`, where the C's would be uninitialised).
//! - [`azalia_get_response`], once it has its response, runs [`azalia_rirb_intr`] itself
//!   when the controller flags the RIRB interrupt (`RIRBSTS.RINTFL`), as the interrupt
//!   handler would next. QEMU's `intel-hda` fetches no further command until that flag is
//!   acknowledged (`RINTCNT` is 1), and the handler cannot run while autoconf holds
//!   `IPL_HIGH` or while `azalia_comresp` holds `audio_lock`; the C, which leaves the flag to
//!   the handler, finds the codec's first answer and times out on the second.
//! - The C's bare `-1` and `1` failures are `EIO` (a `-1` reaching audio(4) would be
//!   `ERESTART`).
//! - `azalia_codec_find_defdac` and `azalia_codec_find_defadc_sub` keep the C's guard
//!   `selected < sizeof(w->connections)` (the size of a pointer) and skip a `selected`
//!   beyond the connection list, where the C reads past it.
//! - `unsolq_wp` and `intctl` are atomics: the C reads them outside the lock that orders
//!   their writes (the unsolicited timeout; the interrupt handler against
//!   `azalia_stream_start`/`_halt`).
//! - The interrupt's name is the device's `dv_xname`, borrowed for as long as the
//!   interrupt is established.
//! - `azalia_set_params` gets both parameter blocks from audio(4) (never NULL), so the C's
//!   `p != NULL` / `r != NULL` tests are gone.
//! - The poll loops `for (i = 5000; i > 0; i--) { DELAY(10); ... }` are `azalia_poll`;
//!   `azalia_sorted_pins`, `azalia_codec_nbits`, `azalia_widget_label_conv`,
//!   `azalia_selected_conn`, `azalia_trigger`, `azalia_unsolq_put` and `azalia_rirb_entry`
//!   are helpers for code the C repeats.
//! - The mixer (`azalia_mixer_init`, `azalia_mixer_get`, `azalia_mixer_set`) is not ported
//!   yet (`azalia_codec.rs`): the codec has no mixer controls, so `mixerctl(1)` shows only
//!   audio(4)'s own `record.enable`.

use alloc::vec::Vec;
use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::slice;
use core::sync::atomic::{AtomicI32, AtomicU32, Ordering};

use libkern::strlcpy;

use crate::dev::audio::{AUDIO_LOCK, audio_attach_mi, audio_blksz_bytes};
use crate::dev::audio_if::{
    AUDIO_ENCODING_SLINEAR_LE, AUDIO_ENCODING_ULINEAR_LE, AudioHwIf, AudioIntr, AudioParams,
    audio_bps,
};
use crate::dev::pci::azalia_codec::{
    azalia_codec_enable_unsol, azalia_codec_fnode, azalia_codec_gpio_quirks,
    azalia_codec_init_dolby_atmos, azalia_codec_init_vtbl, azalia_codec_widget_quirks,
    azalia_init_dacgroup, azalia_mixer_delete, azalia_mixer_get, azalia_mixer_init,
    azalia_mixer_set, azalia_unsol_event, azalia_widget_enabled,
};
use crate::dev::pci::pci::{pci_matchbyid, pci_set_powerstate};
use crate::dev::pci::pci_map::pci_mapreg_map;
use crate::dev::pci::pci_subr::pci_findvendor;
use crate::dev::pci::pcidevs::*;
use crate::dev::pci::pcireg::{
    PCI_CLASS_MULTIMEDIA, PCI_COMMAND_BACKTOBACK_ENABLE, PCI_COMMAND_STATUS_REG,
    PCI_MAPREG_MEM_TYPE_MASK, PCI_MAPREG_TYPE_MASK, PCI_PMCSR_STATE_D0,
    PCI_SUBCLASS_MULTIMEDIA_HDAUDIO, PCI_SUBSYS_ID_REG, PciProductId, PciVendorId, pci_class,
    pci_product, pci_subclass, pci_vendor,
};
use crate::dev::pci::pcivar::{PciAttachArgs, PciMatchid, Pcireg};
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_malloc::{free, malloc, mallocarray};
use crate::kern::kern_timeout::{timeout_add_msec, timeout_del, timeout_set};
use crate::kern::subr_autoconf::{config_activate_children, config_detach_children};
use crate::kern::subr_prf::{Str, panic, printf, snprintf};
use crate::machine::bus::{
    BUS_DMA_COHERENT, BUS_DMA_NOWAIT, BusAddr, BusDmaSegment, BusDmaTag, BusDmamap, BusSize,
    BusSpaceHandle, BusSpaceTag, bus_dmamap_create, bus_dmamap_destroy, bus_dmamap_load,
    bus_dmamap_unload, bus_dmamem_alloc, bus_dmamem_free, bus_dmamem_map, bus_dmamem_unmap,
    bus_space_read_1, bus_space_read_2, bus_space_read_4, bus_space_unmap, bus_space_write_1,
    bus_space_write_2, bus_space_write_4,
};
use crate::machine::cpu::delay;
use crate::machine::intr::{IPL_AUDIO, IPL_MPSAFE};
use crate::machine::pci_machdep::{
    PciChipsetTag, Pcitag, pci_conf_read, pci_conf_write, pci_intr_disestablish,
    pci_intr_establish, pci_intr_map, pci_intr_map_msi, pci_intr_string,
};
use crate::sys::audioio::{
    AUMODE_PLAY, AUMODE_RECORD, AudioNaux, AudioNcd, AudioNheadphone, AudioNline, AudioNmicrophone,
    AudioNspeaker, MAX_AUDIO_DEV_LEN, MixerCtrl, MixerDevinfo, MixerLevel,
};
use crate::sys::device::{
    CD_SKIPHIBERNATE, CfMatch, Cfattach, Cfdriver, DV_DULL, DVACT_POWERDOWN, DVACT_QUIESCE,
    DVACT_RESUME, DVACT_SUSPEND, Device, Softc,
};
use crate::sys::endian::htole32;
use crate::sys::errno::Errno;
use crate::sys::fcntl::{FREAD, FWRITE};
use crate::sys::malloc::{M_DEVBUF, M_NOWAIT, M_ZERO};
use crate::sys::timeout::Timeout;
/// `HDA_GCAP`.
pub const HDA_GCAP: BusSize = 0x000;
/// `HDA_GCAP_NSDO_MASK`.
pub const HDA_GCAP_NSDO_MASK: u16 = 0x0006;
/// `HDA_GCAP_NSDO_1`.
pub const HDA_GCAP_NSDO_1: u16 = 0x0000;
/// `HDA_GCAP_NSDO_2`.
pub const HDA_GCAP_NSDO_2: u16 = 0x0002;
/// `HDA_GCAP_NSDO_4`.
pub const HDA_GCAP_NSDO_4: u16 = 0x0004;
/// `HDA_GCAP_NSDO_RESERVED`.
pub const HDA_GCAP_NSDO_RESERVED: u16 = 0x0006;
/// `HDA_GCAP_64OK`.
pub const HDA_GCAP_64OK: u16 = 0x0001;
/// `HDA_VMIN`.
pub const HDA_VMIN: BusSize = 0x002;
/// `HDA_VMAJ`.
pub const HDA_VMAJ: BusSize = 0x003;
/// `HDA_OUTPAY`.
pub const HDA_OUTPAY: BusSize = 0x004;
/// `HDA_INPAY`.
pub const HDA_INPAY: BusSize = 0x006;
/// `HDA_GCTL`.
pub const HDA_GCTL: BusSize = 0x008;
/// `HDA_GCTL_UNSOL`.
pub const HDA_GCTL_UNSOL: u32 = 0x00000100;
/// `HDA_GCTL_FCNTRL`.
pub const HDA_GCTL_FCNTRL: u32 = 0x00000002;
/// `HDA_GCTL_CRST`.
pub const HDA_GCTL_CRST: u32 = 0x00000001;
/// `HDA_WAKEEN`.
pub const HDA_WAKEEN: BusSize = 0x00c;
/// `HDA_WAKEEN_SDIWEN`.
pub const HDA_WAKEEN_SDIWEN: u16 = 0x7fff;
/// `HDA_STATESTS`.
pub const HDA_STATESTS: BusSize = 0x00e;
/// `HDA_STATESTS_SDIWAKE`.
pub const HDA_STATESTS_SDIWAKE: u16 = 0x7fff;
/// `HDA_GSTS`.
pub const HDA_GSTS: BusSize = 0x010;
/// `HDA_GSTS_FSTS`.
pub const HDA_GSTS_FSTS: u16 = 0x0002;
/// `HDA_OUTSTRMPAY`.
pub const HDA_OUTSTRMPAY: BusSize = 0x018;
/// `HDA_INSTRMPAY`.
pub const HDA_INSTRMPAY: BusSize = 0x01a;
/// `HDA_INTCTL`.
pub const HDA_INTCTL: BusSize = 0x020;
/// `HDA_INTCTL_GIE`.
pub const HDA_INTCTL_GIE: u32 = 0x80000000;
/// `HDA_INTCTL_CIE`.
pub const HDA_INTCTL_CIE: u32 = 0x40000000;
/// `HDA_INTCTL_SIE`.
pub const HDA_INTCTL_SIE: u32 = 0x3fffffff;
/// `HDA_INTSTS`.
pub const HDA_INTSTS: BusSize = 0x024;
/// `HDA_INTSTS_GIS`.
pub const HDA_INTSTS_GIS: u32 = 0x80000000;
/// `HDA_INTSTS_CIS`.
pub const HDA_INTSTS_CIS: u32 = 0x40000000;
/// `HDA_INTSTS_SIS`.
pub const HDA_INTSTS_SIS: u32 = 0x3fffffff;
/// `HDA_WALCLK`.
pub const HDA_WALCLK: BusSize = 0x030;
/// `HDA_SSYNC`.
pub const HDA_SSYNC: BusSize = 0x034;
/// `HDA_SSYNC_SSYNC`.
pub const HDA_SSYNC_SSYNC: u32 = 0x3fffffff;
/// `HDA_CORBLBASE`.
pub const HDA_CORBLBASE: BusSize = 0x040;
/// `HDA_CORBUBASE`.
pub const HDA_CORBUBASE: BusSize = 0x044;
/// `HDA_CORBWP`.
pub const HDA_CORBWP: BusSize = 0x048;
/// `HDA_CORBWP_CORBWP`.
pub const HDA_CORBWP_CORBWP: u16 = 0x00ff;
/// `HDA_CORBRP`.
pub const HDA_CORBRP: BusSize = 0x04a;
/// `HDA_CORBRP_CORBRPRST`.
pub const HDA_CORBRP_CORBRPRST: u16 = 0x8000;
/// `HDA_CORBRP_CORBRP`.
pub const HDA_CORBRP_CORBRP: u16 = 0x00ff;
/// `HDA_CORBCTL`.
pub const HDA_CORBCTL: BusSize = 0x04c;
/// `HDA_CORBCTL_CORBRUN`.
pub const HDA_CORBCTL_CORBRUN: u8 = 0x02;
/// `HDA_CORBCTL_CMEIE`.
pub const HDA_CORBCTL_CMEIE: u8 = 0x01;
/// `HDA_CORBSTS`.
pub const HDA_CORBSTS: BusSize = 0x04d;
/// `HDA_CORBSTS_CMEI`.
pub const HDA_CORBSTS_CMEI: u8 = 0x01;
/// `HDA_CORBSIZE`.
pub const HDA_CORBSIZE: BusSize = 0x04e;
/// `HDA_CORBSIZE_CORBSZCAP_MASK`.
pub const HDA_CORBSIZE_CORBSZCAP_MASK: u8 = 0xf0;
/// `HDA_CORBSIZE_CORBSZCAP_2`.
pub const HDA_CORBSIZE_CORBSZCAP_2: u8 = 0x10;
/// `HDA_CORBSIZE_CORBSZCAP_16`.
pub const HDA_CORBSIZE_CORBSZCAP_16: u8 = 0x20;
/// `HDA_CORBSIZE_CORBSZCAP_256`.
pub const HDA_CORBSIZE_CORBSZCAP_256: u8 = 0x40;
/// `HDA_CORBSIZE_CORBSIZE_MASK`.
pub const HDA_CORBSIZE_CORBSIZE_MASK: u8 = 0x03;
/// `HDA_CORBSIZE_CORBSIZE_2`.
pub const HDA_CORBSIZE_CORBSIZE_2: u8 = 0x00;
/// `HDA_CORBSIZE_CORBSIZE_16`.
pub const HDA_CORBSIZE_CORBSIZE_16: u8 = 0x01;
/// `HDA_CORBSIZE_CORBSIZE_256`.
pub const HDA_CORBSIZE_CORBSIZE_256: u8 = 0x02;
/// `HDA_RIRBLBASE`.
pub const HDA_RIRBLBASE: BusSize = 0x050;
/// `HDA_RIRBUBASE`.
pub const HDA_RIRBUBASE: BusSize = 0x054;
/// `HDA_RIRBWP`.
pub const HDA_RIRBWP: BusSize = 0x058;
/// `HDA_RIRBWP_RIRBWPRST`.
pub const HDA_RIRBWP_RIRBWPRST: u16 = 0x8000;
/// `HDA_RIRBWP_RIRBWP`.
pub const HDA_RIRBWP_RIRBWP: u16 = 0x00ff;
/// `HDA_RINTCNT`.
pub const HDA_RINTCNT: BusSize = 0x05a;
/// `HDA_RINTCNT_RINTCNT`.
pub const HDA_RINTCNT_RINTCNT: u16 = 0x00ff;
/// `HDA_RIRBCTL`.
pub const HDA_RIRBCTL: BusSize = 0x05c;
/// `HDA_RIRBCTL_RIRBOIC`.
pub const HDA_RIRBCTL_RIRBOIC: u8 = 0x04;
/// `HDA_RIRBCTL_RIRBDMAEN`.
pub const HDA_RIRBCTL_RIRBDMAEN: u8 = 0x02;
/// `HDA_RIRBCTL_RINTCTL`.
pub const HDA_RIRBCTL_RINTCTL: u8 = 0x01;
/// `HDA_RIRBSTS`.
pub const HDA_RIRBSTS: BusSize = 0x05d;
/// `HDA_RIRBSTS_RIRBOIS`.
pub const HDA_RIRBSTS_RIRBOIS: u8 = 0x04;
/// `HDA_RIRBSTS_RINTFL`.
pub const HDA_RIRBSTS_RINTFL: u8 = 0x01;
/// `HDA_RIRBSIZE`.
pub const HDA_RIRBSIZE: BusSize = 0x05e;
/// `HDA_RIRBSIZE_RIRBSZCAP_MASK`.
pub const HDA_RIRBSIZE_RIRBSZCAP_MASK: u8 = 0xf0;
/// `HDA_RIRBSIZE_RIRBSZCAP_2`.
pub const HDA_RIRBSIZE_RIRBSZCAP_2: u8 = 0x10;
/// `HDA_RIRBSIZE_RIRBSZCAP_16`.
pub const HDA_RIRBSIZE_RIRBSZCAP_16: u8 = 0x20;
/// `HDA_RIRBSIZE_RIRBSZCAP_256`.
pub const HDA_RIRBSIZE_RIRBSZCAP_256: u8 = 0x40;
/// `HDA_RIRBSIZE_RIRBSIZE_MASK`.
pub const HDA_RIRBSIZE_RIRBSIZE_MASK: u8 = 0x03;
/// `HDA_RIRBSIZE_RIRBSIZE_2`.
pub const HDA_RIRBSIZE_RIRBSIZE_2: u8 = 0x00;
/// `HDA_RIRBSIZE_RIRBSIZE_16`.
pub const HDA_RIRBSIZE_RIRBSIZE_16: u8 = 0x01;
/// `HDA_RIRBSIZE_RIRBSIZE_256`.
pub const HDA_RIRBSIZE_RIRBSIZE_256: u8 = 0x02;
/// `HDA_IC`.
pub const HDA_IC: BusSize = 0x060;
/// `HDA_IR`.
pub const HDA_IR: BusSize = 0x064;
/// `HDA_IRS`.
pub const HDA_IRS: BusSize = 0x068;
/// `HDA_IRS_IRRADD`.
pub const HDA_IRS_IRRADD: u16 = 0x00f0;
/// `HDA_IRS_IRRUNSOL`.
pub const HDA_IRS_IRRUNSOL: u16 = 0x0008;
/// `HDA_IRS_IRV`.
pub const HDA_IRS_IRV: u16 = 0x0002;
/// `HDA_IRS_ICB`.
pub const HDA_IRS_ICB: u16 = 0x0001;
/// `HDA_DPLBASE`.
pub const HDA_DPLBASE: BusSize = 0x070;
/// `HDA_DPLBASE_DPLBASE`.
pub const HDA_DPLBASE_DPLBASE: u32 = 0xffffff80;
/// `HDA_DPLBASE_ENABLE`.
pub const HDA_DPLBASE_ENABLE: u32 = 0x00000001;
/// `HDA_DPUBASE`.
pub const HDA_DPUBASE: BusSize = 0x074;
/// `HDA_SD_BASE`.
pub const HDA_SD_BASE: BusSize = 0x080;
/// `HDA_SD_CTL`.
pub const HDA_SD_CTL: BusSize = 0x00;
/// `HDA_SD_CTL_DEIE`.
pub const HDA_SD_CTL_DEIE: u16 = 0x0010;
/// `HDA_SD_CTL_FEIE`.
pub const HDA_SD_CTL_FEIE: u16 = 0x0008;
/// `HDA_SD_CTL_IOCE`.
pub const HDA_SD_CTL_IOCE: u16 = 0x0004;
/// `HDA_SD_CTL_RUN`.
pub const HDA_SD_CTL_RUN: u16 = 0x0002;
/// `HDA_SD_CTL_SRST`.
pub const HDA_SD_CTL_SRST: u16 = 0x0001;
/// `HDA_SD_CTL2`.
pub const HDA_SD_CTL2: BusSize = 0x02;
/// `HDA_SD_CTL2_STRM`.
pub const HDA_SD_CTL2_STRM: u8 = 0xf0;
/// `HDA_SD_CTL2_STRM_SHIFT`.
pub const HDA_SD_CTL2_STRM_SHIFT: u8 = 4;
/// `HDA_SD_CTL2_DIR`.
pub const HDA_SD_CTL2_DIR: u8 = 0x08;
/// `HDA_SD_CTL2_TP`.
pub const HDA_SD_CTL2_TP: u8 = 0x04;
/// `HDA_SD_CTL2_STRIPE`.
pub const HDA_SD_CTL2_STRIPE: u8 = 0x03;
/// `HDA_SD_STS`.
pub const HDA_SD_STS: BusSize = 0x03;
/// `HDA_SD_STS_FIFORDY`.
pub const HDA_SD_STS_FIFORDY: u8 = 0x20;
/// `HDA_SD_STS_DESE`.
pub const HDA_SD_STS_DESE: u8 = 0x10;
/// `HDA_SD_STS_FIFOE`.
pub const HDA_SD_STS_FIFOE: u8 = 0x08;
/// `HDA_SD_STS_BCIS`.
pub const HDA_SD_STS_BCIS: u8 = 0x04;
/// `HDA_SD_LPIB`.
pub const HDA_SD_LPIB: BusSize = 0x04;
/// `HDA_SD_CBL`.
pub const HDA_SD_CBL: BusSize = 0x08;
/// `HDA_SD_LVI`.
pub const HDA_SD_LVI: BusSize = 0x0c;
/// `HDA_SD_LVI_LVI`.
pub const HDA_SD_LVI_LVI: u16 = 0x00ff;
/// `HDA_SD_FIFOW`.
pub const HDA_SD_FIFOW: BusSize = 0x0e;
/// `HDA_SD_FIFOS`.
pub const HDA_SD_FIFOS: BusSize = 0x10;
/// `HDA_SD_FMT`.
pub const HDA_SD_FMT: BusSize = 0x12;
/// `HDA_SD_FMT_BASE`.
pub const HDA_SD_FMT_BASE: u16 = 0x4000;
/// `HDA_SD_FMT_BASE_48`.
pub const HDA_SD_FMT_BASE_48: u16 = 0x0000;
/// `HDA_SD_FMT_BASE_44`.
pub const HDA_SD_FMT_BASE_44: u16 = 0x4000;
/// `HDA_SD_FMT_MULT`.
pub const HDA_SD_FMT_MULT: u16 = 0x3800;
/// `HDA_SD_FMT_MULT_X1`.
pub const HDA_SD_FMT_MULT_X1: u16 = 0x0000;
/// `HDA_SD_FMT_MULT_X2`.
pub const HDA_SD_FMT_MULT_X2: u16 = 0x0800;
/// `HDA_SD_FMT_MULT_X3`.
pub const HDA_SD_FMT_MULT_X3: u16 = 0x1000;
/// `HDA_SD_FMT_MULT_X4`.
pub const HDA_SD_FMT_MULT_X4: u16 = 0x1800;
/// `HDA_SD_FMT_DIV`.
pub const HDA_SD_FMT_DIV: u16 = 0x0700;
/// `HDA_SD_FMT_DIV_BY1`.
pub const HDA_SD_FMT_DIV_BY1: u16 = 0x0000;
/// `HDA_SD_FMT_DIV_BY2`.
pub const HDA_SD_FMT_DIV_BY2: u16 = 0x0100;
/// `HDA_SD_FMT_DIV_BY3`.
pub const HDA_SD_FMT_DIV_BY3: u16 = 0x0200;
/// `HDA_SD_FMT_DIV_BY4`.
pub const HDA_SD_FMT_DIV_BY4: u16 = 0x0300;
/// `HDA_SD_FMT_DIV_BY5`.
pub const HDA_SD_FMT_DIV_BY5: u16 = 0x0400;
/// `HDA_SD_FMT_DIV_BY6`.
pub const HDA_SD_FMT_DIV_BY6: u16 = 0x0500;
/// `HDA_SD_FMT_DIV_BY7`.
pub const HDA_SD_FMT_DIV_BY7: u16 = 0x0600;
/// `HDA_SD_FMT_DIV_BY8`.
pub const HDA_SD_FMT_DIV_BY8: u16 = 0x0700;
/// `HDA_SD_FMT_BITS`.
pub const HDA_SD_FMT_BITS: u16 = 0x0070;
/// `HDA_SD_FMT_BITS_8_16`.
pub const HDA_SD_FMT_BITS_8_16: u16 = 0x0000;
/// `HDA_SD_FMT_BITS_16_16`.
pub const HDA_SD_FMT_BITS_16_16: u16 = 0x0010;
/// `HDA_SD_FMT_BITS_20_32`.
pub const HDA_SD_FMT_BITS_20_32: u16 = 0x0020;
/// `HDA_SD_FMT_BITS_24_32`.
pub const HDA_SD_FMT_BITS_24_32: u16 = 0x0030;
/// `HDA_SD_FMT_BITS_32_32`.
pub const HDA_SD_FMT_BITS_32_32: u16 = 0x0040;
/// `HDA_SD_FMT_CHAN`.
pub const HDA_SD_FMT_CHAN: u16 = 0x000f;
/// `HDA_SD_BDPL`.
pub const HDA_SD_BDPL: BusSize = 0x18;
/// `HDA_SD_BDPU`.
pub const HDA_SD_BDPU: BusSize = 0x1c;
/// `HDA_SD_SIZE`.
pub const HDA_SD_SIZE: BusSize = 0x20;
/// `CORB_GET_PARAMETER`.
pub const CORB_GET_PARAMETER: u32 = 0xf00;
/// `COP_VENDOR_ID`.
pub const COP_VENDOR_ID: u32 = 0x00;
/// `COP_REVISION_ID`.
pub const COP_REVISION_ID: u32 = 0x02;
/// `COP_SUBORDINATE_NODE_COUNT`.
pub const COP_SUBORDINATE_NODE_COUNT: u32 = 0x04;
/// `COP_FUNCTION_GROUP_TYPE`.
pub const COP_FUNCTION_GROUP_TYPE: u32 = 0x05;
/// `COP_FTYPE_RESERVED`.
pub const COP_FTYPE_RESERVED: u32 = 0x01;
/// `COP_FTYPE_AUDIO`.
pub const COP_FTYPE_AUDIO: u32 = 0x01;
/// `COP_FTYPE_MODEM`.
pub const COP_FTYPE_MODEM: u32 = 0x02;
/// `COP_AUDIO_FUNCTION_GROUP_CAPABILITY`.
pub const COP_AUDIO_FUNCTION_GROUP_CAPABILITY: u32 = 0x08;
/// `COP_AUDIO_WIDGET_CAP`.
pub const COP_AUDIO_WIDGET_CAP: u32 = 0x09;
/// `COP_AWTYPE_AUDIO_OUTPUT`.
pub const COP_AWTYPE_AUDIO_OUTPUT: u32 = 0x0;
/// `COP_AWTYPE_AUDIO_INPUT`.
pub const COP_AWTYPE_AUDIO_INPUT: u32 = 0x1;
/// `COP_AWTYPE_AUDIO_MIXER`.
pub const COP_AWTYPE_AUDIO_MIXER: u32 = 0x2;
/// `COP_AWTYPE_AUDIO_SELECTOR`.
pub const COP_AWTYPE_AUDIO_SELECTOR: u32 = 0x3;
/// `COP_AWTYPE_PIN_COMPLEX`.
pub const COP_AWTYPE_PIN_COMPLEX: u32 = 0x4;
/// `COP_AWTYPE_POWER`.
pub const COP_AWTYPE_POWER: u32 = 0x5;
/// `COP_AWTYPE_VOLUME_KNOB`.
pub const COP_AWTYPE_VOLUME_KNOB: u32 = 0x6;
/// `COP_AWTYPE_BEEP_GENERATOR`.
pub const COP_AWTYPE_BEEP_GENERATOR: u32 = 0x7;
/// `COP_AWTYPE_VENDOR_DEFINED`.
pub const COP_AWTYPE_VENDOR_DEFINED: u32 = 0xf;
/// `COP_AWCAP_STEREO`.
pub const COP_AWCAP_STEREO: u32 = 0x001;
/// `COP_AWCAP_INAMP`.
pub const COP_AWCAP_INAMP: u32 = 0x002;
/// `COP_AWCAP_OUTAMP`.
pub const COP_AWCAP_OUTAMP: u32 = 0x004;
/// `COP_AWCAP_AMPOV`.
pub const COP_AWCAP_AMPOV: u32 = 0x008;
/// `COP_AWCAP_FORMATOV`.
pub const COP_AWCAP_FORMATOV: u32 = 0x010;
/// `COP_AWCAP_STRIPE`.
pub const COP_AWCAP_STRIPE: u32 = 0x020;
/// `COP_AWCAP_PROC`.
pub const COP_AWCAP_PROC: u32 = 0x040;
/// `COP_AWCAP_UNSOL`.
pub const COP_AWCAP_UNSOL: u32 = 0x080;
/// `COP_AWCAP_CONNLIST`.
pub const COP_AWCAP_CONNLIST: u32 = 0x100;
/// `COP_AWCAP_DIGITAL`.
pub const COP_AWCAP_DIGITAL: u32 = 0x200;
/// `COP_AWCAP_POWER`.
pub const COP_AWCAP_POWER: u32 = 0x400;
/// `COP_AWCAP_LRSWAP`.
pub const COP_AWCAP_LRSWAP: u32 = 0x800;
/// `COP_PCM`.
pub const COP_PCM: u32 = 0x0a;
/// `COP_PCM_B32`.
pub const COP_PCM_B32: u32 = 0x00100000;
/// `COP_PCM_B24`.
pub const COP_PCM_B24: u32 = 0x00080000;
/// `COP_PCM_B20`.
pub const COP_PCM_B20: u32 = 0x00040000;
/// `COP_PCM_B16`.
pub const COP_PCM_B16: u32 = 0x00020000;
/// `COP_PCM_B8`.
pub const COP_PCM_B8: u32 = 0x00010000;
/// `COP_PCM_R3840`.
pub const COP_PCM_R3840: u32 = 0x00000800;
/// `COP_PCM_R1920`.
pub const COP_PCM_R1920: u32 = 0x00000400;
/// `COP_PCM_R1764`.
pub const COP_PCM_R1764: u32 = 0x00000200;
/// `COP_PCM_R960`.
pub const COP_PCM_R960: u32 = 0x00000100;
/// `COP_PCM_R882`.
pub const COP_PCM_R882: u32 = 0x00000080;
/// `COP_PCM_R480`.
pub const COP_PCM_R480: u32 = 0x00000040;
/// `COP_PCM_R441`.
pub const COP_PCM_R441: u32 = 0x00000020;
/// `COP_PCM_R320`.
pub const COP_PCM_R320: u32 = 0x00000010;
/// `COP_PCM_R220`.
pub const COP_PCM_R220: u32 = 0x00000008;
/// `COP_PCM_R160`.
pub const COP_PCM_R160: u32 = 0x00000004;
/// `COP_PCM_R110`.
pub const COP_PCM_R110: u32 = 0x00000002;
/// `COP_PCM_R80`.
pub const COP_PCM_R80: u32 = 0x00000001;
/// `COP_STREAM_FORMATS`.
pub const COP_STREAM_FORMATS: u32 = 0x0b;
/// `COP_STREAM_FORMAT_PCM`.
pub const COP_STREAM_FORMAT_PCM: u32 = 0x00000001;
/// `COP_STREAM_FORMAT_FLOAT32`.
pub const COP_STREAM_FORMAT_FLOAT32: u32 = 0x00000002;
/// `COP_STREAM_FORMAT_AC3`.
pub const COP_STREAM_FORMAT_AC3: u32 = 0x00000003;
/// `COP_PINCAP`.
pub const COP_PINCAP: u32 = 0x0c;
/// `COP_PINCAP_IMPEDANCE`.
pub const COP_PINCAP_IMPEDANCE: u32 = 0x00000001;
/// `COP_PINCAP_TRIGGER`.
pub const COP_PINCAP_TRIGGER: u32 = 0x00000002;
/// `COP_PINCAP_PRESENCE`.
pub const COP_PINCAP_PRESENCE: u32 = 0x00000004;
/// `COP_PINCAP_HEADPHONE`.
pub const COP_PINCAP_HEADPHONE: u32 = 0x00000008;
/// `COP_PINCAP_OUTPUT`.
pub const COP_PINCAP_OUTPUT: u32 = 0x00000010;
/// `COP_PINCAP_INPUT`.
pub const COP_PINCAP_INPUT: u32 = 0x00000020;
/// `COP_PINCAP_BALANCE`.
pub const COP_PINCAP_BALANCE: u32 = 0x00000040;
/// `COP_PINCAP_HDMI`.
pub const COP_PINCAP_HDMI: u32 = 0x00000080;
/// `COP_PINCAP_EAPD`.
pub const COP_PINCAP_EAPD: u32 = 0x00010000;
/// `COP_INPUT_AMPCAP`.
pub const COP_INPUT_AMPCAP: u32 = 0x0d;
/// `COP_AMPCAP_MUTE`.
pub const COP_AMPCAP_MUTE: u32 = 0x80000000;
/// `COP_CONNECTION_LIST_LENGTH`.
pub const COP_CONNECTION_LIST_LENGTH: u32 = 0x0e;
/// `COP_CLL_LONG`.
pub const COP_CLL_LONG: u32 = 0x00000080;
/// `COP_SUPPORTED_POWER_STATES`.
pub const COP_SUPPORTED_POWER_STATES: u32 = 0x0f;
/// `COP_PROCESSING_CAPABILITIES`.
pub const COP_PROCESSING_CAPABILITIES: u32 = 0x10;
/// `COP_GPIO_COUNT`.
pub const COP_GPIO_COUNT: u32 = 0x11;
/// `COP_GPIO_UNSOL`.
pub const COP_GPIO_UNSOL: u32 = 0x40000000;
/// `COP_GPIO_WAKE`.
pub const COP_GPIO_WAKE: u32 = 0x80000000;
/// `COP_OUTPUT_AMPCAP`.
pub const COP_OUTPUT_AMPCAP: u32 = 0x12;
/// `COP_VOLUME_KNOB_CAPABILITIES`.
pub const COP_VOLUME_KNOB_CAPABILITIES: u32 = 0x13;
/// `COP_VKCAP_DELTA`.
pub const COP_VKCAP_DELTA: u32 = 0x00000080;
/// `CORB_GET_CONNECTION_SELECT_CONTROL`.
pub const CORB_GET_CONNECTION_SELECT_CONTROL: u32 = 0xf01;
/// `CORB_SET_CONNECTION_SELECT_CONTROL`.
pub const CORB_SET_CONNECTION_SELECT_CONTROL: u32 = 0x701;
/// `CORB_GET_CONNECTION_LIST_ENTRY`.
pub const CORB_GET_CONNECTION_LIST_ENTRY: u32 = 0xf02;
/// `CORB_GET_PROCESSING_STATE`.
pub const CORB_GET_PROCESSING_STATE: u32 = 0xf03;
/// `CORB_SET_PROCESSING_STATE`.
pub const CORB_SET_PROCESSING_STATE: u32 = 0x703;
/// `CORB_GET_COEFFICIENT_INDEX`.
pub const CORB_GET_COEFFICIENT_INDEX: u32 = 0xd00;
/// `CORB_SET_COEFFICIENT_INDEX`.
pub const CORB_SET_COEFFICIENT_INDEX: u32 = 0x500;
/// `CORB_GET_PROCESSING_COEFFICIENT`.
pub const CORB_GET_PROCESSING_COEFFICIENT: u32 = 0xc00;
/// `CORB_SET_PROCESSING_COEFFICIENT`.
pub const CORB_SET_PROCESSING_COEFFICIENT: u32 = 0x400;
/// `CORB_GET_AMPLIFIER_GAIN_MUTE`.
pub const CORB_GET_AMPLIFIER_GAIN_MUTE: u32 = 0xb00;
/// `CORB_GAGM_INPUT`.
pub const CORB_GAGM_INPUT: u32 = 0x0000;
/// `CORB_GAGM_OUTPUT`.
pub const CORB_GAGM_OUTPUT: u32 = 0x8000;
/// `CORB_GAGM_RIGHT`.
pub const CORB_GAGM_RIGHT: u32 = 0x0000;
/// `CORB_GAGM_LEFT`.
pub const CORB_GAGM_LEFT: u32 = 0x2000;
/// `CORB_GAGM_MUTE`.
pub const CORB_GAGM_MUTE: u32 = 0x00000080;
/// `CORB_SET_AMPLIFIER_GAIN_MUTE`.
pub const CORB_SET_AMPLIFIER_GAIN_MUTE: u32 = 0x300;
/// `CORB_AGM_GAIN_MASK`.
pub const CORB_AGM_GAIN_MASK: u32 = 0x007f;
/// `CORB_AGM_MUTE`.
pub const CORB_AGM_MUTE: u32 = 0x0080;
/// `CORB_AGM_INDEX_SHIFT`.
pub const CORB_AGM_INDEX_SHIFT: u32 = 8;
/// `CORB_AGM_RIGHT`.
pub const CORB_AGM_RIGHT: u32 = 0x1000;
/// `CORB_AGM_LEFT`.
pub const CORB_AGM_LEFT: u32 = 0x2000;
/// `CORB_AGM_INPUT`.
pub const CORB_AGM_INPUT: u32 = 0x4000;
/// `CORB_AGM_OUTPUT`.
pub const CORB_AGM_OUTPUT: u32 = 0x8000;
/// `CORB_GET_CONVERTER_FORMAT`.
pub const CORB_GET_CONVERTER_FORMAT: u32 = 0xa00;
/// `CORB_SET_CONVERTER_FORMAT`.
pub const CORB_SET_CONVERTER_FORMAT: u32 = 0x200;
/// `CORB_GET_DIGITAL_CONTROL`.
pub const CORB_GET_DIGITAL_CONTROL: u32 = 0xf0d;
/// `CORB_SET_DIGITAL_CONTROL_L`.
pub const CORB_SET_DIGITAL_CONTROL_L: u32 = 0x70d;
/// `CORB_SET_DIGITAL_CONTROL_H`.
pub const CORB_SET_DIGITAL_CONTROL_H: u32 = 0x70e;
/// `CORB_DCC_DIGEN`.
pub const CORB_DCC_DIGEN: u32 = 0x01;
/// `CORB_DCC_V`.
pub const CORB_DCC_V: u32 = 0x02;
/// `CORB_DCC_VCFG`.
pub const CORB_DCC_VCFG: u32 = 0x04;
/// `CORB_DCC_PRE`.
pub const CORB_DCC_PRE: u32 = 0x08;
/// `CORB_DCC_COPY`.
pub const CORB_DCC_COPY: u32 = 0x10;
/// `CORB_DCC_NAUDIO`.
pub const CORB_DCC_NAUDIO: u32 = 0x20;
/// `CORB_DCC_PRO`.
pub const CORB_DCC_PRO: u32 = 0x40;
/// `CORB_DCC_L`.
pub const CORB_DCC_L: u32 = 0x80;
/// `CORB_GET_POWER_STATE`.
pub const CORB_GET_POWER_STATE: u32 = 0xf05;
/// `CORB_SET_POWER_STATE`.
pub const CORB_SET_POWER_STATE: u32 = 0x705;
/// `CORB_PS_D0`.
pub const CORB_PS_D0: u32 = 0x0;
/// `CORB_PS_D1`.
pub const CORB_PS_D1: u32 = 0x1;
/// `CORB_PS_D2`.
pub const CORB_PS_D2: u32 = 0x2;
/// `CORB_PS_D3`.
pub const CORB_PS_D3: u32 = 0x3;
/// `CORB_GET_CONVERTER_STREAM_CHANNEL`.
pub const CORB_GET_CONVERTER_STREAM_CHANNEL: u32 = 0xf06;
/// `CORB_SET_CONVERTER_STREAM_CHANNEL`.
pub const CORB_SET_CONVERTER_STREAM_CHANNEL: u32 = 0x706;
/// `CORB_GET_INPUT_CONVERTER_SDI_SELECT`.
pub const CORB_GET_INPUT_CONVERTER_SDI_SELECT: u32 = 0xf04;
/// `CORB_SET_INPUT_CONVERTER_SDI_SELECT`.
pub const CORB_SET_INPUT_CONVERTER_SDI_SELECT: u32 = 0x704;
/// `CORB_GET_PIN_WIDGET_CONTROL`.
pub const CORB_GET_PIN_WIDGET_CONTROL: u32 = 0xf07;
/// `CORB_SET_PIN_WIDGET_CONTROL`.
pub const CORB_SET_PIN_WIDGET_CONTROL: u32 = 0x707;
/// `CORB_PWC_HEADPHONE`.
pub const CORB_PWC_HEADPHONE: u32 = 0x80;
/// `CORB_PWC_OUTPUT`.
pub const CORB_PWC_OUTPUT: u32 = 0x40;
/// `CORB_PWC_INPUT`.
pub const CORB_PWC_INPUT: u32 = 0x20;
/// `CORB_PWC_VREF_MASK`.
pub const CORB_PWC_VREF_MASK: u32 = 0x07;
/// `CORB_PWC_VREF_HIZ`.
pub const CORB_PWC_VREF_HIZ: u32 = 0x00;
/// `CORB_PWC_VREF_50`.
pub const CORB_PWC_VREF_50: u32 = 0x01;
/// `CORB_PWC_VREF_GND`.
pub const CORB_PWC_VREF_GND: u32 = 0x02;
/// `CORB_PWC_VREF_80`.
pub const CORB_PWC_VREF_80: u32 = 0x04;
/// `CORB_PWC_VREF_100`.
pub const CORB_PWC_VREF_100: u32 = 0x05;
/// `CORB_GET_UNSOLICITED_RESPONSE`.
pub const CORB_GET_UNSOLICITED_RESPONSE: u32 = 0xf08;
/// `CORB_SET_UNSOLICITED_RESPONSE`.
pub const CORB_SET_UNSOLICITED_RESPONSE: u32 = 0x708;
/// `CORB_UNSOL_ENABLE`.
pub const CORB_UNSOL_ENABLE: u32 = 0x80;
/// `CORB_GET_PIN_SENSE`.
pub const CORB_GET_PIN_SENSE: u32 = 0xf09;
/// `CORB_PS_PRESENCE`.
pub const CORB_PS_PRESENCE: u32 = 0x80000000;
/// `CORB_EXECUTE_PIN_SENSE`.
pub const CORB_EXECUTE_PIN_SENSE: u32 = 0x709;
/// `CORB_PS_RIGHT`.
pub const CORB_PS_RIGHT: u32 = 0x1;
/// `CORB_GET_EAPD_BTL_ENABLE`.
pub const CORB_GET_EAPD_BTL_ENABLE: u32 = 0xf0c;
/// `CORB_SET_EAPD_BTL_ENABLE`.
pub const CORB_SET_EAPD_BTL_ENABLE: u32 = 0x70c;
/// `CORB_EAPD_BTL`.
pub const CORB_EAPD_BTL: u32 = 0x01;
/// `CORB_EAPD_EAPD`.
pub const CORB_EAPD_EAPD: u32 = 0x02;
/// `CORB_EAPD_LRSWAP`.
pub const CORB_EAPD_LRSWAP: u32 = 0x04;
/// `CORB_GET_GPI_DATA`.
pub const CORB_GET_GPI_DATA: u32 = 0xf10;
/// `CORB_SET_GPI_DATA`.
pub const CORB_SET_GPI_DATA: u32 = 0x710;
/// `CORB_GET_GPI_WAKE_ENABLE_MASK`.
pub const CORB_GET_GPI_WAKE_ENABLE_MASK: u32 = 0xf11;
/// `CORB_SET_GPI_WAKE_ENABLE_MASK`.
pub const CORB_SET_GPI_WAKE_ENABLE_MASK: u32 = 0x711;
/// `CORB_GET_GPI_UNSOLICITED_ENABLE_MASK`.
pub const CORB_GET_GPI_UNSOLICITED_ENABLE_MASK: u32 = 0xf12;
/// `CORB_SET_GPI_UNSOLICITED_ENABLE_MASK`.
pub const CORB_SET_GPI_UNSOLICITED_ENABLE_MASK: u32 = 0x712;
/// `CORB_GET_GPI_STICKY_MASK`.
pub const CORB_GET_GPI_STICKY_MASK: u32 = 0xf13;
/// `CORB_SET_GPI_STICKY_MASK`.
pub const CORB_SET_GPI_STICKY_MASK: u32 = 0x713;
/// `CORB_GET_GPO_DATA`.
pub const CORB_GET_GPO_DATA: u32 = 0xf14;
/// `CORB_SET_GPO_DATA`.
pub const CORB_SET_GPO_DATA: u32 = 0x714;
/// `CORB_GET_GPIO_DATA`.
pub const CORB_GET_GPIO_DATA: u32 = 0xf15;
/// `CORB_SET_GPIO_DATA`.
pub const CORB_SET_GPIO_DATA: u32 = 0x715;
/// `CORB_GET_GPIO_ENABLE_MASK`.
pub const CORB_GET_GPIO_ENABLE_MASK: u32 = 0xf16;
/// `CORB_SET_GPIO_ENABLE_MASK`.
pub const CORB_SET_GPIO_ENABLE_MASK: u32 = 0x716;
/// `CORB_GET_GPIO_DIRECTION`.
pub const CORB_GET_GPIO_DIRECTION: u32 = 0xf17;
/// `CORB_SET_GPIO_DIRECTION`.
pub const CORB_SET_GPIO_DIRECTION: u32 = 0x717;
/// `CORB_GET_GPIO_WAKE_ENABLE_MASK`.
pub const CORB_GET_GPIO_WAKE_ENABLE_MASK: u32 = 0xf18;
/// `CORB_SET_GPIO_WAKE_ENABLE_MASK`.
pub const CORB_SET_GPIO_WAKE_ENABLE_MASK: u32 = 0x718;
/// `CORB_GET_GPIO_UNSOLICITED_ENABLE_MASK`.
pub const CORB_GET_GPIO_UNSOLICITED_ENABLE_MASK: u32 = 0xf19;
/// `CORB_SET_GPIO_UNSOLICITED_ENABLE_MASK`.
pub const CORB_SET_GPIO_UNSOLICITED_ENABLE_MASK: u32 = 0x719;
/// `CORB_GET_GPIO_STICKY_MASK`.
pub const CORB_GET_GPIO_STICKY_MASK: u32 = 0xf1a;
/// `CORB_SET_GPIO_STICKY_MASK`.
pub const CORB_SET_GPIO_STICKY_MASK: u32 = 0x71a;
/// `CORB_GET_GPIO_POLARITY`.
pub const CORB_GET_GPIO_POLARITY: u32 = 0xfe7;
/// `CORB_SET_GPIO_POLARITY`.
pub const CORB_SET_GPIO_POLARITY: u32 = 0x7e7;
/// `CORB_GET_BEEP_GENERATION`.
pub const CORB_GET_BEEP_GENERATION: u32 = 0xf0a;
/// `CORB_SET_BEEP_GENERATION`.
pub const CORB_SET_BEEP_GENERATION: u32 = 0x70a;
/// `CORB_GET_VOLUME_KNOB`.
pub const CORB_GET_VOLUME_KNOB: u32 = 0xf0f;
/// `CORB_SET_VOLUME_KNOB`.
pub const CORB_SET_VOLUME_KNOB: u32 = 0x70f;
/// `CORB_VKNOB_DIRECT`.
pub const CORB_VKNOB_DIRECT: u32 = 0x80;
/// `CORB_GET_SUBSYSTEM_ID`.
pub const CORB_GET_SUBSYSTEM_ID: u32 = 0xf20;
/// `CORB_SET_SUBSYSTEM_ID_1`.
pub const CORB_SET_SUBSYSTEM_ID_1: u32 = 0x720;
/// `CORB_SET_SUBSYSTEM_ID_2`.
pub const CORB_SET_SUBSYSTEM_ID_2: u32 = 0x721;
/// `CORB_SET_SUBSYSTEM_ID_3`.
pub const CORB_SET_SUBSYSTEM_ID_3: u32 = 0x722;
/// `CORB_SET_SUBSYSTEM_ID_4`.
pub const CORB_SET_SUBSYSTEM_ID_4: u32 = 0x723;
/// `CORB_GET_CONFIGURATION_DEFAULT`.
pub const CORB_GET_CONFIGURATION_DEFAULT: u32 = 0xf1c;
/// `CORB_SET_CONFIGURATION_DEFAULT_1`.
pub const CORB_SET_CONFIGURATION_DEFAULT_1: u32 = 0x71c;
/// `CORB_SET_CONFIGURATION_DEFAULT_2`.
pub const CORB_SET_CONFIGURATION_DEFAULT_2: u32 = 0x71d;
/// `CORB_SET_CONFIGURATION_DEFAULT_3`.
pub const CORB_SET_CONFIGURATION_DEFAULT_3: u32 = 0x71e;
/// `CORB_SET_CONFIGURATION_DEFAULT_4`.
pub const CORB_SET_CONFIGURATION_DEFAULT_4: u32 = 0x71f;
/// `CORB_CD_SEQUENCE_MAX`.
pub const CORB_CD_SEQUENCE_MAX: u32 = 0x0f;
/// `CORB_CD_ASSOCIATION_MAX`.
pub const CORB_CD_ASSOCIATION_MAX: u32 = 0x0f;
/// `CORB_CD_MISC_MASK`.
pub const CORB_CD_MISC_MASK: u32 = 0x00000f00;
/// `CORB_CD_PRESENCEOV`.
pub const CORB_CD_PRESENCEOV: u32 = 0x1;
/// `CORB_CD_COLOR_UNKNOWN`.
pub const CORB_CD_COLOR_UNKNOWN: u32 = 0x0;
/// `CORB_CD_BLACK`.
pub const CORB_CD_BLACK: u32 = 0x1;
/// `CORB_CD_GRAY`.
pub const CORB_CD_GRAY: u32 = 0x2;
/// `CORB_CD_BLUE`.
pub const CORB_CD_BLUE: u32 = 0x3;
/// `CORB_CD_GREEN`.
pub const CORB_CD_GREEN: u32 = 0x4;
/// `CORB_CD_RED`.
pub const CORB_CD_RED: u32 = 0x5;
/// `CORB_CD_ORANGE`.
pub const CORB_CD_ORANGE: u32 = 0x6;
/// `CORB_CD_YELLOW`.
pub const CORB_CD_YELLOW: u32 = 0x7;
/// `CORB_CD_PURPLE`.
pub const CORB_CD_PURPLE: u32 = 0x8;
/// `CORB_CD_PINK`.
pub const CORB_CD_PINK: u32 = 0x9;
/// `CORB_CD_WHITE`.
pub const CORB_CD_WHITE: u32 = 0xe;
/// `CORB_CD_COLOR_OTHER`.
pub const CORB_CD_COLOR_OTHER: u32 = 0xf;
/// `CORB_CD_CONNECTION_OFFSET`.
pub const CORB_CD_CONNECTION_OFFSET: u32 = 16;
/// `CORB_CD_CONNECTION_BITS`.
pub const CORB_CD_CONNECTION_BITS: u32 = 0xf;
/// `CORB_CD_CONNECTION_MASK`.
pub const CORB_CD_CONNECTION_MASK: u32 = CORB_CD_CONNECTION_BITS << CORB_CD_CONNECTION_OFFSET;
/// `CORB_CD_CONN_UNKNOWN`.
pub const CORB_CD_CONN_UNKNOWN: u32 = 0x0;
/// `CORB_CD_18`.
pub const CORB_CD_18: u32 = 0x1;
/// `CORB_CD_14`.
pub const CORB_CD_14: u32 = 0x2;
/// `CORB_CD_ATAPI`.
pub const CORB_CD_ATAPI: u32 = 0x3;
/// `CORB_CD_RCA`.
pub const CORB_CD_RCA: u32 = 0x4;
/// `CORB_CD_OPTICAL`.
pub const CORB_CD_OPTICAL: u32 = 0x5;
/// `CORB_CD_OTHER_DIG`.
pub const CORB_CD_OTHER_DIG: u32 = 0x6;
/// `CORB_CD_OTHER_ANALOG`.
pub const CORB_CD_OTHER_ANALOG: u32 = 0x7;
/// `CORB_CD_DIN`.
pub const CORB_CD_DIN: u32 = 0x8;
/// `CORB_CD_XLF`.
pub const CORB_CD_XLF: u32 = 0x9;
/// `CORB_CD_RJ11`.
pub const CORB_CD_RJ11: u32 = 0xa;
/// `CORB_CD_CONN_COMB`.
pub const CORB_CD_CONN_COMB: u32 = 0xb;
/// `CORB_CD_CONN_OTHER`.
pub const CORB_CD_CONN_OTHER: u32 = 0xf;
/// `CORB_CD_DEVICE_OFFSET`.
pub const CORB_CD_DEVICE_OFFSET: u32 = 20;
/// `CORB_CD_DEVICE_BITS`.
pub const CORB_CD_DEVICE_BITS: u32 = 0xf;
/// `CORB_CD_DEVICE_MASK`.
pub const CORB_CD_DEVICE_MASK: u32 = CORB_CD_DEVICE_BITS << CORB_CD_DEVICE_OFFSET;
/// `CORB_CD_LINEOUT`.
pub const CORB_CD_LINEOUT: u32 = 0x0;
/// `CORB_CD_SPEAKER`.
pub const CORB_CD_SPEAKER: u32 = 0x1;
/// `CORB_CD_HEADPHONE`.
pub const CORB_CD_HEADPHONE: u32 = 0x2;
/// `CORB_CD_CD`.
pub const CORB_CD_CD: u32 = 0x3;
/// `CORB_CD_SPDIFOUT`.
pub const CORB_CD_SPDIFOUT: u32 = 0x4;
/// `CORB_CD_DIGITALOUT`.
pub const CORB_CD_DIGITALOUT: u32 = 0x5;
/// `CORB_CD_MODEMLINE`.
pub const CORB_CD_MODEMLINE: u32 = 0x6;
/// `CORB_CD_MODEMHANDSET`.
pub const CORB_CD_MODEMHANDSET: u32 = 0x7;
/// `CORB_CD_LINEIN`.
pub const CORB_CD_LINEIN: u32 = 0x8;
/// `CORB_CD_AUX`.
pub const CORB_CD_AUX: u32 = 0x9;
/// `CORB_CD_MICIN`.
pub const CORB_CD_MICIN: u32 = 0xa;
/// `CORB_CD_TELEPHONY`.
pub const CORB_CD_TELEPHONY: u32 = 0xb;
/// `CORB_CD_SPDIFIN`.
pub const CORB_CD_SPDIFIN: u32 = 0xc;
/// `CORB_CD_DIGITALIN`.
pub const CORB_CD_DIGITALIN: u32 = 0xd;
/// `CORB_CD_BEEP`.
pub const CORB_CD_BEEP: u32 = 0xe;
/// `CORB_CD_DEVICE_OTHER`.
pub const CORB_CD_DEVICE_OTHER: u32 = 0xf;
/// `CORB_CD_LOCATION_MASK`.
pub const CORB_CD_LOCATION_MASK: u32 = 0x3f000000;
/// `CORB_CD_LOC_GEO_NA`.
pub const CORB_CD_LOC_GEO_NA: u32 = 0x0;
/// `CORB_CD_REAR`.
pub const CORB_CD_REAR: u32 = 0x1;
/// `CORB_CD_FRONT`.
pub const CORB_CD_FRONT: u32 = 0x2;
/// `CORB_CD_LEFT`.
pub const CORB_CD_LEFT: u32 = 0x3;
/// `CORB_CD_RIGHT`.
pub const CORB_CD_RIGHT: u32 = 0x4;
/// `CORB_CD_TOP`.
pub const CORB_CD_TOP: u32 = 0x5;
/// `CORB_CD_BOTTOM`.
pub const CORB_CD_BOTTOM: u32 = 0x6;
/// `CORB_CD_LOC_SPEC0`.
pub const CORB_CD_LOC_SPEC0: u32 = 0x7;
/// `CORB_CD_LOC_SPEC1`.
pub const CORB_CD_LOC_SPEC1: u32 = 0x8;
/// `CORB_CD_LOC_SPEC2`.
pub const CORB_CD_LOC_SPEC2: u32 = 0x9;
/// `CORB_CD_EXTERNAL`.
pub const CORB_CD_EXTERNAL: u32 = 0x0;
/// `CORB_CD_INTERNAL`.
pub const CORB_CD_INTERNAL: u32 = 0x1;
/// `CORB_CD_SEPARATE`.
pub const CORB_CD_SEPARATE: u32 = 0x2;
/// `CORB_CD_LOC_OTHER`.
pub const CORB_CD_LOC_OTHER: u32 = 0x3;
/// `CORB_CD_PORT_OFFSET`.
pub const CORB_CD_PORT_OFFSET: u32 = 30;
/// `CORB_CD_PORT_BITS`.
pub const CORB_CD_PORT_BITS: u32 = 0x3;
/// `CORB_CD_PORT_MASK`.
pub const CORB_CD_PORT_MASK: u32 = CORB_CD_PORT_BITS << CORB_CD_PORT_OFFSET;
/// `CORB_CD_JACK`.
pub const CORB_CD_JACK: u32 = 0x0;
/// `CORB_CD_NONE`.
pub const CORB_CD_NONE: u32 = 0x1;
/// `CORB_CD_FIXED`.
pub const CORB_CD_FIXED: u32 = 0x2;
/// `CORB_CD_BOTH`.
pub const CORB_CD_BOTH: u32 = 0x3;
/// `CORB_GET_STRIPE_CONTROL`.
pub const CORB_GET_STRIPE_CONTROL: u32 = 0xf24;
/// `CORB_SET_STRIPE_CONTROL`: XXX typo in the spec?.
pub const CORB_SET_STRIPE_CONTROL: u32 = 0x720;
/// `CORB_EXECUTE_FUNCTION_RESET`.
pub const CORB_EXECUTE_FUNCTION_RESET: u32 = 0x7ff;
/// `CORB_NID_ROOT`.
pub const CORB_NID_ROOT: NidT = 0;
/// `HDA_MAX_CHANNELS`.
pub const HDA_MAX_CHANNELS: usize = 16;
/// `HDA_MAX_SENSE_PINS`.
pub const HDA_MAX_SENSE_PINS: usize = 16;
/// `HDA_MAX_CODECS`.
pub const HDA_MAX_CODECS: usize = 15;
/// `AZ_MAX_VOL_SLAVES`.
pub const AZ_MAX_VOL_SLAVES: usize = 16;
/// `AZ_TAG_SPKR`.
pub const AZ_TAG_SPKR: i32 = 0x01;
/// `AZ_TAG_PLAYVOL`.
pub const AZ_TAG_PLAYVOL: i32 = 0x02;
/// `AZ_CLASS_INPUT`.
pub const AZ_CLASS_INPUT: i32 = 0;
/// `AZ_CLASS_OUTPUT`.
pub const AZ_CLASS_OUTPUT: i32 = 1;
/// `AZ_CLASS_RECORD`.
pub const AZ_CLASS_RECORD: i32 = 2;
/// `AZ_QRK_NONE`.
pub const AZ_QRK_NONE: i32 = 0x00000000;
/// `AZ_QRK_GPIO_MASK`.
pub const AZ_QRK_GPIO_MASK: i32 = 0x00000fff;
/// `AZ_QRK_GPIO_UNMUTE_0`.
pub const AZ_QRK_GPIO_UNMUTE_0: i32 = 0x00000001;
/// `AZ_QRK_GPIO_UNMUTE_1`.
pub const AZ_QRK_GPIO_UNMUTE_1: i32 = 0x00000002;
/// `AZ_QRK_GPIO_UNMUTE_2`.
pub const AZ_QRK_GPIO_UNMUTE_2: i32 = 0x00000004;
/// `AZ_QRK_GPIO_UNMUTE_3`.
pub const AZ_QRK_GPIO_UNMUTE_3: i32 = 0x00000008;
/// `AZ_QRK_GPIO_UNMUTE_4`.
pub const AZ_QRK_GPIO_UNMUTE_4: i32 = 0x00000010;
/// `AZ_QRK_GPIO_UNMUTE_5`.
pub const AZ_QRK_GPIO_UNMUTE_5: i32 = 0x00000020;
/// `AZ_QRK_GPIO_UNMUTE_6`.
pub const AZ_QRK_GPIO_UNMUTE_6: i32 = 0x00000040;
/// `AZ_QRK_GPIO_UNMUTE_7`.
pub const AZ_QRK_GPIO_UNMUTE_7: i32 = 0x00000080;
/// `AZ_QRK_GPIO_POL_0`.
pub const AZ_QRK_GPIO_POL_0: i32 = 0x00000100;
/// `AZ_QRK_WID_MASK`.
pub const AZ_QRK_WID_MASK: i32 = 0x00fff000;
/// `AZ_QRK_WID_CDIN_1C`.
pub const AZ_QRK_WID_CDIN_1C: i32 = 0x00001000;
/// `AZ_QRK_WID_BEEP_1D`.
pub const AZ_QRK_WID_BEEP_1D: i32 = 0x00002000;
/// `AZ_QRK_WID_OVREF50`.
pub const AZ_QRK_WID_OVREF50: i32 = 0x00004000;
/// `AZ_QRK_WID_AD1981_OAMP`.
pub const AZ_QRK_WID_AD1981_OAMP: i32 = 0x00008000;
/// `AZ_QRK_WID_TPDOCK1`.
pub const AZ_QRK_WID_TPDOCK1: i32 = 0x00010000;
/// `AZ_QRK_WID_TPDOCK2`.
pub const AZ_QRK_WID_TPDOCK2: i32 = 0x00020000;
/// `AZ_QRK_WID_TPDOCK3`.
pub const AZ_QRK_WID_TPDOCK3: i32 = 0x00040000;
/// `AZ_QRK_WID_CLOSE_PCBEEP`.
pub const AZ_QRK_WID_CLOSE_PCBEEP: i32 = 0x00080000;
/// `AZ_QRK_ROUTE_SPKR2_DAC`.
pub const AZ_QRK_ROUTE_SPKR2_DAC: i32 = 0x01000000;
/// `AZ_QRK_DOLBY_ATMOS`.
pub const AZ_QRK_DOLBY_ATMOS: i32 = 0x02000000;
/// `BDLIST_ENTRY_IOC`.
pub const BDLIST_ENTRY_IOC: u32 = 0x00000001;
/// `HDA_BDL_MAX`.
pub const HDA_BDL_MAX: usize = 256;
/// `RIRB_RESP_UNSOL`.
pub const RIRB_RESP_UNSOL: u32 = 1 << 4;
/// `MI_TARGET_OUTAMP`.
pub const MI_TARGET_OUTAMP: i32 = 0x100;
/// `MI_TARGET_CONNLIST`.
pub const MI_TARGET_CONNLIST: i32 = 0x101;
/// `MI_TARGET_PINDIR`: for bidirectional pin.
pub const MI_TARGET_PINDIR: i32 = 0x102;
/// `MI_TARGET_PINBOOST`: for headphone pin.
pub const MI_TARGET_PINBOOST: i32 = 0x103;
/// `MI_TARGET_DAC`.
pub const MI_TARGET_DAC: i32 = 0x104;
/// `MI_TARGET_ADC`.
pub const MI_TARGET_ADC: i32 = 0x105;
/// `MI_TARGET_VOLUME`.
pub const MI_TARGET_VOLUME: i32 = 0x106;
/// `MI_TARGET_SPDIF`.
pub const MI_TARGET_SPDIF: i32 = 0x107;
/// `MI_TARGET_SPDIF_CC`.
pub const MI_TARGET_SPDIF_CC: i32 = 0x108;
/// `MI_TARGET_EAPD`.
pub const MI_TARGET_EAPD: i32 = 0x109;
/// `MI_TARGET_MUTESET`.
pub const MI_TARGET_MUTESET: i32 = 0x10a;
/// `MI_TARGET_PINSENSE`.
pub const MI_TARGET_PINSENSE: i32 = 0x10b;
/// `MI_TARGET_SENSESET`.
pub const MI_TARGET_SENSESET: i32 = 0x10c;
/// `MI_TARGET_PLAYVOL`.
pub const MI_TARGET_PLAYVOL: i32 = 0x10d;
/// `MI_TARGET_RECVOL`.
pub const MI_TARGET_RECVOL: i32 = 0x10e;
/// `MI_TARGET_MIXERSET`.
pub const MI_TARGET_MIXERSET: i32 = 0x10f;
/// `AZ_CODEC_TYPE_ANALOG`.
pub const AZ_CODEC_TYPE_ANALOG: i32 = 0;
/// `AZ_CODEC_TYPE_DIGITAL`.
pub const AZ_CODEC_TYPE_DIGITAL: i32 = 1;
/// `AZ_CODEC_TYPE_HDMI`.
pub const AZ_CODEC_TYPE_HDMI: i32 = 2;
/// `AZ_SPKR_MUTE_NONE`.
pub const AZ_SPKR_MUTE_NONE: i32 = 0;
/// `AZ_SPKR_MUTE_SPKR_MUTE`.
pub const AZ_SPKR_MUTE_SPKR_MUTE: i32 = 1;
/// `AZ_SPKR_MUTE_SPKR_DIR`.
pub const AZ_SPKR_MUTE_SPKR_DIR: i32 = 2;
/// `AZ_SPKR_MUTE_DAC_MUTE`.
pub const AZ_SPKR_MUTE_DAC_MUTE: i32 = 3;

// The constants of azalia.c.

/// `AUFMT_MAX_FREQUENCIES`.
pub const AUFMT_MAX_FREQUENCIES: usize = 16;

// ICH6/ICH7 constant values: PCI registers

/// `ICH_PCI_HDBARL`.
const ICH_PCI_HDBARL: i32 = 0x10;
/// `ICH_PCI_HDBARU`.
#[allow(dead_code)] // the C defines it too and never uses it
const ICH_PCI_HDBARU: i32 = 0x14;
/// `ICH_PCI_HDCTL`.
#[allow(dead_code)] // the C defines it too and never uses it
const ICH_PCI_HDCTL: i32 = 0x40;
/// `ICH_PCI_HDCTL_CLKDETCLR`.
#[allow(dead_code)] // the C defines it too and never uses it
const ICH_PCI_HDCTL_CLKDETCLR: u32 = 0x08;
/// `ICH_PCI_HDCTL_CLKDETEN`.
#[allow(dead_code)] // the C defines it too and never uses it
const ICH_PCI_HDCTL_CLKDETEN: u32 = 0x04;
/// `ICH_PCI_HDCTL_CLKDETINV`.
#[allow(dead_code)] // the C defines it too and never uses it
const ICH_PCI_HDCTL_CLKDETINV: u32 = 0x02;
/// `ICH_PCI_HDCTL_SIGNALMODE`.
#[allow(dead_code)] // the C defines it too and never uses it
const ICH_PCI_HDCTL_SIGNALMODE: u32 = 0x01;
/// `ICH_PCI_HDTCSEL`.
const ICH_PCI_HDTCSEL: i32 = 0x44;
/// `ICH_PCI_HDTCSEL_MASK`.
const ICH_PCI_HDTCSEL_MASK: u32 = 0x7;
/// `ICH_PCI_MMC`.
const ICH_PCI_MMC: i32 = 0x62;
/// `ICH_PCI_MMC_ME`.
const ICH_PCI_MMC_ME: u8 = 0x1;

/// `UNSOLQ_SIZE`: entries of the unsolicited response queue.
const UNSOLQ_SIZE: i32 = 256;

// PCI functions

/// `ATI_PCIE_SNOOP_REG`.
const ATI_PCIE_SNOOP_REG: i32 = 0x42;
/// `ATI_PCIE_SNOOP_MASK`.
const ATI_PCIE_SNOOP_MASK: u8 = 0xf8;
/// `ATI_PCIE_SNOOP_ENABLE`.
const ATI_PCIE_SNOOP_ENABLE: u8 = 0x02;
/// `NVIDIA_PCIE_SNOOP_REG`.
const NVIDIA_PCIE_SNOOP_REG: i32 = 0x4e;
/// `NVIDIA_PCIE_SNOOP_MASK`.
const NVIDIA_PCIE_SNOOP_MASK: u8 = 0xf0;
/// `NVIDIA_PCIE_SNOOP_ENABLE`.
const NVIDIA_PCIE_SNOOP_ENABLE: u8 = 0x0f;
/// `NVIDIA_HDA_ISTR_COH_REG`.
const NVIDIA_HDA_ISTR_COH_REG: i32 = 0x4d;
/// `NVIDIA_HDA_OSTR_COH_REG`.
const NVIDIA_HDA_OSTR_COH_REG: i32 = 0x4c;
/// `NVIDIA_HDA_STR_COH_ENABLE`.
const NVIDIA_HDA_STR_COH_ENABLE: u8 = 0x01;
/// `INTEL_PCIE_NOSNOOP_REG`.
const INTEL_PCIE_NOSNOOP_REG: i32 = 0x79;
/// `INTEL_PCIE_NOSNOOP_MASK`.
const INTEL_PCIE_NOSNOOP_MASK: u8 = 0xf7;
/// `INTEL_PCIE_NOSNOOP_ENABLE`.
#[allow(dead_code)] // the C defines it too and never uses it
const INTEL_PCIE_NOSNOOP_ENABLE: u8 = 0x08;

/// `MAX_PINS` of `azalia_codec_sort_pins`.
const MAX_PINS: usize = 16;

// memory-mapped types

/// `bdlist_entry_t`: one entry of a stream's buffer descriptor list.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BdlistEntry {
    /// `low`.
    pub low: u32,
    /// `high`.
    pub high: u32,
    /// `length`.
    pub length: u32,
    /// `flags`.
    pub flags: u32,
}

/// `dmaposition_t`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Dmaposition {
    /// `position`.
    pub position: u32,
    /// `reserved`.
    pub reserved: u32,
}

/// `corb_entry_t`.
pub type CorbEntry = u32;

/// `rirb_entry_t`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RirbEntry {
    /// `resp`.
    pub resp: u32,
    /// `resp_ex`.
    pub resp_ex: u32,
}

/// `nid_t`: a codec node ID (-1: none).
pub type NidT = i32;

/// `widget_t`'s `d.audio`: for AUDIO_INPUT/OUTPUT.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WidgetAudio {
    /// `encodings`.
    pub encodings: u32,
    /// `bits_rates`.
    pub bits_rates: u32,
}

/// `widget_t`'s `d.pin`: for PIN.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WidgetPin {
    /// `cap`.
    pub cap: u32,
    /// `config`.
    pub config: u32,
    /// `sequence`.
    pub sequence: u32,
    /// `association`.
    pub association: u32,
    /// `color`.
    pub color: u32,
    /// `device`.
    pub device: u32,
}

/// `widget_t`'s `d.volume`: for VOLUME_KNOB.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WidgetVolume {
    /// `cap`.
    pub cap: u32,
}

/// `widget_t`'s `union { ... } d`. Every member is plain 32-bit integers, so any member may
/// be read whatever member was written last, as in C; the accessors are the safe views.
#[repr(C)]
#[derive(Clone, Copy)]
pub union WidgetD {
    /// `audio`: for AUDIO_INPUT/OUTPUT.
    audio: WidgetAudio,
    /// `pin`: for PIN.
    pin: WidgetPin,
    /// `volume`: for VOLUME_KNOB.
    volume: WidgetVolume,
}

impl WidgetD {
    /// All zeros (the union's largest member, so every byte is initialised).
    pub const fn zeroed() -> Self {
        Self {
            pin: WidgetPin {
                cap: 0,
                config: 0,
                sequence: 0,
                association: 0,
                color: 0,
                device: 0,
            },
        }
    }

    /// `d.audio`.
    pub fn audio(&self) -> &WidgetAudio {
        // SAFETY: every byte of the union is initialised (`zeroed` writes the largest
        // member) and any bit pattern is a valid `WidgetAudio`.
        unsafe { &self.audio }
    }

    /// `d.audio`, to write.
    pub fn audio_mut(&mut self) -> &mut WidgetAudio {
        // SAFETY: as in `audio`; plain integers, so writing them leaves every member valid.
        unsafe { &mut self.audio }
    }

    /// `d.pin`.
    pub fn pin(&self) -> &WidgetPin {
        // SAFETY: as in `audio`.
        unsafe { &self.pin }
    }

    /// `d.pin`, to write.
    pub fn pin_mut(&mut self) -> &mut WidgetPin {
        // SAFETY: as in `audio_mut`.
        unsafe { &mut self.pin }
    }

    /// `d.volume`.
    pub fn volume(&self) -> &WidgetVolume {
        // SAFETY: as in `audio`.
        unsafe { &self.volume }
    }

    /// `d.volume`, to write.
    pub fn volume_mut(&mut self) -> &mut WidgetVolume {
        // SAFETY: as in `audio_mut`.
        unsafe { &mut self.volume }
    }
}

/// `widget_t`: one widget (node) of a codec's audio function.
#[derive(Clone)]
pub struct Widget {
    /// `nid`.
    pub nid: NidT,
    /// `enable`.
    pub enable: bool,
    /// `widgetcap`.
    pub widgetcap: u32,
    /// `type`: bits 20-23 of `widgetcap` (`COP_AWTYPE_*`).
    pub type_: u32,
    /// `parent`.
    pub parent: NidT,
    /// `mixer_class`: `AZ_CLASS_*`, -1 for none.
    pub mixer_class: i32,
    /// `connections[nconnections]`.
    pub connections: Vec<NidT>,
    /// `selected`: index into `connections`, -1 for none.
    pub selected: i32,
    /// `inamp_cap`.
    pub inamp_cap: u32,
    /// `outamp_cap`.
    pub outamp_cap: u32,
    /// `name`.
    pub name: [u8; MAX_AUDIO_DEV_LEN],
    /// `d`.
    pub d: WidgetD,
}

impl Widget {
    /// A widget as `M_ZERO` memory holds it.
    pub const fn new() -> Self {
        Self {
            nid: 0,
            enable: false,
            widgetcap: 0,
            type_: 0,
            parent: 0,
            mixer_class: 0,
            connections: Vec::new(),
            selected: 0,
            inamp_cap: 0,
            outamp_cap: 0,
            name: [0; MAX_AUDIO_DEV_LEN],
            d: WidgetD::zeroed(),
        }
    }

    /// `nconnections`.
    pub fn nconnections(&self) -> i32 {
        self.connections.len() as i32
    }
}

impl Default for Widget {
    fn default() -> Self {
        Self::new()
    }
}

/// `mixer_item_t`'s `union { ord; mask; value; } saved`.
#[repr(C)]
#[derive(Clone, Copy)]
pub union MixerItemSaved {
    /// `ord`.
    ord: i32,
    /// `mask`.
    mask: i32,
    /// `value`.
    value: MixerLevel,
}

impl MixerItemSaved {
    /// All zeros (the union's largest member).
    pub const fn zeroed() -> Self {
        Self {
            value: MixerLevel {
                num_channels: 0,
                level: [0; 8],
            },
        }
    }

    /// `saved.ord`.
    pub fn ord(&self) -> i32 {
        // SAFETY: every byte is initialised (`zeroed`) and any bit pattern is an `i32`.
        unsafe { self.ord }
    }

    /// `saved.ord = ord`.
    pub fn set_ord(&mut self, ord: i32) {
        self.ord = ord;
    }

    /// `saved.mask`.
    pub fn mask(&self) -> i32 {
        // SAFETY: as in `ord`.
        unsafe { self.mask }
    }

    /// `saved.mask = mask`.
    pub fn set_mask(&mut self, mask: i32) {
        self.mask = mask;
    }

    /// `saved.value`.
    pub fn value(&self) -> &MixerLevel {
        // SAFETY: as in `ord`; `MixerLevel` is an `int` and eight bytes, any bit pattern.
        unsafe { &self.value }
    }

    /// `saved.value`, to write.
    pub fn value_mut(&mut self) -> &mut MixerLevel {
        // SAFETY: as in `value`; writing plain integers leaves every member valid.
        unsafe { &mut self.value }
    }
}

/// `mixer_item_t`: one mixer control of the codec.
#[derive(Clone, Copy)]
pub struct MixerItem {
    /// `devinfo`.
    pub devinfo: MixerDevinfo,
    /// `nid`: target NID; 0 is invalid.
    pub nid: NidT,
    /// `target`: 0-15: inamp index, 0x100: outamp, ... (`MI_TARGET_*`).
    pub target: i32,
    /// `saved`.
    pub saved: MixerItemSaved,
}

impl MixerItem {
    /// A mixer item as `M_ZERO` memory holds it.
    pub const fn new() -> Self {
        Self {
            devinfo: MixerDevinfo::zeroed(),
            nid: 0,
            target: 0,
            saved: MixerItemSaved::zeroed(),
        }
    }
}

impl Default for MixerItem {
    fn default() -> Self {
        Self::new()
    }
}

/// `convgroup_t`: converters that play or record one stream together.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Convgroup {
    /// `nconv`.
    pub nconv: i32,
    /// `conv`.
    pub conv: [NidT; HDA_MAX_CHANNELS],
}

/// `convgroupset_t`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Convgroupset {
    /// `cur`.
    pub cur: i32,
    /// `ngroups`.
    pub ngroups: i32,
    /// `groups`.
    pub groups: [Convgroup; 2],
}

impl Convgroupset {
    /// `&set->groups[set->cur]`.
    pub fn current(&self) -> &Convgroup {
        &self.groups[self.cur as usize]
    }
}

/// `volgroup_t`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Volgroup {
    /// `master`.
    pub master: i32,
    /// `vol_l`.
    pub vol_l: i32,
    /// `vol_r`.
    pub vol_r: i32,
    /// `mute`.
    pub mute: i32,
    /// `hw_step`.
    pub hw_step: i32,
    /// `hw_nsteps`.
    pub hw_nsteps: i32,
    /// `slaves`.
    pub slaves: [NidT; AZ_MAX_VOL_SLAVES],
    /// `nslaves`.
    pub nslaves: i32,
    /// `mask`.
    pub mask: i32,
    /// `cur`.
    pub cur: i32,
}

/// `struct io_pin`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IoPin {
    /// `nid`: NID of pin.
    pub nid: NidT,
    /// `conv`: NID of default converter.
    pub conv: NidT,
    /// `prio`: assoc/seq/dir "priority".
    pub prio: i32,
}

/// `codec_t`: one HD Audio codec on the link.
pub struct Codec {
    /// `az`: the controller.
    pub az: *const AzaliaSoftc,
    /// `vid`: codec vendor/device ID.
    pub vid: u32,
    /// `subid`: PCI subvendor/device ID.
    pub subid: u32,
    /// `name`.
    pub name: Option<&'static str>,
    /// `address`.
    pub address: i32,
    /// `nfunctions`.
    pub nfunctions: i32,
    /// `audiofunc`: NID of an audio function node.
    pub audiofunc: NidT,
    /// `wstart`: start NID of audio widgets.
    pub wstart: NidT,
    /// `wend`: the last NID of audio widgets + 1.
    pub wend: NidT,
    /// `w`: widgets in the audio function. `w[0]` to `w[wstart-1]` are unused.
    pub w: Vec<Widget>,
    /// `codec_type`: `AZ_CODEC_TYPE_*`.
    pub codec_type: i32,
    /// `qrks`: `AZ_QRK_*`.
    pub qrks: i32,
    /// `dacs`.
    pub dacs: Convgroupset,
    /// `adcs`.
    pub adcs: Convgroupset,
    /// `running`.
    pub running: i32,
    /// `mixers[nmixers]`, room for `maxmixers` (the vector's capacity).
    pub mixers: Vec<MixerItem>,
    /// `formats[nformats]`.
    pub formats: Vec<AudioFormat>,
    /// `ipins[nipins]`.
    pub ipins: Vec<IoPin>,
    /// `ipins_d[nipins_d]`.
    pub ipins_d: Vec<IoPin>,
    /// `opins[nopins]`.
    pub opins: Vec<IoPin>,
    /// `opins_d[nopins_d]`.
    pub opins_d: Vec<IoPin>,
    /// `a_dacs`.
    pub a_dacs: [NidT; HDA_MAX_CHANNELS],
    /// `a_dacs_d`.
    pub a_dacs_d: [NidT; HDA_MAX_CHANNELS],
    /// `na_dacs`.
    pub na_dacs: i32,
    /// `na_dacs_d`.
    pub na_dacs_d: i32,
    /// `a_adcs`.
    pub a_adcs: [NidT; HDA_MAX_CHANNELS],
    /// `a_adcs_d`.
    pub a_adcs_d: [NidT; HDA_MAX_CHANNELS],
    /// `na_adcs`.
    pub na_adcs: i32,
    /// `na_adcs_d`.
    pub na_adcs_d: i32,
    /// `mic`: fixed (internal) mic.
    pub mic: NidT,
    /// `mic_adc`.
    pub mic_adc: NidT,
    /// `speaker`: fixed (internal) speaker.
    pub speaker: NidT,
    /// `speaker2`: 2nd fixed (internal) speaker.
    pub speaker2: NidT,
    /// `spkr_dac`: default DAC for speaker and speaker2.
    pub spkr_dac: NidT,
    /// `input_mixer`.
    pub input_mixer: NidT,
    /// `fhp`: front headphone jack.
    pub fhp: NidT,
    /// `fhp_dac`.
    pub fhp_dac: NidT,
    /// `nout_jacks`: number of default output jacks.
    pub nout_jacks: i32,
    /// `spkr_muted`.
    pub spkr_muted: i32,
    /// `spkr_muters`.
    pub spkr_muters: i32,
    /// `spkr_mute_method`: `AZ_SPKR_MUTE_*`.
    pub spkr_mute_method: i32,
    /// `playvols`.
    pub playvols: Volgroup,
    /// `recvols`.
    pub recvols: Volgroup,
    /// `sense_pins`.
    pub sense_pins: [NidT; HDA_MAX_SENSE_PINS],
    /// `nsense_pins`.
    pub nsense_pins: i32,
}

impl Codec {
    /// A codec as the C's `M_ZERO` allocation holds it, with `az` set.
    pub const fn new(az: *const AzaliaSoftc) -> Self {
        let vg = Volgroup {
            master: 0,
            vol_l: 0,
            vol_r: 0,
            mute: 0,
            hw_step: 0,
            hw_nsteps: 0,
            slaves: [0; AZ_MAX_VOL_SLAVES],
            nslaves: 0,
            mask: 0,
            cur: 0,
        };
        let cg = Convgroup {
            nconv: 0,
            conv: [0; HDA_MAX_CHANNELS],
        };
        let cgs = Convgroupset {
            cur: 0,
            ngroups: 0,
            groups: [cg; 2],
        };
        Self {
            az,
            vid: 0,
            subid: 0,
            name: None,
            address: 0,
            nfunctions: 0,
            audiofunc: 0,
            wstart: 0,
            wend: 0,
            w: Vec::new(),
            codec_type: 0,
            qrks: 0,
            dacs: cgs,
            adcs: cgs,
            running: 0,
            mixers: Vec::new(),
            formats: Vec::new(),
            ipins: Vec::new(),
            ipins_d: Vec::new(),
            opins: Vec::new(),
            opins_d: Vec::new(),
            a_dacs: [0; HDA_MAX_CHANNELS],
            a_dacs_d: [0; HDA_MAX_CHANNELS],
            na_dacs: 0,
            na_dacs_d: 0,
            a_adcs: [0; HDA_MAX_CHANNELS],
            a_adcs_d: [0; HDA_MAX_CHANNELS],
            na_adcs: 0,
            na_adcs_d: 0,
            mic: 0,
            mic_adc: 0,
            speaker: 0,
            speaker2: 0,
            spkr_dac: 0,
            input_mixer: 0,
            fhp: 0,
            fhp_dac: 0,
            nout_jacks: 0,
            spkr_muted: 0,
            spkr_muters: 0,
            spkr_mute_method: 0,
            playvols: vg,
            recvols: vg,
            sense_pins: [0; HDA_MAX_SENSE_PINS],
            nsense_pins: 0,
        }
    }

    /// `this->az`.
    pub fn az(&self) -> &AzaliaSoftc {
        // SAFETY: `azalia_get_ctrlr_caps` sets `az` to the softc that owns the codec array;
        // softcs outlive their codecs (`azalia_pci_detach` deletes the codecs first).
        unsafe { &*self.az }
    }

    /// `&this->w[nid]`.
    pub fn wi(&self, nid: NidT) -> &Widget {
        &self.w[nid as usize]
    }

    /// `&this->w[nid]`, to write.
    pub fn wi_mut(&mut self, nid: NidT) -> &mut Widget {
        &mut self.w[nid as usize]
    }

    /// `FOR_EACH_WIDGET(this, i)`: `wstart..wend`.
    pub fn widgets(&self) -> core::ops::Range<NidT> {
        self.wstart..self.wend
    }

    /// `nmixers`.
    pub fn nmixers(&self) -> i32 {
        self.mixers.len() as i32
    }

    /// `nformats`.
    pub fn nformats(&self) -> i32 {
        self.formats.len() as i32
    }

    /// `nopins`.
    pub fn nopins(&self) -> i32 {
        self.opins.len() as i32
    }

    /// `nipins`.
    pub fn nipins(&self) -> i32 {
        self.ipins.len() as i32
    }
}

/// `typedef struct audio_params audio_params_t`.
pub type AudioParamsT = AudioParams;

/// `struct audio_format`: one format the codec plays or records.
#[derive(Clone, Copy, Debug)]
pub struct AudioFormat {
    /// `driver_data`.
    pub driver_data: *mut c_void,
    /// `mode`: `AUMODE_PLAY` or `AUMODE_RECORD`.
    pub mode: i32,
    /// `encoding`.
    pub encoding: u32,
    /// `precision`.
    pub precision: u32,
    /// `channels`.
    pub channels: u32,
    /// `frequency_type`: 0: `frequency[0]` is lower limit, and `frequency[1]` is higher
    /// limit; 1-16: `frequency[0]` to `frequency[frequency_type-1]` are valid.
    pub frequency_type: u32,
    /// `frequency`: sampling rates.
    pub frequency: [u32; AUFMT_MAX_FREQUENCIES],
}

impl AudioFormat {
    /// A format as `M_ZERO` memory holds it.
    pub const fn new() -> Self {
        Self {
            driver_data: ptr::null_mut(),
            mode: 0,
            encoding: 0,
            precision: 0,
            channels: 0,
            frequency_type: 0,
            frequency: [0; AUFMT_MAX_FREQUENCIES],
        }
    }
}

impl Default for AudioFormat {
    fn default() -> Self {
        Self::new()
    }
}

/// `azalia_dma_t`: a DMA allocation, mapped into the kernel and loaded.
#[derive(Clone, Copy)]
pub struct AzaliaDma {
    /// `map`.
    pub map: Option<&'static BusDmamap>,
    /// `addr`: kernel virtual address (NULL: not allocated).
    pub addr: *mut u8,
    /// `segments`.
    pub segments: [BusDmaSegment; 1],
    /// `size`.
    pub size: usize,
}

impl AzaliaDma {
    /// `AZALIA_DMA_DMAADDR(p)`: the bus address of the loaded memory.
    pub fn dmaaddr(&self) -> BusAddr {
        match self.map.and_then(|map| map.dm_segs().first()) {
            Some(seg) => seg.get().ds_addr,
            None => panic(format_args!("azalia: DMA memory not loaded")),
        }
    }
}

/// `stream_t`: one DMA stream (the controller's stream descriptor and its buffers).
///
/// The members the interrupt handler reads (`intr`, `intr_arg`, `bufsize`, `blk`, `swpos`)
/// are set by `trigger_*` before the stream's interrupt is enabled and are otherwise
/// touched only under [`AUDIO_LOCK`], as in C.
pub struct Stream {
    /// `az`.
    pub az: Cell<*const AzaliaSoftc>,
    /// `regbase`: the stream descriptor's register offset.
    pub regbase: Cell<BusSize>,
    /// `number`.
    pub number: Cell<i32>,
    /// `dir`: `AUMODE_PLAY` or `AUMODE_RECORD`.
    pub dir: Cell<i32>,
    /// `intr_bit`.
    pub intr_bit: Cell<u32>,
    /// `bdlist`.
    pub bdlist: Cell<AzaliaDma>,
    /// `buffer`.
    pub buffer: Cell<AzaliaDma>,
    /// `intr`.
    pub intr: Cell<Option<AudioIntr>>,
    /// `intr_arg`.
    pub intr_arg: Cell<*mut c_void>,
    /// `bufsize`.
    pub bufsize: Cell<i32>,
    /// `fmt`.
    pub fmt: Cell<u16>,
    /// `blk`.
    pub blk: Cell<i32>,
    /// `swpos`: position in the audio(4) layer.
    pub swpos: Cell<u32>,
}

impl Stream {
    /// `this->az`.
    fn az(&self) -> &AzaliaSoftc {
        // SAFETY: `azalia_stream_init` points it at the softc that embeds the stream.
        unsafe { &*self.az.get() }
    }

    /// `STR_READ_1(s, r)`.
    fn str_read_1(&self, r: BusSize) -> u8 {
        let (iot, ioh) = self.az().regs();
        bus_space_read_1(iot, ioh, self.regbase.get() + r)
    }

    /// `STR_READ_2(s, r)`.
    fn str_read_2(&self, r: BusSize) -> u16 {
        let (iot, ioh) = self.az().regs();
        bus_space_read_2(iot, ioh, self.regbase.get() + r)
    }

    /// `STR_READ_4(s, r)`.
    fn str_read_4(&self, r: BusSize) -> u32 {
        let (iot, ioh) = self.az().regs();
        bus_space_read_4(iot, ioh, self.regbase.get() + r)
    }

    /// `STR_WRITE_1(s, r, v)`.
    fn str_write_1(&self, r: BusSize, v: u8) {
        let (iot, ioh) = self.az().regs();
        bus_space_write_1(iot, ioh, self.regbase.get() + r, v)
    }

    /// `STR_WRITE_2(s, r, v)`.
    fn str_write_2(&self, r: BusSize, v: u16) {
        let (iot, ioh) = self.az().regs();
        bus_space_write_2(iot, ioh, self.regbase.get() + r, v)
    }

    /// `STR_WRITE_4(s, r, v)`.
    fn str_write_4(&self, r: BusSize, v: u32) {
        let (iot, ioh) = self.az().regs();
        bus_space_write_4(iot, ioh, self.regbase.get() + r, v)
    }
}

/// `azalia_t`: the softc.
#[repr(C)]
pub struct AzaliaSoftc {
    /// `dev`.
    pub dev: Device,

    /// `pc`.
    pub pc: Cell<Option<PciChipsetTag>>,
    /// `tag`.
    pub tag: Cell<Pcitag>,
    /// `ih`.
    pub ih: Cell<Option<NonNull<c_void>>>,
    /// `iot`.
    pub iot: Cell<Option<BusSpaceTag>>,
    /// `ioh`.
    pub ioh: Cell<Option<BusSpaceHandle>>,
    /// `map_size`.
    pub map_size: Cell<BusSize>,
    /// `dmat`.
    pub dmat: Cell<Option<BusDmaTag>>,
    /// `pciid`.
    pub pciid: Cell<Pcireg>,
    /// `subid`.
    pub subid: Cell<u32>,

    /// `codecs[ncodecs]`. Protected by: the kernel lock (attach, audio(4)'s system calls and
    /// the `unsol_to` timeout, none of them MPSAFE; the interrupt handler never touches it).
    pub codecs: Cell<*mut Codec>,
    /// `ncodecs`: number of codecs.
    pub ncodecs: Cell<i32>,
    /// `codecno`: index of the using codec.
    pub codecno: Cell<i32>,
    /// `detached`: 1 if failed to initialize, 2 if `azalia_pci_detach` has run.
    pub detached: Cell<i32>,
    /// `corb_dma`.
    pub corb_dma: Cell<AzaliaDma>,
    /// `corb_entries`.
    pub corb_entries: Cell<i32>,
    /// `corbsize`.
    pub corbsize: Cell<u8>,
    /// `rirb_dma`.
    pub rirb_dma: Cell<AzaliaDma>,
    /// `rirb_entries`.
    pub rirb_entries: Cell<i32>,
    /// `rirbsize`.
    pub rirbsize: Cell<u8>,
    /// `rirb_rp`. Protected by: [`AUDIO_LOCK`].
    pub rirb_rp: Cell<i32>,
    /// `unsolq[UNSOLQ_SIZE]`. An entry is written under [`AUDIO_LOCK`] before `unsolq_wp`
    /// moves past it.
    pub unsolq: Cell<*mut RirbEntry>,
    /// `unsolq_wp`: written under [`AUDIO_LOCK`]; the `unsol_to` timeout reads it unlocked,
    /// as the C does, so it is an atomic (release store, acquire load).
    pub unsolq_wp: AtomicI32,
    /// `unsolq_rp`. Protected by: the kernel lock (the timeout, and the attach and resume
    /// that reset it).
    pub unsolq_rp: Cell<i32>,
    /// `unsolq_kick`. Protected by: the kernel lock.
    pub unsolq_kick: Cell<i32>,
    /// `unsol_to`.
    pub unsol_to: Timeout,

    /// `ok64`.
    pub ok64: Cell<i32>,
    /// `nistreams`.
    pub nistreams: Cell<i32>,
    /// `nostreams`.
    pub nostreams: Cell<i32>,
    /// `nbstreams`.
    pub nbstreams: Cell<i32>,
    /// `pstream`.
    pub pstream: Stream,
    /// `rstream`.
    pub rstream: Stream,
    /// `intctl`: changed by `azalia_stream_start`/`_halt` and read by the interrupt
    /// handler, which the C lets race; an atomic here.
    pub intctl: AtomicU32,
}

// SAFETY: `#[repr(C)]` with the device first; the other members are `Cell`s of integers, raw
// pointers and `Option`s of references, function pointers and `Copy` tags (all valid as zero
// bits), atomics and a `Timeout`, which is valid zeroed (as every softc that embeds one).
unsafe impl Softc for AzaliaSoftc {}

impl AzaliaSoftc {
    /// `(az->iot, az->ioh)`.
    fn regs(&self) -> (BusSpaceTag, BusSpaceHandle) {
        match (self.iot.get(), self.ioh.get()) {
            (Some(t), Some(h)) => (t, h),
            _ => panic(format_args!("azalia: registers not mapped")),
        }
    }

    /// `az->pc`.
    fn pc(&self) -> PciChipsetTag {
        match self.pc.get() {
            Some(pc) => pc,
            None => panic(format_args!("azalia: no chipset tag")),
        }
    }

    /// `az->dmat`.
    fn dmat(&self) -> BusDmaTag {
        match self.dmat.get() {
            Some(t) => t,
            None => panic(format_args!("azalia: no DMA tag")),
        }
    }

    /// `AZ_READ_1(z, r)`.
    fn az_read_1(&self, r: BusSize) -> u8 {
        let (iot, ioh) = self.regs();
        bus_space_read_1(iot, ioh, r)
    }

    /// `AZ_READ_2(z, r)`.
    fn az_read_2(&self, r: BusSize) -> u16 {
        let (iot, ioh) = self.regs();
        bus_space_read_2(iot, ioh, r)
    }

    /// `AZ_READ_4(z, r)`.
    fn az_read_4(&self, r: BusSize) -> u32 {
        let (iot, ioh) = self.regs();
        bus_space_read_4(iot, ioh, r)
    }

    /// `AZ_WRITE_1(z, r, v)`.
    fn az_write_1(&self, r: BusSize, v: u8) {
        let (iot, ioh) = self.regs();
        bus_space_write_1(iot, ioh, r, v)
    }

    /// `AZ_WRITE_2(z, r, v)`.
    fn az_write_2(&self, r: BusSize, v: u16) {
        let (iot, ioh) = self.regs();
        bus_space_write_2(iot, ioh, r, v)
    }

    /// `AZ_WRITE_4(z, r, v)`.
    fn az_write_4(&self, r: BusSize, v: u32) {
        let (iot, ioh) = self.regs();
        bus_space_write_4(iot, ioh, r, v)
    }

    /// `az->codecs[0..ncodecs]`.
    ///
    /// # Safety
    ///
    /// The caller holds the kernel lock and no other reference into the codec array is live
    /// for as long as the returned one.
    #[allow(clippy::mut_from_ref)] // the C's `codec_t *`; the kernel lock serialises it
    pub unsafe fn codecs(&self) -> &mut [Codec] {
        let p = self.codecs.get();
        let n = self.ncodecs.get();
        if p.is_null() || n <= 0 {
            return &mut [];
        }
        // SAFETY: `azalia_get_ctrlr_caps` allocated and initialised `ncodecs` codecs at
        // `codecs`; the caller guarantees exclusive access.
        unsafe { slice::from_raw_parts_mut(p, n as usize) }
    }

    /// `&az->codecs[az->codecno]`.
    ///
    /// # Safety
    ///
    /// As for [`AzaliaSoftc::codecs`].
    #[allow(clippy::mut_from_ref)] // as `codecs`
    pub unsafe fn cur_codec(&self) -> &mut Codec {
        let no = self.codecno.get() as usize;
        // SAFETY: forwarded.
        let codecs = unsafe { self.codecs() };
        &mut codecs[no]
    }

    /// The device name for `pci_intr_establish`, which keeps it as long as the interrupt is
    /// established (`evcount(9)`): `dv_xname`, borrowed from the softc. A port helper, not an
    /// OpenBSD function.
    fn intr_name(&self) -> &'static str {
        let name = self.dev.xname();
        // SAFETY: the softc is never freed while its interrupt is established:
        // `azalia_pci_detach` disestablishes it, and nothing frees an azalia softc.
        unsafe { &*ptr::from_ref(name) }
    }
}

/// `azalia_ca`.
pub static AZALIA_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<AzaliaSoftc>(),
    ca_match: Some(azalia_pci_match),
    ca_attach: azalia_pci_attach,
    ca_detach: Some(azalia_pci_detach),
    ca_activate: Some(azalia_pci_activate),
};

/// `azalia_cd`.
pub static AZALIA_CD: Cfdriver = Cfdriver::new(b"azalia", DV_DULL, CD_SKIPHIBERNATE);

/// `azalia_hw_if`.
pub static AZALIA_HW_IF: AudioHwIf = AudioHwIf {
    open: Some(azalia_open),
    close: Some(azalia_close),
    set_params: Some(azalia_set_params),
    halt_output: Some(azalia_halt_output),
    halt_input: Some(azalia_halt_input),
    set_port: Some(azalia_set_port),
    get_port: Some(azalia_get_port),
    query_devinfo: Some(azalia_query_devinfo),
    allocm: Some(azalia_allocm),
    freem: Some(azalia_freem),
    round_buffersize: Some(azalia_round_buffersize),
    trigger_output: Some(azalia_trigger_output),
    trigger_input: Some(azalia_trigger_input),
    set_blksz: Some(azalia_set_blksz),
    set_nblks: Some(azalia_set_nblks),
    ..AudioHwIf::new()
};

/// `pin_devices`.
static PIN_DEVICES: [&[u8]; 16] = [
    AudioNline,
    AudioNspeaker,
    AudioNheadphone,
    AudioNcd,
    b"SPDIF",
    b"digital-out",
    b"modem-line",
    b"modem-handset",
    b"line-in",
    AudioNaux,
    AudioNmicrophone,
    b"telephony",
    b"SPDIF-in",
    b"digital-in",
    b"beep",
    b"other",
];

/// `wtypes`.
static WTYPES: [&str; 16] = [
    "dac", "adc", "mix", "sel", "pin", "pow", "volume", "beep", "wid08", "wid09", "wid0a", "wid0b",
    "wid0c", "wid0d", "wid0e", "vendor",
];

/// `line_colors`.
static LINE_COLORS: [&str; 16] = [
    "unk", "blk", "gry", "blu", "grn", "red", "org", "yel", "pur", "pnk", "0xa", "0xb", "0xc",
    "0xd", "wht", "oth",
];

/// `azalia_pci_devices`: the Intel controllers that do not say they are HD Audio.
static AZALIA_PCI_DEVICES: [PciMatchid; 20] = [
    intel(PCI_PRODUCT_INTEL_200SERIES_U_HDA),
    intel(PCI_PRODUCT_INTEL_300SERIES_CAVS),
    intel(PCI_PRODUCT_INTEL_300SERIES_U_HDA),
    intel(PCI_PRODUCT_INTEL_400SERIES_CAVS),
    intel(PCI_PRODUCT_INTEL_400SERIES_LP_HDA),
    intel(PCI_PRODUCT_INTEL_500SERIES_HDA),
    intel(PCI_PRODUCT_INTEL_500SERIES_LP_HDA),
    intel(PCI_PRODUCT_INTEL_600SERIES_LP_HDA),
    intel(PCI_PRODUCT_INTEL_700SERIES_LP_HDA),
    intel(PCI_PRODUCT_INTEL_800SERIES_HDA),
    intel(PCI_PRODUCT_INTEL_APOLLOLAKE_HDA),
    intel(PCI_PRODUCT_INTEL_GLK_HDA),
    intel(PCI_PRODUCT_INTEL_JSL_HDA),
    intel(PCI_PRODUCT_INTEL_EHL_HDA),
    intel(PCI_PRODUCT_INTEL_ADL_N_HDA),
    intel(PCI_PRODUCT_INTEL_MTL_HDA),
    intel(PCI_PRODUCT_INTEL_LNL_HDA),
    intel(PCI_PRODUCT_INTEL_ARL_U_HDA),
    intel(PCI_PRODUCT_INTEL_PTL_HDA),
    intel(PCI_PRODUCT_INTEL_PTL_H_HDA),
];

// The header's function-like macros.

/// `HDA_GCAP_OSS(x)`: output streams supported.
pub const fn hda_gcap_oss(x: u16) -> u16 {
    (x & 0xf000) >> 12
}

/// `HDA_GCAP_ISS(x)`: input streams supported.
pub const fn hda_gcap_iss(x: u16) -> u16 {
    (x & 0x0f00) >> 8
}

/// `HDA_GCAP_BSS(x)`: bidirectional streams supported.
pub const fn hda_gcap_bss(x: u16) -> u16 {
    (x & 0x00f8) >> 3
}

/// `COP_VID_VENDOR(x)`.
pub const fn cop_vid_vendor(x: u32) -> u32 {
    x >> 16
}

/// `COP_VID_DEVICE(x)`.
pub const fn cop_vid_device(x: u32) -> u32 {
    x & 0xffff
}

/// `COP_RID_MAJ(x)`.
pub const fn cop_rid_maj(x: u32) -> u32 {
    (x >> 20) & 0x0f
}

/// `COP_RID_MIN(x)`.
pub const fn cop_rid_min(x: u32) -> u32 {
    (x >> 16) & 0x0f
}

/// `COP_RID_REVISION(x)`.
pub const fn cop_rid_revision(x: u32) -> u32 {
    (x >> 8) & 0xff
}

/// `COP_RID_STEPPING(x)`.
pub const fn cop_rid_stepping(x: u32) -> u32 {
    x & 0xff
}

/// `COP_START_NID(x)`.
pub const fn cop_start_nid(x: u32) -> u32 {
    (x & 0x00ff0000) >> 16
}

/// `COP_NSUBNODES(x)`.
pub const fn cop_nsubnodes(x: u32) -> u32 {
    x & 0x000000ff
}

/// `COP_FTYPE(x)`.
pub const fn cop_ftype(x: u32) -> u32 {
    x & 0x000000ff
}

/// `COP_AWCAP_TYPE(x)`: the widget type, `COP_AWTYPE_*`.
pub const fn cop_awcap_type(x: u32) -> u32 {
    (x >> 20) & 0xf
}

/// `COP_AWCAP_DELAY(x)`.
pub const fn cop_awcap_delay(x: u32) -> u32 {
    (x >> 16) & 0xf
}

/// `COP_PINCAP_VREF(x)`.
pub const fn cop_pincap_vref(x: u32) -> u32 {
    (x >> 8) & 0xff
}

/// `COP_AMPCAP_OFFSET(x)`.
pub const fn cop_ampcap_offset(x: u32) -> u32 {
    x & 0x0000007f
}

/// `COP_AMPCAP_NUMSTEPS(x)`.
pub const fn cop_ampcap_numsteps(x: u32) -> u32 {
    (x >> 8) & 0x7f
}

/// `COP_AMPCAP_STEPSIZE(x)`.
pub const fn cop_ampcap_stepsize(x: u32) -> u32 {
    (x >> 16) & 0x7f
}

/// `COP_AMPCAP_CTLOFF(x)`.
pub const fn cop_ampcap_ctloff(x: u32) -> u32 {
    (x >> 24) & 0x7f
}

/// `COP_CLL_LENGTH(x)`.
pub const fn cop_cll_length(x: u32) -> u32 {
    x & 0x0000007f
}

/// `COP_GPIO_GPIOS(x)`.
pub const fn cop_gpio_gpios(x: u32) -> u32 {
    x & 0xff
}

/// `COP_GPIO_GPOS(x)`.
pub const fn cop_gpio_gpos(x: u32) -> u32 {
    (x >> 8) & 0xff
}

/// `COP_GPIO_GPIS(x)`.
pub const fn cop_gpio_gpis(x: u32) -> u32 {
    (x >> 16) & 0xff
}

/// `COP_VKCAP_NUMSTEPS(x)`.
pub const fn cop_vkcap_numsteps(x: u32) -> u32 {
    x & 0x7f
}

/// `CORB_CSC_INDEX(x)`.
pub const fn corb_csc_index(x: u32) -> u32 {
    x & 0xff
}

/// `CORB_GAGM_GAIN(x)`.
pub const fn corb_gagm_gain(x: u32) -> u32 {
    x & 0x0000007f
}

/// `CORB_DCC_CC(x)`.
pub const fn corb_dcc_cc(x: u32) -> u32 {
    (x >> 8) & 0x7f
}

/// `CORB_UNSOL_TAG(x)`.
pub const fn corb_unsol_tag(x: u32) -> u32 {
    x & 0x3f
}

/// `CORB_PS_IMPEDANCE(x)`.
pub const fn corb_ps_impedance(x: u32) -> u32 {
    x & 0x7fffffff
}

/// `CORB_VKNOB_VOLUME(x)`.
pub const fn corb_vknob_volume(x: u32) -> u32 {
    x & 0x7f
}

/// `CORB_CD_SEQUENCE(x)`.
pub const fn corb_cd_sequence(x: u32) -> u32 {
    x & 0x0000000f
}

/// `CORB_CD_ASSOCIATION(x)`.
pub const fn corb_cd_association(x: u32) -> u32 {
    (x >> 4) & 0xf
}

/// `CORB_CD_MISC(x)`.
pub const fn corb_cd_misc(x: u32) -> u32 {
    (x >> 8) & 0xf
}

/// `CORB_CD_COLOR(x)`.
pub const fn corb_cd_color(x: u32) -> u32 {
    (x >> 12) & 0xf
}

/// `CORB_CD_CONNECTION(x)`.
pub const fn corb_cd_connection(x: u32) -> u32 {
    (x >> CORB_CD_CONNECTION_OFFSET) & CORB_CD_CONNECTION_BITS
}

/// `CORB_CD_DEVICE(x)`.
pub const fn corb_cd_device(x: u32) -> u32 {
    (x >> CORB_CD_DEVICE_OFFSET) & CORB_CD_DEVICE_BITS
}

/// `CORB_CD_LOC_GEO(x)`.
pub const fn corb_cd_loc_geo(x: u32) -> u32 {
    (x >> 24) & 0xf
}

/// `CORB_CD_LOC_CHASS(x)`.
pub const fn corb_cd_loc_chass(x: u32) -> u32 {
    (x >> 28) & 0x3
}

/// `CORB_CD_PORT(x)`.
pub const fn corb_cd_port(x: u32) -> u32 {
    (x >> CORB_CD_PORT_OFFSET) & CORB_CD_PORT_BITS
}

/// `RIRB_UNSOL_TAG(resp)`.
pub const fn rirb_unsol_tag(resp: u32) -> u32 {
    resp >> 26
}

/// `RIRB_RESP_CODEC(ex)`.
pub const fn rirb_resp_codec(ex: u32) -> u32 {
    ex & 0xf
}

/// `PTR_UPPER32(x)`: the upper 32 bits of a 64-bit DMA address.
pub const fn ptr_upper32(x: u64) -> u32 {
    (x >> 32) as u32
}

/// `WIDGET_CHANNELS(w)`.
pub const fn widget_channels(w: &Widget) -> i32 {
    if w.widgetcap & COP_AWCAP_STEREO != 0 {
        2
    } else {
        1
    }
}

/// `IS_MI_TARGET_INAMP(x)`.
pub const fn is_mi_target_inamp(x: i32) -> bool {
    x <= 15
}

/// `MI_TARGET_INAMP(x)`.
pub const fn mi_target_inamp(x: i32) -> i32 {
    x
}

/// `VALID_WIDGET_NID(nid, codec)`.
pub fn valid_widget_nid(nid: NidT, codec: &Codec) -> bool {
    nid == codec.audiofunc || (nid >= codec.wstart && nid < codec.wend)
}

// ================================================================
// PCI functions
// ================================================================

/// `azalia_pci_read`: one byte of configuration space.
pub fn azalia_pci_read(pc: PciChipsetTag, pa: Pcitag, reg: i32) -> u8 {
    ((pci_conf_read(pc, pa, reg & !0x03) >> ((reg & 0x03) * 8)) & 0xff) as u8
}

/// `azalia_pci_write`: one byte of configuration space.
pub fn azalia_pci_write(pc: PciChipsetTag, pa: Pcitag, reg: i32, val: u8) {
    let shift = (reg & 0x03) * 8;
    let mut pcival = pci_conf_read(pc, pa, reg & !0x03);
    pcival &= !(0xff << shift);
    pcival |= u32::from(val) << shift;
    pci_conf_write(pc, pa, reg & !0x03, pcival);
}

/// `azalia_configure_pci`: back-to-back, traffic class 0 and the chipsets' snoop bits.
pub fn azalia_configure_pci(az: &AzaliaSoftc) {
    let pc = az.pc();
    let tag = az.tag.get();

    // enable back-to-back
    let v = pci_conf_read(pc, tag, PCI_COMMAND_STATUS_REG);
    pci_conf_write(
        pc,
        tag,
        PCI_COMMAND_STATUS_REG,
        v | PCI_COMMAND_BACKTOBACK_ENABLE,
    );

    // traffic class select
    let v = pci_conf_read(pc, tag, ICH_PCI_HDTCSEL);
    pci_conf_write(pc, tag, ICH_PCI_HDTCSEL, v & !ICH_PCI_HDTCSEL_MASK);

    // enable PCIe snoop
    match pci_product(az.pciid.get()) {
        PCI_PRODUCT_ATI_SB450_HDA
        | PCI_PRODUCT_ATI_SBX00_HDA
        | PCI_PRODUCT_AMD_15_6X_AUDIO
        | PCI_PRODUCT_AMD_17_HDA
        | PCI_PRODUCT_AMD_17_1X_HDA
        | PCI_PRODUCT_AMD_17_3X_HDA
        | PCI_PRODUCT_AMD_HUDSON2_HDA => {
            let mut reg = azalia_pci_read(pc, tag, ATI_PCIE_SNOOP_REG);
            reg &= ATI_PCIE_SNOOP_MASK;
            reg |= ATI_PCIE_SNOOP_ENABLE;
            azalia_pci_write(pc, tag, ATI_PCIE_SNOOP_REG, reg);
        }
        PCI_PRODUCT_NVIDIA_MCP51_HDA
        | PCI_PRODUCT_NVIDIA_MCP55_HDA
        | PCI_PRODUCT_NVIDIA_MCP61_HDA_1
        | PCI_PRODUCT_NVIDIA_MCP61_HDA_2
        | PCI_PRODUCT_NVIDIA_MCP65_HDA_1
        | PCI_PRODUCT_NVIDIA_MCP65_HDA_2
        | PCI_PRODUCT_NVIDIA_MCP67_HDA_1
        | PCI_PRODUCT_NVIDIA_MCP67_HDA_2
        | PCI_PRODUCT_NVIDIA_MCP73_HDA_1
        | PCI_PRODUCT_NVIDIA_MCP73_HDA_2
        | PCI_PRODUCT_NVIDIA_MCP77_HDA_1
        | PCI_PRODUCT_NVIDIA_MCP77_HDA_2
        | PCI_PRODUCT_NVIDIA_MCP77_HDA_3
        | PCI_PRODUCT_NVIDIA_MCP77_HDA_4
        | PCI_PRODUCT_NVIDIA_MCP79_HDA_1
        | PCI_PRODUCT_NVIDIA_MCP79_HDA_2
        | PCI_PRODUCT_NVIDIA_MCP79_HDA_3
        | PCI_PRODUCT_NVIDIA_MCP79_HDA_4
        | PCI_PRODUCT_NVIDIA_MCP89_HDA_1
        | PCI_PRODUCT_NVIDIA_MCP89_HDA_2
        | PCI_PRODUCT_NVIDIA_MCP89_HDA_3
        | PCI_PRODUCT_NVIDIA_MCP89_HDA_4 => {
            let mut reg = azalia_pci_read(pc, tag, NVIDIA_HDA_OSTR_COH_REG);
            reg |= NVIDIA_HDA_STR_COH_ENABLE;
            azalia_pci_write(pc, tag, NVIDIA_HDA_OSTR_COH_REG, reg);

            let mut reg = azalia_pci_read(pc, tag, NVIDIA_HDA_ISTR_COH_REG);
            reg |= NVIDIA_HDA_STR_COH_ENABLE;
            azalia_pci_write(pc, tag, NVIDIA_HDA_ISTR_COH_REG, reg);

            let mut reg = azalia_pci_read(pc, tag, NVIDIA_PCIE_SNOOP_REG);
            reg &= NVIDIA_PCIE_SNOOP_MASK;
            reg |= NVIDIA_PCIE_SNOOP_ENABLE;
            azalia_pci_write(pc, tag, NVIDIA_PCIE_SNOOP_REG, reg);

            let reg = azalia_pci_read(pc, tag, NVIDIA_PCIE_SNOOP_REG);
            if reg & NVIDIA_PCIE_SNOOP_ENABLE != NVIDIA_PCIE_SNOOP_ENABLE {
                printf(format_args!(": could not enable PCIe cache snooping!\n"));
            }
        }
        PCI_PRODUCT_INTEL_82801FB_HDA
        | PCI_PRODUCT_INTEL_82801GB_HDA
        | PCI_PRODUCT_INTEL_82801H_HDA
        | PCI_PRODUCT_INTEL_82801I_HDA
        | PCI_PRODUCT_INTEL_82801JI_HDA
        | PCI_PRODUCT_INTEL_82801JD_HDA
        | PCI_PRODUCT_INTEL_6321ESB_HDA
        | PCI_PRODUCT_INTEL_3400_HDA
        | PCI_PRODUCT_INTEL_QS57_HDA
        | PCI_PRODUCT_INTEL_6SERIES_HDA
        | PCI_PRODUCT_INTEL_7SERIES_HDA
        | PCI_PRODUCT_INTEL_8SERIES_HDA
        | PCI_PRODUCT_INTEL_8SERIES_LP_HDA
        | PCI_PRODUCT_INTEL_9SERIES_HDA
        | PCI_PRODUCT_INTEL_9SERIES_LP_HDA
        | PCI_PRODUCT_INTEL_100SERIES_HDA
        | PCI_PRODUCT_INTEL_100SERIES_H_HDA
        | PCI_PRODUCT_INTEL_100SERIES_LP_HDA
        | PCI_PRODUCT_INTEL_200SERIES_HDA
        | PCI_PRODUCT_INTEL_200SERIES_U_HDA
        | PCI_PRODUCT_INTEL_300SERIES_CAVS
        | PCI_PRODUCT_INTEL_300SERIES_U_HDA
        | PCI_PRODUCT_INTEL_400SERIES_CAVS
        | PCI_PRODUCT_INTEL_400SERIES_LP_HDA
        | PCI_PRODUCT_INTEL_495SERIES_LP_HDA
        | PCI_PRODUCT_INTEL_500SERIES_HDA
        | PCI_PRODUCT_INTEL_500SERIES_HDA_2
        | PCI_PRODUCT_INTEL_500SERIES_LP_HDA
        | PCI_PRODUCT_INTEL_600SERIES_HDA
        | PCI_PRODUCT_INTEL_600SERIES_LP_HDA
        | PCI_PRODUCT_INTEL_700SERIES_HDA
        | PCI_PRODUCT_INTEL_700SERIES_LP_HDA
        | PCI_PRODUCT_INTEL_800SERIES_HDA
        | PCI_PRODUCT_INTEL_C600_HDA
        | PCI_PRODUCT_INTEL_C610_HDA_1
        | PCI_PRODUCT_INTEL_C610_HDA_2
        | PCI_PRODUCT_INTEL_C620_HDA_1
        | PCI_PRODUCT_INTEL_C620_HDA_2
        | PCI_PRODUCT_INTEL_APOLLOLAKE_HDA
        | PCI_PRODUCT_INTEL_BAYTRAIL_HDA
        | PCI_PRODUCT_INTEL_BSW_HDA
        | PCI_PRODUCT_INTEL_GLK_HDA
        | PCI_PRODUCT_INTEL_JSL_HDA
        | PCI_PRODUCT_INTEL_EHL_HDA
        | PCI_PRODUCT_INTEL_ADL_N_HDA
        | PCI_PRODUCT_INTEL_MTL_HDA
        | PCI_PRODUCT_INTEL_LNL_HDA
        | PCI_PRODUCT_INTEL_ARL_U_HDA
        | PCI_PRODUCT_INTEL_PTL_HDA
        | PCI_PRODUCT_INTEL_PTL_H_HDA => {
            let mut reg = azalia_pci_read(pc, tag, INTEL_PCIE_NOSNOOP_REG);
            reg &= INTEL_PCIE_NOSNOOP_MASK;
            azalia_pci_write(pc, tag, INTEL_PCIE_NOSNOOP_REG, reg);
        }
        _ => {}
    }
}

/// `{ PCI_VENDOR_INTEL, product }` of `azalia_pci_devices`.
const fn intel(product: u32) -> PciMatchid {
    PciMatchid {
        pm_vid: PCI_VENDOR_INTEL as PciVendorId,
        pm_pid: product as PciProductId,
    }
}

/// `azalia_pci_match`: every HD Audio function, and the Intel controllers that do not say
/// they are one.
pub fn azalia_pci_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: pci hands its children pci_attach_args.
    let pa = unsafe { &*aux.cast::<PciAttachArgs>() };
    if pci_class(pa.pa_class) == PCI_CLASS_MULTIMEDIA
        && pci_subclass(pa.pa_class) == PCI_SUBCLASS_MULTIMEDIA_HDAUDIO
    {
        return 1;
    }
    pci_matchbyid(pa, &AZALIA_PCI_DEVICES)
}

/// `azalia_pci_attach`.
pub fn azalia_pci_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `self_` was made for `AZALIA_CA`, whose softc is an `AzaliaSoftc`.
    let sc = unsafe { self_.softc::<AzaliaSoftc>() };
    // SAFETY: pci hands its children pci_attach_args, alive for the whole attach.
    let pa = unsafe { &*aux.cast::<PciAttachArgs>() };

    sc.dmat.set(Some(pa.pa_dmat));

    pci_set_powerstate(pa.pa_pc, pa.pa_tag, PCI_PMCSR_STATE_D0 as i32);

    let mut v = pci_conf_read(pa.pa_pc, pa.pa_tag, ICH_PCI_HDBARL);
    v &= PCI_MAPREG_TYPE_MASK | PCI_MAPREG_MEM_TYPE_MASK;
    match pci_mapreg_map(pa, ICH_PCI_HDBARL, v, 0, 0) {
        Ok((iot, ioh, _, size)) => {
            sc.iot.set(Some(iot));
            sc.ioh.set(Some(ioh));
            sc.map_size.set(size);
        }
        Err(_) => {
            printf(format_args!(": can't map device i/o space\n"));
            return;
        }
    }

    sc.pc.set(Some(pa.pa_pc));
    sc.tag.set(pa.pa_tag);
    sc.pciid.set(pa.pa_id);
    sc.subid
        .set(pci_conf_read(pa.pa_pc, pa.pa_tag, PCI_SUBSYS_ID_REG));

    azalia_configure_pci(sc);

    // disable MSI, use INTx instead
    if pci_vendor(sc.pciid.get()) == PCI_VENDOR_INTEL {
        let mut reg = azalia_pci_read(sc.pc(), sc.tag.get(), ICH_PCI_MMC);
        reg &= !ICH_PCI_MMC_ME;
        azalia_pci_write(sc.pc(), sc.tag.get(), ICH_PCI_MMC, reg);
    }

    // interrupt
    let Some(ih) = pci_intr_map_msi(pa).or_else(|| pci_intr_map(pa)) else {
        printf(format_args!(": can't map interrupt\n"));
        return;
    };
    let interrupt_str = pci_intr_string(pa.pa_pc, ih);
    let cookie = pci_intr_establish(
        pa.pa_pc,
        ih,
        IPL_AUDIO | IPL_MPSAFE,
        azalia_intr,
        ptr::from_ref(sc).cast_mut().cast(),
        sc.intr_name(),
    );
    let Some(cookie) = cookie else {
        printf(format_args!(": can't establish interrupt"));
        printf(format_args!(" at {interrupt_str}"));
        printf(format_args!("\n"));
        return;
    };
    sc.ih.set(Some(cookie));
    printf(format_args!(": {interrupt_str}\n"));

    'err_exit: {
        if azalia_init(sc, false).is_err() {
            break 'err_exit;
        }

        if azalia_init_codecs(sc).is_err() {
            break 'err_exit;
        }

        if azalia_init_streams(sc).is_err() {
            break 'err_exit;
        }

        audio_attach_mi(
            &AZALIA_HW_IF,
            ptr::from_ref(sc).cast_mut().cast(),
            ptr::null_mut(),
            &sc.dev,
        );

        return;
    }

    // err_exit:
    sc.detached.set(1);
    let _ = azalia_pci_detach(self_, 0);
}

/// `azalia_pci_activate`.
pub fn azalia_pci_activate(self_: &Device, act: i32) -> Result<(), Errno> {
    // SAFETY: `AZALIA_CA` makes `AzaliaSoftc`s.
    let sc = unsafe { self_.softc::<AzaliaSoftc>() };

    if sc.detached.get() != 0 {
        return Ok(());
    }

    match act {
        DVACT_QUIESCE => {
            let rv = config_activate_children(self_, act);
            // stop interrupts and clear status registers
            sc.az_write_4(HDA_INTCTL, 0);
            sc.az_write_2(HDA_STATESTS, HDA_STATESTS_SDIWAKE);
            sc.az_write_1(HDA_RIRBSTS, HDA_RIRBSTS_RINTFL | HDA_RIRBSTS_RIRBOIS);
            let _ = sc.az_read_4(HDA_INTSTS);
            rv
        }
        DVACT_SUSPEND => {
            let rv = config_activate_children(self_, act);
            let _ = azalia_suspend(sc);
            rv
        }
        DVACT_RESUME => {
            let _ = azalia_resume(sc);
            config_activate_children(self_, act)
        }
        DVACT_POWERDOWN => {
            let rv = config_activate_children(self_, act);
            azalia_shutdown(sc);
            rv
        }
        _ => config_activate_children(self_, act),
    }
}

/// `azalia_pci_detach`. Also the cleanup of a failed attach, which sets `detached` to 1
/// first; the function then has nothing to do the second time (`detached` 2).
pub fn azalia_pci_detach(self_: &Device, flags: i32) -> Result<(), Errno> {
    // SAFETY: `AZALIA_CA` makes `AzaliaSoftc`s.
    let az = unsafe { self_.softc::<AzaliaSoftc>() };

    // this function is called if the device could not be supported, in which case
    // az->detached == 1. check if this function has already cleaned up.
    if az.detached.get() > 1 {
        return Ok(());
    }

    let _ = config_detach_children(self_, flags);

    // disable unsolicited responses if soft detaching
    if az.detached.get() == 1 {
        let gctl = az.az_read_4(HDA_GCTL);
        az.az_write_4(HDA_GCTL, gctl & !HDA_GCTL_UNSOL);
    }

    timeout_del(&az.unsol_to);

    // delete streams
    if !az.rstream.bdlist.get().addr.is_null() {
        azalia_free_dmamem(az, &az.rstream.bdlist);
    }
    if !az.pstream.bdlist.get().addr.is_null() {
        azalia_free_dmamem(az, &az.pstream.bdlist);
    }

    // delete codecs
    // SAFETY: detach runs under the kernel lock and nothing else holds a codec.
    for codec in unsafe { az.codecs() }.iter_mut() {
        let _ = azalia_codec_delete(codec);
    }
    let ncodecs = az.ncodecs.replace(0);
    if let Some(codecs) = NonNull::new(az.codecs.replace(ptr::null_mut())) {
        let n = usize::try_from(ncodecs).unwrap_or(0);
        // SAFETY: `azalia_get_ctrlr_caps` initialised `ncodecs` codecs in this allocation;
        // nothing refers to them any more (the array pointer was cleared above).
        unsafe {
            ptr::drop_in_place(ptr::slice_from_raw_parts_mut(codecs.as_ptr(), n));
        }
        free(codecs.cast(), M_DEVBUF, n * size_of::<Codec>());
    }

    // delete CORB and RIRB
    if !az.corb_dma.get().addr.is_null() {
        azalia_free_dmamem(az, &az.corb_dma);
    }
    if !az.rirb_dma.get().addr.is_null() {
        azalia_free_dmamem(az, &az.rirb_dma);
    }
    if let Some(unsolq) = NonNull::new(az.unsolq.replace(ptr::null_mut())) {
        free(
            unsolq.cast(),
            M_DEVBUF,
            size_of::<RirbEntry>() * UNSOLQ_SIZE as usize,
        );
    }

    // disable interrupts if soft detaching
    if az.detached.get() == 1 {
        az.az_write_4(HDA_INTCTL, 0);

        // clear interrupts
        az.az_write_2(HDA_STATESTS, HDA_STATESTS_SDIWAKE);
        az.az_write_1(HDA_RIRBSTS, HDA_RIRBSTS_RINTFL | HDA_RIRBSTS_RIRBOIS);
    }

    // delete PCI resources
    if let Some(ih) = az.ih.take() {
        // SAFETY: the cookie `pci_intr_establish` returned at attach, dropped here.
        unsafe { pci_intr_disestablish(az.pc(), ih) };
    }
    if az.map_size.get() != 0 {
        let (iot, ioh) = az.regs();
        bus_space_unmap(iot, ioh, az.map_size.get());
        az.map_size.set(0);
    }

    az.detached.set(2);
    Ok(())
}

/// `azalia_intr`: established `IPL_AUDIO | IPL_MPSAFE`; runs under `audio_lock`.
pub fn azalia_intr(v: *mut c_void) -> i32 {
    // SAFETY: established with the softc as its argument; softcs outlive their interrupts.
    let az = unsafe { &*v.cast::<AzaliaSoftc>() };
    let mut ret = 0;

    mtx_enter(&AUDIO_LOCK);
    loop {
        let intsts = az.az_read_4(HDA_INTSTS);
        if intsts & az.intctl.load(Ordering::Relaxed) == 0 || intsts == 0xffffffff {
            break;
        }

        if intsts & az.pstream.intr_bit.get() != 0 {
            azalia_stream_intr(&az.pstream);
            ret = 1;
        }

        if intsts & az.rstream.intr_bit.get() != 0 {
            azalia_stream_intr(&az.rstream);
            ret = 1;
        }

        if intsts & HDA_INTSTS_CIS != 0
            && az.az_read_1(HDA_RIRBCTL) & HDA_RIRBCTL_RINTCTL != 0
            && az.az_read_1(HDA_RIRBSTS) & HDA_RIRBSTS_RINTFL != 0
        {
            azalia_rirb_intr(az);
            ret = 1;
        }
    }
    mtx_leave(&AUDIO_LOCK);
    ret
}

/// `azalia_shutdown`: unsolicited responses off, CORB and RIRB halted.
pub fn azalia_shutdown(az: &AzaliaSoftc) {
    if az.detached.get() != 0 {
        return;
    }

    // disable unsolicited response
    let gctl = az.az_read_4(HDA_GCTL);
    az.az_write_4(HDA_GCTL, gctl & !HDA_GCTL_UNSOL);

    timeout_del(&az.unsol_to);

    // halt CORB/RIRB
    let _ = azalia_halt_corb(az);
    let _ = azalia_halt_rirb(az);
}

// ================================================================
// HDA controller functions
// ================================================================

/// `azalia_print_codec`.
pub fn azalia_print_codec(codec: &Codec) {
    match codec.name {
        None => match pci_findvendor(codec.vid >> 16) {
            None => printf(format_args!(
                "0x{:04x}/0x{:04x}",
                codec.vid >> 16,
                codec.vid & 0xffff
            )),
            Some(vendor) => printf(format_args!("{}/0x{:04x}", Str(vendor), codec.vid & 0xffff)),
        },
        Some(name) => printf(format_args!("{name}")),
    };
}

/// The C's `for (i = 5000; i > 0; i--) { DELAY(10); if (done) break; }` polls: true when
/// `done` held before the count ran out (the C's `i != 0`). A port helper.
fn azalia_poll(mut done: impl FnMut() -> bool) -> bool {
    for _ in 0..5000 {
        delay(10);
        if done() {
            return true;
        }
    }
    false
}

/// `azalia_reset`: 4.2.2 Starting the High Definition Audio Controller.
pub fn azalia_reset(az: &AzaliaSoftc) -> Result<(), Errno> {
    let gctl = az.az_read_4(HDA_GCTL);
    az.az_write_4(HDA_GCTL, gctl & !HDA_GCTL_CRST);
    if !azalia_poll(|| az.az_read_4(HDA_GCTL) & HDA_GCTL_CRST == 0) {
        // reset failure
        return Err(Errno::ETIMEDOUT);
    }
    delay(1000);
    let gctl = az.az_read_4(HDA_GCTL);
    az.az_write_4(HDA_GCTL, gctl | HDA_GCTL_CRST);
    if !azalia_poll(|| az.az_read_4(HDA_GCTL) & HDA_GCTL_CRST != 0) {
        // reset-exit failure
        return Err(Errno::ETIMEDOUT);
    }
    delay(1000);

    Ok(())
}

/// `azalia_get_ctrlr_caps`: the streams, the codecs present and the CORB/RIRB sizes.
pub fn azalia_get_ctrlr_caps(az: &AzaliaSoftc) -> Result<(), Errno> {
    let xname = az.dev.xname();

    let gcap = az.az_read_2(HDA_GCAP);
    az.nistreams.set(i32::from(hda_gcap_iss(gcap)));
    az.nostreams.set(i32::from(hda_gcap_oss(gcap)));
    az.nbstreams.set(i32::from(hda_gcap_bss(gcap)));
    az.ok64.set(i32::from(gcap & HDA_GCAP_64OK != 0));

    // 4.3 Codec discovery
    let statests = az.az_read_2(HDA_STATESTS);
    let n = (0..HDA_MAX_CODECS)
        .filter(|&i| (statests >> i) & 1 != 0)
        .count();
    az.ncodecs.set(n as i32);
    if n < 1 {
        printf(format_args!("{xname}: no HD-Audio codecs\n"));
        return Err(Errno::EIO);
    }
    let Some(mem) = mallocarray(n, size_of::<Codec>(), M_DEVBUF, M_NOWAIT | M_ZERO) else {
        printf(format_args!("{xname}: can't allocate memory for codecs\n"));
        return Err(Errno::ENOMEM);
    };
    let codecs = mem.as_ptr().cast::<Codec>();
    let mut k = 0;
    for i in 0..HDA_MAX_CODECS {
        if k == n {
            break;
        }
        if (statests >> i) & 1 != 0 {
            let mut codec = Codec::new(az);
            codec.address = i as i32;
            // SAFETY: `k < n` slots of `Codec` size were just allocated (malloc's alignment
            // suits any kernel type), each written once here.
            unsafe { codecs.add(k).write(codec) };
            k += 1;
        }
    }
    az.codecs.set(codecs);

    // determine CORB size
    let mut corbsize = az.az_read_1(HDA_CORBSIZE);
    let cap = corbsize & HDA_CORBSIZE_CORBSZCAP_MASK;
    corbsize &= !HDA_CORBSIZE_CORBSIZE_MASK;
    if cap & HDA_CORBSIZE_CORBSZCAP_256 != 0 {
        az.corb_entries.set(256);
        corbsize |= HDA_CORBSIZE_CORBSIZE_256;
    } else if cap & HDA_CORBSIZE_CORBSZCAP_16 != 0 {
        az.corb_entries.set(16);
        corbsize |= HDA_CORBSIZE_CORBSIZE_16;
    } else if cap & HDA_CORBSIZE_CORBSZCAP_2 != 0 {
        az.corb_entries.set(2);
        corbsize |= HDA_CORBSIZE_CORBSIZE_2;
    } else {
        printf(format_args!("{xname}: invalid CORBSZCAP: 0x{cap:2x}\n"));
        return Err(Errno::EIO);
    }
    az.corbsize.set(corbsize);

    // determine RIRB size
    let mut rirbsize = az.az_read_1(HDA_RIRBSIZE);
    let cap = rirbsize & HDA_RIRBSIZE_RIRBSZCAP_MASK;
    rirbsize &= !HDA_RIRBSIZE_RIRBSIZE_MASK;
    if cap & HDA_RIRBSIZE_RIRBSZCAP_256 != 0 {
        az.rirb_entries.set(256);
        rirbsize |= HDA_RIRBSIZE_RIRBSIZE_256;
    } else if cap & HDA_RIRBSIZE_RIRBSZCAP_16 != 0 {
        az.rirb_entries.set(16);
        rirbsize |= HDA_RIRBSIZE_RIRBSIZE_16;
    } else if cap & HDA_RIRBSIZE_RIRBSZCAP_2 != 0 {
        az.rirb_entries.set(2);
        rirbsize |= HDA_RIRBSIZE_RIRBSIZE_2;
    } else {
        printf(format_args!("{xname}: invalid RIRBSZCAP: 0x{cap:2x}\n"));
        return Err(Errno::EIO);
    }
    az.rirbsize.set(rirbsize);

    Ok(())
}

/// `azalia_init`: reset the controller and start the CORB and the RIRB.
pub fn azalia_init(az: &AzaliaSoftc, resuming: bool) -> Result<(), Errno> {
    azalia_reset(az)?;

    if !resuming {
        azalia_get_ctrlr_caps(az)?;
    }

    // clear interrupt status
    az.az_write_2(HDA_STATESTS, HDA_STATESTS_SDIWAKE);
    az.az_write_1(HDA_RIRBSTS, HDA_RIRBSTS_RINTFL | HDA_RIRBSTS_RIRBOIS);
    az.az_write_4(HDA_DPLBASE, 0);
    az.az_write_4(HDA_DPUBASE, 0);

    // 4.4.1 Command Outbound Ring Buffer
    azalia_init_corb(az, resuming)?;

    // 4.4.2 Response Inbound Ring Buffer
    azalia_init_rirb(az, resuming)?;

    let intctl = HDA_INTCTL_CIE | HDA_INTCTL_GIE;
    az.intctl.store(intctl, Ordering::Relaxed);
    az.az_write_4(HDA_INTCTL, intctl);

    Ok(())
}

/// `azalia_init_codecs`: initialise every codec and pick the one to use.
pub fn azalia_init_codecs(az: &AzaliaSoftc) -> Result<(), Errno> {
    let xname = az.dev.xname();
    // SAFETY: attach, under the kernel lock; nothing else holds a codec.
    let codecs = unsafe { az.codecs() };
    let ncodecs = codecs.len();

    let mut c = 0;
    for codec in codecs.iter_mut() {
        if azalia_codec_init(codec).is_ok() {
            c += 1;
        }
    }
    if c == 0 {
        printf(format_args!("{xname}: No codecs found\n"));
        return Err(Errno::EIO);
    }

    // Use the first codec capable of analog I/O. If there are none, use the first codec
    // capable of digital I/O. Skip HDMI codecs.
    let mut c: i32 = -1;
    for (i, codec) in codecs.iter().enumerate() {
        if codec.audiofunc < 0 || codec.codec_type == AZ_CODEC_TYPE_HDMI {
            continue;
        }
        if codec.codec_type == AZ_CODEC_TYPE_DIGITAL {
            if c < 0 {
                c = i as i32;
            }
        } else {
            c = i as i32;
            break;
        }
    }
    az.codecno.set(c);
    if c < 0 {
        printf(format_args!("{xname}: no supported codecs\n"));
        return Err(Errno::EIO);
    }
    let codecno = c as usize;

    printf(format_args!("{xname}: codecs: "));
    for (i, codec) in codecs.iter().enumerate() {
        azalia_print_codec(codec);
        if i + 1 < ncodecs {
            printf(format_args!(", "));
        }
    }
    if ncodecs > 1 {
        printf(format_args!(", using "));
        azalia_print_codec(&codecs[codecno]);
    }
    printf(format_args!("\n"));

    // All codecs with audio are enabled, but only one will be used.
    for (i, codec) in codecs.iter_mut().enumerate() {
        if i != codecno {
            if codec.audiofunc < 0 {
                continue;
            }
            let _ = azalia_comresp(codec, codec.audiofunc, CORB_SET_POWER_STATE, CORB_PS_D3);
            delay(100);
            let _ = azalia_codec_delete(codec);
        }
    }

    // Enable unsolicited responses now that az->codecno is set.
    az.az_write_4(HDA_GCTL, az.az_read_4(HDA_GCTL) | HDA_GCTL_UNSOL);

    Ok(())
}

/// `azalia_init_streams`: use stream#1 and #2. Don't use stream#0.
pub fn azalia_init_streams(az: &AzaliaSoftc) -> Result<(), Errno> {
    azalia_stream_init(&az.pstream, az, az.nistreams.get(), 1, AUMODE_PLAY)?;
    azalia_stream_init(&az.rstream, az, 0, 2, AUMODE_RECORD)?;
    Ok(())
}

/// `azalia_halt_corb`: power the codecs off and stop the CORB, if it runs.
pub fn azalia_halt_corb(az: &AzaliaSoftc) -> Result<(), Errno> {
    let corbctl = az.az_read_1(HDA_CORBCTL);
    if corbctl & HDA_CORBCTL_CORBRUN != 0 {
        // running? power off all codecs
        // SAFETY: attach, suspend and shutdown run under the kernel lock, no codec held.
        for codec in unsafe { az.codecs() }.iter() {
            if codec.audiofunc < 0 {
                continue;
            }
            let _ = azalia_comresp(codec, codec.audiofunc, CORB_SET_POWER_STATE, CORB_PS_D3);
        }

        az.az_write_1(HDA_CORBCTL, corbctl & !HDA_CORBCTL_CORBRUN);
        if !azalia_poll(|| az.az_read_1(HDA_CORBCTL) & HDA_CORBCTL_CORBRUN == 0) {
            // CORB is running
            return Err(Errno::EBUSY);
        }
    }
    Ok(())
}

/// `azalia_init_corb`: 4.4.1 Command Outbound Ring Buffer.
pub fn azalia_init_corb(az: &AzaliaSoftc, resuming: bool) -> Result<(), Errno> {
    azalia_halt_corb(az)?;

    if !resuming {
        let size = az.corb_entries.get() as usize * size_of::<CorbEntry>();
        if let Err(err) = azalia_alloc_dmamem(az, size, 128, &az.corb_dma) {
            printf(format_args!(
                "{}: can't allocate CORB buffer\n",
                az.dev.xname()
            ));
            return Err(err);
        }
    }
    timeout_set(
        &az.unsol_to,
        azalia_rirb_kick_unsol_events,
        ptr::from_ref(az).cast_mut().cast(),
    );

    let dmaaddr = az.corb_dma.get().dmaaddr() as u64;
    az.az_write_4(HDA_CORBLBASE, dmaaddr as u32);
    az.az_write_4(HDA_CORBUBASE, ptr_upper32(dmaaddr));
    az.az_write_1(HDA_CORBSIZE, az.corbsize.get());

    // reset CORBRP
    let corbrp = az.az_read_2(HDA_CORBRP);
    az.az_write_2(HDA_CORBRP, corbrp | HDA_CORBRP_CORBRPRST);
    az.az_write_2(HDA_CORBRP, corbrp & !HDA_CORBRP_CORBRPRST);
    if !azalia_poll(|| az.az_read_2(HDA_CORBRP) & HDA_CORBRP_CORBRPRST == 0) {
        // CORBRP reset failure
        return Err(Errno::EIO);
    }

    // clear CORBWP
    let corbwp = az.az_read_2(HDA_CORBWP);
    az.az_write_2(HDA_CORBWP, corbwp & !HDA_CORBWP_CORBWP);

    // Run!
    let corbctl = az.az_read_1(HDA_CORBCTL);
    az.az_write_1(HDA_CORBCTL, corbctl | HDA_CORBCTL_CORBRUN);
    Ok(())
}

/// `azalia_halt_rirb`: stop the RIRB's DMA, if it runs.
pub fn azalia_halt_rirb(az: &AzaliaSoftc) -> Result<(), Errno> {
    let rirbctl = az.az_read_1(HDA_RIRBCTL);
    if rirbctl & HDA_RIRBCTL_RIRBDMAEN != 0 {
        // running?
        az.az_write_1(HDA_RIRBCTL, rirbctl & !HDA_RIRBCTL_RIRBDMAEN);
        if !azalia_poll(|| az.az_read_1(HDA_RIRBCTL) & HDA_RIRBCTL_RIRBDMAEN == 0) {
            // RIRB is running
            return Err(Errno::EBUSY);
        }
    }
    Ok(())
}

/// `azalia_init_rirb`: 4.4.2 Response Inbound Ring Buffer, and the queue of unsolicited
/// responses.
pub fn azalia_init_rirb(az: &AzaliaSoftc, resuming: bool) -> Result<(), Errno> {
    azalia_halt_rirb(az)?;

    if !resuming {
        let size = az.rirb_entries.get() as usize * size_of::<RirbEntry>();
        if let Err(err) = azalia_alloc_dmamem(az, size, 128, &az.rirb_dma) {
            printf(format_args!(
                "{}: can't allocate RIRB buffer\n",
                az.dev.xname()
            ));
            return Err(err);
        }

        // setup the unsolicited response queue
        let Some(q) = malloc(
            size_of::<RirbEntry>() * UNSOLQ_SIZE as usize,
            M_DEVBUF,
            M_NOWAIT | M_ZERO,
        ) else {
            azalia_free_dmamem(az, &az.rirb_dma);
            return Err(Errno::ENOMEM);
        };
        az.unsolq.set(q.as_ptr().cast());
    }
    let dmaaddr = az.rirb_dma.get().dmaaddr() as u64;
    az.az_write_4(HDA_RIRBLBASE, dmaaddr as u32);
    az.az_write_4(HDA_RIRBUBASE, ptr_upper32(dmaaddr));
    az.az_write_1(HDA_RIRBSIZE, az.rirbsize.get());

    // reset the write pointer
    let rirbwp = az.az_read_2(HDA_RIRBWP);
    az.az_write_2(HDA_RIRBWP, rirbwp | HDA_RIRBWP_RIRBWPRST);

    // clear the read pointer
    az.rirb_rp
        .set(i32::from(az.az_read_2(HDA_RIRBWP) & HDA_RIRBWP_RIRBWP));

    az.unsolq_rp.set(0);
    az.unsolq_wp.store(0, Ordering::Relaxed);
    az.unsolq_kick.set(0);

    az.az_write_2(HDA_RINTCNT, 1);

    // Run!
    let rirbctl = az.az_read_1(HDA_RIRBCTL);
    az.az_write_1(
        HDA_RIRBCTL,
        rirbctl | HDA_RIRBCTL_RIRBDMAEN | HDA_RIRBCTL_RINTCTL,
    );
    if !azalia_poll(|| az.az_read_1(HDA_RIRBCTL) & HDA_RIRBCTL_RIRBDMAEN != 0) {
        // RIRB is not running
        return Err(Errno::EBUSY);
    }

    Ok(())
}

/// `azalia_comresp`: send `control`/`param` to node `nid` of `codec` and return its
/// response (the C's `*result`), under `audio_lock`.
pub fn azalia_comresp(codec: &Codec, nid: NidT, control: u32, param: u32) -> Result<u32, Errno> {
    let az = codec.az();
    mtx_enter(&AUDIO_LOCK);
    let r = azalia_set_command(az, codec.address, nid, control, param)
        .and_then(|()| azalia_get_response(az));
    mtx_leave(&AUDIO_LOCK);
    r
}

/// `azalia_set_command`: queue one verb on the CORB. Called with `audio_lock` held.
pub fn azalia_set_command(
    az: &AzaliaSoftc,
    caddr: i32,
    nid: NidT,
    control: u32,
    param: u32,
) -> Result<(), Errno> {
    if az.az_read_1(HDA_CORBCTL) & HDA_CORBCTL_CORBRUN == 0 {
        printf(format_args!("{}: CORB is not running.\n", az.dev.xname()));
        return Err(Errno::EIO);
    }
    let verb = ((caddr as u32) << 28) | ((nid as u32) << 20) | (control << 8) | param;
    let corbwp = az.az_read_2(HDA_CORBWP);
    let mut wp = i32::from(corbwp & HDA_CORBWP_CORBWP);
    let corb = az.corb_dma.get().addr.cast::<CorbEntry>();
    wp += 1;
    if wp >= az.corb_entries.get() {
        wp = 0;
    }
    // SAFETY: `azalia_init_corb` allocated `corb_entries` entries at `corb`, and
    // `wp < corb_entries`; the device reads them, hence the volatile store.
    unsafe { corb.add(wp as usize).write_volatile(verb) };

    az.az_write_2(HDA_CORBWP, (corbwp & !HDA_CORBWP_CORBWP) | wp as u16);

    Ok(())
}

/// The C's queueing of an unsolicited response in `azalia_get_response` and
/// `azalia_rirb_intr`, with `audio_lock` held. A port helper.
fn azalia_unsolq_put(az: &AzaliaSoftc, e: RirbEntry) {
    let wp = az.unsolq_wp.load(Ordering::Relaxed);
    // SAFETY: `azalia_init_rirb` allocated `UNSOLQ_SIZE` entries at `unsolq` and
    // `0 <= wp < UNSOLQ_SIZE`; the reader looks at the entry only after the store below.
    unsafe { az.unsolq.get().add(wp as usize).write(e) };
    az.unsolq_wp
        .store((wp + 1) % UNSOLQ_SIZE, Ordering::Release);
}

/// `azalia_rirb_entry`: entry `rp` of the RIRB. A port helper.
fn azalia_rirb_entry(az: &AzaliaSoftc, rp: i32) -> RirbEntry {
    let rirb = az.rirb_dma.get().addr.cast::<RirbEntry>();
    // SAFETY: `azalia_init_rirb` allocated `rirb_entries` entries at `rirb` and the callers
    // keep `0 <= rp < rirb_entries`; the device writes them, hence the volatile load.
    unsafe { rirb.add(rp as usize).read_volatile() }
}

/// `azalia_get_response`: wait for the response to the last command; unsolicited responses
/// met on the way are queued. Called with `audio_lock` held.
pub fn azalia_get_response(az: &AzaliaSoftc) -> Result<u32, Errno> {
    if az.az_read_1(HDA_RIRBCTL) & HDA_RIRBCTL_RIRBDMAEN == 0 {
        printf(format_args!("{}: RIRB is not running.\n", az.dev.xname()));
        return Err(Errno::EIO);
    }

    let mut i = 5000;
    loop {
        while i > 0 {
            let wp = i32::from(az.az_read_2(HDA_RIRBWP) & HDA_RIRBWP_RIRBWP);
            if az.rirb_rp.get() != wp {
                break;
            }
            delay(10);
            i -= 1;
        }
        if i == 0 {
            // RIRB time out
            return Err(Errno::ETIMEDOUT);
        }
        let mut rp = az.rirb_rp.get() + 1;
        if rp >= az.rirb_entries.get() {
            rp = 0;
        }
        az.rirb_rp.set(rp);
        let e = azalia_rirb_entry(az, rp);
        if e.resp_ex & RIRB_RESP_UNSOL != 0 {
            azalia_unsolq_put(az, e);
        } else {
            // QEMU's intel-hda fetches no further command while RINTCNT responses wait for
            // RIRBSTS.RINTFL to be acknowledged, which only the interrupt handler does, and
            // the handler cannot run during autoconf (IPL_HIGH) or while a CPU holds
            // audio_lock in here: do its RIRB work now, as it would next (deviation).
            if az.az_read_1(HDA_RIRBCTL) & HDA_RIRBCTL_RINTCTL != 0
                && az.az_read_1(HDA_RIRBSTS) & HDA_RIRBSTS_RINTFL != 0
            {
                azalia_rirb_intr(az);
            }
            return Ok(e.resp);
        }
    }
}

/// `azalia_rirb_kick_unsol_events`: the `unsol_to` timeout; hands the queued unsolicited
/// responses of the codec in use to [`azalia_unsol_event`].
pub fn azalia_rirb_kick_unsol_events(v: *mut c_void) {
    // SAFETY: `timeout_set` in `azalia_init_corb` gives the softc as the argument.
    let az = unsafe { &*v.cast::<AzaliaSoftc>() };

    if az.unsolq_kick.get() != 0 {
        return;
    }
    az.unsolq_kick.set(1);
    while az.unsolq_rp.get() != az.unsolq_wp.load(Ordering::Acquire) {
        let rp = az.unsolq_rp.get();
        // SAFETY: `rp < UNSOLQ_SIZE` entries of the queue; the acquire load above makes
        // the entry the interrupt wrote before moving `unsolq_wp` visible.
        let e = unsafe { az.unsolq.get().add(rp as usize).read() };
        let addr = rirb_resp_codec(e.resp_ex) as i32;
        let tag = rirb_unsol_tag(e.resp) as i32;

        az.unsolq_rp.set((rp + 1) % UNSOLQ_SIZE);

        // We only care about events on the using codec.
        // SAFETY: this timeout is not MPSAFE, so it runs under the kernel lock, and no
        // other codec reference is live in softclock.
        let codec = unsafe { az.cur_codec() };
        if codec.address == addr {
            let _ = azalia_unsol_event(codec, tag);
        }
    }
    az.unsolq_kick.set(0);
}

/// `azalia_rirb_intr`: drain the RIRB from the interrupt handler; solicited responses no
/// one waits for are dropped. Called with `audio_lock` held.
pub fn azalia_rirb_intr(az: &AzaliaSoftc) {
    let rirbsts = az.az_read_1(HDA_RIRBSTS);

    let wp = i32::from(az.az_read_2(HDA_RIRBWP) & HDA_RIRBWP_RIRBWP);
    while az.rirb_rp.get() != wp {
        let mut rp = az.rirb_rp.get() + 1;
        if rp >= az.rirb_entries.get() {
            rp = 0;
        }
        az.rirb_rp.set(rp);
        let e = azalia_rirb_entry(az, rp);
        if e.resp_ex & RIRB_RESP_UNSOL != 0 {
            azalia_unsolq_put(az, e);
        }
        // else: dropped solicited response
    }
    timeout_add_msec(&az.unsol_to, 1);

    az.az_write_1(
        HDA_RIRBSTS,
        rirbsts | HDA_RIRBSTS_RIRBOIS | HDA_RIRBSTS_RINTFL,
    );
}

/// `azalia_alloc_dmamem`: one coherent segment of `size` bytes, mapped and loaded, into
/// `d`.
pub fn azalia_alloc_dmamem(
    az: &AzaliaSoftc,
    size: usize,
    align: usize,
    d: &Cell<AzaliaDma>,
) -> Result<(), Errno> {
    let dmat = az.dmat();
    let mut dma = d.get();

    dma.size = size;
    let nsegs = bus_dmamem_alloc(dmat, size, align, 0, &mut dma.segments, BUS_DMA_NOWAIT)?;
    // The C goes to `free` with `err` still 0 when `nsegs != 1`; a one-entry array cannot
    // come back with more.
    let mut err: Result<(), Errno> = Ok(());
    'free: {
        if nsegs != 1 {
            break 'free;
        }
        let addr = match bus_dmamem_map(
            dmat,
            &mut dma.segments,
            size,
            BUS_DMA_NOWAIT | BUS_DMA_COHERENT,
        ) {
            Ok(addr) => addr,
            Err(e) => {
                err = Err(e);
                break 'free;
            }
        };
        'unmap: {
            let map = match bus_dmamap_create(dmat, size, 1, size, 0, BUS_DMA_NOWAIT) {
                Ok(map) => map,
                Err(e) => {
                    err = Err(e);
                    break 'unmap;
                }
            };
            // SAFETY: `addr` maps `size` bytes this allocation owns until it is freed.
            let loaded =
                unsafe { bus_dmamap_load(dmat, map, addr.as_ptr(), size, None, BUS_DMA_NOWAIT) };
            if let Err(e) = loaded {
                err = Err(e);
                // destroy:
                // SAFETY: the map was just created and is not used again.
                unsafe { bus_dmamap_destroy(dmat, NonNull::from(map)) };
                break 'unmap;
            }

            dma.map = Some(map);
            dma.addr = addr.as_ptr();
            d.set(dma);

            if az.ok64.get() == 0 && ptr_upper32(dma.dmaaddr() as u64) != 0 {
                azalia_free_dmamem(az, d);
                return Err(Errno::EIO);
            }
            return Ok(());
        }
        // unmap:
        // SAFETY: the mapping made above, not used again.
        unsafe { bus_dmamem_unmap(dmat, addr, size) };
    }
    // free:
    // SAFETY: the segments allocated above, unmapped and not used again.
    unsafe { bus_dmamem_free(dmat, &dma.segments[..nsegs.min(1)]) };
    dma.addr = ptr::null_mut();
    dma.map = None;
    d.set(dma);
    err
}

/// `azalia_free_dmamem`: undo [`azalia_alloc_dmamem`].
pub fn azalia_free_dmamem(az: &AzaliaSoftc, d: &Cell<AzaliaDma>) {
    let mut dma = d.get();
    let Some(addr) = NonNull::new(dma.addr) else {
        return;
    };
    let dmat = az.dmat();
    if let Some(map) = dma.map.take() {
        bus_dmamap_unload(dmat, map);
        // SAFETY: the map `azalia_alloc_dmamem` created; `d` no longer refers to it.
        unsafe { bus_dmamap_destroy(dmat, NonNull::from(map)) };
    }
    // SAFETY: the mapping and segments `azalia_alloc_dmamem` made, of `dma.size` bytes;
    // their users (the controller and audio(4)) stopped before the free.
    unsafe {
        bus_dmamem_unmap(dmat, addr, dma.size);
        bus_dmamem_free(dmat, &dma.segments);
    }
    dma.addr = ptr::null_mut();
    d.set(dma);
}

/// `azalia_suspend`.
pub fn azalia_suspend(az: &AzaliaSoftc) -> Result<(), Errno> {
    if az.detached.get() != 0 {
        return Ok(());
    }

    // stop interrupts and clear status registers
    az.az_write_4(HDA_INTCTL, 0);
    az.az_write_2(HDA_STATESTS, HDA_STATESTS_SDIWAKE);
    az.az_write_1(HDA_RIRBSTS, HDA_RIRBSTS_RINTFL | HDA_RIRBSTS_RIRBOIS);
    let _ = az.az_read_4(HDA_INTSTS);

    // disable unsolicited responses
    az.az_write_4(HDA_GCTL, az.az_read_4(HDA_GCTL) & !HDA_GCTL_UNSOL);

    timeout_del(&az.unsol_to);

    // azalia_halt_{corb,rirb}() only fail if the {CORB,RIRB} can't be stopped and
    // azalia_init_{corb,rirb}(), which starts the {CORB,RIRB}, first calls
    // azalia_halt_{corb,rirb}(). If halt fails, don't try to restart.
    let err;
    'corb_fail: {
        if let Err(e) = azalia_halt_corb(az) {
            err = e;
            break 'corb_fail;
        }

        if let Err(e) = azalia_halt_rirb(az) {
            err = e;
            // rirb_fail:
            let _ = azalia_init_corb(az, true);
            break 'corb_fail;
        }

        // stop interrupts and clear status registers
        az.az_write_4(HDA_INTCTL, 0);
        az.az_write_2(HDA_STATESTS, HDA_STATESTS_SDIWAKE);
        az.az_write_1(HDA_RIRBSTS, HDA_RIRBSTS_RINTFL | HDA_RIRBSTS_RIRBOIS);

        return Ok(());
    }
    // corb_fail:
    az.az_write_4(HDA_GCTL, az.az_read_4(HDA_GCTL) | HDA_GCTL_UNSOL);

    Err(err)
}

/// `azalia_resume_codec`: power the codec's nodes up again and redo the pin setup and
/// quirks.
pub fn azalia_resume_codec(this: &mut Codec) -> Result<(), Errno> {
    // An error is only reported under AZALIA_DEBUG.
    let _ = azalia_comresp(this, this.audiofunc, CORB_SET_POWER_STATE, CORB_PS_D0);
    delay(100);

    if this.qrks & AZ_QRK_DOLBY_ATMOS != 0 {
        azalia_codec_init_dolby_atmos(this);
    }

    for i in this.widgets() {
        let (widgetcap, type_, nid) = {
            let w = this.wi(i);
            (w.widgetcap, w.type_, w.nid)
        };
        if widgetcap & COP_AWCAP_POWER != 0 {
            let _ = azalia_comresp(this, nid, CORB_SET_POWER_STATE, CORB_PS_D0);
            delay(100);
        }
        if type_ == COP_AWTYPE_PIN_COMPLEX {
            let _ = azalia_widget_init_pin(this, i);
        }
        if this.qrks & AZ_QRK_WID_MASK != 0 {
            let _ = azalia_codec_widget_quirks(this, nid);
        }
    }

    if this.qrks & AZ_QRK_GPIO_MASK != 0 {
        azalia_codec_gpio_quirks(this)?;
    }

    Ok(())
}

/// `azalia_resume`.
pub fn azalia_resume(az: &AzaliaSoftc) -> Result<(), Errno> {
    if az.detached.get() != 0 {
        return Ok(());
    }

    azalia_configure_pci(az);

    // is this necessary?
    pci_conf_write(az.pc(), az.tag.get(), PCI_SUBSYS_ID_REG, az.subid.get());

    azalia_init(az, true)?;

    // enable unsolicited responses on the controller
    az.az_write_4(HDA_GCTL, az.az_read_4(HDA_GCTL) | HDA_GCTL_UNSOL);

    // SAFETY: resume runs under the kernel lock, and nothing else holds a codec.
    let codec = unsafe { az.cur_codec() };
    azalia_resume_codec(codec)?;

    azalia_codec_enable_unsol(codec)?;

    Ok(())
}

// ================================================================
// HDA codec functions
// ================================================================

/// `azalia_codec_init`: identify the codec, walk its audio function's widgets, choose
/// converters and pins, and build the formats, volume groups and mixer.
pub fn azalia_codec_init(this: &mut Codec) -> Result<(), Errno> {
    // codec vendor/device/revision
    let _rev = azalia_comresp(this, CORB_NID_ROOT, CORB_GET_PARAMETER, COP_REVISION_ID)?;
    let id = azalia_comresp(this, CORB_NID_ROOT, CORB_GET_PARAMETER, COP_VENDOR_ID)?;
    this.vid = id;
    this.subid = this.az().subid.get();
    let _ = azalia_codec_init_vtbl(this);

    // identify function nodes
    let result = azalia_comresp(
        this,
        CORB_NID_ROOT,
        CORB_GET_PARAMETER,
        COP_SUBORDINATE_NODE_COUNT,
    )?;
    this.nfunctions = cop_nsubnodes(result) as i32;
    if cop_nsubnodes(result) == 0 {
        // No function groups
        return Err(Errno::EIO);
    }
    // iterate function nodes and find an audio function
    let n = cop_start_nid(result) as NidT;
    this.audiofunc = -1;
    for i in 0..this.nfunctions {
        let Ok(result) = azalia_comresp(this, n + i, CORB_GET_PARAMETER, COP_FUNCTION_GROUP_TYPE)
        else {
            continue;
        };
        if cop_ftype(result) == COP_FTYPE_AUDIO {
            this.audiofunc = n + i;
            break; // XXX multiple audio functions?
        }
    }
    if this.audiofunc < 0 {
        // No audio function groups
        return Err(Errno::EIO);
    }
    let af = this.audiofunc;

    // power the audio function
    let _ = azalia_comresp(this, af, CORB_SET_POWER_STATE, CORB_PS_D0);
    delay(100);

    // check widgets in the audio function
    let mut result = azalia_comresp(this, af, CORB_GET_PARAMETER, COP_SUBORDINATE_NODE_COUNT)?;
    this.wstart = cop_start_nid(result) as NidT;
    if this.wstart < 2 {
        printf(format_args!(
            "{}: invalid node structure\n",
            this.az().dev.xname()
        ));
        return Err(Errno::EIO);
    }
    this.wend = this.wstart + cop_nsubnodes(result) as NidT;
    let mut w: Vec<Widget> = Vec::new();
    if w.try_reserve_exact(this.wend as usize).is_err() {
        printf(format_args!("{}: out of memory\n", this.az().dev.xname()));
        return Err(Errno::ENOMEM);
    }
    w.resize_with(this.wend as usize, Widget::new);
    this.w = w;

    if this.qrks & AZ_QRK_DOLBY_ATMOS != 0 {
        azalia_codec_init_dolby_atmos(this);
    }

    // query the base parameters; as in C, a failed query leaves `result` as it was
    if let Ok(r) = azalia_comresp(this, af, CORB_GET_PARAMETER, COP_STREAM_FORMATS) {
        result = r;
    }
    this.wi_mut(af).d.audio_mut().encodings = result;
    if let Ok(r) = azalia_comresp(this, af, CORB_GET_PARAMETER, COP_PCM) {
        result = r;
    }
    this.wi_mut(af).d.audio_mut().bits_rates = result;
    if let Ok(r) = azalia_comresp(this, af, CORB_GET_PARAMETER, COP_INPUT_AMPCAP) {
        result = r;
    }
    this.wi_mut(af).inamp_cap = result;
    if let Ok(r) = azalia_comresp(this, af, CORB_GET_PARAMETER, COP_OUTPUT_AMPCAP) {
        result = r;
    }
    this.wi_mut(af).outamp_cap = result;

    azalia_codec_print_audiofunc(this);

    strlcpy(&mut this.wi_mut(CORB_NID_ROOT).name, b"root");
    strlcpy(&mut this.wi_mut(af).name, b"hdaudio");
    this.wi_mut(af).enable = true;

    for i in this.widgets() {
        azalia_widget_init(this, i)?;
        azalia_widget_init_connection(this, i)?;

        azalia_widget_print_widget(this.wi(i), this);

        if this.qrks & AZ_QRK_WID_MASK != 0 {
            let _ = azalia_codec_widget_quirks(this, i);
        }
    }

    this.na_dacs = 0;
    this.na_dacs_d = 0;
    this.na_adcs = 0;
    this.na_adcs_d = 0;
    this.speaker = -1;
    this.speaker2 = -1;
    this.spkr_dac = -1;
    this.fhp = -1;
    this.fhp_dac = -1;
    this.mic = -1;
    this.mic_adc = -1;
    this.nsense_pins = 0;
    this.nout_jacks = 0;
    let mut nspdif = 0;
    let mut nhdmi = 0;
    for i in this.widgets() {
        let w = this.wi(i);
        if !w.enable {
            continue;
        }

        match w.type_ {
            COP_AWTYPE_AUDIO_MIXER | COP_AWTYPE_AUDIO_SELECTOR => {
                if !azalia_widget_check_conn(this, i, 0) {
                    this.wi_mut(i).enable = false;
                }
            }

            COP_AWTYPE_AUDIO_OUTPUT => {
                if w.widgetcap & COP_AWCAP_DIGITAL == 0 {
                    if (this.na_dacs as usize) < HDA_MAX_CHANNELS {
                        this.a_dacs[this.na_dacs as usize] = i;
                        this.na_dacs += 1;
                    }
                } else if (this.na_dacs_d as usize) < HDA_MAX_CHANNELS {
                    this.a_dacs_d[this.na_dacs_d as usize] = i;
                    this.na_dacs_d += 1;
                }
            }

            COP_AWTYPE_AUDIO_INPUT => {
                if w.widgetcap & COP_AWCAP_DIGITAL == 0 {
                    if (this.na_adcs as usize) < HDA_MAX_CHANNELS {
                        this.a_adcs[this.na_adcs as usize] = i;
                        this.na_adcs += 1;
                    }
                } else if (this.na_adcs_d as usize) < HDA_MAX_CHANNELS {
                    this.a_adcs_d[this.na_adcs_d as usize] = i;
                    this.na_adcs_d += 1;
                }
            }

            COP_AWTYPE_PIN_COMPLEX => {
                let pin = *w.d.pin();
                match corb_cd_port(pin.config) {
                    CORB_CD_FIXED => match pin.device {
                        CORB_CD_SPEAKER => {
                            if this.speaker == -1 {
                                this.speaker = i;
                            } else {
                                let sp = *this.wi(this.speaker).d.pin();
                                if pin.association < sp.association
                                    || (pin.association == sp.association
                                        && pin.sequence < sp.sequence)
                                {
                                    this.speaker2 = this.speaker;
                                    this.speaker = i;
                                } else {
                                    this.speaker2 = i;
                                }
                            }
                            if this.speaker == i {
                                this.spkr_dac = azalia_codec_find_defdac(this, i, 0);
                            }
                        }
                        CORB_CD_MICIN => {
                            this.mic = i;
                            this.mic_adc = azalia_codec_find_defadc(this, i, 0);
                        }
                        _ => {}
                    },
                    CORB_CD_JACK => {
                        if pin.device == CORB_CD_LINEOUT {
                            this.nout_jacks += 1;
                        } else if pin.device == CORB_CD_HEADPHONE
                            && corb_cd_loc_geo(pin.config) == CORB_CD_FRONT
                        {
                            this.fhp = i;
                            this.fhp_dac = azalia_codec_find_defdac(this, i, 0);
                        }
                        if (this.nsense_pins as usize) < HDA_MAX_SENSE_PINS
                            && pin.cap & COP_PINCAP_PRESENCE != 0
                        {
                            // check override bit
                            if let Ok(result) =
                                azalia_comresp(this, i, CORB_GET_CONFIGURATION_DEFAULT, 0)
                                && corb_cd_misc(result) & CORB_CD_PRESENCEOV == 0
                            {
                                this.sense_pins[this.nsense_pins as usize] = i;
                                this.nsense_pins += 1;
                            }
                        }
                    }
                    _ => {}
                }
                if pin.device == CORB_CD_DIGITALOUT && pin.cap & COP_PINCAP_HDMI != 0 {
                    nhdmi += 1;
                } else if pin.device == CORB_CD_SPDIFOUT || pin.device == CORB_CD_SPDIFIN {
                    nspdif += 1;
                }
            }
            _ => {}
        }
    }
    this.codec_type = AZ_CODEC_TYPE_ANALOG;
    if this.na_dacs == 0 && this.na_adcs == 0 {
        this.codec_type = AZ_CODEC_TYPE_DIGITAL;
        if nspdif == 0 && nhdmi > 0 {
            this.codec_type = AZ_CODEC_TYPE_HDMI;
        }
    }

    // make sure built-in mic is connected to an adc
    if this.mic != -1 && this.mic_adc == -1 {
        // A failure is only reported under AZALIA_DEBUG.
        let _ = azalia_codec_select_micadc(this);
    }

    azalia_codec_sort_pins(this)?;

    azalia_codec_find_inputmixer(this)?;

    // If the codec can do multichannel, select different DACs for the multichannel jack
    // group. Also be sure to keep track of which DAC the front headphone is connected to.
    if this.na_dacs >= 3 && this.nopins() >= 3 {
        azalia_codec_select_dacs(this)?;
    }

    azalia_codec_select_spkrdac(this)?;

    azalia_init_dacgroup(this)?;

    azalia_codec_print_groups(this);

    azalia_widget_label_widgets(this)?;

    azalia_codec_construct_format(this, 0, 0)?;

    azalia_codec_init_volgroups(this)?;

    if this.qrks & AZ_QRK_GPIO_MASK != 0 {
        azalia_codec_gpio_quirks(this)?;
    }

    azalia_mixer_init(this)?;

    Ok(())
}

/// `azalia_codec_find_inputmixer`: a mixer that takes an input pin and feeds both an
/// output pin and an ADC.
pub fn azalia_codec_find_inputmixer(this: &mut Codec) -> Result<(), Errno> {
    this.input_mixer = -1;

    for i in this.widgets() {
        let w = this.wi(i);
        if w.type_ != COP_AWTYPE_AUDIO_MIXER {
            continue;
        }

        // can input from a pin
        if !this
            .ipins
            .iter()
            .any(|p| azalia_codec_fnode(this, p.nid, w.nid, 0) != -1)
        {
            continue;
        }

        // can output to a pin
        if !this
            .opins
            .iter()
            .any(|p| azalia_codec_fnode(this, w.nid, p.nid, 0) != -1)
        {
            continue;
        }

        // can output to an ADC
        if !this.a_adcs[..this.na_adcs as usize]
            .iter()
            .any(|&adc| azalia_codec_fnode(this, w.nid, adc, 0) != -1)
        {
            continue;
        }

        this.input_mixer = i;
        break;
    }
    Ok(())
}

/// `azalia_codec_select_micadc`: route the built-in mic to an ADC through the selectors.
pub fn azalia_codec_select_micadc(this: &mut Codec) -> Result<(), Errno> {
    let nadcs = this.na_adcs as usize;
    let Some(conv) = this.a_adcs[..nadcs]
        .iter()
        .copied()
        .find(|&adc| azalia_codec_fnode(this, this.mic, adc, 0) >= 0)
    else {
        return Err(Errno::EIO);
    };

    let mut w = conv;
    for j in 0..10 {
        let nconn = this.wi(w).connections.len();
        let mut i = 0;
        while i < nconn {
            let c = this.wi(w).connections[i];
            if azalia_widget_enabled(this, c) && azalia_codec_fnode(this, this.mic, c, j + 1) >= 0 {
                break;
            }
            i += 1;
        }
        if i >= nconn {
            return Err(Errno::EIO);
        }
        let nid = this.wi(w).nid;
        azalia_comresp(this, nid, CORB_SET_CONNECTION_SELECT_CONTROL, i as u32)?;
        this.wi_mut(w).selected = i as i32;
        let c = this.wi(w).connections[i];
        if c == this.mic {
            this.mic_adc = conv;
            return Ok(());
        }
        w = c;
    }
    Err(Errno::EIO)
}

/// The C's insertion of the pins found by `azalia_codec_sort_pins` into a new array,
/// ordered by `prio` (equal priorities keep their order). A port helper.
fn azalia_sorted_pins(pins: &[IoPin]) -> Result<Vec<IoPin>, Errno> {
    let mut sorted: Vec<IoPin> = Vec::new();
    sorted
        .try_reserve_exact(pins.len())
        .map_err(|_| Errno::ENOMEM)?;
    for pin in pins {
        let j = sorted
            .iter()
            .position(|p| p.prio > pin.prio)
            .unwrap_or(sorted.len());
        sorted.insert(j, *pin);
    }
    Ok(sorted)
}

/// `azalia_codec_sort_pins`: the analog and digital output and input pins with their
/// default converters, by priority.
pub fn azalia_codec_sort_pins(this: &mut Codec) -> Result<(), Errno> {
    let mut opins = [IoPin::default(); MAX_PINS];
    let mut opins_d = [IoPin::default(); MAX_PINS];
    let mut ipins = [IoPin::default(); MAX_PINS];
    let mut ipins_d = [IoPin::default(); MAX_PINS];
    let (mut nopins, mut nopins_d, mut nipins, mut nipins_d) = (0, 0, 0, 0);

    for i in this.widgets() {
        let w = this.wi(i);
        if !w.enable || w.type_ != COP_AWTYPE_PIN_COMPLEX {
            continue;
        }
        let pin = *w.d.pin();
        let digital = w.widgetcap & COP_AWCAP_DIGITAL != 0;

        let mut loc = 0;
        if this.na_dacs >= 3 && this.nout_jacks < 3 {
            loc = corb_cd_loc_geo(pin.config) as i32;
        }

        let mut prio = ((pin.association << 4) | pin.sequence) as i32;

        // analog out
        if pin.cap & COP_PINCAP_OUTPUT != 0 && !digital {
            let (mut add, mut nd) = (false, 0);
            let conv = azalia_codec_find_defdac(this, w.nid, 0);
            match pin.device {
                // primary - output by default
                CORB_CD_SPEAKER if w.nid == this.speaker || w.nid == this.speaker2 => {}
                CORB_CD_SPEAKER | CORB_CD_HEADPHONE | CORB_CD_LINEOUT => add = true,
                // secondary - input by default
                CORB_CD_MICIN if w.nid == this.mic => {}
                CORB_CD_MICIN | CORB_CD_LINEIN => {
                    add = true;
                    nd = 1;
                }
                _ => {}
            }
            if add && nopins < MAX_PINS {
                prio |= (nd << 8) | (loc << 9);
                opins[nopins] = IoPin {
                    nid: w.nid,
                    conv,
                    prio,
                };
                nopins += 1;
            }
        }
        // digital out
        if pin.cap & COP_PINCAP_OUTPUT != 0 && digital {
            let conv = azalia_codec_find_defdac(this, w.nid, 0);
            if matches!(pin.device, CORB_CD_SPDIFOUT | CORB_CD_DIGITALOUT) && nopins_d < MAX_PINS {
                opins_d[nopins_d] = IoPin {
                    nid: w.nid,
                    conv,
                    prio,
                };
                nopins_d += 1;
            }
        }
        // analog in
        if pin.cap & COP_PINCAP_INPUT != 0 && !digital {
            let (mut add, mut nd) = (false, 0);
            let conv = azalia_codec_find_defadc(this, w.nid, 0);
            match pin.device {
                // primary - input by default
                CORB_CD_MICIN | CORB_CD_LINEIN => add = true,
                // secondary - output by default
                CORB_CD_SPEAKER if w.nid == this.speaker || w.nid == this.speaker2 => {}
                CORB_CD_SPEAKER | CORB_CD_HEADPHONE | CORB_CD_LINEOUT => {
                    add = true;
                    nd = 1;
                }
                _ => {}
            }
            if add && nipins < MAX_PINS {
                ipins[nipins] = IoPin {
                    nid: w.nid,
                    prio: prio | (nd << 8),
                    conv,
                };
                nipins += 1;
            }
        }
        // digital in
        if pin.cap & COP_PINCAP_INPUT != 0 && digital {
            let conv = azalia_codec_find_defadc(this, w.nid, 0);
            if matches!(
                pin.device,
                CORB_CD_SPDIFIN | CORB_CD_DIGITALIN | CORB_CD_MICIN
            ) && nipins_d < MAX_PINS
            {
                ipins_d[nipins_d] = IoPin {
                    nid: w.nid,
                    prio,
                    conv,
                };
                nipins_d += 1;
            }
        }
    }

    this.opins = azalia_sorted_pins(&opins[..nopins])?;
    this.opins_d = azalia_sorted_pins(&opins_d[..nopins_d])?;
    this.ipins = azalia_sorted_pins(&ipins[..nipins])?;
    this.ipins_d = azalia_sorted_pins(&ipins_d[..nipins_d])?;

    Ok(())
}

/// `azalia_codec_select_dacs`: give the output pins of a multichannel codec different
/// DACs where their selectors allow.
pub fn azalia_codec_select_dacs(this: &mut Codec) -> Result<(), Errno> {
    let na_dacs = this.na_dacs as usize;
    let mut convs: Vec<NidT> = Vec::new();
    convs
        .try_reserve_exact(na_dacs)
        .map_err(|_| Errno::ENOMEM)?;

    let mut err = Ok(());
    for i in 0..this.opins.len() {
        let w = this.opins[i].nid;

        let mut conv = this.opins[i].conv;
        if !convs.contains(&conv) {
            convs.push(conv);
            if this.wi(w).nid == this.fhp {
                this.fhp_dac = conv;
            }
            if convs.len() >= na_dacs {
                break;
            }
        } else {
            // find a different dac
            conv = -1;
            let nconn = this.wi(w).connections.len();
            let mut j = 0;
            while j < nconn {
                let c = this.wi(w).connections[j];
                if azalia_widget_enabled(this, c) {
                    conv = azalia_codec_find_defdac(this, c, 1);
                    if conv != -1 && !convs.contains(&conv) {
                        break;
                    }
                }
                j += 1;
            }
            if j < nconn && conv != -1 {
                let nid = this.wi(w).nid;
                if let Err(e) =
                    azalia_comresp(this, nid, CORB_SET_CONNECTION_SELECT_CONTROL, j as u32)
                {
                    err = Err(e);
                    break;
                }
                this.wi_mut(w).selected = j as i32;
                this.opins[i].conv = conv;
                if nid == this.fhp {
                    this.fhp_dac = conv;
                }
                convs.push(conv);
                if convs.len() >= na_dacs {
                    break;
                }
            }
        }
    }

    err
}

/// `azalia_codec_select_spkrdac`: connect the speaker to a DAC that no other output pin is
/// connected to by default. If that is not possible, connect to a DAC other than the one
/// the first output pin is connected to.
pub fn azalia_codec_select_spkrdac(this: &mut Codec) -> Result<(), Errno> {
    let mut convs = [0 as NidT; HDA_MAX_CHANNELS];
    let mut nconv = 0;
    let mut fspkr = false;
    for i in 0..this.opins.len() {
        let conv = this.opins[i].conv;
        if !convs[..nconv].contains(&conv) {
            if conv == this.spkr_dac {
                fspkr = true;
            }
            convs[nconv] = conv;
            nconv += 1;
            if nconv as i32 == this.na_dacs {
                break;
            }
        }
    }

    if fspkr {
        let mut conn: i32 = -1;
        let mut conv: NidT = -1;
        let mut w = this.speaker;
        let nconn = this.wi(w).connections.len();
        let mut i = 0;
        while i < nconn {
            conv = azalia_codec_find_defdac(this, this.wi(w).connections[i], 1);
            if !convs[..nconv].contains(&conv) {
                break;
            }
            i += 1;
        }
        if i < nconn {
            conn = i as i32;
        } else {
            // Couldn't get a unique DAC. Try to get a different DAC than the first pin's
            // DAC.
            if this.spkr_dac == this.opins[0].conv {
                // If the speaker connection can't be changed, change the first pin's
                // connection.
                if this.wi(w).connections.len() == 1 {
                    w = this.opins[0].nid;
                }
                for j in 0..this.wi(w).connections.len() {
                    conv = azalia_codec_find_defdac(this, this.wi(w).connections[j], 1);
                    if conv != this.opins[0].conv {
                        conn = j as i32;
                        break;
                    }
                }
            }
        }
        if conn != -1 && conv != -1 {
            let nid = this.wi(w).nid;
            azalia_comresp(this, nid, CORB_SET_CONNECTION_SELECT_CONTROL, conn as u32)?;
            this.wi_mut(w).selected = conn;
            if nid == this.speaker {
                this.spkr_dac = conv;
            } else {
                this.opins[0].conv = conv;
            }
        }
    }

    // If there is a speaker2, try to connect it to spkr_dac.
    if this.speaker2 != -1 {
        let mut conn: i32 = -1;
        let w = this.speaker2;
        for i in 0..this.wi(w).connections.len() {
            let conv = azalia_codec_find_defdac(this, this.wi(w).connections[i], 1);
            if this.qrks & AZ_QRK_ROUTE_SPKR2_DAC != 0 {
                if conv != this.spkr_dac {
                    conn = i as i32;
                    break;
                }
            } else if conv == this.spkr_dac {
                conn = i as i32;
                break;
            }
        }
        if conn != -1 {
            let nid = this.wi(w).nid;
            azalia_comresp(this, nid, CORB_SET_CONNECTION_SELECT_CONTROL, conn as u32)?;
            this.wi_mut(w).selected = conn;
        }
    }

    Ok(())
}

/// The connection a selector or pin uses by default, `w->connections[w->selected]`, under
/// the C's guard `w->selected >= 0 && w->selected < sizeof(w->connections)` (the size of a
/// pointer); an index past the list is `None` here where the C reads past the array. A port
/// helper.
fn azalia_selected_conn(w: &Widget) -> Option<NidT> {
    if w.selected >= 0 && (w.selected as usize) < size_of::<*const NidT>() {
        w.connections.get(w.selected as usize).copied()
    } else {
        None
    }
}

/// `azalia_codec_find_defdac`: the DAC that widget `index` plays from by default, -1 for
/// none.
pub fn azalia_codec_find_defdac(this: &Codec, mut index: i32, mut depth: i32) -> i32 {
    let w = this.wi(index);
    if !w.enable {
        return -1;
    }

    if w.type_ == COP_AWTYPE_AUDIO_OUTPUT {
        return index;
    }

    if depth > 0
        && (w.type_ == COP_AWTYPE_PIN_COMPLEX
            || w.type_ == COP_AWTYPE_BEEP_GENERATOR
            || w.type_ == COP_AWTYPE_AUDIO_INPUT)
    {
        return -1;
    }
    depth += 1;
    if depth >= 10 {
        return -1;
    }

    if !w.connections.is_empty() {
        // by default, all mixer connections are active
        if w.type_ == COP_AWTYPE_AUDIO_MIXER {
            for &c in &w.connections {
                index = c;
                if !azalia_widget_enabled(this, index) {
                    continue;
                }
                let ret = azalia_codec_find_defdac(this, index, depth);
                if ret >= 0 {
                    return ret;
                }
            }
        // 7.3.3.2 Connection Select Control
        // If an attempt is made to Set an index value greater than the number of list
        // entries (index is equal to or greater than the Connection List Length property
        // for the widget) the behavior is not predictable.

        // negative index values are wrong too
        } else if let Some(index) = azalia_selected_conn(w)
            && valid_widget_nid(index, this)
        {
            let ret = azalia_codec_find_defdac(this, index, depth);
            if ret >= 0 {
                return ret;
            }
        }
    }

    -1
}

/// `azalia_codec_find_defadc_sub`: `index` if node `node` reaches it by default, walking
/// back from the ADC side; -1 otherwise.
pub fn azalia_codec_find_defadc_sub(this: &Codec, node: NidT, index: i32, mut depth: i32) -> i32 {
    let w = this.wi(index);
    if w.nid == node {
        return index;
    }
    // back at the beginning or a bad end
    if depth > 0
        && (w.type_ == COP_AWTYPE_PIN_COMPLEX
            || w.type_ == COP_AWTYPE_BEEP_GENERATOR
            || w.type_ == COP_AWTYPE_AUDIO_OUTPUT
            || w.type_ == COP_AWTYPE_AUDIO_INPUT)
    {
        return -1;
    }
    depth += 1;
    if depth >= 10 {
        return -1;
    }

    if !w.connections.is_empty() {
        // by default, all mixer connections are active
        if w.type_ == COP_AWTYPE_AUDIO_MIXER {
            for &c in &w.connections {
                if !azalia_widget_enabled(this, c) {
                    continue;
                }
                let ret = azalia_codec_find_defadc_sub(this, node, c, depth);
                if ret >= 0 {
                    return ret;
                }
            }
        // 7.3.3.2 Connection Select Control (see azalia_codec_find_defdac); negative index
        // values are wrong too
        } else if let Some(index) = azalia_selected_conn(w)
            && valid_widget_nid(index, this)
        {
            let ret = azalia_codec_find_defadc_sub(this, node, index, depth);
            if ret >= 0 {
                return ret;
            }
        }
    }
    -1
}

/// `azalia_codec_find_defadc`: the ADC that records widget `index` by default, -1 for
/// none.
pub fn azalia_codec_find_defadc(this: &Codec, index: i32, _depth: i32) -> i32 {
    this.a_adcs[..this.na_adcs as usize]
        .iter()
        .copied()
        .find(|&adc| azalia_codec_find_defadc_sub(this, index, adc, 0) >= 0)
        .unwrap_or(-1)
}

/// `azalia_codec_init_volgroups`: the amplifiers `outputs.master` and `record.volume`
/// drive, and which of them are on by default.
pub fn azalia_codec_init_volgroups(this: &mut Codec) -> Result<(), Errno> {
    let mut j = 0;
    this.playvols.mask = 0;
    for i in this.widgets() {
        let w = this.wi(i);
        if !w.enable {
            continue;
        }
        if w.mixer_class == AZ_CLASS_RECORD {
            continue;
        }
        if w.widgetcap & COP_AWCAP_OUTAMP == 0 {
            continue;
        }
        if cop_ampcap_numsteps(w.outamp_cap) == 0 && w.outamp_cap & COP_AMPCAP_MUTE == 0 {
            continue;
        }
        let nid = w.nid;
        this.playvols.mask |= 1 << j;
        this.playvols.slaves[j] = nid;
        j += 1;
        if j >= AZ_MAX_VOL_SLAVES {
            break;
        }
    }
    this.playvols.nslaves = j as i32;

    let defdac = this.dacs.current().conv[0];
    this.playvols.cur = 0;
    for i in 0..j {
        let w = this.wi(this.playvols.slaves[i]);
        if w.nid == this.input_mixer || w.parent == this.input_mixer || widget_channels(w) < 2 {
            continue;
        }
        // azalia_codec_find_defdac only goes 10 connections deep. Start the connection
        // depth at 7 so it doesn't go more than 3 connections deep.
        let depth = if w.type_ == COP_AWTYPE_AUDIO_MIXER || w.type_ == COP_AWTYPE_AUDIO_SELECTOR {
            7
        } else {
            0
        };
        let dac = azalia_codec_find_defdac(this, w.nid, depth);
        if dac == -1 {
            continue;
        }
        if dac != defdac && dac != this.spkr_dac && dac != this.fhp_dac {
            continue;
        }
        let cap = w.outamp_cap;
        if cap & COP_AMPCAP_MUTE != 0 && cop_ampcap_numsteps(cap) != 0 {
            if w.type_ == COP_AWTYPE_BEEP_GENERATOR {
                continue;
            } else if w.type_ == COP_AWTYPE_PIN_COMPLEX {
                if let Ok(result) = azalia_comresp(this, w.nid, CORB_GET_PIN_WIDGET_CONTROL, 0)
                    && result & CORB_PWC_OUTPUT != 0
                {
                    this.playvols.cur |= 1 << i;
                }
            } else {
                this.playvols.cur |= 1 << i;
            }
        }
    }
    if this.playvols.cur == 0 {
        for i in 0..j {
            let w = this.wi(this.playvols.slaves[i]);
            let depth = if w.type_ == COP_AWTYPE_AUDIO_MIXER || w.type_ == COP_AWTYPE_AUDIO_SELECTOR
            {
                7
            } else {
                0
            };
            let dac = azalia_codec_find_defdac(this, w.nid, depth);
            if dac == -1 {
                continue;
            }
            if dac != defdac && dac != this.spkr_dac && dac != this.fhp_dac {
                continue;
            }
            if w.type_ == COP_AWTYPE_BEEP_GENERATOR {
                continue;
            }
            if w.type_ == COP_AWTYPE_PIN_COMPLEX {
                if let Ok(result) = azalia_comresp(this, w.nid, CORB_GET_PIN_WIDGET_CONTROL, 0)
                    && result & CORB_PWC_OUTPUT != 0
                {
                    this.playvols.cur |= 1 << i;
                }
            } else {
                this.playvols.cur |= 1 << i;
            }
        }
    }

    this.playvols.master = this.audiofunc;
    if this.playvols.nslaves > 0 {
        for i in this.widgets() {
            let w = this.wi(i);
            if w.type_ != COP_AWTYPE_VOLUME_KNOB {
                continue;
            }
            if cop_vkcap_numsteps(w.d.volume().cap) == 0 {
                continue;
            }
            this.playvols.master = w.nid;
            break;
        }
    }

    let mut j = 0;
    this.recvols.mask = 0;
    for i in this.widgets() {
        let w = this.wi(i);
        if !w.enable {
            continue;
        }
        if w.type_ == COP_AWTYPE_AUDIO_INPUT || w.type_ == COP_AWTYPE_PIN_COMPLEX {
            if w.widgetcap & COP_AWCAP_INAMP == 0 {
                continue;
            }
            if cop_ampcap_numsteps(w.inamp_cap) == 0 && w.inamp_cap & COP_AMPCAP_MUTE == 0 {
                continue;
            }
        } else if w.type_ == COP_AWTYPE_AUDIO_MIXER || w.type_ == COP_AWTYPE_AUDIO_SELECTOR {
            if w.mixer_class != AZ_CLASS_RECORD {
                continue;
            }
            if w.widgetcap & COP_AWCAP_OUTAMP == 0 {
                continue;
            }
            if cop_ampcap_numsteps(w.outamp_cap) == 0 && w.outamp_cap & COP_AMPCAP_MUTE == 0 {
                continue;
            }
        } else {
            continue;
        }
        let nid = w.nid;
        this.recvols.mask |= 1 << j;
        this.recvols.slaves[j] = nid;
        j += 1;
        if j >= AZ_MAX_VOL_SLAVES {
            break;
        }
    }
    this.recvols.nslaves = j as i32;

    this.recvols.cur = 0;
    for i in 0..j {
        let w = this.wi(this.recvols.slaves[i]);
        let cap = if w.type_ == COP_AWTYPE_AUDIO_INPUT || w.type_ != COP_AWTYPE_PIN_COMPLEX {
            w.inamp_cap
        } else if w.mixer_class != AZ_CLASS_RECORD {
            continue;
        } else {
            w.outamp_cap
        };
        if cap & COP_AMPCAP_MUTE != 0 && cop_ampcap_numsteps(cap) != 0 {
            if w.type_ == COP_AWTYPE_PIN_COMPLEX {
                if let Ok(result) = azalia_comresp(this, w.nid, CORB_GET_PIN_WIDGET_CONTROL, 0)
                    && result & CORB_PWC_OUTPUT == 0
                {
                    this.recvols.cur |= 1 << i;
                }
            } else {
                this.recvols.cur |= 1 << i;
            }
        }
    }
    if this.recvols.cur == 0 {
        for i in 0..j {
            let w = this.wi(this.recvols.slaves[i]);
            // the C computes the amplifier's capabilities here too, and does not use them
            if !(w.type_ == COP_AWTYPE_AUDIO_INPUT || w.type_ != COP_AWTYPE_PIN_COMPLEX)
                && w.mixer_class != AZ_CLASS_RECORD
            {
                continue;
            }
            if w.type_ == COP_AWTYPE_PIN_COMPLEX {
                if let Ok(result) = azalia_comresp(this, w.nid, CORB_GET_PIN_WIDGET_CONTROL, 0)
                    && result & CORB_PWC_OUTPUT == 0
                {
                    this.recvols.cur |= 1 << i;
                }
            } else {
                this.recvols.cur |= 1 << i;
            }
        }
    }

    this.recvols.master = this.audiofunc;

    Ok(())
}

/// `azalia_codec_delete`: free what [`azalia_codec_init`] allocated.
pub fn azalia_codec_delete(this: &mut Codec) -> Result<(), Errno> {
    let _ = azalia_mixer_delete(this);

    this.formats = Vec::new();
    this.opins = Vec::new();
    this.opins_d = Vec::new();
    this.ipins = Vec::new();
    this.ipins_d = Vec::new();
    this.w = Vec::new();

    Ok(())
}

/// The number of sample sizes a converter supports, `nbits` of
/// `azalia_codec_construct_format`; 32 bits do not count on a digital converter. A port
/// helper for the C's two identical blocks.
fn azalia_codec_nbits(w: &Widget) -> i32 {
    let bits_rates = w.d.audio().bits_rates;
    let mut nbits = 0;
    for b in [COP_PCM_B8, COP_PCM_B16, COP_PCM_B20, COP_PCM_B24] {
        if bits_rates & b != 0 {
            nbits += 1;
        }
    }
    if bits_rates & COP_PCM_B32 != 0 && w.widgetcap & COP_AWCAP_DIGITAL == 0 {
        nbits += 1;
    }
    nbits
}

/// `azalia_codec_construct_format`: the formats of DAC group `newdac` and ADC group
/// `newadc`.
pub fn azalia_codec_construct_format(
    this: &mut Codec,
    newdac: i32,
    newadc: i32,
) -> Result<(), Errno> {
    let mut variation = 0;

    if this.dacs.ngroups > 0 && newdac < this.dacs.ngroups && newdac >= 0 {
        this.dacs.cur = newdac;
        let w = this.wi(this.dacs.current().conv[0]);
        let nbits = azalia_codec_nbits(w);
        if nbits == 0 {
            printf(format_args!(
                "{}: invalid DAC PCM format: 0x{:08x}\n",
                this.az().dev.xname(),
                w.d.audio().bits_rates
            ));
            return Err(Errno::EIO);
        }
        variation += this.dacs.current().nconv * nbits;
    }

    if this.adcs.ngroups > 0 && newadc < this.adcs.ngroups && newadc >= 0 {
        this.adcs.cur = newadc;
        let w = this.wi(this.adcs.current().conv[0]);
        let nbits = azalia_codec_nbits(w);
        if nbits == 0 {
            printf(format_args!(
                "{}: invalid ADC PCM format: 0x{:08x}\n",
                this.az().dev.xname(),
                w.d.audio().bits_rates
            ));
            return Err(Errno::EIO);
        }
        variation += this.adcs.current().nconv * nbits;
    }

    if variation == 0 {
        // no converter groups
        return Err(Errno::EIO);
    }

    this.formats = Vec::new();
    let mut formats: Vec<AudioFormat> = Vec::new();
    if formats.try_reserve_exact(variation as usize).is_err() {
        printf(format_args!(
            "{}: out of memory in azalia_codec_construct_format\n",
            this.az().dev.xname()
        ));
        return Err(Errno::ENOMEM);
    }
    this.formats = formats;

    // register formats for playback, then for recording
    for (mode, group) in [
        (
            AUMODE_PLAY,
            (this.dacs.ngroups > 0).then(|| *this.dacs.current()),
        ),
        (
            AUMODE_RECORD,
            (this.adcs.ngroups > 0).then(|| *this.adcs.current()),
        ),
    ] {
        let Some(group) = group else {
            continue;
        };
        for c in 0..group.nconv as usize {
            let mut chan = 0;
            let mut bits_rates = !0u32;
            if this.wi(group.conv[0]).widgetcap & COP_AWCAP_DIGITAL != 0 {
                bits_rates &= !COP_PCM_B32;
            }
            for &nid in &group.conv[..=c] {
                chan += widget_channels(this.wi(nid));
                bits_rates &= this.wi(nid).d.audio().bits_rates;
            }
            azalia_codec_add_bits(this, chan, bits_rates, mode);
        }
    }

    Ok(())
}

/// `azalia_codec_add_bits`: one format per sample size in `bits_rates`.
pub fn azalia_codec_add_bits(this: &mut Codec, chan: i32, bits_rates: u32, mode: i32) {
    for (bit, prec) in [
        (COP_PCM_B8, 8),
        (COP_PCM_B16, 16),
        (COP_PCM_B20, 20),
        (COP_PCM_B24, 24),
        (COP_PCM_B32, 32),
    ] {
        if bits_rates & bit != 0 {
            azalia_codec_add_format(this, chan, prec, bits_rates, mode);
        }
    }
}

/// `azalia_codec_add_format`: append a format with the rates in `rates`.
pub fn azalia_codec_add_format(this: &mut Codec, chan: i32, prec: i32, rates: u32, mode: i32) {
    let mut f = AudioFormat::new();
    f.mode = mode;
    f.encoding = AUDIO_ENCODING_SLINEAR_LE;
    if prec == 8 {
        f.encoding = AUDIO_ENCODING_ULINEAR_LE;
    }
    f.precision = prec as u32;
    f.channels = chan as u32;
    f.frequency_type = 0;
    for (bit, rate) in [
        (COP_PCM_R80, 8000),
        (COP_PCM_R110, 11025),
        (COP_PCM_R160, 16000),
        (COP_PCM_R220, 22050),
        (COP_PCM_R320, 32000),
        (COP_PCM_R441, 44100),
        (COP_PCM_R480, 48000),
        (COP_PCM_R882, 88200),
        (COP_PCM_R960, 96000),
        (COP_PCM_R1764, 176400),
        (COP_PCM_R1920, 192000),
        (COP_PCM_R3840, 384000),
    ] {
        if rates & bit != 0 {
            f.frequency[f.frequency_type as usize] = rate;
            f.frequency_type += 1;
        }
    }
    this.formats.push(f);
}

/// `azalia_codec_connect_stream`: program the converters of the current group with the
/// stream's format and channels.
pub fn azalia_codec_connect_stream(this: &Stream) -> Result<(), Errno> {
    // SAFETY: called from audio(4)'s trigger methods, under the kernel lock, while nothing
    // else holds the codec.
    let codec: &Codec = unsafe { this.az().cur_codec() };

    let mut err = Ok(());
    let fmt = this.fmt.get();
    let nchan = i32::from(fmt & HDA_SD_FMT_CHAN) + 1;

    let group = if this.dir.get() == AUMODE_RECORD {
        codec.adcs.current()
    } else {
        codec.dacs.current()
    };

    let mut curchan = 0;
    for &conv in &group.conv[..group.nconv as usize] {
        let w = codec.wi(conv);
        let widchan = widget_channels(w);

        let mut stream_chan = (this.number.get() as u32) << 4;
        if curchan < nchan {
            stream_chan |= curchan as u32;
        } else if w.nid == codec.spkr_dac || w.nid == codec.fhp_dac {
            // first channel(s)
        } else {
            stream_chan = 0; // idle stream
        }

        if let Err(e) = azalia_comresp(codec, w.nid, CORB_SET_CONVERTER_FORMAT, u32::from(fmt)) {
            err = Err(e);
            break;
        }
        if let Err(e) = azalia_comresp(codec, w.nid, CORB_SET_CONVERTER_STREAM_CHANNEL, stream_chan)
        {
            err = Err(e);
            break;
        }

        if w.widgetcap & COP_AWCAP_DIGITAL != 0 {
            let digital = match azalia_comresp(codec, w.nid, CORB_GET_DIGITAL_CONTROL, 0) {
                Ok(d) => d,
                Err(e) => {
                    err = Err(e);
                    break;
                }
            };
            let digital = (digital & 0xff) | CORB_DCC_DIGEN;
            if let Err(e) = azalia_comresp(codec, w.nid, CORB_SET_DIGITAL_CONTROL_L, digital) {
                err = Err(e);
                break;
            }
        }
        curchan += widchan;
    }

    err
}

/// `azalia_codec_disconnect_stream`: put the current group's converters on stream #0 and
/// turn S/PDIF off.
pub fn azalia_codec_disconnect_stream(this: &Stream) -> Result<(), Errno> {
    // SAFETY: called from audio(4)'s halt methods, under the kernel lock, while nothing
    // else holds the codec.
    let codec: &Codec = unsafe { this.az().cur_codec() };

    let group = if this.dir.get() == AUMODE_RECORD {
        codec.adcs.current()
    } else {
        codec.dacs.current()
    };
    for &nid in &group.conv[..group.nconv as usize] {
        let _ = azalia_comresp(codec, nid, CORB_SET_CONVERTER_STREAM_CHANNEL, 0); // stream#0
        if codec.wi(nid).widgetcap & COP_AWCAP_DIGITAL != 0 {
            // disable S/PDIF
            let v = azalia_comresp(codec, nid, CORB_GET_DIGITAL_CONTROL, 0).unwrap_or(0);
            let v = (v & !CORB_DCC_DIGEN) & 0xff;
            let _ = azalia_comresp(codec, nid, CORB_SET_DIGITAL_CONTROL_L, v);
        }
    }
    Ok(())
}

// ================================================================
// HDA widget functions
// ================================================================

/// `azalia_widget_init`: the capabilities, type, power, amplifiers and type-specific
/// parameters of widget `nid`.
pub fn azalia_widget_init(codec: &mut Codec, nid: NidT) -> Result<(), Errno> {
    let result = azalia_comresp(codec, nid, CORB_GET_PARAMETER, COP_AUDIO_WIDGET_CAP)?;
    {
        let this = codec.wi_mut(nid);
        this.nid = nid;
        this.widgetcap = result;
        this.type_ = cop_awcap_type(result);
    }
    if result & COP_AWCAP_POWER != 0 {
        let _ = azalia_comresp(codec, nid, CORB_SET_POWER_STATE, CORB_PS_D0);
        delay(100);
    }

    let af = codec.audiofunc;
    {
        let this = codec.wi_mut(nid);
        this.enable = true;
        this.mixer_class = -1;
        this.parent = af;
    }

    match codec.wi(nid).type_ {
        COP_AWTYPE_AUDIO_OUTPUT | COP_AWTYPE_AUDIO_INPUT => {
            let _ = azalia_widget_init_audio(codec, nid);
        }
        COP_AWTYPE_PIN_COMPLEX => {
            let _ = azalia_widget_init_pin(codec, nid);
        }
        COP_AWTYPE_VOLUME_KNOB => {
            let result =
                azalia_comresp(codec, nid, CORB_GET_PARAMETER, COP_VOLUME_KNOB_CAPABILITIES)?;
            codec.wi_mut(nid).d.volume_mut().cap = result;
        }
        COP_AWTYPE_POWER | COP_AWTYPE_VENDOR_DEFINED => {
            codec.wi_mut(nid).enable = false;
        }
        _ => {}
    }

    // amplifier information
    // XXX (ab)use bits 24-30 to store the "control offset", which is the number of steps,
    // starting at 0, that have no effect. these bits are reserved in HDA 1.0.
    let widgetcap = codec.wi(nid).widgetcap;
    if widgetcap & COP_AWCAP_INAMP != 0 {
        let mut cap = codec.wi(nid).inamp_cap;
        if widgetcap & COP_AWCAP_AMPOV != 0 {
            if let Ok(r) = azalia_comresp(codec, nid, CORB_GET_PARAMETER, COP_INPUT_AMPCAP) {
                cap = r;
            }
        } else {
            cap = codec.wi(af).inamp_cap;
        }
        codec.wi_mut(nid).inamp_cap = cap & !(0x7f << 24);
    }
    if widgetcap & COP_AWCAP_OUTAMP != 0 {
        let mut cap = codec.wi(nid).outamp_cap;
        if widgetcap & COP_AWCAP_AMPOV != 0 {
            if let Ok(r) = azalia_comresp(codec, nid, CORB_GET_PARAMETER, COP_OUTPUT_AMPCAP) {
                cap = r;
            }
        } else {
            cap = codec.wi(af).outamp_cap;
        }
        codec.wi_mut(nid).outamp_cap = cap & !(0x7f << 24);
    }
    Ok(())
}

/// `azalia_widget_sole_conn`: the converter or pin that is the only user of widget `nid`,
/// -1 if there is none or more than one.
pub fn azalia_widget_sole_conn(this: &Codec, nid: NidT) -> i32 {
    // connected to ADC, then to DAC
    for set in [&this.adcs, &this.dacs] {
        for group in &set.groups[..set.ngroups as usize] {
            for &target in &group.conv[..group.nconv as usize] {
                let w = this.wi(target);
                if w.connections.len() == 1 && w.connections[0] == nid {
                    return target;
                }
            }
        }
    }
    // connected to pin complex
    let mut target = -1;
    for i in this.widgets() {
        let w = this.wi(i);
        if w.type_ != COP_AWTYPE_PIN_COMPLEX {
            continue;
        }
        if w.connections.len() == 1 && w.connections[0] == nid {
            if target != -1 {
                return -1;
            }
            target = i;
        } else {
            let mut nconn = 0;
            let mut has_target = false;
            for &c in &w.connections {
                if !this.wi(c).enable {
                    continue;
                }
                nconn += 1;
                if c == nid {
                    has_target = true;
                }
            }
            if has_target {
                if nconn == 1 {
                    if target != -1 {
                        return -1;
                    }
                    target = i;
                } else {
                    // not sole connection at least once
                    return -1;
                }
            }
        }
    }
    if target != -1 {
        return target;
    }

    -1
}

/// The name of the converter `w` in convgroup `group`: `"<prefix><type>-<first>:<last>"`
/// with its channels in the group, as the C's two loops per converter type write it. A
/// port helper.
fn azalia_widget_label_conv(w: &mut Widget, group: &Convgroup, prefix: &str) {
    let ch = widget_channels(w);
    let mut schan = 0;
    for &conv in &group.conv[..group.nconv as usize] {
        if w.nid == conv {
            snprintf(
                &mut w.name,
                format_args!(
                    "{prefix}{}-{}:{}",
                    WTYPES[w.type_ as usize],
                    schan,
                    schan + ch - 1
                ),
            );
        }
        schan += ch;
    }
}

/// `azalia_widget_label_widgets`: the mixer names of the widgets.
pub fn azalia_widget_label_widgets(codec: &mut Codec) -> Result<(), Errno> {
    let mut types = [0i32; 16];
    let mut pins = [0i32; 16];

    // If codec has more than one line-out jack, check if the jacks have unique colors.
    // If so, use the colors in the mixer names.
    let mut use_colors = true;
    let mut colors_used: u32 = 0;
    if codec.nout_jacks < 2 {
        use_colors = false;
    }
    for p in &codec.opins {
        if !use_colors {
            break;
        }
        let pin = codec.wi(p.nid).d.pin();
        if pin.device != CORB_CD_LINEOUT {
            continue;
        }
        if colors_used & (1 << pin.color) != 0 {
            use_colors = false;
        } else {
            colors_used |= 1 << pin.color;
        }
    }

    let dacs = codec.dacs;
    let adcs = codec.adcs;
    for i in codec.widgets() {
        let w = codec.wi_mut(i);
        // default for disabled/unused widgets
        let nid = w.nid;
        snprintf(&mut w.name, format_args!("u-wid{nid:02x}"));
        if !w.enable {
            continue;
        }
        match w.type_ {
            COP_AWTYPE_PIN_COMPLEX => {
                let pin = *w.d.pin();
                let dev = pin.device as usize;
                pins[dev] += 1;
                if use_colors && pin.device == CORB_CD_LINEOUT {
                    snprintf(
                        &mut w.name,
                        format_args!(
                            "{}-{}",
                            Str(PIN_DEVICES[dev]),
                            LINE_COLORS[pin.color as usize]
                        ),
                    );
                } else if pins[dev] > 1 {
                    snprintf(
                        &mut w.name,
                        format_args!("{}{}", Str(PIN_DEVICES[dev]), pins[dev]),
                    );
                } else {
                    snprintf(&mut w.name, format_args!("{}", Str(PIN_DEVICES[dev])));
                }
            }
            COP_AWTYPE_AUDIO_OUTPUT => {
                if dacs.ngroups < 1 {
                    continue;
                }
                azalia_widget_label_conv(w, &dacs.groups[0], "");
                if dacs.ngroups < 2 {
                    continue;
                }
                azalia_widget_label_conv(w, &dacs.groups[1], "dig-");
            }
            COP_AWTYPE_AUDIO_INPUT => {
                w.mixer_class = AZ_CLASS_RECORD;
                if adcs.ngroups < 1 {
                    continue;
                }
                azalia_widget_label_conv(w, &adcs.groups[0], "");
                if adcs.ngroups < 2 {
                    continue;
                }
                azalia_widget_label_conv(w, &adcs.groups[1], "dig-");
            }
            t => {
                let t = t as usize;
                types[t] += 1;
                if types[t] > 1 {
                    snprintf(&mut w.name, format_args!("{}{}", WTYPES[t], types[t]));
                } else {
                    snprintf(&mut w.name, format_args!("{}", WTYPES[t]));
                }
            }
        }
    }

    // Mixers and selectors that connect to only one other widget are functionally part of
    // the widget they are connected to. Show that relationship in the name.
    for i in codec.widgets() {
        let w = codec.wi(i);
        if w.type_ != COP_AWTYPE_AUDIO_MIXER && w.type_ != COP_AWTYPE_AUDIO_SELECTOR {
            continue;
        }
        if !w.enable {
            continue;
        }
        let mut j = azalia_widget_sole_conn(codec, i);
        if j == -1 {
            // Special case. A selector with outamp capabilities and is connected to a
            // single widget that has either no input or no output capabilities. This
            // widget serves as the input or output amp for the widget it is connected to.
            let w = codec.wi(i);
            if w.type_ == COP_AWTYPE_AUDIO_SELECTOR
                && w.widgetcap & COP_AWCAP_OUTAMP != 0
                && w.connections.len() == 1
            {
                j = w.connections[0];
                if !azalia_widget_enabled(codec, j) {
                    continue;
                }
                let jcap = codec.wi(j).widgetcap;
                if jcap & COP_AWCAP_INAMP == 0 {
                    codec.wi_mut(i).mixer_class = AZ_CLASS_INPUT;
                } else if jcap & COP_AWCAP_OUTAMP == 0 {
                    codec.wi_mut(i).mixer_class = AZ_CLASS_OUTPUT;
                } else {
                    continue;
                }
            }
        }
        if j >= 0 {
            // As part of a disabled widget, this widget should be disabled as well.
            if !codec.wi(j).enable {
                let w = codec.wi_mut(i);
                w.enable = false;
                snprintf(&mut w.name, format_args!("u-wid{i:02x}"));
                continue;
            }
            let name = codec.wi(j).name;
            let jclass = codec.wi(j).mixer_class;
            let w = codec.wi_mut(i);
            snprintf(&mut w.name, format_args!("{}", Str(&name)));
            if jclass == AZ_CLASS_RECORD {
                w.mixer_class = AZ_CLASS_RECORD;
            }
            w.parent = j;
        }
    }

    Ok(())
}

/// `azalia_widget_init_audio`: the stream formats and PCM sizes and rates of converter
/// `nid`, its own or the audio function's.
pub fn azalia_widget_init_audio(codec: &mut Codec, nid: NidT) -> Result<(), Errno> {
    let af = *codec.wi(codec.audiofunc).d.audio();

    // check audio format
    if codec.wi(nid).widgetcap & COP_AWCAP_FORMATOV != 0 {
        let result = azalia_comresp(codec, nid, CORB_GET_PARAMETER, COP_STREAM_FORMATS)?;
        codec.wi_mut(nid).d.audio_mut().encodings = result;
        if result == 0 {
            // quirk for CMI9880. This must not occur usually...
            *codec.wi_mut(nid).d.audio_mut() = af;
        } else {
            if result & COP_STREAM_FORMAT_PCM == 0 {
                printf(format_args!(
                    "{}: {}: No PCM support: {:x}\n",
                    codec.az().dev.xname(),
                    Str(&codec.wi(nid).name),
                    result
                ));
                return Err(Errno::EIO);
            }
            let result = azalia_comresp(codec, nid, CORB_GET_PARAMETER, COP_PCM)?;
            codec.wi_mut(nid).d.audio_mut().bits_rates = result;
        }
    } else {
        *codec.wi_mut(nid).d.audio_mut() = af;
    }
    Ok(())
}

/// `azalia_widget_init_pin`: the configuration default, capabilities, direction and EAPD of
/// pin `nid`; pins with no connection are disabled.
pub fn azalia_widget_init_pin(codec: &mut Codec, nid: NidT) -> Result<(), Errno> {
    let result = azalia_comresp(codec, nid, CORB_GET_CONFIGURATION_DEFAULT, 0)?;
    {
        let pin = codec.wi_mut(nid).d.pin_mut();
        pin.config = result;
        pin.sequence = corb_cd_sequence(result);
        pin.association = corb_cd_association(result);
        pin.color = corb_cd_color(result);
        pin.device = corb_cd_device(result);
    }

    let result = azalia_comresp(codec, nid, CORB_GET_PARAMETER, COP_PINCAP)?;
    codec.wi_mut(nid).d.pin_mut().cap = result;
    let pin = *codec.wi(nid).d.pin();

    let mut dir = CORB_PWC_INPUT;
    if matches!(
        pin.device,
        CORB_CD_LINEOUT
            | CORB_CD_SPEAKER
            | CORB_CD_HEADPHONE
            | CORB_CD_SPDIFOUT
            | CORB_CD_DIGITALOUT
    ) {
        dir = CORB_PWC_OUTPUT;
    }

    if dir == CORB_PWC_INPUT && pin.cap & COP_PINCAP_INPUT == 0 {
        dir = CORB_PWC_OUTPUT;
    }
    if dir == CORB_PWC_OUTPUT && pin.cap & COP_PINCAP_OUTPUT == 0 {
        dir = CORB_PWC_INPUT;
    }

    if dir == CORB_PWC_INPUT && pin.device == CORB_CD_MICIN {
        if cop_pincap_vref(pin.cap) & (1 << CORB_PWC_VREF_80) != 0 {
            dir |= CORB_PWC_VREF_80;
        } else if cop_pincap_vref(pin.cap) & (1 << CORB_PWC_VREF_50) != 0 {
            dir |= CORB_PWC_VREF_50;
        }
    }

    if codec.qrks & AZ_QRK_WID_OVREF50 != 0 && dir == CORB_PWC_OUTPUT {
        dir |= CORB_PWC_VREF_50;
    }

    let _ = azalia_comresp(codec, nid, CORB_SET_PIN_WIDGET_CONTROL, dir);

    if pin.cap & COP_PINCAP_EAPD != 0 {
        let mut result = azalia_comresp(codec, nid, CORB_GET_EAPD_BTL_ENABLE, 0)?;
        result &= 0xff;
        result |= CORB_EAPD_EAPD;
        azalia_comresp(codec, nid, CORB_SET_EAPD_BTL_ENABLE, result)?;
    }

    // Disable unconnected pins
    if corb_cd_port(pin.config) == CORB_CD_NONE {
        codec.wi_mut(nid).enable = false;
    }

    Ok(())
}

/// `azalia_widget_init_connection`: the connection list of widget `nid` and the connection
/// it has selected.
pub fn azalia_widget_init_connection(codec: &mut Codec, nid: NidT) -> Result<(), Errno> {
    codec.wi_mut(nid).selected = -1;
    if codec.wi(nid).widgetcap & COP_AWCAP_CONNLIST == 0 {
        return Ok(());
    }

    let result = azalia_comresp(codec, nid, CORB_GET_PARAMETER, COP_CONNECTION_LIST_LENGTH)?;

    let bits: u32 = if result & COP_CLL_LONG != 0 { 16 } else { 8 };
    let hibit: i32 = 1 << (bits - 1);
    let mask: u32 = (1 << bits) - 1;

    let length = cop_cll_length(result) as i32;
    if length == 0 {
        return Ok(());
    }

    // 'length' is the number of entries, not the number of connections. Find the number of
    // connections, 'nconn', so enough space can be allocated for the list of connected
    // nids.
    let mut nconn: i32 = 0;
    let mut last: i32 = 0;
    let mut i = 0;
    while i < length {
        let result = azalia_comresp(codec, nid, CORB_GET_CONNECTION_LIST_ENTRY, i as u32)?;
        let mut k = 0;
        while i < length && k < 32 / bits {
            let conn = ((result >> (k * bits)) & mask) as i32;
            // If high bit is set, this is the end of a continuous list that started with
            // the last connection.
            if nconn > 0 && conn & hibit != 0 {
                nconn += (conn & !hibit) - last;
            } else {
                nconn += 1;
            }
            last = conn;
            i += 1;
            k += 1;
        }
    }

    let mut connections: Vec<NidT> = Vec::new();
    let reserved = usize::try_from(nconn)
        .ok()
        .is_some_and(|n| connections.try_reserve_exact(n).is_ok());
    if !reserved {
        printf(format_args!("{}: out of memory\n", codec.az().dev.xname()));
        return Err(Errno::ENOMEM);
    }
    let mut i = 0;
    while i < nconn {
        let result = azalia_comresp(codec, nid, CORB_GET_CONNECTION_LIST_ENTRY, i as u32)?;
        let mut k = 0;
        while i < nconn && k < 32 / bits {
            let conn = ((result >> (k * bits)) & mask) as i32;
            // If high bit is set, this is the end of a continuous list that started with
            // the last connection.
            if i > 0 && conn & hibit != 0 {
                let mut j = 1;
                while i < nconn && j <= conn - last {
                    connections.push(last + j);
                    i += 1;
                    j += 1;
                }
            } else {
                connections.push(conn);
                i += 1;
            }
            last = conn;
            k += 1;
        }
    }
    codec.wi_mut(nid).connections = connections;

    if nconn > 0 {
        let result = azalia_comresp(codec, nid, CORB_GET_CONNECTION_SELECT_CONTROL, 0)?;
        codec.wi_mut(nid).selected = corb_csc_index(result) as i32;
    }
    Ok(())
}

/// `azalia_widget_check_conn`: whether mixer or selector `index` connects, within ten
/// hops, to an enabled pin or converter.
pub fn azalia_widget_check_conn(codec: &Codec, index: i32, mut depth: i32) -> bool {
    let w = codec.wi(index);

    if w.type_ == COP_AWTYPE_BEEP_GENERATOR {
        return false;
    }

    if depth > 0
        && (w.type_ == COP_AWTYPE_PIN_COMPLEX
            || w.type_ == COP_AWTYPE_AUDIO_OUTPUT
            || w.type_ == COP_AWTYPE_AUDIO_INPUT)
    {
        return w.enable;
    }
    depth += 1;
    if depth >= 10 {
        return false;
    }
    for &c in &w.connections {
        if !azalia_widget_enabled(codec, c) {
            continue;
        }
        if azalia_widget_check_conn(codec, c, depth) {
            return true;
        }
    }
    false
}

/// `azalia_codec_print_audiofunc`: empty, AZALIA_DEBUG not being configured.
pub fn azalia_codec_print_audiofunc(_this: &Codec) {}

/// `azalia_codec_print_groups`: empty, AZALIA_DEBUG not being configured.
pub fn azalia_codec_print_groups(_this: &Codec) {}

/// `azalia_widget_print_audio`: empty, AZALIA_DEBUG not being configured.
pub fn azalia_widget_print_audio(_this: &Widget, _lead: &str) {}

/// `azalia_widget_print_widget`: empty, AZALIA_DEBUG not being configured.
pub fn azalia_widget_print_widget(_w: &Widget, _codec: &Codec) {}

/// `azalia_widget_print_pin`: empty, AZALIA_DEBUG not being configured.
pub fn azalia_widget_print_pin(_this: &Widget) {}

// ================================================================
// Stream functions
// ================================================================

/// `azalia_stream_init`: stream number `strnum` on descriptor `regindex`, and its BDL.
pub fn azalia_stream_init(
    this: &Stream,
    az: &AzaliaSoftc,
    regindex: i32,
    strnum: i32,
    dir: i32,
) -> Result<(), Errno> {
    this.az.set(az);
    this.regbase
        .set(HDA_SD_BASE + regindex as usize * HDA_SD_SIZE);
    this.intr_bit.set(1 << regindex);
    this.number.set(strnum);
    this.dir.set(dir);

    // setup BDL buffers
    if let Err(err) = azalia_alloc_dmamem(
        az,
        size_of::<BdlistEntry>() * HDA_BDL_MAX,
        128,
        &this.bdlist,
    ) {
        printf(format_args!(
            "{}: can't allocate a BDL buffer\n",
            az.dev.xname()
        ));
        return Err(err);
    }
    Ok(())
}

/// `azalia_stream_reset`.
pub fn azalia_stream_reset(this: &Stream) -> Result<(), Errno> {
    // Make sure RUN bit is zero before resetting
    let mut ctl = this.str_read_2(HDA_SD_CTL);
    ctl &= !HDA_SD_CTL_RUN;
    this.str_write_2(HDA_SD_CTL, ctl);
    delay(40);

    // Start reset and wait for chip to enter.
    let ctl = this.str_read_2(HDA_SD_CTL);
    this.str_write_2(HDA_SD_CTL, ctl | HDA_SD_CTL_SRST);
    let mut ctl = 0;
    if !azalia_poll(|| {
        ctl = this.str_read_2(HDA_SD_CTL);
        ctl & HDA_SD_CTL_SRST != 0
    }) {
        // stream reset failure 1
        return Err(Errno::EIO);
    }

    // Clear reset and wait for chip to finish
    this.str_write_2(HDA_SD_CTL, ctl & !HDA_SD_CTL_SRST);
    if !azalia_poll(|| this.str_read_2(HDA_SD_CTL) & HDA_SD_CTL_SRST == 0) {
        // stream reset failure 2
        return Err(Errno::EIO);
    }

    let mut sts = this.str_read_1(HDA_SD_STS);
    sts |= HDA_SD_STS_DESE | HDA_SD_STS_FIFOE | HDA_SD_STS_BCIS;
    this.str_write_1(HDA_SD_STS, sts);

    Ok(())
}

/// `azalia_stream_start`: fill the BDL with the ring's blocks, program the descriptor and
/// the converters, and run.
pub fn azalia_stream_start(this: &Stream) -> Result<(), Errno> {
    azalia_stream_reset(this)?;

    this.str_write_4(HDA_SD_BDPL, 0);
    this.str_write_4(HDA_SD_BDPU, 0);

    // setup BDL
    let mut dmaaddr = this.buffer.get().dmaaddr() as u64;
    let dmaend = dmaaddr + this.bufsize.get() as u64;
    let bdlist = this.bdlist.get().addr.cast::<BdlistEntry>();
    let blk = this.blk.get();
    let mut index = 0;
    while index < HDA_BDL_MAX {
        let entry = BdlistEntry {
            low: htole32(dmaaddr as u32),
            high: htole32(ptr_upper32(dmaaddr)),
            length: htole32(blk as u32),
            flags: htole32(BDLIST_ENTRY_IOC),
        };
        // SAFETY: `azalia_stream_init` allocated `HDA_BDL_MAX` entries at `bdlist` and
        // `index < HDA_BDL_MAX`; the controller reads them, hence the volatile store.
        unsafe { bdlist.add(index).write_volatile(entry) };
        dmaaddr += blk as u64;
        index += 1;
        if dmaaddr >= dmaend {
            break;
        }
    }

    let dmaaddr = this.bdlist.get().dmaaddr() as u64;
    this.str_write_4(HDA_SD_BDPL, dmaaddr as u32);
    this.str_write_4(HDA_SD_BDPU, ptr_upper32(dmaaddr));
    this.str_write_2(HDA_SD_LVI, (index as u16).wrapping_sub(1) & HDA_SD_LVI_LVI);
    let ctl2 = this.str_read_1(HDA_SD_CTL2);
    this.str_write_1(
        HDA_SD_CTL2,
        (ctl2 & !HDA_SD_CTL2_STRM) | ((this.number.get() as u8) << HDA_SD_CTL2_STRM_SHIFT),
    );
    this.str_write_4(HDA_SD_CBL, this.bufsize.get() as u32);
    this.str_write_2(HDA_SD_FMT, this.fmt.get());

    if azalia_codec_connect_stream(this).is_err() {
        return Err(Errno::EINVAL);
    }

    let az = this.az();
    let bit = this.intr_bit.get();
    let intctl = az.intctl.fetch_or(bit, Ordering::Relaxed) | bit;
    az.az_write_4(HDA_INTCTL, intctl);

    this.str_write_1(
        HDA_SD_CTL,
        this.str_read_1(HDA_SD_CTL)
            | (HDA_SD_CTL_DEIE | HDA_SD_CTL_FEIE | HDA_SD_CTL_IOCE | HDA_SD_CTL_RUN) as u8,
    );
    Ok(())
}

/// `azalia_stream_halt`.
pub fn azalia_stream_halt(this: &Stream) -> Result<(), Errno> {
    let mut ctl = this.str_read_2(HDA_SD_CTL);
    ctl &= !(HDA_SD_CTL_DEIE | HDA_SD_CTL_FEIE | HDA_SD_CTL_IOCE | HDA_SD_CTL_RUN);
    this.str_write_2(HDA_SD_CTL, ctl);
    let az = this.az();
    let bit = this.intr_bit.get();
    let intctl = az.intctl.fetch_and(!bit, Ordering::Relaxed) & !bit;
    az.az_write_4(HDA_INTCTL, intctl);
    let _ = azalia_codec_disconnect_stream(this);

    Ok(())
}

/// `azalia_stream_intr`: one call-back to audio(4) per block the controller has moved
/// past. Called with `audio_lock` held.
pub fn azalia_stream_intr(this: &Stream) -> i32 {
    let sts = this.str_read_1(HDA_SD_STS);
    this.str_write_1(
        HDA_SD_STS,
        sts | HDA_SD_STS_DESE | HDA_SD_STS_FIFOE | HDA_SD_STS_BCIS,
    );

    if sts & HDA_SD_STS_BCIS != 0 {
        let lpib = this.str_read_4(HDA_SD_LPIB);
        let mut fifos = u32::from(this.str_read_2(HDA_SD_FIFOS));
        if fifos & 1 != 0 {
            fifos += 1;
        }
        let mut hwpos = lpib;
        if this.dir.get() == AUMODE_PLAY {
            hwpos = hwpos.wrapping_add(fifos + 1);
        }
        let bufsize = this.bufsize.get() as u32;
        if hwpos >= bufsize {
            hwpos = hwpos.wrapping_sub(bufsize);
        }
        let blk = this.blk.get() as u32;
        while hwpos.wrapping_sub(this.swpos.get()) >= blk {
            if let Some(intr) = this.intr.get() {
                // SAFETY: the call-back and argument audio(4) handed to `trigger_*`;
                // `audio_lock` is held, as audio(4) requires.
                unsafe { intr(this.intr_arg.get()) };
            }
            let mut swpos = this.swpos.get().wrapping_add(blk);
            if swpos == bufsize {
                swpos = 0;
            }
            this.swpos.set(swpos);
        }
    }
    1
}

// ================================================================
// MI audio entries
// ================================================================

/// The softc behind the handle audio(4) passes to the `audio_hw_if` methods. A port
/// helper.
///
/// # Safety
///
/// `v` is the handle given to `audio_attach_mi`: an attached `AzaliaSoftc`.
unsafe fn azalia_hdl<'a>(v: *mut c_void) -> &'a AzaliaSoftc {
    // SAFETY: the caller's contract.
    unsafe { &*v.cast::<AzaliaSoftc>() }
}

/// `azalia_open`.
///
/// # Safety
///
/// `v` is the handle given to `audio_attach_mi`; called by audio(4) under the kernel lock.
pub unsafe fn azalia_open(v: *mut c_void, flags: i32) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let az = unsafe { azalia_hdl(v) };
    // SAFETY: audio(4)'s entry points run under the kernel lock; nothing else holds the
    // codec.
    let codec = unsafe { az.cur_codec() };
    if flags & FWRITE != 0 && codec.dacs.ngroups == 0 {
        return Err(Errno::ENODEV);
    }
    if flags & FREAD != 0 && codec.adcs.ngroups == 0 {
        return Err(Errno::ENODEV);
    }
    codec.running += 1;
    Ok(())
}

/// `azalia_close`.
///
/// # Safety
///
/// As for [`azalia_open`].
pub unsafe fn azalia_close(v: *mut c_void) {
    // SAFETY: the caller's contract.
    let az = unsafe { azalia_hdl(v) };
    // SAFETY: as in `azalia_open`.
    let codec = unsafe { az.cur_codec() };
    codec.running -= 1;
}

/// `azalia_match_format`: the index of the codec's format with the mode, encoding,
/// precision and channels of `par`; `nformats` if there is none.
pub fn azalia_match_format(codec: &Codec, mode: i32, par: &AudioParams) -> i32 {
    codec
        .formats
        .iter()
        .position(|f| {
            mode == f.mode
                && par.encoding == f.encoding
                && par.precision == f.precision
                && par.channels == f.channels
        })
        .unwrap_or(codec.formats.len()) as i32
}

/// `azalia_set_params_sub`: bend `par` to a format and rate the codec has for `mode`.
pub fn azalia_set_params_sub(codec: &Codec, mode: i32, par: &mut AudioParams) -> Result<(), Errno> {
    let oenc = par.encoding;
    let opre = par.precision;
    let nformats = codec.nformats();

    if (mode == AUMODE_PLAY && codec.dacs.ngroups == 0)
        || (mode == AUMODE_RECORD && codec.adcs.ngroups == 0)
    {
        return Ok(());
    }

    let mut i = azalia_match_format(codec, mode, par);
    if i == nformats && (par.precision != 16 || par.encoding != AUDIO_ENCODING_SLINEAR_LE) {
        // try with default encoding/precision
        par.encoding = AUDIO_ENCODING_SLINEAR_LE;
        par.precision = 16;
        i = azalia_match_format(codec, mode, par);
    }
    if i == nformats && par.channels != 2 {
        // try with default channels
        par.encoding = oenc;
        par.precision = opre;
        par.channels = 2;
        i = azalia_match_format(codec, mode, par);
    }
    // try with default everything
    if i == nformats {
        par.encoding = AUDIO_ENCODING_SLINEAR_LE;
        par.precision = 16;
        par.channels = 2;
        i = azalia_match_format(codec, mode, par);
        if i == nformats {
            return Err(Errno::EINVAL);
        }
    }
    let f = &codec.formats[i as usize];
    if f.frequency_type == 0 {
        return Err(Errno::EINVAL);
    }

    let rates = &f.frequency[..f.frequency_type as usize];
    if !rates.iter().any(|&r| par.sample_rate == u64::from(r)) {
        // try again with default
        par.sample_rate = 48000;
        if !rates.iter().any(|&r| par.sample_rate == u64::from(r)) {
            return Err(Errno::EINVAL);
        }
    }
    par.bps = audio_bps(par.precision);
    par.msb = 1;

    Ok(())
}

/// `azalia_set_params`.
///
/// # Safety
///
/// As for [`azalia_open`].
pub unsafe fn azalia_set_params(
    v: *mut c_void,
    smode: i32,
    _umode: i32,
    p: &mut AudioParams,
    r: &mut AudioParams,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let az = unsafe { azalia_hdl(v) };
    // SAFETY: as in `azalia_open`.
    let codec: &Codec = unsafe { az.cur_codec() };
    if codec.formats.is_empty() {
        // codec has no formats
        return Err(Errno::EINVAL);
    }

    if smode & AUMODE_RECORD != 0 {
        azalia_set_params_sub(codec, AUMODE_RECORD, r)?;
    }

    if smode & AUMODE_PLAY != 0 {
        azalia_set_params_sub(codec, AUMODE_PLAY, p)?;
    }

    Ok(())
}

/// `azalia_set_blksz`: the block must be a multiple of 128 bytes.
///
/// # Safety
///
/// As for [`azalia_open`] (the handle is not used).
pub unsafe fn azalia_set_blksz(
    _v: *mut c_void,
    mode: i32,
    p: &mut AudioParams,
    r: &mut AudioParams,
    mut blksz: u32,
) -> u32 {
    // must be multiple of 128 bytes
    let mult = audio_blksz_bytes(mode, p, r, 128) as u32;

    blksz -= blksz % mult;
    if blksz == 0 {
        blksz = mult;
    }

    blksz
}

/// `azalia_set_nblks`: number of blocks must be <= HDA_BDL_MAX.
///
/// # Safety
///
/// As for [`azalia_open`] (the handle is not used).
pub unsafe fn azalia_set_nblks(
    _v: *mut c_void,
    _mode: i32,
    _params: &mut AudioParams,
    _blksz: u32,
    nblks: u32,
) -> u32 {
    nblks.min(HDA_BDL_MAX as u32)
}

/// `azalia_halt_output`.
///
/// # Safety
///
/// As for [`azalia_open`].
pub unsafe fn azalia_halt_output(v: *mut c_void) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let az = unsafe { azalia_hdl(v) };
    azalia_stream_halt(&az.pstream)
}

/// `azalia_halt_input`.
///
/// # Safety
///
/// As for [`azalia_open`].
pub unsafe fn azalia_halt_input(v: *mut c_void) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let az = unsafe { azalia_hdl(v) };
    azalia_stream_halt(&az.rstream)
}

/// `azalia_set_port`.
///
/// # Safety
///
/// As for [`azalia_open`].
pub unsafe fn azalia_set_port(v: *mut c_void, mc: &mut MixerCtrl) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let az = unsafe { azalia_hdl(v) };
    // SAFETY: as in `azalia_open`.
    let co = unsafe { az.cur_codec() };
    if mc.dev < 0 || mc.dev >= co.nmixers() {
        return Err(Errno::EINVAL);
    }

    let m = co.mixers[mc.dev as usize];
    if mc.type_ != m.devinfo.type_ {
        return Err(Errno::EINVAL);
    }

    azalia_mixer_set(co, m.nid, m.target, mc)
}

/// `azalia_get_port`.
///
/// # Safety
///
/// As for [`azalia_open`].
pub unsafe fn azalia_get_port(v: *mut c_void, mc: &mut MixerCtrl) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let az = unsafe { azalia_hdl(v) };
    // SAFETY: as in `azalia_open`.
    let co: &Codec = unsafe { az.cur_codec() };
    if mc.dev < 0 || mc.dev >= co.nmixers() {
        return Err(Errno::EINVAL);
    }

    let m = &co.mixers[mc.dev as usize];
    mc.type_ = m.devinfo.type_;

    azalia_mixer_get(co, m.nid, m.target, mc)
}

/// `azalia_query_devinfo`.
///
/// # Safety
///
/// As for [`azalia_open`].
pub unsafe fn azalia_query_devinfo(v: *mut c_void, mdev: &mut MixerDevinfo) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let az = unsafe { azalia_hdl(v) };
    // SAFETY: as in `azalia_open`.
    let co: &Codec = unsafe { az.cur_codec() };
    if mdev.index < 0 || mdev.index >= co.nmixers() {
        return Err(Errno::ENXIO);
    }
    *mdev = co.mixers[mdev.index as usize].devinfo;
    Ok(())
}

/// `azalia_allocm`: the DMA ring of direction `dir`.
///
/// # Safety
///
/// As for [`azalia_open`].
pub unsafe fn azalia_allocm(
    v: *mut c_void,
    dir: i32,
    size: usize,
    _pool: i32,
    _flags: i32,
) -> Option<NonNull<u8>> {
    // SAFETY: the caller's contract.
    let az = unsafe { azalia_hdl(v) };
    let stream = if dir == AUMODE_PLAY {
        &az.pstream
    } else {
        &az.rstream
    };
    if azalia_alloc_dmamem(az, size, 128, &stream.buffer).is_err() {
        printf(format_args!("{}: allocm failed\n", az.dev.xname()));
        return None;
    }
    NonNull::new(stream.buffer.get().addr)
}

/// `azalia_freem`.
///
/// # Safety
///
/// As for [`azalia_open`]; `addr` came from [`azalia_allocm`] and is no longer used.
pub unsafe fn azalia_freem(v: *mut c_void, addr: NonNull<u8>, _pool: i32) {
    // SAFETY: the caller's contract.
    let az = unsafe { azalia_hdl(v) };
    let stream = if addr.as_ptr() == az.pstream.buffer.get().addr {
        &az.pstream
    } else if addr.as_ptr() == az.rstream.buffer.get().addr {
        &az.rstream
    } else {
        return;
    };
    azalia_free_dmamem(az, &stream.buffer);
}

/// `azalia_round_buffersize`: must be multiple of 128.
///
/// # Safety
///
/// As for [`azalia_open`] (the handle is not used).
pub unsafe fn azalia_round_buffersize(_v: *mut c_void, _dir: i32, mut size: usize) -> usize {
    size &= !0x7f; // must be multiple of 128
    if size == 0 {
        size = 128;
    }
    size
}

/// The body of `azalia_trigger_output` and `azalia_trigger_input` once the converter group
/// is checked. A port helper.
fn azalia_trigger(
    stream: &Stream,
    start: *mut u8,
    end: *mut u8,
    blk: i32,
    intr: AudioIntr,
    arg: *mut c_void,
    param: &AudioParams,
) -> Result<(), Errno> {
    let Ok(fmt) = azalia_params2fmt(param) else {
        return Err(Errno::EINVAL);
    };

    stream
        .bufsize
        .set((end as usize).wrapping_sub(start as usize) as i32);
    stream.blk.set(blk);
    stream.fmt.set(fmt);
    stream.intr.set(Some(intr));
    stream.intr_arg.set(arg);
    stream.swpos.set(0);

    azalia_stream_start(stream)
}

/// `azalia_trigger_output`.
///
/// # Safety
///
/// As for [`azalia_open`]; `start..end` is the ring [`azalia_allocm`] returned.
pub unsafe fn azalia_trigger_output(
    v: *mut c_void,
    start: *mut u8,
    end: *mut u8,
    blk: i32,
    intr: AudioIntr,
    arg: *mut c_void,
    param: &AudioParams,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let az = unsafe { azalia_hdl(v) };

    // SAFETY: as in `azalia_open`.
    if unsafe { az.cur_codec() }.dacs.ngroups == 0 {
        // can't play without a DAC
        return Err(Errno::ENXIO);
    }

    azalia_trigger(&az.pstream, start, end, blk, intr, arg, param)
}

/// `azalia_trigger_input`.
///
/// # Safety
///
/// As for [`azalia_trigger_output`].
pub unsafe fn azalia_trigger_input(
    v: *mut c_void,
    start: *mut u8,
    end: *mut u8,
    blk: i32,
    intr: AudioIntr,
    arg: *mut c_void,
    param: &AudioParams,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let az = unsafe { azalia_hdl(v) };

    // SAFETY: as in `azalia_open`.
    if unsafe { az.cur_codec() }.adcs.ngroups == 0 {
        // can't record without an ADC
        return Err(Errno::ENXIO);
    }

    azalia_trigger(&az.rstream, start, end, blk, intr, arg, param)
}

// --------------------------------
// helpers for MI audio functions
// --------------------------------

/// `azalia_params2fmt`: the stream format register value of `param`.
pub fn azalia_params2fmt(param: &AudioParams) -> Result<u16, Errno> {
    let mut ret: u16 = 0;
    if param.channels > HDA_MAX_CHANNELS as u32 {
        printf(format_args!(
            "azalia_params2fmt: too many channels: {}\n",
            param.channels
        ));
        return Err(Errno::EINVAL);
    }

    // XXX: can channels be >2 ?
    ret |= param.channels.wrapping_sub(1) as u16;

    match param.precision {
        8 => ret |= HDA_SD_FMT_BITS_8_16,
        16 => ret |= HDA_SD_FMT_BITS_16_16,
        20 => ret |= HDA_SD_FMT_BITS_20_32,
        24 => ret |= HDA_SD_FMT_BITS_24_32,
        32 => ret |= HDA_SD_FMT_BITS_32_32,
        _ => {}
    }

    ret |= match param.sample_rate {
        192000 => HDA_SD_FMT_BASE_48 | HDA_SD_FMT_MULT_X4 | HDA_SD_FMT_DIV_BY1,
        176400 => HDA_SD_FMT_BASE_44 | HDA_SD_FMT_MULT_X4 | HDA_SD_FMT_DIV_BY1,
        96000 => HDA_SD_FMT_BASE_48 | HDA_SD_FMT_MULT_X2 | HDA_SD_FMT_DIV_BY1,
        88200 => HDA_SD_FMT_BASE_44 | HDA_SD_FMT_MULT_X2 | HDA_SD_FMT_DIV_BY1,
        48000 => HDA_SD_FMT_BASE_48 | HDA_SD_FMT_MULT_X1 | HDA_SD_FMT_DIV_BY1,
        44100 => HDA_SD_FMT_BASE_44 | HDA_SD_FMT_MULT_X1 | HDA_SD_FMT_DIV_BY1,
        32000 => HDA_SD_FMT_BASE_48 | HDA_SD_FMT_MULT_X2 | HDA_SD_FMT_DIV_BY3,
        22050 => HDA_SD_FMT_BASE_44 | HDA_SD_FMT_MULT_X1 | HDA_SD_FMT_DIV_BY2,
        16000 => HDA_SD_FMT_BASE_48 | HDA_SD_FMT_MULT_X1 | HDA_SD_FMT_DIV_BY3,
        11025 => HDA_SD_FMT_BASE_44 | HDA_SD_FMT_MULT_X1 | HDA_SD_FMT_DIV_BY4,
        8000 => HDA_SD_FMT_BASE_48 | HDA_SD_FMT_MULT_X1 | HDA_SD_FMT_DIV_BY6,
        // default, and 384000
        rate => {
            printf(format_args!(
                "azalia_params2fmt: invalid sample_rate: {rate}\n"
            ));
            return Err(Errno::EINVAL);
        }
    };
    Ok(ret)
}

#[cfg(test)]
pub(crate) mod tests;
