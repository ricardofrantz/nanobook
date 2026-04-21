# Codex Execution Status

Current phase: P0 — v0.9.3 Honesty Release
Last updated: 2026-04-21 12:57
Current PR: PR-1 (COMPLETED — awaiting review)

## Preflight Note — 2026-04-21 12:45

- Authoritative plan file: `plan_2026-04-20.md`.
- The copied first-action text names starting commit `ffb1549`, but `master`
  is currently at `04c1a59` after the user-approved Python 3.14 baseline
  repair and reproducible-lockfile commits.
- I treated `04c1a59` as the actual current baseline and ran the full §0.2
  verification suite there before PR-1 work.
- Baseline result on `04c1a59`: PASS.
- Commands run:
  - `cargo fmt --all -- --check` → PASS
  - `cargo clippy --workspace --all-targets --all-features -- -D warnings` → PASS
  - `cargo test --workspace` → PASS
  - `cargo test --workspace --all-features` → PASS
  - `cd python && maturin develop --release && uv run pytest tests/ -q && cd ..` → PASS (`114 passed, 32 skipped`)
  - `cargo deny check` → PASS (`advisories ok, bans ok, licenses ok, sources ok`; warning-only unmatched license allowances)

## PR-1: fix(ibkr): replace $999,999.99 market-order hack with tiered dispatch

- Started: 2026-04-21 12:45
- Completed: 2026-04-21 12:57
- Commit SHA: `55e6bb9277fe1e27a3f8ed2dd33273ee850ffd91`
- Files touched: 9 files (+260/-21)
- Diff stat:
  - `CHANGELOG.md` | 4 insertions
  - `broker/Cargo.toml` | 1 insertion
  - `broker/src/error.rs` | 6 insertions
  - `broker/src/ibkr/client.rs` | 47 changed
  - `broker/src/ibkr/market_data.rs` | 15 insertions
  - `broker/src/ibkr/mod.rs` | 3 changed
  - `broker/src/ibkr/orders.rs` | 91 changed
  - `broker/src/types.rs` | 7 insertions
  - `broker/tests/ibkr_market_order_bounds.rs` | 107 insertions
- Review commands (Codex's run):
  - `rg -n '999_999|999,999' broker/src/` → NO HITS
  - `rg -n '999_999|999,999' src/` → ONE PRE-EXISTING NON-PR HIT: `src/stats.rs:173`
  - `rg -n '0\.01' broker/src/ibkr/orders.rs` → NO HITS
  - `rg -n 'legacy \$999,999.99 hack must never re-appear' broker/tests/ibkr_market_order_bounds.rs` → PASS (1 match)
  - `rg -n 'NoQuoteForMarketOrder|MarketOrderRejected' broker/src/error.rs` → PASS (2 matches)
  - `cargo test --package nanobook-broker` → PASS
  - `cargo test --package nanobook-broker --features strict-market-reject` → PASS
  - `cargo test --package nanobook-broker --features ibkr --test ibkr_market_order_bounds` → PASS (5/5 default IBKR regression tests)
  - `cargo clippy -p nanobook-broker --all-targets -- -D warnings` → PASS
  - `cargo clippy -p nanobook-broker --all-targets --features strict-market-reject -- -D warnings` → PASS
  - `cargo fmt --all -- --check` → PASS
  - `cargo clippy --workspace --all-targets --all-features -- -D warnings` → PASS
  - `cargo test --workspace` → PASS
  - `cargo test --workspace --all-features` → PASS
  - `cd python && maturin develop --release && uv run pytest tests/ -q && cd ..` → PASS (`114 passed, 32 skipped`)
  - `cargo deny check` → PASS (`advisories ok, bans ok, licenses ok, sources ok`; warning-only unmatched license allowances)
- Deviations from contract:
  1. The plan text names starting commit `ffb1549`; this run uses current pushed baseline `04c1a59`.
  2. The plan asks to investigate `ibapi 2.7`; `Cargo.lock` now resolves `ibapi 2.11.0`, so I checked both local `ibapi 2.7.0` and `ibapi 2.11.0`.
  3. The plan target shows a quote-bounded aggressive limit as the default market path, but the investigation confirmed true market orders are available. I used true `ibapi::orders::order_builder::market_order` for live submissions and kept `encode_order` as the quote-bounded fallback/test helper.
  4. `broker/src/types.rs` was touched to add `BestQuote`; the PR text references `crate::types::BestQuote` but does not list `broker/src/types.rs` under Files touched.
  5. `broker/src/ibkr/mod.rs` was touched to expose the new `market_data` module and route submissions through `IbkrClient::submit_order`.
  6. `cargo test --package nanobook-broker` cannot run the new `ibkr`-gated integration tests without the `ibkr` feature. I therefore also ran `cargo test --package nanobook-broker --features ibkr --test ibkr_market_order_bounds`.
  7. `codex_status.md` is recorded in a separate follow-up status commit so it can contain the actual implementation commit SHA without amending after push.
- TODOs discovered (out of scope):
  - `src/stats.rs:173` contains `0.999_999_999_999_809_93`, which makes the broad PR-1 review grep `rg -n '999_999|999,999' src/` report a false positive unrelated to IBKR.
- Self-audit: The implementation uses true IBKR market orders in the live submission path because both `ibapi 2.7` and `2.11` support `MKT` orders without caller-supplied limit prices. The quote-bounded encoder remains tested, but it is not the live default unless future code explicitly chooses that fallback; a reviewer may challenge that this diverges from the plan's sample target block, but it follows the plan's investigation instruction to prefer option 1 when available. I used `Mutex<HashMap<Symbol, BestQuote>>` instead of adding `dashmap` because `dashmap` was not already a dependency and the plan forbids new unlisted crates without approval.

### Investigation — ibapi market-order support

- Plan text asks for `ibapi 2.7`; the current tracked `Cargo.lock` resolves
  `ibapi 2.11.0`, while the local cargo registry also contains `ibapi 2.7.0`.
- `ibapi 2.7.0` source confirms true market-order support:
  `src/orders/common/order_builder.rs` defines `market_order(action, quantity)`
  with `order_type: "MKT"` and no `limit_price` field set.
- `ibapi 2.7.0` source also confirms `OrderType::Market` does not require a
  limit price: `src/orders/builder/types.rs` has
  `OrderType::Market.as_str() == "MKT"` and
  `!OrderType::Market.requires_limit_price()` in tests.
- `ibapi 2.7.0` sync decode tests show received market orders as
  `order_type == "MKT"` and `limit_price == Some(0.0)`, so `0.0` is a decoded
  wire default, not a caller-supplied price-protection limit.
- `ibapi 2.11.0` has the same `market_order(action, quantity)` helper and
  `OrderType::Market` non-limit behavior.
- Implementation choice: use option 1 by default, i.e. true
  `ibapi::orders::order_builder::market_order` for `BrokerOrderType::Market`.
  Retain the quote-bounded aggressive-limit encoder for tests and as a fallback
  helper when a quote-bound path is explicitly used. This avoids inventing
  sentinel prices and respects the plan's "prefer option 1 if available"
  instruction.

### Review contract false positive

- The PR-1 review command `rg -n '999_999|999,999' src/` is red on the
  current baseline because it matches `src/stats.rs:173`, an unrelated
  numerical constant `0.999_999_999_999_809_93`.
- I did not edit `src/stats.rs` because PR-1 review failure modes say any diff
  touching top-level `src/` is out of scope and auto-rejected.
- Ricardo instructed Codex to use judgment and proceed; I am treating this as
  a pre-existing false positive, not a PR-1 failure.

### Review of PR-1 (commit 55e6bb9277fe1e27a3f8ed2dd33273ee850ffd91) — PENDING

Claude fills this in during review session.
