use core::cell::Cell;
use core::ffi::c_void;
use core::sync::atomic::{AtomicI32, Ordering};
use std::boxed::Box;

use super::*;
use crate::kern::subr_pool::tests::setup_real_memory;
use crate::sys::audioio::AudioNdac;

// --- a fake codec behind a fake host ---------------------------------------------------

/// The registers of a codec the host can read and write; `RESET` and the IDs are read-only.
struct FakeCodec {
    regs: [Cell<u16>; 64],
    /// The `spdif_event` calls.
    spdif: AtomicI32,
}

impl FakeCodec {
    fn new(vendor: u32, caps: u16, ext_id: u16) -> &'static Self {
        let c: &'static Self = Box::leak(Box::new(Self {
            regs: core::array::from_fn(|_| Cell::new(0)),
            spdif: AtomicI32::new(-1),
        }));
        c.regs[0].set(caps);
        c.regs[usize::from(AC97_REG_VENDOR_ID1 >> 1)].set((vendor >> 16) as u16);
        c.regs[usize::from(AC97_REG_VENDOR_ID2 >> 1)].set(vendor as u16);
        c.regs[usize::from(AC97_REG_EXT_AUDIO_ID >> 1)].set(ext_id);
        c
    }

    fn reg(&self, reg: u8) -> u16 {
        self.regs[usize::from(reg >> 1)].get()
    }
}

/// The codec the host's `arg` names.
///
/// # Safety
///
/// `arg` is a `*const FakeCodec` made by `host_if`.
unsafe fn codec<'a>(arg: *mut c_void) -> &'a FakeCodec {
    // SAFETY: the contract.
    unsafe { &*arg.cast::<FakeCodec>() }
}

/// The codec interface the host got from `attach`.
static CODEC_IF: core::sync::atomic::AtomicPtr<Ac97CodecIf> =
    core::sync::atomic::AtomicPtr::new(ptr::null_mut());

unsafe fn fake_attach(_: *mut c_void, cif: &Ac97CodecIf) -> Result<(), Errno> {
    CODEC_IF.store(ptr::from_ref(cif).cast_mut(), Ordering::SeqCst);
    Ok(())
}

unsafe fn fake_read(arg: *mut c_void, reg: u8, val: &mut u16) -> Result<(), Errno> {
    // SAFETY: `host_if` passes the codec.
    *val = unsafe { codec(arg) }.reg(reg);
    Ok(())
}

unsafe fn fake_write(arg: *mut c_void, reg: u8, val: u16) -> Result<(), Errno> {
    let idx = usize::from(reg >> 1);
    // The reset register and the IDs cannot be written.
    if reg != AC97_REG_RESET
        && reg != AC97_REG_VENDOR_ID1
        && reg != AC97_REG_VENDOR_ID2
        && reg != AC97_REG_EXT_AUDIO_ID
    {
        // SAFETY: `host_if` passes the codec.
        unsafe { codec(arg) }.regs[idx].set(val);
    }
    Ok(())
}

unsafe fn fake_reset(_: *mut c_void) {}

unsafe fn fake_spdif_event(arg: *mut c_void, flag: i32) {
    // SAFETY: `host_if` passes the codec.
    unsafe { codec(arg) }.spdif.store(flag, Ordering::SeqCst);
}

fn host_if(c: &'static FakeCodec) -> Ac97HostIf {
    Ac97HostIf {
        arg: ptr::from_ref(c).cast_mut().cast(),
        attach: Some(fake_attach),
        read: Some(fake_read),
        write: Some(fake_write),
        reset: Some(fake_reset),
        flags: None,
        spdif_event: Some(fake_spdif_event),
    }
}

/// Attaches `c` and returns the interface.
fn attach(c: &'static FakeCodec) -> &'static Ac97CodecIf {
    ac97_attach(&host_if(c)).unwrap();
    let p = CODEC_IF.load(Ordering::SeqCst);
    // SAFETY: `fake_attach` stored the interface of a softc that ac97_attach never frees.
    unsafe { &*p }
}

fn stereo(dev: i32, l: u8, r: u8) -> MixerCtrl {
    let mut ctl = MixerCtrl::default();
    ctl.dev = dev;
    ctl.type_ = AUDIO_MIXER_VALUE;
    ctl.un.value_mut().num_channels = 2;
    ctl.un.value_mut().level[AUDIO_MIXER_LEVEL_LEFT] = l;
    ctl.un.value_mut().level[AUDIO_MIXER_LEVEL_RIGHT] = r;
    ctl
}

fn label(di: &MixerDevinfo) -> &[u8] {
    let n = di.label.name.iter().position(|&c| c == 0).unwrap();
    &di.label.name[..n]
}

// --- tables ------------------------------------------------------------------------------

#[test]
fn codec_ids_find_their_vendor_and_codec() {
    // AD1980, an Analog Devices codec with an init function.
    let (v, c) = ac97_find_codec(0x4144_5370).unwrap();
    assert_eq!(v.name, "Analog Devices");
    let c = c.unwrap();
    assert_eq!(c.name, "AD1980");
    assert!(c.init.is_some());

    // The SigmaTel STAC9750 the QEMU-like ICH codecs report.
    let (v, c) = ac97_find_codec(0x8384_7650).unwrap();
    assert_eq!((v.name, c.unwrap().name), ("SigmaTel", "STAC9750/51"));
    assert!(c.unwrap().init.is_none());

    // ALC655 matches on the high nibble; the low one is the revision.
    let (_, c) = ac97_find_codec(0x414c_4763).unwrap();
    let c = c.unwrap();
    assert_eq!(
        (c.name, c.rev, 0x414c_4763 & u32::from(c.rev)),
        ("ALC655", 0xf, 3)
    );

    // Cirrus Logic masks five bits of the id and keeps three of revision.
    let (_, c) = ac97_find_codec(0x4352_5933).unwrap();
    assert_eq!(c.unwrap().name, "CS4299");

    // A known vendor with an unknown codec, and an unknown vendor.
    let (v, c) = ac97_find_codec(0x4144_53ff).unwrap();
    assert_eq!(v.name, "Analog Devices");
    assert!(c.is_none());
    assert!(ac97_find_codec(0x1234_5678).is_none());

    // Every id in the table finds itself.
    for v in &AC97_VENDORS {
        for c in v.codecs {
            let (fv, fc) = ac97_find_codec(v.id | u32::from(c.id)).unwrap();
            assert_eq!(fv.id, v.id);
            assert!(fc.is_some(), "{} {}", v.name, c.name);
        }
    }
}

#[test]
fn vendors_are_sorted_as_in_the_c() {
    let ids: Vec<u32> = AC97_VENDORS.iter().map(|v| v.id).collect();
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    assert_eq!(ids, sorted);
    assert_eq!(AC97_VENDORS.len(), 24);
}

use std::vec::Vec;

#[test]
fn enum_names_are_the_concatenated_audio_names() {
    let name = |e: &AudioMixerEnum, i: usize| {
        let n = &e.member[i].label.name;
        n[..n.iter().position(|&c| c == 0).unwrap()].to_vec()
    };
    assert_eq!(name(&AC97_MIC_SELECT, 0), [AudioNmicrophone, b"0"].concat());
    assert_eq!(name(&AC97_MIC_SELECT, 1), [AudioNmicrophone, b"1"].concat());
    assert_eq!(name(&AC97_SOURCE, 6), [AudioNmixerout, AudioNmono].concat());
    assert_eq!(AC97_SOURCE.num_mem, 8);
    assert_eq!(AC97_ON_OFF.member[1].ord, 1);
    assert_eq!(name(&AC97_ON_OFF, 1), AudioNon);
}

#[test]
fn enhancement_and_the_bit_strings() {
    assert_eq!(ac97_caps_enhancement(0xfc00), 0x1f);
    assert_eq!(ac97_caps_enhancement(0x0400), 1);
    assert_eq!(
        AC97ENHANCEMENT[usize::from(ac97_caps_enhancement(0x0400))],
        "Analog Devices Phat Stereo"
    );
    assert_eq!(AC97_BITS_6CH, 0x01c0);
    assert_eq!(AC97_EXT_AUDIO_BITS.first(), Some(&0x10));
    assert_eq!(AC97_REG_SPDIF_CTRL_BITS.first(), Some(&0x02));
}

// --- the mixer ---------------------------------------------------------------------------

#[test]
fn mixer_over_a_fake_codec() {
    let _guard = setup_real_memory();
    let caps = AC97_CAPS_MICIN | AC97_CAPS_HEADPHONES | AC97_CAPS_LOUDNESS | (1 << 10);
    let c = FakeCodec::new(0x8384_7650, caps, AC97_EXT_AUDIO_VRA | AC97_EXT_AUDIO_SPDIF);
    let cif = attach(c);
    let vt = cif.vtbl;

    // attach wrote the reset-state defaults, then switched the master, dac and record mutes
    // off and selected the microphone.
    assert_eq!(c.reg(AC97_REG_MASTER_VOLUME) & 0x8000, 0);
    assert_eq!(c.reg(AC97_REG_PCMOUT_VOLUME) & 0x8000, 0);
    assert_eq!(c.reg(AC97_REG_RECORD_GAIN) & 0x8000, 0);
    assert_eq!(c.reg(AC97_REG_RECORD_SELECT), 0);
    // S/PDIF's ext audio control: VRA on, the S/PDIF slot choice and 48 kHz.
    assert_eq!(
        c.reg(AC97_REG_EXT_AUDIO_CTRL) & AC97_EXT_AUDIO_VRA,
        AC97_EXT_AUDIO_VRA
    );
    assert_eq!(
        c.reg(AC97_REG_SPDIF_CTRL) & AC97_SPDIF_SPSR_MASK,
        AC97_SPDIF_SPSR_48K
    );
    assert_eq!(c.reg(AC97_REG_PCM_FRONT_DAC_RATE), 48000);

    // devinfo walks every control, then ENXIO; the capabilities pick the controls: headphone,
    // mic gain, loudness, mic channel, S/PDIF; no tone, surround, center, lfe, 3D.
    let mut di = MixerDevinfo::zeroed();
    let mut names = Vec::new();
    for i in 0.. {
        di.index = i;
        if let Err(e) = (vt.query_devinfo)(cif, &mut di) {
            assert_eq!(e, Errno::ENXIO);
            break;
        }
        names.push((di.type_, label(&di).to_vec()));
    }
    // 3 classes; master 2, mono 3, headphone 2; speaker 2, phone 2, mic 4, line 2, cd 2, video 2,
    // aux 2, dac 2; record source 1, gain 2, mic gain 2; loudness 1, spatial 3, extamp 1, spdif 1.
    assert_eq!(
        names.len(),
        3 + (2 + 3 + 2) + (2 + 2 + 4 + 2 + 2 + 2 + 2 + 2) + (1 + 2 + 2) + (1 + 3 + 1 + 1)
    );
    assert_eq!(names[0], (AUDIO_MIXER_CLASS, AudioCinputs.to_vec()));
    assert_eq!(names[3], (AUDIO_MIXER_VALUE, AudioNmaster.to_vec()));
    assert_eq!(names[4], (AUDIO_MIXER_ENUM, AudioNmute.to_vec()));
    assert!(
        names
            .iter()
            .all(|(_, n)| n != b"tone" && n != AudioNsurround)
    );
    assert!(names.iter().any(|(_, n)| n == b"spdif"));

    // The master volume's query: stereo, delta 8 (5 bits), class `outputs`.
    let master = (vt.get_portnum_by_name)(cif, Some(AudioCoutputs), Some(AudioNmaster), None);
    assert_eq!(master, 3);
    di.index = master;
    (vt.query_devinfo)(cif, &mut di).unwrap();
    assert_eq!(di.un.v().num_channels, 2);
    assert_eq!(di.un.v().delta, 8);
    assert_eq!(di.mixer_class, 1);
    assert_eq!(di.prev, AUDIO_MIXER_LAST);
    assert_eq!(di.next, master + 1);
    assert_eq!(
        (vt.get_portnum_by_name)(cif, Some(b"nothing"), None, None),
        -1
    );

    // Left 255 and right 0: the register holds 0 (loud) and 31 (quiet) with polarity 0.
    let mut ctl = stereo(master, 255, 0);
    (vt.mixer_set_port)(cif, &mut ctl).unwrap();
    assert_eq!(c.reg(AC97_REG_MASTER_VOLUME), 0x1f00);
    let mut back = stereo(master, 0, 0);
    (vt.mixer_get_port)(cif, &mut back).unwrap();
    assert_eq!(back.un.value().level[..2], [255, 7]); // 5 bits: 31 << 3 is 248, 255 - 248

    // A mono control with one channel, and more channels than the control has.
    let mic = (vt.get_portnum_by_name)(cif, Some(AudioCinputs), Some(AudioNmicrophone), None);
    let mut ctl = MixerCtrl::default();
    ctl.dev = mic;
    ctl.type_ = AUDIO_MIXER_VALUE;
    ctl.un.value_mut().num_channels = 1;
    ctl.un.value_mut().level[AUDIO_MIXER_LEVEL_MONO] = 100;
    (vt.mixer_set_port)(cif, &mut ctl).unwrap();
    assert_eq!(c.reg(AC97_REG_MIC_VOLUME) & 0x1f, (255 - 100) >> 3);
    ctl.un.value_mut().num_channels = 2;
    assert_eq!((vt.mixer_set_port)(cif, &mut ctl), Err(Errno::EINVAL));

    // The mute enum.
    let mute = (vt.get_portnum_by_name)(
        cif,
        Some(AudioCoutputs),
        Some(AudioNmaster),
        Some(AudioNmute),
    );
    let mut ctl = MixerCtrl::default();
    ctl.dev = mute;
    ctl.type_ = AUDIO_MIXER_ENUM;
    ctl.un.set_ord(1);
    (vt.mixer_set_port)(cif, &mut ctl).unwrap();
    assert_eq!(c.reg(AC97_REG_MASTER_VOLUME) & 0x8080, 0x8000);
    ctl.un.set_ord(2);
    assert_eq!((vt.mixer_set_port)(cif, &mut ctl), Err(Errno::EINVAL));
    ctl.un.set_ord(0);
    (vt.mixer_set_port)(cif, &mut ctl).unwrap();
    assert_eq!(c.reg(AC97_REG_MASTER_VOLUME) & 0x8080, 0);
    // The volume survived the mute.
    assert_eq!(c.reg(AC97_REG_MASTER_VOLUME) & 0x1f1f, 0x1f00);

    // Record source: the choice goes into both channels.
    let src = (vt.get_portnum_by_name)(cif, Some(AudioCrecord), Some(AudioNsource), None);
    let mut ctl = MixerCtrl::default();
    ctl.dev = src;
    ctl.type_ = AUDIO_MIXER_ENUM;
    ctl.un.set_ord(4);
    (vt.mixer_set_port)(cif, &mut ctl).unwrap();
    assert_eq!(c.reg(AC97_REG_RECORD_SELECT) & 0x0707, 0x0404);

    // Bad controls.
    let mut ctl = MixerCtrl::default();
    ctl.dev = 99;
    assert_eq!((vt.mixer_set_port)(cif, &mut ctl), Err(Errno::EINVAL));
    assert_eq!((vt.mixer_get_port)(cif, &mut ctl), Err(Errno::EINVAL));
    ctl.dev = 0; // a class
    ctl.type_ = AUDIO_MIXER_CLASS;
    assert_eq!((vt.mixer_set_port)(cif, &mut ctl), Err(Errno::EINVAL));
    ctl.dev = master;
    ctl.type_ = AUDIO_MIXER_ENUM; // not the master's type
    assert_eq!((vt.mixer_get_port)(cif, &mut ctl), Err(Errno::EINVAL));

    // S/PDIF: unlocked until the host's `unlock` brings the counter below zero.
    let spdif = (vt.get_portnum_by_name)(cif, Some(AudioCoutputs), Some(b"spdif"), None);
    let mut ctl = MixerCtrl::default();
    ctl.dev = spdif;
    ctl.type_ = AUDIO_MIXER_ENUM;
    ctl.un.set_ord(1);
    assert_eq!((vt.mixer_set_port)(cif, &mut ctl), Err(Errno::EBUSY));
    (vt.unlock)(cif);
    (vt.mixer_set_port)(cif, &mut ctl).unwrap();
    assert_eq!(c.spdif.load(Ordering::SeqCst), 1);
    assert_eq!(
        c.reg(AC97_REG_EXT_AUDIO_CTRL) & AC97_EXT_AUDIO_SPDIF,
        AC97_EXT_AUDIO_SPDIF
    );
    (vt.lock)(cif);
    assert_eq!((vt.mixer_set_port)(cif, &mut ctl), Err(Errno::EBUSY));

    // Rates: a variable-rate codec takes 44100 as is; an out-of-range one is refused.
    assert!(!ac97_is_fixed_rate(cif));
    let mut rate = 44100;
    (vt.set_rate)(cif, i32::from(AC97_REG_PCM_FRONT_DAC_RATE), &mut rate).unwrap();
    assert_eq!(rate, 44100);
    assert_eq!(c.reg(AC97_REG_PCM_FRONT_DAC_RATE), 44100);
    let mut rate = 96000;
    assert_eq!(
        (vt.set_rate)(cif, i32::from(AC97_REG_PCM_FRONT_DAC_RATE), &mut rate),
        Err(Errno::EINVAL)
    );
    // The surround DAC is absent from this codec: nothing happens and *rate is untouched.
    let mut rate = 22050;
    (vt.set_rate)(cif, i32::from(AC97_REG_PCM_SURR_DAC_RATE), &mut rate).unwrap();
    assert_eq!(rate, 22050);
    // A clock that is 1.5 times too fast: the register gets 2/3 of the rate, and reads back
    // scaled up again.
    (vt.set_clock)(cif, 72000);
    let mut rate = 48000;
    (vt.set_rate)(cif, i32::from(AC97_REG_PCM_LR_ADC_RATE), &mut rate).unwrap();
    assert_eq!(c.reg(AC97_REG_PCM_LR_ADC_RATE), 32000);
    assert_eq!(rate, 48000);
    // Unknown registers are refused.
    assert_eq!((vt.set_rate)(cif, 0x40, &mut rate), Err(Errno::EINVAL));
    assert_eq!(
        (vt.get_caps)(cif),
        AC97_EXT_AUDIO_VRA | AC97_EXT_AUDIO_SPDIF
    );
    let _ = AudioNdac;
}

#[test]
fn a_fixed_rate_codec_with_an_init_function() {
    let _guard = setup_real_memory();
    // AD1980 swaps the master and surround registers in its mixer; without the surround DAC
    // the ID has no surround control, so the master control keeps its own register.
    let c = FakeCodec::new(0x4144_5370, 0, 0);
    let cif = attach(c);
    let vt = cif.vtbl;
    assert!(ac97_is_fixed_rate(cif));
    let mut rate = 44100;
    (vt.set_rate)(cif, i32::from(AC97_REG_PCM_FRONT_DAC_RATE), &mut rate).unwrap();
    assert_eq!(rate, 48000);
    let mut rate = 8000;
    (vt.set_rate)(cif, i32::from(AC97_REG_PCM_MIC_ADC_RATE), &mut rate).unwrap();
    assert_eq!(rate, 48000);
    // ac97_ad198x_init set HPSEL and LOSEL.
    assert_eq!(
        c.reg(AC97_AD_REG_MISC) & (AC97_AD_MISC_HPSEL | AC97_AD_MISC_LOSEL),
        AC97_AD_MISC_HPSEL | AC97_AD_MISC_LOSEL
    );
    let master = (vt.get_portnum_by_name)(cif, Some(AudioCoutputs), Some(AudioNmaster), None);
    let mut ctl = stereo(master, 255, 255);
    (vt.mixer_set_port)(cif, &mut ctl).unwrap();
    // The master volume of an AD198x is the surround register (the init function swapped
    // them): the volume and mute went to 0x38. Its mute is the C's hard-coded surround
    // logic (`reg == AC97_REG_SURR_MASTER` clears both mute bits, 0x8080), so the default
    // 0x8080 became 0; the real master register kept its default.
    assert_eq!(c.reg(AC97_REG_SURR_MASTER), 0x0000);
    assert_eq!(c.reg(AC97_REG_MASTER_VOLUME), 0x8000);
}

// --- C header values -----------------------------------------------------------------------

/// The bytes of a C string literal with octal escapes, as the header spells it.
fn c_string(text: &str) -> Vec<u8> {
    let t = text.trim().trim_matches('"').as_bytes().to_vec();
    let mut out = Vec::new();
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
fn values_match_the_c_header() {
    let defs = crate::reftest::defines("sys/dev/ic/ac97.h");
    let mut ours = crate::reftest::assert_defines!(defs;
        AC97_REG_RESET, AC97_CAPS_MICIN, AC97_CAPS_TONECTRL, AC97_CAPS_SIMSTEREO,
        AC97_CAPS_HEADPHONES, AC97_CAPS_LOUDNESS, AC97_CAPS_DAC18, AC97_CAPS_DAC20,
        AC97_CAPS_ADC18, AC97_CAPS_ADC20, AC97_CAPS_ENHANCEMENT_MASK, AC97_CAPS_ENHANCEMENT_SHIFT,
        AC97_REG_MASTER_VOLUME, AC97_REG_HEADPHONE_VOLUME, AC97_REG_MASTER_VOLUME_MONO,
        AC97_REG_MASTER_TONE, AC97_REG_PCBEEP_VOLUME, AC97_REG_PHONE_VOLUME, AC97_REG_MIC_VOLUME,
        AC97_REG_LINEIN_VOLUME, AC97_REG_CD_VOLUME, AC97_REG_VIDEO_VOLUME, AC97_REG_AUX_VOLUME,
        AC97_REG_PCMOUT_VOLUME, AC97_REG_RECORD_SELECT, AC97_REG_RECORD_GAIN,
        AC97_REG_RECORD_GAIN_MIC, AC97_REG_GP, AC97_REG_3D_CONTROL, AC97_REG_MODEM_SAMPLE_RATE,
        AC97_REG_POWER, AC97_POWER_ADC, AC97_POWER_DAC, AC97_POWER_ANL, AC97_POWER_REF,
        AC97_POWER_IN, AC97_POWER_OUT, AC97_POWER_MIXER, AC97_POWER_MIXER_VREF,
        AC97_POWER_ACLINK, AC97_POWER_CLK, AC97_POWER_AUX, AC97_POWER_EAMP,
        AC97_REG_EXT_AUDIO_ID, AC97_REG_EXT_AUDIO_CTRL, AC97_EXT_AUDIO_VRA, AC97_EXT_AUDIO_DRA,
        AC97_EXT_AUDIO_SPDIF, AC97_EXT_AUDIO_VRM, AC97_EXT_AUDIO_DSA_MASK, AC97_EXT_AUDIO_DSA00,
        AC97_EXT_AUDIO_DSA01, AC97_EXT_AUDIO_DSA10, AC97_EXT_AUDIO_DSA11,
        AC97_EXT_AUDIO_SPSA_MASK, AC97_EXT_AUDIO_SPSA34, AC97_EXT_AUDIO_SPSA78,
        AC97_EXT_AUDIO_SPSA69, AC97_EXT_AUDIO_SPSAAB, AC97_EXT_AUDIO_CDAC, AC97_EXT_AUDIO_SDAC,
        AC97_EXT_AUDIO_LDAC, AC97_EXT_AUDIO_AMAP, AC97_EXT_AUDIO_SPCV, AC97_EXT_AUDIO_REV_11,
        AC97_EXT_AUDIO_REV_22, AC97_EXT_AUDIO_REV_23, AC97_EXT_AUDIO_REV_MASK,
        AC97_EXT_AUDIO_ID, AC97_SINGLE_RATE, AC97_REG_PCM_FRONT_DAC_RATE,
        AC97_REG_PCM_SURR_DAC_RATE, AC97_REG_PCM_LFE_DAC_RATE, AC97_REG_PCM_LR_ADC_RATE,
        AC97_REG_PCM_MIC_ADC_RATE, AC97_REG_CENTER_LFE_MASTER, AC97_REG_SURR_MASTER,
        AC97_REG_SPDIF_CTRL, AC97_SPDIF_V, AC97_SPDIF_DRS, AC97_SPDIF_SPSR_MASK,
        AC97_SPDIF_SPSR_44K, AC97_SPDIF_SPSR_48K, AC97_SPDIF_SPSR_32K, AC97_SPDIF_L,
        AC97_SPDIF_CC_MASK, AC97_SPDIF_PRE, AC97_SPDIF_COPY, AC97_SPDIF_NOAUDIO, AC97_SPDIF_PRO,
        AC97_REG_VENDOR_ID1, AC97_REG_VENDOR_ID2, AC97_VENDOR_ID_MASK,
        AC97_AD_REG_MISC, AC97_AD_MISC_MBG0, AC97_AD_MISC_MBG1, AC97_AD_MISC_VREFD,
        AC97_AD_MISC_VREFH, AC97_AD_MISC_SRU, AC97_AD_MISC_LOSEL, AC97_AD_MISC_2CMIC,
        AC97_AD_MISC_SPRD, AC97_AD_MISC_DMIX0, AC97_AD_MISC_DMIX1, AC97_AD_MISC_HPSEL,
        AC97_AD_MISC_CLDIS, AC97_AD_MISC_LODIS, AC97_AD_MISC_MSPLT, AC97_AD_MISC_AC97NC,
        AC97_AD_MISC_DACZ,
        AC97_ALC650_REG_MULTI_CHANNEL_CONTROL, AC97_ALC650_MCC_SLOT_MODIFY_MASK,
        AC97_ALC650_MCC_FRONTDAC_FROM_SPDIFIN, AC97_ALC650_MCC_SPDIFOUT_FROM_ADC,
        AC97_ALC650_MCC_PCM_FROM_SPDIFIN, AC97_ALC650_MCC_MIC_OR_CENTERLFE,
        AC97_ALC650_MCC_LINEIN_OR_SURROUND, AC97_ALC650_MCC_INDEPENDENT_MASTER_L,
        AC97_ALC650_MCC_INDEPENDENT_MASTER_R, AC97_ALC650_MCC_ANALOG_TO_CENTERLFE,
        AC97_ALC650_MCC_ANALOG_TO_SURROUND, AC97_ALC650_MCC_EXCHANGE_CENTERLFE,
        AC97_ALC650_MCC_CENTERLFE_DOWNMIX, AC97_ALC650_MCC_SURROUND_DOWNMIX,
        AC97_ALC650_MCC_LINEOUT_TO_SURROUND, AC97_ALC650_REG_MISC, AC97_ALC650_MISC_PIN47,
        AC97_ALC650_MISC_VREFDIS,
        AC97_CX_REG_MISC, AC97_CX_PCM, AC97_CX_AC3, AC97_CX_MASK, AC97_CX_COPYRIGHT,
        AC97_CX_SPDIFEN,
        AC97_VT_REG_TEST, AC97_VT_LVL, AC97_VT_LCTF, AC97_VT_STF, AC97_VT_BPDC, AC97_VT_DC);

    // The two description strings, and the one expression the header splits over two lines.
    assert_eq!(c_string(&defs["AC97_EXT_AUDIO_BITS"]), AC97_EXT_AUDIO_BITS);
    assert_eq!(
        c_string(&defs["AC97_REG_SPDIF_CTRL_BITS"]),
        AC97_REG_SPDIF_CTRL_BITS
    );
    assert_eq!(
        AC97_BITS_6CH,
        AC97_EXT_AUDIO_SDAC | AC97_EXT_AUDIO_CDAC | AC97_EXT_AUDIO_LDAC
    );
    ours.extend([
        "AC97_EXT_AUDIO_BITS",
        "AC97_REG_SPDIF_CTRL_BITS",
        "AC97_BITS_6CH",
    ]);
    crate::reftest::assert_complete(&defs, "AC97_", &ours);
}
