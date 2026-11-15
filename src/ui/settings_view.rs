//! What the settings window shows: one page of controls per section, built from a `Config` and the
//! few facts that do not live in it (`Env`). Pure data, so the tests can build every page for both
//! builds and check that each former menu entry has a control.
//!
//! Every option carries the command id or `Action` the menu entry carried, and the window runs it
//! through `run_command` / `run_action`, the code the menu ran. Nothing here changes state.

use super::*;
use crate::menu_commands::Action;
use crate::settings::{opt, Act, Control, Opt, Section};

/// A display, as the Position choice lists it.
#[derive(Clone)]
pub(super) struct MonitorFact {
    pub device: String,
    pub primary: bool,
    pub w: i32,
    pub h: i32,
}

/// The facts a page needs besides the config.
pub(super) struct Env {
    pub ultra: bool,
    /// The panel is open: the strip's position cannot change then.
    pub open: bool,
    pub picked: bool,
    /// What Undo would undo, when something is held.
    pub undo: Option<&'static str>,
    pub status: String,
    pub meter: String,
    pub autostart: bool,
    pub hotkey_failed: bool,
    pub adding: Option<String>,
    pub monitors: Vec<MonitorFact>,
    pub alerts: AlertsView,
    pub reach: ReachView,
    pub save_failed: bool,
    pub kept_bad: Option<String>,
    pub update_running: bool,
    pub report_running: bool,
    pub version: String,
    /// The symbol whose card settings and alerts the pages show; the first shown symbol when `None` or
    /// when it is no longer shown.
    pub card: Option<String>,
    /// The decimals *Auto* gives each shown symbol (symbol, digits), from the live price.
    pub autos: Vec<(String, u32)>,
}

impl Default for Env {
    fn default() -> Env {
        Env {
            ultra: false,
            open: false,
            picked: false,
            undo: None,
            status: String::new(),
            meter: String::new(),
            autostart: false,
            hotkey_failed: false,
            adding: None,
            monitors: Vec::new(),
            alerts: AlertsView::default(),
            reach: ReachView { updown: Default::default(), hollow: false, contrast: false, animate: false, zoom: 100 },
            save_failed: false,
            kept_bad: None,
            update_running: false,
            report_running: false,
            version: String::new(),
            card: None,
            autos: Vec::new(),
        }
    }
}

fn cmd(id: usize) -> Act {
    Act::Cmd(id)
}

fn radio(label: &str, hint: &str, labels: Vec<String>, base: usize, current: usize, enabled: bool) -> Control {
    let opts = labels.into_iter().enumerate().map(|(i, l)| opt(l, cmd(base + i)).enabled(enabled)).collect();
    Control::choice(label, hint, opts, Some(current))
}

/// The controls of one section.
pub(super) fn build(cfg: &Config, env: &Env, section: Section) -> Vec<Control> {
    match section {
        Section::Symbols => symbols(cfg, env),
        Section::Display => display(cfg, env),
        Section::Chart => chart(env),
        Section::Alerts => alerts_page(cfg, env),
        Section::System => system(cfg, env),
        Section::About => about(env),
    }
}

/// The shown row the card settings and the alerts are about: the one `env.card` names, else the first.
fn selected_row<'a>(cfg: &'a Config, env: &Env) -> Option<(usize, &'a config::Row)> {
    let named = env.card.as_ref().and_then(|c| cfg.rows.iter().position(|r| &r.symbol == c));
    let i = named.or(if cfg.rows.is_empty() { None } else { Some(0) })?;
    Some((i, &cfg.rows[i]))
}

/// The choice that picks the symbol the rest of the page is about.
fn card_picker(cfg: &Config, current: usize, label: &str, hint: &str) -> Control {
    let opts = cfg.rows.iter().map(|r| opt(r.label.clone(), Act::Card(r.symbol.clone()))).collect();
    Control::choice(label, hint, opts, Some(current))
}

/// The per-symbol settings the card's right-click menu also offers: name, decimals, the taskbar strip pin
/// and, in Ultra, the position among the cards.
fn card_settings(cfg: &Config, env: &Env) -> Vec<Control> {
    let Some((i, r)) = selected_row(cfg, env) else { return Vec::new() };
    let mut v = vec![Control::header("Card settings")];
    v.push(card_picker(cfg, i, "Symbol", "the card these settings are for"));
    let renamed = config::derived_label(cfg, &r.symbol) != r.label;
    v.push(Control::choice(
        "Name",
        &format!("shown as {}", r.label),
        vec![opt("Rename...", Act::Do(Action::Rename(r.symbol.clone()))), opt("Reset name", Act::Do(Action::ResetName(r.symbol.clone()))).enabled(renamed)],
        None,
    ));
    let auto = env.autos.iter().find(|(s, _)| s == &r.symbol).map_or_else(|| config::auto_digits(&r.symbol, None), |(_, d)| *d);
    let mut digits = vec![opt(format!("Auto ({auto})"), Act::Do(Action::Digits(r.symbol.clone(), None))).checked(r.digits == auto)];
    for d in 0..=6u32 {
        digits.push(opt(d.to_string(), Act::Do(Action::Digits(r.symbol.clone(), Some(d)))).checked(r.digits == d && r.digits != auto));
    }
    v.push(Control::choice("Decimals", "digits after the point", digits, None));
    let pinned: Vec<bool> = cfg.rows.iter().map(|r| r.strip).collect();
    let can = if r.strip { crate::pins::can_unpin(&pinned, i) } else { pinned.len() > 1 };
    v.push(Control::toggle(
        "On the taskbar strip",
        if can { "the panel always shows every card" } else { "at least one symbol stays on the strip" },
        r.strip,
        Act::Do(Action::Pin(r.symbol.clone(), !r.strip)),
        can,
    ));
    if env.ultra {
        // The card menu's Move left, Move right and Move first, enabled by the same rule.
        let [left, right, first] = move_enabled(i, cfg.rows.len());
        let sym = &r.symbol;
        v.push(Control::choice(
            "Position",
            "among the cards",
            vec![
                opt("Move left", Act::Do(Action::Move(sym.clone(), -1))).enabled(left),
                opt("Move right", Act::Do(Action::Move(sym.clone(), 1))).enabled(right),
                opt("Move first", Act::Do(Action::MoveFirst(sym.clone()))).enabled(first),
            ],
            None,
        ));
    }
    v
}

fn symbols(cfg: &Config, env: &Env) -> Vec<Control> {
    let list = config::menu_list(cfg);
    let shown: Vec<bool> = list.iter().map(|r| config::is_shown(cfg, &r.symbol)).collect();
    let n_shown = shown.iter().filter(|s| **s).count();
    let full = n_shown >= config::MAX_ROWS;
    let mut v = vec![Control::header(&format!("Shown symbols ({n_shown}, at most {})", config::MAX_ROWS))];
    // One chip per catalogue entry: on means the symbol has a card. The last one cannot go and at most
    // MAX_ROWS have one. Whether a card is also on the taskbar strip is the Card settings toggle.
    let chips = list
        .iter()
        .zip(&shown)
        .map(|(r, on)| {
            let locked = if *on { n_shown <= 1 } else { full };
            opt(r.label.clone(), Act::Do(Action::Symbol(r.symbol.clone()))).checked(*on).enabled(!locked)
        })
        .collect();
    v.push(Control::choice("Symbols you follow", "tick to add a card, untick to remove it", chips, None));
    let adding = env.adding.as_deref();
    v.push(Control::button(
        "Add a symbol",
        "by its CNBC symbol",
        &adding.map_or("Add symbol...".to_string(), |s| format!("Adding {s}...")),
        cmd(CMD_ADD_SYMBOL),
        adding.is_none(),
    ));
    let custom = config::custom_entries(cfg);
    if !custom.is_empty() {
        let opts = custom
            .iter()
            .map(|r| {
                let last = n_shown <= 1 && list.iter().zip(&shown).any(|(l, on)| *on && l.symbol == r.symbol);
                opt(format!("{} ({})", r.label, r.symbol), Act::Do(Action::Forget(r.symbol.clone()))).enabled(!last)
            })
            .collect();
        v.push(Control::choice("Remove a symbol I added", "drops it from the catalogue", opts, None));
    }
    v.extend(card_settings(cfg, env));
    v.push(Control::header("Order and reset"));
    let order = |l: &str, id: usize| opt(l, cmd(id)).enabled(n_shown > 1);
    v.push(Control::choice(
        "Order the cards",
        "once, not a standing rule",
        vec![order("By day move, biggest first", CMD_ORDER_MOVE), order("By name, A to Z", CMD_ORDER_NAME), order("Default order", CMD_ORDER_DEFAULT)],
        None,
    ));
    let unpinned = cfg.rows.iter().any(|r| !r.strip);
    v.push(Control::button("Taskbar strip", "pin every symbol to it", "Show all on strip", cmd(CMD_PIN_ALL), unpinned));
    v.push(Control::button("Defaults", "the six starting symbols", "Reset to default", cmd(CMD_RESET), true));
    let undo = env.undo.map_or("Undo".to_string(), |k| format!("Undo the last {k}"));
    v.push(Control::button("Undo", "puts the last change back", &undo, cmd(CMD_UNDO), env.undo.is_some()));
    v
}

fn display(cfg: &Config, env: &Env) -> Vec<Control> {
    let docked = cfg.placement().on_taskbar();
    let mut v = vec![Control::header("Strip")];
    v.push(Control::toggle("Show strip", "", cfg.strip_visible, cmd(CMD_STRIP), true));
    v.push(Control::toggle(
        "Always on top",
        if docked { "on while docked to the taskbar" } else { "" },
        cfg.topmost || docked,
        cmd(CMD_TOP),
        !docked,
    ));
    let placement = cfg.placement();
    let cur = Placement::ALL.iter().position(|p| *p == placement).unwrap_or(0);
    v.push(radio("Position", if env.open { "back to the taskbar to change it" } else { "" }, Placement::ALL.iter().map(|p| p.label().to_string()).collect(), CMD_POS, cur, !env.open));
    if env.monitors.len() > 1 {
        let primary = env.monitors.iter().position(|d| d.primary).unwrap_or(0);
        let active = env.monitors.iter().position(|d| Some(&d.device) == cfg.monitor.as_ref()).unwrap_or(primary);
        let opts = env
            .monitors
            .iter()
            .enumerate()
            .map(|(i, d)| {
                let main = if d.primary { ", main" } else { "" };
                // The choice moves the strip and applies only to a strip, like Position.
                opt(format!("Display {}{main} ({} x {})", i + 1, d.w, d.h), Act::Do(Action::Monitor((!d.primary).then(|| d.device.clone())))).enabled(!env.open)
            })
            .collect();
        v.push(Control::choice("Display", if env.open { "back to the taskbar to change it" } else { "which screen the strip uses" }, opts, Some(active)));
    }
    let rows = STRIP_ROWS.iter().map(|n| format!("{n} row{}", if *n == 1 { "" } else { "s" })).collect();
    v.push(radio("Rows in the taskbar", if docked { "" } else { "applies on the taskbar" }, rows, CMD_STRIP_ROWS, nearest_rows(cfg.dock_rows), docked));
    let text = TEXT_STEPS.iter().map(|(_, _, l)| l.to_string()).collect();
    v.push(radio("Text size", "", text, CMD_STRIP_TEXT, nearest_text(cfg.dock_font_pt, cfg.font_pt), true));
    let opacity = OPACITY_STEPS.iter().map(|(_, l)| l.to_string()).collect();
    v.push(radio("Opacity", "", opacity, CMD_STRIP_OPACITY, nearest_opacity(cfg.opacity), true));
    if env.ultra {
        v.push(Control::header("Theme and font"));
        let cur = theme::index_of(&cfg.theme);
        for kind in theme::Kind::ALL {
            let opts = theme::tier(kind).map(|i| opt(theme::ALL[i].name, cmd(CMD_THEME + i)).checked(i == cur)).collect();
            v.push(Control::choice(kind.label(), kind.note(), opts, None));
        }
        let face = theme::font_of(&theme::ALL[cur], cfg.theme_font.as_deref());
        for group in theme::FACE_GROUPS {
            // A face this machine cannot draw is greyed rather than hidden.
            let opts = theme::FACES
                .iter()
                .enumerate()
                .filter(|(_, f)| f.group == group)
                .map(|(i, f)| opt(f.name, cmd(CMD_FONT + i)).checked(f.name == face).enabled(theme::face_available(f.name)))
                .collect();
            let note = theme::FACES.iter().find(|f| f.group == group && f.name == face).map_or("", |f| f.note);
            v.push(Control::choice(&format!("Font: {group}"), note, opts, None));
        }
        v.push(Control::toggle("Symbol picker", "the chip list under the cards", env.picked, cmd(CMD_PICK), true));
    }
    v.push(Control::header("Accessibility"));
    v.push(Control::toggle("Contrast theme", "follow the Windows contrast theme", env.reach.contrast, cmd(CMD_CONTRAST), true));
    v.push(Control::toggle("Animation", "follow the Windows animation setting", env.reach.animate, cmd(CMD_ANIMATE), true));
    if env.ultra {
        let sizes = crate::zoom::STEPS.iter().map(|z| format!("{z} %")).collect();
        let cur = crate::zoom::STEPS.iter().position(|z| *z == env.reach.zoom).unwrap_or(2);
        v.push(radio("Panel size", "", sizes, CMD_ZOOM, cur, true));
    }
    v
}

fn chart(env: &Env) -> Vec<Control> {
    let mut v = vec![Control::header("Candles")];
    let modes = crate::updown::UpDown::ALL;
    let cur = modes.iter().position(|m| *m == env.reach.updown).unwrap_or(0);
    v.push(radio("Up and down colours", "", modes.iter().map(|m| m.label().to_string()).collect(), CMD_UPDOWN, cur, true));
    v.push(Control::toggle("Hollow up candles", "up candles drawn as outlines", env.reach.hollow, cmd(CMD_HOLLOW), true));
    v
}

fn alerts_page(cfg: &Config, env: &Env) -> Vec<Control> {
    let av = &env.alerts;
    let list = config::menu_list(cfg);
    let shown: Vec<bool> = list.iter().map(|r| config::is_shown(cfg, &r.symbol)).collect();
    let shown_labels: Vec<&str> = list.iter().zip(&shown).filter(|(_, on)| **on).map(|(r, _)| r.label.as_str()).collect();
    let alerted = armed_labels(&cfg.alerts, &shown_labels);
    let mut v = vec![Control::header("Every alert")];
    let (cur_repeat, _) = if av.once {
        (1, "once")
    } else if av.crossing {
        (2, "each crossing")
    } else {
        (0, "every cooldown")
    };
    v.push(Control::choice(
        "Repeat",
        "how an alert comes back after it fires",
        vec![opt("Every cooldown", cmd(CMD_REPEAT_COOLDOWN)), opt("Fire once, then clear", cmd(CMD_ALERT_ONCE)), opt("Each crossing", cmd(CMD_REPEAT_CROSSING))],
        Some(cur_repeat),
    ));
    let (muted_text, is_muted) = match av.silence {
        Some(alerts::Silence::Muted) => ("Muted".to_string(), true),
        Some(alerts::Silence::MutedUntil(t)) => (format!("Muted until {}", alerts::local_hhmm(&local_offset_at, av.now, t)), true),
        _ => ("Not muted".to_string(), false),
    };
    v.push(Control::choice(
        "Mute",
        &muted_text,
        vec![
            opt("For 1 hour", cmd(CMD_MUTE_1H)),
            opt("For 8 hours", cmd(CMD_MUTE_8H)),
            opt("Until a time...", cmd(CMD_MUTE_UNTIL)),
            opt("Until I turn it back on", cmd(CMD_MUTE_INDEF)),
            opt("Unmute", cmd(CMD_UNMUTE)).enabled(is_muted),
        ],
        None,
    ));
    let mut quiet = vec![opt("Off", cmd(CMD_QUIET_OFF))];
    for (i, [from, to]) in alerts::QUIET_CHOICES.iter().enumerate() {
        quiet.push(opt(format!("{} to {}", alerts::hhmm(*from), alerts::hhmm(*to)), cmd(CMD_QUIET + i)));
    }
    let custom_quiet = av.quiet.filter(|w| w[0] != w[1] && !alerts::QUIET_CHOICES.contains(w));
    quiet.push(opt(custom_quiet.map_or("Type hours...".to_string(), |w| format!("{}...", crate::typed::span_text(w))), cmd(CMD_QUIET_TYPE)).checked(custom_quiet.is_some()));
    let cur_quiet = match av.quiet {
        None => Some(0),
        Some(w) => alerts::QUIET_CHOICES.iter().position(|c| *c == w).map(|i| i + 1),
    };
    v.push(Control::choice("Quiet hours", "local time", quiet, cur_quiet));

    v.push(Control::header("Webhook"));
    let has_hook = av.host.is_some();
    v.push(Control::note(&av.host.as_ref().map_or("(no webhook set)".to_string(), |h| format!("Sends to {h}"))));
    v.push(Control::choice(
        "Webhook",
        "only the host is ever shown",
        vec![
            opt("Paste URL from clipboard", cmd(CMD_HOOK_PASTE)),
            opt("Send a test alert", cmd(CMD_HOOK_TEST)).enabled(has_hook),
            opt("Remove", cmd(CMD_HOOK_REMOVE)).enabled(has_hook),
        ],
        None,
    ));
    v.push(Control::toggle("Alert gaps", "say when alerts could not be checked", av.gap_notice, cmd(CMD_GAP_NOTICE), has_hook));

    v.push(Control::header("Alerts for a symbol"));
    let picked = selected_row(cfg, env);
    if let Some((i, _)) = picked {
        v.push(card_picker(cfg, i, "Symbol", "the alerts below are for this one"));
    }
    if av.rows.is_empty() {
        v.push(Control::note("(no symbols shown)"));
    }
    // Rows are matched by symbol: two cards can carry the same label after a rename.
    let picked_symbol = picked.map(|(_, r)| r.symbol.as_str());
    for row in av.rows.iter().filter(|r| picked_symbol.is_none() || Some(r.symbol.as_str()) == picked_symbol) {
        let line = row.alert.as_ref().map(|a| alerts::describe(a, row.digits)).filter(|d| !d.is_empty());
        // The presets are split where the menu drew a separator: one control per group, the first one
        // carrying the symbol's name and its current alert.
        let mut first = true;
        for group in 0..=alerts::PRESETS.iter().map(|p| p.group).max().unwrap_or(0) {
            let presets: Vec<Opt> = alerts::PRESETS
                .iter()
                .enumerate()
                .filter(|(_, p)| p.group == group)
                .map(|(k, p)| opt(p.label, Act::Do(Action::Alert(row.label.clone(), k))).enabled(row.enabled.get(k).copied().unwrap_or(false)))
                .collect();
            if presets.is_empty() {
                continue;
            }
            let (name, hint) = if first { (row.label.as_str(), line.clone().unwrap_or_else(|| "nothing set".into())) } else { ("", String::new()) };
            v.push(Control::choice(name, &hint, presets, None));
            first = false;
        }
        let cur_fast = row.alert.as_ref().map_or(0, |a| a.fast as usize);
        let fast = (0..alerts::FAST_MOVES.len())
            .map(|k| opt(format!("fast move {}", alerts::fast_label(k)), Act::Do(Action::Fast(row.label.clone(), k))).checked(cur_fast == k + 1))
            .collect();
        v.push(Control::choice(&format!("{} fast move", row.label), "the current one again turns it off", fast, None));
        let cooldown = row.alert.as_ref().map(|a| a.cooldown_s);
        let cooldown_text = cooldown.map_or("cooldown...".to_string(), |c| format!("cooldown {}...", crate::typed::cooldown_text(c)));
        v.push(Control::choice(
            &format!("{} extras", row.label),
            "",
            vec![
                opt("at a price...", Act::Do(Action::AlertAt(row.label.clone()))).enabled(row.fresh),
                opt(cooldown_text, Act::Do(Action::Cooldown(row.label.clone()))).enabled(cooldown.is_some()),
                opt("Clear this symbol's alerts", Act::Do(Action::ClearAlert(row.label.clone()))).enabled(alerted.contains(&row.label)),
            ],
            None,
        ));
    }

    let cannot_fire = av.overview.iter().filter(|l| l.warn).count();
    v.push(Control::header(&if cannot_fire > 0 {
        format!("At a glance ({}, {cannot_fire} cannot fire)", av.overview.len())
    } else {
        format!("At a glance ({})", av.overview.len())
    }));
    if av.overview.is_empty() {
        v.push(Control::note("(nothing set)"));
    }
    for line in &av.overview {
        v.push(Control::note(&line.text));
    }
    v.push(Control::button("All alerts", "asks before it clears", "Clear all alerts...", cmd(CMD_ALERTS_CLEAR_ALL), !av.overview.is_empty()));

    v.push(Control::header(&format!("Recent ({})", av.log.len())));
    if av.log.is_empty() {
        v.push(Control::note("(nothing yet)"));
    }
    for e in &av.log {
        v.push(Control::note(&alerts::entry_line(e, &local_offset_at, av.now)));
    }
    v.push(Control::choice(
        "Recent list",
        "",
        vec![opt("Copy list", cmd(CMD_LOG_COPY)).enabled(!av.log.is_empty()), opt("Clear list", cmd(CMD_LOG_CLEAR)).enabled(!av.log.is_empty())],
        None,
    ));
    v
}

fn system(cfg: &Config, env: &Env) -> Vec<Control> {
    let mut v = vec![Control::header("Startup")];
    v.push(Control::toggle("Start with Windows", "", env.autostart, cmd(CMD_AUTOSTART), true));
    // Hotkey: Off, the presets, and the configured combo when it is none of them.
    let parsed = cfg.hotkey.as_deref().and_then(hotkey::parse);
    let preset = cfg.hotkey.as_deref().and_then(hotkey::preset_index);
    let custom = cfg.hotkey.as_ref().filter(|_| preset.is_none());
    let note = if !env.hotkey_failed { "" } else if parsed.is_some() { " (in use by another app)" } else { " (not a valid hotkey)" };
    let mut keys = vec![opt("Off", cmd(CMD_HOTKEY))];
    for (i, p) in hotkey::PRESETS.iter().enumerate() {
        let tail = if preset == Some(i) { note } else { "" };
        keys.push(opt(format!("{p}{tail}"), cmd(CMD_HOTKEY + 1 + i)));
    }
    if let Some(text) = custom {
        let shown = parsed.map(|c| hotkey::label(&c)).unwrap_or_else(|| text.chars().take(24).collect());
        keys.push(opt(format!("{shown}{note}"), cmd(CMD_HOTKEY_CUSTOM)));
    }
    let selected = match (&cfg.hotkey, preset) {
        (None, _) => 0,
        (Some(_), Some(i)) => 1 + i,
        (Some(_), None) => keys.len() - 1,
    };
    keys.push(opt("Type a hotkey...", cmd(CMD_HOTKEY_TYPE)));
    v.push(Control::choice("Hotkey", "opens and closes the panel", keys, Some(selected)));
    v.push(Control::header("Prices"));
    let cur_poll = POLL_CHOICES.iter().position(|secs| (secs - cfg.poll_s).abs() < 0.01).unwrap_or(2);
    v.push(radio("Poll every", "while prices move", POLL_CHOICES.iter().map(|s| format!("{s} s")).collect(), CMD_POLL, cur_poll, true));
    let gov = cfg.governor(env.ultra);
    v.push(Control::toggle("Slow when quiet", "slow down when nothing moves; never below one poll in 15 s with an alert armed", gov.slow_when_quiet, cmd(CMD_SLOW_QUIET), true));
    v.push(Control::toggle("Rest when locked", "rest while the PC is locked", gov.rest_when_locked, cmd(CMD_REST_LOCKED), true));
    v.push(Control::toggle("Allow fallback sources", "Gold API or Kraken when CNBC is unavailable", cfg.fallback_enabled, cmd(CMD_FALLBACK), true));
    v.push(Control::choice("Now", "", vec![opt("Refresh now", cmd(CMD_REFRESH)), opt("Copy all prices", cmd(CMD_COPY_ALL))], None));
    v
}

fn about(env: &Env) -> Vec<Control> {
    let mut v = vec![Control::header(&env.version)];
    v.push(Control::note(&env.status));
    v.push(Control::note(&env.meter));
    if env.save_failed {
        v.push(Control::note("Settings could not be saved (changes last until you quit)"));
    }
    if let Some(name) = &env.kept_bad {
        v.push(Control::note(&format!("Settings were unreadable; kept as {name}, defaults loaded")));
    }
    v.push(Control::button(
        "Updates",
        "",
        if env.update_running { "Checking..." } else { "Check for updates" },
        cmd(CMD_UPDATE),
        !env.update_running,
    ));
    v.push(Control::button(
        "Diagnostics",
        "a report you can paste into an issue",
        if env.report_running { "Diagnostics report: checking..." } else { "Copy diagnostics report" },
        cmd(CMD_REPORT),
        !env.report_running,
    ));
    v.push(Control::button("Settings file", "", "Open config folder", cmd(CMD_FOLDER), true));
    v.push(Control::button("Exit", "", "Quit", cmd(CMD_EXIT), true));
    v
}

/// The page the still renderer draws: the default config with `theme`, two displays, an alert on the
/// first two symbols and a webhook host.
#[cfg(feature = "shots")]
pub(super) fn fixture(ultra: bool, theme_name: &str, section: Section) -> Vec<Control> {
    let mut cfg = Config::default();
    cfg.theme = theme_name.to_string();
    cfg.hotkey = Some(hotkey::PRESETS[0].to_string());
    cfg.alerts = cfg.rows.iter().take(2).map(|r| config::Alert { label: r.label.clone(), ..Default::default() }).collect();
    let mut env = Env { ultra, ..Env::default() };
    env.status = "live".into();
    env.meter = "data used this month: 12 MB".into();
    env.autostart = true;
    env.version = format!("{} {}", if ultra { "MH Ticker Ultra" } else { "MH Ticker" }, env!("CARGO_PKG_VERSION"));
    env.monitors = vec![
        MonitorFact { device: "A".into(), primary: true, w: 2560, h: 1440 },
        MonitorFact { device: "B".into(), primary: false, w: 1920, h: 1080 },
    ];
    env.alerts.host = Some("hooks.example.com".into());
    env.alerts.rows = cfg
        .rows
        .iter()
        .enumerate()
        .map(|(i, r)| AlertsRow {
            symbol: r.symbol.clone(),
            label: r.label.clone(),
            digits: r.digits,
            enabled: (0..alerts::PRESETS.len()).map(|k| (k + i) % 4 != 0).collect(),
            fresh: true,
            alert: cfg.alerts.iter().find(|a| a.label == r.label).cloned(),
        })
        .collect();
    build(&cfg, &env, section)
}

/// The option a control would run, for tests and for the window's activation.
pub(super) fn opt_of(controls: &[Control], ci: usize, oi: usize) -> Option<&Opt> {
    controls.get(ci)?.opts.get(oi)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{self, Kind};

    fn all_acts(cfg: &Config, env: &Env) -> Vec<Act> {
        Section::for_build(env.ultra).into_iter().flat_map(|s| build(cfg, env, s)).flat_map(|c| c.opts).map(|o| o.act).collect()
    }

    fn rich_env(ultra: bool) -> (Config, Env) {
        let mut cfg = Config { hotkey: Some("Ctrl+Alt+Q".into()), ..Config::default() };
        cfg.rows.push(config::Row { label: "ZZZ".into(), symbol: "ZZZ.TEST".into(), digits: 2, name: String::new(), strip: true });
        let mut env = Env { ultra, ..Env::default() };
        env.monitors = vec![
            MonitorFact { device: "A".into(), primary: true, w: 1920, h: 1080 },
            MonitorFact { device: "B".into(), primary: false, w: 2560, h: 1440 },
        ];
        env.undo = Some("removal");
        env.adding = None;
        let labels: Vec<String> = cfg.rows.iter().map(|r| r.label.clone()).collect();
        env.alerts.rows = labels
            .iter()
            .map(|l| AlertsRow { symbol: cfg.rows.iter().find(|r| &r.label == l).map_or_else(String::new, |r| r.symbol.clone()), label: l.clone(), digits: 2, enabled: vec![true; alerts::PRESETS.len()], fresh: true, alert: Some(config::Alert { label: l.clone(), ..Default::default() }) })
            .collect();
        env.alerts.host = Some("hooks.example".into());
        env.alerts.quiet = Some([21 * 60, 6 * 60]);
        cfg.alerts = labels.iter().map(|l| config::Alert { label: l.clone(), ..Default::default() }).collect();
        (cfg, env)
    }

    fn has_cmd(acts: &[Act], id: usize) -> bool {
        acts.contains(&Act::Cmd(id))
    }

    /// Every fixed command the 1.9 menu listed, for both builds.
    fn both_builds() -> Vec<usize> {
        let mut v = vec![
            CMD_STRIP, CMD_TOP, CMD_FOLDER, CMD_EXIT, CMD_RESET, CMD_FALLBACK, CMD_REFRESH, CMD_COPY_ALL, CMD_AUTOSTART, CMD_UPDATE, CMD_REPORT,
            CMD_HOTKEY, CMD_HOTKEY_CUSTOM, CMD_HOTKEY_TYPE, CMD_SLOW_QUIET, CMD_REST_LOCKED, CMD_ORDER_MOVE, CMD_ORDER_NAME, CMD_ORDER_DEFAULT, CMD_PIN_ALL,
            CMD_UNDO, CMD_CONTRAST, CMD_ANIMATE, CMD_ADD_SYMBOL,
        ];
        v.extend((0..Placement::ALL.len()).map(|i| CMD_POS + i));
        v.extend((0..POLL_CHOICES.len()).map(|i| CMD_POLL + i));
        v.extend((0..hotkey::PRESETS.len()).map(|i| CMD_HOTKEY + 1 + i));
        v.extend((0..STRIP_ROWS.len()).map(|i| CMD_STRIP_ROWS + i));
        v.extend((0..TEXT_STEPS.len()).map(|i| CMD_STRIP_TEXT + i));
        v.extend((0..OPACITY_STEPS.len()).map(|i| CMD_STRIP_OPACITY + i));
        v
    }

    fn ultra_only() -> Vec<usize> {
        let mut v = vec![
            CMD_PICK, CMD_HOLLOW, CMD_ALERT_ONCE, CMD_REPEAT_COOLDOWN, CMD_REPEAT_CROSSING, CMD_MUTE_1H, CMD_MUTE_8H, CMD_MUTE_INDEF, CMD_UNMUTE, CMD_MUTE_UNTIL,
            CMD_QUIET_OFF, CMD_QUIET_TYPE, CMD_HOOK_PASTE, CMD_HOOK_TEST, CMD_HOOK_REMOVE, CMD_GAP_NOTICE, CMD_LOG_COPY, CMD_LOG_CLEAR, CMD_ALERTS_CLEAR_ALL,
        ];
        v.extend((0..theme::ALL.len()).map(|i| CMD_THEME + i));
        v.extend((0..theme::FACES.len()).map(|i| CMD_FONT + i));
        v.extend((0..crate::updown::UpDown::ALL.len()).map(|i| CMD_UPDOWN + i));
        v.extend((0..crate::zoom::STEPS.len()).map(|i| CMD_ZOOM + i));
        v.extend((0..alerts::QUIET_CHOICES.len()).map(|i| CMD_QUIET + i));
        v
    }

    #[test]
    fn every_former_menu_command_has_a_control_in_ultra() {
        let (cfg, env) = rich_env(true);
        let acts = all_acts(&cfg, &env);
        for id in both_builds().into_iter().chain(ultra_only()) {
            assert!(has_cmd(&acts, id), "command {id} has no control");
        }
    }

    #[test]
    fn classic_has_every_shared_command_and_none_of_ultras() {
        let (cfg, env) = rich_env(false);
        let acts = all_acts(&cfg, &env);
        for id in both_builds() {
            assert!(has_cmd(&acts, id), "command {id} has no control in Classic");
        }
        for id in ultra_only() {
            assert!(!has_cmd(&acts, id), "command {id} must not appear in Classic");
        }
    }

    #[test]
    fn every_dynamic_action_the_menu_listed_has_a_control() {
        let (cfg, mut env) = rich_env(true);
        // A symbol the user added, so the Forget chip exists.
        let custom = config::custom_entries(&cfg);
        let acts = all_acts(&cfg, &env);
        let any = |f: &dyn Fn(&Action) -> bool| acts.iter().any(|a| matches!(a, Act::Do(x) if f(x)));
        assert!(any(&|a| matches!(a, Action::Symbol(_))));
        assert!(any(&|a| matches!(a, Action::Alert(_, 0))));
        assert!(any(&|a| matches!(a, Action::Alert(_, k) if *k == alerts::PRESETS.len() - 1)));
        assert!(any(&|a| matches!(a, Action::Fast(_, 2))));
        assert!(any(&|a| matches!(a, Action::ClearAlert(_))));
        assert!(any(&|a| matches!(a, Action::AlertAt(_))));
        assert!(any(&|a| matches!(a, Action::Cooldown(_))));
        assert!(any(&|a| matches!(a, Action::Monitor(None))));
        assert!(any(&|a| matches!(a, Action::Monitor(Some(d)) if d == "B")));
        assert_eq!(any(&|a| matches!(a, Action::Forget(_))), !custom.is_empty());
        // The card menu's per-symbol entries.
        assert!(any(&|a| matches!(a, Action::Rename(_))));
        assert!(any(&|a| matches!(a, Action::ResetName(_))));
        assert!(any(&|a| matches!(a, Action::Digits(_, None))));
        assert!((0..=6).all(|d| any(&|a| matches!(a, Action::Digits(_, Some(x)) if *x == d))));
        assert!(any(&|a| matches!(a, Action::Pin(_, false))), "a pinned symbol offers to unpin");
        // One monitor: no Display choice.
        env.monitors.truncate(1);
        let one = all_acts(&cfg, &env);
        assert!(!one.iter().any(|a| matches!(a, Act::Do(Action::Monitor(_)))));
    }

    #[test]
    fn every_symbol_in_the_catalogue_has_a_chip_and_the_shown_ones_are_on() {
        let (cfg, env) = rich_env(true);
        let page = build(&cfg, &env, Section::Symbols);
        let chips = &page.iter().find(|c| c.label == "Symbols you follow").unwrap().opts;
        let list = config::menu_list(&cfg);
        assert_eq!(chips.len(), list.len());
        for (chip, row) in chips.iter().zip(&list) {
            assert_eq!(chip.act, Act::Do(Action::Symbol(row.symbol.clone())));
            assert_eq!(chip.on, config::is_shown(&cfg, &row.symbol), "{}", row.label);
        }
    }

    #[test]
    fn the_last_shown_symbol_cannot_be_hidden_and_a_full_strip_cannot_grow() {
        let (mut cfg, env) = rich_env(true);
        cfg.rows.truncate(1);
        let page = build(&cfg, &env, Section::Symbols);
        let chips = &page.iter().find(|c| c.label == "Symbols you follow").unwrap().opts;
        assert!(chips.iter().filter(|c| c.on).all(|c| !c.enabled), "the only shown symbol is locked on");
        assert!(chips.iter().filter(|c| !c.on).all(|c| c.enabled));
    }

    /// Audit 2026-11, display-choice-ignored-with-panel-open: the Display choice applies only to a
    /// strip, so it is greyed with the same hint as Position while the panel is open.
    #[test]
    fn display_is_greyed_with_the_panel_open_as_position_is() {
        let (cfg, mut env) = rich_env(true);
        let pick = |env: &Env| {
            let page = build(&cfg, env, Section::Display);
            let find = |label: &str| page.iter().find(|c| c.label == label).cloned().unwrap();
            (find("Position"), find("Display"))
        };
        let (position, display) = pick(&env);
        assert!(position.opts.iter().all(|o| o.enabled) && display.opts.iter().all(|o| o.enabled), "a strip can move");
        assert_eq!(display.hint, "which screen the strip uses");
        env.open = true;
        let (position, display) = pick(&env);
        assert!(position.opts.iter().all(|o| !o.enabled));
        assert!(display.opts.iter().all(|o| !o.enabled), "the pick would be dropped, so it is not offered");
        assert_eq!(display.hint, position.hint, "the same hint as Position");
        assert!(display.opts.iter().any(|o| o.on), "the current display stays marked");
    }

    #[test]
    fn radio_choices_mark_what_the_config_holds() {
        let (mut cfg, env) = rich_env(true);
        cfg.poll_s = 10.0;
        cfg.opacity = 205;
        cfg.theme = "midnight".into();
        let sys = build(&cfg, &env, Section::System);
        let poll = sys.iter().find(|c| c.label == "Poll every").unwrap();
        assert_eq!(poll.opts.iter().position(|o| o.on), POLL_CHOICES.iter().position(|s| *s == 10.0));
        let disp = build(&cfg, &env, Section::Display);
        let op = disp.iter().find(|c| c.label == "Opacity").unwrap();
        assert_eq!(op.opts.iter().position(|o| o.on), Some(2));
        let on: Vec<&str> = disp.iter().flat_map(|c| &c.opts).filter(|o| o.on && matches!(o.act, Act::Cmd(c) if (CMD_THEME..CMD_THEME + theme::ALL.len()).contains(&c))).map(|o| o.label.as_str()).collect();
        assert_eq!(on, ["midnight"], "exactly the configured theme is marked");
    }

    #[test]
    fn toggles_show_the_state_the_menu_check_mark_showed() {
        let (mut cfg, mut env) = rich_env(true);
        cfg.strip_visible = false;
        env.autostart = true;
        env.reach.hollow = true;
        let find = |sec: Section, label: &str, cfg: &Config, env: &Env| build(cfg, env, sec).into_iter().find(|c| c.label == label).unwrap();
        assert!(!find(Section::Display, "Show strip", &cfg, &env).on);
        assert!(find(Section::System, "Start with Windows", &cfg, &env).on);
        assert!(find(Section::Chart, "Hollow up candles", &cfg, &env).on);
        cfg.strip_visible = true;
        assert!(find(Section::Display, "Show strip", &cfg, &env).on);
        // Docked: always on top is shown on and cannot be changed, as the menu greyed it.
        cfg.set_placement(Placement::Auto);
        let top = find(Section::Display, "Always on top", &cfg, &env);
        assert!(top.on && !top.opts[0].enabled);
    }

    #[test]
    fn position_is_locked_while_the_panel_is_open() {
        let (cfg, mut env) = rich_env(true);
        env.open = true;
        let pos = build(&cfg, &env, Section::Display).into_iter().find(|c| c.label == "Position").unwrap();
        assert!(pos.opts.iter().all(|o| !o.enabled));
        env.open = false;
        let pos = build(&cfg, &env, Section::Display).into_iter().find(|c| c.label == "Position").unwrap();
        assert!(pos.opts.iter().all(|o| o.enabled));
    }

    #[test]
    fn the_hotkey_choice_marks_off_a_preset_or_the_typed_combo() {
        let (mut cfg, env) = rich_env(true);
        let marked = |cfg: &Config| {
            let c = build(cfg, &env, Section::System).into_iter().find(|c| c.label == "Hotkey").unwrap();
            c.opts.iter().position(|o| o.on).map(|i| c.opts[i].label.clone())
        };
        cfg.hotkey = None;
        assert_eq!(marked(&cfg).as_deref(), Some("Off"));
        cfg.hotkey = Some(hotkey::PRESETS[1].to_string());
        assert_eq!(marked(&cfg).as_deref(), Some(hotkey::PRESETS[1]));
        cfg.hotkey = Some("Ctrl+Alt+Q".into());
        assert!(marked(&cfg).is_some_and(|l| l.contains('Q')), "the typed combo is listed and marked");
    }

    #[test]
    fn no_control_label_needs_more_than_one_line_at_the_default_window_size() {
        let (w, _) = settings::window_size(96);
        let lw = settings::label_width(96, w);
        for ultra in [false, true] {
            let (cfg, env) = rich_env(ultra);
            for s in Section::for_build(ultra) {
                for c in build(&cfg, &env, s).iter().filter(|c| matches!(c.kind, Kind::Toggle | Kind::Choice | Kind::Button)) {
                    let (names, hints) = settings::label_lines(c, lw, 96);
                    assert!(names.len() <= 1, "label {:?} wraps in {s:?}", c.label);
                    assert!(hints.len() <= 3, "hint {:?} needs {} lines in {s:?}", c.hint, hints.len());
                    for l in names.iter() {
                        assert!(l.chars().count() as i32 * 8 <= lw, "{l:?} is wider than the label column {lw}");
                    }
                }
            }
        }
    }

    #[test]
    fn alert_presets_are_split_where_the_menu_had_separators() {
        let (cfg, env) = rich_env(true);
        let page = build(&cfg, &env, Section::Alerts);
        let first = cfg.rows[0].label.as_str();
        let at = page.iter().position(|c| c.label == first).expect("the first symbol has a preset row");
        let groups = alerts::PRESETS.iter().map(|p| p.group).max().unwrap() as usize + 1;
        let shown: usize = page[at..at + groups].iter().map(|c| c.opts.len()).sum();
        assert_eq!(shown, alerts::PRESETS.len(), "every preset appears once across the group rows");
        assert!(page[at + 1..at + groups].iter().all(|c| c.label.is_empty()), "only the first row carries the name");
    }

    #[test]
    fn a_font_group_shows_the_note_of_the_face_in_use() {
        let (mut cfg, env) = rich_env(true);
        cfg.theme_font = Some("Georgia".into());
        let page = build(&cfg, &env, Section::Display);
        let serif = page.iter().find(|c| c.label == "Font: Serif").unwrap();
        assert_eq!(serif.hint, "warm, screen-safe");
    }

    #[test]
    fn every_page_has_focusable_controls_and_every_option_has_text() {
        for ultra in [false, true] {
            let (cfg, env) = rich_env(ultra);
            for s in Section::for_build(ultra) {
                let page = build(&cfg, &env, s);
                assert!(!page.is_empty(), "{s:?}");
                assert!(page.iter().any(|c| c.focusable()) , "{s:?} has nothing to focus");
                for c in &page {
                    for o in &c.opts {
                        assert!(!o.label.trim().is_empty(), "{} has an option with no text", c.label);
                    }
                    if matches!(c.kind, Kind::Toggle | Kind::Button) {
                        assert_eq!(c.opts.len(), 1, "{}", c.label);
                    }
                }
            }
        }
    }

    #[test]
    fn every_page_lays_out_without_overlap_at_every_scale() {
        for ultra in [false, true] {
            let (cfg, env) = rich_env(ultra);
            for s in Section::for_build(ultra) {
                let page = build(&cfg, &env, s);
                for eff in [96u32, 120, 144, 192, 72, 86] {
                    let (w, h) = settings::window_size(eff);
                    let l = settings::layout(eff, w, h, 6, &page, 0);
                    for pair in l.rows.windows(2) {
                        assert!(pair[0].bounds.bottom <= pair[1].bounds.top);
                    }
                    for (row, c) in l.rows.iter().zip(&page) {
                        for r in &row.opts {
                            assert!(r.right <= l.viewport.right, "{} overflows at {eff}", c.label);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn about_reports_a_failed_save_and_a_kept_config() {
        let (cfg, mut env) = rich_env(true);
        env.save_failed = true;
        env.kept_bad = Some("config.bad".into());
        let notes: Vec<String> = build(&cfg, &env, Section::About).into_iter().filter(|c| c.kind == Kind::Note).map(|c| c.label).collect();
        assert!(notes.iter().any(|n| n.contains("could not be saved")));
        assert!(notes.iter().any(|n| n.contains("config.bad")));
        env.update_running = true;
        let upd = build(&cfg, &env, Section::About).into_iter().find(|c| c.label == "Updates").unwrap();
        assert!(!upd.opts[0].enabled, "one update check at a time");
    }

    fn find_in(page: &[Control], label: &str) -> usize {
        page.iter().position(|c| c.label == label).unwrap_or_else(|| panic!("no control {label:?}"))
    }

    #[test]
    fn an_unpinned_symbol_offers_pin_and_the_last_pinned_one_cannot_be_unpinned() {
        let (mut cfg, mut env) = rich_env(true);
        env.card = Some(cfg.rows[1].symbol.clone());
        cfg.rows[1].strip = false;
        let page = build(&cfg, &env, Section::Symbols);
        let pin = &page[find_in(&page, "On the taskbar strip")];
        assert!(!pin.on);
        assert_eq!(pin.opts[0].act, Act::Do(Action::Pin(cfg.rows[1].symbol.clone(), true)));
        assert!(pin.opts[0].enabled, "several rows: pinning is allowed");
        // Only one row stays pinned: it cannot be unpinned.
        for (k, r) in cfg.rows.iter_mut().enumerate() {
            r.strip = k == 0;
        }
        env.card = Some(cfg.rows[0].symbol.clone());
        let page = build(&cfg, &env, Section::Symbols);
        let pin = &page[find_in(&page, "On the taskbar strip")];
        assert!(pin.on);
        assert_eq!(pin.opts[0].act, Act::Do(Action::Pin(cfg.rows[0].symbol.clone(), false)));
        assert!(!pin.opts[0].enabled);
    }

    #[test]
    fn rename_is_always_offered_and_reset_name_only_when_the_label_was_changed() {
        let (mut cfg, mut env) = rich_env(true);
        env.card = Some(cfg.rows[0].symbol.clone());
        let name = |cfg: &Config, env: &Env| {
            let page = build(cfg, env, Section::Symbols);
            page[find_in(&page, "Name")].clone()
        };
        let c = name(&cfg, &env);
        assert_eq!(c.opts[0].act, Act::Do(Action::Rename(cfg.rows[0].symbol.clone())));
        assert!(c.opts[0].enabled);
        assert_eq!(c.opts[1].act, Act::Do(Action::ResetName(cfg.rows[0].symbol.clone())));
        assert!(!c.opts[1].enabled, "the label is the derived one");
        cfg.rows[0].label = "My name".into();
        let c = name(&cfg, &env);
        assert!(c.opts[1].enabled);
        assert!(c.hint.contains("My name"));
    }

    #[test]
    fn decimals_mark_auto_or_the_chosen_count_like_the_menu_did() {
        let (mut cfg, mut env) = rich_env(true);
        let sym = cfg.rows[0].symbol.clone();
        env.card = Some(sym.clone());
        env.autos = vec![(sym.clone(), 2)];
        let marked = |cfg: &Config, env: &Env| {
            let page = build(cfg, env, Section::Symbols);
            let c = &page[find_in(&page, "Decimals")];
            assert_eq!(c.opts.len(), 8, "Auto and 0 to 6");
            c.opts.iter().position(|o| o.on)
        };
        cfg.rows[0].digits = 2;
        assert_eq!(marked(&cfg, &env), Some(0), "the count equals what Auto gives");
        cfg.rows[0].digits = 4;
        assert_eq!(marked(&cfg, &env), Some(1 + 4));
        let page = build(&cfg, &env, Section::Symbols);
        assert_eq!(page[find_in(&page, "Decimals")].opts[0].label, "Auto (2)");
        assert_eq!(page[find_in(&page, "Decimals")].opts[3].act, Act::Do(Action::Digits(sym, Some(2))));
    }

    #[test]
    fn the_card_picker_defaults_to_the_first_shown_symbol_and_follows_the_choice() {
        let (cfg, mut env) = rich_env(true);
        let picker = |cfg: &Config, env: &Env| {
            let page = build(cfg, env, Section::Symbols);
            let c = page[find_in(&page, "Symbol")].clone();
            (c.sel, c.opts.len(), c.opts.iter().map(|o| o.act.clone()).collect::<Vec<_>>())
        };
        let (sel, n, acts) = picker(&cfg, &env);
        assert_eq!(sel, Some(0));
        assert_eq!(n, cfg.rows.len());
        assert_eq!(acts[2], Act::Card(cfg.rows[2].symbol.clone()));
        env.card = Some(cfg.rows[2].symbol.clone());
        assert_eq!(picker(&cfg, &env).0, Some(2));
        env.card = Some("NOT.SHOWN".into());
        assert_eq!(picker(&cfg, &env).0, Some(0), "a symbol that is no longer shown falls back to the first");
    }

    #[test]
    fn the_alerts_page_shows_the_alert_rows_of_the_picked_symbol_only() {
        let (cfg, mut env) = rich_env(true);
        let rows = |cfg: &Config, env: &Env| {
            let page = build(cfg, env, Section::Alerts);
            let named = |l: &str| page.iter().any(|c| c.label == l);
            (named(&format!("{} fast move", cfg.rows[0].label)), named(&format!("{} fast move", cfg.rows[1].label)), page.len())
        };
        let (first, second, len_first) = rows(&cfg, &env);
        assert!(first && !second);
        env.card = Some(cfg.rows[1].symbol.clone());
        let (first, second, len_second) = rows(&cfg, &env);
        assert!(!first && second);
        assert_eq!(len_first, len_second, "the page is as long for any symbol");
        // Every preset of every symbol is still reachable by choosing the symbol.
        for r in &cfg.rows {
            env.card = Some(r.symbol.clone());
            let acts = all_acts(&cfg, &env);
            assert!(acts.contains(&Act::Do(Action::Alert(r.label.clone(), 0))), "{}", r.label);
        }
    }

    #[test]
    fn the_position_control_runs_the_card_menus_moves_with_the_menus_enablement() {
        let (cfg, mut env) = rich_env(true);
        let last = cfg.rows.len() - 1;
        let pos = |cfg: &Config, env: &Env| {
            let page = build(cfg, env, Section::Symbols);
            let c = page[find_in(&page, "Position")].clone();
            (c.opts.iter().map(|o| (o.label.clone(), o.act.clone(), o.enabled)).collect::<Vec<_>>(), page.len())
        };
        for (at, want) in [(0, [false, true, false]), (1, [true, true, true]), (last, [true, false, true])] {
            env.card = Some(cfg.rows[at].symbol.clone());
            let (opts, _) = pos(&cfg, &env);
            let sym = cfg.rows[at].symbol.clone();
            assert_eq!(opts[0], ("Move left".to_string(), Act::Do(Action::Move(sym.clone(), -1)), want[0]), "row {at}");
            assert_eq!(opts[1], ("Move right".to_string(), Act::Do(Action::Move(sym.clone(), 1)), want[1]), "row {at}");
            assert_eq!(opts[2], ("Move first".to_string(), Act::Do(Action::MoveFirst(sym)), want[2]), "row {at}");
            assert_eq!(want, move_enabled(at, cfg.rows.len()), "the card menu uses the same rule");
        }
        // One card: nothing to move.
        let mut one = cfg.clone();
        one.rows.truncate(1);
        env.card = None;
        assert!(pos(&one, &env).0.iter().all(|o| !o.2));
    }

    #[test]
    fn classic_has_no_position_control_because_it_has_no_card_ordering() {
        let (cfg, env) = rich_env(false);
        let page = build(&cfg, &env, Section::Symbols);
        assert!(!page.iter().any(|c| c.label == "Position"));
    }

    #[test]
    fn move_enablement_follows_the_place_among_the_cards() {
        assert_eq!(move_enabled(0, 1), [false, false, false]);
        assert_eq!(move_enabled(0, 3), [false, true, false]);
        assert_eq!(move_enabled(1, 3), [true, true, true]);
        assert_eq!(move_enabled(2, 3), [true, false, true]);
    }

    #[test]
    fn two_cards_with_the_same_label_do_not_share_their_alert_rows() {
        let (mut cfg, mut env) = rich_env(true);
        // The second card is renamed to the first one's label.
        let (a, b) = (cfg.rows[0].symbol.clone(), cfg.rows[1].symbol.clone());
        cfg.rows[1].label = cfg.rows[0].label.clone();
        env.alerts.rows[1].label = cfg.rows[0].label.clone();
        let acts_for = |sym: &str, env: &mut Env| {
            env.card = Some(sym.to_string());
            let page = build(&cfg, env, Section::Alerts);
            page.iter().filter(|c| c.label.ends_with(" fast move")).count()
        };
        assert_eq!(acts_for(&a, &mut env), 1, "one symbol's alert block, not both");
        assert_eq!(acts_for(&b, &mut env), 1);
        // The block shown is the picked symbol's own: its alert line is the one of that row.
        env.alerts.rows[0].alert.as_mut().unwrap().cooldown_s = 111.0;
        env.alerts.rows[1].alert.as_mut().unwrap().cooldown_s = 222.0;
        let cooldown = |sym: &str, env: &mut Env| {
            env.card = Some(sym.to_string());
            let page = build(&cfg, env, Section::Alerts);
            page.iter().flat_map(|c| &c.opts).find(|o| o.label.starts_with("cooldown ")).map(|o| o.label.clone()).unwrap()
        };
        assert_ne!(cooldown(&a, &mut env), cooldown(&b, &mut env));
    }

    #[test]
    fn the_symbols_header_does_not_read_as_a_share_of_the_catalogue() {
        let (cfg, env) = rich_env(true);
        let page = build(&cfg, &env, Section::Symbols);
        let h = &page[0].label;
        assert!(h.starts_with("Shown symbols (") && h.contains(", at most 24)") && !h.contains(" of "), "{h}");
        assert_eq!(page[find_in(&page, "Symbols you follow")].hint, "tick to add a card, untick to remove it");
    }

    #[test]
    fn focus_on_undo_stays_on_undo_when_a_custom_symbol_adds_a_control_above_it() {
        let (mut cfg, mut env) = rich_env(true);
        cfg.rows.pop();
        env.undo = Some("removal");
        let old = build(&cfg, &env, Section::Symbols);
        let at = find_in(&old, "Undo");
        let focus = settings::Hit::Opt(at, 0);
        // A symbol is added to the catalogue: "Remove a symbol I added" appears above Undo.
        cfg.catalog.push(config::Row { label: "NEW".into(), symbol: "NEW.TEST".into(), digits: 2, name: String::new(), strip: true });
        let new = build(&cfg, &env, Section::Symbols);
        assert!(new.len() > old.len(), "the page gained a control");
        let f = settings::settle(focus, &old, 0, 6, &new);
        let settings::Hit::Opt(ci, oi) = f else { panic!("the focus left the controls: {f:?}") };
        assert_eq!(new[ci].opts[oi].act, Act::Cmd(CMD_UNDO), "the focus stays on Undo");
        assert_ne!(new[ci].opts[oi].act, Act::Cmd(CMD_RESET), "and never lands on Reset to default");
        assert_eq!(new[ci].label, "Undo");
    }
}
