#!/usr/bin/env python3
"""Deterministic end-to-end benchmark for ccstats core paths."""

from __future__ import annotations

import argparse
import json
import os
import shutil
import statistics
import subprocess
import tempfile
import time
from dataclasses import dataclass
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Dict, List


@dataclass(frozen=True)
class BenchCommand:
    name: str
    args: List[str]
    env: Dict[str, str]


def iso_z(dt: datetime) -> str:
    return dt.astimezone(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def write_jsonl(path: Path, rows: List[dict]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as f:
        for row in rows:
            f.write(json.dumps(row, separators=(",", ":")))
            f.write("\n")


def generate_claude_dataset(root: Path, files: int, lines_per_file: int) -> Path:
    home = root / "claude-home"
    projects_root = home / ".claude" / "projects"
    base = datetime(2026, 1, 1, 0, 0, 0, tzinfo=timezone.utc)

    for idx in range(files):
        project = f"project-{idx % 8:02d}"
        session = f"session-{idx:04d}"
        file_path = projects_root / project / f"{session}.jsonl"
        rows: List[dict] = []
        total_pairs = lines_per_file // 2

        for j in range(total_pairs):
            ts = base + timedelta(seconds=idx * 13 + j * 11)
            message_id = f"m-{idx}-{j}"

            partial = {
                "isSidechain": False,
                "timestamp": iso_z(ts),
                "message": {
                    "id": message_id,
                    "model": "claude-3-5-sonnet-20241022",
                    "stop_reason": None,
                    "usage": {
                        "input_tokens": 200 + (j % 20),
                        "output_tokens": 80 + (j % 10),
                        "cache_creation_input_tokens": 0,
                        "cache_read_input_tokens": 40 + (j % 5),
                    },
                },
            }
            complete = {
                "isSidechain": False,
                "timestamp": iso_z(ts + timedelta(seconds=1)),
                "message": {
                    "id": message_id,
                    "model": "claude-3-5-sonnet-20241022",
                    "stop_reason": "end_turn",
                    "usage": {
                        "input_tokens": 220 + (j % 20),
                        "output_tokens": 90 + (j % 10),
                        "cache_creation_input_tokens": 0,
                        "cache_read_input_tokens": 45 + (j % 5),
                    },
                },
            }
            rows.append(partial)
            rows.append(complete)

        write_jsonl(file_path, rows)

    return home


def generate_codex_dataset(root: Path, files: int, events_per_file: int) -> Path:
    home = root / "codex-home"
    sessions_root = home / "sessions"
    base = datetime(2026, 2, 1, 0, 0, 0, tzinfo=timezone.utc)

    for idx in range(files):
        file_path = sessions_root / f"session-{idx:04d}.jsonl"
        rows: List[dict] = [
            {
                "timestamp": iso_z(base + timedelta(seconds=idx)),
                "type": "turn_context",
                "payload": {"model": "gpt-5.4"},
            }
        ]

        total_input = 0
        total_cached = 0
        total_output = 0
        total_reasoning = 0
        for j in range(events_per_file):
            total_input += 300 + (j % 7)
            total_cached += 70 + (j % 3)
            total_output += 140 + (j % 5)
            total_reasoning += 30 + (j % 4)
            total_tokens = total_input + total_output
            ts = base + timedelta(seconds=idx * 17 + j * 3)
            rows.append(
                {
                    "timestamp": iso_z(ts),
                    "type": "event_msg",
                    "payload": {
                        "type": "token_count",
                        "info": {
                            "total_token_usage": {
                                "input_tokens": total_input,
                                "cached_input_tokens": total_cached,
                                "output_tokens": total_output,
                                "reasoning_output_tokens": total_reasoning,
                                "total_tokens": total_tokens,
                            },
                            "last_token_usage": {
                                "input_tokens": 300 + (j % 7),
                                "cached_input_tokens": 70 + (j % 3),
                                "output_tokens": 140 + (j % 5),
                                "reasoning_output_tokens": 30 + (j % 4),
                                "total_tokens": (300 + (j % 7)) + (140 + (j % 5)),
                            },
                            "model": "gpt-5.4",
                        },
                    },
                }
            )

        write_jsonl(file_path, rows)

    return home


def run_once(binary: Path, cmd: BenchCommand) -> float:
    env = os.environ.copy()
    env.update(cmd.env)
    start = time.perf_counter_ns()
    result = subprocess.run(
        [str(binary), *cmd.args],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        env=env,
        check=False,
    )
    elapsed_ms = (time.perf_counter_ns() - start) / 1_000_000.0
    if result.returncode != 0:
        raise RuntimeError(f"command failed ({cmd.name}): {result.returncode}")
    return elapsed_ms


def benchmark(binary: Path, commands: List[BenchCommand], warmup: int, iters: int) -> dict:
    samples: Dict[str, List[float]] = {c.name: [] for c in commands}

    for _ in range(warmup):
        for cmd in commands:
            run_once(binary, cmd)

    for _ in range(iters):
        for cmd in commands:
            samples[cmd.name].append(run_once(binary, cmd))

    medians = {name: statistics.median(vals) for name, vals in samples.items()}
    score = sum(medians.values())
    return {
        "score_ms": score,
        "medians_ms": medians,
        "samples_ms": samples,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Run deterministic ccstats benchmarks")
    parser.add_argument("--iters", type=int, default=5)
    parser.add_argument("--warmup", type=int, default=1)
    parser.add_argument("--claude-files", type=int, default=48)
    parser.add_argument("--claude-lines", type=int, default=1200)
    parser.add_argument("--codex-files", type=int, default=48)
    parser.add_argument("--codex-events", type=int, default=600)
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args()

    workspace = Path(__file__).resolve().parents[1]
    binary = workspace / "target" / "release" / "ccstats"
    if not binary.exists():
        subprocess.run(
            ["cargo", "build", "--release"],
            cwd=workspace,
            check=True,
        )

    bench_root = Path(tempfile.mkdtemp(prefix="ccstats-bench-"))
    try:
        claude_home = generate_claude_dataset(
            bench_root,
            files=args.claude_files,
            lines_per_file=args.claude_lines,
        )
        codex_home = generate_codex_dataset(
            bench_root,
            files=args.codex_files,
            events_per_file=args.codex_events,
        )

        commands = [
            BenchCommand(
                name="claude_daily",
                args=[
                    "daily",
                    "-j",
                    "-O",
                    "--no-cost",
                    "--timezone",
                    "UTC",
                    "--since",
                    "2026-01-01",
                    "--until",
                    "2026-12-31",
                ],
                env={"HOME": str(claude_home)},
            ),
            BenchCommand(
                name="claude_session",
                args=[
                    "session",
                    "-j",
                    "-O",
                    "--no-cost",
                    "--timezone",
                    "UTC",
                    "--since",
                    "2026-01-01",
                    "--until",
                    "2026-12-31",
                ],
                env={"HOME": str(claude_home)},
            ),
            BenchCommand(
                name="codex_daily",
                args=[
                    "codex",
                    "daily",
                    "-j",
                    "-O",
                    "--no-cost",
                    "--timezone",
                    "UTC",
                    "--since",
                    "2026-01-01",
                    "--until",
                    "2026-12-31",
                ],
                env={"CODEX_HOME": str(codex_home)},
            ),
            BenchCommand(
                name="codex_session",
                args=[
                    "codex",
                    "session",
                    "-j",
                    "-O",
                    "--no-cost",
                    "--timezone",
                    "UTC",
                    "--since",
                    "2026-01-01",
                    "--until",
                    "2026-12-31",
                ],
                env={"CODEX_HOME": str(codex_home)},
            ),
        ]

        result = benchmark(binary, commands, warmup=args.warmup, iters=args.iters)

        if args.json:
            print(json.dumps(result, ensure_ascii=True, separators=(",", ":")))
            return 0

        print(f"BENCH_SCORE_MS={result['score_ms']:.3f}")
        for name, median_ms in sorted(result["medians_ms"].items()):
            print(f"{name}_median_ms={median_ms:.3f}")
        return 0
    finally:
        shutil.rmtree(bench_root, ignore_errors=True)


if __name__ == "__main__":
    raise SystemExit(main())
