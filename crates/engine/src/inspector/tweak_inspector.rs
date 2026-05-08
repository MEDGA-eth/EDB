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

//! Contract deployment inspector for replacing creation bytecode
//!
//! This inspector intercepts contract creation calls and can replace the init code
//! with custom bytecode when the deployment would create a specific target address.

use alloy_dyn_abi::JsonAbiExt;
use alloy_primitives::{Address, Bytes, U256};
use edb_common::EdbContext;
use eyre::Result;
use foundry_compilers::{Artifact as _, artifacts::Contract};
use itertools::Itertools;
use revm::{
    Database, DatabaseCommit, DatabaseRef, Inspector,
    bytecode::OpCode,
    context::{CreateScheme, JournalTr},
    database::CacheDB,
    interpreter::{CreateInputs, CreateOutcome, Interpreter},
};
use tracing::{debug, error, info, warn};

use crate::{
    inspector::utils::relax_gas_limit_at_callsite,
    utils::disasm::{disassemble, extract_push_value},
};

static CONSTRUCTOR_ARG_SEARCH_RANGE: usize = 1024;
/// Inspector that intercepts and modifies contract deployments
#[derive(Debug)]
pub struct TweakInspector<'a> {
    /// Target address we're looking for
    target_address: Address,

    /// Original init code (contract creation bytecode)
    contract: &'a Contract,

    /// Custom init code to use (contract creation bytecode)
    recompiled_contract: &'a Contract,

    /// Constructor arguments to append to init code
    constructor_args: &'a Bytes,

    /// The deployed bytecode we captured (filled after successful deployment)
    deployed_code: Option<Bytes>,

    /// Whether we found and replaced the target deployment
    found_target: bool,
}

impl<'a> TweakInspector<'a> {
    /// Create a new deployment inspector
    pub fn new(
        target_address: Address,
        contract: &'a Contract,
        recompiled_contract: &'a Contract,
        constructor_args: &'a Bytes,
    ) -> Self {
        Self {
            target_address,
            contract,
            recompiled_contract,
            constructor_args,
            deployed_code: None,
            found_target: false,
        }
    }

    /// Get the deployed bytecode if the target was found and deployed
    pub fn deployed_code(&self) -> Option<&Bytes> {
        self.deployed_code.as_ref()
    }

    /// Check if the target deployment was found
    pub fn found_target(&self) -> bool {
        self.found_target
    }

    /// Generate the deployed code
    pub fn into_deployed_code(self) -> Result<Bytes> {
        self.deployed_code.ok_or(eyre::eyre!("No deployed code found"))
    }

    /// Extract constructor arguments from the actual init code
    /// This tries multiple strategies to extract the constructor arguments
    fn extract_constructor_args(&self, init_code: &Bytes) -> Option<Bytes> {
        // Early check: whether the constructor has arguments
        if self
            .contract
            .abi
            .as_ref()
            .and_then(|abi| abi.constructor.as_ref())
            .map(|c| c.inputs.is_empty())
            .unwrap_or(true)
        {
            // If there is no constructor information, we assume Etherscan is correct.
            debug!("No constructor args needed, using bytes from Etherscan");
            return Some(self.constructor_args.clone());
        }

        // Strategy 1: If constructor_args from Etherscan is not empty, use it
        if !self.constructor_args.is_empty() {
            debug!("Using constructor args from Etherscan: {} bytes", self.constructor_args.len());
            return Some(self.constructor_args.clone());
        }

        // Strategy 2: Check if original_init_code is an exact prefix of init_code
        let original_creation_code = self.contract.get_bytecode_bytes()?;
        if init_code.len() >= original_creation_code.len() {
            let prefix = &init_code[..original_creation_code.len()];
            if prefix == original_creation_code.as_ref() {
                // Extract constructor args from the suffix
                let constructor_args = &init_code[original_creation_code.len()..];
                debug!(
                    "Original init code is exact prefix, extracted constructor args: {} bytes",
                    constructor_args.len()
                );
                return Some(Bytes::from(constructor_args.to_vec()));
            }
        }

        // Strategy 3: Fallback - heuristically extract constructor args
        // Assume constructor args start around original_init_code length and try to
        // use constroctor abi to decode them
        if init_code.len() > original_creation_code.len() {
            let constructor_args = &init_code[original_creation_code.len()..];
            debug!(
                "Using heuristic extraction for constructor args: {} bytes",
                constructor_args.len()
            );

            // At this point, we must have constructor args
            let k = original_creation_code.len();
            for i in (0..=CONSTRUCTOR_ARG_SEARCH_RANGE)
                .flat_map(move |d| {
                    [k.saturating_add(d).min(init_code.len() - 1), k.saturating_sub(d)]
                })
                .unique()
            {
                if self.can_use_as_constructor_args(&init_code[i..]) {
                    debug!(
                        "Successfully extracted constructor args: {} bytes",
                        init_code[i..].len()
                    );
                    return Some(Bytes::from(init_code[i..].to_vec()));
                }
            }
        }

        // Strategy 4: Try to extract constructor args using the K pattern algorithm
        if let Some(constructor_args) = self.extract_constructor_args_with_k_pattern(init_code) {
            debug!("Extracted constructor args using K pattern: {} bytes", constructor_args.len());
            return Some(constructor_args);
        }

        // No constructor args found
        warn!("Could not extract constructor args");
        None
    }

    /// Check whether the given bytes can be used as the constructor arguments
    fn can_use_as_constructor_args(&self, data: &[u8]) -> bool {
        let Some(constructor) = self.contract.abi.as_ref().and_then(|abi| abi.constructor.as_ref())
        else {
            return false;
        };

        if let Ok(decoded) = constructor.abi_decode_input(data) {
            constructor
                .abi_encode_input(&decoded)
                .ok()
                .map(|encoded| encoded == data)
                .unwrap_or(false)
        } else {
            false
        }
    }

    /// Extract constructor arguments using the K pattern from Solidity's init code
    /// The pattern is: PUSHn <K> CODESIZE SUB ... PUSHn <K> ... CODECOPY
    /// where K is the offset where [runtime_code][constructor_args] starts
    fn extract_constructor_args_with_k_pattern(&self, init_code: &Bytes) -> Option<Bytes> {
        // Disassemble the init code
        let disasm = disassemble(init_code);

        // Find all potential K values by looking for the complete pattern
        let mut k_candidates = Vec::new();

        for (i, inst) in disasm.instructions.iter().enumerate() {
            // Look for the pattern: PUSHn <K> CODESIZE SUB
            if !inst.is_push() {
                continue;
            }

            // Check whether K value is valid
            let Some(k_value) = extract_push_value(inst) else { continue };
            if k_value >= U256::from(init_code.len()) {
                continue;
            }

            // Check if this is followed by CODESIZE SUB
            //
            // CODESIZE check
            let Some(codesize_inst) = disasm.instructions.get(i + 1) else { continue };
            if codesize_inst.opcode != OpCode::CODESIZE {
                continue;
            }

            // SUB check
            let Some(sub_inst) = disasm.instructions.get(i + 2) else { continue };
            if sub_inst.opcode != OpCode::SUB {
                continue;
            }

            // Now verify this K appears again before CODECOPY
            // Look ahead for CODECOPY (0x39)
            for j in (i + 3)..(i + CONSTRUCTOR_ARG_SEARCH_RANGE) {
                // CODECOPY
                if disasm.instructions[j].opcode != OpCode::CODECOPY {
                    continue;
                }

                // Check if the same K value appears before CODECOPY
                // CODECOPY typically has pattern: PUSHn <K> ... CODECOPY
                if let Some(push_before_codecopy) = disasm.instructions.get(j - 2)
                    && push_before_codecopy.is_push()
                {
                    let Some(k2) = extract_push_value(push_before_codecopy) else { continue };
                    if k2 == k_value {
                        k_candidates.push(k_value);
                        debug!("Found confirmed K value with full pattern: {}", k_value);
                        break;
                    }
                }

                // Also check j-1 position for direct PUSH K CODECOPY
                if let Some(push_before_codecopy) = disasm.instructions.get(j - 1)
                    && push_before_codecopy.is_push()
                {
                    let Some(k2) = extract_push_value(push_before_codecopy) else { continue };
                    if k2 == k_value {
                        k_candidates.push(k_value);
                        debug!("Found confirmed K value with full pattern: {}", k_value);
                        break;
                    }
                }
            }
        }

        if k_candidates.is_empty() {
            debug!("No K candidates found with complete pattern in init code");
            return None;
        }

        for k_value in k_candidates {
            // Use the first confirmed K value (they should all be the same if valid)
            let Ok(k) = TryInto::<usize>::try_into(k_value) else { continue };
            debug!("Using confirmed K value: {}", k);

            // Extract the tail from position K
            if k >= init_code.len() {
                continue;
            }

            let tail = &init_code[k..];

            // The tail contains [runtime_code][constructor_args]
            // We need to determine where runtime ends and constructor args begin

            // If we have the deployed code, we can use its length
            let Some(deployed) = self
                .contract
                .evm
                .as_ref()
                .and_then(|e| e.deployed_bytecode.as_ref())
                .and_then(|d| d.bytes())
            else {
                continue;
            };

            let runtime_len = deployed.len();
            if tail.len() > runtime_len {
                let constructor_args = &tail[runtime_len..];
                if self.can_use_as_constructor_args(constructor_args) {
                    return Some(Bytes::from(constructor_args.to_vec()));
                }
            }
        }

        // Final fallback: assume no constructor args if we can't determine the split
        warn!("Could not determine runtime/constructor args split in tail");
        None
    }

    /// Combine init code with constructor arguments
    fn get_full_init_code(&self, init_code: &Bytes) -> Option<Bytes> {
        // Extract constructor arguments using various strategies.
        // If we cannot extract a valid one, we trust Etherscan.
        let constructor_args =
            self.extract_constructor_args(init_code).unwrap_or(self.constructor_args.clone());

        // Simply concatenate recompiled init code with constructor args
        let Some(recompiled_creation_code) = self.recompiled_contract.get_bytecode_bytes() else {
            error!("Failed to get recompiled creation code for {}", self.target_address);
            return None;
        };

        let mut full_code = recompiled_creation_code.to_vec();
        full_code.extend_from_slice(&constructor_args);

        debug!(
            "Created full init code: {} bytes (init: {}, args: {})",
            full_code.len(),
            recompiled_creation_code.len(),
            constructor_args.len()
        );

        Some(Bytes::from(full_code))
    }
}

impl<DB> Inspector<EdbContext<DB>> for TweakInspector<'_>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone,
    <CacheDB<DB> as Database>::Error: Clone,
    <DB as Database>::Error: Clone,
{
    fn step(&mut self, interp: &mut Interpreter, _ctx: &mut EdbContext<DB>) {
        relax_gas_limit_at_callsite(interp);
    }

    fn create(
        &mut self,
        context: &mut EdbContext<DB>,
        inputs: &mut CreateInputs,
    ) -> Option<CreateOutcome> {
        // Get the nonce from the caller account
        let account = context.journaled_state.load_account(inputs.caller()).ok()?;
        let nonce = account.info.nonce;

        // Calculate what address would be created using the built-in method
        let predicted_address = inputs.created_address(nonce);

        debug!(
            "CREATE intercepted: deployer={:?}, predicted={:?}, target={:?}",
            inputs.caller(),
            predicted_address,
            self.target_address
        );

        // Check if this is our target deployment
        if predicted_address == self.target_address {
            info!(
                "Found target deployment! Replacing init code for address {:?}",
                self.target_address
            );

            self.found_target = true;

            // Replace the init code with our custom code + constructor args
            inputs.set_init_code(self.get_full_init_code(inputs.init_code()).unwrap_or_default());

            // Force the address to be our target (in case of any calculation differences)
            // Convert to Custom scheme to ensure the exact address
            inputs.set_scheme(CreateScheme::Custom { address: self.target_address });
        }

        // Continue with normal execution
        None
    }

    fn create_end(
        &mut self,
        _context: &mut EdbContext<DB>,
        inputs: &CreateInputs,
        outcome: &mut CreateOutcome,
    ) {
        // Check if this was our target deployment and it succeeded
        if self.found_target
            && matches!(inputs.scheme(), CreateScheme::Custom { address } if address == self.target_address)
        {
            if outcome.result.is_ok() {
                // Get the deployed bytecode from the context
                if let Some(created_address) = outcome.address
                    && created_address == self.target_address
                {
                    // Get deployed code from outcome's output (runtime bytecode)
                    // self.deployed_code =
                    //     context.load_account_code(created_address).map(|c| c.data.clone());
                    self.deployed_code = Some(outcome.result.output.clone());
                    info!(
                        "Successfully captured deployed bytecode for {:?}: {} bytes",
                        self.target_address,
                        outcome.result.output.len()
                    );
                }
            } else {
                error!(
                    "Target deployment failed for {:?}: {:?} ({:?}, Bytes: {}, Address: {:?})",
                    self.target_address,
                    outcome.result.result,
                    outcome.result.gas,
                    outcome.result.output.len(),
                    outcome.address
                );
            }
        }
    }
}
