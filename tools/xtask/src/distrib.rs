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
//! M16g: the release images `distrib/` makes from the install media, and their smokes.
//! See docs/ARCHITECTURE.md, "The release images".
//!
//! - `cargo xtask install-img --arch A`: `install${OSrev}.img`, the install media with the sets
//!   on one disk image, for a USB stick (`distrib/amd64/iso/Makefile`,
//!   `distrib/arm64/iso/Makefile`), in `target/install/<arch>`. The layout is the Makefiles':
//!   an MBR as `fdisk -i` makes it with the boot partition of `-b` (amd64: an EFI system
//!   partition of 960 sectors at 64, from `fdisk -l FSSIZE`; arm64: a FAT partition of type
//!   `0x0C` at 32768, active, which Rockchip boards need) and the OpenBSD partition after it;
//!   the label `disklabel -wAT-` writes with the template `/ *` on the vnd(4) of the image (`a`
//!   the whole OpenBSD partition, `i` the boot partition, `c` the disk); the boot partition
//!   as `newfs -t msdos` formats it, holding `efi/boot/` with efiboot; and on `a` an FFS1 as
//!   `newfs -O 1 -m 0 -o space -i 524288` makes it, holding the release directory
//!   `8.0/<arch>/` with the sets, the kernels, `INSTALL.<arch>` and `SHA256` (no `SHA256.sig`:
//!   the Makefiles' `XXX no SHA256.sig`), and the install kernel as `/bsd` and `/bsd.rd`
//!   (amd64: a copy of `bsd.rd` and a link to it, with `/etc/boot.conf` saying `set image
//!   /8.0/amd64/bsd.rd`; arm64: links to `8.0/arm64/bsd.rd`). The file systems are made by
//!   OpenBSD's makefs (the host build of `just userland`), the MBR and the label here.
//! - `cargo xtask cd-iso --arch amd64`: `cd${OSrev}.iso`, the boot-only CD
//!   (`distrib/amd64/ramdisk_cd/Makefile`): `8.0/amd64/bsd.rd`, `etc/boot.conf` and
//!   `8.0/amd64/eficdboot`, the FAT image (`makefs -t msdos -o create_size=350K`) with
//!   efiboot that the El Torito catalogue `8.0/amd64/boot.catalog` names, written by
//!   `iso9660.rs` as mkhybrid writes it.
//! - `cargo xtask install80 --arch A`: `install80.img` as a USB stick (`boot.rs`,
//!   [`BootMedium::UsbStick`]) and a fresh disk: the firmware boots efiboot from the stick,
//!   efiboot the install kernel, and OpenBSD's installer, unmodified, is answered over the
//!   serial console as a person would answer it (no response file, no network, no HTTP
//!   server): the sets come from the stick's `a` partition (`Location of sets? disk`). Then
//!   the installed disk boots to `login:` and compiles a program (`install.rs`,
//!   `boot_installed`). With `--image FILE` the stick is another install image, OpenBSD's
//!   own `install80.img` of the `diff-openbsd` snapshot, answered the same way, to compare
//!   what its installer asks; that run ends at the installer's reboot.
//! - `cargo xtask cd80 --arch amd64`: `cd80.iso` as a CD-ROM ([`BootMedium::Cdrom`]): the
//!   firmware boots the El Torito EFI image, efiboot reads `etc/boot.conf` from the CD and
//!   loads `bsd.rd`, which reaches the installer's first question; a shell then mounts the CD
//!   with mount_cd9660(8) (Rock Ridge names) and reads `etc/boot.conf` back.
//!
//! ## Deviations
//!
//! - Not built here, so not on the images: the BIOS boot programs (`/usr/mdec/mbr`'s code,
//!   `biosboot`, `boot`, `cdbr`, `cdboot`; `stand/` wants GNU as), so the stick's MBR has no
//!   boot code, its `a` has no `/boot` and no partition boot record, and the CD has no BIOS
//!   El Torito entry (its EFI image is the default entry); the sets `game`, `man`, the X sets
//!   and `BUILDINFO`; the Raspberry Pi firmware, device trees and U-Boot of the arm64 boot
//!   partition (packages, not in the tree). Each is printed when an image is made.
//! - `d_uid` of the label is fixed per architecture (`I80AMD64`, `I80ARM64`), not random,
//!   and makefs's FFS has no `-c FSSIZE` cylinder group option (its own groups).
//! - The CD's descriptors name EmiBSD and its author where the Makefile's mkhybrid line names
//!   OpenBSD and its release engineer, as the kernel's `ostype` names EmiBSD.

use std::fs;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use crate::Result;
use crate::boot::{self, Arch, BootMedium, Disks};
use crate::diffopenbsd::serial::{Stop, Vm};
use crate::install::install_dir;

/// The bytes of a disk sector.
const SECTOR: u64 = 512;

/// `struct disklabel`'s magic (`DISKMAGIC`).
const DISKMAGIC: u32 = 0x8256_4557;
/// `DTYPE_VND`: the label `disklabel -w` writes on a vnd(4).
const DTYPE_VND: u16 = 12;
/// `MAXPARTITIONS`.
const MAXPARTITIONS: u16 = 16;
/// `FS_BSDFFS`, `FS_MSDOS`.
const FS_BSDFFS: u8 = 7;
const FS_MSDOS: u8 = 8;
/// `DOSPTYP_OPENBSD`, `DOSPTYP_EFISYS`, `DOSPTYP_EFI`, `DOSACTIVE`.
const DOSPTYP_OPENBSD: u8 = 0xa6;
const DOSPTYP_EFISYS: u8 = 0xef;
const DOSPTYP_EFI: u8 = 0xee;
const DOSACTIVE: u8 = 0x80;

/// The geometry vnd(4) gives a label (`vndgetdisklabel`: 100 sectors a track, one track).
const VND_NSECTORS: u32 = 100;

/// The FFS1 superblock's offset in its partition (`SBLOCK_UFS1`), and its fields' offsets.
const SBLOCK_UFS1: usize = 8192;
const FS_BSIZE: usize = 48;
const FS_FSIZE: usize = 52;
const FS_FRAG: usize = 56;
const FS_CPG: usize = 180;
const FS_MAGIC_OFF: usize = 1372;
const FS_UFS1_MAGIC: u32 = 0x0001_1954;

/// The time every file of the images carries (the ramdisk's `makefs -T`: the pin's date).
const TIMESTAMP: u64 = crate::userland::TIMESTAMP;

/// How a release image of `arch` is laid out (`distrib/<arch>/iso/Makefile`).
struct Layout {
    /// `FSSIZE`: what fdisk(8) is told the disk is (amd64's `-l`), in sectors.
    fs_size: u64,
    /// The image's sectors (amd64 `TOTALSIZE`: `FSSIZE + MSDOSSIZE`).
    total: u64,
    /// The boot partition (`fdisk -b`): first sector, sectors, MBR type.
    boot_start: u64,
    boot_size: u64,
    boot_type: u8,
    /// fdisk's geometry for the CHS fields: sectors a track, heads (amd64: `-l` sets 64 and
    /// 1; arm64: the vnd's label).
    spt: u64,
    heads: u64,
    /// `newfs -t msdos`'s options, as makefs's msdos options. The BPB's 63 sectors a track
    /// are mkfs_msdos's default: given `sectors_per_track` and `drive_heads` both, it takes
    /// no size from the image and fails.
    fat: &'static str,
    /// `d_uid` of the label (module docs, deviations).
    duid: [u8; 8],
}

impl Layout {
    fn of(arch: Arch) -> Layout {
        match arch {
            // FSSIZE=1638400, MSDOSSIZE=960; `fdisk -yi -l FSSIZE -b 960 -f mbr`; plain
            // `newfs -t msdos` of 960 sectors.
            Arch::Amd64 => Layout {
                fs_size: 1_638_400,
                total: 1_638_400 + 960,
                boot_start: 64,
                boot_size: 960,
                boot_type: DOSPTYP_EFISYS,
                spt: 64,
                heads: 1,
                fat: "fat_type=12,sectors_per_cluster=8,directory_entries=512,\
                      media_descriptor=248,drive_heads=1,hidden_sectors=64",
                duid: *b"I80AMD64",
            },
            // FSSIZE=1433600; `fdisk -iy -b "16384@32768:c"`; `newfs -t msdos -L boot -c1
            // -F16`.
            Arch::Arm64 => Layout {
                fs_size: 1_433_600,
                total: 1_433_600,
                boot_start: 32_768,
                boot_size: 16_384,
                boot_type: 0x0c,
                spt: u64::from(VND_NSECTORS),
                heads: 1,
                fat: "fat_type=16,sectors_per_cluster=1,directory_entries=512,\
                      media_descriptor=248,drive_heads=1,\
                      hidden_sectors=32768,volume_label=BOOT",
                duid: *b"I80ARM64",
            },
        }
    }

    /// The OpenBSD partition: after the boot partition (`MBR_init`), to the end of what fdisk
    /// was told the disk is.
    fn openbsd(&self) -> (u64, u64) {
        let start = self.boot_start + self.boot_size;
        (start, self.fs_size - start)
    }
}

/// `${OSrev}` and `${OSREV}` of the sources (`80`, `8.0`).
fn release(root: &Path) -> Result<(String, String)> {
    let rev = crate::userland::miniroot::os_rev(&root.join("reference/openbsd-src"))?;
    let (major, minor) = rev.split_at(rev.len().saturating_sub(1));
    Ok((rev.clone(), format!("{major}.{minor}")))
}

/// The value after `name`, if present.
fn opt<'a>(args: &[&'a str], name: &str) -> Option<&'a str> {
    args.windows(2).find(|w| w[0] == name).map(|w| w[1])
}

fn arch_of(args: &[&str]) -> Result<Arch> {
    Arch::parse(opt(args, "--arch").ok_or("missing `--arch A`")?)
}

/// OpenBSD's makefs as `just userland` built it for this machine.
fn makefs(root: &Path, arch: Arch) -> Result<PathBuf> {
    let p = root.join(format!("target/userland/{}/host/bin/makefs", arch.name()));
    if p.is_file() {
        Ok(p)
    } else {
        Err(format!("{}: no makefs; run `just userland` first", p.display()).into())
    }
}

/// Runs makefs: `kind` (`ffs`, `msdos`), its options, `size` bytes (0: makefs's own),
/// `tree` into `image`.
fn run_makefs(
    makefs: &Path,
    kind: &str,
    options: &str,
    size: Option<u64>,
    image: &Path,
    tree: &Path,
) -> Result<()> {
    let _ = fs::remove_file(image);
    let mut cmd = Command::new(makefs);
    cmd.args(["-t", kind, "-T", &TIMESTAMP.to_string(), "-o", options]);
    if let Some(s) = size {
        cmd.args(["-s", &s.to_string()]);
    }
    let out = cmd
        .arg(image)
        .arg(tree)
        .output()
        .map_err(|e| format!("{}: {e}", makefs.display()))?;
    if !out.status.success() {
        return Err(format!(
            "makefs -t {kind} {}: {}{}",
            image.display(),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
        .into());
    }
    Ok(())
}

/// A fresh, empty directory at `dir`.
fn fresh_dir(dir: &Path) -> Result<()> {
    if dir.exists() {
        fs::remove_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(())
}

/// Writes `bytes` to `path` with mode `mode`.
fn put(path: &Path, bytes: &[u8], mode: u32) -> Result<()> {
    if let Some(d) = path.parent() {
        fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(())
}

/// `cp -p from to`: a hard link where the file system allows (the sets are big and the
/// copy keeps their mode anyway), a copy otherwise.
fn copy_keep(from: &Path, to: &Path) -> Result<()> {
    let _ = fs::remove_file(to);
    if fs::hard_link(from, to).is_err() {
        fs::copy(from, to).map_err(|e| format!("{} -> {}: {e}", from.display(), to.display()))?;
    }
    Ok(())
}

/// The efiboot of `arch` (`just efiboot-<arch>`).
fn efiboot(root: &Path, arch: Arch) -> Result<(PathBuf, &'static str)> {
    let name = match arch {
        Arch::Amd64 => "BOOTX64.EFI",
        Arch::Arm64 => "BOOTAA64.EFI",
    };
    let p = root.join(format!("target/efiboot/{}/{name}", arch.name()));
    if p.is_file() {
        Ok((p, name))
    } else {
        Err(format!(
            "{}: no efiboot; run `just efiboot-{}`",
            p.display(),
            arch.name()
        )
        .into())
    }
}

/// `BOOTIA32.EFI` as the base set has it (`sets.rs`): one zero sector, there is no IA32
/// loader in this port. The amd64 images copy `${DESTDIR}/usr/mdec/BOOTIA32.EFI` beside
/// `BOOTX64.EFI`.
fn bootia32() -> Vec<u8> {
    vec![0u8; SECTOR as usize]
}

/// The tree of the boot partition (`efi/boot/` and, on arm64, the Raspberry Pi files the
/// Makefile copies; module docs, deviations).
fn stage_boot_partition(root: &Path, arch: Arch, dir: &Path) -> Result<()> {
    fresh_dir(dir)?;
    let (efi, name) = efiboot(root, arch)?;
    let bytes = fs::read(&efi).map_err(|e| format!("{}: {e}", efi.display()))?;
    // `cp` of /usr/mdec's 0444 files: the FAT entries are read-only.
    match arch {
        Arch::Amd64 => {
            put(&dir.join("efi/boot").join(name), &bytes, 0o444)?;
            put(&dir.join("efi/boot/BOOTIA32.EFI"), &bootia32(), 0o444)?;
        }
        Arch::Arm64 => {
            println!(
                "  install-img arm64: not built, left out of the boot partition: the Raspberry \
                 Pi firmware, device trees and overlays, u-boot.bin (packages)"
            );
            fs::create_dir_all(dir.join("overlays"))?;
            put(&dir.join("efi/boot/bootaa64.efi"), &bytes, 0o444)?;
            put(&dir.join("efi/boot/startup.nsh"), b"bootaa64.efi\n", 0o644)?;
            put(
                &dir.join("config.txt"),
                b"arm_64bit=1\nenable_uart=1\ndtoverlay=disable-bt\nkernel=u-boot.bin\n",
                0o644,
            )?;
        }
    }
    Ok(())
}

/// The release files the Makefile's `BASE` names, those the build has, and the ones it
/// lacks (printed).
fn release_files(sets: &Path, arch: Arch, rev: &str) -> Result<Vec<PathBuf>> {
    let names = [
        format!("base{rev}.tgz"),
        format!("comp{rev}.tgz"),
        format!("game{rev}.tgz"),
        format!("man{rev}.tgz"),
        "bsd".to_string(),
        "bsd.rd".to_string(),
        "bsd.mp".to_string(),
        format!("INSTALL.{}", arch.name()),
        "BUILDINFO".to_string(),
    ];
    let mut have = Vec::new();
    let mut lack = Vec::new();
    for n in &names {
        let p = sets.join(n);
        if p.is_file() {
            have.push(p);
        } else {
            lack.push(n.as_str());
        }
    }
    for must in [
        format!("base{rev}.tgz"),
        "bsd.rd".to_string(),
        "SHA256".to_string(),
    ] {
        if !sets.join(&must).is_file() {
            return Err(format!(
                "{}: no {must}; run `just install-media-{}`",
                sets.display(),
                arch.name()
            )
            .into());
        }
    }
    println!(
        "  install-img {}: not built, left out of {}: {} and the X sets",
        arch.name(),
        sets.display(),
        lack.join(", ")
    );
    Ok(have)
}

/// The tree of the OpenBSD partition's file system.
fn stage_openbsd_partition(
    root: &Path,
    arch: Arch,
    dir: &Path,
    rev: &str,
    osrev: &str,
) -> Result<()> {
    fresh_dir(dir)?;
    let sets = crate::userland::sets::sets_dir(root, arch);
    let reldir = dir.join(osrev).join(arch.name());
    fs::create_dir_all(&reldir).map_err(|e| format!("{}: {e}", reldir.display()))?;
    for f in release_files(&sets, arch, rev)? {
        if let Some(n) = f.file_name() {
            copy_keep(&f, &reldir.join(n))?;
        }
    }
    // `cat ${RELDIR}/SHA256 ${RELXDIR}/SHA256 > .../SHA256` (no X sets here).
    let sha = fs::read(sets.join("SHA256"))?;
    put(&reldir.join("SHA256"), &sha, 0o644)?;
    match arch {
        Arch::Amd64 => {
            // `boot` (the BIOS boot(8)) is not built (module docs, deviations).
            put(
                &dir.join("etc/boot.conf"),
                format!("set image /{osrev}/amd64/bsd.rd\n").as_bytes(),
                0o644,
            )?;
            // `install -c -m 555 -o root -g wheel bsd.rd ${MOUNT_POINT}/bsd`, then `ln`.
            let rd = fs::read(sets.join("bsd.rd"))?;
            put(&dir.join("bsd"), &rd, 0o555)?;
            fs::hard_link(dir.join("bsd"), dir.join("bsd.rd"))?;
        }
        Arch::Arm64 => {
            fs::hard_link(reldir.join("bsd.rd"), dir.join("bsd.rd"))?;
            fs::hard_link(reldir.join("bsd.rd"), dir.join("bsd"))?;
        }
    }
    Ok(())
}

/// The CHS of `lba` in fdisk's geometry (`PRT_lba_to_chs`), as `chs_to_dp` stores it: past
/// what CHS can say, the maximum (head 255 for a protective GPT partition, 254 otherwise).
fn chs(lba: u64, spt: u64, heads: u64, typ: u8) -> [u8; 3] {
    let mut cyl = lba / (spt * heads);
    let mut head = (lba / spt) % heads;
    let mut sect = (lba % spt) + 1;
    if head > 254 || sect > 63 || cyl > 1023 {
        head = if typ == DOSPTYP_EFI { 255 } else { 254 };
        sect = 63;
        cyl = 1023;
    }
    [
        head as u8,
        ((sect & 0x3f) | ((cyl & 0x300) >> 2)) as u8,
        (cyl & 0xff) as u8,
    ]
}

/// One MBR entry (`PRT_prt_to_dp`).
fn mbr_entry(flag: u8, typ: u8, start: u64, size: u64, l: &Layout) -> [u8; 16] {
    let mut e = [0u8; 16];
    e[0] = flag;
    e[1..4].copy_from_slice(&chs(start, l.spt, l.heads, typ));
    e[4] = typ;
    e[5..8].copy_from_slice(&chs(start + size - 1, l.spt, l.heads, typ));
    e[8..12].copy_from_slice(&(start as u32).to_le_bytes());
    e[12..16].copy_from_slice(&(size as u32).to_le_bytes());
    e
}

/// The MBR `fdisk -i -b ...` writes (`MBR_init`): the boot partition first, the OpenBSD
/// partition fourth, active unless the boot partition is (an EFI system partition is not).
fn mbr(l: &Layout) -> [u8; 512] {
    let mut m = [0u8; 512];
    let boot_active = l.boot_type != DOSPTYP_EFISYS;
    let (ob_start, ob_size) = l.openbsd();
    let boot = mbr_entry(
        if boot_active { DOSACTIVE } else { 0 },
        l.boot_type,
        l.boot_start,
        l.boot_size,
        l,
    );
    let obsd = mbr_entry(
        if boot_active { 0 } else { DOSACTIVE },
        DOSPTYP_OPENBSD,
        ob_start,
        ob_size,
        l,
    );
    m[446..462].copy_from_slice(&boot);
    m[494..510].copy_from_slice(&obsd);
    m[510] = 0x55;
    m[511] = 0xaa;
    m
}

/// `DISKLABELV1_FFS_FRAGBLOCK(fsize, frag)`.
fn fragblock(fsize: u32, frag: u32) -> u8 {
    let ffs = |v: u32| if v == 0 { 0 } else { v.trailing_zeros() + 1 };
    if fsize * frag == 0 {
        0
    } else {
        (((ffs(fsize * frag) - 13) << 3) | ffs(frag)) as u8
    }
}

/// The label `echo '/ *' | disklabel -wAT-` writes on the image's vnd(4): the vnd's
/// geometry, `a` the whole OpenBSD partition (FFS of block `bsize`, fragment `fsize`,
/// `cpg`), `c` the disk, `i` the boot partition (`spoofmbr` gives it, as MSDOS).
fn disklabel(l: &Layout, bsize: u32, fsize: u32, cpg: u16) -> [u8; 512] {
    let mut d = [0u8; 512];
    let put = |d: &mut [u8; 512], off: usize, b: &[u8]| d[off..off + b.len()].copy_from_slice(b);
    let total = l.total as u32;
    let (ob_start, ob_size) = l.openbsd();
    put(&mut d, 0, &DISKMAGIC.to_le_bytes());
    put(&mut d, 4, &DTYPE_VND.to_le_bytes());
    put(&mut d, 8, b"vnd device");
    put(&mut d, 24, b"fictitious");
    put(&mut d, 40, &(SECTOR as u32).to_le_bytes()); // d_secsize
    put(&mut d, 44, &VND_NSECTORS.to_le_bytes()); // d_nsectors
    put(&mut d, 48, &1u32.to_le_bytes()); // d_ntracks
    put(&mut d, 52, &(total / VND_NSECTORS).to_le_bytes()); // d_ncylinders
    put(&mut d, 56, &VND_NSECTORS.to_le_bytes()); // d_secpercyl
    put(&mut d, 60, &total.to_le_bytes()); // d_secperunit
    put(&mut d, 64, &l.duid); // d_uid
    put(&mut d, 80, &(ob_start as u32).to_le_bytes()); // d_bstart
    put(&mut d, 84, &((ob_start + ob_size) as u32).to_le_bytes()); // d_bend
    put(&mut d, 114, &1u16.to_le_bytes()); // d_version
    put(&mut d, 132, &DISKMAGIC.to_le_bytes()); // d_magic2
    put(&mut d, 138, &MAXPARTITIONS.to_le_bytes()); // d_npartitions
    let part =
        |d: &mut [u8; 512], letter: u8, size: u64, off: u64, fstype: u8, frag: u8, cpg: u16| {
            let at = 148 + 16 * usize::from(letter - b'a');
            d[at..at + 4].copy_from_slice(&(size as u32).to_le_bytes());
            d[at + 4..at + 8].copy_from_slice(&(off as u32).to_le_bytes());
            d[at + 12] = fstype;
            d[at + 13] = frag;
            d[at + 14..at + 16].copy_from_slice(&cpg.to_le_bytes());
        };
    part(
        &mut d,
        b'a',
        ob_size,
        ob_start,
        FS_BSDFFS,
        fragblock(fsize, bsize / fsize),
        cpg,
    );
    part(&mut d, b'c', l.total, 0, 0, 0, 0);
    part(&mut d, b'i', l.boot_size, l.boot_start, FS_MSDOS, 0, 0);
    let end = 148 + 16 * usize::from(MAXPARTITIONS);
    let sum = d[..end]
        .chunks(2)
        .fold(0u16, |s, w| s ^ u16::from_le_bytes([w[0], w[1]]));
    put(&mut d, 136, &sum.to_le_bytes()); // d_checksum
    d
}

/// The FFS1 superblock's block size, fragment size and cylinders per group, checked.
fn ffs1_params(image: &Path) -> Result<(u32, u32, u16)> {
    let mut f = fs::File::open(image).map_err(|e| format!("{}: {e}", image.display()))?;
    let mut sb = vec![0u8; 2048];
    f.seek(SeekFrom::Start(SBLOCK_UFS1 as u64))?;
    f.read_exact(&mut sb)?;
    let at = |o: usize| u32::from_le_bytes([sb[o], sb[o + 1], sb[o + 2], sb[o + 3]]);
    if at(FS_MAGIC_OFF) != FS_UFS1_MAGIC {
        return Err(format!("{}: no FFS1 superblock", image.display()).into());
    }
    let (bsize, fsize, frag) = (at(FS_BSIZE), at(FS_FSIZE), at(FS_FRAG));
    if fsize == 0 || bsize / fsize != frag {
        return Err(format!(
            "{}: bsize {bsize} fsize {fsize} frag {frag}",
            image.display()
        )
        .into());
    }
    Ok((bsize, fsize, at(FS_CPG).min(u32::from(u16::MAX)) as u16))
}

/// The FAT boot sector's sanity: a boot sector at all, and, when given, `want_spc` sectors a
/// cluster and the FAT type. makefs exits 0 when `mkfs_msdos` fails and leaves a file of
/// zeros (`usr.sbin/makefs/msdos.c:143`, docs/EXTERNAL_BUGS.md EXT-223), so its status
/// alone says nothing.
fn check_fat(image: &Path, want: Option<(u8, &[u8])>) -> Result<()> {
    let b = fs::read(image).map_err(|e| format!("{}: {e}", image.display()))?;
    let ok = b.len() >= 512
        && b[510..512] == [0x55, 0xaa]
        && b[11..13] == [0, 2]
        && want.is_none_or(|(spc, kind)| b[13] == spc && &b[54..62] == kind);
    if !ok {
        return Err(format!(
            "{}: makefs -t msdos made no FAT file system{}",
            image.display(),
            want.map_or(String::new(), |(spc, kind)| format!(
                " of {spc} sectors a cluster, {}",
                String::from_utf8_lossy(kind).trim_end()
            ))
        )
        .into());
    }
    Ok(())
}

/// Copies `src` whole into `dst` at byte `at`.
fn place(dst: &mut fs::File, at: u64, src: &Path) -> Result<()> {
    let mut f = fs::File::open(src).map_err(|e| format!("{}: {e}", src.display()))?;
    dst.seek(SeekFrom::Start(at))?;
    io::copy(&mut f, dst)?;
    Ok(())
}

/// `install${OSrev}.img` of `arch` (module docs).
pub(crate) fn make_install_img(root: &Path, arch: Arch) -> Result<PathBuf> {
    let (rev, osrev) = release(root)?;
    let l = Layout::of(arch);
    let makefs = makefs(root, arch)?;
    let dir = install_dir(root, arch);
    let work = dir.join("install-img");
    fresh_dir(&work)?;

    // The boot partition, as `newfs -t msdos` and the `cp`s make it.
    let boot_tree = work.join("boot-tree");
    stage_boot_partition(root, arch, &boot_tree)?;
    let fat = work.join("boot.fat");
    run_makefs(
        &makefs,
        "msdos",
        &format!("{},OEM_string=BSD  4.4", l.fat.replace(' ', "")),
        Some(l.boot_size * SECTOR),
        &fat,
        &boot_tree,
    )?;
    let (spc, kind): (u8, &[u8]) = match arch {
        Arch::Amd64 => (8, b"FAT12   "),
        Arch::Arm64 => (1, b"FAT16   "),
    };
    check_fat(&fat, Some((spc, kind)))?;

    // The OpenBSD partition's FFS1.
    let (ob_start, ob_size) = l.openbsd();
    let tree = work.join("a-tree");
    stage_openbsd_partition(root, arch, &tree, &rev, &osrev)?;
    let ffs = work.join("a.ffs");
    run_makefs(
        &makefs,
        "ffs",
        "version=1,bsize=16384,fsize=2048,minfree=0,optimization=space,density=524288",
        Some(ob_size * SECTOR),
        &ffs,
        &tree,
    )?;
    let (bsize, fsize, cpg) = ffs1_params(&ffs)?;
    let ffs_len = fs::metadata(&ffs)?.len();
    if ffs_len != ob_size * SECTOR {
        return Err(format!("{}: {ffs_len} bytes, not the partition's", ffs.display()).into());
    }

    // The disk: MBR, boot partition, FFS, then the label in the FFS's boot area.
    let out = dir.join(format!("install{rev}.img"));
    let _ = fs::remove_file(&out);
    let mut img = fs::File::create(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    img.set_len(l.total * SECTOR)?;
    img.write_all(&mbr(&l))?;
    place(&mut img, l.boot_start * SECTOR, &fat)?;
    place(&mut img, ob_start * SECTOR, &ffs)?;
    img.seek(SeekFrom::Start((ob_start + 1) * SECTOR))?;
    img.write_all(&disklabel(&l, bsize, fsize, cpg))?;
    drop(img);
    fs::remove_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;
    println!(
        "install-img {}: {} ({} sectors: {} partition at {} ({} sectors, efi/boot), OpenBSD \
         partition at {ob_start} (a: FFS1, {bsize}/{fsize}, {osrev}/{}/ and the install kernel))",
        arch.name(),
        out.display(),
        l.total,
        if l.boot_type == DOSPTYP_EFISYS {
            "EFI system"
        } else {
            "FAT"
        },
        l.boot_start,
        l.boot_size,
        arch.name()
    );
    Ok(out)
}

/// `cargo xtask install-img --arch A` (module docs).
pub(crate) fn install_img(root: &Path, args: &[&str]) -> Result<()> {
    make_install_img(root, arch_of(args)?).map(|_| ())
}

/// The El Torito EFI image's size (`EFICDBOOTSIZE=350K`).
const EFICDBOOTSIZE: &str = "350k";

/// `cargo xtask cd-iso --arch amd64`: `cd${OSrev}.iso` (module docs).
pub(crate) fn cd_iso(root: &Path, args: &[&str]) -> Result<()> {
    let arch = arch_of(args)?;
    if arch != Arch::Amd64 {
        return Err("cd-iso: cd80.iso is amd64's (ROADMAP M16g)".into());
    }
    // `--rd-kernel K`: the miniroot and bsd.rd first, as `install-media` makes them (the
    // boot-only CD needs no sets, so no comp build).
    if let Some(k) = opt(args, "--rd-kernel") {
        let k = PathBuf::from(k);
        crate::userland::with_ctx(root, arch, |ctx| {
            crate::install::make_bsd_rd(ctx, root, arch, &k).map(|_| ())
        })?;
    }
    make_cd_iso(root, arch).map(|_| ())
}

/// `cd${OSrev}.iso` of amd64 (module docs).
pub(crate) fn make_cd_iso(root: &Path, arch: Arch) -> Result<PathBuf> {
    let (rev, osrev) = release(root)?;
    let makefs = makefs(root, arch)?;
    let dir = install_dir(root, arch);
    let bsd_rd = dir.join("bsd.rd");
    if !bsd_rd.is_file() {
        return Err(format!(
            "{}: no bsd.rd; run `just install-media-{}`",
            bsd_rd.display(),
            arch.name()
        )
        .into());
    }
    let work = dir.join("cd-iso");
    fresh_dir(&work)?;

    // ${EFICDBOOT}: `makefs -t msdos -o create_size=350K eficdboot eficdboot-dir`.
    let efi_tree = work.join("eficdboot-dir");
    let (efi, name) = efiboot(root, arch)?;
    let bytes = fs::read(&efi).map_err(|e| format!("{}: {e}", efi.display()))?;
    put(&efi_tree.join("efi/boot").join(name), &bytes, 0o444)?;
    put(&efi_tree.join("efi/boot/BOOTIA32.EFI"), &bootia32(), 0o444)?;
    let eficdboot = work.join("eficdboot");
    run_makefs(
        &makefs,
        "msdos",
        &format!("create_size={EFICDBOOTSIZE}"),
        None,
        &eficdboot,
        &efi_tree,
    )?;
    check_fat(&eficdboot, None)?;

    // ${CDROM}: the tree mkhybrid is given.
    let cd = work.join("cd-dir");
    let reldir = cd.join(&osrev).join(arch.name());
    fs::create_dir_all(&reldir)?;
    put(
        &cd.join("etc/boot.conf"),
        format!("set image /{osrev}/{}/bsd.rd\n", arch.name()).as_bytes(),
        0o644,
    )?;
    fs::copy(&bsd_rd, reldir.join("bsd.rd"))?;
    fs::set_permissions(reldir.join("bsd.rd"), fs::Permissions::from_mode(0o755))?;
    fs::copy(&eficdboot, reldir.join("eficdboot"))?;
    println!(
        "  cd-iso {}: not built, left out: cdbr and cdboot (the BIOS El Torito boot)",
        arch.name()
    );
    let out = dir.join(format!("cd{rev}.iso"));
    let application = format!("EmiBSD {osrev} {} bootonly CD", arch.name());
    let volume = format!("EmiBSD/{}   {osrev} bootonly CD", arch.name());
    let publisher = "Copyright (c) 2026 Emilio Navarrete Lineros, EmiBSD";
    let preparer = "Emilio Navarrete Lineros <enavarre@outlook.com>";
    let catalog = format!("{osrev}/{}/boot.catalog", arch.name());
    let efi_boot = format!("{osrev}/{}/eficdboot", arch.name());
    crate::iso9660::mkhybrid(
        &cd,
        &out,
        &crate::iso9660::Options {
            application: &application,
            publisher,
            preparer,
            volume: &volume,
            bios_boot: None,
            catalog: Some(&catalog),
            efi_boot: Some(&efi_boot),
            time: TIMESTAMP as i64,
        },
    )?;
    // The boot loader must find the kernel and boot.conf as libsa's cd9660 looks them up.
    let iso = fs::read(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    for p in [
        format!("/{osrev}/{}/bsd.rd", arch.name()),
        "/etc/boot.conf".to_string(),
    ] {
        if crate::iso9660::libsa_lookup(&iso, &p).is_none() {
            return Err(format!("{}: libsa's lookup does not find {p}", out.display()).into());
        }
    }
    fs::remove_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;
    println!(
        "cd-iso {}: {} ({} KiB: {osrev}/{}/bsd.rd, etc/boot.conf, El Torito EFI image \
         {osrev}/{}/eficdboot)",
        arch.name(),
        out.display(),
        iso.len() / 1024,
        arch.name(),
        arch.name()
    );
    Ok(out)
}

/// The fresh disk of the `install80` run: small enough that disklabel's automatic layout
/// takes its `alloc_small` table (one `/` and swap; `sbin/disklabel/editor.c`), as on a
/// 2.25 GB disk, room enough for the base and comp sets.
const TARGET_DISK_BYTES: u64 = 2304 << 20;

/// The installer's questions in the order it asks them, and what a person installing from
/// the stick answers (`install.sub`, the arch's `install.md`); a question asked twice gets
/// its answers in turn ([`Vm::respond_in_turn`]). The fresh disk is `sd0` and the stick
/// `sd1` (the virtio disk attaches before umass(4)); the run checks it.
fn answers(arch: Arch, osrev: &str) -> Vec<(&'static str, Vec<String>)> {
    let s = |v: &str| v.to_string() + "\n";
    // amd64's efiboot prompt comes first ([`efiboot_on_com0`]).
    let mut a: Vec<(&'static str, Vec<String>)> = Vec::new();
    a.extend([
        (
            "(I)nstall, (U)pgrade, (A)utoinstall or (S)hell?",
            vec![s("i")],
        ),
        ("Terminal type?", vec![s("vt220")]),
        ("System hostname?", vec![s("emibsd")]),
        ("Network interface to configure?", vec![s("done")]),
        ("DNS domain name?", vec![s("emibsd.test")]),
        ("DNS nameservers?", vec![s("none")]),
        ("Password for root account?", vec![s("emibsd")]),
        ("Start sshd(8) by default?", vec![s("no")]),
        ("Do you expect to run the X Window System?", vec![s("no")]),
        (
            "Do you want the X Window System to be started by xenodm(1)?",
            vec![s("no")],
        ),
        ("Change the default console to com0?", vec![s("yes")]),
        ("Which speed should com0 use?", vec![s("115200")]),
        ("Setup a user?", vec![s("no")]),
        ("Which disk is the root disk?", vec![s("sd0")]),
        (
            "Encrypt the root disk with a (p)assphrase or (k)eydisk?",
            vec![s("no")],
        ),
        // amd64 (booted by UEFI: `MDEFI`): a GPT with an EFI system partition.
        ("Use (W)hole disk MBR, whole disk (G)PT", vec![s("gpt")]),
        ("An EFI/GPT disk may not boot. Proceed?", vec![s("yes")]),
        // arm64: an MBR with the FAT boot partition.
        ("Use (W)hole disk or (E)dit the MBR?", vec![s("whole")]),
        (
            "Use (A)uto layout, (E)dit auto layout, or create (C)ustom layout?",
            vec![s("a")],
        ),
        ("Which disk do you wish to initialize?", vec![s("done")]),
        ("Location of sets?", vec![s("disk"), s("done")]),
        ("Is the disk partition already mounted?", vec![s("no")]),
        ("Which disk contains the install media?", vec![s("sd1")]),
        ("Which sd1 partition has the install sets?", vec![s("a")]),
        (
            "Pathname to the sets?",
            vec![s(&format!("{osrev}/{}", arch.name()))],
        ),
        // The base install: what EmiBSD's image holds (bsd, bsd.mp, bsd.rd, base, comp),
        // and on OpenBSD's own image the same, its X, game and man sets deselected so it
        // fits the disk (`--image`): the list is shown again, then `done`.
        ("Set name(s)?", vec![s("-x* -game* -man*"), s("done")]),
        (
            "Directory does not contain SHA256.sig. Continue without verification?",
            vec![s("yes")],
        ),
        ("What timezone are you in?", vec![s("UTC")]),
        ("Exit to (S)hell, (H)alt or (R)eboot?", vec![s("reboot")]),
    ]);
    a
}

/// Serial lines that mean the installer or the kernel failed.
const FAILURES: &[&str] = &[
    "panic: ",
    "is not a valid choice",
    "not a valid timezone",
    "No filesystems found on",
    "does not exist.",
    "and found no OpenBSD sets",
    "Checksum test for",
    "Installation of ",
    "Failed to install bootblocks",
    "No OpenBSD partition",
    "must be configured!",
];

/// The kernel's attach line for a disk of `bytes` at `unit`: `sd0: 2304MB,`.
fn attach(unit: &str, bytes: u64) -> String {
    format!("{unit}: {}MB,", bytes >> 20)
}

/// How long a typed character may take to come back on the console before it is typed again.
const ECHO_WAIT: Duration = Duration::from_secs(10);

/// Types `line` at a prompt as a person at a serial terminal does: a character at a time,
/// each one again when its echo does not come back (the Enter at the end is not echoed).
/// After `set tty com0`, efiboot reads the port through the firmware's serial protocol
/// (`efi_com_getc`), and under load a line typed at once has come back as its first
/// character only, the rest never reaching the prompt (`boot> b`, a `smoke-cd80` run of
/// 2026-10-09, four smokes at a time); efiboot then waits for the rest of the line forever.
fn type_echoed(vm: &mut Vm, line: &str) -> Result<()> {
    for c in line.chars() {
        let s = c.to_string();
        let mut tries = 0;
        loop {
            vm.send(&s)?;
            match vm.wait_for(&s, boot::time_limit(ECHO_WAIT)) {
                Ok(()) => break,
                Err(_) if tries < 3 && !vm.exited() => tries += 1,
                Err(e) => return Err(e),
            }
        }
    }
    vm.send("\n")
}

/// efiboot's prompt on amd64: `set tty com0` and `boot`, as one does with OpenBSD's own
/// media on a machine whose console is serial (the kernel takes efiboot's console).
fn efiboot_on_com0(vm: &mut Vm, limit: Duration) -> Result<()> {
    vm.wait_for("boot> ", limit)?;
    type_echoed(vm, "set tty com0")?;
    vm.wait_for("boot> ", limit)?;
    type_echoed(vm, "boot")
}

/// `cargo xtask install80 --arch A` (module docs).
pub(crate) fn install80(root: &Path, args: &[&str]) -> Result<()> {
    let arch = arch_of(args)?;
    let (rev, osrev) = release(root)?;
    // `--image FILE`: another install image on the stick, OpenBSD's own `install80.img` (the
    // `diff-openbsd` snapshot's) to see what its installer asks given the same answers; the
    // run then stops at the reboot (the result is OpenBSD, not EmiBSD).
    let other = opt(args, "--image").map(PathBuf::from);
    let img = other
        .clone()
        .unwrap_or_else(|| install_dir(root, arch).join(format!("install{rev}.img")));
    if !img.is_file() {
        return Err(format!(
            "{}: no install image; run `just install-img-{}`",
            img.display(),
            arch.name()
        )
        .into());
    }
    let work = install_dir(root, arch).join(if other.is_some() {
        "install80-image"
    } else {
        "install80"
    });
    fs::create_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;
    let started = Instant::now();

    // The fresh disk, `sd0`.
    let disk = boot::disk_path_n(root, arch, Some("install80"), 0);
    let _ = fs::remove_file(&disk);
    if let Some(parent) = disk.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let f = fs::File::create(&disk).map_err(|e| format!("{}: {e}", disk.display()))?;
    f.set_len(TARGET_DISK_BYTES)
        .map_err(|e| format!("{}: {e}", disk.display()))?;
    drop(f);

    // The installer, from the stick.
    boot::set_boot_medium(BootMedium::UsbStick);
    let disks = Disks {
        fresh: false,
        count: 1,
        set: Some("install80"),
    };
    let cmd = boot::qemu_command(root, arch, &img, "stdio", None, &disks)?;
    boot::set_boot_medium(BootMedium::Disk);
    let log = work.join("install.log");
    let mut vm = Vm::spawn(&format!("install80-{}", arch.name()), cmd, log.clone())?;
    if arch == Arch::Amd64 {
        efiboot_on_com0(&mut vm, boot::time_limit(Duration::from_secs(600)))?;
    }
    let table = answers(arch, &osrev);
    let rules: Vec<(&str, Vec<&str>)> = table
        .iter()
        .map(|(q, a)| (*q, a.iter().map(String::as_str).collect()))
        .collect();
    let rules: Vec<(&str, &[&str])> = rules.iter().map(|(q, a)| (*q, a.as_slice())).collect();
    // The stick at sd0 would be wiped: stop before the installer gets there.
    let wrong = attach("sd1", TARGET_DISK_BYTES);
    let mut fails = FAILURES.to_vec();
    fails.push(&wrong);
    vm.respond_in_turn(
        &rules,
        &fails,
        &[Stop::Exited],
        boot::time_limit(Duration::from_secs(3600)),
        boot::time_limit(Duration::from_secs(180)),
    )?;
    let text = vm.text();
    drop(vm);
    let image_mb = fs::metadata(&img).map(|m| m.len()).unwrap_or(0) >> 20;
    for line in [
        attach("sd0", TARGET_DISK_BYTES),
        format!("sd1: {image_mb}MB,"),
        "Let's install the sets!".to_string(),
        format!("Installing base{rev}.tgz"),
        format!("Installing comp{rev}.tgz"),
        "Making all device nodes".to_string(),
        "CONGRATULATIONS!".to_string(),
    ] {
        if !text.contains(&line) {
            return Err(format!(
                "install80 {}: never saw {line:?} (log: {})",
                arch.name(),
                log.display()
            )
            .into());
        }
    }
    if text.contains("ftp: ") && text.contains("http://") {
        return Err(format!(
            "install80 {}: the installer went to the network",
            arch.name()
        )
        .into());
    }
    println!(
        "xtask: install80 {}: installed from the stick in {:.0}s",
        arch.name(),
        started.elapsed().as_secs_f32()
    );
    if other.is_some() {
        return Ok(());
    }

    // The installed disk, booted to `login:`.
    crate::install::boot_installed(root, arch, &disk, work.join("boot.log"))?;
    println!(
        "xtask: install80 {}: ok in {:.0}s",
        arch.name(),
        started.elapsed().as_secs_f32()
    );
    Ok(())
}

/// `cargo xtask cd80 --arch amd64` (module docs).
pub(crate) fn cd80(root: &Path, args: &[&str]) -> Result<()> {
    let arch = arch_of(args)?;
    if arch != Arch::Amd64 {
        return Err("cd80: cd80.iso is amd64's (ROADMAP M16g)".into());
    }
    let (rev, osrev) = release(root)?;
    let iso = install_dir(root, arch).join(format!("cd{rev}.iso"));
    if !iso.is_file() {
        return Err(format!(
            "{}: no CD image; run `just cd-iso-{}`",
            iso.display(),
            arch.name()
        )
        .into());
    }
    // A smoke of `smokes`: its files go to the run directory (`smoke-all`'s per recipe).
    let work = boot::run_dir(root);
    fs::create_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;
    boot::set_boot_medium(BootMedium::Cdrom);
    let cmd = boot::qemu_command(
        root,
        arch,
        &iso,
        "stdio",
        None,
        &Disks {
            fresh: true,
            count: 1,
            set: Some("cd80"),
        },
    )?;
    boot::set_boot_medium(BootMedium::Disk);
    let log = work.join(format!("cd80-{}.log", arch.name()));
    let mut vm = Vm::spawn(&format!("cd80-{}", arch.name()), cmd, log.clone())?;
    let limit = boot::time_limit(Duration::from_secs(600));
    efiboot_on_com0(&mut vm, limit)?;
    vm.wait_for("(I)nstall, (U)pgrade, (A)utoinstall or (S)hell?", limit)?;
    let text = vm.text();
    for line in [
        format!("/{osrev}/{}/bsd.rd", arch.name()),
        "cd0 at scsibus".to_string(),
        "Welcome to the OpenBSD/amd64".to_string(),
    ] {
        if !text.contains(&line) {
            return Err(format!(
                "cd80 {}: never saw {line:?} (log: {})",
                arch.name(),
                log.display()
            )
            .into());
        }
    }
    // The CD as the kernel reads it: Rock Ridge names, and the file efiboot read.
    vm.send("s\n")?;
    vm.wait_for("# ", limit)?;
    vm.send(&format!(
        "cd /dev && sh MAKEDEV cd0 && cd / && mount_cd9660 /dev/cd0c /mnt && \
         ls /mnt/{osrev}/{} && cat /mnt/etc/boot.conf && umount /mnt && echo cd80-$((6*7))\n",
        arch.name()
    ))?;
    vm.wait_for("cd80-42", limit)?;
    let text = vm.text();
    drop(vm);
    for line in [
        "boot.catalog",
        "eficdboot",
        "TRANS.TBL",
        &format!("set image /{osrev}/{}/bsd.rd", arch.name()),
    ] {
        if !text.contains(line) {
            return Err(format!(
                "cd80 {}: the mounted CD lacks {line:?} (log: {})",
                arch.name(),
                log.display()
            )
            .into());
        }
    }
    println!("xtask: cd80 {}: ok", arch.name());
    Ok(())
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    /// The MBRs and labels of OpenBSD's own install80.img (the snapshot of
    /// `openbsd-snapshot.toml`, read with a hex dump): partition entries byte for byte.
    #[test]
    fn mbr_as_fdisk_writes_it() {
        let m = mbr(&Layout::of(Arch::Amd64));
        assert_eq!(
            m[446..462],
            [
                0x00, 0x00, 0x01, 0x01, 0xef, 0xfe, 0xff, 0xff, 64, 0, 0, 0, 0xc0, 0x03, 0, 0
            ]
        );
        assert_eq!(m[462..494], [0u8; 32]);
        let ob = &m[494..510];
        assert_eq!(&ob[..8], &[0x80, 0x00, 0x01, 0x10, 0xa6, 0xfe, 0xff, 0xff]);
        assert_eq!(u32::from_le_bytes(ob[8..12].try_into().unwrap()), 1024);
        assert_eq!(
            u32::from_le_bytes(ob[12..16].try_into().unwrap()),
            1_637_376
        );
        assert_eq!(m[510..], [0x55, 0xaa]);

        let m = mbr(&Layout::of(Arch::Arm64));
        assert_eq!(
            &m[446..454],
            &[0x80, 0xfe, 0xff, 0xff, 0x0c, 0x00, 0x74, 0xeb]
        );
        assert_eq!(u32::from_le_bytes(m[454..458].try_into().unwrap()), 32_768);
        assert_eq!(u32::from_le_bytes(m[458..462].try_into().unwrap()), 16_384);
        let ob = &m[494..510];
        assert_eq!(&ob[..8], &[0x00, 0x00, 0x75, 0xeb, 0xa6, 0xfe, 0xff, 0xff]);
        assert_eq!(u32::from_le_bytes(ob[8..12].try_into().unwrap()), 49_152);
        assert_eq!(
            u32::from_le_bytes(ob[12..16].try_into().unwrap()),
            1_384_448
        );
    }

    #[test]
    fn label_as_disklabel_writes_it() {
        let l = Layout::of(Arch::Amd64);
        let d = disklabel(&l, 16384, 2048, 16142);
        let u32at = |o: usize| u32::from_le_bytes(d[o..o + 4].try_into().unwrap());
        assert_eq!(u32at(0), DISKMAGIC);
        assert_eq!(u32at(60), 1_639_360); // d_secperunit
        assert_eq!(u32at(52), 16_393); // d_ncylinders
        assert_eq!((u32at(80), u32at(84)), (1024, 1_638_400)); // d_bstart, d_bend
        // a, c, i as OpenBSD's: a FFS at 1024 with fragblock 20, i MSDOS at 64.
        assert_eq!((u32at(148), u32at(152)), (1_637_376, 1024));
        assert_eq!((d[160], d[161]), (FS_BSDFFS, 20));
        assert_eq!(u16::from_le_bytes([d[162], d[163]]), 16142);
        assert_eq!((u32at(180), u32at(184)), (1_639_360, 0));
        assert_eq!((u32at(276), u32at(280)), (960, 64));
        assert_eq!(d[288], FS_MSDOS);
        // dkcksum: the XOR of the label's words is zero.
        let x = d[..148 + 16 * 16]
            .chunks(2)
            .fold(0u16, |s, w| s ^ u16::from_le_bytes([w[0], w[1]]));
        assert_eq!(x, 0);
        let a = disklabel(&Layout::of(Arch::Arm64), 16384, 2048, 1);
        let a32 = |o: usize| u32::from_le_bytes(a[o..o + 4].try_into().unwrap());
        assert_eq!((a32(80), a32(84)), (49_152, 1_433_600));
        assert_eq!((a32(276), a32(280)), (16_384, 32_768));
    }

    #[test]
    fn fragblock_values() {
        assert_eq!(fragblock(2048, 8), 20);
        assert_eq!(fragblock(1024, 8), 12);
        assert_eq!(fragblock(0, 8), 0);
    }

    #[test]
    fn answers_cover_the_stick() {
        let a = answers(Arch::Arm64, "8.0");
        assert!(a.iter().all(|(q, _)| *q != "boot> "));
        let sets = a.iter().find(|(q, _)| *q == "Location of sets?").unwrap();
        assert_eq!(sets.1, ["disk\n", "done\n"]);
        let path = a
            .iter()
            .find(|(q, _)| *q == "Pathname to the sets?")
            .unwrap();
        assert_eq!(path.1, ["8.0/arm64\n"]);
        let a = answers(Arch::Amd64, "8.0");
        assert!(a.iter().all(|(q, _)| *q != "boot> "));
        let sets = a.iter().find(|(q, _)| *q == "Set name(s)?").unwrap();
        assert_eq!(sets.1, ["-x* -game* -man*\n", "done\n"]);
    }
}
/* </TESTS> */
