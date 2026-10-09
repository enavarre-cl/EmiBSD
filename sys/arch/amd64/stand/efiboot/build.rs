/* <LICENSES> */
/*
 * Copyright (c) 2026 Emilio Navarrete Lineros <enavarre@outlook.com>
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */
/* </LICENSES> */

/* <CODE> */
//! Build script: hands `ldscript.amd64` to the linker, bare-metal only, with the flags of
//! efiboot's `Makefile.common` that lld needs here (`-Bsymbolic`, no packed relocations) and
//! no RELRO segment (the script puts the relocated read-only data in the one `.data`).

use std::env;

fn main() {
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "none" {
        return;
    }
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    let ld = format!("{manifest_dir}/ldscript.amd64");
    println!("cargo:rustc-link-arg-bins=-T{ld}");
    println!("cargo:rustc-link-arg-bins=-Bsymbolic");
    println!("cargo:rustc-link-arg-bins=--pack-dyn-relocs=none");
    println!("cargo:rustc-link-arg-bins=-znorelro");
    println!("cargo:rerun-if-changed={ld}");
    println!("cargo:rerun-if-changed={manifest_dir}/start_amd64.S");
    println!("cargo:rerun-if-changed={manifest_dir}/run_i386.S");
}
/* </CODE> */
