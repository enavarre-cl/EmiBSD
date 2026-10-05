/*	$OpenBSD: audio_if.h,v 1.43 2025/11/02 14:33:06 ratchov Exp $	*/
/*	$NetBSD: audio_if.h,v 1.24 1998/01/10 14:07:25 tv Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1994 Havard Eidnes.
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

//! `<dev/audio_if.h>`: the interface between audio(4) and the hardware drivers.
//!
//! Upstream: sys/dev/audio_if.h @ 3ce1f3f79392
//!
//! A hardware driver (azalia(4), auich(4)) fills a `static` [`AudioHwIf`] and hands it,
//! with its own softc as the opaque handle, to `audio_attach_mi` (`dev/audio.rs`). audio(4)
//! then calls the driver through the table with that handle as first argument. The
//! functions `audio_attach_mi`, `audioprint`, `audio_blksz_bytes` and the `audio_lock`
//! mutex (`AUDIO_LOCK`) this header declares live in `dev/audio.rs`, where the C defines
//! them.
//!
//! ## Deviations
//! - The operations are `unsafe fn`s over the `void *` handle (`docs/C_TO_RUST.md`, a
//!   table of function pointers over an opaque `void *`): only the driver knows the type
//!   behind it, and the caller vouches that it is the handle the driver attached with this
//!   table. Every member is an `Option` because the C allows NULL for each (audio(4) checks
//!   the mandatory ones under `DIAGNOSTIC`).
//! - The `int` returns that are an errno are `Result<(), Errno>`; `allocm` returns
//!   `Option<NonNull<u8>>` (NULL is `None`); `display_name` takes the name buffer as a
//!   slice instead of a pointer and a size.
//! - The interrupt call-back `void (*)(void *)` is [`AudioIntr`], an `unsafe fn`: the
//!   driver must call it with the argument audio(4) passed with it, and with `AUDIO_LOCK`
//!   held.
//! - The DMA buffer pointers (`void *start`, `void *end`, `void *block`) stay raw
//!   `*mut u8`: they are hardware addresses' kernel mappings shared with the device.
//! - `struct audio_attach_args`' `hwif` is `*const c_void` as in C (an `audio_hw_if` or a
//!   `midi_hw_if`, told apart by `type`); midi(4) is not ported.

use core::ffi::c_void;
use core::ptr::NonNull;

use crate::sys::audioio::{MixerCtrl, MixerDevinfo};
use crate::sys::errno::Errno;

/// `AUDIO_BPS(bits)`: the bytes a sample of `bits` bits takes.
pub const fn audio_bps(bits: u32) -> u32 {
    if bits <= 8 {
        1
    } else if bits <= 16 {
        2
    } else {
        4
    }
}

/// `AUDIO_ENCODING_NONE`: no encoding assigned.
pub const AUDIO_ENCODING_NONE: u32 = 0;
/// `AUDIO_ENCODING_ULAW`: ITU G.711 mu-law.
pub const AUDIO_ENCODING_ULAW: u32 = 1;
/// `AUDIO_ENCODING_ALAW`: ITU G.711 A-law.
pub const AUDIO_ENCODING_ALAW: u32 = 2;
/// `AUDIO_ENCODING_SLINEAR_LE`.
pub const AUDIO_ENCODING_SLINEAR_LE: u32 = 6;
/// `AUDIO_ENCODING_SLINEAR_BE`.
pub const AUDIO_ENCODING_SLINEAR_BE: u32 = 7;
/// `AUDIO_ENCODING_ULINEAR_LE`.
pub const AUDIO_ENCODING_ULINEAR_LE: u32 = 8;
/// `AUDIO_ENCODING_ULINEAR_BE`.
pub const AUDIO_ENCODING_ULINEAR_BE: u32 = 9;

/// `struct audio_params`: the encoding parameters audio(4) and the driver negotiate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AudioParams {
    /// `sample_rate`.
    pub sample_rate: u64,
    /// `encoding`: mu-law, linear, etc (`AUDIO_ENCODING_*`).
    pub encoding: u32,
    /// `precision`: bits/sample.
    pub precision: u32,
    /// `bps`: bytes/sample.
    pub bps: u32,
    /// `msb`: data alignment.
    pub msb: u32,
    /// `channels`: mono(1), stereo(2).
    pub channels: u32,
}

/// `void (*intr)(void *)`: the block call-back audio(4) hands to `start_output`,
/// `start_input`, `trigger_output` and `trigger_input` (`audio_pintr`, `audio_rintr`).
///
/// # Safety
///
/// The argument is the `void *` audio(4) passed together with the function, and the
/// caller holds `AUDIO_LOCK` (`audio_lock`).
pub type AudioIntr = unsafe fn(*mut c_void);

/// `SPKR_ON`.
pub const SPKR_ON: i32 = 1;
/// `SPKR_OFF`.
pub const SPKR_OFF: i32 = 0;

/// `int (*open)(void *, int)`.
pub type AudioOpenFn = unsafe fn(*mut c_void, i32) -> Result<(), Errno>;
/// `void (*close)(void *)`, and the other methods with only the handle and no result
/// (`underrun`).
pub type AudioVoidFn = unsafe fn(*mut c_void);
/// `int (*set_params)(void *, int, int, struct audio_params *, struct audio_params *)`.
pub type AudioSetParamsFn =
    unsafe fn(*mut c_void, i32, i32, &mut AudioParams, &mut AudioParams) -> Result<(), Errno>;
/// `int (*round_blocksize)(void *, int)`.
pub type AudioRoundBlocksizeFn = unsafe fn(*mut c_void, i32) -> i32;
/// `int (*f)(void *)`: `commit_settings`, `halt_output`, `halt_input`.
pub type AudioHandleFn = unsafe fn(*mut c_void) -> Result<(), Errno>;
/// `int (*init_output)(void *, void *, int)`, and `init_input`.
pub type AudioInitFn = unsafe fn(*mut c_void, *mut u8, i32) -> Result<(), Errno>;
/// `int (*start_output)(void *, void *, int, void (*)(void *), void *)`, and `start_input`.
pub type AudioStartFn =
    unsafe fn(*mut c_void, *mut u8, i32, AudioIntr, *mut c_void) -> Result<(), Errno>;
/// `int (*set_port)(void *, struct mixer_ctrl *)`, and `get_port`.
pub type AudioPortFn = unsafe fn(*mut c_void, &mut MixerCtrl) -> Result<(), Errno>;
/// `int (*query_devinfo)(void *, struct mixer_devinfo *)`.
pub type AudioQueryDevinfoFn = unsafe fn(*mut c_void, &mut MixerDevinfo) -> Result<(), Errno>;
/// `void *(*allocm)(void *, int, size_t, int, int)`.
pub type AudioAllocmFn = unsafe fn(*mut c_void, i32, usize, i32, i32) -> Option<NonNull<u8>>;
/// `void (*freem)(void *, void *, int)`.
pub type AudioFreemFn = unsafe fn(*mut c_void, NonNull<u8>, i32);
/// `size_t (*round_buffersize)(void *, int, size_t)`.
pub type AudioRoundBuffersizeFn = unsafe fn(*mut c_void, i32, usize) -> usize;
/// `int (*trigger_output)(void *, void *, void *, int, void (*)(void *), void *, struct
/// audio_params *)`, and `trigger_input`.
pub type AudioTriggerFn = unsafe fn(
    *mut c_void,
    *mut u8,
    *mut u8,
    i32,
    AudioIntr,
    *mut c_void,
    &AudioParams,
) -> Result<(), Errno>;
/// `void (*copy_output)(void *, size_t)`.
pub type AudioCopyOutputFn = unsafe fn(*mut c_void, usize);
/// `unsigned int (*set_blksz)(void *, int, struct audio_params *, struct audio_params *,
/// unsigned int)`.
pub type AudioSetBlkszFn =
    unsafe fn(*mut c_void, i32, &mut AudioParams, &mut AudioParams, u32) -> u32;
/// `unsigned int (*set_nblks)(void *, int, struct audio_params *, unsigned int, unsigned
/// int)`.
pub type AudioSetNblksFn = unsafe fn(*mut c_void, i32, &mut AudioParams, u32, u32) -> u32;
/// `size_t (*display_name)(void *, char *, size_t)`.
pub type AudioDisplayNameFn = unsafe fn(*mut c_void, &mut [u8]) -> usize;

/// `struct audio_hw_if`: the generic interface to a hardware driver.
///
/// Every operation takes the driver's opaque handle first. They are `unsafe fn`s whose
/// contract is that this handle is the `hdl` the driver gave to `audio_attach_mi` together
/// with this table.
///
/// Driver methods may sleep (e.g. in `malloc`); audio(4) calls them with `AUDIO_LOCK`
/// unlocked, and they lock it themselves when they touch data their interrupt handler
/// uses.
#[derive(Clone, Copy)]
pub struct AudioHwIf {
    /// `open(hdl, flags)`: open hardware.
    pub open: Option<AudioOpenFn>,
    /// `close(hdl)`: close hardware.
    pub close: Option<AudioVoidFn>,
    /// `set_params(hdl, setmode, usemode, play, rec)`: set the audio encoding parameters
    /// (record and play). An error if the requested parameters are impossible. The values
    /// in the params may be changed (e.g. rounding to the nearest sample rate).
    pub set_params: Option<AudioSetParamsFn>,
    /// `round_blocksize(hdl, blksz)`: hardware may have some say in the blocksize to
    /// choose.
    pub round_blocksize: Option<AudioRoundBlocksizeFn>,
    /// `commit_settings(hdl)`: changing settings may require taking device out of "data
    /// mode", which can be quite expensive. Also, `audiosetinfo()` may change several
    /// settings in quick succession. To avoid having to take the device in/out of "data
    /// mode", this indicates completion of settings adjustment.
    pub commit_settings: Option<AudioHandleFn>,
    /// `init_output(hdl, buffer, size)`.
    pub init_output: Option<AudioInitFn>,
    /// `init_input(hdl, buffer, size)`.
    pub init_input: Option<AudioInitFn>,
    /// `start_output(hdl, block, blksize, intr, intrarg)`: start one output block (these
    /// usually control DMA).
    pub start_output: Option<AudioStartFn>,
    /// `start_input(hdl, block, blksize, intr, intrarg)`: start one input block.
    pub start_input: Option<AudioStartFn>,
    /// `halt_output(hdl)`.
    pub halt_output: Option<AudioHandleFn>,
    /// `halt_input(hdl)`.
    pub halt_input: Option<AudioHandleFn>,
    /// `set_port(hdl, ctrl)`: mixer (in/out ports).
    pub set_port: Option<AudioPortFn>,
    /// `get_port(hdl, ctrl)`.
    pub get_port: Option<AudioPortFn>,
    /// `query_devinfo(hdl, devinfo)`: describe mixer control `devinfo.index`; an error
    /// past the last one.
    pub query_devinfo: Option<AudioQueryDevinfoFn>,
    /// `allocm(hdl, direction, size, type, flags)`: allocate memory for the ring buffer.
    /// Usually `malloc`.
    pub allocm: Option<AudioAllocmFn>,
    /// `freem(hdl, addr, type)`: free what `allocm` gave. Usually `free`.
    pub freem: Option<AudioFreemFn>,
    /// `round_buffersize(hdl, direction, size)`.
    pub round_buffersize: Option<AudioRoundBuffersizeFn>,
    /// `trigger_output(hdl, start, end, blksize, intr, intrarg, params)`: run the ring
    /// `[start, end)` and call `intr(intrarg)` after each block.
    pub trigger_output: Option<AudioTriggerFn>,
    /// `trigger_input(hdl, start, end, blksize, intr, intrarg, params)`.
    pub trigger_input: Option<AudioTriggerFn>,
    /// `copy_output(hdl, count)`: `count` new bytes were written to the play ring.
    pub copy_output: Option<AudioCopyOutputFn>,
    /// `underrun(hdl)`: the play ring ran empty; the driver inserts silence itself.
    pub underrun: Option<AudioVoidFn>,
    /// `set_blksz(hdl, mode, play, rec, blksz)`: the block size (frames) the hardware
    /// takes, close to `blksz`.
    pub set_blksz: Option<AudioSetBlkszFn>,
    /// `set_nblks(hdl, mode, params, blksz, nblks)`: the number of blocks the hardware
    /// takes, close to `nblks`.
    pub set_nblks: Option<AudioSetNblksFn>,
    /// `display_name(hdl, buf)`: the device's name into `buf`, NUL-terminated; its length,
    /// 0 for none.
    pub display_name: Option<AudioDisplayNameFn>,
}

impl AudioHwIf {
    /// A table with every member NULL, which a driver's `static` completes with struct
    /// update syntax (`..AudioHwIf::new()`), as C's designated initialisers leave the
    /// unnamed members zero.
    pub const fn new() -> Self {
        Self {
            open: None,
            close: None,
            set_params: None,
            round_blocksize: None,
            commit_settings: None,
            init_output: None,
            init_input: None,
            start_output: None,
            start_input: None,
            halt_output: None,
            halt_input: None,
            set_port: None,
            get_port: None,
            query_devinfo: None,
            allocm: None,
            freem: None,
            round_buffersize: None,
            trigger_output: None,
            trigger_input: None,
            copy_output: None,
            underrun: None,
            set_blksz: None,
            set_nblks: None,
            display_name: None,
        }
    }
}

impl Default for AudioHwIf {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct audio_attach_args`: what a hardware driver hands to audio(4) (or midi(4))
/// through `config_found_sm`.
#[derive(Clone, Copy, Debug)]
pub struct AudioAttachArgs {
    /// `type`: `AUDIODEV_TYPE_*`.
    pub type_: i32,
    /// `hwif`: either an `audio_hw_if *` or a `midi_hw_if *`.
    pub hwif: *const c_void,
    /// `hdl`: the driver's handle, first argument of every `hwif` operation.
    pub hdl: *mut c_void,
    /// `cookie`: the wskbd(4) cookie that names this device for the volume keys.
    pub cookie: *mut c_void,
}

/// `AUDIODEV_TYPE_AUDIO`.
pub const AUDIODEV_TYPE_AUDIO: i32 = 0;
/// `AUDIODEV_TYPE_MIDI`.
pub const AUDIODEV_TYPE_MIDI: i32 = 1;
/// `AUDIODEV_TYPE_OPL`.
pub const AUDIODEV_TYPE_OPL: i32 = 2;
/// `AUDIODEV_TYPE_MPU`.
pub const AUDIODEV_TYPE_MPU: i32 = 3;
/// `AUDIODEV_TYPE_RADIO`.
pub const AUDIODEV_TYPE_RADIO: i32 = 4;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_per_sample() {
        assert_eq!(audio_bps(8), 1);
        assert_eq!(audio_bps(12), 2);
        assert_eq!(audio_bps(16), 2);
        assert_eq!(audio_bps(24), 4);
        assert_eq!(audio_bps(32), 4);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/audio_if.h");
        let ours = crate::reftest::assert_defines!(defs;
            AUDIO_ENCODING_NONE, AUDIO_ENCODING_ULAW, AUDIO_ENCODING_ALAW,
            AUDIO_ENCODING_SLINEAR_LE, AUDIO_ENCODING_SLINEAR_BE, AUDIO_ENCODING_ULINEAR_LE,
            AUDIO_ENCODING_ULINEAR_BE, SPKR_ON, SPKR_OFF, AUDIODEV_TYPE_AUDIO,
            AUDIODEV_TYPE_MIDI, AUDIODEV_TYPE_OPL, AUDIODEV_TYPE_MPU, AUDIODEV_TYPE_RADIO);
        crate::reftest::assert_complete(&defs, "AUDIO", &ours);
    }
}
