# Optimisation Notes

Simulation performance work verified with the release timing harness. Rendering stays on the **grid-sized texture** path (one texel per visible spiral cell), not the stashed viewport-texel renderer.

## Guardrails

- Correctness: `cargo test --features bevy/dynamic_linking`
- Timing: `cargo run --release --features bevy/dynamic_linking --bin time_sim`
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

## Future work

- Safe far zoom-out needs a viewport-limited **render** path, not only a higher `max_scale`.
- Exact simulation cannot skip offscreen spiral history; only the **target index** may be capped to the visible rectangle (+ margin).
- Occupancy fast paths need a bitset **and** a skip path that is not evaluated on every rejection when it cannot fire (e.g. only when `contains_index(next)` and word-aligned occupied mask is full — still regressed when tried with maintained bitset).
- No further low-risk CPU wins obvious without profiling (`sample`/`perf`) on `step_turn` vs `record_forbidden` split.
