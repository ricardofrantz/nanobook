//! NASDAQ ITCH 5.0 parser and nanobook Event conversion.

use crate::{Event, OrderId, Price, Side, TimeInForce};
use std::io::{Read, Result};

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
    pub fn next_message(&mut self) -> Result<Option<ItchMessage>> {
        let mut len_buf = [0u8; 2];
        if self.reader.read_exact(&mut len_buf).is_err() {
            return Ok(None);
        }
        let len = u16::from_be_bytes(len_buf) as usize;
        if len == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "ITCH message length is 0",
            ));
        }

        let mut msg_buf = vec![0u8; len];
        self.reader.read_exact(&mut msg_buf)?;

        let msg_type = msg_buf[0] as char;
        let payload = &msg_buf[1..];

        // Minimum payload sizes per ITCH 5.0 spec (bytes after message type)
        let min_payload = match msg_type {
            'A' | 'F' => 35, // ..payload[31..35]
            'E'        => 30, // ..payload[22..30]
            'C'        => 35, // ..payload[31..35]
            'X'        => 22, // ..payload[18..22]
            'D'        => 18, // ..payload[10..18]
            'U'        => 34, // ..payload[30..34]
            'P'        => 43, // ..payload[35..43]
            'R'        => 10, // ..payload[2..10]
            _          =>  0,
        };
        if payload.len() < min_payload {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
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
                let timestamp = read_u48_be(&payload[4..10]);
                let order_ref = u64::from_be_bytes(payload[10..18].try_into().unwrap());
                let side = if payload[18] == b'B' { Side::Buy } else { Side::Sell };
                let shares = u32::from_be_bytes(payload[19..23].try_into().unwrap());
                let stock = String::from_utf8_lossy(&payload[23..31]).trim().to_string();
                let price = u32::from_be_bytes(payload[31..35].try_into().unwrap());
                Ok(Some(ItchMessage::AddOrder {
                    timestamp, order_ref, side, shares, stock, price
                }))
            }
            'E' => {
                let timestamp = read_u48_be(&payload[4..10]);
                let order_ref = u64::from_be_bytes(payload[10..18].try_into().unwrap());
                let shares = u32::from_be_bytes(payload[18..22].try_into().unwrap());
                let match_number = u64::from_be_bytes(payload[22..30].try_into().unwrap());
                Ok(Some(ItchMessage::OrderExecuted {
                    timestamp, order_ref, shares, match_number
                }))
            }
            'C' => {
                let timestamp = read_u48_be(&payload[4..10]);
                let order_ref = u64::from_be_bytes(payload[10..18].try_into().unwrap());
                let shares = u32::from_be_bytes(payload[18..22].try_into().unwrap());
                let match_number = u64::from_be_bytes(payload[22..30].try_into().unwrap());
                let printable = payload[30] == b'Y';
                let price = u32::from_be_bytes(payload[31..35].try_into().unwrap());
                Ok(Some(ItchMessage::OrderExecutedWithPrice {
                    timestamp, order_ref, shares, match_number, printable, price
                }))
            }
            'X' => {
                let timestamp = read_u48_be(&payload[4..10]);
                let order_ref = u64::from_be_bytes(payload[10..18].try_into().unwrap());
                let shares = u32::from_be_bytes(payload[18..22].try_into().unwrap());
                Ok(Some(ItchMessage::OrderCancel {
                    timestamp, order_ref, shares
                }))
            }
            'D' => {
                let timestamp = read_u48_be(&payload[4..10]);
                let order_ref = u64::from_be_bytes(payload[10..18].try_into().unwrap());
                Ok(Some(ItchMessage::OrderDelete {
                    timestamp, order_ref
                }))
            }
            'U' => {
                let timestamp = read_u48_be(&payload[4..10]);
                let old_order_ref = u64::from_be_bytes(payload[10..18].try_into().unwrap());
                let new_order_ref = u64::from_be_bytes(payload[18..26].try_into().unwrap());
                let shares = u32::from_be_bytes(payload[26..30].try_into().unwrap());
                let price = u32::from_be_bytes(payload[30..34].try_into().unwrap());
                Ok(Some(ItchMessage::OrderReplace {
                    timestamp, old_order_ref, new_order_ref, shares, price
                }))
            }
            'P' => {
                let timestamp = read_u48_be(&payload[4..10]);
                let side = match payload[18] {
                    b'B' => Side::Buy,
                    _ => Side::Sell,
                };
                let shares = u32::from_be_bytes(payload[19..23].try_into().unwrap());
                let stock = String::from_utf8_lossy(&payload[23..31]).trim().to_string();
                let price = u32::from_be_bytes(payload[31..35].try_into().unwrap());
                let match_number = u64::from_be_bytes(payload[35..43].try_into().unwrap());
                Ok(Some(ItchMessage::Trade {
                    timestamp, side, shares, stock, price, match_number
                }))
            }
            'R' => {
                let locate = u16::from_be_bytes(payload[0..2].try_into().unwrap());
                let stock = String::from_utf8_lossy(&payload[2..10]).trim().to_string();
                self.stock_locates.insert(locate, stock.clone());
                Ok(Some(ItchMessage::StockDirectory { stock, locate }))
            }
            _ => Ok(Some(ItchMessage::Other(msg_type))),
        }
    }
}

fn read_u48_be(buf: &[u8]) -> u64 {
    let mut extended = [0u8; 8];
    extended[2..8].copy_from_slice(buf);
    u64::from_be_bytes(extended)
}

/// Convert ITCH messages to nanobook Events.
/// 
/// Note: This only includes messages that modify the book.
pub fn itch_to_event(msg: ItchMessage) -> Option<(String, Event)> {
    match msg {
        ItchMessage::AddOrder { side, shares, stock, price, .. } => {
            // ITCH price is scaled by 10,000. Nanobook Price is cents (scaled by 100).
            // NB_Price = ITCH_Price / 100
            let nb_price = (price / 100) as i64;
            Some((stock, Event::SubmitLimit {
                side,
                price: Price(nb_price),
                quantity: shares as u64,
                time_in_force: TimeInForce::GTC,
            }))
        }
        ItchMessage::OrderCancel { order_ref, .. } | ItchMessage::OrderDelete { order_ref, .. } => {
            // Note: We need a mapping from ITCH order_ref to nanobook OrderId.
            // For now, we'll assume they match or let the caller handle mapping.
            // ITCH order_refs are global and unique.
            Some(("".to_string(), Event::Cancel {
                order_id: OrderId(order_ref),
            }))
        }
        ItchMessage::OrderReplace { old_order_ref, shares, price, .. } => {
            let nb_price = (price / 100) as i64;
            Some(("".to_string(), Event::Modify {
                order_id: OrderId(old_order_ref),
                new_price: Price(nb_price),
                new_quantity: shares as u64,
            }))
        }
        _ => None,
    }
}
