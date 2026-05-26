# Changelog

All notable changes to EDB (Ethereum Debugger) will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `edb test --no-ui` — runs the prepare pipeline and prints a one-line JSON
  summary to stdout (`{"target":…,"status":…,"snapshots":…,"trace_entries":…,
  "reverts":…,"edb_rejections":…}`), then exits without launching the UI.
  Status values: `ok` / `edb-rejected` / `test-revert` / `unknown`. Useful
  for batch coverage scripts and CI.
- `scripts/edb-test-coverage.sh` — walks every test function in
  `testdata/foundry-e2e/{forge-template,solady}` (populated by
  `scripts/fetch-e2e-foundry-projects.sh`), runs `edb test --no-ui` on each,
  and prints a per-test JSON line plus a final tally by status
  (ok / edb-rejected / test-revert / engine-panic / timeout). Accepts
  `--limit N` to cap the number of tests per project.
- `edb test <Contract>::<testFn>` — Foundry test debugging command.
  Walks parent dirs for `foundry.toml`, compiles via `foundry-compilers`,
  synthesizes a single-tx entrypoint, embeds a hand-rolled cheatcode
  inspector (~90 cheatcodes), and runs the whole thing through EDB's
  existing engine pipeline. See README's "Debug a Foundry Test" section
  and [`docs/cheatcodes.md`](docs/cheatcodes.md).
- `edb test --fork-url <rpc>` — opt-in mainnet/L2 forking for tests.
  Falls back to `foundry.toml`'s `eth_rpc_url` (with `${VAR}` env-var
  expansion).
- `LocalArtifactSet` indexes locally-compiled contracts by deployed-bytecode
  codehash, so `edb test` resolves source code without hitting Etherscan.
- `CheatedStack` inspector wrapper (in `crates/engine`) for layering an
  optional cheatcodes inspector over EDB's existing inspectors.
- Per-snapshot `BlockEnv` + `CfgEnv` capture so cheatcode-driven mid-tx
  env mutation (`vm.warp` / `vm.roll` / `vm.chainId`) shows up correctly
  in snapshots.
- **Snapshot family cheatcodes**: `vm.snapshotState` / `vm.snapshot`,
  `vm.revertToState` / `vm.revertTo` / `vm.revertToStateAndDelete`,
  `vm.deleteStateSnapshot` / `vm.deleteStateSnapshots`. Snapshots capture
  the journaled state (which carries the CacheDB) and are restored on
  revert (foundry-faithful one-shot semantics).
- **Assertion family cheatcodes** (40 overloads): `vm.assertEq` /
  `assertNotEq` / `assertGt` / `assertGe` / `assertLt` / `assertLe` /
  `assertTrue` / `assertFalse` for the fixed-width primitive types
  (`uint256`, `int256`, `address`, `bool`, `bytes32`), with and without
  the optional `string err` argument. Signed comparisons handle the
  cross-sign case explicitly. Dynamic / array / decimal / approxEq
  overloads are cataloged as "not yet implemented" so users see an
  actionable error instead of "unknown selector".
- **Gas snapshot stub cheatcodes** (6 overloads): `vm.startSnapshotGas`,
  `vm.stopSnapshotGas` (3 overloads), `vm.snapshotGasLastCall` (2
  overloads). Accepted as no-ops — EDB is not a gas profiler in v1.
- **`vm.rollFork(uint256)` (partial v1)**: updates `block.number` only.
  Does not touch `block.timestamp` (pair with `vm.warp`) or invalidate
  the CacheDB. See `docs/cheatcodes.md` for the limitation.
  Cross-fork roll variants (`rollFork(uint,uint)`, `rollFork(bytes32)`)
  remain rejected.

### Changed

- `--rpc-urls` and `--proxy-port` moved from the top-level CLI to the
  `replay` (and `proxy-status`) subcommands. `edb test` does not use the
  proxy — it talks directly to the upstream RPC when forking, mirroring
  `forge test` behavior.
- TUI mouse capture is **disabled by default**. The `--disable-mouse`
  flag is removed; `--enable-mouse` may return in a future release.
- `Engine::prepare_with_router_and_cheats` is the new generic engine
  entry point. The existing `prepare_with_router` delegates to it with
  `None` for cheats + local artifacts, so the replay capability is
  unaffected.

### Removed

- `cmd/debug.rs` stub (replaced by `cmd/test/`).
- `TuiOptions::disable_mouse` flag (replaced by mouse-off default).

### Internal

- `.github/workflows/release-test.yml` — release-only `cargo test --release` matrix
  across ubuntu/windows/macos. Triggered by published releases or manual
  `workflow_dispatch`. Catches optimizer-induced regressions without burdening
  per-PR CI.

## [0.0.3] - 2026-05-10

### Added

#### Engine / CLI
- Support for calldata variables ([#33](https://github.com/edb-rs/edb/pull/33))
- `r` / `R` commands in the code panel (`run` / `runback` in terminal panels) to run forward / backward until the next breakpoint
- `edb server` subcommand that collectively spawns the edb debug server ([#46](https://github.com/edb-rs/edb/pull/46))
- New `--ui=web` option (and made it the default; see Changed)

#### Browser-based web UI (`crates/web`)
- **Dockview-based shell** with draggable file tabs and movable Display / Terminal panels (drop-on-edge to split, drop-on-tab to stack) and persisted layouts.
- **CodeMirror 6 Solidity editor** with the EDB highlight theme, find panel (`Ctrl/Cmd+F`), gutter-click breakpoints, and a "Reveal current" toolbar button that scrolls the active line to center.
- **Opcode view** with PC-aware current-instruction highlighting (matching accent stripe + dim background as the source view) and a `Reveal current PC` button.
- **Debug toolbar** with VSCode-style bindings: Continue (F5), Step Into (F11), Step Out (⇧F11), Step Over (F10), Reverse Continue (⌥F5), Reverse Step (⌥F10), Restart (⇧⌘F5), Stop (⇧F5), Prev/Next Call (⌥←/⌥→), and a global "Where am I?" button that opens / focuses the file or disasm tab matching the active snapshot.
- **Command palette** (`⌘P` / `Ctrl+P` to toggle, `⌘⇧P` for command-mode-prefilled-`>`) with file open, snapshot goto, and every toolbar action.
- **Variables & Watch** sidebar with type-chipped variable cards and an inline `+` watch input; clicking an address value opens that contract's source in a new tab.
- **Display panel** with Variables, Watch, Stack, Memory, Storage, Transient, Calldata, and Output tabs. Stack rows show a per-row depth `[N]`, a top-of-stack badge, truncated hex with a full-value tooltip, and click-to-copy. Memory adds an ASCII gutter with 8-byte hex grouping.
- **Trace panel** with click-to-reveal-source (no snapshot change), right-click context menu (Jump to snapshot N · Reveal source only · Toggle children), and shift-click as a keyboard alternative for Jump.
- **Breakpoints panel** with conditional and unconditional source / opcode breakpoints, a hit list, and "clear all".
- **Terminal panel** with a Solidity REPL, terminal commands (`continue`, `step`, `over`, `out`, `goto <n>`, `break <addr>:<line>`, `break <addr>:pc=<pc>`, `bp`, `unbreak <#>`, `clear`, `help`), and an "Eye" chip that promotes the last expression to a watch.
- **Help overlay** (status-bar `?`) covering stepping shortcuts, palette, trace tree, editor, variables / watch, terminal commands, and layout / panels — all rows verified against live behavior.
- **Reopen menu** that lights up when a fixed panel (Display / Terminal) has been closed, restoring it next to its sibling.
- **Status bar** with snapshot counter, connection indicator, theme toggle, and help button.
- **Theme toggle** with light and dark themes shipped by default.
- **Navigation history** (`navHistory`) with a 200-entry cap so Reverse Step is a true undo of the user's last navigation (step / continue / palette goto / trace click).
- **Auto-follow active snapshot**: every snapshot change opens (or focuses) the file / disasm tab matching the new `(bytecode_address, source_path)` and re-pulses the in-tab scroll-to-current effect.
- **Mobile layout** with stacked code / display, scaled fonts, and a popup sheet for stage hints.

#### Marketing website (`website/`)
- New fixed-shell SPA with a 10-stage tour, IDE mock, animated tour cursor, and stage-pip navigation; deployed at `edb.zzhang.xyz`.
- Rich per-stage rail explanations with section headings, bullet lists, and starred recommendations.
- Mobile-sheet popups for stage explanations on phones; click-anywhere-dismiss + tap-hint pulse.
- Rotate-phone overlay shown for portrait phones; mobile chevrons for prev / next.
- Side-by-side Display / Code layout for landscape phones, vertical stack for portrait.
- Steppy mascot inline with the EDB wordmark in both the website hero and the README header.

#### Documentation / branding
- Online tutor badge in README pointing at `edb.zzhang.xyz`.
- Sponsor split: GitHub Sponsors badge for individuals, mailto badge for companies, with a subtle "coordinated through DAPLab @ Columbia" attribution.
- README header refreshed with new screenshots (web UI light + dark, TUI), Steppy icon, and shorter intro copy.
- DEV.md notes the `EDB_SKIP_WEB_BUILD=1` escape hatch and bun-not-found troubleshooting.
- `release.yml` for automatic release publishing to GitHub Releases (Bun installed before `cargo build --release` so prebuilt binaries always include the SPA).

### Fixed
- Struct fields are no longer incorrectly treated as variables ([#33](https://github.com/edb-rs/edb/pull/33))
- Gas limit relaxation is now correctly applied at callsites ([#39](https://github.com/edb-rs/edb/issues/39))
- Web Memory view rendered nothing for opcode snapshots with empty memory; now shows an explicit `(empty)` placeholder.
- Web opcode view's auto-scroll lost its target on backward navigation due to a single-ref-swap pattern (React's commit cleared the shared ref when the previously-current row's `ref` flipped to `undefined`); replaced with a `data-edb-current` attribute lookup that's order-independent.
- Web tab focus didn't follow the active snapshot across contracts; `MainArea` now opens (or focuses) the file / disasm tab matching the snapshot on every change.
- Web reopen menu wasn't reactive to layout changes; subscribes to `layoutJson` so the affordance pops the moment a tab is closed.
- Help overlay rendered behind the dockview surface in some browsers; z-index now ≥ 100.
- Editor toolbar metadata wrapped when the source path was long; the file-name pill now truncates and the full path lives on the `title` attribute.
- Mobile website's hit / watchpoint tags overlapped the source text on narrow viewports; tags now float above the line.
- Mobile-website empty LOCALS placeholder hidden on phones; STATE VARIABLES alone is enough.

### Changed
- `--ui` defaults to `web` (was `tui`); `--ui=tui` is the explicit fallback. The release pipeline already installs Bun, so prebuilt binaries ship with the embedded SPA baked in.
- Reverse Step (`⌥F10`) is now a true navigation-history pop ("undo my last navigation"). The engine's `prev_id`-based reverse-step-over could leap across contract boundaries when reverse-stepping out of a freshly-entered call body — replaced with a per-session `navHistory` stack populated on every `setSnapshotId`.
- `install.sh` downloads the latest release from GitHub Releases first, falling back to source builds when no release matches the platform; source builds gate on `bun --version` resolving on PATH.
- README structure: prerequisites moved under "Build from Source"; RPC endpoint guidance moved into Quickstart; em-dashes removed; sponsor framing reverted to the urgent "short on funding" line; sponsor section split into individual / company badges.
- Snapshot ids in the web UI surface as 1-based (1 / N) in the status bar and palette while remaining 0-based on the wire.
- Help overlay rows updated to match live menus (trace right-click items, ⌥F10 history-pop semantics, command palette command-mode prefill).
- Web UI variable cards replace the previous flat list; trace tree is reveal-only on left-click; activity bar uses a colour palette; toolbars get verbose labels.
- Mobile website (`@media (pointer: coarse) and ((max-width: 720px) or (max-height: 500px))`): trimmed fonts, icons, structural rows, and ide-mock-wrap padding so the IDE mock fits a phone-landscape viewport once browser chrome is accounted for; switched the shell height to `100dvh` so the layout tracks the *visible* viewport as chrome shows or hides.
- README, DEV.md, ARCH.md, and HelpOverlay copy aligned with the new defaults / semantics; the in-code `Locate` references renamed to "Where am I?" to match the user-facing label.

## [0.0.2] - 2024-10-11

### Added
- Add expression watcher ([#7](https://github.com/edb-rs/edb/issues/7))
- Partially support integration tests for edb ([#6](https://github.com/edb-rs/edb/issues/6))
- Add a popup window when errors occur in TUI
- Add mouse interaction support in TUI ([#16](https://github.com/edb-rs/edb/issues/16))
- Support conditional, unconditional, and data breakpoints ([#9](https://github.com/edb-rs/edb/issues/9))
- Tracking transient storage changes in source-level snapshots
- Runtime path-based filtering via EDB_ASSERT environment variable for assertion
- Introduced statement body analysis for more accurate source-level debugging ([#21](https://github.com/edb-rs/edb/pull/21))

### Changed
- Improved horizontal scrolling support in terminal panel vim mode
- Update dependencies to match foundry [0867fc1](https://github.com/foundry-rs/foundry/commit/0867fc1)
- Extend CI to Windows and MacOS
- Improve the cache mechanism to avoid redundant downloads ([#10](https://github.com/edb-rs/edb/issues/10))
- Speed up health check in rpc proxy ([#11](https://github.com/edb-rs/edb/pull/11))
- Remove Web UI code and dependencies ([#15](https://github.com/edb-rs/edb/pull/15))
- Add more tests for common and rpc-proxy crates, and well as more end-to-end tests for engine crate
- Optimized snapshot memory usage and processing speed by selectively storing calldata/memory/storage changes only when necessary and using persistent data structures for stack.
- Refactored analysis module with improved AST abstractions for better maintainability and extensibility ([#21](https://github.com/edb-rs/edb/pull/21))

## [0.0.1] - 2024-09-19

### Added
- **Initial release** of EDB - Ethereum Debugger
- **Source-level debugging** for Solidity smart contracts
- **Time-travel capabilities** with step-by-step execution navigation
- **Local variable inspection** with real-time value tracking
- **Custom expression evaluation** using Solidity syntax
- **Terminal User Interface (TUI)** with vim-style navigation
- **RPC proxy system** with intelligent caching and load balancing
- **Transaction replay** functionality for mainnet and testnet transactions
- **Multi-chain support** for EVM-compatible networks

#### Core Components
- `edb` - Main CLI binary for transaction debugging
- `edb-rpc-proxy` - Intelligent RPC proxy with caching
- `edb-tui` - Terminal-based debugger interface
- `edb-engine` - Core debugging and instrumentation engine
- `edb-common` - Shared utilities and types

#### Key Features
- **Bytecode instrumentation** for source-level debugging without relying on fragile source maps
- **Smart contract intelligence** with automatic ABI detection and decoding
- **Expression evaluator** supporting arbitrary Solidity expressions during debugging
- **Flexible navigation** with vim-style keybindings and time-travel controls
- **Performance optimization** through RPC caching and efficient state management

#### Debugging Capabilities
- Step through Solidity code line-by-line
- Inspect local variables, function parameters, and contract state
- Navigate function calls and returns naturally
- Jump to specific execution points
- Evaluate custom expressions against current execution state
- View opcodes and EVM state when needed

#### User Interface
- Full-featured terminal UI with syntax highlighting
- Multiple panel layout (code, variables, terminal, stack trace)
- Vim-style navigation with support for movement commands
- Real-time status updates and progress indicators
- Horizontal and vertical scrolling support

#### Technical Architecture
- Built on REVM for fast and accurate EVM simulation
- Modular crate structure for maintainability
- Comprehensive error handling and logging
- Extensible plugin architecture for future enhancements

### Dependencies
- Rust 1.89+ required
- REVM v27 for EVM simulation
- Ratatui for terminal user interface
- Tokio for async runtime
- Alloy for Ethereum type definitions

### Known Limitations
- Source code must be available and verified for full debugging capabilities
- Some advanced Solidity features may have limited support
- Performance may vary with complex contracts and long execution traces

---

## Release Notes

### Version 0.0.1
This is the initial public release of EDB, representing months of development and testing. While marked as 0.0.1, the debugger is functional and can handle real-world debugging scenarios for most Solidity contracts.

**Feedback Welcome!**
This is an early release and we're actively seeking feedback from the Ethereum development community. Please report issues, request features, and share your debugging experiences through GitHub Issues.

---

**Note**: Versions prior to 0.0.1 were internal development releases and are not documented in this changelog.
