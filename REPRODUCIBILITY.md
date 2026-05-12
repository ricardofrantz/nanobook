# Reproducibility

## v0.11 ITCH replay source

Selected source file: `07302019.NASDAQ_ITCH50.gz`

- URL: `https://emi.nasdaq.com/ITCH/Nasdaq%20ITCH/07302019.NASDAQ_ITCH50.gz`
- Date represented: 2019-07-30
- Compressed size advertised by NASDAQ: 3,662,140,094 bytes
- MD5: `8744aba2ea125bfdde1a340ee2cea924`
- SHA-256: `c65784c48c28735901ae442dc00e215834218a359bc12a139ab4eec209bc2d4a`
- HTTP verification: `200 OK`, `content-type: application/x-gzip`, `accept-ranges: bytes`

Rationale: this is the smallest complete post-2019-01-30 `NASDAQ_ITCH50.gz` trading-day file currently advertised in NASDAQ's public ITCH archive, keeping local replay setup lighter while still exercising a full modern cancel-heavy TotalView-ITCH day.

NASDAQ's directory listing advertises `07302019.NASDAQ_ITCH50.gz.md5sum`, but direct checksum fetches currently return `404`; `download.sh` therefore verifies against the committed MD5 above, computed from a full download of the selected file.

## v0.11 data licensing stance

`examples/itch-replay/download.sh` must fetch ITCH data from NASDAQ at user runtime. The repository must not commit raw `*.NASDAQ_ITCH50.gz` files, decompressed ITCH bytes, or derived byte slices containing NASDAQ message payloads.

Public sources checked:

- NASDAQ public ITCH archive: `https://emi.nasdaq.com/ITCH/Nasdaq%20ITCH/`
- NASDAQ TotalView-ITCH product page: `https://www.nasdaqtrader.com/trader.aspx?id=itch`
- NASDAQ Global Data Agreement search result: `https://www.nasdaq.com/docs/globaldataagreement.pdf`
- NASDAQ Basic/QBBO product references: `https://www.nasdaq.com/solutions/data/equities/nasdaq-basic`

Implementation rule: source code, scripts, checksums, aggregate statistics, and generated reports may be committed; raw ITCH data and Nasdaq Basic/QBBO reference data may not. QBBO/Nasdaq Basic is a separate entitlement and must not be used as the v0.11 verification target.

## v0.11 CI slice parameters

- Source: `07302019.NASDAQ_ITCH50.gz`
- Window: 09:30:00 through 09:30:59.999999999 ET
- `--start-ns`: `34200000000000`
- `--duration-ns`: `60000000000`
- Local output name: `07302019-0930-0931.itch`
- Local output size: 101,698,706 bytes
- SHA-256: `75fd777510c27ad1a8015cd482ec09d7703e2a79c43093f871886a3de3bb964e`

The raw CI slice is intentionally generated under `examples/itch-replay/data/`, which is git-ignored, rather than committed.

## v0.11 command sequence

```sh
bash examples/itch-replay/download.sh
gzip -dc examples/itch-replay/data/07302019.NASDAQ_ITCH50.gz \
  | cargo run --bin itch-slice -- \
      --input - \
      --output examples/itch-replay/data/07302019-0930-0931.itch \
      --start-ns 34200000000000 \
      --duration-ns 60000000000
cd examples/itch-replay/data && shasum -a 256 -c ../expected/slice.sha256
cargo run --release --features itch --example itch-replay -- \
  --input examples/itch-replay/data/07302019-0930-0931.itch \
  --output-dir examples/itch-replay/data/replay-v2 \
  --warmup 1000
```

### Report generation

```sh
cd examples/itch-replay
uv venv
uv pip install -e ../../python
uv run report.py --input data/replay-v2/event-log.jsonl --output data/replay-v2/report.html
```

## v0.11 reference environment

- Hardware: Apple M1 Pro, 16 GB RAM
- Model: MacBook Pro 18,3 (Z15G003L8SM/A)
- OS: macOS 26.3.1 (a), build 25D771280a
- Rust: `rustc 1.95.0 (59807616e 2026-04-14)`
- Python: `Python 3.14.1`
- uv: `uv 0.11.12 (Homebrew 2026-05-08 aarch64-apple-darwin)`
- `python/uv.lock` SHA-256: `0482e5b7b4af766ab43a00a37348a84ff39ec31a91c8a737439cf18f761424e7`

## v0.11 measured performance

Measured from the reference environment run (N=973,285 measured events, 1,000 warmup events excluded):

| Stage | p50 latency | p95 latency | p99 latency |
|-------|-------------|-------------|-------------|
| ITCH parse | 83 ns | 166 ns | 417 ns |
| LOB book-update | 250 ns | 1,042 ns | 3,541 ns |

These numbers are published in the README Performance section and reflect steady-state operation after excluding the first 1,000 warmup events from the 974,288 total events processed.

## v0.11 verification

Reproducibility is verified by:
1. CI smoke job (`.github/workflows/ci.yml` - `examples-smoke`) that runs the full command sequence on every push
2. SHA-256 validation of the ITCH slice against `examples/itch-replay/expected/slice.sha256`
3. Deterministic replay: same input produces identical event-log.jsonl output
4. Performance regression detection: CI job fails if output files are not generated

Cross-platform verification: To be completed by running the command sequence on a different machine (Linux x86_64) and confirming performance numbers are within expected variance (±20% due to hardware differences).
