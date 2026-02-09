"""Shared fixtures for reference tests.

Reference tests validate nanobook's Rust implementations against
established Python libraries (ta-lib, scipy, scikit-learn, quantstats).
These libraries are dev-only dependencies — test oracles, not runtime deps.
"""

import numpy as np
import pytest


@pytest.fixture
def rng():
    """Seeded random number generator for reproducible tests."""
    return np.random.default_rng(42)


@pytest.fixture
def random_close(rng):
    """Simulated daily close prices (geometric random walk, ~1000 bars)."""
    daily_returns = rng.normal(0.0005, 0.015, size=1000)
    prices = 100.0 * np.cumprod(1.0 + daily_returns)
    return prices


@pytest.fixture
def random_ohlc(random_close):
    """Simulated OHLC data derived from random close prices."""
    close = random_close
    high = close * (1 + np.abs(np.random.default_rng(43).normal(0, 0.005, size=len(close))))
    low = close * (1 - np.abs(np.random.default_rng(44).normal(0, 0.005, size=len(close))))
    return high, low, close


@pytest.fixture
def random_returns(rng):
    """500 simulated daily returns for metric tests."""
    return rng.normal(0.0003, 0.012, size=500)
