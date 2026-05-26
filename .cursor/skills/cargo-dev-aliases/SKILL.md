---
name: cargo-dev-aliases
description: Use cargo aliases from .cargo/config.toml (buildd, testd, rund) for native dev builds in red_black_knights. Use when compiling, testing, running the game locally, verifying changes, or the user mentions cargo build/test/run without wasm or release shipping.
---

# Cargo dev aliases (this repo)

Defined in [`.cargo/config.toml`](../../.cargo/config.toml):

| Alias | Expands to |
|-------|------------|
| `buildd` | `cargo build --features bevy/dynamic_linking` |
| `testd` | `cargo test --features bevy/dynamic_linking` |
| `rund` | `cargo run --features bevy/dynamic_linking` |

Dynamic linking speeds up Bevy relinks during iteration. **Do not** use these for wasm builds or when validating a release artifact you intend to ship.

## Default commands

Prefer aliases over repeating `--features bevy/dynamic_linking`:

```bash
cargo buildd
cargo testd
cargo rund --bin red_black_knights
cargo rund --release --bin red_black_knights
cargo rund --release --bin time_sim
```

Append normal cargo flags after the alias (e.g. `--release`, `--bin`, `-p`, test filters).

## When not to use

- **`wasm32-unknown-unknown`**: use `cargo build --profile wasm-release-fast --target wasm32-unknown-unknown` or `./web/serve.sh` (no dynamic linking on wasm).
- **Release/shipping checks** where you need a fully static binary: `cargo build --release` without the alias.

## Agent checklist

Before saying a native build or test passed:

1. Use `buildd` / `testd` / `rund` unless the task is explicitly wasm or static release.
2. Run the command; do not skip verification after code changes.
