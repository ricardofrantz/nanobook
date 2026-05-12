# nanobook v0.12 — "Backtest + Positioning" — Plan (PREVIEW)

**Status:** PREVIEW — concrete spec to be written when v0.11 ships
**Target version:** v0.12.0
**Timeline:** 2–3 weeks
**Baseline:** v0.11.0 (ITCH replay)

**Theme:** Second case study + competitive repositioning. Move from market-data fidelity (v0.11) to strategy-level evidence, and flip the README from defensive ("what nanobook is NOT") to offensive ("nanobook owns this niche").

---

## Goal

Two deliverables, one release:

1. Demonstrate nanobook's portfolio simulator on a strategy a reader can sanity-check, with a parity check against an established backtest framework.
2. Reposition the README with an explicit competitive table that names nanobook's niche offensively.

## Non-goals

- No new public API beyond what the strategy needs.
- No live broker work (v0.13/v0.14).
- No new analytics primitives — use what's already in `nanobook::portfolio` and `nanobook::stats`.
- No OCaml.

## Candidate strategy (to confirm)

Cross-sectional momentum on a small fixed universe (e.g., S&P 100 monthly rebalance, 12-month lookback, top-decile long / bottom-decile short, equal-weight). Public daily data via `stooq` or `yfinance`. Strategy is uncontroversial — readers can verify it themselves.

## Deliverables

1. **`examples/momentum-backtest/`**:
   - `README.md` — strategy spec, methodology, expected results
   - `download_prices.py` — fetches and caches OHLCV for the universe
   - `strategy.py` — implements the signal + target weights using nanobook's PyO3 portfolio API
   - `report.py` → `report.html` — equity curve, drawdown, turnover, cost decomposition, Sharpe / Sortino / max DD vs `vectorbt` reference
   - `expected/` — golden output for a fixed historical window
2. **`examples/momentum-backtest/COMPARISON.md`** — line-by-line: what nanobook reports vs vectorbt, where they agree, where (and why) they diverge.
3. **README competitive-positioning section** — explicit table:

   | Tool | Scope | Execution model | Python API | Rust core | Auditability | Niche |
   |---|---|---|---|---|---|---|
   | nanobook | Kernel | Deterministic | Yes (PyO3) | Yes | Audit logs + event journal | Small, auditable execution kernel for Python research-to-live bridges |
   | vectorbt | Platform | Vectorized | Yes | No | Notebook-driven | Fast Python backtesting |
   | NautilusTrader | Platform | Event-driven | Yes (Cython) | Partial | Built-in | Full-stack live trading platform |
   | Hummingbot | Platform | Event-driven | Yes | No | Operational | Crypto market making |
   | Freqtrade | Platform | Strategy-engine | Yes | No | Telegram-driven | Hobbyist crypto |
   | LEAN | Platform | Event-driven | Multi-lang | No | QuantConnect-integrated | Cloud quant platform |

   Replaces the current defensive "What nanobook is NOT" section.
4. **`docs/solutions/portfolio-sim-parity-learnings.md`** — surprises from the parity check.
5. **CI** — `examples-smoke` extends to run the backtest on a fixed cached price window.

## Acceptance criteria (to refine at planning gate)

- [ ] Backtest reproduces published results within tolerance from at least one peer-reviewed momentum reference (e.g., Jegadeesh & Titman 1993, or a more recent Asness paper).
- [ ] Equity curve and Sharpe within ε of vectorbt's output when cost models are zeroed; documented deviations when costs are on.
- [ ] `report.html` renders in <30s on cached data; full backtest on uncached data in <10 min.
- [ ] Competitive table in README is honest about nanobook's weaknesses (no GUI, no community size, no broker breadth) as well as strengths.

## Risks

- **Data licensing** for stooq/yfinance — same `download_at_runtime` pattern as v0.11.
- **Parity definition** — vectorbt's cost model isn't identical to nanobook's. Differences are educational, not bugs; document them.
- **Survivorship bias** — universe choice will introduce some. Disclose, don't hide.
- **Competitive table becoming bait** — if framed as "nanobook beats X at Y," it invites pointless arguments. Frame as "nanobook is for X niche; if your need is Y, prefer tool Z." Honest positioning is durable; competitive snipes are not.

## Version bumps

| Crate | v0.11.0 | v0.12.0 | Reason |
|---|---|---|---|
| `nanobook` | 0.11.0 | 0.12.0 | Possible portfolio-sim fixes if parity check surfaces issues. |
| `nanobook-broker` | 0.5.0 | 0.5.0 | Untouched. |
| `nanobook-risk` | 0.5.0 | 0.5.0 | Untouched. |
| `nanobook-rebalancer` | 0.6.0 | 0.6.0 | Untouched. |
| `nanobook-python` | 0.11.0 | 0.12.0 | Re-export; may add convenience helpers if needed for the demo. |

## Open questions

1. Cross-sectional momentum vs. simpler SMA crossover — which serves the demo narrative better?
2. vectorbt vs zipline-reloaded vs bt as the reference framework?
3. Stress runs (high vol regime, low vol regime, 2020 covid window) included in v0.12 or deferred?

## Phasing (3 weeks)

| Week | Phase |
|---|---|
| 1 | Strategy spec; data acquisition; cached prices + checksum; baseline vectorbt run; competitive table draft |
| 2 | nanobook strategy implementation; equity-curve parity at zero cost; report skeleton |
| 3 | Cost-on parity check; `report.html` polish; competitive table merged into README; learnings doc; release |
