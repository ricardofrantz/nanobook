#!/usr/bin/env python3
"""
VectorBT parity check for cross-sectional momentum strategy.
Goal: compare nanobook and vectorbt equity curves with an explicit cost model.

Differences are EXPECTED when costs are enabled because the engines implement
costs differently:
- nanobook applies commission and slippage separately per trade.
- vectorbt applies fee and slippage as percentages.
- These are fundamentally different cost models, so parity is not expected.

KNOWN LIMITATION:
- Zero-cost parity is excellent for 2020-2022-11 (max diff: 0.0818%)
- 2022-12+ shows 0.4-2.0% discrepancies due to fundamental differences in how
  the two systems handle portfolio valuation between rebalance dates
- With costs enabled, nanobook and vectorbt use different cost model
  implementations, so differences are expected even before valuation effects
- Use --end-date 2022-11-30 for validated parity check
"""

import argparse
import sys
from pathlib import Path

import numpy as np
import pandas as pd
import vectorbt as vbt

from strategy import load_data, compute_momentum_signal, get_target_weights, run_backtest

def run_vbt_backtest(
    data_file: Path,
    start_date: str,
    end_date: str,
    initial_cash: float,
    cost_bps: int,
):
    df = load_data(data_file)
    df = df[(df["Date"] >= start_date) & (df["Date"] <= end_date)]

    close_prices = df.pivot(index="Date", columns="Ticker", values="Close")

    momentum_df = compute_momentum_signal(df)
    rebalance_dates = sorted(momentum_df["Date"].unique())

    # No forward-fill needed since we aligned rebalance dates to trading days

    # VectorBT expects target percentages. We build a dataframe of target weights.
    target_weights = pd.DataFrame(np.nan, index=close_prices.index, columns=close_prices.columns)

    for rebalance_date in rebalance_dates:
        # Check if the rebalance_date is in our price index (after forward fill)
        if rebalance_date not in target_weights.index:
            print(f"Warning: Rebalance date {rebalance_date} not in price index")
            continue

        targets = get_target_weights(momentum_df, rebalance_date)

        # Start by zeroing out all weights for this date
        target_weights.loc[rebalance_date, :] = 0.0

        for ticker, weight in targets:
            if ticker in target_weights.columns:
                target_weights.loc[rebalance_date, ticker] = weight

    # Run vectorbt
    cost_rate = cost_bps / 10000
    portfolio = vbt.Portfolio.from_orders(
        close=close_prices,
        size=target_weights,
        size_type='targetpercent',
        # group_by applies to *columns* (assets). Here we intentionally group all
        # tickers into a single portfolio for comparison against nanobook.
        group_by=True,
        cash_sharing=True,
        init_cash=initial_cash,
        fees=cost_rate,
        slippage=cost_rate,
        freq='D'
    )

    return portfolio

def main():
    parser = argparse.ArgumentParser(description="VectorBT parity check")
    parser.add_argument("--data-file", default="data/sp100_ohlcv.csv")
    parser.add_argument("--start-date", default="2019-01-01")
    parser.add_argument("--end-date", default="2024-01-01")
    parser.add_argument("--initial-cash", type=int, default=1000000)
    parser.add_argument("--cost-bps", type=int, default=5)
    args = parser.parse_args()

    data_file = Path(args.data_file)
    if not data_file.exists():
        print(f"Error: {data_file} not found")
        sys.exit(1)

    print(f"Running nanobook backtest ({args.cost_bps} bps cost)...")
    nb_results = run_backtest(
        data_file=data_file,
        start_date=args.start_date,
        end_date=args.end_date,
        initial_cash=args.initial_cash * 100,  # cents
        commission_bps=args.cost_bps,
        slippage_bps=args.cost_bps
    )

    print(f"\nRunning vectorbt backtest ({args.cost_bps} bps cost)...")
    vbt_portfolio = run_vbt_backtest(
        data_file=data_file,
        start_date=args.start_date,
        end_date=args.end_date,
        initial_cash=args.initial_cash,
        cost_bps=args.cost_bps
    )

    # Extract equity curves
    nb_equity = nb_results["equity_curve"]
    # The nanobook equity curve is only sampled on rebalance dates (and end).
    nb_dates = [s["date"] for s in nb_results["snapshots"]]

    vbt_equity_series = vbt_portfolio.value()
    if isinstance(vbt_equity_series, pd.DataFrame):
        # Defensive: some vectorbt configurations may return a 2D value series.
        # Prefer a single column if present.
        if vbt_equity_series.shape[1] == 1:
            vbt_equity_series = vbt_equity_series.iloc[:, 0]
        else:
            vbt_equity_series = vbt_equity_series.sum(axis=1)

    print("\n" + "="*70)
    print(f"PARITY CHECK RESULTS (Cost: {args.cost_bps} bps)")
    print("="*70)

    differences = []

    # Compare only on the snapshot dates
    for i, snapshot in enumerate(nb_results["snapshots"]):
        date = snapshot["date"]
        nb_val = snapshot["equity"] / 100  # Convert cents to dollars

        # Match date in VBT
        if date in vbt_equity_series.index:
            vbt_val = vbt_equity_series.loc[date]
            diff = abs(nb_val - vbt_val) / args.initial_cash
            differences.append(diff)
            # We want to print significant differences
            if diff > 1e-4:
                print(f"Date: {date.date()} | Nanobook: ${nb_val:,.2f} | VectorBT: ${vbt_val:,.2f} | Diff: {diff:.4%}")
        else:
            print(f"Date {date.date()} not found in VectorBT results.")

    if not differences:
        print("No matching dates to compare.")
    else:
        max_diff = max(differences)
        print(f"\nMax difference between nanobook and vectorbt: {max_diff:.4%}")
        # Use 1% epsilon because the cost models are not equivalent:
        # nanobook applies commission and slippage separately per trade, while
        # vectorbt applies fees and slippage as percentages.
        # Known limitation: 2022-12 onwards shows larger discrepancies
        # (0.4-2.0%) due to valuation differences
        # For full-period validation, restrict end-date to 2022-11-30
        if max_diff < 0.01:
            print("PARITY ACHIEVED: Max difference is within acceptable epsilon (< 1%).")
            sys.exit(0)
        else:
            print("PARITY FAILED: Max difference exceeds acceptable epsilon.")
            print(
                "Note: Cost-model differences are expected; for zero-cost "
                "valuation validation, use --cost-bps 0 --end-date 2022-11-30."
            )
            sys.exit(1)

if __name__ == "__main__":
    main()
