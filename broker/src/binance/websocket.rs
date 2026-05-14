//! Binance WebSocket user-data stream client.

use std::error::Error;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::net::TcpStream;
use tokio::time::{sleep, Instant};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};

/// Current Binance WebSocket connection state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Connected,
    Disconnected,
    Reconnecting,
}

/// Parsed Binance user-data WebSocket event.
#[derive(Debug, Clone, PartialEq)]
pub enum BinanceWebSocketEvent {
    AccountUpdate(AccountUpdate),
    ExecutionReport(ExecutionReport),
    Other(Value),
}

/// Minimal account update event for Phase 1.
#[derive(Debug, Clone, PartialEq)]
pub struct AccountUpdate {
    pub event_time: Option<u64>,
    pub raw: Value,
}

/// Minimal execution report event for Phase 1.
#[derive(Debug, Clone, PartialEq)]
pub struct ExecutionReport {
    pub event_time: Option<u64>,
    pub symbol: Option<String>,
    pub order_id: Option<u64>,
    pub client_order_id: Option<String>,
    pub order_status: Option<String>,
    pub raw: Value,
}

/// Binance WebSocket client.
pub struct BinanceWebSocket {
    api_key: String,
    secret_key: String,
    testnet: bool,
    connected: Arc<AtomicBool>,
    state: ConnectionState,
    client: Option<WebSocketStream<MaybeTlsStream<TcpStream>>>,
    events: Vec<BinanceWebSocketEvent>,
    // Heartbeat mechanism
    last_heartbeat: Arc<Mutex<Option<Instant>>>,
    heartbeat_interval: Duration,
    // Auto-reconnect mechanism
    max_reconnect_attempts: u32,
    reconnect_attempts: u32,
}

impl BinanceWebSocket {
    /// Create a disconnected Binance WebSocket client.
    pub fn new(api_key: &str, secret_key: &str, testnet: bool) -> Self {
        Self {
            api_key: api_key.to_string(),
            secret_key: secret_key.to_string(),
            testnet,
            connected: Arc::new(AtomicBool::new(false)),
            state: ConnectionState::Disconnected,
            client: None,
            events: Vec::new(),
            last_heartbeat: Arc::new(Mutex::new(None)),
            heartbeat_interval: Duration::from_secs(10),
            max_reconnect_attempts: 5,
            reconnect_attempts: 0,
        }
    }

    /// Returns true when the underlying socket is currently connected.
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    /// Return the tracked connection state.
    pub fn state(&self) -> ConnectionState {
        self.state
    }

    /// Return parsed events buffered by the most recent read/subscribe call.
    pub fn events(&self) -> &[BinanceWebSocketEvent] {
        &self.events
    }

    /// Connect to the Binance WebSocket endpoint.
    pub async fn connect(&mut self) -> Result<(), Box<dyn Error>> {
        self.state = ConnectionState::Reconnecting;
        self.reconnect_attempts = 0;
        let (stream, _) = connect_async(self.endpoint()).await.map_err(|err| {
            self.connected.store(false, Ordering::SeqCst);
            self.state = ConnectionState::Disconnected;
            err
        })?;

        self.client = Some(stream);
        self.connected.store(true, Ordering::SeqCst);
        self.state = ConnectionState::Connected;
        self.update_heartbeat();
        Ok(())
    }

    /// Subscribe to user-data updates and parse one available text message.
    ///
    /// Binance user-data streams normally require a REST-created listen key
    /// and a `/ws/<listenKey>` URL. Phase 1 keeps authentication deliberately
    /// minimal and sends a simple subscription frame so tests and future mock
    /// servers can exercise the connection path without live credentials.
    pub async fn subscribe_user_data(&mut self) -> Result<(), Box<dyn Error>> {
        // Check heartbeat before processing
        if !self.check_heartbeat() {
            self.state = ConnectionState::Disconnected;
            return Err("Heartbeat timeout detected".into());
        }

        let client = self
            .client
            .as_mut()
            .ok_or("Binance WebSocket is not connected")?;

        let subscription = serde_json::json!({
            "method": "SUBSCRIBE",
            "params": ["!userData"],
            "id": 1,
        });
        client.send(Message::Text(subscription.to_string())).await?;

        if let Some(message) = client.next().await {
            if let Some(event) = Self::parse_frame(message?)? {
                self.events.push(event);
                // Update heartbeat on successful message processing
                self.update_heartbeat();
            }
        }

        Ok(())
    }

    /// Close the WebSocket connection and reset connection state.
    pub async fn disconnect(&mut self) {
        if let Some(mut client) = self.client.take() {
            let _ = client.close(None).await;
        }
        self.connected.store(false, Ordering::SeqCst);
        self.state = ConnectionState::Disconnected;
        *self.last_heartbeat.lock().expect("heartbeat mutex poisoned") = None;
        self.reconnect_attempts = 0;
    }

    /// Send a ping message to the server.
    pub async fn send_ping(&mut self) -> Result<(), Box<dyn Error>> {
        let client = self
            .client
            .as_mut()
            .ok_or("Binance WebSocket is not connected")?;

        client.send(Message::Ping(vec![])).await?;
        self.update_heartbeat();
        Ok(())
    }

    /// Attempt to reconnect with exponential backoff.
    ///
    /// Uses exponential backoff: 1s, 2s, 4s, 8s, 16s (max).
    /// Returns error if all attempts fail.
    pub async fn reconnect_with_backoff(&mut self) -> Result<(), Box<dyn Error>> {
        self.reconnect_attempts = 0;

        while self.reconnect_attempts < self.max_reconnect_attempts {
            self.reconnect_attempts += 1;
            self.state = ConnectionState::Reconnecting;

            // Calculate backoff delay: 2^(attempt-1) seconds, capped at 16s
            let backoff_secs = 2u64.pow(self.reconnect_attempts.saturating_sub(1)).min(16);
            let backoff = Duration::from_secs(backoff_secs);

            // Wait for backoff
            sleep(backoff).await;

            // Attempt to reconnect
            if let Ok(()) = self.connect().await {
                return Ok(());
            }
        }

        // All attempts failed
        self.state = ConnectionState::Disconnected;
        Err(format!(
            "Failed to reconnect after {} attempts",
            self.max_reconnect_attempts
        )
        .into())
    }

    /// Check if heartbeat has timed out.
    ///
    /// Returns true if the heartbeat is still valid (no timeout),
    /// false if a timeout has occurred.
    pub fn check_heartbeat(&self) -> bool {
        let last_heartbeat = self.last_heartbeat.lock().expect("heartbeat mutex poisoned");
        if let Some(last) = *last_heartbeat {
            last.elapsed() < self.heartbeat_interval
        } else {
            false
        }
    }

    /// Update the last heartbeat timestamp to now.
    pub fn update_heartbeat(&self) {
        *self.last_heartbeat.lock().expect("heartbeat mutex poisoned") = Some(Instant::now());
    }

    /// Get the current number of reconnect attempts.
    pub fn reconnect_attempts(&self) -> u32 {
        self.reconnect_attempts
    }

    /// Get the maximum number of reconnect attempts.
    pub fn max_reconnect_attempts(&self) -> u32 {
        self.max_reconnect_attempts
    }

    /// Set the maximum number of reconnect attempts (for testing).
    pub fn set_max_reconnect_attempts(&mut self, max: u32) {
        self.max_reconnect_attempts = max;
    }

    /// Set the heartbeat interval (for testing).
    pub fn set_heartbeat_interval(&mut self, interval: Duration) {
        self.heartbeat_interval = interval;
    }

    fn endpoint(&self) -> &'static str {
        let _ = (&self.api_key, &self.secret_key, self.testnet);
        "wss://stream.binance.com:9443/ws"
    }

    fn parse_frame(message: Message) -> Result<Option<BinanceWebSocketEvent>, Box<dyn Error>> {
        match message {
            Message::Text(text) => Self::parse_text_message(&text).map(Some),
            Message::Binary(bytes) => {
                let text = String::from_utf8(bytes)?;
                Self::parse_text_message(&text).map(Some)
            }
            Message::Ping(_) | Message::Pong(_) => Ok(None),
            Message::Close(_) => Ok(None),
            Message::Frame(_) => Ok(None),
        }
    }

    fn parse_text_message(text: &str) -> Result<BinanceWebSocketEvent, Box<dyn Error>> {
        let value: Value = serde_json::from_str(text)?;
        let event_type = value.get("e").and_then(Value::as_str);
        let event_time = value.get("E").and_then(Value::as_u64);

        match event_type {
            Some("outboundAccountPosition") | Some("balanceUpdate") => {
                Ok(BinanceWebSocketEvent::AccountUpdate(AccountUpdate {
                    event_time,
                    raw: value,
                }))
            }
            Some("executionReport") => {
                Ok(BinanceWebSocketEvent::ExecutionReport(ExecutionReport {
                    event_time,
                    symbol: value
                        .get("s")
                        .and_then(Value::as_str)
                        .map(ToString::to_string),
                    order_id: value.get("i").and_then(Value::as_u64),
                    client_order_id: value
                        .get("c")
                        .and_then(Value::as_str)
                        .map(ToString::to_string),
                    order_status: value
                        .get("X")
                        .and_then(Value::as_str)
                        .map(ToString::to_string),
                    raw: value,
                }))
            }
            _ => Ok(BinanceWebSocketEvent::Other(value)),
        }
    }
}
