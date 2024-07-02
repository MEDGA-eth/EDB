use std::{
    collections::{HashMap, HashSet},
    fmt::Debug,
    path::PathBuf,
    time::Duration,
};

use alloy_chains::Chain;
use alloy_primitives::{Address, Bytes};
use edb_utils::{cache::CachePath, init_progress, update_progress};
use eyre::{eyre, Result};
use foundry_block_explorers::{contract::Metadata, errors::EtherscanError, Client};
use foundry_compilers::{
    artifacts::{output_selection::OutputSelection, SolcInput, Source},
    solc::{Solc, SolcLanguage},
};
use revm::{
    db::CacheDB,
    primitives::{CreateScheme, EnvWithHandlerCfg},
    DatabaseRef,
};

/// Default cache TTL for etherscan.
/// Set to 1 day since the source code of a contract is unlikely to change frequently.
const DEFAULT_CACHE_TTL: u64 = 86400;

use crate::{
    analysis::source_map::SourceMapAnalysis,
    artifact::{
        compilation::{AsCompilationArtifact, CompilationArtifact},
        debug::{DebugArtifact, DebugNodeFlat},
    },
    etherscan_rate_limit_guard,
    inspector::{CollectInspector, DebugInspector},
    utils::evm::new_evm_with_inspector,
};

#[derive(Debug, Default)]
pub struct DebugBackendBuilder {
    chain: Option<Chain>,
    api_key: Option<String>,
    cache_root: Option<PathBuf>,
    cache_ttl: Option<Duration>,

    // Compilation artifact from local file system
    // XXX (ZZ): let's support them later
    local_compilation_artifact: Option<CompilationArtifact>,
    compilation_artifacts: Option<HashMap<Address, CompilationArtifact>>,
}

impl DebugBackendBuilder {
    /// Set the chain to use.
    /// If not set, the default chain will be used.
    pub fn chain(mut self, chain: Chain) -> Self {
        self.chain = Some(chain);
        self
    }

    /// Set the cache root directory.
    /// If not set, the default cache directory will be used.
    pub fn cache_root(mut self, path: PathBuf) -> Self {
        self.cache_root = Some(path);
        self
    }

    /// Set the cache TTL.
    /// If not set, the default cache TTL will be used.
    pub fn cache_ttl(mut self, duration: Duration) -> Self {
        self.cache_ttl = Some(duration);
        self
    }

    /// Set the etherscan API key.
    /// If not set, a blank API key will be used.
    pub fn etherscan_api_key(mut self, etherscan_api_key: String) -> Self {
        self.api_key = Some(etherscan_api_key);
        self
    }

    // XXX (ZZ): let's support them later
    /// Set the local compilation artifact.
    /// If not set, the local compilation artifact will not be used.
    pub fn local_compilation_artifact(
        mut self,
        local_compilation_artifact: impl AsCompilationArtifact,
    ) -> Result<Self> {
        self.local_compilation_artifact = Some(local_compilation_artifact.as_artifact()?);
        Ok(self)
    }

    // XXX (ZZ): let's support them later
    /// Set the compilation artifacts.
    /// If not set, the compilation artifacts will not be used.
    pub fn compilation_artifacts(
        mut self,
        compilation_artifacts: HashMap<Address, impl AsCompilationArtifact>,
    ) -> Result<Self> {
        let result: Result<HashMap<Address, CompilationArtifact>, _> = compilation_artifacts
            .into_iter()
            .map(|(k, v)| {
                let artifact = v.as_artifact()?;
                Ok::<_, eyre::Error>((k, artifact))
            })
            .collect();

        self.compilation_artifacts = Some(result?);
        Ok(self)
    }

    /// Build the debug backend.
    pub fn build<DBRef>(self, db: &DBRef, env: EnvWithHandlerCfg) -> Result<DebugBackend<&DBRef>>
    where
        DBRef: DatabaseRef,
        DBRef::Error: std::error::Error,
    {
        // XXX: the following code looks bad and needs to be refactored
        let cb = Client::builder().with_cache(
            self.cache_root.or(CachePath::edb_etherscan_chain_cache_dir(
                self.chain.unwrap_or(Chain::default()),
            )),
            self.cache_ttl.unwrap_or(Duration::from_secs(DEFAULT_CACHE_TTL)),
        );
        let cb = if let Some(chain) = self.chain { cb.chain(chain)? } else { cb };
        let cb = if let Some(api_key) = self.api_key { cb.with_api_key(api_key) } else { cb };
        let client = cb.build()?;

        let local_compilation_artifact = self.local_compilation_artifact;

        let compilation_artifacts = self.compilation_artifacts.unwrap_or_default();

        Ok(DebugBackend {
            compilation_artifacts,
            local_compilation_artifact,
            addresses: HashSet::new(),
            metadata: HashMap::new(),
            creation_codes: HashMap::new(),
            etherscan: client,
            base_db: CacheDB::new(db),
            env,
        })
    }
}

#[derive(Debug)]
pub struct DebugBackend<DBRef> {
    // Addresses of contracts that have been visited during the transaction
    pub addresses: HashSet<Address>,

    // Creation code of contracts that are deployed during the transaction
    pub creation_codes: HashMap<Address, (Bytes, CreateScheme)>,

    /// Metadata of each contract.
    pub metadata: HashMap<Address, Metadata>,

    /// Map of source files. Note that each address will have a compilation artifact.
    pub compilation_artifacts: HashMap<Address, CompilationArtifact>,

    // Compilation artifact from local file system
    // TODO: support local compilation artifact later
    #[allow(dead_code)]
    local_compilation_artifact: Option<CompilationArtifact>,

    // Etherscan client
    etherscan: Client,

    // Transaction information
    // The base database
    base_db: CacheDB<DBRef>,
    // EVM evnironment
    env: EnvWithHandlerCfg,
}

impl<DBRef> DebugBackend<DBRef>
where
    DBRef: DatabaseRef,
    DBRef::Error: std::error::Error,
{
    #[inline]
    pub fn builder() -> DebugBackendBuilder {
        DebugBackendBuilder::default()
    }

    /// Analyze the transaction and return the debug artifact.
    pub async fn analyze(mut self) -> Result<DebugArtifact> {
        self.collect_compilation_artifacts().await?;
        self.analyze_source_map()?;

        let debug_arena = self.collect_debug_trace()?;

        Ok(DebugArtifact { debug_arena, compilation_artifacts: self.compilation_artifacts })
    }

    fn analyze_source_map(&mut self) -> Result<()> {
        for (_, artifact) in &self.compilation_artifacts {
            SourceMapAnalysis::analyze(artifact)?;
        }

        Ok(())
    }

    async fn collect_compilation_artifacts(&mut self) -> Result<()> {
        // We need to commit the transaction first (to a newly cloned cache db) before we can
        // collect the compilation artifacts.
        //
        // The major reason is that, since the transaction may create/deploy new contracts, without
        // actually committing the transaction, we cannot know the deployed code of the new
        // contracts.
        let mut db = CacheDB::new(&self.base_db);

        // Step 1. collect addresses of contracts that are visited during the transaction,
        // as well as the creation codes of contracts that are deployed during the transaction
        let mut inspect = CollectInspector::new(&mut self.addresses, &mut self.creation_codes);
        let mut evm = new_evm_with_inspector(&mut db, self.env.clone(), &mut inspect);
        evm.transact_commit().map_err(|err| eyre!("failed to transact: {}", err))?;
        drop(evm);

        // Step 2. collect source code from etherscan
        let pb = init_progress!(self.addresses, "Compiling source code from etherscan");
        for (index, addr) in self.addresses.iter().enumerate() {
            println!("{:#?} {}", addr, self.creation_codes.contains_key(addr));

            let mut meta =
                match etherscan_rate_limit_guard!(self.etherscan.contract_source_code(*addr).await)
                {
                    Ok(meta) => meta,
                    Err(EtherscanError::ContractCodeNotVerified(_)) => {
                        update_progress!(pb, index);
                        continue;
                    }
                    Err(e) => return Err(e.into()),
                };
            eyre::ensure!(meta.items.len() == 1, "contract not found or ill-formed");
            let meta = meta.items.remove(0);
            if meta.is_vyper() {
                // TODO: support Vyper later
                update_progress!(pb, index);
                continue;
            }

            // get contract name
            let contract_name = meta.contract_name.as_str();

            // get the deployed bytecode
            let deployed_bytecode = if let Some(ref bytecode) = db
                .load_account(*addr)
                .map_err(|e| {
                    eyre!(format!("the account ({}) does not exist: {}", addr, e.to_string()))
                })?
                .info
                .code
            {
                bytecode.clone()
            } else {
                let code_hash = db
                    .load_account(*addr)
                    .map_err(|e| {
                        eyre!(format!("the account ({}) does not exist: {}", addr, e.to_string()))
                    })?
                    .info
                    .code_hash();
                db.code_by_hash_ref(code_hash).map_err(|e| {
                    eyre!(format!(
                        "the code hash ({}) does not exist: {}",
                        code_hash,
                        e.to_string()
                    ))
                })?
            };

            // prepare the input for solc
            let mut settings = meta.settings()?;
            // enforce compiler output all possible outputs
            settings.output_selection = OutputSelection::complete_output_selection();
            let sources = meta
                .sources()
                .into_iter()
                .map(|(k, v)| (k.into(), Source::new(v.content)))
                .collect();
            let input = SolcInput::new(SolcLanguage::Solidity, sources, settings);

            // prepare the compiler
            let version = meta.compiler_version()?;
            let compiler = Solc::find_or_install(&version)?;

            // compile the source code
            let output = match compiler.compile_exact(&input) {
                Ok(compiler_output) => compiler_output,
                Err(_) if version.major == 0 && version.minor == 4 => {
                    // check compiler version
                    // it is known that Solc 0.4.x does not support --standard-json
                    warn!("Solc 0.4.x does not support --standard-json, skipping");
                    update_progress!(pb, index);
                    continue;
                }
                Err(e) => {
                    return Err(eyre!("failed to compile contract: {}", e));
                }
            };

            let artifact =
                (contract_name, deployed_bytecode, &input.sources, output).as_artifact()?;

            self.compilation_artifacts.insert(*addr, artifact);
            self.metadata.insert(*addr, meta);

            update_progress!(pb, index);
        }

        Ok(())
    }

    fn collect_debug_trace(&mut self) -> Result<Vec<DebugNodeFlat>> {
        let mut inspector = DebugInspector::new();
        let mut evm = new_evm_with_inspector(&mut self.base_db, self.env.clone(), &mut inspector);
        evm.transact().map_err(|err| eyre!("failed to transact: {}", err))?;
        drop(evm);

        Ok(inspector.arena.arena.into_iter().map(|n| n.into_flat()).collect())
    }
}
