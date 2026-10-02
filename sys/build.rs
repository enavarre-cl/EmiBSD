//! Build script: hands the per-architecture linker script to the linker, bare-metal only.
//!
//! Host builds (tests, `cargo check`, the `sys/arch/host` double) never see it.

use std::env;

// A build script that cannot determine its target has nothing sensible to do but stop.
#[allow(clippy::panic)]
fn main() {
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "none" {
        return;
    }
    let arch = match env::var("CARGO_CFG_TARGET_ARCH")
        .unwrap_or_default()
        .as_str()
    {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => panic!("bsd: unsupported target_arch `{other}`; expected x86_64 or aarch64"),
    };
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    let ld = format!("{manifest_dir}/arch/{arch}/conf/kernel.ld");
    println!("cargo:rustc-link-arg-bins=-T{ld}");
    println!("cargo:rerun-if-changed={ld}");
}
