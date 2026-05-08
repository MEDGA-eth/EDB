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

//! Forking tests.

use alloy_primitives::{U256, address, b256};
use alloy_rpc_types::Transaction;
use edb_common::{ForkInfo, get_tx_env_from_tx};
use revm::primitives::hardfork::SpecId;
use tracing::{debug, info};

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_fork_info_creation() {
    edb_common::ensure_test_logging(None);
    debug!("Testing ForkInfo struct creation and properties");

    let fork_info = ForkInfo {
        block_number: 12345678,
        block_hash: b256!("0000000000000000000000000000000000000000000000000000000000000000"),
        timestamp: 1640995200,
        chain_id: 1,
        spec_id: SpecId::LONDON,
    };

    assert_eq!(fork_info.block_number, 12345678);
    assert_eq!(fork_info.chain_id, 1);
    assert_eq!(fork_info.spec_id, SpecId::LONDON);
}

#[test]
fn test_get_tx_env_from_tx() {
    edb_common::ensure_test_logging(None);
    info!("Testing transaction environment extraction");

    // Create a mock transaction for testing
    let raw_tx = r#"{
        "blockHash": "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
        "blockNumber": "0xf4240",
        "from": "0x742d35cc6634c0532925a3b844bc9e7d62a7e6e5",
        "gas": "0x5208",
        "gasPrice": "0x3b9aca00",
        "hash": "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890",
        "input": "0x",
        "nonce": "0x0",
        "to": "0x7be8076f4ea4a4ad08075c2508e481d6c946d12b",
        "transactionIndex": "0x0",
        "value": "0xde0b6b3a7640000",
        "type": "0x0",
        "chainId": "0x1",
        "v": "0x1b",
        "r": "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
        "s": "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890"
    }"#;

    let tx: Transaction = serde_json::from_str(raw_tx).expect("valid transaction JSON");
    let chain_id = 1u64;

    let result = get_tx_env_from_tx(&tx, chain_id);

    match result {
        Ok(tx_env) => {
            assert_eq!(tx_env.chain_id, Some(chain_id));
            assert_eq!(tx_env.nonce, 0);
            assert_eq!(tx_env.gas_limit, 0x5208); // 21000 in hex
            assert_eq!(tx_env.value, U256::from(0xde0b6b3a7640000u64)); // 1 ETH in wei
            assert!(tx_env.data.is_empty());
        }
        Err(e) => {
            panic!("Failed to convert transaction to TxEnv: {e:?}");
        }
    }
}

#[test]
fn test_get_tx_env_contract_creation() {
    // Test with a contract creation transaction (no "to" field)
    let raw_tx = r#"{
        "blockHash": "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
        "blockNumber": "0xf4240",
        "from": "0x742d35cc6634c0532925a3b844bc9e7d62a7e6e5",
        "gas": "0x186a0",
        "gasPrice": "0x3b9aca00",
        "hash": "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890",
        "input": "0x606060405260008054600160a060020a03191633179055",
        "nonce": "0x1",
        "to": null,
        "transactionIndex": "0x0",
        "value": "0x0",
        "type": "0x0",
        "chainId": "0x1",
        "v": "0x1b",
        "r": "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
        "s": "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890"
    }"#;

    let tx: Transaction = serde_json::from_str(raw_tx).expect("valid transaction JSON");
    let chain_id = 1u64;

    let result = get_tx_env_from_tx(&tx, chain_id);

    match result {
        Ok(tx_env) => {
            // Verify it's a contract creation
            assert!(matches!(tx_env.kind, alloy_primitives::TxKind::Create));
            assert_eq!(tx_env.nonce, 1);
            assert_eq!(tx_env.gas_limit, 0x186a0); // 100000 in hex
            assert_eq!(tx_env.value, U256::ZERO);
            assert!(!tx_env.data.is_empty()); // Contract bytecode
        }
        Err(e) => {
            panic!("Failed to convert contract creation tx to TxEnv: {e:?}");
        }
    }
}

#[test]
fn test_get_tx_env_with_access_list() {
    // Test with EIP-2930 transaction that includes access list
    let raw_tx = r#"{
        "blockHash": "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
        "blockNumber": "0xf4240",
        "from": "0x742d35cc6634c0532925a3b844bc9e7d62a7e6e5",
        "gas": "0x5208",
        "gasPrice": "0x3b9aca00",
        "maxFeePerGas": "0x77359400",
        "maxPriorityFeePerGas": "0x59682f00",
        "hash": "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890",
        "input": "0x",
        "nonce": "0x2",
        "to": "0x7be8076f4ea4a4ad08075c2508e481d6c946d12b",
        "transactionIndex": "0x0",
        "value": "0x0",
        "type": "0x2",
        "accessList": [
            {
                "address": "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2",
                "storageKeys": [
                    "0x0000000000000000000000000000000000000000000000000000000000000000",
                    "0x0000000000000000000000000000000000000000000000000000000000000001"
                ]
            }
        ],
        "chainId": "0x1",
        "v": "0x1",
        "r": "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
        "s": "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890"
    }"#;

    let tx: Transaction = serde_json::from_str(raw_tx).expect("valid transaction JSON");
    let chain_id = 1u64;

    let result = get_tx_env_from_tx(&tx, chain_id);

    match result {
        Ok(tx_env) => {
            assert_eq!(tx_env.chain_id, Some(chain_id));
            assert_eq!(tx_env.nonce, 2);

            // Check that access list is properly converted
            let access_list = tx_env.access_list;
            assert_eq!(access_list.len(), 1);
            assert_eq!(
                access_list[0].address,
                address!("c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2")
            );
            assert_eq!(access_list[0].storage_keys.len(), 2);
        }
        Err(e) => {
            panic!("Failed to convert EIP-2930 tx to TxEnv: {e:?}");
        }
    }
}

// Tests moved to crates/integration-tests/tests/forking_with_proxy_tests.rs
// These tests now use cached proxy for better reliability and performance
