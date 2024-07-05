use std::{
    fs::{self, File},
    io::Write,
    path::PathBuf,
};

use alloy_primitives::Address;
use eyre::Result;
use foundry_block_explorers::{
    contract::{Metadata, SourceCodeMetadata},
    errors::EtherscanError,
    Client,
};
use foundry_compilers::{
    artifacts::{output_selection::OutputSelection, CompilerOutput, SolcInput, Source, Sources},
    solc::{Solc, SolcLanguage},
};

use crate::{cache::Cache, etherscan_rate_limit_guard};

type Output = (Metadata, Sources, CompilerOutput);

#[derive(Debug, Clone)]
pub struct OnchainCompiler {
    pub cache: Cache<Output>,
}

impl OnchainCompiler {
    pub fn new(cache_root: impl Into<PathBuf>) -> Result<OnchainCompiler> {
        Ok(OnchainCompiler {
            cache: Cache::new(cache_root, None)?, // None for no expiry
        })
    }

    /// Compile the contract at the given address.
    /// Returns `Some`` if the contract is successfully compiled.
    /// Returns `None` if the contract is not verified, is a Vyper contract, or it is a Solidity
    /// 0.4.x contract which does not support --stand-json option.
    pub async fn compile(&self, etherscan: &Client, addr: Address) -> Result<Option<Output>> {
        // Get the cache_root. If not provided, use the default cache directory.
        if let Some(output) = self.cache.load_cache(addr.to_string()) {
            Ok(Some(output))
        } else {
            let mut meta =
                match etherscan_rate_limit_guard!(etherscan.contract_source_code(addr).await) {
                    Ok(meta) => meta,
                    Err(EtherscanError::ContractCodeNotVerified(_)) => {
                        // We do not cache if the contract is not verified.
                        return Ok(None);
                    }
                    Err(e) => return Err(e.into()),
                };
            eyre::ensure!(meta.items.len() == 1, "contract not found or ill-formed");
            let meta = meta.items.remove(0);

            if meta.is_vyper() {
                // We do not cache if the contract is a Vyper contract.
                return Ok(None);
            }

            // prepare the input for solc
            let mut settings = meta.settings()?;
            // enforce compiler output all possible outputs
            settings.output_selection = OutputSelection::complete_output_selection();
            trace!("using settings: {:?}", settings);

            // prepare the sources
            let sources = meta
                .sources()
                .into_iter()
                .map(|(k, v)| (k.into(), Source::new(v.content)))
                .collect();
            let input = SolcInput::new(SolcLanguage::Solidity, sources, settings);

            // prepare the compiler
            let version = meta.compiler_version()?;
            let compiler = Solc::find_or_install(&version)?;
            trace!("using compiler: {:?}", compiler);

            // compile the source code
            let output = match compiler.compile_exact(&input) {
                Ok(compiler_output) => (meta, input.sources, compiler_output),
                Err(_) if version.major == 0 && version.minor == 4 => {
                    return Ok(None);
                }
                Err(e) => {
                    return Err(eyre::eyre!("failed to compile contract: {}", e));
                }
            };

            self.cache.save_cache(addr.to_string(), &output)?;
            Ok(Some(output))
        }
    }
}
