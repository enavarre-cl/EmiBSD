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
//! `xtask`: host-side developer tooling for EmiBSD.
//!
//! Invoked through the cargo alias in `.cargo/config.toml`:
//!
//! ```text
//! cargo xtask lz check                     validate lineage.toml against the tree and the LZ pin
//! cargo xtask lz status [--write]          inherited/redesigned modules per subsystem; --write
//!                                          regenerates the tables of README.md and docs/STATUS.md
//! cargo xtask lz drift [--fetch] [--security] [--functions] [--strict]
//!                                          LZ commits after the pin without an lz-sync.toml record
//! cargo xtask lz trace <lz path>:<item> | --rust <path>:<item> | --c <c path>:<item>
//!                                          where an item's code is now, or came from (lz.rs)
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
//! cargo xtask diff-openbsd --arch A [--ipmi] [--nic MODEL] [--usb] [--usb-hc H] [--ukc CMD]... [--sh CMD] probe
//!                                          that OpenBSD alone with the smokes' device
//!                                          options, booted with `-c` and the UKC commands
//!                                          given: its dmesg and CMD's output
//! cargo xtask unsafe-report [--write|--check]
//!                                          `unsafe` blocks, fns, impls and traits per kernel
//!                                          subsystem, test code apart; --write puts the totals
//!                                          on docs/STATUS.md's `Unsafe` line and lowers
//!                                          unsafe-budget.toml; --check fails when a subsystem
//!                                          exceeds its budget (unsafereport.rs)
//! cargo xtask symbolize --arch A [--kernel K]
//!                                          annotate the addresses of a stack trace on stdin
//!                                          with K's symbols (default: the debug kernel)
//! cargo xtask userland --arch A            cross-compile OpenBSD's libc, init, ksh, echo and
//!                                          ls from the reference sources (target/userland/A)
//! cargo xtask comp --arch A [--jobs N]     M14: OpenBSD's compiler (clang, lld, libc++) from
//!                                          gnu/llvm by its build glue, into target/comp/A
//!                                          (userland/comp.rs); N jobs, default half the CPUs
//! cargo xtask install-img --arch A         M16g: install80.img, the install media with the
//!                                          sets on one disk image (distrib.rs)
//! cargo xtask cd-iso --arch amd64          M16g: cd80.iso, bsd.rd on an El Torito CD
//!                                          (distrib.rs, iso9660.rs)
//! cargo xtask install80|cd80 --arch A      M16g: install from install80.img as a USB stick
//!                                          and boot the result; boot cd80.iso to the
//!                                          installer (distrib.rs)
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
//! time limits of `smoke` and `smoke2`. While a boot waits it prints a line a minute
//! (`boot::Heartbeat`), which `smoke-all`'s watchdog relies on (`smokeall.rs`).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

mod boot;
mod bsdmake;
mod devices;
mod diffopenbsd;
mod distrib;
mod e2fs;
mod efiboot;
mod https;
mod hwopts;
mod install;
mod iso9660;
mod layout;
mod lz;
mod ntfsgen;
mod rdsetroot;
mod smokeall;
mod storage;
mod symbolize;
mod syscalls;
mod twovm;
mod unsafereport;
mod userland;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Placeholder hash used before the reference tree has been cloned and pinned.
const REFERENCE_DIR: &str = "reference/openbsd-src";

const USAGE: &str = "usage: cargo xtask <lz check | lz status [--write] | lz drift [--fetch] [--security] [--functions] [--strict] | \
                     lz trace <lz path>:<item> | lz trace --rust <path>:<item> | lz trace --c <c path>:<item> | image --arch A --kernel K [--cmdline C] [--init I] [--ramdisk R] | \
                     qemu --arch A [--kernel K] [--init I] [--ramdisk R] [--disk-fresh] [--disks N] [--disk-set NAME] [--nvme FILE] [--ahci FILE] [--scsi-cd ISO] [--lsi FILE [--lsi-cd ISO]] [--pci-serial FILE] [--pci-bridges] [--machine pc] [--ide|--megasas|--megasas-gen2|--mptsas|--pvscsi|--am53c974|--dc390|--ufs|--sdhci|--floppy FILE]... [--ipmi] [--reboot] [--vio-mq] [--fb] | gen-syscalls [--check] | \
                     smoke --arch A [--kernel K] [--cmdline C] [--init I] [--ramdisk R] [--expect-ramdisk] [--disk-fresh] [--disks N] [--disk-set NAME] [--nvme FILE] [--ahci FILE] [--scsi-cd ISO] [--lsi FILE [--lsi-cd ISO]] [--pci-serial FILE] [--pci-bridges] [--machine pc] [--ide|--megasas|--megasas-gen2|--mptsas|--pvscsi|--am53c974|--dc390|--ufs|--sdhci|--floppy FILE]... [--ipmi] [--reboot] [--vio-mq] [--expect-pci-serial T]... [--fb] [--screenshot-after L [--screen-text ROW:COL:TEXT]] [--sendkey-after L --sendkeys K] [--usb] [--usb-hc xhci|ehci|uhci|ohci] [--usb-mouse] [--usb-tablet] [--usb-wacom-tablet] [--usb-ccid] [--usb-net] [--usb-serial FILE [--usb-serial-send-after L --usb-serial-send T]... [--expect-usb-serial T]...] [--audio hda|ac97|usb] [--expect-tone] [--status N] [--send-after L --send T]... [--until-seen] [--https-server DIR:PORT:MODE]... [--reject L]... --expect L... | \
                     smoke2 --arch A [--kernel K] [--cmdline C] [--timeout S] [--show-transcripts] [--disk-fresh] [--disks N] [--both-|--a-|--b-send-after L --send T]... [--both-|--a-|--b-expect L]... [--reject L]... [--https-server DIR:PORT:MODE]... | \
                     smoke-all [-j N] [--just PATH] RECIPE... | \
                     unsafe-report [--write] [--check] | \
                     diff-openbsd [--arch A]... [--smp N] [--kernel-dir D] [fetch | install | run | powerbtn] | \
                     diff-openbsd --arch A [--ipmi] [--nic MODEL] [--usb] [--usb-hc xhci|ehci|uhci|ohci] [--ukc CMD]... [--sh CMD] probe | \
                     symbolize --arch A [--kernel K] | userland --arch A | comp --arch A [--jobs N] | ntfs-image OUT [--check] | \
                     e2fsck --arch A [--disk-set NAME] [--cat PATH=TEXT]... | \
                     nvme-root --arch A [--duid HEX] [--out FILE] [--root-dev DEV]>";

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
    storage::set(&root, &argv)?;
    devices::set_from_args(&argv)?;
    match argv.as_slice() {
        ["lz", "check"] => lz::check(&root),
        ["lz", "status"] => lz::status(&root, false),
        ["lz", "status", "--write"] => lz::status(&root, true),
        ["lz", "drift", flags @ ..] => lz::drift(
            &root,
            flags.contains(&"--fetch"),
            flags.contains(&"--security"),
            flags.contains(&"--functions"),
            flags.contains(&"--strict"),
        ),
        ["lz", "trace", rest @ ..] => lz::trace(&root, rest),
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
        ["unsafe-report", flags @ ..] => unsafereport::unsafe_report(
            &root,
            flags.contains(&"--write"),
            flags.contains(&"--check"),
        ),
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
        // M16g: the release images and their smokes (distrib.rs, iso9660.rs).
        ["install-img", rest @ ..] => distrib::install_img(&root, rest),
        ["cd-iso", rest @ ..] => distrib::cd_iso(&root, rest),
        ["install80", rest @ ..] => distrib::install80(&root, rest),
        ["cd80", rest @ ..] => distrib::cd80(&root, rest),
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

fn short(hash: &str) -> &str {
    hash.get(..12).unwrap_or(hash)
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
