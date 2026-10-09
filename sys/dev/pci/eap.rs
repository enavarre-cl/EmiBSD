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
 * Copyright (c) 1998, 1999 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Lennart Augustsson <augustss@netbsd.org> and Charles M. Hannum.
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

/*
 * Debugging:   Andreas Gustafsson <gson@araneus.fi>
 * Testing:     Chuck Cranor       <chuck@maria.wustl.edu>
 *              Phil Nelson        <phil@cs.wwu.edu>
 *
 * ES1371/AC97:	Ezra Story         <ezy@panix.com>
 */
/* </LICENSES> */

/* <CODE> */
//! `eap(4)`: the Ensoniq AudioPCI ES1370 with its AK4531 codec, and the ES1371/ES1373,
//! CT5880 and Ectiva EV1938 with an AC97 codec (`eap* at pci?`), with the chip's MIDI UART
//! for midi(4).
//!
//! Upstream: sys/dev/pci/eap.c @ 3ce1f3f79392
//!
//! Ensoniq ES1370 + AK4531 and ES1371/ES1373 + AC97.
//!
//! Documentation links:
//!
//! - <ftp://ftp.alsa-project.org/pub/manuals/ensoniq/>
//! - <ftp://ftp.alsa-project.org/pub/manuals/asahi_kasei/4531.pdf>
//!
//! Attach maps the I/O BAR, establishes the interrupt at `IPL_AUDIO | IPL_MPSAFE` and sets
//! the chip up: on the ES1370 (QEMU's `ES1370`) it enables the codec and the DAC2/ADC
//! interrupts, resets the AK4531 and sets its mixer to the driver's defaults through its
//! own `set_port`; on the ES1371 family it programs the sample rate converter to resample
//! 48 kHz to 48 kHz and attaches the AC97 codec through ac97(4). It then registers the
//! hardware interface with audio(4) ([`audio_attach_mi`]) and the UART with midi(4)
//! ([`midi_attach_mi`]). Playback runs on DAC2 and recording on the ADC: `trigger_output`
//! and `trigger_input` point the channel's page at the ring and program the block size,
//! and each block interrupt calls audio(4)'s `audio_pintr`/`audio_rintr`.
//!
//! ## Deviations
//! - `AUDIO_DEBUG` is not configured: the `DPRINTF`s are not carried over. `NMIDI` is
//!   greater than 0 (`midi* at eap?` is in amd64's GENERIC), so the MIDI parts are always
//!   built.
//! - The softc and the DMA bookkeeping (`struct eap_dma`, a list from `sc_dmas`) are made
//!   of `Cell`s, all valid as zero bits (the device is allocated zeroed); `eap_malloc`
//!   allocates its `struct eap_dma` with `M_ZERO` added, so the list node starts valid.
//!   The softc is reached as `&'static` (never freed while the device exists), which also
//!   gives the interrupt its `&'static str` name.
//! - `sc_prun`/`sc_rrun` exist whatever the options; only the `diagnostic` feature checks
//!   them, as only DIAGNOSTIC kernels have them in the C.
//! - The AC97 host methods return `Ok(())` where the C returns 0; `eap_set_params`'s
//!   `ULINEAR` cases return `EINVAL` at once: in the C their precision check falls through
//!   to `default:`, which returns `EINVAL` anyway.
//! - The device has no `ca_detach` (the C has none either).

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};

use libkern::strlcpy;

use crate::dev::audio::{AUDIO_LOCK, audio_attach_mi};
use crate::dev::audio_if::{
    AUDIO_ENCODING_SLINEAR_LE, AUDIO_ENCODING_ULINEAR_BE, AUDIO_ENCODING_ULINEAR_LE, AudioHwIf,
    AudioIntr, AudioParams, audio_bps,
};
use crate::dev::ic::ac97::{
    AC97_HOST_DONT_READ, Ac97CodecIf, Ac97HostIf, ac97_attach, ac97_resume,
};
use crate::dev::midi::midi_attach_mi;
use crate::dev::midi_if::{
    MIDI_PROP_CAN_INPUT, MIDI_PROP_OUT_INTR, MidiHwIf, MidiIintr, MidiInfo, MidiOintr,
};
use crate::dev::pci::eapreg::*;
use crate::dev::pci::pci::pci_matchbyid;
use crate::dev::pci::pci_map::pci_mapreg_map;
use crate::dev::pci::pcidevs::{
    PCI_PRODUCT_CREATIVELABS_EV1938, PCI_PRODUCT_ENSONIQ_AUDIOPCI, PCI_PRODUCT_ENSONIQ_AUDIOPCI97,
    PCI_PRODUCT_ENSONIQ_CT5880, PCI_VENDOR_CREATIVELABS, PCI_VENDOR_ENSONIQ,
};
use crate::dev::pci::pcireg::{
    PCI_MAPREG_TYPE_IO, PciProductId, PciVendorId, pci_product, pci_revision, pci_vendor,
};
use crate::dev::pci::pcivar::{PciAttachArgs, PciMatchid};
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_synch::tsleep_nsec;
use crate::kern::subr_autoconf::config_activate_children;
use crate::kern::subr_prf::{panic, printf};
use crate::machine::bus::{
    BUS_DMA_COHERENT, BUS_DMA_NOWAIT, BusDmaSegment, BusDmaTag, BusDmamap, BusSpaceHandle,
    BusSpaceTag, bus_dmamap_create, bus_dmamap_destroy, bus_dmamap_load, bus_dmamap_unload,
    bus_dmamem_alloc, bus_dmamem_free, bus_dmamem_map, bus_dmamem_unmap, bus_space_read_1,
    bus_space_read_4, bus_space_write_1, bus_space_write_4,
};
use crate::machine::cpu::delay;
use crate::machine::intr::{IPL_AUDIO, IPL_MPSAFE};
use crate::machine::pci_machdep::{pci_intr_establish, pci_intr_map, pci_intr_string};
use crate::sys::audioio::{
    AUDIO_MIXER_CLASS, AUDIO_MIXER_ENUM, AUDIO_MIXER_LAST, AUDIO_MIXER_LEVEL_LEFT,
    AUDIO_MIXER_LEVEL_MONO, AUDIO_MIXER_LEVEL_RIGHT, AUDIO_MIXER_SET, AUDIO_MIXER_VALUE,
    AUMODE_PLAY, AUMODE_RECORD, AudioCinputs, AudioCoutputs, AudioCrecord, AudioNaux, AudioNcd,
    AudioNdac, AudioNfmsynth, AudioNline, AudioNmaster, AudioNmicrophone, AudioNoff, AudioNon,
    AudioNpreamp, AudioNsource, AudioNvolume, MixerCtrl, MixerDevinfo,
};
use crate::sys::device::{CfMatch, Cfattach, Cfdriver, DV_DULL, DVACT_RESUME, Device, Softc};
use crate::sys::errno::Errno;
use crate::sys::fcntl::FREAD;
use crate::sys::malloc::M_ZERO;
use crate::sys::param::PWAIT;
use crate::sys::time::msec_to_nsec;

/// `PCI_CBIO`: the I/O BAR.
const PCI_CBIO: i32 = 0x10;

/// The number of AK4531 registers `sc_port` mirrors.
const NPORTS: usize = AK_NPORTS as usize;

/// `struct eap_dma`: the DMA memory of a ring; the softc keeps them in a list.
#[repr(C)]
pub struct EapDma {
    /// `map`.
    map: Cell<Option<&'static BusDmamap>>,
    /// `addr`.
    addr: Cell<*mut u8>,
    /// `segs[1]`.
    segs: [Cell<BusDmaSegment>; 1],
    /// `nsegs`.
    nsegs: Cell<usize>,
    /// `size`.
    size: Cell<usize>,
    /// `next`.
    next: Cell<*mut EapDma>,
}

impl EapDma {
    /// `DMAADDR(p)`: the ring's bus address, as the chip takes it.
    fn dmaaddr(&self) -> u32 {
        match self.map.get() {
            Some(map) => map.dm_segs().first().map_or(0, |s| s.get().ds_addr) as u32,
            None => panic(format_args!("eap: DMA memory without a map")),
        }
    }
}

/// `struct eap_softc`. Allocated zeroed by autoconf, so every member is valid as zero.
#[repr(C)]
pub struct EapSoftc {
    /// `sc_dev`: base device.
    sc_dev: Device,
    /// `sc_ih`: interrupt vectoring.
    sc_ih: Cell<Option<NonNull<c_void>>>,
    /// `iot`.
    iot: Cell<Option<BusSpaceTag>>,
    /// `ioh`.
    ioh: Cell<Option<BusSpaceHandle>>,
    /// `sc_dmatag`: DMA tag.
    sc_dmatag: Cell<Option<BusDmaTag>>,

    /// `sc_dmas`.
    sc_dmas: Cell<*mut EapDma>,

    /// `sc_pintr`: dma completion intr handler.
    sc_pintr: Cell<Option<AudioIntr>>,
    /// `sc_parg`: arg for sc_intr().
    sc_parg: Cell<*mut c_void>,
    /// `sc_prun` (DIAGNOSTIC).
    sc_prun: Cell<bool>,

    /// `sc_rintr`: dma completion intr handler.
    sc_rintr: Cell<Option<AudioIntr>>,
    /// `sc_rarg`: arg for sc_intr().
    sc_rarg: Cell<*mut c_void>,
    /// `sc_rrun` (DIAGNOSTIC).
    sc_rrun: Cell<bool>,

    /// `sc_iintr`: midi input ready handler.
    sc_iintr: Cell<Option<MidiIintr>>,
    /// `sc_ointr`: midi output ready handler.
    sc_ointr: Cell<Option<MidiOintr>>,
    /// `sc_arg`.
    sc_arg: Cell<*mut c_void>,
    /// `sc_uctrl`.
    sc_uctrl: Cell<u8>,
    /// `sc_mididev`.
    sc_mididev: Cell<Option<NonNull<Device>>>,

    /// `sc_port`: mirror of the hardware setting.
    sc_port: [Cell<u16>; NPORTS],
    /// `sc_record_source`: recording source mask.
    sc_record_source: Cell<u32>,
    /// `sc_input_source`: input source mask.
    sc_input_source: Cell<u32>,
    /// `sc_mic_preamp`.
    sc_mic_preamp: Cell<u32>,
    /// `sc_1371`: Using ES1371/AC97 codec.
    sc_1371: Cell<bool>,
    /// `sc_ct5880`: CT5880 chip.
    sc_ct5880: Cell<bool>,

    /// `codec_if`.
    codec_if: Cell<*const Ac97CodecIf>,
    /// `host_if`.
    host_if: Cell<Ac97HostIf>,

    /// `flags`.
    flags: Cell<u32>,
}

// SAFETY: `#[repr(C)]` with the device first; the other members are `Cell`s of integers,
// booleans, pointers, `Option`s of handles, references and function pointers, and the
// AC97 host interface (a pointer and `Option`s of function pointers), all valid as zero
// bits.
unsafe impl Softc for EapSoftc {}

impl EapSoftc {
    /// `sc->iot`, `sc->ioh`.
    fn io(&self) -> (BusSpaceTag, BusSpaceHandle) {
        match (self.iot.get(), self.ioh.get()) {
            (Some(t), Some(h)) => (t, h),
            _ => panic(format_args!("eap: registers not mapped")),
        }
    }

    /// `sc->sc_dmatag`.
    fn dmat(&self) -> BusDmaTag {
        match self.sc_dmatag.get() {
            Some(t) => t,
            None => panic(format_args!("eap: no DMA tag")),
        }
    }

    /// `sc->codec_if`.
    fn codec(&self) -> &Ac97CodecIf {
        let p = self.codec_if.get();
        if p.is_null() {
            panic(format_args!("eap: no codec interface"));
        }
        // SAFETY: `eap1371_attach_codec` stored the interface of the codec's softc, which
        // ac97_attach never frees once it succeeded; the ES1371 hardware interface is only
        // registered after that.
        unsafe { &*p }
    }

    /// `sc->sc_dev.dv_xname`.
    fn xname(&self) -> &str {
        self.sc_dev.xname()
    }

    /// The softc as the handle audio(4), midi(4), ac97(4) and the interrupt get.
    fn as_arg(&self) -> *mut c_void {
        ptr::from_ref(self).cast_mut().cast()
    }

    /// `EWRITE1(sc, r, x)`.
    fn write1(&self, r: usize, x: u8) {
        let (t, h) = self.io();
        bus_space_write_1(t, h, r, x);
    }

    /// `EWRITE4(sc, r, x)`.
    fn write4(&self, r: usize, x: u32) {
        let (t, h) = self.io();
        bus_space_write_4(t, h, r, x);
    }

    /// `EREAD1(sc, r)`.
    fn read1(&self, r: usize) -> u8 {
        let (t, h) = self.io();
        bus_space_read_1(t, h, r)
    }

    /// `EREAD4(sc, r)`.
    fn read4(&self, r: usize) -> u32 {
        let (t, h) = self.io();
        bus_space_read_4(t, h, r)
    }
}

/// `eap_cd`.
pub static EAP_CD: Cfdriver = Cfdriver::new(b"eap", DV_DULL, 0);

/// `eap_ca`.
pub static EAP_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<EapSoftc>(),
    ca_match: Some(eap_match),
    ca_attach: eap_attach,
    ca_detach: None,
    ca_activate: Some(eap_activate),
};

/// `eap1370_hw_if`.
pub static EAP1370_HW_IF: AudioHwIf = AudioHwIf {
    open: Some(eap_open),
    close: Some(eap_close),
    set_params: Some(eap_set_params),
    round_blocksize: Some(eap_round_blocksize),
    halt_output: Some(eap_halt_output),
    halt_input: Some(eap_halt_input),
    set_port: Some(eap1370_mixer_set_port),
    get_port: Some(eap1370_mixer_get_port),
    query_devinfo: Some(eap1370_query_devinfo),
    allocm: Some(eap_malloc),
    freem: Some(eap_free),
    trigger_output: Some(eap_trigger_output),
    trigger_input: Some(eap_trigger_input),
    ..AudioHwIf::new()
};

/// `eap1371_hw_if`.
pub static EAP1371_HW_IF: AudioHwIf = AudioHwIf {
    open: Some(eap_open),
    close: Some(eap_close),
    set_params: Some(eap_set_params),
    round_blocksize: Some(eap_round_blocksize),
    halt_output: Some(eap_halt_output),
    halt_input: Some(eap_halt_input),
    set_port: Some(eap1371_mixer_set_port),
    get_port: Some(eap1371_mixer_get_port),
    query_devinfo: Some(eap1371_query_devinfo),
    allocm: Some(eap_malloc),
    freem: Some(eap_free),
    trigger_output: Some(eap_trigger_output),
    trigger_input: Some(eap_trigger_input),
    ..AudioHwIf::new()
};

/// `eap_midi_hw_if`.
pub static EAP_MIDI_HW_IF: MidiHwIf = MidiHwIf {
    open: Some(eap_midi_open),
    close: Some(eap_midi_close),
    output: Some(eap_midi_output),
    flush: None,
    getinfo: Some(eap_midi_getinfo),
    ioctl: None,
};

/// A `struct pci_matchid` entry.
const fn id(vendor: u32, product: u32) -> PciMatchid {
    PciMatchid {
        pm_vid: vendor as PciVendorId,
        pm_pid: product as PciProductId,
    }
}

/// `eap_devices[]`.
pub static EAP_DEVICES: [PciMatchid; 4] = [
    id(PCI_VENDOR_CREATIVELABS, PCI_PRODUCT_CREATIVELABS_EV1938),
    id(PCI_VENDOR_ENSONIQ, PCI_PRODUCT_ENSONIQ_AUDIOPCI),
    id(PCI_VENDOR_ENSONIQ, PCI_PRODUCT_ENSONIQ_AUDIOPCI97),
    id(PCI_VENDOR_ENSONIQ, PCI_PRODUCT_ENSONIQ_CT5880),
];

/// The softc of a handle audio(4), midi(4) or ac97(4) passes back.
///
/// # Safety
///
/// `hdl` is the softc given to `audio_attach_mi`, `midi_attach_mi`, ac97(4)'s host
/// interface or the interrupt, a live `EapSoftc` (softcs are never freed while their device
/// exists).
unsafe fn sc_of<'a>(hdl: *mut c_void) -> &'a EapSoftc {
    // SAFETY: the contract.
    unsafe { &*hdl.cast::<EapSoftc>() }
}

/// `eap_match`.
pub fn eap_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: pci hands its children pci_attach_args.
    let pa = unsafe { &*aux.cast::<PciAttachArgs>() };
    pci_matchbyid(pa, &EAP_DEVICES)
}

/// `eap_activate`.
pub fn eap_activate(self_: &Device, act: i32) -> Result<(), Errno> {
    // SAFETY: `self_` was made for `eap_ca`.
    let sc = unsafe { self_.softc::<EapSoftc>() };

    if act == DVACT_RESUME {
        eap_resume(sc);
    }
    config_activate_children(self_, act)
}

/// `eap1370_write_codec`: an AK4531 register write, once the codec is idle.
pub fn eap1370_write_codec(sc: &EapSoftc, a: i32, d: i32) {
    let mut to = EAP_WRITE_TIMEOUT;
    loop {
        let icss = sc.read4(EAP_ICSS);
        if to == 0 {
            printf(format_args!("{}: timeout writing to codec\n", sc.xname()));
            return;
        }
        to -= 1;
        if icss & EAP_CWRIP == 0 {
            break; // XXX could use CSTAT here
        }
    }
    sc.write4(EAP_CODEC, eap_set_codec(a, d));
}

// Reading and writing the CODEC is very convoluted.  This mimics the FreeBSD and Linux
// drivers.

/// `eap1371_ready_codec`.
fn eap1371_ready_codec(sc: &EapSoftc, _a: u8, wd: u32) {
    let mut to = 0;
    while to < EAP_WRITE_TIMEOUT {
        if sc.read4(E1371_CODEC) & E1371_CODEC_WIP == 0 {
            break;
        }
        delay(1);
        to += 1;
    }
    if to == EAP_WRITE_TIMEOUT {
        printf(format_args!(
            "{}: eap1371_ready_codec timeout 1\n",
            sc.xname()
        ));
    }

    mtx_enter(&AUDIO_LOCK);
    let src = eap1371_src_wait(sc) & E1371_SRC_CTLMASK;
    sc.write4(E1371_SRC, src | E1371_SRC_STATE_OK);

    let mut to = 0;
    while to < EAP_READ_TIMEOUT {
        let t = sc.read4(E1371_SRC);
        if t & E1371_SRC_STATE_MASK == 0 {
            break;
        }
        delay(1);
        to += 1;
    }
    if to == EAP_READ_TIMEOUT {
        printf(format_args!(
            "{}: eap1371_ready_codec timeout 2\n",
            sc.xname()
        ));
    }

    let mut to = 0;
    while to < EAP_READ_TIMEOUT {
        let t = sc.read4(E1371_SRC);
        if t & E1371_SRC_STATE_MASK == E1371_SRC_STATE_OK {
            break;
        }
        delay(1);
        to += 1;
    }
    if to == EAP_READ_TIMEOUT {
        printf(format_args!(
            "{}: eap1371_ready_codec timeout 3\n",
            sc.xname()
        ));
    }

    sc.write4(E1371_CODEC, wd);

    eap1371_src_wait(sc);
    sc.write4(E1371_SRC, src);

    mtx_leave(&AUDIO_LOCK);
}

/// `eap1371_read_codec`.
///
/// # Safety
///
/// `sc_` is the `arg` of the softc's AC97 host interface.
pub unsafe fn eap1371_read_codec(sc_: *mut c_void, a: u8, d: &mut u16) -> Result<(), Errno> {
    // SAFETY: the contract.
    let sc = unsafe { sc_of(sc_) };

    eap1371_ready_codec(sc, a, e1371_set_codec(a, 0) | E1371_CODEC_READ);

    let mut to = 0;
    while to < EAP_WRITE_TIMEOUT {
        if sc.read4(E1371_CODEC) & E1371_CODEC_WIP == 0 {
            break;
        }
        delay(1);
        to += 1;
    }
    if to == EAP_WRITE_TIMEOUT {
        printf(format_args!(
            "{}: eap1371_read_codec timeout 1\n",
            sc.xname()
        ));
    }

    let mut t = 0;
    let mut to = 0;
    while to < EAP_WRITE_TIMEOUT {
        t = sc.read4(E1371_CODEC);
        if t & E1371_CODEC_VALID != 0 {
            break;
        }
        delay(1);
        to += 1;
    }
    if to == EAP_WRITE_TIMEOUT {
        printf(format_args!(
            "{}: eap1371_read_codec timeout 2\n",
            sc.xname()
        ));
    }

    *d = t as u16;

    Ok(())
}

/// `eap1371_write_codec`.
///
/// # Safety
///
/// As [`eap1371_read_codec`].
pub unsafe fn eap1371_write_codec(sc_: *mut c_void, a: u8, d: u16) -> Result<(), Errno> {
    // SAFETY: the contract.
    let sc = unsafe { sc_of(sc_) };

    eap1371_ready_codec(sc, a, e1371_set_codec(a, d));

    Ok(())
}

/// `eap1371_src_wait`: the sample rate converter's register once it is not busy.
pub fn eap1371_src_wait(sc: &EapSoftc) -> u32 {
    let mut src = 0;

    for _ in 0..EAP_READ_TIMEOUT {
        src = sc.read4(E1371_SRC);
        if src & E1371_SRC_RBUSY == 0 {
            return src;
        }
        delay(1);
    }
    printf(format_args!("{}: eap1371_src_wait timeout\n", sc.xname()));
    src
}

/// `eap1371_src_write`: a write to the sample rate converter's RAM.
pub fn eap1371_src_write(sc: &EapSoftc, a: i32, d: i32) {
    let mut r = eap1371_src_wait(sc) & E1371_SRC_CTLMASK;
    r |= E1371_SRC_RAMWE | e1371_src_addr(a) | e1371_src_data(d);
    sc.write4(E1371_SRC, r);
}

/// The sample rate converter's settings that "resample" 48kHz to 48kHz, shared by
/// `eap_attach` and `eap_resume`.
fn eap1371_src_init(sc: &EapSoftc) {
    sc.write4(E1371_SRC, E1371_SRC_DISABLE);
    for i in 0..0x80 {
        eap1371_src_write(sc, i, 0);
    }
    eap1371_src_write(sc, ESRC_ADC + ESRC_TRUNC_N, esrc_set_n(16));
    eap1371_src_write(sc, ESRC_ADC + ESRC_IREGS, esrc_set_vfi(16));
    eap1371_src_write(sc, ESRC_ADC + ESRC_VFF, 0);
    eap1371_src_write(sc, ESRC_ADC_VOLL, esrc_set_adc_vol(16));
    eap1371_src_write(sc, ESRC_ADC_VOLR, esrc_set_adc_vol(16));
    eap1371_src_write(sc, ESRC_DAC1 + ESRC_TRUNC_N, esrc_set_n(16));
    eap1371_src_write(sc, ESRC_DAC1 + ESRC_IREGS, esrc_set_vfi(16));
    eap1371_src_write(sc, ESRC_DAC1 + ESRC_VFF, 0);
    eap1371_src_write(sc, ESRC_DAC1_VOLL, esrc_set_dac_voli(1));
    eap1371_src_write(sc, ESRC_DAC1_VOLR, esrc_set_dac_voli(1));
    eap1371_src_write(sc, ESRC_DAC2 + ESRC_IREGS, esrc_set_vfi(16));
    eap1371_src_write(sc, ESRC_DAC2 + ESRC_TRUNC_N, esrc_set_n(16));
    eap1371_src_write(sc, ESRC_DAC2 + ESRC_VFF, 0);
    eap1371_src_write(sc, ESRC_DAC2_VOLL, esrc_set_dac_voli(1));
    eap1371_src_write(sc, ESRC_DAC2_VOLR, esrc_set_dac_voli(1));
    sc.write4(E1371_SRC, 0);
}

/// The ES1370's codec bring-up, shared by `eap_attach` and `eap_resume`: enable
/// interrupts and looping mode, the parts we need, then reset the codec, set normal
/// operation and select the codec clocks.
fn eap1370_init(sc: &EapSoftc) {
    sc.write4(EAP_SIC, EAP_P2_INTR_EN | EAP_R1_INTR_EN);
    sc.write4(EAP_ICSC, EAP_CDC_EN);

    eap1370_write_codec(sc, AK_RESET, AK_PD);
    eap1370_write_codec(sc, AK_RESET, AK_PD | AK_NRST);
    eap1370_write_codec(sc, AK_CS, 0x0);
}

/// The ES1371's clean slate, shared by `eap_attach` and `eap_resume`.
fn eap1371_clean_slate(sc: &EapSoftc) {
    sc.write4(EAP_SIC, 0);
    sc.write4(EAP_ICSC, 0);
    sc.write4(E1371_LEGACY, 0);

    if sc.sc_ct5880.get() {
        sc.write4(EAP_ICSS, EAP_CT5880_AC97_RESET);
        // Let codec wake up
        delay(20000);
    }
}

/// `eap_attach`.
pub fn eap_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `self_` was made for `eap_ca`, whose softc is an `EapSoftc`; softcs are never
    // freed while the device exists, so it may be borrowed for 'static.
    let sc: &'static EapSoftc = unsafe { &*ptr::from_ref(self_.softc::<EapSoftc>()) };
    // SAFETY: pci hands its children pci_attach_args.
    let pa = unsafe { &*aux.cast::<PciAttachArgs>() };
    let pc = pa.pa_pc;

    // Flag if we're "creative"
    sc.sc_1371.set(
        !(pci_vendor(pa.pa_id) == PCI_VENDOR_ENSONIQ
            && pci_product(pa.pa_id) == PCI_PRODUCT_ENSONIQ_AUDIOPCI),
    );

    let revision = pci_revision(pa.pa_class);
    if sc.sc_1371.get()
        && pci_vendor(pa.pa_id) == PCI_VENDOR_ENSONIQ
        && ((pci_product(pa.pa_id) == PCI_PRODUCT_ENSONIQ_AUDIOPCI97
            && (revision == EAP_ES1373_8 || revision == EAP_CT5880_A))
            || pci_product(pa.pa_id) == PCI_PRODUCT_ENSONIQ_CT5880)
    {
        sc.sc_ct5880.set(true);
    }

    // Map I/O register
    let Ok((iot, ioh, _, _)) = pci_mapreg_map(pa, PCI_CBIO, PCI_MAPREG_TYPE_IO, 0, 0) else {
        return;
    };
    sc.iot.set(Some(iot));
    sc.ioh.set(Some(ioh));

    sc.sc_dmatag.set(Some(pa.pa_dmat));

    // Map and establish the interrupt.
    let Some(ih) = pci_intr_map(pa) else {
        printf(format_args!(": couldn't map interrupt\n"));
        return;
    };
    let intrstr = pci_intr_string(pc, ih);
    let ihc = pci_intr_establish(
        pc,
        ih,
        IPL_AUDIO | IPL_MPSAFE,
        eap_intr,
        sc.as_arg(),
        sc.xname(),
    );
    sc.sc_ih.set(ihc);
    if ihc.is_none() {
        printf(format_args!(": couldn't establish interrupt"));
        printf(format_args!(" at {intrstr}"));
        printf(format_args!("\n"));
        return;
    }
    printf(format_args!(": {intrstr}\n"));

    let eap_hw_if: &'static AudioHwIf = if !sc.sc_1371.get() {
        eap1370_init(sc);

        // Enable all relevant mixer switches.
        let mut ctl = MixerCtrl {
            dev: EAP_INPUT_SOURCE,
            type_: AUDIO_MIXER_SET,
            ..MixerCtrl::default()
        };
        ctl.un.set_mask(
            (1 << EAP_VOICE_VOL)
                | (1 << EAP_FM_VOL)
                | (1 << EAP_CD_VOL)
                | (1 << EAP_LINE_VOL)
                | (1 << EAP_AUX_VOL)
                | (1 << EAP_MIC_VOL),
        );
        let arg = sc.as_arg();
        // SAFETY: `arg` is this softc (and for each `set_port` below).
        let _ = unsafe { eap1370_mixer_set_port(arg, &mut ctl) };

        ctl.type_ = AUDIO_MIXER_VALUE;
        ctl.un.value_mut().num_channels = 1;
        ctl.dev = EAP_MASTER_VOL;
        while ctl.dev < EAP_MIC_VOL {
            ctl.un.value_mut().level[AUDIO_MIXER_LEVEL_MONO] = VOL_0DB as u8;
            // SAFETY: see above.
            let _ = unsafe { eap1370_mixer_set_port(arg, &mut ctl) };
            ctl.dev += 1;
        }
        ctl.un.value_mut().level[AUDIO_MIXER_LEVEL_MONO] = 0;
        // SAFETY: see above.
        let _ = unsafe { eap1370_mixer_set_port(arg, &mut ctl) };
        ctl.dev = EAP_MIC_PREAMP;
        ctl.type_ = AUDIO_MIXER_ENUM;
        ctl.un.set_ord(0);
        // SAFETY: see above.
        let _ = unsafe { eap1370_mixer_set_port(arg, &mut ctl) };
        ctl.dev = EAP_RECORD_SOURCE;
        ctl.type_ = AUDIO_MIXER_SET;
        ctl.un.set_mask(1 << EAP_MIC_VOL);
        // SAFETY: see above.
        let _ = unsafe { eap1370_mixer_set_port(arg, &mut ctl) };

        &EAP1370_HW_IF
    } else {
        // clean slate
        eap1371_clean_slate(sc);

        // Reset from es1371's perspective
        sc.write4(EAP_ICSC, E1371_SYNC_RES);
        delay(20);
        sc.write4(EAP_ICSC, 0);

        // Must properly reprogram sample rate converter, or it locks up.
        //
        // We don't know how to program it (no documentation), and the linux/oss magic
        // recipe doesn't work (breaks full-duplex, by selecting different play and record
        // rates). On the other hand, the sample rate converter can't be disabled
        // (disabling it would disable DMA), so we use these magic defaults that make it
        // "resample" 48kHz to 48kHz without breaking full-duplex.
        eap1371_src_init(sc);

        // Reset codec

        // Interrupt enable
        sc.host_if.set(Ac97HostIf {
            arg: sc.as_arg(),
            attach: Some(eap1371_attach_codec),
            read: Some(eap1371_read_codec),
            write: Some(eap1371_write_codec),
            reset: Some(eap1371_reset_codec),
            flags: Some(eap_flags_codec),
            spdif_event: None,
        });
        sc.flags.set(AC97_HOST_DONT_READ);

        if ac97_attach(&sc.host_if.get()).is_ok() {
            // Interrupt enable
            sc.write4(EAP_SIC, EAP_P2_INTR_EN | EAP_R1_INTR_EN);
        } else {
            return;
        }

        &EAP1371_HW_IF
    };

    audio_attach_mi(eap_hw_if, sc.as_arg(), ptr::null_mut(), &sc.sc_dev);
    sc.sc_mididev
        .set(midi_attach_mi(&EAP_MIDI_HW_IF, sc.as_arg(), &sc.sc_dev));
}

/// `eap_resume`.
pub fn eap_resume(sc: &EapSoftc) {
    if !sc.sc_1371.get() {
        eap1370_init(sc);
    } else {
        // clean slate
        eap1371_clean_slate(sc);

        let _ = ac97_resume(&sc.host_if.get(), sc.codec());

        eap1371_src_init(sc);

        // Interrupt enable
        sc.write4(EAP_SIC, EAP_P2_INTR_EN | EAP_R1_INTR_EN);
    }
}

/// `eap1371_attach_codec`.
///
/// # Safety
///
/// As [`eap1371_read_codec`].
pub unsafe fn eap1371_attach_codec(sc_: *mut c_void, codec_if: &Ac97CodecIf) -> Result<(), Errno> {
    // SAFETY: the contract.
    let sc = unsafe { sc_of(sc_) };

    sc.codec_if.set(ptr::from_ref(codec_if));
    Ok(())
}

/// `eap1371_reset_codec`.
///
/// # Safety
///
/// As [`eap1371_read_codec`].
pub unsafe fn eap1371_reset_codec(sc_: *mut c_void) {
    // SAFETY: the contract.
    let sc = unsafe { sc_of(sc_) };

    mtx_enter(&AUDIO_LOCK);
    let icsc = sc.read4(EAP_ICSC);
    sc.write4(EAP_ICSC, icsc | E1371_SYNC_RES);
    delay(20);
    sc.write4(EAP_ICSC, icsc & !E1371_SYNC_RES);
    delay(1);
    mtx_leave(&AUDIO_LOCK);
}

/// `eap_intr`: the DAC2, ADC and UART interrupts. Takes `AUDIO_LOCK`, which audio(4)'s and
/// midi(4)'s call-backs need.
fn eap_intr(p: *mut c_void) -> i32 {
    // SAFETY: the handler is established with the softc as its argument.
    let sc = unsafe { sc_of(p) };

    mtx_enter(&AUDIO_LOCK);
    let intr = sc.read4(EAP_ICSS);
    if intr & EAP_INTR == 0 {
        mtx_leave(&AUDIO_LOCK);
        return 0;
    }
    let sic = sc.read4(EAP_SIC);
    if intr & EAP_I_ADC != 0 {
        // (The C keeps, under `#if 0`, a busy wait for the last 8 longwords of a recording
        // DMA that the chip sometimes signals early.)
        sc.write4(EAP_SIC, sic & !EAP_R1_INTR_EN);
        sc.write4(EAP_SIC, sic | EAP_R1_INTR_EN);
        if let Some(rintr) = sc.sc_rintr.get() {
            // SAFETY: audio(4) gave `rintr` and `sc_rarg` together to trigger_input; the
            // lock is held.
            unsafe { rintr(sc.sc_rarg.get()) };
        }
    }
    if intr & EAP_I_DAC2 != 0 {
        sc.write4(EAP_SIC, sic & !EAP_P2_INTR_EN);
        sc.write4(EAP_SIC, sic | EAP_P2_INTR_EN);
        if let Some(pintr) = sc.sc_pintr.get() {
            // SAFETY: as above, from trigger_output.
            unsafe { pintr(sc.sc_parg.get()) };
        }
    }
    if intr & EAP_I_UART != 0 {
        if sc.read1(EAP_UART_STATUS) & EAP_US_RXINT != 0 {
            while sc.read1(EAP_UART_STATUS) & EAP_US_RXRDY != 0 {
                let data = sc.read1(EAP_UART_DATA);
                if let Some(iintr) = sc.sc_iintr.get() {
                    // SAFETY: midi(4) gave `iintr` and `sc_arg` together to
                    // eap_midi_open; the lock is held.
                    unsafe { iintr(sc.sc_arg.get(), i32::from(data)) };
                }
            }
        }
        if sc.read1(EAP_UART_STATUS) & EAP_US_TXINT != 0 {
            sc.sc_uctrl.set(sc.sc_uctrl.get() & !EAP_UC_TXINTEN);
            sc.write1(EAP_UART_CONTROL, sc.sc_uctrl.get());
            if let Some(ointr) = sc.sc_ointr.get() {
                // SAFETY: as above.
                unsafe { ointr(sc.sc_arg.get()) };
            }
        }
    }
    mtx_leave(&AUDIO_LOCK);
    1
}

/// `eap_allocmem`: `size` bytes of DMA memory in one segment, mapped, with a loaded map.
pub fn eap_allocmem(sc: &EapSoftc, size: usize, align: usize, p: &EapDma) -> Result<(), Errno> {
    let dmat = sc.dmat();
    let mut segs = [BusDmaSegment::default(); 1];

    p.size.set(size);
    let nsegs = bus_dmamem_alloc(dmat, size, align, 0, &mut segs, BUS_DMA_NOWAIT)?;
    p.segs[0].set(segs[0]);
    p.nsegs.set(nsegs);

    // free: bus_dmamem_free
    let free_segs = |segs: &[BusDmaSegment; 1]| {
        // SAFETY: the segments came from bus_dmamem_alloc above and are unmapped.
        unsafe { bus_dmamem_free(dmat, &segs[..nsegs]) };
    };

    let addr = match bus_dmamem_map(dmat, &mut segs, size, BUS_DMA_NOWAIT | BUS_DMA_COHERENT) {
        Ok(addr) => addr,
        Err(e) => {
            free_segs(&segs);
            return Err(e);
        }
    };
    p.addr.set(addr.as_ptr());

    // unmap: bus_dmamem_unmap, then free
    let unmap = |segs: &[BusDmaSegment; 1]| {
        // SAFETY: the mapping came from bus_dmamem_map above with this size.
        unsafe { bus_dmamem_unmap(dmat, addr, size) };
        free_segs(segs);
    };

    let map = match bus_dmamap_create(dmat, size, 1, size, 0, BUS_DMA_NOWAIT) {
        Ok(map) => map,
        Err(e) => {
            unmap(&segs);
            return Err(e);
        }
    };
    p.map.set(Some(map));

    // SAFETY: `addr` maps `size` bytes of DMA memory that stay allocated until eap_freemem
    // unloads the map before unmapping and freeing them.
    if let Err(e) = unsafe { bus_dmamap_load(dmat, map, addr.as_ptr(), size, None, BUS_DMA_NOWAIT) }
    {
        // destroy
        // SAFETY: the map came from bus_dmamap_create above and nothing else has it.
        unsafe { bus_dmamap_destroy(dmat, NonNull::from(map)) };
        unmap(&segs);
        return Err(e);
    }
    Ok(())
}

/// `eap_freemem`: undoes `eap_allocmem`.
pub fn eap_freemem(sc: &EapSoftc, p: &EapDma) {
    let dmat = sc.dmat();

    if let Some(map) = p.map.get() {
        bus_dmamap_unload(dmat, map);
        // SAFETY: the map came from bus_dmamap_create in eap_allocmem and is unloaded.
        unsafe { bus_dmamap_destroy(dmat, NonNull::from(map)) };
    }
    if let Some(addr) = NonNull::new(p.addr.get()) {
        // SAFETY: the mapping came from bus_dmamem_map with this size.
        unsafe { bus_dmamem_unmap(dmat, addr, p.size.get()) };
    }
    // SAFETY: the segments came from bus_dmamem_alloc and are unmapped and unloaded.
    unsafe { bus_dmamem_free(dmat, &[p.segs[0].get()][..p.nsegs.get()]) };
}

/// `eap_open`.
///
/// # Safety
///
/// `addr` is the `hdl` given to `audio_attach_mi`.
pub unsafe fn eap_open(_addr: *mut c_void, _flags: i32) -> Result<(), Errno> {
    Ok(())
}

/// `eap_close`. (The C's comment: called at splaudio().)
///
/// # Safety
///
/// As [`eap_open`].
pub unsafe fn eap_close(addr: *mut c_void) {
    // SAFETY: the contract.
    let sc = unsafe { sc_of(addr) };

    // SAFETY: the contract.
    let _ = unsafe { eap_halt_output(addr) };
    // SAFETY: the contract.
    let _ = unsafe { eap_halt_input(addr) };

    sc.sc_pintr.set(None);
    sc.sc_rintr.set(None);
}

/// `eap_set_params`: 16-bit signed little-endian, up to two channels, 4 to 48 kHz on the
/// ES1370 (one clock for both directions), 48 kHz on the ES1371.
///
/// # Safety
///
/// As [`eap_open`].
pub unsafe fn eap_set_params(
    addr: *mut c_void,
    mut setmode: i32,
    usemode: i32,
    play: &mut AudioParams,
    rec: &mut AudioParams,
) -> Result<(), Errno> {
    // SAFETY: the contract.
    let sc = unsafe { sc_of(addr) };

    // The es1370 only has one clock, so make the sample rates match.
    if !sc.sc_1371.get()
        && play.sample_rate != rec.sample_rate
        && usemode == (AUMODE_PLAY | AUMODE_RECORD)
    {
        if setmode == AUMODE_PLAY {
            rec.sample_rate = play.sample_rate;
            setmode |= AUMODE_RECORD;
        } else if setmode == AUMODE_RECORD {
            play.sample_rate = rec.sample_rate;
            setmode |= AUMODE_PLAY;
        } else {
            return Err(Errno::EINVAL);
        }
    }

    for mode in [AUMODE_RECORD, AUMODE_PLAY] {
        if setmode & mode == 0 {
            continue;
        }

        let p = if mode == AUMODE_PLAY {
            &mut *play
        } else {
            &mut *rec
        };

        if sc.sc_1371.get() {
            p.sample_rate = 48000;
        }
        p.sample_rate = p.sample_rate.clamp(4000, 48000);
        if p.precision > 16 {
            p.precision = 16;
        }
        if p.channels > 2 {
            p.channels = 2;
        }
        match p.encoding {
            AUDIO_ENCODING_SLINEAR_LE => {
                if p.precision != 16 {
                    return Err(Errno::EINVAL);
                }
            }
            // The C checks for 8 bits, then falls through to `default:`.
            AUDIO_ENCODING_ULINEAR_LE | AUDIO_ENCODING_ULINEAR_BE => return Err(Errno::EINVAL),
            _ => return Err(Errno::EINVAL),
        }
        p.bps = audio_bps(p.precision);
        p.msb = 1;
    }

    if !sc.sc_1371.get() {
        // Set the speed
        let mut div = sc.read4(EAP_ICSC) & !EAP_PCLKBITS;
        // XXX
        // The -2 isn't documented, but seemed to make the wall time match what I expect.
        // - mycroft
        let rate = if usemode == AUMODE_RECORD {
            rec.sample_rate
        } else {
            play.sample_rate
        };
        div |= eap_set_pclkdiv((u64::from(EAP_XTAL_FREQ) / rate) as u32 - 2);
        div |= EAP_CCB_INTRM;
        sc.write4(EAP_ICSC, div);
    }

    Ok(())
}

/// `eap_round_blocksize`: keep good alignment.
///
/// # Safety
///
/// As [`eap_open`].
pub unsafe fn eap_round_blocksize(_addr: *mut c_void, blk: i32) -> i32 {
    (blk + 31) & -32
}

/// The DMA memory `eap_malloc` made whose kernel address is `start` (`KERNADDR(p)`).
fn eap_find_dma(sc: &EapSoftc, start: *mut u8) -> Option<&EapDma> {
    let mut p = sc.sc_dmas.get();
    while !p.is_null() {
        // SAFETY: the list holds live `EapDma`s made by eap_malloc; eap_free unlinks one
        // before freeing it.
        let d = unsafe { &*p };
        if d.addr.get() == start {
            return Some(d);
        }
        p = d.next.get();
    }
    None
}

/// `eap_trigger_output`: plays the ring `[start, end)` on DAC2, `intr(arg)` after each
/// block.
///
/// # Safety
///
/// As [`eap_open`].
pub unsafe fn eap_trigger_output(
    addr: *mut c_void,
    start: *mut u8,
    end: *mut u8,
    blksize: i32,
    intr: AudioIntr,
    arg: *mut c_void,
    param: &AudioParams,
) -> Result<(), Errno> {
    // SAFETY: the contract.
    let sc = unsafe { sc_of(addr) };

    #[cfg(feature = "diagnostic")]
    {
        if sc.sc_prun.get() {
            panic(format_args!("eap_trigger_output: already running"));
        }
        sc.sc_prun.set(true);
    }

    sc.sc_pintr.set(Some(intr));
    sc.sc_parg.set(arg);
    mtx_enter(&AUDIO_LOCK);
    let mut sic = sc.read4(EAP_SIC);
    sic &= !(EAP_P2_S_EB | EAP_P2_S_MB | EAP_INC_BITS);
    sic |= eap_set_p2_st_inc(0) | eap_set_p2_end_inc(param.precision / 8);
    let mut sampshift = 0;
    if param.precision == 16 {
        sic |= EAP_P2_S_EB;
        sampshift += 1;
    }
    if param.channels == 2 {
        sic |= EAP_P2_S_MB;
        sampshift += 1;
    }
    sc.write4(EAP_SIC, sic & !EAP_P2_INTR_EN);
    sc.write4(EAP_SIC, sic | EAP_P2_INTR_EN);

    let Some(p) = eap_find_dma(sc, start) else {
        mtx_leave(&AUDIO_LOCK);
        printf(format_args!("eap_trigger_output: bad addr {start:p}\n"));
        return Err(Errno::EINVAL);
    };

    let len = (end as usize).wrapping_sub(start as usize);
    sc.write4(EAP_MEMPAGE, EAP_DAC_PAGE);
    sc.write4(EAP_DAC2_ADDR, p.dmaaddr());
    sc.write4(
        EAP_DAC2_SIZE,
        eap_set_size(0, ((len >> 2) as u32).wrapping_sub(1)),
    );

    sc.write4(EAP_DAC2_CSR, ((blksize >> sampshift) - 1) as u32);

    if sc.sc_1371.get() {
        sc.write4(E1371_SRC, 0);
    }

    let icsc = sc.read4(EAP_ICSC);
    sc.write4(EAP_ICSC, icsc | EAP_DAC2_EN);

    mtx_leave(&AUDIO_LOCK);
    Ok(())
}

/// `eap_trigger_input`: records into the ring `[start, end)` from the ADC.
///
/// # Safety
///
/// As [`eap_open`].
pub unsafe fn eap_trigger_input(
    addr: *mut c_void,
    start: *mut u8,
    end: *mut u8,
    blksize: i32,
    intr: AudioIntr,
    arg: *mut c_void,
    param: &AudioParams,
) -> Result<(), Errno> {
    // SAFETY: the contract.
    let sc = unsafe { sc_of(addr) };

    #[cfg(feature = "diagnostic")]
    {
        if sc.sc_rrun.get() {
            panic(format_args!("eap_trigger_input: already running"));
        }
        sc.sc_rrun.set(true);
    }

    sc.sc_rintr.set(Some(intr));
    sc.sc_rarg.set(arg);
    mtx_enter(&AUDIO_LOCK);
    let mut sic = sc.read4(EAP_SIC);
    sic &= !(EAP_R1_S_EB | EAP_R1_S_MB);
    let mut sampshift = 0;
    if param.precision == 16 {
        sic |= EAP_R1_S_EB;
        sampshift += 1;
    }
    if param.channels == 2 {
        sic |= EAP_R1_S_MB;
        sampshift += 1;
    }
    sc.write4(EAP_SIC, sic & !EAP_R1_INTR_EN);
    sc.write4(EAP_SIC, sic | EAP_R1_INTR_EN);

    let Some(p) = eap_find_dma(sc, start) else {
        mtx_leave(&AUDIO_LOCK);
        printf(format_args!("eap_trigger_input: bad addr {start:p}\n"));
        return Err(Errno::EINVAL);
    };

    let len = (end as usize).wrapping_sub(start as usize);
    sc.write4(EAP_MEMPAGE, EAP_ADC_PAGE);
    sc.write4(EAP_ADC_ADDR, p.dmaaddr());
    sc.write4(
        EAP_ADC_SIZE,
        eap_set_size(0, ((len >> 2) as u32).wrapping_sub(1)),
    );

    sc.write4(EAP_ADC_CSR, ((blksize >> sampshift) - 1) as u32);

    if sc.sc_1371.get() {
        sc.write4(E1371_SRC, 0);
    }

    let icsc = sc.read4(EAP_ICSC);
    sc.write4(EAP_ICSC, icsc | EAP_ADC_EN);

    mtx_leave(&AUDIO_LOCK);
    Ok(())
}

/// `eap_halt_output`.
///
/// # Safety
///
/// As [`eap_open`].
pub unsafe fn eap_halt_output(addr: *mut c_void) -> Result<(), Errno> {
    // SAFETY: the contract.
    let sc = unsafe { sc_of(addr) };

    mtx_enter(&AUDIO_LOCK);
    let icsc = sc.read4(EAP_ICSC);
    sc.write4(EAP_ICSC, icsc & !EAP_DAC2_EN);
    sc.sc_prun.set(false);
    mtx_leave(&AUDIO_LOCK);
    Ok(())
}

/// `eap_halt_input`.
///
/// # Safety
///
/// As [`eap_open`].
pub unsafe fn eap_halt_input(addr: *mut c_void) -> Result<(), Errno> {
    // SAFETY: the contract.
    let sc = unsafe { sc_of(addr) };

    mtx_enter(&AUDIO_LOCK);
    let icsc = sc.read4(EAP_ICSC);
    sc.write4(EAP_ICSC, icsc & !EAP_ADC_EN);
    sc.sc_rrun.set(false);
    mtx_leave(&AUDIO_LOCK);
    Ok(())
}

/// `eap1371_mixer_set_port`: the AC97 codec's.
///
/// # Safety
///
/// As [`eap_open`].
pub unsafe fn eap1371_mixer_set_port(addr: *mut c_void, cp: &mut MixerCtrl) -> Result<(), Errno> {
    // SAFETY: the contract.
    let sc = unsafe { sc_of(addr) };
    let codec = sc.codec();
    (codec.vtbl.mixer_set_port)(codec, cp)
}

/// `eap1371_mixer_get_port`.
///
/// # Safety
///
/// As [`eap_open`].
pub unsafe fn eap1371_mixer_get_port(addr: *mut c_void, cp: &mut MixerCtrl) -> Result<(), Errno> {
    // SAFETY: the contract.
    let sc = unsafe { sc_of(addr) };
    let codec = sc.codec();
    (codec.vtbl.mixer_get_port)(codec, cp)
}

/// `eap1371_query_devinfo`.
///
/// # Safety
///
/// As [`eap_open`].
pub unsafe fn eap1371_query_devinfo(
    addr: *mut c_void,
    dip: &mut MixerDevinfo,
) -> Result<(), Errno> {
    // SAFETY: the contract.
    let sc = unsafe { sc_of(addr) };
    let codec = sc.codec();
    (codec.vtbl.query_devinfo)(codec, dip)
}

/// `eap1370_set_mixer`: writes an AK4531 register and remembers it.
pub fn eap1370_set_mixer(sc: &EapSoftc, a: i32, d: i32) {
    eap1370_write_codec(sc, a, d);

    sc.sc_port[a as usize].set(d as u16);
}

/// The AK4531 input and output mixer bits of a source mask (`1 << EAP_*_VOL`), for
/// `EAP_RECORD_SOURCE` (`(l1, r1, l2, r2)`) and `EAP_INPUT_SOURCE` (`(o1, o2)`).
fn eap1370_record_mixers(m: u32) -> (i32, i32, i32, i32) {
    let (mut l1, mut r1, mut l2, mut r2) = (0, 0, 0, 0);
    if m & (1 << EAP_VOICE_VOL) != 0 {
        l2 |= AK_M_VOICE;
        r2 |= AK_M_VOICE;
    }
    if m & (1 << EAP_FM_VOL) != 0 {
        l1 |= AK_M_FM_L;
        r1 |= AK_M_FM_R;
    }
    if m & (1 << EAP_CD_VOL) != 0 {
        l1 |= AK_M_CD_L;
        r1 |= AK_M_CD_R;
    }
    if m & (1 << EAP_LINE_VOL) != 0 {
        l1 |= AK_M_LINE_L;
        r1 |= AK_M_LINE_R;
    }
    if m & (1 << EAP_AUX_VOL) != 0 {
        l2 |= AK_M2_AUX_L;
        r2 |= AK_M2_AUX_R;
    }
    if m & (1 << EAP_MIC_VOL) != 0 {
        l2 |= AK_M_TMIC;
        r2 |= AK_M_TMIC;
    }
    (l1, r1, l2, r2)
}

/// The output mixer bits of `EAP_INPUT_SOURCE`'s mask, `(o1, o2)`.
fn eap1370_input_mixers(m: u32) -> (i32, i32) {
    let (mut o1, mut o2) = (0, 0);
    if m & (1 << EAP_VOICE_VOL) != 0 {
        o2 |= AK_M_VOICE_L | AK_M_VOICE_R;
    }
    if m & (1 << EAP_FM_VOL) != 0 {
        o1 |= AK_M_FM_L | AK_M_FM_R;
    }
    if m & (1 << EAP_CD_VOL) != 0 {
        o1 |= AK_M_CD_L | AK_M_CD_R;
    }
    if m & (1 << EAP_LINE_VOL) != 0 {
        o1 |= AK_M_LINE_L | AK_M_LINE_R;
    }
    if m & (1 << EAP_AUX_VOL) != 0 {
        o2 |= AK_M_AUX_L | AK_M_AUX_R;
    }
    if m & (1 << EAP_MIC_VOL) != 0 {
        o1 |= AK_M_MIC;
    }
    (o1, o2)
}

/// `eap1370_mixer_set_port`: the AK4531's mixer.
///
/// # Safety
///
/// As [`eap_open`].
pub unsafe fn eap1370_mixer_set_port(addr: *mut c_void, cp: &mut MixerCtrl) -> Result<(), Errno> {
    // SAFETY: the contract.
    let sc = unsafe { sc_of(addr) };

    if cp.dev == EAP_RECORD_SOURCE {
        if cp.type_ != AUDIO_MIXER_SET {
            return Err(Errno::EINVAL);
        }
        let m = cp.un.mask() as u32;
        sc.sc_record_source.set(m);
        let (l1, r1, l2, r2) = eap1370_record_mixers(m);
        eap1370_set_mixer(sc, AK_IN_MIXER1_L, l1);
        eap1370_set_mixer(sc, AK_IN_MIXER1_R, r1);
        eap1370_set_mixer(sc, AK_IN_MIXER2_L, l2);
        eap1370_set_mixer(sc, AK_IN_MIXER2_R, r2);
        return Ok(());
    }
    if cp.dev == EAP_INPUT_SOURCE {
        if cp.type_ != AUDIO_MIXER_SET {
            return Err(Errno::EINVAL);
        }
        let m = cp.un.mask() as u32;
        sc.sc_input_source.set(m);
        let (o1, o2) = eap1370_input_mixers(m);
        eap1370_set_mixer(sc, AK_OUT_MIXER1, o1);
        eap1370_set_mixer(sc, AK_OUT_MIXER2, o2);
        return Ok(());
    }
    if cp.dev == EAP_MIC_PREAMP {
        if cp.type_ != AUDIO_MIXER_ENUM {
            return Err(Errno::EINVAL);
        }
        if cp.un.ord() != 0 && cp.un.ord() != 1 {
            return Err(Errno::EINVAL);
        }
        sc.sc_mic_preamp.set(cp.un.ord() as u32);
        eap1370_set_mixer(sc, AK_MGAIN, cp.un.ord());
        return Ok(());
    }
    if cp.type_ != AUDIO_MIXER_VALUE {
        return Err(Errno::EINVAL);
    }
    let v = cp.un.value();
    let (lval, rval) = match v.num_channels {
        1 => {
            let m = i32::from(v.level[AUDIO_MIXER_LEVEL_MONO]);
            (m, m)
        }
        2 => (
            i32::from(v.level[AUDIO_MIXER_LEVEL_LEFT]),
            i32::from(v.level[AUDIO_MIXER_LEVEL_RIGHT]),
        ),
        _ => return Err(Errno::EINVAL),
    };
    let (l, r, la, ra) = match cp.dev {
        EAP_MASTER_VOL => (
            vol_to_att5(lval),
            vol_to_att5(rval),
            AK_MASTER_L,
            AK_MASTER_R,
        ),
        _ => {
            let (la, ra) = match cp.dev {
                EAP_MIC_VOL => {
                    if v.num_channels != 1 {
                        return Err(Errno::EINVAL);
                    }
                    (AK_MIC, -1)
                }
                EAP_VOICE_VOL => (AK_VOICE_L, AK_VOICE_R),
                EAP_FM_VOL => (AK_FM_L, AK_FM_R),
                EAP_CD_VOL => (AK_CD_L, AK_CD_R),
                EAP_LINE_VOL => (AK_LINE_L, AK_LINE_R),
                EAP_AUX_VOL => (AK_AUX_L, AK_AUX_R),
                _ => return Err(Errno::EINVAL),
            };
            // lr:
            (vol_to_gain5(lval), vol_to_gain5(rval), la, ra)
        }
    };
    eap1370_set_mixer(sc, la, l);
    if ra >= 0 {
        eap1370_set_mixer(sc, ra, r);
    }
    Ok(())
}

/// `eap1370_mixer_get_port`.
///
/// # Safety
///
/// As [`eap_open`].
pub unsafe fn eap1370_mixer_get_port(addr: *mut c_void, cp: &mut MixerCtrl) -> Result<(), Errno> {
    // SAFETY: the contract.
    let sc = unsafe { sc_of(addr) };
    let port = |a: i32| i32::from(sc.sc_port[a as usize].get());

    let (l, r) = match cp.dev {
        EAP_RECORD_SOURCE => {
            if cp.type_ != AUDIO_MIXER_SET {
                return Err(Errno::EINVAL);
            }
            cp.un.set_mask(sc.sc_record_source.get() as i32);
            return Ok(());
        }
        EAP_INPUT_SOURCE => {
            if cp.type_ != AUDIO_MIXER_SET {
                return Err(Errno::EINVAL);
            }
            cp.un.set_mask(sc.sc_input_source.get() as i32);
            return Ok(());
        }
        EAP_MIC_PREAMP => {
            if cp.type_ != AUDIO_MIXER_ENUM {
                return Err(Errno::EINVAL);
            }
            cp.un.set_ord(sc.sc_mic_preamp.get() as i32);
            return Ok(());
        }
        EAP_MASTER_VOL => (
            att5_to_vol(port(AK_MASTER_L)),
            att5_to_vol(port(AK_MASTER_R)),
        ),
        _ => {
            let (la, ra) = match cp.dev {
                EAP_MIC_VOL => {
                    if cp.un.value().num_channels != 1 {
                        return Err(Errno::EINVAL);
                    }
                    (AK_MIC, AK_MIC)
                }
                EAP_VOICE_VOL => (AK_VOICE_L, AK_VOICE_R),
                EAP_FM_VOL => (AK_FM_L, AK_FM_R),
                EAP_CD_VOL => (AK_CD_L, AK_CD_R),
                EAP_LINE_VOL => (AK_LINE_L, AK_LINE_R),
                EAP_AUX_VOL => (AK_AUX_L, AK_AUX_R),
                _ => return Err(Errno::EINVAL),
            };
            // lr:
            (gain5_to_vol(port(la)), gain5_to_vol(port(ra)))
        }
    };
    let v = cp.un.value_mut();
    match v.num_channels {
        1 => v.level[AUDIO_MIXER_LEVEL_MONO] = ((l + r) / 2) as u8,
        2 => {
            v.level[AUDIO_MIXER_LEVEL_LEFT] = l as u8;
            v.level[AUDIO_MIXER_LEVEL_RIGHT] = r as u8;
        }
        _ => return Err(Errno::EINVAL),
    }
    Ok(())
}

/// The members of `EAP_RECORD_SOURCE` and `EAP_INPUT_SOURCE`, in the C's order.
const EAP_SOURCES: [(&[u8], i32); 6] = [
    (AudioNmicrophone, EAP_MIC_VOL),
    (AudioNcd, EAP_CD_VOL),
    (AudioNline, EAP_LINE_VOL),
    (AudioNfmsynth, EAP_FM_VOL),
    (AudioNaux, EAP_AUX_VOL),
    (AudioNdac, EAP_VOICE_VOL),
];

/// Fills an `AUDIO_MIXER_VALUE` control of class `class` named `name` with
/// `num_channels`, in volume units.
fn devinfo_value(dip: &mut MixerDevinfo, class: i32, name: &[u8], num_channels: i32) {
    dip.type_ = AUDIO_MIXER_VALUE;
    dip.mixer_class = class;
    strlcpy(&mut dip.label.name, name);
    dip.un.v_mut().num_channels = num_channels;
    strlcpy(&mut dip.un.v_mut().units.name, AudioNvolume);
}

/// Fills a source set control (`EAP_RECORD_SOURCE`, `EAP_INPUT_SOURCE`).
fn devinfo_sources(dip: &mut MixerDevinfo, class: i32) {
    dip.mixer_class = class;
    dip.prev = AUDIO_MIXER_LAST;
    dip.next = AUDIO_MIXER_LAST;
    strlcpy(&mut dip.label.name, AudioNsource);
    dip.type_ = AUDIO_MIXER_SET;
    let s = dip.un.s_mut();
    s.num_mem = 6;
    for (m, (name, vol)) in s.member.iter_mut().zip(EAP_SOURCES) {
        strlcpy(&mut m.label.name, name);
        m.mask = 1 << vol;
    }
}

/// Fills a class control.
fn devinfo_class(dip: &mut MixerDevinfo, class: i32, name: &[u8]) {
    dip.type_ = AUDIO_MIXER_CLASS;
    dip.mixer_class = class;
    dip.next = AUDIO_MIXER_LAST;
    dip.prev = AUDIO_MIXER_LAST;
    strlcpy(&mut dip.label.name, name);
}

/// `eap1370_query_devinfo`: the AK4531 mixer's controls.
///
/// # Safety
///
/// As [`eap_open`].
pub unsafe fn eap1370_query_devinfo(
    _addr: *mut c_void,
    dip: &mut MixerDevinfo,
) -> Result<(), Errno> {
    match dip.index {
        EAP_MASTER_VOL => {
            devinfo_value(dip, EAP_OUTPUT_CLASS, AudioNmaster, 2);
            dip.prev = AUDIO_MIXER_LAST;
            dip.next = AUDIO_MIXER_LAST;
        }
        EAP_VOICE_VOL | EAP_FM_VOL | EAP_CD_VOL | EAP_LINE_VOL | EAP_AUX_VOL => {
            let name: &[u8] = match dip.index {
                EAP_VOICE_VOL => AudioNdac,
                EAP_FM_VOL => AudioNfmsynth,
                EAP_CD_VOL => AudioNcd,
                EAP_LINE_VOL => AudioNline,
                _ => AudioNaux,
            };
            dip.prev = AUDIO_MIXER_LAST;
            dip.next = AUDIO_MIXER_LAST;
            devinfo_value(dip, EAP_INPUT_CLASS, name, 2);
        }
        EAP_MIC_VOL => {
            dip.prev = AUDIO_MIXER_LAST;
            dip.next = EAP_MIC_PREAMP;
            devinfo_value(dip, EAP_INPUT_CLASS, AudioNmicrophone, 1);
        }
        EAP_RECORD_SOURCE => devinfo_sources(dip, EAP_RECORD_CLASS),
        EAP_INPUT_SOURCE => devinfo_sources(dip, EAP_INPUT_CLASS),
        EAP_MIC_PREAMP => {
            dip.type_ = AUDIO_MIXER_ENUM;
            dip.mixer_class = EAP_INPUT_CLASS;
            dip.prev = EAP_MIC_VOL;
            dip.next = AUDIO_MIXER_LAST;
            strlcpy(&mut dip.label.name, AudioNpreamp);
            let e = dip.un.e_mut();
            e.num_mem = 2;
            strlcpy(&mut e.member[0].label.name, AudioNoff);
            e.member[0].ord = 0;
            strlcpy(&mut e.member[1].label.name, AudioNon);
            e.member[1].ord = 1;
        }
        EAP_OUTPUT_CLASS => devinfo_class(dip, EAP_OUTPUT_CLASS, AudioCoutputs),
        EAP_RECORD_CLASS => devinfo_class(dip, EAP_RECORD_CLASS, AudioCrecord),
        EAP_INPUT_CLASS => devinfo_class(dip, EAP_INPUT_CLASS, AudioCinputs),
        _ => return Err(Errno::ENXIO),
    }
    Ok(())
}

/// `eap_malloc`: DMA memory for a ring, remembered in the softc's list.
///
/// # Safety
///
/// As [`eap_open`].
pub unsafe fn eap_malloc(
    addr: *mut c_void,
    _direction: i32,
    size: usize,
    pool: i32,
    flags: i32,
) -> Option<NonNull<u8>> {
    // SAFETY: the contract.
    let sc = unsafe { sc_of(addr) };

    let p = malloc(size_of::<EapDma>(), pool, flags | M_ZERO)?;
    let pd = p.cast::<EapDma>();
    // SAFETY: a fresh zeroed allocation of an `EapDma`, valid as zero bits.
    let dma = unsafe { pd.as_ref() };
    if eap_allocmem(sc, size, 16, dma).is_err() {
        free(p, pool, size_of::<EapDma>());
        return None;
    }
    dma.next.set(sc.sc_dmas.get());
    sc.sc_dmas.set(pd.as_ptr());
    NonNull::new(dma.addr.get())
}

/// `eap_free`: gives back what `eap_malloc` made for `ptr`.
///
/// # Safety
///
/// As [`eap_open`].
pub unsafe fn eap_free(addr: *mut c_void, ptr: NonNull<u8>, pool: i32) {
    // SAFETY: the contract.
    let sc = unsafe { sc_of(addr) };

    let mut pp: &Cell<*mut EapDma> = &sc.sc_dmas;
    while let Some(p) = NonNull::new(pp.get()) {
        // SAFETY: the list holds live `EapDma`s made by eap_malloc.
        let d = unsafe { p.as_ref() };
        if d.addr.get() == ptr.as_ptr() {
            eap_freemem(sc, d);
            pp.set(d.next.get());
            free(p.cast(), pool, size_of::<EapDma>());
            return;
        }
        pp = &d.next;
    }
}

/// `eap_flags_codec`.
///
/// # Safety
///
/// As [`eap1371_read_codec`].
pub unsafe fn eap_flags_codec(v: *mut c_void) -> u32 {
    // SAFETY: the contract.
    let sc = unsafe { sc_of(v) };

    sc.flags.get()
}

/// `eap_midi_open`: enables the UART, with its receive interrupt when reading.
///
/// # Safety
///
/// `addr` is the `hdl` given to `midi_attach_mi`.
pub unsafe fn eap_midi_open(
    addr: *mut c_void,
    flags: i32,
    iintr: MidiIintr,
    ointr: MidiOintr,
    arg: *mut c_void,
) -> Result<(), Errno> {
    // SAFETY: the contract.
    let sc = unsafe { sc_of(addr) };

    sc.sc_iintr.set(Some(iintr));
    sc.sc_ointr.set(Some(ointr));
    sc.sc_arg.set(arg);

    sc.write4(EAP_ICSC, sc.read4(EAP_ICSC) | EAP_UART_EN);
    sc.sc_uctrl.set(0);
    if flags & FREAD != 0 {
        sc.sc_uctrl.set(sc.sc_uctrl.get() | EAP_UC_RXINTEN);
    }
    sc.write1(EAP_UART_CONTROL, sc.sc_uctrl.get());

    Ok(())
}

/// `eap_midi_close`.
///
/// # Safety
///
/// As [`eap_midi_open`].
pub unsafe fn eap_midi_close(addr: *mut c_void) {
    // SAFETY: the contract.
    let sc = unsafe { sc_of(addr) };

    // give uart a chance to drain
    let _ = tsleep_nsec(ptr::from_ref(sc), PWAIT, "eapclm", msec_to_nsec(100));

    sc.write1(EAP_UART_CONTROL, 0);
    sc.write4(EAP_ICSC, sc.read4(EAP_ICSC) & !EAP_UART_EN);

    sc.sc_iintr.set(None);
    sc.sc_ointr.set(None);
}

/// `eap_midi_output`: sends a byte if the UART can take it.
///
/// # Safety
///
/// As [`eap_midi_open`].
pub unsafe fn eap_midi_output(addr: *mut c_void, d: i32) -> bool {
    // SAFETY: the contract.
    let sc = unsafe { sc_of(addr) };

    if sc.read1(EAP_UART_STATUS) & EAP_US_TXRDY == 0 {
        return false;
    }
    sc.write1(EAP_UART_DATA, d as u8);
    sc.sc_uctrl.set(sc.sc_uctrl.get() | EAP_UC_TXINTEN);
    sc.write1(EAP_UART_CONTROL, sc.sc_uctrl.get());
    true
}

/// `eap_midi_getinfo`.
///
/// # Safety
///
/// As [`eap_midi_open`].
pub unsafe fn eap_midi_getinfo(_addr: *mut c_void, mi: &mut MidiInfo) {
    mi.name = "AudioPCI MIDI UART";
    mi.props = MIDI_PROP_CAN_INPUT | MIDI_PROP_OUT_INTR;
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_masks_map_to_the_ak4531_mixers() {
        // attach's input sources: every one but the master.
        let all = (1 << EAP_VOICE_VOL)
            | (1 << EAP_FM_VOL)
            | (1 << EAP_CD_VOL)
            | (1 << EAP_LINE_VOL)
            | (1 << EAP_AUX_VOL)
            | (1 << EAP_MIC_VOL);
        assert_eq!(eap1370_input_mixers(all), (0x7f, 0x3c));
        // attach's record source: the microphone.
        assert_eq!(
            eap1370_record_mixers(1 << EAP_MIC_VOL),
            (0, 0, AK_M_TMIC, AK_M_TMIC)
        );
        assert_eq!(
            eap1370_record_mixers(1 << EAP_FM_VOL),
            (AK_M_FM_L, AK_M_FM_R, 0, 0)
        );
    }

    #[test]
    fn devinfo_tables() {
        let mut d = MixerDevinfo::zeroed();
        d.index = EAP_RECORD_SOURCE;
        // SAFETY: eap1370_query_devinfo does not use its handle.
        unsafe { eap1370_query_devinfo(ptr::null_mut(), &mut d) }.unwrap();
        assert_eq!(d.type_, AUDIO_MIXER_SET);
        assert_eq!(d.mixer_class, EAP_RECORD_CLASS);
        assert_eq!(d.un.s().num_mem, 6);
        assert_eq!(&d.un.s().member[5].label.name[..4], b"dac\0");
        assert_eq!(d.un.s().member[5].mask, 1 << EAP_VOICE_VOL);
        d.index = EAP_MIC_VOL;
        // SAFETY: as above.
        unsafe { eap1370_query_devinfo(ptr::null_mut(), &mut d) }.unwrap();
        assert_eq!(d.next, EAP_MIC_PREAMP);
        assert_eq!(d.un.v().num_channels, 1);
        d.index = EAP_INPUT_CLASS + 1;
        // SAFETY: as above.
        let e = unsafe { eap1370_query_devinfo(ptr::null_mut(), &mut d) };
        assert_eq!(e, Err(Errno::ENXIO));
    }

    #[test]
    fn blocksize_rounding() {
        // SAFETY: eap_round_blocksize does not use its handle.
        let r = |b| unsafe { eap_round_blocksize(ptr::null_mut(), b) };
        assert_eq!(r(1), 32);
        assert_eq!(r(32), 32);
        assert_eq!(r(4097), 4128);
    }

    #[test]
    fn midi_info() {
        let mut mi = MidiInfo::default();
        // SAFETY: eap_midi_getinfo does not use its handle.
        unsafe { eap_midi_getinfo(ptr::null_mut(), &mut mi) };
        assert_eq!(mi.name, "AudioPCI MIDI UART");
        assert_eq!(mi.props, MIDI_PROP_CAN_INPUT | MIDI_PROP_OUT_INTR);
    }
}
/* </TESTS> */
