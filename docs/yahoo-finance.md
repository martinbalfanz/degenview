# Yahoo Finance provider (development branch)

This branch begins with a native, keyless Yahoo client in `service::market::yahoo`. It follows Wickline's chart, search and spark endpoints and two-host transport. It uses the app's existing proxy-aware, bounded HTTP client; it does not require Node, an API key or new dependencies.

The client deliberately keeps original UTC timestamps, optional volume, exchange timezone, currency and quote timestamps. It does not use a present-day UTC offset to reconstruct historical sessions, substitute zero for absent volume, fabricate candles or silently retry HTTP 429 through another hostname. The exchange's native data may be delayed. These endpoints are unofficial and their availability, limits and usage terms need verification before release.

## Remaining integration work

The draft registers Yahoo for US equities, ETFs and selected indexes plus nine international exchange profiles, symbol search, historical charts and REST watchlist quotes. It folds longer candles from native 1/5/15-minute and daily bars on DegenView's UTC grid (not Wickline's session-open grid). Successful quote refreshes are marked 'May be delayed' (never certified live), and become stale on failed or expired refreshes. A shared 30-second cooldown follows HTTP 429. Historical ranges exclude observations at or beyond the exclusive end, and closing prints cannot update an earlier candle across a gap. The cache refetches the whole series when a refreshed tail revises a completed Yahoo candle's prices; this reduces the risk of mixed adjustment bases after corporate actions. Paper trading and price alerts are disabled for Yahoo. Before proposing upstream:
- Validate interval planning, older-history bounds (1m: 7 days; 5m/15m: 60 days by conservative policy, including folded hourly charts), UTC folds versus session-anchored candles and official close prints against live data. Yahoo's ordinary chart OHLC (not dividend-adjusted adjclose) is used, like Wickline. Verify split adjustment and dividend behavior against live fixtures.
- Verify picker, quote freshness, caching, source mark, persistence and translations on supported platforms. International markets require instrument-level currency, asset type and session metadata.
- Finish auditing Pine alerts, replay, session statistics, portfolio, source-specific assumptions and MCP. Yahoo is not yet exposed as an MCP input provider.
- Add streaming only after validating the vendor's protobuf feed, disconnect behavior, exchange delays and correct cumulative volume handling. Keep REST polling fallback.
- The fork owner confirmed the initial integration and the subsequent manual checklist (NVDA timeframe changes, AAPL daily history, BRK-B, ETFs, watchlist quotes and persistence). The fork owner also confirmed the US index implementation works. The fork owner subsequently confirmed the Xetra increment works. The additional international profiles still need compilation and manual testing. Run cargo fmt, test and clippy on subsequent changes; manually verify AAPL, BRK-B, holidays, daylight-saving transitions, range limits and network failures. Unlisted international exchanges, forex and futures remain out of scope.

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
