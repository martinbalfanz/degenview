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

#[cfg(test)]
#[test]
fn yahoo_render_copy_keeps_volume_and_time() {
    let rest = crate::model::Kline { open_time: 1, close_time: 60_000, open: 100.0, high: 103.0, low: 99.0, close: 102.0, volume: 500.0 };
    let mut copy = rest.clone();
    Preview { open_time: 1, open: 100.0, high: 105.0, low: 99.0, close: 105.0 }.apply_to(&mut copy);
    assert_eq!((copy.high, copy.close), (105.0, 105.0));
    assert_eq!((copy.open_time, copy.close_time, copy.volume), (rest.open_time, rest.close_time, rest.volume));
    assert_eq!(rest.close, 102.0);
}
