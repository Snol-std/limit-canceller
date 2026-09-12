# Validation 0.2.5

Changes:

- cancellation side moved to the individual ticker level;
- the legacy string-only `symbols` format remains compatible through the exchange-level `side` fallback;
- different exchanges can use different sides for the same ticker;
- duplicate entries for the same ticker within one exchange are not allowed;
- each worker receives its own `CancelSide` and filters only the matching orders;
- ANSI output remains disabled;
- the Binance time-offset fix is preserved.

If Cargo is installed, verify with:

```powershell
cargo test --locked
cargo run --release
```