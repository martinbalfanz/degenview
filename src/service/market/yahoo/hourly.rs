//! Native US equity/ETF hourly candles. Vendor session metadata defines
//! the exact open and end of each observed bar.
use super::{Chart, ChartMeta, adapter::collect_sessions};
use crate::model::{DataSource, Kline};
use crate::model::market::yahoo_session::Session;
use crate::service::ProviderError;

const HOUR: i64 = 3_600;

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

/// Only the current vendor schedule is needed for a live preview. This small
/// cache does not perform I/O and is never populated by historical paging.
struct CurrentPeriods {
    requested_at: i64,
    received: Instant,
    periods: Vec<Period>,
}

static CURRENT: LazyLock<Mutex<HashMap<String, CurrentPeriods>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
const MAX_CURRENT: usize = 128;
const CURRENT_TTL: Duration = Duration::from_secs(15 * 60);

pub(super) fn remember_current(meta: &ChartMeta, requested_at: i64) {
    let mut current = Vec::new();
    if let Some(value) = &meta.current_trading_period {
        for (session, name) in [(Session::Pre, "pre"), (Session::Regular, "regular"), (Session::Post, "post")] {
            let mut ranges = Vec::new();
            if let Some(value) = value.get(name) {
                collect_sessions(value, &mut ranges);
            }
            ranges.sort_unstable();
            ranges.dedup();
            current.extend(ranges.into_iter().map(|(start, end)| Period { session, start, end }));
        }
    }
    let key = meta.symbol.to_ascii_uppercase();
    let mut held = crate::lock::lock(&CURRENT);
    if held.get(&key).is_some_and(|entry| entry.requested_at > requested_at) {
        return;
    }
    if current.is_empty() {
        held.remove(&key);
        return;
    }
    if held.len() >= MAX_CURRENT && !held.contains_key(&key)
        && let Some(oldest) = held.iter().min_by_key(|(_, entry)| entry.received).map(|(key, _)| key.clone())
    {
        held.remove(&oldest);
    }
    held.insert(key, CurrentPeriods { requested_at, received: Instant::now(), periods: current });
}

/// Exact open and exclusive end for the observed quote. Both the quote and
/// the current time must be in the same vendor period; no standard-hours
/// fallback, no stale schedule and no guessing the next exchange session.
pub(crate) fn current_hour_bucket(symbol: &str, timestamp: i64, now: i64, extended: bool) -> Option<(i64, i64)> {
    let held = CURRENT.try_lock().ok()?;
    let entry = held.get(&symbol.to_ascii_uppercase())?;
    if entry.received.elapsed() > CURRENT_TTL {
        return None;
    }
    observed_bucket(&entry.periods, timestamp, now, extended)
}

fn observed_bucket(periods: &[Period], timestamp: i64, now: i64, extended: bool) -> Option<(i64, i64)> {
    let period = period_at(periods, timestamp.div_euclid(1_000)).ok()??;
    if period_at(periods, now.div_euclid(1_000)).ok()?? != period
        || (!extended && period.session != Session::Regular)
        // The watchlist currently classifies by standard NY hours. Do not
        // use its price on a shortened day when those classifications disagree.
        || crate::model::market::yahoo_session::session_at(timestamp) != Some(period.session)
    {
        return None;
    }
    let start = period.start + (timestamp.div_euclid(1_000) - period.start).div_euclid(HOUR) * HOUR;
    Some((start * 1_000, (start + HOUR).min(period.end) * 1_000))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Period {
    session: Session,
    start: i64,
    end: i64,
}

fn periods(meta: &ChartMeta, extended: bool) -> Vec<Period> {
    let mut out = Vec::new();
    for (session, name) in [(Session::Pre, "pre"), (Session::Regular, "regular"), (Session::Post, "post")] {
        if !extended && session != Session::Regular {
            continue;
        }
        let mut ranges = Vec::new();
        if let Some(history) = &meta.trading_periods {
            if let Some(value) = history.get(name) {
                collect_sessions(value, &mut ranges);
            } else if session == Session::Regular && history.is_array() {
                collect_sessions(history, &mut ranges);
            }
        }
        if let Some(current) = meta.current_trading_period.as_ref().and_then(|value| value.get(name)) {
            collect_sessions(current, &mut ranges);
        }
        ranges.sort_unstable();
        ranges.dedup();
        out.extend(ranges.into_iter().map(|(start, end)| Period { session, start, end }));
    }
    out.sort_unstable_by_key(|period| period.start);
    out
}

fn period_at(periods: &[Period], timestamp: i64) -> Result<Option<Period>, ProviderError> {
    let mut matches = periods.iter().copied().filter(|period| (period.start..period.end).contains(&timestamp));
    let first = matches.next();
    if matches.next().is_some() {
        return Err(ProviderError::data(format!("Overlapping Yahoo hourly trading periods at {timestamp}")));
    }
    Ok(first)
}

/// Preserve the vendor's native open time and shorten its last bar to
/// the actual session end. Do not round, reaggregate, or invent volume.
/// Strictly require historical period metadata rather than guessing holidays
/// and early closes. If coverage is insufficient the live probe reports it.
pub(super) fn shape(
    chart: &Chart,
    start_ms: i64,
    end_ms: i64,
    extended: bool,
    include_forming: bool,
    max: usize,
) -> Result<Vec<Kline>, ProviderError> {
    if !DataSource::Yahoo.yahoo_stream_symbol(&chart.meta.symbol) || chart.meta.equity_price_scale() != Some(1.0) {
        return Err(ProviderError::data("Native Yahoo hourly history requires verified US equity/ETF metadata"));
    }
    let periods = periods(&chart.meta, extended);
    let mut out: Vec<Kline> = Vec::new();
    for (index, bar) in chart.bars.iter().enumerate() {
        let Some(open_time) = bar.timestamp.checked_mul(1_000) else {
            continue;
        };
        if !(start_ms..end_ms).contains(&open_time) {
            continue;
        }
        let period = match period_at(&periods, bar.timestamp)? {
            Some(period) => period,
            None => {
                // Yahoo can append an extra row exactly at regular close,
                // including on shortened days. It is not an hourly candle
                // in the requested regular session. Only the already supported
                // flat, explicitly zero-volume close print may correct the
                // adjacent final bar. Other boundary rows are ambiguous:
                // discard them without absorbing OHLC or volume.
                if !extended && periods.iter().any(|period| {
                    period.session == Session::Regular && period.end == bar.timestamp
                }) {
                    if bar.volume == Some(0.0) && bar.open == bar.close
                        && bar.high == bar.close && bar.low == bar.close
                        && let Some(last) = out.last_mut().filter(|last| last.close_time.checked_add(1) == Some(open_time))
                    {
                        last.high = last.high.max(bar.close);
                        last.low = last.low.min(bar.close);
                        last.close = bar.close;
                    }
                    continue;
                }
                return Err(ProviderError::data(format!("No Yahoo hourly trading period for {}", bar.timestamp)));
            }
        };
        if (bar.timestamp - period.start).rem_euclid(HOUR) != 0 {
            // Yahoo can append a current-price observation at a trade
            // timestamp rather than a candle's native open. Only a trailing
            // live row identified by regularMarketTime (or a recent flat,
            // zero/absent-volume quote in an extended session) qualifies.
            // Do not fold its ambiguous OHLCV into authoritative candles.
            let latest_regular = period.session == Session::Regular
                && chart.meta.regular_market_time == Some(bar.timestamp);
            let recent_extended_quote = extended && period.session != Session::Regular
                && end_ms.saturating_sub(open_time) <= 30 * 60_000
                && bar.open == bar.close && bar.high == bar.close && bar.low == bar.close
                && bar.volume.is_none_or(|volume| volume == 0.0);
            if include_forming && index + 1 == chart.bars.len()
                && (latest_regular || recent_extended_quote)
            {
                continue;
            }
            return Err(ProviderError::data(format!(
                "Unexpected Yahoo hourly anchor at {} (session={:?}, period={}..{}, last_row={}, regularMarketTime={:?}, OHLCV={:?})",
                bar.timestamp, period.session, period.start, period.end,
                index + 1 == chart.bars.len(), chart.meta.regular_market_time, bar,
            )));
        }
        let Some(volume) = bar.volume else {
            return Err(ProviderError::data(format!("Missing native hourly volume field at {}", bar.timestamp)));
        };
        let end = (bar.timestamp + HOUR).min(period.end) * 1_000;
        if !include_forming && end > end_ms {
            continue;
        }
        out.push(Kline {
            open_time,
            close_time: end - 1,
            open: bar.open,
            high: bar.high,
            low: bar.low,
            close: bar.close,
            volume,
        });
    }
    let skip = out.len().saturating_sub(max);
    out.drain(..skip);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::market::yahoo::Bar;

    fn fixture() -> Chart {
        let start = 1_800;
        let bar = |timestamp, volume| Bar {
            timestamp, open: 100.0, high: 105.0, low: 99.0, close: 103.0, volume: Some(volume),
        };
        Chart {
            meta: ChartMeta {
                symbol: "NVDA".into(),
                currency: Some("USD".into()),
                exchange_timezone_name: Some("America/New_York".into()),
                instrument_type: Some("EQUITY".into()),
                trading_periods: Some(serde_json::json!({
                    "pre": [[{"start": 0, "end": start}]],
                    "regular": [[{"start": start, "end": 7200}]],
                    "post": [[{"start": 7200, "end": 10800}]]
                })),
                ..Default::default()
            },
            bars: vec![bar(0, 0.0), bar(start, 400.0), bar(5400, 200.0), bar(7200, 0.0)],
        }
    }

    #[test]
    fn yahoo_native_hourly_observation_uses_current_vendor_period_and_short_end() {
        let at = |h, m| crate::pine::time::calendar::make(2026, 11, 25, h, m, 0, 0).unwrap();
        let period = Period { session: Session::Regular, start: at(14, 30) / 1_000, end: at(21, 0) / 1_000 };
        let post = Period { session: Session::Post, start: at(21, 0) / 1_000, end: at(23, 0) / 1_000 };
        let periods = [period, post];
        assert_eq!(observed_bucket(&periods, at(20, 45), at(20, 46), false), Some((at(20, 30), at(21, 0))));
        assert_eq!(observed_bucket(&periods, at(21, 5), at(21, 6), true), Some((at(21, 0), at(22, 0))));
        assert!(observed_bucket(&periods, at(21, 5), at(21, 6), false).is_none());
        assert!(observed_bucket(&periods, at(20, 59), at(21, 0), true).is_none());
        assert!(observed_bucket(&periods, at(23, 1), at(23, 2), true).is_none());
        let shortened = [Period { end: at(18, 0) / 1_000, ..period }];
        assert_eq!(observed_bucket(&shortened, at(17, 45), at(17, 46), false), Some((at(17, 30), at(18, 0))));
        assert!(observed_bucket(&shortened, at(18, 5), at(18, 6), true).is_none());
    }

    #[test]
    fn yahoo_native_hourly_shape_preserves_anchors_and_shortens_session_ends() {
        let chart = fixture();
        let bars = shape(&chart, 0, 10_800_000, true, false, 100).unwrap();
        assert_eq!(bars.iter().map(|bar| (bar.open_time, bar.close_time, bar.volume)).collect::<Vec<_>>(),
            [(0, 1_799_999, 0.0), (1_800_000, 5_399_999, 400.0), (5_400_000, 7_199_999, 200.0), (7_200_000, 10_799_999, 0.0)]);
        assert_eq!((bars[1].open, bars[1].high, bars[1].low, bars[1].close), (100.0, 105.0, 99.0, 103.0));
        let page = shape(&chart, 1_800_000, 7_200_000, true, false, 1).unwrap();
        assert_eq!(page, bars[2..3]);
        assert_eq!(shape(&chart, 1_800_000, 6_000_000, true, false, 100).unwrap(), bars[1..2]);
        assert_eq!(shape(&chart, 1_800_000, 6_000_000, true, true, 100).unwrap(), bars[1..3]);
    }

    #[test]
    fn yahoo_native_hourly_ignores_only_identifiable_trailing_live_observations() {
        let mut chart = fixture();
        chart.bars.pop();
        chart.bars.remove(0);
        let expected = shape(&chart, 0, 6_800_000, false, true, 100).unwrap();
        let observation = Bar {
            timestamp: 6_744, open: 115.0, high: 116.0, low: 114.0,
            close: 115.5, volume: Some(500.0),
        };
        chart.meta.regular_market_time = Some(observation.timestamp);
        chart.bars.push(observation.clone());
        // Ignore the latest print entirely: no synthetic OHLC, volume, or
        // new anchor, even when its payload looks like a candle.
        assert_eq!(shape(&chart, 0, 6_800_000, false, true, 100).unwrap(), expected);
        assert_eq!(shape(&chart, 0, 6_800_000, true, true, 100).unwrap(), expected);
        assert!(shape(&chart, 0, 7_200_000, false, false, 100).is_err());
        chart.meta.regular_market_time = None;
        assert!(shape(&chart, 0, 6_800_000, false, true, 100).is_err());
        chart.meta.regular_market_time = Some(observation.timestamp);
        chart.bars.push(Bar { timestamp: 7200, ..observation });
        assert!(shape(&chart, 0, 8_000_000, true, true, 100).is_err());
    }

    #[test]
    fn yahoo_native_hourly_ignores_recent_flat_extended_quote_but_not_unknown_ohlcv() {
        let mut chart = fixture();
        let expected = shape(&chart, 0, 8_000_000, true, true, 100).unwrap();
        chart.bars.push(Bar { timestamp: 7900, open: 115.0, high: 115.0, low: 115.0, close: 115.0, volume: None });
        assert_eq!(shape(&chart, 0, 8_000_000, true, true, 100).unwrap(), expected);
        assert!(shape(&chart, 0, 10_000_000, true, true, 100).is_err());
        assert!(shape(&chart, 0, 8_000_000, true, false, 100).is_err());
        chart.bars.last_mut().unwrap().volume = Some(10.0);
        assert!(shape(&chart, 0, 8_000_000, true, true, 100).is_err());
    }

    #[test]
    fn yahoo_native_hourly_shape_requires_metadata_and_rejects_relabeling() {
        let mut chart = fixture();
        chart.bars[1].timestamp = 3600;
        assert!(shape(&chart, 0, 10_800_000, true, false, 100).is_err());
        let mut chart = fixture();
        chart.meta.trading_periods = None;
        assert!(shape(&chart, 0, 10_800_000, true, false, 100).is_err());
        let mut chart = fixture();
        chart.bars[1].volume = None;
        assert!(shape(&chart, 0, 10_800_000, true, false, 100).is_err());
    }

    #[test]
    fn yahoo_native_hourly_shape_ignores_ambiguous_close_boundary_rows_only_in_regular_mode() {
        let chart = fixture();
        // The final row has a range and is timestamped at the regular close.
        // In regular mode it is not a valid additional candle, even if Yahoo
        // supplied volume; in extended mode it is the first postmarket bar.
        for volume in [Some(0.0), Some(500.0), None] {
            let mut chart = chart.clone();
            chart.bars.remove(0);
            chart.bars.last_mut().unwrap().volume = volume;
            let regular = shape(&chart, 0, 10_800_000, false, false, 100).unwrap();
            assert_eq!(regular.len(), 2);
            assert_eq!((regular[1].close_time, regular[1].close, regular[1].volume), (7_199_999, 103.0, 200.0));
            if let Some(volume) = volume {
                let extended = shape(&chart, 0, 10_800_000, true, false, 100).unwrap();
                assert_eq!(extended.len(), 3);
                assert_eq!((extended[2].open_time, extended[2].volume), (7_200_000, volume));
            }
        }
        // An unknown timestamp elsewhere is still rejected: do not turn a
        // general metadata failure into silently missing history.
        let mut unknown = chart;
        unknown.bars.remove(0);
        unknown.bars.last_mut().unwrap().timestamp += 60;
        assert!(shape(&unknown, 0, 10_800_000, false, false, 100).is_err());
    }

    #[test]
    fn yahoo_native_hourly_shape_handles_regular_only_array_and_closing_print() {
        let mut chart = fixture();
        chart.meta.trading_periods = Some(serde_json::json!([[{"start": 1800, "end": 7200}]]));
        chart.bars.remove(0);
        let print = chart.bars.last_mut().unwrap();
        print.open = 104.0;
        print.high = 104.0;
        print.low = 104.0;
        print.close = 104.0;
        let bars = shape(&chart, 0, 10_800_000, false, false, 100).unwrap();
        assert_eq!(bars.len(), 2);
        assert_eq!((bars[1].close_time, bars[1].close, bars[1].volume), (7_199_999, 104.0, 200.0));
    }
}
