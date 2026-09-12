# Validation notes for v0.3.3

This environment does not contain Rust/Cargo, so a real `cargo check` or release build could not be executed here.

Static checks performed:

- Project version and GUI title are `0.3.3`.
- Exchange-level `market` and `side` fields were removed from the active configuration schema.
- Each ticker now requires its own `market`, `symbol`, and `side`.
- The legacy string-only `symbols = ["BTC/USDT"]` format is rejected by the new schema.
- GUI ticker rows contain a lightweight two-state market button, symbol input, cancellation-side selector, and remove button.
- Symbol normalization supports bare base symbols, concatenated USDT/USDC pairs, slash-separated pairs, and backslash-separated pairs.
- Bare base symbols default to USDT.
- Only USDT and USDC quote assets are accepted.
- The same normalized symbol is allowed in different markets on the same exchange, but duplicate `market + symbol` entries are rejected.
- The engine starts independent worker groups for each configured market on Binance, OKX, and Bybit.
- GUI saves use compact inline ticker tables under `symbols = [...]`; `[[exchange.symbols]]` output is no longer generated.
- Low-resource `iced` settings from v0.3.1/v0.3.2 remain unchanged.
- English remains the default UI language and Russian remains available from the GUI selector.
- Static header/settings are outside the main `scrollable`, reducing clipped text and redraw work during scrolling.
- Per-ticker market `pick_list` widgets were removed to avoid the iced 0.14 tiny-skia clipping hot path during scroll repaints.

Recommended local verification on Windows:

```powershell
cargo check
cargo test
cargo build --release
```
