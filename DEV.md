# Development Document

This is a quick note for early-stage development.

## Before Pushing to GitHub

Please ensure the following commands pass if you have changed the code:

```rust
cargo check --all
cargo test --all --all-features
cargo +nightly fmt -- --check
cargo +nightly clippy --all --all-targets --all-features -- -D warnings
```

## Notes

+ The `debug-layer` and `debug-ui` will be used by other projects like Foundry. Therefore, it is crucial to avoid introducing any circular dependencies. The following crates are safe to use:
    + `foundry-common`
    + `foundry-block-explorers`
    + `foundry-compiler`
    + `foundry-evm`: theoretically safe to use, but it is recommended not to use it in `debug-layer` and `debug-ui`.
    + `anvil`: safe to use as it does not depend on any debugger crates (recommended to use only in the `edb` crate).
+ The `utils` crate is intended for functions that should either be used by other projects like Foundry or generally by all EDB crates. Functions used exclusively by a single EDB crate should be placed within that crate.

## Todo

+ [ ] Migrate the Foundry Debugger into EDB.
+ [ ] Redesign the UI to support complex user input.
+ [ ] Support command history.
+ [ ] Support caching in the debugger.
+ [ ] Move the dependency declarations to the top of `cargo.toml`.