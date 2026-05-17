# Solutions Documentation

This directory contains learning documents and solution writeups for nanobook features and hardening work. Each document captures what was learned, what surprised us, and what remains for future work.

## Documents

### [Ops Hardening Learnings (v0.13)](ops-hardening-learnings.md)
Documents the 9 IBKR failure modes implemented in v0.13, including duplicate callbacks, race conditions, disconnects, stale data, clock skew, restarts, idempotency, kill switches, and crash recovery. Covers what v0.10 handled vs. what v0.13 added, with detailed analysis per failure mode and cross-cutting learnings.

### [ITCH Replay Learnings (v0.11)](itch-replay-learnings.md)
Captures learnings from the reproducible ITCH replay harness introduced in v0.11, including warmup event handling, JSON file corruption on interrupt, and Python environment setup friction. Documents ITCH parser gaps fixed in v0.11.

### [Portfolio Simulator Parity Learnings](portfolio-sim-parity-learnings.md)
Documents the parity check between nanobook's portfolio simulator and vectorbt's backtesting framework for the cross-sectional momentum strategy. Covers snapshot timing, API misunderstandings, unit conversion consistency, and cost model differences.

### [v0.14 Kill Gate Criteria](v0.14-kill-gate-criteria.md)
Defines the criteria for determining when v0.14 is ready to release, including completion of all blocking beads, test coverage requirements, documentation requirements, and stability requirements.

### [Paper Soak Learnings (v0.15)](paper-soak-learnings.md)
Pre-soak scaffold for the v0.15 IBKR paper-live learning document. It records the evidence sources, incident log format, daily/weekly check-in templates, and required "what's still papered-over" section that must be filled from sanitized audit excerpts after the actual soak.

## Purpose

These documents serve as:

1. **Historical record**: What we learned during implementation
2. **Decision rationale**: Why we made certain architectural choices
3. **Future reference**: Lessons learned for similar work
4. **Onboarding**: Understanding the evolution of nanobook's features

## Contributing

When implementing a new feature or hardening work, consider adding a learning document to this directory if:

- The implementation revealed unexpected challenges
- The architectural decisions have broader applicability
- The work involved significant debugging or problem-solving
- Future implementers would benefit from understanding the trade-offs

Document format is flexible, but should include:

- Context: What was the problem or goal?
- What surprised us: Unexpected findings or challenges
- What we fixed: Specific solutions and their rationale
- What remains: Open issues or future work
