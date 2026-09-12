# Validation notes for v0.3.4-egui.1 experimental

This environment does not contain Rust/Cargo, so a real `cargo check` or Windows release build could not be executed here.

Static checks performed:

- `iced` was removed from `Cargo.toml`.
- No `wgpu` dependency is present in this experimental build.
- `eframe = 0.36.2` is pinned with `default-features = false` and only `glow` plus `default_fonts` enabled.
- The native renderer is explicitly set to `eframe::Renderer::Glow`.
- MSAA, depth buffer, stencil buffer, and dithering are disabled.
- egui animation time is set to zero to avoid animation-driven redraws.
- A single-worker Tokio runtime is retained for the asynchronous exchange engine.
- The engine completion task calls `egui::Context::request_repaint()` only when the engine exits, so no GUI polling timer is needed.
- Save / Start / Stop behavior is preserved.
- API secret and OKX passphrase inputs remain password-masked.
- English and Russian UI translations are preserved.
- Market and side remain configured per ticker.
- New tickers default to Binance futures, OKX swap, and Bybit linear.
- Symbol normalization and USDT/USDC support remain in the existing backend/config code.
- The compact inline `symbols = [{ market = ..., symbol = ..., side = ... }]` TOML format is unchanged.
- The application icon and release Windows GUI subsystem are retained.
- The stale iced-based `Cargo.lock` was intentionally removed; the first Cargo build will generate a fresh lockfile for the egui dependency graph.

Recommended Windows verification:

```powershell
cargo check
cargo test
cargo build --release
.\target\release\limit-canceller.exe
```

For the performance comparison, record idle RAM/CPU, CPU while moving the mouse over the window, and CPU while scrolling the exchange list.
