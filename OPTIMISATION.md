# Optimisation Notes

Simulation performance work verified with the release timing harness. Rendering stays on the **grid-sized texture** path (one texel per visible spiral cell), not the stashed viewport-texel renderer.

## Guardrails

- Correctness: `cargo test --features bevy/dynamic_linking`
- Timing: `cargo run --release --features bevy/dynamic_linking --bin time_sim` (optional `TIME_SIM_ITERS`, `TIME_SIM_WARMUP`; reports mean/median/stdev)
- Checksums on all representative presets must stay unchanged.
- **WASM / portable sim CPU work:** Prefer algorithm and data-structure changes that behave the same on native and `wasm32-unknown-unknown`. Do **not** treat release profile tweaks (`lto`, `codegen-units`), `#[inline]` / `#[cold]`, or `unsafe` micro-hacks as the primary optimisation path—they may not apply to wasm shipping profiles (`wasm-release`, `wasm-release-fast`) and are easy to mis-read on native-only A/B. Validate meaningful wins on native `time_sim` first; for wasm-specific regressions, use the wasm release profile when investigating.
- **Interactive app profiling:** optional Cargo feature `app_profile` (not enabled in default release). Run scripted scenarios with `cargo run --release --features app_profile --bin perf_app -- --perf-scenario zoom_out_catchup` (built-ins: `origin_settled`, `pan_east`, `zoom_out_catchup`, `pan_render_stress`, or path to scenario `.toml`). Prints per-frame ms for `sync_viewport`, `render_raster`, `render_image_write`, `render_sprite_layout`, and worker `display_clone`. Headless raster A/B: `cargo run --release --bin perf_render` (optional `PERF_RENDER_ITERS`, `PERF_RENDER_TURNS`). Unit tests in `render` and `perf_harness` cover raster checksums and script determinism without a window.

## Cherry-picked from stash (sim / viewport / spiral only)

These changes are in the tree; they do **not** alter board zoom visuals:

- **`OccupancyGrid`** — dense occupancy instead of `HashMap` + separate occupied bitset; **`Arc<Vec<ArmyId>>`** so worker→UI snapshots are shallow clones (COW before sim mutation when shared).
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
- **`ActiveTurnOrder` (2026-05-27)** — `active_turn_order()` returns a lazy view (`iter` / `get` / `len`) instead of allocating a `Vec`; dense path indexes `turn_order` when every entry is enabled.

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

## Post–attack-layers scan vs place (2026-05-26)

Method: `cargo run --release --features place_profile,bevy/dynamic_linking --bin profile_place` — median of 5× full `step_turn` vs 5× place-only replay on the same 100k stream; `scan_est = step − place_replay`. `combined_forbidden_word` invocations counted during the profiled collect pass (`forb_combines_per_cell` = combines ÷ cells examined).

**`time_sim` medians** (`TIME_SIM_ITERS=20`, `TIME_SIM_WARMUP=2`, same session): 2.476 / 2.681 / 2.723 / 2.810 / 3.960 ms (checksums unchanged).

| Case | step (profile) | place replay | scan est. | scan % | cells/place | layer ins/place | forb combine/cell |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `knight_2_pairwise` | ~4.04 | ~2.58 | ~1.45 | ~36% | ~2.67 | 8 | ~0.39 |
| `knight_3_clique` | ~3.43 | ~2.30 | ~1.13 | ~33% | ~2.95 | 8 | ~0.36 |
| `leaper_4_mixed_clique` | ~3.61 | ~2.26 | ~1.34 | ~37% | ~2.94 | 8 | ~0.36 |
| `king_6_clique` | ~3.58 | ~2.42 | ~1.15 | ~32% | ~2.97 | 8 | ~0.37 |
| `chimera_3_clique` | ~4.90 | ~3.87 | ~1.04 | ~21% | ~2.36 | 16 | ~0.44 |

**Place replay (isolated components, median):** `xy_to_index` dominates knight/leaper/king presets (~59–63% of place replay); layer `ForbiddenSet::insert` ~18–20% (single-set replay); chimera ~72% xy / ~32% forb (16 moves). Occupancy ~2–4%.

**Takeaways:** Forbidden write fanout is gone; **scan is ~⅓ of step** on multi-army benches (up from ~8–15% pre-layers). `forb_combines_per_cell` ≈ **1 ÷ cells_per_place** (~0.36–0.44): one `combined_forbidden_word` at scan start each turn, plus occasional word-boundary/tail-skip reloads — not one OR per examined cell. Scan time is mostly **occupancy check + spiral_step + index_to_xy**, not layer OR. Further sim CPU without algorithm change targets the scan loop, not place fanout. **`chimera_3_clique`** remains slowest via **2× move count** on place, not scan.

## What did not work (2026-05-26 perf pass, round 8 — profile-guided)

Session baseline for A/B: `TIME_SIM_ITERS=20`, `TIME_SIM_WARMUP=2`, medians `knight_2_pairwise` 3.084, `knight_3_clique` 2.720, `leaper_4_mixed_clique` 2.761, `king_6_clique` 2.866, `chimera_3_clique` 4.054 ms. Checksums unchanged; `cargo testd -p red_black_knights --lib` passed.

- **Within-word forbidden-run skip** (`trailing_ones` on `forb_word >> pos`, jump with `index_to_xy` when `run > 1`, threshold `run > 4` also tried) — mixed / within jitter vs baseline (`knight_2_pairwise` occasionally ~3–8% faster, `chimera_3_clique` often slightly slower); extra `index_to_xy` on short runs; reverted.
- **`record_forbidden` match on `moves.len()` 8 / 16** (indexed loop + hoisted `attack_layers` mut ref) — medians up on several cases (e.g. `chimera_3_clique` ~4.21 ms); reverted.

**Kept (round 8):** `place_profile` scan reject counters — `scan_forbidden_tail_skips`, `scan_single_step_rejects` (feature `place_profile` only); `profile_place` prints tail vs single-step reject rates.

**Scan reject breakdown** (post–attack-layers, `profile_place` collect pass, 100k turns):

| Case | cells/place | tail_skip/place | single_step/place |
| --- | ---: | ---: | ---: |
| `knight_2_pairwise` | 2.67 | 0.018 | 1.65 |
| `king_6_clique` | 2.97 | 0.084 | 1.89 |
| `chimera_3_clique` | 2.36 | 0.035 | 1.33 |

Most rejections advance one index via `spiral_step`; full-word forbidden tail jumps are rare. Partial forbidden runs are uncommon enough that `trailing_ones` jumps did not pay for their `index_to_xy` cost.

**Next targets (unchanged):** `xy_to_index` in `record_forbidden` (~60–72% of place replay); scan loop single-step + occupancy (no low-risk micro-optim left after rounds 1–7 and round 8 above).

## Attack indexing investigation (2026-05-26)

**Question:** Can we mark attacked cells without one full [`xy_to_index`](src/spiral.rs) per move per placement?

**Harness:** `cargo test -p red_black_knights --release --features bevy/dynamic_linking --lib attack_indexing_survey -- --nocapture` (survey + fair LUT microbench). [`RingOffset`](src/spiral.rs) helpers round-trip with `index` / `xy` (same geometry as `xy_to_index`, not faster by themselves).

### Geometry facts (100k placements)

| Preset | max \|x\|,\|y\| | same-ring attacks | \|Δring\| ≤ 1 | unique (ring, move)→Δ with conflicts |
| --- | ---: | ---: | ---: | ---: |
| `knight_2_pairwise` | 165 | 0.3% | 50% | 1328 keys, **663k** conflicts |
| `king_6_clique` | 164 | 25% | **100%** | 1320 keys, **550k** conflicts |
| `chimera_3_clique` | 164 | 13% | 50% | 2640 keys, **1.2M** conflicts |

- **Index delta is never global:** 800k attacks ⇒ 800k distinct `(placement_index, move_slot) → index Δ` pairs.
- **Perimeter position matters:** `(ring, offset, move)` is injective (800k unique, 0 conflicts) — each occupied spiral cell has its own attack index set.
- **Ring-only tables are wrong:** the same ring + move slot yields different index Δ depending on where you are on that ring (hundreds of thousands of conflicts).

### Strategies evaluated

| Approach | Verdict |
| --- | --- |
| **Lazy cache keyed by placement index** | **No win** — each spiral index is occupied at most once, so no reuse within a run. |
| **Runtime memo `(ring, move)`** | **Invalid** — see conflicts above. |
| **Ring/offset parametric OR** | Same branches as `xy_to_index`; no cheaper closed form for arbitrary leaper offsets. |
| **Coordinate LUT** (`R=448`, prior round) | **Regressed** — likely cache footprint / access pattern, not lack of coverage (max coord ≈ 165 at 100k). |
| **Precomputed `attacks[index][move]` table** | **Lookup ~15× faster** than live `xy_to_index` on replay (0.1 ms vs 1.5 ms for 800k knight attacks), but **building** all indices `0..=max_index` costs ~1.8 ms once; **building only placed indices** costs the same 800k `xy_to_index` as today. **Net:** only helps if the table is **precomputed offline** (build script / `include_bytes!`) and shipped — not from lazy runtime fill. |
| **Idempotent layer `insert` / skip OR** | Already rejected (branch cost). |

### Fair microbench (knight, 100k placements, `max_index ≈ 109390`)

- Live `record_forbidden` path (8× `xy_to_index` from cached placement `xy`): **~1.5 ms** for 800k conversions in isolation.
- Replay from prebuilt `table[index][0..8]`: **~0.1 ms** (requires table already filled for `0..=max_index`).

At 100k turns, place replay is ~2.5–3 ms total — attack indexing is a large slice, but a shipped static table for one piece shape is **~3.5 MiB** (`max_index × 8 × 4` bytes) per piece variant and must be regenerated when spiral mapping or move sets change.

### Practical directions (ranked)

1. **Build-time attack tables per catalog `PieceDef`** (optional feature `precomputed_attacks`) — real runtime win for native/wasm if binary size acceptable; generate in `build.rs` up to `MAX_SPIRAL_INDEX` per piece.
2. **Piece-specific micro paths** — king (and wazir/ferz subset) always \|Δring\| ≤ 1; could specialize side/offset arithmetic instead of full `xy_to_index` (engineering heavy, moderate gain).
3. **Semantic change** — store threats in grid coords or chunks (big sim/render change); only if willing to rework scan + layers.
4. **Do nothing** — current `xy_to_index` loop is hard to beat on a **single** playthrough without shipping precomputed data.

**Not recommended:** ring-only LUT, per-index memo during play, or repeating coordinate LUT without a new access pattern (e.g. index-major static table).

## Kept (2026-05-27 perf pass — turn-order borrow)

Session baseline for A/B: `TIME_SIM_ITERS=20`, `TIME_SIM_WARMUP=2`, medians with per-turn `active_turn_order()` alloc: `knight_2_pairwise` 5.489, `knight_3_clique` 5.630, `leaper_4_mixed_clique` 5.826, `king_6_clique` 9.980, `chimera_3_clique` 7.336 ms. Checksums unchanged; `cargo testd` passed.

**Change:** `ActiveTurnOrder` lazy view (`GameDefinition::active_turn_order()`) with dense fast path over `turn_order` (not a cached copy on `Simulation` — differs from the 2026-05-25 rejected `turn_order` cache). Confirming run medians: **3.078 / 2.965 / 3.125 / 3.147 / 4.261 ms** (~44–68% vs baseline above on the same session). Prior doc timings (~2.5–4 ms post–attack-layers) align with this fast path; the allocating `active_turn_order()` call per turn had become a large regression vs those numbers.

## Kept (2026-05-27 — grid raster / app profiling)

**Render CPU (`raster_spiral_grid`):** Pre-fill the RGBA buffer with the empty texel color, then only write occupied cells (most visible cells when zoomed out are empty). Reuse the existing `Image` asset when width/height unchanged instead of `Image::new_fill` every redraw.

**Headless baselines** (`cargo run --release --bin perf_render`, 10k turns, 15 iters): `knight_2_tight` (2401 cells) med **0.007 ms**; `knight_3_wide` (21025 cells) med **0.069 ms**; `king_6_wide` (16641 cells) med **0.048 ms**. Checksums stable via `render::tests::raster_checksum_stable_for_knight_pairwise`.

**App profile labels:** `sync_viewport`, `render_raster`, `render_image_write`, `render_sprite_layout`, `display_clone`. Scenario `pan_render_stress` exercises repeated pans for full redraw + sim sync in `perf_app`. After the Arc occupancy change, `display_clone` is mostly pointer bump + metadata, not a full vec copy at 20M indices.

## Kept (2026-05-27 — max zoom / long history)

Users can zoom to the viewport cap (~4096 half-extent → **~67M** visible cells) and accept multi-second frames. Optimise for that reality rather than capping zoom.

**Occupancy snapshots:** `OccupancyGrid` stores cells in `Arc<Vec<ArmyId>>`. `Clone` is cheap; `ensure_unique_for_mutation()` copies only when the worker and main thread both hold the same buffer before a sim batch.

**Raster (`draw_spiral_cells` / `raster_spiral_grid_into`):** Reuse `RenderCache.scratch_rgba`; fill empty texels with bulk `u32` writes; paint occupied cells only; **parallel row bands** on native (`std::thread::scope`, disjoint row slices). WASM keeps sequential rows. Swap scratch into the Bevy `Image` buffer when dimensions match to avoid realloc.

**Headless max zoom** (`PERF_RENDER_TURNS=20000000`, `max_zoom_grid` case): median raster **~22 ms** (15 iters, M4-class macOS) vs **~148 ms** before parallel + u32 fill (same checksum `14869173723860730011`). Small grids unchanged (~0.12 ms @ 2401 cells).

**Tests:** `render::tests::max_zoom_raster_completes_within_budget` (±512 grid, 120 ms budget on dev builds).
