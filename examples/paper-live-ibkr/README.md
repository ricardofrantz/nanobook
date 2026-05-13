# Paper Trading IBKR Dry-Run

**Purpose:** Pre-soak rehearsal for v0.14 failure-injection hardening validation.

## Overview

This is a 1-week IBKR paper trading dry-run to validate v0.13's hardened plumbing with `--cron-mode`. The goal is to test the rebalancer's resilience to failure modes in a controlled paper trading environment before the full soak test.

## Step-by-Step Setup

### Step 1: Install and Configure IBKR TWS or Gateway

**Option A: IBKR Gateway (Recommended for automated trading)**
1. Download IBKR Gateway from IBKR website: https://www.interactivebrokers.com/en/trading/ibgateway-stable.php
2. Install Gateway on your machine
3. Configure Gateway for paper trading:
   - Log in with your paper trading account
   - Enable API connections in Gateway configuration
   - Set socket port to 4002 (paper) or 4001 (live)
   - Set "Read-Only API" to "No" to allow order submission
   - Set "Allow connections from localhost" to "Yes"

**Option B: IBKR TWS (Trader Workstation)**
1. Download TWS from IBKR website: https://www.interactivebrokers.com/en/trading/tws-stable.php
2. Install TWS on your machine
3. Configure TWS for paper trading:
   - Log in with your paper trading account
   - Go to File → Global Configuration → API → Settings
   - Enable "ActiveX and Socket Clients"
   - Set socket port to 7497 (paper) or 7496 (live)
   - Uncheck "Read-Only API"
   - Set "Allow connections from localhost" to "Yes"

### Step 2: Get Your IBKR Account ID

1. Log in to IBKR Account Management: https://www.interactivebrokers.com/sso
2. Navigate to Settings → Account Settings
3. Find your Account ID:
   - Paper trading accounts start with "DU" (e.g., DU123456)
   - Live accounts start with "U" (e.g., U1234567)
4. Copy this ID for the configuration file

### Step 3: Configure nanobook

1. **Navigate to the paper-live-ibkr directory:**
   ```bash
   cd examples/paper-live-ibkr
   ```

2. **Create your config file from the template:**
   ```bash
   cp risk-config.toml my-config.toml
   ```

3. **Edit `my-config.toml` with your IBKR settings:**
   ```toml
   [connection]
   host = "127.0.0.1"        # localhost (Gateway/TWS on same machine)
   port = 4002                # Gateway paper: 4002, TWS paper: 7497
   client_id = 100           # Unique ID (use different ID per connection)
   timeout_secs = 30

   [account]
   id = "DU123456"           # YOUR paper account ID from Step 2
   type = "margin"           # "margin" or "cash"

   [execution]
   order_interval_ms = 100
   limit_offset_bps = 5
   order_timeout_secs = 300
   max_orders_per_run = 50
   quote_staleness_threshold_sec = 30

   [risk]
   max_position_pct = 0.25   # Adjust based on your paper account size
   max_leverage = 1.0       # 1.0 = no leverage (conservative for paper)
   min_trade_usd = 100.0    # Skip tiny trades
   max_trade_usd = 10000.0  # Adjust based on your paper account size
   allow_short = true
   max_short_pct = 0.20

   [cost]
   commission_per_share = 0.0035
   commission_min = 0.35
   slippage_bps = 5

   [logging]
   dir = "./audit"
   audit_file = "audit.jsonl"
   clock_skew_threshold_sec = 30
   max_jump_rate_sec_per_sec = 2.0
   ```

### Step 4: Create Your Target Portfolio

1. **Copy the example target file:**
   ```bash
   cp target.json.example my-target.json
   ```

2. **Edit `my-target.json` with your desired symbols and weights:**
   ```json
   {
     "timestamp": "2026-05-13T15:30:00Z",
     "metadata": {
       "id": "paper-soak-rehearsal-2026-05-13"
     },
     "targets": [
       { "symbol": "AAPL", "weight": 0.20 },
       { "symbol": "MSFT", "weight": 0.20 },
       { "symbol": "GOOGL", "weight": 0.20 },
       { "symbol": "AMZN", "weight": 0.20 },
       { "symbol": "TSLA", "weight": 0.20 }
     ],
     "constraints": {
       "max_position_pct": 0.25,
       "max_leverage": 1.0,
       "min_trade_usd": 100.0
     }
   }
   ```

   **Important constraints:**
   - Symbol names must be ≤ 8 characters (IBKR limit)
   - Weights must be in (-1.0, 1.0) range
   - Sum of positive weights must be ≤ 1.0
   - Zero weights are not allowed (omit the symbol instead)

### Step 5: Build the Rebalancer

1. **From the nanobook workspace root:**
   ```bash
   cargo build --release -p nanobook-rebalancer
   ```

2. **Verify the binary exists:**
   ```bash
   ls -la target/release/rebalancer
   ```

### Step 6: Test IBKR Connection

1. **Start IBKR Gateway or TWS and log in to your paper account**

2. **Test connection with the rebalancer:**
   ```bash
   cd examples/paper-live-ibkr
   ../../target/release/rebalancer status --config my-config.toml
   ```

3. **Expected output:**
   - If successful: "Connected to IB Gateway at 127.0.0.1:4002 (client_id=100)"
   - If failed: Check that Gateway/TWS is running and API is enabled

### Step 7: Run a Dry-Run Test

1. **Test with --dry-run flag (no orders executed):**
   ```bash
   ../../target/release/rebalancer run --dry-run --config my-config.toml my-target.json
   ```

2. **Review the output:**
   - Check that the target portfolio is parsed correctly
   - Verify risk limits are respected
   - Confirm no errors in the plan

### Step 8: Run First Rebalance (Live Execution)

1. **Run with actual execution (paper trading only):**
   ```bash
   ../../target/release/rebalancer run --config my-config.toml my-target.json
   ```

2. **Confirm the orders:**
   - Rebalancer will show the planned orders
   - Type "yes" to confirm execution
   - Orders will be submitted to IBKR paper account

3. **Verify in IBKR Gateway/TWS:**
   - Check that orders appear in your paper account
   - Confirm fills are reflected

### Step 9: Set Up Cron Mode (Automated Execution)

1. **Use the runner script for cron-friendly execution:**
   ```bash
   ./runner.sh my-config.toml my-target.json
   ```

2. **The runner script:**
   - Uses `--cron-mode` for idempotency
   - Logs output to dated log files in `logs/`
   - Creates audit logs in `audit/`
   - Handles errors gracefully

3. **Set up cron job (optional):**
   ```bash
   # Edit crontab
   crontab -e

   # Add entry (run every 30 minutes during market hours)
   */30 09:30-16:00 * * 1-5 /path/to/nanobook/examples/paper-live-ibkr/runner.sh /path/to/nanobook/examples/paper-live-ibkr/my-config.toml /path/to/nanobook/examples/paper-live-ibkr/my-target.json
   ```

## Run Details

**Status:** Setup phase (awaiting execution)

**Execution Window:** TBD (to be filled during execution)

**Universe:** TBD (to be filled during execution)

**Risk Limits:** TBD (to be filled during execution)

## Audit Logs

Audit logs are stored in `audit/` directory. These logs contain:
- Run metadata (timestamps, sequence numbers)
- Order submission and fill events
- Risk limit checks
- Failure injection events (if any)

**Note:** Audit logs are NOT committed to git (see `.gitignore`). Use `scripts/sanitize-audit.py` to scrub PII before publishing.

## Post-Run Analysis

After the dry-run completes:
1. Review audit logs for anomalies
2. Verify cron-mode idempotency (no duplicate orders)
3. Check failure injection handling
4. Sanitize audit logs before sharing: `python3 scripts/sanitize-audit.py audit/audit.jsonl`

## Troubleshooting

**Connection refused:**
- Verify Gateway/TWS is running
- Check port number (4002 for Gateway paper, 7497 for TWS paper)
- Ensure API is enabled in Gateway/TWS settings

**Authentication failed:**
- Verify you're logged into the correct paper account
- Check that "Read-Only API" is disabled in Gateway/TWS

**Order rejected:**
- Verify account has sufficient buying power
- Check that symbols are valid IBKR tickers
- Ensure risk limits are not violated

**Clock skew warnings:**
- Check system clock accuracy
- Adjust `clock_skew_threshold_sec` in config if needed

## Notes

- This is paper trading only - no real money at risk
- The dry-run validates plumbing robustness, not strategy performance
- All trading is on IBKR's paper trading environment
- Use conservative risk limits for the rehearsal
- Monitor the first few runs closely before automating with cron
