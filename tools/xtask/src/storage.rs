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
//! M16a's storage controllers: one QEMU option per controller, each with the disk image FILE
//! it holds (a relative path is in the run directory, [`boot::run_dir`]), for `qemu`,
//! `smoke` and `diff-openbsd probe`.
//!
//! - `--ide FILE` (amd64, with `--machine pc`): an `ide-hd` on the primary channel of `pc`'s
//!   PIIX3 IDE controller (00:01.1, `ide.0`, master), for pciide(4) and wd(4). The disk is
//!   second in the firmware's boot order (`bootindex=1`): OVMF turns on the decode of a PIIX
//!   IDE channel (the `IDETIM` enable bit) only when a boot option is on it, and without it
//!   OpenBSD 8.0 prints `pciide0: channel 0 ignored (disabled)` and finds no wd0 (M16a's
//!   probe; with it, `wd0 at pciide0 channel 0 drive 0: <QEMU HARDDISK>`).
//! - `--megasas FILE`, `--megasas-gen2 FILE`: QEMU's LSI MegaRAID SAS 1078 (`megasas`) and
//!   SAS2108 (`megasas-gen2`, PCI product 0x0079, which mfi(4)'s table names "MegaRAID
//!   SAS2108 GEN2"), each with a `scsi-hd` at target 0.
//! - `--mptsas FILE`: QEMU's LSI SAS1068 (`mptsas1068`), mpi(4), with a `scsi-hd` at target 0.
//! - `--pvscsi FILE`: QEMU's VMware paravirtual SCSI (`pvscsi`), vmwpvs(4), with a `scsi-hd`.
//! - `--am53c974 FILE`, `--dc390 FILE`: QEMU's AMD Am53c974 (`am53c974`) and the Tekram
//!   DC-390 built on it (`dc390`), pcscp(4) over ncr53c9x, with a `scsi-hd` at target 0.
//! - `--ufs FILE`: QEMU's UFS host controller (`ufs`), ufshci(4), with one logical unit
//!   (`ufs-lu`, LUN 0).
//! - `--sdhci FILE`: QEMU's SD host controller on PCI (`sdhci-pci`), sdhc(4), with an
//!   `sd-card` on its bus (QEMU needs a power-of-two card size; [`DISK_BYTES`] is one).
//! - `--floppy FILE` (amd64): a 3.5" 1.44 MB drive on the ISA floppy controller (`pc` has one
//!   built in; on `q35` an `isa-fdc` is added on the LPC's ISA bus), fdc(4) and fd(4).
//!
//! The options are offered on both archs where QEMU has the device, whatever the GENERICs
//! say, so `diff-openbsd probe` can show what OpenBSD does with them. Every disk is made
//! afresh each run: [`DISK_BYTES`] of zeroes, so the session partitions it, makes a file
//! system, writes and reads it back; the floppy is a FAT12 1.44 MB image holding
//! [`FLOPPY_NOTE`] with [`FLOPPY_NOTE_TEXT`] ([`make_floppy`]), which the guest reads with
//! mount_msdos(8). The controllers go after every other device ([`add_devices`], called
//! after `hwopts::add_devices`), so no PCI slot an older smoke expects moves; on arm64 they
//! sit on `virt`'s PCI Express bus. Each controller's QEMU id and drive id are its own, so
//! several can be given at once.

use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use crate::Result;
use crate::boot::{self, Arch};

/// The size of every disk but the floppy: 64 MiB, as `--lsi`'s and `--disk-fresh`'s.
pub(crate) const DISK_BYTES: u64 = 64 << 20;

/// The floppy image's size: 80 cylinders, 2 heads, 18 sectors of 512 bytes.
const FLOPPY_BYTES: usize = 1_474_560;

/// The file on the floppy.
pub(crate) const FLOPPY_NOTE: &str = "M16A-FD.TXT";

/// What [`FLOPPY_NOTE`] holds.
pub(crate) const FLOPPY_NOTE_TEXT: &str = "m16a-fd-42\n";

/// The archs every PCI controller is offered on.
const BOTH: &[Arch] = &[Arch::Amd64, Arch::Arm64];

/// amd64 only: the ISA floppy controller and `pc`'s IDE controller.
const AMD64: &[Arch] = &[Arch::Amd64];

/// One controller option: its name, the archs it is offered on, whether it needs `pc`, and
/// the QEMU devices for the drive id `drive` on a `q35` (or not) machine (the `-drive`
/// itself is added by [`add_devices`]).
struct Hba {
    opt: &'static str,
    arches: &'static [Arch],
    needs_pc: bool,
    devices: fn(drive: &str, q35: bool) -> Vec<String>,
}

/// The controllers, in the order their devices are added.
const HBAS: &[Hba] = &[
    Hba {
        opt: "--ide",
        arches: AMD64,
        needs_pc: true,
        devices: |d, _| {
            vec![
                "-device".into(),
                format!("ide-hd,drive={d},bus=ide.0,unit=0,bootindex=1"),
            ]
        },
    },
    Hba {
        opt: "--megasas",
        arches: BOTH,
        needs_pc: false,
        devices: |d, _| scsi_hba("megasas", "m16a-mfi", d),
    },
    Hba {
        opt: "--megasas-gen2",
        arches: BOTH,
        needs_pc: false,
        devices: |d, _| scsi_hba("megasas-gen2", "m16a-mfi2", d),
    },
    Hba {
        opt: "--mptsas",
        arches: BOTH,
        needs_pc: false,
        devices: |d, _| scsi_hba("mptsas1068", "m16a-mpt", d),
    },
    Hba {
        opt: "--pvscsi",
        arches: BOTH,
        needs_pc: false,
        devices: |d, _| scsi_hba("pvscsi", "m16a-pvs", d),
    },
    Hba {
        opt: "--am53c974",
        arches: BOTH,
        needs_pc: false,
        devices: |d, _| scsi_hba("am53c974", "m16a-esp", d),
    },
    Hba {
        opt: "--dc390",
        arches: BOTH,
        needs_pc: false,
        devices: |d, _| scsi_hba("dc390", "m16a-dc390", d),
    },
    Hba {
        opt: "--ufs",
        arches: BOTH,
        needs_pc: false,
        devices: |d, _| {
            vec![
                "-device".into(),
                "ufs,id=m16a-ufs".into(),
                "-device".into(),
                format!("ufs-lu,drive={d},bus=m16a-ufs,lun=0"),
            ]
        },
    },
    Hba {
        opt: "--sdhci",
        arches: BOTH,
        needs_pc: false,
        devices: |d, _| {
            vec![
                "-device".into(),
                "sdhci-pci,id=m16a-sdhci".into(),
                "-device".into(),
                format!("sd-card,drive={d}"),
            ]
        },
    },
    Hba {
        opt: "--floppy",
        arches: AMD64,
        needs_pc: false,
        devices: |d, q35| {
            let mut a = Vec::new();
            if q35 {
                a.push("-device".into());
                a.push("isa-fdc,id=m16a-fdc".into());
            }
            a.push("-device".into());
            a.push(format!("floppy,drive={d},unit=0,drive-type=144"));
            a
        },
    },
];

/// The controllers this run attaches and their image paths (set once by `main`).
static CHOSEN: OnceLock<Vec<(&'static Hba, PathBuf)>> = OnceLock::new();

/// A host adapter `dev` with id `id` and a `scsi-hd` at target 0 holding `drive`.
fn scsi_hba(dev: &str, id: &str, drive: &str) -> Vec<String> {
    vec![
        "-device".into(),
        format!("{dev},id={id}"),
        "-device".into(),
        format!("scsi-hd,drive={drive},bus={id}.0,scsi-id=0"),
    ]
}

/// The options [`set`] knows, for the parsers that must skip them with their value.
pub(crate) fn options() -> impl Iterator<Item = &'static str> {
    HBAS.iter().map(|h| h.opt)
}

/// Reads the controller options from `args` (each `--opt FILE`).
pub(crate) fn set(root: &Path, args: &[&str]) -> Result<()> {
    let arm64 = args.windows(2).any(|w| w == ["--arch", "arm64"]);
    let pc = args.windows(2).any(|w| w == ["--machine", "pc"]);
    let mut chosen = Vec::new();
    for h in HBAS {
        let Some(i) = args.iter().position(|a| *a == h.opt) else {
            continue;
        };
        let file = args
            .get(i + 1)
            .filter(|f| !f.starts_with("--"))
            .ok_or_else(|| format!("{}: expected the disk image's file name", h.opt))?;
        if arm64 && !h.arches.contains(&Arch::Arm64) {
            return Err(format!("{}: amd64 only", h.opt).into());
        }
        if h.needs_pc && !pc {
            return Err(format!("{}: needs --machine pc (PIIX3's IDE controller)", h.opt).into());
        }
        chosen.push((h, boot::run_dir(root).join(file)));
    }
    let _ = CHOSEN.set(chosen);
    Ok(())
}

/// Adds the chosen controllers and their disks, made afresh, to `cmd` (after every other
/// device, see the module docs).
pub(crate) fn add_devices(cmd: &mut Command, arch: Arch) -> Result<()> {
    let Some(chosen) = CHOSEN.get() else {
        return Ok(());
    };
    for (h, image) in chosen {
        if !h.arches.contains(&arch) {
            return Err(format!("{}: not on {}", h.opt, arch.name()).into());
        }
        if h.opt == "--floppy" {
            make_floppy(image)?;
        } else {
            zeroed_disk(image, DISK_BYTES)?;
        }
        let drive = drive_id(h.opt);
        cmd.arg("-drive").arg(format!(
            "if=none,format=raw,file={},id={drive}",
            image.display()
        ));
        cmd.args((h.devices)(&drive, !crate::hwopts::machine_pc()));
    }
    Ok(())
}

/// The `-drive` id of the option `opt`'s disk: `m16a` and the option's letters.
fn drive_id(opt: &str) -> String {
    format!("m16a{}", opt.trim_start_matches('-').replace('-', ""))
}

/// Makes `bytes` of zeroes at `image`, replacing any old file.
fn zeroed_disk(image: &Path, bytes: u64) -> Result<()> {
    if let Some(dir) = image.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let _ = fs::remove_file(image);
    let f = fs::File::create(image).map_err(|e| format!("{}: {e}", image.display()))?;
    f.set_len(bytes)
        .map_err(|e| format!("{}: {e}", image.display()))?;
    Ok(())
}

/// Makes the 1.44 MB FAT12 floppy at `image` (no partition table, as a formatted floppy
/// is), holding [`FLOPPY_NOTE`].
pub(crate) fn make_floppy(image: &Path) -> Result<()> {
    let mut disk = vec![0u8; FLOPPY_BYTES];
    {
        let mut cur = Cursor::new(&mut disk[..]);
        fatfs::format_volume(
            &mut cur,
            fatfs::FormatVolumeOptions::new()
                .fat_type(fatfs::FatType::Fat12)
                .bytes_per_sector(512)
                .total_sectors((FLOPPY_BYTES / 512) as u32)
                .media(0xf0)
                .sectors_per_track(18)
                .heads(2)
                .volume_label(*b"EMIBSD FD  "),
        )?;
        let fs = fatfs::FileSystem::new(&mut cur, fatfs::FsOptions::new())?;
        fs.root_dir()
            .create_file(FLOPPY_NOTE)?
            .write_all(FLOPPY_NOTE_TEXT.as_bytes())?;
        fs.unmount()?;
    }
    if let Some(dir) = image.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    fs::write(image, &disk).map_err(|e| format!("{}: {e}", image.display()))?;
    Ok(())
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floppy_is_fat12_with_the_note() {
        let dir = std::env::temp_dir().join(format!("xtask-floppy-{}", std::process::id()));
        let img = dir.join("fd.img");
        make_floppy(&img).unwrap();
        let bytes = fs::read(&img).unwrap();
        assert_eq!(bytes.len(), FLOPPY_BYTES);
        assert_eq!(&bytes[510..512], &[0x55, 0xaa]);
        let fs = fatfs::FileSystem::new(Cursor::new(bytes), fatfs::FsOptions::new()).unwrap();
        assert_eq!(fs.fat_type(), fatfs::FatType::Fat12);
        let mut text = String::new();
        let mut f = fs.root_dir().open_file(FLOPPY_NOTE).unwrap();
        std::io::Read::read_to_string(&mut f, &mut text).unwrap();
        assert_eq!(text, FLOPPY_NOTE_TEXT);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn options_are_distinct_and_devices_named() {
        let names: Vec<_> = options().collect();
        for (i, n) in names.iter().enumerate() {
            assert!(n.starts_with("--"));
            assert!(!names[i + 1..].contains(n));
        }
        assert_eq!(drive_id("--megasas-gen2"), "m16amegasasgen2");
        let a = scsi_hba("megasas", "m16a-mfi", "d0");
        assert_eq!(a[3], "scsi-hd,drive=d0,bus=m16a-mfi.0,scsi-id=0");
    }
}
/* </TESTS> */
