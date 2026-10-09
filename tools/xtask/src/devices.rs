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
/* </LICENSES> */

/* <CODE> */
//! M12's QEMU devices: USB through `qemu-xhci` (or, M16b, another host controller) and
//! audio through Intel HDA or AC97, and M16b's USB pointers, smart card reader and USB audio.
//!
//! `smoke` and `qemu` take, besides the flags every boot has:
//! - `--usb`: a `qemu-xhci` controller with a `usb-storage` stick and a `usb-kbd` on its
//!   root hub (`xhci(4)`, `umass(4)`, `ukbd(4)`). The stick is a fresh raw image next to the
//!   boot image (`<image>.usb`, see [`stick_path`]): an MBR with one FAT32 partition (type
//!   0x0c, what `newfs_msdos` makes on a real stick) holding [`STICK_NOTE`] and [`STICK_BIG`],
//!   the latter [`BIG_LEN`] bytes of a fixed pseudo-random sequence whose POSIX `cksum(1)` is
//!   printed when the image is made. The kernel spoofs the partition as `i` (`spoofmbr`).
//! - `--usb-mouse`, `--usb-tablet`, `--usb-wacom-tablet` and `--usb-ccid` (M16b; each repeats
//!   nothing and combines with the others and with `--usb`): QEMU's `usb-mouse` (relative),
//!   `usb-tablet` (absolute) and `usb-wacom-tablet` (a PenPartner) pointers and its `usb-ccid`
//!   smart card reader on the [`UsbHc`] bus (full speed devices: `xhci`, `uhci` or `ohci`) (`ums(4)`, `uwacom(4)`, `ugen(4)`); any of them
//!   brings the controller, with the stick and the `usb-kbd` only if `--usb` is also given. The
//!   pointers register with the guest in this order (`mouse_set N` in the monitor picks the
//!   one `mouse_move` and `mouse_button` drive; `--monitor-after`, hwopts.rs).
//! - `--usb-net` (M16b, `cdce(4)`): QEMU's `usb-net` on the [`UsbHc`] bus (full speed: `xhci`,
//!   `uhci` or `ohci`; it needs no `--usb`) as the NIC on the user network (netdev `n0`), in vio0's place:
//!   the run has no virtio NIC, the way `--nic` has none ([`usb_net`] tells `boot.rs`).
//!   QEMU's `usb-net` offers two configurations, RNDIS first and CDC Ethernet second;
//!   `usbd_probe_and_attach` tries each in turn, and cdce(4) takes the second.
//! - `--usb-serial FILE` (M16b, `ucom(4)` over `uftdi(4)`): QEMU's `usb-serial` (an FTDI
//!   FT232 at full speed: `xhci`, `uhci` or `ohci`) on the [`UsbHc`] bus. Its chardev is a Unix socket
//!   server in the run directory whose `logfile` is FILE: everything the guest writes to the
//!   serial port is logged there (made afresh each run), connected client or not. The device
//!   is `always-plugged=on`: otherwise QEMU plugs it only while a client is connected.
//!   `--usb-serial-send-after LINE --usb-serial-send TEXT` (repeatable; `\n` in TEXT is a
//!   newline) connects to the socket once the serial console has LINE, writes TEXT and
//!   closes: what the guest reads from the port. (A file chardev with an input file would
//!   deliver the text at once, before the guest opens the port, and the line discipline drops
//!   input on a tty that is not open.) `--expect-usb-serial TEXT` (repeatable, `smoke`)
//!   requires FILE to contain TEXT once the serial expectations passed ([`after_smoke`]).
//! - `--usb-hc xhci|ehci|uhci|ohci` (implies `--usb`, M16b): the host controller [`UsbHc`] the
//!   devices sit on, `xhci` by default. `ehci` is QEMU's `usb-ehci` (an ICH4 EHCI function,
//!   `ehci(4)`) with the stick alone: QEMU refuses a full speed device such as `usb-kbd` on a
//!   high speed EHCI port with no companion controller ("speed mismatch"), so the keyboard
//!   stays off that bus. `uhci` is QEMU's `piix3-usb-uhci` (a PIIX3 UHCI function, `uhci(4)`),
//!   full speed, so the keyboard sits beside the stick. `ohci` is QEMU's `pci-ohci` (an
//!   Apple KeyLargo OHCI function, three full speed ports, `ohci(4)`): the stick and the
//!   keyboard both fit. A controller is one arm of
//!   [`UsbHc`]'s matches (its QEMU device and whether the keyboard fits on it).
//! - `--audio hda`, `--audio ac97` or `--audio usb`: QEMU's `wav` audio backend writes what
//!   the guest plays to `<image>.wav` (removed first), through `intel-hda` + `hda-output`
//!   (`azalia(4)`), `AC97` (`auich(4)`) or (M16b) a `usb-audio` speaker on the
//!   [`UsbHc`] bus (full speed: `xhci`, `uhci` or `ohci`) (`uaudio(4)`; the controller comes with it, the stick and the `usb-kbd` only with
//!   `--usb`).
//! - `--audio es1370` (M16d, amd64): QEMU's `ES1370`, an Ensoniq AudioPCI (`eap(4)`), into
//!   the same backend. `--pcspk` (M16d, amd64): the PC speaker (`pcppi(4)`, i8254 channel 2
//!   gated by port 0x61) is heard through the same backend (`-machine pcspk-audiodev=snd0`),
//!   with or without `--audio`, so `--expect-tone` checks the console bell.
//! - `--speakers` (with `--audio` or `--pcspk`): QEMU's `coreaudio` backend instead of `wav`, so what the
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
    /// `usb-audio` on the [`UsbHc`] bus: `uaudio(4)`.
    Usb,
    /// `ES1370`, an Ensoniq AudioPCI: `eap(4)` (M16d, amd64).
    Es1370,
}

/// The USB host controller `--usb-hc` puts the devices on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum UsbHc {
    /// `qemu-xhci`: `xhci(4)` (M12), the stick and the keyboard.
    #[default]
    Xhci,
    /// `usb-ehci`: `ehci(4)` (M16b), the stick only (a full speed keyboard does not fit).
    Ehci,
    /// `piix3-usb-uhci`: `uhci(4)` (M16b), the stick and the keyboard (full speed).
    Uhci,
    /// `pci-ohci`: `ohci(4)` (M16b), the stick and the keyboard (full speed ports).
    Ohci,
}

impl UsbHc {
    /// The `--usb-hc` value.
    fn parse(s: Option<&str>) -> Result<UsbHc> {
        match s {
            Some("xhci") => Ok(UsbHc::Xhci),
            Some("ehci") => Ok(UsbHc::Ehci),
            Some("uhci") => Ok(UsbHc::Uhci),
            Some("ohci") => Ok(UsbHc::Ohci),
            other => {
                Err(format!("--usb-hc {other:?}: expected `xhci`, `ehci`, `uhci` or `ohci`").into())
            }
        }
    }

    /// QEMU's controller device, with the id [`USB_HC_ID`] its bus is named after.
    fn qemu_device(self) -> String {
        let dev = match self {
            UsbHc::Xhci => "qemu-xhci",
            UsbHc::Ehci => "usb-ehci",
            UsbHc::Uhci => "piix3-usb-uhci",
            UsbHc::Ohci => "pci-ohci",
        };
        format!("{dev},id={USB_HC_ID}")
    }

    /// Whether QEMU's full speed `usb-kbd` can sit on the controller's root hub.
    fn takes_full_speed(self) -> bool {
        match self {
            UsbHc::Xhci | UsbHc::Uhci | UsbHc::Ohci => true,
            UsbHc::Ehci => false,
        }
    }
}

/// The QEMU id of the `--usb` host controller; its root hub's bus is `<id>.0`.
const USB_HC_ID: &str = "usbhc";

/// The M12 devices of this run (set once by `main`, read when QEMU's command line is made).
#[derive(Clone, Debug, Default)]
pub(crate) struct Devices {
    /// `--usb` (or `--usb-hc`).
    pub usb: bool,
    /// `--usb-hc xhci|ehci|ohci`.
    pub usb_hc: UsbHc,
    /// `--usb-mouse`.
    pub usb_mouse: bool,
    /// `--usb-tablet`.
    pub usb_tablet: bool,
    /// `--usb-wacom-tablet`.
    pub usb_wacom: bool,
    /// `--usb-ccid`.
    pub usb_ccid: bool,
    /// `--usb-net`.
    pub usb_net: bool,
    /// `--usb-serial FILE`: the file's name in the run directory.
    pub usb_serial: Option<String>,
    /// `--usb-serial-send-after LINE --usb-serial-send TEXT`, with `\n` turned into newlines.
    pub usb_serial_send: Vec<(String, String)>,
    /// `--expect-usb-serial TEXT`, each.
    pub expect_usb_serial: Vec<String>,
    /// `--audio hda|ac97|usb|es1370`.
    pub audio: Option<Audio>,
    /// `--pcspk` (M16d): the PC speaker's sound goes to the audio backend.
    pub pcspk: bool,
    /// `--expect-tone`.
    pub expect_tone: bool,
    /// `--speakers`.
    pub speakers: bool,
}

static DEVICES: OnceLock<Devices> = OnceLock::new();

/// Parses `--usb`, `--usb-hc <xhci|ehci|uhci|ohci>`, `--usb-mouse`, `--usb-tablet`,
/// `--usb-wacom-tablet`, `--usb-ccid`, `--usb-net`, `--usb-serial`,
/// `--usb-serial-send-after`/`--usb-serial-send`, `--expect-usb-serial`,
/// `--audio <hda|ac97|usb|es1370>`, `--pcspk`, `--speakers` and `--expect-tone` and records
/// them for the run.
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
            Some("usb") => Some(Audio::Usb),
            Some("es1370") => Some(Audio::Es1370),
            other => {
                return Err(format!(
                    "--audio {other:?}: expected `hda`, `ac97`, `usb` or `es1370`"
                )
                .into());
            }
        },
    };
    let pcspk = args.contains(&"--pcspk");
    let arm64 = args.windows(2).any(|w| w == ["--arch", "arm64"]);
    if arm64 && (pcspk || audio == Some(Audio::Es1370)) {
        return Err(
            "--pcspk and --audio es1370: amd64 only (arm64's GENERIC has no pcppi or eap)".into(),
        );
    }
    let expect_tone = args.contains(&"--expect-tone");
    if expect_tone && audio.is_none() && !pcspk {
        return Err("--expect-tone needs --audio or --pcspk".into());
    }
    let speakers = args.contains(&"--speakers");
    if speakers && audio.is_none() && !pcspk {
        return Err("--speakers needs --audio or --pcspk".into());
    }
    if speakers && expect_tone {
        return Err("--speakers records nothing for --expect-tone".into());
    }
    let usb_hc = match args.iter().position(|a| *a == "--usb-hc") {
        None => None,
        Some(i) => Some(UsbHc::parse(args.get(i + 1).copied())?),
    };
    let value = |opt: &str| -> Result<Option<String>> {
        match args.iter().position(|a| *a == opt) {
            None => Ok(None),
            Some(i) => match args.get(i + 1) {
                Some(v) => Ok(Some((*v).to_string())),
                None => Err(format!("{opt}: expected a value").into()),
            },
        }
    };
    let usb_serial = value("--usb-serial")?;
    let usb_serial_send: Vec<(String, String)> =
        crate::hwopts::parse_pairs(args, "--usb-serial-send-after", "--usb-serial-send")?
            .into_iter()
            .map(|(line, text)| (line, text.replace("\\n", "\n")))
            .collect();
    let expect_usb_serial: Vec<String> = args
        .windows(2)
        .filter(|w| w[0] == "--expect-usb-serial")
        .map(|w| w[1].to_string())
        .collect();
    if usb_serial.is_none() && (!usb_serial_send.is_empty() || !expect_usb_serial.is_empty()) {
        return Err("--usb-serial-send and --expect-usb-serial need --usb-serial".into());
    }
    Ok(Devices {
        usb: args.contains(&"--usb") || usb_hc.is_some(),
        usb_hc: usb_hc.unwrap_or_default(),
        usb_mouse: args.contains(&"--usb-mouse"),
        usb_tablet: args.contains(&"--usb-tablet"),
        usb_wacom: args.contains(&"--usb-wacom-tablet"),
        usb_ccid: args.contains(&"--usb-ccid"),
        usb_net: args.contains(&"--usb-net"),
        usb_serial,
        usb_serial_send,
        expect_usb_serial,
        audio,
        pcspk,
        expect_tone,
        speakers,
    })
}

fn devices() -> Devices {
    DEVICES.get().cloned().unwrap_or_default()
}

/// The `--usb-serial` chardev's socket for the run directory of `image`.
fn usb_serial_sock(image: &Path) -> PathBuf {
    crate::hwopts::run_sock(image.parent().unwrap_or(Path::new(".")), "usbser.sock")
}

/// How many `--usb-serial-send` texts were written.
static SERIAL_SENT: std::sync::Mutex<usize> = std::sync::Mutex::new(0);

/// Whether a `--usb-serial-send` text is still to be written.
pub(crate) fn usb_serial_pending() -> bool {
    let sent = SERIAL_SENT.lock().map(|s| *s).unwrap_or(0);
    sent < devices().usb_serial_send.len()
}

/// Writes the next `--usb-serial-send` text to the port once `serial` has its
/// `--usb-serial-send-after` line.
pub(crate) fn poll_usb_serial(serial: &str, image: &Path) -> Result<()> {
    use std::os::unix::net::UnixStream;
    let d = devices();
    let sent = SERIAL_SENT.lock().map(|s| *s).unwrap_or(0);
    let Some((after, text)) = d.usb_serial_send.get(sent) else {
        return Ok(());
    };
    if !serial.contains(after.as_str()) {
        return Ok(());
    }
    let sock = usb_serial_sock(image);
    let mut s = UnixStream::connect(&sock).map_err(|e| format!("{}: {e}", sock.display()))?;
    s.write_all(text.as_bytes())?;
    drop(s);
    println!("xtask: usb-serial gets {text:?} (saw {after:?})");
    if let Ok(mut n) = SERIAL_SENT.lock() {
        *n += 1;
    }
    Ok(())
}

/// Whether this run has `--usb-net`: the user network's NIC is then the USB one, and the
/// virtio (or `--nic`) NIC is left out of QEMU's command line.
pub(crate) fn usb_net() -> bool {
    devices().usb_net
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
    let extras = d.usb_mouse
        || d.usb_tablet
        || d.usb_wacom
        || d.usb_ccid
        || d.usb_net
        || d.usb_serial.is_some()
        || d.audio == Some(Audio::Usb);
    if extras && !d.usb_hc.takes_full_speed() {
        return Err(
            "--usb-mouse, --usb-tablet, --usb-wacom-tablet, --usb-ccid, --usb-net, \
                    --usb-serial and --audio usb are full speed devices: they need a controller \
                    with full speed ports (--usb-hc xhci, uhci or ohci)"
                .into(),
        );
    }
    if d.usb || extras {
        args.extend(["-device".to_string(), d.usb_hc.qemu_device()]);
    }
    if d.usb {
        let stick = stick_path(image);
        make_stick(&stick)?;
        args.extend([
            "-drive".to_string(),
            format!("if=none,id=usbstick,format=raw,file={}", stick.display()),
            "-device".to_string(),
            format!("usb-storage,bus={USB_HC_ID}.0,drive=usbstick"),
        ]);
        if d.usb_hc.takes_full_speed() {
            args.extend(["-device".to_string(), format!("usb-kbd,bus={USB_HC_ID}.0")]);
        }
    }
    for (on, dev) in [
        (d.usb_mouse, "usb-mouse"),
        (d.usb_tablet, "usb-tablet"),
        (d.usb_wacom, "usb-wacom-tablet"),
        (d.usb_ccid, "usb-ccid"),
    ] {
        if on {
            args.extend(["-device".to_string(), format!("{dev},bus={USB_HC_ID}.0")]);
        }
    }
    if d.usb_net {
        args.extend([
            "-device".to_string(),
            format!("usb-net,netdev=n0,bus={USB_HC_ID}.0"),
        ]);
    }
    if let Some(name) = &d.usb_serial {
        let out = image.with_file_name(name);
        // QEMU opens the log file for writing when it starts; a stale one would only matter
        // if QEMU died before that.
        let _ = fs::remove_file(&out);
        let sock = usb_serial_sock(image);
        let _ = fs::remove_file(&sock);
        args.extend([
            "-chardev".to_string(),
            format!(
                "socket,id=usbser0,path={},server=on,wait=off,logfile={}",
                sock.display(),
                out.display()
            ),
            "-device".to_string(),
            format!("usb-serial,chardev=usbser0,bus={USB_HC_ID}.0,always-plugged=on"),
        ]);
    }
    if d.audio.is_some() || d.pcspk {
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
        if d.pcspk {
            // QEMU merges the `-machine` options: this adds to the `-M q35` (or `pc`) given.
            args.extend(["-machine", "pcspk-audiodev=snd0"].map(String::from));
        }
    }
    if let Some(audio) = d.audio {
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
            Audio::Usb => args.extend([
                "-device".to_string(),
                format!("usb-audio,bus={USB_HC_ID}.0,audiodev=snd0"),
            ]),
            Audio::Es1370 => args.extend(["-device", "ES1370,audiodev=snd0"].map(String::from)),
        }
    }
    Ok(args)
}

/// What a run must leave behind once its serial expectations passed: with `--expect-tone`,
/// a tone in the WAV file of `image`.
pub(crate) fn after_smoke(image: &Path) -> Result<()> {
    let d = devices();
    if let Some(name) = &d.usb_serial {
        let file = image.with_file_name(name);
        let bytes = fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        let text = String::from_utf8_lossy(&bytes);
        for want in &d.expect_usb_serial {
            if !text.contains(want.as_str()) {
                return Err(
                    format!("{}: {want:?} not written by the guest", file.display()).into(),
                );
            }
            println!("xtask: {}: has {want:?}", file.display());
        }
    }
    if !d.expect_tone {
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

/// `diff-openbsd probe` (M16d): what the WAV file of `image` holds, printed whatever it is
/// (a probe records what OpenBSD does; nothing is required).
pub(crate) fn probe_report(image: &Path) {
    let d = devices();
    if d.audio.is_none() && !d.pcspk {
        return;
    }
    let wav = wav_path(image);
    match fs::read(&wav)
        .map_err(|e| e.to_string())
        .and_then(|b| tone(&b))
    {
        Ok(t) => println!(
            "xtask: {}: {} Hz, {} channel(s), {} frames, {} loud sample(s), peak {}",
            wav.display(),
            t.rate,
            t.channels,
            t.frames,
            t.loud,
            t.peak
        ),
        Err(e) => println!("xtask: {}: {e}", wav.display()),
    }
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
    fn usb_pointer_options() {
        let d = parse(&["--usb-mouse", "--usb-ccid"]).unwrap();
        assert!(d.usb_mouse && d.usb_ccid && !d.usb_tablet && !d.usb_wacom && !d.usb);
        let d = parse(&["--usb-tablet", "--usb-wacom-tablet", "--usb"]).unwrap();
        assert!(d.usb_tablet && d.usb_wacom && d.usb && !d.usb_mouse);
    }

    #[test]
    fn usb_net_and_serial_options() {
        let d = parse(&["--usb-net"]).unwrap();
        assert!(d.usb_net && !d.usb && d.usb_serial.is_none());
        let d = parse(&[
            "--usb-serial",
            "ser.txt",
            "--usb-serial-send-after",
            "ready",
            "--usb-serial-send",
            "hello\\nworld\\n",
            "--expect-usb-serial",
            "a",
            "--expect-usb-serial",
            "b",
        ])
        .unwrap();
        assert_eq!(d.usb_serial.as_deref(), Some("ser.txt"));
        assert_eq!(
            d.usb_serial_send,
            [("ready".to_string(), "hello\nworld\n".to_string())]
        );
        assert_eq!(d.expect_usb_serial, ["a", "b"]);
        assert!(parse(&["--expect-usb-serial", "a"]).is_err());
        assert!(parse(&["--usb-serial-send-after", "a", "--usb-serial-send", "b"]).is_err());
        assert!(parse(&["--usb-serial"]).is_err());
    }

    #[test]
    fn speakers_flag() {
        let d = parse(&["--audio", "hda", "--speakers"]).unwrap();
        assert!(d.speakers && !d.expect_tone);
        assert_eq!(d.audio, Some(Audio::Hda));
        assert!(!parse(&["--audio", "ac97"]).unwrap().speakers);
        assert_eq!(parse(&["--audio", "usb"]).unwrap().audio, Some(Audio::Usb));
        assert!(parse(&["--audio", "sb"]).is_err());
        assert!(parse(&["--speakers"]).is_err());
        assert!(parse(&["--audio", "hda", "--speakers", "--expect-tone"]).is_err());
    }

    #[test]
    fn pcspk_and_es1370() {
        let d = parse(&["--arch", "amd64", "--pcspk", "--expect-tone"]).unwrap();
        assert!(d.pcspk && d.expect_tone && d.audio.is_none());
        let d = parse(&["--audio", "es1370", "--expect-tone"]).unwrap();
        assert_eq!(d.audio, Some(Audio::Es1370));
        assert!(!d.pcspk);
        assert!(parse(&["--arch", "arm64", "--pcspk"]).is_err());
        assert!(parse(&["--arch", "arm64", "--audio", "es1370"]).is_err());
    }

    #[test]
    fn usb_hc_flag() {
        let d = parse(&["--usb"]).unwrap();
        assert!(d.usb);
        assert_eq!(d.usb_hc, UsbHc::Xhci);
        let d = parse(&["--usb-hc", "ehci"]).unwrap();
        assert!(d.usb);
        assert_eq!(d.usb_hc, UsbHc::Ehci);
        assert_eq!(UsbHc::Ehci.qemu_device(), "usb-ehci,id=usbhc");
        assert!(!UsbHc::Ehci.takes_full_speed());
        assert!(UsbHc::Xhci.takes_full_speed());
        let d = parse(&["--usb-hc", "uhci"]).unwrap();
        assert!(d.usb);
        assert_eq!(d.usb_hc, UsbHc::Uhci);
        assert_eq!(UsbHc::Uhci.qemu_device(), "piix3-usb-uhci,id=usbhc");
        assert!(UsbHc::Uhci.takes_full_speed());

        let d = parse(&["--usb-hc", "ohci"]).unwrap();
        assert!(d.usb);
        assert_eq!(d.usb_hc, UsbHc::Ohci);
        assert_eq!(UsbHc::Ohci.qemu_device(), "pci-ohci,id=usbhc");
        assert!(UsbHc::Ohci.takes_full_speed());
        assert!(parse(&["--usb-hc"]).is_err());
        assert!(parse(&["--usb-hc", "fhci"]).is_err());
        assert!(!parse(&[]).unwrap().usb);
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
