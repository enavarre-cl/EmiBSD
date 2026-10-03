//! `cargo xtask userland --arch A`: cross-compiles OpenBSD's own userland C, unmodified, from
//! the reference clone (milestone M8, decided by the user on 2026-10-03).
//!
//! What it builds, all under `target/userland/<arch>/`:
//!
//! 1. `sysroot/usr/include`, assembled the way `include/Makefile` installs `/usr/include`
//!    (`includes` + `copies`): `FILES`, `DIRS`, the `LFILES`/`MFILES` symlinks, the kernel
//!    headers of `LDIRS`, `<machine/*>` from `sys/arch/<MACHINE>/include`, and of the `RDIRS`
//!    those of `lib/libutil` and `lib/librpcsvc` (made by OpenBSD's `rpcgen`, built for this
//!    machine first), the only ones what is built here needs.
//! 2. `lib/csu` (`crt0.o`, `rcrt0.o`, `crtbegin.o`, `crtend.o`, ...) by running the rules of
//!    its Makefile, into `sysroot/usr/lib`.
//! 3. `libc.a` and `libutil.a`: `SRCS`, `OBJS` and `.PATH` come from evaluating the libraries'
//!    Makefiles with `bsdmake.rs`; the system-call stubs are made by the rules of
//!    `lib/libc/sys/Makefile.inc` (`GENERATE.*` piped into `FINISH.*`), run through `/bin/sh`
//!    exactly as make would; generated C (the `lib/libc/hash` helpers) likewise.
//! 4. `sbin/init`, `bin/ksh`, `bin/echo` and `bin/ls`, linked as static PIE executables (what
//!    OpenBSD's `cc -static` makes for `/bin` and `/sbin`), installed stripped into `root/`.
//! 5. `ramdisk.ffs`: `root/` plus `/dev`, made into an ffs image by OpenBSD's makefs(8) built
//!    for this machine (`ramdisk.rs`).
//!
//! `share/mk` is not in the reference clone. `sys.mk` and `bsd.own.mk` are stood in for by
//! the predefined variables and `BSD_OWN_MK` below; `bsd.prog.mk`/`bsd.lib.mk` by
//! `BSD_PROG_MK` and the implicit `.c.o`/`.S.o` rules (`RULE_C_O`, `RULE_S_O`). Every compiler
//! flag workaround is in `UNSUPPORTED_FLAGS`, `HOST_CFLAGS` and `VARIANTS`, and in
//! `docs/ARCHITECTURE.md`. `-lcompiler_rt` is linked only when the clone has
//! `COMPILER_RT_DIR` (added to the sparse set on 2026-10-03); without it arm64 programs do
//! not link (`__multf3`).
//!
//! Tools (approved by the user on 2026-10-03): Apple clang (`$EMIBSD_CC`, default
//! `/usr/bin/clang`) and LLVM's `ld.lld`, `llvm-ar`, `llvm-ranlib`, `llvm-objcopy`,
//! `llvm-objdump` from `$EMIBSD_LLVM_BIN` (default `~/.swiftly/bin`). The OpenBSD sources come
//! from `$OPENBSD_SRC`, else `reference/openbsd-src` of this checkout, else of the main
//! checkout when run from a git worktree. Nothing is ever written there.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::SystemTime;

use crate::Result;
use crate::boot::Arch;
use crate::bsdmake::{Locals, Make};

/// `<bsd.own.mk>`: the OpenBSD defaults the Makefiles read.
const BSD_OWN_MK: &str = "\
YP?=\t\tyes
COMPILER_VERSION?=\tclang
BUILD_CLANG?=\tyes
STATIC?=\t-static
NOPIE_FLAGS?=\t-fno-pie
PICFLAG?=\t-fpic
";

/// `<bsd.prog.mk>` and `<bsd.lib.mk>`: the parts that change variables. `CDIAGFLAGS`
/// (warnings only) is left empty.
const BSD_PROG_MK: &str = "\
.if exists(${.CURDIR}/../Makefile.inc)
.include \"${.CURDIR}/../Makefile.inc\"
.endif
.include <bsd.own.mk>
.if ${WARNINGS:L} == \"yes\"
CFLAGS+=\t${CDIAGFLAGS}
.endif
CFLAGS+=\t${COPTS}
";

/// `bsd.lib.mk`'s `.c.o` rule, without its final `ld -X -r` (which only drops local
/// temporary symbols).
const RULE_C_O: &str = "${COMPILE.c} ${DFLAGS} -MF ${.TARGET:R}.d ${.IMPSRC} -o ${.TARGET}";
/// `bsd.lib.mk`'s `.S.o` rule, likewise.
const RULE_S_O: &str = "${COMPILE.S} ${CFLAGS:M-[IDM]*} ${AINC} ${DFLAGS} -MF ${.TARGET:R}.d \
                        -o ${.TARGET} ${.IMPSRC}";

/// Flags of OpenBSD's patched base clang that Apple clang does not know, removed from
/// `CFLAGS`/`AFLAGS`/`COPTS` after evaluation, with the reason.
const UNSUPPORTED_FLAGS: &[(&str, &str)] = &[(
    "-fret-clean",
    "OpenBSD-local clang option (lib/libc/arch/amd64/Makefile.inc): clears the return \
     address slot after use; Apple clang rejects it",
)];

/// A program built differently from its Makefile, and why.
struct Variant {
    dir: &'static str,
    add_cflags: &'static str,
    drop_ldadd: &'static [&'static str],
    why: &'static str,
}

const VARIANTS: &[Variant] = &[Variant {
    dir: "bin/ksh",
    add_cflags: "-DSMALL",
    drop_ldadd: &["-lcurses"],
    why: "built like OpenBSD's install-media ksh (-DSMALL, no -lcurses): libcurses \
          (ncurses, with host-built generators and share/termtypes) is not built yet",
}];

/// OpenBSD's compiler runtime (the `-lcompiler_rt` its clang driver adds to every link): a
/// Makefile over `gnu/llvm/compiler-rt` (Apache-2.0 WITH LLVM-exception), both in the sparse
/// clone since 2026-10-03. When the clone has it, it is built like the other libraries and
/// linked; `BUILD_CLANG` in `BSD_OWN_MK` selects its clang branch.
const COMPILER_RT_DIR: &str = "gnu/lib/libcompiler_rt";

/// The programs, in build order.
const PROGRAMS: &[&str] = &["sbin/init", "bin/ksh", "bin/echo", "bin/ls"];

/// Flags added to host tools (built for macOS with the same clang) and why.
const HOST_CFLAGS: &[(&str, &str)] = &[(
    "'-Dpledge(p,e)=0'",
    "usr.bin/rpcgen calls pledge(2), which macOS does not have; the call only restricts \
     the tool itself",
)];

/// One architecture's names, as OpenBSD's make sees them.
struct Machine {
    machine: &'static str,
    machine_arch: &'static str,
    machine_cpu: &'static str,
    triple: &'static str,
    e_machine: u16,
}

impl Machine {
    fn of(arch: Arch) -> Self {
        match arch {
            Arch::Amd64 => Machine {
                machine: "amd64",
                machine_arch: "amd64",
                machine_cpu: "amd64",
                triple: "x86_64-unknown-openbsd",
                e_machine: 62,
            },
            Arch::Arm64 => Machine {
                machine: "arm64",
                machine_arch: "aarch64",
                machine_cpu: "aarch64",
                triple: "aarch64-unknown-openbsd",
                e_machine: 183,
            },
        }
    }
}

/// The external tools, verified to exist.
struct Tools {
    cc: PathBuf,
    ld: PathBuf,
    ar: PathBuf,
    ranlib: PathBuf,
    objcopy: PathBuf,
    objdump: PathBuf,
}

impl Tools {
    fn locate() -> Result<Self> {
        let cc = std::env::var_os("EMIBSD_CC")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/usr/bin/clang"));
        let bindir = match std::env::var_os("EMIBSD_LLVM_BIN") {
            Some(d) => PathBuf::from(d),
            None => {
                let home = std::env::var_os("HOME").ok_or("$HOME is not set")?;
                PathBuf::from(home).join(".swiftly").join("bin")
            }
        };
        let need = |p: PathBuf, env: &str| -> Result<PathBuf> {
            if p.is_file() {
                Ok(p)
            } else {
                Err(format!(
                    "{} not found; see docs/SETUP.md (\"Userland toolchain\") or set ${env}",
                    p.display()
                )
                .into())
            }
        };
        Ok(Tools {
            cc: need(cc, "EMIBSD_CC")?,
            ld: need(bindir.join("ld.lld"), "EMIBSD_LLVM_BIN")?,
            ar: need(bindir.join("llvm-ar"), "EMIBSD_LLVM_BIN")?,
            ranlib: need(bindir.join("llvm-ranlib"), "EMIBSD_LLVM_BIN")?,
            objcopy: need(bindir.join("llvm-objcopy"), "EMIBSD_LLVM_BIN")?,
            objdump: need(bindir.join("llvm-objdump"), "EMIBSD_LLVM_BIN")?,
        })
    }
}

/// Everything a build step needs.
struct Ctx<'a> {
    src: PathBuf,
    out: PathBuf,
    sysroot: PathBuf,
    m: Machine,
    tools: &'a Tools,
    /// Installed header → its file in the reference tree (for the licence report).
    installed: Mutex<HashMap<PathBuf, PathBuf>>,
    /// Lower-cased installed path → installed path: two names that differ only in case
    /// would be one file on macOS's default file system.
    lower: Mutex<HashMap<String, PathBuf>>,
    /// Every input file a compile read (sources and headers), for the licence report.
    inputs: Mutex<BTreeSet<PathBuf>>,
}

/// `cargo xtask userland --arch A`.
pub fn userland(root: &Path, arch: Arch) -> Result<()> {
    let tools = Tools::locate()?;
    let src = fs::canonicalize(openbsd_src(root)?)?;
    let out = root.join("target").join("userland").join(arch.name());
    for p in [&src, &out] {
        if p.to_string_lossy().contains(char::is_whitespace) {
            return Err(format!("{}: paths with whitespace are not supported", p.display()).into());
        }
    }
    let sysroot = out.join("sysroot");
    let ctx = Ctx {
        src,
        sysroot,
        m: Machine::of(arch),
        tools: &tools,
        installed: Mutex::new(HashMap::new()),
        lower: Mutex::new(HashMap::new()),
        inputs: Mutex::new(BTreeSet::new()),
        out,
    };
    println!(
        "userland {}: OpenBSD sources {}, output {}",
        arch.name(),
        ctx.src.display(),
        ctx.out.display()
    );
    println!("  cc {}: {}", tools.cc.display(), tool_version(&tools.cc)?);
    println!("  ld {}: {}", tools.ld.display(), tool_version(&tools.ld)?);

    install_includes(&ctx)?;
    build_csu(&ctx)?;
    build_lib(&ctx, "lib/libc")?;
    build_lib(&ctx, "lib/libutil")?;
    if has_compiler_rt(&ctx) {
        build_lib(&ctx, COMPILER_RT_DIR)?;
    } else {
        println!("  {COMPILER_RT_DIR}: not in the reference clone; links go without it");
    }
    let mut built = Vec::new();
    let mut blocked = Vec::new();
    for dir in PROGRAMS {
        match build_prog(&ctx, dir)? {
            Linked::Yes(prog, exe, installed) => built.push((prog, exe, installed)),
            Linked::NeedsCompilerRt(symbols) => {
                println!(
                    "  {dir}: NOT LINKED: needs {} from libcompiler_rt",
                    symbols.join(" ")
                );
                blocked.push(*dir);
            }
        }
    }
    if !built.is_empty() {
        println!("  executables (static PIE, installed stripped under root/):");
    }
    for (name, path, installed) in &built {
        verify(&ctx, name, path, installed)?;
    }
    if blocked.is_empty() {
        ramdisk::build_ramdisk(&ctx)?;
        licence_report(&ctx)?;
        return Ok(());
    }
    licence_report(&ctx)?;
    Err(format!(
        "{}: {} not linked: they need compiler builtins that OpenBSD takes from \
         libcompiler_rt ({COMPILER_RT_DIR} over gnu/llvm/compiler-rt, Apache-2.0 WITH \
         LLVM-exception), which is not in the sparse reference clone; widening the clone is \
         the user's decision (docs/SETUP.md, \"Userland toolchain\")",
        arch.name(),
        blocked.join(", ")
    )
    .into())
}

fn has_compiler_rt(ctx: &Ctx<'_>) -> bool {
    ctx.src.join(COMPILER_RT_DIR).join("Makefile").is_file()
}

/// The outcome of linking a program.
enum Linked {
    /// Linked: the program's name, the linked file and the installed (stripped) copy.
    Yes(String, PathBuf, PathBuf),
    /// Every undefined symbol is a compiler builtin (`__multf3`, ...): libcompiler_rt is
    /// missing.
    NeedsCompilerRt(Vec<String>),
}

/// Whether `sym` is a compiler-rt builtin (soft quad float, 128-bit integer, emulated TLS).
fn is_compiler_builtin(sym: &str) -> bool {
    sym.starts_with("__")
        && (["tf3", "tf2", "ti3", "ti2", "di3"]
            .iter()
            .any(|s| sym.ends_with(s))
            || ["__extend", "__trunc", "__fix", "__float", "__emutls"]
                .iter()
                .any(|p| sym.starts_with(p)))
}

/// `$OPENBSD_SRC`, `<root>/reference/openbsd-src`, or the main checkout's when `root` is a
/// git worktree (the clone is gitignored, so worktrees do not have it).
fn openbsd_src(root: &Path) -> Result<PathBuf> {
    let usable = |p: &Path| p.join("lib/libc/Makefile").is_file() && p.join("sys").is_dir();
    if let Some(env) = std::env::var_os("OPENBSD_SRC") {
        let p = PathBuf::from(env);
        let p = if p.is_absolute() { p } else { root.join(p) };
        return if usable(&p) {
            Ok(p)
        } else {
            Err(format!("$OPENBSD_SRC={}: no lib/libc/Makefile or sys/", p.display()).into())
        };
    }
    let local = root.join(crate::REFERENCE_DIR);
    if usable(&local) {
        return Ok(local);
    }
    let common = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| PathBuf::from(String::from_utf8_lossy(&o.stdout).trim()));
    if let Some(main) = common.as_deref().and_then(Path::parent) {
        let p = main.join(crate::REFERENCE_DIR);
        if usable(&p) {
            return Ok(p);
        }
    }
    Err(format!(
        "{} has no userland sources (lib/, include/); clone it as in reference/README.md or set \
         $OPENBSD_SRC",
        local.display()
    )
    .into())
}

fn tool_version(tool: &Path) -> Result<String> {
    let out = Command::new(tool)
        .arg("--version")
        .output()
        .map_err(|e| format!("{}: {e}", tool.display()))?;
    if !out.status.success() {
        return Err(format!("{} --version failed", tool.display()).into());
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .unwrap_or("")
        .to_string())
}

// --- make glue ------------------------------------------------------------------------------

/// A `Make` for the Makefile in `dir` (relative to the sources), with what `sys.mk` and the
/// environment of a cross build would define.
fn new_make(ctx: &Ctx<'_>, dir: &str, objdir: &Path) -> Result<Make> {
    make_for(ctx, dir, objdir, false)
}

/// A `Make` for a build tool that runs on this machine (`HOSTCC`): same clang, no target,
/// no sysroot.
fn new_host_make(ctx: &Ctx<'_>, dir: &str, objdir: &Path) -> Result<Make> {
    let mut mk = make_for(ctx, dir, objdir, true)?;
    let cflags = mk.var("CFLAGS")?;
    let extra: Vec<&str> = HOST_CFLAGS.iter().map(|(f, _)| *f).collect();
    mk.set("CFLAGS", &format!("{cflags} {}", extra.join(" ")));
    for (f, why) in HOST_CFLAGS {
        println!("  {dir} (host tool): added {f}: {why}");
    }
    Ok(mk)
}

fn make_for(ctx: &Ctx<'_>, dir: &str, objdir: &Path, host: bool) -> Result<Make> {
    let curdir = ctx.src.join(dir);
    let t = ctx.tools;
    let cc = if host {
        t.cc.display().to_string()
    } else {
        format!(
            "{} --target={} --sysroot={}",
            t.cc.display(),
            ctx.m.triple,
            ctx.sysroot.display()
        )
    };
    let predefined = [
        ("MACHINE", ctx.m.machine.to_string()),
        ("MACHINE_ARCH", ctx.m.machine_arch.to_string()),
        ("MACHINE_CPU", ctx.m.machine_cpu.to_string()),
        (".OBJDIR", objdir.display().to_string()),
        ("DESTDIR", ctx.sysroot.display().to_string()),
        ("CC", cc),
        ("LD", t.ld.display().to_string()),
        ("AR", t.ar.display().to_string()),
        ("RANLIB", t.ranlib.display().to_string()),
        // sys.mk
        ("CFLAGS", "-O2 ${PIPE} ${DEBUG}".to_string()),
        ("PIPE", "-pipe".to_string()),
        ("COMPILE.c", "${CC} ${CFLAGS} ${CPPFLAGS} -c".to_string()),
        ("COMPILE.S", "${CC} ${AFLAGS} ${CPPFLAGS} -c".to_string()),
        ("DFLAGS", "-MD -MP".to_string()),
    ];
    let sys_mk = [
        ("bsd.own.mk", BSD_OWN_MK),
        ("bsd.prog.mk", BSD_PROG_MK),
        ("bsd.lib.mk", BSD_PROG_MK),
    ];
    let mut mk = Make::new(&curdir, &predefined, &sys_mk);
    mk.read(&curdir.join("Makefile"))?;
    for (flag, why) in UNSUPPORTED_FLAGS {
        let mut removed = false;
        for var in ["CFLAGS", "AFLAGS", "COPTS"] {
            removed |= mk.remove_word(var, flag);
        }
        if removed {
            println!("  {dir}: dropped {flag}: {why}");
        }
    }
    Ok(mk)
}

/// Target-local variables for making `target` from `sources`.
fn locals(target: &str, sources: &[PathBuf]) -> Locals {
    let stem = target
        .rsplit_once('.')
        .map_or(target, |(s, _)| s)
        .to_string();
    let all: Vec<String> = sources.iter().map(|p| p.display().to_string()).collect();
    let first = all.first().cloned().unwrap_or_default();
    let mut l = Locals::new();
    for (k, v) in [
        ("@", target.to_string()),
        (".TARGET", target.to_string()),
        ("*", stem.clone()),
        (".PREFIX", stem),
        ("<", first.clone()),
        (".IMPSRC", first),
        (">", all.join(" ")),
        (".ALLSRC", all.join(" ")),
    ] {
        l.insert(k.to_string(), v);
    }
    l
}

/// One target and the shell commands that make it.
struct Job {
    target: PathBuf,
    cwd: PathBuf,
    /// Expanded commands, with make's `-` (ignore errors) prefix kept as a flag.
    commands: Vec<(String, bool)>,
    /// Inputs known before running (rule sources); the `.d` file adds the rest.
    deps: Vec<PathBuf>,
    /// A directory put first in `$PATH` (where the host-built tools are).
    path: Option<PathBuf>,
}

impl Job {
    /// The job for an explicit rule's commands.
    fn from_rule(
        mk: &Make,
        commands: &[String],
        target: &str,
        sources: Vec<PathBuf>,
        objdir: &Path,
    ) -> Result<Job> {
        let l = locals(target, &sources);
        let mut cmds = Vec::new();
        for c in commands {
            let mut c = c.as_str();
            let mut ignore = false;
            while let Some(rest) = c.strip_prefix(['@', '-', '+']) {
                ignore |= c.starts_with('-');
                c = rest;
            }
            cmds.push((mk.expand_local(c, &l)?, ignore));
        }
        Ok(Job {
            target: objdir.join(target),
            cwd: objdir.to_path_buf(),
            commands: cmds,
            deps: sources,
            path: None,
        })
    }

    fn stamp(&self) -> String {
        let mut s = String::new();
        for (c, _) in &self.commands {
            s.push_str(c);
            s.push('\n');
        }
        s
    }

    fn stamp_path(&self) -> PathBuf {
        let mut p = self.target.clone().into_os_string();
        p.push(".cmd");
        PathBuf::from(p)
    }

    fn depfile(&self) -> Option<PathBuf> {
        (self.target.extension().and_then(|e| e.to_str()) == Some("o"))
            .then(|| self.target.with_extension("d"))
    }

    /// Every input: the rule's sources and what the compiler's `.d` file lists.
    fn all_deps(&self) -> Vec<PathBuf> {
        let mut deps = self.deps.clone();
        if let Some(d) = self.depfile().and_then(|d| fs::read_to_string(d).ok()) {
            deps.extend(
                parse_depfile(&d)
                    .into_iter()
                    .map(|p| if p.is_absolute() { p } else { self.cwd.join(p) }),
            );
        }
        deps
    }

    fn up_to_date(&self) -> bool {
        let Some(t) = mtime(&self.target) else {
            return false;
        };
        if fs::read_to_string(self.stamp_path()).ok().as_deref() != Some(self.stamp().as_str()) {
            return false;
        }
        self.all_deps()
            .iter()
            .all(|d| d.as_os_str() == "-" || mtime(d).is_some_and(|m| m <= t))
    }

    fn run(&self) -> std::result::Result<(), String> {
        for (cmd, ignore) in &self.commands {
            let mut sh = Command::new("/bin/sh");
            sh.arg("-c")
                .arg(cmd)
                .current_dir(&self.cwd)
                .stdin(Stdio::null());
            if let Some(dir) = &self.path {
                let old = std::env::var_os("PATH").unwrap_or_default();
                let mut paths = vec![dir.clone()];
                paths.extend(std::env::split_paths(&old));
                sh.env(
                    "PATH",
                    std::env::join_paths(paths).map_err(|e| format!("PATH: {e}"))?,
                );
            }
            let out = sh.output().map_err(|e| format!("/bin/sh: {e}"))?;
            if !out.status.success() && !ignore {
                let _ = fs::remove_file(&self.target);
                return Err(format!(
                    "{}\n{}{}",
                    cmd,
                    String::from_utf8_lossy(&out.stdout),
                    String::from_utf8_lossy(&out.stderr)
                ));
            }
        }
        fs::write(self.stamp_path(), self.stamp()).map_err(|e| e.to_string())
    }
}

/// The prerequisites listed by a compiler `.d` file (first rule only).
fn parse_depfile(text: &str) -> Vec<PathBuf> {
    let joined = text.replace("\\\n", " ");
    let Some(first) = joined.lines().next() else {
        return Vec::new();
    };
    let Some((_, deps)) = first.split_once(": ") else {
        return Vec::new();
    };
    deps.split_whitespace().map(PathBuf::from).collect()
}

fn mtime(p: &Path) -> Option<SystemTime> {
    fs::metadata(p).and_then(|m| m.modified()).ok()
}

/// Runs the jobs that are out of date, in parallel; returns how many ran. Every failure is
/// reported, not just the first.
fn run_jobs(ctx: &Ctx<'_>, what: &str, jobs: &[Job]) -> Result<usize> {
    let todo: Vec<&Job> = jobs.iter().filter(|j| !j.up_to_date()).collect();
    let next = AtomicUsize::new(0);
    let failures: Mutex<Vec<(PathBuf, String)>> = Mutex::new(Vec::new());
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get());
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(job) = todo.get(i) else {
                        break;
                    };
                    if let Err(e) = job.run()
                        && let Ok(mut f) = failures.lock()
                    {
                        f.push((job.target.clone(), e));
                    }
                }
            });
        }
    });
    if let Ok(mut inputs) = ctx.inputs.lock() {
        for j in jobs {
            inputs.extend(j.all_deps());
        }
    }
    let failures = failures
        .into_inner()
        .map_err(|_| "a build thread panicked")?;
    if failures.is_empty() {
        return Ok(todo.len());
    }
    for (target, err) in &failures {
        eprintln!("--- failed: {}", target.display());
        for line in err.lines().take(40) {
            eprintln!("    {line}");
        }
    }
    Err(format!("{what}: {} of {} job(s) failed", failures.len(), jobs.len()).into())
}

// --- 1. /usr/include ------------------------------------------------------------------------

/// Copies `from` to `to` unless `to` already has the same contents (`cmp -s || install`), so
/// unchanged headers keep their mtime and nothing recompiles. Returns whether it wrote.
fn install_file(ctx: &Ctx<'_>, from: &Path, to: &Path) -> Result<bool> {
    let data = fs::read(from).map_err(|e| format!("{}: {e}", from.display()))?;
    if let Ok(mut l) = ctx.lower.lock() {
        let key = to.to_string_lossy().to_lowercase();
        if let Some(other) = l.get(&key).filter(|o| o.as_path() != to) {
            return Err(format!(
                "{} and {} differ only in case; this file system cannot hold both",
                other.display(),
                to.display()
            )
            .into());
        }
        l.insert(key, to.to_path_buf());
    }
    if let Ok(mut m) = ctx.installed.lock() {
        m.insert(to.to_path_buf(), from.to_path_buf());
    }
    if fs::read(to).ok().as_deref() == Some(data.as_slice()) {
        return Ok(false);
    }
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let _ = fs::remove_file(to);
    fs::write(to, &data).map_err(|e| format!("{}: {e}", to.display()))?;
    Ok(true)
}

/// `ln -sf target link`, unless it already points there.
fn symlink(target: &str, link: &Path) -> Result<()> {
    if fs::read_link(link).ok().as_deref() == Some(Path::new(target)) {
        return Ok(());
    }
    if fs::symlink_metadata(link).is_ok_and(|m| m.is_dir()) {
        fs::remove_dir_all(link).map_err(|e| format!("{}: {e}", link.display()))?;
    } else {
        let _ = fs::remove_file(link);
    }
    std::os::unix::fs::symlink(target, link)
        .map_err(|e| format!("ln -s {target} {}: {e}", link.display()).into())
}

/// Files of `dir` (not recursive) whose name matches `pattern`, sorted.
fn glob_dir(dir: &Path, pattern: &str) -> Result<Vec<PathBuf>> {
    let mut v = Vec::new();
    for e in fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let p = e?.path();
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if p.is_file() && crate::bsdmake::glob(pattern.as_bytes(), name.as_bytes()) {
            v.push(p);
        }
    }
    v.sort();
    Ok(v)
}

/// `find dir -follow -type f -name '*.h'`, as paths relative to `base`.
fn find_headers(base: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for e in fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let p = e?.path();
        // `metadata` follows symbolic links, like `find -follow`.
        let Ok(meta) = fs::metadata(&p) else {
            continue;
        };
        if meta.is_dir() {
            find_headers(base, &p, out)?;
        } else if meta.is_file() && p.extension().and_then(|x| x.to_str()) == Some("h") {
            out.push(p.strip_prefix(base).unwrap_or(&p).to_path_buf());
        }
    }
    Ok(())
}

fn install_includes(ctx: &Ctx<'_>) -> Result<()> {
    let objdir = ctx.out.join("obj/include");
    let mk = new_make(ctx, "include", &objdir)?;
    let inc = ctx.sysroot.join("usr/include");
    let srcinc = ctx.src.join("include");
    let mut total = 0usize;
    let mut written = 0usize;
    let mut count = |w: bool| {
        total += 1;
        written += usize::from(w);
    };

    // includes: FILES, DIRS, LFILES, MFILES.
    for f in mk.words("FILES")? {
        count(install_file(ctx, &srcinc.join(&f), &inc.join(&f))?);
    }
    for d in mk.words("DIRS")? {
        for p in glob_dir(&srcinc.join(&d), "*.[ih]")? {
            let name = p.file_name().ok_or("bad file name")?;
            count(install_file(ctx, &p, &inc.join(&d).join(name))?);
        }
    }
    for f in mk.words("LFILES")? {
        symlink(&format!("sys/{f}"), &inc.join(&f))?;
    }
    for f in mk.words("MFILES")? {
        symlink(&format!("machine/{f}"), &inc.join(&f))?;
    }

    // copies (SYS_INCLUDE?= copies): LDIRS from sys/, then <machine/*>.
    if mk.var("SYS_INCLUDE")? != "copies" {
        return Err("include/Makefile: SYS_INCLUDE is not `copies`".into());
    }
    let sys = ctx.src.join("sys");
    let mut headers = Vec::new();
    for d in mk.words("LDIRS")? {
        find_headers(&sys, &sys.join(&d), &mut headers)?;
    }
    headers.retain(|h| !(h.starts_with("dev/microcode") || h.starts_with("dev/pci/drm")));
    headers.sort();
    for h in &headers {
        count(install_file(ctx, &sys.join(h), &inc.join(h))?);
    }
    let (machine, cpu) = (ctx.m.machine, ctx.m.machine_cpu);
    for p in glob_dir(&sys.join("arch").join(machine).join("include"), "*.h")? {
        let name = p.file_name().ok_or("bad file name")?;
        count(install_file(ctx, &p, &inc.join(machine).join(name))?);
    }
    let cpu_inc = sys.join("arch").join(cpu).join("include");
    if machine != cpu && cpu_inc.is_dir() {
        for p in glob_dir(&cpu_inc, "*.h")? {
            let name = p.file_name().ok_or("bad file name")?;
            count(install_file(ctx, &p, &inc.join(cpu).join(name))?);
        }
    }
    symlink(machine, &inc.join("machine"))?;

    // RDIRS (PRDIRS included): their own `includes` targets. Only libutil's and librpcsvc's
    // headers are needed by what is built here.
    let mut skipped = Vec::new();
    for r in mk.words("RDIRS")? {
        if r == "../lib/librpcsvc" {
            for (from, to) in rpcsvc_headers(ctx)? {
                count(install_file(ctx, &from, &inc.join("rpcsvc").join(to))?);
            }
            continue;
        }
        if r != "../lib/libutil" {
            skipped.push(r);
            continue;
        }
        let dir = Path::new("include").join(&r);
        let dir = dir.to_string_lossy();
        let sub = new_make(ctx, &dir, &objdir)?;
        let rule = sub
            .rule_for("includes")
            .ok_or_else(|| format!("{r}/Makefile: no `includes` target"))?;
        if !rule.commands.iter().any(|c| c.contains("$(HDRS)")) {
            return Err(format!("{r}/Makefile: `includes` does not install $(HDRS)").into());
        }
        for h in sub.words("HDRS")? {
            count(install_file(
                ctx,
                &ctx.src.join(&*dir).join(&h),
                &inc.join(&h),
            )?);
        }
    }
    println!(
        "  /usr/include: {total} headers ({written} updated); RDIRS not installed (nothing \
         built here needs them): {}",
        skipped.join(" ")
    );
    Ok(())
}

/// `lib/librpcsvc`'s `includes`: its `.x` files and the headers its `.x.h` rule makes with
/// `rpcgen`, which is OpenBSD's own, built for this machine first. Returns (file, name).
fn rpcsvc_headers(ctx: &Ctx<'_>) -> Result<Vec<(PathBuf, String)>> {
    let bindir = build_host_prog(ctx, "usr.bin/rpcgen")?;
    let dir = "lib/librpcsvc";
    let objdir = ctx.out.join("obj").join(dir);
    fs::create_dir_all(&objdir).map_err(|e| format!("{}: {e}", objdir.display()))?;
    let mk = new_make(ctx, dir, &objdir)?;
    let rule = mk
        .rule_for(".x.h")
        .ok_or("lib/librpcsvc/Makefile: no .x.h rule")?;
    let mut jobs = Vec::new();
    let mut out = Vec::new();
    for x in mk.words("RPCSRCS")? {
        let src = mk
            .search(&x)
            .ok_or_else(|| format!("lib/librpcsvc: no {x}"))?;
        let h = format!("{}.h", x.trim_end_matches(".x"));
        let mut job = Job::from_rule(&mk, &rule.commands, &h, vec![src.clone()], &objdir)?;
        job.path = Some(bindir.clone());
        out.push((src, x));
        out.push((job.target.clone(), h));
        jobs.push(job);
    }
    let hdrs = mk.words("HDRS")?;
    if hdrs.len() != jobs.len() {
        return Err("lib/librpcsvc: HDRS and RPCSRCS disagree".into());
    }
    run_jobs(ctx, dir, &jobs)?;
    Ok(out)
}

/// Builds a program for this machine (a build tool); returns the directory holding it.
fn build_host_prog(ctx: &Ctx<'_>, dir: &str) -> Result<PathBuf> {
    build_host_prog_with(ctx, dir, |_, _| Ok(()))
}

/// `build_host_prog`, with `adapt` changing the evaluated Makefile (given the object
/// directory) before anything is compiled: the host portability shims of `ramdisk.rs`.
fn build_host_prog_with(
    ctx: &Ctx<'_>,
    dir: &str,
    adapt: impl FnOnce(&mut Make, &Path) -> Result<()>,
) -> Result<PathBuf> {
    let objdir = ctx.out.join("host/obj").join(dir);
    let bindir = ctx.out.join("host/bin");
    for d in [&objdir, &bindir] {
        fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    let mut mk = new_host_make(ctx, dir, &objdir)?;
    adapt(&mut mk, &objdir)?;
    let prog = mk.var("PROG")?;
    let jobs = object_jobs(ctx, &mk, &objdir, &[])?;
    run_jobs(ctx, dir, &jobs)?;
    let objs: Vec<PathBuf> = jobs.iter().map(|j| j.target.clone()).collect();
    let target = bindir.join(&prog);
    let link = Job::from_rule(
        &mk,
        &["${CC} ${LDFLAGS} -o ${.TARGET} ${.ALLSRC}".to_string()],
        &target.display().to_string(),
        objs,
        &objdir,
    )?;
    run_jobs(ctx, dir, &[link])?;
    Ok(bindir)
}

// --- 2. csu ---------------------------------------------------------------------------------

fn build_csu(ctx: &Ctx<'_>) -> Result<()> {
    let objdir = ctx.out.join("obj/lib/csu");
    fs::create_dir_all(&objdir).map_err(|e| format!("{}: {e}", objdir.display()))?;
    let mk = new_make(ctx, "lib/csu", &objdir)?;
    let mut jobs = Vec::new();
    for o in mk.words("OBJS")? {
        let rule = mk
            .rule_for(&o)
            .ok_or_else(|| format!("lib/csu/Makefile: no rule for {o}"))?;
        let sources = resolve_sources(&mk, &rule.sources)?;
        jobs.push(Job::from_rule(&mk, &rule.commands, &o, sources, &objdir)?);
    }
    let ran = run_jobs(ctx, "lib/csu", &jobs)?;
    let lib = ctx.sysroot.join("usr/lib");
    for j in &jobs {
        let name = j.target.file_name().ok_or("bad object name")?;
        install_file(ctx, &j.target, &lib.join(name))?;
    }
    println!("  lib/csu: {} objects ({ran} rebuilt)", jobs.len());
    Ok(())
}

/// Rule sources: absolute paths stay, others are looked up like make does.
fn resolve_sources(mk: &Make, sources: &[String]) -> Result<Vec<PathBuf>> {
    sources
        .iter()
        .map(|s| {
            mk.search(s)
                .ok_or_else(|| format!("cannot find source {s}").into())
        })
        .collect()
}

// --- 3. libraries ---------------------------------------------------------------------------

/// The object files `mk` builds, as jobs: explicit rules (the system-call stubs) or the
/// implicit `.c.o`/`.S.o` rules on the source found through `.PATH` or generated by a rule.
fn object_jobs(ctx: &Ctx<'_>, mk: &Make, objdir: &Path, extra_objs: &[String]) -> Result<Vec<Job>> {
    let srcs = mk.words("SRCS")?;
    let mut objs: Vec<String> = extra_objs.to_vec();
    for s in srcs.iter().filter(|s| !s.ends_with(".h")) {
        let stem = s.rsplit_once('.').map_or(s.as_str(), |(a, _)| a);
        objs.push(format!("{stem}.o"));
    }
    let mut seen = BTreeSet::new();
    objs.retain(|o| seen.insert(o.clone()));

    let mut generated = Vec::new();
    let mut jobs = Vec::new();
    let mut lower: HashMap<String, usize> = HashMap::new();
    for o in &objs {
        // macOS file systems ignore case by default: libc's `_exit.o` (a system-call stub)
        // and `_Exit.o` (stdlib/_Exit.c) would be one file. The second of such a pair is
        // built in a subdirectory of its own; the archive keeps both member names.
        let n = lower.entry(o.to_lowercase()).or_insert(0);
        let odir = if *n == 0 {
            objdir.to_path_buf()
        } else {
            objdir.join(format!("case{n}"))
        };
        *n += 1;
        fs::create_dir_all(&odir).map_err(|e| format!("{}: {e}", odir.display()))?;
        if let Some(rule) = mk.rule_for(o) {
            let sources = resolve_sources(mk, &rule.sources)?;
            jobs.push(Job::from_rule(mk, &rule.commands, o, sources, &odir)?);
            continue;
        }
        let stem = &o[..o.len() - 2];
        let named: Vec<String> = srcs
            .iter()
            .filter(|s| s.rsplit_once('.').is_some_and(|(a, _)| a == stem))
            .cloned()
            .collect();
        let candidates = if named.is_empty() {
            ["c", "S", "s"]
                .iter()
                .map(|x| format!("{stem}.{x}"))
                .collect()
        } else {
            named
        };
        let mut found = None;
        for c in &candidates {
            if let Some(rule) = mk.rule_for(c) {
                let sources = resolve_sources(mk, &rule.sources)?;
                generated.push(Job::from_rule(mk, &rule.commands, c, sources, objdir)?);
                found = Some((c.clone(), objdir.join(c)));
                break;
            }
            if let Some(p) = mk.search(c) {
                found = Some((c.clone(), p));
                break;
            }
        }
        let (name, path) =
            found.ok_or_else(|| format!("no source for {o} (tried {candidates:?})"))?;
        let template = if name.ends_with(".c") {
            RULE_C_O
        } else {
            RULE_S_O
        };
        jobs.push(Job::from_rule(
            mk,
            &[template.to_string()],
            o,
            vec![path],
            &odir,
        )?);
    }
    if !generated.is_empty() {
        run_jobs(ctx, "generated sources", &generated)?;
    }
    Ok(jobs)
}

fn build_lib(ctx: &Ctx<'_>, dir: &str) -> Result<()> {
    let objdir = ctx.out.join("obj").join(dir);
    fs::create_dir_all(&objdir).map_err(|e| format!("{}: {e}", objdir.display()))?;
    let mk = new_make(ctx, dir, &objdir)?;
    let lib = mk.var("LIB")?;
    let jobs = object_jobs(ctx, &mk, &objdir, &mk.words("OBJS")?)?;
    let ran = run_jobs(ctx, dir, &jobs)?;

    let archive = objdir.join(format!("lib{lib}.a"));
    let _ = fs::remove_file(&archive);
    let mut ar = Command::new(&ctx.tools.ar);
    ar.arg("cq").arg(&archive).current_dir(&objdir);
    for j in &jobs {
        ar.arg(j.target.strip_prefix(&objdir).unwrap_or(&j.target));
    }
    run(&mut ar)?;
    run(Command::new(&ctx.tools.ranlib).arg(&archive))?;
    let installed = ctx.sysroot.join("usr/lib").join(format!("lib{lib}.a"));
    install_file(ctx, &archive, &installed)?;
    let size = fs::metadata(&archive).map(|m| m.len()).unwrap_or(0);
    println!(
        "  {dir}: lib{lib}.a, {} objects ({ran} rebuilt), {size} bytes",
        jobs.len()
    );
    Ok(())
}

fn run(cmd: &mut Command) -> Result<()> {
    let out = cmd.output().map_err(|e| format!("{cmd:?}: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{cmd:?} failed:\n{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
        .into())
    }
}

// --- 4. programs ----------------------------------------------------------------------------

/// Builds the program in `dir`; returns its name, the linked file and the installed copy.
fn build_prog(ctx: &Ctx<'_>, dir: &str) -> Result<Linked> {
    let objdir = ctx.out.join("obj").join(dir);
    fs::create_dir_all(&objdir).map_err(|e| format!("{}: {e}", objdir.display()))?;
    let mut mk = new_make(ctx, dir, &objdir)?;
    let prog = mk.var("PROG")?;
    if prog.is_empty() {
        return Err(format!("{dir}/Makefile: no PROG").into());
    }
    if !mk.defined("SRCS") {
        mk.set("SRCS", &format!("{prog}.c"));
    }
    let mut ldadd = mk.words("LDADD")?;
    if let Some(v) = VARIANTS.iter().find(|v| v.dir == dir) {
        let cflags = mk.var("CFLAGS")?;
        mk.set("CFLAGS", &format!("{cflags} {}", v.add_cflags));
        ldadd.retain(|w| !v.drop_ldadd.contains(&w.as_str()));
        println!("  {dir}: {}", v.why);
    }
    if !mk.words("LDSTATIC")?.iter().any(|w| w == "-static") {
        return Err(format!(
            "{dir}: not linked -static (LDSTATIC); dynamic programs need ld.so, not built"
        )
        .into());
    }
    if let Some(w) = ldadd.iter().find(|w| !w.starts_with("-l")) {
        return Err(format!("{dir}: unsupported LDADD word `{w}`").into());
    }

    let jobs = object_jobs(ctx, &mk, &objdir, &[])?;
    let ran = run_jobs(ctx, dir, &jobs)?;

    // What OpenBSD's clang driver passes for `cc -static` (its lld defaults to PIE; LLD 17
    // needs `-pie` said); `-lcompiler_rt` only when the clone has it (`COMPILER_RT_DIR`).
    let lib = ctx.sysroot.join("usr/lib");
    let exe = objdir.join(&prog);
    let mut ld = Command::new(&ctx.tools.ld);
    ld.arg(format!("--sysroot={}", ctx.sysroot.display()))
        .args(["-e", "__start", "--eh-frame-hdr", "-Bstatic", "-pie", "-o"])
        .arg(&exe)
        .arg(lib.join("rcrt0.o"))
        .arg(lib.join("crtbegin.o"))
        .arg(format!("-L{}", lib.display()));
    for j in &jobs {
        ld.arg(&j.target);
    }
    ld.args(&ldadd);
    if has_compiler_rt(ctx) {
        ld.args(["-lcompiler_rt", "-lc", "-lcompiler_rt"]);
    } else {
        ld.arg("-lc");
    }
    ld.arg(lib.join("crtend.o"));
    let out = ld.output().map_err(|e| format!("ld.lld: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let undefined: BTreeSet<String> = stderr
            .lines()
            .filter_map(|l| l.strip_prefix("ld.lld: error: undefined symbol: "))
            .map(|s| s.trim().to_string())
            .collect();
        if !undefined.is_empty() && undefined.iter().all(|s| is_compiler_builtin(s)) {
            return Ok(Linked::NeedsCompilerRt(undefined.into_iter().collect()));
        }
        return Err(format!("{dir}: link failed:\n{ld:?}\n{stderr}").into());
    }

    let bindir = mk.var("BINDIR")?;
    let rootdir = ctx.out.join("root");
    let installed = rootdir.join(bindir.trim_start_matches('/')).join(&prog);
    if let Some(parent) = installed.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    // `install -s`.
    run(Command::new(&ctx.tools.objcopy)
        .arg("--strip-all")
        .arg(&exe)
        .arg(&installed))?;
    // LINKS: pairs of (existing, new) absolute paths, hard links like install(1) makes.
    let links = mk.words("LINKS")?;
    for pair in links.chunks(2) {
        if let [from, to] = pair {
            let to = rootdir.join(to.trim_start_matches('/'));
            let _ = fs::remove_file(&to);
            fs::hard_link(rootdir.join(from.trim_start_matches('/')), &to)
                .map_err(|e| format!("ln {from} {}: {e}", to.display()))?;
        }
    }
    println!("  {dir}: {} objects ({ran} rebuilt), linked", jobs.len());
    Ok(Linked::Yes(prog, exe, installed))
}

// --- verification ---------------------------------------------------------------------------

fn phdr_name(t: u32) -> String {
    match t {
        1 => "LOAD".into(),
        2 => "DYNAMIC".into(),
        3 => "INTERP".into(),
        4 => "NOTE".into(),
        6 => "PHDR".into(),
        7 => "TLS".into(),
        0x6474_e550 => "GNU_EH_FRAME".into(),
        0x6474_e551 => "GNU_STACK".into(),
        0x6474_e552 => "GNU_RELRO".into(),
        0x6474_e553 => "GNU_PROPERTY".into(),
        0x65a3_dbe5 => "OPENBSD_MUTABLE".into(),
        0x65a3_dbe6 => "OPENBSD_RANDOMIZE".into(),
        0x65a3_dbe7 => "OPENBSD_WXNEEDED".into(),
        0x65a3_dbe8 => "OPENBSD_NOBTCFI".into(),
        0x65a3_dbe9 => "OPENBSD_SYSCALLS".into(),
        0x65a4_1be6 => "OPENBSD_BOOTDATA".into(),
        other => format!("{other:#x}"),
    }
}

/// Checks the ELF header and program headers directly, and with `llvm-objdump -p -h`.
fn verify(ctx: &Ctx<'_>, name: &str, exe: &Path, installed: &Path) -> Result<()> {
    let data = fs::read(exe).map_err(|e| format!("{}: {e}", exe.display()))?;
    let u16_at = |o: usize| data.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
    let u32_at = |o: usize| {
        data.get(o..o + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    let u64_at = |o: usize| {
        data.get(o..o + 8).map(|b| {
            let mut a = [0u8; 8];
            a.copy_from_slice(b);
            u64::from_le_bytes(a)
        })
    };
    if data.get(..4) != Some(b"\x7fELF".as_slice()) || data.get(4) != Some(&2) {
        return Err(format!("{name}: not an ELF64 file").into());
    }
    let (Some(e_type), Some(e_machine), Some(phoff), Some(phentsize), Some(phnum)) =
        (u16_at(16), u16_at(18), u64_at(32), u16_at(54), u16_at(56))
    else {
        return Err(format!("{name}: truncated ELF header").into());
    };
    if e_machine != ctx.m.e_machine {
        return Err(format!(
            "{name}: e_machine {e_machine}, expected {}",
            ctx.m.e_machine
        )
        .into());
    }
    let mut types = Vec::new();
    for i in 0..usize::from(phnum) {
        let off = usize::try_from(phoff)? + i * usize::from(phentsize);
        types.push(phdr_name(
            u32_at(off).ok_or_else(|| format!("{name}: truncated program headers"))?,
        ));
    }
    if types.iter().any(|t| t == "INTERP") {
        return Err(format!("{name}: has PT_INTERP, not statically linked").into());
    }
    let objdump = Command::new(&ctx.tools.objdump)
        .args(["-p", "-h"])
        .arg(exe)
        .output()
        .map_err(|e| format!("llvm-objdump: {e}"))?;
    let text = String::from_utf8_lossy(&objdump.stdout);
    if !objdump.status.success() || text.lines().any(|l| l.trim_start().starts_with("INTERP ")) {
        return Err(format!("{name}: llvm-objdump -p failed or shows INTERP").into());
    }
    if !text.contains(".note.openbsd.ident") {
        return Err(format!("{name}: no .note.openbsd.ident section").into());
    }
    let report = exe.with_extension("objdump.txt");
    fs::write(&report, text.as_bytes()).map_err(|e| format!("{}: {e}", report.display()))?;
    let size = data.len();
    let stripped = fs::metadata(installed).map(|m| m.len()).unwrap_or(0);
    let kind = if e_type == 3 {
        "ET_DYN (PIE)"
    } else {
        "ET_EXEC"
    };
    println!(
        "    {name}: ELF64 {} {kind}, no PT_INTERP, {size} bytes ({stripped} stripped); \
         phdrs: {}",
        ctx.m.machine_arch,
        types.join(" ")
    );
    Ok(())
}

// --- licences -------------------------------------------------------------------------------

/// The licence families named in a file's text (first 300 lines).
fn licence_families(text: &str) -> Vec<&'static str> {
    let head: String = text.lines().take(300).collect::<Vec<_>>().join(" ");
    let t = head
        .to_lowercase()
        .replace(['*', '#', '\t'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let mut f = Vec::new();
    if t.contains("gnu general public license") || t.contains("gnu lesser general public") {
        f.push("GPL/LGPL");
    }
    if t.contains("redistribution and use in source and binary forms") {
        if t.contains("all advertising materials mentioning") {
            f.push("BSD-4-Clause");
        } else if t.contains("neither the name") || t.contains("to endorse or promote") {
            f.push("BSD-3-Clause");
        } else {
            f.push("BSD-2-Clause");
        }
    }
    if t.contains("permission to use, copy, modify, and/or distribute this software for any")
        || t.contains("permission to use, copy, modify, and distribute this software for any purpose with or without fee")
    {
        f.push("ISC");
    }
    if t.contains("permission is hereby granted, free of charge") {
        f.push(if t.contains("unicode, inc") {
            "Unicode (data files and software)"
        } else {
            "MIT"
        });
    }
    if t.contains("beer-ware") {
        f.push("beerware");
    }
    if t.contains("carnegie mellon") && t.contains("permission to use, copy, modify and distribute")
    {
        f.push("Mach (CMU)");
    }
    if t.contains("spdx-license-identifier: apache-2.0 with llvm-exception") {
        f.push("Apache-2.0 WITH LLVM-exception");
    }
    if t.contains("lucent technologies")
        && t.contains("permission to use, copy, modify, and distribute")
    {
        f.push("Lucent (gdtoa)");
    }
    if t.contains("martin birgmeier") && t.contains("you may redistribute unmodified or modified") {
        f.push("Birgmeier (rand48)");
    }
    if t.contains("developed at sunpro") && t.contains("is freely granted") {
        f.push("SunPro (fdlibm)");
    }
    if t.contains("aleksey cheusov") && t.contains("permission to use or copy this software") {
        f.push("Cheusov");
    }
    if t.contains("daniel boulet") && t.contains("provided that this entire comment appears intact")
    {
        f.push("Boulet/RTMX");
    }
    if f.is_empty() {
        if t.contains("permission to use, copy, modify, and distribute this software")
            || t.contains("permission to use, copy, modify and distribute this software")
        {
            f.push("other permissive notice");
        } else if t.contains("public domain") {
            f.push("public domain");
        } else if t.contains("copyright") {
            f.push("unclassified (copyright without a recognised licence)");
        } else {
            f.push("no licence text");
        }
    }
    f
}

/// Classifies every source and header the build read and writes `licences.txt`.
fn licence_report(ctx: &Ctx<'_>) -> Result<()> {
    let installed = ctx.installed.lock().map_err(|_| "lock poisoned")?.clone();
    let inputs = ctx.inputs.lock().map_err(|_| "lock poisoned")?.clone();
    let mut by_file: BTreeMap<PathBuf, String> = BTreeMap::new();
    for p in inputs {
        // Generated files (stubs come from stdin, helpers from hash/helper.c) are not
        // classified themselves; their inputs are.
        if p.starts_with(&ctx.out) && !p.starts_with(&ctx.sysroot) {
            continue;
        }
        let real = installed.get(&p).cloned().unwrap_or(p);
        // `-I${.CURDIR}/../../lib/libc/gen` gives paths with `..`.
        let real = fs::canonicalize(&real).unwrap_or(real);
        if !real.starts_with(&ctx.src) {
            continue; // clang's own headers (stddef.h, ...), outside the OpenBSD tree
        }
        let Ok(text) = fs::read(&real) else {
            continue;
        };
        let mut fam = licence_families(&String::from_utf8_lossy(&text)).join(" + ");
        if fam == "no licence text" {
            // pdksh states its licence once, in bin/ksh/LEGAL.
            let legal = real.with_file_name("LEGAL");
            if let Ok(t) = fs::read_to_string(&legal) {
                let rel = legal.strip_prefix(&ctx.src).unwrap_or(&legal);
                fam = format!(
                    "{} (per {})",
                    licence_families(&t).join(" + "),
                    rel.display()
                );
            }
        }
        let rel = real.strip_prefix(&ctx.src).unwrap_or(&real).to_path_buf();
        by_file.insert(rel, fam);
    }
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for f in by_file.values() {
        *counts.entry(f.as_str()).or_insert(0) += 1;
    }
    let path = ctx.out.join("licences.txt");
    let mut file = fs::File::create(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    for (p, f) in &by_file {
        writeln!(file, "{f}\t{}", p.display())?;
    }
    println!(
        "  licences of the {} OpenBSD files compiled or included ({}):",
        by_file.len(),
        path.display()
    );
    for (f, n) in &counts {
        println!("    {n:5}  {f}");
    }
    // The families the user has accepted (`.claude/rules/scope-and-stubs.md`); the userland
    // ones (Apache-2.0 WITH LLVM-exception, public domain, no licence text, Lucent,
    // Birgmeier, Unicode, SunPro, Cheusov, Boulet/RTMX) only for code compiled unmodified,
    // decided 2026-10-03.
    let usual = [
        "ISC",
        "BSD-2-Clause",
        "BSD-3-Clause",
        "BSD-4-Clause",
        "MIT",
        "Mach (CMU)",
        "beerware",
        "public domain",
        "no licence text",
        "Apache-2.0 WITH LLVM-exception",
        "Lucent (gdtoa)",
        "Birgmeier (rand48)",
        "Unicode (data files and software)",
        "SunPro (fdlibm)",
        "Cheusov",
        "Boulet/RTMX",
    ];
    let unusual: Vec<_> = by_file
        .iter()
        .filter(|(_, f)| {
            let f = f.split(" (per ").next().unwrap_or(f);
            !f.split(" + ").all(|x| usual.contains(&x))
        })
        .collect();
    if !unusual.is_empty() {
        println!("  files outside the accepted licences (review them; nothing is decided here):");
        for (p, f) in unusual {
            println!("    {f}: {}", p.display());
        }
    }
    Ok(())
}

mod ramdisk;

#[cfg(test)]
mod tests;
