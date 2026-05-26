---
name: preset-discovery
description: Sweep simple chess-like piece combinations, render multiscale board images, and rarely curate emergent presets for red_black_knights. Compare to the two-knights reference. Not random attack patterns.
---

# Preset discovery loop

Discovery **does not use random attack blobs** anymore. `discover_batch` walks a fixed **catalog** of simple pieces (`src/discover_catalog.rs`):

- **100 pairwise** matchups (`wazir`, `ferz`, `dabbaba`, `alfil`, `knight`, `king`, `camel`, `zebra`, `giraffe`, `trebuchet`)
- **40 same-piece cliques** (2–5 armies)
- **12 mixed 3-army cliques** (e.g. wazir+ferz+knight)

Iteration `i` → catalog index `i % 152`. `meta.toml` records `recipe_id`, `recipe_label`, `catalog_index`.

## Gold standard — read before curating

**[Two knights example](./knight-example.md)** and local PNGs:

`.discover/reference/knight_2_pairwise/scale_{center,mid,full}.png`

Emergent art from **simple, understandable pieces** is the goal—like knight vs knight—not generic full-frame wallpaper.

## Each pending run includes

- `scale_center.png`, `scale_mid.png`, `scale_full.png` (plus `board.png`)
- `config.toml` — exact armies
- `meta.toml` — `recipe_id`, settle stats

**You must read all three scale PNGs** before keeping anything.

## Curation bar (very selective)

Default: **zero keepers per batch**. Keep **one** only if:

- All three scales tell a **knight-like story** (chaotic core → islands/order → macro provinces), **and**
- The recipe is **simple** (you can explain the pieces in one line), **and**
- It is **not** already boringly similar to knight pairwise or a prior keeper

Reject:

- Pretty symmetry with no chaotic core or no province phase
- Random-looking texture without readable piece logic
- “Good enough” — if unsure, skip

When keeping, slug should include recipe, e.g. `pairwise_knight_vs_camel_b4_00012`.

## Generate a batch

```bash
cargo run --release --bin discover_batch -- \
  --out .discover/pending \
  --start "$(jq -r .next_iteration .discover/session.json)" \
  --count "$(jq -r .batch_size .discover/session.json)" \
  --target-index "$(jq -r .target_index .discover/session.json)" \
  --cell-scale "$(jq -r .cell_pixel_scale .discover/session.json)"
```

Advance `.discover/session.json` → `next_iteration` += `batch_size`.

## Commands

```bash
cargo testd discover_catalog::
cargo run --release --bin discover_reference -- --preset knight_2_pairwise --out .discover/reference/knight_2_pairwise
cargo run --release --bin discover_batch -- --help
```

List recipe for an index in Rust tests or `recipe_meta` in `discover_catalog.rs`.
