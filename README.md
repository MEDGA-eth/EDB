# EDB: The EVM Project Debugger

---

Please make sure that the following commands pass if you have changed the code:

```rust
cargo check --all
cargo test --all --all-features
cargo +nightly fmt -- --check
cargo +nightly clippy --all --all-targets --all-features -- -D warnings
```
