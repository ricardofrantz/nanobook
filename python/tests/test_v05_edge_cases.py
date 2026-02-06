import nanobook
import pytest

def test_exchange_replay_no_events():
    ex = nanobook.Exchange.replay([])
    assert ex.best_bid_ask() == (None, None)
    assert len(ex.trades()) == 0

def test_portfolio_empty_rebalance():
    p = nanobook.Portfolio(100_00, nanobook.CostModel.zero())
    p.rebalance_simple([], [])
    assert p.cash == 100_00
    assert len(p.positions()) == 0

def test_portfolio_rebalance_zero_equity():
    p = nanobook.Portfolio(0, nanobook.CostModel.zero())
    p.rebalance_simple([("AAPL", 1.0)], [("AAPL", 100_00)])
    assert p.cash == 0
    assert len(p.positions()) == 0

def test_multi_exchange_get_or_create_idempotency():
    multi = nanobook.MultiExchange()
    ex1 = multi.get_or_create("AAPL")
    ex2 = multi.get_or_create("AAPL")
    # They are copies, but should represent same state (empty)
    assert ex1.best_bid() == ex2.best_bid()

def test_exchange_modify_terminal_order():
    ex = nanobook.Exchange()
    res = ex.submit_limit("buy", 10000, 100)
    ex.cancel(1)
    # Trying to modify a cancelled order
    res2 = ex.modify(1, 10100, 200)
    assert not res2.success
    assert "OrderNotActive" in res2.error

def test_exchange_cancel_terminal_order():
    ex = nanobook.Exchange()
    res = ex.submit_limit("buy", 10000, 100)
    ex.cancel(1)
    res2 = ex.cancel(1)
    assert not res2.success
    assert "OrderNotActive" in res2.error

def test_portfolio_positions_iter():
    p = nanobook.Portfolio(1000_00, nanobook.CostModel.zero())
    p.rebalance_simple([("AAPL", 0.5), ("MSFT", 0.5)], [("AAPL", 10_00), ("MSFT", 20_00)])
    positions = p.positions()
    assert len(positions) == 2
    assert "AAPL" in positions
    assert "MSFT" in positions

def test_strategy_empty_returns():
    def strat(i, p, port): return []
    res = nanobook.run_backtest(strat, [], 100_00, nanobook.CostModel.zero())
    assert res.metrics is None
    assert len(res.portfolio.returns()) == 0

def test_book_snapshot_imbalance_extreme():
    ex = nanobook.Exchange()
    ex.submit_limit("buy", 10000, 1000)
    snap = ex.full_book()
    assert snap.imbalance() == 1.0
    
    ex.submit_limit("sell", 11000, 1000)
    assert snap.imbalance() == 1.0 # snapshot is a copy
    
    snap2 = ex.full_book()
    assert snap2.imbalance() == 0.0

def test_exchange_clear_order_history_empty():
    ex = nanobook.Exchange()
    assert ex.clear_order_history() == 0

def test_exchange_compact():
    ex = nanobook.Exchange()
    # Fill a level with many orders
    for _ in range(100):
        ex.submit_limit("buy", 10000, 10)
    
    # Cancel all of them - creates 100 tombstones
    for i in range(1, 101):
        ex.cancel(i)
    
    # Compact should clean them up
    # We can't directly check tombstone count from Python, but we can verify it doesn't crash
    ex.compact()
    assert ex.best_bid() is None

def test_order_timestamp_is_increasing():
    ex = nanobook.Exchange()
    o1 = ex.get_order(ex.submit_limit("buy", 10000, 100).order_id)
    o2 = ex.get_order(ex.submit_limit("buy", 10000, 100).order_id)
    assert o2.timestamp >= o1.timestamp
