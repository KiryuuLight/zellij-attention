mod state;

use std::collections::{BTreeMap, HashMap, HashSet};
use zellij_tile::prelude::*;

use crate::state::{load_state, save_state, NotificationType, PersistedState};

#[derive(Default)]
struct State {
    permissions_granted: bool,
    tabs: Vec<TabInfo>,
    panes: PaneManifest,
    notification_state: HashMap<u32, HashSet<NotificationType>>,
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
                self.check_and_clear_focus();
                #[cfg(debug_assertions)]
                eprintln!(
                    "zellij-attention: PaneUpdate - {} tabs with panes",
                    self.panes.panes.len()
                );
                true // Will trigger render
            }
            _ => false,
        }
    }

    fn render(&mut self, _rows: usize, _cols: usize) {
        if self.permissions_granted {
            println!(
                "zellij-attention: {} tabs, {} pane groups, {} notifications",
                self.tabs.len(),
                self.panes.panes.len(),
                self.notification_state.len()
            );
        } else {
            println!("zellij-attention: Waiting for permissions...");
        }
    }
}

register_plugin!(State);
