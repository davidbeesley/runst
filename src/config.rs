use crate::error::{Error, Result};
use crate::notification::{Notification, NotificationFilter, Urgency};
use colorsys::Rgb;
use log::LevelFilter;
use rust_embed::RustEmbed;
use serde::de::{Deserializer, Error as SerdeError};
use serde::ser::Serializer;
use serde::{Deserialize, Serialize};
use sscanf::scanf;
use std::collections::HashMap;
use std::env;
use std::fmt;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::result::Result as StdResult;
use std::str::{self, FromStr};
use std::time::{SystemTime, UNIX_EPOCH};
use tera::Tera;

/// Window origin/anchor point for positioning.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Origin {
    /// Anchor to top-left corner (default).
    #[default]
    TopLeft,
    /// Anchor to top-right corner.
    TopRight,
    /// Anchor to bottom-left corner.
    BottomLeft,
    /// Anchor to bottom-right corner.
    BottomRight,
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TopLeft => write!(f, "top-left"),
            Self::TopRight => write!(f, "top-right"),
            Self::BottomLeft => write!(f, "bottom-left"),
            Self::BottomRight => write!(f, "bottom-right"),
        }
    }
}

impl FromStr for Origin {
    type Err = Error;
    fn from_str(s: &str) -> StdResult<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "top-left" | "topleft" => Ok(Self::TopLeft),
            "top-right" | "topright" => Ok(Self::TopRight),
            "bottom-left" | "bottomleft" => Ok(Self::BottomLeft),
            "bottom-right" | "bottomright" => Ok(Self::BottomRight),
            _ => Err(Error::Config(format!("invalid origin: {}", s))),
        }
    }
}

/// Environment variable for the configuration file.
const CONFIG_ENV: &str = "RUNST_CONFIG";

/// Name of the default configuration file.
const DEFAULT_CONFIG: &str = concat!(env!("CARGO_PKG_NAME"), ".toml");

/// Embedded (default) configuration.
#[derive(Debug, RustEmbed)]
#[folder = "config/"]
struct EmbeddedConfig;

/// Configuration.
#[derive(Debug, Deserialize, Serialize)]
pub struct Config {
    /// Global configuration.
    pub global: GlobalConfig,
    /// Configuration for low urgency.
    pub urgency_low: UrgencyConfig,
    /// Configuration for normal urgency.
    pub urgency_normal: UrgencyConfig,
    /// Configuration for critical urgency.
    pub urgency_critical: UrgencyConfig,
    /// Color mapping for specific applications (app_name -> hex color).
    #[serde(default)]
    pub app_colors: HashMap<String, String>,
    /// Notification styling rules based on patterns.
    #[serde(default)]
    pub rules: Vec<NotificationRule>,
}

/// A rule for styling notifications based on patterns.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct NotificationRule {
    /// Pattern to match against app_name (glob-style with *).
    #[serde(default)]
    pub app_name: Option<String>,
    /// Pattern to match against summary (glob-style with *).
    #[serde(default)]
    pub summary: Option<String>,
    /// Pattern to match against body (glob-style with *).
    #[serde(default)]
    pub body: Option<String>,
    /// Foreground color to use for matching notifications.
    #[serde(default)]
    pub foreground: Option<String>,
    /// Background color to use for matching notifications.
    #[serde(default)]
    pub background: Option<String>,
}

/// Checks if a value matches a glob-style pattern (case-insensitive).
/// Supports `*` as wildcard.
pub fn glob_match(pattern: &str, value: &str) -> bool {
    let pattern_lower = pattern.to_lowercase();
    let value_lower = value.to_lowercase();

    if !pattern_lower.contains('*') {
        return pattern_lower == value_lower;
    }

    let parts: Vec<&str> = pattern_lower.split('*').collect();

    match parts.as_slice() {
        ["", suffix] => value_lower.ends_with(suffix),
        [prefix, ""] => value_lower.starts_with(prefix),
        [prefix, suffix] => value_lower.starts_with(prefix) && value_lower.ends_with(suffix),
        ["", middle, ""] => value_lower.contains(middle),
        _ => {
            let mut remaining = value_lower.as_str();
            for part in &parts {
                if part.is_empty() {
                    continue;
                }
                if let Some(pos) = remaining.find(part) {
                    remaining = &remaining[pos + part.len()..];
                } else {
                    return false;
                }
            }
            true
        }
    }
}

impl NotificationRule {
    /// Checks if this rule matches the given notification.
    pub fn matches(&self, app_name: &str, summary: &str, body: &str) -> bool {
        // All specified patterns must match
        if let Some(ref pattern) = self.app_name
            && !glob_match(pattern, app_name)
        {
            return false;
        }
        if let Some(ref pattern) = self.summary
            && !glob_match(pattern, summary)
        {
            return false;
        }
        if let Some(ref pattern) = self.body
            && !glob_match(pattern, body)
        {
            return false;
        }
        true
    }
}

impl Config {
    /// Parses the configuration file.
    pub fn parse() -> Result<Self> {
        for config_path in [
            env::var(CONFIG_ENV).ok().map(PathBuf::from),
            dirs::config_dir().map(|p| p.join(env!("CARGO_PKG_NAME")).join(DEFAULT_CONFIG)),
            dirs::home_dir().map(|p| {
                p.join(concat!(".", env!("CARGO_PKG_NAME")))
                    .join(DEFAULT_CONFIG)
            }),
        ]
        .iter()
        .flatten()
        {
            if config_path.exists() {
                let contents = fs::read_to_string(config_path)?;
                let config = toml::from_str(&contents)?;
                return Ok(config);
            }
        }
        if let Some(embedded_config) = EmbeddedConfig::get(DEFAULT_CONFIG)
            .and_then(|v| String::from_utf8(v.data.as_ref().to_vec()).ok())
        {
            let config = toml::from_str(&embedded_config)?;
            Ok(config)
        } else {
            Err(Error::Config(String::from("configuration file not found")))
        }
    }

    /// Returns the appropriate urgency configuration.
    pub fn get_urgency_config(&self, urgency: &Urgency) -> UrgencyConfig {
        match urgency {
            Urgency::Low => self.urgency_low.clone(),
            Urgency::Normal => self.urgency_normal.clone(),
            Urgency::Critical => self.urgency_critical.clone(),
        }
    }

    /// Returns the color for a specific application, if configured.
    /// Supports glob-style patterns with `*` as a wildcard.
    /// Examples: "Claude*" matches "Claude Code", "*bash*" matches "my-bash-script"
    pub fn get_app_color(&self, app_name: &str) -> Option<&String> {
        // First try exact match
        if let Some(color) = self.app_colors.get(app_name) {
            return Some(color);
        }

        // Then try pattern matching
        for (pattern, color) in &self.app_colors {
            if glob_match(pattern, app_name) {
                return Some(color);
            }
        }

        None
    }

    /// Returns the first matching rule for a notification, if any.
    pub fn get_matching_rule(
        &self,
        app_name: &str,
        summary: &str,
        body: &str,
    ) -> Option<&NotificationRule> {
        self.rules
            .iter()
            .find(|rule| rule.matches(app_name, summary, body))
    }
}

/// Global configuration.
#[derive(Debug, Deserialize, Serialize)]
pub struct GlobalConfig {
    /// Log verbosity.
    #[serde(deserialize_with = "deserialize_level_from_string", skip_serializing)]
    pub log_verbosity: LevelFilter,
    /// Whether if a startup notification should be shown.
    pub startup_notification: bool,
    /// Geometry of the notification window.
    #[serde(deserialize_with = "deserialize_geometry_from_string")]
    pub geometry: Geometry,
    /// Window origin/anchor point (top-left, top-right, bottom-left, bottom-right).
    /// The geometry x,y become offsets from this origin.
    #[serde(default)]
    pub origin: Origin,
    /// Whether if the window will be resized to wrap the content.
    pub wrap_content: bool,
    /// Text font.
    pub font: String,
    /// Template for the notification message.
    pub template: String,
    /// Maximum number of notifications to display at once (ring buffer).
    /// When exceeded, oldest notifications are automatically dismissed.
    /// Set to 0 for unlimited.
    #[serde(default)]
    pub display_limit: usize,
    /// Minimum window width in pixels. If not set, window sizes to content.
    #[serde(default)]
    pub min_width: Option<u32>,
    /// Refresh interval in milliseconds for updating the age counter.
    /// Set to 0 to disable periodic refresh. Default is 1000 (1 second).
    #[serde(default = "default_refresh_interval")]
    pub refresh_interval_ms: u64,
}

fn default_refresh_interval() -> u64 {
    1000
}

/// Custom deserializer implementation for converting `String` to [`LevelFilter`]
fn deserialize_level_from_string<'de, D>(deserializer: D) -> StdResult<LevelFilter, D::Error>
where
    D: Deserializer<'de>,
{
    let value: String = Deserialize::deserialize(deserializer)?;
    LevelFilter::from_str(&value).map_err(SerdeError::custom)
}

/// Custom deserializer implementation for converting `String` to [`Geometry`]
fn deserialize_geometry_from_string<'de, D>(deserializer: D) -> StdResult<Geometry, D::Error>
where
    D: Deserializer<'de>,
{
    let value: String = Deserialize::deserialize(deserializer)?;
    Geometry::from_str(&value).map_err(SerdeError::custom)
}

/// Window geometry.
#[derive(Debug, Deserialize, Serialize)]
pub struct Geometry {
    /// Width of the window.
    pub width: u32,
    /// Height of the window.
    pub height: u32,
    /// X coordinate.
    pub x: u32,
    /// Y coordinate.
    pub y: u32,
}

impl FromStr for Geometry {
    type Err = Error;
    fn from_str(s: &str) -> StdResult<Self, Self::Err> {
        let (width, height, x, y) =
            scanf!(s, "{u32}x{u32}+{u32}+{u32}").map_err(|e| Error::Scanf(e.to_string()))?;
        Ok(Self {
            width,
            height,
            x,
            y,
        })
    }
}

/// Urgency configuration.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct UrgencyConfig {
    /// Background color.
    #[serde(
        deserialize_with = "deserialize_rgb_from_string",
        serialize_with = "serialize_rgb_to_string"
    )]
    pub background: Rgb,
    /// Foreground color.
    #[serde(
        deserialize_with = "deserialize_rgb_from_string",
        serialize_with = "serialize_rgb_to_string"
    )]
    pub foreground: Rgb,
    /// Timeout value.
    pub timeout: u32,
    /// Whether if auto timeout is enabled.
    pub auto_clear: Option<bool>,
    /// Text.
    pub text: Option<String>,
    /// Custom OS commands to run.
    pub custom_commands: Option<Vec<CustomCommand>>,
}

/// Custom deserializer implementation for converting `String` to [`Rgb`]
fn deserialize_rgb_from_string<'de, D>(deserializer: D) -> StdResult<Rgb, D::Error>
where
    D: Deserializer<'de>,
{
    let value: String = Deserialize::deserialize(deserializer)?;
    Rgb::from_hex_str(&value).map_err(SerdeError::custom)
}

/// Custom serializer implementation for converting [`Rgb`] to `String`
fn serialize_rgb_to_string<S>(value: &Rgb, s: S) -> StdResult<S::Ok, S::Error>
where
    S: Serializer,
{
    s.serialize_str(&value.to_hex_string())
}

impl UrgencyConfig {
    /// Runs the custom OS commands that are determined by configuration.
    pub fn run_commands(&self, notification: &Notification) -> Result<()> {
        if let Some(commands) = &self.custom_commands {
            for command in commands {
                if let Some(filter) = &command.filter
                    && !notification.matches_filter(filter)
                {
                    continue;
                }
                if (notification.timestamp
                    + notification.expire_timeout.unwrap_or_default().as_secs())
                    < SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs()
                {
                    continue;
                }
                log::trace!("running command: {:#?}", command);
                let command = Tera::one_off(
                    &command.command,
                    &notification.into_context(
                        self.text
                            .clone()
                            .unwrap_or_else(|| notification.urgency.to_string()),
                        0,
                    )?,
                    true,
                )?;
                Command::new("sh").args(["-c", &command]).spawn()?;
            }
        }
        Ok(())
    }
}

/// Custom OS commands along with notification filters.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CustomCommand {
    /// Notification message filter.
    #[serde(deserialize_with = "deserialize_filter_from_string", default)]
    filter: Option<NotificationFilter>,
    /// Command.
    command: String,
}

/// Custom deserializer implementation for converting `String` to [`NotificationFilter`]
fn deserialize_filter_from_string<'de, D>(
    deserializer: D,
) -> StdResult<Option<NotificationFilter>, D::Error>
where
    D: Deserializer<'de>,
{
    let value: Option<String> = Deserialize::deserialize(deserializer)?;
    match value {
        Some(v) => serde_json::from_str(&v).map_err(SerdeError::custom),
        None => Ok(None),
    }
}
