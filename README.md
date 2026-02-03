# zellij-attention

Know which Zellij pane needs attention without checking each one manually.

## What This Does

`zellij-attention` tracks notification state across your Zellij panes and displays indicators in your zjstatus bar. Designed for Claude Code users running multiple AI sessions in floating panes, it lets you see at a glance which tabs have panes waiting for input or have completed tasks—without switching between them.

## How It Works

External processes (like Claude Code hooks or manual shell commands) send pipe messages to the plugin using `zellij pipe --name "zellij-attention::EVENT_TYPE::PANE_ID"`. The plugin tracks notification state per pane, formats a status summary, and writes it to `~/.zellij-attention-status`. zjstatus reads this file via a command widget and displays it in the status bar. When you focus a pane with notifications, they're automatically cleared.

## Installation

**1. Download the plugin:**

```bash
mkdir -p ~/.config/zellij/plugins
curl -L https://github.com/KiryuuLight/zellij-attention/releases/latest/download/zellij-attention.wasm -o ~/.config/zellij/plugins/zellij-attention.wasm
```

**2. Add to your Zellij layout** (see [Layout Configuration](#zellij-layout-configuration) below)

## Zellij Layout Configuration

The plugin loads via your Zellij layout file, **NOT** via `load_plugins` in `config.kdl`.

Add this to your layout (e.g., `~/.config/zellij/layouts/default.kdl`):

```kdl
layout {
    default_tab_template {
        pane size=1 borderless=true {
            // zjstatus plugin - your existing status bar
            plugin location="file:~/.config/zellij/plugins/zjstatus.wasm" {
                // ... your existing zjstatus config ...

                // Add these lines to your zjstatus config:
                // Command widget that reads the attention status file
                command_attention_command "/bin/bash -c \"cat $HOME/.zellij-attention-status 2>/dev/null || echo '#[fg=green]✓'\""
                command_attention_format "{stdout}"
                command_attention_interval "1"
                command_attention_rendermode "static"

                // Then use {command_attention} in your format string:
                // format_left "{tabs} {command_attention}"
            }
        }

        // Main pane - your terminal content goes here
        children

        pane size=1 borderless=true {
            // zellij-attention plugin - runs as background service
            plugin location="file:~/.config/zellij/plugins/zellij-attention.wasm" {
                // Optional configuration (these are defaults):
                enabled "true"
                waiting_color "#f5a623"      // Orange for waiting state
                waiting_icon "⏳"
                completed_color "#7cb342"    // Green for completed state
                completed_icon "✓"
            }
        }
    }
}
```

**Key points:**

- The plugin runs in a borderless pane (size=1) as a background service
- zjstatus reads `~/.zellij-attention-status` using a command widget
- The fallback `echo '#[fg=green]✓'` shows when no notifications exist
- The command widget polls every 1 second (`interval "1"`)

## Claude Code Hooks

Automate notifications when Claude needs input or finishes a task.

Add this to your `~/.claude/settings.json`:

```json
{
  "hooks": {
    "Notification": {
      "matcher": "",
      "hooks": [
        {
          "type": "command",
          "command": "zellij",
          "args": [
            "pipe",
            "--name",
            "zellij-attention::waiting::$ZELLIJ_PANE_ID"
          ]
        }
      ]
    },
    "Stop": {
      "matcher": "",
      "hooks": [
        {
          "type": "command",
          "command": "zellij",
          "args": [
            "pipe",
            "--name",
            "zellij-attention::completed::$ZELLIJ_PANE_ID"
          ]
        }
      ]
    }
  }
}
```

**What these hooks do:**

- **Notification event**: Fires when Claude is waiting for user input. Sends a "waiting" notification.
- **Stop event**: Fires when Claude finishes a task. Sends a "completed" notification.

The hooks use `$ZELLIJ_PANE_ID` which is automatically set by Zellij in every pane.

## Shell Functions

For manual notification testing or custom workflows, add these functions to your `~/.bashrc` or `~/.zshrc`:

```bash
# Send a "waiting" notification for the current pane
notify-waiting() {
    if [ -z "$ZELLIJ_PANE_ID" ]; then
        echo "Error: Not running in Zellij (ZELLIJ_PANE_ID not set)"
        return 1
    fi
    zellij pipe --name "zellij-attention::waiting::$ZELLIJ_PANE_ID"
}

# Send a "completed" notification for the current pane
notify-completed() {
    if [ -z "$ZELLIJ_PANE_ID" ]; then
        echo "Error: Not running in Zellij (ZELLIJ_PANE_ID not set)"
        return 1
    fi
    zellij pipe --name "zellij-attention::completed::$ZELLIJ_PANE_ID"
}
```

**Usage examples:**

```bash
# Start a long task, notify when done
cargo build && notify-completed

# Or notify that you're waiting
notify-waiting
```

## Pane IDs

`$ZELLIJ_PANE_ID` is an environment variable automatically set by Zellij in every terminal pane. It's a numeric identifier that uniquely identifies the pane within your Zellij session.

**Check your current pane ID:**

```bash
echo $ZELLIJ_PANE_ID
```

This ID is used in pipe messages to tell the plugin which pane triggered the notification.

## Configuration

All configuration options are optional. The plugin works out-of-the-box with sensible defaults.

| Option            | Type       | Default   | Description                           |
|-------------------|------------|-----------|---------------------------------------|
| `enabled`         | bool       | `true`    | Enable/disable all notifications      |
| `waiting_color`   | hex color  | `#f5a623` | Color for waiting state (orange)      |
| `waiting_icon`    | string     | `⏳`      | Icon for waiting state                |
| `completed_color` | hex color  | `#7cb342` | Color for completed state (green)     |
| `completed_icon`  | string     | `✓`       | Icon for completed state              |

**Example custom configuration:**

```kdl
plugin location="file:~/.config/zellij/plugins/zellij-attention.wasm" {
    enabled "true"
    waiting_color "#ff6b6b"      // Red for urgent
    waiting_icon "!"
    completed_color "#51cf66"    // Bright green
    completed_icon "✓"
}
```

**Notes:**

- Hex colors accept `#RGB` or `#RRGGBB` formats (with or without `#`)
- Invalid colors fall back to defaults with a warning in Zellij logs
- Icons longer than 4 characters may not display well

## Pipe Message Format

The plugin listens for pipe messages with this format:

```
zellij-attention::EVENT_TYPE::PANE_ID
```

**Components:**

- **Prefix**: Always `zellij-attention`
- **EVENT_TYPE**: Either `waiting` or `completed` (case-insensitive)
- **PANE_ID**: Numeric pane ID (from `$ZELLIJ_PANE_ID`)

**Example commands:**

```bash
# Send "waiting" notification for pane 42
zellij pipe --name "zellij-attention::waiting::42"

# Send "completed" notification for current pane
zellij pipe --name "zellij-attention::completed::$ZELLIJ_PANE_ID"
```

**CRITICAL**: Always use the `--name` flag (broadcast pipe). Do NOT use `--plugin` (targeted pipe), as it creates new plugin instances due to configuration mismatches.

## Troubleshooting

### Notifications not appearing

1. **Check plugin is loaded**: Look for a size=1 pane in your layout with the plugin
2. **Verify pipe commands**: Run `zellij pipe --name "zellij-attention::waiting::$ZELLIJ_PANE_ID"` manually
3. **Check status file**: `cat ~/.zellij-attention-status` should show output
4. **Check zjstatus config**: Verify you added the `command_attention_*` options and `{command_attention}` in your format string

### Plugin not loading

The plugin **must** be loaded via a layout pane, NOT via `load_plugins` in `config.kdl`. The `load_plugins` directive doesn't pass configuration to plugins properly.

**Wrong:**

```kdl
// In config.kdl - DOESN'T WORK
plugins {
    zellij-attention location="file:~/.config/zellij/plugins/zellij-attention.wasm"
}
```

**Correct:**

```kdl
// In layout file - WORKS
pane size=1 borderless=true {
    plugin location="file:~/.config/zellij/plugins/zellij-attention.wasm" {
        // config here
    }
}
```

### Pipe command hangs

If `zellij pipe` hangs, you're likely using `--plugin` instead of `--name`.

**Wrong:**

```bash
zellij pipe --plugin zellij-attention "message"  # Hangs or creates new instances
```

**Correct:**

```bash
zellij pipe --name "zellij-attention::waiting::$ZELLIJ_PANE_ID"  # Works
```

### Wrong format errors

The message format is colon-separated, NOT JSON or other formats.

**Wrong:**

```bash
zellij pipe --name '{"event":"waiting","pane":42}'  # Invalid format
zellij pipe --name "waiting:42"                      # Missing prefix
```

**Correct:**

```bash
zellij pipe --name "zellij-attention::waiting::42"  # Double colons
```

### Multiple notification instances

It's normal to have one plugin instance per tab. The plugin uses file-based state (`~/.zellij-attention-state.bin`) to coordinate between instances.

## Development

Build from source:

```bash
cargo build --target wasm32-wasip1 --release
```

The WASM binary will be at `target/wasm32-wasip1/release/zellij-attention.wasm`.

---

**Repository**: https://github.com/KiryuuLight/zellij-attention

**License**: MIT
