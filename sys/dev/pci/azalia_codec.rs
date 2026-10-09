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

/* <CODE> */
//! The generic codec support of azalia(4): codec names and quirks, converter groups, path
//! finding between widgets, unsolicited events, and the generic mixer (the controls of a
//! codec's amplifiers, selectors, pins and volume groups).
//!
//! Upstream: sys/dev/pci/azalia_codec.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The functions take the codec as `&Codec` or `&mut Codec`, and widgets by their nid;
//!   `azalia_pin_config_ov` and `azalia_ampcap_ov` take the widget.
//! - [`azalia_widget_enabled`] returns `bool`.
//! - The C's `int` results that are always 0 are `Result<(), Errno>` like the others; the C's
//!   `-1` ("internal error", an unknown target) is `EIO`.
//! - The mixer list is a `Vec<MixerItem>`: `nmixers` is its length and `maxmixers` its
//!   capacity ([`azalia_mixer_ensure_capacity`] reserves ten more slots, as the C reallocates).
//!   `MIXER_REG_PROLOG` builds the control in a local item that [`azalia_mixer_init`] adds
//!   when it is complete; a control with no member is dropped before it is added (the C leaves
//!   the slot to be overwritten).
//! - `azalia_mixer_init` is split: [`azalia_mixer_register`] builds the controls, and the
//!   rest (`azalia_mixer_fix_indexes`, `azalia_mixer_default`) follows, so the host tests can
//!   run the first part without a controller. When growing the list fails the C ignores the
//!   error in the volume-group blocks and writes past the end; here the error is returned.
//! - The `DIAGNOSTIC` "index mismatch" message of `azalia_mixer_fix_indexes` is under the
//!   `diagnostic` cargo feature, as option `DIAGNOSTIC`. `DPRINTF` messages are not carried.
//! - Indexes into a widget's connection list that are out of range count as disabled
//!   connections (the C reads past the array).
//! - `azalia_mixer_set` for `MI_TARGET_SPDIF` reads `0` where the C reuses an unset
//!   `result` when the first verb fails.
//! - The pairs of verbs that set one channel's gain or mute are written once
//!   (`amp_set_gain`, `amp_set_mute`) and run for the left and the right channel in the C's
//!   order; likewise the play and the record volume group (`push_volgroup`, `volgroup_get`).

use alloc::vec::Vec;

use libkern::strlcpy;

use crate::dev::pci::azalia::{
    AZ_CLASS_INPUT, AZ_CLASS_OUTPUT, AZ_CLASS_RECORD, AZ_QRK_DOLBY_ATMOS, AZ_QRK_GPIO_POL_0,
    AZ_QRK_GPIO_UNMUTE_0, AZ_QRK_GPIO_UNMUTE_1, AZ_QRK_GPIO_UNMUTE_2, AZ_QRK_GPIO_UNMUTE_3,
    AZ_QRK_NONE, AZ_QRK_ROUTE_SPKR2_DAC, AZ_QRK_WID_AD1981_OAMP, AZ_QRK_WID_BEEP_1D,
    AZ_QRK_WID_CDIN_1C, AZ_QRK_WID_CLOSE_PCBEEP, AZ_QRK_WID_OVREF50, AZ_QRK_WID_TPDOCK1,
    AZ_QRK_WID_TPDOCK2, AZ_QRK_WID_TPDOCK3, AZ_SPKR_MUTE_DAC_MUTE, AZ_SPKR_MUTE_NONE,
    AZ_SPKR_MUTE_SPKR_DIR, AZ_SPKR_MUTE_SPKR_MUTE, AZ_TAG_PLAYVOL, AZ_TAG_SPKR, COP_AMPCAP_MUTE,
    COP_AWCAP_DIGITAL, COP_AWCAP_INAMP, COP_AWCAP_OUTAMP, COP_AWCAP_STEREO, COP_AWCAP_UNSOL,
    COP_AWTYPE_AUDIO_INPUT, COP_AWTYPE_AUDIO_MIXER, COP_AWTYPE_AUDIO_OUTPUT,
    COP_AWTYPE_AUDIO_SELECTOR, COP_AWTYPE_BEEP_GENERATOR, COP_AWTYPE_PIN_COMPLEX, COP_INPUT_AMPCAP,
    COP_OUTPUT_AMPCAP, COP_PINCAP_EAPD, COP_PINCAP_HEADPHONE, COP_PINCAP_INPUT, COP_PINCAP_OUTPUT,
    CORB_AGM_GAIN_MASK, CORB_AGM_INDEX_SHIFT, CORB_AGM_INPUT, CORB_AGM_LEFT, CORB_AGM_MUTE,
    CORB_AGM_OUTPUT, CORB_AGM_RIGHT, CORB_CD_BEEP, CORB_CD_CD, CORB_CD_DEVICE_BITS,
    CORB_CD_DEVICE_MASK, CORB_CD_DEVICE_OFFSET, CORB_CD_FIXED, CORB_CD_PORT_BITS,
    CORB_CD_PORT_MASK, CORB_CD_PORT_OFFSET, CORB_DCC_DIGEN, CORB_DCC_NAUDIO, CORB_EAPD_EAPD,
    CORB_GAGM_INPUT, CORB_GAGM_LEFT, CORB_GAGM_MUTE, CORB_GAGM_OUTPUT, CORB_GAGM_RIGHT,
    CORB_GET_AMPLIFIER_GAIN_MUTE, CORB_GET_CONNECTION_SELECT_CONTROL, CORB_GET_DIGITAL_CONTROL,
    CORB_GET_EAPD_BTL_ENABLE, CORB_GET_GPIO_DATA, CORB_GET_GPIO_DIRECTION,
    CORB_GET_GPIO_ENABLE_MASK, CORB_GET_PIN_SENSE, CORB_GET_PIN_WIDGET_CONTROL,
    CORB_GET_VOLUME_KNOB, CORB_PS_PRESENCE, CORB_PWC_HEADPHONE, CORB_PWC_INPUT, CORB_PWC_OUTPUT,
    CORB_PWC_VREF_50, CORB_PWC_VREF_80, CORB_PWC_VREF_100, CORB_PWC_VREF_GND, CORB_PWC_VREF_MASK,
    CORB_SET_AMPLIFIER_GAIN_MUTE, CORB_SET_COEFFICIENT_INDEX, CORB_SET_CONNECTION_SELECT_CONTROL,
    CORB_SET_DIGITAL_CONTROL_H, CORB_SET_DIGITAL_CONTROL_L, CORB_SET_EAPD_BTL_ENABLE,
    CORB_SET_GPIO_DATA, CORB_SET_GPIO_DIRECTION, CORB_SET_GPIO_ENABLE_MASK, CORB_SET_GPIO_POLARITY,
    CORB_SET_PIN_WIDGET_CONTROL, CORB_SET_PROCESSING_COEFFICIENT, CORB_SET_UNSOLICITED_RESPONSE,
    CORB_SET_VOLUME_KNOB, CORB_UNSOL_ENABLE, CORB_VKNOB_DIRECT, Codec, Convgroupset,
    HDA_MAX_CHANNELS, IoPin, MI_TARGET_ADC, MI_TARGET_CONNLIST, MI_TARGET_DAC, MI_TARGET_EAPD,
    MI_TARGET_MIXERSET, MI_TARGET_MUTESET, MI_TARGET_OUTAMP, MI_TARGET_PINBOOST, MI_TARGET_PINDIR,
    MI_TARGET_PINSENSE, MI_TARGET_PLAYVOL, MI_TARGET_RECVOL, MI_TARGET_SENSESET, MI_TARGET_SPDIF,
    MI_TARGET_SPDIF_CC, MixerItem, NidT, Volgroup, Widget, azalia_codec_construct_format,
    azalia_comresp, cop_ampcap_ctloff, cop_ampcap_numsteps, cop_pincap_vref, cop_vkcap_numsteps,
    corb_csc_index, corb_dcc_cc, corb_gagm_gain, corb_unsol_tag, corb_vknob_volume,
    is_mi_target_inamp, mi_target_inamp, valid_widget_nid, widget_channels,
};
use crate::dev::pci::pcidevs::{PCI_VENDOR_DELL, PCI_VENDOR_HP};
use crate::dev::pci::pcireg::pci_vendor;
use crate::kern::subr_prf::{Str, printf, snprintf};
use crate::machine::cpu::delay;
use crate::sys::audioio::{
    AUDIO_MAX_GAIN, AUDIO_MIN_GAIN, AUDIO_MIXER_CLASS, AUDIO_MIXER_ENUM, AUDIO_MIXER_LAST,
    AUDIO_MIXER_SET, AUDIO_MIXER_VALUE, AudioCinputs, AudioCoutputs, AudioCrecord, AudioNinput,
    AudioNmaster, AudioNmode, AudioNmute, AudioNoff, AudioNon, AudioNoutput, AudioNvolume,
    MixerCtrl, MixerDevinfo,
};
use crate::sys::errno::Errno;

/// `azalia_codec_init_vtbl`: the codec's name and quirks, from its vendor/device ID and
/// the PCI subsystem ID.
pub fn azalia_codec_init_vtbl(this: &mut Codec) -> Result<(), Errno> {
    // We can refer this->vid and this->subid.
    this.name = None;
    this.qrks = AZ_QRK_NONE;
    let subid = this.subid;
    let subvendor = pci_vendor(subid);
    let (name, qrks): (&'static str, i32) = match this.vid {
        0x10134206 => (
            "Cirrus Logic CS4206",
            if matches!(
                subid,
                0xcb8910de // APPLE_MBA3_1
                    | 0x72708086 // APPLE_MBA4_1
                    | 0xcb7910de // APPLE_MBP5_5
            ) {
                AZ_QRK_GPIO_UNMUTE_1 | AZ_QRK_GPIO_UNMUTE_3
            } else {
                0
            },
        ),
        0x10134208 => (
            "Cirrus Logic CS4208",
            if subid == 0x72708086 {
                // APPLE_MBA6_1
                AZ_QRK_GPIO_UNMUTE_0 | AZ_QRK_GPIO_UNMUTE_1
            } else {
                0
            },
        ),
        0x10ec0221 => ("Realtek ALC221", AZ_QRK_WID_CDIN_1C | AZ_QRK_WID_BEEP_1D),
        0x10ec0225 => ("Realtek ALC225", 0),
        0x10ec0233 | 0x10ec0235 => ("Realtek ALC233", 0),
        0x10ec0236 => (
            if subvendor == PCI_VENDOR_DELL {
                "Realtek ALC3204"
            } else {
                "Realtek ALC236"
            },
            0,
        ),
        0x10ec0245 => ("Realtek ALC245", 0),
        0x10ec0255 => ("Realtek ALC255", 0),
        0x10ec0256 => ("Realtek ALC256", 0),
        0x10ec0257 => ("Realtek ALC257", 0),
        0x10ec0260 => (
            "Realtek ALC260",
            if subid == 0x008f1025 {
                AZ_QRK_GPIO_UNMUTE_0
            } else {
                0
            },
        ),
        0x10ec0262 => ("Realtek ALC262", AZ_QRK_WID_CDIN_1C | AZ_QRK_WID_BEEP_1D),
        0x10ec0268 => ("Realtek ALC268", AZ_QRK_WID_CDIN_1C | AZ_QRK_WID_BEEP_1D),
        0x10ec0269 => (
            "Realtek ALC269",
            AZ_QRK_WID_CDIN_1C
                | AZ_QRK_WID_BEEP_1D
                // Enable dock audio on Thinkpad docks
                // 0x17aa : 0x21f3 = Thinkpad T430
                // 0x17aa : 0x21f6 = Thinkpad T530
                // 0x17aa : 0x21fa = Thinkpad X230
                // 0x17aa : 0x21fb = Thinkpad T430s
                // 0x17aa : 0x2203 = Thinkpad X230t
                // 0x17aa : 0x2208 = Thinkpad T431s
                | if matches!(
                    subid,
                    0x21f317aa | 0x21f617aa | 0x21fa17aa | 0x21fb17aa | 0x220317aa | 0x220817aa
                ) {
                    AZ_QRK_WID_TPDOCK1
                } else {
                    0
                },
        ),
        0x10ec0270 => ("Realtek ALC270", 0),
        0x10ec0272 => ("Realtek ALC272", 0),
        0x10ec0275 => ("Realtek ALC275", 0),
        0x10ec0280 => ("Realtek ALC280", 0),
        0x10ec0282 => ("Realtek ALC282", AZ_QRK_WID_CDIN_1C | AZ_QRK_WID_BEEP_1D),
        0x10ec0283 => ("Realtek ALC283", 0),
        0x10ec0285 => (
            "Realtek ALC285",
            if subid == 0x229217aa {
                // Thinkpad X1 Carbon 7
                AZ_QRK_ROUTE_SPKR2_DAC | AZ_QRK_WID_CLOSE_PCBEEP
            } else if subid == 0x22c017aa {
                // Thinkpad X1 Extreme 3
                AZ_QRK_DOLBY_ATMOS | AZ_QRK_ROUTE_SPKR2_DAC
            } else {
                0
            },
        ),
        0x10ec0287 => ("Realtek ALC287", 0),
        0x10ec0292 => (
            "Realtek ALC292",
            AZ_QRK_WID_CDIN_1C
                | AZ_QRK_WID_BEEP_1D
                // Enable dock audio on Thinkpad docks
                // 0x17aa : 0x220c = Thinkpad T440s
                // 0x17aa : 0x220e = Thinkpad T440p
                // 0x17aa : 0x2210 = Thinkpad T540p
                // 0x17aa : 0x2212 = Thinkpad T440
                // 0x17aa : 0x2214 = Thinkpad X240
                // 0x17aa : 0x2226 = Thinkpad X250
                // 0x17aa : 0x501e = Thinkpad L440
                // 0x17aa : 0x5034 = Thinkpad T450
                // 0x17aa : 0x5036 = Thinkpad T450s
                // 0x17aa : 0x503c = Thinkpad L450
                | if matches!(
                    subid,
                    0x220c17aa
                        | 0x220e17aa
                        | 0x221017aa
                        | 0x221217aa
                        | 0x221417aa
                        | 0x222617aa
                        | 0x501e17aa
                        | 0x503417aa
                        | 0x503617aa
                        | 0x503c17aa
                ) {
                    AZ_QRK_WID_TPDOCK2
                } else {
                    0
                },
        ),
        0x10ec0293 => (
            if subvendor == PCI_VENDOR_DELL {
                "Realtek ALC3235"
            } else {
                "Realtek ALC293"
            },
            0,
        ),
        0x10ec0294 => ("Realtek ALC294", 0),
        0x10ec0295 => (
            if subvendor == PCI_VENDOR_DELL {
                "Realtek ALC3254"
            } else {
                "Realtek ALC295"
            },
            0,
        ),
        0x10ec0298 => (
            "Realtek ALC298",
            if matches!(subid, 0x320019e5 | 0x320119e5) {
                // Huawei Matebook X
                AZ_QRK_DOLBY_ATMOS
            } else {
                0
            },
        ),
        0x10ec0299 => ("Realtek ALC299", 0),
        0x10ec0660 => (
            "Realtek ALC660",
            if subid == 0x13391043 {
                // ASUS_G2K
                AZ_QRK_GPIO_UNMUTE_0
            } else {
                0
            },
        ),
        0x10ec0662 => ("Realtek ALC662", AZ_QRK_WID_CDIN_1C | AZ_QRK_WID_BEEP_1D),
        0x10ec0663 => ("Realtek ALC663", 0),
        0x10ec0668 => (
            if subvendor == PCI_VENDOR_DELL {
                "Realtek ALC3661"
            } else {
                "Realtek ALC668"
            },
            0,
        ),
        0x10ec0671 => ("Realtek ALC671", 0),
        0x10ec0700 => ("Realtek ALC700", 0),
        0x10ec0861 => ("Realtek ALC861", 0),
        0x10ec0880 => {
            let mut q = AZ_QRK_WID_CDIN_1C | AZ_QRK_WID_BEEP_1D;
            if matches!(
                subid,
                0x19931043 /* ASUS_M5200 */ | 0x13231043 /* ASUS_A7M */
            ) {
                q |= AZ_QRK_GPIO_UNMUTE_0;
            }
            if subid == 0x203d161f {
                // MEDION_MD95257
                q |= AZ_QRK_GPIO_UNMUTE_1;
            }
            ("Realtek ALC880", q)
        }
        0x10ec0882 => {
            let mut q = AZ_QRK_WID_CDIN_1C | AZ_QRK_WID_BEEP_1D;
            if matches!(
                subid,
                0x13c21043 /* ASUS_A7T */ | 0x19711043 /* ASUS_W2J */
            ) {
                q |= AZ_QRK_GPIO_UNMUTE_0;
            }
            ("Realtek ALC882", q)
        }
        0x10ec0883 => {
            let mut q = AZ_QRK_WID_CDIN_1C | AZ_QRK_WID_BEEP_1D;
            if subid == 0x00981025 {
                // ACER_ID
                q |= AZ_QRK_GPIO_UNMUTE_0 | AZ_QRK_GPIO_UNMUTE_1;
            }
            ("Realtek ALC883", q)
        }
        0x10ec0885 => {
            let mut q = AZ_QRK_WID_CDIN_1C | AZ_QRK_WID_BEEP_1D;
            if matches!(
                subid,
                0x00a1106b // APPLE_MB3
                    | 0xcb7910de // APPLE_MACMINI3_1 (line-in + hp)
                    | 0x00a0106b // APPLE_MB3_1
                    | 0x00a3106b // APPLE_MB4
            ) {
                q |= AZ_QRK_GPIO_UNMUTE_0;
            }
            if matches!(
                subid,
                0x00a1106b | 0xcb7910de /* APPLE_MACMINI3_1 (internal spkr) */ | 0x00a0106b
            ) {
                q |= AZ_QRK_WID_OVREF50;
            }
            ("Realtek ALC885", q)
        }
        0x10ec0887 => ("Realtek ALC887", 0),
        0x10ec0888 => ("Realtek ALC888", AZ_QRK_WID_CDIN_1C | AZ_QRK_WID_BEEP_1D),
        0x10ec0889 => ("Realtek ALC889", 0),
        0x10ec0892 => ("Realtek ALC892", 0),
        0x10ec0897 => ("Realtek ALC897", 0),
        0x10ec0900 => ("Realtek ALC1150", 0),
        0x10ec0b00 => ("Realtek ALC1200", 0),
        0x10ec1168 | 0x10ec1220 => ("Realtek ALC1220", 0),
        0x11060398 | 0x11061398 | 0x11062398 | 0x11063398 | 0x11064398 | 0x11065398
        | 0x11066398 | 0x11067398 => ("VIA VT1702", 0),
        0x111d7603 => (
            "IDT 92HD75B3/4",
            if subvendor == PCI_VENDOR_HP {
                AZ_QRK_GPIO_UNMUTE_0
            } else {
                0
            },
        ),
        0x111d7604 => ("IDT 92HD83C1X", 0),
        0x111d7605 => ("IDT 92HD81B1X", 0),
        0x111d7608 => (
            "IDT 92HD75B1/2",
            if subvendor == PCI_VENDOR_HP {
                AZ_QRK_GPIO_UNMUTE_0
            } else {
                0
            },
        ),
        0x111d7674 => ("IDT 92HD73D1", 0),
        0x111d7675 => (
            "IDT 92HD73C1", // aka 92HDW74C1
            if subvendor == PCI_VENDOR_DELL {
                AZ_QRK_GPIO_UNMUTE_0
            } else {
                0
            },
        ),
        0x111d7676 => ("IDT 92HD73E1", 0), // aka 92HDW74E1
        0x111d7695 => ("IDT 92HD95", 0),   // aka IDT/TSI 92HD95B
        0x111d76b0 => ("IDT 92HD71B8", 0),
        0x111d76b2 => (
            "IDT 92HD71B7",
            if subvendor == PCI_VENDOR_DELL || subvendor == PCI_VENDOR_HP {
                AZ_QRK_GPIO_UNMUTE_0
            } else {
                0
            },
        ),
        0x111d76b6 => ("IDT 92HD71B5", 0),
        0x111d76d4 => ("IDT 92HD83C1C", 0),
        0x111d76d5 => ("IDT 92HD81B1C", 0),
        0x11d4184a => ("Analog Devices AD1884A", 0),
        0x11d41882 => ("Analog Devices AD1882", 0),
        0x11d41883 => ("Analog Devices AD1883", 0),
        0x11d41884 => ("Analog Devices AD1884", 0),
        0x11d4194a => ("Analog Devices AD1984A", 0),
        0x11d41981 => ("Analog Devices AD1981HD", AZ_QRK_WID_AD1981_OAMP),
        0x11d41983 => ("Analog Devices AD1983", 0),
        0x11d41984 => ("Analog Devices AD1984", 0),
        0x11d41988 => ("Analog Devices AD1988A", 0),
        0x11d4198b => ("Analog Devices AD1988B", 0),
        0x11d4882a => ("Analog Devices AD1882A", 0),
        0x11d4989a => ("Analog Devices AD1989A", 0),
        0x11d4989b => ("Analog Devices AD1989B", 0),
        0x14f15045 => ("Conexant CX20549", 0), // Venice
        0x14f15047 => ("Conexant CX20551", 0), // Waikiki
        0x14f15051 => ("Conexant CX20561", 0), // Hermosa
        0x14f1506e => (
            "Conexant CX20590",
            // Enable dock audio on Thinkpad docks
            // 0x17aa : 0x20f2 = Thinkpad T400
            // 0x17aa : 0x215e = Thinkpad T410
            // 0x17aa : 0x215f = Thinkpad T510
            // 0x17aa : 0x21ce = Thinkpad T420
            // 0x17aa : 0x21cf = Thinkpad T520
            // 0x17aa : 0x21da = Thinkpad X220
            // 0x17aa : 0x21db = Thinkpad X220t
            if matches!(
                subid,
                0x20f217aa
                    | 0x215e17aa
                    | 0x215f17aa
                    | 0x21ce17aa
                    | 0x21cf17aa
                    | 0x21da17aa
                    | 0x21db17aa
            ) {
                AZ_QRK_WID_TPDOCK3
            } else {
                0
            },
        ),
        0x434d4980 => ("CMedia CMI9880", 0),
        0x83847612 => ("Sigmatel STAC9230X", 0),
        0x83847613 => ("Sigmatel STAC9230D", 0),
        0x83847614 => ("Sigmatel STAC9229X", 0),
        0x83847615 => ("Sigmatel STAC9229D", 0),
        0x83847616 => (
            "Sigmatel STAC9228X",
            if matches!(
                subid,
                0x02271028 /* DELL_V1400 */ | 0x01f31028 /* DELL_I1400 */
            ) {
                AZ_QRK_GPIO_UNMUTE_2
            } else {
                0
            },
        ),
        0x83847617 => ("Sigmatel STAC9228D", 0),
        0x83847618 => ("Sigmatel STAC9227X", 0),
        0x83847619 => ("Sigmatel STAC9227D", 0),
        0x83847620 => ("Sigmatel STAC9274", 0),
        0x83847621 => ("Sigmatel STAC9274D", 0),
        0x83847626 => ("Sigmatel STAC9271X", 0),
        0x83847627 => ("Sigmatel STAC9271D", 0),
        0x83847632 => ("Sigmatel STAC9202", 0),
        0x83847634 => ("Sigmatel STAC9250", 0),
        0x83847636 => ("Sigmatel STAC9251", 0),
        0x83847638 => ("IDT 92HD700X", 0),
        0x83847639 => ("IDT 92HD700D", 0),
        0x83847645 => ("IDT 92HD206X", 0),
        0x83847646 => ("IDT 92HD206D", 0),
        0x83847661 | 0x83847662 => ("Sigmatel STAC9225", 0),
        0x83847680 => (
            "Sigmatel STAC9220/1",
            if subid == 0x76808384 {
                // APPLE_ID
                AZ_QRK_GPIO_POL_0 | AZ_QRK_GPIO_UNMUTE_0 | AZ_QRK_GPIO_UNMUTE_1
            } else {
                0
            },
        ),
        0x83847682 | 0x83847683 => ("Sigmatel STAC9221D", 0), // aka IDT 92HD202
        0x83847690 => ("Sigmatel STAC9200", 0),               // aka IDT 92HD001
        0x83847691 => ("Sigmatel STAC9200D", 0),
        0x83847698 => ("IDT 92HD005", 0),
        0x83847699 => ("IDT 92HD005D", 0),
        0x838476a0 => (
            "Sigmatel STAC9205X",
            if matches!(
                subid,
                0x01f91028 /* DELL_D630 */ | 0x02281028 /* DELL_V1500 */
            ) {
                AZ_QRK_GPIO_UNMUTE_0
            } else {
                0
            },
        ),
        0x838476a1 => ("Sigmatel STAC9205D", 0),
        0x838476a2 => ("Sigmatel STAC9204X", 0),
        0x838476a3 => ("Sigmatel STAC9204D", 0),
        _ => return Ok(()),
    };
    this.name = Some(name);
    this.qrks |= qrks;
    Ok(())
}

// ----------------------------------------------------------------
// functions for generic codecs
// ----------------------------------------------------------------

/// `azalia_widget_enabled`: `nid` is a widget of the audio function and is enabled.
pub fn azalia_widget_enabled(this: &Codec, nid: NidT) -> bool {
    valid_widget_nid(nid, this) && this.wi(nid).enable
}

/// `azalia_init_dacgroup`: the analog and digital DAC and ADC groups.
pub fn azalia_init_dacgroup(this: &mut Codec) -> Result<(), Errno> {
    let mut dacs = this.dacs;
    dacs.ngroups = 0;
    if this.na_dacs > 0 {
        let pins = this.opins.clone();
        let convs = this.a_dacs;
        let n = this.na_dacs as usize;
        azalia_add_convgroup(
            this,
            &mut dacs,
            &pins,
            &convs[..n],
            COP_AWTYPE_AUDIO_OUTPUT,
            0,
        )?;
    }
    if this.na_dacs_d > 0 {
        let pins = this.opins_d.clone();
        let convs = this.a_dacs_d;
        let n = this.na_dacs_d as usize;
        azalia_add_convgroup(
            this,
            &mut dacs,
            &pins,
            &convs[..n],
            COP_AWTYPE_AUDIO_OUTPUT,
            COP_AWCAP_DIGITAL,
        )?;
    }
    dacs.cur = 0;
    this.dacs = dacs;

    let mut adcs = this.adcs;
    adcs.ngroups = 0;
    if this.na_adcs > 0 {
        let pins = this.ipins.clone();
        let convs = this.a_adcs;
        let n = this.na_adcs as usize;
        azalia_add_convgroup(
            this,
            &mut adcs,
            &pins,
            &convs[..n],
            COP_AWTYPE_AUDIO_INPUT,
            0,
        )?;
    }
    if this.na_adcs_d > 0 {
        let pins = this.ipins_d.clone();
        let convs = this.a_adcs_d;
        let n = this.na_adcs_d as usize;
        azalia_add_convgroup(
            this,
            &mut adcs,
            &pins,
            &convs[..n],
            COP_AWTYPE_AUDIO_INPUT,
            COP_AWCAP_DIGITAL,
        )?;
    }
    adcs.cur = 0;
    this.adcs = adcs;

    Ok(())
}

/// `azalia_add_convgroup`: a group of the converters in `all_convs` that the pins reach,
/// default connections first; the converters left out are disabled.
pub fn azalia_add_convgroup(
    this: &mut Codec,
    group: &mut Convgroupset,
    pins: &[IoPin],
    all_convs: &[NidT],
    type_: u32,
    digital: u32,
) -> Result<(), Errno> {
    let mut convs = [0 as NidT; HDA_MAX_CHANNELS];
    let mut nconvs = 0;
    let nall_convs = all_convs.len();

    'done: {
        // default pin connections
        for pin in pins {
            let conv = pin.conv;
            if conv < 0 {
                continue;
            }
            if convs[..nconvs].contains(&conv) {
                continue;
            }
            convs[nconvs] = conv;
            nconvs += 1;
            if nconvs >= nall_convs {
                break 'done;
            }
        }
        // non-default connections
        for pin in pins {
            for &conv in all_convs {
                if convs[..nconvs].contains(&conv) {
                    continue;
                }
                if type_ == COP_AWTYPE_AUDIO_OUTPUT {
                    if azalia_codec_fnode(this, conv, pin.nid, 0) < 0 {
                        continue;
                    }
                } else {
                    if !azalia_widget_enabled(this, conv) {
                        continue;
                    }
                    if azalia_codec_fnode(this, pin.nid, conv, 0) < 0 {
                        continue;
                    }
                }
                convs[nconvs] = conv;
                nconvs += 1;
                if nconvs >= nall_convs {
                    break 'done;
                }
            }
        }
        // Make sure the speaker dac is part of the analog output convgroup or it won't get
        // connected by azalia_codec_connect_stream().
        if type_ == COP_AWTYPE_AUDIO_OUTPUT
            && digital == 0
            && nconvs < nall_convs
            && this.spkr_dac != -1
            && !convs[..nconvs].contains(&this.spkr_dac)
        {
            convs[nconvs] = this.spkr_dac;
            nconvs += 1;
        }
    }
    // done:
    let g = &mut group.groups[group.ngroups as usize];
    g.conv[..nconvs].copy_from_slice(&convs[..nconvs]);
    if nconvs > 0 {
        g.nconv = nconvs as i32;
        group.ngroups += 1;
    }

    // Disable converters that aren't in a convgroup.
    for &conv in all_convs {
        if !convs[..nconvs].contains(&conv) {
            this.wi_mut(conv).enable = false;
        }
    }

    Ok(())
}

/// `azalia_codec_fnode`: `index` if node `node` reaches widget `index` through enabled
/// widgets within ten hops, -1 otherwise.
pub fn azalia_codec_fnode(this: &Codec, node: NidT, index: i32, mut depth: i32) -> i32 {
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
    for &c in &w.connections {
        if !azalia_widget_enabled(this, c) {
            continue;
        }
        let ret = azalia_codec_fnode(this, node, c, depth);
        if ret >= 0 {
            return ret;
        }
    }
    -1
}

/// `azalia_unsol_event`: a jack-sense change mutes or unmutes the speaker; a volume-knob
/// turn moves the play volume.
pub fn azalia_unsol_event(this: &mut Codec, tag: i32) -> Result<(), Errno> {
    let mut mc = MixerCtrl::default();
    let mut err: Result<(), Errno> = Ok(());
    let tag = corb_unsol_tag(tag as u32) as i32;
    match tag {
        AZ_TAG_SPKR => {
            mc.type_ = AUDIO_MIXER_ENUM;
            let mut vol = 0;
            for i in 0..this.nsense_pins as usize {
                if vol != 0 || err.is_err() {
                    break;
                }
                if this.spkr_muters & (1 << i) == 0 {
                    continue;
                }
                let pin = this.sense_pins[i];
                match azalia_comresp(this, pin, CORB_GET_PIN_WIDGET_CONTROL, 0) {
                    Err(e) => {
                        err = Err(e);
                        continue;
                    }
                    Ok(result) if result & CORB_PWC_OUTPUT == 0 => continue,
                    Ok(_) => {}
                }
                match azalia_comresp(this, pin, CORB_GET_PIN_SENSE, 0) {
                    Ok(result) if result & CORB_PS_PRESENCE != 0 => vol = 1,
                    Ok(_) => {}
                    Err(e) => err = Err(e),
                }
            }
            err?;
            this.spkr_muted = vol;
            match this.spkr_mute_method {
                AZ_SPKR_MUTE_SPKR_MUTE => {
                    mc.un.set_ord(vol);
                    err = azalia_mixer_set(this, this.speaker, MI_TARGET_OUTAMP, &mc);
                    if err.is_ok() && this.speaker2 != -1 {
                        let w: &Widget = this.wi(this.speaker2);
                        if w.widgetcap & COP_AWCAP_OUTAMP != 0
                            && w.outamp_cap & COP_AMPCAP_MUTE != 0
                        {
                            err = azalia_mixer_set(this, this.speaker2, MI_TARGET_OUTAMP, &mc);
                        }
                    }
                }
                AZ_SPKR_MUTE_SPKR_DIR => {
                    mc.un.set_ord(if vol != 0 { 0 } else { 1 });
                    err = azalia_mixer_set(this, this.speaker, MI_TARGET_PINDIR, &mc);
                    if err.is_ok() && this.speaker2 != -1 {
                        let cap = this.wi(this.speaker2).d.pin().cap;
                        if cap & COP_PINCAP_OUTPUT != 0 && cap & COP_PINCAP_INPUT != 0 {
                            err = azalia_mixer_set(this, this.speaker2, MI_TARGET_PINDIR, &mc);
                        }
                    }
                }
                AZ_SPKR_MUTE_DAC_MUTE => {
                    mc.un.set_ord(vol);
                    err = azalia_mixer_set(this, this.spkr_dac, MI_TARGET_OUTAMP, &mc);
                }
                _ => {}
            }
        }

        AZ_TAG_PLAYVOL => {
            if this.playvols.master == this.audiofunc {
                return Err(Errno::EINVAL);
            }
            let result = azalia_comresp(this, this.playvols.master, CORB_GET_VOLUME_KNOB, 0)?;

            let vol = corb_vknob_volume(result) as i32 - this.playvols.hw_step;
            let vol2 = vol * (AUDIO_MAX_GAIN / this.playvols.hw_nsteps);
            this.playvols.hw_step = corb_vknob_volume(result) as i32;

            this.playvols.vol_l = (vol2 + this.playvols.vol_l).clamp(0, AUDIO_MAX_GAIN);
            this.playvols.vol_r = (vol2 + this.playvols.vol_r).clamp(0, AUDIO_MAX_GAIN);

            mc.type_ = AUDIO_MIXER_VALUE;
            let value = mc.un.value_mut();
            value.num_channels = 2;
            value.level[0] = this.playvols.vol_l as u8;
            value.level[1] = this.playvols.vol_r as u8;
            err = azalia_mixer_set(this, this.playvols.master, MI_TARGET_PLAYVOL, &mc);
        }

        _ => {
            // unknown tag
        }
    }

    err
}

// ----------------------------------------------------------------
// Generic mixer functions
// ----------------------------------------------------------------

/// `MIXER_DELTA(n)`: the step of a value control that has `n` hardware steps.
const fn mixer_delta(n: u32) -> i32 {
    AUDIO_MAX_GAIN / n as i32
}

/// The text of a C string held in a fixed buffer: the bytes up to the first NUL.
fn cstr(s: &[u8]) -> &[u8] {
    let len = s.iter().position(|&b| b == 0).unwrap_or(s.len());
    &s[..len]
}

/// A mixer control for widget `nid`, as `M_ZERO` memory holds it: the part of
/// `MIXER_REG_PROLOG` that sets the item up (the item is added by [`mixer_add`]).
fn mixer_new(nid: NidT) -> MixerItem {
    let mut m = MixerItem::new();
    m.nid = nid;
    m
}

/// The end of a registration in `azalia_mixer_init`: `MIXER_REG_PROLOG`'s
/// `azalia_mixer_ensure_capacity` and the C's `this->nmixers++`.
fn mixer_add(this: &mut Codec, m: MixerItem) -> Result<(), Errno> {
    azalia_mixer_ensure_capacity(this, this.mixers.len() + 1)?;
    this.mixers.push(m);
    Ok(())
}

/// The class of an output control of widget `w`: its own, or by its type.
fn out_class(w: &Widget) -> i32 {
    if w.mixer_class >= 0 {
        w.mixer_class
    } else if w.type_ == COP_AWTYPE_AUDIO_MIXER
        || w.type_ == COP_AWTYPE_AUDIO_SELECTOR
        || w.type_ == COP_AWTYPE_PIN_COMPLEX
    {
        AZ_CLASS_OUTPUT
    } else {
        AZ_CLASS_INPUT
    }
}

/// The class of an input control of widget `w`: its own, or `AZ_CLASS_INPUT`.
fn in_class(w: &Widget) -> i32 {
    if w.mixer_class >= 0 {
        w.mixer_class
    } else {
        AZ_CLASS_INPUT
    }
}

/// Fill the members of a set control with the enabled connections of `w` that are not the
/// speakers, as the mute-set and the hardcoded-mixer-inputs controls of `azalia_mixer_init`
/// list them. Returns whether there is any member (the C adds the control only then).
fn fill_connection_set(this: &Codec, w: &Widget, d: &mut MixerDevinfo) -> bool {
    let mut k = 0;
    for (j, &c) in w.connections.iter().enumerate() {
        if k >= 32 {
            break;
        }
        if !azalia_widget_enabled(this, c) {
            continue;
        }
        if c == this.speaker || c == this.speaker2 {
            continue;
        }
        let member = &mut d.un.s_mut().member[k];
        member.mask = 1 << j;
        strlcpy(&mut member.label.name, &this.wi(c).name);
        k += 1;
    }
    d.un.s_mut().num_mem = k as i32;
    k != 0
}

/// The shape of a value control over an amplifier of `w` with `steps` steps.
fn set_value_shape(d: &mut MixerDevinfo, w: &Widget, steps: u32) {
    d.type_ = AUDIO_MIXER_VALUE;
    let v = d.un.v_mut();
    v.num_channels = widget_channels(w);
    v.units.name[0] = 0;
    v.delta = mixer_delta(steps);
}

/// The three controls of a volume group (`playvols`/`recvols`: volume, mute and slaves):
/// the C writes them out twice with the same lines.
fn push_volgroup(
    this: &mut Codec,
    target: i32,
    class: i32,
    first_label: &[u8],
    group: &Volgroup,
) -> Result<(), Errno> {
    let master = group.master;
    azalia_mixer_ensure_capacity(this, this.mixers.len() + 3)?;

    // volume
    let mut m = mixer_new(master);
    m.target = target;
    m.devinfo.mixer_class = class;
    strlcpy(&mut m.devinfo.label.name, first_label);
    m.devinfo.type_ = AUDIO_MIXER_VALUE;
    m.devinfo.un.v_mut().num_channels = 2;
    m.devinfo.un.v_mut().delta = 8;
    let n = this.mixers.len() as i32 + 1;
    m.devinfo.next = n;
    this.mixers.push(m);

    // mute
    let mut m = mixer_new(master);
    m.target = target;
    m.devinfo.prev = n - 1;
    m.devinfo.mixer_class = class;
    strlcpy(&mut m.devinfo.label.name, AudioNmute);
    azalia_devinfo_offon(&mut m.devinfo);
    m.devinfo.next = n + 1;
    this.mixers.push(m);

    // slaves
    let mut m = mixer_new(master);
    m.target = target;
    m.devinfo.prev = n;
    m.devinfo.mixer_class = class;
    strlcpy(&mut m.devinfo.label.name, b"slaves");
    m.devinfo.type_ = AUDIO_MIXER_SET;
    for i in 0..group.nslaves as usize {
        let ww = this.wi(group.slaves[i]);
        let member = &mut m.devinfo.un.s_mut().member[i];
        member.mask = 1 << i;
        strlcpy(&mut member.label.name, &ww.name);
    }
    m.devinfo.un.s_mut().num_mem = group.nslaves;
    this.mixers.push(m);
    Ok(())
}

/// The "mode" control (analog or digital converters) of `azalia_mixer_init`.
fn mode_control(this: &Codec, class: i32, target: i32) -> MixerItem {
    let mut m = mixer_new(this.audiofunc);
    let d = &mut m.devinfo;
    strlcpy(&mut d.label.name, AudioNmode);
    d.type_ = AUDIO_MIXER_ENUM;
    d.mixer_class = class;
    m.target = target;
    let e = m.devinfo.un.e_mut();
    e.member[0].ord = 0;
    strlcpy(&mut e.member[0].label.name, b"analog");
    e.member[1].ord = 1;
    strlcpy(&mut e.member[1].label.name, b"digital");
    e.num_mem = 2;
    m
}

/// `azalia_mixer_init`: build the codec's mixer controls from its widgets, then give them
/// their default values and enable the unsolicited responses.
pub fn azalia_mixer_init(this: &mut Codec) -> Result<(), Errno> {
    azalia_mixer_register(this)?;

    azalia_mixer_fix_indexes(this)?;
    // The C ignores the result of the defaults (and of `azalia_codec_enable_unsol`).
    let _ = azalia_mixer_default(this);
    Ok(())
}

/// The first part of `azalia_mixer_init`: the list of controls. Split from it so the host
/// tests can run it without a controller to talk to.
fn azalia_mixer_register(this: &mut Codec) -> Result<(), Errno> {
    // pin		"<color>%2.2x"
    // audio output	"dac%2.2x"
    // audio input	"adc%2.2x"
    // mixer	"mixer%2.2x"
    // selector	"sel%2.2x"

    this.mixers = Vec::new();
    if this.mixers.try_reserve_exact(10).is_err() {
        printf(format_args!(
            "{}: out of memory in azalia_mixer_init\n",
            this.az().dev.xname()
        ));
        return Err(Errno::ENOMEM);
    }

    // register classes
    for (index, name) in [
        (AZ_CLASS_INPUT, AudioCinputs),
        (AZ_CLASS_OUTPUT, AudioCoutputs),
        (AZ_CLASS_RECORD, AudioCrecord),
    ] {
        let mut m = MixerItem::new();
        m.devinfo.index = index;
        strlcpy(&mut m.devinfo.label.name, name);
        m.devinfo.type_ = AUDIO_MIXER_CLASS;
        m.devinfo.mixer_class = index;
        m.devinfo.next = AUDIO_MIXER_LAST;
        m.devinfo.prev = AUDIO_MIXER_LAST;
        m.nid = 0;
        this.mixers.push(m);
    }

    for i in this.widgets() {
        let w = this.wi(i).clone();
        if !w.enable {
            continue;
        }

        // selector
        if w.nconnections() > 0
            && w.type_ != COP_AWTYPE_AUDIO_MIXER
            && !(w.nconnections() == 1
                && azalia_widget_enabled(this, w.connections[0])
                && cstr(&w.name) == cstr(&this.wi(w.connections[0]).name))
            && w.nid != this.mic
        {
            let mut m = mixer_new(i);
            let d = &mut m.devinfo;
            snprintf(&mut d.label.name, format_args!("{}_source", Str(&w.name)));
            d.type_ = AUDIO_MIXER_ENUM;
            if w.mixer_class >= 0 {
                d.mixer_class = w.mixer_class;
            } else if w.type_ == COP_AWTYPE_AUDIO_SELECTOR {
                d.mixer_class = AZ_CLASS_INPUT;
            } else {
                d.mixer_class = AZ_CLASS_OUTPUT;
            }
            m.target = MI_TARGET_CONNLIST;
            let mut k = 0;
            for (j, &c) in w.connections.iter().enumerate() {
                if k >= 32 {
                    break;
                }
                if !azalia_widget_enabled(this, c) {
                    continue;
                }
                let member = &mut m.devinfo.un.e_mut().member[k];
                member.ord = j as i32;
                strlcpy(&mut member.label.name, &this.wi(c).name);
                k += 1;
            }
            m.devinfo.un.e_mut().num_mem = k as i32;
            mixer_add(this, m)?;
        }

        // output mute
        if w.widgetcap & COP_AWCAP_OUTAMP != 0
            && w.outamp_cap & COP_AMPCAP_MUTE != 0
            && w.nid != this.mic
        {
            let mut m = mixer_new(i);
            snprintf(
                &mut m.devinfo.label.name,
                format_args!("{}_mute", Str(&w.name)),
            );
            m.devinfo.mixer_class = out_class(&w);
            m.target = MI_TARGET_OUTAMP;
            azalia_devinfo_offon(&mut m.devinfo);
            mixer_add(this, m)?;
        }

        // output gain
        if w.widgetcap & COP_AWCAP_OUTAMP != 0
            && cop_ampcap_numsteps(w.outamp_cap) != 0
            && w.nid != this.mic
        {
            let mut m = mixer_new(i);
            snprintf(&mut m.devinfo.label.name, format_args!("{}", Str(&w.name)));
            m.devinfo.mixer_class = out_class(&w);
            m.target = MI_TARGET_OUTAMP;
            set_value_shape(&mut m.devinfo, &w, cop_ampcap_numsteps(w.outamp_cap));
            mixer_add(this, m)?;
        }

        // input mute
        if w.widgetcap & COP_AWCAP_INAMP != 0
            && w.inamp_cap & COP_AMPCAP_MUTE != 0
            && w.nid != this.speaker
            && w.nid != this.speaker2
        {
            if w.type_ != COP_AWTYPE_AUDIO_MIXER {
                let mut m = mixer_new(i);
                snprintf(
                    &mut m.devinfo.label.name,
                    format_args!("{}_mute", Str(&w.name)),
                );
                m.devinfo.mixer_class = in_class(&w);
                m.target = 0;
                azalia_devinfo_offon(&mut m.devinfo);
                mixer_add(this, m)?;
            } else {
                let mut m = mixer_new(i);
                snprintf(
                    &mut m.devinfo.label.name,
                    format_args!("{}_source", Str(&w.name)),
                );
                m.target = MI_TARGET_MUTESET;
                m.devinfo.type_ = AUDIO_MIXER_SET;
                m.devinfo.mixer_class = in_class(&w);
                if fill_connection_set(this, &w, &mut m.devinfo) {
                    mixer_add(this, m)?;
                }
            }
        }

        // input gain
        if w.widgetcap & COP_AWCAP_INAMP != 0
            && cop_ampcap_numsteps(w.inamp_cap) != 0
            && w.nid != this.speaker
            && w.nid != this.speaker2
        {
            if w.type_ != COP_AWTYPE_AUDIO_SELECTOR && w.type_ != COP_AWTYPE_AUDIO_MIXER {
                let mut m = mixer_new(i);
                snprintf(&mut m.devinfo.label.name, format_args!("{}", Str(&w.name)));
                m.devinfo.mixer_class = in_class(&w);
                m.target = 0;
                set_value_shape(&mut m.devinfo, &w, cop_ampcap_numsteps(w.inamp_cap));
                mixer_add(this, m)?;
            } else {
                for (j, &c) in w.connections.iter().enumerate() {
                    if !azalia_widget_enabled(this, c) {
                        continue;
                    }
                    if c == this.speaker || c == this.speaker2 {
                        continue;
                    }
                    let mut m = mixer_new(i);
                    snprintf(
                        &mut m.devinfo.label.name,
                        format_args!("{}_{}", Str(&w.name), Str(&this.wi(c).name)),
                    );
                    m.devinfo.mixer_class = in_class(&w);
                    m.target = j as i32;
                    set_value_shape(&mut m.devinfo, &w, cop_ampcap_numsteps(w.inamp_cap));
                    mixer_add(this, m)?;
                }
            }
        }

        // hardcoded mixer inputs
        if w.type_ == COP_AWTYPE_AUDIO_MIXER && w.widgetcap & COP_AWCAP_INAMP == 0 {
            let mut m = mixer_new(i);
            snprintf(
                &mut m.devinfo.label.name,
                format_args!("{}_source", Str(&w.name)),
            );
            m.target = MI_TARGET_MIXERSET;
            m.devinfo.type_ = AUDIO_MIXER_SET;
            m.devinfo.mixer_class = in_class(&w);
            if fill_connection_set(this, &w, &mut m.devinfo) {
                mixer_add(this, m)?;
            }
        }

        // pin direction
        let pincap = w.d.pin().cap;
        if w.type_ == COP_AWTYPE_PIN_COMPLEX
            && ((pincap & COP_PINCAP_OUTPUT != 0 && pincap & COP_PINCAP_INPUT != 0)
                || cop_pincap_vref(pincap) > 1)
        {
            let mut m = mixer_new(i);
            let d = &mut m.devinfo;
            snprintf(&mut d.label.name, format_args!("{}_dir", Str(&w.name)));
            d.type_ = AUDIO_MIXER_ENUM;
            d.mixer_class = AZ_CLASS_OUTPUT;
            m.target = MI_TARGET_PINDIR;
            let e = m.devinfo.un.e_mut();

            let mut k = 0;
            e.member[k].ord = 0;
            strlcpy(&mut e.member[k].label.name, b"none");
            k += 1;

            if pincap & COP_PINCAP_OUTPUT != 0 {
                e.member[k].ord = 1;
                strlcpy(&mut e.member[k].label.name, AudioNoutput);
                k += 1;
            }

            if pincap & COP_PINCAP_INPUT != 0 {
                e.member[k].ord = 2;
                strlcpy(&mut e.member[k].label.name, AudioNinput);
                k += 1;

                for (j, (vref, label)) in [
                    (CORB_PWC_VREF_GND, b"input-vr0".as_slice()),
                    (CORB_PWC_VREF_50, b"input-vr50".as_slice()),
                    (CORB_PWC_VREF_80, b"input-vr80".as_slice()),
                    (CORB_PWC_VREF_100, b"input-vr100".as_slice()),
                ]
                .into_iter()
                .enumerate()
                {
                    // the pin keeps the slot only if it supports the reference voltage
                    let bits = 1 << vref;
                    if cop_pincap_vref(pincap) & bits == bits {
                        strlcpy(&mut e.member[k].label.name, label);
                        e.member[k].ord = j as i32 + 3;
                        k += 1;
                    }
                }
            }
            e.num_mem = k as i32;
            mixer_add(this, m)?;
        }

        // pin headphone-boost
        if w.type_ == COP_AWTYPE_PIN_COMPLEX
            && pincap & COP_PINCAP_HEADPHONE != 0
            && w.nid != this.mic
        {
            let mut m = mixer_new(i);
            snprintf(
                &mut m.devinfo.label.name,
                format_args!("{}_boost", Str(&w.name)),
            );
            m.devinfo.mixer_class = AZ_CLASS_OUTPUT;
            m.target = MI_TARGET_PINBOOST;
            azalia_devinfo_offon(&mut m.devinfo);
            mixer_add(this, m)?;
        }

        if w.type_ == COP_AWTYPE_PIN_COMPLEX && pincap & COP_PINCAP_EAPD != 0 {
            let mut m = mixer_new(i);
            snprintf(
                &mut m.devinfo.label.name,
                format_args!("{}_eapd", Str(&w.name)),
            );
            m.devinfo.mixer_class = AZ_CLASS_OUTPUT;
            m.target = MI_TARGET_EAPD;
            azalia_devinfo_offon(&mut m.devinfo);
            mixer_add(this, m)?;
        }
    }

    // sense pins
    for i in 0..this.nsense_pins as usize {
        let pin = this.sense_pins[i];
        if !azalia_widget_enabled(this, pin) {
            // sense pin not found
            continue;
        }

        let ww = this.wi(pin);
        let mut m = mixer_new(ww.nid);
        let d = &mut m.devinfo;
        snprintf(&mut d.label.name, format_args!("{}_sense", Str(&ww.name)));
        d.type_ = AUDIO_MIXER_ENUM;
        d.mixer_class = AZ_CLASS_OUTPUT;
        m.target = MI_TARGET_PINSENSE;
        let e = m.devinfo.un.e_mut();
        e.num_mem = 2;
        e.member[0].ord = 0;
        strlcpy(&mut e.member[0].label.name, b"unplugged");
        e.member[1].ord = 1;
        strlcpy(&mut e.member[1].label.name, b"plugged");
        mixer_add(this, m)?;
    }

    // spkr mute by jack sense
    this.spkr_mute_method = AZ_SPKR_MUTE_NONE;
    if this.speaker != -1 && this.spkr_dac != -1 && this.nsense_pins > 0 {
        let w = this.wi(this.speaker);
        if w.widgetcap & COP_AWCAP_OUTAMP != 0 && w.outamp_cap & COP_AMPCAP_MUTE != 0 {
            this.spkr_mute_method = AZ_SPKR_MUTE_SPKR_MUTE;
        } else if w.d.pin().cap & COP_PINCAP_OUTPUT != 0 && w.d.pin().cap & COP_PINCAP_INPUT != 0 {
            this.spkr_mute_method = AZ_SPKR_MUTE_SPKR_DIR;
        } else {
            let w = this.wi(this.spkr_dac);
            if w.nid != this.dacs.groups[0].conv[0]
                && w.widgetcap & COP_AWCAP_OUTAMP != 0
                && w.outamp_cap & COP_AMPCAP_MUTE != 0
            {
                this.spkr_mute_method = AZ_SPKR_MUTE_DAC_MUTE;
            }
        }
    }
    if this.spkr_mute_method != AZ_SPKR_MUTE_NONE {
        let w = this.wi(this.speaker);
        let mut m = mixer_new(w.nid);
        snprintf(
            &mut m.devinfo.label.name,
            format_args!("{}_muters", Str(&w.name)),
        );
        m.target = MI_TARGET_SENSESET;
        m.devinfo.type_ = AUDIO_MIXER_SET;
        m.devinfo.mixer_class = AZ_CLASS_OUTPUT;
        let mut spkr_muters = 0;
        let mut j = 0;
        for i in 0..this.nsense_pins as usize {
            let ww = this.wi(this.sense_pins[i]);
            if ww.d.pin().cap & COP_PINCAP_OUTPUT == 0 {
                continue;
            }
            if ww.widgetcap & COP_AWCAP_UNSOL == 0 {
                continue;
            }
            let member = &mut m.devinfo.un.s_mut().member[j];
            member.mask = 1 << i;
            spkr_muters |= 1 << i;
            strlcpy(&mut member.label.name, &ww.name);
            j += 1;
        }
        m.devinfo.un.s_mut().num_mem = j as i32;
        this.spkr_muters = spkr_muters;
        if j != 0 {
            mixer_add(this, m)?;
        }
    }

    // playback volume group
    if this.playvols.nslaves > 0 {
        let group = this.playvols;
        push_volgroup(
            this,
            MI_TARGET_PLAYVOL,
            AZ_CLASS_OUTPUT,
            AudioNmaster,
            &group,
        )?;
    }

    // recording volume group
    if this.recvols.nslaves > 0 {
        let group = this.recvols;
        push_volgroup(
            this,
            MI_TARGET_RECVOL,
            AZ_CLASS_RECORD,
            AudioNvolume,
            &group,
        )?;
    }

    // if the codec has more than one DAC group, the first is analog
    // and the second is digital.
    if this.dacs.ngroups > 1 {
        let m = mode_control(this, AZ_CLASS_OUTPUT, MI_TARGET_DAC);
        mixer_add(this, m)?;
    }

    // if the codec has more than one ADC group, the first is analog
    // and the second is digital.
    if this.adcs.ngroups > 1 {
        let m = mode_control(this, AZ_CLASS_RECORD, MI_TARGET_ADC);
        mixer_add(this, m)?;
    }

    Ok(())
}

/// `azalia_devinfo_offon`: make `d` an off/on enumeration.
pub fn azalia_devinfo_offon(d: &mut MixerDevinfo) {
    d.type_ = AUDIO_MIXER_ENUM;
    let e = d.un.e_mut();
    e.num_mem = 2;
    e.member[0].ord = 0;
    strlcpy(&mut e.member[0].label.name, AudioNoff);
    e.member[1].ord = 1;
    strlcpy(&mut e.member[1].label.name, AudioNon);
}

/// `azalia_mixer_ensure_capacity`: room for `newsize` controls. The C grows `mixers` by ten
/// slots (or to `newsize`); `maxmixers` is the vector's capacity.
pub fn azalia_mixer_ensure_capacity(this: &mut Codec, newsize: usize) -> Result<(), Errno> {
    let maxmixers = this.mixers.capacity();
    if maxmixers >= newsize {
        return Ok(());
    }
    let newmax = (maxmixers + 10).max(newsize);
    if this
        .mixers
        .try_reserve_exact(newmax - this.mixers.len())
        .is_err()
    {
        printf(format_args!(
            "{}: out of memory in azalia_mixer_ensure_capacity\n",
            this.az().dev.xname()
        ));
        return Err(Errno::ENOMEM);
    }
    Ok(())
}

/// `azalia_mixer_fix_indexes`: number the controls and end every chain with
/// `AUDIO_MIXER_LAST`.
pub fn azalia_mixer_fix_indexes(this: &mut Codec) -> Result<(), Errno> {
    for (i, m) in this.mixers.iter_mut().enumerate() {
        let d = &mut m.devinfo;
        #[cfg(feature = "diagnostic")]
        {
            if d.index != 0 && d.index != i as i32 {
                printf(format_args!(
                    "azalia_mixer_fix_indexes: index mismatch {} {}\n",
                    d.index, i
                ));
            }
        }
        d.index = i as i32;
        if d.prev == 0 {
            d.prev = AUDIO_MIXER_LAST;
        }
        if d.next == 0 {
            d.next = AUDIO_MIXER_LAST;
        }
    }
    Ok(())
}

/// `azalia_mixer_default`: unmute everything, set the amplifiers to half gain, select a
/// valid input for every selector, read the volume groups' masters, and enable the
/// unsolicited responses.
pub fn azalia_mixer_default(this: &mut Codec) -> Result<(), Errno> {
    // One `mixer_ctrl_t` for the whole function, as in the C: the connection-list check
    // reads with whatever `type` an earlier step left in it.
    let mut mc = MixerCtrl::default();

    // unmute all
    for i in 0..this.mixers.len() {
        let (nid, target, type_) = {
            let m = &this.mixers[i];
            (m.nid, m.target, m.devinfo.type_)
        };
        if !is_mi_target_inamp(target) && target != MI_TARGET_OUTAMP {
            continue;
        }
        if type_ != AUDIO_MIXER_ENUM {
            continue;
        }
        mc = MixerCtrl::default();
        mc.dev = i as i32;
        mc.type_ = AUDIO_MIXER_ENUM;
        let _ = azalia_mixer_set(this, nid, target, &mc);
    }

    // set unextreme volume
    for i in 0..this.mixers.len() {
        let (nid, target, type_) = {
            let m = &this.mixers[i];
            (m.nid, m.target, m.devinfo.type_)
        };
        if !is_mi_target_inamp(target) && target != MI_TARGET_OUTAMP {
            continue;
        }
        if type_ != AUDIO_MIXER_VALUE {
            continue;
        }
        mc = MixerCtrl::default();
        mc.dev = i as i32;
        mc.type_ = AUDIO_MIXER_VALUE;
        let stereo = widget_channels(this.wi(nid)) == 2;
        let value = mc.un.value_mut();
        value.num_channels = 1;
        value.level[0] = (AUDIO_MAX_GAIN / 2) as u8;
        if stereo {
            value.num_channels = 2;
            value.level[1] = value.level[0];
        }
        let _ = azalia_mixer_set(this, nid, target, &mc);
    }

    // unmute all
    for i in 0..this.mixers.len() {
        let (nid, target, type_) = {
            let m = &this.mixers[i];
            (m.nid, m.target, m.devinfo.type_)
        };
        if target != MI_TARGET_MUTESET {
            continue;
        }
        if type_ != AUDIO_MIXER_SET {
            continue;
        }
        mc = MixerCtrl::default();
        mc.dev = i as i32;
        mc.type_ = AUDIO_MIXER_SET;
        if !azalia_widget_enabled(this, nid) {
            // invalid set nid
            return Err(Errno::EINVAL);
        }
        let w = this.wi(nid);
        let mut mask = 0;
        for (j, &c) in w.connections.iter().enumerate() {
            if !azalia_widget_enabled(this, c) {
                continue;
            }
            if w.nid == this.input_mixer && c == this.mic {
                continue;
            }
            mask |= 1 << j;
        }
        mc.un.set_mask(mask);
        let _ = azalia_mixer_set(this, nid, target, &mc);
    }

    // make sure default connection is valid
    for i in 0..this.mixers.len() {
        let (nid, target) = {
            let m = &this.mixers[i];
            (m.nid, m.target)
        };
        if target != MI_TARGET_CONNLIST {
            continue;
        }

        let _ = azalia_mixer_get(this, nid, target, &mut mc);
        let e = this.mixers[i].devinfo.un.e();
        let members = &e.member[..e.num_mem as usize];
        if !members.iter().any(|member| member.ord == mc.un.ord()) {
            let first = e.member[0].ord;
            mc = MixerCtrl::default();
            mc.dev = i as i32;
            mc.type_ = AUDIO_MIXER_ENUM;
            mc.un.set_ord(first);
        }
        let _ = azalia_mixer_set(this, nid, target, &mc);
    }

    // get default value for play group master
    for i in 0..this.playvols.nslaves as usize {
        if this.playvols.cur & (1 << i) == 0 {
            continue;
        }
        let w = this.wi(this.playvols.slaves[i]);
        if cop_ampcap_numsteps(w.outamp_cap) == 0 {
            continue;
        }
        let nid = w.nid;
        mc.type_ = AUDIO_MIXER_VALUE;
        let _ = azalia_mixer_get(this, nid, MI_TARGET_OUTAMP, &mut mc);
        this.playvols.vol_l = i32::from(mc.un.value().level[0]);
        this.playvols.vol_r = i32::from(mc.un.value().level[0]);
        break;
    }
    this.playvols.mute = 0;

    // get default value for record group master
    for i in 0..this.recvols.nslaves as usize {
        if this.recvols.cur & (1 << i) == 0 {
            continue;
        }
        let w = this.wi(this.recvols.slaves[i]);
        mc.type_ = AUDIO_MIXER_VALUE;
        let mut tgt = MI_TARGET_OUTAMP;
        let mut cap = w.outamp_cap;
        if w.type_ == COP_AWTYPE_PIN_COMPLEX || w.type_ == COP_AWTYPE_AUDIO_INPUT {
            tgt = 0;
            cap = w.inamp_cap;
        }
        if cop_ampcap_numsteps(cap) == 0 {
            continue;
        }
        let nid = w.nid;
        let _ = azalia_mixer_get(this, nid, tgt, &mut mc);
        this.recvols.vol_l = i32::from(mc.un.value().level[0]);
        this.recvols.vol_r = i32::from(mc.un.value().level[0]);
        break;
    }
    this.recvols.mute = 0;

    azalia_codec_enable_unsol(this)?;

    Ok(())
}

/// `azalia_codec_enable_unsol`: unsolicited responses from the sense pins that mute the
/// speaker and from the volume knob.
pub fn azalia_codec_enable_unsol(this: &mut Codec) -> Result<(), Errno> {
    // jack sense
    for i in 0..this.nsense_pins as usize {
        if this.spkr_muters & (1 << i) != 0 {
            let _ = azalia_comresp(
                this,
                this.sense_pins[i],
                CORB_SET_UNSOLICITED_RESPONSE,
                CORB_UNSOL_ENABLE | AZ_TAG_SPKR as u32,
            );
        }
    }
    if this.spkr_muters != 0 {
        let _ = azalia_unsol_event(this, AZ_TAG_SPKR);
    }

    // volume knob
    if this.playvols.master != this.audiofunc {
        let w = this.wi(this.playvols.master);
        let nid = w.nid;
        let nsteps = cop_vkcap_numsteps(w.d.volume().cap) as i32;
        // get volume knob error
        let mut result = azalia_comresp(this, nid, CORB_GET_VOLUME_KNOB, 0)?;

        // current level
        this.playvols.hw_step = corb_vknob_volume(result) as i32;
        this.playvols.hw_nsteps = nsteps;

        // indirect mode
        result &= !CORB_VKNOB_DIRECT;
        if azalia_comresp(this, nid, CORB_SET_VOLUME_KNOB, result).is_err() {
            // XXX If there was an error setting indirect mode, do not return an error.
            // However, do not enable unsolicited responses either. Most likely the volume
            // knob doesn't work right. Perhaps it's simply not wired/enabled.
            return Ok(());
        }

        // enable unsolicited responses
        let result = CORB_UNSOL_ENABLE | AZ_TAG_PLAYVOL as u32;
        // set vknob unsol resp error
        azalia_comresp(this, nid, CORB_SET_UNSOLICITED_RESPONSE, result)?;
    }

    Ok(())
}

/// `azalia_mixer_delete`: free the mixer controls.
pub fn azalia_mixer_delete(this: &mut Codec) -> Result<(), Errno> {
    this.mixers = Vec::new();
    Ok(())
}

/// `azalia_mixer_get`: the value of control `target` of widget `nid` (`mc->type` is set by
/// the caller).
pub fn azalia_mixer_get(
    this: &Codec,
    nid: NidT,
    target: i32,
    mc: &mut MixerCtrl,
) -> Result<(), Errno> {
    if mc.type_ == AUDIO_MIXER_CLASS {
        return Ok(());
    }
    // inamp mute
    else if is_mi_target_inamp(target) && mc.type_ == AUDIO_MIXER_ENUM {
        let result = azalia_comresp(
            this,
            nid,
            CORB_GET_AMPLIFIER_GAIN_MUTE,
            CORB_GAGM_INPUT | CORB_GAGM_LEFT | mi_target_inamp(target) as u32,
        )?;
        mc.un.set_ord(i32::from(result & CORB_GAGM_MUTE != 0));
    }
    // inamp gain
    else if is_mi_target_inamp(target) && mc.type_ == AUDIO_MIXER_VALUE {
        let result = azalia_comresp(
            this,
            nid,
            CORB_GET_AMPLIFIER_GAIN_MUTE,
            CORB_GAGM_INPUT | CORB_GAGM_LEFT | mi_target_inamp(target) as u32,
        )?;
        mc.un.value_mut().level[0] =
            azalia_mixer_from_device_value(this, nid, target, corb_gagm_gain(result));
        let w = this.wi(nid);
        let n = if w.type_ == COP_AWTYPE_AUDIO_SELECTOR || w.type_ == COP_AWTYPE_AUDIO_MIXER {
            match w.connections.get(mi_target_inamp(target) as usize) {
                Some(&n) if azalia_widget_enabled(this, n) => n,
                // invalid index
                _ => nid,
            }
        } else {
            nid
        };
        let channels = widget_channels(this.wi(n));
        mc.un.value_mut().num_channels = channels;
        if channels == 2 {
            let result = azalia_comresp(
                this,
                nid,
                CORB_GET_AMPLIFIER_GAIN_MUTE,
                CORB_GAGM_INPUT | CORB_GAGM_RIGHT | mi_target_inamp(target) as u32,
            )?;
            mc.un.value_mut().level[1] =
                azalia_mixer_from_device_value(this, nid, target, corb_gagm_gain(result));
        }
    }
    // outamp mute
    else if target == MI_TARGET_OUTAMP && mc.type_ == AUDIO_MIXER_ENUM {
        let result = azalia_comresp(
            this,
            nid,
            CORB_GET_AMPLIFIER_GAIN_MUTE,
            CORB_GAGM_OUTPUT | CORB_GAGM_LEFT,
        )?;
        mc.un.set_ord(i32::from(result & CORB_GAGM_MUTE != 0));
    }
    // outamp gain
    else if target == MI_TARGET_OUTAMP && mc.type_ == AUDIO_MIXER_VALUE {
        let result = azalia_comresp(
            this,
            nid,
            CORB_GET_AMPLIFIER_GAIN_MUTE,
            CORB_GAGM_OUTPUT | CORB_GAGM_LEFT,
        )?;
        mc.un.value_mut().level[0] =
            azalia_mixer_from_device_value(this, nid, target, corb_gagm_gain(result));
        let channels = widget_channels(this.wi(nid));
        mc.un.value_mut().num_channels = channels;
        if channels == 2 {
            let result = azalia_comresp(
                this,
                nid,
                CORB_GET_AMPLIFIER_GAIN_MUTE,
                CORB_GAGM_OUTPUT | CORB_GAGM_RIGHT,
            )?;
            mc.un.value_mut().level[1] =
                azalia_mixer_from_device_value(this, nid, target, corb_gagm_gain(result));
        }
    }
    // selection
    else if target == MI_TARGET_CONNLIST {
        let result = azalia_comresp(this, nid, CORB_GET_CONNECTION_SELECT_CONTROL, 0)?;
        let result = corb_csc_index(result);
        match this.wi(nid).connections.get(result as usize) {
            Some(&c) if azalia_widget_enabled(this, c) => mc.un.set_ord(result as i32),
            _ => mc.un.set_ord(-1),
        }
    }
    // pin I/O
    else if target == MI_TARGET_PINDIR {
        let result = azalia_comresp(this, nid, CORB_GET_PIN_WIDGET_CONTROL, 0)?;

        if result & (CORB_PWC_INPUT | CORB_PWC_OUTPUT) == 0 {
            mc.un.set_ord(0);
        } else if result & CORB_PWC_OUTPUT != 0 {
            mc.un.set_ord(1);
        } else {
            mc.un.set_ord(match result & CORB_PWC_VREF_MASK {
                CORB_PWC_VREF_GND => 3,
                CORB_PWC_VREF_50 => 4,
                CORB_PWC_VREF_80 => 5,
                CORB_PWC_VREF_100 => 6,
                _ => 2,
            });
        }
    }
    // pin headphone-boost
    else if target == MI_TARGET_PINBOOST {
        let result = azalia_comresp(this, nid, CORB_GET_PIN_WIDGET_CONTROL, 0)?;
        mc.un.set_ord(i32::from(result & CORB_PWC_HEADPHONE != 0));
    }
    // DAC group selection
    else if target == MI_TARGET_DAC {
        mc.un.set_ord(this.dacs.cur);
    }
    // ADC selection
    else if target == MI_TARGET_ADC {
        mc.un.set_ord(this.adcs.cur);
    }
    // S/PDIF
    else if target == MI_TARGET_SPDIF {
        let result = azalia_comresp(this, nid, CORB_GET_DIGITAL_CONTROL, 0)?;
        mc.un
            .set_mask((result & 0xff & !(CORB_DCC_DIGEN | CORB_DCC_NAUDIO)) as i32);
    } else if target == MI_TARGET_SPDIF_CC {
        let result = azalia_comresp(this, nid, CORB_GET_DIGITAL_CONTROL, 0)?;
        mc.un.value_mut().num_channels = 1;
        mc.un.value_mut().level[0] = corb_dcc_cc(result) as u8;
    }
    // EAPD
    else if target == MI_TARGET_EAPD {
        let result = azalia_comresp(this, nid, CORB_GET_EAPD_BTL_ENABLE, 0)?;
        mc.un.set_ord(i32::from(result & CORB_EAPD_EAPD != 0));
    }
    // sense pin
    else if target == MI_TARGET_PINSENSE {
        let result = azalia_comresp(this, nid, CORB_GET_PIN_SENSE, 0)?;
        mc.un.set_ord(i32::from(result & CORB_PS_PRESENCE != 0));
    }
    // mute set
    else if target == MI_TARGET_MUTESET && mc.type_ == AUDIO_MIXER_SET {
        if !azalia_widget_enabled(this, nid) {
            // invalid muteset nid
            return Err(Errno::EINVAL);
        }
        let w = this.wi(nid);
        let mut mask = 0;
        for (i, &c) in w.connections.iter().enumerate() {
            if !azalia_widget_enabled(this, c) {
                continue;
            }
            let result = azalia_comresp(
                this,
                nid,
                CORB_GET_AMPLIFIER_GAIN_MUTE,
                CORB_GAGM_INPUT | CORB_GAGM_LEFT | mi_target_inamp(i as i32) as u32,
            )?;
            if result & CORB_GAGM_MUTE == 0 {
                mask |= 1 << i;
            }
        }
        mc.un.set_mask(mask);
    }
    // mixer set - show all connections
    else if target == MI_TARGET_MIXERSET && mc.type_ == AUDIO_MIXER_SET {
        if !azalia_widget_enabled(this, nid) {
            // invalid mixerset nid
            return Err(Errno::EINVAL);
        }
        let w = this.wi(nid);
        let mut mask = 0;
        for (i, &c) in w.connections.iter().enumerate() {
            if !azalia_widget_enabled(this, c) {
                continue;
            }
            mask |= 1 << i;
        }
        mc.un.set_mask(mask);
    } else if target == MI_TARGET_SENSESET && mc.type_ == AUDIO_MIXER_SET {
        if nid == this.speaker {
            mc.un.set_mask(this.spkr_muters);
        } else {
            // invalid senseset nid
            return Err(Errno::EINVAL);
        }
    } else if target == MI_TARGET_PLAYVOL {
        volgroup_get(&this.playvols, mc)?;
    } else if target == MI_TARGET_RECVOL {
        volgroup_get(&this.recvols, mc)?;
    } else {
        // internal error: target
        return Err(Errno::EIO);
    }
    Ok(())
}

/// The `MI_TARGET_PLAYVOL` and `MI_TARGET_RECVOL` arms of `azalia_mixer_get`.
fn volgroup_get(group: &Volgroup, mc: &mut MixerCtrl) -> Result<(), Errno> {
    if mc.type_ == AUDIO_MIXER_VALUE {
        let value = mc.un.value_mut();
        value.num_channels = 2;
        value.level[0] = group.vol_l as u8;
        value.level[1] = group.vol_r as u8;
    } else if mc.type_ == AUDIO_MIXER_ENUM {
        mc.un.set_ord(group.mute);
    } else if mc.type_ == AUDIO_MIXER_SET {
        mc.un.set_mask(group.cur);
    } else {
        // invalid master mixer type
        return Err(Errno::EINVAL);
    }
    Ok(())
}

/// Write the gain and mute of one amplifier side: the pair of verbs (get the amplifier,
/// then set it with the gain kept) that `azalia_mixer_set` sends for each channel.
///
/// `side` is `CORB_GAGM_INPUT`/`OUTPUT` | `CORB_GAGM_LEFT`/`RIGHT` | the index, the get
/// parameter; `agm` is the matching `CORB_AGM_*` bits of the set verb.
fn amp_set_mute(this: &Codec, nid: NidT, side: u32, agm: u32, mute: bool) -> Result<(), Errno> {
    // set stereo mute separately to keep each gain value
    let result = azalia_comresp(this, nid, CORB_GET_AMPLIFIER_GAIN_MUTE, side)?;
    let mut value = agm | corb_gagm_gain(result);
    if mute {
        value |= CORB_AGM_MUTE;
    }
    azalia_comresp(this, nid, CORB_SET_AMPLIFIER_GAIN_MUTE, value)?;
    Ok(())
}

/// As [`amp_set_mute`], for the gain: the mute bit is kept and the gain comes from
/// `level`.
fn amp_set_gain(
    this: &Codec,
    nid: NidT,
    target: i32,
    side: u32,
    agm: u32,
    level: u8,
) -> Result<(), Errno> {
    let result = azalia_comresp(this, nid, CORB_GET_AMPLIFIER_GAIN_MUTE, side)?;
    let value = azalia_mixer_to_device_value(this, nid, target, level);
    let value =
        agm | if result & CORB_GAGM_MUTE != 0 {
            CORB_AGM_MUTE
        } else {
            0
        } | (value & CORB_AGM_GAIN_MASK);
    azalia_comresp(this, nid, CORB_SET_AMPLIFIER_GAIN_MUTE, value)?;
    Ok(())
}

/// `azalia_mixer_set`: set control `target` of widget `nid`.
pub fn azalia_mixer_set(
    this: &mut Codec,
    nid: NidT,
    target: i32,
    mc: &MixerCtrl,
) -> Result<(), Errno> {
    if mc.type_ == AUDIO_MIXER_CLASS {
        return Ok(());
    }
    // inamp mute
    else if is_mi_target_inamp(target) && mc.type_ == AUDIO_MIXER_ENUM {
        let index = mi_target_inamp(target) as u32;
        let mute = mc.un.ord() != 0;
        amp_set_mute(
            this,
            nid,
            CORB_GAGM_INPUT | CORB_GAGM_LEFT | index,
            CORB_AGM_INPUT | CORB_AGM_LEFT | (target as u32) << CORB_AGM_INDEX_SHIFT,
            mute,
        )?;
        if widget_channels(this.wi(nid)) == 2 {
            amp_set_mute(
                this,
                nid,
                CORB_GAGM_INPUT | CORB_GAGM_RIGHT | index,
                CORB_AGM_INPUT | CORB_AGM_RIGHT | (target as u32) << CORB_AGM_INDEX_SHIFT,
                mute,
            )?;
        }
    }
    // inamp gain
    else if is_mi_target_inamp(target) && mc.type_ == AUDIO_MIXER_VALUE {
        let value = mc.un.value();
        if value.num_channels < 1 {
            return Err(Errno::EINVAL);
        }
        let index = mi_target_inamp(target) as u32;
        amp_set_gain(
            this,
            nid,
            target,
            CORB_GAGM_INPUT | CORB_GAGM_LEFT | index,
            CORB_AGM_INPUT | CORB_AGM_LEFT | (target as u32) << CORB_AGM_INDEX_SHIFT,
            value.level[0],
        )?;
        if value.num_channels >= 2 && widget_channels(this.wi(nid)) == 2 {
            amp_set_gain(
                this,
                nid,
                target,
                CORB_GAGM_INPUT | CORB_GAGM_RIGHT | index,
                CORB_AGM_INPUT | CORB_AGM_RIGHT | (target as u32) << CORB_AGM_INDEX_SHIFT,
                value.level[1],
            )?;
        }
    }
    // outamp mute
    else if target == MI_TARGET_OUTAMP && mc.type_ == AUDIO_MIXER_ENUM {
        let mute = mc.un.ord() != 0;
        amp_set_mute(
            this,
            nid,
            CORB_GAGM_OUTPUT | CORB_GAGM_LEFT,
            CORB_AGM_OUTPUT | CORB_AGM_LEFT,
            mute,
        )?;
        if widget_channels(this.wi(nid)) == 2 {
            amp_set_mute(
                this,
                nid,
                CORB_GAGM_OUTPUT | CORB_GAGM_RIGHT,
                CORB_AGM_OUTPUT | CORB_AGM_RIGHT,
                mute,
            )?;
        }
    }
    // outamp gain
    else if target == MI_TARGET_OUTAMP && mc.type_ == AUDIO_MIXER_VALUE {
        let value = mc.un.value();
        if value.num_channels < 1 {
            return Err(Errno::EINVAL);
        }
        amp_set_gain(
            this,
            nid,
            target,
            CORB_GAGM_OUTPUT | CORB_GAGM_LEFT,
            CORB_AGM_OUTPUT | CORB_AGM_LEFT,
            value.level[0],
        )?;
        if value.num_channels >= 2 && widget_channels(this.wi(nid)) == 2 {
            amp_set_gain(
                this,
                nid,
                target,
                CORB_GAGM_OUTPUT | CORB_GAGM_RIGHT,
                CORB_AGM_OUTPUT | CORB_AGM_RIGHT,
                value.level[1],
            )?;
        }
    }
    // selection
    else if target == MI_TARGET_CONNLIST {
        let ord = mc.un.ord();
        let w = this.wi(nid);
        if ord < 0
            || ord >= w.nconnections()
            || !azalia_widget_enabled(this, w.connections[ord as usize])
        {
            return Err(Errno::EINVAL);
        }
        azalia_comresp(this, nid, CORB_SET_CONNECTION_SELECT_CONTROL, ord as u32)?;
    }
    // pin I/O
    else if target == MI_TARGET_PINDIR {
        let result = azalia_comresp(this, nid, CORB_GET_PIN_WIDGET_CONTROL, 0)?;

        let ord = mc.un.ord();
        let mut value = result;
        value &= !CORB_PWC_VREF_MASK;
        if ord == 0 {
            value &= !(CORB_PWC_OUTPUT | CORB_PWC_INPUT);
        } else if ord == 1 {
            value &= !CORB_PWC_INPUT;
            value |= CORB_PWC_OUTPUT;
            if this.qrks & AZ_QRK_WID_OVREF50 != 0 {
                value |= CORB_PWC_VREF_50;
            }
        } else {
            value &= !CORB_PWC_OUTPUT;
            value |= CORB_PWC_INPUT;

            if ord == 3 {
                value |= CORB_PWC_VREF_GND;
            }
            if ord == 4 {
                value |= CORB_PWC_VREF_50;
            }
            if ord == 5 {
                value |= CORB_PWC_VREF_80;
            }
            if ord == 6 {
                value |= CORB_PWC_VREF_100;
            }
        }
        azalia_comresp(this, nid, CORB_SET_PIN_WIDGET_CONTROL, value)?;

        // Run the unsolicited response handler for speaker mute
        // since it depends on pin direction.
        if this.sense_pins[..this.nsense_pins as usize].contains(&nid) {
            let _ = azalia_unsol_event(this, AZ_TAG_SPKR);
        }
    }
    // pin headphone-boost
    else if target == MI_TARGET_PINBOOST {
        if mc.un.ord() >= 2 {
            return Err(Errno::EINVAL);
        }
        let mut result = azalia_comresp(this, nid, CORB_GET_PIN_WIDGET_CONTROL, 0)?;
        if mc.un.ord() == 0 {
            result &= !CORB_PWC_HEADPHONE;
        } else {
            result |= CORB_PWC_HEADPHONE;
        }
        azalia_comresp(this, nid, CORB_SET_PIN_WIDGET_CONTROL, result)?;
    }
    // DAC group selection
    else if target == MI_TARGET_DAC {
        if this.running != 0 {
            return Err(Errno::EBUSY);
        }
        if mc.un.ord() >= this.dacs.ngroups {
            return Err(Errno::EINVAL);
        }
        if mc.un.ord() != this.dacs.cur {
            return azalia_codec_construct_format(this, mc.un.ord(), this.adcs.cur);
        } else {
            return Ok(());
        }
    }
    // ADC selection
    else if target == MI_TARGET_ADC {
        if this.running != 0 {
            return Err(Errno::EBUSY);
        }
        if mc.un.ord() >= this.adcs.ngroups {
            return Err(Errno::EINVAL);
        }
        if mc.un.ord() != this.adcs.cur {
            return azalia_codec_construct_format(this, this.dacs.cur, mc.un.ord());
        } else {
            return Ok(());
        }
    }
    // S/PDIF
    else if target == MI_TARGET_SPDIF {
        // The C does not look at the result of this read.
        let mut result = azalia_comresp(this, nid, CORB_GET_DIGITAL_CONTROL, 0).unwrap_or(0);
        result &= CORB_DCC_DIGEN | CORB_DCC_NAUDIO;
        result |= mc.un.mask() as u32 & 0xff & !CORB_DCC_DIGEN;
        azalia_comresp(this, nid, CORB_SET_DIGITAL_CONTROL_L, result)?;
    } else if target == MI_TARGET_SPDIF_CC {
        if mc.un.value().num_channels != 1 {
            return Err(Errno::EINVAL);
        }
        if mc.un.value().level[0] > 127 {
            return Err(Errno::EINVAL);
        }
        azalia_comresp(
            this,
            nid,
            CORB_SET_DIGITAL_CONTROL_H,
            u32::from(mc.un.value().level[0]),
        )?;
    }
    // EAPD
    else if target == MI_TARGET_EAPD {
        if mc.un.ord() >= 2 {
            return Err(Errno::EINVAL);
        }
        let mut result = azalia_comresp(this, nid, CORB_GET_EAPD_BTL_ENABLE, 0)?;
        result &= 0xff;
        if mc.un.ord() == 0 {
            result &= !CORB_EAPD_EAPD;
        } else {
            result |= CORB_EAPD_EAPD;
        }
        azalia_comresp(this, nid, CORB_SET_EAPD_BTL_ENABLE, result)?;
    } else if target == MI_TARGET_PINSENSE {
        // do nothing, control is read only
    } else if target == MI_TARGET_MUTESET && mc.type_ == AUDIO_MIXER_SET {
        if !azalia_widget_enabled(this, nid) {
            // invalid muteset nid
            return Err(Errno::EINVAL);
        }
        let w = this.wi(nid);
        let stereo = widget_channels(w) == 2;
        for (i, &c) in w.connections.iter().enumerate() {
            if !azalia_widget_enabled(this, c) {
                continue;
            }

            // We have to set stereo mute separately
            // to keep each gain value.
            let mute = mc.un.mask() & (1 << i) == 0;
            amp_set_mute(
                this,
                nid,
                CORB_GAGM_INPUT | CORB_GAGM_LEFT | mi_target_inamp(i as i32) as u32,
                CORB_AGM_INPUT | CORB_AGM_LEFT | (i as u32) << CORB_AGM_INDEX_SHIFT,
                mute,
            )?;

            if stereo {
                amp_set_mute(
                    this,
                    nid,
                    CORB_GAGM_INPUT | CORB_GAGM_RIGHT | mi_target_inamp(i as i32) as u32,
                    CORB_AGM_INPUT | CORB_AGM_RIGHT | (i as u32) << CORB_AGM_INDEX_SHIFT,
                    mute,
                )?;
            }
        }
    } else if target == MI_TARGET_MIXERSET && mc.type_ == AUDIO_MIXER_SET {
        // do nothing, control is read only
    } else if target == MI_TARGET_SENSESET && mc.type_ == AUDIO_MIXER_SET {
        if nid == this.speaker {
            this.spkr_muters = mc.un.mask();
            let _ = azalia_unsol_event(this, AZ_TAG_SPKR);
        } else {
            // invalid senseset nid
            return Err(Errno::EINVAL);
        }
    } else if target == MI_TARGET_PLAYVOL {
        let mut mc2 = MixerCtrl::default();

        if mc.type_ == AUDIO_MIXER_VALUE {
            if mc.un.value().num_channels != 2 {
                return Err(Errno::EINVAL);
            }
            this.playvols.vol_l = i32::from(mc.un.value().level[0]);
            this.playvols.vol_r = i32::from(mc.un.value().level[1]);
            for i in 0..this.playvols.nslaves as usize {
                if this.playvols.cur & (1 << i) == 0 {
                    continue;
                }
                let w = this.wi(this.playvols.slaves[i]);
                if cop_ampcap_numsteps(w.outamp_cap) == 0 {
                    continue;
                }
                let (wnid, mutable, channels) = (
                    w.nid,
                    w.outamp_cap & COP_AMPCAP_MUTE != 0,
                    widget_channels(w),
                );

                // don't change volume if muted
                if mutable {
                    mc2.type_ = AUDIO_MIXER_ENUM;
                    let _ = azalia_mixer_get(this, wnid, MI_TARGET_OUTAMP, &mut mc2);
                    if mc2.un.ord() != 0 {
                        continue;
                    }
                }
                mc2.type_ = AUDIO_MIXER_VALUE;
                let value = mc2.un.value_mut();
                value.num_channels = channels;
                value.level[0] = this.playvols.vol_l as u8;
                value.level[1] = this.playvols.vol_r as u8;
                // out slave volume
                azalia_mixer_set(this, wnid, MI_TARGET_OUTAMP, &mc2)?;
            }
        } else if mc.type_ == AUDIO_MIXER_ENUM {
            if mc.un.ord() != 0 && mc.un.ord() != 1 {
                return Err(Errno::EINVAL);
            }
            this.playvols.mute = mc.un.ord();
            for i in 0..this.playvols.nslaves as usize {
                if this.playvols.cur & (1 << i) == 0 {
                    continue;
                }
                let w = this.wi(this.playvols.slaves[i]);
                if w.outamp_cap & COP_AMPCAP_MUTE == 0 {
                    continue;
                }
                let wnid = w.nid;
                if this.spkr_muted == 1
                    && ((this.spkr_mute_method == AZ_SPKR_MUTE_SPKR_MUTE
                        && (wnid == this.speaker || wnid == this.speaker2))
                        || (this.spkr_mute_method == AZ_SPKR_MUTE_DAC_MUTE
                            && wnid == this.spkr_dac))
                {
                    continue;
                }
                mc2.type_ = AUDIO_MIXER_ENUM;
                mc2.un.set_ord(this.playvols.mute);
                // out slave mute
                azalia_mixer_set(this, wnid, MI_TARGET_OUTAMP, &mc2)?;
            }
        } else if mc.type_ == AUDIO_MIXER_SET {
            this.playvols.cur = mc.un.mask() & this.playvols.mask;
        } else {
            // invalid output master mixer type
            return Err(Errno::EINVAL);
        }
    } else if target == MI_TARGET_RECVOL {
        let mut mc2 = MixerCtrl::default();

        if mc.type_ == AUDIO_MIXER_VALUE {
            if mc.un.value().num_channels != 2 {
                return Err(Errno::EINVAL);
            }
            this.recvols.vol_l = i32::from(mc.un.value().level[0]);
            this.recvols.vol_r = i32::from(mc.un.value().level[1]);
            for i in 0..this.recvols.nslaves as usize {
                if this.recvols.cur & (1 << i) == 0 {
                    continue;
                }
                let w = this.wi(this.recvols.slaves[i]);
                let mut tgt = MI_TARGET_OUTAMP;
                let mut cap = w.outamp_cap;
                if w.type_ == COP_AWTYPE_AUDIO_INPUT || w.type_ == COP_AWTYPE_PIN_COMPLEX {
                    tgt = 0;
                    cap = w.inamp_cap;
                }
                if cop_ampcap_numsteps(cap) == 0 {
                    continue;
                }
                let (wnid, channels) = (w.nid, widget_channels(w));
                mc2.type_ = AUDIO_MIXER_VALUE;
                let value = mc2.un.value_mut();
                value.num_channels = channels;
                value.level[0] = this.recvols.vol_l as u8;
                value.level[1] = this.recvols.vol_r as u8;
                // in slave volume
                azalia_mixer_set(this, wnid, tgt, &mc2)?;
            }
        } else if mc.type_ == AUDIO_MIXER_ENUM {
            if mc.un.ord() != 0 && mc.un.ord() != 1 {
                return Err(Errno::EINVAL);
            }
            this.recvols.mute = mc.un.ord();
            for i in 0..this.recvols.nslaves as usize {
                if this.recvols.cur & (1 << i) == 0 {
                    continue;
                }
                let w = this.wi(this.recvols.slaves[i]);
                let mut tgt = MI_TARGET_OUTAMP;
                let mut cap = w.outamp_cap;
                if w.type_ == COP_AWTYPE_AUDIO_INPUT || w.type_ == COP_AWTYPE_PIN_COMPLEX {
                    tgt = 0;
                    cap = w.inamp_cap;
                }
                if cap & COP_AMPCAP_MUTE == 0 {
                    continue;
                }
                let wnid = w.nid;
                mc2.type_ = AUDIO_MIXER_ENUM;
                mc2.un.set_ord(this.recvols.mute);
                // in slave mute
                azalia_mixer_set(this, wnid, tgt, &mc2)?;
            }
        } else if mc.type_ == AUDIO_MIXER_SET {
            this.recvols.cur = mc.un.mask() & this.recvols.mask;
        } else {
            // invalid input master mixer type
            return Err(Errno::EINVAL);
        }
    } else {
        // internal error: target
        return Err(Errno::EIO);
    }
    Ok(())
}

/// The amplifier steps and the offset of the control's amplifier: the C's `steps` and
/// `ctloff` in the two value conversions.
fn amp_steps_ctloff(this: &Codec, nid: NidT, target: i32) -> (u32, u32) {
    if is_mi_target_inamp(target) {
        let cap = this.wi(nid).inamp_cap;
        (cop_ampcap_numsteps(cap), cop_ampcap_ctloff(cap))
    } else if target == MI_TARGET_OUTAMP {
        let cap = this.wi(nid).outamp_cap;
        (cop_ampcap_numsteps(cap), cop_ampcap_ctloff(cap))
    } else {
        // unknown target
        (255, 0)
    }
}

/// `azalia_mixer_from_device_value`: an amplifier setting as a 0..255 mixer level.
pub fn azalia_mixer_from_device_value(this: &Codec, nid: NidT, target: i32, dv: u32) -> u8 {
    let (steps, ctloff) = amp_steps_ctloff(this, nid, target);
    // `dv` is unsigned in the C too: a setting below the offset wraps and reads as the top
    let dv = dv.wrapping_sub(ctloff);
    if dv == 0 || steps == 0 {
        return AUDIO_MIN_GAIN as u8;
    }
    let max_gain = AUDIO_MAX_GAIN as u32 - AUDIO_MAX_GAIN as u32 % steps;
    if dv >= steps {
        return max_gain as u8;
    }
    (dv * max_gain / steps) as u8
}

/// `azalia_mixer_to_device_value`: a 0..255 mixer level as an amplifier setting.
pub fn azalia_mixer_to_device_value(this: &Codec, nid: NidT, target: i32, uv: u8) -> u32 {
    let (steps, ctloff) = amp_steps_ctloff(this, nid, target);
    if i32::from(uv) <= AUDIO_MIN_GAIN || steps == 0 {
        return ctloff;
    }
    let max_gain = AUDIO_MAX_GAIN as u32 - AUDIO_MAX_GAIN as u32 % steps;
    if u32::from(uv) >= max_gain {
        return steps + ctloff;
    }
    u32::from(uv) * steps / max_gain + ctloff
}

/// `azalia_gpio_unmute`: drive GPIO `pin` of the audio function high.
pub fn azalia_gpio_unmute(this: &Codec, pin: i32) -> Result<(), Errno> {
    let af = this.audiofunc;
    // As in C, a failed read leaves the value 0 (the C's would be uninitialised).
    let mut data = azalia_comresp(this, af, CORB_GET_GPIO_DATA, 0).unwrap_or(0);
    let mut mask = azalia_comresp(this, af, CORB_GET_GPIO_ENABLE_MASK, 0).unwrap_or(0);
    let mut dir = azalia_comresp(this, af, CORB_GET_GPIO_DIRECTION, 0).unwrap_or(0);

    data |= 1 << pin;
    mask |= 1 << pin;
    dir |= 1 << pin;

    let _ = azalia_comresp(this, af, CORB_SET_GPIO_ENABLE_MASK, mask);
    let _ = azalia_comresp(this, af, CORB_SET_GPIO_DIRECTION, dir);
    delay(1000);
    let _ = azalia_comresp(this, af, CORB_SET_GPIO_DATA, data);

    Ok(())
}

/// `azalia_ampcap_ov`: override an amplifier's capabilities.
pub fn azalia_ampcap_ov(
    w: &mut Widget,
    type_: u32,
    offset: u32,
    steps: u32,
    size: u32,
    ctloff: u32,
    mute: bool,
) {
    let cap = (offset & 0x7f)
        | ((steps & 0x7f) << 8)
        | ((size & 0x7f) << 16)
        | ((ctloff & 0x7f) << 24)
        | if mute { COP_AMPCAP_MUTE } else { 0 };

    if type_ == COP_OUTPUT_AMPCAP {
        w.outamp_cap = cap;
    } else if type_ == COP_INPUT_AMPCAP {
        w.inamp_cap = cap;
    }
}

/// `azalia_pin_config_ov`: override the device or port field of a pin's configuration.
pub fn azalia_pin_config_ov(w: &mut Widget, mask: u32, val: u32) {
    let (bits, offset) = match mask {
        CORB_CD_DEVICE_MASK => (CORB_CD_DEVICE_BITS, CORB_CD_DEVICE_OFFSET),
        CORB_CD_PORT_MASK => (CORB_CD_PORT_BITS, CORB_CD_PORT_OFFSET),
        _ => return,
    };
    let val = val & bits;
    let pin = w.d.pin_mut();
    pin.config &= !mask;
    pin.config |= val << offset;
    if mask == CORB_CD_DEVICE_MASK {
        pin.device = val;
    }
}

/// `azalia_codec_gpio_quirks`.
pub fn azalia_codec_gpio_quirks(this: &mut Codec) -> Result<(), Errno> {
    if this.qrks & AZ_QRK_GPIO_POL_0 != 0 {
        let _ = azalia_comresp(this, this.audiofunc, CORB_SET_GPIO_POLARITY, 0);
    }
    for (q, pin) in [
        (AZ_QRK_GPIO_UNMUTE_0, 0),
        (AZ_QRK_GPIO_UNMUTE_1, 1),
        (AZ_QRK_GPIO_UNMUTE_2, 2),
        (AZ_QRK_GPIO_UNMUTE_3, 3),
    ] {
        if this.qrks & q != 0 {
            let _ = azalia_gpio_unmute(this, pin);
        }
    }

    Ok(())
}

/// `azalia_codec_widget_quirks`: the per-widget fixes of the quirky codecs.
pub fn azalia_codec_widget_quirks(this: &mut Codec, nid: NidT) -> Result<(), Errno> {
    let qrks = this.qrks;
    let w = this.wi_mut(nid);

    if qrks & AZ_QRK_WID_BEEP_1D != 0 && nid == 0x1d && !w.enable {
        azalia_pin_config_ov(w, CORB_CD_DEVICE_MASK, CORB_CD_BEEP);
        azalia_pin_config_ov(w, CORB_CD_PORT_MASK, CORB_CD_FIXED);
        w.widgetcap |= COP_AWCAP_STEREO;
        w.enable = true;
    }

    if qrks & AZ_QRK_WID_TPDOCK1 != 0 && nid == 0x19 {
        // Thinkpad x230/t430 style dock microphone
        w.d.pin_mut().config = 0x23a11040;
        w.enable = true;
    }

    if qrks & AZ_QRK_WID_TPDOCK1 != 0 && nid == 0x1b {
        // Thinkpad x230/t430 style dock headphone
        w.d.pin_mut().config = 0x2121103f;
        w.enable = true;
    }

    if qrks & AZ_QRK_WID_TPDOCK2 != 0 && nid == 0x16 {
        // Thinkpad x240/t440 style dock headphone
        w.d.pin_mut().config = 0x21211010;
        w.enable = true;
    }

    if qrks & AZ_QRK_WID_TPDOCK2 != 0 && nid == 0x19 {
        // Thinkpad x240/t440 style dock microphone
        w.d.pin_mut().config = 0x21a11010;
        w.enable = true;
    }

    if qrks & AZ_QRK_WID_TPDOCK3 != 0 && nid == 0x1a {
        // Thinkpad x220/t420 style dock microphone
        w.d.pin_mut().config = 0x21a190f0;
        w.enable = true;
    }

    if qrks & AZ_QRK_WID_TPDOCK3 != 0 && nid == 0x1c {
        // Thinkpad x220/t420 style dock headphone
        w.d.pin_mut().config = 0x212140ff;
        w.enable = true;
    }

    if qrks & AZ_QRK_WID_CDIN_1C != 0 && nid == 0x1c && !w.enable && w.d.pin().device == CORB_CD_CD
    {
        azalia_pin_config_ov(w, CORB_CD_PORT_MASK, CORB_CD_FIXED);
        w.widgetcap |= COP_AWCAP_STEREO;
        w.enable = true;
    }

    if qrks & AZ_QRK_WID_AD1981_OAMP != 0 && matches!(nid, 0x05 | 0x06 | 0x07 | 0x09 | 0x18) {
        azalia_ampcap_ov(w, COP_OUTPUT_AMPCAP, 31, 33, 6, 30, true);
    }

    if qrks & AZ_QRK_WID_CLOSE_PCBEEP != 0 && nid == 0x20 {
        // Close PC beep passthrough to avoid headphone noise
        let _ = azalia_comresp(this, nid, CORB_SET_COEFFICIENT_INDEX, 0x36);
        let _ = azalia_comresp(this, nid, CORB_SET_PROCESSING_COEFFICIENT, 0x57d7);
    }

    Ok(())
}

/// `atmos_init` of `azalia_codec_init_dolby_atmos`: (nid, verb, payload) triples.
static ATMOS_INIT: [u16; 36] = [
    0x06, 0x73e, 0x00, 0x06, 0x73e, 0x80, 0x20, 0x500, 0x26, 0x20, 0x4f0, 0x00, 0x20, 0x500, 0x22,
    0x20, 0x400, 0x31, 0x20, 0x500, 0x23, 0x20, 0x400, 0x0b, 0x20, 0x500, 0x25, 0x20, 0x400, 0x00,
    0x20, 0x500, 0x26, 0x20, 0x4b0, 0x10,
];

/// `atmos_v23_v25` of `azalia_codec_init_dolby_atmos`: `(v23, v25)` pairs.
static ATMOS_V23_V25: [(u8, u8); 36] = [
    (0x0c, 0x00),
    (0x0d, 0x00),
    (0x0e, 0x00),
    (0x0f, 0x00),
    (0x10, 0x00),
    (0x1a, 0x40),
    (0x1b, 0x82),
    (0x1c, 0x00),
    (0x1d, 0x00),
    (0x1e, 0x00),
    (0x1f, 0x00),
    (0x20, 0xc2),
    (0x21, 0xc8),
    (0x22, 0x26),
    (0x23, 0x24),
    (0x27, 0xff),
    (0x28, 0xff),
    (0x29, 0xff),
    (0x2a, 0x8f),
    (0x2b, 0x02),
    (0x2c, 0x48),
    (0x2d, 0x34),
    (0x2e, 0x00),
    (0x2f, 0x00),
    (0x30, 0x00),
    (0x31, 0x00),
    (0x32, 0x00),
    (0x33, 0x00),
    (0x34, 0x00),
    (0x35, 0x01),
    (0x36, 0x93),
    (0x37, 0x0c),
    (0x38, 0x00),
    (0x39, 0x00),
    (0x3a, 0xf8),
    (0x38, 0x80),
];

/// `azalia_codec_init_dolby_atmos`: magic init sequence to make the right speaker work
/// (reverse-engineered). Stops at the first failed command.
pub fn azalia_codec_init_dolby_atmos(this: &Codec) {
    let cmd = |nid: u16, verb: u16, val: u16| {
        azalia_comresp(this, NidT::from(nid), u32::from(verb), u32::from(val))
    };

    for &[nid, verb, val] in ATMOS_INIT.as_chunks::<3>().0 {
        if cmd(nid, verb, val).is_err() {
            return;
        }
    }

    for (i, &(v23, v25)) in ATMOS_V23_V25.iter().enumerate() {
        let step = || -> Result<u32, Errno> {
            cmd(0x06, 0x73e, 0x00)?;
            cmd(0x20, 0x500, 0x26)?;
            cmd(0x20, 0x4b0, 0x00)?;
            if i == 0 {
                cmd(0x21, 0xf09, 0x00)?;
            }
            if i != 20 {
                cmd(0x06, 0x73e, 0x80)?;
            }

            cmd(0x20, 0x500, 0x26)?;
            cmd(0x20, 0x4f0, 0x00)?;
            cmd(0x20, 0x500, 0x23)?;

            cmd(0x20, 0x400, u16::from(v23))?;

            if v23 != 0x1e {
                cmd(0x20, 0x500, 0x25)?;
                cmd(0x20, 0x400, u16::from(v25))?;
            }

            cmd(0x20, 0x500, 0x26)?;
            cmd(0x20, 0x4b0, 0x10)
        };
        if step().is_err() {
            return;
        }
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
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
}
/* </TESTS> */
