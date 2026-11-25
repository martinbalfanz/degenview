//! The live candle feeds: one per streamed source on the workspace, folding its candles into the
//! charts.

use crate::model::market::live::LiveMark;
use crate::model::{DataSource, InstrumentId, Kline, Timeframe};
use crate::service::market::live::LiveFeed;
use crate::workspace::ChartWorkspace;
use eframe::egui;
use std::time::Instant;

/// Separate leased consumer: a chart needs quotes even with the sidebar hidden.
pub(in crate::workspace) fn yahoo_chart_quote_owner(workspace_id: u64) -> u64 {
    (1 << 59) + workspace_id
}

pub(in crate::workspace) mod provisional;
use provisional::Observation;

fn indicative_yahoo_price(quote: &crate::model::watchlist::quote::WatchlistQuote, last: &Kline, now: i64, extended: bool) -> Option<f64> {
    use crate::model::watchlist::quote::Freshness;
    use crate::model::market::yahoo_session::{Session, session_at};
    let timestamp = quote.timestamp_ms?;
    if quote.freshness != Freshness::Delayed || timestamp < last.open_time
        || timestamp > now.saturating_add(60_000)
        || now.saturating_sub(timestamp) > 30 * 60_000
        || session_at(timestamp).is_none()
        || session_at(timestamp) != session_at(now)
        || (!extended && session_at(now) != Some(Session::Regular))
    {
        return None;
    }
    quote.last.filter(|price| price.is_finite() && *price > 0.0)
}


impl ChartWorkspace {
    pub(in crate::workspace) fn update_live_feeds(&mut self, ctx: &egui::Context) {
        for source in DataSource::CANDLE_STREAMS {
            self.update_live_feed(source, ctx);
        }
        self.update_yahoo_chart_quotes();
    }

    fn update_yahoo_chart_quotes(&mut self) {
        let active = !self.replay.is_engaged();
        let demand: Vec<_> = self.charts.iter()
            .filter(|chart| active && chart.is_market() && chart.source.yahoo_stream_symbol(&chart.symbol))
            .map(|chart| InstrumentId::new(chart.source, &chart.symbol))
            .collect();
        self.quote_coordinator.set_sidebar_demand(yahoo_chart_quote_owner(self.id), demand, active);

        let now = crate::clock::now_ms();
        for chart in &mut self.charts {
            let observation = if active && chart.is_market() && chart.source.yahoo_stream_symbol(&chart.symbol) {
                chart.candles.last().and_then(|last| {
                    let instrument = InstrumentId::new(chart.source, &chart.symbol);
                    let extended = crate::model::market::yahoo_session::extended_candles(chart.source, &chart.symbol, self.timeframe);
                    let quote = if extended && matches!(crate::model::market::yahoo_session::session_at(now), Some(crate::model::market::yahoo_session::Session::Pre | crate::model::market::yahoo_session::Session::Post)) {
                        self.quote_book.get_extended(&instrument)?
                    } else {
                        self.quote_book.get(&instrument)?
                    };
                    let price = indicative_yahoo_price(&quote, last, now, extended)?;
                    Some(Observation { price, timestamp_ms: quote.timestamp_ms? })
                })
            } else {
                None
            };
            // Preview only; never insert an observation into Chart::candles.
            let preview = chart.candles.last().and_then(|last| {
                chart.yahoo_preview.observe(self.timeframe, last, observation)
            });
            if !active {
                chart.yahoo_preview.clear();
            }
            // Do not draw a stale same-bucket price marker after REST has
            // superseded its preview. Other buckets can still show a quote.
            let marker = observation.filter(|quote| {
                preview.is_some() || chart.candles.last().is_some_and(|last| {
                    !self.timeframe.is_intraday()
                        || crate::model::market::bucket::bucket_start_ms(self.timeframe, quote.timestamp_ms) != last.open_time
                })
            });
            chart.chart.set_yahoo_quote(marker.map(|quote| quote.price));
            chart.chart.set_yahoo_preview(preview);
        }
    }

    fn update_live_feed(&mut self, source: DataSource, ctx: &egui::Context) {
        let mut symbols: Vec<String> = self
            .charts
            .iter()
            .filter(|chart| chart.is_market() && chart.source == source)
            .map(|chart| InstrumentId::new(source, chart.symbol.as_str()).api_symbol())
            .collect();
        // The markets compared on a chart stream too, once however many overlays name them.
        for symbol in self.overlay_hub.streamed_symbols(source) {
            if !symbols.contains(&symbol) {
                symbols.push(symbol);
            }
        }
        // The stream is of the candle length the source serves the timeframe from (one-minute
        // klines for a 22-minute chart, monthly ones for a quarter); a timeframe it cannot reach
        // has none.
        let base = source
            .plan(self.timeframe)
            .map(|plan| plan.base())
            .filter(|_| !symbols.is_empty());
        let wanted = base.map(|base| (symbols, base));
        let changed = match (self.live_feeds.get(&source), &wanted) {
            (Some(feed), Some(wanted)) => feed.signature() != wanted,
            (None, None) => false,
            _ => true,
        };
        if changed {
            let feed = wanted
                .and_then(|(symbols, base)| LiveFeed::connect(source, symbols, base, ctx.clone()));
            match feed {
                Some(feed) => {
                    self.live_feeds.insert(source, feed);
                }
                None => {
                    self.live_feeds.remove(&source);
                }
            }
        }

        let updates = self
            .live_feeds
            .get(&source)
            .map(|feed| feed.inbox.drain())
            .unwrap_or_default();
        let Some(base) = base else {
            return;
        };
        let now = Instant::now();
        for (symbol, kline) in updates {
            self.overlay_hub
                .apply_live(source, self.timeframe, base, &symbol, &kline, now);
            self.apply_live_kline(source, base, &symbol, kline, now);
        }
    }

    /// Every chart of `symbol` on `source` takes the kline of `base` candles, whatever else it
    /// shows. A chart's symbol may be spelled any way the source takes (`BTC`, `btc/usdt`): the
    /// feed names the market as the source does, so they are compared by `api_symbol`.
    fn apply_live_kline(
        &mut self,
        source: DataSource,
        base: Timeframe,
        symbol: &str,
        kline: Kline,
        now: Instant,
    ) {
        let timeframe = self.timeframe;
        for chart in self.charts.iter_mut().filter(|chart| {
            chart.source == source
                && InstrumentId::new(source, chart.symbol.as_str())
                    .api_symbol()
                    .eq_ignore_ascii_case(symbol)
        }) {
            if let Some(open_time) =
                chart
                    .live_fold
                    .apply(timeframe, base, &mut chart.candles, kline.clone())
            {
                chart.live = Some(LiveMark::new(open_time, now));
                chart.last_data_ms = Some(crate::clock::now_ms());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::chart::Chart;
    use crate::workspace::test_support::{candle, empty_workspace};

    #[test]
    fn every_chart_of_the_symbol_takes_the_live_bar() {
        let mut workspace = empty_workspace(1);
        for id in 1..=2 {
            let mut chart = Chart::blank(id, "BTCUSDT", DataSource::Binance);
            chart.candles = vec![candle(1, 1.0)];
            workspace.charts.push(chart);
        }
        let mut other = Chart::blank(3, "ETHUSDT", DataSource::Binance);
        other.candles = vec![candle(1, 1.0)];
        workspace.charts.push(other);

        let base = workspace.timeframe;
        workspace.apply_live_kline(
            DataSource::Binance,
            base,
            "btcusdt",
            candle(2, 5.0),
            Instant::now(),
        );

        let lens: Vec<usize> = workspace.charts.iter().map(|c| c.candles.len()).collect();
        assert_eq!(lens, [2, 2, 1]);
        assert!(workspace.charts[0].live.is_some() && workspace.charts[1].live.is_some());
    }

    #[test]
    fn a_kraken_chart_takes_the_live_bar_whatever_way_its_symbol_is_spelled() {
        let mut workspace = empty_workspace(1);
        for (id, symbol) in [(1, "xbt/usd"), (2, "BTC-USD"), (3, "BTC")] {
            let mut chart = Chart::blank(id, symbol, DataSource::Kraken);
            chart.candles = vec![candle(1, 1.0)];
            workspace.charts.push(chart);
        }
        let mut other = Chart::blank(4, "ETH/USD", DataSource::Kraken);
        other.candles = vec![candle(1, 1.0)];
        workspace.charts.push(other);

        let base = workspace.timeframe;
        workspace.apply_live_kline(
            DataSource::Kraken,
            base,
            "BTC/USD",
            candle(2, 5.0),
            Instant::now(),
        );

        let lens: Vec<usize> = workspace.charts.iter().map(|c| c.candles.len()).collect();
        assert_eq!(lens, [2, 2, 2, 1]);
    }

    #[test]
    fn an_okx_chart_takes_the_live_bar_whatever_way_its_symbol_is_spelled() {
        let mut workspace = empty_workspace(1);
        for (id, symbol) in [(1, "btc/usdt"), (2, "BTC-USDT"), (3, "BTC")] {
            let mut chart = Chart::blank(id, symbol, DataSource::Okx);
            chart.candles = vec![candle(1, 1.0)];
            workspace.charts.push(chart);
        }
        let mut other = Chart::blank(4, "ETH-USDT", DataSource::Okx);
        other.candles = vec![candle(1, 1.0)];
        workspace.charts.push(other);

        let base = workspace.timeframe;
        workspace.apply_live_kline(
            DataSource::Okx,
            base,
            "BTC-USDT",
            candle(2, 5.0),
            Instant::now(),
        );

        let lens: Vec<usize> = workspace.charts.iter().map(|c| c.candles.len()).collect();
        assert_eq!(lens, [2, 2, 2, 1]);
    }

    #[test]
    fn a_bybit_chart_takes_the_live_bar_whatever_way_its_symbol_is_spelled() {
        let mut workspace = empty_workspace(1);
        for (id, symbol) in [(1, "btc/usdt"), (2, "BTCUSDT"), (3, "BTC")] {
            let mut chart = Chart::blank(id, symbol, DataSource::Bybit);
            chart.candles = vec![candle(1, 1.0)];
            workspace.charts.push(chart);
        }
        let mut other = Chart::blank(4, "ETHUSDT", DataSource::Bybit);
        other.candles = vec![candle(1, 1.0)];
        workspace.charts.push(other);

        let base = workspace.timeframe;
        workspace.apply_live_kline(
            DataSource::Bybit,
            base,
            "BTCUSDT",
            candle(2, 5.0),
            Instant::now(),
        );

        let lens: Vec<usize> = workspace.charts.iter().map(|c| c.candles.len()).collect();
        assert_eq!(lens, [2, 2, 2, 1]);
    }

    #[test]
    fn a_gate_chart_takes_the_live_bar_whatever_way_its_symbol_is_spelled() {
        let mut workspace = empty_workspace(1);
        for (id, symbol) in [(1, "btc/usdt"), (2, "BTC_USDT"), (3, "BTC")] {
            let mut chart = Chart::blank(id, symbol, DataSource::Gate);
            chart.candles = vec![candle(1, 1.0)];
            workspace.charts.push(chart);
        }
        let mut other = Chart::blank(4, "ETH_USDT", DataSource::Gate);
        other.candles = vec![candle(1, 1.0)];
        workspace.charts.push(other);

        let base = workspace.timeframe;
        workspace.apply_live_kline(
            DataSource::Gate,
            base,
            "BTC_USDT",
            candle(2, 5.0),
            Instant::now(),
        );

        let lens: Vec<usize> = workspace.charts.iter().map(|c| c.candles.len()).collect();
        assert_eq!(lens, [2, 2, 2, 1]);
    }

    #[test]
    fn a_mexc_chart_takes_the_live_bar_whatever_way_its_symbol_is_spelled() {
        let mut workspace = empty_workspace(1);
        for (id, symbol) in [(1, "btc/usdt"), (2, "BTCUSDT"), (3, "BTC")] {
            let mut chart = Chart::blank(id, symbol, DataSource::Mexc);
            chart.candles = vec![candle(1, 1.0)];
            workspace.charts.push(chart);
        }
        let mut other = Chart::blank(4, "ETHUSDT", DataSource::Mexc);
        other.candles = vec![candle(1, 1.0)];
        workspace.charts.push(other);

        let base = workspace.timeframe;
        workspace.apply_live_kline(
            DataSource::Mexc,
            base,
            "BTCUSDT",
            candle(2, 5.0),
            Instant::now(),
        );

        let lens: Vec<usize> = workspace.charts.iter().map(|c| c.candles.len()).collect();
        assert_eq!(lens, [2, 2, 2, 1]);
    }

    #[test]
    fn a_late_tick_of_an_older_bar_does_not_unsort_the_chart() {
        let mut workspace = empty_workspace(1);
        let mut chart = Chart::blank(1, "BTCUSDT", DataSource::Binance);
        chart.candles = vec![candle(1, 1.0), candle(2, 2.0)];
        workspace.charts.push(chart);

        let base = workspace.timeframe;
        workspace.apply_live_kline(
            DataSource::Binance,
            base,
            "BTCUSDT",
            candle(1, 9.0),
            Instant::now(),
        );

        assert_eq!(
            workspace.charts[0].candles,
            [candle(1, 1.0), candle(2, 2.0)]
        );
        assert!(workspace.charts[0].live.is_none());
    }

    #[test]
    fn every_chart_of_the_symbol_folds_the_base_klines_into_its_own_bucket() {
        let minute = 60_000;
        let mut workspace = empty_workspace(1);
        workspace.timeframe = Timeframe::parse("5m").unwrap();
        for id in 1..=2 {
            workspace
                .charts
                .push(Chart::blank(id, "BTCUSDT", DataSource::Binance));
        }
        let live = |open: i64, volume: f64| Kline {
            volume,
            ..candle(open, 5.0)
        };

        for kline in [live(0, 1.0), live(0, 2.0), live(minute, 3.0)] {
            workspace.apply_live_kline(
                DataSource::Binance,
                Timeframe::MINUTE_1,
                "BTCUSDT",
                kline,
                Instant::now(),
            );
        }

        for chart in &workspace.charts {
            assert_eq!(chart.candles.len(), 1);
            assert_eq!(
                chart.candles[0].volume, 5.0,
                "each bar once, each chart its own"
            );
            assert_eq!(
                chart.live.map(|mark| mark.guard_for(Instant::now())),
                Some(None)
            );
        }
    }

    #[test]
    fn a_refresh_that_replaces_the_forming_bucket_makes_the_next_live_kline_start_from_it() {
        let mut workspace = empty_workspace(1);
        workspace.timeframe = Timeframe::parse("5m").unwrap();
        workspace
            .charts
            .push(Chart::blank(1, "BTCUSDT", DataSource::Binance));
        let mut kline = candle(0, 5.0);
        kline.volume = 4.0;
        workspace.apply_live_kline(
            DataSource::Binance,
            Timeframe::MINUTE_1,
            "BTCUSDT",
            kline.clone(),
            Instant::now(),
        );

        // A refresh with no live guard replaces the candle: the socket's account is dropped.
        let chart = &mut workspace.charts[0];
        chart.live = None;
        let (sender, receiver) = std::sync::mpsc::channel();
        let fetched = Kline {
            volume: 10.0,
            close_time: 5 * 60_000 - 1,
            ..candle(0, 5.0)
        };
        sender
            .send(Ok(crate::workspace::chart::Fetched {
                candles: vec![fetched],
                bars: Vec::new(),
                series: Vec::new(),
            }))
            .unwrap();
        chart.inbox = Some(receiver);
        chart.receive_fetch(workspace.timeframe);

        // The bar already counted in the refresh's volume adds only what it trades from here.
        kline.volume = 4.5;
        workspace.apply_live_kline(
            DataSource::Binance,
            Timeframe::MINUTE_1,
            "BTCUSDT",
            kline.clone(),
            Instant::now(),
        );
        assert_eq!(workspace.charts[0].candles[0].volume, 10.0);
        kline.volume = 5.0;
        workspace.apply_live_kline(
            DataSource::Binance,
            Timeframe::MINUTE_1,
            "BTCUSDT",
            kline,
            Instant::now(),
        );
        assert_eq!(workspace.charts[0].candles[0].volume, 10.5);
    }

    #[test]
    fn an_answer_with_the_buckets_base_bars_gives_the_forming_candle_what_the_socket_missed() {
        let mut workspace = empty_workspace(1);
        workspace.timeframe = Timeframe::parse("5m").unwrap();
        let mut chart = Chart::blank(1, "BTCUSDT", DataSource::Binance);
        // The socket was first: it saw only the bar at minute 3, 8 traded.
        chart.last_fetch = Instant::now();
        workspace.charts.push(chart);
        let mut kline = candle(3 * 60_000, 5.0);
        kline.volume = 8.0;
        workspace.apply_live_kline(
            DataSource::Binance,
            Timeframe::MINUTE_1,
            "BTCUSDT",
            kline,
            Instant::now(),
        );

        // REST answers later with the bucket's first four minutes, the last one cut at 3.
        let bars: Vec<Kline> = (0..4)
            .map(|minute| {
                let mut bar = candle(minute * 60_000, 4.0);
                bar.volume = if minute == 3 { 3.0 } else { 2.0 };
                bar
            })
            .collect();
        let chart = &mut workspace.charts[0];
        let (sender, receiver) = std::sync::mpsc::channel();
        sender
            .send(Ok(crate::workspace::chart::Fetched {
                candles: vec![Kline {
                    volume: 9.0,
                    close_time: 5 * 60_000 - 1,
                    ..candle(0, 4.0)
                }],
                bars,
                series: Vec::new(),
            }))
            .unwrap();
        chart.inbox = Some(receiver);
        chart.live = Some(LiveMark::new(0, Instant::now()));
        chart.receive_fetch(workspace.timeframe);

        assert_eq!(workspace.charts[0].candles.len(), 1);
        assert_eq!(
            workspace.charts[0].candles[0].volume, 14.0,
            "three minutes of 2 and the socket's 8"
        );
    }
}

#[cfg(test)]
mod yahoo_chart_tests {
    use super::*;
    use crate::model::watchlist::quote::{Freshness, WatchlistQuote};
    use crate::workspace::test_support::candle;

    #[test]
    fn indicative_price_requires_a_recent_regular_session_quote_and_never_changes_candle() {
        let now = crate::pine::time::calendar::make(2026, 11, 25, 15, 0, 0, 0).unwrap();
        let bar = candle(now - 60_000, 100.0);
        let quote = WatchlistQuote {
            last: Some(101.0), timestamp_ms: Some(now - 1_000), freshness: Freshness::Delayed,
            ..WatchlistQuote::default()
        };
        assert_eq!(indicative_yahoo_price(&quote, &bar, now, false), Some(101.0));
        assert_eq!(bar.close, 100.0);
        assert!(indicative_yahoo_price(&quote.with_freshness(Freshness::Stale), &bar, now, false).is_none());
        let old = WatchlistQuote { timestamp_ms: Some(now - 120_000), ..quote.clone() };
        assert!(indicative_yahoo_price(&old, &bar, now, false).is_none());
        assert!(indicative_yahoo_price(&quote, &bar, now + 31 * 60_000, false).is_none());
    }
}
