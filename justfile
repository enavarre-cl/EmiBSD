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
# The default boot's init stand-in also checks the Internet sockets (`init: inet sockets ok`:
# vio0's address through SIOCGIFADDR, a ping from a raw ICMP socket, a local UDP datagram) and
# PF_KEY (`init: pfkey ok`: SADB_REGISTER on a PF_KEY socket and its answer) and TCP
# (`init: tcp ok`: lo0 configured, connect/accept on 127.0.0.1, a line each way, FIN, close).
# All of those boot without a ramdisk (`--ramdisk none`, so the kernel says
# `rd: no ramdisk module`, `--expect-ramdisk`) and run the Rust stand-in init, the kernel's
# self-test. Then `smoke-shell` (M8's exit criterion) boots the ffs ramdisk `just userland`
# makes, booted `-s` (RB_SINGLE; a plain boot goes multi-user, see `smoke-login`): rd(4) reads
# its superblock, the root is mounted from rd0a, OpenBSD's init(8) runs from it and goes single
# user, and ksh(1) answers `uname -a`, `uname -sr`, `cat /etc/motd` and `ls /` on the serial
# console.
smoke: (build-amd64 "--features qemu") (build-arm64 "--features qemu") build-init-amd64 build-init-arm64 smoke-shell smoke-login smoke-net smoke-route smoke-diag smoke-link smoke-wg smoke-pf smoke-ipsec smoke-esp smoke-pfsync smoke-ipcomp smoke-https smoke-tcp smoke-divert smoke-tcpdump smoke-inet6 smoke-disk smoke-ufsopts smoke-fs
    cargo xtask smoke {{reject}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --ramdisk none --expect-ramdisk \
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
    cargo xtask smoke {{reject}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --ramdisk none --cmdline "-d" \
        --expect "Stopped at" --expect "selftest: malloc/pool stress ok"
    cargo xtask smoke {{reject}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --ramdisk none --cmdline "selftest=trap" --status 35 \
        --expect "fatal page fault in supervisor mode" --expect "trap type 6 code" \
        --expect "panic: trap type 6, code=" --expect "Starting stack trace..." \
        --expect "End of stack trace." --expect "The operating system has halted."
    cargo xtask smoke {{reject}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --ramdisk none --cmdline "selftest=uart" \
        --send-after "selftest: uart rx interrupt armed" --send 'hello\n' \
        --expect "selftest: uart rx interrupt armed" --expect "selftest: uart echo: hello"
    cargo xtask smoke {{reject}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --ramdisk none --cmdline "selftest=clock" \
        --expect "selftest: clock ok"
    cargo xtask smoke {{reject}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --ramdisk none --cmdline "selftest=kthread" \
        --expect "selftest: kthread ping-pong ok"
    cargo xtask smoke {{reject}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --ramdisk none --cmdline "selftest=taskq" \
        --expect "selftest: taskq ok"
    cargo xtask smoke {{reject}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --ramdisk none --cmdline "selftest=vio" \
        --expect "selftest: vio up ok" --expect "selftest: vio rx ok"
    cargo xtask smoke {{reject}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --ramdisk none --expect-ramdisk \
        --expect "bsd: booted on arm64" --expect "The Regents of the University of California" \
        --expect "EmiBSD 8.0 (GENERIC) #" \
        --expect "real mem  = " --expect "avail mem = " --expect "selftest: pmap kernel mapping ok" \
        --expect "selftest: malloc/pool stress ok" --expect "selftest: mbufs ok" \
        --expect "selftest: buffer cache ok" --expect "selftest: pager map ok" \
        --expect "mainbus0 at root" --expect "ampintc0 at mainbus0 nirq " \
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
    cargo xtask smoke {{reject}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --ramdisk none --cmdline "-d" \
        --expect "Stopped at" --expect "selftest: malloc/pool stress ok"
    cargo xtask smoke {{reject}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --ramdisk none --cmdline "selftest=trap" --status 35 \
        --expect "panic: uvm_fault failed:" --expect "Starting stack trace..." \
        --expect "End of stack trace." --expect "The operating system has halted."
    cargo xtask smoke {{reject}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --ramdisk none --cmdline "selftest=uart" \
        --send-after "selftest: uart rx interrupt armed" --send 'hello\n' \
        --expect "selftest: uart rx interrupt armed" --expect "selftest: uart echo: hello"
    cargo xtask smoke {{reject}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --ramdisk none --cmdline "selftest=clock" \
        --expect "selftest: clock ok"
    cargo xtask smoke {{reject}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --ramdisk none --cmdline "selftest=kthread" \
        --expect "selftest: kthread ping-pong ok"
    cargo xtask smoke {{reject}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --ramdisk none --cmdline "selftest=taskq" \
        --expect "selftest: taskq ok"
    cargo xtask smoke {{reject}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --ramdisk none --cmdline "selftest=vio" \
        --expect "selftest: vio up ok" --expect "selftest: vio rx ok"

# M8: OpenBSD's init(8) and ksh(1) from the ffs ramdisk, driven over the serial console. Needs
# `just userland` (the ramdisk image); stops once every expected line was seen.
smoke-shell: (build-amd64 "--features qemu") (build-arm64 "--features qemu")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-shell: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke {{reject}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --cmdline "-s" --expect-ramdisk --until-seen \
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
    cargo xtask smoke {{reject}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --cmdline "-s" --expect-ramdisk --until-seen \
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

# M8b: a plain boot of the ramdisk goes multi-user: init(8) runs /etc/rc (`rc: multi-user`),
# then getty(8) on tty00 prints `login:`; the session logs in as root (the test image's
# password, docs/SETUP.md) and runs `id` and `uname -a`, then checks that the clock came from
# the time-of-day chip (mc146818 on amd64, the UEFI runtime services, efi0, on arm64): later
# than 2026-10-03 (1790985600), a day past the ramdisk's fixed file system time, which is
# what the kernel would run on without one. `rtc-$x` keeps the echoed command line from
# matching. Part of `smoke`.
smoke-login: (build-amd64 "--features qemu") (build-arm64 "--features qemu")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-login: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke {{reject}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen \
        --send-after "login:" --send 'root\n' --send-after "Password:" --send 'emibsd\n' \
        --send-after "# " --send 'id\n' --send-after "uid=0(root)" --send 'uname -a\n' \
        --send-after " 8.0 GENERIC#" --send 'x=ok; [ $(date +%s) -gt 1790985600 ] && echo rtc-$x\n' \
        --expect "rc: multi-user" --expect "EmiBSD/amd64 (Amnesiac) (tty00)" \
        --expect "uid=0(root)" --expect " 8.0 GENERIC#" --expect "amd64" --expect "rtc-ok"
    cargo xtask smoke {{reject}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --expect-ramdisk --until-seen \
        --send-after "login:" --send 'root\n' --send-after "Password:" --send 'emibsd\n' \
        --send-after "# " --send 'id\n' --send-after "uid=0(root)" --send 'uname -a\n' \
        --send-after " 8.0 GENERIC#" --send 'x=ok; [ $(date +%s) -gt 1790985600 ] && echo rtc-$x\n' \
        --expect "rc: multi-user" --expect "EmiBSD/arm64 (Amnesiac) (tty00)" \
        --expect "uid=0(root)" --expect " 8.0 GENERIC#" --expect "arm64" --expect "rtc-ok"

# M9a: the routing socket and the `net.route` sysctl from userland. Logs in as `smoke-login`
# does, then runs OpenBSD's route(8) (`show`: a routing socket, then NET_RT_DUMP through
# sysctl(2); `get`: RTM_GET written to the routing socket and its answer read back) and
# ifconfig(8) (getifaddrs(3): NET_RT_IFLIST, then interface ioctls on an AF_INET socket). The kernel's network self-test configured vio0 (10.0.2.15) and the default route
# through 10.0.2.2 before init ran. Part of `smoke`.
smoke-route: (build-amd64 "--features qemu") (build-arm64 "--features qemu")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-route: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke {{reject}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen \
        --send-after "login:" --send 'root\n' --send-after "Password:" --send 'emibsd\n' \
        --send-after "# " --send 'route -n show -inet\n' \
        --send-after "# " --send 'route -n get 8.8.8.8\n' \
        --send-after "# " --send 'ifconfig -a\n' \
        --expect "rc: multi-user" --expect "Internet:" --expect "default            10.0.2.2" \
        --expect "10.0.2/24" --expect "gateway: 10.0.2.2" --expect "interface: vio0" \
        --expect "lo0: flags=" --expect "vio0: flags=" {{https_run}}
    cargo xtask smoke {{reject}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --expect-ramdisk --until-seen \
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
smoke-https: (build-amd64 "--features qemu") (build-arm64 "--features qemu")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-https: no ramdisk image; run just userland first"; exit 1; }
    @mkdir -p target/https-www && echo 'hello from emibsd-host over https' >target/https-www/hello.txt
    cargo xtask smoke {{reject}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen {{https_check}}
    cargo xtask smoke {{reject}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --expect-ramdisk --until-seen {{https_check}}

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
smoke-internet: (build-amd64 "--features qemu") (build-arm64 "--features qemu")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-internet: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke {{reject}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen {{internet_check}}
    cargo xtask smoke {{reject}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --expect-ramdisk --until-seen {{internet_check}}

internet_check := "--send-after login: --send 'root\\n' --send-after Password: --send 'emibsd\\n' " + \
    "--send-after '# ' --send 'ftp -o - https://www.openbsd.org/robots.txt\\n' " + \
    "--expect 'rc: multi-user' --expect 'User-agent:'"

# Diagnostic tools stage 2: OpenBSD's ps(1), fstat(1) and vmstat(8) over libkvm's sysctl(2)
# paths (kern.proc, kern.proc_args, kern.file, vm.uvmexp, hw.diskstats, kern.intrcnt,
# kern.pool, kern.malloc), df(1) and mount(8) over getfsstat(2), and sysctl(8)'s
# kern.timecounter (amd64 runs on the TSC, arm64 on agtimer). Logs in as `smoke-login`
# does; `echo diag-$((40+2))` marks the end. Part of `smoke`.
smoke-diag: (build-amd64 "--features qemu") (build-arm64 "--features qemu")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-diag: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke {{reject}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen \
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
        --expect "rw    tty00" --expect "sr sd0 rd0  int" \
        --expect "interrupt                       total     rate" --expect "/com0" \
        --expect "bytes per page" --expect "Memory statistics by bucket size" \
        --expect "Memory resource pool statistics" --expect "/dev/rd0a        " \
        --expect "/dev/rd0a on / type ffs (local)" --expect "diag-42" \
        --expect "kern.timecounter.hardware=tsc" --expect "kern.timecounter.choice=i8254(0) tsc(2000)"
    cargo xtask smoke {{reject}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --expect-ramdisk --until-seen \
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
        --expect "Memory resource pool statistics" --expect "/dev/rd0a        " \
        --expect "/dev/rd0a on / type ffs (local)" --expect "diag-42" \
        --expect "kern.timecounter.hardware=agtimer" --expect "kern.timecounter.choice=agtimer(0)"

# M9b/M9c harness: two VMs of one arch at once (`cargo xtask smoke2`), each with vio0 on QEMU's
# user network and vio1 on a private link between the two (docs/SETUP.md, "Two VMs"). Both log
# in as root, give vio1 an address on 192.168.77.0/24 and ping each other across the link.
smoke-link: (build-amd64 "--features qemu") (build-arm64 "--features qemu")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-link: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke2 {{reject}} --arch amd64 --kernel target/{{amd64}}/debug/bsd \
        --both-send-after "login:" --both-send 'root\n' --both-send-after "Password:" --both-send 'emibsd\n' \
        --a-send-after "# " --a-send 'ifconfig vio1 inet 192.168.77.1/24 up\n' \
        --a-send-after "# " --a-send 'ping -c 10 192.168.77.2\n' \
        --b-send-after "# " --b-send 'ifconfig vio1 inet 192.168.77.2/24 up\n' \
        --b-send-after "# " --b-send 'ping -c 10 192.168.77.1\n' \
        --a-expect "vio1 at virtio1: 1 queue, address 52:54:00:bb:00:01" \
        --b-expect "vio1 at virtio1: 1 queue, address 52:54:00:bb:00:02" \
        --a-expect "bytes from 192.168.77.2: icmp_seq=" --b-expect "bytes from 192.168.77.1: icmp_seq="
    cargo xtask smoke2 {{reject}} --arch arm64 --kernel target/{{arm64}}/debug/bsd \
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
smoke-wg: (build-amd64 "--features qemu") (build-arm64 "--features qemu")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-wg: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke2 {{reject}} --arch amd64 --kernel target/{{amd64}}/debug/bsd \
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
    cargo xtask smoke2 {{reject}} --arch arm64 --kernel target/{{arm64}}/debug/bsd \
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
smoke-net: (build-amd64 "--features qemu") (build-arm64 "--features qemu")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-net: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke {{reject}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen \
        --send-after "login:" --send 'root\n' --send-after "Password:" --send 'emibsd\n' \
        --send-after "# " --send 'ifconfig vio0\n' --send-after "# " --send 'ping -c 1 10.0.2.2\n' \
        --expect "rc: multi-user" --expect "vio0: flags=" --expect "inet 10.0.2.15 netmask 0xffffff00" \
        --expect "PING 10.0.2.2 (10.0.2.2): 56 data bytes" \
        --expect "1 packets transmitted, 1 packets received, 0.0% packet loss"
    cargo xtask smoke {{reject}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --expect-ramdisk --until-seen \
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
smoke-pf: (build-amd64 "--features qemu") (build-arm64 "--features qemu")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-pf: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke {{reject}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen \
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
    cargo xtask smoke {{reject}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --expect-ramdisk --until-seen \
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
smoke-ipsec: (build-amd64 "--features qemu") (build-arm64 "--features qemu")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-ipsec: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke {{reject}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen \
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
    cargo xtask smoke {{reject}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --expect-ramdisk --until-seen \
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
smoke-esp: (build-amd64 "--features qemu") (build-arm64 "--features qemu")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-esp: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke2 {{reject}} --arch amd64 --kernel target/{{amd64}}/debug/bsd \
        {{esp_both}} \
        --a-send-after "# " --a-send 'ifconfig vio1 inet 192.168.77.1/24\n' \
        --a-send-after "# " --a-send 'ifconfig lo1 create; ifconfig lo1 inet 10.77.1.1/32\n' \
        {{esp_a}} \
        --b-send-after "# " --b-send 'ifconfig vio1 inet 192.168.77.2/24\n' \
        --b-send-after "# " --b-send 'ifconfig lo1 create; ifconfig lo1 inet 10.77.2.1/32\n' \
        {{esp_b}} \
        --a-expect "bytes from 192.168.77.2" --a-expect "bytes from 10.77.2.1" \
        --b-expect "bytes from 192.168.77.1" --b-expect "bytes from 10.77.1.1"
    cargo xtask smoke2 {{reject}} --arch arm64 --kernel target/{{arm64}}/debug/bsd \
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
smoke-pfsync: (build-amd64 "--features qemu") (build-arm64 "--features qemu")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-pfsync: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke2 {{reject}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --timeout 400 \
        {{pfsync_both}} {{pfsync_a}} {{pfsync_b}} {{pfsync_expect}}
    cargo xtask smoke2 {{reject}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --timeout 400 \
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
smoke-ipcomp: (build-amd64 "--features qemu") (build-arm64 "--features qemu")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-ipcomp: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke2 {{reject}} --arch amd64 --kernel target/{{amd64}}/debug/bsd \
        {{esp_both}} {{ipcomp_both}} {{ipcomp_a}} {{ipcomp_b}} \
        --a-expect "bytes from 192.168.77.2" --a-expect "1008 bytes from 10.77.2.1" \
        --a-expect "ipcomp tunnel from 192.168.77.2 to 192.168.77.1 spi 0x00002002 comp deflate" \
        --b-expect "bytes from 192.168.77.1" --b-expect "1008 bytes from 10.77.1.1" \
        --b-expect "ipcomp tunnel from 192.168.77.1 to 192.168.77.2 spi 0x00002001 comp deflate"
    cargo xtask smoke2 {{reject}} --arch arm64 --kernel target/{{arm64}}/debug/bsd \
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
smoke-tcp: (build-amd64 "--features qemu") (build-arm64 "--features qemu")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-tcp: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke2 {{reject}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --timeout 300 \
        {{esp_both}} {{tcp_a}} {{tcp_b}} {{tcp_expect}}
    cargo xtask smoke2 {{reject}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --timeout 300 \
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

# M9+: pf's divert-to between the two VMs of `smoke-link`. B gives lo0 its 127.0.0.1 (as
# netstart(8) would), loads a rule that diverts TCP to its port 80 arriving on vio1 to
# 127.0.0.1 port 8080 (pf.conf(5), `divert-to`) and listens there with `nc -l`; nothing
# listens on port 80. A writes a line to B's port 80 with
# `nc -N`, retrying every second until B's listener is up: pf_test marks the packet
# PF_DIVERT and tcp_input finds the listener through in_pcblookup_listen's divert lookup.
# Part of `smoke`.
smoke-divert: (build-amd64 "--features qemu") (build-arm64 "--features qemu")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-divert: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke2 {{reject}} --arch amd64 --kernel target/{{amd64}}/debug/bsd \
        {{divert_both}} {{divert_a}} {{divert_b}} {{divert_expect}}
    cargo xtask smoke2 {{reject}} --arch arm64 --kernel target/{{arm64}}/debug/bsd \
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
smoke-tcpdump: (build-amd64 "--features qemu") (build-arm64 "--features qemu")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-tcpdump: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke2 {{reject}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --timeout 300 \
        {{tcpdump_both}} {{tcpdump_a}} {{tcpdump_b}} {{tcpdump_expect}}
    cargo xtask smoke2 {{reject}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --timeout 300 \
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
smoke-disk: (build-amd64 "--features qemu") (build-arm64 "--features qemu")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-disk: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke {{reject}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen \
        --disk-fresh {{disk_make}} --expect 'vioblk0 at virtio1'
    cargo xtask smoke {{reject}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen \
        {{disk_check}}
    cargo xtask smoke {{reject}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --expect-ramdisk --until-seen \
        --disk-fresh {{disk_make}} --expect 'vioblk0 at virtio29'
    cargo xtask smoke {{reject}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --expect-ramdisk --until-seen \
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
smoke-ufsopts: (build-amd64 "--features qemu") (build-arm64 "--features qemu")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-ufsopts: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke {{reject}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen \
        --disk-fresh {{ufsopts_quota}}
    cargo xtask smoke {{reject}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen \
        {{ufsopts_dirhash_mfs}}
    cargo xtask smoke {{reject}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --expect-ramdisk --until-seen \
        --disk-fresh {{ufsopts_quota}}
    cargo xtask smoke {{reject}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --expect-ramdisk --until-seen \
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

# M9+: IPv6 between the two VMs of `smoke-link` (option INET6, sys/netinet6). Bringing lo0
# up gives it ::1 (if_up calls in6_ifattach for the default loopback); vio1 gets fd00:77::1
# on A and fd00:77::2 on B, and in6_ifattach its EUI-64 link-local address (B's MAC
# 52:54:00:bb:00:02 makes fe80::5054:ff:febb:2). A pings B's global and link-local addresses
# with ping6 (OpenBSD's ping, linked as ping6), B pings A's global one. ndp(8) is not in the
# reference clone. Part of `smoke`.
smoke-inet6: (build-amd64 "--features qemu") (build-arm64 "--features qemu")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-inet6: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke2 {{reject}} --arch amd64 --kernel target/{{amd64}}/debug/bsd {{inet6_sends}} {{inet6_expects}}
    cargo xtask smoke2 {{reject}} --arch arm64 --kernel target/{{arm64}}/debug/bsd {{inet6_sends}} {{inet6_expects}}

# `smoke-inet6`'s sends and expectations.
inet6_sends := "--both-send-after login: --both-send 'root\\n' --both-send-after Password: --both-send 'emibsd\\n' " + \
    "--both-send-after '# ' --both-send 'ifconfig lo0 inet 127.0.0.1/8 up\\n' --both-send-after '# ' --both-send 'ifconfig lo0\\n' " + \
    "--a-send-after '# ' --a-send 'ifconfig vio1 inet6 fd00:77::1/64 up\\n' " + \
    "--b-send-after '# ' --b-send 'ifconfig vio1 inet6 fd00:77::2/64 up\\n' " + \
    "--a-send-after '# ' --a-send 'ifconfig vio1\\n' --b-send-after '# ' --b-send 'ifconfig vio1\\n' " + \
    "--a-send-after '# ' --a-send 'ping6 -c 3 fd00:77::2\\n' " + \
    "--a-send-after '# ' --a-send 'ping6 -c 3 fe80::5054:ff:febb:2%vio1\\n' " + \
    "--b-send-after '# ' --b-send 'ping6 -c 3 fd00:77::1\\n'"
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
smoke-fs: (build-amd64 "--features qemu") (build-arm64 "--features qemu")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-fs: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke {{reject}} --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen {{fs_steps}}
    cargo xtask smoke {{reject}} --arch arm64 --kernel target/{{arm64}}/debug/bsd --expect-ramdisk --until-seen {{fs_steps}}

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
