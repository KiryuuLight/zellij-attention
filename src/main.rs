mod config;
mod state;

use std::collections::{BTreeMap, HashMap, HashSet};
use zellij_tile::prelude::*;
use zellij_tile::shim::{rename_tab, unblock_cli_pipe_input};

use crate::config::NotificationConfig;
use crate::state::{load_state, save_state, NotificationType, PersistedState};

struct State {
    permissions_granted: bool,
    tabs: Vec<TabInfo>,
    panes: PaneManifest,
    notification_state: HashMap<u32, HashSet<NotificationType>>,
    original_tab_names: HashMap<usize, String>,
    config: NotificationConfig,
    updating_tabs: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            permissions_granted: false,
            tabs: Vec::new(),
            panes: PaneManifest::default(),
            notification_state: HashMap::new(),
            original_tab_names: HashMap::new(),
            config: NotificationConfig::default(),
            updating_tabs: false,
        }
    }
}

impl State {
    fn determine_focused_pane(&self) -> Option<u32> {
        let active_tab = self.tabs.iter().find(|t| t.active)?;
        let panes = self.panes.panes.get(&active_tab.position)?;
        let focused = panes.iter().find(|p| {
            !p.is_plugin
                && p.is_focused
                && (p.is_floating == active_tab.are_floating_panes_visible)
        })?;
        Some(focused.id)
    }

    /// Checks if focused pane has notifications and clears them.
    /// Returns true if any notification was cleared.
    fn check_and_clear_focus(&mut self) -> bool {
        if let Some(focused_pane_id) = self.determine_focused_pane() {
            if self.notification_state.remove(&focused_pane_id).is_some() {
                #[cfg(debug_assertions)]
                eprintln!(
                    "zellij-attention: Cleared notifications for focused pane {}",
                    focused_pane_id
                );
                self.persist_state();
                return true;
            }
        }
        false
    }

    fn get_tab_notification_state(&self, tab_position: usize) -> Option<NotificationType> {
        let panes = self.panes.panes.get(&tab_position)?;
        let mut has_completed = false;

        for pane in panes {
            // Skip plugin panes — their IDs overlap with terminal pane IDs
            if pane.is_plugin {
                continue;
            }
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

    fn persist_state(&self) {
        let persisted = PersistedState {
            notifications: self.notification_state.clone(),
            original_tab_names: self.original_tab_names.clone(),
        };
        if let Err(e) = save_state(&persisted) {
            eprintln!("zellij-attention: Failed to save state: {}", e);
        }
    }

    /// Updates tab names to show notification icons or restore original names.
    /// Only called when notification state changes (pipe received, notification cleared).
    /// Uses in-memory state — no disk I/O inside this method.
    fn update_tab_names(&mut self) {
        if self.updating_tabs || !self.config.enabled {
            return;
        }
        self.updating_tabs = true;

        let mut notified_positions: HashSet<usize> = HashSet::new();

        for tab in &self.tabs {
            if let Some(notification) = self.get_tab_notification_state(tab.position) {
                notified_positions.insert(tab.position);

                if !self.original_tab_names.contains_key(&tab.position) {
                    let mut original = if tab.name.is_empty() {
                        format!("Tab #{}", tab.position + 1)
                    } else {
                        tab.name.clone()
                    };
                    // Defensive: strip any trailing notification icons from stale tab.name
                    // to prevent accumulation (e.g. "Name ⏳ ⏳" → "Name")
                    for icon in [&self.config.waiting_icon, &self.config.completed_icon] {
                        let suffix = format!(" {}", icon);
                        while original.ends_with(&suffix) {
                            original.truncate(original.len() - suffix.len());
                        }
                    }
                    self.original_tab_names.insert(tab.position, original);
                }

                let icon = match notification {
                    NotificationType::Waiting => &self.config.waiting_icon,
                    NotificationType::Completed => &self.config.completed_icon,
                };

                let original = self.original_tab_names.get(&tab.position)
                    .cloned()
                    .unwrap_or_else(|| format!("Tab #{}", tab.position + 1));
                let new_name = format!("{} {}", original, icon);

                if tab.name != new_name {
                    #[cfg(debug_assertions)]
                    eprintln!(
                        "zellij-attention: RENAME tab pos={} '{}' -> '{}'",
                        tab.position, tab.name, new_name
                    );
                    // Zellij's RenameTab handler subtracts 1 (expects 1-indexed)
                    rename_tab((tab.position + 1) as u32, &new_name);
                }
            }
        }

        // Restore original names for tabs whose notifications were cleared
        let positions_to_restore: Vec<usize> = self.original_tab_names.keys()
            .filter(|pos| !notified_positions.contains(pos))
            .cloned()
            .collect();

        for pos in positions_to_restore {
            if let Some(original_name) = self.original_tab_names.remove(&pos) {
                if let Some(tab) = self.tabs.iter().find(|t| t.position == pos) {
                    if tab.name != original_name {
                        // Zellij's RenameTab handler subtracts 1 (expects 1-indexed)
                        rename_tab((pos + 1) as u32, &original_name);
                    }
                }
            }
        }

        // Clean up cached names for tabs that no longer exist
        let valid_positions: HashSet<usize> = self.tabs.iter().map(|t| t.position).collect();
        self.original_tab_names.retain(|pos, _| valid_positions.contains(pos));

        // Persist original_tab_names changes
        self.persist_state();

        self.updating_tabs = false;
    }
}

impl ZellijPlugin for State {
    fn load(&mut self, configuration: BTreeMap<String, String>) {
        request_permission(&[
            PermissionType::ReadApplicationState,
            PermissionType::ChangeApplicationState,
            PermissionType::MessageAndLaunchOtherPlugins,
            PermissionType::ReadCliPipes,
            PermissionType::FullHdAccess,
        ]);

        subscribe(&[
            EventType::PermissionRequestResult,
            EventType::TabUpdate,
            EventType::PaneUpdate,
        ]);

        let persisted = load_state();
        self.notification_state = persisted.notifications;
        self.original_tab_names = persisted.original_tab_names;

        self.config = NotificationConfig::from_configuration(&configuration);

        eprintln!("zellij-attention: loaded\n");
    }

    fn update(&mut self, event: Event) -> bool {
        match event {
            Event::PermissionRequestResult(status) => {
                self.permissions_granted = status == PermissionStatus::Granted;
                set_selectable(false);

                // Apply any persisted notifications on startup
                self.update_tab_names();
                true
            }
            Event::TabUpdate(tab_info) => {
                self.tabs = tab_info;
                if self.check_and_clear_focus() {
                    self.update_tab_names();
                }
                false
            }
            Event::PaneUpdate(pane_manifest) => {
                self.panes = pane_manifest;
                // Only update tab names if a notification was cleared
                if self.check_and_clear_focus() {
                    self.update_tab_names();
                }
                false
            }
            _ => false,
        }
    }

    fn render(&mut self, _rows: usize, _cols: usize) {}

    fn pipe(&mut self, pipe_message: PipeMessage) -> bool {
        #[cfg(debug_assertions)]
        eprintln!(
            "zellij-attention: pipe name={} payload={:?}\n",
            pipe_message.name, pipe_message.payload
        );

        let message = if pipe_message.name.starts_with("zellij-attention::") {
            pipe_message.name.clone()
        } else if let Some(ref payload) = pipe_message.payload {
            if payload.starts_with("zellij-attention::") {
                payload.clone()
            } else {
                return false;
            }
        } else {
            return false;
        };

        let parts: Vec<&str> = message.split("::").collect();

        let (event_type, pane_id) = if parts.len() >= 3 {
            let event_type = parts[1].to_string();
            let pane_id: u32 = match parts[2].parse() {
                Ok(n) => n,
                Err(_) => {
                    eprintln!("zellij-attention: Invalid pane_id: {}\n", parts[2]);
                    unblock_cli_pipe_input(&pipe_message.name);
                    return false;
                }
            };
            (event_type, pane_id)
        } else {
            eprintln!("zellij-attention: Invalid format. Use: zellij-attention::EVENT_TYPE::PANE_ID\n");
            unblock_cli_pipe_input(&pipe_message.name);
            return false;
        };

        let notification_type = match event_type.to_lowercase().as_str() {
            "waiting" => NotificationType::Waiting,
            "completed" => NotificationType::Completed,
            unknown => {
                eprintln!("zellij-attention: Unknown event type: {}\n", unknown);
                unblock_cli_pipe_input(&pipe_message.name);
                return false;
            }
        };

        let mut notifications = HashSet::new();
        notifications.insert(notification_type);
        self.notification_state.insert(pane_id, notifications);

        #[cfg(debug_assertions)]
        eprintln!("zellij-attention: Set pane {} to {:?}\n", pane_id, notification_type);

        #[cfg(debug_assertions)]
        {
            for tab in &self.tabs {
                if let Some(panes) = self.panes.panes.get(&tab.position) {
                    let terminal_panes: Vec<String> = panes.iter()
                        .filter(|p| !p.is_plugin)
                        .map(|p| format!("{}", p.id))
                        .collect();
                    let plugin_panes: Vec<String> = panes.iter()
                        .filter(|p| p.is_plugin)
                        .map(|p| format!("{}", p.id))
                        .collect();
                    eprintln!(
                        "zellij-attention: tab pos={} name='{}' terminal_panes=[{}] plugin_panes=[{}]",
                        tab.position, tab.name,
                        terminal_panes.join(","), plugin_panes.join(",")
                    );
                }
            }
        }

        // Persist and update tab names
        self.persist_state();
        self.update_tab_names();

        // Unblock the CLI pipe so the command returns
        unblock_cli_pipe_input(&pipe_message.name);

        false
    }
}

register_plugin!(State);
