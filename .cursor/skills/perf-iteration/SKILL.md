---
name: perf-iteration
description: Run accuracy-preserving performance improvement loops for this Rust project. Use when optimizing simulation speed, reducing calculation time, benchmarking, timing release builds, or iterating on performance experiments.
---

# Performance Iteration

## Core Rule

Optimize with evidence. Before changing the algorithm, add or confirm correctness tests, capture a release-mode timing baseline, then keep only changes that preserve exact outputs and improve measured runtime.

Read [`OPTIMISATION.md`](../../OPTIMISATION.md) before proposing sim CPU work: it lists guardrails, verified timings, changes already in the tree, and experiments that regressed (threading, SIMD/SWAR, occupancy skips, and similar). Do not repeat failed approaches unless you have a new hypothesis and a fresh A/B plan.

## Workflow

1. Identify the hot path and existing invariants.
2. Add focused regression tests before optimization when behavior is not already locked down.
3. Run the test suite.
4. Capture a release-mode timing baseline.
5. Make one small optimization hypothesis at a time.
6. Re-run tests and release timing.
7. Keep the change only if correctness is unchanged and timing improves. Otherwise revert or discard it.
8. Summarize the before/after timings and any remaining risk.

## Commands For This Repo

Use the [`.cargo/config.toml`](../../.cargo/config.toml) aliases (see **cargo-dev-aliases** skill):

```bash
cargo testd
```

Run timing in release mode only:

```bash
cargo rund --release --bin time_sim
```

Optional env vars: `TIME_SIM_ITERS` (default 15), `TIME_SIM_WARMUP` (default 2). Output includes mean, median, and stdev milliseconds per case. Record results in [`OPTIMISATION.md`](../../OPTIMISATION.md) when a change is kept or definitively rejected.

If the timing harness does not exist yet, add a small binary at `src/bin/time_sim.rs` that:

- Uses `std::time::Instant`.
- Runs fixed representative workloads.
- Performs at least one warmup iteration.
- Prints machine-readable output with case name, workload size, elapsed time, and checksum.
- Includes a checksum or exact output comparison so timing cannot hide behavior changes.

## Correctness Expectations

For simulation work, preserve exact placement outputs. Prefer golden checksums over huge literal vectors, but include a small literal placement sequence for early-turn readability.

Tests should cover:

- Representative presets from `GameDefinition`.
- Multiple horizons, such as small, medium, and larger turn counts.
- Invariants such as unique placements, legal placements, and cursor/index consistency.

## Experiment Guidelines

Good experiment candidates:

- Replace hashing or allocation-heavy structures with denser representations.
- Remove repeated temporary allocations in hot loops.
- Cache derived values when the source definition is stable.
- Tighten membership checks and scanning loops.

Avoid:

- Debug-mode timing for performance conclusions.
- Combining multiple algorithm changes before measuring.
- Accepting faster results when checksums or golden outputs changed.
- Optimizing UI/rendering code when the request targets calculation speed.
- Release profile / codegen / attribute tuning (`lto`, `#[inline]`, `#[cold]`, `unsafe` hot-path tricks) as the main sim CPU strategy—sim work should stay portable for WASM (see **WASM / portable sim CPU work** in [`OPTIMISATION.md`](../../OPTIMISATION.md)).

## Reporting

Final summaries should include:

- Tests run.
- Release timing command used.
- Baseline and final timings for the representative cases.
- Confirmation that checksums or golden outputs stayed unchanged.
