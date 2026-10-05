//! M14c: the install media and the install run (`cargo xtask miniroot`, `sets`,
//! `install-media`, `install`, `install-boot`). See docs/ARCHITECTURE.md, "The install media".
//!
//! `install-media` makes what OpenBSD's release makes: the install ramdisk (`miniroot.rs`),
//! `bsd.rd` (the kernel built with feature `miniroot` and the ramdisk put in with
//! `rdsetroot`), and the sets with their signed `SHA256` (`sets.rs`).
//!
//! `install` then runs OpenBSD's installer, unmodified, in QEMU on a fresh disk: it boots a
//! `bsd.rd` whose ramdisk holds `/auto_install.conf` (autoinstall(8)'s response file; a
//! response file in `/` is how OpenBSD itself starts an unattended install from the ramdisk
//! when there is no DHCP server to name one: `.profile` finds it and starts `autoinstall`
//! after five seconds), serves the sets over HTTP from this machine (QEMU's user network
//! reaches it as `10.0.2.2:PORT`; the same server `diff-openbsd` uses), and watches the
//! serial console until the installer says `CONGRATULATIONS!` and reboots. A second boot of
//! the plain `bsd.rd` then mounts the new disk read-only and lists what the installer put on
//! it. `install-boot` boots that disk through the loader the installer put on it (efiboot).

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::Result;
use crate::boot::{self, Arch, Disks};
use crate::diffopenbsd::http::Server;
use crate::diffopenbsd::serial::{Stop, Vm};

/// The size of the disk the installer installs onto (sparse): the EFI system partition, a
/// swap partition and a root for the base and comp sets with room to spare.
const TARGET_DISK_BYTES: u64 = 3 << 30;

/// `target/install/<arch>`: everything the install run uses.
pub(crate) fn install_dir(root: &Path, arch: Arch) -> PathBuf {
    root.join("target").join("install").join(arch.name())
}

/// The value after `name`, if present.
fn opt<'a>(args: &[&'a str], name: &str) -> Option<&'a str> {
    args.windows(2).find(|w| w[0] == name).map(|w| w[1])
}

fn arch_of(args: &[&str]) -> Result<Arch> {
    Arch::parse(opt(args, "--arch").ok_or("missing `--arch A`")?)
}

/// `cargo xtask miniroot --arch A [--pubkey FILE] [--conf FILE] [--out IMG]`: the install
/// ramdisk alone.
pub(crate) fn miniroot(root: &Path, args: &[&str]) -> Result<()> {
    let arch = arch_of(args)?;
    let conf = match opt(args, "--conf") {
        Some(f) => Some(fs::read_to_string(f).map_err(|e| format!("{f}: {e}"))?),
        None => None,
    };
    let out = opt(args, "--out").map_or_else(
        || install_dir(root, arch).join("miniroot.ffs"),
        PathBuf::from,
    );
    let pubkey = opt(args, "--pubkey").map(PathBuf::from);
    crate::userland::with_ctx(root, arch, |ctx| {
        crate::userland::miniroot::build(
            ctx,
            arch,
            &crate::userland::miniroot::Options {
                pubkey: pubkey.as_deref(),
                auto_install_conf: conf.as_deref(),
                image: &out,
            },
        )
        .map(|_| ())
    })
}

/// `cargo xtask sets --arch A [--bsd FILE] [--bsd-rd FILE]`: `base`, `comp`, the kernels,
/// `SHA256` and `SHA256.sig` in `target/install/<arch>/sets`.
pub(crate) fn sets(root: &Path, args: &[&str]) -> Result<()> {
    let arch = arch_of(args)?;
    let bsd = opt(args, "--bsd").map(PathBuf::from);
    let bsd_rd = opt(args, "--bsd-rd").map(PathBuf::from);
    crate::userland::with_ctx(root, arch, |ctx| {
        crate::userland::sets::build(ctx, arch, bsd.as_deref(), bsd_rd.as_deref()).map(|_| ())
    })
}

/// `cargo xtask install-media --arch A --rd-kernel K --bsd K2`: the miniroot, `bsd.rd` and
/// the sets. `K` is the kernel built with feature `miniroot`, `K2` the kernel the system
/// installs (`bsd`).
pub(crate) fn install_media(root: &Path, args: &[&str]) -> Result<()> {
    let arch = arch_of(args)?;
    let rd_kernel = PathBuf::from(opt(args, "--rd-kernel").ok_or("missing `--rd-kernel FILE`")?);
    let bsd = PathBuf::from(opt(args, "--bsd").ok_or("missing `--bsd FILE`")?);
    let dir = install_dir(root, arch);
    crate::userland::with_ctx(root, arch, |ctx| {
        use crate::userland::{miniroot, sets};
        let pubkey = sets::test_pubkey(ctx)?;
        let image = dir.join("miniroot.ffs");
        miniroot::build(
            ctx,
            arch,
            &miniroot::Options {
                pubkey: Some(&pubkey),
                auto_install_conf: None,
                image: &image,
            },
        )?;
        let bsd_rd = dir.join("bsd.rd");
        miniroot::make_bsd_rd(ctx, &rd_kernel, &image, &bsd_rd)?;
        sets::build(ctx, arch, Some(&bsd), Some(&bsd_rd)).map(|_| ())
    })
}

/// The answers of the autoinstall run (`install.sub`'s questions, in the order it asks them;
/// a question matches an answer by the text before its `?`). `port` is the HTTP server's.
fn auto_install_conf(port: u16) -> String {
    let server = format!("10.0.2.2:{port}");
    let answers = [
        ("System hostname", "emibsd".to_string()),
        ("Network interface to configure", "vio0".to_string()),
        ("IPv4 address for vio0", "10.0.2.15".to_string()),
        ("Netmask for vio0", "255.255.255.0".to_string()),
        ("IPv6 address for vio0", "none".to_string()),
        ("Network interface to configure", "done".to_string()),
        ("Default IPv4 route", "10.0.2.2".to_string()),
        ("DNS domain name", "emibsd.test".to_string()),
        ("DNS nameservers", "10.0.2.3".to_string()),
        ("Password for root account", "emibsd".to_string()),
        ("Public ssh key for root account", "none".to_string()),
        ("Start sshd(8) by default", "no".to_string()),
        ("Change the default console to com0", "yes".to_string()),
        ("Which speed should com0 use", "115200".to_string()),
        ("Setup a user", "no".to_string()),
        ("What timezone are you in", "UTC".to_string()),
        ("Which disk is the root disk", "sd0".to_string()),
        (
            "Use (W)hole disk MBR, whole disk (G)PT or (E)dit",
            "G".to_string(),
        ),
        ("An EFI/GPT disk may not boot. Proceed", "yes".to_string()),
        (
            "URL to autopartitioning template for disklabel",
            format!("http://{server}/disklabel.tmpl"),
        ),
        ("Location of sets", "http".to_string()),
        ("HTTP proxy URL", "none".to_string()),
        ("HTTP Server", server),
        ("Server directory", "sets".to_string()),
        ("Set name(s)", "-all bsd bsd.mp base* comp*".to_string()),
    ];
    let mut text = String::from(
        "# autoinstall(8) answers of `cargo xtask install` (tools/xtask/src/install.rs).\n",
    );
    for (question, answer) in answers {
        text.push_str(&format!("{question} = {answer}\n"));
    }
    text
}

/// What `disklabel -T` reads for the autopartitioning: one root for everything (base and comp
/// take about 1.5 GB extracted) and a small swap, in the 2.7 GB the EFI system partition
/// leaves of the disk. Partitions are lettered in the template's order
/// (`editor_allocspace`), so `/` comes first to be `a`, as in disklabel's own tables, and
/// swap is `b`. A range without a percentage gets its minimum, so the sizes are fixed.
const DISKLABEL_TEMPLATE: &str = "/\t2400M\nswap\t64M\n";

/// Serial lines that mean the installer or the kernel failed.
const FAILURES: &[&str] = &[
    "Question has no answer in response file",
    "failed; check /tmp/ai/ai.log",
    "Unable to get a verified list",
    "Autopartitioning failed",
    "No autopartitioning template found",
    "Failed to install bootblocks",
    "Checksum test for",
    "Installation of ",
    "panic: ",
    "not a valid choice",
];

/// The installed disk's path: `disk-<arch>-install.img` in the run directory.
fn target_disk(root: &Path, arch: Arch) -> PathBuf {
    boot::disk_path_n(root, arch, Some("install"), 0)
}

/// A kernel's path, or an error telling which recipe builds it.
fn need(path: &Path, what: &str, recipe: &str) -> Result<()> {
    if path.is_file() {
        Ok(())
    } else {
        Err(format!("{}: no {what}; run `just {recipe}` first", path.display()).into())
    }
}

/// The efiboot that boots the install media, where it boots the kernel: amd64's
/// `BOOTX64.EFI` (`just efiboot-amd64`). arm64's efiboot is track A3's; until it boots the
/// kernel, arm64's media boot through Limine.
fn media_efiboot(root: &Path, arch: Arch) -> Option<PathBuf> {
    match arch {
        Arch::Amd64 => {
            let efi = root.join("target/efiboot/amd64/BOOTX64.EFI");
            efi.is_file().then_some(efi)
        }
        Arch::Arm64 => None,
    }
}

/// `boot> ` answered: efiboot's prompt waits (the media's `boot.conf` turns its timeout off),
/// and a bare `boot` loads `/bsd`, which is the `bsd.rd` given.
const EFIBOOT_PROMPTS: &[(&str, &str)] = &[("boot> ", "boot\n")];

/// The boot image of the install media for `bsd_rd`, and the prompts to answer on the way to
/// the kernel: laid out as OpenBSD's `miniroot<rev>.img` (`efiboot-disk`: an OpenBSD
/// partition with an FFS holding `/bsd` and the EFI system partition with efiboot) where
/// efiboot is available, else the Limine image.
fn boot_media(
    root: &Path,
    arch: Arch,
    bsd_rd: &Path,
) -> Result<(PathBuf, &'static [(&'static str, &'static str)])> {
    match media_efiboot(root, arch) {
        Some(efi) => {
            crate::efiboot::efiboot_disk(root, arch, &efi, bsd_rd, None)?;
            Ok((boot::image_path(root, arch, None), EFIBOOT_PROMPTS))
        }
        None => Ok((boot::image(root, arch, bsd_rd, None, None, None)?, &[])),
    }
}

/// `cargo xtask install --arch A --rd-kernel K [--check-only]`: the install run and its
/// check (module docs); `--check-only` checks the disk a previous run installed.
pub(crate) fn install(root: &Path, args: &[&str]) -> Result<()> {
    let arch = arch_of(args)?;
    let rd_kernel = PathBuf::from(opt(args, "--rd-kernel").ok_or("missing `--rd-kernel FILE`")?);
    let dir = install_dir(root, arch);
    let sets = crate::userland::sets::sets_dir(root, arch);
    need(
        &sets.join("SHA256.sig"),
        "signed sets",
        &format!("install-media-{}", arch.name()),
    )?;
    need(
        &sets.join("bsd.rd"),
        "bsd.rd",
        &format!("install-media-{}", arch.name()),
    )?;
    let started = Instant::now();
    let disks = Disks {
        fresh: false,
        count: 1,
        set: Some("install"),
    };
    if args.contains(&"--check-only") {
        // Only run 2, on the disk a previous run installed.
        need(
            &target_disk(root, arch),
            "installed disk",
            &format!("smoke-install-{}", arch.name()),
        )?;
        return check_disk(root, arch, &sets.join("bsd.rd"), &disks, &dir);
    }

    // The server, the answers, and the install kernel whose ramdisk holds them.
    fs::write(dir.join("disklabel.tmpl"), DISKLABEL_TEMPLATE)
        .map_err(|e| format!("{}: {e}", dir.display()))?;
    let server = Server::start(&dir)?;
    let conf = auto_install_conf(server.port);
    let auto = dir.join("bsd.rd.auto");
    crate::userland::with_ctx(root, arch, |ctx| {
        use crate::userland::{miniroot, sets};
        let pubkey = sets::test_pubkey(ctx)?;
        let image = dir.join("miniroot-auto.ffs");
        miniroot::build(
            ctx,
            arch,
            &miniroot::Options {
                pubkey: Some(&pubkey),
                auto_install_conf: Some(&conf),
                image: &image,
            },
        )?;
        miniroot::make_bsd_rd(ctx, &rd_kernel, &image, &auto)
    })?;

    // The fresh disk: sd0 on both architectures (the install media's own disk is sd1).
    let disk = target_disk(root, arch);
    let _ = fs::remove_file(&disk);
    if let Some(parent) = disk.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let f = fs::File::create(&disk).map_err(|e| format!("{}: {e}", disk.display()))?;
    f.set_len(TARGET_DISK_BYTES)
        .map_err(|e| format!("{}: {e}", disk.display()))?;
    drop(f);

    // Run 1: the installer.
    let (image, prompts) = boot_media(root, arch, &auto)?;
    let cmd = boot::qemu_command(root, arch, &image, "stdio", None, &disks)?;
    let log = dir.join("install.log");
    let mut vm = Vm::spawn(&format!("install-{}", arch.name()), cmd, log.clone())?;
    vm.respond(
        prompts,
        FAILURES,
        &[Stop::Exited],
        boot::time_limit(Duration::from_secs(3600)),
    )?;
    let transcript = vm.text();
    drop(vm);
    for line in [
        "Starting non-interactive mode in 5 seconds",
        "Performing non-interactive install",
        "Let's install the sets!",
        "Installing base80.tgz",
        "Installing comp80.tgz",
        "Making all device nodes",
        "CONGRATULATIONS!",
    ] {
        if !transcript.contains(line) {
            return Err(format!(
                "install {}: the installer never said {line:?} (log: {})",
                arch.name(),
                log.display()
            )
            .into());
        }
    }
    println!(
        "xtask: install {}: the installer finished in {:.0}s",
        arch.name(),
        started.elapsed().as_secs_f32()
    );

    // Run 2: look at what it made, from the plain bsd.rd.
    check_disk(root, arch, &sets.join("bsd.rd"), &disks, &dir)?;
    println!(
        "xtask: install {}: ok in {:.0}s ({})",
        arch.name(),
        started.elapsed().as_secs_f32(),
        disk.display()
    );
    Ok(())
}

/// Boots the plain `bsd.rd`, mounts the installed disk read-only and checks what the
/// installer left: the kernel, the compiler, `/etc/rc`, the run-time linker, the boot loader
/// on the EFI system partition, and `fsck_ffs` agreeing the file system is clean.
fn check_disk(root: &Path, arch: Arch, bsd_rd: &Path, disks: &Disks<'_>, dir: &Path) -> Result<()> {
    let (image, prompts) = boot_media(root, arch, bsd_rd)?;
    let cmd = boot::qemu_command(root, arch, &image, "stdio", None, disks)?;
    let mut vm = Vm::spawn(
        &format!("check-{}", arch.name()),
        cmd,
        dir.join("check.log"),
    )?;
    let limit = boot::time_limit(Duration::from_secs(900));
    for (prompt, answer) in prompts {
        vm.wait_for(prompt, limit)?;
        vm.send(answer)?;
    }
    vm.wait_for("(I)nstall, (U)pgrade, (A)utoinstall or (S)hell?", limit)?;
    vm.send("s\n")?;
    vm.wait_for("# ", limit)?;
    let efi_part = match arch {
        Arch::Amd64 => "i",
        Arch::Arm64 => "i",
    };
    // The ramdisk's /dev has no node for the new disk: install.sub makes them with MAKEDEV
    // as it needs them, and so does the check.
    let script = format!(
        "cd /dev && sh MAKEDEV sd0 && cd / && disklabel sd0 && \
         mount -r /dev/sd0a /mnt && ls -l /mnt/bsd /mnt/bsd.sp /mnt/usr/bin/cc /mnt/etc/rc \
         /mnt/usr/libexec/ld.so /mnt/usr/lib/libc.so.104.0 /mnt/etc/fstab /mnt/etc/boot.conf && \
         cat /mnt/etc/fstab /mnt/etc/boot.conf && echo check-root-$((40+2)); \
         mount_msdos -o ro /dev/sd0{efi_part} /mnt2 && ls -lR /mnt2/efi && echo check-esp-$((40+2)); \
         umount /mnt2; umount /mnt; fsck_ffs -n /dev/rsd0a && echo check-fsck-$((40+2)); \
         echo check-$((6*7))-done\n"
    );
    vm.send(&script)?;
    vm.wait_for("check-42-done", boot::time_limit(Duration::from_secs(600)))?;
    let text = vm.text();
    drop(vm);
    for line in ["check-root-42", "check-esp-42", "check-fsck-42"] {
        if !text.contains(line) {
            return Err(format!("install {}: the installed disk lacks {line}", arch.name()).into());
        }
    }
    Ok(())
}

/// `cargo xtask install-boot --arch A`: boots the installed disk through the boot loader
/// the installer put on it, to `login:`, logs in and compiles and runs a program with the
/// installed `cc`. The disk is the boot disk of the VM (the loader finds the root by the
/// disk's DUID, as on OpenBSD).
pub(crate) fn install_boot(root: &Path, args: &[&str]) -> Result<()> {
    let arch = arch_of(args)?;
    let disk = target_disk(root, arch);
    need(
        &disk,
        "installed disk",
        &format!("smoke-install-{}", arch.name()),
    )?;
    let boot_image = boot::image_path(root, arch, None);
    let _ = fs::remove_file(&boot_image);
    fs::copy(&disk, &boot_image).map_err(|e| format!("{}: {e}", boot_image.display()))?;
    let cmd = boot::qemu_command(
        root,
        arch,
        &boot_image,
        "stdio",
        None,
        &Disks {
            fresh: true,
            count: 1,
            set: Some("boot"),
        },
    )?;
    let mut vm = Vm::spawn(
        &format!("boot-{}", arch.name()),
        cmd,
        install_dir(root, arch).join("boot.log"),
    )?;
    let limit = boot::time_limit(Duration::from_secs(900));
    vm.wait_for("login:", limit)?;
    vm.send("root\n")?;
    vm.wait_for("Password:", limit)?;
    vm.send("emibsd\n")?;
    vm.wait_for("# ", limit)?;
    vm.send("uname -a; mount; echo up-$((6*7))\n")?;
    vm.wait_for("up-42", limit)?;
    // ksh's `print -r` (a builtin: base has no printf(1) yet) writes the lines as they are.
    vm.send(
        "print -r '#include <stdio.h>' > hello.c; \
         print -r 'int main(void) { printf(\"hello from cc %d\\n\", 6 * 7); return 0; }' >> hello.c; \
         cc hello.c && ./a.out\n",
    )?;
    vm.wait_for(
        "hello from cc 42",
        boot::time_limit(Duration::from_secs(300)),
    )?;
    println!("xtask: install-boot {}: ok", arch.name());
    Ok(())
}
