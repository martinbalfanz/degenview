//! View-only preview of the tail of a candle. Yahoo's ticker is sampled,
//! not a full trade feed. REST remains authoritative, including volume.
use crate::model::market::bucket::{bucket_start_ms, next_bucket_start_ms};
use crate::model::{Kline, Timeframe};

#[derive(Clone, Copy, Debug)]
pub(super) struct Observation {
    pub price: f64,
    pub timestamp_ms: i64,
}

use crate::model::market::yahoo_preview::Preview;

/// One bucket of observed prices, independent of the REST candle.
#[derive(Default)]
pub(in crate::workspace) struct Provisional {
    sampled: Option<(Timeframe, Preview, i64)>,
    rest: Option<(Timeframe, Kline)>,
    observed_through: Option<i64>,
    /// Conservative same-bucket cutoff, not a vendor trade watermark.
    rest_floor: Option<(i64, i64)>,
}

impl Provisional {
    pub fn clear(&mut self) {
        self.sampled = None;
        self.rest = None;
        self.observed_through = None;
        self.rest_floor = None;
    }

    /// A revision to completed history may have changed its price scale.
    /// Reject previously observed samples and, conservatively, observations
    /// no newer than the request that exposed the change.
    pub(in crate::workspace) fn discard_samples(&mut self, request_started_ms: Option<i64>) {
        self.sampled = None;
        if let Some(started) = request_started_ms {
            self.observed_through = Some(self.observed_through.map_or(started, |seen| seen.max(started)));
        }
    }

    fn sync_rest(&mut self, tf: Timeframe, last: &Kline) -> bool {
        // The provider has no true trade watermark. A changed REST bar is an
        // authoritative correction, so discard old sampled extrema and do not
        // reapply the last seen quote. Unchanged cached bars are not corrections.
        if self.rest.as_ref().is_none_or(|(old_tf, old)| *old_tf != tf || old != last) {
            if self.rest.as_ref().is_some_and(|(old_tf, old)| *old_tf == tf && old.open_time <= last.open_time) {
                // A correction to the preceding bar does not invalidate samples
                // of a still-unconfirmed next bucket. Once REST reaches that
                // bucket, discard its samples but keep the duplicate guard.
                if self.sampled.is_some_and(|(_, sample, _)| sample.open_time <= last.open_time) {
                    self.sampled = None;
                }
            } else {
                self.clear();
            }
            self.rest = Some((tf, last.clone()));
            self.rest_floor = None;
            return true;
        }
        false
    }

    /// A changed authoritative candle suppresses observations at or before
    /// the originating request start, including delayed packets not seen yet.
    /// Do not advance on an unchanged response, even if it was re-fetched.
    pub(in crate::workspace) fn reconcile_rest(&mut self, tf: Timeframe, last: &Kline, request_started_ms: i64) {
        if self.sync_rest(tf, last) || self.rest_floor.is_none() {
            self.rest_floor = Some((last.open_time, request_started_ms));
            if self.sampled.is_some_and(|(_, sample, ts)| sample.open_time == last.open_time && ts <= request_started_ms) {
                self.sampled = None;
            }
        }
    }

    #[cfg(test)]
    fn observe(&mut self, tf: Timeframe, last: &Kline, quote: Option<Observation>) -> Option<Preview> {
        self.observe_with_grid(tf, last, quote, false, None)
    }

    pub(super) fn observe_with_grid(&mut self, tf: Timeframe, last: &Kline, quote: Option<Observation>, native_hourly: bool, native_bucket: Option<(i64, i64)>) -> Option<Preview> {
        self.sync_rest(tf, last);
        let Some(quote) = quote else {
            self.sampled = None;
            return None;
        };
        // A native future slot requires the provider's current period.
        // The existing REST bar is already authoritative about its bounds.
        let (bucket, end_time) = if native_hourly {
            if (last.open_time..=last.close_time).contains(&quote.timestamp_ms) {
                (last.open_time, last.close_time.saturating_add(1))
            } else if let Some((start, end)) = native_bucket.filter(|(start, end)| *start <= quote.timestamp_ms && quote.timestamp_ms < *end) {
                (start, end)
            } else {
                // A busy or expired schedule cache hides the preview, but
                // does not erase a sample that may still be valid once the
                // identical period is confirmed again.
                return None;
            }
        } else {
            let start = bucket_start_ms(tf, quote.timestamp_ms);
            (start, next_bucket_start_ms(tf, start))
        };
        let next = if native_hourly { last.close_time.saturating_add(1) } else { next_bucket_start_ms(tf, last.open_time) };
        // The caller has already checked the quote's current trading session.
        // One observed newer bucket may follow a gap, but no intervening bars
        // are invented and a late observation cannot rewind the preview.
        let newer = bucket >= next && bucket > last.close_time;
        if !tf.is_intraday() || !quote.price.is_finite() || quote.price <= 0.0
            || (bucket != last.open_time && !newer)
        {
            self.sampled = None;
            return None;
        }
        if self.rest_floor.is_some_and(|(open, cutoff)| bucket == open && quote.timestamp_ms <= cutoff) {
            return None;
        }
        if self.observed_through.is_some_and(|ts| quote.timestamp_ms <= ts) {
            // An identical book observation can redraw its existing preview,
            // but cannot revive samples discarded by a REST correction.
            if self.sampled.is_none_or(|(_, sample, ts)| ts != quote.timestamp_ms || sample.close != quote.price) {
                return None;
            }
        }
        let mut sampled = match self.sampled {
            Some((old_tf, sample, ts)) if old_tf == tf && sample.open_time == bucket && sample.end_time == end_time => {
                if quote.timestamp_ms < ts {
                    return None;
                }
                sample
            }
            _ => Preview { open_time: bucket, end_time, open: quote.price, high: quote.price, low: quote.price, close: quote.price },
        };
        sampled.high = sampled.high.max(quote.price);
        sampled.low = sampled.low.min(quote.price);
        sampled.close = quote.price;
        self.observed_through = Some(quote.timestamp_ms);
        self.sampled = Some((tf, sampled, quote.timestamp_ms));
        if bucket != last.open_time {
            // Open is the first observed price, not the official opening trade.
            return Some(sampled);
        }
        Some(Preview {
            open_time: last.open_time,
            end_time: last.close_time.saturating_add(1),
            open: last.open,
            high: last.high.max(sampled.high),
            low: last.low.min(sampled.low),
            close: sampled.close,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn bar(start: i64) -> Kline {
        Kline { open_time: start, close_time: start + 299_999, open: 100.0, high: 102.0, low: 98.0, close: 101.0, volume: 1234.0 }
    }

    #[test]
    fn yahoo_rest_request_floor_rejects_unseen_delayed_ticks_without_advancing_on_cache_hits() {
        let mut state = Provisional::default();
        let tf = Timeframe::parse("5m").unwrap();
        let mut rest = bar(0);
        let observe = |price, timestamp_ms| Some(Observation { price, timestamp_ms });
        state.reconcile_rest(tf, &rest, 1000);
        assert!(state.observe(tf, &rest, observe(200.0, 900)).is_none());
        assert!(state.observe(tf, &rest, observe(103.0, 1100)).is_some());
        state.reconcile_rest(tf, &rest, 2000);
        assert!(state.observe(tf, &rest, observe(104.0, 1200)).is_some());
        rest.close = 100.0;
        state.reconcile_rest(tf, &rest, 3000);
        assert!(state.observe(tf, &rest, observe(300.0, 2900)).is_none());
        assert_eq!(state.observe(tf, &rest, observe(101.0, 3100)).unwrap().high, 102.0);
        // A changed preceding candle's newer floor never blocks the independent next bucket.
        rest.volume += 1.0;
        state.reconcile_rest(tf, &rest, 301_000);
        assert_eq!(state.observe(tf, &rest, observe(107.0, 300_100)).unwrap().open, 107.0);
    }

    #[test]
    fn yahoo_next_bucket_uses_observations_only_and_yields_to_rest() {
        let mut state = Provisional::default();
        let tf = Timeframe::parse("5m").unwrap();
        let mut rest = bar(0);
        let observe = |price, timestamp_ms| Some(Observation { price, timestamp_ms });
        let first = state.observe(tf, &rest, observe(110.0, 301_000)).unwrap();
        assert_eq!((first.open, first.high, first.low, first.close), (110.0, 110.0, 110.0, 110.0));
        rest.close = 100.0;
        let second = state.observe(tf, &rest, observe(108.0, 302_000)).unwrap();
        assert_eq!((second.open, second.high, second.low, second.close), (110.0, 110.0, 108.0, 108.0));
        assert!(state.observe(tf, &bar(300_000), observe(108.0, 302_000)).is_none());
        let newer = state.observe(tf, &bar(300_000), observe(103.0, 303_000)).unwrap();
        assert_eq!((newer.open, newer.high, newer.low, newer.close), (100.0, 103.0, 98.0, 103.0));
        let gap = state.observe(tf, &bar(300_000), observe(200.0, 900_000)).unwrap();
        assert_eq!((gap.open_time, gap.open, gap.high, gap.low), (900_000, 200.0, 200.0, 200.0));
        assert!(state.observe(tf, &bar(300_000), observe(100.0, 600_000)).is_none());
    }

    #[test]
    fn yahoo_native_hourly_future_preview_requires_metadata_and_yields_to_rest() {
        let mut state = Provisional::default();
        let mut rest = bar(1_800_000);
        rest.close_time = 5_399_999;
        let quote = |price, timestamp_ms| Some(Observation { price, timestamp_ms });
        assert!(state.observe_with_grid(Timeframe::HOUR_1, &rest, quote(110.0, 5_400_001), true, None).is_none());
        let bounds = Some((5_400_000, 7_200_000));
        let first = state.observe_with_grid(Timeframe::HOUR_1, &rest, quote(110.0, 5_400_001), true, bounds).unwrap();
        assert_eq!((first.open_time, first.end_time, first.open), (5_400_000, 7_200_000, 110.0));
        assert!(state.observe_with_grid(Timeframe::HOUR_1, &rest, quote(110.0, 5_400_001), true, None).is_none());
        let second = state.observe_with_grid(Timeframe::HOUR_1, &rest, quote(108.0, 5_400_002), true, bounds).unwrap();
        assert_eq!((second.open, second.high, second.low, second.close), (110.0, 110.0, 108.0, 108.0));
        let mut next = bar(5_400_000);
        next.close_time = 7_199_999;
        assert!(state.observe_with_grid(Timeframe::HOUR_1, &next, quote(108.0, 5_400_002), true, bounds).is_none());
        assert!(state.observe_with_grid(Timeframe::HOUR_1, &next, quote(109.0, 5_400_003), true, bounds).is_some());
    }

    #[test]
    fn yahoo_native_hourly_preview_uses_actual_rest_interval() {
        let mut state = Provisional::default();
        let mut rest = bar(1_800_000);
        rest.close_time = 5_399_999;
        let quote = |timestamp_ms| Some(Observation { price: 110.0, timestamp_ms });
        let preview = state.observe_with_grid(Timeframe::HOUR_1, &rest, quote(4_000_000), true, None).unwrap();
        assert_eq!(preview.open_time, 1_800_000);
        assert!(state.observe_with_grid(Timeframe::HOUR_1, &rest, quote(5_400_000), true, None).is_none());
        let mut rest = bar(5_400_000);
        rest.close_time = 7_199_999;
        assert!(state.observe_with_grid(Timeframe::HOUR_1, &rest, quote(6_000_000), true, None).is_some());
        assert!(state.observe_with_grid(Timeframe::HOUR_1, &rest, quote(7_200_000), true, None).is_none());
    }

    #[test]
    fn yahoo_preview_across_session_gap_never_fabricates_intervening_bars() {
        let mut state = Provisional::default();
        let tf = Timeframe::parse("5m").unwrap();
        let rest = bar(0);
        let start = 3 * 86_400_000;
        let observe = |price, timestamp_ms| Some(Observation { price, timestamp_ms });
        let first = state.observe(tf, &rest, observe(90.0, start + 1000)).unwrap();
        assert_eq!((first.open_time, first.open, first.close), (start, 90.0, 90.0));
        let second = state.observe(tf, &rest, observe(92.0, start + 2000)).unwrap();
        assert_eq!((second.open, second.high, second.low, second.close), (90.0, 92.0, 90.0, 92.0));
        assert!(state.observe(tf, &rest, observe(200.0, start - 1000)).is_none());
        assert!(state.observe(tf, &bar(start), observe(92.0, start + 2000)).is_none());
    }

    #[test]
    fn preview_tracks_sampled_extremes_without_mutating_rest() {
        let mut state = Provisional::default();
        let tf = Timeframe::parse("5m").unwrap();
        let rest = bar(0);
        let observe = |price, timestamp_ms| Some(Observation { price, timestamp_ms });
        let first = state.observe(tf, &rest, observe(103.0, 1000)).unwrap();
        assert_eq!(first.high, 103.0);
        let next = state.observe(tf, &rest, observe(97.0, 2000)).unwrap();
        assert_eq!((next.open, next.high, next.low, next.close), (100.0, 103.0, 97.0, 97.0));
        assert_eq!(rest.volume, 1234.0);
        assert_eq!(rest.close, 101.0);
        assert!(state.observe(tf, &rest, observe(200.0, 1500)).is_none());
        let next = state.observe(tf, &rest, observe(104.0, 300_000)).unwrap();
        assert_eq!((next.open_time, next.open, next.close), (300_000, 104.0, 104.0));
        let fresh = state.observe(tf, &bar(300_000), observe(101.0, 301_000)).unwrap();
        assert_eq!((fresh.high, fresh.low), (102.0, 98.0));
        assert!(state.observe(tf, &bar(300_000), None).is_none());
        assert!(state.observe(Timeframe::DAY_1, &bar(0), observe(100.0, 10_000)).is_none());
    }
}

#[cfg(test)]
#[test]
fn yahoo_rest_correction_discards_old_extrema_and_duplicate_tick() {
    let mut state = Provisional::default();
    let tf = Timeframe::parse("5m").unwrap();
    let mut rest = Kline { open_time: 0, close_time: 299_999, open: 100.0, high: 102.0, low: 98.0, close: 101.0, volume: 100.0 };
    let old = Observation { price: 110.0, timestamp_ms: 1000 };
    assert_eq!(state.observe(tf, &rest, Some(old)).unwrap().high, 110.0);
    rest.close = 100.5;
    rest.volume = 200.0;
    assert!(state.observe(tf, &rest, Some(old)).is_none());
    assert!(state.observe(tf, &rest, Some(Observation { price: 120.0, timestamp_ms: 999 })).is_none());
    let next = state.observe(tf, &rest, Some(Observation { price: 101.5, timestamp_ms: 2000 })).unwrap();
    assert_eq!((next.high, next.low, next.close), (102.0, 98.0, 101.5));
    assert_eq!(rest.volume, 200.0);
}
