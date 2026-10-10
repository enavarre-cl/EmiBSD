export const meta = {
  name: 'redesign',
  description: 'Redesign inherited EmiBSD modules: plan clusters, one redesigner per cluster in its own worktree, an independent review of each branch with one fix round, then one integrator merges the approved branches and runs just ci',
  phases: [
    { title: 'Plan', detail: 'group the requested modules into clusters that redesign alone from main, modules that share types folded in' },
    { title: 'Redesign', detail: 'one redesigner or mechanical per cluster, in its own worktree, at most 4 at once' },
    { title: 'Review', detail: 'an independent reviewer per branch; one fix round, then a second look' },
    { title: 'Integrate', detail: 'merge the approved branches, just ci under the machine lock' },
  ],
}

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

// /redesign: the re-engineering loop of CLAUDE.md run as a workflow over the agents of
// .claude/agents/. Adapted from EmiBSD.LZ's /port (.claude/workflows/port.js at b0901dd3).
//
//   /redesign sys/sys/intrmap.rs sys/sys/softintr.rs    native module paths as in lineage.toml
//   /redesign N2                                        every inherited module an N row's scope names
//   /redesign {"modules": [...], "max": 2, "base": "<branch>", "note": "<the user's words>"}
//
// The result is a branch with every approved redesign merged and `just ci` green; the main
// session fast-forwards main after the user's OK. The workflow never touches main and never
// pushes. Rules every agent follows: .claude/rules/subagents.md (its "Workflows" section names
// the lock and the notes directory used here).

const opts = (args && typeof args === 'object' && !Array.isArray(args)) ? args : {}
const requested = Array.isArray(args) ? args.map(String)
  : (typeof args === 'string' && args.trim()) ? args.trim().split(/\s+/)
  : Array.isArray(opts.modules) ? opts.modules.map(String) : []
const MAX = Number(opts.max) > 0 ? Number(opts.max) : 4 // the project's cap on agents that boot QEMU
const BASE = opts.base ? String(opts.base) : ''
const NOTE = opts.note ? String(opts.note) : `the user invoked /redesign ${requested.join(' ') || '(no arguments)'}`
const LOCK = '/tmp/emibsd/ci.lock'            // shared with EmiBSD.LZ: one Mac, one ci at a time
const NOTES = '/tmp/emibsd-native/redesign'

if (!requested.length) {
  log('nothing requested: name modules (sys/... paths of lineage.toml) or an N row of docs/ROADMAP.md')
  return { planned: 0, excluded: ['nothing requested'], notes: '' }
}

const COMMON = [
  'Context of this run (the rest is in .claude/rules/subagents.md and in your definition):',
  `- base: your worktree is branched from the main checkout's HEAD${BASE ? `; first \`git merge ${BASE}\`` : ''}.`,
  `- machine lock: ${LOCK} (mkdir takes it; shared with EmiBSD.LZ; a redesigner never runs just ci, just smoke or just ci-full).`,
  `- who else runs: up to ${MAX} redesigners of this /redesign workflow, each in its own worktree, single smokes only; EmiBSD.LZ may run on the same Mac.`,
  `- authorisation: ${NOTE}.`,
  '- the main checkout is the first line of `git worktree list --porcelain` (reference trees, target/openbsd, target/userland).',
  '- a "STOP ... wait for the user" refusal or a permission denial: stop, commit nothing more, report it as blocked.',
].join('\n')

// At most MAX agents that may boot QEMU at once; reviewers run outside the limit (no QEMU).
function limiter(n) {
  let active = 0
  const queue = []
  const pump = () => {
    while (active < n && queue.length) {
      active++
      queue.shift()()
    }
  }
  return fn => new Promise((resolve, reject) => {
    queue.push(() => fn().then(resolve, reject).finally(() => { active--; pump() }))
    pump()
  })
}
const slot = limiter(MAX)

const PLAN = {
  type: 'object',
  properties: {
    clusters: {
      type: 'array',
      items: {
        type: 'object',
        properties: {
          name: { type: 'string', description: 'a short slug, e.g. softintr' },
          modules: { type: 'array', items: { type: 'string' }, description: 'native paths as in lineage.toml (sys/...rs): the modules redesigned together' },
          lz: { type: 'array', items: { type: 'string' }, description: 'the LZ files they derive from (their lz lists)' },
          delicate: { type: 'boolean', description: 'true = redesigner (opus); false = mechanical (sonnet)' },
          sensitive: { type: 'boolean', description: 'true when any module is in an area of .claude/rules/security-review.md' },
          lines: { type: 'integer', description: 'wc -l of the native modules together' },
          why: { type: 'string', description: 'why this grouping and this class, one line' },
          study: { type: 'array', items: { type: 'string' }, description: 'redesigned modules and docs/IDIOMS.md rows to copy idioms from' },
        },
        required: ['name', 'modules', 'lz', 'delicate', 'sensitive', 'lines', 'why', 'study'],
      },
    },
    excluded: { type: 'array', items: { type: 'string' }, description: 'requested modules left out, each with its reason' },
    notes: { type: 'string', description: 'anything the redesigners or the user should know first' },
  },
  required: ['clusters', 'excluded', 'notes'],
}

const RESULT = {
  type: 'object',
  properties: {
    branch: { type: 'string' },
    tip: { type: 'string', description: 'the commit hash at the tip of the branch' },
    commits: { type: 'array', items: { type: 'string' }, description: 'hash and subject, oldest first' },
    status: { type: 'string', enum: ['done', 'partial', 'blocked'] },
    evidence: { type: 'array', items: { type: 'string' }, description: 'test counts before and after, smoke recipe names and their exact serial lines, the diff-openbsd result' },
    unsafe: { type: 'array', items: { type: 'string' }, description: 'one "<subsystem> <before> -> <after>" per subsystem whose count moved, from cargo xtask unsafe-report' },
    lineage: { type: 'array', items: { type: 'string' }, description: 'lineage.toml rows changed: module and new status, [[module.fn]] rows added' },
    deviations: { type: 'array', items: { type: 'string' }, description: 'Deviations closed or kept' },
    left: { type: 'array', items: { type: 'string' } },
    question: { type: 'string', description: 'what needs the user, if anything; empty otherwise' },
    handoff: { type: 'string', description: 'the path of HANDOFF.md' },
  },
  required: ['branch', 'tip', 'commits', 'status', 'evidence', 'unsafe', 'lineage', 'deviations', 'left', 'question', 'handoff'],
}

const REVIEW = {
  type: 'object',
  properties: {
    verdict: { type: 'string', enum: ['approve', 'changes'] },
    defects: {
      type: 'array',
      items: {
        type: 'object',
        properties: {
          file: { type: 'string' },
          line: { type: 'integer' },
          summary: { type: 'string' },
          ref: { type: 'string', description: 'reference/emibsd-lz/<path>:<line> or reference/openbsd-src/<path>:<line> the native code must keep' },
          fix: { type: 'string' },
        },
        required: ['file', 'line', 'summary', 'ref', 'fix'],
      },
    },
    notes: { type: 'array', items: { type: 'string' }, description: 'lesser remarks, no action required' },
    unchecked: { type: 'string', description: 'what the review did not cover' },
    security_review: { type: 'string', description: 'the Security-Review: paragraph for the merge commit when the cluster is in a security-review.md area (the invariant reviewed, the tests run, the diff-openbsd scenarios); empty otherwise' },
  },
  required: ['verdict', 'defects', 'notes', 'unchecked', 'security_review'],
}

const INTEGRATION = {
  type: 'object',
  properties: {
    branch: { type: 'string' },
    tip: { type: 'string' },
    ci_rc: { type: 'integer', description: 'exit code of just ci; -1 if it did not run' },
    ci_minutes: { type: 'number' },
    diff_openbsd: { type: 'string', description: '"equal", "not run", or the new differences' },
    conflicts: { type: 'array', items: { type: 'string' }, description: 'each conflict and how it was resolved' },
    userland_rebuilt: { type: 'boolean' },
    left: { type: 'array', items: { type: 'string' }, description: 'anything not green or not merged, with the reason' },
  },
  required: ['branch', 'tip', 'ci_rc', 'ci_minutes', 'diff_openbsd', 'conflicts', 'userland_rebuilt', 'left'],
}

const redesignPrompt = c => [
  `Redesign cluster "${c.name}" of EmiBSD: ${c.modules.join(', ')} (${c.lines} Rust lines; LZ sources ${c.lz.join(', ')}). ${c.why}`,
  `Study first: ${c.study.length ? c.study.join(', ') : 'the nearest redesigned sibling and docs/IDIOMS.md'}.`,
  c.sensitive
    ? 'This cluster is in an area of .claude/rules/security-review.md: every check the C makes is kept or made a type (name it in the commit body), constant time kept where the C has it, secrets wiped on the real storage; the reviewer writes the Security-Review paragraph, you do not.'
    : 'This cluster is not in an area of .claude/rules/security-review.md.',
  `Notes and HANDOFF.md: ${NOTES}/${c.name}/ (mkdir -p).`,
  COMMON,
  'Your final answer is the structured result: branch, tip, commits, status (done | partial | blocked), evidence, unsafe deltas, lineage rows changed, deviations, what is left, any question for the user, the HANDOFF.md path.',
].join('\n')

const reviewPrompt = (c, r) => [
  `Review branch ${r.branch} (tip ${r.tip}) of EmiBSD against main: the redesign of ${c.modules.join(', ')} (from LZ ${c.lz.join(', ')}).`,
  `Work in the main checkout, read-only: \`git log --oneline main..${r.branch}\`, \`git diff main ${r.branch} -- <path>\`, \`git show ${r.branch}:<path>\`, the LZ side with \`git -C reference/emibsd-lz show <pin>:<path>\`, the C under reference/openbsd-src.`,
  `The redesigner reported: status ${r.status}; evidence ${JSON.stringify(r.evidence)}; unsafe ${JSON.stringify(r.unsafe)}; lineage ${JSON.stringify(r.lineage)}; deviations ${JSON.stringify(r.deviations)}; left ${JSON.stringify(r.left)}.`,
  "Apply your definition's checklist. 'changes' only for a defect with evidence on both sides (native path:line and the LZ or C path:line it must keep) and a concrete failing input or sequence; style alone is a note.",
  c.sensitive
    ? 'The cluster is in an area of .claude/rules/security-review.md: return the Security-Review: paragraph (the invariant reviewed, the tests run, the diff-openbsd scenarios) in security_review.'
    : 'The cluster is not in a security-review.md area: leave security_review empty.',
].join('\n')

const fixPrompt = (c, r, rev) => [
  `Fix the reviewed defects of EmiBSD branch ${r.branch} (the redesign of ${c.modules.join(', ')}).`,
  `In your worktree first: \`git merge ${r.branch}\` (a fast-forward from main). Then fix each defect, with a host test where the logic allows, run the checks your definition lists, and commit on your branch (trailers as in git-commits.md). Do not rewrite the redesign.`,
  `Defects:\n${rev.defects.map(d => `- ${d.file}:${d.line}: ${d.summary} (must keep: ${d.ref}). Fix: ${d.fix}`).join('\n')}`,
  `Reviewer notes: ${rev.notes.length ? rev.notes.join(' | ') : 'none'}.`,
  `Notes and HANDOFF.md: ${NOTES}/${c.name}/.`,
  COMMON,
  'Your final answer is the structured result with your own branch and tip (they differ from the reviewed ones).',
].join('\n')

// --- Plan -----------------------------------------------------------------------------------

phase('Plan')
const plan = await agent([
  'You plan a batch of EmiBSD redesigns (the native operating system in Rust derived from EmiBSD.LZ, the faithful port of the OpenBSD kernel). You write nothing, commit nothing, build nothing, boot nothing.',
  `Requested: ${requested.join(', ')}.`,
  'A single token like "N2" names a row of docs/ROADMAP.md: take every module lineage.toml lists as `inherited` whose path the row\'s scope cell names in backticks (a directory such as `sys/uvm/`, a file such as `sys/sys/queue.rs`, or a glob such as `sys/kern/vfs_*`).',
  'For each requested module: its lineage.toml row (status, lz list, notes), its size (`wc -l`), the C behind it (the `//! Upstream:` line), the items it exports and how many modules use them (`grep -rl` over sys/).',
  'Group into clusters that each redesign alone from main: modules that share a type, or that cannot change one without the other, go together; a module over about 3,000 lines alone (.claude/rules/large-changes.md); two clusters that would need each other become one.',
  'Class per .claude/rules/subagents.md: delicate = locking, unsafe, intrusive data structures, uvm, the scheduler, VFS, the network stack, drivers with DMA or MMIO, anything in security-review.md\'s list; mechanical = tests only, lineage bookkeeping, call-site migrations. Unsure = delicate.',
  'Exclude, each with its reason: modules already `redesigned` or `adapted`, or in progress on another branch (`git branch -a`, the row notes); a module in .claude/rules/security-review.md\'s list whose subsystem is not finished (they are redesigned last, and crypto, IPsec, WireGuard and softraid CRYPTO last of all, N8) unless the authorisation says so; a module exporting an item used by more than 50 modules while an LZ milestone touching them is open (the timing rule of .claude/rules/re-engineering.md; `cargo xtask lz drift` and docs/STATUS.md say what LZ has open); a decision the user has not made (.claude/rules/scope-and-stubs.md, subagents.md "Stopping"); paths outside lineage.toml (sys/machine/, sys/stand/, sys/arch/host/).',
  'For each cluster say whether it is sensitive (an area of security-review.md) and name the redesigned modules and docs/IDIOMS.md rows to copy idioms from (grep the tree for the redesigned siblings).',
].join('\n'), { phase: 'Plan', label: 'plan', schema: PLAN })

if (!plan || !plan.clusters.length) {
  log('nothing to redesign')
  return { planned: 0, excluded: plan ? plan.excluded : ['the planner returned nothing'], notes: plan ? plan.notes : '' }
}
log(`${plan.clusters.length} clusters (${plan.clusters.filter(c => c.delicate).length} delicate, ${plan.clusters.filter(c => c.sensitive).length} sensitive), ${plan.excluded.length} excluded, at most ${MAX} redesigners at once`)
plan.excluded.forEach(x => log(`excluded: ${x}`))

// --- Redesign, then review each branch as soon as its worker is done (no barrier) --------------

const results = await pipeline(
  plan.clusters,
  c => slot(() => agent(redesignPrompt(c), {
    agentType: c.delicate ? 'redesigner' : 'mechanical', isolation: 'worktree',
    phase: 'Redesign', label: `redesign:${c.name}`, schema: RESULT,
  })),
  async (r, c) => {
    if (!r) {
      log(`${c.name}: the redesigner returned nothing`)
      return null
    }
    if (r.status === 'blocked') {
      log(`${c.name}: blocked: ${r.question || r.left.join('; ') || 'no reason given'}`)
      return { cluster: c, work: r, review: null, verdict: 'blocked' }
    }
    let work = r
    let review = await agent(reviewPrompt(c, work), { agentType: 'reviewer', phase: 'Review', label: `review:${c.name}`, schema: REVIEW })
    if (review && review.verdict === 'changes' && review.defects.length) {
      log(`${c.name}: ${review.defects.length} defect(s), one fix round`)
      const fixed = await slot(() => agent(fixPrompt(c, work, review), {
        agentType: c.delicate ? 'redesigner' : 'mechanical', isolation: 'worktree',
        phase: 'Review', label: `fix:${c.name}`, schema: RESULT,
      }))
      if (fixed && fixed.status !== 'blocked') {
        work = fixed
        review = await agent(reviewPrompt(c, work), { agentType: 'reviewer', phase: 'Review', label: `re-review:${c.name}`, schema: REVIEW })
      }
    }
    const verdict = review ? review.verdict : 'unreviewed'
    log(`${c.name}: ${verdict}${work.status === 'partial' ? ' (partial redesign)' : ''}`)
    return { cluster: c, work, review, verdict }
  },
)

const done = results.filter(Boolean)
const approved = done.filter(x => x.verdict === 'approve')
const held = done.filter(x => x.verdict !== 'approve')
held.forEach(x => log(`not integrated: ${x.cluster.name} (${x.verdict})`))

const summary = {
  planned: plan.clusters.length,
  excluded: plan.excluded,
  planner_notes: plan.notes,
  approved: approved.map(x => ({
    cluster: x.cluster.name, branch: x.work.branch, tip: x.work.tip, status: x.work.status,
    sensitive: x.cluster.sensitive, commits: x.work.commits, evidence: x.work.evidence,
    unsafe: x.work.unsafe, lineage: x.work.lineage, deviations: x.work.deviations,
    left: x.work.left, review_notes: x.review.notes, unchecked: x.review.unchecked,
    security_review: x.review.security_review,
  })),
  held: held.map(x => ({
    cluster: x.cluster.name, verdict: x.verdict, branch: x.work.branch, tip: x.work.tip,
    question: x.work.question, defects: x.review ? x.review.defects : [], left: x.work.left,
    handoff: x.work.handoff,
  })),
  integrated: null,
  next: '',
}

if (!approved.length) {
  summary.next = 'nothing approved: read the held list; the branches and HANDOFF.md files are still there'
  return summary
}

// --- Integrate: one agent needs every approved branch, so this barrier is the real one -------------

phase('Integrate')
const sensitive = approved.filter(x => x.cluster.sensitive)
const integ = await agent([
  `Integrate these reviewed EmiBSD branches into one: ${approved.map(x => `${x.work.branch} (${x.cluster.name}: ${x.cluster.modules.join(', ')})`).join('; ')}.`,
  'In your worktree: `git merge` each branch in that order (`--no-ff` for a sensitive one), resolving the shared files as your definition says (lineage.toml with one [[module]] per file and both sides\' [[module.fn]] rows, unsafe-budget.toml never up, lz-sync.toml, docs/IDIOMS.md, the justfile smokes list, xtask option tables, docs, agent-memory files with both sides kept).',
  sensitive.length
    ? `Security-Review paragraphs for the merge commits (one per sensitive branch, as .claude/rules/security-review.md requires):\n${sensitive.map(x => `- ${x.work.branch}: ${x.review.security_review || '(the reviewer returned none: stop and report it)'}`).join('\n')}`
    : 'No branch is in a security-review.md area.',
  `Then, under the lock ${LOCK}: \`just userland\` if any branch touched tools/xtask/src/userland*, then \`just jobs=3 ci\`, gated on rc=0 exactly (it includes check-lineage, check-drift and check-unsafe), then \`just diff-openbsd\` if any branch touched syscalls, VFS, a file system or a security-review.md area (every new difference is a defect to report). Fix only what the merge broke; a redesign's own defect goes in your report, not under the rug.`,
  `Never touch main, never push. Notes: ${NOTES}/integrate/.`,
  COMMON,
  'Your final answer is the structured result: branch, tip, ci rc and minutes, the diff-openbsd result, each conflict and its resolution, whether the userland was rebuilt, what is left.',
].join('\n'), { agentType: 'integrator', isolation: 'worktree', phase: 'Integrate', label: 'integrate', schema: INTEGRATION })

summary.integrated = integ
summary.next = integ && integ.ci_rc === 0
  ? `ci green on ${integ.branch} (${integ.tip}): with the user's OK, fast-forward main to it, then remove the merged worktrees and branches (disk hygiene)`
  : 'ci not green or the integrator returned nothing: read integrated.left and the held list before anything reaches main'
return summary
