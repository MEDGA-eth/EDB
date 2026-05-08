// EDB - Ethereum Debugger
// Copyright (C) 2024 Zhuo Zhang and Wuqi Zhang
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

//! Configuration system for EDB TUI
//!
//! Manages user preferences including color schemes and other settings.

use eyre::{Context, Result};
use ratatui::style::Color;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};
use tracing::{debug, info, warn};

use crate::Theme;

/// Main configuration structure
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    /// Current theme configuration
    pub theme: ThemeConfig,
    /// Panel-specific settings
    pub panels: PanelConfig,
}

/// Theme configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeConfig {
    /// Current active theme name
    pub active: String,
    /// Available themes
    pub themes: std::collections::HashMap<String, Theme>,
}

/// Panel-specific configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelConfig {
    /// Terminal panel settings
    pub terminal: TerminalPanelConfig,
    /// Code panel settings
    pub code: CodePanelConfig,
    /// Trace panel settings
    pub trace: TracePanelConfig,
    /// Display panel settings
    pub display: DisplayPanelConfig,
}

/// Terminal panel configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalPanelConfig {
    /// Maximum number of history lines to keep
    pub max_history: usize,
    /// Show timestamps in output
    pub show_timestamps: bool,
}

/// Code panel configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodePanelConfig {
    /// Show line numbers
    pub show_line_numbers: bool,
    /// Highlight current line
    pub highlight_current_line: bool,
}

/// Trace panel configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TracePanelConfig {
    /// Show trace depth indicators
    pub show_depth_indicators: bool,
    /// Maximum trace entries to display
    pub max_entries: usize,
}

/// Display panel configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplayPanelConfig {
    /// Default display mode on startup
    pub default_mode: String,
    /// Show variable types
    pub show_types: bool,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        let themes = Theme::all().iter().map(|theme| (theme.name().to_string(), *theme)).collect();

        Self { active: Theme::default().name().to_string(), themes }
    }
}

impl Default for PanelConfig {
    fn default() -> Self {
        Self {
            terminal: TerminalPanelConfig { max_history: 1000, show_timestamps: false },
            code: CodePanelConfig { show_line_numbers: true, highlight_current_line: true },
            trace: TracePanelConfig { show_depth_indicators: true, max_entries: 500 },
            display: DisplayPanelConfig { default_mode: "Variables".to_string(), show_types: true },
        }
    }
}

impl Config {
    /// Get the config file path (~/.edb.toml)
    pub fn config_path() -> Result<PathBuf> {
        let home =
            dirs::home_dir().ok_or_else(|| eyre::eyre!("Unable to determine home directory"))?;
        Ok(home.join(".edb.toml"))
    }

    /// Load configuration from file, creating default if it doesn't exist
    pub fn load() -> Result<Self> {
        let config_path = Self::config_path()?;

        if !config_path.exists() {
            info!("Config file not found, creating default at {:?}", config_path);
            let default_config = Self::default();
            default_config.save()?;
            return Ok(default_config);
        }

        let content = fs::read_to_string(&config_path)
            .with_context(|| format!("Failed to read config file: {config_path:?}"))?;

        let config: Self =
            toml::from_str(&content).with_context(|| "Failed to parse config file as TOML")?;

        debug!("Loaded configuration from {:?}", config_path);
        Ok(config)
    }

    /// Load configuration from a specific path
    pub fn load_from_path(path: PathBuf) -> Result<Self> {
        if !path.exists() {
            return Err(eyre::eyre!("Config file not found at {:?}", path));
        }

        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config file: {path:?}"))?;

        let config: Self =
            toml::from_str(&content).with_context(|| "Failed to parse config file as TOML")?;

        debug!("Loaded configuration from {:?}", path);
        Ok(config)
    }

    /// Save configuration to file
    pub fn save(&self) -> Result<()> {
        let config_path = Self::config_path()?;

        let content =
            toml::to_string_pretty(self).with_context(|| "Failed to serialize config to TOML")?;

        fs::write(&config_path, content)
            .with_context(|| format!("Failed to write config file: {config_path:?}"))?;

        debug!("Saved configuration to {:?}", config_path);
        Ok(())
    }

    /// Get the currently active theme
    pub fn get_active_theme(&self) -> Option<&Theme> {
        self.theme.themes.get(&self.theme.active)
    }

    /// Switch to a different theme
    pub fn set_theme(&mut self, theme_name: &str) -> Result<()> {
        if !self.theme.themes.contains_key(theme_name) {
            return Err(eyre::eyre!("Theme '{}' not found", theme_name));
        }

        self.theme.active = theme_name.to_string();
        info!("Switched to theme: {}", theme_name);
        Ok(())
    }

    /// List available themes
    pub fn list_themes(&self) -> Vec<(&String, &Theme)> {
        self.theme.themes.iter().collect()
    }

    /// Convert color string to ratatui Color
    pub fn parse_color(color_str: &str) -> Color {
        match color_str.to_lowercase().as_str() {
            "black" => Color::Black,
            "red" => Color::Red,
            "green" => Color::Green,
            "yellow" => Color::Yellow,
            "blue" => Color::Blue,
            "magenta" => Color::Magenta,
            "cyan" => Color::Cyan,
            "gray" => Color::Gray,
            "dark_gray" => Color::DarkGray,
            "light_red" => Color::LightRed,
            "light_green" => Color::LightGreen,
            "light_yellow" => Color::LightYellow,
            "light_blue" => Color::LightBlue,
            "light_magenta" => Color::LightMagenta,
            "light_cyan" => Color::LightCyan,
            "white" => Color::White,
            "light_gray" => Color::Gray,
            _ => {
                warn!("Unknown color '{}', using default gray", color_str);
                Color::Gray
            }
        }
    }
}
