#!/usr/bin/env python3
#
# Copyright (c) 2026 Emilio Navarrete Lineros <enavarre@outlook.com>
#
# Permission to use, copy, modify, and distribute this software for any
# purpose with or without fee is hereby granted, provided that the above
# copyright notice and this permission notice appear in all copies.
#
# THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
# WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
# MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
# ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
# WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
# ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
# OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
#
"""Redesign progress of EmiBSD, one row per milestone of docs/ROADMAP.md.

Adapted from EmiBSD.LZ's .claude/skills/progress/scripts/progress.py (b0901dd3), which
measured the port from ports.toml; this one measures the redesign from lineage.toml.

Sources, all read live (nothing is typed in by hand):
  - docs/ROADMAP.md: the milestone table (id, title, scope, exit criterion);
  - lineage.toml: every [[module]] (rust, lz, status) and [[extra]]. A module
    belongs to the N row whose scope cell names its path in backticks: a
    directory (`sys/uvm/`), a file (`sys/sys/queue.rs`) or a glob
    (`sys/kern/vfs_*`); when several rows name it, the most specific pattern
    wins. A module no row names is listed apart as "(no milestone)". Extras
    (no LZ source) count nowhere. done = redesigned (adapted shown apart: their
    call sites followed a redesign, nothing of their own changed); left =
    inherited;
  - reference/emibsd-lz at the pin: the lines of each module's LZ sources
    (`git cat-file --batch`), each LZ file once per row. Without the clone the
    native file stands in for an inherited module (it is byte-identical modulo
    the RCS ident lines) and a redesigned module's sources count as unknown;
  - the native .rs files: `wc -l`, tests included;
  - git: the `docs: Nx met` close commits after the pin, and the active days
    (distinct commit dates after lz-origin) that give the rate for the estimate.

The estimate is a linear extrapolation at the project's average rate of LZ
lines redesigned per active day. It measures the past; it promises nothing.

Usage: progress.py [MILESTONE]   (e.g. N2: adds that milestone's module list)
Always exits 0; errors are printed, so a skill preamble never aborts on them.
"""

import datetime as _dt
import fnmatch
import os
import re
import subprocess
import sys
from pathlib import Path

try:
    import tomllib
except ModuleNotFoundError:  # macOS's /usr/bin/python3 is older than 3.11
    print(f"progress: Python 3.11+ is needed (tomllib); this is {sys.version.split()[0]}")
    sys.exit(0)

MS_ID = re.compile(r"N\d+[a-z]*")
STATUSES = ("redesigned", "adapted", "inherited")
DONE = ("redesigned", "adapted")


def root_dir() -> Path:
    env = os.environ.get("CLAUDE_PROJECT_DIR")
    if env and (Path(env) / "lineage.toml").is_file():
        return Path(env)
    for p in Path(__file__).resolve().parents:
        if (p / "lineage.toml").is_file() and (p / "docs" / "ROADMAP.md").is_file():
            return p
    raise SystemExit("progress: cannot find the repository root (lineage.toml)")


def count_lines(path: Path, cache: dict) -> int | None:
    if path in cache:
        return cache[path]
    try:
        with open(path, "rb") as f:
            n = f.read().count(b"\n")
    except OSError:
        n = None
    cache[path] = n
    return n


# --- ROADMAP ---------------------------------------------------------------

def parse_roadmap(root: Path) -> list[dict]:
    """The milestone table: [{id, title, scope, criterion, row_date}] in file order."""
    rows = []
    for line in (root / "docs" / "ROADMAP.md").read_text().splitlines():
        if not line.startswith("| **N"):
            continue
        cells = [c.strip() for c in re.split(r"(?<!\\)\|", line)][1:-1]
        if len(cells) < 3:
            continue
        m = re.match(r"\*\*(N\d+[a-z]*)\s*(.*?)\*\*", cells[0])
        if not m:
            continue
        dm = re.search(r"\b(?:done|Met|met)\**\s+(\d{4}-\d{2}-\d{2})", line)
        rows.append({"id": m.group(1), "title": m.group(2).strip(), "scope": cells[1],
                     "criterion": cells[2], "row_date": dm.group(1) if dm else None})
    return rows


def scope_patterns(scope: str) -> list[str]:
    """The `sys/...` paths a scope cell names in backticks: a directory, a file or a glob."""
    pats = []
    for tok in re.findall(r"`([^`]+)`", scope):
        tok = tok.strip()
        if tok.startswith("sys/") and " " not in tok and "{" not in tok:
            pats.append(tok)
    return pats


def pattern_matches(pattern: str, path: str) -> bool:
    if any(ch in pattern for ch in "*?["):
        return fnmatch.fnmatchcase(path, pattern)
    if pattern.endswith("/"):
        return path.startswith(pattern)
    if pattern.endswith(".rs"):
        return path == pattern
    return path == pattern or path.startswith(pattern + "/")


def specificity(pattern: str) -> int:
    """The literal prefix length: `sys/kern/vfs_*` beats `sys/kern/`."""
    m = re.search(r"[*?\[]", pattern)
    return len(pattern) if m is None else m.start()


# --- lineage.toml ------------------------------------------------------------

def parse_lineage(root: Path) -> tuple[dict, list[dict], list[dict]]:
    data = tomllib.loads((root / "lineage.toml").read_text())
    return data.get("meta", {}), data.get("module", []), data.get("extra", [])


# --- git ---------------------------------------------------------------------

def git(root: Path, *args: str) -> str:
    try:
        return subprocess.run(["git", "-C", str(root), *args], check=True,
                              capture_output=True, text=True).stdout
    except (OSError, subprocess.CalledProcessError) as e:
        print(f"progress: git {' '.join(args)} failed: {e}")
        return ""


def close_dates(root: Path, rng: str) -> dict[str, str]:
    """Milestone id -> the date it was declared met (`docs: Nx met ...` in the range)."""
    dates: dict[str, str] = {}
    for line in reversed(git(root, "log", rng, "--format=%cd%x09%s", "--date=short").splitlines()):
        date, _, subject = line.partition("\t")
        if not subject.startswith("docs:") or not re.search(r"\bmet\b", subject):
            continue
        if re.search(r"\b(left|blocked|partial|pending)\b", subject):
            continue  # a partial criterion, not the close
        for mid in MS_ID.findall(subject.split("(")[0]):
            dates.setdefault(mid, date)
    return dates


def under_way(root: Path) -> set[str]:
    status = root / "docs" / "STATUS.md"
    if not status.is_file():
        return set()
    return {m.group(1) for m in
            re.finditer(r"(N\d+[a-z]*)\b[^.;]*?\bunder way", status.read_text())}


def lz_lines(root: Path, pin: str, paths: list[str]) -> dict[str, int | None] | None:
    """LZ path -> lines of `<pin>:<path>` in reference/emibsd-lz; None when the clone is absent."""
    clone = root / "reference" / "emibsd-lz"
    if not pin or not paths or not clone.is_dir():
        return None
    req = "".join(f"{pin}:{p}\n" for p in paths).encode()
    try:
        r = subprocess.run(["git", "-C", str(clone), "cat-file", "--batch"], input=req,
                           capture_output=True, check=True)
    except (OSError, subprocess.CalledProcessError) as e:
        print(f"progress: git cat-file in reference/emibsd-lz failed: {e}")
        return None
    out: dict[str, int | None] = {}
    data, pos = r.stdout, 0
    for p in paths:
        nl = data.find(b"\n", pos)
        if nl < 0:
            out[p] = None
            continue
        header = data[pos:nl].decode(errors="replace").split()
        pos = nl + 1
        if len(header) == 3 and header[1] == "blob":  # "<sha> blob <size>"
            size = int(header[2])
            out[p] = data[pos:pos + size].count(b"\n")
            pos += size + 1  # the trailing LF
        else:  # "<name> missing"
            out[p] = None
    return out


# --- main -----------------------------------------------------------------------

def fmt(n) -> str:
    return "—" if n is None else f"{n:,}"


def new_tally() -> dict:
    return {"redesigned": 0, "adapted": 0, "inherited": 0, "rust": 0,
            "lz_done": set(), "lz_left": set()}


def subsystem(path: str) -> str:
    parts = path.split("/")
    if len(parts) > 3 and parts[1] in ("arch", "dev", "lib"):
        return parts[1] + "/" + parts[2]
    return parts[1] if len(parts) > 2 else parts[0]


def main() -> None:
    root = root_dir()
    want = sys.argv[1] if len(sys.argv) > 1 and sys.argv[1] else None
    cache: dict = {}

    rows = parse_roadmap(root)
    meta, modules, extras = parse_lineage(root)
    pin = str(meta.get("lz", ""))
    in_history = bool(pin) and subprocess.run(
        ["git", "-C", str(root), "merge-base", "--is-ancestor", pin, "HEAD"],
        capture_output=True).returncode == 0
    rng = f"{pin}..HEAD" if in_history else "HEAD"
    dates = close_dates(root, rng)
    wip_ms = under_way(root)
    commit_dates = sorted(set(git(root, "log", rng, "--format=%cd", "--date=short").split()))
    days = len(commit_dates) or 1

    lz_paths = sorted({p for m in modules for p in m.get("lz", [])})
    lz_len = lz_lines(root, pin, lz_paths)
    clone_missing = lz_len is None
    if clone_missing:  # the native file of an inherited module is LZ's, byte for byte
        lz_len = {}
        for m in modules:
            if m.get("status", "inherited") == "inherited" and len(m.get("lz", [])) == 1:
                lz_len[m["lz"][0]] = count_lines(root / m["rust"], cache)

    def lines_of(paths) -> tuple[int, int]:
        """(lines, files whose size is unknown) of a set of LZ paths."""
        total, unknown = 0, 0
        for p in paths:
            n = lz_len.get(p)
            if n is None:
                unknown += 1
            else:
                total += n
        return total, unknown

    row_ids = [r["id"] for r in rows]
    children = {rid: [c for c in row_ids if re.fullmatch(re.escape(rid) + r"[a-z]", c)]
                for rid in row_ids}
    is_child = {c for cs in children.values() for c in cs}

    pats = []  # (specificity, -row index, pattern, row id)
    for i, r in enumerate(rows):
        for p in scope_patterns(r["scope"]):
            pats.append((specificity(p), -i, p, r["id"]))

    def owner(path: str) -> str:
        """The ROADMAP row whose scope names the path, the most specific pattern winning."""
        best = None
        for spec, negi, p, rid in pats:
            if pattern_matches(p, path) and (best is None or (spec, negi) > best[:2]):
                best = (spec, negi, rid)
        return best[2] if best else "unassigned"

    own = {rid: new_tally() for rid in row_ids + ["unassigned"]}
    rows_of: dict[str, list[dict]] = {rid: [] for rid in own}
    for m in modules:
        path = m.get("rust", "")
        st = m.get("status", "inherited")
        key = st if st in STATUSES else "inherited"
        rid = owner(path)
        rows_of[rid].append(m)
        t = own[rid]
        t[key] += 1
        rl = count_lines(root / path, cache)
        m["_rust"] = rl
        m["_key"] = key
        if rl is not None:
            t["rust"] += rl
        srcs = m.get("lz", [])
        vals = [lz_len.get(p) for p in srcs]
        m["_lz"] = sum(vals) if vals and all(v is not None for v in vals) else None
        (t["lz_done"] if key in DONE else t["lz_left"]).update(srcs)
    for t in own.values():
        t["lz_left"] -= t["lz_done"]

    # A parent row (N2 with N2a, N2b, ...) shows its own modules plus its children's.
    agg = {}
    for rid, t in own.items():
        a = {k: (set(v) if isinstance(v, set) else v) for k, v in t.items()}
        for cid in children.get(rid, []):
            for k, v in own[cid].items():
                if isinstance(v, set):
                    a[k] |= v
                else:
                    a[k] += v
        a["lz_left"] -= a["lz_done"]
        agg[rid] = a

    total = new_tally()
    for t in own.values():
        for k, v in t.items():
            if isinstance(v, set):
                total[k] |= v
            else:
                total[k] += v
    total["lz_left"] -= total["lz_done"]
    done_lines, done_unknown = lines_of(total["lz_done"])
    left_lines, left_unknown = lines_of(total["lz_left"])
    rate = done_lines / days

    def status_of(rid: str) -> str:
        if children[rid] and all(status_of(c).startswith("met") for c in children[rid]):
            return "met " + (dates.get(rid) or max(dates.get(c, "") for c in children[rid]))
        if rid in dates:
            return "met " + dates[rid]
        r = next(x for x in rows if x["id"] == rid)
        if r["row_date"] and not agg[rid]["inherited"]:
            return "met " + r["row_date"]
        if rid in wip_ms or (agg[rid]["redesigned"] and agg[rid]["inherited"]):
            return "under way"
        return "pending"

    def est_days(lines: int) -> str:
        if not rate:
            return "—"
        d = lines / rate
        return "<0.1 d" if d < 0.1 else f"~{d:.1f} d"

    def with_unknown(n: int, unknown: int) -> str:
        return fmt(n) + (f" (+{unknown} unknown)" if unknown else "")

    print(f"EmiBSD redesign progress — {_dt.date.today()} — EmiBSD.LZ pin {pin[:12] or '?'} — "
          f"{len(modules)} modules ({len(extras)} extras not counted) — "
          f"{days} active day{'s' if days != 1 else ''} since lz-origin"
          f"{' (' + commit_dates[0] + ')' if commit_dates else ''}")
    print()
    print("| Milestone | Title | Status | Modules done | Modules left | LZ lines done | LZ lines left | Rust lines | Est. left |")
    print("|---|---|---|---:|---:|---:|---:|---:|---:|")
    for r in rows:
        rid = r["id"]
        if want and not (rid == want or re.fullmatch(re.escape(want) + r"[a-z]", rid)):
            continue
        t = agg[rid]
        st = status_of(rid)
        has_patterns = bool(scope_patterns(r["scope"])) or bool(children[rid])
        has_modules = any(t[k] for k in STATUSES)
        done_n = t["redesigned"] + t["adapted"]
        done_txt = (fmt(done_n) + (f" ({t['adapted']} adapted)" if t["adapted"] else "")) if has_modules else "—"
        left_txt = fmt(t["inherited"]) if has_modules else "—"
        ld, lu = lines_of(t["lz_done"])
        ll, llu = lines_of(t["lz_left"])
        if not has_patterns:
            lz_left_txt = "—" if st.startswith("met") else "scope in prose: not resolved"
            lz_done_txt = "—"
        elif not has_modules:
            lz_left_txt, lz_done_txt = "no modules match", "—"
        else:
            lz_done_txt, lz_left_txt = with_unknown(ld, lu), with_unknown(ll, llu)
        if st.startswith("met"):
            est = "done"
        elif ll:
            est = est_days(ll)
        else:
            est = "—"
        indent = "&nbsp;&nbsp;" if rid in is_child else ""
        print(f"| {indent}{rid} | {r['title']} | {st} | {done_txt} | {left_txt} | "
              f"{lz_done_txt} | {lz_left_txt} | {fmt(t['rust']) if t['rust'] else '—'} | {est} |")
    u = own["unassigned"]
    if any(u[k] for k in STATUSES) and not want:
        subs = sorted({subsystem(m.get("rust", "")) for m in rows_of["unassigned"]})
        shown = ", ".join(subs[:8]) + (", ..." if len(subs) > 8 else "")
        ud, udu = lines_of(u["lz_done"])
        ul, ulu = lines_of(u["lz_left"])
        print(f"| (no milestone) | modules no ROADMAP scope names: {shown} | — | "
              f"{fmt(u['redesigned'] + u['adapted'])}"
              f"{f' ({u['adapted']} adapted)' if u['adapted'] else ''} | {fmt(u['inherited'])} | "
              f"{with_unknown(ud, udu)} | {with_unknown(ul, ulu)} | {fmt(u['rust'])} | — |")
    print()

    if want:
        ids = [rid for rid in row_ids if rid == want or re.fullmatch(re.escape(want) + r"[a-z]", rid)]
        sel = [m for rid in ids for m in rows_of.get(rid, [])]
        if sel:
            print(f"Modules of {want} ({len(sel)}):")
            print()
            print("| Module | Status | LZ lines | Rust lines | LZ sources |")
            print("|---|---|---:|---:|---|")
            order = {"inherited": 0, "adapted": 1, "redesigned": 2}
            for m in sorted(sel, key=lambda m: (order[m["_key"]], m.get("rust", ""))):
                srcs = m.get("lz", [])
                src_txt = "same path" if srcs == [m.get("rust")] else ", ".join(srcs)
                print(f"| {m.get('rust', '')} | {m['_key']} | {fmt(m['_lz'])} | "
                      f"{fmt(m['_rust'])} | {src_txt} |")
            print()
        else:
            print(f"No ROADMAP row named {want}, or its scope names no module.")
            print()

    rust_total = 0
    for sub in ("sys", "tools"):
        for p in (root / sub).rglob("*.rs"):
            rust_total += count_lines(p, cache) or 0
    print(f"Totals: {total['redesigned']} modules redesigned, {total['adapted']} adapted, "
          f"{total['inherited']} inherited (of {len(modules)}); "
          f"LZ lines done {with_unknown(done_lines, done_unknown)}, left {with_unknown(left_lines, left_unknown)}; "
          f"Rust lines in sys/ and tools/: {rust_total:,}.")
    if rate:
        print(f"Rate: {rate:,.0f} LZ lines per active day over {days} active day{'s' if days != 1 else ''} "
              f"since lz-origin. Every inherited module, at that rate: {est_days(left_lines)}.")
    if clone_missing:
        print("Warning: reference/emibsd-lz is not checked out: inherited modules are measured on the "
              "native file (identical to LZ's); the LZ sources of redesigned modules count as unknown.")
    if not in_history:
        print("Warning: the pinned LZ commit is not in this history; the rate counts every commit date.")
    print()
    print("Method: modules = lineage.toml [[module]] rows; done = redesigned, with adapted shown apart "
          "(their call sites followed a redesign, nothing of their own changed); left = inherited; "
          "extras count nowhere. A module belongs to the N row whose scope cell names its path in "
          "backticks (a directory, a file or a glob), the most specific pattern winning; a row whose "
          "scope is prose resolves nothing, and a met row may keep inherited modules that a later "
          "row's prose claims. LZ lines = lines of the module's LZ sources at the pin, each file once "
          "per row; Rust lines = wc -l of the native file, tests included. A parent milestone shows "
          "its own modules plus its lettered children's. The time column is a linear extrapolation "
          "of the project's own average; it measures the past and promises nothing.")


if __name__ == "__main__":
    try:
        main()
    except Exception as exc:  # the skill preamble must never abort on this script
        print(f"progress: error: {exc!r}")
    sys.exit(0)
