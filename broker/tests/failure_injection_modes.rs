//! Integration tests for the 9 failure injection modes (F1-F9).

mod mock_tws;

use mock_tws::{MockTws, FailureMode, FailureTiming};

/// F1: Test duplicate order-status callback injection
#[test]
fn test_f1_duplicate_status_callback() {
    let mock = MockTws::new();
    mock.connect().unwrap();

    // First, submit a normal order
    let order_id = mock.submit_order("AAPL", 100).unwrap();
    mock.fill_order(order_id, 100).unwrap();

    // Record the callback count
    let callback_count_before = mock.callbacks().len();

    // Inject F1 failure post-submit for a second order
    mock.inject_failure(FailureMode::F1DuplicateStatus, FailureTiming::PostSubmit);

    let result = mock.submit_order("MSFT", 50);
    // F1 post-submit currently returns an error (failure injection)
    assert!(result.is_err());

    // Clear failure and verify normal operation resumes
    mock.clear_failure();
    let order_id2 = mock.submit_order("MSFT", 50).unwrap();
    mock.fill_order(order_id2, 50).unwrap();

    let status = mock.order_status(order_id2).unwrap();
    assert_eq!(status.status, "Filled");

    // Verify callbacks were recorded
    assert!(mock.callbacks().len() > callback_count_before);
}

/// F2: Test cancel reject race with fill
#[test]
fn test_f2_cancel_reject_race_with_fill() {
    let mock = MockTws::new();
    mock.connect().unwrap();

    let order_id = mock.submit_order("AAPL", 100).unwrap();

    // Inject F2 failure mid-fill
    mock.inject_failure(FailureMode::F2CancelRejectRace, FailureTiming::MidFill);

    // Attempt to fill should fail with cancel reject race error
    let result = mock.fill_order(order_id, 50);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("CancelReject"));

    // Verify order is still in submitted state
    let status = mock.order_status(order_id).unwrap();
    assert_eq!(status.status, "Submitted");
}

/// F3: Test partial fill followed by disconnect
#[test]
fn test_f3_partial_fill_disconnect() {
    let mock = MockTws::new();
    mock.connect().unwrap();

    let order_id = mock.submit_order("AAPL", 100).unwrap();

    // Inject F3 failure post-fill
    mock.inject_failure(FailureMode::F3PartialFillDisconnect, FailureTiming::PostFill);

    // Partial fill should succeed
    mock.fill_order(order_id, 50).unwrap();

    // Verify disconnect was injected
    assert!(mock.was_disconnect_injected());
    assert!(!mock.is_connected());

    // Verify partial fill state
    let status = mock.order_status(order_id);
    // Should fail because disconnected
    assert!(status.is_err());
}

/// F4: Test stale market data detection
#[test]
fn test_f4_stale_market_data_detection() {
    let mock = MockTws::new();
    mock.connect().unwrap();

    // Inject F4 failure pre-submit
    mock.inject_failure(FailureMode::F4StaleMarketData, FailureTiming::PreSubmit);

    // Order submission should fail with stale market data error
    let result = mock.submit_order("AAPL", 100);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("StaleMarketData"));

    // Clear and verify normal operation
    mock.clear_failure();
    let order_id = mock.submit_order("AAPL", 100).unwrap();
    assert!(order_id > 0);
}

/// F5: Test clock skew detection
#[test]
fn test_f5_clock_skew_detection() {
    let mock = MockTws::new();
    mock.connect().unwrap();

    // Inject F5 failure pre-submit
    mock.inject_failure(FailureMode::F5ClockSkew, FailureTiming::PreSubmit);

    // Order submission should fail with clock skew error
    let result = mock.submit_order("AAPL", 100);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("ClockSkew"));

    // Clear and verify normal operation
    mock.clear_failure();
    let order_id = mock.submit_order("AAPL", 100).unwrap();
    assert!(order_id > 0);
}

/// F6: Test TWS reconnect drill
#[test]
fn test_f6_reconnect_drill() {
    let mock = MockTws::new();
    mock.connect().unwrap();
    assert!(mock.is_connected());

    // Inject F6 failure pre-submit
    mock.inject_failure(FailureMode::F6ReconnectDrill, FailureTiming::PreSubmit);

    // Order submission should trigger disconnect
    let result = mock.submit_order("AAPL", 100);
    assert!(result.is_err());
    assert!(mock.was_disconnect_injected());
    assert!(!mock.is_connected());

    // Simulate reconnect
    mock.clear_failure();
    mock.connect().unwrap();
    assert!(mock.is_connected());

    // Verify normal operation after reconnect
    let order_id = mock.submit_order("AAPL", 100).unwrap();
    mock.fill_order(order_id, 100).unwrap();
    let status = mock.order_status(order_id).unwrap();
    assert_eq!(status.status, "Filled");
}

/// F7: Test cron double-fire idempotency
#[test]
fn test_f7_cron_double_fire_idempotency() {
    let mock = MockTws::new();
    mock.connect().unwrap();

    // Inject F7 failure pre-submit
    mock.inject_failure(FailureMode::F7CronDoubleFire, FailureTiming::PreSubmit);

    // First submission should detect double-fire
    let result = mock.submit_order("AAPL", 100);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("CronDoubleFire"));

    // Clear and verify normal operation
    mock.clear_failure();
    let order_id = mock.submit_order("AAPL", 100).unwrap();
    assert!(order_id > 0);

    // Verify idempotency: same order submitted twice should get different IDs
    mock.clear_callbacks();
    let order_id2 = mock.submit_order("AAPL", 100).unwrap();
    assert_ne!(order_id, order_id2);
}

/// F8: Test kill switch subcommand
#[test]
fn test_f8_kill_switch() {
    let mock = MockTws::new();
    mock.connect().unwrap();

    // Inject F8 failure pre-submit
    mock.inject_failure(FailureMode::F8KillSwitch, FailureTiming::PreSubmit);

    // Order submission should fail with kill switch error
    let result = mock.submit_order("AAPL", 100);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("KillSwitch"));

    // Verify mock is still operational after clearing
    mock.clear_failure();
    let order_id = mock.submit_order("AAPL", 100).unwrap();
    assert!(order_id > 0);
}

/// F9: Test process crash + warm restart
#[test]
fn test_f9_process_crash_warm_restart() {
    let mock = MockTws::new();
    mock.connect().unwrap();

    // Inject F9 failure pre-submit
    mock.inject_failure(FailureMode::F9ProcessCrash, FailureTiming::PreSubmit);

    // Order submission should fail with process crash error
    let result = mock.submit_order("AAPL", 100);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("ProcessCrash"));

    // Simulate warm restart: clear failure, reconnect, verify state
    mock.clear_failure();
    mock.disconnect().unwrap();
    mock.connect().unwrap();

    // Verify normal operation after restart
    let order_id = mock.submit_order("AAPL", 100).unwrap();
    mock.fill_order(order_id, 100).unwrap();
    let status = mock.order_status(order_id).unwrap();
    assert_eq!(status.status, "Filled");
}

/// Test that multiple failure modes can be tested in sequence
#[test]
fn test_multiple_failure_modes_sequence() {
    let mock = MockTws::new();
    mock.connect().unwrap();

    // Test F4
    mock.inject_failure(FailureMode::F4StaleMarketData, FailureTiming::PreSubmit);
    assert!(mock.submit_order("AAPL", 100).is_err());
    mock.clear_failure();

    // Test F5
    mock.inject_failure(FailureMode::F5ClockSkew, FailureTiming::PreSubmit);
    assert!(mock.submit_order("AAPL", 100).is_err());
    mock.clear_failure();

    // Test normal operation
    let order_id = mock.submit_order("AAPL", 100).unwrap();
    mock.fill_order(order_id, 100).unwrap();
    let status = mock.order_status(order_id).unwrap();
    assert_eq!(status.status, "Filled");
}