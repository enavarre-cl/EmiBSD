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
//! M10c's test images, staged into the ramdisk's `/root/images` before the ramdisk is made.
//!
//! `just smoke-fs` attaches each one to a vnd(4) with vnconfig(8), mounts it and reads its
//! known file back:
//!
//! - `fat.img`: a 2 MiB FAT12 file system made by OpenBSD's makefs(8) (`-t msdos`, the same
//!   host build as the ramdisk's), with no partition table, as `newfs_msdos` makes one;
//! - `cd.iso`: an ISO 9660 image with Rock Ridge, by makefs (`-t cd9660 -o rockridge`);
//! - `udf.img`: a UDF image by macOS's own `hdiutil makehybrid -udf` (part of macOS,
//!   nothing installed): OpenBSD has no UDF writer, and makefs makes none.
//!
//! Each holds one file, `m10c-<fs>.txt` (a long name: FAT's Windows 95 entries, ISO's Rock
//! Ridge names), whose one line is `m10c-<fs>-42`.
//!
//! M10d adds, on amd64 only (the only kernel with ntfs), `ntfs.img`: a 4 MiB NTFS 3.1 volume
//! by our own generator (`ntfsgen.rs`; there is no mkntfs for macOS), holding `m10d-ntfs.txt`
//! (resident in its MFT record), whose one line is `m10d-ntfs-42`, and `m10d-ntfs-big.txt`
//! (500 lines in three clusters, the last `m10d-ntfs-big-line-0499`). Every time it is made,
//! macOS's own NTFS driver mounts it read-only and reads both files back; a Mac without
//! `/System/Library/Filesystems/ntfs.fs` fails there, and `EMIBSD_NTFS_CHECK=0` skips the
//! check (with a warning).

use super::*;
use crate::ntfsgen;

/// macOS's disk image tool (`/usr/bin/hdiutil`).
const HDIUTIL: &str = "/usr/bin/hdiutil";

/// The file systems of the images: (image name, file system label, makefs arguments; empty
/// for the UDF image, which hdiutil makes).
const IMAGES: &[(&str, &str, &[&str])] = &[
    (
        "fat.img",
        "fat",
        &[
            "-t",
            "msdos",
            "-s",
            "2m",
            "-o",
            "fat_type=12,volume_label=M10C",
        ],
    ),
    (
        "cd.iso",
        "iso",
        &["-t", "cd9660", "-o", "rockridge,label=M10C"],
    ),
    ("udf.img", "udf", &[]),
];

/// Makes the images into `dir` (the ramdisk staging tree's `root/images/`), with `makefs`.
pub(super) fn make_images(ctx: &Ctx<'_>, makefs: &Path, dir: &Path) -> Result<()> {
    fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for (image, label, args) in IMAGES {
        let src = ctx.out.join("host/images").join(label);
        if src.exists() {
            fs::remove_dir_all(&src).map_err(|e| format!("{}: {e}", src.display()))?;
        }
        fs::create_dir_all(&src).map_err(|e| format!("{}: {e}", src.display()))?;
        let file = src.join(format!("m10c-{label}.txt"));
        fs::write(&file, format!("m10c-{label}-42\n"))
            .map_err(|e| format!("{}: {e}", file.display()))?;
        let to = dir.join(image);
        let _ = fs::remove_file(&to);
        if args.is_empty() {
            make_udf(&src, &to)?;
        } else {
            run(Command::new(makefs)
                .args(["-T", &ramdisk::TIMESTAMP.to_string()])
                .args(*args)
                .arg(&to)
                .arg(&src))?;
        }
        let size = fs::metadata(&to).map(|m| m.len()).unwrap_or(0);
        println!("  images: {image} ({size} bytes, m10c-{label}.txt)");
    }
    if ctx.m.machine == "amd64" {
        make_ntfs(&dir.join("ntfs.img"))?;
    }
    Ok(())
}

/// M10d's `ntfs.img` at `to`, checked by macOS's NTFS driver unless `EMIBSD_NTFS_CHECK=0`.
fn make_ntfs(to: &Path) -> Result<()> {
    let check = std::env::var_os("EMIBSD_NTFS_CHECK").is_none_or(|v| v != "0");
    if !check {
        println!("  images: warning: ntfs.img is not checked by macOS (EMIBSD_NTFS_CHECK=0)");
    }
    ntfsgen::ntfs_image(to, check)
}

/// `hdiutil makehybrid -udf`: a UDF-only image of `src` at `to`. hdiutil names its output
/// `<name>.iso` whatever it is asked for, so it writes beside `to` and the image is renamed.
fn make_udf(src: &Path, to: &Path) -> Result<()> {
    if !Path::new(HDIUTIL).is_file() {
        return Err(
            format!("{HDIUTIL} not found: the UDF test image needs macOS's hdiutil").into(),
        );
    }
    let iso = to.with_extension("iso");
    let _ = fs::remove_file(&iso);
    run(Command::new(HDIUTIL)
        .args([
            "makehybrid",
            "-quiet",
            "-udf",
            "-udf-volume-name",
            "M10C",
            "-o",
        ])
        .arg(&iso)
        .arg(src))?;
    fs::rename(&iso, to).map_err(|e| format!("{} -> {}: {e}", iso.display(), to.display()).into())
}
/* </CODE> */
