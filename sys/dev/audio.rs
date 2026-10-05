/*	$OpenBSD: audio.c,v 1.213 2025/11/18 09:30:27 ratchov Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2015 Alexandre Ratchov <alex@caoua.org>
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

//! audio(4): the machine-independent audio layer between the hardware drivers (azalia(4),
//! auich(4)) and `/dev/audioN` and `/dev/audioctlN`.
//!
//! Upstream: sys/dev/audio.c @ 3ce1f3f79392
//!
//! A hardware driver calls [`audio_attach_mi`] with its `static` [`AudioHwIf`] and its
//! softc as handle; `audio* at <driver>?` then attaches an `audio` device whose two ring
//! buffers (`play` and `rec`) the driver's DMA runs over. The driver's interrupt handler
//! takes [`AUDIO_LOCK`] and calls back [`audio_pintr`] / [`audio_rintr`] once per block;
//! those advance the rings, insert silence on underrun, and wake the readers, writers and
//! knotes. `read(2)`/`write(2)` copy between user space and the rings, starting DMA on
//! their own when the buffers are ready, so `/dev/audio0` works from scripts; sndio's
//! `sio_sun` backend drives it with `AUDIO_SETPAR`/`AUDIO_GETPAR`, `AUDIO_START`/`AUDIO_STOP`
//! and `AUDIO_GETPOS`. The minor's high nibble picks `/dev/audioN` (0) or `/dev/audioctlN`
//! (0xc0), the mixer: `AUDIO_MIXER_*` ioctls, and `read(2)` of the indices of the controls
//! that changed.
//!
//! Locking is the C's: [`AUDIO_LOCK`] (`audio_lock`, `IPL_AUDIO`) protects what both the
//! interrupt handler and the system calls touch (the ring pointers, `blocking`, the
//! counters, the mixer events, the knotes); driver methods are called with it unlocked
//! (they may sleep) and take it themselves around what their interrupt shares. The device
//! switch entries are not `D_MPSAFE`, so system calls run under the kernel lock.
//!
//! ## Deviations
//! - `AUDIO_DEBUG` is not configured: the `DPRINTF`/`DPRINTFN` messages, the debug-only
//!   `panic`s of `audio_buf_rdiscard`/`audio_buf_wcommit` and the encoding check of
//!   `audio_calc_sil` are not carried over.
//! - `#if NWSKBD > 0` (the volume keys: `wskbd_mixer_init`, `wskbd_set_mixervolume*`,
//!   `wskbd_set_mixermute`, `kern.audio.kbdcontrol`) is ported and compiled: GENERIC has
//!   wskbd(4) on both archs and the code needs only task(9). Nothing calls the
//!   `wskbd_set_mixer*` entry points until wskbd(4) and the acpi hotkey drivers are ported
//!   (M13); `wskbd_mixer_init` runs at attach as in C.
//! - `audio_softintr` does not exist at this pin: the C wakes sleepers and knotes directly
//!   from `audio_pintr`/`audio_rintr` under `audio_lock`, and so does this file.
//! - The ring's DMA memory is a raw pointer in a `Cell` (`AudioBuf::data`); the parts the
//!   ring protocol hands to a system call are borrowed as slices by the `unsafe fn`
//!   [`AudioBuf::bytes`], whose contract is that protocol (`docs/C_TO_RUST.md`, `struct
//!   buf`'s data).
//! - `struct mixer_ev`'s `next` and the softc's `mix_pending` are
//!   `Option<NonNull<MixerEv>>` (NULL is `None`).
//! - `mix_ents` is allocated `M_ZERO`: the C leaves the class entries uninitialised and
//!   saves and restores them on suspend anyway.
//! - `audio_setpar_blksz`'s `np`/`nr` start at 1 instead of being unset: the C reads them
//!   only for the directions `sc->mode` has, which is never 0 there.
//! - `audiokqfilter` on a minor that is neither audio nor audioctl returns `ENXIO`; the C
//!   inserts the knote on an unset klist pointer (`audioopen` refuses such minors first).
//! - The ioctl argument is the byte slice `sys_ioctl` copied; a handler works on it in
//!   place when it is aligned for the structure (the `malloc`ed buffer of a large
//!   argument) and on a local copy written back otherwise (`audio_ioctl_arg`).
//! - `audio_record_enable` and `audio_kbdcontrol_enable` are the `AtomicI32`s
//!   [`AUDIO_RECORD_ENABLE`] and [`AUDIO_KBDCONTROL_ENABLE`] (the C reads them with
//!   `atomic_load_int`); `kern.audio` (`sysctl_audio`, `kern_sysctl.rs`) sets them.
//! - Arithmetic the C lets wrap (`size_t`/`unsigned` ring sizes, `pos`, user-chosen
//!   `round`, mixer indices) wraps explicitly, so a debug kernel does not trap where the
//!   C does not.

use core::cell::Cell;
use core::ffi::c_void;
use core::mem::{align_of, size_of};
use core::ptr::{self, NonNull};
use core::slice;
use core::sync::atomic::{AtomicI32, Ordering};

use libkern::strlcpy;

use crate::dev::audio_if::{
    AUDIO_ENCODING_ALAW, AUDIO_ENCODING_SLINEAR_BE, AUDIO_ENCODING_SLINEAR_LE, AUDIO_ENCODING_ULAW,
    AUDIO_ENCODING_ULINEAR_BE, AUDIO_ENCODING_ULINEAR_LE, AUDIODEV_TYPE_AUDIO, AUDIODEV_TYPE_MPU,
    AUDIODEV_TYPE_OPL, AudioAttachArgs, AudioHwIf, AudioParams,
};
use crate::dev::mulaw::{
    mulaw_to_slinear8, mulaw24_to_slinear24, slinear8_to_mulaw, slinear24_to_mulaw24,
};
use crate::kern::kern_event::{
    klist_free, klist_init_mutex, klist_insert, klist_invalidate, klist_remove, knote_locked,
};
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_malloc::{free, malloc, mallocarray};
use crate::kern::kern_prot::suser;
use crate::kern::kern_subr::uiomove;
use crate::kern::kern_synch::{msleep_nsec, tsleep_nsec, wakeup};
use crate::kern::kern_task::{SYSTQ, task_add, task_set};
use crate::kern::subr_autoconf::{config_found_sm, device_lookup, device_unref};
use crate::kern::subr_prf::{Str, panic, printf};
use crate::kern::vfs_subr::vdevgone;
use crate::machine::conf::{cdevsw, nchrdev};
use crate::machine::copy::AbiPod;
use crate::machine::intr::{IPL_AUDIO, spltty, splx};
use crate::sys::audioio::{
    AUDIO_GETDEV, AUDIO_GETPAR, AUDIO_GETPOS, AUDIO_GETSTATUS, AUDIO_MAX_GAIN, AUDIO_MIN_GAIN,
    AUDIO_MIXER_CLASS, AUDIO_MIXER_DEVINFO, AUDIO_MIXER_ENUM, AUDIO_MIXER_READ, AUDIO_MIXER_SET,
    AUDIO_MIXER_VALUE, AUDIO_MIXER_WRITE, AUDIO_SETPAR, AUDIO_START, AUDIO_STOP, AUMODE_PLAY,
    AUMODE_RECORD, AudioCinputs, AudioCoutputs, AudioCrecord, AudioDevice, AudioNdac, AudioNinput,
    AudioNmaster, AudioNmute, AudioNoutput, AudioNrecord, AudioNvolume, AudioPos, AudioStatus,
    AudioSwpar, MixerCtrl, MixerDevinfo,
};
use crate::sys::conf::DevTypeOpen;
use crate::sys::device::{
    CfMatch, Cfattach, Cfdriver, DV_DULL, DVACT_QUIESCE, DVACT_WAKEUP, DVF_ACTIVE, Device, Softc,
    UNCONF,
};
use crate::sys::endian::{BYTE_ORDER, LITTLE_ENDIAN};
use crate::sys::errno::Errno;
use crate::sys::event::{
    EVFILT_READ, EVFILT_WRITE, FILTEROP_ISFD, FILTEROP_MPSAFE, Filterops, Kevent, Klist, Knote,
    knote_modify, knote_process,
};
use crate::sys::fcntl::{FREAD, FWRITE};
use crate::sys::ioctl::{ioctl_arg, ioctl_ret};
use crate::sys::malloc::{M_DEVBUF, M_TEMP, M_WAITOK, M_ZERO};
use crate::sys::mutex::{Mutex, mutex_assert_locked};
use crate::sys::param::{PCATCH, PWAIT};
use crate::sys::proc::Proc;
use crate::sys::systm::INFSLP;
use crate::sys::task::Task;
use crate::sys::time::sec_to_nsec;
use crate::sys::types::{Dev, minor};
use crate::sys::uio::Uio;
use crate::sys::vnode::{IO_NDELAY, VCHR};

/// `NAUDIO`: `audio* at ...` is configured (`needs-flag`).
pub const NAUDIO: i32 = 1;

/// `AUDIO_DEV_AUDIO`: minor of `/dev/audio0`.
const AUDIO_DEV_AUDIO: u32 = 0;
/// `AUDIO_DEV_AUDIOCTL`: minor of `/dev/audioctl`.
const AUDIO_DEV_AUDIOCTL: u32 = 0xc0;
/// `AUDIO_BUFSZ`: buffer size in bytes.
const AUDIO_BUFSZ: usize = 65536;

// mixer entries added by the audio(4) layer

/// `MIXER_RECORD`: record class.
const MIXER_RECORD: i32 = 0;
/// `MIXER_RECORD_ENABLE`: record.enable control.
const MIXER_RECORD_ENABLE: i32 = 1;
/// `MIXER_RECORD_ENABLE_OFF`: record.enable=off value.
const MIXER_RECORD_ENABLE_OFF: i32 = 0;
/// `MIXER_RECORD_ENABLE_ON`: record.enable=on value.
const MIXER_RECORD_ENABLE_ON: i32 = 1;
/// `MIXER_RECORD_ENABLE_SYSCTL`: record.enable=sysctl value.
const MIXER_RECORD_ENABLE_SYSCTL: i32 = 2;

/// `WSKBD_MUTE_TOGGLE`.
const WSKBD_MUTE_TOGGLE: i32 = 1;
/// `WSKBD_MUTE_DISABLE`.
const WSKBD_MUTE_DISABLE: i32 = 2;
/// `WSKBD_MUTE_ENABLE`.
const WSKBD_MUTE_ENABLE: i32 = 3;

/// `AUDIO_UNIT(n)`: the unit in a minor's low nibble.
const fn audio_unit(dev: Dev) -> i32 {
    (minor(dev) & 0x0f) as i32
}

/// `AUDIO_DEV(n)`: audio or audioctl, in a minor's high nibble.
const fn audio_dev(dev: Dev) -> u32 {
    minor(dev) & 0xf0
}

/// `struct audio_buf`: a DMA ring buffer.
///
/// The ring is `klen` bytes of `data`; the FIFO the user sees is the first `ulen` of them.
/// `start` and `used` say which bytes hold data. The pointers, `blocking`, `pos` and
/// `xrun` are protected by `AUDIO_LOCK`; the sizes change only while DMA is stopped.
pub struct AudioBuf {
    /// `data`: DMA memory block.
    pub data: Cell<*mut u8>,
    /// `datalen`: size of DMA memory block.
    pub datalen: Cell<usize>,
    /// `klen`: size of DMA FIFO.
    pub klen: Cell<usize>,
    /// `ulen`: size of the userland FIFO.
    pub ulen: Cell<usize>,
    /// `start`: first byte used in the FIFO.
    pub start: Cell<usize>,
    /// `used`: bytes used in the FIFO.
    pub used: Cell<usize>,
    /// `blksz`: DMA block size.
    pub blksz: Cell<usize>,
    /// `nblks`: number of blocks.
    pub nblks: Cell<u32>,
    /// `klist`: list of knotes.
    pub klist: Klist,
    /// `pos`: bytes transferred.
    pub pos: Cell<u32>,
    /// `xrun`: bytes lost by xruns.
    pub xrun: Cell<u32>,
    /// `blocking`: read/write blocking.
    pub blocking: Cell<i32>,
}

impl AudioBuf {
    /// The ring's bytes `[off, off + len)`, which must lie inside `data`.
    ///
    /// # Safety
    ///
    /// `data` is the buffer `audio_buf_init` allocated, and the caller owns those bytes
    /// for the borrow under the ring protocol: the part between `rgetblk`/`rdiscard` or
    /// `wgetblk`/`wcommit` that the system call took under `AUDIO_LOCK`, the block the
    /// interrupt just finished (with the lock held), or the whole ring while DMA is
    /// stopped. The device does not touch those bytes meanwhile.
    #[allow(clippy::mut_from_ref)] // the DMA memory behind `data`, owned by the ring protocol
    pub unsafe fn bytes(&self, off: usize, len: usize) -> &mut [u8] {
        assert!(
            off.checked_add(len)
                .is_some_and(|e| e <= self.datalen.get()),
            "audio_buf: {off}+{len} past the buffer"
        );
        // SAFETY: inside the allocation (checked above); exclusive by the caller's
        // contract.
        unsafe { slice::from_raw_parts_mut(self.data.get().add(off), len) }
    }

    /// The offset of `p`, a pointer `rgetblk`/`wgetblk` returned, in `data`.
    fn offset_of(&self, p: *mut u8) -> usize {
        (p as usize).wrapping_sub(self.data.get() as usize)
    }
}

/// `struct wskbd_vol`: a volume control the keyboard keys drive.
#[derive(Default)]
pub struct WskbdVol {
    /// `val`: index of the value control.
    pub val: Cell<i32>,
    /// `mute`: index of the mute control.
    pub mute: Cell<i32>,
    /// `step`: increment/decrement step.
    pub step: Cell<i32>,
    /// `nch`: channels in the value control.
    pub nch: Cell<i32>,
    /// `val_pending`: pending change of val.
    pub val_pending: Cell<i32>,
    /// `mute_pending`: pending change of mute (`WSKBD_MUTE_*`).
    pub mute_pending: Cell<i32>,
}

/// `struct mixer_ev`: event indicating that a control was changed.
#[derive(Default)]
pub struct MixerEv {
    /// `next`.
    pub next: Cell<Option<NonNull<MixerEv>>>,
    /// `pending`.
    pub pending: Cell<i32>,
}

/// `void (*conv)(unsigned char *, int)`: an in-place encoding conversion (`mulaw.c`).
pub type ConvFn = fn(&mut [u8]);

/// `struct audio_softc`: the device structure.
#[repr(C)]
pub struct AudioSoftc {
    /// `dev`.
    pub dev: Device,
    /// `ops`: driver funcs.
    pub ops: Cell<Option<&'static AudioHwIf>>,
    /// `cookie`: wskbd cookie.
    pub cookie: Cell<*mut c_void>,
    /// `arg`: first arg to driver funcs.
    pub arg: Cell<*mut c_void>,
    /// `mode`: bitmask of `AUMODE_*`.
    pub mode: Cell<i32>,
    /// `quiesce`: device suspended.
    pub quiesce: Cell<i32>,
    /// `play`.
    pub play: AudioBuf,
    /// `rec`.
    pub rec: AudioBuf,
    /// `sw_enc`: user exposed `AUDIO_ENCODING_*`.
    pub sw_enc: Cell<u32>,
    /// `hw_enc`: hardware `AUDIO_ENCODING_*`.
    pub hw_enc: Cell<u32>,
    /// `bits`: bits per sample.
    pub bits: Cell<u32>,
    /// `bps`: bytes-per-sample.
    pub bps: Cell<u32>,
    /// `msb`: sample are MSB aligned.
    pub msb: Cell<u32>,
    /// `rate`: rate in Hz.
    pub rate: Cell<u32>,
    /// `round`: block size in frames.
    pub round: Cell<u32>,
    /// `pchan`: number of play channels.
    pub pchan: Cell<u32>,
    /// `rchan`: number of record channels.
    pub rchan: Cell<u32>,
    /// `silence`: a sample of silence.
    pub silence: Cell<[u8; 4]>,
    /// `pause`: not trying to start DMA.
    pub pause: Cell<i32>,
    /// `active`: DMA in process.
    pub active: Cell<i32>,
    /// `offs`: offset between play & rec dir.
    pub offs: Cell<i32>,
    /// `conv_enc`: encode to native.
    pub conv_enc: Cell<Option<ConvFn>>,
    /// `conv_dec`: decode to user.
    pub conv_dec: Cell<Option<ConvFn>>,
    /// `mix_ents`: mixer state for suspend/resume (`mix_nent` entries).
    pub mix_ents: Cell<*mut MixerCtrl>,
    /// `mix_nent`: size of mixer state.
    pub mix_nent: Cell<i32>,
    /// `mix_isopen`: mixer open for reading.
    pub mix_isopen: Cell<i32>,
    /// `mix_blocking`: `read()` blocking.
    pub mix_blocking: Cell<i32>,
    /// `mix_klist`: list of knotes.
    pub mix_klist: Klist,
    /// `mix_evbuf`: per mixer-control event (`mix_nent` entries).
    pub mix_evbuf: Cell<*mut MixerEv>,
    /// `mix_pending`: list of changed controls.
    pub mix_pending: Cell<Option<NonNull<MixerEv>>>,
    /// `spkr`: the keyboard's speaker volume.
    pub spkr: WskbdVol,
    /// `mic`: the keyboard's microphone volume.
    pub mic: WskbdVol,
    /// `wskbd_task`.
    pub wskbd_task: Task,
    /// `record_enable`: mixer record.enable value.
    pub record_enable: Cell<i32>,
}

// SAFETY: `#[repr(C)]` with the `struct device` first; every member is a `Cell` of an
// integer, raw pointer, `Option` of a reference or `fn`, a `Klist` or a `Task`, all valid
// as zero bytes, as `config_make_softc` requires.
unsafe impl Softc for AudioSoftc {}

impl AudioSoftc {
    /// `DEVNAME(sc)`.
    fn devname(&self) -> &str {
        self.dev.xname()
    }

    /// `sc->ops`: the driver's table; one with every member NULL before attach.
    fn ops(&self) -> &'static AudioHwIf {
        static NONE: AudioHwIf = AudioHwIf::new();
        self.ops.get().unwrap_or(&NONE)
    }

    /// `sc` as the `void *` the interrupt call-backs and the task take.
    fn as_arg(&self) -> *mut c_void {
        ptr::from_ref(self).cast_mut().cast()
    }

    /// `sc->mix_ents` as a slice.
    ///
    /// # Safety
    ///
    /// No other borrow of the entries is live: their users (attach, suspend and resume)
    /// run alone.
    #[allow(clippy::mut_from_ref)] // the C's malloc'd array behind `mix_ents`
    unsafe fn mix_ents(&self) -> &mut [MixerCtrl] {
        let n = usize::try_from(self.mix_nent.get()).unwrap_or(0);
        if self.mix_ents.get().is_null() || n == 0 {
            return &mut [];
        }
        // SAFETY: `audio_attach` allocated `mix_nent` zeroed entries (zero is a valid
        // `MixerCtrl`), freed only by `audio_detach`; exclusive by the caller's contract.
        unsafe { slice::from_raw_parts_mut(self.mix_ents.get(), n) }
    }

    /// `sc->mix_evbuf` as a slice.
    fn mix_evbuf(&self) -> &[MixerEv] {
        let n = usize::try_from(self.mix_nent.get()).unwrap_or(0);
        if self.mix_evbuf.get().is_null() || n == 0 {
            return &[];
        }
        // SAFETY: `audio_attach` allocated `mix_nent` zeroed entries (zero is a valid
        // `MixerEv`), freed only by `audio_detach`; the members are `Cell`s changed under
        // `AUDIO_LOCK`.
        unsafe { slice::from_raw_parts(self.mix_evbuf.get(), n) }
    }
}

/// The C calls through a NULL driver method: the driver's table lacks a mandatory member.
fn required<F: Copy>(f: Option<F>, name: &str) -> F {
    match f {
        Some(f) => f,
        None => panic(format_args!("audio: driver has no {name} method")),
    }
}

/// A reference to an attached audio device, from `device_lookup`, given back with
/// `device_unref` when dropped.
struct AudioRef(NonNull<Device>);

impl AudioRef {
    /// `(struct audio_softc *)device_lookup(&audio_cd, unit)`.
    fn lookup(unit: i32) -> Option<Self> {
        device_lookup(&AUDIO_CD, unit).map(Self)
    }

    /// The softc.
    fn sc(&self) -> &AudioSoftc {
        // SAFETY: `audio_cd`'s devices are made by `audio_ca`, whose softc is an
        // `AudioSoftc`; the reference this holds keeps the allocation alive.
        unsafe { self.0.as_ref().softc::<AudioSoftc>() }
    }
}

impl Drop for AudioRef {
    fn drop(&mut self) {
        // SAFETY: the reference `device_lookup` took.
        unsafe { device_unref(self.0) };
    }
}

/// `audio_ca`.
pub static AUDIO_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<AudioSoftc>(),
    ca_match: Some(audio_match),
    ca_attach: audio_attach,
    ca_detach: Some(audio_detach),
    ca_activate: Some(audio_activate),
};

/// `audio_cd`.
pub static AUDIO_CD: Cfdriver = Cfdriver::new(b"audio", DV_DULL, 0);

/// `audioctlread_filtops`.
pub static AUDIOCTLREAD_FILTOPS: Filterops = Filterops {
    f_flags: FILTEROP_ISFD | FILTEROP_MPSAFE,
    f_attach: None,
    f_detach: Some(filt_audioctlrdetach),
    f_event: Some(filt_audioctlread),
    f_modify: Some(filt_audiomodify),
    f_process: Some(filt_audioprocess),
};

/// `audiowrite_filtops`.
pub static AUDIOWRITE_FILTOPS: Filterops = Filterops {
    f_flags: FILTEROP_ISFD | FILTEROP_MPSAFE,
    f_attach: None,
    f_detach: Some(filt_audiowdetach),
    f_event: Some(filt_audiowrite),
    f_modify: Some(filt_audiomodify),
    f_process: Some(filt_audioprocess),
};

/// `audioread_filtops`.
pub static AUDIOREAD_FILTOPS: Filterops = Filterops {
    f_flags: FILTEROP_ISFD | FILTEROP_MPSAFE,
    f_attach: None,
    f_detach: Some(filt_audiordetach),
    f_event: Some(filt_audioread),
    f_modify: Some(filt_audiomodify),
    f_process: Some(filt_audioprocess),
};

/// `audio_lock`: protects data structures (including registers on the sound-card) that
/// are manipulated by both the interrupt handler and syscall code-paths.
///
/// Driver methods may sleep (e.g. in `malloc`); consequently the audio layer calls them
/// with the mutex unlocked. Driver methods are responsible for locking the mutex when they
/// manipulate data used by the interrupt handler and interrupts may occur.
///
/// Similarly, the driver is responsible for locking the mutex in its interrupt handler and
/// to call the audio layer call-backs (i.e. `audio_{p,r}intr()`) with the mutex locked.
pub static AUDIO_LOCK: Mutex = Mutex::new(IPL_AUDIO);

/// `audio_record_enable` \[a\]: whether audio recording is enabled when the mixerctl
/// setting is `record.enable=sysctl` (`kern.audio.record`).
pub static AUDIO_RECORD_ENABLE: AtomicI32 = AtomicI32::new(0);

/// `audio_kbdcontrol_enable` \[a\]: whether the keyboard's volume keys drive the mixer
/// (`kern.audio.kbdcontrol`; read by wskbd(4)).
pub static AUDIO_KBDCONTROL_ENABLE: AtomicI32 = AtomicI32::new(1);

/// `audio_gcd`: greatest common divisor.
pub fn audio_gcd(mut a: u32, mut b: u32) -> u32 {
    while b > 0 {
        let r = a % b;
        a = b;
        b = r;
    }
    a
}

/// `audio_blksz_bytes`: the least block size (in frames) such that both the corresponding
/// play and/or record block sizes (in bytes) are multiple of the given number of bytes.
pub fn audio_blksz_bytes(mode: i32, p: &AudioParams, r: &AudioParams, bytes: i32) -> i32 {
    let bytes = bytes as u32;
    // The C leaves the other direction's multiplier unset; it is never read.
    let mut np: u32 = 1;
    let mut nr: u32 = 1;

    if mode & AUMODE_PLAY != 0 {
        np = bytes / audio_gcd(p.bps * p.channels, bytes);
        if mode & AUMODE_RECORD == 0 {
            nr = np;
        }
    }
    if mode & AUMODE_RECORD != 0 {
        nr = bytes / audio_gcd(r.bps * r.channels, bytes);
        if mode & AUMODE_PLAY == 0 {
            np = nr;
        }
    }

    (nr * np / audio_gcd(nr, np)) as i32
}

/// `audio_mixer_wakeup`: wake the mixer's reader and knotes. Called with `AUDIO_LOCK`.
fn audio_mixer_wakeup(sc: &AudioSoftc) {
    mutex_assert_locked(&AUDIO_LOCK, "audio_mixer_wakeup");

    if sc.mix_blocking.get() != 0 {
        wakeup(ptr::from_ref(&sc.mix_blocking));
        sc.mix_blocking.set(0);
    }
    knote_locked(&sc.mix_klist, 0);
}

/// `audio_buf_wakeup`: wake the ring's reader or writer and knotes. Called with
/// `AUDIO_LOCK`.
fn audio_buf_wakeup(buf: &AudioBuf) {
    mutex_assert_locked(&AUDIO_LOCK, "audio_buf_wakeup");

    if buf.blocking.get() != 0 {
        wakeup(ptr::from_ref(&buf.blocking));
        buf.blocking.set(0);
    }
    knote_locked(&buf.klist, 0);
}

/// `audio_buf_init`: allocate a ring through the driver (or `malloc`).
fn audio_buf_init(sc: &AudioSoftc, buf: &AudioBuf, dir: i32) -> Result<(), Errno> {
    let ops = sc.ops();

    // SAFETY: `AUDIO_LOCK` is a static, which outlives every klist.
    unsafe { klist_init_mutex(&buf.klist, &AUDIO_LOCK) };
    match ops.round_buffersize {
        // SAFETY: `sc.arg` is the handle the driver attached `sc.ops` with.
        Some(f) => buf
            .datalen
            .set(unsafe { f(sc.arg.get(), dir, AUDIO_BUFSZ) }),
        None => buf.datalen.set(AUDIO_BUFSZ),
    }
    let data = match ops.allocm {
        // SAFETY: as above.
        Some(f) => unsafe { f(sc.arg.get(), dir, buf.datalen.get(), M_DEVBUF, M_WAITOK) },
        None => malloc(buf.datalen.get(), M_DEVBUF, M_WAITOK),
    };
    match data {
        Some(d) => buf.data.set(d.as_ptr()),
        None => {
            klist_free(&buf.klist);
            return Err(Errno::ENOMEM);
        }
    }
    Ok(())
}

/// `audio_buf_done`: free a ring.
fn audio_buf_done(sc: &AudioSoftc, buf: &AudioBuf) {
    if let Some(data) = NonNull::new(buf.data.get()) {
        match sc.ops().freem {
            // SAFETY: `sc.arg` is the handle the driver attached `sc.ops` with; `data`
            // came from its `allocm`.
            Some(f) => unsafe { f(sc.arg.get(), data, M_DEVBUF) },
            None => free(data, M_DEVBUF, buf.datalen.get()),
        }
        buf.data.set(ptr::null_mut());
    }
    klist_free(&buf.klist);
}

/// `audio_buf_rgetblk`: the reader pointer and the number of bytes available.
fn audio_buf_rgetblk(buf: &AudioBuf) -> (*mut u8, usize) {
    // `start` may be past `ulen` (the play ring's `klen` is larger); the C's size_t
    // difference wraps and `used` bounds it.
    let count = buf
        .ulen
        .get()
        .wrapping_sub(buf.start.get())
        .min(buf.used.get());
    (buf.data.get().wrapping_add(buf.start.get()), count)
}

/// `audio_buf_rdiscard`: discard `count` bytes at the start position.
fn audio_buf_rdiscard(buf: &AudioBuf, count: usize) {
    // AUDIO_DEBUG: the panic on count > used is not configured.
    buf.used.set(buf.used.get() - count);
    buf.start.set(buf.start.get() + count);
    if buf.start.get() >= buf.klen.get() {
        buf.start.set(buf.start.get() - buf.klen.get());
    }
}

/// `audio_buf_wcommit`: advance the writer pointer by `count` bytes.
fn audio_buf_wcommit(buf: &AudioBuf, count: usize) {
    // AUDIO_DEBUG: the panic on count > klen - used is not configured.
    buf.used.set(buf.used.get() + count);
}

/// `audio_buf_wgetblk`: the writer pointer and the number of bytes writable.
fn audio_buf_wgetblk(buf: &AudioBuf) -> (*mut u8, usize) {
    let mut end = buf.start.get() + buf.used.get();
    if end >= buf.klen.get() {
        end -= buf.klen.get();
    }
    let avail = buf.ulen.get() - buf.used.get();
    let count = (buf.klen.get() - end).min(avail);
    (buf.data.get().wrapping_add(end), count)
}

/// `audio_calc_sil`: compute a sample of silence in the user encoding.
fn audio_calc_sil(sc: &AudioSoftc) {
    let e = sc.sw_enc.get();
    let bps = (sc.bps.get() as usize).min(4);
    // AUDIO_DEBUG: the check for an unhandled encoding is not configured.
    let mut sil = [0u8; 4];
    let s0: u32 = if e == AUDIO_ENCODING_SLINEAR_LE || e == AUDIO_ENCODING_SLINEAR_BE {
        0
    } else {
        let shift = if sc.msb.get() != 0 {
            32u32.saturating_sub(8 * sc.bps.get())
        } else {
            32u32.saturating_sub(sc.bits.get())
        };
        0x8000_0000u32.checked_shr(shift).unwrap_or(0)
    };
    let mut s = s0;
    for i in 0..bps {
        // Big-endian encodings fill from the last byte backwards.
        let at = if e == AUDIO_ENCODING_SLINEAR_BE || e == AUDIO_ENCODING_ULINEAR_BE {
            bps - 1 - i
        } else {
            i
        };
        sil[at] = s as u8;
        s >>= 8;
    }
    if let Some(conv) = sc.conv_enc.get() {
        conv(&mut sil[..bps]);
    }
    sc.silence.set(sil);
}

/// `audio_fill_sil`: fill `buf` with silence, a whole number of samples.
fn audio_fill_sil(sc: &AudioSoftc, buf: &mut [u8]) {
    let bps = (sc.bps.get() as usize).clamp(1, 4);
    let sil = sc.silence.get();
    for q in buf.chunks_exact_mut(bps) {
        q.copy_from_slice(&sil[..bps]);
    }
}

/// `audio_clear`: empty the rings and fill them with silence.
fn audio_clear(sc: &AudioSoftc) {
    if sc.mode.get() & AUMODE_PLAY != 0 {
        sc.play.used.set(0);
        sc.play.start.set(0);
        sc.play.pos.set(0);
        sc.play.xrun.set(0);
        // SAFETY: DMA is stopped (or not yet started), the whole ring is ours.
        audio_fill_sil(sc, unsafe { sc.play.bytes(0, sc.play.klen.get()) });
    }
    if sc.mode.get() & AUMODE_RECORD != 0 {
        sc.rec.used.set(0);
        sc.rec.start.set(0);
        sc.rec.pos.set(0);
        sc.rec.xrun.set(0);
        // SAFETY: as above.
        audio_fill_sil(sc, unsafe { sc.rec.bytes(0, sc.rec.klen.get()) });
    }
}

/// `audio_pintr`: called whenever a block is consumed by the driver.
///
/// # Safety
///
/// `addr` is the argument audio(4) passed with this function to the driver's
/// `start_output` or `trigger_output` (its softc), and `AUDIO_LOCK` is held.
pub unsafe fn audio_pintr(addr: *mut c_void) {
    // SAFETY: the caller's contract: `addr` is a live audio softc.
    let sc = unsafe { &*addr.cast::<AudioSoftc>() };
    audio_pintr_sc(sc);
}

/// [`audio_pintr`] on the softc.
fn audio_pintr_sc(sc: &AudioSoftc) {
    mutex_assert_locked(&AUDIO_LOCK, "audio_pintr");
    let ops = sc.ops();

    if sc.mode.get() & AUMODE_PLAY == 0 || sc.active.get() == 0 {
        printf(format_args!(
            "{}: play interrupt but not playing\n",
            sc.devname()
        ));
        return;
    }
    if sc.quiesce.get() != 0 {
        return;
    }

    // check if record pointer wrapped, see explanation in audio_rintr()
    if sc.mode.get() & AUMODE_RECORD != 0 && ops.underrun.is_none() {
        sc.offs.set(sc.offs.get() - 1);
        let nblk = (sc.rec.klen.get() / sc.rec.blksz.get()) as i32;
        let mut todo = -sc.offs.get();
        if todo >= nblk {
            todo -= todo % nblk;
            while todo > 0 {
                todo -= 1;
                audio_rintr_sc(sc);
            }
        }
    }

    let blksz = sc.play.blksz.get();
    sc.play
        .pos
        .set(sc.play.pos.get().wrapping_add(blksz as u32));
    if ops.underrun.is_none() {
        // SAFETY: the block at `start` is the one the driver just consumed; with the
        // lock held no system call touches it.
        audio_fill_sil(sc, unsafe { sc.play.bytes(sc.play.start.get(), blksz) });
    }
    audio_buf_rdiscard(&sc.play, blksz);
    if sc.play.used.get() < blksz {
        sc.play
            .xrun
            .set(sc.play.xrun.get().wrapping_add(blksz as u32));
        audio_buf_wcommit(&sc.play, blksz);
        if let Some(f) = ops.underrun {
            // SAFETY: `sc.arg` is the handle the driver attached `sc.ops` with.
            unsafe { f(sc.arg.get()) };
        }
    }

    if ops.trigger_output.is_none() {
        let (p, _count) = audio_buf_rgetblk(&sc.play);
        let start_output = required(ops.start_output, "start_output");
        // SAFETY: as above; `p` points at a block of the ring, which outlives DMA.
        let error =
            unsafe { start_output(sc.arg.get(), p, blksz as i32, audio_pintr, sc.as_arg()) };
        if let Err(e) = error {
            printf(format_args!(
                "{}: play restart failed: {}\n",
                sc.devname(),
                e as i32
            ));
        }
    }

    if sc.play.used.get() < sc.play.ulen.get() {
        audio_buf_wakeup(&sc.play);
    }
}

/// `audio_rintr`: called whenever a block is produced by the driver.
///
/// # Safety
///
/// `addr` is the argument audio(4) passed with this function to the driver's
/// `start_input` or `trigger_input` (its softc), and `AUDIO_LOCK` is held.
pub unsafe fn audio_rintr(addr: *mut c_void) {
    // SAFETY: the caller's contract: `addr` is a live audio softc.
    let sc = unsafe { &*addr.cast::<AudioSoftc>() };
    audio_rintr_sc(sc);
}

/// [`audio_rintr`] on the softc.
fn audio_rintr_sc(sc: &AudioSoftc) {
    mutex_assert_locked(&AUDIO_LOCK, "audio_rintr");
    let ops = sc.ops();

    if sc.mode.get() & AUMODE_RECORD == 0 || sc.active.get() == 0 {
        printf(format_args!(
            "{}: rec interrupt but not recording\n",
            sc.devname()
        ));
        return;
    }
    if sc.quiesce.get() != 0 {
        return;
    }

    // Interrupts may be masked by other sub-systems during 320ms and more. During such a
    // delay the hardware doesn't stop playing and the play buffer pointers may wrap, this
    // can't be detected and corrected by low level drivers. This makes the record stream
    // ahead of the play stream; this is detected as a hardware anomaly by userland and
    // cause programs to misbehave.
    //
    // We fix this by advancing play position by an integer count of full buffers, so it
    // reaches the record position.
    if sc.mode.get() & AUMODE_PLAY != 0 && ops.underrun.is_none() {
        sc.offs.set(sc.offs.get() + 1);
        let nblk = (sc.play.klen.get() / sc.play.blksz.get()) as i32;
        let mut todo = sc.offs.get();
        if todo >= nblk {
            todo -= todo % nblk;
            while todo > 0 {
                todo -= 1;
                audio_pintr_sc(sc);
            }
        }
    }

    let blksz = sc.rec.blksz.get();
    sc.rec.pos.set(sc.rec.pos.get().wrapping_add(blksz as u32));
    if (sc.record_enable.get() == MIXER_RECORD_ENABLE_SYSCTL
        && AUDIO_RECORD_ENABLE.load(Ordering::Relaxed) == 0)
        || sc.record_enable.get() == MIXER_RECORD_ENABLE_OFF
    {
        let (p, _count) = audio_buf_wgetblk(&sc.rec);
        let off = sc.rec.offset_of(p);
        // SAFETY: the block the driver just produced; with the lock held no system call
        // touches it.
        audio_fill_sil(sc, unsafe { sc.rec.bytes(off, blksz) });
    }
    audio_buf_wcommit(&sc.rec, blksz);
    if sc.rec.used.get() > sc.rec.ulen.get() - blksz {
        sc.rec
            .xrun
            .set(sc.rec.xrun.get().wrapping_add(blksz as u32));
        audio_buf_rdiscard(&sc.rec, blksz);
    }

    if ops.trigger_input.is_none() {
        let (p, _count) = audio_buf_wgetblk(&sc.rec);
        let start_input = required(ops.start_input, "start_input");
        // SAFETY: `sc.arg` is the handle the driver attached `sc.ops` with; `p` points at
        // a block of the ring, which outlives DMA.
        let error = unsafe { start_input(sc.arg.get(), p, blksz as i32, audio_rintr, sc.as_arg()) };
        if let Err(e) = error {
            printf(format_args!(
                "{}: rec restart failed: {}\n",
                sc.devname(),
                e as i32
            ));
        }
    }

    if sc.rec.used.get() > 0 {
        audio_buf_wakeup(&sc.rec);
    }
}

/// `audio_start_do`: start DMA in the open directions.
fn audio_start_do(sc: &AudioSoftc) -> Result<(), Errno> {
    let ops = sc.ops();
    let mut error = Ok(());

    sc.offs.set(0);
    if sc.mode.get() & AUMODE_PLAY != 0 {
        if let Some(trigger_output) = ops.trigger_output {
            let p = AudioParams {
                encoding: sc.hw_enc.get(),
                precision: sc.bits.get(),
                bps: sc.bps.get(),
                msb: sc.msb.get(),
                sample_rate: u64::from(sc.rate.get()),
                channels: sc.pchan.get(),
            };
            let start = sc.play.data.get();
            // SAFETY: `sc.arg` is the handle the driver attached `sc.ops` with; the ring
            // `[start, start + klen)` outlives DMA (freed only after `halt_output`).
            error = unsafe {
                trigger_output(
                    sc.arg.get(),
                    start,
                    start.wrapping_add(sc.play.klen.get()),
                    sc.play.blksz.get() as i32,
                    audio_pintr,
                    sc.as_arg(),
                    &p,
                )
            };
        } else {
            mtx_enter(&AUDIO_LOCK);
            let (p, _count) = audio_buf_rgetblk(&sc.play);
            let start_output = required(ops.start_output, "start_output");
            // SAFETY: as above.
            error = unsafe {
                start_output(
                    sc.arg.get(),
                    p,
                    sc.play.blksz.get() as i32,
                    audio_pintr,
                    sc.as_arg(),
                )
            };
            mtx_leave(&AUDIO_LOCK);
        }
        if error.is_err() {
            printf(format_args!("{}: failed to start playback\n", sc.devname()));
        }
    }
    if sc.mode.get() & AUMODE_RECORD != 0 {
        if let Some(trigger_input) = ops.trigger_input {
            let p = AudioParams {
                encoding: sc.hw_enc.get(),
                precision: sc.bits.get(),
                bps: sc.bps.get(),
                msb: sc.msb.get(),
                sample_rate: u64::from(sc.rate.get()),
                channels: sc.rchan.get(),
            };
            let start = sc.rec.data.get();
            // SAFETY: as above, for the record ring.
            error = unsafe {
                trigger_input(
                    sc.arg.get(),
                    start,
                    start.wrapping_add(sc.rec.klen.get()),
                    sc.rec.blksz.get() as i32,
                    audio_rintr,
                    sc.as_arg(),
                    &p,
                )
            };
        } else {
            mtx_enter(&AUDIO_LOCK);
            let (p, _count) = audio_buf_wgetblk(&sc.rec);
            let start_input = required(ops.start_input, "start_input");
            // SAFETY: as above.
            error = unsafe {
                start_input(
                    sc.arg.get(),
                    p,
                    sc.rec.blksz.get() as i32,
                    audio_rintr,
                    sc.as_arg(),
                )
            };
            mtx_leave(&AUDIO_LOCK);
        }
        if error.is_err() {
            printf(format_args!(
                "{}: failed to start recording\n",
                sc.devname()
            ));
        }
    }
    error
}

/// `audio_stop_do`: halt DMA in the open directions.
fn audio_stop_do(sc: &AudioSoftc) -> Result<(), Errno> {
    let ops = sc.ops();
    // The C ignores what the halt methods return.
    if sc.mode.get() & AUMODE_PLAY != 0 {
        // SAFETY: `sc.arg` is the handle the driver attached `sc.ops` with.
        let _ = unsafe { required(ops.halt_output, "halt_output")(sc.arg.get()) };
    }
    if sc.mode.get() & AUMODE_RECORD != 0 {
        // SAFETY: as above.
        let _ = unsafe { required(ops.halt_input, "halt_input")(sc.arg.get()) };
    }
    Ok(())
}

/// `audio_start`.
fn audio_start(sc: &AudioSoftc) -> Result<(), Errno> {
    sc.active.set(1);
    sc.play.xrun.set(0);
    sc.play.pos.set(0);
    sc.rec.xrun.set(0);
    sc.rec.pos.set(0);
    audio_start_do(sc)
}

/// `audio_stop`.
fn audio_stop(sc: &AudioSoftc) -> Result<(), Errno> {
    audio_stop_do(sc)?;
    audio_clear(sc);
    sc.active.set(0);
    Ok(())
}

/// `audio_canstart`: whether `read`/`write` may start DMA by themselves.
fn audio_canstart(sc: &AudioSoftc) -> bool {
    if sc.active.get() != 0 || sc.pause.get() != 0 {
        return false;
    }
    if sc.mode.get() & AUMODE_RECORD != 0 && sc.rec.used.get() != 0 {
        return false;
    }
    if sc.mode.get() & AUMODE_PLAY != 0 && sc.play.used.get() != sc.play.ulen.get() {
        return false;
    }
    true
}

/// `audio_setpar_blksz`: choose the block size (`sc->round`, in frames).
fn audio_setpar_blksz(
    sc: &AudioSoftc,
    p: &mut AudioParams,
    r: &mut AudioParams,
) -> Result<(), Errno> {
    let ops = sc.ops();
    let mode = sc.mode.get();

    if let Some(set_blksz) = ops.set_blksz {
        // Don't allow block size of exceed half the buffer size
        if mode & AUMODE_PLAY != 0 {
            let max = (sc.play.datalen.get() / 2 / (sc.pchan.get() * sc.bps.get()) as usize) as u32;
            if sc.round.get() > max {
                sc.round.set(max);
            }
        }
        if mode & AUMODE_RECORD != 0 {
            let max = (sc.rec.datalen.get() / 2 / (sc.rchan.get() * sc.bps.get()) as usize) as u32;
            if sc.round.get() > max {
                sc.round.set(max);
            }
        }

        // SAFETY: `sc.arg` is the handle the driver attached `sc.ops` with.
        sc.round
            .set(unsafe { set_blksz(sc.arg.get(), mode, p, r, sc.round.get()) });
        return Ok(());
    }

    // get least multiplier of the number of frames per block
    let blk_mult: u32 = match ops.round_blocksize {
        Some(round_blocksize) => {
            // SAFETY: as above.
            let m = unsafe { round_blocksize(sc.arg.get(), 1) } as u32;
            if m == 0 {
                printf(format_args!(
                    "{}: 0x{:x}: bad block size multiplier\n",
                    sc.devname(),
                    m
                ));
                return Err(Errno::ENODEV);
            }
            m
        }
        None => 1,
    };
    // The C leaves the other direction's multiplier unset; `mode` is never 0 here.
    let mut np: u32 = 1;
    let mut nr: u32 = 1;
    if mode & AUMODE_PLAY != 0 {
        np = blk_mult / audio_gcd(sc.pchan.get() * sc.bps.get(), blk_mult);
        if mode & AUMODE_RECORD == 0 {
            nr = np;
        }
    }
    if mode & AUMODE_RECORD != 0 {
        nr = blk_mult / audio_gcd(sc.rchan.get() * sc.bps.get(), blk_mult);
        if mode & AUMODE_PLAY == 0 {
            np = nr;
        }
    }
    let mult = nr * np / audio_gcd(nr, np);

    // get minimum and maximum frames per block
    let mut blk_max: u32 = match ops.round_blocksize {
        Some(round_blocksize) => {
            // SAFETY: as above.
            let m = unsafe { round_blocksize(sc.arg.get(), AUDIO_BUFSZ as i32) };
            m as u32
        }
        None => AUDIO_BUFSZ as u32,
    };
    if mode & AUMODE_PLAY != 0 && blk_max as usize > sc.play.datalen.get() / 2 {
        blk_max = (sc.play.datalen.get() / 2) as u32;
    }
    if mode & AUMODE_RECORD != 0 && blk_max as usize > sc.rec.datalen.get() / 2 {
        blk_max = (sc.rec.datalen.get() / 2) as u32;
    }
    if mode & AUMODE_PLAY != 0 {
        np = blk_max / (sc.pchan.get() * sc.bps.get());
        if mode & AUMODE_RECORD == 0 {
            nr = np;
        }
    }
    if mode & AUMODE_RECORD != 0 {
        nr = blk_max / (sc.rchan.get() * sc.bps.get());
        if mode & AUMODE_PLAY == 0 {
            np = nr;
        }
    }
    let mut max = np.min(nr);
    max -= max % mult;
    let mut min = sc.rate.get() / 1000 + mult - 1;
    min -= min % mult;
    if max < min {
        printf(format_args!(
            "{}: {}: bad max frame number\n",
            sc.devname(),
            max
        ));
        return Err(Errno::EIO);
    }

    // adjust the frame per block to match our constraints
    let mut round = sc.round.get().wrapping_add(mult / 2);
    round -= round % mult;
    if round > max {
        round = max;
    } else if round < min {
        round = min;
    }
    sc.round.set(round);

    Ok(())
}

/// `audio_setpar_nblks`: set buffer size (number of blocks).
fn audio_setpar_nblks(
    sc: &AudioSoftc,
    p: &mut AudioParams,
    r: &mut AudioParams,
) -> Result<(), Errno> {
    let ops = sc.ops();
    let mode = sc.mode.get();

    if mode & AUMODE_PLAY != 0 {
        let max = (sc.play.datalen.get()
            / (sc.round.get() * sc.pchan.get() * sc.bps.get()) as usize) as u32;
        if sc.play.nblks.get() > max {
            sc.play.nblks.set(max);
        } else if sc.play.nblks.get() < 2 {
            sc.play.nblks.set(2);
        }
        if let Some(set_nblks) = ops.set_nblks {
            // SAFETY: `sc.arg` is the handle the driver attached `sc.ops` with.
            let n =
                unsafe { set_nblks(sc.arg.get(), mode, p, sc.round.get(), sc.play.nblks.get()) };
            sc.play.nblks.set(n);
        }
    }
    if mode & AUMODE_RECORD != 0 {
        // for recording, buffer size is not the latency (it's exactly one block), so
        // let's get the maximum buffer size of maximum reliability during xruns
        let mut max = (sc.rec.datalen.get()
            / (sc.round.get() * sc.rchan.get() * sc.bps.get()) as usize)
            as u32;
        if let Some(set_nblks) = ops.set_nblks {
            // SAFETY: as above.
            max = unsafe { set_nblks(sc.arg.get(), mode, r, sc.round.get(), max) };
        }
        sc.rec.nblks.set(max);
    }
    Ok(())
}

/// `audio_setpar`: negotiate the parameters in the softc with the hardware and size the
/// rings.
fn audio_setpar(sc: &AudioSoftc) -> Result<(), Errno> {
    let ops = sc.ops();
    let mode = sc.mode.get();

    // check if requested parameters are in the allowed ranges
    if mode & AUMODE_PLAY != 0 {
        sc.pchan.set(sc.pchan.get().clamp(1, 64));
    }
    if mode & AUMODE_RECORD != 0 {
        sc.rchan.set(sc.rchan.get().clamp(1, 64));
    }
    match sc.sw_enc.get() {
        AUDIO_ENCODING_ULAW
        | AUDIO_ENCODING_ALAW
        | AUDIO_ENCODING_SLINEAR_LE
        | AUDIO_ENCODING_SLINEAR_BE
        | AUDIO_ENCODING_ULINEAR_LE
        | AUDIO_ENCODING_ULINEAR_BE => {}
        _ => sc.sw_enc.set(AUDIO_ENCODING_SLINEAR_LE),
    }
    sc.bits.set(sc.bits.get().clamp(8, 32));
    sc.bps.set(sc.bps.get().clamp(1, 4));
    sc.rate.set(sc.rate.get().clamp(4000, 192000));

    // copy into struct audio_params, required by drivers
    let mut p = AudioParams {
        encoding: sc.sw_enc.get(),
        precision: sc.bits.get(),
        bps: sc.bps.get(),
        msb: sc.msb.get(),
        sample_rate: u64::from(sc.rate.get()),
        channels: sc.pchan.get(),
    };
    let mut r = AudioParams {
        channels: sc.rchan.get(),
        ..p
    };

    // set parameters
    // SAFETY: `sc.arg` is the handle the driver attached `sc.ops` with.
    unsafe { required(ops.set_params, "set_params")(sc.arg.get(), mode, mode, &mut p, &mut r)? };
    if mode == (AUMODE_PLAY | AUMODE_RECORD)
        && (p.encoding != r.encoding
            || p.precision != r.precision
            || p.bps != r.bps
            || p.msb != r.msb
            || p.sample_rate != r.sample_rate)
    {
        printf(format_args!(
            "{}: different play and record parameters returned by hardware\n",
            sc.devname()
        ));
        return Err(Errno::ENODEV);
    }
    if mode & AUMODE_PLAY != 0 {
        sc.hw_enc.set(p.encoding);
        sc.bits.set(p.precision);
        sc.bps.set(p.bps);
        sc.msb.set(p.msb);
        sc.rate.set(p.sample_rate as u32);
        sc.pchan.set(p.channels);
    }
    if mode & AUMODE_RECORD != 0 {
        sc.hw_enc.set(r.encoding);
        sc.bits.set(r.precision);
        sc.bps.set(r.bps);
        sc.msb.set(r.msb);
        sc.rate.set(r.sample_rate as u32);
        sc.rchan.set(r.channels);
    }
    if sc.rate.get() == 0 || sc.bps.get() == 0 || sc.bits.get() == 0 {
        printf(format_args!(
            "{}: invalid parameters returned by hardware\n",
            sc.devname()
        ));
        return Err(Errno::ENODEV);
    }
    if let Some(commit_settings) = ops.commit_settings {
        // SAFETY: as above.
        unsafe { commit_settings(sc.arg.get())? };
    }

    // conversion from/to exotic/dead encoding, for drivers not supporting linear
    match sc.hw_enc.get() {
        AUDIO_ENCODING_SLINEAR_LE
        | AUDIO_ENCODING_SLINEAR_BE
        | AUDIO_ENCODING_ULINEAR_LE
        | AUDIO_ENCODING_ULINEAR_BE => {
            sc.sw_enc.set(sc.hw_enc.get());
            sc.conv_dec.set(None);
            sc.conv_enc.set(None);
        }
        AUDIO_ENCODING_ULAW => {
            sc.sw_enc.set(if BYTE_ORDER == LITTLE_ENDIAN {
                AUDIO_ENCODING_SLINEAR_LE
            } else {
                AUDIO_ENCODING_SLINEAR_BE
            });
            if sc.bits.get() == 8 {
                sc.conv_enc.set(Some(slinear8_to_mulaw));
                sc.conv_dec.set(Some(mulaw_to_slinear8));
            } else if sc.bits.get() == 24 {
                sc.conv_enc.set(Some(slinear24_to_mulaw24));
                sc.conv_dec.set(Some(mulaw24_to_slinear24));
            } else {
                sc.sw_enc.set(sc.hw_enc.get());
                sc.conv_dec.set(None);
                sc.conv_enc.set(None);
            }
        }
        _ => {
            printf(format_args!(
                "{}: setpar: enc = {}, bits = {}: emulation skipped\n",
                sc.devname(),
                sc.hw_enc.get(),
                sc.bits.get()
            ));
            sc.sw_enc.set(sc.hw_enc.get());
            sc.conv_dec.set(None);
            sc.conv_enc.set(None);
        }
    }
    audio_calc_sil(sc);

    audio_setpar_blksz(sc, &mut p, &mut r)?;

    audio_setpar_nblks(sc, &mut p, &mut r)?;

    // set buffer
    if mode & AUMODE_PLAY != 0 {
        let blksz = (sc.round.get() * sc.pchan.get() * sc.bps.get()) as usize;
        sc.play.blksz.set(blksz);
        sc.play.ulen.set(sc.play.nblks.get() as usize * blksz);
        sc.play
            .klen
            .set(sc.play.datalen.get() - sc.play.datalen.get() % blksz);
    }
    if mode & AUMODE_RECORD != 0 {
        let blksz = (sc.round.get() * sc.rchan.get() * sc.bps.get()) as usize;
        sc.rec.blksz.set(blksz);
        sc.rec.ulen.set(sc.rec.nblks.get() as usize * blksz);
        sc.rec
            .klen
            .set(sc.rec.datalen.get() - sc.rec.datalen.get() % blksz);
    }

    Ok(())
}

/// `audio_ioc_start`: `AUDIO_START`.
fn audio_ioc_start(sc: &AudioSoftc) -> Result<(), Errno> {
    if sc.pause.get() == 0 {
        // can't start: already started
        return Err(Errno::EBUSY);
    }
    if sc.mode.get() & AUMODE_PLAY != 0 && sc.play.used.get() != sc.play.ulen.get() {
        // play buffer not ready
        return Err(Errno::EBUSY);
    }
    if sc.mode.get() & AUMODE_RECORD != 0 && sc.rec.used.get() != 0 {
        // record buffer not ready
        return Err(Errno::EBUSY);
    }
    sc.pause.set(0);
    audio_start(sc)
}

/// `audio_ioc_stop`: `AUDIO_STOP`.
fn audio_ioc_stop(sc: &AudioSoftc) -> Result<(), Errno> {
    if sc.pause.get() != 0 {
        // can't stop: not started
        return Err(Errno::EBUSY);
    }
    sc.pause.set(1);
    if sc.active.get() != 0 {
        return audio_stop(sc);
    }
    Ok(())
}

/// `audio_ioc_getpar`: `AUDIO_GETPAR`.
fn audio_ioc_getpar(sc: &AudioSoftc, p: &mut AudioSwpar) -> Result<(), Errno> {
    let enc = sc.sw_enc.get();
    p.rate = sc.rate.get();
    p.sig = u32::from(enc == AUDIO_ENCODING_SLINEAR_LE || enc == AUDIO_ENCODING_SLINEAR_BE);
    p.le = u32::from(enc == AUDIO_ENCODING_SLINEAR_LE || enc == AUDIO_ENCODING_ULINEAR_LE);
    p.bits = sc.bits.get();
    p.bps = sc.bps.get();
    p.msb = sc.msb.get();
    p.pchan = sc.pchan.get();
    p.rchan = sc.rchan.get();
    p.nblks = sc.play.nblks.get();
    p.round = sc.round.get();
    Ok(())
}

/// `audio_ioc_setpar`: `AUDIO_SETPAR`.
fn audio_ioc_setpar(sc: &AudioSoftc, p: &AudioSwpar) -> Result<(), Errno> {
    if sc.active.get() != 0 {
        // can't change params during dma
        return Err(Errno::EBUSY);
    }

    // copy desired parameters into the softc structure
    if p.sig != !0 || p.le != !0 || p.bits != !0 {
        let mut sig = 1;
        let mut le = u32::from(BYTE_ORDER == LITTLE_ENDIAN);
        sc.bits.set(16);
        sc.bps.set(2);
        sc.msb.set(1);
        if p.sig != !0 {
            sig = p.sig;
        }
        if p.le != !0 {
            le = p.le;
        }
        if p.bits != !0 {
            sc.bits.set(p.bits);
            sc.bps.set(if p.bits <= 8 {
                1
            } else if p.bits <= 16 {
                2
            } else {
                4
            });
            if p.bps != !0 {
                sc.bps.set(p.bps);
            }
            if p.msb != !0 {
                sc.msb.set(u32::from(p.msb != 0));
            }
        }
        sc.sw_enc.set(match (sig != 0, le != 0) {
            (true, true) => AUDIO_ENCODING_SLINEAR_LE,
            (true, false) => AUDIO_ENCODING_SLINEAR_BE,
            (false, true) => AUDIO_ENCODING_ULINEAR_LE,
            (false, false) => AUDIO_ENCODING_ULINEAR_BE,
        });
    }
    if p.rate != !0 {
        sc.rate.set(p.rate);
    }
    if p.pchan != !0 {
        sc.pchan.set(p.pchan);
    }
    if p.rchan != !0 {
        sc.rchan.set(p.rchan);
    }
    if p.round != !0 {
        sc.round.set(p.round);
    }
    if p.nblks != !0 {
        sc.play.nblks.set(p.nblks);
    }

    // if the device is not opened for playback or recording don't touch the hardware yet
    // (ex. if this is /dev/audioctlN)
    if sc.mode.get() == 0 {
        return Ok(());
    }

    // negotiate parameters with the hardware
    audio_setpar(sc)?;
    audio_clear(sc);
    let ops = sc.ops();
    if sc.mode.get() & AUMODE_PLAY != 0
        && let Some(init_output) = ops.init_output
    {
        let (data, klen) = (sc.play.data.get(), sc.play.klen.get() as i32);
        // SAFETY: `sc.arg` is the handle the driver attached `sc.ops` with; the ring
        // outlives the driver's use of it.
        unsafe { init_output(sc.arg.get(), data, klen)? };
    }
    if sc.mode.get() & AUMODE_RECORD != 0
        && let Some(init_input) = ops.init_input
    {
        let (data, klen) = (sc.rec.data.get(), sc.rec.klen.get() as i32);
        // SAFETY: as above.
        unsafe { init_input(sc.arg.get(), data, klen)? };
    }
    Ok(())
}

/// `audio_ioc_getstatus`: `AUDIO_GETSTATUS`.
fn audio_ioc_getstatus(sc: &AudioSoftc, p: &mut AudioStatus) -> Result<(), Errno> {
    p.mode = sc.mode.get();
    p.pause = sc.pause.get();
    p.active = sc.active.get();
    Ok(())
}

/// `audio_match`: an `audio_attach_args` of type audio.
pub fn audio_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: audio's parents hand it the `struct audio_attach_args` of `audio_attach_mi`.
    let sa = unsafe { &*aux.cast::<AudioAttachArgs>() };

    i32::from(sa.type_ == AUDIODEV_TYPE_AUDIO)
}

/// `audio_attach`: take the driver's table, allocate the rings, set the default
/// parameters and save the mixer's controls.
pub fn audio_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `audio_ca` makes `AudioSoftc`s.
    let sc = unsafe { self_.softc::<AudioSoftc>() };
    // SAFETY: as in `audio_match`.
    let sa = unsafe { &*aux.cast::<AudioAttachArgs>() };
    // SAFETY: an `AUDIODEV_TYPE_AUDIO` attachment's `hwif` is the driver's `static`
    // `audio_hw_if` (`audio_attach_mi`), or NULL.
    let ops: Option<&'static AudioHwIf> = unsafe { sa.hwif.cast::<AudioHwIf>().as_ref() };
    let arg = sa.hdl;

    printf(format_args!("\n"));

    #[cfg(feature = "diagnostic")]
    if ops.is_none_or(|o| {
        o.open.is_none()
            || o.close.is_none()
            || o.set_params.is_none()
            || (o.start_output.is_none() && o.trigger_output.is_none())
            || (o.start_input.is_none() && o.trigger_input.is_none())
            || o.halt_output.is_none()
            || o.halt_input.is_none()
            || o.set_port.is_none()
            || o.get_port.is_none()
            || o.query_devinfo.is_none()
    }) {
        printf(format_args!("{}: missing method\n", sc.devname()));
        sc.ops.set(None);
        return;
    }
    sc.ops.set(ops);
    sc.cookie.set(sa.cookie);
    sc.arg.set(arg);

    wskbd_mixer_init(sc);

    if audio_buf_init(sc, &sc.play, AUMODE_PLAY).is_err() {
        sc.ops.set(None);
        printf(format_args!(
            "{}: could not allocate play buffer\n",
            sc.devname()
        ));
        return;
    }
    if audio_buf_init(sc, &sc.rec, AUMODE_RECORD).is_err() {
        audio_buf_done(sc, &sc.play);
        sc.ops.set(None);
        printf(format_args!(
            "{}: could not allocate record buffer\n",
            sc.devname()
        ));
        return;
    }

    // SAFETY: `AUDIO_LOCK` is a static, which outlives every klist.
    unsafe { klist_init_mutex(&sc.mix_klist, &AUDIO_LOCK) };

    // set defaults
    sc.sw_enc.set(if BYTE_ORDER == LITTLE_ENDIAN {
        AUDIO_ENCODING_SLINEAR_LE
    } else {
        AUDIO_ENCODING_SLINEAR_BE
    });
    sc.bits.set(16);
    sc.bps.set(2);
    sc.msb.set(1);
    sc.rate.set(48000);
    sc.pchan.set(2);
    sc.rchan.set(2);
    sc.round.set(960);
    sc.play.nblks.set(2);
    sc.play.pos.set(0);
    sc.play.xrun.set(0);
    sc.rec.pos.set(0);
    sc.rec.xrun.set(0);
    sc.record_enable.set(MIXER_RECORD_ENABLE_SYSCTL);

    // allocate an array of mixer_ctrl structures to save the mixer state and prefill them.
    let Some(mi) = devinfo_alloc() else {
        return;
    };
    // SAFETY: `devinfo_alloc` returns a zeroed, exclusively owned `MixerDevinfo`.
    let mi_ref = unsafe { &mut *mi.as_ptr() };
    let query_devinfo = required(sc.ops().query_devinfo, "query_devinfo");

    mi_ref.index = 0;
    // SAFETY: `sc.arg` is the handle the driver attached `sc.ops` with.
    while unsafe { query_devinfo(sc.arg.get(), mi_ref) }.is_ok() {
        mi_ref.index += 1;
    }
    let nent = mi_ref.index;
    sc.mix_nent.set(nent);
    let n = nent as usize;
    let ents = mallocarray(n, size_of::<MixerCtrl>(), M_DEVBUF, M_WAITOK | M_ZERO);
    sc.mix_ents
        .set(ents.map_or(ptr::null_mut(), |p| p.as_ptr().cast::<MixerCtrl>()));
    let evbuf = mallocarray(n, size_of::<MixerEv>(), M_DEVBUF, M_WAITOK | M_ZERO);
    sc.mix_evbuf
        .set(evbuf.map_or(ptr::null_mut(), |p| p.as_ptr().cast::<MixerEv>()));

    // SAFETY: attach runs alone.
    let ents = unsafe { sc.mix_ents() };
    mi_ref.index = 0;
    let mut i = 0;
    // SAFETY: as above.
    while unsafe { query_devinfo(sc.arg.get(), mi_ref) }.is_ok() {
        // The driver's controls do not change; a driver that grew one is not saved.
        if let Some(ent) = ents.get_mut(i) {
            match mi_ref.type_ {
                AUDIO_MIXER_VALUE | AUDIO_MIXER_SET | AUDIO_MIXER_ENUM => {
                    if mi_ref.type_ == AUDIO_MIXER_VALUE {
                        ent.un.value_mut().num_channels = mi_ref.un.v().num_channels;
                    }
                    ent.dev = mi_ref.index;
                    ent.type_ = mi_ref.type_;
                }
                _ => {}
            }
        }
        mi_ref.index += 1;
        i += 1;
    }

    devinfo_free(mi);
}

/// `malloc(sizeof(struct mixer_devinfo), M_TEMP, M_WAITOK)`, zeroed: the C keeps the
/// 812-byte structure off the kernel stack.
fn devinfo_alloc() -> Option<NonNull<MixerDevinfo>> {
    let p = malloc(size_of::<MixerDevinfo>(), M_TEMP, M_WAITOK | M_ZERO)?;
    Some(p.cast::<MixerDevinfo>())
}

/// `free(mi, M_TEMP, sizeof(struct mixer_devinfo))`.
fn devinfo_free(mi: NonNull<MixerDevinfo>) {
    free(mi.cast(), M_TEMP, size_of::<MixerDevinfo>());
}

/// `audio_activate`: quiesce (save the mixer, stop DMA) and wake up (restore and restart).
pub fn audio_activate(self_: &Device, act: i32) -> Result<(), Errno> {
    // SAFETY: `audio_ca` makes `AudioSoftc`s.
    let sc = unsafe { self_.softc::<AudioSoftc>() };
    let ops = sc.ops();

    match act {
        DVACT_QUIESCE => {
            // good drivers run play and rec handlers in a single interrupt. Grab the lock
            // to ensure we expose the same sc->quiesce value to both play and rec handlers
            mtx_enter(&AUDIO_LOCK);
            sc.quiesce.set(1);
            mtx_leave(&AUDIO_LOCK);

            // once sc->quiesce is set, interrupts may occur, but counters are not advanced
            // and consequently processes keep sleeping.
            //
            // XXX: ensure read/write/ioctl don't start/stop DMA at the same time, this
            // needs a "ready" condvar
            if sc.mode.get() != 0 && sc.active.get() != 0 {
                let _ = audio_stop_do(sc);
            }

            // save mixer state
            let get_port = required(ops.get_port, "get_port");
            // SAFETY: suspend runs alone (the caller ensures it).
            for ent in unsafe { sc.mix_ents() }.iter_mut() {
                // SAFETY: `sc.arg` is the handle the driver attached `sc.ops` with.
                let _ = unsafe { get_port(sc.arg.get(), ent) };
            }
        }
        DVACT_WAKEUP => {
            // restore mixer state
            let set_port = required(ops.set_port, "set_port");
            // SAFETY: resume runs alone (the caller ensures it).
            for ent in unsafe { sc.mix_ents() }.iter_mut() {
                // SAFETY: as above.
                let _ = unsafe { set_port(sc.arg.get(), ent) };
            }

            // keep buffer usage the same, but set start pointer to the beginning of the
            // buffer.
            //
            // No need to grab the audio_lock as DMA is stopped and this is the only thread
            // running (caller ensures this)
            sc.quiesce.set(0);
            wakeup(ptr::from_ref(&sc.quiesce));

            if sc.mode.get() != 0 {
                if audio_setpar(sc).is_err() {
                    return Ok(());
                }
                if sc.mode.get() & AUMODE_PLAY != 0 {
                    sc.play.start.set(0);
                    // SAFETY: DMA is stopped, the whole ring is ours.
                    audio_fill_sil(sc, unsafe { sc.play.bytes(0, sc.play.klen.get()) });
                }
                if sc.mode.get() & AUMODE_RECORD != 0 {
                    sc.rec.start.set(sc.rec.ulen.get() - sc.rec.used.get());
                    // SAFETY: as above.
                    audio_fill_sil(sc, unsafe { sc.rec.bytes(0, sc.rec.klen.get()) });
                }
                if sc.active.get() != 0 {
                    let _ = audio_start_do(sc);
                }
            }
        }
        _ => {}
    }
    Ok(())
}

/// `audio_detach`: revoke the open instances, halt DMA, wake the sleepers and free.
pub fn audio_detach(self_: &Device, _flags: i32) -> Result<(), Errno> {
    // SAFETY: `audio_ca` makes `AudioSoftc`s.
    let sc = unsafe { self_.softc::<AudioSoftc>() };

    wakeup(ptr::from_ref(&sc.quiesce));

    // locate the major number
    let maj = (0..nchrdev())
        .find(|&maj| ptr::fn_addr_eq(cdevsw(maj).d_open, audioopen as DevTypeOpen))
        .unwrap_or(nchrdev());
    // Nuke the vnodes for any open instances, calls close but as close uses
    // device_lookup, it returns EXIO and does nothing
    let mn = self_.dv_unit.get() as u32;
    vdevgone(maj, mn | AUDIO_DEV_AUDIO, mn | AUDIO_DEV_AUDIO, VCHR);
    vdevgone(maj, mn | AUDIO_DEV_AUDIOCTL, mn | AUDIO_DEV_AUDIOCTL, VCHR);

    // The close() method did nothing, quickly halt DMA (normally parent is already gone,
    // and code below is no-op), and wake-up user-land blocked in read/write/ioctl, which
    // return EIO.
    if sc.mode.get() != 0 {
        if sc.active.get() != 0 {
            wakeup(ptr::from_ref(&sc.play.blocking));
            wakeup(ptr::from_ref(&sc.rec.blocking));
            let _ = audio_stop(sc);
        }
        // SAFETY: `sc.arg` is the handle the driver attached `sc.ops` with.
        unsafe { required(sc.ops().close, "close")(sc.arg.get()) };
        sc.mode.set(0);
    }
    if sc.mix_isopen.get() != 0 {
        wakeup(ptr::from_ref(&sc.mix_blocking));
    }
    klist_invalidate(&sc.play.klist);
    klist_invalidate(&sc.rec.klist);
    klist_invalidate(&sc.mix_klist);

    // free resources
    klist_free(&sc.mix_klist);
    let n = usize::try_from(sc.mix_nent.get()).unwrap_or(0);
    if let Some(p) = NonNull::new(sc.mix_evbuf.get()) {
        free(p.cast(), M_DEVBUF, n * size_of::<MixerEv>());
        sc.mix_evbuf.set(ptr::null_mut());
    }
    if let Some(p) = NonNull::new(sc.mix_ents.get()) {
        free(p.cast(), M_DEVBUF, n * size_of::<MixerCtrl>());
        sc.mix_ents.set(ptr::null_mut());
    }
    audio_buf_done(sc, &sc.play);
    audio_buf_done(sc, &sc.rec);
    Ok(())
}

/// `audio_submatch`: only `audio` attaches through `audio_attach_mi`.
pub fn audio_submatch(_parent: Option<&Device>, match_: &CfMatch, _aux: *mut c_void) -> i32 {
    i32::from(ptr::eq(match_.cfdata().cf_driver, &AUDIO_CD))
}

/// `audio_attach_mi`: attach this driver to the caller (hardware driver); this checks the
/// kernel config and possibly calls `audio_attach()`.
///
/// `arg` is the handle every `ops` method gets first; `cookie` names the device for the
/// keyboard's volume keys (wskbd(4)), NULL for none.
pub fn audio_attach_mi(
    ops: &'static AudioHwIf,
    arg: *mut c_void,
    cookie: *mut c_void,
    dev: &Device,
) -> Option<NonNull<Device>> {
    let mut aa = AudioAttachArgs {
        type_: AUDIODEV_TYPE_AUDIO,
        hwif: ptr::from_ref(ops).cast(),
        hdl: arg,
        cookie,
    };

    config_found_sm(
        dev,
        ptr::from_mut(&mut aa).cast(),
        Some(audioprint),
        Some(audio_submatch),
    )
}

/// `audioprint`: the "not configured" line of an audio child.
pub fn audioprint(aux: *mut c_void, pnp: Option<&[u8]>) -> i32 {
    // SAFETY: `config_found_sm` hands back the `struct audio_attach_args` it was given.
    let arg = unsafe { &*aux.cast::<AudioAttachArgs>() };

    if let Some(pnp) = pnp {
        let type_ = match arg.type_ {
            AUDIODEV_TYPE_AUDIO => "audio",
            AUDIODEV_TYPE_OPL => "opl",
            AUDIODEV_TYPE_MPU => "mpu",
            t => panic(format_args!("audioprint: unknown type {t}")),
        };
        printf(format_args!("{} at {}", type_, Str(pnp)));
    }
    UNCONF
}

/// `audio_open`: open `/dev/audioN` for the directions `flags` asks.
fn audio_open(sc: &AudioSoftc, flags: i32) -> Result<(), Errno> {
    if sc.mode.get() != 0 {
        return Err(Errno::EBUSY);
    }
    let ops = sc.ops();
    // SAFETY: `sc.arg` is the handle the driver attached `sc.ops` with.
    unsafe { required(ops.open, "open")(sc.arg.get(), flags)? };
    sc.active.set(0);
    sc.pause.set(1);
    sc.rec.blocking.set(0);
    sc.play.blocking.set(0);
    sc.mode.set(0);
    if flags & FWRITE != 0 {
        sc.mode.set(sc.mode.get() | AUMODE_PLAY);
    }
    if flags & FREAD != 0 {
        sc.mode.set(sc.mode.get() | AUMODE_RECORD);
    }

    if let Err(e) = audio_setpar(sc) {
        // SAFETY: as above.
        unsafe { required(ops.close, "close")(sc.arg.get()) };
        sc.mode.set(0);
        return Err(e);
    }
    audio_clear(sc);

    // allow read(2)/write(2) to automatically start DMA, without the need for ioctl(), to
    // make /dev/audio usable in scripts
    sc.pause.set(0);
    Ok(())
}

/// `audio_drain`: play what is in the buffer, padded with silence, before close.
fn audio_drain(sc: &AudioSoftc) -> Result<(), Errno> {
    if sc.mode.get() & AUMODE_PLAY == 0 || sc.pause.get() != 0 {
        return Ok(());
    }

    // discard partial samples, required by audio_fill_sil()
    mtx_enter(&AUDIO_LOCK);
    let bpf = (sc.pchan.get() * sc.bps.get()) as usize;
    sc.play
        .used
        .set(sc.play.used.get() - sc.play.used.get() % bpf);
    if sc.play.used.get() == 0 {
        mtx_leave(&AUDIO_LOCK);
        return Ok(());
    }

    if sc.active.get() == 0 {
        // dma not started yet because buffer was not full enough to start automatically.
        // Pad it and start now.
        loop {
            let (p, count) = audio_buf_wgetblk(&sc.play);
            if count == 0 {
                break;
            }
            let off = sc.play.offset_of(p);
            // SAFETY: the free part of the ring, which `wgetblk` handed out under the lock
            // and DMA is not running.
            audio_fill_sil(sc, unsafe { sc.play.bytes(off, count) });
            audio_buf_wcommit(&sc.play, count);
        }
        mtx_leave(&AUDIO_LOCK);
        audio_start(sc)?;
        mtx_enter(&AUDIO_LOCK);
    }

    let xrun = sc.play.xrun.get();
    let mut error = Ok(());
    while sc.play.xrun.get() == xrun {
        // set a 5 second timeout, in case interrupts don't work, useful only for
        // debugging drivers
        sc.play.blocking.set(1);
        error = msleep_nsec(
            ptr::from_ref(&sc.play.blocking),
            &AUDIO_LOCK,
            PWAIT | PCATCH,
            "au_dr",
            sec_to_nsec(5),
        );
        if sc.dev.dv_flags.get() & DVF_ACTIVE == 0 {
            error = Err(Errno::EIO);
        }
        if error.is_err() {
            break;
        }
    }
    mtx_leave(&AUDIO_LOCK);
    error
}

/// `audio_close`.
fn audio_close(sc: &AudioSoftc) -> Result<(), Errno> {
    let _ = audio_drain(sc);
    if sc.active.get() != 0 {
        let _ = audio_stop(sc);
    }
    // SAFETY: `sc.arg` is the handle the driver attached `sc.ops` with.
    unsafe { required(sc.ops().close, "close")(sc.arg.get()) };
    sc.mode.set(0);
    Ok(())
}

/// `audio_read`: copy recorded data out, sleeping until some is there.
fn audio_read(sc: &AudioSoftc, uio: &mut Uio<'_>, ioflag: i32) -> Result<(), Errno> {
    // block if quiesced
    while sc.quiesce.get() != 0 {
        let _ = tsleep_nsec(ptr::from_ref(&sc.quiesce), 0, "au_qrd", INFSLP);
    }

    // start automatically if audio_ioc_start() was never called
    if audio_canstart(sc) {
        audio_start(sc)?;
    }

    mtx_enter(&AUDIO_LOCK);

    // if there is no data then sleep
    while sc.rec.used.get() == 0 {
        if ioflag & IO_NDELAY != 0 {
            mtx_leave(&AUDIO_LOCK);
            return Err(Errno::EWOULDBLOCK);
        }
        sc.rec.blocking.set(1);
        let mut error = msleep_nsec(
            ptr::from_ref(&sc.rec.blocking),
            &AUDIO_LOCK,
            PWAIT | PCATCH,
            "au_rd",
            INFSLP,
        );
        if sc.dev.dv_flags.get() & DVF_ACTIVE == 0 {
            error = Err(Errno::EIO);
        }
        if let Err(e) = error {
            mtx_leave(&AUDIO_LOCK);
            return Err(e);
        }
    }

    // at this stage, there is data to transfer
    while uio.uio_resid > 0 && sc.rec.used.get() > 0 {
        let (p, count) = audio_buf_rgetblk(&sc.rec);
        let count = count.min(uio.uio_resid);
        let off = sc.rec.offset_of(p);
        mtx_leave(&AUDIO_LOCK);
        // SAFETY: the recorded bytes `rgetblk` handed out under the lock; the interrupt
        // writes past them and does not discard them while `used` covers them (only an
        // overrun does, as in C).
        let data = unsafe { sc.rec.bytes(off, count) };
        if let Some(conv) = sc.conv_dec.get() {
            conv(data);
        }
        uiomove(data, uio)?;
        mtx_enter(&AUDIO_LOCK);
        audio_buf_rdiscard(&sc.rec, count);
    }
    mtx_leave(&AUDIO_LOCK);
    Ok(())
}

/// `audio_write`: copy data to play in, sleeping while the buffer is full.
fn audio_write(sc: &AudioSoftc, uio: &mut Uio<'_>, ioflag: i32) -> Result<(), Errno> {
    let ops = sc.ops();

    // block if quiesced
    while sc.quiesce.get() != 0 {
        let _ = tsleep_nsec(ptr::from_ref(&sc.quiesce), 0, "au_qwr", INFSLP);
    }

    // if IO_NDELAY flag is set then check if there is enough room in the buffer to store at
    // least one byte. If not then don't start the write process.
    mtx_enter(&AUDIO_LOCK);
    if uio.uio_resid > 0 && ioflag & IO_NDELAY != 0 && sc.play.used.get() == sc.play.ulen.get() {
        mtx_leave(&AUDIO_LOCK);
        return Err(Errno::EWOULDBLOCK);
    }

    while uio.uio_resid > 0 {
        let (p, count) = loop {
            let (p, count) = audio_buf_wgetblk(&sc.play);
            if count > 0 {
                break (p, count);
            }
            if ioflag & IO_NDELAY != 0 {
                // At this stage at least one byte is already moved so we do not return
                // EWOULDBLOCK
                mtx_leave(&AUDIO_LOCK);
                return Ok(());
            }
            sc.play.blocking.set(1);
            let mut error = msleep_nsec(
                ptr::from_ref(&sc.play.blocking),
                &AUDIO_LOCK,
                PWAIT | PCATCH,
                "au_wr",
                INFSLP,
            );
            if sc.dev.dv_flags.get() & DVF_ACTIVE == 0 {
                error = Err(Errno::EIO);
            }
            if let Err(e) = error {
                mtx_leave(&AUDIO_LOCK);
                return Err(e);
            }
        };
        let count = count.min(uio.uio_resid);
        let off = sc.play.offset_of(p);
        mtx_leave(&AUDIO_LOCK);
        // SAFETY: the free bytes `wgetblk` handed out under the lock; DMA plays only the
        // committed part of the ring before them.
        let data = unsafe { sc.play.bytes(off, count) };
        if uiomove(data, uio).is_err() {
            // The C returns 0 here: the bytes already moved count.
            return Ok(());
        }
        if let Some(conv) = sc.conv_enc.get() {
            conv(data);
        }
        if let Some(copy_output) = ops.copy_output {
            // SAFETY: `sc.arg` is the handle the driver attached `sc.ops` with.
            unsafe { copy_output(sc.arg.get(), count) };
        }

        mtx_enter(&AUDIO_LOCK);
        audio_buf_wcommit(&sc.play, count);

        // start automatically if audio_ioc_start() was never called
        if audio_canstart(sc) {
            mtx_leave(&AUDIO_LOCK);
            audio_start(sc)?;
            mtx_enter(&AUDIO_LOCK);
        }
    }
    mtx_leave(&AUDIO_LOCK);
    Ok(())
}

/// `audio_getdev`: `AUDIO_GETDEV`, the driver's display name or the parent's name.
fn audio_getdev(sc: &AudioSoftc, p: &mut AudioDevice) -> Result<(), Errno> {
    *p = AudioDevice::default();
    let mut sz = 0;

    if let Some(display_name) = sc.ops().display_name {
        // SAFETY: `sc.arg` is the handle the driver attached `sc.ops` with.
        sz = unsafe { display_name(sc.arg.get(), &mut p.name) };
    }

    if sz == 0 {
        let Some(parent) = sc.dev.parent() else {
            return Err(Errno::EIO);
        };
        strlcpy(&mut p.name, &parent.dv_xname.get());
    }

    Ok(())
}

/// Runs `f` on the ioctl argument `data` as a `T`: in place when `data` is aligned for it
/// (the `malloc`ed buffer `sys_ioctl` uses for a large argument), else on a copy that is
/// written back, as `*(T *)addr` is in C.
fn audio_ioctl_arg<T: AbiPod, R>(data: &mut [u8], f: impl FnOnce(&mut T) -> R) -> R {
    if data.len() >= size_of::<T>() && data.as_ptr().align_offset(align_of::<T>()) == 0 {
        // SAFETY: `data` holds at least `size_of::<T>()` bytes, aligned for `T`; `T:
        // AbiPod` makes any bytes a valid `T` and any `T` valid bytes; the exclusive
        // borrow of `data` covers the view.
        let v = unsafe { &mut *data.as_mut_ptr().cast::<T>() };
        f(v)
    } else {
        let mut v: T = ioctl_arg(data);
        let r = f(&mut v);
        ioctl_ret(data, &v);
        r
    }
}

/// `audio_ioctl`: the `/dev/audioN` ioctls.
fn audio_ioctl(sc: &AudioSoftc, cmd: u64, addr: &mut [u8]) -> Result<(), Errno> {
    // block if quiesced
    while sc.quiesce.get() != 0 {
        let _ = tsleep_nsec(ptr::from_ref(&sc.quiesce), 0, "au_qio", INFSLP);
    }

    match cmd {
        AUDIO_GETPOS => {
            mtx_enter(&AUDIO_LOCK);
            let ap = AudioPos {
                play_pos: sc.play.pos.get(),
                play_xrun: sc.play.xrun.get(),
                rec_pos: sc.rec.pos.get(),
                rec_xrun: sc.rec.xrun.get(),
            };
            mtx_leave(&AUDIO_LOCK);
            ioctl_ret(addr, &ap);
            Ok(())
        }
        AUDIO_START => audio_ioc_start(sc),
        AUDIO_STOP => audio_ioc_stop(sc),
        AUDIO_SETPAR => audio_ioctl_arg(addr, |p: &mut AudioSwpar| audio_ioc_setpar(sc, p)),
        AUDIO_GETPAR => audio_ioctl_arg(addr, |p: &mut AudioSwpar| audio_ioc_getpar(sc, p)),
        AUDIO_GETSTATUS => audio_ioctl_arg(addr, |p: &mut AudioStatus| audio_ioc_getstatus(sc, p)),
        AUDIO_GETDEV => audio_ioctl_arg(addr, |p: &mut AudioDevice| audio_getdev(sc, p)),
        _ => Err(Errno::ENOTTY),
    }
}

/// `audio_event`: queue a change of mixer control `addr` for the mixer's reader.
fn audio_event(sc: &AudioSoftc, addr: i32) {
    mtx_enter(&AUDIO_LOCK);
    if sc.mix_isopen.get() != 0 {
        if let Some(e) = usize::try_from(addr)
            .ok()
            .and_then(|a| sc.mix_evbuf().get(a))
            && e.pending.get() == 0
        {
            e.pending.set(1);
            e.next.set(sc.mix_pending.get());
            sc.mix_pending.set(Some(NonNull::from(e)));
        }
        audio_mixer_wakeup(sc);
    }
    mtx_leave(&AUDIO_LOCK);
}

/// `audio_mixer_devinfo`: `AUDIO_MIXER_DEVINFO`: the driver's controls, then audio(4)'s
/// own `record` class and `record.enable` control.
fn audio_mixer_devinfo(sc: &AudioSoftc, devinfo: &mut MixerDevinfo) -> Result<(), Errno> {
    if devinfo.index < sc.mix_nent.get() {
        // SAFETY: `sc.arg` is the handle the driver attached `sc.ops` with.
        return unsafe { required(sc.ops().query_devinfo, "query_devinfo")(sc.arg.get(), devinfo) };
    }

    devinfo.next = -1;
    devinfo.prev = -1;
    match devinfo.index.wrapping_sub(sc.mix_nent.get()) {
        MIXER_RECORD => {
            strlcpy(&mut devinfo.label.name, AudioCrecord);
            devinfo.type_ = AUDIO_MIXER_CLASS;
            devinfo.mixer_class = -1;
        }
        MIXER_RECORD_ENABLE => {
            strlcpy(&mut devinfo.label.name, b"enable");
            devinfo.type_ = AUDIO_MIXER_ENUM;
            devinfo.mixer_class = MIXER_RECORD + sc.mix_nent.get();
            let e = devinfo.un.e_mut();
            e.num_mem = 3;
            e.member[0].ord = MIXER_RECORD_ENABLE_OFF;
            strlcpy(&mut e.member[0].label.name, b"off");
            e.member[1].ord = MIXER_RECORD_ENABLE_ON;
            strlcpy(&mut e.member[1].label.name, b"on");
            e.member[2].ord = MIXER_RECORD_ENABLE_SYSCTL;
            strlcpy(&mut e.member[2].label.name, b"sysctl");
        }
        _ => return Err(Errno::EINVAL),
    }

    Ok(())
}

/// `audio_mixer_get`: `AUDIO_MIXER_READ`.
fn audio_mixer_get(sc: &AudioSoftc, c: &mut MixerCtrl) -> Result<(), Errno> {
    if c.dev < sc.mix_nent.get() {
        // SAFETY: `sc.arg` is the handle the driver attached `sc.ops` with.
        return unsafe { required(sc.ops().get_port, "get_port")(sc.arg.get(), c) };
    }

    match c.dev.wrapping_sub(sc.mix_nent.get()) {
        MIXER_RECORD => return Err(Errno::EBADF),
        MIXER_RECORD_ENABLE => c.un.set_ord(sc.record_enable.get()),
        _ => return Err(Errno::EINVAL),
    }

    Ok(())
}

/// `audio_mixer_set`: `AUDIO_MIXER_WRITE`; `record.enable` needs the superuser.
fn audio_mixer_set(sc: &AudioSoftc, c: &mut MixerCtrl, p: &Proc) -> Result<(), Errno> {
    let ops = sc.ops();

    if c.dev < sc.mix_nent.get() {
        // SAFETY: `sc.arg` is the handle the driver attached `sc.ops` with.
        unsafe { required(ops.set_port, "set_port")(sc.arg.get(), c)? };
        if let Some(commit_settings) = ops.commit_settings {
            // SAFETY: as above.
            return unsafe { commit_settings(sc.arg.get()) };
        }
        audio_event(sc, c.dev);
        return Ok(());
    }

    match c.dev.wrapping_sub(sc.mix_nent.get()) {
        MIXER_RECORD => return Err(Errno::EBADF),
        MIXER_RECORD_ENABLE => {
            match c.un.ord() {
                MIXER_RECORD_ENABLE_OFF | MIXER_RECORD_ENABLE_ON | MIXER_RECORD_ENABLE_SYSCTL => {}
                _ => return Err(Errno::EINVAL),
            }
            if suser(p).is_ok() {
                sc.record_enable.set(c.un.ord());
            }
        }
        _ => return Err(Errno::EINVAL),
    }

    Ok(())
}

/// `audio_ioctl_mixer`: the mixer ioctls of `/dev/audioctlN`.
fn audio_ioctl_mixer(sc: &AudioSoftc, cmd: u64, addr: &mut [u8], p: &Proc) -> Result<(), Errno> {
    // block if quiesced
    while sc.quiesce.get() != 0 {
        let _ = tsleep_nsec(ptr::from_ref(&sc.quiesce), 0, "mix_qio", INFSLP);
    }

    match cmd {
        AUDIO_MIXER_DEVINFO => {
            audio_ioctl_arg(addr, |d: &mut MixerDevinfo| audio_mixer_devinfo(sc, d))
        }
        AUDIO_MIXER_READ => audio_ioctl_arg(addr, |c: &mut MixerCtrl| audio_mixer_get(sc, c)),
        AUDIO_MIXER_WRITE => audio_ioctl_arg(addr, |c: &mut MixerCtrl| audio_mixer_set(sc, c, p)),
        _ => Err(Errno::ENOTTY),
    }
}

/// `audio_mixer_read`: `read(2)` of `/dev/audioctlN`: the indices (`int`s) of the controls
/// that changed, sleeping until one does.
fn audio_mixer_read(sc: &AudioSoftc, uio: &mut Uio<'_>, ioflag: i32) -> Result<(), Errno> {
    // block if quiesced
    while sc.quiesce.get() != 0 {
        let _ = tsleep_nsec(ptr::from_ref(&sc.quiesce), 0, "mix_qrd", INFSLP);
    }

    mtx_enter(&AUDIO_LOCK);

    // if there are no events then sleep
    while sc.mix_pending.get().is_none() {
        if ioflag & IO_NDELAY != 0 {
            mtx_leave(&AUDIO_LOCK);
            return Err(Errno::EWOULDBLOCK);
        }
        sc.mix_blocking.set(1);
        let mut error = msleep_nsec(
            ptr::from_ref(&sc.mix_blocking),
            &AUDIO_LOCK,
            PWAIT | PCATCH,
            "mix_rd",
            INFSLP,
        );
        if sc.dev.dv_flags.get() & DVF_ACTIVE == 0 {
            error = Err(Errno::EIO);
        }
        if let Err(e) = error {
            mtx_leave(&AUDIO_LOCK);
            return Err(e);
        }
    }

    // at this stage, there is an event to transfer
    while uio.uio_resid >= size_of::<i32>() {
        let Some(e) = sc.mix_pending.get() else {
            break;
        };
        // SAFETY: a pending event is an element of `mix_evbuf`, which lives until detach.
        let ev = unsafe { e.as_ref() };
        sc.mix_pending.set(ev.next.get());
        ev.pending.set(0);
        // SAFETY: both pointers are into the `mix_evbuf` array.
        let data = unsafe { e.as_ptr().offset_from(sc.mix_evbuf.get()) } as i32;
        mtx_leave(&AUDIO_LOCK);
        let mut bytes = data.to_ne_bytes();
        uiomove(&mut bytes, uio)?;
        mtx_enter(&AUDIO_LOCK);
    }

    mtx_leave(&AUDIO_LOCK);
    Ok(())
}

/// `audio_mixer_open`: one reader of the mixer events at a time.
fn audio_mixer_open(sc: &AudioSoftc, flags: i32) -> Result<(), Errno> {
    if flags & FREAD != 0 {
        if sc.mix_isopen.get() != 0 {
            return Err(Errno::EBUSY);
        }
        sc.mix_isopen.set(1);
    }
    Ok(())
}

/// `audio_mixer_close`: drop the reader's pending events.
fn audio_mixer_close(sc: &AudioSoftc, flags: i32) -> Result<(), Errno> {
    if flags & FREAD != 0 {
        sc.mix_isopen.set(0);

        mtx_enter(&AUDIO_LOCK);
        sc.mix_pending.set(None);
        for e in sc.mix_evbuf() {
            e.pending.set(0);
        }
        mtx_leave(&AUDIO_LOCK);
    }
    Ok(())
}

/// `audioopen`: the device switch's `d_open`.
pub fn audioopen(dev: Dev, flags: i32, _mode: i32, _p: &Proc) -> Result<(), Errno> {
    let r = AudioRef::lookup(audio_unit(dev)).ok_or(Errno::ENXIO)?;
    let sc = r.sc();
    if sc.ops.get().is_none() {
        return Err(Errno::ENXIO);
    }
    match audio_dev(dev) {
        AUDIO_DEV_AUDIO => audio_open(sc, flags),
        AUDIO_DEV_AUDIOCTL => audio_mixer_open(sc, flags),
        _ => Err(Errno::ENXIO),
    }
}

/// `audioclose`: the device switch's `d_close`.
pub fn audioclose(dev: Dev, flags: i32, _ifmt: i32, _p: Option<&Proc>) -> Result<(), Errno> {
    let r = AudioRef::lookup(audio_unit(dev)).ok_or(Errno::ENXIO)?;
    let sc = r.sc();
    match audio_dev(dev) {
        AUDIO_DEV_AUDIO => audio_close(sc),
        AUDIO_DEV_AUDIOCTL => audio_mixer_close(sc, flags),
        _ => Err(Errno::ENXIO),
    }
}

/// `audioread`: the device switch's `d_read`.
pub fn audioread(dev: Dev, uio: &mut Uio<'_>, ioflag: i32) -> Result<(), Errno> {
    let r = AudioRef::lookup(audio_unit(dev)).ok_or(Errno::ENXIO)?;
    let sc = r.sc();
    match audio_dev(dev) {
        AUDIO_DEV_AUDIO => audio_read(sc, uio, ioflag),
        AUDIO_DEV_AUDIOCTL => audio_mixer_read(sc, uio, ioflag),
        _ => Err(Errno::ENXIO),
    }
}

/// `audiowrite`: the device switch's `d_write`.
pub fn audiowrite(dev: Dev, uio: &mut Uio<'_>, ioflag: i32) -> Result<(), Errno> {
    let r = AudioRef::lookup(audio_unit(dev)).ok_or(Errno::ENXIO)?;
    let sc = r.sc();
    match audio_dev(dev) {
        AUDIO_DEV_AUDIO => audio_write(sc, uio, ioflag),
        AUDIO_DEV_AUDIOCTL => Err(Errno::ENODEV),
        _ => Err(Errno::ENXIO),
    }
}

/// `audioioctl`: the device switch's `d_ioctl`.
pub fn audioioctl(dev: Dev, cmd: u64, addr: &mut [u8], _flag: i32, p: &Proc) -> Result<(), Errno> {
    let r = AudioRef::lookup(audio_unit(dev)).ok_or(Errno::ENXIO)?;
    let sc = r.sc();
    match audio_dev(dev) {
        AUDIO_DEV_AUDIO => audio_ioctl(sc, cmd, addr),
        AUDIO_DEV_AUDIOCTL => {
            if cmd == AUDIO_SETPAR && sc.mode.get() != 0 {
                return Err(Errno::EBUSY);
            }
            if cmd == AUDIO_START || cmd == AUDIO_STOP {
                return Err(Errno::ENXIO);
            }
            if cmd == AUDIO_MIXER_DEVINFO || cmd == AUDIO_MIXER_READ || cmd == AUDIO_MIXER_WRITE {
                audio_ioctl_mixer(sc, cmd, addr, p)
            } else {
                audio_ioctl(sc, cmd, addr)
            }
        }
        _ => Err(Errno::ENXIO),
    }
}

/// `audiokqfilter`: the device switch's `d_kqfilter`: readable and writable rings, and
/// the mixer's events.
pub fn audiokqfilter(dev: Dev, kn: &Knote) -> Result<(), Errno> {
    let r = AudioRef::lookup(audio_unit(dev)).ok_or(Errno::ENXIO)?;
    let sc = r.sc();
    let klist = match audio_dev(dev) {
        AUDIO_DEV_AUDIO => match kn.kn_filter().get() {
            EVFILT_READ => {
                kn.kn_fop.set(Some(&AUDIOREAD_FILTOPS));
                &sc.rec.klist
            }
            EVFILT_WRITE => {
                kn.kn_fop.set(Some(&AUDIOWRITE_FILTOPS));
                &sc.play.klist
            }
            _ => return Err(Errno::EINVAL),
        },
        AUDIO_DEV_AUDIOCTL => match kn.kn_filter().get() {
            EVFILT_READ => {
                kn.kn_fop.set(Some(&AUDIOCTLREAD_FILTOPS));
                &sc.mix_klist
            }
            _ => return Err(Errno::EINVAL),
        },
        _ => return Err(Errno::ENXIO),
    };
    kn.kn_hook.set(sc.as_arg());

    klist_insert(klist, kn);
    Ok(())
}

/// The softc an audio knote hangs on (`kn->kn_hook`).
fn kn_audio(kn: &Knote) -> &AudioSoftc {
    // SAFETY: `audiokqfilter` set `kn_hook` to the softc; `audio_detach` invalidates the
    // klists before the softc goes away.
    unsafe { &*kn.kn_hook.get().cast::<AudioSoftc>() }
}

/// `filt_audiordetach`.
pub fn filt_audiordetach(kn: &Knote) {
    let sc = kn_audio(kn);

    klist_remove(&sc.rec.klist, kn);
}

/// `filt_audioread`: readable when recorded data is there. Called with `AUDIO_LOCK`.
pub fn filt_audioread(kn: &Knote, _hint: i64) -> bool {
    let sc = kn_audio(kn);

    mutex_assert_locked(&AUDIO_LOCK, "filt_audioread");

    sc.mode.get() & AUMODE_RECORD != 0 && sc.rec.used.get() > 0
}

/// `filt_audiowdetach`.
pub fn filt_audiowdetach(kn: &Knote) {
    let sc = kn_audio(kn);

    klist_remove(&sc.play.klist, kn);
}

/// `filt_audiowrite`: writable when the play buffer has room. Called with `AUDIO_LOCK`.
pub fn filt_audiowrite(kn: &Knote, _hint: i64) -> bool {
    let sc = kn_audio(kn);

    mutex_assert_locked(&AUDIO_LOCK, "filt_audiowrite");

    sc.mode.get() & AUMODE_PLAY != 0 && sc.play.used.get() < sc.play.ulen.get()
}

/// `filt_audioctlrdetach`.
pub fn filt_audioctlrdetach(kn: &Knote) {
    let sc = kn_audio(kn);

    klist_remove(&sc.mix_klist, kn);
}

/// `filt_audioctlread`: readable when a mixer event is pending. Called with `AUDIO_LOCK`.
pub fn filt_audioctlread(kn: &Knote, _hint: i64) -> bool {
    let sc = kn_audio(kn);

    mutex_assert_locked(&AUDIO_LOCK, "filt_audioctlread");

    sc.mix_isopen.get() != 0 && sc.mix_pending.get().is_some()
}

/// `filt_audiomodify`.
pub fn filt_audiomodify(kev: &mut Kevent, kn: &Knote) -> bool {
    mtx_enter(&AUDIO_LOCK);
    let active = knote_modify(kev, kn);
    mtx_leave(&AUDIO_LOCK);

    active
}

/// `filt_audioprocess`.
pub fn filt_audioprocess(kn: &Knote, kev: Option<&mut Kevent>) -> bool {
    mtx_enter(&AUDIO_LOCK);
    let active = knote_process(kn, kev);
    mtx_leave(&AUDIO_LOCK);

    active
}

/// `wskbd_initmute`: the `mute` control in the same class as `vol`, or -1.
fn wskbd_initmute(sc: &AudioSoftc, vol: &MixerDevinfo) -> i32 {
    let mut index = -1;
    let Some(mi) = devinfo_alloc() else {
        return index;
    };
    // SAFETY: `devinfo_alloc` returns a zeroed, exclusively owned `MixerDevinfo`.
    let mi_ref = unsafe { &mut *mi.as_ptr() };
    let query_devinfo = required(sc.ops().query_devinfo, "query_devinfo");

    mi_ref.index = vol.next;
    while mi_ref.index != -1 {
        // SAFETY: `sc.arg` is the handle the driver attached `sc.ops` with.
        if unsafe { query_devinfo(sc.arg.get(), mi_ref) }.is_err() {
            break;
        }
        if cstr_eq(&mi_ref.label.name, AudioNmute) {
            index = mi_ref.index;
            break;
        }
        mi_ref.index = mi_ref.next;
    }

    devinfo_free(mi);
    index
}

/// `strcmp(a, b) == 0` for a NUL-terminated array and a name.
fn cstr_eq(a: &[u8], b: &[u8]) -> bool {
    let len = a.iter().position(|&c| c == 0).unwrap_or(a.len());
    &a[..len] == b
}

/// `wskbd_initvol`: find the value control `cn.dn` for the keyboard's keys.
fn wskbd_initvol(sc: &AudioSoftc, vol: &WskbdVol, cn: &[u8], dn: &[u8]) -> bool {
    vol.val.set(-1);
    vol.mute.set(-1);
    let (Some(dev), Some(cls)) = (devinfo_alloc(), devinfo_alloc()) else {
        return false;
    };
    // SAFETY: `devinfo_alloc` returns zeroed, exclusively owned `MixerDevinfo`s, two
    // distinct allocations.
    let (dev_ref, cls_ref) = unsafe { (&mut *dev.as_ptr(), &mut *cls.as_ptr()) };
    let query_devinfo = required(sc.ops().query_devinfo, "query_devinfo");

    dev_ref.index = 0;
    loop {
        // SAFETY: `sc.arg` is the handle the driver attached `sc.ops` with.
        if unsafe { query_devinfo(sc.arg.get(), dev_ref) }.is_err() {
            break;
        }
        if dev_ref.type_ == AUDIO_MIXER_VALUE {
            cls_ref.index = dev_ref.mixer_class;
            // SAFETY: as above.
            if unsafe { query_devinfo(sc.arg.get(), cls_ref) }.is_ok()
                && cstr_eq(&cls_ref.label.name, cn)
                && cstr_eq(&dev_ref.label.name, dn)
            {
                vol.val.set(dev_ref.index);
                vol.nch.set(dev_ref.un.v().num_channels);
                vol.step.set(dev_ref.un.v().delta.max(8));
                vol.mute.set(wskbd_initmute(sc, dev_ref));
                vol.val_pending.set(0);
                vol.mute_pending.set(0);
                break;
            }
        }
        dev_ref.index += 1;
    }

    devinfo_free(cls);
    devinfo_free(dev);
    vol.val.get() != -1
}

/// `wskbd_mixer_init`: pick the speaker and microphone controls the volume keys drive.
fn wskbd_mixer_init(sc: &AudioSoftc) {
    const SPKR_NAMES: [(&[u8], &[u8]); 4] = [
        (AudioCoutputs, AudioNmaster),
        (AudioCinputs, AudioNdac),
        (AudioCoutputs, AudioNdac),
        (AudioCoutputs, AudioNoutput),
    ];
    const MIC_NAMES: [(&[u8], &[u8]); 5] = [
        (AudioCrecord, AudioNrecord),
        (AudioCrecord, AudioNvolume),
        (AudioCinputs, AudioNrecord),
        (AudioCinputs, AudioNvolume),
        (AudioCinputs, AudioNinput),
    ];

    for (cn, dn) in SPKR_NAMES {
        if wskbd_initvol(sc, &sc.spkr, cn, dn) {
            break;
        }
    }
    for (cn, dn) in MIC_NAMES {
        if wskbd_initvol(sc, &sc.mic, cn, dn) {
            break;
        }
    }
    task_set(&sc.wskbd_task, wskbd_mixer_cb, sc.as_arg());
}

/// `wskbd_mixer_update`: apply the pending key presses to the mixer.
fn wskbd_mixer_update(sc: &AudioSoftc, vol: &WskbdVol) {
    let s = spltty();
    let val_pending = vol.val_pending.replace(0);
    let mute_pending = vol.mute_pending.replace(0);
    splx(s);

    let Some(ops) = sc.ops.get() else {
        return;
    };
    let mut ctrl = MixerCtrl::default();
    if vol.mute.get() >= 0 && mute_pending != 0 {
        ctrl.dev = vol.mute.get();
        ctrl.type_ = AUDIO_MIXER_ENUM;
        // SAFETY: `sc.arg` is the handle the driver attached `sc.ops` with.
        if unsafe { required(ops.get_port, "get_port")(sc.arg.get(), &mut ctrl) }.is_err() {
            return;
        }
        match mute_pending {
            WSKBD_MUTE_TOGGLE => ctrl.un.set_ord(i32::from(ctrl.un.ord() == 0)),
            WSKBD_MUTE_DISABLE => ctrl.un.set_ord(0),
            WSKBD_MUTE_ENABLE => ctrl.un.set_ord(1),
            _ => {}
        }
        // SAFETY: as above.
        if unsafe { required(ops.set_port, "set_port")(sc.arg.get(), &mut ctrl) }.is_err() {
            return;
        }
        audio_event(sc, vol.mute.get());
    }
    if vol.val.get() >= 0 && val_pending != 0 {
        ctrl.dev = vol.val.get();
        ctrl.type_ = AUDIO_MIXER_VALUE;
        ctrl.un.value_mut().num_channels = vol.nch.get();
        // SAFETY: as above.
        if unsafe { required(ops.get_port, "get_port")(sc.arg.get(), &mut ctrl) }.is_err() {
            return;
        }
        let nch = usize::try_from(vol.nch.get()).unwrap_or(0).min(8);
        for level in &mut ctrl.un.value_mut().level[..nch] {
            let gain = i32::from(*level)
                .wrapping_add(vol.step.get().wrapping_mul(val_pending))
                .clamp(AUDIO_MIN_GAIN, AUDIO_MAX_GAIN);
            *level = gain as u8;
        }
        // SAFETY: as above.
        if unsafe { required(ops.set_port, "set_port")(sc.arg.get(), &mut ctrl) }.is_err() {
            return;
        }
        audio_event(sc, vol.val.get());
    }
}

/// `wskbd_mixer_cb`: the task the volume keys queue on `systq`.
fn wskbd_mixer_cb(arg: *mut c_void) {
    // SAFETY: `wskbd_mixer_init` paired this task with its softc, and the queuer handed
    // the task its `device_lookup` reference, which keeps the softc alive until the unref
    // below.
    let sc = unsafe { &*arg.cast::<AudioSoftc>() };

    wskbd_mixer_update(sc, &sc.spkr);
    wskbd_mixer_update(sc, &sc.mic);
    // SAFETY: the reference the queuer handed over (`wskbd_set_mixervolume_unit`).
    unsafe { device_unref(NonNull::from(&sc.dev)) };
}

/// Queues the softc's key task, handing it `r`'s reference (`if (!task_add(systq,
/// &sc->wskbd_task)) device_unref(&sc->dev)`).
fn wskbd_task_add(r: AudioRef) {
    let sc = r.sc();
    // SAFETY: the reference `r` holds keeps the softc, and the task in it, alive until the
    // task runs and gives it back; that is the lifetime `task_add` needs.
    let task: &'static Task = unsafe { &*ptr::from_ref(&sc.wskbd_task) };
    if task_add(SYSTQ, task) {
        // The task gives the reference back (`wskbd_mixer_cb`).
        core::mem::forget(r);
    }
}

/// `wskbd_set_mixermute`: mute (or unmute) audio0's speaker or microphone.
pub fn wskbd_set_mixermute(mute: i64, out: i64) -> Result<(), Errno> {
    let r = AudioRef::lookup(0).ok_or(Errno::ENODEV)?;
    let sc = r.sc();
    let vol = if out != 0 { &sc.spkr } else { &sc.mic };
    vol.mute_pending.set(if mute != 0 {
        WSKBD_MUTE_ENABLE
    } else {
        WSKBD_MUTE_DISABLE
    });
    wskbd_task_add(r);
    Ok(())
}

/// `wskbd_set_mixervolume_dev`: adjust the volume of the audio device associated with the
/// given cookie. Otherwise, fallback to audio0.
pub fn wskbd_set_mixervolume_dev(cookie: *mut c_void, dir: i64, out: i64) -> Result<(), Errno> {
    let mut unit = 0;

    for i in 0..AUDIO_CD.cd_ndevs.get() {
        let Some(r) = AudioRef::lookup(i) else {
            continue;
        };
        if r.sc().cookie.get() != cookie {
            continue;
        }
        unit = i;
        break;
    }

    wskbd_set_mixervolume_unit(unit, dir, out)
}

/// `wskbd_set_mixervolume`: adjust audio0's volume.
pub fn wskbd_set_mixervolume(dir: i64, out: i64) -> Result<(), Errno> {
    wskbd_set_mixervolume_unit(0, dir, out)
}

/// `wskbd_set_mixervolume_unit`: one volume key press (`dir` 0 toggles mute) for unit
/// `unit`'s speaker (`out`) or microphone.
pub fn wskbd_set_mixervolume_unit(unit: i32, dir: i64, out: i64) -> Result<(), Errno> {
    let r = AudioRef::lookup(unit).ok_or(Errno::ENODEV)?;
    let sc = r.sc();
    let vol = if out != 0 { &sc.spkr } else { &sc.mic };
    if dir == 0 {
        vol.mute_pending
            .set(vol.mute_pending.get() ^ WSKBD_MUTE_TOGGLE);
    } else {
        vol.val_pending
            .set(vol.val_pending.get().wrapping_add(dir as i32));
    }
    wskbd_task_add(r);
    Ok(())
}

#[cfg(test)]
mod tests;
