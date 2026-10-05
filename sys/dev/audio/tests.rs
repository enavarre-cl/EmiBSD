use core::sync::atomic::AtomicUsize;
use std::boxed::Box;
use std::vec;
use std::vec::Vec;

use super::*;
use crate::dev::audio_if::AudioIntr;
use crate::kern::init_main::PROC0;
use crate::kern::subr_pool::tests::setup_real_memory;
use crate::sys::audioio::{AudioMixerName, MixerLevel};
use crate::sys::uio::{Iovec, UioRw, UioSeg};

// --- a fake hardware driver -------------------------------------------------------------

/// Calls of `fake_trigger_output`.
static TRIGGERS: AtomicUsize = AtomicUsize::new(0);
/// The master volume `fake_set_port` last stored.
static MASTER: AtomicI32 = AtomicI32::new(100);

unsafe fn fake_open(_: *mut c_void, _: i32) -> Result<(), Errno> {
    Ok(())
}

unsafe fn fake_close(_: *mut c_void) {}

unsafe fn fake_set_params(
    _: *mut c_void,
    _: i32,
    _: i32,
    _: &mut AudioParams,
    _: &mut AudioParams,
) -> Result<(), Errno> {
    Ok(())
}

/// Blocks are multiples of 128 bytes.
unsafe fn fake_round_blocksize(_: *mut c_void, blksz: i32) -> i32 {
    (blksz + 127) & !127
}

unsafe fn fake_halt(_: *mut c_void) -> Result<(), Errno> {
    Ok(())
}

unsafe fn fake_trigger(
    _: *mut c_void,
    start: *mut u8,
    end: *mut u8,
    blksz: i32,
    _: AudioIntr,
    _: *mut c_void,
    _: &AudioParams,
) -> Result<(), Errno> {
    assert!(end as usize > start as usize && blksz > 0);
    TRIGGERS.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

fn name(s: &[u8]) -> AudioMixerName {
    let mut n = AudioMixerName::default();
    n.name[..s.len()].copy_from_slice(s);
    n
}

/// `outputs` (class), `outputs.master` (value, 2 channels), `outputs.mute` (enum).
unsafe fn fake_query_devinfo(_: *mut c_void, mi: &mut MixerDevinfo) -> Result<(), Errno> {
    mi.mixer_class = 0;
    match mi.index {
        0 => {
            mi.type_ = AUDIO_MIXER_CLASS;
            mi.label = name(b"outputs");
            mi.next = -1;
        }
        1 => {
            mi.type_ = AUDIO_MIXER_VALUE;
            mi.label = name(b"master");
            mi.next = 2;
            mi.un.v_mut().num_channels = 2;
            mi.un.v_mut().delta = 4;
        }
        2 => {
            mi.type_ = AUDIO_MIXER_ENUM;
            mi.label = name(b"mute");
            mi.prev = 1;
            mi.next = -1;
        }
        _ => return Err(Errno::ENXIO),
    }
    Ok(())
}

unsafe fn fake_get_port(_: *mut c_void, c: &mut MixerCtrl) -> Result<(), Errno> {
    if c.dev == 1 {
        let v = MASTER.load(Ordering::Relaxed) as u8;
        *c.un.value_mut() = MixerLevel {
            num_channels: 2,
            level: [v, v, 0, 0, 0, 0, 0, 0],
        };
    }
    Ok(())
}

unsafe fn fake_set_port(_: *mut c_void, c: &mut MixerCtrl) -> Result<(), Errno> {
    if c.dev == 1 {
        MASTER.store(i32::from(c.un.value().level[0]), Ordering::Relaxed);
    }
    Ok(())
}

static FAKE_HW_IF: AudioHwIf = AudioHwIf {
    open: Some(fake_open),
    close: Some(fake_close),
    set_params: Some(fake_set_params),
    round_blocksize: Some(fake_round_blocksize),
    halt_output: Some(fake_halt),
    halt_input: Some(fake_halt),
    set_port: Some(fake_set_port),
    get_port: Some(fake_get_port),
    query_devinfo: Some(fake_query_devinfo),
    trigger_output: Some(fake_trigger),
    trigger_input: Some(fake_trigger),
    ..AudioHwIf::new()
};

/// A zeroed softc, attached to the fake driver; the caller holds `setup_real_memory()`
/// for `malloc(9)`.
fn attached() -> &'static AudioSoftc {
    // SAFETY: `AudioSoftc` is a `Softc`: all-zero bytes are a valid value of it.
    let sc: &'static AudioSoftc = Box::leak(Box::new(unsafe { core::mem::zeroed::<AudioSoftc>() }));
    sc.dev.dv_flags.set(DVF_ACTIVE);
    let mut aa = AudioAttachArgs {
        type_: AUDIODEV_TYPE_AUDIO,
        hwif: ptr::from_ref(&FAKE_HW_IF).cast(),
        hdl: ptr::null_mut(),
        cookie: ptr::null_mut(),
    };
    let aux = ptr::from_mut(&mut aa).cast();
    assert_eq!(audio_match(None, &CfMatch::Cfdata(&DUMMY_CF), aux), 1);
    audio_attach(None, &sc.dev, aux);
    sc
}

static DUMMY_CF: crate::sys::device::Cfdata =
    crate::sys::device::Cfdata::new(&AUDIO_CA, &AUDIO_CD, 0, 0, &[], 0, &[], 0, 0);

/// A kernel-space uio over `buf`.
fn uio_over<'a>(iov: &'a mut [Iovec], rw: UioRw) -> Uio<'a> {
    let resid = iov.iter().map(|v| v.iov_len).sum();
    Uio {
        uio_iov: iov,
        uio_offset: 0,
        uio_resid: resid,
        uio_segflg: UioSeg::UIO_SYSSPACE,
        uio_rw: rw,
        uio_procp: None,
    }
}

fn getpar(sc: &AudioSoftc) -> AudioSwpar {
    let mut data = [0u8; size_of::<AudioSwpar>()];
    audio_ioctl(sc, AUDIO_GETPAR, &mut data).unwrap();
    ioctl_arg(&data)
}

// --- tests --------------------------------------------------------------------------------

#[test]
fn gcd_and_blksz_bytes() {
    assert_eq!(audio_gcd(12, 18), 6);
    assert_eq!(audio_gcd(7, 0), 7);
    assert_eq!(audio_gcd(0, 5), 5);
    let p = AudioParams {
        bps: 2,
        channels: 2,
        ..AudioParams::default()
    };
    let r = AudioParams {
        bps: 2,
        channels: 1,
        ..AudioParams::default()
    };
    // 4-byte play frames, 2-byte record frames, 128-byte blocks: 32 and 64 frames.
    assert_eq!(audio_blksz_bytes(AUMODE_PLAY, &p, &r, 128), 32);
    assert_eq!(audio_blksz_bytes(AUMODE_RECORD, &p, &r, 128), 64);
    assert_eq!(
        audio_blksz_bytes(AUMODE_PLAY | AUMODE_RECORD, &p, &r, 128),
        64
    );
}

#[test]
fn ring_pointers_wrap() {
    // SAFETY: as in `attached`.
    let sc: &'static AudioSoftc = Box::leak(Box::new(unsafe { core::mem::zeroed::<AudioSoftc>() }));
    let mut mem = vec![0u8; 16];
    let b = &sc.play;
    b.data.set(mem.as_mut_ptr());
    b.datalen.set(16);
    b.klen.set(12);
    b.ulen.set(8);

    let (p, n) = audio_buf_wgetblk(b);
    assert_eq!((b.offset_of(p), n), (0, 8));
    audio_buf_wcommit(b, 8);
    assert_eq!(audio_buf_wgetblk(b).1, 0);
    audio_buf_rdiscard(b, 4);
    // Room again, after the 4 bytes still used.
    let (p, n) = audio_buf_wgetblk(b);
    assert_eq!((b.offset_of(p), n), (8, 4));
    audio_buf_wcommit(b, 4);
    audio_buf_rdiscard(b, 6);
    let (p, n) = audio_buf_rgetblk(b);
    assert_eq!((b.offset_of(p), n), (10, 2));
    audio_buf_rdiscard(b, 2);
    // The reader wrapped at klen.
    assert_eq!((b.start.get(), b.used.get()), (0, 0));
    // A start past ulen: the C's size_t difference wraps and `used` bounds it.
    b.start.set(10);
    b.used.set(2);
    let (p, n) = audio_buf_rgetblk(b);
    assert_eq!((b.offset_of(p), n), (10, 2));
    drop(mem);
}

#[test]
fn silence_per_encoding() {
    // SAFETY: as in `attached`.
    let sc: &'static AudioSoftc = Box::leak(Box::new(unsafe { core::mem::zeroed::<AudioSoftc>() }));
    let sil = |enc, bits, bps, msb| {
        sc.sw_enc.set(enc);
        sc.bits.set(bits);
        sc.bps.set(bps);
        sc.msb.set(msb);
        audio_calc_sil(sc);
        sc.silence.get()[..bps as usize].to_vec()
    };
    assert_eq!(sil(AUDIO_ENCODING_SLINEAR_LE, 16, 2, 1), [0, 0]);
    assert_eq!(sil(AUDIO_ENCODING_ULINEAR_LE, 16, 2, 1), [0x00, 0x80]);
    assert_eq!(sil(AUDIO_ENCODING_ULINEAR_BE, 16, 2, 1), [0x80, 0x00]);
    assert_eq!(sil(AUDIO_ENCODING_ULINEAR_LE, 8, 1, 1), [0x80]);
    // 24 bits lsb-aligned in 4 bytes, and msb-aligned.
    assert_eq!(sil(AUDIO_ENCODING_ULINEAR_LE, 24, 4, 0), [0, 0, 0x80, 0]);
    assert_eq!(sil(AUDIO_ENCODING_ULINEAR_BE, 24, 4, 1), [0x80, 0, 0, 0]);
    // Encoded for a mu-law device: linear zero is mu-law 0xff.
    sc.conv_enc.set(Some(slinear8_to_mulaw));
    assert_eq!(sil(AUDIO_ENCODING_SLINEAR_LE, 8, 1, 1), [0xff]);

    let mut buf = [1u8; 7];
    sc.conv_enc.set(None);
    sil(AUDIO_ENCODING_ULINEAR_LE, 16, 2, 1);
    audio_fill_sil(sc, &mut buf);
    // Whole samples only; the odd byte is left.
    assert_eq!(buf, [0, 0x80, 0, 0x80, 0, 0x80, 1]);
}

#[test]
fn attach_saves_the_mixer_and_finds_the_volume() {
    let _g = setup_real_memory();
    let sc = attached();
    assert!(sc.ops.get().is_some());
    assert_eq!(sc.mix_nent.get(), 3);
    // SAFETY: the test owns the softc.
    let ents = unsafe { sc.mix_ents() };
    assert_eq!((ents[1].dev, ents[1].type_), (1, AUDIO_MIXER_VALUE));
    assert_eq!(ents[1].un.value().num_channels, 2);
    assert_eq!((ents[2].dev, ents[2].type_), (2, AUDIO_MIXER_ENUM));
    // The class entry stays zero.
    assert_eq!(ents[0].type_, 0);
    // outputs.master and its mute control drive the keyboard's keys; no microphone.
    assert_eq!(sc.spkr.val.get(), 1);
    assert_eq!(sc.spkr.mute.get(), 2);
    assert_eq!(sc.spkr.step.get(), 8);
    assert_eq!(sc.mic.val.get(), -1);
    // Defaults.
    assert_eq!(sc.rate.get(), 48000);
    assert_eq!(sc.round.get(), 960);
    assert_eq!(sc.record_enable.get(), MIXER_RECORD_ENABLE_SYSCTL);
    assert_eq!(sc.play.datalen.get(), AUDIO_BUFSZ);
}

#[test]
fn write_starts_playback_and_interrupts_advance() {
    let _g = setup_real_memory();
    let sc = attached();
    audio_open(sc, FWRITE).unwrap();
    assert_eq!(sc.mode.get(), AUMODE_PLAY);
    let par = getpar(sc);
    assert_eq!((par.rate, par.round, par.nblks), (48000, 960, 2));
    assert_eq!(
        (par.sig, par.le, par.bits, par.bps, par.pchan),
        (1, 1, 16, 2, 2)
    );
    // 960 frames of 4 bytes; two blocks; the ring a whole number of blocks.
    assert_eq!(sc.play.blksz.get(), 3840);
    assert_eq!(sc.play.ulen.get(), 7680);
    assert_eq!(sc.play.klen.get(), 65280);

    // Writing a full buffer starts DMA by itself.
    let before = TRIGGERS.load(Ordering::Relaxed);
    let mut src = vec![0x11u8; 7680];
    let mut iov = [Iovec {
        iov_base: src.as_mut_ptr().cast(),
        iov_len: src.len(),
    }];
    let mut uio = uio_over(&mut iov, UioRw::UIO_WRITE);
    audio_write(sc, &mut uio, IO_NDELAY).unwrap();
    assert_eq!(uio.uio_resid, 0);
    assert_eq!(sc.active.get(), 1);
    assert!(TRIGGERS.load(Ordering::Relaxed) > before);
    // Full: a non-blocking write would block.
    let mut more = [0u8; 4];
    let mut iov = [Iovec {
        iov_base: more.as_mut_ptr().cast(),
        iov_len: 4,
    }];
    let mut uio = uio_over(&mut iov, UioRw::UIO_WRITE);
    assert_eq!(
        audio_write(sc, &mut uio, IO_NDELAY),
        Err(Errno::EWOULDBLOCK)
    );

    // One block played: the hardware's interrupt.
    mtx_enter(&AUDIO_LOCK);
    // SAFETY: the softc audio(4) handed the driver; the lock is held.
    unsafe { audio_pintr(sc.as_arg()) };
    mtx_leave(&AUDIO_LOCK);
    assert_eq!(sc.play.used.get(), 3840);
    // The block played was refilled with silence.
    // SAFETY: the test owns the ring.
    assert!(unsafe { sc.play.bytes(0, 3840) }.iter().all(|&b| b == 0));
    let mut pos = [0u8; size_of::<AudioPos>()];
    audio_ioctl(sc, AUDIO_GETPOS, &mut pos).unwrap();
    let pos: AudioPos = ioctl_arg(&pos);
    assert_eq!((pos.play_pos, pos.play_xrun), (3840, 0));

    // Two more blocks with nothing written: the second user block plays, then each
    // interrupt finds the buffer empty and inserts a block of silence.
    mtx_enter(&AUDIO_LOCK);
    // SAFETY: as above.
    unsafe {
        audio_pintr(sc.as_arg());
        audio_pintr(sc.as_arg());
    }
    mtx_leave(&AUDIO_LOCK);
    assert_eq!(sc.play.xrun.get(), 2 * 3840);
    assert_eq!(sc.play.used.get(), 3840);
    assert_eq!(sc.play.pos.get(), 3 * 3840);

    let mut st = [0u8; size_of::<AudioStatus>()];
    audio_ioctl(sc, AUDIO_GETSTATUS, &mut st).unwrap();
    let st: AudioStatus = ioctl_arg(&st);
    assert_eq!((st.mode, st.pause, st.active), (AUMODE_PLAY, 0, 1));

    // Stop, then close (no drain while paused).
    audio_ioctl(sc, AUDIO_STOP, &mut []).unwrap();
    assert_eq!((sc.pause.get(), sc.active.get()), (1, 0));
    assert_eq!(audio_ioctl(sc, AUDIO_STOP, &mut []), Err(Errno::EBUSY));
    audio_close(sc).unwrap();
    assert_eq!(sc.mode.get(), 0);
}

#[test]
fn setpar_clamps_and_picks_the_encoding() {
    let _g = setup_real_memory();
    let sc = attached();
    audio_open(sc, FREAD | FWRITE).unwrap();
    let mut p = AudioSwpar::initpar();
    p.sig = 0;
    p.bits = 8;
    p.rate = 1000;
    p.pchan = 100;
    p.round = 1;
    let mut data = [0u8; size_of::<AudioSwpar>()];
    ioctl_ret(&mut data, &p);
    audio_ioctl(sc, AUDIO_SETPAR, &mut data).unwrap();
    let got = getpar(sc);
    assert_eq!((got.sig, got.le, got.bits, got.bps), (0, 1, 8, 1));
    assert_eq!((got.rate, got.pchan, got.rchan), (4000, 64, 2));
    // 128-byte blocks: 2 frames of 64 bytes for play, 64 of 2 for record; the least
    // common multiple of the frame counts, at least rate/1000.
    assert_eq!(got.round % 64, 0);
    assert!(got.round >= 4);
    assert_eq!(sc.silence.get()[0], 0x80);
    // SAFETY: the test owns the ring, DMA is stopped.
    assert!(
        unsafe { sc.play.bytes(0, sc.play.klen.get()) }
            .iter()
            .all(|&b| b == 0x80)
    );
    // No change while DMA runs.
    sc.active.set(1);
    assert_eq!(audio_ioctl(sc, AUDIO_SETPAR, &mut data), Err(Errno::EBUSY));
    sc.active.set(0);
}

#[test]
fn record_respects_record_enable_and_reads_back() {
    let _g = setup_real_memory();
    let sc = attached();
    audio_open(sc, FREAD).unwrap();
    let blksz = sc.rec.blksz.get();
    assert_eq!(sc.rec.ulen.get(), sc.rec.klen.get());
    audio_start(sc).unwrap();

    // The hardware wrote 0x55s into the ring.
    // SAFETY: the test owns the ring.
    unsafe { sc.rec.bytes(0, sc.rec.klen.get()) }.fill(0x55);
    // record.enable=sysctl and kern.audio.record=0: silence instead.
    mtx_enter(&AUDIO_LOCK);
    // SAFETY: the softc audio(4) handed the driver; the lock is held.
    unsafe { audio_rintr(sc.as_arg()) };
    sc.record_enable.set(MIXER_RECORD_ENABLE_ON);
    // SAFETY: as above.
    unsafe { audio_rintr(sc.as_arg()) };
    mtx_leave(&AUDIO_LOCK);
    assert_eq!(sc.rec.used.get(), 2 * blksz);
    assert_eq!(sc.rec.pos.get() as usize, 2 * blksz);

    let mut dst = vec![0xaau8; 2 * blksz + 8];
    let mut iov = [Iovec {
        iov_base: dst.as_mut_ptr().cast(),
        iov_len: dst.len(),
    }];
    let mut uio = uio_over(&mut iov, UioRw::UIO_READ);
    audio_read(sc, &mut uio, 0).unwrap();
    assert_eq!(uio.uio_resid, 8);
    assert!(dst[..blksz].iter().all(|&b| b == 0));
    assert!(dst[blksz..2 * blksz].iter().all(|&b| b == 0x55));
    assert_eq!(sc.rec.used.get(), 0);
    // Nothing left: a non-blocking read would block.
    let mut iov = [Iovec {
        iov_base: dst.as_mut_ptr().cast(),
        iov_len: 4,
    }];
    let mut uio = uio_over(&mut iov, UioRw::UIO_READ);
    assert_eq!(audio_read(sc, &mut uio, IO_NDELAY), Err(Errno::EWOULDBLOCK));
    let _ = audio_stop(sc);
}

#[test]
fn mixer_controls_and_events() {
    let _g = setup_real_memory();
    let sc = attached();
    let nent = sc.mix_nent.get();

    // audio(4)'s own controls follow the driver's.
    let mut d = MixerDevinfo::zeroed();
    d.index = nent;
    audio_mixer_devinfo(sc, &mut d).unwrap();
    assert_eq!((d.type_, d.mixer_class), (AUDIO_MIXER_CLASS, -1));
    assert!(cstr_eq(&d.label.name, b"record"));
    d.index = nent + 1;
    audio_mixer_devinfo(sc, &mut d).unwrap();
    assert_eq!(
        (d.type_, d.mixer_class, d.un.e().num_mem),
        (AUDIO_MIXER_ENUM, nent, 3)
    );
    assert!(cstr_eq(&d.un.e().member[2].label.name, b"sysctl"));
    d.index = nent + 2;
    assert_eq!(audio_mixer_devinfo(sc, &mut d), Err(Errno::EINVAL));
    d.index = i32::MIN;
    // A driver index: the fake has none below 0.
    assert_eq!(audio_mixer_devinfo(sc, &mut d), Err(Errno::ENXIO));

    let mut c = MixerCtrl {
        dev: nent + 1,
        ..MixerCtrl::default()
    };
    audio_mixer_get(sc, &mut c).unwrap();
    assert_eq!(c.un.ord(), MIXER_RECORD_ENABLE_SYSCTL);
    c.dev = nent;
    assert_eq!(audio_mixer_get(sc, &mut c), Err(Errno::EBADF));

    // A change of a driver control is queued for the mixer's reader.
    audio_mixer_open(sc, FREAD).unwrap();
    assert_eq!(audio_mixer_open(sc, FREAD), Err(Errno::EBUSY));
    let mut c = MixerCtrl {
        dev: 1,
        type_: AUDIO_MIXER_VALUE,
        ..MixerCtrl::default()
    };
    c.un.value_mut().num_channels = 2;
    c.un.value_mut().level = [50, 50, 0, 0, 0, 0, 0, 0];
    audio_mixer_set(sc, &mut c, &PROC0).unwrap();
    assert_eq!(MASTER.load(Ordering::Relaxed), 50);
    assert!(sc.mix_pending.get().is_some());

    let mut out = [0u8; 8];
    let mut iov = [Iovec {
        iov_base: out.as_mut_ptr().cast(),
        iov_len: out.len(),
    }];
    let mut uio = uio_over(&mut iov, UioRw::UIO_READ);
    audio_mixer_read(sc, &mut uio, 0).unwrap();
    // One event: control 1.
    assert_eq!(uio.uio_resid, 4);
    assert_eq!(i32::from_ne_bytes([out[0], out[1], out[2], out[3]]), 1);
    assert!(sc.mix_pending.get().is_none());

    // The volume keys: two presses up, run the task's body by hand.
    sc.spkr.val_pending.set(2);
    wskbd_mixer_update(sc, &sc.spkr);
    assert_eq!(MASTER.load(Ordering::Relaxed), 50 + 2 * 8);
    audio_mixer_close(sc, FREAD).unwrap();
    assert!(sc.mix_pending.get().is_none());
    assert!(sc.mix_evbuf().iter().all(|e| e.pending.get() == 0));
}

#[test]
fn ioctl_args_in_place_and_copied() {
    // Aligned: in place.
    let mut words = vec![0u32; 8];
    // SAFETY: a view of the vector's bytes, which the vector owns.
    let bytes = unsafe { slice::from_raw_parts_mut(words.as_mut_ptr().cast::<u8>(), 32) };
    audio_ioctl_arg(bytes, |s: &mut AudioStatus| s.active = 7);
    assert_eq!(words[2], 7);
    // Misaligned: a copy, written back.
    let mut raw: Vec<u8> = vec![0; 33];
    audio_ioctl_arg(&mut raw[1..], |s: &mut AudioStatus| s.pause = 3);
    assert_eq!(i32::from_ne_bytes([raw[5], raw[6], raw[7], raw[8]]), 3);
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn values_match_the_c_file() {
    let defs = crate::reftest::defines("sys/dev/audio.c");
    let ours = crate::reftest::assert_defines!(defs;
        AUDIO_DEV_AUDIO, AUDIO_DEV_AUDIOCTL, AUDIO_BUFSZ, MIXER_RECORD, MIXER_RECORD_ENABLE,
        MIXER_RECORD_ENABLE_OFF, MIXER_RECORD_ENABLE_ON, MIXER_RECORD_ENABLE_SYSCTL,
        WSKBD_MUTE_TOGGLE, WSKBD_MUTE_DISABLE, WSKBD_MUTE_ENABLE);
    crate::reftest::assert_complete(&defs, "MIXER_", &ours);
    crate::reftest::assert_complete(&defs, "WSKBD_", &ours);
}
