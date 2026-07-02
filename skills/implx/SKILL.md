---
name: implx
description: Use when the user says "implx", "use implx", "用 implx", "implx drain full queue", "implx resume full queue", or asks for the short SpecRail queue shortcut to process or drain a repository's approved-spec issue/PR queue with SpecRail implementation queue planning, optional threads orchestration, per-issue implementation PRs, review-thread and CI gates, merge authorization, and closure audit.
---

# Implx

Use this skill as a short operational entrypoint. It does not replace the
focused SpecRail skills; it routes to them in the right order.

## Startup

1. Run the normal SpecRail startup:
   - read the repository `AGENTS.md`
   - read `AGENT_USAGE.md`, `workflow.yaml`, `states.yaml`, `labels.yaml`, and
     `skills/specrail-workflow/SKILL.md` when present
   - select the human-facing locale
   - identify human gates and route-gate requirements
2. Fetch current remote state before mapping a GitHub queue.
3. List open issues, open PRs, current branch, dirty files, and worktrees.
4. For broad queues, record whether the run has explicit queue-drain, goal, or
   budget authorization. `implx drain full queue`, `implx resume full queue`,
   and equivalent "finish all actionable issues/PRs" wording count as explicit
   queue-drain authorization, but do not silently create a Codex goal.

## Queue-Drain Shortcuts

Treat these short prompts as full queue-drain requests:

- `implx drain full queue`
- `implx resume full queue`
- `用 implx 完成所有 actionable issues 和 PRs`
- `用 implx 做完整队列`

For these prompts, set:

```yaml
overall_objective: drain_all_actionable_issues_and_prs
queue_mode: full_queue_drain
tranche_policy: bounded_loop
stop_policy: queue_drained_or_only_blockers
```

This means the current tranche stays bounded, but the objective is not narrowed
to one issue, one PR, or one tranche. After each tranche, refresh remote truth,
update the checkpoint, then choose the next actionable tranche until the queue
is drained or every remaining item is explicitly blocked, deferred, waiting on
CI, or needs human input.

If the parent reaches the context hard stop, write the checkpoint and resume
prompt instead of ending the full-queue objective. The next parent must resume
from `.specrail/runtime/current.json`, repo-local run logs, and fresh GitHub
truth, not old Codex session transcripts.

## Route

Use `skills/specrail-implement-queue/SKILL.md` when approved specs or open
issues need coordinated implementation PRs. For each candidate issue, require
the matching `specs/GH<issue-number>/product.md`, `tech.md`, and `tasks.md`
when present.

Before selecting an implementation tranche, build a spec coverage map for every
open issue and linked PR:

- `complete`: `product.md`, `tech.md`, and `tasks.md` all exist for
  `specs/GH<issue-number>/`
- `needs_tasks`: product and tech specs exist, but `tasks.md` is missing
- `needs_spec`: product or tech spec is missing
- `umbrella_covered`: another complete GH spec explicitly lists this issue in
  scope, acceptance criteria, tasks, or linked work
- `exception_allowed`: the item is a dependency bump, focused CI fix, docs-only
  correction, or another explicitly justified small change

If specs or tasks are missing, route that issue to the appropriate SpecRail
`write_spec` or `plan_tasks` step instead of implementing from assumptions.
For `queue_mode: full_queue_drain`, missing specs do not finish the queue:
choose a spec-writing tranche when no implementation-ready tranche is available.
Only treat missing specs as blockers when the user constrained the run to
implementation-only work or the issue evidence is insufficient to draft a spec.

For approved-spec queues, route to `skills/specrail-implement-queue/SKILL.md`
and follow its context budget, output firewall, runtime checkpoint, and goal-use
rules. Pass through `queue_mode: full_queue_drain` when a queue-drain shortcut
was used.

## Threads

When the queue needs parallel lanes, disjoint writable ownership, reviewer
lanes, CI polling, review-thread checks, merge gates, or closure audit, read
`integrations/threads.md` and use an available threads skill.

If no native threads capability is available, continue with the normal
single-agent SpecRail flow and report that no native threads were launched.

Keep ownership boundaries explicit:

- planners and reviewers are read-only
- workers own disjoint files or modules
- shared verification belongs to one coordinator
- dependent specs run serially

Keep the parent thin. Do not use old Codex session logs as queue state. Large
command output must be written to artifacts first; the parent reads only exit
codes, short tails, targeted grep results, and artifact paths.

## Implementation

For each issue slice:

1. Map existing PRs before opening replacement PRs.
2. Use one issue per implementation PR by default.
3. Use multiple PRs for one issue only when the task plan or risk justifies
   smaller slices.
4. Use `Refs #<issue>` for partial slices.
5. Use closing keywords only for the final slice that satisfies every
   acceptance criterion.
6. Route scoped implementation work through `skills/specrail-implement/SKILL.md`.
7. Implement only acceptance criteria from the linked spec and task plan.
8. Add or update tests that prove the changed behavior.

## Gates

Before reporting a PR as ready:

- run focused tests for touched behavior
- run repository deterministic checks
- run `python3 checks/check_workflow.py --repo .`
- run `python3 checks/check_workflow.py --repo . --spec-dir specs/GH<issue>`
  when the spec packet changed
- use `skills/specrail-check-impl-against-spec/SKILL.md` for spec comparison
- use `skills/specrail-pr-gate/SKILL.md` before merge-readiness claims

GitHub PR evidence must include current PR head SHA, CI/check rollup,
review-thread state from GraphQL, merge state, linked issue intent, and explicit
human merge authorization before merge.

## Boundaries

- Do not grant final approval.
- Do not merge without current PR-gate evidence and explicit authorization in
  the current conversation.
- Do not treat green CI as merge readiness without review-thread and merge-state
  truth.
- Do not close an issue from a partial implementation.
- Do not replace an existing maintainer-writable PR unless it is stale, unsafe,
  unwritable, or a human approves replacement.

## Handoff

Report a compact handoff when `implx` is active:

```yaml
implx_handoff:
  route: implement_queue
  overall_objective:
  queue_mode:
  issue_to_pr_map:
  spec_coverage:
    complete:
    needs_tasks:
    needs_spec:
    umbrella_covered:
    exception_allowed:
  approved_specs:
  current_tranche:
  remaining_queue:
  threads:
    mode:
    lanes:
    fallback_reason:
  goal:
    enabled:
    objective:
    stop_reason:
  context_budget:
    soft_stop_ratio:
    hard_stop_ratio:
    critical_stop_ratio:
  checkpoint:
    path:
    runtime_gate:
  gates:
    route_gate:
    pr_gate:
    review_threads:
    merge_authorization:
  closure_audit:
```
