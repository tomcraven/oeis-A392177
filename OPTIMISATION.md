# Optimisation Notes

Simulation performance work verified with the release timing harness. Rendering stays on the **grid-sized texture** path (one texel per visible spiral cell), not the stashed viewport-texel renderer.

## Guardrails

- Correctness: `cargo test --features bevy/dynamic_linking`
- Timing: `cargo run --release --features bevy/dynamic_linking --bin time_sim` (optional `TIME_SIM_ITERS`, `TIME_SIM_WARMUP`; reports mean/median/stdev)
- Checksums on all representative presets must stay unchanged.

## Cherry-picked from stash (sim / viewport / spiral only)

These changes are in the tree; they do **not** alter board zoom visuals:

- **`OccupancyGrid`** — dense occupancy instead of `HashMap` + separate occupied bitset.
- **`record_forbidden` fanout** — specialize 1–5 target armies; generic fallback for larger fanout.
- **Rolling `turn_order_index`** — avoids modulo each turn.
- **`advance_for_duration`** — 16ms wall-clock budget per frame (UI counters tick; sim uses available CPU).
- **Perimeter `max_visible_spiral_index`** — max spiral index from rectangle edges only (same target as full scan).
- **`spiral` multiply** — `inner_side * inner_side` instead of `pow(2)` in index conversion.
- **Forbidden-word scan skip** — when every remaining index in the current `u64` forbidden word is set, advance the cursor to the next word with `index_to_xy` instead of repeated `spiral_step`.
- **Local cursor/`xy` in `step_turn`** — scan loop mutates locals and writes `cursors`/`cursor_positions` only on placement or failure (fewer repeated vector index ops in the hot loop).
- **`range_bits_all_set`** — shared helper for forbidden word-tail tests (same bit logic as before, single implementation).
- **Scan loop word cache (2026-05-26)** — cache active forbidden `u64` across `spiral_step` advances within a word; inline same-word tail skip (no closure); `get`-style membership checks; drop unreachable `word_end == 0` branch.
- **`step_turn_scan` const-generic counter (2026-05-26)** — `COUNT_CELLS` monomorphization drops per-iteration `cells_examined` increments on the release `step_turn` path; tests still use `step_turn_inner`.
- **`ForbiddenSet::insert` hot path (2026-05-26)** — branch on `word_index < words.len()` before `resize`; late-game fanout mostly ORs into existing words.

## Left in stash (changes visuals or GPU risk)

- Viewport-sized render texture and per-screen-pixel sampling (`ViewportRenderPlan`, `grid_at_texel_center`).
- **`max_scale: 1024`** — with grid-sized textures, extreme zoom-out can exceed GPU texture limits; camera stays at `max_scale: 8.0` until rendering is viewport-limited again.

## Release timings (100k turns)

Session baseline (forbidden-word skip only, start of perf pass) vs **2026-05-25 final** after local-cursor / `range_bits_all_set` work. Five runs each, median `best_ms`; checksums unchanged.

| Case | baseline med | final med | Checksum |
| --- | ---: | ---: | --- |
| `red_black_knights` | 3.42 | **3.16** | `6661495926608663269` |
| `three_knights` | 3.57 | **3.36** | `14483324988186358612` |
| `four_classic_leapers` | 3.61 | **3.47** | `13276021962575979013` |
| `six_guards` | 4.17 | **4.17** | `4841317422612335357` |
| `fusion_piece_freeforall` | 4.62 | **4.59** | `4966002382127755860` |

Expect ~5–15% jitter on `best_ms`. Optional `RUSTFLAGS='-C target-cpu=native'` helps some presets and hurts others on Apple Silicon; not enabled in-repo.

**2026-05-26 scan word-cache A/B** (`TIME_SIM_WARMUP=2`, `TIME_SIM_ITERS=20`, three harness passes; compare avg of per-pass `median_ms` vs prior `step_turn` on `HEAD`). Checksums unchanged.

| Case | before avg median | after avg median | Δ |
| --- | ---: | ---: | ---: |
| `knight_2_pairwise` | 2.875 | **2.703** | −6.0% |
| `knight_3_clique` | 3.111 | **2.988** | −4.0% |
| `leaper_4_mixed_clique` | 3.614 | **3.401** | −5.9% |
| `guard_6_clique` | 4.312 | **3.989** | −7.5% |
| `chimera_3_clique` | 4.812 | **4.585** | −4.7% |

## What did not work (earlier experiments)

- Caching move sets inside `Simulation` — noisy regressions on multi-army presets.
- Compact `u32` occupancy — hurt simpler cases.
- Local cursor scanning — regressed smaller cases.
- Viewport-texel rendering with footprint / mode switches at 1 cell/px — moiré and zoom pops (reverted).

## What did not work (2026-05-25 iteration)

- **Cached `turn_order` on `Simulation`** — one lucky session looked ~25% faster; A/B (5 runs with vs without the cache) showed no reliable win and slightly worse median on `red_black_knights` / `three_knights` (reverted).
- Hoisting `&forbidden[army_id]` in the scan loop — no gain / noisy regression.
- Stack-buffer + batched `ForbiddenSet` inserts in `record_forbidden` — large regression on multi-army presets.
- Cursor `+= 1` instead of `saturating_add` — regression (likely codegen / overflow-check interaction).
- **Full-word `first_legal` batch scan** — scanning the whole forbidden word on every rejection regressed badly; keep only the cheap “all forbidden bits set → jump” path.
- **Occupancy-word tail skip** — `all_occupied_in_range` on the word tail (with `contains_index(next)` gating and `word_end - next >= 16`) regressed medians in A/B vs forbidden-only skip; extra slice work on rejections outweighs occasional jumps.
- **Occupancy bitset + O(1) occupied-word skip** (including short-circuit after forbidden check, and `(occ|forb)` combined mask) — regressed medians vs forbidden-only skip; extra bit work on most rejections outweighs extra jumps.
- **`placements` `with_capacity(131_072)`** — noisy, no consistent win in A/B (reverted).
- **`target-cpu=native`** — mixed across presets (reverted from repo config).

## What did not work (threading, 2026-05-26)

- **Parallel forbidden fanout** (rayon / scoped writes into disjoint `ForbiddenSet`s) — does not apply to the five bench presets (they use the specialized 1–5 target paths); on 7+ army generic fanout the spawn overhead dominates tiny bitset inserts. Unified fanout without threads also regressed vs specialized `match` arms.
- **Parallel benchmark cases** in `time_sim` — only speeds the harness, not the sim; removed.
- **Turn-level parallelism** — each `step_turn` depends on global occupancy/forbidden state and must pick the first legal spiral index in order; not safely splittable without a different algorithm.

## What did not work (SIMD / SWAR batching, 2026-05-26)

- **64-bit word batch scan in `step_turn`** — OR forbidden + occupancy masks in a u64 tail, use `trailing_ones` to skip blocked runs and jump with `index_to_xy`. Correct (checksums unchanged) but medians matched baseline within jitter; extra bitset maintenance on `OccupancyGrid` and tail work on most rejections did not pay off. Aligns with earlier failed combined-mask / occupancy-skip attempts in this file.
- **True SIMD intrinsics** — hot path is already u64 lane logic; `xy_to_index` / `spiral_step` are branchy and not batch-friendly. No separate intrinsic pass tried after SWAR showed no win.

## What did not work (2026-05-26 perf pass)

- **Forbidden check before occupancy in `step_turn`** — skip `contains_index` when the forbidden bit is set. Checksums unchanged; `cargo testd` passed. Release `time_sim` (15 iters) regressed `knight_2_pairwise` median (~2.59 ms → ~3.20 ms) with no reliable win on other bench cases (within jitter). Reverted.

## What did not work (2026-05-26 perf pass, round 2)

Session baseline for A/B: `TIME_SIM_ITERS=20`, medians `knight_2_pairwise` 2.600, `knight_3_clique` 3.044, `leaper_4_mixed_clique` 3.436, `guard_6_clique` 4.051, `chimera_3_clique` 4.604 ms. Checksums unchanged on all runs; `cargo testd` passed each time.

- **Occupancy `contains_index` with explicit `i < len` branch** — regressed `knight_2_pairwise` (~2.93 ms median); reverted.
- **Occupancy `insert` with `next_power_of_two` logical length** — mixed (multi-army medians down, `knight_2_pairwise` up ~2.95 ms); reverted.
- **Forbidden `words` geometric growth (`next_power_of_two` capacity)** — large regression on all cases (e.g. `guard_6_clique` ~6.8 ms); extra zeroed words hurt cache; reverted.
- **`#[inline(always)]` on `spiral::{index_to_xy, xy_to_index, spiral_step}`** — noisy, no consistent win; reverted.
- **Occupancy `insert` with `reserve` doubling before exact `resize`** — no win; reverted.
- **Hoist `occupancy.cells` slice in `step_turn`** — no win; reverted.
- **Idempotent `ForbiddenSet::insert` (skip if bit already set)** — extra branch on every fanout; medians up; reverted.
- **Cached `word_idx` in `step_turn` (avoid repeated `cursor >> 6`)** — mixed, `knight_2_pairwise` worse; reverted.
- **`record_forbidden` via local `&mut [ForbiddenSet]`** — no win (similar to rejected scan-loop hoisting); reverted.
- **Const `FORBIDDEN_TAIL_MASKS` lookup instead of `(1 << len) - 1`** — within jitter; reverted.
- **`usize` cursor in `step_turn` scan loop** — within jitter / slight regression on chimera; reverted.
- **Hybrid `spiral_step` chain vs `index_to_xy` on forbidden tail skip (`dist <= 8`)** — within jitter; reverted.

`sample(1)` on release `time_sim` shows ~all CPU in `Simulation::step_turn`; `record_forbidden` / `index_to_xy` barely appear separately (inlined or cold vs scan).

## `step_turn` scan rejections (2026-05-26)

Hypothesis: late-game clique presets might reject **thousands** of spiral cells per turn, making a dynamic “next legal index” structure worthwhile.

**Measurement:** `scan_rejection_late_game_presets_and_random` in `src/sim.rs` (`step_turn_inner` counts loop iterations; on success **rejections = cells_examined − 1**). Forbidden word-tail skips count as **one** iteration, matching CPU work rather than raw spiral index distance.

**Run report:**

```bash
cargo test --release --features bevy/dynamic_linking scan_rejection_late_game -- --nocapture
```

**Findings (release):** Five bench presets at **100k** turns; random `RandomGenConfig` shapes × 3 seeds at **20k** turns. Stats over the **last 1000** turns (p50 / p99) plus global max over the full run.

| Case | max rej. | late p99 | mean |
| --- | ---: | ---: | ---: |
| `knight_2_pairwise` | 108 | 23 | 1.7 |
| `knight_3_clique` | 192 | 39 | 2.0 |
| `guard_6_clique` | 118 | 17 | 2.0 |
| `chimera_3_clique` | 92 | 4 | 1.4 |
| Random (worst of 12) | **286** | ≤62 | ~1–5 |

**Never ≥1000 rejections/turn** in this survey (test asserts `global_max < 1000`). Typical turns are 0–1 rejection (late p50 ≈ 1). Cost is ~100k turns × a **short** scan loop plus placement fanout, not long rejection runs—so successor/rank-select bitstructures are unlikely to beat the current scan.

## Hot-path heap allocations (2026-05-26)

**`step_turn` scan loop:** no heap allocation (only stack locals and bit/occupancy reads).

**`place()` (once per successful turn):** may extend `occupancy.cells`, each affected `forbidden[].words`, and `placements` via `push`. Until backing `capacity()` reaches the run’s max spiral index / word count, those extensions can reallocate—typically tens of times over the first 100k turns (`hot_path_vec_capacity_growth_is_bounded` prints the count with `--nocapture`), then **zero** capacity growth for subsequent turns and after `reset()` (buffers are cleared, not dropped).

**Changes kept:**

- **`Simulation::reset`** — `clear()` occupancy, placements, and forbidden words; reuse cursor vectors with `fill` when army count unchanged (avoids fresh `Vec` allocs on preset reload).

**Not kept (release timing regression):** power-of-two `reserve` before `resize` in `insert` (`grow_to_len`), upfront `placements`/`occupancy`/`forbidden` pre-reservation—large reserved arenas hurt cache/TLB despite fewer `realloc` calls.

**Regression test:** `hot_path_vec_capacity_growth_is_bounded` (`cargo testd hot_path_vec_capacity -- --nocapture`).

## Future work

- Safe far zoom-out needs a viewport-limited **render** path, not only a higher `max_scale`.
- Exact simulation cannot skip offscreen spiral history; only the **target index** may be capped to the visible rectangle (+ margin).
- Occupancy fast paths need a bitset **and** a skip path that is not evaluated on every rejection when it cannot fire (e.g. only when `contains_index(next)` and word-aligned occupied mask is full — still regressed when tried with maintained bitset).
- No further low-risk CPU wins obvious without deeper profiling (`sample`/`perf`) on `step_turn` codegen (scan vs `place` / inlined `record_forbidden`). Scan-rejection survey shows per-turn rejections stay **O(1)** (max ~286 in tested configs), not thousands—see **`step_turn` scan rejections** above.
- **2026-05-26 session baseline** (`cargo rund --release --bin time_sim`, 15 iters): `knight_2_pairwise` med 2.589 ms, `knight_3_clique` 3.041, `leaper_4_mixed_clique` 3.463, `guard_6_clique` 4.011, `chimera_3_clique` 4.719 ms; checksums match golden table above.
- **2026-05-26 round-2 baseline** (`TIME_SIM_ITERS=20`): medians 2.600 / 3.044 / 3.436 / 4.051 / 4.604 ms (same five cases); twelve new hypotheses tried, none kept (see round 2 above).

## What did not work (2026-05-26 perf pass, round 3)

Session baseline for A/B: `TIME_SIM_ITERS=20`, `TIME_SIM_WARMUP=2`, medians `knight_2_pairwise` 2.988, `knight_3_clique` 3.021, `leaper_4_mixed_clique` 3.424, `guard_6_clique` 4.004, `chimera_3_clique` 4.595 ms. Checksums unchanged on all runs; `cargo testd` passed each time.

- **Occupancy `contains_index` via explicit `idx < len` branch** — `knight_2_pairwise` median up (~3.11 ms); reverted (same idea as round-2 explicit branch regression).
- **Proactive forbidden word-tail skip at loop head** (jump before examining `cursor` when the rest of the word is all forbidden) — extra mask work every iteration; medians up on multi-army cases (e.g. `guard_6_clique` ~4.31 ms, `chimera_3_clique` ~4.88 ms); reverted.
- **`forb_word == 0` inner scan loop** (occupancy-only until word boundary) — mixed; clique presets worse despite slightly faster `knight_2_pairwise`; reverted.
- **`#[cold]` on `place`** — within jitter / slight chimera regression; reverted.
- **Occupancy `insert` with `index < len` branch** — noisy vs forbidden-only insert win alone; reverted.

**Kept (round 3 A/B):** `step_turn_scan::<false>` (no release counter) + `ForbiddenSet::insert` existing-word branch. Confirming run medians: 2.808 / 2.836 / 3.243 / 3.859 / 4.371 ms (−4–6% vs session baseline above).
