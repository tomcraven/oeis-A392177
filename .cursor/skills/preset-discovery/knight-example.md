# Example preset: two knights (`knight_2_pairwise`)

This is the **bar** for catalog discovery: **simple pieces** (two knights, standard pairwise blocking) producing **complex emergent** geometry at scale.

Catalog runs use the same piece vocabulary (`discover_catalog.rs`)—not random attack patterns. When a catalog recipe looks as rich as this reference, it is a rare keeper.

## Before you curate a batch

1. If missing, generate the reference (once per machine, ~20s release):

```bash
cargo run --release --bin discover_reference -- \
  --preset knight_2_pairwise \
  --out .discover/reference/knight_2_pairwise
```

2. **Read these images** (tool read on PNG paths):

| Path | Phase |
|------|--------|
| `.discover/reference/knight_2_pairwise/scale_center.png` | **Chaos** at the spiral core |
| `.discover/reference/knight_2_pairwise/scale_mid.png` | **Islands of order** — filaments, nested shapes, partial regularity |
| `.discover/reference/knight_2_pairwise/scale_full.png` | **Macro settle** — large color provinces (here: top red / bottom dark, with a striped cross) |

Optional prose: `.discover/reference/knight_2_pairwise/verdict.md` (same folder, created with the reference).

3. When reviewing each pending `board.png`, ask: *Does this run show a knight-like **scale story**—messy center, structured middle, big provinces at full settle—or is it only pretty at one zoom?*

## What makes two knights “interesting”

Two **phase transitions** as you zoom out:

1. **Center (~145×145 cells):** Red and dark blue are tightly interleaved; no stable macro shape—local knight alternation only.
2. **Mid (~641×641 cells):** Chaos gives way to **islands**: nested L-shapes, striped axial bands, patches of repeating structure in open areas.
3. **Full (~2200×2200 at target index 4819953):** **Quadrants / provinces**—vast solid color regions (top vs bottom here), with intricate arms along the axes between them.

That arc—**chaos → ordered islands → settled provinces**—is the bar. A keeper should be describable in those terms when compared to this example.

## In-game preset

Same rules as the reference: `GameDefinition::knight_2_pairwise()` in `src/model.rs`, UI preset **knight_2_pairwise**.
