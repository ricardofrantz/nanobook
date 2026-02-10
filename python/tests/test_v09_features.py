import math

import nanobook


def _sample_returns_matrix():
    return [
        [0.010, 0.004, -0.002],
        [-0.003, 0.006, 0.001],
        [0.007, -0.001, 0.002],
        [0.004, 0.003, -0.004],
        [-0.002, 0.005, 0.003],
        [0.006, -0.002, 0.001],
    ]


def _assert_long_only_weights(weights: dict[str, float], symbols: list[str]):
    assert set(weights.keys()) == set(symbols)
    assert all(math.isfinite(w) and w >= -1e-12 for w in weights.values())
    assert abs(sum(weights.values()) - 1.0) < 1e-6


def test_capabilities_surface():
    caps = set(nanobook.py_capabilities())
    expected = {
        "backtest_stops",
        "garch_forecast",
        "optimize_min_variance",
        "optimize_max_sharpe",
        "optimize_risk_parity",
        "optimize_cvar",
        "optimize_cdar",
        "backtest_holdings",
    }
    assert expected.issubset(caps)


def test_garch_forecast_finite():
    v = nanobook.py_garch_forecast([0.01, -0.003, 0.007, -0.002, 0.004], p=1, q=1)
    assert math.isfinite(v)
    assert v >= 0.0


def test_optimizers_return_valid_weights():
    symbols = ["AAPL", "MSFT", "NVDA"]
    r = _sample_returns_matrix()

    minvar = nanobook.py_optimize_min_variance(r, symbols)
    maxsh = nanobook.py_optimize_max_sharpe(r, symbols, risk_free=0.0)
    rp = nanobook.py_optimize_risk_parity(r, symbols)
    cvar = nanobook.py_optimize_cvar(r, symbols, alpha=0.95)
    cdar = nanobook.py_optimize_cdar(r, symbols, alpha=0.95)

    _assert_long_only_weights(minvar, symbols)
    _assert_long_only_weights(maxsh, symbols)
    _assert_long_only_weights(rp, symbols)
    _assert_long_only_weights(cvar, symbols)
    _assert_long_only_weights(cdar, symbols)


def test_backtest_weights_v09_payload():
    result = nanobook.py_backtest_weights(
        weight_schedule=[[('AAPL', 1.0)], [('AAPL', 1.0)]],
        price_schedule=[[('AAPL', 100_00)], [('AAPL', 102_00)]],
        initial_cash=100_000_00,
        cost_bps=0,
    )

    assert "holdings" in result
    assert "symbol_returns" in result
    assert "stop_events" in result
    assert len(result["holdings"]) == 2
    assert len(result["symbol_returns"]) == 2
    assert result["stop_events"] == []


def test_backtest_weights_fixed_stop():
    result = nanobook.py_backtest_weights(
        weight_schedule=[[('AAPL', 1.0)], [('AAPL', 1.0)]],
        price_schedule=[[('AAPL', 100_00)], [('AAPL', 85_00)]],
        initial_cash=100_000_00,
        cost_bps=0,
        stop_cfg={"fixed_stop_pct": 0.10},
    )

    assert len(result["stop_events"]) == 1
    event = result["stop_events"][0]
    assert event["symbol"] == "AAPL"
    assert event["reason"] == "fixed"
    assert event["trigger_price"] == 90_00
    assert event["exit_price"] == 85_00
    assert result["holdings"][1] == []


def test_backtest_first_breach_once_per_position_lifecycle():
    result = nanobook.py_backtest_weights(
        weight_schedule=[[("AAPL", 1.0)], [("AAPL", 1.0)], [("AAPL", 1.0)]],
        price_schedule=[[("AAPL", 100_00)], [("AAPL", 90_00)], [("AAPL", 89_00)]],
        initial_cash=100_000_00,
        cost_bps=0,
        stop_cfg={"fixed_stop_pct": 0.10},
    )

    assert len(result["stop_events"]) == 1
    assert result["stop_events"][0]["period_index"] == 1
    assert result["stop_events"][0]["reason"] == "fixed"


def test_backtest_reports_tightest_stop_reason():
    result = nanobook.py_backtest_weights(
        weight_schedule=[[("AAPL", 1.0)], [("AAPL", 1.0)], [("AAPL", 1.0)]],
        price_schedule=[[("AAPL", 100_00)], [("AAPL", 110_00)], [("AAPL", 103_00)]],
        initial_cash=100_000_00,
        cost_bps=0,
        stop_cfg={"fixed_stop_pct": 0.10, "trailing_stop_pct": 0.05},
    )

    assert len(result["stop_events"]) == 1
    event = result["stop_events"][0]
    assert event["reason"] == "trailing"
    assert event["trigger_price"] == 104_50
