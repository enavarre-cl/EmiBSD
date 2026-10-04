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
# The default boot's init stand-in also checks the Internet sockets (`init: inet sockets ok`:
# vio0's address through SIOCGIFADDR, a ping from a raw ICMP socket, a local UDP datagram) and
# PF_KEY (`init: pfkey ok`: SADB_REGISTER on a PF_KEY socket and its answer).
# All of those boot without a ramdisk (`--ramdisk none`, so the kernel says
# `rd: no ramdisk module`, `--expect-ramdisk`) and run the Rust stand-in init, the kernel's
# self-test. Then `smoke-shell` (M8's exit criterion) boots the ffs ramdisk `just userland`
# makes, booted `-s` (RB_SINGLE; a plain boot goes multi-user, see `smoke-login`): rd(4) reads
# its superblock, the root is mounted from rd0a, OpenBSD's init(8) runs from it and goes single
# user, and ksh(1) answers `uname -a`, `uname -sr`, `cat /etc/motd` and `ls /` on the serial
# console.
smoke: (build-amd64 "--features qemu") (build-arm64 "--features qemu") build-init-amd64 build-init-arm64 smoke-shell smoke-login smoke-net smoke-route smoke-link smoke-wg smoke-pf smoke-ipsec smoke-esp
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
        --expect "init: pipes ok" --expect "init: sockets ok" --expect "init: wg ok" --expect "init: kqueue ok" --expect "init: inet sockets ok" --expect "init: pfkey ok" --expect "init: processes ok" --expect "init: time ok" --expect "init: unveil ok" --expect "init: sendsyslog ok" --expect "pinsyscalls addr" \
        --expect "selftest: pmap reuse ok" --expect "selftest: ping 10.0.2.2: echo reply received" \
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
        --expect "init: pipes ok" --expect "init: sockets ok" --expect "init: wg ok" --expect "init: kqueue ok" --expect "init: inet sockets ok" --expect "init: pfkey ok" --expect "init: processes ok" --expect "init: time ok" --expect "init: unveil ok" --expect "init: sendsyslog ok" --expect "pinsyscalls addr" \
        --expect "selftest: pmap reuse ok" --expect "selftest: ping 10.0.2.2: echo reply received" \
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
# password, docs/SETUP.md) and runs `id` and `uname -a`. Part of `smoke`.
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
# through 10.0.2.2 before init ran. Part of `smoke`.
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
        --expect "lo0: flags=" --expect "vio0: flags=" {{https_run}}
    cargo xtask smoke --arch arm64 --kernel target/{{arm64}}/debug/bsd --expect-ramdisk --until-seen \
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
# `-R` the CA, `-e` the expected name). Needs `just userland`. NOT in `smoke` until the kernel
# has TCP (ported in parallel): today ftp stops at `socket: Protocol not supported`.
smoke-https: (build-amd64 "--features qemu") (build-arm64 "--features qemu")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-https: no ramdisk image; run just userland first"; exit 1; }
    @mkdir -p target/https-www && echo 'hello from emibsd-host over https' >target/https-www/hello.txt
    cargo xtask smoke --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen {{https_check}}
    cargo xtask smoke --arch arm64 --kernel target/{{arm64}}/debug/bsd --expect-ramdisk --until-seen {{https_check}}

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
    cargo xtask smoke --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen {{internet_check}}
    cargo xtask smoke --arch arm64 --kernel target/{{arm64}}/debug/bsd --expect-ramdisk --until-seen {{internet_check}}

internet_check := "--send-after login: --send 'root\\n' --send-after Password: --send 'emibsd\\n' " + \
    "--send-after '# ' --send 'ftp -o - https://www.openbsd.org/robots.txt\\n' " + \
    "--expect 'rc: multi-user' --expect 'User-agent:'"

# M9b/M9c harness: two VMs of one arch at once (`cargo xtask smoke2`), each with vio0 on QEMU's
# user network and vio1 on a private link between the two (docs/SETUP.md, "Two VMs"). Both log
# in as root, give vio1 an address on 192.168.77.0/24 and ping each other across the link.
smoke-link: (build-amd64 "--features qemu") (build-arm64 "--features qemu")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-link: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke2 --arch amd64 --kernel target/{{amd64}}/debug/bsd \
        --both-send-after "login:" --both-send 'root\n' --both-send-after "Password:" --both-send 'emibsd\n' \
        --a-send-after "# " --a-send 'ifconfig vio1 inet 192.168.77.1/24 up\n' \
        --a-send-after "# " --a-send 'ping -c 10 192.168.77.2\n' \
        --b-send-after "# " --b-send 'ifconfig vio1 inet 192.168.77.2/24 up\n' \
        --b-send-after "# " --b-send 'ping -c 10 192.168.77.1\n' \
        --a-expect "vio1 at virtio1: 1 queue, address 52:54:00:bb:00:01" \
        --b-expect "vio1 at virtio1: 1 queue, address 52:54:00:bb:00:02" \
        --a-expect "bytes from 192.168.77.2: icmp_seq=" --b-expect "bytes from 192.168.77.1: icmp_seq="
    cargo xtask smoke2 --arch arm64 --kernel target/{{arm64}}/debug/bsd \
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
    cargo xtask smoke2 --arch amd64 --kernel target/{{amd64}}/debug/bsd \
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
    cargo xtask smoke2 --arch arm64 --kernel target/{{arm64}}/debug/bsd \
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
    cargo xtask smoke --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen \
        --send-after "login:" --send 'root\n' --send-after "Password:" --send 'emibsd\n' \
        --send-after "# " --send 'ifconfig vio0\n' --send-after "# " --send 'ping -c 1 10.0.2.2\n' \
        --expect "rc: multi-user" --expect "vio0: flags=" --expect "inet 10.0.2.15 netmask 0xffffff00" \
        --expect "PING 10.0.2.2 (10.0.2.2): 56 data bytes" \
        --expect "1 packets transmitted, 1 packets received, 0.0% packet loss"
    cargo xtask smoke --arch arm64 --kernel target/{{arm64}}/debug/bsd --expect-ramdisk --until-seen \
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
    cargo xtask smoke --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen \
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
    cargo xtask smoke --arch arm64 --kernel target/{{arm64}}/debug/bsd --expect-ramdisk --until-seen \
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
    cargo xtask smoke --arch amd64 --kernel target/{{amd64}}/debug/bsd --expect-ramdisk --until-seen \
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
    cargo xtask smoke --arch arm64 --kernel target/{{arm64}}/debug/bsd --expect-ramdisk --until-seen \
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
# Both are gateways (net.inet.ip.forwarding=1): without bpf(4) (NBPFILTER 0) the C leaves a
# decapsulated packet on vio1 instead of moving it to enc0, and a plain host drops it as
# `ips_wrongif`, its inner address being on lo1. Each VM ends with `ipsecctl -sa -v` (the SA
# counters). Part of `smoke`.
smoke-esp: (build-amd64 "--features qemu") (build-arm64 "--features qemu")
    @test -f target/userland/amd64/ramdisk.ffs -a -f target/userland/arm64/ramdisk.ffs || \
        { echo "smoke-esp: no ramdisk image; run just userland first"; exit 1; }
    cargo xtask smoke2 --arch amd64 --kernel target/{{amd64}}/debug/bsd \
        {{esp_both}} \
        --a-send-after "# " --a-send 'ifconfig vio1 inet 192.168.77.1/24\n' \
        --a-send-after "# " --a-send 'ifconfig lo1 create; ifconfig lo1 inet 10.77.1.1/32\n' \
        {{esp_a}} \
        --b-send-after "# " --b-send 'ifconfig vio1 inet 192.168.77.2/24\n' \
        --b-send-after "# " --b-send 'ifconfig lo1 create; ifconfig lo1 inet 10.77.2.1/32\n' \
        {{esp_b}} \
        --a-expect "bytes from 192.168.77.2" --a-expect "bytes from 10.77.2.1" \
        --b-expect "bytes from 192.168.77.1" --b-expect "bytes from 10.77.1.1"
    cargo xtask smoke2 --arch arm64 --kernel target/{{arm64}}/debug/bsd \
        {{esp_both}} \
        --a-send-after "# " --a-send 'ifconfig vio1 inet 192.168.77.1/24\n' \
        --a-send-after "# " --a-send 'ifconfig lo1 create; ifconfig lo1 inet 10.77.1.1/32\n' \
        {{esp_a}} \
        --b-send-after "# " --b-send 'ifconfig vio1 inet 192.168.77.2/24\n' \
        --b-send-after "# " --b-send 'ifconfig lo1 create; ifconfig lo1 inet 10.77.2.1/32\n' \
        {{esp_b}} \
        --a-expect "bytes from 192.168.77.2" --a-expect "bytes from 10.77.2.1" \
        --b-expect "bytes from 192.168.77.1" --b-expect "bytes from 10.77.1.1"

# `smoke-esp`'s sends: the login and the keys on both VMs, then each VM's ipsec.conf (its
# flow, the SA pair), ipsecctl -f, a ping across the link, the ping through the tunnel and
# the SA counters.
esp_both := "--both-send-after 'login:' --both-send 'root\\n' --both-send-after 'Password:' --both-send 'emibsd\\n' " + \
    "--both-send-after '# ' --both-send 'cd /tmp; umask 077; sysctl net.inet.ip.forwarding=1\\n' " + \
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
