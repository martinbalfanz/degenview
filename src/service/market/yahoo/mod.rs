//! Yahoo Finance's keyless chart, search and spark endpoints.
//!
//! Endpoint selection follows Wickline (MIT, see docs/yahoo-finance.md).
//! Keep vendor timestamps and metadata here; the market adapter owns candle
//! alignment, session semantics and the mapping to DegenView's instrument model.

use crate::model::DataSource;
use crate::service::{ProviderError, net::http};

mod adapter;
pub use adapter::YAHOO;
use serde::Deserialize;
use std::collections::HashSet;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const HOSTS: [&str; 2] = ["query1.finance.yahoo.com", "query2.finance.yahoo.com"];
const HEADERS: &[(&str, &str)] = &[
    ("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Safari/537.36"),
    ("Accept", "application/json"),
];
const DAY: i64 = 86_400;
const RATE_LIMIT_COOLDOWN: Duration = Duration::from_secs(30);
static RATE_LIMITED_UNTIL: Mutex<Option<Instant>> = Mutex::new(None);
pub const QUOTE_BATCH_SIZE: usize = 20;

/// Native Yahoo chart intervals, not a claim about which DegenView timeframes
/// can be constructed from them. Intraday history has a finite vendor horizon.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Interval {
    Minute,
    FiveMinutes,
    FifteenMinutes,
    ThirtyMinutes,
    Hour,
    Day,
    Week,
    Month,
}

impl Interval {
    fn token(self) -> &'static str {
        match self {
            Self::Minute => "1m",
            Self::FiveMinutes => "5m",
            Self::FifteenMinutes => "15m",
            Self::ThirtyMinutes => "30m",
            Self::Hour => "60m",
            Self::Day => "1d",
            Self::Week => "1wk",
            Self::Month => "1mo",
        }
    }

    pub fn max_window_seconds(self) -> Option<i64> {
        match self {
            Self::Minute => Some(7 * DAY),
            Self::FiveMinutes | Self::FifteenMinutes | Self::ThirtyMinutes => Some(60 * DAY),
            Self::Hour => Some(730 * DAY),
            Self::Day | Self::Week | Self::Month => None,
        }
    }
}

/// Chart metadata must travel with its rows. A current GMT offset is not
/// sufficient to reconstruct historical local times across daylight saving.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct ChartMeta {
    pub symbol: String,
    pub currency: Option<String>,
    pub exchange_timezone_name: Option<String>,
    pub exchange_name: Option<String>,
    pub full_exchange_name: Option<String>,
    pub instrument_type: Option<String>,
    pub regular_market_price: Option<f64>,
    pub regular_market_time: Option<i64>,
    pub gmtoffset: Option<i64>,
    pub trading_periods: Option<serde_json::Value>,
    pub current_trading_period: Option<serde_json::Value>,
}

fn index_price_scale(symbol: &str, currency: Option<&str>, timezone: Option<&str>, kind: Option<&str>) -> Option<f64> {
    if !DataSource::Yahoo.is_yahoo_index(symbol) || kind != Some("INDEX") {
        return None;
    }
    if let Some(market) = DataSource::Yahoo.yahoo_market(symbol) {
        if timezone != Some(market.timezone()) {
            return None;
        }
        return market.price_scale(currency?);
    }
    (currency == Some("USD") && matches!(timezone, Some("America/New_York" | "America/Chicago"))).then_some(1.0)
}

fn equity_price_scale(symbol: &str, currency: Option<&str>, timezone: Option<&str>, exchange: Option<&str>, kind: Option<&str>) -> Option<f64> {
    if !matches!(kind, Some("EQUITY" | "ETF")) || DataSource::Yahoo.is_yahoo_index(symbol) {
        return None;
    }
    if let Some(market) = DataSource::Yahoo.yahoo_market(symbol) {
        if timezone != Some(market.timezone()) || exchange != Some(market.exchange()) {
            return None;
        }
        return market.price_scale(currency?);
    }
    (currency == Some("USD") && timezone == Some("America/New_York")).then_some(1.0)
}

impl ChartMeta {
    fn equity_price_scale(&self) -> Option<f64> {
        equity_price_scale(&self.symbol, self.currency.as_deref(), self.exchange_timezone_name.as_deref(), self.exchange_name.as_deref(), self.instrument_type.as_deref())
    }

    fn index_price_scale(&self) -> Option<f64> {
        index_price_scale(&self.symbol, self.currency.as_deref(), self.exchange_timezone_name.as_deref(), self.instrument_type.as_deref())
    }
}

/// Unmodified Yahoo candle. Timestamp is Unix seconds; absent volume is not zero.
#[derive(Clone, Debug, PartialEq)]
pub struct Bar {
    pub timestamp: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Chart {
    pub meta: ChartMeta,
    pub bars: Vec<Bar>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Instrument {
    pub symbol: String,
    pub name: String,
    pub exchange: Option<String>,
    pub exchange_code: Option<String>,
    pub quote_type: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Quote {
    pub symbol: String,
    pub price: f64,
    pub previous_close: Option<f64>,
    pub regular_market_volume: Option<f64>,
    pub timestamp: Option<i64>,
    pub currency: Option<String>,
    pub instrument_type: Option<String>,
    pub exchange_timezone_name: Option<String>,
    pub exchange_name: Option<String>,
}

impl Quote {
    pub fn equity_price_scale(&self) -> Option<f64> {
        equity_price_scale(&self.symbol, self.currency.as_deref(), self.exchange_timezone_name.as_deref(), self.exchange_name.as_deref(), self.instrument_type.as_deref())
    }

    pub fn index_price_scale(&self) -> Option<f64> {
        index_price_scale(&self.symbol, self.currency.as_deref(), self.exchange_timezone_name.as_deref(), self.instrument_type.as_deref())
    }

    #[cfg(test)]
    fn is_supported_equity(&self) -> bool {
        self.equity_price_scale().is_some()
    }

    #[cfg(test)]
    fn is_supported_index(&self) -> bool {
        self.index_price_scale().is_some()
    }

    #[cfg(test)]
    fn is_us_equity(&self) -> bool {
        DataSource::Yahoo.yahoo_market(&self.symbol).is_none() && self.is_supported_equity()
    }
}

#[derive(Deserialize)]
struct ChartEnvelope {
    chart: ChartResponse,
}

#[derive(Deserialize)]
struct ChartResponse {
    result: Option<Vec<ChartResult>>,
    error: Option<YahooError>,
}

#[derive(Deserialize)]
struct YahooError {
    code: Option<String>,
    description: Option<String>,
}

impl YahooError {
    fn into_provider_error(self) -> ProviderError {
        ProviderError::other(format!(
            "Yahoo Finance: {}",
            self.description
                .or(self.code)
                .unwrap_or_else(|| "request failed".into())
        ))
    }
}

#[derive(Deserialize)]
struct ChartResult {
    meta: ChartMeta,
    timestamp: Option<Vec<Option<i64>>>,
    indicators: Option<Indicators>,
}

#[derive(Deserialize)]
struct Indicators {
    quote: Vec<Columns>,
}

#[derive(Deserialize)]
struct Columns {
    open: Vec<Option<f64>>,
    high: Vec<Option<f64>>,
    low: Vec<Option<f64>>,
    close: Vec<Option<f64>>,
    volume: Option<Vec<Option<f64>>>,
}

fn encode(value: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(byte));
        } else {
            write!(out, "%{byte:02X}").expect("writing to String cannot fail");
        }
    }
    out
}

/// Wickline uses two Yahoo hosts. Do not retry a rate limit against the second
/// hostname: a 429 is exposed to DegenView's existing backoff machinery.
fn get(path: &str) -> Result<String, ProviderError> {
    let get_host = |host| {
        // Share the cooldown across chart, search and watchlist workers.
        // Do not hold a lock over I/O or sleep on a worker.
        if crate::lock::lock(&RATE_LIMITED_UNTIL)
            .is_some_and(|until| Instant::now() < until)
        {
            return Err(http::Error::Status(429));
        }
        let url = format!("https://{host}{path}");
        let result = http::get_text(&url, HEADERS, http::Retry::Never);
        if matches!(&result, Err(http::Error::Status(429))) {
            *crate::lock::lock(&RATE_LIMITED_UNTIL) = Some(Instant::now() + RATE_LIMIT_COOLDOWN);
        }
        result
    };
    match get_host(HOSTS[0]) {
        Ok(body) => Ok(body),
        Err(error) if error.is_transient() && error != http::Error::Status(429) => {
            get_host(HOSTS[1]).map_err(Into::into)
        }
        Err(error) => Err(error.into()),
    }
}

fn chart_path(
    symbol: &str,
    interval: Interval,
    start: i64,
    end: i64,
    extended: bool,
) -> Result<String, ProviderError> {
    let symbol = symbol.trim();
    if symbol.is_empty() || start < 0 || end <= start {
        return Err(ProviderError::other("Invalid Yahoo chart symbol or range"));
    }
    if interval
        .max_window_seconds()
        .is_some_and(|max| end - start > max)
    {
        return Err(ProviderError::other("Yahoo chart range exceeds interval limit"));
    }
    let prepost = extended && interval.max_window_seconds().is_some();
    Ok(format!(
        "/v8/finance/chart/{}?interval={}&period1={start}&period2={end}&includePrePost={prepost}",
        encode(symbol),
        interval.token()
    ))
}

/// Fetch one explicitly bounded native interval. The caller chooses the
/// window, handles the vendor's historical reach and folds missing intervals.
pub fn chart(
    symbol: &str,
    interval: Interval,
    start: i64,
    end: i64,
    extended: bool,
) -> Result<Chart, ProviderError> {
    let path = chart_path(symbol, interval, start, end, extended)?;
    let response = parse_chart(&get(&path)?)?;
    if !response.meta.symbol.eq_ignore_ascii_case(symbol.trim()) {
        return Err(ProviderError::data("Yahoo returned a different chart symbol"));
    }
    Ok(response)
}

fn parse_chart(body: &str) -> Result<Chart, ProviderError> {
    let envelope: ChartEnvelope = serde_json::from_str(body).map_err(ProviderError::data)?;
    if let Some(error) = envelope.chart.error {
        return Err(error.into_provider_error());
    }
    let result = envelope
        .chart
        .result
        .and_then(|results| results.into_iter().next())
        .ok_or_else(|| ProviderError::data("Yahoo chart result missing"))?;
    if result.meta.symbol.is_empty() {
        return Err(ProviderError::data("Yahoo chart symbol missing"));
    }
    let timestamps = result.timestamp.unwrap_or_default();
    let mut bars = Vec::with_capacity(timestamps.len());
    if !timestamps.is_empty() {
        let columns = result
            .indicators
            .and_then(|indicators| indicators.quote.into_iter().next())
            .ok_or_else(|| ProviderError::data("Yahoo OHLC columns missing"))?;
        if [
            columns.open.len(),
            columns.high.len(),
            columns.low.len(),
            columns.close.len(),
        ]
        .into_iter()
        .any(|len| len != timestamps.len())
            || columns
                .volume
                .as_ref()
                .is_some_and(|volume| volume.len() != timestamps.len())
        {
            return Err(ProviderError::data("Yahoo OHLC column length mismatch"));
        }
        for (index, timestamp) in timestamps.into_iter().enumerate() {
            let (Some(timestamp), Some(open), Some(high), Some(low), Some(close)) = (
                timestamp,
                columns.open[index],
                columns.high[index],
                columns.low[index],
                columns.close[index],
            ) else {
                continue;
            };
            if timestamp < 0
                || ![open, high, low, close].into_iter().all(f64::is_finite)
                || low > open.min(close)
                || high < open.max(close)
            {
                continue;
            }
            let volume = columns
                .volume
                .as_ref()
                .and_then(|values| values[index])
                .filter(|value| value.is_finite() && *value >= 0.0);
            bars.push(Bar {
                timestamp,
                open,
                high,
                low,
                close,
                volume,
            });
        }
    }
    // Keep the last valid observation for a duplicate timestamp. Do not add
    // the volume of a second snapshot of the same bar to the first.
    bars.sort_by_key(|bar| bar.timestamp);
    let mut unique: Vec<Bar> = Vec::with_capacity(bars.len());
    for bar in bars {
        if let Some(last) = unique.last_mut()
            && last.timestamp == bar.timestamp
        {
            *last = bar;
        } else {
            unique.push(bar);
        }
    }
    Ok(Chart {
        meta: result.meta,
        bars: unique,
    })
}

#[derive(Deserialize)]
struct SearchEnvelope {
    quotes: Vec<SearchHit>,
}

#[derive(Deserialize)]
struct SearchHit {
    symbol: Option<String>,
    shortname: Option<String>,
    longname: Option<String>,
    exchange: Option<String>,
    #[serde(rename = "exchDisp")]
    exchange_display: Option<String>,
    #[serde(rename = "quoteType")]
    quote_type: Option<String>,
}

pub fn search(query: &str) -> Result<Vec<Instrument>, ProviderError> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let path = format!(
        "/v1/finance/search?q={}&quotesCount=25&newsCount=0&listsCount=0&enableFuzzyQuery=false",
        encode(query)
    );
    parse_search(&get(&path)?)
}

fn parse_search(body: &str) -> Result<Vec<Instrument>, ProviderError> {
    let envelope: SearchEnvelope = serde_json::from_str(body).map_err(ProviderError::data)?;
    let mut seen = HashSet::new();
    Ok(envelope
        .quotes
        .into_iter()
        .filter_map(|hit| {
            let symbol = hit.symbol?;
            let quote_type = hit.quote_type?;
            if symbol.is_empty()
                || !matches!(
                    quote_type.as_str(),
                    "EQUITY" | "ETF" | "INDEX" | "CURRENCY" | "FUTURE" | "MUTUALFUND"
                )
                || !seen.insert(symbol.clone())
            {
                return None;
            }
            Some(Instrument {
                name: hit
                    .shortname
                    .filter(|name| !name.is_empty())
                    .or(hit.longname)
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| symbol.clone()),
                symbol,
                exchange: hit.exchange_display.or_else(|| hit.exchange.clone()),
                exchange_code: hit.exchange,
                quote_type,
            })
        })
        .collect())
}

#[derive(Deserialize)]
struct SparkEnvelope {
    spark: SparkResponse,
}

#[derive(Deserialize)]
struct SparkResponse {
    result: Option<Vec<SparkResult>>,
    error: Option<YahooError>,
}

#[derive(Deserialize)]
struct SparkResult {
    symbol: String,
    response: Option<Vec<SparkRow>>,
}

#[derive(Deserialize)]
struct SparkRow {
    meta: SparkMeta,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SparkMeta {
    regular_market_price: Option<f64>,
    regular_market_volume: Option<f64>,
    chart_previous_close: Option<f64>,
    previous_close: Option<f64>,
    regular_market_time: Option<i64>,
    currency: Option<String>,
    instrument_type: Option<String>,
    exchange_timezone_name: Option<String>,
    exchange_name: Option<String>,
}

/// One spark batch, matching Wickline's keyless quote method. The caller
/// chunks larger watchlists and owns scheduling/caching.
pub fn quotes(symbols: &[&str]) -> Result<Vec<Quote>, ProviderError> {
    if symbols.is_empty() {
        return Ok(Vec::new());
    }
    if symbols.len() > QUOTE_BATCH_SIZE
        || symbols
            .iter()
            .any(|symbol| symbol.trim().is_empty() || symbol.contains(','))
    {
        return Err(ProviderError::other("Invalid Yahoo quote batch"));
    }
    let path = format!(
        "/v7/finance/spark?symbols={}&range=1d&interval=1d",
        encode(&symbols.join(","))
    );
    parse_quotes(&get(&path)?)
}

fn parse_quotes(body: &str) -> Result<Vec<Quote>, ProviderError> {
    let envelope: SparkEnvelope = serde_json::from_str(body).map_err(ProviderError::data)?;
    if let Some(error) = envelope.spark.error {
        return Err(error.into_provider_error());
    }
    let results = envelope
        .spark
        .result
        .ok_or_else(|| ProviderError::data("Yahoo spark result missing"))?;
    Ok(results
        .into_iter()
        .filter_map(|result| {
            let meta = result.response?.into_iter().next()?.meta;
            let price = meta.regular_market_price.filter(|price| price.is_finite())?;
            Some(Quote {
                symbol: result.symbol,
                price,
                previous_close: meta
                    .chart_previous_close
                    .filter(|price| price.is_finite() && *price > 0.0)
                    .or_else(|| meta.previous_close.filter(|price| price.is_finite() && *price > 0.0)),
                regular_market_volume: meta
                    .regular_market_volume
                    .filter(|volume| volume.is_finite() && *volume >= 0.0),
                timestamp: meta.regular_market_time.filter(|time| *time > 0),
                currency: meta.currency,
                instrument_type: meta.instrument_type,
                exchange_timezone_name: meta.exchange_timezone_name,
                exchange_name: meta.exchange_name,
            })
        })
        .collect())
}

#[cfg(test)]
mod tests;
