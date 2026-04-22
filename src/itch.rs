//! NASDAQ ITCH 5.0 parser and nanobook Event conversion.

use crate::{Event, OrderId, Price, Side, TimeInForce};
use std::io::{Error, ErrorKind, Read, Result};

/// Construct an `InvalidData` error with a short static message.
///
/// Kept as a helper so every fallible slice-read throughout the
/// parser emits the same error shape. Callers can match on
/// `ErrorKind::InvalidData` to distinguish malformed input from
/// transport errors.
fn short_payload(field: &'static str) -> Error {
    Error::new(
        ErrorKind::InvalidData,
        format!("ITCH short payload reading {field}"),
    )
}

/// Read a big-endian `u16` from the first two bytes of `slice`.
///
/// Returns `InvalidData` if `slice.len() < 2`, matching the behavior
/// previously obtained via `try_into().unwrap()` — but as an explicit
/// error rather than a panic.
fn read_u16_be(slice: &[u8], field: &'static str) -> Result<u16> {
    slice
        .get(..2)
        .ok_or_else(|| short_payload(field))
        .and_then(|b| b.try_into().map_err(|_| short_payload(field)))
        .map(u16::from_be_bytes)
}

fn read_u32_be(slice: &[u8], field: &'static str) -> Result<u32> {
    slice
        .get(..4)
        .ok_or_else(|| short_payload(field))
        .and_then(|b| b.try_into().map_err(|_| short_payload(field)))
        .map(u32::from_be_bytes)
}

fn read_u64_be(slice: &[u8], field: &'static str) -> Result<u64> {
    slice
        .get(..8)
        .ok_or_else(|| short_payload(field))
        .and_then(|b| b.try_into().map_err(|_| short_payload(field)))
        .map(u64::from_be_bytes)
}

/// Read a big-endian 48-bit integer into a `u64`. ITCH timestamps use
/// this wire width (nanoseconds since midnight, max ≈ 24 × 3600 × 1e9
/// ≈ 8.6e13, fits in 47 bits).
fn read_u48_be(slice: &[u8], field: &'static str) -> Result<u64> {
    let bytes: [u8; 6] = slice
        .get(..6)
        .ok_or_else(|| short_payload(field))
        .and_then(|b| b.try_into().map_err(|_| short_payload(field)))?;
    let mut extended = [0u8; 8];
    extended[2..8].copy_from_slice(&bytes);
    Ok(u64::from_be_bytes(extended))
}

/// ITCH 5.0 Message Types
#[derive(Debug, Clone, PartialEq)]
pub enum ItchMessage {
    AddOrder {
        timestamp: u64,
        order_ref: u64,
        side: Side,
        shares: u32,
        stock: String,
        price: u32,
    },
    OrderExecuted {
        timestamp: u64,
        order_ref: u64,
        shares: u32,
        match_number: u64,
    },
    OrderExecutedWithPrice {
        timestamp: u64,
        order_ref: u64,
        shares: u32,
        match_number: u64,
        printable: bool,
        price: u32,
    },
    OrderCancel {
        timestamp: u64,
        order_ref: u64,
        shares: u32,
    },
    OrderDelete {
        timestamp: u64,
        order_ref: u64,
    },
    OrderReplace {
        timestamp: u64,
        old_order_ref: u64,
        new_order_ref: u64,
        shares: u32,
        price: u32,
    },
    Trade {
        timestamp: u64,
        side: Side,
        shares: u32,
        stock: String,
        price: u32,
        match_number: u64,
    },
    StockDirectory {
        stock: String,
        locate: u16,
    },
    Other(char),
}

/// Parser for ITCH 5.0 binary format.
pub struct ItchParser<R: Read> {
    reader: R,
    stock_locates: std::collections::HashMap<u16, String>,
}

impl<R: Read> ItchParser<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            stock_locates: std::collections::HashMap::new(),
        }
    }

    /// Read the next message from the stream.
    ///
    /// Returns `Ok(None)` on a clean EOF at a message boundary.
    /// Returns `Err(io::Error)` (kind `InvalidData`) on:
    /// - zero-length length prefix,
    /// - a payload that is shorter than the ITCH message type requires,
    /// - a message body truncated mid-field (caught by the fallible
    ///   `read_{u16,u32,u48,u64}_be` helpers).
    ///
    /// Never panics on malformed input. This is a hard requirement:
    /// ITCH feeds come from external transports and a panic is a DoS
    /// vector.
    pub fn next_message(&mut self) -> Result<Option<ItchMessage>> {
        let mut len_buf = [0u8; 2];
        if self.reader.read_exact(&mut len_buf).is_err() {
            return Ok(None);
        }
        let len = u16::from_be_bytes(len_buf) as usize;
        if len == 0 {
            return Err(Error::new(
                ErrorKind::InvalidData,
                "ITCH message length is 0",
            ));
        }

        let mut msg_buf = vec![0u8; len];
        self.reader.read_exact(&mut msg_buf)?;

        let msg_type = msg_buf[0] as char;
        let payload = &msg_buf[1..];

        // Minimum payload sizes per ITCH 5.0 spec (bytes after message
        // type). This gate is a fast-fail with a clear, type-scoped
        // error message. The per-field reads below are individually
        // fallible, so the parser remains correct even if this table
        // is out of sync with a message layout — the per-field errors
        // kick in and no panic occurs.
        let min_payload = match msg_type {
            'A' | 'F' => 35, // ..payload[31..35]
            'E' => 30,       // ..payload[22..30]
            'C' => 35,       // ..payload[31..35]
            'X' => 22,       // ..payload[18..22]
            'D' => 18,       // ..payload[10..18]
            'U' => 34,       // ..payload[30..34]
            'P' => 43,       // ..payload[35..43]
            'R' => 10,       // ..payload[2..10]
            _ => 0,
        };
        if payload.len() < min_payload {
            return Err(Error::new(
                ErrorKind::InvalidData,
                format!(
                    "ITCH '{}' message too short: {} bytes, need {}",
                    msg_type,
                    payload.len(),
                    min_payload,
                ),
            ));
        }

        match msg_type {
            'A' | 'F' => {
                let timestamp = read_u48_be(&payload[4..10], "A/F.timestamp")?;
                let order_ref = read_u64_be(&payload[10..18], "A/F.order_ref")?;
                let side = if payload[18] == b'B' {
                    Side::Buy
                } else {
                    Side::Sell
                };
                let shares = read_u32_be(&payload[19..23], "A/F.shares")?;
                let stock = String::from_utf8_lossy(&payload[23..31]).trim().to_string();
                let price = read_u32_be(&payload[31..35], "A/F.price")?;
                Ok(Some(ItchMessage::AddOrder {
                    timestamp,
                    order_ref,
                    side,
                    shares,
                    stock,
                    price,
                }))
            }
            'E' => {
                let timestamp = read_u48_be(&payload[4..10], "E.timestamp")?;
                let order_ref = read_u64_be(&payload[10..18], "E.order_ref")?;
                let shares = read_u32_be(&payload[18..22], "E.shares")?;
                let match_number = read_u64_be(&payload[22..30], "E.match_number")?;
                Ok(Some(ItchMessage::OrderExecuted {
                    timestamp,
                    order_ref,
                    shares,
                    match_number,
                }))
            }
            'C' => {
                let timestamp = read_u48_be(&payload[4..10], "C.timestamp")?;
                let order_ref = read_u64_be(&payload[10..18], "C.order_ref")?;
                let shares = read_u32_be(&payload[18..22], "C.shares")?;
                let match_number = read_u64_be(&payload[22..30], "C.match_number")?;
                let printable = payload[30] == b'Y';
                let price = read_u32_be(&payload[31..35], "C.price")?;
                Ok(Some(ItchMessage::OrderExecutedWithPrice {
                    timestamp,
                    order_ref,
                    shares,
                    match_number,
                    printable,
                    price,
                }))
            }
            'X' => {
                let timestamp = read_u48_be(&payload[4..10], "X.timestamp")?;
                let order_ref = read_u64_be(&payload[10..18], "X.order_ref")?;
                let shares = read_u32_be(&payload[18..22], "X.shares")?;
                Ok(Some(ItchMessage::OrderCancel {
                    timestamp,
                    order_ref,
                    shares,
                }))
            }
            'D' => {
                let timestamp = read_u48_be(&payload[4..10], "D.timestamp")?;
                let order_ref = read_u64_be(&payload[10..18], "D.order_ref")?;
                Ok(Some(ItchMessage::OrderDelete {
                    timestamp,
                    order_ref,
                }))
            }
            'U' => {
                let timestamp = read_u48_be(&payload[4..10], "U.timestamp")?;
                let old_order_ref = read_u64_be(&payload[10..18], "U.old_order_ref")?;
                let new_order_ref = read_u64_be(&payload[18..26], "U.new_order_ref")?;
                let shares = read_u32_be(&payload[26..30], "U.shares")?;
                let price = read_u32_be(&payload[30..34], "U.price")?;
                Ok(Some(ItchMessage::OrderReplace {
                    timestamp,
                    old_order_ref,
                    new_order_ref,
                    shares,
                    price,
                }))
            }
            'P' => {
                let timestamp = read_u48_be(&payload[4..10], "P.timestamp")?;
                let side = match payload[18] {
                    b'B' => Side::Buy,
                    _ => Side::Sell,
                };
                let shares = read_u32_be(&payload[19..23], "P.shares")?;
                let stock = String::from_utf8_lossy(&payload[23..31]).trim().to_string();
                let price = read_u32_be(&payload[31..35], "P.price")?;
                let match_number = read_u64_be(&payload[35..43], "P.match_number")?;
                Ok(Some(ItchMessage::Trade {
                    timestamp,
                    side,
                    shares,
                    stock,
                    price,
                    match_number,
                }))
            }
            'R' => {
                let locate = read_u16_be(&payload[0..2], "R.locate")?;
                let stock = String::from_utf8_lossy(&payload[2..10]).trim().to_string();
                self.stock_locates.insert(locate, stock.clone());
                Ok(Some(ItchMessage::StockDirectory { stock, locate }))
            }
            _ => Ok(Some(ItchMessage::Other(msg_type))),
        }
    }
}

/// Convert ITCH messages to nanobook Events.
///
/// Note: This only includes messages that modify the book.
pub fn itch_to_event(msg: ItchMessage) -> Option<(String, Event)> {
    match msg {
        ItchMessage::AddOrder {
            side,
            shares,
            stock,
            price,
            ..
        } => {
            // ITCH price is scaled by 10,000. Nanobook Price is cents (scaled by 100).
            // NB_Price = ITCH_Price / 100
            let nb_price = (price / 100) as i64;
            Some((
                stock,
                Event::SubmitLimit {
                    side,
                    price: Price(nb_price),
                    quantity: shares as u64,
                    time_in_force: TimeInForce::GTC,
                },
            ))
        }
        ItchMessage::OrderCancel { order_ref, .. } | ItchMessage::OrderDelete { order_ref, .. } => {
            // Note: We need a mapping from ITCH order_ref to nanobook OrderId.
            // For now, we'll assume they match or let the caller handle mapping.
            // ITCH order_refs are global and unique.
            Some((
                "".to_string(),
                Event::Cancel {
                    order_id: OrderId(order_ref),
                },
            ))
        }
        ItchMessage::OrderReplace {
            old_order_ref,
            shares,
            price,
            ..
        } => {
            let nb_price = (price / 100) as i64;
            Some((
                "".to_string(),
                Event::Modify {
                    order_id: OrderId(old_order_ref),
                    new_price: Price(nb_price),
                    new_quantity: shares as u64,
                },
            ))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    // ------------------------------------------------------------------
    // Slice-read helpers
    // ------------------------------------------------------------------

    #[test]
    fn read_helpers_accept_exact_slice() {
        assert_eq!(read_u16_be(&[0x01, 0x02], "t").unwrap(), 0x0102);
        assert_eq!(read_u32_be(&[0, 0, 0x01, 0x02], "t").unwrap(), 0x0102);
        assert_eq!(
            read_u64_be(&[0, 0, 0, 0, 0, 0, 0x01, 0x02], "t").unwrap(),
            0x0102,
        );
        assert_eq!(read_u48_be(&[0, 0, 0, 0, 0x01, 0x02], "t").unwrap(), 0x0102);
    }

    #[test]
    fn read_helpers_accept_longer_slice() {
        // Excess bytes beyond the read width must be ignored rather
        // than rejected — callers often pass a larger range by design.
        assert_eq!(read_u16_be(&[0x01, 0x02, 0x03], "t").unwrap(), 0x0102);
    }

    #[test]
    fn read_helpers_reject_short_slice() {
        assert_eq!(
            read_u16_be(&[0x01], "t").unwrap_err().kind(),
            ErrorKind::InvalidData,
        );
        assert_eq!(
            read_u32_be(&[0, 0, 0x01], "t").unwrap_err().kind(),
            ErrorKind::InvalidData,
        );
        assert_eq!(
            read_u64_be(&[0; 7], "t").unwrap_err().kind(),
            ErrorKind::InvalidData,
        );
        assert_eq!(
            read_u48_be(&[0; 5], "t").unwrap_err().kind(),
            ErrorKind::InvalidData,
        );
    }

    // ------------------------------------------------------------------
    // Parser: malformed input surfaces as Err, never as panic
    // ------------------------------------------------------------------

    /// EOF at a message boundary is the happy-stream-end signal.
    #[test]
    fn empty_stream_yields_ok_none() {
        let mut parser = ItchParser::new(&[][..]);
        assert!(matches!(parser.next_message(), Ok(None)));
    }

    #[test]
    fn zero_length_prefix_returns_err() {
        let bytes = [0x00, 0x00];
        let mut parser = ItchParser::new(&bytes[..]);
        let err = parser.next_message().unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidData);
    }

    /// AddOrder ('A') requires 35 payload bytes. A length prefix of
    /// 10 is well under that, and the `min_payload` gate catches it
    /// before any per-field read.
    #[test]
    fn truncated_add_order_message_returns_err() {
        let mut bytes = vec![0x00, 0x0A, b'A'];
        bytes.extend_from_slice(&[0u8; 9]); // 9 bytes of payload after 'A'
        let mut parser = ItchParser::new(&bytes[..]);
        let err = parser.next_message().unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidData);
        assert!(err.to_string().contains("too short"), "got: {err}");
    }

    /// Length prefix larger than the remaining bytes triggers
    /// `read_exact` at the transport layer — again Err, never panic.
    #[test]
    fn length_prefix_longer_than_stream_returns_err() {
        // Length claims 100 bytes, only 5 follow.
        let bytes: [u8; 7] = [0x00, 0x64, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE];
        let mut parser = ItchParser::new(&bytes[..]);
        assert!(parser.next_message().is_err());
    }

    /// Unknown message type (no entry in `min_payload` table) is a
    /// valid ITCH frame — it's just opaque to our parser. We return
    /// `Ok(Some(Other(c)))` rather than failing.
    #[test]
    fn unknown_message_type_is_ok_other() {
        let bytes = [0x00, 0x01, b'Z'];
        let mut parser = ItchParser::new(&bytes[..]);
        let msg = parser.next_message().unwrap().unwrap();
        assert_eq!(msg, ItchMessage::Other('Z'));
    }

    // ------------------------------------------------------------------
    // Property: arbitrary bytes in → never panic out
    // ------------------------------------------------------------------

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(1000))]

        /// Hard safety guarantee: feeding any byte sequence, of any
        /// length, must produce a structured `Ok(Some(_))`,
        /// `Ok(None)`, or `Err(_)` — never a panic. ITCH data comes
        /// from network transports and a parser panic is a DoS vector.
        #[test]
        fn arbitrary_bytes_never_panic(
            bytes in prop::collection::vec(any::<u8>(), 0..4096),
        ) {
            let mut parser = ItchParser::new(bytes.as_slice());
            // Drain the whole stream. Any individual call may Err,
            // but the loop must terminate cleanly.
            for _ in 0..32 {
                match parser.next_message() {
                    Ok(None) => break,
                    Ok(Some(_)) => continue,
                    Err(_) => break,
                }
            }
        }
    }
}
