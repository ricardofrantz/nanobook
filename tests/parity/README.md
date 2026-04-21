# Reference-parity golden fixture

This directory holds the input fixture and pre-computed reference
outputs that `tests/reference_parity.rs` compares nanobook's
implementation against. Every numeric claim in the README (RSI, ATR,
Sharpe, Sortino, CVaR, max drawdown, etc.) is pinned here.

## Files

- `requirements.txt` — pinned Python reference-library versions.
- `generate_golden.py` — seeded fixture generator. Run manually.
- `golden.json` — generated output. **Check in.** Read-only from CI.
- `README.md` — this file.

## Regeneration (manual only)

Regenerate only when `requirements.txt` is deliberately bumped or a
fixture parameter (seed, N) is changed with intent.

**System prerequisites.**

- macOS: `brew install ta-lib`
- Ubuntu: `apt-get install libta-lib-dev`

**Procedure.**

```bash
# From the repository root.
uv venv .parity-venv
source .parity-venv/bin/activate
uv pip install -r tests/parity/requirements.txt
python tests/parity/generate_golden.py
deactivate
rm -rf .parity-venv
```

The script prints the SHA-256 of the generated `golden.json` and the
exact reference-library versions used. Commit the updated JSON in the
same commit as the `requirements.txt` bump, alongside the Rust-side
tolerance adjustments if any are needed.

## Philosophy

This harness is the measurement substrate for every numerical fix in
nanobook. Each fix commit either:

- Adds a new test that compares a specific function to a known
  reference, or
- Turns a previously-failing test green (because the fix aligns
  nanobook with the reference).

**Never loosen tolerance to make a test pass.** If a 1e-6 test fails
with our current output differing from the reference by 1e-5, either
the reference convention is different (document it, pick a different
reference) or nanobook has a bug (fix it, don't hide it).

## Fixture structure

`golden.json` is nested:

```
{
  "_meta": { "seed": 42, "n": 500, "versions": { ... } },
  "inputs": {
    "returns":  [N floats],
    "close":    [N floats],
    "highs":    [N floats],
    "lows":     [N floats]
  },
  "scipy":      { ... },
  "talib":      { ... },
  "quantstats": { ... }
}
```

Any `null` value in an array represents a NaN / non-finite in the
reference output (e.g., TA-Lib's RSI is undefined for the first
`period` indices). The Rust side reads these as `None`.

## Drift policy

If a Rust test fails after a library bump:

1. Read `_meta.versions` in `golden.json` — what moved?
2. Run the upstream changelog / release notes for the bumped
   library.
3. If the reference library changed numerics deliberately (rare),
   update the Rust-side expected behavior explicitly in the same
   commit.
4. If the reference library has a bug, pin the previous version in
   `requirements.txt` and wait for a fix upstream.

Never silently regenerate `golden.json` without understanding the
delta.
