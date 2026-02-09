"""Reference tests: nanobook extended metrics vs quantstats.

Validates CVaR, win_rate, profit_factor, payoff_ratio, Kelly, and
rolling metrics against quantstats implementations.

Dev dependencies: quantstats, pandas, numpy
"""

import numpy as np
import pandas as pd
import pytest

try:
    import quantstats as qs

    HAS_QS = True
except ImportError:
    HAS_QS = False

import nanobook

pytestmark = pytest.mark.skipif(not HAS_QS, reason="quantstats not installed")


class TestCVaRReference:
    """Validate nanobook CVaR against quantstats."""

    ATOL = 1e-8

    def test_random_returns(self, random_returns):
        ret_pd = pd.Series(random_returns)
        ref = qs.stats.cvar(ret_pd)
        m = nanobook.py_compute_metrics(random_returns.tolist(), 252.0, 0.0)
        assert abs(m.cvar_95 - ref) < self.ATOL


class TestWinRateReference:
    """Validate nanobook win_rate against quantstats."""

    ATOL = 1e-10

    def test_random_returns(self, random_returns):
        ret_pd = pd.Series(random_returns)
        ref = qs.stats.win_rate(ret_pd)
        m = nanobook.py_compute_metrics(random_returns.tolist(), 252.0, 0.0)
        assert abs(m.win_rate - ref) < self.ATOL


class TestProfitFactorReference:
    """Validate nanobook profit_factor against quantstats."""

    ATOL = 1e-10

    def test_random_returns(self, random_returns):
        ret_pd = pd.Series(random_returns)
        ref = qs.stats.profit_factor(ret_pd)
        m = nanobook.py_compute_metrics(random_returns.tolist(), 252.0, 0.0)
        if np.isfinite(ref):
            assert abs(m.profit_factor - ref) < self.ATOL


class TestPayoffRatioReference:
    """Validate nanobook payoff_ratio against quantstats."""

    ATOL = 1e-10

    def test_random_returns(self, random_returns):
        ret_pd = pd.Series(random_returns)
        ref = qs.stats.payoff_ratio(ret_pd)
        m = nanobook.py_compute_metrics(random_returns.tolist(), 252.0, 0.0)
        if np.isfinite(ref):
            assert abs(m.payoff_ratio - ref) < self.ATOL


class TestKellyReference:
    """Validate nanobook Kelly criterion against quantstats."""

    ATOL = 1e-10

    def test_random_returns(self, random_returns):
        ret_pd = pd.Series(random_returns)
        ref = qs.stats.kelly_criterion(ret_pd)
        m = nanobook.py_compute_metrics(random_returns.tolist(), 252.0, 0.0)
        if np.isfinite(ref):
            assert abs(m.kelly - ref) < self.ATOL


class TestRollingSharpeReference:
    """Validate nanobook rolling Sharpe against quantstats."""

    ATOL = 1e-8

    def test_random_returns(self, random_returns):
        ret_pd = pd.Series(random_returns)
        ref = qs.stats.rolling_sharpe(ret_pd, rolling_period=63)
        got = nanobook.py_rolling_sharpe(random_returns.tolist(), 63, 252)

        # Compare only where both are valid
        ref_arr = ref.values if hasattr(ref, "values") else np.array(ref)
        got_arr = np.array(got)
        valid = ~np.isnan(ref_arr) & ~np.isnan(got_arr)
        if valid.any():
            np.testing.assert_allclose(got_arr[valid], ref_arr[valid], atol=self.ATOL)


class TestRollingVolatilityReference:
    """Validate nanobook rolling volatility against quantstats."""

    ATOL = 1e-8

    def test_random_returns(self, random_returns):
        ret_pd = pd.Series(random_returns)
        ref = qs.stats.rolling_volatility(ret_pd, rolling_period=63)
        got = nanobook.py_rolling_volatility(random_returns.tolist(), 63, 252)

        ref_arr = ref.values if hasattr(ref, "values") else np.array(ref)
        got_arr = np.array(got)
        valid = ~np.isnan(ref_arr) & ~np.isnan(got_arr)
        if valid.any():
            np.testing.assert_allclose(got_arr[valid], ref_arr[valid], atol=self.ATOL)
