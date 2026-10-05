//! Scenario files, the shell scripts made from them, the transcripts they leave, and the
//! comparison of two transcripts (`diff-openbsd`).
//!
//! A scenario file (`tools/xtask/diff-openbsd/<set>.scn`) is a list of one-line shell
//! commands, run in order by `sh` on both systems:
//!
//! ```text
//! # a comment
//! ## rename                     a scenario: the steps below are rename#1, rename#2, ...
//! ! mkdir /mnt/t                setup: run, nothing compared
//! ? newfs ${D}a                 the exit status is compared
//! $ ls -1 /mnt/t                the output (stdout and stderr) and the exit status
//! $[dates,inodes] ls -li        the same, after these extra normalizers
//! ```
//!
//! A step's id is `<set>/<scenario>#<n>`, n counting every step of the scenario from 1.
//! `$D` is the scratch disk (`sd0` on EmiBSD, `sd1` on OpenBSD; [`script`]'s preamble).

use std::collections::{BTreeMap, HashMap};

use serde::Deserialize;

use crate::Result;

/// What is compared for a step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Check {
    /// `!`: nothing.
    Setup,
    /// `?`: the exit status.
    Status,
    /// `$`: the output and the exit status.
    Output,
}

/// An extra normalizer a `$[..]` step asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Norm {
    /// `Mmm dd HH:MM` and `Mmm dd  YYYY` (ls -l) → `<date>`.
    Dates,
    /// The first number of every line (ls -i) → `#k`, in order of first appearance.
    Inodes,
    /// Every run of digits → `N`.
    Numbers,
}

/// One step of a scenario file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Step {
    /// `<set>/<scenario>#<n>`.
    pub(crate) id: String,
    pub(crate) check: Check,
    pub(crate) norms: Vec<Norm>,
    pub(crate) cmd: String,
}

/// Parses scenario file `text` of set `set`.
pub(crate) fn parse(set: &str, text: &str) -> Result<Vec<Step>> {
    let mut steps = Vec::new();
    let mut scenario: Option<String> = None;
    let mut n = 0;
    for (lineno, line) in text.lines().enumerate() {
        let where_ = || format!("{set}.scn:{}", lineno + 1);
        let line = line.trim_end();
        if line.is_empty() || (line.starts_with('#') && !line.starts_with("##")) {
            continue;
        }
        if let Some(name) = line.strip_prefix("##") {
            let name = name.trim();
            if name.is_empty() || name.contains(['#', '/', ' ']) {
                return Err(format!("{}: bad scenario name {name:?}", where_()).into());
            }
            scenario = Some(name.to_string());
            n = 0;
            continue;
        }
        let Some(sc) = scenario.as_deref() else {
            return Err(format!("{}: a step before the first `## name`", where_()).into());
        };
        let (check, rest) = match line.split_at(1) {
            ("!", r) => (Check::Setup, r),
            ("?", r) => (Check::Status, r),
            ("$", r) => (Check::Output, r),
            _ => return Err(format!("{}: a step starts with !, ? or $", where_()).into()),
        };
        let mut norms = Vec::new();
        let rest = if let Some(r) = rest.strip_prefix('[') {
            let (list, after) = r
                .split_once(']')
                .ok_or_else(|| format!("{}: unclosed [", where_()))?;
            for w in list.split(',') {
                norms.push(match w.trim() {
                    "dates" => Norm::Dates,
                    "inodes" => Norm::Inodes,
                    "numbers" => Norm::Numbers,
                    other => {
                        return Err(format!("{}: unknown normalizer {other:?}", where_()).into());
                    }
                });
            }
            after
        } else {
            rest
        };
        let cmd = rest.trim();
        if cmd.is_empty() {
            return Err(format!("{}: empty command", where_()).into());
        }
        n += 1;
        steps.push(Step {
            id: format!("{set}/{sc}#{n}"),
            check,
            norms,
            cmd: cmd.to_string(),
        });
    }
    Ok(steps)
}

/// The `sh` script that runs `steps` after `preamble` (the per-system variables): every step
/// between `@@B <k>` and `@@E <k> <status>` lines, k counting from 1, stdin from /dev/null,
/// stderr into stdout; `@@HOST <name>` first and `@@DONE` last.
pub(crate) fn script(preamble: &str, steps: &[Step]) -> String {
    let mut s = String::from(preamble);
    if !s.ends_with('\n') {
        s.push('\n');
    }
    s.push_str("echo \"@@HOST $(hostname)\"\n");
    for (k, step) in steps.iter().enumerate() {
        let k = k + 1;
        s.push_str(&format!(
            "echo \"@@B {k}\"\n{{ {}\n}} </dev/null 2>&1\necho \"@@E {k} $?\"\n",
            step.cmd
        ));
    }
    s.push_str("echo \"@@DONE\"\n");
    s
}

/// What one system printed for one step.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Outcome {
    pub(crate) lines: Vec<String>,
    pub(crate) status: Option<i32>,
}

/// A transcript, cut into steps: `(host name, outcome of step k at index k - 1)`. Steps whose
/// markers are missing get an empty outcome without a status.
pub(crate) fn outcomes(transcript: &str, nsteps: usize) -> (String, Vec<Outcome>) {
    let mut host = String::new();
    let mut out = vec![Outcome::default(); nsteps];
    let mut current: Option<usize> = None;
    for raw in transcript.lines() {
        let line = raw.trim_end_matches('\r');
        if let Some(h) = line.strip_prefix("@@HOST ") {
            host = h.trim().to_string();
            continue;
        }
        if let Some(k) = line
            .strip_prefix("@@B ")
            .and_then(|k| k.trim().parse::<usize>().ok())
        {
            if (1..=nsteps).contains(&k) {
                current = Some(k - 1);
                out[k - 1] = Outcome::default();
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("@@E ") {
            let mut w = rest.split_whitespace();
            if let (Some(k), Some(st)) = (w.next(), w.next())
                && let (Ok(k), Ok(st)) = (k.parse::<usize>(), st.parse::<i32>())
                && current == Some(k.wrapping_sub(1))
            {
                out[k - 1].status = Some(st);
                current = None;
                continue;
            }
        }
        if let Some(i) = current {
            out[i].lines.push(line.trim_end().to_string());
        }
    }
    (host, out)
}

/// Per-system replacements applied to every output line before comparing.
pub(crate) struct Subst {
    /// The system's host name → `<host>`.
    pub(crate) host: String,
    /// The scratch disk's name (`sd0`, `sd1`) → `sdX`.
    pub(crate) disk: String,
}

/// `lines` with the global normalizers (host name, scratch disk, `name[pid]`) and then
/// `norms` applied.
pub(crate) fn normalize(lines: &[String], subst: &Subst, norms: &[Norm]) -> Vec<String> {
    let mut labels: HashMap<String, usize> = HashMap::new();
    lines
        .iter()
        .map(|l| {
            let mut l = l.clone();
            if !subst.host.is_empty() {
                l = l.replace(&subst.host, "<host>");
            }
            if !subst.disk.is_empty() {
                l = l.replace(&subst.disk, "sdX");
            }
            l = pids(&l);
            for n in norms {
                l = match n {
                    Norm::Dates => dates(&l),
                    Norm::Inodes => inode(&l, &mut labels),
                    Norm::Numbers => numbers(&l),
                };
            }
            l
        })
        .collect()
}

/// `word[123]` → `word[PID]` (the kernel's `prog[pid]: ...` console lines).
fn pids(l: &str) -> String {
    let b = l.as_bytes();
    let mut out = String::with_capacity(l.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'[' && i > 0 && (b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_') {
            let mut j = i + 1;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            if j > i + 1 && j < b.len() && b[j] == b']' {
                out.push_str("[PID]");
                i = j + 1;
                continue;
            }
        }
        let ch = l[i..].chars().next().unwrap_or(' ');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// ls(1)'s `Mmm dd HH:MM` / `Mmm dd  YYYY` → `<date>`.
fn dates(l: &str) -> String {
    let b = l.as_bytes();
    let mut out = String::with_capacity(l.len());
    let mut i = 0;
    while i < b.len() {
        if (i == 0 || b[i - 1] == b' ')
            && let Some(len) = date_at(&b[i..])
        {
            out.push_str("<date>");
            i += len;
            continue;
        }
        let ch = l[i..].chars().next().unwrap_or(' ');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// The length of a date at the start of `b`, if one is there.
fn date_at(b: &[u8]) -> Option<usize> {
    if b.len() < 3 || !MONTHS.iter().any(|m| m.as_bytes() == &b[..3]) {
        return None;
    }
    let mut i = 3;
    let spaces = |i: &mut usize| {
        let s = *i;
        while *i < b.len() && b[*i] == b' ' {
            *i += 1;
        }
        *i > s
    };
    let digits = |i: &mut usize| {
        let s = *i;
        while *i < b.len() && b[*i].is_ascii_digit() {
            *i += 1;
        }
        *i - s
    };
    if !spaces(&mut i) || !(1..=2).contains(&digits(&mut i)) || !spaces(&mut i) {
        return None;
    }
    let s = i;
    if digits(&mut i) == 2 && i < b.len() && b[i] == b':' {
        i += 1;
        if digits(&mut i) == 2 {
            return Some(i);
        }
        return None;
    }
    i = s;
    (digits(&mut i) == 4).then_some(i)
}

/// The first number of the line → `#k`, k by first appearance in `labels`.
fn inode(l: &str, labels: &mut HashMap<String, usize>) -> String {
    let start = l.len() - l.trim_start().len();
    let digits = l[start..].bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return l.to_string();
    }
    let num = &l[start..start + digits];
    let next = labels.len() + 1;
    let k = *labels.entry(num.to_string()).or_insert(next);
    format!("{}#{k}{}", &l[..start], &l[start + digits..])
}

/// Every run of digits → `N`.
fn numbers(l: &str) -> String {
    let mut out = String::with_capacity(l.len());
    let mut in_num = false;
    for c in l.chars() {
        if c.is_ascii_digit() {
            if !in_num {
                out.push('N');
            }
            in_num = true;
        } else {
            in_num = false;
            out.push(c);
        }
    }
    out
}

/// One entry of the expected-differences file.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Expected {
    /// The step id.
    pub(crate) step: String,
    /// The step's command, as in the scenario file (an entry is stale when it changed).
    pub(crate) cmd: String,
    /// `amd64` or `arm64`; both when absent.
    #[serde(default)]
    pub(crate) arch: Option<String>,
    /// Why EmiBSD does not (or should not) behave as OpenBSD here.
    pub(crate) reason: String,
}

/// The expected-differences file.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExpectedFile {
    #[serde(default, rename = "difference")]
    pub(crate) differences: Vec<Expected>,
}

/// The result of comparing one arch's two runs.
#[derive(Debug, Default)]
pub(crate) struct Report {
    /// Steps whose outcome was compared (not setup steps).
    pub(crate) compared: usize,
    /// Of those, the ones that were equal.
    pub(crate) equal: usize,
    /// Differences listed in the expected file: `(step id, reason)`.
    pub(crate) expected: Vec<(String, String)>,
    /// Differences that are not: the text to show.
    pub(crate) unexpected: Vec<String>,
    /// Expected-file entries that did not match a difference this run.
    pub(crate) stale: Vec<String>,
}

impl Report {
    pub(crate) fn passed(&self) -> bool {
        self.unexpected.is_empty() && self.stale.is_empty()
    }
}

/// Compares the outcomes of `steps` on OpenBSD (`obsd`) and EmiBSD (`emi`) for `arch`.
pub(crate) fn compare(
    arch: &str,
    steps: &[Step],
    obsd: (&Subst, &[Outcome]),
    emi: (&Subst, &[Outcome]),
    expected: &[Expected],
) -> Report {
    let mut r = Report::default();
    let mut used: BTreeMap<usize, bool> = BTreeMap::new();
    let applies = |e: &Expected| e.arch.as_deref().is_none_or(|a| a == arch);
    for (i, step) in steps.iter().enumerate() {
        if step.check == Check::Setup {
            continue;
        }
        r.compared += 1;
        let (os, oo) = (obsd.0, &obsd.1[i]);
        let (es, eo) = (emi.0, &emi.1[i]);
        let ol = normalize(&oo.lines, os, &step.norms);
        let el = normalize(&eo.lines, es, &step.norms);
        let same_status = oo.status == eo.status && oo.status.is_some();
        let same = same_status && (step.check == Check::Status || ol == el);
        if same {
            r.equal += 1;
            continue;
        }
        let entry = expected
            .iter()
            .position(|e| e.step == step.id && applies(e));
        if let Some(k) = entry {
            used.insert(k, true);
            if expected[k].cmd == step.cmd {
                r.expected
                    .push((step.id.clone(), expected[k].reason.clone()));
                continue;
            }
            r.stale.push(format!(
                "{}: the expected-differences entry names the command {:?}, the scenario now \
                 runs {:?}",
                step.id, expected[k].cmd, step.cmd
            ));
        }
        let mut text = format!("{} ({}): $ {}\n", step.id, arch, step.cmd);
        let st = |s: Option<i32>| s.map_or("none (no end marker)".to_string(), |s| s.to_string());
        text.push_str(&format!(
            "  status: openbsd {}, emibsd {}\n",
            st(oo.status),
            st(eo.status)
        ));
        if step.check == Check::Output && ol != el {
            text.push_str(&side_by_side(&ol, &el));
        }
        r.unexpected.push(text);
    }
    for (k, e) in expected.iter().enumerate() {
        if applies(e) && !used.contains_key(&k) && steps.iter().any(|s| s.id == e.step) {
            r.stale.push(format!(
                "{} ({arch}): listed as an expected difference, but both systems agree now",
                e.step
            ));
        }
    }
    r
}

/// The lines that differ, OpenBSD's then EmiBSD's, with the common ones as context.
fn side_by_side(o: &[String], e: &[String]) -> String {
    let mut s = String::new();
    let n = o.len().max(e.len());
    for i in 0..n {
        match (o.get(i), e.get(i)) {
            (Some(a), Some(b)) if a == b => s.push_str(&format!("    = {a}\n")),
            (a, b) => {
                if let Some(a) = a {
                    s.push_str(&format!("    openbsd: {a}\n"));
                }
                if let Some(b) = b {
                    s.push_str(&format!("    emibsd:  {b}\n"));
                }
            }
        }
    }
    s
}
