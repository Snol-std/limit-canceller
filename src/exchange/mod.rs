//! Shared core: independent loop per symbol, order cancellation, and request signing.
pub mod binance;
pub mod bybit;
pub mod model;
pub mod okx;
mod transport;
#[cfg(test)]
mod test_support;

pub use model::{CancelSide, Order, Side};
pub(crate) use transport::{query_string, required_string, RequestGate, response_json};

use std::{future::Future, sync::Arc, time::Duration};
use anyhow::{bail, Context, Result};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use tracing::{debug, info, warn};
use crate::symbol::Symbol;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone)]
pub struct SymbolRule {
    pub symbol: Symbol,
    pub cancel_side: CancelSide,
}

pub(crate) fn text_snippet(text: &str) -> String { text.chars().take(200).collect() }

pub(crate) fn hmac_sha256_hex(key: &str, data: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(key.as_bytes()).expect("HMAC accepts any key size");
    mac.update(data.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

pub(crate) fn hmac_sha256_base64(key: &str, data: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(key.as_bytes()).expect("HMAC accepts any key size");
    mac.update(data.as_bytes());
    BASE64.encode(mac.finalize().into_bytes())
}

pub trait Exchange: Send + Sync + 'static {
    fn name(&self) -> &str;
    fn to_exchange_symbol(&self, symbol: &Symbol) -> String;
    fn fetch_open_orders(&self, symbol: &str)
        -> impl Future<Output = Result<Vec<Order>>> + Send;
    /// Cancels the selected orders using the exchange-specific method. Ok means the request was accepted,
    /// not that the final state of every order has already been confirmed.
    fn cancel_orders(&self, symbol: &str, orders: &[Order], side: CancelSide)
        -> impl Future<Output = Result<()>> + Send;
}

async fn sweep<E: Exchange>(exchange: &E, symbol: &str, cancel_side: CancelSide) -> Result<()> {
    let orders = exchange.fetch_open_orders(symbol).await
        .context("fetching open orders")?;
    let orders: Vec<Order> = orders.into_iter()
        .filter(|order| cancel_side.matches(order.side))
        .collect();
    if orders.is_empty() { return Ok(()); }
    for order in &orders {
        debug!(exchange = exchange.name(), symbol, order_id = %order.id,
            side = order.side.as_str(), price = %order.price, quantity = %order.quantity,
            "open order found");
    }
    info!(exchange = exchange.name(), symbol, side = cancel_side.as_str(), found = orders.len(),
        "sending open-order cancellation");
    exchange.cancel_orders(symbol, &orders, cancel_side).await.context("cancelling orders")?;
    info!(exchange = exchange.name(), symbol, found = orders.len(),
        "cancellation requests accepted; any remaining open orders will be handled on the next poll");
    Ok(())
}

async fn run_symbol<E: Exchange>(exchange: Arc<E>, symbol: String, interval: Duration, cancel_side: CancelSide) {
    let mut ticker = tokio::time::interval(interval);
    // Do not catch up missed ticks with a burst of requests after a network delay.
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut failures = 0u32;
    loop {
        ticker.tick().await;
        match sweep(exchange.as_ref(), &symbol, cancel_side).await {
            Ok(()) => failures = 0,
            Err(error) => {
                failures = failures.saturating_add(1);
                let delay = Duration::from_millis((500u64 << failures.min(6)).min(30_000));
                warn!(exchange = exchange.name(), symbol, error = %format!("{error:#}"),
                    retry_ms = delay.as_millis() as u64, "cancellation loop error");
                tokio::time::sleep(delay).await;
                ticker.reset_after(interval);
            }
        }
    }
}

pub async fn run<E: Exchange>(exchange: E, rules: Vec<SymbolRule>, interval: Duration) -> Result<()> {
    let rule_list = rules.iter()
        .map(|rule| format!("{}:{}", rule.symbol, rule.cancel_side.as_str()))
        .collect::<Vec<_>>()
        .join(", ");
    info!(exchange = exchange.name(), interval_ms = interval.as_millis() as u64,
        symbols = %rule_list, count = rules.len(), "order cancellation started");
    let exchange = Arc::new(exchange);
    let mut workers = tokio::task::JoinSet::new();
    for rule in rules {
        let symbol = exchange.to_exchange_symbol(&rule.symbol);
        workers.spawn(run_symbol(exchange.clone(), symbol, interval, rule.cancel_side));
    }
    match workers.join_next().await {
        Some(Err(error)) => bail!("{}: symbol task failed: {error}", exchange.name()),
        _ => bail!("{}: symbol task stopped unexpectedly", exchange.name()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Fake { count: usize, batches: AtomicUsize }
    impl Exchange for Fake {
        fn name(&self) -> &str { "fake" }
        fn to_exchange_symbol(&self, s: &Symbol) -> String { s.binance() }
        async fn fetch_open_orders(&self, _: &str) -> Result<Vec<Order>> {
            Ok((0..self.count).map(|i| Order { id: i.to_string(), side: Side::Buy,
                price: "1".into(), quantity: "1".into() }).collect())
        }
        async fn cancel_orders(&self, _: &str, orders: &[Order], _: CancelSide) -> Result<()> {
            assert_eq!(orders.len(), self.count);
            self.batches.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }
    #[tokio::test]
    async fn one_mass_call_for_many_orders_and_none_when_empty() {
        for count in [0, 42] {
            let fake = Fake { count, batches: AtomicUsize::new(0) };
            sweep(&fake, "BTCUSDT", CancelSide::Both).await.unwrap();
            assert_eq!(fake.batches.load(Ordering::SeqCst), usize::from(count != 0));
        }
    }
    struct MixedFake { batches: AtomicUsize, last_count: AtomicUsize }
    impl Exchange for MixedFake {
        fn name(&self) -> &str { "mixed" }
        fn to_exchange_symbol(&self, s: &Symbol) -> String { s.binance() }
        async fn fetch_open_orders(&self, _: &str) -> Result<Vec<Order>> {
            Ok(vec![
                Order { id: "1".into(), side: Side::Buy, price: "1".into(), quantity: "1".into() },
                Order { id: "2".into(), side: Side::Sell, price: "1".into(), quantity: "1".into() },
            ])
        }
        async fn cancel_orders(&self, _: &str, orders: &[Order], side: CancelSide) -> Result<()> {
            self.batches.fetch_add(1, Ordering::SeqCst);
            self.last_count.store(orders.len(), Ordering::SeqCst);
            assert!(orders.iter().all(|order| side.matches(order.side)));
            Ok(())
        }
    }

    #[tokio::test]
    async fn filters_orders_by_configured_side_before_cancel() {
        for side in [CancelSide::Buy, CancelSide::Sell] {
            let fake = MixedFake { batches: AtomicUsize::new(0), last_count: AtomicUsize::new(0) };
            sweep(&fake, "BTCUSDT", side).await.unwrap();
            assert_eq!(fake.batches.load(Ordering::SeqCst), 1);
            assert_eq!(fake.last_count.load(Ordering::SeqCst), 1);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn subsecond_polling_and_no_backlog_after_slow_cycle() {
        let fake = Arc::new(Fake { count: 1, batches: AtomicUsize::new(0) });
        let task = tokio::spawn(run_symbol(fake.clone(), "BTCUSDT".into(), Duration::from_millis(25), CancelSide::Both));
        tokio::task::yield_now().await;
        assert_eq!(fake.batches.load(Ordering::SeqCst), 1);
        tokio::time::advance(Duration::from_millis(25)).await;
        tokio::task::yield_now().await;
        assert_eq!(fake.batches.load(Ordering::SeqCst), 2);
        tokio::time::advance(Duration::from_millis(500)).await;
        tokio::task::yield_now().await;
        assert_eq!(fake.batches.load(Ordering::SeqCst), 3);
        task.abort();
    }
}
