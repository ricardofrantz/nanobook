//! Multi-symbol exchange: one LOB per symbol.

use crate::{Exchange, Price, Symbol};
use rustc_hash::FxHashMap;

/// A collection of per-symbol `Exchange` instances.
///
/// Each symbol gets its own independent order book. This is the entry point
/// for multi-asset simulations.
///
/// ```
/// use nanobook::{MultiExchange, Symbol, Side, Price, TimeInForce};
///
/// let mut multi = MultiExchange::new();
///
/// let aapl = Symbol::new("AAPL");
/// let msft = Symbol::new("MSFT");
///
/// // Orders are routed to per-symbol books
/// multi.get_or_create(&aapl).submit_limit(Side::Sell, Price(150_00), 100, TimeInForce::GTC);
/// multi.get_or_create(&msft).submit_limit(Side::Sell, Price(300_00), 200, TimeInForce::GTC);
///
/// assert_eq!(multi.get(&aapl).unwrap().best_ask(), Some(Price(150_00)));
/// assert_eq!(multi.get(&msft).unwrap().best_ask(), Some(Price(300_00)));
/// ```
#[derive(Clone, Debug, Default)]
pub struct MultiExchange {
    exchanges: FxHashMap<Symbol, Exchange>,
}

impl MultiExchange {
    /// Create an empty multi-exchange.
    pub fn new() -> Self {
        Self::default()
    }

    /// Get or create the exchange for a symbol.
    pub fn get_or_create(&mut self, symbol: &Symbol) -> &mut Exchange {
        self.exchanges.entry(*symbol).or_default()
    }

    /// Get a reference to the exchange for a symbol, if it exists.
    pub fn get(&self, symbol: &Symbol) -> Option<&Exchange> {
        self.exchanges.get(symbol)
    }

    /// Get a mutable reference to the exchange for a symbol, if it exists.
    pub fn get_mut(&mut self, symbol: &Symbol) -> Option<&mut Exchange> {
        self.exchanges.get_mut(symbol)
    }

    /// Iterator over all symbols that have exchanges.
    pub fn symbols(&self) -> impl Iterator<Item = &Symbol> {
        self.exchanges.keys()
    }

    /// Number of symbols.
    pub fn len(&self) -> usize {
        self.exchanges.len()
    }

    /// Returns true if no exchanges exist.
    pub fn is_empty(&self) -> bool {
        self.exchanges.is_empty()
    }

    /// Get the best bid and ask for all symbols.
    pub fn best_prices(&self) -> Vec<(Symbol, Option<Price>, Option<Price>)> {
        self.exchanges
            .iter()
            .map(|(sym, ex)| {
                let (bid, ask) = ex.best_bid_ask();
                (*sym, bid, ask)
            })
            .collect()
    }
}

#[cfg(test)]
#[allow(clippy::inconsistent_digit_grouping)]
mod tests {
    use super::*;
    use crate::{Side, TimeInForce};

    fn aapl() -> Symbol {
        Symbol::new("AAPL")
    }
    fn msft() -> Symbol {
        Symbol::new("MSFT")
    }

    #[test]
    fn create_and_query() {
        let mut multi = MultiExchange::new();
        multi
            .get_or_create(&aapl())
            .submit_limit(Side::Sell, Price(150_00), 100, TimeInForce::GTC);

        assert_eq!(multi.get(&aapl()).unwrap().best_ask(), Some(Price(150_00)));
        assert!(multi.get(&msft()).is_none());
    }

    #[test]
    fn multiple_symbols() {
        let mut multi = MultiExchange::new();
        multi
            .get_or_create(&aapl())
            .submit_limit(Side::Sell, Price(150_00), 100, TimeInForce::GTC);
        multi
            .get_or_create(&msft())
            .submit_limit(Side::Sell, Price(300_00), 200, TimeInForce::GTC);

        assert_eq!(multi.len(), 2);
        let prices = multi.best_prices();
        assert_eq!(prices.len(), 2);
    }

    #[test]
    fn symbols_iter() {
        let mut multi = MultiExchange::new();
        multi.get_or_create(&aapl());
        multi.get_or_create(&msft());

        let syms: Vec<&Symbol> = multi.symbols().collect();
        assert_eq!(syms.len(), 2);
    }

    #[test]
    fn empty() {
        let multi = MultiExchange::new();
        assert!(multi.is_empty());
        assert_eq!(multi.len(), 0);
    }

    #[test]
    fn independent_books() {
        let mut multi = MultiExchange::new();

        // Trade on AAPL
        multi
            .get_or_create(&aapl())
            .submit_limit(Side::Sell, Price(150_00), 100, TimeInForce::GTC);
        multi
            .get_or_create(&aapl())
            .submit_limit(Side::Buy, Price(150_00), 100, TimeInForce::GTC);

        // MSFT is untouched
        multi
            .get_or_create(&msft())
            .submit_limit(Side::Sell, Price(300_00), 200, TimeInForce::GTC);

        assert_eq!(multi.get(&aapl()).unwrap().trades().len(), 1);
        assert_eq!(multi.get(&msft()).unwrap().trades().len(), 0);
    }
}
