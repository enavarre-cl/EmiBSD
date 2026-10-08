/* <CODE> */
//! M12's QEMU devices: USB through `qemu-xhci` and audio through Intel HDA or AC97.
//!
//! `smoke` and `qemu` take, besides the flags every boot has:
//! - `--usb`: a `qemu-xhci` controller with a `usb-storage` stick and a `usb-kbd` on its
//!   root hub (`xhci(4)`, `umass(4)`, `ukbd(4)`). The stick is a fresh raw image next to the
//!   boot image (`<image>.usb`, see [`stick_path`]): an MBR with one FAT32 partition (type
//!   0x0c, what `newfs_msdos` makes on a real stick) holding [`STICK_NOTE`] and [`STICK_BIG`],
//!   the latter [`BIG_LEN`] bytes of a fixed pseudo-random sequence whose POSIX `cksum(1)` is
//!   printed when the image is made. The kernel spoofs the partition as `i` (`spoofmbr`).
//! - `--audio hda` or `--audio ac97`: QEMU's `wav` audio backend writes what the guest plays
//!   to `<image>.wav` (removed first), through `intel-hda` + `hda-output` (`azalia(4)`) or
//!   `AC97` (`auich(4)`).
//! - `--speakers` (with `--audio`): QEMU's `coreaudio` backend instead of `wav`, so what the
//!   guest plays comes out of the Mac's speakers; nothing is recorded, so it excludes
//!   `--expect-tone` (`just play-audio`, by ear, outside `smoke`).
//! - `--expect-tone`: after a successful run, the WAV file must hold a tone: at least a
//!   tenth of a second of samples louder than [`TONE_THRESHOLD`]. QEMU writes the WAV header's
//!   sizes only on a clean exit, so the data chunk runs to the end of the file whatever the
//!   header says (a smoke that stops `--until-seen` kills QEMU).
//!
//! The paths follow the boot image's, which lives in the run directory (`boot::run_dir`,
//! `$EMIBSD_RUN_DIR`: `target/smoke/<recipe>/` under `smoke-all`), so parallel recipes never
//! share a stick or a WAV file.

use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::Result;

/// Size of the USB stick: 64 MiB, enough for FAT32 with one-sector clusters.
const STICK_SIZE: u64 = 64 * 1024 * 1024;

/// First sector of the stick's FAT partition (1 MiB aligned, as fdisk(8) would put it).
const STICK_PART_START: u64 = 2048;

/// The small file on the stick: `cat` shows this text.
pub(crate) const STICK_NOTE: &str = "M12USB.TXT";

/// [`STICK_NOTE`]'s contents.
pub(crate) const STICK_NOTE_TEXT: &str = "emibsd m12: hello from a usb stick\n";

/// The large file on the stick: read back through several bulk transfers.
pub(crate) const STICK_BIG: &str = "BIG.BIN";

/// [`STICK_BIG`]'s length: 1 MiB.
pub(crate) const BIG_LEN: usize = 1024 * 1024;

/// `cksum(1)` of [`STICK_BIG`] (the `big_file_cksum` test checks it).
pub(crate) const BIG_CKSUM: u32 = 4_071_711_340;

/// A sample louder than this (of 32767) counts as sound for `--expect-tone`.
const TONE_THRESHOLD: i32 = 1000;

/// The audio device `--audio` asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Audio {
    /// `intel-hda` with an `hda-output` codec: `azalia(4)`.
    Hda,
    /// `AC97`: `auich(4)`.
    Ac97,
}

/// The M12 devices of this run (set once by `main`, read when QEMU's command line is made).
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Devices {
    /// `--usb`.
    pub usb: bool,
    /// `--audio hda|ac97`.
    pub audio: Option<Audio>,
    /// `--expect-tone`.
    pub expect_tone: bool,
    /// `--speakers`.
    pub speakers: bool,
}

static DEVICES: OnceLock<Devices> = OnceLock::new();

/// Parses `--usb`, `--audio <hda|ac97>`, `--speakers` and `--expect-tone` and records them
/// for the run.
pub(crate) fn set_from_args(args: &[&str]) -> Result<()> {
    let _ = DEVICES.set(parse(args)?);
    Ok(())
}

/// [`set_from_args`]'s parser.
fn parse(args: &[&str]) -> Result<Devices> {
    let audio = match args.iter().position(|a| *a == "--audio") {
        None => None,
        Some(i) => match args.get(i + 1).copied() {
            Some("hda") => Some(Audio::Hda),
            Some("ac97") => Some(Audio::Ac97),
            other => return Err(format!("--audio {other:?}: expected `hda` or `ac97`").into()),
        },
    };
    let expect_tone = args.contains(&"--expect-tone");
    if expect_tone && audio.is_none() {
        return Err("--expect-tone needs --audio".into());
    }
    let speakers = args.contains(&"--speakers");
    if speakers && audio.is_none() {
        return Err("--speakers needs --audio".into());
    }
    if speakers && expect_tone {
        return Err("--speakers records nothing for --expect-tone".into());
    }
    Ok(Devices {
        usb: args.contains(&"--usb"),
        audio,
        expect_tone,
        speakers,
    })
}

fn devices() -> Devices {
    DEVICES.get().copied().unwrap_or_default()
}

/// The USB stick of the boot image `image`: `<image>.usb`.
pub(crate) fn stick_path(image: &Path) -> PathBuf {
    image.with_extension("usb")
}

/// The WAV file of the boot image `image`: `<image>.wav`.
pub(crate) fn wav_path(image: &Path) -> PathBuf {
    image.with_extension("wav")
}

/// The QEMU arguments for this run's M12 devices, booting `image`; makes the USB stick and
/// removes an old WAV file first. Empty when no M12 device was asked for.
pub(crate) fn qemu_args(image: &Path) -> Result<Vec<String>> {
    let d = devices();
    let mut args = Vec::new();
    if d.usb {
        let stick = stick_path(image);
        make_stick(&stick)?;
        args.extend([
            "-device".to_string(),
            "qemu-xhci,id=xhci".to_string(),
            "-drive".to_string(),
            format!("if=none,id=usbstick,format=raw,file={}", stick.display()),
            "-device".to_string(),
            "usb-storage,bus=xhci.0,drive=usbstick".to_string(),
            "-device".to_string(),
            "usb-kbd,bus=xhci.0".to_string(),
        ]);
    }
    if let Some(audio) = d.audio {
        args.push("-audiodev".to_string());
        if d.speakers {
            args.push("coreaudio,id=snd0".to_string());
        } else {
            let wav = wav_path(image);
            if wav.exists() {
                fs::remove_file(&wav).map_err(|e| format!("{}: {e}", wav.display()))?;
            }
            args.push(format!("wav,id=snd0,path={}", wav.display()));
        }
        match audio {
            Audio::Hda => args.extend(
                [
                    "-device",
                    "intel-hda",
                    "-device",
                    "hda-output,audiodev=snd0",
                ]
                .map(String::from),
            ),
            Audio::Ac97 => args.extend(["-device", "AC97,audiodev=snd0"].map(String::from)),
        }
    }
    Ok(args)
}

/// What a run must leave behind once its serial expectations passed: with `--expect-tone`,
/// a tone in the WAV file of `image`.
pub(crate) fn after_smoke(image: &Path) -> Result<()> {
    if !devices().expect_tone {
        return Ok(());
    }
    let wav = wav_path(image);
    let bytes = fs::read(&wav).map_err(|e| format!("{}: {e}", wav.display()))?;
    let t = tone(&bytes).map_err(|e| format!("{}: {e}", wav.display()))?;
    println!(
        "xtask: {}: {} Hz, {} channel(s), {} frames, {} loud sample(s), peak {}",
        wav.display(),
        t.rate,
        t.channels,
        t.frames,
        t.loud,
        t.peak
    );
    if t.loud < u64::from(t.rate / 10) {
        return Err(format!(
            "{}: silent: {} sample(s) above {TONE_THRESHOLD}, at least {} expected",
            wav.display(),
            t.loud,
            t.rate / 10
        )
        .into());
    }
    Ok(())
}

/// What [`tone`] measured in a WAV file.
#[derive(Debug, PartialEq, Eq)]
struct Tone {
    rate: u32,
    channels: u16,
    frames: u64,
    loud: u64,
    peak: i32,
}

/// Measures the 16-bit PCM data of a WAV file. The data chunk is taken to the end of the
/// file (see the module docs).
fn tone(bytes: &[u8]) -> std::result::Result<Tone, String> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("not a RIFF WAVE file".into());
    }
    let mut at = 12;
    let mut fmt: Option<(u16, u16, u32, u16)> = None;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let len = u32::from_le_bytes([bytes[at + 4], bytes[at + 5], bytes[at + 6], bytes[at + 7]])
            as usize;
        let body = at + 8;
        if id == b"fmt " {
            let f = bytes.get(body..body + 16).ok_or("short fmt chunk")?;
            fmt = Some((
                u16::from_le_bytes([f[0], f[1]]),
                u16::from_le_bytes([f[2], f[3]]),
                u32::from_le_bytes([f[4], f[5], f[6], f[7]]),
                u16::from_le_bytes([f[14], f[15]]),
            ));
        } else if id == b"data" {
            let (format, channels, rate, bits) = fmt.ok_or("data before fmt")?;
            if format != 1 || bits != 16 || channels == 0 {
                return Err(format!(
                    "format {format}, {bits} bits, {channels} channels: not 16-bit PCM"
                ));
            }
            let data = &bytes[body..];
            let mut loud = 0u64;
            let mut peak = 0i32;
            for s in data.as_chunks::<2>().0 {
                let v = i32::from(i16::from_le_bytes(*s)).abs();
                peak = peak.max(v);
                if v > TONE_THRESHOLD {
                    loud += 1;
                }
            }
            return Ok(Tone {
                rate,
                channels,
                frames: (data.len() / 2 / usize::from(channels)) as u64,
                loud,
                peak,
            });
        }
        at = body + len + (len & 1);
    }
    Err("no data chunk".into())
}

/// The bytes of [`STICK_BIG`]: a 32-bit xorshift sequence from a fixed seed.
pub(crate) fn big_contents() -> Vec<u8> {
    let mut x: u32 = 0x1234_5678;
    let mut v = Vec::with_capacity(BIG_LEN);
    while v.len() < BIG_LEN {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        v.extend_from_slice(&x.to_le_bytes());
    }
    v.truncate(BIG_LEN);
    v
}

/// POSIX `cksum(1)`: the CRC (polynomial 0x04c11db7, MSB first) of the data followed by its
/// length in as few bytes as it takes, least significant first, complemented.
pub(crate) fn posix_cksum(data: &[u8]) -> u32 {
    fn step(mut crc: u32, b: u8) -> u32 {
        crc ^= u32::from(b) << 24;
        for _ in 0..8 {
            crc = if crc & 0x8000_0000 != 0 {
                (crc << 1) ^ 0x04c1_1db7
            } else {
                crc << 1
            };
        }
        crc
    }
    let mut crc = data.iter().fold(0u32, |c, &b| step(c, b));
    let mut n = data.len();
    while n != 0 {
        crc = step(crc, (n & 0xff) as u8);
        n >>= 8;
    }
    !crc
}

/// Makes the USB stick at `path`, replacing any old one.
fn make_stick(path: &Path) -> Result<()> {
    let part_len = STICK_SIZE - STICK_PART_START * 512;
    let mut part = vec![0u8; part_len as usize];
    {
        let mut cur = Cursor::new(&mut part[..]);
        fatfs::format_volume(
            &mut cur,
            fatfs::FormatVolumeOptions::new()
                .fat_type(fatfs::FatType::Fat32)
                .bytes_per_cluster(4096)
                .volume_label(*b"EMIBSD USB "),
        )?;
        let fs = fatfs::FileSystem::new(&mut cur, fatfs::FsOptions::new())?;
        {
            let root = fs.root_dir();
            root.create_file(STICK_NOTE)?
                .write_all(STICK_NOTE_TEXT.as_bytes())?;
            root.create_file(STICK_BIG)?.write_all(&big_contents())?;
        }
        fs.unmount()?;
    }
    let mut image = Vec::with_capacity(STICK_SIZE as usize);
    image.extend_from_slice(&stick_mbr(STICK_PART_START as u32, (part_len / 512) as u32));
    image.resize((STICK_PART_START * 512) as usize, 0);
    image.extend_from_slice(&part);
    fs::write(path, &image).map_err(|e| format!("{}: {e}", path.display()))?;
    let sum = posix_cksum(&big_contents());
    if sum != BIG_CKSUM {
        return Err(format!("{STICK_BIG}: cksum {sum}, the smokes expect {BIG_CKSUM}").into());
    }
    println!(
        "xtask: {} (USB stick: FAT32, {STICK_NOTE}, {STICK_BIG} cksum {sum} {BIG_LEN})",
        path.display()
    );
    Ok(())
}

/// A master boot record with one partition of type 0x0c (FAT32, LBA), not bootable.
fn stick_mbr(start: u32, sectors: u32) -> [u8; 512] {
    let mut sector = [0u8; 512];
    let entry = &mut sector[446..462];
    entry[1..4].copy_from_slice(&[0xfe, 0xff, 0xff]);
    entry[4] = 0x0c;
    entry[5..8].copy_from_slice(&[0xfe, 0xff, 0xff]);
    entry[8..12].copy_from_slice(&start.to_le_bytes());
    entry[12..16].copy_from_slice(&sectors.to_le_bytes());
    sector[510] = 0x55;
    sector[511] = 0xaa;
    sector
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cksum_vectors() {
        // `printf 123456789 | cksum` and `cksum </dev/null`.
        assert_eq!(posix_cksum(b"123456789"), 930_766_865);
        assert_eq!(posix_cksum(b""), 4_294_967_295);
    }

    #[test]
    fn speakers_flag() {
        let d = parse(&["--audio", "hda", "--speakers"]).unwrap();
        assert!(d.speakers && !d.expect_tone);
        assert_eq!(d.audio, Some(Audio::Hda));
        assert!(!parse(&["--audio", "ac97"]).unwrap().speakers);
        assert!(parse(&["--speakers"]).is_err());
        assert!(parse(&["--audio", "hda", "--speakers", "--expect-tone"]).is_err());
    }

    #[test]
    fn big_file_cksum() {
        // The value the smoke expects from the guest's `cksum` (justfile, `smoke-usb`).
        assert_eq!(posix_cksum(&big_contents()), BIG_CKSUM);
    }

    fn wav(samples: &[i16]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(b"RIFF\0\0\0\0WAVEfmt ");
        v.extend_from_slice(&16u32.to_le_bytes());
        v.extend_from_slice(&1u16.to_le_bytes());
        v.extend_from_slice(&2u16.to_le_bytes());
        v.extend_from_slice(&44100u32.to_le_bytes());
        v.extend_from_slice(&(44100u32 * 4).to_le_bytes());
        v.extend_from_slice(&4u16.to_le_bytes());
        v.extend_from_slice(&16u16.to_le_bytes());
        // QEMU killed before closing: the data chunk's size is still 0.
        v.extend_from_slice(b"data\0\0\0\0");
        for s in samples {
            v.extend_from_slice(&s.to_le_bytes());
        }
        v
    }

    #[test]
    fn tone_measures_unfinished_files() {
        let t = tone(&wav(&[0, 0, 5000, -6000, 10, -10])).unwrap();
        assert_eq!(
            t,
            Tone {
                rate: 44100,
                channels: 2,
                frames: 3,
                loud: 2,
                peak: 6000
            }
        );
        assert!(tone(b"RIFF\0\0\0\0WAVE").is_err());
    }

    #[test]
    fn stick_mbr_layout() {
        let s = stick_mbr(2048, 1000);
        assert_eq!(s[446 + 4], 0x0c);
        assert_eq!(&s[446 + 8..446 + 12], &2048u32.to_le_bytes());
        assert_eq!(&s[510..], &[0x55, 0xaa]);
    }
}
/* </TESTS> */
