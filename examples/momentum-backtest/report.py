#!/usr/bin/env python3
"""
Generate HTML report for momentum backtest results.
"""

import argparse
import json
from pathlib import Path
from typing import Any, Dict
import matplotlib.pyplot as plt
import matplotlib.dates as mdates
from datetime import datetime
import base64
from io import BytesIO


def load_results(results_file: Path) -> Dict[str, Any]:
    """Load backtest results from JSON file."""
    with open(results_file, 'r') as f:
        return json.load(f)


def plot_equity_curve(equity_curve: list, dates: list) -> str:
    """Generate equity curve plot and return as base64-encoded image."""
    fig, ax = plt.subplots(figsize=(12, 6))
    
    # Convert dates to datetime objects if they're strings
    if dates and isinstance(dates[0], str):
        dates = [datetime.strptime(d, '%Y-%m-%d') for d in dates]
    
    ax.plot(dates, equity_curve, linewidth=2, label='Portfolio Value')
    ax.axhline(y=equity_curve[0], color='r', linestyle='--', alpha=0.5, label='Initial')
    
    ax.set_xlabel('Date')
    ax.set_ylabel('Portfolio Value ($)')
    ax.set_title('Equity Curve')
    ax.legend()
    ax.grid(True, alpha=0.3)
    
    # Format x-axis dates
    ax.xaxis.set_major_formatter(mdates.DateFormatter('%Y-%m'))
    ax.xaxis.set_major_locator(mdates.MonthLocator(interval=6))
    plt.xticks(rotation=45)
    
    plt.tight_layout()
    
    # Convert to base64
    buffer = BytesIO()
    plt.savefig(buffer, format='png', dpi=100)
    buffer.seek(0)
    image_base64 = base64.b64encode(buffer.read()).decode()
    plt.close()
    
    return image_base64


def plot_drawdown(equity_curve: list, dates: list) -> str:
    """Generate drawdown plot and return as base64-encoded image."""
    # Calculate drawdown
    equity_array = equity_curve
    running_max = [equity_array[0]]
    for i in range(1, len(equity_array)):
        running_max.append(max(running_max[-1], equity_array[i]))
    
    drawdown = [(equity_array[i] - running_max[i]) / running_max[i] * 100 
                for i in range(len(equity_array))]
    
    fig, ax = plt.subplots(figsize=(12, 4))
    
    if dates and isinstance(dates[0], str):
        dates = [datetime.strptime(d, '%Y-%m-%d') for d in dates]
    
    ax.fill_between(dates, drawdown, 0, alpha=0.3, color='red')
    ax.plot(dates, drawdown, color='red', linewidth=1)
    
    ax.set_xlabel('Date')
    ax.set_ylabel('Drawdown (%)')
    ax.set_title('Portfolio Drawdown')
    ax.grid(True, alpha=0.3)
    
    # Format x-axis dates
    ax.xaxis.set_major_formatter(mdates.DateFormatter('%Y-%m'))
    ax.xaxis.set_major_locator(mdates.MonthLocator(interval=6))
    plt.xticks(rotation=45)
    
    plt.tight_layout()
    
    # Convert to base64
    buffer = BytesIO()
    plt.savefig(buffer, format='png', dpi=100)
    buffer.seek(0)
    image_base64 = base64.b64encode(buffer.read()).decode()
    plt.close()
    
    return image_base64


def generate_html_report(results: Dict[str, Any], output_file: Path):
    """Generate HTML report from backtest results."""
    
    # Extract data
    equity_curve = results.get('equity_curve', [])
    snapshots = results.get('snapshots', [])
    metrics = results.get('metrics', {})
    
    # Extract dates from snapshots
    dates = [s['date'] for s in snapshots] if snapshots else []
    
    # Generate plots
    equity_plot = plot_equity_curve(equity_curve, dates)
    drawdown_plot = plot_drawdown(equity_curve, dates)
    
    # Build HTML
    html = f"""<!DOCTYPE html>
<html>
<head>
    <title>Momentum Backtest Report</title>
    <style>
        body {{
            font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
            max-width: 1200px;
            margin: 0 auto;
            padding: 20px;
            line-height: 1.6;
        }}
        h1 {{ color: #333; border-bottom: 2px solid #007acc; padding-bottom: 10px; }}
        h2 {{ color: #555; margin-top: 30px; }}
        .metrics-grid {{
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
            gap: 20px;
            margin: 20px 0;
        }}
        .metric-card {{
            background: #f5f5f5;
            padding: 15px;
            border-radius: 8px;
            border-left: 4px solid #007acc;
        }}
        .metric-label {{ font-size: 0.9em; color: #666; margin-bottom: 5px; }}
        .metric-value {{ font-size: 1.5em; font-weight: bold; color: #333; }}
        .plot-container {{
            margin: 30px 0;
            text-align: center;
        }}
        .plot-container img {{
            max-width: 100%;
            height: auto;
            border: 1px solid #ddd;
            border-radius: 8px;
        }}
        .summary {{
            background: #e8f4f8;
            padding: 20px;
            border-radius: 8px;
            margin: 20px 0;
        }}
    </style>
</head>
<body>
    <h1>Momentum Backtest Report</h1>
    
    <div class="summary">
        <h2>Summary</h2>
        <p><strong>Initial Cash:</strong> ${results['initial_cash']:,.2f}</p>
        <p><strong>Final Equity:</strong> ${results['final_equity']:,.2f}</p>
        <p><strong>Total Return:</strong> {results['total_return']:.2%}</p>
    </div>
    
    <h2>Performance Metrics</h2>
    <div class="metrics-grid">
        <div class="metric-card">
            <div class="metric-label">Sharpe Ratio</div>
            <div class="metric-value">{metrics.get('sharpe', 'N/A'):.2f}</div>
        </div>
        <div class="metric-card">
            <div class="metric-label">Sortino Ratio</div>
            <div class="metric-value">{metrics.get('sortino', 'N/A'):.2f}</div>
        </div>
        <div class="metric-card">
            <div class="metric-label">Max Drawdown</div>
            <div class="metric-value">{metrics.get('max_drawdown', 0):.2%}</div>
        </div>
        <div class="metric-card">
            <div class="metric-label">Annual Return</div>
            <div class="metric-value">{metrics.get('annual_return', 0):.2%}</div>
        </div>
        <div class="metric-card">
            <div class="metric-label">Annual Volatility</div>
            <div class="metric-value">{metrics.get('annual_volatility', 0):.2%}</div>
        </div>
    </div>
    
    <h2>Equity Curve</h2>
    <div class="plot-container">
        <img src="data:image/png;base64,{equity_plot}" alt="Equity Curve">
    </div>
    
    <h2>Drawdown</h2>
    <div class="plot-container">
        <img src="data:image/png;base64,{drawdown_plot}" alt="Drawdown">
    </div>
    
    <h2>Strategy Details</h2>
    <ul>
        <li><strong>Strategy:</strong> Cross-sectional momentum</li>
        <li><strong>Universe:</strong> S&P 100</li>
        <li><strong>Lookback:</strong> 12 months (excluding most recent month)</li>
        <li><strong>Rebalancing:</strong> Monthly</li>
        <li><strong>Position sizing:</strong> Equal-weight long/short (top/bottom decile)</li>
        <li><strong>Gross leverage:</strong> 2.0</li>
        <li><strong>Net exposure:</strong> 0.0 (market-neutral)</li>
    </ul>
    
    <p style="color: #666; font-size: 0.9em; margin-top: 40px;">
        Generated by nanobook momentum backtest example
    </p>
</body>
</html>"""
    
    # Write HTML file
    with open(output_file, 'w') as f:
        f.write(html)
    
    print(f"Report generated: {output_file}")


def main():
    parser = argparse.ArgumentParser(description="Generate HTML report from backtest results")
    parser.add_argument(
        "--results",
        default="results.json",
        help="Path to results JSON file (default: results.json)"
    )
    parser.add_argument(
        "--output",
        default="report.html",
        help="Output HTML file (default: report.html)"
    )
    args = parser.parse_args()
    
    results = load_results(Path(args.results))
    generate_html_report(results, Path(args.output))


if __name__ == "__main__":
    main()