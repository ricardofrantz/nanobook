"""Tests for the Exchange Python bindings."""

import nanobook


def test_version():
    assert nanobook.__version__ == "0.5.0"


def test_exchange_repr():
    ex = nanobook.Exchange()
    assert "Exchange" in repr(ex)


def test_submit_limit_no_match():
    ex = nanobook.Exchange()
    result = ex.submit_limit("buy", 10000, 100, "gtc")
    assert result.order_id == 1
    assert result.status == "New"
    assert result.filled_quantity == 0
    assert result.resting_quantity == 100
    assert len(result.trades) == 0


def test_submit_limit_full_fill():
    ex = nanobook.Exchange()
    ex.submit_limit("sell", 10000, 100, "gtc")
    result = ex.submit_limit("buy", 10000, 100, "gtc")
    assert result.status == "Filled"
    assert result.filled_quantity == 100
    assert len(result.trades) == 1
    assert result.trades[0].price == 10000
    assert result.trades[0].quantity == 100


def test_submit_limit_ioc():
    ex = nanobook.Exchange()
    ex.submit_limit("sell", 10000, 30, "gtc")
    result = ex.submit_limit("buy", 10000, 100, "ioc")
    assert result.filled_quantity == 30
    assert result.cancelled_quantity == 70
    assert result.resting_quantity == 0


def test_submit_limit_fok_reject():
    ex = nanobook.Exchange()
    ex.submit_limit("sell", 10000, 50, "gtc")
    result = ex.submit_limit("buy", 10000, 100, "fok")
    assert result.filled_quantity == 0
    assert result.cancelled_quantity == 100
    assert len(result.trades) == 0


def test_submit_market():
    ex = nanobook.Exchange()
    ex.submit_limit("sell", 10000, 100, "gtc")
    result = ex.submit_market("buy", 100)
    assert result.filled_quantity == 100
    assert result.status == "Filled"


def test_cancel():
    ex = nanobook.Exchange()
    submit = ex.submit_limit("buy", 10000, 100, "gtc")
    result = ex.cancel(submit.order_id)
    assert result.success
    assert result.cancelled_quantity == 100


def test_cancel_nonexistent():
    ex = nanobook.Exchange()
    result = ex.cancel(999)
    assert not result.success
    assert result.error is not None


def test_modify():
    ex = nanobook.Exchange()
    submit = ex.submit_limit("buy", 10000, 100, "gtc")
    result = ex.modify(submit.order_id, 9900, 150)
    assert result.success
    assert result.new_order_id is not None
    assert result.cancelled_quantity == 100


def test_best_bid_ask():
    ex = nanobook.Exchange()
    ex.submit_limit("buy", 10000, 100, "gtc")
    ex.submit_limit("sell", 10100, 100, "gtc")
    bid, ask = ex.best_bid_ask()
    assert bid == 10000
    assert ask == 10100


def test_spread():
    ex = nanobook.Exchange()
    ex.submit_limit("buy", 10000, 100, "gtc")
    ex.submit_limit("sell", 10100, 100, "gtc")
    assert ex.spread() == 100


def test_depth():
    ex = nanobook.Exchange()
    ex.submit_limit("buy", 10000, 100, "gtc")
    ex.submit_limit("buy", 9900, 200, "gtc")
    ex.submit_limit("sell", 10100, 150, "gtc")
    snap = ex.depth(10)
    assert len(snap.bids) == 2
    assert len(snap.asks) == 1


def test_trades():
    ex = nanobook.Exchange()
    ex.submit_limit("sell", 10000, 100, "gtc")
    ex.submit_limit("buy", 10000, 100, "gtc")
    trades = ex.trades()
    assert len(trades) == 1
    assert trades[0].quantity == 100


def test_trade_repr():
    ex = nanobook.Exchange()
    ex.submit_limit("sell", 10050, 100, "gtc")
    ex.submit_limit("buy", 10050, 100, "gtc")
    trade = ex.trades()[0]
    assert "Trade" in repr(trade)
    assert trade.price_float == 100.50


def test_stop_market():
    ex = nanobook.Exchange()
    result = ex.submit_stop_market("buy", 10500, 100)
    assert result.status == "Pending"
    assert ex.pending_stop_count() == 1


def test_cancel_stop():
    ex = nanobook.Exchange()
    stop = ex.submit_stop_market("buy", 10500, 100)
    result = ex.cancel(stop.order_id)
    assert result.success
    assert ex.pending_stop_count() == 0


def test_trailing_stop_fixed():
    ex = nanobook.Exchange()
    result = ex.submit_trailing_stop_market(
        "sell", 9500, 100, "fixed", 200
    )
    assert result.status == "Pending"
    assert ex.pending_stop_count() == 1


def test_trailing_stop_percentage():
    ex = nanobook.Exchange()
    result = ex.submit_trailing_stop_market(
        "sell", 9500, 100, "percentage", 0.05
    )
    assert result.status == "Pending"


def test_trailing_stop_atr():
    ex = nanobook.Exchange()
    result = ex.submit_trailing_stop_market(
        "sell", 9500, 100, "atr", 2.0, atr_period=14
    )
    assert result.status == "Pending"


def test_clear_trades():
    ex = nanobook.Exchange()
    ex.submit_limit("sell", 10000, 100, "gtc")
    ex.submit_limit("buy", 10000, 100, "gtc")
    assert len(ex.trades()) == 1
    ex.clear_trades()
    assert len(ex.trades()) == 0


def test_invalid_side():
    ex = nanobook.Exchange()
    try:
        ex.submit_limit("invalid", 10000, 100, "gtc")
        assert False, "Should have raised ValueError"
    except ValueError:
        pass


def test_invalid_tif():
    ex = nanobook.Exchange()
    try:
        ex.submit_limit("buy", 10000, 100, "invalid")
        assert False, "Should have raised ValueError"
    except ValueError:
        pass
