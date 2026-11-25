//! Process-wide session policy, loaded from saved settings before market workers start.
//! Session changes publish a new revision so open chart histories cannot mix policies. Extended history
//! is limited to US equity/ETF intraday candles.
use crate::model::{DataSource, Timeframe};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Session {
    Pre,
    Regular,
    Post,
}

pub const SETTINGS_KEY: &str = "market.yahoo.extended_hours";
// Low bit is the mode; remaining bits are the revision. One atomic read
// observes both fields, so a worker cannot capture a torn session policy.
static POLICY: OnceLock<AtomicU64> = OnceLock::new();

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub(crate) struct PolicySnapshot {
    pub(crate) extended: bool,
    pub(crate) revision: u64,
}

impl PolicySnapshot {
    fn decode(value: u64) -> Self {
        Self {
            extended: value & 1 != 0,
            revision: value >> 1,
        }
    }

    /// Irrelevant markets have one stable policy, independent of global changes.
    pub(crate) fn effective(self, source: DataSource, symbol: &str, timeframe: Timeframe) -> Self {
        if source.yahoo_stream_symbol(symbol) && timeframe.is_intraday() {
            self
        } else {
            Self::default()
        }
    }
}

fn policy() -> &'static AtomicU64 {
    POLICY.get_or_init(|| AtomicU64::new(u64::from(environment_override().unwrap_or(false))))
}

pub(crate) fn snapshot() -> PolicySnapshot {
    PolicySnapshot::decode(policy().load(Ordering::Acquire))
}

/// Publish one revision only when the effective mode changes.
fn transition(state: &AtomicU64, extended: bool) -> PolicySnapshot {
    let mut old = state.load(Ordering::Acquire);
    loop {
        let current = PolicySnapshot::decode(old);
        if current.extended == extended {
            return current;
        }
        let next = (old & !1).wrapping_add(2) | u64::from(extended);
        match state.compare_exchange_weak(old, next, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => return PolicySnapshot::decode(next),
            Err(actual) => old = actual,
        }
    }
}

fn environment_override() -> Option<bool> {
    match std::env::var("DEGENVIEW_YAHOO_EXTENDED_HOURS").as_deref() {
        Ok("1") => Some(true),
        Ok("0") => Some(false),
        _ => None,
    }
}

/// Called before market workers and restored charts start.
pub fn initialize(saved: bool) {
    let _ = POLICY.set(AtomicU64::new(u64::from(environment_override().unwrap_or(saved))));
}

pub fn extended_enabled() -> bool {
    snapshot().extended
}

/// Applies a persisted preference, respecting the diagnostic startup override.
pub(crate) fn apply_saved(saved: bool) -> PolicySnapshot {
    transition(policy(), environment_override().unwrap_or(saved))
}

pub fn overridden_by_environment() -> bool {
    environment_override().is_some()
}

pub fn extended_candles(source: DataSource, symbol: &str, timeframe: Timeframe) -> bool {
    snapshot().effective(source, symbol, timeframe).extended
}

/// Only the exact 1h US equity/ETF series uses native session-anchored bars.
/// Longer intraday intervals still fold their original UTC-grid bases.
pub(crate) fn native_hourly(source: DataSource, symbol: &str, timeframe: Timeframe) -> bool {
    timeframe == Timeframe::HOUR_1 && source.yahoo_stream_symbol(symbol)
}

/// Standard weekday US equity hours in New York. Holiday/early-close
/// calendars are not modeled: never infer that an exchange is actually open.
pub fn session_at(timestamp_ms: i64) -> Option<Session> {
    let market = crate::pine::time::trading_session::MarketSessions::for_kind("stock");
    if market.is_regular(timestamp_ms) {
        Some(Session::Regular)
    } else if market.is_premarket(timestamp_ms) {
        Some(Session::Pre)
    } else if market.is_postmarket(timestamp_ms) {
        Some(Session::Post)
    } else {
        None
    }
}

/// Extended-session intersections with an actual intraday bar's time range.
/// Fractions refer to the bar's own slot, not the gap to the next bar.
/// US weekday hours are the same standard windows as `session_at`;
/// holiday/early-close metadata is not available here.
pub(crate) fn extended_spans(start: i64, end: i64) -> impl Iterator<Item = (Session, f32, f32)> {
    const DAY: i64 = 86_400_000;
    const MINUTE: i64 = 60_000;
    let valid = end > start && end.saturating_sub(start) <= DAY;
    let local = |time| {
        crate::pine::time::components(time, crate::pine::time::zone_offset_secs(Some("America/New_York"), time))
    };
    let first = local(start);
    let last = local(end.saturating_sub(1));
    let days = [Some(first), (first.year != last.year || first.month != last.month || first.day != last.day).then_some(last)];
    days.into_iter().flatten().filter(move |day| valid && (2..=6).contains(&day.weekday)).flat_map(move |day| {
        // Noon's offset is valid throughout these weekday trading windows.
        let noon = crate::pine::time::calendar::make(day.year, day.month, day.day, 12, 0, 0, 0);
        let midnight = noon.map(|noon| noon - 12 * 60 * MINUTE
            - crate::pine::time::zone_offset_secs(Some("America/New_York"), noon) * 1000);
        [(Session::Pre, 4 * 60, 9 * 60 + 30), (Session::Post, 16 * 60, 20 * 60)]
            .into_iter()
            .filter_map(move |(session, from, to)| {
                let midnight = midnight?;
                let left = start.max(midnight + from * MINUTE);
                let right = end.min(midnight + to * MINUTE);
                if left >= right {
                    return None;
                }
                let duration = (end - start) as f64;
                Some((session, ((left - start) as f64 / duration) as f32, ((right - start) as f64 / duration) as f32))
            })
    })
}

/// Protobuf enum zero is PRE and can be omitted in proto3. Only accept
/// an omitted/zero field in the expected premarket window, never infer a
/// regular or postmarket session from a missing enum.
pub fn admits_wire_session(value: Option<u64>, timestamp_ms: i64, extended: bool) -> bool {
    matches!((value, session_at(timestamp_ms)),
        (Some(1), Some(Session::Regular)))
        || (extended && matches!((value, session_at(timestamp_ms)),
            (None | Some(0), Some(Session::Pre)) | (Some(2), Some(Session::Post))))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn at(hour: i64, minute: i64) -> i64 {
        crate::pine::time::calendar::make(2026, 11, 25, hour, minute, 0, 0).unwrap()
    }
    #[test]
    fn yahoo_native_hourly_scope_is_exact_and_source_qualified() {
        assert!(native_hourly(DataSource::Yahoo, "NVDA", Timeframe::HOUR_1));
        assert!(native_hourly(DataSource::Yahoo, "SPY", Timeframe::HOUR_1));
        for symbol in ["SAP.DE", "^GSPC", "ES=F", "EURUSD=X"] {
            assert!(!native_hourly(DataSource::Yahoo, symbol, Timeframe::HOUR_1));
        }
        assert!(!native_hourly(DataSource::Alpaca, "NVDA", Timeframe::HOUR_1));
        assert!(!native_hourly(DataSource::Yahoo, "NVDA", Timeframe::parse("2h").unwrap()));
    }

    #[test]
    fn yahoo_session_revision_detects_round_trips_and_ignores_noops() {
        let state = AtomicU64::new(0);
        assert_eq!(transition(&state, false), PolicySnapshot::default());
        let extended = transition(&state, true);
        assert_eq!(extended, PolicySnapshot { extended: true, revision: 1 });
        assert_eq!(transition(&state, true), extended);
        let regular = transition(&state, false);
        assert_eq!(regular, PolicySnapshot { extended: false, revision: 2 });
        assert_eq!(PolicySnapshot::decode(state.load(Ordering::Acquire)), regular);
        assert_eq!(regular.effective(DataSource::Yahoo, "NVDA", Timeframe::HOUR_1), regular);
        assert_eq!(regular.effective(DataSource::Yahoo, "NVDA", Timeframe::DAY_1), PolicySnapshot::default());
        assert_eq!(regular.effective(DataSource::Binance, "BTCUSDT", Timeframe::HOUR_1), PolicySnapshot::default());
    }

    #[test]
    fn yahoo_session_shading_matches_new_york_hours_and_splits_mixed_buckets() {
        let spans = |start, end| extended_spans(start, end).collect::<Vec<_>>();
        assert_eq!(spans(at(9, 0), at(10, 0)), vec![(Session::Pre, 0.0, 1.0)]);
        assert_eq!(spans(at(14, 0), at(15, 0)), vec![(Session::Pre, 0.0, 0.5)]);
        assert!(spans(at(15, 0), at(16, 0)).is_empty());
        assert_eq!(spans(at(20, 30), at(21, 30)), vec![(Session::Post, 0.5, 1.0)]);
        assert_eq!(spans(at(22, 0), at(23, 0)), vec![(Session::Post, 0.0, 1.0)]);
        let summer = |hour| crate::pine::time::calendar::make(2026, 7, 1, hour, 0, 0, 0).unwrap();
        assert_eq!(spans(summer(13), summer(14)), vec![(Session::Pre, 0.0, 0.5)]);
        assert_eq!(spans(summer(20), summer(21)), vec![(Session::Post, 0.0, 1.0)]);
        let weekend = |hour| crate::pine::time::calendar::make(2026, 11, 28, hour, 0, 0, 0).unwrap();
        assert!(spans(weekend(10), weekend(23)).is_empty());
        assert!(spans(at(14, 0), at(14, 0)).is_empty());
        assert!(spans(at(14, 0), at(14, 0) + 2 * 86_400_000).is_empty());
    }

    #[test]
    fn wire_session_must_agree_with_new_york_window() {
        let pre = at(12, 0);
        let regular = at(15, 0);
        let post = at(22, 0);
        assert!(admits_wire_session(None, pre, true));
        assert!(admits_wire_session(Some(0), pre, true));
        assert!(!admits_wire_session(Some(0), pre, false));
        assert!(!admits_wire_session(Some(1), pre, true));
        assert!(admits_wire_session(Some(1), regular, false));
        assert!(admits_wire_session(Some(2), post, true));
        assert!(!admits_wire_session(Some(2), post, false));
        assert!(!admits_wire_session(Some(3), post, true));
    }
}
