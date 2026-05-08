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

//! Enhanced border system with Unicode box-drawing characters
//!
//! Provides beautiful rounded borders and dynamic highlighting for focused panels

use ratatui::{
    style::{Color, Style},
    widgets::{Block, BorderType, Borders},
};

/// Enhanced border styles for panels
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnhancedBorderStyle {
    /// Standard rounded corners with elegant Unicode characters
    Rounded,
    /// Double-line borders for emphasis
    Double,
    /// Thick borders for high priority panels
    Thick,
    /// Classic square borders (default ratatui style)
    Square,
}

/// Enhanced border builder for panels
pub struct EnhancedBorder {
    style: EnhancedBorderStyle,
    focused: bool,
    title: Option<String>,
    focused_color: Color,
    unfocused_color: Color,
}

impl EnhancedBorder {
    /// Create a new enhanced border
    pub fn new(style: EnhancedBorderStyle) -> Self {
        Self {
            style,
            focused: false,
            title: None,
            focused_color: Color::Cyan,
            unfocused_color: Color::Gray,
        }
    }

    /// Set focus state
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// Set border title
    pub fn title<S: Into<String>>(mut self, title: S) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Set focused border color
    pub fn focused_color(mut self, color: Color) -> Self {
        self.focused_color = color;
        self
    }

    /// Set unfocused border color
    pub fn unfocused_color(mut self, color: Color) -> Self {
        self.unfocused_color = color;
        self
    }

    /// Build the Block widget with enhanced styling
    pub fn build(self) -> Block<'static> {
        let border_color = if self.focused { self.focused_color } else { self.unfocused_color };

        let border_type = match self.style {
            EnhancedBorderStyle::Rounded => BorderType::Rounded,
            EnhancedBorderStyle::Double => BorderType::Double,
            EnhancedBorderStyle::Thick => BorderType::Thick,
            EnhancedBorderStyle::Square => BorderType::Plain,
        };

        let mut block = Block::default()
            .borders(Borders::ALL)
            .border_type(border_type)
            .border_style(self.get_border_style(border_color));

        if let Some(title) = self.title {
            // Add special indicators for focused panels
            let title_with_indicator = if self.focused {
                match self.style {
                    EnhancedBorderStyle::Rounded => format!("╭─ {title} ─╮"),
                    EnhancedBorderStyle::Double => format!("╔═ {title} ═╗"),
                    EnhancedBorderStyle::Thick => format!("┏━ {title} ━┓"),
                    EnhancedBorderStyle::Square => format!("┌─ {title} ─┐"),
                }
            } else {
                title
            };
            block = block.title(title_with_indicator);
        }

        block
    }

    /// Get enhanced border style with potential animation effects
    fn get_border_style(&self, base_color: Color) -> Style {
        if self.focused {
            // Enhanced styling for focused panels
            Style::default().fg(base_color)
        } else {
            Style::default().fg(base_color)
        }
    }
}

/// Convenience functions for common border styles
impl EnhancedBorder {
    /// Create a rounded border (most common style)
    pub fn rounded() -> Self {
        Self::new(EnhancedBorderStyle::Rounded)
    }

    /// Create a double-line border for emphasis
    pub fn double() -> Self {
        Self::new(EnhancedBorderStyle::Double)
    }

    /// Create a thick border for high priority
    pub fn thick() -> Self {
        Self::new(EnhancedBorderStyle::Thick)
    }

    /// Create a square border (classic style)
    pub fn square() -> Self {
        Self::new(EnhancedBorderStyle::Square)
    }
}

/// Enhanced border presets for different panel types
pub struct BorderPresets;

impl BorderPresets {
    /// Terminal panel border - rounded with system styling
    pub fn terminal(
        focused: bool,
        title: String,
        focused_color: Color,
        unfocused_color: Color,
    ) -> Block<'static> {
        EnhancedBorder::rounded()
            .focused(focused)
            .title(title)
            .focused_color(focused_color)
            .unfocused_color(unfocused_color)
            .build()
    }

    /// Code panel border - double-line for emphasis
    pub fn code(
        focused: bool,
        title: String,
        focused_color: Color,
        unfocused_color: Color,
    ) -> Block<'static> {
        EnhancedBorder::double()
            .focused(focused)
            .title(title)
            .focused_color(focused_color)
            .unfocused_color(unfocused_color)
            .build()
    }

    /// Trace panel border - thick for importance
    pub fn trace(
        focused: bool,
        title: String,
        focused_color: Color,
        unfocused_color: Color,
    ) -> Block<'static> {
        EnhancedBorder::thick()
            .focused(focused)
            .title(title)
            .focused_color(focused_color)
            .unfocused_color(unfocused_color)
            .build()
    }

    /// Display panel border - rounded standard
    pub fn display(
        focused: bool,
        title: String,
        focused_color: Color,
        unfocused_color: Color,
    ) -> Block<'static> {
        EnhancedBorder::rounded()
            .focused(focused)
            .title(title)
            .focused_color(focused_color)
            .unfocused_color(unfocused_color)
            .build()
    }
}
