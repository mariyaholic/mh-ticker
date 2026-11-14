//! Time the hot paths of the panel painter. Under Wine the GDI half is emulated, so read the numbers
//! as a ratio between runs, not as Windows milliseconds; the `fold` line is plain CPU and is real.
//!
//! ```text
//! cargo run --release --example perf --features shots
//! ```

#[cfg(not(feature = "shots"))]
fn main() {}

#[cfg(feature = "shots")]
fn main() {
    use mh_ticker::shot;
    let iters = 200;
    println!("panel draw + copy to the window, ms per frame ({iters} frames):");
    println!("  chart closed          {:8.3}", shot::bench_draw(None, false, false, iters).unwrap());
    for r in ["1D", "5D", "1M", "5Y"] {
        println!("  chart {r} still        {:8.3}", shot::bench_draw(Some(r), false, false, iters).unwrap());
        println!("  chart {r} hover        {:8.3}", shot::bench_draw(Some(r), true, false, iters).unwrap());
        println!("  chart {r} hover, clip  {:8.3}", shot::bench_draw(Some(r), true, true, iters).unwrap());
    }
    println!("fitted_to over one-minute bars, microseconds per call:");
    for (n, w) in [(390, 1000), (1_950, 1000), (4_000, 1000), (4_000, 300)] {
        println!("  {n:5} bars, plot {w:4} px  {:9.1}", shot::bench_fold(n, w, 2_000));
    }
    let (m, o) = shot::bench_text(20_000);
    println!("GDI text, microseconds per call: measure {m:.2}, draw {o:.2}");
    // What `draw_panel` builds before every frame, picker open or not.
    let cfg = mh_ticker::config::Config::default();
    let t = std::time::Instant::now();
    let mut k = 0;
    for _ in 0..20_000 {
        let list = mh_ticker::config::menu_list(&cfg);
        let shown: Vec<bool> = list.iter().map(|r| mh_ticker::config::is_shown(&cfg, &r.symbol)).collect();
        k += list.len() + shown.len();
    }
    std::hint::black_box(k);
    println!("menu_list + shown ({} rows), microseconds per frame: {:.2}", cfg.catalog.len(), t.elapsed().as_secs_f64() * 1e6 / 20_000.0);
}
