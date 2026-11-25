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
}

impl Provisional {
    pub fn clear(&mut self) {
        self.sampled = None;
        self.rest = None;
        self.observed_through = None;
    }

    pub(super) fn observe(&mut self, tf: Timeframe, last: &Kline, quote: Option<Observation>) -> Option<Preview> {
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
        }
        let Some(quote) = quote else {
            self.sampled = None;
            return None;
        };
        let bucket = bucket_start_ms(tf, quote.timestamp_ms);
        let next = next_bucket_start_ms(tf, last.open_time);
        // Only an observed, contiguous next bucket can occupy the existing
        // future slot on the UTC grid. Never bridge gaps or fabricate bars.
        let contiguous = bucket == next
            && last.close_time.checked_add(1) == Some(next)
            && next_bucket_start_ms(tf, next) - next == next - last.open_time;
        if !tf.is_intraday() || !quote.price.is_finite() || quote.price <= 0.0
            || (bucket != last.open_time && !contiguous)
        {
            self.sampled = None;
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
            Some((old_tf, sample, ts)) if old_tf == tf && sample.open_time == bucket => {
                if quote.timestamp_ms < ts {
                    return None;
                }
                sample
            }
            _ => Preview { open_time: bucket, open: quote.price, high: quote.price, low: quote.price, close: quote.price },
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
        assert!(state.observe(tf, &bar(300_000), observe(200.0, 900_000)).is_none());
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
