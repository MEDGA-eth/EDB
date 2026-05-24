// EDB - Ethereum Debugger
// Copyright (C) 2024 Zhuo Zhang and Wuqi Zhang
// SPDX-License-Identifier: AGPL-3.0

//! Build a synthetic ForkResult + TxEnv that drives the engine through a
//! single test invocation. Both fork-free and forked (ProviderDb) variants.

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

/// Foundry's `DEFAULT_SENDER` — the EOA used as `msg.sender` for setUp/test calls
/// when no `vm.prank` is active. Derived as
/// `address(uint160(uint256(keccak256("foundry default caller"))))`, matching
/// forge-std's `StdConstants.sol::DEFAULT_SENDER`.
///
/// Previously this constant was set to `0x1F1B3d8F17b59EE4e3fadf2BA936CDA90D9d6A92`,
/// which is the deterministic-deploy address of the test contract INSTANCE under
/// `forge test` — not the test caller. That divergence broke any test reading
/// `tx.origin` / `msg.sender` (access-control checks, CREATE2 derivations,
/// EIP-712 signature flows). The unit test
/// `forge_caller_matches_foundry_default_sender` re-derives the constant at
/// test time so silent drift is caught immediately.
#[allow(dead_code)] // consumed by downstream tasks (4.3+)
pub const FORGE_CALLER: Address = address!("1804c8AB1F12E6bbf3894d4083f33e07309d1f38");

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
/// Default block number for fork-free test sessions.
#[allow(dead_code)] // consumed by downstream tasks (4.3+)
pub const DEFAULT_FORK_FREE_BLOCK: u64 = 1;
/// Default block timestamp for fork-free test sessions.
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
/// - Each `libs_to_etch` library's linked deployed bytecode etched at its
///   forge-computed address (and `LIBRARY_DEPLOYER`'s nonce bumped to match)
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
    libs_to_etch: &[(Address, Bytes)],
) -> Result<ForkResult<EdbDB<CacheDB<EmptyDB>>>> {
    use revm::Database;
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

    // Etch each external library's linked deployed bytecode at the forge-computed
    // address (`LIBRARY_DEPLOYER.create(nonce)`). Without this the linked bytecode
    // would jump to empty accounts and revert.
    for (lib_addr, lib_code) in libs_to_etch {
        cache_db.insert_account_info(
            *lib_addr,
            AccountInfo::from_bytecode(Bytecode::new_raw(lib_code.clone())),
        );
    }
    // Cosmetic: make LIBRARY_DEPLOYER's nonce match a real forge run (it deploys
    // each library via CREATE at sequential nonces).
    if !libs_to_etch.is_empty() {
        let dep = crate::cmd::test::link::LIBRARY_DEPLOYER;
        let mut info = cache_db.basic(dep)?.unwrap_or_default();
        info.nonce = libs_to_etch.len() as u64;
        cache_db.insert_account_info(dep, info);
    }

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
            // Relax EVM size constraints so test contracts larger than the
            // mainnet EIP-170 (24 kB deployed) or EIP-3860 (49 kB initcode)
            // limits deploy cleanly in the initial replay pass, matching
            // forge's own behaviour and the snapshot-pass relaxation applied
            // by `relax_evm_context_constraints`.
            c.limit_contract_code_size = Some(usize::MAX);
            c.limit_contract_initcode_size = Some(usize::MAX);
            c.disable_base_fee = true;
            c.disable_block_gas_limit = true;
            // EIP-3607 rejects txs from senders with deployed code. Foundry's
            // `configure_env` disables it for the same reason — tests routinely
            // `vm.etch(caller, ...)` to install helpers at FORGE_CALLER, and on
            // forked state the caller's mainnet account may already carry code.
            c.disable_eip3607 = true;
            // EDB diverges from foundry by also disabling the balance check:
            // multi-pass instrumentation can inflate gas usage past what the
            // test setUp's balance funds, and we want passes to be balance-
            // invariant. Tests intentionally exercising "insufficient balance"
            // semantics will silently succeed here but fail under `forge test`.
            c.disable_balance_check = true;
            c.tx_gas_limit_cap = Some(u64::MAX);
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

/// Build a forked ForkResult backed by an upstream RPC via `ProviderDb`.
///
/// The DB nesting matches `fork_and_prepare` in `edb_common::forking`:
///   `CacheDB<EdbDB<CacheDB<Arc<WrapDatabaseAsync<ProviderDb<..>>>>>>`.
///
/// Pre-stages the same accounts as `build_clean_fork_result` on top of the
/// forked state (entrypoint, cheatcode sentinel, funded FORGE_CALLER).
#[allow(dead_code)]
pub async fn build_forked_fork_result(
    entrypoint_bytecode: Bytes,
    contract_name: &str,
    test_fn: &str,
    run_selector: [u8; 4],
    libs_to_etch: &[(Address, Bytes)],
    upstream_rpc: &str,
    fork_block_number: Option<u64>,
) -> Result<
    ForkResult<
        impl revm::Database<Error = edb_common::EdbDBError>
        + revm::DatabaseCommit
        + revm::DatabaseRef<Error = edb_common::EdbDBError>
        + Clone
        + Send
        + Sync
        + 'static,
    >,
> {
    use alloy_provider::{Provider, ProviderBuilder};
    use alloy_rpc_types::BlockNumberOrTag;
    use edb_common::{get_blob_base_fee_update_fraction_by_spec_id, get_mainnet_spec_id};
    use revm::{
        Database, context_interface::block::BlobExcessGasAndPrice,
        database_interface::WrapDatabaseAsync,
    };
    use std::sync::Arc;

    let provider = ProviderBuilder::new().connect(upstream_rpc).await?;
    let chain_id = provider.get_chain_id().await?;

    let block_tag = match fork_block_number {
        Some(n) => BlockNumberOrTag::Number(n),
        None => BlockNumberOrTag::Latest,
    };
    let block = provider
        .get_block_by_number(block_tag)
        .full()
        .await?
        .ok_or_else(|| eyre::eyre!("fork block not available from upstream RPC"))?;
    let block_number = block.header.number;
    let spec_id = get_mainnet_spec_id(block_number);

    let alloy_db = edb_common::provider_db::ProviderDb::new(provider, block_number.into());
    let state_db = WrapDatabaseAsync::new(alloy_db).ok_or_else(|| {
        eyre::eyre!(
            "Cannot create WrapDatabaseAsync: build_forked_fork_result must run \
             inside a multi-threaded Tokio runtime."
        )
    })?;

    // Mirror fork_and_prepare's nesting: EdbDB<CacheDB<Arc<WrapDatabaseAsync<...>>>>
    let edb_db = EdbDB::new(CacheDB::new(Arc::new(state_db)));
    let mut db: CacheDB<_> = CacheDB::new(edb_db);

    // Pre-stage entrypoint
    let entrypoint_code = Bytecode::new_raw(entrypoint_bytecode);
    db.insert_account_info(
        crate::cmd::test::entrypoint::ENTRYPOINT_ADDR,
        AccountInfo::from_bytecode(entrypoint_code),
    );

    // Etch each external library's linked deployed bytecode at the forge-computed
    // address (`LIBRARY_DEPLOYER.create(nonce)`). Without this the linked bytecode
    // would jump to empty accounts and revert.
    for (lib_addr, lib_code) in libs_to_etch {
        db.insert_account_info(
            *lib_addr,
            AccountInfo::from_bytecode(Bytecode::new_raw(lib_code.clone())),
        );
    }
    // Cosmetic: make LIBRARY_DEPLOYER's nonce match a real forge run (it deploys
    // each library via CREATE at sequential nonces).
    if !libs_to_etch.is_empty() {
        let dep = crate::cmd::test::link::LIBRARY_DEPLOYER;
        let mut info = db.basic(dep)?.unwrap_or_default();
        info.nonce = libs_to_etch.len() as u64;
        db.insert_account_info(dep, info);
    }

    // Pre-stage cheatcode sentinel
    let cheat_bytes = Bytes::copy_from_slice(CHEATCODE_SENTINEL_BYTECODE);
    let cheat_code = Bytecode::new_raw(cheat_bytes);
    db.insert_account_info(CHEATCODE_ADDRESS, AccountInfo::from_bytecode(cheat_code));

    // Fund FORGE_CALLER — preserve live nonce/code but overwrite balance.
    let mut caller_info = db.basic(FORGE_CALLER)?.unwrap_or_default();
    caller_info.balance = U256::MAX;
    db.insert_account_info(FORGE_CALLER, caller_info);

    let ctx = Context::mainnet()
        .with_db(db)
        .modify_block_chained(|b| {
            b.number = U256::from(block_number);
            b.timestamp = U256::from(block.header.timestamp);
            b.basefee = block.header.base_fee_per_gas.unwrap_or_default();
            b.difficulty = block.header.difficulty;
            b.gas_limit = block.header.gas_limit;
            b.prevrandao = Some(block.header.mix_hash);
            b.beneficiary = block.header.beneficiary;
            b.blob_excess_gas_and_price = block.header.excess_blob_gas.map(|g| {
                BlobExcessGasAndPrice::new(g, get_blob_base_fee_update_fraction_by_spec_id(spec_id))
            });
        })
        .modify_cfg_chained(|c| {
            c.chain_id = chain_id;
            c.spec = spec_id;
            c.disable_nonce_check = true;
            // Same size-limit relaxation as the non-fork path (see
            // build_clean_fork_result) — test contracts can be larger than
            // the mainnet EIP-170/EIP-3860 thresholds.
            c.limit_contract_code_size = Some(usize::MAX);
            c.limit_contract_initcode_size = Some(usize::MAX);
            c.disable_base_fee = true;
            c.disable_block_gas_limit = true;
            // See build_clean_fork_result for the rationale on these flags.
            c.disable_eip3607 = true;
            c.disable_balance_check = true;
            c.tx_gas_limit_cap = Some(u64::MAX);
        });

    let tx_env = TxEnv::builder()
        .caller(FORGE_CALLER)
        .kind(TxKind::Call(crate::cmd::test::entrypoint::ENTRYPOINT_ADDR))
        .data(Bytes::copy_from_slice(&run_selector))
        .gas_limit(1_000_000_000)
        .chain_id(Some(chain_id))
        .nonce(0)
        .gas_price(0)
        .build()
        .map_err(|e| eyre::eyre!("TxEnv build failed: {e:?}"))?;

    let mut evm = ctx.build_mainnet();
    evm.finalize();
    let context = evm.ctx;

    Ok(ForkResult {
        fork_info: ForkInfo {
            block_number,
            block_hash: block.header.hash,
            timestamp: block.header.timestamp,
            chain_id,
            spec_id,
        },
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
        // Pinned to the forge-std DEFAULT_SENDER literal so the byte sequence
        // is explicit at the test site (any silent drift in the const flips
        // this assertion immediately).
        assert_eq!(
            FORGE_CALLER,
            alloy_primitives::address!("1804c8AB1F12E6bbf3894d4083f33e07309d1f38")
        );
    }

    /// Re-derive `FORGE_CALLER` from its definition at test time so a future
    /// edit that flips the byte literal away from the keccak("foundry default
    /// caller") derivation can't silently sneak past review.
    ///
    /// forge-std/src/StdConstants.sol:
    ///   `address(uint160(uint256(keccak256("foundry default caller"))))`
    #[test]
    fn forge_caller_matches_foundry_default_sender() {
        let derived = Address::from_word(keccak256(b"foundry default caller"));
        assert_eq!(
            FORGE_CALLER, derived,
            "FORGE_CALLER must equal address(uint160(uint256(keccak256(\"foundry default caller\")))) \
             — i.e. forge-std's StdConstants.DEFAULT_SENDER. Got {FORGE_CALLER} vs derived {derived}",
        );
    }

    #[test]
    fn cheatcode_address_matches_spec() {
        assert_eq!(
            CHEATCODE_ADDRESS,
            alloy_primitives::address!("7109709ECfa91a80626fF3989D68f67F5b1DD12D")
        );
    }

    /// Regression for C2-5 (Round 2 audit): the clean-fork cfg must set
    /// `disable_eip3607 = true` so foundry-style tests where the caller has
    /// deployed code (via `vm.etch(caller, ...)` or, on forks, a contract
    /// already deployed at the address) don't get rejected with
    /// `EIP3607Rejected` by REVM. Mirrors foundry's `configure_env`.
    #[test]
    fn clean_fork_disables_eip3607_balance_and_block_gas() {
        let fork = build_clean_fork_result(
            Bytes::from_static(&[0x60, 0x00, 0x60, 0x00, 0xf3]), // tiny PUSH1 0 PUSH1 0 RETURN
            "Foo",
            "testBar",
            [0x12, 0x34, 0x56, 0x78],
            &[],
        )
        .expect("build_clean_fork_result");
        let cfg = &fork.context.cfg;
        assert!(cfg.disable_eip3607, "EIP-3607 must be disabled (matches forge's configure_env)");
        assert!(cfg.disable_balance_check, "balance check disabled (multi-pass instrumentation)");
        assert!(cfg.disable_block_gas_limit, "block gas limit disabled");
        assert!(cfg.disable_nonce_check, "nonce check disabled");
    }
}
