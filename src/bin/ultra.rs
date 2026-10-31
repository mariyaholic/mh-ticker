//! MH_Ticker_Ultra: the heavy build. Themes, a symbol picker in the panel, price alerts and an
//! optional webhook.
//!
//!     MH_Ticker_Ultra            run the strip + tray
//!     MH_Ticker_Ultra --probe    print one pass over the quotes and the hour of bars, then exit
//!
//! Same engine as the classic build, same network code, same full hour of history. The profile is
//! what differs: `config_ultra.json`, its own single-instance mutex, and the Ultra-only menu
//! entries. It costs more RAM than the classic build (a gradient, a picker strip, alert state)
//! which is exactly why the classic build exists.

#![cfg_attr(not(test), windows_subsystem = "windows")]

fn main() {
    mh_ticker::launch(mh_ticker::config::Profile::ultra());
}
