//! `xtask`: host-side developer tooling for openbsd-rs.
//!
//! Invoked through the cargo alias in `.cargo/config.toml`:
//!
//! ```text
//! cargo xtask ports check                  validate ports.toml against the tree and the pin
//! cargo xtask ports status [--write]       counts per subsystem; --write regenerates docs/PORTING.md
//! cargo xtask ports next                   `todo` entries whose dependencies are all ported
//! cargo xtask ports drift [--strict|--diff] ported files whose upstream content changed
//! cargo xtask image | qemu | smoke         boot image and QEMU drivers (milestone M0)
//! ```
//!
//! Paths are resolved from the workspace root (derived from `CARGO_MANIFEST_DIR`), never from the
//! current directory.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use serde::Deserialize;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Placeholder hash used before the reference tree has been cloned and pinned.
const UNPINNED: &str = "UNPINNED";
const PORTS_FILE: &str = "ports.toml";
const PINNED_FILE: &str = "reference/PINNED.md";
const REFERENCE_DIR: &str = "reference/openbsd-src";
const PORTING_DOC: &str = "docs/PORTING.md";
const TABLE_BEGIN: &str = "<!-- ports:begin -->";
const TABLE_END: &str = "<!-- ports:end -->";

const USAGE: &str = "usage: cargo xtask <ports check | ports status [--write] | ports next | \
                     ports drift [--strict] [--diff] | image | qemu | smoke>";

#[derive(Deserialize)]
struct Ports {
    meta: Meta,
    #[serde(default, rename = "file")]
    files: Vec<Entry>,
    #[serde(default, rename = "extra")]
    extras: Vec<Extra>,
}

#[derive(Deserialize)]
struct Meta {
    upstream: String,
    pinned: String,
}

#[derive(Deserialize)]
struct Entry {
    c: String,
    #[serde(default)]
    rust: String,
    status: Status,
    #[serde(default)]
    upstream_commit: String,
    #[serde(default)]
    upstream_blob: String,
    #[serde(default)]
    deps: Vec<String>,
    #[serde(default)]
    notes: String,
}

/// A Rust file under `sys/` that is not the port of any C file (project helpers).
#[derive(Deserialize)]
struct Extra {
    rust: String,
    reason: String,
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[serde(rename_all = "lowercase")]
enum Status {
    Todo,
    Wip,
    Ported,
    Skipped,
}

impl Status {
    const ALL: [Status; 4] = [Status::Todo, Status::Wip, Status::Ported, Status::Skipped];

    fn as_str(self) -> &'static str {
        match self {
            Status::Todo => "todo",
            Status::Wip => "wip",
            Status::Ported => "ported",
            Status::Skipped => "skipped",
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xtask: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<()> {
    let root = workspace_root()?;
    let argv: Vec<&str> = args.iter().map(String::as_str).collect();
    match argv.as_slice() {
        ["ports", "check"] => ports_check(&root),
        ["ports", "status"] => ports_status(&root, false),
        ["ports", "status", "--write"] => ports_status(&root, true),
        ["ports", "next"] => ports_next(&root),
        ["ports", "drift", flags @ ..] => ports_drift(
            &root,
            flags.contains(&"--strict"),
            flags.contains(&"--diff"),
        ),
        [cmd @ ("image" | "qemu" | "smoke"), ..] => Err(format!(
            "`xtask {cmd}` arrives with milestone M0 (docs/ROADMAP.md); nothing to boot yet"
        )
        .into()),
        _ => Err(USAGE.into()),
    }
}

/// `tools/xtask` → workspace root. Compile-time manifest dir, so the cwd never matters.
fn workspace_root() -> Result<PathBuf> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest
        .parent()
        .and_then(Path::parent)
        .ok_or("cannot locate the workspace root from CARGO_MANIFEST_DIR")?;
    Ok(root.to_path_buf())
}

fn load_ports(root: &Path) -> Result<Ports> {
    let path = root.join(PORTS_FILE);
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let ports: Ports = toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(ports)
}

/// The `Commit:` line of `reference/PINNED.md`.
fn pinned_commit(root: &Path) -> Result<String> {
    let path = root.join(PINNED_FILE);
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    text.lines()
        .find_map(|l| l.strip_prefix("Commit:"))
        .map(|s| s.trim().to_string())
        .ok_or_else(|| format!("{}: no `Commit:` line", path.display()).into())
}

fn short(hash: &str) -> &str {
    hash.get(..12).unwrap_or(hash)
}

fn is_rust_path_under_sys(p: &str) -> bool {
    p.starts_with("sys/") && p.ends_with(".rs")
}

/// Files that are structure, not ports: never need a `ports.toml` entry.
fn is_structural(rel: &str) -> bool {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    matches!(name, "mod.rs" | "lib.rs" | "main.rs" | "build.rs")
        || rel.starts_with("sys/machine/")
        || rel.starts_with("sys/arch/host/")
        || rel.starts_with("sys/stand/")
}

fn walk_rs(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let path = entry?.path();
        if path.is_dir() {
            if path.file_name().and_then(|n| n.to_str()) == Some("target") {
                continue;
            }
            walk_rs(&path, out)?;
        } else if path.extension().and_then(|x| x.to_str()) == Some("rs") {
            out.push(path);
        }
    }
    Ok(())
}

/// Second-level grouping used by `ports status`: `sys/kern/x.c` → `kern`,
/// `sys/lib/libkern/x.c` → `lib/libkern`, `sys/arch/amd64/...` → `arch/amd64`.
fn subsystem(c: &str) -> String {
    let parts: Vec<&str> = c.strip_prefix("sys/").unwrap_or(c).split('/').collect();
    match parts.as_slice() {
        [first, second, _, ..] if *first == "lib" || *first == "arch" => {
            format!("{first}/{second}")
        }
        [first, _, ..] => (*first).to_string(),
        _ => "(root)".to_string(),
    }
}

fn ports_check(root: &Path) -> Result<()> {
    let ports = load_ports(root)?;
    let pinned = pinned_commit(root)?;
    let reference = root.join(REFERENCE_DIR);
    let have_reference = reference.join("sys").is_dir();
    let mut errors: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    if ports.meta.pinned == UNPINNED || pinned == UNPINNED {
        warnings.push(
            "reference not pinned yet: clone it (reference/README.md), then set `Commit:` in \
             reference/PINNED.md and [meta].pinned in ports.toml"
                .to_string(),
        );
    }
    if ports.meta.pinned != pinned {
        let meta_pinned = &ports.meta.pinned;
        errors.push(format!(
            "[meta].pinned ({meta_pinned}) != reference/PINNED.md Commit: ({pinned})"
        ));
    }
    if !have_reference {
        warnings.push(format!(
            "{REFERENCE_DIR} not present; C paths were not verified"
        ));
    }

    let mut seen_c: HashSet<&str> = HashSet::new();
    for e in &ports.files {
        let c = &e.c;
        let tag = format!("[[file]] c = \"{c}\"");
        if !seen_c.insert(c.as_str()) {
            errors.push(format!("{tag}: duplicate entry"));
        }
        if !c.starts_with("sys/") {
            errors.push(format!("{tag}: `c` must start with sys/"));
        }
        if have_reference && !reference.join(c).is_file() {
            errors.push(format!("{tag}: not found in {REFERENCE_DIR}"));
        }
        let rust_required = e.status != Status::Skipped;
        if (rust_required || !e.rust.is_empty()) && !is_rust_path_under_sys(&e.rust) {
            errors.push(format!("{tag}: `rust` must be a .rs path under sys/"));
        }
        match e.status {
            Status::Todo => {}
            Status::Wip | Status::Ported => {
                let status = e.status.as_str();
                let rust = &e.rust;
                match fs::read_to_string(root.join(rust)) {
                    Ok(src) if !src.contains("Upstream:") => {
                        errors.push(format!("{tag}: {rust} lacks an `Upstream:` line"));
                    }
                    Ok(_) => {}
                    Err(_) => {
                        errors.push(format!("{tag}: status {status} but {rust} does not exist"))
                    }
                }
                if e.upstream_commit.is_empty() {
                    errors.push(format!("{tag}: upstream_commit is required for {status}"));
                }
                if e.status == Status::Ported && e.upstream_blob.is_empty() {
                    errors.push(format!("{tag}: upstream_blob is required for ported"));
                }
            }
            Status::Skipped => {
                if e.notes.trim().is_empty() {
                    errors.push(format!("{tag}: skipped entries need `notes` (the reason)"));
                }
            }
        }
        for d in &e.deps {
            if !ports.files.iter().any(|o| &o.c == d) {
                errors.push(format!("{tag}: dep {d} has no entry"));
            }
        }
    }

    // Every non-structural Rust file under sys/ is a tracked port or a declared extra.
    let tracked: HashSet<&str> = ports
        .files
        .iter()
        .map(|e| e.rust.as_str())
        .chain(ports.extras.iter().map(|x| x.rust.as_str()))
        .collect();
    let mut rs_files = Vec::new();
    walk_rs(&root.join("sys"), &mut rs_files)?;
    for f in &rs_files {
        let rel = f
            .strip_prefix(root)
            .unwrap_or(f)
            .to_string_lossy()
            .replace('\\', "/");
        if is_structural(&rel) || tracked.contains(rel.as_str()) {
            continue;
        }
        errors.push(format!(
            "{rel}: not tracked in ports.toml (add a [[file]] entry, or an [[extra]] with a reason)"
        ));
    }
    for x in &ports.extras {
        let rust = &x.rust;
        if !root.join(rust).is_file() {
            errors.push(format!("[[extra]] rust = \"{rust}\": file does not exist"));
        }
        if x.reason.trim().is_empty() {
            errors.push(format!("[[extra]] rust = \"{rust}\": `reason` is required"));
        }
    }

    for w in &warnings {
        println!("warning: {w}");
    }
    for e in &errors {
        println!("error: {e}");
    }
    let (nfiles, nextras, nerr, nwarn) = (
        ports.files.len(),
        ports.extras.len(),
        errors.len(),
        warnings.len(),
    );
    println!(
        "ports check: {nfiles} entries, {nextras} extras, {nerr} error(s), {nwarn} warning(s)"
    );
    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!("{nerr} error(s) in {PORTS_FILE}").into())
    }
}

fn ports_status(root: &Path, write: bool) -> Result<()> {
    let ports = load_ports(root)?;
    let mut table: BTreeMap<String, BTreeMap<Status, usize>> = BTreeMap::new();
    for e in &ports.files {
        *table
            .entry(subsystem(&e.c))
            .or_default()
            .entry(e.status)
            .or_insert(0) += 1;
    }

    let mut md = String::new();
    md.push_str("| Subsystem | todo | wip | ported | skipped | total |\n");
    md.push_str("|---|---:|---:|---:|---:|---:|\n");
    let mut totals: BTreeMap<Status, usize> = BTreeMap::new();
    for (sub, counts) in &table {
        let mut row = format!("| {sub} |");
        for s in Status::ALL {
            let n = counts.get(&s).copied().unwrap_or(0);
            *totals.entry(s).or_insert(0) += n;
            row.push_str(&format!(" {n} |"));
        }
        let total: usize = counts.values().sum();
        row.push_str(&format!(" {total} |\n"));
        md.push_str(&row);
    }
    let mut row = String::from("| **total** |");
    for s in Status::ALL {
        let n = totals.get(&s).copied().unwrap_or(0);
        row.push_str(&format!(" {n} |"));
    }
    let grand = ports.files.len();
    row.push_str(&format!(" {grand} |\n"));
    md.push_str(&row);

    let upstream = &ports.meta.upstream;
    let pin = short(&ports.meta.pinned);
    println!("upstream: {upstream} @ {pin}");
    print!("{md}");

    if write {
        let path = root.join(PORTING_DOC);
        let doc = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let (Some(b), Some(e)) = (doc.find(TABLE_BEGIN), doc.find(TABLE_END)) else {
            return Err(
                format!("{PORTING_DOC}: markers {TABLE_BEGIN} / {TABLE_END} not found").into(),
            );
        };
        if e < b {
            return Err(format!("{PORTING_DOC}: {TABLE_END} appears before {TABLE_BEGIN}").into());
        }
        let head = &doc[..b];
        let tail = &doc[e..];
        let new = format!(
            "{head}{TABLE_BEGIN}\n_Generated by `cargo xtask ports status --write` against pin {pin}._\n\n{md}{tail}"
        );
        fs::write(&path, new).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("wrote {PORTING_DOC}");
    }
    Ok(())
}

fn ports_next(root: &Path) -> Result<()> {
    let ports = load_ports(root)?;
    let status: HashMap<&str, Status> = ports
        .files
        .iter()
        .map(|e| (e.c.as_str(), e.status))
        .collect();
    let satisfied = |dep: &str| {
        matches!(
            status.get(dep),
            Some(Status::Ported) | Some(Status::Skipped)
        )
    };
    let mut any = false;
    for e in ports.files.iter().filter(|e| e.status == Status::Todo) {
        if e.deps.iter().all(|d| satisfied(d)) {
            let (c, rust) = (&e.c, &e.rust);
            println!("{c}  ->  {rust}");
            any = true;
        }
    }
    if !any {
        println!("nothing unblocked: no `todo` entry has all its dependencies ported or skipped");
    }
    Ok(())
}

fn ports_drift(root: &Path, strict: bool, diff: bool) -> Result<()> {
    let ports = load_ports(root)?;
    let reference = root.join(REFERENCE_DIR);
    if !reference.join("sys").is_dir() {
        return Err(format!("{REFERENCE_DIR} not present; see reference/README.md").into());
    }
    let mut checked = 0usize;
    let mut drifted = 0usize;
    for e in ports
        .files
        .iter()
        .filter(|e| matches!(e.status, Status::Ported | Status::Wip) && !e.upstream_blob.is_empty())
    {
        checked += 1;
        let c = &e.c;
        let now = git(&reference, &["rev-parse", &format!("HEAD:{c}")])?;
        if now == e.upstream_blob {
            continue;
        }
        drifted += 1;
        let (old, new) = (short(&e.upstream_blob), short(&now));
        println!("DRIFT {c}  {old}..{new}");
        if diff {
            let commit = &e.upstream_commit;
            match git(&reference, &["diff", commit, "HEAD", "--", c]) {
                Ok(d) => println!("{d}"),
                Err(err) => println!(
                    "  (diff unavailable: {err}; fetch the old commit first: \
                     git -C {REFERENCE_DIR} fetch --depth 1 origin {commit})"
                ),
            }
        }
    }
    let pin = short(&ports.meta.pinned);
    println!("ports drift: {checked} checked, {drifted} drifted (pin {pin})");
    if strict && drifted > 0 {
        Err(format!("{drifted} ported file(s) drifted upstream").into())
    } else {
        Ok(())
    }
}

fn git(repo: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        let cmd = args.join(" ");
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(format!("git {cmd}: {stderr}").into());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}
