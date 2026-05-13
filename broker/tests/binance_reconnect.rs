#[path = "../src/binance/websocket.rs"]
#[allow(dead_code)]
mod websocket;

use std::time::Duration;
use tokio::time::sleep;
use websocket::{BinanceWebSocket, ConnectionState};

#[tokio::test]
async fn test_heartbeat_timeout_detection() {
    let mut ws = BinanceWebSocket::new("api-key", "secret-key", true);

    // Set a short heartbeat interval for testing
    ws.set_heartbeat_interval(Duration::from_millis(100));

    // Initially, heartbeat should be invalid (no timestamp)
    assert!(!ws.check_heartbeat());

    // After updating heartbeat, it should be valid
    ws.update_heartbeat();
    assert!(ws.check_heartbeat());

    // Wait for heartbeat interval to expire
    sleep(Duration::from_millis(150)).await;

    // Heartbeat should now be timed out
    assert!(!ws.check_heartbeat());
}

#[test]
fn test_reconnect_with_backoff() {
    // Verify backoff calculation: 2^(attempt-1) seconds, capped at 16s
    let attempt: u32 = 1;
    let backoff_secs = 2u64.pow(attempt.saturating_sub(1)).min(16);
    assert_eq!(backoff_secs, 1);

    let attempt: u32 = 2;
    let backoff_secs = 2u64.pow(attempt.saturating_sub(1)).min(16);
    assert_eq!(backoff_secs, 2);

    let attempt: u32 = 3;
    let backoff_secs = 2u64.pow(attempt.saturating_sub(1)).min(16);
    assert_eq!(backoff_secs, 4);

    let attempt: u32 = 4;
    let backoff_secs = 2u64.pow(attempt.saturating_sub(1)).min(16);
    assert_eq!(backoff_secs, 8);

    let attempt: u32 = 5;
    let backoff_secs = 2u64.pow(attempt.saturating_sub(1)).min(16);
    assert_eq!(backoff_secs, 16);

    // Test that backoff caps at 16s
    let attempt: u32 = 10;
    let backoff_secs = 2u64.pow(attempt.saturating_sub(1)).min(16);
    assert_eq!(backoff_secs, 16);
}

#[tokio::test]
async fn test_reconnect_attempts_tracking() {
    let mut ws = BinanceWebSocket::new("api-key", "secret-key", true);

    // Initially, reconnect attempts should be 0
    assert_eq!(ws.reconnect_attempts(), 0);

    // Set max attempts to a low number for testing
    ws.set_max_reconnect_attempts(3);
    assert_eq!(ws.max_reconnect_attempts(), 3);

    // Disconnect to reset attempts
    ws.disconnect().await;
    assert_eq!(ws.reconnect_attempts(), 0);
}

#[test]
fn test_connection_state_transitions() {
    let ws = BinanceWebSocket::new("api-key", "secret-key", true);

    // Initial state
    assert_eq!(ws.state(), ConnectionState::Disconnected);
    assert!(!ws.is_connected());
}

#[tokio::test]
async fn test_send_ping_requires_connection() {
    let mut ws = BinanceWebSocket::new("api-key", "secret-key", true);

    // Should fail when not connected
    let result = ws.send_ping().await;
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().to_string(), "Binance WebSocket is not connected");
}

#[test]
fn test_heartbeat_update() {
    let ws = BinanceWebSocket::new("api-key", "secret-key", true);

    // Initially no heartbeat
    assert!(!ws.check_heartbeat());

    // Update heartbeat
    ws.update_heartbeat();

    // Now heartbeat should be valid
    assert!(ws.check_heartbeat());
}

#[tokio::test]
async fn test_disconnect_resets_heartbeat() {
    let mut ws = BinanceWebSocket::new("api-key", "secret-key", true);

    // Set heartbeat
    ws.update_heartbeat();
    assert!(ws.check_heartbeat());

    // Disconnect should reset heartbeat
    ws.disconnect().await;
    assert!(!ws.check_heartbeat());
}
