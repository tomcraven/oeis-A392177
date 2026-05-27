#!/usr/bin/env python3
"""
Compare perf/python_reference.py to perf_knight2 (Rust) and write results under perf/results/.
"""

from __future__ import annotations

import json
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PERF = ROOT / "perf"
RESULTS = PERF / "results"
PYTHON = PERF / "python_reference.py"

# Same turn counts as sim.rs GOLDEN_TURNS plus a few bench sizes.
ACCURACY_TURNS = [64, 1_024, 10_000, 100_000, 500_000]
BENCH_TURNS = [10_000, 100_000, 500_000]
RUST_WARMUP = 1
RUST_ITERS = 5
PYTHON_ITERS = 3


def run_python(turns: int) -> dict:
    proc = subprocess.run(
        [sys.executable, str(PYTHON), str(turns), "--json"],
        check=True,
        capture_output=True,
        text=True,
        cwd=ROOT,
    )
    return json.loads(proc.stdout.strip())


def cargo_release_bin(*args: str) -> dict:
    proc = subprocess.run(
        ["cargo", "run", "--release", "-q", "--bin", "perf_knight2", "--", *args],
        check=True,
        capture_output=True,
        text=True,
        cwd=ROOT,
    )
    line = proc.stdout.strip().splitlines()[-1]
    return json.loads(line)


def bench_rust(turns: int) -> dict:
    for _ in range(RUST_WARMUP):
        cargo_release_bin(str(turns))
    samples: list[float] = []
    last: dict | None = None
    for _ in range(RUST_ITERS):
        last = cargo_release_bin(str(turns))
        samples.append(last["elapsed_s"])
    samples.sort()
    return {
        "turns": turns,
        "checksum": last["checksum"] if last else None,
        "elapsed_s_best": samples[0],
        "elapsed_s_median": samples[len(samples) // 2],
        "elapsed_s_mean": sum(samples) / len(samples),
        "iters": RUST_ITERS,
    }


def bench_python(turns: int) -> dict:
    samples: list[float] = []
    last: dict | None = None
    for _ in range(PYTHON_ITERS):
        last = run_python(turns)
        samples.append(last["elapsed_s"])
    samples.sort()
    return {
        "turns": turns,
        "checksum": last["checksum"] if last else None,
        "elapsed_s_best": samples[0],
        "elapsed_s_median": samples[len(samples) // 2],
        "elapsed_s_mean": sum(samples) / len(samples),
        "iters": PYTHON_ITERS,
    }


def main() -> int:
    RESULTS.mkdir(parents=True, exist_ok=True)

    accuracy_rows = []
    for turns in ACCURACY_TURNS:
        py = run_python(turns)
        rs = cargo_release_bin(str(turns))
        match = py["checksum"] == rs["checksum"]
        accuracy_rows.append(
            {
                "turns": turns,
                "python_checksum": py["checksum"],
                "rust_checksum": rs["checksum"],
                "match": match,
                "black_last_index": py["black_last_index"],
                "red_last_index": py["red_last_index"],
            }
        )
        status = "ok" if match else "MISMATCH"
        print(f"accuracy turns={turns} {status} checksum={py['checksum']}")
        if not match:
            return 1

    accuracy_doc = {
        "preset": "knight_2_pairwise",
        "reference": "src/a392177_2.py logic in perf/python_reference.py",
        "rust_bin": "perf_knight2",
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "cases": accuracy_rows,
        "rust_golden_turns_note": "64/1024/10000 checksums are locked in sim.rs representative_preset_checksums_are_stable",
    }
    (RESULTS / "accuracy.json").write_text(
        json.dumps(accuracy_doc, indent=2) + "\n", encoding="utf-8"
    )

    bench_cases = []
    for turns in BENCH_TURNS:
        print(f"benchmark turns={turns} …")
        py_b = bench_python(turns)
        rs_b = bench_rust(turns)
        ratio = py_b["elapsed_s_median"] / rs_b["elapsed_s_median"]
        bench_cases.append(
            {
                "turns": turns,
                "python": py_b,
                "rust": rs_b,
                "median_speedup_rust_vs_python": ratio,
            }
        )
        print(
            f"  python median {py_b['elapsed_s_median']:.4f}s  "
            f"rust median {rs_b['elapsed_s_median']:.4f}s  "
            f"speedup {ratio:.1f}x"
        )

    bench_doc = {
        "preset": "knight_2_pairwise",
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "rust_warmup": RUST_WARMUP,
        "rust_iters": RUST_ITERS,
        "python_iters": PYTHON_ITERS,
        "cases": bench_cases,
    }
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    (RESULTS / "benchmark.json").write_text(
        json.dumps(bench_doc, indent=2) + "\n", encoding="utf-8"
    )
    (RESULTS / f"benchmark_{stamp}.json").write_text(
        json.dumps(bench_doc, indent=2) + "\n", encoding="utf-8"
    )

    print(f"wrote {RESULTS / 'accuracy.json'}")
    print(f"wrote {RESULTS / 'benchmark.json'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
