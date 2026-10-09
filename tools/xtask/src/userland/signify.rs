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
//! signify(1) for this machine, and the install media's test key (M14c).
//!
//! The install sets are signed (`SHA256.sig`) and the installer checks the signature with the
//! public key in its `/etc/signify`. The key is a test key made here, never one of OpenBSD's;
//! the signing tool is OpenBSD's own `usr.bin/signify` built for macOS, like makefs and
//! pwd_mkdb (the C is unmodified; the shims are this file's and no source is edited):
//!
//! - a force-included header (`COMPAT_H`): `pledge`/`unveil` as no-ops (macOS has neither),
//!   `__dead`, `explicit_bzero` and `freezero` (macOS has neither), libc's symbol-namespace
//!   macros as nothing, `MAKE_CLONE` as a forwarding function (the `SHA*Update` aliases of
//!   `lib/libc/hash/sha256.c` and `sha512.c`, which use a weak alias macOS has no equivalent
//!   of);
//! - OpenBSD's own `<sha2.h>` (with libc's `hidden/` wrapper in front of it, found first and
//!   `#include_next`ing the real one), `<blf.h>` and `<ohash.h>` from the clone, a `<util.h>`
//!   that declares only `bcrypt_pbkdf` (OpenBSD's does not compile on macOS and signify takes
//!   nothing else from it) and an `<endian.h>` over `<libkern/OSByteOrder.h>`;
//! - the libraries signify links, as OpenBSD's own sources: `lib/libutil/ohash.c` and
//!   `bcrypt_pbkdf.c`, `lib/libc/crypt/blowfish.c`, `lib/libc/hash/sha256.c`, `sha512.c` and
//!   the `helper.c` instances (`SHA256File`, `SHA512File`, `SHA512_256Data`, made by the
//!   `sed` of `lib/libc/hash/Makefile.inc`). `sha512.c` is compiled without its aarch64 and
//!   amd64 `__sha512_block` assembly (`-U__aarch64__ -U__amd64__`: the C fallback);
//! - `b64_pton` and `b64_ntop` come from macOS's libresolv.

use super::ramdisk::write_if_changed;
use super::*;

/// The header force-included into every source.
const COMPAT_H: &str = "\
/* EmiBSD: host shims for building OpenBSD's signify(1) on macOS (tools/xtask, signify.rs). */
#include <sys/types.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <resolv.h>
#define pledge(p, e) 0
#define unveil(p, e) 0
#ifndef __dead
#define __dead __attribute__((__noreturn__))
#endif
static inline void
emibsd_explicit_bzero(void *p, size_t n)
{
	volatile unsigned char *v = p;

	while (n--)
		*v++ = 0;
}
#define explicit_bzero emibsd_explicit_bzero
static inline void
emibsd_freezero(void *p, size_t n)
{
	if (p) {
		emibsd_explicit_bzero(p, n);
		free(p);
	}
}
#define freezero emibsd_freezero
#define DEF_WEAK(x)
#define DEF_STRONG(x)
#define PROTO_NORMAL(x)
#define MAKE_CLONE(x, y) void x(SHA2_CTX *c, const uint8_t *d, size_t n) { y(c, d, n); }
#define __BEGIN_HIDDEN_DECLS
#define __END_HIDDEN_DECLS
";

/// `<util.h>`: the one function signify takes from it.
const UTIL_H: &str = "\
#include <stdint.h>
#include <stddef.h>
int bcrypt_pbkdf(const char *, size_t, const uint8_t *, size_t, uint8_t *, size_t, unsigned int);
";

/// `<endian.h>` over macOS's byte-swap functions.
const ENDIAN_H: &str = "\
#include <libkern/OSByteOrder.h>
#define htobe64(x) OSSwapHostToBigInt64(x)
#define htobe32(x) OSSwapHostToBigInt32(x)
#define be64toh(x) OSSwapBigToHostInt64(x)
#define be32toh(x) OSSwapBigToHostInt32(x)
#define betoh64(x) OSSwapBigToHostInt64(x)
#define betoh32(x) OSSwapBigToHostInt32(x)
#define htole64(x) OSSwapHostToLittleInt64(x)
#define htole32(x) OSSwapHostToLittleInt32(x)
#define le64toh(x) OSSwapLittleToHostInt64(x)
#define le32toh(x) OSSwapLittleToHostInt32(x)
";

/// The `helper.c` instances libc's `Makefile.inc` makes with `sed`: (hash name).
const HELPERS: &[&str] = &["SHA256", "SHA512", "SHA512_256"];

/// Builds OpenBSD's signify for this machine; returns the executable.
pub(super) fn build_host_signify(ctx: &Ctx<'_>) -> Result<PathBuf> {
    let src = &ctx.src;
    let dir = ctx.out.join("host/obj/usr.bin/signify");
    let inc = dir.join("emibsd-include");
    let real = inc.join("real");
    let gen_dir = dir.join("gen");
    for d in [&real, &gen_dir] {
        fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    write_if_changed(&inc.join("emibsd-compat.h"), COMPAT_H)?;
    write_if_changed(&inc.join("util.h"), UTIL_H)?;
    write_if_changed(&inc.join("endian.h"), ENDIAN_H)?;
    for (link, target) in [
        (inc.join("sha2.h"), src.join("lib/libc/hidden/sha2.h")),
        (real.join("sha2.h"), src.join("include/sha2.h")),
        (inc.join("blf.h"), src.join("include/blf.h")),
        (inc.join("ohash.h"), src.join("lib/libutil/ohash.h")),
    ] {
        symlink(&target.to_string_lossy(), &link)?;
    }
    let helper = fs::read_to_string(src.join("lib/libc/hash/helper.c"))
        .map_err(|e| format!("lib/libc/hash/helper.c: {e}"))?;
    let mut generated = Vec::new();
    for algo in HELPERS {
        // `sed -e 's/hashinc/sha2.h/g' -e 's/HASH/<algo>/g' -e 's/SHA[0-9][0-9][0-9]_CTX/SHA2_CTX/g'
        // -e 's/SHA512_256_CTX/SHA2_CTX/g'`.
        let mut text = helper.replace("hashinc", "sha2.h").replace("HASH", algo);
        for ctx_name in [
            "SHA256_CTX",
            "SHA384_CTX",
            "SHA512_CTX",
            "SHA224_CTX",
            "SHA512_256_CTX",
        ] {
            text = text.replace(ctx_name, "SHA2_CTX");
        }
        let p = gen_dir.join(format!("{algo}hl.c"));
        write_if_changed(&p, &text)?;
        generated.push(p);
    }

    let tool = src.join("usr.bin/signify");
    let mut sources: Vec<PathBuf> = [
        "signify.c",
        "zsig.c",
        "fe25519.c",
        "sc25519.c",
        "mod_ed25519.c",
        "mod_ge25519.c",
        "crypto_api.c",
    ]
    .iter()
    .map(|f| tool.join(f))
    .collect();
    sources.extend(
        [
            "lib/libutil/ohash.c",
            "lib/libutil/bcrypt_pbkdf.c",
            "lib/libc/crypt/blowfish.c",
            "lib/libc/hash/sha256.c",
            "lib/libc/hash/sha512.c",
        ]
        .iter()
        .map(|f| src.join(f)),
    );
    sources.extend(generated);

    let exe = ctx.out.join("host/bin/signify");
    if let Some(bin) = exe.parent() {
        fs::create_dir_all(bin).map_err(|e| format!("{}: {e}", bin.display()))?;
    }
    let newest = sources
        .iter()
        .chain([
            &inc.join("emibsd-compat.h"),
            &inc.join("util.h"),
            &inc.join("endian.h"),
        ])
        .filter_map(|p| mtime(p))
        .max();
    if let (Some(built), Some(newest)) = (mtime(&exe), newest)
        && built >= newest
    {
        return Ok(exe);
    }
    let mut cc = Command::new(&ctx.tools.cc);
    cc.args(["-O1", "-w", "-U__aarch64__", "-U__amd64__", "-include"])
        .arg(inc.join("emibsd-compat.h"))
        .arg(format!("-I{}", inc.display()))
        .arg(format!("-I{}", real.display()))
        .arg(format!("-I{}", tool.display()))
        .arg(format!("-I{}", src.join("lib/libc/crypt").display()))
        .arg("-o")
        .arg(&exe)
        .args(&sources)
        .arg("-lresolv");
    println!(
        "  usr.bin/signify (host tool): built with the host shims of \
         tools/xtask/src/userland/signify.rs (OpenBSD's sha2.h, blf.h, ohash.h; lib/libutil, \
         lib/libc/crypt and lib/libc/hash sources; macOS's libresolv for b64_pton)"
    );
    run(&mut cc)?;
    Ok(exe)
}

/// The install media's test key pair (`openbsd-<rev>-base.pub`/`.sec`), made once under
/// `target/install/test-signify/`, with no passphrase. Returns (public, secret).
pub(super) fn test_keys(ctx: &Ctx<'_>, signify: &Path, rev: &str) -> Result<(PathBuf, PathBuf)> {
    let dir = ctx.root.join("target/install/test-signify");
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let public = dir.join(format!("openbsd-{rev}-base.pub"));
    let secret = dir.join(format!("openbsd-{rev}-base.sec"));
    if !public.is_file() || !secret.is_file() {
        let _ = fs::remove_file(&public);
        let _ = fs::remove_file(&secret);
        run(Command::new(signify)
            .args(["-G", "-n", "-p"])
            .arg(&public)
            .arg("-s")
            .arg(&secret)
            .args(["-c", "EmiBSD test key, not OpenBSD's"]))?;
    }
    Ok((public, secret))
}

/// `signify -S -e -s SECRET -m MESSAGE -x SIG`: signs `message`, embedding it in `sig`, the
/// form of OpenBSD's `SHA256.sig`.
pub(super) fn sign(signify: &Path, secret: &Path, message: &Path, sig: &Path) -> Result<()> {
    let _ = fs::remove_file(sig);
    run(Command::new(signify)
        .args(["-S", "-e", "-s"])
        .arg(secret)
        .arg("-m")
        .arg(message)
        .arg("-x")
        .arg(sig))
}

/// `signify -V -e -p PUBLIC -x SIG -m OUT`: checks `sig` and writes the message to `out`.
pub(super) fn verify(signify: &Path, public: &Path, sig: &Path, out: &Path) -> Result<()> {
    run(Command::new(signify)
        .args(["-V", "-e", "-p"])
        .arg(public)
        .arg("-x")
        .arg(sig)
        .arg("-m")
        .arg(out))
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shims_name_what_signify_takes() {
        assert!(UTIL_H.contains("bcrypt_pbkdf"));
        assert!(COMPAT_H.contains("MAKE_CLONE"));
        assert!(ENDIAN_H.contains("htobe64"));
        assert_eq!(HELPERS, ["SHA256", "SHA512", "SHA512_256"]);
    }
}
/* </TESTS> */
