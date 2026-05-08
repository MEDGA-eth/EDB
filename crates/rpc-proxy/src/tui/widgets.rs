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

//! Custom widgets for the TUI interface

use super::app::App;
use ratatui::{prelude::*, widgets::*};
use std::time::{SystemTime, UNIX_EPOCH};

impl App {
    pub fn render_overview(&mut self, f: &mut Frame<'_>, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(7),  // Status cards
                Constraint::Length(10), // Metrics table with trends
                Constraint::Min(8),     // Charts
            ])
            .split(area);

        // Render status cards
        self.render_status_cards(f, chunks[0]);

        // Render metrics table with trends
        self.render_metrics_table(f, chunks[1]);

        // Render simplified charts
        self.render_charts(f, chunks[2]);
    }

    fn render_status_cards(&self, f: &mut Frame<'_>, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
            ])
            .split(area);

        // Providers status card
        let healthy_providers = self.providers.iter().filter(|p| p.is_healthy).count();
        let total_providers = self.providers.len();
        let provider_status = if healthy_providers == total_providers && total_providers > 0 {
            ("🟢 Healthy", Color::Green)
        } else if healthy_providers > 0 {
            ("🟡 Degraded", Color::Yellow)
        } else {
            ("🔴 Down", Color::Red)
        };

        let provider_card = Paragraph::new(vec![
            Line::from(vec![Span::styled(
                "Providers",
                Style::default().add_modifier(Modifier::BOLD),
            )]),
            Line::from(""),
            Line::from(vec![Span::styled(
                provider_status.0,
                Style::default().fg(provider_status.1),
            )]),
            Line::from(format!("{healthy_providers}/{total_providers} Online")),
        ])
        .block(Block::default().borders(Borders::ALL).title("RPC Providers"))
        .alignment(Alignment::Center);

        f.render_widget(provider_card, chunks[0]);

        // Enhanced Cache status card with hit rate
        let cache_stats = self.cache_stats.as_ref();
        let cache_utilization =
            cache_stats.map(|s| s.utilization.clone()).unwrap_or_else(|| "0%".to_string());
        let cache_entries = cache_stats.map(|s| s.total_entries).unwrap_or(0);

        // Extract cache hit rate from enhanced metrics - it comes as a string like "75.5%"
        let hit_rate = self
            .cache_metrics
            .as_ref()
            .and_then(|m| m.get("hit_rate").and_then(|v| v.as_str()))
            .map(|s| s.to_string())
            .unwrap_or_else(|| "N/A".to_string());

        let cache_color =
            if cache_utilization.trim_end_matches('%').parse::<f32>().unwrap_or(0.0) > 90.0 {
                Color::Red
            } else if cache_utilization.trim_end_matches('%').parse::<f32>().unwrap_or(0.0) > 75.0 {
                Color::Yellow
            } else {
                Color::Green
            };

        let cache_card = Paragraph::new(vec![
            Line::from(vec![Span::styled("Cache", Style::default().add_modifier(Modifier::BOLD))]),
            Line::from(""),
            Line::from(vec![Span::styled(&cache_utilization, Style::default().fg(cache_color))]),
            Line::from(format!("Hit Rate: {hit_rate}")),
            Line::from(format!("{cache_entries} entries")),
        ])
        .block(Block::default().borders(Borders::ALL).title("Cache Status"))
        .alignment(Alignment::Center);

        f.render_widget(cache_card, chunks[1]);

        // EDB instances card
        let instance_count = self.active_instances.len();
        let instance_status = if instance_count > 0 {
            ("🟢 Active", Color::Green)
        } else {
            ("⚪ Idle", Color::Gray)
        };

        let instance_card = Paragraph::new(vec![
            Line::from(vec![Span::styled(
                "EDB Instances",
                Style::default().add_modifier(Modifier::BOLD),
            )]),
            Line::from(""),
            Line::from(vec![Span::styled(
                instance_status.0,
                Style::default().fg(instance_status.1),
            )]),
            Line::from(format!("{instance_count} connected")),
        ])
        .block(Block::default().borders(Borders::ALL).title("Debugging Sessions"))
        .alignment(Alignment::Center);

        f.render_widget(instance_card, chunks[2]);

        // Performance card
        let avg_response_time =
            self.providers.iter().filter_map(|p| p.response_time_ms).sum::<u64>() as f64
                / self.providers.iter().filter(|p| p.response_time_ms.is_some()).count().max(1)
                    as f64;

        let perf_color = if avg_response_time > 2000.0 {
            Color::Red
        } else if avg_response_time > 1000.0 {
            Color::Yellow
        } else {
            Color::Green
        };

        let perf_card = Paragraph::new(vec![
            Line::from(vec![Span::styled(
                "Performance",
                Style::default().add_modifier(Modifier::BOLD),
            )]),
            Line::from(""),
            Line::from(vec![Span::styled(
                format!("{avg_response_time:.0}ms"),
                Style::default().fg(perf_color),
            )]),
            Line::from("Avg Response"),
        ])
        .block(Block::default().borders(Borders::ALL).title("Response Time"))
        .alignment(Alignment::Center);

        f.render_widget(perf_card, chunks[3]);
    }

    fn render_charts(&self, f: &mut Frame<'_>, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);

        // Cache hit rate percentage chart (simplified)
        self.render_cache_hit_rate_chart(f, chunks[0]);

        // Response time trend chart
        self.render_response_time_chart(f, chunks[1]);
    }

    fn render_metrics_table(&self, f: &mut Frame<'_>, area: Rect) {
        // Calculate trends from historical data
        let (hit_rate_trend, rpm_trend, response_trend, cache_size_trend) = self.calculate_trends();

        // Current metrics - get hit rate directly from backend
        let hit_rate = self
            .cache_metrics
            .as_ref()
            .and_then(|m| m.get("hit_rate").and_then(|v| v.as_str()))
            .and_then(|s| s.trim_end_matches('%').parse::<f64>().ok())
            .unwrap_or(0.0);

        let rpm = self
            .request_metrics
            .as_ref()
            .and_then(|m| m.get("requests_per_minute").and_then(|v| v.as_u64()))
            .unwrap_or(0);

        let avg_response =
            self.metrics_history.back().map(|m| m.avg_response_time_ms).unwrap_or(0.0);

        let cache_size = self.metrics_history.back().map(|m| m.cache_size).unwrap_or(0);

        // Create table rows with trend indicators
        let rows = vec![
            Row::new(vec![
                Cell::from("Cache Hit Rate"),
                Cell::from(format!("{hit_rate:.1}%"))
                    .style(self.get_metric_color(hit_rate, 80.0, 50.0)),
                Cell::from(self.format_trend(hit_rate_trend, true))
                    .style(self.get_trend_color(hit_rate_trend)),
                Cell::from(self.create_mini_sparkline(&self.get_hit_rate_history(), 40)),
            ]),
            Row::new(vec![
                Cell::from("Cache Size"),
                Cell::from(format!("{cache_size}")),
                Cell::from(self.format_trend(cache_size_trend, false))
                    .style(self.get_trend_color(cache_size_trend)),
                Cell::from(self.create_mini_sparkline(&self.get_cache_size_history(), 40)),
            ]),
            Row::new(vec![
                Cell::from("Requests/Min"),
                Cell::from(format!("{rpm}")),
                Cell::from(self.format_trend(rpm_trend, false))
                    .style(self.get_trend_color(rpm_trend)),
                Cell::from(self.create_mini_sparkline(&self.get_rpm_history(), 40)),
            ]),
            Row::new(vec![
                Cell::from("Avg Response"),
                Cell::from(format!("{avg_response:.0}ms"))
                    .style(self.get_response_color(avg_response)),
                Cell::from(self.format_trend(response_trend, false))
                    .style(self.get_trend_color(-response_trend)), // Invert for response time
                Cell::from(self.create_mini_sparkline(&self.get_response_history(), 40)),
            ]),
            Row::new(vec![
                Cell::from("Active Instances"),
                Cell::from(format!("{}", self.active_instances.len())),
                Cell::from(""),
                Cell::from(""),
            ]),
        ];

        let table = Table::new(
            rows,
            vec![
                Constraint::Length(15),
                Constraint::Length(12),
                Constraint::Length(10),
                Constraint::Min(20),
            ],
        )
        .header(
            Row::new(vec!["Metric", "Current", "Trend", "History"])
                .style(Style::default().add_modifier(Modifier::BOLD)),
        )
        .block(Block::default().borders(Borders::ALL).title("Real-time Metrics"))
        .row_highlight_style(Style::default().add_modifier(Modifier::BOLD));

        f.render_widget(table, area);
    }

    fn calculate_trends(&self) -> (f64, f64, f64, f64) {
        if self.metrics_history.len() < 2 {
            return (0.0, 0.0, 0.0, 0.0);
        }

        let recent = self.metrics_history.back().unwrap();
        let previous = self
            .metrics_history
            .get(self.metrics_history.len().saturating_sub(10))
            .unwrap_or(self.metrics_history.front().unwrap());

        // Use the hit_rate directly from the stored data
        let recent_hit_rate = recent.hit_rate;
        let previous_hit_rate = previous.hit_rate;

        let hit_rate_trend = recent_hit_rate - previous_hit_rate;
        let rpm_trend = recent.requests_per_minute as f64 - previous.requests_per_minute as f64;
        let response_trend = recent.avg_response_time_ms - previous.avg_response_time_ms;
        let cache_size_trend = recent.cache_size as f64 - previous.cache_size as f64;

        (hit_rate_trend, rpm_trend, response_trend, cache_size_trend)
    }

    fn format_trend(&self, trend: f64, is_percentage: bool) -> String {
        if trend.abs() < 0.1 {
            "→".to_string()
        } else if trend > 0.0 {
            if is_percentage {
                format!("↑ {:.1}%", trend.abs())
            } else {
                format!("↑ {:.0}", trend.abs())
            }
        } else if is_percentage {
            format!("↓ {:.1}%", trend.abs())
        } else {
            format!("↓ {:.0}", trend.abs())
        }
    }

    fn get_trend_color(&self, trend: f64) -> Style {
        if trend > 0.0 {
            Style::default().fg(Color::Green)
        } else if trend < 0.0 {
            Style::default().fg(Color::Red)
        } else {
            Style::default().fg(Color::Gray)
        }
    }

    fn get_metric_color(&self, value: f64, good_threshold: f64, bad_threshold: f64) -> Style {
        if value >= good_threshold {
            Style::default().fg(Color::Green)
        } else if value >= bad_threshold {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::Red)
        }
    }

    fn get_response_color(&self, ms: f64) -> Style {
        if ms < 100.0 {
            Style::default().fg(Color::Green)
        } else if ms < 500.0 {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::Red)
        }
    }

    fn create_mini_sparkline(&self, data: &[f64], width: usize) -> String {
        if data.is_empty() {
            return "—".repeat(width);
        }

        let chars = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
        let min = data.iter().fold(f64::INFINITY, |a, &b| a.min(b));
        let max = data.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
        let range = max - min;

        // Take last `width` points
        let start = data.len().saturating_sub(width);
        data[start..]
            .iter()
            .map(|&v| {
                if range.abs() < f64::EPSILON {
                    chars[(chars.len() - 1) / 2]
                } else {
                    let index = ((v - min) / range * (chars.len() - 1) as f64) as usize;
                    chars[index.min(chars.len() - 1)]
                }
            })
            .collect()
    }

    fn get_hit_rate_history(&self) -> Vec<f64> {
        self.metrics_history.iter().map(|m| m.hit_rate).collect()
    }

    fn get_rpm_history(&self) -> Vec<f64> {
        self.metrics_history.iter().map(|m| m.requests_per_minute as f64).collect()
    }

    fn get_response_history(&self) -> Vec<f64> {
        self.metrics_history.iter().map(|m| m.avg_response_time_ms).collect()
    }

    fn get_cache_size_history(&self) -> Vec<f64> {
        self.metrics_history.iter().map(|m| m.cache_size as f64).collect()
    }

    fn render_cache_hit_rate_chart(&self, f: &mut Frame<'_>, area: Rect) {
        if self.metrics_history.is_empty() {
            let empty = Paragraph::new("No data available")
                .block(Block::default().borders(Borders::ALL).title("Cache Hit Rate"))
                .alignment(Alignment::Center);
            f.render_widget(empty, area);
            return;
        }

        // Use hit rate directly from stored data
        let data: Vec<(f64, f64)> = self
            .metrics_history
            .iter()
            .enumerate()
            .map(|(i, metric)| (i as f64, metric.hit_rate))
            .collect();

        // Create time labels for X-axis
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
        let x_labels = if self.metrics_history.len() > 1 {
            let oldest_time = self.metrics_history.front().map(|m| m.timestamp).unwrap_or(now);
            let time_range = now - oldest_time;

            if time_range < 300 {
                // Less than 5 minutes
                vec![
                    format!("{}s ago", time_range),
                    format!("{}s ago", time_range / 2),
                    "now".to_string(),
                ]
            } else {
                vec![
                    format!("{}m ago", time_range / 60),
                    format!("{}m ago", time_range / 120),
                    "now".to_string(),
                ]
            }
        } else {
            vec![String::new()]
        };

        let datasets = vec![
            Dataset::default()
                .name("Hit Rate %")
                .marker(symbols::Marker::Braille)
                .style(Style::default().fg(Color::Green))
                .data(&data),
        ];

        let chart = Chart::new(datasets)
            .block(Block::default().borders(Borders::ALL).title("Cache Hit Rate (%)"))
            .x_axis(
                Axis::default()
                    .title("Time")
                    .style(Style::default().fg(Color::Gray))
                    .bounds([0.0, self.metrics_history.len().max(1) as f64 - 1.0])
                    .labels(x_labels),
            )
            .y_axis(
                Axis::default()
                    .title("Hit Rate %")
                    .style(Style::default().fg(Color::Gray))
                    .bounds([0.0, 100.0])
                    .labels(vec!["0%", "50%", "100%"]),
            );

        f.render_widget(chart, area);
    }

    fn render_response_time_chart(&self, f: &mut Frame<'_>, area: Rect) {
        if self.metrics_history.is_empty() {
            let empty = Paragraph::new("No data available")
                .block(Block::default().borders(Borders::ALL).title("Response Time Trend"))
                .alignment(Alignment::Center);
            f.render_widget(empty, area);
            return;
        }

        // Get response time data
        let data: Vec<(f64, f64)> = self
            .metrics_history
            .iter()
            .enumerate()
            .map(|(i, metric)| (i as f64, metric.avg_response_time_ms))
            .collect();

        let max_response = data.iter().map(|(_, y)| *y).fold(0.0, f64::max).max(100.0);

        // Create time labels for X-axis (reuse logic from hit rate chart)
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
        let x_labels = if self.metrics_history.len() > 1 {
            let oldest_time = self.metrics_history.front().map(|m| m.timestamp).unwrap_or(now);
            let time_range = now - oldest_time;

            if time_range < 300 {
                // Less than 5 minutes
                vec![
                    format!("{}s ago", time_range),
                    format!("{}s ago", time_range / 2),
                    "now".to_string(),
                ]
            } else {
                vec![
                    format!("{}m ago", time_range / 60),
                    format!("{}m ago", time_range / 120),
                    "now".to_string(),
                ]
            }
        } else {
            vec![String::new()]
        };

        let datasets = vec![
            Dataset::default()
                .name("Avg Response Time")
                .marker(symbols::Marker::Braille)
                .style(Style::default().fg(Color::Cyan))
                .data(&data),
        ];

        let chart = Chart::new(datasets)
            .block(Block::default().borders(Borders::ALL).title("Response Time (ms)"))
            .x_axis(
                Axis::default()
                    .title("Time")
                    .style(Style::default().fg(Color::Gray))
                    .bounds([0.0, self.metrics_history.len().max(1) as f64 - 1.0])
                    .labels(x_labels),
            )
            .y_axis(
                Axis::default()
                    .title("Response (ms)")
                    .style(Style::default().fg(Color::Gray))
                    .bounds([0.0, max_response])
                    .labels(vec![
                        "0ms".to_string(),
                        format!("{}ms", (max_response / 2.0) as u64),
                        format!("{}ms", max_response as u64),
                    ]),
            );

        f.render_widget(chart, area);
    }

    pub fn render_providers(&mut self, f: &mut Frame<'_>, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(50), // Provider statistics table
                Constraint::Percentage(50), // Provider list and details
            ])
            .split(area);

        // Provider statistics table
        self.render_provider_statistics(f, chunks[0]);

        // Provider list and details
        let provider_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(chunks[1]);

        self.render_provider_list(f, provider_chunks[0]);
        self.render_provider_details(f, provider_chunks[1]);
    }

    fn render_provider_statistics(&self, f: &mut Frame<'_>, area: Rect) {
        // Extract provider metrics from enhanced data
        // The provider_metrics response has format: { "providers": [...] }
        let providers_data = self
            .provider_metrics
            .as_ref()
            .and_then(|m| m.get("providers"))
            .and_then(|p| p.as_array())
            .cloned()
            .unwrap_or_default();

        let mut provider_rows: Vec<Row<'_>> = Vec::new();

        // Add header-like summary row
        let total_healthy = self.providers.iter().filter(|p| p.is_healthy).count();
        let total_providers = self.providers.len();
        let mut total_requests = 0;
        let avg_response = self
            .providers
            .iter()
            .filter_map(|p| p.response_time_ms)
            .map(|ms| ms as f64)
            .sum::<f64>()
            / self.providers.iter().filter(|p| p.response_time_ms.is_some()).count().max(1) as f64;

        // Create rows for each provider with usage metrics
        for provider in &self.providers {
            let url_short = if provider.url.len() > 60 {
                format!("...{}", &provider.url[provider.url.len() - 57..])
            } else {
                provider.url.clone()
            };

            // Find matching provider in the providers data array
            let usage_data = providers_data
                .iter()
                .find(|p| p.get("url").and_then(|u| u.as_str()) == Some(&provider.url))
                .and_then(|p| p.as_object());

            let request_count = usage_data
                .and_then(|d| d.get("request_count").and_then(|v| v.as_u64()))
                .unwrap_or(0);
            total_requests += request_count;

            let success_rate = usage_data
                .and_then(|d| d.get("success_rate").and_then(|v| v.as_str()))
                .and_then(|s| s.trim_end_matches('%').parse::<f64>().ok())
                .unwrap_or(0.0);

            let load_percent = usage_data
                .and_then(|d| d.get("load_percentage").and_then(|v| v.as_str()))
                .and_then(|s| s.trim_end_matches('%').parse::<f64>().ok())
                .unwrap_or(0.0);

            provider_rows.push(Row::new(vec![
                Cell::from(url_short),
                Cell::from(if provider.is_healthy { "✓" } else { "✗" }).style(
                    if provider.is_healthy {
                        Style::default().fg(Color::Green)
                    } else {
                        Style::default().fg(Color::Red)
                    },
                ),
                Cell::from(format!("{request_count}")),
                Cell::from(format!("{success_rate:.1}%")).style(self.get_metric_color(
                    success_rate,
                    95.0,
                    80.0,
                )),
                Cell::from(format!("{load_percent:.1}%")),
                Cell::from(
                    provider
                        .response_time_ms
                        .map(|ms| format!("{ms}ms"))
                        .unwrap_or_else(|| "—".to_string()),
                )
                .style(
                    provider
                        .response_time_ms
                        .map(|ms| self.get_response_color(ms as f64))
                        .unwrap_or_else(|| Style::default().fg(Color::Gray)),
                ),
            ]));
        }

        // Add summary row at the top
        provider_rows.insert(
            0,
            Row::new(vec![
                Cell::from(format!("TOTAL: {total_providers} providers"))
                    .style(Style::default().add_modifier(Modifier::BOLD)),
                Cell::from(format!("{total_healthy}/{total_providers}"))
                    .style(Style::default().add_modifier(Modifier::BOLD)),
                Cell::from(format!("{total_requests}"))
                    .style(Style::default().add_modifier(Modifier::BOLD)),
                Cell::from("—"),
                Cell::from("100%").style(Style::default().add_modifier(Modifier::BOLD)),
                Cell::from(format!("{avg_response:.0}ms"))
                    .style(Style::default().add_modifier(Modifier::BOLD)),
            ]),
        );

        let table = Table::new(
            provider_rows,
            vec![
                Constraint::Min(60),    // Provider URL
                Constraint::Length(8),  // Status
                Constraint::Length(10), // Requests
                Constraint::Length(10), // Success %
                Constraint::Length(10), // Load %
                Constraint::Length(12), // Response
            ],
        )
        .header(
            Row::new(vec!["Provider", "Status", "Requests", "Success", "Load", "Response"])
                .style(Style::default().add_modifier(Modifier::BOLD)),
        )
        .block(Block::default().borders(Borders::ALL).title("Provider Analytics"))
        .row_highlight_style(Style::default().add_modifier(Modifier::BOLD));

        let mut table_state = ratatui::widgets::TableState::default();
        table_state.select(Some(self.provider_analytics_scroll));

        f.render_stateful_widget(table, area, &mut table_state);
    }

    fn render_provider_list(&self, f: &mut Frame<'_>, area: Rect) {
        let items: Vec<ListItem<'_>> = self
            .providers
            .iter()
            .map(|provider| {
                let status_icon = if provider.is_healthy { "🟢" } else { "🔴" };
                let response_time = provider
                    .response_time_ms
                    .map(|ms| format!("{ms}ms"))
                    .unwrap_or_else(|| "N/A".to_string());

                let line = Line::from(vec![
                    Span::raw(format!("{status_icon} ")),
                    Span::styled(
                        provider.url.chars().take(40).collect::<String>(),
                        if provider.is_healthy {
                            Style::default().fg(Color::Green)
                        } else {
                            Style::default().fg(Color::Red)
                        },
                    ),
                    Span::styled(format!(" ({response_time})"), Style::default().fg(Color::Gray)),
                ]);

                ListItem::new(line)
            })
            .collect();

        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title("RPC Providers"))
            .highlight_style(Style::default().bg(Color::DarkGray))
            .highlight_symbol("► ");

        let mut state = ListState::default();
        state.select(Some(self.selected_provider));

        f.render_stateful_widget(list, area, &mut state);
    }

    fn render_provider_details(&self, f: &mut Frame<'_>, area: Rect) {
        if let Some(provider) = self.providers.get(self.selected_provider) {
            // Get additional usage metrics from provider_metrics
            // The provider_metrics response has format: { "providers": [...] }
            let providers_data = self
                .provider_metrics
                .as_ref()
                .and_then(|m| m.get("providers"))
                .and_then(|p| p.as_array());

            // Find matching provider in the providers data array
            let provider_data = providers_data.and_then(|providers| {
                providers
                    .iter()
                    .find(|p| p.get("url").and_then(|u| u.as_str()) == Some(&provider.url))
                    .and_then(|p| p.as_object())
            });

            let error_count = provider_data
                .and_then(|m| m.get("error_count").and_then(|v| v.as_u64()))
                .unwrap_or(0);

            let last_used_timestamp = provider_data
                .and_then(|m| m.get("last_used_timestamp").and_then(|v| v.as_u64()))
                .unwrap_or(0);

            let last_used_text = if last_used_timestamp > 0 {
                let now =
                    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
                let seconds_ago = now.saturating_sub(last_used_timestamp);
                if seconds_ago < 60 {
                    format!("{seconds_ago}s ago")
                } else if seconds_ago < 3600 {
                    format!("{}m ago", seconds_ago / 60)
                } else {
                    format!("{}h ago", seconds_ago / 3600)
                }
            } else {
                "Never".to_string()
            };

            let details = vec![
                Line::from(vec![
                    Span::styled("URL: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(&provider.url),
                ]),
                Line::from(""),
                Line::from(vec![
                    Span::styled("Status: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::styled(
                        if provider.is_healthy { "Healthy" } else { "Unhealthy" },
                        if provider.is_healthy {
                            Style::default().fg(Color::Green)
                        } else {
                            Style::default().fg(Color::Red)
                        },
                    ),
                ]),
                Line::from(vec![
                    Span::styled("Response Time: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(
                        provider
                            .response_time_ms
                            .map(|ms| format!("{ms}ms"))
                            .unwrap_or_else(|| "N/A".to_string()),
                    ),
                ]),
                Line::from(vec![
                    Span::styled(
                        "Consecutive Failures: ",
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(format!("{}", provider.consecutive_failures)),
                ]),
                Line::from(vec![
                    Span::styled("Total Errors: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::styled(
                        format!("{error_count}"),
                        if error_count > 0 {
                            Style::default().fg(Color::Red)
                        } else {
                            Style::default().fg(Color::Green)
                        },
                    ),
                ]),
                Line::from(vec![
                    Span::styled("Last Used: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(last_used_text),
                ]),
                Line::from(vec![
                    Span::styled(
                        "Last Health Check: ",
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(
                        provider
                            .last_health_check_seconds_ago
                            .map(|s| format!("{s}s ago"))
                            .unwrap_or_else(|| "Never".to_string()),
                    ),
                ]),
            ];

            let details_widget = Paragraph::new(details)
                .block(Block::default().borders(Borders::ALL).title("Provider Details"))
                .wrap(Wrap { trim: true });

            f.render_widget(details_widget, area);
        } else {
            let empty = Paragraph::new("No provider selected")
                .block(Block::default().borders(Borders::ALL).title("Provider Details"))
                .alignment(Alignment::Center);
            f.render_widget(empty, area);
        }
    }

    pub fn render_cache(&mut self, f: &mut Frame<'_>, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(8), // Cache stats
                Constraint::Min(0),    // Cache details
            ])
            .split(area);

        // Cache statistics
        self.render_cache_stats(f, chunks[0]);

        // Enhanced cache details with hit/miss analytics
        self.render_enhanced_cache_details(f, chunks[1]);
    }

    fn render_cache_stats(&self, f: &mut Frame<'_>, area: Rect) {
        if let Some(stats) = &self.cache_stats {
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(25),
                    Constraint::Percentage(25),
                    Constraint::Percentage(25),
                    Constraint::Percentage(25),
                ])
                .split(area);

            // Total entries
            let entries_widget = Paragraph::new(vec![
                Line::from(vec![Span::styled(
                    "Total Entries",
                    Style::default().add_modifier(Modifier::BOLD),
                )]),
                Line::from(""),
                Line::from(format!("{}", stats.total_entries)),
                Line::from(format!("Max: {}", stats.max_entries)),
            ])
            .block(Block::default().borders(Borders::ALL))
            .alignment(Alignment::Center);

            // Utilization
            let util_color =
                if stats.utilization.trim_end_matches('%').parse::<f32>().unwrap_or(0.0) > 90.0 {
                    Color::Red
                } else if stats.utilization.trim_end_matches('%').parse::<f32>().unwrap_or(0.0)
                    > 75.0
                {
                    Color::Yellow
                } else {
                    Color::Green
                };

            let util_widget = Paragraph::new(vec![
                Line::from(vec![Span::styled(
                    "Utilization",
                    Style::default().add_modifier(Modifier::BOLD),
                )]),
                Line::from(""),
                Line::from(vec![Span::styled(
                    &stats.utilization,
                    Style::default().fg(util_color).add_modifier(Modifier::BOLD),
                )]),
                Line::from("of capacity"),
            ])
            .block(Block::default().borders(Borders::ALL))
            .alignment(Alignment::Center);

            // Oldest entry
            let oldest_widget = Paragraph::new(vec![
                Line::from(vec![Span::styled(
                    "Oldest Entry",
                    Style::default().add_modifier(Modifier::BOLD),
                )]),
                Line::from(""),
                Line::from(
                    stats
                        .oldest_entry_age_seconds
                        .map(|s| format!("{s}s ago"))
                        .unwrap_or_else(|| "N/A".to_string()),
                ),
                Line::from("age"),
            ])
            .block(Block::default().borders(Borders::ALL))
            .alignment(Alignment::Center);

            // Newest entry
            let newest_widget = Paragraph::new(vec![
                Line::from(vec![Span::styled(
                    "Newest Entry",
                    Style::default().add_modifier(Modifier::BOLD),
                )]),
                Line::from(""),
                Line::from(
                    stats
                        .newest_entry_age_seconds
                        .map(|s| format!("{s}s ago"))
                        .unwrap_or_else(|| "N/A".to_string()),
                ),
                Line::from("age"),
            ])
            .block(Block::default().borders(Borders::ALL))
            .alignment(Alignment::Center);

            f.render_widget(entries_widget, chunks[0]);
            f.render_widget(util_widget, chunks[1]);
            f.render_widget(oldest_widget, chunks[2]);
            f.render_widget(newest_widget, chunks[3]);
        } else {
            let empty = Paragraph::new("Loading cache statistics...")
                .block(Block::default().borders(Borders::ALL).title("Cache Statistics"))
                .alignment(Alignment::Center);
            f.render_widget(empty, area);
        }
    }

    fn render_enhanced_cache_details(&self, f: &mut Frame<'_>, area: Rect) {
        if let Some(stats) = &self.cache_stats {
            // Extract enhanced cache metrics
            let cache_hits = self
                .cache_metrics
                .as_ref()
                .and_then(|m| m.get("cache_hits").and_then(|v| v.as_u64()))
                .unwrap_or(0);
            let cache_misses = self
                .cache_metrics
                .as_ref()
                .and_then(|m| m.get("cache_misses").and_then(|v| v.as_u64()))
                .unwrap_or(0);
            let total_requests = cache_hits + cache_misses;

            // Get hit rate directly from backend
            let hit_rate = self
                .cache_metrics
                .as_ref()
                .and_then(|m| m.get("hit_rate").and_then(|v| v.as_str()))
                .and_then(|s| s.trim_end_matches('%').parse::<f64>().ok())
                .unwrap_or(0.0);

            // Extract method statistics if available
            let method_stats_lines = if let Some(metrics) = &self.cache_metrics {
                if let Some(method_stats) = metrics.get("method_stats").and_then(|v| v.as_object())
                {
                    let mut lines = vec![Line::from(vec![Span::styled(
                        "Top Methods by Cache Usage:",
                        Style::default().add_modifier(Modifier::BOLD),
                    )])];

                    // Sort methods by hit count and take top 5
                    let mut methods: Vec<_> = method_stats.iter().collect();
                    methods.sort_by(|a, b| {
                        let hits_a = a.1.get("hits").and_then(|v| v.as_u64()).unwrap_or(0);
                        let hits_b = b.1.get("hits").and_then(|v| v.as_u64()).unwrap_or(0);
                        hits_b.cmp(&hits_a)
                    });

                    for (method, stats) in methods.iter().take(5) {
                        let hits = stats.get("hits").and_then(|v| v.as_u64()).unwrap_or(0);
                        let misses = stats.get("misses").and_then(|v| v.as_u64()).unwrap_or(0);
                        let method_hit_rate = if hits + misses > 0 {
                            hits as f64 / (hits + misses) as f64 * 100.0
                        } else {
                            0.0
                        };
                        lines.push(Line::from(format!(
                            "• {method}: {hits} hits, {method_hit_rate:.1}% hit rate"
                        )));
                    }
                    lines
                } else {
                    vec![]
                }
            } else {
                vec![]
            };

            let mut details = vec![
                Line::from(vec![Span::styled(
                    "Cache File Path:",
                    Style::default().add_modifier(Modifier::BOLD),
                )]),
                Line::from(stats.cache_file_path.clone()),
                Line::from(""),
                Line::from(vec![Span::styled(
                    "Cache Information:",
                    Style::default().add_modifier(Modifier::BOLD),
                )]),
                Line::from(format!("• Total entries: {}", stats.total_entries)),
                Line::from(format!("• Maximum capacity: {}", stats.max_entries)),
                Line::from(format!("• Current utilization: {}", stats.utilization)),
                Line::from(""),
                Line::from(vec![Span::styled(
                    "Cache Performance:",
                    Style::default().add_modifier(Modifier::BOLD),
                )]),
                Line::from(vec![
                    Span::raw("• Hit Rate: "),
                    Span::styled(
                        format!("{hit_rate:.1}%"),
                        if hit_rate > 80.0 {
                            Style::default().fg(Color::Green)
                        } else if hit_rate > 50.0 {
                            Style::default().fg(Color::Yellow)
                        } else {
                            Style::default().fg(Color::Red)
                        },
                    ),
                ]),
                Line::from(format!("• Cache Hits: {cache_hits}")),
                Line::from(format!("• Cache Misses: {cache_misses}")),
                Line::from(format!("• Total Requests: {total_requests}")),
                Line::from(""),
                Line::from(vec![Span::styled(
                    "Entry Age Range:",
                    Style::default().add_modifier(Modifier::BOLD),
                )]),
                Line::from(format!(
                    "• Oldest: {}",
                    stats
                        .oldest_entry_age_seconds
                        .map(|s| format!("{s}s ago"))
                        .unwrap_or_else(|| "N/A".to_string())
                )),
                Line::from(format!(
                    "• Newest: {}",
                    stats
                        .newest_entry_age_seconds
                        .map(|s| format!("{s}s ago"))
                        .unwrap_or_else(|| "N/A".to_string())
                )),
            ];

            // Add method statistics if available
            if !method_stats_lines.is_empty() {
                details.push(Line::from(""));
                details.extend(method_stats_lines);
            }

            details.push(Line::from(""));
            details.push(Line::from(vec![
                Span::styled(
                    "Note:",
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                ),
                Span::raw(" Cache automatically evicts oldest entries when capacity is reached."),
            ]));

            let details_widget = Paragraph::new(details)
                .block(Block::default().borders(Borders::ALL).title("Enhanced Cache Analytics"))
                .wrap(Wrap { trim: true })
                .scroll((self.cache_details_scroll as u16, 0));

            f.render_widget(details_widget, area);
        } else {
            let empty = Paragraph::new("Loading cache details...")
                .block(Block::default().borders(Borders::ALL).title("Cache Details"))
                .alignment(Alignment::Center);
            f.render_widget(empty, area);
        }
    }

    pub fn render_methods(&mut self, f: &mut Frame<'_>, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(8), // Method statistics summary
                Constraint::Min(0),    // Method details
            ])
            .split(area);

        // Method statistics summary
        self.render_method_stats_summary(f, chunks[0]);

        // Method details
        self.render_method_details(f, chunks[1]);
    }

    fn render_method_stats_summary(&self, f: &mut Frame<'_>, area: Rect) {
        if let Some(metrics) = &self.cache_metrics {
            if let Some(method_stats) = metrics.get("method_stats").and_then(|v| v.as_object()) {
                let chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([
                        Constraint::Percentage(25),
                        Constraint::Percentage(25),
                        Constraint::Percentage(25),
                        Constraint::Percentage(25),
                    ])
                    .split(area);

                // Total unique methods
                let method_count = method_stats.len();
                let methods_widget = Paragraph::new(vec![
                    Line::from(vec![Span::styled(
                        "Total Methods",
                        Style::default().add_modifier(Modifier::BOLD),
                    )]),
                    Line::from(""),
                    Line::from(format!("{method_count}")),
                    Line::from("tracked"),
                ])
                .block(Block::default().borders(Borders::ALL))
                .alignment(Alignment::Center);

                // Most used method
                let most_used = method_stats
                    .iter()
                    .max_by_key(|(_, stats)| {
                        stats.get("hits").and_then(|v| v.as_u64()).unwrap_or(0)
                            + stats.get("misses").and_then(|v| v.as_u64()).unwrap_or(0)
                    })
                    .map(|(method, _)| method.as_str())
                    .unwrap_or("N/A");

                let most_used_widget = Paragraph::new(vec![
                    Line::from(vec![Span::styled(
                        "Most Used",
                        Style::default().add_modifier(Modifier::BOLD),
                    )]),
                    Line::from(""),
                    Line::from(most_used.chars().take(12).collect::<String>()),
                    Line::from("method"),
                ])
                .block(Block::default().borders(Borders::ALL))
                .alignment(Alignment::Center);

                // Best hit rate method
                let best_hit_rate = method_stats
                    .iter()
                    .filter_map(|(method, stats)| {
                        let hits = stats.get("hits").and_then(|v| v.as_u64()).unwrap_or(0);
                        let misses = stats.get("misses").and_then(|v| v.as_u64()).unwrap_or(0);
                        let total = hits + misses;
                        if total > 0 {
                            Some((method.as_str(), hits as f64 / total as f64))
                        } else {
                            None
                        }
                    })
                    .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
                    .map(|(method, rate)| {
                        (method.chars().take(12).collect::<String>(), rate * 100.0)
                    })
                    .unwrap_or_else(|| ("N/A".to_string(), 0.0));

                let best_rate_widget = Paragraph::new(vec![
                    Line::from(vec![Span::styled(
                        "Best Hit Rate",
                        Style::default().add_modifier(Modifier::BOLD),
                    )]),
                    Line::from(""),
                    Line::from(best_hit_rate.0),
                    Line::from(format!("{:.1}%", best_hit_rate.1)),
                ])
                .block(Block::default().borders(Borders::ALL))
                .alignment(Alignment::Center);

                // Total method requests
                let total_requests: u64 = method_stats
                    .values()
                    .map(|stats| {
                        let hits = stats.get("hits").and_then(|v| v.as_u64()).unwrap_or(0);
                        let misses = stats.get("misses").and_then(|v| v.as_u64()).unwrap_or(0);
                        hits + misses
                    })
                    .sum();

                let total_widget = Paragraph::new(vec![
                    Line::from(vec![Span::styled(
                        "Total Requests",
                        Style::default().add_modifier(Modifier::BOLD),
                    )]),
                    Line::from(""),
                    Line::from(format!("{total_requests}")),
                    Line::from("processed"),
                ])
                .block(Block::default().borders(Borders::ALL))
                .alignment(Alignment::Center);

                f.render_widget(methods_widget, chunks[0]);
                f.render_widget(most_used_widget, chunks[1]);
                f.render_widget(best_rate_widget, chunks[2]);
                f.render_widget(total_widget, chunks[3]);
            } else {
                let empty = Paragraph::new("No method statistics available")
                    .block(Block::default().borders(Borders::ALL).title("Method Statistics"))
                    .alignment(Alignment::Center);
                f.render_widget(empty, area);
            }
        } else {
            let empty = Paragraph::new("Loading method statistics...")
                .block(Block::default().borders(Borders::ALL).title("Method Statistics"))
                .alignment(Alignment::Center);
            f.render_widget(empty, area);
        }
    }

    fn render_method_details(&self, f: &mut Frame<'_>, area: Rect) {
        if let Some(metrics) = &self.cache_metrics {
            if let Some(method_stats) = metrics.get("method_stats").and_then(|v| v.as_object()) {
                // Sort methods by total requests (hits + misses) descending
                let mut method_list: Vec<_> = method_stats.iter().collect();
                method_list.sort_by(|a, b| {
                    let requests_a = a.1.get("hits").and_then(|v| v.as_u64()).unwrap_or(0)
                        + a.1.get("misses").and_then(|v| v.as_u64()).unwrap_or(0);
                    let requests_b = b.1.get("hits").and_then(|v| v.as_u64()).unwrap_or(0)
                        + b.1.get("misses").and_then(|v| v.as_u64()).unwrap_or(0);
                    requests_b.cmp(&requests_a)
                });

                let mut details = vec![
                    Line::from(vec![Span::styled(
                        "Method-Level Cache Statistics",
                        Style::default().add_modifier(Modifier::BOLD),
                    )]),
                    Line::from(""),
                    Line::from(vec![
                        Span::styled("Method", Style::default().add_modifier(Modifier::BOLD)),
                        Span::raw("                 "),
                        Span::styled("Requests", Style::default().add_modifier(Modifier::BOLD)),
                        Span::raw("  "),
                        Span::styled("Hits", Style::default().add_modifier(Modifier::BOLD)),
                        Span::raw("     "),
                        Span::styled("Misses", Style::default().add_modifier(Modifier::BOLD)),
                        Span::raw("   "),
                        Span::styled("Hit Rate", Style::default().add_modifier(Modifier::BOLD)),
                    ]),
                    Line::from("─".repeat(80)),
                ];

                for (method, stats) in method_list.iter().take(15) {
                    let hits = stats.get("hits").and_then(|v| v.as_u64()).unwrap_or(0);
                    let misses = stats.get("misses").and_then(|v| v.as_u64()).unwrap_or(0);
                    let total = hits + misses;
                    let hit_rate = if total > 0 { hits as f64 / total as f64 * 100.0 } else { 0.0 };

                    let method_display = if method.len() > 25 {
                        format!("{}...", &method[..22])
                    } else {
                        method.to_string()
                    };

                    let hit_rate_color = if hit_rate > 80.0 {
                        Color::Green
                    } else if hit_rate > 50.0 {
                        Color::Yellow
                    } else {
                        Color::Red
                    };

                    details.push(Line::from(vec![
                        Span::raw(format!("{method_display:<25}")),
                        Span::raw(format!("{total:>8}")),
                        Span::raw(format!("{hits:>8}")),
                        Span::raw(format!("{misses:>8}")),
                        Span::raw("  "),
                        Span::styled(
                            format!("{hit_rate:>6.1}%"),
                            Style::default().fg(hit_rate_color),
                        ),
                    ]));
                }

                if method_list.len() > 15 {
                    details.push(Line::from(""));
                    details.push(Line::from(vec![Span::styled(
                        format!(
                            "... and {} more methods (scroll to see all)",
                            method_list.len() - 15
                        ),
                        Style::default().fg(Color::Gray),
                    )]));
                }

                let details_widget = Paragraph::new(details)
                    .block(
                        Block::default().borders(Borders::ALL).title("Method Performance Details"),
                    )
                    .wrap(Wrap { trim: false })
                    .scroll((self.methods_scroll as u16, 0));

                f.render_widget(details_widget, area);
            } else {
                let empty = Paragraph::new(vec![
                    Line::from("No detailed method statistics available."),
                    Line::from(""),
                    Line::from("Method-level cache statistics will appear here"),
                    Line::from("once RPC requests have been processed."),
                ])
                .block(Block::default().borders(Borders::ALL).title("Method Performance Details"))
                .alignment(Alignment::Center);
                f.render_widget(empty, area);
            }
        } else {
            let empty = Paragraph::new("Loading method details...")
                .block(Block::default().borders(Borders::ALL).title("Method Performance Details"))
                .alignment(Alignment::Center);
            f.render_widget(empty, area);
        }
    }

    pub fn render_instances(&mut self, f: &mut Frame<'_>, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(5), // Summary
                Constraint::Min(0),    // Instance list
            ])
            .split(area);

        // Instance summary
        self.render_instance_summary(f, chunks[0]);

        // Instance list
        self.render_instance_list(f, chunks[1]);
    }

    fn render_instance_summary(&self, f: &mut Frame<'_>, area: Rect) {
        let instance_count = self.active_instances.len();
        let status_text = if instance_count > 0 {
            format!(
                "🟢 {} active debugging session{}",
                instance_count,
                if instance_count == 1 { "" } else { "s" }
            )
        } else {
            "⚪ No active debugging sessions".to_string()
        };

        let summary = Paragraph::new(vec![
            Line::from(vec![Span::styled(
                "EDB Instance Registry",
                Style::default().add_modifier(Modifier::BOLD),
            )]),
            Line::from(""),
            Line::from(status_text),
        ])
        .block(Block::default().borders(Borders::ALL).title("Summary"))
        .alignment(Alignment::Center);

        f.render_widget(summary, area);
    }

    fn render_instance_list(&self, f: &mut Frame<'_>, area: Rect) {
        if self.active_instances.is_empty() {
            let empty = Paragraph::new(vec![
                Line::from("No EDB instances are currently registered."),
                Line::from(""),
                Line::from("When EDB instances connect to this proxy, they will"),
                Line::from("appear here with their process IDs and status."),
                Line::from(""),
                Line::from(vec![
                    Span::styled(
                        "Note: ",
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                    ),
                    Span::raw("EDB instances automatically register when using"),
                ]),
                Line::from("this proxy as their RPC endpoint."),
            ])
            .block(Block::default().borders(Borders::ALL).title("Active Instances"))
            .alignment(Alignment::Center);

            f.render_widget(empty, area);
        } else {
            let items: Vec<ListItem<'_>> = self
                .active_instances
                .iter()
                .map(|&pid| {
                    let line = Line::from(vec![
                        Span::raw("🟢 "),
                        Span::styled(format!("PID: {pid}"), Style::default().fg(Color::Green)),
                        Span::styled(" (Active)", Style::default().fg(Color::Gray)),
                    ]);
                    ListItem::new(line)
                })
                .collect();

            let list = List::new(items)
                .block(Block::default().borders(Borders::ALL).title("Active EDB Instances"));

            let mut list_state = ratatui::widgets::ListState::default();
            list_state.select(Some(self.instances_scroll));

            f.render_stateful_widget(list, area, &mut list_state);
        }
    }
}
