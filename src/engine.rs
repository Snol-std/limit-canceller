//! Starts the network order-cancellation engine from a validated configuration.

use anyhow::{bail, Context, Result};
use tracing::{info, warn};

use crate::config::{AppConfig, SymbolConfig};
use crate::exchange::binance::{Binance, BinanceMarket};
use crate::exchange::bybit::{Bybit, BybitMarket};
use crate::exchange::okx::{Okx, OkxMarket};
use crate::exchange::{self, CancelSide, SymbolRule};
use crate::symbol::Symbol;

fn parse_symbol_rules(exchange: &str, raw_symbols: &[SymbolConfig], fallback_side: &str) -> Result<Vec<SymbolRule>> {
    let mut rules: Vec<SymbolRule> = Vec::new();
    for raw in raw_symbols {
        let symbol = Symbol::parse(raw.symbol())
            .with_context(|| format!("[{exchange}]: invalid symbol {:?}", raw.symbol()))?;
        let cancel_side = CancelSide::parse(raw.side(fallback_side))
            .with_context(|| format!("[{exchange}]: invalid side for {}", raw.symbol()))?;

        if rules.iter().any(|rule| rule.symbol == symbol) {
            bail!("[{exchange}]: symbol {symbol} appears more than once in symbols");
        }
        rules.push(SymbolRule { symbol, cancel_side });
    }
    if rules.is_empty() {
        bail!("[{exchange}]: symbols list is empty after normalization");
    }
    Ok(rules)
}

/// Starts all enabled exchanges and runs until the future is cancelled
/// (the GUI does this through iced::task::Handle) or until one of the tasks fails.
pub async fn run(config: AppConfig) -> Result<()> {
    config.validate()?;
    let interval = config.poll_interval()?;
    if config.poll_seconds.is_some() {
        warn!("poll_seconds is deprecated; use poll_milliseconds (1 second = 1000 ms)");
    }

    let binance = match config.binance.as_ref() {
        Some(c) if c.enabled() => {
            let rules = parse_symbol_rules("binance", &c.symbols, &c.side)?;
            let exchange = Binance::new(&c.api_key, &c.api_secret, BinanceMarket::parse(&c.market)?)?;
            Some((exchange, rules))
        }
        Some(_) => { warn!(exchange = "binance", "skipping exchange: api_key or api_secret is empty"); None }
        None => None,
    };

    let okx = match config.okx.as_ref() {
        Some(c) if c.enabled() => {
            let rules = parse_symbol_rules("okx", &c.symbols, &c.side)?;
            let exchange = Okx::new(&c.api_key, &c.api_secret, &c.passphrase, OkxMarket::parse(&c.market)?)?;
            Some((exchange, rules))
        }
        Some(_) => { warn!(exchange = "okx", "skipping exchange: api_key or api_secret is empty"); None }
        None => None,
    };

    let bybit = match config.bybit.as_ref() {
        Some(c) if c.enabled() => {
            let rules = parse_symbol_rules("bybit", &c.symbols, &c.side)?;
            let exchange = Bybit::new(&c.api_key, &c.api_secret, BybitMarket::parse(&c.market)?)?;
            Some((exchange, rules))
        }
        Some(_) => { warn!(exchange = "bybit", "skipping exchange: api_key or api_secret is empty"); None }
        None => None,
    };

    info!(interval_ms = interval.as_millis() as u64, "starting limit-canceller");
    let mut tasks = tokio::task::JoinSet::new();
    if let Some((exchange, rules)) = binance { tasks.spawn(exchange::run(exchange, rules, interval)); }
    if let Some((exchange, rules)) = okx { tasks.spawn(exchange::run(exchange, rules, interval)); }
    if let Some((exchange, rules)) = bybit { tasks.spawn(exchange::run(exchange, rules, interval)); }

    if tasks.is_empty() {
        bail!("no exchanges have both api_key and api_secret configured");
    }
    info!(exchanges = tasks.len(), "engine started");

    match tasks.join_next().await {
        Some(Ok(Err(error))) => Err(error.context("exchange task stopped")),
        Some(Err(error)) => bail!("exchange task failed: {error}"),
        _ => bail!("exchange task stopped unexpectedly"),
    }
}
