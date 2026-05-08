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

//! Help overlay for displaying keyboard shortcuts and navigation
//!
//! This module provides a help overlay that displays context-aware keyboard shortcuts.

use crate::{data::DataManager, layout::LayoutType};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

/// Help item representing a keyboard shortcut
#[derive(Debug, Clone)]
struct HelpItem {
    keys: &'static str,
    description: &'static str,
    layout_specific: Option<LayoutType>,
}

/// Help section grouping related shortcuts
#[derive(Debug, Clone)]
struct HelpSection {
    title: &'static str,
    items: Vec<HelpItem>,
}

/// Help overlay renderer
pub struct HelpOverlay {
    scroll_offset: usize,
    content_height: usize,
    viewport_height: usize,
}

impl HelpOverlay {
    /// Create a new help overlay
    pub fn new() -> Self {
        Self { scroll_offset: 0, content_height: 0, viewport_height: 0 }
    }

    /// Render the help overlay
    pub fn render(&mut self, frame: &mut Frame<'_>, layout_type: LayoutType, dm: &DataManager) {
        let area = frame.area();

        // Create a centered popup area (80% width, 90% height)
        let popup_area = centered_rect(80, 90, area);

        // Clear the background with a semi-transparent overlay effect
        frame.render_widget(Clear, popup_area);

        // Create the help content based on layout
        let help_content = self.generate_help_content(layout_type, dm);

        // Calculate content dimensions
        self.content_height = help_content.lines.len();
        self.viewport_height = popup_area.height.saturating_sub(2) as usize; // -2 for borders

        // Create the help block with borders
        let help_block = Block::default()
            .title(format!(
                " EDB Debugger Help - {} Layout ",
                match layout_type {
                    LayoutType::Full => "Full",
                    LayoutType::Compact => "Compact",
                    LayoutType::Mobile => "Mobile",
                }
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(dm.theme.focused_border))
            .title_alignment(Alignment::Center);

        // Create the paragraph with the help content
        let help_paragraph = Paragraph::new(help_content)
            .block(help_block)
            .style(Style::default().fg(dm.theme.help_text_color))
            .wrap(Wrap { trim: false })
            .scroll((self.scroll_offset as u16, 0));

        frame.render_widget(help_paragraph, popup_area);

        // Render scroll indicator if content is scrollable
        if self.content_height > self.viewport_height {
            self.render_scroll_indicator(frame, popup_area, dm);
        }
    }

    /// Generate help content based on the current layout
    fn generate_help_content(&self, layout_type: LayoutType, dm: &DataManager) -> Text<'static> {
        let mut lines = Vec::new();

        // Add sections based on layout
        let sections = self.get_help_sections(layout_type);

        for (i, section) in sections.iter().enumerate() {
            if i > 0 {
                lines.push(Line::from("")); // Empty line between sections
            }

            // Section title
            lines.push(Line::from(vec![Span::styled(
                section.title,
                Style::default().fg(dm.theme.warning_color).add_modifier(Modifier::BOLD),
            )]));

            // Section separator
            lines.push(Line::from(vec![Span::styled(
                "─".repeat(section.title.len()),
                Style::default().fg(dm.theme.unfocused_border),
            )]));

            // Section items
            for item in &section.items {
                // Skip items that are layout-specific and don't match current layout
                if let Some(specific_layout) = item.layout_specific
                    && specific_layout != layout_type
                {
                    continue;
                }

                lines.push(Line::from(vec![
                    Span::styled(
                        format!("  {:20} ", item.keys),
                        Style::default().fg(dm.theme.accent_color).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(item.description, Style::default().fg(dm.theme.help_text_color)),
                ]));
            }
        }

        // Add footer with instructions
        lines.push(Line::from(""));
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("Press ", Style::default().fg(dm.theme.help_text_color)),
            Span::styled(
                "?",
                Style::default().fg(dm.theme.success_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" or ", Style::default().fg(dm.theme.help_text_color)),
            Span::styled(
                "ESC",
                Style::default().fg(dm.theme.success_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" to close this help", Style::default().fg(dm.theme.help_text_color)),
        ]));

        Text::from(lines)
    }

    /// Get help sections based on layout type
    fn get_help_sections(&self, layout_type: LayoutType) -> Vec<HelpSection> {
        let mut sections = vec![];

        // Navigation & Focus section
        sections.push(HelpSection {
            title: "Navigation & Focus",
            items: vec![
                HelpItem {
                    keys: "?",
                    description: "Show/hide this help screen",
                    layout_specific: None,
                },
                HelpItem {
                    keys: "Tab/Shift+Tab",
                    description: "Cycle through visible panels",
                    layout_specific: None,
                },
                HelpItem {
                    keys: "F1-F4",
                    description: "Jump to panel (Trace/Code/Display/Terminal)",
                    layout_specific: None,
                },
                HelpItem {
                    keys: "\\",
                    description: "Toggle mouse mode (click to focus, scroll to navigate)",
                    layout_specific: None,
                },
                HelpItem {
                    keys: "ESC",
                    description: "Return to Terminal panel",
                    layout_specific: None,
                },
            ],
        });

        // Panel Resizing section (not for mobile)
        if layout_type != LayoutType::Mobile {
            sections.push(HelpSection {
                title: "Panel Resizing",
                items: vec![
                    HelpItem {
                        keys: "Ctrl+Shift+←/→",
                        description: "Adjust vertical split",
                        layout_specific: None,
                    },
                    HelpItem {
                        keys: "Ctrl+Shift+↑/↓",
                        description: "Adjust horizontal split",
                        layout_specific: None,
                    },
                ],
            });
        }

        // Code Panel section
        if layout_type != LayoutType::Mobile {
            // Always show for reference
            sections.push(HelpSection {
                title: "Code Panel",
                items: vec![
                    HelpItem {
                        keys: "Vim Navigation",
                        description: "j/k/h/l, gg/G, {/}, ↑/↓, ←/→, numeric prefixes",
                        layout_specific: None,
                    },
                    HelpItem {
                        keys: "f/F",
                        description: "Toggle file selector",
                        layout_specific: None,
                    },
                    HelpItem {
                        keys: "b/B",
                        description: "Toggle breakpoint at cursor",
                        layout_specific: None,
                    },
                    HelpItem {
                        keys: "n/N",
                        description: "Next/Previous step",
                        layout_specific: None,
                    },
                    HelpItem {
                        keys: "s/S",
                        description: "Step forward/backward",
                        layout_specific: None,
                    },
                    HelpItem {
                        keys: "c/C",
                        description: "Next/Previous call",
                        layout_specific: None,
                    },
                    HelpItem {
                        keys: "r/R",
                        description: "Run forward/backward until next breakpoint",
                        layout_specific: None,
                    },
                    HelpItem {
                        keys: "Space",
                        description: "Switch to Trace panel",
                        layout_specific: Some(LayoutType::Full),
                    },
                    HelpItem {
                        keys: "Space",
                        description: "Cycle: Trace → Code → Display",
                        layout_specific: Some(LayoutType::Compact),
                    },
                    HelpItem {
                        keys: ":",
                        description: "Enter Vim command mode",
                        layout_specific: None,
                    },
                ],
            });
        }

        // Trace Panel section
        sections.push(HelpSection {
            title: "Trace Panel",
            items: vec![
                HelpItem {
                    keys: "Vim Navigation",
                    description: "j/k/h/l, gg/G, {/}, ↑/↓, ←/→, numeric prefixes",
                    layout_specific: None,
                },
                HelpItem { keys: "c/C", description: "Goto code", layout_specific: None },
                HelpItem { keys: "v/V", description: "View code", layout_specific: None },
                HelpItem {
                    keys: "Enter",
                    description: "Toggle expand/collapse",
                    layout_specific: None,
                },
                HelpItem {
                    keys: "Space",
                    description: "Switch to Code panel",
                    layout_specific: Some(LayoutType::Full),
                },
                HelpItem {
                    keys: "Space",
                    description: "Cycle: Trace → Code → Display",
                    layout_specific: Some(LayoutType::Compact),
                },
            ],
        });

        // Display Panel section
        sections.push(HelpSection {
            title: "Display Panel",
            items: vec![
                HelpItem { keys: "↑/↓", description: "Navigate items", layout_specific: None },
                HelpItem {
                    keys: "←/→", description: "Horizontal scroll", layout_specific: None
                },
                HelpItem {
                    keys: "s/S",
                    description: "Cycle display modes forward/backward",
                    layout_specific: None,
                },
                HelpItem {
                    keys: "PageUp/PageDown",
                    description: "Fast navigation (5 items)",
                    layout_specific: None,
                },
                HelpItem {
                    keys: "Enter",
                    description: "Toggle multi-line view (Variables/Expressions)",
                    layout_specific: None,
                },
                HelpItem {
                    keys: "Space",
                    description: "Cycle: Trace → Code → Display",
                    layout_specific: Some(LayoutType::Compact),
                },
            ],
        });

        // Terminal Panel section
        sections.push(HelpSection {
            title: "Terminal Panel",
            items: vec![
                HelpItem {
                    keys: "ESC",
                    description: "Enter Vim mode (from Insert mode)",
                    layout_specific: None,
                },
                HelpItem {
                    keys: "↑/↓",
                    description: "Command history (Insert mode)",
                    layout_specific: None,
                },
                HelpItem {
                    keys: "i",
                    description: "Enter Insert mode (from Vim mode)",
                    layout_specific: None,
                },
                HelpItem {
                    keys: "Vim Navigation",
                    description: "j/k/h/l, gg/G, {/}, ↑/↓, ←/→, numeric prefixes (Vim mode)",
                    layout_specific: None,
                },
                HelpItem { keys: "Ctrl+L", description: "Clear terminal", layout_specific: None },
                HelpItem {
                    keys: "Ctrl+C (2x)",
                    description: "First clears input, second exits",
                    layout_specific: None,
                },
            ],
        });

        // Exit Commands section
        sections.push(HelpSection {
            title: "Exit Commands",
            items: vec![
                HelpItem { keys: "Ctrl+Q", description: "Quit application", layout_specific: None },
                HelpItem {
                    keys: "Ctrl+D",
                    description: "Exit (EOF signal)",
                    layout_specific: None,
                },
                HelpItem { keys: "Alt+Q", description: "Quick exit", layout_specific: None },
            ],
        });

        sections
    }

    /// Render scroll indicator
    fn render_scroll_indicator(&self, frame: &mut Frame<'_>, area: Rect, dm: &DataManager) {
        let scroll_percentage = if self.content_height > 0 {
            // self.content_height is guaranteed to be larger than self.viewport_height
            (self.scroll_offset as f32
                / self.content_height.saturating_sub(self.viewport_height) as f32
                * 100.0) as u16
        } else {
            0
        };

        let more_content_below = self.scroll_offset + self.viewport_height < self.content_height;
        let more_content_above = self.scroll_offset > 0;

        // Create scroll indicator text
        let mut indicator_parts = vec![];

        if more_content_above {
            indicator_parts.push(Span::styled("↑ ", Style::default().fg(dm.theme.warning_color)));
        }

        indicator_parts.push(Span::styled(
            format!("{scroll_percentage}%"),
            Style::default().fg(dm.theme.accent_color),
        ));

        if more_content_below {
            indicator_parts.push(Span::styled(" ↓", Style::default().fg(dm.theme.warning_color)));
        }

        if more_content_above || more_content_below {
            indicator_parts.push(Span::styled(
                " (j/k to scroll)",
                Style::default().fg(dm.theme.help_text_color),
            ));
        }

        let indicator_line = Line::from(indicator_parts);
        let indicator_line_length = indicator_line.width() as u16;

        // Position the indicator at the bottom-right corner of the help box
        let indicator_area = Rect {
            x: area.x + area.width - indicator_line_length - 1,
            y: area.y + area.height - 1,
            width: indicator_line_length,
            height: 1,
        };

        frame.render_widget(
            Paragraph::new(indicator_line).alignment(Alignment::Right),
            indicator_area,
        );
    }

    /// Scroll up
    pub fn scroll_up(&mut self, amount: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(amount);
    }

    /// Scroll down
    pub fn scroll_down(&mut self, amount: usize) {
        let max_scroll = self.content_height.saturating_sub(self.viewport_height);
        self.scroll_offset = (self.scroll_offset + amount).min(max_scroll);
    }

    /// Reset scroll position
    pub fn reset_scroll(&mut self) {
        self.scroll_offset = 0;
    }
}

/// Helper function to create a centered rect
fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
