//! M13's QEMU device options, and the disk images they attach (`docs/ARCHITECTURE.md`,
//! "Parallel smokes" for where the files go).
//!
//! - `--nvme FILE` (`qemu`, `smoke`): an NVM Express controller (`-device nvme`, nvme(4))
//!   whose one namespace is the raw image FILE (a relative path is in the run directory,
//!   [`boot::run_dir`]). amd64 only: it sits on `q35`'s PCI bus, added right after the NICs
//!   so that it comes before the persistent virtio-blk disks and its namespace is `sd0`;
//!   arm64's `virt` has no PCI bus in the kernel until M12.
//! - `--ahci FILE` (`qemu`, `smoke`): a SATA disk holding the raw image FILE (a relative
//!   path is in the run directory) on the second port of q35's built-in AHCI controller
//!   (`ich9-ahci` at 0:1f.2, `ide-hd` on `ide.1`; the boot image is on `ide.0`), ahci(4).
//!   The controller's place on the bus does not change, so the disk is the unit after the
//!   boot image's (`sd2` with the one persistent virtio-blk disk). amd64 only: arm64's
//!   `virt` has no AHCI controller of its own, and no PCI bus in the kernel until M12.
//! - `--scsi-cd ISO` (`qemu`, `smoke`, `smoke2`): a virtio SCSI host adapter with a CD-ROM
//!   drive holding the file `ISO` (read-only, `media=cdrom`; a relative path is taken from the
//!   workspace root): `virtio-scsi-pci` on amd64, `virtio-scsi-device` (virtio-mmio) on arm64,
//!   with `scsi-cd` on its bus. It is the LAST device added on both archs ([`add_devices`]),
//!   so the numbering of the other virtio devices is unchanged: amd64's PCI slots go up, and
//!   arm64 `virt` hands virtio-mmio slots out from the top down while the kernel finds them
//!   bottom up (the adapter takes the lowest slot, `vioscsi0`, found first; the NIC and the
//!   disks keep their slots, hence their names). Its `scsibus` is the one attached first.
//! - `cargo xtask nvme-root --arch A [--duid HEX] [--out FILE]`: the disk `just smoke-nvme`
//!   boots from, `nvme-<arch>.img` in the run directory unless `--out` names another file.
//!   It is laid out as OpenBSD's installer lays a disk out: an MBR whose one partition is
//!   the OpenBSD one (`DOSPTYP_OPENBSD`, 0xA6) from sector 64, the disklabel in its sector
//!   `LABELSECTOR` (1), partition `a` (4.2BSD) from sector 64 holding the file system, `c`
//!   the whole disk. The file system is the userland's ffs image
//!   (`target/userland/<arch>/ramdisk.ffs`, made by OpenBSD's makefs, `userland/ramdisk.rs`)
//!   copied whole, with one change: its `/etc/fstab` names the root `/dev/sd0a`, where the
//!   ramdisk's names `/dev/rd0a` (rc(8)'s `mount -uw /` looks the root's device up there;
//!   `--root-dev sdNa` names another unit, for a disk that is not the first: `smoke-ahci`'s
//!   is `sd2a`, as diskmap(4) and DUIDs in fstab are not ported). The two lines have the
//!   same length, so the file is rewritten in place in the copy. The
//!   label's DUID is `--duid` (16 hexadecimal digits, [`NVME_ROOT_DUID`] by default): the
//!   kernel's command line names it with `bootduid=` (boot(8)'s `BOOTARG_BOOTDUID`), and
//!   `setroot` mounts the root from the disk whose label has it.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use crate::Result;
use crate::boot::{self, Arch};

/// The DUID of the root disk `nvme-root` writes by default (`NVMEROOT` in ASCII).
pub(crate) const NVME_ROOT_DUID: &str = "4e564d45524f4f54";

/// The bytes of a sector (`DEV_BSIZE`).
const SECTOR: u64 = 512;
/// The OpenBSD partition's first sector, as fdisk(8) puts it (and partition `a`'s).
const OPENBSD_START: u64 = 64;
/// The disk is a whole number of MiB.
const ALIGN: u64 = 2048;
/// `LABELSECTOR`: the label's sector, relative to the OpenBSD partition.
const LABELSECTOR: u64 = 1;
/// `DOSPTYP_OPENBSD`.
const DOSPTYP_OPENBSD: u8 = 0xa6;
/// `DISKMAGIC`.
const DISKMAGIC: u32 = 0x8256_4557;
/// `DTYPE_SCSI`: sd(4)'s disks.
const DTYPE_SCSI: u16 = 4;
/// `MAXPARTITIONS` (amd64 and arm64).
const MAXPARTITIONS: u16 = 16;
/// `FS_BSDFFS`.
const FS_BSDFFS: u8 = 7;
/// `DISKLABELV1_FFS_FRAGBLOCK(512, 8)`: makefs's 512-byte fragments, 4096-byte blocks.
const FFS_FRAGBLOCK: u8 = 4;
/// `BBSIZE` and `SBSIZE` (`ufs/ffs/fs.h`), for `d_bbsize` and `d_sbsize`.
const BBSIZE: u32 = 8192;
const SBSIZE: u32 = 8192;

/// The ramdisk's root line in `/etc/fstab` (`userland/ramdisk.rs`, `FSTAB`) and the disk's.
const FSTAB_RD0A: &[u8] = b"/dev/rd0a / ffs rw 1 1\n";
const FSTAB_SD0A: &[u8] = b"/dev/sd0a / ffs rw 1 1\n";

/// The fstab root line naming `dev` (`sd0a` by default; `--root-dev`): four characters, a
/// two-letter driver, a unit digit and a partition letter, so that the line keeps the
/// ramdisk's length.
fn fstab_root_line(dev: Option<&str>) -> Result<Vec<u8>> {
    let Some(dev) = dev else {
        return Ok(FSTAB_SD0A.to_vec());
    };
    let b = dev.as_bytes();
    if b.len() != 4
        || !b[..2].iter().all(u8::is_ascii_lowercase)
        || !b[2].is_ascii_digit()
        || !(b'a'..=b'p').contains(&b[3])
    {
        return Err(format!("--root-dev {dev}: expected a disk partition such as sd2a").into());
    }
    Ok(format!("/dev/{dev} / ffs rw 1 1\n").into_bytes())
}

/// `--nvme FILE` for every VM this run starts (set once by `main`).
static NVME: OnceLock<PathBuf> = OnceLock::new();

/// `--ahci FILE` for every VM this run starts (set once by `main`).
static AHCI: OnceLock<PathBuf> = OnceLock::new();

/// `--scsi-cd ISO` for every VM this run starts (set once by `main`).
static SCSI_CD: OnceLock<PathBuf> = OnceLock::new();

/// Records this run's device options (`--nvme`, `--ahci`, `--scsi-cd`).
pub(crate) fn set(root: &Path, args: &[&str]) -> Result<()> {
    if let Some(w) = args.windows(2).find(|w| w[0] == "--nvme") {
        let _ = NVME.set(boot::run_dir(root).join(w[1]));
    }
    if let Some(w) = args.windows(2).find(|w| w[0] == "--ahci") {
        let _ = AHCI.set(boot::run_dir(root).join(w[1]));
    }
    if let Some(i) = args.iter().position(|a| *a == "--scsi-cd") {
        let Some(iso) = args.get(i + 1) else {
            return Err("--scsi-cd: expected the path of an ISO file".into());
        };
        let _ = SCSI_CD.set(PathBuf::from(iso));
    }
    Ok(())
}

/// Adds the PCI storage controllers this run asked for to `cmd` (amd64's PCI bus: called
/// after the NICs, before the virtio-blk disks), and the `--ahci` disk.
pub(crate) fn pci_storage(cmd: &mut Command, arch: Arch) -> Result<()> {
    if let Some(image) = AHCI.get() {
        if arch != Arch::Amd64 {
            return Err("--ahci: amd64 only (q35's AHCI controller; arm64 waits for M12)".into());
        }
        if !image.is_file() {
            return Err(format!(
                "--ahci {}: no such image (cargo xtask nvme-root makes one)",
                image.display()
            )
            .into());
        }
        cmd.args(ahci_args(image));
    }
    let Some(image) = NVME.get() else {
        return Ok(());
    };
    if arch != Arch::Amd64 {
        return Err("--nvme: amd64 only (arm64 has no PCI bus before M12)".into());
    }
    if !image.is_file() {
        return Err(format!(
            "--nvme {}: no such image (cargo xtask nvme-root makes one)",
            image.display()
        )
        .into());
    }
    cmd.arg("-drive").arg(format!(
        "if=none,format=raw,file={},id=nvm0",
        image.display()
    ));
    cmd.args(["-device", "nvme,drive=nvm0,serial=EMIBSD0001"]);
    Ok(())
}

/// The QEMU arguments of the `--ahci` disk: `image` on port 1 of q35's AHCI controller.
fn ahci_args(image: &Path) -> Vec<String> {
    vec![
        "-drive".into(),
        format!("if=none,format=raw,file={},id=ahci1", image.display()),
        "-device".into(),
        "ide-hd,drive=ahci1,bus=ide.1".into(),
    ]
}

/// The bytes of a DUID written as 16 hexadecimal digits.
fn parse_duid(hex: &str) -> Result<[u8; 8]> {
    let bad = || format!("--duid {hex}: expected 16 hexadecimal digits");
    if hex.len() != 16 || !hex.is_ascii() {
        return Err(bad().into());
    }
    let mut duid = [0u8; 8];
    for (i, d) in duid.iter_mut().enumerate() {
        *d = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).map_err(|_| bad())?;
    }
    Ok(duid)
}

/// The MBR: one OpenBSD partition from `start`, `sectors` long, the rest of the disk.
fn mbr(start: u64, sectors: u64) -> [u8; 512] {
    let mut s = [0u8; 512];
    let e = &mut s[446..462];
    e[0] = 0x80; // active
    e[1..4].copy_from_slice(&[0xfe, 0xff, 0xff]);
    e[4] = DOSPTYP_OPENBSD;
    e[5..8].copy_from_slice(&[0xfe, 0xff, 0xff]);
    e[8..12].copy_from_slice(&(start as u32).to_le_bytes());
    e[12..16].copy_from_slice(&(sectors as u32).to_le_bytes());
    s[510] = 0x55;
    s[511] = 0xaa;
    s
}

/// `struct disklabel` (`sys/disklabel.h`, little-endian) for a disk of `total` sectors whose
/// partition `a` is the `fs` sectors from [`OPENBSD_START`], with `dkcksum` filled in.
fn disklabel(total: u64, fs: u64, duid: [u8; 8]) -> [u8; 512] {
    let mut l = [0u8; 512];
    let put = |l: &mut [u8; 512], off: usize, b: &[u8]| l[off..off + b.len()].copy_from_slice(b);
    let (nsectors, ntracks) = (63u32, 255u32);
    let secpercyl = nsectors * ntracks;

    put(&mut l, 0, &DISKMAGIC.to_le_bytes()); // d_magic
    put(&mut l, 4, &DTYPE_SCSI.to_le_bytes()); // d_type
    put(&mut l, 8, b"NVMe"); // d_typename
    put(&mut l, 24, b"emibsd root"); // d_packname
    put(&mut l, 40, &(SECTOR as u32).to_le_bytes()); // d_secsize
    put(&mut l, 44, &nsectors.to_le_bytes()); // d_nsectors
    put(&mut l, 48, &ntracks.to_le_bytes()); // d_ntracks
    let ncylinders = (total / u64::from(secpercyl)) as u32;
    put(&mut l, 52, &ncylinders.to_le_bytes()); // d_ncylinders
    put(&mut l, 56, &secpercyl.to_le_bytes()); // d_secpercyl
    put(&mut l, 60, &(total as u32).to_le_bytes()); // d_secperunit
    put(&mut l, 64, &duid); // d_uid
    put(&mut l, 80, &(OPENBSD_START as u32).to_le_bytes()); // d_bstart
    put(&mut l, 84, &(total as u32).to_le_bytes()); // d_bend
    put(&mut l, 112, &((total >> 32) as u16).to_le_bytes()); // d_secperunith
    put(&mut l, 114, &1u16.to_le_bytes()); // d_version
    put(&mut l, 132, &DISKMAGIC.to_le_bytes()); // d_magic2
    put(&mut l, 138, &MAXPARTITIONS.to_le_bytes()); // d_npartitions
    put(&mut l, 140, &BBSIZE.to_le_bytes()); // d_spare2 (was d_bbsize)
    put(&mut l, 144, &SBSIZE.to_le_bytes()); // d_spare3 (was d_sbsize)
    // a: the file system.
    put(&mut l, 148, &(fs as u32).to_le_bytes()); // p_size
    put(&mut l, 152, &(OPENBSD_START as u32).to_le_bytes()); // p_offset
    l[148 + 12] = FS_BSDFFS; // p_fstype
    l[148 + 13] = FFS_FRAGBLOCK; // p_fragblock
    // c: the whole disk.
    put(&mut l, 148 + 2 * 16, &(total as u32).to_le_bytes());

    // dkcksum: the XOR of the 16-bit words up to the end of d_partitions[d_npartitions].
    let end = 148 + 16 * usize::from(MAXPARTITIONS);
    let sum = l[..end]
        .chunks(2)
        .fold(0u16, |s, w| s ^ u16::from_le_bytes([w[0], w[1]]));
    put(&mut l, 136, &sum.to_le_bytes()); // d_checksum
    l
}

/// The file system with its root's fstab line naming `line` (`sd0a`'s by default, see the
/// module docs).
fn rewrite_fstab(fs: &mut [u8], line: &[u8]) -> Result<()> {
    let at: Vec<usize> = fs
        .windows(FSTAB_RD0A.len())
        .enumerate()
        .filter(|(_, w)| *w == FSTAB_RD0A)
        .map(|(i, _)| i)
        .collect();
    let [i] = at[..] else {
        return Err(format!(
            "the file system holds {} copies of the ramdisk's fstab root line, not one",
            at.len()
        )
        .into());
    };
    if line.len() != FSTAB_RD0A.len() {
        return Err("the fstab root line must keep the ramdisk's length".into());
    }
    fs[i..i + line.len()].copy_from_slice(line);
    Ok(())
}

/// `cargo xtask nvme-root`: writes the NVMe root disk (module docs).
pub(crate) fn nvme_root(
    root: &Path,
    arch: Arch,
    duid: Option<&str>,
    out: Option<&str>,
    root_dev: Option<&str>,
) -> Result<()> {
    let duid = parse_duid(duid.unwrap_or(NVME_ROOT_DUID))?;
    let Some(src) = boot::default_ramdisk(root, arch) else {
        return Err(format!(
            "no target/userland/{}/{}: run `just userland` first",
            arch.name(),
            boot::RAMDISK_MODULE
        )
        .into());
    };
    let mut fs = fs::read(&src).map_err(|e| format!("{}: {e}", src.display()))?;
    if !(fs.len() as u64).is_multiple_of(SECTOR) || fs.is_empty() {
        return Err(format!("{}: not a whole number of sectors", src.display()).into());
    }
    rewrite_fstab(&mut fs, &fstab_root_line(root_dev)?)?;

    let fs_sectors = fs.len() as u64 / SECTOR;
    let total = (OPENBSD_START + fs_sectors).div_ceil(ALIGN) * ALIGN;
    let mut disk = vec![0u8; (total * SECTOR) as usize];
    disk[..512].copy_from_slice(&mbr(OPENBSD_START, total - OPENBSD_START));
    let fs_at = (OPENBSD_START * SECTOR) as usize;
    disk[fs_at..fs_at + fs.len()].copy_from_slice(&fs);
    // The label goes in the file system's boot area, which ffs leaves free.
    let label_at = ((OPENBSD_START + LABELSECTOR) * SECTOR) as usize;
    disk[label_at..label_at + 512].copy_from_slice(&disklabel(total, fs_sectors, duid));

    let out = match out {
        Some(o) => boot::run_dir(root).join(o),
        None => boot::run_dir(root).join(format!("nvme-{}.img", arch.name())),
    };
    if let Some(dir) = out.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    fs::write(&out, &disk).map_err(|e| format!("{}: {e}", out.display()))?;
    println!(
        "nvme-root: {} ({total} sectors; sd0a: {fs_sectors} sectors at {OPENBSD_START}, \
         DUID {}, fstab root /dev/sd0a)",
        out.display(),
        duid.iter().map(|b| format!("{b:02x}")).collect::<String>()
    );
    Ok(())
}

/// The QEMU arguments of the `--scsi-cd` device on `arch`, in order. A relative `iso` is
/// taken from the workspace `root`.
fn scsi_cd_args(root: &Path, arch: Arch, iso: &Path) -> Vec<String> {
    let iso = if iso.is_absolute() {
        iso.to_path_buf()
    } else {
        root.join(iso)
    };
    let adapter = match arch {
        Arch::Amd64 => "virtio-scsi-pci,id=scsi0",
        Arch::Arm64 => "virtio-scsi-device,id=scsi0",
    };
    vec![
        "-drive".into(),
        format!(
            "if=none,format=raw,file={},id=cd0,media=cdrom,readonly=on",
            iso.display()
        ),
        "-device".into(),
        adapter.into(),
        "-device".into(),
        "scsi-cd,drive=cd0,bus=scsi0.0".into(),
    ]
}

/// Adds the devices that go last on the command line to `cmd` (`--scsi-cd`, see the module
/// docs).
pub(crate) fn add_devices(cmd: &mut Command, root: &Path, arch: Arch) {
    if let Some(iso) = SCSI_CD.get() {
        cmd.args(scsi_cd_args(root, arch, iso));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::e2fs;

    #[test]
    fn label_is_found_as_the_kernel_finds_it() {
        let total = 131_072;
        let duid = parse_duid(NVME_ROOT_DUID).unwrap();
        assert_eq!(&duid, b"NVMEROOT");
        let m = mbr(OPENBSD_START, total - OPENBSD_START);
        assert_eq!(e2fs::openbsd_mbr_start(&m).unwrap(), OPENBSD_START);
        let l = disklabel(total, 129_024, duid);
        // dkcksum over the label, checksum included, is 0.
        let sum = l[..148 + 16 * 16]
            .chunks(2)
            .fold(0u16, |s, w| s ^ u16::from_le_bytes([w[0], w[1]]));
        assert_eq!(sum, 0);
        assert_eq!(&l[64..72], b"NVMEROOT");
        // Partition a is typed 4.2BSD, so e2fs's ext2 reader refuses it by its type.
        let err = e2fs::ext2_partition(&l, 0).unwrap_err().to_string();
        assert!(err.contains("fstype 7"), "{err}");
    }

    #[test]
    fn fstab_root_line_is_rewritten_once() {
        let mut fs =
            b"xx/dev/rd0a / ffs rw 1 1\n/dev/sd0a /mnt ffs rw,userquota,noauto 1 2\n".to_vec();
        rewrite_fstab(&mut fs, FSTAB_SD0A).unwrap();
        assert!(fs.starts_with(b"xx/dev/sd0a / ffs rw 1 1\n"));
        assert!(rewrite_fstab(&mut fs, FSTAB_SD0A).is_err());
        let mut fs = b"/dev/rd0a / ffs rw 1 1\n".to_vec();
        let line = fstab_root_line(Some("sd2a")).unwrap();
        rewrite_fstab(&mut fs, &line).unwrap();
        assert_eq!(fs, b"/dev/sd2a / ffs rw 1 1\n");
        assert_eq!(fstab_root_line(None).unwrap(), FSTAB_SD0A);
        for bad in ["sd10a", "sd2", "sd2z", "SD2a", "s2aa"] {
            assert!(fstab_root_line(Some(bad)).is_err(), "{bad}");
        }
        assert!(parse_duid("4e564d45524f4f5").is_err());
        assert!(parse_duid("4e564d45524f4fzz").is_err());
    }

    #[test]
    fn scsi_cd_is_a_read_only_cdrom_on_the_arch_s_virtio_bus() {
        let a = scsi_cd_args(Path::new("/r"), Arch::Amd64, Path::new("t/cd.iso"));
        assert_eq!(a[0], "-drive");
        assert!(a[1].contains("file=/r/t/cd.iso"));
        assert!(a[1].contains("media=cdrom,readonly=on"));
        assert_eq!(a[3], "virtio-scsi-pci,id=scsi0");
        assert_eq!(a[5], "scsi-cd,drive=cd0,bus=scsi0.0");
        let b = scsi_cd_args(Path::new("/r"), Arch::Arm64, Path::new("/x/cd.iso"));
        assert!(b[1].contains("file=/x/cd.iso"));
        assert_eq!(b[3], "virtio-scsi-device,id=scsi0");
    }

    #[test]
    fn ahci_disk_is_on_the_second_port() {
        let args = ahci_args(Path::new("/r/target/smoke/x/ahci-amd64.img"));
        assert_eq!(
            args,
            [
                "-drive",
                "if=none,format=raw,file=/r/target/smoke/x/ahci-amd64.img,id=ahci1",
                "-device",
                "ide-hd,drive=ahci1,bus=ide.1"
            ]
        );
    }

    #[test]
    fn scsi_cd_needs_a_path() {
        assert!(set(Path::new("/r"), &["--scsi-cd"]).is_err());
        assert!(set(Path::new("/r"), &["--arch", "arm64"]).is_ok());
    }
}
