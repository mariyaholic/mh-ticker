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
use crate::model::{resolve, Bar, Board, Link, Quote};
use crate::ranges::{Done, Series, Slots};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const QUOTE_URL: &str = "https://quote.cnbc.com/quote-html-webservice/restQuote/symbolType/symbol";
const CHART_URL: &str = "https://ts-api.cnbc.com/harmony/app/charts/1D.json";
/// The folder the other range files (`5D.json`, `1M.json`, `1Y.json`, `5Y.json`) live in.
const CHARTS_DIR: &str = "https://ts-api.cnbc.com/harmony/app/charts/";
/// Bars kept from one range file: the newest. 5D is ~2,000 one-minute bars; the daily and weekly
/// files are a few hundred.
const SERIES_BARS: usize = 4_000;
const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
const BARS_REFRESH: Duration = Duration::from_secs(600);
const BARS_RETRY: Duration = Duration::from_secs(60);
/// A day of 1-minute bars is ~220 KB: more than the quote feed's timeout was set for.
const BARS_TIMEOUT: Duration = Duration::from_secs(45);
/// While waiting for the next poll, how often to look for a Symbols-menu change.
const ROWS_CHECK: Duration = Duration::from_millis(250);
const HOUR_S: f64 = 3600.0;
/// Bars kept per row. A day of 1-minute bars is ~1,400; the cap only guards against a payload that
/// is not what this endpoint is documented to be.
const MAX_BARS: usize = 1_600;

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

/// A number that only counts when it is above zero. CNBC sends `0.00` for a value a symbol does
/// not have (`high`/`low` on spot silver and platinum, `yrloprice` on six of the catalogue), and a
/// zero drawn on the card would read as a real price.
fn positive(v: Option<&Value>) -> Option<f64> {
    v.and_then(number).filter(|n| *n > 0.0)
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
                    source: "",
                    received_at: None,
                    name: r.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
                    price,
                    change_pct: r.get("change_pct").and_then(number),
                    prev_close: r.get("previous_day_closing").and_then(number),
                    ts: r.get("last_time").and_then(Value::as_str).and_then(parse_time),
                    realtime: flag(r.get("realTime")),
                    open: positive(r.get("open")),
                    high: positive(r.get("high")),
                    low: positive(r.get("low")),
                    volume: positive(r.get("volume")),
                    yr_hi: positive(r.get("yrhiprice")),
                    yr_lo: positive(r.get("yrloprice")),
                    yr_hi_date: r.get("yrhidate").and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string),
                    currency: r.get("currencyCode").and_then(Value::as_str).unwrap_or("").to_string(),
                })
            })
            .collect()
    })
    .unwrap_or_default()
}

fn parse_alternate(symbol: &str, value: &Value) -> Option<Quote> {
    let (source, price) = match symbol {
        "XAU=" | "XAG=" => {
            let want = if symbol == "XAU=" { "XAU" } else { "XAG" };
            if value.get("symbol")?.as_str()? != want || value.get("currency")?.as_str()? != "USD" { return None; }
            let updated = value.get("updatedAt")?.as_str()?;
            parse_time(&updated.replace('Z', "+0000"))?;
            ("Gold API", number(value.get("price")?)?)
        }
        "BTC.CB=" => {
            if !value.get("error")?.as_array()?.is_empty() { return None; }
            ("Kraken", number(value.pointer("/result/XXBTZUSD/c/0")?)?)
        }
        _ => return None,
    };
    if !price.is_finite() || price <= 0.0 { return None; }
    Some(Quote { symbol: symbol.into(), source, price, currency: "USD".into(), ..Quote::default() })
}

/// 1-day chart body -> (t0, bars): the whole day of 1-minute OHLC bars, each stamped at its end
/// (open + 60 s) and capped at `now` for the bar still forming. The sparkline takes the last hour of
/// this same series, so a line and a candle can never disagree about what the price did.
pub fn parse_chart(v: &Value, now: f64) -> (f64, Vec<Bar>) {
    let mut pts: Vec<(f64, Bar)> = Vec::new();
    if let Some(bars) = v.pointer("/barData/priceBars").and_then(Value::as_array) {
        for b in bars {
            let Some(t) = b.get("tradeTimeinMills").and_then(number).map(|ms| ms / 1000.0 + 60.0) else { continue };
            // A bar still forming is clamped to now, as the close series always was.
            let t = t.min(now);
            let Some(c) = b.get("close").and_then(number) else { continue };
            // A bar without o/h/l is not a candle: fall back to the close so it draws as a doji
            // rather than a spike to zero.
            let o = b.get("open").and_then(number).unwrap_or(c);
            let h = b.get("high").and_then(number).unwrap_or(c);
            let l = b.get("low").and_then(number).unwrap_or(c);
            let v = b.get("volume").and_then(number).filter(|v| v.is_finite() && *v > 0.0).unwrap_or(0.0);
            pts.push((t, Bar { dt: 0.0, o: o as f32, h: h as f32, l: l as f32, c: c as f32, v: v as f32 }));
        }
    }
    pts.sort_by(|a, b| a.0.total_cmp(&b.0));
    // A day of 1-minute bars is ~1,400; anything longer is a different endpoint, not a chart.
    let keep = pts.len().saturating_sub(MAX_BARS);
    pts.drain(..keep);
    let t0 = pts.first().map(|p| p.0).unwrap_or(now);
    let bars = pts.into_iter().map(|(t, mut b)| { b.dt = (t - t0) as f32; b }).collect();
    (t0, bars)
}

/// A range file (`5D.json` .. `5Y.json`) -> a [`Series`] fetched at `now`.
///
/// Assumes as little as it can, because it has only been measured on one symbol on one day:
/// * the bars are `barData.priceBars`; anything else (junk, an empty array, no such path) is an EMPTY
///   series (`t0 == now`), not an error: the pane says "no data for this range";
/// * a bar's time is `tradeTimeinMills` or `tradeTimeInMills` (either spelling, a number or a numeric
///   string), in milliseconds, and is the START of the bar: it is NOT shifted by a minute the way
///   `parse_chart` stamps its bars at their end. A bar with neither field is skipped;
/// * `close` is required (a bar without one is skipped); `open`, `high` and `low` fall back to the
///   close; `volume` is optional and counts only when finite and above zero (else 0);
/// * the bars are sorted by time (stably; equal times keep the later one), and only the newest
///   [`SERIES_BARS`] are kept;
/// * `t0` is the first kept bar's time and each `Bar::dt` is its offset from it; `Series::new` infers
///   `bar_s`.
pub fn parse_series(v: &Value, now: f64) -> Series {
    let mut pts: Vec<(f64, Bar)> = Vec::new();
    if let Some(bars) = v.pointer("/barData/priceBars").and_then(Value::as_array) {
        for b in bars {
            let ms = ["tradeTimeinMills", "tradeTimeInMills"].iter().find_map(|k| b.get(k).and_then(number));
            let Some(t) = ms.map(|ms| ms / 1000.0).filter(|t| t.is_finite()) else { continue };
            let Some(c) = b.get("close").and_then(number).filter(|c| c.is_finite()) else { continue };
            let o = b.get("open").and_then(number).filter(|x| x.is_finite()).unwrap_or(c);
            let h = b.get("high").and_then(number).filter(|x| x.is_finite()).unwrap_or(c);
            let l = b.get("low").and_then(number).filter(|x| x.is_finite()).unwrap_or(c);
            let v = b.get("volume").and_then(number).filter(|v| v.is_finite() && *v > 0.0).unwrap_or(0.0);
            pts.push((t, Bar { dt: 0.0, o: o as f32, h: h as f32, l: l as f32, c: c as f32, v: v as f32 }));
        }
    }
    // Stable sort, then one bar per instant: of equal times the later one in the file is kept.
    pts.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut kept: Vec<(f64, Bar)> = Vec::with_capacity(pts.len());
    for p in pts {
        match kept.last_mut() {
            Some(last) if last.0 == p.0 => *last = p,
            _ => kept.push(p),
        }
    }
    let drop = kept.len().saturating_sub(SERIES_BARS);
    kept.drain(..drop);
    let Some(t0) = kept.first().map(|p| p.0) else { return Series::new(now, Vec::new(), now) };
    let bars = kept.into_iter().map(|(t, mut b)| { b.dt = (t - t0) as f32; b }).collect();
    Series::new(t0, bars, now)
}

/// The long-lived `ranges` worker: polls the shared request slot every [`ROWS_CHECK`], fetches one
/// range file at a time, hands the result back through the slot and tells the window with `notify`
/// (which posts `WM_RANGE`). No thread per click.
///
/// * A request replaced in the slot before this thread takes it is never fetched (`Slots::submit`).
/// * A request being fetched is not interrupted: it finishes, is delivered, and the UI's `Book`
///   caches it if its generation is still the one in flight for that key and drops it otherwise.
/// * The client is built on the first request, and again on the next one if TLS setup failed, so a
///   failure is reported as a failed fetch (the pane shows "could not load, retrying") rather than as
///   a request that never answers.
/// * If the thread cannot be started nothing ever answers; the book presumes each request lost after
///   two minutes and the pane keeps saying "loading". It is not worth a message box.
pub fn spawn_ranges(slots: Arc<Mutex<Slots>>, notify: Box<dyn Fn() + Send + Sync>) {
    let _ = thread::Builder::new().name("ranges".into()).spawn(move || {
        let mut client: Option<Client> = None;
        loop {
            let req = slots.lock().ok().and_then(|mut s| s.take_request());
            let Some(req) = req else {
                thread::sleep(ROWS_CHECK);
                continue;
            };
            if client.is_none() {
                client = Client::new().ok();
            }
            let result = match &client {
                Some(c) => c.chart_file(&req.key.symbol, req.key.file),
                None => Err("TLS".to_string()),
            };
            if let Ok(mut s) = slots.lock() {
                s.deliver(Done { generation: req.generation, key: req.key, result });
            }
            notify();
        }
    });
}

/// One outbound POST, for alerts. Deliberately a free function: alert traffic is rare, so it gets
/// its own short-lived agent instead of sharing the quote feed's connection pool.
///
/// Errors are reduced to "HTTP <code>" or "network" on purpose: `ureq`'s own message can echo the
/// URL, and for a webhook the URL *is* the credential.
pub fn post(url: &str, body: &str, content_type: &str, timeout_s: u64) -> Result<(), String> {
    let tls = native_tls::TlsConnector::new().map_err(|_| "tls".to_string())?;
    let agent = ureq::AgentBuilder::new()
        .tls_connector(Arc::new(tls))
        .user_agent(UA)
        .timeout(Duration::from_secs(timeout_s))
        .build();
    match agent.post(url).set("Content-Type", content_type).send_string(body) {
        Ok(_) => Ok(()),
        Err(ureq::Error::Status(code, _)) => Err(format!("HTTP {code}")),
        Err(_) => Err("network".into()),
    }
}

/// The update check's one request: GET `url` with a dedicated short-lived agent (native-tls, the
/// same UA, 10 s timeout, no redirects) and `Accept: application/vnd.github+json`, reading at most
/// `update::MAX_BODY` bytes. Returns the body text, or a fixed `update::Outcome::Failed` text
/// (`"network"`, `"HTTP <code>"`, `"too large"`, `"unreadable answer"`): `ureq`'s own messages
/// are not passed on. Blocking; call it from a worker thread only.
pub fn get_release(url: &str) -> Result<String, String> {
    use std::io::Read;
    let tls = native_tls::TlsConnector::new().map_err(|_| "network".to_string())?;
    let agent = ureq::AgentBuilder::new()
        .tls_connector(Arc::new(tls))
        .user_agent(UA)
        .timeout(Duration::from_secs(10))
        .redirects(0)
        .build();
    let resp = match agent.get(url).set("Accept", "application/vnd.github+json").call() {
        Ok(r) => r,
        Err(ureq::Error::Status(code, _)) => return Err(format!("HTTP {code}")),
        Err(_) => return Err("network".into()),
    };
    let limit = crate::update::MAX_BODY;
    let mut body = Vec::new();
    resp.into_reader().take((limit + 1) as u64).read_to_end(&mut body).map_err(|_| "network".to_string())?;
    if body.len() > limit {
        return Err("too large".into());
    }
    String::from_utf8(body).map_err(|_| "unreadable answer".to_string())
}

/// What one quote request said about a typed symbol (*Add symbol...*).
#[derive(Clone, Debug, PartialEq)]
pub enum Lookup {
    /// CNBC quotes it: its name (as sent, maybe empty) and a positive, finite last price.
    Found { name: String, price: f64 },
    /// CNBC answered, but not with a price for this symbol.
    NotFound,
    /// No answer: `"network"`, `"HTTP <code>"`, `"unreadable answer"` or `"could not start"`. Never
    /// `ureq`'s own message.
    Failed(String),
}

/// The longest quote answer `lookup` reads. One symbol's quote is about two kilobytes.
const LOOKUP_MAX_BODY: usize = 256 * 1024;

/// One quote request for `symbol` (already cleaned), with a short-lived agent (native-tls, the feed's
/// UA, 10 s timeout, no redirects), reading at most `LOOKUP_MAX_BODY` bytes. Blocking: call it from a
/// worker thread only.
pub fn lookup(symbol: &str) -> Lookup {
    use std::io::Read;
    let Ok(tls) = native_tls::TlsConnector::new() else {
        return Lookup::Failed("network".into());
    };
    let agent = ureq::AgentBuilder::new()
        .tls_connector(Arc::new(tls))
        .user_agent(UA)
        .timeout(Duration::from_secs(10))
        .redirects(0)
        .build();
    let url = format!("{QUOTE_URL}?symbols={}&requestMethod=itv&noform=1&partnerId=2&fund=1&exthrs=1&output=json", encode(symbol));
    let resp = match agent.get(&url).call() {
        Ok(r) => r,
        Err(ureq::Error::Status(code, _)) => return Lookup::Failed(format!("HTTP {code}")),
        Err(_) => return Lookup::Failed("network".into()),
    };
    let mut body = Vec::new();
    if resp.into_reader().take((LOOKUP_MAX_BODY + 1) as u64).read_to_end(&mut body).is_err() {
        return Lookup::Failed("network".into());
    }
    if body.len() > LOOKUP_MAX_BODY {
        return Lookup::Failed("unreadable answer".into());
    }
    match serde_json::from_slice::<Value>(&body) {
        Ok(v) => parse_lookup(&v, symbol),
        Err(_) => Lookup::Failed("unreadable answer".into()),
    }
}

/// The quote answer for `symbol`: `Found` when `parse_quotes` gives a quote whose symbol equals
/// `symbol` ignoring ASCII case (CNBC echoes it), with a positive finite price; `NotFound` for any
/// other answer, including an empty list, another symbol's quote, and a body of another shape.
pub fn parse_lookup(v: &Value, symbol: &str) -> Lookup {
    parse_quotes(v)
        .into_iter()
        .find(|q| q.symbol.eq_ignore_ascii_case(symbol) && q.price.is_finite() && q.price > 0.0)
        .map_or(Lookup::NotFound, |q| Lookup::Found { name: q.name, price: q.price })
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

    fn new_fallback() -> Result<Self, String> {
        let tls = native_tls::TlsConnector::new().map_err(|e| e.to_string())?;
        let agent = ureq::AgentBuilder::new().tls_connector(Arc::new(tls)).user_agent(UA)
            .timeout(Duration::from_secs(10)).redirects(0).build();
        Ok(Self { agent })
    }

    fn get_fallback(&self, url: &str) -> Result<Value, String> {
        use std::io::Read;
        const LIMIT: usize = 32 * 1024;
        let resp = self.agent.get(url).call().map_err(|_| "fallback request failed".to_string())?;
        if !(200..300).contains(&resp.status()) { return Err("fallback HTTP status".into()); }
        let mut body = Vec::new();
        resp.into_reader().take((LIMIT + 1) as u64).read_to_end(&mut body).map_err(|_| "fallback read failed".to_string())?;
        if body.len() > LIMIT { return Err("fallback response too large".into()); }
        serde_json::from_slice(&body).map_err(|_| "invalid fallback JSON".into())
    }

    fn get(&self, url: &str) -> Result<Value, String> {
        self.get_within(url, None)
    }

    /// `timeout` overrides the agent's for one request: a quote is a few hundred bytes, a day of
    /// bars is a quarter of a megabyte, and the ten seconds that serve the first are not enough
    /// for the second on a slow link. A truncated body is worse than a late one.
    fn get_within(&self, url: &str, timeout: Option<Duration>) -> Result<Value, String> {
        let req = self.agent.get(url);
        let req = match timeout {
            Some(t) => req.timeout(t),
            None => req,
        };
        match req.call() {
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

    pub fn chart(&self, symbol: &str) -> Result<(f64, Vec<Bar>), String> {
        let url = format!("{CHART_URL}?symbol={}", encode(symbol));
        Ok(parse_chart(&self.get_within(&url, Some(BARS_TIMEOUT))?, now_s()))
    }

    /// One range file of one symbol, parsed (see [`parse_series`]). `file` is one of the constants
    /// from `ranges::Range::file`, never text from a response.
    pub fn chart_file(&self, symbol: &str, file: &str) -> Result<Series, String> {
        let url = format!("{CHARTS_DIR}{file}?symbol={}", encode(symbol));
        Ok(parse_series(&self.get_within(&url, Some(BARS_TIMEOUT))?, now_s()))
    }
}

/// One alternate request, with the policy and row generation checked before and after I/O.
fn fallback_attempt(
    shared: &Arc<Mutex<Board>>, row: &Row,
    fetch: impl FnOnce(&str) -> Option<Quote>,
) -> bool {
    let (policy, rows_gen) = match shared.lock() {
        Ok(b) if b.fallback_enabled && b.note != "starting" => {
            let Some(i) = b.rows.iter().position(|r| r.label == row.label && r.symbol == row.symbol) else { return false; };
            if b.link != Link::Down && !b.cells[i].stale { return false; }
            (b.fallback_gen, b.rows_gen)
        }
        _ => return false,
    };
    let Some(mut quote) = fetch(&row.symbol) else { return false; };
    quote.received_at = Some(now_s());
    shared.lock().map(|mut b| b.publish_fallback(policy, rows_gen, row, quote)).unwrap_or(false)
}

fn alternate_url(symbol: &str) -> Option<&'static str> {
    match symbol {
        "XAU=" => Some("https://api.gold-api.com/price/XAU"),
        "XAG=" => Some("https://api.gold-api.com/price/XAG"),
        "BTC.CB=" => Some("https://api.kraken.com/0/public/Ticker?pair=XBTUSD"),
        _ => None,
    }
}

fn fetch_alternate(client: &Client, symbol: &str) -> Option<Quote> {
    let payload = client.get_fallback(alternate_url(symbol)?).ok()?;
    if symbol == "XAU=" || symbol == "XAG=" {
        // This is the provider's fetch time, not a verified trade timestamp.
        let updated = payload.get("updatedAt")?.as_str()?.replace('Z', "+0000");
        let age = now_s() - parse_time(&updated)?;
        if !(-5.0..=90.0).contains(&age) { return None; }
    }
    parse_alternate(symbol, &payload)
}

fn fallback_worker(client: Client, shared: Arc<Mutex<Board>>, notify: Arc<dyn Fn() + Send + Sync>, symbol: &'static str) {
    // Deadlines survive toggling so rapid opt-in cycles do not evade provider limits.
    let mut due: HashMap<String, Instant> = HashMap::new();
    loop {
        let rows = shared.lock().map(|b| if b.fallback_enabled { b.rows.clone() } else { Vec::new() }).unwrap_or_default();
        for row in rows {
            if row.symbol != symbol || due.get(&row.symbol).is_some_and(|t| *t > Instant::now()) { continue; }
            let attempted = std::cell::Cell::new(false);
            let ok = fallback_attempt(&shared, &row, |symbol| {
                attempted.set(true);
                fetch_alternate(&client, symbol)
            });
            if attempted.get() { due.insert(row.symbol.clone(), Instant::now() + Duration::from_secs(30)); }
            if ok { notify(); }
        }
        thread::sleep(ROWS_CHECK);
    }
}

struct Feed {
    cfg: Config,
    /// The rows this thread polls; adopted from `Board::rows` when `rows_gen` moves.
    rows: Vec<Row>,
    rows_gen: u64,
    shared: Arc<Mutex<Board>>,
    notify: Arc<dyn Fn() + Send + Sync>,
    client: Client,
    quotes: HashMap<String, Quote>,
    /// (t0, bars) per row: the candles, and the hour of closes the sparklines are seeded from.
    chart: Vec<(f64, Vec<Bar>)>,
    backfill: Vec<Vec<(f64, f64)>>,
    backfill_gen: u64,
    bars_next: Instant,
    bars_due: Vec<Instant>,
    fails: u32,
}

/// A symbol that timed out this pass keeps the bars it got last pass.
///
/// A day of bars is a quarter of a megabyte over SChannel: on a slow link a pass is a few wins and
/// a few losses, and replacing the set wholesale would make the chart flap between drawn and empty.
/// Rows are stable between passes; `sync_rows` remaps the cache by identity when they change.
/// An empty result for row `i` is treated as a failed fetch, retaining its prior bars if present.
fn merge_bars(prev: &[(f64, Vec<Bar>)], got: Vec<(f64, Vec<Bar>)>) -> Vec<(f64, Vec<Bar>)> {
    let mut got = got;
    if got.len() == prev.len() {
        for (i, want) in got.iter_mut().enumerate() {
            if want.1.is_empty() {
                if let Some((t0, kept)) = prev.get(i) {
                    if !kept.is_empty() {
                        *want = (*t0, kept.clone());
                    }
                }
            }
        }
    }
    got
}

impl Feed {
    /// Adopt the UI's row list if the Symbols menu changed it; the hour of bars is refetched.
    fn sync_rows(&mut self) {
        if let Ok(b) = self.shared.lock() {
            if b.rows_gen != self.rows_gen {
                let now = Instant::now();
                let due: Vec<Instant> = b.rows.iter().enumerate().map(|(new_i, r)| {
                    let Some(old_i) = self.rows.iter().position(|old| old.label == r.label && old.symbol == r.symbol) else { return now; };
                    // A row can be removed and re-added between syncs, or a successful fetch can
                    // be rejected between its generation check and publish. In either case the
                    // private cache differs from the board, so the success deadline is not valid.
                    let private = self.chart.get(old_i);
                    let published = b.bars.get(new_i).map(|v| v.as_ref()).unwrap_or(&[]);
                    if let Some((t0, bars)) = private {
                        if !bars.is_empty() && (bars.as_slice() != published || b.bars_t0.get(new_i).copied() != Some(*t0)) {
                            return now;
                        }
                    }
                    self.bars_due.get(old_i).copied().unwrap_or(now)
                }).collect();
                self.rows = b.rows.clone();
                self.rows_gen = b.rows_gen;
                // Board::set_rows has already remapped these vectors by identity. Reuse that
                // cache so a failed fetch after the change cannot wipe a surviving symbol's bars.
                self.chart = self.rows.iter().enumerate().map(|(i, _)| {
                    (b.bars_t0.get(i).copied().unwrap_or_default(),
                     b.bars.get(i).map(|v| v.to_vec()).unwrap_or_default())
                }).collect();
                self.backfill = b.backfill.clone();
                // A rejected in-flight fetch may have advanced our private counter. Adopt the
                // published generation before sending quotes, or stale cache state wipes retained rows.
                self.backfill_gen = b.backfill_gen;
                self.bars_next = due.iter().copied().min().unwrap_or(now + BARS_REFRESH);
                self.bars_due = due;
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
            let now = now_s();
            let mut cells = resolve(&self.rows, &self.quotes, now, self.cfg.stale_after_s, self.cfg.delayed_grace_s);
            if link == Link::Down { for cell in &mut cells { cell.stale = true; } }
            // Keep a recent fallback until CNBC supplies a fresh response; never turn receipt
            // time into market freshness, and expire fallback if its refresh stops succeeding.
            if b.fallback_enabled {
                for (next, old) in cells.iter_mut().zip(&b.cells) {
                    if (link == Link::Down || next.stale) && old.quote.as_ref().is_some_and(|q| {
                        !q.source.is_empty() && q.received_at.is_some_and(|t| (0.0..=90.0).contains(&(now - t)))
                    }) {
                        *next = old.clone();
                    }
                }
            }
            b.cells = cells;
            b.link = link;
            b.note = note;
        }
        (self.notify)();
    }

    fn publish_bars(&self) {
        if let Ok(mut b) = self.shared.lock() {
            if b.rows_gen != self.rows_gen { return; }
            if b.backfill_gen != self.backfill_gen {
                b.backfill = self.backfill.clone();
                b.backfill_gen = self.backfill_gen;
                b.bars = self.chart.iter().map(|(_, bars)| bars.clone().into()).collect();
                b.bars_t0 = self.chart.iter().map(|(t0, _)| *t0).collect();
                b.bars_gen = self.backfill_gen;
            }
        }
        (self.notify)();
    }

    /// The cadence in force. Read from the board rather than `cfg`, so the Poll-interval menu item
    /// applies on the next tick instead of needing a restart.
    fn cadence(&self) -> f64 {
        self.shared.lock().map(|b| b.poll_s).unwrap_or(self.cfg.poll_s).clamp(1.0, 60.0)
    }

    /// Sleep until the next poll, waking early when the Symbols menu changes the rows so a new
    /// symbol shows up within a fraction of a second, or when *Refresh now* bumps `Board::kick`.
    /// `acted` is the `kick` value the caller last acted on, read BEFORE it started the work that
    /// precedes this wait (a quote request can take 10 s, a chart fetch 45 s). A kick that lands
    /// during that work therefore differs from `acted` and ends the wait at once, instead of being
    /// taken as the baseline and lost. Polled every `ROWS_CHECK`.
    fn wait(&self, d: Duration, acted: u64) {
        let until = Instant::now() + d;
        while Instant::now() < until {
            thread::sleep(ROWS_CHECK.min(until.saturating_duration_since(Instant::now())));
            if self.shared.lock().map(|b| b.rows_gen != self.rows_gen || b.kick != acted).unwrap_or(false) {
                return;
            }
        }
    }

    /// *Refresh now* for the chart thread: every row whose bars are empty (never fetched, or the
    /// last fetch failed and is waiting out `BARS_RETRY`) becomes due at once. Rows that have bars
    /// keep their schedule.
    fn kick_bars(&mut self) {
        let now = Instant::now();
        if self.bars_due.len() != self.rows.len() {
            self.bars_due = vec![now; self.rows.len()];
        }
        for (i, due) in self.bars_due.iter_mut().enumerate() {
            if self.chart.get(i).map(|(_, bars)| bars.is_empty()).unwrap_or(true) {
                *due = now;
            }
        }
        self.bars_next = self.bars_due.iter().copied().min().unwrap_or(now).min(self.bars_next);
    }

    /// Fetch the hour of 1-minute bars when due; true when a fresh set arrived.
    fn maybe_bars(&mut self) -> bool {
        let client = Client { agent: self.client.agent.clone() };
        self.maybe_bars_with(|symbol| client.chart(symbol).unwrap_or_default())
    }

    fn maybe_bars_with(&mut self, mut fetch: impl FnMut(&str) -> (f64, Vec<Bar>)) -> bool {
        if Instant::now() < self.bars_next {
            return false;
        }
        if self.bars_due.len() != self.rows.len() {
            self.bars_due = vec![Instant::now(); self.rows.len()];
        }
        let Some((i, _)) = self.bars_due.iter().enumerate().min_by_key(|(_, due)| *due) else {
            self.bars_next = Instant::now() + BARS_REFRESH;
            return false;
        };
        let result = fetch(&self.rows[i].symbol);
        // A row change invalidates even a successful response from the previous generation.
        if self.shared.lock().map(|b| b.rows_gen != self.rows_gen).unwrap_or(true) {
            return false;
        }
        let fresh = !result.1.is_empty();
        self.bars_due[i] = Instant::now() + if fresh { BARS_REFRESH } else { BARS_RETRY };
        let changed = if fresh {
            let mut got = self.chart.clone();
            got.resize_with(self.rows.len(), Default::default);
            got[i] = result;
            self.accept_bars(got)
        } else { false };
        self.bars_next = self.bars_due.iter().copied().min().unwrap_or_else(|| Instant::now() + BARS_REFRESH);
        changed
    }

    fn accept_bars(&mut self, got: Vec<(f64, Vec<Bar>)>) -> bool {
        let retry = got.iter().any(|(_, bars)| bars.is_empty());
        let got = merge_bars(&self.chart, got);
        if got.iter().any(|(_, bars)| !bars.is_empty()) {
            // The sparkline is the last hour of the same bars, so the two never disagree.
            let now = now_s();
            self.backfill = got
                .iter()
                .map(|(t0, bars)| {
                    bars.iter()
                        .filter(|b| b.t(*t0) >= now - HOUR_S - 60.0)
                        .map(|b| (b.t(*t0), b.c as f64))
                        .collect()
                })
                .collect();
            self.chart = got;
            self.backfill_gen += 1;
            self.bars_next = Instant::now() + if retry { BARS_RETRY } else { BARS_REFRESH };
            true
        } else {
            self.bars_next = Instant::now() + BARS_RETRY;
            false
        }
    }

    fn run(mut self) {
        loop {
            self.sync_rows();
            // Read before the request: a kick that lands while it is in flight ends the wait below.
            let acted = self.shared.lock().map(|b| b.kick).unwrap_or(0);
            let (link, note) = match self.client.quotes(&self.rows) {
                Ok(qs) => {
                    self.fails = 0;
                    // A partial batch is a per-row failure, not confirmation of a cached quote.
                    let returned: std::collections::HashSet<_> = qs.iter().map(|q| q.symbol.as_str()).collect();
                    for row in &self.rows {
                        if !returned.contains(row.symbol.as_str()) { self.quotes.remove(&row.symbol); }
                    }
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
            let cadence = self.cadence();
            let wait = if self.fails == 0 {
                cadence
            } else {
                (cadence * 2f64.powi(self.fails.min(6) as i32)).min(60.0)
            };
            self.wait(Duration::from_secs_f64(wait), acted);
        }
    }
}

pub fn spawn(cfg: Config, shared: Arc<Mutex<Board>>, notify: Box<dyn Fn() + Send + Sync>) {
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
    let notify: Arc<dyn Fn() + Send + Sync> = Arc::from(notify);
    // Expiry must progress even when every HTTP worker is blocked.
    {
        let board = shared.clone();
        let notify_expiry = notify.clone();
        thread::Builder::new().name("fallback-expiry".into()).spawn(move || loop {
            let changed = board.lock().map(|mut b| b.expire_fallback(now_s())).unwrap_or(false);
            if changed { notify_expiry(); }
            thread::sleep(ROWS_CHECK);
        }).expect("spawn fallback expiry thread");
    }
    // A slow provider or symbol cannot stall another fallback or the CNBC workers.
    // These three long-lived workers are bounded; toggling never creates new threads.
    for symbol in ["XAU=", "XAG=", "BTC.CB="] {
        let fallback_client = match Client::new_fallback() {
            Ok(client) => client,
            Err(_) => continue, // CNBC still works if an alternate client cannot initialize.
        };
        let fallback_board = shared.clone();
        let fallback_notify = notify.clone();
        thread::Builder::new().name(format!("fallback-{symbol}")).spawn(move || {
            fallback_worker(fallback_client, fallback_board, fallback_notify, symbol)
        }).expect("spawn fallback thread");
    }
    let chart_client = Client { agent: client.agent.clone() };
    let mut chart_feed = Feed {
        cfg: cfg.clone(), rows: rows.clone(), rows_gen, shared: shared.clone(),
        notify: notify.clone(), client: chart_client, quotes: HashMap::new(), chart: Vec::new(),
        backfill: Vec::new(), backfill_gen: 0, bars_next: Instant::now(), bars_due: Vec::new(), fails: 0,
    };
    thread::Builder::new().name("chart".into()).spawn(move || {
        let mut seen_kick = chart_feed.shared.lock().map(|b| b.kick).unwrap_or(0);
        loop {
        chart_feed.sync_rows();
        let kick = chart_feed.shared.lock().map(|b| b.kick).unwrap_or(seen_kick);
        if kick != seen_kick {
            seen_kick = kick;
            chart_feed.kick_bars();
        }
        if chart_feed.maybe_bars() { chart_feed.publish_bars(); }
        let wait = chart_feed.bars_next.saturating_duration_since(Instant::now());
        chart_feed.wait(wait, seen_kick);
        }
    }).expect("spawn chart thread");
    let feed = Feed {
        cfg,
        rows,
        rows_gen,
        shared,
        notify,
        client,
        quotes: HashMap::new(),
        chart: Vec::new(),
        backfill: Vec::new(),
        backfill_gen: 0,
        bars_next: Instant::now(),
        bars_due: Vec::new(),
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
            Ok((t0, bars)) if !bars.is_empty() => println!(
                "bars {:<5} {} bars, {:.0} min long, last {:.0} s ago",
                r.label,
                bars.len(),
                (bars[bars.len() - 1].t(t0) - t0) / 60.0,
                (now - bars[bars.len() - 1].t(t0)).max(0.0)
            ),
            Ok(_) => println!("bars {:<5} none", r.label),
            Err(e) => println!("bars {:<5} {e}", r.label),
        }
    }
    // The live check for the range files, on the first row: what each file really carries.
    if let Some(r) = cfg.rows.first() {
        for file in ["5D.json", "1M.json", "1Y.json", "5Y.json"] {
            match client.chart_file(&r.symbol, file) {
                Ok(sr) => {
                    let n = sr.bars.len();
                    let span_d = sr.bars.last().map_or(0.0, |b| b.dt as f64) / 86_400.0;
                    // The last bar's stamp, in UTC. Daily and weekly candles are dated on the
                    // assumption that this is midnight on the exchange's clock (`chartview::date_zone`):
                    // this line is where that can be seen to hold.
                    let last = sr.bars.last().map_or_else(String::new, |b| {
                        let t = b.t(sr.t0);
                        format!(
                            ", last bar {} {} UTC",
                            crate::chartview::date_label(t, 0, crate::chartview::DateStyle::Full),
                            crate::chartview::clock(t, 0)
                        )
                    });
                    println!(
                        "range {:<5} {file}: {n} bars, {:.0} s a bar, {:.1} days, volume {}{last}",
                        r.label,
                        sr.bar_s,
                        span_d,
                        if sr.bars.iter().any(|b| b.v > 0.0) { "yes" } else { "no" }
                    );
                }
                Err(e) => println!("range {:<5} {file}: {e}", r.label),
            }
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
    fn fallback_rejects_redirects_and_oversized_responses() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        let target = TcpListener::bind("127.0.0.1:0").unwrap();
        target.set_nonblocking(true).unwrap();
        let target_addr = target.local_addr().unwrap();
        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = server.local_addr().unwrap();
        let redirect = thread::spawn(move || {
            let (mut stream, _) = server.accept().unwrap();
            let mut request = [0; 2048]; let _ = stream.read(&mut request);
            write!(stream, "HTTP/1.1 302 Found\r\nLocation: http://{target_addr}/elsewhere\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
        });
        let client = Client::new_fallback().unwrap();
        assert!(client.get_fallback(&format!("http://{addr}/test")).is_err());
        redirect.join().unwrap();
        assert!(target.accept().is_err(), "redirect must not contact another host");
        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = server.local_addr().unwrap();
        let large = thread::spawn(move || {
            let (mut stream, _) = server.accept().unwrap();
            let mut request = [0; 2048]; let _ = stream.read(&mut request);
            let body = vec![b'x'; 40 * 1024];
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).unwrap();
            let _ = stream.write_all(&body);
        });
        assert_eq!(client.get_fallback(&format!("http://{addr}/test")).unwrap_err(), "fallback response too large");
        large.join().unwrap();
    }

    #[test]
    fn alternate_survives_primary_outage_and_yields_to_fresh_cnbc() {
        let cfg = Config::default();
        let row = cfg.rows[0].clone();
        let rows = vec![row.clone()];
        let shared = Arc::new(Mutex::new(Board::new(&rows)));
        let mut feed = Feed { cfg, rows, rows_gen: 0, shared: shared.clone(), notify: Arc::new(|| {}),
            client: Client::new().unwrap(), quotes: HashMap::new(), chart: Vec::new(),
            backfill: Vec::new(), backfill_gen: 0, bars_next: Instant::now(), bars_due: Vec::new(), fails: 0 };
        shared.lock().unwrap().set_fallback(true);
        feed.publish(Link::Down, "CNBC network error".into());
        assert!(fallback_attempt(&shared, &row, |symbol| Some(Quote {
            symbol: symbol.into(), source: "Gold API", price: 100.0, ..Quote::default()
        })));
        feed.publish(Link::Down, "still down".into());
        assert_eq!(shared.lock().unwrap().cells[0].quote.as_ref().unwrap().source, "Gold API");
        feed.quotes.insert(row.symbol.clone(), Quote { symbol: row.symbol.clone(), price: 101.0,
            ts: Some(now_s()), realtime: true, ..Quote::default() });
        feed.publish(Link::Live, String::new());
        assert_eq!(shared.lock().unwrap().cells[0].quote.as_ref().unwrap().source, "");
        let called = std::cell::Cell::new(false);
        assert!(!fallback_attempt(&shared, &row, |_| { called.set(true); None }));
        assert!(!called.get());
    }

    #[test]
    fn blocked_gold_does_not_block_other_provider_or_cnbc_recovery() {
        let gold = crate::config::default_rows()[0].clone();
        let btc = crate::config::default_rows()[2].clone();
        assert_eq!(btc.symbol, "BTC.CB=");
        let rows = vec![gold.clone(), btc.clone()];
        let shared = Arc::new(Mutex::new(Board::new(&rows)));
        {
            let mut b = shared.lock().unwrap();
            b.note = "CNBC network error".into();
            b.set_fallback(true);
        }
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let blocked_board = shared.clone();
        let blocked_gold = gold.clone();
        let worker = thread::spawn(move || fallback_attempt(&blocked_board, &blocked_gold, |symbol| {
            entered_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            Some(Quote { symbol: symbol.into(), source: "Gold API", price: 100.0, ..Quote::default() })
        }));
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(fallback_attempt(&shared, &btc, |symbol| Some(Quote {
            symbol: symbol.into(), source: "Kraken", price: 42.0, ..Quote::default()
        })));
        {
            let mut b = shared.lock().unwrap();
            b.link = Link::Live;
            b.cells[0].stale = false;
            b.cells[0].quote = Some(Quote { symbol: gold.symbol.clone(), price: 101.0,
                ts: Some(now_s()), realtime: true, ..Quote::default() });
        }
        release_tx.send(()).unwrap();
        assert!(!worker.join().unwrap());
        let b = shared.lock().unwrap();
        assert_eq!(b.cells[0].quote.as_ref().unwrap().source, "");
        assert_eq!(b.cells[1].quote.as_ref().unwrap().source, "Kraken");
    }

    #[test]
    fn authorized_request_may_finish_after_disable_but_cannot_publish() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let row = Config::default().rows[0].clone();
        let shared = Arc::new(Mutex::new(Board::new(std::slice::from_ref(&row))));
        { let mut b = shared.lock().unwrap(); b.note = "CNBC network error".into(); b.set_fallback(true); }
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let connected = Arc::new(AtomicBool::new(false));
        let worker_connected = connected.clone();
        let child_board = shared.clone();
        let worker = thread::spawn(move || fallback_attempt(&child_board, &row, |_| {
            entered_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            // This marks the point at which a provider connection would begin.
            worker_connected.store(true, Ordering::SeqCst);
            Some(Quote { symbol: row.symbol.clone(), source: "Gold API", price: 100.0, ..Quote::default() })
        }));
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        shared.lock().unwrap().set_fallback(false);
        release_tx.send(()).unwrap();
        assert!(!worker.join().unwrap());
        assert!(connected.load(Ordering::SeqCst), "already-authorized work may finish");
        assert!(shared.lock().unwrap().cells[0].quote.is_none());
    }

    #[test]
    fn blocked_alternate_result_is_rejected_after_rapid_toggle_stress() {
        let row = Config::default().rows[0].clone();
        let shared = Arc::new(Mutex::new(Board::new(std::slice::from_ref(&row))));
        {
            let mut b = shared.lock().unwrap();
            b.note = "CNBC network error".into();
            b.set_fallback(true);
        }
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let child_board = shared.clone();
        let child_row = row.clone();
        let worker = thread::spawn(move || fallback_attempt(&child_board, &child_row, |symbol| {
            entered_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            Some(Quote { symbol: symbol.into(), source: "Gold API", price: 1.0, ..Quote::default() })
        }));
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        for _ in 0..10_000 {
            let mut b = shared.lock().unwrap();
            b.set_fallback(false);
            b.set_fallback(true);
        }
        release_tx.send(()).unwrap();
        assert!(!worker.join().unwrap());
        assert!(shared.lock().unwrap().cells[0].quote.is_none());
        shared.lock().unwrap().set_fallback(false);
        let requested = std::cell::Cell::new(false);
        assert!(!fallback_attempt(&shared, &row, |_| { requested.set(true); None }));
        assert!(!requested.get());
    }

    #[test]
    fn alternate_quotes_check_identity_and_never_claim_market_freshness() {
        let gold = serde_json::json!({"symbol":"XAU", "currency":"USD", "price":1234.5, "updatedAt":"2026-11-03T21:17:57Z"});
        let q = parse_alternate("XAU=", &gold).unwrap();
        assert_eq!((q.symbol.as_str(), q.source, q.price, q.ts), ("XAU=", "Gold API", 1234.5, None));
        assert!(parse_alternate("XAG=", &gold).is_none());
        assert!(parse_alternate("XAU=", &serde_json::json!({"symbol":"XAU","currency":"USD","price":1234.5})).is_none());
        let btc = serde_json::json!({"error":[],"result":{"XXBTZUSD":{"c":["12345.0","0.1"]}}});
        let q = parse_alternate("BTC.CB=", &btc).unwrap();
        assert_eq!((q.source, q.price, q.ts), ("Kraken", 12345.0, None));
        assert!(parse_alternate("@DJ.1", &btc).is_none());
        assert!(parse_alternate("BTC.CB=", &serde_json::json!({"error":["bad"],"result":{}})).is_none());
    }

    #[test]
    fn parses_formatted_quotes() {
        let v: Value = serde_json::from_str(
            r#"{"FormattedQuoteResult":{"FormattedQuote":[
              {"symbol":"XAU=","name":"Gold / US Dollar Spot","last":"4,155.24","change_pct":"+1.22%",
               "previous_day_closing":"4,105.122","last_time":"2026-10-15T04:29:41.313-0400","realTime":"true",
               "open":"4,110.00","high":"4,190.00","low":"4,102.50","volume":"128400",
               "yrhiprice":"4,381.00","yrloprice":"3,940.00","yrhidate":"2026-09-22","currencyCode":"USD"},
              {"symbol":"@DJ.1","name":"Dow Jones Fut (Dec'26)","last":"52,078.00","change_pct":"-0.42%",
               "last_time":"2026-10-15T04:19:40.000-0400","realTime":"false",
               "open":"0.00","high":"0.00","low":"0.00","volume":"0","yrhiprice":"0.00","yrloprice":"0.00",
               "currencyCode":""},
              {"symbol":"BAD","last":"UNCH"}]}}"#,
        )
        .unwrap();
        let qs = parse_quotes(&v);
        assert_eq!(qs.len(), 2);
        assert_eq!((qs[0].price, qs[0].change_pct, qs[0].prev_close, qs[0].realtime), (4155.24, Some(1.22), Some(4105.122), true));
        assert_eq!((qs[1].price, qs[1].change_pct, qs[1].realtime), (52078.0, Some(-0.42), false));
        // The fields that were always there and used to be parsed away.
        assert_eq!((qs[0].open, qs[0].high, qs[0].low), (Some(4110.0), Some(4190.0), Some(4102.5)));
        assert_eq!(qs[0].volume, Some(128_400.0));
        assert_eq!((qs[0].yr_hi, qs[0].yr_lo), (Some(4381.0), Some(3940.0)));
        assert_eq!(qs[0].yr_hi_date.as_deref(), Some("2026-09-22"));
        assert_eq!(qs[0].currency, "USD");
        // `0.00` is how CNBC says "this symbol has no such value"; it must not draw as a price.
        assert_eq!((qs[1].high, qs[1].yr_lo, qs[1].volume), (None, None, None));
        assert_eq!(qs[1].currency, "");
    }

    #[test]
    fn lookup_answers_found_or_not_found() {
        let v: Value = serde_json::from_str(
            r#"{"FormattedQuoteResult":{"FormattedQuote":[{"symbol":"AAPL","name":"Apple Inc","last":"231.40"}]}}"#,
        )
        .unwrap();
        assert_eq!(parse_lookup(&v, "AAPL"), Lookup::Found { name: "Apple Inc".into(), price: 231.4 });
        assert_eq!(parse_lookup(&v, "MSFT"), Lookup::NotFound, "another symbol's quote is not this one");
        let lower: Value = serde_json::from_str(
            r#"{"FormattedQuoteResult":{"FormattedQuote":[{"symbol":"aapl","name":"Apple Inc","last":"231.40"}]}}"#,
        )
        .unwrap();
        assert_eq!(parse_lookup(&lower, "AAPL"), Lookup::Found { name: "Apple Inc".into(), price: 231.4 }, "a lower-case echo");

        for last in [r#""UNCH""#, r#""0.00""#] {
            let v: Value = serde_json::from_str(&format!(
                r#"{{"FormattedQuoteResult":{{"FormattedQuote":[{{"symbol":"AAPL","name":"Apple Inc","last":{last}}}]}}}}"#
            ))
            .unwrap();
            assert_eq!(parse_lookup(&v, "AAPL"), Lookup::NotFound, "last {last}");
        }
        let no_last: Value = serde_json::from_str(
            r#"{"FormattedQuoteResult":{"FormattedQuote":[{"symbol":"AAPL","name":"Apple Inc"}]}}"#,
        )
        .unwrap();
        assert_eq!(parse_lookup(&no_last, "AAPL"), Lookup::NotFound, "no last");

        for empty in [r#"{}"#, r#"[]"#, r#"null"#, r#"{"FormattedQuoteResult":{}}"#] {
            let v: Value = serde_json::from_str(empty).unwrap();
            assert_eq!(parse_lookup(&v, "AAPL"), Lookup::NotFound, "{empty}");
        }

        let no_name: Value = serde_json::from_str(
            r#"{"FormattedQuoteResult":{"FormattedQuote":[{"symbol":"AAPL","last":"231.40"}]}}"#,
        )
        .unwrap();
        assert_eq!(parse_lookup(&no_name, "AAPL"), Lookup::Found { name: String::new(), price: 231.4 });
    }

    #[test]
    fn chart_keeps_the_whole_payload_with_ohlc() {
        let v: Value = serde_json::from_str(
            r#"{"barData":{"priceBars":[
              {"close":"1.0","open":"0.5","high":"1.5","low":"0.25","tradeTimeinMills":1000000},
              {"close":"2.0","tradeTimeinMills":10000000},
              {"close":"3.0","tradeTimeinMills":10060000}]}}"#,
        )
        .unwrap();
        let (t0, bars) = parse_chart(&v, 10_100.0);
        // The whole day is kept now, not just the last hour: the chart needs it and the hour is a
        // slice of it.
        assert_eq!((t0, bars.len()), (1_060.0, 3));
        assert_eq!((bars[0].o, bars[0].h, bars[0].l, bars[0].c), (0.5, 1.5, 0.25, 1.0));
        assert_eq!(bars[2].t(t0), 10_100.0);
        // A bar with no o/h/l is a doji at its close, never a spike to zero.
        assert_eq!((bars[1].o, bars[1].h, bars[1].l, bars[1].c), (2.0, 2.0, 2.0, 2.0));
    }

    #[test]
    fn parses_the_bars_cnbc_sends_today() {
        // Copied verbatim from a live `1D.json` for `@SP.1`: prices arrive as strings with four
        // decimals, volume as a number, and the time in milliseconds. Two real bars and one flat
        // one, because a flat bar is what a spot symbol sends for most of its minutes.
        let v: Value = serde_json::from_str(
            r#"{"barData":{"priceBars":[
              {"open":"7626.0000","high":"7638.7500","low":"7626.0000","close":"7635.7500","volume":3443,"tradeTime":"20261101180000","tradeTimeinMills":1793574000000},
              {"open":"7635.7500","high":"7636.0000","low":"7626.5000","close":"7627.7500","volume":1026,"tradeTime":"20261101180100","tradeTimeinMills":1793574060000},
              {"open":"7584.0000","high":"7584.0000","low":"7584.0000","close":"7584.0000","volume":0,"tradeTime":"20261102191100","tradeTimeinMills":1793664660000}]}}"#,
        )
        .unwrap();
        let (t0, bars) = parse_chart(&v, 1_793_664_900.0);
        assert_eq!(bars.len(), 3);
        assert_eq!(t0, 1_793_574_060.0, "the first bar stamped at its end");
        assert_eq!((bars[0].o, bars[0].h, bars[0].l, bars[0].c), (7_626.0, 7_638.75, 7_626.0, 7_635.75));
        assert_eq!(bars[2].dt, 90_660.0, "offsets, not absolute times");
        // A flat bar survives as a degenerate candle rather than vanishing.
        assert_eq!((bars[2].o, bars[2].h, bars[2].l, bars[2].c), (7_584.0, 7_584.0, 7_584.0, 7_584.0));
    }

    #[test]
    fn obsolete_chart_fetch_does_not_erase_remapped_board_bars() {
        let cfg = Config::default();
        let rows = cfg.rows[..2].to_vec();
        let mut board = Board::new(&rows);
        let bar = |c| Bar { dt: 0.0, o: c, h: c, l: c, c, v: 0.0 };
        board.bars = vec![vec![bar(1.0)].into(), vec![bar(2.0)].into()];
        board.bars_t0 = vec![100.0, 200.0];
        board.backfill = vec![vec![(100.0, 1.0)], vec![(200.0, 2.0)]];
        board.backfill_gen = 1;
        let shared = Arc::new(Mutex::new(board));
        let mut feed = Feed {
            cfg, rows: rows.clone(), rows_gen: 0, shared: shared.clone(), notify: Arc::new(|| {}),
            client: Client::new().unwrap(), quotes: HashMap::new(), chart: Vec::new(),
            backfill: Vec::new(), backfill_gen: 1, bars_next: Instant::now(), bars_due: Vec::new(), fails: 0,
        };
        // A row change races a fetch. Its publication is rejected as stale.
        shared.lock().unwrap().set_rows(&rows[1..]);
        feed.accept_bars(vec![(100.0, vec![bar(3.0)]), (200.0, vec![bar(4.0)])]);
        feed.publish_bars();
        feed.sync_rows();
        feed.publish_bars();
        let board = shared.lock().unwrap();
        assert_eq!(board.bars[0][0].c, 2.0, "obsolete result must not clear retained candles");
        assert_eq!(board.bars_t0[0], 200.0);
        assert_eq!(board.backfill[0][0].1, 2.0);
    }

    #[test]
    fn row_change_then_partial_failure_keeps_remapped_cached_bars() {
        let cfg = Config::default();
        let rows = cfg.rows[..2].to_vec();
        let mut board = Board::new(&rows);
        let bar = |c| Bar { dt: 0.0, o: c, h: c, l: c, c, v: 0.0 };
        board.bars = vec![vec![bar(1.0)].into(), vec![bar(2.0)].into()];
        board.bars_t0 = vec![100.0, 200.0];
        board.backfill_gen = 1;
        let shared = Arc::new(Mutex::new(board));
        let mut feed = Feed {
            cfg, rows: rows.clone(), rows_gen: 0, shared: shared.clone(), notify: Arc::new(|| {}),
            client: Client::new().unwrap(), quotes: HashMap::new(), chart: Vec::new(),
            backfill: Vec::new(), backfill_gen: 1, bars_next: Instant::now(), bars_due: Vec::new(), fails: 0,
        };
        shared.lock().unwrap().set_rows(&[rows[1].clone(), rows[0].clone()]);
        feed.sync_rows();
        feed.accept_bars(vec![(0.0, vec![]), (300.0, vec![bar(3.0)])]);
        feed.publish_bars();
        let board = shared.lock().unwrap();
        assert_eq!(board.bars[0][0].c, 2.0, "failed symbol retains its own remapped cache");
        assert_eq!(board.bars[1][0].c, 3.0);
    }

    #[test]
    fn failed_first_fetch_after_row_change_still_retries_promptly() {
        let cfg = Config::default();
        let rows = cfg.rows[..2].to_vec();
        let mut board = Board::new(&rows[..1]);
        let bar = Bar { dt: 0.0, o: 1.0, h: 1.0, l: 1.0, c: 1.0, v: 0.0 };
        board.bars = vec![vec![bar].into()];
        board.bars_t0 = vec![100.0];
        board.backfill_gen = 1;
        let shared = Arc::new(Mutex::new(board));
        let mut feed = Feed {
            cfg, rows: rows[..1].to_vec(), rows_gen: 0, shared: shared.clone(), notify: Arc::new(|| {}),
            client: Client::new().unwrap(), quotes: HashMap::new(), chart: Vec::new(),
            backfill: Vec::new(), backfill_gen: 1, bars_next: Instant::now(), bars_due: Vec::new(), fails: 0,
        };
        shared.lock().unwrap().set_rows(&rows);
        feed.sync_rows();
        let start = Instant::now();
        feed.accept_bars(vec![(0.0, vec![]), (0.0, vec![])]);
        assert!(feed.bars_next >= start + BARS_RETRY);
        assert!(feed.bars_next <= Instant::now() + BARS_RETRY);
        assert_eq!(feed.chart[0].1[0].c, 1.0);
        assert!(feed.chart[1].1.is_empty());
        let next = Instant::now();
        feed.accept_bars(vec![(0.0, vec![]), (0.0, vec![])]);
        assert!(feed.bars_next >= next + BARS_RETRY);
        assert!(feed.bars_next <= Instant::now() + BARS_RETRY);
        feed.accept_bars(vec![(0.0, vec![]), (300.0, vec![bar])]);
        let next = Instant::now();
        feed.accept_bars(vec![(0.0, vec![]), (0.0, vec![])]);
        assert!(feed.bars_next >= next + BARS_RETRY);
        assert!(feed.bars_next <= Instant::now() + BARS_RETRY);
    }

    #[test]
    fn blocked_chart_does_not_block_quotes_or_apply_obsolete_rows() {
        use std::sync::mpsc;
        let cfg = Config::default();
        let rows = cfg.rows[..2].to_vec();
        let shared = Arc::new(Mutex::new(Board::new(&rows)));
        let make_feed = || Feed {
            cfg: cfg.clone(), rows: rows.clone(), rows_gen: 0, shared: shared.clone(),
            notify: Arc::new(|| {}), client: Client::new().unwrap(), quotes: HashMap::new(),
            chart: Vec::new(), backfill: Vec::new(), backfill_gen: 0,
            bars_next: Instant::now(), bars_due: Vec::new(), fails: 0,
        };
        let mut chart_feed = make_feed();
        let quote_feed = make_feed();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            chart_feed.maybe_bars_with(|_| {
                entered_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                (100.0, vec![Bar { dt: 0.0, o: 1.0, h: 1.0, l: 1.0, c: 1.0, v: 0.0 }])
            })
        });
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        quote_feed.publish(Link::Live, String::new());
        assert_eq!(shared.lock().unwrap().link, Link::Live);
        shared.lock().unwrap().set_rows(&rows[1..]);
        release_tx.send(()).unwrap();
        assert!(!thread.join().unwrap(), "obsolete result must be discarded");
        assert!(shared.lock().unwrap().bars[0].is_empty());
    }

    #[test]
    fn remove_then_readd_before_sync_retries_the_lost_chart() {
        let cfg = Config::default();
        let rows = cfg.rows[..2].to_vec();
        let mut board = Board::new(&rows);
        let bar = Bar { dt: 0.0, o: 1.0, h: 1.0, l: 1.0, c: 1.0, v: 0.0 };
        board.bars = vec![vec![bar].into(), vec![bar].into()];
        board.bars_t0 = vec![100.0, 100.0];
        let shared = Arc::new(Mutex::new(board));
        let now = Instant::now();
        let mut feed = Feed { cfg, rows: rows.clone(), rows_gen: 0, shared: shared.clone(), notify: Arc::new(|| {}),
            client: Client::new().unwrap(), quotes: HashMap::new(), chart: vec![(100.0, vec![bar]); 2],
            backfill: Vec::new(), backfill_gen: 0, bars_next: now + BARS_REFRESH,
            bars_due: vec![now + BARS_REFRESH; 2], fails: 0 };
        shared.lock().unwrap().set_rows(&rows[1..]);
        shared.lock().unwrap().set_rows(&rows);
        feed.sync_rows();
        assert!(feed.bars_due[0] <= Instant::now(), "re-added row lost its cached data");
        assert_eq!(feed.bars_due[1], now + BARS_REFRESH);
    }

    #[test]
    fn rejected_chart_publication_retries_after_resync() {
        let cfg = Config::default();
        let rows = cfg.rows[..2].to_vec();
        let shared = Arc::new(Mutex::new(Board::new(&rows)));
        let mut feed = Feed { cfg, rows: rows.clone(), rows_gen: 0, shared: shared.clone(), notify: Arc::new(|| {}),
            client: Client::new().unwrap(), quotes: HashMap::new(), chart: Vec::new(),
            backfill: Vec::new(), backfill_gen: 0, bars_next: Instant::now(), bars_due: Vec::new(), fails: 0 };
        let bar = Bar { dt: 0.0, o: 1.0, h: 1.0, l: 1.0, c: 1.0, v: 0.0 };
        assert!(feed.maybe_bars_with(|_| (100.0, vec![bar])));
        shared.lock().unwrap().set_rows(&[rows[1].clone(), rows[0].clone()]);
        feed.publish_bars();
        feed.sync_rows();
        assert!(feed.bars_due[1] <= Instant::now(), "rejected success must not suppress a retry");
    }

    #[test]
    fn row_changes_preserve_surviving_chart_deadlines() {
        let cfg = Config::default();
        let rows = cfg.rows[..3].to_vec();
        let shared = Arc::new(Mutex::new(Board::new(&rows[..2])));
        let now = Instant::now();
        let mut feed = Feed {
            cfg, rows: rows[..2].to_vec(), rows_gen: 0, shared: shared.clone(), notify: Arc::new(|| {}),
            client: Client::new().unwrap(), quotes: HashMap::new(), chart: Vec::new(),
            backfill: Vec::new(), backfill_gen: 0, bars_next: now + BARS_RETRY,
            bars_due: vec![now + BARS_RETRY, now + BARS_REFRESH], fails: 0,
        };
        shared.lock().unwrap().set_rows(&[rows[1].clone(), rows[0].clone(), rows[2].clone()]);
        feed.sync_rows();
        assert_eq!(feed.bars_due[0], now + BARS_REFRESH);
        assert_eq!(feed.bars_due[1], now + BARS_RETRY);
        assert!(feed.bars_due[2] <= Instant::now());
        let mut seen = String::new();
        feed.maybe_bars_with(|s| { seen = s.to_string(); (0.0, Vec::new()) });
        assert_eq!(seen, rows[2].symbol, "the newly added row goes before a backoff retry");
    }

    #[test]
    fn chart_scheduler_retries_only_the_failed_row() {
        let cfg = Config::default();
        let rows = cfg.rows[..2].to_vec();
        let shared = Arc::new(Mutex::new(Board::new(&rows)));
        let mut feed = Feed {
            cfg, rows: rows.clone(), rows_gen: 0, shared, notify: Arc::new(|| {}),
            client: Client::new().unwrap(), quotes: HashMap::new(), chart: Vec::new(),
            backfill: Vec::new(), backfill_gen: 0, bars_next: Instant::now(), bars_due: Vec::new(), fails: 0,
        };
        let bar = Bar { dt: 0.0, o: 1.0, h: 1.0, l: 1.0, c: 1.0, v: 0.0 };
        let mut seen = Vec::new();
        assert!(feed.maybe_bars_with(|s| { seen.push(s.to_string()); (100.0, vec![bar]) }));
        feed.publish_bars();
        assert!(!feed.maybe_bars_with(|s| { seen.push(s.to_string()); (0.0, vec![]) }));
        assert_eq!(seen, vec![rows[0].symbol.clone(), rows[1].symbol.clone()]);
        assert!(feed.bars_due[1] < feed.bars_due[0]);
        feed.bars_due[1] = Instant::now();
        feed.bars_next = feed.bars_due[1];
        assert!(feed.maybe_bars_with(|s| { seen.push(s.to_string()); (200.0, vec![bar]) }));
        feed.publish_bars();
        assert_eq!(seen[2], rows[1].symbol);
        let board = feed.shared.lock().unwrap();
        assert_eq!(board.bars_t0, vec![100.0, 200.0]);
        assert_eq!(board.bars[0][0].c, 1.0);
        assert_eq!(board.bars[1][0].c, 1.0);
    }

    #[test]
    fn partial_failure_schedules_a_retry_despite_a_successful_row() {
        let cfg = Config::default();
        let rows = cfg.rows[..2].to_vec();
        let shared = Arc::new(Mutex::new(Board::new(&rows)));
        let mut feed = Feed {
            cfg, rows, rows_gen: 0, shared, notify: Arc::new(|| {}),
            client: Client::new().unwrap(), quotes: HashMap::new(), chart: Vec::new(),
            backfill: Vec::new(), backfill_gen: 0, bars_next: Instant::now(), bars_due: Vec::new(), fails: 0,
        };
        let bar = Bar { dt: 0.0, o: 1.0, h: 1.0, l: 1.0, c: 1.0, v: 0.0 };
        let before = Instant::now();
        feed.accept_bars(vec![(100.0, vec![bar]), (0.0, vec![])]);
        assert!(feed.bars_next <= before + BARS_RETRY + Duration::from_secs(1), "the failed row must not wait ten minutes");
    }

    #[test]
    fn a_timed_out_symbol_keeps_the_bars_it_had() {
        let bar = |c: f32| Bar { dt: 0.0, o: c, h: c, l: c, c, v: 0.0 };
        let prev = vec![(100.0, vec![bar(1.0)]), (100.0, vec![])];
        // Row 0 lost its fetch this pass, row 1 won one: neither result may wipe the other.
        let got = vec![(0.0, vec![]), (200.0, vec![bar(2.0)])];
        let merged = merge_bars(&prev, got);
        assert_eq!(merged[0], (100.0, vec![bar(1.0)]), "the chart keeps its day");
        assert_eq!(merged[1], (200.0, vec![bar(2.0)]), "the new day wins over the empty one");
        // A changed row list is not merged at all: the indices no longer mean the same symbol.
        let moved = merge_bars(&prev, vec![(0.0, vec![])]);
        assert_eq!(moved.len(), 1);
        assert!(moved[0].1.is_empty());
    }

    #[test]
    fn encodes_cnbc_symbols() {
        assert_eq!(encode("XAU=|@DJ.1"), "XAU%3D%7C%40DJ.1");
    }

    #[test]
    fn a_kick_wakes_wait_early() {
        let cfg = Config::default();
        let rows = cfg.rows[..2].to_vec();
        let shared = Arc::new(Mutex::new(Board::new(&rows)));
        let feed = Feed { cfg, rows, rows_gen: 0, shared: shared.clone(), notify: Arc::new(|| {}),
            client: Client::new().unwrap(), quotes: HashMap::new(), chart: Vec::new(),
            backfill: Vec::new(), backfill_gen: 0, bars_next: Instant::now(), bars_due: Vec::new(), fails: 0 };
        let bumper = shared.clone();
        let started = Instant::now();
        let t = thread::spawn(move || {
            thread::sleep(Duration::from_millis(100));
            let mut b = bumper.lock().unwrap();
            b.kick = b.kick.wrapping_add(1);
        });
        feed.wait(Duration::from_secs(30), 0);
        t.join().unwrap();
        assert!(started.elapsed() < Duration::from_secs(2), "wait ignored the kick: {:?}", started.elapsed());
    }

    #[test]
    fn a_kick_makes_rows_without_bars_due_now_and_leaves_the_rest() {
        let cfg = Config::default();
        let rows = cfg.rows[..2].to_vec();
        let shared = Arc::new(Mutex::new(Board::new(&rows)));
        let bar = Bar { dt: 0.0, o: 1.0, h: 1.0, l: 1.0, c: 1.0, v: 0.0 };
        let later = Instant::now() + BARS_REFRESH;
        let mut feed = Feed { cfg, rows, rows_gen: 0, shared, notify: Arc::new(|| {}),
            client: Client::new().unwrap(), quotes: HashMap::new(),
            chart: vec![(100.0, vec![bar]), (100.0, Vec::new())],
            backfill: Vec::new(), backfill_gen: 0, bars_next: later, bars_due: vec![later; 2], fails: 0 };
        feed.kick_bars();
        assert_eq!(feed.bars_due[0], later, "a row with bars keeps its schedule");
        assert!(feed.bars_due[1] <= Instant::now(), "an empty row is due at once");
        assert!(feed.bars_next <= Instant::now());
    }

    #[test]
    fn a_kick_during_work_ends_the_next_wait_at_once() {
        let cfg = Config::default();
        let rows = cfg.rows[..2].to_vec();
        let shared = Arc::new(Mutex::new(Board::new(&rows)));
        let feed = Feed { cfg, rows, rows_gen: 0, shared: shared.clone(), notify: Arc::new(|| {}),
            client: Client::new().unwrap(), quotes: HashMap::new(), chart: Vec::new(),
            backfill: Vec::new(), backfill_gen: 0, bars_next: Instant::now(), bars_due: Vec::new(), fails: 0 };
        let acted = shared.lock().unwrap().kick; // baseline read before the "request"
        {
            let mut b = shared.lock().unwrap(); // the kick lands while the request is in flight
            b.kick = b.kick.wrapping_add(1);
        }
        let started = Instant::now();
        feed.wait(Duration::from_secs(30), acted);
        assert!(started.elapsed() < Duration::from_secs(1), "lost wake-up: {:?}", started.elapsed());
    }

    // ---- range files (P4) ----
    fn ms(secs: i64) -> i64 {
        secs * 1000
    }

    #[test]
    fn parse_series_reads_both_time_spellings() {
        let v = serde_json::json!({"barData": {"priceBars": [
            {"close": "10.0", "tradeTimeinMills": ms(1_793_500_000)},
            {"close": "11.0", "tradeTimeInMills": ms(1_793_586_400)},
            {"close": "12.0", "tradeTimeinMills": (ms(1_793_672_800)).to_string()}
        ]}});
        let s = parse_series(&v, 1_800_000_000.0);
        assert_eq!(s.t0, 1_793_500_000.0);
        assert_eq!(s.bars.len(), 3);
        let times: Vec<f64> = s.bars.iter().map(|b| b.t(s.t0)).collect();
        // The time is the bar START: no 60 s shift, and no clamp to `now`.
        assert_eq!(times, [1_793_500_000.0, 1_793_586_400.0, 1_793_672_800.0]);
        assert_eq!(s.bars[1].dt, 86_400.0);
        assert_eq!(s.bar_s, 86_400.0);
        assert_eq!(s.fetched, 1_800_000_000.0);
    }

    #[test]
    fn parse_series_fills_missing_fields() {
        let v = serde_json::json!({"barData": {"priceBars": [
            {"close": "5.5", "tradeTimeinMills": ms(100), "volume": 0},
            {"close": "6.0", "open": "5.0", "high": "7.0", "low": "4.0", "tradeTimeinMills": ms(160), "volume": -3},
            {"close": "7.0", "tradeTimeinMills": ms(220), "volume": "text"},
            {"close": "8.0", "tradeTimeinMills": ms(280), "volume": 3443},
            {"close": "9.0", "tradeTimeinMills": ms(340), "volume": "1,026"},
            {"open": "1.0", "high": "2.0", "low": "0.5", "tradeTimeinMills": ms(400)},
            {"close": "2.0", "open": "1.0"}
        ]}});
        let s = parse_series(&v, 1_000.0);
        // The bar without a close and the bar without a time are skipped: five bars remain.
        assert_eq!(s.bars.len(), 5);
        let b = s.bars[0];
        assert_eq!((b.o, b.h, b.l, b.c), (5.5, 5.5, 5.5, 5.5), "no o/h/l: the close");
        assert_eq!(b.v, 0.0, "volume 0 is none");
        assert_eq!(s.bars[1].v, 0.0, "negative volume is none");
        assert_eq!(s.bars[2].v, 0.0, "text volume is none");
        assert_eq!(s.bars[3].v, 3443.0);
        assert_eq!(s.bars[4].v, 1026.0, "a comma-grouped string counts");
        assert_eq!((s.bars[1].o, s.bars[1].h, s.bars[1].l), (5.0, 7.0, 4.0));
    }

    #[test]
    fn parse_series_sorts_and_caps() {
        // Unsorted input comes back in time order.
        let v = serde_json::json!({"barData": {"priceBars": [
            {"close": "3", "tradeTimeinMills": ms(300)},
            {"close": "1", "tradeTimeinMills": ms(100)},
            {"close": "2", "tradeTimeinMills": ms(200)}
        ]}});
        let s = parse_series(&v, 1_000.0);
        let closes: Vec<f32> = s.bars.iter().map(|b| b.c).collect();
        assert_eq!(closes, [1.0, 2.0, 3.0]);
        assert_eq!((s.t0, s.bars[2].dt), (100.0, 200.0));

        // 4,001 minute bars keep the newest 4,000; t0 is the second bar's time.
        let many: Vec<Value> = (0..4_001)
            .map(|i| serde_json::json!({"close": "1", "tradeTimeinMills": ms(60 * i)}))
            .collect();
        let v = serde_json::json!({"barData": {"priceBars": many}});
        let s = parse_series(&v, 1e9);
        assert_eq!(s.bars.len(), SERIES_BARS);
        assert_eq!(s.t0, 60.0);
        assert_eq!(s.bar_s, 60.0);
    }

    #[test]
    fn parse_series_keeps_the_later_of_two_bars_at_one_instant() {
        let v = serde_json::json!({"barData": {"priceBars": [
            {"close": "1", "tradeTimeinMills": ms(100)},
            {"close": "2", "tradeTimeinMills": ms(100)},
            {"close": "3", "tradeTimeinMills": ms(160)}
        ]}});
        let s = parse_series(&v, 1_000.0);
        let closes: Vec<f32> = s.bars.iter().map(|b| b.c).collect();
        assert_eq!(closes, [2.0, 3.0]);
    }

    #[test]
    fn parse_series_survives_junk() {
        let junk = [
            serde_json::json!({}),
            serde_json::json!(null),
            serde_json::json!("x"),
            serde_json::json!({"barData": null}),
            serde_json::json!({"barData": {"priceBars": []}}),
            serde_json::json!({"barData": {"priceBars": [1, "a", {}]}}),
            serde_json::json!({"barData": {"priceBars": [{"close": "NaN", "tradeTimeinMills": ms(5)}]}}),
        ];
        for v in &junk {
            let s = parse_series(v, 777.0);
            assert!(s.bars.is_empty(), "{v}");
            assert_eq!((s.t0, s.fetched, s.bar_s), (777.0, 777.0, 0.0), "{v}");
        }
    }

    #[test]
    fn parse_chart_reads_volume() {
        let v = serde_json::json!({"barData": {"priceBars": [
            {"open": "7626.0000", "high": "7638.7500", "low": "7626.0000", "close": "7635.7500", "volume": 3443, "tradeTimeinMills": 1793574000000i64},
            {"open": "7635.7500", "high": "7636.0000", "low": "7626.5000", "close": "7627.7500", "volume": 1026, "tradeTimeinMills": 1793574060000i64},
            {"open": "7584.0000", "high": "7584.0000", "low": "7584.0000", "close": "7584.0000", "volume": 0, "tradeTimeinMills": 1793664660000i64}
        ]}});
        let (_t0, bars) = parse_chart(&v, 1_793_664_900.0);
        let vols: Vec<f32> = bars.iter().map(|b| b.v).collect();
        assert_eq!(vols, [3443.0, 1026.0, 0.0]);
    }
}
