"""Tests for the Portfolio Python bindings."""

import nanobook
import tempfile
import os


def test_cost_model_zero():
    model = nanobook.CostModel.zero()
    assert model.compute_cost(1_000_000) == 0


def test_cost_model_custom():
    model = nanobook.CostModel(commission_bps=10, slippage_bps=5)
    cost = model.compute_cost(1_000_000)
    assert cost == 1500  # 15 bps on $10,000


def test_cost_model_repr():
    model = nanobook.CostModel.zero()
    assert "CostModel" in repr(model)


def test_portfolio_new():
    p = nanobook.Portfolio(1_000_000_00, nanobook.CostModel.zero())
    assert p.cash == 1_000_000_00


def test_portfolio_rebalance_simple():
    p = nanobook.Portfolio(1_000_000_00, nanobook.CostModel.zero())
    targets = [("AAPL", 0.5)]
    prices = [("AAPL", 150_00)]
    p.rebalance_simple(targets, prices)
    assert p.cash < 1_000_000_00  # Some cash spent buying


def test_portfolio_equity():
    p = nanobook.Portfolio(1_000_000_00, nanobook.CostModel.zero())
    equity = p.total_equity([("AAPL", 150_00)])
    assert equity == 1_000_000_00  # No positions yet


def test_portfolio_returns():
    p = nanobook.Portfolio(100_00, nanobook.CostModel.zero())
    prices = [("AAPL", 10_00)]
    p.rebalance_simple([("AAPL", 1.0)], prices)
    p.record_return([("AAPL", 11_00)])  # 10% up
    returns = p.returns()
    assert len(returns) == 1
    assert returns[0] > 0


def test_portfolio_equity_curve():
    p = nanobook.Portfolio(100_00, nanobook.CostModel.zero())
    curve = p.equity_curve()
    assert len(curve) == 1  # Initial equity
    assert curve[0] == 100_00


def test_compute_metrics():
    m = nanobook.py_compute_metrics([0.01, -0.005, 0.02], 252.0, 0.0)
    assert m is not None
    assert m.total_return > 0
    assert m.num_periods == 3
    assert m.winning_periods == 2
    assert m.losing_periods == 1
    assert "Metrics" in repr(m)


def test_compute_metrics_empty():
    m = nanobook.py_compute_metrics([], 252.0, 0.0)
    assert m is None


def test_portfolio_compute_metrics():
    p = nanobook.Portfolio(1_000_000_00, nanobook.CostModel.zero())
    prices = [("AAPL", 150_00)]
    p.rebalance_simple([("AAPL", 0.5)], prices)
    p.record_return([("AAPL", 155_00)])
    p.record_return([("AAPL", 160_00)])
    m = p.compute_metrics(12.0, 0.0)
    assert m is not None
    assert m.total_return > 0


def test_portfolio_save_load():
    p = nanobook.Portfolio(1_000_000_00, nanobook.CostModel.zero())
    prices = [("AAPL", 150_00)]
    p.rebalance_simple([("AAPL", 0.5)], prices)

    with tempfile.NamedTemporaryFile(suffix=".json", delete=False) as f:
        path = f.name

    try:
        p.save_json(path)
        loaded = nanobook.Portfolio.load_json(path)
        assert loaded.cash == p.cash
    finally:
        os.unlink(path)


def test_portfolio_repr():
    p = nanobook.Portfolio(1_000_000_00, nanobook.CostModel.zero())
    assert "Portfolio" in repr(p)
