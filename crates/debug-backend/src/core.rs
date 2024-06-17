use std::{cell::RefCell, collections::HashMap, fmt::Debug, rc::Rc};

use alloy_chains::Chain;
use alloy_primitives::Address;
use eyre::{eyre, Result};
use foundry_block_explorers::Client;
use revm::{primitives::EnvWithHandlerCfg, Database};

use crate::{
    artifact::{
        compilation::{AsCompilationArtifact, CompilationArtifact},
        debug::DebugArtifact,
    },
    inspector::DebugInspector,
    utils::evm::new_evm_with_inspector,
};

#[derive(Debug, Default)]
pub struct DebugBackendBuilder {
    chain: Option<Chain>,
    api_key: Option<String>,
    local_compilation_artifact: Option<CompilationArtifact>,
    identified_contracts: Option<HashMap<Address, String>>,
    compilation_artifacts: Option<HashMap<Address, CompilationArtifact>>,
}

impl DebugBackendBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn chain(mut self, chain: Chain) -> Self {
        self.chain = Some(chain);
        self
    }

    pub fn etherscan_api_key(mut self, etherscan_api_key: String) -> Self {
        self.api_key = Some(etherscan_api_key);
        self
    }

    pub fn local_compilation_artifact(
        mut self,
        local_compilation_artifact: impl AsCompilationArtifact,
    ) -> Self {
        self.local_compilation_artifact = Some(local_compilation_artifact.into());
        self
    }

    pub fn identified_contracts(mut self, identified_contracts: HashMap<Address, String>) -> Self {
        self.identified_contracts = Some(identified_contracts);
        self
    }

    pub fn compilation_artifacts(
        mut self,
        compilation_artifacts: HashMap<Address, impl AsCompilationArtifact>,
    ) -> Self {
        self.compilation_artifacts = Some(
            compilation_artifacts
                .into_iter()
                .map(|(k, v)| (k, v.into()))
                .collect::<HashMap<Address, CompilationArtifact>>(),
        );
        self
    }

    pub fn build<DB>(self) -> Result<DebugBackend<DB>>
    where
        DB: Database,
        DB::Error: std::error::Error,
    {
        // XXX: the following code looks not elegant and needs to be refactored
        let cb = Client::builder();
        let cb = if let Some(chain) = self.chain { cb.chain(chain)? } else { cb };
        let cb = if let Some(api_key) = self.api_key { cb.with_api_key(api_key) } else { cb };
        let client = cb.build()?;

        let local_compilation_artifact =
            self.local_compilation_artifact.map(|a| Rc::new(RefCell::new(a)));

        let identified_contracts =
            Rc::new(RefCell::new(self.identified_contracts.unwrap_or_default()));

        let compilation_artifacts =
            Rc::new(RefCell::new(self.compilation_artifacts.unwrap_or_default()));

        let creation_code = Rc::new(RefCell::new(HashMap::new()));

        Ok(DebugBackend {
            identified_contracts,
            compilation_artifacts,
            local_compilation_artifact,
            creation_code,
            client,
            phantom: std::marker::PhantomData,
        })
    }
}

#[derive(Debug)]
pub struct DebugBackend<DB> {
    /// Identified contracts.
    pub identified_contracts: Rc<RefCell<HashMap<Address, String>>>,
    /// Map of source files. Note that each address will have a compilation artifact.
    pub compilation_artifacts: Rc<RefCell<HashMap<Address, CompilationArtifact>>>,

    // Compilation artifact from local file system
    local_compilation_artifact: Option<Rc<RefCell<CompilationArtifact>>>,
    // Creation code for each contract
    creation_code: Rc<RefCell<HashMap<Address, Option<u64>>>>,

    // etherscan client
    client: Client,

    phantom: std::marker::PhantomData<DB>,
}

impl<DB> DebugBackend<DB>
where
    DB: Database,
    DB::Error: std::error::Error,
{
    pub fn builder() -> DebugBackendBuilder {
        DebugBackendBuilder::default()
    }

    pub async fn debug(&mut self, mut db: DB, env: EnvWithHandlerCfg) -> Result<DebugArtifact> {
        let mut inspector = DebugInspector::new();
        let mut evm = new_evm_with_inspector(&mut db, env, &mut inspector);
        evm.transact().map_err(|err| eyre!("failed to transact: {}", err))?;
        drop(evm);

        let debug_arena = inspector.arena.arena.into_iter().map(|n| n.into_flat()).collect();
        println!("{:?}", debug_arena);

        Ok(DebugArtifact {
            debug_arena,
            identified_contracts: self.identified_contracts.borrow().clone(),
            compilation_artifacts: self.compilation_artifacts.borrow().clone(),
        })
    }
}
