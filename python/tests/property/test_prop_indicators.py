"""Property-based tests for indicators using Hypothesis.

Tests mathematical invariants that must hold for ALL inputs,
regardless of what the reference library does.

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
        st.floats(min_value=0.01, max_value=100_000.0, allow_nan=False, allow_infinity=False),
        min_size=30,
        max_size=500,
    )
)
@settings(max_examples=200)
def test_rsi_bounds(close):
    """RSI output is always in [0, 100] for non-NaN values."""
    result = nanobook.py_rsi(close, 14)
    for v in result:
        if not math.isnan(v):
            assert 0.0 <= v <= 100.0, f"RSI out of bounds: {v}"


@given(
    st.lists(
        st.floats(min_value=0.01, max_value=100_000.0, allow_nan=False, allow_infinity=False),
        min_size=30,
        max_size=500,
    )
)
@settings(max_examples=200)
def test_rsi_output_length(close):
    """RSI output has same length as input."""
    result = nanobook.py_rsi(close, 14)
    assert len(result) == len(close)


@given(
    st.lists(
        st.floats(min_value=0.01, max_value=100_000.0, allow_nan=False, allow_infinity=False),
        min_size=30,
        max_size=500,
    )
)
@settings(max_examples=100)
def test_bbands_ordering(close):
    """Bollinger Bands: lower <= middle <= upper (when not NaN)."""
    upper, middle, lower = nanobook.py_bbands(close, 20, 2.0, 2.0)
    for i in range(len(close)):
        if not math.isnan(middle[i]):
            assert lower[i] <= middle[i] + 1e-10, f"lower > middle at {i}"
            assert middle[i] <= upper[i] + 1e-10, f"middle > upper at {i}"


@given(
    st.lists(
        st.floats(min_value=0.01, max_value=100_000.0, allow_nan=False, allow_infinity=False),
        min_size=30,
        max_size=500,
    )
)
@settings(max_examples=100)
def test_atr_non_negative(close):
    """ATR is always non-negative (when not NaN)."""
    # Generate plausible high/low from close
    high = [c * 1.01 for c in close]
    low = [c * 0.99 for c in close]
    result = nanobook.py_atr(high, low, close, 14)
    for v in result:
        if not math.isnan(v):
            assert v >= 0.0, f"ATR negative: {v}"


@given(
    st.lists(
        st.floats(min_value=0.01, max_value=100_000.0, allow_nan=False, allow_infinity=False),
        min_size=40,
        max_size=500,
    )
)
@settings(max_examples=100)
def test_macd_output_lengths(close):
    """MACD returns three same-length arrays."""
    macd_line, signal, hist = nanobook.py_macd(close, 12, 26, 9)
    assert len(macd_line) == len(close)
    assert len(signal) == len(close)
    assert len(hist) == len(close)
