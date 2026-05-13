# Why nanobook stays 0.x

nanobook is intentionally staying pre-1.0 for now. This is not a signal that the
project is experimental in every dimension: the core matching, portfolio, risk,
and broker workflows are tested and usable. It is a statement about the public
API promise that a 1.0 release would make.

The current public-surface audit in
[`docs/api-surface-audit.md`](api-surface-audit.md) shows that nanobook still
needs more time before it can responsibly promise long-term API stability.

## 1. What 1.0 means

For this project, 1.0 should mean more than "the code works today." It should
mean that the public Rust crates, Python bindings, rebalancer interfaces, event
schemas, and documented workflows are stable enough that users can build on
them without expecting frequent breaking changes.

A 1.0 release would imply that:

- public Rust items are intentional API, not accidental exports;
- Python functions, classes, and deprecations have a clear compatibility policy;
- schemas and operational workflows are stable enough for downstream automation;
- documentation completeness is treated as a release gate;
- breaking changes are rare, deliberate, and communicated ahead of time.

That is a higher bar than the project should claim today.

## 2. Why 0.x is right

nanobook is maintained as a focused, solo-maintained execution kernel. That
shape is a strength: the project can stay small, direct, and honest about what
it supports. It also means the project should not over-promise indefinite
compatibility before the abstractions have settled.

The audit found several concrete reasons to keep the project in 0.x:

- No crate currently enforces `missing_docs`, so documentation completeness is
  best-effort rather than a mechanical release gate.
- Several public items look operational or internal, especially in `broker` and
  `rebalancer`: audit logging helpers, reconciliation helpers, cache types, PID
  helpers, recovery routines, and broker adapter internals. They may be useful,
  but visibility currently makes them part of the implied public API.
- Python deprecations are exposed through runtime warnings rather than formal
  Python-level deprecation metadata and consistently documented deprecation
  intent.

These are not cosmetic issues. A 1.0 release would turn today's public surface
into a compatibility commitment. Before that happens, nanobook needs more time
to separate durable API from operational plumbing, make documentation gaps
mechanical, and align deprecation behavior across Rust and Python.

The project is also still in a learning phase. Its abstractions are being tested
against real workflows: deterministic backtesting, broker adapters, risk checks,
paper-trading operations, and rebalancing. Keeping the version at 0.x preserves
room to simplify or remove public surfaces that prove to be wrong, overly
broad, or too costly to maintain.

## 3. Fork-at-tag path for users needing stability

Users who need a stable base can fork from a specific release tag and treat that
tag as their compatibility boundary.

A practical path is:

1. Choose the nanobook tag you have validated for your workflow.
2. Fork the repository at that tag.
3. Vendor or pin that fork in your Rust and Python dependency chain.
4. Apply only the patches you need, with your own stability policy.

This is the right path for users who need production-grade immutability before
nanobook itself is ready to make a 1.0 promise. It keeps the upstream project
free to keep improving the API while giving stability-sensitive users an
auditable base.

## 4. Conditions to revisit 1.0

nanobook should revisit 1.0 only when the maintenance and API conditions support
the promise.

The minimum conditions are:

- at least three active maintainers who can review API changes and share
  long-term compatibility work;
- at least six months of stable schema behavior, including event formats and
  operational inputs used by downstream automation;
- a demonstrated user base with real external workflows, not only maintainer
  use;
- `missing_docs` or an equivalent documentation gate applied before release;
- a reviewed public-surface boundary that removes or explicitly commits to
  operational items currently exported by visibility;
- consistent deprecation hygiene across Rust and Python.

Until those conditions are met, staying in 0.x is the more honest contract:
usable software, tested behavior, and public APIs that may still change when the
right long-term shape becomes clearer.
