"""Tests for the parallel sweep Python bindings."""

import nanobook


def test_sweep_basic():
    prices = [
        [("AAPL", 150_00)],
        [("AAPL", 155_00)],
        [("AAPL", 160_00)],
    ]
    results = nanobook.py_sweep_equal_weight(
        n_params=5,
        price_series=prices,
        initial_cash=1_000_000_00,
        periods_per_year=12.0,
        risk_free=0.0,
    )
    assert len(results) == 5
    for m in results:
        assert m is not None
        assert m.total_return > 0


def test_sweep_empty():
    prices = [
        [("AAPL", 150_00)],
        [("AAPL", 155_00)],
    ]
    results = nanobook.py_sweep_equal_weight(
        n_params=0,
        price_series=prices,
        initial_cash=1_000_000_00,
    )
    assert len(results) == 0


def test_sweep_multi_stock():
    prices = [
        [("AAPL", 150_00), ("MSFT", 300_00)],
        [("AAPL", 155_00), ("MSFT", 310_00)],
        [("AAPL", 160_00), ("MSFT", 320_00)],
    ]
    results = nanobook.py_sweep_equal_weight(
        n_params=10,
        price_series=prices,
        initial_cash=1_000_000_00,
        periods_per_year=12.0,
    )
    assert len(results) == 10
    assert all(r is not None for r in results)
