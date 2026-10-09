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
//! `cargo xtask diff-openbsd` (M12+): the same scenarios on EmiBSD and on a real OpenBSD,
//! compared step by step (exit statuses, outputs, errno names, file contents).
//!
//! ```text
//! cargo xtask diff-openbsd [--arch A]... [--smp N] [fetch | install | run | powerbtn]
//! cargo xtask diff-openbsd --arch A [--ipmi] [--nic MODEL] [--usb] [--usb-hc xhci|ehci|uhci|ohci] [--ukc CMD]...
//!                           [--virtio-rng] [--balloon] [--virtio-gpu] [--parallel FILE] [--pcspk]
//!                           [--audio MODEL] [--sendkey-after LINE --sendkeys KEYS]...
//!                           [--monitor-after LINE --monitor CMD]... [--machine pc]
//!                           [--ide|--megasas|...|--floppy FILE]... [--sh CMD] probe
//! ```
//!
//! - `fetch`: the OpenBSD -current snapshot recorded in `tools/xtask/openbsd-snapshot.toml`
//!   (the user's choice: the one nearest the pin), downloaded once from the official mirror
//!   into `target/openbsd/<arch>/` and checked against the snapshot's own `SHA256` file and
//!   the hashes recorded here. A mismatch stops everything.
//! - `install`: autoinstall(8) from the snapshot's `install80.img` onto
//!   `target/openbsd/<arch>/disk.img`, headless over the serial console: the installer is
//!   told `A`utoinstall and given `install.conf` from a small HTTP server on this machine
//!   (`http.rs`; `tools/xtask/diff-openbsd/install.conf`). A first boot then sets
//!   `library_aslr=NO` and drops the kernel relink kit, so later boots spend no time
//!   relinking, and halts. The installed disk is kept and reused (`installed` marker).
//! - `run` (the default; fetches and installs first when needed): boots EmiBSD (the
//!   multiprocessor kernel and the ramdisk, as the smokes do) and the installed OpenBSD
//!   (with `-snapshot`, so the disk is never changed) side by side, each with a blank 64 MiB
//!   scratch disk, logs in on both, and runs every scenario set (`tools/xtask/diff-openbsd/
//!   <set>.scn`, `scenario.rs`) as a shell script both fetch with ftp(1) from the HTTP
//!   server. The two transcripts are cut into steps, normalized, compared, and checked
//!   against the expected differences (`tools/xtask/diff-openbsd/expected.toml`). Any other
//!   difference, or an expected one that no longer happens, fails the run.
//! - `powerbtn` (M16f): boots the installed OpenBSD alone (`-snapshot`) with QEMU's monitor
//!   on a socket, logs in, prints what the kernel attached for the power key (on arm64
//!   `virt,acpi=off`, the `gpio-keys` node on the PL061), sends the monitor's
//!   `system_powerdown` and reports whether OpenBSD powered off within a minute
//!   (`<run dir>/<arch>/openbsd-powerbtn.log`). It is how M16f checked what OpenBSD 8.0
//!   does with QEMU's power key before porting gpiokeys(4).
//! - `probe` (M16e): boots the installed OpenBSD alone (`-snapshot`) with the smokes' device
//!   options (`hwopts.rs`: `--ipmi`, and `--nic MODEL` (M16c), the user network's NIC in
//!   virtio-net's place, which the installed system leaves unconfigured, so `--sh` sets it
//!   up; `devices.rs`: `--usb` and `--usb-hc xhci|ehci|uhci`,
//!   the M12 stick on that controller, `openbsd-probe.usb` in the work directory; M16a:
//!   `--machine pc`, the installed disk then on an AHCI controller added last as EmiBSD's
//!   boot image is, and `storage.rs`'s controllers with their disks in the run directory),
//!   logs in and runs `dmesg` and the shell command
//!   `--sh` gives (`<run dir>/<arch>/openbsd-probe.log`). Each `--ukc CMD` makes it boot
//!   with `boot -c` at efiboot's `boot>` prompt and send CMD at `UKC>`, then `quit`, as an
//!   OpenBSD user enables a GENERIC line marked `disable`. It is how M16e checked what
//!   OpenBSD 8.0 does with ichiic(4) under OVMF and with ipmi(4) on QEMU's simulated BMC,
//!   and M16b what it does with ehci(4) on QEMU's `usb-ehci`, and M16c what its network
//!   drivers do on QEMU's NIC models. M16d added the console, virtio and legacy devices
//!   (`hwopts.rs`: `--virtio-rng`, `--balloon`, `--virtio-gpu`, `--parallel FILE`;
//!   `devices.rs`: `--pcspk`, `--audio es1370`) and the smokes' monitor pairs: while `--sh`
//!   runs, each `--sendkey-after LINE --sendkeys KEYS` and `--monitor-after LINE --monitor
//!   CMD` fires once LINE is in its output (the VM's monitor is then the smokes' socket,
//!   `hwopts::poll_monitor`); afterwards the probe prints what the `--parallel` file holds
//!   and what the WAV file of `--audio`/`--pcspk` measures (`openbsd-probe.wav`), without
//!   requiring anything.
//!
//! The OpenBSD binaries are test fixtures under `target/` only: never committed, never
//! redistributed. Per-run files (logs, scripts, reports, the OpenBSD VM's variable store) go
//! to `<run dir>/<arch>/` ([`work_dir`]), EmiBSD's image and disk to the run directory itself
//! (`EMIBSD_RUN_DIR`, `boot::run_dir`); `just diff-openbsd` sets it to `target/diff-openbsd`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::Result;
use crate::boot::{self, Arch};

pub(crate) mod http;
mod scenario;
pub(crate) mod serial;

use serial::{Stop, Vm};

const SNAPSHOT_FILE: &str = "tools/xtask/openbsd-snapshot.toml";
const DATA_DIR: &str = "tools/xtask/diff-openbsd";
const EXPECTED_FILE: &str = "tools/xtask/diff-openbsd/expected.toml";
/// The scenario sets, in the order they run (`setup` formats and mounts the scratch disk).
const SETS: &[&str] = &["setup", "syscalls", "fs"];
/// The installed system's root password (`install.conf`), the same as the ramdisk's.
const ROOT_PASSWORD: &str = "emibsd";
/// Size of the OpenBSD VM's disk (sparse).
const OPENBSD_DISK_SIZE: u64 = 8 << 30;

/// `tools/xtask/openbsd-snapshot.toml`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    mirror: String,
    version: String,
    #[allow(dead_code)]
    pin: String,
    #[allow(dead_code)]
    fetched: String,
    arch: BTreeMap<String, ArchSnapshot>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ArchSnapshot {
    build_date: String,
    sha256_file: String,
    install_img: String,
    install_img_sha256: String,
}

/// `cargo xtask diff-openbsd ...`.
pub(crate) fn diff_openbsd(root: &Path, args: &[&str]) -> Result<()> {
    let mut arches = Vec::new();
    let mut what = "run";
    let mut kernel_dir = None;
    let mut ukc = Vec::new();
    let mut sh = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match *a {
            "--arch" => arches.push(Arch::parse(it.next().ok_or("--arch needs a value")?)?),
            "--smp" => {
                it.next();
            }
            "--kernel-dir" => kernel_dir = Some(*it.next().ok_or("--kernel-dir needs a value")?),
            "--ukc" => ukc.push(*it.next().ok_or("--ukc needs a value")?),
            "--sh" => sh = Some(*it.next().ok_or("--sh needs a value")?),
            // A device option: `hwopts::set` or `devices::set_from_args` (main) has recorded
            // it; `probe` adds the device.
            "--ipmi" | "--usb" | "--virtio-rng" | "--balloon" | "--virtio-gpu" | "--pcspk"
            | "--expect-tone" | "--usb-mouse" | "--usb-tablet" | "--usb-wacom-tablet"
            | "--usb-ccid" => {}
            "--usb-hc" | "--nic" | "--audio" | "--parallel" | "--monitor-after" | "--monitor"
            | "--sendkey-after" | "--sendkeys" | "--machine" => {
                it.next();
            }
            // M16a: a storage controller and its disk (`storage.rs`).
            o if crate::storage::options().any(|s| s == o) => {
                it.next();
            }
            "fetch" | "install" | "run" | "powerbtn" | "probe" => what = a,
            other => return Err(format!("diff-openbsd: unknown argument {other:?}").into()),
        }
    }
    if arches.is_empty() {
        arches = vec![Arch::Amd64, Arch::Arm64];
    }
    let snap = load_snapshot(root)?;
    let mut failed = Vec::new();
    for arch in arches {
        let a = snap
            .arch
            .get(arch.name())
            .ok_or_else(|| format!("{SNAPSHOT_FILE}: no [arch.{}]", arch.name()))?;
        fetch(root, &snap, arch, a)?;
        if what == "fetch" {
            continue;
        }
        install(root, &snap, arch, a)?;
        if what == "install" {
            continue;
        }
        if what == "powerbtn" {
            powerbtn(root, arch)?;
            continue;
        }
        if what == "probe" {
            probe(root, arch, &ukc, sh)?;
            continue;
        }
        let started = Instant::now();
        let ok = run(root, arch, kernel_dir)?;
        println!(
            "diff-openbsd {}: {} in {:.0}s",
            arch.name(),
            if ok { "ok" } else { "FAILED" },
            started.elapsed().as_secs_f32()
        );
        if !ok {
            failed.push(arch.name());
        }
    }
    if failed.is_empty() {
        Ok(())
    } else {
        Err(format!("diff-openbsd: differences on {}", failed.join(", ")).into())
    }
}

fn load_snapshot(root: &Path) -> Result<Snapshot> {
    let p = root.join(SNAPSHOT_FILE);
    let text = fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    Ok(toml::from_str(&text).map_err(|e| format!("{}: {e}", p.display()))?)
}

/// `target/openbsd/<arch>`: the downloaded snapshot and the installed disk.
fn cache_dir(root: &Path, arch: Arch) -> PathBuf {
    root.join("target").join("openbsd").join(arch.name())
}

/// This run's files: `$EMIBSD_RUN_DIR/<arch>` (`just diff-openbsd` sets it to
/// `target/diff-openbsd`), or `target/diff-openbsd/<arch>` without it.
fn work_dir(root: &Path, arch: Arch) -> Result<PathBuf> {
    let base = match std::env::var_os(boot::RUN_DIR_ENV) {
        Some(d) if !d.is_empty() => boot::run_dir(root),
        _ => root.join("target").join("diff-openbsd"),
    };
    let d = base.join(arch.name());
    fs::create_dir_all(&d).map_err(|e| format!("{}: {e}", d.display()))?;
    Ok(d)
}

/// `shasum -a 256 <file>` (part of macOS).
fn sha256(path: &Path) -> Result<String> {
    let out = Command::new("shasum")
        .args(["-a", "256"])
        .arg(path)
        .output()
        .map_err(|e| format!("shasum: {e}"))?;
    if !out.status.success() {
        return Err(format!("shasum {}: failed", path.display()).into());
    }
    let s = String::from_utf8_lossy(&out.stdout);
    Ok(s.split_whitespace().next().unwrap_or("").to_string())
}

/// The hash a snapshot `SHA256` file gives `name`.
pub(crate) fn listed_hash(sha256_file: &str, name: &str) -> Option<String> {
    let prefix = format!("SHA256 ({name}) = ");
    sha256_file
        .lines()
        .find_map(|l| l.strip_prefix(&prefix))
        .map(|h| h.trim().to_string())
}

fn curl(url: &str, to: &Path) -> Result<()> {
    println!("xtask: downloading {url}");
    let st = Command::new("curl")
        .args(["-fsSL", "--retry", "3", "-o"])
        .arg(to)
        .arg(url)
        .status()
        .map_err(|e| format!("curl: {e}"))?;
    if !st.success() {
        return Err(format!("curl {url}: {st}").into());
    }
    Ok(())
}

/// Downloads (once) and verifies the install image of `arch`.
fn fetch(root: &Path, snap: &Snapshot, arch: Arch, a: &ArchSnapshot) -> Result<()> {
    let dir = cache_dir(root, arch);
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let img = dir.join(&a.install_img);
    let sums = dir.join("SHA256");
    let base = format!("{}/{}", snap.mirror.trim_end_matches('/'), arch.name());
    if !sums.is_file() {
        curl(&format!("{base}/SHA256"), &sums)?;
    }
    let refuse = |why: String| -> Result<()> {
        Err(format!(
            "OpenBSD snapshot for {}: {why}; refusing it. The mirrors keep only the latest \
             snapshot: if it moved on, copy target/openbsd/ from a machine that has this one, \
             or ask the user to record a new snapshot in {SNAPSHOT_FILE}",
            arch.name()
        )
        .into())
    };
    if sha256(&sums)? != a.sha256_file {
        let _ = fs::remove_file(&sums);
        return refuse("its SHA256 file is not the one recorded".into());
    }
    let text = fs::read_to_string(&sums).map_err(|e| format!("{}: {e}", sums.display()))?;
    if listed_hash(&text, &a.install_img).as_deref() != Some(a.install_img_sha256.as_str()) {
        return refuse(format!(
            "SHA256 does not list {} as recorded",
            a.install_img
        ));
    }
    if img.is_file() && sha256(&img)? == a.install_img_sha256 {
        return Ok(());
    }
    let part = dir.join(format!("{}.part", a.install_img));
    curl(&format!("{base}/{}", a.install_img), &part)?;
    let got = sha256(&part)?;
    if got != a.install_img_sha256 {
        let _ = fs::remove_file(&part);
        return refuse(format!("{} has SHA256 {got}", a.install_img));
    }
    fs::rename(&part, &img).map_err(|e| format!("{}: {e}", img.display()))?;
    println!(
        "xtask: {} verified (snapshot built {})",
        img.display(),
        a.build_date
    );
    Ok(())
}

/// How the OpenBSD VM boots.
enum Boot<'a> {
    /// From the install image, onto the blank disk.
    Install(&'a Path),
    /// From the installed disk, writing to it.
    Prepare,
    /// From the installed disk under `-snapshot`, with a scratch disk.
    Run(&'a Path),
    /// From the installed disk under `-snapshot`, alone, with QEMU's monitor on the Unix
    /// socket given (`powerbtn`, `probe`).
    Probe(&'a Path),
}

/// QEMU for the OpenBSD VM. Disks: on amd64 PCI slots go up and the disk added first is
/// `sd0`; on arm64 `virt` the virtio-mmio device added last is found first. The installed
/// disk is `sd0` in every mode, the install image or the scratch disk `sd1`.
fn openbsd_qemu(root: &Path, arch: Arch, mode: &Boot<'_>) -> Result<Command> {
    let disk = cache_dir(root, arch).join("disk.img");
    let code = boot::edk2_file(arch.edk2_code())?;
    let vars_src = boot::edk2_file(arch.edk2_vars())?;
    let vars = work_dir(root, arch)?.join("openbsd-vars.fd");
    fs::copy(&vars_src, &vars).map_err(|e| format!("{}: {e}", vars.display()))?;
    let mut cmd = Command::new(arch.qemu());
    let monitor = match mode {
        Boot::Probe(sock) => format!("unix:{},server=on,wait=off", sock.display()),
        _ => "none".to_string(),
    };
    cmd.args([
        "-m",
        "1024",
        "-display",
        "none",
        "-monitor",
        &monitor,
        "-no-reboot",
    ]);
    cmd.args(["-serial", "stdio", "-boot", "menu=on,splash-time=0"]);
    // At least two processors: the installer offers bsd.mp only on a multiprocessor.
    cmd.args(["-smp", &boot::smp().unwrap_or(2).max(2).to_string()]);
    cmd.arg("-drive").arg(format!(
        "if=pflash,format=raw,readonly=on,file={}",
        code.display()
    ));
    cmd.arg("-drive")
        .arg(format!("if=pflash,format=raw,file={}", vars.display()));
    // The first boot runs rc.firsttime, whose fw_update(8) and syspatch(8) would reach the
    // Internet: that boot gets no network at all (`restrict=on`), so nothing but the
    // recorded snapshot is ever downloaded. The installer and the runs only talk to this
    // machine's HTTP server (10.0.2.2).
    let restrict = if matches!(mode, Boot::Prepare) {
        ",restrict=on"
    } else {
        ""
    };
    cmd.args(["-netdev", &format!("user,id=n0{restrict}")]);
    let (dev, net) = match arch {
        Arch::Amd64 => {
            // `probe` takes `--machine pc` (hwopts, M16a: PIIX3's IDE controller).
            let machine = match mode {
                Boot::Probe(_) => crate::hwopts::amd64_machine(),
                _ => "q35",
            };
            cmd.args(["-M", machine, "-cpu", "qemu64"]);
            ("virtio-blk-pci", "virtio-net-pci")
        }
        Arch::Arm64 => {
            cmd.args(["-M", "virt,acpi=off", "-cpu", "cortex-a72"]);
            ("virtio-blk-device", "virtio-net-device")
        }
    };
    // `probe` takes `--nic`'s model (hwopts) in the virtio NIC's place.
    match (mode, crate::hwopts::nic_model()) {
        (Boot::Probe(_), Some(model)) => cmd.args(["-device", &format!("{model},netdev=n0")]),
        _ => cmd.args(["-device", &format!("{net},netdev=n0")]),
    };
    let (second, second_opts, boot_second) = match mode {
        Boot::Install(img) => (Some(*img), ",snapshot=on", true),
        Boot::Prepare | Boot::Probe(_) => (None, "", false),
        Boot::Run(scratch) => (Some(*scratch), "", false),
    };
    let mut root_disk = vec![
        "-drive".to_string(),
        format!("if=none,format=raw,file={},id=d0", disk.display()),
        "-device".to_string(),
        format!(
            "{dev},drive=d0{}",
            if boot_second { "" } else { ",bootindex=0" }
        ),
    ];
    // On `pc` (`probe --machine pc`), `hwopts::add_devices` puts the drive `hd0` on an AHCI
    // controller added last, as it does EmiBSD's boot image, so PIIX3's IDE channels stay
    // free: the installed disk is that drive.
    if arch == Arch::Amd64 && crate::hwopts::machine_pc() && matches!(mode, Boot::Probe(_)) {
        root_disk = vec![
            "-drive".to_string(),
            format!("if=none,format=raw,file={},id=hd0", disk.display()),
        ];
    }
    let other_disk = second.map(|p| {
        [
            "-drive".to_string(),
            format!("if=none,format=raw,file={},id=d1{second_opts}", p.display()),
            "-device".to_string(),
            format!(
                "{dev},drive=d1{}",
                if boot_second { ",bootindex=0" } else { "" }
            ),
        ]
    });
    match arch {
        Arch::Amd64 => {
            cmd.args(&root_disk);
            if let Some(o) = &other_disk {
                cmd.args(o);
            }
        }
        Arch::Arm64 => {
            if let Some(o) = &other_disk {
                cmd.args(o);
            }
            cmd.args(&root_disk);
        }
    }
    if matches!(mode, Boot::Run(_) | Boot::Probe(_)) {
        cmd.arg("-snapshot");
    }
    Ok(cmd)
}

/// The installer's answers, `tools/xtask/diff-openbsd/install.conf` with `{template}` and
/// `{sets}` filled in.
fn install_conf(root: &Path, template_url: &str, sets: &str) -> Result<String> {
    let p = root.join(DATA_DIR).join("install.conf");
    let text = fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    Ok(text
        .lines()
        .filter(|l| !l.starts_with('#'))
        .map(|l| {
            l.replace("{template}", template_url)
                .replace("{sets}", sets)
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n")
}

/// Installs the snapshot onto `target/openbsd/<arch>/disk.img`, unless the marker says it
/// already holds this snapshot.
fn install(root: &Path, snap: &Snapshot, arch: Arch, a: &ArchSnapshot) -> Result<()> {
    let dir = cache_dir(root, arch);
    let marker = dir.join("installed");
    let disk = dir.join("disk.img");
    if disk.is_file() && fs::read_to_string(&marker).is_ok_and(|m| m.trim() == a.install_img_sha256)
    {
        return Ok(());
    }
    let _ = fs::remove_file(&marker);
    let _ = fs::remove_file(&disk);
    let f = fs::File::create(&disk).map_err(|e| format!("{}: {e}", disk.display()))?;
    f.set_len(OPENBSD_DISK_SIZE)
        .map_err(|e| format!("{}: {e}", disk.display()))?;
    drop(f);

    let work = work_dir(root, arch)?;
    let www = work.join("www");
    fs::create_dir_all(&www).map_err(|e| format!("{}: {e}", www.display()))?;
    let server = http::Server::start(&www)?;
    let tmpl = root.join(DATA_DIR).join("disklabel.tmpl");
    fs::copy(&tmpl, www.join("disklabel.tmpl")).map_err(|e| format!("{}: {e}", tmpl.display()))?;
    let sets = format!("{}/{}", snap.version, arch.name());
    fs::write(
        www.join("install.conf"),
        install_conf(root, &server.url("disklabel.tmpl"), &sets)?,
    )
    .map_err(|e| format!("{}: {e}", www.display()))?;

    let started = Instant::now();
    let img = dir.join(&a.install_img);
    let cmd = openbsd_qemu(root, arch, &Boot::Install(&img))?;
    let mut vm = Vm::spawn(
        &format!("openbsd-{}-install", arch.name()),
        cmd,
        work.join("openbsd-install.log"),
    )?;
    if arch == Arch::Amd64 {
        // efiboot(8) talks on the EFI console, which EDK2 puts on the serial port; the
        // kernel must be told to use com0 as well.
        vm.wait_for("boot>", boot::time_limit(Duration::from_secs(300)))?;
        vm.send("set tty com0\n")?;
        vm.wait_for("boot>", boot::time_limit(Duration::from_secs(60)))?;
        vm.send("boot\n")?;
    }
    let url = format!("{}\n", server.url("install.conf"));
    vm.respond(
        &[
            ("(I)nstall, (U)pgrade, (A)utoinstall or (S)hell?", "a\n"),
            ("should be used for the initial DHCP request?", "vio0\n"),
            ("Response file location?", &url),
        ],
        &[
            "Question has no answer in response file",
            "panic:",
            "Unable to get a response file",
            "failed; check /tmp/ai/ai.log",
        ],
        &[Stop::Exited],
        boot::time_limit(Duration::from_secs(5400)),
    )?;
    if !vm.text().contains("CONGRATULATIONS!") {
        return Err(format!(
            "{}: the installer ended without CONGRATULATIONS (see {})",
            vm.name,
            work.join("openbsd-install.log").display()
        )
        .into());
    }
    drop(vm);
    println!(
        "xtask: openbsd-{} installed in {:.0}s; first boot",
        arch.name(),
        started.elapsed().as_secs_f32()
    );

    let cmd = openbsd_qemu(root, arch, &Boot::Prepare)?;
    let mut vm = Vm::spawn(
        &format!("openbsd-{}-firstboot", arch.name()),
        cmd,
        work.join("openbsd-firstboot.log"),
    )?;
    login(&mut vm, boot::time_limit(Duration::from_secs(3600)))?;
    vm.send(
        "echo library_aslr=NO >>/etc/rc.conf.local; rm -rf /usr/share/relink/kernel; \
         sync; halt -p\n",
    )?;
    vm.wait_exit(boot::time_limit(Duration::from_secs(600)))?;
    drop(vm);
    fs::write(&marker, format!("{}\n", a.install_img_sha256))
        .map_err(|e| format!("{}: {e}", marker.display()))?;
    println!(
        "xtask: openbsd-{} ready in {:.0}s ({})",
        arch.name(),
        started.elapsed().as_secs_f32(),
        disk.display()
    );
    Ok(())
}

/// `powerbtn`: what the installed OpenBSD does when QEMU's power key is pressed.
fn powerbtn(root: &Path, arch: Arch) -> Result<()> {
    let work = work_dir(root, arch)?;
    let sock = std::env::temp_dir().join(format!("emibsd-obsd-{}.sock", std::process::id()));
    let _ = fs::remove_file(&sock);
    let cmd = openbsd_qemu(root, arch, &Boot::Probe(&sock))?;
    let mut vm = Vm::spawn(
        &format!("openbsd-{}-powerbtn", arch.name()),
        cmd,
        work.join("openbsd-powerbtn.log"),
    )?;
    login(&mut vm, boot::time_limit(Duration::from_secs(900)))?;
    vm.send(
        "dmesg | grep -i -e gpio -e pl061 -e acpibtn -e power; \
         sysctl hw.sensors machdep.pwraction 2>&1; echo @@PROBED\n",
    )?;
    vm.wait_for("@@PROBED\r\n", Duration::from_secs(120))?;
    vm.wait_for("# ", Duration::from_secs(60))?;
    thread::sleep(Duration::from_secs(2));
    let mark = vm.mark();
    crate::hwopts::monitor_command(&sock, "system_powerdown")?;
    println!("xtask: openbsd-{}: system_powerdown sent", arch.name());
    let off = vm.wait_exit(Duration::from_secs(60)).is_ok();
    let after = vm.since(mark);
    let _ = fs::remove_file(&sock);
    println!(
        "xtask: openbsd-{} powerbtn: {}; console after system_powerdown:\n{}",
        arch.name(),
        if off {
            "OpenBSD powered off"
        } else {
            "OpenBSD still running after 60 s"
        },
        after.trim()
    );
    Ok(())
}

/// `probe`: the installed OpenBSD's `dmesg` and the output of `sh`, with the device options
/// `hwopts` recorded, after the UKC commands `ukc` (none: a plain boot).
fn probe(root: &Path, arch: Arch, ukc: &[&str], sh: Option<&str>) -> Result<()> {
    let work = work_dir(root, arch)?;
    // With `--monitor-after`/`--sendkey-after` (M16d) the monitor is the smokes' socket, so
    // `hwopts::poll_monitor` drives it while `sh` runs.
    let sock = crate::hwopts::monitor_sock_path().unwrap_or_else(|| {
        std::env::temp_dir().join(format!("emibsd-obsd-{}.sock", std::process::id()))
    });
    let _ = fs::remove_file(&sock);
    let mut cmd = openbsd_qemu(root, arch, &Boot::Probe(&sock))?;
    crate::hwopts::add_devices(&mut cmd, root, arch)?;
    // M16a: the storage controllers of `storage.rs`, with their disks in the run directory.
    crate::storage::add_devices(&mut cmd, arch)?;
    // The USB devices of `devices.rs` (the stick is `openbsd-probe.usb` in the work directory).
    cmd.args(crate::devices::qemu_args(&work.join("openbsd-probe.img"))?);
    let log = work.join("openbsd-probe.log");
    let mut vm = Vm::spawn(&format!("openbsd-{}-probe", arch.name()), cmd, log.clone())?;
    if !ukc.is_empty() {
        // efiboot waits five seconds at its prompt (boot.conf sets the serial console).
        vm.wait_for("boot>", boot::time_limit(Duration::from_secs(300)))?;
        vm.send("boot -c\n")?;
        for c in ukc.iter().copied().chain(["quit"]) {
            vm.wait_for("UKC> ", boot::time_limit(Duration::from_secs(300)))?;
            vm.send(&format!("{c}\n"))?;
        }
    }
    login(&mut vm, boot::time_limit(Duration::from_secs(900)))?;
    let mark = vm.mark();
    vm.send(&format!(
        "dmesg; {} 2>&1; echo @@PROBED\n",
        sh.unwrap_or("true")
    ))?;
    // The monitor commands and keys wait for their lines in what `sh` prints.
    let limit = boot::time_limit(Duration::from_secs(300));
    let started = Instant::now();
    while vm
        .wait_for("@@PROBED\r\n", Duration::from_millis(500))
        .is_err()
    {
        if started.elapsed() > limit || vm.exited() {
            return Err(format!(
                "openbsd-{} probe: QEMU exited or no @@PROBED in {}s",
                arch.name(),
                limit.as_secs()
            )
            .into());
        }
        crate::hwopts::poll_monitor(&vm.since(mark))?;
    }
    vm.wait_for("# ", Duration::from_secs(60))?;
    let out = vm.since(mark);
    let _ = fs::write(&log, vm.text());
    let _ = fs::remove_file(&sock);
    // What the parallel port printed and the tone in the WAV file (M16d), reported, not
    // required: a probe records what OpenBSD does.
    crate::hwopts::probe_report();
    crate::devices::probe_report(&work.join("openbsd-probe.img"));
    println!(
        "xtask: openbsd-{} probe ({}):\n{}",
        arch.name(),
        log.display(),
        out.trim()
    );
    Ok(())
}

/// Logs in as root on the serial console and waits for the shell's prompt.
fn login(vm: &mut Vm, limit: Duration) -> Result<()> {
    vm.wait_for("login:", limit)?;
    vm.send("root\n")?;
    vm.wait_for("Password:", Duration::from_secs(120))?;
    vm.send(&format!("{ROOT_PASSWORD}\n"))?;
    vm.wait_for("# ", Duration::from_secs(300))?;
    Ok(())
}

/// One system's run of every set: the transcript of each set, in [`SETS`] order.
fn session(
    mut vm: Vm,
    boot_limit: Duration,
    prepare: &str,
    scripts: &[(String, String)],
) -> Result<Vec<String>> {
    login(&mut vm, boot_limit)?;
    if !prepare.is_empty() {
        vm.send(prepare)?;
        vm.wait_for("@@READY", boot::time_limit(Duration::from_secs(300)))?;
    }
    let mut out = Vec::new();
    for (set, url) in scripts {
        let mark = vm.mark();
        vm.send(&format!(
            "cd /; ftp -V -o /tmp/{set}.sh {url} >/dev/null && sh /tmp/{set}.sh\n"
        ))?;
        vm.wait_for("@@DONE", boot::time_limit(Duration::from_secs(900)))?;
        out.push(vm.since(mark));
    }
    Ok(out)
}

/// Runs and compares every set on `arch`; whether it passed.
fn run(root: &Path, arch: Arch, kernel_dir: Option<&str>) -> Result<bool> {
    let work = work_dir(root, arch)?;
    let www = work.join("www");
    fs::create_dir_all(&www).map_err(|e| format!("{}: {e}", www.display()))?;
    let difftest = root
        .join("target/userland")
        .join(arch.name())
        .join("root/usr/bin/difftest");
    fs::copy(&difftest, www.join("difftest"))
        .map_err(|e| format!("{}: {e}; run `just userland` first", difftest.display()))?;
    let mut steps_by_set = Vec::new();
    for set in SETS {
        let p = root.join(DATA_DIR).join(format!("{set}.scn"));
        let text = fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        steps_by_set.push(scenario::parse(set, &text)?);
    }
    let expected: scenario::ExpectedFile = {
        let p = root.join(EXPECTED_FILE);
        let text = fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        toml::from_str(&text).map_err(|e| format!("{}: {e}", p.display()))?
    };
    let server = http::Server::start(&www)?;
    let scripts_for = |who: &str, disk: &str| -> Result<Vec<(String, String)>> {
        let mut v = Vec::new();
        for (set, steps) in SETS.iter().zip(&steps_by_set) {
            let name = format!("{who}-{set}.sh");
            let pre = format!(
                "D={disk}\nPATH=/sbin:/bin:/usr/sbin:/usr/bin:/usr/local/bin\nexport D PATH\n"
            );
            fs::write(www.join(&name), scenario::script(&pre, steps))
                .map_err(|e| format!("{}: {e}", www.display()))?;
            v.push((set.to_string(), server.url(&name)));
        }
        Ok(v)
    };
    let obsd_scripts = scripts_for("openbsd", "sd1")?;
    let emi_scripts = scripts_for("emibsd", "sd0")?;

    // EmiBSD: the smokes' MP kernel and ramdisk; its sd0 is a fresh blank 64 MiB disk.
    let kernel = match kernel_dir {
        Some(d) => root.join(d).join("bsd"),
        None => root
            .join("target")
            .join(arch.target())
            .join("debug")
            .join("bsd"),
    };
    let image = boot::image_tagged(
        root,
        arch,
        Some("diff"),
        &kernel,
        None,
        boot::default_init(root, arch).as_deref(),
        boot::default_ramdisk(root, arch).as_deref(),
    )?;
    let ecmd = boot::qemu_command(
        root,
        arch,
        &image,
        "stdio",
        None,
        &boot::Disks {
            fresh: true,
            count: 1,
            set: Some("diff"),
        },
    )?;
    let evm = Vm::spawn(
        &format!("emibsd-{}", arch.name()),
        ecmd,
        work.join("emibsd.log"),
    )?;
    let emi_prepare = format!(
        "ifconfig vio0 inet 10.0.2.15/24 up; until ftp -V -o /dev/null {} >/dev/null 2>&1; \
         do sleep 1; done; echo @@READ\"Y\"\n",
        server.url("difftest")
    );
    let emi = thread::spawn(move || {
        session(
            evm,
            boot::time_limit(Duration::from_secs(600)),
            &emi_prepare,
            &emi_scripts,
        )
        .map_err(|e| e.to_string())
    });

    // OpenBSD: the installed disk under -snapshot, a blank scratch disk as sd1.
    let scratch = work.join("openbsd-scratch.img");
    boot::ensure_disk(&scratch, true)?;
    let ocmd = openbsd_qemu(root, arch, &Boot::Run(&scratch))?;
    let ovm = Vm::spawn(
        &format!("openbsd-{}", arch.name()),
        ocmd,
        work.join("openbsd.log"),
    )?;
    let obsd_prepare = format!(
        "until ftp -V -o /usr/local/bin/difftest {} >/dev/null 2>&1; do sleep 1; done; \
         chmod 755 /usr/local/bin/difftest; echo @@READ\"Y\"\n",
        server.url("difftest")
    );
    let obsd = session(
        ovm,
        boot::time_limit(Duration::from_secs(1800)),
        &obsd_prepare,
        &obsd_scripts,
    );
    let emi = emi.join().map_err(|_| "the EmiBSD session panicked")?;
    drop(server);
    let (obsd, emi) = (obsd?, emi?);

    let mut report_text = String::new();
    let mut ok = true;
    let (mut compared, mut equal, mut expected_n) = (0, 0, 0);
    for (i, steps) in steps_by_set.iter().enumerate() {
        let (oh, oo) = scenario::outcomes(&obsd[i], steps.len());
        let (eh, eo) = scenario::outcomes(&emi[i], steps.len());
        let os = scenario::Subst {
            host: oh,
            disk: "sd1".into(),
        };
        let es = scenario::Subst {
            host: eh,
            disk: "sd0".into(),
        };
        let r = scenario::compare(
            arch.name(),
            steps,
            (&os, &oo),
            (&es, &eo),
            &expected.differences,
        );
        compared += r.compared;
        equal += r.equal;
        expected_n += r.expected.len();
        for (id, why) in &r.expected {
            report_text.push_str(&format!("expected: {id}: {why}\n"));
        }
        for u in &r.unexpected {
            report_text.push_str(&format!("DIFFERENT: {u}"));
        }
        for s in &r.stale {
            report_text.push_str(&format!("STALE: {s}\n"));
        }
        ok &= r.passed();
    }
    let summary = format!(
        "diff-openbsd {}: {compared} steps compared, {equal} equal, {expected_n} expected \
         differences, {} unexpected\n",
        arch.name(),
        compared - equal - expected_n
    );
    report_text.push_str(&summary);
    let rp = work.join("report.txt");
    fs::write(&rp, &report_text).map_err(|e| format!("{}: {e}", rp.display()))?;
    print!("{report_text}");
    println!("xtask: report in {}", rp.display());
    Ok(ok)
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::scenario::*;
    use super::*;

    #[test]
    fn diff_lines_aligns_an_inserted_line() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let o = s(&["a", "b", "c"]);
        let e = s(&["a", "x", "b", "c"]);
        assert_eq!(
            diff_lines(&o, &e),
            vec![
                Line::Same("a"),
                Line::EmiBsd("x"),
                Line::Same("b"),
                Line::Same("c")
            ]
        );
        let e = s(&["a", "B", "c"]);
        assert_eq!(
            diff_lines(&o, &e),
            vec![
                Line::Same("a"),
                Line::OpenBsd("b"),
                Line::EmiBsd("B"),
                Line::Same("c")
            ]
        );
    }

    #[test]
    fn snapshot_file_parses_and_names_both_archs() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let s = load_snapshot(&root).unwrap();
        for a in ["amd64", "arm64"] {
            let x = &s.arch[a];
            assert_eq!(x.install_img_sha256.len(), 64);
            assert_eq!(x.sha256_file.len(), 64);
        }
    }

    #[test]
    fn listed_hash_reads_the_sha256_file() {
        let f = "SHA256 (bsd) = aa\nSHA256 (install80.img) = bb\n";
        assert_eq!(listed_hash(f, "install80.img").as_deref(), Some("bb"));
        assert_eq!(listed_hash(f, "install8.img"), None);
    }

    #[test]
    fn scenario_files_and_expected_file_parse() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut ids = Vec::new();
        for set in SETS {
            let text = fs::read_to_string(root.join(DATA_DIR).join(format!("{set}.scn"))).unwrap();
            let steps = parse(set, &text).unwrap();
            assert!(!steps.is_empty(), "{set}");
            ids.extend(steps.into_iter().map(|s| (s.id, s.cmd)));
        }
        let text = fs::read_to_string(root.join(EXPECTED_FILE)).unwrap();
        let e: ExpectedFile = toml::from_str(&text).unwrap();
        for d in &e.differences {
            assert!(
                ids.iter().any(|(id, cmd)| *id == d.step && *cmd == d.cmd),
                "expected.toml names {} / {:?}, which no scenario has",
                d.step,
                d.cmd
            );
            assert!(!d.reason.trim().is_empty());
        }
    }

    #[test]
    fn parse_steps() {
        let t = "# c\n## one\n! mkdir x\n$ ls x\n## two\n?  true\n$[dates,inodes] ls -li\n";
        let s = parse("fs", t).unwrap();
        assert_eq!(s.len(), 4);
        assert_eq!(s[0].id, "fs/one#1");
        assert_eq!(s[0].check, Check::Setup);
        assert_eq!(s[1].id, "fs/one#2");
        assert_eq!(s[2].id, "fs/two#1");
        assert_eq!(s[2].check, Check::Status);
        assert_eq!(s[2].cmd, "true");
        assert_eq!(s[3].norms, vec![Norm::Dates, Norm::Inodes]);
        assert!(parse("fs", "$ ls\n").is_err());
        assert!(parse("fs", "## a\n% ls\n").is_err());
        assert!(parse("fs", "## a\n$[bogus] ls\n").is_err());
    }

    #[test]
    fn script_and_outcomes_round_trip() {
        let steps = parse("x", "## a\n$ echo hi\n? false\n").unwrap();
        let s = script("D=sd0\n", &steps);
        assert!(s.starts_with("D=sd0\necho \"@@HOST $(hostname)\"\n"));
        assert!(s.contains("echo \"@@B 1\"\n{ echo hi\n} </dev/null 2>&1\necho \"@@E 1 $?\"\n"));
        assert!(s.ends_with("echo \"@@DONE\"\n"));
        // What the console shows: the typed command echoed, then the output.
        let t = "# sh /tmp/x.sh\r\n@@HOST box\r\n@@B 1\r\nhi\r\n@@E 1 0\r\n@@B 2\r\n@@E 2 1\r\n@@DONE\r\n";
        let (host, o) = outcomes(t, 2);
        assert_eq!(host, "box");
        assert_eq!(o[0].lines, vec!["hi".to_string()]);
        assert_eq!(o[0].status, Some(0));
        assert_eq!(o[1].status, Some(1));
        let (_, o) = outcomes("@@B 1\nhalf", 2);
        assert_eq!(o[0].status, None);
        assert_eq!(o[1], Outcome::default());
    }

    #[test]
    fn normalizers() {
        let s = Subst {
            host: "openbsd".into(),
            disk: "sd1".into(),
        };
        let l = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(
            normalize(
                &l(&[
                    "openbsd# newfs /dev/rsd1a",
                    "difftest[4242]: pledge \"rpath\""
                ]),
                &s,
                &[]
            ),
            l(&[
                "<host># newfs /dev/rsdXa",
                "difftest[PID]: pledge \"rpath\""
            ])
        );
        assert_eq!(
            normalize(
                &l(&[
                    "-rw-r--r--  1 root  wheel  6 Oct  5 02:21 f",
                    "drwx  2 root  wheel  512 Jan 12  2025 d"
                ]),
                &s,
                &[Norm::Dates]
            ),
            l(&[
                "-rw-r--r--  1 root  wheel  6 <date> f",
                "drwx  2 root  wheel  512 <date> d"
            ])
        );
        assert_eq!(
            normalize(
                &l(&["  812 a", "  77 b", "812 c", "x 9"]),
                &s,
                &[Norm::Inodes]
            ),
            l(&["  #1 a", "  #2 b", "#1 c", "x 9"])
        );
        assert_eq!(
            normalize(&l(&["a 12 b 3"]), &s, &[Norm::Numbers]),
            l(&["a N b N"])
        );
        assert_eq!(
            normalize(&l(&["Disk: sd1\tgeometry", "\tx"]), &s, &[]),
            l(&["Disk: sdX       geometry", "        x"])
        );
    }

    fn out(lines: &[&str], st: i32) -> Outcome {
        Outcome {
            lines: lines.iter().map(|x| x.to_string()).collect(),
            status: Some(st),
        }
    }

    #[test]
    fn compare_counts_expected_unexpected_and_stale() {
        let steps = parse("x", "## a\n! setup\n$ one\n$ two\n? three\n$ four\n").unwrap();
        let os = Subst {
            host: "openbsd".into(),
            disk: "sd1".into(),
        };
        let es = Subst {
            host: "Amnesiac".into(),
            disk: "sd0".into(),
        };
        let o = vec![
            out(&["junk"], 1),
            out(&["openbsd ok"], 0),
            out(&["OpenBSD"], 0),
            out(&["x"], 0),
            out(&["same"], 0),
        ];
        let e = vec![
            out(&[], 0),
            out(&["Amnesiac ok"], 0),
            out(&["EmiBSD"], 0),
            out(&["y"], 0),
            out(&["other"], 0),
        ];
        let exp = vec![
            Expected {
                step: "x/a#3".into(),
                cmd: "two".into(),
                arch: None,
                reason: "branding".into(),
            },
            Expected {
                step: "x/a#4".into(),
                cmd: "three".into(),
                arch: Some("arm64".into()),
                reason: "arm64 only".into(),
            },
        ];
        let r = compare("amd64", &steps, (&os, &o), (&es, &e), &exp);
        assert_eq!(r.compared, 4);
        assert_eq!(r.equal, 2); // #2 after the host name, #4 by status only
        assert_eq!(r.expected.len(), 1);
        assert_eq!(r.unexpected.len(), 1);
        assert!(r.unexpected[0].contains("openbsd: same"));
        assert!(r.stale.is_empty());
        assert!(!r.passed());
        // On arm64 the #4 entry applies, but #4 is equal: stale.
        let r = compare("arm64", &steps, (&os, &o), (&es, &e), &exp);
        assert_eq!(r.stale.len(), 1);
    }

    #[test]
    fn http_request_paths_stay_inside() {
        let d = Path::new("/srv");
        assert_eq!(
            http::request_path(d, "GET /install.conf?path=8.0/amd64 HTTP/1.1"),
            Some(PathBuf::from("/srv/install.conf"))
        );
        assert_eq!(http::request_path(d, "GET /../etc/passwd HTTP/1.1"), None);
        assert_eq!(http::request_path(d, "POST /x HTTP/1.1"), None);
        assert_eq!(http::request_path(d, "GET / HTTP/1.1"), None);
    }
}
/* </TESTS> */
