//! Renders the wider set of pictures (24 cards, 24 symbols on 1, 2 and 3 strip rows, the narrow pane,
//! 192 DPI, each range) into the directory given as the first argument, beside the theme catalogue
//! and never over it.
//!
//!     cargo run --release --example wide --features shots -- shots/

fn main() {
    let dir = std::env::args().nth(1).unwrap_or_else(|| "shots".to_string());
    match mh_ticker::shot::write_wide(std::path::Path::new(&dir)) {
        Ok(n) => println!("\n{n} pictures in {dir}"),
        Err(e) => {
            eprintln!("wide: {e}");
            std::process::exit(1);
        }
    }
}
