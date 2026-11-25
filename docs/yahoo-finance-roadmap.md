# Yahoo Finance implementation roadmap

**Branch:** `feat/yahoo-finance` in [martinbalfanz/degenview](https://github.com/martinbalfanz/degenview).  
**Draft PR:** [#1](https://github.com/martinbalfanz/degenview/pull/1) against the fork's main. Do not merge or open an upstream PR without the owner's agreement.  
**Reference:** [Wickline server.mjs](https://github.com/martinbalfanz/wickline/blob/main/server.mjs), especially chart/search/spark and YahooStreamer. See [provider notes](yahoo-finance.md) for decisions, limitations and MIT attribution.

## Validated by fork owner

- [x] Initial native provider, chart/search/spark transport and source integration.
- [x] US stocks/ETFs, NVDA 5m and timeframe switches, AAPL older daily history, BRK-B, SPY/QQQ, watchlists, restart persistence.
- [x] US indexes and Xetra.
- [x] Additional international market profiles (London GBp normalization, European exchanges, Toronto).
- [x] Selected FX and rolling futures symbols.
- Last owner-confirmed working head: `191cfe140d666b12c51278a038fa9d0b81e83f98`. These are manual confirmations, not a claim that all cargo tests or edge cases passed.

## P0 — validate current branch before expanding streaming

- [ ] Compile current head, run cargo fmt --all -- --check, cargo check --locked, cargo test --lib, cargo clippy --all-targets --all-features -- -D warnings; follow upstream AGENTS.md and relevant guides. Fix errors before adding scope.
- [ ] In regular US hours: cargo test --lib live_yahoo_receives_regular_session_tick -- --ignored --nocapture.
- [ ] Test watchlist tick updates, quote age, REST fallback, reconnect, proxy changes, symbol changes, no timestamp regression, previous close, cumulative volume. Capture sanitized real protocol fixtures to test envelope variations.
- [ ] Test NVDA chart with sidebar closed, daily/5m switch, replay, historical pan, tab hide, no extra socket per chart, snapshot. Verify indicative quote and provisional candle overlay update separately from authoritative OHLCV; no Pine/VWAP changes, no persistence of previews.
- [ ] Confirm quote session/type field handling and ticker subscription behavior; keep REST when vendor data is absent. The current implementation is not certified real-time.
- [ ] Review REST/stream merge when REST is older, equal or untimestamped, at day boundaries and after reconnect; separate REST metadata confirmation from stream freshness if required.

## P1 — streaming candle reconciliation

- [x] US regular-session shared quote socket with bounded JSON/base64/protobuf decoder, reconnect, quote ordering, REST fallback (5c0c6e0).
- [x] View-only chart indicative quote line (cac82af).
- [x] Initial view-only intraday forming candle outline, restricted to the existing REST bucket. No invented volume, no authoritative mutation.
- [ ] Validate actual REST refresh watermark/observation timing; same-bucket corrections, split revisions, late ticks, duplicate timestamp semantics and session/day transitions. Avoid mixing adjustment bases; reset on corrections as necessary.
- [ ] Decide how to show a new provisional bucket before REST returns it without inventing opening trade, missing bars, or complete volume. Keep it explicitly provisional; preserve time axis/crosshair behavior and do not feed incomplete bars into Pine/backtests. Reconcile when REST covers the bucket. Test normal/short days, gaps, disconnections, timeframes and replay.
- [ ] Expand streaming to international stocks, indexes, FX and futures only after verified real payload metadata and session/currency validation (London pence, overnight dates); do not assume cumulative volume represents per-bar volume.
- [ ] Consider subscription churn, fairness over 100 symbols, and REST request coalescing. Respect proxy configuration and upstream rate limits.

## P2 — extended hours

- [ ] Explicit opt-in regular vs extended session semantics in source configuration, cache keys, chart/picker, watchlist reference and Pine. Validate pre/post session flags, OHLCV, corporate actions, holidays/early closes; do not mix regular and extended history silently.
- [ ] Stream session metadata and matching chart policy. Default regular session.

## P3 — deeper hourly history

- [ ] Design session-anchored native 60m history (roughly 730 days if available) vs UTC-grid existing 15m folds (60 days). Do not splice incompatible candles or relabel OHLC.
- [ ] Distinct cache identity, consistent bucket boundaries in chart/history paging/Pine/overlays/replay, short sessions, DST, overnight sessions. Validate against vendor and Wickline. Decide whether to keep UTC grid or introduce explicit session-anchored mode.
- [ ] Verify retention bounds, pagination, historical corrections and adjustment basis; preserve older loaded pages after revisions correctly.

## P4 — integration audit and upstream readiness

- [ ] Audit all provider switch statements and user-facing localization, picker categories, volume availability (index/FX raw Pine placeholders), chart precision and currency, session stats, paper trading and alerts (currently disabled), live broker eligibility, portfolio direct import/recents, MCP input/output, symbol identity and persistence. Do not claim futures strategy P&L/point value is correct.
- [ ] Assess dynamic instrument metadata and calendar support; non-local currency listings and unsupported instruments should fail safely. Consider additional markets only after core integration stable.
- [ ] Verify Yahoo unofficial endpoint availability, permitted use, rate limits, and upstream maintainer expectations. No new dependencies unless agreed; preserve licensing and attribution.
- [ ] Run full tests, formatting, clippy, CI on supported platforms; provide manual validation matrix and real fixtures. Review branch diff against current upstream, resolve conflicts, polish documentation and changelog; agree with owner on upstream scope and whether to split PR.

## Handoff procedure

Read this roadmap and provider notes, inspect PR #1 and current branch head, then select the first uncompleted task in priority order. Record exact commits, outcomes and remaining uncertainties here after each increment. Keep main untouched. The original implementation environment could access GitHub but not build Rust or reach live Yahoo; the owner runs local tests. Do not mark an item validated until evidence is available. If a review or local test reports an error, fix that before expanding scope.
