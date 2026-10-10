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
 * Copyright (c) 1990 Eric S. Raymond (esr@snark.thyrsus.com)
 * Copyright (c) 1990 Andrew A. Chernov (ache@astral.msk.su)
 * Copyright (c) 1990 Lennart Augustsson (lennart@augustsson.net)
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
 *	This product includes software developed by Eric S. Raymond
 * 4. The name of the author may not be used to endorse or promote products
 *    derived from this software without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
 * WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
 * DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT,
 * INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
 * (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
 * SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT,
 * STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN
 * ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! `spkr(4)`: device driver for console speaker on 80386 (`spkr0 at pcppi?`, `/dev/speaker`).
//!
//! Upstream: sys/dev/isa/spkr.c @ 3ce1f3f79392
//!
//! v1.1 by Eric S. Raymond (esr@snark.thyrsus.com) Feb 1990, modified for 386bsd by Andrew
//! A. Chernov, 386bsd only clean version, all SYSV stuff removed, use hz value from
//! param.c.
//!
//! A write is a play string, interpreted as IBM BASIC 2.0's PLAY statement (`M[LNS]` are
//! missing and the `~` synonym and octave-tracking facility is added): notes `A`..`G` with
//! `#`/`+`/`-` and a length and dots, `O` octave (`ON`/`OL` turn octave tracking off/on),
//! `<`/`>`, `N` absolute note, `L` length, `P`/`~` pause, `T` tempo, `MN`/`ML`/`MS`
//! articulation. Each tone is pcppi(4)'s bell, slept through; a rest is a sleep. The
//! `SPKRTONE` and `SPKRTUNE` ioctls play one tone or a zero-terminated array of them.
//!
//! ## Deviations
//! - The interpreter ([`playstring`], [`playtone`]) works on a copy of the play state and
//!   hands each tone or rest to a call-back ([`Sound`]): the driver plays them
//!   ([`tone`], [`rest`]), the host tests record them. The state is written back once the
//!   string is played, so no borrow of the global outlives a sleep.
//! - The C's file-scope state (`ppicookie`, `spkr_active`, `spkr_inbuf`, `spkr_attached`,
//!   `octave`, `whole`, `value`, `fill`, `octtrack`, `octprefix`) is one [`SpkrState`] in a
//!   `StaticCell`, used under the kernel lock as the C's is.
//! - Number parsing and note-length arithmetic wrap on overflow, where the C's `int`
//!   arithmetic is undefined; a division by zero (more than 25 dots after a note) panics,
//!   where the C traps.
//! - `SPKRDEBUG` is not carried over.

use core::ffi::c_void;
use core::ptr::NonNull;

use libkern::StaticCell;

use crate::dev::isa::pcppi::pcppi_bell;
use crate::dev::isa::pcppivar::{PCPPI_BELL_SLEEP, PcppiAttachArgs, PcppiTag};
use crate::dev::isa::spkrio::{SPKRTONE, SPKRTUNE, ToneT};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_subr::uiomove;
use crate::kern::kern_synch::tsleep_nsec;
use crate::kern::subr_prf::printf;
use crate::machine::copy::copyin;
use crate::sys::device::{CfMatch, Cfattach, Cfdriver, DV_DULL, Device};
use crate::sys::errno::Errno;
use crate::sys::fcntl::FWRITE;
use crate::sys::ioctl::ioctl_arg;
use crate::sys::malloc::{M_DEVBUF, M_WAITOK};
use crate::sys::param::{DEV_BSIZE, PCATCH, PZERO};
use crate::sys::proc::Proc;
use crate::sys::time::msec_to_nsec;
use crate::sys::types::{Dev, minor};
use crate::sys::uio::Uio;

/// `SPKRPRI`.
const SPKRPRI: i32 = PZERO - 1;

/// `SECS_PER_MIN`: seconds per minute.
const SECS_PER_MIN: i32 = 60;
/// `WHOLE_NOTE`: quarter notes per whole note.
const WHOLE_NOTE: i32 = 4;
/// `MIN_VALUE`: the most we can divide a note by.
const MIN_VALUE: i32 = 64;
/// `DFLT_VALUE`: default value (quarter-note).
const DFLT_VALUE: i32 = 4;
/// `FILLTIME`: for articulation, break note in parts.
const FILLTIME: i32 = 8;
/// `STACCATO`: 6/8 = 3/4 of note is filled.
const STACCATO: i32 = 6;
/// `NORMAL`: 7/8ths of note interval is filled.
const NORMAL: i32 = 7;
/// `LEGATO`: all of note interval is filled.
const LEGATO: i32 = 8;
/// `DFLT_OCTAVE`: default octave.
const DFLT_OCTAVE: i32 = 4;
/// `MIN_TEMPO`: minimum tempo.
const MIN_TEMPO: i32 = 32;
/// `DFLT_TEMPO`: default tempo.
const DFLT_TEMPO: i32 = 120;
/// `MAX_TEMPO`: max tempo.
const MAX_TEMPO: i32 = 255;
/// `NUM_MULT`: numerator of dot multiplier.
const NUM_MULT: i32 = 3;
/// `DENOM_MULT`: denominator of dot multiplier.
const DENOM_MULT: i32 = 2;

/// `notetab`: letter to half-tone, `A` to `G` (the C's array has an eighth, unused, 0).
const NOTETAB: [i32; 8] = [9, 11, 0, 2, 4, 5, 7, 0];

/// `OCTAVE_NOTES`: semitones per octave.
const OCTAVE_NOTES: i32 = 12;

/// `pitchtab`: the American Standard A440 Equal-Tempered scale with frequencies rounded to
/// nearest integer (thank Goddess for the good ol' CRC Handbook); our octave 0 is standard
/// octave 2.
#[rustfmt::skip]
const PITCHTAB: [i32; 84] = [
    //  C     C#    D     D#    E     F     F#    G     G#    A     A#    B
          65,   69,   73,   78,   82,   87,   93,   98,  103,  110,  117,  123, // 0
         131,  139,  147,  156,  165,  175,  185,  196,  208,  220,  233,  247, // 1
         262,  277,  294,  311,  330,  349,  370,  392,  415,  440,  466,  494, // 2
         523,  554,  587,  622,  659,  698,  740,  784,  831,  880,  932,  988, // 3
        1047, 1109, 1175, 1245, 1319, 1397, 1480, 1568, 1661, 1760, 1865, 1975, // 4
        2093, 2217, 2349, 2489, 2637, 2794, 2960, 3136, 3322, 3520, 3729, 3951, // 5
        4186, 4435, 4698, 4978, 5274, 5588, 5920, 6272, 6644, 7040, 7459, 7902, // 6
];

/// `NOCTAVES`.
const NOCTAVES: i32 = (PITCHTAB.len() as i32) / OCTAVE_NOTES;

/// What the interpreter plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sound {
    /// `tone(freq, ms)`.
    Tone(i32, i32),
    /// `rest(ms)`.
    Rest(i32),
}

/// The play string interpreter's state (the C's file-scope `int`s).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlayState {
    /// `octave`: currently selected octave.
    pub octave: i32,
    /// `whole`: whole-note time at current tempo, in milliseconds.
    pub whole: i32,
    /// `value`: whole divisor for note time, quarter note = 1.
    pub value: i32,
    /// `fill`: controls spacing of notes.
    pub fill: i32,
    /// `octtrack`: octave-tracking on?
    pub octtrack: i32,
    /// `octprefix`: override current octave-tracking state?
    pub octprefix: i32,
}

/// The driver's file-scope state.
struct SpkrState {
    /// `ppicookie`, as the address it is.
    ppicookie: usize,
    /// `spkr_active`: exclusion flag.
    spkr_active: bool,
    /// `spkr_inbuf`, as the address it is.
    spkr_inbuf: usize,
    /// `spkr_attached`.
    spkr_attached: bool,
    /// The interpreter's state.
    play: PlayState,
}

/// The driver's state; read and written under the kernel lock (attach, the system calls).
static SPKR: StaticCell<SpkrState> = StaticCell::new(SpkrState {
    ppicookie: 0,
    spkr_active: false,
    spkr_inbuf: 0,
    spkr_attached: false,
    play: PlayState {
        octave: 0,
        whole: 0,
        value: 0,
        fill: 0,
        octtrack: 0,
        octprefix: 0,
    },
});

/// The driver's state.
fn spkr() -> &'static mut SpkrState {
    // SAFETY: the kernel lock serialises every user (attach and the system calls, which
    // run with it held); no reference is kept across a sleep (see the module docs).
    unsafe { SPKR.get_mut() }
}

/// `spkr_ca`: the softc is a bare `struct device`.
pub static SPKR_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<Device>(),
    ca_match: Some(spkrprobe),
    ca_attach: spkrattach,
    ca_detach: None,
    ca_activate: None,
};

/// `spkr_cd`.
pub static SPKR_CD: Cfdriver = Cfdriver::new(b"spkr", DV_DULL, 0);

/// `tone`: emit tone of frequency freq for given number of milliseconds.
fn tone(freq: u32, ms: u32) {
    let cookie: PcppiTag = spkr().ppicookie as PcppiTag;
    pcppi_bell(cookie, freq as i32, ms as i32, PCPPI_BELL_SLEEP);
}

/// The sleep channel of a rest: `rest`'s address, as in the C.
fn rest_chan() -> *const () {
    rest as fn(i32) as *const ()
}

/// `rest`: rest for given number of milliseconds: sleep, so other processes can execute
/// while the rest is being waited out.
fn rest(ms: i32) {
    if ms > 0 {
        let _ = tsleep_nsec(
            rest_chan(),
            SPKRPRI | PCATCH,
            "rest",
            msec_to_nsec(ms as u64),
        );
    }
}

/// `playinit`: the defaults a fresh open starts with.
pub fn playinit(st: &mut PlayState) {
    st.octave = DFLT_OCTAVE;
    st.whole = (1000 * SECS_PER_MIN * WHOLE_NOTE) / DFLT_TEMPO;
    st.fill = NORMAL;
    st.value = DFLT_VALUE;
    st.octtrack = 0;
    st.octprefix = 1; // act as though there was an initial O(n)
}

/// `playtone`: play tone of proper duration for current rhythm signature.
pub fn playtone(st: &PlayState, pitch: i32, value: i32, sustain: i32, out: &mut dyn FnMut(Sound)) {
    let (mut snum, mut sdenom) = (1i32, 1i32);

    // this weirdness avoids floating-point arithmetic
    for _ in 0..sustain {
        snum = snum.wrapping_mul(NUM_MULT);
        sdenom = sdenom.wrapping_mul(DENOM_MULT);
    }

    let whole = st.whole;
    if pitch == -1 {
        out(Sound::Rest(
            whole.wrapping_mul(snum) / value.wrapping_mul(sdenom),
        ));
    } else if pitch >= 0 && (pitch as usize) < PITCHTAB.len() {
        let sound = whole.wrapping_mul(snum) / value.wrapping_mul(sdenom)
            - whole.wrapping_mul(FILLTIME - st.fill) / value.wrapping_mul(FILLTIME);
        let silence = whole.wrapping_mul(FILLTIME - st.fill).wrapping_mul(snum)
            / (FILLTIME * value).wrapping_mul(sdenom);

        out(Sound::Tone(PITCHTAB[pitch as usize], sound));
        if st.fill != LEGATO {
            out(Sound::Rest(silence));
        }
    }
}

/// `playstring`: interpret and play the items of a notation string.
pub fn playstring(st: &mut PlayState, s: &[u8], out: &mut dyn FnMut(Sound)) {
    let mut lastpitch = OCTAVE_NOTES * DFLT_OCTAVE;
    let mut slen = s.len();
    let mut cp = 0usize;
    let at = |i: usize| s.get(i).copied().unwrap_or(0);

    // GETNUM(cp, v): the digits after `cp`, consumed.
    let getnum = |cp: &mut usize, slen: &mut usize| -> i32 {
        let mut v: i32 = 0;
        while *slen > 0 && at(*cp + 1).is_ascii_digit() {
            *cp += 1;
            v = v.wrapping_mul(10).wrapping_add(i32::from(at(*cp) - b'0'));
            *slen -= 1;
        }
        v
    };
    // The sustain dots after `cp`, consumed.
    let dots = |cp: &mut usize, slen: &mut usize| -> i32 {
        let mut sustain = 0;
        while *slen > 0 && at(*cp + 1) == b'.' {
            *slen -= 1;
            sustain += 1;
            *cp += 1;
        }
        sustain
    };

    while slen > 0 {
        slen -= 1;
        let c = at(cp).to_ascii_uppercase();

        match c {
            b'A'..=b'G' => {
                // compute pitch
                let mut pitch = NOTETAB[usize::from(c - b'A')] + st.octave * OCTAVE_NOTES;

                // this may be followed by an accidental sign
                if slen > 0 && (at(cp + 1) == b'#' || at(cp + 1) == b'+') {
                    pitch += 1;
                    cp += 1;
                    slen -= 1;
                } else if slen > 0 && at(cp + 1) == b'-' {
                    pitch -= 1;
                    cp += 1;
                    slen -= 1;
                }

                // If octave-tracking mode is on, and there has been no octave-setting
                // prefix, find the version of the current letter note closest to the last
                // regardless of octave.
                if st.octtrack != 0 && st.octprefix == 0 {
                    if (pitch - lastpitch).abs() > (pitch + OCTAVE_NOTES - lastpitch).abs() {
                        st.octave += 1;
                        pitch += OCTAVE_NOTES;
                    }

                    if (pitch - lastpitch).abs() > (pitch - OCTAVE_NOTES - lastpitch).abs() {
                        st.octave -= 1;
                        pitch -= OCTAVE_NOTES;
                    }
                }
                st.octprefix = 0;
                lastpitch = pitch;

                // ...which may in turn be followed by an override time value
                let mut timeval = getnum(&mut cp, &mut slen);
                if timeval <= 0 || timeval > MIN_VALUE {
                    timeval = st.value;
                }

                // ...and/or sustain dots
                let sustain = dots(&mut cp, &mut slen);

                // time to emit the actual tone
                playtone(st, pitch, timeval, sustain, out);
            }
            b'O' => {
                if slen > 0 && (at(cp + 1) == b'N' || at(cp + 1) == b'n') {
                    st.octprefix = 0;
                    st.octtrack = 0;
                    cp += 1;
                    slen -= 1;
                } else if slen > 0 && (at(cp + 1) == b'L' || at(cp + 1) == b'l') {
                    st.octtrack = 1;
                    cp += 1;
                    slen -= 1;
                } else {
                    st.octave = getnum(&mut cp, &mut slen);
                    if st.octave >= NOCTAVES {
                        st.octave = DFLT_OCTAVE;
                    }
                    st.octprefix = 1;
                }
            }
            b'>' => {
                if st.octave < NOCTAVES - 1 {
                    st.octave += 1;
                }
                st.octprefix = 1;
            }
            b'<' => {
                if st.octave > 0 {
                    st.octave -= 1;
                }
                st.octprefix = 1;
            }
            b'N' => {
                let pitch = getnum(&mut cp, &mut slen);
                let sustain = dots(&mut cp, &mut slen);
                playtone(st, pitch.wrapping_sub(1), st.value, sustain, out);
            }
            b'L' => {
                st.value = getnum(&mut cp, &mut slen);
                if st.value <= 0 || st.value > MIN_VALUE {
                    st.value = DFLT_VALUE;
                }
            }
            b'P' | b'~' => {
                // this may be followed by an override time value
                let mut timeval = getnum(&mut cp, &mut slen);
                if timeval <= 0 || timeval > MIN_VALUE {
                    timeval = st.value;
                }
                let sustain = dots(&mut cp, &mut slen);
                playtone(st, -1, timeval, sustain, out);
            }
            b'T' => {
                let mut tempo = getnum(&mut cp, &mut slen);
                if !(MIN_TEMPO..=MAX_TEMPO).contains(&tempo) {
                    tempo = DFLT_TEMPO;
                }
                st.whole = (1000 * SECS_PER_MIN * WHOLE_NOTE) / tempo;
            }
            b'M' => {
                if slen > 0 && (at(cp + 1) == b'N' || at(cp + 1) == b'n') {
                    st.fill = NORMAL;
                    cp += 1;
                    slen -= 1;
                } else if slen > 0 && (at(cp + 1) == b'L' || at(cp + 1) == b'l') {
                    st.fill = LEGATO;
                    cp += 1;
                    slen -= 1;
                } else if slen > 0 && (at(cp + 1) == b'S' || at(cp + 1) == b's') {
                    st.fill = STACCATO;
                    cp += 1;
                    slen -= 1;
                }
            }
            _ => {}
        }
        cp += 1;
    }
}

/// Plays what the interpreter produces on the speaker.
fn play(sound: Sound) {
    match sound {
        Sound::Tone(freq, ms) => tone(freq as u32, ms as u32),
        Sound::Rest(ms) => rest(ms),
    }
}

/// `spkrprobe`: one speaker.
pub fn spkrprobe(_parent: Option<&Device>, _match: &CfMatch, _aux: *mut c_void) -> i32 {
    i32::from(!spkr().spkr_attached)
}

/// `spkrattach`: keep pcppi's cookie.
pub fn spkrattach(_parent: Option<&Device>, _self: &Device, aux: *mut c_void) {
    printf(format_args!("\n"));
    // SAFETY: pcppi hands its children `struct pcppi_attach_args`.
    let pa = unsafe { &*aux.cast::<PcppiAttachArgs>() };
    let st = spkr();
    st.ppicookie = pa.pa_cookie as usize;
    st.spkr_attached = true;
}

/// `spkropen`.
pub fn spkropen(dev: Dev, _flags: i32, _mode: i32, _p: &Proc) -> Result<(), Errno> {
    let st = spkr();
    if minor(dev) != 0 || !st.spkr_attached {
        return Err(Errno::ENXIO);
    } else if st.spkr_active {
        return Err(Errno::EBUSY);
    }
    playinit(&mut st.play);
    st.spkr_inbuf = malloc(DEV_BSIZE, M_DEVBUF, M_WAITOK).map_or(0, |p| p.as_ptr() as usize);
    st.spkr_active = true;
    Ok(())
}

/// `spkrwrite`: play the string written (at most `DEV_BSIZE` bytes per call).
pub fn spkrwrite(dev: Dev, uio: &mut Uio<'_>, _flags: i32) -> Result<(), Errno> {
    if minor(dev) != 0 {
        return Err(Errno::ENXIO);
    }
    let n = DEV_BSIZE.min(uio.uio_resid);
    let buf = spkr().spkr_inbuf as *mut u8;
    if buf.is_null() {
        return Err(Errno::ENXIO);
    }
    // SAFETY: `spkr_inbuf` is the `DEV_BSIZE` bytes spkropen allocated, used by this open
    // only (`spkr_active` excludes another).
    let data = unsafe { core::slice::from_raw_parts_mut(buf, n) };
    uiomove(data, uio)?;
    let mut st = spkr().play;
    playstring(&mut st, data, &mut play);
    spkr().play = st;
    Ok(())
}

/// `spkrclose`.
pub fn spkrclose(dev: Dev, _flags: i32, _mode: i32, _p: Option<&Proc>) -> Result<(), Errno> {
    if minor(dev) != 0 {
        return Err(Errno::ENXIO);
    }
    tone(0, 0);
    let st = spkr();
    if let Some(buf) = NonNull::new(st.spkr_inbuf as *mut u8) {
        free(buf, M_DEVBUF, DEV_BSIZE);
    }
    st.spkr_inbuf = 0;
    st.spkr_active = false;
    Ok(())
}

/// `spkrioctl`: `SPKRTONE` (one tone) and `SPKRTUNE` (an array of them, ended by a zero
/// duration, read from the user's address).
pub fn spkrioctl(dev: Dev, cmd: u64, data: &mut [u8], flag: i32, _p: &Proc) -> Result<(), Errno> {
    if minor(dev) != 0 {
        return Err(Errno::ENXIO);
    }

    if (cmd == SPKRTONE || cmd == SPKRTUNE) && flag & FWRITE == 0 {
        return Err(Errno::EACCES);
    }

    match cmd {
        SPKRTONE => {
            let tp: ToneT = ioctl_arg(data);

            if tp.duration < 0 || tp.frequency < 0 {
                return Err(Errno::EINVAL);
            }
            if tp.frequency == 0 {
                rest(tp.duration);
            } else {
                tone(tp.frequency as u32, tp.duration as u32);
            }
        }
        SPKRTUNE => {
            // `*(caddr_t *)data`: sys_ioctl stored the user's pointer there (`IOC_VOID`).
            let mut addr = [0u8; size_of::<usize>()];
            if let Some(src) = data.get(..addr.len()) {
                addr.copy_from_slice(src);
            }
            let mut tp = usize::from_ne_bytes(addr);

            loop {
                let mut raw = [0u8; size_of::<ToneT>()];
                copyin(tp, &mut raw)?;
                let ttp: ToneT = ioctl_arg(&raw);
                if ttp.duration < 0 || ttp.frequency < 0 {
                    return Err(Errno::EINVAL);
                }
                if ttp.duration == 0 {
                    break;
                }
                if ttp.frequency == 0 {
                    rest(ttp.duration);
                } else {
                    tone(ttp.frequency as u32, ttp.duration as u32);
                }
                tp = tp.wrapping_add(size_of::<ToneT>());
            }
        }
        _ => return Err(Errno::ENOTTY),
    }

    Ok(())
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    fn run(s: &str) -> (PlayState, std::vec::Vec<Sound>) {
        let mut st = PlayState::default();
        playinit(&mut st);
        let mut out = std::vec::Vec::new();
        playstring(&mut st, s.as_bytes(), &mut |x| out.push(x));
        (st, out)
    }

    #[test]
    fn defaults_play_quarter_notes() {
        // T120: whole = 2000 ms, quarter = 500 ms, 7/8 sounded.
        let (st, out) = run("c");
        assert_eq!(st.whole, 2000);
        assert_eq!(out, [Sound::Tone(1047, 500 - 62), Sound::Rest(62)]);
    }

    #[test]
    fn octaves_accidentals_lengths_dots() {
        let (_, out) = run("o2a#8.");
        // A# in octave 2 is 466 Hz; an eighth (250 ms) dotted is 375 ms.
        assert_eq!(out[0], Sound::Tone(466, 375 - 31));
        let (st, _) = run(">>>>");
        assert_eq!(st.octave, NOCTAVES - 1);
        let (st, out) = run("ml l16 t240 n1");
        assert_eq!((st.fill, st.value, st.whole), (LEGATO, 16, 1000));
        assert_eq!(out, [Sound::Tone(65, 62)]);
        let (_, out) = run("p2 n0");
        assert_eq!(out, [Sound::Rest(1000), Sound::Rest(500)]);
    }

    #[test]
    fn octave_tracking() {
        // With tracking on, B after C goes down to the closest B.
        let (st, out) = run("olcb");
        assert_eq!(st.octave, DFLT_OCTAVE - 1);
        assert_eq!(out[2], Sound::Tone(PITCHTAB[(3 * 12 + 11) as usize], 438));
    }

    #[test]
    fn bad_values_fall_back() {
        let (st, _) = run("o9 l99 t999");
        assert_eq!(
            (st.octave, st.value, st.whole),
            (DFLT_OCTAVE, DFLT_VALUE, 2000)
        );
        // Unknown characters are ignored.
        let (_, out) = run("xyz!");
        assert!(out.is_empty());
    }
}
/* </TESTS> */
