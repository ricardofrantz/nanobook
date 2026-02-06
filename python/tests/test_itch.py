import nanobook
import pytest
import struct
import tempfile
import os

def test_parse_itch_add_order():
    # ITCH 5.0 Add Order (A) message
    # Length: 36 bytes
    # Type: 'A'
    # Stock Locate: 1 (u16)
    # Tracking: 0 (u16)
    # Timestamp: 12345 (u48)
    # Order Ref: 1 (u64)
    # Side: 'B'
    # Shares: 100 (u32)
    # Stock: 'AAPL    ' (8 chars)
    # Price: 1000000 (u32) ($100.0000)
    
    msg_type = b'A'
    locate = struct.pack(">H", 1)
    tracking = struct.pack(">H", 0)
    ts = b'\x00\x00\x00\x00\x30\x39' # 12345 in 6 bytes
    ref = struct.pack(">Q", 1)
    side = b'B'
    shares = struct.pack(">I", 100)
    stock = b'AAPL    '
    price = struct.pack(">I", 1000000)
    
    payload = msg_type + locate + tracking + ts + ref + side + shares + stock + price
    length = struct.pack(">H", len(payload))
    full_msg = length + payload
    
    with tempfile.NamedTemporaryFile(delete=False) as f:
        f.write(full_msg)
        path = f.name
        
    try:
        events = nanobook.parse_itch(path)
        assert len(events) == 1
        symbol, event = events[0]
        assert symbol == "AAPL"
        assert event.kind == "submit_limit"
        # Nanobook price is cents. ITCH price 1,000,000 / 100 = 10,000 cents ($100.00)
        # Wait, my itch_to_event did: nb_price = (price / 100) as i64;
        # 1,000,000 / 100 = 10,000. Correct.
        assert "price: Price(10000)" in repr(event)
    finally:
        os.unlink(path)

def test_parse_itch_replace_order():
    # ITCH 5.0 Replace Order (U) message
    # Length: 46 bytes
    # Type: 'U'
    # ...
    # Old Ref: 1 (u64)
    # New Ref: 2 (u64)
    # Shares: 50 (u32)
    # Price: 1010000 (u32)
    
    payload = b'U' + struct.pack(">HH6sQQII", 1, 0, b'\x00'*6, 1, 2, 50, 1010000)
    length = struct.pack(">H", len(payload))
    
    with tempfile.NamedTemporaryFile(delete=False) as f:
        f.write(length + payload)
        path = f.name
        
    try:
        events = nanobook.parse_itch(path)
        assert len(events) == 1
        symbol, event = events[0]
        assert event.kind == "modify"
        assert "order_id: OrderId(1)" in repr(event)
        assert "new_price: Price(10100)" in repr(event)
        assert "new_quantity: 50" in repr(event)
    finally:
        os.unlink(path)

def test_parse_itch_executed():
    # ITCH 5.0 Order Executed (E)
    # Ref: 1 (u64), Shares: 100 (u32), Match: 42 (u64)
    payload = b'E' + struct.pack(">HH6sQIQ", 1, 0, b'\x00'*6, 1, 100, 42)
    length = struct.pack(">H", len(payload))
    
    with tempfile.NamedTemporaryFile(delete=False) as f:
        f.write(length + payload)
        path = f.name
    try:
        events = nanobook.parse_itch(path)
        assert len(events) == 0 # internal match handles it
    finally:
        os.unlink(path)

def test_parse_itch_delete():
    # ITCH 5.0 Order Delete (D)
    payload = b'D' + struct.pack(">HH6sQ", 1, 0, b'\x00'*6, 1)
    length = struct.pack(">H", len(payload))
    
    with tempfile.NamedTemporaryFile(delete=False) as f:
        f.write(length + payload)
        path = f.name
    try:
        events = nanobook.parse_itch(path)
        assert len(events) == 1
        assert events[0][1].kind == "cancel"
    finally:
        os.unlink(path)

def test_parse_itch_trade():
    # ITCH 5.0 Trade (P)
    payload = bytearray(b'P' + b'\x00'*43)
    payload[19] = ord('B') # Side
    struct.pack_into(">I", payload, 20, 100) # Shares
    payload[24:32] = b'AAPL    '
    struct.pack_into(">I", payload, 32, 1000000) # Price
    
    length = struct.pack(">H", len(payload))
    with tempfile.NamedTemporaryFile(delete=False) as f:
        f.write(length + payload)
        path = f.name
    try:
        events = nanobook.parse_itch(path)
        assert len(events) == 0 # P msg is off-book
    finally:
        os.unlink(path)

def test_parse_itch_truncated_message():
    # Malformed: type 'A' (AddOrder) needs 36 bytes but we only provide 5
    payload = b'A' + b'\x00' * 4
    length = struct.pack(">H", len(payload))

    with tempfile.NamedTemporaryFile(delete=False) as f:
        f.write(length + payload)
        path = f.name
    try:
        with pytest.raises(OSError, match="too short"):
            nanobook.parse_itch(path)
    finally:
        os.unlink(path)

def test_parse_itch_zero_length():
    # Malformed: length prefix is 0
    with tempfile.NamedTemporaryFile(delete=False) as f:
        f.write(struct.pack(">H", 0))
        path = f.name
    try:
        with pytest.raises(OSError, match="length is 0"):
            nanobook.parse_itch(path)
    finally:
        os.unlink(path)
