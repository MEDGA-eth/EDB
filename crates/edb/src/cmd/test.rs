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

//! `edb test` — Foundry test debugging command.

use eyre::Result;

pub async fn run_foundry_test(
    _target: &str,
    _root: Option<&str>,
    _profile: Option<&str>,
    _fork_url: Option<&str>,
    _fork_block_number: Option<u64>,
    _cli: &crate::Cli,
) -> Result<()> {
    eyre::bail!("edb test is under active development on feat/foundry-test. Not yet implemented.")
}
