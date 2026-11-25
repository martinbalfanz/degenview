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
