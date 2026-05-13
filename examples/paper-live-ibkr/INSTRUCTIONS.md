# IBKR Paper Trading Setup - Deferred

**Status:** Infrastructure ready, execution deferred

## What Was Completed

All infrastructure for the v0.15 paper trading soak has been set up:

1. ✅ Directory structure created (`examples/paper-live-ibkr/`)
2. ✅ `runner.sh` script for cron-friendly execution
3. ✅ `risk-config.toml` template with correct rebalancer config structure
4. ✅ `target.json.example` with S&P 100 sample portfolio
5. ✅ Comprehensive 9-step setup guide in README.md
6. ✅ `audit/` directory for logs
7. ✅ `scripts/sanitize-audit.py` verified to exist
8. ✅ rebalancer binary built (`target/release/rebalancer`)

## What To Do When Ready

Follow the 9-step guide in `README.md`:

1. Install and configure IBKR Gateway or TWS
2. Get your IBKR paper account ID
3. Create your config file (`cp risk-config.toml my-config.toml`)
4. Create your target portfolio (`cp target.json.example my-target.json`)
5. Build rebalancer (`cargo build --release -p nanobook-rebalancer`)
6. Test IBKR connection
7. Run dry-run test
8. Run first rebalance
9. Set up cron automation

## Why This Was Deferred

IBKR Gateway/TWS requires manual download and installation. It's a GUI application that cannot be automated via CLI.

## Key Notes

- **No API keys needed** - IBKR authenticates via socket connection (host, port, client_id)
- **Paper trading only** - no real money at risk
- **Purpose** - validate v0.15's production readiness with S&P 100 monthly strategy
- **Duration** - 2-4 calendar weeks

## Next Steps When Returning

1. Download IBKR Gateway from: https://www.interactivebrokers.com/en/trading/ibgateway-stable.php
2. Follow the detailed setup in `README.md`
3. Run connection test: `../../target/release/rebalancer status --config my-config.toml`
4. Start with dry-run: `../../target/release/rebalancer run --dry-run --config my-config.toml my-target.json`

## Files To Customize

- `my-config.toml` - your IBKR settings (account ID, port, etc.)
- `my-target.json` - your target portfolio (symbols, weights)

## Files To Use As-Is

- `runner.sh` - cron-friendly execution script
- `risk-config.toml` - template (copy to my-config.toml)
- `target.json.example` - template (copy to my-target.json)

## Documentation

Full setup guide: `README.md`
Sanitization script: `scripts/sanitize-audit.py`
