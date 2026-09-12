#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Limit Canceller: iced GUI plus the open-order cancellation engine.
mod config;
mod engine;
mod exchange;
mod gui;
mod symbol;

fn main() -> iced::Result {
    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    gui::run()
}
