"""Reference-parity golden fixture generator.

Produces tests/parity/golden.json from scipy, TA-Lib, and quantstats.

This script is run MANUALLY, not in CI. The generated JSON is
checked into the repository and read-only from the Rust test side.
Regenerate only when reference library versions in
tests/parity/requirements.txt are deliberately bumped.

Usage:
    uv pip install -r tests/parity/requirements.txt
    uv run python tests/parity/generate_golden.py

System prerequisites:
    macOS:  brew install ta-lib
    Ubuntu: apt-get install libta-lib-dev
"""

from __future__ import annotations

import hashlib
import json
import sys
from pathlib import Path

import numpy as np
import quantstats as qs
import scipy.stats as sps
import talib

SEED = 42
N = 500
RETURN_SCALE = 0.01


def to_jsonable(value):
    """Convert numpy scalars / arrays to JSON-compatible primitives.

    NaN and +/-Inf become None (JSON null) so the Rust side can
    distinguish "not yet computed" (first `period - 1` indicator
    values) from real finite outputs.
    """
    if isinstance(value, (float, np.floating)):
        f = float(value)
        if not np.isfinite(f):
            return None
        return f
    if isinstance(value, (int, np.integer)):
        return int(value)
    if isinstance(value, (list, np.ndarray)):
        return [to_jsonable(v) for v in value]
    return value


def main() -> int:
    # Seeded inputs. NEVER change SEED or N without a deliberate
    # decision; every regenerated value depends on them.
    rng = np.random.default_rng(SEED)
    returns = rng.standard_normal(N) * RETURN_SCALE

    # Synthetic OHLC series derived from the same returns. High/low
    # bands are small perturbations around close so that ATR has
    # non-trivial signal.
    close = 100.0 * np.cumprod(1.0 + returns)
    highs = close * (1.0 + 0.002 * rng.random(N))
    lows = close * (1.0 - 0.002 * rng.random(N))

    # --- scipy.stats ---
    spearman_self = sps.spearmanr(returns, returns).statistic
    shuffled = np.roll(returns, 7)
    spearman_shuffled = sps.spearmanr(returns, shuffled).statistic

    # --- TA-Lib ---
    talib_rsi_14 = talib.RSI(close, timeperiod=14)
    talib_atr_14 = talib.ATR(highs, lows, close, timeperiod=14)

    # --- quantstats ---
    # quantstats.stats expects a pandas Series with a DatetimeIndex for
    # drawdown computations (it subtracts time deltas from the index).
    # Use a business-day index anchored at 2023-01-01 — the specific
    # date is irrelevant to the numeric outputs.
    import pandas as pd

    idx = pd.date_range("2023-01-01", periods=N, freq="B")
    returns_series = pd.Series(returns, index=idx)
    qs_sharpe = qs.stats.sharpe(returns_series, rf=0.0, periods=252, annualize=True)
    qs_sortino = qs.stats.sortino(returns_series, rf=0.0, periods=252, annualize=True)
    qs_max_dd = qs.stats.max_drawdown(returns_series)
    # quantstats's expected_shortfall is a *hybrid*: parametric-normal
    # VaR threshold, then empirical mean of returns below it. Nanobook
    # exposes this under CVaRMethod::ParametricNormal (the v0.9.3
    # default). From v0.10, the default is CVaRMethod::Historical —
    # pure empirical, matches the standard academic convention.
    qs_cvar_95_parametric = qs.stats.expected_shortfall(
        returns_series, confidence=0.95
    )

    # Pure empirical CVaR: sort, take the lowest ceil(n * alpha), mean.
    # This is the new default (CVaRMethod::Historical) in v0.10.
    alpha = 0.05
    sorted_returns = np.sort(returns)
    tail_n = int(np.ceil(N * alpha))
    empirical_cvar_95 = float(sorted_returns[:tail_n].mean())

    # Reference library versions — recorded so future regenerations
    # can detect drift without re-running the script.
    import scipy

    versions = {
        "numpy": np.__version__,
        "scipy": scipy.__version__,
        "talib": getattr(talib, "__version__", "unknown"),
        "quantstats": getattr(qs, "__version__", "unknown"),
        "pandas": pd.__version__,
    }

    out = {
        "_meta": {
            "seed": SEED,
            "n": N,
            "return_scale": RETURN_SCALE,
            "versions": versions,
            "note": (
                "Regenerate only when requirements.txt is deliberately "
                "bumped. See tests/parity/README.md."
            ),
        },
        "inputs": {
            "returns": to_jsonable(returns),
            "close": to_jsonable(close),
            "highs": to_jsonable(highs),
            "lows": to_jsonable(lows),
        },
        "scipy": {
            "spearman_self_correlation": to_jsonable(spearman_self),
            "spearman_shuffled_correlation": to_jsonable(spearman_shuffled),
        },
        "talib": {
            "rsi_14": to_jsonable(talib_rsi_14),
            "atr_14": to_jsonable(talib_atr_14),
        },
        "quantstats": {
            "sharpe_annual_252": to_jsonable(qs_sharpe),
            "sortino_annual_252": to_jsonable(qs_sortino),
            "max_drawdown": to_jsonable(qs_max_dd),
            "cvar_95_parametric": to_jsonable(qs_cvar_95_parametric),
        },
        "empirical": {
            # Pure-empirical Historical CVaR at 95% confidence: mean of
            # the lowest ceil(N * 0.05) returns. Matches nanobook's
            # CVaRMethod::Historical (the v0.10 default).
            "cvar_95": empirical_cvar_95,
        },
    }

    path = Path(__file__).parent / "golden.json"
    path.write_text(json.dumps(out, indent=2) + "\n")

    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    print(f"Wrote {path}")
    print(f"sha256: {digest}")
    print(f"Reference versions: {versions}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
