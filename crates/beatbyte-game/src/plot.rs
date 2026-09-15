//! Diagrams, drawn the way this game draws everything else.
//!
//! **Not** "charts" — in this codebase a chart is a note chart, and
//! one word for two things would cost somebody an hour eventually.
//!
//! No plotting library. The alternatives were weighed (see
//! `docs/decisions/ADR-0016-drawing-diagrams.md`): the only one that
//! fits Bevy 0.19 brings a second UI toolkit with its own font stack,
//! its own look and a mouse-first input model, which is precisely
//! what [`crate::ui_kit`] exists to prevent. So a plot here is what
//! every other surface in this game is — plain nodes in the house
//! palette — and the geometry is a handful of pure functions with
//! tests, the same way `shapes.rs` handles its textures.
//!
//! The pure half ([`Bounds`], [`fraction`], [`ticks`], [`polyline`])
//! knows nothing about Bevy and carries the arithmetic worth being
//! wrong about. The spawning half turns it into nodes.

use bevy::prelude::*;
use bevy::ui::Val::Px as px;

use crate::palette;
use crate::ui::UiFont;
use crate::ui_kit;

/// Thickness of a plotted line, in pixels.
const LINE_W: f32 = 2.0;
/// Thickness of an axis or a grid line.
const AXIS_W: f32 = 1.0;
/// How far the y labels sit to the left of the plot area.
pub const Y_AXIS_W: f32 = 54.0;
/// Height reserved under a plot for its x caption.
pub const X_AXIS_H: f32 = 16.0;

/// The value range an axis covers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    /// Lowest value drawn.
    pub min: f64,
    /// Highest value drawn.
    pub max: f64,
}

impl Bounds {
    /// A range that is never degenerate.
    ///
    /// Equal ends would divide by zero in [`fraction`] and collapse
    /// every point onto one line, which is how a flat series becomes
    /// an invisible plot. A flat series gets a band around its value
    /// instead, so it draws as the straight line it is.
    #[must_use]
    pub fn new(min: f64, max: f64) -> Bounds {
        let (min, max) = if min <= max { (min, max) } else { (max, min) };
        if (max - min).abs() < f64::EPSILON {
            let pad = if min.abs() > 1.0 {
                min.abs() * 0.1
            } else {
                0.5
            };
            Bounds {
                min: min - pad,
                max: max + pad,
            }
        } else {
            Bounds { min, max }
        }
    }

    /// Bounds that hold every value, padded by a tenth so points do
    /// not sit on the frame. `None` for no finite values at all.
    #[must_use]
    pub fn around(values: impl IntoIterator<Item = f64>) -> Option<Bounds> {
        let mut min = f64::INFINITY;
        let mut max = f64::NEG_INFINITY;
        for value in values.into_iter().filter(|v| v.is_finite()) {
            min = min.min(value);
            max = max.max(value);
        }
        if !min.is_finite() {
            return None;
        }
        let pad = (max - min) * 0.1;
        Some(Bounds::new(min - pad, max + pad))
    }

    /// The same bounds widened to include a value — the zero line of
    /// a drift plot, say, which must be on screen even when every
    /// run was late.
    #[must_use]
    pub fn including(self, value: f64) -> Bounds {
        Bounds::new(self.min.min(value), self.max.max(value))
    }

    /// The same bounds clamped into a hard range, for a quantity
    /// that cannot exceed it (accuracy never passes 100 %).
    #[must_use]
    pub fn clamped(self, low: f64, high: f64) -> Bounds {
        Bounds::new(self.min.max(low), self.max.min(high))
    }

    /// How far across the range a value sits, 0.0 at `min`, 1.0 at
    /// `max`. Values outside are clamped so a plot never draws
    /// outside its own box.
    #[must_use]
    pub fn fraction(&self, value: f64) -> f32 {
        fraction(*self, value)
    }
}

/// Where a value sits in a range, clamped to `0.0..=1.0`. Pure —
/// tested.
#[must_use]
pub fn fraction(bounds: Bounds, value: f64) -> f32 {
    let span = bounds.max - bounds.min;
    if span.abs() < f64::EPSILON {
        return 0.5;
    }
    (((value - bounds.min) / span) as f32).clamp(0.0, 1.0)
}

/// Round tick values across a range, at most `count` of them.
///
/// Ticks land on 1/2/5 × a power of ten — the steps a reader expects
/// on an axis. Pure — tested.
#[must_use]
pub fn ticks(bounds: Bounds, count: usize) -> Vec<f64> {
    if count < 2 {
        return vec![bounds.min, bounds.max];
    }
    let raw = (bounds.max - bounds.min) / (count - 1) as f64;
    if !raw.is_finite() || raw <= 0.0 {
        return vec![bounds.min, bounds.max];
    }
    let magnitude = 10f64.powf(raw.abs().log10().floor());
    let normalized = raw / magnitude;
    let step = magnitude
        * if normalized <= 1.0 {
            1.0
        } else if normalized <= 2.0 {
            2.0
        } else if normalized <= 5.0 {
            5.0
        } else {
            10.0
        };
    let first = (bounds.min / step).ceil() * step;
    let mut out = Vec::new();
    let mut value = first;
    while value <= bounds.max + step * 1e-9 && out.len() <= count {
        out.push(value);
        value += step;
    }
    if out.is_empty() {
        out.push(bounds.min);
        out.push(bounds.max);
    }
    out
}

/// One straight piece of a plotted line, ready to become a node.
///
/// Carries its MIDPOINT, because a rotated UI node turns around its
/// own centre (`bevy_ui::layout` adds the node's local centre after
/// the transform) — placing by a corner would swing every segment
/// off its own endpoints.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    /// Midpoint, pixels right of the plot box's left edge.
    pub cx: f32,
    /// Midpoint, pixels below the plot box's top edge.
    pub cy: f32,
    /// Length of the piece.
    pub len: f32,
    /// Clockwise rotation in radians, which is what `Rot2` applies
    /// in UI space (y grows downward).
    pub angle: f32,
}

/// Turn points into drawable segments.
///
/// Points are in plot-box pixels with y measured DOWNWARD, the same
/// way UI positions are; a caller converts values to that space with
/// [`Bounds::fraction`]. Zero-length pieces are dropped — two runs at
/// the same instant would otherwise leave a node with an undefined
/// angle. Pure — tested.
#[must_use]
pub fn polyline(points: &[(f32, f32)]) -> Vec<Segment> {
    points
        .windows(2)
        .filter_map(|pair| {
            let (x0, y0) = pair[0];
            let (x1, y1) = pair[1];
            let (dx, dy) = (x1 - x0, y1 - y0);
            let len = dx.hypot(dy);
            (len > f32::EPSILON).then(|| Segment {
                cx: x0 + dx / 2.0,
                cy: y0 + dy / 2.0,
                len,
                angle: dy.atan2(dx),
            })
        })
        .collect()
}

/// One line on a plot.
pub struct Series {
    /// Name for the legend.
    pub label: String,
    /// Colour of the line and its legend swatch.
    pub colour: Color,
    /// The values, oldest first. X is the index: these are runs, and
    /// spacing them by wall-clock time would bunch a year of play
    /// into whichever week was busiest.
    pub values: Vec<f64>,
}

/// Everything a line plot needs.
pub struct LinePlot<'a> {
    /// Width of the drawing area (without the y axis).
    pub width: f32,
    /// Height of the drawing area.
    pub height: f32,
    /// The lines.
    pub series: &'a [Series],
    /// The y range.
    pub bounds: Bounds,
    /// Draw a marked line at this value (0 for a drift plot).
    pub rule: Option<f64>,
    /// How a y value is written on the axis.
    pub label: fn(f64) -> String,
    /// What the x axis is, said in words.
    pub caption: String,
}

/// Spawn a line plot: y axis, grid, optional rule, one line per
/// series, legend, caption.
pub fn spawn_line_plot(parent: &mut ChildSpawnerCommands, font: &UiFont, plot: &LinePlot) {
    let drawn: usize = plot.series.iter().map(|s| s.values.len()).sum();
    if drawn == 0 {
        empty_note(parent, font, "NO RUNS YET");
        return;
    }
    parent
        .spawn(Node {
            width: px(Y_AXIS_W + plot.width),
            flex_direction: FlexDirection::Column,
            row_gap: px(6.0),
            ..default()
        })
        .with_children(|column| {
            column
                .spawn(Node {
                    width: px(Y_AXIS_W + plot.width),
                    height: px(plot.height),
                    ..default()
                })
                .with_children(|area| {
                    spawn_y_axis(area, font, plot.bounds, plot.height, plot.width, plot.label);
                    // The plot box itself: everything inside is
                    // positioned absolutely from its top-left.
                    area.spawn(Node {
                        width: px(plot.width),
                        height: px(plot.height),
                        border: UiRect::left(px(AXIS_W)).with_bottom(px(AXIS_W)),
                        ..default()
                    })
                    .insert(BorderColor::all(palette::dimmed(palette::TEXT_DIM, 0.5)))
                    .with_children(|box_node| {
                        if let Some(rule) = plot.rule {
                            let y = (1.0 - plot.bounds.fraction(rule)) * plot.height;
                            box_node.spawn((
                                Node {
                                    position_type: PositionType::Absolute,
                                    left: px(0.0),
                                    top: px(y - AXIS_W / 2.0),
                                    width: px(plot.width),
                                    height: px(AXIS_W),
                                    ..default()
                                },
                                BackgroundColor(palette::dimmed(palette::TEXT_DIM, 0.8)),
                            ));
                        }
                        for series in plot.series {
                            spawn_series(box_node, series, plot);
                        }
                    });
                });
            spawn_legend(column, font, plot.series, &plot.caption);
        });
}

/// One series' line and its point markers.
fn spawn_series(parent: &mut ChildSpawnerCommands, series: &Series, plot: &LinePlot) {
    let count = series.values.len();
    if count == 0 {
        return;
    }
    // A single run has no line to draw — but it is still a fact, and
    // a lone marker says "one run" where an empty box says "no data".
    let step = if count > 1 {
        plot.width / (count - 1) as f32
    } else {
        0.0
    };
    let at = |index: usize, value: f64| -> (f32, f32) {
        let x = if count > 1 {
            index as f32 * step
        } else {
            plot.width / 2.0
        };
        (x, (1.0 - plot.bounds.fraction(value)) * plot.height)
    };
    let points: Vec<(f32, f32)> = series
        .values
        .iter()
        .enumerate()
        .map(|(index, value)| at(index, *value))
        .collect();
    for segment in polyline(&points) {
        parent.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(segment.cx - segment.len / 2.0),
                top: px(segment.cy - LINE_W / 2.0),
                width: px(segment.len),
                height: px(LINE_W),
                ..default()
            },
            BackgroundColor(series.colour),
            UiTransform::from_rotation(Rot2::radians(segment.angle)),
        ));
    }
    // Markers, so a reader can see how many runs a line is made of —
    // a smooth line over four points invites more confidence than
    // four points deserve.
    let marker = if count > 60 { 0.0 } else { 4.0 };
    if marker > 0.0 {
        for (x, y) in points {
            parent.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: px(x - marker / 2.0),
                    top: px(y - marker / 2.0),
                    width: px(marker),
                    height: px(marker),
                    border_radius: BorderRadius::all(px(marker / 2.0)),
                    ..default()
                },
                BackgroundColor(series.colour),
            ));
        }
    }
}

/// The y axis: tick labels, right-aligned against the plot box.
fn spawn_y_axis(
    parent: &mut ChildSpawnerCommands,
    font: &UiFont,
    bounds: Bounds,
    height: f32,
    width: f32,
    label: fn(f64) -> String,
) {
    parent
        .spawn(Node {
            width: px(Y_AXIS_W),
            height: px(height),
            ..default()
        })
        .with_children(|axis| {
            for value in ticks(bounds, 4) {
                let y = (1.0 - bounds.fraction(value)) * height;
                axis.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        right: px(6.0),
                        top: px(y - ui_kit::SMALL / 2.0 - 2.0),
                        ..default()
                    },
                    Text::new(label(value)),
                    font.text(ui_kit::SMALL),
                    TextColor(ui_kit::dimmed_subtitle()),
                ));
                // The grid line reaches across the plot; drawn from
                // the axis so it sits UNDER the series nodes.
                axis.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(Y_AXIS_W),
                        top: px(y),
                        width: px(width),
                        height: px(AXIS_W),
                        ..default()
                    },
                    BackgroundColor(palette::dimmed(palette::TEXT_DIM, 0.18)),
                ));
            }
        });
}

/// Swatches and the x caption under a plot.
fn spawn_legend(
    parent: &mut ChildSpawnerCommands,
    font: &UiFont,
    series: &[Series],
    caption: &str,
) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            column_gap: px(14.0),
            align_items: AlignItems::Center,
            flex_wrap: FlexWrap::Wrap,
            padding: UiRect::left(px(Y_AXIS_W)),
            ..default()
        })
        .with_children(|legend| {
            for entry in series.iter().filter(|s| !s.values.is_empty()) {
                legend
                    .spawn(Node {
                        column_gap: px(5.0),
                        align_items: AlignItems::Center,
                        ..default()
                    })
                    .with_children(|item| {
                        item.spawn((
                            Node {
                                width: px(10.0),
                                height: px(3.0),
                                ..default()
                            },
                            BackgroundColor(entry.colour),
                        ));
                        item.spawn((
                            Text::new(entry.label.clone()),
                            font.text(ui_kit::SMALL),
                            TextColor(ui_kit::dimmed_subtitle()),
                        ));
                    });
            }
            if !caption.is_empty() {
                legend.spawn((
                    Node {
                        flex_grow: 1.0,
                        justify_content: JustifyContent::FlexEnd,
                        ..default()
                    },
                    Text::new(caption.to_owned()),
                    font.text(ui_kit::SMALL),
                    TextColor(ui_kit::dimmed_subtitle()),
                ));
            }
        });
}

/// One bar of a bar plot.
pub struct Bar {
    /// What it stands for.
    pub label: String,
    /// Its value, already in the plot's units.
    pub value: Option<f64>,
    /// Bar colour.
    pub colour: Color,
    /// The value as text, drawn at the bar's end. Empty for none.
    pub note: String,
}

/// Spawn horizontal bars: one row per bar, label left, bar right.
///
/// Horizontal rather than vertical because the labels are words
/// ("EXPERT", a song title) and a vertical bar chart would either
/// turn them on their side or cut them off.
pub fn spawn_bar_plot(
    parent: &mut ChildSpawnerCommands,
    font: &UiFont,
    bars: &[Bar],
    bounds: Bounds,
    width: f32,
) {
    if bars.is_empty() {
        empty_note(parent, font, "NOTHING TO SHOW");
        return;
    }
    parent
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(5.0),
            width: Val::Percent(100.0),
            ..default()
        })
        .with_children(|column| {
            for bar in bars {
                column
                    .spawn(Node {
                        column_gap: px(10.0),
                        align_items: AlignItems::Center,
                        ..default()
                    })
                    .with_children(|row| {
                        row.spawn((
                            Node {
                                width: px(96.0),
                                flex_shrink: 0.0,
                                ..default()
                            },
                            Text::new(bar.label.clone()),
                            font.text(ui_kit::SMALL),
                            TextColor(palette::TEXT_DIM),
                        ));
                        // The track: a bar of zero length and a bar
                        // that was never played must not look alike,
                        // so an absent value draws no fill at all and
                        // says so in its note.
                        row.spawn((
                            Node {
                                width: px(width),
                                height: px(10.0),
                                flex_shrink: 0.0,
                                ..default()
                            },
                            BackgroundColor(palette::dimmed(palette::SURFACE, 1.6)),
                        ))
                        .with_children(|track| {
                            if let Some(value) = bar.value {
                                track.spawn((
                                    Node {
                                        width: px(bounds.fraction(value) * width),
                                        height: px(10.0),
                                        ..default()
                                    },
                                    BackgroundColor(bar.colour),
                                ));
                            }
                        });
                        row.spawn((
                            Text::new(bar.note.clone()),
                            font.text(ui_kit::SMALL),
                            TextColor(if bar.value.is_some() {
                                palette::TEXT
                            } else {
                                ui_kit::dimmed_subtitle()
                            }),
                        ));
                    });
            }
        });
}

/// One slice of a stacked bar.
pub struct Slice {
    /// Share of the whole, 0.0–1.0.
    pub share: f64,
    /// Colour.
    pub colour: Color,
}

/// Spawn one stacked bar — a mix as a single line, for "what were
/// this player's judgments made of".
pub fn spawn_stack(parent: &mut ChildSpawnerCommands, slices: &[Slice], width: f32, height: f32) {
    parent
        .spawn(Node {
            width: px(width),
            height: px(height),
            flex_shrink: 0.0,
            ..default()
        })
        .with_children(|bar| {
            for slice in slices.iter().filter(|s| s.share > 0.0) {
                bar.spawn((
                    Node {
                        width: px((slice.share as f32).clamp(0.0, 1.0) * width),
                        height: px(height),
                        ..default()
                    },
                    BackgroundColor(slice.colour),
                ));
            }
        });
}

/// One row of a diverging comparison: two players on one song.
pub struct Duel {
    /// What is being compared (a song, a difficulty).
    pub label: String,
    /// How far the left player is ahead, −1.0…1.0. Negative means
    /// the right player leads.
    pub margin: f64,
    /// The two sides as text.
    pub note: String,
}

/// Spawn a diverging bar per row: a centre line, the margin growing
/// left or right from it.
///
/// The shape says who leads before a single number is read, which is
/// the whole reason a comparison is drawn rather than tabulated.
pub fn spawn_duel_plot(
    parent: &mut ChildSpawnerCommands,
    font: &UiFont,
    duels: &[Duel],
    width: f32,
    left: Color,
    right: Color,
) {
    let half = width / 2.0;
    let scale = duels
        .iter()
        .map(|duel| duel.margin.abs())
        .fold(0.0_f64, f64::max)
        .max(0.05);
    parent
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(4.0),
            ..default()
        })
        .with_children(|column| {
            for duel in duels {
                column
                    .spawn(Node {
                        column_gap: px(10.0),
                        align_items: AlignItems::Center,
                        ..default()
                    })
                    .with_children(|row| {
                        row.spawn((
                            Node {
                                width: px(210.0),
                                flex_shrink: 0.0,
                                overflow: Overflow::clip(),
                                ..default()
                            },
                            Text::new(duel.label.clone()),
                            font.text(ui_kit::SMALL),
                            TextColor(palette::TEXT_DIM),
                        ));
                        row.spawn(Node {
                            width: px(width),
                            height: px(12.0),
                            flex_shrink: 0.0,
                            ..default()
                        })
                        .with_children(|track| {
                            // The centre line: the reference every
                            // bar is read against.
                            track.spawn((
                                Node {
                                    position_type: PositionType::Absolute,
                                    left: px(half),
                                    top: px(0.0),
                                    width: px(AXIS_W),
                                    height: px(12.0),
                                    ..default()
                                },
                                BackgroundColor(palette::dimmed(palette::TEXT_DIM, 0.6)),
                            ));
                            let length =
                                ((duel.margin.abs() / scale) as f32).clamp(0.0, 1.0) * half;
                            let ahead = duel.margin >= 0.0;
                            track.spawn((
                                Node {
                                    position_type: PositionType::Absolute,
                                    left: px(if ahead { half } else { half - length }),
                                    top: px(2.0),
                                    width: px(length),
                                    height: px(8.0),
                                    ..default()
                                },
                                BackgroundColor(if ahead { left } else { right }),
                            ));
                        });
                        row.spawn((
                            Text::new(duel.note.clone()),
                            font.text(ui_kit::SMALL),
                            TextColor(palette::TEXT),
                        ));
                    });
            }
        });
}

/// The line a plot shows instead of nothing.
///
/// An empty plot area and a plot that has not loaded look identical;
/// a sentence does not.
pub fn empty_note(parent: &mut ChildSpawnerCommands, font: &UiFont, text: &str) {
    parent.spawn((
        Node {
            padding: UiRect::vertical(px(10.0)),
            ..default()
        },
        Text::new(text.to_owned()),
        font.text(ui_kit::SMALL),
        TextColor(ui_kit::dimmed_subtitle()),
    ));
}

/// A percentage, as an axis writes it.
#[must_use]
pub fn percent(value: f64) -> String {
    format!("{:.0}%", value * 100.0)
}

/// A signed millisecond figure, as the drift axis writes it.
#[must_use]
pub fn millis(value: f64) -> String {
    format!("{value:+.0}ms")
}

/// A plain rounded number, as the score axis writes it.
#[must_use]
pub fn plain(value: f64) -> String {
    if value.abs() >= 10_000.0 {
        format!("{:.0}k", value / 1000.0)
    } else {
        format!("{value:.0}")
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn a_flat_series_still_has_a_range_to_draw_in() {
        // Equal ends divide by zero and collapse every point onto one
        // line — which is how a player with three identical runs gets
        // an empty plot instead of a straight one.
        let flat = Bounds::new(0.5, 0.5);
        assert!(flat.max > flat.min);
        assert!((flat.fraction(0.5) - 0.5).abs() < 1e-6);
        let none = Bounds::around(Vec::<f64>::new());
        assert_eq!(none, None, "no values is not a range");
        let one = Bounds::around([0.75]).unwrap();
        assert!(one.max > one.min);
    }

    #[test]
    fn a_value_outside_the_range_is_drawn_on_the_frame_not_past_it() {
        let bounds = Bounds::new(0.0, 1.0);
        assert!((fraction(bounds, -5.0)).abs() < 1e-6);
        assert!((fraction(bounds, 5.0) - 1.0).abs() < 1e-6);
        assert!((fraction(bounds, 0.25) - 0.25).abs() < 1e-6);
    }

    #[test]
    fn bounds_can_be_widened_to_a_rule_and_clamped_to_what_is_possible() {
        // Every run late: the zero line must still be on screen, or
        // "late" has nothing to be late against.
        let late = Bounds::new(12.0, 30.0).including(0.0);
        assert!(late.min <= 0.0);
        // Accuracy cannot pass 100 %, so the padding must not invent
        // headroom above it.
        let high = Bounds::around([0.97, 0.99]).unwrap().clamped(0.0, 1.0);
        assert!(high.max <= 1.0 && high.min >= 0.0);
    }

    #[test]
    fn ticks_land_on_numbers_a_reader_expects() {
        let round = ticks(Bounds::new(0.0, 1.0), 4);
        assert!(round.iter().all(|v| (v * 100.0).round() % 5.0 == 0.0));
        assert!(round.len() >= 2 && round.len() <= 5);
        // Every tick is inside the range it labels.
        for value in ticks(Bounds::new(-30.0, 45.0), 4) {
            assert!((-30.0..=45.0).contains(&value), "tick {value} is off-axis");
        }
        // A degenerate range still yields something to label.
        assert_eq!(ticks(Bounds::new(2.0, 2.0), 1).len(), 2);
    }

    #[test]
    fn a_segment_is_placed_by_its_middle_and_leans_the_right_way() {
        // A rotated UI node turns about its own centre, so a segment
        // carries its midpoint. Placing by a corner would swing every
        // piece off its own endpoints.
        let flat = polyline(&[(0.0, 10.0), (10.0, 10.0)]);
        assert_eq!(flat.len(), 1);
        assert!((flat[0].cx - 5.0).abs() < 1e-5 && (flat[0].cy - 10.0).abs() < 1e-5);
        assert!((flat[0].len - 10.0).abs() < 1e-5);
        assert!(flat[0].angle.abs() < 1e-5, "a flat line is not rotated");

        // y grows DOWNWARD in UI space, and Rot2 turns clockwise: a
        // value that IMPROVES (rises on screen, smaller y) must tip
        // the segment upward, which is a negative angle.
        let rising = polyline(&[(0.0, 20.0), (10.0, 10.0)]);
        assert!(rising[0].angle < 0.0, "a rising line tipped the wrong way");
        let falling = polyline(&[(0.0, 10.0), (10.0, 20.0)]);
        assert!(falling[0].angle > 0.0);
    }

    #[test]
    fn a_repeated_point_leaves_no_zero_length_node() {
        // Two runs at the same instant would give a node with an
        // undefined angle, which renders as a stray speck.
        let doubled = polyline(&[(5.0, 5.0), (5.0, 5.0), (9.0, 5.0)]);
        assert_eq!(doubled.len(), 1);
        assert!(polyline(&[(1.0, 1.0)]).is_empty(), "one point is no line");
        assert!(polyline(&[]).is_empty());
    }

    #[test]
    fn axis_labels_read_as_the_quantity_they_measure() {
        assert_eq!(percent(0.848), "85%");
        assert_eq!(millis(-12.4), "-12ms");
        assert_eq!(millis(7.0), "+7ms", "a late drift must show its sign");
        assert_eq!(plain(4844.0), "4844");
        assert_eq!(plain(81_929.0), "82k");
    }
}
