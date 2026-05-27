#!/usr/bin/env bash
# Compare Python reference (a392177_2) to Rust sim and record timings under perf/results/.
set -euo pipefail
cd "$(dirname "$0")/.."
exec python3 perf/compare_and_bench.py
