use alloy_primitives::Address;
use eyre::{eyre, Result};
use foundry_compilers::artifacts::DeployedBytecode;

pub fn link_contracts_fakely(contract: &mut DeployedBytecode, addr: Option<Address>) -> Result<()> {
    let addr = addr.unwrap_or_default();
    let bytecode = contract.bytecode.as_mut().ok_or(eyre!("missing bytecode"))?;

    let references: Vec<_> = bytecode
        .link_references
        .iter()
        .flat_map(|(file, libraries)| {
            libraries.iter().map(move |(library, _)| (file.clone(), library.clone()))
        })
        .collect();

    for (file, library) in references {
        bytecode.link(&file, &library, addr);
    }

    bytecode.object.resolve().ok_or(eyre!("object linking failed"))?;

    Ok(())
}

pub fn bytecode_similarity(bytecode1: &[u8], bytecode2: &[u8]) -> f64 {
    let len_s1 = bytecode1.len();
    let len_s2 = bytecode2.len();

    // create a 2D array to store lengths of longest common subsequence
    let mut lcs_table = vec![vec![0; len_s2 + 1]; len_s1 + 1];

    // build the table in bottom-up fashion
    for i in 1..=len_s1 {
        for j in 1..=len_s2 {
            if bytecode1[i - 1] == bytecode2[j - 1] {
                lcs_table[i][j] = lcs_table[i - 1][j - 1] + 1;
            } else {
                lcs_table[i][j] = lcs_table[i - 1][j].max(lcs_table[i][j - 1]);
            }
        }
    }

    return lcs_table[len_s1][len_s2] as f64 / len_s1.max(len_s2) as f64;
}
