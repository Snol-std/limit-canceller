//! Canonical ticker representation and tolerant GUI input parsing.
//!
//! The GUI accepts common user forms such as `btc`, `btcusdt`, `btc/usdt`,
//! `btc\\usdt`, `btcusdc`, `btc/usdc`, and `btc\\usdc`. They are normalized to
//! canonical `BASE/QUOTE` form before being saved to `config.toml`.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub base: String,
    pub quote: String,
}

impl Symbol {
    /// Parses a user-entered symbol and normalizes it to a USDT or USDC market.
    ///
    /// If no quote asset is provided, USDT is used by default.
    pub fn parse(input: &str) -> anyhow::Result<Self> {
        let mut normalized = input.trim().to_ascii_uppercase().replace('\\', "/");
        normalized.retain(|c| !c.is_whitespace());

        if normalized.is_empty() {
            anyhow::bail!("symbol is empty");
        }

        let (base, quote) = if normalized.contains('/') {
            let mut parts = normalized.split('/');
            let base = parts.next().unwrap_or_default();
            let quote = parts.next().unwrap_or_default();
            if parts.next().is_some() {
                anyhow::bail!("symbol {input:?} contains more than one separator");
            }
            (base.to_string(), quote.to_string())
        } else if normalized.ends_with("USDT") && normalized.len() > 4 {
            let split = normalized.len() - 4;
            (normalized[..split].to_string(), "USDT".to_string())
        } else if normalized.ends_with("USDC") && normalized.len() > 4 {
            let split = normalized.len() - 4;
            (normalized[..split].to_string(), "USDC".to_string())
        } else {
            (normalized, "USDT".to_string())
        };

        if base.is_empty() || quote.is_empty() {
            anyhow::bail!("symbol {input:?} contains an empty component");
        }
        if !base
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            anyhow::bail!("symbol {input:?} contains unsupported base characters");
        }
        if !matches!(quote.as_str(), "USDT" | "USDC") {
            anyhow::bail!(
                "symbol {input:?} uses unsupported quote {quote:?}; only USDT and USDC are supported"
            );
        }

        Ok(Self { base, quote })
    }

    pub fn canonical(&self) -> String {
        format!("{}/{}", self.base, self.quote)
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

impl fmt::Display for Symbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.base, self.quote)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_requested_usdt_forms() {
        for input in ["btc", "btcusdt", "btc/usdt", "btc\\usdt", " btc / usdt "] {
            assert_eq!(Symbol::parse(input).unwrap().canonical(), "BTC/USDT");
        }
    }

    #[test]
    fn accepts_requested_usdc_forms() {
        for input in ["btcusdc", "btc/usdc", "btc\\usdc", " BTC / USDC "] {
            assert_eq!(Symbol::parse(input).unwrap().canonical(), "BTC/USDC");
        }
    }

    #[test]
    fn bare_base_defaults_to_usdt() {
        assert_eq!(Symbol::parse("eth").unwrap().canonical(), "ETH/USDT");
        assert_eq!(Symbol::parse("1000pepe").unwrap().canonical(), "1000PEPE/USDT");
    }

    #[test]
    fn rejects_unsupported_quotes_and_injection() {
        for bad in [
            "BTC/EUR",
            "BTC/USDT/ETH",
            "BTC/USDT&symbol=ETHUSDT",
            "/USDT",
            "BTC/",
        ] {
            assert!(Symbol::parse(bad).is_err(), "{bad} should be rejected");
        }
    }
}
