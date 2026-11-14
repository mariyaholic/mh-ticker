//! Render the panel with its candle pane open, from deterministic bars.
//!
//! The theme catalogue is `shots`; this is the one picture the catalogue cannot take, because the
//! chart is a live pane. It exists so the painter can be reviewed when the feed is closed or
//! blocked:
//!
//! ```text
//! cargo run --release --example chart --features shots -- shots-chart/ midnight 96 1488
//! cargo run --release --example chart --features shots -- shots-chart/ midnight 96 1488 4   # lock card 4
//! cargo run --release --example chart --features shots -- shots-chart/ midnight 96 1488 4 40  # and hover candle 40
//! cargo run --release --example chart --features shots -- shots-chart/ midnight 96 1488 1 40 refs  # reference lines and a break
//! cargo run --release --example chart --features shots -- shots-chart/ midnight 96 1488 1 40 - 5D     # a range (1D 5D 1M 6M 1Y 5Y) with a synthetic series
//! cargo run --release --example chart --features shots -- shots-chart/ midnight 96 1488 1 40 - 1D vol # volume strip, with the tabs
//! cargo run --release --example chart --features shots -- shots-chart/ midnight 96 1488 1 - - - - levels # alert levels: one inside, one above
//! ```

use mh_ticker::shot;

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = std::path::PathBuf::from(args.next().unwrap_or_else(|| "shots-chart".into()));
    let name = args.next().unwrap_or_else(|| "midnight".into());
    let dpi: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(96);
    let width: i32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(1488);
    // Optional 5th argument: lock card N, as clicking it would (the chart follows the lock).
    let hover: Option<usize> = args.next().and_then(|v| v.parse().ok());
    // Optional 6th argument: hover candle N (0 = leftmost as drawn), which draws the crosshair and legend.
    let bar: Option<usize> = args.next().and_then(|v| v.parse().ok());
    // Optional 7th argument: the word `refs` draws the pane's reference lines and one session break.
    let refs = args.next().as_deref() == Some("refs");
    // Optional 8th argument: a range (`1D`, `5D`, `1M`, `6M`, `1Y`, `5Y`; `-` keeps the pane as it was
    // before ranges). A range draws the tabs and charts a synthetic series shaped like that range's file.
    let range = args.next().filter(|r| r != "-");
    // Optional 9th argument: the word `vol` gives the charted bars a volume, which draws the strip.
    let volume = args.next().as_deref() == Some("vol");
    // Optional 10th argument: the word `levels` puts two alert levels on the charted symbol, one inside
    // the price window and one above it.
    let levels = args.next().as_deref() == Some("levels");
    match shot::write_chart_named_levels(&dir, name.as_str(), dpi, width, hover, bar, refs, range.as_deref(), volume, levels) {
        Ok(p) => println!("{}", p.display()),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
