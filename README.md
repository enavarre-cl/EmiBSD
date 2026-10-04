# EmiBSD

<p align="center">
  <img src="docs/Ferffy.jpg" alt="Ferffy, the EmiBSD mascot: a spiky orange crab in front of a shield, above the EmiBSD name" width="380">
</p>

<p align="center"><em>Ferffy, the EmiBSD mascot: half Ferris (Rust's crab), half Puffy (OpenBSD's pufferfish).</em></p>

<p align="center"><strong>The OpenBSD kernel, re-implemented in Rust, one file at a time.</strong></p>

<p align="center">License: ISC · Rust: stable (1.98.1) · Targets: amd64, arm64 · Runs in: QEMU</p>

## What is this

- A file-by-file port of the OpenBSD kernel, pinned to commit `3ce1f3f79392` of
  [openbsd/src](https://github.com/openbsd/src) (`reference/PINNED.md`).
- Not a new kernel design, not a Rust-for-Linux style hybrid, not a wrapper around C.
- The C sources are the specification. Names, structure and semantics stay OpenBSD's.
  Every deviation is written down, in the file and in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).
- A standalone `#![no_std]` kernel for amd64 and arm64, booted by Limine, run in QEMU.

## Status

Status: M11a (MP bring-up, four CPUs per VM) met; M11b (MP timekeeping) next.

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
| M11b..M11e | SMP: MP timekeeping, ddb on MP, network parallelism, the MP audit | next |
| M12 | Devices (audio, USB), in QEMU | next |
| M13 | Storage, firmware and console | next |
| M14, M14b | Installable; code and test layout | next |
| M15 | Real hardware and virtualisation (vmm, vmd; optional) | next |

Stage 2 of the diagnostic tools (ps, fstat, vmstat, df) is also met. Exit criteria and dates are
in [docs/ROADMAP.md](docs/ROADMAP.md); the current state is in
[docs/STATUS.md](docs/STATUS.md).

## What works today

Every line below is a recipe of `just smoke`, run on both architectures.

On one VM, with OpenBSD's own binaries from the ramdisk:

- Boot, autoconf, kernel self-tests, ddb-lite, a deliberate panic with a stack trace (`smoke`).
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
  CPU stresses the pools and the page allocator, and the init self-test passes (`smoke-mp`).

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

And from `smoke-mp`, the `multiprocessor` kernel with `-smp 4` on amd64 (trimmed):

```
bsd: 4 processors, boot processor hwid 0x0
cpu1 at mainbus0: apid 1 (application processor)
cpu3 at mainbus0: apid 3 (application processor)
x86_ipi_selftest: X86_IPI_NOP taken by 3 cpus, tlb shootdowns acknowledged
selftest: 4 cpus running
selftest: mpstress pool ok (4 cpus, 432000 gets, 431588 through the per-cpu caches, 71904 items exchanged between threads, 0 PR_NOWAIT refused, 29 pages reclaimed, 251 ms)
selftest: mpstress pmemrange ok (4 cpus, 14400 page lists, 88389 pages, 0 UVM_PLA_NOWAIT refused, 95995 pages free before and after; 96108 free at the start, 96077 at the end)
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

Details, boot flow and deviations: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Porting progress

From `cargo xtask ports status` at the commit of this README:

| todo | wip | ported | skipped | total |
|---:|---:|---:|---:|---:|
| 4 | 143 | 625 | 16 | 788 |

The tracker lists the files claimed by the milestones so far, not all of OpenBSD's `sys/`.
`wip` files are in use with visible stubs. Per subsystem: [docs/PORTING.md](docs/PORTING.md).
Source of truth: [ports.toml](ports.toml).

## Testing

Three tiers:

1. Host unit tests (`just test`): pure logic runs on macOS through `sys/arch/host`.
2. Reference-backed tests (`just test-ref`): constants are cross-checked against the C headers.
3. QEMU smoke tests (`just smoke`): boot both architectures headless and assert serial lines and
   exit codes. A full run boots 48 single VMs and 20 pairs of VMs.

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

External contributions are not accepted, for now.

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
