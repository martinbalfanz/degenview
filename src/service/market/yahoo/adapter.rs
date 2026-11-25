//! Native Yahoo chart adapter. Supported market profiles are validated
//! against vendor currency, instrument type and exchange metadata.

use super::{Chart, Instrument, Interval, chart, search};
use crate::model::market::bucket;
use crate::model::{DataSource, Kline, Timeframe};
use crate::service::{MarketSource, NAME_SEPARATOR, ProviderError, SearchResult, rank_results};

pub struct Yahoo;
pub static YAHOO: Yahoo = Yahoo;
const DAY_MS: i64 = 86_400_000;
const MAX_BARS: usize = 10_000;

fn is_supported(hit: &Instrument) -> bool {
    if let Some(market) = DataSource::Yahoo.yahoo_derivative(&hit.symbol) {
        return match market {
            crate::model::YahooDerivative::Forex { .. } => hit.quote_type == "CURRENCY"
                && hit.exchange_code.as_deref() == Some("CCY"),
            crate::model::YahooDerivative::Future { exchange, .. } => hit.quote_type == "FUTURE"
                && hit.exchange_code.as_deref() == Some(exchange),
        };
    }
    if hit.quote_type == "INDEX" {
        return DataSource::Yahoo.is_yahoo_index(&hit.symbol);
    }
    if let Some(market) = DataSource::Yahoo.yahoo_market(&hit.symbol) {
        return matches!(hit.quote_type.as_str(), "EQUITY" | "ETF")
            && hit.exchange_code.as_deref() == Some(market.exchange());
    }
    matches!(hit.quote_type.as_str(), "EQUITY" | "ETF")
        && matches!(
            hit.exchange_code.as_deref(),
            Some("NMS" | "NGM" | "NCM" | "NYQ" | "ASE" | "PCX" | "BTS")
        )
}

fn native(timeframe: Timeframe) -> Result<Interval, ProviderError> {
    if timeframe == Timeframe::DAY_1 {
        Ok(Interval::Day)
    } else if timeframe == Timeframe::MINUTE_1 {
        Ok(Interval::Minute)
    } else if timeframe == Timeframe::parse("5m").expect("native interval") {
        Ok(Interval::FiveMinutes)
    } else if timeframe == Timeframe::parse("15m").expect("native interval") {
        Ok(Interval::FifteenMinutes)
    } else {
        Err(ProviderError::unsupported(DataSource::Yahoo, timeframe))
    }
}

pub(super) fn collect_sessions(value: &serde_json::Value, sessions: &mut Vec<(i64, i64)>) {
    if let Some(items) = value.as_array() {
        for item in items {
            collect_sessions(item, sessions);
        }
    } else if let (Some(start), Some(end)) = (
        value.get("start").and_then(serde_json::Value::as_i64),
        value.get("end").and_then(serde_json::Value::as_i64),
    ) && start < end {
        sessions.push((start, end));
    }
}

/// The supported regular sessions occur within their UTC date.
/// Use that date for daily
/// bars, and real UTC instants for intraday bars (not today's GMT offset).
#[cfg(test)]
fn native_bars(chart: Chart, timeframe: Timeframe) -> Result<Vec<Kline>, ProviderError> {
    native_bars_with_session(chart, timeframe, false)
}

fn native_bars_with_session(chart: Chart, timeframe: Timeframe, extended: bool) -> Result<Vec<Kline>, ProviderError> {
    let meta = chart.meta;
    let index_scale = meta.index_price_scale();
    let derivative = meta.derivative();
    let no_volume = index_scale.is_some() || derivative.is_some_and(|market| !market.has_volume());
    let scale = index_scale.or_else(|| meta.equity_price_scale()).or_else(|| derivative.map(|_| 1.0)).ok_or_else(|| {
        ProviderError::other("Yahoo returned an unsupported market or quote currency")
    })?;
    let mut sessions = Vec::new();
    if let Some(periods) = &meta.trading_periods {
        collect_sessions(periods.get("regular").unwrap_or(periods), &mut sessions);
    }
    if let Some(regular) = meta.current_trading_period.as_ref().and_then(|p| p.get("regular")) {
        collect_sessions(regular, &mut sessions);
    }

    let mut out: Vec<Kline> = Vec::new();
    for mut bar in chart.bars {
        bar.open *= scale;
        bar.high *= scale;
        bar.low *= scale;
        bar.close *= scale;
        // Index levels are not traded shares. Kline currently has no
        // optional volume: use its zero placeholder only for indexes, and
        // hide volume for those symbols at presentation boundaries.
        let volume = if no_volume {
            0.0
        } else if let Some(volume) = bar.volume {
            volume
        } else {
            continue;
        };
        let Some(timestamp) = bar.timestamp.checked_mul(1_000) else {
            continue;
        };
        let open_time = if !timeframe.is_intraday() && let Some(market) = derivative {
            market.trading_day_ms(timestamp)
        } else {
            bucket::bucket_start_ms(timeframe, timestamp)
        };
        // Wickline merges Yahoo's appended closing print. Do that only
        // when metadata confirms the exact close and the previous bar
        // belongs to that same regular session.
        if timeframe.is_intraday()
            && !extended
            && !no_volume
            && bar.volume.is_none_or(|volume| volume == 0.0)
            && bar.open == bar.close
            && bar.high == bar.low
            && bar.high == bar.close
            && let Some((session_start, _)) = sessions.iter().find(|(_, end)| *end == bar.timestamp)
        {
            // A closing print cannot fill a missing interval or change a much
            // earlier candle. Without the final interval, discard the print.
            if let Some(last) = out.last_mut()
                && last.open_time >= session_start.saturating_mul(1_000)
                && last.open_time == bucket::bucket_start_ms(timeframe, timestamp - 1)
            {
                last.high = last.high.max(bar.close);
                last.low = last.low.min(bar.close);
                last.close = bar.close;
            }
            continue;
        }
        if let Some(last) = out.last_mut()
            && last.open_time == open_time
        {
            // Same native bucket, newer snapshot; its cumulative volume
            // is not a second interval's volume.
            last.high = last.high.max(bar.high);
            last.low = last.low.min(bar.low);
            last.close = bar.close;
            last.volume = last.volume.max(volume);
            continue;
        }
        out.push(Kline {
            open_time,
            close_time: bucket::next_bucket_start_ms(timeframe, open_time) - 1,
            open: bar.open,
            high: bar.high,
            low: bar.low,
            close: bar.close,
            volume,
        });
    }
    Ok(out)
}

fn fetch_range(
    symbol: &str,
    timeframe: Timeframe,
    start_ms: i64,
    end_ms: i64,
    max: usize,
    now_ms: i64,
    extended: bool,
) -> Result<Vec<Kline>, ProviderError> {
    if crate::model::market::yahoo_session::native_hourly(DataSource::Yahoo, symbol, timeframe) {
        return fetch_native_hourly(symbol, start_ms, end_ms, max, now_ms, extended);
    }
    let plan = crate::service::market::candles::plan(DataSource::Yahoo, timeframe)?;
    let interval = native(plan.base())?;
    if max == 0 || end_ms <= start_ms {
        return Ok(Vec::new());
    }
    let end = end_ms.min(now_ms);
    let mut start = bucket::bucket_start_ms(timeframe, start_ms.max(0)).max(0);
    if let Some(seconds) = interval.max_window_seconds() {
        // The intraday horizon is relative to now, not the requested end.
        start = start.max(now_ms.saturating_sub(seconds * 1_000).saturating_add(1_000));
    }
    // Overnight daily bars may be stamped on the preceding evening.
    let vendor_start = if !plan.base().is_intraday() && DataSource::Yahoo.yahoo_derivative(symbol).is_some() {
        start.saturating_sub(DAY_MS).max(0)
    } else { start };
    let start_seconds = vendor_start.div_euclid(1_000);
    let end_seconds = end.div_euclid(1_000);
    if end_seconds <= start_seconds {
        return Ok(Vec::new());
    }
    let response = chart(
        &symbol.to_uppercase(),
        interval,
        start_seconds,
        end_seconds,
        extended,
    )?;
    shape_range(response, plan, start, start_ms, end, max, extended)
}

/// Request from the beginning of the UTC day containing the requested start,
/// so Yahoo sees the whole first exchange session instead of a partial
/// arbitrary hourly request. Shape filters back to the exact requested range.
fn hourly_request_bounds(start: i64, end: i64, now: i64) -> Option<(i64, i64)> {
    if start >= end {
        return None;
    }
    let horizon = Interval::Hour.max_window_seconds()? * 1_000;
    let earliest = now.saturating_sub(horizon).saturating_add(1_000).max(0);
    let start = start.max(0).div_euclid(DAY_MS) * DAY_MS;
    let start = start.max(earliest);
    let end = end.min(now);
    if end.div_euclid(1_000) <= start.div_euclid(1_000) {
        return None;
    }
    Some((start, end))
}

fn fetch_native_hourly(
    symbol: &str,
    start: i64,
    end: i64,
    max: usize,
    now: i64,
    extended: bool,
) -> Result<Vec<Kline>, ProviderError> {
    if max == 0 {
        return Ok(Vec::new());
    }
    let Some((request_start, request_end)) = hourly_request_bounds(start, end, now) else {
        return Ok(Vec::new());
    };
    let response = chart(&symbol.to_uppercase(), Interval::Hour, request_start / 1_000, request_end / 1_000, extended)?;
    super::hourly::shape(
        &response, start.max(request_start), request_end, extended, end >= now,
        max.min(MAX_BARS),
    )
}

fn shape_range(
    mut response: Chart,
    plan: bucket::FoldPlan,
    requested_start: i64,
    start: i64,
    end: i64,
    max: usize,
    extended: bool,
) -> Result<Vec<Kline>, ProviderError> {
    // Yahoo sometimes includes a forming/latest observation. A historical
    // range must never incorporate a trade from at or after its exclusive end.
    let derivative = response.meta.derivative().filter(|_| !plan.base().is_intraday());
    response.bars.retain(|bar| {
        bar.timestamp.checked_mul(1_000).is_some_and(|ms| {
            let date = derivative.map_or(ms, |market| market.trading_day_ms(ms));
            date >= requested_start && date < end && ms < end
        })
    });
    let bars = native_bars_with_session(response, plan.base(), extended)?;
    let mut candles = plan.shape_since(bars, usize::MAX, requested_start, usize::MAX);
    candles.retain(|bar| bar.open_time >= start && bar.open_time < end);
    let skip = candles.len().saturating_sub(max.min(MAX_BARS));
    candles.drain(..skip);
    Ok(candles)
}

impl Yahoo {
    pub(crate) fn fetch_klines_with_session(
        &self,
        symbol: &str,
        timeframe: Timeframe,
        limit: u32,
        extended: bool,
    ) -> Result<Vec<Kline>, ProviderError> {
        let now = crate::clock::now_ms();
        let count = i64::from(limit).min(MAX_BARS as i64).saturating_add(1);
        let factor = if timeframe.is_intraday() { 8 } else { 2 };
        let span = timeframe.nominal_ms().saturating_mul(count).saturating_mul(factor);
        let start = now.saturating_sub(span.max(7 * DAY_MS)).max(0);
        let bars = fetch_range(symbol, timeframe, start, now, limit as usize, now, extended)?;
        if bars.is_empty() && limit > 0 {
            return Err(ProviderError::other(format!("No Yahoo data for {symbol}")));
        }
        Ok(bars)
    }

    pub(crate) fn fetch_klines_range_with_session(
        &self,
        symbol: &str,
        timeframe: Timeframe,
        start_ms: i64,
        end_ms: i64,
        max: usize,
        extended: bool,
    ) -> Result<Vec<Kline>, ProviderError> {
        fetch_range(
            symbol,
            timeframe,
            start_ms,
            end_ms,
            max,
            crate::clock::now_ms(),
            extended,
        )
    }
}

impl MarketSource for Yahoo {
    fn fetch_klines(
        &self,
        symbol: &str,
        timeframe: Timeframe,
        limit: u32,
    ) -> Result<Vec<Kline>, ProviderError> {
        let extended = crate::model::market::yahoo_session::extended_candles(
            DataSource::Yahoo,
            symbol,
            timeframe,
        );
        self.fetch_klines_with_session(symbol, timeframe, limit, extended)
    }

    fn fetch_klines_range(
        &self,
        symbol: &str,
        timeframe: Timeframe,
        start_ms: i64,
        end_ms: i64,
        max: usize,
    ) -> Result<Vec<Kline>, ProviderError> {
        let extended = crate::model::market::yahoo_session::extended_candles(
            DataSource::Yahoo,
            symbol,
            timeframe,
        );
        self.fetch_klines_range_with_session(symbol, timeframe, start_ms, end_ms, max, extended)
    }

    fn search_tickers(&self, query: &str) -> Result<Vec<SearchResult>, ProviderError> {
        let hits = search(query)?
            .into_iter()
            .filter(is_supported)
            .map(|hit| {
                let label = format!("{}{NAME_SEPARATOR}{}", hit.symbol, hit.name);
                SearchResult::new(DataSource::Yahoo, label, hit.symbol)
            })
            .collect();
        Ok(rank_results(hits, query))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::market::yahoo::{Bar, ChartMeta};

    fn stock(bars: Vec<Bar>) -> Chart {
        Chart {
            meta: ChartMeta {
                currency: Some("USD".into()),
                exchange_timezone_name: Some("America/New_York".into()),
                instrument_type: Some("EQUITY".into()),
                ..ChartMeta::default()
            },
            bars,
        }
    }

    fn bar(timestamp: i64, close: f64, volume: Option<f64>) -> Bar {
        Bar {
            timestamp,
            open: close,
            high: close,
            low: close,
            close,
            volume,
        }
    }

    #[test]
    fn yahoo_native_hourly_request_bounds_pad_start_but_respect_retention() {
        let now = 1_000 * DAY_MS;
        assert_eq!(hourly_request_bounds(now - 3 * DAY_MS + 1_800_000, now - DAY_MS, now),
            Some((now - 3 * DAY_MS, now - DAY_MS)));
        assert_eq!(hourly_request_bounds(0, now, now), Some((now - 730 * DAY_MS + 1_000, now)));
        assert!(hourly_request_bounds(0, now - 731 * DAY_MS, now).is_none());
    }

    #[test]
    fn daily_snapshots_do_not_double_count_volume() {
        let day = 20_000 * 86_400;
        let chart = stock(vec![
            bar(day + 13 * 3_600, 10.0, Some(100.0)),
            bar(day + 20 * 3_600, 12.0, Some(150.0)),
            bar(day + 86_400 + 13 * 3_600, 13.0, None),
        ]);
        let result = native_bars(chart, Timeframe::DAY_1).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].open_time, day * 1_000);
        assert_eq!(result[0].open, 10.0);
        assert_eq!(result[0].close, 12.0);
        assert_eq!(result[0].volume, 150.0);
    }

    #[test]
    fn exact_session_close_print_updates_previous_candle() {
        let mut chart = stock(vec![
            bar(60, 10.0, Some(100.0)),
            bar(120, 11.0, Some(0.0)),
        ]);
        chart.meta.current_trading_period = Some(serde_json::json!({
            "regular": {"start": 60, "end": 120}
        }));
        let extended = native_bars_with_session(chart.clone(), Timeframe::MINUTE_1, true).unwrap();
        assert_eq!(extended.len(), 2);
        assert_eq!(extended[0].close, 10.0);
        assert_eq!(extended[1].close, 11.0);

        let result = native_bars(chart, Timeframe::MINUTE_1).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].close, 11.0);
        assert_eq!(result[0].volume, 100.0);
    }

    #[test]
    fn a_close_print_cannot_update_an_earlier_bar_across_a_gap() {
        let mut chart = stock(vec![
            bar(60, 10.0, Some(100.0)),
            bar(180, 11.0, Some(0.0)),
        ]);
        chart.meta.current_trading_period = Some(serde_json::json!({
            "regular": {"start": 60, "end": 180}
        }));
        let result = native_bars(chart, Timeframe::MINUTE_1).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].close, 10.0);
    }

    #[test]
    fn historical_fold_does_not_ingest_a_latest_point_outside_the_window() {
        let timeframe = Timeframe::parse("5m").unwrap();
        let plan = bucket::FoldPlan::new(timeframe, &[Timeframe::MINUTE_1]).unwrap();
        let chart = stock(vec![
            bar(300, 10.0, Some(100.0)),
            bar(360, 11.0, Some(150.0)),
            bar(420, 12.0, Some(200.0)),
            bar(480, 99.0, Some(900.0)),
            bar(540, 100.0, Some(900.0)),
        ]);
        let result = shape_range(chart, plan, 300_000, 300_000, 480_000, 1, false).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].close, 12.0);
        assert_eq!(result[0].volume, 450.0);
    }

    #[test]
    fn selected_us_indexes_keep_price_history_without_share_volume() {
        let mut chart = stock(vec![bar(86_400, 100.0, None), bar(172_800, 101.0, Some(123.0))]);
        chart.meta.symbol = "^IXIC".into();
        chart.meta.instrument_type = Some("INDEX".into());
        let result = native_bars(chart, Timeframe::DAY_1).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].close, 100.0);
        assert!(result.iter().all(|bar| bar.volume == 0.0));
    }

    #[test]
    fn unapproved_index_is_not_accepted_even_with_us_metadata() {
        let mut chart = stock(Vec::new());
        chart.meta.symbol = "^OTHER".into();
        chart.meta.instrument_type = Some("INDEX".into());
        assert!(native_bars(chart, Timeframe::DAY_1).is_err());
    }

    #[test]
    fn xetra_equity_requires_matching_market_metadata() {
        let mut chart = stock(vec![bar(86_400 + 7 * 3_600, 100.0, Some(10.0))]);
        chart.meta.symbol = "SAP.DE".into();
        chart.meta.currency = Some("EUR".into());
        chart.meta.exchange_timezone_name = Some("Europe/Berlin".into());
        chart.meta.exchange_name = Some("GER".into());
        assert_eq!(native_bars(chart.clone(), Timeframe::DAY_1).unwrap()[0].open_time, 86_400_000);
        chart.meta.exchange_name = Some("FRA".into());
        assert!(native_bars(chart, Timeframe::DAY_1).is_err());
    }

    #[test]
    fn london_pence_candles_are_normalized_without_scaling_volume() {
        let mut chart = stock(vec![bar(86_400 + 8 * 3_600, 2500.0, Some(500.0))]);
        chart.meta.symbol = "SHEL.L".into();
        chart.meta.currency = Some("GBp".into());
        chart.meta.exchange_timezone_name = Some("Europe/London".into());
        chart.meta.exchange_name = Some("LSE".into());
        let bars = native_bars(chart.clone(), Timeframe::DAY_1).unwrap();
        assert_eq!(bars[0].close, 25.0);
        assert_eq!(bars[0].volume, 500.0);
        chart.meta.currency = Some("USD".into());
        assert!(native_bars(chart, Timeframe::DAY_1).is_err());
    }

    #[test]
    fn futures_daily_evening_timestamp_uses_next_trading_date() {
        let open = crate::pine::time::calendar::make(2026, 1, 11, 23, 0, 0, 0).unwrap();
        let date = crate::pine::time::calendar::make(2026, 1, 12, 0, 0, 0, 0).unwrap();
        let mut chart = stock(vec![bar(open / 1_000, 5000.0, Some(10.0))]);
        chart.meta.symbol = "ES=F".into();
        chart.meta.instrument_type = Some("FUTURE".into());
        chart.meta.exchange_timezone_name = Some("America/Chicago".into());
        chart.meta.exchange_name = Some("CME".into());
        let result = native_bars(chart, Timeframe::DAY_1).unwrap();
        assert_eq!(result[0].open_time, date);
        assert_eq!(result[0].volume, 10.0);
    }

    #[test]
    fn forex_candles_do_not_require_vendor_volume() {
        let mut chart = stock(vec![bar(86_400 + 8 * 3_600, 1.12345, None)]);
        chart.meta.symbol = "EURUSD=X".into();
        chart.meta.instrument_type = Some("CURRENCY".into());
        chart.meta.exchange_timezone_name = Some("Europe/London".into());
        chart.meta.exchange_name = Some("CCY".into());
        let result = native_bars(chart, Timeframe::MINUTE_1).unwrap();
        assert_eq!(result[0].close, 1.12345);
        assert_eq!(result[0].volume, 0.0);
    }

    #[test]
    fn non_us_markets_are_not_mislabeled_as_us_stocks() {
        let mut chart = stock(Vec::new());
        chart.meta.currency = Some("EUR".into());
        assert!(native_bars(chart, Timeframe::DAY_1).is_err());
    }
}
