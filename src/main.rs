//! MH Ticker: a price strip docked in the Windows taskbar that opens into a centred panel,
//! with a tray icon to expand it or quit. Prices come from CNBC's public quote service: no
//! login, no account, no local server.
//!
//!     MH_Ticker            run the strip + tray
//!     MH_Ticker --probe    print one pass over the quotes and the hour of bars, then exit

#![cfg_attr(not(test), windows_subsystem = "windows")]

mod config;
mod dock;
mod model;
mod panel;
mod source;
mod ui;

use std::sync::{Arc, Mutex};

fn main() {
    let cfg = config::load();
    if std::env::args().any(|a| a == "--probe") {
        source::probe(&cfg);
        return;
    }
    if !ui::single_instance() {
        return;
    }
    let board = Arc::new(Mutex::new(model::Board::new(&cfg.rows)));
    ui::run(cfg, board);
}
