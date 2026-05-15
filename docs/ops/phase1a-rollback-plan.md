# Phase 1.6A Rollback Plan: Write-Ahead Logging for Orders

## Overview

This document describes the rollback plan for Phase 1.6A, which introduces write-ahead logging for order submission only. The feature is gated by the `write_ahead_logging` feature flag, which can be disabled to revert to the previous behavior.

## Rollback Criteria

Rollback should be triggered if any of the following conditions are met:

### 1. Duplicate Orders Detected
- **Condition**: Duplicate orders are submitted to the broker for the same symbol, action, and quantity within a short time window.
- **Threshold**: 2 or more duplicate orders within 5 minutes for the same client order ID.
- **Detection**: Monitor audit logs for duplicate `order_intent` events with the same `client_order_id`.
- **Severity**: Critical - immediate rollback required.

### 2. Performance Degradation
- **Condition**: Order submission latency increases significantly compared to baseline.
- **Threshold**: Order submission latency > 10 seconds (baseline: < 2 seconds) for 5 consecutive orders.
- **Detection**: Monitor order submission latency metrics.
- **Severity**: High - rollback within 1 hour.

### 3. Audit Log Errors
- **Condition**: Audit log write failures or corruption.
- **Threshold**: 3 or more audit log write failures within 10 minutes.
- **Detection**: Monitor application logs for audit log write errors.
- **Severity**: High - rollback within 1 hour.

### 4. Recovery Failures
- **Condition**: Recovery from crash fails or produces incorrect state.
- **Threshold**: Recovery action is incorrect (e.g., Resume when ManualReview is required) for 2 or more crash scenarios.
- **Detection**: Monitor recovery action accuracy in test runs.
- **Severity**: Critical - immediate rollback required.

### 5. Broker Reconciliation Failures
- **Condition**: Broker reconciliation fails to match orders or produces incorrect results.
- **Threshold**: 3 or more reconciliation failures within 1 hour.
- **Detection**: Monitor reconciliation success rate.
- **Severity**: High - rollback within 1 hour.

### 6. Data Inconsistency
- **Condition**: Audit log state does not match broker state after recovery.
- **Threshold**: 2 or more state mismatches detected by discrepancy reports.
- **Detection**: Monitor discrepancy reports from `rebalancer recover`.
- **Severity**: Critical - immediate rollback required.

## Rollback Steps

### Step 1: Disable Feature Flag
1. Edit the build configuration to remove the `write_ahead_logging` feature flag.
2. Rebuild the application without the feature:
   ```bash
   cargo build --release
   ```
3. Verify the build succeeds.

### Step 2: Deploy New Binary
1. Stop the running rebalancer process:
   ```bash
   pkill rebalancer
   ```
2. Replace the binary with the newly built version.
3. Start the rebalancer process:
   ```bash
   rebalancer run target.json
   ```
4. Verify the process starts successfully.

### Step 3: Verify Rollback
1. Run a dry-run to verify the feature is disabled:
   ```bash
   rebalancer run target.json --dry-run
   ```
2. Check the audit log to confirm no `order_intent` events are logged.
3. Verify order submission works without write-ahead logging.

### Step 4: Monitor for Issues
1. Monitor application logs for any errors.
2. Verify order submission latency returns to baseline.
3. Check that no duplicate orders are submitted.
4. Confirm audit logs are still being written correctly.

## Rollback Timeline

| Phase | Duration | Actions |
|-------|----------|---------|
| Detection | Immediate | Identify rollback trigger condition |
| Decision | < 5 minutes | Confirm rollback is necessary |
| Disable Feature Flag | < 2 minutes | Remove feature flag from build config |
| Rebuild | < 5 minutes | Build binary without feature flag |
| Deploy | < 5 minutes | Stop process, replace binary, start process |
| Verification | < 10 minutes | Run dry-run, check audit logs, verify functionality |
| Total Time | < 30 minutes | Complete rollback |

## Rollback Verification Checklist

- [ ] Feature flag is disabled in build configuration
- [ ] Binary rebuilt successfully without feature flag
- [ ] Process stopped and replaced with new binary
- [ ] Process started successfully
- [ ] Dry-run completed without `order_intent` events
- [ ] Order submission works without write-ahead logging
- [ ] Application logs show no errors
- [ ] Order submission latency returns to baseline
- [ ] No duplicate orders detected
- [ ] Audit logs are still being written correctly

## Post-Rollback Actions

### 1. Incident Analysis
- Document the reason for rollback.
- Collect logs and metrics for analysis.
- Identify the root cause of the issue.

### 2. Fix Implementation
- Fix the identified issue in the code.
- Add tests to prevent regression.
- Verify the fix in a test environment.

### 3. Re-Rollout Planning
- Schedule a re-rollout after the fix is verified.
- Consider additional monitoring or safeguards.
- Update the rollout plan based on lessons learned.

### 4. Communication
- Notify stakeholders of the rollback.
- Share incident analysis with the team.
- Document lessons learned.

## Rollback Testing

Before rolling back in production, test the rollback procedure in a test environment:

1. **Test Feature Flag Disable**
   - Build without the feature flag.
   - Verify the binary runs correctly.
   - Confirm no `order_intent` events are logged.

2. **Test Order Submission**
   - Submit orders without write-ahead logging.
   - Verify orders are submitted successfully.
   - Confirm audit logs are still written.

3. **Test Recovery**
   - Simulate a crash scenario.
   - Run recovery without write-ahead logging.
   - Verify recovery produces the correct action.

4. **Test Performance**
   - Measure order submission latency without the feature.
   - Confirm latency returns to baseline.
   - Verify no performance degradation.

## Rollback Decision Matrix

| Condition | Severity | Rollback Timeline | Approval Required |
|-----------|----------|-------------------|-------------------|
| Duplicate orders | Critical | Immediate (< 5 min) | No (automatic) |
| Recovery failures | Critical | Immediate (< 5 min) | No (automatic) |
| Data inconsistency | Critical | Immediate (< 5 min) | No (automatic) |
| Performance degradation | High | < 1 hour | Yes (operator) |
| Audit log errors | High | < 1 hour | Yes (operator) |
| Broker reconciliation failures | High | < 1 hour | Yes (operator) |

## Rollback Communication

### Internal Communication
- Notify engineering team of rollback.
- Share incident details and timeline.
- Schedule post-mortem meeting.

### External Communication
- If rollback affects trading, notify trading desk.
- If rollback affects customers, notify account manager.
- Document rollback in incident log.

## Rollback Success Criteria

Rollback is considered successful if:
1. Feature flag is disabled and binary is rebuilt.
2. Application runs without errors.
3. Order submission works without write-ahead logging.
4. Order submission latency returns to baseline.
5. No duplicate orders are detected.
6. Audit logs are still being written correctly.
7. Recovery works without write-ahead logging.
8. No data inconsistency is detected.

## Rollback Failure Handling

If rollback fails:
1. Escalate to senior engineering team.
2. Consider manual intervention (e.g., cancel open orders).
3. Document the failure and next steps.
4. Communicate with stakeholders about the situation.
