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
//! Shared libraries and the run-time link-editor (M14): what a plain `cc hello.c` on
//! EmiBSD links against and runs with, as on OpenBSD, where `cc` makes a dynamic PIE
//! (`-dynamic-linker /usr/libexec/ld.so`, `-lc` found as `libc.so.M.m`).
//!
//! - **Shared libraries** (`SHARED_LIBRARIES`): built the way `share/mk/bsd.lib.mk` builds
//!   `${FULLSHLIBNAME}`: every object of the static library again as a `.so` object
//!   (`${SOBJS}`, the `.c.so`/`.S.so` rules with `${PICFLAG} -DPIC` and `-DSOLIB`, and the
//!   Makefile's own `.so` rules, libc's system-call stubs), linked
//!   `cc -shared -Wl,-soname,lib${LIB}.so.${SHLIB_MAJOR}.${SHLIB_MINOR} ${PICFLAG}
//!   ${SOBJS} ${LDADD}` with the Makefile's `VERSION_SCRIPT` (libc's `Symbols.map` made by
//!   its own rule from the `Symbols.list` files). The version comes from the library's
//!   `shlib_version`. The `cc` driver's part of that link is done here with `ld.lld` as
//!   `build_prog` does: `crtbeginS.o`/`crtendS.o` and `-lcompiler_rt` unless `${LDADD}` says
//!   `-nostdlib` (libc, which names `-lcompiler_rt` itself). Installed into the sysroot and
//!   the staging root at `${LIBDIR}` (`/usr/lib`), `${LIBOWN}:${LIBGRP}` mode
//!   `${LIBMODE}` (root:bin 444, `bsd.own.mk`).
//! - **ld.so** (`libexec/ld.so`): its sources, the libc string functions of its `VPATH`,
//!   and the `dl_<syscall>.o` stubs its `.for` loop makes, linked by its rule
//!   (`${LD} -e _dl_start ${ELF_LDFLAGS}`: its `Symbols.map`, its `ld.script`, `--shared
//!   -Bsymbolic --no-undefined`), checked by its `CHECK_LDSO` (only `RELATIVE_RELOC`
//!   dynamic relocations, read with `llvm-objdump` where the Makefile runs `readelf`), and
//!   installed at `/usr/libexec/ld.so`, `${BINOWN}:${BINGRP}` 444, unstripped
//!   (`INSTALL_STRIP=`).
//!
//! ## Deviations
//!
//! - Objects are linked in their Makefile order; `bsd.lib.mk` and ld.so's rule shuffle them
//!   (`sort -R`) so that each build has another layout. The boot-time relinking that keeps
//!   doing so on OpenBSD (`LIBREBUILD`'s `/usr/share/relink` kits, `ld.so.a`) is not made.
//! - ld.so's `test-ld.so` (a program linked against the new ld.so and run on the build
//!   machine) is neither built nor run, as in OpenBSD's cross builds (`CROSSDIR`); the
//!   `smoke-cc` run on EmiBSD is that test.
//! - ld.so is built with `STACK_PROTECTOR=` (its stack-protector stubs, `__guard_local`
//!   and `__stack_smash_handler`, and the `sendsyslog` stub they use), which OpenBSD sets
//!   only on the architectures its clang has no retguard for: on amd64 and arm64 OpenBSD's
//!   clang protects returns with retguard (`-ret-protector`) and so emits no stack
//!   protector, while the host's clang has no retguard and emits the stack protector, as
//!   OpenBSD's clang does on those other architectures.
//! - Profiled libraries (`*_p.a`) are not built, as nowhere in `userland`.

use super::*;

/// The libraries built shared besides their static archive, in link order of dependence:
/// libc first (the others' `.so` objects compile against the sysroot it filled). `libpthread`
/// (`lib/librthread`) is also built static here, as `comp` builds it for the compiler.
pub(super) const SHARED_LIBRARIES: &[&str] =
    &["lib/libc", "lib/libutil", "lib/libm", "lib/librthread"];

/// The run-time link-editor's directory.
const LDSO_DIR: &str = "libexec/ld.so";

/// Defaults of OpenBSD's lld (`gnu/llvm/lld/ELF/Driver.cpp`, `#ifdef __OpenBSD__`) that the
/// host's LLD 17 does not have, said explicitly for the shared links: a version script
/// may name symbols the library does not define (libc's `Symbols.list` names `__data_start`
/// and the `quad` helpers of other architectures), and protected symbols stay preemptible.
/// (Its execute-only text default is not repeated: LLD 17 refuses `--execute-only` on
/// amd64, and the static programs are linked without it too.)
const OPENBSD_LLD_DEFAULTS: &[&str] =
    &["--undefined-version", "--ignore-function-address-equality"];

/// Builds and installs every shared library of `SHARED_LIBRARIES` and ld.so.
pub(super) fn build_shared(ctx: &Ctx<'_>) -> Result<()> {
    for dir in SHARED_LIBRARIES {
        build_shared_lib(ctx, dir)?;
    }
    build_ldso(ctx)
}

/// `shlib_version`'s `major=` and `minor=` (the file `bsd.lib.mk` includes), or `None`
/// when the library has none (it is then not built shared).
fn shlib_version(text: &str) -> Option<(String, String)> {
    let get = |key: &str| {
        text.lines()
            .map(|l| l.split('#').next().unwrap_or("").trim())
            .find_map(|l| {
                let (k, v) = l.split_once('=')?;
                (k.trim() == key).then(|| v.trim().to_string())
            })
            .filter(|v| !v.is_empty())
    };
    Some((get("major")?, get("minor")?))
}

/// The linker arguments of a `cc` command line's `LDADD` words: `-Wl,a,b` gives `a b`,
/// `-l`/`-L` words stay; `-nostdlib` is returned as a flag (the driver then adds no start
/// files and no default libraries).
fn ldadd_args(words: &[String]) -> Result<(Vec<String>, bool)> {
    let mut args = Vec::new();
    let mut nostdlib = false;
    for w in words {
        if let Some(rest) = w.strip_prefix("-Wl,") {
            args.extend(rest.split(',').map(str::to_string));
        } else if w.starts_with("-l") || w.starts_with("-L") {
            args.push(w.clone());
        } else if w == "-nostdlib" {
            nostdlib = true;
        } else {
            return Err(format!("unsupported LDADD word `{w}` for a shared library").into());
        }
    }
    Ok((args, nostdlib))
}

/// `${FULLSHLIBNAME}` of the library in `dir`, built and installed.
fn build_shared_lib(ctx: &Ctx<'_>, dir: &str) -> Result<()> {
    let objdir = ctx.out.join("obj").join(dir);
    fs::create_dir_all(&objdir).map_err(|e| format!("{}: {e}", objdir.display()))?;
    let mk = new_make(ctx, dir, &objdir)?;
    let lib = mk.var("LIB")?;
    let version_file = ctx.src.join(dir).join("shlib_version");
    let text = fs::read_to_string(&version_file)
        .map_err(|e| format!("{}: {e}", version_file.display()))?;
    let (major, minor) = shlib_version(&text)
        .ok_or_else(|| format!("{}: no major= and minor=", version_file.display()))?;
    let name = format!("lib{lib}.so.{major}.{minor}");

    // `BUILDFIRST`, the `.so` objects, and the version script (libc's is made by a rule).
    let mut made = BTreeSet::new();
    for t in mk.words("BUILDFIRST")? {
        libraries::make_target(ctx, &mk, &objdir, &t, None, &mut made)?;
    }
    let jobs = object_jobs_as(ctx, &mk, &objdir, &mk.words("OBJS")?, ObjKind::Pic)?;
    let ran = run_jobs(ctx, &format!("{dir} (shared)"), &jobs)?;
    let script = mk.var("VERSION_SCRIPT")?;
    let script = if script.is_empty() {
        None
    } else if mk.rule_for(&script).is_some() {
        Some(libraries::make_target(
            ctx, &mk, &objdir, &script, None, &mut made,
        )?)
    } else {
        Some(
            mk.search(&script)
                .ok_or_else(|| format!("{dir}: no VERSION_SCRIPT {script}"))?,
        )
    };

    let (ldadd, nostdlib) = ldadd_args(&mk.words("LDADD")?)?;
    let libdir = ctx.sysroot.join("usr/lib");
    let out = objdir.join(&name);
    let mut ld = Command::new(&ctx.tools.ld);
    ld.arg(format!("--sysroot={}", ctx.sysroot.display()))
        .args(OPENBSD_LLD_DEFAULTS)
        .args(["--eh-frame-hdr", "-shared", "-o"])
        .arg(&out);
    if !nostdlib {
        ld.arg(libdir.join("crtbeginS.o"));
    }
    ld.arg(format!("-L{}", libdir.display()));
    for j in &jobs {
        ld.arg(&j.target);
    }
    ld.args(["-soname", &name]).args(&ldadd);
    if let Some(script) = &script {
        ld.arg(format!("--version-script={}", script.display()));
    }
    if !nostdlib {
        if has_compiler_rt(ctx) {
            ld.args(["-lcompiler_rt", "-lcompiler_rt"]);
        }
        ld.arg(libdir.join("crtendS.o"));
    }
    run(&mut ld).map_err(|e| format!("{dir}: shared link failed: {e}"))?;

    let libdir_mk = mk.var("LIBDIR")?;
    let libdir_mk = if libdir_mk.is_empty() {
        "/usr/lib".to_string()
    } else {
        libdir_mk
    };
    install_shared(
        ctx,
        &out,
        &format!("{}/{name}", libdir_mk.trim_end_matches('/')),
    )?;
    let size = fs::metadata(&out).map(|m| m.len()).unwrap_or(0);
    println!(
        "  {dir}: {name}, {} objects ({ran} rebuilt), {size} bytes",
        jobs.len()
    );
    Ok(())
}

/// Installs `from` at the absolute `path` into the sysroot and the staging root, root:bin
/// 444 (`${LIBOWN}:${LIBGRP}`, `${LIBMODE}`, which ld.so's `BINMODE=444` also is).
fn install_shared(ctx: &Ctx<'_>, from: &Path, path: &str) -> Result<()> {
    let rel = path.trim_start_matches('/');
    install_file(ctx, from, &ctx.sysroot.join(rel))?;
    install_file(ctx, from, &ctx.out.join("root").join(rel))?;
    ctx.owners
        .lock()
        .map_err(|_| "lock poisoned")?
        .push(ramdisk::Attr::installed(path, "root", "bin", "444")?);
    Ok(())
}

/// The dynamic relocation types `llvm-objdump --dynamic-reloc` lists.
fn dynamic_reloc_types(text: &str) -> Vec<String> {
    text.lines()
        .skip_while(|l| !l.starts_with("OFFSET"))
        .skip(1)
        .filter_map(|l| l.split_whitespace().nth(1).map(str::to_string))
        .collect()
}

/// `/usr/libexec/ld.so`, built, checked and installed.
fn build_ldso(ctx: &Ctx<'_>) -> Result<()> {
    let objdir = ctx.out.join("obj").join(LDSO_DIR);
    fs::create_dir_all(&objdir).map_err(|e| format!("{}: {e}", objdir.display()))?;
    // `STACK_PROTECTOR=`, as the Makefile.inc of the architectures without retguard says:
    // see the module's deviations.
    let mut mk = make_for_with(
        ctx,
        LDSO_DIR,
        &objdir,
        false,
        &[("STACK_PROTECTOR", String::new())],
    )?;
    let prog = mk.var("PROG")?;
    if prog.is_empty() {
        return Err(format!("{LDSO_DIR}/Makefile: no PROG (NOPIC?)").into());
    }
    // `VPATH`: libc's string functions ld.so compiles under its own names.
    for d in mk.var("VPATH")?.split([':', ' ']).filter(|d| !d.is_empty()) {
        mk.add_path(Path::new(d));
    }
    let jobs = object_jobs(ctx, &mk, &objdir, &mk.words("OBJS")?)?;
    let ran = run_jobs(ctx, LDSO_DIR, &jobs)?;

    // `$(LD) -e _dl_start $(ELF_LDFLAGS) -o $(candidate) ${OBJS}`.
    let candidate = objdir.join(format!("{prog}.test"));
    let mut ld = Command::new(&ctx.tools.ld);
    ld.args(OPENBSD_LLD_DEFAULTS)
        .args(["-e", "_dl_start"])
        .args(mk.words("ELF_LDFLAGS")?)
        .arg("-o")
        .arg(&candidate);
    for j in &jobs {
        ld.arg(&j.target);
    }
    run(&mut ld).map_err(|e| format!("{LDSO_DIR}: link failed: {e}"))?;

    // `CHECK_LDSO`: nothing but `RELATIVE_RELOC` relocations.
    let relative = mk.var("RELATIVE_RELOC")?;
    if !relative.is_empty() {
        let out = Command::new(&ctx.tools.objdump)
            .arg("--dynamic-reloc")
            .arg(&candidate)
            .output()
            .map_err(|e| format!("llvm-objdump: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "{LDSO_DIR}: llvm-objdump --dynamic-reloc: {}",
                String::from_utf8_lossy(&out.stderr)
            )
            .into());
        }
        let allowed: Vec<&str> = relative.split('|').collect();
        let types = dynamic_reloc_types(&String::from_utf8_lossy(&out.stdout));
        if let Some(bad) = types.iter().find(|t| !allowed.contains(&t.as_str())) {
            return Err(format!(
                "{LDSO_DIR}: CHECK_LDSO: relocation {bad} (only {relative} allowed)"
            )
            .into());
        }
        println!(
            "  {LDSO_DIR}: CHECK_LDSO: {} dynamic relocations, all {relative}",
            types.len()
        );
    }
    let exe = objdir.join(&prog);
    fs::copy(&candidate, &exe).map_err(|e| format!("{}: {e}", exe.display()))?;
    install_shared(ctx, &exe, &format!("/usr/libexec/{prog}"))?;
    let size = fs::metadata(&exe).map(|m| m.len()).unwrap_or(0);
    println!(
        "  {LDSO_DIR}: {prog}, {} objects ({ran} rebuilt), {size} bytes",
        jobs.len()
    );
    Ok(())
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shlib_version_reads_major_and_minor() {
        let rthread = "# note: If changes were made\n# were added\nmajor=28\nminor=1\n";
        assert_eq!(
            shlib_version(rthread),
            Some(("28".to_string(), "1".to_string()))
        );
        assert_eq!(shlib_version("major=104\n"), None);
    }

    #[test]
    fn ldadd_words_become_linker_arguments() -> Result<()> {
        let words: Vec<String> = ["-nostdlib", "-lcompiler_rt", "-Wl,-zinitfirst,-znow"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let (args, nostdlib) = ldadd_args(&words)?;
        assert!(nostdlib);
        assert_eq!(args, ["-lcompiler_rt", "-zinitfirst", "-znow"]);
        assert!(ldadd_args(&["-pthread".to_string()]).is_err());
        Ok(())
    }

    #[test]
    fn dynamic_relocations_are_read_from_objdump() {
        let text = "\nld.so:\tfile format elf64-x86-64\n\nDYNAMIC RELOCATION RECORDS\n\
                    OFFSET           TYPE                     VALUE\n\
                    0000000000004e10 R_X86_64_RELATIVE        *ABS*+0x3a20\n";
        assert_eq!(dynamic_reloc_types(text), ["R_X86_64_RELATIVE"]);
    }
}
/* </TESTS> */
