// EDB - Ethereum Debugger
// Copyright (C) 2024 Zhuo Zhang and Wuqi Zhang
// SPDX-License-Identifier: AGPL-3.0

//! Build a synthetic ForkResult + TxEnv that drives the engine through a
//! single test invocation. Fork-free variant only; the forked variant lands
//! in Phase 8.

use alloy_primitives::{Address, B256, Bytes, TxHash, U256, address, keccak256};
use edb_common::{EdbContext, EdbDB, ForkInfo, ForkResult};
use eyre::Result;
use revm::{
    Context, ExecuteEvm, MainBuilder, MainContext,
    context::TxEnv,
    database::{CacheDB, EmptyDB},
    primitives::{TxKind, hardfork::SpecId},
    state::{AccountInfo, Bytecode},
};

use crate::cmd::test::entrypoint::ENTRYPOINT_ADDR;

/// Foundry's CALLER constant — the EOA used for setUp / test calls in `forge test`.
#[allow(dead_code)] // consumed by downstream tasks (4.3+)
pub const FORGE_CALLER: Address = address!("1F1B3d8F17b59EE4e3fadf2BA936CDA90D9d6A92");

/// Foundry's cheatcode precompile address.
#[allow(dead_code)] // consumed by downstream tasks (5.2+)
pub const CHEATCODE_ADDRESS: Address = address!("7109709ECfa91a80626fF3989D68f67F5b1DD12D");

/// Sentinel bytecode etched at the cheatcode address. Execution never reaches it because
/// the cheatcodes inspector intercepts every CALL to this address in its `call` hook;
/// the bytecode just has to make plain `CALL` succeed instead of returning early
/// with "no code at address".
#[allow(dead_code)] // consumed by downstream tasks (5.2+)
pub const CHEATCODE_SENTINEL_BYTECODE: &[u8] = &[0xfe];

/// Default chain id for fork-free test sessions. Matches forge's default.
#[allow(dead_code)] // consumed by downstream tasks (4.3+)
pub const DEFAULT_FORK_FREE_CHAIN_ID: u64 = 31337;
#[allow(dead_code)] // consumed by downstream tasks (4.3+)
pub const DEFAULT_FORK_FREE_BLOCK: u64 = 1;
#[allow(dead_code)] // consumed by downstream tasks (4.3+)
pub const DEFAULT_FORK_FREE_TIMESTAMP: u64 = 1;

/// Build a deterministic synthetic tx hash for caching/identification.
#[allow(dead_code)] // consumed by downstream tasks (4.3+)
pub fn synthetic_tx_hash(contract: &str, test_fn: &str) -> TxHash {
    let mut buf = Vec::with_capacity(16 + contract.len() + 2 + test_fn.len());
    buf.extend_from_slice(b"edb-test:");
    buf.extend_from_slice(contract.as_bytes());
    buf.extend_from_slice(b"::");
    buf.extend_from_slice(test_fn.as_bytes());
    TxHash::from(keccak256(&buf).0)
}

/// Build a clean (fork-free) ForkResult containing:
/// - Empty in-memory DB
/// - `_EdbTestEntrypoint` etched at `ENTRYPOINT_ADDR`
/// - Cheatcode sentinel etched at `CHEATCODE_ADDRESS`
/// - `FORGE_CALLER` funded with U256::MAX
/// - Block env: chain_id=31337, block=1, ts=1, basefee=0, gas_limit=1_000_000_000
/// - Synthetic TxEnv: CALLER → entrypoint.run()
#[allow(dead_code)] // consumed by downstream tasks (4.3+)
pub fn build_clean_fork_result(
    entrypoint_bytecode: Bytes,
    contract_name: &str,
    test_fn: &str,
    run_selector: [u8; 4],
) -> Result<ForkResult<EdbDB<CacheDB<EmptyDB>>>> {
    let chain_id = DEFAULT_FORK_FREE_CHAIN_ID;
    let block_number = DEFAULT_FORK_FREE_BLOCK;
    let timestamp = DEFAULT_FORK_FREE_TIMESTAMP;
    let spec_id = SpecId::CANCUN;

    // Mirror the nesting from fork_and_prepare:
    //   EdbDB::new(CacheDB::new(inner)) → then wrap in outer CacheDB
    // EdbContext<DB> = Context<..., CacheDB<DB>>, so DB = EdbDB<CacheDB<EmptyDB>>
    let inner_db = EdbDB::new(CacheDB::new(EmptyDB::default()));
    let mut cache_db: CacheDB<EdbDB<CacheDB<EmptyDB>>> = CacheDB::new(inner_db);

    let entrypoint_code = Bytecode::new_raw(entrypoint_bytecode);
    cache_db.insert_account_info(ENTRYPOINT_ADDR, AccountInfo::from_bytecode(entrypoint_code));

    let cheat_bytes = Bytes::copy_from_slice(CHEATCODE_SENTINEL_BYTECODE);
    let cheat_code = Bytecode::new_raw(cheat_bytes);
    cache_db.insert_account_info(CHEATCODE_ADDRESS, AccountInfo::from_bytecode(cheat_code));

    cache_db.insert_account_info(FORGE_CALLER, AccountInfo::from_balance(U256::MAX));

    let ctx: EdbContext<EdbDB<CacheDB<EmptyDB>>> = Context::mainnet()
        .with_db(cache_db)
        .modify_block_chained(|b| {
            b.number = U256::from(block_number);
            b.timestamp = U256::from(timestamp);
            b.basefee = 0;
            b.gas_limit = 1_000_000_000;
            b.beneficiary = Address::ZERO;
            b.prevrandao = Some(B256::ZERO);
        })
        .modify_cfg_chained(|c| {
            c.chain_id = chain_id;
            c.spec = spec_id;
            c.disable_nonce_check = true;
        });

    let tx_env = TxEnv::builder()
        .caller(FORGE_CALLER)
        .kind(TxKind::Call(ENTRYPOINT_ADDR))
        .data(Bytes::copy_from_slice(&run_selector))
        .gas_limit(1_000_000_000)
        .chain_id(Some(chain_id))
        .nonce(0)
        .gas_price(0)
        .build()
        .map_err(|e| eyre::eyre!("TxEnv build failed: {e:?}"))?;

    // Build then finalize to extract the prepared context — mirrors fork_and_prepare.
    let mut evm = ctx.build_mainnet();
    evm.finalize();
    let context = evm.ctx;

    Ok(ForkResult {
        fork_info: ForkInfo { block_number, block_hash: B256::ZERO, timestamp, chain_id, spec_id },
        context,
        target_tx_env: tx_env,
        target_tx_hash: synthetic_tx_hash(contract_name, test_fn),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_synthetic_hash() {
        let h1 = synthetic_tx_hash("MyTest", "testFoo");
        let h2 = synthetic_tx_hash("MyTest", "testFoo");
        assert_eq!(h1, h2);
        let h3 = synthetic_tx_hash("MyTest", "testBar");
        assert_ne!(h1, h3);
        let h4 = synthetic_tx_hash("Other", "testFoo");
        assert_ne!(h1, h4);
    }

    #[test]
    fn forge_caller_constant_matches_spec() {
        assert_eq!(
            FORGE_CALLER,
            alloy_primitives::address!("1F1B3d8F17b59EE4e3fadf2BA936CDA90D9d6A92")
        );
    }

    #[test]
    fn cheatcode_address_matches_spec() {
        assert_eq!(
            CHEATCODE_ADDRESS,
            alloy_primitives::address!("7109709ECfa91a80626fF3989D68f67F5b1DD12D")
        );
    }
}
