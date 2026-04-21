//! IBKR market-data helpers.

use crate::types::{BestQuote, Quote};

/// Extract a usable NBBO quote from a broker quote snapshot.
pub(crate) fn best_quote_from_quote(quote: &Quote) -> Option<BestQuote> {
    if quote.bid_cents > 0 && quote.ask_cents > 0 {
        Some(BestQuote {
            bid_cents: quote.bid_cents,
            ask_cents: quote.ask_cents,
        })
    } else {
        None
    }
}
