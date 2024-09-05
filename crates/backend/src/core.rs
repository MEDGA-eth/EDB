use std::{collections::BTreeMap, fmt::Debug, sync::Mutex, time::Duration};

use alloy_chains::Chain;
use alloy_primitives::Address;
use edb_utils::{
    api_keys,
    cache::{Cache, CachePath, EDBCache, EDBCachePath, DEFAULT_ETHERSCAN_CACHE_TTL},
    init_progress,
    onchain_compiler::OnchainCompiler,
    update_progress,
};
use eyre::{eyre, Result};
use foundry_block_explorers::Client;
use foundry_compilers::artifacts::Severity;
use rayon::prelude::*;
use revm::{
    db::CacheDB,
    primitives::{CreateScheme, EnvWithHandlerCfg},
    DatabaseRef,
};

use crate::{
    analysis::{
        inspector::{
            AnalyzedCallTrace, CallTraceInspector, DebugInspector, PushJumpInspector,
            VisitedAddrInspector,
        },
        source_map::{RefinedSourceMap, SourceMapAnalysis},
    },
    artifact::{
        debug::{DebugArtifact, DebugNodeFlat},
        deploy::{DeployArtifact, DeployArtifactBuilder},
        onchain::AnalyzedBytecode,
    },
    utils::{db, evm::new_evm_with_inspector},
    RuntimeAddress,
};

#[derive(Debug, Default)]
pub struct DebugBackendBuilder {
    cache_path: Option<EDBCachePath>,
    chain: Option<Chain>,
    client: Option<Client>,

    // Deployment artifact from local file system
    // XXX (ZZ): let's support them later
    deploy_artifacts: Option<BTreeMap<Address, DeployArtifact>>,
}

impl DebugBackendBuilder {
    /// Set the cache path. If not set, no cache will be used.
    pub fn cache_path(mut self, path: Option<EDBCachePath>) -> Self {
        self.cache_path = path;
        self
    }

    /// Set the etherscan client. If not set, a default client will be used.
    pub fn etherscan_client(mut self, client: Option<Client>) -> Self {
        self.client = client;
        self
    }

    /// Set the chain. If not set, the default chain will be used.
    pub fn chain(mut self, chain: Option<Chain>) -> Self {
        self.chain = chain;
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

        let chain = self.chain.unwrap_or_default();

        let mut client = self.client.map_or_else(
            || {
                Client::builder()
                    .chain(chain)?
                    .with_api_key(api_keys::next_etherscan_api_key())
                    .build()
            },
            Ok,
        )?;

        if let Some(etherscan_cache_path) = self.cache_path.etherscan_chain_cache_dir(chain) {
            client
                .set_cache(etherscan_cache_path, Duration::from_secs(DEFAULT_ETHERSCAN_CACHE_TTL));
        }

        let compiler_cache_root = self.cache_path.compiler_chain_cache_dir(chain);
        let compiler = OnchainCompiler::new(compiler_cache_root)?;

        let cache_root = self.cache_path.backend_chain_cache_dir(chain);
        // We do not set the cache TTL for the backend cache.
        let cache = EDBCache::new(cache_root, None)?;

        let deploy_artifacts = self.deploy_artifacts.unwrap_or_default();

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
        self.collect_artifacts().await?;

        let source_maps = self.analyze_source_map()?;
        let mut call_trace = self.analyze_call_trace(&source_maps)?;
        call_trace.calibrate_with_source(&source_maps)?;

        let debug_arena = self.collect_debug_trace()?;

        Ok(DebugArtifact { debug_arena, deploy_artifacts: self.deploy_artifacts })
    }

    fn analyze_call_trace(
        &mut self,
        source_maps: &BTreeMap<RuntimeAddress, RefinedSourceMap>,
    ) -> Result<AnalyzedCallTrace> {
        // we first need to analyze the labels for both push and jump instructions
        let mut inspector = PushJumpInspector::new(&self.addresses);
        let mut evm = new_evm_with_inspector(&mut self.base_db, self.env.clone(), &mut inspector);
        evm.transact().map_err(|err| eyre!("failed to transact: {}", err))?;
        drop(evm);
        inspector.posterior_analysis()?;
        inspector.refine_analysis_by_source_map(source_maps)?;

        #[cfg(debug_assertions)]
        inspector.log_unknown_hints();

        // we then try to construct the fine-grained call trace, which includes the call graph
        // within each contract call.
        let push_jump_info = inspector.extract();
        let mut inspector = CallTraceInspector::new(&push_jump_info, &self.addresses);
        let mut evm = new_evm_with_inspector(&mut self.base_db, self.env.clone(), &mut inspector);
        evm.transact().map_err(|err| eyre!("failed to transact: {}", err))?;
        drop(evm);

        Ok(inspector.extract())
    }

    fn analyze_source_map(&mut self) -> Result<BTreeMap<RuntimeAddress, RefinedSourceMap>> {
        let source_maps = Mutex::new(BTreeMap::new());

        self.deploy_artifacts.par_iter().try_for_each(|(addr, artifact)| -> Result<()> {
            trace!("analyzing source map for {addr:#?}");

            let [mut constructor, mut deployed] = SourceMapAnalysis::analyze(artifact)?;
            debug_assert!(constructor.is_constructor());
            debug_assert!(deployed.is_deployed());

            let constructor_addr = RuntimeAddress::constructor(*addr);
            if let Some(bytecode) = self.addresses.get(&constructor_addr) {
                constructor.labels.refine(bytecode)?;
            }

            let deployed_addr = RuntimeAddress::deployed(*addr);
            if let Some(bytecode) = self.addresses.get(&deployed_addr) {
                deployed.labels.refine(bytecode)?;
            }

            let mut source_maps = source_maps.lock().expect("this should not happen");
            source_maps.insert(constructor_addr, constructor);
            source_maps.insert(deployed_addr, deployed);
            Ok(())
        })?;

        Ok(source_maps.into_inner()?)
    }

    async fn collect_artifacts(&mut self) -> Result<()> {
        // We need to commit the transaction first (to a newly cloned cache db) before we can
        // collect the deployment artifacts.
        //
        // The major reason is that, since the transaction may create/deploy new contracts, without
        // actually committing the transaction, we cannot know the deployed code of the new
        // contracts.
        let mut db = CacheDB::new(&self.base_db);

        // Step 1. collect addresses of contracts that are visited during the transaction,
        // as well as the creation codes of contracts that are deployed during the transaction
        // (i.e., on-chain artifacts).
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

                        let artifact = DeployArtifactBuilder {
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
