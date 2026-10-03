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

build: build-amd64 build-arm64 build-init-amd64 build-init-arm64

# --- boot images and QEMU ---------------------------------------------------

image-amd64: (build-amd64 "--features qemu") build-init-amd64
    cargo xtask image --arch amd64 --kernel target/{{amd64}}/debug/bsd

image-arm64: (build-arm64 "--features qemu") build-init-arm64
    cargo xtask image --arch arm64 --kernel target/{{arm64}}/debug/bsd

run-amd64: image-amd64
    cargo xtask qemu --arch amd64

run-arm64: image-arm64
    cargo xtask qemu --arch arm64

# Boots per arch, every one with a virtio network card on QEMU's user network: a plain one that
# must reach the end of main() (status 33), printing the EmiBSD 8.0 version banner and the
# virtio attach lines, with init checking its identity through sysctl(2) and the vfs system
# calls failing as they must with no root file system yet (`main` says it cannot mount root and
# `check_console` that /dev/console does not exist) and making the console tty its controlling
# terminal (`init: tty ok`); `boot -d`, which
# enters ddb-lite through a breakpoint trap, prints where it stopped and continues (status 33);
# `selftest=trap`, a deliberate bad access that must print OpenBSD's fatal trap message and
# panic with a stack trace (status 35); and `selftest=uart`, which opens the console's tty through
# the device switch, gets a line typed on the serial console through the line discipline and
# echoes it (status 33);
# `selftest=clock`, which waits for hz clock interrupts and a timeout (status 33);
# `selftest=kthread`, two kernel threads passing a turn with msleep/wakeup (status 33); and
# `selftest=taskq`, tasks run by systq, systqmp and a created then destroyed queue (status 33);
# and `selftest=vio`, which brings vio0 up, sends an ARP request for QEMU's gateway and waits
# for a frame through the receive interrupt (status 33).
# All of those boot without a ramdisk (`--ramdisk none`, so the kernel says
# `rd: no ramdisk module`, `--expect-ramdisk`) and run the Rust stand-in init, the kernel's
# self-test. Then `smoke-shell` (M8's exit criterion) boots the ffs ramdisk `just userland`
# makes, booted `-s` (RB_SINGLE; a plain boot goes multi-user, see `smoke-login`): rd(4) reads
# its superblock, the root is mounted from rd0a, OpenBSD's init(8) runs from it and goes single
# user, and ksh(1) answers `uname -a`, `uname -sr`, `cat /etc/motd` and `ls /` on the serial
# console.
smoke: (build-amd64 "--features qemu") (build-arm64 "--features qemu") build-init-amd64 build-init-arm64 smoke-shell smoke-login
    cargo xtask smoke --arch amd64 --kernel target/{{amd64}}/debug/bsd --ramdisk none --expect-ramdisk \
        --expect "bsd: booted on amd64" --expect "The Regents of the University of California" \
        --expect "EmiBSD 8.0 (GENERIC) #" \
        --expect "real mem = " --expect "avail mem = " --expect "selftest: pmap kernel mapping ok" \
        --expect "selftest: malloc/pool stress ok" --expect "selftest: mbufs ok" \
        --expect "selftest: buffer cache ok" --expect "selftest: pager map ok" \
        --expect "selftest: bus_dma ok" --expect "mainbus0 at root" \
        --expect "cpu0 at mainbus0: (uniprocessor)" --expect "pci0 at mainbus0 bus 0" \
        --expect "at pci0 dev 0 function 0 not configured" \
        --expect "virtio0 at pci0 dev 2 function 0 vendor 0x1af4 product 0x1000 rev 0x00" \
        --expect "vio0 at virtio0: 1 queue, address 52:54:00:12:34:56" --expect "virtio0: irq " \
        --expect "isa0 at mainbus0" \
        --expect "com0 at isa0 port 0x3f8/8 irq 4: ns16550a, 16 byte fifo" --expect "com0: console" \
        --expect "cpu0: apic clock running at" \
        --expect "module: /init (" --expect "init: hello from user mode" --expect "init: argv and auxv ok" \
        --expect "init: demand-zero bss ok" --expect "init: ids and tcb ok" \
        --expect "init: fds ok" --expect "init: signals ok" --expect "init: EmiBSD 8.0" \
        --expect "cannot mount root: no root file system" \
        --expect "warning: /dev/console does not exist" --expect "init: vfs ok (no root file system)" \
        --expect "init: pipes ok" --expect "init: sockets ok" --expect "init: kqueue ok" --expect "init: processes ok" --expect "init: time ok" --expect "init: unveil ok" --expect "init: sendsyslog ok" --expect "pinsyscalls addr" \
        --expect "selftest: ping 10.0.2.2: echo reply received" \
        --expect "init: tty ok" \
        --expect "init exited with status 0 (signal 0)"
    cargo xtask smoke --arch amd64 --kernel target/{{amd64}}/debug/bsd --ramdisk none --cmdline "-d" \
        --expect "Stopped at" --expect "selftest: malloc/pool stress ok"
    cargo xtask smoke --arch amd64 --kernel target/{{amd64}}/debug/bsd --ramdisk none --cmdline "selftest=trap" --status 35 \
        --expect "fatal page fault in supervisor mode" --expect "trap type 6 code" \
        --expect "panic: trap type 6, code=" --expect "Starting stack trace..." \
        --expect "End of stack trace." --expect "The operating system has halted."
    cargo xtask smoke --arch amd64 --kernel target/{{amd64}}/debug/bsd --ramdisk none --cmdline "selftest=uart" \
        --send-after "selftest: uart rx interrupt armed" --send 'hello\n' \
        --expect "selftest: uart rx interrupt armed" --expect "selftest: uart echo: hello"
    cargo xtask smoke --arch amd64 --kernel target/{{amd64}}/debug/bsd --ramdisk none --cmdline "selftest=clock" \
        --expect "selftest: clock ok"
    cargo xtask smoke --arch amd64 --kernel target/{{amd64}}/debug/bsd --ramdisk none --cmdline "selftest=kthread" \
        --expect "selftest: kthread ping-pong ok"
    cargo xtask smoke --arch amd64 --kernel target/{{amd64}}/debug/bsd --ramdisk none --cmdline "selftest=taskq" \
        --expect "selftest: taskq ok"
    cargo xtask smoke --arch amd64 --kernel target/{{amd64}}/debug/bsd --ramdisk none --cmdline "selftest=vio" \
        --expect "selftest: vio up ok" --expect "selftest: vio rx ok"
    cargo xtask smoke --arch arm64 --kernel target/{{arm64}}/debug/bsd --ramdisk none --expect-ramdisk \
        --expect "bsd: booted on arm64" --expect "The Regents of the University of California" \
        --expect "EmiBSD 8.0 (GENERIC) #" \
        --expect "real mem  = " --expect "avail mem = " --expect "selftest: pmap kernel mapping ok" \
        --expect "selftest: malloc/pool stress ok" --expect "selftest: mbufs ok" \
        --expect "selftest: buffer cache ok" --expect "selftest: pager map ok" \
        --expect "mainbus0 at root" --expect "ampintc0 at mainbus0 nirq " \
        --expect "agtimer0 at mainbus0: " --expect "selftest: bus_dma ok" \
        --expect "virtio0 at mainbus0: Virtio Unknown (0) Device" \
        --expect "virtio30 at mainbus0: Virtio Network Device" \
        --expect "virtio31 at mainbus0: Virtio Block Device" \
        --expect "vio0 at virtio30: 1 queue, address 52:54:00:12:34:56" \
        --expect ": rev 1, 16 byte fifo" --expect "pluart0: console" \
        --expect "module: /init (" --expect "init: hello from user mode" --expect "init: argv and auxv ok" \
        --expect "init: demand-zero bss ok" --expect "init: ids and tcb ok" \
        --expect "init: fds ok" --expect "init: signals ok" --expect "init: EmiBSD 8.0" \
        --expect "cannot mount root: no root file system" \
        --expect "warning: /dev/console does not exist" --expect "init: vfs ok (no root file system)" \
        --expect "init: pipes ok" --expect "init: sockets ok" --expect "init: kqueue ok" --expect "init: processes ok" --expect "init: time ok" --expect "init: unveil ok" --expect "init: sendsyslog ok" --expect "pinsyscalls addr" \
        --expect "selftest: ping 10.0.2.2: echo reply received" \
        --expect "init: tty ok" \
        --expect "init exited with status 0 (signal 0)"
    cargo xtask smoke --arch arm64 --kernel target/{{arm64}}/debug/bsd --ramdisk none --cmdline "-d" \
        --expect "Stopped at" --expect "selftest: malloc/pool stress ok"
    cargo xtask smoke --arch arm64 --kernel target/{{arm64}}/debug/bsd --ramdisk none --cmdline "selftest=trap" --status 35 \
        --expect "panic: uvm_fault failed:" --expect "Starting stack trace..." \
        --expect "End of stack trace." --expect "The operating system has halted."
    cargo xtask smoke --arch arm64 --kernel target/{{arm64}}/debug/bsd --ramdisk none --cmdline "selftest=uart" \
        --send-after "selftest: uart rx interrupt armed" --send 'hello\n' \
        --expect "selftest: uart rx interrupt armed" --expect "selftest: uart echo: hello"
    cargo xtask smoke --arch arm64 --kernel target/{{arm64}}/debug/bsd --ramdisk none --cmdline "selftest=clock" \
        --expect "selftest: clock ok"
    cargo xtask smoke --arch arm64 --kernel target/{{arm64}}/debug/bsd --ramdisk none --cmdline "selftest=kthread" \
        --expect "selftest: kthread ping-pong ok"
    cargo xtask smoke --arch arm64 --kernel target/{{arm64}}/debug/bsd --ramdisk none --cmdline "selftest=taskq" \
        --expect "selftest: taskq ok"
    cargo xtask smoke --arch arm64 --kernel target/{{arm64}}/debug/bsd --ramdisk none --cmdline "selftest=vio" \
        --expect "selftest: vio up ok" --expect "selftest: vio rx ok"

# M8: OpenBSD's init(8) and ksh(1) from the ffs ramdisk, driven over the serial console. Needs
# `just userland` (the ramdisk image); stops once every expected line was seen.
smoke-shell: (build-amd64 "--features qemu") (build-arm64 "--features qemu")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-shell: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke --arch amd64 --kernel target/{{amd64}}/debug/bsd --cmdline "-s" --expect-ramdisk --until-seen \
        --send-after "RETURN for sh:" --send '\n' \
        --send-after "# " --send 'uname -a\n' --send-after "GENERIC#" --send 'uname -sr\n' \
        --send-after "EmiBSD 8.0" --send 'cat /etc/motd\n' \
        --send-after "Welcome to EmiBSD" --send 'ls /\n' \
        --send-after "bin  dev  etc" --send 'ls /sbin\n' \
        --expect "root on rd0a swap on rd0b dump on rd0b" \
        --expect "Enter pathname of shell or RETURN for sh:" \
        --expect " 8.0 GENERIC#" --expect "amd64" \
        --expect "Welcome to EmiBSD 8.0: OpenBSD's init(8) and ksh(1)" \
        --expect "bin  dev  etc  home root sbin tmp  usr  var" --expect "pfctl"
    cargo xtask smoke --arch arm64 --kernel target/{{arm64}}/debug/bsd --cmdline "-s" --expect-ramdisk --until-seen \
        --send-after "RETURN for sh:" --send '\n' \
        --send-after "# " --send 'uname -a\n' --send-after "GENERIC#" --send 'uname -sr\n' \
        --send-after "EmiBSD 8.0" --send 'cat /etc/motd\n' \
        --send-after "Welcome to EmiBSD" --send 'ls /\n' \
        --send-after "bin  dev  etc" --send 'ls /sbin\n' \
        --expect "root on rd0a swap on rd0b dump on rd0b" \
        --expect "Enter pathname of shell or RETURN for sh:" \
        --expect " 8.0 GENERIC#" --expect "arm64" \
        --expect "Welcome to EmiBSD 8.0: OpenBSD's init(8) and ksh(1)" \
        --expect "bin  dev  etc  home root sbin tmp  usr  var" --expect "pfctl"

# M8b: a plain boot of the ramdisk goes multi-user: init(8) runs /etc/rc (`rc: multi-user`),
# then getty(8) on tty00 prints `login:`; the session logs in as root (the test image's
# password, docs/SETUP.md) and runs `id` and `uname -a`. Not part of `smoke` yet: login(1)
# needs BSD Auth's socketpair(2) (AF_UNIX) to talk to login_passwd, which the kernel is
# getting in parallel work.
smoke-login: (build-amd64 "--features qemu") (build-arm64 "--features qemu")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-login: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen \
        --send-after "login:" --send 'root\n' --send-after "Password:" --send 'emibsd\n' \
        --send-after "# " --send 'id\n' --send-after "uid=0(root)" --send 'uname -a\n' \
        --expect "rc: multi-user" --expect "EmiBSD/amd64 (Amnesiac) (tty00)" \
        --expect "uid=0(root)" --expect " 8.0 GENERIC#" --expect "amd64"
    cargo xtask smoke --arch arm64 --kernel target/{{arm64}}/debug/bsd --expect-ramdisk --until-seen \
        --send-after "login:" --send 'root\n' --send-after "Password:" --send 'emibsd\n' \
        --send-after "# " --send 'id\n' --send-after "uid=0(root)" --send 'uname -a\n' \
        --expect "rc: multi-user" --expect "EmiBSD/arm64 (Amnesiac) (tty00)" \
        --expect "uid=0(root)" --expect " 8.0 GENERIC#" --expect "arm64"

# M9a: the routing socket and the `net.route` sysctl from userland. Logs in as `smoke-login`
# does, then runs OpenBSD's route(8) (`show`: a routing socket, then NET_RT_DUMP through
# sysctl(2); `get`: RTM_GET written to the routing socket and its answer read back) and
# ifconfig(8) (getifaddrs(3): NET_RT_IFLIST, then interface ioctls on an AF_INET socket). The kernel's network self-test configured vio0 (10.0.2.15) and the default route
# through 10.0.2.2 before init ran. Not part of `smoke` yet: ifconfig needs the inet socket
# protocols (UDP), ported in parallel work.
smoke-route: (build-amd64 "--features qemu") (build-arm64 "--features qemu")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-route: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen \
        --send-after "login:" --send 'root\n' --send-after "Password:" --send 'emibsd\n' \
        --send-after "# " --send 'route -n show -inet\n' \
        --send-after "# " --send 'route -n get 8.8.8.8\n' \
        --send-after "# " --send 'ifconfig -a\n' \
        --expect "rc: multi-user" --expect "Internet:" --expect "default            10.0.2.2" \
        --expect "10.0.2/24" --expect "gateway: 10.0.2.2" --expect "interface: vio0" \
        --expect "lo0: flags=" --expect "vio0: flags="
    cargo xtask smoke --arch arm64 --kernel target/{{arm64}}/debug/bsd --expect-ramdisk --until-seen \
        --send-after "login:" --send 'root\n' --send-after "Password:" --send 'emibsd\n' \
        --send-after "# " --send 'route -n show -inet\n' \
        --send-after "# " --send 'route -n get 8.8.8.8\n' \
        --send-after "# " --send 'ifconfig -a\n' \
        --expect "rc: multi-user" --expect "Internet:" --expect "default            10.0.2.2" \
        --expect "10.0.2/24" --expect "gateway: 10.0.2.2" --expect "interface: vio0" \
        --expect "lo0: flags=" --expect "vio0: flags="

# M9b/M9c harness: two VMs of one arch at once (`cargo xtask smoke2`), each with vio0 on QEMU's
# user network and vio1 on a private link between the two (docs/SETUP.md, "Two VMs"). Both log
# in as root and run `ifconfig vio1`. Passes when each kernel attached vio1 with the MAC of the
# link NIC (A: 52:54:00:bb:00:01, B: 52:54:00:bb:00:02). TODO: once ifconfig(8) can open an
# AF_INET socket (today: `ifconfig: socket: Protocol not supported`), also expect
# `vio1: flags=` from it. Not part of `smoke` yet. A tunnel needs more than this: by hand,
# `ifconfig vio1 inet 192.168.77.1/24` on A and `.2` on B, then `ping` (docs/SETUP.md).
smoke-link: (build-amd64 "--features qemu") (build-arm64 "--features qemu")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-link: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke2 --arch amd64 --kernel target/{{amd64}}/debug/bsd \
        --both-send-after "login:" --both-send 'root\n' --both-send-after "Password:" --both-send 'emibsd\n' \
        --both-send-after "# " --both-send 'ifconfig vio1\n' \
        --a-expect "vio1 at virtio1: 1 queue, address 52:54:00:bb:00:01" \
        --b-expect "vio1 at virtio1: 1 queue, address 52:54:00:bb:00:02"
    cargo xtask smoke2 --arch arm64 --kernel target/{{arm64}}/debug/bsd \
        --both-send-after "login:" --both-send 'root\n' --both-send-after "Password:" --both-send 'emibsd\n' \
        --both-send-after "# " --both-send 'ifconfig vio1\n' \
        --a-expect "vio1 at virtio30: 1 queue, address 52:54:00:bb:00:01" \
        --b-expect "vio1 at virtio30: 1 queue, address 52:54:00:bb:00:02"

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

# --- quality -----------------------------------------------------------------

# host unit tests (libkern + libz + bsd through sys/arch/host, plus xtask's own)
test:
    cargo test -p libkern -p libz -p bsd -p xtask

# tests that cross-check constants against the C reference tree
test-ref:
    OPENBSD_SRC=reference/openbsd-src cargo test -p libkern -p libz -p bsd -- --ignored

# bare targets with `--features qemu`: a superset of the plain build, which `just build` covers
clippy:
    cargo clippy -p bsd --target {{amd64}} --features qemu -- -D warnings
    cargo clippy -p bsd --target {{arm64}} --features qemu -- -D warnings
    cargo clippy -p init --target {{amd64}} -- -D warnings
    cargo clippy -p init --target {{arm64}} -- -D warnings
    cargo clippy -p libkern -p libz -p bsd -p xtask -- -D warnings

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

ci: fmt clippy test build smoke check-ports check-syscalls
