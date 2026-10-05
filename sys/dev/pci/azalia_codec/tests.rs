use std::boxed::Box;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::string::String;
use std::vec::Vec;

use super::*;
use crate::dev::pci::azalia::tests::{set_fake_codec, test_codec};
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

// --- the mixer ----------------------------------------------------------------------------

/// A simulated HD Audio codec link: amplifier registers, connection selects and pin
/// controls, and a log of the verbs sent.
#[derive(Default)]
struct FakeHda {
    /// (node, output?, left?, index) -> mute bit | gain.
    amps: HashMap<(NidT, bool, bool, u32), u32>,
    conn_sel: HashMap<NidT, u32>,
    pin_ctl: HashMap<NidT, u32>,
    log: Vec<(NidT, u32, u32)>,
}

impl FakeHda {
    fn verb(&mut self, nid: NidT, control: u32, param: u32) -> Result<u32, Errno> {
        self.log.push((nid, control, param));
        Ok(match control {
            CORB_GET_AMPLIFIER_GAIN_MUTE => *self
                .amps
                .get(&(nid, param & 0x8000 != 0, param & 0x2000 != 0, param & 0xf))
                .unwrap_or(&0),
            CORB_SET_AMPLIFIER_GAIN_MUTE => {
                let key = (
                    nid,
                    param & 0x8000 != 0,
                    param & 0x2000 != 0,
                    (param >> 8) & 0xf,
                );
                self.amps.insert(key, param & 0xff);
                0
            }
            CORB_GET_CONNECTION_SELECT_CONTROL => *self.conn_sel.get(&nid).unwrap_or(&0),
            CORB_SET_CONNECTION_SELECT_CONTROL => {
                self.conn_sel.insert(nid, param);
                0
            }
            CORB_GET_PIN_WIDGET_CONTROL => *self.pin_ctl.get(&nid).unwrap_or(&0),
            CORB_SET_PIN_WIDGET_CONTROL => {
                self.pin_ctl.insert(nid, param);
                0
            }
            _ => 0,
        })
    }

    fn amp(&self, nid: NidT, output: bool, left: bool, index: u32) -> u32 {
        *self.amps.get(&(nid, output, left, index)).unwrap_or(&0)
    }
}

/// Removes the simulated codec when a test ends.
struct Link(Rc<RefCell<FakeHda>>);

impl Link {
    fn new() -> Self {
        let hda = Rc::new(RefCell::new(FakeHda::default()));
        let h = Rc::clone(&hda);
        set_fake_codec(Some(Box::new(move |nid, control, param| {
            h.borrow_mut().verb(nid, control, param)
        })));
        Link(hda)
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        set_fake_codec(None);
    }
}

/// `test_codec` with names, and a stereo output amplifier (31 steps, mute) on the mixer.
fn mixer_codec() -> Codec {
    let mut codec = test_codec();
    for (nid, name) in [(2, "dac2"), (3, "mixer3"), (4, "spkr"), (5, "lineout")] {
        strlcpy(&mut codec.wi_mut(nid).name, name.as_bytes());
    }
    let mixer = codec.wi_mut(3);
    mixer.widgetcap |= COP_AWCAP_OUTAMP | COP_AWCAP_STEREO;
    mixer.outamp_cap = COP_AMPCAP_MUTE | (31 << 8);
    codec.playvols.master = codec.audiofunc;
    codec
}

fn label(m: &MixerItem) -> String {
    String::from_utf8_lossy(cstr(&m.devinfo.label.name)).into_owned()
}

fn labels(codec: &Codec) -> Vec<String> {
    codec.mixers.iter().map(label).collect()
}

fn value_ctrl(l: u8, r: u8) -> MixerCtrl {
    let mut mc = MixerCtrl {
        type_: AUDIO_MIXER_VALUE,
        ..MixerCtrl::default()
    };
    mc.un.value_mut().num_channels = 2;
    mc.un.value_mut().level[0] = l;
    mc.un.value_mut().level[1] = r;
    mc
}

fn enum_ctrl(ord: i32) -> MixerCtrl {
    let mut mc = MixerCtrl {
        type_: AUDIO_MIXER_ENUM,
        ..MixerCtrl::default()
    };
    mc.un.set_ord(ord);
    mc
}

#[test]
fn device_values() {
    let mut codec = mixer_codec();
    // 31 steps: the top level is 255 - 255 % 31 = 248
    let to = |c: &Codec, uv| azalia_mixer_to_device_value(c, 3, MI_TARGET_OUTAMP, uv);
    let from = |c: &Codec, dv| azalia_mixer_from_device_value(c, 3, MI_TARGET_OUTAMP, dv);
    assert_eq!(to(&codec, 0), 0);
    assert_eq!(to(&codec, 128), 16);
    assert_eq!(to(&codec, 247), 30);
    assert_eq!(to(&codec, 248), 31);
    assert_eq!(to(&codec, 255), 31);
    assert_eq!(from(&codec, 0), 0);
    assert_eq!(from(&codec, 16), 128);
    assert_eq!(from(&codec, 30), 240);
    assert_eq!(from(&codec, 31), 248);
    assert_eq!(from(&codec, 99), 248);

    // an offset (ctloff = 3, 10 steps): 0 maps to the offset; below it wraps to the top
    codec.wi_mut(3).outamp_cap = COP_AMPCAP_MUTE | (3 << 24) | (10 << 8);
    assert_eq!(to(&codec, 0), 3);
    assert_eq!(to(&codec, 255), 13);
    assert_eq!(from(&codec, 3), 0);
    assert_eq!(from(&codec, 8), 125);
    assert_eq!(from(&codec, 2), 250);

    // no steps: always the bottom
    codec.wi_mut(3).outamp_cap = COP_AMPCAP_MUTE;
    assert_eq!(to(&codec, 200), 0);
    assert_eq!(from(&codec, 5), 0);

    // an input amplifier of the same widget
    codec.wi_mut(3).inamp_cap = 20 << 8;
    assert_eq!(azalia_mixer_to_device_value(&codec, 3, 0, 255), 20);
    // an unknown target counts 255 steps
    assert_eq!(
        azalia_mixer_to_device_value(&codec, 3, MI_TARGET_PINDIR, 100),
        100
    );
    assert_eq!(
        azalia_mixer_from_device_value(&codec, 3, MI_TARGET_PINDIR, 100),
        100
    );
}

#[test]
fn devinfo_offon() {
    let mut d = MixerDevinfo::zeroed();
    azalia_devinfo_offon(&mut d);
    assert_eq!(d.type_, AUDIO_MIXER_ENUM);
    assert_eq!(d.un.e().num_mem, 2);
    assert_eq!(cstr(&d.un.e().member[0].label.name), b"off");
    assert_eq!(d.un.e().member[1].ord, 1);
    assert_eq!(cstr(&d.un.e().member[1].label.name), b"on");
}

#[test]
fn registers_the_controls_of_the_widgets() {
    let mut codec = mixer_codec();
    azalia_mixer_register(&mut codec).unwrap();
    assert_eq!(
        labels(&codec),
        [
            "inputs",
            "outputs",
            "record",
            "mixer3_mute",
            "mixer3",
            "mixer3_source",
            "spkr_source",
            "lineout_source",
        ]
    );
    let m = &codec.mixers;
    assert_eq!(m[AZ_CLASS_RECORD as usize].devinfo.type_, AUDIO_MIXER_CLASS);

    // an output mute and an output gain: stereo, 31 steps, delta 255 / 31
    assert_eq!((m[3].nid, m[3].target), (3, MI_TARGET_OUTAMP));
    assert_eq!(m[3].devinfo.type_, AUDIO_MIXER_ENUM);
    assert_eq!(m[3].devinfo.mixer_class, AZ_CLASS_OUTPUT);
    assert_eq!(m[4].devinfo.type_, AUDIO_MIXER_VALUE);
    assert_eq!(m[4].devinfo.un.v().num_channels, 2);
    assert_eq!(m[4].devinfo.un.v().delta, 8);

    // the hardcoded inputs of a mixer without an input amplifier
    assert_eq!((m[5].nid, m[5].target), (3, MI_TARGET_MIXERSET));
    assert_eq!(m[5].devinfo.type_, AUDIO_MIXER_SET);
    assert_eq!(m[5].devinfo.un.s().num_mem, 1);
    assert_eq!(m[5].devinfo.un.s().member[0].mask, 1);
    assert_eq!(cstr(&m[5].devinfo.un.s().member[0].label.name), b"dac2");

    // a pin's selector lists the enabled connections by position
    assert_eq!(m[6].target, MI_TARGET_CONNLIST);
    assert_eq!(m[6].devinfo.un.e().num_mem, 1);
    assert_eq!(m[6].devinfo.un.e().member[0].ord, 0);
    assert_eq!(cstr(&m[6].devinfo.un.e().member[0].label.name), b"mixer3");
    assert_eq!(m[7].devinfo.mixer_class, AZ_CLASS_OUTPUT);

    // a disabled connection is not listed, a node that is the microphone has no controls
    let mut codec = mixer_codec();
    codec.wi_mut(3).enable = false;
    codec.mic = 5;
    azalia_mixer_register(&mut codec).unwrap();
    // (the speaker's selector stays, with no member: its one connection is disabled)
    assert_eq!(
        labels(&codec),
        ["inputs", "outputs", "record", "spkr_source"]
    );
    assert_eq!(codec.mixers[3].devinfo.un.e().num_mem, 0);
}

#[test]
fn registers_the_volume_groups_and_the_converter_modes() {
    let mut codec = mixer_codec();
    codec.playvols.nslaves = 1;
    codec.playvols.slaves[0] = 3;
    codec.playvols.mask = 1;
    codec.playvols.cur = 1;
    codec.dacs.ngroups = 2;
    azalia_mixer_register(&mut codec).unwrap();
    let n = codec.mixers.len();
    assert_eq!(
        &labels(&codec)[n - 4..],
        ["master", "mute", "slaves", "mode"]
    );
    azalia_mixer_fix_indexes(&mut codec).unwrap();
    let d: Vec<_> = codec.mixers[n - 4..]
        .iter()
        .map(|m| (m.devinfo.index, m.devinfo.prev, m.devinfo.next))
        .collect();
    let i = (n - 4) as i32;
    assert_eq!(
        d,
        [
            (i, AUDIO_MIXER_LAST, i + 1),
            (i + 1, i, i + 2),
            (i + 2, i + 1, AUDIO_MIXER_LAST),
            (i + 3, AUDIO_MIXER_LAST, AUDIO_MIXER_LAST),
        ]
    );
    let slaves = &codec.mixers[n - 2].devinfo;
    assert_eq!(slaves.type_, AUDIO_MIXER_SET);
    assert_eq!(slaves.un.s().num_mem, 1);
    assert_eq!(cstr(&slaves.un.s().member[0].label.name), b"mixer3");
    let mode = &codec.mixers[n - 1];
    assert_eq!((mode.nid, mode.target), (1, MI_TARGET_DAC));
    assert_eq!(cstr(&mode.devinfo.un.e().member[1].label.name), b"digital");
    // the classes end their chains too
    assert_eq!(codec.mixers[0].devinfo.index, 0);
    assert_eq!(codec.mixers[0].devinfo.next, AUDIO_MIXER_LAST);
    assert!(
        codec
            .mixers
            .iter()
            .enumerate()
            .all(|(i, m)| m.devinfo.index == i as i32)
    );
}

#[test]
fn the_list_grows_by_ten() {
    let mut codec = test_codec();
    codec.mixers = Vec::new();
    codec.mixers.try_reserve_exact(10).unwrap();
    let cap = codec.mixers.capacity();
    azalia_mixer_ensure_capacity(&mut codec, cap).unwrap();
    assert_eq!(codec.mixers.capacity(), cap);
    azalia_mixer_ensure_capacity(&mut codec, cap + 1).unwrap();
    assert!(codec.mixers.capacity() >= cap + 10);
    azalia_mixer_ensure_capacity(&mut codec, 100).unwrap();
    assert!(codec.mixers.capacity() >= 100);
}

#[test]
fn defaults_and_amplifier_verbs() {
    let link = Link::new();
    let mut codec = mixer_codec();
    azalia_mixer_init(&mut codec).unwrap();
    {
        let hda = link.0.borrow();
        // unmuted, half gain (127 * 31 / 248 = 15) on both channels
        assert_eq!(hda.amp(3, true, true, 0), 15);
        assert_eq!(hda.amp(3, true, false, 0), 15);
        // the pin selectors pick their first connection
        assert_eq!(hda.conn_sel.get(&4), Some(&0));
        assert_eq!(hda.conn_sel.get(&5), Some(&0));
    }

    // the control reads back as 15 steps of 8
    let idx = codec
        .mixers
        .iter()
        .position(|m| label(m) == "mixer3")
        .unwrap();
    let mut mc = MixerCtrl {
        dev: idx as i32,
        type_: AUDIO_MIXER_VALUE,
        ..MixerCtrl::default()
    };
    let (nid, target) = (codec.mixers[idx].nid, codec.mixers[idx].target);
    azalia_mixer_get(&codec, nid, target, &mut mc).unwrap();
    assert_eq!(mc.un.value().num_channels, 2);
    assert_eq!(mc.un.value().level, [120, 120, 0, 0, 0, 0, 0, 0]);

    // mute keeps the gain, unmute keeps it too
    link.0.borrow_mut().log.clear();
    azalia_mixer_set(&mut codec, 3, MI_TARGET_OUTAMP, &enum_ctrl(1)).unwrap();
    assert_eq!(
        link.0.borrow().log,
        [
            (3, CORB_GET_AMPLIFIER_GAIN_MUTE, 0xa000),
            (3, CORB_SET_AMPLIFIER_GAIN_MUTE, 0xa000 | 0x80 | 15),
            (3, CORB_GET_AMPLIFIER_GAIN_MUTE, 0x8000),
            (3, CORB_SET_AMPLIFIER_GAIN_MUTE, 0x9000 | 0x80 | 15),
        ]
    );
    let mut mc = enum_ctrl(0);
    azalia_mixer_get(&codec, 3, MI_TARGET_OUTAMP, &mut mc).unwrap();
    assert_eq!(mc.un.ord(), 1);
    azalia_mixer_set(&mut codec, 3, MI_TARGET_OUTAMP, &enum_ctrl(0)).unwrap();
    assert_eq!(link.0.borrow().amp(3, true, true, 0), 15);

    // a new level: the mute bit is kept
    azalia_mixer_set(&mut codec, 3, MI_TARGET_OUTAMP, &enum_ctrl(1)).unwrap();
    azalia_mixer_set(&mut codec, 3, MI_TARGET_OUTAMP, &value_ctrl(248, 0)).unwrap();
    assert_eq!(link.0.borrow().amp(3, true, true, 0), 0x80 | 31);
    assert_eq!(link.0.borrow().amp(3, true, false, 0), 0x80 | 0);

    // mono request on a stereo widget: only the left channel is written
    azalia_mixer_set(&mut codec, 3, MI_TARGET_OUTAMP, &enum_ctrl(0)).unwrap();
    let mut mono = value_ctrl(100, 0);
    mono.un.value_mut().num_channels = 1;
    azalia_mixer_set(&mut codec, 3, MI_TARGET_OUTAMP, &mono).unwrap();
    assert_eq!(link.0.borrow().amp(3, true, true, 0), 12);
    mono.un.value_mut().num_channels = 0;
    assert_eq!(
        azalia_mixer_set(&mut codec, 3, MI_TARGET_OUTAMP, &mono),
        Err(Errno::EINVAL)
    );
}

#[test]
fn selectors_and_the_rest() {
    let _link = Link::new();
    let mut codec = mixer_codec();
    azalia_mixer_init(&mut codec).unwrap();

    // a selection must be one of the enabled connections
    let mut mc = enum_ctrl(0);
    azalia_mixer_set(&mut codec, 4, MI_TARGET_CONNLIST, &mc).unwrap();
    mc.un.set_ord(1);
    assert_eq!(
        azalia_mixer_set(&mut codec, 4, MI_TARGET_CONNLIST, &mc),
        Err(Errno::EINVAL)
    );
    mc.un.set_ord(-1);
    assert_eq!(
        azalia_mixer_set(&mut codec, 4, MI_TARGET_CONNLIST, &mc),
        Err(Errno::EINVAL)
    );
    azalia_mixer_get(&codec, 4, MI_TARGET_CONNLIST, &mut mc).unwrap();
    assert_eq!(mc.un.ord(), 0);

    // pin direction: none, output, input
    for ord in [0, 1, 2] {
        azalia_mixer_set(&mut codec, 5, MI_TARGET_PINDIR, &enum_ctrl(ord)).unwrap();
        let mut mc = enum_ctrl(-1);
        azalia_mixer_get(&codec, 5, MI_TARGET_PINDIR, &mut mc).unwrap();
        assert_eq!(mc.un.ord(), ord);
    }

    // headphone boost and EAPD take 0 or 1
    assert_eq!(
        azalia_mixer_set(&mut codec, 5, MI_TARGET_PINBOOST, &enum_ctrl(2)),
        Err(Errno::EINVAL)
    );
    assert_eq!(
        azalia_mixer_set(&mut codec, 5, MI_TARGET_EAPD, &enum_ctrl(2)),
        Err(Errno::EINVAL)
    );

    // the converter groups: the selection does not change while running
    codec.dacs.ngroups = 1;
    assert_eq!(
        azalia_mixer_set(&mut codec, 1, MI_TARGET_DAC, &enum_ctrl(1)),
        Err(Errno::EINVAL)
    );
    codec.running = 1;
    assert_eq!(
        azalia_mixer_set(&mut codec, 1, MI_TARGET_DAC, &enum_ctrl(0)),
        Err(Errno::EBUSY)
    );
    let mut mc = enum_ctrl(-1);
    azalia_mixer_get(&codec, 1, MI_TARGET_DAC, &mut mc).unwrap();
    assert_eq!(mc.un.ord(), 0);

    // a class has no value; an unknown target is an error
    let mut class = MixerCtrl::default();
    azalia_mixer_set(&mut codec, 0, 0, &class).unwrap();
    azalia_mixer_get(&codec, 0, 0, &mut class).unwrap();
    assert_eq!(
        azalia_mixer_set(&mut codec, 3, 0x1ff, &enum_ctrl(0)),
        Err(Errno::EIO)
    );
    let mut mc = enum_ctrl(0);
    assert_eq!(azalia_mixer_get(&codec, 3, 0x1ff, &mut mc), Err(Errno::EIO));
}

#[test]
fn volume_groups_drive_their_slaves() {
    let link = Link::new();
    let mut codec = mixer_codec();
    codec.playvols.nslaves = 1;
    codec.playvols.slaves[0] = 3;
    codec.playvols.mask = 1;
    codec.playvols.cur = 1;
    azalia_mixer_init(&mut codec).unwrap();
    // the group starts from the slave's default
    assert_eq!((codec.playvols.vol_l, codec.playvols.vol_r), (120, 120));

    // the master volume sets each slave through its steps
    azalia_mixer_set(&mut codec, 1, MI_TARGET_PLAYVOL, &value_ctrl(200, 100)).unwrap();
    assert_eq!(link.0.borrow().amp(3, true, true, 0), 25);
    assert_eq!(link.0.borrow().amp(3, true, false, 0), 12);
    let mut mc = value_ctrl(0, 0);
    azalia_mixer_get(&codec, 1, MI_TARGET_PLAYVOL, &mut mc).unwrap();
    assert_eq!(&mc.un.value().level[..2], [200, 100]);

    // the master mute mutes it, and a muted slave keeps its gain
    azalia_mixer_set(&mut codec, 1, MI_TARGET_PLAYVOL, &enum_ctrl(1)).unwrap();
    assert_eq!(link.0.borrow().amp(3, true, true, 0), 0x80 | 25);
    azalia_mixer_set(&mut codec, 1, MI_TARGET_PLAYVOL, &value_ctrl(50, 50)).unwrap();
    assert_eq!(link.0.borrow().amp(3, true, true, 0), 0x80 | 25);
    azalia_mixer_set(&mut codec, 1, MI_TARGET_PLAYVOL, &enum_ctrl(0)).unwrap();
    assert_eq!(link.0.borrow().amp(3, true, true, 0), 25);

    // the slave set is limited to the group's mask
    let mut set = MixerCtrl {
        type_: AUDIO_MIXER_SET,
        ..MixerCtrl::default()
    };
    set.un.set_mask(0xff);
    azalia_mixer_set(&mut codec, 1, MI_TARGET_PLAYVOL, &set).unwrap();
    assert_eq!(codec.playvols.cur, 1);
    let mut mc = MixerCtrl {
        type_: AUDIO_MIXER_SET,
        ..MixerCtrl::default()
    };
    azalia_mixer_get(&codec, 1, MI_TARGET_PLAYVOL, &mut mc).unwrap();
    assert_eq!(mc.un.mask(), 1);

    // the master volume wants two channels
    let mut mono = value_ctrl(1, 1);
    mono.un.value_mut().num_channels = 1;
    assert_eq!(
        azalia_mixer_set(&mut codec, 1, MI_TARGET_PLAYVOL, &mono),
        Err(Errno::EINVAL)
    );
}
