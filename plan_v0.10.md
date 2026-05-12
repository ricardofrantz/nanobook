# nanobook v0.10 — Hardening Release Plan

**Date:** 2026-04-21 (reviewed + expanded)
**Target version:** v0.10.0
**Timeline:** 4–6 weeks realistic, 2–4 weeks optimistic
**Baseline:** v0.9.3 (shipped 2026-04-21, commit `865ad6f`)

**Theme:** Zero new features. Every change is a correctness fix, a
security hardening, or a test that would have caught an existing bug.

---

## 0. Workflow

Worked directly — no separate executor, no review status files.
Claude writes the code, runs tests, commits, moves on. Ricardo
intervenes when direction needs to change.

### 0.1 Ground rules (distilled from v0.9.3 lessons)

- **Small commits per logical unit.** One fix per commit. Don't bundle.
- **Conventional Commits** style: `fix:`, `feat:`, `refactor:`, `docs:`,
  `chore:`, `ci:`, `test:`. Match existing repo history.
- **Always `git commit -F <file>`** for any body with backticks,
  newlines, or special characters. `git commit -m "$(cat <<EOF ...)"`
  is a trap: backticks get stripped, newlines collapse.
- **Named staging only.** Never `git add -A` or `git add .`. Always
  `git add <explicit paths>`.
- **Never weaken a test to pass.** A red test is a signal. Diagnose.
- **Per-commit verification** on touched scope (not full workspace each
  time — that's minutes of overhead per commit):
  ```bash
  cargo fmt --all -- --check
  cargo clippy --package <affected> --all-targets --all-features -- -D warnings
  cargo test --package <affected>
  ```
- **Before pushing a batch** run full workspace:
  ```bash
  cargo test --workspace --all-features
  cargo clippy --workspace --all-targets --all-features -- -D warnings
  cd python && maturin develop --release && uv run pytest tests/ -q && cd ..
  cargo deny check
  ```
- **Use `cargo build --workspace`** for iteration (release build invokes
  `python/build.rs`; default build skips it, faster).
- **Rollback.** If a commit breaks the build: `git revert <sha>` (not
  `git reset --hard`). If a commit breaks a subtle numerical test:
  write a minimal regression test FIRST, then revert and re-approach.

### 0.2 Commit-message templates

For simple commits:
```
<type>(<scope>): <subject, imperative, under 72 chars>

<one-paragraph body explaining why, wrapped at 72>

<optional trailer: Security-X, Numerical-Y, etc.>
```

For larger commits (N1, N2, N8, N10, S1, S5):
```
<type>(<scope>): <subject>

<2–3 sentence motivation — the bug, the blast radius, the user
impact. Cite the audit or observation that flagged it.>

Changes:
- <specific change 1>
- <specific change 2>

Testing:
- <test 1 added/updated>
- <test 2 added/updated>

Breaking: <description, or "none">.
```

---

## 1. Version bumps (applied in D1 closer, NOT per-item)

| Crate | v0.9.3 | v0.10.0 | Reason |
|---|---|---|---|
| `nanobook` | 0.9.3 | 0.10.0 | Welford variance; `CVaRMethod` enum (BREAKING — default numeric output changes); `StpPolicy` enum added; Sortino ddof default change (BREAKING). |
| `nanobook-broker` | 0.4.0 | 0.5.0 | rustls default (BREAKING for native-tls users); logging downgrades; `f64_cents_checked` helper; `ZeroizeOnDrop`. |
| `nanobook-risk` | 0.4.1 | 0.5.0 | `RiskEngine::new` returns `Result` (BREAKING). |
| `nanobook-rebalancer` | 0.5.0 | 0.6.0 | Downstream of broker + risk; audit-path allowlist (BREAKING for configs pointing outside CWD). |
| `nanobook-python` | 0.9.3 | 0.10.0 | Surfaces `CVaRMethod`, `StpPolicy`, updated `RiskEngine::new` error path. |

### 1.1 Final feature matrix (v0.10.0)

| Crate | Feature | Default | Purpose |
|---|---|---|---|
| `nanobook` | `event-log` | yes | Event journal (unchanged from v0.9.3) |
| `nanobook` | `serde` | no | Serde derives on public types |
| `nanobook` | `persistence` | no | Event replay via serde_json |
| `nanobook` | `portfolio` | no | Portfolio simulator (requires for optimizer use) |
| `nanobook` | `parallel` | no | Rayon-based parallel backtests |
| `nanobook` | `itch` | no | ITCH binary parser |
| `nanobook-broker` | `rustls` | **yes** (new default) | Pure-Rust TLS via reqwest+rustls |
| `nanobook-broker` | `native-tls` | no | System OpenSSL for FIPS users |
| `nanobook-broker` | `ibkr` | yes | IBKR adapter (unchanged) |
| `nanobook-broker` | `binance` | no | Binance adapter (unchanged) |
| `nanobook-broker` | `strict-market-reject` | no | Reject market orders entirely (unchanged from v0.9.3) |

No new public features added beyond what the items below document.

---

## 2. Work items

Each item is a single logical commit unless noted. Numbered for
tracking. Subjects in backticks. Ordering in §3.

### N — Numerical correctness

#### N1. `fix(metrics,indicators): Welford variance (O(window)-per-step)`

**Priority.** Highest-impact correctness fix in P1. Rolling variance
using `sum_sq - sum²/k` suffers catastrophic cancellation on high-price,
low-variance series. Bollinger bands and rolling Sharpe silently
collapse to zero on any stock trading above ~$500 with sub-cent ticks.

**Files.**

- `src/portfolio/metrics.rs:340-370` (the `rolling_window_compute`
  helper and `compute` closures).
- `src/indicators.rs:70-92` (`rolling_std_pop`).
- `tests/catastrophic_cancellation.rs` (new).

**Design correction from initial plan.** The initial plan said
"maintain `(mean, m2, k)` + reverse Welford on evict." This is
**wrong** — reverse Welford is numerically unstable and reintroduces
cancellation through a different path (subtracting nearly-equal
accumulated moments).

The correct approach for sliding windows with bounded window size (≤
several hundred):

1. Drop the O(1) sliding state entirely.
2. Recompute Welford freshly over the window slice on each step.
3. O(window) per step, O(n·window) total. For daily 63-day vol over
   10 years (2520 steps), ~158k f64 ops — negligible.

```rust
/// Population variance (ddof=0) via Welford's online algorithm.
/// Returns 0.0 for slices of length < 2.
#[inline]
fn welford_variance(slice: &[f64]) -> f64 {
    let mut mean = 0.0_f64;
    let mut m2 = 0.0_f64;
    let mut n = 0.0_f64;
    for &x in slice {
        n += 1.0;
        let delta = x - mean;
        mean += delta / n;
        let delta2 = x - mean;
        m2 += delta * delta2;
    }
    if n < 2.0 { 0.0 } else { m2 / n }  // population variance
}

/// Sample variance (ddof=1). Use for most rolling statistics.
#[inline]
fn welford_variance_sample(slice: &[f64]) -> f64 {
    let mut mean = 0.0_f64;
    let mut m2 = 0.0_f64;
    let mut n = 0.0_f64;
    for &x in slice {
        n += 1.0;
        let delta = x - mean;
        mean += delta / n;
        let delta2 = x - mean;
        m2 += delta * delta2;
    }
    if n < 2.0 { 0.0 } else { m2 / (n - 1.0) }
}
```

Rewrite `rolling_window_compute` to iterate window slices:
```rust
for i in (window - 1)..n {
    let slice = &values[i + 1 - window..=i];
    let (mean, var) = welford_mean_variance(slice);
    out[i] = compute(mean, var);
}
```

Note the `compute` closure signature changes from `(sum, sum_sq, k)` to
`(mean, var)` — adjust all call sites (`rolling_sharpe`,
`rolling_volatility`, etc.).

**Tests.**

```rust
// tests/catastrophic_cancellation.rs
#[test]
fn rolling_std_does_not_collapse_on_high_mean_low_variance() {
    use nanobook::indicators::rolling_std_pop;
    let values: Vec<f64> = (0..100).map(|i| 1000.0 + 1e-9 * i as f64).collect();
    let stds = rolling_std_pop(&values, 20);
    // Last 81 elements are non-NaN windows; all must be strictly positive.
    // Under the old formula, all are exactly 0.0 via silent .max(0.0) clip.
    for (i, s) in stds.iter().enumerate().skip(19) {
        assert!(
            *s > 1e-12,
            "idx {i}: expected s > 1e-12 for non-constant series, got {s}"
        );
    }
}

#[test]
fn rolling_sharpe_nonzero_for_perturbed_series() {
    use nanobook::portfolio::metrics::rolling_sharpe;
    // Mean return 10 bps, tiny perturbation. Old formula returns NaN or 0.
    let returns: Vec<f64> = (0..100).map(|i| 0.001 + 1e-12 * i as f64).collect();
    let sharpes = rolling_sharpe(&returns, 20, 252);
    for (i, s) in sharpes.iter().enumerate().skip(19) {
        assert!(s.is_finite() && *s > 0.0, "idx {i}: {s}");
    }
}
```

**Acceptance.**

- `rg 'sum_sq' src/` returns zero hits in live code (tests may
  reference in comments).
- Existing tests still pass — the numerical change is <1 ULP for
  well-conditioned series.
- Parity test N10 includes a rolling-std check against numpy.

**Test command.** `cargo test --test catastrophic_cancellation`.

**Commit message body:**
```
fix(metrics,indicators): Welford variance (O(window)-per-step)

Rolling variance using `sum_sq - sum²/k` suffers catastrophic
cancellation on high-mean, low-variance series. On any stock trading
above ~$500/share with sub-cent ticks, `rolling_std_pop` and
`rolling_sharpe` silently return 0 via `.max(0.0)` clipping.

Replace the O(1)-sliding-sum approach with O(window) Welford recompute
per step. For typical window sizes (≤252), overhead is negligible.
Reverse Welford is not used because it is itself unstable.

Changes:
- welford_variance / welford_variance_sample helpers in a shared
  numerics module
- rolling_window_compute rewritten to recompute per window
- rolling_std_pop rewritten to use Welford directly

Testing:
- tests/catastrophic_cancellation.rs regression tests
- existing rolling_* tests still pass (<1 ULP difference on
  well-conditioned series)

Breaking: none (output changes are <1 ULP except on pathological
high-mean low-variance series where old output was silently zero).
```

#### N2. `fix(metrics): historical CVaR with CVaRMethod enum`

**Priority.** Correctness + API honesty.

**Files.** `src/portfolio/metrics.rs:238-269` (`compute_cvar`).

**Current semantics (audited from code, lines 242-268).** `compute_cvar(returns, alpha)` treats `alpha` as the **tail probability** (e.g., `alpha=0.05` → 5% lower tail). It uses `norm_ppf(alpha)` to compute a parametric VaR threshold from the sample mean/variance, then averages returns strictly below that threshold.

**Problem.** This is a hybrid parametric-threshold + empirical-tail
estimator. quantstats, scipy, and every practitioner expect pure
**historical** (empirical) CVaR: pick the lowest `alpha` fraction of
returns and average them.

**Change.**

```rust
/// Method for computing Conditional Value at Risk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CVaRMethod {
    /// Empirical: mean of the lowest `alpha` fraction of returns.
    /// Matches `quantstats.stats.expected_shortfall` and `scipy`
    /// percentile-based CVaR.
    #[default]
    Historical,
    /// Parametric: compute a normal-distribution VaR threshold, then
    /// average empirical returns below it. The v0.9 default; kept for
    /// backward compatibility.
    ParametricNormal,
}

/// Conditional Value at Risk (a.k.a. Expected Shortfall).
///
/// `alpha` is the tail probability (e.g., `0.05` for 5% tail). Result
/// is negative-signed (a "loss" return).
pub fn compute_cvar(returns: &[f64], alpha: f64, method: CVaRMethod) -> f64 {
    if returns.is_empty() || !(0.0..1.0).contains(&alpha) || alpha == 0.0 {
        return 0.0;
    }
    match method {
        CVaRMethod::Historical => {
            let mut sorted: Vec<f64> = returns.iter().copied()
                .filter(|r| r.is_finite()).collect();
            sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
            if sorted.is_empty() { return 0.0; }
            // Lower tail: fraction alpha of the data.
            let tail_n = ((sorted.len() as f64) * alpha).ceil() as usize;
            let tail_n = tail_n.max(1).min(sorted.len());
            sorted[..tail_n].iter().sum::<f64>() / tail_n as f64
        }
        CVaRMethod::ParametricNormal => {
            /* existing logic from metrics.rs:247-268 */
        }
    }
}
```

Update `PortfolioMetrics::cvar` callers to use `CVaRMethod::Historical`
by default. Add `compute_cvar_with(returns, alpha, method)` for
explicit opt-in. Python binding exposes both.

**Tests.** Parity test N10 asserts `Historical` CVaR matches
`quantstats.stats.expected_shortfall(returns, confidence=1-alpha)` to
1e-6 on a seeded 500-point series.

**Breaking.** Default numeric output changes. **Document prominently in
CHANGELOG with a comparison table** (parametric vs historical values for
a common dataset).

**Test command.** `cargo test --test reference_parity -- cvar`.

#### N3. `fix(stats): NaN propagation in rankdata`

**Files.** `src/stats.rs:17-50` (`rankdata`).

**Change.** At function entry:
```rust
if xs.iter().any(|v| v.is_nan()) {
    return vec![f64::NAN; xs.len()];
}
```

Document: "NaN inputs propagate — if any input is NaN, the entire
output is NaN. This matches `scipy.stats.rankdata(..., nan_policy='propagate')`."

**Tests.**
```rust
#[test]
fn nan_in_input_propagates_to_all_ranks() {
    let xs = vec![1.0, 2.0, f64::NAN, 3.0];
    assert!(nanobook::stats::rankdata(&xs).iter().all(|r| r.is_nan()));
}
```

Plus proptest invariant: `forall xs: xs.contains(NaN) => rankdata(xs).iter().all(|r| r.is_nan())`.

**Test command.** `cargo test --package nanobook --lib stats`.

#### N4. `fix(metrics): Sortino ddof=0 default`

**Files.** `src/portfolio/metrics.rs:118-138`.

**Current.** Downside deviation uses `/ (n - 1)` (Bessel correction,
ddof=1). Most practitioner libraries (quantstats default) use `/ n`
(ddof=0).

**Change.** Switch to ddof=0 as default. Add `fn compute_sortino_ddof(returns, rf, periods, ddof: u32)` for explicit opt-in.

**Tests.** Parity against quantstats on a seeded 252-point series.

**Breaking.** Sortino value shifts. For 252 returns the shift is
`sqrt(252/251) ≈ 1.00199` — small but visible. Document in CHANGELOG.

**Test command.** `cargo test --test reference_parity -- sortino`.

#### N5. `fix(stop): rename Atr → SmaAbsChange (honest)`

**Files.** `src/stop.rs:328-340` (`compute_atr_offset`).

**Reality check.** The stop module has no OHLC data — only trade
prices. `TrailMethod::Atr` currently computes `sum(|Δprice|) / period`:
that is a simple moving average of absolute trade-price changes, NOT
Wilder's ATR (which requires true range = max of `high-low`,
`|high-prev_close|`, `|low-prev_close|`).

The initial plan said "call through to `indicators::atr`" — **that
would not work** because `indicators::atr` requires OHLC arrays and the
stop module doesn't have them.

**Honest change.** Rename the variant:

```rust
pub enum TrailMethod {
    /// Trail by fixed cent offset.
    Fixed(i64),
    /// Trail by percentage of watermark.
    Percentage(f64),
    /// Trail by a multiple of the simple moving average of absolute
    /// trade-price changes over `period` recent trades. This is NOT
    /// Wilder's Average True Range — no OHLC is available at the stop
    /// module. For true ATR, pre-compute via `indicators::atr` and use
    /// `Fixed(atr * multiplier)`.
    SmaAbsChange { multiplier: f64, period: usize },
    // (Atr variant kept as #[deprecated] shim for one minor.)
}
```

Add deprecated shim:
```rust
#[deprecated(since = "0.10.0", note = "use TrailMethod::SmaAbsChange; this variant is not Wilder's ATR")]
#[allow(non_camel_case_types)]
pub const Atr: TrailMethodShim = ...;  // or similar pattern
```

If Rust enum variants can't be individually `#[deprecated]`, leave the
`Atr` variant in place with an updated doc-comment flagging the
deprecation, and emit a `log::warn!` on first construction.

**Tests.** Update existing tests using `TrailMethod::Atr` to use
`TrailMethod::SmaAbsChange`.

**Test command.** `cargo test --package nanobook stop`.

**Breaking.** Yes — the variant name changes. The doc-comment already
lied about the semantics; callers relying on the name believed they
were getting Wilder ATR.

#### N6. `fix(stop): round Percentage trail offset`

**Files.** `src/stop.rs:267`.

**Change.** `(watermark as f64 * pct) as i64` → `(watermark as f64 * pct).round() as i64`.

**Test.** `watermark = 100_00` ($100.00), `pct = 0.001` (0.1%) → offset
must be `10` (1¢) cents, not `9` due to FP representation
(`100_00 * 0.001 = 9.9999999...` truncates to 9).

**Test command.** `cargo test --package nanobook stop::tests::percentage_trail_rounds`.

#### N7. `test(exchange): document FOK id-without-order contract`

**Files.**

- `src/exchange.rs` (update `submit_limit` doc).
- `tests/proptest_invariants.rs` (new invariant).

**Change.** Doc on `submit_limit`:

```
/// An order rejected under TimeInForce::FOK (no full fill available)
/// returns a valid `SubmitResult.order_id` but that id is NOT
/// retrievable via `get_order` — it is logically non-existent.
///
/// Callers MUST check `SubmitResult.status == SubmitStatus::Rejected`
/// before querying `get_order(id)`.
```

Proptest: generate arbitrary FOK submissions against an empty book,
assert rejections have `get_order(id) == None` but
`SubmitResult.order_id.is_some()`.

**Test command.** `cargo test --test proptest_invariants -- fok`.

#### N8. `feat(matching): self-trade prevention policy`

**Priority.** Safety. No SemVer bump for adding — the new field is optional.

**Files.**

- `src/order.rs` (add `pub owner: Option<OrderOwner>` — note field
  ordering matters for struct-literal callers).
- `src/matching.rs` (matcher STP logic).
- `src/exchange.rs` (`Exchange::with_stp_policy(policy)` builder).
- `src/lib.rs` (export `OrderOwner`, `StpPolicy`).
- `tests/stp_policy.rs` (new).

**Change.**

```rust
// src/order.rs
/// Opaque identifier for the party that submitted an order. Use to
/// enable self-trade prevention. Values are opaque and caller-assigned;
/// the engine does not interpret them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OrderOwner(pub u32);

pub struct Order {
    // ...existing fields...
    pub owner: Option<OrderOwner>,  // None = opt out of STP
}

// src/exchange.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StpPolicy {
    /// No self-trade prevention — same-owner orders may cross. Default.
    #[default]
    Off,
    /// Cancel the incoming order (remainder), leave resting intact.
    CancelNewest,
    /// Cancel the resting order; incoming order re-attempts match.
    CancelOldest,
    /// Decrement smaller side, cancel remainder of smaller, preserve
    /// the larger order.
    DecrementAndCancel,
}
```

When the matcher is about to cross two orders, check owner equality:
- If either side has `owner = None`, matching proceeds (opt-out).
- If owners match and policy is `Off`, matching proceeds (explicit).
- If owners match and policy is anything else, apply the policy.

**Tests** (matrix):
- Same-owner buy 100 + rest sell 100: under each policy, assert book
  state + trade tape.
- Different-owner: all policies must produce the normal trade.
- `owner = None` on either side: all policies must produce the normal
  trade.

**Reference.** CME Rule 536 (self-match prevention); NASDAQ OUCH 4.x STP spec.

**Breaking (minor).** Struct-literal construction of `Order` now
requires the `owner` field. Callers using builders or `Default::default`
are unaffected. We can mitigate by making `Order` `#[non_exhaustive]`
in a separate cleanup if needed.

**Test command.** `cargo test --test stp_policy`.

#### N9. `fix(optimize): relative ridge on covariance matrix`

**Files.** `src/optimize.rs:240` (ridge term in covariance).

**Change.** Replace `1e-10 * I` with `(1e-6 * trace(Σ) / n) * I`
(relative ridge). Fixed 1e-10 is too small for correlated assets —
scales as O(1) while covariance eigenvalues scale as O(σ²) which can be
O(10⁻⁴) or smaller on daily returns.

**Tests.**
```rust
#[test]
fn min_variance_on_perfectly_correlated_assets_returns_finite() {
    // 3 assets with perfect correlation, σ ≈ 1%/day
    let returns = /* 3 assets with identical shifted-copy returns */;
    let w = nanobook::optimize::optimize_min_variance(returns);
    assert!(w.iter().all(|v| v.is_finite()));
    assert!((w.iter().sum::<f64>() - 1.0).abs() < 1e-6);
}
```

**Test command.** `cargo test --package nanobook --lib optimize::tests::ridge`.

#### N10. `test(parity): reference-parity golden-file harness`

**Highest-leverage single investment in P1.** One harness, checked-in
JSON fixture, per-function Rust tests.

**Files.**

- `tests/parity/README.md` (new — documents regeneration procedure).
- `tests/parity/generate_golden.py` (new — run manually).
- `tests/parity/requirements.txt` (pinned scipy/talib/quantstats versions).
- `tests/parity/golden.json` (generated, checked in).
- `tests/reference_parity.rs` (new — read-only test consumer).

**Regeneration procedure** (`tests/parity/README.md`):

```markdown
# Reference-parity golden fixture

To regenerate after bumping reference library versions:

1. Install system TA-Lib C library:
   - macOS: `brew install ta-lib`
   - Ubuntu: `apt-get install libta-lib-dev`
2. `uv pip install -r tests/parity/requirements.txt`
3. `uv run python tests/parity/generate_golden.py`
4. Commit `golden.json` alongside the library bumps.

The fixture is pinned to seed=42 and a specific version set. Never
regenerate without a deliberate reason (CI is read-only).
```

**`generate_golden.py`** (see the initial plan's code block — keep).

**`requirements.txt`:**
```
numpy>=1.26,<2
scipy>=1.14,<2
TA-Lib-binary>=0.5,<1   ; sys_platform != "win32"
quantstats>=0.0.62,<0.1
```

**Rust test skeleton.** Exactly as sketched in the initial plan, with
one `#[test]` per reference function. Ship the first six:

1. `rsi_matches_talib`
2. `atr_matches_talib`
3. `sharpe_matches_quantstats`
4. `sortino_matches_quantstats`
5. `max_drawdown_matches_quantstats`
6. `empirical_cvar_matches_quantstats`

Subsequent items (N2, N4) add more reference tests as they land.

**Acceptance.** Rust tests pass at 1e-6 tolerance. Regeneration script
reproduces the checked-in JSON bit-identically.

**Test command.** `cargo test --test reference_parity`.

#### N11. `docs(level): fix order_count doc mismatch`

**Files.** `src/level.rs:49-52`.

**Change.** Doc says "including tombstones"; implementation (`orders.len() - tombstone_count`) excludes them. Correct the doc.

#### N12. `docs(stats): document quintile_spread non-divisible n behavior`

**Files.** `src/stats.rs:283` (`quintile_spread`).

**Change.** Doc-only. Note that when `n % n_quantiles != 0`, middle
elements are excluded; top and bottom groups each receive `floor(n /
n_quantiles)` elements. Justify the choice (consistency with common
quintile-spread convention in factor-research papers).

#### N13. `fix(metrics): guard periods_per_year = 0 in Sharpe/Sortino`

**Files.** `src/portfolio/metrics.rs:90-145` (Sharpe, Sortino sites).

**Change.** Early-return `0.0` (or `NaN`?) if `periods_per_year <=
0.0`. Current behavior produces `NaN` via `0.0.sqrt() / 0.0` — not a
crash, just a silent NaN. Pick one behavior:

- Return `0.0` with a `log::warn!` — defensive.
- Return `NaN` explicitly (document the choice).

Recommend: `0.0` return with a debug-level log (noisy `warn!` on every
metric call is annoying).

**Test.**
```rust
#[test]
fn sharpe_returns_zero_on_nonpositive_periods_per_year() {
    let returns = vec![0.01; 100];
    assert_eq!(nanobook::portfolio::metrics::sharpe(&returns, 0.0, 0.0), 0.0);
    assert_eq!(nanobook::portfolio::metrics::sharpe(&returns, -1.0, 0.0), 0.0);
}
```

#### N14. `docs(indicators): RSI behavior for n <= period`

**Files.** `src/indicators.rs` (docs on `rsi`).

**Change.** Document: "For input length `n <= period`, returns a vector
of `n` NaN values. At least `period + 1` prices are required for the
first non-NaN RSI." TA-Lib compatible.

No code change. One-line commit.

#### N15. `docs(indicators): Bollinger Bands zero-std behavior`

**Files.** `src/indicators.rs` (docs on `bbands`).

**Change.** Document: "If `num_std_up == 0.0` (or equivalently for
`num_std_down`), the returned upper (or lower) band equals the middle
band. No warning is emitted — this is a documented feature for callers
requesting a plain SMA alongside a band."

No code change.

#### N16. `fix(matching): tombstone count on orphan recovery`

**Files.** `src/matching.rs:103-108`.

**Issue.** The orphan-recovery path (when an order id is in the book
but the order struct has been freed) pops with `pop_front(0)`, which
doesn't decrement `tombstone_count` because the orphan isn't technically
a tombstone. Under pathological sequences this could leak tombstone
entries. Edge case — need to reproduce.

**Change.** Audit the path, add a test case that forces orphan
recovery and verifies `level.order_count()` is correct afterwards.
Minimal code change (likely one accounting adjustment); no API change.

**Acceptance.** Proptest invariant: after arbitrary submission
sequences, `level.tombstone_count + level.order_count() == level.raw_len()`.

#### N17. `fix(optimize): project_simplex degenerate-input handling`

**Files.** `src/optimize.rs:328-335` (`project_simplex`).

**Issue.** On degenerate input (all-zero or all-negative vector) the
function falls back to equal-weight. This silently masks upstream bugs
(e.g., an optimizer that converged to a zero gradient).

**Change.** Return `Err(OptimizeError::DegenerateProjection)` instead
of equal-weight. Upstream callers that need equal-weight-on-failure
handle it explicitly.

**Breaking.** `project_simplex` signature changes from `-> Vec<f64>` to
`-> Result<Vec<f64>, OptimizeError>`. May ripple into `optimize_*`
functions — audit and propagate.

**Test.**
```rust
#[test]
fn project_simplex_on_all_zeros_errors() {
    assert!(matches!(
        nanobook::optimize::project_simplex(&[0.0, 0.0, 0.0]),
        Err(OptimizeError::DegenerateProjection)
    ));
}
```

#### N18. `fix(optimize): min_variance convergence diagnostics`

**Files.** `src/optimize.rs:19-36` (`optimize_min_variance`).

**Issue.** Fixed 350-iter budget with `lr *= 0.995` per iter, `tol =
1e-16`. Tolerance never triggers in practice; optimizer burns the full
budget. No signal to the caller about convergence quality.

**Change.** Add `OptimizerOptions { max_iters, tol, verbose }` and
return `OptimizerResult { weights, converged, final_grad_norm,
iters }`. Keep the bare `optimize_min_variance(returns)` as a wrapper
that panics on non-convergence (for backward compat — document) or
returns the result struct.

**Breaking (mild).** New struct is additive, but any caller that
wants convergence info must migrate.

**Test.** Add a reference test against `cvxpy` for a 5-asset portfolio
with known optimal weights; assert agreement to 3 significant figures.

#### N19. `feat(config): deny_unknown_fields on new deserializable types`

**Files.** Any new struct deriving `Deserialize` introduced in P1.
Retrospective audit after all other items land.

**Change.** Apply `#[serde(deny_unknown_fields)]` systematically —
same pattern as PR-5 in v0.9.3.

### S — Security hardening

#### S1. `fix(ssl): default to rustls; native-tls behind feature`

**Priority.** Drops vendored OpenSSL's permanent CVE patch surface.

**Files.** `broker/Cargo.toml`, `.github/workflows/wheels.yml` (if the
Linux wheel currently relies on vendored openssl — verify).

**Change.**

```toml
# broker/Cargo.toml
reqwest = { version = "0.12", default-features = false, features = ["rustls-tls", "json"] }

[features]
default = ["rustls"]
rustls = []
native-tls = ["reqwest/native-tls"]
```

**Verification.**
- `cargo tree -p nanobook-broker | grep -iE 'openssl|native-tls'` →
  no matches under default features.
- `cargo tree -p nanobook-broker | grep -i rustls` → ≥1 match.
- `cargo build -p nanobook-broker --no-default-features --features native-tls` → still compiles.
- `cd python && maturin develop --release` still produces a wheel.

**Breaking.** For callers that relied on system OpenSSL (e.g., using
custom CA bundles via OpenSSL config). Migration: add `--features native-tls`.

**Test command.** `cargo test -p nanobook-broker` (both feature matrices).

#### S2. `fix(broker): f64_cents_checked helper`

**Files.**

- `broker/src/types.rs` (helper + error variants).
- `broker/src/error.rs` (`BrokerError::NonFiniteValue`, `::ValueOutOfRange`).
- `broker/src/binance/mod.rs:58-61,93` (use it).
- `broker/src/ibkr/client.rs:50-51,105-108,152-154` (use it).
- `broker/tests/f64_cents_checked.rs` (new).

**Helper.**

```rust
/// Convert a floating-point dollar value to integer cents, rejecting
/// non-finite and out-of-range inputs.
///
/// Range: ±$9 × 10¹⁰ (i.e., ±$90B). This is well below `i64::MAX`
/// cents (~9.2 × 10¹⁶) and accommodates individual equity positions,
/// account equity, and most crypto holdings. Values outside this range
/// return `Err` — callers needing larger magnitudes should use i128
/// explicitly.
pub fn f64_cents_checked(v: f64) -> Result<i64, BrokerError> {
    if !v.is_finite() {
        return Err(BrokerError::NonFiniteValue(v));
    }
    let cents = v * 100.0;
    if !(-9_000_000_000_000.0..=9_000_000_000_000.0).contains(&cents) {
        return Err(BrokerError::ValueOutOfRange(v));
    }
    Ok(cents.round() as i64)
}
```

**Tests.** NaN, +Inf, -Inf, `f64::MAX`, `f64::MIN`, `0.0`, `-0.0`,
`185.05`, exactly-at-boundary cases (`9e10`, `9e10 + 1`).

**Breaking.** Broker field types that were populated via raw `as i64`
now return `Result`. Upstream error-handling needed. Audit every call
site.

**Test command.** `cargo test -p nanobook-broker --test f64_cents_checked`.

#### S3. `fix(itch): io::Error instead of unwrap on short payload`

**Files.** `src/itch.rs` (all `try_into().unwrap()` sites — 97-118,
123-222).

**Change.**

```rust
fn read_u64_be(slice: &[u8]) -> io::Result<u64> {
    slice.try_into()
        .map(u64::from_be_bytes)
        .map_err(|_| io::Error::new(
            io::ErrorKind::InvalidData,
            "short ITCH payload"
        ))
}
// Similar for u32, u16, i32, i64.
```

Thread through all message parsers.

**Tests.**
```rust
#[test]
fn truncated_add_order_message_returns_err() {
    let truncated = [0x00; 10];  // well under AddOrder's 36-byte payload
    let mut parser = ItchParser::new(&truncated);
    assert!(parser.next_message().is_err());
}
```

Plus property test: arbitrary bytes → parser returns `Err`, never
panics.

**Test command.** `cargo test --package nanobook itch`.

#### S4. `fix(trade,backtest): checked arithmetic on price × quantity`

**Files.** `src/trade.rs:63,89`, `src/backtest_bridge.rs:390-414`.

**Change.** Replace `price.0 * (quantity as i64)` with:

```rust
price.0.checked_mul(quantity as i64)
    .ok_or(Error::NotionalOverflow { price: price.0, quantity })?
```

Add `Error::NotionalOverflow { price: i64, quantity: u64 }` variant.

**Tests.** Property test: `forall price in (Price::MAX/2)..=Price::MAX,
qty in 2..=10`, no panic, `Err` returned.

#### S5. `fix(risk): RiskEngine::new returns Result`

**Files.**

- `risk/src/lib.rs:32` (constructor).
- `risk/src/error.rs` (`RiskError::InvalidConfig` if not present).
- `python/src/risk.rs` (binding — raise `ValueError` on error).
- Update all call sites in `rebalancer/` and `python/`.

**Breaking.** All callers must handle the `Result`. Migration: `.expect("config")` for callers with static configs; proper error propagation for user-supplied configs.

**Test command.** `cargo test --package nanobook-risk`.

#### S6. `fix(broker): warn on unparseable broker floats`

**Files.**

- `broker/src/binance/mod.rs:85,86,117-119`.
- `broker/src/ibkr/client.rs:92`.

**Correction from initial plan.** Broker already uses `log = "0.4"` as
an optional dep (confirmed in `broker/Cargo.toml:25`). Use `log::warn!`,
not `tracing::warn!`. No new dependency needed.

**Change.**

```rust
.unwrap_or_else(|e| {
    log::warn!(
        "failed to parse broker-returned float: field={} raw={:?} error={}",
        name, s, e
    );
    0.0
})
```

Promote `log` from optional to always-on for the broker crate (remove
the `optional = true` on line 25 of `broker/Cargo.toml`). This matches
the rebalancer which uses `log` unconditionally.

**Test command.** `cargo test -p nanobook-broker`.

#### S7. `fix(logs,audit): debug-level for equity; 0o600 audit file`

**Files.**

- `broker/src/ibkr/client.rs:52-55,91,111-114,190` (logging).
- `rebalancer/src/audit.rs:37` (chmod).

**Changes.**

1. Downgrade `info!(...)` → `debug!(...)` for position and equity log
   lines.
2. Audit file mode:

```rust
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

let mut opts = OpenOptions::new();
opts.create(true).append(true);
#[cfg(unix)]
{ opts.mode(0o600); }
let file = opts.open(path)?;
```

Document in the audit-file doc-comment: "On Windows, file permissions
are inherited from the parent directory and not restricted by
nanobook. Users on shared Windows systems should set ACLs manually."

#### S8. `fix(rebalancer): audit path sandboxing`

**Files.** `rebalancer/src/audit.rs:33-37`.

**Change.**

```rust
fn validate_audit_dir(dir: &Path, workdir: &Path) -> Result<()> {
    let canonical = dir.canonicalize().or_else(|_| {
        // Dir doesn't exist yet — canonicalize its parent.
        dir.parent().unwrap_or(dir).canonicalize().map(|p| p.join(dir.file_name().unwrap_or_default()))
    })?;
    let workdir_canonical = workdir.canonicalize()?;
    if !canonical.starts_with(&workdir_canonical) {
        return Err(Error::AuditPathOutsideWorkdir { path: canonical });
    }
    Ok(())
}
```

**Breaking.** Configs pointing to absolute paths outside CWD will now
error at audit open. Document migration: either move the audit
directory under the working directory or use an explicit allowlist
(future work).

#### S9. `fix(broker): zeroize on drop for broker wrappers`

**Files.**

- `broker/Cargo.toml` (add `zeroize = { version = "1.8", features = ["zeroize_derive"] }`).
- `broker/src/binance/mod.rs:18-19` (derive on `BinanceBroker`).
- `broker/src/ibkr/client.rs` (derive on `IbkrClient`).
- `python/src/broker.rs:238-243` (derive on `PyBinanceBroker`).
- `broker/README.md` (new or update — document the `&str` caveat).

**Change.**

```rust
use zeroize::ZeroizeOnDrop;

#[derive(ZeroizeOnDrop)]
pub struct BinanceBroker {
    // fields holding API keys/secrets
}
```

**Documentation caveat.** PyO3 `&str` parameters live in `PyString`
and cannot be zeroized from Rust. Recommend users pass credentials via
environment variables (so the sensitive bytes never transit through
PyString) — document in `broker/README.md`.

**Test command.** `cargo test -p nanobook-broker`.

#### S10. `feat(config): deny_unknown_fields on all new deserializables`

**Files.** Retrospective audit after all other items land.

**Change.** Any new type deriving `Deserialize` introduced in P1 must
carry `#[serde(deny_unknown_fields)]`. Grep to verify:

```bash
rg -B 2 'derive\([^)]*Deserialize' src/ broker/src/ risk/src/ rebalancer/src/ python/src/ \
  | rg -v 'deny_unknown_fields'
```

Expected: no matches (every Deserialize has the attribute).

### I — Infra / CI / supply chain

#### I1. `ci: permissions lockdown; pinned tool versions`

**Files.** `.github/workflows/ci.yml`, `.github/workflows/release.yml`,
`.github/workflows/wheels.yml`.

**Changes.**

1. Top of `ci.yml`:
   ```yaml
   permissions:
     contents: read
   ```

2. Pin tool installs (update versions to latest at commit time):
   ```yaml
   - run: cargo install cargo-deny --version 0.16.3 --locked
   - run: cargo install cargo-audit --version 0.21.0 --locked
   - run: cargo install cargo-llvm-cov --version 0.6.0 --locked
   ```

3. Remove `|| true` from `release.yml:128-131`. Replace with
   conditional publish:
   ```bash
   current_version="$(cargo pkgid -p nanobook | cut -d'#' -f2)"
   published_version="$(cargo search nanobook --limit 1 | head -1 | sed -E 's/.*"([0-9.]+)".*/\1/')"
   if [ "$current_version" != "$published_version" ]; then
     cargo publish -p nanobook
   fi
   ```

4. `wheels.yml`: verify `attestations: true` is set (confirmed in
   v0.9.3).

**Acceptance.** All workflows still pass on a dry-run PR.

#### I2. `test(fuzz): cargo-fuzz harness on matching engine`

**Files.** `fuzz/` (new via `cargo fuzz init`),
`fuzz/fuzz_targets/fuzz_submit.rs`, `fuzz/fuzz_targets/fuzz_itch.rs`,
`fuzz/README.md`.

**Targets.**

1. **`fuzz_submit.rs`** — `arbitrary::Arbitrary` derives for `Order`;
   submit arbitrary orders against a fresh `Exchange`; assert:
   - No panic.
   - Book invariants hold (bid-ask non-crossed, total quantity
     non-negative, no zero-quantity active orders).
   - `get_order(id)` monotonic with submission order.

2. **`fuzz_itch.rs`** — arbitrary bytes as ITCH payload; assert:
   - No panic (`unwrap` sites already fixed by S3).
   - Parser either returns `Ok(msg)` with well-formed contents or
     `Err(InvalidData)`.

**Not CI-gated.** `fuzz/README.md` documents manual invocation:

```bash
# Nightly Rust required for cargo-fuzz.
rustup toolchain install nightly
cargo +nightly fuzz run fuzz_submit -- -runs=10000000
cargo +nightly fuzz run fuzz_itch -- -runs=10000000
```

#### I3. `test(mutants): cargo-mutants baseline on matching engine`

**Files.** `fuzz/mutants-baseline.md` (report).

**Change.** Run:

```bash
cargo install cargo-mutants --locked --version 25.3.1  # or latest
cargo mutants --package nanobook \
    --file src/matching.rs \
    --file src/exchange.rs \
    --file src/level.rs \
    --timeout 60 \
    --jobs 4
```

Target: ≥85% kill rate (relaxed from the plan's initial 90% — mutants
runtime on this engine is likely 20-40 min, and 85% is a realistic
bar). For any survivors:

- Analyze each surviving mutation — what semantic change does it
  represent?
- If the survivor is a trivially-equivalent mutation (e.g., `.max(0.0)`
  vs `.max(-0.0)`), mark as "expected-survivor" in the report.
- If the survivor is a real gap, add a targeted unit test.

**Acceptance.** `fuzz/mutants-baseline.md` exists with:
- Date of run, `cargo mutants` version.
- Kill rate (≥85%).
- Surviving mutations enumerated with one-line justification each.

### D — Release

#### D0. `bench(v0.10): pre-tag benchmark comparison`

**Files.** `benches/v0.10-comparison.md` (new, dated).

**Change.** Before tagging, run `cargo bench --bench throughput` and
compare against v0.9.3 baseline. Document:
- Regressions >5% in hot paths (expected: Welford N1 adds O(window)
  per step to rolling functions; matching engine should be unchanged).
- Improvements if any.
- Rationale for any regression.

**Acceptance.** No unexplained >5% regression in the matching-engine
hot path (the 120 ns / 8M ops claim must still hold).

#### D1. `docs(release): v0.10.0 hardening release notes + version bump`

**P1 closer.**

Applies:
- Version bumps per §1.
- CHANGELOG consolidation with dated `[0.10.0]` block (template §7).
- README pointer update if any API names changed at the top level.

Commit via `git commit -F <file>` from a pre-written body. No shell
substitution.

#### D2. `docs: cargo-doc spot-check before tagging`

**Files.** None — a pre-tag check, not a commit.

**Check.**

```bash
cargo doc --workspace --no-deps --all-features
# Manually open target/doc/nanobook/index.html and verify:
# - new types (CVaRMethod, StpPolicy, ClientOrderId, OrderOwner) render
# - new deprecated attributes render with the note visible
# - examples still compile (cargo test --doc)
```

If missing docs fail `-D missing-docs` (which we don't currently
enforce — consider adding), fix before tagging.

---

## 3. Ordering & milestones

### 3.1 Week-by-week (realistic estimate: 4-6 weeks)

**Week 1 — numerical foundation.**
- N10 (parity harness, scaffold first — every subsequent numeric fix
  adds a golden comparison).
- N1 (Welford) — highest-impact fix.
- N2 (historical CVaR).
- N3 (NaN rankdata).
- N4 (Sortino ddof).
- N11, N12, N14, N15 (doc-only fixes, batch into one commit).

**Week 2 — numerical cleanup.**
- N5 (stop rename).
- N6 (round trail offset).
- N7 (FOK doc + proptest).
- N9 (relative ridge).
- N13 (periods_per_year guard).
- N16 (tombstone accounting).

**Week 3 — security hardening.**
- S1 (rustls — high-impact, test wheel builds).
- S2 (f64_cents_checked).
- S3 (ITCH io::Error).
- S4 (checked_mul).
- S5 (RiskEngine Result).
- S6 (log::warn).

**Week 4 — more security + infra.**
- S7 (debug logs + 0o600).
- S8 (audit path sandbox).
- S9 (zeroize).
- I1 (CI permissions + pins).
- I3 (mutants baseline — parallel with other work).

**Week 5 — bigger items + fuzz.**
- N8 (STP policy) — biggest new feature in P1, give it space.
- N17 (project_simplex).
- N18 (min_variance convergence).
- I2 (fuzz harness).
- S10 (deny_unknown_fields sweep).
- N19 (same).

**Week 6 — release prep.**
- D0 (benchmark comparison).
- D2 (cargo-doc spot check).
- D1 (version bumps + CHANGELOG + tag).

### 3.2 Dependencies between items

- N1 before N10's rolling-std golden test (the old code returns 0; the
  test would fail pre-fix).
- N2 before N10's CVaR golden test.
- N4 before N10's Sortino golden test.
- S5 before S5-downstream refactors (rebalancer, python bindings).
- S1 before release (changes wheel build path).
- D0, D1, D2 are the release-closer sequence — do last.

### 3.3 Hard-stop triggers (stop and ask)

- N1 Welford recompute adds >5% workspace-test-runtime: investigate
  before proceeding — might need a parallel path.
- N2 historical CVaR differs from quantstats by >1e-4 on the seeded
  fixture: real algorithm bug somewhere, diagnose.
- N8 STP matrix test reveals engine behavior not covered by the 4
  policies (e.g., cross-price partial-match with same-owner tail):
  document and consult Ricardo before extending the policy menu.
- Any regression >5% in matching-engine microbenchmarks: hard stop.

---

## 4. Acceptance gates for v0.10.0

Before tagging, ALL of:

1. `cargo fmt --all -- --check` — clean.
2. `cargo clippy --workspace --all-targets --all-features -- -D warnings` — clean.
3. `cargo test --workspace` — green.
4. `cargo test --workspace --all-features` — green.
5. `cargo deny check` — clean.
6. `cargo build --release --workspace` — clean.
7. `cd python && maturin develop --release && uv run pytest tests/ -q && cd ..` — all Python tests pass.
8. `tests/parity/` — ≥10 golden comparisons, all Rust-side tests pass at 1e-6 tolerance.
9. `fuzz/mutants-baseline.md` — ≥85% kill rate documented, survivors justified.
10. `fuzz/` — both targets exist; spot run of 10k iterations each locally with zero panics.
11. `benches/v0.10-comparison.md` — no unexplained >5% regression in matching-engine hot path.
12. CHANGELOG `[0.10.0]` block — lists every N/S/I/D item, with BREAKING markers on N2, N4, N8 (Order struct), N17, S1, S5, S8, D1.
13. README "What nanobook is NOT" block — still present and current.
14. No new unlisted dependencies. `cargo tree` diff vs v0.9.3 reviewed.

---

## 5. Explicit non-goals for v0.10

Do NOT pursue:

- FIX 4.4 / 5.0 SP2 adapter.
- GPU / FPGA work.
- WASM + TypeScript SDK.
- New venue adapters (Hyperliquid, dYdX, etc.).
- Event-sourced OMS / replay journal (v0.11).
- True LP-based CVaR optimizer (v0.11 behind `cvar-lp`).
- MLE-fitted GARCH (v0.11 behind `garch-mle`).
- Rewriting the commit bodies of PR-2 and PR-5 (cosmetic only; `git
  rebase` rewrites SHAs referenced nowhere outside the local tree
  anyway — leave alone).
- Adding `#[non_exhaustive]` to existing public enums (API sugar; do
  in a dedicated v0.11 API-cleanup release).

---

## 6. Lessons carried from v0.9.3

- **Prefer content-based assertions over count-based or
  formatting-sensitive regex.** Three P0 review-command failures traced
  to rigid counts and multiline regex.
- **Commit bodies: `git commit -F <file>`.** Always.
- **Lockfiles are expected-touched files** on any Cargo metadata
  change. Pre-list.
- **Maturin handles Python extensions; `python/build.rs`** uses
  `pyo3_build_config::add_extension_module_link_args()` for direct
  cargo builds. Already in place.
- **Small regression tests beat broad ones.** One failing
  `[1000.0 + 1e-9 * i]` Welford test catches a real bug no existing
  test hits.
- **`#[serde(deny_unknown_fields)]` is cheap.** Apply to every new
  deserializable.
- **Reverse Welford is unstable.** For sliding windows with bounded
  size, recompute freshly. Don't optimize prematurely.

---

## 7. CHANGELOG template for v0.10.0

```markdown
## [0.10.0] - YYYY-MM-DD — Hardening Release

Hardening release. No new features. Every change is a correctness
fix, security hardening, or test that would have caught an existing
bug.

### Changed (BREAKING)

- **`nanobook` 0.10.0**: `compute_cvar` default method changed from
  parametric-normal to historical (empirical). Callers relying on the
  old behavior must explicitly pass `CVaRMethod::ParametricNormal`.
  See migration note below.
- **`nanobook` 0.10.0**: `Sortino` downside-deviation denominator
  changed from `(n-1)` to `n` to match quantstats convention. Numeric
  output shifts by `sqrt(n/(n-1))`, ≈0.2% for n=252. Callers needing
  Bessel correction can call `compute_sortino_ddof(..., ddof=1)`.
- **`nanobook` 0.10.0**: `Order` struct gains an optional `owner:
  Option<OrderOwner>` field for self-trade prevention. Struct-literal
  construction requires the field; builder-based construction unaffected.
- **`nanobook` 0.10.0**: `project_simplex` returns
  `Result<Vec<f64>, OptimizeError>` instead of `Vec<f64>`. Degenerate
  input previously returned equal-weight silently; now errors.
- **`nanobook-broker` 0.5.0**: default TLS backend switched from
  `native-tls` (vendored OpenSSL) to `rustls`. Callers needing system
  OpenSSL: `--features native-tls`.
- **`nanobook-risk` 0.5.0**: `RiskEngine::new` returns
  `Result<Self, RiskError>` instead of `Self`. Invalid configs
  previously panicked; now return an error.
- **`nanobook-rebalancer` 0.6.0**: audit-log directory must resolve
  to a path inside the working directory. Configs pointing outside
  now error at load time.

### Fixed (Numerical)

- **Catastrophic cancellation in rolling variance** (`src/portfolio/metrics.rs`,
  `src/indicators.rs`): replaced the `sum_sq - sum²/k` formula with
  O(window)-per-step Welford recompute. Previously, rolling std
  silently returned 0 for high-mean low-variance series (e.g., any
  stock over ~$500 with sub-cent ticks). Now correct.
- **CVaR method** (`src/portfolio/metrics.rs`): default method is now
  `Historical` (empirical percentile), matching quantstats and scipy.
  Parametric-normal preserved as `CVaRMethod::ParametricNormal`.
- **Sortino ddof** (`src/portfolio/metrics.rs`): default ddof=0. See
  BREAKING note above.
- **NaN propagation in `rankdata`** (`src/stats.rs`): NaN inputs now
  propagate to NaN ranks. Previously produced silently-corrupt
  Spearman and quintile-spread values.
- **ATR trail semantics** (`src/stop.rs`): `TrailMethod::Atr` renamed
  to `SmaAbsChange` with an honest doc. The stop module has no OHLC;
  it cannot compute Wilder's true-range ATR. For Wilder ATR,
  pre-compute via `indicators::atr` and pass as `Fixed`.
- **Percentage trail rounding** (`src/stop.rs`): trail offset now
  rounds instead of truncating, eliminating systematic ≤1¢
  under-trailing.
- **Relative covariance ridge** (`src/optimize.rs`): ridge scales with
  `trace(Σ)/n` instead of fixed `1e-10`. Perfectly-correlated asset
  baskets now produce finite weights.
- **`min_variance` convergence diagnostics** (`src/optimize.rs`):
  optional `OptimizerOptions` and `OptimizerResult` expose
  convergence state. Bare API unchanged.
- **Tombstone accounting** (`src/matching.rs`): orphan-recovery path
  now correctly maintains `tombstone_count`.
- **Sharpe/Sortino guards** (`src/portfolio/metrics.rs`):
  non-positive `periods_per_year` returns 0.0 instead of NaN.

### Added (Numerical)

- **`StpPolicy` and `OrderOwner`** (`src/order.rs`, `src/exchange.rs`):
  self-trade prevention via `CancelNewest`, `CancelOldest`,
  `DecrementAndCancel`, or `Off` (default). Orders without an `owner`
  opt out of STP entirely.
- **Reference-parity test harness** (`tests/parity/`, `tests/reference_parity.rs`):
  10+ golden comparisons against scipy, ta-lib, and quantstats at
  1e-6 tolerance.

### Fixed (Security)

- **`f64_cents_checked`** (`nanobook-broker`): all broker-returned
  float values now route through a validator that rejects non-finite
  and out-of-range inputs. Previously silently produced 0 or
  saturated to `i64::MAX`.
- **ITCH parser panic-free** (`src/itch.rs`): short or malformed
  payloads now return `io::Error::InvalidData` instead of panicking.
- **Checked notional arithmetic** (`src/trade.rs`,
  `src/backtest_bridge.rs`): price × quantity now uses `checked_mul`.
- **Zeroize on drop** (`nanobook-broker`): API keys and secrets are
  zeroed when broker wrappers are dropped. (PyO3 `&str` parameters
  remain an unavoidable caveat; use env-var injection.)
- **Audit log hardening** (`nanobook-rebalancer`): audit files
  created with mode `0o600` on Unix; path sandboxed to the working
  directory.
- **Logging scrub** (`nanobook-broker`): equity and position values
  moved from `info!` to `debug!`.

### Infrastructure

- CI workflows: top-level `permissions: { contents: read }`; all
  tool installs pinned by version.
- Fuzz harness: `fuzz_submit` and `fuzz_itch` targets.
- Mutation baseline: ≥85% kill rate on matching engine documented in
  `fuzz/mutants-baseline.md`.

### Migration guide

**CVaR default change (N2):**
If your code relies on the v0.9 parametric behavior:
\`\`\`rust
// Before
let cvar = compute_cvar(&returns, 0.05);
// After
let cvar = compute_cvar_with(&returns, 0.05, CVaRMethod::ParametricNormal);
\`\`\`

**RiskEngine::new Result (S5):**
\`\`\`rust
// Before
let engine = RiskEngine::new(config);
// After
let engine = RiskEngine::new(config)?;  // or .expect("config")
\`\`\`

**rustls default (S1):**
If you relied on system OpenSSL: add `--features native-tls` to your
`nanobook-broker` dependency.
```

---

## 8. Open questions (resolve before starting)

1. **`tests/parity/golden.json`:** check in (recommended) or
   `.gitignore`? Check in. ~100 KB, one-shot regeneration on
   deliberate library bumps.

2. **`cargo mutants` kill-rate target:** 90% (aspirational) or 85%
   (realistic)? Recommend 85% with enumerated survivors. If we land
   higher, great.

3. **STP policy default:** `Off` (backward compat; silent) or `Off`
   + warn log at exchange construction? Leave silent. Users who care
   read the docs and opt in; users who don't stay on current
   behavior.

4. **Zeroize (S9) Python wrapper:** document the `&str` limitation
   and recommend env-var injection. No code change possible without a
   PyO3 API refactor — defer that to v0.11.

5. **Welford parallel merge path (N1 parallel-Bollinger via Rayon):**
   skip for v0.10; add to v0.11 backlog only if a user requests it.

6. **`#[non_exhaustive]` on public enums:** defer to a dedicated
   v0.11 API-cleanup release. Adding non_exhaustive to
   `BrokerOrderType`, `TrailMethod`, `StpPolicy` in v0.10 would widen
   the blast radius and isn't scoped here.

7. **`Sortino` ddof default (N4):** ddof=0 (quantstats/industry
   practitioner default) or expose a user-facing `ddof` parameter
   everywhere? Recommend ddof=0 default with a
   `compute_sortino_ddof(..., ddof)` escape hatch.

8. **N5 deprecation mechanism:** Rust doesn't support
   `#[deprecated]` on individual enum variants pre-1.87. Use a
   doc-only deprecation + `log::warn!` on construction, or wait until
   MSRV allows variant-level deprecation? Recommend doc-only +
   construction warning. Remove the variant in v0.11.

---

## 9. Dependency audit checklist

Pre-tag, run:

```bash
# Diff dependencies vs v0.9.3.
git diff v0.9.3..HEAD -- Cargo.lock | grep -E '^\+name = ' | sort -u
git diff v0.9.3..HEAD -- Cargo.lock | grep -E '^-name = ' | sort -u
```

Expected additions from P1:

- `zeroize` (S9).
- `arbitrary` (I2 fuzz, dev-dep only).

Expected removals or demotions:

- `openssl-*` crates if `rustls` default lands cleanly (S1).
- `tracing` should NOT appear (S6 uses existing `log`).

For any unexpected addition, investigate with `cargo tree -i <crate>`
and document in the commit.

Run `cargo deny check` — should be clean against `deny.toml`.

Run `cargo audit` — flag any advisories.

---

## 10. Rollback strategy

**Single-commit regression.** `git revert <sha>`. Push the revert.
File a follow-up issue.

**Subtle numerical regression.** If a landed commit introduces a
numerical drift that only shows under specific input:

1. Write a minimal failing test first (capture the regression).
2. Commit the failing test marked `#[ignore = "regression — see issue #N"]`.
3. Revert the offending commit.
4. Re-approach with a different algorithm or tighter tolerances.

**Release-level rollback.** If v0.10.0 publishes and a user reports a
showstopper:

1. `cargo yank --vers 0.10.0 -p <affected-crate>` on crates.io.
2. `pip yank nanobook==0.10.0` or PyPI equivalent.
3. Emergency v0.10.1 patch release with the fix.
4. Do NOT un-yank or re-upload the same version number.

---

## 11. Start point

**First action:** implement N10 (parity harness scaffolding) **before**
N1 (Welford). The harness is the measurement substrate for every
subsequent numeric fix — landing it first means N1's golden rolling-std
test catches the Welford regression in its own commit.

Order-of-operations for N10:
1. Create `tests/parity/` directory and `generate_golden.py`.
2. Pin `requirements.txt`.
3. Run `uv run python tests/parity/generate_golden.py` to produce
   `golden.json`.
4. Commit the directory + JSON (check-in).
5. Create `tests/reference_parity.rs` with one trivial test
   (e.g., `returns_fixture_length_matches`) to verify scaffolding.
6. Commit.

Then N1 adds the rolling-std golden test in its own commit, and the
Welford fix closes it.

**End of plan.**
