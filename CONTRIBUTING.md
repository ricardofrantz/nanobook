# Contributing to rustbook

Thank you for your interest in contributing to rustbook!

## Getting Started

1. Fork the repository
2. Clone your fork: `git clone https://github.com/YOUR_USERNAME/rustbook.git`
3. Create a branch: `git checkout -b feature/your-feature`
4. Make your changes
5. Run tests: `cargo test --all-features`
6. Run lints: `cargo fmt && cargo clippy -- -D warnings`
7. Commit and push
8. Open a pull request

## Development Setup

```bash
# Install Rust (if needed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Clone and build
git clone https://github.com/ricardofrantz/rustbook.git
cd rustbook
cargo build

# Run tests
cargo test --all-features

# Run benchmarks
cargo bench

# Try the CLI
cargo run --bin lob
```

## Code Style

- Follow standard Rust conventions
- Run `cargo fmt` before committing
- All code must pass `cargo clippy -- -D warnings`
- Add tests for new functionality
- Update documentation for API changes

## Pull Request Guidelines

- Keep PRs focused on a single change
- Include tests for bug fixes and new features
- Update CHANGELOG.md for user-facing changes
- Ensure CI passes before requesting review

## Testing

```bash
# Run all tests
cargo test --all-features

# Run with no default features (no event logging)
cargo test --no-default-features

# Run benchmarks
cargo bench
```

## Architecture

See [SPECS.md](SPECS.md) for the technical specification. Key modules:

| Module | Purpose |
|--------|---------|
| `types.rs` | Core types (Price, Quantity, etc.) |
| `order.rs` | Order struct and status |
| `level.rs` | FIFO queue at a single price |
| `price_levels.rs` | One side of the book |
| `book.rs` | Complete order book |
| `matching.rs` | Price-time priority algorithm |
| `exchange.rs` | High-level API |
| `event.rs` | Event log for replay |

## Reporting Issues

- Search existing issues first
- Include Rust version (`rustc --version`)
- Provide minimal reproduction steps
- Include expected vs actual behavior

## License

By contributing, you agree that your contributions will be licensed under the MIT License.
