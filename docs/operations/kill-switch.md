# Kill Switch

The rebalancer kill switch is the emergency stop path for an active rebalance. It first tries to stop the runner gracefully, then, when built with the forceful kill feature, can connect directly to the broker and cancel remaining open orders.

## Modes

### Graceful-only mode

The default behavior is to signal the running process and let the graceful shutdown path finish the current broker call, skip queued orders, and write a completion event.

Expected audit sequence:

```text
kill_requested(method=graceful)
kill_completed(method=graceful)
```

### Two-phase mode

When the forceful kill feature is enabled, `rebalancer kill` uses a two-phase workflow.

1. **Graceful phase**
   - Read the runner PID from the PID file.
   - Append `kill_requested` and phase-start events.
   - Send SIGTERM to the runner.
   - Wait for the configured timeout.
   - If the process exits and no dangling orders remain, append `kill_completed(method=graceful)`.

2. **Forceful broker-cancel phase**
   - Runs when graceful shutdown times out or dangling orders remain.
   - Connect directly to the broker using configured connection settings.
   - Query cancellable open orders.
   - Cancel each order and verify cancellation with retry/backoff.
   - Append phase-completion and final `kill_completed(method=forced)` events.

## Timeout configuration

Timeout precedence:

1. CLI option: `rebalancer kill --timeout-secs N`
2. Environment variable: `NANOBOOK_KILL_TIMEOUT_SECS=N`
3. Config file: `[kill] timeout_secs = N`

The recommended default is 30 seconds. Zero or invalid values should be rejected or ignored in favor of the next valid source.

Example config:

```toml
[kill]
timeout_secs = 30
```

## Audit events

Graceful success:

```text
kill_requested(method=two_phase)
kill_phase1_started
kill_phase1_completed
kill_completed(method=graceful)
```

Graceful timeout followed by broker cancellation:

```text
kill_requested(method=two_phase)
kill_phase1_started
kill_phase2_started
kill_phase2_completed
kill_completed(method=forced)
```

If broker cancellation partially fails, `kill_phase2_completed` and `kill_completed` should report non-zero remaining-order counts and list remaining order IDs where possible.

## Monitoring signals

Recommended signals:

- kill command success rate;
- phase-1 success rate;
- phase-2 success rate;
- phase-1 duration p95 relative to configured timeout;
- phase-2 broker cancellation duration p95;
- total kill duration p95;
- audit write failures during kill;
- remaining order IDs after kill completion.

Treat any remaining broker order after a kill attempt as safety-critical until manually resolved.

## Rollback criteria

Rebuild or redeploy without forceful kill behavior if any of these occur repeatedly:

- PID-file handling cannot identify the runner reliably;
- phase 1 cannot signal the runner due to permission or stale-PID errors;
- phase 2 cannot connect to the broker often enough to be trusted;
- forceful cancellation leaves orders open;
- kill audit events cannot be written reliably.

Rollback preserves graceful-only behavior; it does not remove the need to manually resolve broker state.

## Incident response

If the kill switch reports remaining orders or cannot connect to the broker:

1. Check whether the rebalancer process is still running and whether the PID file is stale.
2. Verify broker connectivity from the host running the kill command.
3. Cancel remaining orders manually in the broker UI if any order IDs remain or broker state is uncertain.
4. Preserve audit and application logs.
5. Investigate why graceful shutdown timed out or broker cancellation failed.
6. Re-enable forceful kill only after reproducing and fixing the failure mode in a safe environment.

## Related documentation

- [Graceful shutdown](graceful-shutdown.md)
- [Warm restart](warm-restart.md)
- [Write-ahead audit logging](write-ahead-audit-logging.md)
