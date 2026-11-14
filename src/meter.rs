//! The data meter: how many bytes the app has taken off the wire this hour and since it started.
//! Pure and portable: `std` only. In memory, gone at quit; nothing is saved and nothing is sent.
//!
//! Sixty one-minute slots in a ring plus one running total, about half a kilobyte. A slot carries the
//! minute number it was last written in, so a slot left over from an earlier hour is recognised and
//! ignored without any timer sweeping it: nothing here runs unless a response arrives or the menu is
//! built. Bytes are counted as the program reads them (after the HTTP library has unpacked a
//! compressed body), so on the wire the figure is an upper bound when the answer is gzip.

/// Minutes in the ring: the span "this hour" covers.
pub const SLOTS: usize = 60;

#[derive(Clone, Debug)]
pub struct Meter {
    total: u64,
    /// Bytes counted in the minute named by `stamp[i]`.
    bytes: [u32; SLOTS],
    /// The minute number (`seconds / 60`) slot `i` was last written in; 0 means never.
    stamp: [u32; SLOTS],
}

impl Default for Meter {
    fn default() -> Self {
        Self::new()
    }
}

fn minute(now_s: f64) -> u32 {
    if now_s.is_finite() && now_s >= 60.0 { (now_s / 60.0).min(u32::MAX as f64) as u32 } else { 1 }
}

impl Meter {
    pub const fn new() -> Self {
        Meter { total: 0, bytes: [0; SLOTS], stamp: [0; SLOTS] }
    }

    /// Count `n` bytes read at `now_s` (UTC seconds).
    pub fn add(&mut self, now_s: f64, n: u64) {
        if n == 0 {
            return;
        }
        self.total = self.total.saturating_add(n);
        let m = minute(now_s);
        let i = m as usize % SLOTS;
        if self.stamp[i] != m {
            // The slot belongs to another minute (an hour ago, or a clock that went back): start over.
            self.stamp[i] = m;
            self.bytes[i] = 0;
        }
        self.bytes[i] = self.bytes[i].saturating_add(n.min(u32::MAX as u64) as u32);
    }

    /// Bytes counted in the last hour: the current minute and the 59 before it. A slot stamped in the
    /// future (the clock went back) is not counted.
    pub fn last_hour(&self, now_s: f64) -> u64 {
        let m = minute(now_s);
        let first = m.saturating_sub(SLOTS as u32 - 1);
        (0..SLOTS)
            .filter(|&i| self.stamp[i] != 0 && self.stamp[i] >= first && self.stamp[i] <= m)
            .map(|i| self.bytes[i] as u64)
            .sum()
    }

    /// Bytes counted since the app started.
    #[cfg(test)]
    pub fn since_start(&self) -> u64 {
        self.total
    }

    /// The greyed menu line: `Data: 3.1 MB this hour, 41 MB since start`.
    pub fn line(&self, now_s: f64) -> String {
        format!("Data: {} this hour, {} since start", size(self.last_hour(now_s)), size(self.total))
    }
}

/// `0 B`, `812 B`, `3.1 KB`, `41 MB` (one decimal under ten, none from ten up), `1.2 GB`. Decimal
/// units: a thousand, not 1,024.
pub fn size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    if bytes < 1000 {
        return format!("{bytes} B");
    }
    let mut v = bytes as f64;
    let mut u = 0;
    while v >= 1000.0 && u < UNITS.len() - 1 {
        v /= 1000.0;
        u += 1;
    }
    // 999.5 KB would print as 1000 KB: promote it.
    if v >= 999.5 && u < UNITS.len() - 1 {
        v /= 1000.0;
        u += 1;
    }
    if v < 9.95 { format!("{v:.1} {}", UNITS[u]) } else { format!("{v:.0} {}", UNITS[u]) }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: f64 = 1_800_000_000.0;

    #[test]
    fn bytes_add_up_this_hour_and_since_start() {
        let mut m = Meter::new();
        m.add(T0, 1_000_000);
        m.add(T0 + 30.0, 2_100_000);
        m.add(T0 + 600.0, 38_000_000);
        assert_eq!(m.last_hour(T0 + 600.0), 41_100_000);
        assert_eq!(m.since_start(), 41_100_000);
        assert_eq!(m.line(T0 + 600.0), "Data: 41 MB this hour, 41 MB since start");
    }

    #[test]
    fn the_hour_rolls_over_minute_by_minute() {
        let mut m = Meter::new();
        m.add(T0, 5_000);
        // 59 minutes later the first minute is still inside the hour, 60 minutes later it is not.
        let first = (T0 / 60.0).floor() * 60.0;
        assert_eq!(m.last_hour(first + 59.0 * 60.0), 5_000);
        assert_eq!(m.last_hour(first + 60.0 * 60.0), 0);
        assert_eq!(m.since_start(), 5_000, "the total never rolls");
        // A write into the slot an hour later replaces the old minute rather than adding to it.
        m.add(first + 3_600.0, 7);
        assert_eq!(m.last_hour(first + 3_600.0), 7);
        assert_eq!(m.since_start(), 5_007);
    }

    #[test]
    fn an_idle_hour_reads_zero_and_says_so() {
        let mut m = Meter::new();
        assert_eq!(m.line(T0), "Data: 0 B this hour, 0 B since start");
        m.add(T0, 3_100_000);
        assert_eq!(m.line(T0 + 7_200.0), "Data: 0 B this hour, 3.1 MB since start");
    }

    #[test]
    fn a_clock_set_back_does_not_count_the_future() {
        let mut m = Meter::new();
        m.add(T0 + 7_200.0, 9_000);
        assert_eq!(m.last_hour(T0), 0, "a write stamped after now is not this hour's");
        m.add(T0, 1_000);
        assert_eq!(m.last_hour(T0), 1_000);
        assert_eq!(m.since_start(), 10_000);
        // Non-finite or tiny clocks do not panic.
        m.add(f64::NAN, 1);
        assert_eq!(m.last_hour(f64::NAN), 1);
    }

    #[test]
    fn sizes_read_in_decimal_units() {
        for (n, want) in [
            (0, "0 B"),
            (999, "999 B"),
            (1_000, "1.0 KB"),
            (3_100, "3.1 KB"),
            (999_499, "999 KB"),
            (999_500, "1.0 MB"),
            (3_100_000, "3.1 MB"),
            (41_000_000, "41 MB"),
            (1_234_000_000, "1.2 GB"),
            (12_000_000_000_000, "12000 GB"),
        ] {
            assert_eq!(size(n), want, "{n}");
        }
    }

    #[test]
    fn the_ring_is_about_half_a_kilobyte() {
        assert!(std::mem::size_of::<Meter>() <= 512);
    }
}
