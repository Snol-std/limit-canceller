//! Loading, validation, and saving of the TOML configuration.

use anyhow::{bail, Context, Result};
use serde::Deserialize;

use crate::symbol::Symbol;

/// One ticker rule. Market, symbol, and cancellation side always belong to the ticker itself.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SymbolConfig {
    pub market: String,
    pub symbol: String,
    pub side: String,
}

/// Root configuration object.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    #[serde(default)]
    pub poll_milliseconds: Option<u64>,
    #[serde(default)]
    pub poll_seconds: Option<u64>,
    #[serde(default)]
    pub ui_language: Option<String>,
    #[serde(default)]
    pub binance: Option<BinanceConfig>,
    #[serde(default)]
    pub okx: Option<OkxConfig>,
    #[serde(default)]
    pub bybit: Option<BybitConfig>,
    #[serde(default)]
    pub tigerx: Option<TigerXConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BinanceConfig {
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub api_secret: String,
    #[serde(default)]
    pub symbols: Vec<SymbolConfig>,
}

#[derive(Debug, Clone, Deserialize)]
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
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BybitConfig {
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub api_secret: String,
    #[serde(default)]
    pub symbols: Vec<SymbolConfig>,
}

/// One TigerX ticker rule. The underlying exchange (`binance` / `okx`) and
/// the business type (`spot` / `perp`) belong to the ticker itself because a
/// single TigerX account aggregates both.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TigerXSymbolConfig {
    pub exchange: String,
    pub market: String,
    pub symbol: String,
    pub side: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TigerXConfig {
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub api_secret: String,
    #[serde(default)]
    pub symbols: Vec<TigerXSymbolConfig>,
}

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
            tigerx: Some(TigerXConfig::default()),
        }
    }
}

impl Default for BinanceConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            api_secret: String::new(),
            symbols: Vec::new(),
        }
    }
}

impl Default for OkxConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            api_secret: String::new(),
            passphrase: String::new(),
            symbols: Vec::new(),
        }
    }
}

impl Default for BybitConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            api_secret: String::new(),
            symbols: Vec::new(),
        }
    }
}

impl Default for TigerXConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            api_secret: String::new(),
            symbols: Vec::new(),
        }
    }
}

impl BinanceConfig {
    pub fn enabled(&self) -> bool {
        credentials_present(&self.api_key, &self.api_secret)
    }
}

impl OkxConfig {
    pub fn enabled(&self) -> bool {
        credentials_present(&self.api_key, &self.api_secret)
    }
}

impl BybitConfig {
    pub fn enabled(&self) -> bool {
        credentials_present(&self.api_key, &self.api_secret)
    }
}

impl TigerXConfig {
    pub fn enabled(&self) -> bool {
        credentials_present(&self.api_key, &self.api_secret)
    }
}

impl AppConfig {
    pub fn load(path: &str) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read file {path}"))?;
        let config: Self = toml::from_str(&text).context("failed to parse TOML configuration")?;
        config.validate()?;
        Ok(config)
    }

    pub fn load_or_default(path: &str) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                let config: Self =
                    toml::from_str(&text).context("failed to parse TOML configuration")?;
                config.validate()?;
                Ok(config)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error).with_context(|| format!("failed to read file {path}")),
        }
    }

    /// Saves a stable, compact GUI-owned TOML format.
    ///
    /// `toml::to_string_pretty` serializes `Vec<struct>` as arrays of tables (`[[...]]`).
    /// The GUI intentionally writes ticker rules as inline tables instead:
    /// `symbols = [{ market = "spot", symbol = "BTC/USDT", side = "both" }]`.
    pub fn save(&self, path: &str) -> Result<()> {
        self.validate()?;
        let text = self.to_gui_toml();
        std::fs::write(path, text).with_context(|| format!("failed to write file {path}"))
    }

    pub fn has_enabled_exchange(&self) -> bool {
        self.binance.as_ref().is_some_and(BinanceConfig::enabled)
            || self.okx.as_ref().is_some_and(OkxConfig::enabled)
            || self.bybit.as_ref().is_some_and(BybitConfig::enabled)
            || self.tigerx.as_ref().is_some_and(TigerXConfig::enabled)
    }

    pub fn poll_interval(&self) -> Result<std::time::Duration> {
        let millis = match (self.poll_milliseconds, self.poll_seconds) {
            (Some(_), Some(_)) => bail!("set only poll_milliseconds or poll_seconds, not both"),
            (Some(ms), None) => ms,
            (None, Some(seconds)) => seconds
                .checked_mul(1000)
                .context("poll_seconds is too large")?,
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
            matches!(
                side.trim().to_ascii_lowercase().as_str(),
                "buy" | "sell" | "both"
            )
        }

        fn check_symbols(
            symbols: &[SymbolConfig],
            allowed_markets: &[&str],
            name: &str,
            enabled: bool,
        ) -> Result<()> {
            if enabled && symbols.is_empty() {
                bail!("[{name}]: symbols must not be empty for an enabled exchange");
            }

            let mut seen: Vec<(String, Symbol)> = Vec::new();
            for item in symbols {
                let market = item.market.trim().to_ascii_lowercase();
                if !allowed_markets.contains(&market.as_str()) {
                    bail!(
                        "[{name}]: unsupported market {:?} for symbol {:?}",
                        item.market,
                        item.symbol
                    );
                }
                if !valid_side(&item.side) {
                    bail!(
                        "[{name}]: unsupported side {:?} for symbol {:?}; allowed values: buy, sell, both",
                        item.side,
                        item.symbol
                    );
                }
                let symbol = Symbol::parse(&item.symbol)
                    .with_context(|| format!("[{name}]: invalid symbol {:?}", item.symbol))?;
                if seen
                    .iter()
                    .any(|(seen_market, seen_symbol)| seen_market == &market && seen_symbol == &symbol)
                {
                    bail!(
                        "[{name}]: symbol {symbol} appears more than once in market {market}"
                    );
                }
                seen.push((market, symbol));
            }
            Ok(())
        }

        fn check_tigerx_symbols(symbols: &[TigerXSymbolConfig], enabled: bool) -> Result<()> {
            if enabled && symbols.is_empty() {
                bail!("[tigerx]: symbols must not be empty for an enabled exchange");
            }

            let mut seen: Vec<(String, String, Symbol)> = Vec::new();
            for item in symbols {
                let exchange = item.exchange.trim().to_ascii_lowercase();
                if !matches!(exchange.as_str(), "binance" | "okx") {
                    bail!(
                        "[tigerx]: unsupported exchange {:?} for symbol {:?}; allowed values: binance, okx",
                        item.exchange,
                        item.symbol
                    );
                }
                let market = item.market.trim().to_ascii_lowercase();
                if !matches!(market.as_str(), "spot" | "perp") {
                    bail!(
                        "[tigerx]: unsupported market {:?} for symbol {:?}; allowed values: spot, perp",
                        item.market,
                        item.symbol
                    );
                }
                if !valid_side(&item.side) {
                    bail!(
                        "[tigerx]: unsupported side {:?} for symbol {:?}; allowed values: buy, sell, both",
                        item.side,
                        item.symbol
                    );
                }
                let symbol = Symbol::parse(&item.symbol)
                    .with_context(|| format!("[tigerx]: invalid symbol {:?}", item.symbol))?;
                if seen
                    .iter()
                    .any(|(seen_exchange, seen_market, seen_symbol)| {
                        seen_exchange == &exchange && seen_market == &market && seen_symbol == &symbol
                    })
                {
                    bail!(
                        "[tigerx]: symbol {symbol} appears more than once in {exchange} {market}"
                    );
                }
                seen.push((exchange, market, symbol));
            }
            Ok(())
        }

        if let Some(c) = &self.binance {
            check_symbols(&c.symbols, &["spot", "futures"], "binance", c.enabled())?;
        }
        if let Some(c) = &self.okx {
            check_symbols(&c.symbols, &["spot", "swap"], "okx", c.enabled())?;
            if c.enabled() && c.passphrase.trim().is_empty() {
                bail!("[okx]: passphrase is empty");
            }
        }
        if let Some(c) = &self.bybit {
            check_symbols(&c.symbols, &["spot", "linear"], "bybit", c.enabled())?;
        }
        if let Some(c) = &self.tigerx {
            check_tigerx_symbols(&c.symbols, c.enabled())?;
        }
        Ok(())
    }

    fn to_gui_toml(&self) -> String {
        let mut out = String::new();

        if let Some(ms) = self.poll_milliseconds {
            out.push_str(&format!("poll_milliseconds = {ms}\n"));
        }
        if let Some(seconds) = self.poll_seconds {
            out.push_str(&format!("poll_seconds = {seconds}\n"));
        }
        if let Some(language) = &self.ui_language {
            out.push_str(&format!("ui_language = {}\n", toml_string(language)));
        }

        if let Some(c) = &self.binance {
            out.push('\n');
            out.push_str("[binance]\n");
            out.push_str(&format!("api_key = {}\n", toml_string(&c.api_key)));
            out.push_str(&format!("api_secret = {}\n", toml_string(&c.api_secret)));
            write_symbols(&mut out, &c.symbols);
        }

        if let Some(c) = &self.okx {
            out.push('\n');
            out.push_str("[okx]\n");
            out.push_str(&format!("api_key = {}\n", toml_string(&c.api_key)));
            out.push_str(&format!("api_secret = {}\n", toml_string(&c.api_secret)));
            out.push_str(&format!("passphrase = {}\n", toml_string(&c.passphrase)));
            write_symbols(&mut out, &c.symbols);
        }

        if let Some(c) = &self.bybit {
            out.push('\n');
            out.push_str("[bybit]\n");
            out.push_str(&format!("api_key = {}\n", toml_string(&c.api_key)));
            out.push_str(&format!("api_secret = {}\n", toml_string(&c.api_secret)));
            write_symbols(&mut out, &c.symbols);
        }

        if let Some(c) = &self.tigerx {
            out.push('\n');
            out.push_str("[tigerx]\n");
            out.push_str(&format!("api_key = {}\n", toml_string(&c.api_key)));
            out.push_str(&format!("api_secret = {}\n", toml_string(&c.api_secret)));
            write_tigerx_symbols(&mut out, &c.symbols);
        }

        out
    }
}

fn write_symbols(out: &mut String, symbols: &[SymbolConfig]) {
    if symbols.is_empty() {
        out.push_str("symbols = []\n");
        return;
    }

    out.push_str("symbols = [\n");
    for item in symbols {
        out.push_str("  { market = ");
        out.push_str(&toml_string(item.market.trim()));
        out.push_str(", symbol = ");
        out.push_str(&toml_string(item.symbol.trim()));
        out.push_str(", side = ");
        out.push_str(&toml_string(item.side.trim()));
        out.push_str(" },\n");
    }
    out.push_str("]\n");
}

fn write_tigerx_symbols(out: &mut String, symbols: &[TigerXSymbolConfig]) {
    if symbols.is_empty() {
        out.push_str("symbols = []\n");
        return;
    }

    out.push_str("symbols = [\n");
    for item in symbols {
        out.push_str("  { exchange = ");
        out.push_str(&toml_string(item.exchange.trim()));
        out.push_str(", market = ");
        out.push_str(&toml_string(item.market.trim()));
        out.push_str(", symbol = ");
        out.push_str(&toml_string(item.symbol.trim()));
        out.push_str(", side = ");
        out.push_str(&toml_string(item.side.trim()));
        out.push_str(" },\n");
    }
    out.push_str("]\n");
}

fn toml_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000C}' => out.push_str("\\f"),
            c if c <= '\u{001F}' || c == '\u{007F}' => {
                out.push_str(&format!("\\u{:04X}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(fields: &str) -> AppConfig {
        toml::from_str(fields).unwrap()
    }

    #[test]
    fn millisecond_and_legacy_intervals() {
        assert_eq!(
            parse("poll_milliseconds = 25")
                .poll_interval()
                .unwrap()
                .as_millis(),
            25
        );
        assert_eq!(
            parse("poll_seconds = 2")
                .poll_interval()
                .unwrap()
                .as_millis(),
            2000
        );
        assert_eq!(parse("").poll_interval().unwrap().as_millis(), 5000);
    }

    #[test]
    fn ticker_market_and_side_are_per_entry() {
        let c = parse(
            r#"
            [binance]
            api_key = "key"
            api_secret = "secret"
            symbols = [
                { market = "futures", symbol = "BTC/USDT", side = "sell" },
                { market = "spot", symbol = "ETH/USDC", side = "both" },
                { market = "spot", symbol = "LA/USDT", side = "buy" },
                { market = "futures", symbol = "LA/USDT", side = "buy" }
            ]
        "#,
        );
        assert!(c.validate().is_ok());
        let b = c.binance.unwrap();
        assert_eq!(b.symbols[0].market, "futures");
        assert_eq!(b.symbols[1].symbol, "ETH/USDC");
        assert_eq!(b.symbols[2].side, "buy");
    }

    #[test]
    fn same_symbol_is_allowed_in_different_markets() {
        let c = parse(
            r#"
            [binance]
            api_key = "key"
            api_secret = "secret"
            symbols = [
                { market = "spot", symbol = "LA/USDT", side = "both" },
                { market = "futures", symbol = "LA/USDT", side = "both" }
            ]
        "#,
        );
        assert!(c.validate().is_ok());
    }

    #[test]
    fn duplicate_symbol_in_same_market_fails_after_normalization() {
        let c = parse(
            r#"
            [binance]
            api_key = "key"
            api_secret = "secret"
            symbols = [
                { market = "spot", symbol = "btc", side = "both" },
                { market = "spot", symbol = "BTCUSDT", side = "sell" }
            ]
        "#,
        );
        assert!(c.validate().is_err());
    }

    #[test]
    fn old_exchange_level_market_and_side_are_rejected() {
        let result = toml::from_str::<AppConfig>(
            r#"
            [binance]
            api_key = "key"
            api_secret = "secret"
            market = "futures"
            side = "both"
            symbols = []
        "#,
        );
        assert!(result.is_err());
    }

    #[test]
    fn old_string_symbol_format_is_rejected() {
        let result = toml::from_str::<AppConfig>(
            r#"
            [binance]
            api_key = "key"
            api_secret = "secret"
            symbols = ["BTC/USDT"]
        "#,
        );
        assert!(result.is_err());
    }

    #[test]
    fn compact_save_format_uses_inline_ticker_tables() {
        let c = parse(
            r#"
            poll_milliseconds = 100
            ui_language = "en"
            [binance]
            api_key = ""
            api_secret = ""
            symbols = [
                { market = "futures", symbol = "BTC/USDT", side = "sell" },
                { market = "spot", symbol = "ETH/USDC", side = "both" }
            ]
        "#,
        );
        let text = c.to_gui_toml();
        assert!(text.contains("symbols = ["));
        assert!(text.contains("{ market = \"futures\", symbol = \"BTC/USDT\", side = \"sell\" },"));
        assert!(!text.contains("[[binance.symbols]]"));
        let decoded: AppConfig = toml::from_str(&text).unwrap();
        assert_eq!(decoded.binance.unwrap().symbols.len(), 2);
    }

    #[test]
    fn empty_keys_disable_only_that_exchange() {
        let c = parse(
            r#"
            [binance]
            api_key = "key"
            api_secret = "secret"
            symbols = [{ market = "spot", symbol = "BTC/USDT", side = "both" }]
            [okx]
            api_key = ""
            api_secret = ""
            symbols = []
        "#,
        );
        assert!(c.validate().is_ok());
        assert!(c.binance.as_ref().unwrap().enabled());
        assert!(!c.okx.as_ref().unwrap().enabled());
    }

    #[test]
    fn ui_language_is_optional_and_validated() {
        assert!(parse("ui_language = \"en\"").validate().is_ok());
        assert!(parse("ui_language = \"ru\"").validate().is_ok());
        assert!(parse("").validate().is_ok());
        assert!(parse("ui_language = \"de\"").validate().is_err());
    }

    #[test]
    fn unknown_settings_fail() {
        assert!(toml::from_str::<AppConfig>("poll_miliseconds=10").is_err());
    }

    #[test]
    fn tigerx_symbols_carry_exchange_and_market() {
        let c = parse(
            r#"
            [tigerx]
            api_key = "key"
            api_secret = "secret"
            symbols = [
                { exchange = "binance", market = "perp", symbol = "BTC/USDT", side = "both" },
                { exchange = "okx", market = "spot", symbol = "ETH/USDC", side = "sell" },
                { exchange = "okx", market = "perp", symbol = "ETH/USDC", side = "buy" }
            ]
        "#,
        );
        assert!(c.validate().is_ok());
        let t = c.tigerx.unwrap();
        assert_eq!(t.symbols.len(), 3);
        assert_eq!(t.symbols[0].exchange, "binance");
        assert_eq!(t.symbols[2].market, "perp");
    }

    #[test]
    fn tigerx_rejects_unknown_exchange_market_side_and_duplicates() {
        let cases = [
            r#"{ exchange = "kraken", market = "perp", symbol = "BTC/USDT", side = "both" }"#,
            r#"{ exchange = "binance", market = "futures", symbol = "BTC/USDT", side = "both" }"#,
            r#"{ exchange = "binance", market = "perp", symbol = "BTC/USDT", side = "left" }"#,
            r#"{ exchange = "binance", market = "perp", symbol = "BTC/EUR", side = "both" }"#,
            r#"
                { exchange = "binance", market = "perp", symbol = "btc", side = "both" },
                { exchange = "binance", market = "perp", symbol = "BTCUSDT", side = "sell" }
            "#,
        ];
        for fields in cases {
            let result = toml::from_str::<AppConfig>(&format!(
                r#"[tigerx]
                api_key = "key"
                api_secret = "secret"
                symbols = [{fields}]"#
            ));
            assert!(result.is_err() || result.as_ref().unwrap().validate().is_err(),
                "case should fail: {fields}");
        }
    }

    #[test]
    fn tigerx_empty_keys_disable_only_that_exchange() {
        let c = parse(
            r#"
            [tigerx]
            api_key = ""
            api_secret = ""
            symbols = []
        "#,
        );
        assert!(c.validate().is_ok());
        assert!(!c.tigerx.as_ref().unwrap().enabled());
        assert!(!c.has_enabled_exchange());
    }

    #[test]
    fn tigerx_save_format_uses_inline_ticker_tables() {
        let c = parse(
            r#"
            [tigerx]
            api_key = "key"
            api_secret = "secret"
            symbols = [
                { exchange = "binance", market = "perp", symbol = "BTC/USDT", side = "sell" }
            ]
        "#,
        );
        let text = c.to_gui_toml();
        assert!(text.contains("[tigerx]\n"));
        assert!(text.contains("{ exchange = \"binance\", market = \"perp\", symbol = \"BTC/USDT\", side = \"sell\" },"));
        let decoded: AppConfig = toml::from_str(&text).unwrap();
        assert_eq!(decoded.tigerx.unwrap().symbols.len(), 1);
    }
}
