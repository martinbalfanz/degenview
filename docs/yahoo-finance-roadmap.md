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
- Owner compiled 6833a65 and verified NVDA 1m/5m chart updates. Regular live test timed out; no claim of proven WebSocket delivery. The warning for unused native interval variants is addressed by removing the unused variants.
- [x] Owner compiled `fc24fc6646611bc68dc3f141bc15642f97957279` and passed `live_yahoo_receives_extended_session_tick` on 2026-11-25 in 1.36 s. Accepted SPY at 704.49, vendor timestamp 1795608368000, PRE enum 0, absent currency and volume. No compiler warnings in supplied output.
- Last owner-confirmed symbol coverage head: `191cfe140d666b12c51278a038fa9d0b81e83f98`. Last owner-confirmed premarket socket diagnostic head: `fc24fc6646611bc68dc3f141bc15642f97957279`. These are limited validation results, not a claim that all cargo tests or edge cases passed.

## P0 — validate current branch before expanding streaming

- [ ] Compile current head, run cargo fmt --all -- --check, cargo check --locked, cargo test --lib, cargo clippy --all-targets --all-features -- -D warnings; follow upstream AGENTS.md and relevant guides. Fix errors before adding scope.
- [x] Premarket WebSocket handshake, subscription, decoding and sparse-tick admission verified at fc24fc6. The previous regular-session diagnostic at 6833a65 timed out; retest using the revised diagnostic.
- [ ] Regular-session live verification: cargo test --lib live_yahoo_receives_regular_session_tick -- --ignored --nocapture. After-hours delivery separately unverified.
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

- [x] Experimental startup opt-in for US stock/ETF intraday history, isolated cache and chart watermark; extended stream admission and explicit extended live test.
- [x] Persist source-wide extended-hours preference in Settings → Yahoo Finance; revision-aware restart-free switching implemented, live validation pending. Environment flag retained as explicit diagnostic override.
- [ ] Review quote reference and Pine/session stats; evaluate per-chart setting or safe immediate reload after validation. Validate pre/post session flags, OHLCV, corporate actions, holidays/early closes; do not mix regular and extended history silently.
- [x] Premarket live transport and decoder acceptance verified.
- [ ] Validate after-hours delivery, full coordinator/watchlist publication, regular/extended session boundaries, cache isolation, closing prints, quote reference and volume semantics. Default regular session.

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

### 2026-11-25 owner feedback
Settings persisted across restart. NVDA and TSLA premarket socket frames decoded successfully with missing currency and volume; strict currency requirement caused timeout. This increment accepts sparse USD-scope ticks after REST metadata validation, maintains a separate REST baseline for watchlist change/volume, and restyles secondary text. Premarket diagnostic subsequently passed at fc24fc6; validate the regular-session and after-hours tests and watchlist in the app. The restart requirement was questioned: remove only after capturing session policy per request/cache key, invalidating in-flight chart and overlay requests and Pine security caches, clearing stale extended watchlist quotes, and auditing background consumers. Do not mix histories or silently change script inputs.

### Restart-free switching implementation order
1. Capture an immutable session-policy snapshot for each fetch before deriving its cache key, including retries, history paging, and Pine security requests. Keep regular and extended caches isolated.
2. Introduce a session-policy revision, discard old in-flight results, and reload affected chart and overlay histories; invalidate Pine security caches and active script runs that depend on Yahoo. Audit hidden/background consumers and replay.
3. On disabling extended hours, restore the validated regular-session watchlist baseline and reject queued extended ticks. Repaint affected windows and preserve the saved preference. Test rapid toggles, in-flight requests, and reverting to regular mode before removing the restart notice.

### Restart-free switching: request-policy groundwork
The candle service captures effective session mode once, passes it to the memory cache and Yahoo transport/response shaping, and uses the same value for tail and full-fetch fallback. Chart fetches and older pages capture before entering the work queue. Pine security cache keys include effective mode and retries/fallback use the captured value. Added regression coverage for independent cache series and session-dependent closing-print shaping. This is groundwork only: startup policy remains immutable; overlay queue capture, active Pine run invalidation, chart/overlay reload, quote transitions, replay audit, and compilation/tests remain before enabling the toggle. No live validation claimed.

### Owner validation and overlay session groundwork
Owner reported successful `cargo check --locked` and `cargo test --lib yahoo` for dcd289f: 37 passed, 0 failed, 2 network-dependent tests ignored; 8,713 filtered out. No warnings in supplied output. This is not a full-suite or formatting/Clippy result.
Overlay requests now capture effective session mode before queueing both newest and older pages, and a session change resets only affected intraday Yahoo overlay series before polling answers. Queued jobs are invalidated by generation. Added a deterministic rapid-toggle regression test with a fake provider. The live settings toggle is still disabled pending main-chart, Pine-run and quote invalidation. Overlay changes require local compilation/test validation.

### Overlay validation follow-up
Owner reported that f421c7d passed `cargo check --locked` with warnings but failed to compile library tests: the `Immediate` test backend in `workspace/overlay/actions.rs` had not been migrated to the new `OverlayBackend` signature (E0050). Updated both methods, removed the unused `fetch_klines_live` wrapper and unimplemented native Yahoo `Interval::Hour` (hourly history remains folded; P3 remains pending). Updated the URL test to use a supported native interval. Manifest warnings for `tar` and `zip` are separate from this provider change and were left alone. Repeat check and targeted tests before advancing.

### Main chart session groundwork
Owner validated 099ccd9: `cargo check --locked` passed (only manifest tar/zip warnings), Yahoo tests 38 passed/2 ignored, overlay tests 43 passed/1 ignored. The Yahoo rapid-toggle overlay test passed in both runs. Main charts now track their effective session mode and invalidate both newest and older history requests before draining results on a change; clear stale live/paper state and rebase replay/link state while retaining chart configuration and viewport. New tests cover abandoned channels, repeat/reverse transitions and unaffected instruments/timeframes. This is not yet the instant toggle: warm-cache restoration, session revisions across hidden/background workspaces, Pine and quotes remain pending. New main-chart changes require local validation.

### Warm-cache session restoration
Owner reports all requested checks passed for e1d0128, with one ignored test noted (no exact per-suite counts supplied). Added nonblocking read of fresh, sufficient Yahoo intraday history by exact session key, using the existing 30-second freshness policy; no provider flight or stale/cross-session fallback on the UI thread. Main charts populate before queuing their normal worker and do not synthesize a paper quote timestamp. Visible overlays restore the selected mode before scheduling, including when worker slots are busy. Added cache, main-chart and overlay warm-toggle regression tests. Cold or expired cache still loads asynchronously; longer-lived display-only snapshots, optional prewarming and full session revision/Pine/quote coordination remain to be considered before enabling the setting. No local compile/test result yet for this increment.

Owner reported e7f8835 passed `cargo check --locked` (unchanged manifest warnings), but test compilation failed with E0308 in two warm-cache assertions: pass `LONG.fresh` (Duration), not `LONG` (Policy). Corrected both assertions; production behavior unchanged. Targeted tests still need rerunning.

### Revision-aware session identity
Owner reports all targeted tests green for 8f8014f. Added atomic session snapshots (mode and revision in one word), effective only for eligible Yahoo intraday history. Main charts and overlays now detect even a regular→extended→regular transition missed while inactive, invalidate old work and restore the matching warm cache. Tests exercise no-op updates, revision round trips and unaffected markets without mutating global state. The transition writer remains test-only until active Pine runs, background consumers and quote transitions are coordinated; the setting still requires restart. New revision tests and compilation require owner validation.

### Pine controller session invalidation
Owner validated ec1fec7: check passed, Yahoo 47 passed/2 ignored, overlays 45 passed/1 ignored, charts 221 passed/1 ignored. Replaced deprecated test-only atomic fetch_update with compare_exchange_weak (compatible with the repository's Rust 1.95 minimum). Applied chart script workers and alert controllers capture one session snapshot, rebuild on revision changes (including scripts on other markets that may request Yahoo), drop old outputs, and replay alert history silently. Foreign-security cache keys use effective policy revision; expired entries are pruned. Workspace alert feeds skip stale Yahoo chart history. One-shot checked runs capture a policy; the original dynamic provider remains for other callers pending studio/debugger/profiler and background-consumer audit. Regression tests cover chart worker epoch changes and silent subscription rebuilds. Runtime toggle still disabled; compilation and test validation pending.

### Watchlist transition groundwork
Owner reports all targeted checks green at d60caee (counts not supplied). Quote engine now preserves the regular REST receipt time separately, restores that baseline on session revision changes, rejects extended ticks in regular mode at consumption, and requests a new REST round without cancelling backoff. The coordinator uses a separate Yahoo mailbox replaced with the socket on revision changes so a rapid round trip cannot drain a previous mailbox. Book reads hide affected quotes until the new revision's baseline is published. Added a deterministic restoration/queued extended tick test. Settings toggle not enabled in this increment; studio/background audit and local validation remain.

### Restart-free switching enabled (implementation; validation pending)
The setting saves first and publishes a revision only on success; environment override and default regular mode remain intact. Root logic synchronizes open chart tabs before alert/MCP consumers and repaints windows; hidden Yahoo charts can consume their pending history without opening the tab. Studio preview drops its old history and queued result, applied outputs are invalidated even for other markets, debugger sessions end without losing breakpoints, and active profiler runs are discarded and re-created. Debugger/profiler providers pin one policy; preview identities include the revision. Watchlist transition uses the regular REST baseline, original freshness and isolated mailbox as above. UI no longer asks for restart. First load/expired cache still waits for REST; fresh cache is immediate. Added Studio revision test. Validate both modes in live UI, toggling rapidly in pre/post/regular hours, with inactive chart tabs, open debugger/profiler, overlays and alerts, then restart to confirm persisted preference. Verify baseline timestamp, rejection of old ticks, and no false alerts. This completes the implemented P2 runtime toggle, not its live validation. Do not start P1/P3 expansion until owner validates the increment; the remaining stream watermark/new-bucket and hourly anchoring tasks retain their scope.

Final safety pass: debugger output/logs are discarded with a restart-debugging notice while retaining breakpoints and watches; alert dispatch drops events if the policy changed during evaluation. No user or CI test result yet for this set of commits.
