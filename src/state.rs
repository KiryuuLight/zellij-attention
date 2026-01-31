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
// Write to /host which maps to cwd where zellij started
const STATUS_PATH: &str = "/host/.zellij-attention-status";

/// Save state to persistent storage.
///
/// Uses atomic write pattern: writes to temp file first, then renames.
pub fn save_state(state: &PersistedState) -> Result<(), Box<dyn std::error::Error>> {
    let encoded = bincode::serde::encode_to_vec(state, bincode::config::standard())?;
    std::fs::write(STATE_TMP_PATH, &encoded)?;
    std::fs::rename(STATE_TMP_PATH, STATE_PATH)?;
    Ok(())
}

/// Write status text to file for zjstatus command widget to read.
///
/// This is the communication channel to zjstatus - it polls this file.
pub fn write_status(status: &str) -> Result<(), std::io::Error> {
    std::fs::write(STATUS_PATH, status)
}

/// Load state from persistent storage.
///
/// Returns default state on any error (file missing, corruption, etc.).
pub fn load_state() -> PersistedState {
    match std::fs::read(STATE_PATH) {
        Ok(data) => {
            match bincode::serde::decode_from_slice(&data, bincode::config::standard()) {
                Ok((state, _)) => state,
                Err(_e) => {
                    #[cfg(debug_assertions)]
                    eprintln!("zellij-attention: Failed to deserialize state: {}", _e);
                    PersistedState::default()
                }
            }
        }
        Err(_e) => {
            #[cfg(debug_assertions)]
            eprintln!("zellij-attention: Failed to read state file: {}", _e);
            PersistedState::default()
        }
    }
}
