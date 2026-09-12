# Validation Notes

Statically verified:

- The GUI uses the iced 0.14 API (`application`, `Task`, `Task::abortable`, `text_input`, `pick_list`, `scrollable`).
- `api_secret` and the OKX `passphrase` use secure text inputs.
- The GUI loads an existing `config.toml`, including legacy string-only `symbols` entries.
- When saved, the GUI writes detailed symbol entries with an independent `side` per ticker.
- Start saves and validates the configuration before launching the existing async engine.
- Stop triggers the iced abort handle; dropping the engine future also stops its child `JoinSet` tasks.
- If every exchange has empty credentials, Start does not launch the engine.
- Editing settings while RUNNING does not mutate the configuration snapshot already used by the engine.
- Existing exchange adapters are preserved.

Environment limitation:

Rust/Cargo is not installed in the validation environment, so `cargo check` and `cargo test` cannot be executed here. On a machine with Rust 1.88+ run:

```powershell
cargo check
cargo test --locked
cargo run --release
```

Iced 0.14 specifies `rust-version = 1.88`; Rust 1.97.1 is compatible.

Additional static checks for v0.3.2:

- Window settings use 820×620, a 680×480 minimum size, and centered positioning.
- The embedded RGBA icon is 64×64×4 = 16384 bytes.
- The Windows `.ico` contains multiple sizes.
- `build.rs` uses `winres` only under `cfg(windows)`.
- The version in Cargo.toml and the GUI title is 0.3.2.
- iced default features are disabled; the GUI uses the lightweight `tiny-skia` renderer instead of `wgpu`.
- The custom Tokio executor uses a single worker thread to reduce idle resource usage.

- English is the default GUI language.
- Russian can be selected from the GUI language picker.
- The selected language is persisted as `ui_language = "en"` or `ui_language = "ru"` in `config.toml`.
- No Cyrillic text appears in Rust comments or documentation comments; Cyrillic is limited to runtime Russian localization strings.
