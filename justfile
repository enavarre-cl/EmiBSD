# openbsd-rs task runner. `just` lists recipes. Never call qemu or `cargo --target` by hand.
set shell := ["zsh", "-cu"]

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

build: build-amd64 build-arm64

# --- boot images and QEMU ---------------------------------------------------

image-amd64: (build-amd64 "--features qemu")
    cargo xtask image --arch amd64 --kernel target/{{amd64}}/debug/bsd

image-arm64: (build-arm64 "--features qemu")
    cargo xtask image --arch arm64 --kernel target/{{arm64}}/debug/bsd

run-amd64: image-amd64
    cargo xtask qemu --arch amd64

run-arm64: image-arm64
    cargo xtask qemu --arch arm64

smoke: image-amd64 image-arm64
    cargo xtask smoke --arch amd64 --expect "bsd: booted on amd64"
    cargo xtask smoke --arch arm64 --expect "bsd: booted on arm64"

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
    cargo clippy -p libkern -p bsd -p xtask -- -D warnings

fmt:
    cargo fmt --all -- --check

check-ports:
    cargo xtask ports check

drift:
    cargo xtask ports drift

ci: fmt clippy test build smoke check-ports
