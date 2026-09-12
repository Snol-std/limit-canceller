# Limit Canceller

Limit Canceller is a lightweight Rust application for automatically monitoring and cancelling open orders on Binance, OKX, and Bybit. It supports exchange-specific markets, independent ticker lists, per-ticker cancellation rules (`buy`, `sell`, or `both`), millisecond polling, API rate-limit handling, and a compact Windows GUI for managing configuration and controlling the cancellation engine. The GUI is English by default and can be switched to Russian from the application settings.

## GitHub repository:

https://github.com/Snol-std/limit-canceller

## Release Notes

### v0.1.0
- Initial release.
- Added the first console-based order cancellation workflow.
- Added initial support for Binance, OKX, and Bybit.

### v0.1.1
- Maintenance release focused on stability of the initial implementation.
- Improved request error handling and logging around exchange operations.
- Improved configuration validation and general runtime reliability.

### v0.2.0
- Reworked the cancellation engine for faster and more reliable order removal.
- Added mass cancellation for Binance and Bybit and batch cancellation for OKX, with up to 20 orders per OKX batch.
- Added `poll_milliseconds` for sub-second polling intervals.
- Improved exchange request signing, pagination, API response validation, rate-limit handling, and cooldown behavior.
- Prevented missed polling ticks from accumulating when a previous cycle takes longer than the configured interval.

### v0.2.1
- Changed Bybit cancellation from cancel-all to individual order cancellation by `orderId`.
- Added concurrent Bybit order cancellation while keeping Binance and OKX on their optimized bulk/batch cancellation paths.
- Added shared Bybit request throttling to stay within cancellation API limits.
- Refined the asynchronous per-symbol cancellation workflow.

### v0.2.2
- Fixed the Bybit async lifetime / `FnOnce is not general enough` compilation error.
- Empty `api_key` or `api_secret` values now disable only the affected exchange instead of stopping the whole application.
- Moved ticker configuration from one global symbol list to separate `symbols` lists for Binance, OKX, and Bybit.
- Improved startup validation so enabled and disabled exchanges are handled independently.

### v0.2.3
- Added automatic Binance server-time synchronization for signed requests.
- Added clock-offset compensation to prevent Binance error `-1021` (`Timestamp for this request is outside of the recvWindow`).
- Added automatic time resynchronization and immediate retry when Binance returns `-1021`.
- Kept the synchronized Binance time offset shared across all Binance ticker workers.

### v0.2.4
- Added cancellation-side selection for each exchange: `buy`, `sell`, or `both`.
- Binance now uses fast cancel-all requests for `both` and individual `orderId` cancellation when only `buy` or `sell` should be removed.
- OKX and Bybit now filter open orders by side before sending cancellation requests.
- Disabled ANSI terminal colors so logs display correctly in Windows `cmd.exe`.

### v0.2.5
- Added independent cancellation-side settings for every ticker on every exchange.
- A single exchange can now use different rules for different symbols, for example `BTC/USDT = sell` and `ETH/USDT = both`.
- Kept backward compatibility with the older string-only `symbols` format by using the exchange-level `side` value as a fallback.
- Added duplicate-symbol validation to prevent multiple workers from managing the same ticker on the same exchange.

### v0.3.0
- Added the first graphical interface using `iced 0.14`.
- Added GUI controls for Binance, OKX, and Bybit API credentials and market selection.
- Added GUI controls to add and remove tickers and configure `buy`, `sell`, or `both` independently for each ticker.
- Added **Save**, **Start**, and **Stop** controls for managing `config.toml` and the cancellation engine directly from the application.
- API secret and OKX passphrase fields are masked in the interface.

### v0.3.1
- Redesigned the GUI to use a smaller window, smaller fonts, tighter spacing, and a more compact layout.
- Added an application icon for the window, taskbar, and Windows executable.
- Fixed Windows resource compilation by resolving the application icon through `CARGO_MANIFEST_DIR`.
- Fixed `iced 0.14` text-size compilation errors by using supported pixel types.
- Release builds now use the Windows GUI subsystem, so launching `limit-canceller.exe` does not open a console window.
- Replaced the heavyweight `wgpu` GUI renderer with `tiny-skia` to significantly reduce memory and CPU usage.
- Added a single-worker Tokio executor for the GUI and asynchronous network engine to reduce idle resource usage.
- Disabled GUI antialiasing and unnecessary `iced` default features for a lighter build.
- Added release-build optimizations including LTO, a single codegen unit, symbol stripping, and `panic = "abort"`.



### v0.3.2
- Changed all default GUI text to English.
- Added a GUI language selector with English and Russian translations.
- Added localized labels, buttons, status messages, placeholders, exchange state text, and per-ticker cancellation-side names.
- Added persistent `ui_language` configuration (`en` or `ru`), with English as the default for existing configurations.
- Kept all source-code comments and technical documentation in English.
- Preserved the low-resource `tiny-skia` renderer, single-worker Tokio executor, compact layout, application icon, and Windows GUI subsystem introduced in v0.3.1.
