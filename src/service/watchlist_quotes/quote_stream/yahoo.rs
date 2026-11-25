//! Yahoo PricingData envelope: JSON carrying a base64 protobuf message.
//! Protocol reference: Wickline server.mjs (MIT, docs/yahoo-finance.md).
//! Only regular-session USD equity/ETF prices are admitted here. A REST
//! snapshot must first validate the instrument in the quote engine.
use super::StreamTick;
use crate::model::{DataSource, encoding};
use crate::model::watchlist::quote::{SourceQuote, VolumeKind};
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

#[derive(Default)]
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
            1 => { take(&mut input, 8)?; }
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
    if text.len() > MAX_MESSAGE {
        return None;
    }
    let envelope: Value = serde_json::from_str(text).ok()?;
    let message = envelope.get("message")?.as_str()?;
    let data = encoding::base64_decode(message)?;
    let pricing = decode(&data)?;
    let symbol = pricing.symbol?;
    let price = pricing.price.filter(|price| price.is_finite() && *price > 0.0)?;
    let timestamp = pricing.timestamp.filter(|timestamp| *timestamp > 0)?;
    if !DataSource::Yahoo.yahoo_stream_symbol(symbol) || pricing.currency != Some("USD")
        || pricing.session != Some(1)
    {
        return None;
    }
    let volume = pricing.volume.filter(|volume| *volume >= 0).map(|volume| volume as f64);
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

    fn message(session: u8) -> String {
        let mut bytes = vec![10, 4, b'N', b'V', b'D', b'A', 21];
        bytes.extend_from_slice(&123.5f32.to_le_bytes());
        bytes.extend_from_slice(&[24, 200, 1, 34, 3, b'U', b'S', b'D', 56, session, 72, 20]);
        json!({"message": encoding::base64_encode(&bytes)}).to_string()
    }

    #[test]
    fn pricing_data_requires_explicit_currency_session_and_vendor_timestamp() {
        let tick = parse_tick(&message(1)).unwrap();
        assert_eq!(tick.product_id, "NVDA");
        assert_eq!(tick.quote.price, 123.5);
        assert_eq!(tick.quote.timestamp_ms, Some(100));
        assert_eq!(tick.quote.volume_24h, Some(10.0));
        assert!(parse_tick(&message(0)).is_none());
        assert!(parse_tick(&message(2)).is_none());
        assert!(parse_tick(r#"{"message":"CgQ="}"#).is_none());
        assert!(parse_tick(r#"{"message":"/////w=="}"#).is_none());
        assert!(parse_tick("{}").is_none());
        assert!(parse_tick(&"x".repeat(MAX_MESSAGE + 1)).is_none());
    }

    #[test]
    fn subscription_is_one_json_message() {
        let session = session(&["NVDA".into(), "SPY".into()]);
        assert_eq!(session.url, "wss://streamer.finance.yahoo.com/?version=2");
        assert_eq!(serde_json::from_str::<Value>(&session.subscribe[0]).unwrap(), json!({"subscribe":["NVDA","SPY"]}));
    }

    #[test]
    #[ignore = "needs network and an open US market"]
    fn live_yahoo_receives_regular_session_tick() {
        let (sender, receiver) = std::sync::mpsc::channel();
        let _socket = super::super::QuoteSocket::connect(DataSource::Yahoo, vec!["NVDA".into(), "SPY".into()], sender).unwrap();
        let tick = receiver.recv_timeout(Duration::from_secs(60)).expect("Yahoo regular-session tick");
        assert!(["NVDA", "SPY"].contains(&tick.product_id.as_str()));
        assert!(tick.quote.timestamp_ms.is_some());
    }
}
