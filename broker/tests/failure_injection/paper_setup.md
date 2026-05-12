# IBKR Paper Trading Setup Guide

This document describes how to set up an IBKR paper trading account for validating the MockTws implementation against real IBKR behavior.

## Prerequisites

1. **IBKR Account**: You need an Interactive Brokers account with paper trading enabled
2. **TWS or IB Gateway**: Install either TWS (Trader Workstation) or IB Gateway
3. **API Access**: Enable API access in your IBKR account settings

## Enabling Paper Trading

### Step 1: Enable Paper Trading in Account Settings

1. Log in to your IBKR account at [interactivebrokers.com](https://www.interactivebrokers.com)
2. Navigate to **Account Management** → **Settings** → **Account Settings**
3. Look for **Paper Trading Account** section
4. Click **Configure** to enable paper trading
5. Your paper trading account will have a separate account ID (typically starts with "DU" or similar)

### Step 2: Enable API Access

1. In Account Management, navigate to **Settings** → **API** → **Settings**
2. Enable **Active API** for your paper trading account
3. Set **API IP Restrictions** to allow your local IP address (or set to "0.0.0.0/0" for testing only)
4. Note your **Master API Key** and **Trading Key** if using API key authentication

## Installing TWS or IB Gateway

### Option A: TWS (Trader Workstation)

1. Download TWS from [IBKR Downloads](https://www.interactivebrokers.com/en/trading/tws.php)
2. Install and launch TWS
3. Log in with your paper trading account credentials
4. Configure TWS for API access (see below)

### Option B: IB Gateway (Recommended for Headless Testing)

1. Download IB Gateway from [IBKR Downloads](https://www.interactivebrokers.com/en/trading/ibgateway-stable.php)
2. Install and launch IB Gateway
3. Log in with your paper trading account credentials
4. Configure IB Gateway for API access (see below)

## Configuring TWS/IB Gateway for API Access

### Step 1: Enable API Port

1. In TWS/IB Gateway, go to **File** → **Global Configuration**
2. Navigate to **API** → **Settings**
3. Check **Enable ActiveX and Socket Clients**
4. Set **Socket Port**:
   - Paper trading: `7497` (default) or custom port
   - Production trading: `7496` (default) - **DO NOT USE FOR VALIDATION**
5. Uncheck **Read-Only API** if you want to test order submission
6. Set **Master API Client ID** (typically `0` or `1`)

### Step 2: Configure Trusted IPs

1. In the same API Settings section
2. Add your local IP to **Trusted IPs** list
3. For local testing, you can use `127.0.0.1`

### Step 3: Save and Restart

1. Click **OK** to save settings
2. Restart TWS/IB Gateway for changes to take effect

## Environment Variables

The validation script uses the following environment variables:

```bash
# IBKR Paper Trading Connection
export IBKR_HOST="127.0.0.1"          # Localhost (TWS/Gateway running locally)
export IBKR_PORT="7497"               # Paper trading port (NOT 7496 for production)
export IBKR_CLIENT_ID="1"            # Unique client ID (increment if multiple clients)
```

### Environment Variable Template

Create a `.env.paper` file in your project root:

```bash
# IBKR Paper Trading Configuration
# Copy this file to .env.paper and fill in your values
# DO NOT commit this file to version control

IBKR_HOST=127.0.0.1
IBKR_PORT=7497
IBKR_CLIENT_ID=1
```

Load the environment before running validation:

```bash
source .env.paper
```

Or pass directly to the test:

```bash
IBKR_HOST=127.0.0.1 IBKR_PORT=7497 IBKR_CLIENT_ID=1 cargo test -p nanobook-broker --test validate_mock_vs_paper
```

## Connection Test Script

Before running the full validation, test your paper trading connection:

### Manual Connection Test

```bash
# Set environment variables
export IBKR_HOST="127.0.0.1"
export IBKR_PORT="7497"
export IBKR_CLIENT_ID="1"

# Run the connection test
cargo test -p nanobook-broker --test validate_mock_vs_paper -- test_paper_connection
```

### Expected Output

If successful, you should see:
```
Connecting to IB Gateway at 127.0.0.1:7497...
Connected (client_id=1)
Account: equity=$XXXX.XX, cash=$XXXX.XX, buying_power=$XXXX.XX
test_paper_connection ... ok
```

If unsuccessful, common errors:
- `Connection refused`: TWS/IB Gateway is not running or port is incorrect
- `Authentication failed`: API access not enabled or wrong credentials
- `Client ID already in use`: Another client is connected with the same ID

## Troubleshooting

### Issue: "Connection refused"

**Cause**: TWS/IB Gateway is not running or port is incorrect

**Solution**:
1. Verify TWS/IB Gateway is running
2. Check the configured port in TWS/IB Gateway settings
3. Ensure you're using the paper trading port (7497), not production (7496)

### Issue: "Authentication failed"

**Cause**: API access not enabled or IP restrictions

**Solution**:
1. Enable API access in Account Management
2. Add your IP to trusted IPs in API settings
3. Verify you're logging in with paper trading account

### Issue: "Client ID already in use"

**Cause**: Another client is connected with the same ID

**Solution**:
1. Increment `IBKR_CLIENT_ID` to use a different ID
2. Disconnect other clients using the same ID

### Issue: "Read-only API" error

**Cause**: API is configured as read-only

**Solution**:
1. In TWS/IB Gateway settings, uncheck "Read-Only API"
2. Restart TWS/IB Gateway

## Security Considerations

1. **Never commit credentials**: Do not commit `.env.paper` or any files with real credentials
2. **Use paper trading only**: Always use paper trading account for validation (port 7497)
3. **IP restrictions**: In production, restrict API access to specific IPs
4. **Client ID uniqueness**: Ensure unique client IDs for concurrent connections
5. **Firewall**: Consider running TWS/IB Gateway behind a firewall in production

## Next Steps

After successful connection test, proceed to:
1. Run the full validation script: `cargo test -p nanobook-broker --test validate_mock_vs_paper`
2. Review divergence reports in `broker/tests/failure_injection/divergence_log.md`
3. Fix any mock implementation issues identified by validation

## Validation Test Cases

This section documents the expected callback sequences for various scenarios that are validated against real IBKR paper trading.

### Normal Order Submission Callback Sequence

**Test Case**: `test_normal_order_submission`

**Expected Callback Sequence**:
1. `Connected` - Initial connection established
2. `OrderSubmitted` - Order accepted by TWS
   - Includes: order_id, sequence number, symbol, quantity
3. `OrderFill` - Order fully filled
   - Includes: order_id, sequence number, filled_quantity (equals order quantity)

**Mock Behavior**: The MockTws simulates this sequence with deterministic callbacks.

**Paper Behavior**: Real IBKR will emit similar callbacks via the PlaceOrder subscription.

**Validation Points**:
- Callback order must match exactly
- Sequence numbers must increment monotonically
- Order ID must be consistent across callbacks
- Fill quantity must equal order quantity for full fill

### Partial Fill Callback Sequence

**Test Case**: `test_partial_fill`

**Expected Callback Sequence**:
1. `Connected` - Initial connection established
2. `OrderSubmitted` - Order accepted by TWS
3. `OrderFill` - Partial fill received
   - Includes: order_id, sequence number, filled_quantity (less than order quantity)
4. (Optional) Additional `OrderFill` callbacks for remaining fills
5. `OrderFill` - Final fill (when fully filled)

**Mock Behavior**: MockTws allows specifying exact fill quantities to test partial fills.

**Paper Behavior**: Real IBKR may emit multiple partial fills depending on market conditions.

**Validation Points**:
- Partial fill quantity must be less than order quantity
- Multiple fills are allowed and should sum to order quantity
- Sequence numbers must increment with each fill
- Order ID must be consistent across all fill callbacks

### Order Cancellation Callback Sequence

**Test Case**: `test_order_cancellation`

**Expected Callback Sequence**:
1. `Connected` - Initial connection established
2. `OrderSubmitted` - Order accepted by TWS
3. `OrderCancelled` - Cancel request accepted and order cancelled
   - Includes: order_id, sequence number

**Mock Behavior**: MockTws immediately marks order as cancelled upon cancel request.

**Paper Behavior**: Real IBKR may have a delay between cancel request and cancellation confirmation.

**Validation Points**:
- Cancel callback must occur after order submission
- Order status must change to "Cancelled"
- No fill callbacks should occur after cancellation

### Error/Reject Callback Patterns

**Test Case**: Not yet implemented in validation script (future enhancement)

**Expected Callback Sequences**:

**Order Reject**:
1. `Connected`
2. `OrderSubmitted`
3. `OrderReject` - Order rejected by TWS
   - Includes: error code, error message, order_id

**Cancel Reject**:
1. `Connected`
2. `OrderSubmitted`
3. `OrderCancelled` - Cancel request
4. `CancelReject` - Cancel rejected (e.g., order already filled)
   - Includes: error code, error message, order_id

**Mock Behavior**: MockTws can simulate rejects via failure injection (F2, F4, F5).

**Paper Behavior**: Real IBKR rejects orders for various reasons (insufficient funds, market closed, etc.).

**Validation Points**:
- Reject callbacks must include error codes and messages
- Reject must prevent order execution
- Cancel reject must include reason for rejection

### Disconnect/Reconnect Callback Sequence

**Test Case**: `test_disconnect_reconnect` (F6: ReconnectDrill)

**Expected Callback Sequence**:
1. `Connected` - Initial connection
2. `Disconnected` - Connection lost (simulated or real)
3. `Connected` - Reconnection established
4. (Optional) `OrderSubmitted` - Order submitted after reconnection

**Mock Behavior**: MockTws injects disconnect at specified timing (PreSubmit, PostSubmit, etc.).

**Paper Behavior**: Real IBKR may disconnect due to network issues, TWS restart, etc.

**Validation Points**:
- Disconnect callback must be recorded
- Reconnection must succeed with new sequence numbers
- Orders submitted after reconnection must get new order IDs
- State should be properly restored after reconnection

### Market Data Subscription Callback Patterns

**Test Case**: Not yet implemented in validation script (future enhancement)

**Expected Callback Sequence**:
1. `Connected`
2. `MarketDataSubscribe` - Subscribe to market data for symbol
3. `TickPrice` - Price updates (bid, ask, last)
4. `TickSize` - Size updates (bid size, ask size)
5. `SnapshotEnd` - End of snapshot (for snapshot requests)

**Mock Behavior**: MockTws does not currently simulate market data (future enhancement).

**Paper Behavior**: Real IBKR emits continuous tick updates for subscribed symbols.

**Validation Points**:
- Tick callbacks must include valid prices and sizes
- Bid/ask prices must maintain reasonable spread
- Sequence numbers should increment with each tick
- Snapshot requests must terminate with SnapshotEnd

### Failure Mode Specific Callbacks

**F1: Duplicate Order-Status Callback Injection**

**Expected Behavior**:
- Normal order submission sequence
- Additional duplicate `OrderStatus` callback with same order_id
- Mock injects this via FailureMode::F1DuplicateStatus

**Validation Points**:
- Duplicate callback must be detected and handled
- Duplicate must not cause state corruption

**F2: Cancel Reject Race with Fill**

**Expected Behavior**:
- Order submitted
- Cancel requested
- Fill occurs before cancel processes
- Cancel reject callback received

**Validation Points**:
- Cancel reject must be handled gracefully
- Fill should be accepted despite cancel attempt

**F3: Partial Fill Followed by Disconnect**

**Expected Behavior**:
- Order submitted
- Partial fill received
- Disconnect occurs (simulated or real)
- Reconnection may be required

**Validation Points**:
- Partial fill state must be preserved across disconnect
- Reconnection should allow querying order status
- Remaining quantity should be accurate

**F4: Stale Market Data Detection**

**Expected Behavior**:
- Market data timestamp too old
- Order rejected or warning issued

**Validation Points**:
- Stale data must be detected before order submission
- Rejection must include stale data error code

**F5: Clock Skew Detection**

**Expected Behavior**:
- Server time mismatch detected
- Warning or error issued

**Validation Points**:
- Clock skew must be detected on connection
- Large skew should prevent order submission

**F6: TWS Reconnect Drill**

**Expected Behavior**:
- Same as disconnect/reconnect sequence above
- Systematic testing of reconnection logic

**Validation Points**:
- All disconnect/reconnect validation points apply

**F7: Cron Double-Fire Idempotency**

**Expected Behavior**:
- Same order submitted twice (duplicate client_order_id)
- Second submission should be rejected or idempotent

**Validation Points**:
- Duplicate submission must not create duplicate orders
- Client order ID must be checked for uniqueness

**F8: Kill Switch Subcommand**

**Expected Behavior**:
- Kill switch triggered
- All active orders cancelled
- No new orders accepted

**Validation Points**:
- Kill switch must cancel all orders immediately
- New orders must be rejected after kill switch

**F9: Process Crash + Warm Restart**

**Expected Behavior**:
- Process crashes during order execution
- Process restarts
- State recovered from persistent storage

**Validation Points**:
- Order state must be recoverable after crash
- In-flight orders must be queried on restart
- No duplicate orders should be created

## References

- [IBKR API Documentation](https://www.interactivebrokers.com/en/trading/workstation-api.php)
- [IBKR Paper Trading Guide](https://www.interactivebrokers.com/en/trading/paper-trading.php)
- [TWS Configuration Guide](https://www.interactivebrokers.com/en/trading/tws-config.php)
