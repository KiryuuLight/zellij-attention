pub mod config;
pub mod state;

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, HashMap, HashSet};
use zellij_tile::prelude::*;
use zellij_tile::shim::{rename_tab, unblock_cli_pipe_input};

use crate::config::NotificationConfig;
use crate::state::NotificationType;

#[derive(Default)]
pub struct State {
    permissions_granted: bool,
    pub(crate) tabs: Vec<TabInfo>,
    pub(crate) panes: PaneManifest,
    pub(crate) notification_state: HashMap<u32, HashSet<NotificationType>>,
    pub(crate) original_tab_names: HashMap<usize, String>,
    pub(crate) config: NotificationConfig,
    updating_tabs: bool,
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
    pub(crate) fn check_and_clear_focus(&mut self) -> bool {
        if let Some(focused_pane_id) = self.determine_focused_pane() {
            if self.notification_state.remove(&focused_pane_id).is_some() {
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
            #[cfg(debug_assertions)]
            eprintln!(
                "zellij-attention: Removed stale notification for pane {}",
                id
            );
        }

        true
    }

    /// Returns true if there are original_tab_names entries waiting to be
    /// restored (i.e., their tab positions have no active notifications).
    pub(crate) fn has_pending_restores(&self) -> bool {
        self.original_tab_names.keys().any(|pos| {
            self.get_tab_notification_state(*pos).is_none()
        })
    }

    /// Returns true if any tab has a stale icon suffix with no active notification.
    pub(crate) fn has_stale_icons(&self) -> bool {
        for tab in &self.tabs {
            if self.get_tab_notification_state(tab.position).is_some() {
                continue;
            }
            if self.original_tab_names.contains_key(&tab.position) {
                continue; // will be handled by restore logic
            }
            if self.tab_name_has_icon(&tab.name) {
                return true;
            }
        }
        false
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

    pub(crate) fn get_tab_notification_state(&self, tab_position: usize) -> Option<NotificationType> {
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
                    let original = if tab.name.is_empty() {
                        format!("Tab #{}", tab.position + 1)
                    } else {
                        // Strip any trailing notification icons from stale tab.name
                        // to prevent accumulation (e.g. "Name ⏳ ⏳" → "Name")
                        self.strip_icons(&tab.name)
                    };
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
            if let Some(tab) = self.tabs.iter().find(|t| t.position == pos) {
                if let Some(original_name) = self.original_tab_names.remove(&pos) {
                    if tab.name != original_name {
                        // Zellij's RenameTab handler subtracts 1 (expects 1-indexed)
                        rename_tab((pos + 1) as u32, &original_name);
                    }
                }
            }
            // If tab not found yet (e.g. tabs not loaded), keep the entry for later
        }

        // Strip stale icons from tabs that have no notification and no pending restore.
        // This handles the case where Zellij persisted renamed tab names across sessions
        // but the plugin's state was lost or pane IDs changed.
        for tab in &self.tabs {
            if notified_positions.contains(&tab.position) {
                continue;
            }
            if self.original_tab_names.contains_key(&tab.position) {
                continue;
            }
            if self.tab_name_has_icon(&tab.name) {
                let clean_name = self.strip_icons(&tab.name);
                eprintln!(
                    "zellij-attention: Stripping stale icon from tab pos={} '{}' -> '{}'",
                    tab.position, tab.name, clean_name
                );
                rename_tab((tab.position + 1) as u32, &clean_name);
            }
        }

        // Clean up cached names for tabs that no longer exist
        // Only clean up if we actually have tab data (avoid wiping on startup before tabs load)
        if !self.tabs.is_empty() {
            let valid_positions: HashSet<usize> = self.tabs.iter().map(|t| t.position).collect();
            self.original_tab_names.retain(|pos, _| valid_positions.contains(pos));
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
        ]);

        self.config = NotificationConfig::from_configuration(&configuration);

        eprintln!("zellij-attention: loaded\n");
    }

    fn update(&mut self, event: Event) -> bool {
        match event {
            Event::PermissionRequestResult(status) => {
                self.permissions_granted = status == PermissionStatus::Granted;
                set_selectable(false);

                // Strip any stale icons on startup
                self.update_tab_names();
                true
            }
            Event::TabUpdate(tab_info) => {
                self.tabs = tab_info;
                let focus_cleared = self.check_and_clear_focus();
                let stale_cleaned = self.clean_stale_notifications();
                if focus_cleared || stale_cleaned || self.has_pending_restores()
                    || self.has_stale_icons()
                {
                    self.update_tab_names();
                }
                false
            }
            Event::PaneUpdate(pane_manifest) => {
                self.panes = pane_manifest;
                let focus_cleared = self.check_and_clear_focus();
                let stale_cleaned = self.clean_stale_notifications();
                if focus_cleared || stale_cleaned || self.has_pending_restores()
                    || self.has_stale_icons()
                {
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

        // Unblock the CLI pipe immediately so the caller never hangs,
        // regardless of what happens during state mutation or tab renaming.
        unblock_cli_pipe_input(&pipe_message.name);

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

        self.update_tab_names();

        false
    }
}
