//! Iced GUI for managing config.toml and controlling the cancellation engine.

use iced::task::Handle;
use iced::widget::{button, column, container, pick_list, row, scrollable, text, text_input, Column, Row};
use iced::{Element, Fill, Length, Task, Theme};
use std::future::Future;

use crate::config::{AppConfig, BinanceConfig, BybitConfig, OkxConfig, SymbolConfig};
use crate::engine;
use crate::exchange::CancelSide;
use crate::symbol::Symbol;

const LANGUAGES: [Language; 2] = [Language::English, Language::Russian];
const SIDES_EN: [SideChoice; 3] = [SideChoice::BuyEn, SideChoice::SellEn, SideChoice::BothEn];
const SIDES_RU: [SideChoice; 3] = [SideChoice::BuyRu, SideChoice::SellRu, SideChoice::BothRu];

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
enum Language {
    English,
    Russian,
}

impl Language {
    fn code(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::Russian => "ru",
        }
    }

    fn parse(value: Option<&str>) -> Self {
        match value.unwrap_or("en").trim().to_ascii_lowercase().as_str() {
            "ru" | "rus" | "russian" => Self::Russian,
            _ => Self::English,
        }
    }
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::English => "English",
            Self::Russian => "Русский",
        })
    }
}

#[derive(Clone, Copy)]
struct UiText {
    ready: &'static str,
    config_label: &'static str,
    running_badge: &'static str,
    stopped_badge: &'static str,
    save: &'static str,
    start: &'static str,
    stop: &'static str,
    polling: &'static str,
    language: &'static str,
    changes_running: &'static str,
    enabled: &'static str,
    disabled_empty_keys: &'static str,
    no_tickers: &'static str,
    api_key: &'static str,
    api_secret: &'static str,
    passphrase: &'static str,
    clear_keys: &'static str,
    tickers_market_cancel_side: &'static str,
    add_ticker: &'static str,
    remove: &'static str,
    credentials_cleared: &'static str,
    saved_running: &'static str,
    saved_prefix: &'static str,
    save_failed: &'static str,
    failed_to_start: &'static str,
    configure_exchange: &'static str,
    running_status: &'static str,
    stopped: &'static str,
    engine_stopped: &'static str,
    engine_error: &'static str,
    failed_load_config: &'static str,
}

fn ui_text(language: Language) -> UiText {
    match language {
        Language::English => UiText {
            ready: "Ready. Changes are saved to config.toml.",
            config_label: "Config",
            running_badge: "● RUNNING",
            stopped_badge: "● STOPPED",
            save: "Save",
            start: "Start",
            stop: "Stop",
            polling: "Polling, ms",
            language: "Language",
            changes_running: "Changes made while RUNNING are applied after restart.",
            enabled: "enabled",
            disabled_empty_keys: "disabled (empty keys)",
            no_tickers: "No tickers configured. Add a ticker below.",
            api_key: "API key",
            api_secret: "API secret",
            passphrase: "Passphrase",
            clear_keys: "Clear keys",
            tickers_market_cancel_side: "Tickers / market / cancel side",
            add_ticker: "+ Add ticker",
            remove: "Remove",
            credentials_cleared: "credentials cleared. Click Save to write the changes to disk.",
            saved_running: "Saved. The engine is still running with the previous configuration snapshot; click Stop and Start to apply changes.",
            saved_prefix: "Saved",
            save_failed: "Save failed",
            failed_to_start: "Failed to start",
            configure_exchange: "configure api_key and api_secret for at least one exchange.",
            running_status: "Running. Stop and start again to apply new settings.",
            stopped: "Stopped.",
            engine_stopped: "Engine stopped.",
            engine_error: "Engine stopped with an error",
            failed_load_config: "Failed to load config.toml",
        },
        Language::Russian => UiText {
            ready: "Готово. Изменения сохраняются в config.toml.",
            config_label: "Конфиг",
            running_badge: "● РАБОТАЕТ",
            stopped_badge: "● ОСТАНОВЛЕНО",
            save: "Сохранить",
            start: "Старт",
            stop: "Стоп",
            polling: "Опрос, мс",
            language: "Язык",
            changes_running: "Изменения во время работы применяются после перезапуска.",
            enabled: "включена",
            disabled_empty_keys: "выключена (пустые ключи)",
            no_tickers: "Нет тикеров. Добавьте тикер ниже.",
            api_key: "API-ключ",
            api_secret: "API-секрет",
            passphrase: "Парольная фраза",
            clear_keys: "Очистить ключи",
            tickers_market_cancel_side: "Тикеры / рынок / сторона снятия",
            add_ticker: "+ Добавить тикер",
            remove: "Удалить",
            credentials_cleared: "ключи API очищены. Нажмите «Сохранить», чтобы записать изменения на диск.",
            saved_running: "Сохранено. Алгоритм продолжает работать с предыдущей копией настроек; нажмите «Стоп», затем «Старт», чтобы применить изменения.",
            saved_prefix: "Сохранено",
            save_failed: "Ошибка сохранения",
            failed_to_start: "Не удалось запустить",
            configure_exchange: "укажите api_key и api_secret хотя бы для одной биржи.",
            running_status: "Запущено. Чтобы применить новые настройки, остановите и запустите снова.",
            stopped: "Остановлено.",
            engine_stopped: "Алгоритм остановлен.",
            engine_error: "Алгоритм остановлен с ошибкой",
            failed_load_config: "Не удалось загрузить config.toml",
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SideChoice {
    BuyEn,
    SellEn,
    BothEn,
    BuyRu,
    SellRu,
    BothRu,
}

impl SideChoice {
    fn options(language: Language) -> &'static [Self] {
        match language {
            Language::English => &SIDES_EN,
            Language::Russian => &SIDES_RU,
        }
    }

    fn from_cancel_side(side: CancelSide, language: Language) -> Self {
        match (language, side) {
            (Language::English, CancelSide::Buy) => Self::BuyEn,
            (Language::English, CancelSide::Sell) => Self::SellEn,
            (Language::English, CancelSide::Both) => Self::BothEn,
            (Language::Russian, CancelSide::Buy) => Self::BuyRu,
            (Language::Russian, CancelSide::Sell) => Self::SellRu,
            (Language::Russian, CancelSide::Both) => Self::BothRu,
        }
    }

    fn cancel_side(self) -> CancelSide {
        match self {
            Self::BuyEn | Self::BuyRu => CancelSide::Buy,
            Self::SellEn | Self::SellRu => CancelSide::Sell,
            Self::BothEn | Self::BothRu => CancelSide::Both,
        }
    }
}

impl std::fmt::Display for SideChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::BuyEn => "buy",
            Self::SellEn => "sell",
            Self::BothEn => "both",
            Self::BuyRu => "покупка",
            Self::SellRu => "продажа",
            Self::BothRu => "обе стороны",
        })
    }
}

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

    /// Returns the other market available for this exchange.
    ///
    /// A button is used instead of a per-row `pick_list` because the software
    /// `tiny-skia` renderer in iced 0.14 becomes expensive when many clipped
    /// dropdown widgets are repainted while scrolling. Each exchange has only
    /// two supported markets, so a direct toggle keeps the same functionality
    /// with a much cheaper widget tree.
    fn toggled(self, exchange: ExchangeId) -> Self {
        match exchange {
            ExchangeId::Binance => match self {
                Self::Futures => Self::Spot,
                _ => Self::Futures,
            },
            ExchangeId::Okx => match self {
                Self::Swap => Self::Spot,
                _ => Self::Swap,
            },
            ExchangeId::Bybit => match self {
                Self::Linear => Self::Spot,
                _ => Self::Linear,
            },
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
    market: MarketChoice,
    symbol: String,
    side: CancelSide,
}

#[derive(Debug, Clone)]
struct ExchangeForm {
    api_key: String,
    api_secret: String,
    passphrase: String,
    symbols: Vec<SymbolForm>,
}

impl ExchangeForm {
    fn blank(_exchange: ExchangeId) -> Self {
        Self {
            api_key: String::new(),
            api_secret: String::new(),
            passphrase: String::new(),
            symbols: Vec::new(),
        }
    }

    fn symbols_from_config(symbols: &[SymbolConfig], exchange: ExchangeId) -> Vec<SymbolForm> {
        symbols
            .iter()
            .map(|item| SymbolForm {
                market: MarketChoice::parse(&item.market, exchange),
                symbol: item.symbol.clone(),
                side: CancelSide::parse(&item.side).unwrap_or(CancelSide::Both),
            })
            .collect()
    }

    fn binance(config: Option<&BinanceConfig>) -> Self {
        match config {
            Some(c) => Self {
                api_key: c.api_key.clone(),
                api_secret: c.api_secret.clone(),
                passphrase: String::new(),
                symbols: Self::symbols_from_config(&c.symbols, ExchangeId::Binance),
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
                symbols: Self::symbols_from_config(&c.symbols, ExchangeId::Okx),
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
                symbols: Self::symbols_from_config(&c.symbols, ExchangeId::Bybit),
            },
            None => Self::blank(ExchangeId::Bybit),
        }
    }

    fn normalize_symbols(&mut self) -> Result<(), String> {
        self.symbols.retain(|item| !item.symbol.trim().is_empty());
        for item in &mut self.symbols {
            let symbol = Symbol::parse(&item.symbol).map_err(|error| format!("{error:#}"))?;
            item.symbol = symbol.canonical();
        }
        Ok(())
    }

    fn detailed_symbols(&self) -> Vec<SymbolConfig> {
        self.symbols
            .iter()
            .map(|item| SymbolConfig {
                market: item.market.as_str().to_string(),
                symbol: item.symbol.trim().to_string(),
                side: item.side.as_str().to_string(),
            })
            .collect()
    }
}

struct State {
    config_path: String,
    poll_milliseconds: String,
    language: Language,
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
    LanguageChanged(Language),
    ApiKeyChanged(ExchangeId, String),
    ApiSecretChanged(ExchangeId, String),
    PassphraseChanged(String),
    MarketChanged(ExchangeId, usize, MarketChoice),
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
        let config_path = std::env::args()
            .nth(1)
            .unwrap_or_else(|| "config.toml".to_string());
        match AppConfig::load_or_default(&config_path) {
            Ok(config) => {
                let language = Language::parse(config.ui_language.as_deref());
                let poll = config
                    .poll_milliseconds
                    .or_else(|| config.poll_seconds.and_then(|s| s.checked_mul(1000)))
                    .unwrap_or(5000);
                Self {
                    config_path,
                    poll_milliseconds: poll.to_string(),
                    language,
                    binance: ExchangeForm::binance(config.binance.as_ref()),
                    okx: ExchangeForm::okx(config.okx.as_ref()),
                    bybit: ExchangeForm::bybit(config.bybit.as_ref()),
                    status: ui_text(language).ready.to_string(),
                    running: false,
                    run_handle: None,
                }
            }
            Err(error) => {
                let language = Language::English;
                let mut state = Self::from_config(config_path, AppConfig::default());
                state.language = language;
                state.status = format!("{}: {error:#}", ui_text(language).failed_load_config);
                state
            }
        }
    }

    fn from_config(config_path: String, config: AppConfig) -> Self {
        let language = Language::parse(config.ui_language.as_deref());
        let poll = config
            .poll_milliseconds
            .or_else(|| config.poll_seconds.and_then(|s| s.checked_mul(1000)))
            .unwrap_or(5000);
        Self {
            config_path,
            poll_milliseconds: poll.to_string(),
            language,
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
        let poll = self
            .poll_milliseconds
            .trim()
            .parse::<u64>()
            .map_err(|_| "poll_milliseconds must be an integer".to_string())?;

        let config = AppConfig {
            poll_milliseconds: Some(poll),
            poll_seconds: None,
            ui_language: Some(self.language.code().to_string()),
            binance: Some(BinanceConfig {
                api_key: self.binance.api_key.trim().to_string(),
                api_secret: self.binance.api_secret.trim().to_string(),
                symbols: self.binance.detailed_symbols(),
            }),
            okx: Some(OkxConfig {
                api_key: self.okx.api_key.trim().to_string(),
                api_secret: self.okx.api_secret.trim().to_string(),
                passphrase: self.okx.passphrase.trim().to_string(),
                symbols: self.okx.detailed_symbols(),
            }),
            bybit: Some(BybitConfig {
                api_key: self.bybit.api_key.trim().to_string(),
                api_secret: self.bybit.api_secret.trim().to_string(),
                symbols: self.bybit.detailed_symbols(),
            }),
        };
        config.validate().map_err(|error| format!("{error:#}"))?;
        Ok(config)
    }

    fn save_config(&mut self) -> Result<AppConfig, String> {
        self.binance.normalize_symbols()?;
        self.okx.normalize_symbols()?;
        self.bybit.normalize_symbols()?;
        let config = self.build_config()?;
        config
            .save(&self.config_path)
            .map_err(|error| format!("{error:#}"))?;
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
        .title("Limit Canceller 0.3.3")
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
    iced::window::icon::from_rgba(include_bytes!("../assets/app.rgba").to_vec(), 64, 64)
        .expect("embedded application icon must be valid RGBA")
}

fn update(state: &mut State, message: Message) -> Task<Message> {
    match message {
        Message::PollChanged(value) => state.poll_milliseconds = value,
        Message::LanguageChanged(language) => {
            state.language = language;
            state.status = if state.running {
                ui_text(language).running_status.to_string()
            } else {
                ui_text(language).ready.to_string()
            };
        }
        Message::ApiKeyChanged(exchange, value) => state.exchange_mut(exchange).api_key = value,
        Message::ApiSecretChanged(exchange, value) => {
            state.exchange_mut(exchange).api_secret = value
        }
        Message::PassphraseChanged(value) => state.okx.passphrase = value,
        Message::MarketChanged(exchange, index, market) => {
            if let Some(item) = state.exchange_mut(exchange).symbols.get_mut(index) {
                item.market = market;
            }
        }
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
            market: match exchange {
                ExchangeId::Binance => MarketChoice::Futures,
                ExchangeId::Okx => MarketChoice::Swap,
                ExchangeId::Bybit => MarketChoice::Linear,
            },
            symbol: String::new(),
            side: CancelSide::Both,
        }),
        Message::RemoveSymbol(exchange, index) => {
            let symbols = &mut state.exchange_mut(exchange).symbols;
            if index < symbols.len() {
                symbols.remove(index);
            }
        }
        Message::ClearCredentials(exchange) => {
            let form = state.exchange_mut(exchange);
            form.api_key.clear();
            form.api_secret.clear();
            form.passphrase.clear();
            state.status = format!(
                "{}: {}",
                exchange.title(),
                ui_text(state.language).credentials_cleared
            );
        }
        Message::Save => match state.save_config() {
            Ok(_) => {
                let t = ui_text(state.language);
                state.status = if state.running {
                    t.saved_running.to_string()
                } else {
                    format!("{}: {}", t.saved_prefix, state.config_path)
                };
            }
            Err(error) => {
                state.status = format!("{}: {error}", ui_text(state.language).save_failed)
            }
        },
        Message::Start => {
            if state.running {
                return Task::none();
            }
            let config = match state.save_config() {
                Ok(config) => config,
                Err(error) => {
                    state.status = format!(
                        "{}: {error}",
                        ui_text(state.language).failed_to_start
                    );
                    return Task::none();
                }
            };
            if !config.has_enabled_exchange() {
                let t = ui_text(state.language);
                state.status = format!("{}: {}", t.failed_to_start, t.configure_exchange);
                return Task::none();
            }
            state.running = true;
            state.status = ui_text(state.language).running_status.to_string();
            let task = Task::perform(engine::run(config), |result| {
                Message::EngineFinished(result.map_err(|error| format!("{error:#}")))
            });
            let (task, handle) = task.abortable();
            state.run_handle = Some(handle);
            return task;
        }
        Message::Stop => {
            if let Some(handle) = state.run_handle.take() {
                handle.abort();
            }
            state.running = false;
            state.status = ui_text(state.language).stopped.to_string();
        }
        Message::EngineFinished(result) => {
            state.run_handle = None;
            state.running = false;
            let t = ui_text(state.language);
            state.status = match result {
                Ok(()) => t.engine_stopped.to_string(),
                Err(error) => format!("{}: {error}", t.engine_error),
            };
        }
    }
    Task::none()
}

fn view(state: &State) -> Element<'_, Message> {
    let t = ui_text(state.language);
    let status_text = if state.running {
        t.running_badge
    } else {
        t.stopped_badge
    };

    let start = if state.running {
        button(text(t.start).size(BODY_SIZE))
            .padding(BUTTON_PADDING)
            .style(button::success)
    } else {
        button(text(t.start).size(BODY_SIZE))
            .padding(BUTTON_PADDING)
            .style(button::success)
            .on_press(Message::Start)
    };
    let stop = if state.running {
        button(text(t.stop).size(BODY_SIZE))
            .padding(BUTTON_PADDING)
            .style(button::danger)
            .on_press(Message::Stop)
    } else {
        button(text(t.stop).size(BODY_SIZE))
            .padding(BUTTON_PADDING)
            .style(button::danger)
    };

    let header = row![
        column![
            text("Limit Canceller 0.3.3").size(TITLE_SIZE),
            text(format!("{}: {}", t.config_label, state.config_path)).size(SMALL_SIZE),
        ]
        .spacing(2).width(Fill),
        text(status_text).size(BODY_SIZE),
        button(text(t.save).size(BODY_SIZE))
            .padding(BUTTON_PADDING)
            .style(button::primary)
            .on_press(Message::Save),
        start,
        stop,
        container("").width(Length::Fixed(5.0)),
    ]
    .spacing(7)
    .align_y(iced::alignment::Vertical::Center)
    .width(Fill);

    let settings = container(
        row![
            text(t.polling).size(BODY_SIZE).width(Length::Fixed(78.0)),
            text_input("100", &state.poll_milliseconds)
                .on_input(Message::PollChanged)
                .size(BODY_SIZE)
                .padding(FIELD_PADDING)
                .width(Length::Fixed(92.0)),
            text(t.language).size(BODY_SIZE),
            pick_list(LANGUAGES, Some(state.language), Message::LanguageChanged)
                .text_size(BODY_SIZE)
                .padding(FIELD_PADDING)
                .width(Length::Fixed(100.0)),
            text(t.changes_running).size(SMALL_SIZE),
        ]
        .spacing(8)
        .align_y(iced::alignment::Vertical::Center),
    )
    .padding(8)
    .width(Fill)
    .style(container::rounded_box);

    // Keep the static header/settings outside the scrolling clip layer.
    // With iced 0.14 + tiny-skia this substantially reduces redraw work while
    // the wheel is moving because fewer text runs are processed by clipping.
    let body = column![
        exchange_card(state, ExchangeId::Binance),
        exchange_card(state, ExchangeId::Okx),
        exchange_card(state, ExchangeId::Bybit),
        container(text(&state.status).size(SMALL_SIZE))
            .padding(8)
            .width(Fill)
            .style(container::bordered_box),
    ]
    .spacing(8)
    .width(Fill);

    column![header, settings, scrollable(body).height(Fill)]
        .spacing(8)
        .padding(10)
        .width(Fill)
        .height(Fill)
        .into()
}

fn exchange_card(state: &State, exchange: ExchangeId) -> Element<'_, Message> {
    let t = ui_text(state.language);
    let form = state.exchange(exchange);
    let enabled = !form.api_key.trim().is_empty() && !form.api_secret.trim().is_empty();
    let state_label = if enabled {
        t.enabled
    } else {
        t.disabled_empty_keys
    };

    let mut symbols: Column<'_, Message> = Column::new().spacing(5).width(Fill);
    if form.symbols.is_empty() {
        symbols = symbols.push(text(t.no_tickers).size(SMALL_SIZE));
    } else {
        for (index, item) in form.symbols.iter().enumerate() {
            let next_market = item.market.toggled(exchange);
            let market = button(text(item.market.as_str()).size(BODY_SIZE))
                .padding(BUTTON_PADDING)
                .style(button::background)
                .width(Length::Fixed(112.0))
                .on_press(Message::MarketChanged(exchange, index, next_market));
            let symbol_input = text_input("BTC/USDT", &item.symbol)
                .on_input(move |value| Message::SymbolChanged(exchange, index, value))
                .size(BODY_SIZE)
                .padding(FIELD_PADDING)
                .width(Length::FillPortion(5));
            let side = pick_list(
                SideChoice::options(state.language),
                Some(SideChoice::from_cancel_side(item.side, state.language)),
                move |value| Message::SideChanged(exchange, index, value.cancel_side()),
            )
            .text_size(BODY_SIZE)
            .padding(FIELD_PADDING)
            .width(Length::FillPortion(2));
            let remove = button(text(t.remove).size(BODY_SIZE))
                .padding(BUTTON_PADDING)
                .style(button::danger)
                .on_press(Message::RemoveSymbol(exchange, index));
            symbols = symbols.push(
                row![market, symbol_input, side, remove]
                    .spacing(6)
                    .align_y(iced::alignment::Vertical::Center)
                    .width(Fill),
            );
        }
    }

    let key_input = text_input(t.api_key, &form.api_key)
        .on_input(move |value| Message::ApiKeyChanged(exchange, value))
        .size(BODY_SIZE)
        .padding(FIELD_PADDING)
        .width(Length::FillPortion(1));
    let secret_input = text_input(t.api_secret, &form.api_secret)
        .secure(true)
        .on_input(move |value| Message::ApiSecretChanged(exchange, value))
        .size(BODY_SIZE)
        .padding(FIELD_PADDING)
        .width(Length::FillPortion(1));

    let mut credentials: Row<'_, Message> = row![key_input, secret_input].spacing(6).width(Fill);
    if exchange == ExchangeId::Okx {
        credentials = credentials.push(
            text_input(t.passphrase, &form.passphrase)
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
        button(text(t.clear_keys).size(BODY_SIZE))
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
            text(t.tickers_market_cancel_side).size(BODY_SIZE),
            symbols,
            button(text(t.add_ticker).size(BODY_SIZE))
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
