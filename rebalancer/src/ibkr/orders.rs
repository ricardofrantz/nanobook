//! Order submission, fill monitoring, rate limiting, and cancellation.

use std::thread;
use std::time::{Duration, Instant};

use ibapi::client::blocking::Client;
use ibapi::contracts::Contract;
use ibapi::orders::order_builder::limit_order;
use ibapi::orders::{Action as IbAction, CancelOrder, PlaceOrder};
use log::{debug, info, warn};
use nanobook::Symbol;

use crate::diff::{Action, RebalanceOrder};
use crate::error::{Error, Result};

/// Result of a single order execution.
#[derive(Debug, Clone)]
pub struct OrderResult {
    pub symbol: Symbol,
    pub order_id: i32,
    pub filled_shares: i64,
    pub avg_fill_price: f64,
    pub commission: f64,
    pub status: OrderOutcome,
}

/// How an order ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderOutcome {
    Filled,
    PartialFill,
    Cancelled,
    Failed,
}

/// Execute a single rebalance order via IBKR.
///
/// Submits a limit order, polls for fill status up to `timeout`, and cancels
/// if not filled within the timeout.
pub fn execute_order(
    client: &Client,
    order: &RebalanceOrder,
    timeout: Duration,
) -> Result<OrderResult> {
    let contract = Contract::stock(order.symbol.as_str()).build();

    let ib_action = match order.action {
        Action::Buy | Action::BuyCover => IbAction::Buy,
        Action::Sell | Action::SellShort => IbAction::Sell,
    };

    let limit_price = order.limit_price_cents as f64 / 100.0;
    let quantity = order.shares as f64;

    let ib_order = limit_order(ib_action, quantity, limit_price);

    let order_id = client
        .next_valid_order_id()
        .map_err(|e| Error::Order(format!("failed to get order id: {e}")))?;

    info!(
        "Submitting: {} {} {} @ ${:.2} (id={})",
        order.action, order.shares, order.symbol, limit_price, order_id
    );

    let subscription = client
        .place_order(order_id, &contract, &ib_order)
        .map_err(|e| Error::Order(format!("failed to place order {order_id}: {e}")))?;

    let start = Instant::now();
    let mut filled = 0.0_f64;
    let mut avg_price = 0.0_f64;
    let mut commission = 0.0_f64;
    let mut final_status = OrderOutcome::Failed;

    for response in subscription {
        if start.elapsed() > timeout {
            warn!("Order {order_id} timed out after {}s", timeout.as_secs());
            cancel_order(client, order_id);
            final_status = if filled > 0.0 {
                OrderOutcome::PartialFill
            } else {
                OrderOutcome::Cancelled
            };
            break;
        }

        match response {
            PlaceOrder::OrderStatus(status) => {
                debug!(
                    "Order {order_id} status: {} filled={} remaining={}",
                    status.status, status.filled, status.remaining
                );
                filled = status.filled;
                avg_price = status.average_fill_price;

                if status.status == "Filled" {
                    final_status = OrderOutcome::Filled;
                    break;
                } else if status.status == "Cancelled" {
                    final_status = if filled > 0.0 {
                        OrderOutcome::PartialFill
                    } else {
                        OrderOutcome::Cancelled
                    };
                    break;
                }
            }
            PlaceOrder::ExecutionData(exec) => {
                debug!(
                    "Execution: {} shares @ ${:.2}",
                    exec.execution.shares, exec.execution.price
                );
            }
            PlaceOrder::CommissionReport(comm) => {
                commission = comm.commission;
                debug!("Commission: ${:.4}", commission);
            }
            PlaceOrder::Message(notice) => {
                if notice.code < 0 || notice.code >= 2000 {
                    warn!("Order {order_id} error {}: {}", notice.code, notice.message);
                }
            }
            _ => {}
        }
    }

    let result = OrderResult {
        symbol: order.symbol,
        order_id,
        filled_shares: filled as i64,
        avg_fill_price: avg_price,
        commission,
        status: final_status,
    };

    info!(
        "Order {order_id}: {:?} — filled {} @ ${:.2}",
        final_status, result.filled_shares, avg_price
    );

    Ok(result)
}

/// Cancel an order by ID.
fn cancel_order(client: &Client, order_id: i32) {
    info!("Cancelling order {order_id}");
    match client.cancel_order(order_id, "") {
        Ok(subscription) => {
            for response in subscription {
                match response {
                    CancelOrder::OrderStatus(s) => {
                        debug!("Cancel status for {order_id}: {}", s.status);
                        if s.status == "Cancelled" {
                            break;
                        }
                    }
                    CancelOrder::Notice(notice) => {
                        debug!("Cancel notice for {order_id}: {}", notice.message);
                    }
                }
            }
        }
        Err(e) => {
            warn!("Failed to cancel order {order_id}: {e}");
        }
    }
}

/// Sleep for the rate-limit interval between orders.
pub fn rate_limit_delay(interval_ms: u64) {
    if interval_ms > 0 {
        thread::sleep(Duration::from_millis(interval_ms));
    }
}
