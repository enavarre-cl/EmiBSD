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
//! `cargo xtask comp --arch A [--jobs N]` (M14): OpenBSD's compiler, clang and lld from
//! `gnu/llvm` (LLVM 22, Apache-2.0 WITH LLVM-exception, compiled unmodified), built for
//! EmiBSD by OpenBSD's own build glue (`gnu/usr.bin/clang`, `gnu/lib/libcxx`,
//! `gnu/lib/libcxxabi`, `gnu/lib/libclang_rt`, `lib/librthread`), evaluated by `bsdmake.rs`
//! and cross-compiled with the toolchain `userland` uses. The user's decision of 2026-10-04:
//! the comp set is part of M14.
//!
//! What it does, all under `target/comp/`:
//!
//! 1. `<arch>/sysroot`: a copy of `userland`'s sysroot (`target/userland/<arch>/sysroot`,
//!    which `just userland` must have made: headers, `crt*.o`, `libc.a`, `libcompiler_rt.a`,
//!    ...), plus what the compiler's Makefiles install there before they build: libc++'s and
//!    libc++abi's headers (`/usr/include/c++/v1`, with `__config_site`, by their own
//!    `includes` rules) and the runtime libraries of `RUNTIME_LIBRARIES` (`libpthread.a`,
//!    `libc++abi.a` with the libunwind sources OpenBSD builds into it, `libc++.a`).
//! 2. `host/`: the build tools `gnu/usr.bin/clang/Makefile` builds and runs on the build
//!    machine (`HOST_TOOLS`: `llvm-min-tblgen`, `llvm-tblgen`, `clang-tblgen`, with the
//!    libraries they link), built for macOS from the same Makefiles, once for both
//!    architectures. Each target tree finds them where the generation rules look
//!    (`${.OBJDIR}/../../../llvm-tblgen/llvm-tblgen`): its `obj/gnu/usr.bin/clang/<tool>`
//!    is a symbolic link to the host's.
//! 3. `<arch>/obj`: the `SUBDIR` of `gnu/usr.bin/clang/Makefile`, for `MACHINE` = the arch
//!    (`Makefile.arch` picks `LLVM_ARCH`: X86 or AArch64, plus AMDGPU, as OpenBSD does):
//!    first every `include/*` directory's generation rules (tblgen runs, `llvm-config.h`,
//!    the `.def` files), then every library (`libLLVM.a`, one archive of all of LLVM, as
//!    `libLLVM/Makefile` includes every `libLLVM*/Makefile`; the `libclang*`, `liblld*`
//!    ones), compiled together so the CPUs stay busy, then the programs, linked and
//!    installed. Then `gnu/lib/libclang_rt` (`CLANG_RT`).
//! 4. `<arch>/root`: the staging root of the comp set: the programs at their `BINDIR` with
//!    their `LINKS` (`clang` as `cc`, `c++`, `clang++`, `clang-cpp`, `cpp`; `ld.lld` as
//!    `ld`; `ar` as `ranlib`, ...), clang's resource headers (`/usr/lib/clang/22/include`,
//!    by `include/clang/intrin`'s own `install` rule), `libclang_rt.*.a`, and, so that `cc`
//!    can compile and link there, the sysroot's `/usr/include` and `/usr/lib` (`crt*.o`,
//!    `lib*.a`, and the shared `lib*.so.M.m`) and `/usr/libexec/ld.so` (M14,
//!    `userland/shlib.rs`). `comp.ffs` is that root as an ffs image (makefs, as `ramdisk.rs` makes
//!    the ramdisk), the disk `just smoke-cc` mounts.
//! 5. `<arch>/licences.txt`: the licence report of `userland`, over everything compiled.
//!
//! ## Deviations
//!
//! - The programs are static PIE executables like the rest of the userland's: OpenBSD
//!   links them dynamically (and `libLLVM` is a shared
//!   library there, `NOLIBSTATIC`); here `libLLVM.a` is linked into each.
//! - The build tools run on macOS: built with Apple clang against macOS's libc++, from the
//!   same Makefiles and the same `include/llvm/Config` (OpenBSD's `config.h`), with the
//!   workarounds of `HOST_FLAGS`.
//! - macOS's file system ignores case: `fix_case_folding` turns `-I` into `-iquote` or
//!   hides a header (`CASE_COLLISIONS`) where a lookup would find another file than on
//!   OpenBSD.
//! - lldb is not built (`BUILD_LLDB=no` in `Comp::new`; bsd.own.mk says yes on amd64 and
//!   arm64): see `NOT_BUILT`.
//! - `/usr/include/llvm` (libLLVM's `includes`, LLVM's headers for its users) is not
//!   installed: nothing here builds against LLVM.
//! - Profiled libraries (`*_p.a`) are not built, as nowhere in `userland`.

use super::*;

/// OpenBSD's build glue for LLVM.
const CLANG_DIR: &str = "gnu/usr.bin/clang";

/// The `SUBDIR` entries that are build tools: built for this machine (`host/`), never for
/// the target. Their libraries are built for this machine as their `LLVM_LIBDEPS` say.
const HOST_TOOLS: &[&str] = &["llvm-min-tblgen", "llvm-tblgen", "clang-tblgen"];

/// `SUBDIR` libraries only the build tools link (the target's `libLLVM.a` holds its sources
/// too): built for this machine only. (`libLLVMTableGen` is built for both:
/// `clang-scan-deps` links it.)
const HOST_ONLY_LIBS: &[&str] = &["libLLVMSupport"];

/// Libraries the compiler and its programs link, built first, in this order: OpenBSD's
/// threads (`lib/librthread`, `libpthread.a`), the C++ ABI library with libunwind
/// (`libc++abi.a`) and libc++ (`libc++.a`). Their headers are installed by their own
/// `includes` rules first (`RUNTIME_INCLUDES`).
const RUNTIME_LIBRARIES: &[&str] = &["lib/librthread", "gnu/lib/libcxxabi", "gnu/lib/libcxx"];

/// The libraries whose `includes` rule installs `/usr/include/c++/v1`.
const RUNTIME_INCLUDES: &[&str] = &["gnu/lib/libcxxabi", "gnu/lib/libcxx"];

/// The compiler runtime the comp set ships under `/usr/lib/clang/<ver>/lib`.
const CLANG_RT: &[&str] = &[
    "gnu/lib/libclang_rt/profile",
    "gnu/lib/libclang_rt/ubsan_minimal",
];

/// What of OpenBSD's comp set (`distrib/sets/lists/comp/{mi,md.<arch>,clang.<arch>}`) and of
/// the glue's `SUBDIR` is not built here, and why.
const NOT_BUILT: &[(&str, &str)] = &[
    (
        "lldb, lldb-server (and lldb-tblgen, liblldb*)",
        "BUILD_LLDB=no: the debugger needs ptrace(2) and a host build of libLLVM for \
         lldb-tblgen; left for after M14's compiler (the exit criterion is `cc hello.c`)",
    ),
    (
        "/usr/include/llvm, /usr/include/llvm-c",
        "libLLVM's `includes` (LLVM's headers for programs built against it): nothing \
         here builds against LLVM",
    ),
    (
        "libc++_p.a, libc++abi_p.a, libpthread_p.a",
        "profiled libraries are built nowhere here (gprof is not ported)",
    ),
];

/// Flags added to (or removed from) the build tools' compiles on this machine, and why.
/// `@COMPAT@` stands for `HOST_COMPAT_H`'s file.
const HOST_FLAGS: &[(&str, &str)] = &[
    (
        "-DHAVE_MACH_MACH_H=1",
        "OpenBSD's config.h says ENABLE_CRASH_OVERRIDES, whose code on macOS \
         (Support/Unix/Signals.inc) needs <mach/mach.h>, which config.h leaves out",
    ),
    (
        "-include @COMPAT@",
        "OpenBSD's config.h says HAVE_PTHREAD_SET_NAME_NP and HAVE_PTHREAD_GET_NAME_NP \
         (Support/Unix/Threading.inc); macOS has pthread_setname_np/pthread_getname_np, \
         which HOST_COMPAT_H maps them to",
    ),
];

/// The header force-included into the build tools' compiles (`HOST_FLAGS`).
const HOST_COMPAT_H: &str = "\
/* EmiBSD: host shims for building LLVM's build tools on macOS (tools/xtask, comp.rs). */
#include <pthread.h>
static inline void pthread_set_name_np(pthread_t t, const char *name)
{
\t(void)t;\t/* macOS names the calling thread only; LLVM passes pthread_self() */
\tpthread_setname_np(name);
}
static inline void pthread_get_name_np(pthread_t t, char *buf, size_t len)
{
\tpthread_getname_np(t, buf, len);
}
";

/// Headers hidden from one `-I` directory because another `-I` directory of the same
/// compile holds a header whose name differs only in case (`fix_case_folding`): (directory
/// suffix, header, why).
const CASE_COLLISIONS: &[(&str, &str, &str)] = &[
    (
        "llvm/include/llvm/ExecutionEngine/JITLink",
        "x86.h",
        "libLLVM's sources include \"X86.h\" (lib/Target/X86) 51 times and a bare \"x86.h\" \
         never (JITLink's users name llvm/ExecutionEngine/JITLink/x86.h)",
    ),
    (
        "llvm/include/llvm/ExecutionEngine/JITLink",
        "aarch64.h",
        "libLLVM's sources include \"AArch64.h\" (lib/Target/AArch64) 29 times and a bare \
         \"aarch64.h\" never (JITLink's users name llvm/ExecutionEngine/JITLink/aarch64.h)",
    ),
];

/// One architecture's build.
struct Comp<'a> {
    ctx: Ctx<'a>,
    /// `bsd.own.mk`/`sys.mk` values the compiler's Makefiles read (`comp_vars`).
    vars: Vec<(&'static str, String)>,
    /// Owners and modes of the staging root's files (applied in `comp.ffs`).
    attrs: Mutex<Vec<ramdisk::Attr>>,
    /// Whether this is the build tools' tree (`host/`), built for this machine.
    host: bool,
    /// The names of libc's and libc++'s top-level headers (`include/`,
    /// `gnu/llvm/libcxx/include`), lower case.
    system_headers: BTreeSet<String>,
    /// The `-I` directories already reported by `quote_folding_includes`.
    folded: Mutex<BTreeSet<String>>,
}

/// `cargo xtask comp --arch A [--jobs N]`.
pub(crate) fn comp(root: &Path, arch: Arch, jobs: usize) -> Result<()> {
    JOBS.store(jobs, Ordering::Relaxed);
    let tools = Tools::locate()?;
    let src = fs::canonicalize(openbsd_src(root)?)?;
    if !src.join(CLANG_DIR).join("Makefile").is_file() {
        return Err(format!(
            "{}: not in the reference clone (gnu/llvm and gnu/usr.bin/clang are needed)",
            src.join(CLANG_DIR).display()
        )
        .into());
    }
    let user_sysroot = root
        .join("target/userland")
        .join(arch.name())
        .join("sysroot");
    if !user_sysroot.join("usr/lib/libc.a").is_file() {
        return Err(format!(
            "{}: no libc.a; run `just userland` first",
            user_sysroot.display()
        )
        .into());
    }
    let started = std::time::Instant::now();
    let base = root.join("target/comp");
    println!(
        "comp {}: OpenBSD sources {}, output {}, {jobs} jobs",
        arch.name(),
        src.display(),
        base.join(arch.name()).display()
    );
    println!("  cc {}: {}", tools.cc.display(), tool_version(&tools.cc)?);

    // 2. The build tools, for this machine (shared by both architectures).
    let host = Comp::new(root, &src, &tools, &base.join("host"), host_arch(), true)?;
    let host_tools = host.build_host_tools()?;

    // 1. The sysroot and the runtime libraries.
    let out = base.join(arch.name());
    let c = Comp::new(root, &src, &tools, &out, arch, false)?;
    c.mirror_sysroot(&user_sysroot)?;
    for dir in RUNTIME_INCLUDES {
        let n = c.install_rule(dir, "includes")?;
        println!("  {dir}: includes ({n} headers)");
    }
    let runtime: Vec<String> = RUNTIME_LIBRARIES.iter().map(|d| d.to_string()).collect();
    for lib in c.build_libraries(&runtime)? {
        let name = lib.file_name().ok_or("bad library name")?;
        install_file(&c.ctx, &lib, &c.ctx.sysroot.join("usr/lib").join(name))?;
    }

    // 3. The compiler.
    c.link_host_tools(&host_tools)?;
    let subdir = c.subdirs(CLANG_DIR)?;
    let (mut includes, mut libs, mut progs) = (Vec::new(), Vec::new(), Vec::new());
    for d in &subdir {
        let name = d.rsplit('/').next().unwrap_or(d);
        if HOST_TOOLS.contains(&name) || HOST_ONLY_LIBS.contains(&name) {
            continue;
        }
        if d.contains("/include/") {
            includes.push(d.clone());
        } else if name.starts_with("lib") {
            libs.push(d.clone());
        } else {
            progs.push(d.clone());
        }
    }
    for d in &includes {
        c.generate(d)?;
    }
    println!(
        "  {CLANG_DIR}/include: {} directories generated",
        includes.len()
    );
    c.build_libraries(&libs)?;
    c.build_programs(&progs)?;
    let rt = c.build_libraries(&CLANG_RT.iter().map(|d| d.to_string()).collect::<Vec<_>>())?;

    // 4. The staging root and its image.
    c.stage(&rt)?;
    for (what, why) in NOT_BUILT {
        println!("  not built: {what}: {why}");
    }
    licence_report(&c.ctx)?;
    c.image()?;
    println!(
        "comp {}: done in {:.0} s",
        arch.name(),
        started.elapsed().as_secs_f64()
    );
    Ok(())
}

/// The architecture of this machine, whose `MACHINE` the build tools' Makefiles are
/// evaluated with (only `include/llvm/Config`'s triple depends on it, which tblgen ignores).
fn host_arch() -> Arch {
    if cfg!(target_arch = "x86_64") {
        Arch::Amd64
    } else {
        Arch::Arm64
    }
}

/// `OSMAJOR` and `OSMINOR` as `share/mk/sys.mk` sets them (the default triple's
/// `openbsd8.0`).
fn os_release(src: &Path) -> Result<(String, String)> {
    let sys_mk = src.join("share/mk/sys.mk");
    let text = fs::read_to_string(&sys_mk).map_err(|e| format!("{}: {e}", sys_mk.display()))?;
    let get = |name: &str| {
        text.lines()
            .filter_map(|l| l.strip_prefix(name))
            .find_map(|rest| rest.trim_start().strip_prefix('='))
            .map(|v| v.trim().to_string())
            .ok_or_else(|| format!("{}: no {name}", sys_mk.display()))
    };
    Ok((get("OSMAJOR")?, get("OSMINOR")?))
}

impl<'a> Comp<'a> {
    fn new(
        root: &Path,
        src: &Path,
        tools: &'a Tools,
        out: &Path,
        arch: Arch,
        host: bool,
    ) -> Result<Self> {
        if out.to_string_lossy().contains(char::is_whitespace) {
            return Err(
                format!("{}: paths with whitespace are not supported", out.display()).into(),
            );
        }
        let (major, minor) = os_release(src)?;
        let vars = vec![
            // share/mk/sys.mk
            ("OSMAJOR", major.clone()),
            ("OSMINOR", minor.clone()),
            ("OSREV", format!("{major}.{minor}")),
            // share/mk/bsd.own.mk, for amd64 and arm64 (NOT_BUILT says why not lldb).
            ("BUILD_LLDB", "no".to_string()),
            ("AR_VERSION", "llvm".to_string()),
            ("LINKER_VERSION", "lld".to_string()),
            ("BSDSRCDIR", src.display().to_string()),
            ("BSDOBJDIR", out.join("obj").display().to_string()),
            ("BINOWN", "root".to_string()),
            ("BINGRP", "bin".to_string()),
            ("BINMODE", "555".to_string()),
            ("NONBINMODE", "444".to_string()),
            ("DIRMODE", "755".to_string()),
            ("BINDIR", "/usr/bin".to_string()),
            ("LIBDIR", "/usr/lib".to_string()),
            ("INSTALL", "install".to_string()),
            ("INSTALL_COPY", "-C".to_string()),
        ];
        Ok(Comp {
            ctx: Ctx {
                root: root.to_path_buf(),
                src: src.to_path_buf(),
                sysroot: out.join("sysroot"),
                m: Machine::of(arch),
                tools,
                installed: Mutex::new(HashMap::new()),
                lower: Mutex::new(HashMap::new()),
                inputs: Mutex::new(BTreeSet::new()),
                owners: Mutex::new(Vec::new()),
                yacc: Mutex::new(None),
                lex: Mutex::new(None),
                out: out.to_path_buf(),
            },
            vars,
            attrs: Mutex::new(Vec::new()),
            host,
            system_headers: system_headers(src)?,
            folded: Mutex::new(BTreeSet::new()),
        })
    }

    fn is_host(&self) -> bool {
        self.host
    }

    fn objdir(&self, dir: &str) -> Result<PathBuf> {
        let objdir = self.ctx.out.join("obj").join(dir);
        fs::create_dir_all(&objdir).map_err(|e| format!("{}: {e}", objdir.display()))?;
        Ok(objdir)
    }

    /// The evaluated Makefile of `dir`, with `comp_vars` and, for the build tools, the
    /// `HOST_FLAGS`.
    fn make(&self, dir: &str) -> Result<(Make, PathBuf)> {
        let objdir = self.objdir(dir)?;
        let mut mk = make_for_with(&self.ctx, dir, &objdir, self.is_host(), &self.vars)?;
        self.fix_case_folding(&mut mk, dir)?;
        // A Makefile that sets CC or CXX itself (Makefile.inc does, unless COMPILER_VERSION
        // is clang) would compile for this machine: never silently.
        if !self.is_host() {
            for var in ["CC", "CXX"] {
                let v = mk.var(var)?;
                if !v.contains("--target=") {
                    return Err(format!("{dir}: {var} is `{v}`, not the cross compiler").into());
                }
            }
        }
        if self.is_host() {
            let compat = self.ctx.out.join("emibsd-host-compat.h");
            ramdisk::write_if_changed(&compat, HOST_COMPAT_H)?;
            let first = self
                .folded
                .lock()
                .map(|mut f| f.insert("HOST_FLAGS".to_string()))
                .unwrap_or(false);
            for (flag, why) in HOST_FLAGS {
                let flag = flag.replace("@COMPAT@", &compat.display().to_string());
                let v = mk.var("CPPFLAGS")?.replace('$', "$$");
                mk.set("CPPFLAGS", &format!("{v} {flag}"));
                if first {
                    println!("  {dir} (and every build tool): added {flag}: {why}");
                }
            }
        }
        Ok((mk, objdir))
    }

    /// Keeps `#include` lookups what they are on OpenBSD's case-sensitive file systems,
    /// which macOS's is not:
    ///
    /// - `-I<dir>` becomes `-iquote <dir>` when `<dir>` holds a header whose name differs
    ///   from a C or C++ library header only in case (`llvm/Support/Errno.h`, `Regex.h`,
    ///   `Endian.h`, `Memory.h`, `Locale.h`): `#include <errno.h>` would find it. `-iquote`
    ///   keeps every `#include "..."` lookup the `-I` gave.
    /// - When two `-I` directories of one compile hold headers differing only in case, the
    ///   one `CASE_COLLISIONS` names is hidden: its directory is replaced by a shadow of
    ///   symbolic links to everything else in it (`obj/emibsd-case/`). A collision the table
    ///   does not name is an error, never a guess.
    fn fix_case_folding(&self, mk: &mut Make, dir: &str) -> Result<()> {
        // Every -I directory of the compile, in order, for the collisions between them.
        let mut all_dirs: Vec<String> = Vec::new();
        for var in ["CPPFLAGS", "CFLAGS", "CXXFLAGS"] {
            all_dirs.extend(include_dirs(&mk.words(var)?).into_iter().map(|(_, d)| d));
        }
        let hidden = self.collisions(&all_dirs, dir)?;
        for var in ["CPPFLAGS", "CFLAGS", "CXXFLAGS"] {
            let words = mk.words(var)?;
            let mut out = Vec::with_capacity(words.len());
            let mut changed = false;
            let mut i = 0;
            for (at, d) in include_dirs(&words) {
                out.extend(words[i..at].iter().cloned());
                i = at + if words[at] == "-I" { 2 } else { 1 };
                if let Some(names) = hidden.get(&d) {
                    changed = true;
                    out.push(format!("-I{}", self.shadow(&d, names)?.display()));
                    continue;
                }
                match self.folding_header(Path::new(&d)) {
                    Some(h) => {
                        changed = true;
                        out.push(format!("-iquote {d}"));
                        if self.first_report(&d) {
                            println!(
                                "  {dir}: -I{d} made -iquote: its {h} would be found for <{}> \
                                 on this case-insensitive file system",
                                h.to_lowercase()
                            );
                        }
                    }
                    None => out.extend(words[at..i].iter().cloned()),
                }
            }
            out.extend(words[i..].iter().cloned());
            if changed {
                mk.set(var, &out.join(" ").replace('$', "$$"));
            }
        }
        Ok(())
    }

    /// The headers to hide, per directory, for the case collisions among `dirs`.
    fn collisions(&self, dirs: &[String], dir: &str) -> Result<HashMap<String, Vec<String>>> {
        let mut by_lower: HashMap<String, Vec<(String, String)>> = HashMap::new();
        let mut seen = BTreeSet::new();
        for d in dirs {
            let canon = fs::canonicalize(d).unwrap_or_else(|_| PathBuf::from(d));
            if !seen.insert(canon) {
                continue;
            }
            let Ok(entries) = fs::read_dir(d) else {
                continue;
            };
            for e in entries.flatten() {
                let Ok(name) = e.file_name().into_string() else {
                    continue;
                };
                if name.ends_with(".h") || name.ends_with(".inc") || name.ends_with(".def") {
                    by_lower
                        .entry(name.to_lowercase())
                        .or_default()
                        .push((name, d.clone()));
                }
            }
        }
        let mut hidden: HashMap<String, Vec<String>> = HashMap::new();
        for files in by_lower.values() {
            let names: BTreeSet<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
            if names.len() < 2 {
                continue;
            }
            let mut resolved = false;
            for (name, d) in files {
                if let Some((_, _, why)) = CASE_COLLISIONS
                    .iter()
                    .find(|(suffix, file, _)| *file == name && d.ends_with(suffix))
                {
                    hidden.entry(d.clone()).or_default().push(name.clone());
                    resolved = true;
                    if self.first_report(&format!("{d}/{name}")) {
                        println!("  {dir}: {name} hidden from -I{d}: {why}");
                    }
                }
            }
            if !resolved {
                return Err(format!(
                    "{dir}: headers differing only in case in two -I directories, which this \
                     case-insensitive file system cannot tell apart: {files:?}; name the one to \
                     hide in CASE_COLLISIONS (comp.rs)"
                )
                .into());
            }
        }
        Ok(hidden)
    }

    /// A directory of symbolic links to every entry of `dir` but `hide`.
    fn shadow(&self, dir: &str, hide: &[String]) -> Result<PathBuf> {
        let name: String = dir
            .trim_start_matches('/')
            .chars()
            .map(|c| if c == '/' { '_' } else { c })
            .collect();
        let shadow = self.ctx.out.join("obj/emibsd-case").join(name);
        fs::create_dir_all(&shadow).map_err(|e| format!("{}: {e}", shadow.display()))?;
        for e in fs::read_dir(dir).map_err(|e| format!("{dir}: {e}"))? {
            let e = e?;
            let n = e.file_name();
            if hide.iter().any(|h| n.to_str() == Some(h.as_str())) {
                continue;
            }
            symlink(&e.path().display().to_string(), &shadow.join(&n))?;
        }
        Ok(shadow)
    }

    /// Whether `key` is reported for the first time in this run.
    fn first_report(&self, key: &str) -> bool {
        self.folded
            .lock()
            .map(|mut f| f.insert(key.to_string()))
            .unwrap_or(false)
    }

    /// A header of `dir` whose name folds to a system header's but is not it.
    fn folding_header(&self, dir: &Path) -> Option<String> {
        let entries = fs::read_dir(dir).ok()?;
        entries
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .find(|n| {
                let lower = n.to_lowercase();
                n.ends_with(".h") && *n != lower && self.system_headers.contains(&lower)
            })
    }

    /// `SUBDIR` of the Makefile in `dir`, recursively expanded into the leaf directories
    /// (relative to the sources), in order.
    fn subdirs(&self, dir: &str) -> Result<Vec<String>> {
        let (mk, _) = self.make(dir)?;
        let mut out = Vec::new();
        for s in mk.words("SUBDIR")? {
            let sub = format!("{dir}/{s}");
            let (inner, _) = self.make(&sub)?;
            let leaf = !inner.var("PROG")?.is_empty() || !inner.var("LIB")?.is_empty();
            if !leaf && !inner.words("SUBDIR")?.is_empty() {
                out.extend(self.subdirs(&sub)?);
            } else {
                out.push(sub);
            }
        }
        Ok(out)
    }

    // --- sysroot ------------------------------------------------------------------------

    /// Copies `userland`'s sysroot into this one (unchanged files keep their mtime).
    fn mirror_sysroot(&self, from: &Path) -> Result<()> {
        let mut n = 0usize;
        mirror(from, &self.ctx.sysroot, &mut n)?;
        println!(
            "  sysroot: {} ({n} files updated from {})",
            self.ctx.sysroot.display(),
            from.display()
        );
        Ok(())
    }

    /// Runs `target` of `dir`'s Makefile with the `install(1)` recorder of
    /// `libraries.rs` (after making its sources), then installs what it recorded; the rule
    /// itself runs again only when the Makefile is newer than its record. Returns how many
    /// files it installs.
    fn install_rule(&self, dir: &str, target: &str) -> Result<usize> {
        let (mk, objdir) = self.make(dir)?;
        let shims = objdir.join("emibsd-install");
        fs::create_dir_all(&shims).map_err(|e| format!("{}: {e}", shims.display()))?;
        let manifest = shims.join(format!("{target}.manifest"));
        let mut made = BTreeSet::new();
        for s in mk.sources_of(target) {
            if mk.rule_for(&s).is_some() {
                libraries::make_target(&self.ctx, &mk, &objdir, &s, None, &mut made)?;
            }
        }
        let makefile = self.ctx.src.join(dir).join("Makefile");
        let fresh = matches!((mtime(&manifest), mtime(&makefile)), (Some(m), Some(f)) if m >= f);
        if !fresh {
            let partial = shims.join("manifest.partial");
            let _ = fs::remove_file(&partial);
            for (name, text) in [
                (
                    "install",
                    libraries::INSTALL_SH.replace("@MANIFEST@", &partial.display().to_string()),
                ),
                ("cmp", libraries::CMP_SH.to_string()),
            ] {
                let p = shims.join(name);
                ramdisk::write_if_changed(&p, &text)?;
                use std::os::unix::fs::PermissionsExt as _;
                fs::set_permissions(&p, fs::Permissions::from_mode(0o755))
                    .map_err(|e| format!("{}: {e}", p.display()))?;
            }
            let rule = mk
                .rule_for(target)
                .ok_or_else(|| format!("{dir}/Makefile: no `{target}` rule"))?;
            let sources = mk
                .sources_of(target)
                .iter()
                .map(|s| {
                    mk.search(s)
                        .or_else(|| Some(objdir.join(s)).filter(|p| p.exists()))
                        .ok_or_else(|| format!("{dir}: no source {s} for `{target}`"))
                })
                .collect::<std::result::Result<Vec<_>, _>>()?;
            let mut job = Job::from_rule(&mk, &rule.commands, target, sources, &objdir)?;
            job.path = Some(shims.clone());
            job.run()
                .map_err(|e| format!("{dir}: `{target}` failed: {e}"))?;
            let _ = fs::remove_file(job.stamp_path());
            let _ = fs::remove_file(&job.target);
            fs::rename(&partial, &manifest)
                .map_err(|e| format!("{dir}: `{target}` installed nothing: {e}"))?;
        }
        let text =
            fs::read_to_string(&manifest).map_err(|e| format!("{}: {e}", manifest.display()))?;
        let mut n = 0;
        for line in text.lines() {
            let (from, dest) = line
                .split_once(' ')
                .ok_or_else(|| format!("{}: bad line `{line}`", manifest.display()))?;
            let (from, mut dest) = (PathBuf::from(from), PathBuf::from(dest));
            if !dest.starts_with(&self.ctx.sysroot) {
                return Err(
                    format!("{dir}: `{target}` installs outside the sysroot: {line}").into(),
                );
            }
            if from == Path::new("-d") {
                if dest.is_file() {
                    fs::remove_file(&dest).map_err(|e| format!("{}: {e}", dest.display()))?;
                }
                fs::create_dir_all(&dest).map_err(|e| format!("{}: {e}", dest.display()))?;
                continue;
            }
            if dest.is_dir() {
                dest = dest.join(from.file_name().ok_or("bad file name")?);
            }
            install_file(&self.ctx, &from, &dest)?;
            n += 1;
        }
        Ok(n)
    }

    // --- build tools --------------------------------------------------------------------

    /// Builds `HOST_TOOLS` for this machine; returns their object directories.
    fn build_host_tools(&self) -> Result<Vec<(String, PathBuf)>> {
        self.generate(&format!("{CLANG_DIR}/include/llvm/Config"))?;
        let mut out = Vec::new();
        for tool in HOST_TOOLS {
            let dir = format!("{CLANG_DIR}/{tool}");
            let (mk, _) = self.make(&dir)?;
            let deps: Vec<String> = mk
                .words("LLVM_LIBDEPS")?
                .iter()
                .map(|l| format!("{CLANG_DIR}/lib{l}"))
                .collect();
            self.build_libraries(&deps)?;
            self.build_programs(std::slice::from_ref(&dir))?;
            out.push((tool.to_string(), self.objdir(&dir)?));
        }
        Ok(out)
    }

    /// Points this tree's build-tool directories at the host's (module docs, step 2).
    fn link_host_tools(&self, host_tools: &[(String, PathBuf)]) -> Result<()> {
        for (tool, objdir) in host_tools {
            let here = self.ctx.out.join("obj").join(CLANG_DIR).join(tool);
            if fs::symlink_metadata(&here).is_ok_and(|m| m.is_dir()) {
                fs::remove_dir_all(&here).map_err(|e| format!("{}: {e}", here.display()))?;
            }
            if let Some(parent) = here.parent() {
                fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
            }
            symlink(&objdir.display().to_string(), &here)?;
        }
        Ok(())
    }

    // --- generated headers --------------------------------------------------------------

    /// Makes `all` of an `include/*` directory (tblgen runs and the like).
    fn generate(&self, dir: &str) -> Result<()> {
        let (mk, objdir) = self.make(dir)?;
        let mut made = BTreeSet::new();
        let targets = mk.sources_of("all");
        let mut jobs = Vec::new();
        for t in &targets {
            // Sources the rules make first (none today besides `all`'s own), then the
            // generated files themselves, in parallel.
            let Some(rule) = mk.rule_for(t) else {
                // A file of the directory itself (`abi-breaking.h`).
                if mk.search(t).is_none() {
                    return Err(format!("{dir}: no rule to make {t}").into());
                }
                continue;
            };
            let mut sources = Vec::new();
            for s in mk.sources_of(t) {
                if mk.rule_for(&s).is_some() {
                    sources.push(libraries::make_target(
                        &self.ctx, &mk, &objdir, &s, None, &mut made,
                    )?);
                } else {
                    sources.push(
                        mk.search(&s)
                            .ok_or_else(|| format!("{dir}: no source {s} for {t}"))?,
                    );
                }
            }
            jobs.push(Job::from_rule(&mk, &rule.commands, t, sources, &objdir)?);
        }
        let ran = run_jobs(&self.ctx, dir, &jobs)?;
        if ran > 0 {
            println!("  {dir}: {} files ({ran} made)", jobs.len());
        }
        Ok(())
    }

    // --- libraries ----------------------------------------------------------------------

    /// Builds the libraries of `dirs` (relative to the sources): every object of all of
    /// them in one parallel run, then one archive each (made again only when an object is
    /// newer, so that nothing relinks when nothing changed). Returns the archives.
    fn build_libraries(&self, dirs: &[String]) -> Result<Vec<PathBuf>> {
        let mut all_jobs = Vec::new();
        let mut libs = Vec::new();
        for dir in dirs {
            let (mk, objdir) = self.make(dir)?;
            let lib = mk.var("LIB")?;
            if lib.is_empty() {
                return Err(format!("{dir}/Makefile: no LIB").into());
            }
            let mut made = BTreeSet::new();
            for t in mk.words("BUILDFIRST")? {
                libraries::make_target(&self.ctx, &mk, &objdir, &t, None, &mut made)?;
            }
            let jobs = self.object_jobs(&mk, &objdir, &mut made)?;
            let first = all_jobs.len();
            all_jobs.extend(jobs);
            libs.push((dir.clone(), lib, objdir, first..all_jobs.len()));
        }
        let what = if dirs.len() == 1 {
            dirs[0].clone()
        } else {
            format!("{} libraries", dirs.len())
        };
        let ran = run_jobs(&self.ctx, &what, &all_jobs)?;
        let mut out = Vec::new();
        for (dir, lib, objdir, range) in libs {
            let objs: Vec<&Job> = all_jobs[range].iter().collect();
            let archive = objdir.join(format!("lib{lib}.a"));
            if self.archive(&archive, &objdir, &objs)? {
                let size = fs::metadata(&archive).map(|m| m.len()).unwrap_or(0);
                println!("  {dir}: lib{lib}.a, {} objects, {size} bytes", objs.len());
            }
            out.push(archive);
        }
        if ran > 0 {
            println!("  ({ran} objects compiled)");
        }
        Ok(out)
    }

    /// `object_jobs`, after making what the Makefile says an object depends on
    /// (`llvm-config.o: BuildVariables.inc`, `ClangScanDeps.o: Opts.inc`).
    fn object_jobs(
        &self,
        mk: &Make,
        objdir: &Path,
        made: &mut BTreeSet<String>,
    ) -> Result<Vec<Job>> {
        let jobs = object_jobs(&self.ctx, mk, objdir, &[])?;
        for j in &jobs {
            let Some(name) = j.target.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            for s in mk.sources_of(name) {
                if mk.rule_for(&s).is_some() {
                    libraries::make_target(&self.ctx, mk, objdir, &s, None, made)?;
                }
            }
        }
        Ok(jobs)
    }

    /// `ar cq` + `ranlib`, unless the archive is newer than its objects and has the same
    /// members (`<archive>.members`). Returns whether it made the archive.
    fn archive(&self, archive: &Path, objdir: &Path, objs: &[&Job]) -> Result<bool> {
        let members: Vec<PathBuf> = objs
            .iter()
            .map(|j| {
                j.target
                    .strip_prefix(objdir)
                    .unwrap_or(&j.target)
                    .to_path_buf()
            })
            .collect();
        let list: String = members
            .iter()
            .map(|m| format!("{}\n", m.display()))
            .collect();
        let stamp = PathBuf::from(format!("{}.members", archive.display()));
        if let Some(t) = mtime(archive)
            && fs::read_to_string(&stamp).ok().as_deref() == Some(list.as_str())
            && objs
                .iter()
                .all(|j| mtime(&j.target).is_some_and(|m| m <= t))
        {
            return Ok(false);
        }
        let _ = fs::remove_file(archive);
        // One `ar` per thousand members keeps the command line short.
        for chunk in members.chunks(1000) {
            let mut ar = Command::new(&self.ctx.tools.ar);
            ar.arg("cq");
            if self.is_host() {
                ar.arg("--format=darwin");
            }
            ar.arg(archive).current_dir(objdir).args(chunk);
            run(&mut ar)?;
        }
        run(Command::new(&self.ctx.tools.ranlib).arg(archive))?;
        fs::write(&stamp, list).map_err(|e| format!("{}: {e}", stamp.display()))?;
        Ok(true)
    }

    // --- programs -----------------------------------------------------------------------

    /// Builds the programs of `dirs`: all objects in one parallel run, then the links in
    /// another, then (target only) the installation into the staging root.
    fn build_programs(&self, dirs: &[String]) -> Result<()> {
        let mut all_jobs = Vec::new();
        let mut progs = Vec::new();
        for dir in dirs {
            let (mut mk, objdir) = self.make(dir)?;
            let prog = mk.var("PROG")?;
            if prog.is_empty() {
                return Err(format!("{dir}/Makefile: no PROG").into());
            }
            if !mk.defined("SRCS") {
                mk.set("SRCS", &format!("{prog}.cpp"));
            }
            let mut made = BTreeSet::new();
            // What `all` makes besides the program (llvm-tblgen's `GenVT.inc`).
            for s in mk.sources_of("all") {
                if mk.rule_for(&s).is_some() {
                    libraries::make_target(&self.ctx, &mk, &objdir, &s, None, &mut made)?;
                }
            }
            let jobs = self.object_jobs(&mk, &objdir, &mut made)?;
            let first = all_jobs.len();
            all_jobs.extend(jobs);
            progs.push((dir.clone(), mk, prog, objdir, first..all_jobs.len()));
        }
        let what = if dirs.len() == 1 {
            dirs[0].clone()
        } else {
            format!("{} programs", dirs.len())
        };
        run_jobs(&self.ctx, &what, &all_jobs)?;
        let mut links = Vec::new();
        for (dir, mk, prog, objdir, range) in &progs {
            let objs: Vec<PathBuf> = all_jobs[range.clone()]
                .iter()
                .map(|j| j.target.clone())
                .collect();
            links.push(self.link_job(dir, mk, prog, objdir, objs)?);
        }
        let ran = run_jobs(&self.ctx, &format!("{what} (link)"), &links)?;
        if ran > 0 {
            println!("  ({ran} programs linked)");
        }
        if self.is_host() {
            return Ok(());
        }
        for ((dir, mk, prog, _, _), link) in progs.iter().zip(&links) {
            self.install_prog(dir, mk, prog, &link.target)?;
        }
        Ok(())
    }

    /// The link of one program: for this machine, `c++` with the host archives; for the
    /// target, `ld.lld` as OpenBSD's `c++ -static` would run it (static PIE, `rcrt0.o`,
    /// libc++, libc++abi, libpthread, libm, libcompiler_rt, libc).
    fn link_job(
        &self,
        dir: &str,
        mk: &Make,
        prog: &str,
        objdir: &Path,
        objs: Vec<PathBuf>,
    ) -> Result<Job> {
        let ldadd = mk.words("LDADD")?;
        let mut args: Vec<String> = Vec::new();
        let mut deps = objs.clone();
        let mut words = ldadd.iter();
        while let Some(w) = words.next() {
            if let Some(rest) = w.strip_prefix("-Wl,") {
                if self.is_host() {
                    // macOS's ld resolves archives in any order (no groups).
                    continue;
                }
                args.extend(rest.split(',').map(str::to_string));
            } else if w == "-L" {
                let d = words
                    .next()
                    .ok_or_else(|| format!("{dir}: LDADD ends with -L"))?;
                args.push(format!("-L{d}"));
            } else if w.starts_with("-L") || w.starts_with("-l") {
                args.push(w.clone());
            } else if w.ends_with(".a") {
                deps.push(PathBuf::from(w));
                args.push(w.clone());
            } else {
                return Err(format!("{dir}: unsupported LDADD word `{w}`").into());
            }
        }
        let exe = objdir.join(prog);
        let objs_s: Vec<String> = objs.iter().map(|o| o.display().to_string()).collect();
        let cmd = if self.is_host() {
            format!(
                "{} -o {} {} {}",
                cxx_of(&self.ctx.tools.cc).display(),
                exe.display(),
                objs_s.join(" "),
                args.join(" ")
            )
        } else {
            let lib = self.ctx.sysroot.join("usr/lib");
            for l in [
                "libc++.a",
                "libc++abi.a",
                "libpthread.a",
                "libm.a",
                "libc.a",
            ] {
                deps.push(lib.join(l));
            }
            let rt = if has_compiler_rt(&self.ctx) {
                "-lcompiler_rt -lc -lcompiler_rt"
            } else {
                "-lc"
            };
            format!(
                "{} --sysroot={sysroot} -e __start --eh-frame-hdr -Bstatic -pie -o {exe} \
                 {lib}/rcrt0.o {lib}/crtbegin.o -L{lib} {objs} {args} \
                 -lc++ -lc++abi -lpthread -lm {rt} {lib}/crtend.o",
                self.ctx.tools.ld.display(),
                sysroot = self.ctx.sysroot.display(),
                exe = exe.display(),
                lib = lib.display(),
                objs = objs_s.join(" "),
                args = args.join(" "),
            )
        };
        Ok(Job {
            target: exe,
            cwd: objdir.to_path_buf(),
            commands: vec![(cmd, false)],
            deps,
            path: None,
        })
    }

    /// `install -s -o ${BINOWN} -g ${BINGRP} -m ${BINMODE}` into the staging root, and the
    /// `LINKS`.
    fn install_prog(&self, dir: &str, mk: &Make, prog: &str, exe: &Path) -> Result<()> {
        let rootdir = self.ctx.out.join("root");
        let bindir = mk.var("BINDIR")?;
        let installed = rootdir.join(bindir.trim_start_matches('/')).join(prog);
        if let Some(parent) = installed.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        if !matches!((mtime(&installed), mtime(exe)), (Some(i), Some(e)) if i >= e) {
            run(Command::new(&self.ctx.tools.objcopy)
                .arg("--strip-all")
                .arg(exe)
                .arg(&installed))?;
        }
        let mode = mk.var("BINMODE")?;
        let path = format!("/{}/{prog}", bindir.trim_matches('/'));
        let mut attrs = self.attrs.lock().map_err(|_| "lock poisoned")?;
        attrs.push(ramdisk::Attr::installed(&path, "root", "bin", &mode)?);
        let links = mk.words("LINKS")?;
        let mut names = Vec::new();
        for pair in links.chunks(2) {
            if let [from, to] = pair {
                let to_p = rootdir.join(to.trim_start_matches('/'));
                let from_p = rootdir.join(from.trim_start_matches('/'));
                use std::os::unix::fs::MetadataExt as _;
                let same = match (fs::metadata(&from_p), fs::symlink_metadata(&to_p)) {
                    (Ok(a), Ok(b)) => a.ino() == b.ino() && a.dev() == b.dev(),
                    _ => false,
                };
                if !same {
                    let _ = fs::remove_file(&to_p);
                    fs::hard_link(&from_p, &to_p)
                        .map_err(|e| format!("ln {from} {}: {e}", to_p.display()))?;
                }
                attrs.push(ramdisk::Attr::installed(to, "root", "bin", &mode)?);
                names.push(to.rsplit('/').next().unwrap_or(to).to_string());
            }
        }
        let size = fs::metadata(&installed).map(|m| m.len()).unwrap_or(0);
        println!(
            "  {dir}: {path} ({size} bytes stripped){}",
            if names.is_empty() {
                String::new()
            } else {
                format!(", links {}", names.join(" "))
            }
        );
        Ok(())
    }

    // --- staging root -------------------------------------------------------------------

    /// Fills the staging root (module docs, step 4) besides the programs.
    fn stage(&self, clang_rt: &[PathBuf]) -> Result<()> {
        let rootdir = self.ctx.out.join("root");
        // clang's resource headers, by include/clang/intrin's `install` rule (into the
        // sysroot, then copied with the rest of /usr/lib below).
        let n = self.install_rule(&format!("{CLANG_DIR}/include/clang/intrin"), "install")?;
        println!("  {CLANG_DIR}/include/clang/intrin: install ({n} headers)");
        let (mk, _) = self.make(CLANG_RT[0])?;
        let rtdir = mk.var("LIBDIR")?;
        for a in clang_rt {
            let name = a.file_name().ok_or("bad library name")?;
            let to = self
                .ctx
                .sysroot
                .join(rtdir.trim_start_matches('/'))
                .join(name);
            install_file(&self.ctx, a, &to)?;
        }
        let mut n = 0usize;
        for sub in ["usr/include", "usr/lib"] {
            mirror(&self.ctx.sysroot.join(sub), &rootdir.join(sub), &mut n)?;
        }
        // M14: the run-time link-editor `userland` installs (`userland/shlib.rs`), so that
        // what `cc` links dynamically runs on this disk (`just smoke-cc` chroots into it).
        let libexec = self.ctx.sysroot.join("usr/libexec");
        if libexec.is_dir() {
            mirror(&libexec, &rootdir.join("usr/libexec"), &mut n)?;
        }
        fs::create_dir_all(rootdir.join("tmp"))
            .map_err(|e| format!("{}: {e}", rootdir.display()))?;
        let mut attrs = self.attrs.lock().map_err(|_| "lock poisoned")?;
        attrs.push(ramdisk::Attr::installed("/tmp", "root", "wheel", "1777")?);
        // The shared libraries and ld.so: root:bin 444, as `userland` installs them.
        for (dir, prefix) in [("usr/lib", "lib"), ("usr/libexec", "ld.so")] {
            let Ok(entries) = fs::read_dir(rootdir.join(dir)) else {
                continue;
            };
            for e in entries {
                let name = e?.file_name().to_string_lossy().into_owned();
                if name.starts_with(prefix) && (name.contains(".so.") || name == "ld.so") {
                    attrs.push(ramdisk::Attr::installed(
                        &format!("/{dir}/{name}"),
                        "root",
                        "bin",
                        "444",
                    )?);
                }
            }
        }
        println!(
            "  root: {} ({n} files of /usr/include and /usr/lib updated)",
            rootdir.display()
        );
        Ok(())
    }

    /// `comp.ffs`: the staging root as an ffs image, by OpenBSD's makefs as built by
    /// `userland` (`ramdisk.rs`), with partition `a` holding the file system.
    fn image(&self) -> Result<()> {
        let user = self
            .ctx
            .root
            .join("target/userland")
            .join(self.ctx.m.machine);
        let makefs = user.join("host/bin/makefs");
        if !makefs.is_file() {
            return Err(format!("{}: missing; run `just userland`", makefs.display()).into());
        }
        let rootdir = self.ctx.out.join("root");
        let image = self.ctx.out.join("comp.ffs");
        let newest = newest_mtime(&rootdir)?;
        if mtime(&image).is_some_and(|i| newest.is_some_and(|n| n <= i)) {
            println!("  image: {} (up to date)", image.display());
            return Ok(());
        }
        let attrs = self.attrs.lock().map_err(|_| "lock poisoned")?.clone();
        let owners = self.ctx.out.join("owners.txt");
        ramdisk::write_if_changed(&owners, &ramdisk::owners_table(&attrs))?;
        const MIB: u64 = 1 << 20;
        let size = (ramdisk::tree_bytes(&rootdir)? * 5 / 4 + 64 * MIB).div_ceil(MIB) * MIB;
        let disktab = self.ctx.out.join("disktab");
        ramdisk::write_if_changed(&disktab, &ramdisk::disktab_entry(size / 512))?;
        let _ = fs::remove_file(&image);
        run(Command::new(&makefs)
            .args(["-t", "ffs", "-T", &ramdisk::TIMESTAMP.to_string()])
            .env("EMIBSD_DISKTAB", &disktab)
            .env("EMIBSD_OWNERS", &owners)
            .env("EMIBSD_STAGING", &rootdir)
            .args(["-o", "disklabel=rdroot,minfree=0"])
            .arg(&image)
            .arg(&rootdir))?;
        let size = fs::metadata(&image).map(|m| m.len()).unwrap_or(0);
        println!("  image: {} ({size} bytes)", image.display());
        Ok(())
    }
}

/// Copies the tree `from` into `to`: files whose contents differ (counted in `n`), and
/// symbolic links. Unchanged files keep their mtime, so nothing that depends on them is
/// remade.
fn mirror(from: &Path, to: &Path, n: &mut usize) -> Result<()> {
    fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    for e in fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))? {
        let e = e?;
        let src = e.path();
        let dst = to.join(e.file_name());
        let md = fs::symlink_metadata(&src)?;
        if md.is_dir() {
            mirror(&src, &dst, n)?;
        } else if md.file_type().is_symlink() {
            let target = fs::read_link(&src)?;
            symlink(&target.display().to_string(), &dst)?;
        } else {
            let same = fs::metadata(&dst).is_ok_and(|d| d.len() == md.len())
                && fs::read(&src).ok() == fs::read(&dst).ok();
            if !same {
                let _ = fs::remove_file(&dst);
                fs::copy(&src, &dst).map_err(|e| format!("{}: {e}", src.display()))?;
                *n += 1;
            }
        }
    }
    Ok(())
}

/// The `-I` directories of `words`: (index of the `-I` word, directory).
fn include_dirs(words: &[String]) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < words.len() {
        match words[i].strip_prefix("-I") {
            Some("") if i + 1 < words.len() => {
                out.push((i, words[i + 1].clone()));
                i += 2;
            }
            Some(d) if !d.is_empty() => {
                out.push((i, d.to_string()));
                i += 1;
            }
            _ => i += 1,
        }
    }
    out
}

/// The lower-case names of the top-level headers of libc (`include/`) and libc++
/// (`gnu/llvm/libcxx/include`).
fn system_headers(src: &Path) -> Result<BTreeSet<String>> {
    let mut out = BTreeSet::new();
    for d in ["include", "gnu/llvm/libcxx/include"] {
        let dir = src.join(d);
        for e in fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))? {
            let e = e?;
            // `*.h` only: libc++'s `<cerrno>` includes `<errno.h>`; extensionless names
            // (`vector`, but also `Makefile`) would match every directory's Makefile.
            if let Ok(name) = e.file_name().into_string()
                && e.path().is_file()
                && name.ends_with(".h")
            {
                out.insert(name.to_lowercase());
            }
        }
    }
    Ok(out)
}

/// The newest mtime of any file under `dir`.
fn newest_mtime(dir: &Path) -> Result<Option<SystemTime>> {
    let mut newest = mtime(dir);
    for e in fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let p = e?.path();
        let md = fs::symlink_metadata(&p)?;
        let m = if md.is_dir() {
            newest_mtime(&p)?
        } else {
            md.modified().ok()
        };
        newest = newest.max(m);
    }
    Ok(newest)
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_release_reads_sys_mk() {
        let dir = std::env::temp_dir().join(format!("emibsd-comp-sysmk-{}", std::process::id()));
        fs::create_dir_all(dir.join("share/mk")).unwrap();
        fs::write(
            dir.join("share/mk/sys.mk"),
            "unix=\t\twe're unix\nOSMAJOR=\t8\nOSMINOR=\t0\nOSREV=\t\t$(OSMAJOR).$(OSMINOR)\n",
        )
        .unwrap();
        assert_eq!(
            os_release(&dir).unwrap(),
            ("8".to_string(), "0".to_string())
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn mirror_copies_changed_files_only() {
        let dir = std::env::temp_dir().join(format!("emibsd-comp-mirror-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("a/sub")).unwrap();
        fs::write(dir.join("a/sub/x.h"), "x").unwrap();
        std::os::unix::fs::symlink("sub/x.h", dir.join("a/y.h")).unwrap();
        let mut n = 0;
        mirror(&dir.join("a"), &dir.join("b"), &mut n).unwrap();
        assert_eq!(n, 1);
        assert_eq!(fs::read_to_string(dir.join("b/y.h")).unwrap(), "x");
        let mut n = 0;
        mirror(&dir.join("a"), &dir.join("b"), &mut n).unwrap();
        assert_eq!(n, 0);
        let _ = fs::remove_dir_all(&dir);
    }
}
/* </TESTS> */
