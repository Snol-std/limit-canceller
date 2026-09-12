//! Starts the network order-cancellation engine from a validated configuration.

use anyhow::{bail, Context, Result};
use tracing::{info, warn};

use crate::config::{AppConfig, SymbolConfig};
use crate::exchange::binance::{Binance, BinanceMarket};
use crate::exchange::bybit::{Bybit, BybitMarket};
use crate::exchange::okx::{Okx, OkxMarket};
use crate::exchange::{self, CancelSide, SymbolRule};
use crate::symbol::Symbol;

fn rules_for_market(
    exchange: &str,
    raw_symbols: &[SymbolConfig],
    market: &str,
) -> Result<Vec<SymbolRule>> {
    let mut rules = Vec::new();
    for raw in raw_symbols
        .iter()
        .filter(|item| item.market.trim().eq_ignore_ascii_case(market))
    {
        let symbol = Symbol::parse(&raw.symbol)
            .with_context(|| format!("[{exchange}]: invalid symbol {:?}", raw.symbol))?;
        let cancel_side = CancelSide::parse(&raw.side)
            .with_context(|| format!("[{exchange}]: invalid side for {}", raw.symbol))?;
        rules.push(SymbolRule {
            symbol,
            cancel_side,
        });
    }
    Ok(rules)
}

/// Starts every configured exchange/market pair and runs until the future is cancelled
/// by the GUI or one of the worker groups fails.
pub async fn run(config: AppConfig) -> Result<()> {
    config.validate()?;
    let interval = config.poll_interval()?;
    if config.poll_seconds.is_some() {
        warn!("poll_seconds is deprecated; use poll_milliseconds (1 second = 1000 ms)");
    }

    info!(
        interval_ms = interval.as_millis() as u64,
        "starting limit-canceller"
    );

    let mut tasks = tokio::task::JoinSet::new();

    match config.binance.as_ref() {
        Some(c) if c.enabled() => {
            for (market_name, market) in [
                ("spot", BinanceMarket::Spot),
                ("futures", BinanceMarket::Futures),
            ] {
                let rules = rules_for_market("binance", &c.symbols, market_name)?;
                if rules.is_empty() {
                    continue;
                }
                let exchange = Binance::new(&c.api_key, &c.api_secret, market)?;
                tasks.spawn(exchange::run(exchange, rules, interval));
            }
        }
        Some(_) => warn!(
            exchange = "binance",
            "skipping exchange: api_key or api_secret is empty"
        ),
        None => {}
    }

    match config.okx.as_ref() {
        Some(c) if c.enabled() => {
            for (market_name, market) in [("spot", OkxMarket::Spot), ("swap", OkxMarket::Swap)] {
                let rules = rules_for_market("okx", &c.symbols, market_name)?;
                if rules.is_empty() {
                    continue;
                }
                let exchange = Okx::new(&c.api_key, &c.api_secret, &c.passphrase, market)?;
                tasks.spawn(exchange::run(exchange, rules, interval));
            }
        }
        Some(_) => warn!(
            exchange = "okx",
            "skipping exchange: api_key or api_secret is empty"
        ),
        None => {}
    }

    match config.bybit.as_ref() {
        Some(c) if c.enabled() => {
            for (market_name, market) in [
                ("spot", BybitMarket::Spot),
                ("linear", BybitMarket::Linear),
            ] {
                let rules = rules_for_market("bybit", &c.symbols, market_name)?;
                if rules.is_empty() {
                    continue;
                }
                let exchange = Bybit::new(&c.api_key, &c.api_secret, market)?;
                tasks.spawn(exchange::run(exchange, rules, interval));
            }
        }
        Some(_) => warn!(
            exchange = "bybit",
            "skipping exchange: api_key or api_secret is empty"
        ),
        None => {}
    }

    if tasks.is_empty() {
        bail!("no enabled exchange has at least one configured ticker");
    }
    info!(worker_groups = tasks.len(), "engine started");

    match tasks.join_next().await {
        Some(Ok(Err(error))) => Err(error.context("exchange task stopped")),
        Some(Err(error)) => bail!("exchange task failed: {error}"),
        _ => bail!("exchange task stopped unexpectedly"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_rules_by_market() {
        let symbols = vec![
            SymbolConfig {
                market: "spot".into(),
                symbol: "btc".into(),
                side: "both".into(),
            },
            SymbolConfig {
                market: "futures".into(),
                symbol: "ethusdc".into(),
                side: "sell".into(),
            },
        ];

        let spot = rules_for_market("binance", &symbols, "spot").unwrap();
        let futures = rules_for_market("binance", &symbols, "futures").unwrap();
        assert_eq!(spot.len(), 1);
        assert_eq!(spot[0].symbol.to_string(), "BTC/USDT");
        assert_eq!(futures.len(), 1);
        assert_eq!(futures[0].symbol.to_string(), "ETH/USDC");
    }
}
