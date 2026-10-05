/*	$OpenBSD: ac97.h,v 1.27 2018/04/11 04:48:31 ratchov Exp $	*/
/*	$OpenBSD: ac97.c,v 1.85 2024/04/29 00:29:48 jsg Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1999 Constantine Sapuntzakis
 *
 * Author:	Constantine Sapuntzakis <csapuntz@stanford.edu>
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
 * THIS SOFTWARE IS PROVIDED BY CONSTANTINE SAPUNTZAKIS AND CONTRIBUTORS
 * ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */
/*
 * Copyright (c) 1999, 2000 Constantine Sapuntzakis
 *
 * Author:	Constantine Sapuntzakis <csapuntz@stanford.edu>
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. The name of the author may not be used to endorse or promote
 *    products derived from this software without specific prior written
 *    permission.
 * THIS SOFTWARE IS PROVIDED BY THE AUTHORS ``AS IS'' AND ANY EXPRESS
 * OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
 * WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE
 * LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT
 * OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR
 * BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF
 * LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
 * USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH
 * DAMAGE.  */

/* Partially inspired by FreeBSD's sys/dev/pcm/ac97.c. It came with
the following copyright */

/*
 * Copyright (c) 1999 Cameron Grant <gandalf@vilnya.demon.co.uk>
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
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 * $FreeBSD$
 */

/* </LICENSES> */

//! `ac97(4)`: the AC'97 codec layer between a controller driver (auich(4), and others) and
//! audio(4): codec probe, the mixer, and the sample-rate registers.
//!
//! Upstream: sys/dev/ic/ac97.c @ 3ce1f3f79392
//! Upstream: sys/dev/ic/ac97.h @ 3ce1f3f79392
//!
//! The controller driver fills an [`Ac97HostIf`] (how to read and write the codec's
//! registers, reset the link) and calls [`ac97_attach`]; the codec layer allocates its softc
//! ([`Ac97Softc`], whose first member is the [`Ac97CodecIf`] the host gets back through
//! `attach`), probes the codec's ID and capabilities, builds the mixer from the table of
//! sources the codec's capabilities allow, and runs the codec-specific initialisation. The
//! host then forwards audio(4)'s mixer methods to the codec's `vtbl`
//! ([`Ac97CodecIfVtbl`]) and calls [`ac97_set_rate`] from `set_params`.
//!
//! ## Deviations
//! - `struct ac97_host_if` is `Copy` and every method is an `Option` (the C's NULL), so the
//!   host's softc can embed one as all-zero bits; the codec's softc keeps a copy taken by
//!   `ac97_attach`, where the C keeps the host's pointer. The host never changes it after
//!   `ac97_attach`.
//! - The `read` and `write` methods of the host return `Result<(), Errno>`; auich(4)'s `-1`
//!   on a codec access timeout is `EIO` here (`-1` is `ERESTART` in the Rust `Errno`, which
//!   no caller of the mixer expects).
//! - The `(struct ac97_softc *)codec_if` casts of the vtbl functions are the codec's `as`
//!   member (which `ac97_attach` points at the softc, as the C does for `ac97_resume`).
//! - `ac97_read` returns the register (the shadow copy after a failed access) instead of
//!   filling `*val` and returning the error; no caller in the C looks at that error.
//! - `const struct ac97_source_info` keeps `class`, `device` and `qualifier` as
//!   `Option<&[u8]>` (NULL is `None`, `ac97_str_equal` is `==`), `info` as the two
//!   `Option`s `info_enum`/`info_value` (the C's `void *` and its `sizeof`), and the bit
//!   fields `bits:3, ofs:4, mute:1, polarity:1` as `u8`/`bool`. Copying `info` into
//!   `mixer_devinfo.un` is the whole member (`sizeof(struct audio_mixer_enum)` in C).
//! - The 16-bit register arithmetic of the mixer (`newval`, `mask`, shifts up to
//!   `ofs + 8`) is done in 32 bits and truncated at each assignment, as the C's integer
//!   promotions and `u_int16_t` stores do, so no shift overflows.
//! - `ac97_add_port` returns `-1` for a class it cannot find where the C indexes
//!   `source_info[-1]`; every class exists.
//! - `ac97_set_rate` computes in `u64` (`u_long`) and wraps where the C's unsigned
//!   arithmetic does (`*rate` is chosen by the user).
//! - `AUDIO_DEBUG`/`AC97_DEBUG` (the `DPRINTF`s) are not configured.
//! - `AC97_IS_FIXED_RATE` and `AC97_CAPS_ENHANCEMENT` are the functions
//!   [`ac97_is_fixed_rate`] and [`ac97_caps_enhancement`].

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr;

use libkern::strlcpy;

use crate::kern::kern_malloc::{free, malloc};
use crate::kern::subr_prf::{panic, printf};
use crate::machine::cpu::delay;
use crate::sys::audioio::{
    AUDIO_MIXER_CLASS, AUDIO_MIXER_ENUM, AUDIO_MIXER_LAST, AUDIO_MIXER_LEVEL_LEFT,
    AUDIO_MIXER_LEVEL_MONO, AUDIO_MIXER_LEVEL_RIGHT, AUDIO_MIXER_VALUE, AudioCinputs,
    AudioCoutputs, AudioCrecord, AudioMixerEnum, AudioMixerEnumMember, AudioMixerName,
    AudioMixerValue, AudioNaux, AudioNcd, AudioNcenter, AudioNdac, AudioNextamp, AudioNheadphone,
    AudioNlfe, AudioNline, AudioNloudness, AudioNmaster, AudioNmicrophone, AudioNmixerout,
    AudioNmono, AudioNmute, AudioNoff, AudioNon, AudioNpreamp, AudioNsource, AudioNspatial,
    AudioNspeaker, AudioNsurround, AudioNvideo, AudioNvolume, MixerCtrl, MixerDevinfo,
};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_DEVBUF, M_NOWAIT, M_ZERO};

/// `AC97_HOST_DONT_READ`: `enum ac97_host_flags`.
pub const AC97_HOST_DONT_READ: u32 = 0x1;
/// `AC97_HOST_DONT_READANY`.
pub const AC97_HOST_DONT_READANY: u32 = 0x2;
/// `AC97_HOST_SWAPPED_CHANNELS`.
pub const AC97_HOST_SWAPPED_CHANNELS: u32 = 0x4;
/// `AC97_HOST_ALC650_PIN47_IS_EAPD`.
pub const AC97_HOST_ALC650_PIN47_IS_EAPD: u32 = 0x8;
/// `AC97_HOST_VT1616_DYNEX`.
pub const AC97_HOST_VT1616_DYNEX: u32 = 0x10;

/// `int (*attach)(void *arg, struct ac97_codec_if *codecif)`: the host keeps the codec
/// interface.
///
/// # Safety
///
/// `arg` is the `arg` of the [`Ac97HostIf`] this method belongs to.
pub type Ac97HostAttachFn = unsafe fn(*mut c_void, &Ac97CodecIf) -> Result<(), Errno>;
/// `int (*read)(void *arg, u_int8_t reg, u_int16_t *val)`.
///
/// # Safety
///
/// As for [`Ac97HostAttachFn`].
pub type Ac97HostReadFn = unsafe fn(*mut c_void, u8, &mut u16) -> Result<(), Errno>;
/// `int (*write)(void *arg, u_int8_t reg, u_int16_t val)`.
///
/// # Safety
///
/// As for [`Ac97HostAttachFn`].
pub type Ac97HostWriteFn = unsafe fn(*mut c_void, u8, u16) -> Result<(), Errno>;
/// `void (*reset)(void *arg)`.
///
/// # Safety
///
/// As for [`Ac97HostAttachFn`].
pub type Ac97HostResetFn = unsafe fn(*mut c_void);
/// `enum ac97_host_flags (*flags)(void *arg)`.
///
/// # Safety
///
/// As for [`Ac97HostAttachFn`].
pub type Ac97HostFlagsFn = unsafe fn(*mut c_void) -> u32;
/// `void (*spdif_event)(void *arg, int)`.
///
/// # Safety
///
/// As for [`Ac97HostAttachFn`].
pub type Ac97HostSpdifEventFn = unsafe fn(*mut c_void, i32);

/// `struct ac97_host_if`: the interface a host controller uses to attach the AC97 compliant
/// codec. `attach`, `read`, `write` and `reset` are mandatory; `flags` and `spdif_event` may
/// be `None`.
#[derive(Clone, Copy)]
pub struct Ac97HostIf {
    /// `arg`: handed to every method.
    pub arg: *mut c_void,
    /// `attach`.
    pub attach: Option<Ac97HostAttachFn>,
    /// `read`.
    pub read: Option<Ac97HostReadFn>,
    /// `write`.
    pub write: Option<Ac97HostWriteFn>,
    /// `reset`.
    pub reset: Option<Ac97HostResetFn>,
    /// `flags`.
    pub flags: Option<Ac97HostFlagsFn>,
    /// `spdif_event`.
    pub spdif_event: Option<Ac97HostSpdifEventFn>,
}

impl Ac97HostIf {
    /// All NULL.
    pub const fn new() -> Self {
        Self {
            arg: ptr::null_mut(),
            attach: None,
            read: None,
            write: None,
            reset: None,
            flags: None,
            spdif_event: None,
        }
    }
}

impl Default for Ac97HostIf {
    fn default() -> Self {
        Self::new()
    }
}

/// A mandatory method of the host: `host_if->$m`, or a panic naming it where the C would
/// call through NULL.
macro_rules! host_method {
    ($hi:expr, $m:ident) => {
        match $hi.$m {
            Some(f) => f,
            None => panic(format_args!("ac97: host_if->{} is NULL", stringify!($m))),
        }
    };
}

/// `int (*mixer_get_port)(struct ac97_codec_if *, mixer_ctrl_t *)`, and `mixer_set_port`.
pub type Ac97MixerPortFn = fn(&Ac97CodecIf, &mut MixerCtrl) -> Result<(), Errno>;
/// `int (*query_devinfo)(struct ac97_codec_if *, mixer_devinfo_t *)`.
pub type Ac97QueryDevinfoFn = fn(&Ac97CodecIf, &mut MixerDevinfo) -> Result<(), Errno>;
/// `int (*get_portnum_by_name)(struct ac97_codec_if *, char *class, char *device, char
/// *qualifier)`: the control's index, `-1` for none.
pub type Ac97GetPortnumFn = fn(&Ac97CodecIf, Option<&[u8]>, Option<&[u8]>, Option<&[u8]>) -> i32;
/// `u_int16_t (*get_caps)(struct ac97_codec_if *)`.
pub type Ac97GetCapsFn = fn(&Ac97CodecIf) -> u16;
/// `int (*set_rate)(struct ac97_codec_if *, int target, u_long *rate)`.
pub type Ac97SetRateFn = fn(&Ac97CodecIf, i32, &mut u64) -> Result<(), Errno>;
/// `void (*set_clock)(struct ac97_codec_if *, unsigned int clock)`.
pub type Ac97SetClockFn = fn(&Ac97CodecIf, u32);
/// `void (*lock)(struct ac97_codec_if *)`, and `unlock`.
pub type Ac97LockFn = fn(&Ac97CodecIf);

/// `struct ac97_codec_if_vtbl`: the interface exported by the AC97 compliant codec.
pub struct Ac97CodecIfVtbl {
    /// `mixer_get_port`.
    pub mixer_get_port: Ac97MixerPortFn,
    /// `mixer_set_port`.
    pub mixer_set_port: Ac97MixerPortFn,
    /// `query_devinfo`.
    pub query_devinfo: Ac97QueryDevinfoFn,
    /// `get_portnum_by_name`.
    pub get_portnum_by_name: Ac97GetPortnumFn,
    /// `get_caps`: the extended audio ID register.
    pub get_caps: Ac97GetCapsFn,
    /// `set_rate`.
    pub set_rate: Ac97SetRateFn,
    /// `set_clock`.
    pub set_clock: Ac97SetClockFn,
    /// `lock`.
    pub lock: Ac97LockFn,
    /// `unlock`.
    pub unlock: Ac97LockFn,
}

/// `void (*initfunc)(struct ac97_softc *, int)`: a codec's own initialisation; the second
/// argument says it is a resume.
pub type Ac97InitFn = fn(&Ac97Softc, i32);

/// `struct ac97_codec_if`: what the host gets from `ac97_attach`. Only [`Ac97Softc`] makes
/// one, as its first member.
#[repr(C)]
pub struct Ac97CodecIf {
    /// `as`: the softc this interface heads.
    as_: Cell<*const Ac97Softc>,
    /// `initfunc`.
    pub initfunc: Cell<Option<Ac97InitFn>>,
    /// `vtbl`.
    pub vtbl: &'static Ac97CodecIfVtbl,
}

impl Ac97CodecIf {
    /// `codec_if->as`.
    fn softc(&self) -> &Ac97Softc {
        let p = self.as_.get();
        if p.is_null() {
            panic(format_args!("ac97: codec_if without a softc"));
        }
        // SAFETY: `ac97_attach` points `as_` at the `Ac97Softc` whose first member this
        // interface is, and that softc is never freed once `ac97_attach` returned success.
        unsafe { &*p }
    }
}

/// `ac97civ`: the codec's vtbl.
pub static AC97CIV: Ac97CodecIfVtbl = Ac97CodecIfVtbl {
    mixer_get_port: ac97_mixer_get_port,
    mixer_set_port: ac97_mixer_set_port,
    query_devinfo: ac97_query_devinfo,
    get_portnum_by_name: ac97_get_portnum_by_name,
    get_caps: ac97_get_extcaps,
    set_rate: ac97_set_rate,
    set_clock: ac97_set_clock,
    lock: ac97_lock,
    unlock: ac97_unlock,
};

/// `AC97_REG_RESET`.
pub const AC97_REG_RESET: u8 = 0x00;
/// `AC97_CAPS_MICIN`.
pub const AC97_CAPS_MICIN: u16 = 0x0001;
/// `AC97_CAPS_TONECTRL`.
pub const AC97_CAPS_TONECTRL: u16 = 0x0004;
/// `AC97_CAPS_SIMSTEREO`.
pub const AC97_CAPS_SIMSTEREO: u16 = 0x0008;
/// `AC97_CAPS_HEADPHONES`.
pub const AC97_CAPS_HEADPHONES: u16 = 0x0010;
/// `AC97_CAPS_LOUDNESS`.
pub const AC97_CAPS_LOUDNESS: u16 = 0x0020;
/// `AC97_CAPS_DAC18`.
pub const AC97_CAPS_DAC18: u16 = 0x0040;
/// `AC97_CAPS_DAC20`.
pub const AC97_CAPS_DAC20: u16 = 0x0080;
/// `AC97_CAPS_ADC18`.
pub const AC97_CAPS_ADC18: u16 = 0x0100;
/// `AC97_CAPS_ADC20`.
pub const AC97_CAPS_ADC20: u16 = 0x0200;
/// `AC97_CAPS_ENHANCEMENT_MASK`.
pub const AC97_CAPS_ENHANCEMENT_MASK: u16 = 0xfc00;
/// `AC97_CAPS_ENHANCEMENT_SHIFT`.
pub const AC97_CAPS_ENHANCEMENT_SHIFT: u32 = 10;

/// `AC97_CAPS_ENHANCEMENT(reg)`: the 3D enhancement code of the reset register.
pub const fn ac97_caps_enhancement(reg: u16) -> u16 {
    (reg >> 10) & 0x1f
}

/// `AC97_REG_MASTER_VOLUME`.
pub const AC97_REG_MASTER_VOLUME: u8 = 0x02;
/// `AC97_REG_HEADPHONE_VOLUME`.
pub const AC97_REG_HEADPHONE_VOLUME: u8 = 0x04;
/// `AC97_REG_MASTER_VOLUME_MONO`.
pub const AC97_REG_MASTER_VOLUME_MONO: u8 = 0x06;
/// `AC97_REG_MASTER_TONE`.
pub const AC97_REG_MASTER_TONE: u8 = 0x08;
/// `AC97_REG_PCBEEP_VOLUME`.
pub const AC97_REG_PCBEEP_VOLUME: u8 = 0x0a;
/// `AC97_REG_PHONE_VOLUME`.
pub const AC97_REG_PHONE_VOLUME: u8 = 0x0c;
/// `AC97_REG_MIC_VOLUME`.
pub const AC97_REG_MIC_VOLUME: u8 = 0x0e;
/// `AC97_REG_LINEIN_VOLUME`.
pub const AC97_REG_LINEIN_VOLUME: u8 = 0x10;
/// `AC97_REG_CD_VOLUME`.
pub const AC97_REG_CD_VOLUME: u8 = 0x12;
/// `AC97_REG_VIDEO_VOLUME`.
pub const AC97_REG_VIDEO_VOLUME: u8 = 0x14;
/// `AC97_REG_AUX_VOLUME`.
pub const AC97_REG_AUX_VOLUME: u8 = 0x16;
/// `AC97_REG_PCMOUT_VOLUME`.
pub const AC97_REG_PCMOUT_VOLUME: u8 = 0x18;
/// `AC97_REG_RECORD_SELECT`.
pub const AC97_REG_RECORD_SELECT: u8 = 0x1a;
/// `AC97_REG_RECORD_GAIN`.
pub const AC97_REG_RECORD_GAIN: u8 = 0x1c;
/// `AC97_REG_RECORD_GAIN_MIC`.
pub const AC97_REG_RECORD_GAIN_MIC: u8 = 0x1e;
/// `AC97_REG_GP`.
pub const AC97_REG_GP: u8 = 0x20;
/// `AC97_REG_3D_CONTROL`.
pub const AC97_REG_3D_CONTROL: u8 = 0x22;
/// `AC97_REG_MODEM_SAMPLE_RATE`.
pub const AC97_REG_MODEM_SAMPLE_RATE: u8 = 0x24;
/// `AC97_REG_POWER`.
pub const AC97_REG_POWER: u8 = 0x26;
/// `AC97_POWER_ADC`.
pub const AC97_POWER_ADC: u16 = 0x0001;
/// `AC97_POWER_DAC`.
pub const AC97_POWER_DAC: u16 = 0x0002;
/// `AC97_POWER_ANL`.
pub const AC97_POWER_ANL: u16 = 0x0004;
/// `AC97_POWER_REF`.
pub const AC97_POWER_REF: u16 = 0x0008;
/// `AC97_POWER_IN`.
pub const AC97_POWER_IN: u16 = 0x0100;
/// `AC97_POWER_OUT`.
pub const AC97_POWER_OUT: u16 = 0x0200;
/// `AC97_POWER_MIXER`.
pub const AC97_POWER_MIXER: u16 = 0x0400;
/// `AC97_POWER_MIXER_VREF`.
pub const AC97_POWER_MIXER_VREF: u16 = 0x0800;
/// `AC97_POWER_ACLINK`.
pub const AC97_POWER_ACLINK: u16 = 0x1000;
/// `AC97_POWER_CLK`.
pub const AC97_POWER_CLK: u16 = 0x2000;
/// `AC97_POWER_AUX`.
pub const AC97_POWER_AUX: u16 = 0x4000;
/// `AC97_POWER_EAMP`.
pub const AC97_POWER_EAMP: u16 = 0x8000;
// Extended Audio Register Set
/// `AC97_REG_EXT_AUDIO_ID`.
pub const AC97_REG_EXT_AUDIO_ID: u8 = 0x28;
/// `AC97_REG_EXT_AUDIO_CTRL`.
pub const AC97_REG_EXT_AUDIO_CTRL: u8 = 0x2a;
/// `AC97_EXT_AUDIO_VRA`.
pub const AC97_EXT_AUDIO_VRA: u16 = 0x0001;
/// `AC97_EXT_AUDIO_DRA`.
pub const AC97_EXT_AUDIO_DRA: u16 = 0x0002;
/// `AC97_EXT_AUDIO_SPDIF`.
pub const AC97_EXT_AUDIO_SPDIF: u16 = 0x0004;
/// `AC97_EXT_AUDIO_VRM`.
pub const AC97_EXT_AUDIO_VRM: u16 = 0x0008;
/// `AC97_EXT_AUDIO_DSA_MASK`.
pub const AC97_EXT_AUDIO_DSA_MASK: u16 = 0x0030;
/// `AC97_EXT_AUDIO_DSA00`.
pub const AC97_EXT_AUDIO_DSA00: u16 = 0x0000;
/// `AC97_EXT_AUDIO_DSA01`.
pub const AC97_EXT_AUDIO_DSA01: u16 = 0x0010;
/// `AC97_EXT_AUDIO_DSA10`.
pub const AC97_EXT_AUDIO_DSA10: u16 = 0x0020;
/// `AC97_EXT_AUDIO_DSA11`.
pub const AC97_EXT_AUDIO_DSA11: u16 = 0x0030;
/// `AC97_EXT_AUDIO_SPSA_MASK`.
pub const AC97_EXT_AUDIO_SPSA_MASK: u16 = 0x0030;
/// `AC97_EXT_AUDIO_SPSA34`.
pub const AC97_EXT_AUDIO_SPSA34: u16 = 0x0000;
/// `AC97_EXT_AUDIO_SPSA78`.
pub const AC97_EXT_AUDIO_SPSA78: u16 = 0x0010;
/// `AC97_EXT_AUDIO_SPSA69`.
pub const AC97_EXT_AUDIO_SPSA69: u16 = 0x0020;
/// `AC97_EXT_AUDIO_SPSAAB`.
pub const AC97_EXT_AUDIO_SPSAAB: u16 = 0x0030;
/// `AC97_EXT_AUDIO_CDAC`.
pub const AC97_EXT_AUDIO_CDAC: u16 = 0x0040;
/// `AC97_EXT_AUDIO_SDAC`.
pub const AC97_EXT_AUDIO_SDAC: u16 = 0x0080;
/// `AC97_EXT_AUDIO_LDAC`.
pub const AC97_EXT_AUDIO_LDAC: u16 = 0x0100;
/// `AC97_EXT_AUDIO_AMAP`.
pub const AC97_EXT_AUDIO_AMAP: u16 = 0x0200;
/// `AC97_EXT_AUDIO_SPCV`.
pub const AC97_EXT_AUDIO_SPCV: u16 = 0x0400;
/// `AC97_EXT_AUDIO_REV_11`.
pub const AC97_EXT_AUDIO_REV_11: u16 = 0x0000;
/// `AC97_EXT_AUDIO_REV_22`.
pub const AC97_EXT_AUDIO_REV_22: u16 = 0x0400;
/// `AC97_EXT_AUDIO_REV_23`.
pub const AC97_EXT_AUDIO_REV_23: u16 = 0x0800;
/// `AC97_EXT_AUDIO_REV_MASK`.
pub const AC97_EXT_AUDIO_REV_MASK: u16 = 0x0c00;
/// `AC97_EXT_AUDIO_ID`.
pub const AC97_EXT_AUDIO_ID: u16 = 0xc000;
/// `AC97_EXT_AUDIO_BITS`: the `%b` description of the extended audio ID register.
pub const AC97_EXT_AUDIO_BITS: &[u8] = b"\x10\x01vra\x02dra\x03spdif\x04vrm\x05dsa0\x06dsa1\x07cdac\x08sdac\x09ldac\x0aamap\x0brev0\x0crev1\x0fid0\x10id1";
/// `AC97_SINGLE_RATE`.
pub const AC97_SINGLE_RATE: u32 = 48000;
/// `AC97_REG_PCM_FRONT_DAC_RATE`.
pub const AC97_REG_PCM_FRONT_DAC_RATE: u8 = 0x2c;
/// `AC97_REG_PCM_SURR_DAC_RATE`.
pub const AC97_REG_PCM_SURR_DAC_RATE: u8 = 0x2e;
/// `AC97_REG_PCM_LFE_DAC_RATE`.
pub const AC97_REG_PCM_LFE_DAC_RATE: u8 = 0x30;
/// `AC97_REG_PCM_LR_ADC_RATE`.
pub const AC97_REG_PCM_LR_ADC_RATE: u8 = 0x32;
/// `AC97_REG_PCM_MIC_ADC_RATE`.
pub const AC97_REG_PCM_MIC_ADC_RATE: u8 = 0x34;
/// `AC97_REG_CENTER_LFE_MASTER`.
pub const AC97_REG_CENTER_LFE_MASTER: u8 = 0x36;
/// `AC97_REG_SURR_MASTER`.
pub const AC97_REG_SURR_MASTER: u8 = 0x38;
/// `AC97_REG_SPDIF_CTRL`.
pub const AC97_REG_SPDIF_CTRL: u8 = 0x3a;
/// `AC97_REG_SPDIF_CTRL_BITS`: the `%b` description of the S/PDIF control register.
pub const AC97_REG_SPDIF_CTRL_BITS: &[u8] =
    b"\x02\x01pro\x02/audio\x03copy\x04pre\x0cl\x0fdrs\x10valid";
/// `AC97_SPDIF_V`.
pub const AC97_SPDIF_V: u16 = 0x8000;
/// `AC97_SPDIF_DRS`.
pub const AC97_SPDIF_DRS: u16 = 0x4000;
/// `AC97_SPDIF_SPSR_MASK`.
pub const AC97_SPDIF_SPSR_MASK: u16 = 0x3000;
/// `AC97_SPDIF_SPSR_44K`.
pub const AC97_SPDIF_SPSR_44K: u16 = 0x0000;
/// `AC97_SPDIF_SPSR_48K`.
pub const AC97_SPDIF_SPSR_48K: u16 = 0x2000;
/// `AC97_SPDIF_SPSR_32K`.
pub const AC97_SPDIF_SPSR_32K: u16 = 0x1000;
/// `AC97_SPDIF_L`.
pub const AC97_SPDIF_L: u16 = 0x0800;
/// `AC97_SPDIF_CC_MASK`.
pub const AC97_SPDIF_CC_MASK: u16 = 0x07f0;
/// `AC97_SPDIF_PRE`.
pub const AC97_SPDIF_PRE: u16 = 0x0008;
/// `AC97_SPDIF_COPY`.
pub const AC97_SPDIF_COPY: u16 = 0x0004;
/// `AC97_SPDIF_NOAUDIO`.
pub const AC97_SPDIF_NOAUDIO: u16 = 0x0002;
/// `AC97_SPDIF_PRO`.
pub const AC97_SPDIF_PRO: u16 = 0x0001;

/// `AC97_REG_VENDOR_ID1`.
pub const AC97_REG_VENDOR_ID1: u8 = 0x7c;
/// `AC97_REG_VENDOR_ID2`.
pub const AC97_REG_VENDOR_ID2: u8 = 0x7e;
/// `AC97_VENDOR_ID_MASK`.
pub const AC97_VENDOR_ID_MASK: u32 = 0xffff_ff00;

// Analog Devices codec specific data
/// `AC97_AD_REG_MISC`.
pub const AC97_AD_REG_MISC: u8 = 0x76;
/// `AC97_AD_MISC_MBG0`.
pub const AC97_AD_MISC_MBG0: u16 = 0x0001;
/// `AC97_AD_MISC_MBG1`.
pub const AC97_AD_MISC_MBG1: u16 = 0x0002;
/// `AC97_AD_MISC_VREFD`.
pub const AC97_AD_MISC_VREFD: u16 = 0x0004;
/// `AC97_AD_MISC_VREFH`.
pub const AC97_AD_MISC_VREFH: u16 = 0x0008;
/// `AC97_AD_MISC_SRU`.
pub const AC97_AD_MISC_SRU: u16 = 0x0010;
/// `AC97_AD_MISC_LOSEL`.
pub const AC97_AD_MISC_LOSEL: u16 = 0x0020;
/// `AC97_AD_MISC_2CMIC`.
pub const AC97_AD_MISC_2CMIC: u16 = 0x0040;
/// `AC97_AD_MISC_SPRD`.
pub const AC97_AD_MISC_SPRD: u16 = 0x0080;
/// `AC97_AD_MISC_DMIX0`.
pub const AC97_AD_MISC_DMIX0: u16 = 0x0100;
/// `AC97_AD_MISC_DMIX1`.
pub const AC97_AD_MISC_DMIX1: u16 = 0x0200;
/// `AC97_AD_MISC_HPSEL`.
pub const AC97_AD_MISC_HPSEL: u16 = 0x0400;
/// `AC97_AD_MISC_CLDIS`.
pub const AC97_AD_MISC_CLDIS: u16 = 0x0800;
/// `AC97_AD_MISC_LODIS`.
pub const AC97_AD_MISC_LODIS: u16 = 0x1000;
/// `AC97_AD_MISC_MSPLT`.
pub const AC97_AD_MISC_MSPLT: u16 = 0x2000;
/// `AC97_AD_MISC_AC97NC`.
pub const AC97_AD_MISC_AC97NC: u16 = 0x4000;
/// `AC97_AD_MISC_DACZ`.
pub const AC97_AD_MISC_DACZ: u16 = 0x8000;

// Avance Logic codec specific data
/// `AC97_ALC650_REG_MULTI_CHANNEL_CONTROL`.
pub const AC97_ALC650_REG_MULTI_CHANNEL_CONTROL: u8 = 0x6a;
/// `AC97_ALC650_MCC_SLOT_MODIFY_MASK`.
pub const AC97_ALC650_MCC_SLOT_MODIFY_MASK: u16 = 0xc000;
/// `AC97_ALC650_MCC_FRONTDAC_FROM_SPDIFIN`.
pub const AC97_ALC650_MCC_FRONTDAC_FROM_SPDIFIN: u16 = 0x2000;
/// `AC97_ALC650_MCC_SPDIFOUT_FROM_ADC`.
pub const AC97_ALC650_MCC_SPDIFOUT_FROM_ADC: u16 = 0x1000;
/// `AC97_ALC650_MCC_PCM_FROM_SPDIFIN`.
pub const AC97_ALC650_MCC_PCM_FROM_SPDIFIN: u16 = 0x0800;
/// `AC97_ALC650_MCC_MIC_OR_CENTERLFE`.
pub const AC97_ALC650_MCC_MIC_OR_CENTERLFE: u16 = 0x0400;
/// `AC97_ALC650_MCC_LINEIN_OR_SURROUND`.
pub const AC97_ALC650_MCC_LINEIN_OR_SURROUND: u16 = 0x0200;
/// `AC97_ALC650_MCC_INDEPENDENT_MASTER_L`.
pub const AC97_ALC650_MCC_INDEPENDENT_MASTER_L: u16 = 0x0080;
/// `AC97_ALC650_MCC_INDEPENDENT_MASTER_R`.
pub const AC97_ALC650_MCC_INDEPENDENT_MASTER_R: u16 = 0x0040;
/// `AC97_ALC650_MCC_ANALOG_TO_CENTERLFE`.
pub const AC97_ALC650_MCC_ANALOG_TO_CENTERLFE: u16 = 0x0020;
/// `AC97_ALC650_MCC_ANALOG_TO_SURROUND`.
pub const AC97_ALC650_MCC_ANALOG_TO_SURROUND: u16 = 0x0010;
/// `AC97_ALC650_MCC_EXCHANGE_CENTERLFE`.
pub const AC97_ALC650_MCC_EXCHANGE_CENTERLFE: u16 = 0x0008;
/// `AC97_ALC650_MCC_CENTERLFE_DOWNMIX`.
pub const AC97_ALC650_MCC_CENTERLFE_DOWNMIX: u16 = 0x0004;
/// `AC97_ALC650_MCC_SURROUND_DOWNMIX`.
pub const AC97_ALC650_MCC_SURROUND_DOWNMIX: u16 = 0x0002;
/// `AC97_ALC650_MCC_LINEOUT_TO_SURROUND`.
pub const AC97_ALC650_MCC_LINEOUT_TO_SURROUND: u16 = 0x0001;
/// `AC97_ALC650_REG_MISC`.
pub const AC97_ALC650_REG_MISC: u8 = 0x7a;
/// `AC97_ALC650_MISC_PIN47`.
pub const AC97_ALC650_MISC_PIN47: u16 = 0x0002;
/// `AC97_ALC650_MISC_VREFDIS`.
pub const AC97_ALC650_MISC_VREFDIS: u16 = 0x1000;

// Conexant codec specific data
/// `AC97_CX_REG_MISC`.
pub const AC97_CX_REG_MISC: u8 = 0x5c;
/// `AC97_CX_PCM`.
pub const AC97_CX_PCM: u16 = 0x00;
/// `AC97_CX_AC3`.
pub const AC97_CX_AC3: u16 = 0x02;
/// `AC97_CX_MASK`.
pub const AC97_CX_MASK: u16 = 0x03;
/// `AC97_CX_COPYRIGHT`.
pub const AC97_CX_COPYRIGHT: u16 = 0x04;
/// `AC97_CX_SPDIFEN`.
pub const AC97_CX_SPDIFEN: u16 = 0x08;

// VIA codec specific data
/// `AC97_VT_REG_TEST`.
pub const AC97_VT_REG_TEST: u8 = 0x5a;
/// `AC97_VT_LVL`: hp controls rear.
pub const AC97_VT_LVL: u16 = 0x8000;
/// `AC97_VT_LCTF`: lfe/center to front downmix.
pub const AC97_VT_LCTF: u16 = 0x1000;
/// `AC97_VT_STF`: surround to front downmix.
pub const AC97_VT_STF: u16 = 0x0800;
/// `AC97_VT_BPDC`: enable DC-offset cancellation.
pub const AC97_VT_BPDC: u16 = 0x0400;
/// `AC97_VT_DC`: DC offset cancellation capable.
pub const AC97_VT_DC: u16 = 0x0200;

/// `AC97_IS_FIXED_RATE(codec)`: the codec has no variable rate audio.
pub fn ac97_is_fixed_rate(codec: &Ac97CodecIf) -> bool {
    (codec.vtbl.get_caps)(codec) & AC97_EXT_AUDIO_VRA == 0
}

/// `AC97_BITS_6CH`.
pub const AC97_BITS_6CH: u16 = AC97_EXT_AUDIO_SDAC | AC97_EXT_AUDIO_CDAC | AC97_EXT_AUDIO_LDAC;

// The mixer's controls

/// `AudioNspdif`.
const AUDIO_NSPDIF: &[u8] = b"spdif";

/// An `audio_mixer_name_t` with `s` as the name.
const fn mname(s: &[u8]) -> AudioMixerName {
    let mut name = [0u8; 16];
    let mut i = 0;
    while i < s.len() {
        name[i] = s[i];
        i += 1;
    }
    AudioMixerName { name, msg_id: 0 }
}

/// A `struct audio_mixer_enum` with the `members` (name, ordinal), the rest zero.
const fn menum<const N: usize>(members: [(&[u8], i32); N]) -> AudioMixerEnum {
    let mut e = AudioMixerEnum {
        num_mem: N as i32,
        member: [AudioMixerEnumMember {
            label: mname(b""),
            ord: 0,
        }; 32],
    };
    let mut i = 0;
    while i < N {
        e.member[i] = AudioMixerEnumMember {
            label: mname(members[i].0),
            ord: members[i].1,
        };
        i += 1;
    }
    e
}

/// `ac97_on_off`.
pub static AC97_ON_OFF: AudioMixerEnum = menum([(AudioNoff, 0), (AudioNon, 1)]);

/// `ac97_mic_select`: the names are `AudioNmicrophone "0"` and `AudioNmicrophone "1"`.
pub static AC97_MIC_SELECT: AudioMixerEnum = menum([(b"mic0", 0), (b"mic1", 1)]);

/// `ac97_mono_select`.
pub static AC97_MONO_SELECT: AudioMixerEnum = menum([(AudioNmixerout, 0), (AudioNmicrophone, 1)]);

/// `ac97_source`: `AudioNmixerout AudioNmono` is `mixeroutmono`.
pub static AC97_SOURCE: AudioMixerEnum = menum([
    (AudioNmicrophone, 0),
    (AudioNcd, 1),
    (b"video", 2),
    (AudioNaux, 3),
    (AudioNline, 4),
    (AudioNmixerout, 5),
    (b"mixeroutmono", 6),
    (b"phone", 7),
]);

/// Due to different values for each source that uses these structures, `ac97_query_devinfo`
/// sets delta in `mixer_devinfo_t` using `ac97_source_info.bits`.
pub static AC97_VOLUME_STEREO: AudioMixerValue = AudioMixerValue {
    units: mname(AudioNvolume),
    num_channels: 2,
    delta: 0,
};

/// `ac97_volume_mono`.
pub static AC97_VOLUME_MONO: AudioMixerValue = AudioMixerValue {
    units: mname(AudioNvolume),
    num_channels: 1,
    delta: 0,
};

/// `CHECK_NONE`: a value of `ac97_source_info.req_feature`.
pub const CHECK_NONE: u8 = 0;
/// `CHECK_SURROUND`.
pub const CHECK_SURROUND: u8 = 1;
/// `CHECK_CENTER`.
pub const CHECK_CENTER: u8 = 2;
/// `CHECK_LFE`.
pub const CHECK_LFE: u8 = 3;
/// `CHECK_HEADPHONES`.
pub const CHECK_HEADPHONES: u8 = 4;
/// `CHECK_TONE`.
pub const CHECK_TONE: u8 = 5;
/// `CHECK_MIC`.
pub const CHECK_MIC: u8 = 6;
/// `CHECK_LOUDNESS`.
pub const CHECK_LOUDNESS: u8 = 7;
/// `CHECK_3D`.
pub const CHECK_3D: u8 = 8;
/// `CHECK_SPDIF`.
pub const CHECK_SPDIF: u8 = 9;

/// `struct ac97_source_info`: one mixer control of the codec.
#[derive(Clone, Copy, Debug)]
pub struct Ac97SourceInfo {
    /// `class`.
    pub class: Option<&'static [u8]>,
    /// `device`.
    pub device: Option<&'static [u8]>,
    /// `qualifier`.
    pub qualifier: Option<&'static [u8]>,
    /// `type`: `AUDIO_MIXER_*`.
    pub type_: i32,
    /// `info` of an `AUDIO_MIXER_ENUM` control.
    pub info_enum: Option<&'static AudioMixerEnum>,
    /// `info` of an `AUDIO_MIXER_VALUE` control.
    pub info_value: Option<&'static AudioMixerValue>,
    /// `reg`.
    pub reg: u8,
    /// `default_value`.
    pub default_value: u16,
    /// `bits:3`.
    pub bits: u8,
    /// `ofs:4`.
    pub ofs: u8,
    /// `mute:1`.
    pub mute: bool,
    /// `polarity:1`: does 0 == MAX or MIN.
    pub polarity: bool,
    /// `req_feature`: a `CHECK_*`.
    pub req_feature: u8,
    /// `prev`.
    pub prev: i16,
    /// `next`.
    pub next: i16,
    /// `mixer_class`.
    pub mixer_class: i16,
}

impl Ac97SourceInfo {
    /// All zero.
    pub const ZERO: Self = Self {
        class: None,
        device: None,
        qualifier: None,
        type_: 0,
        info_enum: None,
        info_value: None,
        reg: 0,
        default_value: 0,
        bits: 0,
        ofs: 0,
        mute: false,
        polarity: false,
        req_feature: CHECK_NONE,
        prev: 0,
        next: 0,
        mixer_class: 0,
    };
}

/// A control's register field: `(reg, default_value, bits, ofs, mute, polarity,
/// req_feature)`.
type Ctl = (u8, u16, u8, u8, bool, bool, u8);

/// A class entry of `source_info[]`.
const fn class_ent(class: &'static [u8]) -> Ac97SourceInfo {
    Ac97SourceInfo {
        class: Some(class),
        type_: AUDIO_MIXER_CLASS,
        ..Ac97SourceInfo::ZERO
    }
}

/// An `AUDIO_MIXER_VALUE` entry of `source_info[]`.
const fn value_ent(
    class: &'static [u8],
    device: &'static [u8],
    qualifier: Option<&'static [u8]>,
    info: &'static AudioMixerValue,
    ctl: Ctl,
) -> Ac97SourceInfo {
    Ac97SourceInfo {
        class: Some(class),
        device: Some(device),
        qualifier,
        type_: AUDIO_MIXER_VALUE,
        info_value: Some(info),
        reg: ctl.0,
        default_value: ctl.1,
        bits: ctl.2,
        ofs: ctl.3,
        mute: ctl.4,
        polarity: ctl.5,
        req_feature: ctl.6,
        ..Ac97SourceInfo::ZERO
    }
}

/// An `AUDIO_MIXER_ENUM` entry of `source_info[]`.
const fn enum_ent(
    class: &'static [u8],
    device: &'static [u8],
    qualifier: Option<&'static [u8]>,
    info: &'static AudioMixerEnum,
    ctl: Ctl,
) -> Ac97SourceInfo {
    Ac97SourceInfo {
        class: Some(class),
        device: Some(device),
        qualifier,
        type_: AUDIO_MIXER_ENUM,
        info_enum: Some(info),
        reg: ctl.0,
        default_value: ctl.1,
        bits: ctl.2,
        ofs: ctl.3,
        mute: ctl.4,
        polarity: ctl.5,
        req_feature: ctl.6,
        ..Ac97SourceInfo::ZERO
    }
}

/// `nitems(source_info)`.
pub const NSOURCE_INFO: usize = 32;

/// `source_info[]`: the mixer controls a codec can have, with the capability each needs.
pub static SOURCE_INFO: [Ac97SourceInfo; NSOURCE_INFO] = [
    class_ent(AudioCinputs),
    class_ent(AudioCoutputs),
    class_ent(AudioCrecord),
    // Stereo master volume
    value_ent(
        AudioCoutputs,
        AudioNmaster,
        None,
        &AC97_VOLUME_STEREO,
        (
            AC97_REG_MASTER_VOLUME,
            0x8000,
            5,
            0,
            true,
            false,
            CHECK_NONE,
        ),
    ),
    // Mono volume
    value_ent(
        AudioCoutputs,
        AudioNmono,
        None,
        &AC97_VOLUME_MONO,
        (
            AC97_REG_MASTER_VOLUME_MONO,
            0x8000,
            6,
            0,
            true,
            false,
            CHECK_NONE,
        ),
    ),
    enum_ent(
        AudioCoutputs,
        AudioNmono,
        Some(AudioNsource),
        &AC97_MONO_SELECT,
        (AC97_REG_GP, 0x0000, 1, 9, false, false, CHECK_NONE),
    ),
    // Headphone volume
    value_ent(
        AudioCoutputs,
        AudioNheadphone,
        None,
        &AC97_VOLUME_STEREO,
        (
            AC97_REG_HEADPHONE_VOLUME,
            0x8000,
            6,
            0,
            true,
            false,
            CHECK_HEADPHONES,
        ),
    ),
    // Surround volume - logic hard coded for mute
    value_ent(
        AudioCoutputs,
        AudioNsurround,
        None,
        &AC97_VOLUME_STEREO,
        (
            AC97_REG_SURR_MASTER,
            0x8080,
            5,
            0,
            true,
            false,
            CHECK_SURROUND,
        ),
    ),
    // Center volume
    value_ent(
        AudioCoutputs,
        AudioNcenter,
        None,
        &AC97_VOLUME_MONO,
        (
            AC97_REG_CENTER_LFE_MASTER,
            0x8080,
            5,
            0,
            false,
            false,
            CHECK_CENTER,
        ),
    ),
    enum_ent(
        AudioCoutputs,
        AudioNcenter,
        Some(AudioNmute),
        &AC97_ON_OFF,
        (
            AC97_REG_CENTER_LFE_MASTER,
            0x8080,
            1,
            7,
            false,
            false,
            CHECK_CENTER,
        ),
    ),
    // LFE volume
    value_ent(
        AudioCoutputs,
        AudioNlfe,
        None,
        &AC97_VOLUME_MONO,
        (
            AC97_REG_CENTER_LFE_MASTER,
            0x8080,
            5,
            8,
            false,
            false,
            CHECK_LFE,
        ),
    ),
    enum_ent(
        AudioCoutputs,
        AudioNlfe,
        Some(AudioNmute),
        &AC97_ON_OFF,
        (
            AC97_REG_CENTER_LFE_MASTER,
            0x8080,
            1,
            15,
            false,
            false,
            CHECK_LFE,
        ),
    ),
    // Tone
    value_ent(
        AudioCoutputs,
        b"tone",
        None,
        &AC97_VOLUME_STEREO,
        (AC97_REG_MASTER_TONE, 0x0f0f, 4, 0, false, false, CHECK_TONE),
    ),
    // PC Beep Volume
    value_ent(
        AudioCinputs,
        AudioNspeaker,
        None,
        &AC97_VOLUME_MONO,
        (
            AC97_REG_PCBEEP_VOLUME,
            0x0000,
            4,
            1,
            true,
            false,
            CHECK_NONE,
        ),
    ),
    // Phone
    value_ent(
        AudioCinputs,
        b"phone",
        None,
        &AC97_VOLUME_MONO,
        (AC97_REG_PHONE_VOLUME, 0x8008, 5, 0, true, false, CHECK_NONE),
    ),
    // Mic Volume
    value_ent(
        AudioCinputs,
        AudioNmicrophone,
        None,
        &AC97_VOLUME_MONO,
        (AC97_REG_MIC_VOLUME, 0x8008, 5, 0, true, false, CHECK_NONE),
    ),
    enum_ent(
        AudioCinputs,
        AudioNmicrophone,
        Some(AudioNpreamp),
        &AC97_ON_OFF,
        (AC97_REG_MIC_VOLUME, 0x8008, 1, 6, false, false, CHECK_NONE),
    ),
    enum_ent(
        AudioCinputs,
        AudioNmicrophone,
        Some(AudioNsource),
        &AC97_MIC_SELECT,
        (AC97_REG_GP, 0x0000, 1, 8, false, false, CHECK_NONE),
    ),
    // Line in Volume
    value_ent(
        AudioCinputs,
        AudioNline,
        None,
        &AC97_VOLUME_STEREO,
        (
            AC97_REG_LINEIN_VOLUME,
            0x8808,
            5,
            0,
            true,
            false,
            CHECK_NONE,
        ),
    ),
    // CD Volume
    value_ent(
        AudioCinputs,
        AudioNcd,
        None,
        &AC97_VOLUME_STEREO,
        (AC97_REG_CD_VOLUME, 0x8808, 5, 0, true, false, CHECK_NONE),
    ),
    // Video Volume
    value_ent(
        AudioCinputs,
        AudioNvideo,
        None,
        &AC97_VOLUME_STEREO,
        (AC97_REG_VIDEO_VOLUME, 0x8808, 5, 0, true, false, CHECK_NONE),
    ),
    // AUX volume
    value_ent(
        AudioCinputs,
        AudioNaux,
        None,
        &AC97_VOLUME_STEREO,
        (AC97_REG_AUX_VOLUME, 0x8808, 5, 0, true, false, CHECK_NONE),
    ),
    // PCM out volume
    value_ent(
        AudioCinputs,
        AudioNdac,
        None,
        &AC97_VOLUME_STEREO,
        (
            AC97_REG_PCMOUT_VOLUME,
            0x8808,
            5,
            0,
            true,
            false,
            CHECK_NONE,
        ),
    ),
    // Record Source - some logic for this is hard coded - see below
    enum_ent(
        AudioCrecord,
        AudioNsource,
        None,
        &AC97_SOURCE,
        (
            AC97_REG_RECORD_SELECT,
            0x0000,
            3,
            0,
            false,
            false,
            CHECK_NONE,
        ),
    ),
    // Record Gain
    value_ent(
        AudioCrecord,
        AudioNvolume,
        None,
        &AC97_VOLUME_STEREO,
        (AC97_REG_RECORD_GAIN, 0x8000, 4, 0, true, false, CHECK_NONE),
    ),
    // Record Gain mic
    value_ent(
        AudioCrecord,
        AudioNmicrophone,
        None,
        &AC97_VOLUME_MONO,
        (
            AC97_REG_RECORD_GAIN_MIC,
            0x8000,
            4,
            0,
            true,
            true,
            CHECK_MIC,
        ),
    ),
    //
    enum_ent(
        AudioCoutputs,
        AudioNloudness,
        None,
        &AC97_ON_OFF,
        (AC97_REG_GP, 0x0000, 1, 12, false, false, CHECK_LOUDNESS),
    ),
    enum_ent(
        AudioCoutputs,
        AudioNspatial,
        None,
        &AC97_ON_OFF,
        (AC97_REG_GP, 0x0000, 1, 13, false, true, CHECK_3D),
    ),
    value_ent(
        AudioCoutputs,
        AudioNspatial,
        Some(b"center"),
        &AC97_VOLUME_MONO,
        (AC97_REG_3D_CONTROL, 0x0000, 4, 8, false, true, CHECK_3D),
    ),
    value_ent(
        AudioCoutputs,
        AudioNspatial,
        Some(b"depth"),
        &AC97_VOLUME_MONO,
        (AC97_REG_3D_CONTROL, 0x0000, 4, 0, false, true, CHECK_3D),
    ),
    // External Amp
    enum_ent(
        AudioCoutputs,
        AudioNextamp,
        None,
        &AC97_ON_OFF,
        (AC97_REG_POWER, 0x0000, 1, 15, false, false, CHECK_NONE),
    ),
    // S/PDIF output enable
    enum_ent(
        AudioCoutputs,
        AUDIO_NSPDIF,
        None,
        &AC97_ON_OFF,
        (
            AC97_REG_EXT_AUDIO_CTRL,
            0x0000,
            1,
            2,
            false,
            false,
            CHECK_SPDIF,
        ),
    ),
    // Missing features: Simulated Stereo, POP, Loopback mode
];

/// `MAX_SOURCES`: `2 * nitems(source_info)`.
pub const MAX_SOURCES: usize = 2 * NSOURCE_INFO;

/// `AC97_STANDARD_CLOCK`.
pub const AC97_STANDARD_CLOCK: u32 = 48000;

/// `struct ac97_softc`. Check out <http://www.intel.com/technology/computing/audio/index.htm>
/// for information on AC-97.
///
/// `codec_if` must be the first member: [`ac97_attach`] allocates this structure zeroed and
/// writes the interface in place, so every other member is valid as all-zero bits.
#[repr(C)]
pub struct Ac97Softc {
    /// `codec_if`.
    pub codec_if: Ac97CodecIf,
    /// `host_if`: a copy of the host's.
    host_if: Cell<Ac97HostIf>,
    /// `source_info[MAX_SOURCES]`.
    source_info: [Cell<Ac97SourceInfo>; MAX_SOURCES],
    /// `num_source_info`.
    num_source_info: Cell<usize>,
    /// `host_flags`: `AC97_HOST_*`.
    host_flags: Cell<u32>,
    /// `ac97_clock`: usually 48000.
    ac97_clock: Cell<u32>,
    /// `caps`: `AC97_REG_RESET`.
    caps: Cell<u16>,
    /// `ext_id`: `AC97_REG_EXT_AUDIO_ID`.
    ext_id: Cell<u16>,
    /// `shadow_reg[128]`.
    shadow_reg: [Cell<u16>; 128],
    /// `lock_counter`.
    lock_counter: Cell<i32>,
}

/// `struct ac97_codecid`: a codec of a vendor, found by its ID byte.
pub struct Ac97CodecId {
    /// `id`.
    pub id: u8,
    /// `mask`.
    pub mask: u8,
    /// `rev`: the mask of the revision in the ID, 0 for none.
    pub rev: u8,
    /// `shift`: no use yet.
    pub shift: u8,
    /// `name`.
    pub name: &'static str,
    /// `init`: codec specific initialisation.
    pub init: Option<Ac97InitFn>,
}

/// A codec of the table without an `init`.
const fn cid(id: u8, mask: u8, rev: u8, name: &'static str) -> Ac97CodecId {
    Ac97CodecId {
        id,
        mask,
        rev,
        shift: 0,
        name,
        init: None,
    }
}

/// A codec of the table with an `init`.
const fn cidi(id: u8, mask: u8, rev: u8, name: &'static str, init: Ac97InitFn) -> Ac97CodecId {
    Ac97CodecId {
        id,
        mask,
        rev,
        shift: 0,
        name,
        init: Some(init),
    }
}

/// `ac97_ad`: Analog Devices.
static AC97_AD: [Ac97CodecId; 11] = [
    cid(0x03, 0xff, 0, "AD1819"),
    cid(0x40, 0xff, 0, "AD1881"),
    cid(0x48, 0xff, 0, "AD1881A"),
    cidi(0x60, 0xff, 0, "AD1885", ac97_ad1885_init),
    cidi(0x61, 0xff, 0, "AD1886", ac97_ad1886_init),
    cid(0x63, 0xff, 0, "AD1886A"),
    cidi(0x68, 0xff, 0, "AD1888", ac97_ad198x_init),
    cidi(0x70, 0xff, 0, "AD1980", ac97_ad198x_init),
    cid(0x72, 0xff, 0, "AD1981A"),
    cid(0x74, 0xff, 0, "AD1981B"),
    cidi(0x75, 0xff, 0, "AD1985", ac97_ad198x_init),
];
/// `ac97_ak`: Asahi Kasei.
static AC97_AK: [Ac97CodecId; 6] = [
    cid(0x00, 0xfe, 1, "AK4540"),
    cid(0x01, 0xfe, 1, "AK4540"),
    cid(0x02, 0xff, 0, "AK4543"),
    cid(0x05, 0xff, 0, "AK4544"),
    cid(0x06, 0xff, 0, "AK4544A"),
    cid(0x07, 0xff, 0, "AK4545"),
];
/// `ac97_av`: Avance Logic.
static AC97_AV: [Ac97CodecId; 13] = [
    cid(0x10, 0xff, 0, "ALC200"),
    cid(0x20, 0xff, 0, "ALC650"),
    cid(0x21, 0xff, 0, "ALC650D"),
    cid(0x22, 0xff, 0, "ALC650E"),
    cid(0x23, 0xff, 0, "ALC650F"),
    cid(0x30, 0xff, 0, "ALC101"),
    cid(0x40, 0xff, 0, "ALC202"),
    cid(0x50, 0xff, 0, "ALC250"),
    cid(0x52, 0xff, 0, "ALC250A?"),
    cidi(0x60, 0xf0, 0xf, "ALC655", ac97_alc650_init),
    cid(0x70, 0xf0, 0xf, "ALC203"),
    cidi(0x80, 0xf0, 0xf, "ALC658", ac97_alc650_init),
    cid(0x90, 0xf0, 0xf, "ALC850"),
];
/// `ac97_rl`: Realtek.
static AC97_RL: [Ac97CodecId; 3] = [
    cid(0x00, 0xf0, 0xf, "RL5306"),
    cid(0x10, 0xf0, 0xf, "RL5382"),
    cid(0x20, 0xf0, 0xf, "RL5383"),
];
/// `ac97_cm`: C-Media Electronics.
static AC97_CM: [Ac97CodecId; 5] = [
    cid(0x41, 0xff, 0, "CMI9738"),
    cid(0x61, 0xff, 0, "CMI9739"),
    cid(0x78, 0xff, 0, "CMI9761A"),
    cid(0x82, 0xff, 0, "CMI9761B"),
    cid(0x83, 0xff, 0, "CMI9761A+"),
];
/// `ac97_cr`: Creative.
static AC97_CR: [Ac97CodecId; 1] = [cid(0x84, 0xff, 0, "EV1938")];
/// `ac97_cs`: Cirrus Logic.
static AC97_CS: [Ac97CodecId; 9] = [
    cid(0x00, 0xf8, 7, "CS4297"),
    cid(0x10, 0xf8, 7, "CS4297A"),
    cid(0x20, 0xf8, 7, "CS4298"),
    cid(0x28, 0xf8, 7, "CS4294"),
    cid(0x30, 0xf8, 7, "CS4299"),
    cid(0x48, 0xf8, 7, "CS4201"),
    cid(0x58, 0xf8, 7, "CS4205"),
    cid(0x60, 0xf8, 7, "CS4291"),
    cid(0x70, 0xf8, 7, "CS4202"),
];
/// `ac97_cx`: Conexant.
static AC97_CX: [Ac97CodecId; 4] = [
    cid(0x21, 0xff, 0, "HSD11246"),
    cidi(0x28, 0xf8, 7, "CX20468", ac97_cx20468_init),
    cid(0x30, 0xff, 0, "CXT48"),
    cid(0x42, 0xff, 0, "CXT66"),
];
/// `ac97_dt`: Diamond Technology.
static AC97_DT: [Ac97CodecId; 1] = [cid(0x00, 0xff, 0, "DT0398")];
/// `ac97_em`: eMicro.
static AC97_EM: [Ac97CodecId; 2] = [cid(0x23, 0xff, 0, "EM28023"), cid(0x28, 0xff, 0, "EM28028")];
/// `ac97_es`: ESS Technology.
static AC97_ES: [Ac97CodecId; 1] = [cid(0x08, 0xff, 0, "ES1921")];
/// `ac97_is`: Intersil.
static AC97_IS: [Ac97CodecId; 1] = [cid(0x00, 0xff, 0, "HMP9701")];
/// `ac97_ic`: ICEnsemble.
static AC97_IC: [Ac97CodecId; 5] = [
    cid(0x01, 0xff, 0, "ICE1230"),
    cid(0x11, 0xff, 0, "ICE1232"),
    cid(0x14, 0xff, 0, "ICE1232A"),
    cid(0x51, 0xff, 0, "VIA VT1616"),
    cidi(0x52, 0xff, 0, "VIA VT1616i", ac97_vt1616_init),
];
/// `ac97_it`: ITE, Inc.
static AC97_IT: [Ac97CodecId; 2] = [
    cid(0x20, 0xff, 0, "ITE2226E"),
    cid(0x60, 0xff, 0, "ITE2646E"),
];
/// `ac97_ns`: National Semiconductor.
static AC97_NS: [Ac97CodecId; 8] = [
    cid(0x00, 0xff, 0, "LM454[03568]"),
    cid(0x31, 0xff, 0, "LM4549"),
    cid(0x40, 0xff, 0, "LM4540"),
    cid(0x43, 0xff, 0, "LM4543"),
    cid(0x46, 0xff, 0, "LM4546A"),
    cid(0x48, 0xff, 0, "LM4548A"),
    cid(0x49, 0xff, 0, "LM4549A"),
    cid(0x50, 0xff, 0, "LM4550"),
];
/// `ac97_ps`: Philips Semiconductor.
static AC97_PS: [Ac97CodecId; 2] = [cid(0x01, 0xff, 0, "UCB1510"), cid(0x04, 0xff, 0, "UCB1400")];
/// `ac97_sl`: Silicon Laboratory.
static AC97_SL: [Ac97CodecId; 1] = [cid(0x20, 0xe0, 0, "Si3036/38")];
/// `ac97_st`: SigmaTel.
static AC97_ST: [Ac97CodecId; 14] = [
    cid(0x00, 0xff, 0, "STAC9700"),
    cid(0x04, 0xff, 0, "STAC970[135]"),
    cid(0x05, 0xff, 0, "STAC9704"),
    cid(0x08, 0xff, 0, "STAC9708/11"),
    cid(0x09, 0xff, 0, "STAC9721/23"),
    cid(0x44, 0xff, 0, "STAC9744/45"),
    cid(0x50, 0xff, 0, "STAC9750/51"),
    cid(0x52, 0xff, 0, "STAC9752/53"),
    cid(0x56, 0xff, 0, "STAC9756/57"),
    cid(0x58, 0xff, 0, "STAC9758/59"),
    cid(0x60, 0xff, 0, "STAC9760/61"),
    cid(0x62, 0xff, 0, "STAC9762/63"),
    cid(0x66, 0xff, 0, "STAC9766/67"),
    cid(0x84, 0xff, 0, "STAC9784/85"),
];
/// `ac97_vi`: VIA Technologies.
static AC97_VI: [Ac97CodecId; 2] = [cid(0x61, 0xff, 0, "VT1612A"), cid(0x70, 0xff, 0, "VT1617")];
/// `ac97_tt`: TriTech Microelectronics.
static AC97_TT: [Ac97CodecId; 5] = [
    cid(0x02, 0xff, 0, "TR28022"),
    cid(0x03, 0xff, 0, "TR28023"),
    cid(0x06, 0xff, 0, "TR28026"),
    cid(0x08, 0xff, 0, "TR28028"),
    cid(0x23, 0xff, 0, "TR28602"),
];
/// `ac97_ti`: Texas Instruments.
static AC97_TI: [Ac97CodecId; 1] = [cid(0x20, 0xff, 0, "TLC320AD9xC")];
/// `ac97_wb`: Winbond.
static AC97_WB: [Ac97CodecId; 1] = [cid(0x01, 0xff, 0, "W83971D")];
/// `ac97_wo`: Wolfson.
static AC97_WO: [Ac97CodecId; 6] = [
    cid(0x00, 0xff, 0, "WM9701A"),
    cid(0x03, 0xff, 0, "WM9704M/Q-0"), // & WM9703
    cid(0x04, 0xff, 0, "WM9704M/Q-1"),
    cid(0x05, 0xff, 0, "WM9705/10"),
    cid(0x09, 0xff, 0, "WM9709"),
    cid(0x12, 0xff, 0, "WM9711/12"),
];
/// `ac97_ym`: Yamaha.
static AC97_YM: [Ac97CodecId; 3] = [
    cid(0x00, 0xff, 0, "YMF743-S"),
    cid(0x02, 0xff, 0, "YMF752-S"),
    cid(0x03, 0xff, 0, "YMF753-S"),
];

/// `struct ac97_vendorid`: a codec vendor, found by the top 24 bits of the ID.
pub struct Ac97VendorId {
    /// `id`.
    pub id: u32,
    /// `name`.
    pub name: &'static str,
    /// `codecs`, `num`.
    pub codecs: &'static [Ac97CodecId],
}

/// `ac97_vendors[]`.
pub static AC97_VENDORS: [Ac97VendorId; 24] = [
    Ac97VendorId {
        id: 0x0140_8300,
        name: "Creative",
        codecs: &AC97_CR,
    },
    Ac97VendorId {
        id: 0x4144_5300,
        name: "Analog Devices",
        codecs: &AC97_AD,
    },
    Ac97VendorId {
        id: 0x414b_4d00,
        name: "Asahi Kasei",
        codecs: &AC97_AK,
    },
    Ac97VendorId {
        id: 0x414c_4300,
        name: "Realtek",
        codecs: &AC97_RL,
    },
    Ac97VendorId {
        id: 0x414c_4700,
        name: "Avance Logic",
        codecs: &AC97_AV,
    },
    Ac97VendorId {
        id: 0x434d_4900,
        name: "C-Media Electronics",
        codecs: &AC97_CM,
    },
    Ac97VendorId {
        id: 0x4352_5900,
        name: "Cirrus Logic",
        codecs: &AC97_CS,
    },
    Ac97VendorId {
        id: 0x4358_5400,
        name: "Conexant",
        codecs: &AC97_CX,
    },
    Ac97VendorId {
        id: 0x4454_3000,
        name: "Diamond Technology",
        codecs: &AC97_DT,
    },
    Ac97VendorId {
        id: 0x454d_4300,
        name: "eMicro",
        codecs: &AC97_EM,
    },
    Ac97VendorId {
        id: 0x4583_8300,
        name: "ESS Technology",
        codecs: &AC97_ES,
    },
    Ac97VendorId {
        id: 0x4852_5300,
        name: "Intersil",
        codecs: &AC97_IS,
    },
    Ac97VendorId {
        id: 0x4943_4500,
        name: "ICEnsemble",
        codecs: &AC97_IC,
    },
    Ac97VendorId {
        id: 0x4954_4500,
        name: "ITE, Inc.",
        codecs: &AC97_IT,
    },
    Ac97VendorId {
        id: 0x4e53_4300,
        name: "National Semiconductor",
        codecs: &AC97_NS,
    },
    Ac97VendorId {
        id: 0x5053_4300,
        name: "Philips Semiconductor",
        codecs: &AC97_PS,
    },
    Ac97VendorId {
        id: 0x5349_4c00,
        name: "Silicon Laboratory",
        codecs: &AC97_SL,
    },
    Ac97VendorId {
        id: 0x5452_4100,
        name: "TriTech Microelectronics",
        codecs: &AC97_TT,
    },
    Ac97VendorId {
        id: 0x5458_4e00,
        name: "Texas Instruments",
        codecs: &AC97_TI,
    },
    Ac97VendorId {
        id: 0x5649_4100,
        name: "VIA Technologies",
        codecs: &AC97_VI,
    },
    Ac97VendorId {
        id: 0x5745_4300,
        name: "Winbond",
        codecs: &AC97_WB,
    },
    Ac97VendorId {
        id: 0x574d_4c00,
        name: "Wolfson",
        codecs: &AC97_WO,
    },
    Ac97VendorId {
        id: 0x594d_4800,
        name: "Yamaha",
        codecs: &AC97_YM,
    },
    Ac97VendorId {
        id: 0x8384_7600,
        name: "SigmaTel",
        codecs: &AC97_ST,
    },
];

/// `ac97enhancement[]`: the names of the 3D enhancement codes.
pub static AC97ENHANCEMENT: [&str; 32] = [
    "No 3D Stereo",
    "Analog Devices Phat Stereo",
    "Creative",
    "National Semi 3D",
    "Yamaha Ymersion",
    "BBE 3D",
    "Crystal Semi 3D",
    "Qsound QXpander",
    "Spatializer 3D",
    "SRS 3D",
    "Platform Tech 3D",
    "AKM 3D",
    "Aureal",
    "AZTECH 3D",
    "Binaura 3D",
    "ESS Technology",
    "Harman International VMAx",
    "Nvidea 3D",
    "Philips Incredible Sound",
    "Texas Instruments 3D",
    "VLSI Technology 3D",
    "TriTech 3D",
    "Realtek 3D",
    "Samsung 3D",
    "Wolfson Microelectronics 3D",
    "Delta Integration 3D",
    "SigmaTel 3D",
    "KS Waves 3D",
    "Rockwell 3D",
    "Unknown 3D",
    "Unknown 3D",
    "Unknown 3D",
];

/// `ac97feature[]`: the names of the codec capability bits 0 to 9.
pub static AC97FEATURE: [&str; 10] = [
    "mic channel",
    "reserved",
    "tone",
    "simulated stereo",
    "headphone",
    "bass boost",
    "18 bit DAC",
    "20 bit DAC",
    "18 bit ADC",
    "20 bit ADC",
];

/// The vendor and codec of a codec ID (`AC97_REG_VENDOR_ID1` and `ID2`), as `ac97_attach`
/// looks them up: the vendor by the ID with the low byte masked, then the codec by
/// `(id & mask) == codec.id`. Both searches go from the end of their table, like the C's
/// loops. `None` for an unknown vendor; the codec is `None` when the vendor does not know
/// the ID.
pub fn ac97_find_codec(id: u32) -> Option<(&'static Ac97VendorId, Option<&'static Ac97CodecId>)> {
    let vendor = AC97_VENDORS
        .iter()
        .rev()
        .find(|v| v.id == (id & AC97_VENDOR_ID_MASK))?;
    let codec = vendor
        .codecs
        .iter()
        .rev()
        .find(|c| u32::from(c.id) == (id & u32::from(c.mask)));
    Some((vendor, codec))
}

/// The `u_int16_t` that `v << n` is assigned to (computed in `int`, truncated).
const fn shl16(v: u16, n: u32) -> u16 {
    ((v as u32) << n) as u16
}

impl Ac97Softc {
    /// A copy of `as->source_info[i]`.
    fn src(&self, i: usize) -> Ac97SourceInfo {
        self.source_info[i].get()
    }

    /// `as->source_info[i]` changed by `f`.
    fn src_update(&self, i: usize, f: impl FnOnce(&mut Ac97SourceInfo)) {
        let mut si = self.source_info[i].get();
        f(&mut si);
        self.source_info[i].set(si);
    }

    /// `as->host_if`.
    fn host(&self) -> Ac97HostIf {
        self.host_if.get()
    }

    /// `ac97_read`: the register, or the shadow copy when the host does not read it or fails
    /// to.
    pub fn ac97_read(&self, reg: u8) -> u16 {
        let flags = self.host_flags.get();
        if ((flags & AC97_HOST_DONT_READ != 0)
            && (reg != AC97_REG_VENDOR_ID1 && reg != AC97_REG_VENDOR_ID2 && reg != AC97_REG_RESET))
            || (flags & AC97_HOST_DONT_READANY != 0)
        {
            return self.shadow_reg[usize::from(reg >> 1)].get();
        }

        let hi = self.host();
        let mut val = 0u16;
        // SAFETY: `hi.arg` is the host's own argument, the one it filled `read` for.
        if unsafe { host_method!(hi, read)(hi.arg, reg, &mut val) }.is_err() {
            val = self.shadow_reg[usize::from(reg >> 1)].get();
        }
        val
    }

    /// `ac97_write`.
    pub fn ac97_write(&self, reg: u8, val: u16) -> Result<(), Errno> {
        self.shadow_reg[usize::from(reg >> 1)].set(val);
        let hi = self.host();
        // SAFETY: `hi.arg` is the host's own argument, the one it filled `write` for.
        unsafe { host_method!(hi, write)(hi.arg, reg, val) }
    }

    /// `ac97_setup_defaults`.
    fn ac97_setup_defaults(&self) {
        for s in &self.shadow_reg {
            s.set(0);
        }

        for si in &SOURCE_INFO {
            let _ = self.ac97_write(si.reg, si.default_value);
        }
    }

    /// `ac97_check_capability`.
    fn ac97_check_capability(&self, check: u8) -> bool {
        match check {
            CHECK_NONE => true,
            CHECK_SURROUND => self.ext_id.get() & AC97_EXT_AUDIO_SDAC != 0,
            CHECK_CENTER => self.ext_id.get() & AC97_EXT_AUDIO_CDAC != 0,
            CHECK_LFE => self.ext_id.get() & AC97_EXT_AUDIO_LDAC != 0,
            CHECK_SPDIF => self.ext_id.get() & AC97_EXT_AUDIO_SPDIF != 0,
            CHECK_HEADPHONES => self.caps.get() & AC97_CAPS_HEADPHONES != 0,
            CHECK_TONE => self.caps.get() & AC97_CAPS_TONECTRL != 0,
            CHECK_MIC => self.caps.get() & AC97_CAPS_MICIN != 0,
            CHECK_LOUDNESS => self.caps.get() & AC97_CAPS_LOUDNESS != 0,
            CHECK_3D => ac97_caps_enhancement(self.caps.get()) != 0,
            _ => {
                printf(format_args!(
                    "ac97_check_capability: internal error: feature={check}\n"
                ));
                false
            }
        }
    }

    /// `ac97_setup_source_info`: the mixer of this codec: the controls of `source_info[]`
    /// its capabilities allow (with a `mute` control after each value that has one), their
    /// classes, and the `prev`/`next` chains of the controls of one device.
    fn ac97_setup_source_info(&self) {
        let mut ouridx = 0usize;

        for src in &SOURCE_INFO {
            if !self.ac97_check_capability(src.req_feature) {
                continue;
            }

            let mut si = *src;
            match si.type_ {
                AUDIO_MIXER_CLASS => {
                    si.mixer_class = ouridx as i16;
                    self.source_info[ouridx].set(si);
                    ouridx += 1;
                }
                AUDIO_MIXER_VALUE => {
                    // Todo - Test to see if it works
                    self.source_info[ouridx].set(si);
                    ouridx += 1;

                    // Add an entry for mute, if necessary
                    if si.mute {
                        si.qualifier = Some(AudioNmute);
                        si.type_ = AUDIO_MIXER_ENUM;
                        si.info_value = None;
                        si.info_enum = Some(&AC97_ON_OFF);
                        si.bits = 1;
                        si.ofs = 15;
                        si.mute = false;
                        si.polarity = false;
                        self.source_info[ouridx].set(si);
                        ouridx += 1;
                    }
                }
                AUDIO_MIXER_ENUM => {
                    // Todo - Test to see if it works
                    self.source_info[ouridx].set(si);
                    ouridx += 1;
                }
                _ => {
                    printf(format_args!("ac97: shouldn't get here\n"));
                }
            }
        }

        let n = ouridx;
        self.num_source_info.set(n);

        for idx in 0..n {
            let si = self.src(idx);

            // Find mixer class
            for idx2 in 0..n {
                let si2 = self.src(idx2);

                if si2.type_ == AUDIO_MIXER_CLASS && si.class == si2.class {
                    self.src_update(idx, |s| s.mixer_class = idx2 as i16);
                }
            }

            // Setup prev and next pointers
            let si = self.src(idx);
            if si.prev != 0 || si.qualifier.is_some() {
                continue;
            }

            self.src_update(idx, |s| s.prev = AUDIO_MIXER_LAST as i16);
            let mut previdx = idx;

            for idx2 in 0..n {
                if idx2 == idx {
                    continue;
                }

                let si2 = self.src(idx2);

                if si2.prev == 0 && si.class == si2.class && si.device == si2.device {
                    self.src_update(previdx, |s| s.next = idx2 as i16);
                    self.src_update(idx2, |s| s.prev = previdx as i16);

                    previdx = idx2;
                }
            }

            self.src_update(previdx, |s| s.next = AUDIO_MIXER_LAST as i16);
        }
    }

    /// The `if (as->ext_id & (VRA | DRA | SPDIF | VRM | CDAC | SDAC | LDAC))` block that
    /// `ac97_attach` and `ac97_resume` share: turns on the extended audio features the
    /// codec reports. Returns whether the block ran.
    fn ac97_enable_ext_audio(&self) -> bool {
        let ext_id = self.ext_id.get();
        if ext_id
            & (AC97_EXT_AUDIO_VRA
                | AC97_EXT_AUDIO_DRA
                | AC97_EXT_AUDIO_SPDIF
                | AC97_EXT_AUDIO_VRM
                | AC97_EXT_AUDIO_CDAC
                | AC97_EXT_AUDIO_SDAC
                | AC97_EXT_AUDIO_LDAC)
            == 0
        {
            return false;
        }

        let mut extstat = self.ac97_read(AC97_REG_EXT_AUDIO_CTRL);
        extstat &= !AC97_EXT_AUDIO_DRA;

        if ext_id & AC97_EXT_AUDIO_VRM != 0 {
            extstat |= AC97_EXT_AUDIO_VRM;
        }

        if ext_id & AC97_EXT_AUDIO_LDAC != 0 {
            extstat |= AC97_EXT_AUDIO_LDAC;
        }
        if ext_id & AC97_EXT_AUDIO_SDAC != 0 {
            extstat |= AC97_EXT_AUDIO_SDAC;
        }
        if ext_id & AC97_EXT_AUDIO_CDAC != 0 {
            extstat |= AC97_EXT_AUDIO_CDAC;
        }
        if ext_id & AC97_EXT_AUDIO_SPDIF != 0 {
            // XXX S/PDIF gets same data as DAC?
            // maybe this should be settable?
            // default is SPSAAB (10/11) on AD1980 and ALC codecs.
            extstat &= !AC97_EXT_AUDIO_SPSA_MASK;
            extstat |= AC97_EXT_AUDIO_SPSA34;
            let mut val = self.ac97_read(AC97_REG_SPDIF_CTRL);
            val = (val & !AC97_SPDIF_SPSR_MASK) | AC97_SPDIF_SPSR_48K;
            let _ = self.ac97_write(AC97_REG_SPDIF_CTRL, val);
        }
        if ext_id & AC97_EXT_AUDIO_VRA != 0 {
            extstat |= AC97_EXT_AUDIO_VRA;
        }
        let _ = self.ac97_write(AC97_REG_EXT_AUDIO_CTRL, extstat);
        true
    }

    /// `ac97_get_portnum_by_name`.
    pub fn ac97_get_portnum_by_name(
        &self,
        class: Option<&[u8]>,
        device: Option<&[u8]>,
        qualifier: Option<&[u8]>,
    ) -> i32 {
        for idx in 0..self.num_source_info.get() {
            let si = self.src(idx);
            if class == si.class && device == si.device && qualifier == si.qualifier {
                return idx as i32;
            }
        }

        -1
    }

    /// `ac97_add_port`: appends an enum control to its device's chain; `-1` when it cannot.
    pub fn ac97_add_port(&self, src: &Ac97SourceInfo) -> i32 {
        let num = self.num_source_info.get();
        if num >= MAX_SOURCES {
            printf(format_args!(
                "ac97_add_port: internal error: increase MAX_SOURCES in {}\n",
                file!()
            ));
            return -1;
        }
        if !self.ac97_check_capability(src.req_feature) {
            return -1;
        }
        let ouridx = num;
        let mut si = *src;

        match si.type_ {
            AUDIO_MIXER_CLASS | AUDIO_MIXER_VALUE => {
                printf(format_args!(
                    "ac97_add_port: adding class/value is not supported yet.\n"
                ));
                return -1;
            }
            AUDIO_MIXER_ENUM => {}
            t => {
                printf(format_args!("ac97_add_port: unknown type: {t}\n"));
                return -1;
            }
        }
        self.num_source_info.set(num + 1);

        si.mixer_class = self.ac97_get_portnum_by_name(si.class, None, None) as i16;
        // Find the root of the device
        let mut idx = self.ac97_get_portnum_by_name(si.class, si.device, None);
        if idx < 0 {
            // The C indexes source_info[-1]; every class and device it is given exists.
            self.num_source_info.set(num);
            return -1;
        }
        // Find the last item
        while i32::from(self.src(idx as usize).next) != AUDIO_MIXER_LAST {
            idx = i32::from(self.src(idx as usize).next);
        }
        // Append
        self.src_update(idx as usize, |s| s.next = ouridx as i16);
        si.prev = idx as i16;
        si.next = AUDIO_MIXER_LAST as i16;
        self.source_info[ouridx].set(si);

        0
    }
}

/// `ac97_attach`: probes the codec behind `host_if` and attaches its mixer. On success the
/// host has the codec interface from its `attach` method (and keeps it for good); the
/// interface stays locked out of S/PDIF changes until the host calls `unlock` once more
/// than `lock`.
pub fn ac97_attach(host_if: &Ac97HostIf) -> Result<(), Errno> {
    let mut initfunc: Option<Ac97InitFn> = None;

    let Some(p) = malloc(size_of::<Ac97Softc>(), M_DEVBUF, M_NOWAIT | M_ZERO) else {
        return Err(Errno::ENOMEM);
    };
    let ptr = p.cast::<Ac97Softc>().as_ptr();
    // SAFETY: a fresh zeroed allocation of the size of an `Ac97Softc`; only `codec_if.vtbl`
    // (a reference) is not valid as zero bits, and it is written here before anything reads
    // the softc; every other member is valid as zero.
    unsafe {
        ptr::addr_of_mut!((*ptr).codec_if).write(Ac97CodecIf {
            as_: Cell::new(ptr),
            initfunc: Cell::new(None),
            vtbl: &AC97CIV,
        });
    }
    // SAFETY: initialised just above, and never freed after this function succeeds.
    let sc: &Ac97Softc = unsafe { &*ptr };
    sc.host_if.set(*host_if);

    // SAFETY: `host_if.arg` is the host's own argument, the one it filled `attach` for.
    if let Err(error) = unsafe { host_method!(host_if, attach)(host_if.arg, &sc.codec_if) } {
        free(p, M_DEVBUF, size_of::<Ac97Softc>());
        return Err(error);
    }

    // SAFETY: `host_if.arg` is the host's own argument, the one it filled `reset` for.
    unsafe { host_method!(host_if, reset)(host_if.arg) };
    delay(1000);

    let _ = sc.ac97_write(AC97_REG_POWER, 0);
    let _ = sc.ac97_write(AC97_REG_RESET, 0);
    delay(10000);

    if let Some(flags) = host_if.flags {
        // SAFETY: `host_if.arg` is the host's own argument, the one it filled `flags` for.
        sc.host_flags.set(unsafe { flags(host_if.arg) });
    }

    sc.ac97_setup_defaults();
    let id1 = sc.ac97_read(AC97_REG_VENDOR_ID1);
    let id2 = sc.ac97_read(AC97_REG_VENDOR_ID2);
    sc.caps.set(sc.ac97_read(AC97_REG_RESET));

    let id = (u32::from(id1) << 16) | u32::from(id2);
    if id != 0 {
        printf(format_args!("ac97: codec id {id:#010x}"));
        if let Some((vendor, codec)) = ac97_find_codec(id) {
            printf(format_args!(" ({}", vendor.name));
            match codec {
                Some(codec) => {
                    printf(format_args!(" {}", codec.name));
                    initfunc = codec.init;
                    if codec.rev != 0 {
                        printf(format_args!(" rev {}", id & u32::from(codec.rev)));
                    }
                }
                None => {
                    printf(format_args!(" <{:02x}>", id & 0xff));
                }
            }
            printf(format_args!(")"));
        }
        printf(format_args!("\n"));
    } else {
        printf(format_args!("ac97: codec id not read\n"));
    }

    let caps = sc.caps.get();
    if caps != 0 {
        printf(format_args!("ac97: codec features "));
        for (i, name) in AC97FEATURE.iter().enumerate() {
            if caps & (1 << i) != 0 {
                printf(format_args!("{name}, "));
            }
        }
        printf(format_args!(
            "{}\n",
            AC97ENHANCEMENT[usize::from(ac97_caps_enhancement(caps))]
        ));
    }

    sc.ac97_clock.set(AC97_STANDARD_CLOCK);
    sc.ext_id.set(sc.ac97_read(AC97_REG_EXT_AUDIO_ID));
    if sc.ac97_enable_ext_audio() && sc.ext_id.get() & AC97_EXT_AUDIO_VRA != 0 {
        // VRA should be enabled.
        // so it claims to do variable rate, let's make sure
        let _ = sc.ac97_write(AC97_REG_PCM_FRONT_DAC_RATE, 44100);
        let rate = sc.ac97_read(AC97_REG_PCM_FRONT_DAC_RATE);
        if rate != 44100 {
            // We can't believe ext_id
            sc.ext_id.set(0);
        }
        // restore the default value
        let _ = sc.ac97_write(AC97_REG_PCM_FRONT_DAC_RATE, AC97_SINGLE_RATE as u16);
    }

    sc.ac97_setup_source_info();

    delay(900 * 1000);

    // use initfunc for specific device
    sc.codec_if.initfunc.set(initfunc);
    if let Some(initfunc) = initfunc {
        initfunc(sc, 0);
    }

    // Just enable the DAC and master volumes by default
    let mut ctl = MixerCtrl {
        type_: AUDIO_MIXER_ENUM,
        ..MixerCtrl::default()
    };

    ctl.un.set_ord(0); // off
    ctl.dev =
        sc.ac97_get_portnum_by_name(Some(AudioCoutputs), Some(AudioNmaster), Some(AudioNmute));
    let _ = ac97_mixer_set_port(&sc.codec_if, &mut ctl);

    ctl.dev = sc.ac97_get_portnum_by_name(Some(AudioCinputs), Some(AudioNdac), Some(AudioNmute));
    let _ = ac97_mixer_set_port(&sc.codec_if, &mut ctl);

    ctl.dev = sc.ac97_get_portnum_by_name(Some(AudioCrecord), Some(AudioNvolume), Some(AudioNmute));
    let _ = ac97_mixer_set_port(&sc.codec_if, &mut ctl);

    ctl.type_ = AUDIO_MIXER_ENUM;
    ctl.un.set_ord(0);
    ctl.dev = sc.ac97_get_portnum_by_name(Some(AudioCrecord), Some(AudioNsource), None);
    let _ = ac97_mixer_set_port(&sc.codec_if, &mut ctl);

    Ok(())
}

/// `ac97_resume`: resets the link and the codec and restores its extended audio setup.
pub fn ac97_resume(host_if: &Ac97HostIf, codec_if: &Ac97CodecIf) -> Result<(), Errno> {
    let sc = codec_if.softc();

    // SAFETY: `host_if.arg` is the host's own argument, the one it filled `reset` for.
    unsafe { host_method!(host_if, reset)(host_if.arg) };
    delay(1000);

    let _ = sc.ac97_write(AC97_REG_POWER, 0);
    let _ = sc.ac97_write(AC97_REG_RESET, 0);
    delay(10000);

    sc.ac97_enable_ext_audio();

    // use initfunc for specific device
    if let Some(initfunc) = sc.codec_if.initfunc.get() {
        initfunc(sc, 1);
    }

    Ok(())
}

/// `ac97_lock`.
pub fn ac97_lock(codec_if: &Ac97CodecIf) {
    let sc = codec_if.softc();
    sc.lock_counter.set(sc.lock_counter.get().wrapping_add(1));
}

/// `ac97_unlock`.
pub fn ac97_unlock(codec_if: &Ac97CodecIf) {
    let sc = codec_if.softc();
    sc.lock_counter.set(sc.lock_counter.get().wrapping_sub(1));
}

/// `ac97_query_devinfo`: describes mixer control `dip.index`; `ENXIO` past the last.
pub fn ac97_query_devinfo(codec_if: &Ac97CodecIf, dip: &mut MixerDevinfo) -> Result<(), Errno> {
    let sc = codec_if.softc();

    if dip.index >= 0 && (dip.index as usize) < sc.num_source_info.get() {
        let si = sc.src(dip.index as usize);

        dip.type_ = si.type_;
        dip.mixer_class = i32::from(si.mixer_class);
        dip.prev = i32::from(si.prev);
        dip.next = i32::from(si.next);

        let name = si.qualifier.or(si.device).or(si.class);

        if let Some(name) = name {
            strlcpy(&mut dip.label.name, name);
        }

        if let Some(e) = si.info_enum {
            *dip.un.e_mut() = *e;
        } else if let Some(v) = si.info_value {
            *dip.un.v_mut() = *v;
        }

        // Set the delta for volume sources
        if dip.type_ == AUDIO_MIXER_VALUE {
            dip.un.v_mut().delta = 1 << (8 - si.bits);
        }

        return Ok(());
    }

    Err(Errno::ENXIO)
}

/// `ac97_mixer_set_port`: writes the control `cp.dev` into its register field.
pub fn ac97_mixer_set_port(codec_if: &Ac97CodecIf, cp: &mut MixerCtrl) -> Result<(), Errno> {
    let sc = codec_if.softc();

    if cp.dev < 0 || cp.dev as usize >= sc.num_source_info.get() {
        return Err(Errno::EINVAL);
    }

    let si = sc.src(cp.dev as usize);

    if cp.type_ == AUDIO_MIXER_CLASS || cp.type_ != si.type_ {
        return Err(Errno::EINVAL);
    }

    let spdif = si.req_feature == CHECK_SPDIF && si.reg == AC97_REG_EXT_AUDIO_CTRL;
    if spdif && sc.lock_counter.get() >= 0 {
        return Err(Errno::EBUSY);
    }

    let val = sc.ac97_read(si.reg);

    let mut mask: u16 = (1u16 << si.bits) - 1;
    let newval: u16;

    match cp.type_ {
        AUDIO_MIXER_ENUM => {
            let ord = cp.un.ord();
            if ord > i32::from(mask) || ord < 0 {
                return Err(Errno::EINVAL);
            }

            let mut nv = ((ord as u32) << si.ofs) as u16;
            if si.reg == AC97_REG_RECORD_SELECT {
                nv |= shl16(nv, 8 + u32::from(si.ofs));
                mask |= shl16(mask, 8);
                mask = shl16(mask, u32::from(si.ofs));
            } else if si.reg == AC97_REG_SURR_MASTER {
                nv = if ord != 0 { 0x8080 } else { 0x0000 };
                mask = 0x8080;
            } else {
                mask = shl16(mask, u32::from(si.ofs));
            }

            if si.mute {
                nv |= shl16(nv, 8);
                mask |= shl16(mask, 8);
            }
            newval = nv;
        }
        AUDIO_MIXER_VALUE => {
            let Some(value) = si.info_value else {
                return Err(Errno::EINVAL);
            };
            let uv = cp.un.value();

            if uv.num_channels <= 0 || uv.num_channels > value.num_channels {
                return Err(Errno::EINVAL);
            }

            let (mut l, mut r): (u16, u16);
            if uv.num_channels == 1 {
                l = u16::from(uv.level[AUDIO_MIXER_LEVEL_MONO]);
                r = l;
            } else if sc.host_flags.get() & AC97_HOST_SWAPPED_CHANNELS == 0 {
                l = u16::from(uv.level[AUDIO_MIXER_LEVEL_LEFT]);
                r = u16::from(uv.level[AUDIO_MIXER_LEVEL_RIGHT]);
            } else {
                r = u16::from(uv.level[AUDIO_MIXER_LEVEL_LEFT]);
                l = u16::from(uv.level[AUDIO_MIXER_LEVEL_RIGHT]);
            }

            if !si.polarity {
                l = 255 - l;
                r = 255 - r;
            }

            l >>= 8 - si.bits;
            r >>= 8 - si.bits;

            let mut nv = shl16(l & mask, u32::from(si.ofs));
            if value.num_channels == 2 {
                nv |= shl16(r & mask, u32::from(si.ofs) + 8);
                mask |= shl16(mask, 8);
            }
            mask = shl16(mask, u32::from(si.ofs));
            newval = nv;
        }
        _ => return Err(Errno::EINVAL),
    }

    sc.ac97_write(si.reg, (val & !mask) | newval)?;

    if spdif {
        let hi = sc.host();
        if let Some(spdif_event) = hi.spdif_event {
            // SAFETY: `hi.arg` is the host's own argument, the one it filled `spdif_event`
            // for.
            unsafe { spdif_event(hi.arg, cp.un.ord()) };
        }
    }

    Ok(())
}

/// `ac97_mixer_get_port`: reads the control `cp.dev` from its register field.
pub fn ac97_mixer_get_port(codec_if: &Ac97CodecIf, cp: &mut MixerCtrl) -> Result<(), Errno> {
    let sc = codec_if.softc();

    if cp.dev < 0 || cp.dev as usize >= sc.num_source_info.get() {
        return Err(Errno::EINVAL);
    }

    let si = sc.src(cp.dev as usize);

    if cp.type_ != si.type_ {
        return Err(Errno::EINVAL);
    }

    let val = sc.ac97_read(si.reg);

    let mask: u16 = (1u16 << si.bits) - 1;

    match cp.type_ {
        AUDIO_MIXER_ENUM => {
            cp.un.set_ord(i32::from((val >> si.ofs) & mask));
        }
        AUDIO_MIXER_VALUE => {
            let Some(value) = si.info_value else {
                return Err(Errno::EINVAL);
            };
            let nch = cp.un.value().num_channels;

            if nch <= 0 || nch > value.num_channels {
                return Err(Errno::EINVAL);
            }

            let (mut l, mut r): (u16, u16);
            if value.num_channels == 1 {
                l = (val >> si.ofs) & mask;
                r = l;
            } else if sc.host_flags.get() & AC97_HOST_SWAPPED_CHANNELS == 0 {
                l = (val >> si.ofs) & mask;
                r = (val >> (si.ofs + 8)) & mask;
            } else {
                r = (val >> si.ofs) & mask;
                l = (val >> (si.ofs + 8)) & mask;
            }

            l = shl16(l, 8 - u32::from(si.bits));
            r = shl16(r, 8 - u32::from(si.bits));
            if !si.polarity {
                l = 255 - l;
                r = 255 - r;
            }

            // The EAP driver averages l and r for stereo channels that are requested in MONO
            // mode. Does this make sense?
            let uv = cp.un.value_mut();
            if nch == 1 {
                uv.level[AUDIO_MIXER_LEVEL_MONO] = l as u8;
            } else if nch == 2 {
                uv.level[AUDIO_MIXER_LEVEL_LEFT] = l as u8;
                uv.level[AUDIO_MIXER_LEVEL_RIGHT] = r as u8;
            }
        }
        _ => return Err(Errno::EINVAL),
    }

    Ok(())
}

/// `ac97_get_portnum_by_name`, through the codec interface.
pub fn ac97_get_portnum_by_name(
    codec_if: &Ac97CodecIf,
    class: Option<&[u8]>,
    device: Option<&[u8]>,
    qualifier: Option<&[u8]>,
) -> i32 {
    codec_if
        .softc()
        .ac97_get_portnum_by_name(class, device, qualifier)
}

/// `ac97_set_rate`: programs the sample rate register `target` (`AC97_REG_PCM_*_RATE`) for
/// `*rate` and stores the rate the codec took back in it. A codec without variable rate
/// audio always gets and gives `AC97_SINGLE_RATE`; `EINVAL` when the rate does not fit the
/// register.
pub fn ac97_set_rate(codec_if: &Ac97CodecIf, target: i32, rate: &mut u64) -> Result<(), Errno> {
    let sc = codec_if.softc();
    let ext_id = sc.ext_id.get();

    if target == i32::from(AC97_REG_PCM_SURR_DAC_RATE) && ext_id & AC97_EXT_AUDIO_SDAC == 0 {
        return Ok(());
    }
    if target == i32::from(AC97_REG_PCM_LFE_DAC_RATE) && ext_id & AC97_EXT_AUDIO_LDAC == 0 {
        return Ok(());
    }
    if target == i32::from(AC97_REG_PCM_MIC_ADC_RATE) {
        if ext_id & AC97_EXT_AUDIO_VRM == 0 {
            *rate = u64::from(AC97_SINGLE_RATE);
            return Ok(());
        }
    } else if ext_id & AC97_EXT_AUDIO_VRA == 0 {
        *rate = u64::from(AC97_SINGLE_RATE);
        return Ok(());
    }
    if sc.ac97_clock.get() == 0 {
        sc.ac97_clock.set(AC97_STANDARD_CLOCK);
    }
    let clock = sc.ac97_clock.get();
    let mut value: u64 = rate.wrapping_mul(u64::from(AC97_STANDARD_CLOCK)) / u64::from(clock);
    let mut ext_stat: u16 = 0;
    // PCM_FRONT_DAC_RATE/PCM_SURR_DAC_RATE/PCM_LFE_DAC_RATE
    //	Check VRA, DRA
    // PCM_LR_ADC_RATE
    //	Check VRA
    // PCM_MIC_ADC_RATE
    //	Check VRM
    let power_bit: u16;
    let reg = match u8::try_from(target) {
        Ok(reg) => reg,
        Err(_) => {
            printf(format_args!(
                "ac97_set_rate: Unknown register: {target:#x}\n"
            ));
            return Err(Errno::EINVAL);
        }
    };
    match reg {
        AC97_REG_PCM_FRONT_DAC_RATE | AC97_REG_PCM_SURR_DAC_RATE | AC97_REG_PCM_LFE_DAC_RATE => {
            power_bit = AC97_POWER_OUT;
            if ext_id & AC97_EXT_AUDIO_DRA != 0 {
                ext_stat = sc.ac97_read(AC97_REG_EXT_AUDIO_CTRL);
                if value > 0x1ffff {
                    return Err(Errno::EINVAL);
                } else if value > 0xffff {
                    // Enable DRA
                    ext_stat |= AC97_EXT_AUDIO_DRA;
                    let _ = sc.ac97_write(AC97_REG_EXT_AUDIO_CTRL, ext_stat);
                    value /= 2;
                } else {
                    // Disable DRA
                    ext_stat &= !AC97_EXT_AUDIO_DRA;
                    let _ = sc.ac97_write(AC97_REG_EXT_AUDIO_CTRL, ext_stat);
                }
            } else if value > 0xffff {
                return Err(Errno::EINVAL);
            }
        }
        AC97_REG_PCM_LR_ADC_RATE | AC97_REG_PCM_MIC_ADC_RATE => {
            power_bit = AC97_POWER_IN;
            if value > 0xffff {
                return Err(Errno::EINVAL);
            }
        }
        _ => {
            printf(format_args!(
                "ac97_set_rate: Unknown register: {target:#x}\n"
            ));
            return Err(Errno::EINVAL);
        }
    }

    let power = sc.ac97_read(AC97_REG_POWER);
    let _ = sc.ac97_write(AC97_REG_POWER, power | power_bit);

    let _ = sc.ac97_write(reg, value as u16);
    let actual = sc.ac97_read(reg);
    let actual = (u32::from(actual).wrapping_mul(clock) / AC97_STANDARD_CLOCK) as u16;

    let _ = sc.ac97_write(AC97_REG_POWER, power);
    if ext_stat & AC97_EXT_AUDIO_DRA != 0 {
        *rate = u64::from(actual) * 2;
    } else {
        *rate = u64::from(actual);
    }
    Ok(())
}

/// `ac97_set_clock`: the codec's AC-link clock, when it is not 48 kHz.
pub fn ac97_set_clock(codec_if: &Ac97CodecIf, clock: u32) {
    codec_if.softc().ac97_clock.set(clock);
}

/// `ac97_get_extcaps`: the extended audio ID register.
pub fn ac97_get_extcaps(codec_if: &Ac97CodecIf) -> u16 {
    codec_if.softc().ext_id.get()
}

// Codec-dependent initialization

/// `ac97_ad1885_init`.
pub fn ac97_ad1885_init(sc: &Ac97Softc, resuming: i32) {
    if resuming != 0 {
        return;
    }

    for i in 0..sc.num_source_info.get() {
        match sc.src(i).reg {
            AC97_REG_HEADPHONE_VOLUME => sc.src_update(i, |s| s.reg = AC97_REG_MASTER_VOLUME),
            AC97_REG_MASTER_VOLUME => sc.src_update(i, |s| s.reg = AC97_REG_HEADPHONE_VOLUME),
            _ => {}
        }
    }
}

/// `AC97_AD1886_JACK_SENSE`.
const AC97_AD1886_JACK_SENSE: u8 = 0x72;

/// `ac97_ad1886_init`.
pub fn ac97_ad1886_init(sc: &Ac97Softc, _resuming: i32) {
    let _ = sc.ac97_write(AC97_AD1886_JACK_SENSE, 0x0010);
}

/// `ac97_ad198x_init`.
pub fn ac97_ad198x_init(sc: &Ac97Softc, resuming: i32) {
    let misc = sc.ac97_read(AC97_AD_REG_MISC);
    let _ = sc.ac97_write(
        AC97_AD_REG_MISC,
        misc | AC97_AD_MISC_HPSEL | AC97_AD_MISC_LOSEL,
    );

    if resuming != 0 {
        return;
    }

    for i in 0..sc.num_source_info.get() {
        match sc.src(i).reg {
            AC97_REG_SURR_MASTER => sc.src_update(i, |s| s.reg = AC97_REG_MASTER_VOLUME),
            AC97_REG_MASTER_VOLUME => sc.src_update(i, |s| s.reg = AC97_REG_SURR_MASTER),
            _ => {}
        }
    }
}

/// `ac97_alc650_init`.
pub fn ac97_alc650_init(sc: &Ac97Softc, resuming: i32) {
    let mut misc = sc.ac97_read(AC97_ALC650_REG_MISC);
    if sc.host_flags.get() & AC97_HOST_ALC650_PIN47_IS_EAPD != 0 {
        misc &= !AC97_ALC650_MISC_PIN47;
    }
    misc &= !AC97_ALC650_MISC_VREFDIS;
    let _ = sc.ac97_write(AC97_ALC650_REG_MISC, misc);

    if resuming != 0 {
        return;
    }

    let sources = [
        enum_ent(
            AudioCoutputs,
            AudioNsurround,
            Some(b"lineinjack"),
            &AC97_ON_OFF,
            (
                AC97_ALC650_REG_MULTI_CHANNEL_CONTROL,
                0x0000,
                1,
                9,
                false,
                false,
                CHECK_SURROUND,
            ),
        ),
        enum_ent(
            AudioCoutputs,
            AudioNcenter,
            Some(b"micjack"),
            &AC97_ON_OFF,
            (
                AC97_ALC650_REG_MULTI_CHANNEL_CONTROL,
                0x0000,
                1,
                10,
                false,
                false,
                CHECK_CENTER,
            ),
        ),
        enum_ent(
            AudioCoutputs,
            AudioNlfe,
            Some(b"micjack"),
            &AC97_ON_OFF,
            (
                AC97_ALC650_REG_MULTI_CHANNEL_CONTROL,
                0x0000,
                1,
                10,
                false,
                false,
                CHECK_LFE,
            ),
        ),
    ];

    for src in &sources {
        sc.ac97_add_port(src);
    }
}

/// `ac97_cx20468_init`.
pub fn ac97_cx20468_init(sc: &Ac97Softc, _resuming: i32) {
    let misc = sc.ac97_read(AC97_CX_REG_MISC);
    let _ = sc.ac97_write(
        AC97_CX_REG_MISC,
        misc & !(AC97_CX_SPDIFEN | AC97_CX_COPYRIGHT | AC97_CX_MASK),
    );
}

/// `ac97_vt1616_init`.
pub fn ac97_vt1616_init(sc: &Ac97Softc, _resuming: i32) {
    if sc.host_flags.get() & AC97_HOST_VT1616_DYNEX != 0 {
        let mut reg = sc.ac97_read(AC97_VT_REG_TEST);

        // disable 'hp' mixer controls controlling the surround pins
        reg &= !AC97_VT_LVL;

        // disable downmixing
        reg &= !(AC97_VT_LCTF | AC97_VT_STF);

        // enable DC offset removal
        reg |= AC97_VT_BPDC;

        let _ = sc.ac97_write(AC97_VT_REG_TEST, reg);
    }
}

#[cfg(test)]
mod tests;
