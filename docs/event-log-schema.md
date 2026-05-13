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

When the schema changes:
1. Increment the `schema_version` field
2. Update both Rust and OCaml implementations simultaneously
3. Maintain backward compatibility if possible
4. Document breaking changes in this file

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

The golden corpus (see `docs/solutions/oracle-golden-corpus.md`) contains test cases that exercise edge cases:
- Cancel at zero quantity
- Multiple fills at same price level
- FOK no-match scenarios
- FOK partial-cross scenarios
- Self-trade prevention with each StpPolicy variant
- Orders at min/max price
- Simultaneous-timestamp events

Both implementations must produce byte-identical trade output for all golden corpus inputs.