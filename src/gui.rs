//! Iced GUI for managing config.toml and controlling the cancellation engine.

use iced::task::Handle;
use iced::widget::{button, column, container, pick_list, row, scrollable, text, text_input, Column, Row};
use iced::{Element, Fill, Length, Task, Theme};
use std::future::Future;

use crate::config::{AppConfig, BinanceConfig, BybitConfig, OkxConfig, SymbolConfig};
use crate::engine;
use crate::exchange::CancelSide;

const SIDES: [CancelSide; 3] = [CancelSide::Buy, CancelSide::Sell, CancelSide::Both];
const BINANCE_MARKETS: [MarketChoice; 2] = [MarketChoice::Spot, MarketChoice::Futures];
const OKX_MARKETS: [MarketChoice; 2] = [MarketChoice::Spot, MarketChoice::Swap];
const BYBIT_MARKETS: [MarketChoice; 2] = [MarketChoice::Spot, MarketChoice::Linear];

const WINDOW_WIDTH: f32 = 820.0;
const WINDOW_HEIGHT: f32 = 620.0;
const WINDOW_MIN_WIDTH: f32 = 680.0;
const WINDOW_MIN_HEIGHT: f32 = 480.0;

const TITLE_SIZE: u32 = 20;
const EXCHANGE_TITLE_SIZE: u32 = 16;
const BODY_SIZE: u32 = 11;
const SMALL_SIZE: u32 = 10;
const FIELD_PADDING: [u16; 2] = [4, 6];
const BUTTON_PADDING: [u16; 2] = [4, 8];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExchangeId {
    Binance,
    Okx,
    Bybit,
}

impl ExchangeId {
    fn title(self) -> &'static str {
        match self {
            Self::Binance => "Binance",
            Self::Okx => "OKX",
            Self::Bybit => "Bybit",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MarketChoice {
    Spot,
    Futures,
    Swap,
    Linear,
}

impl MarketChoice {
    fn as_str(self) -> &'static str {
        match self {
            Self::Spot => "spot",
            Self::Futures => "futures",
            Self::Swap => "swap",
            Self::Linear => "linear",
        }
    }

    fn parse(value: &str, exchange: ExchangeId) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "futures" if exchange == ExchangeId::Binance => Self::Futures,
            "swap" if exchange == ExchangeId::Okx => Self::Swap,
            "linear" if exchange == ExchangeId::Bybit => Self::Linear,
            _ => Self::Spot,
        }
    }

    fn options(exchange: ExchangeId) -> &'static [Self] {
        match exchange {
            ExchangeId::Binance => &BINANCE_MARKETS,
            ExchangeId::Okx => &OKX_MARKETS,
            ExchangeId::Bybit => &BYBIT_MARKETS,
        }
    }
}

impl std::fmt::Display for MarketChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone)]
struct SymbolForm {
    symbol: String,
    side: CancelSide,
}

#[derive(Debug, Clone)]
struct ExchangeForm {
    api_key: String,
    api_secret: String,
    passphrase: String,
    market: MarketChoice,
    symbols: Vec<SymbolForm>,
}

impl ExchangeForm {
    fn blank(_exchange: ExchangeId) -> Self {
        Self {
            api_key: String::new(),
            api_secret: String::new(),
            passphrase: String::new(),
            market: MarketChoice::Spot,
            symbols: Vec::new(),
        }
    }

    fn symbols_from_config(symbols: &[SymbolConfig], fallback: &str) -> Vec<SymbolForm> {
        symbols.iter().map(|item| SymbolForm {
            symbol: item.symbol().to_string(),
            side: CancelSide::parse(item.side(fallback)).unwrap_or(CancelSide::Both),
        }).collect()
    }

    fn binance(config: Option<&BinanceConfig>) -> Self {
        match config {
            Some(c) => Self {
                api_key: c.api_key.clone(),
                api_secret: c.api_secret.clone(),
                passphrase: String::new(),
                market: MarketChoice::parse(&c.market, ExchangeId::Binance),
                symbols: Self::symbols_from_config(&c.symbols, &c.side),
            },
            None => Self::blank(ExchangeId::Binance),
        }
    }

    fn okx(config: Option<&OkxConfig>) -> Self {
        match config {
            Some(c) => Self {
                api_key: c.api_key.clone(),
                api_secret: c.api_secret.clone(),
                passphrase: c.passphrase.clone(),
                market: MarketChoice::parse(&c.market, ExchangeId::Okx),
                symbols: Self::symbols_from_config(&c.symbols, &c.side),
            },
            None => Self::blank(ExchangeId::Okx),
        }
    }

    fn bybit(config: Option<&BybitConfig>) -> Self {
        match config {
            Some(c) => Self {
                api_key: c.api_key.clone(),
                api_secret: c.api_secret.clone(),
                passphrase: String::new(),
                market: MarketChoice::parse(&c.market, ExchangeId::Bybit),
                symbols: Self::symbols_from_config(&c.symbols, &c.side),
            },
            None => Self::blank(ExchangeId::Bybit),
        }
    }

    fn detailed_symbols(&self) -> Vec<SymbolConfig> {
        self.symbols.iter().map(|item| SymbolConfig::Detailed {
            symbol: item.symbol.trim().to_string(),
            side: Some(item.side.as_str().to_string()),
        }).collect()
    }
}

struct State {
    config_path: String,
    poll_milliseconds: String,
    binance: ExchangeForm,
    okx: ExchangeForm,
    bybit: ExchangeForm,
    status: String,
    running: bool,
    run_handle: Option<Handle>,
}

#[derive(Debug, Clone)]
enum Message {
    PollChanged(String),
    ApiKeyChanged(ExchangeId, String),
    ApiSecretChanged(ExchangeId, String),
    PassphraseChanged(String),
    MarketChanged(ExchangeId, MarketChoice),
    SymbolChanged(ExchangeId, usize, String),
    SideChanged(ExchangeId, usize, CancelSide),
    AddSymbol(ExchangeId),
    RemoveSymbol(ExchangeId, usize),
    ClearCredentials(ExchangeId),
    Save,
    Start,
    Stop,
    EngineFinished(Result<(), String>),
}

impl State {
    fn boot() -> Self {
        let config_path = std::env::args().nth(1).unwrap_or_else(|| "config.toml".to_string());
        match AppConfig::load_or_default(&config_path) {
            Ok(config) => {
                let poll = config.poll_milliseconds
                    .or_else(|| config.poll_seconds.and_then(|s| s.checked_mul(1000)))
                    .unwrap_or(5000);
                Self {
                    config_path,
                    poll_milliseconds: poll.to_string(),
                    binance: ExchangeForm::binance(config.binance.as_ref()),
                    okx: ExchangeForm::okx(config.okx.as_ref()),
                    bybit: ExchangeForm::bybit(config.bybit.as_ref()),
                    status: "Ready. Changes are saved to config.toml.".to_string(),
                    running: false,
                    run_handle: None,
                }
            }
            Err(error) => {
                let mut state = Self::from_config(config_path, AppConfig::default());
                state.status = format!("Failed to load config.toml: {error:#}");
                state
            }
        }
    }

    fn from_config(config_path: String, config: AppConfig) -> Self {
        let poll = config.poll_milliseconds
            .or_else(|| config.poll_seconds.and_then(|s| s.checked_mul(1000)))
            .unwrap_or(5000);
        Self {
            config_path,
            poll_milliseconds: poll.to_string(),
            binance: ExchangeForm::binance(config.binance.as_ref()),
            okx: ExchangeForm::okx(config.okx.as_ref()),
            bybit: ExchangeForm::bybit(config.bybit.as_ref()),
            status: String::new(),
            running: false,
            run_handle: None,
        }
    }

    fn exchange(&self, id: ExchangeId) -> &ExchangeForm {
        match id {
            ExchangeId::Binance => &self.binance,
            ExchangeId::Okx => &self.okx,
            ExchangeId::Bybit => &self.bybit,
        }
    }

    fn exchange_mut(&mut self, id: ExchangeId) -> &mut ExchangeForm {
        match id {
            ExchangeId::Binance => &mut self.binance,
            ExchangeId::Okx => &mut self.okx,
            ExchangeId::Bybit => &mut self.bybit,
        }
    }

    fn build_config(&self) -> Result<AppConfig, String> {
        let poll = self.poll_milliseconds.trim().parse::<u64>()
            .map_err(|_| "poll_milliseconds must be an integer".to_string())?;

        let config = AppConfig {
            poll_milliseconds: Some(poll),
            poll_seconds: None,
            binance: Some(BinanceConfig {
                api_key: self.binance.api_key.trim().to_string(),
                api_secret: self.binance.api_secret.trim().to_string(),
                symbols: self.binance.detailed_symbols(),
                market: self.binance.market.as_str().to_string(),
                side: "both".to_string(),
            }),
            okx: Some(OkxConfig {
                api_key: self.okx.api_key.trim().to_string(),
                api_secret: self.okx.api_secret.trim().to_string(),
                passphrase: self.okx.passphrase.trim().to_string(),
                symbols: self.okx.detailed_symbols(),
                market: self.okx.market.as_str().to_string(),
                side: "both".to_string(),
            }),
            bybit: Some(BybitConfig {
                api_key: self.bybit.api_key.trim().to_string(),
                api_secret: self.bybit.api_secret.trim().to_string(),
                symbols: self.bybit.detailed_symbols(),
                market: self.bybit.market.as_str().to_string(),
                side: "both".to_string(),
            }),
        };
        config.validate().map_err(|error| format!("{error:#}"))?;
        Ok(config)
    }

    fn save_config(&mut self) -> Result<AppConfig, String> {
        let config = self.build_config()?;
        config.save(&self.config_path).map_err(|error| format!("{error:#}"))?;
        Ok(config)
    }
}

/// Lightweight executor for the GUI and network engine.
///
/// The default Tokio executor in iced creates a multi-thread runtime with a number of
/// worker threads based on available CPUs. This application only needs one worker because
/// all significant work is asynchronous HTTP I/O and timers.
struct SingleWorkerTokio(tokio::runtime::Runtime);

impl iced::Executor for SingleWorkerTokio {
    fn new() -> Result<Self, iced::futures::io::Error> {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .map(Self)
    }

    fn spawn(&self, future: impl Future<Output = ()> + Send + 'static) {
        let _ = self.0.spawn(future);
    }

    fn enter<R>(&self, f: impl FnOnce() -> R) -> R {
        let _guard = self.0.enter();
        f()
    }

    fn block_on<T>(&self, future: impl Future<Output = T>) -> T {
        self.0.block_on(future)
    }
}

pub fn run() -> iced::Result {
    iced::application(State::boot, update, view)
        .executor::<SingleWorkerTokio>()
        .title("Limit Canceller 0.3.1")
        .theme(Theme::Dark)
        .antialiasing(false)
        .window(iced::window::Settings {
            size: iced::Size::new(WINDOW_WIDTH, WINDOW_HEIGHT),
            min_size: Some(iced::Size::new(WINDOW_MIN_WIDTH, WINDOW_MIN_HEIGHT)),
            position: iced::window::Position::Centered,
            icon: Some(app_icon()),
            ..iced::window::Settings::default()
        })
        .run()
}

fn app_icon() -> iced::window::Icon {
    iced::window::icon::from_rgba(
        include_bytes!("../assets/app.rgba").to_vec(),
        64,
        64,
    )
    .expect("embedded application icon must be valid RGBA")
}

fn update(state: &mut State, message: Message) -> Task<Message> {
    match message {
        Message::PollChanged(value) => state.poll_milliseconds = value,
        Message::ApiKeyChanged(exchange, value) => state.exchange_mut(exchange).api_key = value,
        Message::ApiSecretChanged(exchange, value) => state.exchange_mut(exchange).api_secret = value,
        Message::PassphraseChanged(value) => state.okx.passphrase = value,
        Message::MarketChanged(exchange, market) => state.exchange_mut(exchange).market = market,
        Message::SymbolChanged(exchange, index, value) => {
            if let Some(item) = state.exchange_mut(exchange).symbols.get_mut(index) {
                item.symbol = value;
            }
        }
        Message::SideChanged(exchange, index, side) => {
            if let Some(item) = state.exchange_mut(exchange).symbols.get_mut(index) {
                item.side = side;
            }
        }
        Message::AddSymbol(exchange) => state.exchange_mut(exchange).symbols.push(SymbolForm {
            symbol: String::new(),
            side: CancelSide::Both,
        }),
        Message::RemoveSymbol(exchange, index) => {
            let symbols = &mut state.exchange_mut(exchange).symbols;
            if index < symbols.len() { symbols.remove(index); }
        }
        Message::ClearCredentials(exchange) => {
            let form = state.exchange_mut(exchange);
            form.api_key.clear();
            form.api_secret.clear();
            form.passphrase.clear();
            state.status = format!("{}: credentials cleared. Click Save to write the changes to disk.", exchange.title());
        }
        Message::Save => match state.save_config() {
            Ok(_) => {
                state.status = if state.running {
                    "Saved. The engine is still running with the previous configuration snapshot; click Stop and Start to apply changes.".to_string()
                } else {
                    format!("Saved: {}", state.config_path)
                };
            }
            Err(error) => state.status = format!("Save failed: {error}"),
        },
        Message::Start => {
            if state.running { return Task::none(); }
            let config = match state.save_config() {
                Ok(config) => config,
                Err(error) => {
                    state.status = format!("Failed to start: {error}");
                    return Task::none();
                }
            };
            if !config.has_enabled_exchange() {
                state.status = "Failed to start: configure api_key and api_secret for at least one exchange.".to_string();
                return Task::none();
            }
            state.running = true;
            state.status = "Running. Stop and start again to apply new settings.".to_string();
            let task = Task::perform(
                engine::run(config),
                |result| Message::EngineFinished(result.map_err(|error| format!("{error:#}"))),
            );
            let (task, handle) = task.abortable();
            state.run_handle = Some(handle);
            return task;
        }
        Message::Stop => {
            if let Some(handle) = state.run_handle.take() {
                handle.abort();
            }
            state.running = false;
            state.status = "Stopped.".to_string();
        }
        Message::EngineFinished(result) => {
            state.run_handle = None;
            state.running = false;
            state.status = match result {
                Ok(()) => "Engine stopped.".to_string(),
                Err(error) => format!("Engine stopped with an error: {error}"),
            };
        }
    }
    Task::none()
}

fn view(state: &State) -> Element<'_, Message> {
    let status_text = if state.running { "● RUNNING" } else { "● STOPPED" };

    let start = if state.running {
        button(text("Start").size(BODY_SIZE))
            .padding(BUTTON_PADDING)
            .style(button::success)
    } else {
        button(text("Start").size(BODY_SIZE))
            .padding(BUTTON_PADDING)
            .style(button::success)
            .on_press(Message::Start)
    };
    let stop = if state.running {
        button(text("Stop").size(BODY_SIZE))
            .padding(BUTTON_PADDING)
            .style(button::danger)
            .on_press(Message::Stop)
    } else {
        button(text("Stop").size(BODY_SIZE))
            .padding(BUTTON_PADDING)
            .style(button::danger)
    };

    let header = row![
        column![
            text("Limit Canceller 0.3.1").size(TITLE_SIZE),
            text(format!("Config: {}", state.config_path)).size(SMALL_SIZE),
        ]
        .spacing(2)
        .width(Fill),
        text(status_text).size(BODY_SIZE),
        button(text("Save").size(BODY_SIZE))
            .padding(BUTTON_PADDING)
            .style(button::primary)
            .on_press(Message::Save),
        start,
        stop,
    ]
    .spacing(7)
    .align_y(iced::alignment::Vertical::Center)
    .width(Fill);

    let settings = container(
        row![
            text("Polling, ms").size(BODY_SIZE).width(Length::Fixed(78.0)),
            text_input("100", &state.poll_milliseconds)
                .on_input(Message::PollChanged)
                .size(BODY_SIZE)
                .padding(FIELD_PADDING)
                .width(Length::Fixed(92.0)),
            text("Changes made while RUNNING are applied after restart.")
                .size(SMALL_SIZE),
        ]
        .spacing(8)
        .align_y(iced::alignment::Vertical::Center),
    )
    .padding(8)
    .width(Fill)
    .style(container::rounded_box);

    let content = column![
        header,
        settings,
        exchange_card(state, ExchangeId::Binance),
        exchange_card(state, ExchangeId::Okx),
        exchange_card(state, ExchangeId::Bybit),
        container(text(&state.status).size(SMALL_SIZE))
            .padding(8)
            .width(Fill)
            .style(container::bordered_box),
    ]
    .spacing(8)
    .padding(10)
    .width(Fill);

    scrollable(content).height(Fill).into()
}

fn exchange_card(state: &State, exchange: ExchangeId) -> Element<'_, Message> {
    let form = state.exchange(exchange);
    let enabled = !form.api_key.trim().is_empty() && !form.api_secret.trim().is_empty();
    let state_label = if enabled { "enabled" } else { "disabled (empty keys)" };

    let mut symbols: Column<'_, Message> = Column::new().spacing(5).width(Fill);
    if form.symbols.is_empty() {
        symbols = symbols.push(text("No tickers configured. Add a ticker below.").size(SMALL_SIZE));
    } else {
        for (index, item) in form.symbols.iter().enumerate() {
            let symbol_input = text_input("BTC/USDT", &item.symbol)
                .on_input(move |value| Message::SymbolChanged(exchange, index, value))
                .size(BODY_SIZE)
                .padding(FIELD_PADDING)
                .width(Length::FillPortion(5));
            let side = pick_list(
                SIDES,
                Some(item.side),
                move |value| Message::SideChanged(exchange, index, value),
            )
            .text_size(BODY_SIZE)
            .padding(FIELD_PADDING)
            .width(Length::FillPortion(2));
            let remove = button(text("Remove").size(BODY_SIZE))
                .padding(BUTTON_PADDING)
                .style(button::danger)
                .on_press(Message::RemoveSymbol(exchange, index));
            symbols = symbols.push(
                row![symbol_input, side, remove]
                    .spacing(6)
                    .align_y(iced::alignment::Vertical::Center)
                    .width(Fill),
            );
        }
    }

    let key_input = text_input("API key", &form.api_key)
        .on_input(move |value| Message::ApiKeyChanged(exchange, value))
        .size(BODY_SIZE)
        .padding(FIELD_PADDING)
        .width(Length::FillPortion(1));
    let secret_input = text_input("API secret", &form.api_secret)
        .secure(true)
        .on_input(move |value| Message::ApiSecretChanged(exchange, value))
        .size(BODY_SIZE)
        .padding(FIELD_PADDING)
        .width(Length::FillPortion(1));

    let mut credentials: Row<'_, Message> = row![key_input, secret_input]
        .spacing(6)
        .width(Fill);
    if exchange == ExchangeId::Okx {
        credentials = credentials.push(
            text_input("Passphrase", &form.passphrase)
                .secure(true)
                .on_input(Message::PassphraseChanged)
                .size(BODY_SIZE)
                .padding(FIELD_PADDING)
                .width(Length::FillPortion(1)),
        );
    }

    let top = row![
        column![
            text(exchange.title()).size(EXCHANGE_TITLE_SIZE),
            text(state_label).size(SMALL_SIZE),
        ]
        .spacing(1)
        .width(Fill),
        text("Market").size(BODY_SIZE),
        pick_list(
            MarketChoice::options(exchange),
            Some(form.market),
            move |value| Message::MarketChanged(exchange, value),
        )
        .text_size(BODY_SIZE)
        .padding(FIELD_PADDING)
        .width(Length::Fixed(112.0)),
        button(text("Clear keys").size(BODY_SIZE))
            .padding(BUTTON_PADDING)
            .style(button::danger)
            .on_press(Message::ClearCredentials(exchange)),
    ]
    .spacing(7)
    .align_y(iced::alignment::Vertical::Center)
    .width(Fill);

    container(
        column![
            top,
            credentials,
            text("Tickers / cancel side").size(BODY_SIZE),
            symbols,
            button(text("+ Add ticker").size(BODY_SIZE))
                .padding(BUTTON_PADDING)
                .style(button::secondary)
                .on_press(Message::AddSymbol(exchange)),
        ]
        .spacing(6)
        .width(Fill),
    )
    .padding(10)
    .width(Fill)
    .style(container::rounded_box)
    .into()
}
