/* <LICENSES> */
/*
 * Copyright (c) 2000,2001 Michael Shalayeff
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
 * IN NO EVENT SHALL THE AUTHOR OR HIS RELATIVES BE LIABLE FOR ANY DIRECT,
 * INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
 * (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
 * SERVICES; LOSS OF MIND, USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT,
 * STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING
 * IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF
 * THE POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! `auich(4)`: the Intel ICH AC'97 audio controller (`auich* at pci?`), with the ICH2 to
//! ICH7, ESB, 440MX, SiS 7012, nForce and AMD 768/8111 variants. The controller plays and
//! records through three buffer descriptor lists (PCM out, PCM in, microphone in) of 32
//! entries each, and talks to its codec through ac97(4).
//!
//! Upstream: sys/dev/pci/auich.c @ 3ce1f3f79392
//!
//! AC'97 audio found on Intel 810/815/820/440MX chipsets.
//!
//! - <http://developer.intel.com/design/chipsets/datashts/290655.htm>
//! - <http://developer.intel.com/design/chipsets/manuals/298028.htm>
//! - <http://www.intel.com/design/chipsets/datashts/290714.htm>
//! - <http://www.intel.com/design/chipsets/datashts/290744.htm>
//!
//! Attach maps the codec (NAMBAR) and bus master (NABMBAR) I/O BARs (the memory BARs MMBAR
//! and MBBAR first on ICH4 to ICH7), establishes the interrupt at `IPL_AUDIO | IPL_MPSAFE`
//! (the handler takes `audio_lock`, as audio(4)'s call-backs need), allocates the descriptor
//! lists, resets the AC-link, attaches the codec through [`ac97_attach`] and registers the
//! hardware interface with audio(4) ([`audio_attach_mi`]). The play and record rings are
//! DMA memory made by `allocm` (one segment); `trigger_output` and `trigger_input` fill all
//! 32 descriptors with consecutive blocks of the ring and start the engine, and each
//! block-complete interrupt refills the finished descriptors and calls audio(4)'s
//! `audio_pintr`/`audio_rintr`. The first `open` measures the real AC-link rate against the
//! 48 kHz nominal one by recording a block with interrupts off ([`auich_calibrate`]), and
//! `set_params` scales the codec rates by it.
//!
//! ## Deviations
//! - `AUICH_DEBUG` is not configured: the `DPRINTF`s and the debug branches of `auich_intr`
//!   and `auich_trigger_output` are not carried over.
//! - The softc and the DMA bookkeeping structures are `#[repr(C)]` and made of `Cell`s, all
//!   valid as zero bits (the device is allocated zeroed); the bus tags and handles are
//!   `Option`s, the codec interface and the stream call-backs pointers or `Option`s. The
//!   softc is reached as `&'static` (it is never freed while the device exists), which also
//!   gives the interrupt its `&'static str` name.
//! - The buffer descriptors (`struct auich_dmalist`) are written with volatile stores,
//!   `sc_cddma` is `sc_cddmamap->dm_segs[0].ds_addr`, and the ring addresses (`start`, `p`,
//!   `end`) are the 32-bit values the hardware takes (`ds_addr as u32`).
//! - `trigger_output` and `trigger_input` return `EINVAL` where the C returns `-1`, also when
//!   no play/record buffer was allocated (the C dereferences NULL).
//! - `auich_freem` and `auich_calibrate` clear `sc_pdma`, `sc_rdma` and `sc_cdma` when they
//!   free the buffer they point to; the C leaves them dangling and later compares through
//!   them.
//! - `auich_calibrate` indexes the PCM-in list with `civ & AUICH_LVI_MASK` (the C indexes
//!   with the whole register byte) and divides by `max(wait_us, 1)`.
//! - The three error returns of the codec access methods (`-1` on a timeout in the C) are
//!   `EIO`, see ac97(4)'s deviations.
//! - The `AUICH_MICI` acknowledgement in `auich_intr` writes `sts + (AUICH_BCIS |
//!   AUICH_FIFOE)`, as the C does (every other pipe writes `sts & (...)`).
//! - The device has no `ca_detach` (the C has none either).

use core::cell::Cell;
use core::ffi::c_void;
use core::mem::offset_of;
use core::ptr::{self, NonNull};

use crate::dev::audio::{AUDIO_LOCK, audio_attach_mi};
use crate::dev::audio_if::{
    AUDIO_ENCODING_SLINEAR_LE, AudioHwIf, AudioIntr, AudioParams, audio_bps,
};
use crate::dev::ic::ac97::{
    AC97_BITS_6CH, AC97_EXT_AUDIO_SDAC, AC97_HOST_SWAPPED_CHANNELS, AC97_REG_PCM_FRONT_DAC_RATE,
    AC97_REG_PCM_LFE_DAC_RATE, AC97_REG_PCM_LR_ADC_RATE, AC97_REG_PCM_SURR_DAC_RATE, Ac97CodecIf,
    Ac97HostIf, ac97_attach, ac97_resume,
};
use crate::dev::pci::pci_map::pci_mapreg_map;
use crate::dev::pci::pcidevs::{
    PCI_PRODUCT_AMD_8111_ACA, PCI_PRODUCT_AMD_PBC768_ACA, PCI_PRODUCT_INTEL_6300ESB_ACA,
    PCI_PRODUCT_INTEL_6321ESB_ACA, PCI_PRODUCT_INTEL_82440MX_ACA, PCI_PRODUCT_INTEL_82801AA_ACA,
    PCI_PRODUCT_INTEL_82801AB_ACA, PCI_PRODUCT_INTEL_82801BA_ACA, PCI_PRODUCT_INTEL_82801CA_ACA,
    PCI_PRODUCT_INTEL_82801DB_ACA, PCI_PRODUCT_INTEL_82801EB_ACA, PCI_PRODUCT_INTEL_82801FB_ACA,
    PCI_PRODUCT_INTEL_82801GB_ACA, PCI_PRODUCT_NVIDIA_MCP04_AC97, PCI_PRODUCT_NVIDIA_MCP51_ACA,
    PCI_PRODUCT_NVIDIA_NFORCE_ACA, PCI_PRODUCT_NVIDIA_NFORCE2_400_ACA,
    PCI_PRODUCT_NVIDIA_NFORCE2_ACA, PCI_PRODUCT_NVIDIA_NFORCE3_250_ACA,
    PCI_PRODUCT_NVIDIA_NFORCE3_ACA, PCI_PRODUCT_NVIDIA_NFORCE4_AC, PCI_PRODUCT_SIS_7012_ACA,
    PCI_VENDOR_AMD, PCI_VENDOR_INTEL, PCI_VENDOR_NVIDIA, PCI_VENDOR_SIS,
};
use crate::dev::pci::pcireg::{PCI_MAPREG_TYPE_IO, PCI_MAPREG_TYPE_MEM, pci_product, pci_vendor};
use crate::dev::pci::pcivar::{PciAttachArgs, Pcireg};
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_tc::microuptime;
use crate::kern::subr_autoconf::config_activate_children;
use crate::kern::subr_prf::{Bitmask, panic, printf};
use crate::machine::bus::{
    BUS_DMA_COHERENT, BUS_DMA_NOCACHE, BUS_DMA_NOWAIT, BusDmaSegment, BusDmaTag, BusDmamap,
    BusSpaceHandle, BusSpaceTag, bus_dmamap_create, bus_dmamap_destroy, bus_dmamap_load,
    bus_dmamap_unload, bus_dmamem_alloc, bus_dmamem_free, bus_dmamem_map, bus_dmamem_unmap,
    bus_space_read_1, bus_space_read_2, bus_space_read_4, bus_space_unmap, bus_space_write_1,
    bus_space_write_2, bus_space_write_4,
};
use crate::machine::cpu::delay;
use crate::machine::intr::{IPL_AUDIO, IPL_MPSAFE};
use crate::machine::pci_machdep::{
    PciChipsetTag, pci_conf_read, pci_conf_write, pci_intr_disestablish, pci_intr_establish,
    pci_intr_map, pci_intr_string,
};
use crate::sys::audioio::{AUMODE_PLAY, AUMODE_RECORD, MixerCtrl, MixerDevinfo};
use crate::sys::device::{CfMatch, Cfattach, Cfdriver, DV_DULL, DVACT_RESUME, Device, Softc};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_DEVBUF, M_NOWAIT, M_ZERO};
use crate::sys::param::PAGE_SIZE;

/// `AUICH_NAMBAR`: 12.1.10 NAMBAR - native audio mixer base address register.
pub const AUICH_NAMBAR: i32 = 0x10;
/// `AUICH_NABMBAR`: 12.1.11 NABMBAR - native audio bus mastering base address register.
pub const AUICH_NABMBAR: i32 = 0x14;
/// `AUICH_CFG`.
pub const AUICH_CFG: i32 = 0x41;
/// `AUICH_CFG_IOSE`.
pub const AUICH_CFG_IOSE: u32 = 0x01;
/// `AUICH_MMBAR`: ICH4/ICH5/ICH6/ICH7 native audio mixer BAR.
pub const AUICH_MMBAR: i32 = 0x18;
/// `AUICH_MBBAR`: ICH4/ICH5/ICH6/ICH7 native bus mastering BAR.
pub const AUICH_MBBAR: i32 = 0x1c;
/// `AUICH_S2CR`: tertiary codec ready.
pub const AUICH_S2CR: u32 = 0x1000_0000;

// table 12-3. native audio bus master control registers

/// `AUICH_BDBAR`: 8-byte aligned address.
pub const AUICH_BDBAR: usize = 0x00;
/// `AUICH_CIV`: 5 bits current index value.
pub const AUICH_CIV: usize = 0x04;
/// `AUICH_LVI`: 5 bits last valid index value.
pub const AUICH_LVI: usize = 0x05;
/// `AUICH_LVI_MASK`.
pub const AUICH_LVI_MASK: u8 = 0x1f;
/// `AUICH_STS`: 16 bits status.
pub const AUICH_STS: usize = 0x06;
/// `AUICH_FIFOE`: fifo error.
pub const AUICH_FIFOE: u16 = 0x10;
/// `AUICH_BCIS`: r- buf cmplt int sts; wr ack.
pub const AUICH_BCIS: u16 = 0x08;
/// `AUICH_LVBCI`: r- last valid bci, wr ack.
pub const AUICH_LVBCI: u16 = 0x04;
/// `AUICH_CELV`: current equals last valid.
pub const AUICH_CELV: u16 = 0x02;
/// `AUICH_DCH`: dma halted.
pub const AUICH_DCH: u16 = 0x01;
/// `AUICH_ISTS_BITS`: the `%b` description of the status register.
pub const AUICH_ISTS_BITS: &[u8] = b"\x10\x01dch\x02celv\x03lvbci\x04bcis\x05fifoe";
/// `AUICH_PICB`: 16 bits.
pub const AUICH_PICB: usize = 0x08;
/// `AUICH_PIV`: 5 bits prefetched index value.
pub const AUICH_PIV: usize = 0x0a;
/// `AUICH_CTRL`: control.
pub const AUICH_CTRL: usize = 0x0b;
/// `AUICH_IOCE`: int on completion enable.
pub const AUICH_IOCE: u8 = 0x10;
/// `AUICH_FEIE`: fifo error int enable.
pub const AUICH_FEIE: u8 = 0x08;
/// `AUICH_LVBIE`: last valid buf int enable.
pub const AUICH_LVBIE: u8 = 0x04;
/// `AUICH_RR`: 1 - reset regs.
pub const AUICH_RR: u8 = 0x02;
/// `AUICH_RPBM`: 1 - run, 0 - pause.
pub const AUICH_RPBM: u8 = 0x01;

/// `AUICH_PCMI`.
pub const AUICH_PCMI: usize = 0x00;
/// `AUICH_PCMO`.
pub const AUICH_PCMO: usize = 0x10;
/// `AUICH_MICI`.
pub const AUICH_MICI: usize = 0x20;

/// `AUICH_GCTRL`.
pub const AUICH_GCTRL: usize = 0x2c;
/// `AUICH_SSM_78`: S/PDIF slots 7 and 8.
pub const AUICH_SSM_78: u32 = 0x4000_0000;
/// `AUICH_SSM_69`: S/PDIF slots 6 and 9.
pub const AUICH_SSM_69: u32 = 0x8000_0000;
/// `AUICH_SSM_1011`: S/PDIF slots 10 and 11.
pub const AUICH_SSM_1011: u32 = 0xc000_0000;
/// `AUICH_POM16`: PCM out precision 16bit.
pub const AUICH_POM16: u32 = 0x000000;
/// `AUICH_POM20`: PCM out precision 20bit.
pub const AUICH_POM20: u32 = 0x400000;
/// `AUICH_PCM246_MASK`.
pub const AUICH_PCM246_MASK: u32 = 0x300000;
/// `AUICH_PCM2`: 2ch output.
pub const AUICH_PCM2: u32 = 0x000000;
/// `AUICH_PCM4`: 4ch output.
pub const AUICH_PCM4: u32 = 0x100000;
/// `AUICH_PCM6`: 6ch output.
pub const AUICH_PCM6: u32 = 0x200000;
/// `AUICH_SIS_PCM246_MASK`: SiS 7012.
pub const AUICH_SIS_PCM246_MASK: u32 = 0x0000c0;
/// `AUICH_SIS_PCM2`: SiS 7012 2ch output.
pub const AUICH_SIS_PCM2: u32 = 0x000000;
/// `AUICH_SIS_PCM4`: SiS 7012 4ch output.
pub const AUICH_SIS_PCM4: u32 = 0x000040;
/// `AUICH_SIS_PCM6`: SiS 7012 6ch output.
pub const AUICH_SIS_PCM6: u32 = 0x000080;
/// `AUICH_S2RIE`: int when tertiary codec resume.
pub const AUICH_S2RIE: u32 = 0x40;
/// `AUICH_SRIE`: int when 2ndary codec resume.
pub const AUICH_SRIE: u32 = 0x20;
/// `AUICH_PRIE`: int when primary codec resume.
pub const AUICH_PRIE: u32 = 0x10;
/// `AUICH_ACLSO`: aclink shut off.
pub const AUICH_ACLSO: u32 = 0x08;
/// `AUICH_WRESET`: warm reset.
pub const AUICH_WRESET: u32 = 0x04;
/// `AUICH_CRESET`: cold reset.
pub const AUICH_CRESET: u32 = 0x02;
/// `AUICH_GIE`: gpi int enable.
pub const AUICH_GIE: u32 = 0x01;
/// `AUICH_GSTS`.
pub const AUICH_GSTS: usize = 0x30;
/// `AUICH_MD3`: pwr-dn semaphore for modem.
pub const AUICH_MD3: u32 = 0x20000;
/// `AUICH_AD3`: pwr-dn semaphore for audio.
pub const AUICH_AD3: u32 = 0x10000;
/// `AUICH_RCS`: read completion status.
pub const AUICH_RCS: u32 = 0x08000;
/// `AUICH_B3S12`: bit 3 of slot 12.
pub const AUICH_B3S12: u32 = 0x04000;
/// `AUICH_B2S12`: bit 2 of slot 12.
pub const AUICH_B2S12: u32 = 0x02000;
/// `AUICH_B1S12`: bit 1 of slot 12.
pub const AUICH_B1S12: u32 = 0x01000;
/// `AUICH_SRI`: secondary resume int.
pub const AUICH_SRI: u32 = 0x00800;
/// `AUICH_PRI`: primary resume int.
pub const AUICH_PRI: u32 = 0x00400;
/// `AUICH_SCR`: secondary codec ready.
pub const AUICH_SCR: u32 = 0x00200;
/// `AUICH_PCR`: primary codec ready.
pub const AUICH_PCR: u32 = 0x00100;
/// `AUICH_MINT`: mic in int.
pub const AUICH_MINT: u32 = 0x00080;
/// `AUICH_POINT`: pcm out int.
pub const AUICH_POINT: u32 = 0x00040;
/// `AUICH_PIINT`: pcm in int.
pub const AUICH_PIINT: u32 = 0x00020;
/// `AUICH_MOINT`: modem out int.
pub const AUICH_MOINT: u32 = 0x00004;
/// `AUICH_MIINT`: modem in int.
pub const AUICH_MIINT: u32 = 0x00002;
/// `AUICH_GSCI`: gpi status change.
pub const AUICH_GSCI: u32 = 0x00001;
/// `AUICH_GSTS_BITS`: the `%b` description of the global status register.
pub const AUICH_GSTS_BITS: &[u8] = b"\x10\x01gsci\x02miict\x03moint\x06piint\x07point\x08mint\x09pcr\x0ascr\x0bpri\x0csri\x0db1s12\x0eb2s12\x0fb3s12\x10rcs\x11ad3\x12md3";
/// `AUICH_CAS`: 1/8 bit.
pub const AUICH_CAS: usize = 0x34;
/// `AUICH_SEMATIMO`: us.
pub const AUICH_SEMATIMO: i32 = 1000;
/// `AUICH_RESETIMO`: us.
pub const AUICH_RESETIMO: i32 = 500000;

/// `ICH_SIS_NV_CTL`: some SiS/NVIDIA register. From Linux.
pub const ICH_SIS_NV_CTL: usize = 0x4c;
/// `ICH_SIS_CTL_UNMUTE`: un-mute the output.
pub const ICH_SIS_CTL_UNMUTE: u32 = 0x01;

/// `AUICH_DMALIST_MAX`: there are 32 buffer descriptors. Each can reference up to 2^16
/// 16-bit samples.
pub const AUICH_DMALIST_MAX: usize = 32;
/// `AUICH_DMASEG_MAX`.
pub const AUICH_DMASEG_MAX: usize = 65536 * 2;

/// `AUICH_DMAF_IOC`: 1-int on complete.
pub const AUICH_DMAF_IOC: u32 = 0x8000_0000;
/// `AUICH_DMAF_BUP`: 0-retrans last, 1-transmit 0.
pub const AUICH_DMAF_BUP: u32 = 0x4000_0000;

/// `AUICH_FIXED_RATE`.
pub const AUICH_FIXED_RATE: u32 = 48000;

/// `struct auich_dmalist`: a buffer descriptor, as the hardware reads it from memory.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AuichDmalist {
    /// `base`: the bus address of the block.
    pub base: u32,
    /// `len`: its length in samples, with `AUICH_DMAF_*` flags.
    pub len: u32,
}

/// `struct auich_dma`: the DMA memory of a ring (or the calibration buffer).
#[repr(C)]
pub struct AuichDma {
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
}

/// `struct auich_cdata`: the three descriptor lists, in one DMA page.
#[repr(C)]
pub struct AuichCdata {
    /// `ic_dmalist_pcmo`.
    pub ic_dmalist_pcmo: [AuichDmalist; AUICH_DMALIST_MAX],
    /// `ic_dmalist_pcmi`.
    pub ic_dmalist_pcmi: [AuichDmalist; AUICH_DMALIST_MAX],
    /// `ic_dmalist_mici`.
    pub ic_dmalist_mici: [AuichDmalist; AUICH_DMALIST_MAX],
}

/// `AUICH_PCMO_OFF(x)`: the offset of descriptor `x` of the PCM out list in the control
/// data.
pub const fn auich_pcmo_off(x: usize) -> usize {
    offset_of!(AuichCdata, ic_dmalist_pcmo) + x * size_of::<AuichDmalist>()
}

/// `AUICH_PCMI_OFF(x)`.
pub const fn auich_pcmi_off(x: usize) -> usize {
    offset_of!(AuichCdata, ic_dmalist_pcmi) + x * size_of::<AuichDmalist>()
}

/// `AUICH_MICI_OFF(x)`.
pub const fn auich_mici_off(x: usize) -> usize {
    offset_of!(AuichCdata, ic_dmalist_mici) + x * size_of::<AuichDmalist>()
}

/// `struct auich_softc`'s `struct auich_ring`: the state of one pipe. Protected by
/// `AUDIO_LOCK` once the pipe runs.
pub struct AuichRing {
    /// `qptr`.
    qptr: Cell<i32>,
    /// `dmalist`: the pipe's descriptors in the control data.
    dmalist: Cell<*mut AuichDmalist>,
    /// `start`: the ring's bus address.
    start: Cell<u32>,
    /// `p`: where the next block comes from.
    p: Cell<u32>,
    /// `end`.
    end: Cell<u32>,
    /// `blksize`.
    blksize: Cell<i32>,
    /// `intr`: audio(4)'s call-back for a finished block.
    intr: Cell<Option<AudioIntr>>,
    /// `arg`.
    arg: Cell<*mut c_void>,
    /// `running`.
    running: Cell<i32>,
    /// `size`.
    size: Cell<usize>,
    /// `ap`.
    ap: Cell<u32>,
}

/// `struct auich_softc`. Allocated zeroed by autoconf, so every member is valid as zero.
#[repr(C)]
pub struct AuichSoftc {
    /// `sc_dev`.
    sc_dev: Device,
    /// `sc_ih`.
    sc_ih: Cell<Option<NonNull<c_void>>>,

    /// `pci_id`.
    pci_id: Cell<Pcireg>,
    /// `iot`.
    iot: Cell<Option<BusSpaceTag>>,
    /// `iot_mix`.
    iot_mix: Cell<Option<BusSpaceTag>>,
    /// `mix_ioh`.
    mix_ioh: Cell<Option<BusSpaceHandle>>,
    /// `aud_ioh`.
    aud_ioh: Cell<Option<BusSpaceHandle>>,
    /// `dmat`.
    dmat: Cell<Option<BusDmaTag>>,

    /// `codec_if`: the codec's interface, which ac97(4) made.
    codec_if: Cell<*const Ac97CodecIf>,
    /// `host_if`.
    host_if: Cell<Ac97HostIf>,
    /// `sc_spdif`.
    sc_spdif: Cell<i32>,

    // dma scatter-gather buffer lists
    /// `sc_cddmamap`.
    sc_cddmamap: Cell<Option<&'static BusDmamap>>,
    /// `sc_cdata`.
    sc_cdata: Cell<*mut AuichCdata>,

    /// `pcmo`.
    pcmo: AuichRing,
    /// `pcmi`.
    pcmi: AuichRing,
    /// `mici`.
    mici: AuichRing,

    /// `sc_pdma`: play.
    sc_pdma: Cell<*mut AuichDma>,
    /// `sc_rdma`: record.
    sc_rdma: Cell<*mut AuichDma>,
    /// `sc_cdma`: calibrate.
    sc_cdma: Cell<*mut AuichDma>,

    /// `suspend`.
    suspend: Cell<i32>,
    /// `ext_ctrl`.
    ext_ctrl: Cell<u16>,
    /// `sc_sample_size`.
    sc_sample_size: Cell<i32>,
    /// `sc_sts_reg`.
    sc_sts_reg: Cell<usize>,
    /// `sc_dmamap_flags`.
    sc_dmamap_flags: Cell<i32>,
    /// `sc_ignore_codecready`.
    sc_ignore_codecready: Cell<i32>,
    /// `flags`: `AC97_HOST_*`.
    flags: Cell<u32>,
    /// `sc_ac97rate`.
    sc_ac97rate: Cell<i32>,

    // multi-channel control bits
    /// `sc_pcm246_mask`.
    sc_pcm246_mask: Cell<u32>,
    /// `sc_pcm2`.
    sc_pcm2: Cell<u32>,
    /// `sc_pcm4`.
    sc_pcm4: Cell<u32>,
    /// `sc_pcm6`.
    sc_pcm6: Cell<u32>,

    /// `last_rrate`.
    last_rrate: Cell<u32>,
    /// `last_prate`.
    last_prate: Cell<u32>,
    /// `last_pchan`.
    last_pchan: Cell<u32>,
}

// SAFETY: `#[repr(C)]` with the device first; the other members are `Cell`s of integers,
// pointers, `Option`s of references/handles, and plain structs of the same, all valid as zero
// bits.
unsafe impl Softc for AuichSoftc {}

impl AuichRing {
    /// `*q = ring->dmalist[idx]`: stores descriptor `idx` of the pipe's list.
    fn set_desc(&self, idx: usize, base: u32, len: u32) {
        let p = self.dmalist.get();
        if p.is_null() || idx >= AUICH_DMALIST_MAX {
            panic(format_args!("auich: bad descriptor {idx}"));
        }
        // SAFETY: `auich_alloc_cdata` made the lists (`AUICH_DMALIST_MAX` descriptors each) and
        // `idx` is inside them. They are DMA memory the hardware reads, so the stores are
        // volatile.
        unsafe {
            let q = p.add(idx);
            ptr::addr_of_mut!((*q).base).write_volatile(base);
            ptr::addr_of_mut!((*q).len).write_volatile(len);
        }
    }
}

impl AuichSoftc {
    /// `sc->iot`, `sc->aud_ioh`.
    fn aud(&self) -> (BusSpaceTag, BusSpaceHandle) {
        match (self.iot.get(), self.aud_ioh.get()) {
            (Some(t), Some(h)) => (t, h),
            _ => panic(format_args!("auich: bus master registers not mapped")),
        }
    }

    /// `sc->iot_mix`, `sc->mix_ioh`.
    fn mix(&self) -> (BusSpaceTag, BusSpaceHandle) {
        match (self.iot_mix.get(), self.mix_ioh.get()) {
            (Some(t), Some(h)) => (t, h),
            _ => panic(format_args!("auich: codec registers not mapped")),
        }
    }

    /// `sc->dmat`.
    fn dmat(&self) -> BusDmaTag {
        match self.dmat.get() {
            Some(t) => t,
            None => panic(format_args!("auich: no DMA tag")),
        }
    }

    /// `sc->codec_if`.
    fn codec(&self) -> &Ac97CodecIf {
        let p = self.codec_if.get();
        if p.is_null() {
            panic(format_args!("auich: no codec interface"));
        }
        // SAFETY: `auich_attach_codec` stored the interface of the codec's softc, which
        // ac97_attach never frees once it succeeded; the hardware interface is only
        // registered after that.
        unsafe { &*p }
    }

    /// `sc->sc_dev.dv_xname`.
    fn xname(&self) -> &str {
        self.sc_dev.xname()
    }

    /// `sc_cddma`: `sc_cddmamap->dm_segs[0].ds_addr`.
    fn sc_cddma(&self) -> usize {
        match self.sc_cddmamap.get() {
            Some(map) => map.dm_segs().first().map_or(0, |s| s.get().ds_addr),
            None => panic(format_args!("auich: no control data map")),
        }
    }

    /// `bus_space_read_1(sc->iot, sc->aud_ioh, off)`.
    fn read1(&self, off: usize) -> u8 {
        let (t, h) = self.aud();
        bus_space_read_1(t, h, off)
    }

    /// `bus_space_read_2(sc->iot, sc->aud_ioh, off)`.
    fn read2(&self, off: usize) -> u16 {
        let (t, h) = self.aud();
        bus_space_read_2(t, h, off)
    }

    /// `bus_space_read_4(sc->iot, sc->aud_ioh, off)`.
    fn read4(&self, off: usize) -> u32 {
        let (t, h) = self.aud();
        bus_space_read_4(t, h, off)
    }

    /// `bus_space_write_1(sc->iot, sc->aud_ioh, off, v)`.
    fn write1(&self, off: usize, v: u8) {
        let (t, h) = self.aud();
        bus_space_write_1(t, h, off, v);
    }

    /// `bus_space_write_2(sc->iot, sc->aud_ioh, off, v)`.
    fn write2(&self, off: usize, v: u16) {
        let (t, h) = self.aud();
        bus_space_write_2(t, h, off, v);
    }

    /// `bus_space_write_4(sc->iot, sc->aud_ioh, off, v)`.
    fn write4(&self, off: usize, v: u32) {
        let (t, h) = self.aud();
        bus_space_write_4(t, h, off, v);
    }
}

/// `auich_cd`.
pub static AUICH_CD: Cfdriver = Cfdriver::new(b"auich", DV_DULL, 0);

/// `auich_ca`.
pub static AUICH_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<AuichSoftc>(),
    ca_match: Some(auich_match),
    ca_attach: auich_attach,
    ca_detach: None,
    ca_activate: Some(auich_activate),
};

/// `struct auich_devtype`: a controller this driver supports.
pub struct AuichDevtype {
    /// `vendor`.
    pub vendor: u32,
    /// `product`.
    pub product: u32,
    /// `options`.
    pub options: i32,
    /// `name`.
    pub name: &'static str,
}

/// `auich_devices[]`.
pub static AUICH_DEVICES: [AuichDevtype; 22] = [
    AuichDevtype {
        vendor: PCI_VENDOR_INTEL,
        product: PCI_PRODUCT_INTEL_6300ESB_ACA,
        options: 0,
        name: "ESB",
    },
    AuichDevtype {
        vendor: PCI_VENDOR_INTEL,
        product: PCI_PRODUCT_INTEL_6321ESB_ACA,
        options: 0,
        name: "ESB2",
    },
    AuichDevtype {
        vendor: PCI_VENDOR_INTEL,
        product: PCI_PRODUCT_INTEL_82801AA_ACA,
        options: 0,
        name: "ICH",
    },
    AuichDevtype {
        vendor: PCI_VENDOR_INTEL,
        product: PCI_PRODUCT_INTEL_82801AB_ACA,
        options: 0,
        name: "ICH0",
    },
    AuichDevtype {
        vendor: PCI_VENDOR_INTEL,
        product: PCI_PRODUCT_INTEL_82801BA_ACA,
        options: 0,
        name: "ICH2",
    },
    AuichDevtype {
        vendor: PCI_VENDOR_INTEL,
        product: PCI_PRODUCT_INTEL_82801CA_ACA,
        options: 0,
        name: "ICH3",
    },
    AuichDevtype {
        vendor: PCI_VENDOR_INTEL,
        product: PCI_PRODUCT_INTEL_82801DB_ACA,
        options: 0,
        name: "ICH4",
    },
    AuichDevtype {
        vendor: PCI_VENDOR_INTEL,
        product: PCI_PRODUCT_INTEL_82801EB_ACA,
        options: 0,
        name: "ICH5",
    },
    AuichDevtype {
        vendor: PCI_VENDOR_INTEL,
        product: PCI_PRODUCT_INTEL_82801FB_ACA,
        options: 0,
        name: "ICH6",
    },
    AuichDevtype {
        vendor: PCI_VENDOR_INTEL,
        product: PCI_PRODUCT_INTEL_82801GB_ACA,
        options: 0,
        name: "ICH7",
    },
    AuichDevtype {
        vendor: PCI_VENDOR_INTEL,
        product: PCI_PRODUCT_INTEL_82440MX_ACA,
        options: 0,
        name: "440MX",
    },
    AuichDevtype {
        vendor: PCI_VENDOR_SIS,
        product: PCI_PRODUCT_SIS_7012_ACA,
        options: 0,
        name: "SiS7012",
    },
    AuichDevtype {
        vendor: PCI_VENDOR_NVIDIA,
        product: PCI_PRODUCT_NVIDIA_NFORCE_ACA,
        options: 0,
        name: "nForce",
    },
    AuichDevtype {
        vendor: PCI_VENDOR_NVIDIA,
        product: PCI_PRODUCT_NVIDIA_NFORCE2_ACA,
        options: 0,
        name: "nForce2",
    },
    AuichDevtype {
        vendor: PCI_VENDOR_NVIDIA,
        product: PCI_PRODUCT_NVIDIA_NFORCE2_400_ACA,
        options: 0,
        name: "nForce2",
    },
    AuichDevtype {
        vendor: PCI_VENDOR_NVIDIA,
        product: PCI_PRODUCT_NVIDIA_NFORCE3_ACA,
        options: 0,
        name: "nForce3",
    },
    AuichDevtype {
        vendor: PCI_VENDOR_NVIDIA,
        product: PCI_PRODUCT_NVIDIA_NFORCE3_250_ACA,
        options: 0,
        name: "nForce3",
    },
    AuichDevtype {
        vendor: PCI_VENDOR_NVIDIA,
        product: PCI_PRODUCT_NVIDIA_NFORCE4_AC,
        options: 0,
        name: "nForce4",
    },
    AuichDevtype {
        vendor: PCI_VENDOR_NVIDIA,
        product: PCI_PRODUCT_NVIDIA_MCP04_AC97,
        options: 0,
        name: "MCP04",
    },
    AuichDevtype {
        vendor: PCI_VENDOR_NVIDIA,
        product: PCI_PRODUCT_NVIDIA_MCP51_ACA,
        options: 0,
        name: "MCP51",
    },
    AuichDevtype {
        vendor: PCI_VENDOR_AMD,
        product: PCI_PRODUCT_AMD_PBC768_ACA,
        options: 0,
        name: "AMD768",
    },
    AuichDevtype {
        vendor: PCI_VENDOR_AMD,
        product: PCI_PRODUCT_AMD_8111_ACA,
        options: 0,
        name: "AMD8111",
    },
];

/// `auich_hw_if`: what audio(4) calls.
pub static AUICH_HW_IF: AudioHwIf = AudioHwIf {
    open: Some(auich_open),
    close: Some(auich_close),
    set_params: Some(auich_set_params),
    round_blocksize: Some(auich_round_blocksize),
    halt_output: Some(auich_halt_output),
    halt_input: Some(auich_halt_input),
    set_port: Some(auich_set_port),
    get_port: Some(auich_get_port),
    query_devinfo: Some(auich_query_devinfo),
    allocm: Some(auich_allocm),
    freem: Some(auich_freem),
    round_buffersize: Some(auich_round_buffersize),
    trigger_output: Some(auich_trigger_output),
    trigger_input: Some(auich_trigger_input),
    ..AudioHwIf::new()
};

/// The softc of a `hdl` that audio(4) or ac97(4) passes back.
///
/// # Safety
///
/// `hdl` is the softc given to `audio_attach_mi` or to ac97(4)'s host interface, which is a
/// live `AuichSoftc` (softcs are never freed while their device exists).
unsafe fn sc_of<'a>(hdl: *mut c_void) -> &'a AuichSoftc {
    // SAFETY: the contract.
    unsafe { &*hdl.cast::<AuichSoftc>() }
}

/// `auich_match`.
pub fn auich_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: pci hands its children pci_attach_args.
    let pa = unsafe { &*aux.cast::<PciAttachArgs>() };

    for d in AUICH_DEVICES.iter().rev() {
        if pci_vendor(pa.pa_id) == d.vendor && pci_product(pa.pa_id) == d.product {
            return 1;
        }
    }

    0
}

/// Whether the function is one of the ICH4 to ICH7 audio controllers, which have native
/// (memory) mixer and bus master BARs.
fn is_ich4_to_7(pa: &PciAttachArgs) -> bool {
    let product = pci_product(pa.pa_id);
    pci_vendor(pa.pa_id) == PCI_VENDOR_INTEL
        && (product == PCI_PRODUCT_INTEL_82801DB_ACA
            || product == PCI_PRODUCT_INTEL_82801EB_ACA
            || product == PCI_PRODUCT_INTEL_82801FB_ACA
            || product == PCI_PRODUCT_INTEL_82801GB_ACA)
}

/// `auich_attach`.
pub fn auich_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `self_` was made for `auich_ca`, whose softc is an `AuichSoftc`; softcs are
    // never freed while the device exists, so it may be borrowed for 'static.
    let sc: &'static AuichSoftc = unsafe { &*ptr::from_ref(self_.softc::<AuichSoftc>()) };
    // SAFETY: pci hands its children pci_attach_args.
    let pa = unsafe { &*aux.cast::<PciAttachArgs>() };
    let pc: PciChipsetTag = pa.pa_pc;
    let product = pci_product(pa.pa_id);

    let (iot_mix, mix_ioh, mix_size);
    let (iot, aud_ioh, aud_size);

    if is_ich4_to_7(pa) {
        // Use native mode for ICH4/ICH5/ICH6/ICH7
        match pci_mapreg_map(pa, AUICH_MMBAR, PCI_MAPREG_TYPE_MEM, 0, 0) {
            Ok((t, h, _, size)) => (iot_mix, mix_ioh, mix_size) = (t, h, size),
            Err(_) => {
                let csr = pci_conf_read(pc, pa.pa_tag, AUICH_CFG);
                pci_conf_write(pc, pa.pa_tag, AUICH_CFG, csr | AUICH_CFG_IOSE);
                match pci_mapreg_map(pa, AUICH_NAMBAR, PCI_MAPREG_TYPE_IO, 0, 0) {
                    Ok((t, h, _, size)) => (iot_mix, mix_ioh, mix_size) = (t, h, size),
                    Err(_) => {
                        printf(format_args!(": can't map codec mem/io space\n"));
                        return;
                    }
                }
            }
        }

        match pci_mapreg_map(pa, AUICH_MBBAR, PCI_MAPREG_TYPE_MEM, 0, 0) {
            Ok((t, h, _, size)) => (iot, aud_ioh, aud_size) = (t, h, size),
            Err(_) => {
                let csr = pci_conf_read(pc, pa.pa_tag, AUICH_CFG);
                pci_conf_write(pc, pa.pa_tag, AUICH_CFG, csr | AUICH_CFG_IOSE);
                match pci_mapreg_map(pa, AUICH_NABMBAR, PCI_MAPREG_TYPE_IO, 0, 0) {
                    Ok((t, h, _, size)) => (iot, aud_ioh, aud_size) = (t, h, size),
                    Err(_) => {
                        printf(format_args!(": can't map device mem/io space\n"));
                        // fail_unmap_mix
                        bus_space_unmap(iot_mix, mix_ioh, mix_size);
                        return;
                    }
                }
            }
        }
    } else {
        match pci_mapreg_map(pa, AUICH_NAMBAR, PCI_MAPREG_TYPE_IO, 0, 0) {
            Ok((t, h, _, size)) => (iot_mix, mix_ioh, mix_size) = (t, h, size),
            Err(_) => {
                printf(format_args!(": can't map codec i/o space\n"));
                return;
            }
        }

        match pci_mapreg_map(pa, AUICH_NABMBAR, PCI_MAPREG_TYPE_IO, 0, 0) {
            Ok((t, h, _, size)) => (iot, aud_ioh, aud_size) = (t, h, size),
            Err(_) => {
                printf(format_args!(": can't map device i/o space\n"));
                // fail_unmap_mix
                bus_space_unmap(iot_mix, mix_ioh, mix_size);
                return;
            }
        }
    }
    sc.iot_mix.set(Some(iot_mix));
    sc.mix_ioh.set(Some(mix_ioh));
    sc.iot.set(Some(iot));
    sc.aud_ioh.set(Some(aud_ioh));
    sc.dmat.set(Some(pa.pa_dmat));
    sc.pci_id.set(pa.pa_id);

    // fail_unmap: both register windows.
    let fail_unmap = || {
        bus_space_unmap(iot, aud_ioh, aud_size);
        // fail_unmap_mix
        bus_space_unmap(iot_mix, mix_ioh, mix_size);
    };

    let Some(ih) = pci_intr_map(pa) else {
        printf(format_args!(": can't map interrupt\n"));
        fail_unmap();
        return;
    };
    let intrstr = pci_intr_string(pc, ih);
    let ihc = pci_intr_establish(
        pc,
        ih,
        IPL_AUDIO | IPL_MPSAFE,
        auich_intr,
        ptr::from_ref(sc).cast_mut().cast(),
        sc.xname(),
    );
    sc.sc_ih.set(ihc);
    let Some(ihc) = ihc else {
        printf(format_args!(": can't establish interrupt"));
        printf(format_args!(" at {intrstr}"));
        printf(format_args!("\n"));
        fail_unmap();
        return;
    };

    let name = AUICH_DEVICES
        .iter()
        .rev()
        .find(|d| d.product == product)
        .map_or("?", |d| d.name);

    printf(format_args!(": {intrstr}, {name}\n"));

    // SiS 7012 needs special handling
    if pci_vendor(pa.pa_id) == PCI_VENDOR_SIS && product == PCI_PRODUCT_SIS_7012_ACA {
        sc.sc_sts_reg.set(AUICH_PICB);
        sc.sc_sample_size.set(1);
        sc.sc_pcm246_mask.set(AUICH_SIS_PCM246_MASK);
        sc.sc_pcm2.set(AUICH_SIS_PCM2);
        sc.sc_pcm4.set(AUICH_SIS_PCM4);
        sc.sc_pcm6.set(AUICH_SIS_PCM6);
        // un-mute output
        sc.write4(
            ICH_SIS_NV_CTL,
            sc.read4(ICH_SIS_NV_CTL) | ICH_SIS_CTL_UNMUTE,
        );
    } else {
        sc.sc_sts_reg.set(AUICH_STS);
        sc.sc_sample_size.set(2);
        sc.sc_pcm246_mask.set(AUICH_PCM246_MASK);
        sc.sc_pcm2.set(AUICH_PCM2);
        sc.sc_pcm4.set(AUICH_PCM4);
        sc.sc_pcm6.set(AUICH_PCM6);
    }

    // Workaround for a 440MX B-stepping erratum
    sc.sc_dmamap_flags.set(BUS_DMA_COHERENT);
    if pci_vendor(pa.pa_id) == PCI_VENDOR_INTEL && product == PCI_PRODUCT_INTEL_82440MX_ACA {
        sc.sc_dmamap_flags
            .set(sc.sc_dmamap_flags.get() | BUS_DMA_NOCACHE);
        printf(format_args!("{}: DMA bug workaround enabled\n", sc.xname()));
    }

    // fail_disestablish_intr, then fail_unmap.
    let fail_disestablish_intr = || {
        // SAFETY: the cookie came from pci_intr_establish above and is not used again.
        unsafe { pci_intr_disestablish(pc, ihc) };
        fail_unmap();
    };

    // Set up DMA lists.
    sc.pcmo.qptr.set(0);
    sc.pcmi.qptr.set(0);
    sc.mici.qptr.set(0);
    if auich_alloc_cdata(sc).is_err() {
        fail_disestablish_intr();
        return;
    }

    // Reset codec and AC'97
    let arg: *mut c_void = ptr::from_ref(sc).cast_mut().cast();
    // SAFETY: `arg` is this softc, which every host method takes.
    unsafe { auich_reset_codec(arg) };
    let status = sc.read4(AUICH_GSTS);
    if status & AUICH_PCR == 0 {
        // reset failure
        if is_ich4_to_7(pa) {
            // MSI 845G Max never return AUICH_PCR
            sc.sc_ignore_codecready.set(1);
        } else {
            printf(format_args!("{}: reset failed!\n", sc.xname()));
            return;
        }
    }

    sc.host_if.set(Ac97HostIf {
        arg,
        attach: Some(auich_attach_codec),
        read: Some(auich_read_codec),
        write: Some(auich_write_codec),
        reset: Some(auich_reset_codec),
        flags: Some(auich_flags_codec),
        spdif_event: Some(auich_spdif_event),
    });
    if self_.cfdata().cf_flags & 0x0001 != 0 {
        sc.flags.set(AC97_HOST_SWAPPED_CHANNELS);
    }

    if ac97_attach(&sc.host_if.get()).is_err() {
        fail_disestablish_intr();
        return;
    }
    let codec = sc.codec();
    (codec.vtbl.unlock)(codec);

    audio_attach_mi(&AUICH_HW_IF, arg, ptr::null_mut(), &sc.sc_dev);

    // Watch for power changes
    sc.suspend.set(DVACT_RESUME);

    sc.sc_ac97rate.set(-1);
}

/// `auich_activate`.
pub fn auich_activate(self_: &Device, act: i32) -> Result<(), Errno> {
    // SAFETY: `self_` was made for `auich_ca`.
    let sc = unsafe { self_.softc::<AuichSoftc>() };

    if act == DVACT_RESUME {
        auich_resume(sc);
    }
    config_activate_children(self_, act)
}

/// `auich_read_codec`: the codec register `reg`, after waiting for the access semaphore.
///
/// # Safety
///
/// `v` is the softc this host interface belongs to.
pub unsafe fn auich_read_codec(v: *mut c_void, reg: u8, val: &mut u16) -> Result<(), Errno> {
    // SAFETY: the contract.
    let sc = unsafe { sc_of(v) };

    // wait for an access semaphore
    let mut i = AUICH_SEMATIMO;
    loop {
        let more = i != 0;
        i -= 1;
        if !(more && sc.read1(AUICH_CAS) & 1 != 0) {
            break;
        }
        delay(1);
    }

    if sc.sc_ignore_codecready.get() == 0 && i < 0 {
        return Err(Errno::EIO);
    }

    let (t, h) = sc.mix();
    *val = bus_space_read_2(t, h, usize::from(reg));
    Ok(())
}

/// `auich_write_codec`: the codec register `reg`, after waiting for the access semaphore.
///
/// # Safety
///
/// `v` is the softc this host interface belongs to.
pub unsafe fn auich_write_codec(v: *mut c_void, reg: u8, val: u16) -> Result<(), Errno> {
    // SAFETY: the contract.
    let sc = unsafe { sc_of(v) };

    // wait for an access semaphore
    let mut i = AUICH_SEMATIMO;
    loop {
        let more = i != 0;
        i -= 1;
        if !(more && sc.read1(AUICH_CAS) & 1 != 0) {
            break;
        }
        delay(1);
    }

    if sc.sc_ignore_codecready.get() != 0 || i >= 0 {
        let (t, h) = sc.mix();
        bus_space_write_2(t, h, usize::from(reg), val);
        Ok(())
    } else {
        Err(Errno::EIO)
    }
}

/// `auich_attach_codec`: ac97(4) hands over the codec interface.
///
/// # Safety
///
/// `v` is the softc this host interface belongs to.
pub unsafe fn auich_attach_codec(v: *mut c_void, cif: &Ac97CodecIf) -> Result<(), Errno> {
    // SAFETY: the contract.
    let sc = unsafe { sc_of(v) };

    sc.codec_if.set(ptr::from_ref(cif));
    Ok(())
}

/// `auich_reset_codec`: resets the AC-link and waits for the primary codec to be ready.
///
/// # Safety
///
/// `v` is the softc this host interface belongs to.
pub unsafe fn auich_reset_codec(v: *mut c_void) {
    // SAFETY: the contract.
    let sc = unsafe { sc_of(v) };

    let mut control = sc.read4(AUICH_GCTRL);
    control &= !(AUICH_ACLSO | sc.sc_pcm246_mask.get());
    control |= if control & AUICH_CRESET != 0 {
        AUICH_WRESET
    } else {
        AUICH_CRESET
    };
    sc.write4(AUICH_GCTRL, control);

    let mut i = AUICH_RESETIMO;
    loop {
        let more = i != 0;
        i -= 1;
        if !(more && sc.read4(AUICH_GSTS) & AUICH_PCR == 0) {
            break;
        }
        delay(1);
    }
    // `i < 0` is the C's "reset_codec timeout" `DPRINTF`.
}

/// `auich_flags_codec`.
///
/// # Safety
///
/// `v` is the softc this host interface belongs to.
pub unsafe fn auich_flags_codec(v: *mut c_void) -> u32 {
    // SAFETY: the contract.
    let sc = unsafe { sc_of(v) };

    sc.flags.get()
}

/// `auich_spdif_event`.
///
/// # Safety
///
/// `v` is the softc this host interface belongs to.
pub unsafe fn auich_spdif_event(v: *mut c_void, flag: i32) {
    // SAFETY: the contract.
    let sc = unsafe { sc_of(v) };
    sc.sc_spdif.set(flag);
}

/// `auich_open`.
///
/// # Safety
///
/// `v` is the `hdl` given to `audio_attach_mi`.
pub unsafe fn auich_open(v: *mut c_void, _flags: i32) -> Result<(), Errno> {
    // SAFETY: the contract.
    let sc = unsafe { sc_of(v) };

    if sc.sc_ac97rate.get() == -1 {
        sc.sc_ac97rate.set(auich_calibrate(sc) as i32);
    }

    let codec = sc.codec();
    (codec.vtbl.lock)(codec);

    Ok(())
}

/// `auich_close`.
///
/// # Safety
///
/// `v` is the `hdl` given to `audio_attach_mi`.
pub unsafe fn auich_close(v: *mut c_void) {
    // SAFETY: the contract.
    let sc = unsafe { sc_of(v) };

    let codec = sc.codec();
    (codec.vtbl.unlock)(codec);
}

/// `auich_set_params`: 16-bit linear, 2, 4 or 6 channels the codec can do, rates scaled by
/// the measured AC-link rate.
///
/// # Safety
///
/// `v` is the `hdl` given to `audio_attach_mi`.
pub unsafe fn auich_set_params(
    v: *mut c_void,
    setmode: i32,
    _usemode: i32,
    play: &mut AudioParams,
    rec: &mut AudioParams,
) -> Result<(), Errno> {
    // SAFETY: the contract.
    let sc = unsafe { sc_of(v) };
    let codec = sc.codec();

    if setmode & AUMODE_PLAY != 0 {
        // only 16-bit 48kHz slinear_le if s/pdif enabled
        if sc.sc_spdif.get() != 0 {
            play.sample_rate = 48000;
            play.precision = 16;
            play.encoding = AUDIO_ENCODING_SLINEAR_LE;
        }
    }
    if setmode & AUMODE_PLAY != 0 {
        play.precision = 16;
        match play.encoding {
            AUDIO_ENCODING_SLINEAR_LE => {
                if play.channels > 6 {
                    play.channels = 6;
                }
                if play.channels > 1 {
                    play.channels &= !1;
                }
                match play.channels {
                    1 => play.channels = 2,
                    2 => {}
                    4 => {
                        let ext_id = (codec.vtbl.get_caps)(codec);
                        if ext_id & AC97_EXT_AUDIO_SDAC == 0 {
                            play.channels = 2;
                        }
                    }
                    6 => {
                        let ext_id = (codec.vtbl.get_caps)(codec);
                        if ext_id & AC97_BITS_6CH != AC97_BITS_6CH {
                            play.channels = 2;
                        }
                    }
                    _ => return Err(Errno::EINVAL),
                }
            }
            _ => return Err(Errno::EINVAL),
        }
        play.bps = audio_bps(play.precision);
        play.msb = 1;

        // `u_int orate, adj_rate`: the rate is truncated to 32 bits.
        let orate = play.sample_rate as u32;
        let mut adj_rate = orate;
        if sc.sc_ac97rate.get() != 0 {
            adj_rate = orate.wrapping_mul(AUICH_FIXED_RATE) / (sc.sc_ac97rate.get() as u32);
        }

        play.sample_rate = u64::from(adj_rate);
        sc.last_prate.set(adj_rate);

        (codec.vtbl.set_rate)(
            codec,
            i32::from(AC97_REG_PCM_LFE_DAC_RATE),
            &mut play.sample_rate,
        )?;

        play.sample_rate = u64::from(adj_rate);
        (codec.vtbl.set_rate)(
            codec,
            i32::from(AC97_REG_PCM_SURR_DAC_RATE),
            &mut play.sample_rate,
        )?;

        play.sample_rate = u64::from(adj_rate);
        (codec.vtbl.set_rate)(
            codec,
            i32::from(AC97_REG_PCM_FRONT_DAC_RATE),
            &mut play.sample_rate,
        )?;

        if play.sample_rate == u64::from(adj_rate) {
            play.sample_rate = u64::from(orate);
        }

        let mut control = sc.read4(AUICH_GCTRL);
        control &= !sc.sc_pcm246_mask.get();
        if play.channels == 4 {
            control |= sc.sc_pcm4.get();
        } else if play.channels == 6 {
            control |= sc.sc_pcm6.get();
        }
        sc.write4(AUICH_GCTRL, control);

        sc.last_pchan.set(play.channels);
    }

    if setmode & AUMODE_RECORD != 0 {
        rec.channels = 2;
        rec.precision = 16;
        rec.encoding = AUDIO_ENCODING_SLINEAR_LE;
        rec.bps = audio_bps(rec.precision);
        rec.msb = 1;

        let orate = rec.sample_rate as u32;
        if sc.sc_ac97rate.get() != 0 {
            rec.sample_rate =
                u64::from(orate.wrapping_mul(AUICH_FIXED_RATE) / (sc.sc_ac97rate.get() as u32));
        }
        sc.last_rrate.set(rec.sample_rate as u32);
        (codec.vtbl.set_rate)(
            codec,
            i32::from(AC97_REG_PCM_LR_ADC_RATE),
            &mut rec.sample_rate,
        )?;
        rec.sample_rate = u64::from(orate);
    }

    Ok(())
}

/// `auich_round_blocksize`: blocks are multiples of 64 bytes.
///
/// # Safety
///
/// `v` is the `hdl` given to `audio_attach_mi`.
pub unsafe fn auich_round_blocksize(_v: *mut c_void, blk: i32) -> i32 {
    (blk + 0x3f) & !0x3f
}

/// `auich_halt_pipe`: stops the pipe's DMA engine, clears its status and resets its
/// registers. Called with `AUDIO_LOCK` held.
pub fn auich_halt_pipe(sc: &AuichSoftc, pipe: usize, ring: &AuichRing) {
    let sts_reg = sc.sc_sts_reg.get();

    sc.write1(pipe + AUICH_CTRL, 0);

    // wait for DMA halted and clear interrupt / event bits if needed
    for _ in 0..1000 {
        let sts = sc.read2(pipe + sts_reg);
        if sts & (AUICH_CELV | AUICH_LVBCI | AUICH_BCIS | AUICH_FIFOE) != 0 {
            sc.write2(
                pipe + sts_reg,
                AUICH_CELV | AUICH_LVBCI | AUICH_BCIS | AUICH_FIFOE,
            );
        }
        if sts & AUICH_DCH != 0 {
            break;
        }
        delay(100);
    }
    sc.write1(pipe + AUICH_CTRL, AUICH_RR);

    ring.running.set(0);
}

/// `auich_halt_output`.
///
/// # Safety
///
/// `v` is the `hdl` given to `audio_attach_mi`.
pub unsafe fn auich_halt_output(v: *mut c_void) -> Result<(), Errno> {
    // SAFETY: the contract.
    let sc = unsafe { sc_of(v) };

    mtx_enter(&AUDIO_LOCK);
    auich_halt_pipe(sc, AUICH_PCMO, &sc.pcmo);

    sc.pcmo.intr.set(None);
    mtx_leave(&AUDIO_LOCK);
    Ok(())
}

/// `auich_halt_input`.
///
/// # Safety
///
/// `v` is the `hdl` given to `audio_attach_mi`.
pub unsafe fn auich_halt_input(v: *mut c_void) -> Result<(), Errno> {
    // SAFETY: the contract.
    let sc = unsafe { sc_of(v) };

    // XXX halt both unless known otherwise
    mtx_enter(&AUDIO_LOCK);
    auich_halt_pipe(sc, AUICH_PCMI, &sc.pcmi);
    auich_halt_pipe(sc, AUICH_MICI, &sc.mici);

    sc.pcmi.intr.set(None);
    mtx_leave(&AUDIO_LOCK);
    Ok(())
}

/// `auich_set_port`.
///
/// # Safety
///
/// `v` is the `hdl` given to `audio_attach_mi`.
pub unsafe fn auich_set_port(v: *mut c_void, cp: &mut MixerCtrl) -> Result<(), Errno> {
    // SAFETY: the contract.
    let sc = unsafe { sc_of(v) };
    let codec = sc.codec();
    (codec.vtbl.mixer_set_port)(codec, cp)
}

/// `auich_get_port`.
///
/// # Safety
///
/// `v` is the `hdl` given to `audio_attach_mi`.
pub unsafe fn auich_get_port(v: *mut c_void, cp: &mut MixerCtrl) -> Result<(), Errno> {
    // SAFETY: the contract.
    let sc = unsafe { sc_of(v) };
    let codec = sc.codec();
    (codec.vtbl.mixer_get_port)(codec, cp)
}

/// `auich_query_devinfo`.
///
/// # Safety
///
/// `v` is the `hdl` given to `audio_attach_mi`.
pub unsafe fn auich_query_devinfo(v: *mut c_void, dp: &mut MixerDevinfo) -> Result<(), Errno> {
    // SAFETY: the contract.
    let sc = unsafe { sc_of(v) };
    let codec = sc.codec();
    (codec.vtbl.query_devinfo)(codec, dp)
}

/// `auich_allocm`: DMA memory for a ring, one segment. `direction` picks the softc member
/// that remembers it: `AUMODE_PLAY`, `AUMODE_RECORD`, or the calibration buffer.
///
/// # Safety
///
/// `v` is the `hdl` given to `audio_attach_mi`.
pub unsafe fn auich_allocm(
    v: *mut c_void,
    direction: i32,
    size: usize,
    pool: i32,
    flags: i32,
) -> Option<NonNull<u8>> {
    // SAFETY: the contract.
    let sc = unsafe { sc_of(v) };

    // can only use 1 segment
    if size > AUICH_DMASEG_MAX {
        return None;
    }

    let p = malloc(size_of::<AuichDma>(), pool, flags | M_ZERO)?;
    let pd = p.cast::<AuichDma>();
    // SAFETY: a fresh zeroed allocation of an `AuichDma`, valid as zero bits.
    let dma = unsafe { pd.as_ref() };

    if auich_allocmem(sc, size, PAGE_SIZE, dma).is_err() {
        free(p, pool, size_of::<AuichDma>());
        return None;
    }

    if direction == AUMODE_PLAY {
        sc.sc_pdma.set(pd.as_ptr());
    } else if direction == AUMODE_RECORD {
        sc.sc_rdma.set(pd.as_ptr());
    } else {
        sc.sc_cdma.set(pd.as_ptr());
    }

    NonNull::new(dma.addr.get())
}

/// `auich_freem`: gives back what `auich_allocm` made for `ptr`.
///
/// # Safety
///
/// `v` is the `hdl` given to `audio_attach_mi`.
pub unsafe fn auich_freem(v: *mut c_void, addr: NonNull<u8>, pool: i32) {
    // SAFETY: the contract.
    let sc = unsafe { sc_of(v) };

    // Each of the three pointers is NULL or a live `AuichDma` made by `auich_allocm`.
    let matches = |slot: &Cell<*mut AuichDma>| -> Option<NonNull<AuichDma>> {
        let p = NonNull::new(slot.get())?;
        // SAFETY: see above.
        if unsafe { p.as_ref() }.addr.get() == addr.as_ptr() {
            slot.set(core::ptr::null_mut());
            Some(p)
        } else {
            None
        }
    };
    let Some(p) = matches(&sc.sc_pdma)
        .or_else(|| matches(&sc.sc_rdma))
        .or_else(|| matches(&sc.sc_cdma))
    else {
        return;
    };

    // SAFETY: made by auich_allocm and no longer reachable from the softc.
    auich_freemem(sc, unsafe { p.as_ref() });
    free(p.cast(), pool, size_of::<AuichDma>());
}

/// `auich_round_buffersize`: at most 32 descriptors of the largest segment.
///
/// # Safety
///
/// `v` is the `hdl` given to `audio_attach_mi`.
pub unsafe fn auich_round_buffersize(_v: *mut c_void, _direction: i32, size: usize) -> usize {
    size.min(AUICH_DMALIST_MAX * AUICH_DMASEG_MAX)
}

/// `auich_intr`: block-complete interrupts of the three pipes. Takes `AUDIO_LOCK`, which
/// audio(4)'s call-backs need.
fn auich_intr(v: *mut c_void) -> i32 {
    // SAFETY: the handler is established with the softc as its argument.
    let sc = unsafe { sc_of(v) };
    let sts_reg = sc.sc_sts_reg.get();
    let mut ret = 0;

    mtx_enter(&AUDIO_LOCK);
    let gsts = sc.read4(AUICH_GSTS);

    if gsts & AUICH_POINT != 0 {
        let sts = sc.read2(AUICH_PCMO + sts_reg);

        if sts & AUICH_BCIS != 0 {
            auich_intr_pipe(sc, AUICH_PCMO, &sc.pcmo);
        }

        // int ack
        sc.write2(AUICH_PCMO + sts_reg, sts & (AUICH_BCIS | AUICH_FIFOE));
        sc.write4(AUICH_GSTS, AUICH_POINT);
        ret += 1;
    }

    if gsts & AUICH_PIINT != 0 {
        let sts = sc.read2(AUICH_PCMI + sts_reg);

        if sts & AUICH_BCIS != 0 {
            auich_intr_pipe(sc, AUICH_PCMI, &sc.pcmi);
        }

        // int ack
        sc.write2(AUICH_PCMI + sts_reg, sts & (AUICH_BCIS | AUICH_FIFOE));
        sc.write4(AUICH_GSTS, AUICH_PIINT);
        ret += 1;
    }

    if gsts & AUICH_MINT != 0 {
        let sts = sc.read2(AUICH_MICI + sts_reg);
        if sts & AUICH_BCIS != 0 {
            auich_intr_pipe(sc, AUICH_MICI, &sc.mici);
        }

        // int ack (the C adds where the other pipes mask)
        sc.write2(
            AUICH_MICI + sts_reg,
            sts.wrapping_add(AUICH_BCIS | AUICH_FIFOE),
        );

        sc.write4(AUICH_GSTS, AUICH_MINT);
        ret += 1;
    }
    mtx_leave(&AUDIO_LOCK);
    ret
}

/// `auich_trigger_pipe`: fills all 32 descriptors of the pipe with consecutive blocks of the
/// ring and starts the engine. Called with `AUDIO_LOCK` held.
pub fn auich_trigger_pipe(sc: &AuichSoftc, pipe: usize, ring: &AuichRing) {
    let blksize = ring.blksize.get();
    let mut qptr = i32::from(sc.read1(pipe + AUICH_CIV));
    let oqptr0 = qptr;

    // XXX remove this when no one reports problems
    let oqptr = if oqptr0 >= AUICH_DMALIST_MAX as i32 {
        printf(format_args!("{}: Unexpected CIV: {}\n", sc.xname(), oqptr0));
        qptr = 0;
        0
    } else {
        oqptr0
    };

    loop {
        ring.set_desc(
            qptr as usize,
            ring.p.get(),
            ((blksize / sc.sc_sample_size.get()) as u32) | AUICH_DMAF_IOC,
        );

        ring.p.set(ring.p.get().wrapping_add(blksize as u32));
        if ring.p.get() >= ring.end.get() {
            ring.p.set(ring.start.get());
        }

        qptr = (qptr + 1) & i32::from(AUICH_LVI_MASK);
        if qptr == oqptr {
            break;
        }
    }

    ring.qptr.set(qptr);

    sc.write1(
        pipe + AUICH_LVI,
        ((qptr - 1) & i32::from(AUICH_LVI_MASK)) as u8,
    );
    sc.write1(pipe + AUICH_CTRL, AUICH_IOCE | AUICH_FEIE | AUICH_RPBM);

    ring.running.set(1);
}

/// `auich_intr_pipe`: refills the descriptors the engine finished and tells audio(4) about
/// each block. Called with `AUDIO_LOCK` held.
pub fn auich_intr_pipe(sc: &AuichSoftc, pipe: usize, ring: &AuichRing) {
    let blksize = ring.blksize.get();
    let mut qptr = ring.qptr.get();
    let nqptr = i32::from(sc.read1(pipe + AUICH_CIV));

    while qptr != nqptr {
        ring.set_desc(
            qptr as usize,
            ring.p.get(),
            ((blksize / sc.sc_sample_size.get()) as u32) | AUICH_DMAF_IOC,
        );

        ring.p.set(ring.p.get().wrapping_add(blksize as u32));
        if ring.p.get() >= ring.end.get() {
            ring.p.set(ring.start.get());
        }

        qptr = (qptr + 1) & i32::from(AUICH_LVI_MASK);
        match ring.intr.get() {
            // SAFETY: the call-back and argument audio(4) handed to trigger_*; `AUDIO_LOCK`
            // is held.
            Some(intr) => unsafe { intr(ring.arg.get()) },
            None => {
                printf(format_args!("auich_intr: got progress with intr==NULL\n"));
            }
        }

        ring.ap.set(ring.ap.get().wrapping_add(blksize as u32));
        if ring.ap.get() as usize >= ring.size.get() {
            ring.ap.set(0);
        }
    }
    ring.qptr.set(qptr);

    sc.write1(
        pipe + AUICH_LVI,
        ((qptr - 1) & i32::from(AUICH_LVI_MASK)) as u8,
    );
}

/// What `trigger_output` and `trigger_input` hand to [`auich_trigger`]: the ring's size in
/// bytes, the block size, and audio(4)'s call-back with its argument.
struct Trigger {
    /// `end - start`.
    size: usize,
    /// `blksize`.
    blksize: i32,
    /// `intr`.
    intr: AudioIntr,
    /// `arg`.
    arg: *mut c_void,
}

/// Starts a ring on a pipe, the part `auich_trigger_output` and `auich_trigger_input` share:
/// records the ring and call-back, points the pipe at its descriptor list and triggers it.
fn auich_trigger(
    sc: &AuichSoftc,
    pipe: usize,
    ring: &AuichRing,
    list_off: usize,
    dma: &AuichDma,
    t: &Trigger,
) {
    let Trigger {
        size,
        blksize,
        intr,
        arg,
    } = *t;
    ring.size.set(size);
    ring.intr.set(Some(intr));
    ring.arg.set(arg);

    // The logic behind this is:
    // setup one buffer to play, then LVI dump out the rest
    // to the scatter-gather chain.
    ring.start.set(dma.segs[0].get().ds_addr as u32);
    ring.p.set(ring.start.get());
    ring.end.set(ring.start.get().wrapping_add(size as u32));
    ring.blksize.set(blksize);

    mtx_enter(&AUDIO_LOCK);
    sc.write4(pipe + AUICH_BDBAR, (sc.sc_cddma() + list_off) as u32);
    auich_trigger_pipe(sc, pipe, ring);
    mtx_leave(&AUDIO_LOCK);
}

/// `auich_trigger_output`: plays the ring `[start, end)` in blocks of `blksize` bytes.
///
/// # Safety
///
/// `v` is the `hdl` given to `audio_attach_mi`; `start` and `end` bound the ring `auich_allocm`
/// returned for `AUMODE_PLAY`, and `intr(arg)` is audio(4)'s call-back.
pub unsafe fn auich_trigger_output(
    v: *mut c_void,
    start: *mut u8,
    end: *mut u8,
    blksize: i32,
    intr: AudioIntr,
    arg: *mut c_void,
    _param: &AudioParams,
) -> Result<(), Errno> {
    // SAFETY: the contract.
    let sc = unsafe { sc_of(v) };

    let Some(p) = NonNull::new(sc.sc_pdma.get()) else {
        return Err(Errno::EINVAL);
    };
    // SAFETY: `sc_pdma` is NULL or a live `AuichDma` made by auich_allocm.
    let p = unsafe { p.as_ref() };
    if p.addr.get() != start {
        return Err(Errno::EINVAL);
    }

    let size = (end as usize).wrapping_sub(start as usize);
    auich_trigger(
        sc,
        AUICH_PCMO,
        &sc.pcmo,
        auich_pcmo_off(0),
        p,
        &Trigger {
            size,
            blksize,
            intr,
            arg,
        },
    );
    Ok(())
}

/// `auich_trigger_input`: records into the ring `[start, end)` in blocks of `blksize`
/// bytes.
///
/// # Safety
///
/// `v` is the `hdl` given to `audio_attach_mi`; `start` and `end` bound the ring `auich_allocm`
/// returned for `AUMODE_RECORD`, and `intr(arg)` is audio(4)'s call-back.
pub unsafe fn auich_trigger_input(
    v: *mut c_void,
    start: *mut u8,
    end: *mut u8,
    blksize: i32,
    intr: AudioIntr,
    arg: *mut c_void,
    _param: &AudioParams,
) -> Result<(), Errno> {
    // SAFETY: the contract.
    let sc = unsafe { sc_of(v) };

    let Some(p) = NonNull::new(sc.sc_rdma.get()) else {
        return Err(Errno::EINVAL);
    };
    // SAFETY: `sc_rdma` is NULL or a live `AuichDma` made by auich_allocm.
    let p = unsafe { p.as_ref() };
    if p.addr.get() != start {
        return Err(Errno::EINVAL);
    }

    let size = (end as usize).wrapping_sub(start as usize);
    auich_trigger(
        sc,
        AUICH_PCMI,
        &sc.pcmi,
        auich_pcmi_off(0),
        p,
        &Trigger {
            size,
            blksize,
            intr,
            arg,
        },
    );
    Ok(())
}

/// `auich_allocmem`: `size` bytes of DMA memory in one segment, mapped, with a loaded map.
pub fn auich_allocmem(
    sc: &AuichSoftc,
    size: usize,
    align: usize,
    p: &AuichDma,
) -> Result<(), Errno> {
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

    let addr = match bus_dmamem_map(
        dmat,
        &mut segs,
        size,
        BUS_DMA_NOWAIT | sc.sc_dmamap_flags.get(),
    ) {
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

    // SAFETY: `addr` maps `size` bytes of DMA memory that stay allocated until
    // auich_freemem unloads the map before unmapping and freeing them.
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

/// `auich_freemem`: undoes `auich_allocmem`.
pub fn auich_freemem(sc: &AuichSoftc, p: &AuichDma) {
    let dmat = sc.dmat();

    if let Some(map) = p.map.get() {
        bus_dmamap_unload(dmat, map);
        // SAFETY: the map came from bus_dmamap_create in auich_allocmem and is unloaded.
        unsafe { bus_dmamap_destroy(dmat, NonNull::from(map)) };
    }
    if let Some(addr) = NonNull::new(p.addr.get()) {
        // SAFETY: the mapping came from bus_dmamem_map with this size.
        unsafe { bus_dmamem_unmap(dmat, addr, p.size.get()) };
    }
    // SAFETY: the segments came from bus_dmamem_alloc and are unmapped and unloaded.
    unsafe { bus_dmamem_free(dmat, &[p.segs[0].get()][..p.nsegs.get()]) };
}

/// `auich_alloc_cdata`: allocates the control data (the three descriptor lists), maps it
/// and loads its DMA map.
pub fn auich_alloc_cdata(sc: &AuichSoftc) -> Result<(), Errno> {
    let dmat = sc.dmat();
    let size = size_of::<AuichCdata>();
    let mut seg = [BusDmaSegment::default(); 1];

    // Allocate the control data structure, and create and load the DMA map for it.
    let rseg = match bus_dmamem_alloc(dmat, size, PAGE_SIZE, 0, &mut seg, 0) {
        Ok(rseg) => rseg,
        Err(e) => {
            printf(format_args!(
                "{}: unable to allocate control data, error = {}\n",
                sc.xname(),
                e as i32
            ));
            return Err(e);
        }
    };

    // fail_1
    let free_seg = |seg: &[BusDmaSegment; 1]| {
        // SAFETY: the segment came from bus_dmamem_alloc above and is unmapped.
        unsafe { bus_dmamem_free(dmat, &seg[..rseg]) };
    };

    let cdata = match bus_dmamem_map(dmat, &mut seg, size, sc.sc_dmamap_flags.get()) {
        Ok(cdata) => cdata,
        Err(e) => {
            printf(format_args!(
                "{}: unable to map control data, error = {}\n",
                sc.xname(),
                e as i32
            ));
            free_seg(&seg);
            return Err(e);
        }
    };

    // fail_2
    let unmap = |seg: &[BusDmaSegment; 1]| {
        // SAFETY: the mapping came from bus_dmamem_map above with this size.
        unsafe { bus_dmamem_unmap(dmat, cdata, size) };
        free_seg(seg);
    };

    let map = match bus_dmamap_create(dmat, size, 1, size, 0, 0) {
        Ok(map) => map,
        Err(e) => {
            printf(format_args!(
                "{}: unable to create control data DMA map, error = {}\n",
                sc.xname(),
                e as i32
            ));
            unmap(&seg);
            return Err(e);
        }
    };

    // SAFETY: `cdata` maps `size` bytes of DMA memory that are never given back.
    if let Err(e) = unsafe { bus_dmamap_load(dmat, map, cdata.as_ptr(), size, None, 0) } {
        printf(format_args!(
            "{}: unable to load control data DMA map, error = {}\n",
            sc.xname(),
            e as i32
        ));
        // fail_3
        // SAFETY: the map came from bus_dmamap_create above and nothing else has it.
        unsafe { bus_dmamap_destroy(dmat, NonNull::from(map)) };
        unmap(&seg);
        return Err(e);
    }

    let cd = cdata.as_ptr().cast::<AuichCdata>();
    sc.sc_cddmamap.set(Some(map));
    sc.sc_cdata.set(cd);
    // SAFETY: `cd` is the start of a mapping of an `AuichCdata`; these are the addresses of
    // its three lists (no reference is made).
    unsafe {
        sc.pcmo
            .dmalist
            .set(ptr::addr_of_mut!((*cd).ic_dmalist_pcmo).cast());
        sc.pcmi
            .dmalist
            .set(ptr::addr_of_mut!((*cd).ic_dmalist_pcmi).cast());
        sc.mici
            .dmalist
            .set(ptr::addr_of_mut!((*cd).ic_dmalist_mici).cast());
    }

    Ok(())
}

/// `auich_resume`: un-mutes the SiS output again and resumes the codec.
pub fn auich_resume(sc: &AuichSoftc) {
    // SiS 7012 needs special handling
    if pci_vendor(sc.pci_id.get()) == PCI_VENDOR_SIS
        && pci_product(sc.pci_id.get()) == PCI_PRODUCT_SIS_7012_ACA
    {
        // un-mute output
        sc.write4(
            ICH_SIS_NV_CTL,
            sc.read4(ICH_SIS_NV_CTL) | ICH_SIS_CTL_UNMUTE,
        );
    }

    let _ = ac97_resume(&sc.host_if.get(), sc.codec());
}

// Calibrate card (some boards are overclocked and need scaling)

/// `auich_calibrate`: records one block of 16000 bytes with interrupts off and times it, to
/// find the real AC-link rate; some boards are overclocked. 48000 when the rate is lower
/// than 48500 or cannot be measured.
pub fn auich_calibrate(sc: &AuichSoftc) -> u32 {
    let sts_reg = sc.sc_sts_reg.get();
    let hdl: *mut c_void = ptr::from_ref(sc).cast_mut().cast();
    let mut ac97rate = AUICH_FIXED_RATE;

    // Grab audio from input for fixed interval and compare how much we actually get with what
    // we expect. Interval needs to be sufficiently short that no interrupts are generated.
    // XXX: Is this true? We don't request any interrupts, so why should the chip issue any?

    // Setup a buffer
    let bytes: u32 = 16000;
    // SAFETY: `hdl` is this softc.
    let temp_buffer = unsafe { auich_allocm(hdl, 0, bytes as usize, M_DEVBUF, M_NOWAIT) };
    let Some(temp_buffer) = temp_buffer else {
        return ac97rate;
    };
    // SAFETY: `sc_cdma` is NULL or the live `AuichDma` auich_allocm just made.
    let p = NonNull::new(sc.sc_cdma.get()).map(|p| unsafe { &*p.as_ptr() });
    let Some(p) = p.filter(|p| p.addr.get() == temp_buffer.as_ptr()) else {
        printf(format_args!(
            "auich_calibrate: bad address {:p}\n",
            temp_buffer.as_ptr()
        ));
        return ac97rate;
    };

    // get current CIV (usually 0 after reboot)
    let mut civ = sc.read1(AUICH_PCMI + AUICH_CIV);
    let mut ociv = civ;
    let ds_addr = match p.map.get() {
        Some(map) => map.dm_segs().first().map_or(0, |s| s.get().ds_addr),
        None => panic(format_args!("auich_calibrate: no DMA map")),
    };
    sc.pcmi.set_desc(
        usize::from(civ & AUICH_LVI_MASK),
        ds_addr as u32,
        (bytes as i32 / sc.sc_sample_size.get()) as u32,
    );

    // our data format is stereo, 16 bit so each sample is 4 bytes.
    // assuming we get 48000 samples per second, we get 192000 bytes/sec.
    // we're going to start recording with interrupts disabled and measure
    // the time taken for one block to complete.  we know the block size,
    // we know the time in microseconds, we calculate the sample rate:
    //
    // actual_rate [bps] = bytes / (time [s] * 4)
    // actual_rate [bps] = (bytes * 1000000) / (time [us] * 4)
    // actual_rate [Hz] = (bytes * 250000) / time [us]

    // prepare
    sc.write4(
        AUICH_PCMI + AUICH_BDBAR,
        (sc.sc_cddma() + auich_pcmi_off(0)) as u32,
    );
    // we got only one valid sample, so set LVI to CIV otherwise we provoke a AUICH_FIFOE FIFO
    // error which will confuse the chip later on.
    sc.write1(AUICH_PCMI + AUICH_LVI, civ & AUICH_LVI_MASK);

    // start, but don't request any interrupts
    let t1 = microuptime();
    sc.write1(AUICH_PCMI + AUICH_CTRL, AUICH_RPBM);

    // XXX remove this sometime
    let mut osts = sc.read2(AUICH_PCMI + sts_reg);
    // wait
    let wait_us: u32 = loop {
        let t2 = microuptime();
        let sts = sc.read2(AUICH_PCMI + sts_reg);
        civ = sc.read1(AUICH_PCMI + AUICH_CIV);

        // turn time delta into us
        let wait_us =
            ((t2.tv_sec - t1.tv_sec) * 1000000 + (t2.tv_usec as i64) - (t1.tv_usec as i64)) as u32;

        // this should actually never happen because civ==lvi
        if (civ & AUICH_LVI_MASK) != (ociv & AUICH_LVI_MASK) {
            printf(format_args!(
                "{}: ac97 CIV progressed after {} us sts={} civ={}\n",
                sc.xname(),
                wait_us,
                Bitmask(u64::from(sts), AUICH_ISTS_BITS),
                civ
            ));
            ociv = civ;
        }
        // normal completion
        if sts & (AUICH_DCH | AUICH_CELV | AUICH_LVBCI) != 0 {
            break wait_us;
        }
        // check for strange changes in STS - XXX remove it when everything is fine
        if sts != osts {
            printf(format_args!(
                "{}: ac97 sts changed after {} us sts={} civ={}\n",
                sc.xname(),
                wait_us,
                Bitmask(u64::from(sts), AUICH_ISTS_BITS),
                civ
            ));
            osts = sts;
        }
        // timeout: we expect 83333 us for 48k sampling rate, 600000 us will be enough even
        // for 8k sampling rate
        if wait_us > 600000 {
            printf(format_args!(
                "{}: ac97 link rate timed out {} us sts={} civ={}\n",
                sc.xname(),
                wait_us,
                Bitmask(u64::from(sts), AUICH_ISTS_BITS),
                civ
            ));
            // reset and clean up
            auich_halt_pipe(sc, AUICH_PCMI, &sc.pcmi);
            auich_halt_pipe(sc, AUICH_MICI, &sc.mici);
            // SAFETY: `hdl` is this softc, and `temp_buffer` is what auich_allocm returned.
            unsafe { auich_freem(hdl, temp_buffer, M_DEVBUF) };
            // return default sample rate
            return ac97rate;
        }
    };

    // reset and clean up
    auich_halt_pipe(sc, AUICH_PCMI, &sc.pcmi);
    auich_halt_pipe(sc, AUICH_MICI, &sc.mici);
    // SAFETY: `hdl` is this softc, and `temp_buffer` is what auich_allocm returned.
    unsafe { auich_freem(hdl, temp_buffer, M_DEVBUF) };

    // now finally calculate measured samplerate
    let actual_48k_rate = (bytes * 250000) / wait_us.max(1);

    if actual_48k_rate <= 48500 {
        ac97rate = AUICH_FIXED_RATE;
    } else {
        ac97rate = actual_48k_rate;
    }

    ac97rate
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_data_layout_matches_the_c() {
        // Three lists of 32 descriptors of two 32-bit words.
        assert_eq!(size_of::<AuichDmalist>(), 8);
        assert_eq!(size_of::<AuichCdata>(), 3 * 32 * 8);
        assert_eq!(auich_pcmo_off(0), 0);
        assert_eq!(auich_pcmo_off(31), 31 * 8);
        assert_eq!(auich_pcmi_off(0), 32 * 8);
        assert_eq!(auich_pcmi_off(1), 32 * 8 + 8);
        assert_eq!(auich_mici_off(0), 64 * 8);
        assert_eq!(auich_mici_off(31), 95 * 8);
        // The lists are what the BDBAR registers (8-byte aligned) point at.
        assert_eq!(auich_pcmi_off(0) % 8, 0);
        assert_eq!(AUICH_DMALIST_MAX * AUICH_DMASEG_MAX, 4 << 20);
    }

    #[test]
    fn blocks_are_multiples_of_64_bytes_and_buffers_are_capped() {
        // SAFETY: neither function touches its handle.
        unsafe {
            let nul = ptr::null_mut();
            assert_eq!(auich_round_blocksize(nul, 1), 64);
            assert_eq!(auich_round_blocksize(nul, 64), 64);
            assert_eq!(auich_round_blocksize(nul, 65), 128);
            assert_eq!(auich_round_blocksize(nul, 960), 960);
            assert_eq!(auich_round_blocksize(nul, 961), 1024);

            assert_eq!(auich_round_buffersize(nul, AUMODE_PLAY, 65536), 65536);
            assert_eq!(
                auich_round_buffersize(nul, AUMODE_RECORD, 5 << 20),
                AUICH_DMALIST_MAX * AUICH_DMASEG_MAX
            );
        }
    }

    #[test]
    fn the_device_table_is_the_c_one() {
        assert_eq!(AUICH_DEVICES.len(), 22);
        // QEMU's `-device AC97` is the 82801AA.
        let d = AUICH_DEVICES
            .iter()
            .rev()
            .find(|d| d.product == PCI_PRODUCT_INTEL_82801AA_ACA)
            .unwrap();
        assert_eq!((d.vendor, d.name), (PCI_VENDOR_INTEL, "ICH"));
        // The two nForce2 variants share a name; the SiS one is the special case.
        assert!(AUICH_DEVICES.iter().filter(|d| d.name == "nForce2").count() == 2);
        assert!(
            AUICH_DEVICES
                .iter()
                .any(|d| d.vendor == PCI_VENDOR_SIS && d.product == PCI_PRODUCT_SIS_7012_ACA)
        );
        // Intel's HDA controller (azalia) is not one of them.
        assert!(!AUICH_DEVICES.iter().any(|d| d.product == 0x2668));
    }

    #[test]
    fn bit_descriptions_are_the_c_strings() {
        assert_eq!(AUICH_ISTS_BITS.first(), Some(&0x10));
        assert!(AUICH_ISTS_BITS.ends_with(b"fifoe"));
        assert!(AUICH_GSTS_BITS.ends_with(b"\x12md3"));
    }

    /// The bytes of a C string literal with octal escapes, as the file spells it.
    fn c_string(text: &str) -> std::vec::Vec<u8> {
        let t = text.trim().trim_matches('"').as_bytes().to_vec();
        let mut out = std::vec::Vec::new();
        let mut i = 0;
        while i < t.len() {
            if t[i] == b'\\' {
                let mut n = 0u32;
                let mut j = i + 1;
                while j < t.len() && j < i + 4 && (b'0'..=b'7').contains(&t[j]) {
                    n = n * 8 + u32::from(t[j] - b'0');
                    j += 1;
                }
                out.push(n as u8);
                i = j;
            } else {
                out.push(t[i]);
                i += 1;
            }
        }
        out
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_file() {
        let defs = crate::reftest::defines("sys/dev/pci/auich.c");
        let mut ours = crate::reftest::assert_defines!(defs;
        AUICH_NAMBAR, AUICH_NABMBAR, AUICH_CFG, AUICH_CFG_IOSE, AUICH_MMBAR, AUICH_MBBAR,
        AUICH_S2CR, AUICH_BDBAR, AUICH_CIV, AUICH_LVI, AUICH_LVI_MASK, AUICH_STS, AUICH_FIFOE,
        AUICH_BCIS, AUICH_LVBCI, AUICH_CELV, AUICH_DCH, AUICH_PICB, AUICH_PIV, AUICH_CTRL,
        AUICH_IOCE, AUICH_FEIE, AUICH_LVBIE, AUICH_RR, AUICH_RPBM, AUICH_PCMI, AUICH_PCMO,
        AUICH_MICI, AUICH_GCTRL, AUICH_SSM_78, AUICH_SSM_69, AUICH_SSM_1011, AUICH_POM16,
        AUICH_POM20, AUICH_PCM246_MASK, AUICH_PCM2, AUICH_PCM4, AUICH_PCM6,
        AUICH_SIS_PCM246_MASK, AUICH_SIS_PCM2, AUICH_SIS_PCM4, AUICH_SIS_PCM6, AUICH_S2RIE,
        AUICH_SRIE, AUICH_PRIE, AUICH_ACLSO, AUICH_WRESET, AUICH_CRESET, AUICH_GIE, AUICH_GSTS,
        AUICH_MD3, AUICH_AD3, AUICH_RCS, AUICH_B3S12, AUICH_B2S12, AUICH_B1S12, AUICH_SRI,
        AUICH_PRI, AUICH_SCR, AUICH_PCR, AUICH_MINT, AUICH_POINT, AUICH_PIINT, AUICH_MOINT,
        AUICH_MIINT, AUICH_GSCI, AUICH_CAS, AUICH_SEMATIMO, AUICH_RESETIMO, ICH_SIS_NV_CTL,
        ICH_SIS_CTL_UNMUTE, AUICH_DMALIST_MAX, AUICH_DMAF_IOC, AUICH_DMAF_BUP,
        AUICH_FIXED_RATE);

        // `AUICH_DMASEG_MAX` is `(65536*2)`, parsed with the operator the reader knows.
        assert_eq!(
            crate::reftest::int(&defs, "AUICH_DMASEG_MAX"),
            Some(AUICH_DMASEG_MAX as i64)
        );
        assert_eq!(c_string(&defs["AUICH_ISTS_BITS"]), AUICH_ISTS_BITS);
        assert_eq!(c_string(&defs["AUICH_GSTS_BITS"]), AUICH_GSTS_BITS);
        // The debug masks belong to `AUICH_DEBUG`, which is not configured.
        ours.extend([
            "AUICH_DMASEG_MAX",
            "AUICH_ISTS_BITS",
            "AUICH_GSTS_BITS",
            "AUICH_DEBUG_CODECIO",
            "AUICH_DEBUG_DMA",
            "AUICH_DEBUG_INTR",
        ]);
        crate::reftest::assert_complete(&defs, "AUICH_", &ours);
        crate::reftest::assert_complete(&defs, "ICH_", &ours);
    }
}
/* </TESTS> */
