//! Validation errors for order submission.

use std::fmt;

/// Errors returned by validated order submission methods.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ValidationError {
    /// Quantity must be greater than zero.
    ZeroQuantity,
    /// Price must be greater than zero for limit orders.
    ZeroPrice,
    /// `price × quantity` overflowed `i64` and no meaningful notional
    /// value can be produced. Returning an error rather than wrapping
    /// prevents a financially-absurd value (e.g., a large positive
    /// product that wraps to negative) from propagating to P&L
    /// accounting, risk checks, or log output.
    NotionalOverflow { price: i64, quantity: u64 },
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValidationError::ZeroQuantity => write!(f, "quantity must be greater than zero"),
            ValidationError::ZeroPrice => write!(f, "price must be greater than zero"),
            ValidationError::NotionalOverflow { price, quantity } => write!(
                f,
                "notional overflow: price={price} * quantity={quantity} exceeds i64 range"
            ),
        }
    }
}

impl std::error::Error for ValidationError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display() {
        assert_eq!(
            format!("{}", ValidationError::ZeroQuantity),
            "quantity must be greater than zero"
        );
        assert_eq!(
            format!("{}", ValidationError::ZeroPrice),
            "price must be greater than zero"
        );
    }

    #[test]
    fn is_error() {
        let err: Box<dyn std::error::Error> = Box::new(ValidationError::ZeroQuantity);
        assert!(err.to_string().contains("quantity"));
    }
}
