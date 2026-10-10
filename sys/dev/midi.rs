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
 * Copyright (c) 2003, 2004 Alexandre Ratchov
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
//! `midi(4)`: raw MIDI I/O through `/dev/rmidiN`, over a hardware driver's
//! [`MidiHwIf`](crate::dev::midi_if::MidiHwIf) (eap(4)'s UART).
//!
//! Upstream: sys/dev/midi.c @ 3ce1f3f79392
//!
//! The driver attaches a `midi` child with [`midi_attach_mi`], as audio drivers attach
//! `audio` through `audio_attach_mi`: the child takes the attach arguments of type
//! `AUDIODEV_TYPE_MIDI` ([`midiprobe`]). Each direction has a ring buffer of 1 kB
//! (`dev/midivar.rs`). A write fills the output buffer and starts the output; the driver's
//! output call-back ([`midi_ointr`]) sends the next bytes, or, for hardware without an
//! output interrupt, a timeout tick does ([`midi_timeout`]). Received bytes come through
//! [`midi_iintr`] into the input buffer, which `read` drains. Everything the interrupt
//! paths touch is protected by `AUDIO_LOCK` (`audio_lock`), as in the C.
//!
//! ## Deviations
//! - The softc lookups are a guard ([`MidiRef`]) that calls `device_unref` when dropped,
//!   where the C jumps to `done:` (as audio(4)'s `AudioRef`).
//! - Missing driver methods: the C calls through NULL; here [`required`] panics with the
//!   method's name. `midiattach`'s DIAGNOSTIC check of them is under the `diagnostic`
//!   feature, as in the C.
//! - `midiprint` panics on nothing: its `pnp` line is the C's.

use core::ffi::c_void;
use core::ptr::{self, NonNull};

use crate::dev::audio::AUDIO_LOCK;
use crate::dev::audio_if::{AUDIODEV_TYPE_MIDI, AudioAttachArgs};
use crate::dev::midi_if::{MIDI_PROP_OUT_INTR, MidiHwIf, MidiInfo};
use crate::dev::midivar::{MIDIBUF_SIZE, MidiBuffer, MidiSoftc};
use crate::kern::kern_event::{
    klist_free, klist_init_mutex, klist_insert, klist_invalidate, klist_remove, knote_locked,
};
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_subr::uiomove;
use crate::kern::kern_synch::{msleep_nsec, tsleep_nsec, wakeup};
use crate::kern::kern_timeout::{timeout_add, timeout_set};
use crate::kern::subr_autoconf::{config_found, device_lookup, device_unref};
use crate::kern::subr_prf::{Str, panic, printf};
use crate::kern::vfs_subr::vdevgone;
use crate::machine::conf::{cdevsw, nchrdev};
use crate::sys::conf::DevTypeOpen;
use crate::sys::device::{CfMatch, Cfattach, Cfdriver, DV_DULL, DVF_ACTIVE, Device, UNCONF};
use crate::sys::errno::Errno;
use crate::sys::event::{
    EVFILT_READ, EVFILT_WRITE, FILTEROP_ISFD, FILTEROP_MPSAFE, Filterops, Kevent, Knote,
    knote_modify, knote_process,
};
use crate::sys::fcntl::{FREAD, FWRITE};
use crate::sys::mutex::mutex_assert_locked;
use crate::sys::param::{PCATCH, PWAIT};
use crate::sys::proc::Proc;
use crate::sys::systm::{INFSLP, kernel_assert_locked};
use crate::sys::time::{msec_to_nsec, sec_to_nsec};
use crate::sys::types::{Dev, minor};
use crate::sys::uio::Uio;
use crate::sys::vnode::{IO_NDELAY, VCHR};

/// `NMIDI`: `midi* at ...` is configured (`needs-flag`).
pub const NMIDI: i32 = 1;

/// A reference to an attached midi device, from `device_lookup`, given back with
/// `device_unref` when dropped.
struct MidiRef(NonNull<Device>);

impl MidiRef {
    /// `(struct midi_softc *)device_lookup(&midi_cd, minor(dev))`.
    fn lookup(dev: Dev) -> Option<Self> {
        device_lookup(&MIDI_CD, minor(dev) as i32).map(Self)
    }

    /// The softc.
    fn sc(&self) -> &MidiSoftc {
        // SAFETY: `midi_cd`'s devices are made by `midi_ca`, whose softc is a `MidiSoftc`;
        // the reference this holds keeps the allocation alive.
        unsafe { self.0.as_ref().softc::<MidiSoftc>() }
    }
}

impl Drop for MidiRef {
    fn drop(&mut self) {
        // SAFETY: the reference `device_lookup` took.
        unsafe { device_unref(self.0) };
    }
}

impl MidiSoftc {
    /// `sc->hw_if`.
    fn hw(&self) -> &'static MidiHwIf {
        match self.hw_if.get() {
            Some(hw) => hw,
            None => panic(format_args!("{}: no hardware interface", self.devname())),
        }
    }

    /// `DEVNAME(sc)`.
    fn devname(&self) -> &str {
        self.dev.xname()
    }

    /// The softc as the `void *` the driver and the timeout hand back.
    fn as_arg(&self) -> *mut c_void {
        ptr::from_ref(self).cast_mut().cast()
    }
}

/// The C calls through a NULL driver method: the driver's table lacks a mandatory member.
fn required<F: Copy>(f: Option<F>, name: &str) -> F {
    match f {
        Some(f) => f,
        None => panic(format_args!("midi: driver has no {name} method")),
    }
}

/// The softc behind the `void *` midi(4) gave the driver and the timeout.
///
/// # Safety
///
/// `addr` is a `MidiSoftc` that `midiopen` or `midiattach` handed out; softcs are never
/// freed while their device exists.
unsafe fn sc_of<'a>(addr: *mut c_void) -> &'a MidiSoftc {
    // SAFETY: the contract.
    unsafe { &*addr.cast::<MidiSoftc>() }
}

/// `midi_ca`.
pub static MIDI_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<MidiSoftc>(),
    ca_match: Some(midiprobe),
    ca_attach: midiattach,
    ca_detach: Some(mididetach),
    ca_activate: None,
};

/// `midi_cd`.
pub static MIDI_CD: Cfdriver = Cfdriver::new(b"midi", DV_DULL, 0);

/// `midiwrite_filtops`.
pub static MIDIWRITE_FILTOPS: Filterops = Filterops {
    f_flags: FILTEROP_ISFD | FILTEROP_MPSAFE,
    f_attach: None,
    f_detach: Some(filt_midiwdetach),
    f_event: Some(filt_midiwrite),
    f_modify: Some(filt_midimodify),
    f_process: Some(filt_midiprocess),
};

/// `midiread_filtops`.
pub static MIDIREAD_FILTOPS: Filterops = Filterops {
    f_flags: FILTEROP_ISFD | FILTEROP_MPSAFE,
    f_attach: None,
    f_detach: Some(filt_midirdetach),
    f_event: Some(filt_midiread),
    f_modify: Some(filt_midimodify),
    f_process: Some(filt_midiprocess),
};

/// `midi_buf_wakeup`: wakes a reader or writer sleeping on `buf` and its knotes.
pub fn midi_buf_wakeup(buf: &MidiBuffer) {
    if buf.blocking.get() != 0 {
        wakeup(ptr::from_ref(&buf.blocking));
        buf.blocking.set(0);
    }
    knote_locked(&buf.klist, 0);
}

/// `midi_iintr`: the driver received `data`; queue it for `read`.
///
/// # Safety
///
/// `addr` is the softc `midiopen` gave the driver's `open`; the caller holds
/// `AUDIO_LOCK`.
pub unsafe fn midi_iintr(addr: *mut c_void, data: i32) {
    // SAFETY: the contract.
    let sc = unsafe { sc_of(addr) };
    let mb = &sc.inbuf;

    mutex_assert_locked(&AUDIO_LOCK, "midi_iintr");
    if sc.dev.dv_flags.get() & DVF_ACTIVE == 0 || sc.flags.get() & FREAD == 0 {
        return;
    }

    if mb.is_full() {
        return; // discard data
    }

    mb.write(data as u8);

    midi_buf_wakeup(mb);
}

/// `midiread`.
pub fn midiread(dev: Dev, uio: &mut Uio<'_>, ioflag: i32) -> Result<(), Errno> {
    let r = MidiRef::lookup(dev).ok_or(Errno::ENXIO)?;
    let sc = r.sc();
    if sc.flags.get() & FREAD == 0 {
        return Err(Errno::ENXIO);
    }
    let mb = &sc.inbuf;

    // if there is no data then sleep (unless IO_NDELAY flag is set)
    mtx_enter(&AUDIO_LOCK);
    while mb.is_empty() {
        if ioflag & IO_NDELAY != 0 {
            mtx_leave(&AUDIO_LOCK);
            return Err(Errno::EWOULDBLOCK);
        }
        sc.inbuf.blocking.set(1);
        let mut error = msleep_nsec(
            ptr::from_ref(&sc.inbuf.blocking),
            &AUDIO_LOCK,
            PWAIT | PCATCH,
            "mid_rd",
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

    // at this stage, there is at least 1 byte

    while uio.uio_resid > 0 && mb.used() > 0 {
        let count = (MIDIBUF_SIZE - mb.start())
            .min(mb.used())
            .min(uio.uio_resid);
        let start = mb.start();
        mtx_leave(&AUDIO_LOCK);
        // SAFETY: the `count` bytes from `start` hold data this reader removes below; the
        // interrupt only appends after them.
        uiomove(unsafe { mb.bytes(start, count) }, uio)?;
        mtx_enter(&AUDIO_LOCK);
        mb.remove(count);
    }

    mtx_leave(&AUDIO_LOCK);
    Ok(())
}

/// `midi_ointr`: the hardware can take more output.
///
/// # Safety
///
/// As [`midi_iintr`].
pub unsafe fn midi_ointr(addr: *mut c_void) {
    // SAFETY: the contract.
    let sc = unsafe { sc_of(addr) };

    mutex_assert_locked(&AUDIO_LOCK, "midi_ointr");
    if sc.dev.dv_flags.get() & DVF_ACTIVE == 0 || sc.flags.get() & FWRITE == 0 {
        return;
    }

    let mb = &sc.outbuf;
    if mb.used() > 0 {
        midi_out_do(sc);
    } else if sc.isbusy.get() != 0 {
        midi_out_stop(sc);
    }
}

/// `midi_timeout`: the output tick of hardware without an output interrupt.
fn midi_timeout(addr: *mut c_void) {
    mtx_enter(&AUDIO_LOCK);
    // SAFETY: `midiattach` set the timeout with its softc, and the lock is held.
    unsafe { midi_ointr(addr) };
    mtx_leave(&AUDIO_LOCK);
}

/// `midi_out_start`.
pub fn midi_out_start(sc: &MidiSoftc) {
    if sc.isbusy.get() == 0 {
        sc.isbusy.set(1);
        midi_out_do(sc);
    }
}

/// `midi_out_stop`.
pub fn midi_out_stop(sc: &MidiSoftc) {
    sc.isbusy.set(0);
    midi_buf_wakeup(&sc.outbuf);
}

/// `midi_out_do`: hands the hardware bytes until it refuses one or the buffer is empty.
pub fn midi_out_do(sc: &MidiSoftc) {
    let mb = &sc.outbuf;
    let hw = sc.hw();
    let hdl = sc.hw_hdl.get();

    while mb.used() > 0 {
        let byte = i32::from(mb.byte(mb.start()));
        // SAFETY: `hdl` is the handle the driver attached `hw` with.
        if !unsafe { required(hw.output, "output")(hdl, byte) } {
            break;
        }
        mb.remove(1);
        if mb.is_empty() {
            if let Some(flush) = hw.flush {
                // SAFETY: as above.
                unsafe { flush(hdl) };
            }
            midi_out_stop(sc);
            return;
        }
    }

    if sc.props.get() & MIDI_PROP_OUT_INTR == 0 {
        if mb.is_empty() {
            midi_out_stop(sc);
        } else {
            timeout_add(&sc.timeo, 1);
        }
    }
}

/// `midiwrite`.
pub fn midiwrite(dev: Dev, uio: &mut Uio<'_>, ioflag: i32) -> Result<(), Errno> {
    let r = MidiRef::lookup(dev).ok_or(Errno::ENXIO)?;
    let sc = r.sc();
    if sc.flags.get() & FWRITE == 0 {
        return Err(Errno::ENXIO);
    }
    let mb = &sc.outbuf;

    // If IO_NDELAY flag is set then check if there is enough room in the buffer to store
    // at least one byte. If not then dont start the write process.
    mtx_enter(&AUDIO_LOCK);
    if ioflag & IO_NDELAY != 0 && mb.is_full() && uio.uio_resid > 0 {
        mtx_leave(&AUDIO_LOCK);
        return Err(Errno::EWOULDBLOCK);
    }

    while uio.uio_resid > 0 {
        while mb.is_full() {
            if ioflag & IO_NDELAY != 0 {
                // At this stage at least one byte is already moved so we do not return
                // EWOULDBLOCK
                mtx_leave(&AUDIO_LOCK);
                return Ok(());
            }
            sc.outbuf.blocking.set(1);
            let mut error = msleep_nsec(
                ptr::from_ref(&sc.outbuf.blocking),
                &AUDIO_LOCK,
                PWAIT | PCATCH,
                "mid_wr",
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

        let count = (MIDIBUF_SIZE - mb.end()).min(mb.avail()).min(uio.uio_resid);
        let end = mb.end();
        mtx_leave(&AUDIO_LOCK);
        // SAFETY: the `count` free bytes from `end`, which this writer adds below; the
        // output path only reads the used bytes before them.
        uiomove(unsafe { mb.bytes(end, count) }, uio)?;
        mtx_enter(&AUDIO_LOCK);
        mb.used.set(mb.used() + count);
        midi_out_start(sc);
    }

    mtx_leave(&AUDIO_LOCK);
    Ok(())
}

/// `midikqfilter`.
pub fn midikqfilter(dev: Dev, kn: &Knote) -> Result<(), Errno> {
    let r = MidiRef::lookup(dev).ok_or(Errno::ENXIO)?;
    let sc = r.sc();
    let klist = match kn.kn_filter().get() {
        EVFILT_READ => {
            kn.kn_fop.set(Some(&MIDIREAD_FILTOPS));
            &sc.inbuf.klist
        }
        EVFILT_WRITE => {
            kn.kn_fop.set(Some(&MIDIWRITE_FILTOPS));
            &sc.outbuf.klist
        }
        _ => return Err(Errno::EINVAL),
    };
    kn.kn_hook.set(sc.as_arg());

    klist_insert(klist, kn);
    Ok(())
}

/// The softc a midi knote hangs on (`kn->kn_hook`).
fn kn_midi(kn: &Knote) -> &MidiSoftc {
    // SAFETY: `midikqfilter` set `kn_hook` to the softc; `mididetach` invalidates the
    // klists before the softc goes away.
    unsafe { sc_of(kn.kn_hook.get()) }
}

/// `filt_midirdetach`.
pub fn filt_midirdetach(kn: &Knote) {
    let sc = kn_midi(kn);

    klist_remove(&sc.inbuf.klist, kn);
}

/// `filt_midiread`: readable when input is buffered.
pub fn filt_midiread(kn: &Knote, _hint: i64) -> bool {
    let sc = kn_midi(kn);

    !sc.inbuf.is_empty()
}

/// `filt_midiwdetach`.
pub fn filt_midiwdetach(kn: &Knote) {
    let sc = kn_midi(kn);

    klist_remove(&sc.outbuf.klist, kn);
}

/// `filt_midiwrite`: writable while the output buffer has room.
pub fn filt_midiwrite(kn: &Knote, _hint: i64) -> bool {
    let sc = kn_midi(kn);

    !sc.outbuf.is_full()
}

/// `filt_midimodify`.
pub fn filt_midimodify(kev: &mut Kevent, kn: &Knote) -> bool {
    mtx_enter(&AUDIO_LOCK);
    let active = knote_modify(kev, kn);
    mtx_leave(&AUDIO_LOCK);

    active
}

/// `filt_midiprocess`.
pub fn filt_midiprocess(kn: &Knote, kev: Option<&mut Kevent>) -> bool {
    mtx_enter(&AUDIO_LOCK);
    let active = knote_process(kn, kev);
    mtx_leave(&AUDIO_LOCK);

    active
}

/// `midiioctl`: no ioctl is known.
pub fn midiioctl(
    dev: Dev,
    _cmd: u64,
    _addr: &mut [u8],
    _flag: i32,
    _p: &Proc,
) -> Result<(), Errno> {
    let _r = MidiRef::lookup(dev).ok_or(Errno::ENXIO)?;
    Err(Errno::ENOTTY)
}

/// `midiopen`: one opener at a time.
pub fn midiopen(dev: Dev, flags: i32, _mode: i32, _p: &Proc) -> Result<(), Errno> {
    let r = MidiRef::lookup(dev).ok_or(Errno::ENXIO)?;
    let sc = r.sc();
    if sc.flags.get() != 0 {
        return Err(Errno::EBUSY);
    }
    sc.inbuf.init();
    sc.outbuf.init();
    sc.isbusy.set(0);
    sc.inbuf.blocking.set(0);
    sc.outbuf.blocking.set(0);
    sc.flags.set(flags);
    let hw = sc.hw();
    // SAFETY: `hw_hdl` is the handle the driver attached `hw` with; the call-backs get
    // this softc back, which outlives the open.
    let error = unsafe {
        required(hw.open, "open")(sc.hw_hdl.get(), flags, midi_iintr, midi_ointr, sc.as_arg())
    };
    if error.is_err() {
        sc.flags.set(0);
    }
    error
}

/// `midiclose`: drains the output, then closes the hardware.
pub fn midiclose(dev: Dev, _fflag: i32, _devtype: i32, _p: Option<&Proc>) -> Result<(), Errno> {
    let r = MidiRef::lookup(dev).ok_or(Errno::ENXIO)?;
    let sc = r.sc();

    // start draining output buffer
    let mb = &sc.outbuf;
    mtx_enter(&AUDIO_LOCK);
    if !mb.is_empty() {
        midi_out_start(sc);
    }
    while sc.isbusy.get() != 0 {
        sc.outbuf.blocking.set(1);
        let mut error = msleep_nsec(
            ptr::from_ref(&sc.outbuf.blocking),
            &AUDIO_LOCK,
            PWAIT,
            "mid_dr",
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

    // some hw_if->close() reset immediately the midi uart which flushes the internal
    // buffer of the uart device, so we may lose some (important) data. To avoid this,
    // sleep 20ms (around 64 bytes) to give the time to the uart to drain its internal
    // buffers.
    let _ = tsleep_nsec(
        ptr::from_ref(&sc.outbuf.blocking),
        PWAIT,
        "mid_cl",
        msec_to_nsec(20),
    );
    // SAFETY: `hw_hdl` is the handle the driver attached its table with.
    unsafe { required(sc.hw().close, "close")(sc.hw_hdl.get()) };
    sc.flags.set(0);
    Ok(())
}

/// `midiprobe`: takes the `AUDIODEV_TYPE_MIDI` attachment of `midi_attach_mi`.
pub fn midiprobe(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: midi's parents hand it the `struct audio_attach_args` of
    // `audio_attach_mi`/`midi_attach_mi`, or nothing.
    let sa = unsafe { aux.cast::<AudioAttachArgs>().as_ref() };

    i32::from(sa.is_some_and(|sa| sa.type_ == AUDIODEV_TYPE_MIDI))
}

/// `midiattach`.
pub fn midiattach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `midi_ca` makes `MidiSoftc`s.
    let sc = unsafe { self_.softc::<MidiSoftc>() };
    // SAFETY: as in `midiprobe`, which matched only a non-NULL one.
    let sa = unsafe { &*aux.cast::<AudioAttachArgs>() };
    // SAFETY: an `AUDIODEV_TYPE_MIDI` attachment's `hwif` is the driver's `static`
    // `midi_hw_if` (`midi_attach_mi`), or NULL.
    let hwif: Option<&'static MidiHwIf> = unsafe { sa.hwif.cast::<MidiHwIf>().as_ref() };
    let hdl = sa.hdl;

    #[cfg(feature = "diagnostic")]
    if hwif.is_none_or(|h| {
        h.open.is_none() || h.close.is_none() || h.output.is_none() || h.getinfo.is_none()
    }) {
        printf(format_args!("{}: missing method\n", sc.devname()));
        return;
    }

    // SAFETY: `AUDIO_LOCK` is a static, which outlives every klist.
    unsafe { klist_init_mutex(&sc.inbuf.klist, &AUDIO_LOCK) };
    // SAFETY: as above.
    unsafe { klist_init_mutex(&sc.outbuf.klist, &AUDIO_LOCK) };

    sc.hw_if.set(hwif);
    sc.hw_hdl.set(hdl);
    let mut mi = MidiInfo::default();
    // SAFETY: `hdl` is the handle the driver attached `hwif` with.
    unsafe { required(sc.hw().getinfo, "getinfo")(hdl, &mut mi) };
    sc.props.set(mi.props);
    sc.flags.set(0);
    timeout_set(&sc.timeo, midi_timeout, sc.as_arg());
    printf(format_args!(": <{}>\n", mi.name));
}

/// `mididetach`.
pub fn mididetach(self_: &Device, _flags: i32) -> Result<(), Errno> {
    // SAFETY: `midi_ca` makes `MidiSoftc`s.
    let sc = unsafe { self_.softc::<MidiSoftc>() };

    // locate the major number
    for maj in 0..nchrdev() {
        if ptr::fn_addr_eq(cdevsw(maj).d_open, midiopen as DevTypeOpen) {
            // Nuke the vnodes for any open instances (calls close).
            let mn = self_.dv_unit.get() as u32;
            vdevgone(maj, mn, mn, VCHR);
        }
    }

    // The close() method did nothing (device_lookup() returns NULL), so quickly halt
    // transfers (normally parent is already gone, and code below is no-op), and wake-up
    // user-land blocked in read/write/ioctl, which return EIO.
    if sc.flags.get() != 0 {
        kernel_assert_locked();
        if sc.flags.get() & FREAD != 0 {
            wakeup(ptr::from_ref(&sc.inbuf.blocking));
        }
        if sc.flags.get() & FWRITE != 0 {
            wakeup(ptr::from_ref(&sc.outbuf.blocking));
        }
        // SAFETY: `hw_hdl` is the handle the driver attached its table with.
        unsafe { required(sc.hw().close, "close")(sc.hw_hdl.get()) };
        sc.flags.set(0);
    }

    klist_invalidate(&sc.inbuf.klist);
    klist_invalidate(&sc.outbuf.klist);
    klist_free(&sc.inbuf.klist);
    klist_free(&sc.outbuf.klist);

    Ok(())
}

/// `midiprint`: the "not configured" line of a midi child.
pub fn midiprint(_aux: *mut c_void, pnp: Option<&[u8]>) -> i32 {
    if let Some(pnp) = pnp {
        printf(format_args!("midi at {}", Str(pnp)));
    }
    UNCONF
}

/// `midi_attach_mi`: attach a `midi` child to the hardware driver `dev`, whose handle
/// `hdl` every `hwif` method gets first.
pub fn midi_attach_mi(
    hwif: &'static MidiHwIf,
    hdl: *mut c_void,
    dev: &Device,
) -> Option<NonNull<Device>> {
    let mut arg = AudioAttachArgs {
        type_: AUDIODEV_TYPE_MIDI,
        hwif: ptr::from_ref(hwif).cast(),
        hdl,
        cookie: ptr::null_mut(),
    };
    config_found(dev, ptr::from_mut(&mut arg).cast(), Some(midiprint))
}
/* </CODE> */
