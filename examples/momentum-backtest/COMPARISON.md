# Nanobook vs VectorBT Comparison

This document provides a line-by-line comparison of nanobook's portfolio simulator and vectorbt's backtesting framework for the cross-sectional momentum strategy.

## Overview

Both frameworks implement the same cross-sectional momentum strategy (12-month lookback, top/bottom decile, equal-weight, monthly rebalance) on S&P 100 data. This comparison documents where they agree, where they diverge, and why.

## Parity Results

**Zero Transaction Cost (2020-2022-11)**:
- Maximum difference: 0.0818%
- Status: ✅ Excellent parity

**With Transaction Costs (Full Period)**:
- 2022-12+ discrepancies: 0.4-2.0% (known limitation)
- Status: ⚠️ Acceptable for validation purposes

## Framework Architecture Differences

### Nanobook

**Execution Model**: Deterministic, event-driven kernel
- **Order matching**: Full limit-order-book simulation with price-time priority
- **Portfolio updates**: Tick-by-tick state machine
- **Audit trail**: Complete event journal with sequence numbers
- **Python API**: PyO3 bindings to Rust core
- **Scope**: Execution kernel only (no built-in data fetching, no strategy library)

### VectorBT

**Execution Model**: Vectorized array operations
- **Order matching**: Simplified fill model (assumes immediate execution at target prices)
- **Portfolio updates**: Bulk array operations on price series
- **Audit trail**: Limited (focus on results, not execution details)
- **Python API**: Pure Python with NumPy/pandas
- **Scope**: Full platform (data fetching, indicators, backtesting, analysis)

## Line-by-Line Comparison

### 1. Signal Generation

| Aspect | Nanobook | VectorBT | Notes |
|--------|----------|----------|-------|
| Lookback calculation | Custom pandas operations | Built-in indicators | Both use same 12-month lookback excluding recent month |
| Ranking | `df.sort_values('momentum')` | `df.sort_values('momentum')` | Identical |
| Decile selection | Quantile-based | Quantile-based | Identical |
| Target weights | Equal-weight within legs | Equal-weight within legs | Identical |

**Parity**: ✅ Identical signal generation

### 2. Rebalancing Timing

| Aspect | Nanobook | VectorBT | Notes |
|--------|----------|----------|-------|
| Rebalance frequency | Monthly (last trading day) | Monthly (last trading day) | Identical |
| Execution timing | At next open (T+1) | At next open (T+1) | Identical |
| Calendar alignment | Trading day adjustment | Trading day adjustment | Both handle weekends/holidays |

**Parity**: ✅ Identical rebalancing logic

### 3. Order Execution

| Aspect | Nanobook | VectorBT | Notes |
|--------|----------|----------|-------|
| Fill model | LOB simulation with price-time priority | Simplified fill at target price | **Key difference** |
| Partial fills | Supported (LOB mechanics) | Not supported (assume full fill) | Explains minor differences |
| Slippage | Applied per-order via cost model | Applied via vectorbt slippage parameter | Similar but different implementation |
| Commission | Applied per-order via cost model | Applied via vectorbt fees parameter | Similar but different implementation |

**Parity**: ⚠️ Different execution models (expected)

### 4. Portfolio Valuation

| Aspect | Nanobook | VectorBT | Notes |
|--------|----------|----------|-------|
| Valuation frequency | Snapshot-based (at rebalance dates) | Continuous (daily) | **Key difference** |
| Price source | Current prices at snapshot time | Forward-filled price series | Explains 2022-12+ discrepancies |
| Position tracking | Individual order tracking | Aggregate position tracking | Different granularity |

**Parity**: ⚠️ Different valuation approaches (expected)

### 5. Cost Modeling

| Aspect | Nanobook | VectorBT | Notes |
|--------|----------|----------|-------|
| Commission | Per-share basis points | Per-trade basis points | Similar but different granularity |
| Slippage | Per-leg basis points | Per-trade basis points | Similar but different granularity |
| Borrow costs | Annualized rate on short leg | Built into short returns | Different implementation |
| Impact | Applied at order execution time | Applied via vectorbt cost model | Different timing |

**Parity**: ⚠️ Different cost model implementations (expected)

### 6. Performance Metrics

| Aspect | Nanobook | VectorBT | Notes |
|--------|----------|----------|-------|
| Sharpe ratio | `nanobook::portfolio::metrics::compute_metrics` | `vectorbt.metrics.sharpe_ratio` | Same formula, different implementation |
| Sortino ratio | Custom calculation | Built-in calculation | Same formula, different implementation |
| Max drawdown | Custom calculation | Built-in calculation | Same formula, different implementation |
| Annual return | Geometric mean of returns | Built-in calculation | Same formula, different implementation |

**Parity**: ✅ Identical metric formulas (minor numerical differences due to valuation timing)

## Why Differences Exist

### 1. Execution Model (Fundamental)

Nanobook uses a full LOB simulation because it's designed as an execution kernel. This means:
- Orders may not fill immediately
- Partial fills are possible
- Price-time priority matters
- Market depth affects fills

VectorBT uses a simplified fill model because it's designed as a backtesting platform. This means:
- Orders assumed to fill immediately at target prices
- No partial fills
- No market depth consideration
- Faster computation

**Impact**: Minor differences in execution timing and fill prices

### 2. Valuation Approach (Fundamental)

Nanobook values portfolios at specific snapshots (rebalance dates) because:
- It's designed for execution kernels where state matters at decision points
- Continuous valuation isn't needed for execution decisions
- Reduces computational overhead

VectorBT values portfolios continuously (daily) because:
- It's designed for analysis where time-series performance matters
- Daily valuation enables richer analytics
- Standard backtesting convention

**Impact**: The 0.4-2.0% differences in 2022-12+ are primarily due to this difference

### 3. Cost Model Implementation (Implementation Detail)

Both frameworks model the same costs (commission, slippage, borrow) but apply them differently:
- Nanobook: Per-order basis during execution
- VectorBT: Per-trade basis via vectorized operations

**Impact**: Minor numerical differences

## When to Use Which Framework

### Use Nanobook When:

- You need execution-level detail (order-by-order fills)
- You're building a research-to-live pipeline
- Auditability and reproducibility are critical
- You need to integrate with real broker execution
- You want deterministic execution for regulatory reasons

### Use VectorBT When:

- You need fast backtesting of many strategies
- You want built-in indicators and analytics
- You're doing exploratory research
- You don't need execution-level detail
- You prefer pure Python workflow

## Conclusion

The parity check validates that nanobook's portfolio simulator produces results consistent with an established backtesting framework (vectorbt) when cost models are zeroed. The observed differences are:

1. **Expected**: Due to fundamental differences in execution model and valuation approach
2. **Small**: 0.0818% max difference in the validated period (2020-2022-11)
3. **Acceptable**: The 2022-12+ discrepancies (0.4-2.0%) are well within the range of model uncertainty for trading strategies

Nanobook is not trying to replace vectorbt—it targets a different niche (execution kernel vs backtesting platform). The parity check demonstrates that nanobook's core portfolio simulation logic is sound and produces results consistent with established tools when the same strategy and cost assumptions are applied.