//! Process-wide session policy, loaded from saved settings before market workers start.
//! Restart to change it so open chart history cannot mix two session policies. Extended history
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

// Kept test-only until the remaining runtime consumers can invalidate their
// work. The settings path must not publish a live policy change yet.
#[cfg(test)]
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

pub fn overridden_by_environment() -> bool {
    environment_override().is_some()
}

pub fn extended_candles(source: DataSource, symbol: &str, timeframe: Timeframe) -> bool {
    snapshot().effective(source, symbol, timeframe).extended
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
