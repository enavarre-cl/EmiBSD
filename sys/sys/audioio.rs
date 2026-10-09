/* <LICENSES> */
/*
 * Copyright (c) 1991-1993 Regents of the University of California.
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *	This product includes software developed by the Computer Systems
 *	Engineering Group at Lawrence Berkeley Laboratory.
 * 4. Neither the name of the University nor of the Laboratory may be used
 *    to endorse or promote products derived from this software without
 *    specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE REGENTS AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 */
/* </LICENSES> */

/* <CODE> */
//! `<sys/audioio.h>`: the audio(4) and mixer ioctls and the structures they carry.
//!
//! Upstream: sys/sys/audioio.h @ 3ce1f3f79392
//!
//! Every structure here crosses the `ioctl(2)` boundary, so each is `#[repr(C)]` with the C
//! layout (no implicit padding, which a test pins) and is `AbiPod`: handlers read it with
//! `ioctl_arg` and write it back with `ioctl_ret`.
//!
//! ## Deviations
//! - `AUDIO_INITPAR(p)` (a `memset` to `0xff`) is [`AudioSwpar::initpar`]; the C macro has
//!   no other meaning.
//! - The unions `mixer_devinfo.un` and `mixer_ctrl.un` are 4-aligned byte arrays of the
//!   union's size ([`MixerDevinfoUn`], [`MixerCtrlUn`]) with one view per member
//!   (`e()`/`e_mut()`, `s()`, `v()`; `ord()`, `mask()`, `value()`), instead of Rust `union`s:
//!   a Rust union written through a member smaller than the union leaves bytes
//!   uninitialised, and `AbiPod` promises that every byte of the structure is initialised.
//! - The typedef names (`audio_device_t`, `mixer_level_t`, `mixer_devinfo_t`,
//!   `mixer_ctrl_t`, `audio_mixer_name_t`) and the struct tags are one Rust type each.
//! - The `AudioN*`/`AudioC*` names are byte strings without the NUL; `strlcpy` adds it.

use core::mem::size_of;

use crate::machine::copy::AbiPod;
use crate::sys::ioccom::{_io, _ior, _iowr};

/// `AUMODE_PLAY`.
pub const AUMODE_PLAY: i32 = 0x01;
/// `AUMODE_RECORD`.
pub const AUMODE_RECORD: i32 = 0x02;

/// `struct audio_swpar`: argument to `AUDIO_SETPAR` and `AUDIO_GETPAR`. A member left at
/// `~0` (see [`AudioSwpar::initpar`]) is not changed by `AUDIO_SETPAR`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AudioSwpar {
    /// `sig`: if 1, encoding is signed.
    pub sig: u32,
    /// `le`: if 1, encoding is little-endian.
    pub le: u32,
    /// `bits`: bits per sample.
    pub bits: u32,
    /// `bps`: bytes per sample.
    pub bps: u32,
    /// `msb`: if 1, bits are msb-aligned.
    pub msb: u32,
    /// `rate`: common play & rec sample rate.
    pub rate: u32,
    /// `pchan`: play channels.
    pub pchan: u32,
    /// `rchan`: rec channels.
    pub rchan: u32,
    /// `nblks`: number of blocks in play buffer.
    pub nblks: u32,
    /// `round`: common frames per block.
    pub round: u32,
    /// `_spare`.
    pub _spare: [u32; 6],
}

impl AudioSwpar {
    /// `AUDIO_INITPAR(p)`: every member set to all ones, "not specified".
    pub const fn initpar() -> Self {
        Self {
            sig: !0,
            le: !0,
            bits: !0,
            bps: !0,
            msb: !0,
            rate: !0,
            pchan: !0,
            rchan: !0,
            nblks: !0,
            round: !0,
            _spare: [!0; 6],
        }
    }
}

// SAFETY: sixteen `unsigned int`s, no padding; any bit pattern is a valid value.
unsafe impl AbiPod for AudioSwpar {}

/// `struct audio_status`: argument to `AUDIO_GETSTATUS`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AudioStatus {
    /// `mode`: `AUMODE_*`.
    pub mode: i32,
    /// `pause`.
    pub pause: i32,
    /// `active`.
    pub active: i32,
    /// `_spare`.
    pub _spare: [i32; 5],
}

// SAFETY: eight `int`s, no padding; any bit pattern is a valid value.
unsafe impl AbiPod for AudioStatus {}

/// `MAX_AUDIO_DEV_LEN`.
pub const MAX_AUDIO_DEV_LEN: usize = 16;

/// `struct audio_device` (`audio_device_t`): parameter for the `AUDIO_GETDEV` ioctl, to
/// determine current audio devices.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AudioDevice {
    /// `name`.
    pub name: [u8; MAX_AUDIO_DEV_LEN],
    /// `version`.
    pub version: [u8; MAX_AUDIO_DEV_LEN],
    /// `config`.
    pub config: [u8; MAX_AUDIO_DEV_LEN],
}

// SAFETY: three byte arrays; any bit pattern is a valid value.
unsafe impl AbiPod for AudioDevice {}

/// `struct audio_pos`: argument to `AUDIO_GETPOS`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AudioPos {
    /// `play_pos`: total bytes played.
    pub play_pos: u32,
    /// `play_xrun`: bytes of silence inserted.
    pub play_xrun: u32,
    /// `rec_pos`: total bytes recorded.
    pub rec_pos: u32,
    /// `rec_xrun`: bytes dropped.
    pub rec_xrun: u32,
}

// SAFETY: four `unsigned int`s, no padding; any bit pattern is a valid value.
unsafe impl AbiPod for AudioPos {}

// Audio device operations

/// `AUDIO_GETDEV`.
pub const AUDIO_GETDEV: u64 = _ior::<AudioDevice>(b'A', 27);
/// `AUDIO_GETPOS`.
pub const AUDIO_GETPOS: u64 = _ior::<AudioPos>(b'A', 35);
/// `AUDIO_GETPAR`.
pub const AUDIO_GETPAR: u64 = _ior::<AudioSwpar>(b'A', 36);
/// `AUDIO_SETPAR`.
pub const AUDIO_SETPAR: u64 = _iowr::<AudioSwpar>(b'A', 37);
/// `AUDIO_START`.
pub const AUDIO_START: u64 = _io(b'A', 38);
/// `AUDIO_STOP`.
pub const AUDIO_STOP: u64 = _io(b'A', 39);
/// `AUDIO_GETSTATUS`.
pub const AUDIO_GETSTATUS: u64 = _ior::<AudioStatus>(b'A', 40);

// Mixer device

/// `AUDIO_MIN_GAIN`.
pub const AUDIO_MIN_GAIN: i32 = 0;
/// `AUDIO_MAX_GAIN`.
pub const AUDIO_MAX_GAIN: i32 = 255;

/// `struct mixer_level` (`mixer_level_t`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MixerLevel {
    /// `num_channels`.
    pub num_channels: i32,
    /// `level[num_channels]`.
    pub level: [u8; 8],
}

// SAFETY: an `int` and eight bytes, no padding; any bit pattern is a valid value.
unsafe impl AbiPod for MixerLevel {}

/// `AUDIO_MIXER_LEVEL_MONO`.
pub const AUDIO_MIXER_LEVEL_MONO: usize = 0;
/// `AUDIO_MIXER_LEVEL_LEFT`.
pub const AUDIO_MIXER_LEVEL_LEFT: usize = 0;
/// `AUDIO_MIXER_LEVEL_RIGHT`.
pub const AUDIO_MIXER_LEVEL_RIGHT: usize = 1;

// Device operations

/// `struct audio_mixer_name` (`audio_mixer_name_t`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AudioMixerName {
    /// `name`.
    pub name: [u8; MAX_AUDIO_DEV_LEN],
    /// `msg_id`.
    pub msg_id: i32,
}

/// `AUDIO_MIXER_CLASS`.
pub const AUDIO_MIXER_CLASS: i32 = 0;
/// `AUDIO_MIXER_ENUM`.
pub const AUDIO_MIXER_ENUM: i32 = 1;
/// `AUDIO_MIXER_SET`.
pub const AUDIO_MIXER_SET: i32 = 2;
/// `AUDIO_MIXER_VALUE`.
pub const AUDIO_MIXER_VALUE: i32 = 3;
/// `AUDIO_MIXER_LAST`.
pub const AUDIO_MIXER_LAST: i32 = -1;

/// A member of `struct audio_mixer_enum`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AudioMixerEnumMember {
    /// `label`.
    pub label: AudioMixerName,
    /// `ord`.
    pub ord: i32,
}

/// `struct audio_mixer_enum`: the members of an `AUDIO_MIXER_ENUM` control.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioMixerEnum {
    /// `num_mem`.
    pub num_mem: i32,
    /// `member`.
    pub member: [AudioMixerEnumMember; 32],
}

/// A member of `struct audio_mixer_set`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AudioMixerSetMember {
    /// `label`.
    pub label: AudioMixerName,
    /// `mask`.
    pub mask: i32,
}

/// `struct audio_mixer_set`: the members of an `AUDIO_MIXER_SET` control.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioMixerSet {
    /// `num_mem`.
    pub num_mem: i32,
    /// `member`.
    pub member: [AudioMixerSetMember; 32],
}

/// `struct audio_mixer_value`: the shape of an `AUDIO_MIXER_VALUE` control.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AudioMixerValue {
    /// `units`.
    pub units: AudioMixerName,
    /// `num_channels`.
    pub num_channels: i32,
    /// `delta`.
    pub delta: i32,
}

/// The size of `mixer_devinfo.un`: its largest member.
const MIXER_DEVINFO_UN_SIZE: usize = size_of::<AudioMixerEnum>();

/// `union { e, s, v } un` of `struct mixer_devinfo`: the union's bytes, 4-aligned as in C,
/// read and written through one view per member.
#[repr(C, align(4))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MixerDevinfoUn([u8; MIXER_DEVINFO_UN_SIZE]);

impl MixerDevinfoUn {
    /// All zeros.
    pub const fn zeroed() -> Self {
        Self([0; MIXER_DEVINFO_UN_SIZE])
    }

    /// `un.e`.
    pub fn e(&self) -> &AudioMixerEnum {
        view(&self.0)
    }

    /// `un.e`, to change.
    pub fn e_mut(&mut self) -> &mut AudioMixerEnum {
        view_mut(&mut self.0)
    }

    /// `un.s`.
    pub fn s(&self) -> &AudioMixerSet {
        view(&self.0)
    }

    /// `un.s`, to change.
    pub fn s_mut(&mut self) -> &mut AudioMixerSet {
        view_mut(&mut self.0)
    }

    /// `un.v`.
    pub fn v(&self) -> &AudioMixerValue {
        view(&self.0)
    }

    /// `un.v`, to change.
    pub fn v_mut(&mut self) -> &mut AudioMixerValue {
        view_mut(&mut self.0)
    }
}

/// `struct mixer_devinfo` (`mixer_devinfo_t`): one mixer control, as `AUDIO_MIXER_DEVINFO`
/// and the drivers' `query_devinfo` describe it.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MixerDevinfo {
    /// `index`.
    pub index: i32,
    /// `label`.
    pub label: AudioMixerName,
    /// `type`: `AUDIO_MIXER_*`.
    pub type_: i32,
    /// `mixer_class`.
    pub mixer_class: i32,
    /// `next`.
    pub next: i32,
    /// `prev`.
    pub prev: i32,
    /// `un`.
    pub un: MixerDevinfoUn,
}

impl MixerDevinfo {
    /// All zeros.
    pub const fn zeroed() -> Self {
        Self {
            index: 0,
            label: AudioMixerName {
                name: [0; MAX_AUDIO_DEV_LEN],
                msg_id: 0,
            },
            type_: 0,
            mixer_class: 0,
            next: 0,
            prev: 0,
            un: MixerDevinfoUn::zeroed(),
        }
    }
}

// SAFETY: `int`s and byte arrays (the union is bytes), no padding; any bit pattern is a
// valid value.
unsafe impl AbiPod for MixerDevinfo {}

/// The size of `mixer_ctrl.un`: its largest member.
const MIXER_CTRL_UN_SIZE: usize = size_of::<MixerLevel>();

/// `union { ord, mask, value } un` of `struct mixer_ctrl`: the union's bytes, 4-aligned as
/// in C, read and written through one view per member.
#[repr(C, align(4))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MixerCtrlUn([u8; MIXER_CTRL_UN_SIZE]);

impl MixerCtrlUn {
    /// All zeros.
    pub const fn zeroed() -> Self {
        Self([0; MIXER_CTRL_UN_SIZE])
    }

    /// `un.ord`: an enum control's value.
    pub fn ord(&self) -> i32 {
        *view::<i32>(&self.0)
    }

    /// Sets `un.ord`.
    pub fn set_ord(&mut self, ord: i32) {
        *view_mut::<i32>(&mut self.0) = ord;
    }

    /// `un.mask`: a set control's value.
    pub fn mask(&self) -> i32 {
        *view::<i32>(&self.0)
    }

    /// Sets `un.mask`.
    pub fn set_mask(&mut self, mask: i32) {
        *view_mut::<i32>(&mut self.0) = mask;
    }

    /// `un.value`: a value control's levels.
    pub fn value(&self) -> &MixerLevel {
        view(&self.0)
    }

    /// `un.value`, to change.
    pub fn value_mut(&mut self) -> &mut MixerLevel {
        view_mut(&mut self.0)
    }
}

/// `struct mixer_ctrl` (`mixer_ctrl_t`): the value of one mixer control.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MixerCtrl {
    /// `dev`: the control's index.
    pub dev: i32,
    /// `type`: `AUDIO_MIXER_*`.
    pub type_: i32,
    /// `un`.
    pub un: MixerCtrlUn,
}

// SAFETY: two `int`s and the union's bytes, no padding; any bit pattern is a valid value.
unsafe impl AbiPod for MixerCtrl {}

// Mixer operations

/// `AUDIO_MIXER_READ`.
pub const AUDIO_MIXER_READ: u64 = _iowr::<MixerCtrl>(b'M', 0);
/// `AUDIO_MIXER_WRITE`.
pub const AUDIO_MIXER_WRITE: u64 = _iowr::<MixerCtrl>(b'M', 1);
/// `AUDIO_MIXER_DEVINFO`.
pub const AUDIO_MIXER_DEVINFO: u64 = _iowr::<MixerDevinfo>(b'M', 2);

// Well known device names

/// `AudioNmicrophone`.
#[allow(non_upper_case_globals)] // the C name
pub const AudioNmicrophone: &[u8] = b"mic";
/// `AudioNline`.
#[allow(non_upper_case_globals)] // the C name
pub const AudioNline: &[u8] = b"line";
/// `AudioNcd`.
#[allow(non_upper_case_globals)] // the C name
pub const AudioNcd: &[u8] = b"cd";
/// `AudioNdac`.
#[allow(non_upper_case_globals)] // the C name
pub const AudioNdac: &[u8] = b"dac";
/// `AudioNaux`.
#[allow(non_upper_case_globals)] // the C name
pub const AudioNaux: &[u8] = b"aux";
/// `AudioNrecord`.
#[allow(non_upper_case_globals)] // the C name
pub const AudioNrecord: &[u8] = b"record";
/// `AudioNvolume`.
#[allow(non_upper_case_globals)] // the C name
pub const AudioNvolume: &[u8] = b"volume";
/// `AudioNmonitor`.
#[allow(non_upper_case_globals)] // the C name
pub const AudioNmonitor: &[u8] = b"monitor";
/// `AudioNtreble`.
#[allow(non_upper_case_globals)] // the C name
pub const AudioNtreble: &[u8] = b"treble";
/// `AudioNmid`.
#[allow(non_upper_case_globals)] // the C name
pub const AudioNmid: &[u8] = b"mid";
/// `AudioNbass`.
#[allow(non_upper_case_globals)] // the C name
pub const AudioNbass: &[u8] = b"bass";
/// `AudioNbassboost`.
#[allow(non_upper_case_globals)] // the C name
pub const AudioNbassboost: &[u8] = b"bassboost";
/// `AudioNspeaker`.
#[allow(non_upper_case_globals)] // the C name
pub const AudioNspeaker: &[u8] = b"spkr";
/// `AudioNheadphone`.
#[allow(non_upper_case_globals)] // the C name
pub const AudioNheadphone: &[u8] = b"hp";
/// `AudioNoutput`.
#[allow(non_upper_case_globals)] // the C name
pub const AudioNoutput: &[u8] = b"output";
/// `AudioNinput`.
#[allow(non_upper_case_globals)] // the C name
pub const AudioNinput: &[u8] = b"input";
/// `AudioNmaster`.
#[allow(non_upper_case_globals)] // the C name
pub const AudioNmaster: &[u8] = b"master";
/// `AudioNstereo`.
#[allow(non_upper_case_globals)] // the C name
pub const AudioNstereo: &[u8] = b"stereo";
/// `AudioNmono`.
#[allow(non_upper_case_globals)] // the C name
pub const AudioNmono: &[u8] = b"mono";
/// `AudioNloudness`.
#[allow(non_upper_case_globals)] // the C name
pub const AudioNloudness: &[u8] = b"loudness";
/// `AudioNspatial`.
#[allow(non_upper_case_globals)] // the C name
pub const AudioNspatial: &[u8] = b"spatial";
/// `AudioNsurround`.
#[allow(non_upper_case_globals)] // the C name
pub const AudioNsurround: &[u8] = b"surround";
/// `AudioNpseudo`.
#[allow(non_upper_case_globals)] // the C name
pub const AudioNpseudo: &[u8] = b"pseudo";
/// `AudioNmute`.
#[allow(non_upper_case_globals)] // the C name
pub const AudioNmute: &[u8] = b"mute";
/// `AudioNenhanced`.
#[allow(non_upper_case_globals)] // the C name
pub const AudioNenhanced: &[u8] = b"enhanced";
/// `AudioNpreamp`.
#[allow(non_upper_case_globals)] // the C name
pub const AudioNpreamp: &[u8] = b"preamp";
/// `AudioNon`.
#[allow(non_upper_case_globals)] // the C name
pub const AudioNon: &[u8] = b"on";
/// `AudioNoff`.
#[allow(non_upper_case_globals)] // the C name
pub const AudioNoff: &[u8] = b"off";
/// `AudioNmode`.
#[allow(non_upper_case_globals)] // the C name
pub const AudioNmode: &[u8] = b"mode";
/// `AudioNsource`.
#[allow(non_upper_case_globals)] // the C name
pub const AudioNsource: &[u8] = b"source";
/// `AudioNfmsynth`.
#[allow(non_upper_case_globals)] // the C name
pub const AudioNfmsynth: &[u8] = b"fmsynth";
/// `AudioNwave`.
#[allow(non_upper_case_globals)] // the C name
pub const AudioNwave: &[u8] = b"wave";
/// `AudioNmidi`.
#[allow(non_upper_case_globals)] // the C name
pub const AudioNmidi: &[u8] = b"midi";
/// `AudioNmixerout`.
#[allow(non_upper_case_globals)] // the C name
pub const AudioNmixerout: &[u8] = b"mixerout";
/// `AudioNswap`: swap left and right channels.
#[allow(non_upper_case_globals)] // the C name
pub const AudioNswap: &[u8] = b"swap";
/// `AudioNagc`.
#[allow(non_upper_case_globals)] // the C name
pub const AudioNagc: &[u8] = b"agc";
/// `AudioNdelay`.
#[allow(non_upper_case_globals)] // the C name
pub const AudioNdelay: &[u8] = b"delay";
/// `AudioNselect`: select destination.
#[allow(non_upper_case_globals)] // the C name
pub const AudioNselect: &[u8] = b"select";
/// `AudioNvideo`.
#[allow(non_upper_case_globals)] // the C name
pub const AudioNvideo: &[u8] = b"video";
/// `AudioNcenter`.
#[allow(non_upper_case_globals)] // the C name
pub const AudioNcenter: &[u8] = b"center";
/// `AudioNdepth`.
#[allow(non_upper_case_globals)] // the C name
pub const AudioNdepth: &[u8] = b"depth";
/// `AudioNlfe`.
#[allow(non_upper_case_globals)] // the C name
pub const AudioNlfe: &[u8] = b"lfe";
/// `AudioNextamp`.
#[allow(non_upper_case_globals)] // the C name
pub const AudioNextamp: &[u8] = b"extamp";

/// `AudioCinputs`.
#[allow(non_upper_case_globals)] // the C name
pub const AudioCinputs: &[u8] = b"inputs";
/// `AudioCoutputs`.
#[allow(non_upper_case_globals)] // the C name
pub const AudioCoutputs: &[u8] = b"outputs";
/// `AudioCrecord`.
#[allow(non_upper_case_globals)] // the C name
pub const AudioCrecord: &[u8] = b"record";
/// `AudioCmonitor`.
#[allow(non_upper_case_globals)] // the C name
pub const AudioCmonitor: &[u8] = b"monitor";
/// `AudioCequalization`.
#[allow(non_upper_case_globals)] // the C name
pub const AudioCequalization: &[u8] = b"equalization";

/// Marks the plain-integer types a [`MixerDevinfoUn`] or [`MixerCtrlUn`] view may have.
///
/// # Safety
///
/// The type is `#[repr(C)]`, made of `int`s and byte arrays only, with no padding and an
/// alignment of at most 4: every bit pattern is a valid value.
unsafe trait UnionView: Copy {}

// SAFETY: `int`s and byte arrays, 4-aligned, no padding.
unsafe impl UnionView for AudioMixerEnum {}
// SAFETY: as above.
unsafe impl UnionView for AudioMixerSet {}
// SAFETY: as above.
unsafe impl UnionView for AudioMixerValue {}
// SAFETY: as above.
unsafe impl UnionView for MixerLevel {}
// SAFETY: one `int`.
unsafe impl UnionView for i32 {}

/// `&T` over the first bytes of a 4-aligned union area `bytes`.
fn view<T: UnionView>(bytes: &[u8]) -> &T {
    assert!(
        size_of::<T>() <= bytes.len(),
        "union view larger than the union"
    );
    // SAFETY: `bytes` starts a 4-aligned area (the union types are `align(4)`) and holds
    // at least `size_of::<T>()` bytes; `T: UnionView` makes any bytes a valid `T` with an
    // alignment of at most 4; the borrow of `bytes` covers the returned reference.
    unsafe { &*bytes.as_ptr().cast::<T>() }
}

/// `&mut T` over the first bytes of a 4-aligned union area `bytes`.
fn view_mut<T: UnionView>(bytes: &mut [u8]) -> &mut T {
    assert!(
        size_of::<T>() <= bytes.len(),
        "union view larger than the union"
    );
    // SAFETY: as in `view`; the exclusive borrow of `bytes` makes the view exclusive, and
    // whatever is written through it leaves every byte initialised.
    unsafe { &mut *bytes.as_mut_ptr().cast::<T>() }
}

const _: () = {
    assert!(size_of::<AudioSwpar>() == 64);
    assert!(size_of::<AudioStatus>() == 32);
    assert!(size_of::<AudioDevice>() == 48);
    assert!(size_of::<AudioPos>() == 16);
    assert!(size_of::<MixerLevel>() == 12);
    assert!(size_of::<AudioMixerName>() == 20);
    assert!(size_of::<AudioMixerEnum>() == 772);
    assert!(size_of::<AudioMixerSet>() == 772);
    assert!(size_of::<AudioMixerValue>() == 28);
    assert!(size_of::<MixerDevinfo>() == 812);
    assert!(size_of::<MixerCtrl>() == 20);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
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
}
/* </TESTS> */
