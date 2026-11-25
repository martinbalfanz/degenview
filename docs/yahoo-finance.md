# Yahoo Finance provider (development branch)

This branch begins with a native, keyless Yahoo client in `service::market::yahoo`. It follows Wickline's chart, search and spark endpoints and two-host transport. It uses the app's existing proxy-aware, bounded HTTP client; it does not require Node, an API key or new dependencies.

The client deliberately keeps original UTC timestamps, optional volume, exchange timezone, currency and quote timestamps. It does not use a present-day UTC offset to reconstruct historical sessions, substitute zero for absent volume, fabricate candles or silently retry HTTP 429 through another hostname. The exchange's native data may be delayed. These endpoints are unofficial and their availability, limits and usage terms need verification before release.

## Remaining integration work

The draft registers Yahoo for US equities, ETFs and selected indexes plus nine international exchange profiles and selected FX/futures, symbol search, historical charts and REST watchlist quotes. It folds longer candles from native 1/5/15-minute and daily bars on DegenView's UTC grid (not Wickline's session-open grid). Successful quote refreshes are marked 'May be delayed' (never certified live), and become stale on failed or expired refreshes. A shared 30-second cooldown follows HTTP 429. Historical ranges exclude observations at or beyond the exclusive end, and closing prints cannot update an earlier candle across a gap. The cache refetches the whole series when a refreshed tail revises a completed Yahoo candle's prices; this reduces the risk of mixed adjustment bases after corporate actions. Paper trading and price alerts are disabled for Yahoo. Before proposing upstream:
- Validate interval planning, older-history bounds (1m: 7 days; 5m/15m: 60 days by conservative policy, including folded hourly charts), UTC folds versus session-anchored candles and official close prints against live data. Yahoo's ordinary chart OHLC (not dividend-adjusted adjclose) is used, like Wickline. Verify split adjustment and dividend behavior against live fixtures.
- Verify picker, quote freshness, caching, source mark, persistence and translations on supported platforms. International markets require instrument-level currency, asset type and session metadata.
- Finish auditing Pine alerts, replay, session statistics, portfolio, source-specific assumptions and MCP. Yahoo is not yet exposed as an MCP input provider.
- Validate the initial US equity/ETF ticker stream during regular trading hours, including disconnect behavior, exchange delays and volume. REST remains active every 30 seconds. Chart-candle streaming requires a separate reconciliation design.
- The fork owner confirmed the initial integration and the subsequent manual checklist (NVDA timeframe changes, AAPL daily history, BRK-B, ETFs, watchlist quotes and persistence). The fork owner also confirmed the US index implementation works. The fork owner subsequently confirmed the Xetra increment works. The additional international profiles also passed the fork owner's manual testing. FX/futures symbols also worked in the fork owner's manual testing. The streaming increment requires compilation and live validation. Run cargo fmt, test and clippy on subsequent changes; manually verify AAPL, BRK-B, holidays, daylight-saving transitions, range limits and network failures. Unlisted international exchanges, currency pairs and futures remain out of scope.

## US market indexes

Supported symbols: `^IXIC` (Nasdaq Composite), `^NDX` (Nasdaq 100), `^GSPC` (S&P 500), `^DJI` (Dow Jones Industrial Average), `^RUT` (Russell 2000) and `^VIX` (Cboe Volatility Index). Search and direct chart/watchlist selection use the same Yahoo source. Chart and quote metadata must identify a USD index with a New York or Chicago timezone; other indexes require a separately validated international profile. The Stocks tab includes these index results, individually labeled Index.

Index levels are not traded assets. They are excluded from the portfolio asset picker and shared recent-market shortcuts for now (they can be saved to watchlists). Vendor volume is withheld because an index may report an aggregate or no volume at all. Until Kline supports optional volume, index candles use an internal zero placeholder; chart volume bars and MCP candle volume are suppressed, and Volume Candles render as ordinary candles. Pine scripts that read raw volume may still see the placeholder; do not use volume-based indicators on these indexes.

## First international exchange: Xetra

Yahoo `.DE` equities and ETFs (e.g. SAP.DE, SIE.DE, BAS.DE, ALV.DE) are accepted only when chart/quote metadata confirms exchange GER, currency EUR, Europe/Berlin and EQUITY or ETF. The DAX index `^GDAXI` requires EUR, Europe/Berlin and INDEX. This is a deliberately limited, deterministic market profile, not a general instrument metadata service. Symbols outside supported profiles remain rejected.

Prices stay in EUR, not converted to USD. Search details, chart watermark and watchlist prices indicate EUR; Pine receives EUR and Berlin time. Session statistics and Pine regular-session flags use Xetra core hours (09:00–17:30 weekdays, with DST); holiday/early-close calendars and extended retail hours are not modeled. Daily candles retain the UTC date of the local core session. The portfolio picker and shared recent shortcuts exclude Xetra until currency-aware portfolio entry and validation are reviewed.

## Additional international exchange profiles

The single `YahooMarket` profile registry defines symbol family, expected exchange code, currency, timezone and regular weekday hours. Search filters by exchange and kind; actual chart and spark metadata must agree with the profile before data is displayed. Prices are not FX-converted.

| Exchange | Suffix | Yahoo exchange code | Currency | Core hours (local) | Example |
|---|---|---|---|---|---|
| Xetra | .DE | GER | EUR | 09:00–17:30 | SAP.DE |
| Paris | .PA | PAR | EUR | 09:00–17:30 | MC.PA |
| Amsterdam | .AS | AMS | EUR | 09:00–17:30 | ASML.AS |
| Brussels | .BR | BRU | EUR | 09:00–17:30 | ABI.BR |
| Milan | .MI | MIL | EUR | 09:00–17:30 | ENI.MI |
| Madrid | .MC | MCE | EUR | 09:00–17:30 | SAN.MC |
| Zurich | .SW | EBS | CHF | 09:00–17:20 | NESN.SW |
| London | .L | LSE | GBP | 08:00–16:30 | SHEL.L |
| Toronto | .TO | TOR | CAD | 09:30–16:00 | RY.TO |

Additional index candidates: `^FCHI`, `^AEX`, `^BFX`, `FTSEMIB.MI`, `^IBEX`, `^SSMI`, `^FTSE`, `^GSPTSE`. Each still requires matching index type, currency and timezone.

London vendor GBp or GBX prices are multiplied by 0.01; GBP prices are unchanged. This applies to all OHLC fields, last price and previous close, never volume. Non-local-currency listings on any of these exchanges are rejected, including USD/EUR share classes in London, until full dynamic instrument metadata is supported. The new market profiles remain excluded from portfolio selection and shared recent shortcuts. Regular session flags/statistics use weekday core hours, not holiday, auction or early-close calendars.

Exchange code references: [yfinance EquityQuery](https://ranaroussi.github.io/yfinance/reference/api/yfinance.EquityQuery.html). London pence issue: [portfolio-performance #4038](https://github.com/portfolio-performance/portfolio/issues/4038). Session references: [Euronext trading hours and holidays](https://www.euronext.com/en/trading/trading-hours-holidays), [Nordnet trading venues](https://www.nordnet.de/faq/wertpapierhandel-und-sparplaene/an-welchen-handelsplaetzen-kann-ich-ueber-nordnet-handeln).

## Forex and rolling futures

Supported FX pairs: EURUSD=X, GBPUSD=X, USDJPY=X (also JPY=X), USDCHF=X (CHF=X), USDCAD=X (CAD=X), AUDUSD=X, NZDUSD=X, EURGBP=X, EURJPY=X. Futures: ES=F, NQ=F, GC=F, CL=F. Chart and quote metadata must match the expected kind, quote currency and exchange (CCY, CME, CMX, NYM). Forex has no meaningful centralized volume; hide it, use an internal zero placeholder. Futures volume is contracts, not shares. FX and futures appear in the Traditional markets picker and can be saved to watchlists; neither enters portfolio selection.

Pine receives the market kind, quote currency and FX base currency. The session timezone convention is New York for FX, Chicago for futures, with evening observations assigned to the next trading date. Futures use approximate Globex weekday sessions (17:00–16:00 CT, Sunday evening through Friday); FX uses 17:00 ET Sunday through 17:00 Friday. See [CME E-mini overview](https://www.cmegroup.com/education/lessons/product-e-mini-s-and-p) and [Gold specs](https://www.cmegroup.com/markets/metals/precious/gold/specs). Holiday, early-close and product-specific intraday halts are not modeled. Native daily bars are labeled by trading date using the timestamp and these conventions; intraday bars continue to use the UTC grid. A forming evening daily observation with a future UTC date label may be withheld until that date.

Yahoo's =F symbols are vendor-managed rolling series, not executable contracts or guaranteed back-adjusted histories. Contract rolls can cause discontinuities. Pine strategy P&L, point value and contract sizing are not yet futures-aware; use these instruments for chart/indicator analysis only, not strategy performance or execution. FX quotes are indicative, not executable bid/ask. No real-time certification.

## Yahoo WebSocket ticker (first increment)

One shared subscription per active watchlist coordinator uses Wickline's Yahoo v2 WebSocket endpoint with the existing proxy-aware Session, heartbeat, reconnect and shutdown behavior. The payload decoder is bounded, checks protobuf lengths and varints, and reads the explicit vendor timestamp, currency, price, session and optional cumulative share volume. Only regular-session USD stock/ETF ticker symbols are subscribed (at most 100 sorted symbols); indexes, FX, futures and international listings continue through REST. A REST quote must establish the market before ticks are accepted. Reject old/out-of-order or implausibly future ticks and ticks more than 30 minutes behind receipt; all accepted Yahoo quotes remain marked potentially delayed. An older REST snapshot cannot rewind a newer tick. REST remains on its existing cadence for metadata, previous-close reference, recovery and unsupported/quiet markets. Previous close is carried from REST only within the same UTC trading date. The decoder does not manufacture candles or increment cumulative volume. Chart OHLCV continues to refresh through REST.

Run the offline decoder/merge tests and, during a US regular session, `cargo test --lib live_yahoo_receives_regular_session_tick -- --ignored --nocapture`. Live connectivity and payload variation still require verification. Extending streaming to charts requires authoritative REST reconciliation and careful distinction between cumulative session volume and per-candle volume.

## Streaming chart price marker

Visible US Yahoo equity/ETF charts request quotes through the existing shared coordinator even when the watchlist sidebar is closed. The separate chart consumer uses a lease and releases on workspace drop; replay suspends demand and hides the marker. A recent regular-session quote (REST or streaming) is shown as a dashed indicative price line and label, without replacing the candle-close tag. No additional socket per chart. The marker is hidden on stale, invalid, older-than-last-bar, off-session or out-of-view quotes and on snapshots. It does not affect scaling, OHLCV, scripts, table feeds, MCP, alerts or backtests. The existing REST candle refresh remains authoritative. This is intentionally a view-only streaming increment rather than claiming that a sequence of indicative ticks is a complete per-candle trade feed.

The initial provisional candle overlay outlines the existing intraday REST candle using observed price extrema and the latest indicative price. It is display-only, never creates a new bar, carries no volume, clears on market/timeframe change or invalid/out-of-bucket observations, and stays hidden in replay and snapshots. REST updates continue independently. Full sampled candle rollover and precise reconciliation remain pending. See [implementation roadmap](yahoo-finance-roadmap.md) for the handoff and remaining tasks.

## Experimental US extended hours

Open Settings → Yahoo Finance → Extended hours. The saved preference applies without restart via a process-wide session revision; charts and comparison overlays reload the selected history, restoring fresh matching cache entries immediately when available. Cold or expired cache entries load through the normal worker. Pine controllers rebuild, Studio debug/profile runs invalidate, and the watchlist restores the last validated regular REST snapshot before consuming ticks under the new revision. The preference is loaded before chart/quote workers on startup. Live restart-free switching still requires owner validation. The previous `DEGENVIEW_YAHOO_EXTENDED_HOURS=1` (or 0) environment override remains available for diagnostics and is displayed when present. Default is off. US Yahoo equity/ETF intraday chart history requests use includePrePost=true and a separate candle cache key. Daily and longer, non-US and other instrument histories stay regular/unchanged. The chart watermark labels extended hours. The ticker accepts pre/post messages only when the vendor timestamp agrees with the normal US session window; omitted protobuf enum zero is accepted only during premarket. Unknown overnight/extended enum 3 is rejected. Extended tick volume and change reference are withheld rather than borrowed from a regular session. The shared watchlist quote can reflect extended prices in this opt-in process, labeled as potentially delayed; REST spark still returns regular-session snapshots. Charts show the indicative marker/preview only within the same session window and only on intraday extended charts outside regular hours. Pine session classification already understands standard pre/post windows; session statistics still use regular hours. Holiday and early-close calendars, native session-end print interpretation, extended reference prices and optional per-chart session selection remain to be validated.

To test outside regular hours, run `cargo test --lib live_yahoo_receives_extended_session_tick -- --ignored --nocapture` during US premarket/after-hours. The test explicitly accepts extended ticks regardless of the startup flag; it prints an actual received symbol/price/timestamp and times out if none arrives. It does not prove the provider will always stream outside regular hours.

## Deeper hourly history: design constraint

Yahoo's native 60m history can reach up to roughly 730 days, but US equity candles are ordinarily anchored to the regular session open (09:30 Eastern), whereas DegenView and Pine share UTC-hour buckets. The first native 60m candle can span two of DegenView's buckets. Neither relabeling that candle nor splicing old native hourly candles onto recent 15m folds preserves OHLC or volume. Keep the existing 60-day hourly history until a consistent session-anchored bucket model can be used by charts, older paging, Pine timeframe conversion, overlays, replay and the cache. If added, that model must use a distinct cache identity and real exchange timestamps, handle holidays and short sessions, and apply consistently to the whole series, not only older pages.

## Attribution

Endpoint selection and request approach are based on [Wickline](https://github.com/keptaduona/wickline), especially `server.mjs`, MIT licensed. The reference license is reproduced below.

```text
MIT License

Copyright (c) 2026 Wickline contributors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

### Live protocol finding (2026-11-25)
The owner established a successful premarket WebSocket handshake, subscription, and decoded NVDA/TSLA observations with millisecond timestamps, PRE enum 0, and absent currency and volume. The strict initial decoder rejected those sparse ticks. Accept an absent currency only for the restricted US ticker scope and require an independently validated REST quote in the engine. Explicit non-USD is still rejected. Keep the REST baseline separately from the stream; use a prior regular close in premarket, current previous close in regular hours, and a confirmed near-close regular price after hours. Extended volume is absent. The owner subsequently passed the premarket diagnostic at fc24fc6: SPY 704.49, timestamp 1795608368000, PRE enum 0, absent currency/volume, accepted in a 1.36-second test. This verifies socket delivery and parser admission, not publication through the quote engine or chart/watchlist UI. Regular-session and after-hours diagnostics, watchlist presentation and reconciliation still need retesting.
