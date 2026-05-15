# Phase 1.6A Monitoring Requirements: Write-Ahead Logging for Orders

## Overview

This document describes the monitoring requirements for Phase 1.6A, which introduces write-ahead logging for order submission only. Monitoring is critical to ensure the feature works correctly and to detect issues early.

## Metrics to Monitor

### 1. Order Submission Latency
- **Metric**: Time from order intent to order submission completion
- **Measurement**: Duration in milliseconds
- **Baseline**: < 2000ms (without write-ahead logging)
- **Expected**: < 2500ms (with write-ahead logging, allowing for audit log write overhead)
- **Warning Threshold**: > 5000ms
- **Critical Threshold**: > 10000ms
- **Alert**: Alert if 5 consecutive orders exceed warning threshold

### 2. Audit Log Write Success Rate
- **Metric**: Percentage of successful audit log writes
- **Measurement**: Success rate as percentage
- **Baseline**: 100%
- **Expected**: 100%
- **Warning Threshold**: < 99%
- **Critical Threshold**: < 95%
- **Alert**: Alert if success rate falls below warning threshold

### 3. Audit Log Write Latency
- **Metric**: Time to write audit log entries
- **Measurement**: Duration in milliseconds
- **Baseline**: < 10ms
- **Expected**: < 50ms
- **Warning Threshold**: > 100ms
- **Critical Threshold**: > 500ms
- **Alert**: Alert if 5 consecutive writes exceed warning threshold

### 4. Order Intent to Order Submitted Ratio
- **Metric**: Ratio of order_intent events to order_submitted events
- **Measurement**: Ratio as decimal (should be 1:1)
- **Baseline**: 1.0
- **Expected**: 1.0
- **Warning Threshold**: < 0.95 or > 1.05
- **Critical Threshold**: < 0.9 or > 1.1
- **Alert**: Alert if ratio deviates from expected range

### 5. Order Failed Rate
- **Metric**: Percentage of orders that fail after write-ahead logging
- **Measurement**: Failure rate as percentage
- **Baseline**: < 1%
- **Expected**: < 1%
- **Warning Threshold**: > 5%
- **Critical Threshold**: > 10%
- **Alert**: Alert if failure rate exceeds warning threshold

### 6. Retry Count Distribution
- **Metric**: Distribution of retry counts for transient errors
- **Measurement**: Histogram of retry counts
- **Baseline**: 90% of orders succeed on first attempt
- **Expected**: 90% of orders succeed on first attempt
- **Warning Threshold**: < 80% first attempt success
- **Critical Threshold**: < 70% first attempt success
- **Alert**: Alert if first attempt success rate falls below warning threshold

### 7. Recovery Success Rate
- **Metric**: Percentage of successful recoveries from crash
- **Measurement**: Success rate as percentage
- **Baseline**: 100%
- **Expected**: 100%
- **Warning Threshold**: < 95%
- **Critical Threshold**: < 90%
- **Alert**: Alert if recovery success rate falls below warning threshold

### 8. Recovery Latency
- **Metric**: Time to complete recovery from crash
- **Measurement**: Duration in seconds
- **Baseline**: < 30s
- **Expected**: < 60s
- **Warning Threshold**: > 120s
- **Critical Threshold**: > 300s
- **Alert**: Alert if recovery latency exceeds warning threshold

### 9. Broker Reconciliation Success Rate
- **Metric**: Percentage of successful broker reconciliations
- **Measurement**: Success rate as percentage
- **Baseline**: 100%
- **Expected**: 100%
- **Warning Threshold**: < 95%
- **Critical Threshold**: < 90%
- **Alert**: Alert if reconciliation success rate falls below warning threshold

### 10. Duplicate Order Detection
- **Metric**: Number of duplicate orders detected
- **Measurement**: Count of duplicate orders
- **Baseline**: 0
- **Expected**: 0
- **Warning Threshold**: > 0
- **Critical Threshold**: > 1
- **Alert**: Alert immediately if any duplicate orders are detected

## Monitoring Tools and Dashboards

### 1. Application Metrics (Prometheus/Grafana)
- **Tool**: Prometheus for metrics collection, Grafana for visualization
- **Metrics**: Order submission latency, audit log write latency, retry counts
- **Dashboard**: Create a dedicated dashboard for Phase 1.6A metrics
- **Refresh Interval**: 30 seconds

### 2. Log Aggregation (ELK Stack)
- **Tool**: Elasticsearch, Logstash, Kibana
- **Logs**: Application logs, audit logs
- **Dashboard**: Create a dashboard for log analysis
- **Alerts**: Configure alerts for error patterns in logs

### 3. Audit Log Monitoring
- **Tool**: Custom script or log monitoring tool
- **Metrics**: Audit log write success rate, audit log write latency
- **Dashboard**: Create a dashboard for audit log health
- **Alerts**: Configure alerts for audit log failures

### 4. Broker State Monitoring
- **Tool**: IBKR API or TWS monitoring
- **Metrics**: Open orders, positions, account summary
- **Dashboard**: Create a dashboard for broker state
- **Alerts**: Configure alerts for unexpected broker state changes

### 5. Recovery Monitoring
- **Tool**: Custom script or monitoring tool
- **Metrics**: Recovery success rate, recovery latency, recovery action distribution
- **Dashboard**: Create a dashboard for recovery health
- **Alerts**: Configure alerts for recovery failures

## Alert Configuration

### Alert Severity Levels

#### Critical Alerts
- Duplicate orders detected
- Recovery success rate < 90%
- Audit log write success rate < 95%
- Order submission latency > 10000ms
- Data inconsistency detected

#### Warning Alerts
- Recovery success rate < 95%
- Audit log write success rate < 99%
- Order submission latency > 5000ms
- Audit log write latency > 100ms
- Order failed rate > 5%
- Broker reconciliation success rate < 95%

#### Info Alerts
- Order intent to order submitted ratio deviation
- Retry count distribution change
- Recovery latency > 120s

### Alert Notification Channels

#### Critical Alerts
- PagerDuty / On-call rotation
- SMS to on-call engineer
- Email to engineering team

#### Warning Alerts
- Slack channel #nanobook-alerts
- Email to engineering team

#### Info Alerts
- Slack channel #nanobook-info
- Daily digest email

## Monitoring Schedule

### Pre-Rollout Monitoring
- **Timeline**: 1 week before rollout
- **Actions**:
  - Establish baseline metrics without feature flag
  - Set up monitoring dashboards
  - Configure alert thresholds
  - Test alert notifications

### Dry-Run Monitoring
- **Timeline**: During dry-run phase
- **Actions**:
  - Monitor all metrics in dry-run mode
  - Verify audit log integrity
  - Check for any unexpected behavior
  - Validate alert thresholds

### Live Rollout Monitoring
- **Timeline**: During live rollout
- **Actions**:
  - Monitor all metrics in live mode
  - Watch for alert triggers
  - Verify order submission works correctly
  - Check recovery functionality

### Post-Rollout Monitoring
- **Timeline**: 1 week after rollout
- **Actions**:
  - Monitor metrics for trends
  - Review alert history
  - Analyze performance data
  - Document any issues

## Monitoring Checklist

### Pre-Rollout
- [ ] Baseline metrics established
- [ ] Monitoring dashboards created
- [ ] Alert thresholds configured
- [ ] Alert notifications tested
- [ ] Team trained on monitoring tools

### During Rollout
- [ ] Metrics being collected
- [ ] Dashboards displaying data
- [ ] Alerts functioning correctly
- [ ] Team monitoring alerts
- [ ] No critical alerts triggered

### Post-Rollout
- [ ] Metrics within expected ranges
- [ ] No alert triggers
- [ ] Performance meets expectations
- [ ] Audit logs valid
- [ ] Recovery works correctly

## Monitoring Best Practices

### 1. Regular Dashboard Reviews
- Review dashboards daily during rollout
- Look for trends or anomalies
- Compare metrics to baseline

### 2. Alert Threshold Tuning
- Adjust thresholds based on observed behavior
- Avoid alert fatigue by tuning thresholds appropriately
- Document threshold changes

### 3. Metric Retention
- Retain metrics for at least 30 days
- Archive metrics for long-term analysis
- Use retained metrics for trend analysis

### 4. Alert Escalation
- Define escalation procedures for critical alerts
- Ensure on-call rotation is staffed
- Document escalation paths

### 5. Monitoring Documentation
- Document all monitoring configurations
- Keep alert threshold documentation up to date
- Share monitoring knowledge with team

## Monitoring Failure Handling

If monitoring fails or is unavailable:
1. Escalate to infrastructure team
2. Fall back to manual log review
3. Increase manual monitoring frequency
4. Consider pausing rollout if monitoring is critical

## Monitoring Success Criteria

Monitoring is considered successful if:
1. All critical metrics are being collected
2. Dashboards are displaying data correctly
3. Alerts are triggering appropriately
4. No unexpected metric deviations
5. Team is actively monitoring alerts
6. Issues are detected and addressed quickly
