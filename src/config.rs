//! Loading, validation, and saving of the TOML configuration.

use anyhow::{bail, Context, Result};
use serde::Deserialize;

/// One ticker entry. Both formats are supported for backward compatibility:
/// `"BTC/USDT"` and `{ symbol = "BTC/USDT", side = "sell" }`.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum SymbolConfig {
    Simple(String),
    Detailed {
        symbol: String,
        #[serde(default)]
        side: Option<String>,
    },
}

impl SymbolConfig {
    pub fn symbol(&self) -> &str {
        match self {
            Self::Simple(symbol) => symbol,
            Self::Detailed { symbol, .. } => symbol,
        }
    }

    /// Cancellation side for this ticker. If omitted, the exchange-level fallback is used.
    pub fn side<'a>(&'a self, fallback: &'a str) -> &'a str {
        match self {
            Self::Simple(_) => fallback,
            Self::Detailed { side: Some(side), .. } => side,
            Self::Detailed { side: None, .. } => fallback,
        }
    }
}

/// Root configuration object.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    pub poll_milliseconds: Option<u64>,
    pub poll_seconds: Option<u64>,
    pub binance: Option<BinanceConfig>,
    pub okx: Option<OkxConfig>,
    pub bybit: Option<BybitConfig>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BinanceConfig {
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub api_secret: String,
    #[serde(default)]
    pub symbols: Vec<SymbolConfig>,
    #[serde(default = "default_spot")]
    pub market: String,
    /// Fallback for legacy symbols entries and ticker objects without their own side.
    #[serde(default = "default_both")]
    pub side: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OkxConfig {
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub api_secret: String,
    #[serde(default)]
    pub passphrase: String,
    #[serde(default)]
    pub symbols: Vec<SymbolConfig>,
    #[serde(default = "default_spot")]
    pub market: String,
    #[serde(default = "default_both")]
    pub side: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BybitConfig {
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub api_secret: String,
    #[serde(default)]
    pub symbols: Vec<SymbolConfig>,
    #[serde(default = "default_spot")]
    pub market: String,
    #[serde(default = "default_both")]
    pub side: String,
}

fn default_spot() -> String { "spot".to_string() }
fn default_both() -> String { "both".to_string() }
fn credentials_present(api_key: &str, api_secret: &str) -> bool {
    !api_key.trim().is_empty() && !api_secret.trim().is_empty()
}

impl BinanceConfig { pub fn enabled(&self) -> bool { credentials_present(&self.api_key, &self.api_secret) } }
impl OkxConfig { pub fn enabled(&self) -> bool { credentials_present(&self.api_key, &self.api_secret) } }
impl BybitConfig { pub fn enabled(&self) -> bool { credentials_present(&self.api_key, &self.api_secret) } }

impl AppConfig {
    pub fn load(path: &str) -> Result<Self> {
        let text = std::fs::read_to_string(path).with_context(|| format!("failed to read file {path}"))?;
        let config: Self = toml::from_str(&text).context("failed to parse TOML configuration")?;
        config.poll_interval()?;
        config.validate_exchanges()?;
        Ok(config)
    }

    pub fn poll_interval(&self) -> Result<std::time::Duration> {
        let millis = match (self.poll_milliseconds, self.poll_seconds) {
            (Some(_), Some(_)) => bail!("set only poll_milliseconds or poll_seconds, not both"),
            (Some(ms), None) => ms,
            (None, Some(seconds)) => seconds.checked_mul(1000).context("poll_seconds is too large")?,
            (None, None) => 5000,
        };
        if millis == 0 || millis > 86_400_000 {
            bail!("poll interval must be between 1 and 86400000 ms (24 hours)");
        }
        Ok(std::time::Duration::from_millis(millis))
    }

    fn validate_exchanges(&self) -> Result<()> {
        fn valid_side(side: &str) -> bool {
            matches!(side.trim().to_ascii_lowercase().as_str(), "buy" | "sell" | "both")
        }
        fn check_enabled(symbols: &[SymbolConfig], market: &str, fallback_side: &str, allowed: &[&str], name: &str) -> Result<()> {
            if symbols.is_empty() {
                bail!("[{name}]: symbols must not be empty for an enabled exchange");
            }
            let normalized = market.trim().to_ascii_lowercase();
            if !allowed.contains(&normalized.as_str()) {
                bail!("[{name}]: unsupported market {market:?}");
            }
            if !valid_side(fallback_side) {
                bail!("[{name}]: unsupported side {fallback_side:?}; allowed values: buy, sell, both");
            }
            for item in symbols {
                if item.symbol().trim().is_empty() {
                    bail!("[{name}]: empty symbol in symbols list");
                }
                let side = item.side(fallback_side);
                if !valid_side(side) {
                    bail!("[{name}]: unsupported side {side:?} for symbol {:?}; allowed values: buy, sell, both", item.symbol());
                }
            }
            Ok(())
        }

        if let Some(c) = &self.binance {
            if c.enabled() { check_enabled(&c.symbols, &c.market, &c.side, &["spot", "futures"], "binance")?; }
        }
        if let Some(c) = &self.okx {
            if c.enabled() {
                check_enabled(&c.symbols, &c.market, &c.side, &["spot", "swap"], "okx")?;
                if c.passphrase.trim().is_empty() { bail!("[okx]: passphrase is empty"); }
            }
        }
        if let Some(c) = &self.bybit {
            if c.enabled() { check_enabled(&c.symbols, &c.market, &c.side, &["spot", "linear"], "bybit")?; }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parse(fields: &str) -> AppConfig { toml::from_str(fields).unwrap() }

    #[test]
    fn millisecond_and_legacy_intervals() {
        assert_eq!(parse("poll_milliseconds = 25").poll_interval().unwrap().as_millis(), 25);
        assert_eq!(parse("poll_seconds = 2").poll_interval().unwrap().as_millis(), 2000);
        assert_eq!(parse("").poll_interval().unwrap().as_millis(), 5000);
    }

    #[test]
    fn empty_keys_disable_only_that_exchange() {
        let c = parse(r#"
            [binance]
            api_key = "key"
            api_secret = "secret"
            symbols = ["BTC/USDT"]
            [okx]
            api_key = ""
            api_secret = ""
        "#);
        assert!(c.validate_exchanges().is_ok());
        assert!(c.binance.as_ref().unwrap().enabled());
        assert!(!c.okx.as_ref().unwrap().enabled());
    }

    #[test]
    fn per_symbol_side_and_fallback_work() {
        let c = parse(r#"
            [binance]
            api_key = "key"
            api_secret = "secret"
            market = "futures"
            side = "buy"
            symbols = [
                { symbol = "BTC/USDT", side = "sell" },
                { symbol = "ETH/USDT", side = "both" },
                "LA/USDT",
                { symbol = "SOL/USDT" }
            ]
        "#);
        assert!(c.validate_exchanges().is_ok());
        let b = c.binance.unwrap();
        assert_eq!(b.symbols[0].side(&b.side), "sell");
        assert_eq!(b.symbols[1].side(&b.side), "both");
        assert_eq!(b.symbols[2].side(&b.side), "buy");
        assert_eq!(b.symbols[3].side(&b.side), "buy");
    }

    #[test]
    fn invalid_symbol_side_fails() {
        let c = parse(r#"
            [bybit]
            api_key = "key"
            api_secret = "secret"
            symbols = [{ symbol = "BTC/USDT", side = "short" }]
        "#);
        assert!(c.validate_exchanges().is_err());
    }

    #[test]
    fn old_symbols_format_remains_valid() {
        let c = parse(r#"
            [okx]
            api_key = "key"
            api_secret = "secret"
            passphrase = "pass"
            side = "sell"
            symbols = ["BTC/USDT", "ETH/USDT"]
        "#);
        assert!(c.validate_exchanges().is_ok());
        let o = c.okx.unwrap();
        assert_eq!(o.symbols[0].side(&o.side), "sell");
    }

    #[test]
    fn unknown_settings_fail() {
        assert!(toml::from_str::<AppConfig>("poll_miliseconds=10").is_err());
    }
}
