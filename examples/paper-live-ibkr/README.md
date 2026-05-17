# Paper Trading IBKR Soak

**Purpose:** v0.15 paper trading soak for S&P 100 monthly strategy.

## Overview

This is a 2-4 calendar week IBKR paper trading soak to validate v0.15's production readiness with `--cron-mode`. The goal is to test the rebalancer's resilience in a live paper trading environment using the S&P 100 monthly universe.

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
       "id": "paper-soak-v0.15-2026-05-13"
     },
     "targets": [
       { "symbol": "AAPL", "weight": 0.10 },
       { "symbol": "MSFT", "weight": 0.10 },
       { "symbol": "GOOGL", "weight": 0.10 },
       { "symbol": "AMZN", "weight": 0.10 },
       { "symbol": "NVDA", "weight": 0.10 },
       { "symbol": "META", "weight": 0.10 },
       { "symbol": "TSLA", "weight": 0.10 },
       { "symbol": "BRK.B", "weight": 0.10 },
       { "symbol": "JPM", "weight": 0.10 },
       { "symbol": "V", "weight": 0.10 }
     ],
     "constraints": {
       "max_position_pct": 0.25,
       "max_leverage": 1.0,
       "min_trade_usd": 100.0
     }
   }
   ```

   **Note:** The example above uses a representative subset of S&P 100 (top 10 by market cap). The full S&P 100 universe will be generated at runtime from the momentum strategy.

   **Important constraints:**
   - Symbol names must be ≤ 8 characters (IBKR limit)
   - Weights must be in (-1.0, 1.0) range
   - Sum of positive weights must be ≤ 1.0
   - Zero weights are not allowed (omit the symbol instead)

### Step 5: Run Local Preflight

Before connecting to IBKR, validate the config and target files without touching the broker:

```bash
./preflight.sh risk-config.toml target.json.example
```

For real rehearsal/soak runs, pass your copied files:

```bash
./preflight.sh my-config.toml my-target.json
```

Preflight checks TOML/JSON syntax, required sections, paper-account-looking account IDs, paper ports, target symbol length, target weight bounds, and cron idempotency metadata.

### Step 6: Build the Rebalancer

1. **From the nanobook workspace root:**
   ```bash
   cargo build --release -p nanobook-rebalancer
   ```

2. **Verify the binary exists:**
   ```bash
   ls -la target/release/rebalancer
   ```

### Step 7: Test IBKR Connection

1. **Start IBKR Gateway or TWS and log in to your paper account**

2. **Test connection with the rebalancer:**
   ```bash
   cd examples/paper-live-ibkr
   ../../target/release/rebalancer --config my-config.toml status
   ```

3. **Expected output:**
   - If successful: "Connected to IB Gateway at 127.0.0.1:4002 (client_id=100)"
   - If failed: Check that Gateway/TWS is running and API is enabled

### Step 8: Run a Test Rebalance

1. **Test with --dry-run flag (no orders executed):**
   ```bash
   ../../target/release/rebalancer --config my-config.toml run my-target.json --dry-run
   ```

2. **Review the output:**
   - Check that the target portfolio is parsed correctly
   - Verify risk limits are respected
   - Confirm no errors in the plan

### Step 9: Run First Rebalance (Live Execution)

1. **Run with actual execution (paper trading only):**
   ```bash
   ../../target/release/rebalancer --config my-config.toml run my-target.json
   ```

2. **Confirm the orders:**
   - Rebalancer will show the planned orders
   - Type "yes" to confirm execution
   - Orders will be submitted to IBKR paper account

3. **Verify in IBKR Gateway/TWS:**
   - Check that orders appear in your paper account
   - Confirm fills are reflected

### Step 10: Set Up Cron Mode (Automated Execution)

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

   # Add entry (run monthly on the 1st at 09:30 ET)
   30 9 1 * * /path/to/nanobook/examples/paper-live-ibkr/runner.sh /path/to/nanobook/examples/paper-live-ibkr/my-config.toml /path/to/nanobook/examples/paper-live-ibkr/my-target.json
   ```

## Run Details

**Status:** Ready for v0.15 soak execution

Track actual rehearsal/soak evidence in `SOAK_STATUS.md`. Keep every completed checklist item tied to a log, sanitized audit excerpt, generated report, note, or commit.

**Execution Window:** 2-4 calendar weeks

**Universe:** S&P 100 monthly

**Risk Limits:** See risk-config.toml (max 25% position, 1.0 leverage, $100-$10,000 trade sizes)

## Audit Logs

Audit logs are stored in `audit/` directory. These logs contain:
- Run metadata (timestamps, sequence numbers)
- Order submission and fill events
- Risk limit checks
- Failure injection events (if any)

**Note:** Audit logs are NOT committed to git (see `.gitignore`). Use `../../scripts/sanitize-audit.py` to scrub PII before publishing.

## Post-Run Analysis

After the soak completes:
1. Review audit logs for anomalies
2. Verify cron-mode idempotency (no duplicate orders)
3. Generate HTML report: `python3 report.py audit/audit.jsonl report.html`
4. Sanitize audit logs before sharing: `python3 ../../scripts/sanitize-audit.py audit/audit.jsonl > sanitized-audit.jsonl`

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
- The soak validates production readiness, not strategy performance
- All trading is on IBKR's paper trading environment
- Use conservative risk limits for the soak
- Monitor the first few runs closely before automating with cron
