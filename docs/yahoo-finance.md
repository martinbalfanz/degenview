# Yahoo Finance provider (development branch)

This branch begins with a native, keyless Yahoo client in `service::market::yahoo`. It follows Wickline's chart, search and spark endpoints and two-host transport. It uses the app's existing proxy-aware, bounded HTTP client; it does not require Node, an API key or new dependencies.

The client deliberately keeps original UTC timestamps, optional volume, exchange timezone, currency and quote timestamps. It does not use a present-day UTC offset to reconstruct historical sessions, substitute zero for absent volume, fabricate candles or silently retry HTTP 429 through another hostname. The exchange's native data may be delayed. These endpoints are unofficial and their availability, limits and usage terms need verification before release.

## Remaining integration work

The client is not yet a selectable market source. Before proposing upstream:
- Register Yahoo in DataSource and MarketSource; implement supported interval planning, older-history bounds and session-aware OHLC aggregation, including the shortened session-end candle and Yahoo's appended close print. Decide on the adjustment basis and verify it with split and dividend fixtures.
- Wire symbol picker, watchlist quotes and caches, preserving exact vendor symbols, exchange/currency metadata and quote timestamps. Never present stale or delayed quotes as live. Add a source mark/attribution and translations following the repository guides.
- Audit all stock-specific logic in Pine, replay, alerts, session statistics, paper trading, portfolio and MCP; disable unsupported capabilities rather than treating non-US markets as US equities or USD instruments.
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
