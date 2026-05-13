# Event Log Schema

This document specifies the JSONL (JSON Lines) schema used for deterministic replay of order book events. Both the Rust nanobook implementation and the OCaml oracle must adhere to this schema to ensure differential testing works correctly.

## Schema Version

- **Current version**: `1.0`
- **Version field**: `schema_version` (required in each event)

## Event Format

Each line in the JSONL file represents one event with the following structure:

```json
{
  "schema_version": "1.0",
  "type": "SubmitLimit|SubmitMarket|Cancel",
  "timestamp": 0,
  ... // type-specific fields
}
```

## Event Types

### SubmitLimit

Submit a limit order to the book.

```json
{
  "schema_version": "1.0",
  "type": "SubmitLimit",
  "side": "BUY|SELL",
  "price": 10000,
  "quantity": 100,
  "time_in_force": "GTC|IOC|FOK"
}
```

**Fields:**
- `schema_version`: Schema version string (required)
- `type`: Event type, always "SubmitLimit"
- `side`: "BUY" or "SELL"
- `price`: Integer in smallest units (e.g., cents for USD)
- `quantity`: Positive integer
- `time_in_force`: "GTC" (Good-til-cancelled), "IOC" (Immediate-or-cancel), or "FOK" (Fill-or-kill)

### SubmitMarket

Submit a market order for immediate execution.

```json
{
  "schema_version": "1.0",
  "type": "SubmitMarket",
  "side": "BUY|SELL",
  "quantity": 100
}
```

**Fields:**
- `schema_version`: Schema version string (required)
- `type`: Event type, always "SubmitMarket"
- `side`: "BUY" or "SELL"
- `quantity`: Positive integer

### Cancel

Cancel an existing order by ID.

```json
{
  "schema_version": "1.0",
  "type": "Cancel",
  "order_id": 1
}
```

**Fields:**
- `schema_version`: Schema version string (required)
- `type`: Event type, always "Cancel"
- `order_id`: Integer order ID to cancel

## Trade Format

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
- Reject events with unknown `schema_version`
- Reject events with unknown `type`
- Validate all required fields are present
- Validate field types (integers, strings, enums)
- Validate value ranges (e.g., quantity > 0, price within valid range)

## Example Event Log

```jsonl
{"schema_version":"1.0","type":"SubmitLimit","side":"SELL","price":10100,"quantity":100,"time_in_force":"GTC"}
{"schema_version":"1.0","type":"SubmitLimit","side":"SELL","price":10200,"quantity":200,"time_in_force":"GTC"}
{"schema_version":"1.0","type":"SubmitLimit","side":"BUY","price":10100,"quantity":50,"time_in_force":"GTC"}
{"schema_version":"1.0","type":"Cancel","order_id":1}
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