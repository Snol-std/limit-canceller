//! Bybit V5 HMAC: timestamp + api_key + recv_window + (query or JSON body).
use std::{collections::HashSet, time::Duration};
use anyhow::{bail, Context, Result};
use chrono::Utc;
use futures_util::{stream, StreamExt};
use super::transport::RequestBudget;
use serde_json::Value;
use tracing::{info, warn};
use super::{hmac_sha256_hex, query_string, required_string, response_json, Exchange, Order, RequestGate, Side};
use crate::symbol::Symbol;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BybitMarket { Spot, Linear }
impl BybitMarket {
    pub fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "spot" => Ok(Self::Spot), "linear" => Ok(Self::Linear),
            other => bail!("bybit: unknown market {other:?}"),
        }
    }
}
const RECV_WINDOW: &str = "5000";
const MAX_PAGES: usize = 100;

pub struct Bybit {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    api_secret: String,
    market: BybitMarket,
    gate: RequestGate,
    read_gate: RequestGate,
    cancel_gate: RequestBudget,
}
impl Bybit {
    pub fn new(api_key: &str, api_secret: &str, market: BybitMarket) -> Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder().timeout(Duration::from_secs(15))
                .redirect(reqwest::redirect::Policy::none()).build()?,
            base_url: "https://api.bybit.com".into(),
            api_key: api_key.to_string(), api_secret: api_secret.to_string(), market,
            gate: RequestGate::new(Duration::ZERO),
            read_gate: RequestGate::new(Duration::from_millis(25)),
            // Up to 10 individual cancellations per 1050 ms, with burst submission allowed.
            // Shared budget across all symbols, with a 50 ms safety margin for the rolling API window.
            cancel_gate: RequestBudget::new(10, Duration::from_millis(1050)),
        })
    }
    fn category(&self) -> &'static str {
        match self.market { BybitMarket::Spot => "spot", BybitMarket::Linear => "linear" }
    }
    fn signature(&self, timestamp: &str, payload: &str) -> String {
        hmac_sha256_hex(&self.api_secret, &format!("{timestamp}{}{RECV_WINDOW}{payload}", self.api_key))
    }
    async fn request(&self, method: reqwest::Method, path: &str, query: &str, body: &str) -> Result<Value> {
        if method == reqwest::Method::GET { self.read_gate.wait_after(&self.gate).await; }
        else { self.cancel_gate.wait_after(&self.gate).await; }
        let timestamp = Utc::now().timestamp_millis().to_string();
        let payload = if method == reqwest::Method::GET { query } else { body };
        let signature = self.signature(&timestamp, payload);
        let url = if query.is_empty() { format!("{}{path}", self.base_url) }
            else { format!("{}{path}?{query}", self.base_url) };
        let mut request = self.client.request(method.clone(), url);
        if method == reqwest::Method::POST { request = request.body(body.to_string()); }
        let response = request.header("X-BAPI-API-KEY", &self.api_key)
            .header("X-BAPI-SIGN", signature).header("X-BAPI-TIMESTAMP", timestamp)
            .header("X-BAPI-RECV-WINDOW", RECV_WINDOW).header("Content-Type", "application/json")
            .send().await.map_err(|error| error.without_url()).context("request to bybit")?;
        let reset_ms = response.headers().get("X-Bapi-Limit-Reset-Timestamp")
            .and_then(|s| s.to_str().ok()).and_then(|s| s.parse::<i64>().ok());
        let value = response_json(response, "bybit", &self.gate).await?;
        let code = value.get("retCode").and_then(Value::as_i64).context("bybit: retCode is missing")?;
        if code != 0 {
            if code == 10006 || code == 10018 || code == 10429 {
                let wait_ms = reset_ms.map(|t| t.saturating_sub(Utc::now().timestamp_millis()))
                    .unwrap_or(1000).clamp(1000, 600_000) as u64;
                self.gate.pause(Duration::from_millis(wait_ms + 100)).await;
            }
            bail!("bybit: retCode={code}: {}", value.get("retMsg").and_then(Value::as_str).unwrap_or(""));
        }
        Ok(value)
    }

    async fn cancel_one(&self, symbol: &str, order_id: String) -> Result<()> {
        let mut body = serde_json::json!({
            "category": self.category(), "symbol": symbol, "orderId": order_id.as_str(),
        });
        if self.market == BybitMarket::Spot { body["orderFilter"] = "Order".into(); }

        let result = self.request(reqwest::Method::POST, "/v5/order/cancel", "", &body.to_string()).await;
        let result = result.and_then(|value| {
            let result = value.get("result").context("bybit: cancellation result is missing")?;
            let id = required_string(result, "orderId")?;
            if id != order_id.as_str() { bail!("bybit: cancellation response contains a different orderId: {id}"); }
            Ok(())
        });

        match &result {
            Ok(()) => info!(symbol, order_id = %order_id, "bybit: order cancellation request accepted"),
            Err(error) => warn!(symbol, order_id = %order_id,
                error = %format!("{error:#}"), "bybit: order cancellation failed"),
        }
        result.with_context(|| format!("orderId={order_id}"))
    }
}

fn parse_orders(items: &[Value]) -> Result<Vec<Order>> {
    items.iter().map(|item| Ok(Order {
        id: required_string(item, "orderId")?.into(),
        side: Side::parse(required_string(item, "side")?).context("bybit: invalid side")?,
        price: item.get("price").and_then(Value::as_str).unwrap_or("").into(),
        quantity: required_string(item, "qty")?.into(),
    })).collect()
}

impl Exchange for Bybit {
    fn name(&self) -> &str { "bybit" }
    fn to_exchange_symbol(&self, symbol: &Symbol) -> String { symbol.bybit() }
    async fn fetch_open_orders(&self, symbol: &str) -> Result<Vec<Order>> {
        let mut cursor = String::new();
        let mut seen_cursors = HashSet::new();
        let mut seen_ids = HashSet::new();
        let mut orders = Vec::new();
        for _ in 0..MAX_PAGES {
            let mut params = vec![("category", self.category().into()), ("symbol", symbol.into()),
                ("openOnly", "0".into()), ("limit", "50".into()), ("orderFilter", "Order".into())];
            if !cursor.is_empty() { params.push(("cursor", cursor.clone())); }
            let query = query_string(&params);
            let value = self.request(reqwest::Method::GET, "/v5/order/realtime", &query, "").await?;
            let items = value.pointer("/result/list").and_then(Value::as_array)
                .context("bybit: result.list array is missing")?;
            for order in parse_orders(items)? {
                if seen_ids.insert(order.id.clone()) { orders.push(order); }
            }
            cursor = value.pointer("/result/nextPageCursor").and_then(Value::as_str).unwrap_or("").into();
            if cursor.is_empty() { return Ok(orders); }
            if !seen_cursors.insert(cursor.clone()) { bail!("bybit: pagination loop detected"); }
        }
        // Process the fetched IDs now; remaining orders will be handled on the next cycle.
        warn!(symbol, "bybit: page limit reached; remaining orders will be handled on the next cycle");
        Ok(orders)
    }
    async fn cancel_orders(&self, symbol: &str, orders: &[Order], _side: super::CancelSide) -> Result<()> {
        // Do not move &Order into the async closure. With Rust 2024/RPITIT this could
        // cause `implementation of FnOnce is not general enough` because
        // the closure future retained a reference with the lifetime from stream::iter(&[Order]).
        // Cancellation only needs the ID, so create owned copies up front.
        let mut cancels = Vec::with_capacity(orders.len());
        for order in orders {
            cancels.push(self.cancel_one(symbol, order.id.clone()));
        }
        let results: Vec<Result<()>> = stream::iter(cancels)
            // Individual requests run concurrently; awaiting one does not block the others.
            .buffer_unordered(10)
            .collect().await;
        let failures: Vec<String> = results.into_iter().filter_map(|r| r.err())
            .map(|error| format!("{error:#}")).collect();
        if !failures.is_empty() { bail!("bybit: failed to cancel {} orders: {}", failures.len(), failures.join("; ")); }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::test_support::{client, MockServer};
    #[test]
    fn fixed_get_signature_vector() {
        let exchange = Bybit::new("XXXXXXXXXX", "secret", BybitMarket::Spot).unwrap();
        assert_eq!(exchange.signature("1658384314791", "category=option&symbol=BTC-29JUL22-25000-C"),
            "02e9182e346177050f199ce1e0703d738589e3763805ed71590ced65539a73a7");
    }
    #[tokio::test]
    async fn pagination_encoding_and_signed_single_request() {
        let order = serde_json::json!({"orderId":"1","side":"Buy","price":"1","qty":"2"});
        let server = MockServer::new(vec![
            serde_json::json!({"retCode":0,"result":{"list":[order],"nextPageCursor":"a+b/c=="}}).to_string(),
            serde_json::json!({"retCode":0,"result":{"list":[],"nextPageCursor":""}}).to_string(),
            serde_json::json!({"retCode":0,"result":{"orderId":"1"}}).to_string(),
        ]);
        let mut exchange = Bybit::new("key", "secret", BybitMarket::Linear).unwrap();
        exchange.base_url = server.url.clone(); exchange.client = client();
        let orders = exchange.fetch_open_orders("BTCUSDT").await.unwrap();
        exchange.cancel_orders("BTCUSDT", &orders, super::CancelSide::Both).await.unwrap();
        let requests = server.finish();
        assert!(requests[1].target.contains("cursor=a%2Bb%2Fc%3D%3D"));
        assert_eq!(requests[2].target, "/v5/order/cancel");
        assert_eq!(requests[2].method, "POST");
        let body: Value = serde_json::from_str(&requests[2].body).unwrap();
        assert_eq!(body["symbol"], "BTCUSDT"); assert_eq!(body["orderId"], "1");
        assert!(body.get("orderFilter").is_none());
        for request in requests {
            assert_eq!(request.headers["x-bapi-recv-window"], "5000");
            assert!(!request.headers.contains_key("x-bapi-recv-timestamp"));
            let payload = if request.method == "GET" { request.target.split_once('?').unwrap().1 } else { &request.body };
            let expected = hmac_sha256_hex("secret", &format!("{}key5000{payload}", request.headers["x-bapi-timestamp"]));
            assert_eq!(request.headers["x-bapi-sign"], expected);
        }
    }
    #[tokio::test]
    async fn single_failure_does_not_skip_next_order() {
        let server = MockServer::new(vec![
            r#"{"retCode":110001,"retMsg":"Order does not exist","result":{}}"#.into(),
            r#"{"retCode":0,"result":{"orderId":"2"}}"#.into(),
        ]);
        let mut exchange = Bybit::new("key", "secret", BybitMarket::Spot).unwrap();
        exchange.base_url = server.url.clone(); exchange.client = client();
        let orders = parse_orders(&[
            serde_json::json!({"orderId":"1","side":"Buy","price":"1","qty":"2"}),
            serde_json::json!({"orderId":"2","side":"Sell","price":"1","qty":"2"}),
        ]).unwrap();
        assert!(exchange.cancel_orders("BTCUSDT", &orders, super::CancelSide::Both).await.is_err());
        let requests = server.finish();
        assert_eq!(requests.len(), 2);
        let mut ids = HashSet::new();
        for request in &requests {
            assert_eq!(request.target, "/v5/order/cancel");
            let body: Value = serde_json::from_str(&request.body).unwrap();
            ids.insert(body["orderId"].as_str().unwrap().to_string());
            assert_eq!(body["orderFilter"], "Order");
        }
        assert_eq!(ids, HashSet::from(["1".to_string(), "2".to_string()]));
    }
    #[tokio::test(start_paused = true)]
    async fn both_markets_allow_bursts_with_shared_window_budget() {
        for market in [BybitMarket::Spot, BybitMarket::Linear] {
            let exchange = Bybit::new("key", "secret", market).unwrap();
            let start = tokio::time::Instant::now();
            for _ in 0..10 { exchange.cancel_gate.wait_after(&exchange.gate).await; }
            assert_eq!(start.elapsed(), Duration::ZERO);
            exchange.cancel_gate.wait_after(&exchange.gate).await;
            assert_eq!(start.elapsed(), Duration::from_millis(1050));
        }
    }
    #[tokio::test]
    async fn submits_both_cancels_before_waiting_for_first_response() {
        // The mock holds the first response until both requests have been received.
        // A sequential implementation times out here.
        let server = MockServer::parallel_cancels(2);
        let mut exchange = Bybit::new("key", "secret", BybitMarket::Linear).unwrap();
        exchange.base_url = server.url.clone(); exchange.client = client();
        let orders = parse_orders(&[
            serde_json::json!({"orderId":"1","side":"Buy","price":"1030","qty":"2"}),
            serde_json::json!({"orderId":"2","side":"Sell","price":"1050","qty":"2"}),
        ]).unwrap();
        exchange.cancel_orders("BTCUSDT", &orders, super::CancelSide::Both).await.unwrap();
        let requests = server.finish();
        let ids: HashSet<String> = requests.iter().map(|r| {
            assert_eq!(r.target, "/v5/order/cancel");
            serde_json::from_str::<Value>(&r.body).unwrap()["orderId"].as_str().unwrap().to_string()
        }).collect();
        assert_eq!(ids, HashSet::from(["1".to_string(), "2".to_string()]));
    }
}
