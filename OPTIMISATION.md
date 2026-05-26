# Optimisation Notes

Simulation performance work verified with the release timing harness. Rendering stays on the **grid-sized texture** path (one texel per visible spiral cell), not the stashed viewport-texel renderer.

## Guardrails

- Correctness: `cargo test --features bevy/dynamic_linking`
- Timing: `cargo run --release --features bevy/dynamic_linking --bin time_sim` (optional `TIME_SIM_ITERS`, `TIME_SIM_WARMUP`; reports mean/median/stdev)
- Checksums on all representative presets must stay unchanged.
- **WASM / portable sim CPU work:** Prefer algorithm and data-structure changes that behave the same on native and `wasm32-unknown-unknown`. Do **not** treat release profile tweaks (`lto`, `codegen-units`), `#[inline]` / `#[cold]`, or `unsafe` micro-hacks as the primary optimisation path—they may not apply to wasm shipping profiles (`wasm-release`, `wasm-release-fast`) and are easy to mis-read on native-only A/B. Validate meaningful wins on native `time_sim` first; for wasm-specific regressions, use the wasm release profile when investigating.

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
- **`step_turn_scan` const-generic counter (2026-05-26)** — `COUNT_CELLS` monomorphization drops per-iteration `cells_examined` increments on the release `step_turn` path; rejection diagnostics call `step_turn_scan::<true>` from tests only.
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
| `king_6_clique` | 4.17 | **4.17** | `4841317422612335357` |
| `fusion_piece_freeforall` | 4.62 | **4.59** | `4966002382127755860` |

Expect ~5–15% jitter on `best_ms`. Optional `RUSTFLAGS='-C target-cpu=native'` helps some presets and hurts others on Apple Silicon; not enabled in-repo.

**2026-05-26 scan word-cache A/B** (`TIME_SIM_WARMUP=2`, `TIME_SIM_ITERS=20`, three harness passes; compare avg of per-pass `median_ms` vs prior `step_turn` on `HEAD`). Checksums unchanged.

| Case | before avg median | after avg median | Δ |
| --- | ---: | ---: | ---: |
| `knight_2_pairwise` | 2.875 | **2.703** | −6.0% |
| `knight_3_clique` | 3.111 | **2.988** | −4.0% |
| `leaper_4_mixed_clique` | 3.614 | **3.401** | −5.9% |
| `king_6_clique` | 4.312 | **3.989** | −7.5% |
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

Session baseline for A/B: `TIME_SIM_ITERS=20`, medians `knight_2_pairwise` 2.600, `knight_3_clique` 3.044, `leaper_4_mixed_clique` 3.436, `king_6_clique` 4.051, `chimera_3_clique` 4.604 ms. Checksums unchanged on all runs; `cargo testd` passed each time.

- **Occupancy `contains_index` with explicit `i < len` branch** — regressed `knight_2_pairwise` (~2.93 ms median); reverted.
- **Occupancy `insert` with `next_power_of_two` logical length** — mixed (multi-army medians down, `knight_2_pairwise` up ~2.95 ms); reverted.
- **Forbidden `words` geometric growth (`next_power_of_two` capacity)** — large regression on all cases (e.g. `king_6_clique` ~6.8 ms); extra zeroed words hurt cache; reverted.
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

**Measurement:** `scan_rejection_late_game_presets_and_random` in `src/sim.rs` (`step_turn_scan::<true>` counts loop iterations; on success **rejections = cells_examined − 1**). Forbidden word-tail skips count as **one** iteration, matching CPU work rather than raw spiral index distance.

**Run report:**

```bash
cargo test --release --features bevy/dynamic_linking scan_rejection_late_game -- --nocapture
```

**Findings (release):** Five bench presets at **100k** turns; random `RandomGenConfig` shapes × 3 seeds at **20k** turns. Stats over the **last 1000** turns (p50 / p99) plus global max over the full run.

| Case | max rej. | late p99 | mean |
| --- | ---: | ---: | ---: |
| `knight_2_pairwise` | 108 | 23 | 1.7 |
| `knight_3_clique` | 192 | 39 | 2.0 |
| `king_6_clique` | 118 | 17 | 2.0 |
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
- **2026-05-26 session baseline** (`cargo rund --release --bin time_sim`, 15 iters): `knight_2_pairwise` med 2.589 ms, `knight_3_clique` 3.041, `leaper_4_mixed_clique` 3.463, `king_6_clique` 4.011, `chimera_3_clique` 4.719 ms; checksums match golden table above.
- **2026-05-26 round-2 baseline** (`TIME_SIM_ITERS=20`): medians 2.600 / 3.044 / 3.436 / 4.051 / 4.604 ms (same five cases); twelve new hypotheses tried, none kept (see round 2 above).

## What did not work (2026-05-26 perf pass, round 3)

Session baseline for A/B: `TIME_SIM_ITERS=20`, `TIME_SIM_WARMUP=2`, medians `knight_2_pairwise` 2.988, `knight_3_clique` 3.021, `leaper_4_mixed_clique` 3.424, `king_6_clique` 4.004, `chimera_3_clique` 4.595 ms. Checksums unchanged on all runs; `cargo testd` passed each time.

- **Occupancy `contains_index` via explicit `idx < len` branch** — `knight_2_pairwise` median up (~3.11 ms); reverted (same idea as round-2 explicit branch regression).
- **Proactive forbidden word-tail skip at loop head** (jump before examining `cursor` when the rest of the word is all forbidden) — extra mask work every iteration; medians up on multi-army cases (e.g. `king_6_clique` ~4.31 ms, `chimera_3_clique` ~4.88 ms); reverted.
- **`forb_word == 0` inner scan loop** (occupancy-only until word boundary) — mixed; clique presets worse despite slightly faster `knight_2_pairwise`; reverted.
- **`#[cold]` on `place`** — within jitter / slight chimera regression; reverted.
- **Occupancy `insert` with `index < len` branch** — noisy vs forbidden-only insert win alone; reverted.

**Kept (round 3 A/B):** `step_turn_scan::<false>` (no release counter) + `ForbiddenSet::insert` existing-word branch. Confirming run medians: 2.808 / 2.836 / 3.243 / 3.859 / 4.371 ms (−4–6% vs session baseline above).

## What did not work (2026-05-26 perf pass, round 4)

Session baseline for A/B: `TIME_SIM_ITERS=20`, `TIME_SIM_WARMUP=2`, medians `knight_2_pairwise` 2.582, `knight_3_clique` 2.933, `leaper_4_mixed_clique` 3.306, `king_6_clique` 3.893, `chimera_3_clique` 4.426 ms. Checksums unchanged on all runs; `cargo testd` passed each time.

- **Cached `forbidden.words` slice + `word_idx` in `step_turn_scan`** (direct indexing vs `word_bits`, compare on word change) — mixed; `knight_2_pairwise` median up (~2.90 ms); reverted.
- **Same-word batched OR in `ForbiddenSet::insert_moves_from_xy`** — only helps single-target fanout; bench presets are mostly multi-target; `knight_2_pairwise` regressed (~3.08 ms); reverted.
- **Skip forbidden tail-skip work when `forb_word == 0`** (defer `word_end` / mask math until the active word has any forbidden bit) — noisy across three harness passes; medians often matched or exceeded baseline (e.g. `king_6_clique` ~4.12 ms); reverted.
- **Tail-skip only when `word_end - next > 1`** — regressed multi-army medians; reverted.
- **`ForbiddenSet::insert` via `get_unchecked_mut` on hot path** — within jitter / slight regression vs existing-word branch; reverted.
- **`#[inline(never)]` on `place` / `record_forbidden`** — clear regression (e.g. `king_6_clique` ~4.08 ms); reverted.
- **Forbidden membership via `(forb_word >> (cursor & 63)) & 1`** instead of `1 << bit` — mixed (`knight_2_pairwise` worse, some clique cases better); reverted.
- **Occupancy `insert` with `index < len` branch (retry on post–round-3 tree)** — multi-army medians up; reverted.

**Kept (round 4):** none — tree unchanged vs round 3 commit.

## What did not work (2026-05-26 perf pass, round 5 — WASM-safe scope)

Session baseline for A/B: `TIME_SIM_ITERS=20`, `TIME_SIM_WARMUP=2`, medians `knight_2_pairwise` 2.988, `knight_3_clique` 2.866, `leaper_4_mixed_clique` 3.250, `king_6_clique` 3.846, `chimera_3_clique` 4.500 ms. Checksums unchanged; `cargo testd` passed.

- **`ForbiddenSet::insert` growth via `push(bit)` when `word_index == words.len()`** (avoid `resize` for the next word only) — large regression on all bench cases (e.g. `king_6_clique` ~7 ms); repeated `push` realloc vs batched `resize`; reverted.
- **Toolchain / codegen experiments (out of scope for wasm parity):** `lto = "fat"`, `#[inline(always)]` on hot helpers — not pursued; native-only and not the intended optimisation surface.

**Kept (round 5):** none. `Cargo.toml` release profile remains `lto = "thin"`, `codegen-units = 1`.

## What did not work (2026-05-26 perf pass, round 6)

Session baseline for A/B: `TIME_SIM_ITERS=20`, `TIME_SIM_WARMUP=2`, medians `knight_2_pairwise` 2.691, `knight_3_clique` 2.938, `leaper_4_mixed_clique` 3.299, `king_6_clique` 3.962, `chimera_3_clique` 4.419 ms. Checksums unchanged on all runs; `cargo testd` passed each time.

- **`OccupancyGrid` occupied-word bitset for `contains_index`** (mirror `ForbiddenSet` words; no scan skip) — `knight_2_pairwise` within jitter; multi-army medians up (e.g. `leaper_4_mixed_clique` ~3.54 ms, `chimera_3_clique` ~4.56 ms); extra insert work and second vector growth; reverted.
- **`turn_order_index` via `% turn_order_len`** instead of increment + wrap branch — within jitter / noisy (`knight_2_pairwise` ~2.81 ms); reverted.
- **Hoist `ForbiddenSet::words` slice in `step_turn_scan`** (direct `get` on slice vs `word_bits`) — mixed across cases, no reliable win vs baseline; reverted.
- **`contains_index` via `is_some_and`** — within jitter / `knight_2_pairwise` worse (~2.82 ms); reverted.

**Kept (round 6):** none — tree unchanged vs round 5.

## `place()` profiling (2026-05-26)

Harness: `cargo run --release --features place_profile,bevy/dynamic_linking --bin profile_place` (no timing hooks on benchmark paths; counters only during one collection pass).

Method: median of 5× full `step_turn` runs vs 5× **place-only replay** on the same 100k placement stream; subtract for scan estimate. Component microbenches replay captured work on a **fresh** sim (isolated `xy_to_index`, `ForbiddenSet`-style OR, occupancy insert).

**Findings (representative presets, 100k turns, checksums unchanged):**

| Case | step (ms) | place replay (ms) | scan est. (%) | forb OR / place | xy→index / place | occ / place |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `knight_2_pairwise` | ~4.5 | ~3.1 | ~30% | ~15% | ~48% | ~4% |
| `knight_3_clique` | ~5.9 | ~4.6 | ~22% | ~47% | ~32% | ~2% |
| `leaper_4_mixed_clique` | ~7.6 | ~6.5 | ~15% | ~65% | ~23% | ~1% |
| `king_6_clique` | ~10.6 | ~9.6 | ~10% | ~60% | ~12% | ~1% |
| `chimera_3_clique` | ~10.5 | ~9.5 | ~8% | ~45% | ~25% | ~1% |

- **Scan is not negligible** on two-army presets (~25–30% of `step_turn` at ~2.7 cells examined per placement); clique presets trend **~8–15%** scan at ~2.4–3.0 cells/placement.
- **Within `place`, `record_forbidden` dominates** multi-army cliques: forbidden bit OR scales with `moves × threatened_targets` (e.g. 40 inserts/turn for `king_6_clique` = 8 king moves × 5 targets). `xy_to_index` is a large share only when fanout is small (pairwise).
- **Occupancy `insert` and `placements.push`** are minor vs forbidden fanout (~1–4% in isolation); remaining gap vs summed components is match dispatch, multi-`ForbiddenSet` writes, and cache/TLB on real state (~10–35% of place replay depending on preset).
- macOS `sample` on release `time_sim` still shows almost all time under `step_turn` (inlined `place` / `record_forbidden` rarely appear as separate frames).

Further `place` wins likely need **fewer forbidden writes** (algorithmic dedup or batching that prior experiments rejected) or **cheaper fanout**, not occupancy or push tuning.

## Forbidden fanout investigation (2026-05-26)

Extended `profile_place` (same harness; enable with `place_profile` feature only) records each `(target_army, index)` insert and classifies `ForbiddenSet::insert` as **existing-word OR** vs **new-word resize**, plus **bit already set** before OR.

### Structural fanout (by design)

- Cost scales **`valid_moves.len() × threatened_targets(attacker)`** per placement (specialized `match` already computes each attacked index once per move).
- **Cross-target duplication** is exact: e.g. 8 unique attacked cells × 5 targets ⇒ 32 duplicate inserts/placement and **5.00×** fanout multiplier (`king_6_clique`). Not fixable without changing rules or sharing one bitset across armies (breaks per-army scan).

### Insert path over 100k turns

| Case | inserts | existing-word OR | new-word resize | bit already set |
| --- | ---: | ---: | ---: | ---: |
| `knight_2_pairwise` | 800k | 99.6% | 0.4% | **~85%** |
| `king_6_clique` | 4M | 99.8% | 0.2% | **~86%** |
| `chimera_3_clique` | 3.2M | 99.9% | 0.1% | **~93%** |

- **`resize` is not the problem** late in a run; almost all work is OR into existing words.
- **~85–93% of ORs touch a bit that is already 1** (overlap from earlier placements re-marking the same attacked cells). CPU still does the OR; an idempotent skip would avoid memory writes but **`ForbiddenSet::insert` skip-if-set regressed medians** in round 2 (branch cost).

### Multi-army vs single bitset (isolated replay)

Replaying the captured insert stream into **one** harness bitset vs **one per army** (sim layout):

- **`king_6_clique`**: one set ~6.3 ms vs six sets ~2.8 ms — scattering across smaller per-army vectors can be **cheaper** than one monolithic bitset for the same insert stream (cache / working-set size).
- **`knight_2_pairwise`**: two sets ~61% slower than one set at equal insert count — low army count favors unified working set.

Real `place` still pays **`record_forbidden` dispatch**, `xy_to_index`, and sequential `forbidden[target]` indexing on top of raw OR cost.

### Likely optimization directions (evidence-backed)

1. **Do not chase word growth** — negligible after warm-up.
2. **Idempotent insert** — high redundant-bit rate suggests *possible* win, but prior A/B failed; any retry needs branchless or “likely already set” profiling (late-game conditioned).
3. **Reduce fanout only via semantics** — fewer threatened targets or merged forbidden storage (major sim change).
4. **Same-move batching across targets** — already shares `attacked`; further reduction means **one write propagates to all targets** (e.g. shared layer + per-army view), not more micro-OR tricks.
5. Rejected in tree and still aligned with data: **parallel fanout**, **batched stack inserts**, **same-word batched OR** (round 4).

## What did not work (2026-05-26 perf pass, round 7 — profile-guided)

Session baseline for A/B: `TIME_SIM_ITERS=20`, `TIME_SIM_WARMUP=2`, medians `knight_2_pairwise` 3.303, `knight_3_clique` 4.113, `leaper_4_mixed_clique` 5.006, `king_6_clique` 7.272, `chimera_3_clique` 6.935 ms. Checksums unchanged; `cargo testd` passed.

- **Skip forbidden OR when bit already set** (fanout survey showed ~85–93% redundant touches) — medians up on pairwise/leaper (e.g. `knight_2_pairwise` ~3.47 ms); branch cost outweighs skipped stores; reverted (same class as round-2 idempotent insert).
- **`xy_to_index` lookup table** (`R=448`, ~3.2 MiB static, load vs formula) — all bench medians regressed (e.g. `chimera_3_clique` ~7.46 ms); reverted.

**Kept (round 7):** `ForbiddenSet::insert` only loads the word to test “already set” when built with **`place_profile`** (profiling counters). Default/release builds no longer do that extra read on every insert.

## Shared forbidden representation — attack layers (2026-05-26, kept)

**Idea:** Store one cumulative `ForbiddenSet` per **attacker** (`attack_layers[a]`) instead of fanning each attacked cell into every defender’s bitset on `place`. On scan, army `d` ORs `attack_layers[a].word(w)` for each attacker `a` that `d` respects (`blocked_by`).

Semantically `forbidden[d]` equals `⋁_{a ∈ respected(d)} attack_layers[a]`; golden checksums unchanged.

**Place:** `moves` inserts per turn (not `moves × targets`). **Scan:** up to `|respected(d)|` word loads + ORs per examined cell (specialized for 1–5 attackers).

**`time_sim` medians** (`TIME_SIM_ITERS=20`, `TIME_SIM_WARMUP=2`) before layers vs after:

| Case | before (round 7) | after layers | Δ |
| --- | ---: | ---: | ---: |
| `knight_2_pairwise` | 3.170 | **2.910** | −8% |
| `knight_3_clique` | 4.210 | **2.755** | −35% |
| `leaper_4_mixed_clique` | 5.076 | **2.770** | −45% |
| `king_6_clique` | 7.260 | **2.856** | −61% |
| `chimera_3_clique` | 6.990 | **4.030** | −42% |

Confirming run medians after layers: 2.910 / 2.755 / 2.770 / 2.856 / 4.030 ms.

Harness: `cargo rund --release --bin time_sim`; `profile_place` insert counts are now per-layer (see `inserts_per_placement = moves`).
