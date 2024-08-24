use std::{collections::BTreeMap, fmt::Debug, path::PathBuf, time::Duration};

use alloy_chains::Chain;
use alloy_primitives::Address;
use edb_utils::{
    cache::{Cache, EDBCache},
    init_progress,
    onchain_compiler::OnchainCompiler,
    update_progress,
};
use eyre::{eyre, Result};
use foundry_block_explorers::Client;
use foundry_compilers::artifacts::Severity;
use revm::{
    db::CacheDB,
    primitives::{CreateScheme, EnvWithHandlerCfg},
    DatabaseRef,
};

/// Default cache TTL for etherscan.
/// Set to 1 day since the source code of a contract is unlikely to change frequently.
const DEFAULT_ETHERSCAN_CACHE_TTL: u64 = 86400;

use crate::{
    analysis::source_map::{RefinedSourceMap, SourceMapAnalysis},
    artifact::{
        debug::{DebugArtifact, DebugNodeFlat},
        deploy::{DeployArtifact, DeployArtifactBuilder},
    },
    inspector::{DebugInspector, PushJumpInspector, VisitedAddrInspector},
    utils::{db, evm::new_evm_with_inspector},
    AnalyzedBytecode, RuntimeAddress,
};

#[derive(Debug, Default)]
pub struct DebugBackendBuilder {
    chain: Option<Chain>,
    api_key: Option<String>,
    cache_root: Option<PathBuf>,
    compiler_cache_root: Option<PathBuf>,
    etherscan_cache_root: Option<PathBuf>,
    etherscan_cache_ttl: Option<Duration>,

    // Deployment artifact from local file system
    // XXX (ZZ): let's support them later
    deploy_artifacts: Option<BTreeMap<Address, DeployArtifact>>,
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
    pub fn etherscan_cache_root(mut self, path: Option<PathBuf>) -> Self {
        self.etherscan_cache_root = path;
        self
    }

    /// Set the cache TTL.
    /// If not set, the default cache TTL will be used.
    pub fn etherscan_cache_ttl(mut self, duration: Duration) -> Self {
        self.etherscan_cache_ttl = Some(duration);
        self
    }

    /// Set the compiler cache root directory.
    /// If not set, the default compiler cache directory will be used.
    pub fn compiler_cache_root(mut self, path: Option<PathBuf>) -> Self {
        self.compiler_cache_root = path;
        self
    }

    /// Set the backend cache root directory.
    /// If not set, the default backend cache directory will be used.
    pub fn cache_root(mut self, path: Option<PathBuf>) -> Self {
        self.cache_root = path;
        self
    }

    /// Set the etherscan API key.
    /// If not set, a blank API key will be used.
    pub fn etherscan_api_key(mut self, etherscan_api_key: String) -> Self {
        self.api_key = Some(etherscan_api_key);
        self
    }

    // XXX (ZZ): let's support them later
    /// Set the deployment artifacts.
    /// If not set, the deployment artifacts will not be used.
    pub fn deploy_artifacts<T>(mut self, deploy_artifacts: BTreeMap<Address, T>) -> Result<Self>
    where
        T: TryInto<DeployArtifact>,
        T::Error: std::error::Error + Send + Sync + 'static,
    {
        let result: Result<BTreeMap<Address, DeployArtifact>, _> = deploy_artifacts
            .into_iter()
            .map(|(k, v)| {
                let artifact = v.try_into()?;
                Ok::<_, eyre::Error>((k, artifact))
            })
            .collect();

        self.deploy_artifacts = Some(result?);
        Ok(self)
    }

    /// Build the debug backend.
    pub fn build<DBRef>(self, db: &DBRef, env: EnvWithHandlerCfg) -> Result<DebugBackend<&DBRef>>
    where
        DBRef: DatabaseRef,
        DBRef::Error: std::error::Error,
    {
        trace!("building debug backend with {:?}", self);

        // XXX: the following code looks bad and needs to be refactored
        let cb = Client::builder().with_cache(
            self.etherscan_cache_root,
            self.etherscan_cache_ttl.unwrap_or(Duration::from_secs(DEFAULT_ETHERSCAN_CACHE_TTL)),
        );
        let cb = if let Some(chain) = self.chain { cb.chain(chain)? } else { cb };
        let cb = if let Some(api_key) = self.api_key { cb.with_api_key(api_key) } else { cb };
        let client = cb.build()?;

        let compiler_cache_root = self.compiler_cache_root;

        let deploy_artifacts = self.deploy_artifacts.unwrap_or_default();
        let compiler = OnchainCompiler::new(compiler_cache_root)?;

        let cache_root = self.cache_root;
        // We do not set the cache TTL for the backend cache.
        let cache = EDBCache::new(cache_root, None)?;

        Ok(DebugBackend {
            deploy_artifacts,
            compiler,
            addresses: BTreeMap::new(),
            creation_scheme: BTreeMap::new(),
            etherscan: client,
            base_db: CacheDB::new(db),
            cache,
            env,
        })
    }
}

#[derive(Debug)]
pub struct DebugBackend<DBRef> {
    /// Visited addresses during the transaction, along with its corresponding bytecode.
    pub addresses: BTreeMap<RuntimeAddress, AnalyzedBytecode>,

    // Creation code of contracts that are deployed during the transaction
    pub creation_scheme: BTreeMap<Address, CreateScheme>,

    /// Map of source files. Note that each address will have a deployment artifact.
    pub deploy_artifacts: BTreeMap<Address, DeployArtifact>,

    /// Cache for backend
    pub cache: Option<EDBCache<DeployArtifact>>,

    // Etherscan client
    etherscan: Client,

    // Onchain compiler
    compiler: OnchainCompiler,

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
        self.collect_deploy_artifacts().await?;

        let source_maps = self.analyze_source_map()?;
        self.analyze_call_graph(&source_maps)?;

        let debug_arena = self.collect_debug_trace()?;

        Ok(DebugArtifact { debug_arena, deploy_artifacts: self.deploy_artifacts })
    }

    fn analyze_call_graph(
        &mut self,
        source_maps: &BTreeMap<RuntimeAddress, RefinedSourceMap>,
    ) -> Result<()> {
        // we first need to analyze the labels for both push and jump instructions
        let mut inspector = PushJumpInspector::new(&self.addresses);
        let mut evm = new_evm_with_inspector(&mut self.base_db, self.env.clone(), &mut inspector);
        evm.transact().map_err(|err| eyre!("failed to transact: {}", err))?;
        drop(evm);
        inspector.posterior_analysis()?;
        inspector.refine_analysis_by_source_map(source_maps)?;

        #[cfg(debug_assertions)]
        inspector.log_unknown_labels();

        Ok(())
    }

    fn analyze_source_map(&mut self) -> Result<BTreeMap<RuntimeAddress, RefinedSourceMap>> {
        let mut source_maps = BTreeMap::new();
        for (addr, artifact) in &self.deploy_artifacts {
            println!("\nanalyzing source map for {addr:#?}");
            debug!("analyzing source map for {addr:#?}");
            let [constructor, deployed] = SourceMapAnalysis::analyze(artifact)?;

            source_maps.insert(RuntimeAddress::constructor(*addr), constructor);
            source_maps.insert(RuntimeAddress::deployed(*addr), deployed);
        }

        Ok(source_maps)
    }

    async fn collect_deploy_artifacts(&mut self) -> Result<()> {
        // We need to commit the transaction first (to a newly cloned cache db) before we can
        // collect the deployment artifacts.
        //
        // The major reason is that, since the transaction may create/deploy new contracts, without
        // actually committing the transaction, we cannot know the deployed code of the new
        // contracts.
        let mut db = CacheDB::new(&self.base_db);

        // Step 1. collect addresses of contracts that are visited during the transaction,
        // as well as the creation codes of contracts that are deployed during the transaction
        let mut inspector =
            VisitedAddrInspector::new(&mut self.addresses, &mut self.creation_scheme);
        let mut evm = new_evm_with_inspector(&mut db, self.env.clone(), &mut inspector);
        evm.transact_commit().map_err(|err| eyre!("failed to transact: {}", err))?;
        drop(evm);

        // Step 2. collect source code from etherscan
        let pb = init_progress!(self.addresses, "Compiling source code from etherscan");
        for (index, addr) in self.addresses.keys().enumerate() {
            trace!(
                "collect deployment artifact for {} (creation scheme: {:?})",
                addr,
                self.creation_scheme.get(&addr.address)
            );

            let artifact = match self.cache.load_cache(addr.address.to_string()) {
                Some(output) => output,
                None => {
                    // get the deployed bytecode
                    let deployed_bytecode = db::get_code(&mut db, addr.address)?;
                    trace!(addr=?addr.address, len=deployed_bytecode.len(), "fetching deployed bytecode from database");

                    // compile the source code
                    if let Some((meta, sources, output)) =
                        self.compiler.compile(&self.etherscan, addr.address).await?
                    {
                        if output.errors.iter().any(|err| err.severity == Severity::Error) {
                            return Err(eyre!(format!(
                                "compilation error ({}):\n{}",
                                addr,
                                output
                                    .errors
                                    .iter()
                                    .filter(|err| err.severity == Severity::Error)
                                    .map(|err| err.formatted_message.as_deref().unwrap_or_default())
                                    .collect::<Vec<_>>()
                                    .join("\n\n")
                            )));
                        }

                        // get contract name
                        let contract_name = meta.contract_name.to_string();

                        let artifact = DeployArtifactBuilder {
                            contract_name,
                            input_sources: sources,
                            compilation_output: output,
                            explorer_meta: meta,
                            onchain_bytecode: deployed_bytecode,
                            onchain_address: addr.address,
                        }
                        .build()?;

                        self.cache.save_cache(addr.address.to_string(), &artifact)?;

                        artifact
                    } else {
                        update_progress!(pb, index);
                        continue;
                    }
                }
            };

            self.deploy_artifacts.insert(addr.address, artifact);

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
