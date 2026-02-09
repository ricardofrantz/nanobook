"""Reference tests: nanobook indicators vs TA-Lib.

Validates that nanobook's Rust RSI/MACD/BBANDS/ATR produce numerically
identical results to TA-Lib's C implementation.

Dev dependencies: ta-lib (requires C library: `brew install ta-lib`)
"""

import numpy as np
import pytest

try:
    import talib

    HAS_TALIB = True
except ImportError:
    HAS_TALIB = False

import nanobook

pytestmark = pytest.mark.skipif(not HAS_TALIB, reason="ta-lib not installed")


class TestRSIReference:
    """Validate nanobook RSI against TA-Lib RSI."""

    ATOL = 1e-10

    def test_random_close(self, random_close):
        ref = talib.RSI(random_close, timeperiod=14)
        got = nanobook.py_rsi(random_close.tolist(), 14)
        valid = ~np.isnan(ref)
        np.testing.assert_allclose(
            np.array(got)[valid], ref[valid], atol=self.ATOL
        )

    def test_monotonic_up(self):
        close = np.arange(1.0, 101.0)
        ref = talib.RSI(close, timeperiod=14)
        got = nanobook.py_rsi(close.tolist(), 14)
        valid = ~np.isnan(ref)
        np.testing.assert_allclose(
            np.array(got)[valid], ref[valid], atol=self.ATOL
        )
        assert got[-1] > 99.0

    def test_monotonic_down(self):
        close = np.arange(100.0, 0.0, -1.0)
        ref = talib.RSI(close, timeperiod=14)
        got = nanobook.py_rsi(close.tolist(), 14)
        valid = ~np.isnan(ref)
        np.testing.assert_allclose(
            np.array(got)[valid], ref[valid], atol=self.ATOL
        )

    def test_constant_price(self):
        close = np.full(100, 50.0)
        ref = talib.RSI(close, timeperiod=14)
        got = nanobook.py_rsi(close.tolist(), 14)
        valid = ~np.isnan(ref)
        np.testing.assert_allclose(
            np.array(got)[valid], ref[valid], atol=self.ATOL
        )

    def test_single_spike(self):
        close = np.full(100, 100.0)
        close[50] = 200.0
        ref = talib.RSI(close, timeperiod=14)
        got = nanobook.py_rsi(close.tolist(), 14)
        valid = ~np.isnan(ref)
        np.testing.assert_allclose(
            np.array(got)[valid], ref[valid], atol=self.ATOL
        )

    def test_lookback_nan_count(self, random_close):
        ref = talib.RSI(random_close, timeperiod=14)
        got = nanobook.py_rsi(random_close.tolist(), 14)
        # Same number of leading NaNs
        assert sum(np.isnan(ref)) == sum(np.isnan(got))


class TestMACDReference:
    """Validate nanobook MACD against TA-Lib MACD."""

    ATOL = 1e-10

    def test_random_close(self, random_close):
        ref_macd, ref_signal, ref_hist = talib.MACD(
            random_close, fastperiod=12, slowperiod=26, signalperiod=9
        )
        got_macd, got_signal, got_hist = nanobook.py_macd(
            random_close.tolist(), 12, 26, 9
        )

        for ref, got, name in [
            (ref_macd, got_macd, "macd"),
            (ref_signal, got_signal, "signal"),
            (ref_hist, got_hist, "histogram"),
        ]:
            valid = ~np.isnan(ref)
            np.testing.assert_allclose(
                np.array(got)[valid],
                ref[valid],
                atol=self.ATOL,
                err_msg=f"{name} mismatch",
            )


class TestBBandsReference:
    """Validate nanobook Bollinger Bands against TA-Lib BBANDS."""

    ATOL = 1e-10

    def test_random_close(self, random_close):
        ref_upper, ref_middle, ref_lower = talib.BBANDS(
            random_close, timeperiod=20, nbdevup=2.0, nbdevdn=2.0
        )
        got_upper, got_middle, got_lower = nanobook.py_bbands(
            random_close.tolist(), 20, 2.0, 2.0
        )

        for ref, got, name in [
            (ref_upper, got_upper, "upper"),
            (ref_middle, got_middle, "middle"),
            (ref_lower, got_lower, "lower"),
        ]:
            valid = ~np.isnan(ref)
            np.testing.assert_allclose(
                np.array(got)[valid],
                ref[valid],
                atol=self.ATOL,
                err_msg=f"{name} band mismatch",
            )

    def test_ordering(self, random_close):
        upper, middle, lower = nanobook.py_bbands(
            random_close.tolist(), 20, 2.0, 2.0
        )
        for i in range(19, len(upper)):
            if not np.isnan(upper[i]):
                assert lower[i] <= middle[i] <= upper[i], f"ordering violated at {i}"


class TestATRReference:
    """Validate nanobook ATR against TA-Lib ATR."""

    ATOL = 1e-10

    def test_random_ohlc(self, random_ohlc):
        high, low, close = random_ohlc
        ref = talib.ATR(high, low, close, timeperiod=14)
        got = nanobook.py_atr(high.tolist(), low.tolist(), close.tolist(), 14)
        valid = ~np.isnan(ref)
        np.testing.assert_allclose(
            np.array(got)[valid], ref[valid], atol=self.ATOL
        )

    def test_constant_range(self):
        high = np.full(50, 102.0)
        low = np.full(50, 98.0)
        close = np.full(50, 100.0)
        ref = talib.ATR(high, low, close, timeperiod=14)
        got = nanobook.py_atr(high.tolist(), low.tolist(), close.tolist(), 14)
        valid = ~np.isnan(ref)
        np.testing.assert_allclose(
            np.array(got)[valid], ref[valid], atol=self.ATOL
        )
