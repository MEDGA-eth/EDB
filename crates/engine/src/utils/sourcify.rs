use alloy_primitives::Address;
use alloy_transport_http::reqwest::Client;
use eyre::{Context, Result};
use foundry_block_explorers::contract::Metadata as EtherscanMetadata;
use foundry_compilers::{
    artifacts::{output_selection::OutputSelection, CompilerOutput, SolcInput, Source, Sources},
    solc::SolcLanguage,
};
use semver::Version;
use serde_json::Value;
use tracing::{debug, warn};

use crate::{find_or_install_solc, Artifact};

/// Try to fetch an artifact from Sourcify (full_match then partial_match).
///
/// Returns Ok(Some(Artifact)) if successful, Ok(None) if not found, Err on fatal errors.
pub async fn fetch_artifact_from_sourcify(
    chain_id: u64,
    address: Address,
) -> Result<Option<Artifact>> {
    let addr_str = format!("{address:#x}");
    let client = Client::new();
    // full_match first, then partial_match
    for kind in ["full_match", "partial_match"] {
        // Use the Sourcify server API endpoint directly to avoid 307 redirects
        // repo.sourcify.dev redirects to sourcify.dev/server/repository
        let base = format!(
            "https://sourcify.dev/server/repository/contracts/{}/{}/{}/",
            kind, chain_id, addr_str
        );
        let meta_url = format!("{base}metadata.json");
        let meta_resp = client.get(&meta_url).send().await?;
        if !meta_resp.status().is_success() {
            debug!("sourcify {kind} not found for {addr_str}");
            continue;
        }
        let metadata_json: Value = meta_resp.json().await.context("parse metadata")?;
        let compiler_version = metadata_json
            .get("compiler")
            .and_then(|c| c.get("version"))
            .and_then(Value::as_str)
            .ok_or_else(|| eyre::eyre!("sourcify metadata missing compiler.version"))?
            .to_string();
        let settings_val = metadata_json
            .get("settings")
            .cloned()
            .ok_or_else(|| eyre::eyre!("sourcify metadata missing settings"))?;
        let mut settings: foundry_compilers::artifacts::Settings =
            serde_json::from_value(settings_val).context("parse settings")?;
        settings.output_selection = OutputSelection::complete_output_selection();

        let sources_json = metadata_json
            .get("sources")
            .and_then(Value::as_object)
            .ok_or_else(|| eyre::eyre!("sourcify metadata missing sources"))?;

        // Fetch all sources listed in metadata.sources
        let mut sources: Sources = Sources::new();
        let mut source_code_entries: serde_json::Map<String, Value> = serde_json::Map::new();
        for (path, info) in sources_json {
            let path_str = path.to_string();
            let url = info
                .get("urls")
                .and_then(Value::as_array)
                .and_then(|arr| arr.first())
                .and_then(Value::as_str)
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("{base}sources/{path_str}"));
            // prefer repo source path over swarm/ipfs URLs
            let source_url = if url.starts_with("bzzr://")
                || url.starts_with("bzz-raw://")
                || url.starts_with("ipfs://")
                || url.starts_with("dweb://")
                || !url.starts_with("http")
            {
                format!("{base}sources/{path_str}")
            } else {
                url
            };
            let src_resp = client.get(&source_url).send().await?;
            if !src_resp.status().is_success() {
                warn!("sourcify source fetch failed for {path_str} ({addr_str})");
                return Ok(None);
            }
            let content = src_resp.text().await?;
            sources.insert(path_str.clone().into(), Source::new(content.clone()));
            source_code_entries.insert(
                path_str,
                serde_json::json!({
                    "content": content
                }),
            );
        }

        // Build SolcInput
        let input = SolcInput::new(SolcLanguage::Solidity, sources, settings);

        // compile with exact version
        let version = Version::parse(compiler_version.trim_start_matches('v'))?;
        let solc = find_or_install_solc(&version)?;
        let output: CompilerOutput = solc.compile_exact(&input)?;

        // synthesize Etherscan-like metadata for storage
        // Use compilationTarget from metadata - this is the authoritative contract name
        // DO NOT use output.contracts.iter().next() as that returns alphabetically first
        // contract which may be an interface like "IERC1271" instead of the actual contract
        let contract_name = metadata_json
            .get("settings")
            .and_then(|s| s.get("compilationTarget"))
            .and_then(Value::as_object)
            .and_then(|ct| ct.values().next())
            .and_then(Value::as_str)
            .ok_or_else(|| eyre::eyre!("sourcify metadata missing settings.compilationTarget"))?;
        let abi_val = metadata_json
            .get("output")
            .and_then(|o| o.get("abi"))
            .cloned()
            .unwrap_or(Value::Array(vec![]));
        let abi_string = serde_json::to_string(&abi_val)?;
        // optimizer flags
        let (opt_used, runs) = metadata_json
            .get("settings")
            .and_then(|s| s.get("optimizer"))
            .map(|opt| {
                let en = opt.get("enabled").and_then(Value::as_bool).unwrap_or(false);
                let runs = opt.get("runs").and_then(Value::as_u64).unwrap_or(200);
                (if en { 1 } else { 0 }, runs)
            })
            .unwrap_or((0, 200));
        let evm_version = metadata_json
            .get("settings")
            .and_then(|s| s.get("evmVersion"))
            .and_then(Value::as_str)
            .unwrap_or("Default")
            .to_string();
        let source_code_field = serde_json::json!({
            "language": "Solidity",
            "sources": source_code_entries,
            "settings": metadata_json.get("settings").cloned().unwrap_or(Value::Null)
        });
        let synth = serde_json::json!({
            "Language": "Solidity",
            "CompilerVersion": compiler_version,
            "ContractName": contract_name,
            "SourceCode": source_code_field,
            "ABI": abi_string,
            "OptimizationUsed": opt_used.to_string(),
            "Runs": runs,
            "ConstructorArguments": "",
            "EVMVersion": evm_version,
            "Library": "",
            "LicenseType": "",
            "Proxy": "0",
            "Implementation": "",
            "SwarmSource": ""
        });
        let metadata: EtherscanMetadata =
            serde_json::from_value(synth).context("synthesize metadata")?;

        let artifact = Artifact { meta: metadata, input, output };
        return Ok(Some(artifact));
    }

    Ok(None)
}
