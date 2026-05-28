---
name: ui-session-persistence
description: Persist sidebar and editor UI preferences in AppSession (session.toml on native). Use when adding or changing UiState fields, SavedUiState, capture_session, apply_session_to_ui, or any setting that should survive app restarts.
---

# UI session persistence (red_black_knights)

Native builds save [`AppSession`](../../src/app_session.rs) to `session.toml` (see `config_file_path()`). The game rules live in `session.game`; **sidebar/editor chrome** lives in `session.ui` (`SavedUiState`).

Share codes are a separate, portable subset — see [share-code-versioning](../share-code-versioning/SKILL.md). Do not add session-only UI toggles to share payloads unless product explicitly requires it.

## When to persist a `UiState` field

Persist it in `SavedUiState` when **both** are true:

1. The user can change it in the UI (checkbox, selection, text field meant to stick, collapsed section state, etc.).
2. Losing it on restart would feel like a bug (e.g. **Sync all pieces**, **Mutate → All**, preset index, sidebar open/closed).

Keep it **only** on `UiState` (ephemeral) when it is transient: status toasts, paste buffers, in-progress draft before apply, animation timestamps (`share_code_copied_at`, `export_status`), etc.

`UiState.draft` is rebuilt from `session.game` in `apply_session_to_ui`; do not duplicate the full game in `SavedUiState`.

Backward compatibility for old `session.toml` UI chrome is **not** required by default. When renaming or replacing session-only UI fields, prefer the new clean shape over serde aliases, migration shims, or legacy tests unless the user explicitly asks to preserve old local sessions.

## Checklist for a new persisted UI field

1. Add the field to [`UiState`](../../src/ui.rs) with a sensible `Default`.
2. Add the same field to [`SavedUiState`](../../src/app_session.rs) with `#[serde(default)]` if older `session.toml` files may omit it.
3. Wire [`capture_session`](../../src/app_session.rs): copy `ui_state.field` into `SavedUiState`.
4. Wire [`apply_session_to_ui`](../../src/app_session.rs): copy `saved.field` back (clamp indices with `def.armies.len()` when the field is an army index).
5. Extend [`app_session_round_trips_through_toml`](../../src/app_session.rs) (or add a focused test) so the field round-trips.
6. Run `cargo testd app_session`.

Sidebar section open/closed flags belong in [`SidebarSections`](../../src/ui.rs) inside `SavedUiState.sidebar` — extend that struct instead of adding one-off top-level fields when the setting is “this panel was expanded.”

## Reference: what `SavedUiState` already carries

| Area | Fields |
|------|--------|
| Random / mutate | `random_gen`, `mutate_army`, `mutate_all` |
| Pieces editor | `preset_index`, `edit_army`, `sync_attack_squares`, roster add/remove indices, `add_piece_color` |
| Bookmarks UI | `bookmark_new_name`, `bookmark_selected` |
| Sidebar layout | `sidebar` (`SidebarSections`) |
| Board display | `board_colour_mode` |

Wasm does not write `session.toml`; persistence still matters for consistent types and tests.

## Agent checklist

Before finishing a UI preference change:

1. Decide persist vs ephemeral using the rules above.
2. If persist: complete all wiring steps and run `cargo testd app_session`.
3. If ephemeral: confirm the field is **not** added to `SavedUiState` (avoid silent loss or accidental TOML bloat).
