//! Renders W4's pictures (the up and down palettes, the contrast theme in the two fixed system
//! palettes, and panel zoom) into the directory given as the first argument.
//!
//!     cargo run --release --example w4 --features shots -- shots/

fn main() {
    let dir = std::env::args().nth(1).unwrap_or_else(|| "shots".to_string());
    match mh_ticker::shot::write_w4(std::path::Path::new(&dir)) {
        Ok(n) => println!("{n} pictures in {dir}"),
        Err(e) => {
            eprintln!("w4: {e}");
            std::process::exit(1);
        }
    }
}
