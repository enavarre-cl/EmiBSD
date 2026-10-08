//! Host tests of `wsmousevar.rs`: the MT codes, and the constants and enumerations
//! against `<dev/wscons/wsmousevar.h>`.

use super::*;

#[test]
fn mt_codes() {
    assert!(!wsmouse_is_mt_code(WSMOUSE_TOUCH_WIDTH));
    assert!(wsmouse_is_mt_code(WSMOUSE_MT_REL_X));
    assert!(wsmouse_is_mt_code(WSMOUSE_MT_PRESSURE));
    assert!(!wsmouse_is_mt_code(WSMOUSE_MT_PRESSURE + 1));
}

/// The constants against `<dev/wscons/wsmousevar.h>`.
#[test]
#[ignore = "needs OPENBSD_SRC"]
fn constants_match_the_c_header() {
    let defs = crate::reftest::defines("sys/dev/wscons/wsmousevar.h");
    let _ = crate::reftest::assert_defines!(defs;
        WSMOUSE_DEFAULT_PRESSURE, WSMOUSE_MT_SLOTS_MAX, WSMOUSE_MT_INIT_TRACKING,
        WSMOUSEHW_LR_DOWN, WSMOUSEHW_MT_TRACKING,
    );
    // The enumerations, in the order of the header.
    let text =
        std::fs::read_to_string(crate::reftest::openbsd_src().join("sys/dev/wscons/wsmousevar.h"))
            .unwrap();
    let enum_names = |name: &str| -> std::vec::Vec<std::string::String> {
        let start = text.find(name).unwrap();
        let body = &text[start..start + text[start..].find("};").unwrap()];
        body.lines()
            .skip(1)
            .map(|l| l.trim().trim_end_matches(','))
            .filter(|l| !l.is_empty())
            .map(std::string::String::from)
            .collect()
    };
    assert_eq!(
        enum_names("enum wsmouseval {"),
        [
            "WSMOUSE_REL_X",
            "WSMOUSE_ABS_X",
            "WSMOUSE_REL_Y",
            "WSMOUSE_ABS_Y",
            "WSMOUSE_PRESSURE",
            "WSMOUSE_CONTACTS",
            "WSMOUSE_TOUCH_WIDTH",
            "WSMOUSE_MT_REL_X",
            "WSMOUSE_MT_ABS_X",
            "WSMOUSE_MT_REL_Y",
            "WSMOUSE_MT_ABS_Y",
            "WSMOUSE_MT_PRESSURE",
        ]
    );
    assert_eq!(WSMOUSE_MT_PRESSURE, 11);
    assert_eq!(
        enum_names("enum wsmousehw_type {"),
        [
            "WSMOUSEHW_RAW",
            "WSMOUSEHW_MOUSE",
            "WSMOUSEHW_TOUCHPAD",
            "WSMOUSEHW_CLICKPAD",
            "WSMOUSEHW_TPANEL",
        ]
    );
    assert_eq!(WSMOUSEHW_TPANEL, 4);
}
