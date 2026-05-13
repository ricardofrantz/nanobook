## Phase 1 - Inventory and Targeting

- [x] Confirm the core public APIs from `docs/api-surface-audit.md`.
- [x] Identify the source files for `OrderBook`, `Exchange`, `Order`, `Trade`, `Portfolio`, `RiskEngine`, and `Broker`.
- Acceptance: no internal/operational APIs selected for examples.

## Phase 2 - Root Crate Rustdoc Examples

- [x] Add concise runnable examples for `OrderBook`, `Exchange`, `Order`, `Trade`, and `Portfolio`.
- [x] Keep examples practical: order submission, fills, snapshots/queries, portfolio rebalancing, and trade analytics.
- Acceptance: examples compile under `cargo test --doc -p nanobook --all-features`.

## Phase 3 - Broker and Risk Crate Examples

- [x] Add examples for the `Broker` trait and `RiskEngine`.
- [x] Avoid adapter internals and operational helper APIs.
- Acceptance: examples compile under `cargo test --doc -p nanobook-broker -p nanobook-risk --all-features`.

## Phase 4 - Verification and Commits

- [x] Run `cargo doc --workspace --all-features --no-deps` with warnings denied.
- [x] Review diffs and commit incremental topical changes with named paths only.
- Acceptance: doc build is warning-free and git history contains small focused commits.
