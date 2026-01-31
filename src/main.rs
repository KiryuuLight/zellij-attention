mod state;

use std::collections::{BTreeMap, HashMap, HashSet};
use zellij_tile::prelude::*;

use crate::state::{load_state, save_state, NotificationType, PersistedState};

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
        // Request permissions needed for future functionality
        request_permission(&[
            PermissionType::ReadApplicationState,
            PermissionType::ChangeApplicationState,
        ]);

        // Subscribe to events
        subscribe(&[
            EventType::PermissionRequestResult,
            EventType::TabUpdate,
            EventType::PaneUpdate,
            EventType::ModeUpdate,
        ]);

        // Load persisted state
        self.notification_state = load_state().notifications;
    }

    fn update(&mut self, event: Event) -> bool {
        #[cfg(debug_assertions)]
        eprintln!("zellij-attention: Received event: {:?}", event);

        match event {
            Event::PermissionRequestResult(status) => {
                self.permissions_granted = status == PermissionStatus::Granted;
                true // Re-render to show updated status
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
            _ => false,
        }
    }

    fn render(&mut self, _rows: usize, _cols: usize) {
        if self.permissions_granted {
            let focused = self.determine_focused_pane();
            println!(
                "Tabs: {} | Panes: {} | Notifications: {} | Focused: {:?}",
                self.tabs.len(),
                self.panes.panes.values().map(|p| p.len()).sum::<usize>(),
                self.notification_state.len(),
                focused
            );
        } else {
            println!("zellij-attention: Waiting for permissions...");
        }
    }

    fn pipe(&mut self, pipe_message: PipeMessage) -> bool {
        // Only handle messages to "notification" pipe, silently ignore others
        if pipe_message.name != "notification" {
            return false;
        }

        // Extract payload, log error if missing
        let payload = match pipe_message.payload {
            Some(p) => p,
            None => {
                eprintln!("zellij-attention: No payload in pipe message");
                return false;
            }
        };

        // Parse JSON payload
        let event: PipeEvent = match serde_json::from_str(&payload) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("zellij-attention: Failed to parse pipe payload: {}", e);
                return false;
            }
        };

        // Normalize event_type to lowercase and match
        let notification_type = match event.event_type.to_lowercase().as_str() {
            "waiting" => NotificationType::Waiting,
            "completed" => NotificationType::Completed,
            unknown => {
                eprintln!("zellij-attention: Unknown event type: {}", unknown);
                return false;
            }
        };

        // Latest wins: create new HashSet with single entry, replacing any existing
        let mut notifications = HashSet::new();
        notifications.insert(notification_type);
        self.notification_state.insert(event.pane_id, notifications);

        eprintln!(
            "zellij-attention: Set pane {} to {:?}",
            event.pane_id, notification_type
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
