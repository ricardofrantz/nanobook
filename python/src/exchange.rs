use nanobook::{Event, Exchange, OrderId, Price, TrailMethod};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyDict;

use crate::event::PyEvent;
use crate::order::PyOrder;
use crate::results::*;
use crate::types::{parse_side, parse_tif, price_to_float, side_str};

/// A limit order book exchange.
///
/// This is the main interface for interacting with the order book.
/// All prices are in cents (integer). Use `price_float` getters on
/// results for dollar values.
///
/// Example::
///
///     ex = Exchange()
///     result = ex.submit_limit("buy", 10050, 100, "gtc")
///     print(result)  # SubmitResult(order_id=1, status='New', ...)
///
#[pyclass(name = "Exchange")]
pub struct PyExchange {
    inner: Exchange,
}

impl PyExchange {
    pub fn from_exchange(exchange: Exchange) -> Self {
        Self { inner: exchange }
    }
}

#[pymethods]
impl PyExchange {
    #[new]
    fn new() -> Self {
        Self {
            inner: Exchange::new(),
        }
    }

    /// Replay events to reconstruct exchange state.
    #[staticmethod]
    fn replay(events: Vec<PyEvent>) -> Self {
        let inner_events: Vec<Event> = events.into_iter().map(|e| e.inner).collect();
        Self {
            inner: Exchange::replay(&inner_events),
        }
    }

    // === Order Submission ===

    /// Submit a limit order.
    ///
    /// Args:
    ///     side: "buy" or "sell"
    ///     price: Price in cents (e.g., 10050 = $100.50)
    ///     quantity: Number of shares
    ///     tif: Time-in-force: "gtc", "ioc", or "fok"
    ///
    /// Returns:
    ///     SubmitResult with order_id, status, trades, and fill details
    #[pyo3(signature = (side, price, quantity, tif="gtc"))]
    fn submit_limit(
        &mut self,
        side: &str,
        price: i64,
        quantity: u64,
        tif: &str,
    ) -> PyResult<PySubmitResult> {
        let side = parse_side(side)?;
        let tif = parse_tif(tif)?;
        Ok(self
            .inner
            .submit_limit(side, Price(price), quantity, tif)
            .into())
    }

    /// Submit a market order.
    ///
    /// Args:
    ///     side: "buy" or "sell"
    ///     quantity: Number of shares
    ///
    /// Returns:
    ///     SubmitResult with fill details
    fn submit_market(&mut self, side: &str, quantity: u64) -> PyResult<PySubmitResult> {
        let side = parse_side(side)?;
        Ok(self.inner.submit_market(side, quantity).into())
    }

    // === Order Management ===

    /// Cancel an order by ID.
    fn cancel(&mut self, order_id: u64) -> PyCancelResult {
        self.inner.cancel(OrderId(order_id)).into()
    }

    /// Modify an order (cancel and replace).
    ///
    /// The new order gets a new ID and loses time priority.
    fn modify(&mut self, order_id: u64, new_price: i64, new_quantity: u64) -> PyModifyResult {
        self.inner
            .modify(OrderId(order_id), Price(new_price), new_quantity)
            .into()
    }

    // === Stop Orders ===

    /// Submit a stop-market order.
    ///
    /// Triggers when trade price reaches stop_price, then becomes a market order.
    fn submit_stop_market(
        &mut self,
        side: &str,
        stop_price: i64,
        quantity: u64,
    ) -> PyResult<PyStopSubmitResult> {
        let side = parse_side(side)?;
        Ok(self
            .inner
            .submit_stop_market(side, Price(stop_price), quantity)
            .into())
    }

    /// Submit a stop-limit order.
    #[pyo3(signature = (side, stop_price, limit_price, quantity, tif="gtc"))]
    fn submit_stop_limit(
        &mut self,
        side: &str,
        stop_price: i64,
        limit_price: i64,
        quantity: u64,
        tif: &str,
    ) -> PyResult<PyStopSubmitResult> {
        let side = parse_side(side)?;
        let tif = parse_tif(tif)?;
        Ok(self
            .inner
            .submit_stop_limit(side, Price(stop_price), Price(limit_price), quantity, tif)
            .into())
    }

    /// Submit a trailing stop-market order.
    ///
    /// Args:
    ///     side: "buy" or "sell"
    ///     initial_stop_price: Starting stop price in cents
    ///     quantity: Number of shares
    ///     trail_type: "fixed", "percentage", or "atr"
    ///     trail_value: Offset in cents (fixed), fraction (percentage), or multiplier (atr)
    ///     atr_period: ATR lookback period (only for trail_type="atr")
    #[pyo3(signature = (side, initial_stop_price, quantity, trail_type, trail_value, atr_period=None))]
    fn submit_trailing_stop_market(
        &mut self,
        side: &str,
        initial_stop_price: i64,
        quantity: u64,
        trail_type: &str,
        trail_value: f64,
        atr_period: Option<usize>,
    ) -> PyResult<PyStopSubmitResult> {
        let side = parse_side(side)?;
        let method = parse_trail_method(trail_type, trail_value, atr_period)?;
        Ok(self
            .inner
            .submit_trailing_stop_market(side, Price(initial_stop_price), quantity, method)
            .into())
    }

    /// Submit a trailing stop-limit order.
    #[allow(clippy::too_many_arguments)]
    #[pyo3(signature = (side, initial_stop_price, limit_price, quantity, trail_type, trail_value, tif="gtc", atr_period=None))]
    fn submit_trailing_stop_limit(
        &mut self,
        side: &str,
        initial_stop_price: i64,
        limit_price: i64,
        quantity: u64,
        trail_type: &str,
        trail_value: f64,
        tif: &str,
        atr_period: Option<usize>,
    ) -> PyResult<PyStopSubmitResult> {
        let side = parse_side(side)?;
        let tif = parse_tif(tif)?;
        let method = parse_trail_method(trail_type, trail_value, atr_period)?;
        Ok(self
            .inner
            .submit_trailing_stop_limit(
                side,
                Price(initial_stop_price),
                Price(limit_price),
                quantity,
                tif,
                method,
            )
            .into())
    }

    // === Queries ===

    /// Get an order by ID.
    fn get_order(&self, order_id: u64) -> Option<PyOrder> {
        self.inner
            .get_order(OrderId(order_id))
            .map(|o| PyOrder { inner: o.clone() })
    }

    /// Get a stop order by ID.
    fn get_stop_order(&self, py: Python<'_>, order_id: u64) -> PyResult<Option<PyObject>> {
        if let Some(stop) = self.inner.get_stop_order(OrderId(order_id)) {
            let dict = PyDict::new(py);
            dict.set_item("id", stop.id.0)?;
            dict.set_item("side", side_str(stop.side))?;
            dict.set_item("stop_price", stop.stop_price.0)?;
            dict.set_item("limit_price", stop.limit_price.map(|p| p.0))?;
            dict.set_item("quantity", stop.quantity)?;
            dict.set_item("status", format!("{:?}", stop.status).to_lowercase())?;
            dict.set_item("timestamp", stop.timestamp)?;
            Ok(Some(dict.into()))
        } else {
            Ok(None)
        }
    }

    /// Get the best bid and ask prices as (bid, ask) tuple.
    /// Returns None for sides with no orders.
    fn best_bid_ask(&self) -> (Option<i64>, Option<i64>) {
        let (bid, ask) = self.inner.best_bid_ask();
        (bid.map(|p| p.0), ask.map(|p| p.0))
    }

    /// Get the best bid price, or None.
    fn best_bid(&self) -> Option<i64> {
        self.inner.best_bid().map(|p| p.0)
    }

    /// Get the best ask price, or None.
    fn best_ask(&self) -> Option<i64> {
        self.inner.best_ask().map(|p| p.0)
    }

    /// Get the spread (best_ask - best_bid) in cents, or None.
    fn spread(&self) -> Option<i64> {
        self.inner.spread()
    }

    /// Get the last trade price, or None.
    fn last_trade_price(&self) -> Option<i64> {
        self.inner.last_trade_price().map(|p| p.0)
    }

    /// Get all trades.
    fn trades(&self) -> Vec<PyTrade> {
        self.inner.trades().iter().cloned().map(PyTrade::from).collect()
    }

    /// Get recorded events.
    fn events(&self) -> Vec<PyEvent> {
        self.inner
            .events()
            .iter()
            .cloned()
            .map(|e| PyEvent { inner: e })
            .collect()
    }

    /// Get a depth snapshot of the book (top N levels each side).
    #[pyo3(signature = (levels=10))]
    fn depth(&self, levels: usize) -> PyBookSnapshot {
        let snap = self.inner.depth(levels);
        PyBookSnapshot::from_snapshot(&snap)
    }

    /// Get a full snapshot of the book.
    fn full_book(&self) -> PyBookSnapshot {
        let snap = self.inner.full_book();
        PyBookSnapshot::from_snapshot(&snap)
    }

    /// Number of pending stop orders.
    fn pending_stop_count(&self) -> usize {
        self.inner.pending_stop_count()
    }

    // === Memory Management ===

    /// Clear trade history to free memory.
    fn clear_trades(&mut self) {
        self.inner.clear_trades();
    }

    /// Clear filled/cancelled order history. Returns count removed.
    fn clear_order_history(&mut self) -> usize {
        self.inner.clear_order_history()
    }

    fn __repr__(&self) -> String {
        let (bid, ask) = self.inner.best_bid_ask();
        let fmt_price = |p: nanobook::Price| format!("${:.2}", price_to_float(p));
        let bid_str = bid.map(fmt_price).unwrap_or_else(|| "None".to_string());
        let ask_str = ask.map(fmt_price).unwrap_or_else(|| "None".to_string());
        format!(
            "Exchange(bid={}, ask={}, trades={})",
            bid_str,
            ask_str,
            self.inner.trades().len()
        )
    }
}

/// Book depth snapshot.
#[pyclass(name = "BookSnapshot")]
#[derive(Clone)]
pub struct PyBookSnapshot {
    inner: nanobook::BookSnapshot,
    bids: Vec<PyLevelSnapshot>,
    asks: Vec<PyLevelSnapshot>,
}

#[pymethods]
impl PyBookSnapshot {
    #[getter]
    fn bids(&self) -> Vec<PyLevelSnapshot> {
        self.bids.clone()
    }

    #[getter]
    fn asks(&self) -> Vec<PyLevelSnapshot> {
        self.asks.clone()
    }

    /// Book imbalance: (bid_qty - ask_qty) / (bid_qty + ask_qty).
    fn imbalance(&self) -> Option<f64> {
        self.inner.imbalance()
    }

    /// Volume-weighted midpoint price.
    fn weighted_mid(&self) -> Option<f64> {
        self.inner.weighted_mid()
    }

    /// Mid price: (best_bid + best_ask) / 2.
    fn mid_price(&self) -> Option<f64> {
        self.inner.mid_price()
    }

    /// Spread: best_ask - best_bid.
    fn spread(&self) -> Option<i64> {
        self.inner.spread()
    }

    fn __repr__(&self) -> String {
        format!(
            "BookSnapshot(bids={}, asks={})",
            self.bids.len(),
            self.asks.len()
        )
    }
}

impl PyBookSnapshot {
    pub fn from_snapshot(snap: &nanobook::BookSnapshot) -> Self {
        fn convert_levels(levels: &[nanobook::LevelSnapshot]) -> Vec<PyLevelSnapshot> {
            levels
                .iter()
                .map(|l| PyLevelSnapshot {
                    price: l.price.0,
                    quantity: l.quantity,
                    order_count: l.order_count,
                })
                .collect()
        }

        Self {
            inner: snap.clone(),
            bids: convert_levels(&snap.bids),
            asks: convert_levels(&snap.asks),
        }
    }
}

/// Parse trail method from Python arguments.
fn parse_trail_method(
    trail_type: &str,
    trail_value: f64,
    atr_period: Option<usize>,
) -> PyResult<TrailMethod> {
    match trail_type.to_ascii_lowercase().as_str() {
        "fixed" => Ok(TrailMethod::Fixed(trail_value as i64)),
        "percentage" | "pct" => Ok(TrailMethod::Percentage(trail_value)),
        "atr" => {
            let period = atr_period.ok_or_else(|| {
                PyValueError::new_err("atr_period is required when trail_type='atr'")
            })?;
            Ok(TrailMethod::Atr {
                multiplier: trail_value,
                period,
            })
        }
        _ => Err(PyValueError::new_err(format!(
            "Invalid trail_type '{trail_type}'. Use 'fixed', 'percentage', or 'atr'."
        ))),
    }
}
