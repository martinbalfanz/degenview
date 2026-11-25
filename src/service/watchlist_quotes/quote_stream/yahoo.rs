//! Yahoo PricingData envelope: JSON carrying a base64 protobuf message.
//! Protocol reference: Wickline server.mjs (MIT, docs/yahoo-finance.md).
//! USD equity/ETF incremental ticks may omit currency and other fields.
//! An explicit non-USD currency is rejected; the quote engine requires a
//! validated REST snapshot before any tick can affect an instrument.
use super::StreamTick;
use crate::model::watchlist::quote::{SourceQuote, VolumeKind};
use crate::model::{DataSource, encoding};
use crate::service::net::ws::{Keepalive, Session};
use serde_json::{Value, json};
use std::time::Duration;

const HOST: &str = "streamer.finance.yahoo.com";
const MAX_MESSAGE: usize = 16 * 1024;

pub(super) fn session(products: &[String]) -> Session {
    Session {
        url: format!("wss://{HOST}/?version=2"),
        host: HOST.to_owned(),
        port: 443,
        subscribe: vec![json!({"subscribe": products}).to_string()],
        max_data_silence: Duration::from_secs(600),
        keepalive: Keepalive::Frame,
    }
}

fn varint(input: &mut &[u8]) -> Option<u64> {
    let mut value = 0u64;
    for shift in (0..70).step_by(7) {
        let (byte, rest) = input.split_first()?;
        *input = rest;
        if shift == 63 && *byte > 1 {
            return None;
        }
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}

fn take<'a>(input: &mut &'a [u8], len: usize) -> Option<&'a [u8]> {
    let (value, rest) = input.split_at_checked(len)?;
    *input = rest;
    Some(value)
}

fn zigzag(value: u64) -> i64 {
    ((value >> 1) as i64) ^ -((value & 1) as i64)
}

#[derive(Debug, Default)]
struct Pricing<'a> {
    symbol: Option<&'a str>,
    price: Option<f64>,
    timestamp: Option<i64>,
    currency: Option<&'a str>,
    session: Option<u64>,
    volume: Option<i64>,
}

fn decode(mut input: &[u8]) -> Option<Pricing<'_>> {
    let mut pricing = Pricing::default();
    while !input.is_empty() {
        let key = varint(&mut input)?;
        let field = key >> 3;
        if field == 0 || field > 0x1fff_ffff {
            return None;
        }
        match key & 7 {
            0 => {
                let value = varint(&mut input)?;
                match field {
                    3 => pricing.timestamp = Some(zigzag(value)),
                    7 => pricing.session = Some(value),
                    9 => pricing.volume = Some(zigzag(value)),
                    _ => {}
                }
            }
            1 => {
                take(&mut input, 8)?;
            }
            2 => {
                let len = usize::try_from(varint(&mut input)?).ok()?;
                let value = take(&mut input, len)?;
                match field {
                    1 => pricing.symbol = Some(std::str::from_utf8(value).ok()?),
                    4 => pricing.currency = Some(std::str::from_utf8(value).ok()?),
                    _ => {}
                }
            }
            5 => {
                let value = f32::from_le_bytes(take(&mut input, 4)?.try_into().ok()?);
                if field == 2 {
                    // PricingData carries float32; preserve its seven significant digits.
                    pricing.price = format!("{value:.6e}").parse().ok();
                }
            }
            _ => return None,
        }
    }
    Some(pricing)
}

pub(super) fn parse_tick(text: &str) -> Option<StreamTick> {
    // The watchlist tracks both sessions independently of chart history.
    parse_tick_with_session(text, true)
}

fn payload(text: &str) -> Option<Vec<u8>> {
    if text.len() > MAX_MESSAGE {
        return None;
    }
    let envelope: Value = serde_json::from_str(text).ok()?;
    encoding::base64_decode(envelope.get("message")?.as_str()?)
}

fn parse_tick_with_session(text: &str, extended: bool) -> Option<StreamTick> {
    if text.len() > MAX_MESSAGE {
        return None;
    }
    let data = payload(text)?;
    let pricing = decode(&data)?;
    let symbol = pricing.symbol?;
    let price = pricing
        .price
        .filter(|price| price.is_finite() && *price > 0.0)?;
    let timestamp = pricing.timestamp.filter(|timestamp| *timestamp > 0)?;
    if !DataSource::Yahoo.yahoo_stream_symbol(symbol)
        || pricing.currency.is_some_and(|currency| currency != "USD")
        || !crate::model::market::yahoo_session::admits_wire_session(
            pricing.session,
            timestamp,
            extended,
        )
    {
        return None;
    }
    // Extended-session cumulative volume is not the regular session's share volume.
    let volume = if pricing.session == Some(1) {
        pricing
            .volume
            .filter(|volume| *volume >= 0)
            .map(|volume| volume as f64)
    } else {
        None
    };
    Some(StreamTick {
        source: DataSource::Yahoo,
        product_id: symbol.to_ascii_uppercase(),
        quote: SourceQuote {
            price,
            timestamp_ms: Some(timestamp),
            volume_24h: volume,
            volume_kind: volume.map(|_| VolumeKind::Shares),
            ..SourceQuote::default()
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(session: Option<u8>, timestamp: i64, currency: Option<&str>) -> String {
        let mut bytes = vec![10, 4, b'N', b'V', b'D', b'A', 21];
        bytes.extend_from_slice(&123.5f32.to_le_bytes());
        bytes.push(24);
        let mut stamp = (timestamp as u64) << 1;
        while stamp >= 128 {
            bytes.push((stamp as u8 & 0x7f) | 0x80);
            stamp >>= 7;
        }
        bytes.push(stamp as u8);
        if let Some(currency) = currency {
            bytes.extend_from_slice(&[34, currency.len() as u8]);
            bytes.extend_from_slice(currency.as_bytes());
        }
        if let Some(session) = session {
            bytes.extend_from_slice(&[56, session]);
        }
        bytes.extend_from_slice(&[72, 20]);
        json!({"message": encoding::base64_encode(&bytes)}).to_string()
    }

    fn premarket_timestamp() -> i64 {
        1795607849000
    }

    #[test]
    fn pricing_data_allows_sparse_currency_but_checks_explicit_currency_and_session() {
        let timestamp = crate::pine::time::calendar::make(2026, 11, 25, 15, 0, 0, 0).unwrap();
        let tick =
            parse_tick_with_session(&message(Some(1), timestamp, Some("USD")), false).unwrap();
        assert_eq!(tick.product_id, "NVDA");
        assert_eq!(tick.quote.price, 123.5);
        assert_eq!(tick.quote.timestamp_ms, Some(timestamp));
        assert_eq!(tick.quote.volume_24h, Some(10.0));
        assert!(parse_tick_with_session(&message(Some(1), timestamp, None), false).is_some());
        assert!(
            parse_tick_with_session(&message(Some(1), timestamp, Some("EUR")), false).is_none()
        );
        // Observed live payload: PRE, price and vendor time, no currency/volume.
        assert!(
            parse_tick_with_session(&message(Some(0), premarket_timestamp(), None), true).is_some()
        );
        assert!(
            parse_tick_with_session(&message(Some(0), timestamp, Some("USD")), false).is_none()
        );
        assert!(
            parse_tick_with_session(&message(Some(2), timestamp, Some("USD")), false).is_none()
        );
        let pre = timestamp - 3 * 3_600_000;
        assert!(parse_tick_with_session(&message(None, pre, None), false).is_none());
        let tick = parse_tick_with_session(&message(None, pre, None), true).unwrap();
        assert_eq!(tick.quote.timestamp_ms, Some(pre));
        assert_eq!(tick.quote.volume_24h, None);
        assert!(parse_tick(r#"{"message":"CgQ="}"#).is_none());
        assert!(parse_tick(r#"{"message":"/////w=="}"#).is_none());
        assert!(parse_tick("{}").is_none());
        assert!(parse_tick(&"x".repeat(MAX_MESSAGE + 1)).is_none());
    }

    #[test]
    fn subscription_is_one_json_message() {
        let session = session(&["NVDA".into(), "SPY".into()]);
        assert_eq!(session.url, "wss://streamer.finance.yahoo.com/?version=2");
        assert_eq!(
            serde_json::from_str::<Value>(&session.subscribe[0]).unwrap(),
            json!({"subscribe":["NVDA","SPY"]})
        );
    }

    // This intentionally uses one direct connection, not the reconnecting
    // worker: distinguish a handshake error, a quiet feed and rejected messages.
    fn probe(extended_only: bool) {
        use crate::model::market::yahoo_session::{Session as TradingSession, session_at};
        use crate::service::net::ws::{Health, connect};
        use std::time::Instant;
        use tungstenite::Message;
        let now = crate::clock::now_ms();
        let wanted = |ts| {
            if extended_only {
                matches!(
                    session_at(ts),
                    Some(TradingSession::Pre | TradingSession::Post)
                )
            } else {
                session_at(ts) == Some(TradingSession::Regular)
            }
        };
        assert!(
            wanted(now),
            "Wrong US session for this test: {:?}. Use the matching regular/extended test.",
            session_at(now)
        );
        let session = session(&["NVDA".into(), "SPY".into(), "TSLA".into()]);
        let (mut socket, route) = connect(&session.url, &session.host, session.port)
            .expect("Yahoo WebSocket handshake failed");
        println!("Yahoo WebSocket handshake OK");
        for message in &session.subscribe {
            socket
                .send(Message::Text(message.as_str().into()))
                .expect("Yahoo subscribe failed");
        }
        println!("Subscribed to NVDA, SPY, TSLA");
        let mut health = Health::new(session.max_data_silence, session.keepalive, route);
        let started = Instant::now();
        let mut frames = 0;
        let mut decoded = 0;
        while started.elapsed() < Duration::from_secs(90) {
            let frame = health
                .read(&mut socket)
                .expect("Yahoo WebSocket read failed");
            let text = match &frame {
                Some(Message::Text(text)) => text.as_str(),
                Some(Message::Binary(bytes)) => match std::str::from_utf8(bytes) {
                    Ok(text) => text,
                    Err(_) => {
                        frames += 1;
                        if frames <= 8 {
                            println!("Non-UTF8 binary frame: {} bytes", bytes.len());
                        }
                        continue;
                    }
                },
                Some(Message::Close(reason)) => panic!("Yahoo closed socket: {reason:?}"),
                _ => continue,
            };
            frames += 1;
            if let Some(data) = payload(text)
                && let Some(pricing) = decode(&data)
            {
                decoded += 1;
                if frames <= 8 {
                    println!(
                        "Decoded PricingData: {pricing:?}; timestamp session: {:?}",
                        pricing.timestamp.and_then(session_at)
                    );
                }
            } else if frames <= 8 {
                println!(
                    "Unrecognized message ({} bytes); JSON fields: {:?}",
                    text.len(),
                    serde_json::from_str::<Value>(text).ok().and_then(|v| v
                        .as_object()
                        .map(|obj| obj.keys().cloned().collect::<Vec<_>>()))
                );
            }
            if let Some(tick) = parse_tick_with_session(text, extended_only)
                && tick.quote.timestamp_ms.is_some_and(wanted)
                && ["NVDA", "SPY", "TSLA"].contains(&tick.product_id.as_str())
            {
                println!(
                    "Accepted {}: {} at {:?}",
                    tick.product_id, tick.quote.price, tick.quote.timestamp_ms
                );
                return;
            }
        }
        panic!(
            "Yahoo sent {frames} frames ({decoded} decoded PricingData) but no accepted session tick in 90s"
        );
    }

    #[test]
    #[ignore = "needs network and US premarket/after-hours activity"]
    fn live_yahoo_receives_extended_session_tick() {
        probe(true);
    }

    #[test]
    #[ignore = "needs network and an open US market"]
    fn live_yahoo_receives_regular_session_tick() {
        probe(false);
    }
}
