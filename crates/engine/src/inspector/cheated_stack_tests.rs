// EDB - Ethereum Debugger
// Copyright (C) 2024 Zhuo Zhang and Wuqi Zhang
// SPDX-License-Identifier: AGPL-3.0

//! Smoke tests for `CheatedStack`. Behavior tests live in `crates/edb`
//! once foundry-cheatcodes is integrated.

use crate::inspector::{CheatedStack, NoCheats};

#[test]
fn cheated_stack_compiles_with_none() {
    let mut inner = NoCheats;
    let cheats: Option<&mut NoCheats> = None;
    let _stack = CheatedStack::new(cheats, &mut inner);
}

#[test]
fn cheated_stack_compiles_with_some() {
    let mut inner = NoCheats;
    let mut cheats = NoCheats;
    let _stack = CheatedStack::new(Some(&mut cheats), &mut inner);
}
