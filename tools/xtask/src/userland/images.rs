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

use super::*;

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
    Ok(())
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
