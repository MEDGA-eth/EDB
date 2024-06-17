use edb_debug_backend::DebugBackend;

use eyre::Result;
use revm::DatabaseRef;

pub struct DebugFrontend<'a, DBRef> {
    /// The backend.
    backend: &'a mut DebugBackend<DBRef>,
}

impl<'a, DBRef> DebugFrontend<'a, DBRef>
where
    DBRef: DatabaseRef,
    DBRef::Error: std::error::Error,
{
    /// Create a new frontend.
    pub fn new(backend: &'a mut DebugBackend<DBRef>) -> Self {
        Self { backend }
    }

    /// Run the frontend.
    pub async fn run(&mut self) -> Result<()> {
        self.backend.prepare().await?;
        Ok(())
    }
}
