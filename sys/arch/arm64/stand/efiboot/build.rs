//! Build script: hands `ldscript.arm64` to the linker, bare-metal only, with the flags of
//! efiboot's Makefile that lld needs here (`-Bsymbolic`, no packed relocations) and no RELRO
//! segment (the script puts the relocated read-only data in the one `.data`). The aarch64
//! `none` target links static executables (amd64's is static-pie already): `-pie` and
//! `--no-dynamic-linker` make the position-independent image with the `_DYNAMIC` and
//! `.rela.dyn` that `self_reloc` reads, where OpenBSD's Makefile links `-shared`; `-z notext`
//! lets the precompiled `core`/`alloc` (built for static linking) keep absolute addresses in
//! their read-only data, which become `R_AARCH64_RELATIVE` relocations (the script puts that
//! data in the writable `.data`).

use std::env;

fn main() {
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "none" {
        return;
    }
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    let ld = format!("{manifest_dir}/ldscript.arm64");
    println!("cargo:rustc-link-arg-bins=-T{ld}");
    println!("cargo:rustc-link-arg-bins=-pie");
    println!("cargo:rustc-link-arg-bins=--no-dynamic-linker");
    println!("cargo:rustc-link-arg-bins=-znotext");
    println!("cargo:rustc-link-arg-bins=-Bsymbolic");
    println!("cargo:rustc-link-arg-bins=--pack-dyn-relocs=none");
    println!("cargo:rustc-link-arg-bins=-znorelro");
    println!("cargo:rerun-if-changed={ld}");
    println!("cargo:rerun-if-changed={manifest_dir}/start.S");
}
