//! eframe/egui GUI for managing config.toml and controlling the cancellation engine.
//!
//! This experimental frontend intentionally uses the lightweight Glow/OpenGL
//! renderer instead of wgpu. The trading engine and configuration format are
//! unchanged from the iced build.

use eframe::egui;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use crate::config::{AppConfig, BinanceConfig, BybitConfig, OkxConfig, SymbolConfig};
use crate::engine;
use crate::exchange::CancelSide;
use crate::symbol::Symbol;

const WINDOW_WIDTH: f32 = 820.0;
const WINDOW_HEIGHT: f32 = 620.0;
const WINDOW_MIN_WIDTH: f32 = 680.0;
const WINDOW_MIN_HEIGHT: f32 = 480.0;

const ROW_HEIGHT: f32 = 25.0;
const MARKET_WIDTH: f32 = 88.0;
const SIDE_WIDTH: f32 = 112.0;
const REMOVE_WIDTH: f32 = 72.0;

const MARKET_BUTTON: egui::Color32 = egui::Color32::from_rgb(54, 82, 138);
const SAVE_BUTTON: egui::Color32 = egui::Color32::from_rgb(46, 82, 140);
const START_BUTTON: egui::Color32 = egui::Color32::from_rgb(38, 112, 75);
const STOP_BUTTON: egui::Color32 = egui::Color32::from_rgb(145, 52, 58);
const REMOVE_BUTTON: egui::Color32 = egui::Color32::from_rgb(125, 47, 52);
const SECONDARY_BUTTON: egui::Color32 = egui::Color32::from_rgb(62, 68, 78);

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

    fn futures_default(exchange: ExchangeId) -> Self {
        match exchange {
            ExchangeId::Binance => Self::Futures,
            ExchangeId::Okx => Self::Swap,
            ExchangeId::Bybit => Self::Linear,
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
        }
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
    runtime: Option<tokio::runtime::Runtime>,
    run_handle: Option<tokio::task::JoinHandle<()>>,
    engine_rx: Option<Receiver<Result<(), String>>>,
}

impl State {
    fn boot(cc: &eframe::CreationContext<'_>) -> Self {
        configure_egui(&cc.egui_ctx);

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("failed to create Tokio runtime");

        let config_path = std::env::args()
            .nth(1)
            .unwrap_or_else(|| "config.toml".to_string());

        match AppConfig::load_or_default(&config_path) {
            Ok(config) => Self::from_config(config_path, config, runtime),
            Err(error) => {
                let language = Language::English;
                let mut state = Self::from_config(config_path, AppConfig::default(), runtime);
                state.language = language;
                state.status = format!("{}: {error:#}", ui_text(language).failed_load_config);
                state
            }
        }
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

        Self {
            config_path,
            poll_milliseconds: poll.to_string(),
            language,
            binance: ExchangeForm::binance(config.binance.as_ref()),
            okx: ExchangeForm::okx(config.okx.as_ref()),
            bybit: ExchangeForm::bybit(config.bybit.as_ref()),
            status: ui_text(language).ready.to_string(),
            running: false,
            runtime: Some(runtime),
            run_handle: None,
            engine_rx: None,
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

    fn save_clicked(&mut self) {
        let t = ui_text(self.language);
        match self.save_config() {
            Ok(_) => {
                self.status = if self.running {
                    t.saved_running.to_string()
                } else {
                    format!("{}: {}", t.saved_prefix, self.config_path)
                };
            }
            Err(error) => self.status = format!("{}: {error}", t.save_failed),
        }
    }

    fn start_clicked(&mut self, ctx: &egui::Context) {
        if self.running {
            return;
        }

        let t = ui_text(self.language);
        let config = match self.save_config() {
            Ok(config) => config,
            Err(error) => {
                self.status = format!("{}: {error}", t.failed_to_start);
                return;
            }
        };

        if !config.has_enabled_exchange() {
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
        let mut save = false;
        let mut start = false;
        let mut stop = false;

        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(egui::RichText::new("Limit Canceller 0.3.4 · egui").size(20.0).strong());
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
                        egui::Button::new(t.stop).fill(STOP_BUTTON).min_size(egui::vec2(58.0, 26.0)),
                    )
                    .clicked();
                start = ui
                    .add_enabled(
                        !running,
                        egui::Button::new(t.start).fill(START_BUTTON).min_size(egui::vec2(58.0, 26.0)),
                    )
                    .clicked();
                save = ui
                    .add(egui::Button::new(t.save).fill(SAVE_BUTTON).min_size(egui::vec2(66.0, 26.0)))
                    .clicked();

                let badge = if running {
                    egui::RichText::new(t.running_badge)
                        .size(11.0)
                        .color(egui::Color32::from_rgb(95, 205, 135))
                } else {
                    egui::RichText::new(t.stopped_badge)
                        .size(11.0)
                        .color(egui::Color32::from_rgb(170, 175, 185))
                };
                ui.label(badge);
            });
        });

        if save {
            self.save_clicked();
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

        egui::Frame::group(ui.style()).show(ui, |ui| {
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
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_min_width(ui.available_width());

                let enabled = !form.api_key.trim().is_empty() && !form.api_secret.trim().is_empty();
                let state_label = if enabled {
                    t.enabled
                } else {
                    t.disabled_empty_keys
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
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(egui::RichText::new(&self.status).size(10.0));
        });
    }
}

impl eframe::App for State {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_engine_result();

        egui::CentralPanel::default_margins().show(ui, |ui| {
            self.header(ui);
            ui.add_space(6.0);
            self.settings(ui);
            ui.add_space(6.0);

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .wheel_scroll_multiplier(egui::vec2(1.0, 2.5))
                .show(ui, |ui| {
                    self.exchange_card(ui, ExchangeId::Binance);
                    ui.add_space(6.0);
                    self.exchange_card(ui, ExchangeId::Okx);
                    ui.add_space(6.0);
                    self.exchange_card(ui, ExchangeId::Bybit);
                    ui.add_space(6.0);
                    self.status_box(ui);
                });
        });
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
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

    ctx.global_style_mut(|style| {
        style.animation_time = 0.0;
        style.spacing.item_spacing = egui::vec2(6.0, 5.0);
        style.spacing.button_padding = egui::vec2(8.0, 4.0);

        style.interaction.selectable_labels = false;
        style.interaction.multi_widget_text_select = false;
    });
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
            .with_title("Limit Canceller 0.3.4 · egui experimental")
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
        "Limit Canceller 0.3.3 egui experimental",
        native_options,
        Box::new(|cc| Ok(Box::new(State::boot(cc)))),
    )
}
