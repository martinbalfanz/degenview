use super::*;

#[test]
fn symbols_and_searches_are_encoded_as_single_url_values() {
    assert_eq!(encode("^GSPC"), "%5EGSPC");
    assert_eq!(encode("EURUSD=X"), "EURUSD%3DX");
    assert_eq!(encode("A/B & ü"), "A%2FB%20%26%20%C3%BC");
    let path = chart_path("BRK-B", Interval::Day, 0, DAY, true).unwrap();
    assert!(path.ends_with("includePrePost=false"));
    assert!(chart_path("AAPL", Interval::Minute, 0, 8 * DAY, false).is_err());
    assert!(chart_path("AAPL", Interval::Minute, DAY, DAY, false).is_err());
    assert!(chart_path("AAPL", Interval::FiveMinutes, 0, DAY, true)
        .unwrap()
        .ends_with("includePrePost=true"));
}

#[test]
fn chart_preserves_utc_timestamps_nulls_and_latest_duplicate() {
    let body = r#"{"chart":{"error":null,"result":[{
        "meta":{"symbol":"AAPL","currency":"USD",
                "exchangeTimezoneName":"America/New_York","gmtoffset":-18000},
        "timestamp":[200,100,200,300,400,500],
        "indicators":{"quote":[{
            "open":[12,10,12,null,14,15],
            "high":[14,11,15,14,13,16],
            "low":[11,9,11,12,13,14],
            "close":[13,10,14,13,14,15],
            "volume":[100,null,110,200,300,0]
        }]}
    }]}}"#;
    let chart = parse_chart(body).unwrap();
    assert_eq!(chart.meta.exchange_timezone_name.as_deref(), Some("America/New_York"));
    assert_eq!(chart.bars.len(), 3);
    assert_eq!(chart.bars[0].timestamp, 100);
    assert_eq!(chart.bars[0].volume, None);
    assert_eq!(chart.bars[1].timestamp, 200);
    assert_eq!(chart.bars[1].close, 14.0);
    assert_eq!(chart.bars[1].volume, Some(110.0));
    assert_eq!(chart.bars[2].volume, Some(0.0));
}

#[test]
fn malformed_columns_and_vendor_errors_do_not_become_empty_history() {
    let invalid = r#"{"chart":{"result":[{
        "meta":{"symbol":"AAPL"},"timestamp":[100,200],
        "indicators":{"quote":[{"open":[1],"high":[2,2],"low":[1,1],"close":[2,2]}]}
    }],"error":null}}"#;
    assert!(matches!(parse_chart(invalid), Err(ProviderError::Data(_))));
    let refused = r#"{"chart":{"result":null,"error":{"code":"Not Found","description":"No data"}}}"#;
    assert!(matches!(parse_chart(refused), Err(ProviderError::Other(_))));
    let empty = r#"{"chart":{"result":[{"meta":{"symbol":"AAPL"}}],"error":null}}"#;
    assert!(parse_chart(empty).unwrap().bars.is_empty());
}

#[test]
fn search_preserves_instrument_identity_and_excludes_options_and_duplicates() {
    let body = r#"{"quotes":[
        {"symbol":"^GSPC","shortname":"S&P 500","quoteType":"INDEX"},
        {"symbol":"BAS.DE","longname":"BASF SE","quoteType":"EQUITY","exchDisp":"XETRA"},
        {"symbol":"BAS.DE","quoteType":"EQUITY"},
        {"symbol":"AAPL261204C00100000","quoteType":"OPTION"},
        {"symbol":"EURUSD=X","quoteType":"CURRENCY"},
        {"symbol":"UNKNOWN"}
    ]}"#;
    let instruments = parse_search(body).unwrap();
    assert_eq!(instruments.len(), 3);
    assert_eq!(instruments[0].symbol, "^GSPC");
    assert_eq!(instruments[1].name, "BASF SE");
    assert_eq!(instruments[1].exchange.as_deref(), Some("XETRA"));
    assert_eq!(instruments[2].name, "EURUSD=X");
}

#[test]
fn spark_preserves_previous_close_currency_and_vendor_time() {
    let body = r#"{"spark":{"error":null,"result":[
        {"symbol":"BAS.DE","response":[{"meta":{
            "regularMarketPrice":45.5,"chartPreviousClose":44.0,
            "previousClose":43.0,"regularMarketTime":123,"currency":"EUR"
        }}]},
        {"symbol":"AAPL","response":[{"meta":{"regularMarketPrice":100.0}}]},
        {"symbol":"MISSING","response":[]}
    ]}}"#;
    let result = parse_quotes(body).unwrap();
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].previous_close, Some(44.0));
    assert_eq!(result[0].timestamp, Some(123));
    assert_eq!(result[0].currency.as_deref(), Some("EUR"));
    assert_eq!(result[1].timestamp, None);
    assert_eq!(result[1].previous_close, None);
    assert!(quotes(&["AAPL,MSFT"]).is_err());
    assert!(quotes(&["AAPL"; QUOTE_BATCH_SIZE + 1]).is_err());
}

#[test]
fn only_us_equity_quotes_qualify_for_the_initial_adapter() {
    let body = r#"{"spark":{"error":null,"result":[{"symbol":"AAPL","response":[{"meta":{"regularMarketPrice":100.0,"currency":"USD","instrumentType":"EQUITY","exchangeTimezoneName":"America/New_York"}}]},{"symbol":"BAS.DE","response":[{"meta":{"regularMarketPrice":50.0,"currency":"EUR","instrumentType":"EQUITY","exchangeTimezoneName":"Europe/Berlin"}}]}]}}"#;
    let quotes = parse_quotes(body).unwrap();
    assert!(quotes[0].is_us_equity());
    assert!(!quotes[1].is_us_equity());
}

#[test]
fn spark_uses_valid_previous_close_and_does_not_invent_volume() {
    let body = r#"{"spark":{"error":null,"result":[
        {"symbol":"NVDA","response":[{"meta":{"regularMarketPrice":150.0,"chartPreviousClose":0.0,"previousClose":145.0,"regularMarketVolume":123456,"regularMarketTime":1700000000}}]},
        {"symbol":"AAPL","response":[{"meta":{"regularMarketPrice":100.0,"regularMarketVolume":-1,"regularMarketTime":0}}]}
    ]}}"#;
    let quotes = parse_quotes(body).unwrap();
    assert_eq!(quotes[0].previous_close, Some(145.0));
    assert_eq!(quotes[0].regular_market_volume, Some(123456.0));
    assert_eq!(quotes[1].regular_market_volume, None);
    assert_eq!(quotes[1].timestamp, None);
}

#[test]
fn spark_index_metadata_is_distinct_from_equities() {
    let body = r#"{"spark":{"result":[{"symbol":"^VIX","response":[{"meta":{"regularMarketPrice":20,"currency":"USD","instrumentType":"INDEX","exchangeTimezoneName":"America/Chicago"}}]},{"symbol":"^GDAXI","response":[{"meta":{"regularMarketPrice":20000,"currency":"EUR","instrumentType":"INDEX","exchangeTimezoneName":"Europe/Berlin"}}]}]}}"#;
    let quotes = parse_quotes(body).unwrap();
    assert!(quotes[0].is_supported_index());
    assert!(!quotes[0].is_us_equity());
    assert!(quotes[1].is_supported_index());
}

#[test]
fn xetra_quote_requires_verified_exchange_currency_and_zone() {
    let body = r#"{"spark":{"result":[{"symbol":"SAP.DE","response":[{"meta":{"regularMarketPrice":200,"currency":"EUR","instrumentType":"EQUITY","exchangeTimezoneName":"Europe/Berlin","exchangeName":"GER"}}]},{"symbol":"SAP.DE","response":[{"meta":{"regularMarketPrice":200,"currency":"USD","instrumentType":"EQUITY","exchangeTimezoneName":"Europe/Berlin","exchangeName":"GER"}}]}]}}"#;
    let quotes = parse_quotes(body).unwrap();
    assert!(quotes[0].is_supported_equity());
    assert!(!quotes[0].is_us_equity());
    assert!(!quotes[1].is_supported_equity());
}

#[test]
fn london_spark_prices_and_previous_close_share_a_scale() {
    let body = r#"{"spark":{"result":[{"symbol":"SHEL.L","response":[{"meta":{"regularMarketPrice":2500,"previousClose":2400,"regularMarketVolume":100,"currency":"GBp","instrumentType":"EQUITY","exchangeTimezoneName":"Europe/London","exchangeName":"LSE"}}]},{"symbol":"SHEL.L","response":[{"meta":{"regularMarketPrice":25,"currency":"USD","instrumentType":"EQUITY","exchangeTimezoneName":"Europe/London","exchangeName":"LSE"}}]}]}}"#;
    let quotes = parse_quotes(body).unwrap();
    let scale = quotes[0].equity_price_scale().unwrap();
    assert_eq!(quotes[0].price * scale, 25.0);
    assert_eq!(quotes[0].previous_close.unwrap() * scale, 24.0);
    assert_eq!(quotes[0].regular_market_volume, Some(100.0));
    assert_eq!(quotes[1].equity_price_scale(), None);
}

#[test]
fn derivative_metadata_rejects_wrong_type_currency_or_exchange() {
    let body = r#"{"spark":{"result":[{"symbol":"EURUSD=X","response":[{"meta":{"regularMarketPrice":1.15,"currency":"USD","instrumentType":"CURRENCY","exchangeTimezoneName":"Europe/London","exchangeName":"CCY"}}]},{"symbol":"GC=F","response":[{"meta":{"regularMarketPrice":2500,"currency":"USD","instrumentType":"FUTURE","exchangeTimezoneName":"America/New_York","exchangeName":"CMX"}}]},{"symbol":"ES=F","response":[{"meta":{"regularMarketPrice":5000,"currency":"EUR","instrumentType":"FUTURE","exchangeTimezoneName":"America/Chicago","exchangeName":"CME"}}]}]}}"#;
    let quotes = parse_quotes(body).unwrap();
    assert!(!quotes[0].derivative().unwrap().has_volume());
    assert!(quotes[1].derivative().unwrap().has_volume());
    assert!(quotes[2].derivative().is_none());
}

// P3 diagnostics intentionally use the raw vendor intervals. Do not normalize
// a native hourly timestamp through the UTC fold before comparing it.
#[derive(Debug, Default, PartialEq, Eq)]
struct HourlyComparison {
    aligned_utc: usize,
    shifted_utc: usize,
    complete_quarters: usize,
    matching_ohlcv: usize,
    matching_ohlc: usize,
    incomplete_quarters: usize,
}

fn close_enough(a: f64, b: f64) -> bool {
    (a - b).abs() <= a.abs().max(b.abs()).max(1.0) * 1e-6
}

fn compare_native_hourly(hours: &[Bar], quarters: &[Bar]) -> HourlyComparison {
    let mut report = HourlyComparison::default();
    for hour in hours {
        if hour.timestamp.rem_euclid(3_600) == 0 {
            report.aligned_utc += 1;
        } else {
            report.shifted_utc += 1;
        }
        let start = quarters.partition_point(|bar| bar.timestamp < hour.timestamp);
        let end = quarters.partition_point(|bar| bar.timestamp < hour.timestamp + 3_600);
        let window = &quarters[start..end];
        if window.len() != 4
            || window.iter().enumerate().any(|(i, bar)| bar.timestamp != hour.timestamp + i as i64 * 900)
        {
            // Session-end partial bars, missing observations and request
            // boundaries require separate interpretation, not fake quarters.
            report.incomplete_quarters += 1;
            continue;
        }
        report.complete_quarters += 1;
        let high = window.iter().map(|bar| bar.high).fold(f64::NEG_INFINITY, f64::max);
        let low = window.iter().map(|bar| bar.low).fold(f64::INFINITY, f64::min);
        let ohlc_matches = [hour.open, hour.high, hour.low, hour.close].into_iter()
            .zip([window[0].open, high, low, window[3].close])
            .all(|(a, b)| close_enough(a, b));
        if ohlc_matches {
            report.matching_ohlc += 1;
            let volume = window.iter().try_fold(0.0, |sum, bar| bar.volume.map(|v| sum + v));
            if hour.volume.zip(volume).is_some_and(|(a, b)| close_enough(a, b)) {
                report.matching_ohlcv += 1;
            }
        }
    }
    report
}

#[test]
fn yahoo_native_hourly_comparison_does_not_relabel_shifted_bars_or_fill_gaps() {
    let raw = |timestamp, price: f64, volume| Bar {
        timestamp, open: price, high: price + 1.0, low: price - 1.0, close: price,
        volume: Some(volume),
    };
    let quarters = vec![
        raw(1800, 10.0, 10.0), raw(2700, 11.0, 20.0),
        raw(3600, 12.0, 30.0), raw(4500, 13.0, 40.0),
        raw(5400, 14.0, 50.0),
    ];
    let hours = vec![
        Bar { timestamp: 1800, open: 10.0, high: 14.0, low: 9.0, close: 13.0, volume: Some(100.0) },
        raw(5400, 14.0, 50.0),
    ];
    assert_eq!(compare_native_hourly(&hours, &quarters), HourlyComparison {
        aligned_utc: 0, shifted_utc: 2, complete_quarters: 1,
        matching_ohlcv: 1, matching_ohlc: 1, incomplete_quarters: 1,
    });
    let mut wrong_volume = hours.clone();
    wrong_volume[0].volume = Some(99.0);
    let report = compare_native_hourly(&wrong_volume, &quarters);
    assert_eq!((report.matching_ohlc, report.matching_ohlcv), (1, 0));
    let mut relabeled = hours;
    relabeled[0].timestamp = 3600;
    assert_eq!(compare_native_hourly(&relabeled, &quarters).matching_ohlcv, 0);
}

#[test]
fn yahoo_native_hourly_diagnostic_range_and_session_flag() {
    assert_eq!(Interval::Hour.max_window_seconds(), Some(730 * DAY));
    let path = chart_path("NVDA", Interval::Hour, 0, 730 * DAY, true).unwrap();
    assert!(path.contains("interval=60m"));
    assert!(path.ends_with("includePrePost=true"));
    assert!(chart_path("NVDA", Interval::Hour, 0, 731 * DAY, false).is_err());
}

fn print_hourly_samples(label: &str, response: &Chart) {
    let mut offsets = std::collections::BTreeMap::new();
    for bar in &response.bars {
        *offsets.entry(bar.timestamp.rem_euclid(3_600) / 60).or_insert(0usize) += 1;
    }
    println!("{label}: symbol={} zone={:?} bars={} UTC minute offsets={offsets:?}",
        response.meta.symbol, response.meta.exchange_timezone_name, response.bars.len());
    println!("  first={:?} last={:?}", response.bars.first(), response.bars.last());
    println!("  trading_periods present={} current_trading_period={:?}",
        response.meta.trading_periods.is_some(), response.meta.current_trading_period);
    for bar in response.bars.iter().take(20) {
        let ms = bar.timestamp * 1000;
        let time = crate::pine::time::calendar::components(ms
            + crate::pine::time::zone_offset_secs(Some("America/New_York"), ms) * 1000);
        println!("  NY {:04}-{:02}-{:02} {:02}:{:02} UTC={} O={} H={} L={} C={} V={:?}",
            time.year, time.month, time.day, time.hour, time.minute, bar.timestamp,
            bar.open, bar.high, bar.low, bar.close, bar.volume);
    }
}

fn probe_native_hourly(extended: bool) {
    let symbol = "NVDA";
    // Closed UTC days only; seven days include several completed sessions.
    // Each probe uses exactly three chart requests via the existing transport.
    let today = (crate::clock::now_ms() / 1000).div_euclid(DAY) * DAY;
    let recent_start = today - 7 * DAY;
    let old_start = today - 700 * DAY;
    let old_end = old_start + 7 * DAY;
    let old = chart(symbol, Interval::Hour, old_start, old_end, extended)
        .expect("Yahoo native 60m old window");
    print_hourly_samples("700-day-old sample", &old);
    assert!(old.meta.equity_price_scale().is_some(), "unexpected instrument metadata");
    assert!(!old.bars.is_empty(), "no old hourly history returned");
    assert!(old.bars.iter().all(|bar| (old_start..old_end).contains(&bar.timestamp)),
        "old response included a timestamp outside the requested window");

    let hours = chart(symbol, Interval::Hour, recent_start, today, extended)
        .expect("Yahoo recent native 60m");
    let quarters = chart(symbol, Interval::FifteenMinutes, recent_start, today, extended)
        .expect("Yahoo recent 15m comparison");
    print_hourly_samples("recent native hourly", &hours);
    assert!(hours.meta.equity_price_scale().is_some());
    assert!(quarters.meta.equity_price_scale().is_some());
    assert!(!hours.bars.is_empty() && !quarters.bars.is_empty());
    let report = compare_native_hourly(&hours.bars, &quarters.bars);
    println!("includePrePost={extended} comparison={report:?}");
    assert!(report.complete_quarters > 0, "no complete hourly comparison windows");
    // Differences are diagnostic results, not an assertion that two feeds
    // must match. Retain the raw timestamps and report before choosing a grid.
}

#[test]
#[ignore = "network; three Yahoo requests; tests regular native hourly retention and alignment"]
fn live_yahoo_native_hourly_regular_history() {
    probe_native_hourly(false);
}

#[test]
#[ignore = "network; three Yahoo requests; tests extended native hourly retention and alignment"]
fn live_yahoo_native_hourly_extended_history() {
    probe_native_hourly(true);
}

#[test]
#[ignore = "network; native hourly historical period coverage and candidate shaping"]
fn live_yahoo_native_hourly_shape_contract() {
    let today = (crate::clock::now_ms() / 1000).div_euclid(DAY) * DAY;
    // A single seven-day old and recent request per mode, plus the recent
    // short-day window around US Thanksgiving 2025. No new market subscription.
    let short_day = crate::pine::time::calendar::make(2025, 11, 28, 0, 0, 0, 0).unwrap() / 1000;
    let mut failures = Vec::new();
    for extended in [false, true] {
        for (name, start, end) in [
            ("old", today - 700 * DAY, today - 693 * DAY),
            ("recent", today - 7 * DAY, today),
            ("short-day", short_day, short_day + 2 * DAY),
        ] {
            let raw = chart("NVDA", Interval::Hour, start, end, extended).expect("native hourly chart");
            println!("{name} extended={extended} raw={} period metadata={:?}", raw.bars.len(), raw.meta.trading_periods);
            match hourly::shape(&raw, start * 1000, end * 1000, extended, false, 10_000) {
                Ok(bars) => {
                    let mut spans = std::collections::BTreeMap::new();
                    for bar in &bars {
                        *spans.entry((bar.close_time + 1 - bar.open_time) / 60_000).or_insert(0usize) += 1;
                    }
                    println!("  shaped={} duration minutes={spans:?} first={:?} last={:?}", bars.len(), bars.first(), bars.last());
                    if bars.is_empty() {
                        failures.push(format!("{name} extended={extended}: empty shaped history"));
                    }
                }
                Err(error) => {
                    println!("  shape error: {error:?}");
                    failures.push(format!("{name} extended={extended}: {error:?}"));
                }
            }
        }
    }
    assert!(failures.is_empty(), "native hourly contract failures: {failures:?}");
}
