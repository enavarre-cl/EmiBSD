# EmiBSD

<p align="center">
  <img src="docs/Ferffy.jpg" alt="Ferffy, the EmiBSD mascot: a spiky orange crab in front of a shield, above the EmiBSD name" width="380">
</p>

<p align="center"><em>Ferffy, the EmiBSD mascot: half Ferris (Rust's crab), half Puffy (OpenBSD's pufferfish).</em></p>

<p align="center"><strong>The OpenBSD kernel, re-implemented in Rust, one file at a time.</strong></p>

<p align="center">License: ISC · Rust: stable (1.98.1) · Targets: amd64, arm64 · Runs in: QEMU</p>

> **Not for production.** EmiBSD is an experiment. It runs only in QEMU, and its crypto,
> network and storage code is unaudited. Do not use it to protect real data or real networks.

## What is this

- A file-by-file port of the OpenBSD kernel, pinned to commit `3ce1f3f79392` of
  [openbsd/src](https://github.com/openbsd/src) (`reference/PINNED.md`).
- Not a new kernel design, not a Rust-for-Linux style hybrid, not a wrapper around C.
- The C sources are the specification. Names, structure and semantics stay OpenBSD's.
  Every deviation is written down, in the file and in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).
- A standalone `#![no_std]` kernel for amd64 and arm64, booted by Limine, run in QEMU.

## Status

Status: M12+ (measurement and verification: unsafe-report, JOURNAL, diff-openbsd) met; M13
(storage, firmware and console) under way.

| Milestone | Scope | State |
|---|---|---|
| M0 | Toolchain and boot | met |
| M1 | libkern and `sys/sys` | met |
| M2 | Console, printf, panic, ddb-lite | met |
| M3 | Physical memory and uvm basics | met |
| M4 | Traps and interrupts | met |
| M5 | Timers, scheduler, proc | met |
| M6 | System calls and a minimal init | met |
| M7a, M7b | uvm (demand paging); mbufs, virtio-net, IPv4 ping | met |
| M8, M8b | OpenBSD's userland on an ffs ramdisk; multi-user boot and login | met |
| M9a..M9d | Sockets, WireGuard, IPsec, pf | met |
| M9+ | Network completion: TCP, bpf, divert, IPComp, HTTPS, tcpdump, INET6 | met |
| M10a | Persistent disk (vioblk, SCSI midlayer) | met |
| M10b | UFS options (quotas, dirhash, mfs) | met |
| M10c | Memory and removable file systems (tmpfs, msdosfs, cd9660, udf, vnd) | met |
| M10f | softraid (RAID 0, 1, 5, 6, concat, RAID 1C, CRYPTO; bio(4), bioctl) | met |
| M10e | NFS client and server (portmap, mountd, nfsd, mount_nfs, showmount) | met |
| M10d | ext2fs, ntfs (amd64), fuse | met |
| M11a | MP bring-up: APs started through Limine, the kernel lock, per-CPU run queues, SMR, percpu and pool caches, IPIs and TLB shootdowns | met |
| M11b | MP timekeeping: the TSC synchronisation test per AP, clock interrupts on every CPU | met |
| M11c | ddb on MP: the command loop, the other CPUs stopped by IPI, `machine cpuinfo`, `machine ddbcpu` | met |
| M11d | Network parallelism: one softnet task queue per CPU (up to 8), `kern_intrmap.c`, SMR for the interface index | met |
| M11e | The MP audit: every `MULTIPROCESSOR` site, MPSAFE flags and `SY_NOLOCK` honoured, unlocked page faults; every smoke runs on four CPUs | met |
| M12 | Devices in QEMU: audio(4) with azalia and auich, USB with xhci, uhub, umass and ukbd; arm64's PCI bus | met |
| M12+ | Measurement and verification: unsafe-report, JOURNAL, diff-openbsd against a real OpenBSD | met |
| M13 | Storage, firmware and console | next |
| M14 | Installable | next |
| M15 | Code and test layout | next |
| M16 | QEMU drivers: IDE, floppy, PS/2, parallel, PC speaker, more USB, virtio, network, SCSI/RAID, UFS, SD, audio, IOMMUs, GPIO, GICv3 | next |
| M17 | Real hardware and virtualisation (vmm, vmd; optional) | next |

Stage 2 of the diagnostic tools (ps, fstat, vmstat, df) is also met. Exit criteria and dates are
in [docs/ROADMAP.md](docs/ROADMAP.md); the current state is in
[docs/STATUS.md](docs/STATUS.md).

## What works today

Every line below is a recipe of `just smoke`, run on both architectures, on the
`multiprocessor` kernel with four processors (`-smp 4`); `smoke-up` boots the uniprocessor
kernel once per arch.

On one VM, with OpenBSD's own binaries from the ramdisk:

- Boot, autoconf, kernel self-tests, the ddb(4) prompt at a `-d` stop, a deliberate panic with
  a stack trace (`smoke`).
- init(8) and ksh(1) in single-user mode (`smoke-shell`).
- `/etc/rc`, getty(8), login(1) as root, the clock from the RTC (`smoke-login`).
- ifconfig(8), ping(8), route(8) over the routing socket (`smoke-net`, `smoke-route`).
- ps(1), fstat(1), vmstat(8), df(1), mount(8) over sysctl(2) (`smoke-diag`).
- pfctl(8) loading a ruleset that blocks a ping (`smoke-pf`); ipsecctl(8) over PF_KEY (`smoke-ipsec`).
- ftp(1) and nc(1) over TLS with LibreSSL, against servers on the host (`smoke-https`).
- A persistent disk: `sd0` on vioblk(4), fdisk(8), disklabel(8), newfs(8); after a second boot
  fsck(8) finds it clean and the file reads back (`smoke-disk`).
- Disk quotas: quotacheck(8), quotaon(8), edquota(8); a write as a user over its hard limit
  fails with EDQUOT and repquota(8) shows it; a hashed 5,000-entry directory; mount_mfs(8)
  (`smoke-ufsopts`).
- tmpfs(5) on /tmp; FAT, ISO 9660 and UDF images attached with vnconfig(8) and mounted with
  mount_msdos(8), mount_cd9660(8), mount_udf(8); newfs_msdos(8) on a vnd(4) over a tmpfs file
  and fsck_msdos(8) passing it (`smoke-fs`).
- softraid(4) over four vioblk disks: RAID 0, 1, 5, concat, RAID 1C and CRYPTO made with
  bioctl(8), RAID 6 with our own `sr6create` (bioctl has no `-c 6`); each gets an ffs and a
  file; after a reboot the volumes are assembled at boot, `bioctl -p` unlocks the encrypted
  ones and every file reads back; with a disk missing, RAID 1 and RAID 6 come up degraded
  and still read (`smoke-softraid`).
- ext2fs: newfs_ext2fs(8) on a persistent disk, files written with mount_ext2fs(8); after a
  reboot fsck_ext2fs(8) finds it clean and the files read back, and e2fsprogs' `e2fsck -fn` on
  the Mac passes the same disk image (`smoke-ext2fs`).
- FUSE: our own file system over OpenBSD's libfuse and /dev/fuse0 mounts, serves its files,
  refuses a write and unmounts (`smoke-fuse`).
- NTFS, amd64 only (as in GENERIC): an image made by our own generator, checked first by
  macOS's NTFS driver, attached with vnconfig(8) and mounted with mount_ntfs(8); a resident
  and a non-resident file read back (`smoke-ntfs`).
- Four processors (`-smp 4`) with the `multiprocessor` kernel: the application processors
  start, take IPIs and TLB shootdowns, two kernel threads ping-pong across CPUs, a thread per
  CPU stresses the pools, the page allocator and page faults through uvm, and the init
  self-test passes; amd64 tests
  each application processor's TSC against the boot CPU's, and on both archs every CPU runs
  its own clock interrupts with an uptime that never goes back (`smoke-mp`).
- ddb(4) on four processors: `sysctl ddb.trigger=1` from the shell stops every other CPU by
  IPI, `machine ddbcpu 1` moves the debugger to CPU 1, `machine cpuinfo` shows the other three
  stopped, and `continue` resumes them all (`smoke-ddbmp`).
- Audio: aucat(1) plays a tone through `/dev/audio0` on Intel HD Audio (azalia(4)) on both
  archs and on AC97 (auich(4)) on amd64; audioctl(8) and mixerctl(8) show and set the
  device, and QEMU's WAV capture must hold the tone (`smoke-audio`).
- USB: xhci(4) and uhub(4) enumerate QEMU's stick and keyboard; umass(4) makes the stick an
  sd(4) disk whose FAT partition mount_msdos(8) mounts, reads, writes and compares after a
  remount; uhidev(4) and ukbd(4) attach the keyboard (`smoke-usb`).

Between two VMs on a private link (`cargo xtask smoke2`):

- ping across the link (`smoke-link`); a wg(4) tunnel, with a pf rule on `wg0` (`smoke-wg`).
- An ESP tunnel (`smoke-esp`); IPComp inside ESP (`smoke-ipcomp`).
- pfsync(4) and pflow(4) (`smoke-pfsync`); pf `divert-to` (`smoke-divert`).
- TCP with nc(1): directly, through `wg0` and through ESP (`smoke-tcp`).
- tcpdump(8) on `vio1` and on `pflog0` (`smoke-tcpdump`).
- IPv6: ping(8) as ping6 to the other VM's global and link-local addresses, ::1 on lo0 (`smoke-inet6`).
- NFS over UDP and TCP with OpenBSD's portmap(8), mountd(8), nfsd(8), mount_nfs(8) and
  showmount(8): one VM exports a directory, the other lists and mounts it, reads a file and
  writes files the first one reads (`smoke-nfs`).
- Both VMs on the `multiprocessor` kernel with `-smp 4`: four softnet threads (eight with
  `-smp 8`), a ping across the link and through `wg0`, TCP with nc(1) directly and through
  `wg0`, loopback interfaces created and destroyed (`smoke-net-mp`).
- tcpbench(1) both ways at once, four connections each, for 15 seconds (`smoke-tcpbench`).

An excerpt of the serial console, from `smoke-login` on amd64 (trimmed):

```
bsd: booted on amd64 by Limine 12.9.1
EmiBSD 8.0 (GENERIC) #221: Sun Oct  4 07:56:52 UTC 2026
root on rd0a swap on rd0b dump on rd0b
rc: multi-user
EmiBSD/amd64 (Amnesiac) (tty00)

login: root
Password:
Welcome to EmiBSD 8.0: OpenBSD's init(8) and ksh(1) on an ffs ramdisk root.
# uname -a
EmiBSD  8.0 GENERIC#221 amd64
```

And from `smoke-net`, another boot (trimmed):

```
# ifconfig vio0
vio0: flags=4008843<UP,BROADCAST,RUNNING,SIMPLEX,MULTICAST> mtu 1500
        lladdr 52:54:00:12:34:56
        inet 10.0.2.15 netmask 0xffffff00 broadcast 10.0.2.255
# ping -c 1 10.0.2.2
64 bytes from 10.0.2.2: icmp_seq=0 ttl=255 time=5.278 ms
```

And from `smoke-fuse` on amd64 (trimmed):

```
# mkdir -p /fuse && fusehello /fuse && mount && echo fuse-up-$((40+2))
/dev/rd0a on / type ffs (local)
fusefs on /fuse type fuse
fuse-up-42
# cat /fuse/hello.txt /fuse/sub/deep.txt
m10d-fuse-42
m10d-fuse-sub-42
```

And from `smoke-mp`, the `multiprocessor` kernel with `-smp 4` on amd64 (trimmed; the last
three lines come from its `selftest=mpstress` boot):

```
bsd: 4 processors, boot processor hwid 0x0
cpu1 at mainbus0: apid 1 (application processor)
tsc: cpu0/cpu1: sync test passed
cpu3 at mainbus0: apid 3 (application processor)
tsc: cpu0/cpu3: sync test passed
x86_ipi_selftest: X86_IPI_NOP taken by 3 cpus, tlb shootdowns acknowledged
selftest: 4 cpus running
selftest: cpu1 clockintr: 20 uptime checks, 0 behind
selftest: clockintr on 4 cpus ok, uptime monotonic on each
selftest: mpstress pool ok (4 cpus, 432000 gets, 431592 through the per-cpu caches, 71904 items exchanged between threads, 0 PR_NOWAIT refused, 30 pages reclaimed, 485 ms)
selftest: mpstress pmemrange ok (4 cpus, 14400 page lists, 88389 pages, 0 UVM_PLA_NOWAIT refused, 95997 pages free before and after; 96108 free at the start, 96101 at the end)
selftest: mpstress uvm ok (4 cpus, 9988 pageable kernel pages and 16000 anonymous pages faulted in, 4000 slices unmapped and mapped again, per-cpu page caches 26017 hits 3718 misses, 378 ms)
```

And from `smoke-tcpbench`, VM a of two MP VMs on amd64 (trimmed):

```
# until tcpbench -n 4 -t 15 192.168.77.2; do sleep 1; done; echo bench-a-$((5+5))
Conn:   4 Mbps:      367.652 Peak Mbps:      367.652 Avg Mbps:       91.913
Conn:   4 Mbps:      241.680 Peak Mbps:      367.652 Avg Mbps:       60.420
--- 192.168.77.2 tcpbench statistics ---
445265160 bytes sent over 15.352 seconds
bandwidth min/avg/max/std-dev = 127.436/232.681/367.652/65.268 Mbps
bench-a-10
```

And from `smoke-ddbmp`, ddb on the same kernel and four processors, amd64 (trimmed):

```
# sysctl ddb.trigger=1
Stopped at      0xffffffff802954aa
ddb{0}> machine ddbcpu 2
Stopped at      0xffffffff80195655
ddb{2}> machine ddbcpu 1
Stopped at      0xffffffff80195655
ddb{1}> machine cpuinfo
    0: stopped
*   1: ddb
    2: stopped
    3: stopped
ddb{1}> continue
ddb.trigger: 0 -> 1
```

And from `smoke-net-mp`, VM A on amd64 with `-smp 4` (trimmed):

```
# ifconfig lo3 destroy && echo if-destroyed-$((3+3))
if-destroyed-6
# ps -axk -o pid,cpuid,comm
  PID    CPUID COMMAND
97671        1 softnet0
13175        1 softnet1
72457        1 softnet2
92012        1 softnet3
81307        2 smr
38591        3 wg_crypt
softnets-4
```

And from `smoke-usb` and `smoke-audio` on amd64 (trimmed):

```
xhci0 at pci0 dev 4 function 0 vendor 0x1b36 product 0x000d rev 0x01: irq 10, xHCI 1.0
usb0 at xhci0: USB revision 3.0
umass0 at uhub0 port 1 configuration 1 interface 0 "QEMU QEMU USB HARDDRIVE" rev 3.00/0.00 addr 2
sd1 at scsibus1 targ 1 lun 0: <QEMU, QEMU HARDDISK, 2.5+> serial.46f4000100:00:04.0-1
ukbd0 at uhidev0
emibsd m12: hello from a usb stick
4071711340 1048576 /mnt/BIG.BIN
azalia0 at pci0 dev 4 function 0 vendor 0x8086 product 0x2668 rev 0x01: irq 10
audio0 at azalia0
outputs.master=126,126
# aucat -i /root/tone.wav && echo tone-$((40+2))
tone-42
```

The real console also prints `unported: <name>` lines. Each one is a known gap, reported once.

## Quick start (macOS)

You need:

- Homebrew `rustup`, `qemu`, `limine` and `just`. The pinned toolchain installs itself.
- For the userland: Apple clang and an LLD 17 or newer with OpenBSD support.
- The sparse clone of the OpenBSD sources in `reference/openbsd-src` ([reference/README.md](reference/README.md)).

Every step is in [docs/SETUP.md](docs/SETUP.md). Then:

```sh
just build        # kernel and the Rust init, amd64 and arm64
just userland     # OpenBSD's libc and programs, and the ffs ramdisk (slow)
just run-amd64    # boot in QEMU, serial console on stdio
just run-arm64
just smoke        # every boot test, headless, both architectures
```

The test image's root password is in docs/SETUP.md ("The test image's login").
`just` with no arguments lists every recipe.

## How it's built

- `sys/` mirrors OpenBSD's `sys/` path for path: `kern/tty.c` becomes `sys/kern/tty.rs`.
  Types live where the C header is; functions live where the C file is.
- Generic code reaches the hardware only through `crate::machine`, a set of traits that stands in
  for `<machine/*.h>`. Each arch implements them; the compiler checks it.
- One kernel crate, `bsd`, because kern, uvm and arch call each other. Only true leaves
  (`libkern`, `libz`) are separate crates.
- The userland is OpenBSD's C, cross-compiled unmodified with clang, on an ffs ramdisk
  made by OpenBSD's own makefs(8). So the kernel must speak OpenBSD's system call ABI exactly.
- `#![no_std]`, stable Rust only. No `static mut`. Every `unsafe` block has a `// SAFETY:` comment.
- Gaps are explicit: `unported!()` yields `ENOSYS` and says so on the console. No `todo!()`.

Against a real OpenBSD (`just diff-openbsd`, beside `just ci`): the same 102 steps (291 system
call probes, file-system operations through OpenBSD's own utilities) on EmiBSD and on the
OpenBSD 8.0 snapshot nearest the pin, on both archs: 99 equal, 3 expected differences
(fifofs, core dumps, branding).

Details, boot flow and deviations: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Porting progress

From `cargo xtask ports status` at the commit of this README:

| todo | wip | ported | skipped | total |
|---:|---:|---:|---:|---:|
| 5 | 138 | 709 | 16 | 868 |

The tracker lists the files claimed by the milestones so far, not all of OpenBSD's `sys/`.
`wip` files are in use with visible stubs. Per subsystem: [docs/PORTING.md](docs/PORTING.md).
Source of truth: [ports.toml](ports.toml).

## Testing

Four tiers:

1. Host unit tests (`just test`): pure logic runs on macOS through `sys/arch/host`.
2. Reference-backed tests (`just test-ref`): constants are cross-checked against the C headers.
3. QEMU smoke tests (`just smoke`): boot both architectures headless and assert serial lines and
   exit codes. A full run boots 67 single VMs and 24 pairs of VMs, all on the
   `multiprocessor` kernel with `-smp 4` except `smoke-up`'s uniprocessor boot per arch.
   The recipes run four at a time, each in its own `target/smoke/<recipe>/` with its own log;
   `JOBS=N just smoke` changes N.
4. Differential tests (`just diff-openbsd`, beside `just ci`): the same scenarios on EmiBSD and
   on a real OpenBSD VM (the -current snapshot nearest the pin, installed once with
   autoinstall(8) under `target/openbsd/`), compared step by step; every difference is fixed
   or listed with its reason in `tools/xtask/diff-openbsd/expected.toml`. About two minutes
   for both archs once installed.

`just ci` runs fmt, clippy for amd64, arm64 and the host, all tests, both builds, every smoke and
the tracker checks. Green `just ci` is the definition of done.
Rules: [.claude/rules/testing.md](.claude/rules/testing.md).

## Repository layout

```
reference/openbsd-src/  OpenBSD sources, sparse clone, gitignored, read-only
sys/                    the kernel (package `bsd`), mirroring OpenBSD's sys/
  kern/ uvm/ dev/ net/ netinet/ crypto/ ufs/ ddb/
  sys/                  header types
  machine/              the <machine/*.h> contract (traits)
  arch/{amd64,arm64}/   per-arch code
  arch/host/            std-backed test double for `cargo test`
  stand/                Limine boot glue (replaces boot(8) for now)
  lib/libkern/ lib/libz/
init/                   the Rust init, now the kernel's self-test
tools/xtask/            images, QEMU, smoke tests, userland build, ports tracker
docs/                   architecture, porting, roadmap, setup, status
ports.toml              porting tracker
```

## Documentation

| Question | Where |
|---|---|
| How do I set up the toolchain on macOS? | [docs/SETUP.md](docs/SETUP.md) |
| How do I get the OpenBSD sources? | [reference/README.md](reference/README.md) |
| Why is it built this way? What deviates from OpenBSD? | [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) |
| How does a C file become a Rust file? | [docs/PORTING.md](docs/PORTING.md), tracker in [ports.toml](ports.toml) |
| How is this C idiom written in Rust? | [docs/C_TO_RUST.md](docs/C_TO_RUST.md) |
| What comes next? | [docs/ROADMAP.md](docs/ROADMAP.md) |
| What happens after the faithful port? | [docs/PHASE2.md](docs/PHASE2.md) (draft) |
| Where are we right now? | [docs/STATUS.md](docs/STATUS.md) |
| What rules does every change follow? | [CLAUDE.md](CLAUDE.md), [.claude/rules/](.claude/rules/) |

## Contributing and workflow

Porting one file:

1. Pick: `cargo xtask ports next` lists `todo` files whose dependencies are done.
2. Read the `.c`, its headers and the man pages it cites, completely.
3. Write `sys/<same path>.rs`: the original licence block, an `Upstream:` line, a `Deviations` list.
4. Test: host tests in the same file; `just ci` green.
5. Record: mark the file `ported` in `ports.toml`, then commit.

One commit per file or coherent cluster, with a trailer per ported C file:

```
kern: port subr_prf.c (printf, panic)

Upstream: sys/kern/subr_prf.c@3ce1f3f79392
```

The full process is in [docs/PORTING.md](docs/PORTING.md).

Feedback is welcome: [GitHub Discussions](https://github.com/enavarre-cl/EmiBSD/discussions) for
feedback and design questions, [Issues](https://github.com/enavarre-cl/EmiBSD/issues) for concrete
problems. External pull requests are not accepted, for now.

## Mascot

Ferffy is Ferris, Rust's crab, crossed with Puffy, OpenBSD's pufferfish.
The spikes are Puffy's; the claws are Ferris's.

## License

New code is under the ISC license. Ported files keep their OpenBSD copyright notice and license,
whole, at the top of the file. Every license in the pinned OpenBSD tree is accepted.
The ramdisk's userland keeps the licenses of its sources. See [LICENSE](LICENSE).

## Acknowledgements

- The OpenBSD project and its authors, whose code is the specification. EmiBSD is not affiliated
  with or endorsed by OpenBSD; their names are not used to promote it (BSD-3-Clause, clause 3).
- [Limine](https://github.com/limine-bootloader/limine), the boot loader.
- [QEMU](https://www.qemu.org/), where every test runs.
- The Rust project and its ecosystem: the compiler, cargo, clippy, rustfmt and rust-analyzer.
