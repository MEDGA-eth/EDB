//! This module implements the instrumentation on the contract source code for debugging purposes.
//!
//! When debugging a contract, we need to be able to observe the value of high-level variables.
//! This is not possible by just looking at the EVM memory and stack at runtime since only low-
//! level data are stored there, while we need to reason about high-level data structure.
//! One way (and this is also the way used in remix-IDE) is to recover high-level values from
//! the low level data in memory and stack at runtime, with the help of some heuristics of how
//! high-level data are encoded in memory and stack. However, this approach is imprecise and
//! does not always work. In EDB, we propose a different approach: we mutate the contract source
//! code to insert some debug statements that store the value of high-level variables into the
//! contract storage at runtime. Then when a variable is queried during debugging, we can just
//! read the value from the contract storage. Since we are mutating from the source code, we do
//! not need to worry about the low-level encoding and decoding of high-level data; Solidity
//! compiler will take care of that for us.
//!
//! WARNING: Our approach modifies the contract source code, thus the bytecode will be different.
//! As a result, replaying the transaction, although the execution result will be the same, the
//! gas cost may be different.
//!
//! Each variable will be assigned a unique identifier, called Universal Variable Identifier (UVID).
//! UVID respects the scope of the variable, i.e., variables with the same name but in different
//! scopes will have different UVIDs. UVIDs are used to identify the storage slot where the value
//! of the variable is stored.
//! Abstractly, all variables are stored in storage in a mapping data structure:
//!     mapping(uint256 => any) edb_runtime_values;
//! where the key is the UVID of the variable, and this mapping should be stored in storage at slot
//! `EDB_RUNTIME_VALUES_SLOT`, which is a constant that should be large enough to avoid collision
//! with other storage variables originally defined in the contract. The value of the variable
//! corresponding to a UVID is stored in the same way as they are encoded in storage by Solidity in
//! the slot mapped by UVID.
//!
//! NOTE: Storage variables in the contract are not stored in the `edb_runtime_values` as they are
//! already stored in storage. We only store the value of local variables.
//!
//! When preparing debug artifacts, we will first scan the contract source code and assign UVIDs to
//! all variables.
//! Then, we will insert code between each statement in the contract to store the value of local
//! variables into the `edb_runtime_values` mapping. To optimize, only variables whose values
//! change are stored.
//! In addition, we will insert additional functions to read the value of a variable by its UVID.
//!
//! At debugging time, we will let the transaction run on the bytecode of the instrumented contract.
//! When a variable's value is queried, e.g., in the watcher, in the middle of the execution, we
//! will spawn a new EVM on top of the current storage, and call the function to read the value of
//! that variable, with the UVID as the argument.
