//! Result types for Exchange operations.

use crate::stop::StopStatus;
use crate::{OrderId, OrderStatus, Quantity, Trade};

/// Result of submitting an order.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SubmitResult {
    /// The order ID assigned by the exchange
    pub order_id: OrderId,
    /// Final status of the order
    pub status: OrderStatus,
    /// Trades that occurred (if any)
    pub trades: Vec<Trade>,
    /// Quantity that was filled
    pub filled_quantity: Quantity,
    /// Quantity that is resting on the book (GTC only)
    pub resting_quantity: Quantity,
    /// Quantity that was cancelled (IOC remainder, FOK rejection)
    pub cancelled_quantity: Quantity,
}

impl SubmitResult {
    /// Returns true if any trades occurred.
    pub fn has_trades(&self) -> bool {
        !self.trades.is_empty()
    }

    /// Returns true if the order is resting on the book.
    pub fn is_resting(&self) -> bool {
        self.resting_quantity > 0
    }

    /// Returns true if the order was fully filled.
    pub fn is_fully_filled(&self) -> bool {
        self.status == OrderStatus::Filled
    }
}

/// Result of cancelling an order.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CancelResult {
    /// Whether the cancellation succeeded
    pub success: bool,
    /// Quantity that was cancelled (0 if failed)
    pub cancelled_quantity: Quantity,
    /// Error if cancellation failed
    pub error: Option<CancelError>,
}

impl CancelResult {
    /// Create a successful cancel result.
    pub fn success(cancelled_quantity: Quantity) -> Self {
        Self {
            success: true,
            cancelled_quantity,
            error: None,
        }
    }

    /// Create a failed cancel result.
    pub fn failure(error: CancelError) -> Self {
        Self {
            success: false,
            cancelled_quantity: 0,
            error: Some(error),
        }
    }
}

/// Errors that can occur when cancelling an order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum CancelError {
    /// Order ID not found
    OrderNotFound,
    /// Order already filled or cancelled
    OrderNotActive,
}

/// Result of modifying an order.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ModifyResult {
    /// Whether the modification succeeded
    pub success: bool,
    /// The old order ID (always set)
    pub old_order_id: OrderId,
    /// The new order ID (if successful)
    pub new_order_id: Option<OrderId>,
    /// Quantity cancelled from old order
    pub cancelled_quantity: Quantity,
    /// Trades from the new order (if any)
    pub trades: Vec<Trade>,
    /// Error if modification failed
    pub error: Option<ModifyError>,
}

impl ModifyResult {
    /// Create a successful modify result.
    pub fn success(
        old_order_id: OrderId,
        new_order_id: OrderId,
        cancelled_quantity: Quantity,
        trades: Vec<Trade>,
    ) -> Self {
        Self {
            success: true,
            old_order_id,
            new_order_id: Some(new_order_id),
            cancelled_quantity,
            trades,
            error: None,
        }
    }

    /// Create a failed modify result.
    pub fn failure(old_order_id: OrderId, error: ModifyError) -> Self {
        Self {
            success: false,
            old_order_id,
            new_order_id: None,
            cancelled_quantity: 0,
            trades: Vec::new(),
            error: Some(error),
        }
    }
}

/// Errors that can occur when modifying an order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ModifyError {
    /// Order ID not found
    OrderNotFound,
    /// Order already filled or cancelled
    OrderNotActive,
    /// New quantity is zero
    InvalidQuantity,
}

/// Result of submitting a stop order.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct StopSubmitResult {
    /// The order ID assigned by the exchange.
    pub order_id: OrderId,
    /// Status of the stop order (Pending or Triggered if immediate).
    pub status: StopStatus,
}
