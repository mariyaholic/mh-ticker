//! Renders W3's pictures (the 5Y pane with its header change and high/low marks, 24 cards, and the
//! strip with 6 of 24 symbols pinned) into the directory given as the first argument.
//!
//!     cargo run --release --example w3 --features shots -- shots/

fn main() {
    let dir = std::env::args().nth(1).unwrap_or_else(|| "shots".to_string());
    match mh_ticker::shot::write_w3(std::path::Path::new(&dir)) {
        Ok(n) => println!("{n} pictures in {dir}"),
        Err(e) => {
            eprintln!("w3: {e}");
            std::process::exit(1);
        }
    }
}
