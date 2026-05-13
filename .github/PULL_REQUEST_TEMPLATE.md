## Summary

<!-- 1-3 bullets on what changed and why. Link any related issues. -->

## Test plan

<!-- Checklist of what you've verified locally. -->

- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --all-features --all-targets -- -D warnings`
- [ ] `cargo check --all-features`
- [ ] `cargo build --all-features`
- [ ] Targeted `cargo test` for the affected crate(s)
- [ ] (Optional, slow) `cargo test --workspace --all-features`

## Before merging

The default `ci.yml` pipeline runs only the **fast** checks on every
push (fmt / clippy / check / build / web-frontend). The heavy
`cargo test --workspace --all-features` job — including integration
and real-world e2e suites against solady, uniswap-v4-core, solmate,
prb-math, forge-template — is gated behind a label.

**When this PR is ready to merge:**

- [ ] Add the `run-tests` label to this PR.
      `.github/workflows/ci-test.yml` will fire and run the full Test
      job on Linux + Windows + macOS. Branch protection requires the
      three `Test (*)` checks to be green; **PRs without the label
      cannot be merged.** See `CONTRIBUTING.md` →
      [Pull Request Process](../CONTRIBUTING.md#pull-request-process)
      for details.

<!-- Reviewers: please leave the label off until ready; it costs ~25 min of CI time per push. -->
