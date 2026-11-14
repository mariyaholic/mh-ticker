//! Settings in `%LOCALAPPDATA%\MH_Ticker\<profile>.json`, written with defaults on first run.
//! Every market source is public, so nothing here is a credential — except `webhook`, which is a
//! bearer secret (see `Alert`).
//!
//! `rows` are the symbols on the strip; `catalog` is the pick list behind the menu's *Symbols*
//! submenu and the Ultra symbol picker. Ticking a catalog entry shows it, unticking hides it; the
//! strip keeps catalog order. Any other CNBC symbol can be added to `catalog` (or `rows`) by hand.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

/// The strip, dock grid and panel stay readable up to this many symbols.
pub const MAX_ROWS: usize = 24;

/// Which product this process is. One engine, two builds: the profile picks the config file,
/// the single-instance mutex and the window title, so the two can run side by side and never
/// fight over each other's settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Profile {
    /// Executable name without `.exe`, also the mutex suffix and window title.
    pub id: &'static str,
    /// Config file inside the data folder (`classic.json`, `ultra.json`).
    pub config_file: &'static str,
    /// The Ultra build: theme menu, symbol picker, alerts and webhooks.
    pub ultra: bool,
}

impl Profile {
    /// The original: one theme, no settings beyond placement and symbols.
    pub const fn classic() -> Self {
        Self { id: "MH_Ticker", config_file: "config.json", ultra: false }
    }

    /// The heavy build: aesthetics and alerts.
    pub const fn ultra() -> Self {
        Self { id: "MH_Ticker_Ultra", config_file: "config_ultra.json", ultra: true }
    }

    pub fn title(self) -> &'static str {
        if self.ultra { "MH Ticker Ultra" } else { "MH Ticker" }
    }

    pub fn folder(self) -> PathBuf {
        let base = std::env::var("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("."));
        base.join("MH_Ticker")
    }
}

/// Set once at launch by `mh_ticker::launch`. Every path and mutex below reads it, so no call
/// site has to thread the profile through.
static PROFILE: OnceLock<Profile> = OnceLock::new();

pub fn init(p: Profile) {
    let _ = PROFILE.set(p);
}

pub fn profile() -> Profile {
    *PROFILE.get_or_init(Profile::classic)
}

/// Where the strip sits (the menu's Position submenu).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Placement {
    /// In the taskbar: the gap left of Start while it is roomy, else the gap by the tray, else
    /// the bottom-right corner above the taskbar.
    Auto,
    TaskbarLeft,
    TaskbarRight,
    /// One row anywhere, dragged by hand; the spot is saved in `x`/`y`.
    Floating,
    TopRight,
    BottomRight,
    TopLeft,
    BottomLeft,
}

impl Placement {
    pub const ALL: [Placement; 8] = [
        Placement::Auto,
        Placement::TaskbarLeft,
        Placement::TaskbarRight,
        Placement::Floating,
        Placement::TopRight,
        Placement::BottomRight,
        Placement::TopLeft,
        Placement::BottomLeft,
    ];

    /// A compact grid in a taskbar gap (always on top, re-raised over the taskbar).
    pub fn on_taskbar(self) -> bool {
        matches!(self, Placement::Auto | Placement::TaskbarLeft | Placement::TaskbarRight)
    }

    pub fn label(self) -> &'static str {
        match self {
            Placement::Auto => "Auto (taskbar, best gap)",
            Placement::TaskbarLeft => "Taskbar, left of Start",
            Placement::TaskbarRight => "Taskbar, next to the tray",
            Placement::Floating => "Floating (drag anywhere)",
            Placement::TopRight => "Top-right corner",
            Placement::BottomRight => "Bottom-right corner",
            Placement::TopLeft => "Top-left corner",
            Placement::BottomLeft => "Bottom-left corner",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Row {
    /// Short label drawn on the strip.
    pub label: String,
    /// CNBC quote symbol (`XAU=` gold spot, `@DJ.1` front Dow future, `BTC.CB=` Coinbase bitcoin).
    pub symbol: String,
    pub digits: u32,
    /// What it is, shown beside the label in the Symbols menu.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// Shown on the taskbar strip. Absent means shown, so a 1.8 file and every Classic config are
    /// unchanged; only a pinned-out row writes `"strip": false`. The panel always shows every row. It
    /// lives on the rows (the shown list), not on the catalogue: `in_menu_order` carries it across a
    /// rebuild, and hiding a symbol forgets it.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub strip: bool,
}

fn yes() -> bool {
    true
}

fn is_true(b: &bool) -> bool {
    *b
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Opt-in to alternate quote providers when CNBC fails or is stale.
    pub fallback_enabled: bool,
    /// Seconds between quote requests (one batched request covers every row).
    pub poll_s: f64,
    /// A real-time price older than this is drawn dimmed. Rows CNBC marks as delayed get
    /// `delayed_grace_s` on top (its futures run ~10 min behind).
    pub stale_after_s: f64,
    pub delayed_grace_s: f64,
    pub rows: Vec<Row>,
    /// The Symbols menu's pick list (rows not in it are listed too).
    pub catalog: Vec<Row>,
    pub font: String,
    pub font_pt: i32,
    pub opacity: u8,
    pub strip_visible: bool,
    pub topmost: bool,
    /// Global hotkey that opens and closes the panel, in `hotkey::parse` grammar
    /// (`Ctrl+Alt+M`). None = no hotkey.
    pub hotkey: Option<String>,
    /// Where the strip sits. Unset in configs from before the Position menu: then `dock`
    /// decides (true = Auto, false = Floating).
    pub placement: Option<Placement>,
    /// Display for the strip (`\\.\DISPLAY2`); unset or unplugged = the primary one.
    pub monitor: Option<String>,
    /// Older on/off form of `placement`, kept in step with it.
    pub dock: bool,
    pub dock_font_pt: i32,
    pub dock_rows: u32,
    /// Last strip position in screen pixels when undocked; None = top-right of the work area.
    pub x: Option<i32>,
    pub y: Option<i32>,
    /// The panel's last frame, `[x, y, w, h]`. None = size it to its content and centre it, which
    /// is what every config written before a panel became resizable gets.
    pub panel: Option<[i32; 4]>,
    /// The candle pane's clock: false (default) cuts candles and labels times on the machine's local
    /// time, true keeps them in UTC. Flipped by clicking the zone name in the pane's header.
    pub chart_utc: bool,
    /// The range the candle pane opens on (`1D`, `5D`, `1M`, `6M`, `1Y`, `5Y`): the last tab clicked.
    /// Unset reads as `1D`, and so does any text that names no range (`ranges::Range::from_saved`).
    /// Ultra only; Classic has no pane.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chart_range: Option<String>,

    // --- Ultra only. `serde(default)` keeps a classic config parsing unchanged. ---
    /// Theme name (`theme::ALL`). Ignored by the classic build, which has exactly one.
    pub theme: String,
    /// Panel face override (`theme::FACES`). Unset = the theme's own face. Ignored by the classic
    /// build, and refused (with the theme's face kept) when it names a face that is not listed.
    pub theme_font: Option<String>,
    /// Price alerts, checked after each poll.
    pub alerts: Vec<Alert>,
    /// Where alerts are also POSTed (Discord/Slack incoming webhook, ntfy topic, generic endpoint).
    /// This is a bearer secret: anyone with the URL can post to that channel. It is never printed
    /// in a note, tooltip or `--probe` line.
    pub webhook: Option<String>,
    /// New alerts clear the level that fired (*Fire once, then clear*). The menu toggle also
    /// rewrites `once` on every existing alert.
    pub alert_once: bool,
    /// Alerts are evaluated and logged but not shown or posted, until switched off.
    pub alerts_muted: bool,
    /// UTC seconds a timed mute lasts until; past it the mute is over by the clock, no timer.
    pub alerts_mute_until: f64,
    /// Quiet hours as `[from, to)` minutes of the local day; wraps midnight when `from > to`;
    /// `from == to` is off.
    pub quiet_hours: Option<[u16; 2]>,
    /// New alerts fire on each crossing of their level instead of every cooldown (*Each crossing*).
    /// The menu choice also rewrites `crossing` on every existing alert. Off is 1.8's behaviour.
    #[serde(skip_serializing_if = "is_false")]
    pub alert_crossing: bool,
    /// Poll less often while no shown price has changed (`governor`). Unset = the build's default:
    /// on for Ultra, off for Classic, whose polling stays exactly as it was until the user ticks it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slow_when_quiet: Option<bool>,
    /// Poll once in five minutes while the session is locked and no alert is armed. Unset as above.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rest_when_locked: Option<bool>,
    /// Post a line to the webhook when alerts could not be checked for a while (`watch`). It names
    /// times and never a price. Off by default.
    #[serde(skip_serializing_if = "is_false")]
    pub gap_notice: bool,
    /// Which colours mean up and down (`updown::UpDown`): `red_green` or `blue_orange`. Unset and any
    /// other text are the default, green up and red down. Ultra only; Classic keeps its palette.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updown: Option<String>,
    /// Up candles are drawn as outlines, so a rise differs from a fall by shape. Ultra only.
    #[serde(skip_serializing_if = "is_false")]
    pub hollow_up: bool,
    /// Follow the Windows contrast theme (`theme::from_system`). Unset = the build's default: on
    /// (`auto`) for Ultra, off for Classic, whose pixels stay as they were until the user ticks it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contrast: Option<bool>,
    /// Follow the Windows animation setting for the panel's open and close. Unset = the build's
    /// default: `auto` for Ultra, on (always animate) for Classic, which stays as it was.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub animate: Option<bool>,
    /// The panel's zoom in percent (75, 90, 100, 125, 150). Unset and any other number read as 100.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub panel_zoom: Option<u32>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// One price threshold. Fires when the last price crosses `above` upward or `below` downward, at
/// most once per `cooldown_s`, and never for a stale quote (a closed market frozen at a level
/// would otherwise retrigger forever).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Alert {
    /// The row's `label`, so the alert follows the symbol if the catalogue reorders.
    pub label: String,
    pub above: Option<f64>,
    pub below: Option<f64>,
    pub cooldown_s: f64,
    /// UTC seconds the alert last fired. Persisted so a restart does not re-fire immediately.
    pub last: f64,
    /// Fires when the day's change reaches this many percent, up or down (absolute value).
    pub day_move: Option<f64>,
    /// Clear the level that fired; an alert left with no level is removed.
    pub once: bool,
    /// Fire on each crossing of a level, and re-arm only after the price goes back past it by a small
    /// band (`alerts::crossing_band`). Ignored when `once` is set. Off is 1.8's every-cooldown rule.
    #[serde(skip_serializing_if = "is_false")]
    pub crossing: bool,
    /// A fast-move alert: an index into `alerts::FAST_MOVES` plus one, so 0 is none (`1 % in 5 min`,
    /// `2 % in 15 min`, `3 % in 60 min`). Anything out of range is read as none.
    #[serde(skip_serializing_if = "is_zero")]
    pub fast: u8,
    /// Which levels have fired and not yet re-armed (crossing mode): bit 0 above, 1 below, 2 day
    /// move, 3 fast move. Runtime only, so a restart fires once if the price is already beyond.
    #[serde(skip)]
    pub latched: u8,
}

fn is_zero(n: &u8) -> bool {
    *n == 0
}

impl Default for Alert {
    fn default() -> Self {
        Self {
            label: String::new(),
            above: None,
            below: None,
            cooldown_s: 900.0,
            last: 0.0,
            day_move: None,
            once: false,
            crossing: false,
            fast: 0,
            latched: 0,
        }
    }
}

fn row(label: &str, symbol: &str, digits: u32, name: &str) -> Row {
    Row { label: label.into(), symbol: symbol.into(), digits, name: name.into(), strip: true }
}

/// The symbols shown on a fresh install (and by *Reset to default*).
pub fn default_rows() -> Vec<Row> {
    default_catalog().into_iter().filter(|r| ["XAU", "XAG", "BTC", "DOW", "S&P", "NAS"].contains(&r.label.as_str())).collect()
}

/// CNBC symbols checked live on 2026-10-15. "delayed" = CNBC marks them ~10 min behind.
pub fn default_catalog() -> Vec<Row> {
    vec![
        row("XAU", "XAU=", 2, "Gold spot"),
        row("XAG", "XAG=", 3, "Silver spot"),
        row("XPT", "XPT=", 2, "Platinum spot"),
        row("BTC", "BTC.CB=", 0, "Bitcoin (Coinbase)"),
        row("ETH", "ETH.CB=", 2, "Ether (Coinbase)"),
        row("DOW", "@DJ.1", 0, "Dow futures, delayed"),
        row("S&P", "@SP.1", 2, "S&P 500 futures, delayed"),
        row("NAS", "@ND.1", 2, "Nasdaq 100 futures, delayed"),
        row("WTI", "@CL.1", 2, "WTI crude futures, delayed"),
        row("BRENT", "@LCO.1", 2, "Brent crude futures, delayed"),
        row("NATGAS", "@NG.1", 3, "Natural gas futures, delayed"),
        row("COPPER", "@HG.1", 4, "Copper futures, delayed"),
        row("EURUSD", "EUR=", 4, "Euro / US dollar"),
        row("GBPUSD", "GBP=", 4, "Pound / US dollar"),
        row("USDJPY", "JPY=", 2, "US dollar / yen"),
        row("DXY", ".DXY", 2, "US dollar index"),
        row("US10Y", "US10Y", 3, "US 10-year yield, %"),
        row("US2Y", "US2Y", 3, "US 2-year yield, %"),
        row("VIX", ".VIX", 2, "Volatility index"),
        row("N225", ".N225", 0, "Nikkei 225, delayed"),
        row("DAX", ".GDAXI", 0, "Germany DAX, delayed"),
        row("FTSE", ".FTSE", 0, "FTSE 100, delayed"),
        row("HSI", ".HSI", 0, "Hang Seng, delayed"),
    ]
}

impl Default for Config {
    fn default() -> Self {
        Self {
            fallback_enabled: false,
            poll_s: 3.0,
            stale_after_s: 90.0,
            delayed_grace_s: 900.0,
            rows: default_rows(),
            catalog: default_catalog(),
            font: "Segoe UI".into(),
            font_pt: 10,
            opacity: 235,
            strip_visible: true,
            topmost: true,
            hotkey: None,
            placement: None,
            monitor: None,
            dock: true,
            dock_font_pt: 9,
            dock_rows: 2,
            x: None,
            y: None,
            panel: None,
            chart_utc: false,
            chart_range: None,
            theme: "midnight".into(),
            theme_font: None,
            alerts: Vec::new(),
            webhook: None,
            alert_once: false,
            alerts_muted: false,
            alerts_mute_until: 0.0,
            quiet_hours: None,
            alert_crossing: false,
            slow_when_quiet: None,
            rest_when_locked: None,
            gap_notice: false,
            updown: None,
            hollow_up: false,
            contrast: None,
            animate: None,
            panel_zoom: None,
        }
    }
}

impl Config {
    /// The governor's two ticks as this build sees them: what the user chose, else on for Ultra and
    /// off for Classic.
    pub fn governor(&self, ultra: bool) -> crate::governor::Settings {
        crate::governor::Settings {
            slow_when_quiet: self.slow_when_quiet.unwrap_or(ultra),
            rest_when_locked: self.rest_when_locked.unwrap_or(ultra),
        }
    }

    /// The up/down colours in force. Classic never changes its palette, whatever the file says.
    pub fn updown_mode(&self, ultra: bool) -> crate::updown::UpDown {
        if ultra { crate::updown::UpDown::from_saved(self.updown.as_deref()) } else { crate::updown::UpDown::GreenRed }
    }

    /// Up candles are outlines. Ultra only.
    pub fn hollow(&self, ultra: bool) -> bool {
        ultra && self.hollow_up
    }

    /// Follow the Windows contrast theme: what the user chose, else on for Ultra and off for Classic.
    pub fn follows_contrast(&self, ultra: bool) -> bool {
        self.contrast.unwrap_or(ultra)
    }

    /// Follow the Windows animation setting: what the user chose, else on for Ultra. Classic's
    /// default is "always animate", which is what it did before the setting existed.
    pub fn follows_animation(&self, ultra: bool) -> bool {
        self.animate.unwrap_or(ultra)
    }

    /// The panel zoom in percent. Ultra only, and only a listed step; anything else is 100.
    pub fn zoom(&self, ultra: bool) -> u32 {
        match self.panel_zoom {
            Some(z) if ultra => crate::zoom::from_saved(Some(z)),
            _ => 100,
        }
    }

    pub fn placement(&self) -> Placement {
        self.placement.unwrap_or(if self.dock { Placement::Auto } else { Placement::Floating })
    }

    pub fn set_placement(&mut self, p: Placement) {
        self.placement = Some(p);
        self.dock = p.on_taskbar();
    }
}

/// The Symbols menu: the catalog, then any row that is not in it (hand-added to `rows`).
pub fn menu_list(cfg: &Config) -> Vec<Row> {
    let mut list = cfg.catalog.clone();
    for r in &cfg.rows {
        if !list.iter().any(|c| c.symbol == r.symbol) {
            list.push(r.clone());
        }
    }
    list
}

/// The longest symbol *Add symbol...* accepts, in characters.
pub const MAX_SYMBOL: usize = 20;
/// The longest label made for an added symbol, in characters (the strip is narrow).
pub const MAX_LABEL: usize = 8;
/// The longest name kept for an added symbol, in characters, the ellipsis included.
pub const MAX_NAME: usize = 40;

/// A typed CNBC symbol, made safe to store and to send: trimmed, upper-cased (ASCII), and only when
/// every character is one of `A-Z 0-9 . @ = - ^ / : _` and there are 1 to [`MAX_SYMBOL`] of them.
/// `None` otherwise (empty, a space inside, a letter outside ASCII, too long). `" aapl "` gives
/// `"AAPL"`, `"@gc.1"` gives `"@GC.1"`, `"eur="` gives `"EUR="`.
pub fn clean_symbol(raw: &str) -> Option<String> {
    let s = raw.trim().to_ascii_uppercase();
    let count = s.chars().count();
    let ok = (1..=MAX_SYMBOL).contains(&count) && s.chars().all(|c| c.is_ascii_alphanumeric() || ".@=-^/:_".contains(c));
    ok.then_some(s)
}

/// The strip label for a newly added `symbol` (already cleaned): the symbol without a leading `@`,
/// `.` or `^`, without a trailing `=`, and without a trailing `.1` (a front-month future: `@GC.1` is
/// `GC`); cut to [`MAX_LABEL`] characters; `SYM` if nothing is left. Then made unique against every
/// label in `catalog` and `rows`, compared ignoring ASCII case, by appending `2`, `3`, ... and cutting
/// the base so the whole stays within [`MAX_LABEL`] (`GOLD` taken gives `GOLD2`).
pub fn label_for(cfg: &Config, symbol: &str) -> String {
    let base = symbol.trim_start_matches(['@', '.', '^']);
    let base = base.strip_suffix('=').unwrap_or(base);
    let base = base.strip_suffix(".1").unwrap_or(base);
    let base: String = base.chars().take(MAX_LABEL).collect();
    let base = if base.is_empty() { "SYM".to_string() } else { base };
    unique_label(&base, |label| cfg.catalog.iter().chain(cfg.rows.iter()).any(|r| r.label.eq_ignore_ascii_case(label)))
}

/// `base` if `taken` does not know it, else `base` with `2`, `3`, ... appended, the base cut so the
/// whole stays within [`MAX_LABEL`]. The rule `label_for` and `rename` share.
fn unique_label(base: &str, taken: impl Fn(&str) -> bool) -> String {
    if !taken(base) {
        return base.to_string();
    }
    let mut n = 2;
    loop {
        let suffix = n.to_string();
        let head: String = base.chars().take(MAX_LABEL - suffix.len()).collect();
        let candidate = format!("{head}{suffix}");
        if !taken(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// A typed label made fit for a card: control characters dropped, whitespace runs made one space,
/// trimmed, cut to [`MAX_LABEL`] characters. Empty stays empty. The case is the user's.
pub fn clean_label(raw: &str) -> String {
    let kept: String = raw.chars().map(|c| if c.is_whitespace() { ' ' } else { c }).filter(|c| !c.is_control()).collect();
    kept.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(MAX_LABEL).collect::<String>().trim_end().to_string()
}

/// The label `symbol` would have had when added: a default symbol's own, else `label_for`'s reading of
/// the symbol, either made unique against every other label (the symbol's own entries do not count).
pub fn derived_label(cfg: &Config, symbol: &str) -> String {
    let base = default_catalog().into_iter().find(|d| d.symbol == symbol).map(|d| d.label).unwrap_or_else(|| {
        let b = symbol.trim_start_matches(['@', '.', '^']);
        let b = b.strip_suffix('=').unwrap_or(b);
        let b = b.strip_suffix(".1").unwrap_or(b);
        let b: String = b.chars().take(MAX_LABEL).collect();
        if b.is_empty() { "SYM".to_string() } else { b }
    });
    unique_label(&base, |l| label_taken_by_another(cfg, symbol, l))
}

/// Is `label` the label of another symbol's row or catalogue entry, or of an alert that no card of
/// `symbol` owns? Compared ignoring ASCII case.
fn label_taken_by_another(cfg: &Config, symbol: &str, label: &str) -> bool {
    let mine: Vec<&str> = cfg.catalog.iter().chain(cfg.rows.iter()).filter(|r| r.symbol == symbol).map(|r| r.label.as_str()).collect();
    cfg.catalog.iter().chain(cfg.rows.iter()).any(|r| r.symbol != symbol && r.label.eq_ignore_ascii_case(label))
        || cfg.alerts.iter().any(|a| a.label.eq_ignore_ascii_case(label) && !mine.contains(&a.label.as_str()))
}

/// Give `symbol` the label typed in `raw`: cleaned (`clean_label`), made unique like an added symbol's
/// (a taken label gets a number), and moved everywhere the old one was the key: the shown row, the
/// catalogue entry and the alerts. Returns the label now in force, or `None` when nothing changed
/// (nothing typed, the symbol is in neither list, or the result is the label it has).
pub fn rename(cfg: &mut Config, symbol: &str, raw: &str) -> Option<String> {
    let typed = clean_label(raw);
    if typed.is_empty() {
        return None;
    }
    let new = unique_label(&typed, |l| label_taken_by_another(cfg, symbol, l));
    set_label(cfg, symbol, new)
}

/// *Reset name*: the derived label (`derived_label`) back on `symbol`. Same return as `rename`.
pub fn reset_name(cfg: &mut Config, symbol: &str) -> Option<String> {
    let new = derived_label(cfg, symbol);
    set_label(cfg, symbol, new)
}

fn set_label(cfg: &mut Config, symbol: &str, new: String) -> Option<String> {
    let old = cfg.rows.iter().chain(cfg.catalog.iter()).find(|r| r.symbol == symbol).map(|r| r.label.clone())?;
    if old == new {
        return None;
    }
    for r in cfg.rows.iter_mut().chain(cfg.catalog.iter_mut()).filter(|r| r.symbol == symbol) {
        r.label = new.clone();
    }
    for a in cfg.alerts.iter_mut().filter(|a| a.label == old) {
        a.label = new.clone();
    }
    Some(new)
}

/// The most decimals a card can be set to.
pub const MAX_DIGITS: u32 = 8;

/// What *Decimals: auto* means for `symbol`: a default symbol's own count, else the count its price
/// gives (`digits_for`), else 2 while there is no price.
pub fn auto_digits(symbol: &str, price: Option<f64>) -> u32 {
    default_catalog().into_iter().find(|d| d.symbol == symbol).map(|d| d.digits).unwrap_or_else(|| digits_for(price.unwrap_or(f64::NAN)))
}

/// Set the decimals of `symbol`: `Some(n)` (cut to [`MAX_DIGITS`]) or `None` for auto (`auto_digits`).
/// Row and catalogue entry both change; alerts keep their levels, which are prices, not text. Returns
/// whether anything changed.
pub fn set_digits(cfg: &mut Config, symbol: &str, digits: Option<u32>, price: Option<f64>) -> bool {
    let want = digits.map_or_else(|| auto_digits(symbol, price), |d| d.min(MAX_DIGITS));
    let mut changed = false;
    for r in cfg.rows.iter_mut().chain(cfg.catalog.iter_mut()).filter(|r| r.symbol == symbol) {
        changed |= r.digits != want;
        r.digits = want;
    }
    changed
}

/// Decimal places for an added symbol, from the size of its first price: 0 at 10,000 and above, 2
/// from 10, 4 from 0.01, and below 0.01 enough for three significant figures, at most 8. A price that
/// is not positive and finite gives 2.
pub fn digits_for(price: f64) -> u32 {
    if !price.is_finite() || price <= 0.0 {
        return 2;
    }
    if price >= 10_000.0 {
        0
    } else if price >= 10.0 {
        2
    } else if price >= 0.01 {
        4
    } else {
        // Below 0.01: three significant figures. Here `exp` is at most -3, so `2 - exp` is positive.
        let exp = price.log10().floor() as i32;
        ((2 - exp) as u32).min(8)
    }
}

/// CNBC's name for an added symbol, made fit for a menu line: control characters (a tab would split
/// the menu's columns) dropped, runs of whitespace made one space, trimmed, and cut to [`MAX_NAME`]
/// characters with a trailing `...` when longer. Empty stays empty.
pub fn clean_name(raw: &str) -> String {
    // Whitespace (a tab included) becomes a space and other control characters go; then the runs of
    // spaces collapse and the ends are trimmed.
    let mut kept = String::with_capacity(raw.len());
    for c in raw.chars() {
        if c.is_whitespace() {
            kept.push(' ');
        } else if !c.is_control() {
            kept.push(c);
        }
    }
    let joined = kept.split_whitespace().collect::<Vec<_>>().join(" ");
    if joined.chars().count() <= MAX_NAME {
        return joined;
    }
    let head: String = joined.chars().take(MAX_NAME - 3).collect();
    format!("{head}...")
}

/// What [`add_custom`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Added {
    /// The symbol is on the strip now (newly listed, or listed already and now ticked).
    Shown,
    /// Listed in the Symbols menu, but the strip already shows [`MAX_ROWS`]: not ticked.
    Listed,
    /// It was on the strip already; nothing changed.
    AlreadyShown,
}

/// Add a symbol CNBC knows to the catalogue and tick it when there is room. `symbol` is already
/// cleaned ([`clean_symbol`]), `name` is CNBC's and `price` its first price. A symbol already in the
/// Symbols menu (`menu_list`) is not added twice: it is ticked if it is not shown. A new entry is
/// appended to `catalog` with [`label_for`], [`digits_for`] and [`clean_name`], so it shows last.
pub fn add_custom(cfg: &mut Config, symbol: &str, name: &str, price: f64) -> Added {
    if is_shown(cfg, symbol) {
        return Added::AlreadyShown;
    }
    if !menu_list(cfg).iter().any(|r| r.symbol == symbol) {
        let label = label_for(cfg, symbol);
        cfg.catalog.push(Row { label, symbol: symbol.to_string(), digits: digits_for(price), name: clean_name(name), strip: true });
    }
    if toggle(cfg, symbol) {
        Added::Shown
    } else {
        Added::Listed
    }
}

/// The catalogue entries that are not in `default_catalog()` (by symbol), in catalogue order: what
/// *Remove a symbol I added* lists.
pub fn custom_entries(cfg: &Config) -> Vec<Row> {
    let defaults = default_catalog();
    cfg.catalog.iter().filter(|r| !defaults.iter().any(|d| d.symbol == r.symbol)).cloned().collect()
}

/// Remove an added symbol: from `catalog`, from `rows`, and its alerts (by its label). Refused, with
/// nothing changed, when the symbol is in `default_catalog()`, is not in the catalogue, or is the only
/// symbol shown. Returns whether anything changed.
pub fn forget_custom(cfg: &mut Config, symbol: &str) -> bool {
    let Some(row) = custom_entries(cfg).into_iter().find(|r| r.symbol == symbol) else {
        return false;
    };
    if is_shown(cfg, symbol) && cfg.rows.len() <= 1 {
        return false;
    }
    cfg.catalog.retain(|r| r.symbol != symbol);
    cfg.rows.retain(|r| r.symbol != symbol);
    normalize_pins(cfg);
    cfg.alerts.retain(|a| a.label != row.label);
    true
}

/// What one undo puts back (`undo::Undo`). It holds the cards or the alerts, never the webhook (or
/// anything else), so restoring cannot bring back a webhook that was removed: it has no way to.
#[derive(Clone, Debug)]
pub enum Snapshot {
    /// The shown rows, their order and pins, and the catalogue (labels, decimals and order live there too).
    Rows { rows: Vec<Row>, catalog: Vec<Row> },
    /// The alerts as they were.
    Alerts(Vec<Alert>),
}

impl Config {
    /// The cards as they are now.
    pub fn snapshot_rows(&self) -> Snapshot {
        Snapshot::Rows { rows: self.rows.clone(), catalog: self.catalog.clone() }
    }

    /// The alerts as they are now.
    pub fn snapshot_alerts(&self) -> Snapshot {
        Snapshot::Alerts(self.alerts.clone())
    }

    /// Put a snapshot back. Cards replace the cards. Alerts are merged: every alert in the snapshot
    /// whose label has none now comes back, in its old place; one set since (same label) stays as it is
    /// now, and alerts made since stay, after the others. Nothing else in the
    /// config is touched; the webhook least of all.
    pub fn restore(&mut self, snap: Snapshot) {
        match snap {
            Snapshot::Rows { rows, catalog } => {
                self.rows = rows;
                self.catalog = catalog;
                normalize_pins(self);
            }
            Snapshot::Alerts(old) => {
                let now = std::mem::take(&mut self.alerts);
                let mut merged: Vec<Alert> = old
                    .into_iter()
                    .map(|a| match now.iter().find(|n| n.label == a.label) {
                        Some(n) => n.clone(),
                        None => a,
                    })
                    .collect();
                let extra: Vec<Alert> = now.into_iter().filter(|n| !merged.iter().any(|m| m.label == n.label)).collect();
                merged.extend(extra);
                self.alerts = merged;
            }
        }
    }
}

/// The rows on the taskbar strip, as indexes into `cfg.rows`, in order (`pins::strip_indices`): the
/// pinned ones, or every row when none is pinned. A config with no `strip` field shows every row.
pub fn strip_rows(cfg: &Config) -> Vec<usize> {
    let pinned: Vec<bool> = cfg.rows.iter().map(|r| r.strip).collect();
    crate::pins::strip_indices(&pinned)
}

/// Pin `symbol` to the strip (`on`) or take it off. The last pinned symbol cannot be taken off, and a
/// symbol that is not shown has no pin. Returns whether anything changed.
pub fn set_pin(cfg: &mut Config, symbol: &str, on: bool) -> bool {
    let pinned: Vec<bool> = cfg.rows.iter().map(|r| r.strip).collect();
    let Some(i) = cfg.rows.iter().position(|r| r.symbol == symbol) else { return false };
    if pinned[i] == on || (!on && !crate::pins::can_unpin(&pinned, i)) {
        return false;
    }
    cfg.rows[i].strip = on;
    true
}

/// *Show all on strip*: pin every shown row. Returns whether anything changed.
pub fn pin_all(cfg: &mut Config) -> bool {
    let any = cfg.rows.iter().any(|r| !r.strip);
    cfg.rows.iter_mut().for_each(|r| r.strip = true);
    any
}

/// One row is always on the strip: when no shown row is pinned (the last pinned one was hidden, or a
/// hand-edited file pins none) every shown row is pinned again. Returns whether anything changed.
pub fn normalize_pins(cfg: &mut Config) -> bool {
    if cfg.rows.is_empty() || cfg.rows.iter().any(|r| r.strip) {
        return false;
    }
    cfg.rows.iter_mut().for_each(|r| r.strip = true);
    true
}

pub fn is_shown(cfg: &Config, symbol: &str) -> bool {
    cfg.rows.iter().any(|r| r.symbol == symbol)
}

/// Show or hide one menu entry. The last symbol cannot be hidden and at most `MAX_ROWS` show;
/// shown rows keep menu order. Returns whether anything changed.
pub fn toggle(cfg: &mut Config, symbol: &str) -> bool {
    if is_shown(cfg, symbol) {
        if cfg.rows.len() <= 1 {
            return false;
        }
        cfg.rows.retain(|r| r.symbol != symbol);
        normalize_pins(cfg);
        return true;
    }
    if cfg.rows.len() >= MAX_ROWS {
        return false;
    }
    let shown: Vec<String> = cfg.rows.iter().map(|r| r.symbol.clone()).collect();
    cfg.rows = in_menu_order(cfg, |s| s == symbol || shown.iter().any(|x| x == s));
    true
}

/// The Symbols menu's entries whose symbol `keep` accepts, in menu order.
fn in_menu_order(cfg: &Config, keep: impl Fn(&str) -> bool) -> Vec<Row> {
    menu_list(cfg)
        .into_iter()
        .filter(|r| keep(&r.symbol))
        .map(|mut r| {
            // A row that is shown already keeps its pin; one being shown now starts on the strip.
            r.strip = cfg.rows.iter().find(|o| o.symbol == r.symbol).is_none_or(|o| o.strip);
            r
        })
        .collect()
}

/// Hide a shown symbol. Unlike `toggle`, a symbol that is not shown is never added: the card menu and
/// the Delete key carry a symbol that may have left the strip while a menu was open. Refuses the last
/// row. Returns whether anything changed.
pub fn remove(cfg: &mut Config, symbol: &str) -> bool {
    is_shown(cfg, symbol) && toggle(cfg, symbol)
}

/// Move a shown symbol `delta` places along the strip (negative = left, positive = right), by one
/// place per unit, stopping at the ends. Returns whether the order changed.
///
/// The order is a permutation of the catalogue, so it survives later `toggle`s ("rows keep menu
/// order"): a shown row that is not in the catalogue is first appended to it (which changes nothing
/// visible: `menu_list` already lists it last), then the symbol swaps places in the catalogue with
/// the shown neighbour it passes, hidden catalogue entries in between keep their relative place, and
/// `rows` is rebuilt exactly as `toggle` builds it. A symbol that is not shown, `delta == 0` or a
/// move past an end changes nothing and returns false (the catalogue is then left as it was).
pub fn move_row(cfg: &mut Config, symbol: &str, delta: i32) -> bool {
    let Some(from) = cfg.rows.iter().position(|r| r.symbol == symbol) else {
        return false;
    };
    let to = from as i64 + i64::from(delta);
    if delta == 0 || to < 0 || to >= cfg.rows.len() as i64 {
        return false;
    }
    move_shown(cfg, from, to as usize)
}

/// Move a shown symbol to the front of the strip. Same permutation rules as `move_row`. Returns
/// whether the order changed (false when it is already first, not shown, or the only row).
pub fn move_first(cfg: &mut Config, symbol: &str) -> bool {
    match cfg.rows.iter().position(|r| r.symbol == symbol) {
        Some(from) if from > 0 => move_shown(cfg, from, 0),
        _ => false,
    }
}

/// Put the shown row at `from` at `to`. The shown symbols keep the catalogue positions they hold, and
/// take them in the new order, so hidden entries stay where they are. Works on a copy and commits
/// only when every step holds, so a false return leaves `cfg` untouched.
fn move_shown(cfg: &mut Config, from: usize, to: usize) -> bool {
    let mut order: Vec<String> = cfg.rows.iter().map(|r| r.symbol.clone()).collect();
    let moved = order.remove(from);
    order.insert(to, moved);
    apply_order(cfg, &order)
}

/// Make the shown symbols appear in `order` (a permutation of the shown symbols), by the rule
/// `move_shown` states. Pins travel with their rows (`in_menu_order`). False, with `cfg` untouched, when
/// `order` is not a permutation of the shown symbols.
fn apply_order(cfg: &mut Config, order: &[String]) -> bool {
    let mut next = cfg.clone();
    for r in &cfg.rows {
        if !next.catalog.iter().any(|c| c.symbol == r.symbol) {
            next.catalog.push(r.clone());
        }
    }
    let slots: Vec<usize> = next.catalog.iter().enumerate().filter(|(_, c)| order.contains(&c.symbol)).map(|(i, _)| i).collect();
    let picked: Vec<Row> = order.iter().filter_map(|s| next.catalog.iter().find(|c| &c.symbol == s).cloned()).collect();
    if slots.len() != order.len() || picked.len() != order.len() || order.len() != cfg.rows.len() {
        return false;
    }
    for (pos, row) in slots.into_iter().zip(picked) {
        next.catalog[pos] = row;
    }
    next.rows = in_menu_order(&next, |s| order.iter().any(|x| x == s));
    *cfg = next;
    true
}

/// How *Order the cards* sorts, once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrderKey {
    /// The biggest move of the day first, up or down (the absolute change).
    DayMove,
    /// By label, A to Z, ignoring case.
    Name,
    /// The order of a fresh install: `default_catalog()`'s, then anything added, in its present order.
    Default,
}

/// Rewrite the order of the shown symbols once by `key` (a one-shot, nothing live follows). `moves`
/// maps a symbol to its fresh day change in percent; a symbol with no entry has no fresh quote. The
/// sort is stable. For `DayMove`, rows with no fresh quote keep their relative place after the others.
/// The order is a permutation of the catalogue like every move (`move_row`), so hidden entries stay
/// where they are and a later toggle keeps the order. Returns whether the order changed.
pub fn order_by(cfg: &mut Config, moves: &std::collections::HashMap<String, f64>, key: OrderKey) -> bool {
    let mut shown: Vec<&Row> = cfg.rows.iter().collect();
    match key {
        OrderKey::DayMove => {
            let fresh = |r: &&Row| moves.get(&r.symbol).copied().filter(|m| m.is_finite());
            // `sort_by` is stable; fresh rows first by the size of the move, the rest keep their place.
            shown.sort_by(|a, b| match (fresh(a), fresh(b)) {
                (Some(x), Some(y)) => y.abs().total_cmp(&x.abs()),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            });
        }
        OrderKey::Name => shown.sort_by_key(|r| r.label.to_lowercase()),
        OrderKey::Default => {
            let defaults = default_catalog();
            let rank = |r: &&Row| defaults.iter().position(|d| d.symbol == r.symbol).unwrap_or(usize::MAX);
            shown.sort_by_key(rank);
        }
    }
    let order: Vec<String> = shown.iter().map(|r| r.symbol.clone()).collect();
    let before: Vec<&str> = cfg.rows.iter().map(|r| r.symbol.as_str()).collect();
    if order.iter().map(String::as_str).eq(before.iter().copied()) {
        return false;
    }
    apply_order(cfg, &order)
}

pub fn path() -> PathBuf {
    let p = profile();
    p.folder().join(p.config_file)
}

fn parse_settings(text: &str) -> Result<Config, serde_json::Error> {
    serde_json::from_str(text)
}

/// What start-up found on disk.
#[derive(Debug)]
pub enum Loaded {
    /// The settings file was read (or there was none and the defaults were written).
    Read(Config),
    /// The settings file would not parse. Its bytes were moved, unchanged, to `kept`; the defaults
    /// are loaded and written to the settings path.
    Defaulted { cfg: Config, kept: PathBuf },
}

impl Loaded {
    pub fn into_config(self) -> Config {
        match self {
            Loaded::Read(cfg) | Loaded::Defaulted { cfg, .. } => cfg,
        }
    }
}

/// The file name (never the folder, which carries the user name) a settings file that would not
/// parse was kept under in this run. `None` when nothing was kept. Read by the menu's greyed line.
static KEPT_AS: Mutex<Option<String>> = Mutex::new(None);

pub fn kept_bad_config() -> Option<String> {
    KEPT_AS.lock().ok().and_then(|k| k.clone())
}

/// Start-up load. A settings file that will not parse is kept beside the new one rather than
/// replaced or refused: see [`load_or_keep`]. Only a file that cannot be read at all, or cannot be
/// moved aside, is an `Err`.
pub fn load() -> Result<Config, String> {
    let loaded = load_or_keep(&path())?;
    if let Loaded::Defaulted { kept, .. } = &loaded {
        if let Ok(mut k) = KEPT_AS.lock() {
            *k = kept.file_name().map(|n| n.to_string_lossy().into_owned());
        }
    }
    Ok(loaded.into_config())
}

/// The settings for a command-line probe: read like [`load`], but nothing is ever written or moved.
/// A missing file is the defaults; one that will not parse is an `Err`.
pub fn load_readonly() -> Result<Config, String> {
    let p = path();
    match std::fs::read_to_string(&p) {
        Ok(text) => parse_settings(&text).map_err(|e| format!("Invalid settings (line {}, column {})", e.line(), e.column())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(_) => Err("Could not read the settings file".to_string()),
    }
}

/// [`load_from`], except that a file whose text is not UTF-8 or does not parse is renamed to the
/// first free `<stem>.bad.json`, `<stem>.bad-2.json`, ... in the same folder (the bytes are never
/// rewritten, and an earlier kept file is never replaced), and the defaults are loaded and saved
/// at `p`. If the file cannot be moved it stays where it is and the result is an `Err`.
fn load_or_keep(p: &Path) -> Result<Loaded, String> {
    let bad = match std::fs::read(p) {
        Ok(bytes) => std::str::from_utf8(&bytes).map_or(true, |t| parse_settings(t).is_err()),
        Err(_) => false,
    };
    if !bad {
        return load_from(p).map(Loaded::Read);
    }
    let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "settings".into());
    let kept = (1..100)
        .map(|n| p.with_file_name(if n == 1 { format!("{stem}.bad.json") } else { format!("{stem}.bad-{n}.json") }))
        .find(|k| !k.exists())
        .ok_or_else(|| "Too many kept settings files; remove some".to_string())?;
    std::fs::rename(p, &kept).map_err(|_| "Could not keep the unreadable settings file; it was not changed".to_string())?;
    let cfg = Config::default();
    save_to(p, &cfg);
    Ok(Loaded::Defaulted { cfg, kept })
}

fn load_from(p: &Path) -> Result<Config, String> {
    match std::fs::read_to_string(p) {
        Ok(text) => {
            // Never replace an unreadable configuration with defaults: it may contain the user's
            // symbols, alert thresholds and webhook. Stop before any save can overwrite it.
            let cfg = parse_settings(&text).map_err(|e| {
                format!("Invalid settings at {} (line {}, column {})", p.display(), e.line(), e.column())
            })?;
            if !text.contains("\"catalog\"") {
                save_to(p, &cfg);
            }
            Ok(cfg)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let cfg = Config::default();
            save_to(p, &cfg);
            Ok(cfg)
        }
        Err(e) => Err(format!("Could not read settings at {}: {e}", p.display())),
    }
}

/// Whether the last `save` / `save_checked` failed. Starts false; a success clears it again.
static SAVE_FAILED: AtomicBool = AtomicBool::new(false);

/// True while the most recent save through `save` or `save_checked` failed (read-only folder,
/// full disk, antivirus lock). The menu's status line says so. `save_checked_to` itself does not
/// record, so tests of the low-level writer cannot race with a test of this flag.
pub fn save_failed() -> bool {
    SAVE_FAILED.load(Ordering::Relaxed)
}

/// Record a save outcome in the flag read by `save_failed`. Returns the result unchanged.
fn record_save(result: std::io::Result<()>) -> std::io::Result<()> {
    SAVE_FAILED.store(result.is_err(), Ordering::Relaxed);
    result
}

/// Write the settings. Signature unchanged; the outcome is recorded for `save_failed`.
pub fn save(cfg: &Config) {
    save_to(&path(), cfg);
}

pub fn save_checked(cfg: &Config) -> std::io::Result<()> {
    record_save(save_checked_to(&path(), cfg))
}

fn save_checked_to(p: &Path, cfg: &Config) -> std::io::Result<()> {
    if let Some(dir) = p.parent() { std::fs::create_dir_all(dir)?; }
    let text = serde_json::to_string_pretty(cfg).map_err(std::io::Error::other)?;
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, text)?;
    if let Err(e) = std::fs::rename(&tmp, p) {
        let _ = std::fs::remove_file(tmp);
        return Err(e);
    }
    Ok(())
}

fn save_to(p: &Path, cfg: &Config) {
    let _ = record_save(save_checked_to(p, cfg));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn symbols(rows: &[Row]) -> Vec<&str> {
        rows.iter().map(|r| r.symbol.as_str()).collect()
    }

    #[test]
    fn fallback_policy_is_off_by_default_and_persists_explicit_consent() {
        let old: Config = serde_json::from_str("{}").unwrap();
        let saved = serde_json::to_value(&old).unwrap();
        assert_eq!(saved.get("fallback_enabled"), Some(&serde_json::json!(false)));
        let on: Config = serde_json::from_str(r#"{"fallback_enabled":true}"#).unwrap();
        assert_eq!(serde_json::to_value(on).unwrap()["fallback_enabled"], true);
    }

    #[test]
    fn old_configs_parse_to_todays_alert_behaviour() {
        let c: Config = serde_json::from_str("{}").unwrap();
        assert!(!c.alert_once && !c.alerts_muted && c.alerts_mute_until == 0.0 && c.quiet_hours.is_none());
        let c: Config = serde_json::from_str(r#"{"alerts":[{"label":"XAU","above":4100.0}]}"#).unwrap();
        let a = &c.alerts[0];
        assert_eq!((a.above, a.day_move, a.once, a.cooldown_s), (Some(4100.0), None, false, 900.0));
        let c: Config = serde_json::from_str(r#"{"quiet_hours":[1320,420]}"#).unwrap();
        assert_eq!(c.quiet_hours, Some([1320, 420]));
    }

    #[test]
    fn save_reports_failure_instead_of_claiming_consent_persisted() {
        let root = std::env::var_os("TICKER_TEST_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let dest = root.join(format!("mh-ticker-save-dir-{}", std::process::id()));
        std::fs::create_dir_all(&dest).unwrap();
        let result = save_checked_to(&dest, &Config::default());
        assert!(result.is_err());
        assert!(dest.is_dir());
        let _ = std::fs::remove_file(dest.with_extension("json.tmp"));
        std::fs::remove_dir(dest).unwrap();
    }

    #[test]
    fn malformed_settings_are_not_accepted_as_defaults() {
        assert!(parse_settings("{invalid json").is_err());
        assert!(parse_settings(r#"{"placement":"not-a-placement"}"#).is_err());
        assert!(parse_settings("{}").is_ok());
    }

    #[test]
    fn the_chart_clock_defaults_to_local_and_persists_utc() {
        let c: Config = serde_json::from_str("{}").unwrap();
        assert!(!c.chart_utc, "an old config gets local time");
        let utc: Config = serde_json::from_str(r#"{"chart_utc":true}"#).unwrap();
        assert!(utc.chart_utc);
        assert!(serde_json::to_string(&utc).unwrap().contains(r#""chart_utc":true"#));
    }

    #[test]
    fn a_config_that_will_not_parse_is_kept_and_defaults_load() {
        let root = std::env::var_os("TICKER_TEST_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let dir = root.join(format!("mh-ticker-keep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        let bytes: &[u8] = b"{\"rows\": [ \"webhook\": \"https://hooks.example/x\" ,, \xff";
        std::fs::write(&path, bytes).unwrap();
        let Loaded::Defaulted { cfg, kept } = load_or_keep(&path).unwrap() else { panic!("expected defaults") };
        assert_eq!(serde_json::to_value(&cfg).unwrap(), serde_json::to_value(Config::default()).unwrap());
        assert_eq!(kept.file_name().unwrap(), "settings.bad.json");
        assert_eq!(std::fs::read(&kept).unwrap(), bytes, "the original bytes survive");
        assert!(parse_settings(&std::fs::read_to_string(&path).unwrap()).is_ok());
        // A second bad file does not replace the first kept one.
        std::fs::write(&path, "{nope").unwrap();
        let Loaded::Defaulted { kept: second, .. } = load_or_keep(&path).unwrap() else { panic!("expected defaults") };
        assert_eq!(second.file_name().unwrap(), "settings.bad-2.json");
        assert_eq!(std::fs::read(&kept).unwrap(), bytes);
        // A good file is read as before, and nothing is kept.
        assert!(matches!(load_or_keep(&path).unwrap(), Loaded::Read(_)));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn invalid_config_file_is_not_replaced_or_migrated() {
        let root = std::env::var_os("TICKER_TEST_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let path = root.join(format!("mh-ticker-invalid-{}.json", std::process::id()));
        let text = r#"{"placement":"bad-value","rows":[]}"#;
        std::fs::write(&path, text).unwrap();
        assert!(load_from(&path).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn toggle_keeps_catalog_order_and_limits() {
        let mut c = Config::default();
        assert!(toggle(&mut c, "ETH.CB="));
        assert_eq!(symbols(&c.rows), ["XAU=", "XAG=", "BTC.CB=", "ETH.CB=", "@DJ.1", "@SP.1", "@ND.1"]);
        assert!(toggle(&mut c, "XAU="));
        assert!(!is_shown(&c, "XAU="));
        c.rows.truncate(1);
        let last = c.rows[0].symbol.clone();
        assert!(!toggle(&mut c, &last), "the last symbol stays");
        // A catalogue of its own, one longer than the cap: the default one is shorter than MAX_ROWS.
        let long: Vec<Row> = (0..=MAX_ROWS).map(|i| row(&format!("S{i}"), &format!("S{i}="), 2, "")).collect();
        let mut full = Config { rows: long[..MAX_ROWS].to_vec(), catalog: long, ..Config::default() };
        assert!(!toggle(&mut full, &format!("S{MAX_ROWS}=")), "no more than MAX_ROWS");
        assert_eq!(full.rows.len(), MAX_ROWS);
    }

    #[test]
    fn clean_symbol_trims_uppercases_and_refuses_the_rest() {
        assert_eq!(clean_symbol(" aapl ").as_deref(), Some("AAPL"));
        assert_eq!(clean_symbol("@gc.1").as_deref(), Some("@GC.1"));
        assert_eq!(clean_symbol("eur=").as_deref(), Some("EUR="));
        let too_long = "A".repeat(21);
        for bad in ["", "   ", "A B", "ÄPFEL", "A;B", too_long.as_str()] {
            assert_eq!(clean_symbol(bad), None, "{bad:?} is refused");
        }
        assert_eq!(clean_symbol(&"A".repeat(20)).as_deref(), Some("A".repeat(20).as_str()));
    }

    #[test]
    fn labels_are_derived_and_made_unique() {
        let c = Config::default();
        assert_eq!(label_for(&c, "AAPL"), "AAPL");
        assert_eq!(label_for(&c, "@GC.1"), "GC");
        assert_eq!(label_for(&c, "EUR="), "EUR");
        assert_eq!(label_for(&c, ".SPX"), "SPX");
        assert_eq!(label_for(&c, "BRK.B"), "BRK.B");
        assert_eq!(label_for(&c, "ABCDEFGHIJ"), "ABCDEFGH");
        assert_eq!(label_for(&c, "@="), "SYM");
        assert_eq!(label_for(&c, "XAU"), "XAU2", "XAU is on the default strip");

        let mut taken = Config::default();
        taken.rows.push(row("ABCDEFGH", "ABCDEFGH=", 2, ""));
        assert_eq!(label_for(&taken, "ABCDEFGHIJ"), "ABCDEFG2");
    }

    #[test]
    fn digits_follow_the_price_size() {
        let cases = [
            (98000.0, 0),
            (10000.0, 0),
            (9999.5, 2),
            (4200.0, 2),
            (10.0, 2),
            (9.99, 4),
            (1.08, 4),
            (0.01, 4),
            (0.00123, 5),
            (0.0000001234, 8),
            (0.0, 2),
            (-1.0, 2),
            (f64::NAN, 2),
            (f64::INFINITY, 2),
        ];
        for (price, want) in cases {
            assert_eq!(digits_for(price), want, "price {price}");
        }
    }

    #[test]
    fn names_are_cleaned_and_shortened() {
        assert_eq!(clean_name("  Apple\tInc  "), "Apple Inc");
        assert_eq!(clean_name("Gold\u{7}Spot"), "GoldSpot", "other control characters go");
        assert_eq!(clean_name(""), "");
        let long = "x".repeat(60);
        let short = clean_name(&long);
        assert_eq!(short.chars().count(), MAX_NAME);
        assert!(short.ends_with("..."));
        let exact = "y".repeat(MAX_NAME);
        assert_eq!(clean_name(&exact), exact);
    }

    #[test]
    fn add_custom_appends_ticks_and_respects_the_cap() {
        let mut c = Config::default();
        assert_eq!(add_custom(&mut c, "NVDA", "NVIDIA", 120.0), Added::Shown);
        assert_eq!(c.catalog.last().unwrap().symbol, "NVDA");
        assert_eq!(c.rows.last().unwrap().symbol, "NVDA");
        assert_eq!(c.rows.last().unwrap().label, "NVDA");
        assert_eq!(add_custom(&mut c, "NVDA", "", 120.0), Added::AlreadyShown);
        assert_eq!(c.catalog.iter().filter(|r| r.symbol == "NVDA").count(), 1);

        assert!(!is_shown(&c, "ETH.CB="));
        assert_eq!(add_custom(&mut c, "ETH.CB=", "", 2000.0), Added::Shown);
        assert_eq!(c.catalog.iter().filter(|r| r.symbol == "ETH.CB=").count(), 1, "not duplicated");

        let long: Vec<Row> = (0..=MAX_ROWS).map(|i| row(&format!("S{i}"), &format!("S{i}="), 2, "")).collect();
        let mut full = Config { rows: long[..MAX_ROWS].to_vec(), catalog: long, ..Config::default() };
        assert_eq!(add_custom(&mut full, "NEWX=", "New", 5.0), Added::Listed);
        assert!(full.catalog.iter().any(|r| r.symbol == "NEWX="));
        assert!(!is_shown(&full, "NEWX="));
        assert_eq!(full.rows.len(), MAX_ROWS);

        let mut c = Config::default();
        assert_eq!(add_custom(&mut c, "XAU", "", 2000.0), Added::Shown);
        assert_eq!(c.catalog.last().unwrap().label, "XAU2");
    }

    #[test]
    fn forget_custom_removes_only_what_was_added() {
        let mut c = Config::default();
        assert!(!forget_custom(&mut c, "XAU="), "a default symbol is refused");
        add_custom(&mut c, "NVDA", "", 120.0);
        add_custom(&mut c, "AMD", "", 100.0);
        assert_eq!(custom_entries(&c).iter().map(|r| r.symbol.as_str()).collect::<Vec<_>>(), ["NVDA", "AMD"]);
        c.alerts.push(Alert { label: "NVDA".into(), above: Some(130.0), ..Alert::default() });
        c.alerts.push(Alert { label: "AMD".into(), above: Some(130.0), ..Alert::default() });

        assert!(forget_custom(&mut c, "NVDA"));
        assert!(!c.catalog.iter().any(|r| r.symbol == "NVDA"));
        assert!(!is_shown(&c, "NVDA"));
        assert_eq!(c.alerts.iter().map(|a| a.label.as_str()).collect::<Vec<_>>(), ["AMD"]);
        assert_eq!(custom_entries(&c).iter().map(|r| r.symbol.as_str()).collect::<Vec<_>>(), ["AMD"]);

        let mut only = Config::default();
        add_custom(&mut only, "NVDA", "", 120.0);
        only.rows.retain(|r| r.symbol == "NVDA");
        let before = (only.catalog.clone(), only.rows.clone());
        assert!(!forget_custom(&mut only, "NVDA"), "the only symbol shown is refused");
        assert_eq!((only.catalog, only.rows), before);
    }

    #[test]
    fn hand_added_rows_stay_listed_and_in_place() {
        let mut c = Config::default();
        c.rows.push(row("SOL", "SOL=", 2, ""));
        assert_eq!(menu_list(&c).last().unwrap().symbol, "SOL=");
        assert!(toggle(&mut c, "ETH.CB="));
        assert_eq!(c.rows.last().unwrap().symbol, "SOL=");
    }

    /// The shown symbols, strip order.
    fn order(c: &Config) -> Vec<String> {
        c.rows.iter().map(|r| r.symbol.clone()).collect()
    }

    /// The catalogue's symbols, in catalogue order.
    fn catalog_order(c: &Config) -> Vec<String> {
        c.catalog.iter().map(|r| r.symbol.clone()).collect()
    }

    #[test]
    fn remove_never_adds_and_never_takes_the_last_row() {
        let mut c = Config::default();
        assert!(remove(&mut c, "XAU="));
        assert_eq!(order(&c), ["XAG=", "BTC.CB=", "@DJ.1", "@SP.1", "@ND.1"]);
        let before = order(&c);
        assert!(!remove(&mut c, "XAU="), "already hidden: nothing is added back");
        assert_eq!(order(&c), before);
        assert!(!remove(&mut c, "XPT="), "never shown");
        assert_eq!(order(&c), before);
        c.rows.truncate(1);
        assert!(!remove(&mut c, "XAG="), "the last row stays");
        assert_eq!(order(&c), ["XAG="]);
    }

    #[test]
    fn move_row_swaps_with_the_shown_neighbour_and_stops_at_the_ends() {
        let mut c = Config::default();
        assert_eq!(order(&c), ["XAU=", "XAG=", "BTC.CB=", "@DJ.1", "@SP.1", "@ND.1"]);
        assert!(move_row(&mut c, "BTC.CB=", -1));
        assert_eq!(order(&c), ["XAU=", "BTC.CB=", "XAG=", "@DJ.1", "@SP.1", "@ND.1"]);

        let (rows, catalog) = (order(&c), catalog_order(&c));
        assert!(!move_row(&mut c, "XAU=", -1), "the first row cannot go left");
        assert!(!move_row(&mut c, "@ND.1", 1), "the last row cannot go right");
        assert!(!move_row(&mut c, "XAG=", 0), "delta 0 is no move");
        assert!(!move_row(&mut c, "XPT=", -1), "a hidden symbol does not move");
        assert!(!move_row(&mut c, "NOT.SHOWN=", 1), "an unknown symbol does not move");
        assert_eq!((order(&c), catalog_order(&c)), (rows, catalog), "every false leaves the order alone");

        assert!(move_row(&mut c, "@ND.1", -2), "two places in one call");
        assert_eq!(order(&c), ["XAU=", "BTC.CB=", "XAG=", "@ND.1", "@DJ.1", "@SP.1"]);
    }

    #[test]
    fn move_row_passes_over_hidden_catalogue_entries() {
        let mut c = Config::default();
        assert!(!is_shown(&c, "XPT="));
        assert!(move_row(&mut c, "BTC.CB=", -1));
        assert_eq!(catalog_order(&c)[..4], ["XAU=", "BTC.CB=", "XPT=", "XAG="], "the hidden XPT keeps its place");
        assert!(toggle(&mut c, "XPT="));
        assert_eq!(order(&c), ["XAU=", "BTC.CB=", "XPT=", "XAG=", "@DJ.1", "@SP.1", "@ND.1"]);
    }

    #[test]
    fn move_row_adopts_a_hand_added_row_into_the_catalogue() {
        let mut c = Config::default();
        c.rows.push(row("SOL", "SOL=", 2, ""));
        assert_eq!(menu_list(&c).last().unwrap().symbol, "SOL=");
        let catalog = catalog_order(&c);
        assert!(!move_row(&mut c, "SOL=", 1), "past the end");
        assert_eq!(catalog_order(&c), catalog, "a false return does not adopt the row");
        assert!(move_row(&mut c, "SOL=", -1));
        assert_eq!(order(&c), ["XAU=", "XAG=", "BTC.CB=", "@DJ.1", "@SP.1", "SOL=", "@ND.1"]);
        assert_eq!(catalog_order(&c).iter().filter(|s| *s == "SOL=").count(), 1, "adopted once");
        assert_eq!(menu_list(&c).iter().map(|r| r.symbol.clone()).collect::<Vec<_>>(), catalog_order(&c), "no duplicate in the menu");
    }

    #[test]
    fn the_order_survives_a_later_toggle() {
        let mut c = Config::default();
        assert!(move_first(&mut c, "BTC.CB="));
        assert_eq!(order(&c)[0], "BTC.CB=");
        assert!(toggle(&mut c, "ETH.CB="));
        assert_eq!(order(&c), ["BTC.CB=", "XAU=", "XAG=", "ETH.CB=", "@DJ.1", "@SP.1", "@ND.1"], "BTC stays first");
        assert!(toggle(&mut c, "XAU="), "hide XAU");
        assert!(toggle(&mut c, "XAU="), "show XAU again");
        assert_eq!(order(&c), ["BTC.CB=", "XAU=", "XAG=", "ETH.CB=", "@DJ.1", "@SP.1", "@ND.1"], "the moved order is kept");
    }

    #[test]
    fn move_first_puts_the_symbol_at_the_front() {
        let mut c = Config::default();
        assert!(move_first(&mut c, "@ND.1"));
        assert_eq!(order(&c), ["@ND.1", "XAU=", "XAG=", "BTC.CB=", "@DJ.1", "@SP.1"]);
        let before = (order(&c), catalog_order(&c));
        assert!(!move_first(&mut c, "@ND.1"), "already first");
        assert!(!move_first(&mut c, "XPT="), "not shown");
        assert_eq!((order(&c), catalog_order(&c)), before);

        let mut solo = Config::default();
        solo.rows.truncate(1);
        assert!(!move_first(&mut solo, "XAU="), "the only row");

        let mut a = Config::default();
        let mut b = Config::default();
        assert!(move_first(&mut a, "@ND.1"));
        assert!(move_row(&mut b, "@ND.1", -5));
        assert_eq!((order(&a), catalog_order(&a)), (order(&b), catalog_order(&b)), "agrees with move_row");
    }

    #[test]
    fn placement_falls_back_to_the_old_dock_flag() {
        let docked: Config = serde_json::from_str(r#"{"dock":true}"#).unwrap();
        assert_eq!(docked.placement(), Placement::Auto);
        let mut free: Config = serde_json::from_str(r#"{"dock":false}"#).unwrap();
        assert_eq!(free.placement(), Placement::Floating);
        free.set_placement(Placement::TaskbarRight);
        assert!(free.dock);
        let text = serde_json::to_string(&free).unwrap();
        assert!(text.contains(r#""placement":"taskbar_right""#), "{text}");
    }

    #[test]
    fn save_failed_flag_sets_and_clears_with_the_last_save() {
        // Only record_save touches the flag in this test; no test drives a successful save_to
        // or load_from on a missing file, so the static cannot change under this test.
        let failed = record_save(Err(std::io::Error::other("x")));
        assert!(save_failed());
        assert_eq!(failed.unwrap_err().to_string(), "x", "the error passes through unchanged");
        assert!(record_save(Ok(())).is_ok());
        assert!(!save_failed());
    }

    #[test]
    fn hotkey_defaults_to_none_and_old_configs_parse() {
        let old: Config = serde_json::from_str("{}").unwrap();
        assert_eq!(old.hotkey, None);
        let set: Config = serde_json::from_str(r#"{"hotkey":"Ctrl+Alt+M"}"#).unwrap();
        assert_eq!(set.hotkey.as_deref(), Some("Ctrl+Alt+M"));
        let text = serde_json::to_string(&set).unwrap();
        let back: Config = serde_json::from_str(&text).unwrap();
        assert_eq!(back.hotkey.as_deref(), Some("Ctrl+Alt+M"));
    }

    #[test]
    fn old_config_without_catalog_gets_the_default_one() {
        let c: Config = serde_json::from_str(r#"{"rows":[{"label":"XAU","symbol":"XAU=","digits":2}]}"#).unwrap();
        assert_eq!(c.rows.len(), 1);
        assert_eq!(c.catalog, default_catalog());
    }

    #[test]
    fn the_w2_settings_default_to_1_8_and_a_1_8_file_means_the_same() {
        let c: Config = serde_json::from_str("{}").unwrap();
        assert!(!c.alert_crossing && !c.gap_notice);
        assert_eq!((c.slow_when_quiet, c.rest_when_locked), (None, None));
        // Classic polls exactly as 1.8 unless the user ticks it; Ultra's default is on.
        assert!(!c.governor(false).any());
        assert!(c.governor(true).slow_when_quiet && c.governor(true).rest_when_locked);
        let chosen = Config { slow_when_quiet: Some(true), ..Config::default() };
        assert!(chosen.governor(false).slow_when_quiet && !chosen.governor(false).rest_when_locked);
        let off = Config { slow_when_quiet: Some(false), rest_when_locked: Some(false), ..Config::default() };
        assert!(!off.governor(true).any());
        // A 1.8 alert has no new fields and reads as every cooldown, no fast move, nothing latched.
        let a: Alert = serde_json::from_str(r#"{"label":"XAU","above":4200.0,"below":null,"cooldown_s":900.0,"last":0.0,"day_move":null,"once":false}"#).unwrap();
        assert_eq!((a.crossing, a.fast, a.latched), (false, 0, 0));
        // Nothing new is written for an alert that uses none of it, so a 1.8 reader sees the same file.
        let json = serde_json::to_string(&Alert { label: "XAU".into(), above: Some(1.0), ..Alert::default() }).unwrap();
        assert!(!json.contains("crossing") && !json.contains("fast") && !json.contains("latched"), "{json}");
        let cfg_json = serde_json::to_string(&Config::default()).unwrap();
        for key in ["alert_crossing", "slow_when_quiet", "rest_when_locked", "gap_notice"] {
            assert!(!cfg_json.contains(key), "{key} is written only when chosen");
        }
        let set = serde_json::to_string(&Alert { crossing: true, fast: 2, latched: 7, ..Alert::default() }).unwrap();
        assert!(set.contains("\"crossing\":true") && set.contains("\"fast\":2") && !set.contains("latched"));
    }

    #[test]
    fn chart_range_is_absent_by_default_round_trips_and_an_unknown_value_reads_as_one_day() {
        let old: Config = serde_json::from_str("{}").unwrap();
        assert_eq!(old.chart_range, None);
        assert!(!serde_json::to_string(&old).unwrap().contains("chart_range"), "absent stays absent");
        let c = Config { chart_range: crate::ranges::Range::Y5.to_saved(), ..Config::default() };
        let back: Config = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        assert_eq!(crate::ranges::Range::from_saved(back.chart_range.as_deref()), crate::ranges::Range::Y5);
        let odd: Config = serde_json::from_str(r#"{"chart_range":"7Q"}"#).unwrap();
        assert_eq!(crate::ranges::Range::from_saved(odd.chart_range.as_deref()), crate::ranges::Range::D1);
    }

    #[test]
    fn the_w4_settings_default_to_1_8_and_a_1_8_file_means_the_same() {
        let c: Config = serde_json::from_str("{}").unwrap();
        assert_eq!((c.updown.clone(), c.hollow_up, c.contrast, c.animate, c.panel_zoom), (None, false, None, None, None));
        for ultra in [false, true] {
            assert_eq!(c.updown_mode(ultra), crate::updown::UpDown::GreenRed);
            assert!(!c.hollow(ultra));
            assert_eq!(c.zoom(ultra), 100);
        }
        // Ultra follows Windows on its own; Classic does not until it is ticked.
        assert!(c.follows_contrast(true) && !c.follows_contrast(false));
        assert!(c.follows_animation(true) && !c.follows_animation(false));
        let json = serde_json::to_string(&c).unwrap();
        for key in ["updown", "hollow_up", "contrast", "animate", "panel_zoom"] {
            assert!(!json.contains(key), "{key} is written only when chosen");
        }
        // Classic keeps its palette and size whatever a hand-edited file says.
        let set: Config = serde_json::from_str(r#"{"updown":"blue_orange","hollow_up":true,"panel_zoom":150}"#).unwrap();
        assert_eq!(set.updown_mode(true), crate::updown::UpDown::BlueOrange);
        assert!(set.hollow(true) && set.zoom(true) == 150);
        assert_eq!(set.updown_mode(false), crate::updown::UpDown::GreenRed);
        assert!(!set.hollow(false) && set.zoom(false) == 100);
        // A number that is not a step reads as 100.
        let odd: Config = serde_json::from_str(r#"{"panel_zoom":137}"#).unwrap();
        assert_eq!(odd.zoom(true), 100);
        let ticked = Config { contrast: Some(true), animate: Some(false), ..Config::default() };
        assert!(ticked.follows_contrast(false) && !ticked.follows_animation(true));
    }

    fn moves(list: &[(&str, f64)]) -> std::collections::HashMap<String, f64> {
        list.iter().map(|(s, m)| (s.to_string(), *m)).collect()
    }

    #[test]
    fn order_by_day_move_is_stable_and_puts_rows_with_no_fresh_quote_last() {
        let mut c = Config::default();
        // XAU= XAG= BTC.CB= @DJ.1 @SP.1 @ND.1; XAG and DOW have no fresh quote; BTC and NAS tie.
        let m = moves(&[("XAU=", 0.5), ("BTC.CB=", -2.0), ("@SP.1", 1.0), ("@ND.1", 2.0)]);
        assert!(order_by(&mut c, &m, OrderKey::DayMove));
        assert_eq!(order(&c), ["BTC.CB=", "@ND.1", "@SP.1", "XAU=", "XAG=", "@DJ.1"], "tie keeps place, unquoted last in their old order");
        assert!(!order_by(&mut c, &m, OrderKey::DayMove), "a second sort changes nothing");
    }

    #[test]
    fn order_by_leaves_hidden_symbols_where_they_are_and_later_toggles_keep_the_order() {
        let mut c = Config::default();
        let hidden_before: Vec<(usize, String)> =
            catalog_order(&c).into_iter().enumerate().filter(|(_, s)| !is_shown(&c, s)).collect();
        assert!(order_by(&mut c, &Default::default(), OrderKey::Name));
        assert_eq!(c.rows.iter().map(|r| r.label.as_str()).collect::<Vec<_>>(), ["BTC", "DOW", "NAS", "S&P", "XAG", "XAU"]);
        let hidden_after: Vec<(usize, String)> =
            catalog_order(&c).into_iter().enumerate().filter(|(_, s)| !is_shown(&c, s)).collect();
        assert_eq!(hidden_before, hidden_after, "hidden entries keep their catalogue slots");
        let sorted = order(&c);
        assert!(toggle(&mut c, "XPT="));
        assert!(toggle(&mut c, "XPT="));
        assert_eq!(order(&c), sorted, "the sorted order survives toggles");
    }

    #[test]
    fn order_by_default_restores_the_fresh_install_order_and_keeps_added_symbols_after_it() {
        let mut c = Config::default();
        add_custom(&mut c, "AAPL", "Apple", 200.0);
        assert!(move_first(&mut c, "@ND.1"));
        assert!(move_first(&mut c, "AAPL"));
        assert!(order_by(&mut c, &Default::default(), OrderKey::Default));
        assert_eq!(order(&c), ["XAU=", "XAG=", "BTC.CB=", "@DJ.1", "@SP.1", "@ND.1", "AAPL"]);
        assert!(!order_by(&mut c, &Default::default(), OrderKey::Default));
    }

    #[test]
    fn order_by_name_is_stable_for_equal_labels_and_ignores_case() {
        let mut c = Config::default();
        for (i, l) in [(0, "zed"), (1, "ALPHA")] {
            c.rows[i].label = l.into();
            c.catalog[i].label = l.into();
        }
        assert!(order_by(&mut c, &Default::default(), OrderKey::Name));
        assert_eq!(c.rows[0].label, "ALPHA");
        assert_eq!(c.rows.last().unwrap().label, "zed");
    }

    #[test]
    fn order_by_keeps_pins_with_their_rows() {
        let mut c = Config::default();
        c.rows.iter_mut().find(|r| r.label == "XAU").unwrap().strip = false;
        assert!(order_by(&mut c, &Default::default(), OrderKey::Name));
        assert!(!c.rows.iter().find(|r| r.label == "XAU").unwrap().strip);
        assert_eq!(c.rows.iter().filter(|r| !r.strip).count(), 1);
    }

    #[test]
    fn hiding_the_last_pinned_row_pins_the_rest_again() {
        let mut c = Config::default();
        for r in &mut c.rows[1..] {
            r.strip = false;
        }
        assert!(remove(&mut c, "XAU="));
        assert!(c.rows.iter().all(|r| r.strip), "one row is always on the strip");
    }

    #[test]
    fn strip_is_absent_in_the_file_unless_a_row_is_pinned_out() {
        let c = Config::default();
        assert!(!serde_json::to_string(&c).unwrap().contains("\"strip\":"));
        let mut d = Config::default();
        d.rows[2].strip = false;
        let back: Config = serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
        assert!(!back.rows[2].strip && back.rows[0].strip);
    }

    fn alert_on(label: &str) -> Alert {
        Alert { label: label.into(), above: Some(4100.5), ..Default::default() }
    }

    #[test]
    fn rename_moves_the_label_everywhere_it_is_the_key() {
        let mut c = Config::default();
        c.alerts.push(alert_on("XAU"));
        assert_eq!(rename(&mut c, "XAU=", "  Gold \t spot "), Some("Gold spo".to_string()), "cleaned and cut to the label length");
        assert_eq!(c.rows[0].label, "Gold spo");
        assert_eq!(c.catalog[0].label, "Gold spo");
        assert_eq!(c.alerts[0].label, "Gold spo", "alerts follow the rename");
        assert_eq!(c.alerts[0].above, Some(4100.5), "and keep their level");
        assert_eq!(rename(&mut c, "XAU=", "Gold spo"), None, "the same label changes nothing");
        assert_eq!(rename(&mut c, "XAU=", "   "), None, "nothing typed changes nothing");
        assert_eq!(rename(&mut c, "NOPE", "X"), None, "an unknown symbol changes nothing");
    }

    #[test]
    fn a_taken_label_gets_a_number_and_the_cut_keeps_it_within_the_limit() {
        let mut c = Config::default();
        assert_eq!(rename(&mut c, "XAU=", "xag"), Some("xag2".to_string()), "XAG is taken, case ignored");
        assert_eq!(rename(&mut c, "XPT=", "XAG"), Some("XAG3".to_string()), "XAG and xag2 are taken: next number");
        assert_eq!(rename(&mut c, "BTC.CB=", "LONGLABEL"), Some("LONGLABE".to_string()));
        assert_eq!(rename(&mut c, "ETH.CB=", "LONGLABEL"), Some("LONGLA2".to_string().replace("LONGLA2", "LONGLAB2")), "the base is cut to fit the number");
        assert!(c.catalog.iter().chain(&c.rows).all(|r| r.label.chars().count() <= MAX_LABEL));
        // A hidden catalogue entry's label counts as taken, and so does an alert's.
        let mut d = Config::default();
        d.alerts.push(alert_on("GHOST"));
        assert_eq!(rename(&mut d, "XAU=", "GHOST"), Some("GHOST2".to_string()));
        assert_eq!(d.alerts[0].label, "GHOST", "someone else's alert is not adopted");
        assert_eq!(rename(&mut d, "XAU=", "xpt"), Some("xpt2".to_string()), "XPT is hidden but listed");
        // Its own old label in another case is no collision.
        let mut e = Config::default();
        assert_eq!(rename(&mut e, "XAU=", "xau"), Some("xau".to_string()));
    }

    #[test]
    fn reset_name_returns_the_derived_label_and_the_alerts_follow() {
        let mut c = Config::default();
        c.alerts.push(alert_on("XAU"));
        rename(&mut c, "XAU=", "Gold");
        assert_eq!(reset_name(&mut c, "XAU="), Some("XAU".to_string()));
        assert_eq!(c.alerts[0].label, "XAU");
        assert_eq!(reset_name(&mut c, "XAU="), None, "already derived");
        add_custom(&mut c, "@GC.1", "Gold futures", 4000.0);
        rename(&mut c, "@GC.1", "Fut");
        assert_eq!(reset_name(&mut c, "@GC.1"), Some("GC".to_string()), "an added symbol's label comes from its symbol");
        // The derived label is made unique too.
        rename(&mut c, "@GC.1", "Zed");
        assert_eq!(rename(&mut c, "XAG=", "GC"), Some("GC".to_string()));
        assert_eq!(reset_name(&mut c, "@GC.1"), Some("GC2".to_string()));
    }

    #[test]
    fn decimals_are_clamped_and_auto_goes_back_without_touching_alert_levels() {
        let mut c = Config::default();
        c.alerts.push(alert_on("XAU"));
        assert!(set_digits(&mut c, "XAU=", Some(40), None));
        assert_eq!((c.rows[0].digits, c.catalog[0].digits), (MAX_DIGITS, MAX_DIGITS));
        assert!(!set_digits(&mut c, "XAU=", Some(8), None), "no change reported twice");
        assert!(set_digits(&mut c, "XAU=", None, None));
        assert_eq!(c.rows[0].digits, 2, "a default symbol returns to its own count");
        assert_eq!(c.alerts[0].above, Some(4100.5), "levels keep their value");
        add_custom(&mut c, "AAPL", "Apple", 200.0);
        set_digits(&mut c, "AAPL", Some(0), None);
        assert!(set_digits(&mut c, "AAPL", None, Some(200.0)));
        assert_eq!(c.rows.iter().find(|r| r.symbol == "AAPL").unwrap().digits, 2, "an added symbol from its price");
        assert_eq!(auto_digits("AAPL", None), 2, "no price yet: two");
    }

    #[test]
    fn a_rename_survives_a_toggle_and_a_move() {
        let mut c = Config::default();
        rename(&mut c, "XAG=", "Silv");
        assert!(toggle(&mut c, "XPT="));
        assert!(move_first(&mut c, "XAG="));
        assert_eq!(c.rows[0].label, "Silv");
        assert!(remove(&mut c, "XAG="));
        assert!(toggle(&mut c, "XAG="));
        assert_eq!(c.rows.iter().find(|r| r.symbol == "XAG=").unwrap().label, "Silv", "the catalogue entry holds it too");
    }

    #[test]
    fn with_no_strip_field_every_row_is_on_the_strip() {
        let c: Config = serde_json::from_str(r#"{"rows":[{"label":"A","symbol":"A","digits":2},{"label":"B","symbol":"B","digits":2}]}"#).unwrap();
        assert!(c.rows.iter().all(|r| r.strip));
        assert_eq!(strip_rows(&c), [0, 1]);
        assert_eq!(strip_rows(&Config::default()), [0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn pins_leave_the_panel_whole_and_the_last_pinned_row_stays() {
        let mut c = Config::default();
        assert!(set_pin(&mut c, "XAG=", false));
        assert!(set_pin(&mut c, "@DJ.1", false));
        assert_eq!(strip_rows(&c), [0, 2, 4, 5]);
        assert_eq!(c.rows.len(), 6, "the panel still has every card");
        assert!(!set_pin(&mut c, "XAG=", false), "already off");
        for s in ["XAU=", "BTC.CB=", "@SP.1"] {
            assert!(set_pin(&mut c, s, false));
        }
        assert_eq!(strip_rows(&c), [5], "one is left");
        assert!(!set_pin(&mut c, "@ND.1", false), "the last pinned row cannot be unpinned");
        assert!(!set_pin(&mut c, "XPT=", true), "a hidden symbol has no pin");
        assert!(pin_all(&mut c));
        assert_eq!(strip_rows(&c), [0, 1, 2, 3, 4, 5]);
        assert!(!pin_all(&mut c), "nothing left to do");
    }

    #[test]
    fn a_pin_survives_toggling_other_symbols_and_a_hand_edited_file_with_none_pinned_shows_all() {
        let mut c = Config::default();
        set_pin(&mut c, "XAG=", false);
        assert!(toggle(&mut c, "XPT="));
        assert!(!c.rows.iter().find(|r| r.symbol == "XAG=").unwrap().strip);
        assert!(c.rows.iter().find(|r| r.symbol == "XPT=").unwrap().strip, "a newly shown symbol starts on the strip");
        let mut d = Config::default();
        d.rows.iter_mut().for_each(|r| r.strip = false);
        assert_eq!(strip_rows(&d).len(), 6);
    }

    fn json(c: &Config) -> serde_json::Value {
        serde_json::to_value(c).unwrap()
    }

    #[test]
    fn restoring_the_cards_gives_back_the_exact_config_after_each_kind_of_change() {
        type Change = fn(&mut Config);
        let changes: [(&str, Change); 5] = [
            ("remove", |c| assert!(remove(c, "XAG="))),
            ("move", |c| assert!(move_row(c, "XAU=", 2))),
            ("sort", |c| assert!(order_by(c, &Default::default(), OrderKey::Name))),
            ("pin", |c| assert!(set_pin(c, "@DJ.1", false))),
            ("rename", |c| assert!(rename(c, "XAU=", "Gold").is_some())),
        ];
        for (name, change) in changes {
            let mut c = Config::default();
            c.alerts.push(alert_on("XAU"));
            c.webhook = Some("https://example.invalid/hook/secret".into());
            let before = json(&c);
            let snap = c.snapshot_rows();
            change(&mut c);
            assert_ne!(json(&c), before, "{name} changed something");
            c.restore(snap);
            if name == "rename" {
                // A rename moves the alert's label as well; the cards alone do not carry that back.
                continue;
            }
            assert_eq!(json(&c), before, "{name}: the exact config is back");
        }
    }

    #[test]
    fn restoring_alerts_brings_back_the_cleared_ones_and_keeps_what_was_set_since() {
        let mut c = Config::default();
        c.alerts.push(alert_on("XAU"));
        c.alerts.push(Alert { label: "XAG".into(), below: Some(30.0), last: 100.0, ..Default::default() });
        let before = json(&c);
        let snap = c.snapshot_alerts();
        c.alerts.clear();
        c.restore(snap.clone());
        assert_eq!(json(&c), before, "the exact config is back");
        // Cleared again, then a new level set on XAU and an unrelated one on BTC before undoing.
        c.alerts.clear();
        c.alerts.push(Alert { label: "XAU".into(), above: Some(5000.0), last: 200.0, ..Default::default() });
        c.alerts.push(alert_on("BTC"));
        c.restore(snap);
        let labels: Vec<&str> = c.alerts.iter().map(|a| a.label.as_str()).collect();
        assert_eq!(labels, ["XAU", "XAG", "BTC"]);
        assert_eq!(c.alerts[0].above, Some(5000.0), "what was set since stays");
        assert_eq!(c.alerts[1].below, Some(30.0));
    }

    #[test]
    fn undo_never_brings_back_a_removed_webhook() {
        let mut c = Config { webhook: Some("https://example.invalid/hook/secret".into()), ..Config::default() };
        c.alerts.push(alert_on("XAU"));
        let rows = c.snapshot_rows();
        let alerts = c.snapshot_alerts();
        assert!(!format!("{rows:?}{alerts:?}").contains("secret"), "the snapshot cannot hold it");
        c.webhook = None;
        c.alerts.clear();
        c.restore(alerts);
        c.restore(rows);
        assert_eq!(c.webhook, None);
        // And one set after the snapshot is left alone.
        let snap = c.snapshot_rows();
        c.webhook = Some("https://example.invalid/new".into());
        c.restore(snap);
        assert_eq!(c.webhook.as_deref(), Some("https://example.invalid/new"));
    }

    #[test]
    fn restoring_cards_with_no_pinned_row_pins_them_again() {
        let mut c = Config::default();
        let mut snap = c.snapshot_rows();
        if let Snapshot::Rows { rows, .. } = &mut snap {
            rows.iter_mut().for_each(|r| r.strip = false);
        }
        c.restore(snap);
        assert!(c.rows.iter().all(|r| r.strip));
    }
}
