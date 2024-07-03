use alloy_primitives::Address;
use eyre::{eyre, Result};
use revm::{primitives::Bytecode, Database};

pub fn get_code<T>(db: &mut T, addr: Address) -> Result<Bytecode>
where
    T: Database,
    T::Error: std::error::Error,
{
    // get the deployed bytecode
    let info = db
        .basic(addr)
        .map_err(|e| eyre!(format!("the account ({}) does not exist: {}", addr, e.to_string())))?
        .ok_or(eyre!("the account does not exist"))?;

    if let Some(ref bytecode) = info.code {
        Ok(bytecode.clone())
    } else {
        let code_hash = info.code_hash();
        db.code_by_hash(code_hash).map_err(|e| {
            eyre!(format!("the code hash ({}) does not exist: {}", code_hash, e.to_string()))
        })
    }
}
