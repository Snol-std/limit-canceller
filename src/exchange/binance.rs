//! Binance Spot / USDⓈ-M: cancel-all for both, individual orderId cancellation for buy/sell.
use std::{
    sync::atomic::{AtomicI64, Ordering},
    time::Duration,
};

use anyhow::{bail, Context, Result};
use chrono::Utc;
use futures_util::{stream, StreamExt};
use serde_json::Value;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use super::{
    hmac_sha256_hex, query_string, required_string, response_json, CancelSide, Exchange, Order,
    RequestGate, Side,
};
use crate::symbol::Symbol;

const RECV_WINDOW_MS: i64 = 5_000;
const TIME_SYNC_TTL_MS: i64 = 60_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinanceMarket {
    Spot,
    Futures,
}
impl BinanceMarket {
    pub fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "spot" => Ok(Self::Spot),
            "futures" => Ok(Self::Futures),
            other => bail!("binance: unknown market {other:?}"),
        }
    }
}

pub struct Binance {
    client: reqwest::Client,
    base_url: String,
    open_orders_path: &'static str,
    cancel_all_path: &'static str,
    cancel_order_path: &'static str,
    server_time_path: &'static str,
    market: BinanceMarket,
    api_key: String,
    api_secret: String,
    gate: RequestGate,
    /// serverTime - midpoint(local request start/end), in milliseconds.
    time_offset_ms: AtomicI64,
    /// Local UTC time of the last successful synchronization.
    last_time_sync_ms: AtomicI64,
    /// Prevents multiple symbol workers from synchronizing time concurrently.
    time_sync_lock: Mutex<()>,
}

impl Binance {
    pub fn new(api_key: &str, api_secret: &str, market: BinanceMarket) -> Result<Self> {
        let (base_url, open_orders_path, cancel_all_path, cancel_order_path, server_time_path) = match market {
            BinanceMarket::Spot => (
                "https://api.binance.com",
                "/api/v3/openOrders",
                "/api/v3/openOrders",
                "/api/v3/order",
                "/api/v3/time",
            ),
            BinanceMarket::Futures => (
                "https://fapi.binance.com",
                "/fapi/v1/openOrders",
                "/fapi/v1/allOpenOrders",
                "/fapi/v1/order",
                "/fapi/v1/time",
            ),
        };
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            base_url: base_url.into(),
            open_orders_path,
            cancel_all_path,
            cancel_order_path,
            server_time_path,
            market,
            api_key: api_key.to_string(),
            api_secret: api_secret.to_string(),
            gate: RequestGate::new(Duration::ZERO),
            time_offset_ms: AtomicI64::new(0),
            last_time_sync_ms: AtomicI64::new(0),
            time_sync_lock: Mutex::new(()),
        })
    }

    fn signed_query_at(&self, mut params: Vec<(&str, String)>, timestamp: i64) -> String {
        params.push(("recvWindow", RECV_WINDOW_MS.to_string()));
        params.push(("timestamp", timestamp.to_string()));
        params.sort_by(|a, b| a.0.cmp(b.0));
        let query = query_string(&params);
        let signature = hmac_sha256_hex(&self.api_secret, &query);
        format!("{query}&signature={signature}")
    }

    fn timestamp_ms(&self) -> i64 {
        Utc::now().timestamp_millis() + self.time_offset_ms.load(Ordering::Relaxed)
    }

    fn time_sync_is_fresh(&self, now_ms: i64) -> bool {
        let last = self.last_time_sync_ms.load(Ordering::Acquire);
        last > 0 && now_ms >= last && now_ms - last < TIME_SYNC_TTL_MS
    }

    async fn sync_server_time(&self, force: bool) -> Result<()> {
        let _guard = self.time_sync_lock.lock().await;

        let now_ms = Utc::now().timestamp_millis();
        let first_sync = self.last_time_sync_ms.load(Ordering::Acquire) == 0;
        if !force && self.time_sync_is_fresh(now_ms) {
            return Ok(());
        }

        let started_ms = Utc::now().timestamp_millis();
        let response = self
            .client
            .get(format!("{}{}", self.base_url, self.server_time_path))
            .send()
            .await
            .map_err(|error| error.without_url())
            .context("binance: server time request")?;
        let status = response.status();
        let text = response
            .text()
            .await
            .context("binance: reading server time response")?;
        if !status.is_success() {
            bail!(
                "binance: server time HTTP {status}: {}",
                super::text_snippet(&text)
            );
        }
        let value: Value = serde_json::from_str(&text)
            .with_context(|| format!("binance: invalid server time response: {}", super::text_snippet(&text)))?;
        let server_time_ms = value
            .get("serverTime")
            .and_then(Value::as_i64)
            .context("binance: server time response is missing serverTime")?;
        let finished_ms = Utc::now().timestamp_millis();

        // Using the midpoint reduces systematic error by approximately half the RTT.
        let midpoint_ms = started_ms + (finished_ms - started_ms) / 2;
        let offset_ms = server_time_ms - midpoint_ms;
        self.time_offset_ms.store(offset_ms, Ordering::Release);
        self.last_time_sync_ms.store(finished_ms, Ordering::Release);

        if first_sync {
            info!(
                exchange = "binance",
                offset_ms,
                rtt_ms = finished_ms - started_ms,
                "Binance time synchronized"
            );
        } else {
            debug!(
                exchange = "binance",
                offset_ms,
                rtt_ms = finished_ms - started_ms,
                "Binance time offset updated"
            );
        }
        Ok(())
    }

    async fn ensure_server_time(&self) -> Result<()> {
        if self.time_sync_is_fresh(Utc::now().timestamp_millis()) {
            return Ok(());
        }
        self.sync_server_time(false).await
    }

    fn is_timestamp_error(error: &anyhow::Error) -> bool {
        format!("{error:#}").contains("-1021")
    }

    async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        params: Vec<(&str, String)>,
    ) -> Result<Value> {
        // The signing timestamp must be calculated after waiting for the request gate.
        self.gate.wait().await;
        self.ensure_server_time()
            .await
            .context("binance: time synchronization")?;

        let mut timestamp_retry_used = false;
        loop {
            let query = self.signed_query_at(params.clone(), self.timestamp_ms());
            let response = self
                .client
                .request(method.clone(), format!("{}{}?{}", self.base_url, path, query))
                .header("X-MBX-APIKEY", &self.api_key)
                .send()
                .await
                .map_err(|error| error.without_url())
                .context("request to binance")?;

            let value = match response_json(response, "binance", &self.gate).await {
                Ok(value) => value,
                Err(error) if !timestamp_retry_used && Self::is_timestamp_error(&error) => {
                    // The local clock may have changed abruptly after the previous synchronization.
                    warn!(exchange = "binance", "received -1021; resynchronizing time");
                    self.sync_server_time(true)
                        .await
                        .context("binance: time resynchronization after -1021")?;
                    timestamp_retry_used = true;
                    continue;
                }
                Err(error) => return Err(error),
            };

            if let Some(code) = value.get("code").and_then(Value::as_i64) {
                if code < 0 {
                    if code == -1021 && !timestamp_retry_used {
                        warn!(exchange = "binance", "received -1021; resynchronizing time");
                        self.sync_server_time(true)
                            .await
                            .context("binance: time resynchronization after -1021")?;
                        timestamp_retry_used = true;
                        continue;
                    }
                    if code == -1003 || code == -1015 {
                        self.gate.pause(Duration::from_secs(2)).await;
                    }
                    bail!(
                        "binance: code={code}: {}",
                        value.get("msg").and_then(Value::as_str).unwrap_or("")
                    );
                }
            }
            return Ok(value);
        }
    }

    async fn cancel_one(&self, symbol: &str, order_id: String) -> Result<()> {
        let body = self
            .request(
                reqwest::Method::DELETE,
                self.cancel_order_path,
                vec![("symbol", symbol.to_string()), ("orderId", order_id.clone())],
            )
            .await?;

        let returned_id = body
            .get("orderId")
            .and_then(|value| {
                value
                    .as_u64()
                    .map(|id| id.to_string())
                    .or_else(|| value.as_str().map(str::to_owned))
            })
            .context("binance: single-cancel response is missing orderId")?;
        if returned_id != order_id {
            bail!("binance: expected orderId={order_id}, received orderId={returned_id}");
        }
        Ok(())
    }
}

fn parse_orders(body: &Value) -> Result<Vec<Order>> {
    body.as_array()
        .context("binance: expected an array of orders")?
        .iter()
        .map(|item| {
            let id = item
                .get("orderId")
                .and_then(Value::as_u64)
                .map(|v| v.to_string())
                .or_else(|| {
                    item.get("orderId")
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                })
                .context("binance: valid orderId is missing")?;
            Ok(Order {
                id,
                side: Side::parse(required_string(item, "side")?)
                    .context("binance: invalid side")?,
                price: required_string(item, "price")?.into(),
                quantity: required_string(item, "origQty")?.into(),
            })
        })
        .collect()
}

impl Exchange for Binance {
    fn name(&self) -> &str {
        match self.market {
            BinanceMarket::Spot => "binance-spot",
            BinanceMarket::Futures => "binance-futures",
        }
    }

    fn to_exchange_symbol(&self, symbol: &Symbol) -> String {
        symbol.binance()
    }

    async fn fetch_open_orders(&self, symbol: &str) -> Result<Vec<Order>> {
        let body = self
            .request(
                reqwest::Method::GET,
                self.open_orders_path,
                vec![("symbol", symbol.to_string())],
            )
            .await?;
        parse_orders(&body)
    }

    async fn cancel_orders(&self, symbol: &str, orders: &[Order], side: CancelSide) -> Result<()> {
        if orders.is_empty() {
            return Ok(());
        }

        if side == CancelSide::Both {
            // Keep the fast bulk endpoint for both. It cancels all regular
            // active orders for the symbol, including orders created after the snapshot was fetched.
            let body = self
                .request(
                    reqwest::Method::DELETE,
                    self.cancel_all_path,
                    vec![("symbol", symbol.to_string())],
                )
                .await?;
            match self.market {
                BinanceMarket::Spot => {
                    body.as_array()
                        .context("binance: invalid cancel-all response")?;
                }
                BinanceMarket::Futures => {
                    if body.get("code").and_then(Value::as_i64) != Some(200) {
                        bail!("binance futures: cancel-all did not confirm request acceptance: {body}");
                    }
                }
            }
            return Ok(());
        }

        // Binance cancel-all cannot filter BUY/SELL. For a single side,
        // cancel only the filtered orderIds. An owned String does not retain &Order
        // inside the async future and therefore does not reproduce the Bybit lifetime issue.
        if orders.iter().any(|order| !side.matches(order.side)) {
            bail!("binance: internal error: side={} cancellation received an order from the opposite side", side.as_str());
        }
        let mut cancels = Vec::with_capacity(orders.len());
        for order in orders {
            cancels.push(self.cancel_one(symbol, order.id.clone()));
        }
        let results: Vec<Result<()>> = stream::iter(cancels)
            .buffer_unordered(10)
            .collect()
            .await;
        let failures: Vec<String> = results
            .into_iter()
            .filter_map(|result| result.err())
            .map(|error| format!("{error:#}"))
            .collect();
        if !failures.is_empty() {
            bail!("binance: failed to cancel {} orders: {}", failures.len(), failures.join("; "));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::test_support::MockServer;

    #[test]
    fn reads_orig_qty_and_rejects_missing_id() {
        let body = serde_json::json!([{
            "orderId": 7,
            "side": "BUY",
            "price": "1.00",
            "origQty": "0.123456789"
        }]);
        assert_eq!(parse_orders(&body).unwrap()[0].quantity, "0.123456789");
        assert!(
            parse_orders(&serde_json::json!([{
                "side": "BUY",
                "price": "1",
                "origQty": "1"
            }]))
            .is_err()
        );
    }

    #[tokio::test]
    async fn both_markets_sync_time_and_use_symbol_scoped_mass_endpoint() {
        for (market, time_path, cancel_path, cancel_response) in [
            (
                BinanceMarket::Spot,
                "/api/v3/time",
                "/api/v3/openOrders",
                "[]",
            ),
            (
                BinanceMarket::Futures,
                "/fapi/v1/time",
                "/fapi/v1/allOpenOrders",
                "{\"code\":200,\"msg\":\"ok\"}",
            ),
        ] {
            let server = MockServer::new(vec![
                format!("{{\"serverTime\":{}}}", Utc::now().timestamp_millis()),
                cancel_response.to_string(),
            ]);
            let mut exchange = Binance::new("key", "secret", market).unwrap();
            exchange.base_url = server.url.clone();
            exchange.client = super::super::test_support::client();

            let orders = vec![Order {
                id: "7".into(),
                side: Side::Buy,
                price: "1".into(),
                quantity: "2".into(),
            }];
            exchange.cancel_orders("BTCUSDT", &orders, CancelSide::Both).await.unwrap();

            let requests = server.finish();
            assert_eq!(requests.len(), 2);
            assert_eq!(requests[0].method, "GET");
            assert_eq!(requests[0].target, time_path);

            assert!(requests[1].target.starts_with(&format!("{cancel_path}?")));
            assert_eq!(requests[1].method, "DELETE");
            assert!(requests[1].target.contains("symbol=BTCUSDT"));
            assert!(!requests[1].target.contains("orderId"));
            let query = requests[1].target.split_once('?').unwrap().1;
            let (unsigned, signature) = query.rsplit_once("&signature=").unwrap();
            assert_eq!(signature, hmac_sha256_hex("secret", unsigned));
        }
    }

    #[tokio::test]
    async fn one_side_uses_individual_order_ids_and_never_cancel_all() {
        for (market, time_path, order_path) in [
            (BinanceMarket::Spot, "/api/v3/time", "/api/v3/order"),
            (BinanceMarket::Futures, "/fapi/v1/time", "/fapi/v1/order"),
        ] {
            let server = MockServer::new(vec![
                format!("{{\"serverTime\":{}}}", Utc::now().timestamp_millis()),
                r#"{"orderId":7,"status":"CANCELED"}"#.to_string(),
            ]);
            let mut exchange = Binance::new("key", "secret", market).unwrap();
            exchange.base_url = server.url.clone();
            exchange.client = super::super::test_support::client();

            let orders = vec![Order {
                id: "7".into(),
                side: Side::Buy,
                price: "1".into(),
                quantity: "2".into(),
            }];
            exchange.cancel_orders("BTCUSDT", &orders, CancelSide::Buy).await.unwrap();

            let requests = server.finish();
            assert_eq!(requests.len(), 2);
            assert_eq!(requests[0].target, time_path);
            assert_eq!(requests[1].method, "DELETE");
            assert!(requests[1].target.starts_with(&format!("{order_path}?")));
            assert!(requests[1].target.contains("symbol=BTCUSDT"));
            assert!(requests[1].target.contains("orderId=7"));
            assert!(!requests[1].target.contains("openOrders"));
        }
    }
}
