# Failure Injection Test Harness

This directory contains a wire-level mock of the IBKR TWS/Gateway protocol with deterministic failure injection for testing broker resilience.

## Overview

The harness provides two testing approaches:

1. **Pure Rust integration tests** - Fast, deterministic testing of individual failure modes
2. **Python-driven scenario runner** - Complex multi-step scenario testing via YAML DSL

## Directory Structure

```
broker/tests/
├── mock_tws.rs                      # Wire-level TWS mock implementation (shared)
├── failure_injection_smoke.rs       # Basic smoke tests
├── failure_injection_modes.rs       # F1-F9 failure mode integration tests
└── failure_injection/
    ├── scenarios/                   # Python scenario definitions
    │   ├── runner.py                # Scenario runner script
    │   ├── partial_fill_reconnect.yaml
    │   └── stale_data_clock_skew.yaml
    └── README.md                    # This file
```

## MockTws API

The `MockTws` struct simulates TWS/Gateway wire protocol behavior and is located in `broker/tests/mock_tws.rs`:

```rust
mod mock_tws;

use mock_tws::{MockTws, FailureMode, FailureTiming};

let mock = MockTws::new();
mock.connect().unwrap();

// Inject a failure mode
mock.inject_failure(FailureMode::F3PartialFillDisconnect, FailureTiming::PostFill);

// Submit order and test behavior
let order_id = mock.submit_order("AAPL", 100).unwrap();
mock.fill_order(order_id, 50).unwrap();

// Assert disconnect was injected
assert!(mock.was_disconnect_injected());
```

### Failure Modes

| Mode | Description | Timing Options |
|------|-------------|----------------|
| F1 | Duplicate order-status callback injection | PreSubmit, PostSubmit |
| F2 | Cancel reject race with fill | PostSubmit, MidFill |
| F3 | Partial fill followed by disconnect | PostFill |
| F4 | Stale market data detection | PreSubmit, PostSubmit |
| F5 | Clock skew detection | PreSubmit |
| F6 | TWS reconnect drill | PreSubmit |
| F7 | Cron double-fire idempotency | PreSubmit |
| F8 | Kill switch subcommand | PreSubmit |
| F9 | Process crash + warm restart | PreSubmit |

### Failure Timing

- `PreSubmit` - Inject before order submission
- `PostSubmit` - Inject after order submission
- `MidFill` - Inject during order fill
- `PostFill` - Inject after order fill

## Running Tests

### Rust Integration Tests

Run all failure injection tests:

```bash
cargo test -p nanobook-broker --test failure_injection_smoke
cargo test -p nanobook-broker --test failure_injection_modes
```

Run all broker tests:

```bash
cargo test -p nanobook-broker
```

Run a specific test:

```bash
cargo test -p nanobook-broker test_f3_partial_fill_disconnect
```

### Python Scenario Runner

The scenario runner reads YAML definitions and generates temporary Rust tests.

List available scenarios:

```bash
cd broker/tests/failure_injection/scenarios
python3 runner.py --list
```

Run all scenarios:

```bash
python3 runner.py
```

Run a specific scenario:

```bash
python3 runner.py partial_fill_reconnect.yaml
```

## Python Scenario DSL

Scenarios are defined in YAML format in the `scenarios/` directory:

```yaml
name: "Partial fill with reconnect"
description: "Combines F3 (partial fill disconnect) with F6 (reconnect drill)"
steps:
  - action: connect
  - action: submit_order
    symbol: "AAPL"
    quantity: 100
  - action: inject_failure
    mode: F3PartialFillDisconnect
    timing: PostFill
  - action: fill_order
    order_id: 1
    quantity: 50
  - action: assert
    condition: disconnected
  - action: clear_failure
  - action: connect
  - action: assert
    condition: connected
```

### Scenario Actions

- `connect` - Connect to mock TWS
- `disconnect` - Disconnect from mock TWS
- `submit_order` - Submit an order (symbol, quantity)
- `fill_order` - Fill an order (order_id, quantity)
- `cancel_order` - Cancel an order (order_id)
- `inject_failure` - Inject a failure mode (mode, timing)
- `clear_failure` - Clear active failure injection
- `clear_callbacks` - Clear recorded callbacks
- `assert` - Assert a condition (disconnected, connected, error, callback_count)

## Implementation Notes

- The mock is deterministic and does not use random values
- All callbacks are recorded for test assertions
- Sequence numbers are tracked to simulate TWS message ordering
- The mock is thread-safe for use in concurrent tests
- The Python runner generates temporary Rust test files that are cleaned up after execution

## Adding New Failure Modes

1. Add variant to `FailureMode` enum in `broker/tests/mock_tws.rs`
2. Implement handling in `MockTws::handle_failure()`
3. Add Rust integration test in `failure_injection_modes.rs`
4. Add Python scenario example in `scenarios/` if complex multi-step testing is needed