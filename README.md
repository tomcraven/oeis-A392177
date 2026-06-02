# This entire codebase was almost entirely written with various different LLMs

## Red & Black Knights

Interactive visual simulator: several colored **pieces** take turns **placing** tokens on an infinite board indexed by a **square spiral**. Each piece uses a chess-like **attack pattern** (knight, wazir, ferz, gnu, chimera, and others). Placements are blocked on occupied cells and on squares “attacked” by pieces you configure to respect. The result is turn-by-turn growth of intricate, often fractal-looking color patterns—not a capture-based chess game, but a constraint puzzle on spiral order.

The default preset is two knights (dark and red) that each avoid the other’s knight moves—the namesake **red & black knights** setup.

Built with [Bevy](https://bevyengine.org/) and [egui](https://github.com/emilk/egui). Runs natively and in the browser (WASM).

## How it works

1. **Spiral indexing** — Cell `0` is the center; indices increase along a counterclockwise square spiral (see `src/spiral.rs`, related to [OEIS A316667](https://oeis.org/A316667)).
2. **Turns** — Pieces act in round-robin order. On its turn, a piece scans forward from its cursor along the spiral for the first **empty** cell that is not **forbidden** for that piece.
3. **Placement** — The piece occupies that cell (shown in its color) and marks every cell its piece would attack from there.
4. **Blocking** — Each piece’s `blocked_by` list controls which other pieces’ attacks it must avoid (a per-cell attacker bitmask in board coordinates, masked per defender). In the classic two-knight preset, each knight only respects the other.

You can mix piece types, cliques (everyone blocks everyone), pairwise setups, random generators, and shareable game definitions from the UI.

## Run locally (native)

This repo defines dev aliases in `.cargo/config.toml` that enable Bevy dynamic linking for faster iteration:

```bash
cargo rund --bin red_black_knights
```

Other useful commands:

```bash
cargo buildd          # cargo build --features bevy/dynamic_linking
cargo testd           # cargo test --features bevy/dynamic_linking
cargo rund --release --bin red_black_knights
```

Smoke test (exits after ~2 seconds):

```bash
cargo rund --bin red_black_knights -- --smoke-test
```

For shipping-style native release builds, use `cargo build --release` without the `buildd`/`rund` aliases.

## Run in the browser (WASM)

Requires `wasm-bindgen-cli` (`cargo install -f wasm-bindgen-cli`). The toolchain file already includes the `wasm32-unknown-unknown` target.

```bash
./web/serve.sh
```

Optional: `PROFILE=wasm-release ./web/serve.sh` for a smaller binary (`opt-level = s`).

## Other binaries

| Command | Purpose |
|--------|---------|
| `cargo run --release --bin time_sim` | Benchmark simulation throughput on fixed presets (see [`docs/OPTIMISATION.md`](docs/OPTIMISATION.md)). |
| `cargo run --release --bin discover_batch` | Batch-generate and render candidate setups (pattern discovery). |
| `cargo run --release --bin discover_reference` | Reference renders for discovery pipeline. |
| `cargo run --release --bin discover_rerender` | Re-render from saved discovery metadata. |
| `cargo run --release --bin perf_render` | Headless raster timing A/B. |
| `cargo run --release --features app_profile --bin perf_app` | Scripted in-app profiling scenarios. |

Place-level sim profiling uses the `place_profile` feature and `profile_place` binary.

## Project layout

| Path | Role |
|------|------|
| `src/sim.rs` | Turn loop, occupancy, coordinate `AttackGrid` / `MaskCells` |
| `src/sim_worker.rs` | Background sim on native; `SimDisplay` snapshots to the UI |
| `src/model.rs` | Pieces, piece move sets, presets |
| `src/spiral.rs` | Index ↔ coordinates on the square spiral |
| `src/index_order.rs` | Visit-order variants (default spiral; share/import) |
| `src/render.rs` | Grid-sized board texture and coloring |
| `src/ui.rs` | Sidebar, presets, run controls, share codes |
| `src/discover.rs` | Offline discovery runs and PNG export |
| `docs/SIMULATION.md` | How placement simulation and forbidden storage work |
| `docs/OPTIMISATION.md` | Performance guardrails, timings, experiments |

## Performance notes

Simulation hot paths are heavily tuned (dense spiral occupancy, coordinate forbidden grid with adaptive cell width, soft memory budget with fallible growth on WASM, background worker on native). Architecture is summarized in [docs/SIMULATION.md](docs/SIMULATION.md); methodology and benchmarks are in [docs/OPTIMISATION.md](docs/OPTIMISATION.md).
