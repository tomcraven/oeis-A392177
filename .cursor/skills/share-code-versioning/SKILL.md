---
name: share-code-versioning
description: Maintains versioned rbk share codes (Copy/Import share code in the UI). Use when changing what "the current view" includes, SavedGameDefinition or GameDefinition persistence, camera/target_index/colouring, share_code.rs, or serde fields that affect export/import strings.
---

# Share code versioning (red_black_knights)

Share codes restore **what the user is seeing**: piece rules, sim depth, camera, and board colouring. Wire format:

```text
rbk:<envelope_version>:<standard-base64(JSON)>
```

Canonical implementation: [`src/share_code.rs`](../../src/share_code.rs). UI: sidebar **Share** section — **Copy share code** / **Import share code** in [`src/ui.rs`](../../src/ui.rs).

## When this skill applies

Load and follow this skill if your change touches any of:

| Area | Examples |
|------|----------|
| **View snapshot** | New field needed to recreate the board on another machine (zoom, colour mode, sim index, etc.) |
| **Piece rules serialization** | [`SavedGameDefinition`](../../src/game_snapshot.rs), armies, moves, `blocked_by`, turn order |
| **Camera** | [`CameraSessionConfig`](../../src/camera_config.rs), pan/zoom limits affecting restore |
| **Import apply path** | [`apply_share_snapshot`](../../src/share_code.rs), bookmark-like restore behaviour |
| **Capture path** | [`capture_share_view`](../../src/share_code.rs), `ShareCapture`, copy button in UI |

**Does not require a share bump** (usually): pure rendering, perf, CLI discover-only formats, local session/bookmark file paths — unless bookmarks and share must stay aligned by policy.

**May require coordination**: [`AppSession`](../../src/app_session.rs) persists more than share (full UI). Share is a **deliberate subset** for portability; do not mirror entire `SavedUiState` unless product intent changes.

## V1 payload (current)

`ShareViewSnapshot` at envelope version **1**:

- `version` (inner JSON, must match envelope for v1)
- `game`: `SavedGameDefinition`
- `camera`: `CameraSessionConfig`
- `target_index`: `u32`
- `board_colour_mode`: `BoardColourMode`

Constants: `SHARE_CODE_PREFIX`, `CURRENT_SHARE_VERSION`.

## Decision: patch vs new version

```text
Change only affects NEW encodes but old codes still decode and restore sensibly?
  → Prefer backward-compatible JSON (#[serde(default)], optional fields) on SAME envelope version.
  → Old codes missing new fields get defaults on import.

Change breaks meaning of old payloads OR you remove/rename required fields?
  → Bump envelope: CURRENT_SHARE_VERSION += 1.
  → Keep decoding old envelope versions in decode_share_payload.
```

Never reuse an envelope version number with incompatible JSON.

## Checklist: backward-compatible field (same version)

1. Add field to `ShareViewSnapshot` with `#[serde(default)]` (or custom default) if optional.
2. Set field in `capture_share_view` / `ShareCapture`.
3. Apply field in `apply_share_snapshot`.
4. Wire UI capture if not automatic (same sources as copy button).
5. Extend `share_code_round_trips_v1` (or add case) in [`share_code.rs`](../../src/share_code.rs) tests.
6. Run `cargo testd share_code::`.

## Checklist: new envelope version (v2+)

1. Increment `CURRENT_SHARE_VERSION`.
2. Define `ShareViewSnapshotV2` (or evolve struct with clear docs); encoder writes only the new version.
3. In `decode_share_payload`:
   - `1 =>` existing path (unchanged or via `migrate_v1_to_latest`).
   - `2 =>` parse v2; optionally accept inner `version` check.
   - `v if v > CURRENT_SHARE_VERSION =>` keep "update the app" error.
4. Implement `apply_share_snapshot` for the **canonical** in-memory shape (either migrate all versions to `ShareViewSnapshot` or match on version internally).
5. Add tests:
   - Round-trip for new version (`rbk:2:...`).
   - Import of a **frozen** v1 sample string still works.
   - Reject `rbk:99:...` (future version test pattern).
6. Run `cargo testd share_code::` and smoke copy/import in UI if behaviour changed.

## Migration pattern

Prefer explicit functions over inline clutter:

```rust
fn decode_share_payload(bytes: &[u8], envelope: u32) -> Result<ShareViewSnapshot, String> {
    match envelope {
        1 => migrate_v1(serde_json::from_slice(bytes)?),
        2 => Ok(serde_json::from_slice(bytes)?),
        // ...
    }
}
```

When v2 adds fields, v1 import should produce the same user-visible state as before (defaults for new fields).

## Agent verification

Before marking the task done:

1. `cargo testd share_code::`
2. Confirm **Copy** still produces `rbk:{CURRENT_SHARE_VERSION}:` prefix.
3. If `SavedGameDefinition` changed, confirm round-trip through `SavedGameDefinition::from_game` / `Into<GameDefinition>` still matches expectations in the test.

## Related files (quick map)

| File | Role |
|------|------|
| `src/share_code.rs` | Encode/decode, version match, apply |
| `src/game_snapshot.rs` | `SavedGameDefinition` schema |
| `src/ui.rs` | Copy/import UI, `share_code_input`, status lines |
| `src/app_session.rs` | Local persistence (related, not identical to share) |
