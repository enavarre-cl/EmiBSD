//! Boot images and QEMU: `xtask image`, `xtask qemu`, `xtask smoke`.
//!
//! The image is a raw disk with one MBR partition holding a FAT file system: Limine's UEFI
//! binary at `EFI/BOOT/BOOT{X64,AA64}.EFI`, `limine.conf` at the root and the kernel at `/bsd`.
//! QEMU boots it with EDK2 firmware; the kernel's serial console is on stdio and, under feature
//! `qemu`, the kernel ends the emulator with a status `smoke` checks.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::Result;

/// A kernel architecture, as `--arch` names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arch {
    /// x86-64, QEMU `q35`.
    Amd64,
    /// AArch64, QEMU `virt`.
    Arm64,
}

impl Arch {
    pub fn parse(name: &str) -> Result<Self> {
        match name {
            "amd64" => Ok(Arch::Amd64),
            "arm64" => Ok(Arch::Arm64),
            other => Err(format!("unknown arch `{other}`; expected amd64 or arm64").into()),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Arch::Amd64 => "amd64",
            Arch::Arm64 => "arm64",
        }
    }

    fn qemu(self) -> &'static str {
        match self {
            Arch::Amd64 => "qemu-system-x86_64",
            Arch::Arm64 => "qemu-system-aarch64",
        }
    }

    fn edk2_code(self) -> &'static str {
        match self {
            Arch::Amd64 => "edk2-x86_64-code.fd",
            Arch::Arm64 => "edk2-aarch64-code.fd",
        }
    }

    fn edk2_vars(self) -> &'static str {
        match self {
            Arch::Amd64 => "edk2-i386-vars.fd",
            Arch::Arm64 => "edk2-arm-vars.fd",
        }
    }

    fn limine_efi(self) -> &'static str {
        match self {
            Arch::Amd64 => "BOOTX64.EFI",
            Arch::Arm64 => "BOOTAA64.EFI",
        }
    }
}

/// Exit status QEMU reports when the kernel leaves with `ExitStatus::Success`
/// (`sys/machine/api.rs`).
const QEMU_SUCCESS_STATUS: i32 = 33;
/// Longest a smoke boot may take, EDK2 and Limine included, under TCG.
const SMOKE_TIMEOUT: Duration = Duration::from_secs(180);

const SECTOR: u64 = 512;
/// 64 MiB: room for FAT32 if the formatter picks it, and for modules later.
const IMAGE_SECTORS: u64 = 64 * 1024 * 1024 / SECTOR;
/// First partition sector: 1 MiB, the conventional alignment.
const PART_START: u64 = 2048;

fn image_path(root: &Path, arch: Arch) -> PathBuf {
    root.join("target")
        .join(format!("openbsd-rs-{}.img", arch.name()))
}

/// `brew --prefix <formula>`, if Homebrew is installed and knows the formula.
fn brew_prefix(formula: &str) -> Option<PathBuf> {
    let out = Command::new("brew")
        .args(["--prefix", formula])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout);
    let s = s.trim();
    if s.is_empty() {
        None
    } else {
        Some(PathBuf::from(s))
    }
}

/// Finds `file` in `$<env>`, under `brew --prefix <formula>/<rel>`, or in `extra` directories.
fn locate(env: &str, formula: &str, rel: &str, extra: &[&str], file: &str) -> Result<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(dir) = std::env::var_os(env) {
        candidates.push(PathBuf::from(dir).join(file));
    }
    if let Some(prefix) = brew_prefix(formula) {
        candidates.push(prefix.join(rel).join(file));
    }
    for dir in extra {
        candidates.push(PathBuf::from(dir).join(file));
    }
    candidates
        .iter()
        .find(|p| p.is_file())
        .cloned()
        .ok_or_else(|| {
            let looked: Vec<String> = candidates.iter().map(|p| p.display().to_string()).collect();
            format!(
                "{file} not found (looked at {}); install `{formula}` as in docs/SETUP.md or set \
                 ${env}",
                looked.join(", ")
            )
            .into()
        })
}

fn limine_file(file: &str) -> Result<PathBuf> {
    locate(
        "OPENBSD_RS_LIMINE_DIR",
        "limine",
        "share/limine",
        &["/usr/share/limine", "/usr/local/share/limine"],
        file,
    )
}

fn edk2_file(file: &str) -> Result<PathBuf> {
    locate(
        "OPENBSD_RS_EDK2_DIR",
        "qemu",
        "share/qemu",
        &["/usr/share/qemu", "/usr/local/share/qemu"],
        file,
    )
}

fn read(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).map_err(|e| format!("{}: {e}", path.display()).into())
}

/// Builds the boot image for `arch` from `kernel`; returns its path.
pub fn image(root: &Path, arch: Arch, kernel: &Path) -> Result<PathBuf> {
    let kernel_bytes = read(kernel)?;
    let efi_path = limine_file(arch.limine_efi())?;
    let efi = read(&efi_path)?;
    let conf = read(&root.join("sys/stand/limine.conf"))?;

    let path = image_path(root, arch);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    file.set_len(IMAGE_SECTORS * SECTOR)?;
    let part_sectors = IMAGE_SECTORS - PART_START;
    file.seek(SeekFrom::Start(0))?;
    file.write_all(&mbr(PART_START as u32, part_sectors as u32))?;

    let mut part = Partition::new(file, PART_START * SECTOR, part_sectors * SECTOR);
    fatfs::format_volume(
        &mut part,
        fatfs::FormatVolumeOptions::new().volume_label(*b"OPENBSD-RS "),
    )?;
    let fs = fatfs::FileSystem::new(part, fatfs::FsOptions::new())?;
    {
        let root_dir = fs.root_dir();
        let boot_dir = root_dir.create_dir("EFI")?.create_dir("BOOT")?;
        boot_dir.create_file(arch.limine_efi())?.write_all(&efi)?;
        root_dir.create_file("limine.conf")?.write_all(&conf)?;
        root_dir.create_file("bsd")?.write_all(&kernel_bytes)?;
    }
    fs.unmount()?;

    println!(
        "xtask: {} ({} KiB kernel, {} from {})",
        path.display(),
        kernel_bytes.len() / 1024,
        arch.limine_efi(),
        efi_path.display()
    );
    Ok(path)
}

/// A master boot record with one bootable partition of type 0xEF (EFI system partition).
/// Firmware boots by LBA; the CHS fields are the conventional placeholders.
fn mbr(part_start: u32, part_sectors: u32) -> [u8; 512] {
    let mut sector = [0u8; 512];
    let entry = &mut sector[446..462];
    entry[0] = 0x80;
    entry[1..4].copy_from_slice(&[0x00, 0x02, 0x00]);
    entry[4] = 0xef;
    entry[5..8].copy_from_slice(&[0xfe, 0xff, 0xff]);
    entry[8..12].copy_from_slice(&part_start.to_le_bytes());
    entry[12..16].copy_from_slice(&part_sectors.to_le_bytes());
    sector[510] = 0x55;
    sector[511] = 0xaa;
    sector
}

/// A byte range of a file presented as a whole device, for the FAT formatter and driver.
struct Partition {
    file: File,
    start: u64,
    len: u64,
    pos: u64,
}

impl Partition {
    fn new(file: File, start: u64, len: u64) -> Self {
        Self {
            file,
            start,
            len,
            pos: 0,
        }
    }

    fn remaining(&self) -> u64 {
        self.len.saturating_sub(self.pos)
    }
}

impl Read for Partition {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = (buf.len() as u64).min(self.remaining()) as usize;
        if n == 0 {
            return Ok(0);
        }
        self.file.seek(SeekFrom::Start(self.start + self.pos))?;
        let got = self.file.read(&mut buf[..n])?;
        self.pos += got as u64;
        Ok(got)
    }
}

impl Write for Partition {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = (buf.len() as u64).min(self.remaining()) as usize;
        if n == 0 && !buf.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "write past the end of the partition",
            ));
        }
        self.file.seek(SeekFrom::Start(self.start + self.pos))?;
        let put = self.file.write(&buf[..n])?;
        self.pos += put as u64;
        Ok(put)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

impl Seek for Partition {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let target = match pos {
            SeekFrom::Start(n) => i128::from(n),
            SeekFrom::End(off) => i128::from(self.len) + i128::from(off),
            SeekFrom::Current(off) => i128::from(self.pos) + i128::from(off),
        };
        if target < 0 || target > i128::from(self.len) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "seek outside the partition",
            ));
        }
        self.pos = target as u64;
        Ok(self.pos)
    }
}

/// The QEMU command line for `arch` booting `image`, serial on `serial` (`stdio` or
/// `mon:stdio`), display off, firmware from EDK2. A fresh copy of the EDK2 variable store is
/// made per run so boots do not depend on what the firmware remembered last time.
fn qemu_command(root: &Path, arch: Arch, image: &Path, serial: &str) -> Result<Command> {
    let code = edk2_file(arch.edk2_code())?;
    let vars_src = edk2_file(arch.edk2_vars())?;
    let vars = root
        .join("target")
        .join(format!("edk2-{}-vars.fd", arch.name()));
    fs::copy(&vars_src, &vars).map_err(|e| format!("{}: {e}", vars.display()))?;

    let mut cmd = Command::new(arch.qemu());
    cmd.args([
        "-m",
        "512M",
        "-display",
        "none",
        "-monitor",
        "none",
        "-no-reboot",
    ]);
    cmd.args(["-serial", serial]);
    cmd.arg("-drive").arg(format!(
        "if=pflash,format=raw,readonly=on,file={}",
        code.display()
    ));
    cmd.arg("-drive")
        .arg(format!("if=pflash,format=raw,file={}", vars.display()));
    match arch {
        Arch::Amd64 => {
            cmd.args(["-M", "q35", "-cpu", "qemu64"]);
            cmd.arg("-drive")
                .arg(format!("format=raw,file={}", image.display()));
            cmd.args(["-device", "isa-debug-exit,iobase=0xf4,iosize=0x04"]);
        }
        Arch::Arm64 => {
            cmd.args(["-M", "virt", "-cpu", "cortex-a72"]);
            cmd.arg("-drive").arg(format!(
                "if=none,format=raw,file={},id=hd0",
                image.display()
            ));
            cmd.args(["-device", "virtio-blk-device,drive=hd0"]);
            cmd.args(["-semihosting-config", "enable=on,target=native"]);
        }
    }
    Ok(cmd)
}

fn command_line(cmd: &Command) -> String {
    let mut s = cmd.get_program().to_string_lossy().into_owned();
    for a in cmd.get_args() {
        s.push(' ');
        s.push_str(&a.to_string_lossy());
    }
    s
}

fn spawn_error(arch: Arch, e: &io::Error) -> String {
    format!("{}: {e}; install `qemu` as in docs/SETUP.md", arch.qemu())
}

/// Boots `arch` interactively: serial and the QEMU monitor on stdio (`Ctrl-A X` quits). With
/// `kernel`, the image is rebuilt first.
pub fn qemu(root: &Path, arch: Arch, kernel: Option<&Path>) -> Result<()> {
    let image = match kernel {
        Some(k) => image(root, arch, k)?,
        None => {
            let p = image_path(root, arch);
            if !p.is_file() {
                return Err(format!(
                    "{} does not exist; run `just image-{}`",
                    p.display(),
                    arch.name()
                )
                .into());
            }
            p
        }
    };
    let mut cmd = qemu_command(root, arch, &image, "mon:stdio")?;
    println!("xtask: {}", command_line(&cmd));
    let status = cmd.status().map_err(|e| spawn_error(arch, &e))?;
    match status.code() {
        Some(QEMU_SUCCESS_STATUS) => {
            println!("xtask: kernel exited with success");
            Ok(())
        }
        Some(0) => Ok(()),
        Some(code) => Err(format!("qemu exited with status {code}").into()),
        None => Err("qemu was killed by a signal".into()),
    }
}

/// Boots `arch` headless, captures the serial transcript, and passes when `expect` appears and
/// QEMU exits with the kernel's success status before the timeout.
pub fn smoke(root: &Path, arch: Arch, expect: &str) -> Result<()> {
    let image = image_path(root, arch);
    if !image.is_file() {
        return Err(format!(
            "{} does not exist; run `just image-{}`",
            image.display(),
            arch.name()
        )
        .into());
    }
    let mut cmd = qemu_command(root, arch, &image, "stdio")?;
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    println!("xtask: {}", command_line(&cmd));

    let started = Instant::now();
    let mut child = cmd.spawn().map_err(|e| spawn_error(arch, &e))?;
    let stdout = child.stdout.take().ok_or("qemu stdout is not a pipe")?;
    let stderr = child.stderr.take().ok_or("qemu stderr is not a pipe")?;
    let out_reader = thread::spawn(move || slurp(stdout));
    let err_reader = thread::spawn(move || slurp(stderr));

    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break Some(status);
        }
        if started.elapsed() > SMOKE_TIMEOUT {
            timed_out = true;
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        thread::sleep(Duration::from_millis(100));
    };
    let serial = String::from_utf8_lossy(&out_reader.join().unwrap_or_default()).into_owned();
    let diagnostics = String::from_utf8_lossy(&err_reader.join().unwrap_or_default()).into_owned();

    let found = serial.contains(expect);
    let code = status.and_then(|s| s.code());
    let ok = found && code == Some(QEMU_SUCCESS_STATUS);
    let elapsed = started.elapsed().as_secs_f32();
    if ok {
        // The kernel's own lines, for the record; firmware and bootloader chatter is left out.
        for line in serial.lines() {
            if let Some(at) = line.find("bsd: ") {
                println!("  {}", line[at..].trim_end());
            }
        }
        println!(
            "smoke {}: ok in {elapsed:.1}s (`{expect}` seen, status {QEMU_SUCCESS_STATUS})",
            arch.name()
        );
        return Ok(());
    }
    println!("----- serial transcript ({}) -----", arch.name());
    print!("{serial}");
    if !serial.ends_with('\n') {
        println!();
    }
    if !diagnostics.trim().is_empty() {
        println!("----- qemu stderr -----");
        print!("{diagnostics}");
    }
    println!("-----");
    let why = if timed_out {
        format!("timed out after {elapsed:.0}s")
    } else {
        format!("qemu exited with status {code:?}")
    };
    Err(format!(
        "smoke {}: {} and `{expect}` {}",
        arch.name(),
        why,
        if found { "was seen" } else { "was NOT seen" }
    )
    .into())
}

fn slurp(mut r: impl Read) -> Vec<u8> {
    let mut v = Vec::new();
    let _ = r.read_to_end(&mut v);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mbr_layout() {
        let s = mbr(2048, 1000);
        assert_eq!(&s[510..], &[0x55, 0xaa]);
        assert_eq!(s[446], 0x80);
        assert_eq!(s[450], 0xef);
        assert_eq!(u32::from_le_bytes([s[454], s[455], s[456], s[457]]), 2048);
        assert_eq!(u32::from_le_bytes([s[458], s[459], s[460], s[461]]), 1000);
        assert!(s[..446].iter().all(|&b| b == 0));
    }
}
