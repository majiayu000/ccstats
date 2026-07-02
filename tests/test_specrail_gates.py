from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[1]
CHECKS = ROOT / "checks"
FIXTURES = ROOT / "examples" / "fixtures"
sys.path.insert(0, str(CHECKS))

import github_pr_evidence  # noqa: E402
from check_workflow import validate_spec_packet  # noqa: E402
from github_pr_evidence import EvidenceError, build_evidence, collect_review_threads  # noqa: E402
from pr_gate import evaluate_pr_gate  # noqa: E402


def fixture(name: str) -> dict[str, object]:
    return json.loads((FIXTURES / name).read_text(encoding="utf-8"))


def run_route_gate(*args: str) -> dict[str, object]:
    result = subprocess.run(
        [sys.executable, "checks/route_gate.py", "--repo", ".", *args, "--json"],
        cwd=ROOT,
        check=False,
        capture_output=True,
        text=True,
    )
    assert result.stdout, result.stderr
    return json.loads(result.stdout)


def test_route_gate_rejects_mismatched_issue_evidence(tmp_path: Path) -> None:
    evidence = fixture("issue-ready-to-spec.json")
    evidence_path = tmp_path / "issue-16.json"
    evidence_path.write_text(json.dumps(evidence), encoding="utf-8")

    payload = run_route_gate(
        "--route",
        "write_spec",
        "--issue",
        "1",
        "--evidence",
        str(evidence_path),
    )

    assert payload["decision"] == "blocked"
    assert any("issue evidence mismatch" in reason for reason in payload["reasons"])


def test_route_gate_requires_trusted_label_for_explicit_readiness_state() -> None:
    payload = run_route_gate(
        "--route",
        "write_spec",
        "--issue",
        "16",
        "--state",
        "ready_to_spec",
    )

    assert payload["decision"] == "needs_human"
    assert "trusted_state" in payload["missing"]
    assert any("maintainer readiness label required" in reason for reason in payload["reasons"])


def test_route_gate_missing_required_specs_needs_human() -> None:
    payload = run_route_gate(
        "--route",
        "implement",
        "--issue",
        "16",
        "--evidence",
        str(FIXTURES / "issue-ready-to-implement.json"),
    )

    assert payload["decision"] == "needs_human"
    assert "product_spec:specs/GH16/product.md" in payload["missing"]
    assert "tech_spec:specs/GH16/tech.md" in payload["missing"]


def test_route_gate_rejects_absolute_artifact_paths(tmp_path: Path) -> None:
    evidence = fixture("issue-ready-to-implement.json")
    artifacts = dict(evidence["artifacts"])  # type: ignore[arg-type]
    artifacts["product_spec"] = "/etc/passwd"
    evidence["artifacts"] = artifacts
    evidence_path = tmp_path / "absolute-artifact.json"
    evidence_path.write_text(json.dumps(evidence), encoding="utf-8")

    payload = run_route_gate(
        "--route",
        "implement",
        "--issue",
        "16",
        "--evidence",
        str(evidence_path),
    )

    assert payload["decision"] == "needs_human"
    assert "product_spec:/etc/passwd" in payload["missing"]
    assert any("repo-relative" in reason for reason in payload["reasons"])


def test_spec_packet_allows_product_and_tech_before_tasks(tmp_path: Path) -> None:
    spec_dir = tmp_path / "GH123"
    spec_dir.mkdir()
    (spec_dir / "product.md").write_text("Linked issue: #123\n", encoding="utf-8")
    (spec_dir / "tech.md").write_text("Linked issue: GH-123\n", encoding="utf-8")

    assert validate_spec_packet(spec_dir) == []


def test_pr_gate_requires_human_approval_review() -> None:
    evidence = fixture("pr-clean-authorized.json")
    evidence["reviews"] = [{"author": "chatgpt-codex-connector[bot]", "state": "APPROVED"}]

    result = evaluate_pr_gate(evidence)

    assert result["decision"] == "needs_human"
    assert "human_review" in result["missing"]


def test_pr_gate_allows_clean_authorized_human_approved_merge() -> None:
    result = evaluate_pr_gate(fixture("pr-clean-authorized.json"))

    assert result["decision"] == "allowed"
    assert "human_review" not in result["missing"]


def test_pr_gate_accepts_explicit_human_review_evidence() -> None:
    evidence = fixture("pr-clean-authorized.json")
    evidence["reviews"] = [{"author": "chatgpt-codex-connector[bot]", "state": "APPROVED"}]
    evidence["human_review"] = {
        "actor": "maintainer",
        "source": "chat",
        "summary": "final review approved",
    }

    result = evaluate_pr_gate(evidence)

    assert result["decision"] == "allowed"
    assert "human_review" not in result["missing"]


def test_review_thread_normalizer_rejects_incomplete_pagination() -> None:
    with pytest.raises(EvidenceError, match="pagination is incomplete"):
        build_evidence(
            {
                "number": 1,
                "state": "OPEN",
                "isDraft": False,
                "headRefOid": "1234567",
                "mergeStateStatus": "CLEAN",
                "closingIssuesReferences": [{"number": 1}],
                "statusCheckRollup": [],
                "reviews": [],
            },
            review_threads_payload(has_next_page=True),
        )


def test_collect_review_threads_paginates_all_pages(monkeypatch: pytest.MonkeyPatch) -> None:
    calls: list[str | None] = []

    def fake_run_gh_json(args: list[str]) -> dict[str, object]:
        after = None
        for index, value in enumerate(args):
            if value == "-F" and index + 1 < len(args) and args[index + 1].startswith("after="):
                after = args[index + 1].split("=", 1)[1]
        calls.append(after)
        if after is None:
            return review_threads_payload(has_next_page=True, end_cursor="cursor-1", thread_id="A")
        return review_threads_payload(has_next_page=False, thread_id="B")

    monkeypatch.setattr(github_pr_evidence, "run_gh_json", fake_run_gh_json)

    payload = collect_review_threads("owner", "repo", 1)
    threads = payload["data"]["repository"]["pullRequest"]["reviewThreads"]  # type: ignore[index]

    assert calls == [None, "cursor-1"]
    assert [node["id"] for node in threads["nodes"]] == ["A", "B"]
    assert threads["pageInfo"]["hasNextPage"] is False


def review_threads_payload(
    *,
    has_next_page: bool,
    end_cursor: str | None = None,
    thread_id: str = "T",
) -> dict[str, object]:
    return {
        "data": {
            "repository": {
                "pullRequest": {
                    "reviewThreads": {
                        "pageInfo": {
                            "hasNextPage": has_next_page,
                            "endCursor": end_cursor,
                        },
                        "nodes": [
                            {
                                "id": thread_id,
                                "isResolved": True,
                                "isOutdated": False,
                                "comments": {
                                    "nodes": [
                                        {
                                            "url": f"https://example.invalid/{thread_id}",
                                            "author": {"login": "reviewer"},
                                        }
                                    ]
                                },
                            }
                        ],
                    }
                }
            }
        }
    }
