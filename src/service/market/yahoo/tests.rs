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
    assert!(chart_path("AAPL", Interval::Hour, 0, DAY, true)
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
