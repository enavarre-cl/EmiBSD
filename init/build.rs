//! Build script: hands `init.ld` to the linker, bare-metal only. The host never builds this
//! crate (it is not a default workspace member).

use std::env;

fn main() {
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "none" {
        return;
    }
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    let ld = format!("{manifest_dir}/init.ld");
    println!("cargo:rustc-link-arg-bins=-T{ld}");
    // The kernel's setregs enables branch target identification (PT_OPENBSD_NOBTCFI opts
    // out); this binary carries no BTI landing pads yet.
    if env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default() == "aarch64" {
        println!("cargo:rustc-link-arg-bins=-znobtcfi");
    }
    println!("cargo:rerun-if-changed={ld}");
}
