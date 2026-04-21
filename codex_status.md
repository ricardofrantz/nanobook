# Codex Execution Status

Current phase: P0 — v0.9.3 Honesty Release
Last updated: 2026-04-21 14:15
Current PR: PR-2 (COMPLETED — awaiting review)

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

### Review of PR-1 (commit 55e6bb9277fe1e27a3f8ed2dd33273ee850ffd91) — APPROVED

Reviewer: Claude (Opus 4.7), session 2026-04-21.

**Verdict: APPROVED.** The legacy $999,999.99 market-order hack is
gone from the live path. The implementation uses true
`ibapi::orders::order_builder::market_order` (option 1) for the default
Market submission and retains `encode_order` as a quote-bounded option-2
helper. The `strict-market-reject` feature correctly gates both
functions. All regression tests present and passing.

**Review commands re-run independently (all green):**

- `rg -n '999_999|999,999' broker/src/` → no hits ✓
- `rg -n '0\.01' broker/src/ibkr/orders.rs` → no hits ✓
- `rg -n 'legacy \$999,999.99 hack must never re-appear' broker/tests/ibkr_market_order_bounds.rs` → 1 match (line 79) ✓
- `rg -n 'NoQuoteForMarketOrder|MarketOrderRejected' broker/src/error.rs` → 2 matches (lines 25, 28) ✓
- `cargo test --package nanobook-broker` → 12/12 PASS (including doctests)
- `cargo test --package nanobook-broker --features ibkr --test ibkr_market_order_bounds` → 5/5 PASS
- `cargo test --package nanobook-broker --features "ibkr,strict-market-reject" --test ibkr_market_order_bounds` → 2/2 PASS
- `cargo clippy -p nanobook-broker --all-targets -- -D warnings` → clean
- `cargo clippy -p nanobook-broker --all-targets --features "ibkr,strict-market-reject" -- -D warnings` → clean
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` → clean
- `cargo fmt --all -- --check` → clean
- `git show --stat 55e6bb9` → 9 files, all under `broker/` or `CHANGELOG.md`. No scope creep.

**Deviations accepted:**

1. **Baseline drift (ffb1549 → 04c1a59).** The plan's launcher prompt named
   `ffb1549` as the starting commit. Codex correctly identified that
   `master` had advanced to `04c1a59` (Ricardo-approved Python 3.14 baseline
   + reproducible-lockfile commits) and used that as the baseline. §0.2
   verification confirmed clean on `04c1a59`. Accepted.

2. **Option 1 (true MKT) over option 2 (bounded limit) in live path.** The
   plan's **Investigation required** block explicitly said "prefer option 1
   (true market) if available" and Codex's investigation documented that
   `ibapi` 2.7 and 2.11 both expose `order_builder::market_order` with
   `OrderType::Market` requiring no limit price. Using option 1 as the
   live default and retaining `encode_order` as the bounded-fallback
   helper is faithful to the plan's Investigation instruction. Accepted.

3. **`broker/src/types.rs` touched (BestQuote).** Not listed in **Files
   touched**, but the contract's Target state code block used
   `crate::types::BestQuote`, implying the type needed to be defined
   somewhere. `broker/src/types.rs` is the natural home. Accepted.

4. **`broker/src/ibkr/mod.rs` touched (expose market_data module).**
   Required to surface the new `market_data` module. Wiring change, not
   scope creep. Accepted.

5. **`cargo test --package nanobook-broker --features ibkr ...`** added to
   Codex's own review-command set because new regression tests are gated
   on the `ibkr` feature. I re-ran the same matrix. Accepted.

**Plan defect identified (not Codex's fault):**

The PR-1 review command `rg -n '999_999|999,999' src/` was over-broad and
matches `0.999_999_999_999_809_93` in `src/stats.rs:173` — a
pre-existing rational-approximation coefficient unrelated to the IBKR
hack. Codex correctly declined to edit `src/` because doing so would
violate the PR-1 scope-discipline auto-reject rule. This is a plan
defect, not a PR-1 failure. I will tighten the review-command pattern
in `plan_2026-04-20.md` for future similar patterns to match only the
literal dollar-and-cents form (e.g., `999_999\.99|999,999\.99`). The
false positive does not block approval.

**Non-blocking observations (follow-up candidates):**

1. **Dead error path in live submission.** `orders::submit_order` accepts
   `best_quote: Option<&BestQuote>` but the `BrokerOrderType::Market`
   branch (orders.rs:99) calls `ibapi::market_order` directly and does
   not consult `best_quote`. Consequently, `BrokerError::NoQuoteForMarketOrder`
   is unreachable from the live path and only fires inside the
   `encode_order` helper used by tests and future explicit-fallback
   callers. Defensible, but worth a `// design note:` comment in
   `orders.rs` at some point, or tightening the helper's visibility if
   no external caller is intended. Not required for PR-1.

2. **`let _ = best_quote;` at orders.rs:56** under
   `strict-market-reject` is an explicit silencing of an unused
   parameter in the strict path. Acceptable; standard Rust idiom.

3. **`(x * 100.0) as i64` patterns remain** in `broker/src/ibkr/client.rs`
   at lines 84, 138, 140, 141, 185, 186, 187. These are the H3
   security finding and are slated for PR-20 (`f64_cents_checked`).
   Confirmed not in PR-1 scope. Tracking as a blocker for v0.10, not
   v0.9.3.

4. **`src/stats.rs:173` `0.999_999_999_999_809_93`** TODO is recorded in
   codex_status.md as out-of-scope. Agreed; leave it until whichever PR
   audits `src/stats.rs` gets scheduled. Add to the P1 stats-module
   cleanup candidates (PR-9 or PR-17 are the closest matches, though
   neither touches line 173 directly).

**Self-audit reconciliation.** Codex's self-audit paragraph correctly
anticipated the "diverged from the plan's sample target block" concern
and pre-rebutted it by citing the Investigation instruction. I find the
rebuttal sound. `Mutex<HashMap<Symbol, BestQuote>>` instead of `dashmap`
is the correct call per §0.5 / §C.5 (no new crates without listing).

**Next action:** Codex may proceed to PR-2
(`feat(broker): deterministic client-order-ids for idempotent retries`).
PR-2 builds directly on the broker types touched here and should read
`broker/src/types.rs` before modifying `BrokerOrder`.

## PR-2: feat(broker): deterministic client-order-ids for idempotent retries

- Started: 2026-04-21 13:20
- Completed: 2026-04-21 14:15
- Commit SHA: `176a5d0dc8407eacfa82a55408a63a4d431bc237`
- Files touched: 15 files (+294/-10)
- Diff stat:
  - `CHANGELOG.md` | 7 insertions
  - `broker/Cargo.toml` | 4 changed
  - `broker/src/binance/client.rs` | 8 insertions
  - `broker/src/binance/mod.rs` | 1 insertion
  - `broker/src/ibkr/orders.rs` | 12 changed
  - `broker/src/mock.rs` | 5 insertions
  - `broker/src/types.rs` | 59 insertions
  - `broker/tests/broker_idempotency.rs` | 56 insertions
  - `broker/tests/ibkr_market_order_bounds.rs` | 1 insertion
  - `python/nanobook.pyi` | 4 changed
  - `python/src/broker.rs` | 14 changed
  - `rebalancer/src/broker.rs` | 5 changed
  - `rebalancer/src/execution.rs` | 22 changed
  - `rebalancer/src/target.rs` | 32 insertions
  - `rebalancer/tests/idempotency.rs` | 74 insertions
- Review commands (Codex's run):
  - `rg -nU 'pub struct ClientOrderId' broker/src/types.rs` → PASS (1 match)
  - `rg -n '#\[derive\(.*Debug.*Clone.*PartialEq.*Eq.*Hash' broker/src/types.rs` → PASS (`ClientOrderId` derive present; command also matches `OrderId`)
  - `rg -n 'pub fn derive\(scope: &str' broker/src/types.rs` → PASS (1 match)
  - `rg -n 'order_ref|orderRef' broker/src/ibkr/orders.rs` → PASS (2 matches)
  - `rg -n 'newClientOrderId' broker/src/binance/client.rs` → PASS (2 matches)
  - `rg -n 'ClientOrderId::derive' rebalancer/src/execution.rs` → PASS (1 match)
  - `git diff HEAD~1 -- Cargo.lock | grep -E '^\+name = ' || true` → PASS (no new lockfile entries)
  - `cargo test --package nanobook-broker broker_idempotency` → PASS (5/5 targeted tests)
  - `cargo test --package nanobook-rebalancer idempotency` → PASS (2/2 integration tests plus matching target unit test)
  - `rg -n 'client_order_id' python/nanobook.pyi` → PASS (2 matches)
  - `cargo fmt --all -- --check` → PASS
  - `cargo clippy --workspace --all-targets --all-features -- -D warnings` → PASS
  - `cargo test --workspace` → PASS
  - `cargo test --workspace --all-features` → PASS
  - `cd python && maturin develop --release && uv run pytest tests/ -q && cd ..` → PASS (`114 passed, 32 skipped`)
  - `cargo deny check` → PASS (`advisories ok, bans ok, licenses ok, sources ok`; warning-only unmatched license allowances)
- Deviations from contract:
  1. PR-2 started after Ricardo explicitly allowed proceeding while PR-1 approval was being recorded; PR-1 is now marked APPROVED in this file.
  2. `broker/src/types.rs` already had `sha2` as an optional Binance dependency. For `ClientOrderId` to compile outside the Binance feature, `sha2 = "0.10"` was made a normal broker dependency and removed from the `binance` feature list. No new `Cargo.lock` package entries were added.
  3. `rebalancer/src/broker.rs` was touched because the current runtime executes through `BrokerGateway::execute_limit_order`, not by constructing `BrokerOrder` directly in `rebalancer/src/execution.rs`. The trait needed to accept `Option<&ClientOrderId>` so IBKR `orderRef` is actually set.
  4. `rebalancer/src/target.rs` was touched to add optional `metadata.id` and a timestamp fallback for existing target files. Existing target JSON remains compatible; new users can set `metadata.id` for explicit retry scope.
  5. Python `client_order_id` strings are validated with `ClientOrderId::new` and limited to 36 ASCII-safe characters so the same `BrokerOrder` remains safe for Binance's `newClientOrderId` limit.
  6. The implementation commit body contains literal `\n` sequences due to shell quoting, although the subject and content match the requested template semantically. I did not amend after creating the commit.
- TODOs discovered (out of scope):
  - `BinanceClient::submit_order` now has eight arguments and uses a narrow `#[allow(clippy::too_many_arguments)]`; a future cleanup could introduce a request struct if this API grows again.
- Self-audit: The main risk is the rebalancer compatibility choice. The plan says to derive the id from `target.metadata.id`, but existing target files had no metadata field. I added `metadata.id` as optional and fall back to the target timestamp to preserve existing configs while still producing stable IDs across retries. Review should check whether timestamp fallback is acceptable or whether P0 should require an explicit schedule id. The broker path itself is straightforward: `ClientOrderId` derives a 32-char SHA-256 prefix, IBKR writes it to `order_ref`, Binance writes it to `newClientOrderId`, and the mock records it for regression tests.

### Review of PR-2 (commit 176a5d0dc8407eacfa82a55408a63a4d431bc237) — APPROVED

Reviewer: Claude (Opus 4.7), session 2026-04-21.

**Verdict: APPROVED.** The `ClientOrderId` type is correctly defined,
deterministically derived, threaded through both IBKR `orderRef` and
Binance `newClientOrderId`, validated at the Python boundary, and
tested in both the broker crate and the rebalancer crate. The
`target.metadata.id` scope with timestamp fallback preserves idempotency
across crash-retry.

**Review commands re-run independently (all green):**

- `rg -nU 'pub struct ClientOrderId' broker/src/types.rs` → 1 match (line 53) ✓
- `rg -n 'pub fn derive\(scope: &str' broker/src/types.rs` → 1 match (line 56) ✓
- `rg -n 'order_ref|orderRef' broker/src/ibkr/orders.rs` → 2 matches (lines 108, 161) ✓
- `rg -n 'newClientOrderId' broker/src/binance/client.rs` → 2 matches (lines 143, 157) ✓
- `rg -n 'ClientOrderId::derive' rebalancer/src/execution.rs` → 1 match (line 73) ✓
- `rg -n 'client_order_id' python/nanobook.pyi` → 2 matches ✓
- `git diff 55e6bb9..176a5d0 -- Cargo.lock | grep -E '^\+name = '` → no new lockfile entries ✓
- `cargo test --package nanobook-broker --test broker_idempotency` → 5/5 PASS
- `cargo test --package nanobook-rebalancer --test idempotency` → 2/2 PASS
- `cargo test --workspace --all-features` → PASS
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` → clean
- `cargo fmt --all -- --check` → clean
- `git show --stat 176a5d0` → 15 files, all within scope or documented deviations.

**Idempotency semantics — audited and correct.**

The central design call was whether `TargetSpec::idempotency_scope()`
preserves crash-retry idempotency when `metadata.id` is absent. It does:

- `TargetSpec::timestamp` (`rebalancer/src/target.rs:13`) is a user-supplied
  `DateTime<Utc>` parsed from `target.json`, NOT `SystemTime::now()`.
- If the CLI crashes mid-rebalance and the user re-runs against the same
  `target.json`, the file's `timestamp` is stable → `idempotency_scope`
  returns the same RFC3339 string → `ClientOrderId::derive` returns the
  same hex digest → broker-side dedup rejects the duplicate.
- If the user regenerates `target.json` with a new timestamp, they
  correctly get new `ClientOrderId`s — which is the right semantics for
  a new decision batch, not a retry.

The canonical form inside `ClientOrderId::derive` uses null separators
(`scope || \0 || symbol || \0 || side || \0 || qty_le_bytes`), which
prevents prefix-collision (e.g., `("ab", "c")` vs `("a", "bc")`). This
is the right construction.

32-char hex digest fits both Binance's 36-char `newClientOrderId` limit
and IBKR's 40-char `orderRef` limit. Python-supplied strings are routed
through `ClientOrderId::new` (`python/src/broker.rs:150-153`), which
enforces 1..=36 ASCII-safe chars and raises `PyValueError` otherwise.

**Deviations accepted:**

1. **PR-2 started while PR-1 approval was being recorded.** Ricardo
   explicitly authorized. Accepted.

2. **`sha2` moved from optional Binance dep to normal broker dep.** The
   plan said "Add `sha2 = "0.10"` to `broker/Cargo.toml` dependencies"
   — which Codex did — but also removed it from the `binance` feature
   list because `ClientOrderId::derive` is always available (not feature-
   gated). Correct. Cargo.lock unchanged (sha2 was already a transitive
   dependency). Accepted.

3. **`rebalancer/src/broker.rs` touched.** The `BrokerGateway` trait
   needed to accept `Option<&ClientOrderId>` so the IBKR `orderRef` is
   actually set at submission time. Legitimate wiring; the contract's
   reference to `rebalancer/src/execution.rs` implied this trait
   adjustment. Accepted.

4. **`rebalancer/src/target.rs` touched: added `metadata.id` and
   timestamp fallback.** The contract said "If `target.metadata.id`
   is not already in the TargetSpec, add it in this PR — it's a
   minimal, forward-compatible config field." Codex did this with
   `#[serde(default)]` so existing target.json files remain parseable;
   the timestamp fallback preserves idempotency when `metadata.id` is
   empty. Sound. Accepted.

5. **Python `client_order_id` validation routed through
   `ClientOrderId::new`.** Enforces charset + length. Matches the
   broker-side expectations. Accepted.

6. **Commit message body contains literal `\n` sequences** (line 3 of
   `176a5d0` body). This is a real cosmetic defect: `git log --oneline
   -B` shows the body as one long paragraph. Codex correctly refused to
   amend per §C.10. Not a blocker. Ricardo may `git rebase -i` locally
   to fix the commit message before v0.9.3 tagging if cosmetic polish
   matters. I am NOT requesting a rebuttal because (a) §C.10 forbids
   amending and (b) the content is semantically faithful to the
   template. Flag for the release-prep PR (PR-6).

**Non-blocking observations (follow-up candidates):**

1. **Tests do not verify that `side` or `qty` changes produce different
   IDs.** The canonical form includes them, so correctness is implied
   by the construction, but explicit proptest coverage would strengthen
   the contract. Add to PR-31 release-prep or a dedicated test pass.

2. **`BinanceClient::submit_order` now takes 8 arguments** with a narrow
   `#[allow(clippy::too_many_arguments)]`. Codex's TODO is correct;
   consider a request-struct refactor when the Binance path grows
   further.

3. **`derive_client_order_id` lives in the rebalancer**
   (`rebalancer/src/execution.rs:64-79`), not in `broker`. This is
   the right layering — the rebalancer is the one that knows about
   `RebalanceOrder` and `Action`. Noting for PR-14 (STP policy) which
   may also need to thread `OrderOwner` through this call site.

4. **`ClientOrderId` only records 16 of 32 SHA-256 bytes.** 128 bits of
   entropy is more than enough for order-level collision resistance
   within a single trading session (birthday collision at ~2^64 orders).
   No concern.

5. **PR-1 follow-up TODOs still open:** `src/stats.rs:173`
   false-positive TODO; dead-error-path in `orders::submit_order`
   Market branch; H3 float-cents truncations. None addressed in PR-2
   and none in scope. Carried forward.

**Self-audit reconciliation.** Codex's self-audit worried that the
timestamp fallback might not preserve idempotency. It does, as explained
above — the timestamp is a user-supplied field, not a live clock. The
design is correct and matches the plan's intent. Codex was appropriately
cautious but the worry was unfounded.

**Next action:** Codex may proceed to PR-3
(`refactor(optimize): rename CVaR/CDaR to honest names`). PR-3 is
independent of PR-1 and PR-2; it touches `src/optimize.rs` and the
Python bindings. No coordination with PR-1 or PR-2 required.

**Plan grep-pattern tightening note:** the PR-1 false positive
(`rg '999_999|999,999' src/` matching `0.999_999_999_999_809_93`) was
resolved in the plan by anchoring to `999_999\.99|999,999\.99`. Future
PRs inherit this convention.
