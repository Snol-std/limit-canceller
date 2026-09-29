//! TigerX (x-api.tiger.trade) signed REST client for fast open-order cancellation.
//!
//! TigerX exposes one aggregated account containing Binance and OKX orders.
//! Therefore TigerX is intentionally handled as ONE account-wide polling loop,
//! not as one polling worker per ticker. A single `/trading/orders` request can
//! return all current open orders; configured ticker rules are applied locally.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use anyhow::{bail, Context, Result};
use chrono::Utc;
use serde_json::Value;
use tokio::{task::JoinSet, time::Instant};
use tracing::{debug, info, warn};

use super::{
    hmac_sha256_hex, query_string, response_json, CancelSide, RequestGate, Side,
};
use crate::symbol::Symbol;

const MAX_PAGES: usize = 100;
const PAGE_SIZE: i64 = 1000;
const REST_TIMEOUT: Duration = Duration::from_secs(5);
const READ_SPACING: Duration = Duration::ZERO;
const CANCEL_RETRY_AFTER_SUCCESS: Duration = Duration::from_millis(750);
const CANCEL_RETRY_AFTER_ERROR: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TigerXExchange {
    Binance,
    Okx,
}

impl TigerXExchange {
    pub fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "binance" => Ok(Self::Binance),
            "okx" => Ok(Self::Okx),
            other => bail!("tigerx: unknown exchange {other:?}"),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Binance => "BINANCE",
            Self::Okx => "OKX",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TigerXMarket {
    Spot,
    Perp,
}

impl TigerXMarket {
    pub fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "spot" => Ok(Self::Spot),
            "perp" => Ok(Self::Perp),
            other => bail!("tigerx: unknown market {other:?}"),
        }
    }
}

/// One configured TigerX cancellation rule, already converted to TigerX `sym`.
#[derive(Debug, Clone)]
pub struct TigerXRule {
    symbol: String,
    cancel_side: CancelSide,
}

impl TigerXRule {
    pub fn new(
        exchange: TigerXExchange,
        market: TigerXMarket,
        symbol: &Symbol,
        cancel_side: CancelSide,
    ) -> Self {
        Self {
            symbol: symbol.tigerx(exchange.as_str(), market == TigerXMarket::Perp),
            cancel_side,
        }
    }
}

/// Request gates shared by the whole TigerX API key.
///
/// v0.4.1 intentionally does NOT impose a local 10-per-10s cancel budget.
/// The supplied OpenAPI document advertises that limit for DELETE
/// `/trading/order`, but the TigerX terminal can cancel large same-symbol grids
/// much faster. The old local budget was therefore an artificial bottleneck:
/// if 7 slots had already been used, only 3 more orders were dispatched
/// immediately and the rest waited for the rolling window.
///
/// We now dispatch every matching order immediately and only slow down when
/// TigerX itself reports a real HTTP rate limit. This keeps cancellation
/// ticker-scoped because each request still targets one exact `orderId`.
struct TigerXShared {
    cooldown: RequestGate,
    read_gate: RequestGate,
}

impl TigerXShared {
    fn new() -> Self {
        Self {
            cooldown: RequestGate::new(Duration::ZERO),
            // `/trading/orders` has no rate limit in the supplied OpenAPI file.
            // The configured poll interval is the only normal pacing.
            read_gate: RequestGate::new(READ_SPACING),
        }
    }
}

#[derive(Debug, Clone)]
struct TigerXOpenOrder {
    id: String,
    symbol: String,
    side: Side,
    price: String,
    quantity: String,
}

#[derive(Debug)]
struct CancelCompletion {
    id: String,
    symbol: String,
    result: Result<()>,
}

#[derive(Debug, Clone)]
struct PendingCancel {
    active: bool,
    retry_at: Instant,
}

/// One TigerX client per API key. It intentionally has no exchange/market
/// field: `/trading/orders` can return Binance + OKX and SPOT + PERP together.
pub struct TigerX {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    api_secret: String,
    shared: Arc<TigerXShared>,
}

impl TigerX {
    pub fn new(api_key: &str, api_secret: &str) -> Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(REST_TIMEOUT)
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            base_url: "https://x-api.tiger.trade".into(),
            api_key: api_key.to_string(),
            api_secret: api_secret.to_string(),
            shared: Arc::new(TigerXShared::new()),
        })
    }

    /// TigerX signature from the official sample:
    /// sorted raw parameters joined as `k=v&...`, followed by `&nonce`, then
    /// lowercase HMAC-SHA256 with the API secret.
    fn signature(&self, params: &[(String, String)], nonce: &str) -> String {
        let mut sorted: Vec<&(String, String)> = params.iter().collect();
        sorted.sort_by(|a, b| a.0.cmp(&b.0));
        let payload = sorted
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join("&");
        let payload = if payload.is_empty() {
            nonce.to_string()
        } else {
            format!("{payload}&{nonce}")
        };
        hmac_sha256_hex(&self.api_secret, &payload)
    }

    async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        params: &[(String, String)],
        json_body: Option<&str>,
    ) -> Result<Value> {
        // Generate nonce/signature only AFTER any server-imposed cooldown.
        // GET requests are paced only by the configured polling interval.
        // DELETE requests have no local quota in v0.4.1: all matching order IDs
        // are allowed to leave concurrently unless TigerX itself returns 429/418.
        if method == reqwest::Method::GET {
            self.shared
                .read_gate
                .wait_after(&self.shared.cooldown)
                .await;
        } else {
            self.shared.cooldown.wait().await;
        }

        let nonce = Utc::now().timestamp().to_string();
        let signature = self.signature(params, &nonce);
        let url = if method == reqwest::Method::GET && !params.is_empty() {
            let borrowed: Vec<(&str, String)> = params
                .iter()
                .map(|(key, value)| (key.as_str(), value.clone()))
                .collect();
            format!("{}{}?{}", self.base_url, path, query_string(&borrowed))
        } else {
            format!("{}{path}", self.base_url)
        };

        let mut request = self
            .client
            .request(method, url)
            .header("nonce", &nonce)
            .header("signature", &signature)
            .header("X-MBX-APIKEY", &self.api_key)
            .header("Content-Type", "application/json");

        if let Some(body) = json_body {
            request = request.body(body.to_string());
        }

        let response = request
            .send()
            .await
            .map_err(|error| error.without_url())
            .context("request to tigerx")?;
        let value = response_json(response, "tigerx", &self.shared.cooldown).await?;

        let code = response_code(&value).context("tigerx: code is missing or invalid")?;
        if code != 200000 {
            let message = value
                .get("message")
                .or_else(|| value.get("msg"))
                .and_then(Value::as_str)
                .unwrap_or("");
            bail!("tigerx: code={code}: {message}");
        }

        Ok(value)
    }

    /// Fetch ALL open orders for this TigerX portfolio in one account-wide
    /// request per page. `sym`, `exchange`, and `businessType` are optional in
    /// the supplied OpenAPI document, so filtering is done locally afterwards.
    async fn fetch_open_orders(&self) -> Result<Vec<TigerXOpenOrder>> {
        let mut orders = Vec::new();
        let mut seen_ids = HashSet::new();

        for page in 1..=MAX_PAGES {
            let params = vec![
                ("page".to_string(), page.to_string()),
                ("pageSize".to_string(), PAGE_SIZE.to_string()),
            ];
            let value = self
                .request(
                    reqwest::Method::GET,
                    "/api/v1/trading/orders",
                    &params,
                    None,
                )
                .await?;
            let items = value
                .pointer("/data/list")
                .and_then(Value::as_array)
                .context("tigerx: data.list array is missing")?;

            for item in items {
                match parse_open_order(item) {
                    Ok(Some(order)) => {
                        if seen_ids.insert(order.id.clone()) {
                            orders.push(order);
                        }
                    }
                    Ok(None) => {}
                    Err(error) => {
                        // One malformed/stale record must never prevent all
                        // other valid limit orders from being cancelled.
                        warn!(
                            error = %format!("{error:#}"),
                            item = %short_json(item),
                            "tigerx: skipping malformed open-order record"
                        );
                    }
                }
            }

            let total = value
                .pointer("/data/totalSize")
                .and_then(value_i64)
                .unwrap_or(items.len() as i64);
            if (page as i64) * PAGE_SIZE >= total || items.len() < PAGE_SIZE as usize {
                return Ok(orders);
            }
        }

        warn!(
            max_pages = MAX_PAGES,
            "tigerx: open-order page limit reached; remaining orders will be picked up on the next sweep"
        );
        Ok(orders)
    }

    async fn cancel_one(&self, symbol: &str, order_id: &str) -> Result<()> {
        // The official Java example signs `orderId` and sends the same value in
        // a JSON body for DELETE. Keep that exact wire format.
        let params = vec![("orderId".to_string(), order_id.to_string())];
        let body = serde_json::json!({ "orderId": order_id }).to_string();
        let value = self
            .request(
                reqwest::Method::DELETE,
                "/api/v1/trading/order",
                &params,
                Some(&body),
            )
            .await?;

        // Cancel is asynchronous. According to the supplied OpenAPI document,
        // code=200000 only means the request was accepted; the final order
        // state is confirmed later. Do NOT turn a missing `data.orderId` into a
        // false failure/backoff. If TigerX does echo an ID, only log a mismatch.
        if let Some(returned_id) = value
            .pointer("/data/orderId")
            .and_then(value_string_ref)
            .filter(|id| !id.is_empty())
        {
            if returned_id != order_id {
                warn!(
                    symbol = %symbol,
                    order_id = %order_id,
                    returned_order_id = %returned_id,
                    "tigerx: cancellation accepted but response echoed a different orderId"
                );
            }
        }

        info!(symbol = %symbol, order_id = %order_id, "tigerx: cancellation request accepted");
        Ok(())
    }
}

/// Runs one low-latency TigerX account loop for every configured TigerX ticker.
///
/// Detection is account-wide: the number of configured Binance/OKX tickers no
/// longer multiplies REST latency. Every matching cancellation is dispatched as
/// its own concurrent DELETE immediately; there is no client-side 10-per-10s
/// queue anymore. This is intentionally ticker-safe: unlike `/trading/cancelAll`,
/// no unrelated TigerX symbols are touched.
pub async fn run(exchange: TigerX, rules: Vec<TigerXRule>, interval: Duration) -> Result<()> {
    if rules.is_empty() {
        bail!("tigerx: no ticker rules configured");
    }

    let mut rule_map = HashMap::with_capacity(rules.len());
    for rule in rules {
        rule_map.insert(rule.symbol, rule.cancel_side);
    }

    let rule_list = rule_map
        .iter()
        .map(|(symbol, side)| format!("{symbol}:{}", side.as_str()))
        .collect::<Vec<_>>()
        .join(", ");
    info!(
        exchange = "tigerx",
        interval_ms = interval.as_millis() as u64,
        count = rule_map.len(),
        symbols = %rule_list,
        "order cancellation started (account-wide TigerX polling)"
    );

    let exchange = Arc::new(exchange);
    // JoinSet owns all cancellation tasks. Dropping/aborting the TigerX runner
    // also aborts queued cancellation tasks, so Stop really stops everything.
    let mut cancel_tasks = JoinSet::<CancelCompletion>::new();
    let mut pending: HashMap<(String, String), PendingCancel> = HashMap::new();
    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut read_failures = 0u32;

    loop {
        ticker.tick().await;

        // Drain completed cancellation requests without ever blocking polling.
        while let Some(joined) = cancel_tasks.try_join_next() {
            match joined {
                Ok(completion) => {
                    let completed_at = Instant::now();
                    let failed = completion.result.is_err();
                    if let Some(state) = pending
                        .get_mut(&(completion.symbol.clone(), completion.id.clone()))
                    {
                        state.active = false;
                        state.retry_at = completed_at
                            + if failed {
                                CANCEL_RETRY_AFTER_ERROR
                            } else {
                                CANCEL_RETRY_AFTER_SUCCESS
                            };
                    }
                    if let Err(error) = completion.result {
                        warn!(
                            symbol = %completion.symbol,
                            order_id = %completion.id,
                            error = %format!("{error:#}"),
                            "tigerx: cancellation request failed; retry will be fast if the order remains open"
                        );
                    }
                }
                Err(error) => warn!(
                    error = %error,
                    "tigerx: cancellation task failed unexpectedly"
                ),
            }
        }

        let open_orders = match exchange.fetch_open_orders().await {
            Ok(orders) => {
                read_failures = 0;
                orders
            }
            Err(error) => {
                read_failures = read_failures.saturating_add(1);
                let delay = Duration::from_millis(
                    (500u64 << read_failures.min(6)).min(30_000),
                );
                warn!(
                    exchange = "tigerx",
                    error = %format!("{error:#}"),
                    retry_ms = delay.as_millis() as u64,
                    "tigerx: open-order sweep failed"
                );
                tokio::time::sleep(delay).await;
                ticker.reset_after(interval);
                continue;
            }
        };

        let open_keys: HashSet<(String, String)> = open_orders
            .iter()
            .map(|order| (order.symbol.clone(), order.id.clone()))
            .collect();
        pending.retain(|key, _| open_keys.contains(key));

        let now = Instant::now();
        for order in open_orders {
            let Some(cancel_side) = rule_map.get(&order.symbol).copied() else {
                continue;
            };
            if !cancel_side.matches(order.side) {
                continue;
            }

            let pending_key = (order.symbol.clone(), order.id.clone());
            if let Some(state) = pending.get(&pending_key) {
                if state.active || now < state.retry_at {
                    continue;
                }
            }

            debug!(
                exchange = "tigerx",
                symbol = %order.symbol,
                order_id = %order.id,
                side = order.side.as_str(),
                price = %order.price,
                quantity = %order.quantity,
                "open limit order found"
            );

            pending.insert(
                pending_key,
                PendingCancel {
                    active: true,
                    retry_at: now,
                },
            );

            let client = exchange.clone();
            cancel_tasks.spawn(async move {
                let id = order.id;
                let symbol = order.symbol;
                let result = client.cancel_one(&symbol, &id).await;
                CancelCompletion { id, symbol, result }
            });
        }
    }
}

fn response_code(value: &Value) -> Option<i64> {
    value
        .get("code")
        .and_then(|code| code.as_i64().or_else(|| code.as_str()?.parse().ok()))
}

fn value_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|v| i64::try_from(v).ok()))
        .or_else(|| value.as_str()?.parse().ok())
}

fn value_string_ref(value: &Value) -> Option<&str> {
    value.as_str()
}

fn required_text(value: &Value, key: &str) -> Result<String> {
    let field = value
        .get(key)
        .with_context(|| format!("tigerx: order record is missing {key}"))?;
    match field {
        Value::String(text) if !text.is_empty() => Ok(text.clone()),
        Value::Number(number) => Ok(number.to_string()),
        _ => bail!("tigerx: order record has invalid {key}"),
    }
}

fn optional_text(value: &Value, key: &str) -> Option<String> {
    match value.get(key)? {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn parse_open_order(item: &Value) -> Result<Option<TigerXOpenOrder>> {
    // `/trading/orders` is documented as an open-order endpoint, but filtering
    // explicit terminal states makes the client safe if a stale record appears.
    if let Some(state) = optional_text(item, "orderState") {
        let state = state.to_ascii_uppercase();
        if !matches!(state.as_str(), "NEW" | "OPEN" | "PARTIALLY_FILLED") {
            return Ok(None);
        }
    }

    // This application cancels limit orders only. Older/mock responses may not
    // contain exchangeOrderType, so absence keeps the record eligible.
    if let Some(order_type) = optional_text(item, "exchangeOrderType") {
        if !order_type.eq_ignore_ascii_case("LIMIT") {
            return Ok(None);
        }
    }

    let id = required_text(item, "orderId")?;
    let symbol = required_text(item, "sym")?.trim().to_ascii_uppercase();
    let side_text = required_text(item, "side")?;
    let side = Side::parse(&side_text).context("tigerx: invalid order side")?;
    let price = optional_text(item, "limitPrice").unwrap_or_default();
    // The API docs explicitly allow spot market BUY orders without orderQty.
    // Cancellation itself does not need quantity, so never fail a whole page
    // because this field is absent/empty. quoteOrderQty is only informational.
    let quantity = optional_text(item, "orderQty")
        .filter(|value| !value.is_empty())
        .or_else(|| optional_text(item, "quoteOrderQty"))
        .unwrap_or_default();

    Ok(Some(TigerXOpenOrder {
        id,
        symbol,
        side,
        price,
        quantity,
    }))
}

fn short_json(value: &Value) -> String {
    let text = value.to_string();
    text.chars().take(240).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::test_support::{client, MockServer};

    #[test]
    fn fixed_signature_vector() {
        let exchange = TigerX::new("key", "secret").unwrap();
        let params = vec![("orderId".to_string(), "1720609349457000".to_string())];
        assert_eq!(
            exchange.signature(&params, "1750000000"),
            "0ef7a92152c83058c094457e53f9057d95f3aaae1088290d385e5325607ed1b3"
        );

        let params = vec![
            ("pageSize".to_string(), "1000".to_string()),
            ("page".to_string(), "1".to_string()),
        ];
        let expected = hmac_sha256_hex("secret", "page=1&pageSize=1000&1750000000");
        assert_eq!(exchange.signature(&params, "1750000000"), expected);
    }

    #[test]
    fn rule_builds_binance_and_okx_symbols() {
        let btc = Symbol::parse("BTC").unwrap();
        assert_eq!(
            TigerXRule::new(
                TigerXExchange::Binance,
                TigerXMarket::Perp,
                &btc,
                CancelSide::Both,
            )
            .symbol,
            "BINANCE_PERP_BTC_USDT"
        );
        assert_eq!(
            TigerXRule::new(
                TigerXExchange::Okx,
                TigerXMarket::Spot,
                &btc,
                CancelSide::Buy,
            )
            .symbol,
            "OKX_SPOT_BTC_USDT"
        );
    }

    #[test]
    fn parser_does_not_require_order_qty_and_skips_terminal_or_market_orders() {
        let without_qty = serde_json::json!({
            "orderId":"1",
            "sym":"BINANCE_SPOT_BTC_USDT",
            "side":"BUY",
            "limitPrice":"100",
            "orderState":"NEW",
            "exchangeOrderType":"LIMIT"
        });
        let parsed = parse_open_order(&without_qty).unwrap().unwrap();
        assert_eq!(parsed.id, "1");
        assert_eq!(parsed.quantity, "");

        let terminal = serde_json::json!({
            "orderId":"2", "sym":"OKX_PERP_BTC_USDT", "side":"SELL",
            "orderState":"CANCELLED", "exchangeOrderType":"LIMIT"
        });
        assert!(parse_open_order(&terminal).unwrap().is_none());

        let market = serde_json::json!({
            "orderId":"3", "sym":"OKX_PERP_BTC_USDT", "side":"SELL",
            "orderState":"OPEN", "exchangeOrderType":"MARKET"
        });
        assert!(parse_open_order(&market).unwrap().is_none());
    }

    #[test]
    fn response_code_accepts_number_or_numeric_string() {
        assert_eq!(response_code(&serde_json::json!({"code":200000})), Some(200000));
        assert_eq!(response_code(&serde_json::json!({"code":"200000"})), Some(200000));
    }

    #[tokio::test]
    async fn account_wide_fetch_returns_binance_and_okx_orders() {
        let server = MockServer::new(vec![
            serde_json::json!({"code":200000,"message":"Success","data":{
                "page":1,"pageSize":1000,"pageNum":1,"totalSize":2,
                "list":[
                    {
                        "orderId":"1","sym":"BINANCE_SPOT_BTC_USDT","side":"BUY",
                        "limitPrice":"1030","orderQty":"2","orderState":"NEW",
                        "exchangeOrderType":"LIMIT"
                    },
                    {
                        "orderId":"2","sym":"OKX_PERP_ETH_USDT","side":"SELL",
                        "limitPrice":"1050","orderQty":"3","orderState":"OPEN",
                        "exchangeOrderType":"LIMIT"
                    }
                ]
            }}).to_string(),
        ]);
        let mut exchange = TigerX::new("key", "secret").unwrap();
        exchange.base_url = server.url.clone();
        exchange.client = client();

        let orders = exchange.fetch_open_orders().await.unwrap();
        assert_eq!(orders.len(), 2);
        assert_eq!(orders[0].symbol, "BINANCE_SPOT_BTC_USDT");
        assert_eq!(orders[1].symbol, "OKX_PERP_ETH_USDT");

        let requests = server.finish();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, "GET");
        assert!(requests[0].target.starts_with("/api/v1/trading/orders?"));
        assert!(requests[0].target.contains("page=1"));
        assert!(requests[0].target.contains("pageSize=1000"));
        assert!(!requests[0].target.contains("sym="));
    }

    #[tokio::test]
    async fn cancel_success_does_not_require_data_order_id() {
        let server = MockServer::new(vec![
            r#"{"code":200000,"message":"Success","data":{}}"#.into(),
        ]);
        let mut exchange = TigerX::new("key", "secret").unwrap();
        exchange.base_url = server.url.clone();
        exchange.client = client();

        exchange
            .cancel_one("BINANCE_PERP_BTC_USDT", "123")
            .await
            .unwrap();

        let requests = server.finish();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, "DELETE");
        assert_eq!(requests[0].target, "/api/v1/trading/order");
        let body: Value = serde_json::from_str(&requests[0].body).unwrap();
        assert_eq!(body["orderId"], "123");
    }

    #[tokio::test(start_paused = true)]
    async fn cancel_path_has_no_client_side_10_per_10s_budget() {
        let shared = TigerXShared::new();
        let start = tokio::time::Instant::now();
        for _ in 0..100 {
            shared.cooldown.wait().await;
        }
        assert_eq!(start.elapsed(), Duration::ZERO);
    }
}
