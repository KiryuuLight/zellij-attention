use serde::{Deserialize, Serialize};

/// Types of notifications a pane can have.
#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub enum NotificationType {
    /// Command is still running
    Waiting,
    /// Command has completed
    Completed,
}
