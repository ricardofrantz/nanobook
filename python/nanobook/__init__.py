"""nanobook Python package exports.

The Rust extension keeps legacy ``py_*`` names for compatibility.
These aliases provide clean v0.9 names for new callers.
"""

from .nanobook import *  # noqa: F401,F403


def capabilities():
    return py_capabilities()


def backtest_weights(
    weight_schedule,
    price_schedule,
    initial_cash,
    cost_bps,
    periods_per_year=252.0,
    risk_free=0.0,
    stop_cfg=None,
):
    return py_backtest_weights(
        weight_schedule,
        price_schedule,
        initial_cash,
        cost_bps,
        periods_per_year,
        risk_free,
        stop_cfg,
    )


def garch_forecast(returns, p=1, q=1, mean="zero"):
    return py_garch_forecast(returns, p, q, mean)


def optimize_min_variance(returns_matrix, symbols):
    return py_optimize_min_variance(returns_matrix, symbols)


def optimize_max_sharpe(returns_matrix, symbols, risk_free=0.0):
    return py_optimize_max_sharpe(returns_matrix, symbols, risk_free)


def optimize_risk_parity(returns_matrix, symbols):
    return py_optimize_risk_parity(returns_matrix, symbols)


def optimize_cvar(returns_matrix, symbols, alpha=0.95):
    return py_optimize_cvar(returns_matrix, symbols, alpha)


def optimize_cdar(returns_matrix, symbols, alpha=0.95):
    return py_optimize_cdar(returns_matrix, symbols, alpha)


__all__ = [name for name in globals() if not name.startswith("_")]
