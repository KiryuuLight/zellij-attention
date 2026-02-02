//! User-configurable notification appearance.

use std::collections::BTreeMap;

/// Configuration for notification appearance.
#[derive(Debug, Clone)]
pub struct NotificationConfig {
    /// Whether notifications are enabled
    pub enabled: bool,
    /// Hex color for waiting state (e.g., "#f5a623")
    pub waiting_color: String,
    /// Icon for waiting state (e.g., "⏳")
    pub waiting_icon: String,
    /// Hex color for completed state (e.g., "#7cb342")
    pub completed_color: String,
    /// Icon for completed state (e.g., "✓")
    pub completed_icon: String,
}

impl Default for NotificationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            waiting_color: "#f5a623".to_string(),
            waiting_icon: "⏳".to_string(),
            completed_color: "#7cb342".to_string(),
            completed_icon: "✓".to_string(),
        }
    }
}

impl NotificationConfig {
    /// Parse configuration from Zellij layout configuration.
    ///
    /// Accepts flat key-value pairs:
    /// - `enabled`: "true" enables, anything else disables
    /// - `waiting_color`: hex color with or without # prefix
    /// - `waiting_icon`: icon string (warns if > 4 chars)
    /// - `completed_color`: hex color with or without # prefix
    /// - `completed_icon`: icon string (warns if > 4 chars)
    ///
    /// Invalid values fall back to defaults with warnings.
    pub fn from_configuration(config: &BTreeMap<String, String>) -> Self {
        let mut result = Self::default();

        // Parse enabled flag
        if let Some(enabled) = config.get("enabled") {
            result.enabled = enabled == "true";
        }

        // Parse waiting_color
        if let Some(color) = config.get("waiting_color") {
            if validate_hex_color(color) {
                result.waiting_color = color.clone();
            } else {
                eprintln!(
                    "zellij-attention: Invalid waiting_color '{}', using default '{}'",
                    color, result.waiting_color
                );
            }
        }

        // Parse completed_color
        if let Some(color) = config.get("completed_color") {
            if validate_hex_color(color) {
                result.completed_color = color.clone();
            } else {
                eprintln!(
                    "zellij-attention: Invalid completed_color '{}', using default '{}'",
                    color, result.completed_color
                );
            }
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

/// Validate that a string is a valid hex color.
///
/// Accepts formats:
/// - #RGB (3 hex chars)
/// - #RRGGBB (6 hex chars)
/// - RGB (3 hex chars, without #)
/// - RRGGBB (6 hex chars, without #)
///
/// Returns true if valid, false otherwise.
fn validate_hex_color(color: &str) -> bool {
    let color = color.trim_start_matches('#');
    let len = color.len();
    (len == 3 || len == 6) && color.chars().all(|c| c.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_hex_color() {
        // Valid formats
        assert!(validate_hex_color("#fff"));
        assert!(validate_hex_color("#FFFFFF"));
        assert!(validate_hex_color("fff"));
        assert!(validate_hex_color("FFFFFF"));
        assert!(validate_hex_color("#f5a623"));
        assert!(validate_hex_color("#7cb342"));

        // Invalid formats
        assert!(!validate_hex_color("#ff")); // Too short
        assert!(!validate_hex_color("#fffffff")); // Too long
        assert!(!validate_hex_color("#xyz")); // Non-hex chars
        assert!(!validate_hex_color("")); // Empty
        assert!(!validate_hex_color("#")); // Just #
    }

    #[test]
    fn test_default_config() {
        let config = NotificationConfig::default();
        assert!(config.enabled);
        assert_eq!(config.waiting_color, "#f5a623");
        assert_eq!(config.waiting_icon, "⏳");
        assert_eq!(config.completed_color, "#7cb342");
        assert_eq!(config.completed_icon, "✓");
    }

    #[test]
    fn test_from_configuration_empty() {
        let config_map = BTreeMap::new();
        let config = NotificationConfig::from_configuration(&config_map);
        // Should use defaults
        assert!(config.enabled);
        assert_eq!(config.waiting_color, "#f5a623");
    }

    #[test]
    fn test_from_configuration_custom() {
        let mut config_map = BTreeMap::new();
        config_map.insert("enabled".to_string(), "true".to_string());
        config_map.insert("waiting_color".to_string(), "#ff0000".to_string());
        config_map.insert("waiting_icon".to_string(), "!".to_string());
        config_map.insert("completed_color".to_string(), "#00ff00".to_string());
        config_map.insert("completed_icon".to_string(), "*".to_string());

        let config = NotificationConfig::from_configuration(&config_map);
        assert!(config.enabled);
        assert_eq!(config.waiting_color, "#ff0000");
        assert_eq!(config.waiting_icon, "!");
        assert_eq!(config.completed_color, "#00ff00");
        assert_eq!(config.completed_icon, "*");
    }

    #[test]
    fn test_from_configuration_disabled() {
        let mut config_map = BTreeMap::new();
        config_map.insert("enabled".to_string(), "false".to_string());

        let config = NotificationConfig::from_configuration(&config_map);
        assert!(!config.enabled);
    }

    #[test]
    fn test_from_configuration_invalid_color_fallback() {
        let mut config_map = BTreeMap::new();
        config_map.insert("waiting_color".to_string(), "not-a-color".to_string());

        let config = NotificationConfig::from_configuration(&config_map);
        // Should fall back to default
        assert_eq!(config.waiting_color, "#f5a623");
    }
}
