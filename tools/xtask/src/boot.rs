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
//! Boot images and QEMU: `xtask image`, `xtask qemu`, `xtask smoke`.
//!
//! The image is a raw disk with one MBR partition holding a FAT file system: Limine's UEFI
//! binary at `EFI/BOOT/BOOT{X64,AA64}.EFI`, `limine.conf` at the root, the kernel at `/bsd`
//! and the boot modules: `/init` (the freestanding init of `init/`) and `/ramdisk.ffs` (the
//! root file system image `just userland` makes, rd(4)'s image), each when it exists.
//! QEMU boots it with EDK2 firmware; the kernel's serial console is on stdio and, under feature
//! `qemu`, the kernel ends the emulator with a status `smoke` checks.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
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

    pub fn name(self) -> &'static str {
        match self {
            Arch::Amd64 => "amd64",
            Arch::Arm64 => "arm64",
        }
    }

    /// The Rust target triple the kernel is built for.
    pub fn target(self) -> &'static str {
        match self {
            Arch::Amd64 => "x86_64-unknown-none",
            Arch::Arm64 => "aarch64-unknown-none-softfloat",
        }
    }

    pub(crate) fn qemu(self) -> &'static str {
        match self {
            Arch::Amd64 => "qemu-system-x86_64",
            Arch::Arm64 => "qemu-system-aarch64",
        }
    }

    pub(crate) fn edk2_code(self) -> &'static str {
        match self {
            Arch::Amd64 => "edk2-x86_64-code.fd",
            Arch::Arm64 => "edk2-aarch64-code.fd",
        }
    }

    pub(crate) fn edk2_vars(self) -> &'static str {
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
/// (`sys/machine/cpu.rs`).
pub const QEMU_SUCCESS_STATUS: i32 = 33;
/// Longest a smoke boot may take, EDK2 and Limine included, under TCG.
pub(crate) const SMOKE_TIMEOUT: Duration = Duration::from_secs(180);

/// The environment variable that multiplies every smoke and smoke2 time limit (see
/// [`time_limit`]).
pub(crate) const TIMEOUT_SCALE_ENV: &str = "EMIBSD_TIMEOUT_SCALE";

/// A smoke or smoke2 time limit (the default [`SMOKE_TIMEOUT`] or a recipe's `--timeout`)
/// multiplied by `$EMIBSD_TIMEOUT_SCALE`, a whole number from 1 (the default) to 10.
/// `smoke-all` sets it when recipes run side by side: their VMs share the host's cores, so a
/// boot that takes 60 s alone may take two or three times that, and the limits exist to catch
/// hangs, not slowness. No expectation changes with it.
pub(crate) fn time_limit(base: Duration) -> Duration {
    base * timeout_scale(std::env::var(TIMEOUT_SCALE_ENV).ok().as_deref())
}

/// How often a waiting boot says it is still running ([`Heartbeat`]).
pub(crate) const HEARTBEAT: Duration = Duration::from_secs(60);

/// One line every [`HEARTBEAT`] while a boot waits for QEMU, which otherwise prints nothing
/// until it ends (a boot may take its limit times `$EMIBSD_TIMEOUT_SCALE`, minutes). So a
/// run's log grows for as long as xtask itself is alive, and `smoke-all`'s watchdog can take a
/// log that stays still for ten minutes as a hung recipe (`smokeall.rs`).
pub(crate) struct Heartbeat {
    started: Instant,
    next: Duration,
}

impl Heartbeat {
    pub(crate) fn new() -> Heartbeat {
        Heartbeat {
            started: Instant::now(),
            next: HEARTBEAT,
        }
    }

    /// Prints `xtask: NAME: still running after Ns, B serial bytes` when one is due.
    pub(crate) fn tick(&mut self, name: &str, serial_bytes: usize) {
        if let Some(line) = self.due(self.started.elapsed(), name, serial_bytes) {
            println!("{line}");
        }
    }

    /// The line due at `elapsed`, if any; the next one is due a whole [`HEARTBEAT`] later.
    fn due(&mut self, elapsed: Duration, name: &str, serial_bytes: usize) -> Option<String> {
        if elapsed < self.next {
            return None;
        }
        while self.next <= elapsed {
            self.next += HEARTBEAT;
        }
        Some(format!(
            "xtask: {name}: still running after {}s, {serial_bytes} serial bytes",
            elapsed.as_secs()
        ))
    }
}

/// The factor of [`time_limit`], from the variable's value; anything not in 1..=10 is 1.
fn timeout_scale(value: Option<&str>) -> u32 {
    value
        .and_then(|v| v.trim().parse::<u32>().ok())
        .filter(|n| (1..=10).contains(n))
        .unwrap_or(1)
}

const SECTOR: u64 = 512;
/// 192 MiB: room for FAT32 if the formatter picks it, and for the modules: the debug kernel
/// and the ramdisk (23 MiB since M9+ added LibreSSL, ftp and nc) outgrew 64 MiB, and the
/// 65 MiB debug kernel with the 63 MiB ramdisk of M11e (tcpbench) outgrew 128 MiB.
const IMAGE_SECTORS: u64 = 192 * 1024 * 1024 / SECTOR;
/// First partition sector: 1 MiB, the conventional alignment.
const PART_START: u64 = 2048;

/// The ramdisk module's name on the ESP and in `limine.conf`; `sys/stand` looks it up by
/// this name and hands it to rd(4).
pub const RAMDISK_MODULE: &str = "ramdisk.ffs";

/// The environment variable naming a run's directory (see [`run_dir`]).
pub(crate) const RUN_DIR_ENV: &str = "EMIBSD_RUN_DIR";

/// Where a run keeps the files it writes for its VMs: boot images, EDK2 variable stores and
/// persistent disks. `target/` by default; `$EMIBSD_RUN_DIR` (relative to the workspace root,
/// or absolute) otherwise, which `smoke-all` sets to `target/smoke/<recipe>` so that recipes
/// running at the same time never share a writable file (QEMU's image locking refuses a
/// second writer, and a disk one recipe formats must not surprise another).
pub(crate) fn run_dir(root: &Path) -> PathBuf {
    match std::env::var_os(RUN_DIR_ENV) {
        Some(dir) if !dir.is_empty() => root.join(dir),
        _ => root.join("target"),
    }
}

/// The boot image's path. `tag` names one VM of a two-VM run (`smoke2`): `emibsd-<arch>-<tag>.img`,
/// so concurrent QEMUs never share a writable disk. Like every per-run file it lives in
/// [`run_dir`].
pub(crate) fn image_path(root: &Path, arch: Arch, tag: Option<&str>) -> PathBuf {
    run_dir(root).join(format!("emibsd-{}{}.img", arch.name(), dash(tag)))
}

/// Size of the persistent disk: 64 MiB, sparse.
pub(crate) const DISK_SIZE: u64 = 64 * 1024 * 1024;

/// The persistent disk's path: `disk-<arch>.img`, or `disk-<arch>-<tag>.img` for a `smoke2` VM.
/// Unlike the boot image it is never rebuilt, so what a guest wrote survives across boots.
pub(crate) fn disk_path(root: &Path, arch: Arch, tag: Option<&str>) -> PathBuf {
    run_dir(root).join(format!("disk-{}{}.img", arch.name(), dash(tag)))
}

/// The most persistent disks a VM can have (`--disks`, M10f's softraid smokes).
pub(crate) const MAX_DISKS: usize = 4;

/// The most processors a VM can have (`--smp`, M11): `q35` and `virt`'s GICv2 take eight.
pub(crate) const MAX_SMP: u32 = 8;

/// `--smp N` for every VM this run starts (set once by `main`, read by [`qemu_command`]).
static SMP: std::sync::OnceLock<u32> = std::sync::OnceLock::new();

/// Records `--smp N`; `None` leaves QEMU's default of one processor.
pub(crate) fn set_smp(n: Option<u32>) {
    if let Some(n) = n {
        let _ = SMP.set(n);
    }
}

/// The `--smp N` of this run, if one was given.
pub(crate) fn smp() -> Option<u32> {
    SMP.get().copied()
}

/// The path of persistent disk `k` (`sd<k>`): disk 0 is [`disk_path`], the others are
/// `disk-<arch>[-<tag>]-sd<k>.img`.
pub(crate) fn disk_path_n(root: &Path, arch: Arch, tag: Option<&str>, k: usize) -> PathBuf {
    if k == 0 {
        return disk_path(root, arch, tag);
    }
    run_dir(root).join(format!("disk-{}{}-sd{k}.img", arch.name(), dash(tag)))
}

/// Makes sure the persistent disk at `path` exists: created sparse and zero-filled when
/// missing (or when `fresh`, which deletes it first), otherwise left exactly as it is.
/// Returns whether the file was (re)created.
pub(crate) fn ensure_disk(path: &Path, fresh: bool) -> Result<bool> {
    let err = |e: io::Error| format!("{}: {e}", path.display());
    if fresh {
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(err(e).into()),
        }
    }
    if path.is_file() {
        return Ok(false);
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(err)?;
    }
    let f = File::create(path).map_err(err)?;
    f.set_len(DISK_SIZE).map_err(err)?;
    Ok(true)
}

/// `-<tag>` for a tagged VM, nothing for the single-VM commands.
fn dash(tag: Option<&str>) -> String {
    tag.map(|t| format!("-{t}")).unwrap_or_default()
}

/// `brew --prefix <formula>`, if Homebrew is installed and knows the formula.
pub(crate) fn brew_prefix(formula: &str) -> Option<PathBuf> {
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
        "EMIBSD_LIMINE_DIR",
        "limine",
        "share/limine",
        &["/usr/share/limine", "/usr/local/share/limine"],
        file,
    )
}

pub(crate) fn edk2_file(file: &str) -> Result<PathBuf> {
    locate(
        "EMIBSD_EDK2_DIR",
        "qemu",
        "share/qemu",
        &["/usr/share/qemu", "/usr/local/share/qemu"],
        file,
    )
}

fn read(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).map_err(|e| format!("{}: {e}", path.display()).into())
}

/// Builds the boot image for `arch` from `kernel`, with `cmdline` (if any) as the kernel
/// command line; returns its path.
pub fn image(
    root: &Path,
    arch: Arch,
    kernel: &Path,
    cmdline: Option<&str>,
    init: Option<&Path>,
    ramdisk: Option<&Path>,
) -> Result<PathBuf> {
    image_tagged(root, arch, None, kernel, cmdline, init, ramdisk)
}

/// [`image`] for the VM `tag` of a two-VM run: the image lands in its own file.
pub fn image_tagged(
    root: &Path,
    arch: Arch,
    tag: Option<&str>,
    kernel: &Path,
    cmdline: Option<&str>,
    init: Option<&Path>,
    ramdisk: Option<&Path>,
) -> Result<PathBuf> {
    let kernel_bytes = read(kernel)?;
    let init_bytes = init.map(read).transpose()?;
    if let Some(r) = ramdisk {
        crate::userland::check_ramdisk_devices(r)?;
    }
    let ramdisk_bytes = ramdisk.map(read).transpose()?;
    let efi_path = limine_file(arch.limine_efi())?;
    let efi = read(&efi_path)?;
    let mut conf = read(&root.join("sys/stand/limine.conf"))?;
    // The entry is the last block of the file; `cmdline:` and `module_path:` are more keys
    // of it.
    if !conf.ends_with(b"\n") {
        conf.push(b'\n');
    }
    if let Some(cmdline) = cmdline {
        conf.extend_from_slice(format!("    cmdline: {cmdline}\n").as_bytes());
    }
    if init_bytes.is_some() {
        conf.extend_from_slice(b"    module_path: boot():/init\n");
    }
    if ramdisk_bytes.is_some() {
        conf.extend_from_slice(format!("    module_path: boot():/{RAMDISK_MODULE}\n").as_bytes());
    }

    let path = image_path(root, arch, tag);
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
        fatfs::FormatVolumeOptions::new().volume_label(*b"EMIBSD     "),
    )?;
    let fs = fatfs::FileSystem::new(part, fatfs::FsOptions::new())?;
    {
        let root_dir = fs.root_dir();
        let boot_dir = root_dir.create_dir("EFI")?.create_dir("BOOT")?;
        boot_dir.create_file(arch.limine_efi())?.write_all(&efi)?;
        root_dir.create_file("limine.conf")?.write_all(&conf)?;
        root_dir.create_file("bsd")?.write_all(&kernel_bytes)?;
        if let Some(init_bytes) = &init_bytes {
            root_dir.create_file("init")?.write_all(init_bytes)?;
        }
        if let Some(ramdisk_bytes) = &ramdisk_bytes {
            root_dir
                .create_file(RAMDISK_MODULE)?
                .write_all(ramdisk_bytes)?;
        }
    }
    fs.unmount()?;

    println!(
        "xtask: {} ({} KiB kernel, {} from {}{}{})",
        path.display(),
        kernel_bytes.len() / 1024,
        arch.limine_efi(),
        efi_path.display(),
        cmdline
            .map(|c| format!(", cmdline `{c}`"))
            .unwrap_or_default(),
        match (ramdisk, &ramdisk_bytes) {
            (Some(p), Some(b)) => format!(
                ", {RAMDISK_MODULE} ({} KiB) from {}",
                b.len() / 1024,
                p.display()
            ),
            _ => format!(", no {RAMDISK_MODULE} (run `just userland`)"),
        }
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

/// The private link of one VM in a two-VM run (`smoke2`): a second virtio-net NIC (`vio1`)
/// on a QEMU `dgram` netdev, a pair of UDP sockets on localhost, one per VM, each sending to
/// the other's port. `dgram` is symmetric, so neither VM has to start first (`socket,listen=`
/// and `connect=` would), and it needs no multicast routing (`socket,mcast=` depends on the
/// host's loopback multicast). Both VMs have their own MACs, on `vio0` as well.
///
/// On arm64 the NICs sit on virtio-mmio slots, which QEMU `virt` hands out from the top
/// down while the kernel attaches them bottom up: the link NIC is therefore added before the
/// user-mode one there, and `smoke2`'s checks look at the MACs, not only at the names.
pub struct VmLink {
    /// `a` or `b`: suffix of the image and EDK2 variable store files.
    pub tag: &'static str,
    /// MAC of `vio0`, on QEMU's user-mode network.
    pub user_mac: String,
    /// MAC of `vio1`, on the private link.
    pub link_mac: String,
    /// UDP port this VM's end of the link listens on (localhost).
    pub local_port: u16,
    /// UDP port of the other VM's end.
    pub remote_port: u16,
}

impl VmLink {
    /// The `-netdev` argument of the link NIC (`id=n1`).
    pub fn netdev(&self) -> String {
        format!(
            "dgram,id=n1,local.type=inet,local.host=127.0.0.1,local.port={},\
             remote.type=inet,remote.host=127.0.0.1,remote.port={}",
            self.local_port, self.remote_port
        )
    }
}

/// The QEMU command line for `arch` booting `image`, serial on `serial` (`stdio` or
/// `mon:stdio`), display off, firmware from EDK2, and a virtio network card on QEMU's user
/// mode network (`vio(4)`: virtio-net-pci on amd64's PCI bus, virtio-net-device on one of
/// arm64 `virt`'s virtio-mmio slots). A fresh copy of the EDK2 variable store is made per run
/// so boots do not depend on what the firmware remembered last time.
///
/// Besides the boot image there is always one persistent virtio-blk disk, the raw 64 MiB
/// file `target/disk-<arch>.img` (`disk-<arch>-<tag>.img` for a `smoke2` VM). It is created
/// sparse and zero-filled when missing and reused as is otherwise, so what a guest wrote
/// survives across boots; `disk_fresh` (`--disk-fresh`) deletes and recreates it first. It is
/// the LAST device added on both archs: on amd64 the NICs keep their PCI slots (vio0 is dev
/// 2) and the disk is a later `virtio-blk-pci`; on arm64 `virt` the lowest virtio-mmio slot
/// in use goes to it, so the kernel, which attaches bottom up, finds it before the boot
/// disk (virtio31) and it becomes `sd0`.
///
/// `disks` (`--disks N`, 1 to [`MAX_DISKS`]) persistent disks are attached: `sd0` is the file
/// above and `sd<k>` is `disk-<arch>[-<tag>]-sd<k>.img` (see [`disk_path_n`]), all 64 MiB and
/// all recreated by `disk_fresh`. Files of higher disks than `disks` are left alone, not
/// attached (the "one disk missing" boot). The order keeps `sd0` the first one the kernel
/// finds: on amd64 PCI slots go up, so `sd0` is added first; on arm64 the last device added
/// is found first, so `sd<disks - 1>` is added first and `sd0` last.
///
/// `disk_set` (`--disk-set NAME`, `smoke` and `qemu`) names another set of persistent disks,
/// `disk-<arch>-NAME[-sd<k>].img`, as a `smoke2` VM's tag does: `smoke-softraid` keeps its
/// softraid metadata there, off the disk the other smokes boot with (a disk carrying RAID
/// partitions has softraid assemble volumes at every boot, with threads of their own).
/// A VM's persistent disks, as the `--disk-fresh`, `--disks` and `--disk-set` flags give them
/// (see [`qemu_command`]).
#[derive(Clone, Copy, Debug)]
pub struct Disks<'a> {
    /// `--disk-fresh`: delete and recreate the disks before booting.
    pub fresh: bool,
    /// `--disks N`: how many to attach (1 to [`MAX_DISKS`]).
    pub count: usize,
    /// `--disk-set NAME`: `disk-<arch>-NAME[-sd<k>].img` instead of the default set.
    pub set: Option<&'a str>,
}

pub(crate) fn qemu_command(
    root: &Path,
    arch: Arch,
    image: &Path,
    serial: &str,
    vm: Option<&VmLink>,
    disks: &Disks<'_>,
) -> Result<Command> {
    let tag = vm.map(|v| v.tag).or(disks.set);
    let disk_files: Vec<PathBuf> = (0..disks.count)
        .map(|k| disk_path_n(root, arch, tag, k))
        .collect();
    for disk in &disk_files {
        ensure_disk(disk, disks.fresh)?;
    }
    let code = edk2_file(arch.edk2_code())?;
    let vars_src = edk2_file(arch.edk2_vars())?;
    let vars = run_dir(root).join(format!(
        "edk2-{}{}-vars.fd",
        arch.name(),
        dash(vm.map(|v| v.tag))
    ));
    fs::copy(&vars_src, &vars).map_err(|e| format!("{}: {e}", vars.display()))?;

    let mut cmd = Command::new(arch.qemu());
    // M13 (hwopts.rs): `--screenshot-after` and `--sendkey-after` put the monitor on a socket.
    cmd.args(["-m", "512M", "-display", "none", "-monitor"]);
    cmd.arg(crate::hwopts::monitor_arg());
    // M13 (hwopts.rs): `--reboot` lets a guest reset restart the machine.
    if !crate::hwopts::reboot() {
        cmd.arg("-no-reboot");
    }
    cmd.args(["-serial", serial]);
    // EDK2 boots Limine at once: `bootindex=0` on the boot image's device (below) puts it
    // first in the firmware's BootOrder (QEMU's `bootorder` fw_cfg file, which OVMF and
    // ArmVirtQemu both honour), so the blank persistent disk is no longer tried first
    // (`BdsDxe: failed to load Boot0001 "UEFI Misc Device"`); and `menu=on,splash-time=0`
    // sets the boot manager's timeout to 0 s through `etc/boot-menu-wait`, where ArmVirtQemu
    // otherwise waits its platform default (about 5 s per arm64 boot; OVMF's default is 0).
    cmd.args(["-boot", "menu=on,splash-time=0"]);
    if let Some(n) = SMP.get() {
        cmd.args(["-smp", &n.to_string()]);
    }
    cmd.arg("-drive").arg(format!(
        "if=pflash,format=raw,readonly=on,file={}",
        code.display()
    ));
    cmd.arg("-drive")
        .arg(format!("if=pflash,format=raw,file={}", vars.display()));
    cmd.args(["-netdev", "user,id=n0"]);
    // vio0 must be the user-mode NIC. On amd64 that is the one added first (PCI slots go up);
    // arm64 is the other way round, see below.
    let nic0 = vm.map_or(String::new(), |v| format!(",mac={}", v.user_mac));
    match arch {
        Arch::Amd64 => {
            // M16e (hwopts.rs): `--machine pc` runs i440fx's `pc` instead of `q35`.
            cmd.args(["-M", crate::hwopts::amd64_machine(), "-cpu", "qemu64"]);
            // M16e (hwopts.rs): `--iommu`, before every PCI device.
            cmd.args(crate::hwopts::iommu_args());
            cmd.arg("-drive").arg(format!(
                "if=none,format=raw,file={},id=hd0",
                image.display()
            ));
            // M16e (hwopts.rs): on `--machine pc` the boot image goes on an AHCI controller
            // added after every other device (`add_devices`), not on the PIIX3 IDE channel.
            if !crate::hwopts::machine_pc() {
                cmd.args(["-device", "ide-hd,drive=hd0,bus=ide.0,bootindex=0"]);
            }
            cmd.args(["-device", "isa-debug-exit,iobase=0xf4,iosize=0x04"]);
            // M13 (hwopts.rs): `--nic` puts an em(4) NIC in vio0's place; M16b
            // (devices.rs): `--usb-net` leaves out the virtio NIC, the USB one is the user
            // network's.
            if !crate::devices::usb_net() {
                cmd.args(["-device", &crate::hwopts::user_nic(arch, &nic0)]);
            }
            if let Some(v) = vm {
                cmd.args(["-netdev", &v.netdev()]);
                cmd.args([
                    "-device",
                    &format!(
                        "virtio-net-pci,netdev=n1,mac={}{}",
                        v.link_mac,
                        crate::hwopts::virtio_pci_props()
                    ),
                ]);
            }
            // M13 (hwopts.rs): NVMe and the other PCI storage, before the virtio-blk disks.
            crate::hwopts::pci_storage(&mut cmd, arch)?;
            for (k, disk) in disk_files.iter().enumerate() {
                cmd.arg("-drive").arg(format!(
                    "if=none,format=raw,file={},id=sd{k}",
                    disk.display()
                ));
                cmd.args([
                    "-device",
                    &format!(
                        "virtio-blk-pci,drive=sd{k}{}",
                        crate::hwopts::virtio_pci_props()
                    ),
                ]);
            }
        }
        Arch::Arm64 => {
            // acpi=off: EDK2 then installs the device tree, which the arm64 kernel needs (M4).
            // `--acpi` (hwopts.rs): ACPI tables instead, and the disks on the PCI bus.
            let acpi = crate::hwopts::acpi();
            // M16f: `--gic 3` (or `EMIBSD_GIC=3`) and `--iommu smmuv3` (hwopts.rs).
            let machine = crate::hwopts::virt_machine();
            let blk = if acpi {
                "virtio-blk-pci"
            } else {
                "virtio-blk-device"
            };
            cmd.args(["-M", &machine, "-cpu", "cortex-a72"]);
            cmd.arg("-drive").arg(format!(
                "if=none,format=raw,file={},id=hd0",
                image.display()
            ));
            cmd.args(["-device", &format!("{blk},drive=hd0,bootindex=0")]);
            // QEMU `virt` hands virtio-mmio slots out from the top down and the kernel
            // finds them bottom up, so the device added LAST is vio0: the link NIC goes
            // before the user-mode one.
            if let Some(v) = vm {
                cmd.args(["-netdev", &v.netdev()]);
                cmd.args([
                    "-device",
                    &format!("virtio-net-device,netdev=n1,mac={}", v.link_mac),
                ]);
            }
            if !crate::devices::usb_net() {
                cmd.args(["-device", &crate::hwopts::user_nic(arch, &nic0)]);
            }
            crate::hwopts::pci_storage(&mut cmd, arch)?;
            for (k, disk) in disk_files.iter().enumerate().rev() {
                cmd.arg("-drive").arg(format!(
                    "if=none,format=raw,file={},id=sd{k}",
                    disk.display()
                ));
                cmd.args(["-device", &format!("{blk},drive=sd{k}")]);
            }
            cmd.args(["-semihosting-config", "enable=on,target=native"]);
        }
    }
    crate::hwopts::add_devices(&mut cmd, root, arch)?;
    // M16a (storage.rs): the storage controllers, after the devices above.
    crate::storage::add_devices(&mut cmd, arch)?;
    // M12: `--usb`, `--audio` (`devices.rs`), after every other device so the PCI slots
    // the older smokes expect do not move.
    cmd.args(crate::devices::qemu_args(image)?);
    Ok(cmd)
}

pub(crate) fn command_line(cmd: &Command) -> String {
    let mut s = cmd.get_program().to_string_lossy().into_owned();
    for a in cmd.get_args() {
        s.push(' ');
        s.push_str(&a.to_string_lossy());
    }
    s
}

pub(crate) fn spawn_error(arch: Arch, e: &io::Error) -> String {
    format!("{}: {e}; install `qemu` as in docs/SETUP.md", arch.qemu())
}

/// Boots `arch` interactively: serial and the QEMU monitor on stdio (`Ctrl-A X` quits). With
/// `kernel`, the image is rebuilt first.
/// The `init` the image carries unless `--init` says otherwise: the one `just build-init-*`
/// left in `target/`, if it exists.
pub fn default_init(root: &Path, arch: Arch) -> Option<PathBuf> {
    let p = root
        .join("target")
        .join(arch.target())
        .join("debug")
        .join("init");
    p.is_file().then_some(p)
}

/// The ramdisk the image carries unless `--ramdisk` says otherwise: the one `just userland`
/// left in `target/userland/<arch>/`, if it exists.
pub fn default_ramdisk(root: &Path, arch: Arch) -> Option<PathBuf> {
    let p = root
        .join("target")
        .join("userland")
        .join(arch.name())
        .join(RAMDISK_MODULE);
    p.is_file().then_some(p)
}

pub fn qemu(
    root: &Path,
    arch: Arch,
    kernel: Option<&Path>,
    init: Option<&Path>,
    ramdisk: Option<&Path>,
    disks: &Disks<'_>,
) -> Result<()> {
    let image = match kernel {
        Some(k) => image(root, arch, k, None, init, ramdisk)?,
        None => {
            let p = image_path(root, arch, None);
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
    // M16e (hwopts.rs): `--tpm`'s swtpm, stopped when this returns, after QEMU.
    let _swtpm = crate::hwopts::start_swtpm()?;
    let mut cmd = qemu_command(root, arch, &image, "mon:stdio", None, disks)?;
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

/// Boots `arch` headless, captures the serial transcript, and passes when every line of
/// `expects` appears and QEMU exits with `status` before the timeout. With `kernel`, the image
/// is rebuilt first, with `cmdline` as the kernel command line. With `send`, the text is
/// written to QEMU's stdin (the serial console) once the trigger line has appeared.
/// What a smoke boot runs and expects.
#[derive(Clone, Copy)]
pub struct SmokeOptions<'a> {
    /// The kernel to image, or the existing image when `None`.
    pub kernel: Option<&'a Path>,
    /// The kernel command line.
    pub cmdline: Option<&'a str>,
    /// Serial lines that must appear.
    pub expects: &'a [&'a str],
    /// `--reject`: serial lines that must not appear; any of them fails the run.
    pub rejects: &'a [&'a str],
    /// The QEMU exit status expected.
    pub status: i32,
    /// `(after this line, send this text)` on the serial console, in order: each pair waits
    /// for its trigger line after the previous text was sent.
    pub sends: &'a [(&'a str, &'a str)],
    /// `--until-seen`: succeed as soon as every expected line is seen, and stop QEMU (a
    /// shell session has no way to end the emulator).
    pub until_seen: bool,
    /// The init module to put on the image.
    pub init: Option<&'a Path>,
    /// The ramdisk module to put on the image.
    pub ramdisk: Option<&'a Path>,
    /// `--expect-ramdisk`: also expect rd(4)'s line for the ramdisk the image carries
    /// (`rd0: <N> bytes, ffs magic ok`), or the kernel's `rd: no ramdisk module` without one.
    pub expect_ramdisk: bool,
    /// The persistent disks: `--disk-fresh`, `--disks N`, `--disk-set NAME`.
    pub disks: Disks<'a>,
}

/// Firmware failures that end a boot before the kernel runs and are not ours: each is the two
/// texts of one line of the firmware's console, logged as an external bug in
/// `docs/EXTERNAL_BUGS.md` (the id is the last field). A smoke boot that fails with one of them
/// on its transcript and no kernel line yet is booted once more ([`smoke`]).
const FIRMWARE_FLAKES: &[(&str, &str, &str)] = &[(
    "ASSERT [UhciDxe]",
    "UhciSched.c(974): CR has Bad Signature",
    "EXT-1",
)];

/// The marker `smoke` prints before booting again after a [`FIRMWARE_FLAKES`] failure;
/// `smoke-all` counts it in each recipe's log.
pub(crate) const FIRMWARE_RETRY_MARKER: &str = "retrying once after a known firmware bug";

/// The external bug id of the firmware failure on `serial`, when the boot ended in one of
/// [`FIRMWARE_FLAKES`] before the kernel printed anything (its first line is `bsd: `).
fn firmware_flake(serial: &str) -> Option<&'static str> {
    if serial.contains("bsd: ") {
        return None;
    }
    FIRMWARE_FLAKES
        .iter()
        .find(|(a, b, _)| serial.lines().any(|l| l.contains(a) && l.contains(b)))
        .map(|(_, _, id)| *id)
}

/// Boots `arch` under QEMU and checks the serial lines and the exit status (`--expect`,
/// `--reject`, `--status`, `--until-seen`, ...). A boot that fails because the firmware hit
/// one of [`FIRMWARE_FLAKES`] before the kernel ran is booted once more (the user's decision
/// of 2026-10-09: the failure is the firmware's, a fresh QEMU each time, and our code never
/// ran); the marker line says so, and a second failure of any kind fails the smoke.
pub fn smoke(root: &Path, arch: Arch, opts: &SmokeOptions<'_>) -> Result<()> {
    let mut flake = None;
    match smoke_once(root, arch, opts, &mut flake) {
        Err(e) if flake.is_some() => {
            println!(
                "xtask: smoke {}: {FIRMWARE_RETRY_MARKER} ({}, docs/EXTERNAL_BUGS.md) before the \
                 kernel ran: {e}",
                arch.name(),
                flake.unwrap_or_default()
            );
            let mut again = None;
            smoke_once(root, arch, opts, &mut again)
        }
        result => result,
    }
}

/// One boot of [`smoke`]; on a failure, `flake` is set to the external bug id when it was a
/// [`FIRMWARE_FLAKES`] one.
fn smoke_once(
    root: &Path,
    arch: Arch,
    opts: &SmokeOptions<'_>,
    flake: &mut Option<&'static str>,
) -> Result<()> {
    let SmokeOptions {
        kernel,
        cmdline,
        expects,
        rejects,
        status,
        sends,
        until_seen,
        init,
        ramdisk,
        expect_ramdisk,
        disks,
    } = *opts;
    let image = match kernel {
        Some(k) => image(root, arch, k, cmdline, init, ramdisk)?,
        None => {
            let p = image_path(root, arch, None);
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
    let expected_status = status;
    let limit = time_limit(SMOKE_TIMEOUT);
    // M16e (hwopts.rs): `--tpm`'s swtpm, stopped when this returns, after QEMU.
    let _swtpm = crate::hwopts::start_swtpm()?;
    let mut cmd = qemu_command(root, arch, &image, "stdio", None, &disks)?;
    cmd.stdin(if !sends.is_empty() {
        Stdio::piped()
    } else {
        Stdio::null()
    })
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    println!("xtask: {}", command_line(&cmd));

    let started = Instant::now();
    let mut child = cmd.spawn().map_err(|e| spawn_error(arch, &e))?;
    let stdout = child.stdout.take().ok_or("qemu stdout is not a pipe")?;
    let stderr = child.stderr.take().ok_or("qemu stderr is not a pipe")?;
    let mut stdin = child.stdin.take();
    // The transcript so far, shared with the reader so `send` can wait for its trigger line.
    let transcript: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let out_reader = {
        let transcript = Arc::clone(&transcript);
        thread::spawn(move || slurp_into(stdout, &transcript))
    };
    let err_reader = thread::spawn(move || slurp(stderr));

    let rd_expect = expect_ramdisk.then(|| ramdisk_expectation(ramdisk));
    let all_expects: Vec<&str> = expects
        .iter()
        .copied()
        .chain(rd_expect.as_deref())
        .collect();
    let mut timed_out = false;
    let mut stopped_when_seen = false;
    let mut next_send = 0;
    // Where in the transcript the next trigger is looked for: after the previous send.
    let mut search_from = 0;
    let mut heartbeat = Heartbeat::new();
    let exit = loop {
        heartbeat.tick(arch.name(), transcript.lock().map(|t| t.len()).unwrap_or(0));
        if let Some((after, text)) = sends.get(next_send).copied() {
            let found = transcript.lock().ok().and_then(|t| {
                let s = String::from_utf8_lossy(&t[search_from.min(t.len())..]).into_owned();
                s.find(after).map(|_| t.len())
            });
            if let Some(len) = found {
                if let Some(stdin) = stdin.as_mut() {
                    let text = crate::hwopts::expand_send(text);
                    send_paced(stdin, &text)?;
                    println!(
                        "xtask: sent {text:?} after {:.1}s (saw {after:?})",
                        started.elapsed().as_secs_f32()
                    );
                }
                search_from = len;
                next_send += 1;
            }
        }
        if crate::devices::usb_serial_pending() {
            let text = transcript
                .lock()
                .map(|t| String::from_utf8_lossy(&t).into_owned())
                .unwrap_or_default();
            crate::devices::poll_usb_serial(&text, &image)?;
        }
        if crate::hwopts::monitor_pending() {
            let text = transcript
                .lock()
                .map(|t| String::from_utf8_lossy(&t).into_owned())
                .unwrap_or_default();
            crate::hwopts::poll_monitor(&text)?;
        }
        if until_seen
            && next_send == sends.len()
            && !crate::hwopts::monitor_pending()
            && !crate::devices::usb_serial_pending()
        {
            let all = transcript
                .lock()
                .map(|t| {
                    let s = String::from_utf8_lossy(&t);
                    all_expects.iter().all(|e| s.contains(e))
                })
                .unwrap_or(false);
            if all {
                stopped_when_seen = true;
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
        if let Some(status) = child.try_wait()? {
            break Some(status);
        }
        if started.elapsed() > limit {
            timed_out = true;
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        thread::sleep(Duration::from_millis(100));
    };
    drop(stdin);
    out_reader.join().ok();
    let serial = transcript
        .lock()
        .map(|t| String::from_utf8_lossy(&t).into_owned())
        .unwrap_or_default();
    let diagnostics = String::from_utf8_lossy(&err_reader.join().unwrap_or_default()).into_owned();

    let missing: Vec<&str> = all_expects
        .iter()
        .copied()
        .filter(|e| !serial.contains(e))
        .collect();
    let seen_rejects = rejected(&serial, rejects);
    let code = exit.and_then(|s| s.code());
    let ok = missing.is_empty()
        && seen_rejects.is_empty()
        && (stopped_when_seen || code == Some(expected_status));
    let elapsed = started.elapsed().as_secs_f32();
    if ok {
        // The kernel's own lines, for the record; firmware and bootloader chatter before the
        // first `bsd: ` line is left out.
        let mut kernel_output = false;
        for line in serial.lines() {
            if let Some(at) = line.find("bsd: ") {
                kernel_output = true;
                println!("  {}", line[at..].trim_end());
            } else if kernel_output {
                println!("  {}", line.trim_end());
            }
        }
        println!(
            "smoke {}: ok in {elapsed:.1}s ({} expected line(s) seen, {})",
            arch.name(),
            all_expects.len(),
            if stopped_when_seen {
                "stopped once all were seen".to_string()
            } else {
                format!("status {expected_status}")
            }
        );
        crate::hwopts::after_smoke()?;
        return crate::devices::after_smoke(&image);
    }
    *flake = firmware_flake(&serial);
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
        "smoke {}: {} (expected {expected_status}); {}{}",
        arch.name(),
        why,
        if missing.is_empty() {
            "every expected line was seen".to_string()
        } else {
            format!("NOT seen: {}", missing.join(" | "))
        },
        if seen_rejects.is_empty() {
            String::new()
        } else {
            format!("; REJECTED line(s) seen: {}", seen_rejects.join(" | "))
        }
    )
    .into())
}

/// The lines of `rejects` that appear in `serial` (`--reject`).
pub(crate) fn rejected<'a>(serial: &str, rejects: &[&'a str]) -> Vec<&'a str> {
    rejects
        .iter()
        .copied()
        .filter(|r| serial.contains(r))
        .collect()
}

/// The line rd(4)'s self-test prints for `ramdisk` (its size is the module's), or the line
/// the boot glue prints without a ramdisk module.
fn ramdisk_expectation(ramdisk: Option<&Path>) -> String {
    match ramdisk.and_then(|p| fs::metadata(p).ok()) {
        Some(m) => format!("rd0: {} bytes, ffs magic ok", m.len()),
        None => "rd: no ramdisk module".to_string(),
    }
}

pub(crate) fn slurp(mut r: impl Read) -> Vec<u8> {
    let mut v = Vec::new();
    let _ = r.read_to_end(&mut v);
    v
}

/// Reads `r` to its end, appending to `into` as the bytes arrive.
/// Bytes written to a guest's serial console at once by [`send_paced`].
const SEND_CHUNK: usize = 32;

/// The pause between two chunks of [`send_paced`].
const SEND_PAUSE: Duration = Duration::from_millis(20);

/// Types `text` on a guest's serial console the way a person or a slow line would: in chunks
/// of [`SEND_CHUNK`] bytes, [`SEND_PAUSE`] apart. QEMU hands a whole line to the UART at
/// once, and arm64's pluart(4) keeps only 128 bytes of input until its soft interrupt runs;
/// when the host is loaded the guest's vCPU may not get there in time, and a long command line
/// loses its middle (`pluart0: 0 silo overflows, 2 ibuf overflows`). Pacing keeps every smoke
/// line whole on both archs.
pub(crate) fn send_paced(out: &mut impl Write, text: &str) -> io::Result<()> {
    let bytes = text.as_bytes();
    for (i, chunk) in bytes.chunks(SEND_CHUNK).enumerate() {
        if i > 0 {
            thread::sleep(SEND_PAUSE);
        }
        out.write_all(chunk)?;
        out.flush()?;
    }
    Ok(())
}

pub(crate) fn slurp_into(mut r: impl Read, into: &Mutex<Vec<u8>>) {
    let mut buf = [0u8; 4096];
    loop {
        match r.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if let Ok(mut t) = into.lock() {
                    t.extend_from_slice(&buf[..n]);
                }
            }
        }
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extra_disks_are_numbered_files_beside_the_first() {
        let root = Path::new("/r");
        assert_eq!(
            disk_path_n(root, Arch::Amd64, None, 0),
            disk_path(root, Arch::Amd64, None)
        );
        assert_eq!(
            disk_path_n(root, Arch::Amd64, None, 3),
            Path::new("/r/target/disk-amd64-sd3.img")
        );
        assert_eq!(
            disk_path_n(root, Arch::Arm64, Some("b"), 1),
            Path::new("/r/target/disk-arm64-b-sd1.img")
        );
    }

    #[test]
    fn disk_paths_are_per_arch_and_per_vm() {
        let root = Path::new("/r");
        assert_eq!(
            disk_path(root, Arch::Amd64, None),
            Path::new("/r/target/disk-amd64.img")
        );
        assert_eq!(
            disk_path(root, Arch::Arm64, Some("a")),
            Path::new("/r/target/disk-arm64-a.img")
        );
        assert_ne!(
            disk_path(root, Arch::Arm64, Some("a")),
            disk_path(root, Arch::Arm64, Some("b"))
        );
    }

    #[test]
    fn disk_is_created_sparse_reused_and_recreated_when_fresh() {
        let dir = std::env::temp_dir().join(format!("xtask-disk-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("disk-test.img");
        // Missing: created at the full size.
        assert!(ensure_disk(&path, false).unwrap());
        assert_eq!(fs::metadata(&path).unwrap().len(), DISK_SIZE);
        // Present: kept as is, contents included.
        fs::write(&path, b"guest data").unwrap();
        assert!(!ensure_disk(&path, false).unwrap());
        assert_eq!(fs::read(&path).unwrap(), b"guest data");
        // Fresh: recreated, zero-filled.
        assert!(ensure_disk(&path, true).unwrap());
        assert_eq!(fs::metadata(&path).unwrap().len(), DISK_SIZE);
        let mut head = [1u8; 16];
        File::open(&path).unwrap().read_exact(&mut head).unwrap();
        assert_eq!(head, [0u8; 16]);
        // Fresh on a missing file just creates it.
        fs::remove_file(&path).unwrap();
        assert!(ensure_disk(&path, true).unwrap());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn timeout_scale_is_a_small_whole_factor() {
        assert_eq!(timeout_scale(None), 1);
        assert_eq!(timeout_scale(Some("2")), 2);
        assert_eq!(timeout_scale(Some(" 3\n")), 3);
        assert_eq!(timeout_scale(Some("0")), 1);
        assert_eq!(timeout_scale(Some("11")), 1);
        assert_eq!(timeout_scale(Some("1.5")), 1);
    }

    #[test]
    fn only_a_known_firmware_failure_before_the_kernel_is_a_flake() {
        let edk2 = "BdsDxe: loading Boot0001\r\nASSERT [UhciDxe] /home/kraxel/projects/qemu/roms/\
                    edk2/MdeModulePkg/Bus/Pci/UhciDxe/UhciSched.c(974): CR has Bad Signature\r\n";
        assert_eq!(firmware_flake(edk2), Some("EXT-1"));
        // Once the kernel has printed, a failure is ours, whatever came before.
        let late = format!("{edk2}bsd: EmiBSD 8.0\n");
        assert_eq!(firmware_flake(&late), None);
        // Another assertion, or the two halves on different lines, is not this one.
        assert_eq!(
            firmware_flake("ASSERT [UhciDxe] UhciSched.c(1): other\n"),
            None
        );
        assert_eq!(
            firmware_flake("ASSERT [UhciDxe]\nUhciSched.c(974): CR has Bad Signature\n"),
            None
        );
        assert_eq!(firmware_flake("panic: uvm_fault\n"), None);
    }

    #[test]
    fn heartbeat_is_due_once_a_minute() {
        let mut h = Heartbeat::new();
        let s = |n| Duration::from_secs(n);
        assert_eq!(h.due(s(59), "amd64", 10), None);
        assert_eq!(
            h.due(s(60), "amd64", 10).as_deref(),
            Some("xtask: amd64: still running after 60s, 10 serial bytes")
        );
        assert_eq!(h.due(s(61), "amd64", 10), None);
        // A late poll prints one line, not one per minute missed.
        assert!(h.due(s(250), "amd64", 99).is_some());
        assert_eq!(h.due(s(299), "amd64", 99), None);
        assert!(h.due(s(300), "amd64", 99).is_some());
    }

    #[test]
    fn rejected_lines_are_the_ones_in_the_transcript() {
        let serial = "boot\nuptime went backwards by 27000000 ns (5 -> 4)\nok\n";
        assert_eq!(
            rejected(serial, &["uptime went backwards", "panic:"]),
            vec!["uptime went backwards"]
        );
        assert!(rejected(serial, &[]).is_empty());
        assert!(rejected("boot\nok\n", &["uptime went backwards"]).is_empty());
    }

    #[test]
    fn ramdisk_expectation_names_the_size_or_its_absence() {
        assert_eq!(ramdisk_expectation(None), "rd: no ramdisk module");
        let missing = Path::new("/nonexistent/ramdisk.ffs");
        assert_eq!(ramdisk_expectation(Some(missing)), "rd: no ramdisk module");
        let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        let len = fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
        assert_eq!(
            ramdisk_expectation(Some(&file)),
            format!("rd0: {len} bytes, ffs magic ok")
        );
    }

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
/* </TESTS> */
