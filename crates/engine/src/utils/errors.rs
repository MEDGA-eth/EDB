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

//! Utilities for error handling and reporting
//! Provides functions for sanitizing paths, extracting code context, and more.

use std::{
    fs,
    path::{Path, PathBuf},
};

use alloy_primitives::Address;
use eyre::Result;
use foundry_compilers::artifacts::SolcInput;
use tracing::warn;

/// Sanitize a path to prevent directory traversal attacks
fn sanitize_path(path: &std::path::Path) -> PathBuf {
    use std::path::Component;

    let mut sanitized = PathBuf::new();

    for component in path.components() {
        match component {
            Component::Normal(name) => {
                // Only add normal path components (no .., ., or absolute paths)
                sanitized.push(name);
            }
            Component::CurDir => {
                // Skip "." components
            }
            Component::ParentDir => {
                // Skip ".." components - don't allow traversal
                warn!("Skipping parent directory component in path: {:?}", path);
            }
            Component::RootDir => {
                // Skip root directory "/" - don't allow absolute paths
                warn!("Skipping root directory component in path: {:?}", path);
            }
            Component::Prefix(_) => {
                // Skip Windows drive prefixes like "C:"
                warn!("Skipping prefix component in path: {:?}", path);
            }
        }
    }

    // If the path was completely stripped, use a default name
    if sanitized.as_os_str().is_empty() {
        sanitized.push("unnamed_source");
    }

    sanitized
}

/// Extract code context around an error position
fn extract_code_context(
    file_path: &std::path::Path,
    start_pos: i32,
    end_pos: i32,
    context_lines: usize,
) -> Option<String> {
    use std::io::{BufRead, BufReader};

    let file = fs::File::open(file_path).ok()?;
    let reader = BufReader::new(file);
    let lines: Vec<String> = reader.lines().map_while(Result::ok).collect();

    // Convert byte positions to line/column
    let mut current_pos = 0i32;
    let mut start_line = 0;
    let mut start_col = 0;
    let mut end_line = 0;
    let mut end_col = 0;

    for (line_num, line) in lines.iter().enumerate() {
        let line_start = current_pos;
        let line_end = current_pos + line.len() as i32 + 1; // +1 for newline

        if start_pos >= line_start && start_pos < line_end {
            start_line = line_num;
            start_col = (start_pos - line_start) as usize;
        }

        if end_pos >= line_start && end_pos <= line_end {
            end_line = line_num;
            end_col = (end_pos - line_start) as usize;
        }

        current_pos = line_end;
    }

    // Build context
    let mut context = String::new();
    let context_start = start_line.saturating_sub(context_lines);
    let context_end = (end_line + context_lines + 1).min(lines.len());

    for line_num in context_start..context_end {
        if line_num >= lines.len() {
            break;
        }

        let line_number = line_num + 1; // 1-indexed
        let line = &lines[line_num];

        // Format line with line number
        if line_num >= start_line && line_num <= end_line {
            // Error line - highlight it
            context.push_str(&format!("  {line_number} | {line}\n"));

            // Add underline for the error position on the first error line
            if line_num == start_line {
                let padding = format!("  {line_number} | ").len();
                let mut underline = " ".repeat(padding + start_col);
                let underline_len = if start_line == end_line {
                    (end_col - start_col).max(1)
                } else {
                    line.len() - start_col
                };
                underline.push_str(&"^".repeat(underline_len));
                context.push_str(&format!("{underline}\n"));
            }
        } else {
            // Context line
            context.push_str(&format!("  {line_number} | {line}\n"));
        }
    }

    Some(context)
}

/// Format compiler errors with better source location information
pub fn format_compiler_errors(
    errors: &[foundry_compilers::artifacts::Error],
    dump_dir: &std::path::Path,
) -> String {
    let mut formatted = String::new();

    for error in errors.iter().filter(|e| e.is_error()) {
        formatted.push_str("\n\n");

        // Add error severity and type
        if let Some(error_code) = &error.error_code {
            formatted.push_str(&format!("Error [{error_code}]: "));
        } else {
            formatted.push_str("Error: ");
        }

        // Add the main error message
        formatted.push_str(&error.message);

        // Add source location and code context if available
        if let Some(loc) = &error.source_location {
            formatted.push_str(&format!("\n  --> {}:{}:{}", loc.file.as_str(), loc.start, loc.end));

            // Try to extract code context from the dumped files
            let sanitized_path = sanitize_path(std::path::Path::new(&loc.file));
            let source_file = dump_dir.join(&sanitized_path);

            if let Some(context) = extract_code_context(&source_file, loc.start, loc.end, 5) {
                formatted.push_str("\n\n");
                formatted.push_str(&context);
            }
        }

        // If we have a formatted message with context, also include it
        // (as it might have additional information)
        if let Some(formatted_msg) = &error.formatted_message
            && !formatted_msg.trim().is_empty()
        {
            formatted.push_str("\n\nCompiler's formatted output:\n");
            formatted.push_str(formatted_msg);
        }

        // Add secondary locations if any
        if !error.secondary_source_locations.is_empty() {
            for sec_loc in &error.secondary_source_locations {
                if let Some(msg) = &sec_loc.message {
                    formatted.push_str(&format!("\n  Note: {msg}"));
                }
                if let Some(file) = &sec_loc.file {
                    formatted.push_str(&format!(
                        "\n    --> {}:{}:{}",
                        file,
                        sec_loc.start.map(|s| s.to_string()).unwrap_or_else(|| "?".to_string()),
                        sec_loc.end.map(|e| e.to_string()).unwrap_or_else(|| "?".to_string())
                    ));

                    // Try to show context for secondary locations too
                    if let (Some(start), Some(end)) = (sec_loc.start, sec_loc.end) {
                        let sanitized_path = sanitize_path(std::path::Path::new(file));
                        let source_file = dump_dir.join(&sanitized_path);

                        if let Some(context) = extract_code_context(&source_file, start, end, 1) {
                            formatted.push('\n');
                            formatted.push_str(&context);
                        }
                    }
                }
            }
        }
    }

    if formatted.is_empty() {
        formatted.push_str("\nNo specific error details available");
    }

    formatted
}

/// Dump source code to a temporary directory for debugging
pub fn dump_source_for_debugging(
    address: &Address,
    original_input: &SolcInput,
    instrumented_input: &SolcInput,
) -> Result<(PathBuf, PathBuf)> {
    use std::io::Write;

    // Create temp directories
    let temp_dir = std::env::temp_dir();
    let debug_dir = temp_dir.join(format!("edb_debug_{address}"));
    let original_dir = debug_dir.join("original");
    let instrumented_dir = debug_dir.join("instrumented");

    // Create directories
    fs::create_dir_all(&original_dir)?;
    fs::create_dir_all(&instrumented_dir)?;

    // Write original sources
    for (path_str, source) in &original_input.sources {
        let path = Path::new(path_str);
        let sanitized_path = sanitize_path(path);
        let file_path = original_dir.join(&sanitized_path);

        // Safety check: verify the resulting path is still within our directory
        // We use a path-based check rather than canonicalize to avoid TOCTOU issues
        if !file_path.starts_with(&original_dir) {
            return Err(eyre::eyre!(
                "Path traversal detected in source path: {}",
                path_str.display()
            ));
        }

        // Create parent directories if needed
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut file = fs::File::create(&file_path)?;
        file.write_all(source.content.as_bytes())?;
    }

    // Write original settings.json
    let settings_path = original_dir.join("settings.json");
    let mut settings_file = fs::File::create(&settings_path)?;
    settings_file.write_all(serde_json::to_string_pretty(&original_input.settings)?.as_bytes())?;

    // Write instrumented sources
    for (path_str, source) in &instrumented_input.sources {
        let path = std::path::Path::new(path_str);
        let sanitized_path = sanitize_path(path);
        let file_path = instrumented_dir.join(&sanitized_path);

        // Safety check: verify the resulting path is still within our directory
        // We use a path-based check rather than canonicalize to avoid TOCTOU issues
        if !file_path.starts_with(&instrumented_dir) {
            return Err(eyre::eyre!(
                "Path traversal detected in source path: {}",
                path_str.display()
            ));
        }

        // Create parent directories if needed
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut file = fs::File::create(&file_path)?;
        file.write_all(source.content.as_bytes())?;
    }

    // Write instrumented settings.json
    let settings_path = instrumented_dir.join("settings.json");
    let mut settings_file = fs::File::create(&settings_path)?;
    settings_file
        .write_all(serde_json::to_string_pretty(&instrumented_input.settings)?.as_bytes())?;

    Ok((original_dir, instrumented_dir))
}
