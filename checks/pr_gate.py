#!/usr/bin/env python3
"""Evaluate deterministic PR merge-readiness evidence.

The gate is intentionally offline. GitHub or threads adapters may collect the
evidence JSON, but this script only evaluates it and never writes remote state.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any


CHECK_PASS_CONCLUSIONS = {"SUCCESS"}
CLEAN_MERGE_STATES = {"CLEAN"}
ACTIVE_CHANGE_REQUESTS = {"CHANGES_REQUESTED"}
APPROVAL_STATES = {"APPROVED"}


def _is_bot_author(author: Any) -> bool:
    if not isinstance(author, str):
        return False
    value = author.lower()
    return value.endswith("[bot]") or "code-assist" in value or "codex-connector" in value


def _as_bool(value: Any) -> bool:
    return value is True


def _non_empty_string(value: Any) -> bool:
    return isinstance(value, str) and bool(value.strip())


def _positive_int(value: Any) -> bool:
    return isinstance(value, int) and value > 0


def _load_json(path: Path) -> dict[str, Any]:
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except OSError as exc:
        raise ValueError(f"cannot read evidence file {path}: {exc}") from exc
    except json.JSONDecodeError as exc:
        raise ValueError(f"invalid evidence JSON {path}: {exc.msg}") from exc
    if not isinstance(data, dict):
        raise ValueError("evidence JSON must be an object")
    return data


def _check_items(evidence: dict[str, Any]) -> tuple[list[str], list[str], list[str]]:
    satisfied: list[str] = []
    missing: list[str] = []
    reasons: list[str] = []

    checks = evidence.get("checks")
    if not isinstance(checks, list) or not checks:
        missing.append("checks")
        reasons.append("CI/check evidence is missing")
        return satisfied, missing, reasons

    for index, item in enumerate(checks, start=1):
        if not isinstance(item, dict):
            reasons.append(f"check #{index} is not an object")
            continue
        name = str(item.get("name") or f"check #{index}")
        status = str(item.get("status") or "").upper()
        conclusion = str(item.get("conclusion") or "").upper()
        if status != "COMPLETED":
            reasons.append(f"{name} is not completed: {status or 'missing status'}")
            continue
        if conclusion not in CHECK_PASS_CONCLUSIONS:
            reasons.append(f"{name} did not pass: {conclusion or 'missing conclusion'}")
            continue
        satisfied.append(f"check passed: {name}")
    return satisfied, missing, reasons


def _review_items(evidence: dict[str, Any]) -> tuple[list[str], list[str], list[str]]:
    satisfied: list[str] = []
    missing: list[str] = []
    reasons: list[str] = []

    reviews = evidence.get("reviews", [])
    if reviews is None:
        reviews = []
    if not isinstance(reviews, list):
        reasons.append("reviews must be a list")
        return satisfied, missing, reasons

    approved_human_reviewers: list[str] = []
    for index, review in enumerate(reviews, start=1):
        if not isinstance(review, dict):
            reasons.append(f"review #{index} is not an object")
            continue
        state = str(review.get("state") or "").upper()
        author = review.get("author") or f"review #{index}"
        if state in ACTIVE_CHANGE_REQUESTS:
            reasons.append(f"changes requested by {author}")
        if state in APPROVAL_STATES and isinstance(author, str) and not _is_bot_author(author):
            approved_human_reviewers.append(author)
    if not any(reason.startswith("changes requested") for reason in reasons):
        satisfied.append("no active changes-requested review evidence")
    if approved_human_reviewers:
        satisfied.append("human review approved by " + ", ".join(sorted(approved_human_reviewers)))
    else:
        missing.append("human_review")
    return satisfied, missing, reasons


def _thread_items(evidence: dict[str, Any]) -> tuple[list[str], list[str], list[str]]:
    satisfied: list[str] = []
    missing: list[str] = []
    reasons: list[str] = []

    threads = evidence.get("review_threads")
    if not isinstance(threads, list):
        missing.append("review_threads")
        reasons.append("review thread evidence is missing")
        return satisfied, missing, reasons

    unresolved = []
    for index, thread in enumerate(threads, start=1):
        if not isinstance(thread, dict):
            unresolved.append(f"thread #{index}")
            continue
        is_resolved = _as_bool(thread.get("is_resolved"))
        is_outdated = _as_bool(thread.get("is_outdated"))
        if not is_resolved and not is_outdated:
            unresolved.append(str(thread.get("url") or thread.get("id") or f"thread #{index}"))

    if unresolved:
        reasons.append("unresolved review threads: " + ", ".join(unresolved))
    else:
        satisfied.append("no unresolved active review threads")
    return satisfied, missing, reasons


def _authorization_item(evidence: dict[str, Any]) -> tuple[list[str], list[str]]:
    authorization = evidence.get("human_authorization")
    if not isinstance(authorization, dict):
        return [], ["human_authorization"]
    missing = []
    for key in ["actor", "source"]:
        if not _non_empty_string(authorization.get(key)):
            missing.append(f"human_authorization.{key}")
    if missing:
        return [], missing
    return [f"human authorization from {authorization['actor']} via {authorization['source']}"], []


def _explicit_human_review_item(evidence: dict[str, Any]) -> tuple[list[str], list[str]]:
    review = evidence.get("human_review")
    if not isinstance(review, dict):
        return [], ["human_review"]
    missing = []
    for key in ["actor", "source"]:
        if not _non_empty_string(review.get(key)):
            missing.append(f"human_review.{key}")
    if missing:
        return [], missing
    return [f"human review from {review['actor']} via {review['source']}"], []


def evaluate_pr_gate(evidence: dict[str, Any]) -> dict[str, Any]:
    """Evaluate merge-readiness evidence and return a stable decision object."""

    reasons: list[str] = []
    satisfied: list[str] = []
    missing: list[str] = []

    if _positive_int(evidence.get("pr")):
        satisfied.append(f"pr: {evidence['pr']}")
    else:
        missing.append("pr")

    state = str(evidence.get("state") or "").upper()
    if state == "OPEN":
        satisfied.append("PR state is OPEN")
    elif state:
        reasons.append(f"PR state must be OPEN; got {state}")
    else:
        missing.append("state")

    if evidence.get("is_draft") is False:
        satisfied.append("PR is not draft")
    elif "is_draft" not in evidence:
        missing.append("is_draft")
    else:
        reasons.append("draft PR cannot merge")

    if _non_empty_string(evidence.get("head_sha")):
        satisfied.append(f"head_sha: {evidence['head_sha']}")
    else:
        missing.append("head_sha")

    if _positive_int(evidence.get("linked_issue")):
        satisfied.append(f"linked_issue: {evidence['linked_issue']}")
    else:
        missing.append("linked_issue")

    merge_state = str(evidence.get("merge_state") or "").upper()
    if merge_state in CLEAN_MERGE_STATES:
        satisfied.append(f"merge_state: {merge_state}")
    elif merge_state:
        reasons.append(f"merge_state must be CLEAN; got {merge_state}")
    else:
        missing.append("merge_state")

    check_satisfied, check_missing, check_reasons = _check_items(evidence)
    satisfied.extend(check_satisfied)
    missing.extend(check_missing)
    reasons.extend(check_reasons)

    review_satisfied, review_missing, review_reasons = _review_items(evidence)
    if "human_review" in review_missing:
        explicit_review_satisfied, explicit_review_missing = _explicit_human_review_item(evidence)
        if explicit_review_satisfied:
            review_satisfied.extend(explicit_review_satisfied)
            review_missing = [item for item in review_missing if item != "human_review"]
        else:
            review_missing.extend(explicit_review_missing)
    satisfied.extend(review_satisfied)
    missing.extend(review_missing)
    reasons.extend(review_reasons)

    thread_satisfied, thread_missing, thread_reasons = _thread_items(evidence)
    satisfied.extend(thread_satisfied)
    missing.extend(thread_missing)
    reasons.extend(thread_reasons)

    auth_satisfied, auth_missing = _authorization_item(evidence)
    satisfied.extend(auth_satisfied)
    missing.extend(auth_missing)

    human_gate_missing = [
        item
        for item in missing
        if item.startswith("human_review") or item.startswith("human_authorization")
    ]
    deterministic_missing = [item for item in missing if item not in human_gate_missing]
    if reasons or deterministic_missing:
        decision = "blocked"
    elif human_gate_missing:
        decision = "needs_human"
    else:
        decision = "allowed"

    blocked_actions = []
    if decision in {"blocked", "needs_human"}:
        blocked_actions.append("merge")
    if decision == "blocked":
        blocked_actions.append("final_approval")

    return {
        "decision": decision,
        "pr": evidence.get("pr"),
        "linked_issue": evidence.get("linked_issue"),
        "head_sha": evidence.get("head_sha"),
        "reasons": sorted(set(reasons)),
        "satisfied": sorted(set(satisfied)),
        "missing": sorted(set(missing)),
        "blocked_actions": blocked_actions,
        "verification_commands": [
            "python3 checks/pr_gate.py --repo . --evidence <evidence.json>",
            "python3 checks/check_workflow.py --repo .",
        ],
    }


def print_gate_human(result: dict[str, Any]) -> None:
    print(f"decision: {result['decision']}")
    if result.get("pr"):
        print(f"pr: {result['pr']}")
    if result.get("linked_issue"):
        print(f"linked_issue: GH-{result['linked_issue']}")
    if result.get("head_sha"):
        print(f"head_sha: {result['head_sha']}")
    if result["reasons"]:
        print("reasons:")
        for reason in result["reasons"]:
            print(f"- {reason}")
    if result["missing"]:
        print("missing:")
        for item in result["missing"]:
            print(f"- {item}")


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Evaluate SpecRail PR merge-readiness evidence."
    )
    parser.add_argument("--repo", default=".", help="Repository root, kept for CLI symmetry")
    parser.add_argument("--evidence", required=True, help="PR evidence JSON file")
    parser.add_argument(
        "--mode",
        default="dry_run",
        choices=["dry_run", "advisory", "required"],
        help="Evaluation enforcement mode",
    )
    parser.add_argument("--json", action="store_true", help="Print JSON output")
    args = parser.parse_args()

    try:
        evidence = _load_json(Path(args.evidence))
        result = evaluate_pr_gate(evidence)
    except ValueError as exc:
        result = {
            "decision": "blocked",
            "pr": None,
            "linked_issue": None,
            "head_sha": None,
            "reasons": [str(exc)],
            "satisfied": [],
            "missing": [],
            "blocked_actions": ["merge", "final_approval"],
            "verification_commands": ["python3 checks/pr_gate.py --repo . --evidence <evidence.json>"],
        }

    if args.json:
        print(json.dumps(result, indent=2, sort_keys=True))
    else:
        print_gate_human(result)

    if result["decision"] == "blocked":
        return 1
    if result["decision"] == "needs_human" and args.mode == "required":
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
