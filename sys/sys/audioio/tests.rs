use core::mem::{offset_of, size_of};

use super::*;

#[test]
fn ioctl_numbers_match_openbsd() {
    // Values of the OpenBSD amd64 and arm64 headers (both LP64, same encoding).
    assert_eq!(AUDIO_GETDEV, 0x4030_411b);
    assert_eq!(AUDIO_GETPOS, 0x4010_4123);
    assert_eq!(AUDIO_GETPAR, 0x4040_4124);
    assert_eq!(AUDIO_SETPAR, 0xc040_4125);
    assert_eq!(AUDIO_START, 0x2000_4126);
    assert_eq!(AUDIO_STOP, 0x2000_4127);
    assert_eq!(AUDIO_GETSTATUS, 0x4020_4128);
    assert_eq!(AUDIO_MIXER_READ, 0xc014_4d00);
    assert_eq!(AUDIO_MIXER_WRITE, 0xc014_4d01);
    assert_eq!(AUDIO_MIXER_DEVINFO, 0xc32c_4d02);
}

#[test]
fn layouts_match_the_c() {
    assert_eq!(offset_of!(AudioSwpar, round), 36);
    assert_eq!(offset_of!(AudioSwpar, _spare), 40);
    assert_eq!(offset_of!(AudioDevice, config), 32);
    assert_eq!(offset_of!(MixerLevel, level), 4);
    assert_eq!(offset_of!(AudioMixerName, msg_id), 16);
    assert_eq!(offset_of!(AudioMixerEnumMember, ord), 20);
    assert_eq!(size_of::<AudioMixerEnumMember>(), 24);
    assert_eq!(offset_of!(AudioMixerValue, num_channels), 20);
    assert_eq!(offset_of!(MixerDevinfo, label), 4);
    assert_eq!(offset_of!(MixerDevinfo, type_), 24);
    assert_eq!(offset_of!(MixerDevinfo, mixer_class), 28);
    assert_eq!(offset_of!(MixerDevinfo, next), 32);
    assert_eq!(offset_of!(MixerDevinfo, prev), 36);
    assert_eq!(offset_of!(MixerDevinfo, un), 40);
    assert_eq!(offset_of!(MixerCtrl, un), 8);
}

#[test]
fn union_views_share_the_bytes() {
    let mut c = MixerCtrl::default();
    c.un.value_mut().num_channels = 2;
    c.un.value_mut().level[1] = 200;
    assert_eq!(c.un.ord(), 2);
    assert_eq!(c.un.mask(), 2);
    c.un.set_ord(1);
    assert_eq!(c.un.value().num_channels, 1);
    assert_eq!(c.un.value().level[1], 200);

    let mut d = MixerDevinfo::zeroed();
    d.un.e_mut().num_mem = 3;
    d.un.e_mut().member[2].ord = 7;
    assert_eq!(d.un.s().num_mem, 3);
    assert_eq!(d.un.s().member[2].mask, 7);
    d.un.v_mut().num_channels = 2;
    // `v.num_channels` (offset 20) overlays the first member's `label.msg_id`.
    assert_eq!(d.un.e().member[0].label.msg_id, 2);
    assert_eq!(d.un.e().num_mem, 3);
}

#[test]
fn initpar_is_all_ones() {
    let p = AudioSwpar::initpar();
    assert_eq!(p.rate, !0);
    assert_eq!(p._spare, [!0; 6]);
}

/// `_IOR('A', 27, struct audio_device)` -> (direction, group, number, struct name).
fn parse_ioctl(text: &str) -> (&str, u8, u8, &str) {
    let (dir, rest) = text.split_once('(').expect("an _IO macro");
    let args: std::vec::Vec<&str> = rest
        .trim_end_matches(')')
        .split(',')
        .map(str::trim)
        .collect();
    let group = args[0].as_bytes()[1];
    let num = args[1].parse().expect("a number");
    (dir, group, num, args.get(2).copied().unwrap_or(""))
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn values_match_the_c_header() {
    let defs = crate::reftest::defines("sys/sys/audioio.h");
    let ours = crate::reftest::assert_defines!(defs;
        AUMODE_PLAY, AUMODE_RECORD, MAX_AUDIO_DEV_LEN, AUDIO_MIN_GAIN, AUDIO_MAX_GAIN,
        AUDIO_MIXER_LEVEL_MONO, AUDIO_MIXER_LEVEL_LEFT, AUDIO_MIXER_LEVEL_RIGHT,
        AUDIO_MIXER_CLASS, AUDIO_MIXER_ENUM, AUDIO_MIXER_SET, AUDIO_MIXER_VALUE,
        AUDIO_MIXER_LAST);

    let ioctls: &[(&str, u64, &str, usize)] = &[
        (
            "AUDIO_GETDEV",
            AUDIO_GETDEV,
            "struct audio_device",
            size_of::<AudioDevice>(),
        ),
        (
            "AUDIO_GETPOS",
            AUDIO_GETPOS,
            "struct audio_pos",
            size_of::<AudioPos>(),
        ),
        (
            "AUDIO_GETPAR",
            AUDIO_GETPAR,
            "struct audio_swpar",
            size_of::<AudioSwpar>(),
        ),
        (
            "AUDIO_SETPAR",
            AUDIO_SETPAR,
            "struct audio_swpar",
            size_of::<AudioSwpar>(),
        ),
        ("AUDIO_START", AUDIO_START, "", 0),
        ("AUDIO_STOP", AUDIO_STOP, "", 0),
        (
            "AUDIO_GETSTATUS",
            AUDIO_GETSTATUS,
            "struct audio_status",
            size_of::<AudioStatus>(),
        ),
        (
            "AUDIO_MIXER_READ",
            AUDIO_MIXER_READ,
            "mixer_ctrl_t",
            size_of::<MixerCtrl>(),
        ),
        (
            "AUDIO_MIXER_WRITE",
            AUDIO_MIXER_WRITE,
            "mixer_ctrl_t",
            size_of::<MixerCtrl>(),
        ),
        (
            "AUDIO_MIXER_DEVINFO",
            AUDIO_MIXER_DEVINFO,
            "mixer_devinfo_t",
            size_of::<MixerDevinfo>(),
        ),
    ];
    let mut names = ours;
    for &(name, value, ty, size) in ioctls {
        let text = defs.get(name).unwrap_or_else(|| panic!("{name} missing"));
        let (dir, group, num, cty) = parse_ioctl(text);
        let want = match dir {
            "_IO" => 0x2000_0000 | (u64::from(group) << 8) | u64::from(num),
            "_IOR" => {
                0x4000_0000 | ((size as u64) << 16) | (u64::from(group) << 8) | u64::from(num)
            }
            "_IOWR" => {
                0xc000_0000 | ((size as u64) << 16) | (u64::from(group) << 8) | u64::from(num)
            }
            other => panic!("{name}: {other}"),
        };
        assert_eq!(value, want, "{name}");
        assert_eq!(cty, ty, "{name}");
        names.push(name);
    }

    let strings: &[(&str, &[u8])] = &[
        ("AudioNmicrophone", AudioNmicrophone),
        ("AudioNline", AudioNline),
        ("AudioNcd", AudioNcd),
        ("AudioNdac", AudioNdac),
        ("AudioNaux", AudioNaux),
        ("AudioNrecord", AudioNrecord),
        ("AudioNvolume", AudioNvolume),
        ("AudioNmonitor", AudioNmonitor),
        ("AudioNtreble", AudioNtreble),
        ("AudioNmid", AudioNmid),
        ("AudioNbass", AudioNbass),
        ("AudioNbassboost", AudioNbassboost),
        ("AudioNspeaker", AudioNspeaker),
        ("AudioNheadphone", AudioNheadphone),
        ("AudioNoutput", AudioNoutput),
        ("AudioNinput", AudioNinput),
        ("AudioNmaster", AudioNmaster),
        ("AudioNstereo", AudioNstereo),
        ("AudioNmono", AudioNmono),
        ("AudioNloudness", AudioNloudness),
        ("AudioNspatial", AudioNspatial),
        ("AudioNsurround", AudioNsurround),
        ("AudioNpseudo", AudioNpseudo),
        ("AudioNmute", AudioNmute),
        ("AudioNenhanced", AudioNenhanced),
        ("AudioNpreamp", AudioNpreamp),
        ("AudioNon", AudioNon),
        ("AudioNoff", AudioNoff),
        ("AudioNmode", AudioNmode),
        ("AudioNsource", AudioNsource),
        ("AudioNfmsynth", AudioNfmsynth),
        ("AudioNwave", AudioNwave),
        ("AudioNmidi", AudioNmidi),
        ("AudioNmixerout", AudioNmixerout),
        ("AudioNswap", AudioNswap),
        ("AudioNagc", AudioNagc),
        ("AudioNdelay", AudioNdelay),
        ("AudioNselect", AudioNselect),
        ("AudioNvideo", AudioNvideo),
        ("AudioNcenter", AudioNcenter),
        ("AudioNdepth", AudioNdepth),
        ("AudioNlfe", AudioNlfe),
        ("AudioNextamp", AudioNextamp),
        ("AudioCinputs", AudioCinputs),
        ("AudioCoutputs", AudioCoutputs),
        ("AudioCrecord", AudioCrecord),
        ("AudioCmonitor", AudioCmonitor),
        ("AudioCequalization", AudioCequalization),
    ];
    for &(name, value) in strings {
        let text = defs.get(name).unwrap_or_else(|| panic!("{name} missing"));
        let quoted = text.trim().trim_matches('"');
        assert_eq!(quoted.as_bytes(), value, "{name}");
        names.push(name);
    }
    crate::reftest::assert_complete(&defs, "AUDIO", &names);
    crate::reftest::assert_complete(&defs, "Audio", &names);
    crate::reftest::assert_complete(&defs, "AUMODE", &names);
}
