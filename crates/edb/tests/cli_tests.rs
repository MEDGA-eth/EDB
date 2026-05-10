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

//! CLI tests for EDB

use predicates::prelude::*;
use tracing::info;

#[test]
fn test_help_command() {
    edb_common::ensure_test_logging(None);
    info!("Testing CLI help command");

    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("edb");
    cmd.arg("--help").assert().success().stdout(predicate::str::contains("Ethereum Debugger"));
}

#[test]
fn test_version_command() {
    edb_common::logging::ensure_test_logging(None);
    info!("Running test");
    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("edb");
    cmd.arg("--version").assert().success().stdout(predicate::str::contains("edb"));
}

#[test]
fn test_replay_subcommand_help() {
    edb_common::logging::ensure_test_logging(None);
    info!("Running test");
    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("edb");
    cmd.arg("replay")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Replay an existing transaction"));
}

#[test]
fn test_test_subcommand_help() {
    edb_common::logging::ensure_test_logging(None);
    info!("Running test");
    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("edb");
    cmd.arg("test")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Debug a Foundry test case"));
}

#[test]
fn test_invalid_tx_hash() {
    edb_common::logging::ensure_test_logging(None);
    info!("Running test");
    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("edb");
    cmd.arg("replay").arg("invalid_hash").assert().failure();
}

#[test]
fn test_missing_subcommand() {
    edb_common::logging::ensure_test_logging(None);
    info!("Running test");
    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("edb");
    cmd.assert().failure().stderr(predicate::str::contains("Usage"));
}

#[test]
fn test_replay_requires_rpc_urls() {
    edb_common::logging::ensure_test_logging(None);
    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("edb");
    cmd.arg("replay")
        .arg("0x0000000000000000000000000000000000000000000000000000000000000000")
        .assert()
        .failure()
        .stderr(predicate::str::contains("--rpc-urls"));
}

#[test]
fn test_rpc_urls_not_at_top_level() {
    edb_common::logging::ensure_test_logging(None);
    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("edb");
    cmd.arg("--rpc-urls").arg("http://x").arg("--help").assert().failure();
}

#[test]
fn test_disable_mouse_flag_removed() {
    edb_common::logging::ensure_test_logging(None);
    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("edb");
    cmd.arg("--disable-mouse").arg("--help").assert().failure();
}

#[test]
fn test_test_subcommand_takes_qualified_name() {
    edb_common::logging::ensure_test_logging(None);
    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("edb");
    cmd.arg("test")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("CONTRACT::TEST_FN"));
}

#[test]
fn test_server_requires_rpc_urls() {
    edb_common::logging::ensure_test_logging(None);
    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("edb");
    cmd.arg("server").assert().failure().stderr(predicate::str::contains("--rpc-urls"));
}
