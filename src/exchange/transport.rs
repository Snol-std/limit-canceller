//! URL encoding, shared exchange cooldown, and HTTP response validation.
use std::{collections::VecDeque, time::Duration};
use anyhow::{bail, Context, Result};
use reqwest::{header::HeaderMap, Response};
use serde_json::Value;
use tokio::{sync::Mutex, time::Instant};

pub(crate) fn query_string(params: &[(&str, String)]) -> String {
    // Url uses standard application/x-www-form-urlencoded encoding.
    let mut url = reqwest::Url::parse("https://localhost/").expect("constant URL");
    { let mut query = url.query_pairs_mut();
      for (key, value) in params { query.append_pair(key, value); }
    }
    url.query().unwrap_or("").to_string()
}

pub(crate) fn required_string<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v.get(key).and_then(Value::as_str).filter(|s| !s.is_empty())
        .with_context(|| format!("response is missing non-empty string field {key}"))
}

/// Shared request-start limiter for all symbols on one exchange/endpoint.
/// The signing timestamp must be generated AFTER wait(), otherwise the signature may become stale.
pub(crate) struct RequestGate {
    next: Mutex<Instant>,
    spacing: Duration,
}
impl RequestGate {
    pub fn new(spacing: Duration) -> Self { Self { next: Mutex::new(Instant::now()), spacing } }
    pub async fn wait(&self) {
        loop {
            let mut next = self.next.lock().await;
            let now = Instant::now();
            if *next <= now { *next = now + self.spacing; return; }
            let until = *next;
            drop(next);
            tokio::time::sleep_until(until).await;
        }
    }
    pub async fn pause(&self, delay: Duration) {
        let mut next = self.next.lock().await;
        *next = (*next).max(Instant::now() + delay);
    }
    /// Reserves a slot only after cooldown: waiting symbols will not send
    /// accumulated requests simultaneously when the exchange pause ends.
    pub async fn wait_after(&self, cooldown: &Self) {
        loop {
            let mut next = self.next.lock().await;
            let paused_until = cooldown.next.lock().await;
            let until = (*next).max(*paused_until);
            let now = Instant::now();
            if until <= now { *next = now + self.spacing; return; }
            drop(paused_until);
            drop(next);
            tokio::time::sleep_until(until).await;
        }
    }
}

/// Rolling-window quota: allows multiple requests at once but does not
/// exceed the shared budget across all symbols. Used only for Bybit.
pub(crate) struct RequestBudget {
    starts: Mutex<VecDeque<Instant>>,
    limit: usize,
    window: Duration,
}
impl RequestBudget {
    pub fn new(limit: usize, window: Duration) -> Self {
        assert!(limit > 0);
        Self { starts: Mutex::new(VecDeque::new()), limit, window }
    }
    pub async fn wait_after(&self, cooldown: &RequestGate) {
        loop {
            let mut starts = self.starts.lock().await;
            let paused_until = cooldown.next.lock().await;
            let now = Instant::now();
            while starts.front().is_some_and(|t| *t + self.window <= now) { starts.pop_front(); }
            let mut until = *paused_until;
            if starts.len() >= self.limit {
                until = until.max(*starts.front().expect("nonempty budget") + self.window);
            }
            if until <= now { starts.push_back(now); return; }
            drop(paused_until);
            drop(starts);
            tokio::time::sleep_until(until).await;
        }
    }
}

fn retry_after(headers: &HeaderMap, fallback: Duration) -> Duration {
    let Some(raw) = headers.get("retry-after").and_then(|v| v.to_str().ok()) else { return fallback; };
    if let Ok(seconds) = raw.parse::<u64>() { return Duration::from_secs(seconds.min(86_400)).max(fallback); }
    if let Ok(date) = chrono::DateTime::parse_from_rfc2822(raw) {
        let ms = (date.timestamp_millis() - chrono::Utc::now().timestamp_millis()).clamp(0, 86_400_000);
        return Duration::from_millis(ms as u64).max(fallback);
    }
    fallback
}

pub(crate) async fn response_json(response: Response, name: &str, gate: &RequestGate) -> Result<Value> {
    let status = response.status();
    if status.as_u16() == 429 || status.as_u16() == 418 {
        let fallback = if status.as_u16() == 418 { 120 } else { 1 };
        gate.pause(retry_after(response.headers(), Duration::from_secs(fallback))).await;
    } else if name == "bybit" && status.as_u16() == 403 {
        gate.pause(Duration::from_secs(600)).await;
    }
    let text = response.text().await.with_context(|| format!("{name}: response body"))?;
    let value: Value = serde_json::from_str(&text).with_context(||
        format!("{name}: non-JSON response (HTTP {status}): {}", super::text_snippet(&text)))?;
    if !status.is_success() { bail!("{name}: HTTP {status}: {}", super::text_snippet(&text)); }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cursor_is_encoded_once() {
        let query = query_string(&[("cursor", "a+b/c==&x%20".into())]);
        assert_eq!(query, "cursor=a%2Bb%2Fc%3D%3D%26x%2520");
    }
    #[tokio::test(start_paused = true)]
    async fn gate_paces_requests_and_respects_cooldown() {
        let gate = RequestGate::new(Duration::from_millis(100));
        gate.wait().await;
        let before = Instant::now();
        gate.wait().await;
        assert!(before.elapsed() >= Duration::from_millis(100));
        gate.pause(Duration::from_secs(2)).await;
        let before = Instant::now();
        gate.wait().await;
        assert!(before.elapsed() >= Duration::from_secs(2));
    }
}
