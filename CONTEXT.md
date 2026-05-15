# CONTEXT.md

This document captures the domain concepts, terminology, and technical implementation details used in the nanobook codebase.

## Domain Overview

Nanobook is a **Rust execution layer for Python trading strategies**. It implements a deterministic limit-order-book (LOB) matching engine, portfolio simulator, risk engine, and broker abstraction for trading system backtesting and execution.

**Core separation of concerns:**
- Python layer: Strategy logic (factors, signals, sizing, scheduling) - decides **what** to trade
- Rust layer: Execution mechanics (order routing, portfolio accounting, risk checks) - accounts for **what happened**

**Design philosophy:**
- Determinism: Same inputs always produce identical outputs (no randomness)
- Performance: LOB matching at 6M ops/sec, outside Python GIL
- Auditability: Event sourcing, property tests, mutation testing
- Sharp boundary: Python for strategy, Rust for execution

## Domain Concepts

### Trading Execution Domain

**Orders**
- **Limit order**: Order to buy/sell at a specified price or better; rests on book if not immediately filled
- **Market order**: Order to buy/sell immediately at best available prices; implemented as limit order at Price::MAX (buy) or Price::MIN (sell) with IOC semantics
- **Stop order**: Order that becomes a market/limit order when a trigger price is reached (stop-market, stop-limit)
- **Trailing stop**: Stop order where the trigger price tracks favorable price movements (fixed offset, percentage, adaptive)
- **Aggressor order**: Incoming order that initiates a trade against resting orders (taker)
- **Passive order**: Resting order on the book that provides liquidity (maker)

**Order Lifecycle**
- **New**: Order accepted, resting on book with no fills yet
- **PartiallyFilled**: Some quantity filled, remainder still on book
- **Filled**: Fully executed, no longer on book
- **Cancelled**: Removed by user request or TIF rules, no longer on book
- **Terminal state**: Filled or Cancelled (no further state changes possible)
- **Active state**: New or PartiallyFilled (can be filled or cancelled)

**Order Book (LOB)**
- **Limit Order Book (LOB)**: Central data structure organizing resting orders by price and time
- **Price levels**: Orders at the same price, queued FIFO; each level maintains total quantity and order count
- **Bid**: Buy orders on the book (highest bid is best bid); sorted high → low
- **Ask**: Sell orders on the book (lowest ask is best ask); sorted low → high
- **Spread**: Difference between best bid and best ask
- **BBO (Best Bid/Offer)**: L1 market data (best bid price, best ask price)
- **Depth**: Quantity available at each price level (L2, L3 market data)
- **Order queue**: FIFO queue of OrderIds at each price level; stores only IDs, full orders in central HashMap

**Matching Engine**
- **Price-time priority**: Matching algorithm where better prices match first, FIFO at same price
- **Cross condition**: Buy crosses if buy_price >= ask_price; Sell crosses if sell_price <= bid_price
- **Price improvement**: Aggressor gets the resting order's price (better than their limit)
- **Self-trade prevention (STP)**: Policy to prevent orders from the same owner crossing:
  - **Off**: No prevention (default)
  - **CancelNewest**: Cancel incoming order's remainder; leave resting intact
  - **CancelOldest**: Cancel resting order; incoming continues matching
  - **DecrementAndCancel**: Cancel smaller quantity order; no trade generated
- **Tombstone cancellation**: O(1) cancellation by marking OrderId(0) in queue position instead of removal
- **Compact operation**: Remove tombstones from queues to reclaim memory

**Time-in-Force (TIF)**
- **GTC (Good-Til-Cancelled)**: Order remains on book until filled or cancelled; partial fills allowed
- **IOC (Immediate-or-Cancel)**: Fill whatever possible immediately; remainder cancelled
- **FOK (Fill-or-Kill)**: All-or-nothing; either fills completely or cancelled

**Stop Orders**
- **Stop trigger conditions**: Buy stop triggers when last_trade_price >= stop_price; Sell stop triggers when last_trade_price <= stop_price
- **Stop types**: Stop-market (becomes market order), Stop-limit (becomes limit order at limit_price)
- **Trailing methods**:
  - **Fixed**: Fixed offset in cents from watermark price
  - **Percentage**: Percentage of watermark price (e.g., 0.05 = 5%)
  - **SmaAbsChange**: Adaptive trail using simple moving average of absolute price changes
- **Watermark**: Best price seen since stop submission (high for sell trailing, low for buy trailing)
- **Stop status**: Pending (waiting), Triggered (submitted to book), Cancelled
- **Cascade triggering**: Up to 100 iterations of stop triggers per trade to handle chain reactions

### Portfolio & Accounting Domain

**Portfolio**
- **Cash**: Available cash balance (in cents); decreased on buys, increased on sells
- **Position**: Holdings in a symbol (quantity, average cost, realized PnL, unrealized PnL)
- **Equity**: Total portfolio value (cash + sum of position market values)
- **Weights**: Fraction of total equity allocated to each position; cash implicitly = 1 - sum(weights)
- **Rebalancing**: Adjusting positions to match target weights; positions not in targets are closed

**Position Tracking**
- **Market value**: Current value of position (quantity × current price)
- **Realized PnL**: Profit/loss from closed positions (cumulative)
- **Unrealized PnL**: Profit/loss on open positions (current value - cost basis)
- **Flat position**: Zero quantity (no exposure)
- **Cost basis**: Weighted average entry price across all fills

**Transaction Costs**
- **Commission**: Fixed fee per trade (basis points of notional)
- **Slippage**: Price impact cost (basis points of notional)
- **Cost model**: Parameters for computing transaction costs (commission_bps, slippage_bps, min_trade_fee)
- **Cost computation**: cost = max(commission_bps × notional / 10000, min_trade_fee) + slippage_bps × notional / 10000

**Performance Metrics**
- **Return**: Periodic portfolio return (equity change / previous equity)
- **Equity curve**: Time series of total portfolio value (one entry per record_return call)
- **Sharpe ratio**: Risk-adjusted return (mean return / std dev) using periods_per_year for annualization
- **Max drawdown**: Maximum peak-to-trough decline (peak - trough) / peak
- **Sortino ratio**: Downside risk-adjusted return (mean return / std dev of negative returns)
- **Calmar ratio**: Annualized return / max drawdown
- **Win rate**: Percentage of periods with positive returns
- **Profit factor**: Sum of positive returns / sum of negative returns

**Execution Modes**
- **SimpleFill**: Instant execution at specified prices (fast parameter sweeps, no microstructure)
- **LOBFill**: Route orders through actual LOB matching engines (realistic microstructure, partial fills, price impact)

**Strategy Pattern**
- **Strategy trait**: Interface for batch-oriented backtesting; implement compute_weights() to generate target allocations
- **Bar-oriented**: Each bar calls compute_weights() with current prices and portfolio state
- **Backtest runner**: Handles rebalancing, return recording, and metrics computation

### Market Data Domain

**Price**
- **Fixed-point price**: Integer representation in smallest currency unit (cents for USD); avoids floating-point errors
- **Tick size**: Minimum price increment (implicit in cents representation)
- **Notional**: Monetary value of a trade (price × quantity); uses checked arithmetic to prevent overflow
- **Price constants**: Price::ZERO, Price::MAX (for market buys), Price::MIN (for market sells)
- **Display format**: Formats as dollars.cents (e.g., Price(10050) → "$100.50")

**Identifiers**
- **Symbol**: Ticker identifier (e.g., "AAPL", "MSFT"), max 8 ASCII bytes; stored inline as [u8; 8] with length byte
- **OrderId**: Unique order identifier assigned by exchange (monotonically increasing)
- **TradeId**: Unique trade identifier assigned by exchange (monotonically increasing)
- **Timestamp**: Monotonic nanosecond counter (not wall clock) for deterministic ordering
- **OrderOwner**: Opaque u32 identifier for self-trade prevention; None opts out of STP

**Sides**
- **Buy**: Long side, profit from price increase
- **Sell**: Short side, profit from price decrease
- **Opposite method**: Side::opposite() returns the opposite side

**Market Data Feeds**
- **ITCH**: NASDAQ ITCH 5.0 binary protocol parser for real-time market data
- **ITCH message types**: AddOrder, OrderExecuted, OrderCancel, OrderDelete, OrderReplace, Trade, StockDirectory
- **Event conversion**: ITCH messages convert to nanobook Events for deterministic replay

### Risk Management Domain

**Pre-trade Risk Checks**
- **Position limits**: Maximum exposure per symbol (percentage of equity); post-order position value / equity
- **Leverage limits**: Maximum gross exposure relative to equity; sum of absolute position values / equity
- **Short exposure limits**: Maximum short position size; total short value / equity
- **Order size limits**: Maximum notional value per order (in cents)
- **Short selling control**: allow_short flag to enable/disable short positions

**Risk Engine**
- Validates orders before submission using RiskEngine::check_order()
- Returns RiskReport with individual RiskCheck results (Pass/Fail with details)
- Configurable limits via RiskConfig (max_position_pct, max_order_value_cents, allow_short, etc.)
- Fallible construction: RiskEngine::new() validates config at construction time (NaN, out-of-range checks)
- Batch checking: check_batch() for multiple orders

**Risk Report Structure**
- **RiskStatus**: Pass, Fail, or Skip
- **RiskCheck**: Individual check result with name, status, and detail string
- **RiskReport**: Collection of checks with has_failures() method

### Broker Integration Domain

**Broker Abstraction**
- **Broker trait**: Generic interface for order routing to external venues
- **IBKR (Interactive Brokers)**: Retail broker adapter via TWS API (binary protocol)
- **Binance**: Cryptocurrency exchange adapter via REST API (HTTP/JSON)
- **Mock broker**: In-memory broker for testing (no external connection)

**Broker Operations**
- **connect()**: Establish connection to broker
- **disconnect()**: Graceful shutdown
- **positions()**: Get all current positions (symbol, quantity, side)
- **account()**: Get account summary (equity, buying power, cash, gross position value)
- **submit_order()**: Submit order; returns OrderId
- **order_status()**: Get status of submitted order (status, filled_quantity, remaining_quantity, avg_fill_price)
- **open_orders()**: Get all pending orders from broker
- **cancel_order()**: Cancel a pending order
- **quote()**: Get current quote for symbol (bid, ask, last, volume, timestamp)

**Broker Types**
- **BrokerOrder**: Order specification (symbol, side, quantity, order_type, client_order_id)
- **BrokerOrderType**: Limit(price), Market, StopMarket(stop_price), StopLimit(stop_price, limit_price)
- **BrokerSide**: Buy, Sell
- **Position**: Symbol, quantity, side (long/short)
- **Account**: equity_cents, buying_power_cents, cash_cents, gross_position_value_cents
- **Quote**: symbol, bid_cents, ask_cents, last_cents, volume, timestamp (SystemTime)
- **BrokerOrderStatus**: id, status (Submitted, Filled, Cancelled, etc.), filled_quantity, remaining_quantity, avg_fill_price_cents

**Rebalancer CLI**
- **run**: Compute diff, confirm, execute rebalance orders (with dry_run, force, cron_mode options)
- **positions**: Show current IBKR positions
- **status**: Check IBKR connection
- **reconcile**: Compare actual positions vs target
- **kill**: Send SIGTERM to running runner and verify no dangling orders
- **recover**: Recover from crash using audit log
- **TargetSpec**: Target weights file (JSON) for rebalancing
- **Cron mode**: Idempotency checks via sequence numbers in audit log for automated execution

### Optimization Domain

**Portfolio Optimization**
- **Min-variance**: Minimize portfolio variance for given return using quadratic programming
- **Max-Sharpe**: Maximize risk-adjusted return (Sharpe ratio) using quadratic programming
- **Risk-parity**: Equal risk contribution across assets using inverse volatility scaling
- **HRP (Hierarchical Risk Parity)**: Cluster-based allocation using hierarchical clustering (López de Prado 2016):
  - **correlation_matrix()**: Compute correlation from covariance (raw or ridge-regularized)
  - **distance_matrix()**: Convert correlation to Euclidean distance: d = sqrt(2 * (1 - correlation))
  - **single_linkage_clustering()**: Hierarchical clustering using single-linkage (nearest neighbor)
  - **hrp_quasi_diagonalization()**: Reorder matrix by clustering dendrogram
  - **hrp_recursive_bisection()**: Allocate weights by recursive bisection of dendrogram
- **Inverse CVaR**: Minimize conditional value-at-risk (CVaR) at given confidence level
- **Inverse CDaR**: Minimize conditional drawdown-at-risk (CDaR) at given confidence level

**Risk Estimation**
- **Covariance matrix**: Asset return covariances; computed from returns matrix
- **Correlation matrix**: Normalized covariances (correlation = covariance / (std_i * std_j))
- **Raw covariance**: Unregularized covariance from returns (used for correlation computation)
- **Ridge-regularized covariance**: Covariance + lambda * I (lambda = ridge_parameter) for numerical stability
- **GARCH EWMA**: Volatility forecasting using fixed parameters (alpha = 0.08, beta = 0.90):
  - **EWMA-style recursion**: h[t+1] = omega + sum(alpha_j * eps[t+1-j]^2) + sum(beta_k * h[t+1-k])
  - **Mean options**: "zero" (assume zero mean), "constant" (use sample mean)
  - **Fallback**: Sample volatility on invalid/non-finite inputs
  - **Not MLE**: This is NOT maximum-likelihood GARCH; use Python `arch` package for MLE

**Optimization Algorithms**
- **Quadratic programming**: Used for min-variance and max-Sharpe (convex optimization)
- **Clustering**: Hierarchical clustering for HRP (single-linkage, O(n³) complexity)
- **Inverse volatility**: Risk parity weights = (1/volatility) / sum(1/volatility)
- **Recursive bisection**: HRP weight allocation by splitting dendrogram

### Backtesting Domain

**Strategy Execution**
- **Target weights**: Desired portfolio allocation per symbol per period (Python → Rust)
- **Price schedule**: Historical prices for backtesting (per-period symbol prices)
- **Backtest bridge**: Python → Rust interface for strategy backtesting (backtest_weights function)
- **Parameter sweep**: Parallel evaluation of strategy parameters using Rayon
- **Stop simulation**: Optional stop-loss simulation in backtesting (fixed, trailing, ATR-based)

**Execution Modes**
- **SimpleFill**: Instant execution at specified prices (fast parameter sweeps, no microstructure)
- **LOBFill**: Route orders through actual LOB matching engines (realistic microstructure, partial fills, price impact)

**Backtest Bridge**
- **backtest_weights()**: Main entry point; takes weight_schedule, price_schedule, initial_cash, cost_bps
- **BacktestStopConfig**: Stop simulation configuration (fixed_stop_pct, trailing_stop_pct, atr_multiple, atr_period)
- **BacktestStopEvent**: Stop trigger metadata (period_index, symbol, trigger_price, exit_price, reason)
- **BacktestBridgeResult**: Returns returns, equity_curve, final_cash, metrics, holdings, symbol_returns, stop_events
- **Input validation**: Rejects mismatched schedules, non-positive cash, NaN weights, negative prices, cost > 100%

**Stop Simulation**
- **Fixed stop**: Exit if price drops by fixed percentage from entry price
- **Trailing stop**: Exit if price drops by percentage from watermark (highest price seen)
- **ATR stop**: Exit if price drops by multiplier × ATR from watermark
- **Stop events**: Emitted when stops trigger; include trigger reason and prices

### Event Domain

**Event Sourcing**
- **Event**: Immutable record of state change (input only, not outputs like trades)
- **Event types**: SubmitLimit, SubmitMarket, Cancel, Modify, SubmitStopMarket, SubmitStopLimit, SubmitTrailingStopMarket, SubmitTrailingStopLimit
- **Event log**: Sequential event stream for deterministic replay
- **Replay**: Reconstruct exact state from event log (Exchange::replay(events))
- **Apply method**: Exchange::apply(event) processes single event and returns trades
- **Apply all**: Exchange::apply_all(events) processes multiple events and returns all trades
- **Deterministic guarantee**: Same event sequence always produces identical state

**Event Properties**
- **Immutable**: Events never change once created
- **Inputs only**: Events capture what happened to the exchange, not what the exchange produced
- **Replayable**: Can reconstruct exact state from event log
- **Serializable**: Events support serde serialization for persistence

## Technical Analysis Domain

**Technical Indicators (TA-Lib replacements)**
- **RSI (Relative Strength Index)**: Momentum oscillator using Wilder's smoothing (alpha = 1/period, not 2/(period+1))
  - Lookback: first `period` elements are NaN
  - Edge cases: flat price returns 0.0, always up returns 100.0
- **MACD (Moving Average Convergence Divergence)**: Trend-following indicator
  - Fast EMA: typically 12-period (alpha = 2/(12+1))
  - Slow EMA: typically 26-period (alpha = 2/(26+1))
  - Signal line: 9-period EMA of MACD
  - Standard EMA smoothing (alpha = 2/(period+1))
- **Bollinger Bands**: Volatility bands around SMA
  - Middle band: SMA (typically 20-period)
  - Upper band: SMA + 2 × std (population std, ddof=0)
  - Lower band: SMA - 2 × std
  - Numerical stability: Uses Welford algorithm to avoid catastrophic cancellation
- **ATR (Average True Range)**: Volatility measure using OHLC data
  - True range: max(high - low, |high - close_prev|, |low - close_prev|)
  - Wilder's smoothing: alpha = 1/period

**Smoothing Methods**
- **Standard EMA**: alpha = 2/(period+1) (used in MACD)
- **Wilder's smoothing**: alpha = 1/period (used in RSI, ATR)
- **SMA (Simple Moving Average)**: Rolling window average
- **Welford algorithm**: Numerically stable online computation of mean and variance

## Statistical Domain

**Correlation Analysis**
- **Spearman rank correlation**: Non-parametric correlation using rank data
  - Average tie-breaking for equal values (matches scipy default)
  - NaN propagation: any NaN in input produces all-NaN output
  - t-distribution for p-values
- **Pearson correlation**: Linear correlation using raw values
- **Rankdata**: Compute ranks with average tie-breaking (1-based ranking)

**Statistical Functions**
- **Welford mean/m2**: Numerically stable online computation of mean and sum of squared deviations
  - Avoids catastrophic cancellation on high-mean, low-variance series
  - Used for rolling-window statistics and Bollinger Bands
- **Quintile spread**: Analysis of return distribution across quintiles
- **Deflated Sharpe**: Risk-adjusted return accounting for skewness and kurtosis

**Cross-Validation**
- **Time series split**: Cross-validation for time series data (no look-ahead)
  - Respects temporal ordering
  - sklearn replacement for time series data

## Data Structures & Algorithms

**Order Book Data Structures**
- **BTreeMap<Price, Level>**: Sorted price levels (O(log n) insert/remove)
- **VecDeque<OrderId>**: FIFO queue at each price level (O(1) push/pop from ends)
- **HashMap<OrderId, Order>**: Central order storage (O(1) lookup by ID)
- **Cached best price**: O(1) BBO queries without tree traversal
- **Tombstone mechanism**: OrderId(0) marks cancelled orders in queue (O(1) cancel)

**Stop Book Data Structures**
- **BTreeMap<Price, Vec<OrderId>>**: Stop orders indexed by trigger price (efficient range queries)
- **HashMap<OrderId, StopOrder>**: Central stop order storage
- **Vec<OrderId>**: Trailing stop IDs for efficient iteration
- **Rolling price changes**: Fixed-size buffer for ATR computation (max 10,000 entries)

**Numerical Algorithms**
- **Checked arithmetic**: Overflow detection for trade notional and VWAP
- **Saturating arithmetic**: Prevents underflow/overflow in quantity calculations
- **Welford algorithm**: Numerically stable variance computation
- **Regularized incomplete beta**: For t-distribution CDF (p-value computation)
- **Decaying weights**: For GARCH EWMA coefficients (geometric decay)

**Performance Characteristics**
- **LOB matching**: ~6M ops/sec (single-threaded)
- **Submit (no match)**: ~155 ns
- **Submit (with match)**: ~197 ns
- **BBO query**: ~1.1 ns
- **Cancel (tombstone)**: ~385 ns
- **L2 snapshot (10 levels)**: ~255 ns
- **ITCH parsing**: ~83 ns per message
- **Book update**: ~250 ns per message

## Error Handling & Validation

**Error Types**
- **ValidationError**: Input validation errors (zero price, invalid quantity, etc.)
- **BrokerError**: Broker operation errors (connection, authentication, etc.)
- **RiskError**: Risk engine errors (invalid config, etc.)
- **NotionalOverflow**: Trade notional overflow (price × quantity exceeds i64::MAX)

**Validation Points**
- **Order submission**: try_submit_* methods validate before processing
- **Risk engine**: Config validation at construction (NaN, out-of-range checks)
- **Backtest bridge**: Input validation (schedule lengths, cash, weights, prices)
- **Broker operations**: Connection validation, order validation

## Python Integration

**PyO3 Bindings**
- **Native extension**: Compiled Rust code callable from Python (outside GIL)
- **Zero-copy**: Efficient data transfer between Rust and Python
- **Type mapping**: Rust types map to Python types (Price → int, Symbol → str, etc.)
- **Error handling**: Rust Results map to Python exceptions

**Python API Surface**
- **Exchange**: PyExchange class with order submission, cancellation, queries
- **Portfolio**: PyPortfolio class with rebalancing, metrics computation
- **Optimizers**: Functions for min-variance, max-Sharpe, risk-parity, HRP, inverse CVaR/CDaR
- **GARCH**: garch_ewma_forecast function for volatility forecasting
- **Indicators**: RSI, MACD, Bollinger Bands, ATR functions
- **Statistics**: Spearman correlation, quintile spread, deflated Sharpe
- **Cross-validation**: time_series_split function
- **Backtest bridge**: backtest_weights function for strategy backtesting

## Generic Programming Concepts (Not Domain-Specific)

The following are generic programming concepts used in the codebase but not specific to the trading domain:

- **HashMap/HashSet**: Data structures
- **Vec/Array**: Collections
- **Option/Result**: Error handling
- **Traits**: Rust abstraction mechanism
- **Generics**: Type parameters
- **Iterators**: Lazy sequence processing
- **Serde**: Serialization framework
- **PyO3**: Python bindings
- **Rayon**: Parallel processing

## Key Architectural Decisions

1. **Determinism**: No randomness; same inputs always produce identical outputs
2. **Single-threaded**: Simplifies reasoning and ensures determinism (no race conditions)
3. **Fixed-point arithmetic**: Prices as integers to avoid floating-point errors in financial calculations
4. **Event sourcing**: All state changes recorded as events for replay and audit trails
5. **Sharp boundary**: Python for strategy (what to trade), Rust for execution (how it happened)
6. **Performance focus**: LOB matching at 6M ops/sec, outside Python GIL
7. **In-process design**: No networking overhead; wrap externally if needed
8. **Execution scope, not compliance**: Deterministic STP policies included; regulatory workflows out of scope
9. **No complex order types**: No iceberg or pegged orders (simplifies matching engine)
10. **Checked arithmetic**: Overflow detection in critical paths (trade notional, VWAP)

## Domain Relationships

**Order → Trade Flow**
- Order submitted → matching engine → cross detection → trade generation
- Aggressor order (incoming) + Passive order (resting) → Trade
- Trade executed at passive order's price (price improvement for aggressor)

**Portfolio → Optimization Flow**
- Returns matrix → covariance/correlation → optimizer → target weights
- Portfolio rebalancing → target weights → order execution → position updates
- Cost modeling → transaction costs → portfolio equity curve → performance metrics

**Risk → Broker Flow**
- Strategy signal → risk check → broker order submission → execution confirmation
- Risk limits (position, leverage, short) → pre-trade validation → order blocking
- Account reconciliation → broker positions vs target positions → diff calculation

**Event → State Flow**
- Event submission → event log → state application → trade generation
- Event replay → state reconstruction → audit trail verification
- Event persistence → JSON Lines → crash recovery

## File Structure Mapping

**Core Types & Data Structures**
- `src/types.rs`: Core domain types (Price, Quantity, Symbol, OrderId, TradeId, Timestamp)
- `src/order.rs`: Order representation and lifecycle (Order, OrderStatus, OrderOwner)
- `src/trade.rs`: Trade representation (Trade, VWAP computation, notional calculation)
- `src/side.rs`: Side enum (Buy, Sell) with opposite() method
- `src/tif.rs`: Time-in-force enum (GTC, IOC, FOK)

**Order Book & Matching Engine**
- `src/book.rs`: OrderBook data structure (bids, asks, orders map, ID/timestamp generation)
- `src/level.rs`: Price level FIFO queue (VecDeque<OrderId>, tombstone mechanism)
- `src/price_levels.rs`: One side of order book (BTreeMap<Price, Level>, cached best price)
- `src/matching.rs`: Matching engine algorithm (price-time priority, STP policies)
- `src/exchange.rs`: High-level exchange API (order submission, cancellation, queries)

**Stop Orders**
- `src/stop.rs`: Stop orders and trailing stops (StopBook, StopOrder, TrailMethod, trigger logic)

**Portfolio & Optimization**
- `src/portfolio/mod.rs`: Portfolio simulation (Portfolio, rebalancing, equity tracking)
- `src/portfolio/position.rs`: Position tracking (Position, market value, PnL)
- `src/portfolio/cost_model.rs`: Transaction cost modeling (CostModel, commission, slippage)
- `src/portfolio/metrics.rs`: Performance metrics (Metrics, Sharpe, drawdown, etc.)
- `src/portfolio/strategy.rs`: Strategy trait and backtest runner (Strategy, run_backtest)
- `src/optimize.rs`: Portfolio optimizers (min-variance, max-Sharpe, risk-parity, HRP, inverse CVaR/CDaR)
- `src/garch.rs`: GARCH EWMA volatility forecasting (garch_ewma_forecast)

**Technical Analysis & Statistics**
- `src/indicators.rs`: Technical indicators (RSI, MACD, Bollinger Bands, ATR) - TA-Lib replacements
- `src/stats.rs`: Statistical functions (Spearman correlation, quintile spread, deflated Sharpe)

**Event Sourcing**
- `src/event.rs`: Event types and replay logic (Event enum, Exchange::apply, Exchange::replay)

**Market Data Feeds**
- `src/itch.rs`: NASDAQ ITCH 5.0 parser (ItchParser, ItchMessage, event conversion)

**Error Handling**
- `src/error.rs`: Error types (ValidationError, BrokerError, RiskError, NotionalOverflow)

**Python Bindings**
- `python/src/lib.rs`: PyO3 module registration and function exports
- `python/src/exchange.rs`: PyExchange bindings
- `python/src/portfolio.rs`: PyPortfolio, PyPosition, PyCostModel, PyMetrics bindings
- `python/src/optimize.rs`: Optimizer function bindings
- `python/src/garch.rs`: GARCH function bindings
- `python/src/indicators.rs`: Technical indicator function bindings
- `python/src/stats.rs`: Statistical function bindings
- `python/src/backtest_bridge.rs`: Backtest bridge function bindings
- `python/src/broker.rs`: Broker bindings (PyIbkrBroker, PyBinanceBroker)
- `python/src/risk.rs`: Risk engine bindings (PyRiskEngine)

**Workspace Crates**
- `broker/src/lib.rs`: Broker trait and implementations (IBKR, Binance, mock)
- `risk/src/lib.rs`: Pre-trade risk engine (RiskEngine, RiskConfig, RiskReport)
- `rebalancer/src/main.rs`: CLI for IBKR rebalancing (run, positions, status, reconcile, kill, recover)

**Testing & Validation**
- `tests/`: Integration tests
- `fuzz/`: Fuzz harnesses for matching and ITCH
- `benches/`: Performance benchmarks
