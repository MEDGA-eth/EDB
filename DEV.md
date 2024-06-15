# Development Plan

It is a quick note for early-stage development

## Before Push to Github

Please make sure that the following commands pass if you have changed the code:

```rust
cargo check --all
cargo test --all --all-features
cargo +nightly fmt -- --check
cargo +nightly clippy --all --all-targets --all-features -- -D warnings
```

## Note

+ The `debug-layer` and `debug-ui` will be used by other projects like foundry. It is hence important to make sure it will not introduce any loop dependencies. The following crates are safe to use:
    + `foundry-common` 
    + `foundry-block-explorers`
    + `foundry-compiler`
    + `foundry-evm`
    + `anvil`: anvil would not depend on any debugger crates, and it is hence safe to use.

## Todo

+ [ ] Migrate the Foundry Debugger into EDB.
+ [ ] Redesign the UI to support complex user input.
+ [ ] Support command history.
