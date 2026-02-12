# zellij-attention

Know which Zellij pane needs attention without checking each one manually.

## What This Does

Tracks notification state across panes and displays indicators directly in tab names. Designed for Claude Code users managing multiple AI sessions in floating panes. When Claude waits for input or completes a task, the corresponding tab is automatically renamed with a notification icon (e.g., "terminal" → "terminal ⏳"). Focusing the pane automatically clears the notification and restores the original tab name.

## How It Works

1. External processes (Claude Code hooks, shell functions) send pipe messages to the plugin
2. Plugin tracks notification state per pane and persists it across Zellij restarts
3. Plugin renames tabs to append notification icons (⏳ for waiting, ✓ for completed)
4. When you focus a pane with notifications, the plugin automatically clears them and restores the original tab name
5. Works with any Zellij configuration — no status bar integration or additional plugins required

## Installation

Download the WASM plugin to your Zellij plugins directory:

```bash
mkdir -p ~/.config/zellij/plugins
curl -L https://github.com/KiryuuLight/zellij-attention/releases/latest/download/zellij-attention.wasm -o ~/.config/zellij/plugins/zellij-attention.wasm
```

Add the plugin to your Zellij layout (see [Layout Configuration](#zellij-layout-configuration) below).

## Zellij Layout Configuration

The plugin must be loaded via a layout pane in your Zellij configuration. Add it to `~/.config/zellij/layouts/default.kdl`:

```kdl
layout {
    // Plugin must be in a pane in default_tab_template
    // Size 1 + borderless makes it invisible while active
    default_tab_template {
        pane size=1 borderless=true {
            plugin location="file:~/.config/zellij/plugins/zellij-attention.wasm" {
                // Optional configuration (all have sensible defaults)
                enabled "true"
                waiting_icon "⏳"
                completed_icon "✓"
            }
        }
        children  // Your normal panes go here
    }
}
```

**Important:** The plugin loads via a layout pane, NOT via `load_plugins` in `config.kdl`. This creates one plugin instance per tab, which is the intended behavior.

## Claude Code Hooks

Automate notifications when Claude needs input or finishes tasks. Add to `~/.claude/settings.json`:

```json
{
  "hooks": {
    "Notification": [
      {
        "matcher": "",
        "hooks": [
          {
            "type": "command",
            "command": "zellij pipe --name \"zellij-attention::waiting::$ZELLIJ_PANE_ID\""
          }
        ]
      }
    ],
    "Stop": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "zellij pipe --name \"zellij-attention::completed::$ZELLIJ_PANE_ID\""
          }
        ]
      }
    ]
  }
}
```

**Hook events:**
- `Notification` — Fires when Claude waits for user input (sends "waiting" notification)
- `Stop` — Fires when Claude finishes a task (sends "completed" notification)

## Shell Functions

For manual testing or integration with other tools, add these functions to `~/.bashrc` or `~/.zshrc`:

```bash
# Send waiting notification for current pane
notify-waiting() {
    if [ -z "$ZELLIJ_PANE_ID" ]; then
        echo "Error: Not running inside Zellij (ZELLIJ_PANE_ID not set)"
        return 1
    fi
    zellij pipe --name "zellij-attention::waiting::$ZELLIJ_PANE_ID"
}

# Send completed notification for current pane
notify-completed() {
    if [ -z "$ZELLIJ_PANE_ID" ]; then
        echo "Error: Not running inside Zellij (ZELLIJ_PANE_ID not set)"
        return 1
    fi
    zellij pipe --name "zellij-attention::completed::$ZELLIJ_PANE_ID"
}
```

**Usage:**

```bash
# Run a long command and notify when done
some-long-command && notify-completed

# Signal waiting state manually
notify-waiting
```

## Pane IDs

The plugin uses Zellij's pane IDs to track which pane triggered each notification. `$ZELLIJ_PANE_ID` is an environment variable automatically set by Zellij in every terminal pane.

**Check your pane ID:**

```bash
echo $ZELLIJ_PANE_ID
```

This ID is used in pipe messages to identify which pane needs attention. When you focus a pane, the plugin clears notifications for that specific pane ID.

## Configuration

The plugin works out-of-the-box with sensible defaults. All configuration is optional.

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `enabled` | bool | `true` | Enable or disable all notifications |
| `waiting_icon` | string | `⏳` | Icon appended to tab name for waiting state |
| `completed_icon` | string | `✓` | Icon appended to tab name for completed state |

**Example with custom icons:**

```kdl
plugin location="file:~/.config/zellij/plugins/zellij-attention.wasm" {
    enabled "true"
    waiting_icon "!"
    completed_icon "*"
}
```

**Notes:**
- Icons are appended to the END of tab names (e.g., "terminal ⏳" not "⏳ terminal")
- Icons longer than 4 characters will trigger a warning but will still work
- Setting `enabled "false"` disables all notifications (useful for temporary disabling)

## Pipe Message Format

The plugin listens for pipe messages in this format:

```
zellij-attention::EVENT_TYPE::PANE_ID
```

**Components:**
- `EVENT_TYPE` — Either `waiting` or `completed` (case-insensitive)
- `PANE_ID` — Numeric pane ID (from `$ZELLIJ_PANE_ID`)

**Example commands:**

```bash
# Send waiting notification for pane 5
zellij pipe --name "zellij-attention::waiting::5"

# Send completed notification for pane 5
zellij pipe --name "zellij-attention::completed::5"

# Using environment variable (typical usage)
zellij pipe --name "zellij-attention::waiting::$ZELLIJ_PANE_ID"
```

**CRITICAL:** Always use `--name` (broadcast pipe), NEVER `--plugin` (targeted pipe). Targeted pipes attempt to match plugin configuration and will create new plugin instances instead of communicating with existing ones.

## Troubleshooting

### Notifications not appearing

1. **Check plugin is loaded:**
   - Verify plugin pane exists in your layout's `default_tab_template`
   - Check Zellij logs: `tail -f /tmp/zellij-*/zellij-log-*/zellij.log` (look for "zellij-attention: loaded")

2. **Verify pipe commands work:**
   ```bash
   echo $ZELLIJ_PANE_ID  # Should print a number
   zellij pipe --name "zellij-attention::waiting::$ZELLIJ_PANE_ID"
   # Tab name should change immediately
   ```

3. **Check state file:**
   ```bash
   # State is persisted in the directory where Zellij was launched
   # /host/ in the plugin maps to your cwd
   ls -la .zellij-attention-state.bin
   ```

### Plugin not loading

- The plugin MUST be in a layout pane (in `default_tab_template`), NOT in `load_plugins` section of `config.kdl`
- `load_plugins` in `config.kdl` does NOT pass configuration to plugins — use layout panes instead

### Pipe command hangs or does nothing

- Ensure you're using `--name` flag (broadcast), NOT `--plugin` flag (targeted)
- Check `$ZELLIJ_PANE_ID` is set: `echo $ZELLIJ_PANE_ID`
- Verify format: `zellij-attention::EVENT_TYPE::PANE_ID` (double-colon separated)

### Wrong format errors

**Correct format:**
```bash
zellij pipe --name "zellij-attention::waiting::5"
```

**Common mistakes:**
```bash
# WRONG: Single colon
zellij pipe --name "zellij-attention:waiting:5"

# WRONG: Missing plugin name prefix
zellij pipe --name "waiting::5"

# WRONG: Using --plugin instead of --name
zellij pipe --plugin "zellij-attention" --message "waiting::5"
```

### Tabs not restoring original names

- This is expected behavior if notifications are still present on other panes in the tab
- Focus the pane with the notification to clear it — the tab name will restore automatically
- Check persisted state: `rm .zellij-attention-state.bin` in the directory where Zellij was launched (will clear all notifications on next restart)

### Multiple plugin instances

- This is normal and expected — one instance per tab via `default_tab_template`
- All instances share state via `/host/.zellij-attention-state.bin` (in your cwd)
- Broadcast pipes (`--name`) reach all instances simultaneously

## Development

Build from source:

```bash
cargo build --target wasm32-wasip1 --release
```

Output: `target/wasm32-wasip1/release/zellij-attention.wasm`

Copy to plugins directory:

```bash
cp target/wasm32-wasip1/release/zellij-attention.wasm ~/.config/zellij/plugins/
```

**Debug logging:**

The plugin includes debug output that only appears in debug builds:

```bash
# Build with debug output
cargo build --target wasm32-wasip1

# Watch Zellij logs
tail -f /tmp/zellij-*/zellij-log-*/zellij.log | grep "zellij-attention"
```

## License

MIT
