# Phase 1.6B Monitoring Requirements: Write-Ahead Logging for Positions and Quotes

## Overview

This document describes the monitoring requirements for Phase 1.6B, which introduces write-ahead logging for positions fetch and quotes fetch operations. Monitoring is critical to ensure the feature works correctly and to detect issues early.

## Metrics to Monitor

### 1. Positions Fetch Latency
- **Metric**: Time from positions_intent to positions_result completion
- **Measurement**: Duration in milliseconds
- **Baseline**: < 2000ms (without write-ahead logging)
- **Expected**: < 2500ms (with write-ahead logging, allowing for audit log write overhead)
- **Warning Threshold**: > 5000ms
- **Critical Threshold**: > 10000ms
- **Alert**: Alert if 5 consecutive fetches exceed warning threshold

### 2. Quotes Fetch Latency
- **Metric**: Time from quotes_intent to quotes_result completion
- **Measurement**: Duration in milliseconds
- **Baseline**: < 1000ms (without write-ahead logging)
- **Expected**: < 1500ms (with write-ahead logging, allowing for audit log write overhead)
- **Warning Threshold**: > 3000ms
- **Critical Threshold**: > 5000ms
- **Alert**: Alert if 5 consecutive fetches exceed warning threshold

### 3. Audit Log Write Success Rate
- **Metric**: Percentage of successful audit log writes
- **Measurement**: Success rate as percentage
- **Baseline**: 100%
- **Expected**: 100%
- **Warning Threshold**: < 99%
- **Critical Threshold**: < 95%
- **Alert**: Alert if success rate falls below warning threshold

### 4. Audit Log Write Latency
- **Metric**: Time to write audit log entries
- **Measurement**: Duration in milliseconds
- **Baseline**: < 10ms
- **Expected**: < 50ms
- **Warning Threshold**: > 100ms
- **Critical Threshold**: > 500ms
- **Alert**: Alert if 5 consecutive writes exceed warning threshold

### 5. Positions Intent to Result Ratio
- **Metric**: Ratio of positions_intent events to positions_result events
- **Measurement**: Ratio as decimal (should be 1:1)
- **Baseline**: 1.0
- **Expected**: 1.0
- **Warning Threshold**: < 0.95 or > 1.05
- **Critical Threshold**: < 0.9 or > 1.1
- **Alert**: Alert if ratio deviates from expected range

### 6. Quotes Intent to Result Ratio
- **Metric**: Ratio of quotes_intent events to quotes_result events
- **Measurement**: Ratio as decimal (should be 1:1)
- **Baseline**: 1.0
- **Expected**: 1.0
- **Warning Threshold**: < 0.95 or > 1.05
- **Critical Threshold**: < 0.9 or > 1.1
- **Alert**: Alert if ratio deviates from expected range

### 7. Positions Fetch Failure Rate
- **Metric**: Percentage of positions fetches that fail after write-ahead logging
- **Measurement**: Failure rate as percentage
- **Baseline**: < 1%
- **Expected**: < 1%
- **Warning Threshold**: > 5%
- **Critical Threshold**: > 10%
- **Alert**: Alert if failure rate exceeds warning threshold

### 8. Quotes Fetch Failure Rate
- **Metric**: Percentage of quotes fetches that fail after write-ahead logging
- **Measurement**: Failure rate as percentage
- **Baseline**: < 1%
- **Expected**: < 1%
- **Warning Threshold**: > 5%
- **Critical Threshold**: > 10%
- **Alert**: Alert if failure rate exceeds warning threshold

### 9. Recovery Success Rate (Positions/Quotes)
- **Metric**: Percentage of successful recoveries from crash at positions/quotes checkpoints
- **Measurement**: Success rate as percentage
- **Baseline**: 100%
- **Expected**: 100%
- **Warning Threshold**: < 95%
- **Critical Threshold**: < 90%
- **Alert**: Alert if recovery success rate falls below warning threshold

### 10. Recovery Latency (Positions/Quotes)
- **Metric**: Time to complete recovery from crash at positions/quotes checkpoints
- **Measurement**: Duration in seconds
- **Baseline**: < 30s
- **Expected**: < 60s
- **Warning Threshold**: > 120s
- **Critical Threshold**: > 300s
- **Alert**: Alert if recovery latency exceeds warning threshold

### 11. Duplicate Positions/Quotes Fetch Detection
- **Metric**: Number of duplicate positions or quotes fetches detected
- **Measurement**: Count of duplicate fetches
- **Baseline**: 0
- **Expected**: 0
- **Warning Threshold**: > 0
- **Critical Threshold**: > 1
- **Alert**: Alert immediately if any duplicate fetches are detected

### 12. Positions Data Integrity
- **Metric**: Percentage of positions_result events that match broker state
- **Measurement**: Match rate as percentage
- **Baseline**: 100%
- **Expected**: 100%
- **Warning Threshold**: < 99%
- **Critical Threshold**: < 95%
- **Alert**: Alert if match rate falls below warning threshold

### 13. Quotes Data Integrity
- **Metric**: Percentage of quotes_result events that match broker state
- **Measurement**: Match rate as percentage
- **Baseline**: 100%
- **Expected**: 100%
- **Warning Threshold**: < 99%
- **Critical Threshold**: < 95%
- **Alert**: Alert if match rate falls below warning threshold

## Monitoring Tools and Dashboards

### 1. Application Metrics (Prometheus/Grafana)
- **Tool**: Prometheus for metrics collection, Grafana for visualization
- **Metrics**: Positions fetch latency, quotes fetch latency, audit log write latency, intent:result ratios
- **Dashboard**: Create a dedicated dashboard for Phase 1.6B metrics
- **Refresh Interval**: 30 seconds

### 2. Log Aggregation (ELK Stack)
- **Tool**: Elasticsearch, Logstash, Kibana
- **Logs**: Application logs, audit logs
- **Dashboard**: Create a dashboard for log analysis
- **Alerts**: Configure alerts for error patterns in logs

### 3. Audit Log Monitoring
- **Tool**: Custom script or log monitoring tool
- **Metrics**: Audit log write success rate, audit log write latency, intent:result ratios
- **Dashboard**: Create a dashboard for audit log health
- **Alerts**: Configure alerts for audit log failures

### 4. Broker State Monitoring
- **Tool**: IBKR API or TWS monitoring
- **Metrics**: Positions, quotes, account summary
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
- Duplicate positions/quotes fetches detected
- Recovery success rate < 90% at positions/quotes checkpoints
- Audit log write success rate < 95%
- Positions fetch latency > 10000ms
- Quotes fetch latency > 5000ms
- Data inconsistency detected

#### Warning Alerts
- Recovery success rate < 95% at positions/quotes checkpoints
- Audit log write success rate < 99%
- Positions fetch latency > 5000ms
- Quotes fetch latency > 3000ms
- Audit log write latency > 100ms
- Positions fetch failure rate > 5%
- Quotes fetch failure rate > 5%
- Positions intent:result ratio deviation
- Quotes intent:result ratio deviation

#### Info Alerts
- Recovery latency > 120s at positions/quotes checkpoints
- Positions data integrity < 99%
- Quotes data integrity < 99%

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
  - Verify positions and quotes fetch work correctly
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
