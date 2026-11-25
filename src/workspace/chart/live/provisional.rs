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
}

impl Provisional {
    pub fn clear(&mut self) {
        self.sampled = None;
    }

    pub(super) fn observe(&mut self, tf: Timeframe, last: &Kline, quote: Option<Observation>) -> Option<Preview> {
        let Some(quote) = quote else {
            self.clear();
            return None;
        };
        // Only the already-existing, forming REST candle is eligible. Do
        // not append a synthetic bar or imply that missing intervals traded.
        if !tf.is_intraday() || !quote.price.is_finite() || quote.price <= 0.0
            || quote.timestamp_ms < last.open_time
            || quote.timestamp_ms >= next_bucket_start_ms(tf, last.open_time)
            || bucket_start_ms(tf, quote.timestamp_ms) != last.open_time
        {
            self.clear();
            return None;
        }
        let mut sampled = match self.sampled {
            Some((old_tf, sample, ts)) if old_tf == tf && sample.open_time == last.open_time => {
                if quote.timestamp_ms < ts {
                    return None;
                }
                sample
            }
            _ => Preview { open_time: last.open_time, open: quote.price, high: quote.price, low: quote.price, close: quote.price },
        };
        sampled.high = sampled.high.max(quote.price);
        sampled.low = sampled.low.min(quote.price);
        sampled.close = quote.price;
        self.sampled = Some((tf, sampled, quote.timestamp_ms));
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
        assert!(state.observe(tf, &rest, observe(104.0, 300_000)).is_none());
        let fresh = state.observe(tf, &bar(300_000), observe(101.0, 301_000)).unwrap();
        assert_eq!((fresh.high, fresh.low), (102.0, 98.0));
        assert!(state.observe(tf, &bar(300_000), None).is_none());
        assert!(state.observe(Timeframe::DAY_1, &bar(0), observe(100.0, 10_000)).is_none());
    }
}
