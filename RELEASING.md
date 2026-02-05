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

## Platforms Built

| Target | OS | Architecture |
|--------|-----|--------------|
| `x86_64-unknown-linux-gnu` | Linux | x86_64 |
| `x86_64-unknown-linux-musl` | Linux | x86_64 (static) |
| `aarch64-unknown-linux-gnu` | Linux | ARM64 |
| `x86_64-apple-darwin` | macOS | Intel |
| `aarch64-apple-darwin` | macOS | Apple Silicon |
| `x86_64-pc-windows-msvc` | Windows | x86_64 |

## Manual Release (if needed)

```bash
# Publish to crates.io manually
cargo publish

# Build release binary locally
cargo build --release --bin lob
```

## Installation Methods

After release, users can install via:

```bash
# From crates.io (compiles from source)
cargo install limit-order-book

# From crates.io (pre-built binary, faster)
cargo binstall limit-order-book

# From GitHub releases (direct download)
# Download from https://github.com/ricardofrantz/limit-order-book-rs/releases
```
