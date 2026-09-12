# Validation — 0.3.0 GUI

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
cargo test
cargo run --release
```

Iced 0.14 specifies `rust-version = 1.88`; Rust 1.97.1 is compatible.
