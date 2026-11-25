//! View-only Yahoo candle preview. Intentionally has no volume or close-time field
//! and cannot be confused with an authoritative Kline.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Preview {
    pub open_time: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
}

impl Preview {
    /// True only when the existing time axis can place this at the next slot.
    pub(crate) fn is_next_after(self, last: &crate::model::Kline) -> bool {
        last.close_time.checked_add(1) == Some(self.open_time)
    }

    /// For a temporary render-only copy of the REST candle. Do not change
    /// timestamps, volume or authoritative history.
    pub(crate) fn apply_to(self, candle: &mut crate::model::Kline) {
        if self.open_time != candle.open_time {
            return;
        }
        candle.open = self.open;
        candle.high = self.high;
        candle.low = self.low;
        candle.close = self.close;
    }
}

#[cfg(test)]
#[test]
fn yahoo_render_copy_keeps_volume_and_time() {
    let rest = crate::model::Kline { open_time: 1, close_time: 60_000, open: 100.0, high: 103.0, low: 99.0, close: 102.0, volume: 500.0 };
    let mut copy = rest.clone();
    Preview { open_time: 1, open: 100.0, high: 105.0, low: 99.0, close: 105.0 }.apply_to(&mut copy);
    assert_eq!((copy.high, copy.close), (105.0, 105.0));
    assert_eq!((copy.open_time, copy.close_time, copy.volume), (rest.open_time, rest.close_time, rest.volume));
    assert_eq!(rest.close, 102.0);
}

/// Independent chart price levels. These are quotes, not candle inputs.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct SessionLevels {
    pub regular: Option<RegularLevel>,
    pub extended: Option<(super::yahoo_session::Session, f64)>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RegularLevel {
    pub price: f64,
    pub change: Option<f64>,
    pub at_close: bool,
}

impl SessionLevels {
    pub(crate) fn from_quotes(
        regular: Option<&crate::model::watchlist::quote::WatchlistQuote>,
        extended: Option<&crate::model::watchlist::quote::WatchlistQuote>,
        now: i64,
        intraday: bool,
    ) -> Self {
        use super::yahoo_session::{Session, session_at};
        use crate::model::watchlist::quote::Freshness;
        let current = session_at(now);
        let regular = regular.and_then(|quote| {
            // Intraday regular candles already have their usual price marker.
            // Outside the regular session, retain the independently polled
            // regular quote even when the chart includes extended candles.
            if !quote.freshness.is_recent() || (intraday && current == Some(Session::Regular)) {
                return None;
            }
            let price = quote.last.filter(|p| p.is_finite() && *p > 0.0)?;
            let time = quote.timestamp_ms?;
            let at_close = session_at(time.saturating_sub(60_000)) == Some(Session::Regular)
                && session_at(time.saturating_add(60_000)) == Some(Session::Post);
            if time > now.saturating_add(60_000)
                || !(at_close || session_at(time) == Some(Session::Regular))
            {
                return None;
            }
            Some(RegularLevel { price, change: quote.change.filter(|v| v.is_finite()), at_close })
        });
        let extended = extended.and_then(|quote| {
            let time = quote.timestamp_ms?;
            let session = session_at(time)?;
            if quote.freshness != Freshness::Delayed || Some(session) != current
                || !matches!(session, Session::Pre | Session::Post)
                || time > now.saturating_add(60_000) || now.saturating_sub(time) > 30 * 60_000
            {
                return None;
            }
            Some((session, quote.last.filter(|p| p.is_finite() && *p > 0.0)?))
        });
        Self { regular, extended }
    }

    pub(crate) fn prices(self) -> impl Iterator<Item = f64> {
        self.regular.map(|r| r.price).into_iter().chain(self.extended.map(|(_, p)| p))
    }
}

#[cfg(test)]
#[test]
fn yahoo_session_price_levels_are_independent_of_candle_timeframe() {
    use crate::model::watchlist::quote::{Freshness, WatchlistQuote};
    use super::yahoo_session::Session;
    let at = |day, hour, minute| crate::pine::time::calendar::make(2026, 11, day, hour, minute, 0, 0).unwrap();
    let regular = WatchlistQuote { last: Some(155.38), change: Some(-2.0), timestamp_ms: Some(at(24, 21, 0)), freshness: Freshness::Delayed, ..Default::default() };
    let extended = WatchlistQuote { last: Some(145.40), timestamp_ms: Some(at(25, 12, 0)), freshness: Freshness::Delayed, ..Default::default() };
    let now = at(25, 12, 1);
    for intraday in [true, false] {
        let levels = SessionLevels::from_quotes(Some(&regular), Some(&extended), now, intraday);
        assert_eq!(levels.regular.unwrap().price, 155.38);
        assert!(levels.regular.unwrap().at_close);
        assert_eq!(levels.extended, Some((Session::Pre, 145.40)));
    }
    assert!(SessionLevels::from_quotes(Some(&regular), Some(&extended), at(25, 13, 0), false).extended.is_none());
    assert_eq!(SessionLevels::from_quotes(Some(&regular), Some(&extended), at(25, 15, 0), true), SessionLevels::default());
    let post = WatchlistQuote { timestamp_ms: Some(at(25, 22, 0)), ..extended };
    assert_eq!(SessionLevels::from_quotes(Some(&regular), Some(&post), at(25, 22, 1), false).extended, Some((Session::Post, 145.40)));
}
