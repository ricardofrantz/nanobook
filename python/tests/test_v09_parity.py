"""Deterministic parity targets for qtrade v0.4 bridge fixtures.

These tests lock v0.9 API outputs on fixed qtrade-like datasets,
so drift is caught early during nanobook/qtrade parallel development.
"""

import math

import nanobook


def _qtrade_reference_returns_1d() -> list[float]:
    return [
        0.011,
        -0.007,
        0.004,
        -0.002,
        0.006,
        -0.003,
        0.002,
        0.001,
        -0.004,
        0.005,
        -0.001,
        0.003,
    ]


def _qtrade_reference_returns_2d() -> list[list[float]]:
    return [
        [0.010, 0.004, -0.002, 0.006],
        [-0.003, 0.006, 0.001, -0.002],
        [0.007, -0.001, 0.002, 0.004],
        [0.004, 0.003, -0.004, 0.005],
        [-0.002, 0.005, 0.003, -0.001],
        [0.006, -0.002, 0.001, 0.003],
        [0.003, 0.004, -0.001, 0.002],
        [-0.001, 0.002, 0.002, -0.003],
        [0.005, 0.001, -0.002, 0.004],
        [0.002, 0.003, 0.001, 0.000],
        [-0.004, 0.002, 0.003, -0.002],
        [0.006, -0.001, 0.000, 0.005],
    ]


def _assert_close(got: float, expected: float, atol: float = 5e-13):
    assert math.isfinite(got)
    assert abs(got - expected) <= atol


def _assert_weight_dict_close(
    got: dict[str, float], expected: dict[str, float], atol: float = 5e-13
):
    assert set(got) == set(expected)
    for k, v in expected.items():
        _assert_close(got[k], v, atol=atol)


def test_garch_reference_target_zero_mean():
    got = nanobook.py_garch_forecast(_qtrade_reference_returns_1d(), p=1, q=1, mean="zero")
    _assert_close(got, 0.0044776400483411, atol=5e-14)


def test_garch_reference_target_constant_mean():
    got = nanobook.py_garch_forecast(
        _qtrade_reference_returns_1d(), p=2, q=1, mean="constant"
    )
    _assert_close(got, 0.0043960525154678, atol=5e-14)


def test_optimizer_reference_targets():
    returns_matrix = _qtrade_reference_returns_2d()
    symbols = ["AAPL", "MSFT", "NVDA", "META"]

    minvar = nanobook.py_optimize_min_variance(returns_matrix, symbols)
    maxsh = nanobook.py_optimize_max_sharpe(returns_matrix, symbols, risk_free=0.0)
    rp = nanobook.py_optimize_risk_parity(returns_matrix, symbols)
    cvar = nanobook.py_optimize_cvar(returns_matrix, symbols, alpha=0.95)
    cdar = nanobook.py_optimize_cdar(returns_matrix, symbols, alpha=0.95)

    _assert_weight_dict_close(
        minvar,
        {
            "AAPL": 0.2497573732080370,
            "MSFT": 0.2501599724543681,
            "NVDA": 0.2502155962699676,
            "META": 0.2498670580676274,
        },
        atol=5e-13,
    )
    _assert_weight_dict_close(
        maxsh,
        {
            "AAPL": 0.0621484559673854,
            "MSFT": 0.3035320141422045,
            "NVDA": 0.3816040047931394,
            "META": 0.2527155250972707,
        },
        atol=5e-13,
    )
    _assert_weight_dict_close(
        rp,
        {
            "AAPL": 0.0777787788667712,
            "MSFT": 0.3580541928494367,
            "NVDA": 0.2969466599605388,
            "META": 0.2672203683232534,
        },
        atol=5e-13,
    )
    _assert_weight_dict_close(
        cvar,
        {
            "AAPL": 0.1875,
            "MSFT": 0.3750,
            "NVDA": 0.1875,
            "META": 0.2500,
        },
        atol=1e-15,
    )
    _assert_weight_dict_close(
        cdar,
        {
            "AAPL": 0.1875,
            "MSFT": 0.3750,
            "NVDA": 0.1875,
            "META": 0.2500,
        },
        atol=1e-12,
    )
