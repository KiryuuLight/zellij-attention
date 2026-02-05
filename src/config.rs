//! User-configurable notification appearance.

use std::collections::BTreeMap;

/// Configuration for notification appearance.
#[derive(Debug, Clone)]
pub struct NotificationConfig {
    /// Whether notifications are enabled
    pub enabled: bool,
    /// Icon for waiting state (e.g., "⏳")
    pub waiting_icon: String,
    /// Icon for completed state (e.g., "✓")
    pub completed_icon: String,
}

impl Default for NotificationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            waiting_icon: "⏳".to_string(),
            completed_icon: "✓".to_string(),
        }
    }
}

impl NotificationConfig {
    /// Parse configuration from Zellij layout configuration.
    ///
    /// Accepts flat key-value pairs:
    /// - `enabled`: "true" enables, anything else disables
    /// - `waiting_icon`: icon string (warns if > 4 chars)
    /// - `completed_icon`: icon string (warns if > 4 chars)
    ///
    /// Invalid values fall back to defaults with warnings.
    pub fn from_configuration(config: &BTreeMap<String, String>) -> Self {
        let mut result = Self::default();

        // Parse enabled flag
        if let Some(enabled) = config.get("enabled") {
            result.enabled = enabled == "true";
        }

        // Parse waiting_icon
        if let Some(icon) = config.get("waiting_icon") {
            if icon.chars().count() > 4 {
                eprintln!(
                    "zellij-attention: Warning: waiting_icon '{}' is longer than 4 chars, may not display well",
                    icon
                );
            }
            result.waiting_icon = icon.clone();
        }

        // Parse completed_icon
        if let Some(icon) = config.get("completed_icon") {
            if icon.chars().count() > 4 {
                eprintln!(
                    "zellij-attention: Warning: completed_icon '{}' is longer than 4 chars, may not display well",
                    icon
                );
            }
            result.completed_icon = icon.clone();
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = NotificationConfig::default();
        assert!(config.enabled);
        assert_eq!(config.waiting_icon, "⏳");
        assert_eq!(config.completed_icon, "✓");
    }

    #[test]
    fn test_from_configuration_empty() {
        let config_map = BTreeMap::new();
        let config = NotificationConfig::from_configuration(&config_map);
        // Should use defaults
        assert!(config.enabled);
        assert_eq!(config.waiting_icon, "⏳");
    }

    #[test]
    fn test_from_configuration_custom() {
        let mut config_map = BTreeMap::new();
        config_map.insert("enabled".to_string(), "true".to_string());
        config_map.insert("waiting_icon".to_string(), "!".to_string());
        config_map.insert("completed_icon".to_string(), "*".to_string());

        let config = NotificationConfig::from_configuration(&config_map);
        assert!(config.enabled);
        assert_eq!(config.waiting_icon, "!");
        assert_eq!(config.completed_icon, "*");
    }

    #[test]
    fn test_from_configuration_disabled() {
        let mut config_map = BTreeMap::new();
        config_map.insert("enabled".to_string(), "false".to_string());

        let config = NotificationConfig::from_configuration(&config_map);
        assert!(!config.enabled);
    }
}
