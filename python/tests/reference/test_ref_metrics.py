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
    """Validate nanobook CVaR against the empirical reference.

    From v0.10, nanobook's default CVaR method is `Historical` (pure
    empirical: mean of the lowest `ceil(n * alpha)` returns). quantstats's
    `qs.stats.cvar` uses a hybrid parametric-normal VaR threshold + empirical
    tail mean; the two values coincide for well-behaved normal samples but
    diverge on skewed or small samples.

    This test pins the Historical result against a pure empirical reference
    computed directly from the sample. For the legacy hybrid behavior, see
    the Rust-side `cvar_parametric_matches_quantstats` test.
    """

    ATOL = 1e-12  # Bit-level: both sides compute sort, slice, mean.

    def test_random_returns_empirical(self, random_returns):
        """Historical CVaR = mean of the lowest ceil(n * alpha) returns."""
        n = len(random_returns)
        alpha = 0.05
        tail_n = int(np.ceil(n * alpha))
        sorted_returns = np.sort(random_returns)
        ref = float(sorted_returns[:tail_n].mean())

        m = nanobook.py_compute_metrics(random_returns.tolist(), 252.0, 0.0)
        assert abs(m.cvar_95 - ref) < self.ATOL, (
            f"cvar_95={m.cvar_95}, empirical_ref={ref}, diff={m.cvar_95 - ref}"
        )


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
