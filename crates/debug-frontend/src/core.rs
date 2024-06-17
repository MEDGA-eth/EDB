use edb_debug_backend::DebugBackend;
use revm::Database;

pub struct DebugFrontend<'a, DB> {
    /// The backend.
    backend: &'a mut DebugBackend<DB>,
}

impl<'a, DB> DebugFrontend<'a, DB> where DB: Database, DB::Error: std::error::Error, {
    /// Create a new frontend.
    pub fn new(backend: &'a mut DebugBackend<DB>) -> Self {
        Self { backend }
    }
}