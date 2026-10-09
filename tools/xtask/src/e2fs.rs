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
//! `cargo xtask e2fsck --arch A [--disk-set NAME] [--cat PATH=TEXT]...`: the host half of M10d's
//! ext2fs exit criterion. The guest made an ext2 file system with OpenBSD's newfs_ext2fs(8)
//! on partition `a` of `sd0` (`just smoke-ext2fs`); here e2fsprogs, an independent
//! implementation, checks it in the persistent disk image the guest wrote
//! (`target/disk-<arch>[-NAME].img`):
//!
//! - `e2fsck -fn` must exit 0 (no problem found, nothing changed: `-n` opens it read-only);
//! - for every `--cat PATH=TEXT`, `debugfs -R 'cat PATH'` must print a line holding `TEXT`.
//!
//! The partition is found as OpenBSD finds it (`readdoslabel`, `sys/kern/subr_disk.c`): the
//! MBR's OpenBSD partition (type `DOSPTYP_OPENBSD`, 0xA6), its disklabel in sector
//! `LABELSECTOR` (1) of that partition, and partition `a` of the label, which must be typed
//! `FS_EXT2FS`. Its offsets are absolute sectors of the disk. e2fsprogs is pointed at it with
//! its `unix` I/O manager's `offset` option (`image?offset=BYTES`), so nothing is copied.
//!
//! e2fsprogs is Homebrew's keg-only formula `e2fsprogs`: its tools are located only through
//! `brew --prefix e2fsprogs` (`<prefix>/sbin/e2fsck`, `<prefix>/sbin/debugfs`), docs/SETUP.md.

use std::fs::File;
use std::io::{Read as _, Seek as _, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::Result;
use crate::boot::{self, Arch};

/// `DOSPARTOFF` (`sys/disklabel.h`): the MBR's partition table.
const DOSPARTOFF: usize = 446;
/// `NDOSPART`: entries in it, 16 bytes each (`struct dos_partition`).
const NDOSPART: usize = 4;
/// `DOSMBR_SIGNATURE_OFF` and `DOSMBR_SIGNATURE` (0xAA55, little-endian on disk).
const DOSMBR_SIGNATURE_OFF: usize = 0x1fe;
const DOSMBR_SIGNATURE: u16 = 0xaa55;
/// `DOSPTYP_OPENBSD`: the MBR partition type OpenBSD keeps its disklabel in.
const DOSPTYP_OPENBSD: u8 = 0xa6;
/// `LABELSECTOR` (amd64 and arm64 `<machine/disklabel.h>`): the label's sector, relative to
/// the OpenBSD MBR partition; `LABELOFFSET` is 0.
const LABELSECTOR: u64 = 1;
/// `DISKMAGIC`: `d_magic` and `d_magic2`.
const DISKMAGIC: u32 = 0x8256_4557;
/// `FS_EXT2FS`: the partition's `p_fstype`.
const FS_EXT2FS: u8 = 17;
/// The bytes of a sector as the MBR counts them (`DEV_BSIZE`).
const DEV_BSIZE: u64 = 512;

/// Field offsets in `struct disklabel` (`sys/disklabel.h`): `d_secsize`, `d_magic2`,
/// `d_npartitions` and `d_partitions`, whose entries (`struct partition`) are 16 bytes:
/// `p_size` 0, `p_offset` 4, `p_offseth` 8, `p_sizeh` 10, `p_fstype` 12.
const D_SECSIZE: usize = 40;
const D_MAGIC2: usize = 132;
const D_NPARTITIONS: usize = 138;
const D_PARTITIONS: usize = 148;
const PARTITION_SIZE: usize = 16;

/// Where partition `a` is: its first byte and its length in the disk image.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Extent {
    pub offset: u64,
    pub len: u64,
}

fn u16_at(b: &[u8], o: usize) -> Result<u16> {
    b.get(o..o + 2)
        .map(|s| u16::from_le_bytes([s[0], s[1]]))
        .ok_or_else(|| format!("short read at byte {o}").into())
}

fn u32_at(b: &[u8], o: usize) -> Result<u32> {
    b.get(o..o + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or_else(|| format!("short read at byte {o}").into())
}

/// The OpenBSD MBR partition's first sector, from the MBR in `mbr` (sector 0).
pub(crate) fn openbsd_mbr_start(mbr: &[u8]) -> Result<u64> {
    if u16_at(mbr, DOSMBR_SIGNATURE_OFF)? != DOSMBR_SIGNATURE {
        return Err("no MBR signature (0xAA55) in sector 0".into());
    }
    for i in 0..NDOSPART {
        let e = DOSPARTOFF + i * 16;
        if mbr.get(e + 4) == Some(&DOSPTYP_OPENBSD) {
            return Ok(u64::from(u32_at(mbr, e + 8)?));
        }
    }
    Err("the MBR has no OpenBSD partition (type 0xA6)".into())
}

/// Partition `part` (0 for `a`) of the disklabel in `label`, which must be typed `FS_EXT2FS`.
pub(crate) fn ext2_partition(label: &[u8], part: usize) -> Result<Extent> {
    if u32_at(label, 0)? != DISKMAGIC || u32_at(label, D_MAGIC2)? != DISKMAGIC {
        return Err("no OpenBSD disklabel (DISKMAGIC) in the label sector".into());
    }
    if part >= usize::from(u16_at(label, D_NPARTITIONS)?) {
        return Err(format!("the disklabel has no partition {part}").into());
    }
    let secsize = u64::from(u32_at(label, D_SECSIZE)?);
    let p = D_PARTITIONS + part * PARTITION_SIZE;
    let size = u64::from(u32_at(label, p)?) | (u64::from(u16_at(label, p + 10)?) << 32);
    let offset = u64::from(u32_at(label, p + 4)?) | (u64::from(u16_at(label, p + 8)?) << 32);
    let fstype = *label.get(p + 12).ok_or("short disklabel")?;
    if fstype != FS_EXT2FS {
        return Err(format!(
            "partition {} has fstype {fstype}, not ext2fs ({FS_EXT2FS})",
            char::from(b'a' + part as u8)
        )
        .into());
    }
    if size == 0 || secsize == 0 {
        return Err("the ext2fs partition is empty".into());
    }
    Ok(Extent {
        offset: offset * secsize,
        len: size * secsize,
    })
}

/// Partition `a` of the OpenBSD disklabel in the disk image `disk`.
pub(crate) fn find_partition_a(disk: &Path) -> Result<Extent> {
    let mut f = File::open(disk).map_err(|e| format!("{}: {e}", disk.display()))?;
    let mut mbr = [0u8; DEV_BSIZE as usize];
    f.read_exact(&mut mbr)
        .map_err(|e| format!("{}: MBR: {e}", disk.display()))?;
    let start = openbsd_mbr_start(&mbr).map_err(|e| format!("{}: {e}", disk.display()))?;
    let mut label = [0u8; DEV_BSIZE as usize];
    f.seek(SeekFrom::Start((start + LABELSECTOR) * DEV_BSIZE))
        .and_then(|_| f.read_exact(&mut label))
        .map_err(|e| format!("{}: disklabel: {e}", disk.display()))?;
    ext2_partition(&label, 0).map_err(|e| format!("{}: {e}", disk.display()).into())
}

/// `brew --prefix e2fsprogs`, with the error docs/SETUP.md asks for when it is not there.
fn e2fsprogs_sbin() -> Result<PathBuf> {
    let missing = || {
        "e2fsprogs not found: `brew --prefix e2fsprogs` failed; install the Homebrew formula \
         `e2fsprogs` (keg-only) as docs/SETUP.md (\"e2fsprogs\") says"
            .to_string()
    };
    let out = Command::new("brew")
        .args(["--prefix", "e2fsprogs"])
        .output()
        .map_err(|_| missing())?;
    let prefix = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() || prefix.is_empty() {
        return Err(missing().into());
    }
    let sbin = PathBuf::from(prefix).join("sbin");
    for tool in ["e2fsck", "debugfs"] {
        if !sbin.join(tool).is_file() {
            return Err(format!(
                "{} not found; install the Homebrew formula `e2fsprogs` as docs/SETUP.md \
                 (\"e2fsprogs\") says",
                sbin.join(tool).display()
            )
            .into());
        }
    }
    Ok(sbin)
}

/// `cargo xtask e2fsck` (module docs). `cats` holds the `PATH=TEXT` pairs.
pub fn e2fsck(root: &Path, arch: Arch, set: Option<&str>, cats: &[&str]) -> Result<()> {
    let sbin = e2fsprogs_sbin()?;
    let disk = boot::disk_path(root, arch, set);
    let part = find_partition_a(&disk)?;
    let spec = format!("{}?offset={}", disk.display(), part.offset);
    println!(
        "e2fsck {}: partition a at byte {} ({} bytes), {}",
        arch.name(),
        part.offset,
        part.len,
        sbin.display()
    );

    let out = Command::new(sbin.join("e2fsck"))
        .args(["-f", "-n", &spec])
        .output()
        .map_err(|e| format!("e2fsck: {e}"))?;
    print!("{}", String::from_utf8_lossy(&out.stdout));
    eprint!("{}", String::from_utf8_lossy(&out.stderr));
    match out.status.code() {
        Some(0) => println!("e2fsck -fn: exit status 0, clean"),
        other => {
            return Err(format!(
                "e2fsck -fn {spec}: exit status {other:?}, expected 0 (no errors, nothing \
                 changed)"
            )
            .into());
        }
    }

    for cat in cats {
        let (path, text) = cat
            .split_once('=')
            .ok_or_else(|| format!("--cat {cat}: expected PATH=TEXT"))?;
        let out = Command::new(sbin.join("debugfs"))
            .args(["-R", &format!("cat {path}"), &spec])
            .output()
            .map_err(|e| format!("debugfs: {e}"))?;
        let stdout = String::from_utf8_lossy(&out.stdout);
        print!("debugfs cat {path}: {stdout}");
        if !out.status.success() || !stdout.lines().any(|l| l.contains(text)) {
            return Err(format!(
                "debugfs -R 'cat {path}' {spec}: expected `{text}`, got `{}` (stderr: {})",
                stdout.trim(),
                String::from_utf8_lossy(&out.stderr).trim()
            )
            .into());
        }
    }
    println!("e2fsck {}: ok", arch.name());
    Ok(())
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    fn put32(b: &mut [u8], o: usize, v: u32) {
        b[o..o + 4].copy_from_slice(&v.to_le_bytes());
    }

    fn put16(b: &mut [u8], o: usize, v: u16) {
        b[o..o + 2].copy_from_slice(&v.to_le_bytes());
    }

    /// An MBR with the OpenBSD partition in slot 3 (what `fdisk -iy` makes) at sector 64.
    fn mbr() -> [u8; 512] {
        let mut m = [0u8; 512];
        put16(&mut m, DOSMBR_SIGNATURE_OFF, DOSMBR_SIGNATURE);
        let e = DOSPARTOFF + 3 * 16;
        m[e + 4] = DOSPTYP_OPENBSD;
        put32(&mut m, e + 8, 64);
        put32(&mut m, e + 12, 131_008);
        m
    }

    fn label(fstype: u8) -> [u8; 512] {
        let mut l = [0u8; 512];
        put32(&mut l, 0, DISKMAGIC);
        put32(&mut l, D_MAGIC2, DISKMAGIC);
        put32(&mut l, D_SECSIZE, 512);
        put16(&mut l, D_NPARTITIONS, 16);
        put32(&mut l, D_PARTITIONS, 131_008);
        put32(&mut l, D_PARTITIONS + 4, 64);
        l[D_PARTITIONS + 12] = fstype;
        l
    }

    #[test]
    fn finds_the_openbsd_partition_and_its_ext2fs_a() -> Result<()> {
        assert_eq!(openbsd_mbr_start(&mbr())?, 64);
        assert_eq!(
            ext2_partition(&label(FS_EXT2FS), 0)?,
            Extent {
                offset: 64 * 512,
                len: 131_008 * 512
            }
        );
        Ok(())
    }

    #[test]
    fn rejects_what_is_not_an_ext2fs_partition() {
        let mut m = mbr();
        m[DOSPARTOFF + 3 * 16 + 4] = 0x0c;
        assert!(openbsd_mbr_start(&m).is_err());
        assert!(openbsd_mbr_start(&[0u8; 512]).is_err());
        // 4.2BSD (7) is not ext2fs; nor is a sector without DISKMAGIC.
        assert!(ext2_partition(&label(7), 0).is_err());
        assert!(ext2_partition(&[0u8; 512], 0).is_err());
        assert!(ext2_partition(&label(FS_EXT2FS), 16).is_err());
    }

    #[test]
    fn finds_partition_a_in_an_image() -> Result<()> {
        let dir = std::env::temp_dir().join(format!("emibsd-e2fs-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let img = dir.join("disk.img");
        let mut data = vec![0u8; 66 * 512];
        data[..512].copy_from_slice(&mbr());
        data[65 * 512..66 * 512].copy_from_slice(&label(FS_EXT2FS));
        std::fs::write(&img, &data)?;
        let found = find_partition_a(&img)?;
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(found.offset, 32_768);
        Ok(())
    }

    /// The constants above against `sys/disklabel.h` and amd64's and arm64's
    /// `<machine/disklabel.h>` (`just test-ref`).
    #[test]
    #[ignore]
    fn constants_match_the_reference_headers() {
        let src = PathBuf::from(std::env::var("OPENBSD_SRC").expect("OPENBSD_SRC"));
        let define = |file: &str, name: &str| -> u64 {
            let text = std::fs::read_to_string(src.join(file)).expect(file);
            let line = text
                .lines()
                .find(|l| {
                    let mut w = l.split_whitespace();
                    w.next() == Some("#define") && w.next() == Some(name)
                })
                .unwrap_or_else(|| panic!("{name} not in {file}"));
            let v = line.split_whitespace().nth(2).expect("value");
            let v = v.trim_start_matches('(').trim_end_matches(')');
            let v = v.trim_end_matches('U');
            match v.strip_prefix("0x") {
                Some(h) => u64::from_str_radix(h, 16).expect("hex"),
                None => v.parse().expect("number"),
            }
        };
        let h = "sys/sys/disklabel.h";
        assert_eq!(define(h, "DISKMAGIC"), u64::from(DISKMAGIC));
        assert_eq!(define(h, "FS_EXT2FS"), u64::from(FS_EXT2FS));
        assert_eq!(define(h, "DOSPTYP_OPENBSD"), u64::from(DOSPTYP_OPENBSD));
        assert_eq!(define(h, "DOSPARTOFF"), DOSPARTOFF as u64);
        assert_eq!(define(h, "NDOSPART"), NDOSPART as u64);
        assert_eq!(
            define(h, "DOSMBR_SIGNATURE_OFF"),
            DOSMBR_SIGNATURE_OFF as u64
        );
        assert_eq!(define(h, "DOSMBR_SIGNATURE"), u64::from(DOSMBR_SIGNATURE));
        for arch in ["amd64", "arm64"] {
            let m = format!("sys/arch/{arch}/include/disklabel.h");
            assert_eq!(define(&m, "LABELSECTOR"), LABELSECTOR);
            assert_eq!(define(&m, "LABELOFFSET"), 0);
        }
    }
}
/* </TESTS> */
