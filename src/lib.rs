pub mod config;
pub mod state;

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, HashMap, HashSet};
use zellij_tile::prelude::*;
use zellij_tile::shim::{list_clients, rename_tab, unblock_cli_pipe_input};

use crate::config::NotificationConfig;
use crate::state::NotificationType;

#[derive(Default)]
pub struct State {
    permissions_granted: bool,
    /// Whether this plugin instance has an active client.
    /// Becomes false when the owning client detaches, leaving an orphaned instance.
    /// Orphaned instances must not rename tabs or process notifications to avoid
    /// conflicting with the live instance (causes flickering).
    is_active: bool,
    pub(crate) tabs: Vec<TabInfo>,
    pub(crate) panes: PaneManifest,
    pub(crate) notification_state: HashMap<u32, HashSet<NotificationType>>,
    /// Maps pane_id → tab name (stripped) at the time the notification was set.
    /// Used to verify pane-to-tab mapping during reorders.
    pub(crate) notified_tab_names: HashMap<u32, String>,
    pub(crate) config: NotificationConfig,
    updating_tabs: bool,
    /// Tab positions where we've issued a rename (add icon or strip).
    /// Prevents re-processing on the bounced TabUpdate before Zellij catches up.
    pub(crate) pending_renames: HashSet<usize>,
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
    /// Only clears if the active tab's name shows our notification icon,
    /// preventing false clears during tab reorders when pane/tab data is out of sync.
    /// Returns true if any notification was cleared.
    pub(crate) fn check_and_clear_focus(&mut self) -> bool {
        if self.config.enabled {
            let active_tab = self.tabs.iter().find(|t| t.active);
            if let Some(active_tab) = active_tab {
                if !self.tab_name_has_icon(&active_tab.name) {
                    return false;
                }
            }
        }
        if let Some(focused_pane_id) = self.determine_focused_pane() {
            if self.notification_state.remove(&focused_pane_id).is_some() {
                self.notified_tab_names.remove(&focused_pane_id);
                #[cfg(debug_assertions)]
                eprintln!(
                    "zellij-attention: Cleared notifications for focused pane {}",
                    focused_pane_id
                );
                return true;
            }
        }
        false
    }

    /// Removes notification entries for pane IDs that no longer exist.
    /// Returns true if any stale entries were removed.
    pub(crate) fn clean_stale_notifications(&mut self) -> bool {
        if self.notification_state.is_empty() || self.panes.panes.is_empty() {
            return false;
        }

        let current_pane_ids: HashSet<u32> = self
            .panes
            .panes
            .values()
            .flat_map(|panes| panes.iter().filter(|p| !p.is_plugin).map(|p| p.id))
            .collect();

        let stale_ids: Vec<u32> = self
            .notification_state
            .keys()
            .filter(|id| !current_pane_ids.contains(id))
            .copied()
            .collect();

        if stale_ids.is_empty() {
            return false;
        }

        for id in &stale_ids {
            self.notification_state.remove(id);
            self.notified_tab_names.remove(id);
            #[cfg(debug_assertions)]
            eprintln!(
                "zellij-attention: Removed stale notification for pane {}",
                id
            );
        }

        true
    }

    /// Checks if a tab name ends with one of our notification icon suffixes.
    pub(crate) fn tab_name_has_icon(&self, name: &str) -> bool {
        let waiting_suffix = format!(" {}", self.config.waiting_icon);
        let completed_suffix = format!(" {}", self.config.completed_icon);
        name.ends_with(&waiting_suffix) || name.ends_with(&completed_suffix)
    }

    /// Strips notification icon suffixes from a tab name.
    pub(crate) fn strip_icons(&self, name: &str) -> String {
        let mut result = name.to_string();
        for icon in [&self.config.waiting_icon, &self.config.completed_icon] {
            let suffix = format!(" {}", icon);
            while result.ends_with(&suffix) {
                result.truncate(result.len() - suffix.len());
            }
        }
        result
    }

    /// Find the tab name (stripped) for a pane by looking up which tab contains it.
    fn find_tab_name_for_pane(&self, pane_id: u32) -> Option<String> {
        for tab in &self.tabs {
            if let Some(panes) = self.panes.panes.get(&tab.position) {
                if panes.iter().any(|p| !p.is_plugin && p.id == pane_id) {
                    let name = if tab.name.is_empty() {
                        format!("Tab #{}", tab.position + 1)
                    } else {
                        self.strip_icons(&tab.name)
                    };
                    return Some(name);
                }
            }
        }
        None
    }

    pub(crate) fn get_tab_notification_state(&self, tab_position: usize) -> Option<NotificationType> {
        let tab = self.tabs.iter().find(|t| t.position == tab_position);
        let tab_base_name = tab.map(|t| self.strip_icons(&t.name));
        let panes = self.panes.panes.get(&tab_position)?;
        let mut has_completed = false;

        for pane in panes {
            if pane.is_plugin {
                continue;
            }
            if let Some(notifications) = self.notification_state.get(&pane.id) {
                // Verify this pane actually belongs to this tab (not a stale mapping)
                if let Some(expected_name) = self.notified_tab_names.get(&pane.id) {
                    if let Some(ref actual_name) = tab_base_name {
                        if expected_name != actual_name {
                            #[cfg(debug_assertions)]
                            eprintln!(
                                "zellij-attention: Skipping pane {} at pos={}: expected tab '{}', got '{}'",
                                pane.id, tab_position, expected_name, actual_name
                            );
                            continue;
                        }
                    }
                }
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

    /// Updates tab names to show notification icons or strip stale ones.
    /// Derives original names from strip_icons() — no position-keyed cache needed.
    /// This makes tab reordering safe.
    fn update_tab_names(&mut self) {
        if self.updating_tabs || !self.config.enabled {
            return;
        }
        self.updating_tabs = true;

        // Fast path: no notifications and no pending renames — only check for stale icons
        if self.notification_state.is_empty() && self.pending_renames.is_empty() {
            for tab in &self.tabs {
                if self.tab_name_has_icon(&tab.name)
                    && !self.notified_tab_names.values().any(|name| {
                        let base = self.strip_icons(&tab.name);
                        name == &base
                    })
                {
                    let base_name = self.strip_icons(&tab.name);
                    #[cfg(debug_assertions)]
                    eprintln!(
                        "zellij-attention: Stripping stale icon from tab pos={} '{}' -> '{}'",
                        tab.position, tab.name, base_name
                    );
                    self.pending_renames.insert(tab.position);
                    rename_tab((tab.position + 1) as u32, &base_name);
                }
            }
            self.updating_tabs = false;
            return;
        }

        for tab in &self.tabs {
            let base_name = if tab.name.is_empty() {
                format!("Tab #{}", tab.position + 1)
            } else {
                self.strip_icons(&tab.name)
            };

            if let Some(notification) = self.get_tab_notification_state(tab.position) {
                let icon = match notification {
                    NotificationType::Waiting => &self.config.waiting_icon,
                    NotificationType::Completed => &self.config.completed_icon,
                };
                let new_name = format!("{} {}", base_name, icon);

                if tab.name != new_name {
                    #[cfg(debug_assertions)]
                    eprintln!(
                        "zellij-attention: RENAME tab pos={} '{}' -> '{}'",
                        tab.position, tab.name, new_name
                    );
                    self.pending_renames.insert(tab.position);
                    rename_tab((tab.position + 1) as u32, &new_name);
                } else {
                    self.pending_renames.remove(&tab.position);
                }
            } else if self.pending_renames.contains(&tab.position) {
                // We issued a rename for this position; wait for Zellij to catch up
                if !self.tab_name_has_icon(&tab.name) {
                    self.pending_renames.remove(&tab.position);
                }
            } else if self.tab_name_has_icon(&tab.name) {
                // Check if any active notification expects a tab with this name.
                // If so, the icon isn't stale — the tab just moved to a new position.
                if self.notified_tab_names.values().any(|name| name == &base_name) {
                    continue;
                }
                // Truly stale icon — strip it
                #[cfg(debug_assertions)]
                eprintln!(
                    "zellij-attention: Stripping stale icon from tab pos={} '{}' -> '{}'",
                    tab.position, tab.name, base_name
                );
                self.pending_renames.insert(tab.position);
                rename_tab((tab.position + 1) as u32, &base_name);
            }
        }

        // Clean up pending_renames for tabs that no longer exist
        if !self.tabs.is_empty() {
            let valid_positions: HashSet<usize> = self.tabs.iter().map(|t| t.position).collect();
            self.pending_renames.retain(|pos| valid_positions.contains(pos));
        }

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
        ]);

        subscribe(&[
            EventType::PermissionRequestResult,
            EventType::TabUpdate,
            EventType::PaneUpdate,
            EventType::ListClients,
        ]);

        self.is_active = true;
        self.config = NotificationConfig::from_configuration(&configuration);

        eprintln!("zellij-attention: v{} loaded\n", env!("CARGO_PKG_VERSION"));
    }

    fn update(&mut self, event: Event) -> bool {
        match event {
            Event::PermissionRequestResult(status) => {
                self.permissions_granted = status == PermissionStatus::Granted;
                set_selectable(false);

                // Check if we have an active client (detects orphaned instances)
                list_clients();

                // Strip any stale icons on startup
                self.update_tab_names();
                true
            }
            Event::ListClients(clients) => {
                let was_active = self.is_active;
                self.is_active = clients.iter().any(|c| c.is_current_client);
                if was_active && !self.is_active {
                    eprintln!("zellij-attention: no active client, going dormant");
                }
                false
            }
            Event::TabUpdate(tab_info) => {
                self.tabs = tab_info;
                // Periodically re-check client liveness
                list_clients();
                if !self.is_active {
                    return false;
                }
                self.check_and_clear_focus();
                self.clean_stale_notifications();
                self.update_tab_names();
                false
            }
            Event::PaneUpdate(pane_manifest) => {
                self.panes = pane_manifest;
                if !self.is_active {
                    return false;
                }
                self.check_and_clear_focus();
                self.clean_stale_notifications();
                self.update_tab_names();
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

        // Orphaned instance — unblock pipe and skip processing
        if !self.is_active {
            unblock_cli_pipe_input(&pipe_message.name);
            return false;
        }

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

        // Unblock the CLI pipe immediately so the caller never hangs,
        // regardless of what happens during state mutation or tab renaming.
        unblock_cli_pipe_input(&pipe_message.name);

        let mut notifications = HashSet::new();
        notifications.insert(notification_type);
        self.notification_state.insert(pane_id, notifications);

        // Record which tab this pane belongs to, so we can verify during reorders
        if let Some(tab_name) = self.find_tab_name_for_pane(pane_id) {
            #[cfg(debug_assertions)]
            eprintln!("zellij-attention: Notification for pane {} in tab '{}'", pane_id, tab_name);
            self.notified_tab_names.insert(pane_id, tab_name);
        }

        self.update_tab_names();

        false
    }
}
