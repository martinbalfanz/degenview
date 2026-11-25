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
