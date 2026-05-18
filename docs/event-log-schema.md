# Event Log Schema

This document specifies the JSONL (JSON Lines) schema used for deterministic replay of order book events. Both the Rust nanobook implementation and the OCaml oracle must adhere to this schema to ensure differential testing works correctly.

## Schema Version

- **Current version**: `1.0`
- **Version field**: `schema_version` (optional in v1.0, will be required in future versions for schema evolution)

## Event Format

Each line in the JSONL file represents one event with the following structure:

```json
{
  "type": "SubmitLimit|SubmitMarket|Cancel",
  ... // type-specific fields
}
```

## Event Types

### SubmitLimit

Submit a limit order to the book.

```json
{
  "type": "SubmitLimit",
  "side": "BUY|SELL",
  "price": 10000,
  "quantity": 100,
  "time_in_force": "GTC|IOC|FOK",
  "owner": null or 1,
  "stp_policy": "Off|CancelNewest|CancelOldest|DecrementAndCancel"
}
```

**Fields:**
- `type`: Event type, always "SubmitLimit"
- `side`: "BUY" or "SELL"
- `price`: Integer in smallest units (e.g., cents for USD)
- `quantity`: Positive integer
- `time_in_force`: "GTC" (Good-til-cancelled), "IOC" (Immediate-or-cancel), or "FOK" (Fill-or-kill)
- `owner`: Optional integer owner ID for self-trade prevention (null if no owner)
- `stp_policy`: Self-trade prevention policy (default: "Off")

### SubmitMarket

Submit a market order for immediate execution.

```json
{
  "type": "SubmitMarket",
  "side": "BUY|SELL",
  "quantity": 100,
  "owner": null or 1,
  "stp_policy": "Off|CancelNewest|CancelOldest|DecrementAndCancel"
}
```

**Fields:**
- `type`: Event type, always "SubmitMarket"
- `side`: "BUY" or "SELL"
- `quantity`: Positive integer
- `owner`: Optional integer owner ID for self-trade prevention (null if no owner)
- `stp_policy`: Self-trade prevention policy (default: "Off")

### Cancel

Cancel an existing order by ID.

```json
{
  "type": "Cancel",
  "order_id": 1
}
```

**Fields:**
- `type`: Event type, always "Cancel"
- `order_id`: Integer order ID to cancel

## Self-Trade Prevention (STP) Policies

STP policies control what happens when an order would trade with another order from the same owner:

- `Off`: No self-trade prevention - orders from same owner can trade freely
- `CancelNewest`: Cancel the newest (incoming) order when self-trade detected
- `CancelOldest`: Cancel the oldest (resting) order when self-trade detected
- `DecrementAndCancel`: Cancel the smaller order; if equal, cancel the resting order

STP only applies when both orders have the same `owner` ID. Orders with `owner: null` are not subject to STP.

When replaying events, trades are emitted in JSONL format:

```json
{
  "id": 1,
  "price": 10000,
  "quantity": 50,
  "aggressor_order_id": 2,
  "passive_order_id": 1,
  "aggressor_side": "BUY",
  "timestamp": 1
}
```

**Fields:**
- `id`: Unique trade identifier (assigned by exchange)
- `price`: Execution price (always the resting order's price)
- `quantity`: Quantity executed
- `aggressor_order_id`: Order ID that initiated the trade (taker)
- `passive_order_id`: Order ID that was resting on the book (maker)
- `aggressor_side`: "BUY" or "SELL" (side of the aggressor order)
- `timestamp`: When the trade occurred (assigned by exchange)

## Schema Evolution Rules

Schema evolution is intentionally conservative because these logs are shared
test artifacts for two independent engines. Any change that requires a
different interpretation of an existing log must be treated as a compatibility
event, not as a local implementation detail.

Current field stability:

| Surface | Fields | Compatibility status |
| --- | --- | --- |
| Event envelope | `type` | Stable |
| `SubmitLimit` | `side`, `price`, `quantity`, `time_in_force` | Stable |
| `SubmitMarket` | `side`, `quantity` | Stable |
| `Cancel` | `order_id` | Stable |
| Trade output | `id`, `price`, `quantity`, `aggressor_order_id`, `passive_order_id`, `aggressor_side`, `timestamp` | Stable |
| Schema envelope | `schema_version` | Experimental in v1.0 because it is optional; future schemas may require it |
| Self-trade prevention | `owner`, `stp_policy` | Experimental until both engines enforce the same validation and golden corpus coverage for all policies |

Rules for schema changes:

1. Incrementing `schema_version` is a **breaking change**. A version bump means
   fixtures, parsers, validators, golden outputs, and replay behavior may no
   longer be compatible with older logs unless an explicit migration is provided.
2. Rust and OCaml implementations must be updated in the same change set. It is
   not acceptable for one engine to accept, reject, or emit a new schema before
   the other engine has matching behavior.
3. Add CI regression coverage for every schema change. At minimum, the change
   must include golden corpus inputs and expected trade outputs, Rust replay
   tests, OCaml oracle tests, and a differential test that proves both engines
   produce byte-identical output for the affected cases.
4. Stable fields cannot be renamed, removed, have their type changed, or have
   their semantics changed without a `schema_version` bump and migration plan.
   New optional fields may be added to an existing version only when old readers
   can ignore them and both engines are tested to do so.
5. Experimental fields may change within the current version only if the change
   is documented here, both engines are updated together, and CI contains
   regression tests for the old and new edge cases. Promoting an experimental
   field to stable requires golden corpus coverage and explicit documentation in
   this table.
6. Unknown event `type` values, unknown enum values, invalid required fields,
   and unsupported `schema_version` values must be rejected consistently by both
   engines.

Migration path for breaking schema changes:

1. Add the new `schema_version` definition to this document, including the exact
   field-level differences from the previous version.
2. Add or update a deterministic migration tool or script that converts old
   JSONL logs to the new schema. If automatic migration is impossible, document
   the manual rewrite rule and the reason it cannot be automated.
3. Keep the previous golden corpus fixtures available long enough to test the
   migration path from the previous schema to the new schema.
4. Update Rust and OCaml readers, validators, writers, and replay code in the
   same change set.
5. Add CI checks that run the old fixtures through the migration path, replay
   the migrated logs in both engines, and compare byte-identical trade output.
6. Document the breaking change, migration command, and any removed fields in
   this file before accepting logs written with the new schema.

## Validation

Both implementations must:
- Reject events with unknown `type`
- Validate all required fields are present
- Validate field types (integers, strings, enums)
- Validate value ranges (e.g., quantity > 0, price within valid range)
- (Future) Reject events with unsupported `schema_version` when schema versioning is implemented

## Example Event Log

```jsonl
{"type":"SubmitLimit","side":"SELL","price":10100,"quantity":100,"time_in_force":"GTC"}
{"type":"SubmitLimit","side":"SELL","price":10200,"quantity":200,"time_in_force":"GTC"}
{"type":"SubmitLimit","side":"BUY","price":10100,"quantity":50,"time_in_force":"GTC"}
{"type":"Cancel","order_id":1}
```

## Example Trade Output

```jsonl
{"id":1,"price":10100,"quantity":50,"aggressor_order_id":3,"passive_order_id":1,"aggressor_side":"BUY","timestamp":1}
```

## Testing

The checked-in oracle golden corpus (see `oracle-ocaml/test/golden/README.md` and `oracle-ocaml/test/corpus/README.md`) contains test cases that exercise edge cases:
- Cancel at zero quantity
- Multiple fills at same price level
- FOK no-match scenarios
- FOK partial-cross scenarios
- Self-trade prevention with each StpPolicy variant
- Orders at min/max price
- Simultaneous-timestamp events

Both implementations must produce byte-identical trade output for all golden corpus inputs.
