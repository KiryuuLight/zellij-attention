# Troubleshooting

## Notifications not appearing

1. **Check plugin is loaded:**
   - Verify `load_plugins` is configured in your `config.kdl`
   - Check Zellij logs: `tail -f /tmp/zellij-*/zellij-log-*/zellij.log` (look for `zellij-attention: loaded`)

2. **Verify pipe commands work:**
   ```bash
   echo $ZELLIJ_PANE_ID  # Should print a number
   zellij pipe --name "zellij-attention::waiting::$ZELLIJ_PANE_ID"
   # Tab name should change immediately
   ```

3. **Clear Zellij plugin cache:**
   ```bash
   # Zellij caches compiled WASM — clear if the plugin isn't updating
   find ~/.cache/zellij -path "*zellij-attention*" -exec rm -f {} \;
   ```

## Plugin not loading

- Verify the plugin is in `load_plugins` in your `config.kdl` with the correct file path
- Check that the `.wasm` file exists at `~/.config/zellij/plugins/zellij-attention.wasm`

## Pipe command hangs or does nothing

- Ensure you're using the `--name` flag (broadcast), NOT `--plugin` (targeted)
- Check `$ZELLIJ_PANE_ID` is set: `echo $ZELLIJ_PANE_ID`
- Verify the format uses double-colon separators: `zellij-attention::EVENT_TYPE::PANE_ID`

### Wrong format examples

**Correct:**
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

## Tabs not restoring original names

- This is expected if notifications are still active on other panes in the same tab
- Focus the pane with the notification to clear it — the tab name restores automatically
- To force-clear all notifications, restart the Zellij session (state is memory-only)
