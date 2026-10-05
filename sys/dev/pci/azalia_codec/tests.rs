use super::*;
use crate::dev::pci::azalia::tests::test_codec;
use crate::dev::pci::azalia::{
    AZ_QRK_WID_MASK, COP_AWTYPE_AUDIO_MIXER, CORB_CD_JACK, CORB_CD_LINEOUT, CORB_CD_SPEAKER,
};

#[test]
fn names_and_quirks() {
    let mut codec = test_codec();

    // QEMU's hda-output: a generic codec, no name, no quirks
    codec.vid = 0x1af4_0012;
    azalia_codec_init_vtbl(&mut codec).unwrap();
    assert_eq!((codec.name, codec.qrks), (None, AZ_QRK_NONE));

    codec.vid = 0x10ec_0269;
    codec.subid = 0x21f3_17aa; // Thinkpad T430
    azalia_codec_init_vtbl(&mut codec).unwrap();
    assert_eq!(codec.name, Some("Realtek ALC269"));
    assert_eq!(
        codec.qrks,
        AZ_QRK_WID_CDIN_1C | AZ_QRK_WID_BEEP_1D | AZ_QRK_WID_TPDOCK1
    );

    codec.vid = 0x10ec_0236;
    codec.subid = 0x0000_1028; // a Dell
    azalia_codec_init_vtbl(&mut codec).unwrap();
    assert_eq!((codec.name, codec.qrks), (Some("Realtek ALC3204"), 0));

    codec.vid = 0x8384_7680;
    codec.subid = 0x7680_8384; // APPLE_ID
    azalia_codec_init_vtbl(&mut codec).unwrap();
    assert_eq!(codec.name, Some("Sigmatel STAC9220/1"));
    assert_eq!(
        codec.qrks,
        AZ_QRK_GPIO_POL_0 | AZ_QRK_GPIO_UNMUTE_0 | AZ_QRK_GPIO_UNMUTE_1
    );
}

#[test]
fn reachability() {
    let mut codec = test_codec();
    assert!(azalia_widget_enabled(&codec, 1)); // the audio function
    assert!(azalia_widget_enabled(&codec, 3));
    assert!(!azalia_widget_enabled(&codec, 6)); // past wend
    assert_eq!(azalia_codec_fnode(&codec, 2, 4, 0), 2); // the DAC feeds the speaker
    assert_eq!(azalia_codec_fnode(&codec, 4, 2, 0), -1); // not the other way round
    codec.wi_mut(3).enable = false;
    assert_eq!(azalia_codec_fnode(&codec, 2, 4, 0), -1); // not through a disabled mixer
    assert_eq!(codec.wi(3).type_, COP_AWTYPE_AUDIO_MIXER);
}

#[test]
fn overrides_and_widget_quirks() {
    let mut codec = test_codec();
    let w = codec.wi_mut(5);
    assert_eq!(w.d.pin().device, CORB_CD_LINEOUT);
    azalia_pin_config_ov(w, CORB_CD_DEVICE_MASK, CORB_CD_SPEAKER);
    azalia_pin_config_ov(w, CORB_CD_PORT_MASK, CORB_CD_FIXED);
    assert_eq!(w.d.pin().device, CORB_CD_SPEAKER);
    assert_eq!(w.d.pin().config, 0x8010_0000);
    azalia_pin_config_ov(w, CORB_CD_PORT_MASK, CORB_CD_JACK);
    assert_eq!(w.d.pin().config, 0x0010_0000);
    azalia_ampcap_ov(w, COP_OUTPUT_AMPCAP, 31, 33, 6, 30, true);
    assert_eq!(w.outamp_cap, 0x9e06_211f);
    assert_eq!(w.inamp_cap, 0);

    // AZ_QRK_WID_BEEP_1D turns a disabled node 0x1d into a fixed stereo beep pin.
    codec.qrks = AZ_QRK_WID_BEEP_1D;
    assert_ne!(codec.qrks & AZ_QRK_WID_MASK, 0);
    codec.wend = 0x1e;
    codec.w.resize_with(0x1e, Widget::new);
    azalia_codec_widget_quirks(&mut codec, 0x1d).unwrap();
    let w = codec.wi(0x1d);
    assert!(w.enable);
    assert_eq!(w.d.pin().device, CORB_CD_BEEP);
    assert_ne!(w.widgetcap & COP_AWCAP_STEREO, 0);
}
