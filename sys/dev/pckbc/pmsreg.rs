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
/* </LICENSES> */

/* <CODE> */
//! The PS/2 mouse and touchpad definitions: the mouse's commands, its packet bits, the
//! knocks and queries of the Synaptics, ALPS and Elantech touchpads and the decoding of
//! their answers, `<dev/pckbc/pmsreg.h>`.
//!
//! Upstream: sys/dev/pckbc/pmsreg.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The bytes that go on the wire or come back from the device (the mouse commands, the
//!   reset answer, the packet bits, the magic answers of the knocks, the Elantech commands
//!   and capability bits) are `u8`; the bit masks of the Synaptics answers, which the C
//!   keeps in `int`s, and the coordinate limits are `i32`; the ALPS gesture values are
//!   `u32`, the C's `u_int gesture`.
//! - The function-like macros are `const fn`s of the same names in lower case
//!   (`SYNAPTICS_ID_MODEL(id)` is [`synaptics_id_model`]).

/// `PMS_SET_SCALE11`: set scaling 1:1.
pub const PMS_SET_SCALE11: u8 = 0xe6;
/// `PMS_SET_SCALE21`: set scaling 2:1.
pub const PMS_SET_SCALE21: u8 = 0xe7;
/// `PMS_SET_RES`: set resolution (0..3).
pub const PMS_SET_RES: u8 = 0xe8;
/// `PMS_SEND_DEV_STATUS`: status request.
pub const PMS_SEND_DEV_STATUS: u8 = 0xe9;
/// `PMS_SET_STREAM_MODE`.
pub const PMS_SET_STREAM_MODE: u8 = 0xea;
/// `PMS_SEND_DEV_DATA`: read data.
pub const PMS_SEND_DEV_DATA: u8 = 0xeb;
/// `PMS_RESET_WRAP_MODE`.
pub const PMS_RESET_WRAP_MODE: u8 = 0xec;
/// `PMS_SET_WRAP_MODE`.
pub const PMS_SET_WRAP_MODE: u8 = 0xed;
/// `PMS_SET_REMOTE_MODE`.
pub const PMS_SET_REMOTE_MODE: u8 = 0xf0;
/// `PMS_SEND_DEV_ID`: read device type.
pub const PMS_SEND_DEV_ID: u8 = 0xf2;
/// `PMS_SET_SAMPLE`: set sampling rate.
pub const PMS_SET_SAMPLE: u8 = 0xf3;
/// `PMS_DEV_ENABLE`: mouse on.
pub const PMS_DEV_ENABLE: u8 = 0xf4;
/// `PMS_DEV_DISABLE`: mouse off.
pub const PMS_DEV_DISABLE: u8 = 0xf5;
/// `PMS_SET_DEFAULTS`.
pub const PMS_SET_DEFAULTS: u8 = 0xf6;
/// `PMS_RESEND`.
pub const PMS_RESEND: u8 = 0xfe;
/// `PMS_RESET`: reset.
pub const PMS_RESET: u8 = 0xff;

/// `PMS_RSTDONE`: the answer to a reset.
pub const PMS_RSTDONE: u8 = 0xaa;

/// `PMS_PS2_BUTTONSMASK`: the buttons of a PS/2 mouse data packet.
pub const PMS_PS2_BUTTONSMASK: u8 = 0x07;
/// `PMS_PS2_BUTTON1`: left.
pub const PMS_PS2_BUTTON1: u8 = 0x01;
/// `PMS_PS2_BUTTON2`: middle.
pub const PMS_PS2_BUTTON2: u8 = 0x04;
/// `PMS_PS2_BUTTON3`: right.
pub const PMS_PS2_BUTTON3: u8 = 0x02;
/// `PMS_PS2_XNEG`.
pub const PMS_PS2_XNEG: u8 = 0x10;
/// `PMS_PS2_YNEG`.
pub const PMS_PS2_YNEG: u8 = 0x20;

/// `PMS_INTELLI_MAGIC1`.
pub const PMS_INTELLI_MAGIC1: u8 = 200;
/// `PMS_INTELLI_MAGIC2`.
pub const PMS_INTELLI_MAGIC2: u8 = 100;
/// `PMS_INTELLI_MAGIC3`.
pub const PMS_INTELLI_MAGIC3: u8 = 80;
/// `PMS_INTELLI_ID`.
pub const PMS_INTELLI_ID: u8 = 0x03;

/// `PMS_ALPS_MAGIC1`.
pub const PMS_ALPS_MAGIC1: u8 = 0;
/// `PMS_ALPS_MAGIC2`.
pub const PMS_ALPS_MAGIC2: u8 = 0;
/// `PMS_ALPS_MAGIC3_1`.
pub const PMS_ALPS_MAGIC3_1: u8 = 10;
/// `PMS_ALPS_MAGIC3_2`.
pub const PMS_ALPS_MAGIC3_2: u8 = 80;
/// `PMS_ALPS_MAGIC3_3`.
pub const PMS_ALPS_MAGIC3_3: u8 = 100;

/// `PMS_ELANTECH_MAGIC1`.
pub const PMS_ELANTECH_MAGIC1: u8 = 0x3c;
/// `PMS_ELANTECH_MAGIC2`.
pub const PMS_ELANTECH_MAGIC2: u8 = 0x03;
/// `PMS_ELANTECH_MAGIC3_1`.
pub const PMS_ELANTECH_MAGIC3_1: u8 = 0xc8;
/// `PMS_ELANTECH_MAGIC3_2`.
pub const PMS_ELANTECH_MAGIC3_2: u8 = 0x00;

/// `PMS_ALPS_PS2_MASK`: checking for almost-standard PS/2 packet. Note: ALPS devices never
/// signal overflow condition.
pub const PMS_ALPS_PS2_MASK: u8 = 0xc8;
/// `PMS_ALPS_PS2_VALID`.
pub const PMS_ALPS_PS2_VALID: u8 = 0x08;

/// `PMS_ALPS_INTERLEAVED_MASK`: checking for interleaved packet.
pub const PMS_ALPS_INTERLEAVED_MASK: u8 = 0xcf;
/// `PMS_ALPS_INTERLEAVED_VALID`.
pub const PMS_ALPS_INTERLEAVED_VALID: u8 = 0x0f;

/// `PMS_ALPS_MASK`: checking for non first byte.
pub const PMS_ALPS_MASK: u8 = 0x80;
/// `PMS_ALPS_VALID`.
pub const PMS_ALPS_VALID: u8 = 0x00;

/// `SYNAPTICS_QUE_IDENTIFY`: Synaptics query.
pub const SYNAPTICS_QUE_IDENTIFY: i32 = 0x00;
/// `SYNAPTICS_QUE_MODES`.
pub const SYNAPTICS_QUE_MODES: i32 = 0x01;
/// `SYNAPTICS_QUE_CAPABILITIES`.
pub const SYNAPTICS_QUE_CAPABILITIES: i32 = 0x02;
/// `SYNAPTICS_QUE_MODEL`.
pub const SYNAPTICS_QUE_MODEL: i32 = 0x03;
/// `SYNAPTICS_QUE_SERIAL_NUMBER_PREFIX`.
pub const SYNAPTICS_QUE_SERIAL_NUMBER_PREFIX: i32 = 0x06;
/// `SYNAPTICS_QUE_SERIAL_NUMBER_SUFFIX`.
pub const SYNAPTICS_QUE_SERIAL_NUMBER_SUFFIX: i32 = 0x07;
/// `SYNAPTICS_QUE_RESOLUTION`.
pub const SYNAPTICS_QUE_RESOLUTION: i32 = 0x08;
/// `SYNAPTICS_QUE_EXT_MODEL`.
pub const SYNAPTICS_QUE_EXT_MODEL: i32 = 0x09;
/// `SYNAPTICS_QUE_EXT_CAPABILITIES`.
pub const SYNAPTICS_QUE_EXT_CAPABILITIES: i32 = 0x0c;
/// `SYNAPTICS_QUE_EXT_MAX_COORDS`.
pub const SYNAPTICS_QUE_EXT_MAX_COORDS: i32 = 0x0d;
/// `SYNAPTICS_QUE_EXT_MIN_COORDS`.
pub const SYNAPTICS_QUE_EXT_MIN_COORDS: i32 = 0x0f;
/// `SYNAPTICS_QUE_EXT2_CAPABILITIES`.
pub const SYNAPTICS_QUE_EXT2_CAPABILITIES: i32 = 0x10;

/// `SYNAPTICS_CMD_SET_MODE`.
pub const SYNAPTICS_CMD_SET_MODE: u8 = 0x14;
/// `SYNAPTICS_CMD_SEND_CLIENT`.
pub const SYNAPTICS_CMD_SEND_CLIENT: u8 = 0x28;
/// `SYNAPTICS_CMD_SET_ADV_GESTURE_MODE`.
pub const SYNAPTICS_CMD_SET_ADV_GESTURE_MODE: u8 = 0xc8;

/// `SYNAPTICS_ID_MODEL(id)`.
pub const fn synaptics_id_model(id: i32) -> i32 {
    (id >> 4) & 0x0f
}

/// `SYNAPTICS_ID_MINOR(id)`.
pub const fn synaptics_id_minor(id: i32) -> i32 {
    (id >> 16) & 0xff
}

/// `SYNAPTICS_ID_MAJOR(id)`.
pub const fn synaptics_id_major(id: i32) -> i32 {
    id & 0x0f
}

/// `SYNAPTICS_ID_FULL(id)`: major and minor version, as `0xMMmm`.
pub const fn synaptics_id_full(id: i32) -> i32 {
    (synaptics_id_major(id) << 8) | synaptics_id_minor(id)
}

/// `SYNAPTICS_ID_MAGIC`: the middle byte of the answer to the identify query.
pub const SYNAPTICS_ID_MAGIC: u8 = 0x47;

/// `SYNAPTICS_EXT2_CAP`: modes bit.
pub const SYNAPTICS_EXT2_CAP: i32 = 1 << 17;
/// `SYNAPTICS_ABSOLUTE_MODE`.
pub const SYNAPTICS_ABSOLUTE_MODE: i32 = 1 << 7;
/// `SYNAPTICS_HIGH_RATE`.
pub const SYNAPTICS_HIGH_RATE: i32 = 1 << 6;
/// `SYNAPTICS_SLEEP_MODE`.
pub const SYNAPTICS_SLEEP_MODE: i32 = 1 << 3;
/// `SYNAPTICS_DISABLE_GESTURE`.
pub const SYNAPTICS_DISABLE_GESTURE: i32 = 1 << 2;
/// `SYNAPTICS_FOUR_BYTE_CLIENT`.
pub const SYNAPTICS_FOUR_BYTE_CLIENT: i32 = 1 << 1;
/// `SYNAPTICS_W_MODE`.
pub const SYNAPTICS_W_MODE: i32 = 1 << 0;

/// `SYNAPTICS_CAP_EXTENDED`: capability bit.
pub const SYNAPTICS_CAP_EXTENDED: i32 = 1 << 23;

/// `SYNAPTICS_CAP_EXTENDED_QUERIES(c)`: the number of extended queries.
pub const fn synaptics_cap_extended_queries(c: i32) -> i32 {
    (c >> 20) & 0x07
}

/// `SYNAPTICS_CAP_MIDDLE_BUTTON`.
pub const SYNAPTICS_CAP_MIDDLE_BUTTON: i32 = 1 << 18;
/// `SYNAPTICS_CAP_PASSTHROUGH`.
pub const SYNAPTICS_CAP_PASSTHROUGH: i32 = 1 << 7;
/// `SYNAPTICS_CAP_SLEEP`.
pub const SYNAPTICS_CAP_SLEEP: i32 = 1 << 4;
/// `SYNAPTICS_CAP_FOUR_BUTTON`.
pub const SYNAPTICS_CAP_FOUR_BUTTON: i32 = 1 << 3;
/// `SYNAPTICS_CAP_BALLISTICS`.
pub const SYNAPTICS_CAP_BALLISTICS: i32 = 1 << 2;
/// `SYNAPTICS_CAP_MULTIFINGER`.
pub const SYNAPTICS_CAP_MULTIFINGER: i32 = 1 << 1;
/// `SYNAPTICS_CAP_PALMDETECT`.
pub const SYNAPTICS_CAP_PALMDETECT: i32 = 1 << 0;

/// `SYNAPTICS_MODEL_ROT180`: model ID bit.
pub const SYNAPTICS_MODEL_ROT180: i32 = 1 << 23;
/// `SYNAPTICS_MODEL_PORTRAIT`.
pub const SYNAPTICS_MODEL_PORTRAIT: i32 = 1 << 22;

/// `SYNAPTICS_MODEL_SENSOR(m)`.
pub const fn synaptics_model_sensor(m: i32) -> i32 {
    (m >> 16) & 0x3f
}

/// `SYNAPTICS_MODEL_HARDWARE(m)`.
pub const fn synaptics_model_hardware(m: i32) -> i32 {
    (m >> 9) & 0x7f
}

/// `SYNAPTICS_MODEL_NEWABS`.
pub const SYNAPTICS_MODEL_NEWABS: i32 = 1 << 7;
/// `SYNAPTICS_MODEL_PEN`.
pub const SYNAPTICS_MODEL_PEN: i32 = 1 << 6;
/// `SYNAPTICS_MODEL_SIMPLC`.
pub const SYNAPTICS_MODEL_SIMPLC: i32 = 1 << 5;

/// `SYNAPTICS_MODEL_GEOMETRY(m)`.
pub const fn synaptics_model_geometry(m: i32) -> i32 {
    m & 0x0f
}

/// `SYNAPTICS_RESOLUTION_VALID`: the resolution query's answer is valid.
pub const SYNAPTICS_RESOLUTION_VALID: i32 = 1 << 15;

/// `SYNAPTICS_RESOLUTION_X(r)`.
pub const fn synaptics_resolution_x(r: i32) -> i32 {
    (r >> 16) & 0xff
}

/// `SYNAPTICS_RESOLUTION_Y(r)`.
pub const fn synaptics_resolution_y(r: i32) -> i32 {
    r & 0xff
}

/// `SYNAPTICS_EXT_MODEL_LIGHTCONTROL`: extended model ID bit.
pub const SYNAPTICS_EXT_MODEL_LIGHTCONTROL: i32 = 1 << 22;
/// `SYNAPTICS_EXT_MODEL_PEAKDETECT`.
pub const SYNAPTICS_EXT_MODEL_PEAKDETECT: i32 = 1 << 21;
/// `SYNAPTICS_EXT_MODEL_VWHEEL`.
pub const SYNAPTICS_EXT_MODEL_VWHEEL: i32 = 1 << 19;
/// `SYNAPTICS_EXT_MODEL_EW_MODE`.
pub const SYNAPTICS_EXT_MODEL_EW_MODE: i32 = 1 << 18;
/// `SYNAPTICS_EXT_MODEL_HSCROLL`.
pub const SYNAPTICS_EXT_MODEL_HSCROLL: i32 = 1 << 17;
/// `SYNAPTICS_EXT_MODEL_VSCROLL`.
pub const SYNAPTICS_EXT_MODEL_VSCROLL: i32 = 1 << 16;

/// `SYNAPTICS_EXT_MODEL_BUTTONS(em)`.
pub const fn synaptics_ext_model_buttons(em: i32) -> i32 {
    (em >> 12) & 0x0f
}

/// `SYNAPTICS_EXT_MODEL_SENSOR(em)`.
pub const fn synaptics_ext_model_sensor(em: i32) -> i32 {
    (em >> 10) & 0x03
}

/// `SYNAPTICS_EXT_MODEL_PRODUCT(em)`.
pub const fn synaptics_ext_model_product(em: i32) -> i32 {
    em & 0xff
}

/// `SYNAPTICS_EXT_CAP_CLICKPAD`: extended capability bit.
pub const SYNAPTICS_EXT_CAP_CLICKPAD: i32 = 1 << 20;
/// `SYNAPTICS_EXT_CAP_ADV_GESTURE`.
pub const SYNAPTICS_EXT_CAP_ADV_GESTURE: i32 = 1 << 19;
/// `SYNAPTICS_EXT_CAP_MAX_COORDS`.
pub const SYNAPTICS_EXT_CAP_MAX_COORDS: i32 = 1 << 17;
/// `SYNAPTICS_EXT_CAP_MIN_COORDS`.
pub const SYNAPTICS_EXT_CAP_MIN_COORDS: i32 = 1 << 13;
/// `SYNAPTICS_EXT_CAP_REPORTS_V`.
pub const SYNAPTICS_EXT_CAP_REPORTS_V: i32 = 1 << 11;
/// `SYNAPTICS_EXT_CAP_CLICKPAD_2BTN`.
pub const SYNAPTICS_EXT_CAP_CLICKPAD_2BTN: i32 = 1 << 8;

/// `SYNAPTICS_SUPPORTS_AGM(extcaps)`: the touchpad has the "advanced gesture mode"; the
/// C's nonzero mask.
pub const fn synaptics_supports_agm(extcaps: i32) -> i32 {
    extcaps & (SYNAPTICS_EXT_CAP_ADV_GESTURE | SYNAPTICS_EXT_CAP_REPORTS_V)
}

/// `SYNAPTICS_X_LIMIT(d)`: a coordinate limit's X out of the max or min coords query.
pub const fn synaptics_x_limit(d: i32) -> i32 {
    ((d & 0xff0000) >> 11) | ((d & 0xf00) >> 7)
}

/// `SYNAPTICS_Y_LIMIT(d)`.
pub const fn synaptics_y_limit(d: i32) -> i32 {
    ((d & 0xff) << 5) | ((d & 0xf000) >> 11)
}

/// `SYNAPTICS_EXT2_CAP_BUTTONS_STICK`: extended capability 2 bit.
pub const SYNAPTICS_EXT2_CAP_BUTTONS_STICK: i32 = 1 << 16;

/// `SYNAPTICS_XMIN_BEZEL`: typical bezel limit.
pub const SYNAPTICS_XMIN_BEZEL: i32 = 1472;
/// `SYNAPTICS_XMAX_BEZEL`.
pub const SYNAPTICS_XMAX_BEZEL: i32 = 5472;
/// `SYNAPTICS_YMIN_BEZEL`.
pub const SYNAPTICS_YMIN_BEZEL: i32 = 1408;
/// `SYNAPTICS_YMAX_BEZEL`.
pub const SYNAPTICS_YMAX_BEZEL: i32 = 4448;

/// `ALPS_XMIN_BEZEL`.
pub const ALPS_XMIN_BEZEL: i32 = 0;
/// `ALPS_XMAX_BEZEL`.
pub const ALPS_XMAX_BEZEL: i32 = 1023;
/// `ALPS_YMIN_BEZEL`.
pub const ALPS_YMIN_BEZEL: i32 = 0;
/// `ALPS_YMAX_BEZEL`.
pub const ALPS_YMAX_BEZEL: i32 = 767;

/// `ALPS_XSEC_BEZEL`.
pub const ALPS_XSEC_BEZEL: i32 = 768;
/// `ALPS_YSEC_BEZEL`.
pub const ALPS_YSEC_BEZEL: i32 = 512;

/// `ALPS_Z_MAGIC`.
pub const ALPS_Z_MAGIC: i32 = 127;

/// `ALPS_TAP`: ALPS "gesture" and "finger" bits.
pub const ALPS_TAP: u32 = 0x01;
/// `ALPS_DRAG`.
pub const ALPS_DRAG: u32 = 0x03;

/// `ELANTECH_QUE_FW_ID`: Elantech query.
pub const ELANTECH_QUE_FW_ID: i32 = 0;
/// `ELANTECH_QUE_FW_VER`.
pub const ELANTECH_QUE_FW_VER: i32 = 1;
/// `ELANTECH_QUE_CAPABILITIES`.
pub const ELANTECH_QUE_CAPABILITIES: i32 = 2;
/// `ELANTECH_QUE_SAMPLE`.
pub const ELANTECH_QUE_SAMPLE: i32 = 3;
/// `ELANTECH_QUE_RESOLUTION`.
pub const ELANTECH_QUE_RESOLUTION: i32 = 4;

/// `ELANTECH_CAP_HAS_ROCKER`: Elantech capability.
pub const ELANTECH_CAP_HAS_ROCKER: u8 = 4;
/// `ELANTECH_CAP_TRACKPOINT`.
pub const ELANTECH_CAP_TRACKPOINT: u8 = 0x80;

/// `ELANTECH_PS2_CUSTOM_COMMAND`.
pub const ELANTECH_PS2_CUSTOM_COMMAND: u8 = 0xf8;

/// `ELANTECH_CMD_READ_REG`.
pub const ELANTECH_CMD_READ_REG: u8 = 0x10;
/// `ELANTECH_CMD_WRITE_REG`.
pub const ELANTECH_CMD_WRITE_REG: u8 = 0x11;
/// `ELANTECH_CMD_READ_WRITE_REG`.
pub const ELANTECH_CMD_READ_WRITE_REG: u8 = 0x00;

/// `ELANTECH_ABSOLUTE_MODE`.
pub const ELANTECH_ABSOLUTE_MODE: u8 = 0x04;

/// `ELANTECH_V1_EDGE_OFFSET`: hardware version 1 has hard-coded axis range values. X axis
/// range is 0 to 576, Y axis range is 0 to 384. Edge offset accounts for bezel around the
/// touchpad.
pub const ELANTECH_V1_EDGE_OFFSET: i32 = 32;
/// `ELANTECH_V1_X_MIN`.
pub const ELANTECH_V1_X_MIN: i32 = ELANTECH_V1_EDGE_OFFSET; // 0 + the offset
/// `ELANTECH_V1_X_MAX`.
pub const ELANTECH_V1_X_MAX: i32 = 576 - ELANTECH_V1_EDGE_OFFSET;
/// `ELANTECH_V1_Y_MIN`.
pub const ELANTECH_V1_Y_MIN: i32 = ELANTECH_V1_EDGE_OFFSET; // 0 + the offset
/// `ELANTECH_V1_Y_MAX`.
pub const ELANTECH_V1_Y_MAX: i32 = 384 - ELANTECH_V1_EDGE_OFFSET;

/// `ELANTECH_V2_X_MAX`: older hardware version 2 variants lack ID query capability.
pub const ELANTECH_V2_X_MAX: i32 = 1152;
/// `ELANTECH_V2_Y_MAX`.
pub const ELANTECH_V2_Y_MAX: i32 = 768;

/// `ELANTECH_MAX_FINGERS`: V4.
pub const ELANTECH_MAX_FINGERS: i32 = 5;
/// `ELANTECH_V4_WEIGHT_VALUE`.
pub const ELANTECH_V4_WEIGHT_VALUE: i32 = 5;

/// `ELANTECH_V4_PKT_STATUS`.
pub const ELANTECH_V4_PKT_STATUS: i32 = 0;
/// `ELANTECH_V4_PKT_HEAD`.
pub const ELANTECH_V4_PKT_HEAD: i32 = 0x01;
/// `ELANTECH_V4_PKT_MOTION`.
pub const ELANTECH_V4_PKT_MOTION: i32 = 0x02;

/// `ELANTECH_PKT_TRACKPOINT`: V3 and V4 may be coupled with trackpoints, pms supports them
/// for V4.
pub const ELANTECH_PKT_TRACKPOINT: i32 = 0x06;
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_macros() {
        // A Synaptics identify answer: minor 0x0b, model 0x1, major 8.
        let id = 0x0b_47_18;
        assert_eq!(synaptics_id_major(id), 8);
        assert_eq!(synaptics_id_minor(id), 0x0b);
        assert_eq!(synaptics_id_model(id), 1);
        assert_eq!(synaptics_id_full(id), 0x80b);
        assert_eq!(synaptics_cap_extended_queries(0x00d0_0000), 5);
        assert_eq!(synaptics_model_sensor(0x3f_0000), 0x3f);
        assert_eq!(synaptics_model_hardware(0x7f << 9), 0x7f);
        assert_eq!(synaptics_model_geometry(0x1f), 0x0f);
        assert_eq!(synaptics_resolution_x(0x2f_80_1c), 0x2f);
        assert_eq!(synaptics_resolution_y(0x2f_80_1c), 0x1c);
        assert_eq!(synaptics_ext_model_buttons(0x3000), 3);
        assert_eq!(synaptics_ext_model_sensor(0xc00), 3);
        assert_eq!(synaptics_ext_model_product(0x1234), 0x34);
        assert_eq!(synaptics_supports_agm(SYNAPTICS_EXT_CAP_CLICKPAD), 0);
        assert_ne!(synaptics_supports_agm(SYNAPTICS_EXT_CAP_REPORTS_V), 0);
        // Max coords 0xab_c_d_ef: X is 0xab << 5 | 0xd << 1, Y is 0xef << 5 | 0xc << 1.
        assert_eq!(synaptics_x_limit(0xab_cd_ef), (0xab << 5) | (0xd << 1));
        assert_eq!(synaptics_y_limit(0xab_cd_ef), (0xef << 5) | (0xc << 1));
        assert_eq!(ELANTECH_V1_X_MAX - ELANTECH_V1_X_MIN, 512);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/pckbc/pmsreg.h");
        let ours = crate::reftest::assert_defines!(defs;
            PMS_SET_SCALE11, PMS_SET_SCALE21, PMS_SET_RES, PMS_SEND_DEV_STATUS,
            PMS_SET_STREAM_MODE, PMS_SEND_DEV_DATA, PMS_RESET_WRAP_MODE, PMS_SET_WRAP_MODE,
            PMS_SET_REMOTE_MODE, PMS_SEND_DEV_ID, PMS_SET_SAMPLE, PMS_DEV_ENABLE,
            PMS_DEV_DISABLE, PMS_SET_DEFAULTS, PMS_RESEND, PMS_RESET, PMS_RSTDONE,
            PMS_PS2_BUTTONSMASK, PMS_PS2_BUTTON1, PMS_PS2_BUTTON2, PMS_PS2_BUTTON3,
            PMS_PS2_XNEG, PMS_PS2_YNEG, PMS_INTELLI_MAGIC1, PMS_INTELLI_MAGIC2,
            PMS_INTELLI_MAGIC3, PMS_INTELLI_ID, PMS_ALPS_MAGIC1, PMS_ALPS_MAGIC2,
            PMS_ALPS_MAGIC3_1, PMS_ALPS_MAGIC3_2, PMS_ALPS_MAGIC3_3, PMS_ELANTECH_MAGIC1,
            PMS_ELANTECH_MAGIC2, PMS_ELANTECH_MAGIC3_1, PMS_ELANTECH_MAGIC3_2,
            PMS_ALPS_PS2_MASK, PMS_ALPS_PS2_VALID, PMS_ALPS_INTERLEAVED_MASK,
            PMS_ALPS_INTERLEAVED_VALID, PMS_ALPS_MASK, PMS_ALPS_VALID,
            SYNAPTICS_QUE_IDENTIFY, SYNAPTICS_QUE_MODES, SYNAPTICS_QUE_CAPABILITIES,
            SYNAPTICS_QUE_MODEL, SYNAPTICS_QUE_SERIAL_NUMBER_PREFIX,
            SYNAPTICS_QUE_SERIAL_NUMBER_SUFFIX, SYNAPTICS_QUE_RESOLUTION,
            SYNAPTICS_QUE_EXT_MODEL, SYNAPTICS_QUE_EXT_CAPABILITIES,
            SYNAPTICS_QUE_EXT_MAX_COORDS, SYNAPTICS_QUE_EXT_MIN_COORDS,
            SYNAPTICS_QUE_EXT2_CAPABILITIES, SYNAPTICS_CMD_SET_MODE,
            SYNAPTICS_CMD_SEND_CLIENT, SYNAPTICS_CMD_SET_ADV_GESTURE_MODE,
            SYNAPTICS_ID_MAGIC, SYNAPTICS_EXT2_CAP, SYNAPTICS_ABSOLUTE_MODE,
            SYNAPTICS_HIGH_RATE, SYNAPTICS_SLEEP_MODE, SYNAPTICS_DISABLE_GESTURE,
            SYNAPTICS_FOUR_BYTE_CLIENT, SYNAPTICS_W_MODE, SYNAPTICS_CAP_EXTENDED,
            SYNAPTICS_CAP_MIDDLE_BUTTON, SYNAPTICS_CAP_PASSTHROUGH, SYNAPTICS_CAP_SLEEP,
            SYNAPTICS_CAP_FOUR_BUTTON, SYNAPTICS_CAP_BALLISTICS, SYNAPTICS_CAP_MULTIFINGER,
            SYNAPTICS_CAP_PALMDETECT, SYNAPTICS_MODEL_ROT180, SYNAPTICS_MODEL_PORTRAIT,
            SYNAPTICS_MODEL_NEWABS, SYNAPTICS_MODEL_PEN, SYNAPTICS_MODEL_SIMPLC,
            SYNAPTICS_RESOLUTION_VALID, SYNAPTICS_EXT_MODEL_LIGHTCONTROL,
            SYNAPTICS_EXT_MODEL_PEAKDETECT, SYNAPTICS_EXT_MODEL_VWHEEL,
            SYNAPTICS_EXT_MODEL_EW_MODE, SYNAPTICS_EXT_MODEL_HSCROLL,
            SYNAPTICS_EXT_MODEL_VSCROLL, SYNAPTICS_EXT_CAP_CLICKPAD,
            SYNAPTICS_EXT_CAP_ADV_GESTURE, SYNAPTICS_EXT_CAP_MAX_COORDS,
            SYNAPTICS_EXT_CAP_MIN_COORDS, SYNAPTICS_EXT_CAP_REPORTS_V,
            SYNAPTICS_EXT_CAP_CLICKPAD_2BTN, SYNAPTICS_EXT2_CAP_BUTTONS_STICK,
            SYNAPTICS_XMIN_BEZEL, SYNAPTICS_XMAX_BEZEL, SYNAPTICS_YMIN_BEZEL,
            SYNAPTICS_YMAX_BEZEL, ALPS_XMIN_BEZEL, ALPS_XMAX_BEZEL, ALPS_YMIN_BEZEL,
            ALPS_YMAX_BEZEL, ALPS_XSEC_BEZEL, ALPS_YSEC_BEZEL, ALPS_Z_MAGIC, ALPS_TAP,
            ALPS_DRAG, ELANTECH_QUE_FW_ID, ELANTECH_QUE_FW_VER, ELANTECH_QUE_CAPABILITIES,
            ELANTECH_QUE_SAMPLE, ELANTECH_QUE_RESOLUTION, ELANTECH_CAP_HAS_ROCKER,
            ELANTECH_CAP_TRACKPOINT, ELANTECH_PS2_CUSTOM_COMMAND, ELANTECH_CMD_READ_REG,
            ELANTECH_CMD_WRITE_REG, ELANTECH_CMD_READ_WRITE_REG, ELANTECH_ABSOLUTE_MODE,
            ELANTECH_V1_EDGE_OFFSET, ELANTECH_V1_X_MIN, ELANTECH_V1_X_MAX,
            ELANTECH_V1_Y_MIN, ELANTECH_V1_Y_MAX, ELANTECH_V2_X_MAX, ELANTECH_V2_Y_MAX,
            ELANTECH_MAX_FINGERS, ELANTECH_V4_WEIGHT_VALUE, ELANTECH_V4_PKT_STATUS,
            ELANTECH_V4_PKT_HEAD, ELANTECH_V4_PKT_MOTION, ELANTECH_PKT_TRACKPOINT,
        );
        for prefix in ["PMS_", "SYNAPTICS_", "ALPS_", "ELANTECH_"] {
            crate::reftest::assert_complete(&defs, prefix, &ours);
        }
    }
}
/* </TESTS> */
