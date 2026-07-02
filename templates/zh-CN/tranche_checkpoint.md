# Tranche Checkpoint

这是长时间 agent run 的可选本地运行时 checkpoint。它不替代 GitHub issue、
pull request、label、review、branch，也不替代 SpecRail spec artifact 作为
durable workflow state。

```json
{
  "checkpoint_version": 1,
  "tranche_id": "YYYY-MM-DD-repo-topic-t01",
  "repo": "owner/repo or local/path",
  "scope": "one bounded tranche; name exclusions and non-goals",
  "status": "planning",
  "overall_objective": "drain_all_actionable_issues_and_prs",
  "queue_mode": "bounded_tranche",
  "spec_coverage": {
    "checked_at": null,
    "complete": [],
    "needs_tasks": [],
    "needs_spec": [],
    "umbrella_covered": [],
    "exception_allowed": []
  },
  "goal": {
    "enabled": false,
    "objective": "",
    "status": "",
    "tokens_used": null,
    "token_budget": null
  },
  "goal_candidate": {
    "objective": "Finish this bounded tranche only",
    "done_when": [
      "runtime checkpoint updated",
      "remote truth refreshed",
      "verification evidence recorded"
    ],
    "constraints": [
      "do not read raw Codex session logs",
      "do not paste raw logs into parent context"
    ],
    "blocked_stop_condition": "record blocker and next_action when CI, reviewer, or remote truth is pending"
  },
  "context_budget": {
    "window_tokens": 258400,
    "soft_stop_ratio": 0.5,
    "hard_stop_ratio": 0.65,
    "critical_stop_ratio": 0.75,
    "override_allowed": true
  },
  "output_firewall": {
    "raw_log_policy": "file_only",
    "max_parent_stdout_lines": 150,
    "max_subagent_final_lines": 150,
    "artifact_root": "artifacts/logs/YYYY-MM-DD-repo-topic-t01"
  },
  "items": [
    {
      "issue": null,
      "pr": null,
      "state": "planning",
      "spec_status": null,
      "spec_status_reason": null,
      "branch": null,
      "worktree": null,
      "head_sha": null,
      "truth_level": null,
      "ci": {
        "status": "unknown",
        "run_id": null,
        "evidence": null
      },
      "local_verification": [],
      "review": {
        "reviewer_lane": null,
        "status": "pending",
        "blocking_findings": []
      },
      "review_threads": {
        "status": "unknown",
        "unresolved_count": null,
        "evidence": null,
        "checked_at": null
      },
      "pr_gate": {
        "status": "unknown",
        "head_sha": null,
        "evidence": null,
        "checked_at": null
      },
      "blocker": null,
      "next_action": "refresh remote truth and write queue gate",
      "merge_state": "not_merge_ready"
    }
  ],
  "remaining_queue": [
    {
      "issue": null,
      "pr": null,
      "spec_status": "needs_spec",
      "spec_status_reason": "missing product.md or tech.md for the issue",
      "state": "needs_spec",
      "blocker": null,
      "next_action": "write or update the SpecRail spec packet before implementation"
    }
  ],
  "resume_prompt": "Read this checkpoint, refresh remote truth, and continue only the named tranche. Do not read raw Codex session logs."
}
```
