# EDB Development Roadmap

## Overview

This document outlines the development roadmap for EDB (Ethereum Debugger), organized into milestones focusing on core functionality, performance, and user experience improvements.

## Milestones

### ✅ Milestone 0: Comprehensive Testing [URGENT]
**Goal**: Ensure reliability through extensive testing

- [ ] **Test Coverage Expansion**
  - Unit tests for all core modules (target: >80% coverage)
  - Integration tests for complete debugging workflows
  - End-to-end tests for UI interactions

- [ ] **Test Infrastructure**
  - Automated test suite for mainnet fork testing
  - Regression test suite for known issues
  - Performance benchmarking suite
  - Cross-chain compatibility testing

### 🎯 Milestone 1: Enhanced Debugging Capabilities
**Goal**: Expand core debugging features to support more complex scenarios

- [ ] **Calldata Variable Extraction**
  - Extract and decode function parameters from calldata
  - Support complex calldata types (arrays, structs, dynamic types)
  - Display calldata values in variable watcher
  - Map calldata offsets to parameter names

- [ ] **Solidity 0.4.x Value Extraction**
  - Support variable extraction for legacy Solidity 0.4.x contracts
  - Handle different ABI encoding format in 0.4.x
  - Adapt storage layout differences for older compiler versions
  - Ensure backward compatibility with legacy contracts

- [ ] **Complex Type Variable Watcher**
  - Support user-defined types (structs, enums, mappings)
  - Implement alternative collection methods for non-ABI-encodable types
  - Add nested type inspection capabilities

- [x] **Conditional Breakpoints**
  - Implement expression-based breakpoint conditions
  - Support state-dependent breakpoints
  - Add breakpoint management UI

- [x] **Custom Expression Evaluation**
  - Enable custom watcher expressions
  - Support runtime expression evaluation
  - Add expression history and favorites

- [ ] **Enhanced Code Navigation**
  - Full Vim key binding support for code view panel
  - Customizable key mappings
  - Visual mode selection
  - Search and replace with Vim patterns

- [ ] **State Variable Update Optimization**
  - Inject calls at each step in non-view/non-pure functions to prevent delayed updates over state variables
  - Ensure real-time state synchronization during debugging
  - Optimize state tracking for immediate feedback during execution

### 🤖 Milestone 2: Intelligence & Decoding
**Goal**: Enhanced contract understanding and automatic decoding capabilities

- [ ] **Smart Contract Intelligence**
  - Automatic address labeling from online resources
  - Integration with contract verification services
  - Known contract pattern detection

- [ ] **Selector Database Integration**
  - Function signature decoding without source code
  - Event signature recognition
  - Integration with 4byte directory and similar services

### 🚀 Milestone 3: Performance & Architecture
**Goal**: Optimize memory usage and improve snapshot mechanisms

- [ ] **Memory Optimization**
  - Implement trait objects for variable/state inheritance
  - Optimize accessible variables storage per step
  - Reduce redundant state snapshot storage
  - Reduce memory usage for db snapshots in hooked version by sharing db when no SLOAD operations occur during execution

- [x] **Improved Snapshot System**
  - Replace `call` opcode approach with bytecode pattern injection
  - Implement custom inspector for pattern-based snapshots
  - Remove function mutability requirements

- [ ] **Contract Creation Handling**
  - Fix snapshot issues when replayed transaction creates new contracts
  - Support debugging of constructor execution
  - Track contract deployment context and initialization
  - Handle CREATE and CREATE2 opcodes properly during replay

- [ ] **EngineContext Caching**
  - Cache analyzed EngineContext to disk for reuse
  - Implement cache invalidation strategy
  - Support incremental analysis updates
  - Reduce re-analysis time for repeated debugging sessions

### 🔧 Milestone 4: Tool Integration
**Goal**: Seamless integration with popular Ethereum development tools

- [ ] **Foundry Test Debugging**
  - Full support for debugging Foundry test cases
  - Integration with forge test runner
  - Test failure root cause analysis

- [ ] **Hardhat Test Support**
  - Support for Hardhat test debugging
  - Integration with Hardhat network
  - Compatibility with Hardhat plugins

- [ ] **VS Code Extension**
  - Visual Studio Code extension for integrated debugging experience
  - Seamless debugging workflow within the editor
  - Source code highlighting and breakpoint management
  - Variable inspection and watch panels
  - Integration with existing Solidity extensions

### 🌐 Milestone 5: Multi-Chain & L2 Support
**Goal**: Extend debugging capabilities beyond Ethereum mainnet with L2-specific features

- [ ] **Chain Support Expansion**
  - Support for major EVM chains (Polygon, BSC, Arbitrum, Optimism)
  - Chain-specific configuration handling
  - Multi-chain RPC endpoint management

- [ ] **L2-Specific Features**
  - Optimism/Arbitrum sequencer awareness and debugging
  - L1<->L2 message tracking and cross-layer debugging
  - L2 gas optimization analysis
  - Rollup-specific transaction lifecycle visualization

### 🧠 Milestone 6: AI & Automation
**Goal**: Leverage AI for advanced debugging assistance

- [ ] **Agent MCP Support**
  - Implement Model Context Protocol for AI assistance
  - Enable automated debugging suggestions
  - Support for debugging pattern recognition

### 🔄 Milestone 7: State Manipulation & What-If Analysis
**Goal**: Enable users to modify state and explore alternative execution paths

- [ ] **User State Modification**
  - Interactive state editing during debugging
  - Modify storage variables, balances, and contract code
  - Fork from any execution point with modified state

- [ ] **What-If Scenario Testing**
  - Save and load state snapshots
  - Compare execution paths with different initial states
  - Automated scenario generation for edge cases
  - State diff visualization between scenarios

### 🌟 Milestone 8: Enhanced Web UI
**Goal**: Develop a modern, feature-rich web interface

- [x] **Modern Web UI Development**
  - React/Vue-based responsive interface
  - Real-time debugging with WebSocket connections
  - Direct JSON-RPC communication with engine
  - Multi-tab support for parallel debugging sessions

- [ ] **Advanced UI Features**
  - Interactive call graph visualization
  - Gas usage heatmap
  - Storage and memory layout visualizers
  - Collaborative debugging with session sharing


### 📊 Milestone 9: Scripting & Analysis Tools
**Goal**: Enable programmatic debugging and custom analysis workflows

- [ ] **Python Scripting Support**
  - Python API for programmatic transaction analysis
  - Scriptable debugging sessions
  - Custom analysis script library
  - Export debugging data to Python-friendly formats (JSON, CSV, Parquet)
  - Integration with popular data science libraries (pandas, numpy)
  - Automated report generation

## Notes

- Features are subject to change based on user feedback and technical constraints
- Each milestone should be completed with comprehensive testing and documentation
- Performance benchmarks should be established before and after optimization work
