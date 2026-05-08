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

//! Simplified theme structure for TUI
//!
//! This module provides a direct theme configuration without Arc/RwLock wrapping,
//! as themes don't require async operations or RPC connections.

use std::ops::Deref;

use crate::{
    ColorScheme,
    config::{Config, PanelConfig},
};
use eyre::Result;
use tracing::{debug, info};

/// Direct theme configuration without unnecessary wrapping
#[derive(Debug, Clone)]
pub struct Theme {
    /// Current color scheme for rendering
    pub color_scheme: ColorScheme,
    /// Panel-specific configurations
    pub panel_configs: PanelConfig,
    /// Active theme name
    pub active_theme: String,
    /// Available themes
    _available_themes: Vec<String>,
    /// Configuration storage
    config: Config,
}

impl Deref for Theme {
    type Target = ColorScheme;

    fn deref(&self) -> &Self::Target {
        &self.color_scheme
    }
}

impl Default for Theme {
    fn default() -> Self {
        let config = Config::load().unwrap_or_default();
        let color_scheme = if let Some(theme) = config.get_active_theme() {
            (*theme).into()
        } else {
            ColorScheme::default()
        };

        Self {
            color_scheme,
            panel_configs: config.panels.clone(),
            active_theme: config.theme.active.clone(),
            _available_themes: config
                .list_themes()
                .into_iter()
                .map(|(name, _)| name.clone())
                .collect(),
            config,
        }
    }
}

impl Theme {
    /// Create a new theme from configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Set a new color scheme
    pub fn set_color_scheme(&mut self, scheme: ColorScheme) {
        self.color_scheme = scheme;
        debug!("Color scheme updated");
    }

    /// Switch to a different theme by name
    pub fn switch_theme(&mut self, theme_name: &str) -> Result<()> {
        self.config.set_theme(theme_name)?;
        self.config.save()?;

        // Update current state
        if let Some(theme) = self.config.get_active_theme() {
            self.color_scheme = (*theme).into();
            self.active_theme = theme_name.to_string();
            info!("Theme switched to: {}", theme_name);
        }

        Ok(())
    }

    /// Get list of available themes
    pub fn list_themes(&self) -> Vec<(String, String, String)> {
        self.config
            .list_themes()
            .into_iter()
            .map(|(name, theme)| {
                (name.clone(), theme.display_name().to_string(), theme.description().to_string())
            })
            .collect()
    }

    /// Get current theme name
    pub fn get_active_theme_name(&self) -> &str {
        &self.active_theme
    }

    /// Get panel configuration
    pub fn get_panel_config(&self) -> &PanelConfig {
        &self.panel_configs
    }

    /// Reload theme configuration from disk
    pub fn reload(&mut self) -> Result<()> {
        let new_config = Config::load()?;
        self.config = new_config.clone();
        self.panel_configs = new_config.panels.clone();

        if let Some(theme) = new_config.get_active_theme() {
            self.color_scheme = (*theme).into();
            self.active_theme = new_config.theme.active.clone();
        }

        debug!("Theme configuration reloaded");
        Ok(())
    }
}
