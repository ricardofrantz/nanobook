# Versioning Policy

nanobook uses Semantic Versioning, with an explicit pre-1.0 policy.

While nanobook is in `0.x`, minor releases may include breaking changes. In
practice, this means `0.14.0` to `0.15.0` may break public APIs, schemas,
configuration formats, or documented behavior. Patch releases, such as
`0.14.0` to `0.14.1`, should remain compatible except where a security or
correctness fix makes preserving the old behavior impossible.

This differs from a 1.0 SemVer promise. After a hypothetical `1.0.0`, breaking
changes would normally require a major version bump, such as `1.0.0` to
`2.0.0`; minor releases would add compatible functionality, and patch releases
would fix compatible bugs.

## Why nanobook is still 0.x

The public surface is still being shaped. The API surface audit in
[`docs/api-surface-audit.md`](docs/api-surface-audit.md) found that public Rust
visibility currently exposes more than the long-term stable API should probably
promise, especially in broker adapter internals, rebalancer operations code,
and some Python wrapper exports. It also found incomplete documentation and
deprecation hygiene across parts of the Rust and Python surfaces.

Staying in `0.x` keeps this honest: users can depend on releases, but should
not read minor-version compatibility as a 1.0-style stability guarantee.

## Public API during 0.x

For versioning purposes, public API includes:

- Public Rust items exported from the workspace crates, including `pub`
  modules, structs, enums, traits, functions, methods, type aliases, constants,
  and feature-gated public items.
- Python exports from the `nanobook` Python package, including exported
  classes, functions, module attributes, and documented argument or return-value
  behavior.
- Documented command-line interfaces, documented file inputs, documented file
  outputs, and documented operational workflows.
- Event-log schemas, including documented field names, required fields, field
  meanings, enum/string values, ordering assumptions, and schema-version
  behavior.
- Configuration formats, including documented keys, value types, defaults,
  required fields, and validation behavior.
- Documented behavior that users may reasonably build against, even when the
  behavior is not encoded as a type signature.

## Public API baselines

The Rust public API baselines in [`docs/public-api/`](docs/public-api/) are
generated with `cargo-public-api` for the workspace crates. They are a
documentation and review aid: changes to the text make exported Rust surface
changes visible in code review and release preparation.

These baselines are not a 1.0 stability contract. During `0.x`, minor releases
may still make breaking public API changes, but those changes should be
intentional, visible in the baseline diff, and called out in `CHANGELOG.md`
when they affect users.

## Minimum supported Rust version

nanobook's minimum supported Rust version (MSRV) is the `rust-version` declared
in [`Cargo.toml`](Cargo.toml). The MSRV is part of the public compatibility
policy: patch releases should not raise it unless a security or serious
correctness fix makes the bump unavoidable.

During `0.x`, MSRV increases may happen in minor releases. They should be
called out in `CHANGELOG.md`, and CI should keep an explicit build check for
the declared MSRV so accidental increases are caught before release.

## Breaking changes during 0.x

In `0.x`, these changes may appear in minor releases and should be called out
in `CHANGELOG.md`:

- Removing, renaming, or changing the signature of public Rust items.
- Changing public Rust trait requirements, enum variants, struct fields,
  feature flags, error types, or error conditions in a way that can break
  downstream code.
- Removing, renaming, or changing Python exports, arguments, return types,
  exceptions, warnings, or observable object attributes.
- Changing documented CLI flags, config keys, config defaults, required config
  fields, or validation rules.
- Changing event-log schemas, documented JSON fields, schema-version handling,
  or replay semantics.
- Changing documented behavior in ways that can alter user results, such as
  order matching semantics, stop behavior, risk checks, portfolio accounting,
  audit-log interpretation, or broker/rebalancer safety behavior.
- Tightening validation when existing accepted inputs become rejected, unless
  the old acceptance was clearly a bug or unsafe behavior.

Patch releases should avoid these changes unless they are necessary to fix a
security issue, data-corruption risk, or serious correctness bug. When that
happens, the changelog should say so explicitly.

## Not covered

The compatibility promise does not cover:

- Private Rust items, private modules, and non-exported helper functions.
- Rust items marked `#[doc(hidden)]`.
- Test-only, fuzz-only, benchmark-only, or CI-only code.
- Undocumented internal modules or operational plumbing that is not exported as
  part of a documented interface, even if users can inspect it in the source.
- Byte-level formatting details that are not documented as part of a stable
  format. For example, the event-log schema is covered, but whitespace,
  object-key ordering, and incidental serializer output are not covered unless
  explicitly documented.
- Implementation details such as data structures, algorithm choices,
  allocation behavior, caching, batching, retry internals, logging text, or
  performance characteristics, unless a document explicitly makes them part of
  the interface.
- Behavior that depends on external services, broker APIs, exchange behavior,
  network timing, or platform-specific runtime details outside nanobook's
  control.

## Hypothetical 1.0 policy

If nanobook reaches `1.0.0`, the policy should become stricter:

- `MAJOR` version changes would be reserved for breaking changes.
- `MINOR` version changes would add backward-compatible APIs or behavior.
- `PATCH` version changes would be limited to backward-compatible fixes.
- Public Rust visibility would need to be intentionally curated, documented,
  and treated as stable unless clearly marked otherwise.
- Python exports, documented event-log schemas, and documented config formats
  would need migration paths or deprecation periods before removal.

The audit in [`docs/api-surface-audit.md`](docs/api-surface-audit.md) describes
the cleanup needed before that promise would be credible: narrower public
visibility, clearer public/internal boundaries, stronger documentation, and
more consistent deprecation handling.

## Practical guidance for users

- Pin nanobook to a specific version, especially in production-like systems.
  For Rust, prefer an exact version when compatibility matters:

  ```toml
  nanobook = "=0.14.0"
  ```

- For Python, pin exact versions in the environment or lockfile:

  ```text
  nanobook==0.14.0
  ```

- For experiments that must remain reproducible, fork or branch from a release
  tag and keep your local changes on top of that tag.
- Read [`CHANGELOG.md`](CHANGELOG.md) before upgrading across minor versions.
  Breaking changes in `0.x` should be listed there.
- Test upgrades with your own event logs, configs, broker/rebalancer dry runs,
  and portfolio outputs before using a newer minor release for real workflows.
