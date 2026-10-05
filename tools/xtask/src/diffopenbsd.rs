//! `cargo xtask diff-openbsd` (M12+): the same scenarios on EmiBSD and on a real OpenBSD,
//! compared step by step (exit statuses, outputs, errno names, file contents).
//!
//! ```text
//! cargo xtask diff-openbsd [--arch A]... [--smp N] [fetch | install | run]
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
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match *a {
            "--arch" => arches.push(Arch::parse(it.next().ok_or("--arch needs a value")?)?),
            "--smp" => {
                it.next();
            }
            "--kernel-dir" => kernel_dir = Some(*it.next().ok_or("--kernel-dir needs a value")?),
            "fetch" | "install" | "run" => what = a,
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
    cmd.args([
        "-m",
        "1024",
        "-display",
        "none",
        "-monitor",
        "none",
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
            cmd.args(["-M", "q35", "-cpu", "qemu64"]);
            ("virtio-blk-pci", "virtio-net-pci")
        }
        Arch::Arm64 => {
            cmd.args(["-M", "virt,acpi=off", "-cpu", "cortex-a72"]);
            ("virtio-blk-device", "virtio-net-device")
        }
    };
    cmd.args(["-device", &format!("{net},netdev=n0")]);
    let (second, second_opts, boot_second) = match mode {
        Boot::Install(img) => (Some(*img), ",snapshot=on", true),
        Boot::Prepare => (None, "", false),
        Boot::Run(scratch) => (Some(*scratch), "", false),
    };
    let root_disk = [
        "-drive".to_string(),
        format!("if=none,format=raw,file={},id=d0", disk.display()),
        "-device".to_string(),
        format!(
            "{dev},drive=d0{}",
            if boot_second { "" } else { ",bootindex=0" }
        ),
    ];
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
    if matches!(mode, Boot::Run(_)) {
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

#[cfg(test)]
mod tests;
