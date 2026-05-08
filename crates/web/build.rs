// EDB - Ethereum Debugger
// Copyright (C) 2024 Zhuo Zhang and Wuqi Zhang
//
// SPDX-License-Identifier: AGPL-3.0-only

//! Build script for `edb-web`: invokes `bun install` and `bun run build`
//! against `frontend/`, producing `frontend/dist/` which `lib.rs` embeds.
//!
//! Set `EDB_SKIP_WEB_BUILD=1` to skip the bun build (useful in IDEs that don't
//! have bun available). The resulting binary will fail at startup if web
//! routes are exercised.

use std::path::PathBuf;
use std::process::Command;

fn main() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let frontend = manifest_dir.join("frontend");

    // Tell cargo to rerun this script if any frontend source changes.
    for rel in ["src", "index.html", "package.json", "bun.lock", "tsconfig.json", "vite.config.ts"]
    {
        println!("cargo:rerun-if-changed=frontend/{rel}");
    }
    println!("cargo:rerun-if-env-changed=EDB_SKIP_WEB_BUILD");

    // Ensure dist exists with at least a placeholder so rust-embed has files
    // even when EDB_SKIP_WEB_BUILD=1 (the resulting binary panics at runtime
    // when router() is called — we want compile + check to succeed though).
    std::fs::create_dir_all(frontend.join("dist")).ok();
    let placeholder = frontend.join("dist/index.html");
    if !placeholder.exists() {
        std::fs::write(
            &placeholder,
            "<!DOCTYPE html><html><body><div id=\"root\">edb-web: dist not built (EDB_SKIP_WEB_BUILD=1 or bun missing)</div></body></html>",
        ).expect("write placeholder index.html");
    }

    if std::env::var_os("EDB_SKIP_WEB_BUILD").is_some() {
        println!("cargo:warning=EDB_SKIP_WEB_BUILD is set; skipping bun build");
        return;
    }

    // Verify bun is available.
    let bun_check = Command::new("bun").arg("--version").output();
    match bun_check {
        Ok(out) if out.status.success() => {}
        _ => panic!(
            "[edb-web build] bun not found on PATH. Install via `curl -fsSL https://bun.sh/install | bash`, or set EDB_SKIP_WEB_BUILD=1 to bypass."
        ),
    }

    let install = Command::new("bun")
        .args(["install", "--frozen-lockfile"])
        .current_dir(&frontend)
        .status()
        .expect("[edb-web build] failed to spawn `bun install`");
    if !install.success() {
        panic!("[edb-web build] `bun install --frozen-lockfile` failed");
    }

    let build = Command::new("bun")
        .args(["run", "build"])
        .current_dir(&frontend)
        .status()
        .expect("[edb-web build] failed to spawn `bun run build`");
    if !build.success() {
        panic!("[edb-web build] `bun run build` failed");
    }
}
