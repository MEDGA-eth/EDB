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

//! Animated spinner components for loading states
//!
//! Provides smooth animated spinners to indicate RPC calls and background processing

use std::time::{Duration, Instant};

/// Animated spinner with configurable frames and speed
#[derive(Debug)]
pub struct SpinnerAnimation {
    /// Current frame index
    current_frame: usize,
    /// Spinner animation frames
    frames: &'static [&'static str],
    /// Time between frame updates
    frame_duration: Duration,
    /// Last update time
    last_update: Instant,
    /// Whether the spinner is currently active
    active: bool,
}

impl SpinnerAnimation {
    /// Create a new spinner with default braille pattern
    pub fn new() -> Self {
        Self {
            current_frame: 0,
            frames: SpinnerStyles::BRAILLE,
            frame_duration: Duration::from_millis(100),
            last_update: Instant::now(),
            active: false,
        }
    }

    /// Create a spinner with custom frames
    pub fn with_frames(mut self, frames: &'static [&'static str]) -> Self {
        self.frames = frames;
        self
    }

    /// Create a spinner with custom speed
    pub fn with_speed(mut self, frame_duration: Duration) -> Self {
        self.frame_duration = frame_duration;
        self
    }

    /// Start the spinner animation
    pub fn start(&mut self) {
        self.active = true;
        self.last_update = Instant::now();
    }

    /// Stop the spinner animation
    pub fn stop(&mut self) {
        self.active = false;
        self.current_frame = 0;
    }

    /// Update the spinner animation (call this in render loop)
    pub fn tick(&mut self) {
        if !self.active || self.frames.is_empty() {
            return;
        }

        let now = Instant::now();
        if now.duration_since(self.last_update) >= self.frame_duration {
            self.current_frame = (self.current_frame + 1) % self.frames.len();
            self.last_update = now;
        }
    }

    /// Get the current spinner frame
    pub fn frame(&self) -> &'static str {
        if !self.active || self.frames.is_empty() { "" } else { self.frames[self.current_frame] }
    }

    /// Check if the spinner is active
    pub fn is_active(&self) -> bool {
        self.active
    }
}

impl Default for SpinnerAnimation {
    fn default() -> Self {
        Self::new()
    }
}

/// Collection of pre-defined spinner styles
pub struct SpinnerStyles;

impl SpinnerStyles {
    /// Braille pattern spinner (default) - smooth, low profile
    pub const BRAILLE: &'static [&'static str] =
        &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

    /// Dots spinner - simple and clean
    pub const DOTS: &'static [&'static str] = &["⠁", "⠂", "⠄", "⡀", "⢀", "⠠", "⠐", "⠈"];

    /// Clock spinner - classic clock face animation
    pub const CLOCK: &'static [&'static str] = &[
        "🕛", "🕧", "🕐", "🕜", "🕑", "🕝", "🕒", "🕞", "🕓", "🕟", "🕔", "🕠", "🕕", "🕡", "🕖",
        "🕢", "🕗", "🕣", "🕘", "🕤", "🕙", "🕥", "🕚", "🕦",
    ];

    /// Arrows spinner - rotating arrows
    pub const ARROWS: &'static [&'static str] = &["←", "↖", "↑", "↗", "→", "↘", "↓", "↙"];

    /// Bounce spinner - bouncing ball effect
    pub const BOUNCE: &'static [&'static str] = &["⠁", "⠂", "⠄", "⠂"];

    /// Progress spinner - filling effect
    pub const PROGRESS: &'static [&'static str] =
        &["▏", "▎", "▍", "▌", "▋", "▊", "▉", "█", "▉", "▊", "▋", "▌", "▍", "▎"];

    /// Simple rotating spinner - minimal style
    pub const SIMPLE: &'static [&'static str] = &["|", "/", "-", "\\"];

    /// Square spinner - rotating squares
    pub const SQUARE: &'static [&'static str] = &["▖", "▘", "▝", "▗"];
}

/// RPC loading state with spinner
#[derive(Debug)]
pub struct Spinner {
    /// The underlying spinner
    spinner: SpinnerAnimation,
    /// Current operation description
    operation: Option<String>,
    /// Whether we're waiting for RPC response
    waiting: bool,
}

impl Spinner {
    /// Create a new spinner
    pub fn new(frames: Option<&'static [&'static str]>, frame_duration: Option<Duration>) -> Self {
        Self {
            spinner: SpinnerAnimation::new()
                .with_frames(frames.unwrap_or(SpinnerStyles::BRAILLE))
                .with_speed(frame_duration.unwrap_or(Duration::from_millis(100))),
            operation: None,
            waiting: false,
        }
    }

    /// Start loading with operation description
    pub fn start_loading(&mut self, operation: &str) {
        self.operation = Some(operation.to_string());
        self.waiting = true;
        self.spinner.start();
    }

    /// Finish loading
    pub fn finish_loading(&mut self) {
        self.operation = None;
        self.waiting = false;
        self.spinner.stop();
    }

    /// Update the spinner animation
    pub fn tick(&mut self) {
        self.spinner.tick();
    }

    /// Get the loading display text
    pub fn display_text(&self) -> String {
        if let Some(ref op) = self.operation {
            if self.waiting { format!("{} {}", self.spinner.frame(), op) } else { op.clone() }
        } else {
            String::new()
        }
    }

    /// Check if currently loading
    pub fn is_loading(&self) -> bool {
        self.waiting
    }
}

impl Default for Spinner {
    fn default() -> Self {
        Self::new(None, None)
    }
}
