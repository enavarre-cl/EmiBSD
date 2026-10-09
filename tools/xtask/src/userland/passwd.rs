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
//! The password files of the ramdisk's `/etc`: `pwd.db` and `spwd.db` from `master.passwd`,
//! and the root password's bcrypt hash.
//!
//! OpenBSD's own `usr.sbin/pwd_mkdb` (in the reference clone since 2026-10-03, the user's
//! decision) is built for this machine, like makefs (`ramdisk.rs`) and `rpcgen`, and run as
//! `pwd_mkdb -p -d <staging>/etc <staging>/etc/master.passwd`. The databases must be
//! OpenBSD's format, so the db(3) library it uses is OpenBSD's too: `lib/libc/db` (the hash,
//! btree, recno and mpool sources; `ndbm.c` is not needed) is compiled into the tool, not
//! macOS's `dbopen`. The hash code writes its pages in the host's byte order and its header
//! in a fixed one (`BYTE_ORDER == LITTLE_ENDIAN` in `hash.c`); the host and both targets are
//! little-endian, which `COMPAT_H` checks at compile time.
//!
//! The hash is OpenBSD's `lib/libc/crypt/bcrypt.c` with `blowfish.c`, built into a tiny
//! helper (`emibsd-bcrypt`). Its salt comes from `arc4random_buf`, which the helper replaces
//! with a fixed 16 bytes (`BCRYPT_SALT`) before including the unmodified source, so the
//! hash, and with it the image, is the same on every build. `$2b$`, 8 rounds: what
//! `encrypt(1)` and `login.conf(5)`'s `localcipher=blowfish,8` produce.
//!
//! Host shims, all here and none in the sources (`docs/ARCHITECTURE.md`, "Userland build"):
//!
//! - a force-included header (`COMPAT_H`): `__BSD_VISIBLE` (OpenBSD's `<pwd.h>` hides the
//!   `_PATH_MP_DB` family without it), OpenBSD's `<pwd.h>` read before macOS's, `__dead`,
//!   libc's symbol-namespace macros (`DEF_WEAK`, `PROTO_NORMAL`, ...) as nothing,
//!   `explicit_bzero` (macOS has none);
//! - OpenBSD's own headers, from the clone, in a directory searched first: `<pwd.h>`,
//!   `<util.h>` (libutil's, for `pw_scan`), `<mpool.h>`, and libc's `hidden/db.h`, which
//!   declares the db(3) internals (`__bt_open`, ...) and then includes `<db.h>` itself
//!   (`#include_next`, found in `real/`);
//! - `lib/libutil/passwd.c`, for `pw_scan`;
//! - `getgrnam("_shadow")` (`COMPAT_C`): macOS has no such group. It answers with the
//!   running user's own group, so `pwd_mkdb`'s `fchown(spwd.db, -1, shadow)` succeeds
//!   without root; the image's real owner (root:_shadow) comes from the ownership table the
//!   makefs shim applies (`ramdisk.rs`).

use super::ramdisk::write_if_changed;
use super::*;

/// pwd_mkdb, relative to the OpenBSD sources.
const PWD_MKDB_DIR: &str = "usr.sbin/pwd_mkdb";

/// The root password of the test image (`docs/SETUP.md`).
pub(super) const ROOT_PASSWORD: &str = "emibsd";

/// bcrypt's cost: `log2(rounds)`, `encrypt(1)`'s default.
const BCRYPT_ROUNDS: u32 = 8;

/// The bcrypt salt: 16 bytes, fixed so that the image is reproducible. A test image's
/// password hash does not need to be unpredictable.
const BCRYPT_SALT: &str = "EmiBSD-test-salt";

/// The header force-included into every pwd_mkdb, libutil and db source.
const COMPAT_H: &str = "\
/* EmiBSD: host shims for building OpenBSD's pwd_mkdb(8) on macOS (tools/xtask, passwd.rs). */
#define __BSD_VISIBLE 1
#include <sys/types.h>
#include <pwd.h>
#include <sys/param.h>
#include <sys/stat.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <grp.h>
#if !defined(BYTE_ORDER) || !defined(LITTLE_ENDIAN) || BYTE_ORDER != LITTLE_ENDIAN
#error db(3) files are made by a little-endian host, as the targets are
#endif
#undef __dead
#define __dead __attribute__((__noreturn__))
#define DEF_WEAK(x)
#define DEF_STRONG(x)
#define PROTO_NORMAL(x)
#define PROTO_DEPRECATED(x)
#define __BEGIN_HIDDEN_DECLS
#define __END_HIDDEN_DECLS
static inline void
emibsd_explicit_bzero(void *p, size_t n)
{
	volatile unsigned char *v = p;

	while (n--)
		*v++ = 0;
}
#define explicit_bzero emibsd_explicit_bzero
struct group *emibsd_getgrnam(const char *);
#define getgrnam(n) emibsd_getgrnam(n)
";

/// `getgrnam` for the `_shadow` group (module docs).
const COMPAT_C: &str = r#"/* EmiBSD: getgrnam for the pwd_mkdb host build (tools/xtask, passwd.rs). */
#include <grp.h>
#include <string.h>
#include <unistd.h>

#undef getgrnam	/* the force-included header points getgrnam here */

struct group *
emibsd_getgrnam(const char *name)
{
	static struct group g;
	static char *members[1];

	if (strcmp(name, "_shadow") != 0)
		return getgrnam(name);
	g.gr_name = "_shadow";
	g.gr_passwd = "*";
	g.gr_gid = getgid();
	g.gr_mem = members;
	return &g;
}
"#;

/// The bcrypt helper: `bcrypt.c` and `blowfish.c` unmodified, `arc4random_buf` fixed.
/// Prints the hash of `argv[1]`, after checking it with `bcrypt_checkpass`.
const BCRYPT_HELPER_C: &str = r#"/* EmiBSD: reproducible bcrypt hashes with OpenBSD's own bcrypt.c (tools/xtask, passwd.rs). */
#include <sys/types.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

/* bcrypt_newhash() draws its salt from here; a fixed one makes the hash reproducible. */
static void
emibsd_salt(void *buf, size_t n)
{
	memcpy(buf, "@SALT@", n < 16 ? n : 16);
}
#define arc4random_buf(b, n) emibsd_salt(b, n)

static void
emibsd_explicit_bzero(void *p, size_t n)
{
	volatile unsigned char *v = p;

	while (n--)
		*v++ = 0;
}
#define explicit_bzero emibsd_explicit_bzero
#define DEF_WEAK(x)
#define DEF_STRONG(x)
#define WRAP(x) x

#include "@BCRYPT_C@"
#include "@BLOWFISH_C@"

int
main(int argc, char **argv)
{
	char hash[BCRYPT_HASHSPACE];

	if (argc != 2 || bcrypt_newhash(argv[1], @ROUNDS@, hash, sizeof(hash)) != 0)
		return 1;
	if (bcrypt_checkpass(argv[1], hash) != 0)
		return 2;
	puts(hash);
	return 0;
}
"#;

/// Builds pwd_mkdb for this machine; returns the executable.
pub(super) fn build_pwd_mkdb(ctx: &Ctx<'_>) -> Result<PathBuf> {
    if !ctx.src.join(PWD_MKDB_DIR).join("Makefile").is_file() {
        return Err(format!(
            "{PWD_MKDB_DIR}: not in the reference clone; the image would have no pwd.db"
        )
        .into());
    }
    Ok(
        build_host_prog_with(ctx, PWD_MKDB_DIR, |mk, objdir| shim(ctx, mk, objdir))?
            .join("pwd_mkdb"),
    )
}

/// Adds the host shims (module docs) and the db(3) sources to pwd_mkdb's evaluated Makefile.
fn shim(ctx: &Ctx<'_>, mk: &mut Make, objdir: &Path) -> Result<()> {
    let inc = objdir.join("emibsd-include");
    let real = inc.join("real");
    fs::create_dir_all(&real).map_err(|e| format!("{}: {e}", real.display()))?;
    let links = [
        (inc.join("pwd.h"), ctx.src.join("include/pwd.h")),
        (inc.join("util.h"), ctx.src.join("lib/libutil/util.h")),
        (
            inc.join("mpool.h"),
            ctx.src.join("lib/libc/include/mpool.h"),
        ),
        (inc.join("db.h"), ctx.src.join("lib/libc/hidden/db.h")),
        (real.join("db.h"), ctx.src.join("include/db.h")),
    ];
    for (link, target) in &links {
        symlink(&target.to_string_lossy(), link)?;
    }
    write_if_changed(&inc.join("emibsd-compat.h"), COMPAT_H)?;
    write_if_changed(&inc.join("emibsd_compat.c"), COMPAT_C)?;

    let cppflags = mk.var("CPPFLAGS")?;
    mk.set(
        "CPPFLAGS",
        &format!(
            "{cppflags} -w -include {} -I{} -I{}",
            inc.join("emibsd-compat.h").display(),
            inc.display(),
            real.display()
        ),
    );
    // SRCS: pwd_mkdb.c, libutil's passwd.c (pw_scan) and libc's db(3), taken from the
    // library's own Makefile.inc (SRCS and .PATH), less the ndbm(3) compatibility layer.
    mk.set("SRCS", "pwd_mkdb.c passwd.c emibsd_compat.c");
    mk.add_path(&inc);
    mk.add_path(&ctx.src.join("lib/libutil"));
    mk.set(
        "LIBCSRCDIR",
        &ctx.src.join("lib/libc").display().to_string(),
    );
    mk.read(&ctx.src.join("lib/libc/db/Makefile.inc"))?;
    mk.remove_word("SRCS", "ndbm.c");
    println!(
        "  {PWD_MKDB_DIR} (host tool): built with the host shims of tools/xtask/src/userland/passwd.rs \
         (OpenBSD's <pwd.h>, <util.h>, <mpool.h>, db.h; lib/libc/db, lib/libutil/passwd.c; \
         getgrnam(\"_shadow\") for the running user's group)"
    );
    Ok(())
}

/// The bcrypt hash of `password` (`$2b$08$...`), with the fixed salt (module docs).
pub(super) fn bcrypt_hash(ctx: &Ctx<'_>, password: &str) -> Result<String> {
    let dir = ctx.out.join("host/obj/lib/libc/crypt");
    let inc = dir.join("emibsd-include");
    fs::create_dir_all(&inc).map_err(|e| format!("{}: {e}", inc.display()))?;
    let crypt = ctx.src.join("lib/libc/crypt");
    let blf_h = ctx.src.join("include/blf.h");
    symlink(&blf_h.to_string_lossy(), &inc.join("blf.h"))?;
    let helper_c = dir.join("emibsd-bcrypt.c");
    write_if_changed(&helper_c, &bcrypt_helper_source(&ctx.src))?;

    let exe = ctx.out.join("host/bin/emibsd-bcrypt");
    if let Some(bin) = exe.parent() {
        fs::create_dir_all(bin).map_err(|e| format!("{}: {e}", bin.display()))?;
    }
    let job = Job {
        target: exe.clone(),
        cwd: dir,
        commands: vec![(
            format!(
                "{} -O2 -w -I{} -o {} {}",
                ctx.tools.cc.display(),
                inc.display(),
                exe.display(),
                helper_c.display()
            ),
            false,
        )],
        deps: vec![
            helper_c.clone(),
            crypt.join("bcrypt.c"),
            crypt.join("blowfish.c"),
            blf_h,
        ],
        path: None,
    };
    run_jobs(ctx, "lib/libc/crypt (host helper)", &[job])?;
    let out = Command::new(&exe)
        .arg(password)
        .output()
        .map_err(|e| format!("{}: {e}", exe.display()))?;
    if !out.status.success() {
        return Err(format!("{}: exit {}", exe.display(), out.status).into());
    }
    let hash = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !hash.starts_with(&format!("$2b${BCRYPT_ROUNDS:02}$")) {
        return Err(format!("{}: unexpected hash `{hash}`", exe.display()).into());
    }
    Ok(hash)
}

/// The helper's C source, for the sources under `src`.
fn bcrypt_helper_source(src: &Path) -> String {
    let crypt = src.join("lib/libc/crypt");
    BCRYPT_HELPER_C
        .replace("@SALT@", BCRYPT_SALT)
        .replace("@BCRYPT_C@", &crypt.join("bcrypt.c").display().to_string())
        .replace(
            "@BLOWFISH_C@",
            &crypt.join("blowfish.c").display().to_string(),
        )
        .replace("@ROUNDS@", &BCRYPT_ROUNDS.to_string())
}

/// `pwd_mkdb -p -d <etc> <etc>/master.passwd`: writes `pwd.db`, `spwd.db` and the V7
/// `passwd` into `etc`, as the system's `pwd_mkdb -p` does for `/etc`.
pub(super) fn make_databases(pwd_mkdb: &Path, etc: &Path) -> Result<()> {
    run(Command::new(pwd_mkdb)
        .args(["-p", "-d"])
        .arg(etc)
        .arg(etc.join("master.passwd")))?;
    for f in ["pwd.db", "spwd.db", "passwd"] {
        if !etc.join(f).is_file() {
            return Err(format!("pwd_mkdb did not make {}", etc.join(f).display()).into());
        }
    }
    Ok(())
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn salt_is_sixteen_bytes() {
        assert_eq!(BCRYPT_SALT.len(), 16);
        assert!(BCRYPT_SALT.is_ascii());
    }

    #[test]
    fn helper_source_has_no_placeholders_left() {
        let c = bcrypt_helper_source(Path::new("/src"));
        assert!(!c.contains('@'), "{c}");
        assert!(c.contains("\"/src/lib/libc/crypt/bcrypt.c\""));
        assert!(c.contains("bcrypt_newhash(argv[1], 8,"));
    }
}
/* </TESTS> */
