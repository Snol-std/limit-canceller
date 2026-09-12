# Validation notes for v0.3.4

This environment does not contain Rust/Cargo or a Windows runtime, so a real `cargo check`, Windows release build, and Task Manager memory measurement could not be executed here.

Static checks performed:

- Package version remains `0.3.4`.
- `iced`, `iced_tiny_skia`, Slint, and `wgpu` are absent from `Cargo.toml`.
- `eframe = 0.36.2` is restored with `default-features = false` and only `glow` plus `default_fonts` enabled.
- The original eager single-worker Tokio runtime and normal tracing subscriber behavior from `0.3.4-egui.1` are restored.
- The failed memory-focused experiment from the earlier `0.3.4` draft is removed.
- Windows title-bar DWM attributes are still applied from `CreationContext` before the first visible frame.
- `SetWindowPos(... SWP_FRAMECHANGED ...)` is still used immediately after the DWM changes, preserving the confirmed title-bar fix.
- Dark caption, border, and caption-text colors remain enabled on supported Windows versions.
- The iced-like dark egui palette is preserved.
- Ordinary egui label selection remains disabled; text fields remain selectable/editable.
- Mouse-wheel multiplier remains `2.5x`; scroll animation remains disabled.
- Glow/OpenGL remains the only eframe renderer in this build.

Recommended Windows verification:

```powershell
cargo clean
cargo check
cargo test
cargo build --release
.\target\release\limit-canceller.exe
```
