# Validation notes for v0.4.0 TigerX hotfix

The TigerX implementation was reworked against the supplied `document_text.txt` and `algo (01.07).yaml` API reference.

## API details used

- `GET /api/v1/trading/orders` supports optional `sym`, `exchange`, and `businessType` filters and defaults to page size up to 1000. The hotfix therefore performs one account-wide read and filters configured TigerX rules locally.
- `DELETE /api/v1/trading/order` is asynchronous and is limited to 10 requests per 10 seconds. `code=200000` means the cancellation request was accepted; it is not final-state confirmation.
- The official TigerX Java sample signs sorted raw parameters, appends `&nonce`, uses HMAC-SHA256, and sends `orderId` in the JSON body for DELETE. The implementation keeps that wire format.
- The official Java sample uses a 5-second HTTP timeout.
- Order states eligible for cancellation are `NEW`, `OPEN`, and `PARTIALLY_FILLED`.

## Fixed failure/delay modes

- Removed per-ticker TigerX REST polling. The old shared 300 ms read gate multiplied detection latency by the number of configured TigerX symbols; one portfolio-wide sweep now services Binance + OKX and SPOT + PERP together.
- Cancellation requests are detached from the read loop through a managed `JoinSet`. Requests above the documented 10/10s budget can wait without stopping new open-order discovery. Dropping the TigerX runner aborts those child tasks, so GUI Stop still stops all queued work.
- One malformed order no longer fails the whole page. The parser skips only that record and continues.
- `orderQty` is no longer mandatory for cancellation. The API documentation allows cases where it is absent, and cancellation only requires the order ID.
- Numeric and string representations of TigerX response `code`, `orderId`, and `totalSize` are tolerated where appropriate.
- Explicit terminal orders and explicit non-LIMIT orders are ignored.
- A successful asynchronous cancellation no longer fails merely because `data.orderId` is missing. If TigerX echoes a mismatched ID, it is logged without converting an otherwise accepted request into a global polling backoff.
- Accepted/failed cancellation attempts are deduplicated while active and retried only if the same order remains open after a short grace period.
- HTTP request timeout reduced from 15 seconds to 5 seconds to match the supplied TigerX example and avoid long stalls on a dead request.

## Tests added/updated

- Fixed signature vector for account-wide open-order pagination.
- TigerX symbol construction for Binance/OKX and spot/perp.
- Parser accepts an order without `orderQty` and skips terminal/market orders.
- Response `code` accepts both JSON number and numeric string forms.
- One account-wide open-orders request can return both Binance and OKX orders without a `sym` query filter.
- Successful cancel response does not require `data.orderId`.
- Shared cancellation budget still allows a burst of 10 and delays the 11th until the rolling 10-second window opens.

## Local verification required

This environment does not contain a Rust/Cargo toolchain, so compilation could not be executed here. Run on Windows:

```powershell
cargo check
cargo test
cargo build --release
.\target\release\limit-canceller.exe
```

For a real TigerX smoke test, enable tracing and verify that a new configured limit order is discovered in the next account-wide sweep and logs `tigerx: cancellation request accepted` without delays that scale with the number of configured TigerX tickers.
