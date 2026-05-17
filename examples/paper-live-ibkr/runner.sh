#!/usr/bin/env bash
#
# Cron-friendly runner script for paper trading IBKR dry-run
#
# Usage: ./runner.sh <config-file> <target-file>
#
# This script:
# - Uses rebalancer --cron-mode for idempotent execution
# - Logs output to dated log files
# - Handles errors gracefully
#

set -euo pipefail

# Script directory
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Workspace root (for finding rebalancer binary)
WORKSPACE_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
cd "$SCRIPT_DIR"

# Configuration
CONFIG_FILE="${1:-risk-config.toml}"
TARGET_FILE="${2:-target.json}"
LOG_DIR="$SCRIPT_DIR/logs"
AUDIT_DIR="$SCRIPT_DIR/audit"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
LOG_FILE="$LOG_DIR/rebalancer_${TIMESTAMP}.log"

# Create directories if they don't exist
mkdir -p "$LOG_DIR"
mkdir -p "$AUDIT_DIR"

# Check if config file exists
if [ ! -f "$CONFIG_FILE" ]; then
    echo "[$(date)] ERROR: Config file not found: $CONFIG_FILE" | tee -a "$LOG_FILE"
    exit 1
fi

# Check if target file exists
if [ ! -f "$TARGET_FILE" ]; then
    echo "[$(date)] ERROR: Target file not found: $TARGET_FILE" | tee -a "$LOG_FILE"
    exit 1
fi

# Validate local inputs before contacting IBKR or submitting orders.
if ! "$SCRIPT_DIR/preflight.sh" "$CONFIG_FILE" "$TARGET_FILE" 2>&1 | tee -a "$LOG_FILE"; then
    echo "[$(date)] ERROR: Preflight failed; refusing to run rebalancer" | tee -a "$LOG_FILE"
    exit 1
fi

# Log header
echo "========================================" | tee -a "$LOG_FILE"
echo "[$(date)] Starting rebalancer run" | tee -a "$LOG_FILE"
echo "[$(date)] Config: $CONFIG_FILE" | tee -a "$LOG_FILE"
echo "[$(date)] Target: $TARGET_FILE" | tee -a "$LOG_FILE"
echo "[$(date)] Log: $LOG_FILE" | tee -a "$LOG_FILE"
echo "========================================" | tee -a "$LOG_FILE"

# Run rebalancer with cron-mode
REBALANCER_BIN="$WORKSPACE_ROOT/target/release/rebalancer"
if [ ! -f "$REBALANCER_BIN" ]; then
    echo "[$(date)] ERROR: Rebalancer binary not found: $REBALANCER_BIN" | tee -a "$LOG_FILE"
    echo "[$(date)] Run: cargo build --release -p nanobook-rebalancer" | tee -a "$LOG_FILE"
    exit 1
fi

if "$REBALANCER_BIN" --config "$CONFIG_FILE" run "$TARGET_FILE" --cron-mode \
    2>&1 | tee -a "$LOG_FILE"; then
    EXIT_CODE=0
    STATUS="SUCCESS"
else
    EXIT_CODE=$?
    STATUS="FAILED (exit code: $EXIT_CODE)"
fi

# Log footer
echo "========================================" | tee -a "$LOG_FILE"
echo "[$(date)] Rebalancer run completed: $STATUS" | tee -a "$LOG_FILE"
echo "========================================" | tee -a "$LOG_FILE"

# Exit with rebalancer's exit code
exit $EXIT_CODE
