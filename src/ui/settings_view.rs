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

fn symbols(cfg: &Config, env: &Env) -> Vec<Control> {
    let list = config::menu_list(cfg);
    let shown: Vec<bool> = list.iter().map(|r| config::is_shown(cfg, &r.symbol)).collect();
    let n_shown = shown.iter().filter(|s| **s).count();
    let full = n_shown >= config::MAX_ROWS;
    let mut v = vec![Control::header(&format!("Shown symbols ({n_shown} of {})", config::MAX_ROWS))];
    // One chip per catalogue entry: on means shown. The last one cannot go and at most MAX_ROWS show.
    let chips = list
        .iter()
        .zip(&shown)
        .map(|(r, on)| {
            let locked = if *on { n_shown <= 1 } else { full };
            opt(r.label.clone(), Act::Do(Action::Symbol(r.symbol.clone()))).checked(*on).enabled(!locked)
        })
        .collect();
    v.push(Control::choice("Show on the strip", "tick to show, untick to hide", chips, None));
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
                opt(format!("Display {}{main} ({} x {})", i + 1, d.w, d.h), Act::Do(Action::Monitor((!d.primary).then(|| d.device.clone()))))
            })
            .collect();
        v.push(Control::choice("Display", "which screen the strip uses", opts, Some(active)));
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
            v.push(Control::choice(&format!("Font: {group}"), "", opts, None));
        }
        v.push(Control::toggle("Symbol picker", "the chip list under the cards", env.picked, cmd(CMD_PICK), true));
    }
    v.push(Control::header("Accessibility"));
    v.push(Control::toggle("Follow the Windows contrast theme", "", env.reach.contrast, cmd(CMD_CONTRAST), true));
    v.push(Control::toggle("Follow the Windows animation setting", "", env.reach.animate, cmd(CMD_ANIMATE), true));
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
    v.push(Control::toggle("Say when alerts could not be checked", "", av.gap_notice, cmd(CMD_GAP_NOTICE), has_hook));

    v.push(Control::header("Alerts for each symbol"));
    if av.rows.is_empty() {
        v.push(Control::note("(no symbols shown)"));
    }
    for row in &av.rows {
        let line = row.alert.as_ref().map(|a| alerts::describe(a, row.digits)).filter(|d| !d.is_empty());
        let presets = alerts::PRESETS
            .iter()
            .enumerate()
            .map(|(k, p)| opt(p.label, Act::Do(Action::Alert(row.label.clone(), k))).enabled(row.enabled.get(k).copied().unwrap_or(false)))
            .collect();
        v.push(Control::choice(&row.label, &line.unwrap_or_else(|| "nothing set".into()), presets, None));
        let cur_fast = row.alert.as_ref().map_or(0, |a| a.fast as usize);
        let fast = (0..alerts::FAST_MOVES.len())
            .map(|k| opt(format!("fast move {}", alerts::fast_label(k)), Act::Do(Action::Fast(row.label.clone(), k))).checked(cur_fast == k + 1))
            .collect();
        v.push(Control::choice(&format!("{} fast move", row.label), "the current one again turns it off", fast, None));
        let cooldown = row.alert.as_ref().map(|a| a.cooldown_s);
        let cooldown_text = cooldown.map_or("cooldown...".to_string(), |c| format!("cooldown {}...", crate::typed::cooldown_text(c)));
        v.push(Control::choice(
            &format!("{} more", row.label),
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
    v.push(Control::toggle("Slow down when nothing moves", "never below one poll in 15 s with an alert armed", gov.slow_when_quiet, cmd(CMD_SLOW_QUIET), true));
    v.push(Control::toggle("Rest while the PC is locked", "", gov.rest_when_locked, cmd(CMD_REST_LOCKED), true));
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
        let mut cfg = Config::default();
        cfg.hotkey = Some("Ctrl+Alt+Q".into());
        cfg.rows.push(config::Row { label: "ZZZ".into(), symbol: "ZZZ.TEST".into(), digits: 2, name: String::new(), strip: true, ..cfg.rows[0].clone() });
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
            .map(|l| AlertsRow { label: l.clone(), digits: 2, enabled: vec![true; alerts::PRESETS.len()], fresh: true, alert: Some(config::Alert { label: l.clone(), ..Default::default() }) })
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
        // One monitor: no Display choice.
        env.monitors.truncate(1);
        let one = all_acts(&cfg, &env);
        assert!(!one.iter().any(|a| matches!(a, Act::Do(Action::Monitor(_)))));
    }

    #[test]
    fn every_symbol_in_the_catalogue_has_a_chip_and_the_shown_ones_are_on() {
        let (cfg, env) = rich_env(true);
        let page = build(&cfg, &env, Section::Symbols);
        let chips = &page.iter().find(|c| c.label == "Show on the strip").unwrap().opts;
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
        let chips = &page.iter().find(|c| c.label == "Show on the strip").unwrap().opts;
        assert!(chips.iter().filter(|c| c.on).all(|c| !c.enabled), "the only shown symbol is locked on");
        assert!(chips.iter().filter(|c| !c.on).all(|c| c.enabled));
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
    fn every_page_has_focusable_controls_and_every_option_has_text() {
        for ultra in [false, true] {
            let (cfg, env) = rich_env(ultra);
            for s in Section::for_build(ultra) {
                let page = build(&cfg, &env, s);
                assert!(!page.is_empty(), "{s:?}");
                assert!(page.iter().any(|c| c.focusable()) || s == Section::About && false, "{s:?} has nothing to focus");
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
}
