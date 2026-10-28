//! Feed thread: public CNBC quotes, no login, no server.
//!
//! - Quotes: one batched request for every row (`restQuote`), every `poll_s` (3 s). The connection
//!   is kept alive between requests. On failure it backs off (6, 12, 24 ... 60 s).
//! - History: CNBC's 1-day chart gives 1-minute bars per symbol; the last hour of them seeds the
//!   panel's sparklines at start and every 10 min (retry in 1 min).
//!
//! Gold, silver and bitcoin are real-time on CNBC; its index futures are marked delayed (~10 min)
//! and drawn as such. These endpoints are public but unofficial: CNBC can change them.

use crate::config::{Config, Row};
use crate::model::{resolve, Board, Link, Quote};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const QUOTE_URL: &str = "https://quote.cnbc.com/quote-html-webservice/restQuote/symbolType/symbol";
const CHART_URL: &str = "https://ts-api.cnbc.com/harmony/app/charts/1D.json";
const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
const BARS_REFRESH: Duration = Duration::from_secs(600);
const BARS_RETRY: Duration = Duration::from_secs(60);
/// While waiting for the next poll, how often to look for a Symbols-menu change.
const ROWS_CHECK: Duration = Duration::from_millis(250);
const HOUR_S: f64 = 3600.0;

pub fn now_s() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

/// "4,155.24" / "+1.22%" / 4155.24 -> 4155.24
fn number(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().trim_end_matches('%').replace([',', '+'], "").parse().ok(),
        _ => None,
    }
}

fn flag(v: Option<&Value>) -> bool {
    match v {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => s.eq_ignore_ascii_case("true"),
        _ => false,
    }
}

/// Days from 1970-01-01 to a civil date (proleptic Gregorian).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// "2026-10-15T04:29:41.313-0400" -> UTC epoch seconds. A bare date (a closed cash market's
/// last close) has no time of day and returns None, so the row reads as stale.
pub fn parse_time(s: &str) -> Option<f64> {
    let (date, rest) = s.split_once('T')?;
    let mut d = date.split('-').map(|p| p.parse::<i64>().ok());
    let (y, mo, da) = (d.next()??, d.next()??, d.next()??);
    let split = rest.rfind(['+', '-']).filter(|&i| i >= 8)?;
    let (clock, zone) = rest.split_at(split);
    let mut c = clock.split(':');
    let h: f64 = c.next()?.parse().ok()?;
    let mi: f64 = c.next()?.parse().ok()?;
    let se: f64 = c.next().unwrap_or("0").parse().ok()?;
    let sign = if zone.starts_with('-') { -1.0 } else { 1.0 };
    let z = zone[1..].replace(':', "");
    let zh: f64 = z.get(0..2)?.parse().ok()?;
    let zm: f64 = z.get(2..4).unwrap_or("0").parse().ok()?;
    let local = days_from_civil(y, mo, da) as f64 * 86_400.0 + h * 3600.0 + mi * 60.0 + se;
    Some(local - sign * (zh * 3600.0 + zm * 60.0))
}

/// `restQuote` body -> quotes.
pub fn parse_quotes(v: &Value) -> Vec<Quote> {
    let rows = v.pointer("/FormattedQuoteResult/FormattedQuote").and_then(Value::as_array);
    rows.map(|rows| {
        rows.iter()
            .filter_map(|r| {
                let symbol = r.get("symbol")?.as_str()?.to_string();
                let price = r.get("last").and_then(number).filter(|p| *p > 0.0)?;
                Some(Quote {
                    symbol,
                    name: r.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
                    price,
                    change_pct: r.get("change_pct").and_then(number),
                    prev_close: r.get("previous_day_closing").and_then(number),
                    ts: r.get("last_time").and_then(Value::as_str).and_then(parse_time),
                    realtime: flag(r.get("realTime")),
                })
            })
            .collect()
    })
    .unwrap_or_default()
}

/// 1-day chart body -> the last hour as (utc_s, close), stamped at each bar's end (open + 60 s)
/// and capped at `now` for the bar still forming.
pub fn parse_chart(v: &Value, now: f64) -> Vec<(f64, f64)> {
    let bars = v.pointer("/barData/priceBars").and_then(Value::as_array);
    bars.map(|bars| {
        bars.iter()
            .filter_map(|b| {
                let t = b.get("tradeTimeinMills").and_then(number)? / 1000.0 + 60.0;
                let close = b.get("close").and_then(number)?;
                (t >= now - HOUR_S - 60.0).then_some((t.min(now), close))
            })
            .collect()
    })
    .unwrap_or_default()
}

fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

pub struct Client {
    agent: ureq::Agent,
}

impl Client {
    pub fn new() -> Result<Self, String> {
        let tls = native_tls::TlsConnector::new().map_err(|e| e.to_string())?;
        let agent = ureq::AgentBuilder::new()
            .tls_connector(Arc::new(tls))
            .user_agent(UA)
            .timeout(Duration::from_secs(10))
            .build();
        Ok(Self { agent })
    }

    fn get(&self, url: &str) -> Result<Value, String> {
        match self.agent.get(url).call() {
            Ok(resp) => resp.into_json::<Value>().map_err(|e| e.to_string()),
            Err(ureq::Error::Status(code, _)) => Err(format!("HTTP {code}")),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn quotes(&self, rows: &[Row]) -> Result<Vec<Quote>, String> {
        let symbols: Vec<&str> = rows.iter().map(|r| r.symbol.as_str()).collect();
        let url = format!(
            "{QUOTE_URL}?symbols={}&requestMethod=itv&noform=1&partnerId=2&fund=1&exthrs=1&output=json",
            encode(&symbols.join("|"))
        );
        let qs = parse_quotes(&self.get(&url)?);
        if qs.is_empty() {
            return Err("CNBC answered with no quotes".into());
        }
        Ok(qs)
    }

    pub fn chart(&self, symbol: &str) -> Result<Vec<(f64, f64)>, String> {
        Ok(parse_chart(&self.get(&format!("{CHART_URL}?symbol={}", encode(symbol)))?, now_s()))
    }
}

struct Feed {
    cfg: Config,
    /// The rows this thread polls; adopted from `Board::rows` when `rows_gen` moves.
    rows: Vec<Row>,
    rows_gen: u64,
    shared: Arc<Mutex<Board>>,
    notify: Box<dyn Fn() + Send>,
    client: Client,
    quotes: HashMap<String, Quote>,
    backfill: Vec<Vec<(f64, f64)>>,
    backfill_gen: u64,
    bars_next: Instant,
    fails: u32,
}

impl Feed {
    /// Adopt the UI's row list if the Symbols menu changed it; the hour of bars is refetched.
    fn sync_rows(&mut self) {
        if let Ok(b) = self.shared.lock() {
            if b.rows_gen != self.rows_gen {
                self.rows = b.rows.clone();
                self.rows_gen = b.rows_gen;
                self.backfill.clear();
                self.bars_next = Instant::now();
            }
        }
    }

    fn publish(&self, link: Link, note: String) {
        if let Ok(mut b) = self.shared.lock() {
            // The UI may have changed the rows since this pass started: its cells win until the
            // next pass polls the new list.
            if b.rows_gen != self.rows_gen {
                return;
            }
            b.cells = resolve(&self.rows, &self.quotes, now_s(), self.cfg.stale_after_s, self.cfg.delayed_grace_s);
            b.link = link;
            b.note = note;
            if b.backfill_gen != self.backfill_gen {
                b.backfill = self.backfill.clone();
                b.backfill_gen = self.backfill_gen;
            }
        }
        (self.notify)();
    }

    /// Sleep until the next poll, waking early when the Symbols menu changes the rows so a new
    /// symbol shows up within a fraction of a second.
    fn wait(&self, d: Duration) {
        let until = Instant::now() + d;
        while Instant::now() < until {
            thread::sleep(ROWS_CHECK.min(until.saturating_duration_since(Instant::now())));
            if self.shared.lock().map(|b| b.rows_gen != self.rows_gen).unwrap_or(false) {
                return;
            }
        }
    }

    /// Fetch the hour of 1-minute bars when due; true when a fresh set arrived.
    fn maybe_bars(&mut self) -> bool {
        if Instant::now() < self.bars_next {
            return false;
        }
        let got: Vec<Vec<(f64, f64)>> = self.rows.iter().map(|r| self.client.chart(&r.symbol).unwrap_or_default()).collect();
        if got.iter().any(|p| !p.is_empty()) {
            self.backfill = got;
            self.backfill_gen += 1;
            self.bars_next = Instant::now() + BARS_REFRESH;
            true
        } else {
            self.bars_next = Instant::now() + BARS_RETRY;
            false
        }
    }

    fn run(mut self) {
        loop {
            self.sync_rows();
            let (link, note) = match self.client.quotes(&self.rows) {
                Ok(qs) => {
                    self.fails = 0;
                    for q in qs {
                        self.quotes.insert(q.symbol.clone(), q);
                    }
                    let cells =
                        resolve(&self.rows, &self.quotes, now_s(), self.cfg.stale_after_s, self.cfg.delayed_grace_s);
                    let lagging: Vec<&str> = cells.iter().filter(|c| c.stale).map(|c| c.label.as_str()).collect();
                    if lagging.is_empty() {
                        (Link::Live, String::new())
                    } else {
                        (Link::Partial, format!("no fresh price: {} (market closed?)", lagging.join(", ")))
                    }
                }
                Err(e) => {
                    self.fails = self.fails.saturating_add(1);
                    (Link::Down, format!("CNBC: {e}"))
                }
            };
            self.publish(link, note.clone());
            // Bars after the quotes, so a symbol just ticked in the menu shows its price at once.
            if self.maybe_bars() {
                self.publish(link, note);
            }
            let wait = if self.fails == 0 {
                self.cfg.poll_s.max(1.0)
            } else {
                (self.cfg.poll_s.max(1.0) * 2f64.powi(self.fails.min(6) as i32)).min(60.0)
            };
            self.wait(Duration::from_secs_f64(wait));
        }
    }
}

pub fn spawn(cfg: Config, shared: Arc<Mutex<Board>>, notify: Box<dyn Fn() + Send>) {
    let client = match Client::new() {
        Ok(c) => c,
        Err(e) => {
            if let Ok(mut b) = shared.lock() {
                b.link = Link::Down;
                b.note = format!("TLS: {e}");
            }
            notify();
            return;
        }
    };
    let (rows, rows_gen) = shared.lock().map(|b| (b.rows.clone(), b.rows_gen)).unwrap_or_else(|_| (cfg.rows.clone(), 0));
    let feed = Feed {
        cfg,
        rows,
        rows_gen,
        shared,
        notify,
        client,
        quotes: HashMap::new(),
        backfill: Vec::new(),
        backfill_gen: 0,
        bars_next: Instant::now(),
        fails: 0,
    };
    thread::Builder::new().name("feed".into()).spawn(move || feed.run()).expect("spawn feed thread");
}

/// `MH_Ticker --probe`: one pass over the quotes and the hour of bars, printed, then exit.
pub fn probe(cfg: &Config) {
    let client = match Client::new() {
        Ok(c) => c,
        Err(e) => return println!("TLS: {e}"),
    };
    let now = now_s();
    match client.quotes(&cfg.rows) {
        Ok(qs) => {
            let map: HashMap<_, _> = qs.into_iter().map(|q| (q.symbol.clone(), q)).collect();
            for c in resolve(&cfg.rows, &map, now, cfg.stale_after_s, cfg.delayed_grace_s) {
                match &c.quote {
                    Some(q) => println!(
                        "{:<5} {:>12} {:>8}  {}{}{} age={}",
                        c.label,
                        crate::model::fmt_price(q.price, c.digits),
                        q.change_pct.map(crate::model::fmt_change).unwrap_or_default(),
                        q.name,
                        if q.realtime { "" } else { " DELAYED" },
                        if c.stale { " STALE" } else { "" },
                        q.ts.map(|t| format!("{:.0}s", now - t)).unwrap_or_else(|| "?".into()),
                    ),
                    None => println!("{:<5} {:>12}  no quote", c.label, "-"),
                }
            }
        }
        Err(e) => println!("quotes: {e}"),
    }
    for r in &cfg.rows {
        match client.chart(&r.symbol) {
            Ok(p) if !p.is_empty() => println!(
                "bars {:<5} {} points, {:.0} min back to {:.0} s ago",
                r.label,
                p.len(),
                (now - p[0].0) / 60.0,
                (now - p[p.len() - 1].0).max(0.0)
            ),
            Ok(_) => println!("bars {:<5} none in the last hour", r.label),
            Err(e) => println!("bars {:<5} {e}", r.label),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cnbc_times_to_utc() {
        // 2026-10-15T08:29:41.313Z
        let utc = parse_time("2026-10-15T04:29:41.313-0400").unwrap();
        assert!((utc - 1_792_052_981.313).abs() < 1e-3, "{utc}");
        assert_eq!(parse_time("2026-10-15T08:29:41.313+0000"), Some(utc));
        assert_eq!(parse_time("2026-10-14"), None);
    }

    #[test]
    fn parses_formatted_quotes() {
        let v: Value = serde_json::from_str(
            r#"{"FormattedQuoteResult":{"FormattedQuote":[
              {"symbol":"XAU=","name":"Gold / US Dollar Spot","last":"4,155.24","change_pct":"+1.22%",
               "previous_day_closing":"4,105.122","last_time":"2026-10-15T04:29:41.313-0400","realTime":"true"},
              {"symbol":"@DJ.1","name":"Dow Jones Fut (Dec'26)","last":"52,078.00","change_pct":"-0.42%",
               "last_time":"2026-10-15T04:19:40.000-0400","realTime":"false"},
              {"symbol":"BAD","last":"UNCH"}]}}"#,
        )
        .unwrap();
        let qs = parse_quotes(&v);
        assert_eq!(qs.len(), 2);
        assert_eq!((qs[0].price, qs[0].change_pct, qs[0].prev_close, qs[0].realtime), (4155.24, Some(1.22), Some(4105.122), true));
        assert_eq!((qs[1].price, qs[1].change_pct, qs[1].realtime), (52078.0, Some(-0.42), false));
    }

    #[test]
    fn chart_keeps_the_last_hour_stamped_at_bar_end() {
        let v: Value = serde_json::from_str(
            r#"{"barData":{"priceBars":[
              {"close":"1.0","tradeTimeinMills":1000000},
              {"close":"2.0","tradeTimeinMills":10000000},
              {"close":"3.0","tradeTimeinMills":10060000}]}}"#,
        )
        .unwrap();
        assert_eq!(parse_chart(&v, 10_100.0), vec![(10_060.0, 2.0), (10_100.0, 3.0)]);
    }

    #[test]
    fn encodes_cnbc_symbols() {
        assert_eq!(encode("XAU=|@DJ.1"), "XAU%3D%7C%40DJ.1");
    }
}
