# Mock vs Paper Trading Divergence Report

**Generated**: [TIMESTAMP]
**Validation Mode**: [Mock Only / Paper Trading]
**IBKR Host**: [HOST]
**IBKR Port**: [PORT]
**Client ID**: [CLIENT_ID]

## Executive Summary

- **Total Divergences**: [COUNT]
- **Critical**: [COUNT]
- **Warning**: [COUNT]
- **Info**: [COUNT]
- **Overall Status**: [PASS / FAIL]

## Severity Definitions

### Critical
Divergences that indicate fundamental differences between mock and real IBKR behavior. These must be fixed before the mock can be considered accurate.

**Examples**:
- Callback type mismatch (e.g., mock emits OrderFill, paper emits OrderReject)
- Missing callbacks in sequence
- Incorrect order ID mapping
- Wrong callback ordering

**Action Required**: Fix mock implementation immediately.

### Warning
Divergences that may not break tests but indicate differences in behavior. These should be investigated and possibly fixed.

**Examples**:
- Sequence number gaps (not monotonic)
- Timing differences (mock instant, paper delayed)
- Additional informational callbacks in paper not in mock

**Action Required**: Investigate and decide if fix is needed.

### Info
Minor differences that are unlikely to affect test correctness. These are documented for reference but may not require action.

**Examples**:
- Formatting differences in callback details
- Additional metadata in paper callbacks
- Case sensitivity differences

**Action Required**: Review, but likely no action needed.

## Divergence Details

### Test Case: [TEST_CASE_NAME]

**Status**: [PASS / FAIL]

#### Divergence 1: [SHORT_DESCRIPTION]

**Severity**: [Critical / Warning / Info]

**Mock Callback**:
```
[Event Type] - Order ID: [ID] - Sequence: [SEQ] - Details: [DETAILS]
```

**Paper Callback**:
```
[Event Type] - Order ID: [ID] - Sequence: [SEQ] - Details: [DETAILS]
```

**Description**:
[Detailed explanation of the divergence]

**Impact**:
[How this divergence affects the mock's accuracy]

**Recommended Action**:
[Specific steps to fix the mock]

---

### Test Case: [TEST_CASE_NAME]

**Status**: [PASS / FAIL]

[Repeat for each test case with divergences]

## Test Case Summary

| Test Case | Status | Divergences | Critical | Warning | Info |
|-----------|--------|-------------|----------|---------|------|
| test_normal_order_submission | [PASS/FAIL] | [N] | [N] | [N] | [N] |
| test_partial_fill | [PASS/FAIL] | [N] | [N] | [N] | [N] |
| test_order_cancellation | [PASS/FAIL] | [N] | [N] | [N] | [N] |
| test_disconnect_reconnect | [PASS/FAIL] | [N] | [N] | [N] | [N] |
| test_f3_partial_fill_disconnect | [PASS/FAIL] | [N] | [N] | [N] | [N] |

## Remediation Checklist

Use this checklist to track fixes for identified divergences.

### Critical Divergences

- [ ] Fix callback type mismatch in [TEST_CASE]
- [ ] Add missing callback in [TEST_CASE]
- [ ] Correct callback ordering in [TEST_CASE]
- [ ] Fix order ID mapping in [TEST_CASE]

### Warning Divergences

- [ ] Investigate sequence number gaps in [TEST_CASE]
- [ ] Review timing differences in [TEST_CASE]
- [ ] Determine if additional paper callbacks need to be mocked

### Info Divergences

- [ ] Review formatting differences in [TEST_CASE]
- [ ] Document metadata differences in [TEST_CASE]

## Common Patterns

If the same divergence appears across multiple test cases, document it here:

### Pattern 1: [PATTERN_NAME]

**Affected Test Cases**: [LIST]

**Description**: [COMMON DESCRIPTION]

**Root Cause**: [ROOT CAUSE]

**Fix**: [COMMON FIX]

---

## Historical Notes

### [DATE] - Validation Run

- Summary of changes made
- Divergences fixed
- New divergences discovered

### [DATE] - Validation Run

- Summary of changes made
- Divergences fixed
- New divergences discovered

## Next Steps

1. Review all critical divergences and prioritize fixes
2. Implement fixes in `broker/tests/mock_tws.rs`
3. Re-run validation to verify fixes
4. Update this log with results
5. Once all critical divergences are resolved, mock is validated

## Appendix: Mock vs Paper Implementation Differences

This section documents known intentional differences between mock and paper implementations that are acceptable.

### Intentional Difference 1: [NAME]

**Mock Behavior**: [DESCRIPTION]

**Paper Behavior**: [DESCRIPTION]

**Reason for Difference**: [JUSTIFICATION]

**Acceptable**: [YES/NO]

---

*This report is generated automatically by the validation script. Manual edits should be made in the Historical Notes section only.*
