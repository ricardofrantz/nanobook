# Ubiquitous Language

## Order lifecycle

| Term                | Definition                                                                 | Aliases to avoid              |
| ------------------- | -------------------------------------------------------------------------- | ---------------------------- |
| **Order**           | A request to buy or sell a specified quantity at a given price           | Request, ticket              |
| **Limit order**     | An order to buy or sell at a specified price or better                  | Price order                  |
| **Market order**    | An order to buy or sell immediately at the best available prices         | Immediate order              |
| **Stop order**      | An order that becomes a market or limit order when a trigger price is reached | Trigger order, conditional order |
| **Aggressor**       | The incoming order that initiates a trade against resting orders          | Taker, incoming order        |
| **Passive order**   | A resting order on the book that provides liquidity                      | Maker, resting order         |
| **Fill**            | The execution of an order, either partial or complete                   | Execution, trade             |
| **Cancel**          | The removal of an order from the book before it is filled                 | Remove, delete               |
| **Modify**          | The change of an order's parameters (cancel and replace)                  | Update, amend                 |
| **Order status**    | The current state of an order in its lifecycle                            | State, condition             |
| **Time-in-force**   | The duration and conditions under which an order remains active          | TIF, duration                |

## Order book

| Term                | Definition                                                                 | Aliases to avoid          |
| ------------------- | -------------------------------------------------------------------------- | ------------------------ |
| **Order book**      | The central data structure organizing all resting orders by price and time | LOB, limit order book    |
| **Bid**             | A buy order on the book                                                  | Buy order, demand         |
| **Ask**             | A sell order on the book                                                 | Sell order, supply        |
| **Best bid**        | The highest price among buy orders                                      | Top bid                   |
| **Best ask**        | The lowest price among sell orders                                     | Top ask, offer            |
| **Spread**          | The difference between the best bid and best ask                         | Bid-ask spread            |
| **Price level**     | All orders at the same price, queued FIFO                               | Level, price point        |
| **Depth**           | The quantity available at each price level                              | Liquidity, book depth      |
| **BBO**             | Best bid and best offer (L1 market data)                                | Best bid/offer, top of book |
| **Cross**           | When an incoming order's price can match against a resting order         | Price match, overlap       |
| **Price improvement**| The aggressor getting the resting order's price (better than their limit)  | Better price               |

## Portfolio

| Term                | Definition                                                                 | Aliases to avoid              |
| ------------------- | -------------------------------------------------------------------------- | ---------------------------- |
| **Portfolio**       | A collection of positions and cash tracking investment performance          | Account, holdings             |
| **Position**        | Holdings in a symbol (quantity, cost basis, PnL)                         | Holding, security position    |
| **Cash**            | Available cash balance in the portfolio                                  | Available funds, liquidity    |
| **Equity**          | Total portfolio value (cash + sum of position market values)              | Portfolio value, net worth    |
| **Weight**          | Fraction of total equity allocated to a position                          | Allocation, percentage       |
| **Rebalancing**     | Adjusting positions to match target weights                             | Reallocation, adjustment      |
| **Market value**    | Current value of a position (quantity × current price)                   | Current value, mark-to-market |
| **Realized PnL**    | Profit or loss from closed positions                                     | Closed PnL, booked profit/loss |
| **Unrealized PnL**  | Profit or loss on open positions (current value - cost basis)             | Paper PnL, open PnL           |
| **Cost basis**      | Weighted average entry price across all fills                           | Average cost, entry price    |
| **Flat position**   | A position with zero quantity (no exposure)                              | Neutral, closed              |

## Trading

| Term                | Definition                                                                 | Aliases to avoid          |
| ------------------- | -------------------------------------------------------------------------- | ------------------------ |
| **Trade**           | A completed transaction between an aggressor and a passive order         | Transaction, fill        |
| **Notional**        | The monetary value of a trade (price × quantity)                         | Trade value, dollar value |
| **Side**            | Whether an order or trade is a buy or sell                              | Direction, action        |
| **Symbol**          | A ticker identifier for a financial instrument (max 8 ASCII bytes)         | Ticker, instrument        |
| **Quantity**        | The number of shares or contracts in an order or trade                    | Size, volume, amount      |
| **Price**           | The monetary value per unit, stored as fixed-point integer in cents         | Rate, quote               |
| **Timestamp**       | A monotonic nanosecond counter for deterministic ordering                  | Time, sequence number     |

## Risk

| Term                | Definition                                                                 | Aliases to avoid              |
| ------------------- | -------------------------------------------------------------------------- | ---------------------------- |
| **Pre-trade check** | Validation of an order against risk limits before submission            | Risk validation, gate       |
| **Position limit**  | Maximum exposure per symbol as a percentage of equity                    | Concentration limit        |
| **Leverage limit**  | Maximum gross exposure relative to equity                                | Exposure limit, margin limit |
| **Short exposure**   | Total value of short positions relative to equity                         | Short risk, short exposure   |
| **Order size limit**| Maximum notional value allowed for a single order                      | Order value limit           |

## Optimization

| Term                | Definition                                                                 | Aliases to avoid              |
| ------------------- | -------------------------------------------------------------------------- | ---------------------------- |
| **Min-variance**   | Portfolio optimization that minimizes variance for a given return        | Variance optimization      |
| **Max-Sharpe**      | Portfolio optimization that maximizes risk-adjusted return               | Sharpe optimization        |
| **Risk-parity**     | Portfolio allocation where each asset contributes equal risk                | Equal risk contribution    |
| **HRP**             | Hierarchical Risk Parity using cluster-based allocation                 | Hierarchical allocation    |
| **Covariance**      | Asset return covariances measuring co-movement                          | Co-movement matrix         |
| **Correlation**     | Normalized covariances ranging from -1 to 1                             | Normalized covariance       |
| **Target weights**  | Desired portfolio allocation per symbol per period                        | Allocation targets, desired weights |

## Backtesting

| Term                | Definition                                                                 | Aliases to avoid              |
| ------------------- | -------------------------------------------------------------------------- | ---------------------------- |
| **Backtest**        | Simulation of a trading strategy on historical data                      | Historical simulation       |
| **Strategy**        | Logic that produces target portfolio weights each period                  | Trading logic, algorithm     |
| **Price schedule**  | Historical prices used for backtesting (per-period symbol prices)         | Historical prices, price series |
| **Weight schedule**  | Target portfolio allocations per period produced by a strategy           | Allocation schedule, target schedule |
| **SimpleFill**      | Instant execution at specified prices (no microstructure simulation)       | Instant execution           |
| **LOBFill**         | Order routing through actual limit order book matching engines            | Realistic execution, microstructure simulation |

## Events

| Term                | Definition                                                                 | Aliases to avoid              |
| ------------------- | -------------------------------------------------------------------------- | ---------------------------- |
| **Event**           | An immutable record of a state change (input only, not outputs like trades) | State change, log entry     |
| **Event log**       | Sequential event stream for deterministic replay                           | Event stream, audit trail    |
| **Replay**          | Reconstruction of exact state from an event log                           | State reconstruction, replay |
| **Apply**           | Processing an event to update exchange state                             | Process, execute            |

## Stop orders

| Term                | Definition                                                                 | Aliases to avoid              |
| ------------------- | -------------------------------------------------------------------------- | ---------------------------- |
| **Stop price**      | The trigger price at which a stop order becomes active                   | Trigger price, activation price |
| **Limit price**     | The limit price for a stop-limit order after triggering                    | Stop limit                   |
| **Trailing stop**   | A stop order where the trigger price tracks favorable price movements   | Dynamic stop, adaptive stop  |
| **Watermark**       | Best price seen since stop submission (high for sell trailing, low for buy) | Peak price, best price seen  |
| **Stop status**     | The current state of a stop order (Pending, Triggered, Cancelled)          | Stop state                   |

## Technical analysis

| Term                | Definition                                                                 | Aliases to avoid              |
| ------------------- | -------------------------------------------------------------------------- | ---------------------------- |
| **RSI**             | Relative Strength Index, a momentum oscillator                            | Relative strength            |
| **MACD**            | Moving Average Convergence Divergence, a trend-following indicator         | Trend indicator             |
| **Bollinger Bands** | Volatility bands around a simple moving average                         | Volatility bands             |
| **ATR**             | Average True Range, a volatility measure using OHLC data                  | True range                   |
| **SMA**             | Simple Moving Average                                                   | Moving average               |
| **EMA**             | Exponential Moving Average                                               | Exponential average         |
| **Wilder's smoothing** | Exponential smoothing with alpha = 1/period (used in RSI, ATR)           | Wilder's EMA                 |
| **Standard EMA**    | Exponential smoothing with alpha = 2/(period+1) (used in MACD)             | Regular EMA                  |

## Broker integration

| Term                | Definition                                                                 | Aliases to avoid              |
| ------------------- | -------------------------------------------------------------------------- | ---------------------------- |
| **Broker**          | External venue for order execution (IBKR, Binance, etc.)                | Exchange, venue             |
| **Broker account**  | An account at a broker holding funds and positions                       | External account, real account |
| **Quote**           | Current market data for a symbol (bid, ask, last, volume, timestamp)      | Market quote, price quote    |
| **Open orders**     | Pending orders at the broker that have not been filled or cancelled         | Working orders, pending orders |
| **Connection**      | The communication channel to a broker (TWS API, REST API)                  | Session, link                |
| **Reconciliation**  | Comparison of actual broker positions against target positions            | Position comparison, diff    |
| **Dry run**         | Execution simulation without actually placing orders                      | Simulation mode, paper trade |
| **Cron mode**       | Automated execution with idempotency checks to prevent double-firing      | Automated mode, scheduled mode |

## Cost modeling

| Term                | Definition                                                                 | Aliases to avoid              |
| ------------------- | -------------------------------------------------------------------------- | ---------------------------- |
| **Commission**      | Fixed fee per trade expressed in basis points of notional                | Transaction fee, broker fee  |
| **Slippage**        | Price impact cost expressed in basis points of notional                    | Market impact, price impact  |
| **Cost model**      | Parameters for computing transaction costs (commission, slippage, min fee) | Fee structure, cost structure |
| **Notional**        | Monetary value of a trade (price × quantity)                              | Trade value, dollar value   |
| **Basis points**    | 1/100 of a percent (0.01%), used for expressing fees and costs              | Bps, percentage points      |
| **Min trade fee**   | Minimum fixed fee per trade regardless of notional                         | Minimum commission, base fee |

## Performance metrics

| Term                | Definition                                                                 | Aliases to avoid              |
| ------------------- | -------------------------------------------------------------------------- | ---------------------------- |
| **Return**          | Periodic portfolio return (equity change / previous equity)                | Period return, gain          |
| **Equity curve**    | Time series of total portfolio value                                    | Portfolio value over time    |
| **Sharpe ratio**    | Risk-adjusted return (mean return / standard deviation)                    | Risk-adjusted return         |
| **Sortino ratio**   | Downside risk-adjusted return (mean return / std dev of negative returns)    | Downside Sharpe              |
| **Max drawdown**    | Maximum peak-to-trough decline as percentage of peak                      | Maximum decline, worst drawdown |
| **Calmar ratio**    | Annualized return divided by max drawdown                                | Return/drawdown ratio       |
| **Win rate**        | Percentage of periods with positive returns                               | Batting average, success rate |
| **Profit factor**   | Sum of positive returns divided by sum of negative returns                | Gain/loss ratio              |

## Volatility forecasting

| Term                | Definition                                                                 | Aliases to avoid              |
| ------------------- | -------------------------------------------------------------------------- | ---------------------------- |
| **Volatility**      | Standard deviation of returns                                            | Risk, variation              |
| **GARCH**           | Generalized Autoregressive Conditional Heteroskedasticity model           | Volatility model             |
| **EWMA**            | Exponentially Weighted Moving Average for volatility smoothing           | Exponential smoothing       |
| **Forecast**        | Predicted future volatility based on historical returns                    | Prediction, projection      |
| **Lambda**          | Ridge regularization parameter for covariance stabilization                  | Regularization parameter    |
| **Alpha**           | Smoothing parameter in exponential moving averages                        | Decay factor, smoothing rate |
| **Beta**            | Smoothing parameter for conditional variance in GARCH                     | Variance persistence        |

## Order book implementation

| Term                | Definition                                                                 | Aliases to avoid              |
| ------------------- | -------------------------------------------------------------------------- | ---------------------------- |
| **Tombstone**       | OrderId(0) marker in queue for O(1) cancellation without removal              | Cancel marker, deletion flag |
| **Compact**          | Removal of tombstones from queues to reclaim memory                        | Cleanup, garbage collection   |
| **Price level queue**| FIFO queue of OrderIds at a specific price                              | Order queue, level queue     |
| **Best price cache** | Cached best bid/ask for O(1) BBO queries without tree traversal             | BBO cache, top-of-book cache  |
| **Order storage**    | Central HashMap of all orders (active and historical) for O(1) lookup        | Order map, order index       |
| **BTreeMap**         | Sorted data structure for price levels (log n operations)                  | Balanced tree, sorted map    |
| **VecDeque**         | Double-ended queue for FIFO order processing (O(1) push/pop from ends)      | Deque, double-ended queue    |

## Self-trade prevention

| Term                | Definition                                                                 | Aliases to avoid              |
| ------------------- | -------------------------------------------------------------------------- | ---------------------------- |
| **STP**             | Self-trade prevention policy to prevent orders from the same owner crossing | Self-match prevention       |
| **Order owner**     | Opaque identifier for the party that submitted an order                    | Owner ID, account ID         |
| **CancelNewest**    | STP policy that cancels the incoming order's remainder                    | Cancel incoming              |
| **CancelOldest**    | STP policy that cancels the resting order                              | Cancel resting               |
| **DecrementAndCancel**| STP policy that cancels the smaller quantity order without trade      | Cancel smaller                |
| **STP off**         | No self-trade prevention (same-owner orders cross normally)              | Disabled STP                 |

## Market data feeds

| Term                | Definition                                                                 | Aliases to avoid              |
| ------------------- | -------------------------------------------------------------------------- | ---------------------------- |
| **ITCH**            | NASDAQ TotalView-ITCH 5.0 binary protocol for real-time market data           | NASDAQ feed, market data feed |
| **ITCH message**    | A binary message in the ITCH protocol (AddOrder, OrderExecuted, etc.)     | Feed message, protocol message |
| **Stock locate**    | ITCH mapping from numeric locate codes to symbol strings                  | Symbol mapping, ticker mapping |
| **Message type**    | The identifier for an ITCH message type (e.g., 'A' for AddOrder)          | Message code, type code       |

## Validation

| Term                | Definition                                                                 | Aliases to avoid              |
| ------------------- | -------------------------------------------------------------------------- | ---------------------------- |
| **ValidationError** | Input validation error (zero price, invalid quantity, etc.)              | Input error, validation error |
| **NotionalOverflow** | Trade notional exceeds i64::MAX (price × quantity overflow)                | Overflow error               |
| **RiskError**       | Risk engine error (invalid config, etc.)                                  | Risk validation error        |
| **BrokerError**     | Broker operation error (connection, authentication, etc.)                | Connection error, API error  |
| **Config validation**| Verification of configuration parameters at construction time          | Parameter validation         |

## Python integration

| Term                | Definition                                                                 | Aliases to avoid              |
| ------------------- | -------------------------------------------------------------------------- | ---------------------------- |
| **PyO3**            | Python bindings for Rust (native extension, outside GIL)                   | Rust-Python bridge            |
| **Native extension** | Compiled Rust code callable from Python                                  | Rust module, compiled binding |
| **Zero-copy**       | Efficient data transfer between Rust and Python without copying            | Direct transfer, no-copy      |
| **Type mapping**     | Conversion between Rust types and Python types                           | Type conversion, marshaling   |
| **GIL**             | Global Interpreter Lock in Python (released during Rust execution)        | Global lock, interpreter lock  |

## Performance

| Term                | Definition                                                                 | Aliases to avoid              |
| ------------------- | -------------------------------------------------------------------------- | ---------------------------- |
| **Throughput**      | Number of operations per second (e.g., 6M ops/sec for LOB matching)        | Ops/sec, operations per second |
| **Latency**          | Time taken to complete an operation (e.g., 155 ns for submit)              | Response time, execution time |
| **BBO query**       | Query for best bid and ask prices (O(1) with cached best price)           | Top-of-book query           |
| **L2 snapshot**     | Query for top N price levels with quantities                             | Depth query, level query      |
| **Microbenchmark**   | Performance measurement of isolated operations                             | Benchmark, perf test         |

## Clustering

| Term                | Definition                                                                 | Aliases to avoid              |
| ------------------- | -------------------------------------------------------------------------- | ---------------------------- |
| **Single-linkage**  | Hierarchical clustering using nearest neighbor distance                  | Nearest neighbor clustering  |
| **Dendrogram**      | Tree diagram showing hierarchical clustering structure                    | Cluster tree, hierarchy tree |
| **Quasi-diagonalization**| Reordering of correlation matrix by clustering dendrogram            | Matrix reordering            |
| **Recursive bisection**| HRP weight allocation by splitting dendrogram recursively              | Hierarchical allocation       |
| **Distance matrix** | Matrix of Euclidean distances between assets based on correlation        | Dissimilarity matrix          |

## Statistics

| Term                | Definition                                                                 | Aliases to avoid              |
| ------------------- | -------------------------------------------------------------------------- | ---------------------------- |
| **Spearman**        | Rank-based correlation coefficient (non-parametric)                      | Rank correlation             |
| **Pearson**         | Linear correlation coefficient                                        | Linear correlation           |
| **Rankdata**        | Computation of ranks with average tie-breaking                         | Ranking, rank computation    |
| **T-distribution**  | Statistical distribution used for p-value computation                   | Student's t                   |
| **Welford algorithm**| Numerically stable online computation of mean and variance                | Online variance computation   |
| **Quintile spread** | Analysis of return distribution across quintiles                       | Distribution analysis         |

## Relationships

- An **Order** belongs to exactly one **Symbol**
- An **Order** produces one or more **Trades**
- A **Trade** involves exactly one **Aggressor** and one **Passive order**
- A **Position** belongs to exactly one **Symbol** in a **Portfolio**
- A **Portfolio** contains zero or more **Positions**
- **Target weights** determine how a **Portfolio** is **rebalanced**
- An **Event** records the submission of an **Order** or other operation
- **Events** are applied to reconstruct **Exchange** state via **replay**
- A **Stop order** triggers when the last trade **price** crosses the **stop price**
- **Pre-trade checks** validate an **Order** against **risk limits** before submission
- A **Broker** holds a **Broker account** with external **Positions**
- **Reconciliation** compares **Broker** **Positions** against **target weights**
- **Commission** and **slippage** are computed from **notional** using the **cost model**
- **Volatility forecasting** uses **GARCH** with **EWMA** smoothing parameters
- **HRP** uses **clustering** and **recursive bisection** to compute **target weights**
- **Tombstone** markers enable **O(1) cancel** in the **price level queue**
- **STP policies** prevent same-**owner** **orders** from crossing
- **ITCH messages** convert to **Events** for deterministic **replay**
- **PyO3** provides **zero-copy** **type mapping** between Rust and Python
- **BBO queries** use a **best price cache** for **O(1) latency**
- **Welford algorithm** provides numerically stable **variance** computation for **Bollinger Bands**
- **Spearman correlation** uses **rankdata** with average tie-breaking
- **Config validation** in the **risk engine** prevents **RiskError** at construction
- **NotionalOverflow** detects when **price × quantity** exceeds **i64::MAX**
- **Dry run** in the **rebalancer** simulates execution without placing real **orders**
- **Cron mode** uses sequence numbers in the audit log for idempotency
- **Price levels** are organized in a **BTreeMap** sorted by **price**
- Each **price level** contains a **VecDeque** **price level queue** of **OrderIds**
- **Order storage** is a central **HashMap** mapping **OrderId** to **Order**
- **A **limit order** rests on the **order book** until filled or cancelled
- **A **market order** executes immediately using **IOC** semantics
- **A **stop order** rests in the **stop book** until triggered
- **Trailing stops** update their **stop price** based on the **watermark**
- **Technical indicators** (RSI, MACD, ATR) use **Wilder's smoothing** or **standard EMA**
- **Performance metrics** (Sharpe, Sortino, Calmar) are computed from the **equity curve**
- **Risk-parity** uses **inverse volatility** to compute **target weights**
- **Min-variance** and **max-Sharpe** use **quadratic programming** for optimization
- **Single-linkage clustering** builds a **dendrogram** from the **distance matrix**
- **Quasi-diagonalization** reorders the **correlation matrix** by the **dendrogram**
- **Recursive bisection** allocates **HRP weights** by splitting the **dendrogram**
- **Covariance** is computed from **returns** and can be ridge-regularized with **lambda**
- **Correlation** is derived from **covariance** by normalizing with **standard deviation**
- **GARCH** uses **alpha** and **beta** parameters for **EWMA-style** volatility smoothing
- **Compact** operations remove **tombstones** from **price level queues**
- **Connection** to a **broker** is established before submitting **orders**
- **A **quote** provides current **bid**, **ask**, **last**, **volume**, and **timestamp**
- **Open orders** are **pending orders** at the **broker** that have not been **filled**
- **Basis points** express **commission** and **slippage** as a percentage of **notional**
- **Profit factor** is the ratio of positive **returns** to negative **returns**
- **Sortino ratio** uses only downside **returns** in the denominator
- **Max drawdown** is the maximum peak-to-trough decline in the **equity curve**
- **Win rate** is the percentage of periods with positive **portfolio return**
- **Lambda** is the **ridge regularization parameter** for **covariance** stabilization
- **Alpha** and **beta** are smoothing parameters in **GARCH** **EWMA** recursion
- **Stock locate** maps numeric codes to **symbol** strings in **ITCH** parsing
- **ValidationError** indicates invalid input (zero **price**, invalid **quantity**)
- **RiskError** indicates invalid **risk engine** configuration
- **BrokerError** indicates **broker** operation failures
- **Type mapping** converts Rust types to Python types in **PyO3** bindings
- The **GIL** is released during Rust code execution in **PyO3**
- **Throughput** is measured in operations per second (e.g., 6M ops/sec)
- **Latency** is the time taken to complete an operation (e.g., 155 ns for submit)
- **L2 snapshot** returns the top N **price levels** with quantities
- **Rankdata** assigns ranks with average tie-breaking for **Spearman correlation**
- **T-distribution** is used for p-value computation in **Spearman correlation**
- **Min trade fee** ensures a minimum cost regardless of **notional** size

## Flagged ambiguities

- "Position" was used in two contexts: (1) a holding in a **Portfolio** (quantity, PnL) and (2) a location in an **order book queue**. These are distinct concepts: a **Position** represents an investment holding, while a queue position is an implementation detail of the **order book**. We use "queue position" or "index" for the order book context and reserve "**Position**" for portfolio holdings.
- "Execution" was used for both (1) **trade** execution (fills, matches) and (2) **backtest** execution mode (**SimpleFill** vs **LOBFill**). These are distinct: trade execution is the matching of orders in the order book, while backtest execution mode is a simulation parameter. We use "**fill**" or "**match**" for trade execution and reserve "**execution mode**" for the backtest parameter.
- "Account" was used to mean both (1) a **broker account** (IBKR, Binance) and (2) a **portfolio** (investment holdings). These are distinct: a **broker account** is external to nanobook and holds funds and positions at a broker, while a **Portfolio** is nanobook's internal simulation of holdings and cash. We use "**broker account**" for the external concept and reserve "**Portfolio**" for the internal simulation.
- "Return" can refer to either (1) **portfolio return** (equity change) or (2) **asset return** (price change). Context makes this distinction clear, but when precision matters, use "**portfolio return**" or "**asset return**" explicitly.
- "Volatility" can refer to either (1) **historical volatility** (computed from past returns) or (2) **forecast volatility** (predicted future volatility). Context makes this distinction clear, but when precision matters, use "**historical volatility**" or "**forecast volatility**" explicitly.
- "Order" has subtypes (limit order, market order, stop order, trailing stop). These are hierarchical concepts under the general term "**order**". Use the specific subtype when the distinction matters.

## Example dialogue

> **Dev:** "When a **Strategy** produces **target weights**, do we create **orders** immediately?"
> 
> **Domain expert:** "No — the **backtest bridge** first validates the inputs, then the **Portfolio** is **rebalanced** using **SimpleFill** or **LOBFill**. **Rebalancing** creates **orders** that execute against the **order book**."
> 
> **Dev:** "So if we use **LOBFill**, the **orders** might only partially **fill**?"
> 
> **Domain expert:** "Exactly. **LOBFill** routes **orders** through actual matching engines, so they can be partially filled based on available liquidity. **SimpleFill** assumes instant execution at the specified prices with no microstructure."
> 
> **Dev:** "And **stop orders** in the simulation?"
> 
> **Domain expert:** "The **backtest bridge** can simulate **stop orders** using the **BacktestStopConfig**. When a **stop price** is breached, it emits a **BacktestStopEvent** and exits the **position**. The **stop** can be fixed, trailing, or ATR-based."
> 
> **Dev:** "What about **transaction costs** during **rebalancing**?"
> 
> **Domain expert:** "The **cost model** applies **commission** and **slippage** based on the **notional** value of each trade. **Commission** is a fixed percentage of **notional** in **basis points**, while **slippage** models price impact. The **cost model** can also include a **min trade fee** for very small orders."
> 
> **Dev:** "If we're using a real **broker** instead of simulation, how does **reconciliation** work?"
> 
> **Domain expert:** "The **rebalancer** CLI compares the actual **positions** in your **broker account** against the **target weights**. It computes the diff, shows you a plan, and you can do a **dry run** first. In **cron mode**, it uses sequence numbers in the audit log to prevent double-firing if the job runs multiple times."
> 
> **Dev:** "What if two orders from the same **owner** would cross in the **order book**?"
> 
> **Domain expert:** "That's where **STP** (self-trade prevention) comes in. You can set the policy to **CancelNewest**, **CancelOldest**, or **DecrementAndCancel**. **STP off** means same-**owner** orders cross normally. The **order owner** is an opaque identifier you assign — it could map to a user ID, account ID, or desk ID."
> 
> **Dev:** "How does the **order book** achieve **O(1) cancel** latency?"
> 
> **Domain expert:** "It uses a **tombstone** mechanism. Instead of removing an **order** from the **price level queue**, we mark it with OrderId(0). The matching engine skips these tombstones during fills. Periodically, you call **compact** to clean up the tombstones and reclaim memory."
> 
> **Dev:** "For **HRP optimization**, how do we compute the **distance matrix**?"
> 
> **Domain expert:** "First we compute the **correlation matrix** from returns. Then we convert correlation to Euclidean distance using d = sqrt(2 * (1 - correlation)). **Single-linkage clustering** builds a **dendrogram** from this distance matrix. **Quasi-diagonalization** reorders the matrix by the clustering structure, and **recursive bisection** allocates weights by splitting the dendrogram."