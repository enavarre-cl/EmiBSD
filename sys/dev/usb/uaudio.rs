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

/*
 * Copyright (c) 2018 Alexandre Ratchov <alex@caoua.org>
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
//! uaudio(4): the USB Audio Class driver, for both versions of the class (UAC 1.0 and 2.0),
//! which are not compatible with each other but close enough for one driver.
//!
//! Upstream: sys/dev/usb/uaudio.c @ 3ce1f3f79392
//!
//! `uaudio_match` takes an audio streaming (AS) interface; `uaudio_attach` then parses the
//! whole configuration descriptor (`uaudio_process_conf`): the audio control (AC) interface's
//! circuit of units (terminals, mixers, selectors, feature units, effects, processors, clock
//! sources and selectors), from which the mixer(4) controls and their names are derived, and
//! the AS interfaces' alternate settings (format, channels, rates, data and feedback
//! endpoints), combined into the play/record parameter sets the device can run together. It
//! claims the interfaces it uses and attaches audio(4) through `audio_attach_mi` with
//! [`UAUDIO_HW_IF`].
//!
//! Streaming is isochronous: each direction keeps up to `UAUDIO_NXFERS_MAX` transfers in
//! flight, each a little larger than one audio block (`safe_blksz`), with per-frame sizes
//! computed from a fixed-point samples-per-frame (`UAUDIO_SPF_DIV`) that the device's
//! feedback endpoint (asynchronous play), or the record stream (full duplex without
//! feedback, `uaudio_adjspf`), adjusts. Play data is copied from audio(4)'s ring into the
//! transfer buffers as audio(4) produces it (`copy_output`, `underrun`); recorded frames are
//! copied into the ring as they complete. Each completed block calls audio(4)'s interrupt
//! call-back under `AUDIO_LOCK`.
//!
//! The parser is written against the [`UaudioDev`] trait (class requests on the default
//! pipe, the interface list, the device's name), which the softc implements over its
//! `usbd_device` and the host tests implement over tables, so the descriptor blobs of real
//! devices are parsed on the host.
//!
//! ## Deviations
//! - The parsed configuration (the unit circuit, the names, the alternate settings and the
//!   parameter sets, with the class version, `instnum`, `nin`/`nout`, `ctl_ifnum` and the
//!   UAC 2.0 clocks) is a [`UaudioConf`] allocated by `uaudio_attach` and freed by
//!   `uaudio_detach`, not members of the softc: the softc is zeroed memory, which a `Vec`
//!   is not. The C's linked lists keep their order and their sharing: units are an arena
//!   (`Vec`) whose `unit_next`, `src_next`, `dst_next`, `src_list`, `dst_list` and `clock`
//!   links are indices, so a unit that is the source of several destinations is linked as
//!   in C (`src_next` and `dst_next` belong to the unit, as in C); the mixer entries,
//!   ranges, alternate settings and parameter sets, singly linked lists the C only walks
//!   forward, are vectors in list order (sorted insertion where the C inserts sorted);
//!   `sc->params`, `palt` and `ralt` are indices.
//! - The descriptor blob (`struct uaudio_blob`, two pointers into one buffer) is a slice and
//!   two offsets. `uaudio_getnum` and `uaudio_getdesc` return `Option` (the C's 0 is
//!   `None`), the other parsing functions `bool` (the C's int truth value).
//! - The streams and the run-time state are `Cell`s in the softc (zero-valid), so the USB
//!   call-backs, audio(4)'s methods and audio(4)'s interrupt call-back, which re-enters the
//!   driver through `uaudio_underrun`, share them as the C does under the kernel lock and
//!   `splusb`. The transfer buffers and the frame-size arrays are raw pointers (DMA memory
//!   and the array the controller writes the actual frame lengths into), read and written
//!   through bounds-checked slices; each transfer keeps the lengths it was allocated with.
//! - A unit name is a 16-byte array (`UAUDIO_NAMEMAX`) formatted with `snprintf`. The name
//!   templates the C keeps by pointer are copies: `uaudio_setname_middle` passes a stack
//!   buffer that the C's `names` list keeps pointing to after it returns (a dangling
//!   pointer later read by `strcmp`); here the list compares against what was formatted.
//! - The features without a name (delay, underflow and overflow controls, `NULL` in the
//!   C's table) compare as an empty string where the C would call `strcmp` on `NULL` (a
//!   fault); their mixer labels print `(null)`, as `printf(9)` prints a `NULL` `%s`.
//! - A class version that is neither 1.0 nor 2.0 leaves the C's locals unset in
//!   `uaudio_req_ranges` (`count`, the blob), the feature unit's control size and the format
//!   descriptor's `bps`/`bits` (undefined behaviour); those parses fail here instead.
//! - `uaudio_alt_getrates` follows a UAC 2.0 clock chain that may end in `NULL` (no clock,
//!   a selector without a current source); the C dereferences it, this returns no rates.
//! - `uaudio_getnum` shifts with wrapping shifts (the count modulo 32, as the C compiles on
//!   amd64 and arm64) for a field longer than 4 bytes, where the C's shift is undefined.
//!   The clock selector's `char val` counts down as an unsigned byte (arm64's `char`).
//! - A mixer value control with more than eight channels uses only `level[0..8]` (the
//!   `mixer_ctrl` array); the C reads and writes past it.
//! - The mixer-to-stream arithmetic keeps the C's integer types: the unsigned and signed
//!   mixes (`ring_offs`, `diff_nsamp`, `uaudio_adjspf`'s `diff`) wrap explicitly.
//! - `uaudio_stream_open`'s sizing (samples per frame, `safe_blksz`, `nframes_max`,
//!   `maxpkt`, `nxfers` and the two size checks) is the helper `uaudio_stream_calc`, so the
//!   host tests run it; `uaudio_stream_open` calls it first, as the C computes it first.
//! - `uaudio_detach` waits for the device's references only when the attach got as far as
//!   recording the device (the C calls `usbd_ref_wait(NULL)` after an attach that found no
//!   configuration descriptor).
//! - `UAUDIO_DEBUG` is not configured: the `DPRINTF` messages, the debug printers
//!   (`uaudio_isoname` .. `uaudio_conf_print`), the mixer unit's knob dump and the `0xd0`
//!   fills of the transfer buffers are not carried over. The `DIAGNOSTIC` checks are, behind
//!   the `diagnostic` feature.

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::cell::Cell;
use core::cmp::Ordering;
use core::ffi::c_void;
use core::num::NonZeroUsize;
use core::ptr::{self, NonNull};
use core::slice;

use crate::dev::audio::{AUDIO_LOCK, audio_attach_mi};
use crate::dev::audio_if::{AUDIO_ENCODING_SLINEAR_LE, AudioHwIf, AudioIntr, AudioParams};
use crate::dev::usb::usb::{
    UDESC_CS_ENDPOINT, UDESC_CS_INTERFACE, UDESC_ENDPOINT, UDESC_INTERFACE, UE_DIR_IN,
    UE_ISOCHRONOUS, UICLASS_AUDIO, UISUBCLASS_AUDIOCONTROL, UISUBCLASS_AUDIOSTREAM, USB_SPEED_FULL,
    USB_SPEED_HIGH, USB_SPEED_LOW, USB_SPEED_SUPER, USBD_SHORT_XFER_OK, UT_READ_CLASS_INTERFACE,
    UT_WRITE_CLASS_ENDPOINT, UT_WRITE_CLASS_INTERFACE, UsbDeviceRequest, ue_get_dir,
    ue_get_iso_type, ue_get_size, ue_get_xfertype, ugetw, usetw,
};
use crate::dev::usb::usb_mem::usb_syncmem;
use crate::dev::usb::usbdi::{
    UMATCH_NONE, UMATCH_VENDOR_PRODUCT_CONF_IFACE, USBD_IN_PROGRESS, USBD_IOERROR, USBD_NO_COPY,
    USBD_NORMAL_COMPLETION, UsbAttachArg, UsbdStatus, splusb, usbd_alloc_buffer, usbd_alloc_xfer,
    usbd_claim_iface, usbd_close_pipe, usbd_device2interface_handle, usbd_do_request,
    usbd_free_xfer, usbd_get_config_descriptor, usbd_get_interface_descriptor,
    usbd_get_xfer_status, usbd_iface_claimed, usbd_is_dying, usbd_open_pipe, usbd_ref_decr,
    usbd_ref_incr, usbd_ref_wait, usbd_set_interface, usbd_setup_isoc_xfer, usbd_transfer,
};
use crate::dev::usb::usbdivar::{UsbdDevice, UsbdPipe, UsbdXfer};
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_malloc::{free, mallocarray};
use crate::kern::subr_autoconf::config_detach_children;
use crate::kern::subr_prf::{panic, printf, snprintf};
use crate::machine::bus::BUS_DMASYNC_PREWRITE;
use crate::machine::intr::splx;
use crate::sys::audioio::{
    AUDIO_MIXER_CLASS, AUDIO_MIXER_ENUM, AUDIO_MIXER_VALUE, AUMODE_PLAY, AUMODE_RECORD,
    AudioCinputs, AudioCoutputs, MAX_AUDIO_DEV_LEN, MixerCtrl, MixerDevinfo,
};
use crate::sys::device::{CfMatch, Cfattach, Cfdriver, DV_DULL, Device, Softc};
use crate::sys::errno::Errno;
use crate::sys::fcntl::{FREAD, FWRITE};
use crate::sys::malloc::{M_USBDEV, M_WAITOK};
use libkern::strlcpy;

/// `UE_ISO_USAGE`: isochronous endpoint usage (XXX: these belong to dev/usb/usb.h).
pub const UE_ISO_USAGE: u32 = 0x30;
/// `UE_ISO_USAGE_DATA`.
pub const UE_ISO_USAGE_DATA: u32 = 0x00;
/// `UE_ISO_USAGE_FEEDBACK`.
pub const UE_ISO_USAGE_FEEDBACK: u32 = 0x10;
/// `UE_ISO_USAGE_IMPL`: implicit feedback.
pub const UE_ISO_USAGE_IMPL: u32 = 0x20;

/// `UE_GET_ISO_USAGE(a)`.
pub const fn ue_get_iso_usage(a: u32) -> u32 {
    a & UE_ISO_USAGE
}

/// `UAUDIO_NAMEMAX`: max length of unit names, NUL included.
pub const UAUDIO_NAMEMAX: usize = MAX_AUDIO_DEV_LEN;

/// `UAUDIO_V1`: USB audio class version 1.0.
pub const UAUDIO_V1: u32 = 0x100;
/// `UAUDIO_V2`: USB audio class version 2.0.
pub const UAUDIO_V2: u32 = 0x200;

/// `UAUDIO_AC_HEADER`: AC class-specific descriptor interface sub-type.
pub const UAUDIO_AC_HEADER: u32 = 0x1;
/// `UAUDIO_AC_INPUT`.
pub const UAUDIO_AC_INPUT: u32 = 0x2;
/// `UAUDIO_AC_OUTPUT`.
pub const UAUDIO_AC_OUTPUT: u32 = 0x3;
/// `UAUDIO_AC_MIXER`.
pub const UAUDIO_AC_MIXER: u32 = 0x4;
/// `UAUDIO_AC_SELECTOR`.
pub const UAUDIO_AC_SELECTOR: u32 = 0x5;
/// `UAUDIO_AC_FEATURE`.
pub const UAUDIO_AC_FEATURE: u32 = 0x6;
/// `UAUDIO_AC_EFFECT`.
pub const UAUDIO_AC_EFFECT: u32 = 0x7;
/// `UAUDIO_AC_PROCESSING`.
pub const UAUDIO_AC_PROCESSING: u32 = 0x8;
/// `UAUDIO_AC_EXTENSION`.
pub const UAUDIO_AC_EXTENSION: u32 = 0x9;
/// `UAUDIO_AC_CLKSRC`.
pub const UAUDIO_AC_CLKSRC: u32 = 0xa;
/// `UAUDIO_AC_CLKSEL`.
pub const UAUDIO_AC_CLKSEL: u32 = 0xb;
/// `UAUDIO_AC_CLKMULT`.
pub const UAUDIO_AC_CLKMULT: u32 = 0xc;
/// `UAUDIO_AC_RATECONV`.
pub const UAUDIO_AC_RATECONV: u32 = 0xd;

/// `UAUDIO_CLKSRC_FREQCTL`: AC class-specific CLKSRC control.
pub const UAUDIO_CLKSRC_FREQCTL: u32 = 0x2;

/// `UAUDIO_AS_GENERAL`: AS class-specific interface sub-type.
pub const UAUDIO_AS_GENERAL: u32 = 0x1;
/// `UAUDIO_AS_FORMAT`.
pub const UAUDIO_AS_FORMAT: u32 = 0x2;

/// `UAUDIO_AS_EP_GENERAL`: AS class-specific endpoint sub-type.
pub const UAUDIO_AS_EP_GENERAL: u32 = 0x1;

/// `UAUDIO_EP_GENERAL`: AS class-specific endpoint sub-type.
pub const UAUDIO_EP_GENERAL: u32 = 0x1;

/// `UAUDIO_V1_FMT_PCM`: UAC v1 formats, `wFormatTag` is an enum.
pub const UAUDIO_V1_FMT_PCM: u32 = 0x1;
/// `UAUDIO_V1_FMT_PCM8`.
pub const UAUDIO_V1_FMT_PCM8: u32 = 0x2;
/// `UAUDIO_V1_FMT_FLOAT`.
pub const UAUDIO_V1_FMT_FLOAT: u32 = 0x3;
/// `UAUDIO_V1_FMT_ALAW`.
pub const UAUDIO_V1_FMT_ALAW: u32 = 0x4;
/// `UAUDIO_V1_FMT_MULAW`.
pub const UAUDIO_V1_FMT_MULAW: u32 = 0x5;

/// `UAUDIO_V2_FMT_PCM`: UAC v2 formats, `bmFormats` is a bitmap.
pub const UAUDIO_V2_FMT_PCM: u32 = 0x01;
/// `UAUDIO_V2_FMT_PCM8`.
pub const UAUDIO_V2_FMT_PCM8: u32 = 0x02;
/// `UAUDIO_V2_FMT_FLOAT`.
pub const UAUDIO_V2_FMT_FLOAT: u32 = 0x04;
/// `UAUDIO_V2_FMT_ALAW`.
pub const UAUDIO_V2_FMT_ALAW: u32 = 0x08;
/// `UAUDIO_V2_FMT_MULAW`.
pub const UAUDIO_V2_FMT_MULAW: u32 = 0x10;

/// `UAUDIO_V1_REQ_SET_CUR`: AC requests.
pub const UAUDIO_V1_REQ_SET_CUR: u8 = 0x01;
/// `UAUDIO_V1_REQ_SET_MIN`.
pub const UAUDIO_V1_REQ_SET_MIN: u8 = 0x02;
/// `UAUDIO_V1_REQ_SET_MAX`.
pub const UAUDIO_V1_REQ_SET_MAX: u8 = 0x03;
/// `UAUDIO_V1_REQ_SET_RES`.
pub const UAUDIO_V1_REQ_SET_RES: u8 = 0x04;
/// `UAUDIO_V1_REQ_GET_CUR`.
pub const UAUDIO_V1_REQ_GET_CUR: u8 = 0x81;
/// `UAUDIO_V1_REQ_GET_MIN`.
pub const UAUDIO_V1_REQ_GET_MIN: u8 = 0x82;
/// `UAUDIO_V1_REQ_GET_MAX`.
pub const UAUDIO_V1_REQ_GET_MAX: u8 = 0x83;
/// `UAUDIO_V1_REQ_GET_RES`.
pub const UAUDIO_V1_REQ_GET_RES: u8 = 0x84;
/// `UAUDIO_V2_REQ_CUR`.
pub const UAUDIO_V2_REQ_CUR: u8 = 1;
/// `UAUDIO_V2_REQ_RANGES`.
pub const UAUDIO_V2_REQ_RANGES: u8 = 2;

/// `UAUDIO_V2_REQSEL_CLKFREQ`: AC request "selector control".
pub const UAUDIO_V2_REQSEL_CLKFREQ: u32 = 1;
/// `UAUDIO_V2_REQSEL_CLKSEL`.
pub const UAUDIO_V2_REQSEL_CLKSEL: u32 = 1;

/// `UAUDIO_EP_FREQCTL`: AS class-specific endpoint attribute.
pub const UAUDIO_EP_FREQCTL: u32 = 0x01;

/// `UAUDIO_REQSEL_MUTE`: AC feature control selectors (aka `wValue` in the request).
pub const UAUDIO_REQSEL_MUTE: u32 = 0x01;
/// `UAUDIO_REQSEL_VOLUME`.
pub const UAUDIO_REQSEL_VOLUME: u32 = 0x02;
/// `UAUDIO_REQSEL_BASS`.
pub const UAUDIO_REQSEL_BASS: u32 = 0x03;
/// `UAUDIO_REQSEL_MID`.
pub const UAUDIO_REQSEL_MID: u32 = 0x04;
/// `UAUDIO_REQSEL_TREBLE`.
pub const UAUDIO_REQSEL_TREBLE: u32 = 0x05;
/// `UAUDIO_REQSEL_EQ`.
pub const UAUDIO_REQSEL_EQ: u32 = 0x06;
/// `UAUDIO_REQSEL_AGC`.
pub const UAUDIO_REQSEL_AGC: u32 = 0x07;
/// `UAUDIO_REQSEL_DELAY`.
pub const UAUDIO_REQSEL_DELAY: u32 = 0x08;
/// `UAUDIO_REQSEL_BASSBOOST`.
pub const UAUDIO_REQSEL_BASSBOOST: u32 = 0x09;
/// `UAUDIO_REQSEL_LOUDNESS`.
pub const UAUDIO_REQSEL_LOUDNESS: u32 = 0x0a;
/// `UAUDIO_REQSEL_GAIN`.
pub const UAUDIO_REQSEL_GAIN: u32 = 0x0b;
/// `UAUDIO_REQSEL_GAINPAD`.
pub const UAUDIO_REQSEL_GAINPAD: u32 = 0x0c;
/// `UAUDIO_REQSEL_PHASEINV`.
pub const UAUDIO_REQSEL_PHASEINV: u32 = 0x0d;

/// `UAUDIO_REQSEL_RATE`: endpoint (UAC v1) or clock-source unit (UAC v2) sample rate
/// control.
pub const UAUDIO_REQSEL_RATE: u32 = 0x01;

/// `UAUDIO_SPF_DIV`: the denominator of the fixed-point samples per frame.
///
/// Samples-per-frame are fractions. UAC v2.0 requires the denominator to be a multiple of
/// 2^16, as used in the sync pipe; to represent the samples per frame of every supported
/// rate exactly, `rate / 1000` must be representable, which 80 does. This is the least
/// common multiple of both.
pub const UAUDIO_SPF_DIV: u32 = 327_680;

/// `UAUDIO_NAME_PLAY`: the name of the DAC unit.
pub const UAUDIO_NAME_PLAY: &str = "dac";
/// `UAUDIO_NAME_REC`: the name of the ADC unit.
pub const UAUDIO_NAME_REC: &str = "record";

/// `UAUDIO_CLASS_OUT`: the mixer class of output controls.
pub const UAUDIO_CLASS_OUT: i32 = 0;
/// `UAUDIO_CLASS_IN`: the mixer class of input controls.
pub const UAUDIO_CLASS_IN: i32 = 1;
/// `UAUDIO_CLASS_COUNT`.
pub const UAUDIO_CLASS_COUNT: i32 = 2;

/// `UAUDIO_MIX_SW`: an on/off mixer control.
pub const UAUDIO_MIX_SW: i32 = 0;
/// `UAUDIO_MIX_NUM`: a numeric mixer control.
pub const UAUDIO_MIX_NUM: i32 = 1;
/// `UAUDIO_MIX_ENUM`: an enumerated mixer control (not used yet).
pub const UAUDIO_MIX_ENUM: i32 = 2;

/// `UAUDIO_NXFERS_MIN`: the fewest transfers a stream keeps in flight.
pub const UAUDIO_NXFERS_MIN: u32 = 2;
/// `UAUDIO_NXFERS_MAX`: the most transfers a stream keeps in flight.
pub const UAUDIO_NXFERS_MAX: usize = 8;

/// `uaudio_rates[]`: the only rates supported. To keep things simple, continuous sample
/// rates and other "advanced" features which complicate the implementation are not.
pub const UAUDIO_RATES: [i32; 15] = [
    8000, 11025, 12000, 16000, 22050, 24000, 32000, 44100, 48000, 64000, 88200, 96000, 128000,
    176400, 192000,
];

/// A unit name: `char name[UAUDIO_NAMEMAX]`, NUL-terminated.
pub type UaudioUnitName = [u8; UAUDIO_NAMEMAX];

/// `struct uaudio_blob`: read and write positions for secure sequential access of binary
/// data (USB descriptors, request replies). Bytes are read from `rptr` up to `wptr`.
#[derive(Clone, Copy, Debug)]
pub struct UaudioBlob<'a> {
    /// The whole buffer both positions point into.
    pub buf: &'a [u8],
    /// `rptr`: the next byte to read.
    pub rptr: usize,
    /// `wptr`: the end of the readable bytes.
    pub wptr: usize,
}

impl<'a> UaudioBlob<'a> {
    /// A blob over all of `buf`.
    pub const fn new(buf: &'a [u8]) -> Self {
        Self {
            buf,
            rptr: 0,
            wptr: buf.len(),
        }
    }

    /// `p->wptr - p->rptr`.
    pub const fn remaining(&self) -> usize {
        self.wptr - self.rptr
    }
}

/// `struct uaudio_ranges_el`: one range of a [`UaudioRanges`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UaudioRangesEl {
    /// `min`.
    pub min: i32,
    /// `max`.
    pub max: i32,
    /// `res`: the resolution.
    pub res: i32,
}

/// `struct uaudio_ranges`: ranges of integer values used to represent control values and
/// sample frequencies; the list (`el`) is kept sorted.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UaudioRanges {
    /// `nval`: the number of values in all ranges.
    pub nval: u32,
    /// `el`: the ranges, in list order.
    pub el: Vec<UaudioRangesEl>,
}

/// `struct uaudio_mixent`: a mixer(4) control of a unit.
#[derive(Clone, Debug)]
pub struct UaudioMixent {
    /// `fname`: the feature's name, `None` for the C table's `NULL`s.
    pub fname: Option<&'static str>,
    /// `type`: `UAUDIO_MIX_*`.
    pub type_: i32,
    /// `chan`: the channel, -1 for all.
    pub chan: i32,
    /// `req_sel`: the request selector (`UAUDIO_REQSEL_*`).
    pub req_sel: i32,
    /// `ranges`: the values of a numeric control.
    pub ranges: UaudioRanges,
}

/// `struct uaudio_unit`: one unit of the device's circuit. Input and output jacks are
/// terminal units, others are processing units; UAC v2.0 also exposes its clock circuitry
/// as units. The links are indices into [`UaudioConf::units`].
#[derive(Clone, Debug)]
pub struct UaudioUnit {
    /// `unit_next`: the next unit of `unit_list`.
    pub unit_next: Option<usize>,
    /// `src_next`: the next unit of a `src_list` this unit is on.
    pub src_next: Option<usize>,
    /// `dst_next`: the next unit of a `dst_list` this unit is on.
    pub dst_next: Option<usize>,
    /// `src_list`: the units this one takes its signal from.
    pub src_list: Option<usize>,
    /// `dst_list`: the units this one feeds.
    pub dst_list: Option<usize>,
    /// `name`.
    pub name: UaudioUnitName,
    /// `nch`: number of channels.
    pub nch: u32,
    /// `type`: `UAUDIO_AC_*`.
    pub type_: u32,
    /// `id`: the unit's ID in the descriptors.
    pub id: u32,
    /// `term`: terminal or clock type.
    pub term: u32,
    /// `clock`: clock source, if a terminal or selector.
    pub clock: Option<usize>,
    /// `rates`: sample rates, if this is a clock source.
    pub rates: UaudioRanges,
    /// `cap_freqctl`: the clock source's rate can be set.
    pub cap_freqctl: bool,
    /// `mixer_class`: `UAUDIO_CLASS_*`.
    pub mixer_class: i32,
    /// `mixent_list`: the unit's mixer(4) controls, sorted by name and channel.
    pub mixent_list: Vec<UaudioMixent>,
}

/// `struct uaudio_name`: a name template and the next number to suffix it with, so that
/// generated names are unique ("spkr5").
#[derive(Clone, Debug)]
pub struct UaudioName {
    /// `templ`: the template, without NUL.
    pub templ: Vec<u8>,
    /// `unit`: the number the next use of the template gets.
    pub unit: u32,
}

/// `struct uaudio_alt`: an audio streaming alternate setting: stream format and the USB
/// parameters to use it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UaudioAlt {
    /// `ifnum`.
    pub ifnum: u32,
    /// `altnum`.
    pub altnum: u32,
    /// `mode`: one of `AUMODE_{RECORD,PLAY}`.
    pub mode: i32,
    /// `data_addr`: data endpoint address.
    pub data_addr: u32,
    /// `sync_addr`: feedback endpoint address.
    pub sync_addr: u32,
    /// `impl_fb`: supports implicit feedback.
    pub impl_fb: bool,
    /// `maxpkt`: max supported bytes per frame.
    pub maxpkt: u32,
    /// `fps`: USB (micro-)frames per second.
    pub fps: u32,
    /// `bps`: bytes per sample.
    pub bps: u32,
    /// `bits`: bits per sample.
    pub bits: u32,
    /// `nch`: channels.
    pub nch: u32,
    /// `v1_rates`: if UAC 1.0, bitmap of rates (`uaudio_rates[]` indices).
    pub v1_rates: i32,
    /// `v1_cap_freqctl`: can set the sample rate.
    pub v1_cap_freqctl: bool,
}

/// `struct uaudio_params`: play and record stream formats usable together.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UaudioParams {
    /// `palt`: the play alternate setting, an index into [`UaudioConf::alts`].
    pub palt: Option<usize>,
    /// `ralt`: the record alternate setting.
    pub ralt: Option<usize>,
    /// `v1_rates`.
    pub v1_rates: i32,
}

/// What `uaudio_process_conf` builds: the parts of `struct uaudio_softc` that describe the
/// device (see the module's deviations).
#[derive(Clone, Debug, Default)]
pub struct UaudioConf {
    /// `version`: `UAUDIO_V1` or `UAUDIO_V2`.
    pub version: u32,
    /// `instnum`: the number of AC interfaces other uaudio(4) instances claimed first.
    pub instnum: u32,
    /// The units, in allocation order; `unit_list` and the units' links index this.
    pub units: Vec<UaudioUnit>,
    /// `unit_list`: the first unit of the list (the last allocated).
    pub unit_list: Option<usize>,
    /// `pclock`: current play clock, UAC v2.0 only.
    pub pclock: Option<usize>,
    /// `rclock`: current record clock, UAC v2.0 only.
    pub rclock: Option<usize>,
    /// `nin`: number of input terminals.
    pub nin: u32,
    /// `nout`: number of output terminals.
    pub nout: u32,
    /// `names`: the name templates used so far.
    pub names: Vec<UaudioName>,
    /// `alts`: the AS alternate settings, sorted from the most capable to the least.
    pub alts: Vec<UaudioAlt>,
    /// `params_list`: the parameter sets.
    pub params_list: Vec<UaudioParams>,
    /// `ctl_ifnum`: the AC interface.
    pub ctl_ifnum: u32,
    /// `ufps`: USB frames per second (the softc's, which the endpoint parser reads).
    pub ufps: u32,
}

impl UaudioConf {
    /// An empty configuration for a device running at `ufps` frames per second.
    pub fn new(ufps: u32) -> Self {
        Self {
            ufps,
            ..Self::default()
        }
    }

    /// The units of `unit_list`, in list order.
    pub fn unit_iter(&self) -> impl Iterator<Item = usize> + '_ {
        let mut u = self.unit_list;
        core::iter::from_fn(move || {
            let cur = u?;
            u = self.units[cur].unit_next;
            Some(cur)
        })
    }

    /// The units of the `src_list` of unit `u`.
    pub fn src_iter(&self, u: usize) -> impl Iterator<Item = usize> + '_ {
        let mut s = self.units[u].src_list;
        core::iter::from_fn(move || {
            let cur = s?;
            s = self.units[cur].src_next;
            Some(cur)
        })
    }
}

/// `struct uaudio_xfer`: an isochronous transfer and its bounce buffers.
pub struct UaudioXfer {
    /// `usb_xfer`.
    pub usb_xfer: Cell<Option<&'static UsbdXfer>>,
    /// `buf`: the transfer's DMA buffer (`usbd_alloc_buffer`).
    pub buf: Cell<*mut u8>,
    /// `buf`'s length in bytes.
    pub buflen: Cell<u32>,
    /// `sizes`: the frame lengths (`mallocarray`), handed to the controller, which writes
    /// the actual lengths back.
    pub sizes: Cell<*mut u16>,
    /// The number of entries of `sizes`.
    pub nsizes: Cell<u32>,
    /// `size`: bytes requested.
    pub size: Cell<u32>,
    /// `nframes`: frames requested.
    pub nframes: Cell<u32>,
}

impl UaudioXfer {
    /// `xfer->buf[0..buflen]`, empty before the allocation.
    ///
    /// # Safety
    ///
    /// No other reference to the buffer's bytes is used while the slice lives (the buffer
    /// is the driver's; the controller reads or writes it only for frames due later, as in
    /// the C).
    #[allow(clippy::mut_from_ref)] // DMA memory owned through a raw pointer, as in C
    unsafe fn buf(&self) -> &mut [u8] {
        let p = self.buf.get();
        if p.is_null() {
            return &mut [];
        }
        // SAFETY: `uaudio_xfer_alloc` set `buf` to an allocation of `buflen` bytes, freed
        // only by `uaudio_xfer_free`, which clears it; exclusive use is the caller's
        // contract.
        unsafe { slice::from_raw_parts_mut(p, self.buflen.get() as usize) }
    }

    /// `xfer->sizes[0..nsizes]`, empty before the allocation.
    fn sizes(&self) -> &[Cell<u16>] {
        let p = self.sizes.get();
        if p.is_null() {
            return &[];
        }
        // SAFETY: `uaudio_xfer_alloc` set `sizes` to `nsizes` entries, freed only by
        // `uaudio_xfer_free`, which clears it. `Cell<u16>` has `u16`'s layout, and the
        // controller's writes of the actual lengths (through `frlengths`) happen under the
        // kernel lock, never while this shared view is being read.
        unsafe { slice::from_raw_parts(p.cast::<Cell<u16>>(), self.nsizes.get() as usize) }
    }
}

/// `struct uaudio_stream`: one direction audio stream, aka "DMA" in progress.
pub struct UaudioStream {
    /// `data_xfers`.
    pub data_xfers: [UaudioXfer; UAUDIO_NXFERS_MAX],
    /// `sync_xfers`.
    pub sync_xfers: [UaudioXfer; UAUDIO_NXFERS_MAX],
    /// `nxfers`: the `data_xfers[]` entries used: not all of them, as too many frames
    /// cannot be scheduled in the controller.
    pub nxfers: Cell<u32>,
    /// `spf_remain`: fraction of sample left.
    pub spf_remain: Cell<u32>,
    /// `spf`: average samples per frame.
    pub spf: Cell<u32>,
    /// `spf_min`: allowed lower boundary.
    pub spf_min: Cell<u32>,
    /// `spf_max`: allowed upper boundary.
    pub spf_max: Cell<u32>,
    /// `maxpkt`: the max frame size needed (may be lower than the pipe's maxpkt).
    pub maxpkt: Cell<u32>,
    /// `nframes_max`: max number of frames per xfer needed.
    pub nframes_max: Cell<u32>,
    /// `nframes_mask`: at USB 2.0 speed, the number of (micro-)frames per transfer must
    /// correspond to 1ms, the USB 1.1 frame duration, which lower level drivers require;
    /// the number of frames of a transfer is usable when its bits under this mask are zero
    /// (0x0 on USB 1.1, 0x7 at 8000 fps).
    pub nframes_mask: Cell<u32>,
    /// `data_nextxfer`.
    pub data_nextxfer: Cell<u32>,
    /// `sync_nextxfer`.
    pub sync_nextxfer: Cell<u32>,
    /// `data_pipe`.
    pub data_pipe: Cell<Option<&'static UsbdPipe>>,
    /// `sync_pipe`.
    pub sync_pipe: Cell<Option<&'static UsbdPipe>>,
    /// `intr`: audio(4)'s block call-back.
    pub intr: Cell<Option<AudioIntr>>,
    /// `arg`: its argument.
    pub arg: Cell<*mut c_void>,
    /// `ring_start`: audio ring extents, passed to the `trigger()` methods.
    pub ring_start: Cell<*mut u8>,
    /// `ring_end`.
    pub ring_end: Cell<*mut u8>,
    /// `ring_pos`: pointer to the first byte available.
    pub ring_pos: Cell<*mut u8>,
    /// `ring_blksz`: audio(9) block size in bytes.
    pub ring_blksz: Cell<i32>,
    /// `ring_offs`: xfer position relative to the block boundary.
    pub ring_offs: Cell<i32>,
    /// `safe_blksz`: as USB samples-per-frame is not constant, transfers are slightly
    /// larger than one audio block; this "safe" size ensures a transfer crosses the block
    /// boundary.
    pub safe_blksz: Cell<i32>,
    /// `ring_icnt`: bytes completed; at a block size, an audio(9) interrupt fires.
    pub ring_icnt: Cell<i32>,
    /// `ubuf_xfer`: the USB transfers are a FIFO, the concatenation of all transfers; this
    /// is the write (read) transfer index of the play (rec) stream.
    pub ubuf_xfer: Cell<u32>,
    /// `ubuf_pos`: offset in bytes in that transfer.
    pub ubuf_pos: Cell<u32>,
}

/// `struct uaudio_softc`.
#[repr(C)]
pub struct UaudioSoftc {
    /// `dev`.
    pub dev: Device,
    /// `udev`.
    pub udev: Cell<Option<&'static UsbdDevice>>,
    /// The parsed configuration ([`UaudioConf`]), set by `uaudio_attach` before audio(4)
    /// attaches and freed by `uaudio_detach`.
    pub conf: Cell<Option<NonNull<UaudioConf>>>,
    /// `params`: the current parameter set, an index into `params_list` plus one.
    pub params: Cell<Option<NonZeroUsize>>,
    /// `pstream`.
    pub pstream: UaudioStream,
    /// `rstream`.
    pub rstream: UaudioStream,
    /// `mode`: `open()` mode.
    pub mode: Cell<i32>,
    /// `trigger_mode`: `trigger()` mode.
    pub trigger_mode: Cell<i32>,
    /// `rate`: current sample rate.
    pub rate: Cell<u32>,
    /// `ufps`: USB frames per second.
    pub ufps: Cell<u32>,
    /// `sync_pktsz`: size of sync packet.
    pub sync_pktsz: Cell<u32>,
    /// `host_nframes`: max frames we can schedule.
    pub host_nframes: Cell<u32>,
    /// `diff_nsamp`: samples play is ahead of rec.
    pub diff_nsamp: Cell<i32>,
    /// `diff_nframes`: frames play is ahead of rec.
    pub diff_nframes: Cell<i32>,
    /// `adjspf_age`: frames since the last `uaudio_adjspf`.
    pub adjspf_age: Cell<u32>,
    /// `copy_todo`: bytes pending to be copied to the transfer buffer. This is play only,
    /// as recorded frames are copied as soon as they are received.
    pub copy_todo: Cell<usize>,
}

// SAFETY: `#[repr(C)]` with the `Device` first; every other member is a `Cell` of an
// integer, a raw pointer, an `Option` of a reference, `NonNull`, `NonZeroUsize` or `fn`,
// all valid as zero (`None`, null, 0).
unsafe impl Softc for UaudioSoftc {}

impl UaudioSoftc {
    /// The parsed configuration. Panics before `uaudio_attach` recorded it (the C's
    /// fields would be the zeroes of a failed attach; nothing reaches here then: audio(4)
    /// attaches only after it).
    pub fn conf(&self) -> &UaudioConf {
        match self.conf.get() {
            // SAFETY: `uaudio_attach` stores a leaked `Box` it never changes again, freed
            // only by `uaudio_detach` once audio(4) is detached and the references gone.
            Some(c) => unsafe { c.as_ref() },
            None => panic(format_args!("{}: no configuration", self.dev.xname())),
        }
    }

    /// `sc->params`. Panics when no parameters were set (the C dereferences `NULL`).
    pub fn params(&self) -> &UaudioParams {
        let conf = self.conf();
        match self.params.get() {
            Some(i) => &conf.params_list[i.get() - 1],
            None => panic(format_args!("{}: no parameters", self.dev.xname())),
        }
    }

    /// `sc->params->palt`.
    pub fn palt(&self) -> &UaudioAlt {
        match self.params().palt {
            Some(a) => &self.conf().alts[a],
            None => panic(format_args!("{}: no play parameters", self.dev.xname())),
        }
    }

    /// `sc->params->ralt`.
    pub fn ralt(&self) -> &UaudioAlt {
        match self.params().ralt {
            Some(a) => &self.conf().alts[a],
            None => panic(format_args!("{}: no record parameters", self.dev.xname())),
        }
    }

    /// `sc->udev`. Panics before the attach set it.
    pub fn udev(&self) -> &'static UsbdDevice {
        match self.udev.get() {
            Some(d) => d,
            None => panic(format_args!("{}: no USB device", self.dev.xname())),
        }
    }
}

/// `uaudio_cd`.
pub static UAUDIO_CD: Cfdriver = Cfdriver::new(b"uaudio", DV_DULL, 0);

/// `uaudio_ca`.
pub static UAUDIO_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<UaudioSoftc>(),
    ca_match: Some(uaudio_match),
    ca_attach: uaudio_attach,
    ca_detach: Some(uaudio_detach),
    ca_activate: None,
};

/// `uaudio_hw_if`.
pub static UAUDIO_HW_IF: AudioHwIf = AudioHwIf {
    open: Some(uaudio_open),
    close: Some(uaudio_close),
    set_params: Some(uaudio_set_params),
    halt_output: Some(uaudio_halt_output),
    halt_input: Some(uaudio_halt_input),
    set_port: Some(uaudio_set_port),
    get_port: Some(uaudio_get_port),
    query_devinfo: Some(uaudio_query_devinfo),
    trigger_output: Some(uaudio_trigger_output),
    trigger_input: Some(uaudio_trigger_input),
    copy_output: Some(uaudio_copy_output),
    underrun: Some(uaudio_underrun),
    set_blksz: Some(uaudio_set_blksz),
    display_name: Some(uaudio_display_name),
    ..AudioHwIf::new()
};

/// What the descriptor parser needs from the device: `DEVNAME(sc)`, class requests on the
/// default pipe (`uaudio_req`) and the configuration's interfaces. The softc implements it
/// over its `usbd_device`; the host tests over tables.
pub trait UaudioDev {
    /// `DEVNAME(sc)`.
    fn devname(&self) -> &str;

    /// `uaudio_req`: a class request with `wValue = sel << 8 | chan` and `wIndex = id << 8 |
    /// ifnum`, whose data stage is `buf`; `false` (the C's 0) on failure.
    #[allow(clippy::too_many_arguments)] // the C's signature
    fn uaudio_req(
        &self,
        type_: u8,
        req: u8,
        sel: u32,
        chan: u32,
        ifnum: u32,
        id: u32,
        buf: &mut [u8],
    ) -> bool;

    /// `sc->udev->cdesc->bNumInterfaces`.
    fn nifaces(&self) -> usize;

    /// `sc->udev->ifaces[i].idesc->bInterfaceNumber`.
    fn iface_number(&self, i: usize) -> u32;

    /// `usbd_iface_claimed(sc->udev, i)`.
    fn iface_claimed(&self, i: usize) -> bool;

    /// `usbd_claim_iface(sc->udev, i)`.
    fn claim_iface(&self, i: usize);
}

/// The softc behind audio(4)'s handle.
///
/// # Safety
///
/// `v` is the handle `uaudio_attach` gave `audio_attach_mi`: a uaudio softc, which lives
/// until `uaudio_detach` returns, after its audio(4) child is gone.
unsafe fn uaudio_hdl<'a>(v: *mut c_void) -> &'a UaudioSoftc {
    // SAFETY: the caller's contract.
    unsafe { &*v.cast::<UaudioSoftc>() }
}

/// The softc behind a USB call-back's private pointer.
fn uaudio_xfer_sc<'a>(arg: *mut c_void) -> &'a UaudioSoftc {
    // SAFETY: every transfer this driver starts carries its softc as private pointer
    // (`uaudio_pdata_xfer`, `uaudio_psync_xfer`, `uaudio_rdata_xfer`), and the softc
    // outlives its pipes, which `uaudio_stream_close` closes (aborting the transfers)
    // before the detach can free it.
    unsafe { &*arg.cast::<UaudioSoftc>() }
}

/// A NUL-terminated byte array as the string before its NUL.
fn cstr(b: &[u8]) -> &str {
    let len = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    core::str::from_utf8(&b[..len]).unwrap_or("?")
}

/// `strcmp` of two feature names, `NULL` comparing as an empty string (see the module's
/// deviations).
fn fname_cmp(a: Option<&str>, b: Option<&str>) -> Ordering {
    a.unwrap_or("").as_bytes().cmp(b.unwrap_or("").as_bytes())
}

/// A feature name for `%s`: `printf(9)` prints `(null)` for `NULL`.
fn fname_str(f: Option<&'static str>) -> &'static str {
    f.unwrap_or("(null)")
}

/// `uaudio_sign_expand`: an 8, 16, 24 or 32-bit signed value as an `int`, by expanding
/// the sign bit.
pub fn uaudio_sign_expand(val: u32, opsize: u32) -> i32 {
    let s = 1u32.wrapping_shl(8 * opsize - 1);
    (val ^ s).wrapping_sub(s) as i32
}

impl UaudioDev for UaudioSoftc {
    fn devname(&self) -> &str {
        self.dev.xname()
    }

    /// `uaudio_req`.
    fn uaudio_req(
        &self,
        type_: u8,
        req: u8,
        sel: u32,
        chan: u32,
        ifnum: u32,
        id: u32,
        buf: &mut [u8],
    ) -> bool {
        let mut r = UsbDeviceRequest {
            bmRequestType: type_,
            bRequest: req,
            ..UsbDeviceRequest::default()
        };
        usetw(&mut r.wValue, (sel << 8 | chan) as u16);
        usetw(&mut r.wIndex, (id << 8 | ifnum) as u16);
        usetw(&mut r.wLength, buf.len() as u16);

        let err = usbd_do_request(self.udev(), &r, buf);
        !err.is_err()
    }

    fn nifaces(&self) -> usize {
        self.udev()
            .cdesc()
            .map_or(0, |cd| usize::from(cd.bNumInterfaces))
    }

    fn iface_number(&self, i: usize) -> u32 {
        u32::from(self.udev().ifaces()[i].idesc().bInterfaceNumber)
    }

    fn iface_claimed(&self, i: usize) -> bool {
        usbd_iface_claimed(self.udev(), i)
    }

    fn claim_iface(&self, i: usize) {
        usbd_claim_iface(self.udev(), i);
    }
}

/// `uaudio_getnum`: reads a little-endian number of `size` bytes from the blob; `None`
/// when the blob is too small.
pub fn uaudio_getnum(p: &mut UaudioBlob<'_>, size: u32) -> Option<u32> {
    if p.remaining() < size as usize {
        return None;
    }

    let mut num = 0u32;
    for i in 0..size {
        num |= u32::from(p.buf[p.rptr]).wrapping_shl(8 * i);
        p.rptr += 1;
    }
    Some(num)
}

/// `uaudio_getdesc`: reads a USB descriptor (its length byte, then the rest) from the
/// blob; the returned blob starts after the length byte.
pub fn uaudio_getdesc<'a>(p: &mut UaudioBlob<'a>) -> Option<UaudioBlob<'a>> {
    let size = uaudio_getnum(p, 1)?;
    if size == 0 {
        return None;
    }
    let size = (size - 1) as usize;
    if p.remaining() < size {
        return None;
    }
    let ret = UaudioBlob {
        buf: p.buf,
        rptr: p.rptr,
        wptr: p.rptr + size,
    };
    p.rptr += size;
    Some(ret)
}

/// `uaudio_unit_byid`: the unit with the given id.
pub fn uaudio_unit_byid(conf: &UaudioConf, id: u32) -> Option<usize> {
    conf.unit_iter().find(|&u| conf.units[u].id == id)
}

/// `uaudio_tname`: a terminal name for the given terminal type.
pub fn uaudio_tname(conf: &UaudioConf, type_: u32, isout: bool) -> &'static str {
    let hi = type_ >> 8;
    let lo = type_ & 0xff;

    // usb data stream
    if hi == 1 {
        return if isout {
            UAUDIO_NAME_REC
        } else {
            UAUDIO_NAME_PLAY
        };
    }

    // if there is only one input (output) use "input" ("output")
    if isout {
        if conf.nout == 1 {
            return "output";
        }
    } else if conf.nin == 1 {
        return "input";
    }

    // determine name from USB terminal type
    let pick = |o: &'static str, i: &'static str| if isout { o } else { i };
    match hi {
        // embedded inputs
        2 => pick("mic-out", "mic"),
        // embedded outputs, mostly speakers, except 0x302
        3 => match lo {
            0x02 => pick("hp", "hp-in"),
            _ => pick("spkr", "spkr-in"),
        },
        // handsets and headset
        4 => pick("spkr", "mic"),
        // phone line
        5 => pick("phone-in", "phone-out"),
        // external sources/sinks
        6 => match lo {
            0x02 | 0x05 | 0x06 | 0x07 | 0x09 | 0x0a => pick("dig-out", "dig-in"),
            _ => pick("line-out", "line-in"),
        },
        // internal devices
        7 => pick("int-out", "int-in"),
        _ => pick("unk-out", "unk-in"),
    }
}

/// `uaudio_clkname`: a clock name for the given clock type.
pub fn uaudio_clkname(attr: u32) -> &'static str {
    const NAMES: [&str; 4] = ["ext", "fixed", "var", "prog"];

    NAMES[(attr & 3) as usize]
}

/// `uaudio_mkname`: gives unit `u` a unique name from the template `templ`.
pub fn uaudio_mkname(conf: &mut UaudioConf, templ: &[u8], u: usize) {
    // If this is not a terminal name (i.e. there's an underscore in the name, like in
    // "spkr2_mic3"), then use underscore as separator to avoid concatenating two numbers.
    let sep = if templ.contains(&b'_') { "_" } else { "" };

    let n = match conf.names.iter().position(|n| n.templ == templ) {
        Some(n) => n,
        None => {
            conf.names.push(UaudioName {
                templ: templ.to_vec(),
                unit: 0,
            });
            conf.names.len() - 1
        }
    };
    let templ = core::str::from_utf8(templ).unwrap_or("?");
    let unit = conf.names[n].unit;
    let res = &mut conf.units[u].name;
    if unit == 0 {
        snprintf(res, format_args!("{templ}"));
    } else {
        snprintf(res, format_args!("{templ}{sep}{unit}"));
    }
    conf.names[n].unit += 1;
}

/// `uaudio_feature_fixup`: a UAC v1.0 feature bitmap as a UAC v2.0 feature bitmap (two
/// bits per control, both set when the control is settable).
pub fn uaudio_feature_fixup(conf: &UaudioConf, ctl: u32) -> u32 {
    match conf.version {
        UAUDIO_V1 => {
            let mut n = 0u32;
            for i in 0..16 {
                let mut bits = (ctl >> i) & 1;
                if bits != 0 {
                    bits |= 2;
                }
                n |= bits << (2 * i);
            }
            n
        }
        _ => ctl,
    }
}

/// `uaudio_ranges_init`: the empty set.
pub fn uaudio_ranges_init(r: &mut UaudioRanges) {
    r.el.clear();
    r.nval = 0;
}

/// `uaudio_ranges_add`: adds a range. Ranges are not supposed to overlap (the USB spec
/// requires it); if they do, the range is ignored.
pub fn uaudio_ranges_add(r: &mut UaudioRanges, min: i32, max: i32, res: i32) {
    if min > max {
        return;
    }

    let mut at = r.el.len();
    for (k, e) in r.el.iter().enumerate() {
        if min <= e.max && max >= e.min {
            return;
        }
        if min < e.max {
            at = k;
            break;
        }
    }

    // XXX: use 'res' here
    r.nval = r
        .nval
        .wrapping_add(max.wrapping_sub(min).wrapping_add(1) as u32);

    r.el.insert(at, UaudioRangesEl { min, max, res });
}

/// `uaudio_ranges_clear`: frees all ranges, making the set empty.
pub fn uaudio_ranges_clear(r: &mut UaudioRanges) {
    r.el.clear();
    r.nval = 0;
}

/// `uaudio_ranges_decode`: a value of the ranges as a 0..255 integer, for the mixer.
pub fn uaudio_ranges_decode(r: &UaudioRanges, val: i32) -> i32 {
    let mut pos: i32 = 0;

    for e in &r.el {
        if val >= e.min && val <= e.max {
            pos = pos.wrapping_add(val.wrapping_sub(e.min));
            if r.nval == 1 {
                return 0;
            }
            // `pos * 255 + (nval - 1) / 2` is unsigned arithmetic in C
            let n = r.nval.wrapping_sub(1);
            return ((pos.wrapping_mul(255) as u32).wrapping_add(n / 2) / n) as i32;
        }
        let diff = e.max.wrapping_sub(e.min).wrapping_add(1);
        pos = pos.wrapping_add(diff);
    }
    0
}

/// `uaudio_ranges_encode`: a 0..255 mixer value as a value of the ranges, for a USB
/// request.
pub fn uaudio_ranges_encode(r: &UaudioRanges, val: i32) -> u32 {
    // unsigned arithmetic in C, stored in an int
    let mut pos = ((val as u32)
        .wrapping_mul(r.nval.wrapping_sub(1))
        .wrapping_add(127)
        / 255) as i32;

    for e in &r.el {
        let diff = e.max.wrapping_sub(e.min).wrapping_add(1);
        if pos < diff {
            return e.min.wrapping_add(pos) as u32;
        }
        pos = pos.wrapping_sub(diff);
    }
    0
}

/// `uaudio_ranges_getrates`: the bitmap of the supported rates (`uaudio_rates[]`) in the
/// ranges, each scaled by `mult / div`. Not a mixer thing: UAC v2.0 reports sample rates as
/// ranges.
pub fn uaudio_ranges_getrates(r: &UaudioRanges, mult: u32, div: u32) -> i32 {
    let mut rates = 0i32;

    for e in &r.el {
        for (i, &rate) in UAUDIO_RATES.iter().enumerate() {
            let v = (rate as u64 * u64::from(mult) / u64::from(div)) as i32;
            if v < e.min || v > e.max {
                continue;
            }
            // As the C: `v - e->min % e->res`, the remainder taken first.
            if e.res == 0 || v.wrapping_sub(e.min.wrapping_rem(e.res)) == 0 {
                rates |= 1 << i;
            }
        }
    }

    rates
}

/// `uaudio_rates_indexof`: the index in `uaudio_rates[]` of the rate of `mask` closest to
/// `rate` in Hz, -1 when `mask` has none.
pub fn uaudio_rates_indexof(mask: i32, rate: i32) -> i32 {
    let mut best_index = -1;
    let mut best_diff = i32::MAX;
    for (i, &r) in UAUDIO_RATES.iter().enumerate() {
        if mask & (1 << i) == 0 {
            continue;
        }
        let diff = r.wrapping_sub(rate).wrapping_abs();
        if diff < best_diff {
            best_index = i as i32;
            best_diff = diff;
        }
    }
    best_index
}

/// `uaudio_req_ranges`: a request whose reply is a [`UaudioRanges`]: on UAC v1.0 a
/// min/max/res triplet (three requests), on UAC v2.0 an array of triplets.
#[allow(clippy::too_many_arguments)] // the C's signature
pub fn uaudio_req_ranges<D: UaudioDev + ?Sized>(
    sc: &D,
    conf: &UaudioConf,
    opsize: u32,
    sel: u32,
    chan: u32,
    ifnum: u32,
    id: u32,
    r: &mut UaudioRanges,
) -> bool {
    let mut req_buf = [0u8; 16];
    let mut req_vec: Vec<u8>;
    let opsz = opsize as usize;
    let count: u32;
    let mut p: UaudioBlob<'_>;

    match conf.version {
        UAUDIO_V1 => {
            count = 1;
            // the callers' `opsize` is 2 or 4, so the three replies fit
            if 3 * opsz > req_buf.len() {
                return false;
            }
            for (k, req) in [
                UAUDIO_V1_REQ_GET_MIN,
                UAUDIO_V1_REQ_GET_MAX,
                UAUDIO_V1_REQ_GET_RES,
            ]
            .into_iter()
            .enumerate()
            {
                if !sc.uaudio_req(
                    UT_READ_CLASS_INTERFACE,
                    req,
                    sel,
                    chan,
                    ifnum,
                    id,
                    &mut req_buf[k * opsz..(k + 1) * opsz],
                ) {
                    return false;
                }
            }
            p = UaudioBlob {
                buf: &req_buf,
                rptr: 0,
                wptr: 3 * opsz,
            };
        }
        UAUDIO_V2 => {
            // fetch the ranges count only (first 2 bytes)
            if !sc.uaudio_req(
                UT_READ_CLASS_INTERFACE,
                UAUDIO_V2_REQ_RANGES,
                sel,
                chan,
                ifnum,
                id,
                &mut req_buf[..2],
            ) {
                return false;
            }

            // count is at most 65535
            count = u32::from(req_buf[0]) | u32::from(req_buf[1]) << 8;

            // restart the request on a large enough buffer
            let req_size = 2 + 3 * opsz * count as usize;
            let req: &mut [u8] = if req_buf.len() >= req_size {
                &mut req_buf[..req_size]
            } else {
                req_vec = alloc::vec![0u8; req_size];
                &mut req_vec[..]
            };
            if !sc.uaudio_req(
                UT_READ_CLASS_INTERFACE,
                UAUDIO_V2_REQ_RANGES,
                sel,
                chan,
                ifnum,
                id,
                req,
            ) {
                return false;
            }
            // skip initial 2 bytes of count
            p = UaudioBlob {
                buf: req,
                rptr: 2,
                wptr: req_size,
            };
        }
        _ => return false,
    }

    uaudio_req_ranges_parse(&mut p, count, opsize, r)
}

/// The end of `uaudio_req_ranges`: `count` triplets of `opsize`-byte signed numbers.
fn uaudio_req_ranges_parse(
    p: &mut UaudioBlob<'_>,
    count: u32,
    opsize: u32,
    r: &mut UaudioRanges,
) -> bool {
    for _ in 0..count {
        let Some(min) = uaudio_getnum(p, opsize) else {
            return false;
        };
        let Some(max) = uaudio_getnum(p, opsize) else {
            return false;
        };
        let Some(res) = uaudio_getnum(p, opsize) else {
            return false;
        };
        uaudio_ranges_add(
            r,
            uaudio_sign_expand(min, opsize),
            uaudio_sign_expand(max, opsize),
            uaudio_sign_expand(res, opsize),
        );
    }
    true
}

/// `uaudio_alt_getrates`: the rates bitmap of an alternate setting.
pub fn uaudio_alt_getrates(conf: &UaudioConf, p: &UaudioAlt) -> i32 {
    let (mult, div) = (1, 1);

    match conf.version {
        UAUDIO_V1 => p.v1_rates,
        UAUDIO_V2 => {
            let mut u = if p.mode == AUMODE_PLAY {
                conf.pclock
            } else {
                conf.rclock
            };
            // The C dereferences a missing clock; see the module's deviations.
            while let Some(ui) = u {
                let unit = &conf.units[ui];
                match unit.type_ {
                    UAUDIO_AC_CLKSRC => return uaudio_ranges_getrates(&unit.rates, mult, div),
                    UAUDIO_AC_CLKSEL => u = unit.clock,
                    // XXX: adjust rate with multiplier
                    UAUDIO_AC_CLKMULT | UAUDIO_AC_RATECONV => u = unit.src_list,
                    _ => return 0,
                }
            }
            0
        }
        _ => 0,
    }
}

/// `uaudio_clock`: the clock source unit of a clock chain.
pub fn uaudio_clock(conf: &UaudioConf, mut u: Option<usize>) -> Option<usize> {
    loop {
        let ui = u?;
        let unit = &conf.units[ui];
        match unit.type_ {
            UAUDIO_AC_CLKSRC => return Some(ui),
            UAUDIO_AC_CLKSEL => u = unit.clock,
            UAUDIO_AC_CLKMULT | UAUDIO_AC_RATECONV => u = unit.src_list,
            _ => return None,
        }
    }
}

/// `uaudio_getrates`: the rates bitmap of a parameter set for `mode`.
pub fn uaudio_getrates(conf: &UaudioConf, mode: i32, p: &UaudioParams) -> i32 {
    match conf.version {
        UAUDIO_V1 => p.v1_rates,
        UAUDIO_V2 => {
            let mut rates = !0;
            if let Some(a) = p.palt
                && mode & AUMODE_PLAY != 0
            {
                rates &= uaudio_alt_getrates(conf, &conf.alts[a]);
            }
            if let Some(a) = p.ralt
                && mode & AUMODE_RECORD != 0
            {
                rates &= uaudio_alt_getrates(conf, &conf.alts[a]);
            }
            rates
        }
        _ => 0,
    }
}

/// The features of a feature unit, by `bmaControls` bit: name, mixer type, request
/// selector (`uaudio_feature_addent`'s table).
const UAUDIO_FEATURES: [(Option<&str>, i32, i32); 15] = [
    (Some("mute"), UAUDIO_MIX_SW, UAUDIO_REQSEL_MUTE as i32),
    (Some("level"), UAUDIO_MIX_NUM, UAUDIO_REQSEL_VOLUME as i32),
    (Some("bass"), UAUDIO_MIX_NUM, UAUDIO_REQSEL_BASS as i32),
    (Some("mid"), UAUDIO_MIX_NUM, UAUDIO_REQSEL_MID as i32),
    (Some("treble"), UAUDIO_MIX_NUM, UAUDIO_REQSEL_TREBLE as i32),
    (Some("eq"), UAUDIO_MIX_NUM, UAUDIO_REQSEL_EQ as i32),
    (Some("agc"), UAUDIO_MIX_SW, UAUDIO_REQSEL_AGC as i32),
    (None, -1, -1), // delay
    (
        Some("bassboost"),
        UAUDIO_MIX_SW,
        UAUDIO_REQSEL_BASSBOOST as i32,
    ),
    (Some("loud"), UAUDIO_MIX_SW, UAUDIO_REQSEL_LOUDNESS as i32),
    (Some("gain"), UAUDIO_MIX_NUM, UAUDIO_REQSEL_GAIN as i32),
    (Some("gainpad"), UAUDIO_MIX_SW, UAUDIO_REQSEL_GAINPAD as i32),
    (Some("phase"), UAUDIO_MIX_SW, UAUDIO_REQSEL_PHASEINV as i32),
    (None, -1, -1), // underflow
    (None, -1, -1), // overflow
];

/// `uaudio_feature_addent`: adds the feature (mixer control) `uac_type` of channel `chan`
/// (-1 for all) to unit `u`.
pub fn uaudio_feature_addent<D: UaudioDev + ?Sized>(
    sc: &D,
    conf: &mut UaudioConf,
    u: usize,
    uac_type: u32,
    chan: i32,
) {
    let Some(&(fname, type_, req_sel)) = UAUDIO_FEATURES.get(uac_type as usize) else {
        printf(format_args!("{}: skipped unknown feature\n", sc.devname()));
        return;
    };

    let mut m = UaudioMixent {
        fname,
        type_,
        chan,
        req_sel,
        ranges: UaudioRanges::default(),
    };

    if m.type_ == UAUDIO_MIX_NUM {
        let id = conf.units[u].id;
        if !uaudio_req_ranges(
            sc,
            conf,
            2,
            m.req_sel as u32,
            if chan < 0 { 0 } else { (chan + 1) as u32 },
            conf.ctl_ifnum,
            id,
            &mut m.ranges,
        ) {
            printf(format_args!(
                "{}: failed to get ranges for {} control\n",
                sc.devname(),
                fname_str(m.fname)
            ));
            return;
        }
        if m.ranges.el.is_empty() {
            printf(format_args!(
                "{}: skipped {} control with empty range\n",
                sc.devname(),
                fname_str(m.fname)
            ));
            return;
        }
    }

    // Add to the unit's mixer controls, sorting entries by name and increasing channel
    // number.
    let list = &mut conf.units[u].mixent_list;
    let mut at = list.len();
    for (k, i) in list.iter().enumerate() {
        let mut cmp = fname_cmp(i.fname, m.fname);
        if cmp == Ordering::Equal {
            cmp = i.chan.cmp(&m.chan);
        }
        if cmp == Ordering::Equal {
            // duplicate feature for this channel
            return;
        }
        if cmp == Ordering::Greater {
            at = k;
            break;
        }
    }
    list.insert(at, m);
}

/// `uaudio_clock_equiv`: whether two clock source units are equivalent. According to UAC2
/// each clock source defines its own clock domain, but some hardware exposes several clock
/// units clocked by the same source, which amount to a single domain.
pub fn uaudio_clock_equiv<D: UaudioDev + ?Sized>(
    sc: &D,
    conf: &UaudioConf,
    u: Option<usize>,
    v: Option<usize>,
) -> bool {
    if u.is_none() || v.is_none() {
        return true;
    }

    let Some(u) = uaudio_clock(conf, u) else {
        return false;
    };
    let Some(v) = uaudio_clock(conf, v) else {
        return false;
    };

    if conf.units[u].term != conf.units[v].term {
        printf(format_args!("{}: clock attributes differ\n", sc.devname()));
        return false;
    }

    if u != v {
        printf(format_args!(
            "{}: warning: assuming common clock\n",
            sc.devname()
        ));
    }

    true
}

/// `uaudio_process_srcs`: parses the list of sources of unit `u` and processes each
/// (`uaudio_process_unit`).
pub fn uaudio_process_srcs<D: UaudioDev + ?Sized>(
    sc: &D,
    conf: &mut UaudioConf,
    u: usize,
    units: UaudioBlob<'_>,
    p: &mut UaudioBlob<'_>,
) -> bool {
    let Some(npin) = uaudio_getnum(p, 1) else {
        return false;
    };
    let mut last: Option<usize> = None;
    for _ in 0..npin {
        let Some(sid) = uaudio_getnum(p, 1) else {
            return false;
        };
        let Some(s) = uaudio_process_unit(sc, conf, Some(u), sid, units) else {
            return false;
        };
        conf.units[s].src_next = None;
        match last {
            None => conf.units[u].src_list = Some(s),
            Some(l) => conf.units[l].src_next = Some(s),
        }
        last = Some(s);
    }
    true
}

/// `uaudio_process_nch`: parses the number of channels (and skips the channel
/// configuration and names).
pub fn uaudio_process_nch(conf: &mut UaudioConf, u: usize, p: &mut UaudioBlob<'_>) -> bool {
    let Some(nch) = uaudio_getnum(p, 1) else {
        return false;
    };
    conf.units[u].nch = nch;
    // skip junk: bmChannelConfig (v1), wChannelConfig (v2)
    let junk = match conf.version {
        UAUDIO_V1 => 2,
        UAUDIO_V2 => 4,
        _ => 0,
    };
    if uaudio_getnum(p, junk).is_none() {
        return false;
    }
    // iChannelNames
    uaudio_getnum(p, 1).is_some()
}

/// `uaudio_unit_getdesc`: the AC class-specific descriptor of unit `id` (after its type,
/// subtype and id bytes), and its subtype.
pub fn uaudio_unit_getdesc<'a>(
    id: u32,
    mut units: UaudioBlob<'a>,
) -> Option<(UaudioBlob<'a>, u32)> {
    // Find the usb descriptor for this id.
    loop {
        if units.rptr == units.wptr {
            return None;
        }
        let mut p = uaudio_getdesc(&mut units)?;
        let _type = uaudio_getnum(&mut p, 1)?;
        let subtype = uaudio_getnum(&mut p, 1)?;
        let i = uaudio_getnum(&mut p, 1)?;
        if i == id {
            return Some((p, subtype));
        }
    }
}

/// `uaudio_process_unit`: parses unit `id`, possibly processing its sources recursively;
/// `dest` is the unit it feeds, if any. Returns the unit (the C's `*rchild`), `None` on
/// error.
pub fn uaudio_process_unit<D: UaudioDev + ?Sized>(
    sc: &D,
    conf: &mut UaudioConf,
    dest: Option<usize>,
    id: u32,
    units: UaudioBlob<'_>,
) -> Option<usize> {
    let (mut p, subtype) = uaudio_unit_getdesc(id, units)?;

    // find this unit on the list as it may be already processed as the source of another
    // destination
    let u = match uaudio_unit_byid(conf, id) {
        None => {
            conf.units.push(UaudioUnit {
                unit_next: conf.unit_list,
                src_next: None,
                dst_next: None,
                src_list: None,
                dst_list: None,
                name: [0; UAUDIO_NAMEMAX],
                nch: 0,
                type_: subtype,
                id,
                term: 0,
                clock: None,
                rates: UaudioRanges::default(),
                cap_freqctl: false,
                mixer_class: 0,
                mixent_list: Vec::new(),
            });
            let u = conf.units.len() - 1;
            conf.unit_list = Some(u);
            u
        }
        Some(u) => {
            match conf.units[u].type_ {
                UAUDIO_AC_CLKSRC | UAUDIO_AC_CLKSEL | UAUDIO_AC_CLKMULT | UAUDIO_AC_RATECONV => {
                    // not using 'dest' list
                    return Some(u);
                }
                _ => {}
            }
            u
        }
    };

    if let Some(dest) = dest {
        conf.units[dest].dst_next = conf.units[u].dst_list;
        conf.units[u].dst_list = Some(dest);
        if conf.units[dest].dst_next.is_some() {
            // already seen
            return Some(u);
        }
    }

    match conf.units[u].type_ {
        UAUDIO_AC_INPUT => {
            conf.units[u].term = uaudio_getnum(&mut p, 2)?;
            let _assoc = uaudio_getnum(&mut p, 1)?;
            if conf.units[u].term >> 8 != 1 {
                conf.nin += 1;
            }
            if conf.version == UAUDIO_V2 {
                let clk = uaudio_getnum(&mut p, 1)?;
                let c = uaudio_process_unit(sc, conf, None, clk, units)?;
                conf.units[u].clock = Some(c);
            }
            conf.units[u].nch = uaudio_getnum(&mut p, 1)?;
        }
        UAUDIO_AC_OUTPUT => {
            conf.units[u].term = uaudio_getnum(&mut p, 2)?;
            let _assoc = uaudio_getnum(&mut p, 1)?;
            let id = uaudio_getnum(&mut p, 1)?;
            let s = uaudio_process_unit(sc, conf, Some(u), id, units)?;
            if conf.units[u].term >> 8 != 1 {
                conf.nout += 1;
            }
            if conf.version == UAUDIO_V2 {
                let clk = uaudio_getnum(&mut p, 1)?;
                let c = uaudio_process_unit(sc, conf, None, clk, units)?;
                conf.units[u].clock = Some(c);
            }
            conf.units[u].src_list = Some(s);
            conf.units[s].src_next = None;
            conf.units[u].nch = conf.units[s].nch;
        }
        UAUDIO_AC_MIXER => {
            if !uaudio_process_srcs(sc, conf, u, units, &mut p) {
                return None;
            }
            if !uaudio_process_nch(conf, u, &mut p) {
                return None;
            }
            // The mixer's knobs (a bit matrix) follow. Matrix mixers are rare because
            // levels are already controlled by feature units, making the knobs redundant
            // with the feature's; they are not exposed, as other popular OSes do not.
        }
        UAUDIO_AC_SELECTOR => {
            // Selectors are extremely rare, so not supported yet.
            if !uaudio_process_srcs(sc, conf, u, units, &mut p) {
                return None;
            }
            let Some(s) = conf.units[u].src_list else {
                printf(format_args!(
                    "{}: selector {:02} has no sources\n",
                    sc.devname(),
                    conf.units[u].id
                ));
                return None;
            };
            conf.units[u].nch = conf.units[s].nch;
        }
        UAUDIO_AC_FEATURE => {
            let id = uaudio_getnum(&mut p, 1)?;
            let s = uaudio_process_unit(sc, conf, Some(u), id, units)?;
            conf.units[s].src_next = conf.units[u].src_list;
            conf.units[u].src_list = Some(s);
            conf.units[u].nch = conf.units[s].nch;
            let size = match conf.version {
                UAUDIO_V1 => uaudio_getnum(&mut p, 1)?,
                UAUDIO_V2 => 4,
                // see the module's deviations
                _ => return None,
            };

            let ctl = uaudio_getnum(&mut p, size)?;
            let mut ctl = uaudio_feature_fixup(conf, ctl);
            for i in 0..16 {
                if ctl & 3 == 3 {
                    uaudio_feature_addent(sc, conf, u, i, -1);
                }
                ctl >>= 2;
            }

            // certain devices provide no per-channel control descriptors
            if p.remaining() != 1 {
                for j in 0..conf.units[u].nch {
                    let ctl = uaudio_getnum(&mut p, size)?;
                    let mut ctl = uaudio_feature_fixup(conf, ctl);
                    for i in 0..16 {
                        if ctl & 3 == 3 {
                            uaudio_feature_addent(sc, conf, u, i, j as i32);
                        }
                        ctl >>= 2;
                    }
                }
            }
        }
        UAUDIO_AC_EFFECT => {
            let _type = uaudio_getnum(&mut p, 2)?;
            let id = uaudio_getnum(&mut p, 1)?;
            let s = uaudio_process_unit(sc, conf, Some(u), id, units)?;
            conf.units[s].src_next = conf.units[u].src_list;
            conf.units[u].src_list = Some(s);
            conf.units[u].nch = conf.units[s].nch;
        }
        UAUDIO_AC_PROCESSING | UAUDIO_AC_EXTENSION => {
            let _type = uaudio_getnum(&mut p, 2)?;
            if !uaudio_process_srcs(sc, conf, u, units, &mut p) {
                return None;
            }
            if !uaudio_process_nch(conf, u, &mut p) {
                return None;
            }
        }
        UAUDIO_AC_CLKSRC => {
            conf.units[u].term = uaudio_getnum(&mut p, 1)?;
            let ctl = uaudio_getnum(&mut p, 1)?;
            conf.units[u].cap_freqctl = ctl & UAUDIO_CLKSRC_FREQCTL != 0;
        }
        UAUDIO_AC_CLKSEL => {
            if !uaudio_process_srcs(sc, conf, u, units, &mut p) {
                return None;
            }
            if conf.units[u].src_list.is_none() {
                printf(format_args!(
                    "{}: clock selector {:02} with no srcs\n",
                    sc.devname(),
                    conf.units[u].id
                ));
                return None;
            }
        }
        UAUDIO_AC_CLKMULT => {
            // XXX: fetch multiplier
            printf(format_args!(
                "{}: clock multiplier not supported\n",
                sc.devname()
            ));
        }
        UAUDIO_AC_RATECONV => {
            // XXX: fetch multiplier
            printf(format_args!(
                "{}: rate converter not supported\n",
                sc.devname()
            ));
        }
        _ => {}
    }
    Some(u)
}

/// `uaudio_setname_dsts`: names unit `u` after its destination terminal, unless the name
/// is ambiguous (already given to another source unit, or several destinations); with
/// `name`, only that name is taken.
pub fn uaudio_setname_dsts(conf: &mut UaudioConf, u: usize, name: Option<&str>) -> bool {
    let mut d = Some(u);

    while let Some(di) = d {
        let Some(dl) = conf.units[di].dst_list else {
            break;
        };
        if conf.units[dl].dst_next.is_some() {
            break;
        }
        let di = dl;
        let Some(sl) = conf.units[di].src_list else {
            break;
        };
        if conf.units[sl].src_next.is_some() {
            break;
        }
        if conf.units[di].name[0] != 0 {
            if let Some(name) = name
                && name != cstr(&conf.units[di].name)
            {
                break;
            }
            let dname = conf.units[di].name;
            strlcpy(&mut conf.units[u].name, &dname);
            return true;
        }
        d = Some(di);
    }
    false
}

/// `uaudio_setname_srcs`: names unit `u` after its source terminal, unless the name is
/// ambiguous (already given to another destination unit, or several sources); with
/// `name`, only that name is taken.
pub fn uaudio_setname_srcs(conf: &mut UaudioConf, u: usize, name: Option<&str>) -> bool {
    let mut s = Some(u);

    while let Some(si) = s {
        let Some(sl) = conf.units[si].src_list else {
            break;
        };
        if conf.units[sl].src_next.is_some() {
            break;
        }
        let si = sl;
        let Some(dl) = conf.units[si].dst_list else {
            break;
        };
        if conf.units[dl].dst_next.is_some() {
            break;
        }
        if conf.units[si].name[0] != 0 {
            if let Some(name) = name
                && name != cstr(&conf.units[si].name)
            {
                break;
            }
            let sname = conf.units[si].name;
            strlcpy(&mut conf.units[u].name, &sname);
            return true;
        }
        s = Some(si);
    }
    false
}

/// `uaudio_setname_middle`: names unit `u` after both its source and its destination, for
/// units whose name would be ambiguous with either alone.
pub fn uaudio_setname_middle(conf: &mut UaudioConf, u: usize) {
    let mut s = conf.units[u].src_list;
    let s = loop {
        let Some(si) = s else {
            return;
        };
        if conf.units[si].name[0] != 0 {
            break si;
        }
        s = conf.units[si].src_list;
    };

    let mut d = conf.units[u].dst_list;
    let d = loop {
        let Some(di) = d else {
            return;
        };
        if conf.units[di].name[0] != 0 {
            break di;
        }
        d = conf.units[di].dst_list;
    };

    let mut name: UaudioUnitName = [0; UAUDIO_NAMEMAX];
    snprintf(
        &mut name,
        format_args!(
            "{}_{}",
            cstr(&conf.units[d].name),
            cstr(&conf.units[s].name)
        ),
    );
    let len = name.iter().position(|&c| c == 0).unwrap_or(name.len());
    uaudio_mkname(conf, &name[..len], u);
}

/// `uaudio_mixer_nchan`: the number of mixer entries from `m` on that have the same name
/// (they control different channels of the same stream), and the first entry after them.
pub fn uaudio_mixer_nchan(list: &[UaudioMixent], m: usize) -> (usize, Option<usize>) {
    let name = list[m].fname;
    let mut k = m;
    while k < list.len() && fname_cmp(name, list[k].fname) == Ordering::Equal {
        k += 1;
    }
    (k - m, (k < list.len()).then_some(k))
}

/// `uaudio_mixer_skip`: skips the redundant mixer entries not exposed to userland: with an
/// all-channels control and per-channel controls of the same name, only the per-channel
/// ones are.
pub fn uaudio_mixer_skip(list: &[UaudioMixent], m: Option<usize>) -> Option<usize> {
    let mi = m?;
    if list[mi].chan == -1
        && mi + 1 < list.len()
        && fname_cmp(list[mi].fname, list[mi + 1].fname) == Ordering::Equal
    {
        return Some(mi + 1);
    }
    m
}

/// `uaudio_mixer_byindex`: the unit and mixer entry of the control the mixer(4) API calls
/// `index`.
pub fn uaudio_mixer_byindex(conf: &UaudioConf, index: i32) -> Option<(usize, usize)> {
    let mut i = UAUDIO_CLASS_COUNT;
    for u in conf.unit_iter() {
        let list = &conf.units[u].mixent_list;
        let mut m = (!list.is_empty()).then_some(0);
        loop {
            m = uaudio_mixer_skip(list, m);
            let Some(mi) = m else {
                break;
            };
            if index == i {
                return Some((u, mi));
            }
            if list[mi].type_ == UAUDIO_MIX_NUM {
                m = uaudio_mixer_nchan(list, mi).1;
            } else {
                m = (mi + 1 < list.len()).then_some(mi + 1);
            }
            i += 1;
        }
    }
    None
}

/// `uaudio_process_header`: parses the AC header descriptor, used only to determine the
/// UAC version. Other properties (like `wTotalLength`) can be determined from other
/// descriptors, so they are not relied on, avoiding inconsistencies and quirks.
pub fn uaudio_process_header(conf: &mut UaudioConf, p: &mut UaudioBlob<'_>) -> bool {
    let Some(mut ph) = uaudio_getdesc(p) else {
        return false;
    };
    let Some(type_) = uaudio_getnum(&mut ph, 1) else {
        return false;
    };
    if type_ != u32::from(UDESC_CS_INTERFACE) {
        return false;
    }
    let Some(subtype) = uaudio_getnum(&mut ph, 1) else {
        return false;
    };
    if subtype != UAUDIO_AC_HEADER {
        return false;
    }
    let Some(version) = uaudio_getnum(&mut ph, 2) else {
        return false;
    };
    conf.version = version;
    true
}

/// `uaudio_process_ac_ep`: skips the optional AC interrupt endpoint descriptor: none of
/// its properties are used, as the mixer interface does not support unsolicited state
/// changes yet.
pub fn uaudio_process_ac_ep(p: &mut UaudioBlob<'_>) -> bool {
    // parse optional interrupt endpoint descriptor
    if p.rptr == p.wptr {
        return true;
    }
    let savepos = p.rptr;
    let Some(mut dp) = uaudio_getdesc(p) else {
        return false;
    };
    let Some(type_) = uaudio_getnum(&mut dp, 1) else {
        return false;
    };
    if type_ != u32::from(UDESC_ENDPOINT) {
        p.rptr = savepos;
        return true;
    }

    // addr, attr, maxpkt, ival
    uaudio_getnum(&mut dp, 1).is_some()
        && uaudio_getnum(&mut dp, 1).is_some()
        && uaudio_getnum(&mut dp, 2).is_some()
        && uaudio_getnum(&mut dp, 1).is_some()
}

/// `uaudio_process_ac`: processes the AC interface descriptors: mainly builds the mixer
/// and, for UAC v2.0, finds the clock source.
///
/// The AC interface's descriptors expose the complete circuit of the device: how the
/// signal flows between the USB streaming interfaces and the terminal connectors (jacks,
/// speakers, mics, ...), through mixers, source selectors, gain controls, muters,
/// processors and alike, each with its own set of controls. Most of the work is to parse
/// the circuit and build a human-usable set of mixer(4) controls.
pub fn uaudio_process_ac<D: UaudioDev + ?Sized>(
    sc: &D,
    conf: &mut UaudioConf,
    p: &mut UaudioBlob<'_>,
    ifnum: u32,
) -> bool {
    conf.ctl_ifnum = ifnum;

    // The first AC class-specific descriptor is the AC header
    if !uaudio_process_header(conf, p) {
        return false;
    }

    // Determine the size of the AC descriptors array: scan descriptors until the first
    // non-class-specific descriptor. This avoids relying on the wTotalLength field.
    let savepos = p.rptr;
    let mut units = UaudioBlob {
        buf: p.buf,
        rptr: p.rptr,
        wptr: p.rptr,
    };
    while p.rptr != p.wptr {
        let Some(mut pu) = uaudio_getdesc(p) else {
            return false;
        };
        let Some(type_) = uaudio_getnum(&mut pu, 1) else {
            return false;
        };
        if type_ != u32::from(UDESC_CS_INTERFACE) {
            break;
        }
        units.wptr = p.rptr;
    }
    p.rptr = savepos;

    // Load units, walking from outputs to inputs, as the usb audio class spec requires.
    while p.rptr != units.wptr {
        let Some(mut pu) = uaudio_getdesc(p) else {
            return false;
        };
        let Some(_type) = uaudio_getnum(&mut pu, 1) else {
            return false;
        };
        let Some(subtype) = uaudio_getnum(&mut pu, 1) else {
            return false;
        };
        if subtype == UAUDIO_AC_OUTPUT {
            let Some(id) = uaudio_getnum(&mut pu, 1) else {
                return false;
            };
            if uaudio_process_unit(sc, conf, None, id, units).is_none() {
                return false;
            }
        }
    }

    // set terminal, effect, and processor unit names
    let list: Vec<usize> = conf.unit_iter().collect();
    for &u in &list {
        let (type_, term) = (conf.units[u].type_, conf.units[u].term);
        let templ = match type_ {
            UAUDIO_AC_INPUT => uaudio_tname(conf, term, false),
            UAUDIO_AC_OUTPUT => uaudio_tname(conf, term, true),
            UAUDIO_AC_CLKSRC => uaudio_clkname(term),
            UAUDIO_AC_CLKSEL => "clksel",
            UAUDIO_AC_EFFECT => "fx",
            UAUDIO_AC_PROCESSING => "proc",
            UAUDIO_AC_EXTENSION => "ext",
            _ => continue,
        };
        uaudio_mkname(conf, templ.as_bytes(), u);
    }

    // set mixer/selector unit names
    for &u in &list {
        let name = match conf.units[u].type_ {
            UAUDIO_AC_MIXER => "mix",
            UAUDIO_AC_SELECTOR => "sel",
            _ => continue,
        };
        if !uaudio_setname_dsts(conf, u, None) {
            uaudio_mkname(conf, name.as_bytes(), u);
        }
    }

    // set feature unit names and classes
    for &u in &list {
        if conf.units[u].type_ != UAUDIO_AC_FEATURE {
            continue;
        }
        conf.units[u].mixer_class = if uaudio_setname_dsts(conf, u, Some(UAUDIO_NAME_REC)) {
            UAUDIO_CLASS_IN
        } else if uaudio_setname_srcs(conf, u, Some(UAUDIO_NAME_PLAY))
            || uaudio_setname_dsts(conf, u, None)
        {
            UAUDIO_CLASS_OUT
        } else if uaudio_setname_srcs(conf, u, None) {
            UAUDIO_CLASS_IN
        } else {
            uaudio_setname_middle(conf, u);
            UAUDIO_CLASS_IN
        };
    }

    // follows optional interrupt endpoint descriptor
    if !uaudio_process_ac_ep(p) {
        return false;
    }

    // fetch clock source rates
    for &u in &list {
        match conf.units[u].type_ {
            UAUDIO_AC_CLKSRC => {
                let mut rates = core::mem::take(&mut conf.units[u].rates);
                let ok = uaudio_req_ranges(
                    sc,
                    conf,
                    4,
                    UAUDIO_V2_REQSEL_CLKFREQ,
                    0, // channel (not used)
                    conf.ctl_ifnum,
                    conf.units[u].id,
                    &mut rates,
                );
                conf.units[u].rates = rates;
                if !ok {
                    printf(format_args!(
                        "{}: failed to read clock rates\n",
                        sc.devname()
                    ));
                    return false;
                }
            }
            UAUDIO_AC_CLKSEL => {
                let mut val = [0u8; 1];
                if !sc.uaudio_req(
                    UT_READ_CLASS_INTERFACE,
                    UAUDIO_V2_REQ_CUR,
                    UAUDIO_V2_REQSEL_CLKSEL,
                    0,
                    conf.ctl_ifnum,
                    conf.units[u].id,
                    &mut val,
                ) {
                    printf(format_args!(
                        "{}: failed to read clock selector\n",
                        sc.devname()
                    ));
                    return false;
                }
                // the val-th source (1-based), counting down an unsigned byte
                let mut val = val[0];
                let mut v = conf.units[u].src_list;
                while let Some(vi) = v {
                    val = val.wrapping_sub(1);
                    if val == 0 {
                        break;
                    }
                    v = conf.units[vi].src_next;
                }
                conf.units[u].clock = v;
            }
            _ => {}
        }
    }

    if conf.version == UAUDIO_V2 {
        // Find the clocks for the usb-streaming terminal units
        for &u in &list {
            let unit = &conf.units[u];
            if unit.type_ == UAUDIO_AC_INPUT && unit.term >> 8 == 1 {
                if unit.clock.is_none() {
                    printf(format_args!("{}: no play clock\n", sc.devname()));
                    return false;
                }
                conf.pclock = unit.clock;
                break;
            }
        }

        for &u in &list {
            let unit = &conf.units[u];
            if unit.type_ == UAUDIO_AC_OUTPUT && unit.term >> 8 == 1 {
                if unit.clock.is_none() {
                    printf(format_args!("{}: no rec clock\n", sc.devname()));
                    return false;
                }
                conf.rclock = unit.clock;
                break;
            }
        }

        if !uaudio_clock_equiv(sc, conf, conf.pclock, conf.rclock) {
            return false;
        }
    }
    true
}

/// `uaudio_process_as_ep`: parses an AS endpoint descriptor (after its type byte).
///
/// For playback there is an output data endpoint of one of these types:
///
/// | type  | sync | |
/// |-------|------|-|
/// | async | yes  | the device uses its own clock but sends feedback on an (input) sync endpoint for the host to adjust the next packet size |
/// | sync  | -    | the data rate is constant, and the device is clocked to the USB bus |
/// | adapt | -    | the device adapts to the data rate of the host; with a fixed packet size the data rate is the USB clock, so this is the same as sync |
///
/// For recording there is an input data endpoint of one of these types:
///
/// | type  | sync | |
/// |-------|------|-|
/// | async | -    | the device uses its own clock and adjusts packet sizes |
/// | sync  | -    | the device uses the USB clock rate |
/// | adapt | yes  | the device uses the host's feedback (on a dedicated (output) sync endpoint) to adapt to the software's desired rate |
///
/// For USB 1.1, `ival` is hardcoded to 1 for isochronous transfers: one transfer every
/// frame period (1ms). For USB 2.0 the poll interval is `frame_period * 2^(ival - 1)`;
/// that formula works in all cases. The MaxPacketsOnly attribute is used only by "Type II"
/// encodings, so it does not matter here.
pub fn uaudio_process_as_ep<D: UaudioDev + ?Sized>(
    sc: &D,
    conf: &UaudioConf,
    p: &mut UaudioBlob<'_>,
    a: &mut UaudioAlt,
    nep: u32,
) -> bool {
    let Some(addr) = uaudio_getnum(p, 1) else {
        return false;
    };
    let Some(attr) = uaudio_getnum(p, 1) else {
        return false;
    };
    let Some(maxpkt) = uaudio_getnum(p, 2) else {
        return false;
    };
    // bInterval
    let Some(mut ival) = uaudio_getnum(p, 1) else {
        return false;
    };

    if ue_get_xfertype(attr as u8) != UE_ISOCHRONOUS {
        printf(format_args!("{}: skipped non-isoc endpt.\n", sc.devname()));
        return true;
    }

    // For isoc endpoints ival can't be 0. If it's 0, assume the descriptor is not set and
    // that ival corresponds to 1ms, which works for most (all?) devices.
    if ival == 0 {
        ival = if conf.ufps == 1000 { 1 } else { 4 };
    }

    // For each AS interface setting, there's a single data endpoint and an optional
    // feedback endpoint. The synchronization type is non-zero and must be set in the data
    // endpoints. However the isoc sync type field of the attributes can't be trusted: a lot
    // of devices have it wrong. If it is set, this is necessarily a data endpoint; if not,
    // and this is the only endpoint, it is necessarily the data endpoint.
    let isotype = ue_get_iso_type(attr as u8);
    if isotype != 0 || nep == 1 {
        // this is the data endpoint

        if a.data_addr != 0 && addr != a.data_addr {
            printf(format_args!(
                "{}: skipped extra data endpt.\n",
                sc.devname()
            ));
            return true;
        }

        // interval of more than 10ms makes no sense for audio
        let period = 1u32.checked_shl(ival - 1);
        let Some(period) = period.filter(|&t| t <= conf.ufps / 100) else {
            printf(format_args!(
                "{}: skipped endpt with huge ival\n",
                sc.devname()
            ));
            return true;
        };

        a.mode = if ue_get_dir(addr as u8) == UE_DIR_IN {
            AUMODE_RECORD
        } else {
            AUMODE_PLAY
        };
        a.data_addr = addr;
        a.fps = conf.ufps / period;
        a.maxpkt = u32::from(ue_get_size(maxpkt as u16));

        if ue_get_dir(addr as u8) == UE_DIR_IN {
            a.impl_fb = ue_get_iso_usage(attr) == UE_ISO_USAGE_IMPL;
        }
    } else {
        // this is the sync endpoint

        if a.sync_addr != 0 && addr != a.sync_addr {
            printf(format_args!(
                "{}: skipped extra sync endpt.\n",
                sc.devname()
            ));
            return true;
        }
        a.sync_addr = addr;
    }

    true
}

/// `uaudio_process_as_cs_ep`: parses an AS class-specific endpoint descriptor.
pub fn uaudio_process_as_cs_ep(
    conf: &UaudioConf,
    p: &mut UaudioBlob<'_>,
    a: &mut UaudioAlt,
    _nep: u32,
) -> bool {
    let Some(subtype) = uaudio_getnum(p, 1) else {
        return false;
    };
    if subtype != UAUDIO_AS_EP_GENERAL {
        return false;
    }
    let Some(attr) = uaudio_getnum(p, 1) else {
        return false;
    };
    if conf.version == UAUDIO_V1 {
        a.v1_cap_freqctl = attr & UAUDIO_EP_FREQCTL != 0;
    }
    true
}

/// `uaudio_process_as_general`: parses an AS general descriptor; `*rispcm` tells whether
/// the interface is PCM (others are skipped). UAC v2.0 reports the number of channels
/// here; for UAC v1.0 it comes from the format descriptor.
pub fn uaudio_process_as_general<D: UaudioDev + ?Sized>(
    sc: &D,
    conf: &UaudioConf,
    p: &mut UaudioBlob<'_>,
    rispcm: &mut bool,
    a: &mut UaudioAlt,
) -> bool {
    let Some(_term) = uaudio_getnum(p, 1) else {
        return false;
    };
    match conf.version {
        UAUDIO_V1 => {
            // bDelay
            if uaudio_getnum(p, 1).is_none() {
                return false;
            }
            let Some(fmt) = uaudio_getnum(p, 1) else {
                return false;
            };
            *rispcm = fmt == UAUDIO_V1_FMT_PCM;
        }
        UAUDIO_V2 => {
            // XXX: should we check if alt setting control is valid ?
            let Some(_ctl) = uaudio_getnum(p, 1) else {
                return false;
            };
            let Some(fmt_type) = uaudio_getnum(p, 1) else {
                return false;
            };
            let Some(fmt_map) = uaudio_getnum(p, 4) else {
                return false;
            };
            let Some(nch) = uaudio_getnum(p, 1) else {
                return false;
            };
            if nch == 0 {
                printf(format_args!("{}: skipped 0-chan v2 alt\n", sc.devname()));
                *rispcm = false;
                return true;
            }
            a.nch = nch;
            *rispcm = fmt_type == 1 && fmt_map & UAUDIO_V2_FMT_PCM != 0;
        }
        _ => {}
    }
    true
}

/// `uaudio_process_as_format`: parses an AS format descriptor. Only "Type 1" formats
/// (PCM) are supported: the others are not really audio but data-only interfaces, for
/// which ethernet is much better.
///
/// XXX: handle ieee 754 32-bit floating point formats.
pub fn uaudio_process_as_format<D: UaudioDev + ?Sized>(
    sc: &D,
    conf: &UaudioConf,
    p: &mut UaudioBlob<'_>,
    a: &mut UaudioAlt,
    ispcm: &mut bool,
) -> bool {
    let (bps, bits);

    match conf.version {
        UAUDIO_V1 => {
            let Some(type_) = uaudio_getnum(p, 1) else {
                return false;
            };
            if type_ != 1 {
                *ispcm = false;
                return true;
            }
            let Some(nch) = uaudio_getnum(p, 1) else {
                return false;
            };
            let Some(b) = uaudio_getnum(p, 1) else {
                return false;
            };
            bps = b;
            let Some(b) = uaudio_getnum(p, 1) else {
                return false;
            };
            bits = b;
            let Some(nrates) = uaudio_getnum(p, 1) else {
                return false;
            };
            let mut rates = 0i32;
            if nrates == 0 {
                let Some(rate_min) = uaudio_getnum(p, 3) else {
                    return false;
                };
                let Some(rate_max) = uaudio_getnum(p, 3) else {
                    return false;
                };
                for (i, &r) in UAUDIO_RATES.iter().enumerate() {
                    if r as u32 >= rate_min && r as u32 <= rate_max {
                        rates |= 1 << i;
                    }
                }
            } else {
                for _ in 0..nrates {
                    let Some(rate) = uaudio_getnum(p, 3) else {
                        return false;
                    };
                    for (i, &r) in UAUDIO_RATES.iter().enumerate() {
                        if r as u32 == rate {
                            rates |= 1 << i;
                        }
                    }
                }
            }
            if nch == 0 {
                printf(format_args!("{}: skipped 0-chan v1 alt\n", sc.devname()));
                *ispcm = false;
                return true;
            }
            a.v1_rates = rates;
            a.nch = nch;
        }
        UAUDIO_V2 => {
            // Sample rate ranges are obtained with requests to the clock source, as
            // defined by the clock source descriptor; the number of channels is in the
            // GENERAL descriptor.
            let Some(type_) = uaudio_getnum(p, 1) else {
                return false;
            };
            if type_ != 1 {
                *ispcm = false;
                return true;
            }
            let Some(b) = uaudio_getnum(p, 1) else {
                return false;
            };
            bps = b;
            let Some(b) = uaudio_getnum(p, 1) else {
                return false;
            };
            bits = b;
        }
        // see the module's deviations
        _ => return false,
    }
    if bps == 0 || bps > 4 || bits == 0 || bits > bps * 8 {
        printf(format_args!(
            "{}: s{}le{}: fmt skipped\n",
            sc.devname(),
            bits,
            bps
        ));
        *ispcm = false;
        return false;
    }
    a.bps = bps;
    a.bits = bits;
    *ispcm = true;
    true
}

/// `uaudio_process_as`: parses the descriptors of an AS alternate setting and adds it to
/// `alts`.
///
/// The AS interfaces move data between the host and the device. The device's
/// analog-to-digital and digital-to-analog converters have their own low-jitter clock
/// source, while the USB host runs a bus clock from another; both drift, so the device
/// sends feedback for the host to adjust its data rate continuously, hence the sync
/// endpoints.
pub fn uaudio_process_as<D: UaudioDev + ?Sized>(
    sc: &D,
    conf: &mut UaudioConf,
    p: &mut UaudioBlob<'_>,
    ifnum: u32,
    altnum: u32,
    nep: u32,
) -> bool {
    let mut ispcm = false;
    let mut a = UaudioAlt {
        ifnum,
        altnum,
        ..UaudioAlt::default()
    };

    while p.rptr != p.wptr {
        let savep = p.rptr;
        let Some(mut dp) = uaudio_getdesc(p) else {
            return false;
        };
        let Some(type_) = uaudio_getnum(&mut dp, 1) else {
            return false;
        };
        if type_ != u32::from(UDESC_CS_INTERFACE) {
            p.rptr = savep;
            break;
        }
        let Some(subtype) = uaudio_getnum(&mut dp, 1) else {
            return false;
        };
        match subtype {
            UAUDIO_AS_GENERAL => {
                if !uaudio_process_as_general(sc, conf, &mut dp, &mut ispcm, &mut a) {
                    return false;
                }
            }
            UAUDIO_AS_FORMAT => {
                if !uaudio_process_as_format(sc, conf, &mut dp, &mut a, &mut ispcm) {
                    return false;
                }
            }
            // unknown desc
            _ => continue,
        }
        if !ispcm {
            // non-pcm iface
            return true;
        }
    }

    while p.rptr != p.wptr {
        let savep = p.rptr;
        let Some(mut dp) = uaudio_getdesc(p) else {
            return false;
        };
        let Some(type_) = uaudio_getnum(&mut dp, 1) else {
            return false;
        };
        if type_ == u32::from(UDESC_CS_ENDPOINT) {
            if !uaudio_process_as_cs_ep(conf, &mut dp, &mut a, nep) {
                return false;
            }
        } else if type_ == u32::from(UDESC_ENDPOINT) {
            if !uaudio_process_as_ep(sc, conf, &mut dp, &mut a, nep) {
                return false;
            }
        } else {
            p.rptr = savep;
            break;
        }
    }

    if a.mode == 0 {
        printf(format_args!("{}: no data endpoints found\n", sc.devname()));
        return true;
    }

    // Append to the list of alts, keeping it sorted by number of channels, bits and rate:
    // from the most capable to the least capable.
    let mut at = conf.alts.len();
    for (k, anext) in conf.alts.iter().enumerate() {
        let better_fmt =
            a.bits > anext.bits || (conf.version == UAUDIO_V1 && a.v1_rates > anext.v1_rates);
        if a.nch > anext.nch || (a.nch == anext.nch && better_fmt) {
            at = k;
            break;
        }
    }
    conf.alts.insert(at, a);
    true
}

/// `uaudio_fixup_params`: fills `params_list` with the combinations of play and record
/// alternate settings that work together in full-duplex, or, for unidirectional devices,
/// with the play-only and record-only ones.
pub fn uaudio_fixup_params(conf: &mut UaudioConf) {
    // Add full-duplex parameter combinations.
    for (pi, ap) in conf.alts.iter().enumerate() {
        if ap.mode != AUMODE_PLAY {
            continue;
        }
        for (ri, ar) in conf.alts.iter().enumerate() {
            if ar.mode != AUMODE_RECORD {
                continue;
            }
            if ar.bps != ap.bps || ar.bits != ap.bits {
                continue;
            }
            let rates = match conf.version {
                UAUDIO_V1 => {
                    let rates = ap.v1_rates & ar.v1_rates;
                    if rates == 0 {
                        continue;
                    }
                    rates
                }
                // UAC v2.0 common rates
                _ => 0,
            };
            conf.params_list.push(UaudioParams {
                palt: Some(pi),
                ralt: Some(ri),
                v1_rates: rates,
            });
        }
    }

    // For unidirectional devices, add play-only and or rec-only parameters.
    if conf.params_list.is_empty() {
        for (i, a) in conf.alts.iter().enumerate() {
            let (palt, ralt) = if a.mode == AUMODE_PLAY {
                (Some(i), None)
            } else {
                (None, Some(i))
            };
            conf.params_list.push(UaudioParams {
                palt,
                ralt,
                v1_rates: a.v1_rates,
            });
        }
    }
}

/// `uaudio_iface_index`: the index in the device's interfaces of interface number
/// `ifnum`, -1 when it has none.
pub fn uaudio_iface_index<D: UaudioDev + ?Sized>(sc: &D, ifnum: u32) -> i32 {
    for i in 0..sc.nifaces() {
        if sc.iface_number(i) == ifnum {
            return i as i32;
        }
    }

    printf(format_args!(
        "uaudio_iface_index: {ifnum}: invalid interface number\n"
    ));
    -1
}

/// `uaudio_process_conf`: parses all descriptors and builds the configuration of the
/// device, then claims the interfaces it uses, which keeps other uaudio(4) devices from
/// trying to use them.
pub fn uaudio_process_conf<D: UaudioDev + ?Sized>(
    sc: &D,
    conf: &mut UaudioConf,
    p: &mut UaudioBlob<'_>,
) -> bool {
    'parse: while p.rptr != p.wptr {
        let Some(mut dp) = uaudio_getdesc(p) else {
            return false;
        };
        let Some(type_) = uaudio_getnum(&mut dp, 1) else {
            return false;
        };
        if type_ != u32::from(UDESC_INTERFACE) {
            continue;
        }
        let (Some(ifnum), Some(altnum), Some(nep), Some(class), Some(subclass)) = (
            uaudio_getnum(&mut dp, 1),
            uaudio_getnum(&mut dp, 1),
            uaudio_getnum(&mut dp, 1),
            uaudio_getnum(&mut dp, 1),
            uaudio_getnum(&mut dp, 1),
        ) else {
            return false;
        };
        if class != u32::from(UICLASS_AUDIO) {
            continue;
        }

        match subclass {
            s if s == u32::from(UISUBCLASS_AUDIOCONTROL) => {
                let i = uaudio_iface_index(sc, ifnum);
                if i != -1 && sc.iface_claimed(i as usize) {
                    // AC already claimed
                    conf.instnum += 1;
                    continue;
                }
                if conf.unit_list.is_some() {
                    // >1 AC ifaces
                    break 'parse;
                }
                if !uaudio_process_ac(sc, conf, p, ifnum) {
                    return false;
                }
            }
            s if s == u32::from(UISUBCLASS_AUDIOSTREAM) => {
                let i = uaudio_iface_index(sc, ifnum);
                if i != -1 && sc.iface_claimed(i as usize) {
                    // AS already claimed
                    continue;
                }
                if nep == 0 {
                    // 0 is "stop sound", skip it
                    continue;
                }
                if !uaudio_process_as(sc, conf, p, ifnum, altnum, nep) {
                    return false;
                }
            }
            _ => {}
        }
    }

    uaudio_fixup_params(conf);

    // Claim all interfaces we use. This prevents other uaudio(4) devices from trying to use
    // them.
    for k in 0..conf.alts.len() {
        let i = uaudio_iface_index(sc, conf.alts[k].ifnum);
        if i != -1 {
            sc.claim_iface(i as usize);
        }
    }

    let i = uaudio_iface_index(sc, conf.ctl_ifnum);
    if i != -1 {
        sc.claim_iface(i as usize);
    }

    true
}

/// `uaudio_xfer_alloc`: allocates an isochronous transfer and its bounce buffers for at
/// most `count` frames of `framesize` bytes.
pub fn uaudio_xfer_alloc(
    sc: &UaudioSoftc,
    xfer: &UaudioXfer,
    framesize: u32,
    count: u32,
) -> Result<(), Errno> {
    let Some(x) = usbd_alloc_xfer(sc.udev()) else {
        return Err(Errno::ENOMEM);
    };
    xfer.usb_xfer.set(Some(x));

    let Some(buf) = usbd_alloc_buffer(x, framesize.wrapping_mul(count)) else {
        return Err(Errno::ENOMEM);
    };
    xfer.buf.set(buf.as_ptr());
    xfer.buflen.set(framesize.wrapping_mul(count));

    let Some(sizes) = mallocarray(count as usize, size_of::<u16>(), M_USBDEV, M_WAITOK) else {
        return Err(Errno::ENOMEM);
    };
    xfer.sizes.set(sizes.as_ptr().cast::<u16>());
    xfer.nsizes.set(count);

    Ok(())
}

/// `uaudio_xfer_free`: frees an isochronous transfer and its bounce buffers.
pub fn uaudio_xfer_free(_sc: &UaudioSoftc, xfer: &UaudioXfer, count: u32) {
    if let Some(x) = xfer.usb_xfer.take() {
        // frees request buffer as well
        // SAFETY: the xfer came from `usbd_alloc_xfer` in `uaudio_xfer_alloc`; its pipe was
        // closed first (`uaudio_stream_close`), so it is not queued, and the cell no longer
        // names it.
        unsafe { usbd_free_xfer(NonNull::from(x)) };
        xfer.buf.set(ptr::null_mut());
        xfer.buflen.set(0);
    }
    if let Some(sizes) = NonNull::new(xfer.sizes.get()) {
        free(
            sizes.cast::<u8>(),
            M_USBDEV,
            size_of::<u16>() * count as usize,
        );
        xfer.sizes.set(ptr::null_mut());
        xfer.nsizes.set(0);
    }
}

/// `uaudio_stream_close`: closes a stream and frees all associated resources.
pub fn uaudio_stream_close(sc: &UaudioSoftc, dir: i32) {
    let (s, a) = if dir == AUMODE_PLAY {
        (&sc.pstream, sc.palt())
    } else {
        (&sc.rstream, sc.ralt())
    };

    if let Some(pipe) = s.data_pipe.take() {
        // SAFETY: opened by `uaudio_stream_open`, and the cell no longer names it.
        let _ = unsafe { usbd_close_pipe(NonNull::from(pipe)) };
    }

    if let Some(pipe) = s.sync_pipe.take() {
        // SAFETY: as above.
        let _ = unsafe { usbd_close_pipe(NonNull::from(pipe)) };
    }

    match usbd_device2interface_handle(sc.udev(), a.ifnum as u8) {
        Err(_) => {
            printf(format_args!("{}: can't get iface handle\n", sc.dev.xname()));
        }
        Ok(iface) => {
            if usbd_set_interface(iface, 0).is_err() {
                printf(format_args!("{}: can't reset interface\n", sc.dev.xname()));
            }
        }
    }

    for i in 0..UAUDIO_NXFERS_MAX {
        uaudio_xfer_free(sc, &s.data_xfers[i], s.nframes_max.get());
        uaudio_xfer_free(sc, &s.sync_xfers[i], 1);
    }
}

/// The sizing part of `uaudio_stream_open`: the samples per frame and their bounds, the
/// safe block size, the frames and bytes a transfer may need and the number of transfers,
/// for `blksz`-byte blocks at `rate` over alternate setting `a`; `EIO` when the device's
/// packets or the host's frames cannot carry it.
pub fn uaudio_stream_calc(
    devname: &str,
    s: &UaudioStream,
    a: &UaudioAlt,
    rate: u32,
    host_nframes: u32,
    blksz: usize,
) -> Result<(), Errno> {
    s.nframes_mask.set(0);
    let mut i = a.fps;
    while i > 1000 {
        s.nframes_mask.set((s.nframes_mask.get() << 1) | 1);
        i >>= 1;
    }

    // bytes per audio frame
    let bpa = a.bps * a.nch;

    // ideal samples per usb frame, fixed-point
    s.spf
        .set((u64::from(rate) * u64::from(UAUDIO_SPF_DIV) / u64::from(a.fps)) as u32);

    // The UAC2.0 spec allows 1000PPM tolerance in sample frequency, while USB1.1 requires
    // 1Hz, which is 125PPM at 8kHz. As much as 1/256, which is 2500PPM, is accepted.
    s.spf_min.set((u64::from(s.spf.get()) * 255 / 256) as u32);
    s.spf_max.set((u64::from(s.spf.get()) * 257 / 256) as u32);

    // max spf can't exceed the device usb packet size
    let spf_max = (a.maxpkt / bpa).wrapping_mul(UAUDIO_SPF_DIV);
    if s.spf.get() > spf_max {
        printf(format_args!("{devname}: samples per frame too large\n"));
        return Err(Errno::EIO);
    }
    if s.spf_max.get() > spf_max {
        s.spf_max.set(spf_max);
    }

    // Upon transfer completion the device must reach the audio block boundary, which is
    // propagated to upper layers. In the worst case, only frames of spf_max samples are
    // scheduled, but the device returns only frames of spf_min samples; then the amount
    // actually transferred is at least
    //
    //     min_blksz = blksz / spf_max * spf_min
    //
    // and, with UAUDIO_NXFERS outstanding blocks, the worst-case remaining bytes are at most
    //
    //     UAUDIO_NXFERS * (blksz - min_blksz)
    let mut min_blksz = ((((blksz as u64) << 32) / u64::from(s.spf_max.get())
        * u64::from(s.spf_min.get()))
        >> 32) as u32;

    // round to sample size
    min_blksz -= min_blksz % bpa;

    // finally this is what ensures we cross block boundary
    s.safe_blksz.set((blksz as i32).wrapping_add(
        (UAUDIO_NXFERS_MAX as i32).wrapping_mul((blksz as i32).wrapping_sub(min_blksz as i32)),
    ));

    // max number of (micro-)frames we'll ever use
    let nframes_max = (u64::from(s.safe_blksz.get() as u32 / bpa) * u64::from(UAUDIO_SPF_DIV)
        / u64::from(s.spf_min.get())
        + 1) as u32;

    // round to next usb1.1 frame
    let mask = s.nframes_mask.get();
    s.nframes_max.set((nframes_max + mask) & !mask);

    // this is the max packet size we'll ever need
    s.maxpkt.set(bpa * s.spf_max.get().div_ceil(UAUDIO_SPF_DIV));

    // how many xfers we need to fill sc->host_nframes
    s.nxfers
        .set((host_nframes / s.nframes_max.get()).min(UAUDIO_NXFERS_MAX as u32));

    if s.nxfers.get() < UAUDIO_NXFERS_MIN {
        printf(format_args!("{devname}: block size too large\n"));
        return Err(Errno::EIO);
    }

    // Require at least 2ms block size to ensure no transfer exceeds two blocks.
    //
    // XXX: use s->nframes_mask instead of 1000
    if 1000 * blksz < 2u32.wrapping_mul(rate).wrapping_mul(bpa) as usize {
        printf(format_args!("{devname}: audio block too small\n"));
        return Err(Errno::EIO);
    }

    Ok(())
}

/// `uaudio_stream_open`: opens a stream with the given buffer settings and sets the
/// current interface alternate setting.
#[allow(clippy::too_many_arguments)] // the C's signature
pub fn uaudio_stream_open(
    sc: &UaudioSoftc,
    dir: i32,
    start: *mut u8,
    end: *mut u8,
    blksz: usize,
    intr: AudioIntr,
    arg: *mut c_void,
) -> Result<(), Errno> {
    let conf = sc.conf();
    let (s, a, sync) = if dir == AUMODE_PLAY {
        let a = sc.palt();
        let sync = a.sync_addr != 0 && !(sc.mode.get() & AUMODE_RECORD != 0 && sc.ralt().impl_fb);
        (&sc.pstream, a, sync)
    } else {
        (&sc.rstream, sc.ralt(), false)
    };

    for i in 0..UAUDIO_NXFERS_MAX {
        s.data_xfers[i].usb_xfer.set(None);
        s.data_xfers[i].sizes.set(ptr::null_mut());
        s.sync_xfers[i].usb_xfer.set(None);
        s.sync_xfers[i].sizes.set(ptr::null_mut());
    }
    s.data_pipe.set(None);
    s.sync_pipe.set(None);

    uaudio_stream_calc(
        sc.dev.xname(),
        s,
        a,
        sc.rate.get(),
        sc.host_nframes.get(),
        blksz,
    )?;

    if uaudio_stream_setup(sc, conf, dir, s, a, sync).is_err() {
        // failed:
        uaudio_stream_close(sc, dir);
        return Err(Errno::ENOMEM);
    }

    s.data_nextxfer.set(0);
    s.sync_nextxfer.set(0);
    s.spf_remain.set(0);

    s.intr.set(Some(intr));
    s.arg.set(arg);
    s.ring_start.set(start);
    s.ring_end.set(end);
    s.ring_blksz.set(blksz as i32);

    s.ring_pos.set(start);
    s.ring_offs.set(0);
    s.ring_icnt.set(0);

    s.ubuf_xfer.set(0);
    s.ubuf_pos.set(0);
    Ok(())
}

/// The part of `uaudio_stream_open` that can fail into its `failed:` label: the transfers,
/// the sample rate, the alternate setting and the pipes.
fn uaudio_stream_setup(
    sc: &UaudioSoftc,
    conf: &UaudioConf,
    dir: i32,
    s: &UaudioStream,
    a: &UaudioAlt,
    sync: bool,
) -> Result<(), Errno> {
    for i in 0..s.nxfers.get() as usize {
        uaudio_xfer_alloc(sc, &s.data_xfers[i], s.maxpkt.get(), s.nframes_max.get())?;
        if sync {
            uaudio_xfer_alloc(sc, &s.sync_xfers[i], sc.sync_pktsz.get(), 1)?;
        }
    }

    let Ok(iface) = usbd_device2interface_handle(sc.udev(), a.ifnum as u8) else {
        printf(format_args!("{}: can't get iface handle\n", sc.dev.xname()));
        return Err(Errno::ENOMEM);
    };

    // Set the sample rate and the alternate setting.
    //
    // Unlike UAC1 devices, UAC2 devices set their sample rate with their clock unit, which
    // is independent of the alternate setting. It makes more sense to set the sample rate
    // before the alternate setting, and certain devices require it.
    //
    // Certain devices are able to lock their clock to the data rate and expose no
    // frequency control. In this case, the request to set the frequency would fail and
    // freeze the device.
    let rate = sc.rate.get();
    match conf.version {
        UAUDIO_V1 => {
            if usbd_set_interface(iface, a.altnum as i32).is_err() {
                printf(format_args!("{}: can't set interface\n", sc.dev.xname()));
                return Err(Errno::ENOMEM);
            }
            if a.v1_cap_freqctl {
                let mut req_buf = [rate as u8, (rate >> 8) as u8, (rate >> 16) as u8];
                if !sc.uaudio_req(
                    UT_WRITE_CLASS_ENDPOINT,
                    UAUDIO_V1_REQ_SET_CUR,
                    UAUDIO_REQSEL_RATE,
                    0,
                    a.data_addr,
                    0,
                    &mut req_buf,
                ) {
                    printf(format_args!(
                        "{}: failed to set endpoint rate\n",
                        sc.dev.xname()
                    ));
                }
            }
        }
        UAUDIO_V2 => {
            let Some(clock) = uaudio_clock(
                conf,
                if dir == AUMODE_PLAY {
                    conf.pclock
                } else {
                    conf.rclock
                },
            ) else {
                printf(format_args!("{}: can't get clock\n", sc.dev.xname()));
                return Err(Errno::ENOMEM);
            };
            let clock = &conf.units[clock];
            if clock.cap_freqctl {
                let mut req_buf = rate.to_le_bytes();
                if !sc.uaudio_req(
                    UT_WRITE_CLASS_INTERFACE,
                    UAUDIO_V2_REQ_CUR,
                    UAUDIO_REQSEL_RATE,
                    0,
                    conf.ctl_ifnum,
                    clock.id,
                    &mut req_buf,
                ) {
                    printf(format_args!(
                        "{}: failed to set clock rate\n",
                        sc.dev.xname()
                    ));
                }
            }
            if usbd_set_interface(iface, a.altnum as i32).is_err() {
                printf(format_args!("{}: can't set interface\n", sc.dev.xname()));
                return Err(Errno::ENOMEM);
            }
        }
        _ => {}
    }

    match usbd_open_pipe(iface, a.data_addr as u8, 0) {
        Ok(pipe) => s.data_pipe.set(Some(pipe)),
        Err(_) => {
            printf(format_args!("{}: can't open data pipe\n", sc.dev.xname()));
            return Err(Errno::ENOMEM);
        }
    }

    if sync {
        match usbd_open_pipe(iface, a.sync_addr as u8, 0) {
            Ok(pipe) => s.sync_pipe.set(Some(pipe)),
            Err(_) => {
                printf(format_args!("{}: can't open sync pipe\n", sc.dev.xname()));
                return Err(Errno::ENOMEM);
            }
        }
    }
    Ok(())
}

/// `uaudio_adjspf`: adjusts the play samples-per-frame to keep the play and record
/// streams in sync.
pub fn uaudio_adjspf(sc: &UaudioSoftc) {
    let s = &sc.pstream;

    if sc.mode.get() != (AUMODE_RECORD | AUMODE_PLAY) {
        return;
    }
    if s.sync_pipe.get().is_some() {
        return;
    }

    // number of samples play stream is ahead of record stream.
    let mut diff = sc.diff_nsamp.get();
    let nframes = sc.diff_nframes.get();
    if nframes > 0 {
        let d = u64::from(sc.pstream.spf.get()) * nframes as u64 / u64::from(UAUDIO_SPF_DIV);
        diff = diff.wrapping_sub(d as i32);
    } else {
        let d = u64::from(sc.rstream.spf.get()) * u64::from(nframes.unsigned_abs())
            / u64::from(UAUDIO_SPF_DIV);
        diff = diff.wrapping_add(d as i32);
    }

    // adjust samples-per-frames to resync within the next second
    let spf = (u64::from(sc.rate.get().wrapping_sub(diff as u32)) * u64::from(UAUDIO_SPF_DIV)
        / u64::from(sc.ufps.get())) as u32;
    if spf > s.spf_max.get() {
        s.spf.set(s.spf_max.get());
    } else if spf < s.spf_min.get() {
        s.spf.set(s.spf_min.get());
    } else {
        s.spf.set(spf);
    }
}

/// `uaudio_pdata_copy`: copies what audio(4) produced (`copy_todo` bytes) from the ring
/// to the transfer buffers.
pub fn uaudio_pdata_copy(sc: &UaudioSoftc) {
    let s = &sc.pstream;

    while sc.copy_todo.get() > 0 && s.ubuf_xfer.get() < s.nxfers.get() {
        let mut index = s.data_nextxfer.get() + s.ubuf_xfer.get();
        if index >= s.nxfers.get() {
            index -= s.nxfers.get();
        }
        let xfer = &s.data_xfers[index as usize];
        let avail = s.ring_end.get() as usize - s.ring_pos.get() as usize;
        let ubuf_pos = s.ubuf_pos.get() as usize;
        let count = (xfer.size.get() as usize - ubuf_pos)
            .min(avail)
            .min(sc.copy_todo.get());
        // SAFETY: the buffer is this stream's and no other reference to it is live; the
        // ring is audio(4)'s, `count` bytes from `ring_pos` are within `[ring_start,
        // ring_end)`, written by audio(4) before `copy_output`/`underrun` announced them.
        unsafe {
            let dst = &mut xfer.buf()[ubuf_pos..ubuf_pos + count];
            ptr::copy_nonoverlapping(s.ring_pos.get(), dst.as_mut_ptr(), count);
        }
        sc.copy_todo.set(sc.copy_todo.get() - count);
        // SAFETY: `count <= avail`: at most `ring_end`.
        s.ring_pos.set(unsafe { s.ring_pos.get().add(count) });
        if s.ring_pos.get() == s.ring_end.get() {
            s.ring_pos.set(s.ring_start.get());
        }
        s.ubuf_pos.set((ubuf_pos + count) as u32);
        if s.ubuf_pos.get() == xfer.size.get() {
            if let Some(x) = xfer.usb_xfer.get() {
                usb_syncmem(&x.dmabuf, 0, xfer.size.get() as usize, BUS_DMASYNC_PREWRITE);
            }
            s.ubuf_pos.set(0);
            #[cfg(feature = "diagnostic")]
            if s.ubuf_xfer.get() == s.nxfers.get() {
                printf(format_args!("uaudio_pdata_copy: overflow\n"));
                return;
            }
            s.ubuf_xfer.set(s.ubuf_xfer.get() + 1);
        }
    }
}

/// `uaudio_pdata_calcsizes`: calculates and fills the frame sizes of a play transfer.
pub fn uaudio_pdata_calcsizes(sc: &UaudioSoftc, xfer: &UaudioXfer) {
    let s = &sc.pstream;
    let a = sc.palt();
    let bpf = a.bps * a.nch;
    let mut done = s.ring_offs.get();
    let sizes = xfer.sizes();
    xfer.nframes.set(0);

    loop {
        let nframes = xfer.nframes.get();

        // if we crossed the next block boundary, we're done
        if nframes & s.nframes_mask.get() == 0 && done > s.safe_blksz.get() {
            break;
        }

        // this can't happen, debug only
        if nframes == s.nframes_max.get() {
            printf(format_args!(
                "{}: too many frames for play xfer: done = {}, blksz = {}\n",
                sc.dev.xname(),
                done as u32,
                s.ring_blksz.get()
            ));
            break;
        }

        // calculate frame size and adjust state
        s.spf_remain.set(s.spf_remain.get() + s.spf.get());
        let fsize = s.spf_remain.get() / UAUDIO_SPF_DIV * bpf;
        s.spf_remain.set(s.spf_remain.get() % UAUDIO_SPF_DIV);
        done = done.wrapping_add(fsize as i32);
        sizes[nframes as usize].set(fsize as u16);
        xfer.nframes.set(nframes + 1);
    }

    xfer.size.set(done.wrapping_sub(s.ring_offs.get()) as u32);
    s.ring_offs.set(done.wrapping_sub(s.ring_blksz.get()));

    // SAFETY: the buffer is this stream's and no other reference to it is live.
    unsafe { xfer.buf()[..xfer.size.get() as usize].fill(0) };
}

/// `uaudio_pdata_xfer`: submits a play data transfer to the USB driver.
pub fn uaudio_pdata_xfer(sc: &UaudioSoftc) {
    let s = &sc.pstream;
    let xfer = &s.data_xfers[s.data_nextxfer.get() as usize];

    // this can't happen, debug only
    if xfer.nframes.get() == 0 {
        printf(format_args!("{}: zero frame play xfer\n", sc.dev.xname()));
        return;
    }

    uaudio_isoc_start(sc, xfer, s.data_pipe.get(), uaudio_pdata_intr, "play xfer");

    s.data_nextxfer.set(s.data_nextxfer.get() + 1);
    if s.data_nextxfer.get() == s.nxfers.get() {
        s.data_nextxfer.set(0);
    }
}

/// The `usbd_setup_isoc_xfer` + `usbd_transfer` pair the three `*_xfer` functions share,
/// with their error message. Short transfers are accepted, as a transfer with babble or
/// stale frames is short.
fn uaudio_isoc_start(
    sc: &UaudioSoftc,
    xfer: &UaudioXfer,
    pipe: Option<&'static UsbdPipe>,
    cb: fn(&'static UsbdXfer, *mut c_void, UsbdStatus),
    what: &str,
) {
    let (Some(x), Some(pipe)) = (xfer.usb_xfer.get(), pipe) else {
        return;
    };
    // SAFETY: `sizes` holds `nsizes >= nframes` entries that stay allocated until the
    // stream is closed, which aborts the transfer first.
    unsafe {
        usbd_setup_isoc_xfer(
            x,
            pipe,
            ptr::from_ref(sc).cast_mut().cast(),
            xfer.sizes.get(),
            xfer.nframes.get(),
            USBD_NO_COPY | USBD_SHORT_XFER_OK,
            Some(cb),
        )
    };

    let err = usbd_transfer(x);
    if err != USBD_NORMAL_COMPLETION && err != USBD_IN_PROGRESS {
        printf(format_args!(
            "{}: {what}, err = {}\n",
            sc.dev.xname(),
            err as i32
        ));
    }
}

/// `uaudio_pdata_intr`: the USB driver's completion call-back of a play data transfer.
pub fn uaudio_pdata_intr(usb_xfer: &'static UsbdXfer, arg: *mut c_void, status: UsbdStatus) {
    let sc = uaudio_xfer_sc(arg);
    let s = &sc.pstream;

    if status != USBD_NORMAL_COMPLETION && status != USBD_IOERROR {
        return;
    }

    if sc.trigger_mode.get() & AUMODE_PLAY == 0 {
        // halted
        return;
    }

    let xfer = &s.data_xfers[s.data_nextxfer.get() as usize];
    if !xfer.usb_xfer.get().is_some_and(|x| ptr::eq(x, usb_xfer)) {
        // wrong xfer
        return;
    }

    let a = sc.palt();
    sc.diff_nsamp.set(
        sc.diff_nsamp
            .get()
            .wrapping_add((xfer.size.get() / (a.nch * a.bps)) as i32),
    );
    sc.diff_nframes.set(
        sc.diff_nframes
            .get()
            .wrapping_add(xfer.nframes.get() as i32),
    );

    let mut size = 0u32;
    usbd_get_xfer_status(usb_xfer, None, None, Some(&mut size), None);
    // A short transfer (`size != xfer->size`) is accepted: the upper layer gets the
    // whole block.

    // The upper layer call-back may call uaudio_underrun(), which needs the current size
    // of this transfer. So the sizes are not recalculated, and the transfer not scheduled
    // yet.
    s.ring_icnt
        .set(s.ring_icnt.get().wrapping_add(xfer.size.get() as i32));
    let nintr = uaudio_block_intrs(s);
    if nintr != 1 {
        printf(format_args!(
            "uaudio_pdata_intr: {nintr}: bad play intr count\n"
        ));
    }

    uaudio_pdata_calcsizes(sc, xfer);
    uaudio_pdata_xfer(sc);
    #[cfg(feature = "diagnostic")]
    if s.ubuf_xfer.get() == 0 {
        printf(format_args!("uaudio_pdata_intr: underflow\n"));
        return;
    }
    s.ubuf_xfer.set(s.ubuf_xfer.get().wrapping_sub(1));
    uaudio_pdata_copy(sc);
}

/// The loop of `uaudio_pdata_intr` and `uaudio_rdata_intr` that calls audio(4)'s
/// call-back once per completed block, under `audio_lock`; the number of calls.
fn uaudio_block_intrs(s: &UaudioStream) -> i32 {
    let mut nintr = 0;
    mtx_enter(&AUDIO_LOCK);
    while s.ring_icnt.get() >= s.ring_blksz.get() {
        if let Some(intr) = s.intr.get() {
            // SAFETY: `intr` and `arg` are the pair audio(4) handed to the trigger
            // method, and `AUDIO_LOCK` is held, as the call-back requires.
            unsafe { intr(s.arg.get()) };
        }
        s.ring_icnt
            .set(s.ring_icnt.get().wrapping_sub(s.ring_blksz.get()));
        nintr += 1;
    }
    mtx_leave(&AUDIO_LOCK);
    nintr
}

/// `uaudio_psync_xfer`: submits a play sync (feedback) transfer to the USB driver.
pub fn uaudio_psync_xfer(sc: &UaudioSoftc) {
    let s = &sc.pstream;
    let xfer = &s.sync_xfers[s.sync_nextxfer.get() as usize];
    xfer.nframes.set(1);

    for size in xfer.sizes().iter().take(xfer.nframes.get() as usize) {
        size.set(sc.sync_pktsz.get() as u16);
    }

    xfer.size.set(xfer.nframes.get() * sc.sync_pktsz.get());

    uaudio_isoc_start(
        sc,
        xfer,
        s.sync_pipe.get(),
        uaudio_psync_intr,
        "sync play xfer",
    );

    s.sync_nextxfer.set(s.sync_nextxfer.get() + 1);
    if s.sync_nextxfer.get() == s.nxfers.get() {
        s.sync_nextxfer.set(0);
    }
}

/// `uaudio_psync_intr`: the USB driver's completion call-back of a play sync transfer:
/// takes the device's samples per frame.
pub fn uaudio_psync_intr(usb_xfer: &'static UsbdXfer, arg: *mut c_void, status: UsbdStatus) {
    let sc = uaudio_xfer_sc(arg);
    let s = &sc.pstream;

    if status != USBD_NORMAL_COMPLETION {
        return;
    }

    if sc.trigger_mode.get() & AUMODE_PLAY == 0 {
        // halted
        return;
    }

    let xfer = &s.sync_xfers[s.sync_nextxfer.get() as usize];
    if !xfer.usb_xfer.get().is_some_and(|x| ptr::eq(x, usb_xfer)) {
        // wrong xfer
        return;
    }

    // XXX: there's only one frame, the loop is not necessary

    let pktsz = sc.sync_pktsz.get() as usize;
    // SAFETY: the buffer is this stream's and its transfer completed; no other reference
    // to it is live.
    let buf = unsafe { xfer.buf() };
    let sizes = xfer.sizes();
    let mut off = 0usize;
    for size in sizes.iter().take(xfer.nframes.get() as usize) {
        if usize::from(size.get()) == pktsz {
            let b = &buf[off..];
            let mut val = i32::from(b[0]) | i32::from(b[1]) << 8 | i32::from(b[2]) << 16;
            if pktsz == 4 {
                // as the C, from the start of the buffer
                val |= i32::from(buf[3]) << 24;
            } else {
                val <<= 2;
            }
            val = val.wrapping_mul((UAUDIO_SPF_DIV / (1 << 16)) as i32);
            // compared as unsigned, as in C
            let v = val as u32;
            if v > s.spf_max.get() {
                s.spf.set(s.spf_max.get());
            } else if v < s.spf_min.get() {
                s.spf.set(s.spf_min.get());
            } else {
                s.spf.set(v);
            }
        }
        off += pktsz;
    }

    uaudio_psync_xfer(sc);
}

/// `uaudio_rdata_xfer`: submits a record data transfer to the USB driver.
pub fn uaudio_rdata_xfer(sc: &UaudioSoftc) {
    let s = &sc.rstream;
    let a = sc.ralt();
    let xfer = &s.data_xfers[s.data_nextxfer.get() as usize];
    let bpf = a.bps * a.nch;
    let sizes = xfer.sizes();
    xfer.nframes.set(0);
    let mut done = s.ring_offs.get();

    loop {
        let nframes = xfer.nframes.get();

        // if we crossed the next block boundary, we're done; "this can't happen, debug
        // only": too many frames
        let crossed = nframes & s.nframes_mask.get() == 0 && done > s.safe_blksz.get();
        if !crossed && nframes == s.nframes_max.get() {
            printf(format_args!(
                "{}: too many frames for rec xfer: done = {}, blksz = {}\n",
                sc.dev.xname(),
                done,
                s.ring_blksz.get()
            ));
        }
        if crossed || nframes == s.nframes_max.get() {
            xfer.size.set(done.wrapping_sub(s.ring_offs.get()) as u32);
            s.ring_offs.set(done.wrapping_sub(s.ring_blksz.get()));
            break;
        }

        // estimate next block using s->spf, but allow transfers up to maxpkt
        s.spf_remain.set(s.spf_remain.get() + s.spf.get());
        let fsize = s.spf_remain.get() / UAUDIO_SPF_DIV * bpf;
        s.spf_remain.set(s.spf_remain.get() % UAUDIO_SPF_DIV);
        done = done.wrapping_add(fsize as i32);
        sizes[nframes as usize].set(s.maxpkt.get() as u16);
        xfer.nframes.set(nframes + 1);
    }

    // this can't happen, debug only
    if xfer.nframes.get() == 0 {
        printf(format_args!("{}: zero frame rec xfer\n", sc.dev.xname()));
        return;
    }

    uaudio_isoc_start(sc, xfer, s.data_pipe.get(), uaudio_rdata_intr, "rec xfer");

    s.data_nextxfer.set(s.data_nextxfer.get() + 1);
    if s.data_nextxfer.get() == s.nxfers.get() {
        s.data_nextxfer.set(0);
    }
}

/// `uaudio_rdata_intr`: the USB driver's completion call-back of a record data transfer:
/// copies the frames into audio(4)'s ring.
pub fn uaudio_rdata_intr(usb_xfer: &'static UsbdXfer, arg: *mut c_void, status: UsbdStatus) {
    let sc = uaudio_xfer_sc(arg);
    let s = &sc.rstream;

    if status != USBD_NORMAL_COMPLETION {
        return;
    }

    if sc.trigger_mode.get() & AUMODE_RECORD == 0 {
        // halted
        return;
    }

    let xfer = &s.data_xfers[s.data_nextxfer.get() as usize];
    if !xfer.usb_xfer.get().is_some_and(|x| ptr::eq(x, usb_xfer)) {
        // wrong xfer
        return;
    }

    let a = sc.ralt();
    let bpf = a.bps * a.nch;
    let maxpkt = s.maxpkt.get() as usize;
    let fsize_min = s.spf_min.get() / UAUDIO_SPF_DIV;
    let mut data_size = 0u32;
    let sizes = xfer.sizes();
    // SAFETY: the buffer is this stream's and its transfer completed; no other reference
    // to it is live.
    let buf = unsafe { xfer.buf() };
    for nframes in 0..xfer.nframes.get() as usize {
        let framebuf = &mut buf[nframes * maxpkt..];

        // The device clock may take some time to lock, during which empty or incomplete
        // packets arrive, for which silence is generated.
        let mut fsize = u32::from(sizes[nframes].get());
        if fsize < fsize_min {
            s.spf_remain.set(s.spf_remain.get() + s.spf.get());
            fsize = s.spf_remain.get() / UAUDIO_SPF_DIV * bpf;
            s.spf_remain.set(s.spf_remain.get() % UAUDIO_SPF_DIV);
            framebuf[..fsize as usize].fill(0);
        }
        data_size += fsize;

        // fill ring from frame buffer, handling boundary conditions
        let mut src = 0usize;
        let mut left = fsize as usize;
        while left > 0 {
            let count = (s.ring_end.get() as usize - s.ring_pos.get() as usize).min(left);
            // SAFETY: `count` bytes from `ring_pos` are within audio(4)'s ring `[ring_start,
            // ring_end)`, which audio(4) reads only up to the blocks this driver announced.
            unsafe {
                ptr::copy_nonoverlapping(
                    framebuf[src..src + count].as_ptr(),
                    s.ring_pos.get(),
                    count,
                );
                s.ring_pos.set(s.ring_pos.get().add(count));
            }
            if s.ring_pos.get() == s.ring_end.get() {
                s.ring_pos.set(s.ring_start.get());
            }
            src += count;
            left -= count;
        }
    }

    s.ring_offs.set(
        s.ring_offs
            .get()
            .wrapping_add(data_size.wrapping_sub(xfer.size.get()) as i32),
    );
    s.ring_icnt
        .set(s.ring_icnt.get().wrapping_add(data_size as i32));

    sc.diff_nsamp.set(
        sc.diff_nsamp
            .get()
            .wrapping_sub((data_size / (a.nch * a.bps)) as i32),
    );
    sc.diff_nframes.set(
        sc.diff_nframes
            .get()
            .wrapping_sub(xfer.nframes.get() as i32),
    );

    sc.adjspf_age.set(sc.adjspf_age.get() + xfer.nframes.get());
    if sc.adjspf_age.get() >= sc.ufps.get() / 8 {
        sc.adjspf_age.set(sc.adjspf_age.get() - sc.ufps.get() / 8);
        uaudio_adjspf(sc);
    }

    uaudio_rdata_xfer(sc);

    let nintr = uaudio_block_intrs(s);
    if nintr != 1 {
        printf(format_args!(
            "{}: {}: bad rec intr count\n",
            sc.dev.xname(),
            nintr as u32
        ));
    }
}

/// `uaudio_trigger`: starts playback and recording together, once both `trigger_input()`
/// and `trigger_output()` were called for the open mode.
pub fn uaudio_trigger(sc: &UaudioSoftc) {
    if sc.mode.get() != sc.trigger_mode.get() {
        return;
    }

    // preparing
    if sc.mode.get() & AUMODE_PLAY != 0 {
        for i in 0..sc.pstream.nxfers.get() as usize {
            uaudio_pdata_calcsizes(sc, &sc.pstream.data_xfers[i]);
        }

        uaudio_pdata_copy(sc);
    }

    sc.diff_nsamp.set(0);
    sc.diff_nframes.set(0);
    sc.adjspf_age.set(0);

    // starting
    let s = splusb();
    for i in 0..UAUDIO_NXFERS_MAX as u32 {
        if sc.mode.get() & AUMODE_PLAY != 0 && i < sc.pstream.nxfers.get() {
            if sc.pstream.sync_pipe.get().is_some() {
                uaudio_psync_xfer(sc);
            }
            uaudio_pdata_xfer(sc);
        }
        if sc.mode.get() & AUMODE_RECORD != 0 && i < sc.rstream.nxfers.get() {
            uaudio_rdata_xfer(sc);
        }
    }
    splx(s);
}

/// `uaudio_print`: the attach line's description of the device.
pub fn uaudio_print(sc: &UaudioSoftc, conf: &UaudioConf) {
    let (mut pchan, mut rchan, mut async_, mut impl_fb) = (0, 0, false, false);
    let mut nctl = 0;

    for u in conf.unit_iter() {
        let list = &conf.units[u].mixent_list;
        let mut m = (!list.is_empty()).then_some(0);
        loop {
            m = uaudio_mixer_skip(list, m);
            let Some(mi) = m else {
                break;
            };
            m = (mi + 1 < list.len()).then_some(mi + 1);
            nctl += 1;
        }
    }

    for p in &conf.params_list {
        let palt = p.palt.map(|a| &conf.alts[a]);
        let ralt = p.ralt.map(|a| &conf.alts[a]);
        if let Some(a) = palt {
            pchan = pchan.max(a.nch);
            if a.sync_addr != 0 {
                async_ = true;
            }
        }
        if let Some(a) = ralt {
            rchan = rchan.max(a.nch);
            if a.sync_addr != 0 {
                async_ = true;
            }
            if a.impl_fb {
                impl_fb = true;
            }
        }
    }

    printf(format_args!(
        "{}: class v{}, {}, {}{}, channels: {} play, {} rec, {} ctls\n",
        sc.dev.xname(),
        conf.version >> 8,
        if sc.ufps.get() == 1000 {
            "full-speed"
        } else {
            "high-speed"
        },
        if async_ { "async" } else { "sync" },
        if impl_fb { ", impl-fb" } else { "" },
        pchan,
        rchan,
        nctl
    ));
}

/// `uaudio_match`: an audio streaming interface.
pub fn uaudio_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: `usbd_probe_and_attach` passes a `UsbAttachArg` that lives across the
    // `config_found` calling this.
    let arg = unsafe { &*aux.cast::<UsbAttachArg>() };

    let (Some(iface), Some(_)) = (arg.iface, arg.device) else {
        return UMATCH_NONE;
    };

    let Some(idesc) = usbd_get_interface_descriptor(iface) else {
        return UMATCH_NONE;
    };

    if idesc.bInterfaceClass != UICLASS_AUDIO || idesc.bInterfaceSubClass != UISUBCLASS_AUDIOSTREAM
    {
        return UMATCH_NONE;
    }

    UMATCH_VENDOR_PRODUCT_CONF_IFACE
}

/// `uaudio_attach`: this device has an audio AC, AS or MS interface: get the full
/// configuration descriptor and attach audio devices.
pub fn uaudio_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `self_` was made by `uaudio_ca` (its `ca_devsize`), and lives until
    // `config_detach` frees it after `uaudio_detach`.
    let sc: &UaudioSoftc = unsafe { &*ptr::from_ref(self_.softc::<UaudioSoftc>()) };
    // SAFETY: as in `uaudio_match`.
    let arg = unsafe { &*aux.cast::<UsbAttachArg>() };
    let udev = arg.device();

    let Some(cdesc) = usbd_get_config_descriptor(udev) else {
        return;
    };
    let Some(all) = udev.cdesc.get() else {
        return;
    };
    let total = usize::from(ugetw(cdesc.wTotalLength)).min(all.len());
    let mut desc = UaudioBlob::new(&all[..total]);

    sc.udev.set(Some(udev));
    sc.conf.set(None);
    sc.params.set(None);
    sc.rate.set(0);
    sc.mode.set(0);
    sc.trigger_mode.set(0);
    sc.copy_todo.set(0);

    // Ideally the USB host controller should expose the number of frames we're allowed to
    // schedule, but there's no such interface. The uhci(4) driver can buffer up to 128
    // frames (or it crashes), ehci(4) starts recording null frames past 256 (micro-)frames,
    // ohci(4) works with at most 50 frames.
    match udev.speed.get() {
        USB_SPEED_LOW | USB_SPEED_FULL => {
            sc.ufps.set(1000);
            sc.sync_pktsz.set(3);
            sc.host_nframes.set(50);
        }
        USB_SPEED_HIGH | USB_SPEED_SUPER => {
            sc.ufps.set(8000);
            sc.sync_pktsz.set(4);
            sc.host_nframes.set(240);
        }
        _ => {
            printf(format_args!("{}: unsupported bus speed\n", sc.dev.xname()));
            return;
        }
    }

    let mut conf = Box::new(UaudioConf::new(sc.ufps.get()));
    if !uaudio_process_conf(sc, &mut conf, &mut desc) {
        return;
    }

    // print a nice uaudio attach line
    uaudio_print(sc, &conf);

    sc.conf.set(Some(NonNull::from(Box::leak(conf))));

    let _ = audio_attach_mi(
        &UAUDIO_HW_IF,
        ptr::from_ref(sc).cast_mut().cast(),
        arg.cookie,
        &sc.dev,
    );
}

/// `uaudio_detach`.
pub fn uaudio_detach(self_: &Device, flags: i32) -> Result<(), Errno> {
    // SAFETY: as in `uaudio_attach`.
    let sc: &UaudioSoftc = unsafe { &*ptr::from_ref(self_.softc::<UaudioSoftc>()) };

    let rv = config_detach_children(self_, flags);

    if let Some(udev) = sc.udev.get() {
        usbd_ref_wait(udev);
    }

    if let Some(conf) = sc.conf.take() {
        // SAFETY: the leaked box of `uaudio_attach`; audio(4) is detached and the device's
        // references are gone, so nothing uses the configuration any more.
        drop(unsafe { Box::from_raw(conf.as_ptr()) });
    }

    rv
}

/// `uaudio_open`: picks the open mode the parameter sets allow for `flags`.
///
/// # Safety
///
/// `self_` is the handle `uaudio_attach` gave `audio_attach_mi` (`uaudio_hdl`).
pub unsafe fn uaudio_open(self_: *mut c_void, flags: i32) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let sc = unsafe { uaudio_hdl(self_) };

    if usbd_is_dying(sc.udev()) {
        return Err(Errno::EIO);
    }

    usbd_ref_incr(sc.udev());

    let flags = flags & (FREAD | FWRITE);

    for p in &sc.conf().params_list {
        match flags {
            FWRITE if p.palt.is_some() => {
                sc.mode.set(AUMODE_PLAY);
                return Ok(());
            }
            FREAD if p.ralt.is_some() => {
                sc.mode.set(AUMODE_RECORD);
                return Ok(());
            }
            f if f == FREAD | FWRITE && p.ralt.is_some() && p.palt.is_some() => {
                sc.mode.set(AUMODE_RECORD | AUMODE_PLAY);
                return Ok(());
            }
            _ => {}
        }
    }

    usbd_ref_decr(sc.udev());
    Err(Errno::ENXIO)
}

/// `uaudio_close`.
///
/// # Safety
///
/// As for [`uaudio_open`].
pub unsafe fn uaudio_close(self_: *mut c_void) {
    // SAFETY: the caller's contract.
    let sc = unsafe { uaudio_hdl(self_) };

    sc.mode.set(0);
    usbd_ref_decr(sc.udev());
}

/// `uaudio_set_params`: picks the parameter set closest to what audio(4) asks for and
/// reports what it is.
///
/// # Safety
///
/// As for [`uaudio_open`].
pub unsafe fn uaudio_set_params(
    self_: *mut c_void,
    setmode: i32,
    usemode: i32,
    ap: &mut AudioParams,
    ar: &mut AudioParams,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let sc = unsafe { uaudio_hdl(self_) };
    let conf = sc.conf();
    let mode = sc.mode.get();

    #[cfg(feature = "diagnostic")]
    {
        if setmode != usemode || setmode != mode {
            printf(format_args!(
                "{}: bad call to uaudio_set_params()\n",
                sc.dev.xname()
            ));
            return Err(Errno::EINVAL);
        }
        if mode == 0 {
            printf(format_args!(
                "{}: uaudio_set_params(): not open\n",
                sc.dev.xname()
            ));
            return Err(Errno::EINVAL);
        }
    }
    #[cfg(not(feature = "diagnostic"))]
    let _ = usemode;

    // audio(4) layer requests equal play and record rates
    let rate = if mode & AUMODE_PLAY != 0 {
        ap.sample_rate as i32
    } else {
        ar.sample_rate as i32
    };
    let rateindex = uaudio_rates_indexof(!0, rate);

    let (mut best_mode, mut best_rate, mut best_nch) = (None, None, None);
    let mut found = None;

    for (k, p) in conf.params_list.iter().enumerate() {
        // test if params match the requested mode
        if mode & AUMODE_PLAY != 0 && p.palt.is_none() {
            continue;
        }
        if mode & AUMODE_RECORD != 0 && p.ralt.is_none() {
            continue;
        }
        best_mode.get_or_insert(k);

        // test if params match the requested rate
        if uaudio_getrates(conf, setmode, p) & (1 << rateindex) == 0 {
            continue;
        }
        best_rate.get_or_insert(k);

        // test if params match the requested channel counts
        if let Some(a) = p.palt
            && mode & AUMODE_PLAY != 0
            && conf.alts[a].nch != ap.channels
        {
            continue;
        }
        if let Some(a) = p.ralt
            && mode & AUMODE_RECORD != 0
            && conf.alts[a].nch != ar.channels
        {
            continue;
        }
        best_nch.get_or_insert(k);

        // test if params match the requested precision
        if let Some(a) = p.palt
            && mode & AUMODE_PLAY != 0
            && conf.alts[a].bits != ap.precision
        {
            continue;
        }
        if let Some(a) = p.ralt
            && mode & AUMODE_RECORD != 0
            && conf.alts[a].bits != ar.precision
        {
            continue;
        }

        // everything matched, we're done
        found = Some(k);
        break;
    }

    let Some(k) = found.or(best_nch).or(best_rate).or(best_mode) else {
        return Err(Errno::ENOTTY);
    };
    let p = &conf.params_list[k];

    // Recalculate the rate index, because the chosen parameters may not support the
    // requested one.
    let rateindex = uaudio_rates_indexof(uaudio_getrates(conf, setmode, p), rate);
    if rateindex < 0 {
        return Err(Errno::ENOTTY);
    }

    sc.params.set(NonZeroUsize::new(k + 1));
    sc.rate.set(UAUDIO_RATES[rateindex as usize] as u32);

    let fill = |par: &mut AudioParams, a: &UaudioAlt| {
        par.sample_rate = u64::from(sc.rate.get());
        par.precision = a.bits;
        par.encoding = AUDIO_ENCODING_SLINEAR_LE;
        par.bps = a.bps;
        par.msb = 1;
        par.channels = a.nch;
    };
    if mode & AUMODE_PLAY != 0 {
        fill(ap, sc.palt());
    }
    if mode & AUMODE_RECORD != 0 {
        fill(ar, sc.ralt());
    }

    Ok(())
}

/// `uaudio_set_blksz`: the block size closest to `blksz` the streams can run with.
///
/// # Safety
///
/// As for [`uaudio_open`].
pub unsafe fn uaudio_set_blksz(
    self_: *mut c_void,
    mode: i32,
    _p: &mut AudioParams,
    _r: &mut AudioParams,
    blksz: u32,
) -> u32 {
    // SAFETY: the caller's contract.
    let sc = unsafe { uaudio_hdl(self_) };

    // minimum block size is two transfers, see uaudio_stream_open()
    let mut fps_min = sc.ufps.get();
    if mode & AUMODE_PLAY != 0 {
        fps_min = fps_min.min(sc.palt().fps);
    }
    if mode & AUMODE_RECORD != 0 {
        fps_min = fps_min.min(sc.ralt().fps);
    }
    let rate = sc.rate.get();
    let blksz_min = (rate * 2).div_ceil(fps_min);

    // max block size is only limited by the number of frames the host can schedule
    let blksz_max = rate * (sc.host_nframes.get() / UAUDIO_NXFERS_MIN) / sc.ufps.get() * 85 / 100;

    if blksz > blksz_max {
        blksz_max
    } else if blksz < blksz_min {
        blksz_min
    } else {
        blksz
    }
}

/// `uaudio_trigger_output`.
///
/// # Safety
///
/// As for [`uaudio_open`]; `[start, end)` is audio(4)'s play ring, valid until
/// `uaudio_halt_output`, and `intr(arg)` its call-back.
pub unsafe fn uaudio_trigger_output(
    self_: *mut c_void,
    start: *mut u8,
    end: *mut u8,
    blksz: i32,
    intr: AudioIntr,
    arg: *mut c_void,
    _param: &AudioParams,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let sc = unsafe { uaudio_hdl(self_) };

    uaudio_stream_open(sc, AUMODE_PLAY, start, end, blksz as usize, intr, arg)?;

    sc.trigger_mode.set(sc.trigger_mode.get() | AUMODE_PLAY);
    uaudio_trigger(sc);
    Ok(())
}

/// `uaudio_trigger_input`.
///
/// # Safety
///
/// As for [`uaudio_trigger_output`], with the record ring.
pub unsafe fn uaudio_trigger_input(
    self_: *mut c_void,
    start: *mut u8,
    end: *mut u8,
    blksz: i32,
    intr: AudioIntr,
    arg: *mut c_void,
    _param: &AudioParams,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let sc = unsafe { uaudio_hdl(self_) };

    uaudio_stream_open(sc, AUMODE_RECORD, start, end, blksz as usize, intr, arg)?;

    sc.trigger_mode.set(sc.trigger_mode.get() | AUMODE_RECORD);
    uaudio_trigger(sc);
    Ok(())
}

/// `uaudio_copy_output`: audio(4) wrote `todo` more bytes into the play ring.
///
/// # Safety
///
/// As for [`uaudio_open`].
pub unsafe fn uaudio_copy_output(self_: *mut c_void, todo: usize) {
    // SAFETY: the caller's contract.
    let sc = unsafe { uaudio_hdl(self_) };

    let s = splusb();
    sc.copy_todo.set(sc.copy_todo.get() + todo);

    if sc.mode.get() == sc.trigger_mode.get() {
        uaudio_pdata_copy(sc);
    }
    splx(s);
}

/// `uaudio_underrun`: audio(4) inserted a block of silence into the play ring.
///
/// # Safety
///
/// As for [`uaudio_open`].
pub unsafe fn uaudio_underrun(self_: *mut c_void) {
    // SAFETY: the caller's contract.
    let sc = unsafe { uaudio_hdl(self_) };
    let s = &sc.pstream;

    sc.copy_todo
        .set(sc.copy_todo.get() + s.ring_blksz.get() as usize);

    // copy data (actually silence) produced by the audio(4) layer
    uaudio_pdata_copy(sc);
}

/// `uaudio_halt_output`.
///
/// # Safety
///
/// As for [`uaudio_open`].
pub unsafe fn uaudio_halt_output(self_: *mut c_void) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let sc = unsafe { uaudio_hdl(self_) };

    sc.trigger_mode.set(sc.trigger_mode.get() & !AUMODE_PLAY);
    uaudio_stream_close(sc, AUMODE_PLAY);
    sc.copy_todo.set(0);
    Ok(())
}

/// `uaudio_halt_input`.
///
/// # Safety
///
/// As for [`uaudio_open`].
pub unsafe fn uaudio_halt_input(self_: *mut c_void) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let sc = unsafe { uaudio_hdl(self_) };

    sc.trigger_mode.set(sc.trigger_mode.get() & !AUMODE_RECORD);
    uaudio_stream_close(sc, AUMODE_RECORD);
    Ok(())
}

/// `uaudio_get_port_do`: the current value of mixer control `ctl.dev`.
pub fn uaudio_get_port_do<D: UaudioDev + ?Sized>(
    sc: &D,
    conf: &UaudioConf,
    ctl: &mut MixerCtrl,
) -> Result<(), Errno> {
    let Some((u, mi)) = uaudio_mixer_byindex(conf, ctl.dev) else {
        return Err(Errno::ENOENT);
    };
    let unit = &conf.units[u];
    let list = &unit.mixent_list;
    let m = &list[mi];

    // attach takes no mixer control from a version other than 1.0 or 2.0
    let req_num = if conf.version == UAUDIO_V1 {
        UAUDIO_V1_REQ_GET_CUR
    } else {
        UAUDIO_V2_REQ_CUR
    };

    match m.type_ {
        UAUDIO_MIX_SW => {
            let mut req_buf = [0u8; 4];
            if !sc.uaudio_req(
                UT_READ_CLASS_INTERFACE,
                req_num,
                m.req_sel as u32,
                if m.chan < 0 { 0 } else { m.chan as u32 },
                conf.ctl_ifnum,
                unit.id,
                &mut req_buf[..1],
            ) {
                return Err(Errno::EIO);
            }
            let mut p = UaudioBlob {
                buf: &req_buf,
                rptr: 0,
                wptr: 1,
            };
            let Some(val) = uaudio_getnum(&mut p, 1) else {
                return Err(Errno::EIO);
            };
            ctl.un.set_ord(i32::from(val != 0));
        }
        UAUDIO_MIX_NUM => {
            let (nch, _) = uaudio_mixer_nchan(list, mi);
            ctl.un.value_mut().num_channels = nch as i32;
            for i in 0..nch {
                let m = &list[mi + i];
                let mut req_buf = [0u8; 4];
                if !sc.uaudio_req(
                    UT_READ_CLASS_INTERFACE,
                    req_num,
                    m.req_sel as u32,
                    if m.chan < 0 { 0 } else { i as u32 + 1 },
                    conf.ctl_ifnum,
                    unit.id,
                    &mut req_buf[..2],
                ) {
                    return Err(Errno::EIO);
                }
                let mut p = UaudioBlob {
                    buf: &req_buf,
                    rptr: 0,
                    wptr: 2,
                };
                let Some(val) = uaudio_getnum(&mut p, 2) else {
                    return Err(Errno::EIO);
                };
                let level = uaudio_ranges_decode(&m.ranges, uaudio_sign_expand(val, 2));
                // see the module's deviations: level[] has eight entries
                if let Some(l) = ctl.un.value_mut().level.get_mut(i) {
                    *l = level as u8;
                }
            }
        }
        // UAUDIO_MIX_ENUM: XXX: not used yet
        _ => {}
    }
    Ok(())
}

/// `uaudio_set_port_do`: sets mixer control `ctl.dev`.
pub fn uaudio_set_port_do<D: UaudioDev + ?Sized>(
    sc: &D,
    conf: &UaudioConf,
    ctl: &mut MixerCtrl,
) -> Result<(), Errno> {
    let Some((u, mi)) = uaudio_mixer_byindex(conf, ctl.dev) else {
        return Err(Errno::ENOENT);
    };
    let unit = &conf.units[u];
    let list = &unit.mixent_list;
    let m = &list[mi];

    match m.type_ {
        UAUDIO_MIX_SW => {
            let ord = ctl.un.ord();
            if !(0..=1).contains(&ord) {
                return Err(Errno::EINVAL);
            }
            let mut req_buf = [ord as u8];
            if !sc.uaudio_req(
                UT_WRITE_CLASS_INTERFACE,
                UAUDIO_V1_REQ_SET_CUR,
                m.req_sel as u32,
                if m.chan < 0 { 0 } else { m.chan as u32 },
                conf.ctl_ifnum,
                unit.id,
                &mut req_buf,
            ) {
                return Err(Errno::EIO);
            }
        }
        UAUDIO_MIX_NUM => {
            let (nch, _) = uaudio_mixer_nchan(list, mi);
            ctl.un.value_mut().num_channels = nch as i32;
            for i in 0..nch {
                let m = &list[mi + i];
                // see the module's deviations: level[] has eight entries
                let level = ctl.un.value().level.get(i).copied().unwrap_or(0);
                let val = uaudio_ranges_encode(&m.ranges, i32::from(level));
                let mut req_buf = [val as u8, (val >> 8) as u8];
                if !sc.uaudio_req(
                    UT_WRITE_CLASS_INTERFACE,
                    UAUDIO_V1_REQ_SET_CUR,
                    m.req_sel as u32,
                    if m.chan < 0 { 0 } else { i as u32 + 1 },
                    conf.ctl_ifnum,
                    unit.id,
                    &mut req_buf,
                ) {
                    return Err(Errno::EIO);
                }
            }
        }
        // UAUDIO_MIX_ENUM: XXX: not used yet
        _ => {}
    }
    Ok(())
}

/// `uaudio_query_devinfo_do`: describes mixer control `devinfo.index`.
pub fn uaudio_query_devinfo_do(conf: &UaudioConf, devinfo: &mut MixerDevinfo) -> Result<(), Errno> {
    devinfo.next = -1;
    devinfo.prev = -1;
    match devinfo.index {
        UAUDIO_CLASS_IN => {
            strlcpy(&mut devinfo.label.name, AudioCinputs);
            devinfo.type_ = AUDIO_MIXER_CLASS;
            devinfo.mixer_class = -1;
            return Ok(());
        }
        UAUDIO_CLASS_OUT => {
            strlcpy(&mut devinfo.label.name, AudioCoutputs);
            devinfo.type_ = AUDIO_MIXER_CLASS;
            devinfo.mixer_class = -1;
            return Ok(());
        }
        _ => {}
    }

    // find the unit & mixent structure for the given index
    let Some((u, mi)) = uaudio_mixer_byindex(conf, devinfo.index) else {
        return Err(Errno::ENOENT);
    };
    let unit = &conf.units[u];
    let list = &unit.mixent_list;
    let m = &list[mi];

    if m.fname == Some("level") {
        // mixer(4) interface doesn't give a names to level controls
        strlcpy(&mut devinfo.label.name, &unit.name);
    } else if m.chan == -1 {
        snprintf(
            &mut devinfo.label.name,
            format_args!("{}_{}", cstr(&unit.name), fname_str(m.fname)),
        );
    } else {
        snprintf(
            &mut devinfo.label.name,
            format_args!(
                "{}_{}{}",
                cstr(&unit.name),
                fname_str(m.fname),
                m.chan as u32
            ),
        );
    }

    devinfo.mixer_class = unit.mixer_class;
    match m.type_ {
        UAUDIO_MIX_SW => {
            devinfo.type_ = AUDIO_MIXER_ENUM;
            let e = devinfo.un.e_mut();
            e.num_mem = 2;
            e.member[0].ord = 0;
            strlcpy(&mut e.member[0].label.name, b"off");
            e.member[1].ord = 1;
            strlcpy(&mut e.member[1].label.name, b"on");
        }
        UAUDIO_MIX_NUM => {
            devinfo.type_ = AUDIO_MIXER_VALUE;
            let v = devinfo.un.v_mut();
            v.num_channels = uaudio_mixer_nchan(list, mi).0 as i32;
            v.delta = 1;
        }
        UAUDIO_MIX_ENUM => {
            // XXX: not used yet
            devinfo.type_ = AUDIO_MIXER_ENUM;
            devinfo.un.e_mut().num_mem = 0;
        }
        _ => {}
    }
    Ok(())
}

/// `uaudio_get_port`.
///
/// # Safety
///
/// As for [`uaudio_open`].
pub unsafe fn uaudio_get_port(arg: *mut c_void, ctl: &mut MixerCtrl) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let sc = unsafe { uaudio_hdl(arg) };

    usbd_ref_incr(sc.udev());
    let rc = uaudio_get_port_do(sc, sc.conf(), ctl);
    usbd_ref_decr(sc.udev());
    rc
}

/// `uaudio_set_port`.
///
/// # Safety
///
/// As for [`uaudio_open`].
pub unsafe fn uaudio_set_port(arg: *mut c_void, ctl: &mut MixerCtrl) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let sc = unsafe { uaudio_hdl(arg) };

    usbd_ref_incr(sc.udev());
    let rc = uaudio_set_port_do(sc, sc.conf(), ctl);
    usbd_ref_decr(sc.udev());
    rc
}

/// `uaudio_display_name` over the device's strings: the product name without the vendor
/// prefix, control and non-ASCII characters, then `#N` for the second and later instances;
/// returns the length of the whole name, which may exceed `buf`.
pub fn uaudio_display_name_do(
    devname: &str,
    vendor: Option<&[u8]>,
    product: Option<&[u8]>,
    instnum: u32,
    buf: &mut [u8],
) -> usize {
    let size = buf.len();
    let (Some(vendor), Some(mut product)) = (vendor, product) else {
        return strlcpy(buf, devname.as_bytes());
    };

    // If the product name is prefixed with the vendor, drop the prefix
    let mut i = 0;
    while i < product.len() {
        if i == vendor.len() {
            while i < product.len() && product[i] == b' ' {
                i += 1;
            }
            product = &product[i..];
            break;
        }
        if vendor[i] != product[i] {
            break;
        }
        i += 1;
    }

    // Copy the product name removing control and non-ascii chars. The destination
    // position `p` is advanced, but chars are not written past the end of the buffer.
    let mut p = 0usize;
    for &c in product {
        if !(b' '..=b'~').contains(&c) {
            continue;
        }
        if p < size {
            buf[p] = c;
        }
        p += 1;
    }

    // Terminating '\0'
    if size > 0 {
        buf[if p < size { p } else { size - 1 }] = 0;
    }

    // Append the instance number (if any), similarly advance `p` but don't write past
    // the end
    if instnum > 0 {
        let rest: &mut [u8] = if p < size { &mut buf[p..] } else { &mut [] };
        p += snprintf(rest, format_args!("#{}", instnum + 1));
    }

    p
}

/// `uaudio_display_name`.
///
/// # Safety
///
/// As for [`uaudio_open`].
pub unsafe fn uaudio_display_name(arg: *mut c_void, buf: &mut [u8]) -> usize {
    // SAFETY: the caller's contract.
    let sc = unsafe { uaudio_hdl(arg) };
    let udev = sc.udev();

    uaudio_display_name_do(
        sc.dev.xname(),
        UsbdDevice::string(udev.vendor.get()),
        UsbdDevice::string(udev.product.get()),
        sc.conf().instnum,
        buf,
    )
}

/// `uaudio_query_devinfo`.
///
/// # Safety
///
/// As for [`uaudio_open`].
pub unsafe fn uaudio_query_devinfo(
    arg: *mut c_void,
    devinfo: &mut MixerDevinfo,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let sc = unsafe { uaudio_hdl(arg) };

    usbd_ref_incr(sc.udev());
    let rc = uaudio_query_devinfo_do(sc.conf(), devinfo);
    usbd_ref_decr(sc.udev());
    rc
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for uaudio(4): the descriptor parser over table-driven configuration
    // descriptors (a UAC 1.0 speaker like QEMU's `usb-audio`, a UAC 1.0 headset, a UAC 2.0
    // device with a clock source and a feedback endpoint), the ranges and rates arithmetic,
    // the naming, the mixer and parameter selection, and the stream sizing.

    use core::cell::RefCell;
    use core::mem::MaybeUninit;
    use std::boxed::Box;
    use std::vec::Vec;
    use std::{assert, assert_eq, vec};

    use super::*;

    /// A request the fake device saw: type, request, selector, channel, interface, unit,
    /// data.
    type Req = (u8, u8, u32, u32, u32, u32, Vec<u8>);

    /// A device over tables: the interface numbers, the replies to class reads, keyed by
    /// (request, selector, channel, unit), and the writes it was sent.
    struct FakeDev {
        ifnums: Vec<u32>,
        claimed: RefCell<Vec<bool>>,
        replies: Vec<((u8, u32, u32, u32), Vec<u8>)>,
        writes: RefCell<Vec<Req>>,
    }

    impl FakeDev {
        fn new(ifnums: &[u32], replies: Vec<((u8, u32, u32, u32), Vec<u8>)>) -> Self {
            Self {
                ifnums: ifnums.to_vec(),
                claimed: RefCell::new(vec![false; ifnums.len()]),
                replies,
                writes: RefCell::new(Vec::new()),
            }
        }
    }

    impl UaudioDev for FakeDev {
        fn devname(&self) -> &str {
            "uaudio0"
        }

        fn uaudio_req(
            &self,
            type_: u8,
            req: u8,
            sel: u32,
            chan: u32,
            ifnum: u32,
            id: u32,
            buf: &mut [u8],
        ) -> bool {
            if type_ & 0x80 == 0 {
                self.writes
                    .borrow_mut()
                    .push((type_, req, sel, chan, ifnum, id, buf.to_vec()));
                return true;
            }
            let Some((_, data)) = self
                .replies
                .iter()
                .find(|(k, _)| *k == (req, sel, chan, id))
            else {
                return false;
            };
            let n = buf.len().min(data.len());
            buf[..n].copy_from_slice(&data[..n]);
            true
        }

        fn nifaces(&self) -> usize {
            self.ifnums.len()
        }

        fn iface_number(&self, i: usize) -> u32 {
            self.ifnums[i]
        }

        fn iface_claimed(&self, i: usize) -> bool {
            self.claimed.borrow()[i]
        }

        fn claim_iface(&self, i: usize) {
            self.claimed.borrow_mut()[i] = true;
        }
    }

    /// A configuration descriptor: the 9-byte header with `wTotalLength` filled in, then
    /// `body`.
    fn config(nifaces: u8, body: &[u8]) -> Vec<u8> {
        let total = (9 + body.len()) as u16;
        let mut v = vec![
            9,
            2,
            total as u8,
            (total >> 8) as u8,
            nifaces,
            1,
            0,
            0x80,
            50,
        ];
        v.extend_from_slice(body);
        v
    }

    /// A UAC 1.0 stereo speaker shaped like QEMU's `usb-audio`: USB streaming input
    /// terminal 1 -> feature unit 2 (master mute and volume) -> speaker 3; AS interface 1,
    /// alternate 1: 16-bit stereo PCM at 48 kHz on an adaptive isochronous OUT endpoint.
    fn uac1_speaker() -> Vec<u8> {
        config(
            2,
            &[
                // interface 0: audio control
                9, 4, 0, 0, 0, 1, 1, 0, 0, //
                // AC header: UAC 1.0, one streaming interface (1)
                9, 0x24, 1, 0x00, 0x01, 39, 0, 1, 1, //
                // input terminal 1: USB streaming, 2 channels (L, R)
                12, 0x24, 2, 1, 0x01, 0x01, 0, 2, 0x03, 0x00, 0, 0, //
                // feature unit 2 from 1: 1-byte controls, master mute + volume, none per
                // channel
                10, 0x24, 6, 2, 1, 1, 0x03, 0x00, 0x00, 0, //
                // output terminal 3: speaker, from 2
                9, 0x24, 3, 3, 0x01, 0x03, 0, 2, 0, //
                // interface 1, alternate 0: zero bandwidth
                9, 4, 1, 0, 0, 1, 2, 0, 0, //
                // interface 1, alternate 1: one endpoint
                9, 4, 1, 1, 1, 1, 2, 0, 0, //
                // AS general: terminal 1, delay 1, PCM
                7, 0x24, 1, 1, 1, 0x01, 0x00, //
                // format type I: 2 channels, 2 bytes, 16 bits, one rate: 48000
                11, 0x24, 2, 1, 2, 2, 16, 1, 0x80, 0xbb, 0x00, //
                // endpoint 0x01 OUT, isochronous adaptive, 192 bytes, interval 1
                9, 5, 0x01, 0x09, 192, 0, 1, 0, 0, //
                // class-specific endpoint: general, no sampling frequency control
                7, 0x25, 1, 0x00, 0, 0, 0,
            ],
        )
    }

    /// The volume range of [`uac1_speaker`]'s feature unit: -48 dB..0 dB by 1/256 dB.
    fn uac1_speaker_replies() -> Vec<((u8, u32, u32, u32), Vec<u8>)> {
        vec![
            ((UAUDIO_V1_REQ_GET_MIN, 2, 0, 2), vec![0x00, 0xd0]),
            ((UAUDIO_V1_REQ_GET_MAX, 2, 0, 2), vec![0x00, 0x00]),
            ((UAUDIO_V1_REQ_GET_RES, 2, 0, 2), vec![0x00, 0x01]),
            ((UAUDIO_V1_REQ_GET_CUR, 2, 0, 2), vec![0x00, 0xe8]),
            ((UAUDIO_V1_REQ_GET_CUR, 1, 0, 2), vec![0x01]),
        ]
    }

    /// Parses `cdesc` as `uaudio_attach` does, at `ufps` frames per second.
    fn parse(dev: &FakeDev, cdesc: &[u8], ufps: u32) -> (bool, UaudioConf) {
        let mut conf = UaudioConf::new(ufps);
        let ok = uaudio_process_conf(dev, &mut conf, &mut UaudioBlob::new(cdesc));
        (ok, conf)
    }

    /// The unit with descriptor id `id`.
    fn unit(conf: &UaudioConf, id: u32) -> &UaudioUnit {
        &conf.units[uaudio_unit_byid(conf, id).unwrap()]
    }

    /// A zeroed softc (the `Softc` contract) holding `conf`.
    fn softc_with(conf: UaudioConf) -> Box<UaudioSoftc> {
        let sc: Box<MaybeUninit<UaudioSoftc>> = Box::new_zeroed();
        // SAFETY: every member of `UaudioSoftc` is valid as zero bits (its `Softc` impl).
        let sc = unsafe { sc.assume_init() };
        sc.conf.set(Some(NonNull::from(Box::leak(Box::new(conf)))));
        sc
    }

    /// The label of a mixer control.
    fn label(d: &MixerDevinfo) -> &str {
        cstr(&d.label.name)
    }

    #[test]
    fn getnum_and_getdesc_stay_in_the_blob() {
        let buf = [3, 0x24, 7, 0x34, 0x12, 0, 9];
        let mut p = UaudioBlob::new(&buf);
        let mut d = uaudio_getdesc(&mut p).unwrap();
        assert_eq!(d.remaining(), 2);
        assert_eq!(uaudio_getnum(&mut d, 2), Some(0x0724));
        assert_eq!(uaudio_getnum(&mut d, 1), None);
        assert_eq!(uaudio_getnum(&mut p, 2), Some(0x1234));
        // a zero-sized descriptor, then one longer than what is left
        assert!(uaudio_getdesc(&mut p).is_none());
        let mut q = UaudioBlob::new(&[9, 1, 2]);
        assert!(uaudio_getdesc(&mut q).is_none());
        // a 3-byte number (sample rates)
        let mut r = UaudioBlob::new(&[0x80, 0xbb, 0x00]);
        assert_eq!(uaudio_getnum(&mut r, 3), Some(48000));
        assert_eq!(uaudio_getnum(&mut r, 0), Some(0));
    }

    #[test]
    fn sign_expand_by_size() {
        for (val, size, want) in [
            (0x7f, 1, 127),
            (0x80, 1, -128),
            (0xd000, 2, -12288),
            (0x7fff, 2, 32767),
            (0x80_0000, 3, -8_388_608),
            (0xffff_ffff, 4, -1),
            (48000, 4, 48000),
        ] {
            assert_eq!(uaudio_sign_expand(val, size), want, "{val:#x}/{size}");
        }
    }

    #[test]
    fn ranges_are_sorted_and_reject_overlaps() {
        let mut r = UaudioRanges::default();
        uaudio_ranges_add(&mut r, 10, 20, 1);
        uaudio_ranges_add(&mut r, 0, 5, 1);
        uaudio_ranges_add(&mut r, 15, 30, 1); // overlaps [10:20]
        uaudio_ranges_add(&mut r, 9, 8, 1); // min > max
        uaudio_ranges_add(&mut r, 40, 40, 0);
        let mins: Vec<i32> = r.el.iter().map(|e| e.min).collect();
        assert_eq!(mins, [0, 10, 40]);
        assert_eq!(r.nval, 6 + 11 + 1);

        // 0..255 spans the values of all ranges
        assert_eq!(uaudio_ranges_decode(&r, 0), 0);
        assert_eq!(uaudio_ranges_decode(&r, 40), 255);
        assert_eq!(uaudio_ranges_decode(&r, 7), 0); // in no range
        assert_eq!(uaudio_ranges_encode(&r, 0), 0);
        assert_eq!(uaudio_ranges_encode(&r, 255), 40);
        for v in [0, 3, 5, 10, 15, 20, 40] {
            let level = uaudio_ranges_decode(&r, v);
            assert_eq!(uaudio_ranges_encode(&r, level), v as u32, "value {v}");
        }

        uaudio_ranges_clear(&mut r);
        assert!(r.el.is_empty());
        assert_eq!(r.nval, 0);

        // a single value decodes to 0
        uaudio_ranges_add(&mut r, 5, 5, 0);
        assert_eq!(uaudio_ranges_decode(&r, 5), 0);
    }

    #[test]
    fn ranges_give_the_supported_rates() {
        let mut r = UaudioRanges::default();
        uaudio_ranges_add(&mut r, 44100, 44100, 0);
        uaudio_ranges_add(&mut r, 48000, 48000, 0);
        assert_eq!(uaudio_ranges_getrates(&r, 1, 1), 1 << 7 | 1 << 8);
        // a continuous range with a resolution: the C tests `v - min % res == 0`, so only
        // a rate equal to `min % res` passes
        let mut c = UaudioRanges::default();
        uaudio_ranges_add(&mut c, 8000, 48000, 8000);
        assert_eq!(uaudio_ranges_getrates(&c, 1, 1), 0);
        let mut z = UaudioRanges::default();
        uaudio_ranges_add(&mut z, 8000, 16000, 0);
        assert_eq!(uaudio_ranges_getrates(&z, 1, 1), 0b1111);
    }

    #[test]
    fn rates_indexof_picks_the_closest_allowed() {
        assert_eq!(uaudio_rates_indexof(!0, 48000), 8);
        assert_eq!(uaudio_rates_indexof(!0, 44000), 7);
        assert_eq!(uaudio_rates_indexof(1 << 8, 8000), 8);
        assert_eq!(uaudio_rates_indexof(1 << 0 | 1 << 14, 110_000), 14);
        // a tie keeps the first
        assert_eq!(uaudio_rates_indexof(1 << 0 | 1 << 14, 100_000), 0);
        assert_eq!(uaudio_rates_indexof(0, 48000), -1);
    }

    #[test]
    fn v1_feature_bits_widen_to_v2_pairs() {
        let mut conf = UaudioConf::new(1000);
        conf.version = UAUDIO_V1;
        assert_eq!(uaudio_feature_fixup(&conf, 0b11), 0b1111);
        assert_eq!(uaudio_feature_fixup(&conf, 0b100), 0b11_0000);
        conf.version = UAUDIO_V2;
        assert_eq!(uaudio_feature_fixup(&conf, 0b0110), 0b0110);
    }

    #[test]
    fn terminal_and_clock_names() {
        let mut conf = UaudioConf {
            nin: 2,
            nout: 2,
            ..UaudioConf::default()
        };
        for (type_, isout, want) in [
            (0x0101, false, "dac"),
            (0x0101, true, "record"),
            (0x0201, false, "mic"),
            (0x0301, true, "spkr"),
            (0x0302, true, "hp"),
            (0x0402, false, "mic"),
            (0x0501, true, "phone-in"),
            (0x0603, false, "line-in"),
            (0x0605, true, "dig-out"),
            (0x0701, false, "int-in"),
            (0x0801, true, "unk-out"),
        ] {
            assert_eq!(uaudio_tname(&conf, type_, isout), want, "{type_:#x}");
        }
        conf.nin = 1;
        conf.nout = 1;
        assert_eq!(uaudio_tname(&conf, 0x0201, false), "input");
        assert_eq!(uaudio_tname(&conf, 0x0301, true), "output");
        assert_eq!(uaudio_clkname(0), "ext");
        assert_eq!(uaudio_clkname(5), "fixed");
    }

    #[test]
    fn mkname_numbers_repeated_templates() {
        let mut conf = UaudioConf::default();
        for _ in 0..4 {
            conf.units.push(UaudioUnit {
                unit_next: None,
                src_next: None,
                dst_next: None,
                src_list: None,
                dst_list: None,
                name: [0; UAUDIO_NAMEMAX],
                nch: 0,
                type_: 0,
                id: 0,
                term: 0,
                clock: None,
                rates: UaudioRanges::default(),
                cap_freqctl: false,
                mixer_class: 0,
                mixent_list: Vec::new(),
            });
        }
        uaudio_mkname(&mut conf, b"spkr", 0);
        uaudio_mkname(&mut conf, b"spkr", 1);
        uaudio_mkname(&mut conf, b"spkr_mic", 2);
        uaudio_mkname(&mut conf, b"spkr_mic", 3);
        assert_eq!(cstr(&conf.units[0].name), "spkr");
        assert_eq!(cstr(&conf.units[1].name), "spkr1");
        assert_eq!(cstr(&conf.units[2].name), "spkr_mic");
        assert_eq!(cstr(&conf.units[3].name), "spkr_mic_1");
    }

    /// QEMU's `usb-audio`: one play-only parameter set, a `dac` feature unit whose level
    /// and mute are the two mixer controls, both interfaces claimed.
    #[test]
    fn uac1_speaker_parses() {
        let dev = FakeDev::new(&[0, 1], uac1_speaker_replies());
        let (ok, conf) = parse(&dev, &uac1_speaker(), 1000);
        assert!(ok);
        assert_eq!(conf.version, UAUDIO_V1);
        assert_eq!(conf.ctl_ifnum, 0);
        assert_eq!((conf.nin, conf.nout), (0, 1));
        assert_eq!(*dev.claimed.borrow(), [true, true]);

        // unit_list runs from the last unit found (the input terminal) to the first
        let ids: Vec<u32> = conf.unit_iter().map(|u| conf.units[u].id).collect();
        assert_eq!(ids, [1, 2, 3]);
        assert_eq!(cstr(&unit(&conf, 1).name), "dac");
        assert_eq!(cstr(&unit(&conf, 2).name), "dac");
        assert_eq!(cstr(&unit(&conf, 3).name), "output");
        assert_eq!(unit(&conf, 2).mixer_class, UAUDIO_CLASS_OUT);
        assert_eq!(unit(&conf, 3).nch, 2);
        let fu = unit(&conf, 2);
        let names: Vec<_> = fu.mixent_list.iter().map(|m| (m.fname, m.chan)).collect();
        assert_eq!(names, [(Some("level"), -1), (Some("mute"), -1)]);
        assert_eq!(fu.mixent_list[0].ranges.nval, 12289);

        assert_eq!(conf.alts.len(), 1);
        let a = conf.alts[0];
        assert_eq!(
            (a.ifnum, a.altnum, a.mode, a.data_addr, a.sync_addr),
            (1, 1, AUMODE_PLAY, 1, 0)
        );
        assert_eq!(
            (a.nch, a.bps, a.bits, a.fps, a.maxpkt),
            (2, 2, 16, 1000, 192)
        );
        assert_eq!(a.v1_rates, 1 << 8);
        assert!(!a.v1_cap_freqctl);
        assert_eq!(
            conf.params_list,
            [UaudioParams {
                palt: Some(0),
                ralt: None,
                v1_rates: 1 << 8
            }]
        );

        // the mixer: two classes, then outputs.dac and outputs.dac_mute
        let mut d = MixerDevinfo::zeroed();
        d.index = UAUDIO_CLASS_OUT;
        uaudio_query_devinfo_do(&conf, &mut d).unwrap();
        assert_eq!((label(&d), d.type_), ("outputs", AUDIO_MIXER_CLASS));
        d.index = 2;
        uaudio_query_devinfo_do(&conf, &mut d).unwrap();
        assert_eq!((label(&d), d.type_), ("dac", AUDIO_MIXER_VALUE));
        assert_eq!(d.mixer_class, UAUDIO_CLASS_OUT);
        assert_eq!(d.un.v().num_channels, 1);
        d.index = 3;
        uaudio_query_devinfo_do(&conf, &mut d).unwrap();
        assert_eq!((label(&d), d.type_), ("dac_mute", AUDIO_MIXER_ENUM));
        assert_eq!(d.un.e().num_mem, 2);
        d.index = 4;
        assert_eq!(uaudio_query_devinfo_do(&conf, &mut d), Err(Errno::ENOENT));

        // reading the volume (-6 dB) and the mute, then setting the volume to the top
        let mut ctl = MixerCtrl {
            dev: 2,
            ..MixerCtrl::default()
        };
        uaudio_get_port_do(&dev, &conf, &mut ctl).unwrap();
        assert_eq!(ctl.un.value().num_channels, 1);
        assert_eq!(ctl.un.value().level[0], 128);
        ctl.dev = 3;
        uaudio_get_port_do(&dev, &conf, &mut ctl).unwrap();
        assert_eq!(ctl.un.ord(), 1);
        ctl.dev = 2;
        ctl.un.value_mut().level[0] = 255;
        uaudio_set_port_do(&dev, &conf, &mut ctl).unwrap();
        ctl.dev = 3;
        ctl.un.set_ord(2);
        assert_eq!(
            uaudio_set_port_do(&dev, &conf, &mut ctl),
            Err(Errno::EINVAL)
        );
        assert_eq!(
            *dev.writes.borrow(),
            [(
                UT_WRITE_CLASS_INTERFACE,
                UAUDIO_V1_REQ_SET_CUR,
                2,
                0,
                0,
                2,
                vec![0, 0]
            )]
        );
    }

    /// A feature unit whose range request fails is dropped with its message; the other
    /// controls stay.
    #[test]
    fn a_control_without_ranges_is_skipped() {
        let dev = FakeDev::new(&[0, 1], Vec::new());
        let (ok, conf) = parse(&dev, &uac1_speaker(), 1000);
        assert!(ok);
        let names: Vec<_> = unit(&conf, 2).mixent_list.iter().map(|m| m.fname).collect();
        assert_eq!(names, [Some("mute")]);
    }

    /// An interface another uaudio(4) claimed is left alone, and a truncated descriptor
    /// fails the parse.
    #[test]
    fn claimed_and_truncated_configurations() {
        let dev = FakeDev::new(&[0, 1], uac1_speaker_replies());
        dev.claimed.borrow_mut()[1] = true;
        let (ok, conf) = parse(&dev, &uac1_speaker(), 1000);
        assert!(ok);
        assert!(conf.alts.is_empty());
        assert!(conf.params_list.is_empty());

        let dev = FakeDev::new(&[0, 1], uac1_speaker_replies());
        dev.claimed.borrow_mut()[0] = true;
        let (ok, conf) = parse(&dev, &uac1_speaker(), 1000);
        assert!(ok);
        assert_eq!(conf.instnum, 1);
        assert!(conf.unit_list.is_none());

        let dev = FakeDev::new(&[0, 1], uac1_speaker_replies());
        let mut d = uac1_speaker();
        d.truncate(d.len() - 3);
        let (ok, _) = parse(&dev, &d, 1000);
        assert!(!ok);
    }

    /// A UAC 1.0 headset: a USB-streaming input to a speaker and a microphone to a
    /// USB-streaming output, each through a feature unit, with full-duplex 16-bit stereo
    /// play and mono record at 44.1 and 48 kHz (the record side also at 16 kHz).
    fn uac1_headset() -> Vec<u8> {
        config(
            3,
            &[
                9, 4, 0, 0, 0, 1, 1, 0, 0, //
                10, 0x24, 1, 0x00, 0x01, 0, 0, 2, 1, 2, //
                // IT 1: USB streaming, stereo
                12, 0x24, 2, 1, 0x01, 0x01, 0, 2, 0x03, 0x00, 0, 0, //
                // FU 2 from 1: master mute, per-channel volume
                10, 0x24, 6, 2, 1, 1, 0x01, 0x02, 0x02, 0, //
                // OT 3: speaker from 2
                9, 0x24, 3, 3, 0x01, 0x03, 0, 2, 0, //
                // IT 4: microphone, mono
                12, 0x24, 2, 4, 0x01, 0x02, 0, 1, 0x00, 0x00, 0, 0, //
                // FU 5 from 4: master mute and volume
                9, 0x24, 6, 5, 4, 1, 0x03, 0x00, 0, //
                // OT 6: USB streaming from 5
                9, 0x24, 3, 6, 0x01, 0x01, 0, 5, 0, //
                // interface 1: play
                9, 4, 1, 0, 0, 1, 2, 0, 0, //
                9, 4, 1, 1, 1, 1, 2, 0, 0, //
                7, 0x24, 1, 1, 1, 0x01, 0x00, //
                14, 0x24, 2, 1, 2, 2, 16, 2, 0x44, 0xac, 0x00, 0x80, 0xbb, 0x00, //
                9, 5, 0x02, 0x09, 192, 0, 1, 0, 0, //
                7, 0x25, 1, 0x01, 0, 0, 0, //
                // interface 2: record
                9, 4, 2, 0, 0, 1, 2, 0, 0, //
                9, 4, 2, 1, 1, 1, 2, 0, 0, //
                7, 0x24, 1, 6, 1, 0x01, 0x00, //
                17, 0x24, 2, 1, 1, 2, 16, 3, 0x80, 0x3e, 0x00, 0x44, 0xac, 0x00, 0x80, 0xbb,
                0x00, //
                9, 5, 0x81, 0x05, 96, 0, 1, 0, 0, //
                7, 0x25, 1, 0x01, 0, 0, 0,
            ],
        )
    }

    fn uac1_headset_replies() -> Vec<((u8, u32, u32, u32), Vec<u8>)> {
        let mut r = Vec::new();
        for (chan, id) in [(1, 2), (2, 2), (0, 5)] {
            r.push(((UAUDIO_V1_REQ_GET_MIN, 2, chan, id), vec![0x00, 0xc0]));
            r.push(((UAUDIO_V1_REQ_GET_MAX, 2, chan, id), vec![0x00, 0x10]));
            r.push(((UAUDIO_V1_REQ_GET_RES, 2, chan, id), vec![0x00, 0x01]));
        }
        r
    }

    #[test]
    fn uac1_headset_names_and_duplex_parameters() {
        let dev = FakeDev::new(&[0, 1, 2], uac1_headset_replies());
        let (ok, conf) = parse(&dev, &uac1_headset(), 1000);
        assert!(ok);
        assert_eq!(*dev.claimed.borrow(), [true, true, true]);
        assert_eq!((conf.nin, conf.nout), (1, 1));
        let ids: Vec<u32> = conf.unit_iter().map(|u| conf.units[u].id).collect();
        assert_eq!(ids, [4, 5, 6, 1, 2, 3]);
        for (id, name) in [
            (1, "dac"),
            (2, "dac"),
            (3, "output"),
            (4, "input"),
            (5, "record"),
            (6, "record"),
        ] {
            assert_eq!(cstr(&unit(&conf, id).name), name, "unit {id}");
        }
        assert_eq!(unit(&conf, 2).mixer_class, UAUDIO_CLASS_OUT);
        assert_eq!(unit(&conf, 5).mixer_class, UAUDIO_CLASS_IN);

        // the record controls come first (unit_list order); the per-channel volume of the
        // dac is one two-channel control
        let mut labels = Vec::new();
        let mut d = MixerDevinfo::zeroed();
        d.index = 2;
        while uaudio_query_devinfo_do(&conf, &mut d).is_ok() {
            labels.push((std::string::String::from(label(&d)), d.mixer_class));
            if d.type_ == AUDIO_MIXER_VALUE {
                assert_eq!(
                    d.un.v().num_channels,
                    if d.mixer_class == 0 { 2 } else { 1 }
                );
            }
            d.index += 1;
        }
        let labels: Vec<(&str, i32)> = labels.iter().map(|(s, c)| (s.as_str(), *c)).collect();
        assert_eq!(
            labels,
            [
                ("record", UAUDIO_CLASS_IN),
                ("record_mute", UAUDIO_CLASS_IN),
                ("dac", UAUDIO_CLASS_OUT),
                ("dac_mute", UAUDIO_CLASS_OUT),
            ]
        );

        // full duplex: one play and one record setting, common rates 44.1 and 48 kHz
        assert_eq!(conf.alts.len(), 2);
        assert_eq!(conf.alts[0].mode, AUMODE_PLAY); // two channels first
        assert_eq!(conf.alts[1].mode, AUMODE_RECORD);
        assert!(conf.alts[0].v1_cap_freqctl);
        assert_eq!(conf.alts[1].v1_rates, 1 << 3 | 1 << 7 | 1 << 8);
        assert_eq!(
            conf.params_list,
            [UaudioParams {
                palt: Some(0),
                ralt: Some(1),
                v1_rates: 1 << 7 | 1 << 8
            }]
        );
    }

    /// A UAC 2.0 high-speed device: clock source 10 (internal programmable, frequency
    /// control) clocks the USB streaming input terminal 1 -> feature unit 2 -> speaker 3;
    /// the AS interface plays 24-bit in 4-byte stereo on an asynchronous endpoint with a
    /// feedback endpoint.
    fn uac2_speaker() -> Vec<u8> {
        config(
            2,
            &[
                9, 4, 0, 0, 0, 1, 1, 0x20, 0, //
                9, 0x24, 1, 0x00, 0x02, 1, 0, 0, 0, //
                // clock source 10: internal programmable, frequency control read-write
                8, 0x24, 0x0a, 10, 0x03, 0x07, 0, 0, //
                // IT 1: USB streaming, clock 10, 2 channels
                17, 0x24, 2, 1, 0x01, 0x01, 0, 10, 2, 0x03, 0, 0, 0, 0, 0, 0, 0, //
                // FU 2 from 1: master mute + volume (4-byte controls), none per channel
                18, 0x24, 6, 2, 1, 0x0f, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, //
                // OT 3: speaker from 2, clock 10
                12, 0x24, 3, 3, 0x01, 0x03, 0, 2, 10, 0, 0, 0, //
                9, 4, 1, 0, 0, 1, 2, 0x20, 0, //
                9, 4, 1, 1, 2, 1, 2, 0x20, 0, //
                // AS general v2: terminal 1, PCM, 2 channels
                16, 0x24, 1, 1, 0, 1, 0x01, 0, 0, 0, 2, 0x03, 0, 0, 0, 0, //
                // format type I v2: 4-byte slots, 24 bits
                6, 0x24, 2, 1, 4, 24, //
                // data endpoint 0x01 OUT, isochronous asynchronous, 256 bytes, interval 1
                7, 5, 0x01, 0x05, 0x00, 0x01, 1, //
                8, 0x25, 1, 0, 0, 0, 0, 0, //
                // feedback endpoint 0x81 IN, isochronous feedback usage
                7, 5, 0x81, 0x11, 4, 0, 4,
            ],
        )
    }

    fn uac2_speaker_replies() -> Vec<((u8, u32, u32, u32), Vec<u8>)> {
        let mut clk = vec![2, 0];
        for rate in [44100u32, 48000] {
            for v in [rate, rate, 0] {
                clk.extend_from_slice(&v.to_le_bytes());
            }
        }
        vec![
            ((UAUDIO_V2_REQ_RANGES, UAUDIO_V2_REQSEL_CLKFREQ, 0, 10), clk),
            (
                (UAUDIO_V2_REQ_RANGES, 2, 0, 2),
                vec![1, 0, 0x00, 0x80, 0x00, 0x00, 0x00, 0x01],
            ),
        ]
    }

    #[test]
    fn uac2_speaker_parses_with_its_clock() {
        let dev = FakeDev::new(&[0, 1], uac2_speaker_replies());
        let (ok, conf) = parse(&dev, &uac2_speaker(), 8000);
        assert!(ok);
        assert_eq!(conf.version, UAUDIO_V2);
        let clk = uaudio_unit_byid(&conf, 10).unwrap();
        assert_eq!(conf.pclock, Some(clk));
        assert_eq!(conf.rclock, None);
        assert!(conf.units[clk].cap_freqctl);
        assert_eq!(cstr(&conf.units[clk].name), "prog");
        assert_eq!(conf.units[clk].rates.nval, 2);
        assert_eq!(cstr(&unit(&conf, 2).name), "dac");
        assert_eq!(unit(&conf, 2).mixent_list.len(), 2);

        let a = conf.alts[0];
        assert_eq!((a.nch, a.bps, a.bits), (2, 4, 24));
        assert_eq!(
            (a.data_addr, a.sync_addr, a.fps, a.maxpkt),
            (1, 0x81, 8000, 256)
        );
        assert_eq!(uaudio_alt_getrates(&conf, &a), 1 << 7 | 1 << 8);
        assert_eq!(
            uaudio_getrates(&conf, AUMODE_PLAY, &conf.params_list[0]),
            1 << 7 | 1 << 8
        );
    }

    #[test]
    fn set_params_and_blksz_pick_what_the_device_has() {
        let dev = FakeDev::new(&[0, 1], uac1_speaker_replies());
        let (_, conf) = parse(&dev, &uac1_speaker(), 1000);
        let sc = softc_with(conf);
        sc.mode.set(AUMODE_PLAY);
        sc.ufps.set(1000);
        sc.host_nframes.set(50);
        let hdl: *mut c_void = ptr::from_ref(&*sc).cast_mut().cast();

        let mut ap = AudioParams {
            sample_rate: 44100,
            encoding: AUDIO_ENCODING_SLINEAR_LE,
            precision: 24,
            bps: 4,
            msb: 1,
            channels: 1,
        };
        let mut ar = ap;
        // SAFETY: `hdl` is a uaudio softc with its configuration, alive for the test.
        unsafe { uaudio_set_params(hdl, AUMODE_PLAY, AUMODE_PLAY, &mut ap, &mut ar) }.unwrap();
        assert_eq!(sc.params.get().map(NonZeroUsize::get), Some(1));
        assert_eq!(sc.rate.get(), 48000);
        assert_eq!(
            (ap.sample_rate, ap.precision, ap.bps, ap.channels),
            (48000, 16, 2, 2)
        );

        // SAFETY: as above.
        let mut set = |b| unsafe { uaudio_set_blksz(hdl, AUMODE_PLAY, &mut ap, &mut ar, b) };
        assert_eq!(set(480), 480);
        assert_eq!(set(10), 96);
        assert_eq!(set(100_000), 1020);

        // no record parameters on a speaker
        sc.mode.set(AUMODE_RECORD);
        // SAFETY: as above.
        let r = unsafe { uaudio_set_params(hdl, AUMODE_RECORD, AUMODE_RECORD, &mut ap, &mut ar) };
        assert_eq!(r, Err(Errno::ENOTTY));
    }

    #[test]
    fn stream_sizing_at_48khz_full_speed() {
        let sc = softc_with(UaudioConf::default());
        let a = UaudioAlt {
            mode: AUMODE_PLAY,
            fps: 1000,
            maxpkt: 192,
            bps: 2,
            nch: 2,
            ..UaudioAlt::default()
        };
        let s = &sc.pstream;
        uaudio_stream_calc("uaudio0", s, &a, 48000, 50, 1920).unwrap();
        assert_eq!(s.spf.get(), 48 * UAUDIO_SPF_DIV);
        assert_eq!(s.spf_min.get(), 15_667_200);
        // limited by the 192-byte packets: 48 frames
        assert_eq!(s.spf_max.get(), 48 * UAUDIO_SPF_DIV);
        assert_eq!(s.nframes_mask.get(), 0);
        assert_eq!(s.safe_blksz.get(), 1984);
        assert_eq!(s.nframes_max.get(), 11);
        assert_eq!(s.maxpkt.get(), 192);
        assert_eq!(s.nxfers.get(), 4);

        // a block too large for the host's frames, then one too small
        assert_eq!(
            uaudio_stream_calc("uaudio0", s, &a, 48000, 50, 19200),
            Err(Errno::EIO)
        );
        assert_eq!(
            uaudio_stream_calc("uaudio0", s, &a, 48000, 50, 96),
            Err(Errno::EIO)
        );
        // more samples per frame than the packets carry
        let small = UaudioAlt { maxpkt: 100, ..a };
        assert_eq!(
            uaudio_stream_calc("uaudio0", s, &small, 48000, 50, 1920),
            Err(Errno::EIO)
        );

        // high speed at 8000 micro-frames: 1ms of frames per transfer
        let hs = UaudioAlt {
            fps: 8000,
            maxpkt: 256,
            ..a
        };
        uaudio_stream_calc("uaudio0", s, &hs, 48000, 240, 1920).unwrap();
        assert_eq!(s.nframes_mask.get(), 7);
        assert_eq!(s.nframes_max.get() & 7, 0);
    }

    #[test]
    fn play_transfers_cross_the_block_boundary() {
        let dev = FakeDev::new(&[0, 1], uac1_speaker_replies());
        let (_, conf) = parse(&dev, &uac1_speaker(), 1000);
        let sc = softc_with(conf);
        sc.params.set(NonZeroUsize::new(1));
        let a = *sc.palt();
        let s = &sc.pstream;
        uaudio_stream_calc("uaudio0", s, &a, 48000, 50, 1920).unwrap();
        s.ring_blksz.set(1920);

        let mut buf = vec![0xffu8; 192 * 11];
        let mut sizes = vec![0u16; 11];
        let x = &s.data_xfers[0];
        x.buf.set(buf.as_mut_ptr());
        x.buflen.set(buf.len() as u32);
        x.sizes.set(sizes.as_mut_ptr());
        x.nsizes.set(11);

        uaudio_pdata_calcsizes(&sc, x);
        assert_eq!(x.nframes.get(), 11);
        assert_eq!(x.size.get(), 2112);
        assert_eq!(s.ring_offs.get(), 192);
        uaudio_pdata_calcsizes(&sc, x);
        assert_eq!(x.nframes.get(), 10);
        assert_eq!(x.size.get(), 1920);
        assert_eq!(s.ring_offs.get(), 192);
        x.buf.set(ptr::null_mut());
        x.sizes.set(ptr::null_mut());
        assert!(sizes.iter().all(|&f| f == 192));
        assert!(buf[..1920].iter().all(|&b| b == 0));
    }

    #[test]
    fn play_copy_moves_the_ring_into_the_transfers() {
        let dev = FakeDev::new(&[0, 1], uac1_speaker_replies());
        let (_, conf) = parse(&dev, &uac1_speaker(), 1000);
        let sc = softc_with(conf);
        let s = &sc.pstream;
        s.nxfers.set(2);
        let mut ring: Vec<u8> = (0..64u8).collect();
        let range = ring.as_mut_ptr_range();
        s.ring_start.set(range.start);
        s.ring_end.set(range.end);
        s.ring_pos.set(range.start);
        let mut bufs = [vec![0u8; 48], vec![0u8; 48]];
        for (x, b) in s.data_xfers.iter().zip(bufs.iter_mut()) {
            x.buf.set(b.as_mut_ptr());
            x.buflen.set(48);
            x.size.set(40);
        }

        sc.copy_todo.set(70);
        uaudio_pdata_copy(&sc);
        // 40 bytes into the first transfer, 24 + 6 (after the ring wrapped) into the second
        assert_eq!(sc.copy_todo.get(), 0);
        assert_eq!(s.ubuf_xfer.get(), 1);
        assert_eq!(s.ubuf_pos.get(), 30);
        assert_eq!(s.ring_pos.get(), range.start.wrapping_add(6));
        for x in &s.data_xfers {
            x.buf.set(ptr::null_mut());
        }
        assert_eq!(bufs[0][..40], ring[..40]);
        assert_eq!(bufs[1][..24], ring[40..]);
        assert_eq!(bufs[1][24..30], ring[..6]);
    }

    #[test]
    fn display_name_drops_the_vendor() {
        let mut buf = [0u8; 32];
        let n = uaudio_display_name_do(
            "uaudio0",
            Some(b"QEMU"),
            Some(b"QEMU USB Audio"),
            0,
            &mut buf,
        );
        assert_eq!((n, cstr(&buf)), (9, "USB Audio"));
        let n = uaudio_display_name_do(
            "uaudio0",
            Some(b"QEMU"),
            Some(b"QEMU USB Audio"),
            1,
            &mut buf,
        );
        assert_eq!((n, cstr(&buf)), (11, "USB Audio#2"));
        let n = uaudio_display_name_do("uaudio1", None, Some(b"x"), 0, &mut buf);
        assert_eq!((n, cstr(&buf)), (7, "uaudio1"));
        let n = uaudio_display_name_do(
            "uaudio0",
            Some(b"Acme"),
            Some(b"Gadget\x01\xff!"),
            0,
            &mut buf,
        );
        assert_eq!((n, cstr(&buf)), (7, "Gadget!"));
        let mut small = [0u8; 4];
        let n = uaudio_display_name_do("uaudio0", Some(b"Acme"), Some(b"Gadget"), 2, &mut small);
        assert_eq!((n, cstr(&small)), (8, "Gad"));
    }

    #[test]
    fn an_all_zero_softc_is_a_valid_value() {
        let sc: Box<MaybeUninit<UaudioSoftc>> = Box::new_zeroed();
        // SAFETY: every member of `UaudioSoftc` is valid as zero bits (its `Softc` impl).
        let sc = unsafe { sc.assume_init() };
        assert!(sc.udev.get().is_none());
        assert!(sc.conf.get().is_none());
        assert!(sc.params.get().is_none());
        assert!(sc.pstream.intr.get().is_none());
        assert!(sc.pstream.data_xfers[0].sizes().is_empty());
    }
}
/* </TESTS> */
