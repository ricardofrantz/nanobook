//! PyO3 bindings for the broker crate.

use nanobook_broker::Broker;
use nanobook_broker::ibkr::IbkrBroker as RustIbkrBroker;
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;

use crate::types::parse_symbol;

/// Interactive Brokers connection.
///
/// Args:
///     host: TWS/Gateway hostname (e.g., "127.0.0.1")
///     port: TWS/Gateway port (e.g., 4002 for paper, 4001 for live)
///     client_id: Unique client ID for this connection
///
/// Example::
///
///     broker = IbkrBroker("127.0.0.1", 4002, 100)
///     broker.connect()
///     positions = broker.positions()
///     broker.disconnect()
///
#[pyclass(name = "IbkrBroker")]
pub struct PyIbkrBroker {
    inner: RustIbkrBroker,
}

#[pymethods]
impl PyIbkrBroker {
    #[new]
    fn new(host: &str, port: u16, client_id: i32) -> Self {
        Self {
            inner: RustIbkrBroker::new(host, port, client_id),
        }
    }

    /// Connect to IB Gateway/TWS.
    fn connect(&mut self) -> PyResult<()> {
        self.inner
            .connect()
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))
    }

    /// Disconnect from IB Gateway/TWS.
    fn disconnect(&mut self) -> PyResult<()> {
        self.inner
            .disconnect()
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))
    }

    /// Get all current positions.
    ///
    /// Returns list of dicts with keys: symbol, quantity, avg_cost_cents,
    /// market_value_cents, unrealized_pnl_cents.
    fn positions(&self, py: Python<'_>) -> PyResult<PyObject> {
        let positions = self
            .inner
            .positions()
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;

        let list = pyo3::types::PyList::empty(py);
        for pos in positions {
            let dict = pyo3::types::PyDict::new(py);
            dict.set_item("symbol", pos.symbol.as_str())?;
            dict.set_item("quantity", pos.quantity)?;
            dict.set_item("avg_cost_cents", pos.avg_cost_cents)?;
            dict.set_item("market_value_cents", pos.market_value_cents)?;
            dict.set_item("unrealized_pnl_cents", pos.unrealized_pnl_cents)?;
            list.append(dict)?;
        }
        Ok(list.into())
    }

    /// Get account summary.
    ///
    /// Returns dict with keys: equity_cents, buying_power_cents, cash_cents,
    /// gross_position_value_cents.
    fn account(&self, py: Python<'_>) -> PyResult<PyObject> {
        let account = self
            .inner
            .account()
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;

        let dict = pyo3::types::PyDict::new(py);
        dict.set_item("equity_cents", account.equity_cents)?;
        dict.set_item("buying_power_cents", account.buying_power_cents)?;
        dict.set_item("cash_cents", account.cash_cents)?;
        dict.set_item(
            "gross_position_value_cents",
            account.gross_position_value_cents,
        )?;
        Ok(dict.into())
    }

    /// Submit an order.
    ///
    /// Args:
    ///     symbol: Ticker symbol (e.g., "AAPL")
    ///     side: "buy" or "sell"
    ///     quantity: Number of shares
    ///     order_type: "market" or "limit"
    ///     limit_price_cents: Price in cents (required for limit orders)
    ///
    /// Returns the broker-assigned order ID.
    #[pyo3(signature = (symbol, side, quantity, order_type="market", limit_price_cents=None))]
    fn submit_order(
        &self,
        symbol: &str,
        side: &str,
        quantity: u64,
        order_type: &str,
        limit_price_cents: Option<i64>,
    ) -> PyResult<u64> {
        let sym = parse_symbol(symbol)?;

        let broker_side = match side.to_ascii_lowercase().as_str() {
            "buy" | "b" => nanobook_broker::BrokerSide::Buy,
            "sell" | "s" => nanobook_broker::BrokerSide::Sell,
            _ => {
                return Err(pyo3::exceptions::PyValueError::new_err(format!(
                    "Invalid side '{side}'. Use 'buy' or 'sell'."
                )));
            }
        };

        let broker_order_type = match order_type.to_ascii_lowercase().as_str() {
            "market" => nanobook_broker::BrokerOrderType::Market,
            "limit" => {
                let price = limit_price_cents.ok_or_else(|| {
                    pyo3::exceptions::PyValueError::new_err(
                        "limit_price_cents required for limit orders",
                    )
                })?;
                nanobook_broker::BrokerOrderType::Limit(nanobook::Price(price))
            }
            _ => {
                return Err(pyo3::exceptions::PyValueError::new_err(format!(
                    "Invalid order_type '{order_type}'. Use 'market' or 'limit'."
                )));
            }
        };

        let order = nanobook_broker::BrokerOrder {
            symbol: sym,
            side: broker_side,
            quantity,
            order_type: broker_order_type,
        };

        let id = self
            .inner
            .submit_order(&order)
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;

        Ok(id.0)
    }

    /// Get status of a submitted order.
    ///
    /// Returns dict with keys: id, status, filled_quantity,
    /// remaining_quantity, avg_fill_price_cents.
    fn order_status(&self, py: Python<'_>, order_id: u64) -> PyResult<PyObject> {
        let id = nanobook_broker::OrderId(order_id);
        let status = self
            .inner
            .order_status(id)
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;

        let dict = pyo3::types::PyDict::new(py);
        dict.set_item("id", status.id.0)?;
        dict.set_item("status", format!("{:?}", status.status))?;
        dict.set_item("filled_quantity", status.filled_quantity)?;
        dict.set_item("remaining_quantity", status.remaining_quantity)?;
        dict.set_item("avg_fill_price_cents", status.avg_fill_price_cents)?;
        Ok(dict.into())
    }

    /// Cancel a pending order.
    fn cancel_order(&self, order_id: u64) -> PyResult<()> {
        let id = nanobook_broker::OrderId(order_id);
        self.inner
            .cancel_order(id)
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))
    }

    /// Get current quote for a symbol.
    ///
    /// Returns dict with keys: symbol, bid_cents, ask_cents, last_cents, volume.
    fn quote(&self, py: Python<'_>, symbol: &str) -> PyResult<PyObject> {
        let sym = parse_symbol(symbol)?;
        let quote = self
            .inner
            .quote(&sym)
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;

        let dict = pyo3::types::PyDict::new(py);
        dict.set_item("symbol", quote.symbol.as_str())?;
        dict.set_item("bid_cents", quote.bid_cents)?;
        dict.set_item("ask_cents", quote.ask_cents)?;
        dict.set_item("last_cents", quote.last_cents)?;
        dict.set_item("volume", quote.volume)?;
        Ok(dict.into())
    }

    fn __repr__(&self) -> String {
        "IbkrBroker(...)".to_string()
    }
}

#[cfg(feature = "binance")]
mod binance_binding {
    use super::*;

    /// Binance spot broker.
    ///
    /// Args:
    ///     api_key: Binance API key
    ///     secret_key: Binance secret key
    ///     testnet: Use Binance testnet if True
    ///
    /// Example::
    ///
    ///     broker = BinanceBroker("key", "secret", testnet=True)
    ///     broker.connect()
    ///     quote = broker.quote("BTC")
    ///     broker.disconnect()
    ///
    #[pyclass(name = "BinanceBroker")]
    pub struct PyBinanceBroker {
        inner: nanobook_broker::binance::BinanceBroker,
    }

    #[pymethods]
    impl PyBinanceBroker {
        #[new]
        #[pyo3(signature = (api_key, secret_key, testnet=false, quote_asset="USDT"))]
        fn new(api_key: &str, secret_key: &str, testnet: bool, quote_asset: &str) -> Self {
            Self {
                inner: nanobook_broker::binance::BinanceBroker::new(api_key, secret_key, testnet)
                    .with_quote_asset(quote_asset),
            }
        }

        /// Connect to Binance (sends a ping).
        fn connect(&mut self) -> PyResult<()> {
            self.inner
                .connect()
                .map_err(|e| PyRuntimeError::new_err(e.to_string()))
        }

        /// Disconnect (no-op for REST, clears internal client).
        fn disconnect(&mut self) -> PyResult<()> {
            self.inner
                .disconnect()
                .map_err(|e| PyRuntimeError::new_err(e.to_string()))
        }

        /// Get all positions (non-zero balances).
        fn positions(&self, py: Python<'_>) -> PyResult<PyObject> {
            let positions = self
                .inner
                .positions()
                .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;

            let list = pyo3::types::PyList::empty(py);
            for pos in positions {
                let dict = pyo3::types::PyDict::new(py);
                dict.set_item("symbol", pos.symbol.as_str())?;
                dict.set_item("quantity", pos.quantity)?;
                dict.set_item("avg_cost_cents", pos.avg_cost_cents)?;
                dict.set_item("market_value_cents", pos.market_value_cents)?;
                dict.set_item("unrealized_pnl_cents", pos.unrealized_pnl_cents)?;
                list.append(dict)?;
            }
            Ok(list.into())
        }

        /// Get account summary (USDT balance).
        fn account(&self, py: Python<'_>) -> PyResult<PyObject> {
            let account = self
                .inner
                .account()
                .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;

            let dict = pyo3::types::PyDict::new(py);
            dict.set_item("equity_cents", account.equity_cents)?;
            dict.set_item("buying_power_cents", account.buying_power_cents)?;
            dict.set_item("cash_cents", account.cash_cents)?;
            dict.set_item(
                "gross_position_value_cents",
                account.gross_position_value_cents,
            )?;
            Ok(dict.into())
        }

        /// Submit an order.
        #[pyo3(signature = (symbol, side, quantity, order_type="market", limit_price_cents=None))]
        fn submit_order(
            &self,
            symbol: &str,
            side: &str,
            quantity: u64,
            order_type: &str,
            limit_price_cents: Option<i64>,
        ) -> PyResult<u64> {
            let sym = parse_symbol(symbol)?;

            let broker_side = match side.to_ascii_lowercase().as_str() {
                "buy" | "b" => nanobook_broker::BrokerSide::Buy,
                "sell" | "s" => nanobook_broker::BrokerSide::Sell,
                _ => {
                    return Err(pyo3::exceptions::PyValueError::new_err(format!(
                        "Invalid side '{side}'. Use 'buy' or 'sell'."
                    )));
                }
            };

            let broker_order_type = match order_type.to_ascii_lowercase().as_str() {
                "market" => nanobook_broker::BrokerOrderType::Market,
                "limit" => {
                    let price = limit_price_cents.ok_or_else(|| {
                        pyo3::exceptions::PyValueError::new_err(
                            "limit_price_cents required for limit orders",
                        )
                    })?;
                    nanobook_broker::BrokerOrderType::Limit(nanobook::Price(price))
                }
                _ => {
                    return Err(pyo3::exceptions::PyValueError::new_err(format!(
                        "Invalid order_type '{order_type}'. Use 'market' or 'limit'."
                    )));
                }
            };

            let order = nanobook_broker::BrokerOrder {
                symbol: sym,
                side: broker_side,
                quantity,
                order_type: broker_order_type,
            };

            let id = self
                .inner
                .submit_order(&order)
                .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;

            Ok(id.0)
        }

        /// Get order status.
        fn order_status(&self, py: Python<'_>, order_id: u64) -> PyResult<PyObject> {
            let id = nanobook_broker::OrderId(order_id);
            let status = self
                .inner
                .order_status(id)
                .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;

            let dict = pyo3::types::PyDict::new(py);
            dict.set_item("id", status.id.0)?;
            dict.set_item("status", format!("{:?}", status.status))?;
            dict.set_item("filled_quantity", status.filled_quantity)?;
            dict.set_item("remaining_quantity", status.remaining_quantity)?;
            dict.set_item("avg_fill_price_cents", status.avg_fill_price_cents)?;
            Ok(dict.into())
        }

        /// Cancel an order.
        fn cancel_order(&self, order_id: u64) -> PyResult<()> {
            let id = nanobook_broker::OrderId(order_id);
            self.inner
                .cancel_order(id)
                .map_err(|e| PyRuntimeError::new_err(e.to_string()))
        }

        /// Get current quote for a symbol (e.g., "BTC").
        fn quote(&self, py: Python<'_>, symbol: &str) -> PyResult<PyObject> {
            let sym = parse_symbol(symbol)?;
            let quote = self
                .inner
                .quote(&sym)
                .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;

            let dict = pyo3::types::PyDict::new(py);
            dict.set_item("symbol", quote.symbol.as_str())?;
            dict.set_item("bid_cents", quote.bid_cents)?;
            dict.set_item("ask_cents", quote.ask_cents)?;
            dict.set_item("last_cents", quote.last_cents)?;
            dict.set_item("volume", quote.volume)?;
            Ok(dict.into())
        }

        fn __repr__(&self) -> String {
            "BinanceBroker(...)".to_string()
        }
    }
} // mod binance_binding

#[cfg(feature = "binance")]
pub use binance_binding::PyBinanceBroker;
