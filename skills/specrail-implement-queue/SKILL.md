---
name: specrail-implement-queue
description: Use when implementing or draining a GitHub issue/PR queue in a SpecRail-governed repository where approved specs already exist, such as multiple numbered specs/GH packets that need one or more implementation PRs per issue. Maps issues to specs and existing PRs, supports full-queue drain requests from implx, selects single-agent or optional threads orchestration, preserves partial versus final closing semantics, and requires SpecRail verification plus PR gates before merge-readiness claims.
---

# SpecRail Implement Queue

Use this skill for approved-spec implementation queues. For one small issue,
route to `skills/specrail-implement/SKILL.md` instead.

## Startup

1. Run the SpecRail workflow startup:
   - read `AGENTS.md`, `AGENT_USAGE.md`, `workflow.yaml`, `states.yaml`,
     `labels.yaml`, and `skills/specrail-workflow/SKILL.md` when present
   - select the locale
   - identify the `implement` route and human gates
2. Fetch current remote state before mapping the queue.
3. List open issues, open PRs, local branch, dirty files, and worktrees.
4. For each candidate issue, read:
   - the GitHub issue
   - `specs/GH<issue-number>/product.md`
   - `specs/GH<issue-number>/tech.md`
   - `specs/GH<issue-number>/tasks.md`
5. Map existing PRs before creating replacement PRs.

## Spec Coverage Gate

Before planning implementation work, classify every open issue and linked PR:

- `complete`: `product.md`, `tech.md`, and `tasks.md` all exist for the issue
- `needs_tasks`: product and tech specs exist, but `tasks.md` is missing
- `needs_spec`: product or tech spec is missing
- `umbrella_covered`: another complete GH spec explicitly includes the issue in
  scope, acceptance criteria, task plan, or linked work
- `exception_allowed`: dependency bump, focused CI fix, docs-only correction, or
  another explicitly justified small non-spec change

Implementation candidates are only `complete`, `umbrella_covered`, or
`exception_allowed`. For `needs_spec` and `needs_tasks`, route to the focused
SpecRail spec-writing or task-planning skill first. Do not implement from only
issue text, PR comments, or old chat context unless the user explicitly
authorizes a non-spec exception and the checkpoint records the reason.

For `queue_mode: full_queue_drain`, `needs_spec` and `needs_tasks` are
actionable planning work, not completion. If no implementation-ready tranche is
available, select the smallest spec-writing or task-planning tranche instead of
ending the queue drain. Treat them as blockers only when the user limited the
run to implementation-only work, the issue lacks enough evidence to draft a
spec, or a human gate prevents spec creation.

## Queue Planning

Build an issue-to-PR plan:

- one issue per implementation PR by default
- several PRs per issue only when the task plan or risk justifies smaller slices
- combined PRs only when the specs explicitly share one acceptance surface
- `Refs #<issue>` for partial slices
- closing keywords only for the final slice that satisfies every acceptance
  criterion

Record the plan as:

```yaml
specrail_implementation_queue:
  overall_objective:
  queue_mode: bounded_tranche | full_queue_drain
  spec_coverage:
    complete:
    needs_tasks:
    needs_spec:
    umbrella_covered:
    exception_allowed:
  current_tranche:
  remaining_queue:
  issues:
    - issue:
      spec_dir:
      spec_status: complete | needs_tasks | needs_spec | umbrella_covered | exception_allowed
      spec_status_reason:
      acceptance_criteria:
      existing_prs:
      planned_prs:
      completion_mode: partial | final
      verification:
  gates:
    route_gate:
    pr_gate:
    review_threads:
    merge_authorization:
  context_budget:
    soft_stop_ratio: 0.50
    hard_stop_ratio: 0.65
    critical_stop_ratio: 0.75
  checkpoint:
    path:
    runtime_gate:
  stop_policy:
```

For broad queues, always execute as bounded tranches. If the user or calling
skill says `implx drain full queue`, `implx resume full queue`, or otherwise
explicitly asks to finish all actionable issues/PRs, set
`queue_mode: full_queue_drain`. In that mode, choose the smallest mergeable
current tranche, checkpoint it, then continue selecting new implementation,
spec-writing, or task-planning tranches until the queue is drained or every
remaining item is explicitly blocked, deferred, waiting on CI, or needs human
input.

A blocked or waiting current tranche does not stop full-queue drain. After
checkpointing that tranche, refresh remote truth and look for an independent
next tranche. Stop only when every remaining issue and PR is listed in
`remaining_queue` with `spec_status`, `blocker`, and `next_action`.

If the user only asks for a broad queue without explicit full-queue drain
authorization, choose the smallest mergeable tranche and leave the rest in the
checkpoint.

## Orchestration

Use `integrations/threads.md` and an available threads skill when the queue needs
parallel lanes, disjoint writable ownership, review-thread checks, CI polling,
merge gates, or closure audit.

If threads is unavailable, continue with the normal single-agent SpecRail flow
and report that no native threads were launched.

Keep ownership boundaries explicit:

- planner and reviewer lanes are read-only
- worker lanes own disjoint files or modules
- shared verification belongs to one coordinator
- dependent specs run serially

## Context Budget

For long queues, record a parent context budget before spawning lanes:

- default soft stop: 50% of the active context window
- default hard stop: 65% of the active context window
- default critical stop: 75% of the active context window

These are defaults, not universal limits. If the runtime exposes a different
budget or the user provides one, record the override.

At soft stop, do not spawn new lanes or broaden scope. At hard stop, finish the
current critical step, write the runtime checkpoint, and hand off to a fresh
parent thread. At critical stop, only write checkpoint and resume instructions.
For `queue_mode: full_queue_drain`, a hard-stop handoff preserves the full queue
objective and records the next actionable tranche; it does not redefine success
as completing only the current tranche.

Do not read raw `~/.codex/sessions` logs, old parent transcripts, or broad
session JSONL as queue state. Use the checkpoint, repo-local run logs, and fresh
remote truth.

## Output Firewall

Large output commands are allowed only when raw stdout and stderr go to artifact
files. The coordinator may read exit code, a short tail, targeted grep output,
and the artifact path.

Default rules:

- no raw `gh run view --log` output in parent context
- no raw full `cargo test` or full workspace test output in parent context
- no broad `rg` or `git grep` across `.codex`, `.claude`, `target`,
  `node_modules`, session JSONL, or log files
- parent stdout tail target: 150 lines or less
- subagent final output target: 150 lines or less

Prefer artifact paths such as `artifacts/logs/<tranche>/cargo-test.log` and
summaries such as `artifacts/logs/<tranche>/ci-summary.md`.

## Runtime Checkpoint

For long queues, create or update an optional local runtime checkpoint before:

- spawning writable lanes
- pushing or opening PRs
- waiting on CI or long local tests
- requesting merge review
- compacting, handing off, or closing the parent thread
- selecting the next tranche in a full-queue drain loop

Use `templates/tranche_checkpoint.md` as the shape and validate concrete JSON
checkpoints with:

```bash
python3 checks/runtime_ledger_gate.py --checkpoint .specrail/runtime/current.json
```

The runtime checkpoint is a local handoff layer only. GitHub issues, PRs,
labels, reviews, branches, and SpecRail spec packets remain the durable workflow
truth.

For `queue_mode: full_queue_drain`, the checkpoint must record the overall
objective, spec coverage, current tranche, completed items, remaining queue,
explicit blockers, and next resume action. `needs_spec`, `needs_tasks`,
`eligible_impl`, `waiting_ci`, and `needs_review` do not count as drained while
the checkpoint status is `complete`; they require a next action or a non-drain
handoff status. Resume from the checkpoint plus fresh remote truth; do not
recover queue state from old parent transcripts.

## Goal Use

Codex Goal can control the current tranche only when the user explicitly asks
for goal-controlled work, gives a token or time budget, or explicitly frames the
run as long-running goal work. Otherwise record a `goal_candidate` in the
checkpoint but do not silently create a goal.

Goal never replaces the runtime checkpoint, GitHub truth, or SpecRail gates.

## Implementation

For each issue slice:

1. Use `skills/specrail-implement/SKILL.md` for the scoped implementation.
2. Search before adding files, public APIs, workflow assets, schemas, templates,
   or policies.
3. Implement only acceptance criteria from the linked spec and task plan.
4. Add or update tests that prove the changed behavior.
5. Keep machine IDs, paths, commands, states, routes, and JSON keys in English.
6. Keep human-facing text in the selected locale.

## Review And Verification

Before a PR is considered ready:

- run focused tests for touched behavior
- run repository deterministic checks
- run `python3 checks/check_workflow.py --repo .`
- run `python3 checks/check_workflow.py --repo . --spec-dir specs/GH<issue>`
  when the spec packet changed
- use `skills/specrail-check-impl-against-spec/SKILL.md` to compare the PR or
  diff to the linked specs
- use `skills/specrail-pr-gate/SKILL.md` before reporting merge readiness

For GitHub PRs, current evidence must include:

- PR head SHA
- CI/check rollup
- review decision when available
- GraphQL review-thread state
- merge state
- linked issue or closing reference intent
- explicit human merge authorization before merge

## Boundaries

- Do not grant final approval.
- Do not merge without explicit human authorization and current PR-gate evidence.
- Do not treat green CI as merge readiness without review-thread and merge-state
  truth.
- Do not close an issue from a partial implementation.
- Do not replace an existing maintainer-writable PR unless it is stale, unsafe,
  unwritable, or a human approves replacement.
- Do not vendor a local threads skill into SpecRail.

## Output

Report:

- overall objective, queue mode, current tranche, and remaining queue
- issue-to-PR mapping
- PR links, head SHAs, and merge commits when merged
- acceptance criteria covered or remaining
- tests and deterministic checks run
- review-thread, CI, merge-state, and PR-gate evidence
- issues still open and why
- local dirty or stale worktree state
