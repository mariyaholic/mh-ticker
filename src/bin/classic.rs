//! MH_Ticker: the original build. One theme, one strip, one panel.
//!
//!     MH_Ticker            run the strip + tray
//!     MH_Ticker --probe    print one pass over the quotes and the hour of bars, then exit
//!
//! This file is intentionally almost empty: what makes the classic build "classic" is the profile
//! it passes, not a separate code path. It gets every engine fix the Ultra build gets, and its
//! look comes from `theme::classic()`, which is a single constant.

#![cfg_attr(not(test), windows_subsystem = "windows")]

fn main() {
    mh_ticker::launch(mh_ticker::config::Profile::classic());
}
