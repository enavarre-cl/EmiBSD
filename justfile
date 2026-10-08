# EmiBSD task runner. `just` lists recipes. Never call qemu or `cargo --target` by hand.
set shell := ["zsh", "-cu"]

# What `newvers.sh` reads, made reproducible: `sys/build.rs` builds the kernel's `version`
# string (`sys/conf/vers.rs`) from these. The build number is the commit count, the date the
# last commit's; both come from the local repository, so the same commit gives the same kernel.
export EMIBSD_BUILD := `git rev-list --count HEAD 2>/dev/null || echo 0`
export SOURCE_DATE_EPOCH := `git log -1 --format=%ct 2>/dev/null || echo 0`
export EMIBSD_BUILD_HOST := `hostname -s 2>/dev/null || echo localhost`

amd64 := "x86_64-unknown-none"
arm64 := "aarch64-unknown-none-softfloat"

# Every smoke and smoke2 run fails if a VM prints this (kern_clockintr's check under feature
# `qemu`: an uptime reading behind the previous one).
reject := "--reject 'uptime went backwards'"

# Since M11e every smoke and smoke2 run boots the MULTIPROCESSOR kernel (MP is the configuration
# that matters, as GENERIC.MP is in OpenBSD). Since 2026-10-07 (the user's decision, replacing
# the four processors of 2026-10-03) they boot it on `ncpu` processors, two by default: every
# MP bug found so far needs only two CPUs. `EMIBSD_NCPU=4` (or `just ncpu=4 <recipe>`; it is
# exported, so the recipes `smoke-all` runs see it too) puts every `{{smp}}` recipe on four;
# `ci-full` does that for the whole `ci`, before a milestone is met. Expectations that name
# the count follow `ncpu`: `aps`, the application processors, is also the last CPU's number.
# `smp4` is the fixed group that stays on four to stress MP whatever `ncpu` says: `smoke-mp`
# (mpstress, the kthread ping-pong), the multi-queue network smokes (`smoke-vmx`,
# `smoke-net-mp`) and `smoke-softraid` (the disk smoke under load); their expectations keep
# their literal counts. `smoke-up` keeps one minimal uniprocessor boot per arch.
ncpu := env("EMIBSD_NCPU", "2")
export EMIBSD_NCPU := ncpu
smp := if ncpu == "2" { "--smp 2" } else if ncpu == "4" { "--smp 4" } else { \
    error("EMIBSD_NCPU (ncpu) must be 2 or 4, not '" + ncpu + "'") }
smp4 := "--smp 4"
aps := if ncpu == "4" { "3" } else { "1" }

# M16f: the interrupt controller of arm64's `virt`. `gic` 2 (the default, QEMU's) is the GICv2
# with its GICv2m MSI frame, ampintc(4) and ampintcmsi; `EMIBSD_GIC=3` (or `just gic=3
# <recipe>`; exported, so `smoke-all`'s recipes and xtask's `--gic` default see it) boots
# every arm64 smoke on `virt,gic-version=3`: agintc(4) and its ITS, agintcmsi. The
# expectations that name the GIC follow it: `gic_attach` (the GIC's attach line on the device
# tree), `gic_acpi_attach` and `gic_msi_attach` (`smoke-acpi`'s GIC and MSI controller).
# `smoke-gicv3` boots GICv3 whatever `gic` says.
gic := env("EMIBSD_GIC", "2")
export EMIBSD_GIC := gic
gic_attach := if gic == "2" { "ampintc0 at mainbus0 nirq " } else if gic == "3" { \
    "agintc0 at mainbus0 shift " } else { error("EMIBSD_GIC (gic) must be 2 or 3, not '" + gic + "'") }
gic_acpi_attach := if gic == "3" { "agintc0 at mainbus0 shift 4:4 nirq 288 nredist " + ncpu } else { \
    "ampintc0 at mainbus0 nirq 288, ncpu " + ncpu }
gic_msi_attach := if gic == "3" { "agintcmsi0 at agintc0" } else { "ampintcmsi0 at ampintc0: nspi 64" }

default:
    @just --list

# --- build -------------------------------------------------------------------

# `features` lets the image recipes build with `--features qemu` (emulator exit codes).
build-amd64 features="":
    cargo build -p bsd --target {{amd64}} {{features}}

build-arm64 features="":
    cargo build -p bsd --target {{arm64}} {{features}}

# The freestanding init(8) stand-in (init/), a static user ELF the image carries as a Limine
# module (M6).
build-init-amd64:
    cargo build -p init --target {{amd64}}

build-init-arm64:
    cargo build -p init --target {{arm64}}

build: build-amd64 build-arm64 build-init-amd64 build-init-arm64 build-mp efiboot-amd64 efiboot-arm64

# The MULTIPROCESSOR kernels (option MULTIPROCESSOR, M11a), so the MP paths build on every
# commit; `smoke` boots them (with `--features qemu`) since M11e.
build-mp: (build-amd64 "--features multiprocessor") (build-arm64 "--features multiprocessor")

# M14: OpenBSD's efiboot, amd64's BOOTX64.EFI (sys/arch/amd64/stand/efiboot, with libsa and
# boot(8)'s sys/stand/boot). Linked position-independent at 0 (relocation-model=pie, which
# replaces the kernel's static one: RUSTFLAGS overrides .cargo/config.toml's target
# rustflags), in a target directory of its own so the kernel's builds are not redone, then
# made a PE32+ image with `llvm-objcopy -O binary` (cargo xtask efiboot): target/efiboot/
# amd64/BOOTX64.EFI. docs/ARCHITECTURE.md, "Boot loaders".
efiboot-amd64:
    RUSTFLAGS="-C relocation-model=pie" cargo build -p efiboot-amd64 --target {{amd64}} --target-dir target/efiboot
    cargo xtask efiboot --arch amd64 --elf target/efiboot/{{amd64}}/debug/bootx64

# M14 track A3: arm64's BOOTAA64.EFI (sys/arch/arm64/stand/efiboot), built as amd64's:
# target/efiboot/arm64/BOOTAA64.EFI, its PE header OpenBSD's own (`.peheader`, start.S).
efiboot-arm64:
    RUSTFLAGS="-C relocation-model=pie" cargo build -p efiboot-arm64 --target {{arm64}} --target-dir target/efiboot
    cargo xtask efiboot --arch arm64 --elf target/efiboot/{{arm64}}/debug/bootaa64

# --- boot images and QEMU ---------------------------------------------------

image-amd64: (build-amd64 "--features qemu") build-init-amd64
    cargo xtask image --arch amd64 --kernel target/{{amd64}}/debug/bsd

image-arm64: (build-arm64 "--features qemu") build-init-arm64
    cargo xtask image --arch arm64 --kernel target/{{arm64}}/debug/bsd

run-amd64: image-amd64
    cargo xtask qemu --arch amd64

run-arm64: image-arm64
    cargo xtask qemu --arch arm64

# `just smoke`: every recipe of `smokes`, `jobs` at a time (`cargo xtask smoke-all`,
# tools/xtask/src/smokeall.rs). `smoke-build` first builds all they boot, once and in order;
# then each recipe runs as `just --no-deps <recipe>` in a directory of its own,
# target/smoke/<recipe> (`EMIBSD_RUN_DIR`: its boot images, EDK2 variable stores and
# persistent disks, and `log`, its output), so no two recipes write the same file, and with
# its time limits scaled for the shared cores. A line is printed as each recipe ends, the log
# of every failed one at the end. `JOBS=8 just smoke` (or `just jobs=8 smoke`) runs eight at a
# time, `JOBS=1` one after the other. A new smoke recipe goes into `smokes`; `just <recipe>`
# alone still builds what it needs and runs in target/.
jobs := env("JOBS", "4")
smokes := "smoke-boot smoke-shell smoke-login smoke-net smoke-route smoke-diag smoke-link " + \
    "smoke-wg smoke-pf smoke-ipsec smoke-esp smoke-pfsync smoke-ipcomp smoke-https smoke-tcp " + \
    "smoke-divert smoke-tcpdump smoke-inet6 smoke-disk smoke-ufsopts smoke-fs smoke-cd smoke-softraid " + \
    "smoke-nvme smoke-ahci smoke-smmu smoke-power smoke-siop smoke-em smoke-re smoke-vmx smoke-efiboot smoke-acpi smoke-gicv3 smoke-clock smoke-rtc " + \
    "smoke-nfs smoke-ext2fs smoke-fuse smoke-ntfs smoke-tcpbench smoke-mp smoke-ddbmp " + \
    "smoke-net-mp smoke-up smoke-audio smoke-usb smoke-puc smoke-fb smoke-wscons smoke-vga smoke-kbd " + \
    "smoke-powerbtn smoke-mouse smoke-ugen smoke-cdce"

smoke: smoke-build
    cargo xtask smoke-all -j {{jobs}} --just {{quote(just_executable())}} {{smokes}}

# What the smoke recipes boot: the MULTIPROCESSOR kernels with `--features qemu`, the init
# stand-ins, and `smoke-up`'s uniprocessor kernels (`build-up`).
smoke-build: build-up (build-amd64 "--features qemu,multiprocessor") (build-arm64 "--features qemu,multiprocessor") build-init-amd64 build-init-arm64 efiboot-amd64 efiboot-arm64 build-bsdrd

# `smoke-boot`, the first of `smokes` (it was `smoke`'s own body until the smokes ran in
# parallel). Boots per arch, every one with a virtio network card on QEMU's user network: a
# plain one that must reach the end of main() (status 33), printing the EmiBSD 8.0 version banner and the
# virtio attach lines, with init checking its identity through sysctl(2) and the vfs system
# calls failing as they must with no root file system yet (`main` says it cannot mount root and
# `check_console` that /dev/console does not exist) and making the console tty its controlling
# terminal (`init: tty ok`); `boot -d`, which
# enters ddb(4) through a breakpoint trap, prints where it stopped and gives the `ddb{0}> ` prompt,
# where the smoke types an empty line (arm64 reads the first line typed at that early stop as
# newlines: the PL011 is still as the firmware left it), `help` (the command list), `machine`
# (amd64 lists `sysregs`; arm64's table holds only MULTIPROCESSOR commands), `set $lines = 0`
# (no `--db_more--` pager), `show registers`, `trace` and `continue`, after which the boot goes
# on (status 33);
# `selftest=trap`, a deliberate bad access that must print OpenBSD's fatal trap message and
# panic with a stack trace (status 35); and `selftest=uart`, which opens the console's tty through
# the device switch, gets a line typed on the serial console through the line discipline and
# echoes it (status 33);
# `selftest=clock`, which waits for hz clock interrupts and a timeout (status 33);
# `selftest=kthread`, two kernel threads passing a turn with msleep/wakeup (status 33); and
# `selftest=taskq`, tasks run by systq, systqmp and a created then destroyed queue (status 33);
# and `selftest=vio`, which brings vio0 up, sends an ARP request for QEMU's gateway and waits
# for a frame through the receive interrupt (status 33).
# The default boot's init stand-in also checks the Internet sockets (`init: inet sockets ok`:
# vio0's address through SIOCGIFADDR, a ping from a raw ICMP socket, a local UDP datagram) and
# PF_KEY (`init: pfkey ok`: SADB_REGISTER on a PF_KEY socket and its answer) and TCP
# (`init: tcp ok`: lo0 configured, connect/accept on 127.0.0.1, a line each way, FIN, close).
# All of those boot without a ramdisk (`--ramdisk none`, so the kernel says
# `rd: no ramdisk module`, `--expect-ramdisk`) and run the Rust stand-in init, the kernel's
# self-test. Next in `smokes`, `smoke-shell` (M8's exit criterion) boots the ffs ramdisk `just userland`
# makes, booted `-s` (RB_SINGLE; a plain boot goes multi-user, see `smoke-login`): rd(4) reads
# its superblock, the root is mounted from rd0a, OpenBSD's init(8) runs from it and goes single
# user, and ksh(1) answers `uname -a`, `uname -sr`, `cat /etc/motd` and `ls /` on the serial
# console.
# Since M13 the amd64 boot enumerates its interrupt hardware from ACPI, as OpenBSD does:
# `acpimadt0` reads the MADT (the local APIC address, the processors, which attach at mainbus
# from it, and `ioapic0`, its 24 pins), `acpiprt0` reads bus 0's `_PRT` (the PCI INTx pins
# on the I/O APIC), `acpipci0` is the host bridge (`PCI0`, which printed `"PNP0A08" ... not
# configured` before; its `_OSC` answer follows the name) through which pci0 attaches with
# MSI enabled, and every device interrupt goes through the I/O APIC or MSI(-X); the virtio
# devices take one MSI-X vector per queue (`virtio0: msix per-VQ`, where they printed
# `irq N` through the 8259 before), as OpenBSD/amd64 on QEMU does.
smoke-boot: (build-amd64 "--features qemu,multiprocessor") (build-arm64 "--features qemu,multiprocessor") build-init-amd64 build-init-arm64
    cargo xtask smoke {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --ramdisk none --expect-ramdisk \
        --expect "bsd: booted on amd64" --expect "The Regents of the University of California" \
        --expect "EmiBSD 8.0 (GENERIC) #" \
        --expect "real mem = " --expect "avail mem = " --expect "selftest: pmap kernel mapping ok" \
        --expect "selftest: malloc/pool stress ok" --expect "selftest: mbufs ok" \
        --expect "selftest: buffer cache ok" --expect "selftest: pager map ok" \
        --expect "selftest: bus_dma ok" --expect "mainbus0 at root" \
        --expect "bios0 at mainbus0" --expect "acpi0 at bios0: ACPI 3.0" \
        --expect "acpi0: sleep states S3 S4 S5" --expect "acpi0: tables DSDT FACP APIC HPET MCFG" \
        --expect "acpitimer0 at acpi0: 3579545 Hz, 24 bits" --expect "acpihpet0 at acpi0: 100000000 Hz" \
        --expect "acpimadt0 at acpi0 addr 0xfee00000: PC-AT compat" \
        --expect "ioapic0 at mainbus0: apid 0 pa 0xfec00000, version 20, 24 pins" \
        --expect "acpiprt0 at acpi0: bus 0 (PCI0)" \
        --expect "acpipci0 at acpi0 PCI0: 0x" \
        --expect "cpu0 at mainbus0: apid 0 (boot processor)" --expect "pci0 at mainbus0 bus 0" \
        --expect "at pci0 dev 0 function 0 not configured" \
        --expect "virtio0 at pci0 dev 2 function 0 vendor 0x1af4 product 0x1000 rev 0x00" \
        --expect "vio0 at virtio0: 1 queue, address 52:54:00:12:34:56" --expect "virtio0: msix per-VQ" \
        --expect "virtio1 at pci0 dev 3 function 0 vendor 0x1af4 product 0x1001 rev 0x00" \
        --expect "vioblk0 at virtio1" --expect "scsibus0 at vioblk0: 1 targets" \
        --expect "sd0 at scsibus0 targ 0 lun 0: <VirtIO, Block Device, >" \
        --expect "isa0 at mainbus0" \
        --expect "com0 at isa0 port 0x3f8/8 irq 4: ns16550a, 16 byte fifo" --expect "com0: console" \
        --expect "cpu0: apic clock running at" \
        --expect "module: /init (" --expect "init: hello from user mode" --expect "init: argv and auxv ok" \
        --expect "init: demand-zero bss ok" --expect "init: ids and tcb ok" \
        --expect "init: fds ok" --expect "init: signals ok" --expect "init: EmiBSD 8.0" \
        --expect "cannot mount root: no root file system" \
        --expect "warning: /dev/console does not exist" --expect "init: vfs ok (no root file system)" \
        --expect "init: pipes ok" --expect "init: sockets ok" --expect "init: wg ok" --expect "init: kqueue ok" --expect "init: inet sockets ok" --expect "init: pfkey ok" --expect "init: tcp ok" --expect "init: processes ok" --expect "init: pledge ok" --expect "init: time ok" --expect "init: uptime monotonic ok" --expect "init: unveil ok" --expect "init: sendsyslog ok" --expect "pinsyscalls addr" \
        --expect "selftest: pmap reuse ok" --expect "selftest: ping 10.0.2.2: echo reply received" \
        --expect "init: tty ok" \
        --expect "init exited with status 0 (signal 0)"
    cargo xtask smoke {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --ramdisk none --cmdline "-d" \
        --send-after "ddb{0}> " --send '\n' --send-after "ddb{0}> " --send 'help\n' \
        --send-after "ddb{0}> " --send 'machine\n' --send-after "ddb{0}> " --send 'set $lines = 0\n' \
        --send-after "ddb{0}> " --send 'show registers\n' --send-after "ddb{0}> " --send 'trace\n' \
        --send-after "ddb{0}> " --send 'continue\n' \
        --expect "Stopped at" --expect "hangman" --expect " at 0x" --expect "sysregs" --expect "rflags" \
        --expect "selftest: malloc/pool stress ok"
    cargo xtask smoke {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --ramdisk none --cmdline "selftest=trap" --status 35 \
        --expect "fatal page fault in supervisor mode" --expect "trap type 6 code" \
        --expect "panic: trap type 6, code=" --expect "Starting stack trace..." \
        --expect "End of stack trace." --expect "The operating system has halted."
    cargo xtask smoke {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --ramdisk none --cmdline "selftest=uart" \
        --send-after "selftest: uart rx interrupt armed" --send 'hello\n' \
        --expect "selftest: uart rx interrupt armed" --expect "selftest: uart echo: hello"
    cargo xtask smoke {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --ramdisk none --cmdline "selftest=clock" \
        --expect "selftest: clock ok"
    cargo xtask smoke {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --ramdisk none --cmdline "selftest=kthread" \
        --expect "selftest: kthread ping-pong ok"
    cargo xtask smoke {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --ramdisk none --cmdline "selftest=taskq" \
        --expect "selftest: taskq ok"
    cargo xtask smoke {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --ramdisk none --cmdline "selftest=vio" \
        --expect "selftest: vio up ok" --expect "selftest: vio rx ok"
    cargo xtask smoke {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --ramdisk none --expect-ramdisk \
        --expect "bsd: booted on arm64" --expect "The Regents of the University of California" \
        --expect "EmiBSD 8.0 (GENERIC) #" \
        --expect "real mem  = " --expect "avail mem = " --expect "selftest: pmap kernel mapping ok" \
        --expect "selftest: malloc/pool stress ok" --expect "selftest: mbufs ok" \
        --expect "selftest: buffer cache ok" --expect "selftest: pager map ok" \
        --expect "mainbus0 at root" --expect "{{gic_attach}}" \
        --expect "agtimer0 at mainbus0: " --expect "selftest: bus_dma ok" \
        --expect "efi0 at mainbus0: UEFI 2." --expect "efi0: EDK II rev 0x" \
        --expect "virtio0 at mainbus0: Virtio Unknown (0) Device" \
        --expect "virtio30 at mainbus0: Virtio Network Device" \
        --expect "virtio29 at mainbus0: Virtio Block Device" --expect "vioblk0 at virtio29" \
        --expect "virtio31 at mainbus0: Virtio Block Device" --expect "vioblk1 at virtio31" \
        --expect "sd0 at scsibus0 targ 0 lun 0: <VirtIO, Block Device, >" \
        --expect "sd1 at scsibus1 targ 0 lun 0: <VirtIO, Block Device, >" \
        --expect "vio0 at virtio30: 1 queue, address 52:54:00:12:34:56" \
        --expect ": rev 1, 16 byte fifo" --expect "pluart0: console" \
        --expect "module: /init (" --expect "init: hello from user mode" --expect "init: argv and auxv ok" \
        --expect "init: demand-zero bss ok" --expect "init: ids and tcb ok" \
        --expect "init: fds ok" --expect "init: signals ok" --expect "init: EmiBSD 8.0" \
        --expect "cannot mount root: no root file system" \
        --expect "warning: /dev/console does not exist" --expect "init: vfs ok (no root file system)" \
        --expect "init: pipes ok" --expect "init: sockets ok" --expect "init: wg ok" --expect "init: kqueue ok" --expect "init: inet sockets ok" --expect "init: pfkey ok" --expect "init: tcp ok" --expect "init: processes ok" --expect "init: pledge ok" --expect "init: time ok" --expect "init: uptime monotonic ok" --expect "init: unveil ok" --expect "init: sendsyslog ok" --expect "pinsyscalls addr" \
        --expect "selftest: pmap reuse ok" --expect "selftest: ping 10.0.2.2: echo reply received" \
        --expect "init: tty ok" \
        --expect "init exited with status 0 (signal 0)"
    cargo xtask smoke {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --ramdisk none --cmdline "-d" \
        --send-after "ddb{0}> " --send '\n' --send-after "ddb{0}> " --send 'help\n' \
        --send-after "ddb{0}> " --send 'machine\n' --send-after "ddb{0}> " --send 'set $lines = 0\n' \
        --send-after "ddb{0}> " --send 'show registers\n' --send-after "ddb{0}> " --send 'trace\n' \
        --send-after "ddb{0}> " --send 'continue\n' \
        --expect "Stopped at" --expect "hangman" --expect " at 0x" --expect "spsr" \
        --expect "selftest: malloc/pool stress ok"
    cargo xtask smoke {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --ramdisk none --cmdline "selftest=trap" --status 35 \
        --expect "panic: uvm_fault failed:" --expect "Starting stack trace..." \
        --expect "End of stack trace." --expect "The operating system has halted."
    cargo xtask smoke {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --ramdisk none --cmdline "selftest=uart" \
        --send-after "selftest: uart rx interrupt armed" --send 'hello\n' \
        --expect "selftest: uart rx interrupt armed" --expect "selftest: uart echo: hello"
    cargo xtask smoke {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --ramdisk none --cmdline "selftest=clock" \
        --expect "selftest: clock ok"
    cargo xtask smoke {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --ramdisk none --cmdline "selftest=kthread" \
        --expect "selftest: kthread ping-pong ok"
    cargo xtask smoke {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --ramdisk none --cmdline "selftest=taskq" \
        --expect "selftest: taskq ok"
    cargo xtask smoke {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --ramdisk none --cmdline "selftest=vio" \
        --expect "selftest: vio up ok" --expect "selftest: vio rx ok"

# M8: OpenBSD's init(8) and ksh(1) from the ffs ramdisk, driven over the serial console. Needs
# `just userland` (the ramdisk image); stops once every expected line was seen.
smoke-shell: (build-amd64 "--features qemu,multiprocessor") (build-arm64 "--features qemu,multiprocessor")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-shell: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --cmdline "-s" --expect-ramdisk --until-seen \
        --send-after "RETURN for sh:" --send '\n' \
        --send-after "# " --send 'uname -a\n' --send-after "GENERIC#" --send 'uname -sr\n' \
        --send-after "EmiBSD 8.0" --send 'cat /etc/motd\n' \
        --send-after "Welcome to EmiBSD" --send 'ls /\n' \
        --send-after "bin  dev  etc" --send 'ls /sbin\n' \
        --expect "root on rd0a swap on rd0b dump on rd0b" \
        --expect "Enter pathname of shell or RETURN for sh:" \
        --expect " 8.0 GENERIC#" --expect "amd64" \
        --expect "Welcome to EmiBSD 8.0: OpenBSD's init(8) and ksh(1)" \
        --expect "bin  dev  etc  home mnt  root sbin tmp  usr  var" --expect "pfctl"
    cargo xtask smoke {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --cmdline "-s" --expect-ramdisk --until-seen \
        --send-after "RETURN for sh:" --send '\n' \
        --send-after "# " --send 'uname -a\n' --send-after "GENERIC#" --send 'uname -sr\n' \
        --send-after "EmiBSD 8.0" --send 'cat /etc/motd\n' \
        --send-after "Welcome to EmiBSD" --send 'ls /\n' \
        --send-after "bin  dev  etc" --send 'ls /sbin\n' \
        --expect "root on rd0a swap on rd0b dump on rd0b" \
        --expect "Enter pathname of shell or RETURN for sh:" \
        --expect " 8.0 GENERIC#" --expect "arm64" \
        --expect "Welcome to EmiBSD 8.0: OpenBSD's init(8) and ksh(1)" \
        --expect "bin  dev  etc  home mnt  root sbin tmp  usr  var" --expect "pfctl"

# smoke-login's `#!` script (M14), typed at the shell prompt.
script_line := 'echo "#!/bin/sh" >/tmp/s; echo "echo sh-ran-\$((6*7))-\$0-\$1" >>/tmp/s; chmod +x /tmp/s; /tmp/s ok\n'
ksh_line := 'echo "#!/bin/ksh" >/tmp/k; echo "(( y = 6 * 7 )); echo ksh-arith-\$y" >>/tmp/k; chmod 755 /tmp/k; /tmp/k\n'

# M8b: a plain boot of the ramdisk goes multi-user: init(8) runs /etc/rc (`rc: multi-user`),
# then getty(8) on tty00 prints `login:`; the session logs in as root (the test image's
# password, docs/SETUP.md) and runs `id` and `uname -a`, then checks that the clock came from
# the time-of-day chip (mc146818 on amd64, the UEFI runtime services, efi0, on arm64): later
# than 2026-10-03 (1790985600), a day past the ramdisk's fixed file system time, which is
# what the kernel would run on without one. `rtc-$x` keeps the echoed command line from
# matching. M14: a two-line `#!/bin/sh` script, made executable and run with one argument,
# prints `sh-ran-42-/tmp/s-ok` through the kernel's `exec_script.c` ($0 is the script's path,
# $1 its argument; `$((6*7))` keeps the echoed line from matching), then a `#!/bin/ksh` one
# with `(( ))` arithmetic prints `ksh-arith-42` (`\$y` likewise). Part of `smoke`.
smoke-login: (build-amd64 "--features qemu,multiprocessor") (build-arm64 "--features qemu,multiprocessor")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-login: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen \
        --send-after "login:" --send 'root\n' --send-after "Password:" --send 'emibsd\n' \
        --send-after "# " --send 'id\n' --send-after "uid=0(root)" --send 'uname -a\n' \
        --send-after " 8.0 GENERIC#" --send 'x=ok; [ $(date +%s) -gt 1790985600 ] && echo rtc-$x\n' \
        --send-after "rtc-ok" --send '{{script_line}}' \
        --send-after "sh-ran-42-/tmp/s-ok" --send '{{ksh_line}}' \
        --expect "rc: multi-user" --expect "EmiBSD/amd64 (Amnesiac) (tty00)" \
        --expect "uid=0(root)" --expect " 8.0 GENERIC#" --expect "amd64" --expect "rtc-ok" \
        --expect "sh-ran-42-/tmp/s-ok" --expect "ksh-arith-42"
    cargo xtask smoke {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --expect-ramdisk --until-seen \
        --send-after "login:" --send 'root\n' --send-after "Password:" --send 'emibsd\n' \
        --send-after "# " --send 'id\n' --send-after "uid=0(root)" --send 'uname -a\n' \
        --send-after " 8.0 GENERIC#" --send 'x=ok; [ $(date +%s) -gt 1790985600 ] && echo rtc-$x\n' \
        --send-after "rtc-ok" --send '{{script_line}}' \
        --send-after "sh-ran-42-/tmp/s-ok" --send '{{ksh_line}}' \
        --expect "rc: multi-user" --expect "EmiBSD/arm64 (Amnesiac) (tty00)" \
        --expect "uid=0(root)" --expect " 8.0 GENERIC#" --expect "arm64" --expect "rtc-ok" \
        --expect "sh-ran-42-/tmp/s-ok" --expect "ksh-arith-42"

# M9a: the routing socket and the `net.route` sysctl from userland. Logs in as `smoke-login`
# does, then runs OpenBSD's route(8) (`show`: a routing socket, then NET_RT_DUMP through
# sysctl(2); `get`: RTM_GET written to the routing socket and its answer read back) and
# ifconfig(8) (getifaddrs(3): NET_RT_IFLIST, then interface ioctls on an AF_INET socket). The kernel's network self-test configured vio0 (10.0.2.15) and the default route
# through 10.0.2.2 before init ran. Part of `smoke`.
smoke-route: (build-amd64 "--features qemu,multiprocessor") (build-arm64 "--features qemu,multiprocessor")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-route: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen \
        --send-after "login:" --send 'root\n' --send-after "Password:" --send 'emibsd\n' \
        --send-after "# " --send 'route -n show -inet\n' \
        --send-after "# " --send 'route -n get 8.8.8.8\n' \
        --send-after "# " --send 'ifconfig -a\n' \
        --expect "rc: multi-user" --expect "Internet:" --expect "default            10.0.2.2" \
        --expect "10.0.2/24" --expect "gateway: 10.0.2.2" --expect "interface: vio0" \
        --expect "lo0: flags=" --expect "vio0: flags=" {{https_run}}
    cargo xtask smoke {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --expect-ramdisk --until-seen \
        --send-after "login:" --send 'root\n' --send-after "Password:" --send 'emibsd\n' \
        --send-after "# " --send 'route -n show -inet\n' \
        --send-after "# " --send 'route -n get 8.8.8.8\n' \
        --send-after "# " --send 'ifconfig -a\n' \
        --expect "rc: multi-user" --expect "Internet:" --expect "default            10.0.2.2" \
        --expect "10.0.2/24" --expect "gateway: 10.0.2.2" --expect "interface: vio0" \
        --expect "lo0: flags=" --expect "vio0: flags=" {{https_run}}

# M9+, part of `smoke-route` (so `ci` covers the LibreSSL build): ftp(1) and nc(1) run
# (usage), `/etc/ssl` holds `cert.pem` and the test CA, and ftp resolves `emibsd-host` through
# `/etc/hosts` (`Trying 10.0.2.2...`) and fails at the TCP step with status 1 (no TCP in the
# kernel yet: `socket: Protocol not supported`; with TCP and no server: connection refused).
# Lines stay short for arm64's pluart.
https_run := "--send-after '# ' --send 'ls -l /etc/ssl\\n' --send-after '# ' --send 'ftp -? ; nc -h\\n' " + \
    "--send-after '# ' --send 'u=https://emibsd-host:8443/hello.txt\\n' " + \
    "--send-after '# ' --send 'ftp -v -o - $u; echo ftp-exit=$?\\n' " + \
    "--expect emibsd-test-ca.pem --expect 'usage: ftp' --expect 'usage: nc' " + \
    "--expect 'Trying 10.0.2.2...' --expect ftp-exit=1"

# M9+ (docs/ROADMAP.md, "M9+ Network completion"): HTTPS from userland against TLS servers on
# this machine (`--https-server`: `openssl s_server` with the test CA's certificates, reached
# by the guest as `emibsd-host`, 10.0.2.2; docs/SETUP.md, "The test CA"). Logs in as root,
# fetches `hello.txt` with ftp(1) trusting the test CA, is refused by the self-signed server
# (`certificate verification failed`), and has a line echoed back over TLS by nc(1) (`-c`,
# `-R` the CA, `-e` the expected name). Needs `just userland`. Part of `smoke`.
smoke-https: (build-amd64 "--features qemu,multiprocessor") (build-arm64 "--features qemu,multiprocessor")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-https: no ramdisk image; run just userland first"; exit 1; }
    @mkdir -p target/https-www && echo 'hello from emibsd-host over https' >target/https-www/hello.txt
    cargo xtask smoke {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen {{https_check}}
    cargo xtask smoke {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --expect-ramdisk --until-seen {{https_check}}

https_check := "--https-server target/https-www:8443:trusted --https-server target/https-www:8444:echo " + \
    "--https-server target/https-www:8445:untrusted " + \
    "--send-after login: --send 'root\\n' --send-after Password: --send 'emibsd\\n' " + \
    "--send-after '# ' --send 'date; c=cafile=/etc/ssl/emibsd-test-ca.pem\\n' " + \
    "--send-after '# ' --send 'h=emibsd-host; u=https://emibsd-host\\n' " + \
    "--send-after '# ' --send 'ftp -S $c -o - $u:8443/hello.txt\\n' " + \
    "--send-after '# ' --send 'ftp -S $c -o - $u:8445/hello.txt\\n' " + \
    "--send-after '# ' --send 'r=\"-R /etc/ssl/emibsd-test-ca.pem\"\\n' " + \
    "--send-after '# ' --send 'echo emibsd-$((6*7))-echo | nc -w 5 -c $r -e $h $h 8444\\n' " + \
    "--expect 'rc: multi-user' --expect 'hello from emibsd-host over https' " + \
    "--expect 'certificate verification failed' --expect emibsd-42-echo"

# M9+, outside `smoke` and `ci` (it needs the Internet): ftp(1) fetches a small file from
# https://www.openbsd.org, resolving the name through QEMU's DNS (10.0.2.3, `/etc/resolv.conf`)
# and verifying the server with LibreSSL's default bundle, `/etc/ssl/cert.pem`. Needs TCP.
smoke-internet: (build-amd64 "--features qemu,multiprocessor") (build-arm64 "--features qemu,multiprocessor")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-internet: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen {{internet_check}}
    cargo xtask smoke {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --expect-ramdisk --until-seen {{internet_check}}

internet_check := "--send-after login: --send 'root\\n' --send-after Password: --send 'emibsd\\n' " + \
    "--send-after '# ' --send 'ftp -o - https://www.openbsd.org/robots.txt\\n' " + \
    "--expect 'rc: multi-user' --expect 'User-agent:'"

# Diagnostic tools stage 2: OpenBSD's ps(1), fstat(1) and vmstat(8) over libkvm's sysctl(2)
# paths (kern.proc, kern.proc_args, kern.file, vm.uvmexp, hw.diskstats, kern.intrcnt,
# kern.pool, kern.malloc), df(1) and mount(8) over getfsstat(2), and sysctl(8)'s
# kern.timecounter (arm64 runs on agtimer; amd64 offers i8254, tsc, acpihpet0 and acpitimer0
# and runs on the TSC once acpihpet0 calibrated it, quality 2000, else on acpihpet0 with the
# TSC at -1000, as OpenBSD does when every calibration round is disturbed). Logs in as `smoke-login`
# does; `echo diag-$((40+2))` marks the end. vmstat's disk columns are the first two disks
# of hw.disknames: on both archs now sd0 (the virtio-blk disk) and sd1 (the boot image: on
# arm64 a vioblk, on amd64 port 0 of q35's AHCI controller since ahci(4), M13; before it the
# amd64 header read `sd0 rd0`). Part of `smoke`.
smoke-diag: (build-amd64 "--features qemu,multiprocessor") (build-arm64 "--features qemu,multiprocessor")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-diag: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen \
        --send-after "login:" --send 'root\n' --send-after "Password:" --send 'emibsd\n' \
        --send-after "# " --send 'ps -ax\n' \
        --send-after "# " --send 'ps -aux\n' \
        --send-after "# " --send 'fstat\n' \
        --send-after "# " --send 'vmstat\n' \
        --send-after "# " --send 'vmstat -i\n' \
        --send-after "# " --send 'vmstat -s\n' \
        --send-after "# " --send 'vmstat -m\n' \
        --send-after "# " --send 'df\n' \
        --send-after "# " --send 'mount\n' \
        --send-after "# " --send 'sysctl kern.timecounter\n' \
        --send-after "# " --send 'echo diag-$((40+2))\n' \
        --expect "rc: multi-user" --expect " /sbin/init" --expect " -ksh (ksh)" \
        --expect "root         1  " \
        --expect "USER     CMD          PID   FD MOUNT" --expect "root     ksh" \
        --expect "rw    tty00" --expect "sr sd0 sd1  int" \
        --expect "interrupt                       total     rate" --expect "/com0" \
        --expect "bytes per page" --expect "Memory statistics by bucket size" \
        --expect "Memory resource pool statistics" --expect "/dev/rd0a       " \
        --expect "/dev/rd0a on / type ffs (local)" --expect "diag-42" \
        --expect "kern.timecounter.hardware=" \
        --expect "kern.timecounter.choice=i8254(0) acpihpet0(1000) tsc(" --expect ") acpitimer0(1000)"
    cargo xtask smoke {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --expect-ramdisk --until-seen \
        --send-after "login:" --send 'root\n' --send-after "Password:" --send 'emibsd\n' \
        --send-after "# " --send 'ps -ax\n' \
        --send-after "# " --send 'ps -aux\n' \
        --send-after "# " --send 'fstat\n' \
        --send-after "# " --send 'vmstat\n' \
        --send-after "# " --send 'vmstat -i\n' \
        --send-after "# " --send 'vmstat -s\n' \
        --send-after "# " --send 'vmstat -m\n' \
        --send-after "# " --send 'df\n' \
        --send-after "# " --send 'mount\n' \
        --send-after "# " --send 'sysctl kern.timecounter\n' \
        --send-after "# " --send 'echo diag-$((40+2))\n' \
        --expect "rc: multi-user" --expect " /sbin/init" --expect " -ksh (ksh)" \
        --expect "root         1  " \
        --expect "USER     CMD          PID   FD MOUNT" --expect "root     ksh" \
        --expect "rw    tty00" --expect "sr sd0 sd1  int" \
        --expect "interrupt                       total     rate" --expect "/pluart0" \
        --expect "bytes per page" --expect "Memory statistics by bucket size" \
        --expect "Memory resource pool statistics" --expect "/dev/rd0a       " \
        --expect "/dev/rd0a on / type ffs (local)" --expect "diag-42" \
        --expect "kern.timecounter.hardware=agtimer" --expect "kern.timecounter.choice=agtimer(0)"

# M9b/M9c harness: two VMs of one arch at once (`cargo xtask smoke2`), each with vio0 on QEMU's
# user network and vio1 on a private link between the two (docs/SETUP.md, "Two VMs"). Both log
# in as root, give vio1 an address on 192.168.77.0/24 and ping each other across the link.
smoke-link: (build-amd64 "--features qemu,multiprocessor") (build-arm64 "--features qemu,multiprocessor")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-link: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke2 {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd \
        --both-send-after "login:" --both-send 'root\n' --both-send-after "Password:" --both-send 'emibsd\n' \
        --a-send-after "# " --a-send 'ifconfig vio1 inet 192.168.77.1/24 up\n' \
        --a-send-after "# " --a-send 'ping -c 10 192.168.77.2\n' \
        --b-send-after "# " --b-send 'ifconfig vio1 inet 192.168.77.2/24 up\n' \
        --b-send-after "# " --b-send 'ping -c 10 192.168.77.1\n' \
        --a-expect "vio1 at virtio1: 1 queue, address 52:54:00:bb:00:01" \
        --b-expect "vio1 at virtio1: 1 queue, address 52:54:00:bb:00:02" \
        --a-expect "bytes from 192.168.77.2: icmp_seq=" --b-expect "bytes from 192.168.77.1: icmp_seq="
    cargo xtask smoke2 {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd \
        --both-send-after "login:" --both-send 'root\n' --both-send-after "Password:" --both-send 'emibsd\n' \
        --a-send-after "# " --a-send 'ifconfig vio1 inet 192.168.77.1/24 up\n' \
        --a-send-after "# " --a-send 'ping -c 10 192.168.77.2\n' \
        --b-send-after "# " --b-send 'ifconfig vio1 inet 192.168.77.2/24 up\n' \
        --b-send-after "# " --b-send 'ping -c 10 192.168.77.1\n' \
        --a-expect "vio1 at virtio30: 1 queue, address 52:54:00:bb:00:01" \
        --b-expect "vio1 at virtio30: 1 queue, address 52:54:00:bb:00:02" \
        --a-expect "bytes from 192.168.77.2: icmp_seq=" --b-expect "bytes from 192.168.77.1: icmp_seq="

# M9b: a wg(4) tunnel between the two VMs of `smoke-link`, configured with OpenBSD's
# ifconfig(8): wg0 is 10.77.0.1 on A and 10.77.0.2 on B, the outer endpoints are vio1's
# addresses, the keys are RFC 7748's test vectors (A = Alice, B = Bob). Each side pings the
# other through the tunnel, then `ifconfig wg0` shows the peer's handshake. Then A loads a pf
# rule on wg0 (M9d): the tunnel's ping is blocked, and passes again after `pfctl -d`.
smoke-wg: (build-amd64 "--features qemu,multiprocessor") (build-arm64 "--features qemu,multiprocessor")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-wg: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke2 {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd \
        --both-send-after "login:" --both-send 'root\n' --both-send-after "Password:" --both-send 'emibsd\n' \
        --a-send-after "# " --a-send 'ifconfig vio1 inet 192.168.77.1/24 up\n' \
        --a-send-after "# " --a-send 'ifconfig wg0 create wgport 51820 wgkey dwdtCnMYpX08FsFyUbJmRd9ML4frwJkqsXf7pR25LCo=\n' \
        --a-send-after "# " --a-send 'ifconfig wg0 wgpeer 3p7bfXt9wbTTW2HC7OQ1Nz+DQ8hbeGdNrfx+FG+IK08= wgendpoint 192.168.77.2 51820 wgaip 10.77.0.2/32\n' \
        --a-send-after "# " --a-send 'ifconfig wg0 inet 10.77.0.1/24 up\n' \
        --a-send-after "# " --a-send 'ping -c 15 10.77.0.2\n' \
        --a-send-after "packet loss" --a-send 'ifconfig wg0\n' \
        --a-send-after "last handshake: " --a-send "echo 'block drop quick on wg0 inet proto icmp' | pfctl -e -f -\n" \
        --a-send-after "# " --a-send 'pfctl -sr\n' \
        --a-send-after "# " --a-send 'ping -c 2 -w 2 10.77.0.2 || echo wg-blocked-$((4+4))\n' \
        --a-send-after "# " --a-send 'pfctl -d\n' \
        --a-send-after "# " --a-send 'ping -c 1 10.77.0.2 && echo wg-passes-$((5+5))\n' \
        --b-send-after "# " --b-send 'ifconfig vio1 inet 192.168.77.2/24 up\n' \
        --b-send-after "# " --b-send 'ifconfig wg0 create wgport 51820 wgkey XasIfmJKikt54X+Lg4AO5m87sSkmGLb9HC+LJ/+I4Os=\n' \
        --b-send-after "# " --b-send 'ifconfig wg0 wgpeer hSDwCYkwp1R0i33ctD73Wg2/Og0mOBr066SpjqqbTmo= wgendpoint 192.168.77.1 51820 wgaip 10.77.0.1/32\n' \
        --b-send-after "# " --b-send 'ifconfig wg0 inet 10.77.0.2/24 up\n' \
        --b-send-after "# " --b-send 'ping -c 15 10.77.0.1\n' \
        --b-send-after "packet loss" --b-send 'ifconfig wg0\n' \
        --a-expect "bytes from 10.77.0.2: icmp_seq=" --b-expect "bytes from 10.77.0.1: icmp_seq=" \
        --a-expect "wgpubkey hSDwCYkwp1R0i33ctD73Wg2/Og0mOBr066SpjqqbTmo=" --b-expect "wgpubkey 3p7bfXt9wbTTW2HC7OQ1Nz+DQ8hbeGdNrfx+FG+IK08=" \
        --both-expect "last handshake: " \
        --a-expect "block drop quick on wg0 inet proto icmp all" --a-expect "wg-blocked-8" \
        --a-expect "pf disabled" --a-expect "wg-passes-10"
    cargo xtask smoke2 {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd \
        --both-send-after "login:" --both-send 'root\n' --both-send-after "Password:" --both-send 'emibsd\n' \
        --a-send-after "# " --a-send 'ifconfig vio1 inet 192.168.77.1/24 up\n' \
        --a-send-after "# " --a-send 'ifconfig wg0 create wgport 51820 wgkey dwdtCnMYpX08FsFyUbJmRd9ML4frwJkqsXf7pR25LCo=\n' \
        --a-send-after "# " --a-send 'ifconfig wg0 wgpeer 3p7bfXt9wbTTW2HC7OQ1Nz+DQ8hbeGdNrfx+FG+IK08= wgendpoint 192.168.77.2 51820 wgaip 10.77.0.2/32\n' \
        --a-send-after "# " --a-send 'ifconfig wg0 inet 10.77.0.1/24 up\n' \
        --a-send-after "# " --a-send 'ping -c 15 10.77.0.2\n' \
        --a-send-after "packet loss" --a-send 'ifconfig wg0\n' \
        --a-send-after "last handshake: " --a-send "echo 'block drop quick on wg0 inet proto icmp' | pfctl -e -f -\n" \
        --a-send-after "# " --a-send 'pfctl -sr\n' \
        --a-send-after "# " --a-send 'ping -c 2 -w 2 10.77.0.2 || echo wg-blocked-$((4+4))\n' \
        --a-send-after "# " --a-send 'pfctl -d\n' \
        --a-send-after "# " --a-send 'ping -c 1 10.77.0.2 && echo wg-passes-$((5+5))\n' \
        --b-send-after "# " --b-send 'ifconfig vio1 inet 192.168.77.2/24 up\n' \
        --b-send-after "# " --b-send 'ifconfig wg0 create wgport 51820 wgkey XasIfmJKikt54X+Lg4AO5m87sSkmGLb9HC+LJ/+I4Os=\n' \
        --b-send-after "# " --b-send 'ifconfig wg0 wgpeer hSDwCYkwp1R0i33ctD73Wg2/Og0mOBr066SpjqqbTmo= wgendpoint 192.168.77.1 51820 wgaip 10.77.0.1/32\n' \
        --b-send-after "# " --b-send 'ifconfig wg0 inet 10.77.0.2/24 up\n' \
        --b-send-after "# " --b-send 'ping -c 15 10.77.0.1\n' \
        --b-send-after "packet loss" --b-send 'ifconfig wg0\n' \
        --a-expect "bytes from 10.77.0.2: icmp_seq=" --b-expect "bytes from 10.77.0.1: icmp_seq=" \
        --a-expect "wgpubkey hSDwCYkwp1R0i33ctD73Wg2/Og0mOBr066SpjqqbTmo=" --b-expect "wgpubkey 3p7bfXt9wbTTW2HC7OQ1Nz+DQ8hbeGdNrfx+FG+IK08=" \
        --both-expect "last handshake: " \
        --a-expect "block drop quick on wg0 inet proto icmp all" --a-expect "wg-blocked-8" \
        --a-expect "pf disabled" --a-expect "wg-passes-10"

# M9a: OpenBSD's ifconfig(8) and ping(8) from the ramdisk, multi-user, logged in as root
# (`smoke-login`'s sends). vio0's address (10.0.2.15/24) and the default route through QEMU's
# gateway come from the kernel's boot self-test (`selftest: ping`), so no /etc/hostname.vio0
# is needed. Part of `smoke`.
smoke-net: (build-amd64 "--features qemu,multiprocessor") (build-arm64 "--features qemu,multiprocessor")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-net: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen \
        --send-after "login:" --send 'root\n' --send-after "Password:" --send 'emibsd\n' \
        --send-after "# " --send 'ifconfig vio0\n' --send-after "# " --send 'ping -c 1 10.0.2.2\n' \
        --expect "rc: multi-user" --expect "vio0: flags=" --expect "inet 10.0.2.15 netmask 0xffffff00" \
        --expect "PING 10.0.2.2 (10.0.2.2): 56 data bytes" \
        --expect "1 packets transmitted, 1 packets received, 0.0% packet loss"
    cargo xtask smoke {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --expect-ramdisk --until-seen \
        --send-after "login:" --send 'root\n' --send-after "Password:" --send 'emibsd\n' \
        --send-after "# " --send 'ifconfig vio0\n' --send-after "# " --send 'ping -c 1 10.0.2.2\n' \
        --expect "rc: multi-user" --expect "vio0: flags=" --expect "inet 10.0.2.15 netmask 0xffffff00" \
        --expect "PING 10.0.2.2 (10.0.2.2): 56 data bytes" \
        --expect "1 packets transmitted, 1 packets received, 0.0% packet loss"

# M9d: pf(4) from userland. Logs in as `smoke-login` does, then: `pfctl -si` (DIOCGETSTATUS:
# disabled), a ping to QEMU's gateway that gets its reply, `pfctl -e` (DIOCSTART) and `pfctl
# -si` again (enabled), `pfctl -f /etc/pf.conf` (a ruleset transaction: DIOCXBEGIN,
# DIOCADDRULE, DIOCXCOMMIT; the ramdisk's pf.conf blocks ICMP to 10.0.2.2) and `pfctl -sr`, the
# same ping blocked, `pfctl -d` (DIOCSTOP) and the ping through again. The echoes print
# computed markers so that the expected lines are not matched by the typed commands. Not part
# of `smoke`.
smoke-pf: (build-amd64 "--features qemu,multiprocessor") (build-arm64 "--features qemu,multiprocessor")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-pf: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen \
        --send-after "login:" --send 'root\n' --send-after "Password:" --send 'emibsd\n' \
        --send-after "# " --send 'pfctl -si\n' \
        --send-after "# " --send 'ping -c 1 10.0.2.2 && echo before-pf-$((1+1))\n' \
        --send-after "# " --send 'pfctl -e\n' --send-after "# " --send 'pfctl -si\n' \
        --send-after "# " --send 'pfctl -f /etc/pf.conf\n' --send-after "# " --send 'pfctl -sr\n' \
        --send-after "# " --send 'ping -c 1 -w 2 10.0.2.2 || echo blocked-$((2+2))\n' \
        --send-after "# " --send 'pfctl -d\n' \
        --send-after "# " --send 'ping -c 1 10.0.2.2 && echo after-pfctl-d-$((3+3))\n' \
        --expect "rc: multi-user" --expect "Status: Disabled" --expect "before-pf-2" \
        --expect "pf enabled" --expect "Status: Enabled" \
        --expect "block drop quick inet proto icmp from any to 10.0.2.2" --expect "blocked-4" \
        --expect "pf disabled" --expect "after-pfctl-d-6"
    cargo xtask smoke {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --expect-ramdisk --until-seen \
        --send-after "login:" --send 'root\n' --send-after "Password:" --send 'emibsd\n' \
        --send-after "# " --send 'pfctl -si\n' \
        --send-after "# " --send 'ping -c 1 10.0.2.2 && echo before-pf-$((1+1))\n' \
        --send-after "# " --send 'pfctl -e\n' --send-after "# " --send 'pfctl -si\n' \
        --send-after "# " --send 'pfctl -f /etc/pf.conf\n' --send-after "# " --send 'pfctl -sr\n' \
        --send-after "# " --send 'ping -c 1 -w 2 10.0.2.2 || echo blocked-$((2+2))\n' \
        --send-after "# " --send 'pfctl -d\n' \
        --send-after "# " --send 'ping -c 1 10.0.2.2 && echo after-pfctl-d-$((3+3))\n' \
        --expect "rc: multi-user" --expect "Status: Disabled" --expect "before-pf-2" \
        --expect "pf enabled" --expect "Status: Enabled" \
        --expect "block drop quick inet proto icmp from any to 10.0.2.2" --expect "blocked-4" \
        --expect "pf disabled" --expect "after-pfctl-d-6"

# M9c: OpenBSD's ipsecctl(8) loads a static ESP tunnel through PF_KEY and reads it back.
# Logged in as root (`smoke-login`'s sends), the session writes the keys and an ipsec.conf(5)
# (a flow between 10.77.1.0/24 and 10.77.2.0/24 through the peer 192.168.77.2, and the SA
# pair, hmac-sha2-256 and aes), loads it with `ipsecctl -f` (SADB_X_ADDFLOW and SADB_ADD) and
# lists it with `ipsecctl -sa` (the net.key SPD and SADB dumps through sysctl(2)). One VM:
# nothing is sent through the tunnel (`smoke-esp` does that). The files are in /tmp, named
# relative to it (an unquoted ipsec.conf word cannot hold a `/`), under umask 077: ipsecctl
# refuses a configuration file others can read.
smoke-ipsec: (build-amd64 "--features qemu,multiprocessor") (build-arm64 "--features qemu,multiprocessor")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-ipsec: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen \
        --send-after "login:" --send 'root\n' --send-after "Password:" --send 'emibsd\n' \
        --send-after "# " --send 'cd /tmp; umask 077\n' \
        {{esp_keys}} \
        --send-after "# " --send 'echo flow esp from 10.77.1.0/24 to 10.77.2.0/24 peer 192.168.77.2 >ipsec.conf\n' \
        {{esp_sa}} \
        --send-after "# " --send 'ipsecctl -f ipsec.conf\n' --send-after "# " --send 'ipsecctl -sa\n' \
        --expect "rc: multi-user" --expect "FLOWS:" \
        --expect "flow esp out from 10.77.1.0/24 to 10.77.2.0/24 peer 192.168.77.2" \
        --expect "esp tunnel from 192.168.77.1 to 192.168.77.2 spi 0x00001001 auth hmac-sha2-256 enc aes" \
        --expect "esp tunnel from 192.168.77.2 to 192.168.77.1 spi 0x00001002 auth hmac-sha2-256 enc aes"
    cargo xtask smoke {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --expect-ramdisk --until-seen \
        --send-after "login:" --send 'root\n' --send-after "Password:" --send 'emibsd\n' \
        --send-after "# " --send 'cd /tmp; umask 077\n' \
        {{esp_keys}} \
        --send-after "# " --send 'echo flow esp from 10.77.1.0/24 to 10.77.2.0/24 peer 192.168.77.2 >ipsec.conf\n' \
        {{esp_sa}} \
        --send-after "# " --send 'ipsecctl -f ipsec.conf\n' --send-after "# " --send 'ipsecctl -sa\n' \
        --expect "rc: multi-user" --expect "FLOWS:" \
        --expect "flow esp out from 10.77.1.0/24 to 10.77.2.0/24 peer 192.168.77.2" \
        --expect "esp tunnel from 192.168.77.1 to 192.168.77.2 spi 0x00001001 auth hmac-sha2-256 enc aes" \
        --expect "esp tunnel from 192.168.77.2 to 192.168.77.1 spi 0x00001002 auth hmac-sha2-256 enc aes"

# M9c: an ESP tunnel between two VMs (`cargo xtask smoke2`, `smoke-link`'s private link).
# A is 192.168.77.1 on vio1 with 10.77.1.1 on lo1, B is 192.168.77.2 with 10.77.2.1; each
# loads `smoke-ipsec`'s SA pair and its flow between 10.77.1.0/24 and 10.77.2.0/24 with
# ipsecctl(8), pings the other end of the link, then each pings the other's inner address
# from its own: the echoes and their replies go through ESP in tunnel mode
# (ipsp_process_packet, ipip_output, esp_output; esp_input, ipip_input on the other side).
# Both are plain hosts (no net.inet.ip.forwarding): with bpf(4) configured, ipsec_input
# moves a decapsulated packet to enc0, so its inner address on lo1 is not "the wrong
# interface". Each VM ends with `ipsecctl -sa -v` (the SA counters). Part of `smoke`.
smoke-esp: (build-amd64 "--features qemu,multiprocessor") (build-arm64 "--features qemu,multiprocessor")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-esp: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke2 {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd \
        {{esp_both}} \
        --a-send-after "# " --a-send 'ifconfig vio1 inet 192.168.77.1/24\n' \
        --a-send-after "# " --a-send 'ifconfig lo1 create; ifconfig lo1 inet 10.77.1.1/32\n' \
        {{esp_a}} \
        --b-send-after "# " --b-send 'ifconfig vio1 inet 192.168.77.2/24\n' \
        --b-send-after "# " --b-send 'ifconfig lo1 create; ifconfig lo1 inet 10.77.2.1/32\n' \
        {{esp_b}} \
        --a-expect "bytes from 192.168.77.2" --a-expect "bytes from 10.77.2.1" \
        --b-expect "bytes from 192.168.77.1" --b-expect "bytes from 10.77.1.1"
    cargo xtask smoke2 {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd \
        {{esp_both}} \
        --a-send-after "# " --a-send 'ifconfig vio1 inet 192.168.77.1/24\n' \
        --a-send-after "# " --a-send 'ifconfig lo1 create; ifconfig lo1 inet 10.77.1.1/32\n' \
        {{esp_a}} \
        --b-send-after "# " --b-send 'ifconfig vio1 inet 192.168.77.2/24\n' \
        --b-send-after "# " --b-send 'ifconfig lo1 create; ifconfig lo1 inet 10.77.2.1/32\n' \
        {{esp_b}} \
        --a-expect "bytes from 192.168.77.2" --a-expect "bytes from 10.77.2.1" \
        --b-expect "bytes from 192.168.77.1" --b-expect "bytes from 10.77.1.1"

# M9 (pfsync/pflow, the user's decision of 2026-10-03): pfsync(4) and pflow(4) between the two
# VMs of `smoke-link`. Both bring up pfsync0 on vio1 (the 224.0.0.240 group, IPPROTO_PFSYNC);
# A passes with `keep state (pflow)` and has pflow0 send IPFIX to B's UDP port 9995, B passes
# without state except UDP to 9995. A pings B: B polls `pfctl -ss` until A's ICMP state shows
# up, synced over pfsync (B keeps no ICMP state of its own). A then clears its states (the
# flows are exported, pfsync tells B to clear them too) and flushes pflow0 (`pflowproto 10`
# again); B polls until its own state for A's datagrams to 9995 shows up: the flow records
# (or the templates, sent at start and every 30 seconds) arrived. B's polls go through a short
# shell function: a long typed line can lose characters on the busy arm64 VM's serial input.
# The patterns use `?` for the spaces and `<` so that the typed commands do not match the
# expected lines. Part of `smoke`.
smoke-pfsync: (build-amd64 "--features qemu,multiprocessor") (build-arm64 "--features qemu,multiprocessor")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-pfsync: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke2 {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --timeout 400 \
        {{pfsync_both}} {{pfsync_a}} {{pfsync_b}} {{pfsync_expect}}
    cargo xtask smoke2 {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --timeout 400 \
        {{pfsync_both}} {{pfsync_a}} {{pfsync_b}} {{pfsync_expect}}

# `smoke-pfsync`'s sends and expectations.
pfsync_both := "--both-send-after 'login:' --both-send 'root\\n' --both-send-after 'Password:' --both-send 'emibsd\\n'"
pfsync_a := "--a-send-after '# ' --a-send 'ifconfig vio1 inet 192.168.77.1/24 up\\n' " + \
    "--a-send-after '# ' --a-send 'ifconfig pfsync0 create syncdev vio1 up\\n' " + \
    "--a-send-after '# ' --a-send 'ifconfig pflow0 create flowsrc 192.168.77.1 flowdst 192.168.77.2:9995 pflowproto 10\\n' " + \
    "--a-send-after '# ' --a-send 'echo \"pass keep state (pflow)\" | pfctl -e -f -\\n' " + \
    "--a-send-after '# ' --a-send 'sleep 5; ping -c 20 192.168.77.2\\n' " + \
    "--a-send-after 'packet loss' --a-send 'pfctl -ss; pfctl -F states\\n' " + \
    "--a-send-after '# ' --a-send 'ifconfig pflow0 pflowproto 10; ifconfig pflow0; ifconfig pfsync0\\n'"
pfsync_b := "--b-send-after '# ' --b-send 'ifconfig vio1 inet 192.168.77.2/24 up\\n' " + \
    "--b-send-after '# ' --b-send 'ifconfig pfsync0 create syncdev vio1 up\\n' " + \
    "--b-send-after '# ' --b-send 'echo pass no state >/tmp/b.conf\\n' " + \
    "--b-send-after '# ' --b-send 'echo pass in proto udp to port 9995 >>/tmp/b.conf\\n' " + \
    "--b-send-after '# ' --b-send 'pfctl -e -f /tmp/b.conf\\n' " + \
    "--b-send-after '# ' --b-send 'w(){ until case $(pfctl -ss) in *$1*):;;*)false;;esac;do sleep 1;done;}\\n' " + \
    "--b-send-after '# ' --b-send 'w icmp?192.168.77.1:; pfctl -ss; echo pfsync-synced-$((7+7))\\n' " + \
    "--b-send-after 'pfsync-synced-14' --b-send 'w udp?192.168.77.2:9995????192.168.77.1:\\n' " + \
    "--b-send-after '# ' --b-send 'pfctl -ss; echo pflow-seen-$((8+8))\\n'"
pfsync_expect := "--a-expect 'pfsync: syncdev: vio1' " + \
    "--a-expect 'pflow: sender: 192.168.77.1 receiver: 192.168.77.2:9995 version: 10' " + \
    "--b-expect 'pfsync-synced-14' --b-expect 'all icmp 192.168.77.1:' " + \
    "--b-expect 'pflow-seen-16' --b-expect 'all udp 192.168.77.2:9995 <- 192.168.77.1:'"

# `smoke-esp`'s sends: the login and the keys on both VMs, then each VM's ipsec.conf (its
# flow, the SA pair), ipsecctl -f, a ping across the link, the ping through the tunnel and
# the SA counters.
esp_both := "--both-send-after 'login:' --both-send 'root\\n' --both-send-after 'Password:' --both-send 'emibsd\\n' " + \
    "--both-send-after '# ' --both-send 'cd /tmp; umask 077\\n' " + \
    "--both-send-after '# ' --both-send 'k=0123456789abcdef; echo $k$k$k$k >ak; e=fedcba9876543210; echo $e$e >ek\\n'"
esp_a := "--a-send-after '# ' --a-send 'echo flow esp from 10.77.1.0/24 to 10.77.2.0/24 peer 192.168.77.2 >ipsec.conf\\n' " + \
    replace(replace(esp_sa, "--send-after", "--a-send-after"), "--send ", "--a-send ") + \
    " --a-send-after '# ' --a-send 'ipsecctl -f ipsec.conf\\n' --a-send-after '# ' --a-send 'ping -c 2 192.168.77.2\\n' " + \
    "--a-send-after '# ' --a-send 'ping -c 3 -I 10.77.1.1 10.77.2.1\\n' --a-send-after '# ' --a-send 'ipsecctl -sa -v\\n'"
esp_b := "--b-send-after '# ' --b-send 'echo flow esp from 10.77.2.0/24 to 10.77.1.0/24 peer 192.168.77.1 >ipsec.conf\\n' " + \
    replace(replace(esp_sa, "--send-after", "--b-send-after"), "--send ", "--b-send ") + \
    " --b-send-after '# ' --b-send 'ipsecctl -f ipsec.conf\\n' --b-send-after '# ' --b-send 'ping -c 2 192.168.77.1\\n' --b-send-after '# ' --b-send 'ping -c 3 -I 10.77.2.1 10.77.1.1\\n' --b-send-after '# ' --b-send 'ipsecctl -sa -v\\n'"

# The sends that write the keys (files ak and ek) and append the SA pair to ipsec.conf, for
# `smoke-ipsec` and `smoke-esp` (ipsec.conf(5), "MANUAL SECURITY ASSOCIATIONS"): SPI 0x1001
# from 192.168.77.1 to .2, 0x1002 back, the same keys both ways. Every line stays short:
# arm64's pluart drops input past its buffer (`pluart0: ... ibuf overflow`).
esp_keys := "--send-after '# ' --send 'k=0123456789abcdef; echo $k$k$k$k >ak; e=fedcba9876543210; echo $e$e >ek\\n'"
esp_sa := "--send-after '# ' --send 'a=\"esp tunnel from 192.168.77.1 to 192.168.77.2\"\\n' --send-after '# ' --send 'b=\"spi 0x1001:0x1002 auth hmac-sha2-256 enc aes\"\\n' --send-after '# ' --send 'echo $a $b authkey file ak:ak enckey file ek:ek >>ipsec.conf\\n'"

# M9+: `smoke-esp` with IPComp. Each VM enables net.inet.ipcomp.enable and loads an `ipcomp`
# flow between the inner networks with a bundle (ipsec.conf(5), `bundle`) of an IPComp SA in
# tunnel mode (CPI 0x2001 from 192.168.77.1 to .2, 0x2002 back, `comp deflate`) and an ESP SA
# in transport mode (smoke-esp's SPIs and keys): a packet is tunnelled (ipip_output),
# compressed (ipcomp_output through cryptosoft's deflate) and then encrypted (esp_output);
# the other VM decrypts, decompresses and decapsulates it. The pings to the inner addresses
# carry 1000 bytes, above comp_algo_deflate's 90-byte minimum and compressible (ping(8)
# fills them with a byte ramp). Each VM ends with `ipsecctl -sa`, which lists the IPComp SAs.
# Part of `smoke`.
smoke-ipcomp: (build-amd64 "--features qemu,multiprocessor") (build-arm64 "--features qemu,multiprocessor")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-ipcomp: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke2 {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd \
        {{esp_both}} {{ipcomp_both}} {{ipcomp_a}} {{ipcomp_b}} \
        --a-expect "bytes from 192.168.77.2" --a-expect "1008 bytes from 10.77.2.1" \
        --a-expect "ipcomp tunnel from 192.168.77.2 to 192.168.77.1 spi 0x00002002 comp deflate" \
        --b-expect "bytes from 192.168.77.1" --b-expect "1008 bytes from 10.77.1.1" \
        --b-expect "ipcomp tunnel from 192.168.77.1 to 192.168.77.2 spi 0x00002001 comp deflate"
    cargo xtask smoke2 {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd \
        {{esp_both}} {{ipcomp_both}} {{ipcomp_a}} {{ipcomp_b}} \
        --a-expect "bytes from 192.168.77.2" --a-expect "1008 bytes from 10.77.2.1" \
        --a-expect "ipcomp tunnel from 192.168.77.2 to 192.168.77.1 spi 0x00002002 comp deflate" \
        --b-expect "bytes from 192.168.77.1" --b-expect "1008 bytes from 10.77.1.1" \
        --b-expect "ipcomp tunnel from 192.168.77.1 to 192.168.77.2 spi 0x00002001 comp deflate"

# `smoke-ipcomp`'s sends: IPComp on, then each VM's addresses, its ipcomp flow, the SA
# bundle (ipcomp_sa), ipsecctl -f, the pings and the SAs. Short lines, as for esp_sa.
ipcomp_both := "--both-send-after '# ' --both-send 'sysctl net.inet.ipcomp.enable=1\\n'"
ipcomp_a := "--a-send-after '# ' --a-send 'ifconfig vio1 inet 192.168.77.1/24\\n' " + \
    "--a-send-after '# ' --a-send 'ifconfig lo1 create; ifconfig lo1 inet 10.77.1.1/32\\n' " + \
    "--a-send-after '# ' --a-send 'echo flow ipcomp from 10.77.1.0/24 to 10.77.2.0/24 peer 192.168.77.2 >ipsec.conf\\n' " + \
    replace(replace(ipcomp_sa, "--send-after", "--a-send-after"), "--send ", "--a-send ") + \
    " --a-send-after '# ' --a-send 'ipsecctl -f ipsec.conf\\n' --a-send-after '# ' --a-send 'ping -c 2 192.168.77.2\\n' " + \
    "--a-send-after '# ' --a-send 'ping -c 3 -s 1000 -I 10.77.1.1 10.77.2.1\\n' --a-send-after '# ' --a-send 'ipsecctl -sa\\n'"
ipcomp_b := "--b-send-after '# ' --b-send 'ifconfig vio1 inet 192.168.77.2/24\\n' " + \
    "--b-send-after '# ' --b-send 'ifconfig lo1 create; ifconfig lo1 inet 10.77.2.1/32\\n' " + \
    "--b-send-after '# ' --b-send 'echo flow ipcomp from 10.77.2.0/24 to 10.77.1.0/24 peer 192.168.77.1 >ipsec.conf\\n' " + \
    replace(replace(ipcomp_sa, "--send-after", "--b-send-after"), "--send ", "--b-send ") + \
    " --b-send-after '# ' --b-send 'ipsecctl -f ipsec.conf\\n' --b-send-after '# ' --b-send 'ping -c 2 192.168.77.1\\n' " + \
    "--b-send-after '# ' --b-send 'ping -c 3 -s 1000 -I 10.77.2.1 10.77.1.1\\n' --b-send-after '# ' --b-send 'ipsecctl -sa\\n'"
ipcomp_sa := "--send-after '# ' --send 'c=\"ipcomp tunnel from 192.168.77.1 to 192.168.77.2\"\\n' --send-after '# ' --send 'echo $c spi 0x2001:0x2002 comp deflate bundle x >>ipsec.conf\\n' " + \
    "--send-after '# ' --send 'a=\"esp transport from 192.168.77.1 to 192.168.77.2\"\\n' --send-after '# ' --send 'b=\"spi 0x1001:0x1002 auth hmac-sha2-256 enc aes\"\\n' " + \
    "--send-after '# ' --send 'echo $a $b authkey file ak:ak enckey file ek:ek bundle x >>ipsec.conf\\n'"

# M9+: TCP between the two VMs of `smoke-link`, with OpenBSD's nc(1), three ways: directly on
# vio1, through wg0 (`smoke-wg`'s interfaces and keys) and through the ESP tunnel
# (`smoke-esp`'s flows, SAs and inner addresses on lo1). B listens with `nc -l` on one address
# and port per path, one after the other; A's `t` sends a line with `nc -N` (shut down after
# stdin's EOF) and retries every second until B's listener takes it (`-w 5` bounds a connect
# that gets no answer while wg handshakes). B's nc prints the line and exits on A's FIN.
# The markers are built with `$((3+4))`, so that the typed commands do not match them. First
# B lists a listening TCP socket with fstat(1) (kern.file's tcbtable and tcpcb fields). Part
# of `smoke`.
smoke-tcp: (build-amd64 "--features qemu,multiprocessor") (build-arm64 "--features qemu,multiprocessor")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-tcp: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke2 {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --timeout 300 \
        {{esp_both}} {{tcp_a}} {{tcp_b}} {{tcp_expect}}
    cargo xtask smoke2 {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --timeout 300 \
        {{esp_both}} {{tcp_a}} {{tcp_b}} {{tcp_expect}}

# `smoke-tcp`'s sends after `esp_both` (the login and the ESP keys): each VM's vio1, wg0, lo1
# and ipsec.conf (`esp_sa`), then the transfers.
tcp_a := "--a-send-after '# ' --a-send 'ifconfig vio1 inet 192.168.77.1/24 up\\n' " + \
    "--a-send-after '# ' --a-send 'ifconfig wg0 create wgport 51820 wgkey dwdtCnMYpX08FsFyUbJmRd9ML4frwJkqsXf7pR25LCo=\\n' " + \
    "--a-send-after '# ' --a-send 'ifconfig wg0 wgpeer 3p7bfXt9wbTTW2HC7OQ1Nz+DQ8hbeGdNrfx+FG+IK08= wgendpoint 192.168.77.2 51820 wgaip 10.77.0.2/32\\n' " + \
    "--a-send-after '# ' --a-send 'ifconfig wg0 inet 10.77.0.1/24 up\\n' " + \
    "--a-send-after '# ' --a-send 'ifconfig lo1 create; ifconfig lo1 inet 10.77.1.1/32\\n' " + \
    "--a-send-after '# ' --a-send 'echo flow esp from 10.77.1.0/24 to 10.77.2.0/24 peer 192.168.77.2 >ipsec.conf\\n' " + \
    replace(replace(esp_sa, "--send-after", "--a-send-after"), "--send ", "--a-send ") + \
    " --a-send-after '# ' --a-send 'ipsecctl -f ipsec.conf\\n' " + \
    "--a-send-after '# ' --a-send 't(){ until echo tcp-$1-$((3+4)) | nc -N -w 5 $4 $2 $3; do sleep 1; done; }\\n' " + \
    "--a-send-after '# ' --a-send 't direct 192.168.77.2 7001\\n' " + \
    "--a-send-after '# ' --a-send 't wg 10.77.0.2 7002\\n' " + \
    "--a-send-after '# ' --a-send 't esp 10.77.2.1 7003 \"-s 10.77.1.1\"\\n' " + \
    "--a-send-after '# ' --a-send 'echo tcp-sent-$((4+4))\\n'"
tcp_b := "--b-send-after '# ' --b-send 'ifconfig vio1 inet 192.168.77.2/24 up\\n' " + \
    "--b-send-after '# ' --b-send 'ifconfig wg0 create wgport 51820 wgkey XasIfmJKikt54X+Lg4AO5m87sSkmGLb9HC+LJ/+I4Os=\\n' " + \
    "--b-send-after '# ' --b-send 'ifconfig wg0 wgpeer hSDwCYkwp1R0i33ctD73Wg2/Og0mOBr066SpjqqbTmo= wgendpoint 192.168.77.1 51820 wgaip 10.77.0.1/32\\n' " + \
    "--b-send-after '# ' --b-send 'ifconfig wg0 inet 10.77.0.2/24 up\\n' " + \
    "--b-send-after '# ' --b-send 'ifconfig lo1 create; ifconfig lo1 inet 10.77.2.1/32\\n' " + \
    "--b-send-after '# ' --b-send 'echo flow esp from 10.77.2.0/24 to 10.77.1.0/24 peer 192.168.77.1 >ipsec.conf\\n' " + \
    replace(replace(esp_sa, "--send-after", "--b-send-after"), "--send ", "--b-send ") + \
    " --b-send-after '# ' --b-send 'ipsecctl -f ipsec.conf\\n' " + \
    "--b-send-after '# ' --b-send 'nc -l 192.168.77.2 7009 </dev/null & sleep 1; fstat -p $!; kill $!\\n' " + \
    "--b-send-after '# ' --b-send 'nc -l 192.168.77.2 7001\\n' " + \
    "--b-send-after '# ' --b-send 'nc -l 10.77.0.2 7002\\n' " + \
    "--b-send-after '# ' --b-send 'nc -l 10.77.2.1 7003\\n'"
tcp_expect := "--b-expect 'internet stream tcp' --b-expect '192.168.77.2:7009' --b-expect 'tcp-direct-7' --b-expect 'tcp-wg-7' --b-expect 'tcp-esp-7' --a-expect 'tcp-sent-8'"

# M11d: the network on the softnet task queues of the MULTIPROCESSOR kernel. Both VMs of
# `smoke-link` boot the MP kernel with `-smp 4` (`smp4`, whatever `ncpu` says). softnet_init makes
# NET_TASKQ (8) softnet queues and softnet_percpu keeps one per CPU, min(8, ncpus) = 4 (net/if.c),
# so ps(1) `-k` (the kern.proc sysctl) lists softnet0..softnet3 (`softnets-4`), each on the CPU it
# last ran on; each interface's work goes to the queue of its index (net_tq), so vio1, wg0 and lo0
# are served by different threads. Then the representative subset of the two-VM smokes: a ping
# across the link (`smoke-link`), a ping through wg0 (`smoke-wg`) and a TCP line with nc(1)
# directly and through wg0 (`smoke-tcp`). A then creates lo3..lo5 (the interface index map grows
# past its first 8 slots; the old map is freed by smr_call) and destroys lo3 (if_idxmap_remove's
# smr_barrier). The transcripts are printed. Last, one VM per arch boots with `-smp 8` and keeps
# all eight softnets (`softnets-8`). Part of `smoke`.
smoke-net-mp: (build-amd64 "--features qemu,multiprocessor") (build-arm64 "--features qemu,multiprocessor")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-net-mp: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke2 {{reject}} {{smp4}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --timeout 300 --show-transcripts \
        {{divert_both}} {{netmp_a}} {{netmp_b}} {{netmp_expect}}
    cargo xtask smoke2 {{reject}} {{smp4}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --timeout 300 --show-transcripts \
        {{divert_both}} {{netmp_a}} {{netmp_b}} {{netmp_expect}}
    cargo xtask smoke {{reject}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --smp 8 --expect-ramdisk --until-seen \
        --send-after "login:" --send 'root\n' --send-after "Password:" --send 'emibsd\n' \
        --send-after "# " --send '{{netmp_count}}' --expect "bsd: 8 processors" --expect "softnets-8"
    cargo xtask smoke {{reject}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --smp 8 --expect-ramdisk --until-seen \
        --send-after "login:" --send 'root\n' --send-after "Password:" --send 'emibsd\n' \
        --send-after "# " --send '{{netmp_count}}' --expect "bsd: 8 processors" --expect "softnets-8"

# `smoke-net-mp`'s sends and expectations (the login is `divert_both`; wg0's keys are
# `smoke-wg`'s, the TCP helper `t` is `smoke-tcp`'s). `netmp_count` counts the softnet
# threads ps(1) lists, in ksh (the ramdisk has no grep).
netmp_count := "n=0;for c in $(ps -axko comm);do [[ $c = softnet? ]]&&((n++));done;echo softnets-$n\\n"
netmp_a := "--a-send-after '# ' --a-send 'ifconfig vio1 inet 192.168.77.1/24 up\\n' " + \
    "--a-send-after '# ' --a-send 'ifconfig wg0 create wgport 51820 wgkey dwdtCnMYpX08FsFyUbJmRd9ML4frwJkqsXf7pR25LCo=\\n' " + \
    "--a-send-after '# ' --a-send 'ifconfig wg0 wgpeer 3p7bfXt9wbTTW2HC7OQ1Nz+DQ8hbeGdNrfx+FG+IK08= wgendpoint 192.168.77.2 51820 wgaip 10.77.0.2/32\\n' " + \
    "--a-send-after '# ' --a-send 'ifconfig wg0 inet 10.77.0.1/24 up\\n' " + \
    "--a-send-after '# ' --a-send 'ping -c 5 192.168.77.2\\n' " + \
    "--a-send-after '# ' --a-send 'ping -c 10 10.77.0.2\\n' " + \
    "--a-send-after '# ' --a-send 't(){ until echo tcp-$1-$((3+4)) | nc -N -w 5 $2 $3; do sleep 1; done; }\\n' " + \
    "--a-send-after '# ' --a-send 't direct 192.168.77.2 7001\\n' " + \
    "--a-send-after '# ' --a-send 't wg 10.77.0.2 7002\\n' " + \
    "--a-send-after '# ' --a-send 'for i in 3 4 5; do ifconfig lo$i create; done; ifconfig lo5\\n' " + \
    "--a-send-after '# ' --a-send 'ifconfig lo3 destroy && echo if-destroyed-$((3+3))\\n' " + \
    "--a-send-after '# ' --a-send 'ps -axk -o pid,cpuid,comm\\n' --a-send-after '# ' --a-send '" + netmp_count + "' " + \
    "--a-send-after '# ' --a-send 'echo tcp-sent-$((4+4))\\n'"
netmp_b := "--b-send-after '# ' --b-send 'ifconfig vio1 inet 192.168.77.2/24 up\\n' " + \
    "--b-send-after '# ' --b-send 'ifconfig wg0 create wgport 51820 wgkey XasIfmJKikt54X+Lg4AO5m87sSkmGLb9HC+LJ/+I4Os=\\n' " + \
    "--b-send-after '# ' --b-send 'ifconfig wg0 wgpeer hSDwCYkwp1R0i33ctD73Wg2/Og0mOBr066SpjqqbTmo= wgendpoint 192.168.77.1 51820 wgaip 10.77.0.1/32\\n' " + \
    "--b-send-after '# ' --b-send 'ifconfig wg0 inet 10.77.0.2/24 up\\n' " + \
    "--b-send-after '# ' --b-send 'ping -c 5 192.168.77.1\\n' " + \
    "--b-send-after '# ' --b-send 'nc -l 192.168.77.2 7001\\n' " + \
    "--b-send-after '# ' --b-send 'nc -l 10.77.0.2 7002\\n' " + \
    "--b-send-after '# ' --b-send 'ps -axk -o pid,cpuid,comm\\n' --b-send-after '# ' --b-send '" + netmp_count + "'"
netmp_expect := "--both-expect 'bsd: 4 processors' --both-expect softnets-4 " + \
    "--a-expect 'bytes from 192.168.77.2: icmp_seq=' --b-expect 'bytes from 192.168.77.1: icmp_seq=' " + \
    "--a-expect 'bytes from 10.77.0.2: icmp_seq=' " + \
    "--b-expect 'tcp-direct-7' --b-expect 'tcp-wg-7' --a-expect 'if-destroyed-6' --a-expect 'tcp-sent-8'"

# M11e: a network stress between the two VMs of `smoke-link`, both on the MULTIPROCESSOR
# kernel (`{{smp}}`): each VM runs a tcpbench(1) server in the background and a
# client of the other's with four connections for 15 seconds (retried every second until the
# other server is up), so TCP runs both ways over eight connections at once. Each client
# prints the per-second `Conn:   4 Mbps:` lines and the summary; the echoed markers follow.
# Part of `smoke`.
smoke-tcpbench: (build-amd64 "--features qemu,multiprocessor") (build-arm64 "--features qemu,multiprocessor")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-tcpbench: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke2 {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --timeout 300 {{tcpbench_steps}}
    cargo xtask smoke2 {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --timeout 300 {{tcpbench_steps}}

# `smoke-tcpbench`'s session.
tcpbench_steps := "--both-send-after 'login:' --both-send 'root\\n' --both-send-after 'Password:' --both-send 'emibsd\\n' " + \
    "--a-send-after '# ' --a-send 'ifconfig vio1 inet 192.168.77.1/24 up\\n' " + \
    "--b-send-after '# ' --b-send 'ifconfig vio1 inet 192.168.77.2/24 up\\n' " + \
    "--both-send-after '# ' --both-send 'tcpbench -s >/dev/null 2>&1 &\\n' " + \
    "--a-send-after '# ' --a-send 'until tcpbench -n 4 -t 15 192.168.77.2; do sleep 1; done; echo bench-a-$((5+5))\\n' " + \
    "--b-send-after '# ' --b-send 'until tcpbench -n 4 -t 15 192.168.77.1; do sleep 1; done; echo bench-b-$((5+5))\\n' " + \
    "--a-expect 'Conn:   4 Mbps:' --a-expect '--- 192.168.77.2 tcpbench statistics ---' " + \
    "--a-expect 'bytes sent over' --a-expect 'bandwidth min/avg/max/std-dev = ' --a-expect 'bench-a-10' " + \
    "--b-expect 'Conn:   4 Mbps:' --b-expect '--- 192.168.77.1 tcpbench statistics ---' " + \
    "--b-expect 'bytes sent over' --b-expect 'bandwidth min/avg/max/std-dev = ' --b-expect 'bench-b-10'"

# M9+: pf's divert-to between the two VMs of `smoke-link`. B gives lo0 its 127.0.0.1 (as
# netstart(8) would), loads a rule that diverts TCP to its port 80 arriving on vio1 to
# 127.0.0.1 port 8080 (pf.conf(5), `divert-to`) and listens there with `nc -l`; nothing
# listens on port 80. A writes a line to B's port 80 with
# `nc -N`, retrying every second until B's listener is up: pf_test marks the packet
# PF_DIVERT and tcp_input finds the listener through in_pcblookup_listen's divert lookup.
# Part of `smoke`.
smoke-divert: (build-amd64 "--features qemu,multiprocessor") (build-arm64 "--features qemu,multiprocessor")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-divert: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke2 {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd \
        {{divert_both}} {{divert_a}} {{divert_b}} {{divert_expect}}
    cargo xtask smoke2 {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd \
        {{divert_both}} {{divert_a}} {{divert_b}} {{divert_expect}}

# `smoke-divert`'s sends and expectations.
divert_both := "--both-send-after 'login:' --both-send 'root\\n' --both-send-after 'Password:' --both-send 'emibsd\\n'"
divert_a := "--a-send-after '# ' --a-send 'ifconfig vio1 inet 192.168.77.1/24 up\\n' " + \
    "--a-send-after '# ' --a-send 'until echo divert-$((3+4)) | nc -N -w 5 192.168.77.2 80; do sleep 1; done\\n' " + \
    "--a-send-after '# ' --a-send 'echo divert-sent-$((4+4))\\n'"
divert_b := "--b-send-after '# ' --b-send 'ifconfig vio1 inet 192.168.77.2/24 up; ifconfig lo0 inet 127.0.0.1/8\\n' " + \
    "--b-send-after '# ' --b-send 'r=\"pass in on vio1 inet proto tcp to port 80\"\\n' " + \
    "--b-send-after '# ' --b-send 'echo \"$r divert-to 127.0.0.1 port 8080\" | pfctl -e -f -\\n' " + \
    "--b-send-after '# ' --b-send 'pfctl -sr\\n' " + \
    "--b-send-after '# ' --b-send 'nc -l 127.0.0.1 8080\\n'"
divert_expect := "--b-expect 'proto tcp from any to any port = 80' --b-expect 'divert-7' --a-expect 'divert-sent-8'"

# M9+: tcpdump(8) between the two VMs of `smoke-link`. A sends a TCP SYN to B's ports 7001
# and 7002 every second with `nc -z` (nothing listens; B answers with a reset). B captures
# one SYN to 7001 on vio1 (`tcpdump -n -l -c 1 -i vio1`), then loads a pf rule that blocks
# and logs TCP to 7002 and shows one packet it dropped on pflog0 (`tcpdump -n -e -ttt -i
# pflog0 -c 1`: the rule number and `block`). tcpdump runs privilege-separated: the
# unprivileged half is chrooted to /var/empty as _tcpdump. Part of `smoke`.
smoke-tcpdump: (build-amd64 "--features qemu,multiprocessor") (build-arm64 "--features qemu,multiprocessor")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-tcpdump: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke2 {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --timeout 300 \
        {{tcpdump_both}} {{tcpdump_a}} {{tcpdump_b}} {{tcpdump_expect}}
    cargo xtask smoke2 {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --timeout 300 \
        {{tcpdump_both}} {{tcpdump_a}} {{tcpdump_b}} {{tcpdump_expect}}

# `smoke-tcpdump`'s sends and expectations.
tcpdump_both := "--both-send-after 'login:' --both-send 'root\\n' --both-send-after 'Password:' --both-send 'emibsd\\n'"
tcpdump_a := "--a-send-after '# ' --a-send 'ifconfig vio1 inet 192.168.77.1/24 up\\n' " + \
    "--a-send-after '# ' --a-send 'p(){ nc -z -w 1 192.168.77.2 $1; }\\n' " + \
    "--a-send-after '# ' --a-send 'i=0; while [ $i -lt 30 ]; do p 7001; p 7002; sleep 1; i=$((i+1)); done\\n' " + \
    "--a-send-after '# ' --a-send 'echo syn-done-$((4+4))\\n'"
tcpdump_b := "--b-send-after '# ' --b-send 'ifconfig vio1 inet 192.168.77.2/24 up\\n' " + \
    "--b-send-after '# ' --b-send 'tcpdump -n -l -c 1 -i vio1 tcp and port 7001\\n' " + \
    "--b-send-after '# ' --b-send 'ifconfig pflog0 create; ifconfig pflog0 up\\n' " + \
    "--b-send-after '# ' --b-send 'r=\"block log quick on vio1 proto tcp to port 7002\"\\n' " + \
    "--b-send-after '# ' --b-send 'echo $r | pfctl -e -f -\\n' " + \
    "--b-send-after '# ' --b-send 'tcpdump -n -e -ttt -i pflog0 -c 1\\n' " + \
    "--b-send-after '# ' --b-send 'echo pflog-done-$((5+5))\\n'"
tcpdump_expect := "--b-expect '192.168.77.2.7001: S ' --b-expect 'block in on vio1: 192.168.77.1.' --b-expect 'pflog-done-10'"

# M10a: the persistent disk. Boot 1 (`--disk-fresh`, a zeroed 64 MiB image) finds sd0 on
# vioblk(4)'s scsibus, runs fdisk(8), disklabel(8)'s automatic layout and newfs(8) on it,
# writes a file on sd0a and unmounts it; boot 2 runs on the same image: fsck(8) -n must find
# the file system clean (by its clean flag, then forced with -f) and the file must read back.
# fdisk's MBR template (`-f`) is the blank disk's own first sector: amd64's fdisk otherwise
# reads boot(8)'s /usr/mdec/mbr, which comes with M14. Part of `smoke`.
smoke-disk: (build-amd64 "--features qemu,multiprocessor") (build-arm64 "--features qemu,multiprocessor")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-disk: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen \
        --disk-fresh {{disk_make}} --expect 'vioblk0 at virtio1'
    cargo xtask smoke {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen \
        {{disk_check}}
    cargo xtask smoke {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --expect-ramdisk --until-seen \
        --disk-fresh {{disk_make}} --expect 'vioblk0 at virtio29'
    cargo xtask smoke {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --expect-ramdisk --until-seen \
        {{disk_check}}

# `smoke-disk`'s two boots.
disk_login := "--send-after 'login:' --send 'root\\n' --send-after 'Password:' --send 'emibsd\\n'"
disk_make := disk_login + " " + \
    "--send-after '# ' --send 'fdisk -iy -f /dev/rsd0c sd0 && fdisk -f /dev/rsd0c sd0\\n' " + \
    "--send-after '# ' --send 'disklabel -w -A sd0 && disklabel sd0\\n' " + \
    "--send-after '# ' --send 'newfs sd0a\\n' " + \
    "--send-after '# ' --send 'mount /dev/sd0a /mnt && echo m10a-persistent-$((40+2)) >/mnt/m10a.txt && umount /mnt && echo disk-written-$((40+2))\\n' " + \
    "--expect 'scsibus0 at vioblk0' --expect 'sd0 at scsibus0 targ 0 lun 0: <VirtIO, Block Device, >' " + \
    "--expect 'sd0: 64MB, 512 bytes/sector, 131072 sectors' --expect '*3: A6' " + \
    "--expect 'boundstart: 64' --expect '131008               64  4.2BSD' " + \
    "--expect '/dev/rsd0a: ' --expect 'disk-written-42'"
disk_check := disk_login + " " + \
    "--send-after '# ' --send 'fsck -n /dev/sd0a; echo fsck-rc=$?\\n' " + \
    "--send-after '# ' --send 'fsck -fn /dev/sd0a; echo fsck-f-rc=$?\\n' " + \
    "--send-after '# ' --send 'mount -r /dev/sd0a /mnt && cat /mnt/m10a.txt\\n' " + \
    "--expect 'sd0 at scsibus0 targ 0 lun 0' --expect '** /dev/rsd0a (NO WRITE)' " + \
    "--expect '** File system is clean; not checking' --expect 'fsck-rc=0' " + \
    "--expect '** Phase 5 - Check Cyl groups' --expect 'fsck-f-rc=0' --expect 'm10a-persistent-42' " + \
    "--reject 'UNEXPECTED' --reject 'FILE SYSTEM WAS MODIFIED'"

# M10b: the UFS options, on sd0a. Boot 1 (`--disk-fresh`) makes the file system as
# `smoke-disk` does, mounts it through its fstab(5) line (`userquota`), runs quotacheck(8) and
# quotaon(8), gives `daemon` a 50/100 KB block quota with edquota(8) (the "editor" copies a
# prepared file over edquota's), and has su(1) write a 220 KB file as `daemon`: the write
# must fail with EDQUOT, and repquota(8) and quota(1) show the user over the soft limit
# (`+-`). Boot 2 reuses the disk: quotas come back on with the usage kept, a directory of
# 5,000 entries gets hashed (`vfs.ffs.dirhash_mem` > 0) and every name looks up, and
# mount_mfs(8) mounts a memory file system on which a file reads back. Part of `smoke`.
smoke-ufsopts: (build-amd64 "--features qemu,multiprocessor") (build-arm64 "--features qemu,multiprocessor")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-ufsopts: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen \
        --disk-fresh {{ufsopts_quota}}
    cargo xtask smoke {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen \
        {{ufsopts_dirhash_mfs}}
    cargo xtask smoke {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --expect-ramdisk --until-seen \
        --disk-fresh {{ufsopts_quota}}
    cargo xtask smoke {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --expect-ramdisk --until-seen \
        {{ufsopts_dirhash_mfs}}

# `smoke-ufsopts`'s two boots.
ufsopts_quota := disk_login + " " + \
    "--send-after '# ' --send 'fdisk -iy -f /dev/rsd0c sd0 && disklabel -w -A sd0 && newfs -q sd0a && echo newfs-$((40+2))\\n' " + \
    "--send-after 'newfs-42' --send 'mount /mnt && mkdir /mnt/q && chown daemon /mnt/q && quotacheck -u /mnt && quotaon -v -u /mnt\\n' " + \
    "--send-after 'quotas turned on' --send 'print \"Quotas for user daemon:\" >/tmp/q; print \"/mnt: KBytes in use: 2, limits (soft = 50, hard = 100)\" >>/tmp/q\\n' " + \
    "--send-after '# ' --send 'print \"inodes in use: 1, limits (soft = 0, hard = 0)\" >>/tmp/q; EDITOR=\"cat /tmp/q >\" edquota -u daemon && echo edquota-$((40+2))\\n' " + \
    "--send-after 'edquota-42' --send 'su -s /bin/ksh daemon -c \"cat /sbin/newfs >/mnt/q/big\"; echo su-rc-$?\\n' " + \
    "--send-after '# ' --send 'repquota /mnt; quota -u daemon\\n' " + \
    "--send-after '# ' --send 'quotaoff -v -u /mnt && umount /mnt && echo quota-done-$((40+2))\\n' " + \
    "--expect '/mnt: user quotas turned on' --expect 'edquota-42' " + \
    "--expect '/mnt: warning, user disk quota exceeded' --expect '/mnt: write failed, user disk limit reached' --expect 'cat: stdout: Disk quota exceeded' --expect 'su-rc-1' " + \
    "--expect 'User            used    soft    hard  grace' --expect 'daemon    +-      98      50     100  7days' --reject 'cannot change current allocation' " + \
    "--expect 'Disk quotas for user daemon (uid 1):' --expect '/mnt      98*      50     100   7days' " + \
    "--expect '/mnt: user quotas turned off' --expect 'quota-done-42'"
ufsopts_dirhash_mfs := disk_login + " " + \
    "--send-after '# ' --send 'mount /mnt && quotaon -v -u /mnt && repquota /mnt\\n' " + \
    "--send-after '# ' --send 'mkdir /mnt/d && i=0 && while [ $i -lt 5000 ]; do : >/mnt/d/f$i; i=$((i+1)); done; echo made-$i\\n' " + \
    "--send-after 'made-5000' --send 'n=0; i=0; while [ $i -lt 5000 ]; do [ -f /mnt/d/f$i ] && n=$((n+1)); i=$((i+1)); done; echo found-$n\\n' " + \
    "--send-after '# ' --send 'sysctl vfs.ffs.dirhash_mem; [ $(sysctl -n vfs.ffs.dirhash_mem) -gt 0 ] && echo dirhash-used-$((40+2))\\n' " + \
    "--send-after '# ' --send 'mkdir -p /mfs && mount_mfs -s 8m swap /mfs && echo m10b-mfs-$((40+2)) >/mfs/f && cat /mfs/f && df /mfs\\n' " + \
    "--send-after '# ' --send 'umount /mfs && quotaoff -u /mnt && umount /mnt && echo ufsopts-done-$((40+2))\\n' " + \
    "--expect '/mnt: user quotas turned on' --expect 'daemon    +-      98      50     100' " + \
    "--expect 'made-5000' --expect 'found-5000' --expect 'vfs.ffs.dirhash_mem=' --expect 'dirhash-used-42' " + \
    "--expect 'm10b-mfs-42' --expect 'mfs:' --expect 'ufsopts-done-42'"

# M10f: softraid(4) over four persistent vioblk disks (`--disks 4`). Boot 1 (`--disk-fresh`)
# gives sd0..sd3 an MBR and four 14 MB RAID partitions each (disklabel(8)'s `-T` table of
# `raid` lines: a, b, d, e), then creates one volume per discipline: RAID 0, 1, 5, concat,
# RAID 1C and CRYPTO with bioctl(8) (the last two keyed from a root-owned passphrase file with
# `-p`), and RAID 6 with our own sr6create (tools/sr6create: bioctl refuses `-c 6`,
# "unsupported RAID level"); it puts an ffs on each and writes a file naming it. Boot 2
# reuses the disks: the kernel assembles the five unencrypted volumes at boot
# (sr_boot_assembly), `-p` unlocks the two encrypted ones, and every file reads back. Boot 3
# runs with sd3 missing (`--disks 3`): the RAID 1 volume (sd2a, sd3a) and the RAID 6 volume
# (sd0d..sd3d) are assembled degraded and their files still read; no chunk may come up
# under another chunk's metadata (`roaming device`). The volumes' sd units differ per arch
# (arm64's boot disk is a vioblk too: sd4), so the scripts find them in `hw.disknames`. The
# command lines stay short (helper functions): arm64's console drops input past about 128
# bytes (`pluart0: ... ibuf overflows`). The disks are a set of their own (`--disk-set
# softraid`): RAID metadata left on the default disk would have softraid assemble volumes, with
# threads of their own, in every later boot (the kthread self-test counts threads). Part of
# `smoke`.
smoke-softraid: (build-amd64 "--features qemu,multiprocessor") (build-arm64 "--features qemu,multiprocessor")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-softraid: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke {{reject}} {{smp4}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen \
        --disks 4 --disk-set softraid --disk-fresh {{softraid_make}}
    cargo xtask smoke {{reject}} {{smp4}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen \
        --disks 4 --disk-set softraid {{softraid_check}}
    cargo xtask smoke {{reject}} {{smp4}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen \
        --disks 3 --disk-set softraid {{softraid_degraded}}
    cargo xtask smoke {{reject}} {{smp4}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --expect-ramdisk --until-seen \
        --disks 4 --disk-set softraid --disk-fresh {{softraid_make}}
    cargo xtask smoke {{reject}} {{smp4}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --expect-ramdisk --until-seen \
        --disks 4 --disk-set softraid {{softraid_check}}
    cargo xtask smoke {{reject}} {{smp4}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --expect-ramdisk --until-seen \
        --disks 3 --disk-set softraid {{softraid_degraded}}

# `smoke-softraid`'s three boots. `sr_pass` writes the passphrase file (the ramdisk root is
# rebuilt every boot); `sr_cat` mounts every disk's `a` partition read-only and prints its
# file (the RAID chunks and arm64's FAT boot disk do not mount, silently). `sr_mk` defines
# `mk name bioctl-args...`: create the volume, label it, newfs, write m10f.txt; `m6` is the
# same through sr6create, for RAID 6 on the four `d` partitions. `f` zeroes the volume's first
# megabyte first, as bioctl(8) and softraid(4) say to: a new CRYPTO or RAID 1C volume reads as
# random data, and `fdisk -i -f /dev/rsdNc` takes its own sector 0 as the MBR template (there
# is no /usr/mdec/mbr in the ramdisk), so a random partition 0 of type A6 became the OpenBSD
# partition, bounds far past the end, and `disklabel -A` failed. Boot 1 rejects
# `disklabels not read: ` with its space, i.e. setroot naming a disk whose label is unread.
# The bare header, with no disk after it, is setroot counting wakeups: it sleeps once per disk
# still pending, at most five times, and each finished label read wakes it early, so when five
# reads end during its wait (arm64 has five vioblk disks; seen with the host loaded by parallel
# smokes) it prints the header over an empty list, as OpenBSD's subr_disk.c does.
sr_pass := "--send-after '# ' --send 'print emibsd-m10f-passphrase >/etc/m10f.pass\\n' " + \
    "--send-after '# ' --send 'chmod 600 /etc/m10f.pass && echo pass-$((40+2))\\n' "
sr_cat := "--send-after '# ' --send 'c() { mount -r /dev/$1a /mnt 2>/dev/null && echo \"$1: $(cat /mnt/m10f.txt)\" && umount /mnt; }\\n' " + \
    "--send-after '# ' --send 'sr_cat() { IFS=,; for e in $(sysctl -n hw.disknames); do c ${e%%:*}; done; IFS=\" \"; echo cat-done-$((40+2)); }\\n' "
sr_mk := "--send-after '# ' --send 'h() { echo m10f-$1-$((40+2)) >/mnt/m10f.txt && umount /mnt; }\\n' " + \
    "--send-after '# ' --send 'g() { newfs -q $1a && mount /dev/$1a /mnt && h $2 && echo made-$2-$((40+2)); }\\n' " + \
    "--send-after '# ' --send 'f() { dd if=/dev/zero of=/dev/r$1c bs=1m count=1 2>/dev/null && fdisk -iy -f /dev/r$1c $1 >/dev/null && disklabel -w -A $1 && g $1 $2; }\\n' " + \
    "--send-after '# ' --send 'mk() { n=$1; shift; o=$(bioctl \"$@\" softraid0) && echo \"$o\" && f ${o##* } $n; }\\n' " + \
    "--send-after '# ' --send 'm6() { o=$(sr6create \"$@\" softraid0) && echo \"$o\" && f ${o##* } raid6; }\\n' "
softraid_make := disk_login + " " + sr_pass + \
    "--send-after '# ' --send 'for i in 1 2 3 4; do echo raid 14M; done >/tmp/t\\n' " + \
    "--send-after '# ' --send 'l() { fdisk -iy -f /dev/r$1c $1 >/dev/null && disklabel -w -A -T /tmp/t $1; }\\n' " + \
    "--send-after '# ' --send 'for d in sd0 sd1 sd2 sd3; do l $d || echo label-fail$((0))ed-$d; done\\n' " + \
    "--send-after '# ' --send 'disklabel sd3; echo labels-$((40+2))\\n' " + sr_mk + \
    "--send-after '# ' --send 'mk raid0 -c 0 -l /dev/sd0a,/dev/sd1a\\n' " + \
    "--send-after 'made-raid0-42' --send 'mk raid1 -c 1 -l /dev/sd2a,/dev/sd3a\\n' " + \
    "--send-after 'made-raid1-42' --send 'mk raid5 -c 5 -l /dev/sd0b,/dev/sd1b,/dev/sd2b\\n' " + \
    "--send-after 'made-raid5-42' --send 'mk concat -c c -l /dev/sd0e,/dev/sd1e\\n' " + \
    "--send-after 'made-concat-42' --send 'm6 -l /dev/sd0d,/dev/sd1d,/dev/sd2d,/dev/sd3d\\n' " + \
    "--send-after 'made-raid6-42' --send 'mk raid1c -c 1C -r 16 -p /etc/m10f.pass -l /dev/sd2e,/dev/sd3e\\n' " + \
    "--send-after 'made-raid1c-42' --send 'mk crypto -c C -r 16 -p /etc/m10f.pass -l /dev/sd3b\\n' " + \
    "--send-after 'made-crypto-42' --send 'bioctl softraid0; echo bioctl-$((40+2))\\n' " + \
    "--expect 'sd3 at scsibus3 targ 0 lun 0: <VirtIO, Block Device, >' --expect 'softraid0 at root' " + \
    "--expect 'pass-42' --expect 'labels-42' --expect '  a:            28672' --expect 'RAID' " + \
    "--expect 'softraid0: RAID 0 volume attached as sd' --expect 'softraid0: RAID 1 volume attached as sd' " + \
    "--expect 'softraid0: RAID 5 volume attached as sd' --expect 'softraid0: RAID 6 volume attached as sd' " + \
    "--expect 'softraid0: CONCAT volume attached as sd' " + \
    "--expect 'softraid0: RAID 1C volume attached as sd' --expect 'softraid0: CRYPTO volume attached as sd' " + \
    "--expect 'made-raid0-42' --expect 'made-raid1-42' --expect 'made-raid5-42' --expect 'made-raid6-42' --expect 'made-concat-42' " + \
    "--expect 'made-raid1c-42' --expect 'made-crypto-42' --expect 'bioctl-42' " + \
    "--reject 'label-fail0ed-' --reject 'disklabels not read: '"
softraid_check := disk_login + " " + sr_pass + sr_cat + \
    "--send-after '# ' --send 'bioctl softraid0; sr_cat\\n' " + \
    "--send-after 'cat-done-42' --send 'bioctl -c 1C -p /etc/m10f.pass -l /dev/sd2e,/dev/sd3e softraid0\\n' " + \
    "--send-after '# ' --send 'bioctl -c C -p /etc/m10f.pass -l /dev/sd3b softraid0\\n' " + \
    "--send-after '# ' --send 'sr_cat; echo unlocked-$((40+2))\\n' " + \
    "--expect ': m10f-raid0-42' --expect ': m10f-raid1-42' --expect ': m10f-raid5-42' " + \
    "--expect ': m10f-raid6-42' --expect ': m10f-concat-42' --expect ': m10f-raid1c-42' --expect ': m10f-crypto-42' " + \
    "--expect 'softraid0: RAID 1C volume attached as sd' --expect 'softraid0: CRYPTO volume attached as sd' " + \
    "--expect 'unlocked-42'"
softraid_degraded := disk_login + " " + sr_cat + \
    "--send-after '# ' --send 'bioctl softraid0; sr_cat\\n' " + \
    "--expect 'trying to bring up' --expect 'Degraded' --expect ': m10f-raid1-42' --expect ': m10f-raid6-42' --expect 'cat-done-42' " + \
    "--reject 'roaming device'"

# M9+: IPv6 between the two VMs of `smoke-link` (option INET6, sys/netinet6). Bringing lo0
# up gives it ::1 (if_up calls in6_ifattach for the default loopback); vio1 gets fd00:77::1
# on A and fd00:77::2 on B, and in6_ifattach its EUI-64 link-local address (B's MAC
# 52:54:00:bb:00:02 makes fe80::5054:ff:febb:2). A pings B's global and link-local addresses
# with ping6 (OpenBSD's ping, linked as ping6), B pings A's global one. ndp(8) is not in the
# reference clone. Part of `smoke`.
smoke-inet6: (build-amd64 "--features qemu,multiprocessor") (build-arm64 "--features qemu,multiprocessor")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-inet6: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke2 {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd {{inet6_sends}} {{inet6_expects}}
    cargo xtask smoke2 {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd {{inet6_sends}} {{inet6_expects}}

# `smoke-inet6`'s sends and expectations.
inet6_sends := "--both-send-after login: --both-send 'root\\n' --both-send-after Password: --both-send 'emibsd\\n' " + \
    "--both-send-after '# ' --both-send 'ifconfig lo0 inet 127.0.0.1/8 up\\n' --both-send-after '# ' --both-send 'ifconfig lo0\\n' " + \
    "--both-send-after '# ' --both-send 'w6(){ i=0; until ping6 -c 1 -w 1 $1 >/dev/null 2>&1; do i=$((i+1)); [ $i -ge 60 ] && break; sleep 1; done; }\\n' " + \
    "--a-send-after '# ' --a-send 'ifconfig vio1 inet6 fd00:77::1/64 up\\n' " + \
    "--b-send-after '# ' --b-send 'ifconfig vio1 inet6 fd00:77::2/64 up\\n' " + \
    "--a-send-after '# ' --a-send 'ifconfig vio1\\n' --b-send-after '# ' --b-send 'ifconfig vio1\\n' " + \
    "--a-send-after '# ' --a-send 'w6 fd00:77::2; ping6 -c 3 fd00:77::2\\n' " + \
    "--a-send-after '# ' --a-send 'w6 fe80::5054:ff:febb:2%vio1; ping6 -c 3 fe80::5054:ff:febb:2%vio1\\n' " + \
    "--b-send-after '# ' --b-send 'w6 fd00:77::1; ping6 -c 3 fd00:77::1\\n'"
inet6_expects := "--a-expect 'inet6 ::1 prefixlen 128' --b-expect 'inet6 ::1 prefixlen 128' " + \
    "--a-expect 'inet6 fe80::5054:ff:febb:1%vio1 prefixlen 64' --b-expect 'inet6 fe80::5054:ff:febb:2%vio1 prefixlen 64' " + \
    "--a-expect 'inet6 fd00:77::1 prefixlen 64' --b-expect 'inet6 fd00:77::2 prefixlen 64' " + \
    "--a-expect 'bytes from fd00:77::2: icmp_seq=' --a-expect 'bytes from fe80::5054:ff:febb:2%vio1: icmp_seq=' " + \
    "--b-expect 'bytes from fd00:77::1: icmp_seq='"

# M10c: the memory and removable file systems. Logs in as `smoke-login` does, mounts a
# tmpfs(5) on /tmp and writes to it; attaches the ramdisk's test images (`/root/images`,
# made on the host by makefs(8) and hdiutil: tools/xtask/src/userland/images.rs) to vnd(4)
# with vnconfig(8) and mounts each, FAT with mount_msdos(8), ISO 9660 with mount_cd9660(8)
# and UDF with mount_udf(8), reading its known file back; then newfs_msdos(8) formats a vnd
# over an empty file on the tmpfs and fsck_msdos(8) -n must pass it. `$((40+2))` keeps the
# echoed command lines from matching. Part of `smoke`.
smoke-fs: (build-amd64 "--features qemu,multiprocessor") (build-arm64 "--features qemu,multiprocessor")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-fs: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen {{fs_steps}}
    cargo xtask smoke {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --expect-ramdisk --until-seen {{fs_steps}}

# `smoke-fs`'s session.
fs_steps := disk_login + " " + \
    "--send-after '# ' --send 'mount_tmpfs tmpfs /tmp && echo m10c-tmpfs-$((40+2)) >/tmp/t.txt && cat /tmp/t.txt && mount\\n' " + \
    "--send-after 'm10c-tmpfs-42' --send 'vnconfig vnd0 /root/images/fat.img && mount_msdos /dev/vnd0c /mnt && cat /mnt/m10c-fat.txt && umount /mnt\\n' " + \
    "--send-after 'm10c-fat-42' --send 'vnconfig vnd1 /root/images/cd.iso && mount_cd9660 /dev/vnd1c /mnt && cat /mnt/m10c-iso.txt && umount /mnt\\n' " + \
    "--send-after 'm10c-iso-42' --send 'vnconfig vnd2 /root/images/udf.img && mount_udf /dev/vnd2c /mnt && cat /mnt/m10c-udf.txt && umount /mnt\\n' " + \
    "--send-after 'm10c-udf-42' --send 'dd if=/dev/zero of=/tmp/new.img bs=64k count=64 && vnconfig vnd3 /tmp/new.img && newfs_msdos /dev/rvnd3c\\n' " + \
    "--send-after '# ' --send 'fsck_msdos -n /dev/rvnd3c; echo fsck-msdos-rc=$?\\n' " + \
    "--send-after 'fsck-msdos-rc=' --send 'vnconfig -l\\n' " + \
    "--expect 'tmpfs on /tmp type tmpfs' --expect 'm10c-tmpfs-42' --expect 'm10c-fat-42' " + \
    "--expect 'm10c-iso-42' --expect 'm10c-udf-42' --expect '** Phase 1 - Read and Compare FATs' " + \
    "--expect 'fsck-msdos-rc=0' --expect 'vnd3: covering /tmp/new.img'"

# M13: cd(4) on vioscsi(4). The ISO `smoke-fs` mounts through vnd (the ramdisk's
# /root/images/cd.iso, made by makefs) is also given to QEMU as a `scsi-cd` drive on a virtio
# SCSI adapter (`--scsi-cd`, `tools/xtask/src/hwopts.rs`: virtio-scsi-pci on amd64,
# virtio-scsi-device on arm64, read-only, `media=cdrom`). The kernel must attach vioscsi0, its
# scsibus and cd0 (`cd0 at scsibus... targ 0 lun 0: <QEMU, QEMU CD-ROM, ...>`), mount_cd9660(8)
# must mount /dev/cd0c (the block device: cdopen, cd_get_parms, the fabricated label, READ(10)
# through cdstart and vioscsi_scsi_cmd) and read the known file back. `$((40+2))` keeps the
# echoed command line from matching. Part of `smoke`.
smoke-cd: (build-amd64 "--features qemu,multiprocessor") (build-arm64 "--features qemu,multiprocessor")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-cd: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen \
        --scsi-cd target/userland/amd64/ramdisk-root/root/images/cd.iso {{cd_steps}}
    cargo xtask smoke {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --expect-ramdisk --until-seen \
        --scsi-cd target/userland/arm64/ramdisk-root/root/images/cd.iso {{cd_steps}}

# `smoke-cd`'s session.
cd_steps := disk_login + " " + \
    "--send-after '# ' --send 'mount_cd9660 /dev/cd0c /mnt && cat /mnt/m10c-iso.txt && umount /mnt\\n' " + \
    "--expect 'vioscsi0 at virtio' --expect ' at vioscsi0: 255 targets' --expect 'cd0 at scsibus' " + \
    "--expect ' targ 0 lun 0: <QEMU, QEMU CD-ROM' " + \
    "--expect 'm10c-iso-42'"

# M10e: NFS between the two VMs of `smoke-link`. A exports /export to B with OpenBSD's
# portmap(8), mountd(8) and nfsd(8) (UDP and TCP); B lists the export with showmount(8),
# mounts it with mount_nfs(8) over UDP and then over TCP (`-T`; mount(8) shows each), reads
# A's file and writes one file per mount, which A then reads from its own disk. B retries showmount until A's
# daemons answer. The markers are built with `$((..))`, so that the typed commands do not
# match them; the commands are short, since a long line can overflow arm64's pluart input
# buffer under load. Part of `smoke`.
smoke-nfs: (build-amd64 "--features qemu,multiprocessor") (build-arm64 "--features qemu,multiprocessor")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-nfs: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke2 {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --timeout 400 \
        {{nfs_both}} {{nfs_a}} {{nfs_b}} {{nfs_expect}}
    cargo xtask smoke2 {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --timeout 400 \
        {{nfs_both}} {{nfs_a}} {{nfs_b}} {{nfs_expect}}

# `smoke-nfs`'s sends and expectations.
nfs_both := "--both-send-after 'login:' --both-send 'root\\n' --both-send-after 'Password:' --both-send 'emibsd\\n' " + \
    "--both-send-after '# ' --both-send 'ifconfig lo0 inet 127.0.0.1/8 up\\n'"
nfs_a := "--a-send-after '# ' --a-send 'ifconfig vio1 inet 192.168.77.1/24 up\\n' " + \
    "--a-send-after '# ' --a-send 'mkdir -p /export\\n' " + \
    "--a-send-after '# ' --a-send 'echo nfs-a-$((3+4)) >/export/a.txt\\n' " + \
    "--a-send-after '# ' --a-send 'echo \"/export -maproot=root 192.168.77.2\" >/etc/exports\\n' " + \
    "--a-send-after '# ' --a-send 'portmap; sleep 1; mountd; nfsd -tu -n 4\\n' " + \
    "--a-send-after '# ' --a-send 'sleep 1; echo nfs-up-$((2+3))\\n' " + \
    "--a-send-after '# ' --a-send 'cd /export; until [ -f b-tcp.txt ]; do sleep 1; done\\n' " + \
    "--a-send-after '# ' --a-send 'sleep 1; cat b-udp.txt b-tcp.txt\\n'"
nfs_b := "--b-send-after '# ' --b-send 'ifconfig vio1 inet 192.168.77.2/24 up\\n' " + \
    "--b-send-after '# ' --b-send 'until showmount -e 192.168.77.1; do sleep 2; done\\n' " + \
    "--b-send-after '# ' --b-send 'mount_nfs 192.168.77.1:/export /mnt && mount\\n' " + \
    "--b-send-after '# ' --b-send 'cat /mnt/a.txt\\n' " + \
    "--b-send-after '# ' --b-send 'echo nfs-udp-$((4+4)) >/mnt/b-udp.txt\\n' " + \
    "--b-send-after '# ' --b-send 'umount /mnt\\n' " + \
    "--b-send-after '# ' --b-send 'mount_nfs -T 192.168.77.1:/export /mnt && mount\\n' " + \
    "--b-send-after '# ' --b-send 'cat /mnt/a.txt /mnt/b-udp.txt\\n' " + \
    "--b-send-after '# ' --b-send 'echo nfs-tcp-$((5+4)) >/mnt/b-tcp.txt\\n' " + \
    "--b-send-after '# ' --b-send 'umount /mnt && echo nfs-done-$((6+4))\\n'"
nfs_expect := "--a-expect 'nfs-up-5' --a-expect 'nfs-udp-8' --a-expect 'nfs-tcp-9' " + \
    "--b-expect 'Exports list on 192.168.77.1:' --b-expect '/export                            192.168.77.2' " + \
    "--b-expect 'nfs-a-7' --b-expect '192.168.77.1:/export on /mnt type nfs (v3, udp' " + \
    "--b-expect '192.168.77.1:/export on /mnt type nfs (v3, tcp' --b-expect 'nfs-done-10'"

# M10d: ext2fs (sys/ufs/ext2fs) on a disk set of its own (`--disk-set ext2fs`, so smoke-disk's
# sd0 is left alone). Boot 1 (`--disk-fresh`) gives sd0 an MBR and disklabel(8)'s automatic
# layout as smoke-disk does, retypes partition a from 4.2BSD to ext2fs (newfs_ext2fs(8)
# insists on it): the label is printed, rewritten by a ksh function `t` and restored with
# `disklabel -R`; then newfs_ext2fs, mount(8) -t ext2fs, a file, a directory of 30 files,
# umount. The file system is 114690 sectors, seven whole block groups of 8192 1 KB blocks
# (`-s`), not the whole partition: OpenBSD's fsck_ext2fs tests `testbmap(d)` before
# `d >= e2fs_bcount` (reference/openbsd-src/sbin/fsck_ext2fs/pass5.c:151), so with a partial
# last group it reads up to 4 bytes past its block map; on the whole 131008-sector partition
# that map is 8188 bytes, which OpenBSD's malloc gives two whole pages, and the read faults on
# the next page (SIGSEGV after "Phase 5", seen here; whole groups pass). Boot 2 reuses the
# disk: fsck_ext2fs(8) -n must skip it as clean and -fn must run its five phases without a
# question, both with status 0, and the files read back after a read-only mount. Then, on
# this machine, `cargo xtask e2fsck` checks the same disk image with e2fsprogs (Homebrew's
# keg-only formula, docs/SETUP.md): `e2fsck -fn` must exit 0 and debugfs must read both files
# back (tools/xtask/src/e2fs.rs). Part of `smoke`.
smoke-ext2fs: (build-amd64 "--features qemu,multiprocessor") (build-arm64 "--features qemu,multiprocessor")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-ext2fs: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen \
        --disk-set ext2fs --disk-fresh {{ext2_make}}
    cargo xtask smoke {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen \
        --disk-set ext2fs {{ext2_check}}
    cargo xtask e2fsck --arch amd64 --disk-set ext2fs {{ext2_host}}
    cargo xtask smoke {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --expect-ramdisk --until-seen \
        --disk-set ext2fs --disk-fresh {{ext2_make}}
    cargo xtask smoke {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --expect-ramdisk --until-seen \
        --disk-set ext2fs {{ext2_check}}
    cargo xtask e2fsck --arch arm64 --disk-set ext2fs {{ext2_host}}

# `smoke-ext2fs`'s two boots and its host check.
ext2_make := disk_login + " " + \
    "--send-after '# ' --send 'fdisk -iy -f /dev/rsd0c sd0 >/dev/null && disklabel -w -A sd0 && echo label-$((40+2))\\n' " + \
    "--send-after 'label-42' --send 't() { while IFS= read -r l; do case $l in *4.2BSD*) l=\"${l%%4.2BSD*}ext2fs\";; esac; print -r -- \"$l\"; done; }\\n' " + \
    "--send-after '# ' --send 'disklabel sd0 | t >/tmp/l && disklabel -R sd0 /tmp/l && disklabel sd0 && echo relabel-$((40+2))\\n' " + \
    "--send-after 'relabel-42' --send 'newfs_ext2fs -s 114690 sd0a && echo newfs-$((40+2))\\n' " + \
    "--send-after 'newfs-42' --send 'mount -t ext2fs /dev/sd0a /mnt && mount && echo m10d-ext2-$((40+2)) >/mnt/m10d-ext2.txt\\n' " + \
    "--send-after '# ' --send 'mkdir /mnt/d && i=0 && while [ $i -lt 30 ]; do echo f$i >/mnt/d/f$i; i=$((i+1)); done\\n' " + \
    "--send-after '# ' --send 'echo m10d-ext2-sub-$((40+2)) >/mnt/d/sub.txt && ls /mnt && cat /mnt/d/f29 /mnt/m10d-ext2.txt\\n' " + \
    "--send-after '# ' --send 'umount /mnt && echo ext2-written-$((40+2))\\n' " + \
    "--expect 'sd0 at scsibus0 targ 0 lun 0: <VirtIO, Block Device, >' --expect 'label-42' " + \
    "--expect '131008               64  ext2fs' --expect 'relabel-42' " + \
    "--expect '/dev/rsd0a: 56.0MB (114690 sectors) block size 1024, fragment size 1024' " + \
    "--expect 'super-block backups (for fsck_ext2fs -b #) at:' --expect 'newfs-42' " + \
    "--expect '/dev/sd0a on /mnt type ext2fs (local)' --expect 'lost+found' --expect 'f29' --expect 'm10d-ext2-42' " + \
    "--expect 'ext2-written-42' --reject 'partition type is not'"
ext2_check := disk_login + " " + \
    "--send-after '# ' --send 'fsck_ext2fs -n /dev/rsd0a; echo fsck-rc=$?\\n' " + \
    "--send-after 'fsck-rc=' --send 'fsck_ext2fs -fn /dev/rsd0a; echo fsck-f-rc=$?\\n' " + \
    "--send-after 'fsck-f-rc=' --send 'mount -r -t ext2fs /dev/sd0a /mnt && cat /mnt/m10d-ext2.txt /mnt/d/sub.txt\\n' " + \
    "--send-after '# ' --send 'set -- /mnt/d/*; echo files-$#; umount /mnt && echo ext2-read-$((40+2))\\n' " + \
    "--expect 'sd0 at scsibus0 targ 0 lun 0' --expect '** /dev/rsd0a (NO WRITE)' " + \
    "--expect '** File system is clean; not checking' --expect 'fsck-rc=0' " + \
    "--expect '** Phase 5 - Check Cyl groups' --expect '35 files, ' --expect 'fsck-f-rc=0' " + \
    "--expect 'm10d-ext2-42' --expect 'm10d-ext2-sub-42' --expect 'files-31' --expect 'ext2-read-42' " + \
    "--reject 'UNEXPECTED' --reject 'FILE SYSTEM WAS MODIFIED' --reject '? no'"
ext2_host := "--cat /m10d-ext2.txt=m10d-ext2-42 --cat /d/sub.txt=m10d-ext2-sub-42 --cat /d/f29=f29"

# M13a: nvme(4). `cargo xtask nvme-root` writes a disk laid out as OpenBSD installs one (MBR
# with the OpenBSD partition, a disklabel whose DUID is `nvme_duid`, the userland's ffs in
# `a`, its fstab naming /dev/sd0a; tools/xtask/src/hwopts.rs), and the VM gets it as the
# namespace of an NVMe controller on q35's PCI bus (`--nvme`, slot 3, before the virtio-blk
# disk, so its namespace is sd0). The kernel boots WITHOUT the ramdisk module: boot(8)'s
# BOOTARG_BOOTDUID is the `bootduid=` word of the command line, and setroot mounts the root
# from the disk whose label has that DUID. Since M13's ACPI interrupt routing the
# controller interrupts by MSI-X (`: msix`). The session logs in, `mount` shows sd0a on /, bioctl(8)
# asks nvme0 (its bio(4) ioctls), and a file is written on the root and read back. arm64
# (M13): the controller is the first device on `virt`'s PCI bus (pciecam, device tree; pci0
# dev 1) and interrupts by MSI-X through the GICv2m frame (ampintcmsi); the virtio-mmio disks
# attach before the PCI bus, so the persistent disk is sd0, the boot image sd1 and the
# namespace sd2 (`nvme-root --root-dev sd2a`). Part of `smoke`.
nvme_duid := "4e564d45524f4f54"

smoke-nvme: (build-amd64 "--features qemu,multiprocessor") build-init-amd64 (build-arm64 "--features qemu,multiprocessor") build-init-arm64
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-nvme: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask nvme-root --arch amd64 --duid {{nvme_duid}}
    cargo xtask smoke {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --ramdisk none --expect-ramdisk \
        --nvme nvme-amd64.img --cmdline "bootduid={{nvme_duid}}" --until-seen \
        {{disk_login}} \
        --send-after '# ' --send 'mount\n' \
        --send-after '# ' --send 'bioctl nvme0\n' \
        --send-after '# ' --send 'echo m13a-nvme-$((40+2)) >/m13a.txt && cat /m13a.txt\n' \
        --send-after '# ' --send 'dd if=/dev/zero of=/dev/rsd0c bs=64k seek=1010 count=8 && dd if=/dev/rsd0c of=/dev/null bs=64k count=64\n' \
        --send-after '# ' --send 'echo m13a-raw-$((40+2)) | dd of=/dev/rsd0c bs=512 seek=130000 conv=sync 2>/dev/null; dd if=/dev/rsd0c bs=512 skip=130000 count=1 2>/dev/null\n' \
        --expect "nvme0 at pci0 dev 3 function 0 vendor 0x1b36 product 0x0010 rev 0x02: msix, NVMe 1.4" \
        --expect "NVMe 1.4" --expect "nvme0: QEMU NVMe Ctrl, firmware " --expect "serial EMIBSD0001" \
        --expect "scsibus0 at nvme0: 257 targets, initiator 0" \
        --expect "sd0 at scsibus0 targ 1 lun 0: <NVMe, QEMU NVMe Ctrl, " \
        --expect "vioblk0 at virtio1" --expect "sd1 at scsibus1 targ 0 lun 0: <VirtIO, Block Device, >" \
        --expect "root on sd0a ({{nvme_duid}}.a) swap on sd0b dump on sd0b" \
        --expect "rc: multi-user" --expect "/dev/sd0a on / type ffs (local)" \
        --expect "nvme0: NVMe 1.4, NVM I/O command set, Enabled, Ready" --expect "nvme0 0 Online" \
        --expect "Namespace 1" --expect "m13a-nvme-42" --expect "524288 bytes transferred" \
        --expect "4194304 bytes transferred" --expect "m13a-raw-42" --reject "mount -uw / failed"
    cargo xtask nvme-root --arch arm64 --duid {{nvme_duid}} --root-dev sd2a
    cargo xtask smoke {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --ramdisk none --expect-ramdisk \
        --nvme nvme-arm64.img --cmdline "bootduid={{nvme_duid}}" --until-seen \
        {{disk_login}} \
        --send-after '# ' --send 'mount\n' \
        --send-after '# ' --send 'bioctl nvme0\n' \
        --send-after '# ' --send 'echo m13a-nvme-$((40+2)) >/m13a.txt && cat /m13a.txt\n' \
        --send-after '# ' --send 'dd if=/dev/zero of=/dev/rsd2c bs=64k seek=1010 count=8 && dd if=/dev/rsd2c of=/dev/null bs=64k count=64\n' \
        --send-after '# ' --send 'echo m13a-raw-$((40+2)) | dd of=/dev/rsd2c bs=512 seek=130000 conv=sync 2>/dev/null; dd if=/dev/rsd2c bs=512 skip=130000 count=1 2>/dev/null\n' \
        --send-after '# ' --send 'vmstat -i\n' \
        --expect "nvme0 at pci0 dev 1 function 0 vendor 0x1b36 product 0x0010 rev 0x02: msix, NVMe 1.4" \
        --expect "nvme0: QEMU NVMe Ctrl, firmware " --expect "serial EMIBSD0001" \
        --expect "scsibus2 at nvme0: 257 targets, initiator 0" \
        --expect "sd2 at scsibus2 targ 1 lun 0: <NVMe, QEMU NVMe Ctrl, " \
        --expect "root on sd2a ({{nvme_duid}}.a) swap on sd2b dump on sd2b" \
        --expect "rc: multi-user" --expect "/dev/sd2a on / type ffs (local)" \
        --expect "nvme0: NVMe 1.4, NVM I/O command set, Enabled, Ready" --expect "nvme0 0 Online" \
        --expect "Namespace 1" --expect "m13a-nvme-42" --expect "524288 bytes transferred" \
        --expect "4194304 bytes transferred" --expect "m13a-raw-42" --expect "/nvme0" \
        --reject "mount -uw / failed"

# M13: ahci(4) and atascsi. The disk `cargo xtask nvme-root` writes (as for smoke-nvme, with
# its own DUID `ahci_duid` and an fstab naming /dev/sd2a: diskmap(4) is not ported, so fstab
# names the unit) goes on the second port of q35's built-in AHCI controller (`--ahci`,
# `ide.1`; the boot image is on port 0, which ahci now attaches too). PCI is probed by device
# number, so the virtio-blk disk (dev 3) is sd0 and the controller (dev 31) gives sd1 (the
# boot image, targ 0) and sd2 (the root, targ 1). The kernel boots WITHOUT the ramdisk module
# and mounts its root from the disk whose label has the `bootduid=` DUID. Since M13's ACPI
# interrupt routing (acpimadt, acpipci) the controller interrupts by MSI (`: msi`; vmstat -i
# counts its interrupts). The session logs in, `mount` shows sd2a on /, a file is written on
# the root and read back, a large file goes through the buffer cache (NCQ, several commands
# on the chip), and raw I/O past the file system reads back what it wrote. arm64 (M13, the
# milestone's exit criterion): `virt` has no AHCI controller of its own, so `--ahci` adds an
# ich9-ahci as the first device on its PCI bus (pciecam, device tree; pci0 dev 1) with the
# disk on port 0; it interrupts by MSI through the GICv2m frame (ampintcmsi). The virtio-mmio
# disks attach before the PCI bus, so the persistent disk is sd0, the boot image sd1 and the
# root sd2 (`nvme-root --root-dev sd2a`). Part of `smoke`.
ahci_duid := "41484349524f4f54"

smoke-ahci: (build-amd64 "--features qemu,multiprocessor") build-init-amd64 (build-arm64 "--features qemu,multiprocessor") build-init-arm64
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-ahci: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask nvme-root --arch amd64 --duid {{ahci_duid}} --out ahci-amd64.img --root-dev sd2a
    cargo xtask smoke {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --ramdisk none --expect-ramdisk \
        --ahci ahci-amd64.img --cmdline "bootduid={{ahci_duid}}" --until-seen \
        {{disk_login}} \
        --send-after '# ' --send 'mount\n' \
        --send-after '# ' --send 'echo m13-ahci-$((40+2)) >/m13ahci.txt && cat /m13ahci.txt\n' \
        --send-after '# ' --send 'dd if=/dev/zero of=/big bs=64k count=128 && dd if=/big of=/dev/null bs=64k && rm /big\n' \
        --send-after '# ' --send 'dd if=/dev/zero of=/dev/rsd2c bs=64k seek=1010 count=8 && dd if=/dev/rsd2c of=/dev/null bs=64k count=64\n' \
        --send-after '# ' --send 'echo m13-ahci-raw-$((40+2)) | dd of=/dev/rsd2c bs=512 seek=130000 conv=sync 2>/dev/null; dd if=/dev/rsd2c bs=512 skip=130000 count=1 2>/dev/null\n' \
        --send-after '# ' --send 'vmstat -i\n' \
        --expect "ahci0 at pci0 dev 31 function 2 vendor 0x8086 product 0x2922 rev 0x02: msi" \
        --expect ", AHCI 1.0" --expect "ahci0: port 0: 1.5Gb/s" --expect "ahci0: port 1: 1.5Gb/s" \
        --expect "vioblk0 at virtio1" --expect "sd0 at scsibus0 targ 0 lun 0: <VirtIO, Block Device, >" \
        --expect "scsibus1 at ahci0: 32 targets" \
        --expect "sd1 at scsibus1 targ 0 lun 0: <ATA, QEMU HARDDISK, 2.5+> t10.ATA_QEMU_HARDDISK_QM00001_" \
        --expect "sd2 at scsibus1 targ 1 lun 0: <ATA, QEMU HARDDISK, 2.5+> t10.ATA_QEMU_HARDDISK_QM00003_" \
        --expect "sd2: " \
        --expect "root on sd2a ({{ahci_duid}}.a) swap on sd2b dump on sd2b" \
        --expect "rc: multi-user" --expect "/dev/sd2a on / type ffs (local)" \
        --expect "m13-ahci-42" --expect "8388608 bytes transferred" \
        --expect "524288 bytes transferred" --expect "4194304 bytes transferred" \
        --expect "m13-ahci-raw-42" --expect "/ahci0" --reject "mount -uw / failed"
    cargo xtask nvme-root --arch arm64 --duid {{ahci_duid}} --out ahci-arm64.img --root-dev sd2a
    cargo xtask smoke {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --ramdisk none --expect-ramdisk \
        --ahci ahci-arm64.img --cmdline "bootduid={{ahci_duid}}" --until-seen \
        {{disk_login}} \
        --send-after '# ' --send 'mount\n' \
        --send-after '# ' --send 'echo m13-ahci-$((40+2)) >/m13ahci.txt && cat /m13ahci.txt\n' \
        --send-after '# ' --send 'dd if=/dev/zero of=/big bs=64k count=128 && dd if=/big of=/dev/null bs=64k && rm /big\n' \
        --send-after '# ' --send 'dd if=/dev/zero of=/dev/rsd2c bs=64k seek=1010 count=8 && dd if=/dev/rsd2c of=/dev/null bs=64k count=64\n' \
        --send-after '# ' --send 'echo m13-ahci-raw-$((40+2)) | dd of=/dev/rsd2c bs=512 seek=130000 conv=sync 2>/dev/null; dd if=/dev/rsd2c bs=512 skip=130000 count=1 2>/dev/null\n' \
        --send-after '# ' --send 'vmstat -i\n' \
        --expect "ahci0 at pci0 dev 1 function 0 vendor 0x8086 product 0x2922 rev 0x02: msi" \
        --expect ", AHCI 1.0" --expect "ahci0: port 0: 1.5Gb/s" \
        --expect "scsibus2 at ahci0: 32 targets" \
        --expect "sd2 at scsibus2 targ 0 lun 0: <ATA, QEMU HARDDISK, 2.5+> t10.ATA_QEMU_HARDDISK_QM00001_" \
        --expect "root on sd2a ({{ahci_duid}}.a) swap on sd2b dump on sd2b" \
        --expect "rc: multi-user" --expect "/dev/sd2a on / type ffs (local)" \
        --expect "m13-ahci-42" --expect "8388608 bytes transferred" \
        --expect "524288 bytes transferred" --expect "4194304 bytes transferred" \
        --expect "m13-ahci-raw-42" --expect "/ahci0" --reject "mount -uw / failed"

# M16f: smmu(4). arm64 only. `virt,iommu=smmuv3` (`--iommu smmuv3`, hwopts.rs) puts QEMU's
# SMMUv3 in front of the PCIe bus; its device-tree node attaches smmu0 at mainbus0 before
# pciecam, and pciecam's `iommu-map` hands every PCI function the DMA tag of its stream's
# domain. The NVMe root disk of `smoke-nvme` (its own image, `smmu-arm64.img`) is mounted
# through it: with the SMMU enabled, DMA that bypasses the stream's domain aborts (checked
# once by handing the PCI functions their untranslated tags: nvme0 could not even identify
# and smmu0 printed `smmu0: event 0x800000010 ...` for stream 8), so a root mounted
# from nvme0, a file written and read back and raw dd through the namespace show the NVMe
# DMA (queues, PRP lists, data, and the MSI-X doorbell loaded into the function's tag) going
# through the SMMU's page tables; `--reject 'smmu0: '` fails the run on any event, global
# error or command queue error. The second boot is `virt,acpi=on` with the SMMUv3 in the
# IORT: at the pin smmu_acpi matches only SMMUv2 nodes, so no smmu attaches, the SMMU stays
# disabled (bypass) and the virtio PCI disk must still work. Part of `smoke`.
smmu_duid := "534d4d55524f4f54"

smoke-smmu: (build-arm64 "--features qemu,multiprocessor") build-init-arm64 efiboot-arm64
    @test -f target/userland/arm64/ramdisk.ffs -a -x target/userland/arm64/host/bin/makefs || \
        { echo "smoke-smmu: no ramdisk image or makefs; run just userland first"; exit 1; }
    cargo xtask nvme-root --arch arm64 --duid {{smmu_duid}} --out smmu-arm64.img --root-dev sd2a
    cargo xtask smoke {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --ramdisk none --expect-ramdisk \
        --iommu smmuv3 --nvme smmu-arm64.img --cmdline "bootduid={{smmu_duid}}" --until-seen \
        {{disk_login}} \
        --send-after '# ' --send 'mount\n' \
        --send-after '# ' --send 'echo m16f-smmu-$((40+2)) >/m16f.txt && cat /m16f.txt\n' \
        --send-after '# ' --send 'dd if=/dev/zero of=/dev/rsd2c bs=64k seek=1010 count=8 && dd if=/dev/rsd2c of=/dev/null bs=64k count=64\n' \
        --send-after '# ' --send 'echo m16f-raw-$((40+2)) | dd of=/dev/rsd2c bs=512 seek=130000 conv=sync 2>/dev/null; dd if=/dev/rsd2c bs=512 skip=130000 count=1 2>/dev/null\n' \
        --send-after '# ' --send 'vmstat -i\n' \
        --expect "smmu0 at mainbus0" \
        --expect "nvme0 at pci0 dev 1 function 0 vendor 0x1b36 product 0x0010 rev 0x02: msix, NVMe 1.4" \
        --expect "scsibus2 at nvme0: 257 targets, initiator 0" \
        --expect "sd2 at scsibus2 targ 1 lun 0: <NVMe, QEMU NVMe Ctrl, " \
        --expect "root on sd2a ({{smmu_duid}}.a) swap on sd2b dump on sd2b" \
        --expect "rc: multi-user" --expect "/dev/sd2a on / type ffs (local)" \
        --expect "m16f-smmu-42" --expect "524288 bytes transferred" \
        --expect "4194304 bytes transferred" --expect "m16f-raw-42" --expect "/nvme0" \
        --reject "smmu0: " --reject "mount -uw / failed"
    cargo xtask efiboot-disk --arch arm64 --efi target/efiboot/arm64/BOOTAA64.EFI --kernel target/{{arm64}}/debug/bsd --root-dev sd0a
    cargo xtask smoke {{reject}} {{smp}} --arch arm64 --acpi --iommu smmuv3 --until-seen \
        --send-after 'boot> ' --send 'boot\n' \
        {{disk_login}} \
        --send-after '# ' --send 'mount\n' \
        --send-after '/ type ffs' --send 'echo m16f-acpi-$((40+2)) >/m16f.txt && cat /m16f.txt\n' \
        --expect "acpi0: tables DSDT FACP APIC PPTT GTDT MCFG SPCR DBG2 IORT" \
        --expect "acpiiort0 at acpi0" --expect "acpipci0 at acpi0 PCI0" \
        --expect "sd0 at scsibus0 targ 0 lun 0: <VirtIO, Block Device, >" \
        --expect "root on sd0a (454649424f4f5430.a) swap on sd0b dump on sd0b" \
        --expect "rc: multi-user" --expect "/dev/sd0a on / type ffs (local)" \
        --expect "m16f-acpi-42" --reject "smmu0"

# M13: siop(4) on QEMU's LSI 53C895A (`--lsi`, `tools/xtask/src/hwopts.rs`: the adapter
# after every other device, a fresh zeroed 64 MiB `scsi-hd` at target 0 and, with
# `--lsi-cd`, the ramdisk's ISO as a `scsi-cd` at target 1). The kernel must attach siop0
# on q35's PCI bus (INTx, which since M13 goes through the I/O APIC pin acpiprt finds:
# `apic 0 int N`; the SCRIPTS in the chip's 8 KB of on-board RAM), its scsibus
# (16 targets, initiator 7), the disk as sd1 (vioblk's persistent disk is sd0) and the
# CD-ROM as cd0. The session runs fdisk(8), disklabel(8) and newfs(8) on sd1, writes a
# file and a copy of /bin/ksh, unmounts, mounts read-only and reads both back (cmp(1)),
# reads 1 MiB raw with dd(1), writes and reads back one raw sector near the end of the
# disk (once the file system is done with), and mounts the ISO with mount_cd9660(8). amd64
# only: arm64's GENERIC has no siop. Part of `smoke`.
smoke-siop: (build-amd64 "--features qemu,multiprocessor")
    @test -f target/userland/amd64/ramdisk.ffs || \
        { echo "smoke-siop: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen \
        --lsi lsi-amd64.img --lsi-cd target/userland/amd64/ramdisk-root/root/images/cd.iso \
        {{disk_login}} \
        --send-after '# ' --send 'fdisk -iy -f /dev/rsd1c sd1 && fdisk -f /dev/rsd1c sd1\n' \
        --send-after '# ' --send 'disklabel -w -A sd1 && disklabel sd1\n' \
        --send-after '# ' --send 'newfs sd1a\n' \
        --send-after '# ' --send 'mount /dev/sd1a /mnt && echo m13-siop-$((40+2)) >/mnt/siop.txt && cp /bin/ksh /mnt/ksh && umount /mnt && echo siop-written-$((40+2))\n' \
        --send-after '# ' --send 'mount -r /dev/sd1a /mnt && cat /mnt/siop.txt && cmp /bin/ksh /mnt/ksh && echo siop-cmp-$((40+2)) && umount /mnt\n' \
        --send-after '# ' --send 'dd if=/dev/rsd1c of=/dev/null bs=64k count=16\n' \
        --send-after '# ' --send 'echo m13-raw-$((40+2)) | dd of=/dev/rsd1c bs=512 seek=131000 conv=sync 2>/dev/null; dd if=/dev/rsd1c bs=512 skip=131000 count=1 2>/dev/null\n' \
        --send-after '# ' --send 'mount_cd9660 /dev/cd0c /mnt && cat /mnt/m10c-iso.txt && umount /mnt\n' \
        --expect "siop0 at pci0 dev " --expect "vendor 0x1000 product 0x0012 rev 0x00: apic 0 int " \
        --expect "using 8K of on-board RAM" \
        --expect "scsibus1 at siop0: 16 targets, initiator 7" \
        --expect "sd1 at scsibus1 targ 0 lun 0: <QEMU, QEMU HARDDISK, " \
        --expect "sd1: 64MB, 512 bytes/sector, 131072 sectors" \
        --expect "cd0 at scsibus1 targ 1 lun 0: <QEMU, QEMU CD-ROM, " \
        --expect '*3: A6' --expect '/dev/rsd1a: ' --expect "siop-written-42" --expect "m13-siop-42" \
        --expect "siop-cmp-42" --expect "1048576 bytes transferred" --expect "m13-raw-42" \
        --expect "m10c-iso-42"

# M13: em(4), the exit criterion's "em(4) on e1000e answers the M7+ ping". `--nic e1000e`
# (`tools/xtask/src/hwopts.rs`) puts QEMU's 82574L on the user network in vio0's place, so
# em0 is the only Ethernet interface: the kernel's boot self-test gives it 10.0.2.15/24 and
# the default route (its own ping goes out before the PHY has negotiated the link, which a
# real NIC takes time to do, so it is not expected), then, logged in, ifconfig(8) shows em0
# up, with its address and an active link, and ping(8) gets the gateway's reply. Both archs
# (arm64: on `virt`'s PCIe bus, MSI through the GICv2m frame). amd64 also boots the
# 82540EM (`--nic e1000`: INTx through the I/O APIC, the I/O BAR, 32-bit DMA) and pings
# through it, and the 82576 (`--nic igb`), which attaches and gets its link but passes no
# traffic, probably because QEMU's igb model writes back advanced receive descriptors only
# while em(4) programs legacy ones (SRRCTL's DESCTYPE 0), as OpenBSD's does (not checked
# against QEMU's source). Part of `smoke`.
smoke-em: (build-amd64 "--features qemu,multiprocessor") (build-arm64 "--features qemu,multiprocessor")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-em: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen \
        --nic e1000e {{em_session}} {{em_ping}} \
        --expect "vendor 0x8086 product 0x10d3 rev 0x00: msi, address 52:54:00:12:34:56"
    cargo xtask smoke {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --expect-ramdisk --until-seen \
        --nic e1000e {{em_session}} {{em_ping}} \
        --expect "vendor 0x8086 product 0x10d3 rev 0x00: msi, address 52:54:00:12:34:56"
    cargo xtask smoke {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen \
        --nic e1000 {{em_session}} {{em_ping}} \
        --expect "vendor 0x8086 product 0x100e rev 0x03: apic 0 int "
    cargo xtask smoke {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen \
        --nic igb {{em_session}} \
        --expect "vendor 0x8086 product 0x10c9 rev 0x01: msi, address 52:54:00:12:34:56"

# `smoke-em`'s login and commands, the expectations every em(4) boot shares, and the ping's.
em_session := "--send-after 'login:' --send 'root\\n' --send-after 'Password:' --send 'emibsd\\n' " + \
    "--send-after '# ' --send 'ifconfig em0\\n' --send-after '# ' --send 'ping -c 1 10.0.2.2\\n' " + \
    "--expect 'em0 at pci0 dev ' --expect 'rc: multi-user' --expect 'em0: flags=' " + \
    "--expect 'status: active' --expect 'inet 10.0.2.15 netmask 0xffffff00'"
em_ping := "--expect 'PING 10.0.2.2 (10.0.2.2): 56 data bytes' " + \
    "--expect '1 packets transmitted, 1 packets received, 0.0% packet loss'"

# M13: re(4) on QEMU's Realtek 8139C+ (`--nic rtl8139`, PCI 10ec:8139 revision 0x20, which
# re_pci_probe takes and rl_pci_match leaves to it), in vio0's place on the user network. re0
# attaches with the PHY that mii(4) finds at address 0 (the 8139C+ has no PHY ID registers, so
# rlphy(4) takes it, at its "RTL internal PHY" priority); the kernel's self-test gives re0
# 10.0.2.15/24, and, logged in, ifconfig(8) shows its media and an active link (SIOCGIFMEDIA
# through ifmedia_ioctl and the PHY's status) and ping(8) gets the gateway's reply. Both
# archs, with INTx (the 8139C+ has no MSI): the I/O APIC on amd64, the GIC through the PCIe
# node's interrupt-map on arm64 (`irq`). Part of `smoke`.
smoke-re: (build-amd64 "--features qemu,multiprocessor") (build-arm64 "--features qemu,multiprocessor")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-re: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen \
        --nic rtl8139 {{re_session}} {{em_ping}} \
        --expect "vendor 0x10ec product 0x8139 rev 0x20: RTL8139C+ (0x7480), apic 0 int "
    cargo xtask smoke {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --expect-ramdisk --until-seen \
        --nic rtl8139 {{re_session}} {{em_ping}} \
        --expect "vendor 0x10ec product 0x8139 rev 0x20: RTL8139C+ (0x7480), irq, address 52:54:00:12:34:56"

# M13: vmx(4) on QEMU's VMware VMXNET3 (`--nic vmxnet3`, PCI 15ad:07b0, revision 1), in vio0's
# place on the user network. QEMU reports the interrupt type AUTO and offers 25 MSI-X vectors, so
# vmx0 takes vector 0 for the device's events and, through intrmap(9), one vector per queue on the
# CPU the map picks: 4 queues on `smp4`'s four CPUs (min(24 spare vectors, 8, ncpus), a power of
# two). The kernel's self-test gives vmx0 10.0.2.15/24 (after the application processors boot, so
# intr_barrier can reach the queues' CPUs); logged in, ifconfig(8) shows an active link and
# ping(8) gets the gateway's reply, which QEMU (one receive queue) delivers on queue 0, and
# vmstat(8) counts queue 0's vector. Both archs: MSI-X through the local APICs on amd64, through
# the GICv2m frames (`ampintcmsi`) on arm64. Part of `smoke`.
smoke-vmx: (build-amd64 "--features qemu,multiprocessor") (build-arm64 "--features qemu,multiprocessor")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-vmx: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke {{reject}} {{smp4}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen \
        --nic vmxnet3 {{vmx_session}} {{em_ping}}
    cargo xtask smoke {{reject}} {{smp4}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --expect-ramdisk --until-seen \
        --nic vmxnet3 {{vmx_session}} {{em_ping}}

# `smoke-vmx`'s login and commands and the expectations both archs share.
vmx_session := "--send-after 'login:' --send 'root\\n' --send-after 'Password:' --send 'emibsd\\n' " + \
    "--send-after '# ' --send 'ifconfig vmx0\\n' --send-after '# ' --send 'ping -c 1 10.0.2.2\\n' " + \
    "--send-after 'packet loss' --send 'vmstat -i\\n' " + \
    "--expect 'vmx0 at pci0 dev ' " + \
    "--expect 'vendor 0x15ad product 0x07b0 rev 0x01: msix, 4 queues, address 52:54:00:12:34:56' " + \
    "--expect 'selftest: ping 10.0.2.2: echo reply received' --expect 'rc: multi-user' " + \
    "--expect 'vmx0: flags=' --expect 'media: Ethernet autoselect' --expect 'status: active' " + \
    "--expect 'inet 10.0.2.15 netmask 0xffffff00' --expect '/vmx0:0 '"

# `smoke-re`'s login and commands and the expectations both archs share.
re_session := "--send-after 'login:' --send 'root\\n' --send-after 'Password:' --send 'emibsd\\n' " + \
    "--send-after '# ' --send 'ifconfig re0\\n' --send-after '# ' --send 'ping -c 1 10.0.2.2\\n' " + \
    "--expect 're0 at pci0 dev ' --expect 'address 52:54:00:12:34:56' " + \
    "--expect 'rlphy0 at re0 phy 0: RTL internal PHY' --expect 'rc: multi-user' " + \
    "--expect 're0: flags=' --expect 'media: Ethernet autoselect (100baseTX full-duplex)' " + \
    "--expect 'status: active' --expect 'inet 10.0.2.15 netmask 0xffffff00'"

# M14: OpenBSD's efiboot boots the disk instead of Limine. `cargo xtask efiboot-disk` writes the
# boot image as OpenBSD installs one (tools/xtask/src/efiboot.rs): an MBR with the OpenBSD
# partition (its disklabel, DUID EFIBOOT0, `a` an ffs made by OpenBSD's makefs holding the root
# `just userland` stages for the ramdisk, with /bsd, the MP smoke kernel, /etc/boot.conf,
# /etc/random.seed, and an fstab whose root is /dev/sd1a) and the EFI system partition holding
# BOOTX64.EFI. EDK2 starts efiboot from the ESP; it prints its banner, probes the console, the
# memory and the disks (efiboot's own names, in EFI block I/O order: the boot disk is hd0, with
# its label; OVMF connects no other disk), runs boot.conf (`set timeout 0`, an echo) and prompts.
# The smoke lists the ffs (`ls /`, `ls /etc`), prints the memory map (`machine memory`) and the
# disks, and boots: loadfile reads the kernel's segments and symbols through ufs and cread
# (`...]=0x<size>`), run_loadfile moves it to its physical address and enters locore0.S's 32-bit
# `start` at 0x1000000 (M14 track A2), which builds the bootstrap page tables and calls the boot
# glue's bootarg entry: the kernel takes its memory map, console, DUID and EFI tables from
# boot(8)'s bootarg list, starts the application processors itself (mptramp.S, INIT/SIPI, from the
# MADT), finds its root by the DUID (sd1a: the boot image is on q35's AHCI port after the virtio
# disk), runs rc to login, and the root login sees `ncpu` CPUs and / on the disk. arm64 (M14 track
# A3): the same disk with BOOTAA64.EFI; EDK2 AArch64 on `virt,acpi=off` hands efiboot its device
# tree (the C's first choice; efiacpi builds one from the ACPI tables only without it), efiboot
# loads /bsd into its 64 MB block and enters locore0.S's _start with the tree (x2): the kernel
# builds its bootstrap tables, takes /chosen's bootargs, DUID, UEFI memory map and system table
# (getbootinfo), starts the other processors by PSCI CPU_ON (cpu_hatch_secondary) and finds its
# root by the DUID (sd1a: the boot image is the virtio disk after the blank one). `machine dtb` is
# arm64's machine command (it has no `machine memory`). Part of `smoke`.
smoke-efiboot: (build-amd64 "--features qemu,multiprocessor") (build-arm64 "--features qemu,multiprocessor") efiboot-amd64 efiboot-arm64
    @test -x target/userland/amd64/host/bin/makefs -a -f target/userland/amd64/ramdisk-root/etc/fstab || \
        { echo "smoke-efiboot: no makefs or staged root; run just userland first"; exit 1; }
    cargo xtask efiboot-disk --arch amd64 --efi target/efiboot/amd64/BOOTX64.EFI --kernel target/{{amd64}}/debug/bsd --root-dev sd1a
    cargo xtask smoke {{reject}} {{smp}} --arch amd64 --until-seen \
        --send-after 'boot> ' --send 'ls /\n' \
        --send-after 'boot> ' --send 'ls /etc\n' \
        --send-after 'boot> ' --send 'machine memory\n' \
        --send-after 'boot> ' --send 'machine diskinfo\n' \
        --send-after 'boot> ' --send 'boot\n' \
        {{disk_login}} \
        --send-after '# ' --send 'sysctl hw.ncpu\n' \
        --send-after 'hw.ncpu=' --send 'mount\n' \
        --expect ">> EmiBSD/amd64 BOOTX64 3.71" --expect "probing: pc0" --expect "disk: hd0" \
        --expect "efiboot: boot.conf read" --expect "boot> " \
        --expect "drwxr-xr-x 0,0" --expect "-r-xr-xr-x 0,0" --expect "-rw-r--r-- 0,0" \
        --expect "Region 0: type 1 at 0x0 for " --expect "Total free memory: " \
        --expect "BlkSiz" \
        --expect "booting hd0a:/bsd: " --expect "]=0x" --expect "entry point at 0x1000000" \
        --expect "bsd: booted on amd64 by boot(8) efiboot" --expect "bsd: boot(8) bootarg protocol, " \
        --expect "bsd: {{ncpu}} processors, boot processor hwid 0x0" --expect "EmiBSD 8.0 (GENERIC) #" \
        --expect "cpu0 at mainbus0: apid 0 (boot processor)" \
        --expect "cpu1 at mainbus0: apid 1 (application processor)" \
        --expect "cpu{{aps}} at mainbus0: apid {{aps}} (application processor)" \
        --expect "x86_ipi_selftest: X86_IPI_NOP taken by {{aps}} cpus, tlb shootdowns acknowledged" \
        --expect "root on sd1a (454649424f4f5430.a) swap on sd1b dump on sd1b" \
        --expect "rc: multi-user" --expect "login:" --expect "hw.ncpu={{ncpu}}" \
        --expect "/dev/sd1a on / type ffs (local)"
    @test -x target/userland/arm64/host/bin/makefs -a -f target/userland/arm64/ramdisk-root/etc/fstab || \
        { echo "smoke-efiboot: no arm64 makefs or staged root; run just userland first"; exit 1; }
    cargo xtask efiboot-disk --arch arm64 --efi target/efiboot/arm64/BOOTAA64.EFI --kernel target/{{arm64}}/debug/bsd --root-dev sd1a
    cargo xtask smoke {{reject}} {{smp}} --arch arm64 --until-seen \
        --send-after 'boot> ' --send 'ls /\n' \
        --send-after 'boot> ' --send 'machine dtb\n' \
        --send-after 'boot> ' --send 'boot\n' \
        {{disk_login}} \
        --send-after '# ' --send 'sysctl hw.ncpu\n' \
        --send-after 'hw.ncpu=' --send 'mount\n' \
        --expect ">> EmiBSD/arm64 BOOTAA64 1.26" --expect "efiboot: boot.conf read" --expect "boot> " \
        --expect "-r-xr-xr-x 0,0" --expect "booting sd0a:/bsd: " --expect "]=0x" \
        --expect "bsd: booted on arm64 by boot(8) efiboot" --expect "bsd: boot(8) bootarg protocol, " \
        --expect "bsd: {{ncpu}} processors, boot processor hwid 0x0" --expect "EmiBSD 8.0 (GENERIC) #" \
        --expect "cpu0 at mainbus0 mpidr 0: ARM Cortex-A72" --expect "cpu{{aps}} at mainbus0 mpidr {{aps}}: ARM Cortex-A72" \
        --expect "cpu: {{aps}} of {{aps}} application processors running" \
        --expect "root on sd1a (454649424f4f5430.a) swap on sd1b dump on sd1b" \
        --expect "rc: multi-user" --expect "login:" --expect "hw.ncpu={{ncpu}}" \
        --expect "/dev/sd1a on / type ffs (local)"

# M14: arm64 on ACPI (sys/arch/arm64/arm64/acpi_machdep.c, dev/acpi/acpimcfg.c,
# arch/arm64/dev/acpipci.c and acpiiort.c, dev/acpi/pluart_acpi.c). QEMU `virt,acpi=on` (`--acpi`,
# hwopts.rs) with the efiboot disk and the NIC as PCI virtio functions: EDK2 hands efiboot the
# ACPI tables and no device tree, so efiboot's efiacpi builds the tree from them (the GIC, the
# timer, PSCI, the CPUs and an `openbsd,acpi-5.0` node naming the RSDP), and the kernel attaches
# acpi0 there: acpimcfg maps the ECAM window, acpipci0 the PCI host bridge (`_CRS` windows, MSI
# through the GICv2m frame), pluart0 at acpi0 becomes the console by the SPCR, and the root is
# found on the PCI disk by its DUID. The root login sees `ncpu` CPUs and pings the host through
# vio0 (virtio-net-pci, MSI-X). arm64 only: amd64 boots on ACPI in every smoke already. Part of
# `smoke`.
smoke-acpi: (build-arm64 "--features qemu,multiprocessor") efiboot-arm64
    @test -x target/userland/arm64/host/bin/makefs -a -f target/userland/arm64/ramdisk-root/etc/fstab || \
        { echo "smoke-acpi: no arm64 makefs or staged root; run just userland first"; exit 1; }
    cargo xtask efiboot-disk --arch arm64 --efi target/efiboot/arm64/BOOTAA64.EFI --kernel target/{{arm64}}/debug/bsd --root-dev sd0a
    cargo xtask smoke {{reject}} {{smp}} --arch arm64 --acpi --until-seen \
        --send-after 'boot> ' --send 'boot\n' \
        {{disk_login}} \
        --send-after '# ' --send 'sysctl hw.ncpu\n' \
        --send-after 'hw.ncpu=' --send 'mount\n' \
        --send-after '/ type ffs' --send 'ping -c 1 10.0.2.2\n' \
        --expect ">> EmiBSD/arm64 BOOTAA64 1.26" --expect "efiboot: boot.conf read" \
        --expect "booting sd0a:/bsd: " --expect "FACP APIC PPTT GTDT MCFG SPCR DBG2 IORT" \
        --expect "bsd: booted on arm64 by boot(8) efiboot" --expect "bsd: {{ncpu}} processors, boot processor hwid 0x0" \
        --expect "mainbus0 at root: ACPI" --expect "{{gic_acpi_attach}}" \
        --expect "{{gic_msi_attach}}" --expect "agtimer0 at mainbus0: 62500 kHz" \
        --expect "acpi0 at mainbus0: ACPI 6.3" --expect "acpi0: tables DSDT FACP APIC PPTT GTDT MCFG SPCR DBG2 IORT" \
        --expect "acpimcfg0 at acpi0" --expect "acpimcfg0: addr 0x4010000000, bus 0-255" \
        --expect "acpiiort0 at acpi0" \
        --expect "pluart0 at acpi0 COM0 addr 0x9000000/0x1000 irq 33" --expect "pluart0: console" \
        --expect "acpipci0 at acpi0 PCI0" --expect "pci0 at acpipci0" \
        --expect "virtio0 at pci0 dev 1 function 0 vendor 0x1af4 product 0x1001" \
        --expect "sd0 at scsibus0 targ 0 lun 0: <VirtIO, Block Device, >" \
        --expect "vio0 at virtio1: 1 queue, address 52:54:00:12:34:56" --expect "virtio1: msix per-VQ" \
        --expect "root on sd0a (454649424f4f5430.a) swap on sd0b dump on sd0b" \
        --expect "cpu: {{aps}} of {{aps}} application processors running" \
        --expect "selftest: ping 10.0.2.2: echo reply received" \
        --expect "rc: multi-user" --expect "(tty00)" --expect "login:" --expect "hw.ncpu={{ncpu}}" \
        --expect "/dev/sd0a on / type ffs (local)" \
        --expect "1 packets transmitted, 1 packets received, 0.0% packet loss"

# M16f: agintc(4), the GICv3 (arch/arm64/dev/agintc.c), on `virt,gic-version=3` (`--gic 3`,
# whatever `gic` says), three boots on `ncpu` processors. The device tree's self-test boot:
# agintc0 finds one redistributor per CPU, takes SGI 0 for the IPIs (` ipi 0`), its ITS
# attaches below it (agintcmsi0), every application processor hatches, sees the TLB
# shootdown and takes an `ARM_IPI_NOP` through ICC_SGI1R, and each CPU runs its own clock
# interrupts (a PPI at its redistributor). Then `smoke-nvme`'s arm64 boot: the NVMe root on
# `virt`'s PCI bus interrupts by MSI-X through the ITS (the pcie node's `msi-map`: MAPD,
# MAPTI, an LPI from 8192, which `vmstat -i` counts) and the session logs in and does I/O.
# Then ACPI's (`--acpi`, booted by efiboot, whose efiacpi builds the GIC, its redistributors
# and the ITS from the MADT): the disk and the NIC are virtio-pci functions with one MSI-X
# vector per queue through the ITS (the IORT names it by its `openbsd,gic-its-id`), the
# root on the PCI disk logs in and pings the host. Part of `smoke`.
smoke-gicv3: (build-arm64 "--features qemu,multiprocessor") build-init-arm64 efiboot-arm64
    @test -x target/userland/arm64/host/bin/makefs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-gicv3: no arm64 userland; run just userland first"; exit 1; }
    cargo xtask smoke {{reject}} {{smp}} --gic 3 --arch arm64 --kernel target/{{arm64}}/debug/bsd --ramdisk none \
        --expect "bsd: {{ncpu}} processors" \
        --expect "agintc0 at mainbus0 shift 4:4 nirq 288 nredist {{ncpu}} ipi 0" \
        --expect "agintcmsi0 at agintc0" --expect "cpu{{aps}} at mainbus0 mpidr {{aps}}: ARM Cortex-A72" \
        --expect "cpu: {{aps}} of {{aps}} application processors running, tlb shootdown seen by {{aps}}, ipi nop seen by {{aps}}" \
        --expect "selftest: {{ncpu}} cpus running" \
        --expect "selftest: clockintr on {{ncpu}} cpus ok, uptime monotonic on each" \
        --expect "selftest: ping 10.0.2.2: echo reply received" \
        --expect "init exited with status 0 (signal 0)" --reject "ampintc0" --reject "unported: cpu_idcache"
    cargo xtask nvme-root --arch arm64 --duid {{nvme_duid}} --root-dev sd2a
    cargo xtask smoke {{reject}} {{smp}} --gic 3 --arch arm64 --kernel target/{{arm64}}/debug/bsd --ramdisk none --expect-ramdisk \
        --nvme nvme-arm64.img --cmdline "bootduid={{nvme_duid}}" --until-seen \
        {{disk_login}} \
        --send-after '# ' --send 'sysctl hw.ncpu\n' \
        --send-after 'hw.ncpu=' --send 'echo m16f-its-$((40+2)) >/m16f.txt && cat /m16f.txt\n' \
        --send-after '# ' --send 'dd if=/dev/rsd2c of=/dev/null bs=64k count=64\n' \
        --send-after '# ' --send 'vmstat -i\n' \
        --expect "agintc0 at mainbus0 shift 4:4 nirq 288 nredist {{ncpu}} ipi 0" \
        --expect "agintcmsi0 at agintc0" \
        --expect "nvme0 at pci0 dev 1 function 0 vendor 0x1b36 product 0x0010 rev 0x02: msix, NVMe 1.4" \
        --expect "root on sd2a ({{nvme_duid}}.a) swap on sd2b dump on sd2b" \
        --expect "rc: multi-user" --expect "login:" --expect "hw.ncpu={{ncpu}}" --expect "m16f-its-42" \
        --expect "4194304 bytes transferred" --expect "irq8192/nvme0" \
        --reject "mount -uw / failed" --reject "command queue timeout"
    cargo xtask efiboot-disk --arch arm64 --efi target/efiboot/arm64/BOOTAA64.EFI --kernel target/{{arm64}}/debug/bsd --root-dev sd0a
    cargo xtask smoke {{reject}} {{smp}} --gic 3 --arch arm64 --acpi --until-seen \
        --send-after 'boot> ' --send 'boot\n' \
        {{disk_login}} \
        --send-after '# ' --send 'sysctl hw.ncpu\n' \
        --send-after 'hw.ncpu=' --send 'ping -c 1 10.0.2.2\n' \
        --send-after 'packet loss' --send 'vmstat -i\n' \
        --expect "bsd: booted on arm64 by boot(8) efiboot" --expect "mainbus0 at root: ACPI" \
        --expect "agintc0 at mainbus0 shift 4:4 nirq 288 nredist {{ncpu}} ipi 0" \
        --expect "agintcmsi0 at agintc0" --expect "acpipci0 at acpi0 PCI0" \
        --expect "virtio0 at pci0 dev 1 function 0 vendor 0x1af4 product 0x1001" \
        --expect "vio0 at virtio1: 1 queue, address 52:54:00:12:34:56" --expect "virtio1: msix per-VQ" \
        --expect "cpu: {{aps}} of {{aps}} application processors running, tlb shootdown seen by {{aps}}, ipi nop seen by {{aps}}" \
        --expect "rc: multi-user" --expect "login:" --expect "hw.ncpu={{ncpu}}" \
        --expect "1 packets transmitted, 1 packets received, 0.0% packet loss" --expect "irq8193/vioblk0:" --expect "irq8195/vio0:1" \
        --reject "command queue timeout"

# M13: power off and reset through ACPI (dev/acpi/acpi.c, arch/amd64/amd64/acpi_machdep.c).
# acpi0 at bios0 takes q35 over from the firmware. The first boot logs in and runs `halt -p`
# (reboot(8)'s halt link: RB_HALT | RB_POWERDOWN), which goes down through acpi_powerdown:
# _PTS(5), then \_S5_'s SLP_TYP with SLP_EN in PM1a_CNT, and QEMU powers the machine off and
# exits with status 0 (`--status 0`). A reset also ends QEMU with 0 under -no-reboot, so the
# run rejects `rebooting...`, which boot(9) prints before every reset, and the halt message
# and the S5 panic. The second boot runs QEMU without -no-reboot (`--reboot`, hwopts.rs):
# `reboot` resets through cpu_reset's cpuresetfn, acpi_reset (the FADT's reset register,
# 0xcf9 on q35), the firmware boots the kernel again, and the second session runs a command.
# arm64 does the same through psci(4) (dev/fdt/psci.c, `psci0 at mainbus0`): `halt -p` is
# PSCI SYSTEM_OFF (powerdownfn = psci_powerdown) and `reboot` is SYSTEM_RESET
# (cpuresetfn = psci_reset). Part of `smoke`.
smoke-power: (build-amd64 "--features qemu,multiprocessor") (build-arm64 "--features qemu,multiprocessor") build-init-amd64 build-init-arm64
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-power: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --status 0 \
        {{disk_login}} --send-after '# ' --send 'sysctl machdep.lidaction machdep.pwraction machdep.tscfreq\n' \
        --send-after '# ' --send 'halt -p\n' \
        --expect "acpi0 at bios0: ACPI 3.0" --expect "acpi0: sleep states S3 S4 S5" \
        --expect "rc: multi-user" --expect "machdep.lidaction=1" --expect "machdep.pwraction=1" \
        --expect "machdep.tscfreq=" --expect "halt -p" \
        --reject "sysctl: Function not implemented" --reject "rebooting..." --reject "The operating system has halted" \
        --reject "acpi S5 transition did not happen"
    cargo xtask smoke {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --reboot --until-seen \
        {{disk_login}} --send-after '# ' --send 'reboot\n' \
        --send-after 'login:' --send 'root\n' --send-after 'Password:' --send 'emibsd\n' \
        --send-after '# ' --send 'echo m13-power-$((40+2))\n' \
        --expect "acpi0 at bios0: ACPI 3.0" --expect "rebooting..." --expect "m13-power-42" \
        --reject "The operating system has halted"
    cargo xtask smoke {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --expect-ramdisk --status 0 \
        {{disk_login}} --send-after '# ' --send 'halt -p\n' \
        --expect "psci0 at mainbus0: PSCI 1." --expect "rc: multi-user" --expect "halt -p" \
        --expect "Attempting to power down..." \
        --reject "rebooting..." --reject "The operating system has halted"
    cargo xtask smoke {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --expect-ramdisk --reboot --until-seen \
        {{disk_login}} --send-after '# ' --send 'reboot\n' \
        --send-after 'login:' --send 'root\n' --send-after 'Password:' --send 'emibsd\n' \
        --send-after '# ' --send 'echo m13-power-$((40+2))\n' \
        --expect "psci0 at mainbus0: PSCI 1." --expect "rebooting..." --expect "m13-power-42" \
        --reject "The operating system has halted"

# M16f: QEMU's power key on arm64 `virt` (dev/fdt/plgpio.c, gpiokeys.c, dev/ofw/ofw_gpio.c).
# The key is the `gpio-keys` node on the PL061: plgpio(4) registers the controller, gpiokeys(4)
# attaches the key ("GPIO Key Poweroff"). plgpio has no interrupts, so gpiokeys polls the key
# once a second through gpiokeys_update_key, which acts only on a lid switch: QEMU's
# `system_powerdown` (sent on the monitor, `--monitor`, hwopts.rs) changes nothing, and the
# session goes on. That is what OpenBSD 8.0 does on the same machine (`cargo xtask
# diff-openbsd --arch arm64 powerbtn`: plgpio0 and gpiokeys0 attach, the console stays silent
# and the VM still runs a minute later), so the port keeps it (the user's option A of
# 2026-10-08). arm64 only (amd64's power button is ACPI's). Part of `smoke`.
smoke-powerbtn: (build-arm64 "--features qemu,multiprocessor") build-init-arm64
    @test -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-powerbtn: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --expect-ramdisk --until-seen \
        {{disk_login}} --send-after '# ' --send 'echo m16f-armed-$((40+1))\n' \
        --monitor-after 'm16f-armed-41' --monitor 'system_powerdown' \
        --send-after 'm16f-armed-41' --send 'sleep 5; echo m16f-still-up-$((40+2))\n' \
        --expect "plgpio0 at mainbus0" --expect 'gpiokeys0 at mainbus0: "GPIO Key Poweroff"' \
        --expect "rc: multi-user" --expect "m16f-armed-41" --expect "m16f-still-up-42" \
        --reject "syncing disks" --reject "Attempting to power down" --reject "The operating system has halted"

# M13: the date comes from the RTC. Each arch boots single user from the ramdisk (`-s`) and
# ksh(1) compares `date +%s` right after boot with the host's clock ({host-ms}, hwopts.rs):
# within 60 s. amd64's time is the mc146818 (`inittodr`, arch/amd64/isa/clock.c); arm64's is
# efi0's GetTime (EDK2 disables the pl031 node, plrtc(4) does not attach). Part of `smoke`.
smoke-rtc: (build-amd64 "--features qemu,multiprocessor") (build-arm64 "--features qemu,multiprocessor")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-rtc: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --cmdline "-s" --expect-ramdisk --until-seen {{rtc_steps}}
    cargo xtask smoke {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --cmdline "-s" --expect-ramdisk --until-seen {{rtc_steps}}

# `smoke-rtc`'s session.
rtc_steps := "--send-after 'RETURN for sh:' --send '\\n' " + \
    "--send-after '# ' --send 'h={host-ms}; g=$(date +%s); d=$((g - h/1000)); echo \"rtc: guest $g, host $((h/1000)), diff $d\"\\n' " + \
    "--send-after '# ' --send '[ ${d#-} -le 60 ] && echo rtc-ok-$((40+2))\\n' " + \
    "--expect 'rtc: guest ' --expect 'rtc-ok-42'"

# M13 (acpitimer, acpihpet): the clock keeps the host's rate. Each arch boots single user
# from the ramdisk (`-s`) and ksh(1) reads date(1) beside the host's clock before and after
# `sleep 45` (xtask replaces `{host-ms}` by the host's time in milliseconds as the line goes
# out, tools/xtask/src/hwopts.rs; the two reading lines have the same length, so their
# serial delays cancel); the guest's elapsed seconds must be within 2 of the host's. Before
# acpitimer/acpihpet, amd64's TSC was measured against the i8254 and, under load, read
# 1.2-1.3 GHz for ~1.0, so the clock ran 20-30 % slow. amd64 also expects acpitimer0,
# acpihpet0 and the TSC's calibration line (`tsc: calibrated against acpihpet0: <N> Hz`, or,
# when every round is disturbed, the failure line: the TSC then keeps quality -1000, as in
# OpenBSD, and acpihpet0 is the timecounter). `sysctl kern.timecounter` is printed. Run it
# while the host is busy (`just smoke` does) to see the load case. Part of `smoke`.
smoke-clock: (build-amd64 "--features qemu,multiprocessor") (build-arm64 "--features qemu,multiprocessor")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-clock: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --cmdline "-s" --expect-ramdisk --until-seen \
        --expect "acpitimer0 at acpi0: 3579545 Hz, 24 bits" --expect "acpihpet0 at acpi0: 100000000 Hz" \
        --expect "tsc: calibrat" {{clock_steps}}
    cargo xtask smoke {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --cmdline "-s" --expect-ramdisk --until-seen \
        {{clock_steps}}

# `smoke-clock`'s session.
clock_steps := "--send-after 'RETURN for sh:' --send '\\n' " + \
    "--send-after '# ' --send 'h0={host-ms}; g0=$(date +%s)\\n' " + \
    "--send-after '# ' --send 'sleep 45; echo clock-slept-$((40+2))\\n' " + \
    "--send-after 'clock-slept-42' --send 'h1={host-ms}; g1=$(date +%s)\\n' " + \
    "--send-after '# ' --send 'd=$((g1-g0)); e=$(((h1-h0+500)/1000)); echo \"clock: guest $d s, host $e s\"\\n' " + \
    "--send-after '# ' --send '[ $((d-e)) -le 2 -a $((e-d)) -le 2 ] && echo clock-ok-$((40+2))\\n' " + \
    "--send-after '# ' --send 'sysctl kern.timecounter\\n' " + \
    "--send-after '# ' --send 'sysctl machdep\\n' " + \
    "--expect 'clock: guest ' --expect 'clock-ok-42' --expect 'kern.timecounter.choice=' " + \
    "--expect 'machdep.lidaction=' --reject 'sysctl: Function not implemented'"

# M10d: FUSE (sys/miscfs/fuse).Our own read-only file system, tools/fusehello (linked to
# OpenBSD's libfuse, which opens /dev/fuse0 and mounts fusefs), is mounted on /fuse; mount(8)
# must list it as `fuse`, its two files read back through the daemon (hello.txt and
# sub/deep.txt), ls(1) lists both directories, a write is refused, and after umount(8) the
# mount is gone. Both archs. Part of `smoke`.
smoke-fuse: (build-amd64 "--features qemu,multiprocessor") (build-arm64 "--features qemu,multiprocessor")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-fuse: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen {{fuse_steps}}
    cargo xtask smoke {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --expect-ramdisk --until-seen {{fuse_steps}}

# `smoke-fuse`'s session.
fuse_steps := disk_login + " " + \
    "--send-after '# ' --send 'mkdir -p /fuse && fusehello /fuse && mount && echo fuse-up-$((40+2))\\n' " + \
    "--send-after 'fuse-up-42' --send 'cat /fuse/hello.txt /fuse/sub/deep.txt\\n' " + \
    "--send-after '# ' --send 'ls -l /fuse /fuse/sub\\n' " + \
    "--send-after '# ' --send '(echo x >/fuse/new.txt) || echo fuse-ro-$((40+2))\\n' " + \
    "--send-after '# ' --send 'umount /fuse && echo fuse-umount-$((40+2))\\n' " + \
    "--send-after 'fuse-umount-42' --send 'case \"$(mount)\" in *fuse*) echo still;; *) echo fuse-gone-$((40+2));; esac\\n' " + \
    "--expect 'on /fuse type fuse' --expect 'fuse-up-42' --expect 'm10d-fuse-42' --expect 'm10d-fuse-sub-42' " + \
    "--expect 'hello.txt' --expect 'deep.txt' --expect 'fuse-ro-42' --expect 'fuse-umount-42' --expect 'fuse-gone-42'"

# M10d: NTFS, read-only, amd64 only (OpenBSD builds ntfs and mount_ntfs(8) for alpha, amd64 and
# i386). The ramdisk's /root/images/ntfs.img (an NTFS volume made on this machine by
# `cargo xtask ntfs-image`, tools/xtask/src/ntfsgen.rs) is attached to vnd(4) and mounted with
# mount_ntfs(8); ls(1) lists the root, the small file (resident in its MFT record) reads back,
# and so do the 500 lines of the big one (non-resident, three clusters). Part of `smoke`.
smoke-ntfs: (build-amd64 "--features qemu,multiprocessor")
    @test -f target/userland/amd64/ramdisk.ffs || \
        { echo "smoke-ntfs: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen {{ntfs_steps}}

# `smoke-ntfs`'s session.
ntfs_steps := disk_login + " " + \
    "--send-after '# ' --send 'vnconfig vnd0 /root/images/ntfs.img && mount_ntfs /dev/vnd0c /mnt && mount\\n' " + \
    "--send-after '# ' --send 'ls /mnt; cat /mnt/m10d-ntfs.txt\\n' " + \
    "--send-after '# ' --send 'n=0; while read l; do x=$l; n=$((n+1)); done </mnt/m10d-ntfs-big.txt; echo \"lines $n $x\"\\n' " + \
    "--send-after '# ' --send 'umount /mnt && vnconfig -u vnd0 && echo ntfs-done-$((40+2))\\n' " + \
    "--expect '/dev/vnd0c on /mnt type ntfs (local, read-only)' --expect 'm10d-ntfs-42' " + \
    "--expect 'lines 500 m10d-ntfs-big-line-0499' --expect 'ntfs-done-42'"

# M11a: the MULTIPROCESSOR kernel on four processors (`smp4`), per arch: every CPU attaches
# and runs (`selftest: 4 cpus running`, the IPI and TLB shootdown check of each machine), the
# default boot's init stand-in passes on it, `selftest=kthread` ping-pongs across two CPUs and
# `selftest=mpstress` hammers the pools (with their per-CPU caches) and uvm_pmemrange from a
# thread pegged to each CPU; since M11e a third phase has each pegged thread, without the
# kernel lock, fault pageable kernel memory in through the trap path and a shared
# copy-on-write anonymous map through uvm_fault, check, unmap and remap it (the page queues,
# amaps, pmap locks, per-CPU page caches and TLB shootdowns on four CPUs). M11b (MP timekeeping), in the default boots: amd64 runs tsc.c's
# synchronisation test against each application processor and prints a line per AP whatever
# the verdict (`tsc: cpu0/cpuN: sync test passed`, `... failed` or `... not run`; QEMU's TCG
# passes it), and on both archs every CPU dispatches its own clock interrupts with an uptime
# that never goes back on it (`selftest: clockintr on 4 cpus ok`, plus the `uptime went
# backwards` reject) and the init stand-in's time checks pass. Since M13 the amd64 processors
# come from ACPI's MADT (`acpimadt0`, which attaches them and `ioapic0`), no longer from the
# bootloader's list, and a `selftest=vio` boot whose virtio-net offers multiqueue
# (`--vio-mq`: `mq=on`; QEMU's user network has one queue pair) takes vio(4)'s intrmap(9)
# path: the configuration, control and queue-pair interrupts on their own MSI-X vectors,
# which the driver establishes itself (`virtio0: msix`, not virtio_pci's `msix per-VQ`),
# and the frame still comes back through the queue's interrupt. Part of `smoke`.
smoke-mp: (build-amd64 "--features qemu,multiprocessor") (build-arm64 "--features qemu,multiprocessor") build-init-amd64 build-init-arm64
    cargo xtask smoke {{reject}} {{smp4}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --ramdisk none \
        --expect "bsd: 4 processors" --expect "acpimadt0 at acpi0 addr 0xfee00000: PC-AT compat" \
        --expect "cpu0 at mainbus0: apid 0 (boot processor)" \
        --expect "ioapic0 at mainbus0: apid 0 pa 0xfec00000, version 20, 24 pins" \
        --expect "cpu3 at mainbus0: apid 3 (application processor)" \
        --expect "x86_ipi_selftest: X86_IPI_NOP taken by 3 cpus, tlb shootdowns acknowledged" \
        --expect "tsc: cpu0/cpu1: sync test" --expect "tsc: cpu0/cpu2: sync test" \
        --expect "tsc: cpu0/cpu3: sync test" \
        --expect "selftest: 4 cpus running" --expect "init: processes ok" \
        --expect "selftest: clockintr on 4 cpus ok, uptime monotonic on each" \
        --expect "init: time ok" --expect "init: uptime monotonic ok" \
        --expect "init exited with status 0 (signal 0)"
    cargo xtask smoke {{reject}} {{smp4}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --ramdisk none \
        --cmdline "selftest=kthread" --expect "selftest: kthread ping-pong ok" --expect ", across cpu"
    cargo xtask smoke {{reject}} {{smp4}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --ramdisk none \
        --cmdline "selftest=mpstress" --expect "selftest: mpstress pool ok (4 cpus" \
        --expect "selftest: mpstress pmemrange ok (4 cpus" --expect "selftest: mpstress uvm ok (4 cpus"
    cargo xtask smoke {{reject}} {{smp4}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --ramdisk none \
        --vio-mq --cmdline "selftest=vio" --reject "virtio0: msix per-VQ" \
        --expect "vio0 at virtio0: 1 queue, address 52:54:00:12:34:56" --expect "virtio0: msix" \
        --expect "selftest: vio up ok" --expect "selftest: vio rx ok"
    cargo xtask smoke {{reject}} {{smp4}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --ramdisk none \
        --expect "bsd: 4 processors" --expect "cpu0 at mainbus0 mpidr 0: ARM Cortex-A72" \
        --expect "cpu3 at mainbus0 mpidr 3: ARM Cortex-A72" \
        --expect "cpu: 3 of 3 application processors running, tlb shootdown seen by 3, ipi nop seen by 3" \
        --expect "selftest: 4 cpus running" --expect "init: processes ok" \
        --expect "selftest: clockintr on 4 cpus ok, uptime monotonic on each" \
        --expect "init: time ok" --expect "init: uptime monotonic ok" \
        --expect "init exited with status 0 (signal 0)"
    cargo xtask smoke {{reject}} {{smp4}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --ramdisk none \
        --cmdline "selftest=kthread" --expect "selftest: kthread ping-pong ok" --expect ", across cpu"
    cargo xtask smoke {{reject}} {{smp4}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --ramdisk none \
        --cmdline "selftest=mpstress" --expect "selftest: mpstress pool ok (4 cpus" \
        --expect "selftest: mpstress pmemrange ok (4 cpus" --expect "selftest: mpstress uvm ok (4 cpus"

# M11c: ddb(4) on the MULTIPROCESSOR kernel with `ncpu` processors (`{{smp}}`), per arch, from
# the ffs ramdisk booted `-ds`. `-d` stops at `ddb{0}> ` before the application processors
# exist and, after the empty line the `-d` smokes type first (arm64's early PL011), `continue`
# goes on; in the single-user shell `sysctl ddb.console=1` and
# `sysctl ddb.trigger=1` enter ddb with every CPU running, the OpenBSD way. The CPU that runs
# sysctl(8) varies, so `machine ddbcpu V` (`ddbmp_via1`: 2 on four CPUs, 0 on two; a CPU
# asked to switch to itself says `Invalid cpu` and stays) then `machine ddbcpu 1` always ends
# in a switch to CPU 1, where `machine cpuinfo` shows the others stopped. After `continue` a
# second trigger, `ddbcpu <last>` (3 or 1), `ddbcpu 0` and `cpuinfo` show CPU 1 stopped again
# (it resumed and took the new IPI), and after the second `continue` the shell answers. Needs
# `just userland`. Part of `smoke`.
smoke-ddbmp: (build-amd64 "--features qemu,multiprocessor") (build-arm64 "--features qemu,multiprocessor") build-init-amd64 build-init-arm64
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-ddbmp: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --cmdline "-ds" \
        --expect-ramdisk --until-seen \
        --send-after "ddb{0}> " --send '\n' --send-after "ddb{0}> " --send 'continue\n' \
        --send-after "RETURN for sh:" --send '\n' \
        --send-after "# " --send 'sysctl ddb.console=1\n' \
        --send-after "ddb.console: 0 -> 1" --send 'sysctl ddb.trigger=1\n' \
        --send-after "ddb{" --send 'machine ddbcpu {{ddbmp_via1}}\n' \
        --send-after "{{ddbmp_via1_prompt}}" --send 'machine ddbcpu 1\n' \
        --send-after "ddb{1}> " --send 'machine cpuinfo\n' \
        --send-after "ddb{1}> " --send 'continue\n' \
        --send-after "# " --send 'sysctl ddb.trigger=1\n' \
        --send-after "ddb{" --send 'machine ddbcpu {{aps}}\n' \
        --send-after "{{ddbmp_last_prompt}}" --send 'machine ddbcpu 0\n' \
        --send-after "ddb{0}> " --send 'machine cpuinfo\n' \
        --send-after "ddb{0}> " --send 'continue\n' \
        --send-after "# " --send 'echo cpus-$((2+2))-resumed\n' \
        --expect "bsd: {{ncpu}} processors" --expect "Stopped at" \
        --expect "    0: stopped" --expect "*   1: ddb" {{ddbmp_others}} \
        --expect "*   0: ddb" --expect "    1: stopped" \
        --expect "cpus-4-resumed"
    cargo xtask smoke {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --cmdline "-ds" \
        --expect-ramdisk --until-seen \
        --send-after "ddb{0}> " --send '\n' --send-after "ddb{0}> " --send 'continue\n' \
        --send-after "RETURN for sh:" --send '\n' \
        --send-after "# " --send 'sysctl ddb.console=1\n' \
        --send-after "ddb.console: 0 -> 1" --send 'sysctl ddb.trigger=1\n' \
        --send-after "ddb{" --send 'machine ddbcpu {{ddbmp_via1}}\n' \
        --send-after "{{ddbmp_via1_prompt}}" --send 'machine ddbcpu 1\n' \
        --send-after "ddb{1}> " --send 'machine cpuinfo\n' \
        --send-after "ddb{1}> " --send 'continue\n' \
        --send-after "# " --send 'sysctl ddb.trigger=1\n' \
        --send-after "ddb{" --send 'machine ddbcpu {{aps}}\n' \
        --send-after "{{ddbmp_last_prompt}}" --send 'machine ddbcpu 0\n' \
        --send-after "ddb{0}> " --send 'machine cpuinfo\n' \
        --send-after "ddb{0}> " --send 'continue\n' \
        --send-after "# " --send 'echo cpus-$((2+2))-resumed\n' \
        --expect "bsd: {{ncpu}} processors" --expect "Stopped at" \
        --expect "    0: stopped" --expect "*   1: ddb" {{ddbmp_others}} \
        --expect "*   0: ddb" --expect "    1: stopped" \
        --expect "cpus-4-resumed"

# `smoke-ddbmp`'s CPU-count dependent parts: the CPU the first trigger goes through on its way
# to CPU 1, and the lines CPU 1's `machine cpuinfo` shows for CPUs 2 and 3 on four processors.
ddbmp_via1 := if ncpu == "4" { "2" } else { "0" }
ddbmp_via1_prompt := "ddb{" + ddbmp_via1 + "}> "
ddbmp_last_prompt := "ddb{" + aps + "}> "
ddbmp_others := if ncpu == "4" { "--expect '    2: stopped' --expect '    3: stopped'" } else { "" }

# The uniprocessor kernels of `smoke-up`: built without MULTIPROCESSOR and kept as
# `target/<arch>/debug/bsd.up`. The MP kernel is rebuilt last, so `target/<arch>/debug/bsd`
# stays the MP one (cargo keeps both builds; switching back only relinks the file).
build-up:
    cargo build -p bsd --target {{amd64}} --features qemu
    cp target/{{amd64}}/debug/bsd target/{{amd64}}/debug/bsd.up
    cargo build -p bsd --target {{arm64}} --features qemu
    cp target/{{arm64}}/debug/bsd target/{{arm64}}/debug/bsd.up
    cargo build -p bsd --target {{amd64}} --features qemu,multiprocessor
    cargo build -p bsd --target {{arm64}} --features qemu,multiprocessor

# M11e: the one uniprocessor boot `smoke` keeps (the user's decision of 2026-10-03), per arch,
# to catch a dependency on MULTIPROCESSOR in the default kernel: the kernel built without the
# feature, kept as `bsd.up`, boots on one processor without a ramdisk and the init stand-in
# passes (`build-up` makes `bsd.up`). Since M13 amd64's processor comes from the MADT as the
# boot processor (`apid 0 (boot processor)`, as OpenBSD's `bsd.sp` prints it), no longer as
# the `(uniprocessor)` mainbus attached without tables. Part of `smoke`.
smoke-up: build-up build-init-amd64 build-init-arm64
    cargo xtask smoke {{reject}} --arch amd64 --kernel target/{{amd64}}/debug/bsd.up --ramdisk none \
        --expect "bsd: booted on amd64" --expect "acpimadt0 at acpi0 addr 0xfee00000: PC-AT compat" \
        --expect "cpu0 at mainbus0: apid 0 (boot processor)" \
        --expect "ioapic0 at mainbus0: apid 0 pa 0xfec00000, version 20, 24 pins" \
        --expect "selftest: malloc/pool stress ok" --expect "init: processes ok" \
        --expect "init: tcp ok" --expect "init: uptime monotonic ok" \
        --expect "init exited with status 0 (signal 0)"
    cargo xtask smoke {{reject}} --arch arm64 --kernel target/{{arm64}}/debug/bsd.up --ramdisk none \
        --expect "bsd: booted on arm64" --expect "cpu0 at mainbus0 mpidr 0: ARM Cortex-A72" \
        --expect "selftest: malloc/pool stress ok" --expect "init: processes ok" \
        --expect "init: tcp ok" --expect "init: uptime monotonic ok" \
        --expect "init exited with status 0 (signal 0)"

# M12: audio. Logs in as `smoke-login` does, shows audio(4)'s parameters with audioctl(8)
# and the mixer with mixerctl(8), plays `/root/tone.wav` with aucat(1) (through
# `/dev/audio0`: no sndiod(8) runs), and `--expect-tone` checks that QEMU's `-audiodev wav`
# file holds the tone (a tenth of a second of samples above 1000, devices.rs). Intel HD
# Audio (azalia(4): `intel-hda` with an `hda-output` codec) on both architectures, AC97
# (auich(4): `AC97`) on amd64, the only GENERIC with auich. Part of `smoke`.
smoke-audio: (build-amd64 "--features qemu,multiprocessor") (build-arm64 "--features qemu,multiprocessor")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-audio: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen \
        --audio hda --expect-tone {{audio_play}} \
        --expect 'azalia0 at pci0 dev 4 function 0 vendor 0x8086 product 0x2668' \
        --expect 'audio0 at azalia0' --expect 'name=azalia0' --expect 'outputs.master=126,126'
    cargo xtask smoke {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --expect-ramdisk --until-seen \
        --audio hda --expect-tone {{audio_play}} \
        --expect 'azalia0 at pci0 dev 1 function 0 vendor 0x8086 product 0x2668' \
        --expect 'audio0 at azalia0' --expect 'name=azalia0' --expect 'outputs.master=126,126'
    cargo xtask smoke {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen \
        --audio ac97 --expect-tone {{audio_play}} \
        --expect 'auich0 at pci0 dev 4 function 0 vendor 0x8086 product 0x2415' \
        --expect 'ac97: codec id 0x83847600 (SigmaTel STAC9700)' \
        --expect 'audio0 at auich0' --expect 'name=auich0' --expect 'outputs.master=255,255'

# `smoke-audio`'s session: the parameters and the mixer, then the tone.
audio_play := disk_login + " " + \
    "--send-after '# ' --send 'audioctl -f /dev/audioctl0; mixerctl -f /dev/audioctl0\\n' " + \
    "--send-after '# ' --send 'aucat -i /root/tone.wav && echo tone-$((40+2))\\n' " + \
    "--expect 'rate=48000' --expect 'encoding=s16le' --expect 'tone-42'"

# `smoke-audio`'s three tones, played live on the Mac's speakers through QEMU's `coreaudio`
# backend (`--speakers`, devices.rs) instead of being recorded: azalia(4) on amd64 and arm64,
# then auich(4) on amd64. Checked by ear, so not part of `smoke`. Each boot waits a second
# after aucat(1) so the host's buffer drains before the run stops.
play-audio: (build-amd64 "--features qemu,multiprocessor") (build-arm64 "--features qemu,multiprocessor")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "play-audio: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen \
        --audio hda --speakers {{audio_live}} --expect 'audio0 at azalia0'
    cargo xtask smoke {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --expect-ramdisk --until-seen \
        --audio hda --speakers {{audio_live}} --expect 'audio0 at azalia0'
    cargo xtask smoke {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen \
        --audio ac97 --speakers {{audio_live}} --expect 'audio0 at auich0'

# `play-audio`'s session: the tone, then a second for the host's buffer.
audio_live := disk_login + " " + \
    "--send-after '# ' --send 'aucat -i /root/tone.wav && sleep 1 && echo tone-$((40+2))\\n' " + \
    "--expect 'tone-42'"

# M12: USB. QEMU's `qemu-xhci` with a `usb-storage` stick and a `usb-kbd` (`--usb`,
# devices.rs): xhci(4), uhub(4), uhidev(4) and ukbd(4) for the keyboard, umass(4) below a
# scsibus, the stick as sd2 on both archs: on amd64 vioblk is sd0 and the boot image on q35's
# AHCI sd1 (M13; the hub is explored after autoconf, so the stick comes after ahci's disks),
# on arm64 the boot disk is sd1. Logs in as `smoke-login` does,
# mounts the stick's FAT partition with mount_msdos(8) (`i`, spoofed from its MBR), reads
# the note and checks the 1 MiB file's cksum(1) (made on the host, devices.rs), copies it,
# remounts and compares the copy. xhci interrupts by MSI-X on both archs (amd64 since M13's
# acpipci enables MSI on its bus). Part of `smoke`.
smoke-usb: (build-amd64 "--features qemu,multiprocessor") (build-arm64 "--features qemu,multiprocessor")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-usb: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen \
        --usb {{disk_login}} {{replace(usb_session, "SD", "sd2")}} {{usb_check}} \
        --expect 'xhci0 at pci0 dev 4 function 0 vendor 0x1b36 product 0x000d rev 0x01: msix' \
        --expect 'sd2 at scsibus2 targ 1 lun 0: <QEMU, QEMU HARDDISK, 2.5+>'
    cargo xtask smoke {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --expect-ramdisk --until-seen \
        --usb {{disk_login}} {{replace(usb_session, "SD", "sd2")}} {{usb_check}} \
        --expect 'xhci0 at pci0 dev 1 function 0 vendor 0x1b36 product 0x000d rev 0x01: msix' \
        --expect 'sd2 at scsibus2 targ 1 lun 0: <QEMU, QEMU HARDDISK, 2.5+>'

# `smoke-usb`'s session on the stick's disk `SD` (replaced per arch).
usb_session := "--send-after '# ' --send 'mount_msdos /dev/SDi /mnt && cat /mnt/M12USB.TXT && " + \
    "cksum /mnt/BIG.BIN && cp /mnt/BIG.BIN /mnt/COPY.BIN && umount /mnt && " + \
    "mount_msdos /dev/SDi /mnt && cmp /mnt/BIG.BIN /mnt/COPY.BIN && echo usb-$((40+2))\\n'"

# `smoke-usb`'s expectations, both archs.
usb_check := "--expect 'usb0 at xhci0: USB revision 3.0' --expect 'uhub0 at usb0' " + \
    "--expect 'umass0 at uhub0 port 1 configuration 1 interface 0 \"QEMU QEMU USB HARDDRIVE\"' " + \
    "--expect 'umass0: using SCSI over Bulk-Only' " + \
    "--expect 'uhidev0 at uhub0 port 6 configuration 1 interface 0 \"QEMU QEMU USB Keyboard\"' " + \
    "--expect 'ukbd0 at uhidev0' " + \
    "--expect 'emibsd m12: hello from a usb stick' --expect '4071711340 1048576 /mnt/BIG.BIN' " + \
    "--expect 'usb-42'"

# M16b: ehci(4). The M12 stick alone on QEMU's `usb-ehci` (an ICH4 EHCI function, 8086:24cd,
# `--usb-hc ehci`, devices.rs: a full speed usb-kbd does not fit on an EHCI with no companion
# controller): ehci(4) at pci with its INTx interrupt, usb(4) and the emulated root hub,
# umass(4) at high speed below a scsibus, the stick as sd2 on both archs, as in `smoke-usb`.
# Logs in, then `smoke-usb`'s session: mount_msdos(8), the note, the 1 MiB file's cksum(1), a
# copy, a remount and cmp(1).
# NOT in `smokes` yet: the faithful port meets two QEMU behaviours that the C meets too, left
# to the user (M16b report): on amd64 ehci_init raises INTx (port change) while the I/O APIC
# pin is still masked and edge-triggered (ioapic_addroute leaves the pin to ioapic_enable while
# cold, as in C), QEMU's I/O APIC drops masked edges, and the level never falls, so no
# interrupt is ever delivered ("uhub0: device problem, disabling port 1"); on arm64
# ehci_pci_attach's EOWRITE2(EHCI_USBINTR, 0) is a 16-bit write QEMU's EHCI operational
# registers refuse (4-byte accesses only), a synchronous external abort ("panic: uvm_fault
# failed ... far ...028" in generic_space_write_2). With both sidestepped by hand (status
# acked once, the write skipped) every line below was seen on both archs.
smoke-ehci: (build-amd64 "--features qemu,multiprocessor") (build-arm64 "--features qemu,multiprocessor")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-ehci: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen \
        --usb-hc ehci {{disk_login}} {{replace(usb_session, "SD", "sd2")}} {{ehci_check}} \
        --expect 'ehci0 at pci0 dev 4 function 0 vendor 0x8086 product 0x24cd rev 0x10: apic 0 int 23'
    cargo xtask smoke {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --expect-ramdisk --until-seen \
        --usb-hc ehci {{disk_login}} {{replace(usb_session, "SD", "sd2")}} {{ehci_check}} \
        --expect 'ehci0 at pci0 dev 1 function 0 vendor 0x8086 product 0x24cd rev 0x10'

# `smoke-ehci`'s expectations, both archs.
ehci_check := "--expect 'usb0 at ehci0: USB revision 2.0' " + \
    "--expect 'uhub0 at usb0 configuration 1 interface 0 \"vendor 0x8086 EHCI root hub\" rev 2.00/1.00 addr 1' " + \
    "--expect 'umass0 at uhub0 port 1 configuration 1 interface 0 \"QEMU QEMU USB HARDDRIVE\" rev 2.00/0.00 addr 2' " + \
    "--expect 'umass0: using SCSI over Bulk-Only' " + \
    "--expect 'sd2 at scsibus2 targ 1 lun 0: <QEMU, QEMU HARDDISK, 2.5+>' " + \
    "--expect 'emibsd m12: hello from a usb stick' --expect '4071711340 1048576 /mnt/BIG.BIN' " + \
    "--expect 'usb-42'"

# M13: com(4) over puc(4), amd64 only: arm64's GENERIC has no puc(4). QEMU's `pci-serial`
# (1b36:0002, a 16550 behind PCI, `--pci-serial`, hwopts.rs) is a file chardev: puc*
# attaches (`ports: 16 com`: puc_print_ports counts the card's empty port slots as com, in
# the C too) and com* below it takes the card's interrupt (INTx, acpiprt's routing) as
# `com4` (com0 to com3 are the ISA lines, `/dev/cua04` is in the ramdisk). The session logs
# in and writes a line to it, and `--expect-pci-serial` checks that the line reached the
# host file. Part of `smoke`.
smoke-puc: (build-amd64 "--features qemu,multiprocessor")
    @test -f target/userland/amd64/ramdisk.ffs || \
        { echo "smoke-puc: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen \
        --pci-serial puc-amd64.txt --expect-pci-serial 'm13-puc-42' {{disk_login}} \
        --send-after '# ' --send 'echo m13-puc-$((40+2)) >/dev/cua04 && echo puc-sent-$((40+2))\n' \
        --expect 'puc0 at pci0 dev 4 function 0 vendor 0x1b36 product 0x0002 rev 0x01: ports: 16 com' \
        --expect 'com4 at puc0 port 0 apic 0 int 20: ns16550a, 16 byte fifo' --expect 'puc-sent-42'

# M13: the frame buffer, both archs. The firmware's GOP (amd64: q35's standard VGA; arm64:
# `-device ramfb`, `--fb`, hwopts.rs) reaches the kernel through Limine's framebuffer
# request: efifb0 attaches at mainbus0 on amd64, simplefb0 on arm64 (at the
# `/chosen/framebuffer` node the boot glue adds, as efiboot does, sys/stand/fdtfb.rs), each
# with rasops on top and wsdisplay0 on it (a plain display: the serial line stays the
# console, as in OpenBSD with a serial console). `selftest=fb` draws "EmiBSD M13" through a
# screen's emulops, as wsdisplay does, and prints the text's box; `--screenshot-after` takes a
# QEMU `screendump` then and checks that the box holds exactly the glyphs' pixels in the
# text's colour and nothing but the background otherwise. Part of `smoke`.
fb_check := "--screenshot-after 'selftest: fb text at' --expect-ramdisk --until-seen " + \
    "--expect 'selftest: fb text at' --expect 'selftest: fb drew'"
smoke-fb: (build-amd64 "--features qemu,multiprocessor") (build-arm64 "--features qemu,multiprocessor")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-fb: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --cmdline "selftest=fb" {{fb_check}} \
        --expect 'bsd: framebuffer 1280x800, 32 bpp' --expect 'efifb0 at mainbus0: 1280x800, 32bpp' \
        --expect 'wsdisplay0 at efifb0 mux 1' --expect "selftest: fb drew 'EmiBSD M13' on efifb0"
    cargo xtask smoke {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --cmdline "selftest=fb" {{fb_check}} \
        --expect 'bsd: framebuffer 800x600, 32 bpp' --expect 'simplefb0 at mainbus0: 800x600, 32bpp' \
        --expect 'wsdisplay0 at simplefb0 mux 1' --expect "selftest: fb drew 'EmiBSD M13' on simplefb0"

# M13: wsdisplay(4), both archs. Booted with the frame buffer (`--fb` through
# `--screenshot-after`), wsdisplay0 attaches at efifb0 (amd64) or simplefb0 (arm64) with its six
# vt100 screens (WSDISPLAY_DEFAULTSCREENS=6), the serial line staying the console;
# `selftest=wscons` prints where the screens' character grid lies. After login the shell
# writes a line to /dev/ttyC0, screen 0's tty (the cdevsw 12 entry points, the line discipline,
# wsdisplaystart and the vt100 emulation drawing through rasops); `--screen-text` then checks
# the QEMU screendump for that text in the grid's first row. Part of `smoke`.
wscons_line := 'x=wrote; echo hello wscons > /dev/ttyC0 && echo wscons-$x\n'
smoke-wscons: (build-amd64 "--features qemu,multiprocessor") (build-arm64 "--features qemu,multiprocessor")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-wscons: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --cmdline "selftest=wscons" \
        --screenshot-after 'wscons-wrote' --screen-text '0:0:hello wscons' --expect-ramdisk --until-seen \
        --expect 'efifb0 at mainbus0: 1280x800, 32bpp' --expect 'wsdisplay0 at efifb0 mux 1' \
        --expect 'wsdisplay0: screen 0-5 added (std, vt100 emulation)' --expect 'selftest: wscons grid' \
        --send-after "login:" --send 'root\n' --send-after "Password:" --send 'emibsd\n' \
        --send-after "# " --send '{{wscons_line}}' --expect 'wscons-wrote'
    cargo xtask smoke {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --cmdline "selftest=wscons" \
        --screenshot-after 'wscons-wrote' --screen-text '0:0:hello wscons' --expect-ramdisk --until-seen \
        --expect 'simplefb0 at mainbus0: 800x600, 32bpp' --expect 'wsdisplay0 at simplefb0 mux 1' \
        --expect 'wsdisplay0: screen 0-5 added (std, vt100 emulation)' --expect 'selftest: wscons grid' \
        --send-after "login:" --send 'root\n' --send-after "Password:" --send 'emibsd\n' \
        --send-after "# " --send '{{wscons_line}}' --expect 'wscons-wrote'

# M13: vga(4) (vga0 at isa?, vga* at pci?, wsdisplay0 at vga? console 1), amd64 only: arm64
# GENERIC has no vga. QEMU's q35 brings its std VGA (PCI 1234:1111) by default, and OVMF has set
# it to a graphics mode for the GOP, so its legacy text memory at 0xb8000 reads back 0xffff and
# vga_common_probe fails at both buses: the VGA stays "not configured" and efifb0 takes the
# display, exactly as in an OpenBSD 8.0 snapshot's dmesg on the same machine (`just
# diff-openbsd`: '"Bochs VGA" rev 0x02 at pci0 dev 1 function 0 not configured', no vga line).
# The probe maps the legacy window, which Limine's direct map leaves out (bus_space.rs's
# isa_hole_mapped); a regression there panics this boot.
smoke-vga: (build-amd64 "--features qemu,multiprocessor")
    @test -f target/userland/amd64/ramdisk.ffs || \
        { echo "smoke-vga: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen \
        --reject 'vga0 at' --reject 'vga1 at' --reject 'wsdisplay at vga' --reject 'unported: vga_post_init' \
        --expect 'vendor 0x1234 product 0x1111 (class display subclass VGA, rev 0x02) at pci0 dev 1 function 0 not configured' \
        --expect 'isa0 at mainbus0' --expect 'com0 at isa0 port 0x3f8/8 irq 4: ns16550a, 16 byte fifo' \
        --expect 'efifb0 at mainbus0: 1280x800, 32bpp' --expect 'wsdisplay0 at efifb0 mux 1' \
        --expect 'wsdisplay0: screen 0-5 added (std, vt100 emulation)' --expect 'login:'

# M13: the keyboard half of wscons, both archs. Booted with the frame buffer (`--fb`) and
# M12's `qemu-xhci` with its `usb-kbd` (`--usb`): ukbd0 offers a wskbd child, `wskbd0 at
# ukbd0 mux 1` joins mux 1, the one wsdisplay0 reads, and connects to wsdisplay0 (the serial
# line stays the console, so the USB keyboard is not the console keyboard, as in OpenBSD).
# After login the shell opens /dev/ttyC0, screen 0's tty, prints a trigger and reads a line
# from it; QEMU's monitor then types `h`, `i` and Return on the USB keyboard (`--sendkey-after`,
# hwopts.rs): ukbd, hidkbd, wskbd_input, wskbd_translate (the US keymap of ukbdmap.rs),
# wsdisplay_kbdinput and the tty's line discipline bring "hi" to the reader, which echoes it
# on the serial line. Then dd(1) reads two events from /dev/wskbd0 (which takes the keyboard
# out of the mux into event mode, wsevent.c) while `a` is typed: a key down and a key up,
# 48 bytes. Part of `smoke`.
kbd_tty := 'exec 3</dev/ttyC0; echo kbd-ready-$((40+2)); read line <&3; echo "kbd-got-[$line]"\n'
kbd_ev := '(sleep 2; echo ev-ready-$((40+2))) & n=$(dd if=/dev/wskbd0 bs=24 count=2 2>/dev/null | wc -c); echo ev-bytes-$((n))\n'
kbd_check := "--fb --usb --expect-ramdisk --until-seen " + \
    "--sendkey-after 'kbd-ready-42' --sendkeys 'h i ret' --sendkey-after 'ev-ready-42' --sendkeys 'a' " + \
    disk_login + " --send-after '# ' --send '" + kbd_tty + "' --send-after 'kbd-got-[hi]' --send '" + kbd_ev + "' " + \
    "--expect 'ukbd0 at uhidev0' --expect 'wskbd0 at ukbd0 mux 1' " + \
    "--expect 'wskbd0: connecting to wsdisplay0' --expect 'kbd-got-[hi]' " + \
    "--expect 'wskbd0: disconnecting from wsdisplay0' --expect 'ev-bytes-48'"
smoke-kbd: (build-amd64 "--features qemu,multiprocessor") (build-arm64 "--features qemu,multiprocessor")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-kbd: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd {{kbd_check}} \
        --expect 'wsdisplay0 at efifb0 mux 1'
    cargo xtask smoke {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd {{kbd_check}} \
        --expect 'wsdisplay0 at simplefb0 mux 1'

# M16b: USB pointers, both archs. QEMU's `usb-mouse`, `usb-tablet` and `usb-wacom-tablet` on a
# `qemu-xhci` (`--usb-mouse --usb-tablet --usb-wacom-tablet`, devices.rs): uhidev(4) takes each,
# ums(4) attaches to all three (`uwacom(4)` matches only the four Wacom products of its table,
# not QEMU's PenPartner, product 0x0000) and offers a wsmouse child on mux 0. After login the
# shell reads two events (dd(1), 24 bytes each) from /dev/wsmouse0, 1 and 2 at once, and
# QEMU's monitor (`--monitor-after`, hwopts.rs) injects the pointer events: `mouse_set N` picks
# the device `info mice` numbers N, `mouse_move` moves the relative usb-mouse (a delta and the
# sync event), `mouse_button` presses and releases the tablet's button (QEMU's monitor cannot
# move an absolute device: the button is the event; the report becomes a button down and a
# sync event). The PenPartner sends its reports without the report ID byte its descriptor
# declares (QEMU's HID mode), so the first byte (the buttons) selects the report ID: with the
# left button down it is 1, ums2's, and the move that follows makes it a report of three
# bytes. wsmouse2 gets its two events from those. The monitor's numbering follows the
# machine: on amd64 the PS/2 mouse is #2 and the HID ones #3, #4 and #5; arm64 has #1, #2
# and #3. The PenPartner's other report IDs (2, 3 and 99, vendor collections) are left to
# uhid(4), which attaches to each (`uhid0` to `uhid2`).
mouse_dd := 'for i in 0 1 2; do (n=$(dd if=/dev/wsmouse$i bs=24 count=2 2>/dev/null | wc -c); echo mouse$i-bytes-$((n))) & done; sleep 2; echo mouse-ready-$((40+2))\n'
mouse_check := "--usb-mouse --usb-tablet --usb-wacom-tablet --expect-ramdisk --until-seen " + \
    disk_login + " --send-after '# ' --send '" + mouse_dd + "' " + \
    "--monitor-after 'mouse-ready-42' --monitor 'info mice' " + \
    "--monitor-after 'mouse-ready-42' --monitor 'mouse_set MOUSE' --monitor-after 'mouse-ready-42' --monitor 'mouse_move 5 3' " + \
    "--monitor-after 'mouse-ready-42' --monitor 'mouse_set TABLET' --monitor-after 'mouse-ready-42' --monitor 'mouse_button 1' --monitor-after 'mouse-ready-42' --monitor 'mouse_button 0' " + \
    "--monitor-after 'mouse-ready-42' --monitor 'mouse_set WACOM' --monitor-after 'mouse-ready-42' --monitor 'mouse_button 1' --monitor-after 'mouse-ready-42' --monitor 'mouse_move 5 3' --monitor-after 'mouse-ready-42' --monitor 'mouse_button 0' " + \
    "--expect 'ums0 at uhidev0: 5 buttons, Z dir' --expect 'wsmouse0 at ums0 mux 0' " + \
    "--expect 'ums1 at uhidev1: 5 buttons, Z dir' --expect 'wsmouse1 at ums1 mux 0' " + \
    "--expect 'ums2 at uhidev2 reportid 1: 3 buttons, Z dir' --expect 'wsmouse2 at ums2 mux 0' " + \
    "--expect 'uhid0 at uhidev2 reportid 2: input=7, output=0, feature=1' " + \
    "--expect 'uhid1 at uhidev2 reportid 3: input=0, output=0, feature=1' " + \
    "--expect 'uhid2 at uhidev2 reportid 99: input=7, output=0, feature=0' " + \
    "--expect 'mouse0-bytes-48' --expect 'mouse1-bytes-48' --expect 'mouse2-bytes-48'"
smoke-mouse: (build-amd64 "--features qemu,multiprocessor") (build-arm64 "--features qemu,multiprocessor")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-mouse: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd \
        {{replace(replace(replace(mouse_check, "MOUSE", "3"), "TABLET", "4"), "WACOM", "5")}}
    cargo xtask smoke {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd \
        {{replace(replace(replace(mouse_check, "MOUSE", "1"), "TABLET", "2"), "WACOM", "3")}}

# M16b: ugen(4), both archs. QEMU's `usb-ccid` smart card reader (`--usb-ccid`, devices.rs) on a
# `qemu-xhci`: no driver takes a CCID interface (class 0x0b), so usbd_probe_and_attach offers the
# device to the generic fallback and ugen0 attaches. After login `usbdevs -v` (usbdevs(8), over
# /dev/usb0's USB_DEVICEINFO) lists the reader with `driver: ugen0`; reading /dev/ugen0.00 (the
# control endpoint) fails with ENODEV, as ugen_do_read says; and a CCID PC_to_RDR_GetSlotStatus
# message (10 bytes: type 0x65, zeros) written to the bulk-out endpoint (/dev/ugen0.03) brings
# the RDR_to_PC_SlotStatus (10 bytes) back from the bulk-in one (/dev/ugen0.02): ugenopen,
# ugen_do_write, ugen_do_read, the synchronous bulk transfers (dd reads exactly 10 bytes: a
# shorter transfer than asked is an error without USB_SET_SHORT_XFER, as in the C). The `ugen_clear_iface_eps:
# clear endpoints failed!` lines are QEMU stalling CLEAR_FEATURE(ENDPOINT_HALT) on the CCID
# interface, which ugenopen sends before it opens a pipe (the C prints them too). Part of `smoke`.
ugen_session := 'usbdevs -v; dd if=/dev/ugen0.00 count=1 2>&1; echo -n "\\0145\\0\\0\\0\\0\\0\\0\\0\\0\\0" > /dev/ugen0.03; n=$(dd if=/dev/ugen0.02 bs=10 count=1 2>/dev/null | wc -c); echo ccid-$((n)); echo ugen-$((40+2))\n'
ugen_check := "--usb-ccid --expect-ramdisk --until-seen " + disk_login + " --send-after '# ' --send '" + ugen_session + "' " + \
    "--expect 'ugen0 at uhub0 port 5 \"QEMU QEMU USB CCID\" rev 1.10/0.00 addr 2' " + \
    "--expect 'addr 02: 08e6:4433 QEMU, QEMU USB CCID' --expect 'driver: ugen0' " + \
    "--expect 'dd: /dev/ugen0.00: Operation not supported by device' --expect 'ccid-10' --expect 'ugen-42'"
smoke-ugen: (build-amd64 "--features qemu,multiprocessor") (build-arm64 "--features qemu,multiprocessor")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-ugen: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd {{ugen_check}}
    cargo xtask smoke {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd {{ugen_check}}

# M16b: cdce(4), both archs. QEMU's `usb-net` (`--usb-net`, devices.rs) on a `qemu-xhci`, as the
# user network's NIC in vio0's place. It offers two configurations, RNDIS first and CDC Ethernet
# second; usbd_probe_and_attach tries each configuration in turn (nothing takes the RNDIS one:
# urndis(4) is not in this tree), and cdce0 attaches to the second, with the address the
# Ethernet descriptor's string gives. After login ifconfig(8) gives cdce0 10.0.2.15/24 (the
# kernel's self-test configures vio0 only), brings it up (SIOCSIFADDR runs cdce_init: the
# interrupt pipe, the bulk pipes, the receive transfer) and ping(8) gets the gateway's reply
# through cdce_start/cdce_txeof and cdce_rxeof. Part of `smoke`.
cdce_session := "--send-after '# ' --send 'ifconfig cdce0 inet 10.0.2.15 netmask 255.255.255.0 up && ifconfig cdce0\\n' " + \
    "--send-after '# ' --send 'ping -c 1 10.0.2.2\\n'"
cdce_check := "--usb-net --expect-ramdisk --until-seen " + disk_login + " " + cdce_session + " " + \
    "--expect 'cdce0 at uhub0 port 5 configuration 1 interface 0 \"QEMU RNDIS/QEMU USB Network Device\" rev 2.00/0.00 addr 2' " + \
    "--expect 'cdce0: address 52:54:00:12:34:56' --expect 'cdce0: flags=' " + \
    "--expect 'inet 10.0.2.15 netmask 0xffffff00' " + em_ping
smoke-cdce: (build-amd64 "--features qemu,multiprocessor") (build-arm64 "--features qemu,multiprocessor")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-cdce: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd {{cdce_check}}
    cargo xtask smoke {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd {{cdce_check}}

# annotate a stack trace (paste it on stdin) with the debug kernel's symbols
symbolize arch:
    cargo xtask symbolize --arch {{arch}}

# --- userland (M8) ---------------------------------------------------------------

# Cross-compile OpenBSD's libc, init(8), ksh(1), cat(1), echo(1), ls(1) and uname(1), unmodified,
# and makefs(8) for the host, from the reference sources into target/userland/<arch> (with the
# ffs ramdisk image) with Apple clang and LLD 17 (docs/SETUP.md, "Userland
# toolchain"). Slow and tool-dependent, so not part of `ci`.
userland:
    cargo xtask userland --arch amd64
    cargo xtask userland --arch arm64

# M12+: the same scenarios on EmiBSD and on a real OpenBSD VM, compared step by step
# (`cargo xtask diff-openbsd`, tools/xtask/src/diffopenbsd.rs; the scenarios and the expected
# differences are in tools/xtask/diff-openbsd/). The first run downloads the OpenBSD snapshot
# recorded in tools/xtask/openbsd-snapshot.toml and installs it with autoinstall(8) into
# target/openbsd/ (once per arch, kept); later runs boot it with `-snapshot` beside the smokes'
# MP kernel. Its files go to target/diff-openbsd unless EMIBSD_RUN_DIR says otherwise. Needs
# `just userland`. Beside `ci`, not in it (timings in docs/ARCHITECTURE.md, "diff-openbsd").
diff-openbsd: (build-amd64 "--features qemu,multiprocessor") (build-arm64 "--features qemu,multiprocessor") build-init-amd64 build-init-arm64
    @test -f target/userland/amd64/root/usr/bin/difftest -a -f target/userland/arm64/root/usr/bin/difftest || \
        { echo "diff-openbsd: no difftest in target/userland; run just userland first"; exit 1; }
    EMIBSD_RUN_DIR=${EMIBSD_RUN_DIR:-target/diff-openbsd} cargo xtask diff-openbsd {{smp4}}

# --- the comp set (M14) --------------------------------------------------------------

# Job count of `comp`: about half the Mac's cores by default (other builds share it).
comp_jobs := env("COMP_JOBS", "6")

# OpenBSD's compiler for EmiBSD: clang, lld, libc++, libc++abi, libpthread and LLVM's tools
# from gnu/llvm (Apache-2.0 WITH LLVM-exception, compiled unmodified) by OpenBSD's own build
# glue (gnu/usr.bin/clang, gnu/lib/libcxx, gnu/lib/libcxxabi, gnu/lib/libclang_rt), into
# target/comp/<arch> (root/ the staging root, comp.ffs its disk image; userland/comp.rs and
# docs/ARCHITECTURE.md, "The comp set"). Needs `just userland`. Not part of `ci`: a first
# build compiles about 2,850 C++ files per arch (30 min for arm64 with 5 jobs, measured while
# other builds kept the Mac at a load average near 30), plus about 200 for the macOS build
# tools, once (under a minute); a run with nothing changed takes 15 to 20 s per arch.
# COMP_JOBS=N overrides the job count.
comp:
    cargo xtask comp --arch amd64 --jobs {{comp_jobs}}
    cargo xtask comp --arch arm64 --jobs {{comp_jobs}}

# M14: the compiler inside EmiBSD, without the installer. Boots the ramdisk kernel with the
# comp set's disk (`target/comp/<arch>/comp.ffs`, copied to the persistent disk set `comp`,
# so sd0), mounts it on /mnt and runs `/mnt/usr/bin/clang --version`, then compiles a hello
# world with `cc --sysroot=/mnt -static` (clang, lld, crt0, libc.a and the headers all from
# the disk) and runs it. Then the plain dynamic link, as on an installed system: chroot(8)
# into the disk, `cc -o /tmp/d /tmp/h.c` (a dynamic PIE: /usr/libexec/ld.so and
# libc.so.M.m, userland/shlib.rs), run in the chroot, and ldd(1) of it from the ramdisk
# (whose own /usr/libexec/ld.so and /usr/lib/libc.so.M.m serve it). The C has no double quotes
# (the string is a char array) to keep the shell quoting simple; every line stays under
# arm64's 128-byte console limit. Not in `smokes`: it needs `just comp`, which is not part of
# `ci`. Time limits are five times the usual (`EMIBSD_TIMEOUT_SCALE`): clang runs under TCG.
smoke-cc: (build-amd64 "--features qemu,multiprocessor") (build-arm64 "--features qemu,multiprocessor")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-cc: no ramdisk image; run just userland first"; exit 1; }
    @test -f target/comp/amd64/comp.ffs -a -f target/comp/arm64/comp.ffs || \
        { echo "smoke-cc: no comp image; run just comp first"; exit 1; }
    cp target/comp/amd64/comp.ffs target/disk-amd64-comp.img
    EMIBSD_TIMEOUT_SCALE=5 cargo xtask smoke {{reject}} {{smp}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen \
        --disk-set comp {{cc_script}} --expect 'Target: amd64-unknown-openbsd8.0'
    cp target/comp/arm64/comp.ffs target/disk-arm64-comp.img
    EMIBSD_TIMEOUT_SCALE=5 cargo xtask smoke {{reject}} {{smp}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --expect-ramdisk --until-seen \
        --disk-set comp {{cc_script}} --expect 'Target: aarch64-unknown-openbsd8.0'

# `smoke-cc`'s session.
cc_script := disk_login + " " + \
    "--send-after '# ' --send 'mount /dev/sd0a /mnt && echo cc-mnt-$((40+2))\\n' " + \
    "--send-after 'cc-mnt-42' --send '/mnt/usr/bin/clang --version\\n' " + \
    "--send-after 'InstalledDir' --send 'print -r \"#include <stdio.h>\" >/tmp/h.c\\n' " + \
    "--send-after '# ' --send 'print -r \"int main(void){char s[]={99,99,45,111,107,0};\" >>/tmp/h.c\\n' " + \
    "--send-after '# ' --send 'print -r \"puts(s);return 0;}\" >>/tmp/h.c\\n' " + \
    "--send-after '# ' --send '/mnt/usr/bin/cc --sysroot=/mnt -static -o /tmp/h /tmp/h.c; echo cc-rc-$?\\n' " + \
    "--send-after 'cc-rc-0' --send '/tmp/h\\n' " + \
    "--send-after 'cc-ok' --send 'cp /tmp/h.c /mnt/tmp/h.c && echo cc-cp-$((40+2))\\n' " + \
    "--send-after 'cc-cp-42' --send '/usr/sbin/chroot /mnt /usr/bin/cc -o /tmp/d /tmp/h.c; echo dyn-rc-$?\\n' " + \
    "--send-after 'dyn-rc-0' --send 'x=$(/usr/sbin/chroot /mnt /tmp/d); echo dyn-$x\\n' " + \
    "--send-after 'dyn-cc-ok' --send '/usr/bin/ldd /mnt/tmp/d\\n' " + \
    "--expect 'cc-mnt-42' --expect 'OpenBSD clang version 22.1.6' --expect 'cc-rc-0' --expect 'cc-ok' " + \
    "--expect 'dyn-rc-0' --expect 'dyn-cc-ok' --expect '/usr/lib/libc.so.' --expect '/usr/libexec/ld.so'"

# --- quality -----------------------------------------------------------------

# host unit tests (libkern + libz + bsd through sys/arch/host, plus xtask's own)
test:
    cargo test -p libkern -p libz -p bsd -p xtask
    cargo test -p libsa -p boot -p efi
    cargo test -p efiboot-arm64

# tests that cross-check constants against the C reference tree
test-ref:
    OPENBSD_SRC={{justfile_directory()}}/reference/openbsd-src cargo test -p libkern -p libz -p bsd -- --ignored
    OPENBSD_SRC={{justfile_directory()}}/reference/openbsd-src cargo test -p efiboot-arm64 -- --ignored

# bare targets with `--features qemu`: a superset of the plain build, which `just build` covers
clippy:
    cargo clippy -p bsd --target {{amd64}} --features qemu -- -D warnings
    cargo clippy -p bsd --target {{arm64}} --features qemu -- -D warnings
    cargo clippy -p bsd --target {{amd64}} --features qemu,multiprocessor -- -D warnings
    cargo clippy -p bsd --target {{arm64}} --features qemu,multiprocessor -- -D warnings
    cargo clippy -p init --target {{amd64}} -- -D warnings
    cargo clippy -p init --target {{arm64}} -- -D warnings
    cargo clippy -p libkern -p libz -p bsd -p xtask -- -D warnings
    cargo clippy -p libsa -p boot -p efi -- -D warnings
    cargo clippy -p libsa -p boot -p efi --target {{arm64}} -- -D warnings
    cargo clippy -p efiboot-amd64 --target {{amd64}} -- -D warnings
    cargo clippy -p efiboot-arm64 --target {{arm64}} -- -D warnings

fmt:
    cargo fmt --all -- --check

check-ports:
    cargo xtask ports check

# Regenerate the system call tables from reference/.../syscalls.master (sys/sys/syscall.rs,
# syscallargs.rs, kern/init_sysent.rs, kern/syscalls.rs). Rerun after porting a sys_* function.
gen-syscalls:
    cargo xtask gen-syscalls

check-syscalls:
    cargo xtask gen-syscalls --check

drift:
    cargo xtask ports drift

# --- the install media (M14c) ---------------------------------------------------------

# `rd0`'s image size in sectors: the ramdisk of `bsd.rd` is built into the kernel
# (`EMIBSD_MINIROOTSIZE`, feature `miniroot`: sys/dev/rd.rs, OpenBSD's `option MINIROOTSIZE`)
# and `cargo xtask miniroot` pads its image to exactly this (userland/miniroot.rs,
# `MINIROOT_SECTORS`: change both).
miniroot_sectors := "65536"

# The kernels the install media uses, each in a target directory of its own so the other
# builds are not redone: `bsd.rd`'s (the MULTIPROCESSOR kernel with the ramdisk compiled in,
# `rdsetroot` puts the miniroot in later) and `bsd`'s, the kernel the sets install (the same
# `--features qemu,multiprocessor` kernel the smokes boot). docs/ARCHITECTURE.md, "The
# install media". `smoke-build` builds them, so `smoke` and `ci` keep them compiling.
build-bsdrd: build-bsdrd-amd64 build-bsdrd-arm64

build-bsdrd-amd64:
    EMIBSD_MINIROOTSIZE={{miniroot_sectors}} cargo build -p bsd --target {{amd64}} --features qemu,multiprocessor,miniroot --target-dir target/bsdrd

build-bsdrd-arm64:
    EMIBSD_MINIROOTSIZE={{miniroot_sectors}} cargo build -p bsd --target {{arm64}} --features qemu,multiprocessor,miniroot --target-dir target/bsdrd

# The install media of one arch: the miniroot, `bsd.rd` and the signed sets, in
# target/install/<arch> (`cargo xtask install-media`). Needs `just userland` and `just comp`.
install-media-amd64: build-bsdrd-amd64 (build-amd64 "--features qemu,multiprocessor") efiboot-amd64
    @test -f target/comp/amd64/comp.ffs || { echo "install-media: no comp build; run just comp first"; exit 1; }
    cargo xtask install-media --arch amd64 --rd-kernel target/bsdrd/{{amd64}}/debug/bsd --bsd target/{{amd64}}/debug/bsd

install-media-arm64: build-bsdrd-arm64 (build-arm64 "--features qemu,multiprocessor") efiboot-arm64
    @test -f target/comp/arm64/comp.ffs || { echo "install-media: no comp build; run just comp first"; exit 1; }
    cargo xtask install-media --arch arm64 --rd-kernel target/bsdrd/{{arm64}}/debug/bsd --bsd target/{{arm64}}/debug/bsd

# M14c: OpenBSD's installer installs EmiBSD. Per arch (`cargo xtask install`,
# tools/xtask/src/install.rs): boots `bsd.rd` by our efiboot (BOOTX64.EFI, BOOTAA64.EFI),
# from a disk laid out as OpenBSD's miniroot image, with a fresh 3 GiB disk (`sd0`), whose
# ramdisk holds `/auto_install.conf`; `install.sub`, unmodified, starts autoinstall(8) by
# itself, partitions the disk (amd64: GPT with an EFI system partition; arm64: MBR with a FAT
# boot partition; then `disklabel -T`), newfs, fetches `bsd`, `bsd.mp`, `base80.tgz` and `comp80.tgz`
# over HTTP from this machine, checks `SHA256.sig` with signify(1) against the test key in its
# `/etc/signify`, extracts them, makes the device nodes, runs installboot(8) and says
# `CONGRATULATIONS!`; then a second boot of the plain `bsd.rd` mounts the new disk and lists
# `/bsd`, `/usr/bin/cc`, `/etc/rc`, `/usr/libexec/ld.so`, the EFI system partition and runs
# `fsck_ffs -n`. Not in `smokes`: about 3.5 minutes on amd64 once the media are made (the
# installer alone 2.5 to 3, most of it extracting the sets under TCG), and making the media
# takes minutes more (docs/ARCHITECTURE.md). Needs `just userland` and `just comp`.
smoke-install: smoke-install-amd64 smoke-install-arm64 smoke-install-arm64-acpi

smoke-install-amd64: install-media-amd64
    EMIBSD_RUN_DIR=${EMIBSD_RUN_DIR:-target/smoke/smoke-install} EMIBSD_TIMEOUT_SCALE=${EMIBSD_TIMEOUT_SCALE:-5} cargo xtask install {{smp}} --arch amd64 --rd-kernel target/bsdrd/{{amd64}}/debug/bsd

smoke-install-arm64: install-media-arm64
    EMIBSD_RUN_DIR=${EMIBSD_RUN_DIR:-target/smoke/smoke-install} EMIBSD_TIMEOUT_SCALE=${EMIBSD_TIMEOUT_SCALE:-5} cargo xtask install {{smp}} --arch arm64 --rd-kernel target/bsdrd/{{arm64}}/debug/bsd

# The last step of M14's criterion: the disk `smoke-install-<arch>` installed, booted through
# the loader installboot(8) put on it (efiboot, from /usr/mdec) to `login:`
# on a fresh VM with OpenBSD's /etc/rc, then `cc hello.c && ./a.out` there prints
# `hello from cc 42`. About a minute on amd64; not in `smokes`, because it needs the disk
# `smoke-install-<arch>` made.
smoke-install-boot-amd64:
    EMIBSD_RUN_DIR=${EMIBSD_RUN_DIR:-target/smoke/smoke-install} EMIBSD_TIMEOUT_SCALE=${EMIBSD_TIMEOUT_SCALE:-5} cargo xtask install-boot {{smp}} --arch amd64

smoke-install-boot-arm64:
    EMIBSD_RUN_DIR=${EMIBSD_RUN_DIR:-target/smoke/smoke-install} EMIBSD_TIMEOUT_SCALE=${EMIBSD_TIMEOUT_SCALE:-5} cargo xtask install-boot {{smp}} --arch arm64

# The same two arm64 runs on `virt,acpi=on` (`--acpi`, as `smoke-acpi`): the installer's VM,
# the check boot and the installed system's VM get ACPI tables and no device tree, the disks
# and vio0 on the PCI bus. The media's disk is then `sd0` and the fresh disk `sd1` (install.rs,
# `target_sd`). Own run directory (`target/smoke/smoke-install-acpi`) and logs
# (`target/install/arm64/acpi/`), so the acpi=off disk and logs stay; the two arm64 install runs
# share the miniroot's staging and run one after the other. Not in `smokes`: the install about
# 2.5 minutes once the media are made (the installer 2.5, 3.5 with the media), the boot half a
# minute (2026-10-08).
smoke-install-arm64-acpi: install-media-arm64
    EMIBSD_RUN_DIR=${EMIBSD_RUN_DIR:-target/smoke/smoke-install-acpi} EMIBSD_TIMEOUT_SCALE=${EMIBSD_TIMEOUT_SCALE:-5} cargo xtask install {{smp}} --arch arm64 --acpi --rd-kernel target/bsdrd/{{arm64}}/debug/bsd

smoke-install-boot-arm64-acpi:
    EMIBSD_RUN_DIR=${EMIBSD_RUN_DIR:-target/smoke/smoke-install-acpi} EMIBSD_TIMEOUT_SCALE=${EMIBSD_TIMEOUT_SCALE:-5} cargo xtask install-boot {{smp}} --arch arm64 --acpi

ci: fmt clippy test build smoke check-ports check-syscalls

# `ci` with every `{{smp}}` recipe on four processors (`EMIBSD_NCPU=4`), not only the `smp4`
# group: four CPUs make the MP races likelier. Then the installer end to end on both archs, also
# on four processors (the user's decision of 2026-10-07): the install media made afresh from the
# tree, `smoke-install-<arch>` and `smoke-install-boot-<arch>` for amd64, arm64 and arm64 on
# ACPI, so every milestone close regenerates and checks the installer. Needs `just userland` and
# `just comp`. Mandatory before a milestone is marked met; its result goes in the milestone's
# closing commit (.claude/rules/testing.md). `just jobs=N ci-full` passes N on.
ci-full:
    EMIBSD_NCPU=4 {{quote(just_executable())}} jobs={{jobs}} ci
    EMIBSD_NCPU=4 {{quote(just_executable())}} smoke-install-amd64 smoke-install-boot-amd64 \
        smoke-install-arm64 smoke-install-boot-arm64 smoke-install-arm64-acpi smoke-install-boot-arm64-acpi
