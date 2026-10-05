/*	$OpenBSD: azalia_codec.c,v 1.189 2022/09/08 01:35:39 jsg Exp $	*/
/*	$NetBSD: azalia_codec.c,v 1.8 2006/05/10 11:17:27 kent Exp $	*/
/* <LICENSES> */
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

//! The generic codec support of azalia(4): codec names and quirks, converter groups, path
//! finding between widgets, unsolicited events, and (not yet) the mixer.
//!
//! Upstream: sys/dev/pci/azalia_codec.c @ 3ce1f3f79392
//!
//! Partial (status `wip`): the functions `azalia.c` calls on the path QEMU's `hda-output`
//! codec takes are ported; the generic mixer is the next port of this file.
//!
//! ## Deviations
//! - Not ported yet: `azalia_mixer_init` (and its helpers `azalia_mixer_default`,
//!   `azalia_mixer_ensure_capacity`, `azalia_mixer_fix_indexes`, `azalia_devinfo_offon`),
//!   `azalia_mixer_get`, `azalia_mixer_set`, `azalia_mixer_from_device_value` and
//!   `azalia_mixer_to_device_value`. [`azalia_mixer_init`] reports the gap with `unported!`
//!   and returns success with no mixer controls, so the codec attaches and plays (its amplifiers
//!   keep their power-on settings, and `azalia_codec_enable_unsol`, which the C calls at the
//!   end of `azalia_mixer_init`, is not reached); [`azalia_mixer_get`] and
//!   [`azalia_mixer_set`] return `ENOSYS` through `unported!` (nothing calls them while
//!   there are no controls).
//! - The functions take the codec as `&Codec` or `&mut Codec`, and widgets by their nid;
//!   `azalia_pin_config_ov` and `azalia_ampcap_ov` take the widget.
//! - [`azalia_widget_enabled`] returns `bool`.
//! - The C's `int` results that are always 0 are `Result<(), Errno>` like the others.

use alloc::vec::Vec;

use crate::dev::pci::azalia::{
    AZ_QRK_DOLBY_ATMOS, AZ_QRK_GPIO_POL_0, AZ_QRK_GPIO_UNMUTE_0, AZ_QRK_GPIO_UNMUTE_1,
    AZ_QRK_GPIO_UNMUTE_2, AZ_QRK_GPIO_UNMUTE_3, AZ_QRK_NONE, AZ_QRK_ROUTE_SPKR2_DAC,
    AZ_QRK_WID_AD1981_OAMP, AZ_QRK_WID_BEEP_1D, AZ_QRK_WID_CDIN_1C, AZ_QRK_WID_CLOSE_PCBEEP,
    AZ_QRK_WID_OVREF50, AZ_QRK_WID_TPDOCK1, AZ_QRK_WID_TPDOCK2, AZ_QRK_WID_TPDOCK3,
    AZ_SPKR_MUTE_DAC_MUTE, AZ_SPKR_MUTE_SPKR_DIR, AZ_SPKR_MUTE_SPKR_MUTE, AZ_TAG_PLAYVOL,
    AZ_TAG_SPKR, COP_AMPCAP_MUTE, COP_AWCAP_DIGITAL, COP_AWCAP_OUTAMP, COP_AWCAP_STEREO,
    COP_AWTYPE_AUDIO_INPUT, COP_AWTYPE_AUDIO_OUTPUT, COP_AWTYPE_BEEP_GENERATOR,
    COP_AWTYPE_PIN_COMPLEX, COP_INPUT_AMPCAP, COP_OUTPUT_AMPCAP, COP_PINCAP_INPUT,
    COP_PINCAP_OUTPUT, CORB_CD_BEEP, CORB_CD_CD, CORB_CD_DEVICE_BITS, CORB_CD_DEVICE_MASK,
    CORB_CD_DEVICE_OFFSET, CORB_CD_FIXED, CORB_CD_PORT_BITS, CORB_CD_PORT_MASK,
    CORB_CD_PORT_OFFSET, CORB_GET_GPIO_DATA, CORB_GET_GPIO_DIRECTION, CORB_GET_GPIO_ENABLE_MASK,
    CORB_GET_PIN_SENSE, CORB_GET_PIN_WIDGET_CONTROL, CORB_GET_VOLUME_KNOB, CORB_PS_PRESENCE,
    CORB_PWC_OUTPUT, CORB_SET_COEFFICIENT_INDEX, CORB_SET_GPIO_DATA, CORB_SET_GPIO_DIRECTION,
    CORB_SET_GPIO_ENABLE_MASK, CORB_SET_GPIO_POLARITY, CORB_SET_PROCESSING_COEFFICIENT,
    CORB_SET_UNSOLICITED_RESPONSE, CORB_SET_VOLUME_KNOB, CORB_UNSOL_ENABLE, CORB_VKNOB_DIRECT,
    Codec, Convgroupset, HDA_MAX_CHANNELS, IoPin, MI_TARGET_OUTAMP, MI_TARGET_PINDIR,
    MI_TARGET_PLAYVOL, NidT, Widget, azalia_comresp, cop_vkcap_numsteps, corb_unsol_tag,
    corb_vknob_volume, valid_widget_nid,
};
use crate::dev::pci::pcidevs::{PCI_VENDOR_DELL, PCI_VENDOR_HP};
use crate::dev::pci::pcireg::pci_vendor;
use crate::machine::cpu::delay;
use crate::sys::audioio::{AUDIO_MAX_GAIN, AUDIO_MIXER_ENUM, AUDIO_MIXER_VALUE, MixerCtrl};
use crate::sys::errno::Errno;
use crate::unported;

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

/// `azalia_mixer_init`: the codec's mixer controls. Not ported yet (see the module's
/// deviations): the gap is reported and the codec gets no controls.
pub fn azalia_mixer_init(_this: &mut Codec) -> Result<(), Errno> {
    let _ = unported!("azalia_mixer_init (azalia_codec.c)");
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

/// `azalia_mixer_get`: the value of control `target` of widget `nid` (`mc->type` set by
/// the caller). Not ported yet: `ENOSYS`.
pub fn azalia_mixer_get(
    _this: &Codec,
    _nid: NidT,
    _target: i32,
    _mc: &mut MixerCtrl,
) -> Result<(), Errno> {
    Err(unported!("azalia_mixer_get (azalia_codec.c)"))
}

/// `azalia_mixer_set`: set control `target` of widget `nid`. Not ported yet: `ENOSYS`.
pub fn azalia_mixer_set(
    _this: &mut Codec,
    _nid: NidT,
    _target: i32,
    _mc: &MixerCtrl,
) -> Result<(), Errno> {
    Err(unported!("azalia_mixer_set (azalia_codec.c)"))
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

#[cfg(test)]
mod tests;
