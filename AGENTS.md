# Project Instructions

## Project Overview

Zellij WASM plugin that adds notification icons to tab names when panes need attention. Designed for Claude Code users running multiple sessions across tabs.

**Flow:** External process → `zellij pipe --name "zellij-attention::EVENT::PANE_ID"` → plugin updates tab name → focus clears notification.

## Architecture

- `src/main.rs` — Core plugin logic: event handling, tab renaming, pipe parsing, focus clearing
- `src/config.rs` — User configuration parsing (enabled, waiting_icon, completed_icon)
- Build target: `wasm32-wasip1` (Zellij WASM plugin)

## Key Design Decisions

- **Single global plugin instance** via `load_plugins` in `config.kdl` (no visible pane)
- **Notification state:** `HashMap<u32, HashSet<NotificationType>>` — pane_id → notification set. Latest event **replaces** (no stacking per pane)
- **Tab-level priority:** If any pane in a tab has Waiting, tab shows ⏳. Only shows ✓ if no Waiting exists.
- **Original tab names** cached before renaming, restored on clear. Icons stripped defensively when caching to prevent accumulation from stale `tab.name`.
- **`rename_tab()` is 1-indexed** — Zellij API quirk, always pass `position + 1`
- **`updating_tabs` flag** prevents re-entrancy from `rename_tab()` → `TabUpdate` → `update_tab_names()` loop

## Zellij Plugin Gotchas

- Use broadcast pipes (`zellij pipe --name`) NOT targeted pipes (`--plugin`) — targeted pipes create new instances due to config mismatch
- Plugin state must use `/host/` path (shared), NOT `/data/` (sandboxed per-instance)
- `load_plugins` in config.kdl supports configuration via plugin aliases or inline config blocks
- Plugin pane IDs overlap with terminal pane IDs — always filter `is_plugin` when mapping panes
- `rename_tab()` triggers a synchronous `TabUpdate` event — beware of race conditions between rename and the resulting event
- `load_plugins` plugins may be lost after session resurrection (zellij attach) — see [#4156](https://github.com/zellij-org/zellij/issues/4156)
- After rebuilding WASM, clear Zellij cache: `find ~/.cache/zellij -path "*zellij-attention*" -exec rm -f {} \;`

## Build & Test

```bash
# Build
cargo build --release --target wasm32-wasip1

# Install
cp target/wasm32-wasip1/release/zellij-attention.wasm ~/.config/zellij/plugins/

# Test manually
zellij pipe --name "zellij-attention::waiting::$ZELLIJ_PANE_ID"
zellij pipe --name "zellij-attention::completed::$ZELLIJ_PANE_ID"
```
