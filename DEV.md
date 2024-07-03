# Development Document

This is a quick note for early-stage development.

## Brainstorming

+ Support abstract-interpretation style for experssion?
    

## Before Pushing to GitHub

Please ensure the following commands pass if you have changed the code:

```rust
cargo check --all
cargo test --all --all-features
cargo +nightly fmt -- --check
cargo +nightly clippy --all --all-targets --all-features -- -D warnings
```

## Notes

+ The `debug-backend` and `debug-frontend` will be used by other projects like Foundry. Therefore, it is crucial to avoid introducing any circular dependencies. The following crates are safe to use:
    + `foundry-block-explorers`
    + `foundry-compiler`
    + `foundry-evm` and `foundry-common`: theoretically safe to use, but it is recommended not to use them in `debug-backend`.
+ The `anvil` dependency is safe to use as it does not depend on any debugger crates (recommended to use only in the `edb` crate).
+ The `utils` crate is intended for functions that should either be used by other projects like Foundry or generally by all EDB crates. Functions used exclusively by a single EDB crate should be placed within that crate.
+ Error message does not need to start with a capital letter.
+ Tracing messages should be in lowercase.

## Todo

+ [x] Migrate the Foundry Debugger into EDB.
+ [x] Redesign the UI to support complex user input.
+ [ ] Support command history.
+ [ ] Support caching in the debugger.
+ [x] Support compilation cache
+ [x] Move the dependency declarations to the top of `cargo.toml`.
+ [ ] Rewrite the subject contract, to add `public` to all storage variables and functions.
+ [ ] Rewrite the subject contract, to enforce a storage update for each local variable update.
+ [ ] Debug session layout dump.
+ [ ] When Etherscan's source code is not available, we should go for Blockscout.
+ [ ] Make compilation multi-threaded.

## Some Hints

### Git Commit Message

+ feat: A new feature for the user.
+ fix: A bug fix.
+ docs: Documentation only changes.
+ style: Changes that do not affect the meaning of the code (white-space, formatting, missing semi-colons, etc).
+ refactor: A code change that neither fixes a bug nor adds a feature.
+ perf: A code change that improves performance.
+ test: Adding missing tests or correcting existing tests.
+ chore: Changes to the build process or auxiliary tools and libraries such as documentation generation.
+ ci: Changes to CI configuration files and scripts (e.g., GitHub Actions, CircleCI).
+ build: Changes that affect the build system or external dependencies (example scopes: gulp, broccoli, npm).
+ revert: Reverts a previous commit.