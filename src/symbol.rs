//! Canonical ticker representation.
//!
//! Symbols are configured as `BASE/QUOTE` (for example `BTC/USDT`),
//! while exchange-specific forms (`BTCUSDT`, `BTC-USDT`, `BTC-USDT-SWAP`) are derived
//! by the corresponding exchange modules.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub base: String,
    pub quote: String,
}

impl Symbol {
    /// Parses a symbol in `BTC/USDT` format.
    pub fn parse(input: &str) -> anyhow::Result<Self> {
        let (base, quote) = input
            .split_once('/')
            .ok_or_else(|| anyhow::anyhow!("symbol {input:?} is missing the '/' separator"))?;
        let base = base.trim().to_ascii_uppercase();
        let quote = quote.trim().to_ascii_uppercase();
        if base.is_empty() || quote.is_empty() {
            anyhow::bail!("symbol {input:?} contains an empty component");
        }
        if !base.chars().all(|c| c.is_alphanumeric() || c == '_')
            || !quote.chars().all(|c| c.is_alphanumeric() || c == '_') {
            anyhow::bail!("symbol {input:?}: expected exactly BASE/QUOTE without special characters");
        }
        Ok(Self { base, quote })
    }

    /// Binance: `BTCUSDT` (same format for spot and futures).
    pub fn binance(&self) -> String {
        format!("{}{}", self.base, self.quote)
    }

    /// OKX: `BTC-USDT` for spot or `BTC-USDT-SWAP` for perpetual swaps.
    pub fn okx(&self, is_swap: bool) -> String {
        if is_swap {
            format!("{}-{}-SWAP", self.base, self.quote)
        } else {
            format!("{}-{}", self.base, self.quote)
        }
    }

    /// Bybit: `BTCUSDT` (same format for spot and linear).
    pub fn bybit(&self) -> String {
        format!("{}{}", self.base, self.quote)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_and_rejects_query_injection() {
        assert_eq!(Symbol::parse(" btc / usdt ").unwrap().binance(), "BTCUSDT");
        for bad in ["BTCUSDT", "BTC/USDT/ETH", "BTC/USDT&symbol=ETHUSDT", "/USDT", "BTC/"] {
            assert!(Symbol::parse(bad).is_err());
        }
    }
}

impl fmt::Display for Symbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.base, self.quote)
    }
}
