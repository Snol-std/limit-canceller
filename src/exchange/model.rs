//! Shared order models independent of any specific exchange.

use anyhow::{bail, Result};

/// Order side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Buy,
    Sell,
}

impl Side {
    pub fn as_str(&self) -> &'static str {
        match self {
            Side::Buy => "buy",
            Side::Sell => "sell",
        }
    }

    /// Accepts exchange variants such as `BUY`, `Buy`, `buy`, etc.
    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_uppercase().as_str() {
            "BUY" => Some(Self::Buy),
            "SELL" => Some(Self::Sell),
            _ => None,
        }
    }
}

/// Which open-order sides are allowed to be cancelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelSide {
    Buy,
    Sell,
    Both,
}

impl CancelSide {
    pub fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "buy" => Ok(Self::Buy),
            "sell" => Ok(Self::Sell),
            "both" => Ok(Self::Both),
            other => bail!("unknown side {other:?}; allowed values: buy, sell, both"),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Buy => "buy",
            Self::Sell => "sell",
            Self::Both => "both",
        }
    }

    pub fn matches(&self, side: Side) -> bool {
        match self {
            Self::Buy => side == Side::Buy,
            Self::Sell => side == Side::Sell,
            Self::Both => true,
        }
    }
}

/// Regular active (open) order. Order type is not filtered.
///
/// Price and quantity are stored as strings because exchanges return them as strings,
/// which avoids precision loss from parsing into `f64`.
#[derive(Debug, Clone)]
pub struct Order {
    pub id: String,
    pub side: Side,
    pub price: String,
    pub quantity: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_side_parsing_and_matching() {
        assert_eq!(CancelSide::parse("BUY").unwrap(), CancelSide::Buy);
        assert_eq!(CancelSide::parse(" sell ").unwrap(), CancelSide::Sell);
        assert_eq!(CancelSide::parse("Both").unwrap(), CancelSide::Both);
        assert!(CancelSide::parse("long").is_err());

        assert!(CancelSide::Buy.matches(Side::Buy));
        assert!(!CancelSide::Buy.matches(Side::Sell));
        assert!(CancelSide::Sell.matches(Side::Sell));
        assert!(CancelSide::Both.matches(Side::Buy));
        assert!(CancelSide::Both.matches(Side::Sell));
    }
}
