//! Render the panel with its candle pane open, from deterministic bars.
//!
//! The theme catalogue is `shots`; this is the one picture the catalogue cannot take, because the
//! chart is a live pane. It exists so the painter can be reviewed when the feed is closed or
//! blocked:
//!
//! ```text
//! cargo run --release --example chart --features shots -- shots-chart/ midnight 96 1488
//! cargo run --release --example chart --features shots -- shots-chart/ midnight 96 1488 4
//! ```

use mh_ticker::shot;

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = std::path::PathBuf::from(args.next().unwrap_or_else(|| "shots-chart".into()));
    let name = args.next().unwrap_or_else(|| "midnight".into());
    let dpi: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(96);
    let width: i32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(1488);
    // Optional 5th argument: hold card N under the pointer, as hovering it would.
    let hover: Option<usize> = args.next().and_then(|v| v.parse().ok());
    match shot::write_chart_named_at(&dir, name.as_str(), dpi, width, hover) {
        Ok(p) => println!("{}", p.display()),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
