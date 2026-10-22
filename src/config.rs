//! Settings in `%LOCALAPPDATA%\MH_Ticker\<profile>.json`, written with defaults on first run.
//! Every market source is public, so nothing here is a credential — except `webhook`, which is a
//! bearer secret (see `Alert`).
//!
//! `rows` are the symbols on the strip; `catalog` is the pick list behind the menu's *Symbols*
//! submenu and the Ultra symbol picker. Ticking a catalog entry shows it, unticking hides it; the
//! strip keeps catalog order. Any other CNBC symbol can be added to `catalog` (or `rows`) by hand.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::OnceLock;

/// The strip, dock grid and panel stay readable up to this many symbols.
pub const MAX_ROWS: usize = 12;

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
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
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
}

impl Default for Alert {
    fn default() -> Self {
        Self { label: String::new(), above: None, below: None, cooldown_s: 900.0, last: 0.0 }
    }
}

fn row(label: &str, symbol: &str, digits: u32, name: &str) -> Row {
    Row { label: label.into(), symbol: symbol.into(), digits, name: name.into() }
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
            placement: None,
            monitor: None,
            dock: true,
            dock_font_pt: 9,
            dock_rows: 2,
            x: None,
            y: None,
            theme: "midnight".into(),
            theme_font: None,
            alerts: Vec::new(),
            webhook: None,
        }
    }
}

impl Config {
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
        return true;
    }
    if cfg.rows.len() >= MAX_ROWS {
        return false;
    }
    let shown: Vec<String> = cfg.rows.iter().map(|r| r.symbol.clone()).collect();
    cfg.rows = menu_list(cfg).into_iter().filter(|r| r.symbol == symbol || shown.contains(&r.symbol)).collect();
    true
}

pub fn path() -> PathBuf {
    let p = profile();
    p.folder().join(p.config_file)
}

pub fn load() -> Config {
    let p = path();
    match std::fs::read_to_string(&p) {
        Ok(text) => {
            let cfg: Config = serde_json::from_str(&text).unwrap_or_default();
            // A config from before the Symbols menu has no catalog: write the default one in.
            if !text.contains("\"catalog\"") {
                save(&cfg);
            }
            cfg
        }
        Err(_) => {
            let cfg = Config::default();
            save(&cfg);
            cfg
        }
    }
}

pub fn save(cfg: &Config) {
    let p = path();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(text) = serde_json::to_string_pretty(cfg) {
        let tmp = p.with_extension("json.tmp");
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(&tmp, &p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn symbols(rows: &[Row]) -> Vec<&str> {
        rows.iter().map(|r| r.symbol.as_str()).collect()
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
        let mut full = Config { rows: default_catalog(), ..Config::default() };
        full.rows.truncate(MAX_ROWS);
        assert!(!toggle(&mut full, ".HSI"), "no more than MAX_ROWS");
    }

    #[test]
    fn hand_added_rows_stay_listed_and_in_place() {
        let mut c = Config::default();
        c.rows.push(row("SOL", "SOL=", 2, ""));
        assert_eq!(menu_list(&c).last().unwrap().symbol, "SOL=");
        assert!(toggle(&mut c, "ETH.CB="));
        assert_eq!(c.rows.last().unwrap().symbol, "SOL=");
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
    fn old_config_without_catalog_gets_the_default_one() {
        let c: Config = serde_json::from_str(r#"{"rows":[{"label":"XAU","symbol":"XAU=","digits":2}]}"#).unwrap();
        assert_eq!(c.rows.len(), 1);
        assert_eq!(c.catalog, default_catalog());
    }
}
