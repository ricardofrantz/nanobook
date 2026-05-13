#!/usr/bin/env bash
#
# Cron-friendly runner script for paper trading IBKR dry-run
#
# Usage: ./runner.sh <config-file>
#
# This script:
# - Uses rebalancer --cron-mode for idempotent execution
# - Logs output to dated log files
# - Handles errors gracefully
#

set -euo pipefail

# Script directory
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# Configuration
CONFIG_FILE="${1:-risk-config.toml}"
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

# Log header
echo "========================================" | tee -a "$LOG_FILE"
echo "[$(date)] Starting rebalancer run" | tee -a "$LOG_FILE"
echo "[$(date)] Config: $CONFIG_FILE" | tee -a "$LOG_FILE"
echo "[$(date)] Log: $LOG_FILE" | tee -a "$LOG_FILE"
echo "========================================" | tee -a "$LOG_FILE"

# Run rebalancer with cron-mode
# Note: Adjust the rebalancer path as needed for your installation
if rebalancer --cron-mode \
    --config "$CONFIG_FILE" \
    --audit-log "$AUDIT_DIR/audit.jsonl" \
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
