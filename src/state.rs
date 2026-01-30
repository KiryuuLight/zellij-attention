//! State persistence for notification tracking across plugin reloads.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Types of notifications a pane can have.
#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub enum NotificationType {
    /// Command is still running
    Waiting,
    /// Command has completed
    Completed,
}

/// State that persists across plugin reloads.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct PersistedState {
    /// Notification state per pane ID
    pub notifications: HashMap<u32, HashSet<NotificationType>>,
}

const STATE_PATH: &str = "/data/state.bin";
const STATE_TMP_PATH: &str = "/data/state.bin.tmp";

/// Save state to persistent storage.
///
/// Uses atomic write pattern: writes to temp file first, then renames.
pub fn save_state(state: &PersistedState) -> Result<(), Box<dyn std::error::Error>> {
    let encoded = bincode::serde::encode_to_vec(state, bincode::config::standard())?;
    std::fs::write(STATE_TMP_PATH, &encoded)?;
    std::fs::rename(STATE_TMP_PATH, STATE_PATH)?;
    Ok(())
}

/// Load state from persistent storage.
///
/// Returns default state on any error (file missing, corruption, etc.).
pub fn load_state() -> PersistedState {
    match std::fs::read(STATE_PATH) {
        Ok(data) => {
            match bincode::serde::decode_from_slice(&data, bincode::config::standard()) {
                Ok((state, _)) => state,
                Err(e) => {
                    #[cfg(debug_assertions)]
                    eprintln!("zellij-attention: Failed to deserialize state: {}", e);
                    PersistedState::default()
                }
            }
        }
        Err(e) => {
            #[cfg(debug_assertions)]
            eprintln!("zellij-attention: Failed to read state file: {}", e);
            PersistedState::default()
        }
    }
}
