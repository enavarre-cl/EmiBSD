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
//! M13's QEMU device options, and the disk images they attach (`docs/ARCHITECTURE.md`,
//! "Parallel smokes" for where the files go).
//!
//! - `--nvme FILE` (`qemu`, `smoke`): an NVM Express controller (`-device nvme`, nvme(4))
//!   whose one namespace is the raw image FILE (a relative path is in the run directory,
//!   [`boot::run_dir`]). On amd64 it sits on `q35`'s PCI bus, added right after the NICs
//!   so that it comes before the persistent virtio-blk disks and its namespace is `sd0`.
//!   On arm64 (M13) it is the first device on `virt`'s PCI bus (`pci0 dev 1`, the virtio
//!   devices are virtio-mmio); the kernel attaches the virtio-mmio disks before the PCI bus
//!   (`pciecam` comes after the `virtio_mmio` nodes in the device tree), so with the one
//!   persistent disk (`sd0`) and the boot image (`sd1`) the namespace is `sd2`.
//! - `--ahci FILE` (`qemu`, `smoke`): a SATA disk holding the raw image FILE (a relative
//!   path is in the run directory), ahci(4). On amd64 it is on the second port of q35's
//!   built-in AHCI controller (`ich9-ahci` at 0:1f.2, `ide-hd` on `ide.1`; the boot image is
//!   on `ide.0`). The controller's place on the bus does not change, so the disk is the unit
//!   after the boot image's (`sd2` with the one persistent virtio-blk disk). arm64's `virt`
//!   has no AHCI controller of its own, so (M13) an `ich9-ahci` is added as the first device
//!   on its PCI bus (`pci0 dev 1`) with the disk on port 0 (`ahci0.0`); as for `--nvme`, the
//!   virtio-mmio disks come first and the disk is `sd2`.
//! - `--scsi-cd ISO` (`qemu`, `smoke`, `smoke2`): a virtio SCSI host adapter with a CD-ROM
//!   drive holding the file `ISO` (read-only, `media=cdrom`; a relative path is taken from the
//!   workspace root): `virtio-scsi-pci` on amd64, `virtio-scsi-device` (virtio-mmio) on arm64,
//!   with `scsi-cd` on its bus. It is the LAST device added on both archs ([`add_devices`]),
//!   so the numbering of the other virtio devices is unchanged: amd64's PCI slots go up, and
//!   arm64 `virt` hands virtio-mmio slots out from the top down while the kernel finds them
//!   bottom up (the adapter takes the lowest slot, `vioscsi0`, found first; the NIC and the
//!   disks keep their slots, hence their names). Its `scsibus` is the one attached first.
//! - `--lsi FILE` (`qemu`, `smoke`): an LSI 53C895A SCSI adapter (`-device lsi53c895a`,
//!   siop(4)) with a `scsi-hd` at target 0 whose image is FILE in the run directory, made
//!   afresh (zeroed, [`LSI_DISK_BYTES`]) each run, as `--disk-fresh` makes its disks; with
//!   `--lsi-cd ISO`, also a read-only `scsi-cd` at target 1 holding `ISO` (a relative path is
//!   taken from the workspace root). amd64 only (arm64's GENERIC has no `siop`). It goes
//!   after `--scsi-cd`, the last devices ([`add_devices`]), so no other PCI slot moves.
//! - `--pci-serial FILE` (`qemu`, `smoke`, M13): a PCI serial card (`-device pci-serial`, QEMU's
//!   16550 behind PCI, 1b36:0002, puc(4) with com(4) on top) whose line is a file chardev:
//!   everything the guest sends to the card's UART is written to FILE in the run directory,
//!   made afresh each run. `--expect-pci-serial TEXT` (repeatable, `smoke`) then requires FILE
//!   to contain TEXT once the serial expectations passed ([`after_smoke`]). Both archs (the
//!   card sits on the PCI bus q35 and arm64's `virt` have). It goes after `--scsi-cd` and
//!   `--lsi`, the last devices ([`add_devices`]), so no other PCI slot moves.
//! - `--pci-bridges` (`qemu`, `smoke`, M16e, `smoke-ppb`): two PCI-PCI bridges, each with a
//!   virtio-blk disk behind it, for ppb(4): a PCI Express root port (`pcie-root-port`) with
//!   the disk `ppb-rp.img` on its secondary bus, and a conventional `pci-bridge` with the disk
//!   `ppb-br.img` in its slot 1, both bridges on the root bus of q35 and of arm64's `virt`
//!   (the firmware numbers their buses). The images are in the run directory, 64 MiB of
//!   zeroes made afresh each run ([`PPB_DISK_BYTES`]). They go after `--pci-serial`
//!   ([`add_devices`]), so no other PCI slot moves.
//! - `--machine pc` (`qemu`, `smoke`, M16e, `smoke-iic`): amd64's QEMU machine is i440fx's `pc`
//!   (PIIX3 and PIIX4: the ISA bridge at 00:01.0, the IDE controller at 00:01.1, the power
//!   management function with the SMBus at 00:01.3, the standard VGA at 00:02.0) instead of
//!   `q35`, so piixpm(4) has hardware to attach to. The boot image is on an `ich9-ahci`
//!   controller added after every other device ([`pc_boot_disk_args`]: q35 has one built in,
//!   and the firmware reads `pc`'s PIIX3 IDE channel with programmed I/O, which is minutes for
//!   the 170 MB Limine loads), so the kernel sees it as on q35 (`ahci0`, `sd1`); the PIIX3 IDE
//!   controller is left free. The NICs, the NVMe controller and the virtio-blk disks are plain
//!   PCI devices on the root bus, and `q35` stays the default. amd64 only (arm64 has only `virt`); `pc` has no
//!   built-in AHCI controller and its PCI bus is not PCI Express, so `--ahci` and
//!   `--pci-bridges` (a `pcie-root-port`) are refused with it. An IDE agent may use the same
//!   option ([`amd64_machine`]).
//! - `--ipmi` (`qemu`, `smoke`, M16e, `smoke-ipmi`, amd64 only): a baseboard management
//!   controller for ipmi(4): QEMU's simulated BMC (`ipmi-bmc-sim`) behind a KCS system
//!   interface on q35's ISA bus (`isa-ipmi-kcs`, I/O ports 0xca2-0xca3). QEMU describes it to
//!   the guest in its DSDT (an `IPI0001` device with `_IFT` 1, `_SRV` 0x200 and the I/O
//!   ports in `_CRS`) and in SMBIOS (type 38). The BMC's SDR repository is the file
//!   `ipmi-sdr.bin` in the run directory, written each run ([`IPMI_SDRS`]): one compact
//!   sensor record, a temperature sensor named `QEMU Temp`, which the simulator also makes a
//!   sensor of (scanning on, reading 0), so a driver that reaches the BMC exports it as
//!   `hw.sensors.ipmi0.temp0`. ipmi(4) does not, OpenBSD 8.0's as EmiBSD's: it maps `_CRS`'s
//!   `_MAX`, 0xca3, and every command fails (`smoke-ipmi`). ISA devices take no PCI slot, so
//!   nothing on the bus moves.
//! - `--reboot` (`qemu`, `smoke`, M13): QEMU runs without `-no-reboot`, so a guest reset
//!   restarts the machine (EDK2, Limine and the kernel again; the EDK2 variable store is the
//!   run's copy) instead of ending QEMU with status 0. `smoke-power` boots, runs `reboot`
//!   and expects a second boot's login. Without it, an ACPI power-off (`halt -p`, S5) and a
//!   reset both end QEMU with status 0, so a smoke that checks a power-off passes
//!   `--status 0` and rejects `rebooting...`, which `boot(9)` prints before every reset.
//! - `--nic MODEL` (`qemu`, `smoke`, M13): the NIC on QEMU's user network, the one that
//!   is vio0 otherwise, is an Intel PRO/1000 of that model instead, for em(4): `e1000`
//!   (82540EM), `e1000e` (82574L) or `igb` (82576); or `rtl8139`, QEMU's Realtek 8139C+,
//!   for re(4) (`smoke-re`); or `vmxnet3`, QEMU's VMware VMXNET3, for vmx(4)
//!   (`smoke-vmx`). It takes vio0's place on the command
//!   line and its netdev (`n0`), so it is the only Ethernet interface (em0, which the
//!   kernel's network self-test configures as it does vio0) and no other device moves. On
//!   arm64 it is a PCI device on `virt`'s PCIe bus, where vio0 is on virtio-mmio. Not with
//!   `--vio-mq` (that is vio0's).
//! - `--fb` (`qemu`, `smoke`, M13): a display for the firmware's GOP. amd64's `q35` has its
//!   standard VGA (`-display none` only hides the window; OVMF's QemuVideoDxe drives it), so
//!   nothing is added there; arm64's `virt` has no display, so `-device ramfb` is added
//!   (ArmVirtQemu's QemuRamfbDxe gives a linear GOP frame buffer in guest RAM; a
//!   `virtio-gpu` GOP is blit-only, which Limine cannot use). `ramfb` is not a PCI device,
//!   so nothing on the buses moves.
//! - `--screenshot-after LINE` (`smoke`, M13, implies `--fb`): QEMU gets a human monitor on a
//!   Unix socket in the run directory (`monitor.sock`, instead of `-monitor none`; named
//!   relative to the checkout, or in the temporary directory, to fit macOS's 104-byte
//!   `sun_path`: [`monitor_sock`]); when a
//!   serial line contains LINE, `screendump` writes `screen.ppm` there, and once the serial
//!   expectations passed ([`after_smoke`]) the picture is checked against that line, which
//!   must read `x=X y=Y w=W h=H ink=N fg=RRGGBB bg=RRGGBB` (the kernel's `selftest=fb`,
//!   `kern/selftest.rs`): inside the box every pixel is `fg` or `bg`, exactly `N` are `fg`
//!   (the set bits of the glyphs drawn), and the box lies inside the picture.
//! - `--screen-text ROW:COL:TEXT` (`smoke`, M13, with `--screenshot-after`): the picture is
//!   checked for TEXT on wsdisplay's character grid instead, at row ROW and column COL of the
//!   grid the kernel's `selftest=wscons` line describes (`selftest: wscons grid x=X y=Y
//!   cw=W ch=H cols=C rows=R`, `kern/selftest.rs`): every character cell of TEXT holds
//!   exactly two colours, its background and the same text colour as the others, with some
//!   text pixels for a character and none for a space, and the cell after TEXT is blank
//!   ([`check_screen_text`]). `smoke-wscons` writes TEXT to `/dev/ttyC0` from the shell.
//! - `--sendkey-after LINE --sendkeys KEYS` (`smoke`, M13, `smoke-kbd`; repeatable, the pairs
//!   in order): QEMU gets the human monitor socket as for `--screenshot-after`; when a serial
//!   line contains LINE, each of the space-separated KEYS (QEMU key names: `h`, `shift-a`,
//!   `ret`, ...) is typed with the monitor's `sendkey` on the guest's keyboard (with `--usb`,
//!   the `usb-kbd` on `qemu-xhci`), one every [`SENDKEY_GAP`] ([`parse_sendkeys`],
//!   [`poll_sendkey`]). The run fails if a LINE never came.
//! - `--iommu intel|amd` (`qemu`, `smoke`, M16e, amd64): a DMA remapping unit on `q35`,
//!   QEMU's `intel-iommu` (VT-d, with an ACPI `DMAR` table) or `amd-iommu` (AMD-Vi, an `IVRS`
//!   table and the unit's own PCI function) with `dma-remap=on` (without it QEMU's AMD-Vi
//!   lets every device's DMA through untranslated, so the I/O virtual addresses the guest
//!   programs land on the wrong memory), added right after `-M q35` ahead of every PCI
//!   device. acpidmar(4) then gives each PCI device a domain, so the guest programs I/O virtual
//!   addresses: the virtio devices (vio0, the virtio-blk disks) get `disable-legacy=on,
//!   iommu_platform=on` ([`virtio_pci_props`]), so that they are modern-only and translate
//!   their DMA through the IOMMU (virtio_pci negotiates `VIRTIO_F_ACCESS_PLATFORM`); without
//!   it QEMU's virtio devices bypass the IOMMU. Emulated devices (AHCI, NVMe, ...) always
//!   go through it. On arm64 the one model is `smmuv3` (M16f, below).
//! - `--gic N` (`qemu`, `smoke`, `smoke2`, M16f; arm64): the version of `virt`'s interrupt
//!   controller, `2` (QEMU's default, ampintc(4) with its GICv2m MSI frame) or `3` (agintc(4)
//!   with its ITS, `gic-version=3`). Without the option the environment's `EMIBSD_GIC` (`2`
//!   or `3`) decides, so `EMIBSD_GIC=3 just smoke` puts every arm64 boot on GICv3; amd64
//!   ignores both ([`virt_machine`]).
//! - `--iommu smmuv3` (`qemu`, `smoke`, M16f; arm64): `virt` with its SMMUv3 in front of the
//!   PCIe bus (`iommu=smmuv3`), smmu(4); the virtio-mmio devices do not go through it.
//! - `--monitor-after LINE --monitor CMD` (`smoke`, M16f, `smoke-powerbtn`; repeatable, the
//!   pairs in order): QEMU gets the human monitor socket as for `--screenshot-after`; when a
//!   serial line contains LINE, the monitor command CMD (`system_powerdown`, ...) is sent
//!   ([`poll_monitor_cmds`]). The run fails if a LINE never came.
//! - `--tpm tis|crb` (`qemu`, `smoke`, M16e, amd64, `smoke-tpm`): a TPM 2.0 for tpm(4),
//!   QEMU's `tpm-tis` (the TIS FIFO registers) or `tpm-crb` (the Command Response Buffer) at
//!   0xfed40000, an ACPI `MSFT0101` device with a `TPM2` table, backed by the software TPM
//!   `swtpm` (found in `$PATH` or under `brew --prefix swtpm`, docs/SETUP.md). Each run
//!   starts its own `swtpm socket --tpm2 --flags startup-clear` with a fresh state directory
//!   `tpm/` and its log `swtpm.log` in the run directory and its control socket beside them
//!   (`swtpm.sock`, placed as the monitor socket is, to fit `sun_path`), waits for the
//!   socket, and gives QEMU `-chardev socket` + `-tpmdev emulator` + `-device
//!   tpm-tis|tpm-crb` ([`tpm_args`]). The TPM is not on a PCI bus, so nothing moves. swtpm
//!   is stopped when the run ends, however it ends ([`Swtpm`]'s `Drop`: kill, wait, remove
//!   the socket), and it also exits on its own when QEMU closes its control connection
//!   (`terminate`), so no swtpm outlives a run. `startup-clear` has swtpm send
//!   `TPM2_Startup(CLEAR)` once QEMU initialises it, so the TPM answers commands whatever the
//!   firmware does. Refused on arm64 (its GENERIC has no tpm) and outside `qemu` and
//!   `smoke`, which start swtpm.
//! - `{host-ms}` in a `smoke` `--send` text (M13, `smoke-clock`): replaced, as the text is
//!   sent, by the host's wall clock in milliseconds since the Epoch ([`expand_send`]), so a
//!   guest script can set its own clock readings beside the host's and compare the rates.
//! - `cargo xtask nvme-root --arch A [--duid HEX] [--out FILE]`: the disk `just smoke-nvme`
//!   boots from, `nvme-<arch>.img` in the run directory unless `--out` names another file.
//!   It is laid out as OpenBSD's installer lays a disk out: an MBR whose one partition is
//!   the OpenBSD one (`DOSPTYP_OPENBSD`, 0xA6) from sector 64, the disklabel in its sector
//!   `LABELSECTOR` (1), partition `a` (4.2BSD) from sector 64 holding the file system, `c`
//!   the whole disk. The file system is the userland's ffs image
//!   (`target/userland/<arch>/ramdisk.ffs`, made by OpenBSD's makefs, `userland/ramdisk.rs`)
//!   copied whole, with one change: its `/etc/fstab` names the root `/dev/sd0a`, where the
//!   ramdisk's names `/dev/rd0a` (rc(8)'s `mount -uw /` looks the root's device up there;
//!   `--root-dev sdNa` names another unit, for a disk that is not the first: `smoke-ahci`'s
//!   is `sd2a`, as diskmap(4) and DUIDs in fstab are not ported). The two lines have the
//!   same length, so the file is rewritten in place in the copy. The
//!   label's DUID is `--duid` (16 hexadecimal digits, [`NVME_ROOT_DUID`] by default): the
//!   kernel's command line names it with `bootduid=` (boot(8)'s `BOOTARG_BOOTDUID`), and
//!   `setroot` mounts the root from the disk whose label has it.

use std::fs;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::Result;
use crate::boot::{self, Arch};

/// The placeholder [`expand_send`] replaces.
const HOST_MS: &str = "{host-ms}";

/// A `--send` text as it goes to the guest: each `{host-ms}` becomes the host's wall clock in
/// milliseconds since the Epoch, read now.
pub(crate) fn expand_send(text: &str) -> std::borrow::Cow<'_, str> {
    if !text.contains(HOST_MS) {
        return std::borrow::Cow::Borrowed(text);
    }
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    std::borrow::Cow::Owned(text.replace(HOST_MS, &ms.to_string()))
}

/// The DUID of the root disk `nvme-root` writes by default (`NVMEROOT` in ASCII).
pub(crate) const NVME_ROOT_DUID: &str = "4e564d45524f4f54";

/// The bytes of a sector (`DEV_BSIZE`).
const SECTOR: u64 = 512;
/// The OpenBSD partition's first sector, as fdisk(8) puts it (and partition `a`'s).
const OPENBSD_START: u64 = 64;
/// The disk is a whole number of MiB.
const ALIGN: u64 = 2048;
/// `LABELSECTOR`: the label's sector, relative to the OpenBSD partition.
const LABELSECTOR: u64 = 1;
/// `DOSPTYP_OPENBSD`.
const DOSPTYP_OPENBSD: u8 = 0xa6;
/// `DISKMAGIC`.
const DISKMAGIC: u32 = 0x8256_4557;
/// `DTYPE_SCSI`: sd(4)'s disks.
const DTYPE_SCSI: u16 = 4;
/// `MAXPARTITIONS` (amd64 and arm64).
const MAXPARTITIONS: u16 = 16;
/// `FS_BSDFFS`.
const FS_BSDFFS: u8 = 7;
/// `DISKLABELV1_FFS_FRAGBLOCK(512, 8)`: makefs's 512-byte fragments, 4096-byte blocks.
const FFS_FRAGBLOCK: u8 = 4;
/// `BBSIZE` and `SBSIZE` (`ufs/ffs/fs.h`), for `d_bbsize` and `d_sbsize`.
const BBSIZE: u32 = 8192;
const SBSIZE: u32 = 8192;

/// The ramdisk's root line in `/etc/fstab` (`userland/ramdisk.rs`, `FSTAB`) and the disk's.
const FSTAB_RD0A: &[u8] = b"/dev/rd0a / ffs rw 1 1\n";
const FSTAB_SD0A: &[u8] = b"/dev/sd0a / ffs rw 1 1\n";

/// The fstab root line naming `dev` (`sd0a` by default; `--root-dev`): four characters, a
/// two-letter driver, a unit digit and a partition letter, so that the line keeps the
/// ramdisk's length.
fn fstab_root_line(dev: Option<&str>) -> Result<Vec<u8>> {
    let Some(dev) = dev else {
        return Ok(FSTAB_SD0A.to_vec());
    };
    let b = dev.as_bytes();
    if b.len() != 4
        || !b[..2].iter().all(u8::is_ascii_lowercase)
        || !b[2].is_ascii_digit()
        || !(b'a'..=b'p').contains(&b[3])
    {
        return Err(format!("--root-dev {dev}: expected a disk partition such as sd2a").into());
    }
    Ok(format!("/dev/{dev} / ffs rw 1 1\n").into_bytes())
}

/// `--nvme FILE` for every VM this run starts (set once by `main`).
static NVME: OnceLock<PathBuf> = OnceLock::new();

/// `--ahci FILE` for every VM this run starts (set once by `main`).
static AHCI: OnceLock<PathBuf> = OnceLock::new();

/// `--scsi-cd ISO` for every VM this run starts (set once by `main`).
static SCSI_CD: OnceLock<PathBuf> = OnceLock::new();

/// The size of the `--lsi` disk: 64 MiB, as `--disk-fresh`'s.
pub(crate) const LSI_DISK_BYTES: u64 = 64 << 20;

/// `--lsi FILE` (in the run directory) for every VM this run starts (set once by `main`).
static LSI: OnceLock<PathBuf> = OnceLock::new();

/// `--lsi-cd ISO` for every VM this run starts (set once by `main`).
static LSI_CD: OnceLock<PathBuf> = OnceLock::new();

/// The path that follows the option `opt` in `args`, if the option is there.
fn opt_path<'a>(args: &[&'a str], opt: &str) -> Result<Option<&'a str>> {
    match args.iter().position(|a| *a == opt) {
        None => Ok(None),
        Some(i) => match args.get(i + 1) {
            Some(p) => Ok(Some(p)),
            None => Err(format!("{opt}: expected a path").into()),
        },
    }
}

/// The size of each `--pci-bridges` disk: 64 MiB, as `--disk-fresh`'s.
pub(crate) const PPB_DISK_BYTES: u64 = 64 << 20;

/// `--pci-bridges`: the run directory its two disks go in (set once by `main`).
static PCI_BRIDGES: OnceLock<PathBuf> = OnceLock::new();

/// `--ipmi`: the run directory the BMC's SDR file goes in (set once by `main`).
static IPMI: OnceLock<PathBuf> = OnceLock::new();

/// `--ipmi`'s SDR repository: one IPMI 2.0 compact sensor record (type 2, 32 bytes and the
/// name). QEMU's `ipmi-bmc-sim` renumbers the records it loads and makes a sensor of every
/// compact one, indexed by its sensor number.
pub(crate) const IPMI_SDRS: &[u8] = &[
    0x00, 0x00, // record ID (QEMU assigns its own)
    0x51, // SDR version 1.5
    0x02, // record type: compact sensor
    36,   // bytes after this header
    0x20, // sensor owner: the BMC
    0x00, // owner LUN
    0x02, // sensor number
    0x07, // entity: system board
    0x01, // entity instance
    0x40, // sensor initialization: scanning on
    0x00, // sensor capabilities
    0x01, // sensor type: temperature
    0x01, // event/reading type: threshold
    0x00, 0x00, // assertion event mask
    0x00, 0x00, // deassertion event mask
    0x00, 0x00, // discrete reading mask
    0x00, // units 1: unsigned
    0x01, // units 2: degrees C
    0x00, // units 3
    0x01, // record sharing: one sensor
    0x00, // instance modifier
    0x00, // positive hysteresis
    0x00, // negative hysteresis
    0x00, 0x00, 0x00, // reserved
    0x00, // OEM
    0xc9, // ID string: 8-bit ASCII, 9 bytes
    b'Q', b'E', b'M', b'U', b' ', b'T', b'e', b'm', b'p',
];

/// `--pci-serial FILE` (in the run directory) for every VM this run starts (set once by `main`).
static PCI_SERIAL: OnceLock<PathBuf> = OnceLock::new();

/// The `--expect-pci-serial TEXT` options of this run (set once by `main`).
static PCI_SERIAL_EXPECT: OnceLock<Vec<String>> = OnceLock::new();

/// `--reboot`: this run's VMs restart on a guest reset (set once by `main`).
static REBOOT: OnceLock<()> = OnceLock::new();

/// `--fb`: a display device for the firmware's GOP.
static FB: OnceLock<()> = OnceLock::new();

/// `--screenshot-after LINE` and the run directory the socket and the picture go in.
static SCREENSHOT: OnceLock<(String, PathBuf)> = OnceLock::new();

/// The serial line the screenshot was taken at, once it was.
static SHOT_LINE: Mutex<Option<String>> = Mutex::new(None);

/// `--screen-text ROW:COL:TEXT`: the text the screenshot must show on wsdisplay's grid.
static SCREEN_TEXT: OnceLock<(usize, usize, String)> = OnceLock::new();

/// A `--sendkey-after LINE --sendkeys KEYS` pair: the line, and the keys split at spaces.
type Sendkeys = (String, Vec<String>);

/// The `--sendkey-after`/`--sendkeys` pairs, in order, and the run directory the monitor
/// socket goes in.
static SENDKEY: OnceLock<(Vec<Sendkeys>, PathBuf)> = OnceLock::new();

/// How many pairs were typed.
static SENT_KEYS: Mutex<usize> = Mutex::new(0);

/// A `--monitor-after LINE --monitor CMD` pair.
type MonitorCmd = (String, String);

/// The `--monitor-after`/`--monitor` pairs, in order, and the run directory the monitor
/// socket goes in.
static MONITOR_CMDS: OnceLock<(Vec<MonitorCmd>, PathBuf)> = OnceLock::new();

/// How many monitor commands were sent.
static SENT_CMDS: Mutex<usize> = Mutex::new(0);

/// The pause between two keys: QEMU holds each key down 100 ms by default.
const SENDKEY_GAP: Duration = Duration::from_millis(250);

/// The start of the kernel's line that describes wsdisplay's character grid.
const GRID_PREFIX: &str = "selftest: wscons grid";

/// The kernel's grid line, kept when the screenshot is taken.
static GRID_LINE: Mutex<Option<String>> = Mutex::new(None);

/// `--vio-mq`: vio0's virtio-net offers multiqueue (`mq=on`, set once by `main`).
static VIO_MQ: OnceLock<()> = OnceLock::new();

/// The IOMMUs `--iommu` takes: the model, its arch, and QEMU's `-device` for it (amd64's
/// are q35 devices; arm64's SMMUv3 is a `virt` machine property, [`virt_machine`]).
const IOMMU_MODELS: &[(&str, Arch, &str)] = &[
    ("intel", Arch::Amd64, "intel-iommu"),
    ("amd", Arch::Amd64, "amd-iommu,dma-remap=on"),
    ("smmuv3", Arch::Arm64, ""),
];

/// `--iommu MODEL`: the IOMMU model, an entry of [`IOMMU_MODELS`] (set once by `main`).
static IOMMU: OnceLock<&'static str> = OnceLock::new();

/// The q35 `-device` of the `--iommu` unit, when it is amd64's (`intel`, `amd`).
fn q35_iommu_dev() -> Option<&'static str> {
    let model = IOMMU.get()?;
    IOMMU_MODELS
        .iter()
        .find(|(m, arch, _)| m == model && *arch == Arch::Amd64)
        .map(|&(_, _, dev)| dev)
}

/// `--machine pc`: amd64 VMs run on i440fx's `pc`, not `q35` (set once by `main`).
static MACHINE_PC: OnceLock<()> = OnceLock::new();

/// Whether this run's amd64 VMs are i440fx's `pc` (`--machine pc`).
pub(crate) fn machine_pc() -> bool {
    MACHINE_PC.get().is_some()
}

/// The QEMU machine of this run's amd64 VMs: `q35`, or `pc` with `--machine pc`.
pub(crate) fn amd64_machine() -> &'static str {
    if machine_pc() { "pc" } else { "q35" }
}

/// Reads `--machine` from `args`: whether it asks for `pc`. Only `pc` is a choice (`q35` is
/// the default, so it is not named); it is amd64 only, and the options that need `q35`'s
/// hardware are refused with it.
fn machine_opt(args: &[&str]) -> Result<bool> {
    let Some(machine) = opt_path(args, "--machine")? else {
        return Ok(false);
    };
    if machine != "pc" {
        return Err(format!("--machine {machine}: expected pc (q35 is the default)").into());
    }
    if opt_path(args, "--arch")? == Some("arm64") {
        return Err("--machine pc: amd64 only (arm64 has only the virt machine)".into());
    }
    if args.contains(&"--ahci") {
        return Err(
            "--machine pc: not with --ahci (only q35 has a built-in AHCI controller)".into(),
        );
    }
    if args.contains(&"--pci-bridges") {
        return Err(
            "--machine pc: not with --pci-bridges (pc's bus has no PCI Express root ports)".into(),
        );
    }
    Ok(true)
}

/// The interfaces `--tpm` takes, and QEMU's device for each.
const TPM_MODELS: &[(&str, &str)] = &[("tis", "tpm-tis"), ("crb", "tpm-crb")];

/// `--tpm MODEL`: the QEMU device of the TPM and the run directory swtpm's files go in
/// (amd64, set once by `main`).
static TPM: OnceLock<(&'static str, PathBuf)> = OnceLock::new();

/// The control socket of the swtpm this run started ([`start_swtpm`]), while it runs.
static SWTPM_SOCK: Mutex<Option<PathBuf>> = Mutex::new(None);

/// How long [`start_swtpm`] waits for swtpm's control socket.
const SWTPM_START_TIMEOUT: Duration = Duration::from_secs(10);

/// The models `--nic` takes: QEMU's emulated Intel PRO/1000 controllers, which em(4) drives,
/// its Realtek 8139C+, which re(4) drives, and its VMware VMXNET3, which vmx(4) drives.
const NIC_MODELS: &[&str] = &["e1000", "e1000e", "igb", "rtl8139", "vmxnet3"];

/// `--nic MODEL`: the user-network NIC's model, in vio0's place (set once by `main`).
static NIC: OnceLock<String> = OnceLock::new();

/// `--acpi` (arm64): `virt` with its ACPI tables, so EDK2 hands the loader ACPI and no
/// device tree, and the disks and the user NIC on `virt`'s PCI bus (`virtio-*-pci`), the
/// only bus a tree made from the ACPI tables can reach (set once by `main`).
static ACPI: OnceLock<()> = OnceLock::new();

/// `--gic N` or `EMIBSD_GIC` (arm64): `virt`'s GIC version, 2 or 3 (set once by `main`).
static GIC: OnceLock<u8> = OnceLock::new();

/// The GIC version a `--gic` value or `EMIBSD_GIC` names: `2` or `3`.
fn parse_gic(v: &str, what: &str) -> Result<u8> {
    match v {
        "2" => Ok(2),
        "3" => Ok(3),
        _ => Err(format!("{what} {v}: expected 2 or 3").into()),
    }
}

/// Records this run's device options (`--nvme`, `--ahci`, `--scsi-cd`, `--lsi`, `--lsi-cd`,
/// `--pci-serial`, `--expect-pci-serial`, `--pci-bridges`, `--ipmi`, `--reboot`, `--vio-mq`,
/// `--nic`, `--acpi`, `--gic`, `--iommu`, `--machine`, `--tpm`).
pub(crate) fn set(root: &Path, args: &[&str]) -> Result<()> {
    if let Some(model) = opt_path(args, "--tpm")? {
        let Some(&(_, dev)) = TPM_MODELS.iter().find(|(m, _)| *m == model) else {
            return Err(format!("--tpm {model}: expected tis or crb").into());
        };
        if args.windows(2).any(|w| w == ["--arch", "arm64"]) {
            return Err("--tpm: amd64 only (arm64's GENERIC has no tpm)".into());
        }
        if !matches!(args.first(), Some(&("qemu" | "smoke"))) {
            return Err("--tpm: only with `qemu` and `smoke`, which start swtpm".into());
        }
        let _ = TPM.set((dev, boot::run_dir(root)));
    }
    if let Some(model) = opt_path(args, "--iommu")? {
        let Some(&(name, arch, _)) = IOMMU_MODELS.iter().find(|(m, _, _)| *m == model) else {
            return Err(
                format!("--iommu {model}: expected intel, amd (amd64) or smmuv3 (arm64)").into(),
            );
        };
        let other = match arch {
            Arch::Amd64 => "arm64",
            Arch::Arm64 => "amd64",
        };
        if args.windows(2).any(|w| w == ["--arch", other]) {
            return Err(format!("--iommu {name}: not on {other}").into());
        }
        if arch == Arch::Amd64 && args.contains(&"--machine") {
            return Err("--iommu: not with --machine (QEMU's IOMMUs need q35)".into());
        }
        let _ = IOMMU.set(name);
    }
    if machine_opt(args)? {
        let _ = MACHINE_PC.set(());
    }
    if let Some(model) = opt_path(args, "--nic")? {
        if !NIC_MODELS.contains(&model) {
            return Err(format!("--nic {model}: expected one of {}", NIC_MODELS.join(", ")).into());
        }
        if args.contains(&"--vio-mq") {
            return Err("--nic: not with --vio-mq (there is no vio0)".into());
        }
        let _ = NIC.set(model.to_string());
    }
    if args.contains(&"--reboot") {
        let _ = REBOOT.set(());
    }
    if args.contains(&"--pci-bridges") {
        let _ = PCI_BRIDGES.set(boot::run_dir(root));
    }
    if args.contains(&"--ipmi") {
        let _ = IPMI.set(boot::run_dir(root));
    }
    if args.contains(&"--acpi") {
        let _ = ACPI.set(());
    }
    match opt_path(args, "--gic")? {
        Some(v) => {
            let _ = GIC.set(parse_gic(v, "--gic")?);
        }
        None => match std::env::var("EMIBSD_GIC") {
            Ok(v) if !v.is_empty() => {
                let _ = GIC.set(parse_gic(&v, "EMIBSD_GIC")?);
            }
            _ => {}
        },
    }
    if args.contains(&"--vio-mq") {
        let _ = VIO_MQ.set(());
    }
    if args.contains(&"--fb") {
        let _ = FB.set(());
    }
    if let Some(line) = opt_path(args, "--screenshot-after")? {
        let _ = FB.set(());
        let _ = SCREENSHOT.set((line.to_string(), boot::run_dir(root)));
    }
    let pairs = parse_sendkeys(args)?;
    if !pairs.is_empty() {
        let _ = SENDKEY.set((pairs, boot::run_dir(root)));
    }
    let cmds = parse_pairs(args, "--monitor-after", "--monitor")?;
    if !cmds.is_empty() {
        let _ = MONITOR_CMDS.set((cmds, boot::run_dir(root)));
    }
    if let Some(spec) = opt_path(args, "--screen-text")? {
        if SCREENSHOT.get().is_none() {
            return Err("--screen-text: needs --screenshot-after".into());
        }
        let _ = SCREEN_TEXT.set(parse_screen_text(spec)?);
    }
    if let Some(w) = args.windows(2).find(|w| w[0] == "--nvme") {
        let _ = NVME.set(boot::run_dir(root).join(w[1]));
    }
    if let Some(w) = args.windows(2).find(|w| w[0] == "--ahci") {
        let _ = AHCI.set(boot::run_dir(root).join(w[1]));
    }
    if let Some(i) = args.iter().position(|a| *a == "--scsi-cd") {
        let Some(iso) = args.get(i + 1) else {
            return Err("--scsi-cd: expected the path of an ISO file".into());
        };
        let _ = SCSI_CD.set(PathBuf::from(iso));
    }
    if let Some(file) = opt_path(args, "--lsi")? {
        let _ = LSI.set(boot::run_dir(root).join(file));
    }
    if let Some(file) = opt_path(args, "--pci-serial")? {
        let _ = PCI_SERIAL.set(boot::run_dir(root).join(file));
    }
    let expect: Vec<String> = args
        .windows(2)
        .filter(|w| w[0] == "--expect-pci-serial")
        .map(|w| w[1].to_string())
        .collect();
    if !expect.is_empty() {
        if PCI_SERIAL.get().is_none() {
            return Err("--expect-pci-serial: needs --pci-serial".into());
        }
        let _ = PCI_SERIAL_EXPECT.set(expect);
    }
    if let Some(iso) = opt_path(args, "--lsi-cd")? {
        if LSI.get().is_none() {
            return Err("--lsi-cd: needs --lsi (the adapter and its disk)".into());
        }
        let _ = LSI_CD.set(PathBuf::from(iso));
    }
    Ok(())
}

/// The extra properties of amd64's user-network NIC (`vio0`): with `--vio-mq`, `mq=on`, so
/// the device offers `VIRTIO_NET_F_MQ` and vio(4) takes its multiqueue path (an intrmap and
/// one MSI-X vector per queue pair, the configuration and control vectors apart). QEMU's
/// user network has one queue pair, so the device reports one.
pub(crate) fn vio0_props() -> &'static str {
    if VIO_MQ.get().is_some() { ",mq=on" } else { "" }
}

/// The QEMU arguments of the `--iommu` unit (none without one); on `q35`, before every PCI
/// device.
pub(crate) fn iommu_args() -> Vec<String> {
    iommu_args_for(q35_iommu_dev())
}

/// [`iommu_args`] for the device `dev`.
fn iommu_args_for(dev: Option<&str>) -> Vec<String> {
    dev.map_or_else(Vec::new, |d| vec!["-device".to_string(), d.to_string()])
}

/// The extra properties of amd64's `virtio-*-pci` devices: with `--iommu`, modern-only and
/// behind the IOMMU (`disable-legacy=on,iommu_platform=on`).
pub(crate) fn virtio_pci_props() -> &'static str {
    if q35_iommu_dev().is_some() {
        ",disable-legacy=on,iommu_platform=on"
    } else {
        ""
    }
}

/// The `-device` argument of the NIC on QEMU's user network (netdev `n0`, `props` its MAC
/// when there is one): vio0, a `virtio-net-pci` on amd64 (with `--vio-mq`'s properties) and a
/// `virtio-net-device` on arm64, or the `--nic` model.
pub(crate) fn user_nic(arch: Arch, props: &str) -> String {
    user_nic_arg(NIC.get().map(String::as_str), arch, props)
}

/// [`user_nic`] for the model `nic` (`--nic`, if any).
fn user_nic_arg(nic: Option<&str>, arch: Arch, props: &str) -> String {
    match (nic, arch) {
        (Some(model), _) => format!("{model},netdev=n0{props}"),
        (None, Arch::Amd64) => format!(
            "virtio-net-pci,netdev=n0{props}{}{}",
            vio0_props(),
            virtio_pci_props()
        ),
        (None, Arch::Arm64) if acpi() => format!("virtio-net-pci,netdev=n0{props}"),
        (None, Arch::Arm64) => format!("virtio-net-device,netdev=n0{props}"),
    }
}

/// Whether this run's arm64 VMs boot `virt` with ACPI (`--acpi`).
pub(crate) fn acpi() -> bool {
    ACPI.get().is_some()
}

/// The `-M` argument of an arm64 VM: `virt` with ACPI (`--acpi`) or without (EDK2 then
/// installs the device tree), its GIC version (`--gic`, `EMIBSD_GIC`) and its SMMUv3
/// (`--iommu smmuv3`).
pub(crate) fn virt_machine() -> String {
    virt_machine_arg(
        acpi(),
        GIC.get().copied().unwrap_or(2),
        IOMMU.get() == Some(&"smmuv3"),
    )
}

/// [`virt_machine`] for the given ACPI, GIC version and SMMU choices.
fn virt_machine_arg(acpi: bool, gic: u8, smmu: bool) -> String {
    let mut m = String::from(if acpi {
        "virt,acpi=on"
    } else {
        "virt,acpi=off"
    });
    if gic == 3 {
        m.push_str(",gic-version=3");
    }
    if smmu {
        m.push_str(",iommu=smmuv3");
    }
    m
}

/// Whether this run's VMs restart on a guest reset (`--reboot`): QEMU then runs without
/// `-no-reboot`, so a reset (`reboot`, the FADT's reset register) boots the firmware again
/// instead of ending the emulator.
pub(crate) fn reboot() -> bool {
    REBOOT.get().is_some()
}

/// Adds the PCI storage controllers this run asked for to `cmd` (called after the NICs,
/// before the virtio-blk disks: amd64's come after them on the PCI bus; arm64's are
/// virtio-mmio, so these are the first devices on `virt`'s PCI bus), and the `--ahci` disk.
pub(crate) fn pci_storage(cmd: &mut Command, arch: Arch) -> Result<()> {
    if let Some(image) = AHCI.get() {
        if !image.is_file() {
            return Err(format!(
                "--ahci {}: no such image (cargo xtask nvme-root makes one)",
                image.display()
            )
            .into());
        }
        cmd.args(ahci_args(arch, image));
    }
    let Some(image) = NVME.get() else {
        return Ok(());
    };
    if !image.is_file() {
        return Err(format!(
            "--nvme {}: no such image (cargo xtask nvme-root makes one)",
            image.display()
        )
        .into());
    }
    cmd.arg("-drive").arg(format!(
        "if=none,format=raw,file={},id=nvm0",
        image.display()
    ));
    cmd.args(["-device", "nvme,drive=nvm0,serial=EMIBSD0001"]);
    Ok(())
}

/// The QEMU arguments of the `--ahci` disk: `image` on port 1 of q35's AHCI controller on
/// amd64; on arm64, an `ich9-ahci` controller on `virt`'s PCI bus with `image` on port 0.
fn ahci_args(arch: Arch, image: &Path) -> Vec<String> {
    let mut a: Vec<String> = Vec::new();
    let bus = match arch {
        Arch::Amd64 => "ide.1",
        Arch::Arm64 => {
            a.extend(["-device".into(), "ich9-ahci,id=ahci0".into()]);
            "ahci0.0"
        }
    };
    a.extend([
        "-drive".into(),
        format!("if=none,format=raw,file={},id=ahci1", image.display()),
        "-device".into(),
        format!("ide-hd,drive=ahci1,bus={bus}"),
    ]);
    a
}

/// The bytes of a DUID written as 16 hexadecimal digits.
fn parse_duid(hex: &str) -> Result<[u8; 8]> {
    let bad = || format!("--duid {hex}: expected 16 hexadecimal digits");
    if hex.len() != 16 || !hex.is_ascii() {
        return Err(bad().into());
    }
    let mut duid = [0u8; 8];
    for (i, d) in duid.iter_mut().enumerate() {
        *d = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).map_err(|_| bad())?;
    }
    Ok(duid)
}

/// The MBR: one OpenBSD partition from `start`, `sectors` long, the rest of the disk.
pub(crate) fn mbr(start: u64, sectors: u64) -> [u8; 512] {
    let mut s = [0u8; 512];
    let e = &mut s[446..462];
    e[0] = 0x80; // active
    e[1..4].copy_from_slice(&[0xfe, 0xff, 0xff]);
    e[4] = DOSPTYP_OPENBSD;
    e[5..8].copy_from_slice(&[0xfe, 0xff, 0xff]);
    e[8..12].copy_from_slice(&(start as u32).to_le_bytes());
    e[12..16].copy_from_slice(&(sectors as u32).to_le_bytes());
    s[510] = 0x55;
    s[511] = 0xaa;
    s
}

/// `struct disklabel` (`sys/disklabel.h`, little-endian) for a disk of `total` sectors whose
/// partition `a` is the `fs` sectors from [`OPENBSD_START`], with `dkcksum` filled in.
pub(crate) fn disklabel(total: u64, fs: u64, duid: [u8; 8]) -> [u8; 512] {
    let mut l = [0u8; 512];
    let put = |l: &mut [u8; 512], off: usize, b: &[u8]| l[off..off + b.len()].copy_from_slice(b);
    let (nsectors, ntracks) = (63u32, 255u32);
    let secpercyl = nsectors * ntracks;

    put(&mut l, 0, &DISKMAGIC.to_le_bytes()); // d_magic
    put(&mut l, 4, &DTYPE_SCSI.to_le_bytes()); // d_type
    put(&mut l, 8, b"NVMe"); // d_typename
    put(&mut l, 24, b"emibsd root"); // d_packname
    put(&mut l, 40, &(SECTOR as u32).to_le_bytes()); // d_secsize
    put(&mut l, 44, &nsectors.to_le_bytes()); // d_nsectors
    put(&mut l, 48, &ntracks.to_le_bytes()); // d_ntracks
    let ncylinders = (total / u64::from(secpercyl)) as u32;
    put(&mut l, 52, &ncylinders.to_le_bytes()); // d_ncylinders
    put(&mut l, 56, &secpercyl.to_le_bytes()); // d_secpercyl
    put(&mut l, 60, &(total as u32).to_le_bytes()); // d_secperunit
    put(&mut l, 64, &duid); // d_uid
    put(&mut l, 80, &(OPENBSD_START as u32).to_le_bytes()); // d_bstart
    put(&mut l, 84, &(total as u32).to_le_bytes()); // d_bend
    put(&mut l, 112, &((total >> 32) as u16).to_le_bytes()); // d_secperunith
    put(&mut l, 114, &1u16.to_le_bytes()); // d_version
    put(&mut l, 132, &DISKMAGIC.to_le_bytes()); // d_magic2
    put(&mut l, 138, &MAXPARTITIONS.to_le_bytes()); // d_npartitions
    put(&mut l, 140, &BBSIZE.to_le_bytes()); // d_spare2 (was d_bbsize)
    put(&mut l, 144, &SBSIZE.to_le_bytes()); // d_spare3 (was d_sbsize)
    // a: the file system.
    put(&mut l, 148, &(fs as u32).to_le_bytes()); // p_size
    put(&mut l, 152, &(OPENBSD_START as u32).to_le_bytes()); // p_offset
    l[148 + 12] = FS_BSDFFS; // p_fstype
    l[148 + 13] = FFS_FRAGBLOCK; // p_fragblock
    // c: the whole disk.
    put(&mut l, 148 + 2 * 16, &(total as u32).to_le_bytes());

    // dkcksum: the XOR of the 16-bit words up to the end of d_partitions[d_npartitions].
    let end = 148 + 16 * usize::from(MAXPARTITIONS);
    let sum = l[..end]
        .chunks(2)
        .fold(0u16, |s, w| s ^ u16::from_le_bytes([w[0], w[1]]));
    put(&mut l, 136, &sum.to_le_bytes()); // d_checksum
    l
}

/// The file system with its root's fstab line naming `line` (`sd0a`'s by default, see the
/// module docs).
fn rewrite_fstab(fs: &mut [u8], line: &[u8]) -> Result<()> {
    let at: Vec<usize> = fs
        .windows(FSTAB_RD0A.len())
        .enumerate()
        .filter(|(_, w)| *w == FSTAB_RD0A)
        .map(|(i, _)| i)
        .collect();
    let [i] = at[..] else {
        return Err(format!(
            "the file system holds {} copies of the ramdisk's fstab root line, not one",
            at.len()
        )
        .into());
    };
    if line.len() != FSTAB_RD0A.len() {
        return Err("the fstab root line must keep the ramdisk's length".into());
    }
    fs[i..i + line.len()].copy_from_slice(line);
    Ok(())
}

/// `cargo xtask nvme-root`: writes the NVMe root disk (module docs).
pub(crate) fn nvme_root(
    root: &Path,
    arch: Arch,
    duid: Option<&str>,
    out: Option<&str>,
    root_dev: Option<&str>,
) -> Result<()> {
    let duid = parse_duid(duid.unwrap_or(NVME_ROOT_DUID))?;
    let Some(src) = boot::default_ramdisk(root, arch) else {
        return Err(format!(
            "no target/userland/{}/{}: run `just userland` first",
            arch.name(),
            boot::RAMDISK_MODULE
        )
        .into());
    };
    let mut fs = fs::read(&src).map_err(|e| format!("{}: {e}", src.display()))?;
    if !(fs.len() as u64).is_multiple_of(SECTOR) || fs.is_empty() {
        return Err(format!("{}: not a whole number of sectors", src.display()).into());
    }
    rewrite_fstab(&mut fs, &fstab_root_line(root_dev)?)?;

    let fs_sectors = fs.len() as u64 / SECTOR;
    let total = (OPENBSD_START + fs_sectors).div_ceil(ALIGN) * ALIGN;
    let mut disk = vec![0u8; (total * SECTOR) as usize];
    disk[..512].copy_from_slice(&mbr(OPENBSD_START, total - OPENBSD_START));
    let fs_at = (OPENBSD_START * SECTOR) as usize;
    disk[fs_at..fs_at + fs.len()].copy_from_slice(&fs);
    // The label goes in the file system's boot area, which ffs leaves free.
    let label_at = ((OPENBSD_START + LABELSECTOR) * SECTOR) as usize;
    disk[label_at..label_at + 512].copy_from_slice(&disklabel(total, fs_sectors, duid));

    let out = match out {
        Some(o) => boot::run_dir(root).join(o),
        None => boot::run_dir(root).join(format!("nvme-{}.img", arch.name())),
    };
    if let Some(dir) = out.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    fs::write(&out, &disk).map_err(|e| format!("{}: {e}", out.display()))?;
    println!(
        "nvme-root: {} ({total} sectors; a: {fs_sectors} sectors at {OPENBSD_START}, \
         DUID {}, fstab root /dev/{})",
        out.display(),
        duid.iter().map(|b| format!("{b:02x}")).collect::<String>(),
        root_dev.unwrap_or("sd0a")
    );
    Ok(())
}

/// The QEMU arguments of the `--scsi-cd` device on `arch`, in order. A relative `iso` is
/// taken from the workspace `root`.
fn scsi_cd_args(root: &Path, arch: Arch, iso: &Path) -> Vec<String> {
    let iso = if iso.is_absolute() {
        iso.to_path_buf()
    } else {
        root.join(iso)
    };
    let adapter = match arch {
        Arch::Amd64 => "virtio-scsi-pci,id=scsi0",
        Arch::Arm64 => "virtio-scsi-device,id=scsi0",
    };
    vec![
        "-drive".into(),
        format!(
            "if=none,format=raw,file={},id=cd0,media=cdrom,readonly=on",
            iso.display()
        ),
        "-device".into(),
        adapter.into(),
        "-device".into(),
        "scsi-cd,drive=cd0,bus=scsi0.0".into(),
    ]
}

/// The QEMU arguments of the `--lsi` adapter with its disk `image` and, with `--lsi-cd`,
/// the CD-ROM drive holding `cd` (a relative path is taken from the workspace `root`).
fn lsi_args(root: &Path, image: &Path, cd: Option<&Path>) -> Vec<String> {
    let mut a = vec![
        "-device".into(),
        "lsi53c895a,id=lsi0".into(),
        "-drive".into(),
        format!("if=none,format=raw,file={},id=lsihd0", image.display()),
        "-device".into(),
        "scsi-hd,drive=lsihd0,bus=lsi0.0,scsi-id=0".into(),
    ];
    if let Some(cd) = cd {
        let cd = if cd.is_absolute() {
            cd.to_path_buf()
        } else {
            root.join(cd)
        };
        a.push("-drive".into());
        a.push(format!(
            "if=none,format=raw,file={},id=lsicd0,media=cdrom,readonly=on",
            cd.display()
        ));
        a.push("-device".into());
        a.push("scsi-cd,drive=lsicd0,bus=lsi0.0,scsi-id=1".into());
    }
    a
}

/// Makes the `--lsi` disk afresh: `LSI_DISK_BYTES` zeroed bytes at `image`.
fn lsi_fresh(image: &Path) -> Result<()> {
    zeroed_disk(image, LSI_DISK_BYTES)
}

/// Adds the devices that go last on the command line to `cmd` (`--scsi-cd`, then `--lsi`,
/// see the module docs).
pub(crate) fn add_devices(cmd: &mut Command, root: &Path, arch: Arch) -> Result<()> {
    if FB.get().is_some() && arch == Arch::Arm64 {
        cmd.args(["-device", "ramfb"]);
    }
    if let Some(iso) = SCSI_CD.get() {
        cmd.args(scsi_cd_args(root, arch, iso));
    }
    if let Some(image) = LSI.get() {
        if arch != Arch::Amd64 {
            return Err("--lsi: amd64 only (arm64's GENERIC has no siop)".into());
        }
        lsi_fresh(image)?;
        cmd.args(lsi_args(root, image, LSI_CD.get().map(PathBuf::as_path)));
    }
    if let Some(file) = PCI_SERIAL.get() {
        // QEMU truncates a file chardev when it opens it; a stale file would only matter if
        // QEMU died before that.
        let _ = fs::remove_file(file);
        cmd.args(pci_serial_args(file));
    }
    if let Some(dir) = PCI_BRIDGES.get() {
        let (rp, br) = (dir.join("ppb-rp.img"), dir.join("ppb-br.img"));
        for image in [&rp, &br] {
            zeroed_disk(image, PPB_DISK_BYTES)?;
        }
        cmd.args(pci_bridges_args(&rp, &br));
    }
    if let Some(dir) = IPMI.get() {
        if arch != Arch::Amd64 {
            return Err("--ipmi: amd64 only (QEMU's isa-ipmi-kcs needs an ISA bus)".into());
        }
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let sdr = dir.join("ipmi-sdr.bin");
        fs::write(&sdr, IPMI_SDRS).map_err(|e| format!("{}: {e}", sdr.display()))?;
        cmd.args(ipmi_args(&sdr));
    }
    if let Some(&(dev, _)) = TPM.get() {
        let sock = SWTPM_SOCK
            .lock()
            .ok()
            .and_then(|s| s.clone())
            .ok_or("--tpm: swtpm is not running (it is started by `qemu` and `smoke`)")?;
        cmd.args(tpm_args(dev, &sock));
    }
    if machine_pc() {
        cmd.args(pc_boot_disk_args());
    }
    Ok(())
}

/// The QEMU arguments of `--ipmi`: the simulated BMC with the SDR file `sdr`, behind a KCS
/// interface on the ISA bus.
fn ipmi_args(sdr: &Path) -> Vec<String> {
    vec![
        "-device".into(),
        format!("ipmi-bmc-sim,id=bmc0,sdrfile={}", sdr.display()),
        "-device".into(),
        "isa-ipmi-kcs,bmc=bmc0".into(),
    ]
}

/// The QEMU arguments of `--machine pc`'s boot disk (the image the `-drive` with id `hd0`
/// names): an `ich9-ahci` controller with it on port 0. `q35` has the AHCI controller built in
/// (`ide.0`); `pc`'s PIIX3 IDE channel is read by the firmware with programmed I/O, which makes
/// the boot of the 170 MB of kernel and ramdisk Limine loads from it take minutes. The
/// controller goes last, so no other PCI slot moves.
fn pc_boot_disk_args() -> Vec<String> {
    vec![
        "-device".into(),
        "ich9-ahci,id=bootahci".into(),
        "-device".into(),
        "ide-hd,drive=hd0,bus=bootahci.0,bootindex=0".into(),
    ]
}

/// The QEMU arguments of a `--tpm` device `dev` (`tpm-tis`, `tpm-crb`) whose backend is the
/// swtpm listening on `sock`: the socket chardev, the `emulator` TPM backend over it, the
/// device.
fn tpm_args(dev: &str, sock: &Path) -> Vec<String> {
    vec![
        "-chardev".into(),
        format!("socket,id=chrtpm,path={}", sock.display()),
        "-tpmdev".into(),
        "emulator,id=tpm0,chardev=chrtpm".into(),
        "-device".into(),
        format!("{dev},tpmdev=tpm0"),
    ]
}

/// The `swtpm` program: in `$PATH`, or under `brew --prefix swtpm`.
fn swtpm_path() -> Result<PathBuf> {
    let in_path: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).map(|d| d.join("swtpm")).collect())
        .unwrap_or_default();
    if let Some(p) = in_path.into_iter().find(|p| p.is_file()) {
        return Ok(p);
    }
    boot::brew_prefix("swtpm")
        .map(|p| p.join("bin").join("swtpm"))
        .filter(|p| p.is_file())
        .ok_or_else(|| {
            "swtpm not found in $PATH or under `brew --prefix swtpm`; install `swtpm` as in \
             docs/SETUP.md"
                .into()
        })
}

/// A running `swtpm`, the backend of `--tpm`'s device. Dropping it stops swtpm (kill, then
/// wait, so no zombie or orphan is left) and removes its control socket.
pub(crate) struct Swtpm {
    /// The swtpm process.
    child: Child,
    /// Its control socket.
    sock: PathBuf,
}

impl Drop for Swtpm {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_file(&self.sock);
        if let Ok(mut s) = SWTPM_SOCK.lock() {
            *s = None;
        }
    }
}

/// With `--tpm`, starts this run's swtpm (see the module docs) and waits for its control
/// socket; `None` without `--tpm`. The caller keeps the guard until QEMU has exited.
pub(crate) fn start_swtpm() -> Result<Option<Swtpm>> {
    let Some((_, dir)) = TPM.get() else {
        return Ok(None);
    };
    let swtpm = swtpm_path()?;
    let state = dir.join("tpm");
    let _ = fs::remove_dir_all(&state);
    fs::create_dir_all(&state).map_err(|e| format!("{}: {e}", state.display()))?;
    let sock = unix_sock(dir, "swtpm.sock");
    let _ = fs::remove_file(&sock);
    let log_path = dir.join("swtpm.log");
    let log = fs::File::create(&log_path).map_err(|e| format!("{}: {e}", log_path.display()))?;
    let log2 = log
        .try_clone()
        .map_err(|e| format!("{}: {e}", log_path.display()))?;
    let mut cmd = Command::new(&swtpm);
    cmd.args(swtpm_args(&state, &sock))
        .stdin(Stdio::null())
        .stdout(log)
        .stderr(log2);
    println!("xtask: {}", boot::command_line(&cmd));
    let child = cmd.spawn().map_err(|e| {
        format!(
            "{}: {e}; install `swtpm` as in docs/SETUP.md",
            swtpm.display()
        )
    })?;
    // From here on the guard stops swtpm on every path out.
    let mut guard = Swtpm {
        child,
        sock: sock.clone(),
    };
    let started = Instant::now();
    while !sock.exists() {
        if let Some(status) = guard.child.try_wait()? {
            let text = fs::read_to_string(&log_path).unwrap_or_default();
            return Err(
                format!("swtpm exited with {status} before its socket appeared: {text}").into(),
            );
        }
        if started.elapsed() > SWTPM_START_TIMEOUT {
            return Err(format!(
                "swtpm: no control socket {} after {}s",
                sock.display(),
                SWTPM_START_TIMEOUT.as_secs()
            )
            .into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    if let Ok(mut s) = SWTPM_SOCK.lock() {
        *s = Some(sock);
    }
    Ok(Some(guard))
}

/// swtpm's arguments: a TPM 2.0 whose state is in `state`, controlled over the Unix socket
/// `sock` (exiting when that connection is lost), that starts itself (`TPM2_Startup(CLEAR)`)
/// once QEMU initialises it.
fn swtpm_args(state: &Path, sock: &Path) -> Vec<String> {
    vec![
        "socket".into(),
        "--tpm2".into(),
        "--tpmstate".into(),
        format!("dir={}", state.display()),
        "--ctrl".into(),
        format!("type=unixio,path={},terminate", sock.display()),
        "--flags".into(),
        "startup-clear".into(),
    ]
}

/// Makes `image` afresh: `bytes` of zeroes (a sparse file).
fn zeroed_disk(image: &Path, bytes: u64) -> Result<()> {
    if let Some(dir) = image.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let f = fs::File::create(image).map_err(|e| format!("{}: {e}", image.display()))?;
    f.set_len(bytes)
        .map_err(|e| format!("{}: {e}", image.display()))?;
    Ok(())
}

/// The QEMU arguments of `--pci-bridges`: a `pcie-root-port` with a virtio-blk disk holding
/// `rp` behind it, and a `pci-bridge` with one holding `br` in its slot 1.
fn pci_bridges_args(rp: &Path, br: &Path) -> Vec<String> {
    vec![
        "-device".into(),
        "pcie-root-port,id=ppbrp,chassis=1,slot=1".into(),
        "-drive".into(),
        format!("if=none,format=raw,file={},id=ppbrp0", rp.display()),
        "-device".into(),
        "virtio-blk-pci,drive=ppbrp0,bus=ppbrp".into(),
        "-device".into(),
        "pci-bridge,id=ppbbr,chassis_nr=2".into(),
        "-drive".into(),
        format!("if=none,format=raw,file={},id=ppbbr0", br.display()),
        "-device".into(),
        "virtio-blk-pci,drive=ppbbr0,bus=ppbbr,addr=1".into(),
    ]
}

/// The QEMU arguments of the `--pci-serial` card: a `pci-serial` device whose chardev is the
/// file `file` (what the guest sends goes there, nothing is sent to the guest).
fn pci_serial_args(file: &Path) -> Vec<String> {
    vec![
        "-chardev".into(),
        format!("file,id=pcis0,path={}", file.display()),
        "-device".into(),
        "pci-serial,chardev=pcis0".into(),
    ]
}

/// The run directory of the monitor socket, when an option drives QEMU's monitor.
fn monitor_dir() -> Option<&'static PathBuf> {
    SCREENSHOT
        .get()
        .map(|(_, dir)| dir)
        .or_else(|| SENDKEY.get().map(|(_, dir)| dir))
        .or_else(|| MONITOR_CMDS.get().map(|(_, dir)| dir))
}

/// QEMU's `-monitor` argument: a socket in the run directory with `--screenshot-after`,
/// `--sendkey-after` or `--monitor-after`, none otherwise.
pub(crate) fn monitor_arg() -> String {
    match monitor_dir() {
        Some(dir) => {
            let sock = monitor_sock(dir);
            let _ = fs::remove_file(&sock);
            format!("unix:{},server=on,wait=off", sock.display())
        }
        None => "none".into(),
    }
}

/// The longest Unix socket path, NUL included: `sizeof(sun_path)` on macOS (108 on Linux).
const SUN_PATH_MAX: usize = 104;

/// The monitor socket of the run directory `dir`. A Unix socket's path must fit
/// `sun_path` ([`SUN_PATH_MAX`]), which an absolute path into a deep checkout (a worktree
/// under `.claude/worktrees/`, then `target/smoke/<recipe>/`) does not.
fn monitor_sock(dir: &Path) -> PathBuf {
    unix_sock(dir, "monitor.sock")
}

/// The Unix socket `name` of the run directory `dir` ([`unix_sock_for`]).
fn unix_sock(dir: &Path, name: &str) -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_default();
    unix_sock_for(dir, name, &cwd, &std::env::temp_dir(), std::process::id())
}

/// Where the socket `name` (`monitor.sock`, `swtpm.sock`) goes, the first that fits
/// `sun_path`: `name` in `dir` relative to `cwd` (QEMU and swtpm inherit xtask's current
/// directory, so every end resolves it alike), the same path absolute, or
/// `emibsd-<hash of dir>-<pid>-<name>` in `tmp` (removed by [`after_smoke`] and
/// [`Swtpm`]'s `Drop`).
fn unix_sock_for(dir: &Path, name: &str, cwd: &Path, tmp: &Path, pid: u32) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let fits = |p: &Path| p.as_os_str().len() < SUN_PATH_MAX;
    let sock = dir.join(name);
    if let Ok(rel) = sock.strip_prefix(cwd)
        && !rel.as_os_str().is_empty()
        && fits(rel)
    {
        return rel.to_path_buf();
    }
    if fits(&sock) {
        return sock;
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    dir.hash(&mut h);
    tmp.join(format!("emibsd-{:08x}-{pid}-{name}", h.finish() as u32))
}

/// A Unix socket `name` for the run directory `dir`, placed as [`monitor_sock_for`] places
/// the monitor's: relative to the current directory, absolute, or hashed into the temporary
/// directory, whichever fits `sun_path` first.
pub(crate) fn run_sock(dir: &Path, name: &str) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let fits = |p: &Path| p.as_os_str().len() < SUN_PATH_MAX;
    let cwd = std::env::current_dir().unwrap_or_default();
    let sock = dir.join(name);
    if let Ok(rel) = sock.strip_prefix(&cwd)
        && !rel.as_os_str().is_empty()
        && fits(rel)
    {
        return rel.to_path_buf();
    }
    if fits(&sock) {
        return sock;
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    dir.hash(&mut h);
    name.hash(&mut h);
    std::env::temp_dir().join(format!(
        "emibsd-{:08x}-{}-{name}",
        h.finish() as u32,
        std::process::id()
    ))
}

/// Whether the monitor still has work: a screenshot to take, keys or commands to send.
pub(crate) fn monitor_pending() -> bool {
    screenshot_pending() || sendkey_pending() || next_monitor_cmd().is_some()
}

/// Drives the monitor once `serial` has the lines it waits for: [`poll_sendkey`],
/// [`poll_monitor_cmds`], then [`poll_screenshot`].
pub(crate) fn poll_monitor(serial: &str) -> Result<()> {
    poll_sendkey(serial)?;
    poll_monitor_cmds(serial)?;
    poll_screenshot(serial)
}

/// The `AFTER LINE CMD TEXT` pairs of `args` (`--monitor-after LINE --monitor CMD`), in
/// order.
pub(crate) fn parse_pairs(
    args: &[&str],
    after_opt: &str,
    what_opt: &str,
) -> Result<Vec<(String, String)>> {
    let mut pairs = Vec::new();
    let mut after: Option<&str> = None;
    let mut i = 0;
    while i < args.len() {
        if args[i] == after_opt {
            if after.is_some() {
                return Err(format!("{after_opt}: the previous one has no {what_opt}").into());
            }
            after = Some(
                args.get(i + 1)
                    .ok_or_else(|| format!("{after_opt}: expected a line"))?,
            );
            i += 1;
        } else if args[i] == what_opt {
            let line = after
                .take()
                .ok_or_else(|| format!("{what_opt}: needs {after_opt}"))?;
            let what = args
                .get(i + 1)
                .ok_or_else(|| format!("{what_opt}: expected a value"))?;
            pairs.push((line.to_string(), what.to_string()));
            i += 1;
        }
        i += 1;
    }
    if after.is_some() {
        return Err(format!("{after_opt}: needs {what_opt}").into());
    }
    Ok(pairs)
}

/// The next `--monitor` command to send, if any is left.
fn next_monitor_cmd() -> Option<&'static MonitorCmd> {
    let (cmds, _) = MONITOR_CMDS.get()?;
    let sent = SENT_CMDS.lock().map(|s| *s).ok()?;
    cmds.get(sent)
}

/// Sends the next `--monitor` command once `serial` has its `--monitor-after` line.
fn poll_monitor_cmds(serial: &str) -> Result<()> {
    let (Some((after, cmd)), Some((_, dir))) = (next_monitor_cmd(), MONITOR_CMDS.get()) else {
        return Ok(());
    };
    if !serial.contains(after.as_str()) {
        return Ok(());
    }
    monitor_command(&monitor_sock(dir), cmd)?;
    println!("xtask: monitor {cmd} (saw {after:?})");
    if let Ok(mut s) = SENT_CMDS.lock() {
        *s += 1;
    }
    Ok(())
}

/// The `--sendkey-after LINE --sendkeys KEYS` pairs of `args`, in order.
fn parse_sendkeys(args: &[&str]) -> Result<Vec<Sendkeys>> {
    let mut pairs = Vec::new();
    let mut after: Option<&str> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i] {
            "--sendkey-after" => {
                if after.is_some() {
                    return Err("--sendkey-after: the previous one has no --sendkeys".into());
                }
                after = Some(args.get(i + 1).ok_or("--sendkey-after: expected a line")?);
                i += 1;
            }
            "--sendkeys" => {
                let line = after.take().ok_or("--sendkeys: needs --sendkey-after")?;
                let keys = args.get(i + 1).ok_or("--sendkeys: expected keys")?;
                let keys: Vec<String> = keys.split_whitespace().map(str::to_string).collect();
                if keys.is_empty() {
                    return Err("--sendkeys: no keys".into());
                }
                pairs.push((line.to_string(), keys));
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    if after.is_some() {
        return Err("--sendkey-after: needs --sendkeys".into());
    }
    Ok(pairs)
}

/// The next pair to type, if any is left.
fn next_sendkeys() -> Option<&'static Sendkeys> {
    let (pairs, _) = SENDKEY.get()?;
    let sent = SENT_KEYS.lock().map(|s| *s).ok()?;
    pairs.get(sent)
}

/// Whether keys are still to be sent.
fn sendkey_pending() -> bool {
    next_sendkeys().is_some()
}

/// Types the next `--sendkeys` once `serial` has its `--sendkey-after` line: one monitor
/// `sendkey` per key, [`SENDKEY_GAP`] apart.
fn poll_sendkey(serial: &str) -> Result<()> {
    let (Some((after, keys)), Some((_, dir))) = (next_sendkeys(), SENDKEY.get()) else {
        return Ok(());
    };
    if !serial.contains(after.as_str()) {
        return Ok(());
    }
    let sock = monitor_sock(dir);
    let mut mon = UnixStream::connect(&sock).map_err(|e| format!("{}: {e}", sock.display()))?;
    mon.set_read_timeout(Some(Duration::from_secs(10)))?;
    read_prompt(&mut mon)?;
    for key in keys {
        writeln!(mon, "sendkey {key}")?;
        read_prompt(&mut mon)?;
        std::thread::sleep(SENDKEY_GAP);
    }
    println!("xtask: sendkey {} (saw {after:?})", keys.join(" "));
    if let Ok(mut s) = SENT_KEYS.lock() {
        *s += 1;
    }
    Ok(())
}

/// Sends one human-monitor command `cmd` on the monitor socket `sock` and waits for its
/// prompt (`system_powerdown`, ...).
pub(crate) fn monitor_command(sock: &Path, cmd: &str) -> Result<()> {
    let mut mon = UnixStream::connect(sock).map_err(|e| format!("{}: {e}", sock.display()))?;
    mon.set_read_timeout(Some(Duration::from_secs(10)))?;
    read_prompt(&mut mon)?;
    writeln!(mon, "{cmd}")?;
    let reply = read_prompt(&mut mon)?;
    // What the monitor answered beyond the echo of the command and the prompt (`info mice`).
    let text = String::from_utf8_lossy(&reply);
    for line in text.lines().map(|l| l.trim_end_matches('\r').trim()) {
        if !line.is_empty() && !line.contains('\u{1b}') && line != "(qemu)" {
            println!("xtask: monitor> {line}");
        }
    }
    Ok(())
}

/// Whether a screenshot is still to be taken.
pub(crate) fn screenshot_pending() -> bool {
    SCREENSHOT.get().is_some() && SHOT_LINE.lock().map(|s| s.is_none()).unwrap_or(false)
}

/// Takes the screenshot once `serial` has the `--screenshot-after` line: QEMU's
/// `screendump` into `screen.ppm` in the run directory, waited for.
pub(crate) fn poll_screenshot(serial: &str) -> Result<()> {
    let Some((after, dir)) = SCREENSHOT.get() else {
        return Ok(());
    };
    if !screenshot_pending() {
        return Ok(());
    }
    let Some(line) = serial.lines().find(|l| l.contains(after.as_str())) else {
        return Ok(());
    };
    let ppm = dir.join("screen.ppm");
    let _ = fs::remove_file(&ppm);
    let sock = monitor_sock(dir);
    let mut mon = UnixStream::connect(&sock).map_err(|e| format!("{}: {e}", sock.display()))?;
    mon.set_read_timeout(Some(Duration::from_secs(10)))?;
    // The greeting ends with the prompt; the command's reply with the next one.
    read_prompt(&mut mon)?;
    writeln!(mon, "screendump {}", ppm.display())?;
    read_prompt(&mut mon)?;
    let started = Instant::now();
    while !ppm_complete(&ppm) {
        if started.elapsed() > Duration::from_secs(10) {
            return Err(format!("{}: screendump wrote no picture", ppm.display()).into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    println!("xtask: screendump {} (saw {after:?})", ppm.display());
    if SCREEN_TEXT.get().is_some()
        && let Some(grid) = serial.lines().find(|l| l.contains(GRID_PREFIX))
        && let Ok(mut g) = GRID_LINE.lock()
    {
        *g = Some(grid.to_string());
    }
    if let Ok(mut s) = SHOT_LINE.lock() {
        *s = Some(line.to_string());
    }
    Ok(())
}

/// `--screen-text`'s `ROW:COL:TEXT`.
fn parse_screen_text(spec: &str) -> Result<(usize, usize, String)> {
    let mut it = spec.splitn(3, ':');
    let (Some(row), Some(col), Some(text)) = (it.next(), it.next(), it.next()) else {
        return Err(format!("--screen-text {spec:?}: expected ROW:COL:TEXT").into());
    };
    let num = |v: &str| {
        v.parse::<usize>()
            .map_err(|e| format!("--screen-text {spec:?}: {v}: {e}"))
    };
    if text.is_empty() {
        return Err(format!("--screen-text {spec:?}: empty text").into());
    }
    Ok((num(row)?, num(col)?, text.to_string()))
}

/// The `key=value` numbers of the kernel's `selftest: wscons grid` line: x, y, cw, ch, cols,
/// rows.
fn grid_of(line: &str) -> Result<[usize; 6]> {
    let get = |key: &str| -> Result<usize> {
        let v = line
            .split_whitespace()
            .find_map(|w| w.strip_prefix(key).and_then(|w| w.strip_prefix('=')))
            .ok_or_else(|| format!("{line:?}: no {key}="))?;
        v.parse::<usize>()
            .map_err(|e| format!("{line:?}: {key}={v}: {e}").into())
    };
    Ok([
        get("x")?,
        get("y")?,
        get("cw")?,
        get("ch")?,
        get("cols")?,
        get("rows")?,
    ])
}

/// A colour of a character cell and how many of its pixels have it.
type ColourCount = ([u8; 3], usize);

/// The colours of the character cell at (`row`, `col`) of the grid: its background (the
/// commonest colour), the other colours and how many pixels have them.
fn cell_colours(
    ppm: &Ppm<'_>,
    grid: &[usize; 6],
    row: usize,
    col: usize,
) -> Result<([u8; 3], Vec<ColourCount>)> {
    let [x0, y0, cw, ch, cols, rows] = *grid;
    if row >= rows || col >= cols {
        return Err(format!("cell {row},{col} is outside the {cols}x{rows} grid").into());
    }
    let (x, y) = (x0 + col * cw, y0 + row * ch);
    if cw == 0 || ch == 0 || x + cw > ppm.width || y + ch > ppm.height {
        return Err(format!(
            "cell {row},{col} ({cw}x{ch} at {x},{y}) is not inside the {}x{} screen",
            ppm.width, ppm.height
        )
        .into());
    }
    let mut counts: Vec<ColourCount> = Vec::new();
    for py in y..y + ch {
        for px in x..x + cw {
            let o = (py * ppm.width + px) * 3;
            let c = [ppm.rgb[o], ppm.rgb[o + 1], ppm.rgb[o + 2]];
            match counts.iter_mut().find(|(k, _)| *k == c) {
                Some((_, n)) => *n += 1,
                None => counts.push((c, 1)),
            }
        }
    }
    counts.sort_by_key(|&(_, n)| std::cmp::Reverse(n));
    let bg = counts[0].0;
    Ok((bg, counts.split_off(1)))
}

/// Checks a screenshot for `text` at (`row`, `col`) of the grid the kernel's `grid_line`
/// describes (see the module documentation); returns the text pixels it found.
fn check_screen_text(
    ppm: &Ppm<'_>,
    grid_line: &str,
    row: usize,
    col: usize,
    text: &str,
) -> Result<usize> {
    let grid = grid_of(grid_line)?;
    let mut fg: Option<[u8; 3]> = None;
    let mut ink = 0;
    for (i, c) in text.chars().enumerate() {
        let (bg, others) = cell_colours(ppm, &grid, row, col + i)?;
        if c == ' ' {
            if !others.is_empty() {
                return Err(format!("cell {row},{} should be blank", col + i).into());
            }
            continue;
        }
        let [(colour, n)] = others[..] else {
            return Err(format!(
                "cell {row},{} ({c:?}) has {} colours besides its background {:02x?}, expected 1",
                col + i,
                others.len(),
                bg
            )
            .into());
        };
        if fg.is_some_and(|f| f != colour) {
            return Err(format!("cell {row},{} ({c:?}) has another text colour", col + i).into());
        }
        fg = Some(colour);
        ink += n;
    }
    let after = col + text.chars().count();
    if after < grid[4] {
        let (_, others) = cell_colours(ppm, &grid, row, after)?;
        if !others.is_empty() {
            return Err(format!("cell {row},{after} after the text is not blank").into());
        }
    }
    Ok(ink)
}

/// Reads the monitor's output up to its `(qemu) ` prompt.
fn read_prompt(mon: &mut UnixStream) -> Result<Vec<u8>> {
    let mut got = Vec::new();
    let mut buf = [0u8; 512];
    while !got.ends_with(b"(qemu) ") {
        let n = mon
            .read(&mut buf)
            .map_err(|e| format!("qemu monitor: {e}"))?;
        if n == 0 {
            return Err("qemu monitor: closed".into());
        }
        got.extend_from_slice(&buf[..n]);
    }
    Ok(got)
}

/// Whether `path` is a whole binary PPM (its header says how big it is).
fn ppm_complete(path: &Path) -> bool {
    fs::read(path)
        .ok()
        .and_then(|b| {
            parse_ppm(&b)
                .ok()
                .map(|p| p.rgb.len() == p.width * p.height * 3)
        })
        .unwrap_or(false)
}

/// A decoded binary (`P6`) PPM, 8 bits per channel.
struct Ppm<'a> {
    width: usize,
    height: usize,
    rgb: &'a [u8],
}

/// Decodes a `P6` PPM with a maximum value of 255, as QEMU's `screendump` writes it.
fn parse_ppm(b: &[u8]) -> Result<Ppm<'_>> {
    let mut fields = Vec::new();
    let mut i = 0;
    while fields.len() < 4 {
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if b.get(i) == Some(&b'#') {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        let start = i;
        while i < b.len() && !b[i].is_ascii_whitespace() {
            i += 1;
        }
        if start == i {
            return Err("ppm: short header".into());
        }
        fields.push(String::from_utf8_lossy(&b[start..i]).into_owned());
    }
    i += 1; // the one whitespace byte before the pixels
    if fields[0] != "P6" || fields[3] != "255" {
        return Err(format!(
            "ppm: {} with maximum {}, expected P6 and 255",
            fields[0], fields[3]
        )
        .into());
    }
    let num = |s: &str| s.parse::<usize>().map_err(|e| format!("ppm: {s}: {e}"));
    let (width, height) = (num(&fields[1])?, num(&fields[2])?);
    let rgb = b.get(i..).unwrap_or(&[]);
    Ok(Ppm {
        width,
        height,
        rgb: &rgb[..rgb.len().min(width * height * 3)],
    })
}

/// The `key=value` numbers of the kernel's `selftest: fb text at` line.
fn fb_box(line: &str) -> Result<[usize; 5]> {
    let get = |key: &str, radix: u32| -> Result<usize> {
        let v = line
            .split_whitespace()
            .find_map(|w| w.strip_prefix(key).and_then(|w| w.strip_prefix('=')))
            .ok_or_else(|| format!("{line:?}: no {key}="))?;
        usize::from_str_radix(v, radix).map_err(|e| format!("{line:?}: {key}={v}: {e}").into())
    };
    Ok([
        get("x", 10)?,
        get("y", 10)?,
        get("w", 10)?,
        get("h", 10)?,
        get("ink", 10)?,
    ])
}

/// Checks a screenshot against the kernel's line (see the module documentation).
fn check_screenshot(ppm: &Ppm<'_>, line: &str) -> Result<usize> {
    let [x, y, w, h, ink] = fb_box(line)?;
    let colour = |key: &str| -> Result<[u8; 3]> {
        let v = line
            .split_whitespace()
            .find_map(|t| t.strip_prefix(key).and_then(|t| t.strip_prefix('=')))
            .ok_or_else(|| format!("{line:?}: no {key}="))?;
        let n = u32::from_str_radix(v, 16).map_err(|e| format!("{line:?}: {key}={v}: {e}"))?;
        Ok([(n >> 16) as u8, (n >> 8) as u8, n as u8])
    };
    let (fg, bg) = (colour("fg")?, colour("bg")?);
    if w == 0 || h == 0 || x + w > ppm.width || y + h > ppm.height {
        return Err(format!(
            "the text box {w}x{h} at {x},{y} is not inside the {}x{} screen",
            ppm.width, ppm.height
        )
        .into());
    }
    let mut lit = 0;
    for row in y..y + h {
        for col in x..x + w {
            let o = (row * ppm.width + col) * 3;
            let px = [ppm.rgb[o], ppm.rgb[o + 1], ppm.rgb[o + 2]];
            if px == fg {
                lit += 1;
            } else if px != bg {
                return Err(format!(
                    "pixel {col},{row} is {:02x}{:02x}{:02x}, neither the text's nor its background's",
                    px[0], px[1], px[2]
                )
                .into());
            }
        }
    }
    if lit != ink {
        return Err(format!("{lit} text pixels in the box, the glyphs have {ink}").into());
    }
    Ok(lit)
}

/// What a run must leave behind once its serial expectations passed: with
/// `--expect-pci-serial`, each text in the file the card's UART wrote; with
/// `--screenshot-after`, a screenshot that shows the kernel's text.
pub(crate) fn after_smoke() -> Result<()> {
    if let Some((_, dir)) = SENDKEY.get() {
        if SCREENSHOT.get().is_none() {
            let _ = fs::remove_file(monitor_sock(dir));
        }
        if let Some((after, _)) = next_sendkeys() {
            return Err(format!("no keys sent: the serial line {after:?} never came").into());
        }
    }
    if let Some((after, dir)) = SCREENSHOT.get() {
        let _ = fs::remove_file(monitor_sock(dir));
        let line = SHOT_LINE
            .lock()
            .ok()
            .and_then(|s| s.clone())
            .ok_or_else(|| format!("no screenshot: the serial line {after:?} never came"))?;
        let ppm_path = dir.join("screen.ppm");
        let bytes = fs::read(&ppm_path).map_err(|e| format!("{}: {e}", ppm_path.display()))?;
        let ppm = parse_ppm(&bytes)?;
        if let Some((row, col, text)) = SCREEN_TEXT.get() {
            let grid = GRID_LINE
                .lock()
                .ok()
                .and_then(|g| g.clone())
                .ok_or_else(|| {
                    format!("--screen-text: no {GRID_PREFIX:?} line before the screenshot")
                })?;
            let ink = check_screen_text(&ppm, &grid, *row, *col, text)
                .map_err(|e| format!("{}: {e}", ppm_path.display()))?;
            println!(
                "xtask: {}: {}x{} screen shows {text:?} at row {row}, column {col} of wsdisplay's grid ({ink} text pixels)",
                ppm_path.display(),
                ppm.width,
                ppm.height
            );
        } else {
            let lit = check_screenshot(&ppm, &line)
                .map_err(|e| format!("{}: {e}", ppm_path.display()))?;
            println!(
                "xtask: {}: {}x{} screen, the text box has its {lit} glyph pixels and nothing else",
                ppm_path.display(),
                ppm.width,
                ppm.height
            );
        }
    }
    let (Some(file), Some(expect)) = (PCI_SERIAL.get(), PCI_SERIAL_EXPECT.get()) else {
        return Ok(());
    };
    let bytes = fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let text = String::from_utf8_lossy(&bytes);
    for want in expect {
        if !text.contains(want.as_str()) {
            return Err(format!(
                "{}: the card's UART never sent {want:?} (it sent {text:?})",
                file.display()
            )
            .into());
        }
        println!("xtask: {}: the card's UART sent {want:?}", file.display());
    }
    Ok(())
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn machine_pc_boots_from_ahci_not_the_piix_ide_channel() {
        let a = pc_boot_disk_args();
        assert_eq!(
            a,
            [
                "-device",
                "ich9-ahci,id=bootahci",
                "-device",
                "ide-hd,drive=hd0,bus=bootahci.0,bootindex=0"
            ]
        );
        assert!(!machine_pc());
    }

    #[test]
    fn machine_pc_is_amd64_only_and_refuses_what_needs_q35() {
        let a = |extra: &[&'static str]| {
            let mut v = vec!["smoke", "--arch", "amd64", "--machine", "pc"];
            v.extend_from_slice(extra);
            machine_opt(&v)
        };
        assert!(matches!(a(&[]), Ok(true)));
        assert!(matches!(
            machine_opt(&["smoke", "--arch", "amd64"]),
            Ok(false)
        ));
        let err = |r: Result<bool>| r.map(|_| ()).unwrap_err().to_string();
        assert!(
            err(machine_opt(&[
                "smoke",
                "--arch",
                "arm64",
                "--machine",
                "pc"
            ]))
            .contains("amd64 only")
        );
        assert!(err(machine_opt(&["smoke", "--machine", "q35"])).contains("expected pc"));
        assert!(err(machine_opt(&["smoke", "--machine"])).contains("expected a path"));
        assert!(err(a(&["--ahci", "x.img"])).contains("--ahci"));
        assert!(err(a(&["--pci-bridges"])).contains("--pci-bridges"));
        // Without the option the machine is q35.
        assert_eq!(amd64_machine(), "q35");
    }

    #[test]
    fn monitor_pairs_parse_in_order() {
        let args = [
            "--monitor-after",
            "login:",
            "--monitor",
            "system_powerdown",
            "--expect",
            "x",
            "--monitor-after",
            "# ",
            "--monitor",
            "info status",
        ];
        let p = parse_pairs(&args, "--monitor-after", "--monitor").unwrap();
        assert_eq!(
            p,
            vec![
                ("login:".to_string(), "system_powerdown".to_string()),
                ("# ".to_string(), "info status".to_string())
            ]
        );
        assert!(parse_pairs(&["--monitor", "x"], "--monitor-after", "--monitor").is_err());
        assert!(parse_pairs(&["--monitor-after", "x"], "--monitor-after", "--monitor").is_err());
    }

    #[test]
    fn virt_machine_names_gic_and_smmu() {
        assert_eq!(virt_machine_arg(false, 2, false), "virt,acpi=off");
        assert_eq!(virt_machine_arg(true, 2, false), "virt,acpi=on");
        assert_eq!(
            virt_machine_arg(false, 3, false),
            "virt,acpi=off,gic-version=3"
        );
        assert_eq!(
            virt_machine_arg(false, 3, true),
            "virt,acpi=off,gic-version=3,iommu=smmuv3"
        );
        assert_eq!(parse_gic("3", "--gic").ok(), Some(3));
        assert!(parse_gic("4", "--gic").is_err());
    }

    #[test]
    fn monitor_sock_fits_sun_path() {
        let tmp = Path::new("/var/folders/xy/abcdefghijklmnopqrstuvwxyz0123/T");
        let cwd = Path::new(
            "/Users/someone/devel/EmiBSD/.claude/worktrees/agent-ae23d66ba61d717c5-and-more",
        );
        // Inside the checkout: relative, whatever the checkout's depth.
        let dir = cwd.join("target/smoke/smoke-fb");
        let sock = unix_sock_for(&dir, "monitor.sock", cwd, tmp, 4242);
        assert_eq!(sock, Path::new("target/smoke/smoke-fb/monitor.sock"));
        // A short absolute run directory elsewhere stays as it is.
        let sock = unix_sock_for(Path::new("/tmp/run"), "monitor.sock", cwd, tmp, 4242);
        assert_eq!(sock, Path::new("/tmp/run/monitor.sock"));
        // A long one outside the checkout falls back to the temporary directory.
        let long = Path::new("/elsewhere").join("d".repeat(120));
        let sock = unix_sock_for(&long, "monitor.sock", cwd, tmp, 4242);
        assert!(sock.starts_with(tmp), "{}", sock.display());
        assert!(sock.to_string_lossy().ends_with("-4242-monitor.sock"));
        for s in [
            unix_sock_for(&dir, "monitor.sock", cwd, tmp, 4242),
            unix_sock_for(&long, "swtpm.sock", cwd, tmp, u32::MAX),
        ] {
            assert!(s.as_os_str().len() < SUN_PATH_MAX, "{}", s.display());
        }
    }

    #[test]
    fn screen_text_check() {
        // A 2x1 grid of 2x2 cells at 1,1 on a 6x4 screen: cell 0 has one text pixel, cell 1
        // is blank.
        let (w, h) = (6, 4);
        let mut rgb = vec![0u8; w * h * 3];
        let mut set = |x: usize, y: usize, c: [u8; 3]| {
            let o = (y * w + x) * 3;
            rgb[o..o + 3].copy_from_slice(&c);
        };
        set(1, 1, [0xaa, 0xaa, 0xaa]);
        let ppm = Ppm {
            width: w,
            height: h,
            rgb: &rgb,
        };
        let grid = "selftest: wscons grid x=1 y=1 cw=2 ch=2 cols=2 rows=1 on efifb0";
        assert_eq!(check_screen_text(&ppm, grid, 0, 0, "h").expect("shows"), 1);
        assert!(
            check_screen_text(&ppm, grid, 0, 0, "hi").is_err(),
            "cell 1 is blank"
        );
        assert!(check_screen_text(&ppm, grid, 0, 1, "x").is_err(), "no ink");
        assert!(check_screen_text(&ppm, grid, 0, 1, " ").is_ok());
        assert!(
            check_screen_text(&ppm, grid, 1, 0, "h").is_err(),
            "outside the grid"
        );
        assert_eq!(
            parse_screen_text("0:2:a:b").expect("parses"),
            (0, 2, "a:b".to_string())
        );
        assert!(parse_screen_text("0:x:a").is_err());
    }

    #[test]
    fn screenshot_check() {
        // A 4x2 screen: the box is the first two pixels of the second line, one of them lit.
        let mut b = b"P6\n# QEMU\n4 2\n255\n".to_vec();
        let (fg, bg, other) = ([255, 255, 255], [0, 0, 127], [9, 9, 9]);
        for px in [other, other, other, other, fg, bg, other, other] {
            b.extend_from_slice(&px);
        }
        let ppm = parse_ppm(&b).expect("parses");
        assert_eq!((ppm.width, ppm.height, ppm.rgb.len()), (4, 2, 24));
        let line = "selftest: fb text at x=0 y=1 w=2 h=1 ink=1 fg=ffffff bg=00007f";
        assert_eq!(check_screenshot(&ppm, line).expect("matches"), 1);
        let wrong_ink = line.replace("ink=1", "ink=2");
        assert!(check_screenshot(&ppm, &wrong_ink).is_err());
        let stray = line.replace("w=2", "w=3");
        assert!(check_screenshot(&ppm, &stray).is_err());
        let outside = line.replace("x=0", "x=3");
        assert!(check_screenshot(&ppm, &outside).is_err());
        assert!(parse_ppm(b"P5\n1 1\n255\n\0").is_err());
    }
    use crate::e2fs;

    #[test]
    fn sendkeys_pairs_in_order() {
        let args = [
            "--sendkey-after",
            "a-42",
            "--sendkeys",
            "h i ret",
            "--expect",
            "x",
            "--sendkey-after",
            "b-42",
            "--sendkeys",
            "a",
        ];
        let pairs = parse_sendkeys(&args).expect("parses");
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0].0, "a-42");
        assert_eq!(pairs[0].1, ["h", "i", "ret"]);
        assert_eq!(pairs[1].1, ["a"]);
        assert!(parse_sendkeys(&["--sendkeys", "a"]).is_err());
        assert!(parse_sendkeys(&["--sendkey-after", "x"]).is_err());
        assert!(parse_sendkeys(&["--sendkey-after", "x", "--sendkeys", " "]).is_err());
        assert!(parse_sendkeys(&[]).expect("none").is_empty());
    }

    #[test]
    fn pci_serial_is_a_file_chardev() {
        let a = pci_serial_args(Path::new("/run/pcis.txt"));
        assert_eq!(a[1], "file,id=pcis0,path=/run/pcis.txt");
        assert_eq!(a[3], "pci-serial,chardev=pcis0");
    }

    #[test]
    fn pci_bridges_put_a_disk_behind_each_bridge() {
        let a = pci_bridges_args(Path::new("/run/ppb-rp.img"), Path::new("/run/ppb-br.img"));
        assert_eq!(a[1], "pcie-root-port,id=ppbrp,chassis=1,slot=1");
        assert_eq!(a[3], "if=none,format=raw,file=/run/ppb-rp.img,id=ppbrp0");
        assert_eq!(a[5], "virtio-blk-pci,drive=ppbrp0,bus=ppbrp");
        assert_eq!(a[7], "pci-bridge,id=ppbbr,chassis_nr=2");
        assert_eq!(a[9], "if=none,format=raw,file=/run/ppb-br.img,id=ppbbr0");
        assert_eq!(a[11], "virtio-blk-pci,drive=ppbbr0,bus=ppbbr,addr=1");
    }

    #[test]
    fn ipmi_is_a_simulated_bmc_on_kcs() {
        let a = ipmi_args(Path::new("/run/ipmi-sdr.bin"));
        assert_eq!(a[1], "ipmi-bmc-sim,id=bmc0,sdrfile=/run/ipmi-sdr.bin");
        assert_eq!(a[3], "isa-ipmi-kcs,bmc=bmc0");
        // The record's length byte counts what follows the 5-byte header.
        assert_eq!(usize::from(IPMI_SDRS[4]) + 5, IPMI_SDRS.len());
        assert_eq!(IPMI_SDRS[31] & 0x1f, 9);
    }

    #[test]
    fn host_ms_is_expanded() {
        assert_eq!(expand_send("uname -a\n"), "uname -a\n");
        let t = expand_send("h={host-ms}; g={host-ms}\n");
        let ms: Vec<u128> = t
            .trim_end()
            .split("; ")
            .map(|kv| kv[2..].parse().unwrap())
            .collect();
        assert_eq!(ms.len(), 2);
        assert!(ms[0] > 1_600_000_000_000, "{t}");
        assert_eq!(ms[0], ms[1]);
    }

    #[test]
    fn label_is_found_as_the_kernel_finds_it() {
        let total = 131_072;
        let duid = parse_duid(NVME_ROOT_DUID).unwrap();
        assert_eq!(&duid, b"NVMEROOT");
        let m = mbr(OPENBSD_START, total - OPENBSD_START);
        assert_eq!(e2fs::openbsd_mbr_start(&m).unwrap(), OPENBSD_START);
        let l = disklabel(total, 129_024, duid);
        // dkcksum over the label, checksum included, is 0.
        let sum = l[..148 + 16 * 16]
            .chunks(2)
            .fold(0u16, |s, w| s ^ u16::from_le_bytes([w[0], w[1]]));
        assert_eq!(sum, 0);
        assert_eq!(&l[64..72], b"NVMEROOT");
        // Partition a is typed 4.2BSD, so e2fs's ext2 reader refuses it by its type.
        let err = e2fs::ext2_partition(&l, 0).unwrap_err().to_string();
        assert!(err.contains("fstype 7"), "{err}");
    }

    #[test]
    fn fstab_root_line_is_rewritten_once() {
        let mut fs =
            b"xx/dev/rd0a / ffs rw 1 1\n/dev/sd0a /mnt ffs rw,userquota,noauto 1 2\n".to_vec();
        rewrite_fstab(&mut fs, FSTAB_SD0A).unwrap();
        assert!(fs.starts_with(b"xx/dev/sd0a / ffs rw 1 1\n"));
        assert!(rewrite_fstab(&mut fs, FSTAB_SD0A).is_err());
        let mut fs = b"/dev/rd0a / ffs rw 1 1\n".to_vec();
        let line = fstab_root_line(Some("sd2a")).unwrap();
        rewrite_fstab(&mut fs, &line).unwrap();
        assert_eq!(fs, b"/dev/sd2a / ffs rw 1 1\n");
        assert_eq!(fstab_root_line(None).unwrap(), FSTAB_SD0A);
        for bad in ["sd10a", "sd2", "sd2z", "SD2a", "s2aa"] {
            assert!(fstab_root_line(Some(bad)).is_err(), "{bad}");
        }
        assert!(parse_duid("4e564d45524f4f5").is_err());
        assert!(parse_duid("4e564d45524f4fzz").is_err());
    }

    #[test]
    fn scsi_cd_is_a_read_only_cdrom_on_the_arch_s_virtio_bus() {
        let a = scsi_cd_args(Path::new("/r"), Arch::Amd64, Path::new("t/cd.iso"));
        assert_eq!(a[0], "-drive");
        assert!(a[1].contains("file=/r/t/cd.iso"));
        assert!(a[1].contains("media=cdrom,readonly=on"));
        assert_eq!(a[3], "virtio-scsi-pci,id=scsi0");
        assert_eq!(a[5], "scsi-cd,drive=cd0,bus=scsi0.0");
        let b = scsi_cd_args(Path::new("/r"), Arch::Arm64, Path::new("/x/cd.iso"));
        assert!(b[1].contains("file=/x/cd.iso"));
        assert_eq!(b[3], "virtio-scsi-device,id=scsi0");
    }

    #[test]
    fn ahci_disk_is_on_the_second_port() {
        let args = ahci_args(Arch::Amd64, Path::new("/r/target/smoke/x/ahci-amd64.img"));
        assert_eq!(
            args,
            [
                "-drive",
                "if=none,format=raw,file=/r/target/smoke/x/ahci-amd64.img,id=ahci1",
                "-device",
                "ide-hd,drive=ahci1,bus=ide.1"
            ]
        );
    }

    #[test]
    fn arm64_ahci_is_a_controller_with_the_disk_on_port_0() {
        let args = ahci_args(Arch::Arm64, Path::new("/run/ahci-arm64.img"));
        assert_eq!(
            args,
            [
                "-device",
                "ich9-ahci,id=ahci0",
                "-drive",
                "if=none,format=raw,file=/run/ahci-arm64.img,id=ahci1",
                "-device",
                "ide-hd,drive=ahci1,bus=ahci0.0"
            ]
        );
    }

    #[test]
    fn lsi_is_an_lsi53c895a_with_a_disk_and_maybe_a_cdrom() {
        let a = lsi_args(Path::new("/r"), Path::new("/run/lsi.img"), None);
        assert_eq!(a[1], "lsi53c895a,id=lsi0");
        assert!(a[3].contains("file=/run/lsi.img"));
        assert_eq!(a[5], "scsi-hd,drive=lsihd0,bus=lsi0.0,scsi-id=0");
        assert_eq!(a.len(), 6);
        let b = lsi_args(
            Path::new("/r"),
            Path::new("/run/lsi.img"),
            Some(Path::new("t/cd.iso")),
        );
        assert!(b[7].contains("file=/r/t/cd.iso"));
        assert!(b[7].contains("media=cdrom,readonly=on"));
        assert_eq!(b[9], "scsi-cd,drive=lsicd0,bus=lsi0.0,scsi-id=1");
        assert!(opt_path(&["--lsi"], "--lsi").is_err());
        assert_eq!(opt_path(&["--lsi", "x"], "--lsi").unwrap(), Some("x"));
        assert_eq!(opt_path(&["--arch", "amd64"], "--lsi").unwrap(), None);
    }

    #[test]
    fn nic_replaces_vio0_on_the_user_network() {
        assert_eq!(
            user_nic_arg(Some("e1000e"), Arch::Amd64, ""),
            "e1000e,netdev=n0"
        );
        assert_eq!(
            user_nic_arg(Some("igb"), Arch::Arm64, ",mac=52:54:00:12:34:57"),
            "igb,netdev=n0,mac=52:54:00:12:34:57"
        );
        assert_eq!(
            user_nic_arg(None, Arch::Arm64, ""),
            "virtio-net-device,netdev=n0"
        );
        assert_eq!(
            user_nic_arg(Some("vmxnet3"), Arch::Arm64, ""),
            "vmxnet3,netdev=n0"
        );
        assert!(set(Path::new("/r"), &["--nic", "vmxnet3"]).is_ok());
        assert!(set(Path::new("/r"), &["--nic", "ne2k_pci"]).is_err());
        assert!(set(Path::new("/r"), &["--nic"]).is_err());
        assert!(set(Path::new("/r"), &["--nic", "e1000", "--vio-mq"]).is_err());
    }

    #[test]
    fn tpm_is_swtpm_behind_a_q35_device() {
        let sock = Path::new("target/smoke/smoke-tpm/swtpm.sock");
        assert_eq!(
            tpm_args("tpm-tis", sock),
            [
                "-chardev",
                "socket,id=chrtpm,path=target/smoke/smoke-tpm/swtpm.sock",
                "-tpmdev",
                "emulator,id=tpm0,chardev=chrtpm",
                "-device",
                "tpm-tis,tpmdev=tpm0",
            ]
        );
        let args = swtpm_args(Path::new("/r/tpm"), sock);
        assert_eq!(&args[..2], ["socket", "--tpm2"]);
        assert!(args.contains(&"dir=/r/tpm".to_string()));
        assert!(
            args.contains(
                &"type=unixio,path=target/smoke/smoke-tpm/swtpm.sock,terminate".to_string()
            )
        );
        assert!(args.contains(&"startup-clear".to_string()));
        assert!(set(Path::new("/r"), &["smoke", "--tpm", "tpm20"]).is_err());
        assert!(set(Path::new("/r"), &["smoke", "--tpm"]).is_err());
        assert!(
            set(
                Path::new("/r"),
                &["smoke", "--arch", "arm64", "--tpm", "tis"]
            )
            .is_err()
        );
        assert!(
            set(
                Path::new("/r"),
                &["smoke2", "--arch", "amd64", "--tpm", "tis"]
            )
            .is_err()
        );
        assert!(
            TPM_MODELS
                .iter()
                .any(|&(m, d)| m == "crb" && d == "tpm-crb")
        );
    }

    #[test]
    fn iommu_is_a_q35_device() {
        assert_eq!(iommu_args_for(None), Vec::<String>::new());
        assert_eq!(iommu_args_for(Some("amd-iommu")), ["-device", "amd-iommu"]);
        assert!(set(Path::new("/r"), &["--iommu", "via"]).is_err());
        assert!(set(Path::new("/r"), &["--iommu"]).is_err());
        assert!(set(Path::new("/r"), &["--arch", "arm64", "--iommu", "intel"]).is_err());
        assert!(set(Path::new("/r"), &["--arch", "amd64", "--iommu", "smmuv3"]).is_err());
        assert!(
            IOMMU_MODELS
                .iter()
                .any(|&(m, a, d)| m == "smmuv3" && a == Arch::Arm64 && d.is_empty())
        );
        assert!(
            IOMMU_MODELS
                .iter()
                .any(|&(m, _, d)| m == "intel" && d == "intel-iommu")
        );
    }

    #[test]
    fn scsi_cd_needs_a_path() {
        assert!(set(Path::new("/r"), &["--scsi-cd"]).is_err());
        assert!(set(Path::new("/r"), &["--arch", "arm64"]).is_ok());
    }
}
/* </TESTS> */
