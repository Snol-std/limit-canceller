//! Bulk cancellation of regular active orders for the configured symbols.
mod config;
mod exchange;
mod symbol;

use anyhow::{bail, Context, Result};
use tracing::{info, warn};
use config::{AppConfig, SymbolConfig};
use exchange::binance::{Binance, BinanceMarket};
use exchange::{CancelSide, SymbolRule};
use exchange::bybit::{Bybit, BybitMarket};
use exchange::okx::{Okx, OkxMarket};
use symbol::Symbol;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_ansi(false).with_env_filter(
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
    ).init();
    if let Err(error) = run_inner().await {
        tracing::error!(error = %format!("{error:#}"), "fatal error");
        std::process::exit(1);
    }
}

fn parse_symbol_rules(exchange: &str, raw_symbols: &[SymbolConfig], fallback_side: &str) -> Result<Vec<SymbolRule>> {
    let mut rules: Vec<SymbolRule> = Vec::new();
    for raw in raw_symbols {
        let symbol = Symbol::parse(raw.symbol())
            .with_context(|| format!("[{exchange}]: invalid symbol {:?}", raw.symbol()))?;
        let cancel_side = CancelSide::parse(raw.side(fallback_side))
            .with_context(|| format!("[{exchange}]: invalid side for {}", raw.symbol()))?;

        // Listing the same ticker twice is dangerous: two workers could cancel orders independently.
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

async fn run_inner() -> Result<()> {
    let path = std::env::args().nth(1).unwrap_or_else(|| "config.toml".into());
    let config = AppConfig::load(&path)?;
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
            let market = BybitMarket::parse(&c.market)?;
            let exchange = Bybit::new(&c.api_key, &c.api_secret, market)?;
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
        warn!("no exchanges have both api_key and api_secret configured");
        return Ok(());
    }
    info!(exchanges = tasks.len(), "ready; press Ctrl+C to stop");

    tokio::select! {
        signal = tokio::signal::ctrl_c() => {
            signal.context("waiting for Ctrl+C")?;
            info!("Ctrl+C received; stopping tasks");
        }
        result = tasks.join_next() => {
            match result {
                Some(Ok(Err(error))) => return Err(error.context("exchange task stopped")),
                Some(Err(error)) => bail!("exchange task failed: {error}"),
                _ => bail!("exchange task stopped unexpectedly"),
            }
        }
    }
    tasks.abort_all();
    while tasks.join_next().await.is_some() {}
    Ok(())
}
