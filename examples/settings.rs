//! Renders the settings window's pictures into the directory given as the first argument.
//!
//!     cargo run --release --example settings --features shots -- <dir>

fn main() {
    let dir = std::env::args().nth(1).unwrap_or_else(|| "shots".to_string());
    match mh_ticker::shot::write_settings(std::path::Path::new(&dir)) {
        Ok(n) => println!("{n} pictures in {dir}"),
        Err(e) => {
            eprintln!("settings: {e}");
            std::process::exit(1);
        }
    }
}
