//! Binance WebSocket user-data stream client.

use std::error::Error;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::net::TcpStream;
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
        let (stream, _) = connect_async(self.endpoint()).await.map_err(|err| {
            self.connected.store(false, Ordering::SeqCst);
            self.state = ConnectionState::Disconnected;
            err
        })?;

        self.client = Some(stream);
        self.connected.store(true, Ordering::SeqCst);
        self.state = ConnectionState::Connected;
        Ok(())
    }

    /// Subscribe to user-data updates and parse one available text message.
    ///
    /// Binance user-data streams normally require a REST-created listen key
    /// and a `/ws/<listenKey>` URL. Phase 1 keeps authentication deliberately
    /// minimal and sends a simple subscription frame so tests and future mock
    /// servers can exercise the connection path without live credentials.
    pub async fn subscribe_user_data(&mut self) -> Result<(), Box<dyn Error>> {
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
