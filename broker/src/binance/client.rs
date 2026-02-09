//! Binance REST API client.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use log::debug;
use reqwest::blocking::Client;

use super::auth;
use super::types::{AccountInfo, BookTicker, OrderResponse};
use crate::error::BrokerError;

/// Blocking Binance REST client.
pub struct BinanceClient {
    client: Client,
    api_key: String,
    secret_key: String,
    base_url: String,
}

impl Drop for BinanceClient {
    fn drop(&mut self) {
        zeroize::Zeroize::zeroize(&mut self.api_key);
        zeroize::Zeroize::zeroize(&mut self.secret_key);
    }
}

impl BinanceClient {
    /// Create a new Binance client.
    pub fn new(api_key: &str, secret_key: &str, testnet: bool) -> Self {
        let base_url = if testnet {
            "https://testnet.binance.vision"
        } else {
            "https://api.binance.com"
        };

        Self {
            client: Client::new(),
            api_key: api_key.to_string(),
            secret_key: secret_key.to_string(),
            base_url: base_url.to_string(),
        }
    }

    /// Test connectivity (GET /api/v3/ping).
    pub fn ping(&self) -> Result<(), BrokerError> {
        let url = format!("{}/api/v3/ping", self.base_url);
        let resp = self
            .client
            .get(&url)
            .send()
            .map_err(|e| BrokerError::Connection(format!("ping failed: {e}")))?;

        if !resp.status().is_success() {
            return Err(BrokerError::Connection(format!(
                "ping returned {}",
                resp.status()
            )));
        }
        Ok(())
    }

    /// Get account information (GET /api/v3/account).
    pub fn account_info(&self) -> Result<AccountInfo, BrokerError> {
        let timestamp = current_timestamp_ms();
        let query = format!("timestamp={timestamp}");
        let signature = auth::sign(&query, &self.secret_key);
        let url = format!(
            "{}/api/v3/account?{query}&signature={signature}",
            self.base_url
        );

        let resp = self
            .client
            .get(&url)
            .header("X-MBX-APIKEY", &self.api_key)
            .send()
            .map_err(|e| BrokerError::Connection(format!("account request failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            return Err(BrokerError::Connection(format!(
                "account returned {status}: {body}"
            )));
        }

        resp.json::<AccountInfo>()
            .map_err(|e| BrokerError::Connection(format!("failed to parse account: {e}")))
    }

    /// Submit a new order (POST /api/v3/order).
    pub fn submit_order(
        &self,
        symbol: &str,
        side: &str,
        order_type: &str,
        quantity: &str,
        price: Option<&str>,
        time_in_force: Option<&str>,
    ) -> Result<OrderResponse, BrokerError> {
        let timestamp = current_timestamp_ms();
        let mut query = format!(
            "symbol={symbol}&side={side}&type={order_type}&quantity={quantity}&timestamp={timestamp}"
        );
        if let Some(p) = price {
            query.push_str(&format!("&price={p}"));
        }
        if let Some(tif) = time_in_force {
            query.push_str(&format!("&timeInForce={tif}"));
        }

        let signature = auth::sign(&query, &self.secret_key);
        let url = format!("{}/api/v3/order", self.base_url);

        debug!("Submitting Binance order: {symbol} {side} qty={quantity}");

        let resp = self
            .client
            .post(&url)
            .header("X-MBX-APIKEY", &self.api_key)
            .body(format!("{query}&signature={signature}"))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .send()
            .map_err(|e| BrokerError::Order(format!("order request failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            return Err(BrokerError::Order(format!(
                "order returned {status}: {body}"
            )));
        }

        resp.json::<OrderResponse>()
            .map_err(|e| BrokerError::Order(format!("failed to parse order response: {e}")))
    }

    /// Get order status (GET /api/v3/order).
    pub fn order_status(&self, symbol: &str, order_id: u64) -> Result<OrderResponse, BrokerError> {
        let timestamp = current_timestamp_ms();
        let query = format!("symbol={symbol}&orderId={order_id}&timestamp={timestamp}");
        let signature = auth::sign(&query, &self.secret_key);
        let url = format!(
            "{}/api/v3/order?{query}&signature={signature}",
            self.base_url
        );

        let resp = self
            .client
            .get(&url)
            .header("X-MBX-APIKEY", &self.api_key)
            .send()
            .map_err(|e| BrokerError::Order(format!("order status request failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            return Err(BrokerError::Order(format!(
                "order status returned {status}: {body}"
            )));
        }

        resp.json::<OrderResponse>()
            .map_err(|e| BrokerError::Order(format!("failed to parse order status: {e}")))
    }

    /// Cancel an order (DELETE /api/v3/order).
    pub fn cancel_order(&self, symbol: &str, order_id: u64) -> Result<(), BrokerError> {
        let timestamp = current_timestamp_ms();
        let query = format!("symbol={symbol}&orderId={order_id}&timestamp={timestamp}");
        let signature = auth::sign(&query, &self.secret_key);
        let url = format!(
            "{}/api/v3/order?{query}&signature={signature}",
            self.base_url
        );

        let resp = self
            .client
            .delete(&url)
            .header("X-MBX-APIKEY", &self.api_key)
            .send()
            .map_err(|e| BrokerError::Order(format!("cancel request failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            return Err(BrokerError::Order(format!(
                "cancel returned {status}: {body}"
            )));
        }

        Ok(())
    }

    /// Get book ticker (best bid/ask) for a symbol (GET /api/v3/ticker/bookTicker).
    pub fn book_ticker(&self, symbol: &str) -> Result<BookTicker, BrokerError> {
        let url = format!("{}/api/v3/ticker/bookTicker?symbol={symbol}", self.base_url);

        let resp = self
            .client
            .get(&url)
            .send()
            .map_err(|e| BrokerError::Connection(format!("ticker request failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            return Err(BrokerError::Connection(format!(
                "ticker returned {status}: {body}"
            )));
        }

        resp.json::<BookTicker>()
            .map_err(|e| BrokerError::Connection(format!("failed to parse ticker: {e}")))
    }
}

/// Current timestamp in milliseconds.
fn current_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis() as u64
}
