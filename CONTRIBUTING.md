# Contributing to EDB

Thank you for your interest in contributing to EDB! We welcome contributions from the community and are grateful for any help you can provide.

## Legal Requirements

### Contributor License Agreement

**Important:** Before we can accept your contributions, you must sign our [Contributor License Agreement (CLA)](CLA.md).

When you submit your first pull request, the CLA Assistant bot will automatically check if you have signed the CLA. If you haven't, it will provide instructions on how to sign it electronically. Your pull request cannot be merged until the CLA is signed.

**Why do we require a CLA?**
- It ensures you have the right to contribute the code
- It allows us to relicense the project if needed (e.g., for dual-licensing)
- It protects both you and the project maintainers

## Getting Started

### Prerequisites

- Rust 1.89.0 or higher
- Git
- A GitHub account

### Development Setup

1. Fork the repository on GitHub
2. Clone your fork locally:
   ```bash
   git clone https://github.com/YOUR_USERNAME/edb.git
   cd EDB
   ```

3. Add the upstream repository:
   ```bash
   git remote add upstream https://github.com/edb-rs/edb.git
   ```

4. Create a new branch for your feature:
   ```bash
   git checkout -b feature/your-feature-name
   ```

## Development Workflow

### Building the Project

```bash
# Build all crates
cargo build

# Build in release mode
cargo build --release

# Check specific crate
cargo check -p edb-engine
```

### Running Tests

```bash
# Run all tests
cargo test --workspace --all-features

# Run tests for a specific crate
cargo test -p edb-common

# Skip the e2e suite (heaviest part) — sufficient for most local iteration
cargo test --workspace --all-features --exclude edb-integration-tests
```

The full workspace test suite includes integration + e2e tests against
vendored Foundry projects (`testdata/foundry-e2e/`) — solady,
uniswap-v4-core, solmate, prb-math, forge-template. The e2e set is
~25 min sequential on CI Linux, so it's gated behind the `run-tests`
PR label in CI (see [Pull Request Process](#pull-request-process)).
Locally you can run it ad-hoc with `cargo test`.

Before the e2e tests can find their fixtures, run:
```bash
./scripts/fetch-e2e-foundry-projects.sh
```
once after a fresh clone. The script is idempotent.

### Code Quality

Before submitting a PR, ensure your code passes all checks:

```bash
# Format code
cargo fmt --all

# Run clippy
cargo clippy --all-targets --all-features -- -D warnings
```

## Contribution Guidelines

### Code Style

- Follow Rust's official style guidelines
- Use `cargo fmt` to format your code
- Address all `cargo clippy` warnings
- Add documentation comments for public APIs
- Include unit tests for new functionality

### Commit Messages

We follow conventional commit format:

```
type(scope): subject

body (optional)

footer (optional)
```

Types:
- `feat`: New feature
- `fix`: Bug fix
- `docs`: Documentation changes
- `refactor`: Code refactoring
- `test`: Test additions or modifications
- `chore`: Maintenance tasks
- `perf`: Performance improvements

Example:
```
feat(engine): add support for contract deployment interception

Implemented DeployInspector to intercept and modify contract
deployments during EVM execution.

Closes #123
```

### Pull Request Process

1. **Ensure your branch is up to date:**
   ```bash
   git fetch upstream
   git rebase upstream/main
   ```

2. **Write clear PR description:**
   - Explain what changes you've made
   - Link any related issues
   - Include testing instructions
   - Add screenshots for UI changes

3. **PR title format:**
   - Use conventional commit format
   - Be concise but descriptive
   - Example: `feat(engine): add bytecode instrumentation support`

4. **Wait for review:**
   - Address reviewer feedback promptly
   - Push additional commits to your branch
   - Re-request review when ready

5. **Run the heavy test suite before requesting merge:**
   - The default CI (`ci.yml`) runs only the fast checks on every push:
     `fmt` / `clippy` / `check` / `build` / web-frontend.
   - The heavy `cargo test --workspace --all-features` job — including
     the integration + real-world e2e suites — is **gated behind the
     `run-tests` label** (see `.github/workflows/ci-test.yml`).
   - **Before requesting final review**, add the label to your PR:
     ```bash
     gh pr edit <number> --add-label run-tests
     ```
     or in the GitHub UI: PR page → right sidebar → Labels → check
     `run-tests` (a maintainer creates the label first if it doesn't
     exist).
   - The label re-fires on every subsequent push as long as it's still
     applied, so you don't need to re-add after rebases.
   - **Required for merge:** branch protection requires a green
     `Test (*)` check on each OS — you can't merge a PR until the
     gated workflow runs and passes.
   - For local iteration, run the full suite yourself with
     `cargo test --workspace --all-features` (slower, but matches CI).
   - Manual one-off run (skipping the label dance):
     ```bash
     gh workflow run ci-test.yml --ref <your-branch>
     ```

6. **After approval:**
   - The maintainers will merge your PR
   - Delete your feature branch

### What We're Looking For

#### Good First Issues

Look for issues labeled `good first issue` - these are ideal for newcomers.

#### Areas of Contribution

- **Core Engine**: Transaction analysis, instrumentation, debugging features
- **RPC Proxy**: Caching improvements, provider management
- **Documentation**: Tutorials, API docs, examples
- **Testing**: Unit tests, integration tests, fuzzing
- **Performance**: Optimization, benchmarking
- **UI/UX**: TUI improvements

### Testing

- Write unit tests for new functions
- Add integration tests for new features
- Ensure existing tests pass
- Test manually with real transactions when applicable

### Documentation

- Update README.md if you change user-facing functionality
- Add inline documentation for complex code
- Update API documentation for public interfaces
- Include examples in doc comments

## Communication

### Getting Help

- **GitHub Issues**: For bug reports and feature requests
- **GitHub Discussions**: For questions and general discussion

### Reporting Issues

When reporting issues, please include:
- EDB version (`edb --version`)
- Rust version (`rustc --version`)
- Operating system
- Steps to reproduce
- Expected vs actual behavior
- Error messages or logs

### Feature Requests

We welcome feature requests! Please:
- Check existing issues first
- Explain the use case
- Provide examples if possible
- Be open to discussion and alternatives

## Security

If you discover a security vulnerability, please:
- **DO NOT** open a public issue
- Email edb@zzhang.xyz with details
- Include steps to reproduce
- Allow time for us to respond and fix

## Code of Conduct

### Our Standards

- Be respectful and inclusive
- Welcome newcomers and help them get started
- Accept constructive criticism gracefully
- Focus on what's best for the community
- Show empathy towards others

### Unacceptable Behavior

- Harassment, discrimination, or hate speech
- Trolling or insulting comments
- Public or private harassment
- Publishing others' private information
- Other conduct deemed inappropriate

## Recognition

Contributors will be:
- Listed in our CONTRIBUTORS.md file
- Mentioned in release notes for significant contributions
- Given credit in project documentation

## Questions?

If you have questions about contributing, feel free to:
- Open a GitHub Discussion
- Ask in our Discord community
- Email the maintainers

Thank you for contributing to EDB! Your efforts help make Ethereum debugging better for everyone. 🚀
