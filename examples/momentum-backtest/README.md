# Momentum Backtest Strategy

## Strategy Choice: Cross-Sectional Momentum

**Decision**: Cross-sectional momentum on S&P 100 (12-month lookback, top-decile long / bottom-decile short, equal-weight, monthly rebalancing)

**Rationale**:
1. **Peer-reviewed foundation**: Jegadeesh & Titman (1993) "Returns to Buying Winners and Selling Losers" provides the canonical academic reference
2. **Uncontroversial**: Momentum is a well-documented market anomaly with decades of replication
3. **Verifiable**: Readers can independently verify results using public price data
4. **Realistic complexity**: Requires universe selection, ranking, and rebalancing logic that maps well to nanobook's positioning system

## Strategy Specification

### Universe
- S&P 100 constituents (OEX index)
- Monthly universe snapshot (no survivorship bias in implementation)

### Signal
- 12-month lookback returns (excluding most recent month to avoid short-term reversal)
- Rank by returns
- Long: top decile (10% of universe)
- Short: bottom decile (10% of universe)

### Portfolio Construction
- Equal-weight within long and short legs
- Gross leverage: 2.0 (100% long, 100% short)
- Net exposure: 0.0 (market-neutral)

### Rebalancing
- Monthly rebalance on last trading day of month
- Execution at next open (T+1)
- No turnover constraints (full turnover each month)

### Costs
- Commission: $0.005 per share (IBKR tiered)
- Slippage: 5 bps per leg (conservative estimate)
- Borrow cost: 50 bps annualized on short leg

## Implementation Notes

- Strategy is implemented in `strategy.py` using nanobook's positioning API
- Backtest uses historical S&P 100 constituent data and price history
- Results compared against vectorbt for validation

## References

- Jegadeesh, N., & Titman, S. (1993). "Returns to Buying Winners and Selling Losers: Implications for Stock Market Efficiency." *Journal of Finance*, 48(1), 65-91.
- Asness, C. S., Moskowitz, T. J., & Pedersen, L. H. (2013). "Value and Momentum Everywhere." *Journal of Finance*, 68(3), 929-985.

## Alternative Considered: SMA Crossover

**Rejected**: Simple moving average crossover (e.g., 50-day vs 200-day SMA)

**Reasons for rejection**:
1. Less realistic for a market-neutral system (requires directional exposure)
2. Overfitting risk: parameter sensitivity (50/200 vs 20/50 vs 100/200)
3. Limited academic foundation compared to momentum
4. Simpler implementation (less value for demonstrating nanobook's capabilities)

## v0.12 Status

- [x] Strategy implementation in `strategy.py`
- [x] Backtest data pipeline
- [x] vectorbt comparison baseline
- [x] Report generation (equity curve, drawdown, metrics)
- [x] Parity documentation (COMPARISON.md)
- [x] Learnings documentation (portfolio-sim-parity-learnings.md)