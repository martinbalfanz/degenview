# Yahoo Finance provider (development branch)

This branch begins with a native, keyless Yahoo client in `service::market::yahoo`. It follows Wickline's chart, search and spark endpoints and two-host transport. It uses the app's existing proxy-aware, bounded HTTP client; it does not require Node, an API key or new dependencies.

The client deliberately keeps original UTC timestamps, optional volume, exchange timezone, currency and quote timestamps. It does not use a present-day UTC offset to reconstruct historical sessions, substitute zero for absent volume, fabricate candles or silently retry HTTP 429 through another hostname. The exchange's native data may be delayed. These endpoints are unofficial and their availability, limits and usage terms need verification before release.

## Remaining integration work

The draft registers Yahoo for US equities and ETFs, symbol search, historical charts and REST watchlist quotes. It folds longer candles from native 1/5/15-minute and daily bars on DegenView's UTC grid (not Wickline's session-open grid). Quotes are explicitly not certified live. Paper trading and price alerts are disabled for Yahoo. Before proposing upstream:
- Validate interval planning, older-history bounds, UTC folds versus session-anchored candles and official close prints against live data. Verify adjustment basis with split and dividend fixtures.
- Verify picker, quote freshness, caching, source mark, persistence and translations on supported platforms. International markets require instrument-level currency, asset type and session metadata.
- Finish auditing Pine alerts, replay, session statistics, portfolio, source-specific assumptions and MCP. Yahoo is not yet exposed as an MCP input provider.
- Add streaming only after validating the vendor's protobuf feed, disconnect behavior, exchange delays and correct cumulative volume handling. Keep REST polling fallback.
- Run cargo fmt, check, test and clippy and manually verify AAPL, BRK-B, BAS.DE, ^GSPC, EURUSD=X, GC=F, holidays, daylight-saving transitions, range limits and network failures.

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
