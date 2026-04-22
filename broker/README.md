# nanobook-broker

Broker-abstraction layer for nanobook, with IBKR (Interactive Brokers
TWS API) and Binance (spot REST) adapters behind feature flags.

## Features

| Feature | Gated adapter / behaviour |
|---------|---------------------------|
| `ibkr` | IBKR via ibapi's blocking client |
| `binance` | Binance spot REST via reqwest::blocking |
| `rustls` (default) | rustls TLS backend for the Binance HTTP client |
| `native-tls` | Opt-in system-OpenSSL TLS backend |
| `strict-market-reject` | Reject all market orders (IBKR only) |

## Credential handling

`BinanceBroker` and its internal `BinanceClient` hold API keys and
secrets as `String` fields. Both derive
[`zeroize::ZeroizeOnDrop`](https://docs.rs/zeroize/latest/zeroize/trait.ZeroizeOnDrop.html),
which writes zeros over the heap allocations backing those strings
before the allocator reclaims them. This closes the
allocator-reuse / core-dump window for post-drop credential
inspection.

### What zeroization does NOT protect against

1. **Runtime memory reads.** An attacker with read access to the
   live process can snoop the key while it is in use (during
   request signing, inside reqwest's internal buffers, etc.).
   Zeroization only applies when the owning struct drops.
2. **Copies made elsewhere.** HMAC signing (`auth::sign`) reads
   `secret_key` and produces a signature — intermediate buffers
   inside the crypto library are out of scope for broker-side
   zeroization.
3. **PyO3 `&str` originals.** When a Python caller does
   `BinanceBroker("key", "secret")`, those strings live in a
   `PyString` owned by the Python interpreter. PyO3 hands Rust a
   `&str` that borrows from `PyString` storage. The Rust side
   zeroizes *its copy* on drop; the original `PyString` bytes
   remain in Python's heap until the interpreter reclaims them,
   which (for interned or short-lived strings) can be arbitrarily
   far in the future.

### Recommendation: pass credentials via environment variables

Read credentials directly from the process environment on the Rust
side, not through a Python-string argument:

```rust
use std::env;

let api_key = env::var("BINANCE_API_KEY").expect("BINANCE_API_KEY");
let secret_key = env::var("BINANCE_SECRET_KEY").expect("BINANCE_SECRET_KEY");
let broker = BinanceBroker::new(&api_key, &secret_key, false);
// `api_key` and `secret_key` get zeroized when they drop at end of scope.
```

Or, from Python, expose a factory that reads `os.environ` and
forwards nothing sensitive through the binding's argument list —
the raw bytes then never transit a `PyString`.

## IBKR authentication (no credentials in memory)

`IbkrClient` does not hold any credentials: TWS authentication
happens at the socket layer via `(host, port, client_id)`. There is
nothing the broker layer can scrub, so no `ZeroizeOnDrop` derive is
present. If you add a field that stores a secret (OAuth token, API
cookie, etc.) in a future refactor, re-evaluate — ibapi's own
`Client` type does not implement `Zeroize` and would need
`#[zeroize(skip)]` on any outer derive.

## TLS backend selection

Default builds link rustls — pure Rust, no transitive OpenSSL.
`--features native-tls` (with `--no-default-features`) opts into
system-OpenSSL for environments that rely on `OPENSSL_CONF` / custom
CA bundles. See `CHANGELOG.md` (S1) for the upstream reasoning.
