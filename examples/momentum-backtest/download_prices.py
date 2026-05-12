#!/usr/bin/env python3
"""
Download daily OHLCV data for S&P 100 constituents.

Usage:
    python download_prices.py [--start-date YYYY-MM-DD] [--end-date YYYY-MM-DD]

This script fetches historical price data for S&P 100 stocks using yfinance,
caches the results with checksum validation, and saves to CSV format.

Data is cached in data/ directory with MD5 checksums to avoid re-downloading
unchanged data. Follows the same pattern as v0.11 ITCH replay download.

Note: This script uses current S&P 100 constituents and does not account for
historical index changes. This introduces survivorship bias into backtests.
For production use, use a historical constituent database (e.g., CRSP, Compustat).
"""

import argparse
import hashlib
import json
import os
import time
from datetime import datetime
from pathlib import Path

import pandas as pd
import yfinance as yf


# Current S&P 100 tickers (as of 2025-01)
# Note: BRK.B changed to BRK-B for yfinance compatibility
# Using smaller subset for parity check due to yfinance API limitations
SP100_TICKERS = [
    "AAPL", "MSFT", "GOOGL", "AMZN", "TSLA", "META", "BRK-B", "NVDA", "JPM",
    "JNJ", "V", "PG", "XOM", "UNH", "HD", "MA", "BAC", "PFE", "ABBV", "KO",
    "PEP", "COST", "TMO", "AVGO", "CRM", "MRK", "ABT", "CVX", "DHR", "NKE",
    "ACN", "MCD", "WMT", "DIS", "VZ", "CSCO", "ADBE", "NFLX", "IBM", "INTC",
    "ORCL", "CMCSA", "QCOM", "TXN", "HON", "AMD", "LIN", "GE", "LLY", "PM",
    "SAP", "NEE", "RTX", "AMT", "UPS", "HCA", "PLD", "T", "LMT", "BA",
    "CAT", "DE", "MMM", "BLK", "SPGI", "GS", "MS", "SCHW", "C", "AXP",
    "CB", "USB", "PNC", "BK", "TJX", "SHW", "CL", "COP", "CVS", "WFC",
    "ADP", "MO", "MDT", "ISRG", "GILD", "VRTX", "MRNA", "REGN", "EL", "LRCX",
]

# For parity check, use only the first 20 tickers that download successfully
PARITY_TICKERS = SP100_TICKERS[:20]


def get_data_dir() -> Path:
    """Get the data directory relative to this script."""
    script_dir = Path(__file__).parent
    data_dir = script_dir / "data"
    data_dir.mkdir(exist_ok=True)
    return data_dir


def get_checksum_path() -> Path:
    """Get the checksum file path."""
    return get_data_dir() / "prices.md5"


def file_hash(filepath: Path) -> str:
    """Compute MD5 hash of a file."""
    hash_md5 = hashlib.md5()
    with open(filepath, "rb") as f:
        for chunk in iter(lambda: f.read(4096), b""):
            hash_md5.update(chunk)
    return hash_md5.hexdigest()


def load_checksums() -> dict:
    """Load existing checksums from file."""
    checksum_path = get_checksum_path()
    if checksum_path.exists():
        with open(checksum_path, "r") as f:
            return json.load(f)
    return {}


def save_checksums(checksums: dict) -> None:
    """Save checksums to file."""
    checksum_path = get_checksum_path()
    with open(checksum_path, "w") as f:
        json.dump(checksums, f, indent=2, sort_keys=True)


def download_ticker_data(
    ticker: str, start_date: str, end_date: str
) -> pd.DataFrame:
    """Download OHLCV data for a single ticker."""
    try:
        # yfinance returns data with tz-aware index
        # Use auto_adjust=False to avoid the FutureWarning
        df = yf.download(ticker, start=start_date, end=end_date, progress=False, auto_adjust=False)
        if df.empty:
            print(f"Warning: No data for {ticker}")
            return None
        # Reset index to make Date a column
        df = df.reset_index()
        # Flatten MultiIndex columns
        df.columns = df.columns.get_level_values(0)
        # Keep only the columns we need
        df = df[["Date", "Open", "High", "Low", "Close", "Volume"]]
        df["Ticker"] = ticker
        return df
    except Exception as e:
        print(f"Error downloading {ticker}: {e}")
        return None


def main():
    parser = argparse.ArgumentParser(
        description="Download S&P 100 OHLCV data with caching"
    )
    parser.add_argument(
        "--start-date",
        default="2019-01-01",
        help="Start date (YYYY-MM-DD), default: 2019-01-01",
    )
    parser.add_argument(
        "--end-date",
        default=datetime.now().strftime("%Y-%m-%d"),
        help="End date (YYYY-MM-DD), default: today",
    )
    parser.add_argument(
        "--force",
        action="store_true",
        help="Force re-download even if cached data matches checksum",
    )
    parser.add_argument(
        "--parity-mode",
        action="store_true",
        help="Download only 20 tickers for parity check (faster, more reliable)",
    )
    args = parser.parse_args()

    # Use smaller subset for parity check
    tickers_to_download = PARITY_TICKERS if args.parity_mode else SP100_TICKERS

    output_file = get_data_dir() / "sp100_ohlcv.csv"
    checksums = load_checksums()

    # Check if cached data exists and is valid
    if output_file.exists() and not args.force:
        current_hash = file_hash(output_file)
        expected_hash = checksums.get(output_file.name)
        if expected_hash and current_hash == expected_hash:
            print(f"{output_file} already exists and matches checksum")
            return

    print(f"Downloading {len(tickers_to_download)} tickers from {args.start_date} to {args.end_date}")

    # Download data for all tickers
    all_data = []
    for i, ticker in enumerate(tickers_to_download, 1):
        print(f"[{i}/{len(tickers_to_download)}] Downloading {ticker}...")
        df = download_ticker_data(ticker, args.start_date, args.end_date)
        if df is not None:
            all_data.append(df)
        # Small delay to avoid "too many open files" error
        time.sleep(0.1)

    if not all_data:
        print("Error: No data downloaded")
        return

    # Combine all data vertically
    combined_df = pd.concat(all_data, ignore_index=True)

    # Save to CSV
    combined_df.to_csv(output_file, index=False)

    # Update checksum
    new_hash = file_hash(output_file)
    checksums[output_file.name] = new_hash
    save_checksums(checksums)

    print(f"Saved {len(combined_df)} rows to {output_file}")
    print(f"Data range: {combined_df['Date'].min()} to {combined_df['Date'].max()}")
    print(f"Tickers: {combined_df['Ticker'].nunique()}")

    # Print survivorship bias warning
    print("\n" + "=" * 70)
    print("SURVIVORSHIP BIAS WARNING")
    print("=" * 70)
    print("This script uses current S&P 100 constituents only.")
    print("Historical index changes (additions/deletions) are not accounted for.")
    print("This introduces survivorship bias into backtests.")
    print("For production use, use a historical constituent database (CRSP, Compustat).")
    print("=" * 70)


if __name__ == "__main__":
    main()