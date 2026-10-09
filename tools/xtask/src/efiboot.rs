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
//! M14: OpenBSD's efiboot as the boot loader (`sys/arch/amd64/stand/efiboot`).
//!
//! - `cargo xtask efiboot --arch amd64 --elf FILE [--out FILE]`: the PE32+ image
//!   `BOOTX64.EFI` of the efiboot ELF that `just efiboot-amd64` linked (position-independent,
//!   at 0, with its PE header first: `ldscript.amd64`, `start_amd64.S`), made as arm64's
//!   efiboot makes its own: `llvm-objcopy -O binary` (OpenBSD's amd64 Makefile uses GNU
//!   objcopy's `efi-app-x86_64` target, which llvm-objcopy does not have). The default output
//!   is `target/efiboot/<arch>/BOOTX64.EFI`. llvm-objcopy is the toolchain's (`llvm-tools`,
//!   `rust-toolchain.toml`), else `$EMIBSD_LLVM_BIN/llvm-objcopy`.
//! - `cargo xtask efiboot-disk --arch amd64 --efi FILE --kernel FILE`: the boot image of
//!   `smoke-efiboot`, written where `smoke` without `--kernel` boots from (the run
//!   directory's `emibsd-<arch>.img`), laid out as OpenBSD installs a disk that boots with
//!   efiboot: an MBR whose first partition is the OpenBSD one (`DOSPTYP_OPENBSD`, from sector
//!   64; the disklabel in its sector 1, partition `a` the FFS holding `/bsd`,
//!   `/etc/boot.conf` and `/etc/random.seed`, `c` the whole disk) and whose second is the
//!   EFI system partition (FAT, `EFI/BOOT/BOOTX64.EFI`). The FFS is made by OpenBSD's makefs,
//!   the host build `cargo xtask userland` leaves (`userland/ramdisk.rs`); the label and the
//!   MBR come from `hwopts.rs`, as `nvme-root`'s.
//! - `... efiboot-disk ... --root-dev DEV` (M14 track A2): partition `a` is a whole root, the
//!   tree `just userland` stages for the ramdisk (`target/userland/<arch>/ramdisk-root`, with
//!   its ownership table `host/owners.txt`) plus the three files above, its `/etc/fstab`
//!   naming `/dev/DEV` as `/` (diskmap(4) is not ported: the kernel's name for the disk,
//!   `sd1a` on amd64 q35, where the boot image is on the AHCI port after the virtio disk).
//!   boot(8) passes the label's DUID (`BOOTARG_BOOTDUID`), by which the kernel finds its root.

use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::Result;
use crate::boot::{self, Arch};

/// The bytes of a sector.
const SECTOR: u64 = 512;
/// The OpenBSD partition's first sector (and partition `a`'s), as `hwopts.rs` has it.
const OPENBSD_START: u64 = 64;
/// The disk is a whole number of MiB.
const ALIGN: u64 = 2048;
/// The EFI system partition's size: 8 MiB.
const ESP_SECTORS: u64 = 16384;
/// The DUID of the efiboot disk's label (`EFIBOOT0` in ASCII).
const DUID: [u8; 8] = *b"EFIBOOT0";

/// `boot.conf` on the disk: efiboot runs it before its prompt. It sets no `boot` command,
/// so the prompt comes, and turns the prompt's timeout off so that the smoke types at it.
const BOOT_CONF: &str = "set timeout 0\necho efiboot: boot.conf read\n";

/// The `.EFI` file name of `arch`'s efiboot.
fn efi_name(arch: Arch) -> &'static str {
    match arch {
        Arch::Amd64 => "BOOTX64.EFI",
        Arch::Arm64 => "BOOTAA64.EFI",
    }
}

/// The toolchain's llvm-objcopy (`llvm-tools`), else `$EMIBSD_LLVM_BIN`'s.
fn llvm_objcopy() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("EMIBSD_LLVM_BIN") {
        let p = PathBuf::from(dir).join("llvm-objcopy");
        if p.is_file() {
            return Ok(p);
        }
    }
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let out = |args: &[&str]| -> Option<String> {
        let o = Command::new(&rustc).args(args).output().ok()?;
        o.status
            .success()
            .then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    if let (Some(sysroot), Some(vv)) = (out(&["--print", "sysroot"]), out(&["-vV"]))
        && let Some(host) = vv.lines().find_map(|l| l.strip_prefix("host: "))
    {
        let p = Path::new(&sysroot)
            .join("lib/rustlib")
            .join(host)
            .join("bin/llvm-objcopy");
        if p.is_file() {
            return Ok(p);
        }
    }
    Err(
        "llvm-objcopy not found: install the toolchain's llvm-tools component \
         (rust-toolchain.toml lists it; docs/SETUP.md) or set EMIBSD_LLVM_BIN"
            .into(),
    )
}

/// `cargo xtask efiboot`: the PE image of the efiboot ELF (module docs).
pub(crate) fn efiboot(root: &Path, arch: Arch, elf: &Path, out: Option<&str>) -> Result<()> {
    let out = match out {
        Some(o) => root.join(o),
        None => root.join(format!("target/efiboot/{}/{}", arch.name(), efi_name(arch))),
    };
    if let Some(dir) = out.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let objcopy = llvm_objcopy()?;
    let st = Command::new(&objcopy)
        .args(["-O", "binary"])
        .arg(elf)
        .arg(&out)
        .status()
        .map_err(|e| format!("{}: {e}", objcopy.display()))?;
    if !st.success() {
        return Err(format!("{} -O binary {} failed", objcopy.display(), elf.display()).into());
    }
    let image = fs::read(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    check_pe(&image).map_err(|e| format!("{}: {e}", out.display()))?;
    println!("efiboot: {} ({} KiB)", out.display(), image.len() / 1024);
    Ok(())
}

/// The PE32+ image's sanity: the DOS and PE signatures, a PE32+ optional header, and a
/// `SizeOfImage` that the file holds whole.
fn check_pe(image: &[u8]) -> std::result::Result<(), String> {
    let le32 = |o: usize| {
        image
            .get(o..o + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    if image.get(..2) != Some(b"MZ") {
        return Err("no MZ signature (is .peheader first?)".into());
    }
    let pe = le32(0x3c).ok_or("truncated")? as usize;
    if image.get(pe..pe + 4) != Some(b"PE\0\0") {
        return Err("no PE signature".into());
    }
    let opt = pe + 24;
    if image.get(opt..opt + 2) != Some(&[0x0b, 0x02]) {
        return Err("not PE32+".into());
    }
    let size_of_image = le32(opt + 56).ok_or("truncated")? as usize;
    if size_of_image != image.len() {
        return Err(format!(
            "SizeOfImage {size_of_image} != file size {}",
            image.len()
        ));
    }
    Ok(())
}

/// The bytes of the regular files under `dir` (symbolic links not followed).
fn tree_bytes(dir: &Path) -> Result<u64> {
    let mut total = 0;
    for entry in fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let entry = entry?;
        let meta = fs::symlink_metadata(entry.path())?;
        if meta.is_dir() {
            total += tree_bytes(&entry.path())?;
        } else {
            total += meta.len();
        }
    }
    Ok(total)
}

/// The ramdisk's fstab with its root line (`/dev/rd0a /`) naming `/dev/<dev>` instead.
fn root_fstab(fstab: &str, dev: &str) -> Result<String> {
    if dev.is_empty() || !dev.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(format!("--root-dev {dev}: expected a device name such as sd1a").into());
    }
    let mut seen = false;
    let lines: Vec<String> = fstab
        .lines()
        .map(|l| match l.strip_prefix("/dev/rd0a / ") {
            Some(rest) => {
                seen = true;
                format!("/dev/{dev} / {rest}")
            }
            None => l.to_string(),
        })
        .collect();
    if !seen {
        return Err("the ramdisk's fstab has no `/dev/rd0a /` root line".into());
    }
    Ok(lines.join("\n") + "\n")
}

/// Writes the FFS of partition `a`: `/bsd`, `/etc/boot.conf`, `/etc/random.seed`, and with
/// `root_dev` the whole ramdisk tree with an fstab whose root is `/dev/<root_dev>`.
fn make_ffs(
    root: &Path,
    arch: Arch,
    kernel: &Path,
    work: &Path,
    root_dev: Option<&str>,
) -> Result<Vec<u8>> {
    let makefs = root.join(format!("target/userland/{}/host/bin/makefs", arch.name()));
    if !makefs.is_file() {
        return Err(format!(
            "{}: no makefs; run `just userland` first (it builds OpenBSD's makefs for the host)",
            makefs.display()
        )
        .into());
    }
    let staging = work.join("efiboot-root");
    let _ = fs::remove_dir_all(&staging);
    let mut owners_text = String::new();
    if let Some(dev) = root_dev {
        let tree = root.join(format!("target/userland/{}/ramdisk-root", arch.name()));
        if !tree.join("etc/fstab").is_file() {
            return Err(format!(
                "{}: no staged root; run `just userland` first",
                tree.display()
            )
            .into());
        }
        // cp -pR copies symbolic links as links and keeps the modes.
        let status = Command::new("cp")
            .arg("-pR")
            .arg(&tree)
            .arg(&staging)
            .status()
            .map_err(|e| format!("cp: {e}"))?;
        if !status.success() {
            return Err(format!("cp -pR {} failed", tree.display()).into());
        }
        let fstab = staging.join("etc/fstab");
        let text = fs::read_to_string(&fstab).map_err(|e| format!("{}: {e}", fstab.display()))?;
        fs::write(&fstab, root_fstab(&text, dev)?)?;
        let table = root.join(format!("target/userland/{}/host/owners.txt", arch.name()));
        owners_text =
            fs::read_to_string(&table).map_err(|e| format!("{}: {e}", table.display()))?;
    }
    fs::create_dir_all(staging.join("etc")).map_err(|e| format!("{}: {e}", staging.display()))?;
    fs::copy(kernel, staging.join("bsd")).map_err(|e| format!("{}: {e}", kernel.display()))?;
    fs::write(staging.join("etc/boot.conf"), BOOT_CONF)?;
    // A seed boot(8) can read (and marks used, sticky bit, as on a real install).
    let seed: Vec<u8> = (0..256u32)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8)
        .collect();
    fs::write(staging.join("etc/random.seed"), seed)?;
    let owners = work.join("efiboot-owners.txt");
    owners_text.push_str(
        "0755 0 0 /etc\n0555 0 0 /bsd\n0644 0 0 /etc/boot.conf\n0600 0 0 /etc/random.seed\n",
    );
    fs::write(&owners, owners_text)?;
    // makefs's own estimate leaves no room for the indirect blocks of a big kernel: a tenth
    // more and 4 MiB, in whole MiB; a root gets room to write besides (twice its size).
    const MIB: u64 = 1 << 20;
    let kernel_len = fs::metadata(kernel).map(|m| m.len()).unwrap_or(0);
    let tree_len = if root_dev.is_some() {
        tree_bytes(&staging)? - kernel_len
    } else {
        0
    };
    let size = (kernel_len + kernel_len / 10 + 2 * tree_len + 4 * MIB).div_ceil(MIB) * MIB;
    let image = work.join("efiboot-root.ffs");
    let _ = fs::remove_file(&image);
    let out = Command::new(&makefs)
        .args([
            "-t",
            "ffs",
            "-o",
            "version=1,minfree=0,density=4096",
            "-T",
            "1790985600",
        ])
        .args(["-s", &size.to_string()])
        .env("EMIBSD_OWNERS", &owners)
        .env("EMIBSD_STAGING", &staging)
        .arg(&image)
        .arg(&staging)
        .output()
        .map_err(|e| format!("{}: {e}", makefs.display()))?;
    if !out.status.success() {
        return Err(format!(
            "makefs failed: {}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
        .into());
    }
    let fs_bytes = fs::read(&image).map_err(|e| format!("{}: {e}", image.display()))?;
    let _ = fs::remove_dir_all(&staging);
    Ok(fs_bytes)
}

/// The FAT system partition holding `EFI/BOOT/<efi_name>`.
fn make_esp(arch: Arch, efi: &[u8]) -> Result<Vec<u8>> {
    let mut part = Cursor::new(vec![0u8; (ESP_SECTORS * SECTOR) as usize]);
    fatfs::format_volume(
        &mut part,
        fatfs::FormatVolumeOptions::new().volume_label(*b"EFIBOOT    "),
    )?;
    {
        let fs = fatfs::FileSystem::new(&mut part, fatfs::FsOptions::new())?;
        {
            let dir = fs.root_dir().create_dir("EFI")?.create_dir("BOOT")?;
            dir.create_file(efi_name(arch))?.write_all(efi)?;
        }
        fs.unmount()?;
    }
    Ok(part.into_inner())
}

/// `cargo xtask efiboot-disk`: the boot image of `smoke-efiboot` (module docs).
pub(crate) fn efiboot_disk(
    root: &Path,
    arch: Arch,
    efi: &Path,
    kernel: &Path,
    root_dev: Option<&str>,
) -> Result<()> {
    let efi_bytes = fs::read(efi).map_err(|e| format!("{}: {e}", efi.display()))?;
    let path = boot::image_path(root, arch, None);
    let work = path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| root.join("target"));
    fs::create_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;

    let ffs = make_ffs(root, arch, kernel, &work, root_dev)?;
    let fs_sectors = (ffs.len() as u64).div_ceil(SECTOR);
    let esp_start = (OPENBSD_START + fs_sectors).div_ceil(ALIGN) * ALIGN;
    let total = esp_start + ESP_SECTORS;
    let mut disk = vec![0u8; (total * SECTOR) as usize];

    // The MBR: hwopts's one OpenBSD partition, and the EFI system partition beside it.
    let mut mbr = crate::hwopts::mbr(OPENBSD_START, esp_start - OPENBSD_START);
    mbr[446] = 0; // only the ESP is "active" for the firmware's sake
    let e = &mut mbr[462..478];
    e[1..4].copy_from_slice(&[0xfe, 0xff, 0xff]);
    e[4] = 0xef; // DOSPTYP_EFISYS
    e[5..8].copy_from_slice(&[0xfe, 0xff, 0xff]);
    e[8..12].copy_from_slice(&(esp_start as u32).to_le_bytes());
    e[12..16].copy_from_slice(&(ESP_SECTORS as u32).to_le_bytes());
    disk[..512].copy_from_slice(&mbr);

    let fs_at = (OPENBSD_START * SECTOR) as usize;
    disk[fs_at..fs_at + ffs.len()].copy_from_slice(&ffs);
    // The label goes in the file system's boot area, which ffs leaves free (`LABELSECTOR`).
    let label_at = ((OPENBSD_START + 1) * SECTOR) as usize;
    disk[label_at..label_at + 512]
        .copy_from_slice(&crate::hwopts::disklabel(total, fs_sectors, DUID));

    let esp = make_esp(arch, &efi_bytes)?;
    let esp_at = (esp_start * SECTOR) as usize;
    disk[esp_at..esp_at + esp.len()].copy_from_slice(&esp);

    fs::write(&path, &disk).map_err(|e| format!("{}: {e}", path.display()))?;
    println!(
        "efiboot-disk: {} ({total} sectors; OpenBSD partition at {OPENBSD_START}: a = ffs of \
         {fs_sectors} sectors with /bsd ({} KiB), /etc/boot.conf, /etc/random.seed{}; ESP at \
         {esp_start}: EFI/BOOT/{} ({} KiB))",
        path.display(),
        fs::metadata(kernel).map(|m| m.len() / 1024).unwrap_or(0),
        root_dev.map_or(String::new(), |d| format!(
            " and the ramdisk's root, fstab root /dev/{d}"
        )),
        efi_name(arch),
        efi_bytes.len() / 1024
    );
    Ok(())
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_fstab_renames_the_root_line_only() {
        let fstab = "/dev/rd0a / ffs rw 1 1\n/dev/sd0a /mnt ffs rw,userquota,noauto 1 2\n";
        assert_eq!(
            root_fstab(fstab, "sd1a").unwrap(),
            "/dev/sd1a / ffs rw 1 1\n/dev/sd0a /mnt ffs rw,userquota,noauto 1 2\n"
        );
        assert!(root_fstab("/dev/sd0a /mnt ffs rw 1 2\n", "sd1a").is_err());
        assert!(root_fstab(fstab, "sd1a /x").is_err());
    }

    #[test]
    fn pe_check_wants_signatures_and_a_whole_image() {
        let mut img = vec![0u8; 0x200];
        img[..2].copy_from_slice(b"MZ");
        img[0x3c] = 0x40;
        img[0x40..0x44].copy_from_slice(b"PE\0\0");
        img[0x58..0x5a].copy_from_slice(&[0x0b, 0x02]);
        img[0x58 + 56..0x58 + 60].copy_from_slice(&0x200u32.to_le_bytes());
        assert_eq!(check_pe(&img), Ok(()));
        img[0x58 + 56..0x58 + 60].copy_from_slice(&0x100u32.to_le_bytes());
        assert!(check_pe(&img).is_err());
        img[0] = b'X';
        assert!(check_pe(&img).is_err());
    }
}
/* </TESTS> */
