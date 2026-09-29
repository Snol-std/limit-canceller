# Validation notes for v0.4.1 TigerX burst cancellation

The TigerX changes are based on the supplied `document_text.txt` and `algo (01.07).yaml` reference.

## What the supplied API actually exposes

- `GET /api/v1/trading/orders` returns current open orders and allows optional `sym`, `exchange`, and `businessType` filters. `pageSize` supports up to 1000.
- `DELETE /api/v1/trading/order` cancels one incomplete order by `orderId` or `clientOrderId`. The document describes this endpoint as asynchronous and says `code=200000` means the request was accepted, not that the final exchange state is already cancelled.
- The same endpoint is documented as `10 requests per 10 seconds`.
- `DELETE /api/v1/trading/cancelAll` exists, but the provided schema only accepts `exchangeType`. It does not document a `sym`/ticker filter. Therefore it is not safe for normal per-ticker cancellation when unrelated orders may exist on the same TigerX Binance or OKX account.
- The private WebSocket reference documents immediate `Orders` push messages, but the supplied material does not document a WebSocket trading/cancel command or a symbol-scoped bulk-cancel request. This release therefore does not invent an undocumented endpoint.

## Why v0.4.0 could cancel only 3, 5, or another partial count immediately

The v0.4.0 client enforced its own rolling `10 / 10.1 s` `RequestBudget` before sending DELETE requests. That budget was shared by all TigerX ticker rules. If the previous few seconds had already consumed slots, a new grid could have only the remaining slots dispatched immediately. The rest waited locally even before TigerX saw them.

That local queue has been removed in v0.4.1.

## v0.4.1 cancellation behavior

- One account-wide open-order snapshot is still used so detection cost does not scale with the number of configured TigerX tickers.
- All open LIMIT orders matching the configured exact TigerX `sym` and cancellation side are submitted concurrently by exact `orderId`.
- There is no client-side 10-per-10-second cancellation quota. A burst of 100 matching order IDs is allowed to attempt 100 DELETE requests immediately.
- This is still selective: other TigerX tickers are not included in the burst unless they independently match another configured cancellation rule.
- If TigerX itself returns HTTP 429/418, the shared cooldown activates. `Retry-After` is honored when present; a TigerX 429 without that header falls back to 10 seconds because that is the documented cancellation window.
- Failed cancellations become retry-eligible after 100 ms once they are observed open again. Accepted asynchronous cancellations get 750 ms before the same still-open order can be sent again.
- The extra fixed 300 ms read gate was removed. The user's `poll_milliseconds` now controls normal TigerX polling cadence.

## Important limitation from the supplied docs

The TigerX terminal may have a private/internal symbol-scoped bulk-cancel path, but it is not present in `document_text.txt` or `algo (01.07).yaml`. The only documented bulk endpoint is exchange-wide `cancelAll(exchangeType)`. Therefore this build pursues maximum documented ticker-safe speed by concurrent single-order DELETEs rather than risking unrelated orders.

## Tests updated

- Existing TigerX signature and parser tests remain.
- Existing account-wide open-order fetch test remains.
- Existing asynchronous cancel-success test remains.
- The old test that expected the 11th cancellation to wait 10.1 seconds was replaced: 100 client-side cancel starts are now allowed at zero simulated time when there is no server cooldown.

## Local verification

This environment does not contain a Rust/Cargo toolchain, so compile and live API verification must be run on Windows:

```powershell
cargo check
cargo test
cargo build --release
.\target\release\limit-canceller.exe
```

Recommended real TigerX smoke test: create 20-100 LIMIT orders on one TigerX ticker while keeping an unrelated ticker open. Start the canceller with only the target ticker/side configured. The target batch should be submitted immediately without waiting for a local 10-second rolling budget, while the unrelated ticker remains untouched. If TigerX returns real 429 responses, capture the logs because that establishes the server-side limit actually enforced for the API key.


# v0.4.2 autosave validation

Autosave is implemented in the GUI only; exchange cancellation code is unchanged from v0.4.1.

Expected behavior:
- Editing any persisted field schedules autosave after 500 ms of inactivity.
- Repeated keystrokes reset the deadline instead of writing on every key event.
- Start performs an immediate validated save and cancels any pending autosave timer.
- A running engine keeps the configuration snapshot captured at Start; autosaved edits take effect only after Stop -> Start or application restart.
- Autosave normalizes symbols on cloned form data, so a valid input such as `btc` is written as `BTC/USDT` without rewriting the text box while the user is editing.
- Invalid partial input is not persisted and the status line reports the validation error.
- Closing the application while a valid autosave is pending attempts one immediate final write.
- Manual Save remains available as an optional force-save action.
- Bybit/OKX unit tests now refer to `crate::exchange::CancelSide::Both`.
