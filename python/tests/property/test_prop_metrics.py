"""Property-based tests for metrics using Hypothesis.

Tests statistical properties that must hold for ALL valid return series.

Dev dependencies: hypothesis
"""

import math

import pytest

try:
    from hypothesis import given, settings, strategies as st

    HAS_HYPOTHESIS = True
except ImportError:
    HAS_HYPOTHESIS = False

import nanobook

pytestmark = pytest.mark.skipif(not HAS_HYPOTHESIS, reason="hypothesis not installed")


@given(
    st.lists(
        st.floats(min_value=-0.5, max_value=0.5, allow_nan=False, allow_infinity=False),
        min_size=10,
        max_size=500,
    )
)
@settings(max_examples=200)
def test_win_rate_bounds(returns):
    """Win rate is always in [0, 1]."""
    m = nanobook.py_compute_metrics(returns, 252.0, 0.0)
    if m is not None:
        assert 0.0 <= m.win_rate <= 1.0


@given(
    st.lists(
        st.floats(min_value=-0.5, max_value=0.5, allow_nan=False, allow_infinity=False),
        min_size=10,
        max_size=500,
    )
)
@settings(max_examples=200)
def test_profit_factor_non_negative(returns):
    """Profit factor is non-negative (or infinity)."""
    m = nanobook.py_compute_metrics(returns, 252.0, 0.0)
    if m is not None:
        assert m.profit_factor >= 0.0 or math.isinf(m.profit_factor)


@given(
    st.lists(
        st.floats(min_value=-0.5, max_value=0.5, allow_nan=False, allow_infinity=False),
        min_size=10,
        max_size=500,
    )
)
@settings(max_examples=200)
def test_max_drawdown_non_negative(returns):
    """Max drawdown is always >= 0."""
    m = nanobook.py_compute_metrics(returns, 252.0, 0.0)
    if m is not None:
        assert m.max_drawdown >= 0.0


@given(
    st.lists(
        st.floats(min_value=0.001, max_value=0.5, allow_nan=False, allow_infinity=False),
        min_size=10,
        max_size=500,
    )
)
@settings(max_examples=100)
def test_all_positive_returns_properties(returns):
    """All-positive returns should yield win_rate=1, sharpe>0, max_dd=0."""
    m = nanobook.py_compute_metrics(returns, 252.0, 0.0)
    if m is not None:
        assert m.win_rate == 1.0
        assert m.max_drawdown < 1e-10
        assert m.sharpe > 0.0


@given(
    st.lists(
        st.floats(min_value=-0.5, max_value=-0.001, allow_nan=False, allow_infinity=False),
        min_size=10,
        max_size=500,
    )
)
@settings(max_examples=100)
def test_all_negative_returns_properties(returns):
    """All-negative returns should yield win_rate=0, sharpe<0."""
    m = nanobook.py_compute_metrics(returns, 252.0, 0.0)
    if m is not None:
        assert m.win_rate == 0.0
        assert m.sharpe <= 0.0  # 0 when constant (zero vol), negative otherwise


@given(
    st.lists(
        st.floats(min_value=-0.3, max_value=0.3, allow_nan=False, allow_infinity=False),
        min_size=30,
        max_size=500,
    )
)
@settings(max_examples=100)
def test_rolling_sharpe_length(returns):
    """Rolling Sharpe output has same length as input."""
    result = nanobook.py_rolling_sharpe(returns, 20, 252)
    assert len(result) == len(returns)


@given(
    st.lists(
        st.floats(min_value=-0.3, max_value=0.3, allow_nan=False, allow_infinity=False),
        min_size=30,
        max_size=500,
    )
)
@settings(max_examples=100)
def test_rolling_volatility_non_negative(returns):
    """Rolling volatility is always non-negative (when not NaN)."""
    result = nanobook.py_rolling_volatility(returns, 20, 252)
    for v in result:
        if not math.isnan(v):
            assert v >= 0.0, f"negative volatility: {v}"
