// EDB - Ethereum Debugger
// Copyright (C) 2024 Zhuo Zhang and Wuqi Zhang
// SPDX-License-Identifier: AGPL-3.0

//! Foundry project resolution: find `foundry.toml`, pick a profile, load config.

use eyre::{Result, bail};
use foundry_config::Config;
use std::path::{Path, PathBuf};

/// Outcome of resolving the project context for `edb test`.
#[derive(Debug)]
#[allow(dead_code)] // fields consumed by downstream tasks (3.3+)
pub struct ResolvedProject {
    pub root: PathBuf,
    pub profile_name: String,
    pub config: Config,
}

pub fn resolve_project(
    root_arg: Option<&str>,
    profile_arg: Option<&str>,
) -> Result<ResolvedProject> {
    let root = match root_arg {
        Some(p) => {
            let path = PathBuf::from(p);
            if !path.join("foundry.toml").exists() {
                bail!("no foundry.toml at --root {}", path.display());
            }
            path
        }
        None => find_root_by_walking_up()?,
    };

    let profile_name = profile_arg
        .map(str::to_owned)
        .or_else(|| std::env::var("FOUNDRY_PROFILE").ok())
        .unwrap_or_else(|| "default".into());

    // Foundry's Config respects FOUNDRY_PROFILE; set it so the load picks the right profile.
    // SAFETY: edition 2024 requires unsafe; runs early in `run_foundry_test` before any
    // background threads read this env var.
    unsafe {
        std::env::set_var("FOUNDRY_PROFILE", &profile_name);
    }

    let config = load_config_at(&root)?;
    Ok(ResolvedProject { root, profile_name, config })
}

fn load_config_at(root: &Path) -> Result<Config> {
    // Config::load_with_root returns Result<Self, ExtractConfigError> in foundry-config v1.7.1.
    Config::load_with_root(root).map_err(|e| eyre::eyre!("{e}"))
}

fn find_root_by_walking_up() -> Result<PathBuf> {
    let cwd = std::env::current_dir()?;
    for dir in cwd.ancestors() {
        if dir.join("foundry.toml").is_file() {
            return Ok(dir.to_path_buf());
        }
    }
    bail!("no foundry.toml found in {} or any parent directory. Use --root.", cwd.display())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::TempDir;

    fn write_toml(dir: &std::path::Path, contents: &str) {
        std::fs::write(dir.join("foundry.toml"), contents).unwrap();
    }

    #[test]
    #[serial]
    fn finds_foundry_toml_in_explicit_root() {
        // Ensure no stale FOUNDRY_PROFILE from other serial tests.
        unsafe {
            std::env::remove_var("FOUNDRY_PROFILE");
        }
        let dir = TempDir::new().unwrap();
        write_toml(dir.path(), "[profile.default]\nsrc = 'src'\n");
        let resolved = resolve_project(Some(dir.path().to_str().unwrap()), None).unwrap();
        assert_eq!(resolved.root, dir.path());
        assert_eq!(resolved.profile_name, "default");
    }

    #[test]
    #[serial]
    fn errors_when_not_in_a_foundry_project() {
        let dir = TempDir::new().unwrap();
        let prev = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        let err = resolve_project(None, None).unwrap_err();
        // Restore even on assertion failure later
        std::env::set_current_dir(prev).unwrap();
        assert!(err.to_string().contains("foundry.toml"));
    }

    #[test]
    #[serial]
    fn respects_explicit_profile() {
        let dir = TempDir::new().unwrap();
        write_toml(dir.path(), "[profile.default]\nsrc='src'\n[profile.ci]\nsrc='alt'\n");
        let resolved = resolve_project(Some(dir.path().to_str().unwrap()), Some("ci")).unwrap();
        assert_eq!(resolved.profile_name, "ci");
        // Clean up so subsequent serial tests don't inherit "ci".
        unsafe {
            std::env::remove_var("FOUNDRY_PROFILE");
        }
    }

    #[test]
    #[serial]
    fn walks_up_for_foundry_toml() {
        unsafe {
            std::env::remove_var("FOUNDRY_PROFILE");
        }
        let dir = TempDir::new().unwrap();
        let sub = dir.path().join("a").join("b");
        std::fs::create_dir_all(&sub).unwrap();
        write_toml(dir.path(), "[profile.default]\n");
        let prev = std::env::current_dir().unwrap();
        std::env::set_current_dir(&sub).unwrap();
        let result = resolve_project(None, None);
        std::env::set_current_dir(prev).unwrap();
        unsafe {
            std::env::remove_var("FOUNDRY_PROFILE");
        }
        let resolved = result.unwrap();
        // canonicalize because TempDir may differ on macOS (/var vs /private/var symlink)
        assert_eq!(resolved.root.canonicalize().unwrap(), dir.path().canonicalize().unwrap());
    }
}
