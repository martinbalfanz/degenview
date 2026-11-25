//! The live chart's layers under the crosshair: the grid, the series, volume, the built-in
//! indicators, trend-flip markers, the last price and the time axis.

use super::*;

impl ChartCard {
    /// Shade only the portions of visible actual bar slots that lie in the
    /// extended session. A mixed hourly bar is split at 09:30 or 16:00 NY;
    /// the compressed time axis does not acquire fictional overnight slots.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw_yahoo_session_background(
        &self,
        painter: &egui::Painter,
        plot: Rect,
        shown: &[Kline],
        slot_width: f32,
        lead: f32,
        tail: Option<crate::model::market::yahoo_preview::Preview>,
    ) {
        use crate::model::market::yahoo_session::{Session, extended_spans};
        if !slot_width.is_finite() || slot_width <= 0.0 || !lead.is_finite() {
            return;
        }
        let pre = theme::pick(
            Color32::from_rgba_unmultiplied(220, 115, 25, 22),
            Color32::from_rgba_unmultiplied(220, 115, 25, 18),
        );
        let post = theme::pick(
            Color32::from_rgba_unmultiplied(150, 75, 225, 28),
            Color32::from_rgba_unmultiplied(150, 75, 225, 18),
        );
        let clipped = painter.with_clip_rect(plot);
        let intervals = shown
            .iter()
            .map(|bar| (bar.open_time, bar.close_time.saturating_add(1)))
            .chain(tail.map(|bar| (bar.open_time, bar.end_time)));
        // Merge adjacent same-session spans to avoid seams and unnecessary shapes.
        let mut pending: Option<(f32, f32, Color32)> = None;
        let paint = |left, right, color| {
            let rect =
                Rect::from_min_max(Pos2::new(left, plot.top()), Pos2::new(right, plot.bottom()))
                    .intersect(plot);
            if rect.is_positive() {
                clipped.rect_filled(rect, 0.0, color);
            }
        };
        for (index, (start, end)) in intervals.enumerate() {
            for (session, from, to) in extended_spans(start, end) {
                let color = match session {
                    Session::Pre => pre,
                    Session::Post => post,
                    Session::Regular => continue,
                };
                let left = plot.left() + slot_width * (index as f32 + lead + from);
                let right = plot.left() + slot_width * (index as f32 + lead + to);
                if let Some((_, previous_right, previous_color)) = pending.as_mut()
                    && *previous_color == color
                    && (left - *previous_right).abs() < 0.01
                {
                    *previous_right = right;
                } else {
                    if let Some((left, right, color)) = pending.take() {
                        paint(left, right, color);
                    }
                    pending = Some((left, right, color));
                }
            }
        }
        if let Some((left, right, color)) = pending {
            paint(left, right, color);
        }
    }

    /// The price labels and the grid lines behind the series. One tick list serves both, so a
    /// line is always on its label; the vertical lines stand on the time axis' own ticks. The
    /// grid is drawn from the scale and never feeds back into it.
    pub(super) fn draw_grid(
        &self,
        painter: &egui::Painter,
        plot: Rect,
        rect: Rect,
        scale: &PriceScale,
        style: &CanvasStyle,
        tick_xs: &[f32],
    ) {
        let budget = chart::canvas::tick_budget(plot.height(), style.price_tick_spacing());
        let AxisTicks { prices, step } = scale.axis_ticks(budget);
        let half = style.tag_half_height();
        for price in prices {
            let y = scale.y(price);
            if !y.is_finite() || y < plot.top() || y > plot.bottom() {
                continue;
            }
            if let Some(grid) = style.horizontal_grid {
                canvas::horizontal(painter, (plot.left(), plot.right()), y, grid);
            }
            // A label that would hang over the plot's edge waits for the scale to move.
            if y - half >= plot.top() - half && y + half <= plot.bottom() + half {
                price_text::paint(
                    painter,
                    Pos2::new(rect.right() - 7.0, y),
                    Align2::RIGHT_CENTER,
                    &scale.tick_label(self.appearance.price_format(), price, step),
                    style.price_font(),
                    style.text_color,
                );
            }
        }
        if let Some(grid) = style.vertical_grid {
            for x in tick_xs {
                if *x >= plot.left() && *x <= plot.right() {
                    canvas::vertical(painter, *x, (plot.top(), plot.bottom()), grid);
                }
            }
        }
    }

    pub(super) fn draw_line(
        &self,
        painter: &egui::Painter,
        candles: &[Kline],
        x_for: &dyn Fn(usize) -> f32,
        scale: &PriceScale,
    ) {
        if candles.len() < 2 {
            if let Some(candle) = candles.first() {
                painter.circle_filled(
                    Pos2::new(x_for(0), scale.y(candle.close)),
                    2.0,
                    self.bullish_color(),
                );
            }
            return;
        }
        let points: Vec<Pos2> = candles
            .iter()
            .enumerate()
            .map(|(index, candle)| Pos2::new(x_for(index), scale.y(candle.close)))
            .collect();
        for pair in points.windows(2) {
            let color = if pair[1].y <= pair[0].y {
                self.bullish_color()
            } else {
                self.bearish_color()
            };
            painter.line_segment([pair[0], pair[1]], Stroke::new(1.5, color));
        }
    }

    /// Each choice as a plain line, labelled at its right end.
    pub(super) fn draw_series(
        &self,
        painter: &egui::Painter,
        plot: Rect,
        shown: &[Kline],
        x_for: &dyn Fn(usize) -> f32,
        scale: &PriceScale,
    ) {
        let (Some(first), Some(last)) = (shown.first(), shown.last()) else {
            return;
        };
        let mut ends: Vec<(f32, &SeriesLine)> = Vec::new();
        for line in &self.series {
            let mut previous: Option<Pos2> = None;
            for candle in self
                .series_points(line)
                .filter(|candle| (first.open_time..=last.open_time).contains(&candle.open_time))
            {
                let index = shown
                    .partition_point(|shown| shown.open_time < candle.open_time)
                    .min(shown.len() - 1);
                let point = Pos2::new(x_for(index), scale.y(candle.close));
                if let Some(previous) = previous {
                    painter.line_segment([previous, point], Stroke::new(1.5, line.color));
                }
                previous = Some(point);
            }
            if let Some(end) = previous {
                ends.push((end.y, line));
            }
        }
        // Top to bottom, pushed apart so neighbouring labels do not stack, and kept in the plot.
        ends.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut top = f32::MIN;
        for (y, line) in ends {
            let y = y
                .max(top + SERIES_LABEL_GAP)
                .clamp(plot.top() + 6.0, plot.bottom() - 6.0);
            top = y;
            let label = painter.layout_no_wrap(
                truncated(&line.label, SERIES_LABEL_CHARS),
                FontId::proportional(9.0),
                theme::text_primary(),
            );
            let text = Rect::from_min_size(
                Pos2::new(
                    plot.right() - 4.0 - label.size().x,
                    y - label.size().y / 2.0,
                ),
                label.size(),
            );
            painter.rect_filled(
                text.expand2(Vec2::new(11.0, 2.0))
                    .translate(Vec2::new(-4.0, 0.0)),
                4.0,
                theme::with_alpha(theme::surface(), 0.85),
            );
            painter.circle_filled(Pos2::new(text.left() - 6.0, y), 3.0, line.color);
            painter.galley(text.min, label, theme::text_primary());
        }
    }

    pub(super) fn draw_volume(
        &self,
        painter: &egui::Painter,
        plot: Rect,
        x_for: &dyn Fn(usize) -> f32,
        slot_width: f32,
        candles: &[Kline],
    ) {
        let peak = candles
            .iter()
            .map(|candle| candle.volume)
            .fold(0.0, f64::max);
        if peak <= 0.0 {
            return;
        }
        let pane_height = plot.height() * 0.22;
        let pane_bottom = plot.bottom();
        let bar_width = (slot_width * 0.6).clamp(1.0, 8.0);
        for (index, candle) in candles.iter().enumerate() {
            if candle.volume <= 0.0 {
                continue;
            }
            let height = (pane_height * (candle.volume / peak) as f32).max(1.0);
            let x = x_for(index);
            let color = if candle.is_up() {
                self.bullish_color()
            } else {
                self.bearish_color()
            };
            let bar = Rect::from_min_max(
                Pos2::new(x - bar_width / 2.0, pane_bottom - height),
                Pos2::new(x + bar_width / 2.0, pane_bottom),
            );
            painter.rect_filled(bar, 0.0, theme::with_alpha_byte(color, 89));
        }
    }

    pub(super) fn draw_ema(
        &self,
        painter: &egui::Painter,
        x_for: &dyn Fn(usize) -> f32,
        scale: &PriceScale,
        values: &[Option<f64>],
    ) {
        let stroke = Stroke::new(1.5, theme::EMA);
        let mut previous: Option<Pos2> = None;
        for (index, value) in values.iter().enumerate() {
            let Some(value) = value else { continue };
            let point = Pos2::new(x_for(index), scale.y(*value));
            if let Some(previous) = previous {
                painter.line_segment([previous, point], stroke);
            }
            previous = Some(point);
        }
    }

    pub(super) fn draw_bollinger(
        &self,
        painter: &egui::Painter,
        x_for: &dyn Fn(usize) -> f32,
        scale: &PriceScale,
        bands: &BollingerSeries,
    ) {
        let mut upper_points = Vec::new();
        let mut lower_points = Vec::new();
        for index in 0..bands.upper.len() {
            if let (Some(upper), Some(lower)) = (bands.upper[index], bands.lower[index]) {
                let x = x_for(index);
                upper_points.push(Pos2::new(x, scale.y(upper)));
                lower_points.push(Pos2::new(x, scale.y(lower)));
            }
        }
        if upper_points.len() < 2 {
            return;
        }

        // Tint the channel as a strip of quads. One `PathShape` around the whole band is a
        // concave polygon, which egui fills as a fan from its first vertex, so it paints
        // spokes from the far-left point to every other vertex.
        let tint = theme::with_alpha_byte(theme::BOLLINGER, 26);
        let mut channel = egui::epaint::Mesh::default();
        for (upper, lower) in upper_points.iter().zip(&lower_points) {
            channel.colored_vertex(*upper, tint);
            channel.colored_vertex(*lower, tint);
        }
        for column in 0..(upper_points.len() as u32 - 1) {
            let top = column * 2;
            channel.add_triangle(top, top + 1, top + 2);
            channel.add_triangle(top + 1, top + 3, top + 2);
        }
        painter.add(egui::Shape::mesh(channel));

        let rail = Stroke::new(1.0, theme::with_alpha_byte(theme::BOLLINGER, 180));
        for pair in upper_points.windows(2) {
            painter.line_segment([pair[0], pair[1]], rail);
        }
        for pair in lower_points.windows(2) {
            painter.line_segment([pair[0], pair[1]], rail);
        }

        let mut previous: Option<Pos2> = None;
        for (index, value) in bands.middle.iter().enumerate() {
            let Some(value) = value else { continue };
            let point = Pos2::new(x_for(index), scale.y(*value));
            if let Some(previous) = previous {
                painter.add(egui::Shape::dashed_line(
                    &[previous, point],
                    Stroke::new(1.0, theme::with_alpha_byte(theme::BOLLINGER, 115)),
                    4.0,
                    3.0,
                ));
            }
            previous = Some(point);
        }
    }

    /// Labelled up/down triangles at confirmed Supertrend regime changes. Each marker (triangle
    /// plus "Bullish"/"Bearish" pill) is clamped inside the plot so a flip at a price extreme
    /// stays readable.
    pub(super) fn draw_trend_flips(
        &self,
        painter: &egui::Painter,
        plot: Rect,
        x_for: &dyn Fn(usize) -> f32,
        scale: &PriceScale,
        candles: &[Kline],
        flips: &[Option<bool>],
    ) {
        let half = TREND_FLIP_MARKER_SIZE / 2.0;
        for (index, flip) in flips.iter().enumerate() {
            let Some(bullish) = *flip else { continue };
            let Some(candle) = candles.get(index) else {
                continue;
            };
            let x = x_for(index);
            let color = if bullish {
                self.bullish_color()
            } else {
                self.bearish_color()
            };
            let galley = painter.layout_no_wrap(
                if bullish {
                    tr!("Bullish")
                } else {
                    tr!("Bearish")
                }
                .to_owned(),
                FontId::proportional(TREND_FLIP_LABEL_FONT_SIZE),
                Color32::WHITE,
            );
            let label_size = galley.size() + TREND_FLIP_LABEL_PAD * 2.0;
            let group_height = TREND_FLIP_MARKER_SIZE + TREND_FLIP_LABEL_GAP + label_size.y;
            let price_y = scale.y(if bullish { candle.low } else { candle.high });
            let raw_center = if bullish {
                price_y + TREND_FLIP_MARKER_GAP + group_height / 2.0
            } else {
                price_y - TREND_FLIP_MARKER_GAP - group_height / 2.0
            };
            let (Some(group_center), Some(label_center_x)) = (
                contained_center(raw_center, plot.top(), plot.bottom(), group_height),
                contained_center(x, plot.left(), plot.right(), label_size.x),
            ) else {
                continue;
            };
            // The triangle sits on the candle's side of the group, the label beyond it.
            let side = if bullish { 1.0 } else { -1.0 };
            let triangle_y = group_center - side * (group_height - TREND_FLIP_MARKER_SIZE) / 2.0;
            let label_y = group_center + side * (group_height - label_size.y) / 2.0;
            let points = if bullish {
                vec![
                    Pos2::new(x, triangle_y - half),
                    Pos2::new(x - half, triangle_y + half),
                    Pos2::new(x + half, triangle_y + half),
                ]
            } else {
                vec![
                    Pos2::new(x, triangle_y + half),
                    Pos2::new(x - half, triangle_y - half),
                    Pos2::new(x + half, triangle_y - half),
                ]
            };
            painter.add(egui::Shape::convex_polygon(points, color, Stroke::NONE));

            let pill = Rect::from_center_size(Pos2::new(label_center_x, label_y), label_size);
            painter.rect_filled(pill, label_size.y / 2.0, color);
            painter.galley(pill.min + TREND_FLIP_LABEL_PAD, galley, Color32::WHITE);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw_last_price(
        &self,
        painter: &egui::Painter,
        plot: Rect,
        rect: Rect,
        scale: &PriceScale,
        candles: &[Kline],
        constructed_last: Option<&Kline>,
        style: &CanvasStyle,
    ) {
        let half = style.tag_half_height();
        // A price-based chart's own last close is not what the market last traded at: it gets its
        // own dashed line and an outlined tag, and the solid tag stays the market's.
        if let Some(bar) = constructed_last {
            let y = scale.y(bar.close);
            if plot.y_range().contains(y) {
                let tint = theme::text_secondary();
                painter.extend(Shape::dashed_line(
                    &[Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)],
                    Stroke::new(1.0, tint.gamma_multiply(0.7)),
                    3.0,
                    3.0,
                ));
                let tag = Rect::from_min_max(
                    Pos2::new(plot.right() + 3.0, y - half),
                    Pos2::new(rect.right() - 3.0, y + half),
                );
                painter.rect_stroke(tag, 3.0, Stroke::new(1.0, tint), StrokeKind::Inside);
                price_text::paint(
                    painter,
                    tag.center(),
                    Align2::CENTER_CENTER,
                    &scale.tag_label(self.appearance.price_format(), bar.close),
                    style.price_font(),
                    tint,
                );
            }
        }
        let Some(last) = candles.last() else {
            return;
        };
        let y = scale.y(last.close);
        if !plot.y_range().contains(y) {
            return;
        }
        let previous_close = candles
            .len()
            .checked_sub(2)
            .map(|before| candles[before].close);
        let color = self.bar_color(last, previous_close);
        // Dotted, so the line that follows the price reads apart from the grid and the drawings.
        canvas::horizontal(
            painter,
            (plot.left(), plot.right()),
            y,
            canvas::LineStyle {
                stroke: Stroke::new(1.0, color.gamma_multiply(0.65)),
                dash: crate::model::chart::kind::LineDash::Dotted,
            },
        );
        let tag = Rect::from_min_max(
            Pos2::new(plot.right() + 3.0, y - half),
            Pos2::new(rect.right() - 3.0, y + half),
        );
        painter.rect_filled(tag, 3.0, color);
        price_text::paint(
            painter,
            tag.center(),
            Align2::CENTER_CENTER,
            &scale.tag_label(self.appearance.price_format(), last.close),
            style.price_font(),
            Color32::WHITE,
        );
    }

    /// Regular and extended session prices are independent of the candle
    /// series. The label sits against the right axis, on its own dotted line.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw_yahoo_session_levels(
        &self,
        painter: &egui::Painter,
        plot: Rect,
        rect: Rect,
        scale: &PriceScale,
        style: &CanvasStyle,
        levels: crate::model::market::yahoo_preview::SessionLevels,
        fallback_y: Option<f32>,
    ) {
        use crate::model::market::yahoo_session::Session;
        let half = style.tag_half_height();
        if plot.height() < 2.0 * half || plot.width() <= 0.0 {
            return;
        }
        let regular = levels.regular.map(|quote| {
            let color = match quote.change {
                Some(change) if change > 0.0 => self.bullish_color(),
                Some(change) if change < 0.0 => self.bearish_color(),
                _ => theme::text_secondary(),
            };
            (
                quote.price,
                if quote.at_close {
                    crate::i18n::tr!("Close")
                } else {
                    crate::i18n::tr!("Regular")
                },
                color,
            )
        });
        let extended = levels.extended.map(|(session, price)| {
            let label = match session {
                Session::Pre => crate::i18n::tr!("Premarket"),
                Session::Post => crate::i18n::tr!("After hours"),
                Session::Regular => crate::i18n::tr!("Regular"),
            };
            (
                price,
                label,
                theme::pick(
                    Color32::from_rgb(148, 91, 221),
                    Color32::from_rgb(111, 61, 183),
                ),
            )
        });
        let mut occupied = fallback_y;
        for (price, label, color) in regular.into_iter().chain(extended) {
            let y = scale.y(price);
            if !y.is_finite() || !plot.y_range().contains(y) {
                continue;
            }
            let mut tag_y = y.clamp(plot.top() + half, plot.bottom() - half);
            if let Some(other) = occupied {
                // Reserve the first tag; when near it, move the second tag
                // rather than moving the price line itself.
                tag_y = yahoo_secondary_tag_y(tag_y, other, half, plot);
            }
            occupied = Some(tag_y);
            canvas::horizontal(
                painter,
                (plot.left(), plot.right()),
                y,
                canvas::LineStyle {
                    stroke: Stroke::new(1.0, color),
                    dash: crate::model::chart::kind::LineDash::Dotted,
                },
            );
            let tag = Rect::from_min_max(
                Pos2::new(plot.right() + 3.0, tag_y - half),
                Pos2::new(rect.right() - 3.0, tag_y + half),
            );
            painter.rect_filled(tag, 3.0, color);
            price_text::paint(
                painter,
                tag.center(),
                Align2::CENTER_CENTER,
                &scale.tag_label(self.appearance.price_format(), price),
                style.price_font(),
                Color32::WHITE,
            );
            let galley =
                painter.layout_no_wrap(label.to_owned(), style.price_font(), Color32::WHITE);
            let left = (plot.right() - galley.size().x - 10.0).max(plot.left());
            let label_rect = Rect::from_min_max(
                Pos2::new(left, tag_y - half),
                Pos2::new(plot.right(), tag_y + half),
            );
            if label_rect.width() >= galley.size().x {
                painter.rect_filled(label_rect, 3.0, color);
                painter.galley(
                    label_rect.center() - galley.size() / 2.0,
                    galley,
                    Color32::WHITE,
                );
            }
            if (tag_y - y).abs() > 0.5 {
                painter.line_segment(
                    [Pos2::new(plot.right(), y), Pos2::new(plot.right(), tag_y)],
                    Stroke::new(1.0, color),
                );
            }
        }
    }

    /// Subtle dashed border on the normal forming candle, indicating sampled prices.
    pub(super) fn draw_yahoo_preview_border(
        &self,
        painter: &egui::Painter,
        plot: Rect,
        scale: &PriceScale,
        x: f32,
        slot_width: f32,
        preview: crate::model::market::yahoo_preview::Preview,
    ) {
        let (o, h, l, c) = (
            scale.y(preview.open),
            scale.y(preview.high),
            scale.y(preview.low),
            scale.y(preview.close),
        );
        if ![o, h, l, c, x].into_iter().all(f32::is_finite) || !plot.x_range().contains(x) {
            return;
        }
        let tint = theme::text_secondary().gamma_multiply(0.7);
        let half = series::candle_half_width(slot_width) + 0.5;
        let stroke = Stroke::new(1.0, tint);
        let top = o.min(c);
        let bottom = o.max(c).max(top + 1.0);
        let body = Rect::from_min_max(Pos2::new(x - half, top), Pos2::new(x + half, bottom));
        let corners = [
            body.left_top(),
            body.right_top(),
            body.right_bottom(),
            body.left_bottom(),
            body.left_top(),
        ];
        painter.extend(Shape::dashed_line(&corners, stroke, 2.0, 2.0));
    }

    /// Draw an indicative price separately from the authoritative candle-close tag.
    /// Deliberately does not affect scaling or any series used by scripts.
    pub(super) fn draw_yahoo_quote(
        &self,
        painter: &egui::Painter,
        plot: Rect,
        scale: &PriceScale,
        price: f64,
        last: Option<&Kline>,
    ) {
        let y = scale.y(price);
        if !y.is_finite() || !plot.y_range().contains(y) {
            return;
        }
        let tint = theme::text_secondary();
        if last.is_none_or(|bar| (scale.y(bar.close) - y).abs() > 2.0) {
            painter.extend(Shape::dashed_line(
                &[Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)],
                Stroke::new(1.0, tint.gamma_multiply(0.65)),
                6.0,
                4.0,
            ));
        }
    }

    /// A bar's time as the chart writes it: to the second on a candle shorter than a minute.
    pub(super) fn time_text(&self, epoch_ms: i64) -> String {
        if self.shows_seconds() {
            format::timestamp_seconds(epoch_ms)
        } else {
            format::timestamp(epoch_ms)
        }
    }

    /// Whether the candles are shorter than a minute, so times carry their seconds.
    pub(super) fn shows_seconds(&self) -> bool {
        self.timeframe.is_some_and(Timeframe::is_subminute)
    }

    /// How wide the crosshair's time tag is: 16 characters of the axis font, 19 with seconds.
    pub(super) fn time_tag_width(&self, style: &CanvasStyle) -> f32 {
        let characters = if self.shows_seconds() { 19.0 } else { 16.0 };
        116.0 * style.time_size / 9.0 * characters / 16.0
    }

    /// The time labels, on the bars the vertical grid stands on.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw_time_axis(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        candles: &[Kline],
        ticks: &[usize],
        x_for: &dyn Fn(usize) -> f32,
        plot_right: f32,
        style: &CanvasStyle,
    ) {
        let y = rect.bottom() - style.time_axis_height() / 2.0;
        for index in ticks {
            let Some(candle) = candles.get(*index) else {
                continue;
            };
            let label = self.time_text(candle.open_time);
            let x = x_for(*index);
            // A label that would hang over the plot's right edge waits for the chart to move, as a
            // price label does at the plot's top and bottom: it would run into the price axis and
            // the scale button in the corner. So does one cut by the left edge.
            let galley =
                painter.layout_no_wrap(label[5..].to_owned(), style.time_font(), style.text_color);
            let half = galley.size().x / 2.0;
            if x + half > plot_right || x - half < rect.left() {
                continue;
            }
            let at = Pos2::new(x, y) - galley.size() / 2.0;
            painter.galley(at, galley, style.text_color);
        }
    }
}

const TREND_FLIP_MARKER_SIZE: f32 = 7.0;
const TREND_FLIP_MARKER_GAP: f32 = 4.0;
const TREND_FLIP_LABEL_FONT_SIZE: f32 = 10.5;
const TREND_FLIP_LABEL_PAD: Vec2 = Vec2::new(5.0, 2.0);
const TREND_FLIP_LABEL_GAP: f32 = 2.0;

/// `center` moved so an item `length` long stays within `lower..=upper`, or `None` when it does
/// not fit at all.
pub(super) fn contained_center(center: f32, lower: f32, upper: f32, length: f32) -> Option<f32> {
    let (low, high) = (lower + length / 2.0, upper - length / 2.0);
    (length >= 0.0 && low <= high).then(|| center.clamp(low, high))
}

fn yahoo_secondary_tag_y(y: f32, fixed: f32, half: f32, plot: Rect) -> f32 {
    let gap = 2.0 * half + 1.0;
    let (top, bottom) = (plot.top() + half, plot.bottom() - half);
    if (y - fixed).abs() >= gap {
        return y;
    }
    let above = fixed - gap;
    let below = fixed + gap;
    if y < fixed && above >= top || below > bottom && above >= top {
        above
    } else if below <= bottom {
        below
    } else {
        y
    }
}

#[cfg(test)]
#[test]
fn yahoo_session_tags_separate_nearby_prices() {
    let plot = Rect::from_min_size(Pos2::ZERO, Vec2::new(100.0, 100.0));
    assert_eq!(yahoo_secondary_tag_y(50.0, 50.0, 8.0, plot), 67.0);
    assert_eq!(yahoo_secondary_tag_y(89.0, 90.0, 8.0, plot), 73.0);
    assert_eq!(yahoo_secondary_tag_y(20.0, 60.0, 8.0, plot), 20.0);
}
