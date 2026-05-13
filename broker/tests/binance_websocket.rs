#[path = "../src/binance/websocket.rs"]
#[allow(dead_code)]
mod websocket;

use websocket::{BinanceWebSocket, ConnectionState};

#[test]
fn test_websocket_construction() {
    let ws = BinanceWebSocket::new("api-key", "secret-key", true);

    assert!(!ws.is_connected());
    assert_eq!(ws.state(), ConnectionState::Disconnected);
    assert!(ws.events().is_empty());
}

#[test]
fn test_websocket_is_connected() {
    let ws = BinanceWebSocket::new("api-key", "secret-key", false);

    assert!(!ws.is_connected());
    assert_eq!(ws.state(), ConnectionState::Disconnected);
}

#[tokio::test]
async fn test_websocket_disconnect() {
    let mut ws = BinanceWebSocket::new("api-key", "secret-key", true);

    ws.disconnect().await;

    assert!(!ws.is_connected());
    assert_eq!(ws.state(), ConnectionState::Disconnected);
}
