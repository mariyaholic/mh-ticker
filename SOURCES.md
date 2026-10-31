# Data sources and alert transports — research, 2026-10-22

*Answers two questions that came with the 1.3.0 brief: what else can feed the ticker, and how should
it talk outward. Every claim here has a source URL; anything I could not verify is marked as such.*

---

## Part 1 — Alert transports

### The decision that matters: webhook, not bot

| Direction | What it is | Cost |
|---|---|---|
| **Outbound webhook** | one HTTPS POST when an alert fires | ~1 request per alert. **Shipped in 1.3.0.** |
| **Discord bot** | the app holds a Gateway WebSocket open and *receives* commands | a persistent connection, a bot token, intents, heartbeats, resume logic |

A bot is not "the webhook, but more". It is a stateful long-lived connection that only works while
the desktop app runs, and it needs a credential that can read a whole server. For a single-exe
ticker that is the wrong trade: the user gets no benefit unless they already live in Discord.

**If it is built anyway**, the parts that actually cost the effort:

- Endpoint `wss://gateway.discord.gg/?v=10&encoding=json`, obtainable from *Get Gateway Bot*.
  ([Gateway](https://docs.discord.com/developers/events/gateway))
- Handshake: the server sends `{"op":10,"d":{"heartbeat_interval":45000}}`; the client replies
  `{"op":2,"d":{"token":…,"intents":…,"properties":{…}}}` and then sends
  `{"op":1,"d":<last seq or null>}` every `heartbeat_interval`.
- Resuming uses `{"op":6,"d":{"token":…,"session_id":…,"seq":…}}`.
- Reading ordinary messages needs `GUILD_MESSAGES` **plus the privileged `MESSAGE_CONTENT`**, enabled
  in the Developer Portal; it is not needed for DMs or messages that mention the bot.
  ([privileged intents](https://docs.discord.com/developers/gateway/you-might-not-need-a-privileged-intent))
- Rust needs a WebSocket transport: `tungstenite` (~281 kB of crate source, sync, no async runtime).
  ([tungstenite](https://docs.rs/crate/tungstenite/0.30.0))

**Preferred alternative if interactivity is ever wanted:** slash commands, which need no message
content intent at all.

### Webhook shapes (what 1.3.0 posts)

| Transport | Endpoint | Body | Auth |
|---|---|---|---|
| **Discord** | `POST https://discord.com/api/v10/webhooks/{id}/{token}?wait=true` | `{"content":…,"allowed_mentions":{"parse":[]}}` | none — the URL token *is* the credential |
| **Slack** | incoming webhook URL | `{"text":…}` | none |
| **ntfy** | `POST https://ntfy.sh/{topic}` | plain text body, title in a `Title:` header | topic name is the secret |
| **Telegram** | `POST https://api.telegram.org/bot{token}/sendMessage` | `{"chat_id":…,"text":…}` | token in the URL |
| **generic / Home Assistant** | user's URL | `{"text":…,"title":…,"body":…}` | varies |

Discord limits worth knowing: `content` ≤ 2,000 chars, ≤ 10 embeds, **6,000 chars across all embeds**,
and — the number that matters — **~30 messages per minute per webhook**.
([webhook resource](https://docs.discord.com/developers/resources/webhook),
[rate limits](https://docs.discord.com/developers/topics/rate-limits))
A 429 returns `{"retry_after": 1.25, "global": false}` plus `X-RateLimit-*` headers; the app's 30-second
cooldown floor is two orders of magnitude below the ceiling, so rate limiting is not reachable in
normal use.

Two deliberate behaviours in `src/alerts.rs`:

1. **Errors are reduced to `HTTP <code>` or `network`.** `ureq`'s own error text can echo the URL, and
   for a webhook the URL *is* the password. `redact()` keeps only the host in any message a human sees.
2. **Nothing is posted for a stale quote.** A market closed at a level would otherwise ping forever.

### Security follow-up not done in 1.3.0

The webhook URL is stored in plaintext in `config_ultra.json`. The right storage is Windows DPAPI —
`CryptProtectData` from `crypt32` (no extra crate, no runtime), user-scoped by default, freed with
`LocalFree`. It protects the secret **at rest**; it does not stop malware running as the same user.
([CryptProtectData](https://learn.microsoft.com/en-us/windows/win32/api/dpapi/nf-dpapi-cryptprotectdata))

---

## Part 2 — Other data sources

The current catalogue is 23 CNBC symbols with two CNBC endpoints. What else is reachable from a
keyless native app, and what is actually worth adding.

### Ranked: the five worth adding first

All five use Yahoo's chart endpoint, which returned JSON **without a crumb or cookie** when tested,
and share one parser:

```
https://query1.finance.yahoo.com/v8/finance/chart/{SYMBOL}?range=1d&interval=1m
chart.result[0].meta.regularMarketPrice          -> price
chart.result[0].meta.chartPreviousClose          -> previous close (fallback: meta.previousClose)
chart.result[0].timestamp[] + indicators.quote[0].{open,high,low,close,volume}
```

| # | Symbol | Why |
|---|---|---|
| 1 | `^GSPC` | S&P 500 **cash** — the ticker only has futures (`@SP.1`), which are a different instrument |
| 2 | `^IXIC` | Nasdaq Composite, same reason |
| 3 | `^DJI` | Dow cash |
| 4 | `AAPL` | first single stock; proves the path for any equity |
| 5 | `NVDA` | highest-interest single name |

**Caveat, stated plainly: this is an unofficial endpoint.** No published quota, no SLA, variable 429
behaviour, and 2025 reports of crumb/cookie failures on the *quote* routes (the chart route kept
working). Keep CNBC as primary and treat Yahoo as an optional, failure-tolerant source.
([yfinance 429 report](https://github.com/ranaroussi/yfinance/issues/2441))

### Crypto — already covered, but worth knowing the alternatives

| Source | Endpoint | Note |
|---|---|---|
| Coinbase Exchange | `api.exchange.coinbase.com/products/BTC-USD/{ticker,stats}` | **10 req/s per IP**, burst 15, 429 beyond |
| Binance | `data-api.binance.vision/api/v3/ticker/24hr?symbol=BTCUSDT` | has `prevClosePrice` — unusual and useful |
| Kraken | `api.kraken.com/0/public/Ticker?pair=XBTUSD` | ≤ 720 OHLC entries, cannot go deep |
| CoinGecko | `api.coingecko.com/api/v3/simple/price?ids=bitcoin` | free tier shared by IP, not for scheduled use |

CNBC's `BTC.CB=` is already Coinbase, so this is mostly a **failover** path rather than a feature.

### FX and metals — the honest gap

There is no dependable **keyless, licensed, intraday** source for spot metals or broad FX.

- **Frankfurter** (`api.frankfurter.dev/v2/rate/eur/usd`) is excellent and free, but it is a *daily
  reference* rate (ECB, ~16:00 CET) and carries no metals. Fine for a "rate" display, useless for an
  intraday line. ([frankfurter](https://frankfurter.dev/))
- **Metals.Dev** and **GoldAPI.net** need keys; GoldAPI.net is 100 requests/month.
  ([GoldAPI.net](https://goldapi.net/docs))
- Keys inside a distributed `.exe` are extractable, so a keyed source cannot be a default.

**This is why CNBC stays the primary feed.** It is the only free source found that gives spot metals
*and* index futures *and* yields with a 3-second cadence and no key.

### Oddball sources that would fit the strip nicely

| Data | Endpoint | Key | Note |
|---|---|---|---|
| Fear & Greed | `api.alternative.me/fng/?limit=2&format=json` → `data[0].value` | none | daily; **attribution required** |
| Treasury yields | `home.treasury.gov/…/xml?data=daily_treasury_yield_curve&field_tdr_date_value=YYYY` | none | XML, daily, from 1990 |
| Weather | `api.open-meteo.com/v1/forecast` | none | free tier **non-commercial**, 10k calls/day |
| Air quality | Open-Meteo Air Quality | none | same terms |
| Tides | NOAA CO-OPS `api.tidesandcurrents.noaa.gov/api/prod/` | none | station-based |
| Sports | `site.api.espn.com/apis/site/v2/sports/{sport}/{league}/scoreboard` | none | undocumented, community-mapped |

Weather and Fear & Greed are the two that would actually earn strip space: both are a single number
that changes, which is exactly what the strip is for.

### Compression, again

None of these document gzip per-response. The lesson from 1.3.0 applies to any new source: **verify
the negotiated encoding rather than assuming**, because a silent `default-features = false` cost this
app 6× its traffic for months.

---

## What I could not verify

- Kraken's and Stooq's own docs pages did not extract in this pass; their limits are from secondary
  reporting and should be re-checked before use.
- No source's response `Content-Encoding` was confirmed from inside this app — the byte figures in
  `FEATURES.md` come from `curl`, not from instrumented requests.
- Webhook delivery was exercised against `payload()` and `redact()` in unit tests only, never against
  a live Discord channel.
