# Releasing

## Prerequisites

1. Add `CARGO_REGISTRY_TOKEN` to GitHub repo secrets:
   - Get token from https://crates.io/settings/tokens
   - Add to repo: Settings → Secrets → Actions → New secret

## Release Process

```bash
# 1. Update version in Cargo.toml
vim Cargo.toml  # Change version = "0.1.0" to "0.2.0" etc.

# 2. Commit the version bump
git add Cargo.toml
git commit -m "Release v0.2.0"

# 3. Create and push tag
git tag v0.2.0
git push origin master
git push origin v0.2.0
```

GitHub Actions will automatically:
- Build binaries for 6 platforms (Linux, macOS, Windows)
- Create GitHub Release with downloadable binaries
- Publish to crates.io
- Build and publish Python wheels to PyPI

## Benchmark Baselines

To maintain performance across releases, capture a baseline for major versions:

```bash
# Capture v0.5 baseline
cargo bench --save-baseline v0.5
```

CI will store these baselines as artifacts to compare performance in future PRs.

## Installation Methods

After release, users can install via:

```bash
# Python (PyPI)
pip install nanobook

# Rust (crates.io - compiles from source)
cargo install nanobook
```
