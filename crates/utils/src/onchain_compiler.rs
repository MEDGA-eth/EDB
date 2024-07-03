use std::{
    fs::{self, File},
    io::Write,
    path::PathBuf,
};

use alloy_primitives::Address;
use eyre::Result;
use foundry_block_explorers::{contract::Metadata, errors::EtherscanError, Client};
use foundry_compilers::{
    artifacts::{output_selection::OutputSelection, CompilerOutput, SolcInput, Source, Sources},
    solc::{Solc, SolcLanguage},
};

use crate::etherscan_rate_limit_guard;

/// Compile the contract at the given address.
/// Returns `Some`` if the contract is successfully compiled.
/// Returns `None` if the contract is not verified, is a Vyper contract, or it is a Solidity 0.4.x
/// contract which does not support --stand-json option.
pub async fn compile(
    etherscan: &Client,
    addr: Address,
    cache_root: &PathBuf,
) -> Result<Option<(Metadata, Sources, CompilerOutput)>> {
    // Get the cache_root. If not provided, use the default cache directory.
    fs::create_dir_all(&cache_root)?;

    let cache_json = cache_root.join(format!("{}.json", addr));

    if cache_json.exists() {
        println!("GET CACHE!");
        let output = serde_json::from_str(&fs::read_to_string(&cache_json)?)?;
        Ok(output)
    } else {
        println!("MISS CACHE!");
        let mut meta = match etherscan_rate_limit_guard!(etherscan.contract_source_code(addr).await)
        {
            Ok(meta) => meta,
            Err(EtherscanError::ContractCodeNotVerified(_)) => {
                let output = None;
                cache_output(cache_json, &output)?;
                return Ok(output);
            }
            Err(e) => return Err(e.into()),
        };
        eyre::ensure!(meta.items.len() == 1, "contract not found or ill-formed");
        let meta = meta.items.remove(0);
        if meta.is_vyper() {
            let output = None;
            cache_output(cache_json, &output)?;
            return Ok(output);
        }

        // prepare the input for solc
        let mut settings = meta.settings()?;
        // enforce compiler output all possible outputs
        settings.output_selection = OutputSelection::complete_output_selection();
        let sources =
            meta.sources().into_iter().map(|(k, v)| (k.into(), Source::new(v.content))).collect();
        let input = SolcInput::new(SolcLanguage::Solidity, sources, settings);

        // prepare the compiler
        let version = meta.compiler_version()?;
        let compiler = Solc::find_or_install(&version)?;

        // compile the source code
        let output = match compiler.compile_exact(&input) {
            Ok(compiler_output) => Some((meta, input.sources, compiler_output)),
            Err(_) if version.major == 0 && version.minor == 4 => None,
            Err(e) => {
                return Err(eyre::eyre!("failed to compile contract: {}", e));
            }
        };

        cache_output(cache_json, &output)?;
        Ok(output)
    }
}

fn cache_output(
    cache_json: PathBuf,
    output: &Option<(Metadata, Sources, CompilerOutput)>,
) -> Result<()> {
    let serialized = serde_json::to_string(output)?;
    let mut file = File::create(cache_json)?;
    file.write_all(serialized.as_bytes())?;

    Ok(())
}
