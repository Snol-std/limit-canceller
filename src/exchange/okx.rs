//! OKX V5: full pagination of regular orders and batch cancellation of up to 20 orders.
use std::{collections::{HashMap, HashSet}, time::Duration};
use anyhow::{bail, Context, Result};
use chrono::Utc;
use serde_json::Value;
use tracing::{info, warn};
use super::{hmac_sha256_base64, query_string, required_string, response_json, Exchange, Order, RequestGate, Side};
use crate::symbol::Symbol;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OkxMarket { Spot, Swap }
impl OkxMarket {
    pub fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "spot" => Ok(Self::Spot), "swap" => Ok(Self::Swap),
            other => bail!("okx: unknown market {other:?}"),
        }
    }
}
const BATCH_SIZE: usize = 20;
const PAGE_SIZE: usize = 100;
const MAX_PAGES: usize = 100;

pub struct Okx {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    api_secret: String,
    passphrase: String,
    market: OkxMarket,
    gate: RequestGate,
    read_gate: RequestGate,
    cancel_gate: RequestGate,
}
impl Okx {
    pub fn new(api_key: &str, api_secret: &str, passphrase: &str, market: OkxMarket) -> Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder().timeout(Duration::from_secs(15))
                .redirect(reqwest::redirect::Policy::none()).build()?,
            base_url: "https://www.okx.com".into(), api_key: api_key.into(),
            api_secret: api_secret.into(), passphrase: passphrase.into(), market,
            gate: RequestGate::new(Duration::ZERO),
            read_gate: RequestGate::new(Duration::from_millis(40)),
            // <= 20 orders / 150 ms, leaving margin against the 300 orders / 2 s limit.
            // The shared gate is more conservative than the per-instrument limit.
            cancel_gate: RequestGate::new(Duration::from_millis(150)),
        })
    }
    fn inst_type(&self) -> &'static str {
        match self.market { OkxMarket::Spot => "SPOT", OkxMarket::Swap => "SWAP" }
    }
    async fn request(&self, method: reqwest::Method, path: &str, body: &str) -> Result<Value> {
        if method == reqwest::Method::GET { self.read_gate.wait_after(&self.gate).await; }
        else { self.cancel_gate.wait_after(&self.gate).await; }
        let timestamp = Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
        let signature = hmac_sha256_base64(&self.api_secret, &format!("{timestamp}{method}{path}{body}"));
        let mut request = self.client.request(method.clone(), format!("{}{path}", self.base_url));
        if method == reqwest::Method::POST { request = request.body(body.to_owned()); }
        let response = request.header("OK-ACCESS-KEY", &self.api_key)
            .header("OK-ACCESS-SIGN", signature).header("OK-ACCESS-TIMESTAMP", timestamp)
            .header("OK-ACCESS-PASSPHRASE", &self.passphrase).header("Content-Type", "application/json")
            .send().await.map_err(|error| error.without_url()).context("request to okx")?;
        let value = response_json(response, "okx", &self.gate).await?;
        let code = required_string(&value, "code")?;
        if code == "50011" || code == "50061" { self.gate.pause(Duration::from_secs(2)).await; }
        if let Some(items) = value.get("data").and_then(Value::as_array) {
            if items.iter().any(|item| matches!(item.get("sCode").and_then(Value::as_str), Some("50011" | "50061"))) {
                self.gate.pause(Duration::from_secs(2)).await;
            }
        }
        // A batch may partially succeed: first inspect every data entry,
        // including global code=1/2 cases, then build an error summary.
        let batch_response = path == "/api/v5/trade/cancel-batch-orders" && matches!(code, "0" | "1" | "2");
        if code != "0" && !batch_response {
            bail!("okx: code={code}: {}", value.get("msg").and_then(Value::as_str).unwrap_or(""));
        }
        Ok(value)
    }
}

fn parse_orders(items: &[Value]) -> Result<Vec<Order>> {
    items.iter().map(|item| Ok(Order {
        id: required_string(item, "ordId")?.into(),
        side: Side::parse(required_string(item, "side")?).context("okx: invalid side")?,
        price: item.get("px").and_then(Value::as_str).unwrap_or("").into(),
        quantity: required_string(item, "sz")?.into(),
    })).collect()
}

fn check_batch(value: &Value, orders: &[Order]) -> Result<()> {
    let data = value.get("data").and_then(Value::as_array).context("okx: cancellation response is missing data")?;
    let mut results = HashMap::new();
    for item in data {
        let id = required_string(item, "ordId")?;
        if results.insert(id, item).is_some() { bail!("okx: duplicate result for ordId={id}"); }
    }
    let mut failures = Vec::new();
    let mut accepted = 0;
    for order in orders {
        match results.remove(order.id.as_str()) {
            Some(item) if item.get("sCode").and_then(Value::as_str) == Some("0") => accepted += 1,
            Some(item) => failures.push(format!("{}: sCode={}, {}", order.id,
                item.get("sCode").and_then(Value::as_str).unwrap_or("MISSING"),
                item.get("sMsg").and_then(Value::as_str).unwrap_or(""))),
            None => failures.push(format!("{}: no result in data", order.id)),
        }
    }
    if !results.is_empty() { failures.push("response contains unrequested IDs".into()); }
    if value.get("code").and_then(Value::as_str) != Some("0") {
        failures.push(format!("global code={}", value.get("code").unwrap_or(&Value::Null)));
    }
    if !failures.is_empty() {
        bail!("okx: accepted {accepted}/{} requests; {}", orders.len(), failures.join("; "));
    }
    Ok(())
}

impl Exchange for Okx {
    fn name(&self) -> &str { match self.market { OkxMarket::Spot => "okx-spot", OkxMarket::Swap => "okx-swap" } }
    fn to_exchange_symbol(&self, symbol: &Symbol) -> String { symbol.okx(self.market == OkxMarket::Swap) }
    async fn fetch_open_orders(&self, symbol: &str) -> Result<Vec<Order>> {
        let mut after = String::new();
        let mut seen_cursors = HashSet::new();
        let mut seen_ids = HashSet::new();
        let mut orders = Vec::new();
        for _ in 0..MAX_PAGES {
            let mut params = vec![("instType", self.inst_type().into()), ("instId", symbol.into()),
                ("limit", PAGE_SIZE.to_string())];
            if !after.is_empty() { params.push(("after", after.clone())); }
            let path = format!("/api/v5/trade/orders-pending?{}", query_string(&params));
            let value = self.request(reqwest::Method::GET, &path, "").await?;
            let data = value.get("data").and_then(Value::as_array).context("okx: data is missing")?;
            for order in parse_orders(data)? {
                if seen_ids.insert(order.id.clone()) { orders.push(order); }
            }
            if data.len() < PAGE_SIZE { return Ok(orders); }
            after = required_string(data.last().context("okx: empty page")?, "ordId")?.into();
            if !seen_cursors.insert(after.clone()) { bail!("okx: pagination loop detected"); }
        }
        warn!(symbol, "okx: 100-page limit reached; remaining orders will be handled on the next cycle");
        Ok(orders)
    }
    async fn cancel_orders(&self, symbol: &str, orders: &[Order], _side: super::CancelSide) -> Result<()> {
        let mut failures = Vec::new();
        for chunk in orders.chunks(BATCH_SIZE) {
            let body: Vec<Value> = chunk.iter().map(|order|
                serde_json::json!({"instId": symbol, "ordId": order.id})).collect();
            let result = match self.request(reqwest::Method::POST, "/api/v5/trade/cancel-batch-orders",
                &serde_json::to_string(&body)?).await {
                Ok(value) => check_batch(&value, chunk),
                Err(error) => Err(error),
            };
            match result {
                Ok(()) => info!(symbol, count = chunk.len(), "okx: cancellation batch accepted"),
                Err(error) => {
                    warn!(symbol, error = %format!("{error:#}"), "okx: cancellation batch errors");
                    failures.push(format!("{error:#}"));
                }
            }
        }
        if !failures.is_empty() { bail!("errors in {} batches: {}", failures.len(), failures.join(" | ")); }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::test_support::{client, MockServer};
    fn orders(count: usize) -> Vec<Order> {
        (1..=count).map(|i| Order { id:i.to_string(), side:Side::Buy, price:"1".into(), quantity:"2".into() }).collect()
    }
    #[test]
    fn every_nonzero_missing_or_partial_result_is_error() {
        for value in [serde_json::json!({"code":"0","data":[]}),
            serde_json::json!({"code":"0","data":[{"ordId":"1","sCode":"51400"}]}),
            serde_json::json!({"code":"0","data":[{"ordId":"1"}]}),
            serde_json::json!({"code":"2","data":[{"ordId":"1","sCode":"0"}]})] {
            assert!(check_batch(&value, &orders(1)).is_err());
        }
        assert!(check_batch(&serde_json::json!({"code":"0","data":[{"ordId":"1","sCode":"0"}]}), &orders(1)).is_ok());
    }
    #[tokio::test]
    async fn uses_batches_of_twenty_and_checks_exact_signed_body() {
        let all = orders(41);
        let responses = all.chunks(20).map(|chunk| serde_json::json!({"code":"0","data":
            chunk.iter().map(|order| serde_json::json!({"ordId":order.id,"sCode":"0"})).collect::<Vec<_>>()
        }).to_string()).collect();
        let server = MockServer::new(responses);
        let mut exchange = Okx::new("key", "secret", "pass", OkxMarket::Spot).unwrap();
        exchange.base_url = server.url.clone(); exchange.client = client();
        exchange.cancel_orders("BTC-USDT", &all, super::CancelSide::Both).await.unwrap();
        let requests = server.finish();
        let sizes: Vec<usize> = requests.iter().map(|r| serde_json::from_str::<Vec<Value>>(&r.body).unwrap().len()).collect();
        assert_eq!(sizes, vec![20,20,1]);
        for request in requests {
            assert_eq!(request.target, "/api/v5/trade/cancel-batch-orders");
            let prehash = format!("{}POST{}{}", request.headers["ok-access-timestamp"], request.target, request.body);
            assert_eq!(request.headers["ok-access-sign"], hmac_sha256_base64("secret", &prehash));
        }
    }
    #[tokio::test]
    async fn reads_next_page_with_after_cursor() {
        let data: Vec<Value> = (1..=100).map(|i| serde_json::json!({"ordId":i.to_string(),"side":"buy","px":"1","sz":"2"})).collect();
        let server = MockServer::new(vec![serde_json::json!({"code":"0","data":data}).to_string(),
            serde_json::json!({"code":"0","data":[{"ordId":"101","side":"sell","px":"1","sz":"2"}]}).to_string()]);
        let mut exchange = Okx::new("key", "secret", "pass", OkxMarket::Swap).unwrap();
        exchange.base_url = server.url.clone(); exchange.client = client();
        assert_eq!(exchange.fetch_open_orders("BTC-USDT-SWAP").await.unwrap().len(), 101);
        let requests = server.finish();
        assert!(requests[1].target.contains("after=100"));
        assert!(requests[0].target.contains("instType=SWAP"));
    }
}
