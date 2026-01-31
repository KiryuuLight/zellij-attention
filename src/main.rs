mod state;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Write;
use zellij_tile::prelude::*;

use crate::state::{load_state, save_state, NotificationType, PersistedState};

/// Converts a PaletteColor to ANSI foreground color escape sequence.
fn palette_color_to_ansi_fg(color: PaletteColor) -> String {
    match color {
        PaletteColor::Rgb((r, g, b)) => format!("\x1b[38;2;{};{};{}m", r, g, b),
        PaletteColor::EightBit(idx) => format!("\x1b[38;5;{}m", idx),
    }
}

/// Converts a PaletteColor to ANSI background color escape sequence.
fn palette_color_to_ansi_bg(color: PaletteColor) -> String {
    match color {
        PaletteColor::Rgb((r, g, b)) => format!("\x1b[48;2;{};{};{}m", r, g, b),
        PaletteColor::EightBit(idx) => format!("\x1b[48;5;{}m", idx),
    }
}

/// JSON payload structure for pipe messages.
/// External processes send: `zellij pipe --name notification -- '{"event_type":"waiting","pane_id":42}'`
#[derive(Debug, serde::Deserialize)]
struct PipeEvent {
    event_type: String,
    pane_id: u32,
}

#[derive(Default)]
struct State {
    permissions_granted: bool,
    tabs: Vec<TabInfo>,
    panes: PaneManifest,
    notification_state: HashMap<u32, HashSet<NotificationType>>,
    mode_info: ModeInfo,
    tab_positions: Vec<(usize, usize)>, // (start_x, end_x) per tab for mouse clicks
}

impl State {
    /// Determines which pane is currently focused, accounting for floating pane visibility.
    /// Returns None if no pane is focused or active tab cannot be determined.
    fn determine_focused_pane(&self) -> Option<u32> {
        // Find active tab
        let active_tab = self.tabs.iter().find(|t| t.active)?;

        // Get panes for active tab
        let panes = self.panes.panes.get(&active_tab.position)?;

        // Find focused pane in the correct layer (floating vs tiled)
        // When floating panes are visible, only floating panes can be focused
        // When floating panes are hidden, only tiled panes can be focused
        let focused = panes.iter().find(|p| {
            p.is_focused && (p.is_floating == active_tab.are_floating_panes_visible)
        })?;

        Some(focused.id)
    }

    /// Checks if focused pane has notifications and clears them.
    /// Persists state to disk if any notifications were cleared.
    fn check_and_clear_focus(&mut self) {
        if let Some(focused_pane_id) = self.determine_focused_pane() {
            if self.notification_state.remove(&focused_pane_id).is_some() {
                #[cfg(debug_assertions)]
                eprintln!(
                    "zellij-attention: Cleared notifications for focused pane {}",
                    focused_pane_id
                );

                // Persist state change
                let persisted = PersistedState {
                    notifications: self.notification_state.clone(),
                };
                if let Err(e) = save_state(&persisted) {
                    eprintln!("zellij-attention: Failed to save state: {}", e);
                }
            }
        }
    }

    /// Determines the notification state for a tab by checking all panes in that tab.
    /// Returns the highest priority notification: Waiting > Completed > None.
    /// Priority: Waiting is attention-seeking, so it takes precedence.
    fn get_tab_notification_state(&self, tab_position: usize) -> Option<NotificationType> {
        // Get panes for this tab position
        let panes = self.panes.panes.get(&tab_position)?;

        // Check if any pane in this tab has notifications
        // Priority: Waiting > Completed (attention-seeking state first)
        let mut has_completed = false;

        for pane in panes {
            if let Some(notifications) = self.notification_state.get(&pane.id) {
                if notifications.contains(&NotificationType::Waiting) {
                    return Some(NotificationType::Waiting);
                }
                if notifications.contains(&NotificationType::Completed) {
                    has_completed = true;
                }
            }
        }

        if has_completed {
            Some(NotificationType::Completed)
        } else {
            None
        }
    }

    /// Removes notification entries for panes that no longer exist.
    /// Called on every PaneUpdate to handle pane closures.
    fn cleanup_stale_panes(&mut self) {
        // Collect all current pane IDs from all tabs
        let current_pane_ids: HashSet<u32> = self
            .panes
            .panes
            .values()
            .flat_map(|panes| panes.iter().map(|p| p.id))
            .collect();

        // Track if any notifications were removed
        let initial_count = self.notification_state.len();

        // Remove notifications for panes that no longer exist
        self.notification_state.retain(|pane_id, _| {
            let exists = current_pane_ids.contains(pane_id);
            if !exists {
                #[cfg(debug_assertions)]
                eprintln!(
                    "zellij-attention: Removing notification for closed pane {}",
                    pane_id
                );
            }
            exists
        });

        // Persist if any notifications were removed
        if self.notification_state.len() != initial_count {
            let persisted = PersistedState {
                notifications: self.notification_state.clone(),
            };
            if let Err(e) = save_state(&persisted) {
                eprintln!("zellij-attention: Failed to save state: {}", e);
            }
        }
    }
}

impl ZellijPlugin for State {
    fn load(&mut self, _configuration: BTreeMap<String, String>) {
        // Request permissions needed for tab/pane state
        request_permission(&[
            PermissionType::ReadApplicationState,
            PermissionType::ChangeApplicationState,
        ]);

        // Subscribe to all events upfront
        // (Can't call subscribe() from update() - causes double-borrow panic)
        subscribe(&[
            EventType::PermissionRequestResult,
            EventType::TabUpdate,
            EventType::PaneUpdate,
            EventType::ModeUpdate,
            EventType::Mouse,
        ]);

        // Load persisted state
        self.notification_state = load_state().notifications;

        eprintln!("zellij-attention: loaded\n");
    }

    fn update(&mut self, event: Event) -> bool {
        match event {
            Event::PermissionRequestResult(status) => {
                self.permissions_granted = status == PermissionStatus::Granted;
                // Tab-bar plugins should not be selectable
                // This also gives us the full row for content (pane_content_rows: 1)
                set_selectable(false);
                eprintln!("zellij-attention: permissions={}, selectable=false\n", self.permissions_granted);
                true
            }
            Event::TabUpdate(tab_info) => {
                self.tabs = tab_info;
                self.check_and_clear_focus();
                #[cfg(debug_assertions)]
                eprintln!("zellij-attention: TabUpdate - {} tabs", self.tabs.len());
                true // Will trigger render
            }
            Event::PaneUpdate(pane_manifest) => {
                self.panes = pane_manifest;
                self.cleanup_stale_panes();
                self.check_and_clear_focus();
                #[cfg(debug_assertions)]
                eprintln!(
                    "zellij-attention: PaneUpdate - {} tabs with panes",
                    self.panes.panes.len()
                );
                true // Will trigger render
            }
            Event::ModeUpdate(mode_info) => {
                self.mode_info = mode_info;
                #[cfg(debug_assertions)]
                eprintln!(
                    "zellij-attention: ModeUpdate - mode: {:?}",
                    self.mode_info.mode
                );
                true // Re-render (theme may have changed)
            }
            Event::Mouse(mouse_event) => {
                match mouse_event {
                    Mouse::LeftClick(_row, col) => {
                        let col = col as usize;
                        // Find which tab was clicked based on tab_positions
                        for (idx, (start, end)) in self.tab_positions.iter().enumerate() {
                            if col >= *start && col < *end {
                                // go_to_tab is 1-indexed, tab indices are 0-indexed
                                go_to_tab((idx + 1) as u32);
                                return true;
                            }
                        }
                        false
                    }
                    _ => false, // Ignore other mouse events
                }
            }
            _ => false,
        }
    }

    fn render(&mut self, _rows: usize, cols: usize) {
        // Clear tab positions at start of each render
        self.tab_positions.clear();

        if !self.permissions_granted {
            print!("Waiting for permissions...");
            let _ = std::io::stdout().flush();
            return;
        }

        if self.tabs.is_empty() {
            return;
        }

        let colors = &self.mode_info.style.colors;
        let mut output = String::new();
        let mut current_x = 0;

        for tab in &self.tabs {
            // Get notification state for this tab
            let notification = self.get_tab_notification_state(tab.position);

            // Build indicator string
            let indicator = match notification {
                Some(NotificationType::Waiting) => " !",
                Some(NotificationType::Completed) => " *",
                None => "",
            };

            // Build tab text: " {position+1}:{name}{indicator} "
            let tab_text = format!(" {}:{}{} ", tab.position + 1, tab.name, indicator);
            let tab_width = tab_text.chars().count();

            // Check if this tab would overflow terminal width
            if current_x + tab_width > cols {
                break;
            }

            // Choose colors based on active state
            let (fg_color, bg_color) = if tab.active {
                (colors.ribbon_selected.base, colors.ribbon_selected.background)
            } else {
                (colors.ribbon_unselected.base, colors.ribbon_unselected.background)
            };

            let fg = palette_color_to_ansi_fg(fg_color);
            let bg = palette_color_to_ansi_bg(bg_color);

            // If there's a notification, colorize the indicator portion
            if let Some(notification_type) = notification {
                let main_text = format!(" {}:{}", tab.position + 1, tab.name);
                let indicator_color = match notification_type {
                    NotificationType::Waiting => {
                        palette_color_to_ansi_fg(colors.exit_code_error.base)
                    }
                    NotificationType::Completed => {
                        palette_color_to_ansi_fg(colors.exit_code_success.base)
                    }
                };
                output.push_str(&format!(
                    "{}{}{}{}{} \x1b[0m",
                    bg, fg, main_text, indicator_color, indicator
                ));
            } else {
                output.push_str(&format!("{}{}{}\x1b[0m", bg, fg, tab_text));
            }

            // Track tab position for mouse clicks
            self.tab_positions.push((current_x, current_x + tab_width));
            current_x += tab_width;
        }

        // Fill remaining space with background color
        if current_x < cols {
            let bg = palette_color_to_ansi_bg(colors.ribbon_unselected.background);
            output.push_str(&format!("{}{}\x1b[0m", bg, " ".repeat(cols - current_x)));
        }

        // Clear to end of line and print
        output.push_str("\x1b[0K");
        print!("{}", output);
        let _ = std::io::stdout().flush();
    }

    fn pipe(&mut self, pipe_message: PipeMessage) -> bool {
        eprintln!("zellij-attention: pipe name={} payload={:?} args={:?}\n",
            pipe_message.name, pipe_message.payload, pipe_message.args);

        // Only handle messages to "notification" pipe, silently ignore others
        if pipe_message.name != "notification" {
            return false;
        }

        // Parse event_type and pane_id from either payload (JSON) or args
        let (event_type, pane_id) = if let Some(payload) = &pipe_message.payload {
            // JSON payload format: {"event_type":"waiting","pane_id":0}
            match serde_json::from_str::<PipeEvent>(payload) {
                Ok(e) => (e.event_type, e.pane_id),
                Err(e) => {
                    eprintln!("zellij-attention: Failed to parse JSON: {}\n", e);
                    return false;
                }
            }
        } else {
            // Args format: --args "event_type=waiting,pane_id=0"
            let event_type = match pipe_message.args.get("event_type") {
                Some(t) => t.clone(),
                None => {
                    eprintln!("zellij-attention: Missing event_type in args\n");
                    return false;
                }
            };
            let pane_id: u32 = match pipe_message.args.get("pane_id") {
                Some(id) => match id.parse() {
                    Ok(n) => n,
                    Err(_) => {
                        eprintln!("zellij-attention: Invalid pane_id: {}\n", id);
                        return false;
                    }
                },
                None => {
                    eprintln!("zellij-attention: Missing pane_id in args\n");
                    return false;
                }
            };
            (event_type, pane_id)
        };

        // Normalize event_type to lowercase and match
        let notification_type = match event_type.to_lowercase().as_str() {
            "waiting" => NotificationType::Waiting,
            "completed" => NotificationType::Completed,
            unknown => {
                eprintln!("zellij-attention: Unknown event type: {}\n", unknown);
                return false;
            }
        };

        // Latest wins: create new HashSet with single entry, replacing any existing
        let mut notifications = HashSet::new();
        notifications.insert(notification_type);
        self.notification_state.insert(pane_id, notifications);

        eprintln!(
            "zellij-attention: Set pane {} to {:?}\n",
            pane_id, notification_type
        );

        // Persist state change
        let persisted = PersistedState {
            notifications: self.notification_state.clone(),
        };
        if let Err(e) = save_state(&persisted) {
            eprintln!("zellij-attention: Failed to save state: {}", e);
        }

        true // Trigger re-render
    }
}

register_plugin!(State);
