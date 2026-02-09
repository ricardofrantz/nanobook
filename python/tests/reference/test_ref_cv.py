"""Reference tests: nanobook CV splits vs scikit-learn.

Validates TimeSeriesSplit index generation against sklearn.

Dev dependencies: scikit-learn
"""

import pytest

try:
    from sklearn.model_selection import TimeSeriesSplit

    HAS_SKLEARN = True
except ImportError:
    HAS_SKLEARN = False

import nanobook

pytestmark = pytest.mark.skipif(not HAS_SKLEARN, reason="scikit-learn not installed")


class TestTimeSeriesSplitReference:
    """Validate nanobook time_series_split against sklearn."""

    @pytest.mark.parametrize(
        "n_samples,n_splits",
        [
            (10, 3),
            (50, 5),
            (100, 5),
            (100, 10),
            (1000, 5),
        ],
    )
    def test_exact_index_match(self, n_samples, n_splits):
        """Indices must be bit-exact (no tolerance — pure integer arithmetic)."""
        tscv = TimeSeriesSplit(n_splits=n_splits)
        import numpy as np

        X = np.arange(n_samples)

        ref_splits = list(tscv.split(X))
        got_splits = nanobook.py_time_series_split(n_samples, n_splits)

        assert len(got_splits) == len(ref_splits), (
            f"fold count mismatch: {len(got_splits)} vs {len(ref_splits)}"
        )

        for i, ((ref_train, ref_test), (got_train, got_test)) in enumerate(
            zip(ref_splits, got_splits)
        ):
            assert got_train == list(ref_train), f"train mismatch at fold {i}"
            assert got_test == list(ref_test), f"test mismatch at fold {i}"

    def test_single_split(self):
        # sklearn >= 1.8 requires n_splits >= 2; nanobook matches this constraint
        got = nanobook.py_time_series_split(10, 1)
        assert len(got) == 0, "n_splits=1 should return empty (matching sklearn constraint)"

    def test_expanding_window(self):
        splits = nanobook.py_time_series_split(100, 5)
        for i in range(1, len(splits)):
            assert len(splits[i][0]) > len(splits[i - 1][0]), "train should expand"
            assert len(splits[i][1]) == len(splits[0][1]), "test size should be constant"
