//! eframe/egui GUI for managing config.toml and controlling the cancellation engine.
//!
//! This experimental frontend intentionally uses the lightweight Glow/OpenGL
//! renderer instead of wgpu. The trading engine and configuration format are
//! unchanged from the iced build.

use eframe::egui;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use crate::config::{AppConfig, BinanceConfig, BybitConfig, OkxConfig, SymbolConfig, TigerXConfig, TigerXSymbolConfig};
use crate::engine;
use crate::exchange::CancelSide;
use crate::symbol::Symbol;

const WINDOW_WIDTH: f32 = 820.0;
const WINDOW_HEIGHT: f32 = 620.0;
const WINDOW_MIN_WIDTH: f32 = 680.0;
const WINDOW_MIN_HEIGHT: f32 = 480.0;

const ROW_HEIGHT: f32 = 25.0;
const MARKET_WIDTH: f32 = 88.0;
const UNDERLYING_WIDTH: f32 = 92.0;
const SIDE_WIDTH: f32 = 112.0;
const REMOVE_WIDTH: f32 = 72.0;
const AUTOSAVE_DELAY: Duration = Duration::from_millis(500);

// Muted dark palette close to the previous iced build.
const APP_BG: egui::Color32 = egui::Color32::from_rgb(24, 26, 31);
const PANEL_BG: egui::Color32 = egui::Color32::from_rgb(31, 34, 40);
const INPUT_BG: egui::Color32 = egui::Color32::from_rgb(37, 40, 48);
const BORDER: egui::Color32 = egui::Color32::from_rgb(55, 61, 72);
const TEXT: egui::Color32 = egui::Color32::from_rgb(225, 229, 236);
const TEXT_WEAK: egui::Color32 = egui::Color32::from_rgb(145, 152, 164);
const ACCENT: egui::Color32 = egui::Color32::from_rgb(73, 105, 174);
const ACCENT_HOVER: egui::Color32 = egui::Color32::from_rgb(88, 123, 198);

const MARKET_BUTTON: egui::Color32 = egui::Color32::from_rgb(61, 88, 146);
const START_BUTTON: egui::Color32 = egui::Color32::from_rgb(46, 122, 80);
const STOP_BUTTON: egui::Color32 = egui::Color32::from_rgb(145, 61, 67);
const REMOVE_BUTTON: egui::Color32 = egui::Color32::from_rgb(119, 52, 57);
const SECONDARY_BUTTON: egui::Color32 = egui::Color32::from_rgb(48, 53, 63);

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

    fn label(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::Russian => "Русский",
        }
    }

}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct HotkeyBinding {
    label: String,
    virtual_key: u32,
}

impl HotkeyBinding {
    fn from_saved(value: &str) -> Option<Self> {
        let normalized = value.trim();
        if normalized.is_empty() {
            return None;
        }
        hotkey_from_name(normalized)
    }
}

fn hotkey_from_name(raw: &str) -> Option<HotkeyBinding> {
    let name = raw.trim();
    let upper = name.to_ascii_uppercase();

    if upper.len() == 1 {
        let byte = upper.as_bytes()[0];
        if byte.is_ascii_alphanumeric() {
            return Some(HotkeyBinding {
                label: upper,
                virtual_key: byte as u32,
            });
        }
    }

    if let Some(number) = upper.strip_prefix('F').and_then(|value| value.parse::<u32>().ok()) {
        if (1..=24).contains(&number) {
            return Some(HotkeyBinding {
                label: format!("F{number}"),
                virtual_key: 0x70 + number - 1,
            });
        }
    }

    let (label, virtual_key) = match upper.as_str() {
        "NUM0" => ("0", 0x30),
        "NUM1" => ("1", 0x31),
        "NUM2" => ("2", 0x32),
        "NUM3" => ("3", 0x33),
        "NUM4" => ("4", 0x34),
        "NUM5" => ("5", 0x35),
        "NUM6" => ("6", 0x36),
        "NUM7" => ("7", 0x37),
        "NUM8" => ("8", 0x38),
        "NUM9" => ("9", 0x39),
        "SPACE" => ("Space", 0x20),
        "ENTER" => ("Enter", 0x0D),
        "TAB" => ("Tab", 0x09),
        "BACKSPACE" => ("Backspace", 0x08),
        "INSERT" => ("Insert", 0x2D),
        "DELETE" => ("Delete", 0x2E),
        "HOME" => ("Home", 0x24),
        "END" => ("End", 0x23),
        "PAGEUP" => ("PageUp", 0x21),
        "PAGEDOWN" => ("PageDown", 0x22),
        "ARROWLEFT" => ("Left", 0x25),
        "ARROWRIGHT" => ("Right", 0x27),
        "ARROWUP" => ("Up", 0x26),
        "ARROWDOWN" => ("Down", 0x28),
        "COMMA" => (",", 0xBC),
        "PERIOD" => (".", 0xBE),
        "SLASH" => ("/", 0xBF),
        "BACKSLASH" => ("\\", 0xDC),
        "SEMICOLON" => (";", 0xBA),
        "MINUS" => ("-", 0xBD),
        "PLUS" | "EQUALS" => ("=", 0xBB),
        "OPENBRACKET" => ("[", 0xDB),
        "CLOSEBRACKET" => ("]", 0xDD),
        "BACKTICK" => ("`", 0xC0),
        "QUOTE" => ("'", 0xDE),
        "BROWSERBACK" => ("BrowserBack", 0xA6),
        "SHIFTLEFT" | "SHIFTRIGHT" => ("Shift", 0x10),
        "CONTROLLEFT" | "CONTROLRIGHT" => ("Ctrl", 0x11),
        "ALTLEFT" | "ALTRIGHT" => ("Alt", 0x12),
        "SUPERLEFT" => ("WinLeft", 0x5B),
        "SUPERRIGHT" => ("WinRight", 0x5C),
        "INTLBACKSLASH" => ("Intl\\", 0xE2),
        _ => return None,
    };

    Some(HotkeyBinding {
        label: label.to_string(),
        virtual_key,
    })
}

#[derive(Debug)]
enum HotkeyEvent {
    Toggle,
    RegistrationFailed(u32),
}

#[cfg(target_os = "windows")]
struct HotkeyManager {
    thread_id: u32,
    events: Receiver<HotkeyEvent>,
    thread: Option<std::thread::JoinHandle<()>>,
}

#[cfg(target_os = "windows")]
impl HotkeyManager {
    const HOTKEY_ID: i32 = 1;
    const WM_HOTKEY: u32 = 0x0312;
    const WM_UPDATE_HOTKEY: u32 = 0x8000 + 41;
    const WM_SHUTDOWN_HOTKEY: u32 = 0x8000 + 42;
    const MOD_NOREPEAT: u32 = 0x4000;

    fn start(ctx: &egui::Context) -> Result<Self, String> {
        use windows_sys::Win32::System::Threading::GetCurrentThreadId;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetMessageW, PeekMessageW, MSG,
        };
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
            RegisterHotKey,
            UnregisterHotKey,
        };

        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let (event_tx, event_rx) = mpsc::channel();
        let repaint = ctx.clone();

        let thread = std::thread::Builder::new()
            .name("limit-canceller-hotkey".to_string())
            .spawn(move || unsafe {
                let mut message: MSG = std::mem::zeroed();
                // Create this thread's Windows message queue before publishing its ID.
                let _ = PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, 0);
                let thread_id = GetCurrentThreadId();
                let _ = ready_tx.send(thread_id);

                let mut registered = false;
                loop {
                    let result = GetMessageW(&mut message, std::ptr::null_mut(), 0, 0);
                    if result <= 0 {
                        break;
                    }

                    match message.message {
                        Self::WM_HOTKEY => {
                            let _ = event_tx.send(HotkeyEvent::Toggle);
                            repaint.request_repaint();
                        }
                        Self::WM_UPDATE_HOTKEY => {
                            if registered {
                                let _ = UnregisterHotKey(std::ptr::null_mut(), Self::HOTKEY_ID);
                                registered = false;
                            }

                            let virtual_key = message.wParam as u32;
                            if virtual_key != 0 {
                                let ok = RegisterHotKey(
                                    std::ptr::null_mut(),
                                    Self::HOTKEY_ID,
                                    Self::MOD_NOREPEAT,
                                    virtual_key,
                                );
                                if ok == 0 {
                                    let _ = event_tx.send(HotkeyEvent::RegistrationFailed(virtual_key));
                                    repaint.request_repaint();
                                } else {
                                    registered = true;
                                }
                            }
                        }
                        Self::WM_SHUTDOWN_HOTKEY => break,
                        _ => {}
                    }
                }

                if registered {
                    let _ = UnregisterHotKey(std::ptr::null_mut(), Self::HOTKEY_ID);
                }
            })
            .map_err(|error| format!("failed to start hotkey thread: {error}"))?;

        let thread_id = ready_rx
            .recv()
            .map_err(|_| "hotkey thread ended during startup".to_string())?;

        Ok(Self {
            thread_id,
            events: event_rx,
            thread: Some(thread),
        })
    }

    fn set_binding(&self, binding: Option<&HotkeyBinding>) {
        use windows_sys::Win32::UI::WindowsAndMessaging::PostThreadMessageW;
        let virtual_key = binding.map_or(0, |binding| binding.virtual_key);
        unsafe {
            let _ = PostThreadMessageW(
                self.thread_id,
                Self::WM_UPDATE_HOTKEY,
                virtual_key as usize,
                0,
            );
        }
    }

    fn drain_events(&self) -> Vec<HotkeyEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.events.try_recv() {
            events.push(event);
        }
        events
    }
}

#[cfg(target_os = "windows")]
impl Drop for HotkeyManager {
    fn drop(&mut self) {
        use windows_sys::Win32::UI::WindowsAndMessaging::PostThreadMessageW;
        unsafe {
            let _ = PostThreadMessageW(self.thread_id, Self::WM_SHUTDOWN_HOTKEY, 0, 0);
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(not(target_os = "windows"))]
struct HotkeyManager;

#[cfg(not(target_os = "windows"))]
impl HotkeyManager {
    fn start(_ctx: &egui::Context) -> Result<Self, String> {
        Ok(Self)
    }

    fn set_binding(&self, _binding: Option<&HotkeyBinding>) {}

    fn drain_events(&self) -> Vec<HotkeyEvent> {
        Vec::new()
    }
}

#[derive(Clone, Copy)]
struct UiText {
    ready: &'static str,
    config_label: &'static str,
    running_badge: &'static str,
    stopped_badge: &'static str,
    start: &'static str,
    stop: &'static str,
    hotkey: &'static str,
    press_key: &'static str,
    hotkey_cleared: &'static str,
    hotkey_set: &'static str,
    hotkey_failed: &'static str,
    polling: &'static str,
    language: &'static str,
    changes_running: &'static str,
    enabled: &'static str,
    idle_no_tickers: &'static str,
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
    autosaved_running: &'static str,
    autosaved_prefix: &'static str,
    autosave_failed: &'static str,
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
            ready: "Ready. Autosave is on; changes are written to config.toml automatically.",
            config_label: "Config",
            running_badge: "· RUNNING",
            stopped_badge: "· STOPPED",
            start: "Start",
            stop: "Stop",
            hotkey: "Hotkey",
            press_key: "Press key...",
            hotkey_cleared: "Hotkey cleared.",
            hotkey_set: "Hotkey set",
            hotkey_failed: "Failed to register global hotkey",
            polling: "Polling, ms",
            language: "Language",
            changes_running: "Changes are autosaved; RUNNING uses the current snapshot until Stop -> Start.",
            enabled: "enabled",
            idle_no_tickers: "idle (no tickers)",
            disabled_empty_keys: "disabled (empty keys)",
            no_tickers: "No tickers configured. Add a ticker below.",
            api_key: "API key",
            api_secret: "API secret",
            passphrase: "Passphrase",
            clear_keys: "Clear keys",
            tickers_market_cancel_side: "Tickers / market / cancel side",
            add_ticker: "+ Add ticker",
            remove: "Remove",
            credentials_cleared: "credentials cleared. Autosave will write the changes shortly.",
            autosaved_running: "Autosaved. The running engine still uses the previous configuration snapshot; Stop and Start to apply changes.",
            autosaved_prefix: "Autosaved",
            autosave_failed: "Autosave pending",
            failed_to_start: "Failed to start",
            configure_exchange: "configure API credentials and at least one ticker on any exchange.",
            running_status: "Running. Stop and start again to apply new settings.",
            stopped: "Stopped.",
            engine_stopped: "Engine stopped.",
            engine_error: "Engine stopped with an error",
            failed_load_config: "Failed to load config.toml",
        },
        Language::Russian => UiText {
            ready: "Готово. Автосохранение включено; изменения автоматически записываются в config.toml.",
            config_label: "Конфиг",
            running_badge: "· РАБОТАЕТ",
            stopped_badge: "· ОСТАНОВЛЕНО",
            start: "Старт",
            stop: "Стоп",
            hotkey: "Клавиша",
            press_key: "Нажмите клавишу...",
            hotkey_cleared: "Горячая клавиша очищена.",
            hotkey_set: "Горячая клавиша",
            hotkey_failed: "Не удалось зарегистрировать глобальную клавишу",
            polling: "Опрос, мс",
            language: "Язык",
            changes_running: "Изменения сохраняются автоматически; во время работы они применятся после «Стоп» -> «Старт».",
            enabled: "включена",
            idle_no_tickers: "ожидает (нет тикеров)",
            disabled_empty_keys: "выключена (пустые ключи)",
            no_tickers: "Нет тикеров. Добавьте тикер ниже.",
            api_key: "API-ключ",
            api_secret: "API-секрет",
            passphrase: "Парольная фраза",
            clear_keys: "Очистить ключи",
            tickers_market_cancel_side: "Тикеры / рынок / сторона снятия",
            add_ticker: "+ Добавить тикер",
            remove: "Удалить",
            credentials_cleared: "ключи API очищены. Автосохранение запишет изменения через мгновение.",
            autosaved_running: "Автосохранено. Запущенный алгоритм продолжает работать с предыдущей копией настроек; нажмите «Стоп», затем «Старт», чтобы применить изменения.",
            autosaved_prefix: "Автосохранено",
            autosave_failed: "Автосохранение ожидает корректных данных",
            failed_to_start: "Не удалось запустить",
            configure_exchange: "укажите API-ключи и хотя бы один тикер на любой бирже.",
            running_status: "Запущено. Чтобы применить новые настройки, остановите и запустите снова.",
            stopped: "Остановлено.",
            engine_stopped: "Алгоритм остановлен.",
            engine_error: "Алгоритм остановлен с ошибкой",
            failed_load_config: "Не удалось загрузить config.toml",
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ExchangeId {
    Binance,
    Okx,
    Bybit,
    Tigerx,
}

impl ExchangeId {
    fn title(self) -> &'static str {
        match self {
            Self::Binance => "Binance",
            Self::Okx => "OKX",
            Self::Bybit => "Bybit",
            Self::Tigerx => "TigerX",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MarketChoice {
    Spot,
    Futures,
    Swap,
    Linear,
    Perp,
}

impl MarketChoice {
    fn as_str(self) -> &'static str {
        match self {
            Self::Spot => "spot",
            Self::Futures => "futures",
            Self::Swap => "swap",
            Self::Linear => "linear",
            Self::Perp => "perp",
        }
    }

    fn parse(value: &str, exchange: ExchangeId) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "futures" if exchange == ExchangeId::Binance => Self::Futures,
            "swap" if exchange == ExchangeId::Okx => Self::Swap,
            "linear" if exchange == ExchangeId::Bybit => Self::Linear,
            "perp" if exchange == ExchangeId::Tigerx => Self::Perp,
            _ => Self::Spot,
        }
    }

    fn futures_default(exchange: ExchangeId) -> Self {
        match exchange {
            ExchangeId::Binance => Self::Futures,
            ExchangeId::Okx => Self::Swap,
            ExchangeId::Bybit => Self::Linear,
            ExchangeId::Tigerx => Self::Perp,
        }
    }

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
            ExchangeId::Tigerx => match self {
                Self::Perp => Self::Spot,
                _ => Self::Perp,
            },
        }
    }
}

/// Underlying exchange of a TigerX symbol (`BINANCE_*` or `OKX_*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UnderlyingExchange {
    Binance,
    Okx,
}

impl UnderlyingExchange {
    fn as_str(self) -> &'static str {
        match self {
            Self::Binance => "binance",
            Self::Okx => "okx",
        }
    }

    fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "okx" => Self::Okx,
            _ => Self::Binance,
        }
    }

    fn toggled(self) -> Self {
        match self {
            Self::Binance => Self::Okx,
            Self::Okx => Self::Binance,
        }
    }
}

#[derive(Debug, Clone)]
struct SymbolForm {
    market: MarketChoice,
    /// Underlying exchange of the TigerX symbol; only TigerX rows use it.
    underlying: UnderlyingExchange,
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
    fn blank() -> Self {
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
                underlying: UnderlyingExchange::Binance,
                symbol: item.symbol.clone(),
                side: CancelSide::parse(&item.side).unwrap_or(CancelSide::Both),
            })
            .collect()
    }

    fn tigerx_symbols_from_config(symbols: &[TigerXSymbolConfig]) -> Vec<SymbolForm> {
        symbols
            .iter()
            .map(|item| SymbolForm {
                market: MarketChoice::parse(&item.market, ExchangeId::Tigerx),
                underlying: UnderlyingExchange::parse(&item.exchange),
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
            None => Self::blank(),
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
            None => Self::blank(),
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
            None => Self::blank(),
        }
    }

    fn tigerx(config: Option<&TigerXConfig>) -> Self {
        match config {
            Some(c) => Self {
                api_key: c.api_key.clone(),
                api_secret: c.api_secret.clone(),
                passphrase: String::new(),
                symbols: Self::tigerx_symbols_from_config(&c.symbols),
            },
            None => Self::blank(),
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

    fn detailed_tigerx_symbols(&self) -> Vec<TigerXSymbolConfig> {
        self.symbols
            .iter()
            .map(|item| TigerXSymbolConfig {
                exchange: item.underlying.as_str().to_string(),
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
    tigerx: ExchangeForm,
    hotkey: Option<HotkeyBinding>,
    hotkey_capture: bool,
    hotkey_manager: Option<HotkeyManager>,
    status: String,
    running: bool,
    runtime: Option<tokio::runtime::Runtime>,
    run_handle: Option<tokio::task::JoinHandle<()>>,
    engine_rx: Option<Receiver<Result<(), String>>>,
    last_config_fingerprint: u64,
    autosave_deadline: Option<Instant>,
}

impl State {
    fn boot(cc: &eframe::CreationContext<'_>) -> Self {
        // The window exists but is still hidden while the app creator runs.
        // Apply native DWM colors now so Windows never exposes a white title bar.
        configure_native_title_bar(cc);
        configure_egui(&cc.egui_ctx);

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("failed to create Tokio runtime");

        let config_path = std::env::args()
            .nth(1)
            .unwrap_or_else(|| "config.toml".to_string());

        let mut state = match AppConfig::load_or_default(&config_path) {
            Ok(config) => Self::from_config(config_path, config, runtime),
            Err(error) => {
                let language = Language::English;
                let mut state = Self::from_config(
                    config_path,
                    AppConfig::default(),
                    runtime,
                );
                state.language = language;
                state.status = format!("{}: {error:#}", ui_text(language).failed_load_config);
                state
            }
        };
        state.initialize_hotkey(&cc.egui_ctx);
        state
    }

    fn from_config(
        config_path: String,
        config: AppConfig,
        runtime: tokio::runtime::Runtime,
    ) -> Self {
        let language = Language::parse(config.ui_language.as_deref());
        let poll = config
            .poll_milliseconds
            .or_else(|| config.poll_seconds.and_then(|seconds| seconds.checked_mul(1000)))
            .unwrap_or(5000);
        let hotkey = config
            .backend_hotkey
            .as_deref()
            .and_then(HotkeyBinding::from_saved);

        let mut state = Self {
            config_path,
            poll_milliseconds: poll.to_string(),
            language,
            binance: ExchangeForm::binance(config.binance.as_ref()),
            okx: ExchangeForm::okx(config.okx.as_ref()),
            bybit: ExchangeForm::bybit(config.bybit.as_ref()),
            tigerx: ExchangeForm::tigerx(config.tigerx.as_ref()),
            hotkey,
            hotkey_capture: false,
            hotkey_manager: None,
            status: ui_text(language).ready.to_string(),
            running: false,
            runtime: Some(runtime),
            run_handle: None,
            engine_rx: None,
            last_config_fingerprint: 0,
            autosave_deadline: None,
        };
        state.last_config_fingerprint = state.config_fingerprint();
        state
    }

    fn exchange_mut(&mut self, id: ExchangeId) -> &mut ExchangeForm {
        match id {
            ExchangeId::Binance => &mut self.binance,
            ExchangeId::Okx => &mut self.okx,
            ExchangeId::Bybit => &mut self.bybit,
            ExchangeId::Tigerx => &mut self.tigerx,
        }
    }

    fn build_config_from_forms(
        &self,
        binance: &ExchangeForm,
        okx: &ExchangeForm,
        bybit: &ExchangeForm,
        tigerx: &ExchangeForm,
    ) -> Result<AppConfig, String> {
        let poll = self
            .poll_milliseconds
            .trim()
            .parse::<u64>()
            .map_err(|_| "poll_milliseconds must be an integer".to_string())?;

        let config = AppConfig {
            poll_milliseconds: Some(poll),
            poll_seconds: None,
            ui_language: Some(self.language.code().to_string()),
            backend_hotkey: self.hotkey.as_ref().map(|hotkey| hotkey.label.clone()),
            binance: Some(BinanceConfig {
                api_key: binance.api_key.trim().to_string(),
                api_secret: binance.api_secret.trim().to_string(),
                symbols: binance.detailed_symbols(),
            }),
            okx: Some(OkxConfig {
                api_key: okx.api_key.trim().to_string(),
                api_secret: okx.api_secret.trim().to_string(),
                passphrase: okx.passphrase.trim().to_string(),
                symbols: okx.detailed_symbols(),
            }),
            bybit: Some(BybitConfig {
                api_key: bybit.api_key.trim().to_string(),
                api_secret: bybit.api_secret.trim().to_string(),
                symbols: bybit.detailed_symbols(),
            }),
            tigerx: Some(TigerXConfig {
                api_key: tigerx.api_key.trim().to_string(),
                api_secret: tigerx.api_secret.trim().to_string(),
                symbols: tigerx.detailed_tigerx_symbols(),
            }),
        };

        config.validate().map_err(|error| format!("{error:#}"))?;
        Ok(config)
    }

    fn build_config(&self) -> Result<AppConfig, String> {
        self.build_config_from_forms(&self.binance, &self.okx, &self.bybit, &self.tigerx)
    }

    /// Build a normalized config without mutating text currently being edited.
    fn build_autosave_config(&self) -> Result<AppConfig, String> {
        let mut binance = self.binance.clone();
        let mut okx = self.okx.clone();
        let mut bybit = self.bybit.clone();
        let mut tigerx = self.tigerx.clone();

        binance.normalize_symbols()?;
        okx.normalize_symbols()?;
        bybit.normalize_symbols()?;
        tigerx.normalize_symbols()?;

        self.build_config_from_forms(&binance, &okx, &bybit, &tigerx)
    }

    fn config_fingerprint(&self) -> u64 {
        fn hash_form(form: &ExchangeForm, hasher: &mut DefaultHasher) {
            form.api_key.hash(hasher);
            form.api_secret.hash(hasher);
            form.passphrase.hash(hasher);
            form.symbols.len().hash(hasher);
            for item in &form.symbols {
                item.market.as_str().hash(hasher);
                item.underlying.as_str().hash(hasher);
                item.symbol.hash(hasher);
                item.side.as_str().hash(hasher);
            }
        }

        let mut hasher = DefaultHasher::new();
        self.poll_milliseconds.hash(&mut hasher);
        self.language.code().hash(&mut hasher);
        self.hotkey.as_ref().map(|hotkey| hotkey.label.as_str()).hash(&mut hasher);
        hash_form(&self.binance, &mut hasher);
        hash_form(&self.okx, &mut hasher);
        hash_form(&self.bybit, &mut hasher);
        hash_form(&self.tigerx, &mut hasher);
        hasher.finish()
    }

    fn mark_config_synced(&mut self) {
        self.last_config_fingerprint = self.config_fingerprint();
        self.autosave_deadline = None;
    }

    fn observe_config_changes(&mut self, ctx: &egui::Context) {
        let fingerprint = self.config_fingerprint();
        if fingerprint == self.last_config_fingerprint {
            return;
        }

        self.last_config_fingerprint = fingerprint;
        self.autosave_deadline = Some(Instant::now() + AUTOSAVE_DELAY);
        ctx.request_repaint_after(AUTOSAVE_DELAY);
    }

    fn autosave_if_due(&mut self, ctx: &egui::Context) {
        let Some(deadline) = self.autosave_deadline else {
            return;
        };

        let now = Instant::now();
        if now < deadline {
            ctx.request_repaint_after(deadline.saturating_duration_since(now));
            return;
        }

        self.autosave_deadline = None;
        let t = ui_text(self.language);
        match self
            .build_autosave_config()
            .and_then(|config| config.save(&self.config_path).map_err(|error| format!("{error:#}")))
        {
            Ok(()) => {
                self.status = if self.running {
                    t.autosaved_running.to_string()
                } else {
                    format!("{}: {}", t.autosaved_prefix, self.config_path)
                };
            }
            Err(error) => {
                self.status = format!("{}: {error}", t.autosave_failed);
            }
        }
        ctx.request_repaint();
    }

    fn autosave_on_exit(&self) {
        if self.autosave_deadline.is_none() {
            return;
        }

        let _ = self
            .build_autosave_config()
            .and_then(|config| config.save(&self.config_path).map_err(|error| format!("{error:#}")));
    }

    fn save_config(&mut self) -> Result<AppConfig, String> {
        self.binance.normalize_symbols()?;
        self.okx.normalize_symbols()?;
        self.bybit.normalize_symbols()?;
        self.tigerx.normalize_symbols()?;

        let config = self.build_config()?;
        config
            .save(&self.config_path)
            .map_err(|error| format!("{error:#}"))?;
        Ok(config)
    }

    fn initialize_hotkey(&mut self, ctx: &egui::Context) {
        match HotkeyManager::start(ctx) {
            Ok(manager) => {
                manager.set_binding(self.hotkey.as_ref());
                self.hotkey_manager = Some(manager);
            }
            Err(error) => {
                self.status = format!("{}: {error}", ui_text(self.language).hotkey_failed);
            }
        }
    }

    fn begin_hotkey_capture(&mut self) {
        self.hotkey_capture = true;
        if let Some(manager) = self.hotkey_manager.as_ref() {
            // Disable the old registration while the next key is being captured,
            // otherwise pressing the existing hotkey would toggle the engine.
            manager.set_binding(None);
        }
        self.status = ui_text(self.language).press_key.to_string();
    }

    fn capture_hotkey_input(&mut self, ctx: &egui::Context) {
        if !self.hotkey_capture {
            return;
        }

        let key_name = ctx.input(|input| {
            input.events.iter().find_map(|event| match event {
                egui::Event::Key {
                    key,
                    pressed: true,
                    repeat: false,
                    ..
                } => Some(format!("{key:?}")),
                _ => None,
            })
        });

        let Some(key_name) = key_name else {
            return;
        };

        let t = ui_text(self.language);
        if key_name.eq_ignore_ascii_case("Escape") {
            self.hotkey = None;
            self.hotkey_capture = false;
            if let Some(manager) = self.hotkey_manager.as_ref() {
                manager.set_binding(None);
            }
            self.status = t.hotkey_cleared.to_string();
            return;
        }

        let Some(binding) = hotkey_from_name(&key_name) else {
            self.status = format!("{}: {key_name}", t.hotkey_failed);
            return;
        };

        if let Some(manager) = self.hotkey_manager.as_ref() {
            manager.set_binding(Some(&binding));
        }
        self.status = format!("{}: {}", t.hotkey_set, binding.label);
        self.hotkey = Some(binding);
        self.hotkey_capture = false;
    }

    fn poll_hotkey_events(&mut self, ctx: &egui::Context) {
        let events = self
            .hotkey_manager
            .as_ref()
            .map(HotkeyManager::drain_events)
            .unwrap_or_default();

        for event in events {
            match event {
                HotkeyEvent::Toggle => {
                    if self.hotkey_capture {
                        continue;
                    }
                    if self.running {
                        self.stop_clicked();
                    } else {
                        self.start_clicked(ctx);
                    }
                }
                HotkeyEvent::RegistrationFailed(virtual_key) => {
                    if self
                        .hotkey
                        .as_ref()
                        .is_some_and(|binding| binding.virtual_key == virtual_key)
                    {
                        let label = self
                            .hotkey
                            .as_ref()
                            .map(|binding| binding.label.clone())
                            .unwrap_or_default();
                        self.hotkey = None;
                        self.hotkey_capture = false;
                        self.status = format!(
                            "{}: {}",
                            ui_text(self.language).hotkey_failed,
                            label
                        );
                    }
                }
            }
        }
    }

    fn start_clicked(&mut self, ctx: &egui::Context) {
        if self.running {
            return;
        }

        let t = ui_text(self.language);
        let config = match self.save_config() {
            Ok(config) => {
                self.mark_config_synced();
                config
            }
            Err(error) => {
                self.status = format!("{}: {error}", t.failed_to_start);
                return;
            }
        };

        if !config.has_runnable_exchange() {
            self.status = format!("{}: {}", t.failed_to_start, t.configure_exchange);
            return;
        }

        let Some(runtime) = self.runtime.as_ref() else {
            self.status = format!("{}: Tokio runtime is unavailable", t.failed_to_start);
            return;
        };

        let (tx, rx) = mpsc::channel();
        let repaint = ctx.clone();
        let handle = runtime.spawn(async move {
            let result = engine::run(config)
                .await
                .map_err(|error| format!("{error:#}"));
            let _ = tx.send(result);
            repaint.request_repaint();
        });

        self.running = true;
        self.status = t.running_status.to_string();
        self.engine_rx = Some(rx);
        self.run_handle = Some(handle);
    }

    fn stop_clicked(&mut self) {
        if let Some(handle) = self.run_handle.take() {
            handle.abort();
        }
        self.engine_rx = None;
        self.running = false;
        self.status = ui_text(self.language).stopped.to_string();
    }

    fn poll_engine_result(&mut self) {
        let received = match self.engine_rx.as_ref() {
            Some(rx) => match rx.try_recv() {
                Ok(result) => Some(result),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => Some(Err(
                    "engine task ended without returning a result".to_string(),
                )),
            },
            None => None,
        };

        if let Some(result) = received {
            self.engine_rx = None;
            self.run_handle = None;
            self.running = false;
            let t = ui_text(self.language);
            self.status = match result {
                Ok(()) => t.engine_stopped.to_string(),
                Err(error) => format!("{}: {error}", t.engine_error),
            };
        }
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        let t = ui_text(self.language);
        let running = self.running;
        let mut start = false;
        let mut stop = false;
        let mut choose_hotkey = false;

        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(egui::RichText::new("Limit Canceller 0.4.3").size(20.0).strong());
                ui.label(
                    egui::RichText::new(format!("{}: {}", t.config_label, self.config_path))
                        .size(10.0)
                        .weak(),
                );
            });

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                stop = ui
                    .add_enabled(
                        running,
                        egui::Button::new(t.stop)
                            .fill(STOP_BUTTON)
                            .min_size(egui::vec2(58.0, 26.0)),
                    )
                    .clicked();
                start = ui
                    .add_enabled(
                        !running,
                        egui::Button::new(t.start)
                            .fill(START_BUTTON)
                            .min_size(egui::vec2(58.0, 26.0)),
                    )
                    .clicked();

                let hotkey_text = if self.hotkey_capture {
                    t.press_key.to_string()
                } else {
                    format!(
                        "{}: {}",
                        t.hotkey,
                        self.hotkey
                            .as_ref()
                            .map(|hotkey| hotkey.label.as_str())
                            .unwrap_or("—")
                    )
                };
                choose_hotkey = ui
                    .add(
                        egui::Button::new(hotkey_text)
                            .fill(SECONDARY_BUTTON)
                            .min_size(egui::vec2(112.0, 26.0)),
                    )
                    .clicked();

                let badge = if running {
                    egui::RichText::new(t.running_badge)
                        .size(11.0)
                        .color(egui::Color32::from_rgb(95, 205, 135))
                } else {
                    egui::RichText::new(t.stopped_badge)
                        .size(11.0)
                        .color(TEXT_WEAK)
                };
                ui.label(badge);
            });
        });

        if choose_hotkey {
            self.begin_hotkey_capture();
        }
        if start {
            self.start_clicked(ui.ctx());
        }
        if stop {
            self.stop_clicked();
        }
    }

    fn settings(&mut self, ui: &mut egui::Ui) {
        let t = ui_text(self.language);
        let previous_language = self.language;

        egui::Frame::group(ui.style()).fill(PANEL_BG).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(t.polling).size(11.0));
                ui.add_sized(
                    [92.0, ROW_HEIGHT],
                    egui::TextEdit::singleline(&mut self.poll_milliseconds).hint_text("100"),
                );
                ui.add_space(4.0);
                ui.label(egui::RichText::new(t.language).size(11.0));
                egui::ComboBox::from_id_salt("ui-language")
                    .selected_text(self.language.label())
                    .width(100.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.language, Language::English, "English");
                        ui.selectable_value(&mut self.language, Language::Russian, "Русский");
                    });
                ui.add_space(6.0);
                ui.label(egui::RichText::new(t.changes_running).size(10.0).weak());
            });
        });

        if self.language != previous_language {
            self.status = if self.running {
                ui_text(self.language).running_status.to_string()
            } else {
                ui_text(self.language).ready.to_string()
            };
        }
    }

    fn exchange_card(&mut self, ui: &mut egui::Ui, exchange: ExchangeId) {
        let language = self.language;
        let t = ui_text(language);
        let mut credentials_cleared = false;

        {
            let form = self.exchange_mut(exchange);
            egui::Frame::group(ui.style()).fill(PANEL_BG).show(ui, |ui| {
                ui.set_min_width(ui.available_width());

                let enabled = !form.api_key.trim().is_empty() && !form.api_secret.trim().is_empty();
                let state_label = if !enabled {
                    t.disabled_empty_keys
                } else if form.symbols.is_empty() {
                    t.idle_no_tickers
                } else {
                    t.enabled
                };

                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new(exchange.title()).size(16.0).strong());
                        ui.label(egui::RichText::new(state_label).size(10.0).weak());
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(t.clear_keys)
                                    .fill(REMOVE_BUTTON)
                                    .min_size(egui::vec2(88.0, 25.0)),
                            )
                            .clicked()
                        {
                            form.api_key.clear();
                            form.api_secret.clear();
                            form.passphrase.clear();
                            credentials_cleared = true;
                        }
                    });
                });

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let fields = if exchange == ExchangeId::Okx { 3.0 } else { 2.0 };
                    let spacing = ui.spacing().item_spacing.x;
                    let width = ((ui.available_width() - spacing * (fields - 1.0)) / fields).max(120.0);

                    ui.add_sized(
                        [width, ROW_HEIGHT],
                        egui::TextEdit::singleline(&mut form.api_key).hint_text(t.api_key),
                    );
                    ui.add_sized(
                        [width, ROW_HEIGHT],
                        egui::TextEdit::singleline(&mut form.api_secret)
                            .password(true)
                            .hint_text(t.api_secret),
                    );
                    if exchange == ExchangeId::Okx {
                        ui.add_sized(
                            [width, ROW_HEIGHT],
                            egui::TextEdit::singleline(&mut form.passphrase)
                                .password(true)
                                .hint_text(t.passphrase),
                        );
                    }
                });

                ui.add_space(4.0);
                ui.label(egui::RichText::new(t.tickers_market_cancel_side).size(11.0));

                if form.symbols.is_empty() {
                    ui.label(egui::RichText::new(t.no_tickers).size(10.0).weak());
                } else {
                    let mut remove_index = None;
                    for (index, item) in form.symbols.iter_mut().enumerate() {
                        ui.push_id((exchange, index), |ui| {
                            ui.horizontal(|ui| {
                                // TigerX rows carry the underlying exchange
                                // (`binance`/`okx`) in addition to the market.
                                if exchange == ExchangeId::Tigerx
                                    && ui
                                        .add_sized(
                                            [UNDERLYING_WIDTH, ROW_HEIGHT],
                                            egui::Button::new(item.underlying.as_str()).fill(MARKET_BUTTON),
                                        )
                                        .clicked()
                                {
                                    item.underlying = item.underlying.toggled();
                                }

                                if ui
                                    .add_sized(
                                        [MARKET_WIDTH, ROW_HEIGHT],
                                        egui::Button::new(item.market.as_str()).fill(MARKET_BUTTON),
                                    )
                                    .clicked()
                                {
                                    item.market = item.market.toggled(exchange);
                                }

                                let symbol_width = (ui.available_width()
                                    - SIDE_WIDTH
                                    - REMOVE_WIDTH
                                    - ui.spacing().item_spacing.x * 2.0)
                                    .max(120.0);
                                ui.add_sized(
                                    [symbol_width, ROW_HEIGHT],
                                    egui::TextEdit::singleline(&mut item.symbol)
                                        .hint_text("BTC/USDT"),
                                );

                                egui::ComboBox::from_id_salt("side")
                                    .selected_text(side_label(item.side, language))
                                    .width(SIDE_WIDTH)
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(
                                            &mut item.side,
                                            CancelSide::Buy,
                                            side_label(CancelSide::Buy, language),
                                        );
                                        ui.selectable_value(
                                            &mut item.side,
                                            CancelSide::Sell,
                                            side_label(CancelSide::Sell, language),
                                        );
                                        ui.selectable_value(
                                            &mut item.side,
                                            CancelSide::Both,
                                            side_label(CancelSide::Both, language),
                                        );
                                    });

                                if ui
                                    .add_sized(
                                        [REMOVE_WIDTH, ROW_HEIGHT],
                                        egui::Button::new(t.remove).fill(REMOVE_BUTTON),
                                    )
                                    .clicked()
                                {
                                    remove_index = Some(index);
                                }
                            });
                        });
                    }

                    if let Some(index) = remove_index {
                        form.symbols.remove(index);
                    }
                }

                ui.add_space(4.0);
                if ui
                    .add(
                        egui::Button::new(t.add_ticker)
                            .fill(SECONDARY_BUTTON)
                            .min_size(egui::vec2(104.0, 25.0)),
                    )
                    .clicked()
                {
                    form.symbols.push(SymbolForm {
                        market: MarketChoice::futures_default(exchange),
                        underlying: UnderlyingExchange::Binance,
                        symbol: String::new(),
                        side: CancelSide::Both,
                    });
                }
            });
        }

        if credentials_cleared {
            self.status = format!("{}: {}", exchange.title(), t.credentials_cleared);
        }
    }

    fn status_box(&self, ui: &mut egui::Ui) {
        egui::Frame::group(ui.style()).fill(PANEL_BG).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(egui::RichText::new(&self.status).size(10.0));
        });
    }
}

impl eframe::App for State {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_hotkey_events(ui.ctx());
        self.capture_hotkey_input(ui.ctx());
        self.poll_engine_result();

        egui::CentralPanel::default_margins().show(ui, |ui| {
            self.header(ui);
            ui.add_space(6.0);
            self.settings(ui);
            ui.add_space(6.0);

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .wheel_scroll_multiplier(egui::vec2(1.0, 2.5))
                .animated(false)
                .show(ui, |ui| {
                    self.exchange_card(ui, ExchangeId::Binance);
                    ui.add_space(6.0);
                    self.exchange_card(ui, ExchangeId::Okx);
                    ui.add_space(6.0);
                    self.exchange_card(ui, ExchangeId::Bybit);
                    ui.add_space(6.0);
                    self.exchange_card(ui, ExchangeId::Tigerx);
                    ui.add_space(6.0);
                    self.status_box(ui);
                });
        });

        self.observe_config_changes(ui.ctx());
        self.autosave_if_due(ui.ctx());
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Do not lose a valid edit when the window is closed before the debounce fires.
        self.autosave_on_exit();
        if let Some(handle) = self.run_handle.take() {
            handle.abort();
        }
        self.engine_rx = None;
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}

fn side_label(side: CancelSide, language: Language) -> &'static str {
    match (language, side) {
        (Language::English, CancelSide::Buy) => "buy",
        (Language::English, CancelSide::Sell) => "sell",
        (Language::English, CancelSide::Both) => "both",
        (Language::Russian, CancelSide::Buy) => "покупка",
        (Language::Russian, CancelSide::Sell) => "продажа",
        (Language::Russian, CancelSide::Both) => "обе стороны",
    }
}


fn configure_egui(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Dark);

    let mut visuals = egui::Visuals::dark();
    visuals.override_text_color = Some(TEXT);
    visuals.panel_fill = APP_BG;
    visuals.window_fill = PANEL_BG;
    visuals.extreme_bg_color = INPUT_BG;
    visuals.faint_bg_color = egui::Color32::from_rgb(28, 31, 37);
    visuals.selection.bg_fill = ACCENT;
    visuals.selection.stroke = egui::Stroke::new(1.0, TEXT);

    visuals.widgets.noninteractive.bg_fill = PANEL_BG;
    visuals.widgets.noninteractive.weak_bg_fill = PANEL_BG;
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, BORDER);
    visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, TEXT);

    visuals.widgets.inactive.bg_fill = INPUT_BG;
    visuals.widgets.inactive.weak_bg_fill = SECONDARY_BUTTON;
    visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, BORDER);
    visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, TEXT);

    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(47, 52, 63);
    visuals.widgets.hovered.weak_bg_fill = egui::Color32::from_rgb(55, 62, 75);
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, ACCENT_HOVER);
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, TEXT);

    visuals.widgets.active.bg_fill = egui::Color32::from_rgb(53, 60, 74);
    visuals.widgets.active.weak_bg_fill = ACCENT;
    visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0, ACCENT_HOVER);
    visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, TEXT);

    visuals.widgets.open.bg_fill = egui::Color32::from_rgb(43, 48, 58);
    visuals.widgets.open.weak_bg_fill = egui::Color32::from_rgb(52, 59, 72);
    visuals.widgets.open.bg_stroke = egui::Stroke::new(1.0, ACCENT_HOVER);
    visuals.widgets.open.fg_stroke = egui::Stroke::new(1.0, TEXT);

    ctx.set_visuals(visuals);
    ctx.global_style_mut(|style| {
        // Static desktop UI: no hover/fade animations and no label text selection.
        style.animation_time = 0.0;
        style.spacing.item_spacing = egui::vec2(6.0, 5.0);
        style.spacing.button_padding = egui::vec2(8.0, 4.0);
        style.interaction.selectable_labels = false;
        style.interaction.multi_widget_text_select = false;
    });
}

#[cfg(target_os = "windows")]
fn configure_native_title_bar(cc: &eframe::CreationContext<'_>) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::Graphics::Dwm::{
        DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR,
        DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
        SetWindowPos,
    };

    let Ok(window_handle) = cc.window_handle() else {
        return;
    };
    let RawWindowHandle::Win32(handle) = window_handle.as_raw() else {
        return;
    };
    let hwnd = handle.hwnd.get() as HWND;

    unsafe fn set_attr<T>(hwnd: HWND, attribute: u32, value: &T) -> i32 {
        unsafe {
            DwmSetWindowAttribute(
                hwnd,
                attribute,
                (value as *const T).cast(),
                std::mem::size_of::<T>() as u32,
            )
        }
    }

    const LEGACY_DARK_MODE_ATTRIBUTE: u32 = 19;
    let enabled: i32 = 1;
    let caption = colorref(APP_BG);
    let border = colorref(BORDER);
    let text = colorref(TEXT);

    unsafe {
        let result = set_attr(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
            &enabled,
        );
        if result < 0 {
            let _ = set_attr(hwnd, LEGACY_DARK_MODE_ATTRIBUTE, &enabled);
        }

        // Windows 11 supports explicit caption/border/text colors. Older Windows
        // versions simply reject these attributes, so failures are harmless.
        let _ = set_attr(hwnd, DWMWA_CAPTION_COLOR as u32, &caption);
        let _ = set_attr(hwnd, DWMWA_BORDER_COLOR as u32, &border);
        let _ = set_attr(hwnd, DWMWA_TEXT_COLOR as u32, &text);

        // DWM can cache the initial non-client frame. Force Windows to rebuild
        // it immediately, instead of waiting for the first deactivate/activate
        // cycle. This fixes the initial white caption and invisible caption buttons.
        let _ = SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

#[cfg(not(target_os = "windows"))]
fn configure_native_title_bar(_cc: &eframe::CreationContext<'_>) {}

#[cfg(target_os = "windows")]
fn colorref(color: egui::Color32) -> u32 {
    let [r, g, b, _] = color.to_array();
    r as u32 | ((g as u32) << 8) | ((b as u32) << 16)
}

fn app_icon() -> egui::IconData {
    egui::IconData {
        rgba: include_bytes!("../assets/app.rgba").to_vec(),
        width: 64,
        height: 64,
    }
}

pub fn run() -> eframe::Result {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Limit Canceller 0.4.3")
            .with_inner_size([WINDOW_WIDTH, WINDOW_HEIGHT])
            .with_min_inner_size([WINDOW_MIN_WIDTH, WINDOW_MIN_HEIGHT])
            .with_icon(app_icon()),
        renderer: eframe::Renderer::Glow,
        centered: true,
        multisampling: 0,
        depth_buffer: 0,
        stencil_buffer: 0,
        dithering: false,
        ..eframe::NativeOptions::default()
    };

    eframe::run_native(
        "Limit Canceller 0.4.3",
        native_options,
        Box::new(|cc| Ok(Box::new(State::boot(cc)))),
    )
}
