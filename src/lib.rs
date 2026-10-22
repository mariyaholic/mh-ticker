//! MH Ticker, the engine. Two products link this one crate:
//!
//! | Binary | Profile | What it adds |
//! |---|---|---|
//! | `MH_Ticker.exe` | `Profile::classic()` | nothing — one theme, the strip, the panel |
//! | `MH_Ticker_Ultra.exe` | `Profile::ultra()` | themes, symbol picker, alerts, webhooks |
//!
//! The split is deliberate and one-directional: **the aesthetics live in Ultra, the engine lives
//! here.** Every correctness and network fix (the gzip feature, the day-based line colour, the
//! full hour of history, the poll cadence) lands in both builds because both link this crate, and
//! the classic build's look cannot drift because its palette is a single `const`.
//!
//! The profile is chosen before anything else touches the disk: `config::init` fixes the config
//! file and the single-instance mutex, so the two builds can run side by side and never share
//! settings.

// The crate's public surface is exactly two things: `launch` and `config::Profile`. Everything else
// is `pub(crate)`, which is not cosmetic — while this was a binary nobody could reach these
// functions, and now that it is a library they are API. Marking them crate-private keeps raw Win32
// handles (`HDC`, `HWND`, `HBRUSH`) from becoming something a caller can dereference, which is what
// `clippy::not_unsafe_ptr_arg_deref` exists to prevent.
pub mod config;
/// The theme screenshot renderer. Off by default, and the only thing that needs a PNG encoder, so
/// neither shipped binary links one. `cargo run --example shots --features shots`.
#[cfg(feature = "shots")]
pub mod shot;
pub(crate) mod alerts;
pub(crate) mod dock;
pub(crate) mod model;
pub(crate) mod panel;
pub(crate) mod source;
pub(crate) mod theme;
pub(crate) mod ui;

use std::sync::{Arc, Mutex};

/// The whole of a binary's `main`: fix the profile, then hand over to the engine.
///
/// `--probe` prints one pass over the quotes and the hour of bars and exits — the same diagnostic
/// in both builds, which is what keeps "does it still fetch and parse" a one-command check.
pub fn launch(profile: config::Profile) {
    config::init(profile);
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
