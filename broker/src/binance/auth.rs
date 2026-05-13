//! HMAC-SHA256 signature generation for Binance API requests.

use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::error::BrokerError;

type HmacSha256 = Hmac<Sha256>;

/// Sign a query string with HMAC-SHA256.
///
/// Returns the hex-encoded signature to append as `&signature=<sig>`.
///
/// # Errors
///
/// Returns `BrokerError::Auth` if the secret key length is invalid for HMAC-SHA256.
/// In practice, HMAC-SHA256 accepts any key length, so this error is unlikely
/// but kept for robustness against future crate changes.
pub fn sign(query_string: &str, secret_key: &str) -> Result<String, BrokerError> {
    let mut mac = HmacSha256::new_from_slice(secret_key.as_bytes())
        .map_err(|e| BrokerError::Auth(format!("invalid HMAC key: {e}")))?;
    mac.update(query_string.as_bytes());
    let result = mac.finalize();
    Ok(hex::encode(result.into_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_signature() {
        // From Binance API docs example
        let query = "symbol=LTCBTC&side=BUY&type=LIMIT&timeInForce=GTC&quantity=1&price=0.1&recvWindow=5000&timestamp=1499827319559";
        let secret = "NhqPtmdSJYdKjVHjA7PZj4Mge3R5YNiP1e3UZjInClVN65XAbvqqM6A7H5fATj0j";
        let sig = sign(query, secret).unwrap();
        assert_eq!(
            sig,
            "c8db56825ae71d6d79447849e617115f4a920fa2acdcab2b053c4b2838bd6b71"
        );
    }
}
