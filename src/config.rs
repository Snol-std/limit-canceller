//! Loading, validation, and saving of the TOML configuration.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

/// One ticker entry. Both formats are supported for backward compatibility:
/// `"BTC/USDT"` and `{ symbol = "BTC/USDT", side = "sell" }`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SymbolConfig {
    Simple(String),
    Detailed {
        symbol: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poll_milliseconds: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poll_seconds: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui_language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binance: Option<BinanceConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub okx: Option<OkxConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bybit: Option<BybitConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            poll_milliseconds: Some(100),
            poll_seconds: None,
            ui_language: Some("en".to_string()),
            binance: Some(BinanceConfig::default()),
            okx: Some(OkxConfig::default()),
            bybit: Some(BybitConfig::default()),
        }
    }
}

impl Default for BinanceConfig {
    fn default() -> Self {
        Self { api_key: String::new(), api_secret: String::new(), symbols: Vec::new(), market: default_spot(), side: default_both() }
    }
}
impl Default for OkxConfig {
    fn default() -> Self {
        Self { api_key: String::new(), api_secret: String::new(), passphrase: String::new(), symbols: Vec::new(), market: default_spot(), side: default_both() }
    }
}
impl Default for BybitConfig {
    fn default() -> Self {
        Self { api_key: String::new(), api_secret: String::new(), symbols: Vec::new(), market: default_spot(), side: default_both() }
    }
}

impl BinanceConfig { pub fn enabled(&self) -> bool { credentials_present(&self.api_key, &self.api_secret) } }
impl OkxConfig { pub fn enabled(&self) -> bool { credentials_present(&self.api_key, &self.api_secret) } }
impl BybitConfig { pub fn enabled(&self) -> bool { credentials_present(&self.api_key, &self.api_secret) } }

impl AppConfig {
    pub fn load(path: &str) -> Result<Self> {
        let text = std::fs::read_to_string(path).with_context(|| format!("failed to read file {path}"))?;
        let config: Self = toml::from_str(&text).context("failed to parse TOML configuration")?;
        config.validate()?;
        Ok(config)
    }

    pub fn load_or_default(path: &str) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                let config: Self = toml::from_str(&text).context("failed to parse TOML configuration")?;
                Ok(config)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error).with_context(|| format!("failed to read file {path}")),
        }
    }

    pub fn save(&self, path: &str) -> Result<()> {
        self.validate()?;
        let text = toml::to_string_pretty(self).context("failed to serialize TOML configuration")?;
        std::fs::write(path, text).with_context(|| format!("failed to write file {path}"))
    }

    pub fn has_enabled_exchange(&self) -> bool {
        self.binance.as_ref().is_some_and(BinanceConfig::enabled)
            || self.okx.as_ref().is_some_and(OkxConfig::enabled)
            || self.bybit.as_ref().is_some_and(BybitConfig::enabled)
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

    pub fn validate(&self) -> Result<()> {
        self.poll_interval()?;
        if let Some(language) = &self.ui_language {
            if !matches!(language.trim().to_ascii_lowercase().as_str(), "en" | "ru") {
                bail!("unsupported ui_language {language:?}; allowed values: en, ru");
            }
        }
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
        assert!(c.validate().is_ok());
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
        assert!(c.validate().is_ok());
        let b = c.binance.unwrap();
        assert_eq!(b.symbols[0].side(&b.side), "sell");
        assert_eq!(b.symbols[1].side(&b.side), "both");
        assert_eq!(b.symbols[2].side(&b.side), "buy");
        assert_eq!(b.symbols[3].side(&b.side), "buy");
    }

    #[test]
    fn save_roundtrip_works() {
        let c = AppConfig::default();
        let text = toml::to_string_pretty(&c).unwrap();
        let decoded: AppConfig = toml::from_str(&text).unwrap();
        assert_eq!(decoded.poll_milliseconds, Some(100));
    }

    #[test]
    fn invalid_symbol_side_fails() {
        let c = parse(r#"
            [bybit]
            api_key = "key"
            api_secret = "secret"
            symbols = [{ symbol = "BTC/USDT", side = "short" }]
        "#);
        assert!(c.validate().is_err());
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
        assert!(c.validate().is_ok());
        let o = c.okx.unwrap();
        assert_eq!(o.symbols[0].side(&o.side), "sell");
    }

    #[test]
    fn unknown_settings_fail() {
        assert!(toml::from_str::<AppConfig>("poll_miliseconds=10").is_err());
    }
    #[test]
    fn ui_language_is_optional_and_validated() {
        assert!(parse("ui_language = \"en\"").validate().is_ok());
        assert!(parse("ui_language = \"ru\"").validate().is_ok());
        assert!(parse("").validate().is_ok());
        assert!(parse("ui_language = \"de\"").validate().is_err());
    }

}
