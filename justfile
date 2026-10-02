# openbsd-rs task runner. `just` lists recipes. Never call qemu or `cargo --target` by hand.
set shell := ["zsh", "-cu"]

amd64 := "x86_64-unknown-none"
arm64 := "aarch64-unknown-none-softfloat"

default:
    @just --list

# --- build -------------------------------------------------------------------

build-amd64:
    cargo build -p bsd --target {{amd64}}

build-arm64:
    cargo build -p bsd --target {{arm64}}

build: build-amd64 build-arm64

# --- boot images and QEMU (xtask subcommands arrive with milestone M0) -------

image-amd64: build-amd64
    cargo xtask image --arch amd64 --kernel target/{{amd64}}/debug/bsd

image-arm64: build-arm64
    cargo xtask image --arch arm64 --kernel target/{{arm64}}/debug/bsd

run-amd64: image-amd64
    cargo xtask qemu --arch amd64

run-arm64: image-arm64
    cargo xtask qemu --arch arm64

smoke: image-amd64 image-arm64
    cargo xtask smoke --arch amd64 --expect "bsd: booted on amd64"
    cargo xtask smoke --arch arm64 --expect "bsd: booted on arm64"

# --- quality -----------------------------------------------------------------

# host unit tests (libkern + bsd through sys/arch/host)
test:
    cargo test -p libkern -p bsd

# tests that cross-check constants against the C reference tree
test-ref:
    OPENBSD_SRC=reference/openbsd-src cargo test -p bsd -- --ignored

clippy:
    cargo clippy -p bsd --target {{amd64}} -- -D warnings
    cargo clippy -p bsd --target {{arm64}} -- -D warnings
    cargo clippy -p libkern -p bsd -p xtask -- -D warnings

fmt:
    cargo fmt --all -- --check

check-ports:
    cargo xtask ports check

drift:
    cargo xtask ports drift

ci: fmt clippy test build smoke check-ports
