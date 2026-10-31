//! Renders one PNG per theme (`theme::ALL`) into the directory given as the first argument.
//!
//!     cargo run --release --example shots --features shots -- shots/
//!
//! It is an example, not a binary, and it needs the `shots` feature, so neither shipped exe links
//! the PNG encoder. See `src/shot.rs` for what it draws and why.

fn main() {
    let dir = std::env::args().nth(1).unwrap_or_else(|| "shots".to_string());
    match mh_ticker::shot::write_all(std::path::Path::new(&dir)) {
        Ok(n) => println!("\n{n} screenshots in {dir}"),
        Err(e) => {
            eprintln!("shots: {e}");
            std::process::exit(1);
        }
    }
}
