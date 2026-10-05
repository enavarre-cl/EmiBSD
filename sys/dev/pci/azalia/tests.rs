use std::vec;
use std::vec::Vec;

use super::*;
use crate::reftest::{assert_complete, assert_defines};

// --- a simulated codec link ---------------------------------------------------------------

/// What answers the verbs of [`azalia_comresp`] on the host: it gets the node, the verb and
/// its parameter.
pub(crate) type FakeVerbs = std::boxed::Box<dyn FnMut(NidT, u32, u32) -> Result<u32, Errno>>;

std::thread_local! {
    static FAKE_CODEC: core::cell::RefCell<Option<FakeVerbs>> =
        const { core::cell::RefCell::new(None) };
}

/// Install (or, with `None`, remove) the simulated codec of the current thread.
pub(crate) fn set_fake_codec(verbs: Option<FakeVerbs>) {
    FAKE_CODEC.with(|f| *f.borrow_mut() = verbs);
}

/// [`azalia_comresp`]'s hook: the simulated codec's answer, if one is installed.
pub(crate) fn fake_comresp(nid: NidT, control: u32, param: u32) -> Option<Result<u32, Errno>> {
    FAKE_CODEC.with(|f| {
        f.borrow_mut()
            .as_mut()
            .map(|verbs| verbs(nid, control, param))
    })
}

// --- a synthetic codec --------------------------------------------------------------------

/// A widget of type `type_` with node ID `nid`, enabled, connected to `connections`, the
/// first selected.
fn widget(nid: NidT, type_: u32, widgetcap: u32, connections: &[NidT]) -> Widget {
    let mut w = Widget::new();
    w.nid = nid;
    w.type_ = type_;
    w.widgetcap = widgetcap | (type_ << 20);
    w.enable = true;
    w.mixer_class = -1;
    w.connections = connections.to_vec();
    w.selected = if connections.is_empty() { -1 } else { 0 };
    w
}

/// A pin widget whose configuration default says `device`, `port`, `association` and
/// `sequence`, with capabilities `cap`.
fn pin(nid: NidT, cap: u32, device: u32, port: u32, conns: &[NidT]) -> Widget {
    let mut w = widget(nid, COP_AWTYPE_PIN_COMPLEX, COP_AWCAP_CONNLIST, conns);
    let p = w.d.pin_mut();
    p.cap = cap;
    p.device = device;
    p.config = (port << CORB_CD_PORT_OFFSET) | (device << CORB_CD_DEVICE_OFFSET);
    w
}

/// The audio function of QEMU's `hda-output` shape, plus a mixer: function node 1, a
/// stereo DAC (2), a mixer (3) fed by the DAC, a fixed speaker pin (4) fed by the mixer and a
/// line-out jack (5) fed by the DAC. No controller behind it (`az` is NULL), so only the
/// functions that send no verb may run on it.
pub(crate) fn test_codec() -> Codec {
    let mut codec = Codec::new(ptr::null());
    codec.audiofunc = 1;
    codec.wstart = 2;
    codec.wend = 6;
    codec.w = vec![Widget::new(), Widget::new()];
    codec.w[1].nid = 1;
    codec.w[1].enable = true;
    let mut dac = widget(2, COP_AWTYPE_AUDIO_OUTPUT, COP_AWCAP_STEREO, &[]);
    *dac.d.audio_mut() = WidgetAudio {
        encodings: COP_STREAM_FORMAT_PCM,
        bits_rates: COP_PCM_B16 | COP_PCM_R441 | COP_PCM_R480,
    };
    codec.w.push(dac);
    codec
        .w
        .push(widget(3, COP_AWTYPE_AUDIO_MIXER, COP_AWCAP_CONNLIST, &[2]));
    codec.w.push(pin(
        4,
        COP_PINCAP_OUTPUT,
        CORB_CD_SPEAKER,
        CORB_CD_FIXED,
        &[3],
    ));
    codec.w.push(pin(
        5,
        COP_PINCAP_OUTPUT,
        CORB_CD_LINEOUT,
        CORB_CD_JACK,
        &[2],
    ));
    codec.speaker = -1;
    codec.speaker2 = -1;
    codec.spkr_dac = -1;
    codec.mic = -1;
    codec.fhp = -1;
    codec
}

/// The name of widget `nid`, up to its NUL.
fn name(codec: &Codec, nid: NidT) -> &str {
    let n = &codec.wi(nid).name;
    let len = n.iter().position(|&b| b == 0).unwrap_or(n.len());
    core::str::from_utf8(&n[..len]).unwrap()
}

// --- reference-backed ---------------------------------------------------------------------

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn header_defines_match_the_c() {
    let defs = crate::reftest::defines("sys/dev/pci/azalia.h");
    let names = assert_defines!(defs;
        HDA_GCAP, HDA_GCAP_NSDO_MASK, HDA_GCAP_NSDO_1, HDA_GCAP_NSDO_2, HDA_GCAP_NSDO_4,
        HDA_GCAP_NSDO_RESERVED, HDA_GCAP_64OK, HDA_VMIN, HDA_VMAJ, HDA_OUTPAY, HDA_INPAY, HDA_GCTL,
        HDA_GCTL_UNSOL, HDA_GCTL_FCNTRL, HDA_GCTL_CRST, HDA_WAKEEN, HDA_WAKEEN_SDIWEN,
        HDA_STATESTS, HDA_STATESTS_SDIWAKE, HDA_GSTS, HDA_GSTS_FSTS, HDA_OUTSTRMPAY, HDA_INSTRMPAY,
        HDA_INTCTL, HDA_INTCTL_GIE, HDA_INTCTL_CIE, HDA_INTCTL_SIE, HDA_INTSTS, HDA_INTSTS_GIS,
        HDA_INTSTS_CIS, HDA_INTSTS_SIS, HDA_WALCLK, HDA_SSYNC, HDA_SSYNC_SSYNC, HDA_CORBLBASE,
        HDA_CORBUBASE, HDA_CORBWP, HDA_CORBWP_CORBWP, HDA_CORBRP, HDA_CORBRP_CORBRPRST,
        HDA_CORBRP_CORBRP, HDA_CORBCTL, HDA_CORBCTL_CORBRUN, HDA_CORBCTL_CMEIE, HDA_CORBSTS,
        HDA_CORBSTS_CMEI, HDA_CORBSIZE, HDA_CORBSIZE_CORBSZCAP_MASK, HDA_CORBSIZE_CORBSZCAP_2,
        HDA_CORBSIZE_CORBSZCAP_16, HDA_CORBSIZE_CORBSZCAP_256, HDA_CORBSIZE_CORBSIZE_MASK,
        HDA_CORBSIZE_CORBSIZE_2, HDA_CORBSIZE_CORBSIZE_16, HDA_CORBSIZE_CORBSIZE_256,
        HDA_RIRBLBASE, HDA_RIRBUBASE, HDA_RIRBWP, HDA_RIRBWP_RIRBWPRST, HDA_RIRBWP_RIRBWP,
        HDA_RINTCNT, HDA_RINTCNT_RINTCNT, HDA_RIRBCTL, HDA_RIRBCTL_RIRBOIC, HDA_RIRBCTL_RIRBDMAEN,
        HDA_RIRBCTL_RINTCTL, HDA_RIRBSTS, HDA_RIRBSTS_RIRBOIS, HDA_RIRBSTS_RINTFL, HDA_RIRBSIZE,
        HDA_RIRBSIZE_RIRBSZCAP_MASK, HDA_RIRBSIZE_RIRBSZCAP_2, HDA_RIRBSIZE_RIRBSZCAP_16,
        HDA_RIRBSIZE_RIRBSZCAP_256, HDA_RIRBSIZE_RIRBSIZE_MASK, HDA_RIRBSIZE_RIRBSIZE_2,
        HDA_RIRBSIZE_RIRBSIZE_16, HDA_RIRBSIZE_RIRBSIZE_256, HDA_IC, HDA_IR, HDA_IRS,
        HDA_IRS_IRRADD, HDA_IRS_IRRUNSOL, HDA_IRS_IRV, HDA_IRS_ICB, HDA_DPLBASE,
        HDA_DPLBASE_DPLBASE, HDA_DPLBASE_ENABLE, HDA_DPUBASE, HDA_SD_BASE, HDA_SD_CTL,
        HDA_SD_CTL_DEIE, HDA_SD_CTL_FEIE, HDA_SD_CTL_IOCE, HDA_SD_CTL_RUN, HDA_SD_CTL_SRST,
        HDA_SD_CTL2, HDA_SD_CTL2_STRM, HDA_SD_CTL2_STRM_SHIFT, HDA_SD_CTL2_DIR, HDA_SD_CTL2_TP,
        HDA_SD_CTL2_STRIPE, HDA_SD_STS, HDA_SD_STS_FIFORDY, HDA_SD_STS_DESE, HDA_SD_STS_FIFOE,
        HDA_SD_STS_BCIS, HDA_SD_LPIB, HDA_SD_CBL, HDA_SD_LVI, HDA_SD_LVI_LVI, HDA_SD_FIFOW,
        HDA_SD_FIFOS, HDA_SD_FMT, HDA_SD_FMT_BASE, HDA_SD_FMT_BASE_48, HDA_SD_FMT_BASE_44,
        HDA_SD_FMT_MULT, HDA_SD_FMT_MULT_X1, HDA_SD_FMT_MULT_X2, HDA_SD_FMT_MULT_X3,
        HDA_SD_FMT_MULT_X4, HDA_SD_FMT_DIV, HDA_SD_FMT_DIV_BY1, HDA_SD_FMT_DIV_BY2,
        HDA_SD_FMT_DIV_BY3, HDA_SD_FMT_DIV_BY4, HDA_SD_FMT_DIV_BY5, HDA_SD_FMT_DIV_BY6,
        HDA_SD_FMT_DIV_BY7, HDA_SD_FMT_DIV_BY8, HDA_SD_FMT_BITS, HDA_SD_FMT_BITS_8_16,
        HDA_SD_FMT_BITS_16_16, HDA_SD_FMT_BITS_20_32, HDA_SD_FMT_BITS_24_32, HDA_SD_FMT_BITS_32_32,
        HDA_SD_FMT_CHAN, HDA_SD_BDPL, HDA_SD_BDPU, HDA_SD_SIZE, CORB_GET_PARAMETER, COP_VENDOR_ID,
        COP_REVISION_ID, COP_SUBORDINATE_NODE_COUNT, COP_FUNCTION_GROUP_TYPE, COP_FTYPE_RESERVED,
        COP_FTYPE_AUDIO, COP_FTYPE_MODEM, COP_AUDIO_FUNCTION_GROUP_CAPABILITY,
        COP_AUDIO_WIDGET_CAP, COP_AWTYPE_AUDIO_OUTPUT, COP_AWTYPE_AUDIO_INPUT,
        COP_AWTYPE_AUDIO_MIXER, COP_AWTYPE_AUDIO_SELECTOR, COP_AWTYPE_PIN_COMPLEX,
        COP_AWTYPE_POWER, COP_AWTYPE_VOLUME_KNOB, COP_AWTYPE_BEEP_GENERATOR,
        COP_AWTYPE_VENDOR_DEFINED, COP_AWCAP_STEREO, COP_AWCAP_INAMP, COP_AWCAP_OUTAMP,
        COP_AWCAP_AMPOV, COP_AWCAP_FORMATOV, COP_AWCAP_STRIPE, COP_AWCAP_PROC, COP_AWCAP_UNSOL,
        COP_AWCAP_CONNLIST, COP_AWCAP_DIGITAL, COP_AWCAP_POWER, COP_AWCAP_LRSWAP, COP_PCM,
        COP_PCM_B32, COP_PCM_B24, COP_PCM_B20, COP_PCM_B16, COP_PCM_B8, COP_PCM_R3840,
        COP_PCM_R1920, COP_PCM_R1764, COP_PCM_R960, COP_PCM_R882, COP_PCM_R480, COP_PCM_R441,
        COP_PCM_R320, COP_PCM_R220, COP_PCM_R160, COP_PCM_R110, COP_PCM_R80, COP_STREAM_FORMATS,
        COP_STREAM_FORMAT_PCM, COP_STREAM_FORMAT_FLOAT32, COP_STREAM_FORMAT_AC3, COP_PINCAP,
        COP_PINCAP_IMPEDANCE, COP_PINCAP_TRIGGER, COP_PINCAP_PRESENCE, COP_PINCAP_HEADPHONE,
        COP_PINCAP_OUTPUT, COP_PINCAP_INPUT, COP_PINCAP_BALANCE, COP_PINCAP_HDMI, COP_PINCAP_EAPD,
        COP_INPUT_AMPCAP, COP_AMPCAP_MUTE, COP_CONNECTION_LIST_LENGTH, COP_CLL_LONG,
        COP_SUPPORTED_POWER_STATES, COP_PROCESSING_CAPABILITIES, COP_GPIO_COUNT, COP_GPIO_UNSOL,
        COP_GPIO_WAKE, COP_OUTPUT_AMPCAP, COP_VOLUME_KNOB_CAPABILITIES, COP_VKCAP_DELTA,
        CORB_GET_CONNECTION_SELECT_CONTROL, CORB_SET_CONNECTION_SELECT_CONTROL,
        CORB_GET_CONNECTION_LIST_ENTRY, CORB_GET_PROCESSING_STATE, CORB_SET_PROCESSING_STATE,
        CORB_GET_COEFFICIENT_INDEX, CORB_SET_COEFFICIENT_INDEX, CORB_GET_PROCESSING_COEFFICIENT,
        CORB_SET_PROCESSING_COEFFICIENT, CORB_GET_AMPLIFIER_GAIN_MUTE, CORB_GAGM_INPUT,
        CORB_GAGM_OUTPUT, CORB_GAGM_RIGHT, CORB_GAGM_LEFT, CORB_GAGM_MUTE,
        CORB_SET_AMPLIFIER_GAIN_MUTE, CORB_AGM_GAIN_MASK, CORB_AGM_MUTE, CORB_AGM_INDEX_SHIFT,
        CORB_AGM_RIGHT, CORB_AGM_LEFT, CORB_AGM_INPUT, CORB_AGM_OUTPUT, CORB_GET_CONVERTER_FORMAT,
        CORB_SET_CONVERTER_FORMAT, CORB_GET_DIGITAL_CONTROL, CORB_SET_DIGITAL_CONTROL_L,
        CORB_SET_DIGITAL_CONTROL_H, CORB_DCC_DIGEN, CORB_DCC_V, CORB_DCC_VCFG, CORB_DCC_PRE,
        CORB_DCC_COPY, CORB_DCC_NAUDIO, CORB_DCC_PRO, CORB_DCC_L, CORB_GET_POWER_STATE,
        CORB_SET_POWER_STATE, CORB_PS_D0, CORB_PS_D1, CORB_PS_D2, CORB_PS_D3,
        CORB_GET_CONVERTER_STREAM_CHANNEL, CORB_SET_CONVERTER_STREAM_CHANNEL,
        CORB_GET_INPUT_CONVERTER_SDI_SELECT, CORB_SET_INPUT_CONVERTER_SDI_SELECT,
        CORB_GET_PIN_WIDGET_CONTROL, CORB_SET_PIN_WIDGET_CONTROL, CORB_PWC_HEADPHONE,
        CORB_PWC_OUTPUT, CORB_PWC_INPUT, CORB_PWC_VREF_MASK, CORB_PWC_VREF_HIZ, CORB_PWC_VREF_50,
        CORB_PWC_VREF_GND, CORB_PWC_VREF_80, CORB_PWC_VREF_100, CORB_GET_UNSOLICITED_RESPONSE,
        CORB_SET_UNSOLICITED_RESPONSE, CORB_UNSOL_ENABLE, CORB_GET_PIN_SENSE, CORB_PS_PRESENCE,
        CORB_EXECUTE_PIN_SENSE, CORB_PS_RIGHT, CORB_GET_EAPD_BTL_ENABLE, CORB_SET_EAPD_BTL_ENABLE,
        CORB_EAPD_BTL, CORB_EAPD_EAPD, CORB_EAPD_LRSWAP, CORB_GET_GPI_DATA, CORB_SET_GPI_DATA,
        CORB_GET_GPI_WAKE_ENABLE_MASK, CORB_SET_GPI_WAKE_ENABLE_MASK,
        CORB_GET_GPI_UNSOLICITED_ENABLE_MASK, CORB_SET_GPI_UNSOLICITED_ENABLE_MASK,
        CORB_GET_GPI_STICKY_MASK, CORB_SET_GPI_STICKY_MASK, CORB_GET_GPO_DATA, CORB_SET_GPO_DATA,
        CORB_GET_GPIO_DATA, CORB_SET_GPIO_DATA, CORB_GET_GPIO_ENABLE_MASK,
        CORB_SET_GPIO_ENABLE_MASK, CORB_GET_GPIO_DIRECTION, CORB_SET_GPIO_DIRECTION,
        CORB_GET_GPIO_WAKE_ENABLE_MASK, CORB_SET_GPIO_WAKE_ENABLE_MASK,
        CORB_GET_GPIO_UNSOLICITED_ENABLE_MASK, CORB_SET_GPIO_UNSOLICITED_ENABLE_MASK,
        CORB_GET_GPIO_STICKY_MASK, CORB_SET_GPIO_STICKY_MASK, CORB_GET_GPIO_POLARITY,
        CORB_SET_GPIO_POLARITY, CORB_GET_BEEP_GENERATION, CORB_SET_BEEP_GENERATION,
        CORB_GET_VOLUME_KNOB, CORB_SET_VOLUME_KNOB, CORB_VKNOB_DIRECT, CORB_GET_SUBSYSTEM_ID,
        CORB_SET_SUBSYSTEM_ID_1, CORB_SET_SUBSYSTEM_ID_2, CORB_SET_SUBSYSTEM_ID_3,
        CORB_SET_SUBSYSTEM_ID_4, CORB_GET_CONFIGURATION_DEFAULT, CORB_SET_CONFIGURATION_DEFAULT_1,
        CORB_SET_CONFIGURATION_DEFAULT_2, CORB_SET_CONFIGURATION_DEFAULT_3,
        CORB_SET_CONFIGURATION_DEFAULT_4, CORB_CD_SEQUENCE_MAX, CORB_CD_ASSOCIATION_MAX,
        CORB_CD_MISC_MASK, CORB_CD_PRESENCEOV, CORB_CD_COLOR_UNKNOWN, CORB_CD_BLACK, CORB_CD_GRAY,
        CORB_CD_BLUE, CORB_CD_GREEN, CORB_CD_RED, CORB_CD_ORANGE, CORB_CD_YELLOW, CORB_CD_PURPLE,
        CORB_CD_PINK, CORB_CD_WHITE, CORB_CD_COLOR_OTHER, CORB_CD_CONNECTION_OFFSET,
        CORB_CD_CONNECTION_BITS, CORB_CD_CONNECTION_MASK, CORB_CD_CONN_UNKNOWN, CORB_CD_18,
        CORB_CD_14, CORB_CD_ATAPI, CORB_CD_RCA, CORB_CD_OPTICAL, CORB_CD_OTHER_DIG,
        CORB_CD_OTHER_ANALOG, CORB_CD_DIN, CORB_CD_XLF, CORB_CD_RJ11, CORB_CD_CONN_COMB,
        CORB_CD_CONN_OTHER, CORB_CD_DEVICE_OFFSET, CORB_CD_DEVICE_BITS, CORB_CD_DEVICE_MASK,
        CORB_CD_LINEOUT, CORB_CD_SPEAKER, CORB_CD_HEADPHONE, CORB_CD_CD, CORB_CD_SPDIFOUT,
        CORB_CD_DIGITALOUT, CORB_CD_MODEMLINE, CORB_CD_MODEMHANDSET, CORB_CD_LINEIN, CORB_CD_AUX,
        CORB_CD_MICIN, CORB_CD_TELEPHONY, CORB_CD_SPDIFIN, CORB_CD_DIGITALIN, CORB_CD_BEEP,
        CORB_CD_DEVICE_OTHER, CORB_CD_LOCATION_MASK, CORB_CD_LOC_GEO_NA, CORB_CD_REAR,
        CORB_CD_FRONT, CORB_CD_LEFT, CORB_CD_RIGHT, CORB_CD_TOP, CORB_CD_BOTTOM, CORB_CD_LOC_SPEC0,
        CORB_CD_LOC_SPEC1, CORB_CD_LOC_SPEC2, CORB_CD_EXTERNAL, CORB_CD_INTERNAL, CORB_CD_SEPARATE,
        CORB_CD_LOC_OTHER, CORB_CD_PORT_OFFSET, CORB_CD_PORT_BITS, CORB_CD_PORT_MASK, CORB_CD_JACK,
        CORB_CD_NONE, CORB_CD_FIXED, CORB_CD_BOTH, CORB_GET_STRIPE_CONTROL,
        CORB_SET_STRIPE_CONTROL, CORB_EXECUTE_FUNCTION_RESET, CORB_NID_ROOT, HDA_MAX_CHANNELS,
        HDA_MAX_SENSE_PINS, HDA_MAX_CODECS, AZ_MAX_VOL_SLAVES, AZ_TAG_SPKR, AZ_TAG_PLAYVOL,
        AZ_CLASS_INPUT, AZ_CLASS_OUTPUT, AZ_CLASS_RECORD, AZ_QRK_NONE, AZ_QRK_GPIO_MASK,
        AZ_QRK_GPIO_UNMUTE_0, AZ_QRK_GPIO_UNMUTE_1, AZ_QRK_GPIO_UNMUTE_2, AZ_QRK_GPIO_UNMUTE_3,
        AZ_QRK_GPIO_UNMUTE_4, AZ_QRK_GPIO_UNMUTE_5, AZ_QRK_GPIO_UNMUTE_6, AZ_QRK_GPIO_UNMUTE_7,
        AZ_QRK_GPIO_POL_0, AZ_QRK_WID_MASK, AZ_QRK_WID_CDIN_1C, AZ_QRK_WID_BEEP_1D,
        AZ_QRK_WID_OVREF50, AZ_QRK_WID_AD1981_OAMP, AZ_QRK_WID_TPDOCK1, AZ_QRK_WID_TPDOCK2,
        AZ_QRK_WID_TPDOCK3, AZ_QRK_WID_CLOSE_PCBEEP, AZ_QRK_ROUTE_SPKR2_DAC, AZ_QRK_DOLBY_ATMOS,
        BDLIST_ENTRY_IOC, HDA_BDL_MAX, RIRB_RESP_UNSOL, MI_TARGET_OUTAMP, MI_TARGET_CONNLIST,
        MI_TARGET_PINDIR, MI_TARGET_PINBOOST, MI_TARGET_DAC, MI_TARGET_ADC, MI_TARGET_VOLUME,
        MI_TARGET_SPDIF, MI_TARGET_SPDIF_CC, MI_TARGET_EAPD, MI_TARGET_MUTESET, MI_TARGET_PINSENSE,
        MI_TARGET_SENSESET, MI_TARGET_PLAYVOL, MI_TARGET_RECVOL, MI_TARGET_MIXERSET,
        AZ_CODEC_TYPE_ANALOG, AZ_CODEC_TYPE_DIGITAL, AZ_CODEC_TYPE_HDMI, AZ_SPKR_MUTE_NONE,
        AZ_SPKR_MUTE_SPKR_MUTE, AZ_SPKR_MUTE_SPKR_DIR, AZ_SPKR_MUTE_DAC_MUTE,
    );
    for prefix in [
        "HDA_",
        "COP_",
        "CORB_",
        "AZ_",
        "MI_TARGET_",
        "RIRB_",
        "BDLIST_",
    ] {
        assert_complete(&defs, prefix, &names);
    }
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn driver_defines_match_the_c() {
    let defs = crate::reftest::defines("sys/dev/pci/azalia.c");
    let names = assert_defines!(defs;
        AUFMT_MAX_FREQUENCIES, ICH_PCI_HDBARL, ICH_PCI_HDBARU, ICH_PCI_HDCTL,
        ICH_PCI_HDCTL_CLKDETCLR, ICH_PCI_HDCTL_CLKDETEN, ICH_PCI_HDCTL_CLKDETINV,
        ICH_PCI_HDCTL_SIGNALMODE, ICH_PCI_HDTCSEL, ICH_PCI_HDTCSEL_MASK, ICH_PCI_MMC,
        ICH_PCI_MMC_ME, UNSOLQ_SIZE, ATI_PCIE_SNOOP_REG, ATI_PCIE_SNOOP_MASK,
        ATI_PCIE_SNOOP_ENABLE, NVIDIA_PCIE_SNOOP_REG, NVIDIA_PCIE_SNOOP_MASK,
        NVIDIA_PCIE_SNOOP_ENABLE, NVIDIA_HDA_ISTR_COH_REG, NVIDIA_HDA_OSTR_COH_REG,
        NVIDIA_HDA_STR_COH_ENABLE, INTEL_PCIE_NOSNOOP_REG, INTEL_PCIE_NOSNOOP_MASK,
        INTEL_PCIE_NOSNOOP_ENABLE, MAX_PINS,
    );
    for prefix in ["ICH_PCI_", "ATI_", "NVIDIA_", "INTEL_", "UNSOLQ_", "AUFMT_"] {
        assert_complete(&defs, prefix, &names);
    }
}

// --- host ---------------------------------------------------------------------------------

#[test]
fn layouts_are_the_hardwares() {
    assert_eq!(size_of::<BdlistEntry>(), 16);
    assert_eq!(size_of::<RirbEntry>(), 8);
    assert_eq!(size_of::<CorbEntry>(), 4);
    assert_eq!(size_of::<Dmaposition>(), 8);
}

#[test]
fn header_macros() {
    // QEMU's GCAP: 4 output, 4 input streams, 64-bit.
    assert_eq!(hda_gcap_oss(0x4401), 4);
    assert_eq!(hda_gcap_iss(0x4401), 4);
    assert_eq!(hda_gcap_bss(0x4401), 0);
    assert_eq!(cop_start_nid(0x0002_0004), 2);
    assert_eq!(cop_nsubnodes(0x0002_0004), 4);
    assert_eq!(cop_awcap_type(0x0040_0000), COP_AWTYPE_PIN_COMPLEX);
    let config = 0x0101_0010; // jack, line-out, association 1
    assert_eq!(corb_cd_port(config), CORB_CD_JACK);
    assert_eq!(corb_cd_device(config), CORB_CD_LINEOUT);
    assert_eq!(corb_cd_association(config), 1);
    assert_eq!(corb_cd_loc_geo(config), CORB_CD_REAR);
    assert_eq!(rirb_unsol_tag(0x0400_0000), 1);
    assert_eq!(rirb_resp_codec(0x13), 3);
    assert_eq!(ptr_upper32(0x1_2345_6789), 1);
    assert!(is_mi_target_inamp(mi_target_inamp(15)));
    assert!(!is_mi_target_inamp(MI_TARGET_OUTAMP));
}

#[test]
fn params2fmt() {
    let mut p = AudioParams {
        sample_rate: 48000,
        encoding: AUDIO_ENCODING_SLINEAR_LE,
        precision: 16,
        bps: 2,
        msb: 1,
        channels: 2,
    };
    assert_eq!(azalia_params2fmt(&p), Ok(0x0011));
    p.sample_rate = 44100;
    assert_eq!(azalia_params2fmt(&p), Ok(0x4011));
    p.sample_rate = 8000;
    p.precision = 8;
    p.channels = 1;
    assert_eq!(
        azalia_params2fmt(&p),
        Ok(HDA_SD_FMT_BASE_48 | HDA_SD_FMT_MULT_X1 | HDA_SD_FMT_DIV_BY6)
    );
    p.sample_rate = 32000;
    p.precision = 24;
    assert_eq!(
        azalia_params2fmt(&p),
        Ok(HDA_SD_FMT_MULT_X2 | HDA_SD_FMT_DIV_BY3 | HDA_SD_FMT_BITS_24_32)
    );
    p.sample_rate = 384000;
    assert_eq!(azalia_params2fmt(&p), Err(Errno::EINVAL));
    p.sample_rate = 48000;
    p.channels = 17;
    assert_eq!(azalia_params2fmt(&p), Err(Errno::EINVAL));
}

#[test]
fn paths_through_the_widgets() {
    let codec = test_codec();
    // pin 4 -> mixer 3 -> DAC 2
    assert_eq!(azalia_codec_find_defdac(&codec, 4, 0), 2);
    assert_eq!(azalia_codec_find_defdac(&codec, 5, 0), 2);
    assert!(azalia_widget_check_conn(&codec, 3, 0));
    // the speaker is the only user of the mixer
    assert_eq!(azalia_widget_sole_conn(&codec, 3), 4);
    // no ADC
    assert_eq!(azalia_codec_find_defadc(&codec, 4, 0), -1);

    // The C's guard against `selected` (the size of a pointer), and a selection past the
    // list, which the C would read beyond.
    let mut w = widget(9, COP_AWTYPE_AUDIO_SELECTOR, 0, &[2, 3]);
    assert_eq!(azalia_selected_conn(&w), Some(2));
    w.selected = 1;
    assert_eq!(azalia_selected_conn(&w), Some(3));
    w.selected = 2;
    assert_eq!(azalia_selected_conn(&w), None);
    w.selected = 8;
    assert_eq!(azalia_selected_conn(&w), None);
    w.selected = -1;
    assert_eq!(azalia_selected_conn(&w), None);
}

#[test]
fn pins_sort_by_priority_stably() {
    let p = |nid, prio| IoPin { nid, conv: 2, prio };
    let sorted = azalia_sorted_pins(&[p(4, 0x10), p(5, 0x01), p(6, 0x10), p(7, 0x00)]).unwrap();
    let nids: Vec<NidT> = sorted.iter().map(|p| p.nid).collect();
    assert_eq!(nids, [7, 5, 4, 6]);
}

#[test]
fn groups_labels_and_formats() {
    let mut codec = test_codec();
    for i in codec.widgets() {
        if codec.wi(i).type_ == COP_AWTYPE_AUDIO_OUTPUT {
            codec.a_dacs[codec.na_dacs as usize] = i;
            codec.na_dacs += 1;
        }
    }
    codec.speaker = 4;
    codec.spkr_dac = azalia_codec_find_defdac(&codec, 4, 0);
    azalia_codec_sort_pins(&mut codec).unwrap();
    // the speaker itself is not an output jack
    assert_eq!(
        codec.opins,
        [IoPin {
            nid: 5,
            conv: 2,
            prio: 0
        }]
    );
    azalia_init_dacgroup(&mut codec).unwrap();
    assert_eq!(codec.dacs.ngroups, 1);
    assert_eq!(codec.dacs.groups[0].nconv, 1);
    assert_eq!(codec.dacs.groups[0].conv[0], 2);
    assert_eq!(codec.adcs.ngroups, 0);

    azalia_widget_label_widgets(&mut codec).unwrap();
    assert_eq!(name(&codec, 2), "dac-0:1");
    assert_eq!(name(&codec, 4), "spkr");
    assert_eq!(name(&codec, 5), "line");
    // the mixer only feeds the speaker: it takes its name
    assert_eq!(name(&codec, 3), "spkr");
    assert_eq!(codec.wi(3).parent, 4);

    azalia_codec_construct_format(&mut codec, 0, 0).unwrap();
    assert_eq!(codec.nformats(), 1);
    let f = codec.formats[0];
    assert_eq!(
        (f.mode, f.encoding, f.precision, f.channels),
        (AUMODE_PLAY, AUDIO_ENCODING_SLINEAR_LE, 16, 2)
    );
    assert_eq!(&f.frequency[..f.frequency_type as usize], &[44100, 48000]);

    // audio(4)'s defaults bend to what the codec has: 16 bits, stereo, 48 kHz
    let mut par = AudioParams {
        sample_rate: 22050,
        encoding: AUDIO_ENCODING_ULINEAR_LE,
        precision: 8,
        bps: 1,
        msb: 1,
        channels: 1,
    };
    assert_eq!(azalia_match_format(&codec, AUMODE_PLAY, &par), 1);
    azalia_set_params_sub(&codec, AUMODE_PLAY, &mut par).unwrap();
    assert_eq!(
        (
            par.sample_rate,
            par.encoding,
            par.precision,
            par.channels,
            par.bps
        ),
        (48000, AUDIO_ENCODING_SLINEAR_LE, 16, 2, 2)
    );
    par.sample_rate = 44100;
    azalia_set_params_sub(&codec, AUMODE_PLAY, &mut par).unwrap();
    assert_eq!(par.sample_rate, 44100);
    // no ADC: recording parameters are left alone
    let mut rec = par;
    rec.sample_rate = 12345;
    azalia_set_params_sub(&codec, AUMODE_RECORD, &mut rec).unwrap();
    assert_eq!(rec.sample_rate, 12345);
}

#[test]
fn add_bits_one_format_per_size() {
    let mut codec = test_codec();
    azalia_codec_add_bits(
        &mut codec,
        2,
        COP_PCM_B8 | COP_PCM_B16 | COP_PCM_R80 | COP_PCM_R3840,
        AUMODE_RECORD,
    );
    assert_eq!(codec.nformats(), 2);
    assert_eq!(codec.formats[0].encoding, AUDIO_ENCODING_ULINEAR_LE);
    assert_eq!(codec.formats[0].precision, 8);
    assert_eq!(codec.formats[1].encoding, AUDIO_ENCODING_SLINEAR_LE);
    assert_eq!(codec.formats[1].frequency_type, 2);
    assert_eq!(&codec.formats[1].frequency[..2], &[8000, 384000]);
}

#[test]
fn buffer_shapes() {
    // SAFETY: these methods do not use the handle.
    unsafe {
        assert_eq!(
            azalia_round_buffersize(ptr::null_mut(), AUMODE_PLAY, 1000),
            896
        );
        assert_eq!(
            azalia_round_buffersize(ptr::null_mut(), AUMODE_PLAY, 100),
            128
        );
        let mut p = AudioParams {
            sample_rate: 48000,
            encoding: AUDIO_ENCODING_SLINEAR_LE,
            precision: 16,
            bps: 2,
            msb: 1,
            channels: 2,
        };
        let mut r = p;
        let mut p2 = p;
        assert_eq!(
            azalia_set_nblks(ptr::null_mut(), AUMODE_PLAY, &mut p2, 960, 300),
            256
        );
        assert_eq!(
            azalia_set_nblks(ptr::null_mut(), AUMODE_PLAY, &mut p2, 960, 2),
            2
        );
        // 128 bytes are 32 stereo 16-bit frames
        assert_eq!(
            azalia_set_blksz(ptr::null_mut(), AUMODE_PLAY, &mut p, &mut r, 1000),
            992
        );
        assert_eq!(
            azalia_set_blksz(ptr::null_mut(), AUMODE_PLAY, &mut p, &mut r, 1),
            32
        );
    }
}
