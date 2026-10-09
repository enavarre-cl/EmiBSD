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

### Zones in a file (M15)

Every `.rs` under `sys/` and `tools/` (not `init/`) is split into zones by comment lines of their
own: `/* <LICENSES> */`, `/* <CODE> */`, `/* <TESTS> */`, each with its closing marker, in that
order (`.claude/rules/rust-kernel.md` has the layout, `tools/xtask/src/layout.rs` the validator
that `cargo xtask ports check` runs). The reason is reading cost: a ported file is mostly licence
text on top and tests at the bottom, and an agent that wants the code reads
`sed -n '/<CODE>/,/<\/CODE>/p' file.rs`. The tests are inline in the same file, however long:
the former `<name>/tests.rs` files (324 of them) became `#[cfg(test)] mod tests { .. }` in the
TESTS zone, so the module path of every test (`crate::x::tests::name`) is unchanged and one file
holds a module and its tests. Two exceptions are written down: the 22 ports whose C file has no
licence text (or is generated) have no LICENSES zone and `license = "none"` in `ports.toml`, and
the test helpers other modules share (`mod testutil;`, `fn foo_reset()` under `#[cfg(test)]`)
stay in CODE, since they are code for other files' tests. `init/` (a stand-in) is outside.

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
`disksubr.c`, `readdisklabel` and `writedisklabel`; `acpi_machdep.rs` (M13) is the machine half of
`<dev/acpi/acpivar.h>`, each arch's `acpi_machdep.c`: `acpi_map`, the register maps, the SCI, the
global lock, `pwr_action`, `ci_acpi_proc_id`, `cpu_suspended`, and the `ACPI_PRT`/`ACPI_SECTWO`
constants that stand for acpi.c's `#ifdef __amd64__`/`__arm64__` walks; arm64 answers as a
machine without ACPI until M14), all re-exported from `sys/machine/mod.rs`, which also re-exports
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

`sys/arch/<arch>/conf/kernel.ld` (arm64: base `0xffffffff80000000` with physical addresses
from 0 and the entry at `_start`'s offset, from M14; amd64: OpenBSD's layout from
M14, `KERNTEXTOFF` with physical addresses from `0x1000000`, "Boot loaders"),
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
| `multiprocessor` | `option MULTIPROCESSOR` | off by default, so the uniprocessor kernel stays the plain build; `just build` and `just clippy` also build it, and since M11e every `just smoke` recipe boots it except `smoke-up` (the user's decision of 2026-10-03), since 2026-10-07 with `-smp 2` but for the `smp4` group ("Parallel smokes"). M11a: `MAXCPUS` 255/256, the kernel lock and the spinning mutex (`kern_lock.c`), the Limine MP request and the application processors' start (see "Deviations"); `just smoke-mp` boots it with `-smp 4`. M11d: `NET_TASKQ` 8 softnet queues, `softnet_percpu` keeps one per CPU; `just smoke-net-mp` runs both VMs of the two-VM smokes on it |
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
| `libsa`, `boot`, `efi` | `sys/lib/libsa`, `sys/stand/boot`, `sys/stand/efi` | not crates.io crates: OpenBSD code ported here (M14), split into crates as OpenBSD builds libsa as a library and compiles `sys/stand/boot` into every boot program; the boot loaders (`sys/arch/amd64/stand/efiboot`, package `efiboot-amd64`) depend on them, the kernel does not. `libsa` dev-depends on `bsd` for the host tests that pin its header layouts to the kernel's |
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
  OpenBSD), as the install media's crunched programs are: the ramdisk's programs all stay
  static; `ld.so` and the shared libraries (M14, "Shared libraries and ld.so") serve what
  `cc` links.
- M10e's `usr.sbin/portmap` and `usr.bin/showmount` are linked `-static` for the same reason,
  and so are `sbin/mountd` and `sbin/nfsd`, whose Makefiles end with `LDSTATIC=` (OpenBSD ships
  them dynamic). No source or other flag differs. The ramdisk gets what they need from a base
  install: the `_portmap` user and group (28:28, from `etc/`), `/etc/rpc` (the portmapper, nfs,
  mountd and rquotad lines of `etc/rpc`) and `/var/db` (mountd's `mountdtab`); no `/etc/exports`.
- A program whose Makefile sets `BINOWN`, `BINGRP` or `BINMODE` (`login_passwd`: root:auth,
  setuid 4555, in `/usr/libexec/auth`, where `lib/libc/gen/auth_subr.c`'s `_PATH_AUTHPROG`
  looks for BSD Auth styles) gets them in the image (below).
- `ksh` is built by its own Makefile (`-lcurses`, no `-DSMALL`) since M14c, as OpenBSD's
  base set has it: it sets `KSH_VERSION`, which `rc.subr` wants. There is no terminfo
  database yet (`share/termtypes` needs tic(1)): `setupterm` fails quietly, and only
  emacs mode's `clear-screen` needs it. Until M14c it was the install media's `-DSMALL`
  build; that one is now `distrib/special/ksh`, built for the miniroot only (below).
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

### The comp set (M14)

The compiler is OpenBSD's: clang and lld from `gnu/llvm` (LLVM 22.1.6, Apache-2.0 WITH
LLVM-exception, compiled unmodified, not ported; the user's decision of 2026-10-04), built by
OpenBSD's own build glue (`gnu/usr.bin/clang`, `gnu/lib/libcxx`, `gnu/lib/libcxxabi`,
`gnu/lib/libclang_rt`, `lib/librthread`) through `bsdmake.rs`, with the userland's toolchain.
`cargo xtask comp --arch A [--jobs N]` (`tools/xtask/src/userland/comp.rs`, `just comp`)
writes `target/comp/`:

- `host/`: the build tools `gnu/usr.bin/clang/Makefile` builds and runs during the build
  (`llvm-min-tblgen`, `llvm-tblgen`, `clang-tblgen`, with `libLLVMSupport`, `libLLVMTableGen`
  and `libclangSupport`), built for macOS from the same Makefiles, once for both
  architectures. The target trees reach them where the generation rules look
  (`${.OBJDIR}/../../../llvm-tblgen/llvm-tblgen`) through symbolic links.
- `<arch>/sysroot`: `userland`'s sysroot (copied; `just userland` must have run), plus
  `/usr/include/c++/v1` by libc++'s and libc++abi's own `includes` rules (with OpenBSD's
  `__config_site`), `libpthread.a`, `libc++abi.a` (with the libunwind sources OpenBSD builds
  into it) and `libc++.a`.
- `<arch>/obj`: `gnu/usr.bin/clang`'s `SUBDIR` for that `MACHINE` (`Makefile.arch`: the X86
  or AArch64 backend, plus AMDGPU, as OpenBSD builds): every `include/*` directory's
  generation rules (tblgen, `llvm-config.h`, the `.def` files), then the libraries, all
  compiled in one parallel run (`libLLVM.a` is all of LLVM, as `libLLVM/Makefile` includes
  every `libLLVM*/Makefile`), then the programs, linked with `ld.lld` as OpenBSD's
  `c++ -static` would (static PIE: `rcrt0.o`, `-lc++ -lc++abi -lpthread -lm`,
  `-lcompiler_rt -lc -lcompiler_rt`).
- `<arch>/root`: the staging root. The programs at their Makefiles' `BINDIR` with their
  `LINKS` (`clang` as `cc`, `c++`, `cpp`, `clang++`, `clang-cpp`; `ld.lld` as `ld`; `ar` as
  `ranlib`; `llvm-objcopy` as `strip`; ...), stripped; clang's resource headers
  (`/usr/lib/clang/22/include`, by `include/clang/intrin`'s own `install` rule);
  `libclang_rt.profile.a` and `libclang_rt.ubsan_minimal.a` in `/usr/lib/clang/22/lib`; and
  the sysroot's `/usr/include` and `/usr/lib`, what `cc` needs to compile and link there.
  `comp.ffs` is that root as an ffs image (OpenBSD's makefs as `userland` builds it; owners
  and modes as the Makefiles say), the disk `just smoke-cc` mounts.
- `<arch>/licences.txt`: the licence report, as for `userland`.

Incremental: objects are remade only when a source, a header or the command changed (the
`.d` files and `.cmd` stamps of `userland`), archives only when an object is newer, links
only when an input is; installed headers keep their mtime when unchanged. `just comp` is not
part of `ci`: a first build compiles about 2,850 C++ files per
architecture (measured: 30 minutes for arm64 with 5 jobs, while other builds kept the Mac at
a load average near 30), plus about 200 files of build tools for macOS, once; a run with
nothing changed takes 15 to 20 seconds (most of it checking the objects' `.d` files and the
licence report). Its job count defaults to about half the Mac's cores (`COMP_JOBS`):
other builds share the machine.

What of OpenBSD's comp set (`distrib/sets/lists/comp/{mi,md.<arch>,clang.<arch>}`) and of
the base set's compiler is built: `clang` (and links), `ld.lld` (`ld`), `clang-scan-deps`,
`llvm-config`, `llvm-objcopy` (`strip`), `llvm-objdump`, `llvm-readobj` (`llvm-readelf`),
`llvm-symbolizer` (`llvm-addr2line`), `llvm-profdata`, `llvm-cov`, `ar` (`ranlib`), libc++,
libc++abi, libpthread, `libclang_rt.*.a` and the headers. Not built: lldb and lldb-server
(`BUILD_LLDB=no`: the debugger needs ptrace(2) work and a host `libLLVM` for
`lldb-tblgen`; the exit criterion is `cc hello.c`); the profiled `*_p.a` libraries (none are
built anywhere here); `/usr/include/llvm` (libLLVM's `includes`), binutils' `as`, `gdb`,
`ldscripts` (`md.amd64`; GPL, not in the clone); the `mi` list's other libraries and tools
that `userland` builds or not, as its lists say.

Workarounds, each printed by the build (flags only; no source is edited):

- `-fno-ret-protector` (`gnu/usr.bin/clang/Makefile.inc`) is OpenBSD-local; dropped
  (`UNSUPPORTED_FLAGS`).
- macOS's file system ignores case, OpenBSD's does not. An `-I` directory holding a header
  whose name differs from a libc or libc++ header only in case (`llvm/Support/Errno.h`,
  `Locale.h`, `llvm/BinaryFormat/ELF.h`) becomes `-iquote`: `<errno.h>` would find it.
  Where two `-I` directories of one compile hold such a pair (JITLink's `x86.h` and the X86
  backend's `X86.h` in `libLLVM`), the header `CASE_COLLISIONS` names is hidden behind a
  directory of symbolic links to the rest; an unnamed collision stops the build.
- The build tools are built against macOS with OpenBSD's `include/llvm/Config/config.h`:
  `-DHAVE_MACH_MACH_H=1` (its `ENABLE_CRASH_OVERRIDES` code on macOS needs `<mach/mach.h>`)
  and a force-included header mapping `pthread_set_name_np`/`pthread_get_name_np` to
  macOS's `pthread_setname_np`/`pthread_getname_np` (`HOST_FLAGS`, `HOST_COMPAT_H`).
- `bsdmake.rs`: a whole-line comment ends at its newline even after a backslash, as in
  OpenBSD's make (`libclangASTMatchers/Makefile` comments out a continued line); the
  `<bsd.lib.mk>` stand-in includes `<bsd.own.mk>` first, as the real one does (the glue's
  `Makefile.inc` sets `CXX=clang++` unless `COMPILER_VERSION` is clang). A target compile
  whose `CC` or `CXX` is not the cross compiler stops the build.

Deviations: the programs are static PIE executables, like the rest of the userland's;
OpenBSD links them dynamically and ships `libLLVM` as a shared
library (`NOLIBSTATIC`), here `libLLVM.a` is linked into each program.

`just smoke-cc` (not in `smokes`: it needs `just comp`) boots the ramdisk kernel with
`comp.ffs` as `sd0`, mounts it on `/mnt`, runs `/mnt/usr/bin/clang --version` (`OpenBSD
clang version 22.1.6`, `Target: amd64-unknown-openbsd8.0` or `aarch64-unknown-openbsd8.0`),
compiles a hello world with `cc --sysroot=/mnt -static` and runs it. On the way the kernel
reports one gap it works around: `unported: amap_copy: chunking`. Then the plain dynamic link
of an installed system: `chroot /mnt /usr/bin/cc -o /tmp/d /tmp/h.c` (a dynamic PIE linked
against `libc.so.M.m` with `-dynamic-linker /usr/libexec/ld.so`), run in the chroot, and
`ldd /mnt/tmp/d` from the ramdisk, whose own `ld.so` and `libc.so` serve it.

### Shared libraries and ld.so (M14)

A plain `cc hello.c` on OpenBSD makes a dynamic PIE, so the system needs the run-time
link-editor and the shared libc. `userland` builds them (`tools/xtask/src/userland/shlib.rs`)
after the static libraries, as OpenBSD's Makefiles say:

- `libc.so.104.0`, `libutil.so.22.0`, `libm.so.10.1`, `libpthread.so.28.1` (`lib/librthread`,
  also built static, as `comp` does): `bsd.lib.mk`'s `${FULLSHLIBNAME}`. Every object again
  as a `.so` object (`.c.so` and `.S.so`: `${PICFLAG} -DPIC`, `-DSOLIB`; libc's system-call
  stubs by its own `${SASM}` rules), linked `-shared -soname lib${LIB}.so.M.m` with the
  Makefile's `LDADD` (libc: `-nostdlib -lcompiler_rt -Wl,-zinitfirst,-znow`; libpthread:
  `-Wl,-znodelete`) and its `VERSION_SCRIPT` (libc's `Symbols.map` made by its rule from the
  `Symbols.list` files). The version is `shlib_version`'s. Installed in the sysroot and the
  staging root at `/usr/lib`, root:bin 444. `libcompiler_rt` stays static, as on OpenBSD.
- `/usr/libexec/ld.so`: its sources, libc's string functions through its `VPATH`, the
  `dl_<syscall>.o` stubs of its `.for` loop, linked by its rule (`-e _dl_start`, its
  `Symbols.map` and `ld.script`, `--shared -Bsymbolic --no-undefined`) and checked by its
  `CHECK_LDSO` (only `R_X86_64_RELATIVE` / `R_AARCH64_RELATIVE` dynamic relocations; read
  with `llvm-objdump --dynamic-reloc` where the Makefile runs `readelf`). root:bin 444.
- `ldconfig(8)`, `ldd(1)` and `chroot(8)` join `PROGRAMS`, static like the rest of the
  ramdisk. No ramdisk program is dynamic: OpenBSD's install media are static too; dynamic
  programs are what `cc` makes.
- `comp` copies `/usr/libexec` of the sysroot into its root with `/usr/lib`, so `comp.ffs`
  carries `ld.so` and the shared libraries.

Deviations, each written in `shlib.rs`: the host's LLD 17 is given two defaults OpenBSD's lld
has built in (`--undefined-version`: libc's `Symbols.list` names symbols that only other
architectures define; `--ignore-function-address-equality`); ld.so is built with
`STACK_PROTECTOR=` (its `__guard_local` and `__stack_smash_handler` stubs), the branch of
the architectures OpenBSD's clang has no retguard for, because the host's clang has no
retguard and emits the stack protector on amd64 and arm64 too; objects are linked in
Makefile order, not shuffled (`sort -R`), and no relink kits (`/usr/share/relink`,
`ld.so.a`) are made; `test-ld.so` is not built or run on the Mac (as in OpenBSD's cross
builds), `smoke-cc` is that test. The kernel side is `uvm_mmap.c`'s file half
(`uvm_mmapfile` over `uvn_attach`; device mappings other than `/dev/zero` report the
unported `udv_attach`). `__thrsleep`/`__thrwakeup` (`kern_synch.c`) are still `sys_nosys`:
libc's `FILE` locks and libpthread's `_rthread_dl_lock` call them only when two threads
contend.

## The install media (M14c)

OpenBSD's installer is run unmodified: `distrib/miniroot/install.sub`, the arch's `install.md`,
`dot.profile`, over a `bsd.rd` and signed sets that this build makes. Commands (all in
`tools/xtask/src/install.rs`; recipes in the justfile, "the install media"):

- `cargo xtask miniroot|sets|install-media --arch A`, `just install-media-<arch>`: the
  miniroot, `bsd.rd` and the sets in `target/install/<arch>`. Needs `just userland` and
  `just comp`.
- `just smoke-install-<arch>` (`cargo xtask install`, `--check-only` repeats only its
  check boot): the install run; not in `smokes` (a few minutes, below).
- `just smoke-install-boot-<arch>` (`cargo xtask install-boot`): the disk the install made,
  booted through the loader installboot(8) put on it; not in `smokes` either (it needs
  the disk `smoke-install-<arch>` made).

**bsd.rd.** OpenBSD's RAMDISK_CD kernel is a smaller GENERIC with `option MINIROOTSIZE`
and `rdsetroot(8)` patches the file system into it. Here the kernel is our GENERIC MP kernel
with cargo feature `miniroot` (`sys/dev/rd.rs`): `rd_root_image[ROOTBYTES]` and `rd_root_size`
are compiled in under the C names, `ROOTBYTES` is `$EMIBSD_MINIROOTSIZE` sectors (the
justfile's `miniroot_sectors`, 65536 = 32 MiB, the miniroot is padded to it so one kernel
serves every run), and `rdattach` roots on `rd0a` itself. `cargo xtask rdsetroot`
(`rdsetroot.rs`) is `usr.sbin/rdsetroot` over our ELF; `bsd.rd` is that kernel with its debug
information stripped. Deviation: the Limine module `ramdisk.ffs` path stays and wins. The
kernel is built in `target/bsdrd` (in `smoke-build` only).

**The miniroot** (`userland/miniroot.rs`) is built from the arch's list
(`distrib/amd64/ramdisk_cd/list`, `distrib/arm64/ramdisk/list`) with `list2sh.awk`'s meaning
(`COPY SCRIPT LINK SYMLINK MKDIR REMOVE SPECIAL TZ TERMCAP`), the directories of
`mtree.conf`, and `install.sub`, `install.md`, `dot.profile`, `group`, `master.passwd`
(`pwd_mkdb`), `protocols`, `services`, `/dev/MAKEDEV`. Deviations: no crunchgen `instbin`
(each name is the static program `userland` built, from the normal Makefiles; `distrib/special`'s
-DSMALL -Oz variants are not used, except `init`, which must be `-DDEFAULT_STATE=single_user`
without `DEBUGSHELL`/`SECURE`, `more`, and `doas`: the media's own is a root-only `doas -u user
command` with no `doas.conf`, which `install.sub`'s `unpriv` runs `ftp` and `signify` through);
`/dev` from the smoke ramdisk's node table plus `diskmap` (`MAKEDEV ramdisk` makes it;
opendev(3) opens a disk by its DUID through it, and `install.sub` names every partition by
DUID); `usr/mdec/mbr` a 512-byte
stub; no firmware, termcap or Raspberry Pi files; `/etc/signify/openbsd-80-base.pub` is a test
key (`userland/signify.rs`: OpenBSD's signify built for macOS with shims, key pair made once
in `target/install/test-signify`), never OpenBSD's. The entries not satisfied go to
`miniroot.txt`.

**The sets** (`userland/sets.rs`): `base80.tgz`, `comp80.tgz`, `bsd` (our MP kernel as built, debug
info kept; only `bsd.rd` is stripped), `bsd.mp` (the same kernel: OpenBSD ships a
uniprocessor `bsd` beside it, and `install.sub` insists on `bsd.mp` when `hw.ncpufound` is
above 1, installs it as `/bsd` and keeps `bsd` as `/bsd.sp`), `bsd.rd`, `INSTALL.<arch>` (host
m4 on `distrib/notes/INSTALL`), `SHA256` and `SHA256.sig`. A set is its list
(`distrib/sets/lists`) applied to the tree this build has: `userland`'s and `comp`'s roots,
`etc/mtree/4.4BSD.dist`'s directories, the `etc` distribution of `etc/Makefile` (as a table,
with `var/sysmerge/etc.tgz` that `install.sub` extracts after base), zoneinfo from OpenBSD's
`zic` built for macOS, `usr/mdec` (efiboot; `BOOTIA32.EFI` is a zero sector because installboot
copies it; `biosboot` is a placeholder i386 ELF whose one segment is a sector of text saying
so, because installboot loads the BIOS boot record (`md_loadboot`) before it looks at the
disk and never writes it on a GPT disk with an EFI system partition, while the BIOS boot
programs are not built (`stand/biosboot`'s Makefile wants GNU as); `BOOTX64.EFI` and
`BOOTAA64.EFI` come from `target/efiboot/<arch>`, and the set build fails without them). Archives are
written by xtask (ustar with OpenBSD's owner names and ids, hard links) and the host's gzip.
What the lists name and the build lacks is printed and written to `MISSING-<set>.txt` with a
reason: perl, cvs, texinfo and GNU `as`, `gdb`, `ld.bfd` and the binutils tools are GPL and not
in the clone (the user's decision of 2026-10-05), and man pages, terminfo, locale, `usr/share/misc`,
firmware, httpd/nsd/unbound files, the BIOS boot programs and the programs and libraries
`userland` does not build are not built yet. Built files in no list are `EXTRA-<set>.txt`
(our test programs). Every program is static, as in the rest of the userland.

**The install run** (`install.rs`): a fresh 3 GiB disk is `sd0` (the install media's own disk
is `sd1`; on arm64 `virt` the virtio-mmio slots are probed top-down, so the disk QEMU is
given last is `sd0` there too). `bsd.rd` is booted by our efiboot (`BOOTX64.EFI`,
`BOOTAA64.EFI`) from a disk laid out as OpenBSD's `miniroot80.img` (`efiboot-disk`: the
OpenBSD partition's FFS holding `/bsd`, which is `bsd.rd`, and the EFI system partition; the
run answers `boot> `). Its ramdisk holds
`/auto_install.conf` (autoinstall(8): `.profile` starts the install after five seconds when
that file exists, the way OpenBSD does when no DHCP server names a response file; static
`10.0.2.15`, so dhcpleased is not needed), the sets come over HTTP from this machine
(`10.0.2.2:PORT`, the server of `diff-openbsd`), the disk is set up by the arch's
`md_prep_fdisk`: on amd64 a GPT with an EFI system partition (`fdisk -gy`, answer `G`, so
installboot(8) uses its EFI path and copies `BOOTX64.EFI`), on arm64 an MBR whose FAT
partition (`fdisk -iy -b 32768@32768:C`, answer `whole`) `installboot -p` formats and
installboot fills with `BOOTAA64.EFI` and `startup.nsh`; both are then partitioned by
`disklabel -T` (a template: `/` 2400M first, so it is `a`, and 64M of swap), `newfs`, the
sets (`-all bsd bsd.mp base* comp*`) verified by signify. The run ends at `CONGRATULATIONS!`
and the installer's reboot (boot(9)'s `vfs_shutdown`: "syncing disks... done"); a second
boot of the plain `bsd.rd` makes the disk's nodes (`MAKEDEV sd0`), prints its label, mounts
it read-only and lists `/bsd`, `/bsd.sp`, `/usr/bin/cc`, `/etc/rc`, `/usr/libexec/ld.so`,
amd64's `/etc/boot.conf` (arm64's installer writes none: its console is already the serial
one), the EFI system partition (`efi/BOOT/BOOTX64.EFI` or `bootaa64.efi`, and the copy in
`efi/openbsd/`) and runs `fsck_ffs -n` (five phases, clean).

`install-boot` copies the installed disk into a fresh VM's boot image: the firmware (OVMF,
EDK2 AArch64) starts `\EFI\BOOT\BOOTX64.EFI` (`BOOTAA64.EFI`) from its ESP, efiboot reads
amd64's `boot.conf` the installer wrote (`stty com0 115200`, `set tty com0`), loads `/bsd`
and boots it; the kernel finds its root by
boot(8)'s DUID, `/etc/rc` from the base set runs (fsck, pf, the network, rc.firsttime) to
`login:`, root logs in and `cc hello.c && ./a.out` prints `hello from cc 42` (the source is
written with ksh's `print -r`: base has no printf(1) yet).

On an ACPI arm64 machine (`just smoke-install-arm64-acpi`, `smoke-install-boot-arm64-acpi`:
both runs with `--acpi`, QEMU `virt,acpi=on`, the disks and vio0 on PCI) the same media install
and boot; there the media's disk is probed first and is `sd0`, so the answers name the fresh
disk `sd1` (`install.rs`, `target_sd`, checked against the kernel's `sd1: 3072MB` line), with
the run's disks in `target/smoke/smoke-install-acpi` and its logs in `target/install/arm64/acpi`.

Status (2026-10-05): both architectures pass both (`smoke-install-amd64` about 3 minutes with
the media already made, the installer itself 2.5; arm64 about 4.5, the installer 4;
`smoke-install-boot-<arch>` under a minute each). What the installed system lacks shows in
its `/etc/rc` run: the programs `userland` does not build (`sort`, `head`, `cut`, `mktemp`,
`find`, `install`, `printf`, `swapctl`, `ttyflags`, `kvm_mkdb`, `dev_mkdb`, `savecore`,
`ssh-keygen`, `openssl`, `mail`, ...), so the rc.d daemons (`syslogd`, `pflogd`, `ntpd`,
`smtpd`, `sndiod`, `cron`) say `(failed)`: rc.subr accepts base's `/bin/ksh`, the full build
(`install-boot` fails on `wrong shell`); `/dev/random` (`random` is not a driver yet);
`/dev/ttyC*` (wscons is not ported); installboot cannot add its UEFI boot entry (`/dev/efi`,
efi(4), is not ported), so the firmware boots the ESP's fallback `\EFI\BOOT\BOOTX64.EFI`
(`BOOTAA64.EFI`). On arm64 the installed kernel has `rd0` with no image; it once took the
root disk's DUID from a stale buffer and `fsck` failed (`sys/dev/rd.rs`, deviations: an
empty `rd0` reads no label now).

## Boot loaders (M14)

OpenBSD's boot programs are ported as OpenBSD builds them (the user's decision of 2026-10-03,
M14; track A1 did amd64's efiboot): three crates and one binary.

- `sys/lib/libsa` (package `libsa`): the standalone library, as efiboot's
  `Makefile.common` builds it (`__INTERNAL_LIBSA_CREAD`: `open`/`read`... decompress, the
  plain ones are `oopen`...). A leaf like `libkern`: it depends on `libkern` and `libz` only,
  as the C takes `mem*`, `strlcpy` and `inflate` from `${S}/lib/libkern` and `${S}/lib/libz`
  through `.PATH`. The kernel headers libsa includes (`<ufs/ffs/fs.h>`, `<sys/disklabel.h>`,
  `<sys/exec_elf.h>`...) are ported in the kernel crate, tied to kernel types; libsa cannot
  link the kernel, so `sys/lib/libsa/hdr/` declares the parts it reads again, one file per
  header, and the TESTS zone of `hdr/mod.rs` checks their layouts against the kernel's (a dev-dependency on
  `bsd`). The module for `alloc.c` is `sa_alloc` (`alloc` is Rust's crate). Host tests run
  the FFS1, FFS2 and ISO 9660 readers on images OpenBSD's makefs made
  (`testdata/gen_fixtures.py`), and `loadfile` on a hand-made ELF.
- `sys/stand/boot` (package `boot`): boot(8)'s machine-independent files (`boot.c`,
  `cmd.c`, `vars.c`, `bootarg.c`), which OpenBSD compiles into each boot program. The
  command state is passed to the commands (`fn(&mut CmdState) -> i32`).
- `sys/stand/efi` (package `efi`): the UEFI headers of `sys/stand/efi/include` as
  `#[repr(C)]` types, the specification's names kept, function pointers `extern "efiapi"`.
- `sys/arch/amd64/stand/efiboot` (package `efiboot-amd64`, binary `bootx64`): efiboot's
  files; the files of `sys/arch/amd64/stand/libsa` it compiles (`disk.h`, `libsa.h`,
  `mdrandom.c`) and `<machine/biosvar.h>` (`sys/arch/amd64/include/biosvar.rs`, written
  self-contained so the kernel can include it too) are modules by `#[path]`.
- What a library takes from the program at link time in C (libsa's `file_system[]`,
  `devsw[]`, `constab[]`, `devopen()`, `_rtt()`, `LOADADDR`; boot(8)'s `machdep()`,
  `devboot()`, `run_loadfile()`, the `machine` table...) is one table of `fn` pointers and
  slices the program registers at its entry (`libsa::stand::SaConf`, `boot::boot::BootMd`,
  efiboot's `conf.rs`); the C options that select those routines (`MDRANDOM`, `FWRANDOM`,
  `HIBERNATE`, `BOOT_STTY`...) are its `Option`s (`docs/C_TO_RUST.md`).
- `SOFTRAID=yes` (bootx64's Makefile) is the cargo feature `softraid` of efiboot, not ported
  yet: its libsa files are `todo` in `ports.toml`, and efiboot says so where the C opens or
  probes softraid volumes. `IDLE_POWEROFF`, which the option also defines, is compiled with
  the feature.
- The banner names the system as the kernel does ("The system's identity"):
  `>> EmiBSD/amd64 BOOTX64 3.71`, where the C prints `OpenBSD`.

Building BOOTX64.EFI (`just efiboot-amd64`, part of `just build`). Deviations, each for a
reason:

- The UEFI Rust targets are not installed (and the toolchain is not changed): efiboot is
  built for `x86_64-unknown-none`, linked position-independent at 0 by
  `sys/arch/amd64/stand/efiboot/ldscript.amd64`, and made a PE32+ image the way OpenBSD's
  arm64 efiboot makes its own: a hand-written PE header (`.peheader`, first in the image,
  in `start_amd64.S` in front of amd64's `_start`), one `.text` and one `.data` section
  (rodata, data, GOT, bss as file bytes, then `.dynamic` and `.rela.dyn`), and
  `llvm-objcopy -O binary` (`cargo xtask efiboot`; llvm-objcopy is the toolchain's
  `llvm-tools`). OpenBSD's amd64 Makefile uses GNU objcopy's `efi-app-x86_64` output
  target, which llvm-objcopy does not have.
- `self_reloc` applies the image's `R_X86_64_RELATIVE` relocations at entry, as in C. The
  code is compiled with `-C relocation-model=pie` (the `RUSTFLAGS` of the recipe, which
  replace `.cargo/config.toml`'s `relocation-model=static` for this build only): with `pic`,
  calls between crates go through the GOT, which `self_reloc` itself would call before it
  is relocated. The build has its own target directory (`target/efiboot`), so switching the
  flags never rebuilds the kernel. `build.rs` passes the script, `-Bsymbolic`,
  `--pack-dyn-relocs=none` (efiboot's `LDFLAGS`) and `-z norelro` (one `.data`).
- `just smoke-efiboot` boots it under OVMF from a disk laid out as OpenBSD installs one
  (`cargo xtask efiboot-disk`: an MBR with the OpenBSD partition, its disklabel and an FFS
  holding `/bsd`, made by OpenBSD's makefs; and the EFI system partition holding
  `EFI/BOOT/BOOTX64.EFI`), types at `boot>` and stops once efiboot has loaded the kernel
  and boots the kernel to `login:` (next section).

arm64's efiboot, BOOTAA64.EFI (M14 track A3; `just efiboot-arm64`, part of `just build`,
output `target/efiboot/arm64/BOOTAA64.EFI`, which `installboot` copies from `/usr/mdec`):

- `sys/arch/arm64/stand/efiboot` (package `efiboot-arm64`, binary `bootaa64`): every file of
  OpenBSD's directory, on the same three crates and registration tables as amd64's.
  OpenBSD's arm64 Makefile already makes the PE image with its `.peheader` in `start.S`
  and `objcopy -O binary`, which is what is done here, for `aarch64-unknown-none-softfloat`.
  `dt_blob.S` (dtc's output of `acpi.dts`, the template efiacpi fills from the ACPI tables
  when the firmware has no device tree) is built by a `const fn` in `dt_blob.rs`; a
  reference-backed test checks its bytes against `dt_blob.S`. efipxe uses libsa's network
  stack (`netif`, `ether`, `arp`, `netudp`, `tftp`, ported for it), which a program
  registers through `SaConf::netif_drivers` (and `SaConf::getsecs`).
- Link flags: the aarch64 `none` target is not static-pie, so `build.rs` adds `-pie
  --no-dynamic-linker -z notext` to amd64's (the precompiled `core`/`alloc` keep absolute
  addresses in read-only data, which become `R_AARCH64_RELATIVE` relocations that
  `self_reloc` applies).
- `softraid_arm64.c` is not ported (efiboot's feature `softraid`, as amd64's); its `sr`
  devsw entries say so. Which device tree the kernel gets is the C's choice: the firmware's
  (EDK2 on `virt,acpi=off`, what the smokes run) and, only without one or after `machine
  acpi`, efiacpi's. The efiacpi path is covered by host tests on synthetic tables; the
  arm64 kernel has no acpi(4) to use the tables it points at.

### The kernel's boot(8) entry (amd64, M14 track A2)

The same kernel ELF boots from Limine and from efiboot (the user's decision: Limine retires
only once boot(8) boots the kernel in QEMU).

- Link: amd64's `kernel.ld` is OpenBSD's layout (`conf/ld.script`): linked at
  `KERNTEXTOFF` (`0xffffffff81000000`), physical addresses from `0x1000000` (`AT()`), so
  efiboot's `paddr & 0xfffffff` move puts the image where `locore0.S` expects it; the ELF
  entry is `locore0.S`'s 32-bit `start`. Limine ignores the physical addresses and enters
  `_start` through its entry point request (`sys/stand/mod.rs`). `.got` is kept in `.data`:
  nothing may follow `end`, where `locore0.S` puts its tables. arm64's `kernel.ld` has its
  own layout (next section).
- `sys/arch/amd64/amd64/locore0.S` (`global_asm!`, AT&T, `const`/`sym` placeholders for
  `assym.h`): saves boot(8)'s arguments, copies the `bootarg` list into `bootinfo[]`, probes
  the CPU, builds the bootstrap page tables (the kernel at `KERNBASE`, the first 4 GB of the
  direct map at `PDIR_SLOT_DIRECT`, the recursive slot), enters long mode and calls
  `bootarg_main` in `sys/stand/bootarg.rs` on a 64 KiB `.bss` boot stack, where the C calls
  `init_x86_64` and `main`.
- The glue: `sys/stand/bootarg.rs` beside `limine.rs`, both ending in `stand::start_kernel`
  (boothowto, DUID, `early_init`, the banner, `main`). The facts come from a machine trait
  method, `Cpu::getbootinfo` (amd64's `machdep.c` `getbootinfo` with the boot(8) half of
  `init_x86_64`; arm64 and host return an error until track A3), so `stand` names no arch
  module. `BootInfo` gained `howto` and `duid`, boot(8)'s own `boothowto` and
  `BOOTARG_BOOTDUID`.
- Processors: under Limine its MP request; after boot(8) the MADT's enabled LAPIC entries
  (`cpu.rs` `mp_madt_cpus`, standing for acpimadt0, which is not ported) and the C's start:
  `map_tramps` copies `mptramp.S` to `MP_TRAMPOLINE`, `pmap_prealloc_lowmem_ptps` maps the
  low 2 MB in the kernel pmap, `mp_cpu_start` sends INIT and two STARTUP IPIs.
- Known limits: no Meltdown/SEV probe (`pg_g_kern` 0), no `pmap_direct_rand`, the direct map
  stops at 4 GB (pmap_bootstrap's extension is not ported; more memory is reported and left
  out), `dkcsum.c` is not ported (the root is found by the DUID, not by `bootdev`).

### The kernel's boot(8) entry (arm64, M14 track A3)

The same arm64 kernel ELF boots from Limine and from arm64's efiboot.

- Link: `conf/kernel.ld` keeps the image at `0xffffffff80000000` (Limine's higher half,
  which `arm64/pmap.rs` adopts) and gives it physical addresses from 0 (`AT()`): efiboot's
  `LOADADDR` keeps the low 39 bits of an address and adds its 64 MB block's
  (`efi_loadaddr`), which puts the image at the block's start. The ELF entry is
  `__start_phys`, the offset of `locore0.S`'s `_start` in the image, because `e_entry` goes
  through `LOADADDR` too; `_start` is first in `.text` (`.text.locore0`), so efiboot's
  cache clean from the entry to the end of the symbols covers the whole image. `locore0.S`
  zeroes `__bss_start`..`_end`, and `locore.S`'s `esym` starts at `end`. Limine ignores the
  physical addresses and enters the boot glue's `_start` through its entry point request
  (that function is not `#[no_mangle]`: the symbol `_start` is OpenBSD's).
- `sys/arch/arm64/arm64/locore0.S` (with the boot half of `locore.S`: `drop_to_el1`,
  `get_virt_delta`, `start_mmu`, the page tables, `initstack`): efiboot enters with the MMU
  on and an identity map, `x0` its end of the symbols, `x2` the device tree. `_start` drops
  to EL1, turns the MMU off and builds what the kernel otherwise gets from Limine: a
  four-level `TTBR1_EL1` (`T1SZ` 16) with the 64 MB block at the link address by 2 MB
  blocks and a direct map at `0xffff000000000000` (Limine's offset) by 1 GB blocks, and an
  identity map of the block in `TTBR0_EL1`; `MAIR_EL1` in this kernel's layout
  (`include/pte.rs`); then it jumps high, zeroes the BSS and calls `bootarg_main` on a
  64 KiB stack with the `arm64_bootparams` (`include/bootconfig.rs`).
- `Cpu::getbootinfo` (`machdep.rs`'s `getbootinfo`, the `/chosen` half of `initarm`) reads
  `bootargs`, `openbsd,boothowto`, `openbsd,bootduid`, `openbsd,bootmac`, the softraid boot
  volume and key, `openbsd,uefi-mmap-*`, `openbsd,uefi-system-table` and
  `openbsd,dma-constraint`, and the memory as `initarm` loads it (the EFI map's conventional
  and boot services memory less `/reserved-memory`'s `no-map` ranges and the 64 MB block,
  whose part after the symbols is usable). It maps those gigabytes in the direct map before
  handing them over; the device tree and the EFI map are read in place (loader data, never
  given to uvm).
- Processors: under Limine its MP request; after boot(8) the `/cpus` nodes and psci(4)'s
  `psci_cpu_on` (`dev/fdt/psci.rs`, `hvc` on QEMU) at `locore.S`'s `cpu_hatch_secondary` with the
  `cpu_info` as context: it brings the MMU up on the identity map and the kernel's
  `TTBR1_EL1` (`cpu.rs`'s `AP_TTBR1`, cleaned to memory first) and enters
  `cpu_hatch_entry` on the processor's kernel stack.
- ACPI (M14, the user's decision of 2026-10-08: the install media must boot an ACPI
  machine too): on `virt,acpi=on` EDK2 hands efiboot ACPI tables and no device tree, and
  efiboot's `efiacpi` builds one from them (the GIC and its MSI frame, the generic timer,
  PSCI, the CPUs from the MADT, the SPCR's UART as `/serial`, left disabled, and an
  `openbsd,acpi-5.0` node whose `reg` is the RSDP). The kernel attaches what that tree
  names as on `acpi=off`, then acpi0 at the ACPI node (`arm64/acpi_machdep.rs`'s
  `acpi_fdt` attachment, as `files.arm64` has it): `acpimcfg` maps the ECAM window
  (`pci_mcfg_init`, a `machine::pci_machdep` method that amd64 implements too),
  `acpiiort` reads the IORT, `acpipci` attaches the `PNP0A08` host bridge with bus spaces
  that translate its `_CRS` windows and MSI through the GICv2m frame (the requester ID
  mapped by the IORT), and `pluart` attaches at acpi0 (`ARMH0011`) and is the console when
  the SPCR names its address. Interrupts that ACPI describes go to the controller with
  phandle 1, which efiacpi gives the GIC. The PCI virtio disk then holds the root, found by
  its DUID. `just smoke-acpi` boots this way; the extents acpipci fills (`subr_extent.rs`)
  are not handed to the PCI bus yet (`pcivar.rs` has no extent members), so a BAR the
  firmware left unassigned would not be placed (EDK2 assigns them all).

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
- Limine instead of `boot(8)`/`efiboot`, until `boot(8)` boots the same kernel (M14): efiboot
  is ported (amd64's BOOTX64.EFI, "Boot loaders" above) and boots to the kernel's load; the
  kernel's own entry from it is M14 track A2.
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
- amd64 processor enumeration: since M13 `acpimadt0` attaches the processors from the MADT
  at mainbus, as OpenBSD does (`CPU_ROLE_BP` for the one whose local APIC is running, the
  others `CPU_ROLE_AP`, `mp_cpu_funcs` with `MULTIPROCESSOR`; a uniprocessor kernel's
  `cpu0` is then `apid 0 (boot processor)`, as `bsd.sp` prints it). The bootloader still
  starts the application processors (`mp_cpu_start` finds the MADT's APIC ID in Limine's
  list). Without a MADT (a kernel without ACPI, until `mpbios.c` is ported) the M11a
  stand-in remains: mainbus maps the local APIC at `LAPIC_BASE` and the `MULTIPROCESSOR`
  kernel attaches one `cpu` per processor the bootloader found (`BootInfo::mp`), the boot
  processor first as `CPU_ROLE_BP`, the others as `CPU_ROLE_AP` in the bootloader's order;
  one processor attaches as `CPU_ROLE_SP`. Without I/O APICs the application processors mask
  `LINT0` (QEMU wires the 8259's ExtINT to every local APIC); with them every CPU masks it, as
  the C does.
- amd64 interrupt routing (M13): `ioapic.c` drives the I/O APICs `acpimadt` attaches; every
  ISA interrupt goes through `mp_isa_bus` (the MADT's overrides, the rest identity-mapped),
  every PCI INTx through `mp_busses[bus]` (`acpiprt`), MSI and MSI-X straight to the local
  APIC (`msi_pic`, `msix_pic`, with the I/O APIC's edge stubs, as in C); the 8259 is left
  masked (nothing is established on it) and `LINT0`'s ExtINT is masked on every CPU.
  Routes are recorded while `ioapic_cold` and programmed by `ioapic_enable` at the end of
  `cpu_configure`, as in C, so no device interrupt arrives during autoconfiguration. All
  device interrupts still go to the boot processor (`apic_set_redir`'s destination, as in
  C). `acpimadt` and `acpiprt` are x86 code in `dev/acpi`: they reach the I/O APICs, the
  `mp_*` globals and the x86 attach arguments through the `machine::mpconfig` contract
  (`<machine/mpconfig.h>`; `struct mp_bus`/`struct mp_intr_map` are defined there because
  the generic drivers build them, the MP-spec and I/O APIC constants are associated
  constants), which arm64 and the host double implement as a machine without I/O APICs.
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
  as in OpenBSD (under QEMU it is measured, so `acpitimer_delay` and then `acpihpet_delay`
  take over when they attach, M13); arm64 uses `intr.c`'s `arm_dflt_delay` until `agtimer` attaches (M4).
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
  previous one, and every smoke run rejects that line (`--reject`). Since M13 `acpitimer0`
  and `acpihpet0` (amd64, under acpi0) are timecounters too and the TSC's reference
  (`kern.timecounter.choice=i8254(0) acpihpet0(1000) tsc(2000) acpitimer0(1000)`: since acpimadt
  attaches the CPUs from acpi0, the TSC registers between the HPET and the PM timer). arm64 attaches `agtimer` from the device tree (through mainbus
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
  `ld.so` through `elf_load_file` (M14: `userland` builds it, "Shared libraries and ld.so").
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
  whose drivers exist: `mainbus0 at root`, `cpu0 at mainbus?`, `bios0 at mainbus0`,
  `acpi0 at bios0` (M13) and `pci* at mainbus0` on amd64; `mainbus0 at
  root`, `ampintc* at fdt? early 1` and `agtimer* at fdt?` on arm64. The tables, `mainbus_cd`
  and `device_register` reach `subr_autoconf.rs` through `machine::autoconf`, so generic code
  never names an arch; the host double serves whatever table a test installs. A device that
  GENERIC configures but whose driver is not ported is reported with `unported!` where its bus
  would probe or attach it (amd64's `efi0`, `mpbios0`, ...); on arm64 every device-tree node
  and on amd64 every PCI function without a driver prints OpenBSD's `not configured` line. The counts
  `config(8)` writes into `<dev>.h` follow the tables: `NMPATH` is 0, the `hotplug(4)` calls
  are reported. Without ACPI or MP tables, amd64's mainbus attaches the boot CPU as
  `CPU_ROLE_SP`, as the C does on such a machine; `cpu_configure` keeps doing after
  `config_rootfound` what the boot processor's attach would add (`lapic_enable`,
  `lapic_calibrate_timer`) and mainbus maps the LAPIC at its architectural base (with ACPI,
  `acpimadt` does both from the MADT, M13). Adding a driver means its
  `cfattach`/`cfdriver` and one `Cfdata` row in each `ioconf.rs` that has it in GENERIC.
- ACPI (M13, amd64): `acpi0 at bios0 at mainbus0`, as in GENERIC. bios0 (`bios.c`) gets the
  RSDP from Limine's RSDP request (`BootInfo::rsdp`, kept by `init_x86_64` as
  `BIOS_EFIINFO_CONFIG_ACPI`, the C's `bios_efiinfo->config_acpi`), so `acpi_probe` finds it
  as on an EFI boot; bios.c's SMBIOS half is not ported yet. acpi0 copies the tables, loads
  the DSDT and the SSDTs into the AML interpreter at boot, and owns power: `boot(RB_HALT |
  RB_POWERDOWN)` enters S5 (`acpi_powerdown`), `cpu_reset` tries `cpuresetfn` (`acpi_reset`,
  the FADT's reset register) before the keyboard controller and the triple fault. acpi0's
  children: `acpimadt0` (the MADT: processors, `ioapic*` at mainbus, the ISA overrides and
  the local APIC NMIs, `mp_busses`), `acpiprt*` (each PCI bus's `_PRT`: INTx pins on the I/O
  APIC, link devices through `_CRS`/`_PRS`/`_SRS`) and `acpipci*` (the host bridges:
  `acpi_haspci`, `_OSC`, MSI enabled for the bus mainbus then attaches) are ported (M13; see
  "amd64 interrupt routing"); `acpitimer`, `acpihpet` and the rest print OpenBSD's `not
  configured` line. The SCI is on its ISA line, which `isa_intr_establish` maps to the I/O
  APIC pin through `mp_isa_bus`. arm64 attaches no acpi0 until M14 (EFI ACPI boot, `efiacpi.c`); its
  `machine::acpi_machdep` answers as a machine without ACPI. The S3/hibernate machinery
  (`acpi_x86.c`, `acpi_wakecode.S`, `subr_suspend.c`) is not M13's and is reported where
  reached. `just smoke-power` checks `halt -p` (QEMU powers off, status 0) and `reboot` (QEMU
  without `-no-reboot`, xtask `--reboot`, boots a second time), on both archs: arm64 goes through
  psci(4) (`dev/fdt/psci.c`, `psci0 at mainbus0`: SYSTEM_OFF and SYSTEM_RESET as `powerdownfn` and
  `cpuresetfn`; `hvc_call`/`smc_call` are `asm!` in `arm64/cpufunc.rs`, reached through
  `machine::fdt`). `just smoke-rtc` compares `date +%s` after boot with the host within 60 s
  (amd64: mc146818 `rtcinit`; arm64: efi0 GetTime, EDK2 disables the pl031 node).
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
  (the TESTS zone of `kern/vfs_subr.rs`), a fixed in-memory tree with a real lock discipline, to drive
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
  build FFS1 and FFS2 images with a `newfs`-like helper (the TESTS zone of `ffs_vfsops.rs`), mount them
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
  Since M13 `acpimadt` installs `mp_busses` and `acpiprt` fills them, so INTx maps to I/O
  APIC pins (`apic 0 int N`), and `acpipci` enables MSI on the bus, so `pci_intr_map_msi*`
  work (`msi`, `msix`); a kernel without ACPI still maps the line register to the 8259 and
  refuses MSI, exactly as the C does without tables. `option PCIVERBOSE` is not configured: the 800 KB name tables
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
  DMA map and programming the function with `arm64/pci_machdep.c`. On `virt,gic-version=3`
  (M16f, `EMIBSD_GIC=3`, `smoke-gicv3`) the GIC is agintc(4) and the pcie node's `msi-map`
  (ACPI: the IORT) names its ITS (`agintcmsi`), which translates each MSI into an LPI. The
  ITS's tables and the LPI tables are uncached DMA memory, so `pmap_kenter_cache` cleans
  the caches of those managed pages first (`cpu_idcache_wbinv_range`), as in C. agintc's
  attach puts back the boot CPU's interrupt mask as it found it (ampintc's unmasks), so the
  `qemu` self-tests that wait for an interrupt before the first context switch
  (`selftest=uart`, `selftest=clock`) unmask it themselves through `machine::cpu::intr_enable`
  (`intr_enable()`, new in the machine contract). The extents are absent
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
  for virtio is `machine::pci_machdep::PCI_MSI_PER_BRIDGE`. Interrupts: since M13 amd64's
  virtio devices take an MSI-X vector per queue (`msix per-VQ`); arm64's come from the node
  through `ampintc`.
- USB (M12): the machine-independent core (`dev/usb/usb.c`, `usbdi.c`, `usb_subr.c`, ...)
  runs under the kernel lock at `splusb()`, as in OpenBSD. xhci(4) (`dev/usb/xhci.c`,
  `dev/pci/xhci_pci.c`) attaches at PCI on both archs (INTx on amd64 QEMU, MSI-X through the
  GICv2m frame on arm64). Its interrupt is `IPL_MPSAFE` as in OpenBSD: the handler only
  reads the status registers and schedules the USB soft interrupt, so the `usbd_bus` members
  it touches (`use_polling`, `dying`, `no_intrs`) are relaxed atomics. The TRB rings,
  contexts and tables live in DMA memory and are reached only through bounds-checked
  volatile accessors (`XhciTrbRef`, the `*_ctx_update` closures). uhub(4) drives both the
  emulated root hub and external hubs. umass(4) (`dev/usb/umass.c`, `umass_scsi.c`,
  `umass_quirks.c`) attaches at `uhub?` and hangs a `scsibus` below it (`umass` is a parent of
  the `scsi` attribute, as vioblk and softraid are), so a USB stick is an `sd(4)` disk. The
  SCSI probe's commands are polled (`umass_polled_transfer` turns the state machine's
  recursion into iteration); later I/O completes from the USB soft interrupt.
  uhidev(4) and ukbd(4) attach for USB keyboards over the HID parser (`dev/hid/hid.c`) and
  hidkbd; the keyboard's interrupt pipe opens only when a `wskbd` child enables it (M13), so
  until then the keyboard attaches but is silent, as on an OpenBSD kernel without `wskbd`.
  ehci(4) (M16b, `dev/usb/ehci.c`, `dev/pci/ehci_pci.c`) is the second host controller:
  `usb*` takes both `xhci` and `ehci` as parents (one cfdata entry, as config(8) merges its
  lines). Its soft QHs, qTDs and iTDs/siTDs are carved from DMA chunks that are never freed,
  so they are `&'static`, the hardware descriptor first as `#[repr(C)]` `Cell<u32>`s read and
  written volatile (`ehci_get`/`ehci_set`, the siop idiom). In QEMU two behaviours stop the
  faithful port short of a mounted stick, left to the user: on amd64 `ehci_init` raises INTx
  while the I/O APIC pin is still masked and edge-triggered (`ioapic_addroute` leaves pins to
  `ioapic_enable` while cold, as in C) and QEMU's I/O APIC drops masked edges, so the level
  that never falls is never delivered; on arm64 `ehci_pci_attach`'s 16-bit
  `EOWRITE2(EHCI_USBINTR, 0)` is refused by QEMU's EHCI (4-byte operational registers) as a
  synchronous external abort.
  uhci(4) (M16b, `dev/usb/uhci.c`, `dev/pci/uhci_pci.c`) is the third: its soft TDs and QHs
  are carved from DMA chunks the same way; a TD's `link` (a C union of a QH and a TD pointer)
  is an enum, so a walk that meets a QH where it expects a TD stops instead of
  reinterpreting it. On arm64 `virt` EDK2 leaves its I/O BAR at 0; as in OpenBSD, the PCI
  bus now carries the host bridge's extents (`pciecam`, arm64 `acpipci`) and
  `pci_mapreg_assign` places such a BAR from them (amd64's buses have no extents yet).
- Audio (M12): audio(4) (`dev/audio.c`) is machine-independent; drivers reach it only
  through `AudioHwIf`, `audio_attach_mi` and `audio_pintr`/`audio_rintr`, called with
  `AUDIO_LOCK` held. azalia(4) attaches QEMU's `intel-hda` on both architectures (through
  pciecam on arm64) and auich(4) with ac97(4) its `AC97` on amd64 (GENERIC has auich on
  amd64 only). QEMU's HD Audio controller stops fetching commands while a RIRB interrupt is
  unacknowledged, and the handler cannot run during autoconf, so `azalia_get_response` does
  the handler's RIRB work itself when the flag is up (a deviation, harmless on hardware).
  The generic mixer of `azalia_codec.c` is ported: mixerctl(1) lists the codec's
  amplifiers, selectors and pins (QEMU's hda-output: `inputs.dac-0:1`, `outputs.master`)
  and sets them.
  Userland plays with aucat(1), which falls back to `/dev/audio0` (`rsnd/0`) when no
  sndiod(8) runs, as `sio_open(3)` does; sndiod itself is not needed for the smokes.
- QEMU's disks (M10a, `boot.rs`, `qemu_command`): besides the boot image every VM has one
  persistent virtio-blk disk, the raw 64 MiB sparse file `target/disk-<arch>.img`
  (`disk-<arch>-a.img` / `-b.img` for `smoke2`'s two VMs; in `target/smoke/<recipe>/` instead
  of `target/` when `just smoke` runs the recipe, see "Parallel smokes"), created zero-filled when missing
  and reused as is, so what a guest wrote survives the next boot; `--disk-fresh` (`qemu`,
  `smoke`, `smoke2`) recreates it. It is added after every NIC: on amd64 it is
  `virtio-blk-pci` on a later PCI slot (the NIC stays `virtio0`/`vio0`, dev 2; the boot
  image is on port 0 of q35's AHCI controller, `ahci0` at dev 31 since M13: PCI is probed by
  device number, so the virtio-blk disks come first and the boot image is the `sd` unit
  after them, `sd1` with the one disk, its `scsibus` the one after theirs; softraid's
  `scsibus` and volumes follow); on arm64 `virt` it takes the lowest
  virtio-mmio slot in use (slots go out top down, the kernel attaches bottom up), so it is
  the first block device found (`sd0`) and the boot disk (`virtio31`) the second (`sd1`).
  `--nvme FILE` (M13a, `qemu` and `smoke`; `hwopts.rs`, the home of M13's QEMU device
  options) adds an NVMe controller whose namespace is FILE. On amd64 it is added right after
  the NICs, so it takes slot 3, attaches before the virtio-blk disk (then at slot 4) and its
  namespace is `sd0`. On arm64 (M13) it is the first device on `virt`'s PCI bus (`pci0 dev
  1`; its NIC and disks are virtio-mmio): `pciecam` comes after the `virtio_mmio` nodes in
  QEMU's device tree, so the virtio disks attach first and the namespace is `sd2` (after the
  persistent disk `sd0` and the boot image `sd1`).
- QEMU's M13 devices (`tools/xtask/src/hwopts.rs`, one option each, hooked into
  `boot::qemu_command` by one call): `--scsi-cd ISO` (`qemu`, `smoke`, `smoke2`) adds a virtio
  SCSI adapter with a read-only `scsi-cd` drive holding the file (`virtio-scsi-pci` on amd64,
  `virtio-scsi-device` on arm64) as the LAST device of the command line, so the numbering of
  the other virtio devices does not change: amd64's PCI slots go up (`vioscsi0 at virtio2`,
  after the NIC and the disk), and on arm64 the adapter takes the lowest virtio-mmio slot, so
  the kernel finds it first (`vioscsi0`, and its `scsibus0`) while the NIC and the disks keep
  the slots, hence the names, they have without it. `smoke-cd` mounts the ramdisk's makefs ISO
  through it (`cd0`, `mount_cd9660 /dev/cd0c`). `--ahci FILE` (`qemu`, `smoke`)
  puts FILE on port 1 of q35's built-in AHCI controller (`ide-hd` on `ide.1`): `sd2` after the
  virtio-blk disk (`sd0`) and the boot image (`sd1`, port 0). `virt` has no AHCI controller
  of its own, so on arm64 (M13) it adds an `ich9-ahci` as the first device on the PCI bus
  (`pci0 dev 1`) with FILE on port 0 (`ahci0.0`): `sd2` there too, after the two virtio-mmio
  disks. `smoke-ahci` boots from it, on both archs, a
  disk `nvme-root --root-dev sd2a` writes (its fstab names the unit: diskmap(4) is not
  ported, so fstab cannot name the DUID).
  `--lsi FILE` (`qemu`, `smoke`; amd64 only:
  arm64's GENERIC has no siop) adds QEMU's `lsi53c895a` after that, with a `scsi-hd` at
  target 0 on FILE (in the run directory, made afresh, zeroed, 64 MiB, each run) and, with
  `--lsi-cd ISO`, a read-only `scsi-cd` at target 1: being last, it takes the next PCI slot
  (`siop0 at pci0 dev 4` beside the NIC and the disk) and its `scsibus` attaches after
  vioblk's and before ahci's (dev 31), so its disk is `sd1` (`smoke-siop`).
  `--pci-serial FILE` (`qemu`, `smoke`) adds QEMU's `pci-serial` (1b36:0002) last, its
  line a file chardev in the run directory, and `--expect-pci-serial TEXT` checks the file once
  the serial expectations passed (`smoke-puc`, amd64 only: arm64's GENERIC has no `puc*`; the
  guest's `echo ... >/dev/cua04` reaches the host). For a future arm64 `puc*`: the card's
  BAR is I/O space that EDK2 leaves unassigned and the kernel cannot place without extents,
  and `com` shares `cdevsw` major 8 with the PL011 console (`pluartcnattach`'s KLUDGE).
  `--nic MODEL` (`qemu`, `smoke`; `e1000`, `e1000e` or `igb`) makes the NIC on the user
  network an Intel PRO/1000 of that model, for em(4), in vio0's place on the command line
  and on its netdev: no other device moves, em0 is the only Ethernet interface and the
  kernel's network self-test configures it as it does vio0 (on arm64 it sits on `virt`'s
  PCIe bus). Not with `--vio-mq`. `smoke-em` pings QEMU's gateway through the 82574L on both
  archs and the 82540EM on amd64; QEMU's 82576 (`igb`) attaches and links up but passes no
  traffic, probably because its model writes back only advanced receive descriptors and
  em(4) programs legacy ones, as OpenBSD's does (not checked against QEMU's source).
  `--nic rtl8139` is QEMU's Realtek 8139C+ (PCI 10ec:8139 revision 0x20), which re(4)
  drives; `smoke-re` pings the gateway through it on both archs, on INTx. `--nic vmxnet3`
  is QEMU's VMware VMXNET3 (PCI 15ad:07b0), which vmx(4) drives; `smoke-vmx` pings the
  gateway through it on both archs, with 4 queues on MSI-X.
- `em(4)` (M13): `dev/pci/if_em.c` is OpenBSD's whole driver over the shared code of
  `if_em_hw.c`. The C changes `sc->hw` only under the kernel lock and reads `mac_type` and
  the registers unlocked from its MP-safe paths (the send queue, the interrupt's ring work);
  here `sc->hw` is handed out as a guard that asserts the kernel lock and refuses a second
  borrow, and the unlocked paths use a second, read-only `struct em_hw` holding the register
  handles and `mac_type` (C_TO_RUST.md). `kstat(4)` is compiled out (`NKSTAT` 0) and
  `vlan(4)` is not configured (`NVLAN` 0). MSI-X stays off as in C (`em_enable_msix`): the
  82574L and 82576 run on MSI, the 82540EM on INTx.
- `ifmedia`, `mii(4)` and `re(4)` (M13): `net/if_media.c` keeps a driver's media list
  (`struct ifmedia`, now in em, vio and re) and answers `SIOCGIFMEDIA`/`SIOCSIFMEDIA`.
  `dev/mii/` is the MII layer: `mii_attach` probes the PHY addresses through the driver's
  `mii_readreg` and attaches the PHY drivers at the `mii` attribute (locator `phy`;
  `rlphy`, `rgephy`, `ukphy` are ported). re(4) (`dev/ic/re.c` with `dev/pci/if_re_pci.c`)
  drives the Realtek 8139C+/8169/8168 family; QEMU's `rtl8139` is an 8139C+ at revision
  0x20, which `re_pci_probe` takes (`rl_pci_match` takes the RT8139 only at revision 0x10,
  so rl(4) stays deferred). As in em(4), the descriptors are read and written whole and
  volatile, and what the MP-safe send queue and interrupt share (`rl_flags`, the transmit
  producer and consumer) are atomics. One deviation fixes a race QEMU exposes: `re_init`
  acknowledges `RL_ISR` before it starts the simulated-moderation timer, which can expire
  into the C's later acknowledgement and leave the timer-only interrupt mask silent.
  `__STRICT_ALIGNMENT` became `MachineParam::STRICT_ALIGNMENT` (arm64 true), for
  `RE_ETHER_ALIGN`.
- `vmx(4)` (M13): `dev/pci/if_vmx.c` and `if_vmxreg.h` whole, for QEMU's `vmxnet3`. As in
  em(4) and re(4), the descriptors are read and written whole and volatile, the shared
  structures member by member, and the transmit ring's producer and consumer (the send
  queue's and the interrupt's) are atomics; each receive ring's fill state stays under its
  mutex. QEMU offers 25 MSI-X vectors: vector 0 takes the device's events and intrmap(9)
  gives each queue its own vector on its own CPU, 4 queues with `-smp 4` on both archs
  (transmit spreads over them by flow id; QEMU receives on queue 0 only). `kstat(4)` is
  compiled out (`NKSTAT` 0), `vlan(4)` is not configured (`NVLAN` 0). Its `init` calls
  `intr_barrier` for each queue vector, which waits for that vector's CPU to go through the
  scheduler; so the kernel's `qemu` network self-test (`selftest::ping_gateway`, which
  configures the first Ethernet interface) runs after `cpu_boot_secondary_processors`, as
  OpenBSD's netstart(8) runs with every processor up.
- `disklabel(8)` and `fdisk(8)` embed their manual page in a generated `manual.c` rendered
  with mandoc(1); the userland build takes their Makefiles' own `.ifdef NOMAN` branch
  (`NOMAN_PROGRAMS` in `tools/xtask/src/userland.rs`), so the embedded page reads
  `no manual`. Only that text differs.
- `vio(4)` (M7b): `dev/pv/if_vio.c` is OpenBSD's whole driver (`vio* at virtio?`), attached
  with the NIC API of the network-interface layer (`if_attach`, `ether_ifattach`, one send
  and one receive queue). QEMU's user-mode network gives it no offloads (slirp has no
  virtio-net header), so it runs with `MRG_RXBUF`, event indexes, indirect descriptors and
  the control queue. Multi-queue (`VIRTIO_NET_F_MQ`, asked for with more than one CPU and
  four MSI-X vectors) goes through `intrmap(9)` since M13, as in C; QEMU's user network
  offers it only with `mq=on` (xtask `--vio-mq`) and with one queue pair, so the default
  boots keep virtio_pci's per-queue vectors. Not ported and reported where called:
  `ifmedia` (`net/if_media.c`: only the five media words
  of `<net/if_media.h>` it reports are here), `tcpstat`. The interrupts work on both
  machines: amd64's through MSI-X since M13 (INTx through the 8259 before),
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
  layout to the byte; the TESTS zone of `net/pfvar.rs` checks their sizes and offsets against clang's.

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
  `tsc_timecounter_init` takes `identifycpu`'s `cpu_freq` and recalibrates it as OpenBSD
  does (M13): `acpitimer0` and `acpihpet0` attach under acpi0 at bios0, before mainbus
  attaches cpu0, and each calls `cpu_recalibrate_tsc`, so `delay(9)` is `acpihpet_delay` when
  `cpu_freq` counts the TSC over 100 ms and `measure_tsc_freq` measures it against
  acpihpet0 (three 100 ms rounds with interrupts off, at least two within 50 us). The quality
  follows the C: 2000 after a good measurement, -1000 if every round was disturbed (a busy
  host descheduling the vCPU), in which case acpihpet0 (1000) is the timecounter. The M11
  stand-in (quality 2000 for a TSC no reference had recalibrated, measured against the
  i8254, which under load read 1.2-1.3 GHz for ~1.0 and ran the clock 20-30 % slow) is gone;
  the invariant-TSC flags above are what remains of this deviation. Under `qemu` only,
  `calibrate_tsc_freq` prints its result (`tsc: calibrated against acpihpet0: 1000000000
  Hz`), beside `machdep.tscfreq` (`cpu_sysctl`, ported in M13); `smoke-clock`
  checks that `date` keeps the host's rate across `sleep 45`, within 2 s, on both archs.
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
- Frame buffer under Limine (M13, the video console's first step). OpenBSD's loaders hand
  the kernel the UEFI GOP frame buffer: amd64 boot(8)/efiboot in `bios_efiinfo`'s `fb_*`
  fields (`BOOTARG_EFIINFO`), arm64 efiboot as a `simple-framebuffer` node `framebuffer`
  under `/chosen` (`efi_framebuffer()`). Under Limine the kernel asks for Limine's
  framebuffer response, which the boot glue turns into `BootInfo::framebuffer` (physical
  address, geometry, channel sizes and shifts). On amd64 `init_x86_64` fills
  `machdep.rs`'s `bios_efiinfo()` from it as efiboot fills `bios_efiinfo` from the GOP mode
  (the reserved mask is the rest of the pixel), so `efifb(4)` is ported unchanged; its
  early map is the bootloader's direct map, which covers the frame buffer
  (`pmap_set_pml4_early` is not needed). On arm64 the glue builds a copy of the device tree
  with the node efiboot would add (`sys/stand/fdtfb.rs`: same properties, same skip rules,
  in a static buffer instead of efiboot's in-place edit), so `simplefb` and
  `mainbus_attach_framebuffer` are ported unchanged; QEMU's `virt` tree, as EDK2 passes it
  on, has no frame buffer node of its own (`ramfb` is set up through fw_cfg). The display
  QEMU gives the GOP: q35's standard VGA on amd64 (present even with `-display none`;
  1280x800 under OVMF), `-device ramfb` on arm64 (800x600 under ArmVirtQemu; a
  `virtio-gpu` GOP is blit-only and Limine needs a linear frame buffer). `xtask`'s `--fb`
  adds it, `--screenshot-after` checks a QEMU `screendump` against the kernel's
  `selftest=fb` (`smoke-fb`). Under feature `qemu` the drivers also hand their
  `wsdisplay` attach arguments to that self-test.
- The frame buffer is a plain display, the serial line the console (M13, wsdisplay). OpenBSD
  picks the console in `cninit` (amd64: `constab[]` with `wscnprobe` at `CN_MIDPRI` and
  `comcnprobe` at `CN_HIGHPRI` when boot(8) says `set tty com0`) or in arm64's `consinit`
  (`simplefb_init_cons` takes the frame buffer only when `/chosen`'s `stdout-path` names it).
  With a serial console, OpenBSD attaches `wsdisplay0 at efifb0 mux 1` (or `at simplefb0`)
  without `: console`, makes its six screens (`ttyC0`..`ttyC5`), and the kernel's messages
  do not appear on it. EmiBSD's console is always the serial line, which every smoke reads,
  so it behaves as OpenBSD with a serial console: amd64's `consinit` attaches com0 directly
  (`cninit`, `constab[]` and `wscons_machdep.c` are not ported, so `efifb_cnattach` is never
  called), arm64's calls `simplefb_init_cons` after `pluart_init_cons` as the C does, and it
  returns at once because QEMU's `stdout-path` names the PL011. `wsdisplay_cnattach` and the
  console paths of efifb/simplefb are ported and wait for a frame buffer console. A boot
  without `--fb` has no frame buffer, so nothing attaches and nothing changes; with `--fb`
  `just smoke-wscons` writes to `/dev/ttyC0` and checks the screendump (`--screen-text`,
  with the grid `selftest=wscons` prints).
- vga(4) under UEFI (M13). `vga0 at isa?`, `vga* at pci?` and `wsdisplay0 at vga? console 1`
  are in amd64's ioconf, as in GENERIC (arm64's GENERIC has no vga). On the smokes' q35 with
  OVMF the std VGA is in the graphics mode OVMF set for the GOP: its I/O and memory decoding
  are on (command register 0x7), but the legacy text memory at 0xb8000 reads back 0xffff, so
  `vga_common_probe` fails at both buses and the device stays "not configured", which is what
  an OpenBSD 8.0 snapshot prints on the same machine (`just diff-openbsd`'s VM: `"Bochs VGA"
  rev 0x02 at pci0 dev 1 function 0 not configured`, no vga line, `efifb0` takes the
  display); `smoke-vga` checks it. Text mode would need a BIOS boot (SeaBIOS), which EmiBSD
  does not have. The probe maps the ISA hole: Limine's direct map (base revision 3) covers
  only its memory map's regions and leaves the VGA window out, so amd64's `bus_space_map`
  takes the hole shortcut (`atdevbase`, which `locore0.S` maps after a boot by boot(8)) only
  for pages that are mapped there and maps the others like any device memory. Without the
  `ioport_ex` extent a second probe of claimed ports does not fail: where a PCI VGA does
  attach, `vga0 at isa?` would attach too (OpenBSD's ISA probe fails there).
- smmu(4) on an SMMUv3 without EL2 support (`SMMU_IDR0.Hyp` clear, QEMU's
  `virt,iommu=smmuv3`) uses the non-secure EL1 regime: `STRW` NS-EL1, no `CR2.E2H`, and
  `TLBI_NH_ASID`/`TLBI_NH_VA` instead of `TLBI_EL2_*`, the forms `smmu.c` keeps commented
  out; with `Hyp` set the C's EL2 forms stay (M16f; `v3.sc_has_hyp` is not in the C). QEMU
  ignores the EL2 invalidations, so a map's IOTLB entries outlived its unload and the next
  load of the same IOVA read the old pages: the NVMe root's disklabel came back wrong.

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

How many processors the VMs get (the user's decision of 2026-10-07, replacing the `-smp 4`
of every recipe decided on 2026-10-03 for M11):

- Every recipe boots the MULTIPROCESSOR kernel with the justfile's `smp`, `-smp 2` by
  default. `ncpu` comes from `EMIBSD_NCPU` (2 or 4; anything else stops `just` with an
  error) and is exported, because `smoke-all` runs each recipe as a new `just --no-deps`
  process: an environment variable reaches it, a `just ncpu=4` override on the command line
  alone would not (the export carries it). Expectations that name the count follow `ncpu`:
  `aps` (the application processors, also the last CPU's number) in `smoke-efiboot`,
  `smoke-acpi` and `smoke-ddbmp`, whose `machine ddbcpu` walk and `cpuinfo` lines depend on
  it. The install recipes (`smoke-install-*`) follow `smp` too.
- The `smp4` group stays on `-smp 4` whatever `ncpu` says, to stress MP: `smoke-mp`
  (mpstress, the kthread ping-pong, the vio multiqueue boot), `smoke-vmx` and `smoke-net-mp`
  (multi-queue and softnet networking; the latter's `-smp 8` boot stays) and
  `smoke-softraid` (the disk smoke under load). `diff-openbsd` also keeps four processors,
  the configuration its expected differences were recorded with. `smoke-up` stays the one
  uniprocessor boot.
- `just ci-full` runs `EMIBSD_NCPU=4 just ci`: every recipe on four processors, as before
  2026-10-07. Then, still on four, the installer end to end on both archs (the user's
  decision of 2026-10-07): `smoke-install-<arch>` and `smoke-install-boot-<arch>` for amd64,
  arm64 and arm64 on ACPI, which make the install media from the tree (so `just comp` must
  have run), so every milestone close regenerates and checks the installer. Mandatory before
  a milestone is marked met; its result goes in the closing commit.
- Why two is enough by default: every MP bug found until then needs only two CPUs (one
  CPU sleeping in vio_ctrl_submit while the interrupt lands on another; one SMR reader and
  one freer in art, rtable, bpf and pfsync; soreceive's sender and receiver; the consfile tty
  race; QEMU's lost `sev` on arm64 with any MTTCG of two or more vCPUs; vmx's intr_barrier on
  an AP that is not running yet, which intrmap reaches with `-smp 2` too). Fewer CPUs make
  them rarer, not impossible, hence `ci-full` before every milestone close.

### diff-openbsd

`just diff-openbsd` (`cargo xtask diff-openbsd`, `tools/xtask/src/diffopenbsd.rs`, M12+) runs
the same scenarios on EmiBSD and on a real OpenBSD and compares them step by step. The smokes
check that EmiBSD does what we expect; this checks that it does what OpenBSD does.

- **The OpenBSD VM** (the user's decision of 2026-10-05): the official -current snapshot
  nearest the pin, amd64 and arm64. The mirrors keep only the latest snapshot; the one used
  was built a day (amd64, 2026-10-03) and two days (arm64, 2026-10-04) after the pin.
  `tools/xtask/openbsd-snapshot.toml` records the mirror, the build dates and two SHA256s per
  arch: the snapshot's own `SHA256` file, and `install80.img` as that file lists it. `fetch`
  downloads the image once into `target/openbsd/<arch>/` and refuses it unless both match.
  Once the mirror moves on, a fresh clone cannot fetch it: copy `target/openbsd/` from a
  machine that has it, or record a new snapshot (the user's decision).
- **Install**: autoinstall(8), headless. The image boots under EDK2 (on amd64 `set tty com0`
  is typed at efiboot's `boot>`); the installer is told `A` and given
  `http://10.0.2.2:<port>/install.conf` (`tools/xtask/diff-openbsd/install.conf`), served by
  a small HTTP/1.0 file server of xtask (`diffopenbsd/http.rs`) with a disklabel template
  (`/` and swap). Sets: `bsd*`, `base`, `comp`. The image's set directory has no
  `SHA256.sig`, so the installer is told to go on without it: the whole image was checked on
  the host. The installer gets two processors (it offers `bsd.mp` only on a multiprocessor).
  A first boot, without network (`restrict=on`, so rc.firsttime's fw_update and syspatch
  reach nothing), sets `library_aslr=NO` and removes `/usr/share/relink/kernel`, so later
  boots do not relink, and halts. The disk is kept with an `installed` marker.
- **A run**: EmiBSD (the smokes' MP kernel and ramdisk, `--smp 4`) and OpenBSD (the installed
  disk under `-snapshot`, never changed) boot side by side, each with a blank 64 MiB scratch
  disk (`sd0` on EmiBSD, `sd1` on OpenBSD; `$D` in the scripts). Both log in as root on the
  serial console and fetch each set's script and the test program with ftp(1) from the HTTP
  server. Each script runs every step between `@@B <k>` and `@@E <k> <status>` markers.
- **Scenarios** (`tools/xtask/diff-openbsd/*.scn`, format in `diffopenbsd/scenario.rs`): one
  shell line per step, `$` compares output and status, `?` the status, `!` nothing. `setup`
  partitions, labels, formats and mounts the scratch disk; `syscalls` runs `tools/difftest`
  (ISC, our own: 291 probes printing return values and errno names, inode numbers as labels)
  one section per step; `fs` uses OpenBSD's own utilities, built into the ramdisk for
  it (cp, ln, mv, rm, rmdir, readlink, stat, touch, wc). The same static difftest binary runs
  on both systems.
- **Normalized**: host names, the scratch disk's name, `prog[pid]`, tabs (expanded on both),
  and, where a step asks, ls dates, inode numbers, or all numbers.
- **Expected differences** (`tools/xtask/diff-openbsd/expected.toml`): each with the step id,
  its command and a reason. Any other difference fails, and so does an entry that no longer
  differs. An entry covers a whole step, so a step that hits an unported part stays alone.
- **Environment, not kernel**: the two root file systems differ (the ramdisk has no
  `/usr/mdec/mbr`, its own `/etc/fstab`, `/etc/group` and gettytab, and no root `.profile`
  running tset(1)). The scenarios avoid or normalize those, and each place says why.

First results (2026-10-05): one stale stub fixed (`amap_copy` chunking, `uvm_amap.c` now
ported); five expected differences on both archs: fifofs, `exec_script.c`, file `mmap`, core
dumps (all visible EmiBSD stubs) and `kern.ostype` (branding). Since M14 `exec_script.c` and
file `mmap` are ported, so their two entries are gone: three expected differences remain
(fifofs, core dumps, branding).

Timings on the M-series Mac with the other milestone agents running: download about 1 minute
for both images (1.5 GB); install and first boot 272 s (amd64) and 426 s (arm64), once. A run
after that: about 35 to 65 s for amd64 and 80 s for arm64, image build included. It stays
**beside `just ci`**, a documented recipe, not part of it: the first run needs the network and
a snapshot that the mirrors drop within days, so `ci` could not stay reproducible on a fresh
clone; and it adds no build step `ci` does not already do. `testing.md` says when to run it.
