#!/usr/bin/env bash
# Validate local paper-soak inputs before connecting to IBKR.
# This does not contact TWS/Gateway and does not submit orders.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CONFIG_FILE="${1:-$SCRIPT_DIR/risk-config.toml}"
TARGET_FILE="${2:-$SCRIPT_DIR/target.json.example}"

python3 - "$CONFIG_FILE" "$TARGET_FILE" <<'PY'
import json
import sys
import tomllib
from pathlib import Path

config_path = Path(sys.argv[1])
target_path = Path(sys.argv[2])
errors: list[str] = []
warnings: list[str] = []

if not config_path.exists():
    errors.append(f"missing config file: {config_path}")
else:
    config = tomllib.loads(config_path.read_text())
    for section in ["connection", "account", "execution", "risk", "cost", "logging"]:
        if section not in config:
            errors.append(f"config missing [{section}] section")
    account_id = str(config.get("account", {}).get("id", ""))
    if account_id and not account_id.startswith("DU"):
        warnings.append("account.id does not look like an IBKR paper account (expected DU...)")
    port = int(config.get("connection", {}).get("port", 0) or 0)
    if port not in {4002, 7497}:
        warnings.append(f"connection.port={port} is not a standard IBKR paper port (4002 Gateway, 7497 TWS)")
    log_dir = config.get("logging", {}).get("dir")
    if log_dir != "./audit":
        warnings.append(f"logging.dir is {log_dir!r}; README/report commands assume './audit'")

if not target_path.exists():
    errors.append(f"missing target file: {target_path}")
else:
    target = json.loads(target_path.read_text())
    rows = target.get("targets", [])
    if not rows:
        errors.append("target has no targets[] entries")
    long_sum = 0.0
    for i, row in enumerate(rows):
        symbol = str(row.get("symbol", ""))
        weight = float(row.get("weight", 0.0))
        if not symbol:
            errors.append(f"target row {i} missing symbol")
        if len(symbol.encode()) > 8:
            errors.append(f"target symbol {symbol!r} exceeds 8-byte IBKR/nanobook limit")
        if weight == 0.0 or abs(weight) > 1.0:
            errors.append(f"target {symbol!r} has invalid weight {weight}")
        if weight > 0:
            long_sum += weight
    if long_sum > 1.0 + 1e-9:
        errors.append(f"sum of positive weights is {long_sum:.6f}, must be <= 1.0")
    if not target.get("metadata", {}).get("id"):
        warnings.append("target metadata.id missing; cron idempotency scope will fall back to timestamp")

if errors:
    print("PRE-FLIGHT FAILED")
    for error in errors:
        print(f"  ERROR: {error}")
    for warning in warnings:
        print(f"  WARN:  {warning}")
    raise SystemExit(1)

print("PRE-FLIGHT OK")
for warning in warnings:
    print(f"  WARN: {warning}")
print(f"  config: {config_path}")
print(f"  target: {target_path}")
PY
