# nanobook-fuzz

libFuzzer / cargo-fuzz harness for the nanobook matching engine
and ITCH parser.

## Why a separate workspace

Fuzzing requires `libfuzzer-sys`, which currently compiles only on
a nightly Rust toolchain. Rather than forcing the entire nanobook
workspace to tolerate nightly, `fuzz/` is its own one-crate
workspace, excluded from the root workspace via
`exclude = ["fuzz"]` in the top-level `Cargo.toml`. Stable builds
of nanobook never see this crate.

## Prerequisites

```bash
rustup toolchain install nightly
cargo install cargo-fuzz --locked
```

## Targets

### `fuzz_submit` — Exchange submit / cancel / modify path

Drives a fresh `Exchange` with an arbitrary sequence of
`SubmitLimit`, `SubmitMarket`, `Cancel`, and `Modify` actions.
Asserts after each step:

- No panic anywhere in the matching engine, level accounting, or
  stop-order cascade.
- Book never crossed (`best_bid < best_ask` whenever both sides
  are populated).
- Order IDs strictly monotonic with submission order — including
  FOK-rejected orders (the ghost-id contract from N7).

### `fuzz_itch` — ITCH 5.0 parser

Feeds arbitrary bytes to `ItchParser::next_message` and drains
the stream up to 64 messages. Asserts the parser never panics on
malformed input — the DoS contract established by S3.

## Running

```bash
# Quick smoke run (roughly 10-60 seconds).
cargo +nightly fuzz run fuzz_submit -- -runs=100000
cargo +nightly fuzz run fuzz_itch   -- -runs=100000

# Long soak (overnight or longer).
cargo +nightly fuzz run fuzz_submit -- -runs=10000000
cargo +nightly fuzz run fuzz_itch   -- -runs=10000000
```

libFuzzer builds a corpus under `fuzz/corpus/<target>/` as it
finds coverage-increasing inputs; reuse that corpus across runs
by default. Crash-reproducing artifacts land in
`fuzz/artifacts/<target>/`. Reproduce a specific crash:

```bash
cargo +nightly fuzz run fuzz_submit fuzz/artifacts/fuzz_submit/crash-abcdef
```

## Not CI-gated

These targets are not run in the GitHub Actions pipeline. Fuzz
testing is a long-running discovery activity, not a per-commit
gate — the cost/value curve for a 10-minute CI slot spent
fuzzing is much worse than a 10-hour nightly job on a dedicated
machine. See `plan_v0.10.md` (I2) for rationale.
