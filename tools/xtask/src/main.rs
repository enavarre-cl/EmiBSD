/* <CODE> */
//! `xtask`: host-side developer tooling for EmiBSD.
//!
//! Invoked through the cargo alias in `.cargo/config.toml`:
//!
//! ```text
//! cargo xtask ports check                  validate ports.toml against the tree and the pin
//! cargo xtask ports status [--write]       counts per subsystem; --write regenerates docs/PORTING.md
//! cargo xtask ports next                   `todo` entries whose dependencies are all ported
//! cargo xtask ports drift [--strict|--diff] ported files whose upstream content changed
//! cargo xtask image --arch A --kernel K [--cmdline C] [--init I] [--ramdisk R]
//!                                          build target/emibsd-A.img (Limine + /bsd),
//!                                          C as the kernel command line (boot(8) flags);
//!                                          the init and ramdisk modules default to the
//!                                          built ones (`none` leaves one out)
//! cargo xtask qemu --arch A [--kernel K] [--disk-fresh] [--disks N]
//!                                          boot the image, serial and monitor on stdio;
//!                                          (also smoke and smoke2) the persistent disk
//!                                          target/disk-A[-a|-b].img, 64 MiB, is kept
//!                                          across boots unless --disk-fresh recreates it;
//!                                          --disks N (1..=4, default 1) attaches N such
//!                                          disks, sd0 the file above and sd1..sd3
//!                                          target/disk-A[-a|-b]-sdK.img (each VM of smoke2
//!                                          gets N); a run with fewer disks than the last
//!                                          keeps the extra files, unattached;
//!                                          --disk-set NAME (qemu, smoke) uses the set
//!                                          target/disk-A-NAME[-sdK].img instead;
//!                                          --smp N (1..=8; qemu, smoke, smoke2) gives
//!                                          every VM N processors;
//!                                          --usb, --audio hda|ac97|usb and --expect-tone (qemu,
//!                                          smoke) add M12's devices: a qemu-xhci with a
//!                                          USB stick and keyboard, Intel HDA, AC97 or a
//!                                          usb-audio (M16b) into a WAV file that must hold
//!                                          a tone (devices.rs); --usb-hc ehci|uhci|ohci (M16b) puts
//!                                          the stick on a usb-ehci (alone) or a
//!                                          piix3-usb-uhci instead
//! cargo xtask smoke --arch A [--kernel K] [--cmdline C] [--status N] [--send-after L --send T]... [--until-seen]
//!                   [--expect-ramdisk] --expect L...
//!                                          boot headless; pass if every L appears and QEMU
//!                                          exits with status N (default: the kernel's success
//!                                          status); with K the image is rebuilt first;
//!                                          --expect-ramdisk adds rd(4)'s line for the
//!                                          ramdisk on the image (or its absence)
//!                   [--https-server DIR:PORT:trusted|untrusted|echo]...
//!                                          (smoke and smoke2) TLS test servers on this
//!                                          machine during the run, the guest's
//!                                          emibsd-host:PORT (https.rs)
//! cargo xtask smoke2 --arch A [--kernel K] [--cmdline C] [--timeout SECS] [--show-transcripts]
//!                   [--both-|--a-|--b-send-after L --send T]... [--both-|--a-|--b-expect L]...
//!                                          boot TWO VMs of A at once, each with vio1 on a
//!                                          private link, each running its own script; pass
//!                                          when both saw all they expect (twovm.rs)
//! cargo xtask smoke-all [-j N] [--just PATH] RECIPE...
//!                                          run the justfile's smoke RECIPEs with
//!                                          `just --no-deps`, N (default 4) at a time, each
//!                                          in target/smoke/RECIPE (its images, disks and
//!                                          log); a line per recipe, the failed ones' logs
//!                                          at the end (smokeall.rs)
//! cargo xtask diff-openbsd [--arch A]... [--smp N] [--kernel-dir D] [fetch|install|run]
//!                                          the same scenarios on EmiBSD and on a real OpenBSD
//!                                          VM (the snapshot of openbsd-snapshot.toml,
//!                                          installed under target/openbsd), compared step by
//!                                          step (diffopenbsd.rs)
//! cargo xtask diff-openbsd --arch A [--ipmi] [--usb] [--usb-hc H] [--ukc CMD]... [--sh CMD] probe
//!                                          that OpenBSD alone with the smokes' device
//!                                          options, booted with `-c` and the UKC commands
//!                                          given: its dmesg and CMD's output
//! cargo xtask unsafe-report [--write]    `unsafe` blocks, fns, impls and traits per kernel
//!                                          subsystem, test code apart; --write puts the totals
//!                                          on docs/STATUS.md's `Unsafe` line (unsafereport.rs)
//! cargo xtask symbolize --arch A [--kernel K]
//!                                          annotate the addresses of a stack trace on stdin
//!                                          with K's symbols (default: the debug kernel)
//! cargo xtask userland --arch A            cross-compile OpenBSD's libc, init, ksh, echo and
//!                                          ls from the reference sources (target/userland/A)
//! cargo xtask comp --arch A [--jobs N]     M14: OpenBSD's compiler (clang, lld, libc++) from
//!                                          gnu/llvm by its build glue, into target/comp/A
//!                                          (userland/comp.rs); N jobs, default half the CPUs
//! cargo xtask ntfs-image OUT [--check]     write M10d's NTFS test volume to OUT (ntfsgen.rs);
//!                                          --check mounts it with macOS's NTFS driver
//! cargo xtask e2fsck --arch A [--disk-set NAME] [--cat PATH=TEXT]...
//!                                          check the ext2 file system on partition a of
//!                                          the persistent disk with e2fsprogs' e2fsck -fn;
//!                                          debugfs must cat PATH and print TEXT (e2fs.rs)
//! ```
//!
//! Paths are resolved from the workspace root (derived from `CARGO_MANIFEST_DIR`), never from the
//! current directory. The files a boot writes (the image, the EDK2 variable store, the
//! persistent disks; `target/...` above) go to `$EMIBSD_RUN_DIR` instead of `target/` when it
//! is set (`smoke-all` does, per recipe); `$EMIBSD_TIMEOUT_SCALE` (1 to 10) multiplies the
//! time limits of `smoke` and `smoke2`.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use serde::Deserialize;

mod boot;
mod bsdmake;
mod devices;
mod diffopenbsd;
mod e2fs;
mod efiboot;
mod https;
mod hwopts;
mod install;
mod layout;
mod ntfsgen;
mod rdsetroot;
mod smokeall;
mod symbolize;
mod syscalls;
mod twovm;
mod unsafereport;
mod userland;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Placeholder hash used before the reference tree has been cloned and pinned.
const UNPINNED: &str = "UNPINNED";
const PORTS_FILE: &str = "ports.toml";
const PINNED_FILE: &str = "reference/PINNED.md";
const REFERENCE_DIR: &str = "reference/openbsd-src";
const PORTING_DOC: &str = "docs/PORTING.md";
const TABLE_BEGIN: &str = "<!-- ports:begin -->";
const TABLE_END: &str = "<!-- ports:end -->";

const USAGE: &str = "usage: cargo xtask <ports check | ports status [--write] | ports next | \
                     ports drift [--strict] [--diff] | image --arch A --kernel K [--cmdline C] [--init I] [--ramdisk R] | \
                     qemu --arch A [--kernel K] [--init I] [--ramdisk R] [--disk-fresh] [--disks N] [--disk-set NAME] [--nvme FILE] [--ahci FILE] [--scsi-cd ISO] [--lsi FILE [--lsi-cd ISO]] [--pci-serial FILE] [--pci-bridges] [--machine pc] [--ipmi] [--reboot] [--vio-mq] [--fb] | gen-syscalls [--check] | \
                     smoke --arch A [--kernel K] [--cmdline C] [--init I] [--ramdisk R] [--expect-ramdisk] [--disk-fresh] [--disks N] [--disk-set NAME] [--nvme FILE] [--ahci FILE] [--scsi-cd ISO] [--lsi FILE [--lsi-cd ISO]] [--pci-serial FILE] [--pci-bridges] [--machine pc] [--ipmi] [--reboot] [--vio-mq] [--expect-pci-serial T]... [--fb] [--screenshot-after L [--screen-text ROW:COL:TEXT]] [--sendkey-after L --sendkeys K] [--usb] [--usb-hc xhci|ehci|uhci|ohci] [--usb-mouse] [--usb-tablet] [--usb-wacom-tablet] [--usb-ccid] [--audio hda|ac97|usb] [--expect-tone] [--status N] [--send-after L --send T]... [--until-seen] [--https-server DIR:PORT:MODE]... [--reject L]... --expect L... | \
                     smoke2 --arch A [--kernel K] [--cmdline C] [--timeout S] [--show-transcripts] [--disk-fresh] [--disks N] [--both-|--a-|--b-send-after L --send T]... [--both-|--a-|--b-expect L]... [--reject L]... [--https-server DIR:PORT:MODE]... | \
                     smoke-all [-j N] [--just PATH] RECIPE... | \
                     unsafe-report [--write] | \
                     diff-openbsd [--arch A]... [--smp N] [--kernel-dir D] [fetch | install | run | powerbtn] | \
                     diff-openbsd --arch A [--ipmi] [--usb] [--usb-hc xhci|ehci|uhci|ohci] [--ukc CMD]... [--sh CMD] probe | \
                     symbolize --arch A [--kernel K] | userland --arch A | comp --arch A [--jobs N] | ntfs-image OUT [--check] | \
                     e2fsck --arch A [--disk-set NAME] [--cat PATH=TEXT]... | \
                     nvme-root --arch A [--duid HEX] [--out FILE] [--root-dev DEV]>";

#[derive(Deserialize)]
struct Ports {
    meta: Meta,
    #[serde(default, rename = "file")]
    files: Vec<Entry>,
    #[serde(default, rename = "extra")]
    extras: Vec<Extra>,
}

#[derive(Deserialize)]
struct Meta {
    upstream: String,
    pinned: String,
}

#[derive(Deserialize)]
struct Entry {
    c: String,
    #[serde(default)]
    rust: String,
    status: Status,
    #[serde(default)]
    upstream_commit: String,
    #[serde(default)]
    upstream_blob: String,
    /// `"none"`: the C file carries no licence text, so its port has no `<LICENSES>` zone.
    #[serde(default)]
    license: String,
    #[serde(default)]
    deps: Vec<String>,
    #[serde(default)]
    notes: String,
}

/// A Rust file under `sys/` that is not the port of any C file (project helpers).
#[derive(Deserialize)]
struct Extra {
    rust: String,
    reason: String,
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[serde(rename_all = "lowercase")]
enum Status {
    Todo,
    Wip,
    Ported,
    Skipped,
}

impl Status {
    const ALL: [Status; 4] = [Status::Todo, Status::Wip, Status::Ported, Status::Skipped];

    fn as_str(self) -> &'static str {
        match self {
            Status::Todo => "todo",
            Status::Wip => "wip",
            Status::Ported => "ported",
            Status::Skipped => "skipped",
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xtask: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<()> {
    let root = workspace_root()?;
    let argv: Vec<&str> = args.iter().map(String::as_str).collect();
    boot::set_smp(smp_flag(&argv)?);
    hwopts::set(&root, &argv)?;
    devices::set_from_args(&argv)?;
    match argv.as_slice() {
        ["ports", "check"] => ports_check(&root),
        ["ports", "status"] => ports_status(&root, false),
        ["ports", "status", "--write"] => ports_status(&root, true),
        ["ports", "next"] => ports_next(&root),
        ["ports", "drift", flags @ ..] => ports_drift(
            &root,
            flags.contains(&"--strict"),
            flags.contains(&"--diff"),
        ),
        ["image", rest @ ..] => {
            let arch = boot::Arch::parse(flag(rest, "--arch")?)?;
            let kernel = PathBuf::from(flag(rest, "--kernel")?);
            let init = init_flag(&root, arch, rest);
            let ramdisk = ramdisk_flag(&root, arch, rest);
            boot::image(
                &root,
                arch,
                &kernel,
                optional_flag(rest, "--cmdline"),
                init.as_deref(),
                ramdisk.as_deref(),
            )
            .map(|_| ())
        }
        ["qemu", rest @ ..] => {
            let arch = boot::Arch::parse(flag(rest, "--arch")?)?;
            let kernel = optional_flag(rest, "--kernel").map(PathBuf::from);
            let init = init_flag(&root, arch, rest);
            let ramdisk = ramdisk_flag(&root, arch, rest);
            boot::qemu(
                &root,
                arch,
                kernel.as_deref(),
                init.as_deref(),
                ramdisk.as_deref(),
                &boot::Disks {
                    fresh: rest.contains(&"--disk-fresh"),
                    count: disks_flag(rest)?,
                    set: optional_flag(rest, "--disk-set"),
                },
            )
        }
        ["smoke", rest @ ..] => {
            let arch = boot::Arch::parse(flag(rest, "--arch")?)?;
            let kernel = optional_flag(rest, "--kernel").map(PathBuf::from);
            let expects = flags(rest, "--expect");
            if expects.is_empty() {
                return Err(format!("missing `--expect <line>`\n{USAGE}").into());
            }
            let status = match optional_flag(rest, "--status") {
                Some(s) => s.parse::<i32>().map_err(|e| format!("--status {s}: {e}"))?,
                None => boot::QEMU_SUCCESS_STATUS,
            };
            // `--send-after A --send T`, repeatable: each text is sent once its trigger line
            // has been seen, in order.
            let afters = flags(rest, "--send-after");
            let texts = flags(rest, "--send");
            if afters.len() != texts.len() {
                return Err(format!("--send-after and --send go together\n{USAGE}").into());
            }
            let sends: Vec<(&str, String)> = afters
                .iter()
                .zip(&texts)
                .map(|(a, t)| (*a, t.replace("\\n", "\n")))
                .collect();
            let init = init_flag(&root, arch, rest);
            let ramdisk = ramdisk_flag(&root, arch, rest);
            // Killed when dropped, after the run.
            let _servers = https::start(&root, &flags(rest, "--https-server"))?;
            boot::smoke(
                &root,
                arch,
                &boot::SmokeOptions {
                    kernel: kernel.as_deref(),
                    cmdline: optional_flag(rest, "--cmdline"),
                    expects: &expects,
                    rejects: &flags(rest, "--reject"),
                    status,
                    sends: &sends
                        .iter()
                        .map(|(a, t)| (*a, t.as_str()))
                        .collect::<Vec<_>>(),
                    until_seen: rest.contains(&"--until-seen"),
                    init: init.as_deref(),
                    ramdisk: ramdisk.as_deref(),
                    expect_ramdisk: rest.contains(&"--expect-ramdisk"),
                    disks: boot::Disks {
                        fresh: rest.contains(&"--disk-fresh"),
                        count: disks_flag(rest)?,
                        set: optional_flag(rest, "--disk-set"),
                    },
                },
            )
        }
        ["smoke2", rest @ ..] => {
            let arch = boot::Arch::parse(flag(rest, "--arch")?)?;
            let kernel = optional_flag(rest, "--kernel").map(PathBuf::from);
            let init = init_flag(&root, arch, rest);
            let ramdisk = ramdisk_flag(&root, arch, rest);
            let plan = twovm::parse_plan(rest)?;
            let _servers = https::start(&root, &flags(rest, "--https-server"))?;
            twovm::smoke2(
                &root,
                arch,
                kernel.as_deref(),
                optional_flag(rest, "--cmdline"),
                init.as_deref(),
                ramdisk.as_deref(),
                plan,
            )
        }
        ["smoke-all", rest @ ..] => {
            let a = smokeall::parse_args(rest)?;
            smokeall::smoke_all(&root, a.jobs, a.just, &a.recipes)
        }
        ["diff-openbsd", rest @ ..] => diffopenbsd::diff_openbsd(&root, rest),
        ["unsafe-report"] => unsafereport::unsafe_report(&root, false),
        ["unsafe-report", "--write"] => unsafereport::unsafe_report(&root, true),
        ["gen-syscalls"] => syscalls::gen_syscalls(&root, false),
        ["gen-syscalls", "--check"] => syscalls::gen_syscalls(&root, true),
        ["symbolize", rest @ ..] => {
            let arch = boot::Arch::parse(flag(rest, "--arch")?)?;
            let kernel = match optional_flag(rest, "--kernel") {
                Some(k) => PathBuf::from(k),
                None => root
                    .join("target")
                    .join(arch.target())
                    .join("debug")
                    .join("bsd"),
            };
            symbolize::symbolize(&kernel)
        }
        ["userland", rest @ ..] => {
            let arch = boot::Arch::parse(flag(rest, "--arch")?)?;
            userland::userland(&root, arch)
        }
        ["comp", rest @ ..] => {
            let arch = boot::Arch::parse(flag(rest, "--arch")?)?;
            // Half the CPUs by default: other builds may share the machine.
            let jobs = match optional_flag(rest, "--jobs") {
                Some(n) => n
                    .parse::<usize>()
                    .ok()
                    .filter(|n| *n > 0)
                    .ok_or_else(|| format!("--jobs {n}: not a positive number"))?,
                None => std::thread::available_parallelism()
                    .map_or(4, |n| n.get())
                    .div_ceil(2),
            };
            userland::comp::comp(&root, arch, jobs)
        }
        // M14c: OpenBSD's rdsetroot(8) for our kernel ELF (rdsetroot.rs).
        ["rdsetroot", rest @ ..] => rdsetroot::rdsetroot(rest),
        ["miniroot", rest @ ..] => install::miniroot(&root, rest),
        ["sets", rest @ ..] => install::sets(&root, rest),
        ["install-media", rest @ ..] => install::install_media(&root, rest),
        ["install", rest @ ..] => install::install(&root, rest),
        ["install-boot", rest @ ..] => install::install_boot(&root, rest),
        ["ntfs-image", out] => ntfsgen::ntfs_image(&root.join(out), false),
        ["ntfs-image", out, "--check"] => ntfsgen::ntfs_image(&root.join(out), true),
        // M14: efiboot's PE image and the disk smoke-efiboot boots (efiboot.rs).
        ["efiboot", rest @ ..] => {
            let arch = boot::Arch::parse(flag(rest, "--arch")?)?;
            efiboot::efiboot(
                &root,
                arch,
                Path::new(flag(rest, "--elf")?),
                optional_flag(rest, "--out"),
            )
        }
        ["efiboot-disk", rest @ ..] => {
            let arch = boot::Arch::parse(flag(rest, "--arch")?)?;
            efiboot::efiboot_disk(
                &root,
                arch,
                Path::new(flag(rest, "--efi")?),
                Path::new(flag(rest, "--kernel")?),
                optional_flag(rest, "--root-dev"),
            )
        }
        ["nvme-root", rest @ ..] => {
            let arch = boot::Arch::parse(flag(rest, "--arch")?)?;
            hwopts::nvme_root(
                &root,
                arch,
                optional_flag(rest, "--duid"),
                optional_flag(rest, "--out"),
                optional_flag(rest, "--root-dev"),
            )
        }
        ["e2fsck", rest @ ..] => {
            let arch = boot::Arch::parse(flag(rest, "--arch")?)?;
            e2fs::e2fsck(
                &root,
                arch,
                optional_flag(rest, "--disk-set"),
                &flags(rest, "--cat"),
            )
        }
        _ => Err(USAGE.into()),
    }
}

/// `--init <path>`: the init module to put on the image; `--init none` leaves it out; absent,
/// the one `just build-init-*` built, if any.
fn init_flag(root: &Path, arch: boot::Arch, args: &[&str]) -> Option<PathBuf> {
    match optional_flag(args, "--init") {
        Some("none") => None,
        Some(p) => Some(PathBuf::from(p)),
        None => boot::default_init(root, arch),
    }
}

/// `--ramdisk <path>`: the ramdisk module to put on the image; `--ramdisk none` leaves it
/// out; absent, the one `just userland` built, if any.
fn ramdisk_flag(root: &Path, arch: boot::Arch, args: &[&str]) -> Option<PathBuf> {
    match optional_flag(args, "--ramdisk") {
        Some("none") => None,
        Some(p) => Some(PathBuf::from(p)),
        None => boot::default_ramdisk(root, arch),
    }
}

/// The value following `name` in `args`, if present.
fn optional_flag<'a>(args: &[&'a str], name: &str) -> Option<&'a str> {
    args.windows(2).find(|w| w[0] == name).map(|w| w[1])
}

/// `--disks N`: how many persistent disks a VM gets, 1 (the default) to `boot::MAX_DISKS`.
fn disks_flag(args: &[&str]) -> Result<usize> {
    let Some(s) = optional_flag(args, "--disks") else {
        return Ok(1);
    };
    match s.parse::<usize>() {
        Ok(n) if (1..=boot::MAX_DISKS).contains(&n) => Ok(n),
        _ => Err(format!(
            "--disks {s}: expected a number from 1 to {}",
            boot::MAX_DISKS
        )
        .into()),
    }
}

/// `--smp N` (qemu, smoke, smoke2): QEMU's `-smp N`, 1 to `boot::MAX_SMP`; absent, QEMU's
/// default of one processor.
fn smp_flag(args: &[&str]) -> Result<Option<u32>> {
    let Some(s) = optional_flag(args, "--smp") else {
        return Ok(None);
    };
    match s.parse::<u32>() {
        Ok(n) if (1..=boot::MAX_SMP).contains(&n) => Ok(Some(n)),
        _ => Err(format!("--smp {s}: expected a number from 1 to {}", boot::MAX_SMP).into()),
    }
}

/// Every value following an occurrence of `name` in `args`.
fn flags<'a>(args: &[&'a str], name: &str) -> Vec<&'a str> {
    args.windows(2)
        .filter(|w| w[0] == name)
        .map(|w| w[1])
        .collect()
}

/// The value following `name` in `args`; an error naming the flag otherwise.
fn flag<'a>(args: &[&'a str], name: &str) -> Result<&'a str> {
    optional_flag(args, name).ok_or_else(|| format!("missing `{name} <value>`\n{USAGE}").into())
}

/// `tools/xtask` → workspace root. Compile-time manifest dir, so the cwd never matters.
fn workspace_root() -> Result<PathBuf> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest
        .parent()
        .and_then(Path::parent)
        .ok_or("cannot locate the workspace root from CARGO_MANIFEST_DIR")?;
    Ok(root.to_path_buf())
}

fn load_ports(root: &Path) -> Result<Ports> {
    let path = root.join(PORTS_FILE);
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let ports: Ports = toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(ports)
}

/// The `Commit:` line of `reference/PINNED.md`.
fn pinned_commit(root: &Path) -> Result<String> {
    let path = root.join(PINNED_FILE);
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    text.lines()
        .find_map(|l| l.strip_prefix("Commit:"))
        .map(|s| s.trim().to_string())
        .ok_or_else(|| format!("{}: no `Commit:` line", path.display()).into())
}

fn short(hash: &str) -> &str {
    hash.get(..12).unwrap_or(hash)
}

fn is_rust_path_under_sys(p: &str) -> bool {
    p.starts_with("sys/") && p.ends_with(".rs")
}

/// Files that are structure, not ports: never need a `ports.toml` entry.
fn is_structural(rel: &str) -> bool {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    matches!(name, "mod.rs" | "lib.rs" | "main.rs" | "build.rs")
        || rel.starts_with("sys/machine/")
        || rel.starts_with("sys/arch/host/")
        || rel.starts_with("sys/stand/")
}

pub(crate) fn walk_rs(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let path = entry?.path();
        if path.is_dir() {
            if path.file_name().and_then(|n| n.to_str()) == Some("target") {
                continue;
            }
            walk_rs(&path, out)?;
        } else if path.extension().and_then(|x| x.to_str()) == Some("rs") {
            out.push(path);
        }
    }
    Ok(())
}

/// Second-level grouping used by `ports status`: `sys/kern/x.c` → `kern`,
/// `sys/lib/libkern/x.c` → `lib/libkern`, `sys/arch/amd64/...` → `arch/amd64`.
fn subsystem(c: &str) -> String {
    let parts: Vec<&str> = c.strip_prefix("sys/").unwrap_or(c).split('/').collect();
    match parts.as_slice() {
        [first, second, _, ..] if *first == "lib" || *first == "arch" => {
            format!("{first}/{second}")
        }
        [first, _, ..] => (*first).to_string(),
        _ => "(root)".to_string(),
    }
}

fn ports_check(root: &Path) -> Result<()> {
    let ports = load_ports(root)?;
    let pinned = pinned_commit(root)?;
    let reference = root.join(REFERENCE_DIR);
    let have_reference = reference.join("sys").is_dir();
    let mut errors: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    if ports.meta.pinned == UNPINNED || pinned == UNPINNED {
        warnings.push(
            "reference not pinned yet: clone it (reference/README.md), then set `Commit:` in \
             reference/PINNED.md and [meta].pinned in ports.toml"
                .to_string(),
        );
    }
    if ports.meta.pinned != pinned {
        let meta_pinned = &ports.meta.pinned;
        errors.push(format!(
            "[meta].pinned ({meta_pinned}) != reference/PINNED.md Commit: ({pinned})"
        ));
    }
    if !have_reference {
        warnings.push(format!(
            "{REFERENCE_DIR} not present; C paths were not verified"
        ));
    }

    let mut seen_c: HashSet<&str> = HashSet::new();
    for e in &ports.files {
        let c = &e.c;
        let tag = format!("[[file]] c = \"{c}\"");
        if !seen_c.insert(c.as_str()) {
            errors.push(format!("{tag}: duplicate entry"));
        }
        if !c.starts_with("sys/") {
            errors.push(format!("{tag}: `c` must start with sys/"));
        }
        if have_reference && !reference.join(c).is_file() {
            errors.push(format!("{tag}: not found in {REFERENCE_DIR}"));
        }
        let rust_required = e.status != Status::Skipped;
        if (rust_required || !e.rust.is_empty()) && !is_rust_path_under_sys(&e.rust) {
            errors.push(format!("{tag}: `rust` must be a .rs path under sys/"));
        }
        match e.status {
            Status::Todo => {}
            Status::Wip | Status::Ported => {
                let status = e.status.as_str();
                let rust = &e.rust;
                match fs::read_to_string(root.join(rust)) {
                    Ok(src) if !src.contains("Upstream:") => {
                        errors.push(format!("{tag}: {rust} lacks an `Upstream:` line"));
                    }
                    Ok(_) => {}
                    Err(_) => {
                        errors.push(format!("{tag}: status {status} but {rust} does not exist"))
                    }
                }
                if e.upstream_commit.is_empty() {
                    errors.push(format!("{tag}: upstream_commit is required for {status}"));
                }
                if e.status == Status::Ported && e.upstream_blob.is_empty() {
                    errors.push(format!("{tag}: upstream_blob is required for ported"));
                }
            }
            Status::Skipped => {
                if e.notes.trim().is_empty() {
                    errors.push(format!("{tag}: skipped entries need `notes` (the reason)"));
                }
            }
        }
        for d in &e.deps {
            if !ports.files.iter().any(|o| &o.c == d) {
                errors.push(format!("{tag}: dep {d} has no entry"));
            }
        }
    }

    // Every non-structural Rust file under sys/ is a tracked port or a declared extra.
    let tracked: HashSet<&str> = ports
        .files
        .iter()
        .map(|e| e.rust.as_str())
        .chain(ports.extras.iter().map(|x| x.rust.as_str()))
        .collect();
    let mut rs_files = Vec::new();
    walk_rs(&root.join("sys"), &mut rs_files)?;
    for f in &rs_files {
        let rel = f
            .strip_prefix(root)
            .unwrap_or(f)
            .to_string_lossy()
            .replace('\\', "/");
        if is_structural(&rel) || tracked.contains(rel.as_str()) {
            continue;
        }
        errors.push(format!(
            "{rel}: not tracked in ports.toml (add a [[file]] entry, or an [[extra]] with a reason)"
        ));
    }
    check_layout(root, &ports, &mut errors)?;
    for x in &ports.extras {
        let rust = &x.rust;
        if !root.join(rust).is_file() {
            errors.push(format!("[[extra]] rust = \"{rust}\": file does not exist"));
        }
        if x.reason.trim().is_empty() {
            errors.push(format!("[[extra]] rust = \"{rust}\": `reason` is required"));
        }
    }

    for w in &warnings {
        println!("warning: {w}");
    }
    for e in &errors {
        println!("error: {e}");
    }
    let (nfiles, nextras, nerr, nwarn) = (
        ports.files.len(),
        ports.extras.len(),
        errors.len(),
        warnings.len(),
    );
    println!(
        "ports check: {nfiles} entries, {nextras} extras, {nerr} error(s), {nwarn} warning(s)"
    );
    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!("{nerr} error(s) in {PORTS_FILE}").into())
    }
}

/// The zone markers of every `.rs` under `sys/` and `tools/` (layout.rs), with the licence
/// policy of `ports.toml`: a file that is the port of a C file keeps its notice in a
/// `<LICENSES>` zone (unless its entries say `license = "none"`), any other file has none.
fn check_layout(root: &Path, ports: &Ports, errors: &mut Vec<String>) -> Result<()> {
    use layout::Licenses;

    let mut policy: HashMap<&str, Licenses> = HashMap::new();
    let mut none: HashSet<&str> = HashSet::new();
    for e in &ports.files {
        if e.rust.is_empty() {
            continue;
        }
        if e.license == "none" {
            none.insert(e.rust.as_str());
        }
        if !matches!(e.license.as_str(), "" | "none") {
            let c = &e.c;
            errors.push(format!(
                "[[file]] c = \"{c}\": license must be \"none\" or absent, not \"{}\"",
                e.license
            ));
        }
        let wants = e.status != Status::Todo && e.status != Status::Skipped && e.license != "none";
        let slot = policy.entry(e.rust.as_str()).or_insert(Licenses::Allowed);
        if wants {
            *slot = Licenses::Required;
        }
    }
    let mut files = Vec::new();
    for tree in ["sys", "tools"] {
        walk_rs(&root.join(tree), &mut files)?;
    }
    files.sort();
    for f in &files {
        let rel = f
            .strip_prefix(root)
            .unwrap_or(f)
            .to_string_lossy()
            .replace('\\', "/");
        if rel.ends_with("/tests.rs") {
            errors.push(format!(
                "{rel}: tests live inline in the file of the module they test (`mod tests` in <TESTS>)"
            ));
            continue;
        }
        let lic = policy
            .get(rel.as_str())
            .copied()
            .unwrap_or(Licenses::Forbidden);
        let src = fs::read_to_string(f).map_err(|e| format!("{rel}: {e}"))?;
        let found = layout::check(&rel, &src, lic);
        if none.contains(rel.as_str()) && src.lines().any(|l| l == "/* <LICENSES> */") {
            errors.push(format!(
                "{rel}: has a <LICENSES> zone but its ports.toml entry says license = \"none\""
            ));
        }
        errors.extend(found);
    }
    Ok(())
}

fn ports_status(root: &Path, write: bool) -> Result<()> {
    let ports = load_ports(root)?;
    let mut table: BTreeMap<String, BTreeMap<Status, usize>> = BTreeMap::new();
    for e in &ports.files {
        *table
            .entry(subsystem(&e.c))
            .or_default()
            .entry(e.status)
            .or_insert(0) += 1;
    }

    let mut md = String::new();
    md.push_str("| Subsystem | todo | wip | ported | skipped | total |\n");
    md.push_str("|---|---:|---:|---:|---:|---:|\n");
    let mut totals: BTreeMap<Status, usize> = BTreeMap::new();
    for (sub, counts) in &table {
        let mut row = format!("| {sub} |");
        for s in Status::ALL {
            let n = counts.get(&s).copied().unwrap_or(0);
            *totals.entry(s).or_insert(0) += n;
            row.push_str(&format!(" {n} |"));
        }
        let total: usize = counts.values().sum();
        row.push_str(&format!(" {total} |\n"));
        md.push_str(&row);
    }
    let mut row = String::from("| **total** |");
    for s in Status::ALL {
        let n = totals.get(&s).copied().unwrap_or(0);
        row.push_str(&format!(" {n} |"));
    }
    let grand = ports.files.len();
    row.push_str(&format!(" {grand} |\n"));
    md.push_str(&row);

    let upstream = &ports.meta.upstream;
    let pin = short(&ports.meta.pinned);
    println!("upstream: {upstream} @ {pin}");
    print!("{md}");

    if write {
        let path = root.join(PORTING_DOC);
        let doc = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let (Some(b), Some(e)) = (doc.find(TABLE_BEGIN), doc.find(TABLE_END)) else {
            return Err(
                format!("{PORTING_DOC}: markers {TABLE_BEGIN} / {TABLE_END} not found").into(),
            );
        };
        if e < b {
            return Err(format!("{PORTING_DOC}: {TABLE_END} appears before {TABLE_BEGIN}").into());
        }
        let head = &doc[..b];
        let tail = &doc[e..];
        let new = format!(
            "{head}{TABLE_BEGIN}\n_Generated by `cargo xtask ports status --write` against pin {pin}._\n\n{md}{tail}"
        );
        fs::write(&path, new).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("wrote {PORTING_DOC}");
    }
    Ok(())
}

fn ports_next(root: &Path) -> Result<()> {
    let ports = load_ports(root)?;
    let status: HashMap<&str, Status> = ports
        .files
        .iter()
        .map(|e| (e.c.as_str(), e.status))
        .collect();
    let satisfied = |dep: &str| {
        matches!(
            status.get(dep),
            Some(Status::Ported) | Some(Status::Skipped)
        )
    };
    let mut any = false;
    for e in ports.files.iter().filter(|e| e.status == Status::Todo) {
        if e.deps.iter().all(|d| satisfied(d)) {
            let (c, rust) = (&e.c, &e.rust);
            println!("{c}  ->  {rust}");
            any = true;
        }
    }
    if !any {
        println!("nothing unblocked: no `todo` entry has all its dependencies ported or skipped");
    }
    Ok(())
}

fn ports_drift(root: &Path, strict: bool, diff: bool) -> Result<()> {
    let ports = load_ports(root)?;
    let reference = root.join(REFERENCE_DIR);
    if !reference.join("sys").is_dir() {
        return Err(format!("{REFERENCE_DIR} not present; see reference/README.md").into());
    }
    let mut checked = 0usize;
    let mut drifted = 0usize;
    for e in ports
        .files
        .iter()
        .filter(|e| matches!(e.status, Status::Ported | Status::Wip) && !e.upstream_blob.is_empty())
    {
        checked += 1;
        let c = &e.c;
        let now = git(&reference, &["rev-parse", &format!("HEAD:{c}")])?;
        if now == e.upstream_blob {
            continue;
        }
        drifted += 1;
        let (old, new) = (short(&e.upstream_blob), short(&now));
        println!("DRIFT {c}  {old}..{new}");
        if diff {
            let commit = &e.upstream_commit;
            match git(&reference, &["diff", commit, "HEAD", "--", c]) {
                Ok(d) => println!("{d}"),
                Err(err) => println!(
                    "  (diff unavailable: {err}; fetch the old commit first: \
                     git -C {REFERENCE_DIR} fetch --depth 1 origin {commit})"
                ),
            }
        }
    }
    let pin = short(&ports.meta.pinned);
    println!("ports drift: {checked} checked, {drifted} drifted (pin {pin})");
    if strict && drifted > 0 {
        Err(format!("{drifted} ported file(s) drifted upstream").into())
    } else {
        Ok(())
    }
}

fn git(repo: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        let cmd = args.join(" ");
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(format!("git {cmd}: {stderr}").into());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smp_is_optional_and_bounded() {
        assert_eq!(smp_flag(&[]).unwrap(), None);
        assert_eq!(
            smp_flag(&["--arch", "arm64", "--smp", "4"]).unwrap(),
            Some(4)
        );
        for bad in ["0", "9", "x"] {
            assert!(smp_flag(&["--smp", bad]).is_err(), "{bad}");
        }
    }

    #[test]
    fn disks_defaults_to_one_and_stops_at_four() {
        assert_eq!(disks_flag(&[]).unwrap(), 1);
        assert_eq!(disks_flag(&["--disks", "4"]).unwrap(), 4);
        assert_eq!(disks_flag(&["--arch", "amd64", "--disks", "2"]).unwrap(), 2);
        for bad in ["0", "5", "x", "-1"] {
            assert!(disks_flag(&["--disks", bad]).is_err(), "{bad}");
        }
    }

    #[test]
    fn smoke_reads_every_reject_line() {
        let args = [
            "--arch",
            "amd64",
            "--reject",
            "uptime went backwards",
            "--expect",
            "init: uptime monotonic ok",
            "--reject",
            "panic:",
        ];
        assert_eq!(
            flags(&args, "--reject"),
            vec!["uptime went backwards", "panic:"]
        );
        assert_eq!(flags(&args, "--expect"), vec!["init: uptime monotonic ok"]);
        assert!(flags(&["--arch", "amd64"], "--reject").is_empty());
    }
}
/* </TESTS> */
