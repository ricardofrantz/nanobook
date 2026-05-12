#!/usr/bin/env python3
"""
Cross-sectional momentum backtest using nanobook portfolio simulator.

Implements the strategy specified in README.md:
- 12-month lookback returns (excluding most recent month)
- Top decile long, bottom decile short
- Equal-weight within each leg
- Monthly rebalancing
- Market-neutral (gross leverage 2.0, net exposure 0.0)

Usage:
    python strategy.py --data-file data/sp100_ohlcv.csv --start-date 2019-01-01 --end-date 2024-01-01

Output:
    - Equity curve and metrics
    - Parity comparison with vectorbt (if available)
"""

import argparse
import json
import sys
from datetime import datetime
from pathlib import Path
from typing import List, Tuple

import numpy as np
import pandas as pd

try:
    import nanobook
except ImportError:
    print("Error: nanobook Python package not installed")
    print("Install with: pip install nanobook")
    sys.exit(1)


def load_data(data_file: Path) -> pd.DataFrame:
    """Load OHLCV data from CSV."""
    df = pd.read_csv(data_file, low_memory=False)
    df["Date"] = pd.to_datetime(df["Date"])
    df = df.sort_values(["Ticker", "Date"])
    return df


def compute_momentum_signal(
    df: pd.DataFrame, lookback_months: int = 12, skip_months: int = 1
) -> pd.DataFrame:
    """
    Compute 12-month momentum signal for each ticker.

    Args:
        df: OHLCV data with columns [Date, Ticker, Open, High, Low, Close, Volume]
        lookback_months: Lookback period in months (default 12)
        skip_months: Months to skip from end (default 1 to avoid short-term reversal)

    Returns:
        DataFrame with columns [Date, Ticker, momentum]
    """
    # Compute monthly returns
    df = df.copy()
    df["Date"] = pd.to_datetime(df["Date"])
    # Ensure Close is numeric
    df["Close"] = pd.to_numeric(df["Close"], errors="coerce")

    # Get list of unique tickers
    tickers = df["Ticker"].unique()

    # Compute monthly close for each ticker separately
    # Use last trading day of each month
    monthly_dfs = []
    for ticker in tickers:
        ticker_df = df[df["Ticker"] == ticker].copy()
        ticker_df = ticker_df.set_index("Date")
        # Group by year-month and take last trading day
        ticker_df["YearMonth"] = ticker_df.index.to_period("M")
        monthly_close = ticker_df.groupby("YearMonth")["Close"].last()
        monthly_df = pd.DataFrame({
            "Date": monthly_close.index.to_timestamp(),  # Convert to timestamp (month-end)
            "Ticker": ticker,
            "Close": monthly_close.values
        })
        monthly_dfs.append(monthly_df)

    # Combine all tickers
    monthly_df = pd.concat(monthly_dfs, ignore_index=True)
    monthly_df = monthly_df.set_index(["Date", "Ticker"]).sort_index()

    # Compute momentum: (price_t-skip / price_t-lookback-skip) - 1
    # NOTE: the `shift(-skip_months)` must be applied per ticker; otherwise it will
    # leak across tickers due to the MultiIndex ordering (Date, then Ticker).
    pct = monthly_df["Close"].groupby(level="Ticker").pct_change(
        lookback_months + skip_months,
        fill_method=None,
    )
    momentum = pct.groupby(level="Ticker").shift(-skip_months)

    # Reset index and rename
    momentum = momentum.reset_index()
    momentum.columns = ["Date", "Ticker", "momentum"]

    # Drop NaN values (insufficient history)
    momentum = momentum.dropna()

    # Convert month-start dates to actual trading days (first trading day on or after the month-start)
    # This avoids forward-fill complexity
    trading_dates = df[["Date"]].drop_duplicates().sort_values("Date")
    momentum["Date"] = momentum["Date"].apply(
        lambda d: trading_dates[trading_dates["Date"] >= d].iloc[0]["Date"]
        if len(trading_dates[trading_dates["Date"] >= d]) > 0
        else d
    )

    return momentum


def get_target_weights(
    momentum_df: pd.DataFrame,
    date: pd.Timestamp,
    top_decile: float = 0.1,
    bottom_decile: float = 0.1,
) -> List[Tuple[str, float]]:
    """
    Generate target weights for a given rebalance date.

    Args:
        momentum_df: DataFrame with momentum signals
        date: Rebalance date
        top_decile: Fraction of universe to go long (default 0.1)
        bottom_decile: Fraction of universe to go short (default 0.1)

    Returns:
        List of (ticker, weight) tuples. Long positions have positive weights,
        short positions have negative weights. Weights are equal within each leg.
    """
    # Filter to current date
    current_momentum = momentum_df[momentum_df["Date"] == date].copy()

    if current_momentum.empty:
        return []

    # Rank by momentum
    current_momentum["rank"] = current_momentum["momentum"].rank(method="first")
    n_stocks = len(current_momentum)

    # Select top and bottom deciles
    top_n = int(n_stocks * top_decile)
    bottom_n = int(n_stocks * bottom_decile)

    long_stocks = current_momentum.nlargest(top_n, "momentum")["Ticker"].tolist()
    short_stocks = current_momentum.nsmallest(bottom_n, "momentum")["Ticker"].tolist()

    # Equal-weight within each leg
    long_weight = 1.0 / len(long_stocks) if long_stocks else 0.0
    short_weight = -1.0 / len(short_stocks) if short_stocks else 0.0

    # Combine into target weights
    targets = [(ticker, long_weight) for ticker in long_stocks] + [
        (ticker, short_weight) for ticker in short_stocks
    ]

    return targets


def run_backtest(
    data_file: Path,
    start_date: str,
    end_date: str,
    initial_cash: int = 1_000_000_00,  # $1M in cents
    commission_bps: int = 5,  # $0.005 per share
    slippage_bps: int = 5,  # 5 bps per leg
) -> dict:
    """
    Run momentum backtest using nanobook portfolio simulator.

    Args:
        data_file: Path to OHLCV CSV file
        start_date: Backtest start date (YYYY-MM-DD)
        end_date: Backtest end date (YYYY-MM-DD)
        initial_cash: Starting cash in cents
        commission_bps: Commission in basis points
        slippage_bps: Slippage in basis points

    Returns:
        Dictionary with backtest results including equity curve, metrics, etc.
    """
    # Load data
    df = load_data(data_file)
    df = df[(df["Date"] >= start_date) & (df["Date"] <= end_date)]

    # Compute momentum signals
    print("Computing momentum signals...")
    momentum_df = compute_momentum_signal(df)

    # Get rebalance dates (month-end)
    rebalance_dates = sorted(momentum_df["Date"].unique())

    # Initialize portfolio
    cost_model = nanobook.CostModel(
        commission_bps=commission_bps, slippage_bps=slippage_bps, min_trade_fee=0
    )
    portfolio = nanobook.Portfolio(initial_cash, cost_model)

    # Track results
    equity_curve = []
    snapshots = []

    print(f"Running backtest from {start_date} to {end_date}")
    print(f"Rebalance dates: {len(rebalance_dates)}")

    # Run backtest month by month
    for i, rebalance_date in enumerate(rebalance_dates):
        print(f"[{i+1}/{len(rebalance_dates)}] Rebalancing on {rebalance_date.date()}")

        # Get target weights
        targets = get_target_weights(momentum_df, rebalance_date)

        if not targets:
            print(f"  Warning: No valid targets for {rebalance_date.date()}, skipping")
            continue

        # Get current prices (exact date match since we aligned to trading days)
        prices_on_date = df[df["Date"] == rebalance_date]
        if prices_on_date.empty:
            print(f"  Warning: No prices for {rebalance_date.date()}, skipping")
            continue

        prices = [
            (row["Ticker"], int(row["Close"] * 100))  # Convert to cents
            for _, row in prices_on_date.iterrows()
        ]

        # Rebalance portfolio
        portfolio.rebalance_simple(targets, prices)

        # Take snapshot AFTER rebalancing to match vectorbt timing
        snapshot = portfolio.snapshot(prices)
        snapshots.append(
            {
                "date": rebalance_date,
                "cash": snapshot["cash"],
                "equity": snapshot["equity"],
                "num_positions": snapshot["num_positions"],
            }
        )
        equity_curve.append(snapshot["equity"])

    # Compute metrics
    metrics = portfolio.compute_metrics(periods_per_year=12.0, risk_free=0.0)

    # Convert equity curve to dollars
    equity_curve_usd = [e / 100.0 for e in portfolio.equity_curve()]

    results = {
        "equity_curve": equity_curve_usd,
        "returns": portfolio.returns(),
        "metrics": metrics,
        "snapshots": snapshots,
        "initial_cash": initial_cash / 100.0,  # Convert to dollars
        "final_equity": portfolio.equity_curve()[-1] / 100.0,
        "total_return": (portfolio.equity_curve()[-1] / initial_cash) - 1.0,
    }

    return results


def print_results(results: dict):
    """Print backtest results summary."""
    print("\n" + "=" * 70)
    print("BACKTEST RESULTS")
    print("=" * 70)
    print(f"Initial cash: ${results['initial_cash']:,.2f}")
    print(f"Final equity: ${results['final_equity']:,.2f}")
    print(f"Total return: {results['total_return']:.2%}")

    if results["metrics"]:
        m = results["metrics"]
        print(f"\nMetrics:")
        print(f"  Sharpe ratio: {m.sharpe:.2f}")
        print(f"  Sortino ratio: {m.sortino:.2f}")
        print(f"  Max drawdown: {m.max_drawdown:.2%}")
        print(f"  Annual return: {m.annual_return:.2%}")
        print(f"  Annual volatility: {m.annual_volatility:.2%}")

    print("=" * 70)


def main():
    parser = argparse.ArgumentParser(
        description="Cross-sectional momentum backtest using nanobook"
    )
    parser.add_argument(
        "--data-file",
        default="data/sp100_ohlcv.csv",
        help="Path to OHLCV CSV file (default: data/sp100_ohlcv.csv)",
    )
    parser.add_argument(
        "--start-date",
        default="2019-01-01",
        help="Start date (YYYY-MM-DD, default: 2019-01-01)",
    )
    parser.add_argument(
        "--end-date",
        default="2024-01-01",
        help="End date (YYYY-MM-DD, default: 2024-01-01)",
    )
    parser.add_argument(
        "--initial-cash",
        type=int,
        default=1000000,
        help="Initial cash in dollars (default: 1000000)",
    )
    parser.add_argument(
        "--commission-bps",
        type=int,
        default=5,
        help="Commission in basis points (default: 5)",
    )
    parser.add_argument(
        "--slippage-bps",
        type=int,
        default=5,
        help="Slippage in basis points (default: 5)",
    )
    parser.add_argument(
        "--output",
        default=None,
        help="Save results to JSON file (e.g., results.json)",
    )
    args = parser.parse_args()

    data_file = Path(args.data_file)
    if not data_file.exists():
        print(f"Error: Data file {data_file} not found")
        print("Run download_prices.py first")
        sys.exit(1)

    # Run backtest
    results = run_backtest(
        data_file=data_file,
        start_date=args.start_date,
        end_date=args.end_date,
        initial_cash=args.initial_cash * 100,  # Convert to cents
        commission_bps=args.commission_bps,
        slippage_bps=args.slippage_bps,
    )

    # Print results
    print_results(results)

    # Save results to JSON if requested
    if args.output:
        output_path = Path(args.output)
        # Convert datetime objects to strings for JSON serialization
        json_results = results.copy()
        if 'snapshots' in json_results:
            json_results['snapshots'] = [
                {**s, 'date': s['date'].isoformat() if hasattr(s['date'], 'isoformat') else str(s['date'])}
                for s in json_results['snapshots']
            ]
        # Convert metrics to dict if it's an object
        if 'metrics' in json_results and hasattr(json_results['metrics'], '__dict__'):
            json_results['metrics'] = json_results['metrics'].__dict__
        
        with open(output_path, 'w') as f:
            json.dump(json_results, f, indent=2)
        print(f"Results saved to {output_path}")


if __name__ == "__main__":
    main()