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

# Boots per arch: a plain one that must reach the end of main() (status 33), printing the
# EmiBSD 7.8 version banner, with init checking its identity through sysctl(2) and the vfs system
# calls failing as they must with no root file system yet (`main` says it cannot mount root and
# `check_console` that /dev/console does not exist); `boot -d`, which
# enters ddb-lite through a breakpoint trap, prints where it stopped and continues (status 33);
# `selftest=trap`, a deliberate bad access that must print OpenBSD's fatal trap message and
# panic with a stack trace (status 35); and `selftest=uart`, which arms the console's receive
# interrupt, gets a line typed on the serial console and echoes it (status 33);
# `selftest=clock`, which waits for hz clock interrupts and a timeout (status 33);
# `selftest=kthread`, two kernel threads passing a turn with msleep/wakeup (status 33); and
# `selftest=taskq`, tasks run by systq, systqmp and a created then destroyed queue (status 33).
smoke: (build-amd64 "--features qemu") (build-arm64 "--features qemu") build-init-amd64 build-init-arm64
    cargo xtask smoke --arch amd64 --kernel target/{{amd64}}/debug/bsd \
        --expect "bsd: booted on amd64" --expect "The Regents of the University of California" \
        --expect "EmiBSD 7.8 (GENERIC) #" \
        --expect "real mem = " --expect "avail mem = " --expect "selftest: pmap kernel mapping ok" \
        --expect "selftest: malloc/pool stress ok" --expect "selftest: mbufs ok" \
        --expect "selftest: bus_dma ok" --expect "mainbus0 at root" \
        --expect "cpu0 at mainbus0: (uniprocessor)" --expect "pci0 at mainbus0 bus 0" \
        --expect "at pci0 dev 0 function 0 not configured" \
        --expect "cpu0: apic clock running at" \
        --expect "module: /init (" --expect "init: hello from user mode" \
        --expect "init: demand-zero bss ok" --expect "init: ids and tcb ok" \
        --expect "init: fds ok" --expect "init: signals ok" --expect "init: EmiBSD 7.8" \
        --expect "cannot mount root: no root file system" \
        --expect "warning: /dev/console does not exist" --expect "init: vfs ok (no root file system)" \
        --expect "init exited with status 0 (signal 0)"
    cargo xtask smoke --arch amd64 --kernel target/{{amd64}}/debug/bsd --cmdline "-d" \
        --expect "Stopped at" --expect "selftest: malloc/pool stress ok"
    cargo xtask smoke --arch amd64 --kernel target/{{amd64}}/debug/bsd --cmdline "selftest=trap" --status 35 \
        --expect "fatal page fault in supervisor mode" --expect "trap type 6 code" \
        --expect "panic: trap type 6, code=" --expect "Starting stack trace..." \
        --expect "End of stack trace." --expect "The operating system has halted."
    cargo xtask smoke --arch amd64 --kernel target/{{amd64}}/debug/bsd --cmdline "selftest=uart" \
        --send-after "selftest: uart rx interrupt armed" --send 'hello\n' \
        --expect "selftest: uart rx interrupt armed" --expect "selftest: uart echo: hello"
    cargo xtask smoke --arch amd64 --kernel target/{{amd64}}/debug/bsd --cmdline "selftest=clock" \
        --expect "selftest: clock ok"
    cargo xtask smoke --arch amd64 --kernel target/{{amd64}}/debug/bsd --cmdline "selftest=kthread" \
        --expect "selftest: kthread ping-pong ok"
    cargo xtask smoke --arch amd64 --kernel target/{{amd64}}/debug/bsd --cmdline "selftest=taskq" \
        --expect "selftest: taskq ok"
    cargo xtask smoke --arch arm64 --kernel target/{{arm64}}/debug/bsd \
        --expect "bsd: booted on arm64" --expect "The Regents of the University of California" \
        --expect "EmiBSD 7.8 (GENERIC) #" \
        --expect "real mem  = " --expect "avail mem = " --expect "selftest: pmap kernel mapping ok" \
        --expect "selftest: malloc/pool stress ok" --expect "selftest: mbufs ok" \
        --expect "mainbus0 at root" --expect "ampintc0 at mainbus0 nirq " \
        --expect "agtimer0 at mainbus0: " --expect "selftest: bus_dma ok" \
        --expect "module: /init (" --expect "init: hello from user mode" \
        --expect "init: demand-zero bss ok" --expect "init: ids and tcb ok" \
        --expect "init: fds ok" --expect "init: signals ok" --expect "init: EmiBSD 7.8" \
        --expect "cannot mount root: no root file system" \
        --expect "warning: /dev/console does not exist" --expect "init: vfs ok (no root file system)" \
        --expect "init exited with status 0 (signal 0)"
    cargo xtask smoke --arch arm64 --kernel target/{{arm64}}/debug/bsd --cmdline "-d" \
        --expect "Stopped at" --expect "selftest: malloc/pool stress ok"
    cargo xtask smoke --arch arm64 --kernel target/{{arm64}}/debug/bsd --cmdline "selftest=trap" --status 35 \
        --expect "panic: uvm_fault failed:" --expect "Starting stack trace..." \
        --expect "End of stack trace." --expect "The operating system has halted."
    cargo xtask smoke --arch arm64 --kernel target/{{arm64}}/debug/bsd --cmdline "selftest=uart" \
        --send-after "selftest: uart rx interrupt armed" --send 'hello\n' \
        --expect "selftest: uart rx interrupt armed" --expect "selftest: uart echo: hello"
    cargo xtask smoke --arch arm64 --kernel target/{{arm64}}/debug/bsd --cmdline "selftest=clock" \
        --expect "selftest: clock ok"
    cargo xtask smoke --arch arm64 --kernel target/{{arm64}}/debug/bsd --cmdline "selftest=kthread" \
        --expect "selftest: kthread ping-pong ok"
    cargo xtask smoke --arch arm64 --kernel target/{{arm64}}/debug/bsd --cmdline "selftest=taskq" \
        --expect "selftest: taskq ok"

# annotate a stack trace (paste it on stdin) with the debug kernel's symbols
symbolize arch:
    cargo xtask symbolize --arch {{arch}}

# --- quality -----------------------------------------------------------------

# host unit tests (libkern + bsd through sys/arch/host, plus xtask's own)
test:
    cargo test -p libkern -p bsd -p xtask

# tests that cross-check constants against the C reference tree
test-ref:
    OPENBSD_SRC=reference/openbsd-src cargo test -p libkern -p bsd -- --ignored

# bare targets with `--features qemu`: a superset of the plain build, which `just build` covers
clippy:
    cargo clippy -p bsd --target {{amd64}} --features qemu -- -D warnings
    cargo clippy -p bsd --target {{arm64}} --features qemu -- -D warnings
    cargo clippy -p init --target {{amd64}} -- -D warnings
    cargo clippy -p init --target {{arm64}} -- -D warnings
    cargo clippy -p libkern -p bsd -p xtask -- -D warnings

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
