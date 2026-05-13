# Oracle Design: Dual-Purpose Reference Implementation

## Dual Purpose

This OCaml oracle serves two honest, complementary purposes:

**1. Technical Bug-Finding (Wrong-but-Consistent Class)**
- Detects semantic bugs where both engines are internally consistent but wrong relative to spec
- Catches issues that fuzzing and mutation testing miss (e.g., wrong-but-consistent interpretation of edge cases)
- Example: Knight Capital class bugs where logic is applied consistently but incorrectly
- Differential testing against Rust implementation provides cross-validation without requiring a "gold standard" oracle

**2. Jane Street Audience Signaling**
- Demonstrates commitment to correctness via dual-language implementation
- OCaml is recognized in HFT circles for type safety and correctness
- Signals that nanobook is serious about LOB engine correctness
- Authentic signal: implementation is real, tested, and integrated into CI

Both purposes are real. Stating both explicitly prevents the rationalization trap of claiming one while pursuing the other.

## Triage Protocol: When Divergence is Found

When CI reports a divergence between Rust and OCaml engines:

### Step 1: Isolate the Failing Test Case
- Identify which golden corpus test case fails
- Extract the minimal event sequence that reproduces the divergence
- Run both engines in debug mode to trace execution

### Step 2: Consult the Specification
- Reference `docs/event-log-schema.md` for the authoritative spec
- Check if spec is ambiguous or underspecified for this edge case
- If spec is clear, determine which engine violates it

### Step 3: Determine Which Engine is Wrong

**If OCaml is wrong:**
- Fix OCaml implementation to match spec
- Add regression test to golden corpus
- Verify Rust implementation still passes all tests

**If Rust is wrong:**
- Fix Rust implementation to match spec
- Add regression test to Rust test suite
- Verify OCaml implementation still passes all tests

**If spec is ambiguous:**
- Clarify spec in `docs/event-log-schema.md`
- Document the decision and rationale
- Update both implementations to match clarified spec
- Add regression test to prevent drift

### Step 4: Root Cause Analysis
- Was this a logic error, type error, or spec interpretation error?
- Would property-based testing have caught this? (If yes, consider adding tests)
- Would static analysis have caught this? (If yes, consider adding lints)
- Document learnings in `docs/solutions/`

### Step 5: Prevent Recurrence
- Add regression test to golden corpus
- Update CI to include the new test case
- Consider adding invariant checks if applicable
- Document the pattern in `docs/solutions/oracle-design.md` if it represents a new class of bugs

## Implementation Invariants

1. **Independence**: OCaml oracle is written from spec only, NOT by reading Rust source
2. **Exhaustive Matching**: OCaml pattern matching is exhaustive; new event types cause compile errors
3. **Type Safety**: OCaml's type system prevents entire classes of bugs
4. **Deterministic**: Both engines produce byte-identical output on the golden corpus
5. **Minimal Dependencies**: Uses stdlib-only (~800 LOC target) to minimize attack surface

## CI Integration

- `.github/workflows/oracle.yml` runs on every PR
- Installs OCaml 5.4 via opam-installer (cached)
- Builds oracle and runs golden corpus (18 test cases)
- Fails build on any divergence
- Sanity-check job confirms `cargo add nanobook` does NOT pull OCaml
- Adds ~2-4 min per PR with aggressive caching

## Future Work

- **v0.16**: Consider property-based testing if new bug classes emerge
- **v0.16**: Schema versioning policy (schema_version field already optional)
- **v0.17**: Expand golden corpus based on real-world edge cases discovered in production