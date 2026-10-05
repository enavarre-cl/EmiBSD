# Architecture

Why the tree looks the way it does. Read when changing structure, not every session.

## Goal

Re-implement the OpenBSD kernel in Rust, file by file, preserving OpenBSD's structure, names and
semantics, as a standalone `#![no_std]` kernel for amd64 and arm64, booted by Limine, run in QEMU.
The C tree under `reference/openbsd-src/sys` is the specification.

## One kernel crate, not one crate per subsystem

`sys/` is a single Cargo package, `bsd` (OpenBSD's kernel image is `/bsd`). Subsystems are modules:
`kern`, `uvm`, `dev`, `ddb`, `sys` (headers), `machine`, `arch`.

Reason: kern, uvm and arch are mutually recursive (`trap → uvm_fault → pmap → tsleep → sched`).
OpenBSD resolves that at link time. Cargo forbids crate cycles, so a crate per subsystem would force
trait inversions everywhere. One crate resolves the cycles the way C does.

Exceptions are true leaves only: `sys/lib/libkern` and `sys/lib/libz` (OpenBSD builds both as libraries
too; libz holds what the kernel compiles of zlib: crc32, adler32, deflate, inflate; zlib licence). A module
may be promoted to a crate only if it uses nothing from `crate::{kern, uvm, arch, machine}`.

No `src/` directory (`[lib] path = "lib.rs"`), so C and Rust paths differ only by extension.

## Path and name mapping

| OpenBSD | Here |
|---|---|
| `sys/kern/subr_prf.c` | `sys/kern/subr_prf.rs` |
| `sys/sys/proc.h` (`struct proc`) | `sys/sys/proc.rs` (`pub struct Proc`) |
| `sys/kern/kern_fork.c` (`fork1()`) | `sys/kern/kern_fork.rs` (`impl Proc { pub fn fork1 }` or `pub fn fork1`) |
| `sys/arch/amd64/amd64/pmap.c` | `sys/arch/amd64/amd64/pmap.rs` |
| `sys/arch/amd64/include/pte.h` | `sys/arch/amd64/include/pte.rs` |
| `sys/lib/libkern/strlcpy.c` | `sys/lib/libkern/strlcpy.rs` |
| `sys/net/if.h`, `sys/netinet/in.h` (Rust keywords) | `sys/net/if_.rs`, `sys/netinet/in_.rs` (`docs/C_TO_RUST.md`) |
| `sys/arch/*/stand/`, `boot(8)`, `efiboot` | `sys/stand/` (Limine glue) until M14, when the user decided (2026-10-03) to port `boot(8)`/`efiboot` and `sys/lib/libsa/`; Limine is scaffolding until `boot(8)` boots the same kernel in QEMU |
| `sys/conf/`, `config(8)`, Makefiles, `newvers.sh` | Cargo features and `tools/xtask`; `ioconf.c` is `sys/arch/<arch>/conf/ioconf.rs`, by hand (M7b); the `vers.c` that `newvers.sh` generates is `sys/conf/vers.rs`, fed by `sys/build.rs` ("The system's identity", below); not ported |

Types live where the **header** is; functions live where the **`.c`** is. Rust allows inherent
`impl` blocks in any module of the defining crate, which is exactly the header/implementation split.

## The `machine` contract

`sys/machine/<header>.rs` holds the traits standing in for `<machine/*.h>` and `cpufunc.h`, one
module per OpenBSD header (`param.rs`, `cpu.rs` with `boot(9)`, `delay(9)` and, since M5, `curcpu()` with the
`cpu_info` accessors the clock code needs, `cons.rs` for
`consinit()`, `bus.rs` for `bus_space(9)`, `db_machdep.rs` for what `ddb` needs; later `pmap.rs`,
`intr.rs`, ...; `autoconf.rs` is what `ioconf.c` and the machine's `autoconf.c` give
`subr_autoconf.c`; `signal.rs` is `<machine/signal.h>` plus `sendsig`, `sys_sigreturn` and the
signal trampoline), all re-exported from `sys/machine/mod.rs`, which also re-exports
`consinit()`, `bus.rs` for `bus_space(9)` and (M7b) `bus_dma(9)`, `db_machdep.rs` for what `ddb`
needs; later `pmap.rs`, `intr.rs`, ...; `autoconf.rs` is what `ioconf.c` and the machine's
`autoconf.c` give `subr_autoconf.c`; `pci_machdep.rs` (M7b) is `<machine/pci_machdep.h>`;
`conf.rs` (M8) is the device switch each arch's `conf.c` fills; `isa_machdep.rs` (M8) is
`<machine/isa_machdep.h>`; `disklabel.rs` (M8) is `<machine/disklabel.h>` plus the machine's
`disksubr.c`, `readdisklabel` and `writedisklabel`), all re-exported from `sys/machine/mod.rs`, which also re-exports
`crate::arch::current::Machine` and asserts at compile time that it implements every trait. Generic
code names only `crate::machine`. `bus.rs` also carries the C names as free functions
(`bus_space_read_1(t, h, o)`, `bus_dmamap_load(t, map, ...)`), so a driver reads like its
original; the tag and handle types are the architecture's (`X86BusSpace`/`BusSpaceHandle` on
amd64, `&'static BusSpace` on arm64), and so are the DMA tag, map and segment types (each arch's
`include/bus.rs`; the tag is a table of functions, as in C). MI code reads a map's public
members by their C names (`dm_nsegs`, `dm_segs()`, `ds_addr`, ...), which every arch must
define (`bus_dma_public_members` checks it at compile time).

Constants travel the same way as functions: `MachineParam` (M1) carries `<machine/param.h>` and
the alignment rules of `<machine/_types.h>` as associated consts, each arch defines them in
`arch/<arch>/include/{param,_types}.rs`, and `sys/sys/param.rs` re-exports them, so generic code
imports `PAGE_SIZE` from `sys::param` exactly as C includes `<sys/param.h>`.

Three implementors:

- `sys/arch/amd64`: `cfg(all(target_os = "none", target_arch = "x86_64"))`
- `sys/arch/arm64`: `cfg(all(target_os = "none", target_arch = "aarch64"))`
- `sys/arch/host`: `cfg(not(target_os = "none"))`, a std-backed double. It makes `cargo test` work
  on macOS and proves mechanically that the contract is complete (a missing method fails to compile
  for the host too). It must not grow logic.

## Boot flow

Limine (UEFI, both archs) → `_start` in `sys/stand/mod.rs` (protocol structs in
`sys/stand/limine.rs`) → `machine::BootInfo` (bootloader-neutral: memory map, HHDM offset, kernel
load addresses, DTB/RSDP pointers, the UEFI system table and memory map (efiboot's
`openbsd,uefi-*` properties), command line; it lives in `sys/machine/bootinfo.rs` so the
machine traits can name it) → `boothowto` from the command line (`BootInfo::boothowto`, the
`boot(8)` letters `-a -c -d -s` as arm64's `initarm` parses them) →
`machine::Machine::early_init(&BootInfo)` (OpenBSD's `init_x86_64` / `initarm` as far as they are
ported: the message buffer, `consinit()`, and `db_enter()` for `boot -d`) → the `bsd: booted on`
banner → `kern::init_main::main` (OpenBSD's `main()` in the C's order; every step whose subsystem
is not here yet reports itself with `unported!`). Under feature `qemu`, `main` ends with the success
exit where proc0 would go to sleep.

A panic anywhere (`panic!` is `kern::subr_prf::panic` through the crate's panic handler) prints
`panic: <message>` through `db_printf`, a frame-pointer stack trace (`db_stack_dump` →
`machine::DbMachdep::db_stack_trace_print`, addresses only; `cargo xtask symbolize --arch A` names
them offline from the ELF symbol table) and reaches `reboot` → `machine::Cpu::boot`, which, cold,
halts; under feature `qemu` the "press any key" wait is the failure exit (status 35).

Leaving the machine goes through `machine::Exit`: under feature `qemu`, amd64 uses the
`isa-debug-exit` device and arm64 the semihosting `SYS_EXIT` call, both making QEMU exit with
status 33 for success and 35 for failure, which `xtask smoke` checks. Without the feature the CPU
halts.

At entry Limine (protocol base revision 6) guarantees: 64-bit mode, MMU on, kernel mapped at
`0xffffffff80000000`, a higher-half direct map of physical memory (HHDM), a memory map, a stack of
at least 64 KiB, interrupts masked, secondary CPUs parked. Limine's page tables live in
bootloader-reclaimable memory, so the kernel owns its own `pmap` and trap vectors before reclaiming
(M3/M4).

Why Limine: it is the only option with an identical boot contract on amd64 and aarch64, which keeps
the arch split focused on real kernel work (traps, pmap, interrupts). OpenBSD also keeps the
bootloader separate from the kernel, so this is faithful in spirit. `bootloader` (crate) is
x86_64-only; QEMU `-kernel` raw loading on `virt` would mean two unrelated early-boot paths.

The protocol is specified in https://github.com/limine-bootloader/limine-protocol (`PROTOCOL.md`,
`include/limine.h`); `sys/stand/limine.rs` implements the subset the kernel asks for, at base
revision 6, with no crate in between (see "Dependencies").

## Toolchain and targets

Stable Rust, pinned in `rust-toolchain.toml`.

- `x86_64-unknown-none`: kernel code model, no SSE/AVX, no red zone.
- `aarch64-unknown-none-softfloat`: no NEON/FP. The hardfloat variant lets the compiler use `q`
  registers in `memcpy`; a kernel that does not save FP state on traps must not touch them. This is
  the Rust equivalent of OpenBSD's `-mgeneral-regs-only`.

No nightly: `asm!`/`global_asm!`, `#[panic_handler]`, `#[global_allocator]`, `#[unsafe(no_mangle)]`
and `#[unsafe(link_section)]` are stable; `core` and `alloc` ship precompiled for both targets.
`extern "x86-interrupt"` is unstable, so interrupt stubs are assembly, like OpenBSD's `vector.S`.
`custom_test_frameworks` is unstable, so in-QEMU tests are serial smoke tests driven by `xtask`.

## Linking

`sys/arch/<arch>/conf/kernel.ld` (identical except `OUTPUT_FORMAT`): base `0xffffffff80000000`,
`PHDRS` text/rodata/data, Limine request sections kept, `.eh_frame`/`.note` discarded.
`sys/build.rs` passes it with `cargo:rustc-link-arg-bins` only when `target_os = "none"`.

Per-target rustflags in `.cargo/config.toml`: `relocation-model=static` (non-PIE higher-half kernel)
and `force-frame-pointers=yes` (backtraces in `panic`). No `[build] target`, so host builds stay
the default and `cargo test` just works.

## Cargo features ↔ `option(4)`

| Feature | OpenBSD | Effect |
|---|---|---|
| `alloc` | — | `extern crate alloc` and the `GlobalAlloc` over `malloc(9)`; default since M3 |
| `diagnostic` | `option DIAGNOSTIC` | `kassert!` active |
| `debug` | `option DEBUG` | `kdassert!` active |
| `kmemstats` | `option KMEMSTATS` | `malloc(9)` statistics and per-type limits |
| `pool_debug` | `option POOL_DEBUG` | `pool_debug = 1` (poisoning, once `subr_poison.c` is here) |
| `ffs` | `option FFS` | the fast file system (`sys/ufs`) and its `vfsconflist[]` entry; default |
| `ffs2` | `option FFS2` | FFS2 (UFS2 dinodes, the 64 KB super-block) in ffs; default |
| `qemu` | — | QEMU-only exits (`isa-debug-exit`, semihosting), the boot self-tests, the TSC under TCG and the `uptime went backwards` check |
| `inet6` | `option INET6` | IPv6: the `#ifdef INET6` sites outside `sys/netinet6` and `inet6domain` in `domains[]`; default, as in GENERIC. `sys/netinet6` itself (and the IPv6 tables and usrreqs it names, `route6_cache`, `tcp6_usrreqs`, ...) always compiles, like a library nothing reaches without the option, so the tree builds both ways |
| `multiprocessor` | `option MULTIPROCESSOR` | off by default, so the uniprocessor kernel stays the plain build; `just build` and `just clippy` also build it, and since M11e every `just smoke` recipe boots it with `-smp 4` except `smoke-up` (the user's decision of 2026-10-03). M11a: `MAXCPUS` 255/256, the kernel lock and the spinning mutex (`kern_lock.c`), the Limine MP request and the application processors' start (see "Deviations"); `just smoke-mp` boots it with `-smp 4`. M11d: `NET_TASKQ` 8 softnet queues, `softnet_percpu` keeps one per CPU; `just smoke-net-mp` runs both VMs of the two-VM smokes on it |
| `ntfs` | `option NTFS` | the read-only NTFS file system (`sys/ntfs`) and its `vfsconflist[]` entry; default, but compiled only where the architecture's GENERIC has it (amd64): see below |
| `fuse` | `option FUSE` | FUSE (`sys/miscfs/fuse`), its `vfsconflist[]` entry, `cdevsw[]` 92 (`/dev/fuse0`) and `fuseattach` in `pdevinit[]`; default, as in GENERIC |

More appear as they are needed (`small_kernel`, ...), one per `option(4)`.

An `option` that only some architectures' GENERIC sets (M10d: `option NTFS`, in
`arch/amd64/conf/GENERIC` alone) cannot be a per-target cargo feature, so `sys/build.rs` plays
`config(8)`'s part: its `ARCH_OPTIONS` table names the feature, a cfg and the architectures,
and it emits the cfg (`option_ntfs`) when the feature is on and the target is one of them, or
a host build (so the host tests cover the code). The code is gated on the cfg, not on the
feature: the arm64 kernel has no NTFS, as OpenBSD's arm64 GENERIC has none, and generic code
still never names an architecture.

A machine-independent driver that `files.<arch>` lists only for some architectures and whose
C uses those machines' headers directly (M12: `dev/fdt/pciecam.c`, written against `struct
machine_pci_chipset`, `struct bus_space`, `struct machine_intr_handle`) is gated the same
way: `sys/build.rs`'s `ARCH_MACHINE` table emits cfg `machine_pci_chipset` for arm64
bare-metal builds (never for host, whose double has none of these headers), the driver's
module is `#[cfg(machine_pci_chipset)]`, and it reaches the machine items through
`sys/machine/pci_chipset.rs`, which re-exports them from `crate::arch::current` under the
same cfg. Generic code still never names an architecture; the alternative, a dozen contract
methods with fake amd64 and host implementations, would make the driver unlike its C and the
fakes untestable anyway. The price: such a driver has no host tests, like arch code.

## Dependencies

| Crate | Where | Why it is not OpenBSD code |
|---|---|---|
| (none for Limine) | `sys/stand/limine.rs` | the `limine` crate was dropped: 0.6+ needs nightly (`ptr_metadata`), 0.5 is stable but frozen at base revision 3, which Limine has already tried to drop once. The protocol is about twenty `#[repr(C)]` structs; they are written from `PROTOCOL.md` |
| `bitflags` | `sys/` | typed flag sets for `#define` groups; a macro, no runtime |
| (none for lists and trees) | `sys/sys/queue.rs`, `sys/sys/tree.rs` | `intrusive-collections` was dropped at M1: the OpenBSD macros are short, their semantics are the project's to keep, and a crate's policy changes would bind us as the `limine` crate's did |
| `proptest` | dev-only | property tests for libkern |
| `serde`, `toml` | `tools/xtask` | tracker parsing |
| `fatfs` | `tools/xtask` | writes the FAT boot image; a host tool, not kernel code |

Not allowed: crates that replace OpenBSD code (`x86_64`, `aarch64-cpu`, `spin`, `uart_16550`,
`fdt`, `linked_list_allocator`, `buddy_system_allocator`). Porting that code is the project.

## The userland build (M8)

The userland is OpenBSD's own C, cross-compiled unmodified (the user's M8 decision), not ported.
`cargo xtask userland --arch A` (`tools/xtask/src/userland.rs`, `just userland`) builds, into
`target/userland/<arch>/`: the `/usr/include` sysroot as `include/Makefile` installs it
(`FILES`, `DIRS`, `LFILES`/`MFILES` links, the kernel headers of `LDIRS`, `<machine/*>`, and of
the `RDIRS` only `lib/libutil`'s headers and `lib/librpcsvc`'s `rpcgen` output, which libc's YP
code includes); `lib/csu`; `libc.a` (988 objects on amd64, 989 on arm64), `libutil.a`, `libm.a`
(259 objects on both; the programs' `-lm`), `libkvm.a` (`kvm_getprocs`, `kvm_getfiles`, ...
over sysctl(2) when no kernel image is named, the way `ps`, `fstat` and `vmstat` use it) and
`libcompiler_rt.a`; and
`sbin/init`, `bin/ksh`, `bin/cat`, `bin/echo`, `bin/ls`, `usr.bin/uname`, `sbin/mount`,
`sbin/mount_ffs`, `libexec/getty`, `usr.bin/login`, `libexec/login_passwd`, the network tools
`sbin/ifconfig`, `sbin/ping` (with its `ping6` link, setuid root), `sbin/route`, `sbin/pfctl` and
`sbin/ipsecctl`, the diagnostic tools `bin/ps`, `bin/df`, `usr.bin/fstat` (and its `fuser`
link) and `usr.bin/vmstat`, the disk tools `sbin/umount`, `sbin/newfs` (with its `mount_mfs` link),
`sbin/fsck`, `sbin/fsck_ffs`, `sbin/disklabel` and `sbin/fdisk` (M10a), the quota tools
`sbin/quotacheck`, `usr.sbin/quotaon` (and `quotaoff`), `usr.sbin/edquota`, `usr.sbin/repquota`
and `usr.bin/quota` (over `librpcsvc.a`, whose sources `rpcgen` makes from its `.x` files) with
`usr.bin/su`, `bin/mkdir` and `bin/chmod` (with its `chgrp` and `/sbin/chown` links; M10b), and a few more as static PIE executables, the form
OpenBSD's `cc -static` gives `/bin` and `/sbin` (`rcrt0.o` relocates the program itself; no
`PT_INTERP`).

Nothing is listed by hand. `tools/xtask/src/bsdmake.rs` evaluates the subset of `make(1)` the
Makefiles use (assignments, lazy expansion, the `:L :M :N :R :S :old=new` modifiers, `.if`,
`.for`, `.include`, `.PATH`, explicit rules) and fails on anything else. `SRCS`, `OBJS`, `.PATH`
and `CFLAGS` come from it; explicit rules are run through `/bin/sh` as make would, so the
system-call stubs are made exactly as `lib/libc/sys/Makefile.inc` makes them (`GENERATE.*`
piped into `FINISH.*`), and so are the generated hash helpers and the `rpcsvc` headers.
`share/mk` is not in the reference clone: `sys.mk` and `bsd.own.mk` are stood in for by
predefined variables (`CFLAGS?= -O2 -pipe ${DEBUG}`, `COMPILE.c`, `YP=yes`, `STATIC=-static`,
...), `bsd.prog.mk`/`bsd.lib.mk` by their variable effects (`../Makefile.inc`, `COPTS`) and the
implicit `.c.o`/`.S.o` rules; `CDIAGFLAGS` (warnings only) is empty. Apple clang's OpenBSD
target supplies the rest of OpenBSD's defaults by itself: PIE, `-fstack-protector-strong`,
IBT (`-fcf-protection=branch`) on amd64, BTI and return-address signing on arm64, emulated TLS.

Workarounds, each printed by the build (flags only; no source is edited):

- `-fret-clean` (amd64 libc) is an OpenBSD-local clang option Apple clang rejects; it is dropped.
- `rpcgen`, `makefs`, `pwd_mkdb` and `yacc` are built for the Mac with `-D'pledge(p,e)=0'`
  (macOS has no `pledge(2)`).
- `.y` sources (`sbin/pfctl/parse.y`, `sbin/ipsecctl/parse.y`) follow `bsd.sys.mk`'s `.y.c` rule:
  `${YACC.y} parse.y` (`YACC.y` is `${YACC} -d ${YFLAGS}`), then `mv y.tab.c parse.c`, run in the
  program's object directory. `YACC` is OpenBSD's own `usr.bin/yacc`, built for the Mac the first
  time a `.y` is met (`host/bin/yacc`, named by its absolute path, so never macOS's bison-based
  `/usr/bin/yacc`). Its only shim is a force-included `reallocarray(3)`, which macOS's libc lacks.
- `usr.bin/uname`, `usr.bin/id`, `usr.bin/login`, `usr.bin/fstat`, `usr.bin/vmstat`,
  `libexec/getty` and `libexec/login_passwd` are linked `-static` (their Makefiles are dynamic, as `/usr/bin` and `/usr/libexec` are on
  OpenBSD; there is no `ld.so` yet), as the install media's crunched programs are.
- M10e's `usr.sbin/portmap` and `usr.bin/showmount` are linked `-static` for the same reason,
  and so are `sbin/mountd` and `sbin/nfsd`, whose Makefiles end with `LDSTATIC=` (OpenBSD ships
  them dynamic). No source or other flag differs. The ramdisk gets what they need from a base
  install: the `_portmap` user and group (28:28, from `etc/`), `/etc/rpc` (the portmapper, nfs,
  mountd and rquotad lines of `etc/rpc`) and `/var/db` (mountd's `mountdtab`); no `/etc/exports`.
- A program whose Makefile sets `BINOWN`, `BINGRP` or `BINMODE` (`login_passwd`: root:auth,
  setuid 4555, in `/usr/libexec/auth`, where `lib/libc/gen/auth_subr.c`'s `_PATH_AUTHPROG`
  looks for BSD Auth styles) gets them in the image (below).
- `ksh` is built like OpenBSD's install-media ksh: `-DSMALL`, no `-lcurses`: `libcurses`
  is built (M9+, below), but the image has no terminfo database (`share/termtypes` is not
  in the clone).
- macOS file systems ignore case: libc's `_exit.o` stub and `stdlib/_Exit.o` are built in
  separate directories (both are archive members).
- `libcompiler_rt.a` is built from `gnu/lib/libcompiler_rt` over `gnu/llvm/compiler-rt`
  (in the clone since 2026-10-03, Apache-2.0 WITH LLVM-exception) and linked as
  `-lcompiler_rt -lc -lcompiler_rt`, as OpenBSD's clang driver does; arm64 needs its
  quad-float helpers (`__multf3`). The stand-in `bsd.own.mk` sets `BUILD_CLANG=yes`, as the
  real one does on amd64 and arm64.

M9+ adds LibreSSL (`lib/libcrypto`, `lib/libssl`, `lib/libtls`), `lib/libcurses` (ncurses)
and `lib/libedit` (`LIBRARIES` in `userland.rs`, the code in `userland/libraries.rs`), for
`usr.bin/ftp` and `usr.bin/nc` (static, like `login`). Nothing new is listed by hand either:

- Generated sources are made by running the Makefiles' own rules for `BUILDFIRST`, which
  `bsd.lib.mk` makes before any object, each after the sources a rule of its own makes
  (`make_target`, make's recursion). On amd64 that runs libcrypto's perlasm: each `${f}.S`
  rule of `arch/amd64/Makefile.inc` (a two-variable `.for dir f in ${SSLASM}`, which
  `bsdmake.rs` supports) runs `/usr/bin/perl ./asm/${f}.pl openbsd`, the Mac's perl, into
  the object directory; arm64 has only its `.S` sources. `objects.pl` and `obj_dat.pl` make
  `obj_mac.h` and `obj_dat.h` the same way. libcurses's rules run its `MK*.sh`/`MK*.awk`
  scripts with the Mac's `sh`, `awk` (the one-true-awk OpenBSD has) and `sort`, and build
  `make_keys` and `make_hash` with `${HOSTCC}` (the Mac's clang, which `sys.mk`'s `HOSTCC`
  names) and run them; libedit's run its `makelist` script.
- Headers: `include/Makefile`'s `RDIRS` entries for these libraries are installed by running
  each library's own `includes` rule, with an `install(1)` stand-in in `$PATH` that records
  what it is asked to install (and a `cmp(1)` that always says "different"); xtask then
  copies the recorded files. So `<openssl/*.h>` gets libcrypto's generated `obj_mac.h`, and
  libcurses's `curses.h` becomes `<ncurses.h>`, as their Makefiles say.
- LibreSSL's Makefiles add `-Werror`. Where they do, `-Wno-pointer-sign` is added
  (`WERROR_DEFAULTS`): OpenBSD's clang does not warn about mixing `char *` and
  `unsigned char *` by default, Apple clang does, and libcrypto mixes them.
- `share/mk`'s `bsd.subdir.mk` (recursion into `SUBDIR`, the `man` directories) is stood in
  for by nothing.

Every OpenBSD file compiled or included is classified by licence into `licences.txt`;
LibreSSL's two licences are named `OpenSSL` and `SSLeay` there (accepted by the user on
2026-10-03), not counted as BSD-4-Clause.

### The ramdisk image

The ffs image the kernel boots from (`target/userland/<arch>/ramdisk.ffs`, the `rd(4)` root) is
made by OpenBSD's own makefs(8), built for the Mac from `usr.sbin/makefs` (in the clone since
2026-10-03), not by a file system writer of our own (decided by the user on 2026-10-03). The
on-disk format is then OpenBSD's by construction. makefs runs as OpenBSD's `distrib/` runs it
for its ramdisks (`-t ffs -o disklabel=rdroot,minfree=0,density=4096`). The `rdroot` entry
is read by OpenBSD's own `getdiskbyname` (`lib/libc/gen/disklabel.c`, built in) from a disktab
xtask writes (`target/userland/<arch>/host/disktab`, OpenBSD's `/etc/disktab` is not in the
clone): one track of one cylinder spanning the image, partition `a` FFS at offset 0 with
4096/512 blocks/fragments. makefs's own `rdroot=1` label is not used: it leaves `d_nsectors`
0, which `checkdisklabel` rejects. The result is FFS1 in `a` and the label in sector 1; the
size is twice the contents in whole MiB (at least 2 MiB), the timestamps fixed (`-T`).
Its tree is `root/` plus `/etc`, `/dev` and the directories below.
M10c adds `/root/images` (`images.rs`): a FAT12 and an ISO 9660 image made by the same makefs
(`-t msdos`, `-t cd9660 -o rockridge`) and a UDF image made by macOS's own `hdiutil makehybrid
-udf` (OpenBSD has no UDF writer; nothing is installed), for vnd(4) to attach in `smoke-fs`.
Disk nodes follow MAKEDEV: `UNITMULT` 64 minors per unit (`MAXPARTITIONSUNIT`).

M10d adds `/root/images/ntfs.img` on amd64 only (ntfs is only in amd64's kernel): a 4 MiB
NTFS 3.1 volume made by a generator of our own, `tools/xtask/src/ntfsgen.rs` (ISC, written
from the public description of the format, no code of ntfs-3g or any other implementation;
`cargo xtask ntfs-image OUT [--check]` makes it alone). Why our own: OpenBSD has no NTFS
writer, and the usual one, ntfs-3g's `mkntfs`, does not build on macOS (decided by the user,
2026-10-04). It holds the 16 system files ($MFT .. $Extend, a full $UpCase, a $LogFile of
0xff bytes, which readers take as empty and clean), a root that is a large index (one `INDX`
block), a resident `m10d-ntfs.txt` and a non-resident `m10d-ntfs-big.txt`. So that a
generator bug shared with our kernel cannot pass unseen, every image is mounted read-only by
an independent reader, macOS's own NTFS driver (`ntfs.fs`, an FSKit module on macOS 26:
`diskutil mount readOnly` over an `hdiutil attach -nomount` raw device, no root), which must
list exactly the two files and read their bytes back (docs/SETUP.md, "NTFS check").

`/etc` is our own minimal set (OpenBSD's `etc/` is not in the clone), text in `ramdisk.rs`:
`motd`, `shells`, `fstab` (`/dev/rd0a / ffs rw 1 1`, which `mount -uw /` needs), `ttys` (a
`getty std.9600` on `tty00`, `console` off), `gettytab`, `login.conf` (a `default` class with
`auth=passwd`, a `daemon` class), `group`, `master.passwd` (root, daemon, nobody; only root
has a password, `emibsd`, docs/SETUP.md) and `rc`, a minimal script that runs `mount -uw /`,
creates `utmp`, `wtmp`, `lastlog` and `failedlogin` and prints `rc: multi-user`. `pwd.db`,
`spwd.db` and `passwd` are made by OpenBSD's own pwd_mkdb(8) (`-p -d <staging>/etc`), built
for the Mac like makefs from `usr.sbin/pwd_mkdb` (in the clone since 2026-10-03), over OpenBSD's
own db(3) (`lib/libc/db`, hash and btree) and `pw_scan` (`lib/libutil/passwd.c`), not macOS's
`dbopen`, so the databases have OpenBSD's format by construction. The hash is OpenBSD's
`bcrypt.c` with `blowfish.c`, in a small helper that replaces `arc4random_buf` with a fixed
salt before including the unmodified source, so the image is reproducible. For the network
clients (M9+): `resolv.conf` (`nameserver 10.0.2.3`, QEMU's user-network DNS), `hosts`
(`localhost`, and `emibsd-host` for `10.0.2.2`, QEMU's alias of the Mac, where
`smoke --https-server` runs test servers), and `/etc/ssl`: `cert.pem`, LibreSSL's CA bundle
(`lib/libcrypto/cert.pem`, as its `distribution` target installs it, 0444), and
`emibsd-test-ca.pem`, the certificate of the test CA `userland/testca.rs` makes once with the
Mac's `openssl` (docs/SETUP.md, "The test CA"). With LibreSSL, ftp and nc the image is about
22 MiB, so the boot image (`boot.rs`, `IMAGE_SECTORS`) is 128 MiB. The directories are
`/home`, `/mnt`, `/root` (0700), `/tmp` and `/var/tmp` (1777), `/var/{log,mail,run}`. `/dev` has
`console`, `tty`, `mem`, `kmem`, `null`, `zero`, `klog`, `tty00` (the console on both
architectures: `com0` on amd64, and on arm64 `pluart0` takes `com`'s slot, major 8, in
`pluartcnattach`), `rd0{a,b,c}` (block 17), `rrd0{a,b,c}` (47), `sd{0,1}{a..p}` (block 4) and `rsd{0,1}{a..p}`
(character 13; minor `unit * 16 + partition`, 0640 root:operator, as `MAKEDEV`'s `dodisk`;
the image has `sd0` and `sd1`), `fd/0..63` and
`stdin`/`stdout`/`stderr`; the majors and minors, with their `conf.c` lines, are in
`DEVICES`'s comment.

makefs is written for OpenBSD only; the Mac build takes host shims, all in
`tools/xtask/src/userland/ramdisk.rs` and none in the sources: a force-included header
(`daddr_t` is 64-bit, `st_*tim`, OpenBSD's `MAXBSIZE`, no-op `pledge`/`unveil`,
`srandom_deterministic` as `srandom`), OpenBSD headers macOS lacks taken from the clone
(`ufs/`, `msdosfs/`, `sys/disklabel.h`, `machine/disklabel.h`, `sys/uuid.h` with `uuid_t`
renamed), a `sys/endian.h` over `<libkern/OSByteOrder.h>`, `scan_scaled` from
`lib/libutil/fmt_scaled.c`, `cgetent` pointed at `$EMIBSD_DISKTAB`, and an `lstat` wrapper for device nodes: macOS lets only root
`mknod` and OpenBSD's makefs has no mtree spec, so a staging file holding one
`emibsd-makefs-device c <major> <minor> <mode>` line is reported to makefs as that device (with
OpenBSD's `makedev()` encoding). The same wrapper makes every file root:wheel and applies a
table (`$EMIBSD_OWNERS`, `mode uid gid path`; paths relative to `$EMIBSD_STAGING`) for the
exceptions (`login_passwd` setuid, `spwd.db` root:_shadow, `master.passwd` 0600, `/tmp`
sticky, `/dev` modes); makefs would otherwise take owner and group from the host files, the
building user's. `pwd_mkdb`'s shims (`passwd.rs`) are likewise a force-included header
(`__BSD_VISIBLE`, OpenBSD's `<pwd.h>` before macOS's, `__dead`, libc's `DEF_WEAK`/`PROTO_*`
macros as nothing, `explicit_bzero`, a check that the host is little-endian as the db(3) files
are made in host order), OpenBSD's `<pwd.h>`, `<util.h>`, `<mpool.h>` and `hidden/db.h` from the
clone in a directory searched first, and a `getgrnam("_shadow")` that answers with the building
user's group (macOS has no such group, and `pwd_mkdb` insists on one).

EmiBSD's own test programs (M10f) are the one thing in the userland build that is not OpenBSD's
C. `OWN_PROGRAMS` in `userland.rs` lists directories of this repository (`tools/<name>/`, paths
relative to the workspace root), each with an OpenBSD-style `Makefile` (`PROG`, `BINDIR`,
`LDSTATIC= -static`, `NOMAN`, `.include <bsd.prog.mk>`) and an ISC-licensed source naming the
EmiBSD authors. They are built after `PROGRAMS` by the same `build_prog`, with the same
clang, sysroot, libc and static-PIE link, and installed stripped under `root/` where `BINDIR`
says; only `make_for` differs, taking the Makefile's directory from the workspace instead of
the reference tree. The licence report lists reference-tree files only, so they do not appear
in it. First user: `tools/sr6create` (`/usr/sbin/sr6create`), which creates a softraid(4) RAID 6
volume through `BIOCCREATERAID`; OpenBSD's bioctl(8) refuses `-c 6` ("unsupported RAID level")
although `softraid_raid6.c` is in the kernel, and the userland is compiled unmodified, so
the test program does what bioctl's `bio_createraid()` does for a non-crypto level, for level 6.
Second user (M10d): `tools/fusehello` (`/usr/sbin/fusehello`), a read-only FUSE file system
with a fixed tree (`/hello.txt`, `/sub/deep.txt`) over OpenBSD's own libfuse (`fuse_main` with
`getattr`, `readdir`, `open`, `read` and `statfs`), for `smoke-fuse`: OpenBSD has no FUSE file
system of its own in base, only the library. Its Makefile adds `-I${DESTDIR}/usr/include/fuse`
and `-lfuse`, what libfuse's `fuse.pc` gives its users.

M10d adds `lib/libfuse` to `LIBRARIES`. Its `includes` rule makes `/usr/include/fuse` with
`install -d` before installing its headers there; the `install(1)` stand-in records a `-d dir`
line for that, and xtask makes the directory. Its sources include the kernel's
`<sys/fusebuf.h>` from the sysroot. ext2fs's `newfs_ext2fs(8)`, `fsck_ext2fs(8)` and
`mount_ext2fs(8)` and `mount_ntfs(8)` are in `PROGRAMS`; mount_ntfs's Makefile sets `NOPROG=`
unless `MACHINE` is alpha, amd64 or i386, and `build_prog` then builds nothing, as
`bsd.prog.mk` does, so arm64's ramdisk has no mount_ntfs. The ramdisk has `/dev/fuse0`
(character 92, minor 0, 0600, as `MAKEDEV` makes it), the one node libfuse opens.

`smoke-ext2fs` checks the guest's ext2 file system twice: with OpenBSD's `fsck_ext2fs(8)` in
the guest and with e2fsprogs on the Mac (`cargo xtask e2fsck`, `tools/xtask/src/e2fs.rs`;
docs/SETUP.md, "e2fsprogs"), an independent implementation, so a bug shared by our kernel and
OpenBSD's tools cannot pass unseen. xtask finds partition `a` as `readdoslabel` does (the MBR's
0xA6 partition, its label in sector 1) and hands e2fsck and debugfs `image?offset=BYTES`.

## Deviations from OpenBSD (deliberate)

- One kernel is both `bsd` and `bsd.rd` (M8). OpenBSD builds GENERIC (`config bsd swap
  generic`, `swapgeneric.c`) and RAMDISK (`config bsd root on rd0a swap on rd0b`, with
  `rd(4)` and its image) and boot(8) loads one. Here `sys/conf/swapgeneric.rs` holds the
  generic values, and when Limine hands over `ramdisk.ffs` the boot glue (`sys/stand`)
  installs it in `rd(4)` and switches the root configuration to RAMDISK's
  (`swapconf_rdroot`) before `main`: `diskconf` (each machine's, no boot device under
  Limine) then runs `setroot`, which prints `root on rd0a swap on rd0b dump on rd0b`, and
  `dk_mountroot` mounts ffs from `rd0a`; `start_init` execs `/sbin/init` from it. Without a
  ramdisk the kernel stays generic and, having no boot device to ask about (`setroot`'s
  `RB_ASKNAME` prompt is not ported), says it cannot mount root and runs its init boot
  module (the Rust self-test).
- boot(8)'s `BOOTARG_BOOTDUID` (efiboot's `openbsd,bootduid` on arm64), the DUID of the disk
  the kernel came from, is a `bootduid=<16 hex digits>` word of the Limine command line
  (M13a): `BootInfo::bootduid` parses it and `sys/stand` writes the kernel's `bootduid`
  before `main`. `setroot` then finds the boot disk by its label's DUID, as in OpenBSD, and a
  kernel without a ramdisk mounts its root from that disk's `a` partition (`root on sd0a
  (<duid>.a)`). The kernel itself sits on the boot image's FAT partition, not on that disk,
  so the DUID is the only boot device there is: `just smoke-nvme` boots from an NVMe disk
  `cargo xtask nvme-root` lays out as OpenBSD's installer does (MBR, OpenBSD partition at 64,
  disklabel, the userland's ffs in `a` with its fstab naming `/dev/sd0a`;
  `tools/xtask/src/hwopts.rs`). Flags go after it on the command line (`boothowto` reads
  every letter from the first `-` on).
- amd64's FPU state uses `fxsave64`/`fxrstor64` only (`amd64/fpu.rs`): the XSAVE family and
  its codepatches are not ported, so there is no AVX state; the switch is eager as in C
  (`CPUPF_USERXSTATE`, saved in `cpu_switchto`, reloaded on the way back to user mode).
- Limine instead of `boot(8)`/`efiboot`.
- The application processors are started by Limine (M11a), not by the kernel's own
  trampoline: amd64's `mptramp.S` (real mode, INIT/SIPI/SIPI from `cpu_start_secondary`)
  and arm64's PSCI `CPU_ON` into `locore.S`'s `cpu_hatch` are replaced. The
  `MULTIPROCESSOR` kernel puts the MP request in `.requests` (the uniprocessor one does not,
  so the bootloader leaves the other processors halted); Limine brings each processor to
  long mode or EL1 with the boot page tables and parks it on its `goto_address`.
  `sys/stand` turns the response into `machine::BootMp` (the processors' hardware IDs and a
  `start(i, arg)`), and the machine's `cpu_start_secondary` calls `start` with the
  processor's `struct cpu_info`; the AP runs `stand::ap_start` on the bootloader's 64 KiB
  stack, which calls `Cpu::cpu_hatch(arg)`, the machine's `cpu_hatch`. Why: Limine already
  owns the boot path (above), and a real-mode or MMU-off trampoline would need identity
  mappings the kernel otherwise never makes. With no ACPI MADT (M13) the processor list
  comes from the same response on amd64; arm64 still enumerates `/cpus` from the device
  tree and matches each `reg` to a response entry by MPIDR. On amd64, `cpu_hatch_entry`
  does `mptramp.S`'s `cpu_spinup_finish` (x2APIC if the boot processor runs it, `EFER.NXE`,
  the CPU's GDT, the kernel's `%cr3`, `CR0`, the idle thread's stack) and loads the IDT
  first, which the C does later in `cpu_hatch`. On arm64 it loads the boot processor's
  `MAIR`, `TCR`, `TTBR0`/`TTBR1` and `SCTLR` (copied into statics: the `cpu_info` itself is
  `malloc`ed kernel memory the bootloader's tables do not map), sets `TPIDR_EL1`,
  `VBAR_EL1` and `CPACR`, moves to the CPU's own stack and runs `cpu_init_secondary`.
- amd64 processor enumeration (M11a): there is no ACPI MADT yet (M13) and no `mpbios`, so the
  `MULTIPROCESSOR` kernel's mainbus attaches one `cpu` per processor the bootloader found
  (`BootInfo::mp`): the boot processor first as `CPU_ROLE_BP`, the others as `CPU_ROLE_AP`
  in the bootloader's order, the hardware ID as `cpu_apicid` (`GENERIC.MP`'s
  `cpu* at mainbus?`). A uniprocessor kernel, or an MP kernel the bootloader found one
  processor for, attaches `cpu0` as `CPU_ROLE_SP` as before. The application processors mask
  `LINT0`: QEMU wires the 8259's ExtINT to every local APIC, and device interrupts stay on
  the boot processor.
- The kernel lock (M11a, audited in M11e). M11a took it around everything ported against one
  CPU, OpenBSD's own way of bringing code under MP. M11e audited every `MULTIPROCESSOR` site
  and every `KERNEL_LOCK` the port had kept as a comment, module by module, and now the lock
  covers what it covers in OpenBSD: each `KERNEL_LOCK`/`KERNEL_UNLOCK`/`KERNEL_ASSERT_LOCKED`
  is a real call (nothing without `MULTIPROCESSOR`), `SIF_MPSAFE` soft interrupts,
  `TASKQ_MPSAFE` task queues (`systqmp`, the softnet queues, wg's) and `TIMEOUT_PROC |
  TIMEOUT_MPSAFE` timeouts (the `softclockmp` thread: TCP, ARP, TDB, syn cache, socket
  timers) run without it, and so do `IPL_MPSAFE` interrupt handlers (vio's). `mi_syscall`
  honours `SY_NOLOCK`, except for the system calls in `SY_NOLOCK_DEFERRED`
  (`sys/sys/syscall_mi.rs`), each group with the reason it still takes the lock; a host test
  keeps the unlocked set equal to the audited list. `uvm_fault` and `uvm_grow` run unlocked
  in both machines' traps, `exit1` drops the lock around `uvm_purge` and the reaper runs
  unlocked. printf takes `kprintf_mutex` and the message buffer `log_mtx`, so CPUs do not
  interleave characters. Unlocked as in OpenBSD since M11a: the scheduler and the idle loop,
  `mi_switch`, the clock interrupt (`clockintr_dispatch`), the SMR thread and the IPIs.
  Deviations: poll's rate-limit static (`poll_lasterr`, `sys_generic.rs`) and wg's
  `wg_last_underload` are guarded (by the kernel lock and a mutex): the C touches them
  unlocked, which is a data race in Rust. The network takes SMR as in C for the ART, the
  rtable maps (the C's SRP is `smr_call` here), `rt_next`, bpf's listener lists and filters,
  pflow's list and pfsync's softc (`SMR_SLIST` is ported for them). Open, a deviation: the
  statistics counters the C bumps with a plain `++` from several softnet threads (pf's rule,
  state and table counters, `rmx_pksent`) and a few words the C reads unlocked from softnet
  (`rt_flags`, `rt_priority`, `rt_gateway`, bpf's `bd_dirfilt`/`bd_fildrop`, `if_bpf`) stay
  `Cell`s like the C's plain words; making them atomics changes every user. The `qemu`-only `uptime went backwards` check compares each CPU's readings with that
  CPU's previous one (`kern_clockintr.rs`); since M11b it counts them per CPU, and the MP boot
  self-test `clockintr_percpu` checks that every CPU runs its own clock interrupts.
- Memory allocators under MP (M11a): the pool lock is the C's mutex or rwlock with and without
  `MULTIPROCESSOR` (the uniprocessor C kernel takes the same mutex); `malloc_mtx` and
  `uvm.fpageqlock` are real mutexes at `IPL_VM`. With `MULTIPROCESSOR` the pools' per-CPU
  caches are ported (`pool_cache_init` on the anon pool, the `selftest=mpstress` pools and,
  since M11e, the knote pool and the mbuf, tag, ext-ref and cluster pools; pfsync's deferral
  pool cache is commented out in the C too) and
  `pool_gc_pages` runs every second; `evcount` and `mbstat` have per-CPU counters. A
  `PR_WAITOK` `pool_get` with no memory sleeps for a request as in C, except while cold or on
  proc0, where it fails (the C panics under `DIAGNOSTIC`). M11e: `uvm.pageqlock` is the C's
  mutex at `IPL_VM`; the pmaps take their `pm_mtx` where the C does (amd64's
  `pmap_map_ptes`/`pmap_unmap_ptes` only lock: the page tables are walked through the direct
  map, with no `%cr3` borrow); pmap and `uvm_object` reference counts are atomics. The per-CPU
  page cache (`__HAVE_UVM_PERCPU`, `uvm_pmr_cache_*`) is ported, its magazines in an array
  indexed by `cpu_number()` rather than in `struct cpu_info`. arm64's `pmap_purge`
  (`__HAVE_PMAP_PURGE`) is a `machine::Pmap` method, a no-op on amd64. amd64's mainbus counts every application processor in `ncpusfound`, as `acpimadt` does.
- arm64 turns on the generic timer's event stream (`CNTKCTL_EL1.EVNTEN`, a `wfe` wake-up
  about every 130 us) on every CPU, which OpenBSD does not (M11e). Reason: QEMU's TCG can lose
  the `sev` that ends a `wfe` wait (its sev helper kicks the halted vCPU without the global
  lock), and an application processor waiting for `CPUF_GO`, whose GIC CPU interface is not
  enabled yet, then never wakes. The event stream bounds every lost event, as Linux keeps it
  on for the same reason.
- ddb on MP (M11c): `db_ktrap` stays at `splhigh` for its whole `db_enter_ddb` loop, where
  the C drops back to the trapped level between iterations. Reason: a CPU that handed ddb to
  another (`machine ddbcpu`) waits in that loop with interrupts on, and at a low level it
  runs the console's interrupt and eats the active CPU's input. Without `longjmp`, a fault
  inside a ddb command prints `Faulted in DDB` and panics instead of returning to the prompt.
- Cargo features and `xtask` instead of `config(8)`, Makefiles and `newvers.sh`; the
  autoconfiguration tables `config(8)` generates are written by hand ("Autoconfiguration",
  below).
- `aarch64-unknown-none-softfloat` target; Intel syntax for amd64 inline assembly.
- `Result<T, Errno>` instead of `int` returns; RAII guards for `spl`/mutex.
- A host test double (`arch/host`), which OpenBSD does not have. It has no MMU: its
  `Pmap::PMAP_NOMMU` makes `km_alloc` and `kmeminit` serve everything through the direct map
  (the test process's memory), where amd64 and arm64 map `kernel_map`/`kmem_map` as OpenBSD does.
- Console attach before autoconfiguration exists (M2 to M4): `consinit()` attaches `com(4)` at
  `CONADDR` (amd64, `consinit.rs`) directly instead of `cninit()`'s `constab[]` walk; arm64
  finds its PL011 in the device tree since M4 (`pluart_init_cons`). This stays after M8: the
  console's tty is the one autoconfiguration attaches later (`com0 at isa0`, `pluart0 at
  mainbus0`), which recognises the console's registers and takes it over as OpenBSD's drivers
  do. On arm64, `initarm` installs a one-block identity map of the first GiB in
  `TTBR0_EL1` with Device-nGnRnE attributes, because the Limine protocol maps RAM but not devices;
  `bus_space_map` is the identity inside it until `pmap` maps devices (M3, page tables).
- `delay(9)` before the clocks: amd64 polls the i8254 (`isa/clock.rs`) through `delay_func`,
  which `delay_init` hands to `tsc_delay` when the TSC frequency is known from CPUID or an MSR,
  as in OpenBSD (under QEMU it is measured, so `i8254_delay` stays); arm64 uses `intr.c`'s `arm_dflt_delay` until `agtimer` attaches (M4).
- ddb-lite: `db_enter()` is a breakpoint trap (`int3`, `brk #0xf000`) that lands in `db_ktrap`
  and `db_trap`, which print `Stopped at <pc>` and the stack trace from `ddb_regs` and then
  return, as the `c` command would, because there is no command loop (`db_command.c`,
  `db_run.c`). A panic prints its trace through `db_stack_dump`. `db_panic` therefore defaults
  to 0: a fatal trap is printed by `kerntrap`/`do_el1h_sync` and panics instead of entering a
  debugger that could not be left. No symbols in the kernel yet (`db_sym.c`): traces are
  addresses, symbolised by `xtask symbolize`.
- Traps (M4, part a): the entry stubs are OpenBSD's `vector.S`/`locore.S` and `exception.S`,
  kept as `.S` files and included by `global_asm!` with the `assym.h` symbols (frame offsets,
  selectors, trap numbers) passed as `const` placeholders. amd64 builds its GDT, TSS and IDT in
  `init_x86_64` (the IDT is a static page; `cpu_init_msrs` runs first thing because there is no
  `locore0.S`), NMI and double fault take `alltraps` on their IST stacks (the `calltrap_specstk`
  path exists for user-mode GS/CR3, M6), and `alltraps_kern` does not re-enable interrupts until
  the interrupt stubs exist. arm64's `initarm` switches to `SP_EL1` (Limine enters with
  `SPSel = 0`, whose vectors are empty, as in C), sets `tpidr_el1` and `VBAR_EL1` itself; `x18`
  is a general register here, so the EL1 paths of `exception.S` save and restore it instead of
  keeping `curcpu()` in it, and `do_el1h_sync` keeps interrupts masked. Without processes every
  kernel page fault or data abort is fatal (`kpageflttrap` returns 0 when `curproc` is NULL, as
  in C; `kdata_abort` has no `pcb_onfault` and `uvm_fault` is reported), which is what the
  `selftest=trap` boot of `smoke` asserts on both archs.
- Interrupts (M4, part b1, amd64): `spl(9)` is OpenBSD's: `splraise`/`spllower` in `intr.c`,
  `Xspllower`/`Xdoreti` in `spl.S`, the per-source masks in `cpu_info`, the `INTRSTUB` stubs of
  `vector.S` for the sixteen legacy IRQs and the MI soft interrupts (`kern_softintr.c`, the
  `Xsoft*` stubs). What autoconfiguration would do was done by `cpu_configure` directly until
  `config_rootfound` existed (M7b, "Autoconfiguration" below): `lapic_boot_init` at the
  architectural LAPIC base (the MADT and MP tables are not ported), `cpu_intr_init`,
  `intr_enable`. `lapic_set_lvt` programs LINT0 as
  ExtINT and LINT1 as NMI, the MP default configuration, because the firmware leaves LINT0
  masked and there are no tables to read it from; the IOAPIC stays off, so the 8259 is the
  PIC. The mutex is the uniprocessor one (`kern_lock.c`), `evcount` has no per-CPU counters
  yet. Until M8 the console's receive interrupt was armed by the machine
  (`Console::cn_rx_intr_establish`) for the `selftest=uart` boot, which types a line on the
  serial console and expects it echoed; since M8 `com_isa`'s attach establishes it and the
  boot reads the line through the console's tty (`comintr`, `comsoft`, `ttyinput`, `ttread`).
- Interrupts (M4, part b2, arm64): the device tree is the one Limine hands over (`fdt.c`
  parses it in place); QEMU `virt` boots with `acpi=off`, because EDK2 installs the device
  tree only when it does not publish ACPI tables, and OpenBSD arm64 needs the tree. The
  console is found through `/chosen` (`pluart_init_cons`), which retires the fixed PL011
  address. `mainbus_attach` pre-registers the interrupt controllers (`arm_intr_init_fdt`)
  and attaches the GICv2 (`ampintc`) from the device tree (built by hand in `cpu_configure`
  until M7b); `ampintc` then owns `spl` through `arm_set_intr_handler`. `do_el1h_sync` enables interrupts
  as the C does. The console's receive interrupt goes through `arm_intr_establish_fdt` (since
  M8 from `pluart_fdt_attach`, `fdt_intr_establish`), so the `selftest=uart` boot exercises
  the same path on arm64 as on amd64.
- Clocks (M5-a): the time code is OpenBSD's (`kern_tc.c` over the timehands ring,
  `kern_clockintr.c`'s per-CPU queue, `kern_timeout.c`'s timing wheel, `kern_clock.c`), reached
  from the machine through the `Cpu` trait's `CpuInfo`/`ClockFrame` associated types and
  accessors. `main` brings up the wheel, the clock queue, the four per-CPU clock interrupts
  (`sched_init_cpu`'s binds) and `initclocks`. amd64 starts the i8254, calibrates the LAPIC
  timer against it (`lapic_calibrate_timer`, as the boot CPU's `cpu_attach` does) and drives
  `clockintr_dispatch` from `Xintr_lapic_ltimer`. The timecounter is the TSC (`tsc.c`, with
  `identcpu.c`'s TSC part; `kern.timecounter.hardware=tsc`), the i8254 the fallback. Behind
  the LAPIC timer the i8254 counts 15 bits (`i8254_inittimecounter_simple`) and wraps every
  27.46 ms: uptime moves forward only while hardclock winds the timehands up within each wrap,
  so a clock interrupt held off longer (a QEMU vCPU descheduled by a busy host) steps
  `nanouptime` back a period, as it would on OpenBSD with this counter; the time code keeps
  the C's modular arithmetic for it. The TSC's 32-bit count wraps after seconds. Under feature
  `qemu`, `clockintr_dispatch` prints `uptime went backwards` if a reading is behind the
  previous one, and every smoke run rejects that line (`--reject`). `acpihpet` and
  `acpitimer` need the ACPI tables. arm64 attaches `agtimer` from the device tree (through mainbus
  since M7b) and takes the virtual timer's PPI through `ampintc`. The `selftest=clock` boot waits for
  `hz` hardclocks and a `timeout(9)`. The host double owns a `cpu_info` of its own so the
  clock queue and the wheel are unit-tested over the dummy timecounter.
- Processes (M5-b, part 1): `struct proc`/`struct process` are OpenBSD's with the members
  the scheduler and the kernel threads use; the machine-dependent parts (`mdproc`, `pcb`)
  come through `machine::proc` (associated types with associated-constant initialisers, so
  `proc0` is a `static`). `proc0paddr` is a static u-area per arch (`Uarea`, `USPACE` bytes,
  page aligned, as `locore` reserves it in C): proc0's kernel stack stays the boot stack
  Limine gave us, its pcb and the trap frame `cpu_fork` copies live in the static. `main`
  sets `curproc` first and builds process 0 as `init_main.c` does.
- Processes (M5-b, part 2, the scheduler): the sleep queues, `mi_switch`, the run queues,
  `fork1` and the kernel threads are OpenBSD's, single-CPU (`MULTIPROCESSOR` paths such as
  stealing, `SPCF_SHOULDHALT` and the barrier task are not configured, `sched_choosecpu` is
  `curcpu()`). The machine contract gained `cpu_switchto`, `cpu_fork`, `clear_resched`,
  `cpu_unidle`, the idle hooks, `cpu_info_foreach` and the mutex nesting counter. The
  context switches are the kernel-thread subsets of `locore.S`/`cpuswitch.S`: stack
  pointers, `curproc`/`curpcb`/`p_cpu`/`p_stat` and, on amd64, `%cr3`; the FPU/xstate and
  user segment handling, the Meltdown CR3s, retguard and the RSB refill come with user
  mode. `proc_trampoline` hands the thread function and its argument to a Rust
  `proc_trampoline_run` instead of calling the function itself (Rust `fn` pointers have no C
  calling convention); the syscall return path after it is M6. Every thread runs on the
  kernel pmap until vmspaces exist: amd64 `pmap_activate` loads it, arm64 `pmap_setttb`
  records `ci_curpm` and leaves `TTBR0_EL1` (still the bootloader's) alone. `uvm_uarea_alloc`
  hands out `USPACE` blocks from the direct map without the guard page (`km_alloc` cannot
  punch a hole in the direct map; the guard returns with `kernel_map`). `cold` and `safepri`
  are `sys/systm.rs` statics like `physmem`. The `selftest=kthread` boot runs two kernel
  threads passing a turn with `msleep`/`wakeup` through the run queues and the idle thread.
- System calls (M6-a): the tables are generated, as in C, but by `cargo xtask gen-syscalls`
  from `syscalls.master` instead of `makesyscalls.sh`; every syscall the tree does not define
  is `sys_nosys` in `init_sysent.rs`, and the generator's `--check` keeps the four files
  current in `just ci`. The entry paths are OpenBSD's: amd64 `Xsyscall` (`syscall`
  instruction, `MSR_LSTAR`) building the trap frame on `ci_kern_rsp`, `syscall()`,
  `mi_syscall` and the AST loop before `sysretq`; arm64 `handle_el0_sync` → `do_el0_sync` →
  `svc_handler`, `do_ast` and `eret`. Not here: the Meltdown U-K page and `Xsyscall_meltdown`,
  the xstate/FS.base restores and the Spectre code patches on amd64; the trampoline vectors
  (`trampoline.S`) on arm64, so `VBAR_EL1` keeps the kernel vectors. `pin_check` is the C's
  since M8: a system call must come from the site the executable's `PT_OPENBSD_SYSCALLS`
  (`ps_pin`) or `pinsyscalls(2)` (`ps_libcpin`) names for its number, or be `sigreturn` from
  the trampoline (`machine::signal`'s `sigcodecall`/`sigcoderet` give the instruction's
  length), else the process gets `SIGABRT`. `copyin(9)` is each arch's
  `copy.S` behind the `machine::copy` contract, with `pcb_onfault` recovery in both page fault
  handlers (amd64 validates it against the `.nofault` table the linker script collects);
  amd64 runs without SMAP's `stac`/`clac` (no `codepatch`, `CR4.SMAP` not set).
- The first user program (M6-b): there is no filesystem, so `init` is a Limine module
  (`module_path: boot():/init` in `limine.conf`, which `cargo xtask image` adds when the
  `init` binary exists) that the boot glue hands over as `BootInfo::modules` and `start_init`
  will exec from memory. `init/` is a freestanding Rust crate (`#![no_std]`, static ELF at
  `0x400000`, raw `syscall`/`svc` with OpenBSD's carry-flag convention) built for the two
  bare targets by `just build-init-*`; it is not OpenBSD code and lives outside `sys/`.
  Since `pin_check` is real (M8) it carries a `PT_OPENBSD_SYSCALLS` table like any OpenBSD
  program: its one system call instruction (`syscall6`, `inline(never)`) emits a
  `.openbsd.syscalls` entry for every system call number, all naming that instruction,
  and `init.ld` puts the section in a segment of type `0x65a3dbe9`. Its `_start` is assembly
  that hands the initial stack pointer to the program, which checks `argc`, `argv` and the
  auxiliary vector `execve` built (`init: argv and auxv ok` in `smoke`).
- Process exit (M6-b): `kern_exit.c`'s `exit1`/`exit2`/`reaper`/`process_zap` are OpenBSD's
  with the pieces that need signals, file descriptors, limits, credentials or a vmspace
  reported; `initprocess` is null until `init` exists and process 0 adopts orphans meanwhile.
  The `selftest=kthread` threads now `kthread_exit` and proc0 checks the reaper freed them.
- User address spaces (M6-b, then M7a): M6 built `exec`'s segments from wired pages outside
  the entry tree; M7a-3b retired those stand-ins. `exec` maps each segment with `uvm_map`:
  a vnode's text and data copy-on-write from its `uvn_attach` object, as OpenBSD does, and
  the boot module's anonymous with the image bytes copied in (`sys/kern/exec_subr.rs`);
  either way the pages are faulted in by `uvm_fault`.
- Raw disk I/O (M10a): `kern_physio.c` is OpenBSD's. A disk's character device (`rdread`/
  `rdwrite`, `sdread`/`sdwrite`) calls `physio(strategy, dev, B_READ|B_WRITE, minphys, uio)`,
  which wires each `minphys`-sized piece of the user buffer with `uvm_vslock_device`
  (bouncing it through `dma_constraint` pages when the device cannot reach them), maps it
  into `phys_map` with the machine's `vmapbuf` (`machine::cpu::Cpu`, `vm_machdep.c` on each
  arch) and runs one private buffer through the strategy routine. `phys_map`, like
  `exec_map`, is a static in `uvm_extern.rs` (`PHYS_MAP`) that each machine's `cpu_startup`
  sets (`VM_PHYS_SIZE` = `USRIOSIZE` pages).
- Exec (M8): `kern_exec.c`, `exec_elf.c` and `exec_subr.c` are OpenBSD's. `sys_execve`
  finds the file with `namei` (`EXECPATH`: the realpath becomes `AUX_openbsd_execpath`),
  checks it (`VOP_GETATTR`, `VOP_ACCESS`, `VOP_OPEN`), reads the header with `vn_rdwr`,
  copies `argv`/`envp` into an `NCARGS` buffer of `exec_map` (a submap every machine's
  `cpu_startup` makes, as in C) and builds the stack: `argc`, the vectors, room for the
  twelve auxiliary vector entries, the strings, a random stack gap (`stackgap_random`),
  `ps_strings` and the execpath. `exec_elf_makecmds` loads `ET_EXEC` and static PIE
  (`ET_DYN`, base from `uvm_map_pie`) executables with `PT_OPENBSD_RANDOMIZE`,
  `PT_OPENBSD_MUTABLE`, `PT_GNU_RELRO`, `DT_TEXTREL` and the `PT_OPENBSD_SYSCALLS` pin
  table; `exec_elf_fixup` writes the auxiliary vector (`AUX_base` is the executable's own
  base for a static PIE, which `rcrt0` relocates itself from). A `PT_INTERP` program loads
  `ld.so` through `elf_load_file`, which fails in `namei` because `ld.so` is not built.
  `exec_timekeep_map` maps the shared timekeep page (wired in `kernel_map`, written by
  `tc_update_timekeep`); where the timecounter has no user-mode reader (`tk_user` 0, the
  i8254 on amd64) libc falls back to `clock_gettime(2)`; amd64's TSC has one
  (`TC_TSC_LFENCE`/`TC_TSC_RDTSCP`). Until a root file system exists, `start_init`
  tries `initpaths[]` through `sys_execve` (each `ENOENT`) and then execs the `init` boot
  module with the same arguments through `exec_image`, the same body with `ep_image` set
  (`docs/C_TO_RUST.md`). What a file system must give `execve`: `namei` of the path, a
  regular-file vnode whose `VOP_GETATTR`, `VOP_ACCESS`, `VOP_OPEN`/`VOP_CLOSE` and
  `VOP_READ` work, and pages through the vnode pager (`uvn_attach`, `uvn_get` over
  `VOP_READ`). Reported: the profiling reset, `cancel_all_itimers` until `kern_time.c`, the
  `NOTE_EXEC` knote and the `/dev/null` fix-up of a set[ug]id exec with a closed standard
  descriptor (the device switch).
- Process system calls (M8): `fork`, `vfork`, `__tfork` (`kern_fork.c`, the child returning
  through the machine's `child_return`), `wait4`, `waitid`, `__threxit` (`kern_exit.c`),
  `futex` (`sys_futex.c`), `sched_yield`, `getentropy` (`dev/rnd.c`; the syscall generator
  also scans `sys/dev`), `reboot`, `utrace` (no `KTRACE`: it succeeds and records nothing),
  `setrtable`/`getrtable` and `sendsyslog` are OpenBSD's. `option ACCOUNTING` is configured,
  as in GENERIC: `kern_acct.c` is whole and the generator takes the `#ifdef ACCOUNTING`
  branch of `syscalls.master`, so `init`'s `acct(NULL)` succeeds. `pledge(2)` parses and
  records the promises but sets neither `PS_PLEDGE` nor `PS_EXECPLEDGE`: the enforcement
  (`pledge_syscall`, `pledge_namei`, `pledge_ioctl`, ...) is not ported, and `init(8)` and
  `ksh(1)` pledge early. `unveil(2)` (`kern_unveil.c`) and `profil(2)` beyond its checks are
  reported. Without `syslogd(8)` (no log device, no sockets) `sendsyslog` writes a
  `LOG_CONS` message to the console and answers `ENOTCONN`, the rest goes to the log stash;
  `ypconnect` answers `EAFNOSUPPORT` without a YP domain. The stand-in `init` forks children
  and waits for them, one of which makes a system call from an unpinned site and dies of
  `SIGABRT` (`pinsyscalls addr ...` and `init: processes ok` in `smoke`).
- Time system calls (M8): `kern_time.c` is OpenBSD's whole file: `clock_gettime`,
  `clock_settime`, `clock_getres`, `nanosleep`, `gettimeofday`, `settimeofday`, `adjtime`,
  `adjfreq`, the interval timers (`setitimer`/`getitimer`; `ITIMER_REAL` through the
  process's `ps_realit_to` timeout and `realitexpire`, the virtual and profiling timers
  through `itimer_update` and the machine's `need_proftick`) and the periodic `resettodr`.
  No time-of-day chip driver is ported (`todr_attach` has no caller), so the clock starts at
  the epoch and `resettodr` has nothing to write. The stand-in `init` checks the clocks, a
  sleep and a `SIGALRM` from `ITIMER_REAL` interrupting `nanosleep` (`init: time ok`).
- `select(2)`, `pselect(2)`, `poll(2)`, `ppoll(2)` (M8, `sys_generic.c`): OpenBSD builds
  them on the thread's poll kqueue (there is no `fo_poll` in `struct fileops` any more), and
  `kern_event.c` is not ported. The system calls and their conversions are, with
  `kqueue_register` a reporting stand-in that fails: without descriptors they sleep for
  their timeout as OpenBSD's do; with descriptors `select` fails with `ENOSYS` and `poll`
  answers `POLLERR` for each. They become real with `kern_event.c` and each file type's
  `kqfilter` (pipes already compute their filters, `PipeFilter`).
- Signals (M7, `kern_sig.c`): the whole file is OpenBSD's, and the traps of both archs call
  its `trapsignal`. The machine half (`sendsig`, `sys_sigreturn`, the `sigcode` trampoline of
  each `locore.S`) is the `machine::MachineSignal` contract; `sys_sigreturn` is entered from
  the table through a forwarding `sys_sigreturn` in `kern_sig.rs`, because the syscall
  generator only scans `sys/kern` and `sys/uvm`. `exec_image` maps the trampoline with the
  C's `exec_sigcode_map` (one shared aobj, `PROT_EXEC`, immutable) and draws a new
  `ps_sigcookie`. amd64 has no FPU code yet (`fpu.c`): `sendsig` copies out the pcb's
  `fxsave`-sized area as it is and `sigreturn` copies it back without `xrstor`, so a handler
  shares the interrupted code's FPU/SSE registers. arm64's trampoline saves the `q`
  registers itself, which needs `fpu_load` (the first FP use of a thread traps): `fpu_save`
  and `fpu_load` are ported, SVE is reported. The kqueue notes, ptrace stops (the code is
  there; nothing sets `PS_TRACED`), core dumps (`vn_open` is reported, so no core is ever
  written) and `pledge_kill` are reported. The stand-in `init` checks `sigaction`, `kill`,
  delivery on the way back from a system call, `sigreturn`, `sigprocmask` and `sigpending`
  (`init: signals ok` in `smoke`).
- User pmaps (M6-b2): amd64 walks a user pmap's tables through the direct map
  (`pmap_get_ptp`, `pmap_enter`, `pmap_do_remove`) instead of borrowing its `%cr3` for the
  recursive mapping (`pmap_map_ptes`), has no pv entries yet and no `pmaps` list, and
  `pmap_pdp_ctor` copies the kernel's whole upper half of the PML4; `cpu_init_msrs` sets
  `EFER.SCE` (the C's `locore0.S` does). On arm64 the bootstrap device map lived in `TTBR0`
  (the lower half), which user pmaps now own: `pmap_init` initialises the pools, remaps the
  console into the kernel half (`pluartcn_remap`), switches `bus_space_map` to kernel-half
  mappings from the `vmmap` range (a 4 MiB window below `virtual_avail`, so device mappings
  never overlap what `kernel_map` hands out; the C takes them from `kernel_map` with
  `km_alloc(kv_any)`), sets `TCR_EL1.T0SZ` for `USER_SPACE_BITS` and points
  `TTBR0_EL1` at the empty table, as the C's `locore` and `pmap_init` do between them; user
  pmaps are three-level, their tables come from the same two-page allocator as the kernel's
  (no `pmap_vp_pool`), and ASIDs are an 8-bit bitmap without rollover. `init` is linked with
  `-z nobtcfi` so `setregs` leaves `pm_guarded` clear (no BTI landing pads yet).
- Autoconfiguration (M7b): `subr_autoconf.c` and `<sys/device.h>` are OpenBSD's and
  `cpu_configure` starts them with `config_rootfound("mainbus")` on both archs. `config(8)`
  is not ported: what it would generate into `ioconf.c` (`cfdata[]` with its locators and
  parent vectors, `cfroots[]`) is written by hand per architecture in
  `sys/arch/<arch>/conf/ioconf.rs`, following `config(8)`'s layout, for the GENERIC lines
  whose drivers exist: `mainbus0 at root`, `cpu0 at mainbus?` and `pci* at mainbus0` on
  amd64; `mainbus0 at
  root`, `ampintc* at fdt? early 1` and `agtimer* at fdt?` on arm64. The tables, `mainbus_cd`
  and `device_register` reach `subr_autoconf.rs` through `machine::autoconf`, so generic code
  never names an arch; the host double serves whatever table a test installs. A device that
  GENERIC configures but whose driver is not ported is reported with `unported!` where its bus
  would probe or attach it (amd64's `bios0`, `isa0`, ...); on arm64 every device-tree node
  and on amd64 every PCI function without a driver prints OpenBSD's `not configured` line. The counts
  `config(8)` writes into `<dev>.h` follow the tables: `NMPATH` is 0, the `hotplug(4)` calls
  are reported. Without ACPI or MP tables, amd64's mainbus attaches the boot CPU as
  `CPU_ROLE_SP`, as the C does on such a machine; `cpu_configure` keeps doing around
  `config_rootfound` what `acpimadt` and the boot processor's attach would add (the LAPIC
  base, `lapic_enable`, `lapic_set_lvt`, `lapic_calibrate_timer`). Adding a driver means its
  `cfattach`/`cfdriver` and one `Cfdata` row in each `ioconf.rs` that has it in GENERIC.
- File descriptors (M7b): `kern_descrip.c`, `<sys/file.h>`, `<sys/filedesc.h>` and the
  read/write/ioctl paths of `sys_generic.c` are OpenBSD's: process 0 gets `fdinit()`,
  `fork1` copies or shares the table, `exec` runs `fdprepforexec`, `exit1` runs `fdfree`,
  and every `read`/`write`/`ioctl` goes through `fd_getfile_mode` and the file's
  `fileops`. What needs kqueues or pledge is reported (`knote_fdclose`, `pledge_*`); the
  vnode paths (`VOP_ADVLOCK`, `VOP_PATHCONF`, `fd_cdir`/`fd_rdir`) are the vfs core's.
- Pipes (`sys_pipe.c`, `<sys/pipe.h>`; John S. Dyson's licence, accepted by the user on
  2026-10-03): OpenBSD's whole file. A pair is a `pipe_pair_pool` item freed with its second
  pipe (`pipe_destroy` is `unsafe`), the buffers are pageable kernel memory
  (`km_alloc(kv_any, kp_pageable)`) that `uvm_fault` fills on `kernel_map`, and a write to a
  pipe without a reader gets `EPIPE` and `SIGPIPE` (`dofilewritev`). Without `kern_event.c`
  there are no knotes: `pipe_wakeup`'s `knote_locked` and `pipe_kqfilter` are reported, the
  filter bodies return what they would set (`PipeFilter`) for `kern_event.c` to apply. The
  host double has no pageable kernel memory, so `pipe_pair_create` fails there and the host
  tests build their pairs with heap buffers; the stand-in `init` checks pipes from user mode
  (`init: pipes ok` in `smoke`).
- The console as a file (M7b, stand-in): in OpenBSD `init(8)` opens `/dev/console`, a
  vnode of the console's character device whose tty does the I/O. Without a root file
  system to hold that node, `start_init` installs `sys/dev/consfile.rs` instead: one `struct
  file` of type `DTYPE_CONSFILE` (127, outside OpenBSD's range), put at descriptors 0, 1 and
  2 of process 1 by `falloc`/`fdinsert`/`fdalloc`. Since M8 its `fileops` are what
  `vn_read`/`vn_write`/`vn_ioctl`/`vn_close` would do for the console's vnode: `cnopen` when
  it is installed, then `cnread`, `cnwrite`, `cnioctl`, `cnkqfilter` and `cnclose` through the
  device switch, so the descriptors are the console's tty (line discipline, `TIOCGETA`,
  `TIOCSCTTY`; `init: tty ok` in `smoke`). `TIOCSCTTY` records the tty in the session but no
  vnode (`s_ttyvp` stays NULL), so `/dev/tty` cannot reach it. A console that is not a tty
  falls back to the polled `cnputc`/`cngetc` path. It goes away when `init` can open
  `/dev/console`.
- The device switch (M8): `<sys/conf.h>` is `sys/sys/conf.rs` (`Cdevsw`, `Bdevsw`, `Linesw`,
  the `cdev_*_init` initialisers as `const fn`s); the tables are each architecture's `conf.c`
  (`arch/<arch>/<arch>/conf.rs`) behind `machine::conf` (`Conf`: `nchrdev`, `cdevsw`,
  `cdevsw_set`, `nblkdev`, `bdevsw`, `chrtoblktbl`, `swapdev`, `mem_no`, `iskmemdev`,
  `iszerodev`, `getnulldev`). Every slot keeps OpenBSD's major number (they are ABI:
  `MAKEDEV(8)` uses them); a slot whose driver is not ported holds `cdev_notdef()`. Present
  today on both archs: `cn` 0, `ctty` 1, `mm` 2 (`mem.c`), `pts`/`ptc` 5/6, `com` 8,
  `filedesc` 22 (its entry points are `kern_descrip.c`'s), `ptm` 81; no block device. The
  tables are `Cell`s so that arm64's `pluartcnattach` can do the C's KLUDGE
  (`cdevsw[com's major] = pluartdev`) at boot; entries are read by copy. Generic code reaches
  them through `crate::machine::conf` (`spec_vnops.rs`, `cons.rs`, `subr_xxx.rs`), never an
  arch module.
- The tty layer (M8): `tty.c`, `tty_subr.c`, `tty_conf.c`, `tty_tty.c`, `tty_pty.c` and their
  headers are OpenBSD's; `struct tty` is a structure of `Cell`s shared by the reading
  process, the driver's interrupt and soft interrupt and the clock (`docs/C_TO_RUST.md`).
  The console is a tty on both archs: amd64 attaches `isa0 at mainbus0` and `com0 at isa0`
  (`isa.c`, `com_isa.c`, the ISA machine hooks behind `machine::isa_machdep`), arm64
  `pluart* at fdt?` (`pluart_fdt.c`, the attach arguments behind `machine::fdt`'s
  `FdtAttachArgs`). Kernel `printf` stays polled (`cnputc`) unless `TIOCCONS` redirects it;
  tty output is interrupt driven (`comstart`/`pluart_start`). What needs `kern_event.c`
  (`ttkqfilter`, `ptckqfilter`, the `klist` of `struct selinfo`) is reported or left out, and
  `PTMGET`/`TIOCCONS` look their `/dev` nodes up with `namei`, which fails with `ENOENT` until
  there is a root file system.
- The VFS core (M7+, stage 1): `vfs_init.c`, `vfs_subr.c`, `vfs_vops.c`, `vfs_default.c`,
  `vfs_cache.c`, `vfs_lookup.c`, `vfs_vnops.c`, `vfs_getcwd.c`, `vfs_syscalls.c`,
  `spec_vnops.c`, `miscfs/deadfs/dead_vnops.c` and their headers (`vnode.h`, `mount.h`,
  `namei.h`, `specdev.h`, `dirent.h`, `lock.h`, `pledge.h`) are OpenBSD's, with no file
  system, no buffer cache and no vnode pager yet. A vnode is a `vnode_pool` item that is
  never freed (`&'static Vnode`, recycled through the free lists); a mount is `malloc`ed
  and reference counted (`&'static Mount`). A file system plugs in with a `static Vfsops`,
  a `static Vops` (one `Option<fn(&mut VopXArgs)>` per operation, `None` answering
  `EOPNOTSUPP`; `docs/C_TO_RUST.md`), its node behind `v_data` (`*mut c_void`, read back with
  `Vnode::data::<T>`) and a `Vfsconf::new(...)` line in `vfsconflist[]` (`vfs_init.rs`; ffs is
  the first) behind a cargo feature named after its `option(4)`. `mountroot` (`sys/systm.rs`)
  is NULL until `setroot` (`subr_disk.c`) and a disk driver exist, so where OpenBSD panics
  "cannot mount root" `main` prints `cannot mount root: no root file system` and goes on
  without a `rootvnode`: every `namei` then fails with `ENOENT` (the C never runs one before
  root is mounted), `check_console` warns that `/dev/console` does not exist, and init is
  still exec'd from its boot module. The stand-in `init` checks that the path system calls
  reach `namei` and fail that way (`init: vfs ok (no root file system)` in `smoke`). What
  stage 2 must bring is reported where the C calls it: the first file system. The device
  switch arrived in M8 ("The device switch" above), so `spec_open`, the character-device
  paths of `spec_vnops.c` and `spec_strategy` call the drivers.
  `pledge` and `unveil` (`kern_pledge.c`, `kern_unveil.c`) are reported for a pledged
  process or an unveiled vnode, which none can be yet; the unveil hooks of `namei` return
  at their `ps_uvpaths == NULL` test. The host tests mount `testfs`
  (`kern/vfs_subr/tests.rs`), a fixed in-memory tree with a real lock discipline, to drive
  `namei`, the name cache, `getcwd` and the vnode life cycle.
- VFS stage 2 (M7+): the buffer cache (`vfs_bio.c`, `vfs_biomem.c`, `kern_bufq.c`,
  `<sys/buf.h>`), the syncer (`vfs_sync.c`) and advisory record locks (`vfs_lockf.c`,
  `<sys/lockf.h>`) are OpenBSD's. Each `cpu_startup` calls `bufinit`, which reserves the
  buffer arena (`bufkvm`, a tenth of the kernel's space capped at `bufpages` pages) in
  `kernel_map`; `main` starts the `cleaner` (`buf_daemon`) and `update` (`syncer_thread`)
  threads; a vnode carries its buffer tree and clean/dirty lists, and `getblk`, `bread`,
  `bwrite`/`bawrite`/`bdwrite` and `brelse` are what a disk file system calls (its
  `VOP_STRATEGY` maps `b_lblkno` and hands the buffer to its device vnode's
  `spec_strategy`). A buffer is a `bufpool` item passed as `&'static Buf` (`docs/C_TO_RUST.md`).
  `spec_strategy` hands the buffer to `bdevsw[].d_strategy` (the device switch, M8), but no
  block device is configured yet; the `DIOCGPART` block size of `spec_read`/`spec_write` is
  reported (`<sys/disklabel.h>`; they use `BLKDEV_IOSIZE`), and no disk driver uses
  `bufq(9)` yet. On the host double (no MMU) the arena is only counted and a buffer's pages
  are one physical segment reached through the direct map. A boot self-test writes and
  reads back anonymous buffers (`selftest: buffer cache ok` in `smoke`).
  The vnode pager (`uvm_vnode.c`, `<uvm/uvm_vnode.h>`) and the vnode half of `uvm_pager.c`
  (the pager map `uvm_pseg_*`/`uvm_pagermapin`, `uvm_mk_pcluster`, `uvm_pager_put`) are
  OpenBSD's: `uvn_attach(vp, prot)` gives the object a mapping uses, pages come in one at a
  time through `VOP_READ` and go out in clusters through `VOP_WRITE`, and an unmapped object
  persists with its pages until `vclean` calls `uvm_vnp_terminate`. `pmap_is_modified` and
  `pmap_clear_reference` joined the `machine::pmap` contract for it (and for
  `uvm_pagedeactivate`). `exec` maps executables through it since M8. Still missing for
  `mmap(2)` of files: `uvm_mmapfile` and the device pager (`uvm_device.c`); the async swap
  pageout of `uvm_pager.c` waits for the swap pager. A boot self-test maps a three-page cluster through the pager map (`selftest: pager
  map ok`).
  A device vnode keeps its `struct lockf_state *` in `specinfo` (`si_lockf`, a
  `LockfStateSlot`), so `spec_advlock` and `vgonel`'s purge are the C's; a file system's
  inode will keep one the same way. `pool_get(PR_WAITOK)` cannot sleep yet, so a lock
  allocation can fail with `ENOLCK` where the C would wait.
- The fast file system (M8): `sys/ufs/ufs` (the UFS layer: `ufs_bmap.c`, `ufs_ihash.c`,
  `ufs_inode.c`, `ufs_lookup.c`, `ufs_vfsops.c`, `ufs_vnops.c` and `dinode.h`, `dir.h`,
  `inode.h`, `quota.h`, `ufsmount.h`, `ufs_extern.h`) and `sys/ufs/ffs` (`ffs_alloc.c`,
  `ffs_balloc.c`, `ffs_inode.c`, `ffs_subr.c`, `ffs_tables.c`, `ffs_vfsops.c`,
  `ffs_vnops.c`, `fs.h`, `ffs_extern.h`) are OpenBSD's whole files, FFS1 and FFS2, behind
  features `ffs`/`ffs2` (both default) with `ffs` in `vfsconflist[]`. OpenBSD has no soft
  updates any more. The on-disk structures keep their C layout: `struct ufs1_dinode`/
  `ufs2_dinode` are plain `#[repr(C)]` integers, `struct fs` and `struct cg` are
  `#[repr(C)]` structures of `Cell`s (the C changes them in place through shared pointers),
  checked offset by offset at compile time and against the headers in `test-ref`. An inode
  is an `ffs_ino_pool` item reached by `vtoi(vp)`, its dinode a pool item behind `DIP`-like
  accessors (`dip_size()`, `dip_set_size()`); the in-core super-block and `ufsmount` are
  `malloc(M_UFSMNT)`ed and reached by `vfstoufs(mp)`. Mounting the root is
  `ffs_mountroot`: `bdevvp(rootdev)` (`sys/systm.rs`'s `ROOTDEV`, set by `setroot`), then
  `ffs_mountfs`, which opens the device (`VOP_OPEN` -> `spec_open` -> `bdevsw[].d_open`),
  reads the super-block at 64 KB, 8 KB or 256 KB through the buffer cache
  (`spec_strategy` -> `bdevsw[].d_strategy`) and needs nothing else from the device: no
  `DIOCGPART`, no disk label. Until the device switch and a disk driver exist,
  `mountroot` stays NULL. Not configured or not ported, and so reported: `option QUOTA`
  (`quota.rs` answers as a kernel without quotas; `ufs_quota_stub.c` has no licence block
  and is skipped), `option UFS_DIRHASH` (directories are searched linearly), `option
  FIFO` (`fifofs`; a fifo on an FFS is refused with `EOPNOTSUPP`), the knotes of
  `kern_event.c` (`VN_KNOTE`, `ufs_kqfilter`), `disk_map` and `inittodr`. The host tests
  build FFS1 and FFS2 images with a `newfs`-like helper (`ffs_vfsops/tests.rs`), mount them
  on a block device vnode whose strategy reads a `Vec`, use them through the system calls,
  and check the counters of the unmounted image as `fsck` would.
- The system's identity (the user's decision, 2026-10-03): the system is **EmiBSD**, release
  **8.0** (the release number tracks the OpenBSD release the reference pin follows: the
  pin's `newvers.sh` has `osr="8.0"` with `STATUS "-current"`; it was 7.8 until
  2026-10-03, a misreading corrected the same day). OpenBSD's
  `conf/newvers.sh` writes `ostype`, `osrelease`, `osversion`, `sccs` and `version` into a
  generated `vers.c` at every build, from a counter file, `date`, `logname` and `hostname`.
  Here `sys/conf/vers.rs` holds those strings, built by `concat!` from what `sys/build.rs`
  passes as `EMIBSD_VERS_*` compile-time variables, and `build.rs` reads only its
  environment: `EMIBSD_BUILD` (the build number), `SOURCE_DATE_EPOCH` (the build date,
  printed in `date(1)`'s format in UTC), `USER`, `EMIBSD_BUILD_HOST` and the `sys/`
  directory. The justfile exports the commit count, the last commit's time and `hostname
  -s`, so one commit gives one kernel: no clock, no counter file, no network. The result is
  `EmiBSD 8.0 (GENERIC) #<commits>: <date>\n    <user>@<host>:<dir>\n` (`STATUS` is the
  release one, empty; the configuration name is always `GENERIC`), `osversion` is
  `GENERIC#<commits>`. `kern.ostype`/`kern.osrelease`/`kern.version`/`kern.osversion`
  (`kern_sysctl.c`) and the line each `cpu_startup` prints after the copyright, as OpenBSD's
  do, come from there; `kern.osrevision` stays the `OpenBSD` API date of `<sys/param.h>`
  that programs test, and the copyright notice stays OpenBSD's text (it is the licence
  notice, not the identity). The stand-in `init` checks the identity through `sysctl(2)`
  (`init: EmiBSD 8.0` in `smoke`).
- `sysctl(2)` (`kern_sysctl.c`): the helpers take user addresses as `usize` and the name as a
  slice; structures are copied out as bytes through `sys::sysctl::SysctlPlain`, which C does
  with a `void *` and a size. Nodes whose variable or subsystem is not here yet report
  themselves with `unported!`, so a walk of the tree prints its gaps on the console.
- DMA (M7b): `bus_dma.c` is ported per architecture (amd64 bounce pages below
  `dma_constraint`; arm64 cache maintenance unless the tag is `BUS_DMA_COHERENT`, the
  `dma-coherent` copy of `mainbus_dma_tag` per node) and reached through `machine::bus`'s
  `BusDma`. The host double has no DMA (its operations fail with `EOPNOTSUPP`); `bus_dma` is
  tested on the machines instead, by a boot self-test each `cpu_configure` runs on its own tag
  (`selftest: bus_dma ok`). Since `km_alloc` maps `kernel_map` space (`kp_none` gives virtual
  space only), `bus_dmamem_map` and the bounce maps work on both machines.
- PCI (M7b): `dev/pci/pci.c`, `pci_map.c`, `pci_subr.c`, `pci_quirks.c` and the headers are
  OpenBSD's; the machine side is `machine::pci_machdep`. On amd64, without ACPI, mainbus
  attaches `pci0` for bus 0 (as the C does when `acpi_haspci` is false) and configuration
  space is reached with mechanism #1 (ports `0xcf8`/`0xcfc`). The extents (`sys/extent.h`)
  are not ported, so a bus reserves nothing and a BAR the firmware left at 0 cannot be placed.
  `mp_busses` is NULL (no `mpbios`/`acpimadt`), so interrupts map to the line register and
  the 8259, and MSI/MSI-X are refused exactly as the C refuses them without tables (the
  routing functions are ported; their `ioapic_edge_stubs` wait for the I/O APIC half of
  `vector.S`). `option PCIVERBOSE` is not configured: the 800 KB name tables
  (`pcidevs_data.h`) and most of `pcidevs.h` are generated by `devlist2h.awk` in C and would
  need a generator in `tools/xtask` (as `gen-syscalls` is for `syscalls.master`), so the attach
  lines print IDs (`vendor 0x8086 product 0x29c0 (class bridge subclass host, rev 0x02) at
  pci0 dev 0 function 0 not configured`) and `pcidevs.rs` holds only the IDs ported code
  names. On arm64 (M12) `pciecam` (`dev/fdt/pciecam.c`) attaches QEMU `virt`'s ECAM host
  bridge from the device tree: it maps the 256 MiB ECAM region (beyond the `vmmap` window,
  so `generic_space_map` takes `km_alloc(kv_any)` space as the C does), gives the bus a copy
  of its parent's bus space that translates PCI addresses through `ranges`, routes INTx
  through `interrupt-map` (`arm_intr_establish_fdt_imap`) and MSI/MSI-X through
  `msi-parent` to the GICv2m frame (`ampintcmsi`, attached below the GIC by
  `simplebus_attach` as in C; `arm_intr_establish_fdt_msi`), loading the doorbell into a
  DMA map and programming the function with `arm64/pci_machdep.c`. The extents are absent
  there too, so BARs must be assigned by the firmware (EDK2 does). `virtio* at pci?` is
  configured on arm64 as in GENERIC. Memory BARs are mapped by amd64's `bus_space.c` memory half (`x86_mem_add_mapping`:
  `km_alloc(kv_any, kp_none)` and uncached `pmap_kenter_pa`, as the C does).
- virtio (M7b): `dev/pv/virtio.c` and its headers are OpenBSD's, with both transports:
  `virtio_pci.c` (`virtio* at pci?`, amd64; QEMU's transitional virtio-net-pci attaches with
  the virtio 1.0 capabilities) and `virtio_mmio.c` (`virtio* at fdt?`, arm64; QEMU `virt`'s
  32 `virtio,mmio` nodes attach, the empty ones print `Virtio Unknown (0) Device` as OpenBSD
  does). The rings are DMA memory the device changes, so the core reaches them only through
  raw pointers with volatile accesses; the barriers `virtio_membar_*` are a new machine
  contract, `machine::atomic` (amd64 compiler barriers and `mfence`, arm64 `dmb st/ld/sy`).
  `virtio_mmio`, a machine-independent driver, gets `struct fdt_attach_args` and
  `fdt_intr_establish` through `machine::fdt` (a generic associated type per machine; amd64
  and the host double have the members but never attach anything with them), and
  `intr_barrier` joined `machine::intr`. The C's `#if defined(__amd64__)` around forcing MSI
  for virtio is `machine::pci_machdep::PCI_MSI_PER_BRIDGE`. Interrupts: amd64 has no MP
  tables, so `pci_intr_map_msi*` refuse and the device's INTx line (the one the firmware
  wrote) is established on the 8259; arm64's comes from the node through `ampintc`.
- USB (M12): the machine-independent core (`dev/usb/usb.c`, `usbdi.c`, `usb_subr.c`, ...)
  runs under the kernel lock at `splusb()`, as in OpenBSD. xhci(4) (`dev/usb/xhci.c`,
  `dev/pci/xhci_pci.c`) attaches at PCI on both archs (INTx on amd64 QEMU, MSI-X through the
  GICv2m frame on arm64). Its interrupt is `IPL_MPSAFE` as in OpenBSD: the handler only
  reads the status registers and schedules the USB soft interrupt, so the `usbd_bus` members
  it touches (`use_polling`, `dying`, `no_intrs`) are relaxed atomics. The TRB rings,
  contexts and tables live in DMA memory and are reached only through bounds-checked
  volatile accessors (`XhciTrbRef`, the `*_ctx_update` closures). uhub(4) drives both the
  emulated root hub and external hubs.
- Audio (M12): audio(4) (`dev/audio.c`) is machine-independent; drivers reach it only
  through `AudioHwIf`, `audio_attach_mi` and `audio_pintr`/`audio_rintr`, called with
  `AUDIO_LOCK` held. azalia(4) attaches QEMU's `intel-hda` on both architectures (through
  pciecam on arm64) and auich(4) with ac97(4) its `AC97` on amd64 (GENERIC has auich on
  amd64 only). QEMU's HD Audio controller stops fetching commands while a RIRB interrupt is
  unacknowledged, and the handler cannot run during autoconf, so `azalia_get_response` does
  the handler's RIRB work itself when the flag is up (a deviation, harmless on hardware).
  Userland plays with aucat(1), which falls back to `/dev/audio0` (`rsnd/0`) when no
  sndiod(8) runs, as `sio_open(3)` does; sndiod itself is not needed for the smokes.
- QEMU's disks (M10a, `boot.rs`, `qemu_command`): besides the boot image every VM has one
  persistent virtio-blk disk, the raw 64 MiB sparse file `target/disk-<arch>.img`
  (`disk-<arch>-a.img` / `-b.img` for `smoke2`'s two VMs; in `target/smoke/<recipe>/` instead
  of `target/` when `just smoke` runs the recipe, see "Parallel smokes"), created zero-filled when missing
  and reused as is, so what a guest wrote survives the next boot; `--disk-fresh` (`qemu`,
  `smoke`, `smoke2`) recreates it. It is added after every NIC: on amd64 it is
  `virtio-blk-pci` on a later PCI slot (the NIC stays `virtio0`/`vio0`, dev 2; the boot
  image is on q35's AHCI, which is not ported); on arm64 `virt` it takes the lowest
  virtio-mmio slot in use (slots go out top down, the kernel attaches bottom up), so it is
  the first block device found (`sd0`) and the boot disk (`virtio31`) the second (`sd1`).
  `--nvme FILE` (M13a, `qemu` and `smoke`; `hwopts.rs`, the home of M13's QEMU device
  options) adds an NVMe controller whose namespace is FILE, on amd64 only (arm64 gets PCI
  with M12): it is added right after the NICs, so it takes slot 3, attaches before the
  virtio-blk disk (then at slot 4) and its namespace is `sd0`.
- QEMU's M13 devices (`tools/xtask/src/hwopts.rs`, one option each, hooked into
  `boot::qemu_command` by one call): `--scsi-cd ISO` (`qemu`, `smoke`, `smoke2`) adds a virtio
  SCSI adapter with a read-only `scsi-cd` drive holding the file (`virtio-scsi-pci` on amd64,
  `virtio-scsi-device` on arm64) as the LAST device of the command line, so the numbering of
  the other virtio devices does not change: amd64's PCI slots go up (`vioscsi0 at virtio2`,
  after the NIC and the disk), and on arm64 the adapter takes the lowest virtio-mmio slot, so
  the kernel finds it first (`vioscsi0`, and its `scsibus0`) while the NIC and the disks keep
  the slots, hence the names, they have without it. `smoke-cd` mounts the ramdisk's makefs ISO
  through it (`cd0`, `mount_cd9660 /dev/cd0c`).
- `disklabel(8)` and `fdisk(8)` embed their manual page in a generated `manual.c` rendered
  with mandoc(1); the userland build takes their Makefiles' own `.ifdef NOMAN` branch
  (`NOMAN_PROGRAMS` in `tools/xtask/src/userland.rs`), so the embedded page reads
  `no manual`. Only that text differs.
- `vio(4)` (M7b): `dev/pv/if_vio.c` is OpenBSD's whole driver (`vio* at virtio?`), attached
  with the NIC API of the network-interface layer (`if_attach`, `ether_ifattach`, one send
  and one receive queue). QEMU's user-mode network gives it no offloads (slirp has no
  virtio-net header), so it runs with `MRG_RXBUF`, event indexes, indirect descriptors and
  the control queue. Not ported and reported where called: `intrmap(9)` (multi-queue is only
  asked for with more than one CPU), `ifmedia` (`net/if_media.c`: only the five media words
  of `<net/if_media.h>` it reports are here), `tcpstat`. The interrupts work on both
  machines: amd64's INTx through the 8259 (q35's firmware routes the PIRQ to IRQ 11),
  arm64's SPI through `ampintc`; the `selftest=vio` boot brings `vio0` up through `ifioctl`
  (the control queue's answers arrive only through the interrupt once `cold` is over),
  stops the receive tick, sends an ARP request built by hand and sees QEMU's answer reach
  `ifiq_input` through `vio_rx_intr`, where `ether_input` reports `arpinput` until netinet
  is here.
- Network interfaces (M7b): `net/if.c`, `net/ifq.c`, `net/if_ethersubr.c` and `net/if_loop.c`
  are OpenBSD's. `netlock` lives in `net/if_.rs` (as in `if.c`) and the `NET_LOCK()` family
  is `sys/systm.rs`'s functions over it; `main` runs `ifinit` and `softnet_init` (one softnet
  task queue, `NET_TASKQ` 1 without `MULTIPROCESSOR`) and attaches the pseudo-devices from
  `pdevinit[]`, which each `ioconf.rs` lists (only `loop`, so `lo0` attaches at boot) and
  `machine::autoconf::pdevinit()` hands over. SMR is not ported: the interface index map is
  read without a lock and replaced under its rwlock as in C, but the old map is freed at once
  (`smr_call`) and `smr_barrier` is empty, which is sound on one CPU with a kernel that is not
  preempted. `MPLS` (and, until M9+, `INET6`) and the pseudo-devices that are not
  ported (`vlan`, `bridge`, `carp`, `pf`, `bpfilter`, `kstat`, `af_frame`, ...) are not
  configured: their code is a comment at each site. A driver embeds a `struct arpcom`
  (all-zero valid, so it fits an `M_ZERO` softc; `Rwlock`'s name became an `Option` for
  this), calls `if_attach(&ac.ac_if)` and `ether_ifattach(&ac)`, and hands received frames
  to `if_input`.
- IPv4 and routing (M7b): `net/art.c`, `net/rtable.c`, `net/route.c`, `netinet/in.c`,
  `if_ether.c` (ARP), `ip_input.c`, `ip_output.c`, `ip_icmp.c` and the checksums are OpenBSD's.
  `main` calls `rtable_init` before the pseudo-devices and `domaininit` after them, as in C;
  `domains[]` (`kern/uipc_domain.rs`) holds `inetdomain` and `routedomain`, and `unixdomain`
  is reported until sockets exist. There are no sockets: the routing socket half of
  `net/rtsock.c` that `route.c` calls (`rtm_miss`, `rtm_addr`, `rtm_ifchg`, ...) builds its
  messages and reports their delivery, and a kernel caller of `in_control` passes a NULL
  socket, which counts as privileged. SMR and SRP are not ported, so routes and ART tables
  are freed at once instead of after a grace period, sound on one CPU with a kernel that is
  not preempted. `ip_ctloutput` and the other socket options wait for sockets; TCP, UDP, raw IP, IGMP, IPv6, IPsec, `pf`, `carp`, multicast routing and divert
  are not configured or report themselves (`netinet/in_proto.rs`). ARP's `rt_expire` 0 means
  "permanent", so an entry made while `time_uptime` is still 0 never expires and never
  re-asks; the boot ping selftest (`kern/selftest.rs`, feature `qemu`) waits for the first
  second of uptime before it configures `10.0.2.15/24` on the first Ethernet interface,
  adds the default route through `10.0.2.2` and sends an ICMP echo.
- `unported!("name")` (`sys/kern/unported.rs`) marks every call into a subsystem that is not here
  yet: it prints once per site and yields `ENOSYS`. The serial transcript of a boot is therefore an
  honest list of what the kernel skipped.
- Physical memory and the direct map (M3): the memory handed to `uvm_page_physload` is the boot
  protocol's usable regions (already without the kernel, the firmware and the bootloader's data),
  so the BIOS/EFI map walks, `avail_end`, the ISA hole and arm64's `memreg_*` bookkeeping have
  nothing to do. Both pmaps use the bootloader's higher-half direct map (`BootInfo::hhdm_offset`)
  as `__HAVE_PMAP_DIRECT` until the kernel owns its page tables: `pmap_direct_base` is that
  offset, `pmap_bootstrap` does not build the direct map's tables, and `virtual_avail` on amd64
  starts above the direct map when Limine places it at `VM_MIN_KERNEL_ADDRESS`. OpenBSD arm64 has
  no direct map and no `PMAP_STEAL_MEMORY` (it uses `pmap_steal_avail` and maps page by page);
  here it has both, so `uvm_pageboot_alloc` works before any `pmap_kenter_pa` exists. The
  `vm_physmem[]` half of amd64's `pmap_steal_memory` is `uvm_page_physsteal` (`uvm/uvm_page.rs`),
  shared by amd64, arm64 and the host double instead of being written three times.
- Kernel page tables (M3, part 2): both kernels keep running on the bootloader's tables and
  extend them. amd64 adopts the PML4 in `CR3` as `pmap_kernel()->pm_pdir`, installs the recursive
  mapping in slot 255 itself (the C's `locore0.S` does) and counts the kernel's PTPs from
  `virtual_avail` (above the direct map, which shares PML4 slot 256); `pmap_alloc_level` keeps the
  page-table pages the bootloader already installed. arm64 copies the bootloader's level-0 table
  and the level-1 table of the kernel's slot into `pmapvp0`/`pmapvp1` so the vp shadow exists,
  switches `TTBR1_EL1` to the copy and fills `MAIR_EL1` indices 2 to 4 around the bootloader's
  0 (write-back) and 1 (device); the kernel pmap is four-level where OpenBSD's is three-level,
  and `pmap_growkernel` populates the first GiB that the C's `pmap_bootstrap` pre-allocates.
  The level-2/3 tables of the kernel image and of the direct map stay the bootloader's on both
  archs. `pg_nx` comes from `EFER.NXE` as the bootloader left it. `uvm_km_init` only records the
  kernel map's bounds until `uvm_map.c`; `kern/selftest.rs` (feature `qemu`) maps a page there
  at boot and `smoke` asserts `selftest: pmap kernel mapping ok` on both archs.
- Kernel allocators (M3, part 3): `km_alloc` has no `kernel_map`/`kmem_map` yet (`uvm_map.c`
  is M6), so every request is served physically contiguous through the direct map, which the C
  does only for single pages and single segments; `kmem_map` is therefore the direct map and
  `kmemusage` has one entry per loaded page frame. `pool(9)` and `malloc(9)` are ported on top
  with their locks reduced to assertion flags (M5), no sleeping (`PR_WAITOK`/`M_WAITOK` fail
  where the C would wait), no idle-page timestamps (`getnsecuptime` is in `kern_tc.c`, whose
  beerware licence needs the user's decision) and the freelist poison (`subr_poison.c`) reported.
  `dev/rnd.rs` is a placeholder stream (SplitMix64, constant seed, NOT random) behind
  `arc4random`, which pools and `XSIMPLEQ` need for their cookies, until the entropy pool and
  ChaCha20 land (M5). `kern/rust_alloc.rs` is the Rust `GlobalAlloc` over `malloc(9)`
  (`M_TEMP`, `M_NOWAIT`); feature `alloc` is on by default. `physmem` lives in `sys/systm.rs`
  (the C defines it per arch) and `<machine/intr.h>`'s `IPL_*` are the `machine::Intr` contract.
- `uvmexp` is a static of atomics (exported under its C name so the amd64 interrupt stubs can
  count `V_INTR`) and the page-queue locks (`uvm_lock_pageq`, `uvm_lock_fpageq`) are no-ops
  until the pools and uvm take the mutex (M5): the boot CPU is alone. `wakeup` and `uvm_wait` report
  themselves unported, so a `UVM_PLA_WAITOK` allocation that cannot be met fails with `ENOMEM`
  instead of sleeping.
- Licences: `ddb/` and the `db_*` arch files carry the Mach licence (Carnegie Mellon);
  `dev/ic/comvar.h` and amd64 `include/bus.h` have a BSD block with the 4-clause advertising
  clause. Since 2026-10-04 every licence in the pinned OpenBSD tree is accepted (the user's rule,
  `.claude/rules/scope-and-stubs.md`). A translation is still a derivative work, so each ported
  file keeps its original licence block whatever the language; code from outside the tree needs
  the user's decision.

- The crypto framework (`sys/crypto`, M9b/M9c) is the software driver only: `crypto.c`,
  `cryptosoft.c`, `xform.c`, `criov.c` and the primitives they and WireGuard use. `cryptop_pool`
  is not ported (a request is a value, see `crypto/crypto.rs`). The IPCOMP transform
  (`CRYPTO_DEFLATE_COMP`, `comp_algo_deflate`, `xform_ipcomp.c`'s `deflate_global`) runs on
  the `libz` crate's deflate and inflate (M9+). `crypto_init` and `swcr_init`
  run in `main` after the pseudo-devices, as `init_main.c` calls them under `#ifdef CRYPTO`
  (GENERIC's `option CRYPTO`, M9b). Primitives
  whose C is public domain (`chacha_private.h`, `poly1305`, `rijndael`, `sha1`, `md5`, `cast`)
  keep their notice verbatim between the licence markers (accepted 2026-10-03).

Every file-level deviation is in that file's `//! ## Deviations` list and in `ports.toml` `notes`.

- rd(4)'s image (M8): OpenBSD links a RAMDISK kernel with an `rd_root_image[]` array that
  `rdsetroot(8)` fills with a file system image. Here the image is a Limine module,
  `/ramdisk.ffs` on the ESP (`target/userland/<arch>/ramdisk.ffs`, made by `makefs(8)` in
  `just userland`; `cargo xtask image` adds it when it exists, `--ramdisk none` leaves it out),
  which `sys/stand` hands to `rd_root_image_set` before `main`. The bootloader maps modules
  read-write in its direct map and never reclaims them, so rd(4) reads and writes the image in
  place, as the C does with its array. Why: the kernel stays one ELF whatever the userland,
  and no tool has to patch it. `pseudo-device rd 1` (from RAMDISK, not GENERIC) is in each
  `ioconf.rs`; without a module rd0 attaches with an empty image and the boot says
  `rd: no ramdisk module`.

- pf(4) (M9d): `pseudo-device pf` and `pflog` are configured as in GENERIC (`/dev/pf` is
  character major 73 on both archs, `pfattach` and `pflogattach` run from `pdevinit[]`), and
  so is `stoeplitz` (pf's state hashes, `inp_flowid`), and so are `pseudo-device pfsync` and
  `pflow` (the user's decision of 2026-10-03): `pfsyncattach` and `pflowattach` in
  `pdevinit[]`, `IPPROTO_PFSYNC` in `inetsw[]`, `net.pflow` in `net_sysctl`, the pf and IPsec
  hooks. `INET6` is configured since M9+ (feature `inet6`), so pf's IPv6 and NAT64 (`af-to`, `netinet/inet_nat64.c`) paths are real.
  `bpf(4)` is not configured, so `pflog0` exists and counts but `pflog_packet` taps nothing.
  Divert sockets (`netinet/ip_divert.c`) are not ported: `divert-packet` rules report
  themselves and drop the packet. The ABI structures pf shares with pfctl(8) keep the C
  layout to the byte; `net/pfvar/tests.rs` checks their sizes and offsets against clang's.

- amd64's TSC under QEMU (the user's decision of 2026-10-04). OpenBSD registers the TSC
  timecounter only with `CPUF_CONST_TSC` and `CPUF_INVAR_TSC`, which on AMD both come from
  cpuid 0x80000007 `%edx` bit 8 (invariant TSC). QEMU's TCG never sets it (QEMU 11.1.2, Apple
  Silicon host, so TCG only):

  | `-cpu` | 0x80000007 `%edx` | cpuid(1) `CPUID_TSC` | highest leaf |
  |---|---|---|---|
  | `qemu64` | 0 | set | 0xd |
  | `qemu64,+invtsc` | 0 | set | 0xd |
  | `max,+invtsc` | 0 | set | 0xd |

  With `+invtsc` QEMU warns `TCG doesn't support requested feature:
  CPUID[eax=80000007h].EDX.invtsc [bit 8]`, so `boot.rs` keeps plain `qemu64`. TCG's TSC is
  monotonic all the same (it follows the host clock). Under feature `qemu` only,
  `identifycpu` sets both flags whenever cpuid(1) reports `CPUID_TSC`. The frequency is then
  unknown to CPUID (no leaf 0x15, and the P0 MSR is AMD family 17h/19h hardware), so
  `tsc_timecounter_init` takes `identifycpu`'s `cpu_freq` (the TSC over a 100 ms
  `i8254_delay`). OpenBSD would leave such a TSC at quality -1000 until `acpitimer` or
  `acpihpet` call `cpu_recalibrate_tsc`; neither is ported, and the i8254 cannot be the
  reference (`measure_tsc_freq` delays 100 ms with interrupts off; the 15-bit count wraps
  every 27 ms). So, also under `qemu` only, a TSC no reference has recalibrated gets the
  quality 2000 `calibrate_tsc_freq` gives a calibrated invariant TSC. Without the feature the
  C's rules apply unchanged.
- amd64's TSC synchronisation test with `MULTIPROCESSOR` (M11b). `cpu.c` runs `tsc.c`'s test
  against each application processor where the C does, and a failure prints the C's
  `tsc: cpu0/cpuN: sync test failed` and drops the TSC to quality -1000. The C prints nothing
  when the test passes, and nothing for the APs it skips after a failure. So under features
  `qemu` and `multiprocessor` only, `cpu_start_secondary` adds one line per AP:
  `tsc: cpu0/cpuN: sync test passed`, or `... sync test not run: <why>`
  (`tsc_report_verdict`). `smoke-mp` then expects one `tsc: cpu0/cpuN: sync test` line
  per AP, whatever the verdict. Under TCG the test passes: every vCPU reads its TSC from one
  host clock that QEMU keeps monotonic across vCPUs.
- softraid's boot keys (`sr_bootuuid`, `sr_bootkey`) have no source under Limine (M10f).
  OpenBSD's own loaders set them: amd64 boot(8) through `bios_bootsr`, arm64 efiboot through
  the `openbsd,sr-bootuuid` and `openbsd,sr-bootkey` properties. Here they stay zero
  (`replaced-by-limine`; comments mark both `machdep.rs` sites), so no crypto volume is
  unlocked at boot. It is unlocked afterwards with `bioctl -c C -p <passfile> -l <chunk>`.

## Testing architecture

See `.claude/rules/testing.md`. Host tests exist because of `arch/host`. QEMU smoke tests exist
because `custom_test_frameworks` is unstable. Reference-backed tests exist because constants copied
by hand drift.

Two-machine tests (the M9b/M9c tunnels) are `cargo xtask smoke2` (`tools/xtask/src/twovm.rs`):
two QEMUs of one arch run at once, each with its own image and EDK2 variable store, each
driven by its own `send-after`/`expect` script; the run passes when both are done. The second
virtio-net NIC of each VM (`vio1`) is on QEMU's `dgram` netdev, a pair of UDP sockets on
localhost: unlike `socket,mcast=` it does not depend on the host's multicast routing, and
unlike `socket,listen=`/`connect=` neither VM has to start first. No kernel change is needed:
`vio* at virtio?` already attaches a second device on both archs. One harness detail follows
from the kernel finding virtio-mmio slots bottom up while QEMU `virt` hands them out top down:
on arm64 the link NIC is added to QEMU's command line before the user-mode one, so that
`vio0` is still the user-mode NIC. `just smoke-link` is the first user and is not part of
`smoke`.

QEMU's command line makes EDK2 boot Limine at once: the boot image's device has
`bootindex=0` (QEMU's `bootorder` fw_cfg file puts it first in the firmware's `BootOrder`, so
the blank persistent disk is no longer tried first: `BdsDxe: failed to load Boot0001 "UEFI
Misc Device"`), and `-boot menu=on,splash-time=0` sets the boot manager's timeout to 0 through
`etc/boot-menu-wait`. ArmVirtQemu otherwise waits its platform default: about 5 s of every
arm64 boot (firmware start to `BdsDxe: starting` went from 5.5 s to 0.5 s). OVMF's default is
already 0, so amd64 boots gain nothing measurable there.

### Parallel smokes

`just smoke` (and so `just ci`) runs its recipes several at a time, four by default, since a
sequential run had grown to about 35 minutes of boots under TCG. Decided on 2026-10-04 (the
user's plan). The shape is one build phase, then one run phase:

- `smoke-build` builds everything a recipe boots, once and in order: the MULTIPROCESSOR
  kernels with `--features qemu`, the init stand-ins, and `smoke-up`'s uniprocessor kernels
  (`build-up`, which leaves `bsd.up` beside the MP `bsd`). No recipe builds while others boot.
- `cargo xtask smoke-all -j N` (`tools/xtask/src/smokeall.rs`) runs each recipe of the
  justfile's `smokes` list as `just --no-deps <recipe>`, N at a time, longest first by the
  time each took last. A recipe runs exactly as `just <recipe>` would, with the same
  expectations; `just <recipe>` alone still builds what it needs and runs in `target/`.
- Every recipe gets a run directory, `target/smoke/<recipe>/` (`EMIBSD_RUN_DIR`, read by
  `boot::run_dir`): its boot images, EDK2 variable stores and persistent disks are there, so
  no two recipes write the same file (QEMU's image locking would refuse the second writer,
  and `smoke-disk` and `smoke-ufsopts` both format `sd0`). The disks persist between runs as
  `target/disk-*.img` do; the boot images and variable stores of a passed recipe are deleted.
  Its output goes to `log` there: one line is printed per recipe as it ends, and the whole log
  of every failed recipe after the last one; the command fails if any recipe failed.
- The other shared resources were already per run: `smoke2`'s link is a pair of free UDP
  ports asked of the system (asked again, up to three times, if QEMU finds one taken in
  between); the HTTPS test servers' ports (8443-8445) are fixed, but only `smoke-https` uses
  them, and a server waits for a port another run holds (another worktree's `just ci`).
- Time limits (180 s per boot, or a recipe's `--timeout`) are multiplied by
  `EMIBSD_TIMEOUT_SCALE`, which `smoke-all` sets to N/2 rounded up (2 for N = 4): the VMs
  share the host's cores, and the limits are there to catch hangs. No expectation changes.

`JOBS=N just smoke` (or `just jobs=N smoke`, or `JOBS=N just ci`) picks another N; `JOBS=1`
runs the recipes one after the other, still each in its own directory.
