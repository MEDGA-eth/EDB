# EDB Development Guide

This comprehensive guide provides everything you need to develop, test, and contribute to the Ethereum Debugger (EDB) project.

*This development guide was crafted with Claude with Love ❤️*

## 🛠️ Prerequisites

### System Requirements

- **Rust**: 1.88+ (required for REVM v27 and latest Foundry dependencies)
- **Git**: For version control
- **Cargo**: Rust's package manager (comes with Rust)
- **Make**: Optional, for convenience commands (future)

### External Dependencies

- **Ethereum RPC Endpoint**: Required for blockchain interaction
  - Public: `https://eth.llamarpc.com` (for testing)
  - Private: Infura, Alchemy, or local node
- **Etherscan API Key**: For downloading verified source code
  - Get free key at https://etherscan.io/apis

## 📁 Project Structure

```
EDB/
├── Cargo.toml                 # Workspace configuration
├── crates/
│   ├── edb/                   # Main CLI binary
│   │   ├── src/
│   │   │   ├── main.rs        # Entry point and CLI parsing
│   │   │   ├── proxy.rs       # RPC proxy management
│   │   │   ├── utils.rs       # Binary discovery utilities
│   │   │   └── cmd/           # Command implementations
│   │   │       ├── mod.rs
│   │   │       ├── replay.rs  # Transaction replay command
│   │   │       ├── debug.rs   # Foundry test debugging
│   │   │       └── proxy_status.rs
│   │   ├── tests/
│   │   └── Cargo.toml
│   │
│   ├── common/                # Shared utilities and types
│   │   ├── src/
│   │   │   ├── lib.rs         # Public API and re-exports
│   │   │   ├── cache.rs       # Caching infrastructure
│   │   │   ├── context.rs     # EVM context wrappers
│   │   │   ├── forking.rs     # Chain forking with REVM
│   │   │   ├── logging.rs     # Structured logging setup
│   │   │   ├── opcode.rs      # Opcode analysis utilities
│   │   │   ├── spec_id.rs     # Hardfork specification mapping
│   │   │   └── types/         # Shared data structures
│   │   │       ├── mod.rs
│   │   │       ├── abi.rs     # ABI type definitions
│   │   │       ├── code.rs    # Code representation (opcode/source)
│   │   │       ├── execution_frame.rs # Execution frame types
│   │   │       ├── snapshot.rs # Snapshot information
│   │   │       ├── sol_value.rs # Solidity value handling
│   │   │       └── trace.rs   # Execution trace structures
│   │   ├── tests/
│   │   └── Cargo.toml
│   │
│   ├── engine/                # Core debugging engine
│   │   ├── src/
│   │   │   ├── lib.rs         # Public API and configuration
│   │   │   ├── core.rs        # Main engine orchestration
│   │   │   ├── analysis/      # AST analysis and processing
│   │   │   │   ├── mod.rs
│   │   │   │   ├── analyzer.rs # Source code analysis
│   │   │   │   ├── annotation.rs # Code annotations
│   │   │   │   ├── common.rs  # Shared analysis utilities
│   │   │   │   ├── contract.rs # Contract-level analysis
│   │   │   │   ├── function.rs # Function-level analysis
│   │   │   │   ├── hook.rs    # Hook placement logic
│   │   │   │   ├── step.rs    # Execution step analysis
│   │   │   │   ├── types.rs   # Analysis type definitions
│   │   │   │   ├── variable.rs # Variable tracking
│   │   │   │   └── visitor.rs # AST visitor pattern
│   │   │   ├── eval/          # Expression evaluation system
│   │   │   │   ├── mod.rs
│   │   │   │   ├── evaluator.rs # Expression evaluator
│   │   │   │   └── handlers/  # Evaluation handlers
│   │   │   │       ├── mod.rs
│   │   │   │       └── edb.rs # EDB-specific evaluation
│   │   │   ├── inspector/     # REVM execution inspectors
│   │   │   │   ├── mod.rs
│   │   │   │   ├── call_tracer.rs # Call trace collection
│   │   │   │   ├── hook_snapshot_inspector.rs # Source-level snapshots
│   │   │   │   ├── opcode_snapshot_inspector.rs # Opcode-level snapshots
│   │   │   │   └── tweak_inspector.rs # Bytecode modification
│   │   │   ├── instrumentation/ # Code instrumentation
│   │   │   │   ├── mod.rs
│   │   │   │   ├── codegen.rs # Code generation
│   │   │   │   └── modification.rs # Source modification
│   │   │   ├── rpc/           # JSON-RPC debugging server
│   │   │   │   ├── mod.rs
│   │   │   │   ├── server.rs  # RPC server implementation
│   │   │   │   ├── types.rs   # RPC type definitions
│   │   │   │   └── methods/   # RPC method handlers
│   │   │   │       ├── mod.rs
│   │   │   │       ├── navigation.rs # Navigation commands
│   │   │   │       └── trace.rs # Trace access
│   │   │   ├── snapshot/      # Snapshot management
│   │   │   │   ├── mod.rs
│   │   │   │   ├── analysis.rs # Snapshot analysis
│   │   │   │   └── pretty_print.rs # Snapshot formatting
│   │   │   └── utils/         # Engine utilities
│   │   │       ├── mod.rs
│   │   │       ├── abi.rs     # ABI utilities
│   │   │       ├── artifact.rs # Contract artifact handling
│   │   │       ├── ast_prune.rs # AST optimization
│   │   │       ├── compilation.rs # Compilation utilities
│   │   │       ├── disasm.rs  # Disassembly utilities
│   │   │       ├── etherscan.rs # Etherscan integration
│   │   │       └── source.rs  # Source code processing
│   │   ├── tests/
│   │   │   └── config_tests.rs # Configuration tests
│   │   └── Cargo.toml
│   │
│   ├── rpc-proxy/             # Caching RPC proxy
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── cache.rs       # Cache implementation
│   │   │   ├── health.rs      # Health monitoring
│   │   │   ├── metrics.rs     # Performance metrics
│   │   │   ├── providers.rs   # Provider management
│   │   │   ├── proxy.rs       # Proxy server
│   │   │   ├── registry.rs    # Provider registry
│   │   │   ├── rpc.rs         # RPC handling
│   │   │   ├── bin/
│   │   │   │   └── main.rs    # Standalone proxy binary
│   │   │   └── tui/           # Proxy monitoring UI
│   │   │       ├── mod.rs
│   │   │       ├── app.rs
│   │   │       ├── remote.rs
│   │   │       └── widgets.rs
│   │   ├── tests/
│   │   │   └── integration_tests.rs
│   │   └── Cargo.toml
│   │
│   ├── tui/                   # Terminal user interface
│   │   ├── src/
│   │   │   ├── lib.rs         # TUI library and main runner
│   │   │   ├── app.rs         # Main TUI application logic
│   │   │   ├── config.rs      # TUI configuration
│   │   │   ├── layout.rs      # Panel layout management
│   │   │   ├── rpc.rs         # RPC client for engine communication
│   │   │   ├── bin/
│   │   │   │   └── main.rs    # TUI standalone binary
│   │   │   ├── data/          # Data management
│   │   │   │   ├── mod.rs
│   │   │   │   └── manager/   # Data core managers
│   │   │   │       ├── mod.rs
│   │   │   │       ├── execution.rs # Execution state management
│   │   │   │       └── resolver.rs # Variable resolution
│   │   │   ├── panels/        # UI panels
│   │   │   │   ├── mod.rs
│   │   │   │   ├── code.rs    # Source code display panel
│   │   │   │   ├── display.rs # Variable display panel
│   │   │   │   ├── help.rs    # Help and documentation panel
│   │   │   │   ├── terminal.rs # Command input panel
│   │   │   │   ├── trace.rs   # Execution trace panel
│   │   │   │   └── utils.rs   # Panel utilities
│   │   │   └── ui/            # UI components and styling
│   │   │       ├── mod.rs
│   │   │       ├── borders.rs # Border styling
│   │   │       ├── colors.rs  # Color schemes and themes
│   │   │       ├── icons.rs   # Unicode icons and symbols
│   │   │       ├── spinner.rs # Loading animations
│   │   │       ├── status.rs  # Status indicators
│   │   │       └── syntax/    # Syntax highlighting
│   │   │           ├── mod.rs
│   │   │           ├── opcodes.rs # EVM opcode highlighting
│   │   │           └── solidity.rs # Solidity syntax highlighting
│   │   ├── tests/
│   │   └── Cargo.toml
│   │
│   ├── web/                   # Browser-based debugging UI
│   │   ├── src/               # Rust crate (axum server + rust-embed)
│   │   │   ├── lib.rs         # Embedded SPA + RPC server entry
│   │   │   └── ...
│   │   ├── frontend/          # React/TypeScript SPA (Bun + Vite + Tailwind v4)
│   │   │   ├── src/           # Components, hooks, store, schemas
│   │   │   ├── e2e/           # Playwright end-to-end specs
│   │   │   ├── scripts/       # Dev tooling (smoke, capture, mocks)
│   │   │   ├── package.json
│   │   │   └── vite.config.ts
│   │   ├── build.rs           # Builds the SPA into the binary at compile time
│   │   └── Cargo.toml
│   │
│   └── integration-tests/     # End-to-end integration tests
│       ├── tests/
│       │   └── forking_with_proxy_tests.rs # Proxy integration tests
│       └── Cargo.toml
│
├── testdata/                  # Test data and cache
├── LICENSE                    # AGPL-3.0 license
├── COPYRIGHT                  # Copyright notice
├── README.md                  # User documentation
├── ARCH.md                    # Architecture documentation
├── DEV.md                     # This file
└── CONTRIBUTING.md            # Contribution guidelines
```

## 📝 Development Roadmap

For the complete list of planned features and improvements, please see [TODO.md](TODO.md).

## 🚀 Getting Started

### Initial Setup

```bash
# Clone the repository
git clone https://github.com/MedGa-eth/EDB.git
cd EDB

# Build all crates
cargo build --workspace

# Run tests to verify setup
cargo test --workspace

# Install binaries locally (optional)
cargo install --path crates/edb        # Main EDB CLI
cargo install --path crates/rpc-proxy  # RPC caching proxy
cargo install --path crates/tui        # Terminal UI

# Or run directly during development
cargo run -p edb -- --help
cargo run -p edb-rpc-proxy -- --help
cargo run -p edb-tui -- --help
```

### Environment Configuration

Create a `.env` file in the project root:

```env
# Ethereum RPC endpoint (required)
ETH_RPC_URL=https://eth.llamarpc.com

# Etherscan API key (recommended)
ETHERSCAN_API_KEY=your_key_here

# Logging configuration
RUST_LOG=info,edb=debug,edb_engine=debug

# Cache directory (optional)
EDB_CACHE_DIR=/tmp/edb_cache
```

## 🧪 Testing

### Running Tests

```bash
# Run all unit tests
cargo test --workspace --lib

# Run all tests including integration tests
cargo test --workspace --all

# Run specific crate tests
cargo test -p edb-common
cargo test -p edb-engine
cargo test -p edb-rpc-proxy

# Run with debug output
RUST_LOG=debug cargo test -- --nocapture

# Run ignored tests (requires RPC)
ETH_RPC_URL=https://eth.llamarpc.com cargo test -- --ignored
```

### Test Coverage Areas

#### Common Crate Tests (`crates/common/tests/`)
- **Forking Tests**: Real transaction replay with REVM integration
- **Cache Tests**: TTL and persistence functionality
- **Context Tests**: EVM wrapper and database functionality
- **Logging Tests**: Structured logging configuration

#### Engine Crate Tests (`crates/engine/tests/`)
- **Configuration Tests**: Engine setup and configuration validation
- **Analysis Tests**: AST parsing and execution step identification
- **Evaluation Tests**: Expression evaluation and type casting
- **Instrumentation Tests**: Code modification and hook insertion
- **Snapshot Tests**: State capture and navigation accuracy

#### RPC Proxy Tests (`crates/rpc-proxy/tests/`)
- **Integration Tests**: End-to-end proxy functionality
- **Cache Tests**: Response caching and invalidation behavior
- **Provider Tests**: Load balancing, failover, and health monitoring
- **Metrics Tests**: Performance metrics collection and reporting

#### CLI Tests (`crates/edb/tests/`)
- **CLI Tests**: Command-line interface and argument parsing
- **Command Tests**: Transaction replay and debugging commands

#### Integration Tests (`crates/integration-tests/tests/`)
- **Forking with Proxy Tests**: Complete workflow testing with caching proxy

### Writing Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_functionality() {
        // Unit test example
        let result = my_function();
        assert_eq!(result, expected_value);
    }

    #[tokio::test]
    async fn test_async_functionality() {
        // Async test example
        let result = async_function().await.unwrap();
        assert!(result.is_valid());
    }

    #[test]
    #[ignore] // Run with --ignored flag
    fn test_requiring_rpc() {
        // Integration test requiring RPC
        let rpc_url = std::env::var("ETH_RPC_URL")
            .expect("ETH_RPC_URL required");
        // Test implementation
    }
}
```

## 💻 Development Workflow

### Before Committing

Run the pre-commit checklist:

```bash
# Format code
cargo fmt --all

# Check linting
cargo clippy --all-targets --all-features -- -D warnings

# Run tests
cargo test --workspace

# Check compilation for all targets
cargo check --all-targets --all-features

# Build documentation
cargo doc --no-deps --workspace
```

### Debugging the Debugger

```bash
# Run with maximum verbosity
RUST_LOG=trace cargo run -- replay 0x...

# Run with specific module debugging
RUST_LOG=edb_engine::core=trace cargo run -- replay 0x...

# Use debugger (with lldb)
rust-lldb target/debug/edb -- replay 0x...

# Profile performance
cargo build --release
perf record --call-graph=dwarf target/release/edb replay 0x...
perf report
```

## 🌐 Web UI Development

The web UI is a React/TypeScript SPA living in `crates/web/frontend/`,
embedded into the `edb` binary at compile time via `rust-embed`. The
engine's existing Axum listener serves both the JSON-RPC API and the SPA
on the same port — no second listener, no extra binary.

### Stack

| Layer | Tooling |
|---|---|
| Runtime / package manager | Bun |
| Framework | React 19 + TypeScript + Vite |
| UI shell | dockview (VSCode-style splits + tabs) |
| Styling | Tailwind v4 |
| State | TanStack Query (server) + Zustand (UI) |
| Editor | CodeMirror 6 (Solidity language pack) |
| Validation | zod schemas matching the engine's serde-default JSON |

### Iterating without a real engine

```bash
cd crates/web/frontend
bun install
bun run dev --mode=mock          # http://127.0.0.1:5173, RPC served from canned fixtures
bun test src/                    # unit + component tests
bun run e2e                      # Playwright flows against the mock dev server
```

The mock fixtures under `src/data/mocks/captures/` are real engine
captures from a Uniswap V2 swap (196 snapshots, 11 trace entries) — sizes
and shapes match production.

### Iterating against a real engine

In one terminal (web UI is the default since the launcher flip):
```bash
edb replay 0x<tx-hash>
```

In another:
```bash
cd crates/web/frontend
bun run dev                      # proxy /POST and /health to the engine's port
```

The dev server proxies `POST /` and `GET /health` to the running engine
(defaults to `127.0.0.1:8545`; override with `VITE_EDB_RPC_PORT`).

### Smoke testing the live UI

```bash
URL=http://127.0.0.1:<engine-port> bun run scripts/smoke.ts
```

Drives the live UI end-to-end and captures `resources/edb-web.png`. Set
`DARK=1` and `OUT=…edb-web-dark.png` for the dark variant. Use
`bun run scripts/capture-with-file.ts` if you want a screenshot with a
Solidity file open in the editor.

### Skipping the bun build

If you don't have `bun` installed, set `EDB_SKIP_WEB_BUILD=1` before any
`cargo` command. The build script writes a placeholder `dist/index.html`
into the embedded assets, so:

- `cargo build` / `cargo check` succeed.
- `--ui=tui` and the CLI work normally.
- The default web UI (and any explicit `--ui=web` invocation) serves the
  placeholder page (a single line of HTML saying "edb-web: dist not
  built"), not the real SPA.

**Don't ship release builds with this flag set**: since the web UI is the
default, end users will see the placeholder when they run `edb replay`.
Verify `bun --version` resolves on the build agent before invoking
`cargo build --release`.

#### Why does cargo say "bun not found on PATH"?

The frontend build script (`crates/web/build.rs`) shells out to `bun`,
inheriting `PATH` from whoever launched `cargo`. The Bun installer adds
`export PATH="$HOME/.bun/bin:$PATH"` to your shell rc file, but that line
only runs in **interactive** shells. If `cargo build` is invoked from a
non-interactive shell, an IDE task runner that doesn't source your rc, or
a terminal opened **before** the install line was appended, `bun` will be
missing. Workarounds:

- `PATH="$HOME/.bun/bin:$PATH" cargo build` — one-off override.
- Add the export to `~/.zprofile` (login shells) **and** `~/.zshrc`
  (interactive) so all spawned shells see it.
- `EDB_SKIP_WEB_BUILD=1 cargo build` — skip the frontend bundle entirely.

### Security boundary

The engine binds to `127.0.0.1`, so the JSON-RPC API and embedded UI are
reachable only from the same machine. On shared environments
(devcontainers, remote SSH, multi-user dev hosts), any local user can
connect — there is no per-user authentication. Use OS-level isolation if
you're debugging sensitive data.

## 🏗️ Architecture Guidelines

### Module Responsibilities

#### Common Crate (`edb-common`)
- **Purpose**: Shared utilities and types across all EDB components
- **Dependencies**: Minimal core libraries (REVM, Alloy, serde)
- **Exports**: Common types, caching, logging, forking utilities
- **Key Features**: Chain forking, execution context, trace types

#### Engine Crate (`edb-engine`)
- **Purpose**: Core debugging engine with analysis and execution
- **Dependencies**: Common crate, Foundry/Alloy ecosystem
- **Key Features**: AST analysis, code instrumentation, snapshot management, expression evaluation
- **State**: Immutable after preparation, thread-safe execution

#### RPC Proxy Crate (`edb-rpc-proxy`)
- **Purpose**: Intelligent caching proxy for Ethereum RPC endpoints
- **Dependencies**: Common crate, HTTP client libraries
- **Key Features**: Response caching, provider load balancing, health monitoring
- **State**: Persistent cache with TTL, provider health tracking

#### TUI Crate (`edb-tui`)
- **Purpose**: Terminal-based debugging interface
- **Dependencies**: Engine RPC client, terminal UI libraries (ratatui)
- **Key Features**: Multi-panel interface, syntax highlighting, real-time updates
- **State**: UI state management, no debugging logic

#### Web Crate (`edb-web`)
- **Purpose**: Browser-based debugging interface
- **Dependencies**: Engine RPC server (shared port), `rust-embed` for the SPA
- **Key Features**: Embedded React/TypeScript SPA, single-port architecture (`GET /*` serves SPA assets, `POST /` serves JSON-RPC), light/dark themes, dockview-based layout
- **State**: Zustand store + TanStack Query cache live in the browser; the Rust crate is stateless beyond serving static assets

#### CLI Crate (`edb`)
- **Purpose**: Command-line interface and workflow orchestration
- **Dependencies**: Engine and proxy crates
- **Key Features**: Transaction replay, proxy management, CLI argument parsing
- **State**: Stateless command execution

### Code Patterns

#### Error Handling
```rust
use eyre::{Result, eyre, Context};

pub fn risky_operation() -> Result<Value> {
    let data = fetch_data()
        .context("Failed to fetch data")?;

    let processed = process_data(data)
        .map_err(|e| eyre!("Processing failed: {}", e))?;

    Ok(processed)
}
```

#### Async Patterns
```rust
use tokio::time::{sleep, Duration};

pub async fn retry_operation<F, T>(
    mut f: F,
    max_retries: usize,
) -> Result<T>
where
    F: FnMut() -> Result<T>,
{
    for attempt in 0..max_retries {
        match f() {
            Ok(result) => return Ok(result),
            Err(e) if attempt < max_retries - 1 => {
                warn!("Attempt {} failed: {}", attempt + 1, e);
                sleep(Duration::from_secs(1 << attempt)).await;
            }
            Err(e) => return Err(e),
        }
    }
    unreachable!()
}
```

#### REVM Integration
```rust
use revm::{Context, MainnetEvm};
use edb_common::EdbContext;

pub fn execute_with_inspector<I>(
    ctx: EdbContext,
    inspector: &mut I,
) -> Result<ExecutionResult>
where
    I: Inspector<CTX = Context>,
{
    let mut evm = ctx.build_mainnet_with_inspector(inspector);
    let result = evm.transact_commit(tx_env)?;
    Ok(result)
}
```

## 🔧 Common Tasks

### Adding a New RPC Method

1. Define the method in `engine/src/rpc/types.rs`:
```rust
#[derive(Serialize, Deserialize)]
pub struct MyMethodParams {
    pub param1: String,
    pub param2: u64,
}
```

2. Implement handler in `engine/src/rpc/methods/`:
```rust
pub async fn handle_my_method(
    params: MyMethodParams,
    context: &EngineContext,
) -> Result<JsonValue> {
    // Implementation
}
```

3. Register in method dispatcher:
```rust
match method {
    "edb_myMethod" => handle_my_method(params, &self.context).await,
    // ...
}
```

### Adding a New Inspector

1. Create inspector in `engine/src/inspector/`:
```rust
pub struct MyInspector {
    // State fields
}

impl Inspector for MyInspector {
    type CTX = Context;

    fn step(&mut self, interp: &mut Interpreter, ctx: &mut Self::CTX) {
        // Capture state
    }
}
```

2. Integrate into engine workflow:
```rust
let mut inspector = MyInspector::new();
let result = ctx.build_mainnet_with_inspector(&mut inspector)
    .transact_commit(tx_env)?;
```

### Adding Expression Evaluation Features

The engine includes a powerful expression evaluation system for debugging:

```rust
// Add a new evaluation handler in engine/src/eval/handlers/
pub async fn handle_new_expression(
    params: &[String],
    context: &EdbExecutionContext,
) -> Result<String> {
    // Custom evaluation logic
    Ok(result)
}

// Register in the evaluation dispatcher
match function_name {
    "myFunction" => handle_new_expression(params, context).await,
    // ...
}
```

### Adding UI Components

#### TUI Panel
```rust
// In tui/src/panels/
pub struct MyPanel {
    // Panel state
}

impl Panel for MyPanel {
    fn render(&mut self, f: &mut Frame, area: Rect) {
        // Render logic using ratatui
    }

    fn handle_input(&mut self, key: KeyEvent) -> Result<EventResponse> {
        // Input handling with proper event responses
    }
}
```

## 🐛 Troubleshooting

### Common Issues

#### "No transport enabled" Error
**Solution**: Enable the correct features for Alloy provider:
```toml
alloy-provider = { version = "0.8", features = ["reqwest"] }
alloy-transport-http = { version = "0.8" }
```

#### REVM v28+ API Changes
**Solution**: Use the new Context-based API pattern:
```rust
// Current pattern for REVM v28+
let mut evm = ctx.build_mainnet_with_inspector(&mut inspector);
let result = evm.transact_commit()?;
```

#### Expression Evaluation Syntax Errors
**Solution**: Follow Solidity-like syntax for debugging expressions:
```rust
// Examples of valid expressions
"block.number"           // Access block information
"msg.sender"            // Access transaction context
"balanceOf(0x123...)"   // Call contract functions
"myVar"                 // Access local variables
```

#### Compilation Errors with Foundry Dependencies
**Solution**: Ensure consistent versions across the Alloy ecosystem:
```bash
cargo tree -d  # Check for duplicate dependencies
cargo update   # Update to compatible versions
```

#### TUI Rendering Issues
**Solution**: Ensure terminal compatibility and proper sizing:
```bash
# Test terminal capabilities
echo $TERM
resize      # Check terminal size

# Run with debug output
RUST_LOG=debug cargo run -p edb-tui
```

### Performance Issues

#### Slow Transaction Replay
- Use `--quick` mode for recent transactions
- Ensure RPC proxy is running for caching
- Consider using local node for frequent debugging

#### High Memory Usage
- Limit snapshot collection with selective instrumentation
- Use streaming for large source files
- Clear cache periodically

## 📝 Code Style

### Rust Guidelines

- Use `rustfmt` for consistent formatting
- Follow Rust API guidelines: https://rust-lang.github.io/api-guidelines/
- Prefer explicit types for public APIs
- Document all public items with doc comments

### Git Commit Convention

Use conventional commits:

```
type(scope): description

[optional body]

[optional footer]
```

Types:
- `feat`: New feature
- `fix`: Bug fix
- `docs`: Documentation
- `style`: Formatting
- `refactor`: Code restructuring
- `perf`: Performance improvement
- `test`: Testing
- `chore`: Maintenance
- `build`: Build system

Examples:
```bash
git commit -m "feat(engine): add dual-layer snapshot system"
git commit -m "fix(tui): correct panel layout calculation"
git commit -m "docs: update development guide with REVM v27"
```

## 🚢 Release Process

### Version Bumping

```bash
# Update version in all Cargo.toml files
cargo set-version 0.2.0

# Update CHANGELOG.md
# Create git tag
git tag -a v0.2.0 -m "Release version 0.2.0"
```

### Publishing (Future)

```bash
# Dry run
cargo publish --dry-run -p edb-common

# Publish in dependency order
cargo publish -p edb-common
cargo publish -p edb-engine
cargo publish -p edb-rpc-proxy
cargo publish -p edb-tui
cargo publish -p edb
```

## 📚 Resources

### Documentation
- [REVM Documentation](https://github.com/bluealloy/revm) - EVM implementation in Rust
- [Alloy Documentation](https://alloy.rs/) - Ethereum library ecosystem
- [Foundry Book](https://book.getfoundry.sh/) - Smart contract development toolkit
- [Ratatui Documentation](https://ratatui.rs/) - Terminal UI library
- [Ethereum Yellow Paper](https://ethereum.github.io/yellowpaper/paper.pdf) - EVM specification

### Development Tools
- [Etherscan API](https://docs.etherscan.io/) - Blockchain data and verified contracts
- [Cast (Foundry CLI)](https://book.getfoundry.sh/cast/) - Command-line Ethereum toolkit
- [Tenderly Debugger](https://tenderly.co/) - Web-based transaction debugger (comparison)
- [EVM Opcodes](https://www.evm.codes/) - Complete opcode reference
- [Solidity Documentation](https://docs.soliditylang.org/) - Smart contract language

### Community & Support
- [EDB GitHub Repository](https://github.com/MedGa-eth/EDB) - Source code and issues
- [Foundry Discord](https://discord.gg/foundry) - Development community
- [Ethereum StackExchange](https://ethereum.stackexchange.com/) - Technical Q&A
- [Rust Programming Language](https://www.rust-lang.org/) - Core language resources

