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

//! Debug command - debug_foundry_test function

use eyre::Result;

/// Debug a Foundry test case
pub async fn debug_foundry_test(
    _test_name: &str,
    _block: Option<u64>,
    _cli: &crate::Cli,
    _rpc_url: &str,
) -> Result<()> {
    // The Foundry test debugging path is still under design. Returning a
    // friendly error here is preferable to `unimplemented!()` because the
    // panic crashes the entire process — including the web UI host —
    // before the browser even loads. With a clean `Err` the CLI prints a
    // single tidy line and exits 1, which is what users expect.
    eyre::bail!("`edb test` is not yet implemented. Use `edb replay <tx-hash>` instead.")
}
