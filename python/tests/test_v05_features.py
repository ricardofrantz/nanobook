import nanobook
import pytest
import os
import json
import tempfile

def test_order_class():
    ex = nanobook.Exchange()
    res = ex.submit_limit("buy", 10050, 100, "gtc")
    order = ex.get_order(res.order_id)
    
    assert order.id == res.order_id
    assert order.side == "buy"
    assert order.price == 10050
    assert order.original_quantity == 100
    assert order.remaining_quantity == 100
    assert order.filled_quantity == 0
    assert order.status == "new"
    assert order.time_in_force == "gtc"
    assert isinstance(order.timestamp, int)
    assert "Order" in repr(order)

def test_order_partial_fill():
    ex = nanobook.Exchange()
    ex.submit_limit("sell", 10000, 40)
    res = ex.submit_limit("buy", 10000, 100)
    order = ex.get_order(res.order_id)
    assert order.status == "partiallyfilled"
    assert order.filled_quantity == 40
    assert order.remaining_quantity == 60

def test_book_snapshot_analytics():
    ex = nanobook.Exchange()
    ex.submit_limit("buy", 10000, 300)
    ex.submit_limit("sell", 10200, 100)
    
    snap = ex.full_book()
    assert snap.mid_price() == 10100.0
    assert snap.spread() == 200
    assert abs(snap.imbalance() - 0.5) < 1e-6  # (300-100)/(300+100) = 0.5
    assert snap.weighted_mid() == 10150.0 # (100*10000 + 300*10200)/400 = 10150
    assert "BookSnapshot" in repr(snap)

def test_book_snapshot_empty_analytics():
    ex = nanobook.Exchange()
    snap = ex.full_book()
    assert snap.mid_price() is None
    assert snap.spread() is None
    assert snap.imbalance() is None
    assert snap.weighted_mid() is None

def test_exchange_events_and_replay():
    ex = nanobook.Exchange()
    ex.submit_limit("buy", 10000, 100)
    ex.submit_limit("sell", 10000, 50)
    ex.cancel(1)
    
    events = ex.events()
    assert len(events) == 3
    assert events[0].kind == "submit_limit"
    assert events[2].kind == "cancel"
    
    # Replay
    ex2 = nanobook.Exchange.replay(events)
    assert ex2.best_bid_ask() == ex.best_bid_ask()
    assert len(ex2.trades()) == len(ex.trades())

def test_event_serialization():
    ex = nanobook.Exchange()
    ex.submit_limit("buy", 10000, 100)
    event = ex.events()[0]
    
    # Simple check if repr works
    assert "SubmitLimit" in repr(event)
    
    state = event.__getstate__()
    assert isinstance(state, str)
    assert "SubmitLimit" in state

def test_all_event_kinds():
    ex = nanobook.Exchange()
    ex.submit_limit("buy", 10000, 100)
    ex.submit_market("sell", 50)
    ex.cancel(1)
    ex.submit_limit("buy", 9000, 100)
    ex.modify(4, 9100, 200)
    ex.submit_stop_market("buy", 11000, 100)
    ex.submit_stop_limit("sell", 8000, 7900, 100)
    ex.submit_trailing_stop_market("buy", 12000, 100, "fixed", 100)
    ex.submit_trailing_stop_limit("sell", 7000, 6900, 100, "percentage", 0.05)
    
    kinds = [e.kind for e in ex.events()]
    assert "submit_limit" in kinds
    assert "submit_market" in kinds
    assert "cancel" in kinds
    assert "modify" in kinds
    assert "submit_stop_market" in kinds
    assert "submit_stop_limit" in kinds
    assert "submit_trailing_stop_market" in kinds
    assert "submit_trailing_stop_limit" in kinds

def test_stop_order_query():
    ex = nanobook.Exchange()
    res = ex.submit_stop_market("buy", 10500, 100)
    stop = ex.get_stop_order(res.order_id)
    
    assert stop["id"] == res.order_id
    assert stop["side"] == "buy"
    assert stop["stop_price"] == 10500
    assert stop["status"] == "pending"

def test_stop_order_not_found():
    ex = nanobook.Exchange()
    assert ex.get_stop_order(999) is None

def test_portfolio_positions():
    portfolio = nanobook.Portfolio(1_000_000_00, nanobook.CostModel.zero())
    portfolio.rebalance_simple([("AAPL", 1.0)], [("AAPL", 100_00)])
    
    pos = portfolio.position("AAPL")
    assert pos.symbol == "AAPL"
    assert pos.quantity == 10000
    assert pos.avg_entry_price == 100_00
    assert pos.total_cost == 1_000_000_00
    assert "Position" in repr(pos)
    
    positions = portfolio.positions()
    assert "AAPL" in positions
    assert positions["AAPL"].quantity == 10000

def test_position_pnl():
    portfolio = nanobook.Portfolio(1_000_000_00, nanobook.CostModel.zero())
    portfolio.rebalance_simple([("AAPL", 1.0)], [("AAPL", 100_00)])
    pos = portfolio.position("AAPL")
    
    assert pos.unrealized_pnl(110_00) == 100_000_00 # (110-100) * 10000
    
    # Sell half at 110
    portfolio.rebalance_simple([("AAPL", 0.5)], [("AAPL", 110_00)])
    pos = portfolio.position("AAPL")
    assert pos.realized_pnl == 50_000_00 # profit on 5000 shares
    assert pos.quantity == 5000

def test_position_shorting():
    portfolio = nanobook.Portfolio(1_000_000_00, nanobook.CostModel.zero())
    # Target negative weight = short
    portfolio.rebalance_simple([("AAPL", -0.5)], [("AAPL", 100_00)])
    pos = portfolio.position("AAPL")
    assert pos.quantity < 0
    assert pos.unrealized_pnl(90_00) > 0 # profit when price drops
    assert pos.unrealized_pnl(110_00) < 0

def test_portfolio_snapshot():
    portfolio = nanobook.Portfolio(1_000_000_00, nanobook.CostModel.zero())
    portfolio.rebalance_simple([("AAPL", 0.5)], [("AAPL", 100_00)])
    
    snap = portfolio.snapshot([("AAPL", 110_00)])
    assert snap["cash"] < 1_000_000_00
    assert snap["equity"] == 1_050_000_00 # 500k cash + 500k stock @ 1.1x = 1.05M
    assert abs(snap["weights"]["AAPL"] - (550_000_00 / 1_050_000_00)) < 1e-6

def test_multiexchange_forwarding():
    multi = nanobook.MultiExchange()
    res = multi.submit_limit("AAPL", "buy", 10000, 100)
    assert res.order_id == 1
    
    multi.submit_limit("MSFT", "sell", 20000, 50)
    prices = multi.best_prices()
    
    price_dict = {p[0]: (p[1], p[2]) for p in prices}
    assert price_dict["AAPL"] == (10000, None)
    assert price_dict["MSFT"] == (None, 20000)

def test_multiexchange_complex_forwarding():
    multi = nanobook.MultiExchange()
    multi.submit_limit("AAPL", "buy", 10000, 100)
    multi.modify("AAPL", 1, 10100, 150)
    multi.submit_market("AAPL", "sell", 50)
    multi.cancel("AAPL", 2)
    
    ex = multi.get_or_create("AAPL")
    assert ex.best_bid() is None # modified order was cancelled, then 2nd order cancelled.
    # Wait, modify creates a NEW order ID.
    # 1: buy 10000 100
    # modify(1, 10100, 150) -> cancels 1, creates 2: buy 10100 150
    # submit_market sell 50 -> matches 50 of order 2. 2 is now PartiallyFilled with 100 remaining.
    # cancel(2) -> cancels remaining 100 of order 2.
    # result: no orders left.
    assert ex.best_bid() is None
    assert len(ex.trades()) == 1

def test_run_backtest_callback():
    def constant_strat(bar_index, prices, portfolio):
        return [("AAPL", 1.0)]
    
    price_series = [
        {"AAPL": 100_00},
        {"AAPL": 110_00},
        {"AAPL": 120_00},
    ]
    
    res = nanobook.run_backtest(
        strategy=constant_strat,
        price_series=price_series,
        initial_cash=100_000_00,
        cost_model=nanobook.CostModel.zero(),
    )
    
    assert len(res.portfolio.returns()) == 3
    assert res.metrics.total_return > 0
    assert "BacktestResult" in repr(res)

def test_strategy_exception_handling():
    def breaking_strat(bar_index, prices, portfolio):
        if bar_index == 1:
            raise ValueError("Strategy exploded")
        return [("AAPL", 1.0)]
    
    price_series = [{"AAPL": 100_00}, {"AAPL": 110_00}]
    # Should not crash the process, but return empty weights for that bar (or handle gracefully)
    # The current implementation prints the error and returns empty weights.
    res = nanobook.run_backtest(breaking_strat, price_series, 100_00, nanobook.CostModel.zero())
    assert len(res.portfolio.returns()) == 2

def test_strategy_invalid_return():
    def bad_return_strat(bar_index, prices, portfolio):
        return "not a list"
    
    price_series = [{"AAPL": 100_00}]
    res = nanobook.run_backtest(bad_return_strat, price_series, 100_00, nanobook.CostModel.zero())
    assert len(res.portfolio.returns()) == 1

def test_portfolio_rebalance_lob():
    multi = nanobook.MultiExchange()
    multi.submit_limit("AAPL", "sell", 100_00, 1000)
    
    portfolio = nanobook.Portfolio(1_000_000_00, nanobook.CostModel.zero())
    # This should buy from the LOB
    portfolio.rebalance_lob([("AAPL", 1.0)], multi)
    
    assert portfolio.position("AAPL").quantity > 0
    assert multi.get_or_create("AAPL").best_ask() is None # Swept the book

def test_clear_order_history():
    ex = nanobook.Exchange()
    ex.submit_limit("buy", 10000, 100)
    ex.cancel(1)
    # Order 1 is now in history
    assert ex.clear_order_history() > 0
    assert ex.get_order(1) is None

def test_last_trade_price():
    ex = nanobook.Exchange()
    ex.submit_limit("sell", 10000, 100)
    ex.submit_limit("buy", 10000, 50)
    assert ex.last_trade_price() == 10000

def test_portfolio_save_load_json_persistence():
    p = nanobook.Portfolio(1_000_000_00, nanobook.CostModel.zero())
    p.rebalance_simple([("AAPL", 0.5)], [("AAPL", 150_00)])
    
    with tempfile.NamedTemporaryFile(suffix=".json", delete=False) as f:
        path = f.name
    
    try:
        p.save_json(path)
        p2 = nanobook.Portfolio.load_json(path)
        assert p2.cash == p.cash
        assert p2.position("AAPL").quantity == p.position("AAPL").quantity
    finally:
        if os.path.exists(path):
            os.remove(path)

def test_metrics_serialization():
    m = nanobook.py_compute_metrics([0.01, 0.02, -0.01], 252, 0.0)
    # Metrics doesn't have explicit serialization in WP1, but let's check it's accessible
    assert m.sharpe is not None

def test_multiexchange_len_symbols():
    multi = nanobook.MultiExchange()
    multi.get_or_create("AAPL")
    multi.get_or_create("GOOG")
    assert multi.len() == 2
    assert "AAPL" in multi.symbols()
    assert "GOOG" in multi.symbols()

def test_book_snapshot_depth_limit():
    ex = nanobook.Exchange()
    for i in range(20):
        ex.submit_limit("buy", 10000 - i, 10)
    
    snap = ex.depth(5)
    assert len(snap.bids) == 5
    
    full = ex.full_book()
    assert len(full.bids) == 20

def test_exchange_replay_complex():
    ex = nanobook.Exchange()
    ex.submit_limit("buy", 10000, 100)
    ex.submit_limit("sell", 10100, 100)
    ex.submit_stop_market("buy", 10100, 50)
    ex.submit_limit("buy", 10100, 20) # Triggers stop partly? No, needs trade AT 10100.
    ex.submit_limit("buy", 10100, 100) # Produces trade at 10100, triggers stop.
    
    replayed = nanobook.Exchange.replay(ex.events())
    assert replayed.best_bid_ask() == ex.best_bid_ask()
    assert replayed.pending_stop_count() == ex.pending_stop_count()
    assert len(replayed.trades()) == len(ex.trades())

def test_exchange_clear_trades_and_history():
    ex = nanobook.Exchange()
    ex.submit_limit("sell", 10000, 100)
    ex.submit_limit("buy", 10000, 100)
    assert len(ex.trades()) == 1
    ex.clear_trades()
    assert len(ex.trades()) == 0
    assert ex.clear_order_history() > 0

def test_order_status_terminal():
    ex = nanobook.Exchange()
    ex.submit_limit("buy", 10000, 100)
    ex.cancel(1)
    order = ex.get_order(1)
    assert order.status == "cancelled"

def test_portfolio_total_equity_multiple_stocks():
    p = nanobook.Portfolio(100_00, nanobook.CostModel.zero())
    p.rebalance_simple([("AAPL", 0.5), ("GOOG", 0.5)], [("AAPL", 10_00), ("GOOG", 20_00)])
    equity = p.total_equity([("AAPL", 12_00), ("GOOG", 18_00)])
    # Initial: 50 in AAPL (5 shares), 50 in GOOG (2.5 shares -> 2 shares if integer)
    # Wait, rebalance_simple uses integer division for quantity.
    # 50 / 10 = 5 shares AAPL.
    # 50 / 20 = 2 shares GOOG.
    # New equity: 5*12 + 2*18 + (100 - 5*10 - 2*20) = 60 + 36 + 10 = 106.
    # But values are in cents, so 10600.
    assert equity == 10600

def test_cost_model_min_fee():
    model = nanobook.CostModel(commission_bps=0, slippage_bps=0, min_trade_fee=100)
    assert model.compute_cost(1000) == 100
    assert model.compute_cost(1000000) == 100
