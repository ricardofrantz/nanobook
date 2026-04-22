#![no_main]
//! Fuzz target for the NASDAQ ITCH 5.0 parser (I2).
//!
//! Feeds arbitrary bytes to `ItchParser` and drains up to 64
//! messages. The safety contract is "never panic on malformed
//! input" — enforced by the fallible slice-read helpers
//! introduced in S3. A panic here reopens a known-closed class of
//! DoS vulnerabilities (ITCH frames commonly arrive from network
//! transports where adversarial input is the baseline assumption).
//!
//! This fuzzer is the external-input extension of the proptest
//! `arbitrary_bytes_never_panic` in `src/itch.rs`. libFuzzer's
//! coverage-guided exploration typically finds edge cases that
//! purely-random proptest inputs miss — particularly around the
//! variable-length message framing and the known-prefix `match
//! msg_type` dispatch.

use libfuzzer_sys::fuzz_target;
use nanobook::itch::ItchParser;

fuzz_target!(|data: &[u8]| {
    let mut parser = ItchParser::new(data);
    // Drain up to 64 messages. Bounds the per-iteration cost; 64
    // is well past anything that could be hidden by state-
    // accumulation bugs (the parser's only mutable state is its
    // stock-locate cache, which is bounded by u16 keys).
    for _ in 0..64 {
        match parser.next_message() {
            Ok(None) => break, // clean EOF
            Ok(Some(_)) => continue,
            Err(_) => break, // malformed input: structured error
        }
    }
});
