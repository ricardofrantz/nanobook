"""Reference tests: nanobook stats vs SciPy.

Validates Spearman correlation and quintile spread against scipy/numpy.

Dev dependencies: scipy, numpy
"""

import numpy as np
import pytest

try:
    from scipy import stats

    HAS_SCIPY = True
except ImportError:
    HAS_SCIPY = False

import nanobook

pytestmark = pytest.mark.skipif(not HAS_SCIPY, reason="scipy not installed")


class TestSpearmanReference:
    """Validate nanobook Spearman against SciPy."""

    ATOL = 1e-10

    def test_random_data(self, rng):
        x = rng.standard_normal(100).tolist()
        y = rng.standard_normal(100).tolist()
        ref_corr, ref_p = stats.spearmanr(x, y)
        got_corr, got_p = nanobook.py_spearman(x, y)
        assert abs(got_corr - ref_corr) < self.ATOL
        assert abs(got_p - ref_p) < self.ATOL

    def test_perfect_positive(self):
        x = list(range(50))
        y = list(range(50))
        ref_corr, ref_p = stats.spearmanr(x, y)
        got_corr, got_p = nanobook.py_spearman(
            [float(v) for v in x], [float(v) for v in y]
        )
        assert abs(got_corr - 1.0) < 1e-10
        assert got_p < 1e-10

    def test_perfect_negative(self):
        x = list(range(50))
        y = list(range(49, -1, -1))
        got_corr, got_p = nanobook.py_spearman(
            [float(v) for v in x], [float(v) for v in y]
        )
        assert abs(got_corr - (-1.0)) < 1e-10

    def test_tied_values(self):
        x = [1.0, 1.0, 2.0, 2.0, 3.0]
        y = [5.0, 4.0, 3.0, 2.0, 1.0]
        ref_corr, ref_p = stats.spearmanr(x, y)
        got_corr, got_p = nanobook.py_spearman(x, y)
        assert abs(got_corr - ref_corr) < self.ATOL
        assert abs(got_p - ref_p) < 1e-6  # p-value tolerance slightly wider

    def test_small_n(self):
        x = [1.0, 2.0, 3.0]
        y = [3.0, 1.0, 2.0]
        ref_corr, ref_p = stats.spearmanr(x, y)
        got_corr, got_p = nanobook.py_spearman(x, y)
        assert abs(got_corr - ref_corr) < self.ATOL


class TestQuintileSpreadReference:
    """Validate quintile spread against manual numpy computation."""

    def test_known_spread(self):
        scores = [float(i) for i in range(100)]
        returns = [float(i) * 0.001 for i in range(100)]
        got = nanobook.py_quintile_spread(scores, returns, 5)

        # Manual: bottom 20 = 0..19, top 20 = 80..99
        bottom = np.mean([i * 0.001 for i in range(20)])
        top = np.mean([i * 0.001 for i in range(80, 100)])
        expected = top - bottom
        assert abs(got - expected) < 1e-12

    def test_inverse_scores(self):
        scores = [float(i) for i in range(100)]
        returns = [float(99 - i) * 0.001 for i in range(100)]
        got = nanobook.py_quintile_spread(scores, returns, 5)
        assert got < 0.0  # inverse → negative spread
