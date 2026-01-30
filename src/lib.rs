use std::collections::BTreeMap;
use zellij_tile::prelude::*;

#[derive(Default)]
struct State {
    permissions_granted: bool,
}

impl ZellijPlugin for State {
    fn load(&mut self, _configuration: BTreeMap<String, String>) {
        // Request permissions needed for future functionality
        request_permission(&[
            PermissionType::ReadApplicationState,
            PermissionType::ChangeApplicationState,
        ]);

        // Subscribe to permission result event
        subscribe(&[EventType::PermissionRequestResult]);
    }

    fn update(&mut self, event: Event) -> bool {
        #[cfg(debug_assertions)]
        eprintln!("zellij-attention: Received event: {:?}", event);

        match event {
            Event::PermissionRequestResult(status) => {
                self.permissions_granted = status == PermissionStatus::Granted;
                true // Re-render to show updated status
            }
            _ => false,
        }
    }

    fn render(&mut self, rows: usize, cols: usize) {
        if self.permissions_granted {
            println!("zellij-attention: Permissions granted - plugin ready");
            println!("Pane size: {}x{}", cols, rows);
        } else {
            println!("zellij-attention: Waiting for permissions...");
        }
    }
}

register_plugin!(State);
