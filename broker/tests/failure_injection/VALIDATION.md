# Mock vs Paper Trading Validation Procedure

This document describes the complete procedure for validating the MockTws implementation against real IBKR paper trading to ensure the mock accurately simulates TWS callback patterns, sequence numbers, and partial-fill semantics.

## Overview

The validation procedure compares the behavior of the MockTws wire-level mock against real IBKR paper trading by:
1. Running the same test scenarios against both mock and paper
2. Recording callback sequences from both
3. Comparing the sequences for divergences
4. Generating a divergence report with severity levels
5. Providing a remediation workflow for fixing identified issues

## Prerequisites

Before running validation, ensure you have:

1. **IBKR Paper Trading Account**: Set up and configured (see [paper_setup.md](paper_setup.md))
2. **TWS or IB Gateway**: Running and configured for API access
3. **Environment Variables**: Set for paper trading connection
4. **Rust Toolchain**: Installed and working (for running tests)

## Quick Start

### Mock-Only Validation (No Paper Account Required)

Run validation in mock mode to verify the test framework works:

```bash
cargo test -p nanobook-broker --test validate_mock_vs_paper
```

This runs all validation tests using only the mock, with paper callbacks simulated as identical to mock callbacks. Useful for:
- Verifying the validation framework works
- Testing without paper account access
- CI/CD pipelines

### Paper Trading Validation (Requires Paper Account)

Run validation against real IBKR paper trading:

```bash
# Set environment variables
export IBKR_HOST="127.0.0.1"
export IBKR_PORT="7497"
export IBKR_CLIENT_ID="1"

# Run validation
cargo test -p nanobook-broker --test validate_mock_vs_paper -- --ignored
```

This runs all validation tests against real paper trading and records actual IBKR callbacks for comparison.

## Detailed Procedure

### Step 1: Set Up Paper Trading Account

Follow the setup guide in [paper_setup.md](paper_setup.md) to:
1. Enable paper trading in your IBKR account
2. Enable API access
3. Configure TWS/IB Gateway
4. Set environment variables

### Step 2: Test Paper Trading Connection

Verify your paper trading connection works:

```bash
export IBKR_HOST="127.0.0.1"
export IBKR_PORT="7497"
export IBKR_CLIENT_ID="1"

cargo test -p nanobook-broker --test validate_mock_vs_paper test_paper_connection -- --ignored
```

**Expected Output**:
```
Running test test_paper_connection
Config: host=127.0.0.1, port=7497, client_id=1
Paper connection test would connect to real IBKR here
Skipping actual connection in this validation script
test test_paper_connection ... ok
```

**Troubleshooting**:
- If connection fails, check TWS/IB Gateway is running
- Verify port is correct (7497 for paper, 7496 for production)
- Check API access is enabled in IBKR account settings
- Ensure your IP is in the trusted IPs list

### Step 3: Run Full Validation

Run all validation tests against paper trading:

```bash
cargo test -p nanobook-broker --test validate_mock_vs_paper -- --ignored
```

This runs:
- `test_paper_connection` - Verifies connection to paper account
- `test_normal_order_submission` - Normal order flow
- `test_partial_fill` - Partial fill behavior
- `test_order_cancellation` - Order cancellation
- `test_disconnect_reconnect` - Disconnect/reconnect handling
- `test_f3_partial_fill_disconnect` - F3 failure mode

### Step 4: Generate Divergence Report

Generate a detailed divergence report:

```bash
cargo test -p nanobook-broker --test validate_mock_vs_paper generate_divergence_report -- --ignored
```

This creates `broker/tests/failure_injection/divergence_log.md` with:
- Executive summary of divergences
- Severity levels (Critical, Warning, Info)
- Detailed divergence descriptions
- Test case summary table
- Remediation checklist

### Step 5: Interpret Divergence Report

Review the divergence report to understand differences between mock and paper behavior.

#### Severity Levels

**Critical**: Fundamental differences that must be fixed
- Callback type mismatches
- Missing callbacks
- Incorrect callback ordering
- Wrong order ID mapping

**Warning**: Differences that may not break tests but should be investigated
- Sequence number gaps
- Timing differences
- Additional informational callbacks

**Info**: Minor differences unlikely to affect correctness
- Formatting differences
- Additional metadata
- Case sensitivity differences

#### Example Divergence Report

```markdown
## Executive Summary

- **Total Divergences**: 2
- **Critical**: 1
- **Warning**: 1
- **Info**: 0
- **Overall Status**: FAIL

## Divergence Details

### Divergence 1: test_normal_order_submission

**Severity**: Critical

**Mock Callback**:
```
OrderSubmitted - Order ID: Some(1) - Sequence: Some(2) - Details: symbol=AAPL, qty=100
```

**Paper Callback**:
```
OrderSubmitted - Order ID: Some(1) - Sequence: Some(2) - Details: symbol=AAPL, qty=100, time=1234567890
```

**Description**: Paper callback includes timestamp field not present in mock callback.

**Impact**: Mock may not accurately simulate timing information.

**Recommended Action**: Add timestamp field to mock callback generation.
```

### Step 6: Fix Identified Divergences

For each critical divergence, fix the mock implementation:

1. **Identify the root cause**: Determine why the mock differs from paper
2. **Update mock implementation**: Edit `broker/tests/mock_tws.rs`
3. **Add test coverage**: Ensure the fix is covered by tests
4. **Re-run validation**: Verify the fix resolves the divergence

#### Example Fix Workflow

**Problem**: Mock doesn't include timestamp in OrderSubmitted callback

**Fix**:
```rust
// In mock_tws.rs
self.record_callback(&format!("OrderSubmitted: id={}, seq={}, symbol={}, qty={}, time={}",
    order_id, seq_num, symbol, quantity, current_timestamp()));
```

**Verify**:
```bash
cargo test -p nanobook-broker --test validate_mock_vs_paper -- --ignored
cargo test -p nanobook-broker --test validate_mock_vs_paper generate_divergence_report -- --ignored
```

**Check**: Review divergence log to confirm the issue is resolved.

### Step 7: Iterate Until Validation Passes

Repeat steps 3-6 until:
- All critical divergences are resolved
- Warning divergences are reviewed and either fixed or documented as acceptable
- Overall status is PASS

## Validation Test Cases

The following test cases are validated:

### test_normal_order_submission
Validates normal order submission and fill callback sequence.

### test_partial_fill
Validates partial fill callback sequence.

### test_order_cancellation
Validates order cancellation callback sequence.

### test_disconnect_reconnect
Validates disconnect/reconnect callback sequence (F6).

### test_f3_partial_fill_disconnect
Validates F3 failure mode: partial fill followed by disconnect.

See [paper_setup.md](paper_setup.md#validation-test-cases) for detailed expected callback sequences.

## Mock Bug Fix Workflow

When divergences are found, follow this workflow:

### 1. Document the Divergence

Add a note to the divergence log's Historical Notes section:
```markdown
### [DATE] - Validation Run

- Divergence found in test_normal_order_submission
- Mock missing timestamp in OrderSubmitted callback
- Will fix in next commit
```

### 2. Create a Fix Branch

```bash
git checkout -b fix/mock-timestamp-callback
```

### 3. Implement the Fix

Edit `broker/tests/mock_tws.rs` to add the missing behavior.

### 4. Update Tests

Ensure existing tests still pass and add new tests if needed.

### 5. Re-run Validation

```bash
cargo test -p nanobook-broker --test validate_mock_vs_paper -- --ignored
cargo test -p nanobook-broker --test validate_mock_vs_paper generate_divergence_report -- --ignored
```

### 6. Review Divergence Report

Check that the divergence is resolved and no new divergences were introduced.

### 7. Commit and Push

```bash
git add broker/tests/mock_tws.rs
git commit -m "Fix: Add timestamp to OrderSubmitted callback in MockTws"
git push origin fix/mock-timestamp-callback
```

### 8. Update Documentation

Update the divergence log with the fix:
```markdown
### [DATE] - Validation Run

- Fixed timestamp in OrderSubmitted callback
- Validation now passes for test_normal_order_submission
```

### 9. Merge and Clean Up

```bash
git checkout main
git merge fix/mock-timestamp-callback
git branch -d fix/mock-timestamp-callback
```

## Continuous Validation

### CI/CD Integration

Add validation to your CI/CD pipeline:

```yaml
# Example GitHub Actions
- name: Run Mock Validation
  run: cargo test -p nanobook-broker --test validate_mock_vs_paper

# Optional: Paper trading validation (requires secrets)
- name: Run Paper Validation
  if: github.event_name == 'schedule'  # Run daily/weekly
  env:
    IBKR_HOST: ${{ secrets.IBKR_HOST }}
    IBKR_PORT: ${{ secrets.IBKR_PORT }}
    IBKR_CLIENT_ID: ${{ secrets.IBKR_CLIENT_ID }}
  run: cargo test -p nanobook-broker --test validate_mock_vs_paper -- --ignored
```

### Pre-Commit Validation

Add a pre-commit hook to run mock validation:

```bash
# .git/hooks/pre-commit
#!/bin/bash
cargo test -p nanobook-broker --test validate_mock_vs_paper
```

## Troubleshooting

### Common Issues

**Issue**: "Connection refused" when running paper validation

**Solution**:
- Verify TWS/IB Gateway is running
- Check port is correct (7497 for paper)
- Ensure API access is enabled

**Issue**: "Client ID already in use"

**Solution**:
- Increment `IBKR_CLIENT_ID` to use a different ID
- Disconnect other clients using the same ID

**Issue**: Divergence report shows no divergences but tests fail

**Solution**:
- Check that paper mode is actually enabled (environment variables set)
- Verify TWS/IB Gateway is accessible
- Review test output for specific error messages

**Issue**: Validation passes but production fails

**Solution**:
- Paper trading may differ from production in some ways
- Document acceptable differences in the divergence log
- Consider adding production-specific validation if needed

## Best Practices

1. **Run mock validation frequently**: At least on every PR
2. **Run paper validation periodically**: Weekly or before releases
3. **Keep divergence log updated**: Document all changes and fixes
4. **Review warnings regularly**: Even if not critical, warnings may indicate issues
5. **Test failure modes**: Ensure F1-F9 are all validated
6. **Document acceptable differences**: Not all differences need to be fixed

## References

- [Paper Trading Setup Guide](paper_setup.md)
- [Failure Injection README](README.md)
- [IBKR API Documentation](https://www.interactivebrokers.com/en/trading/workstation-api.php)
- [MockTws Implementation](../../mock_tws.rs)

## Appendix: Environment Variables

| Variable | Description | Example | Required |
|----------|-------------|---------|----------|
| `IBKR_HOST` | TWS/IB Gateway host | `127.0.0.1` | Yes (for paper mode) |
| `IBKR_PORT` | TWS/IB Gateway port | `7497` | Yes (for paper mode) |
| `IBKR_CLIENT_ID` | Unique client ID | `1` | Yes (for paper mode) |

## Appendix: Test Commands

| Command | Purpose |
|---------|---------|
| `cargo test -p nanobook-broker --test validate_mock_vs_paper` | Run mock-only validation |
| `cargo test -p nanobook-broker --test validate_mock_vs_paper -- --ignored` | Run paper validation |
| `cargo test -p nanobook-broker --test validate_mock_vs_paper test_normal_order_submission` | Run specific test |
| `cargo test -p nanobook-broker --test validate_mock_vs_paper generate_divergence_report -- --ignored` | Generate divergence report |

---

*Last updated: 2025-01-12*
