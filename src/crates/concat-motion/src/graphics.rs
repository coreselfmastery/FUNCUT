// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! Data graphics: charts and counters a number drives.
//!
//! The presets are a vocabulary of *cards* - a bar chart whose heights are
//! the design's own, a progress bar whose fill is baked into its stylesheet.
//! Real data had to go through `set-markup`, rewriting HTML by hand. This
//! module is the other way: an operation hands over data - any domain's
//! data, a quarter's revenue or a season's goals, a survey's shares or a
//! build's timings - and gets back plain nodes: rectangles, ellipses and
//! text, every height, slope and share computed from the numbers, every
//! entrance staggered off them.
//!
//! Nothing here is markup and nothing here is a path: the sidecar engine
//! draws rectangles, ellipses and text, so every node these builders emit
//! renders through it as it stands. A line is a chain of thin rotated
//! rectangles, each growing out of the one before it; a donut is a ring of
//! them, swept in order; an area is a field of columns rising to the line.
//! That is the honest way to draw them today - the module says so in
//! `docs/MOTION.md`, and a path-drawing engine can replace the tricks
//! without touching the operation's shape.
//!
//! Determinism is the house contract: no randomness anywhere, every
//! number's pixels computed from the data alone, so the same chart is the
//! same frames on every machine.

use serde::{Deserialize, Serialize};

use crate::comp::{
    Blend, Length, Node, NodeKind, Paint, Point, Rgba, TextAlign, TextNode, Transform,
};
use crate::curve::Curve;
use crate::engine::MotionError;
use crate::presets::{ACCENT, INK, MUTED, SANS};
use crate::track::{Key, Track, Value};

/// The most points a bar, area or line chart carries. Above this the labels
/// stop being readable at any size a comp draws, and the right answer is a
/// different chart, not a smaller font.
pub const MAX_POINTS: usize = 48;

/// The most slices a donut carries. Shares past a dozen read as noise.
pub const MAX_SLICES: usize = 12;

/// The default palette: the library's own gold line, a light gold, a
/// copper, the paper ink and the muted grey - one hand with the presets,
/// whatever the domain.
const DEFAULT_PALETTE: [&str; 5] = [ACCENT, "#e8c982", "#b0764a", INK, MUTED];

/// How a number reads when it is drawn beside a bar or run up by a counter.
///
/// Domain-neutral by construction: a prefix and suffix say the unit
/// (`$`, `%`, ` ms`, ` goals`), a separator groups the thousands, and
/// `compact` writes `1.2K` the way a dashboard does. Nothing here knows
/// what the number measures.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct NumberFormat {
    /// Digits after the point.
    #[serde(default)]
    pub decimals: u32,
    /// The thousands separator; empty for none.
    #[serde(default = "comma")]
    pub separator: String,
    /// What goes before the magnitude, sign aside: `$`, `+`, `~`.
    #[serde(default)]
    pub prefix: String,
    /// What goes after it: `%`, ` ms`, ` goals`.
    #[serde(default)]
    pub suffix: String,
    /// `1.2K`, `3.4M`, `5.6B` - the dashboard form.
    #[serde(default)]
    pub compact: bool,
}

fn comma() -> String {
    ",".into()
}

impl Default for NumberFormat {
    /// The same numbers a document without the fields parses: grouped by
    /// commas, nothing else said.
    fn default() -> Self {
        NumberFormat {
            decimals: 0,
            separator: comma(),
            prefix: String::new(),
            suffix: String::new(),
            compact: false,
        }
    }
}

impl NumberFormat {
    /// The number as it is drawn.
    ///
    /// A value that is not finite draws as `0`: the operations refuse one
    /// at the door, and this never panics on a hand-written document.
    pub fn format(&self, value: f64) -> String {
        if !value.is_finite() {
            return "0".into();
        }
        let sign = if value < 0.0 { "-" } else { "" };
        let magnitude = value.abs();
        if self.compact && magnitude >= 1000.0 {
            let (unit, divisor) = if magnitude >= 1e9 {
                ("B", 1_000_000_000.0)
            } else if magnitude >= 1_000_000.0 {
                ("M", 1_000_000.0)
            } else {
                ("K", 1_000.0)
            };
            let scaled = format!("{:.1}", magnitude / divisor);
            let scaled = scaled.trim_end_matches('0').trim_end_matches('.');
            return format!("{sign}{}{scaled}{unit}{}", self.prefix, self.suffix);
        }
        let text = format!("{magnitude:.width$}", width = self.decimals as usize);
        let (whole, part) = match text.split_once('.') {
            Some((whole, part)) => (whole, Some(part)),
            None => (text.as_str(), None),
        };
        let mut grouped = String::new();
        for (i, ch) in whole.chars().enumerate() {
            if i > 0 && (whole.len() - i) % 3 == 0 && !self.separator.is_empty() {
                grouped.push_str(&self.separator);
            }
            grouped.push(ch);
        }
        match part {
            Some(part) => format!("{sign}{}{grouped}.{part}{}", self.prefix, self.suffix),
            None => format!("{sign}{}{grouped}{}", self.prefix, self.suffix),
        }
    }
}

/// One measured point: a label, the number it carries, and - when two
/// numbers belong together - the second one. What the label names is the
/// domain's business - a quarter, a country, a build, a striker - the
/// drawing is the same.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Datum {
    /// The label under the bar, beside the slice or dot.
    pub label: String,
    /// The number.
    pub value: f64,
    /// The second number, when two belong together: the B side of a
    /// compare (`value` is A), the target a progress row fills toward
    /// (`value` is the fill). Read by those two kinds only - a kind that
    /// reads one number a point refuses it by name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub b: Option<f64>,
    /// Its colour as `#rrggbb`, overriding the palette for this one. A
    /// compare refuses it: there the colour is the series', or the two
    /// sides of one category stop being tellable apart.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

/// The kinds of chart the operation draws.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChartKind {
    /// Columns from a baseline, negatives below it.
    #[default]
    Bar,
    /// The trend itself, drawn on segment by segment.
    Line,
    /// The trend and the ground under it, rising as a field of columns.
    Area,
    /// Shares of one whole, swept around a ring.
    Donut,
    /// One measured value a point, laid on a shared axis: a distribution,
    /// a ranking, a lab's readings side by side, each dot with its stem
    /// grown out to it.
    Dot,
    /// Two numbers a category, side by side: before and after, this run
    /// and that one, home and away. `value` is the A series, `b` the B.
    Compare,
    /// A number against its target, drawn as a bar filling a track:
    /// `value` achieved of `b` set, goals met, quotas filled, distance
    /// covered. The share letters the truth even past a full track.
    Progress,
}

/// The chart an `add-chart` operation draws, in any domain.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Chart {
    /// Which chart.
    #[serde(default)]
    pub kind: ChartKind,
    /// The headline above everything.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The smaller line under it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    /// The data, in order.
    pub data: Vec<Datum>,
    /// How the numbers read.
    #[serde(default)]
    pub format: NumberFormat,
    /// The colours: a donut cycles them slice by slice, a column chart,
    /// a trend or an area draws its one series in the first, and any point
    /// can name its own. A compare letters its two series in the first
    /// two.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub palette: Vec<String>,
    /// The two series' names, when the chart compares: the legend under
    /// the letterhead letters them beside their dots. Exactly two entries
    /// (`A` and `B` when unsaid), and a kind that does not compare
    /// refuses them by name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub series: Vec<String>,
    /// A plate behind the chart as `#rrggbb(aa)`; none for transparent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plate: Option<String>,
    /// Draw each bar's value above it (or each slice's share beside it).
    #[serde(default = "yes")]
    pub show_values: bool,
    /// Hairlines behind the columns.
    #[serde(default = "yes")]
    pub show_grid: bool,
    /// Seconds between one item's entrance and the next.
    #[serde(default = "default_stagger")]
    pub stagger: f64,
    /// How long one item's entrance takes.
    #[serde(default = "default_seconds")]
    pub seconds: f64,
    /// The entrance's shape.
    #[serde(default = "default_curve")]
    pub curve: Curve,
    /// What the donut's middle says, when it says anything.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub center: Option<String>,
}

fn yes() -> bool {
    true
}

fn default_stagger() -> f64 {
    0.08
}

fn default_seconds() -> f64 {
    0.5
}

fn default_curve() -> Curve {
    Curve::EMPHASIS
}

impl Default for Chart {
    /// The same chart a document without the fields parses: every default
    /// here is the field's own `#[serde(default)]`, so a chart built in
    /// code and a chart read from JSON agree without being told to.
    fn default() -> Self {
        Chart {
            kind: ChartKind::default(),
            title: None,
            subtitle: None,
            data: Vec::new(),
            format: NumberFormat::default(),
            palette: Vec::new(),
            series: Vec::new(),
            plate: None,
            show_values: yes(),
            show_grid: yes(),
            stagger: default_stagger(),
            seconds: default_seconds(),
            curve: default_curve(),
            center: None,
        }
    }
}

/// A counter an `add-counter` operation runs: a number that climbs from
/// `from` to `to`, step by eased step, every step a text node that shows
/// for its moment and hands the count on. The comp carries the count baked
/// - no renderer animates a number, because none of them can.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Counter {
    /// The first number shown.
    pub from: f64,
    /// The number it settles on.
    pub to: f64,
    /// The line under it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// How the number reads.
    #[serde(default)]
    pub format: NumberFormat,
    /// How long the count runs.
    #[serde(default = "default_count_seconds")]
    pub seconds: f64,
    /// How many numbers the count steps through. Held to 8..=96: a
    /// hand-written document cannot ask for ten thousand nodes.
    #[serde(default = "default_steps")]
    pub steps: u32,
    /// The number's size in pixels.
    #[serde(default = "default_count_size")]
    pub size: f64,
    /// The number's colour; the library ink when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// The label's colour; the muted grey when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_color: Option<String>,
    /// The count's shape.
    #[serde(default = "default_curve")]
    pub curve: Curve,
}

fn default_count_seconds() -> f64 {
    1.2
}

fn default_steps() -> u32 {
    24
}

fn default_count_size() -> f64 {
    140.0
}

impl Default for Counter {
    /// The same count a document without the fields parses.
    fn default() -> Self {
        Counter {
            from: 0.0,
            to: 0.0,
            label: None,
            format: NumberFormat::default(),
            seconds: default_count_seconds(),
            steps: default_steps(),
            size: default_count_size(),
            color: None,
            label_color: None,
            curve: default_curve(),
        }
    }
}

/// Where a graphic sits in the comp's frame: the box the chart or the
/// particle field is laid out inside, in absolute frame pixels.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Area {
    /// The box's left edge.
    pub x: f64,
    /// Its top edge.
    pub y: f64,
    /// How wide it is.
    pub width: f64,
    /// How tall it is.
    pub height: f64,
}

impl Area {
    /// The box's centre.
    fn centre(self) -> Point {
        Point::new(self.x + self.width / 2.0, self.y + self.height / 2.0)
    }
}

/// A hairline's grey: the muted grey at a low alpha, the way the library's
/// rules read behind the content.
fn hairline(alpha: f64) -> Rgba {
    Rgba::new(154.0 / 255.0, 151.0 / 255.0, 142.0 / 255.0, alpha)
}

/// A node: the shape of every one this module emits - placed at a point,
/// running from `at` to the end of the comp, transparent until its moment.
fn node(id: &str, at: f64, kind: NodeKind) -> Node {
    Node {
        id: id.into(),
        parent: None,
        start: at,
        duration: 0.0,
        transform: Transform::default(),
        opacity: 1.0,
        blend: Blend::Normal,
        mask: None,
        matte: None,
        motion_blur: None,
        effects: Vec::new(),
        tracks: Default::default(),
        kind,
    }
}

/// A text node in the library's hand: the shared sans stack, centred.
fn text(id: &str, at: f64, words: &str, size: f64, weight: u32, color: Rgba) -> Node {
    let mut built = node(
        id,
        at,
        NodeKind::Text(Box::new(TextNode {
            text: words.into(),
            size,
            weight,
            color,
            font: SANS.into(),
            align: TextAlign::Center,
            ..TextNode::default()
        })),
    );
    built.transform.position = Some(Point::new(0.0, 0.0));
    built
}

/// The fraction of a span that `seconds` fills, so a stagger measured in
/// seconds can be written as track keys measured in fractions.
fn fraction_of(span: f64, seconds: f64) -> f64 {
    if span <= 0.0 {
        return 1.0;
    }
    (seconds / span).clamp(0.0, 1.0)
}

/// An entrance track: hold zero until `start`, rise to one by `start +
/// seconds`, in the chart's curve.
fn entrance(start: f64, hold: f64, curve: &Curve) -> Track {
    let mut keys = vec![Key::new(0.0, Value::number(0.0))];
    if start > 0.0 {
        keys.push(Key::new(start, Value::number(0.0)));
    }
    keys.push(Key::eased(start + hold, Value::number(1.0), curve.clone()));
    Track::new(keys)
}

/// Build a chart's nodes: every rectangle, ellipse and text it draws, laid
/// out inside `area`, entering from `at`, ready for the comp's frame.
///
/// The nodes are root-level, named `{id}.…` - the operation's caller owns
/// the id, and the nodes sit at the root so a camera or a delete reaches
/// them without a group's transform in the way.
///
/// # Errors
///
/// Every refusal names the point that caused it: data that is empty, too
/// long or not finite, a donut with a negative share, a box too small to
/// letter. Nothing is dropped silently.
pub fn chart_nodes(
    id: &str,
    chart: &Chart,
    area: Area,
    at: f64,
    duration: f64,
) -> Result<Vec<Node>, MotionError> {
    let data = check_data(chart)?;
    if area.width < 80.0 || area.height < 80.0 {
        return Err(fail(
            format!(
                "a chart needs a box at least 80x80, and this one is {:.0}x{:.0}",
                area.width, area.height
            ),
            "give the chart room for its labels, or use a counter for one number",
        ));
    }
    let span = (duration - at).max(0.1);
    let palette: Vec<String> = if chart.palette.is_empty() {
        DEFAULT_PALETTE.iter().map(|hex| (*hex).into()).collect()
    } else {
        chart.palette.clone()
    };
    let color_of = |i: usize| -> String {
        data[i].color.clone().unwrap_or_else(|| {
            if matches!(chart.kind, ChartKind::Donut) {
                palette[i % palette.len()].clone()
            } else {
                palette[0].clone()
            }
        })
    };
    let mut nodes = Vec::new();

    // The plate, when there is one, sits under everything the chart draws.
    if let Some(plate) = &chart.plate {
        let mut plate_node = node(
            &format!("{id}.plate"),
            at,
            NodeKind::Rect {
                width: Length::px(area.width),
                height: Length::px(area.height),
                fill: Some(Paint::Solid {
                    color: Rgba::hex(plate),
                }),
                radius: (area.width.min(area.height) * 0.04).round(),
            },
        );
        plate_node.transform.position = Some(area.centre());
        plate_node.tracks.insert(
            "opacity".into(),
            entrance(0.0, fraction_of(span, 0.3), &Curve::EASE_OUT),
        );
        nodes.push(plate_node);
    }

    // The letterhead: title, then subtitle, sized from the box so the same
    // chart reads at any aspect.
    let title_size = (area.height * 0.11).clamp(22.0, 84.0);
    let mut top = area.y + area.height * 0.06;
    if let Some(title) = &chart.title {
        let mut head = text(
            &format!("{id}.title"),
            at,
            title,
            title_size,
            800,
            Rgba::hex(INK),
        );
        head.transform.position = Some(Point::new(
            area.x + area.width / 2.0,
            top + title_size * 0.55,
        ));
        if let NodeKind::Text(boxed) = &mut head.kind {
            boxed.max_width = Some(area.width * 0.9);
        }
        head.tracks.insert(
            "opacity".into(),
            entrance(0.0, fraction_of(span, 0.4), &Curve::EASE_OUT),
        );
        nodes.push(head);
        top += title_size * 1.3;
    }
    if let Some(subtitle) = &chart.subtitle {
        let mut sub = text(
            &format!("{id}.subtitle"),
            at,
            subtitle,
            (title_size * 0.42).max(14.0),
            500,
            Rgba::hex(MUTED),
        );
        sub.transform.position = Some(Point::new(
            area.x + area.width / 2.0,
            top + title_size * 0.24,
        ));
        if let NodeKind::Text(boxed) = &mut sub.kind {
            boxed.max_width = Some(area.width * 0.9);
        }
        sub.tracks.insert(
            "opacity".into(),
            entrance(0.0, fraction_of(span, 0.4), &Curve::EASE_OUT),
        );
        nodes.push(sub);
        top += title_size * 0.72;
    }

    // The plot: what is left of the box once the letterhead has had its
    // share. Bars and lines letter from the bottom up; a donut takes the
    // middle and letters its legend beside it.
    let plot = Area {
        x: area.x + area.width * 0.05,
        y: top + area.height * 0.02,
        width: area.width * 0.9,
        height: area.y + area.height * 0.92 - (top + area.height * 0.02),
    };
    let label_size = (area.height * 0.05).clamp(13.0, 40.0);
    let value_size = label_size * 1.05;
    let stagger = fraction_of(span, chart.stagger);
    let hold = fraction_of(span, chart.seconds);

    match chart.kind {
        ChartKind::Bar => bar_chart(
            id, chart, &data, &color_of, plot, at, span, stagger, hold, label_size, value_size,
            &mut nodes,
        ),
        ChartKind::Line | ChartKind::Area => line_chart(
            id, chart, &data, &color_of, plot, at, span, stagger, hold, label_size, value_size,
            &mut nodes,
        ),
        ChartKind::Donut => donut_chart(
            id, chart, &data, &color_of, plot, at, span, stagger, hold, label_size, &mut nodes,
        ),
        ChartKind::Dot => dot_chart(
            id, chart, &data, &color_of, plot, at, span, stagger, hold, label_size, value_size,
            &mut nodes,
        ),
        ChartKind::Compare => compare_chart(
            id, chart, &data, &palette, plot, at, span, stagger, hold, label_size, value_size,
            &mut nodes,
        ),
        ChartKind::Progress => progress_chart(
            id, chart, &data, &color_of, plot, at, span, stagger, hold, label_size, value_size,
            &mut nodes,
        ),
    }
    Ok(nodes)
}

/// The data, checked once for every kind: finite, present, and not more
/// than the labels can carry.
fn check_data(chart: &Chart) -> Result<Vec<Datum>, MotionError> {
    if chart.data.is_empty() {
        return Err(fail(
            "a chart needs at least one point".into(),
            "data is a list of {label, value}; one number alone is a counter",
        ));
    }
    let cap = match chart.kind {
        ChartKind::Donut => MAX_SLICES,
        _ => MAX_POINTS,
    };
    if chart.data.len() > cap {
        return Err(fail(
            format!(
                "{} points is more than a {:?} chart can letter",
                chart.data.len(),
                chart.kind
            ),
            format!("keep it to {cap}; past that a chart stops communicating"),
        ));
    }
    for (i, point) in chart.data.iter().enumerate() {
        if !point.value.is_finite() {
            return Err(fail(
                format!("point {i} ({:?}) is not a finite number", point.label),
                "a chart communicates data; nan and infinity are not data",
            ));
        }
    }
    // The second number belongs to the kinds that read it, and only those:
    // a field a kind ignores is refused here rather than dropped quietly
    // somewhere below.
    let reads_b = matches!(chart.kind, ChartKind::Compare | ChartKind::Progress);
    for (i, point) in chart.data.iter().enumerate() {
        match point.b {
            Some(b) if !reads_b => {
                return Err(fail(
                    format!(
                        "point {i} ({:?}) carries a b, and a {:?} chart reads \
                         one number a point",
                        point.label, chart.kind
                    ),
                    "b is the other side of a compare or the target of a \
                     progress row",
                ));
            }
            Some(b) if !b.is_finite() => {
                return Err(fail(
                    format!("point {i} ({:?}) has a b that is not finite", point.label),
                    "the second number is data too; nan and infinity are not data",
                ));
            }
            None if reads_b => {
                return Err(fail(
                    format!(
                        "point {i} ({:?}) has no b, and a {:?} chart needs \
                         one a point",
                        point.label, chart.kind
                    ),
                    "b is the other side of a compare or the target of a \
                     progress row",
                ));
            }
            _ => {}
        }
    }
    // A compare's colour is its series': a per-point colour would dress
    // both sides of one category the same, and the chart would stop
    // telling them apart.
    if matches!(chart.kind, ChartKind::Compare) {
        for (i, point) in chart.data.iter().enumerate() {
            if point.color.is_some() {
                return Err(fail(
                    format!("point {i} ({:?}) names a colour", point.label),
                    "a compare's colour is its series': the palette's first \
                     two colours letter its legend",
                ));
            }
        }
    }
    // A progress fills from zero toward a target: a target that is nothing
    // is no target, and a fill below zero is a bar chart's job.
    if matches!(chart.kind, ChartKind::Progress) {
        for (i, point) in chart.data.iter().enumerate() {
            if point.b.is_some_and(|b| b <= 0.0) {
                return Err(fail(
                    format!(
                        "point {i} ({:?}) has a target that is not positive",
                        point.label
                    ),
                    "a progress row fills toward its target; zero and below \
                     leave nothing to fill",
                ));
            }
            if point.value < 0.0 {
                return Err(fail(
                    format!("point {i} ({:?}) fills below zero", point.label),
                    "use a bar chart for gains and losses side by side",
                ));
            }
        }
    }
    // Series names belong to the compare that letters them.
    if !chart.series.is_empty() {
        if !matches!(chart.kind, ChartKind::Compare) {
            return Err(fail(
                format!("a {:?} chart does not compare two series", chart.kind),
                "series names a compare's two sides; drop it here",
            ));
        }
        if chart.series.len() != 2 {
            return Err(fail(
                format!(
                    "{} series names, and a compare has two sides",
                    chart.series.len()
                ),
                "name both - [`this`, `that`] - or neither for A and B",
            ));
        }
    }
    if matches!(chart.kind, ChartKind::Donut) {
        let sum: f64 = chart.data.iter().map(|point| point.value).sum();
        if chart.data.iter().any(|point| point.value < 0.0) {
            return Err(fail(
                "a donut shows shares of one whole, and a negative is not a share".into(),
                "use a bar chart for gains and losses side by side",
            ));
        }
        if sum <= 0.0 {
            return Err(fail(
                "a donut's slices add up to nothing".into(),
                "every slice needs a positive value; a chart of zeroes is a counter at zero",
            ));
        }
    }
    Ok(chart.data.clone())
}

/// The column chart: a baseline, hairlines behind, and one rectangle per
/// point - up for a positive, down for a negative - with its value above
/// it and its label below the baseline.
#[allow(clippy::too_many_arguments)]
fn bar_chart(
    id: &str,
    chart: &Chart,
    data: &[Datum],
    color_of: &dyn Fn(usize) -> String,
    plot: Area,
    at: f64,
    span: f64,
    stagger: f64,
    hold: f64,
    label_size: f64,
    value_size: f64,
    nodes: &mut Vec<Node>,
) {
    let label_row = label_size * 1.9;
    let plot_height = (plot.height - label_row).max(40.0);
    let low = data.iter().map(|point| point.value).fold(0.0_f64, f64::min);
    let mut high = data.iter().map(|point| point.value).fold(0.0_f64, f64::max);
    if high <= low {
        high = low + 1.0;
    }
    let range = high - low;
    let bottom = plot.y + plot_height;
    // The zero line sits wherever zero falls in the data's own range.
    let baseline = bottom - (0.0 - low) / range * plot_height;
    let slot = plot.width / data.len() as f64;
    let bar_width = (slot * 0.56).min(plot_width_cap(plot.width));

    if chart.show_grid && high > 0.0 {
        for g in 1..=3 {
            let y = baseline - (high * f64::from(g) / 4.0) / range * plot_height;
            if y < plot.y {
                continue;
            }
            let mut line = node(
                &format!("{id}.grid.{g}"),
                at,
                NodeKind::Rect {
                    width: Length::px(plot.width),
                    height: Length::px(1.0),
                    fill: Some(Paint::Solid {
                        color: hairline(0.16),
                    }),
                    radius: 0.0,
                },
            );
            line.transform.position = Some(Point::new(plot.x + plot.width / 2.0, y));
            line.tracks.insert(
                "opacity".into(),
                entrance(0.0, fraction_of(span, 0.35), &Curve::EASE_OUT),
            );
            nodes.push(line);
        }
    }

    // The baseline itself: the one rule every column grows from.
    let mut axis = node(
        &format!("{id}.axis"),
        at,
        NodeKind::Rect {
            width: Length::px(plot.width),
            height: Length::px(2.0),
            fill: Some(Paint::Solid {
                color: hairline(0.5),
            }),
            radius: 0.0,
        },
    );
    axis.transform.position = Some(Point::new(plot.x + plot.width / 2.0, baseline));
    axis.tracks.insert(
        "opacity".into(),
        entrance(0.0, fraction_of(span, 0.35), &Curve::EASE_OUT),
    );
    nodes.push(axis);

    for (i, point) in data.iter().enumerate() {
        let start = stagger * (i + 1) as f64;
        let centre_x = plot.x + slot * (i as f64 + 0.5);
        let height = (point.value / range) * plot_height;
        let positive = point.value >= 0.0;
        let mut bar = node(
            &format!("{id}.bar.{i}"),
            at,
            NodeKind::Rect {
                width: Length::px(bar_width),
                height: Length::px(height.abs().max(1.0)),
                fill: Some(Paint::Solid {
                    color: Rgba::hex(&color_of(i)),
                }),
                radius: (bar_width * 0.1).min(6.0),
            },
        );
        // Grown from the baseline: the anchor is the edge that touches it,
        // so the column rises out of the rule rather than unfolding from
        // its middle.
        bar.transform.anchor = Some(Point::new(0.5, if positive { 1.0 } else { 0.0 }));
        bar.transform.position = Some(Point::new(centre_x, baseline));
        bar.tracks
            .insert("scale".into(), entrance(start, hold, &chart.curve));
        nodes.push(bar);

        if chart.show_values {
            let above = if positive {
                baseline - height - value_size * 0.8
            } else {
                baseline + height.abs() + value_size * 0.8
            };
            let mut value = text(
                &format!("{id}.value.{i}"),
                at,
                &chart.format.format(point.value),
                value_size,
                700,
                Rgba::hex(INK),
            );
            value.transform.position = Some(Point::new(centre_x, above));
            if let NodeKind::Text(boxed) = &mut value.kind {
                boxed.max_width = Some(slot);
            }
            value
                .tracks
                .insert("opacity".into(), entrance(start, hold, &chart.curve));
            nodes.push(value);
        }

        let mut label = text(
            &format!("{id}.label.{i}"),
            at,
            &point.label,
            label_size,
            500,
            Rgba::hex(MUTED),
        );
        label.transform.position = Some(Point::new(centre_x, bottom + label_row * 0.5));
        if let NodeKind::Text(boxed) = &mut label.kind {
            boxed.max_width = Some(slot);
        }
        label
            .tracks
            .insert("opacity".into(), entrance(start, hold, &Curve::EASE_OUT));
        nodes.push(label);
    }
}

/// The widest one column may be, so four points do not become four walls.
fn plot_width_cap(plot_width: f64) -> f64 {
    (plot_width * 0.14).max(24.0)
}

/// The line and area chart: the trend as a chain of thin rectangles, each
/// rotated to its slope and grown out of the point before it - the draw-on
/// a path's dash would give, drawn from what the engine has. The area, if
/// asked for, is a field of columns rising to the line first.
#[allow(clippy::too_many_arguments)]
fn line_chart(
    id: &str,
    chart: &Chart,
    data: &[Datum],
    color_of: &dyn Fn(usize) -> String,
    plot: Area,
    at: f64,
    span: f64,
    stagger: f64,
    hold: f64,
    label_size: f64,
    value_size: f64,
    nodes: &mut Vec<Node>,
) {
    let label_row = label_size * 1.9;
    let plot_height = (plot.height - label_row).max(40.0);
    let mut low = data
        .iter()
        .map(|point| point.value)
        .fold(f64::MAX, f64::min);
    let mut high = data
        .iter()
        .map(|point| point.value)
        .fold(f64::MIN, f64::max);
    if high <= low {
        high = low + 1.0;
    }
    let head = (high - low) * 0.08;
    low -= head;
    high += head;
    let y_of = |value: f64| plot.y + (high - value) / (high - low) * plot_height;
    let slot = plot.width / (data.len() - 1).max(1) as f64;
    let x_of = |i: usize| plot.x + slot * i as f64;
    let baseline = y_of(0.0).clamp(plot.y, plot.y + plot_height);

    if chart.show_grid {
        for g in 1..=3 {
            let y = plot.y + plot_height * f64::from(g) / 4.0;
            let mut line = node(
                &format!("{id}.grid.{g}"),
                at,
                NodeKind::Rect {
                    width: Length::px(plot.width),
                    height: Length::px(1.0),
                    fill: Some(Paint::Solid {
                        color: hairline(0.16),
                    }),
                    radius: 0.0,
                },
            );
            line.transform.position = Some(Point::new(plot.x + plot.width / 2.0, y));
            line.tracks.insert(
                "opacity".into(),
                entrance(0.0, fraction_of(span, 0.35), &Curve::EASE_OUT),
            );
            nodes.push(line);
        }
    }

    // The area: one column per point, rising to the line's own height at
    // that point, so the field under the trend is built from the data and
    // not from an approximation of it.
    if matches!(chart.kind, ChartKind::Area) {
        let fill = Rgba::hex(&color_of(0));
        let fill = Rgba::new(fill.r, fill.g, fill.b, 0.2);
        let column_width = (slot * 0.92).max(2.0);
        for (i, point) in data.iter().enumerate() {
            let y = y_of(point.value);
            let height = (y - baseline).abs();
            let above = y <= baseline;
            if height < 1.0 {
                continue;
            }
            let start = stagger * (i + 1) as f64;
            let mut column = node(
                &format!("{id}.col.{i}"),
                at,
                NodeKind::Rect {
                    width: Length::px(column_width),
                    height: Length::px(height),
                    fill: Some(Paint::Solid { color: fill }),
                    radius: 0.0,
                },
            );
            column.transform.anchor = Some(Point::new(0.5, if above { 1.0 } else { 0.0 }));
            column.transform.position = Some(Point::new(x_of(i), baseline));
            column
                .tracks
                .insert("scale".into(), entrance(start, hold, &chart.curve));
            nodes.push(column);
        }
    }

    // The line: each segment a rectangle as long as the step, as thick as
    // the plot can letter, anchored at its own first point and rotated to
    // its slope - so growing it draws the line onward.
    let stroke = (plot.height * 0.014).clamp(2.0, 7.0);
    for i in 0..data.len().saturating_sub(1) {
        let (x1, y1) = (x_of(i), y_of(data[i].value));
        let (x2, y2) = (x_of(i + 1), y_of(data[i + 1].value));
        let length = ((x2 - x1).hypot(y2 - y1) + stroke * 0.5).max(1.0);
        let angle = (y2 - y1).atan2(x2 - x1).to_degrees();
        let start = stagger * (i + 1) as f64;
        let mut segment = node(
            &format!("{id}.seg.{i}"),
            at,
            NodeKind::Rect {
                width: Length::px(length),
                height: Length::px(stroke),
                fill: Some(Paint::Solid {
                    color: Rgba::hex(&color_of(0)),
                }),
                radius: stroke / 2.0,
            },
        );
        segment.transform.anchor = Some(Point::new(0.0, 0.5));
        segment.transform.position = Some(Point::new(x1, y1));
        segment.transform.rotation = angle;
        segment
            .tracks
            .insert("scale".into(), entrance(start, hold, &chart.curve));
        nodes.push(segment);
    }

    // The points themselves: dots that pop once the line has reached them.
    let dot = (plot.height * 0.018).clamp(5.0, 14.0);
    for (i, point) in data.iter().enumerate() {
        let start = stagger * (i + 1) as f64;
        let mut mark = node(
            &format!("{id}.dot.{i}"),
            at,
            NodeKind::Ellipse {
                width: Length::px(dot),
                height: Length::px(dot),
                fill: Some(Paint::Solid {
                    color: Rgba::hex(&color_of(i.min(data.len() - 1))),
                }),
            },
        );
        mark.transform.position = Some(Point::new(x_of(i), y_of(point.value)));
        mark.tracks
            .insert("scale".into(), entrance(start, hold, &Curve::SPRING_POP));
        nodes.push(mark);
    }

    // The last value, lettered at the trend's end, and the labels below.
    if let Some(last) = data.last()
        && chart.show_values
    {
        let start = stagger * data.len() as f64 + hold;
        let mut value = text(
            &format!("{id}.value.last"),
            at,
            &chart.format.format(last.value),
            value_size,
            700,
            Rgba::hex(INK),
        );
        value.transform.position = Some(Point::new(
            (x_of(data.len() - 1) + value_size).min(plot.x + plot.width),
            y_of(last.value) - value_size,
        ));
        value
            .tracks
            .insert("opacity".into(), entrance(start, hold, &chart.curve));
        nodes.push(value);
    }
    let label_inset = slot * 0.5;
    for (i, point) in data.iter().enumerate() {
        let x = if i == 0 {
            plot.x + label_inset
        } else if i + 1 == data.len() {
            plot.x + plot.width - label_inset
        } else {
            x_of(i)
        };
        let start = stagger * (i + 1) as f64;
        let mut label = text(
            &format!("{id}.label.{i}"),
            at,
            &point.label,
            label_size,
            500,
            Rgba::hex(MUTED),
        );
        label.transform.position = Some(Point::new(x, plot.y + plot_height + label_row * 0.5));
        if let NodeKind::Text(boxed) = &mut label.kind {
            boxed.max_width = Some(slot.max(label_size * 4.0));
        }
        label
            .tracks
            .insert("opacity".into(), entrance(start, hold, &Curve::EASE_OUT));
        nodes.push(label);
    }
}

/// The donut: shares of one whole, drawn as a ring of short rectangles -
/// one colour per slice, swept in order around the ring - with the share
/// letters outside and a legend beside.
#[allow(clippy::too_many_arguments)]
fn donut_chart(
    id: &str,
    chart: &Chart,
    data: &[Datum],
    color_of: &dyn Fn(usize) -> String,
    plot: Area,
    at: f64,
    span: f64,
    stagger: f64,
    hold: f64,
    label_size: f64,
    nodes: &mut Vec<Node>,
) {
    // The ring takes the left of the plot; the legend letters the right.
    // On a narrow box the legend drops below the ring instead.
    let legend_below = plot.width < plot.height * 1.1;
    let (ring_area, legend_area) = if legend_below {
        let ring = plot.height * 0.68;
        (
            Area {
                x: plot.x + (plot.width - ring) / 2.0,
                y: plot.y,
                width: ring,
                height: ring,
            },
            Area {
                x: plot.x,
                y: plot.y + ring + label_size * 0.6,
                width: plot.width,
                height: (plot.height - ring).max(label_size * 1.5),
            },
        )
    } else {
        let ring = plot.height;
        (
            Area {
                x: plot.x,
                y: plot.y,
                width: ring,
                height: ring,
            },
            Area {
                x: plot.x + ring + label_size * 1.2,
                y: plot.y,
                width: (plot.width - ring - label_size * 1.2).max(label_size * 6.0),
                height: plot.height,
            },
        )
    };
    let centre = ring_area.centre();
    let radius = ring_area.width.min(ring_area.height) * 0.32;
    let thickness = radius * 0.34;
    let sum: f64 = data.iter().map(|point| point.value).sum();

    // The ring is a fixed number of segments, shared out to the slices by
    // their weight; every slice keeps at least one so the smallest share
    // still shows.
    let segments = 64usize;
    let mut counts = vec![0usize; data.len()];
    let mut assigned = 0usize;
    for (i, point) in data.iter().enumerate() {
        let share = (point.value / sum * segments as f64).floor() as usize;
        counts[i] = share.max(1);
        assigned += counts[i];
    }
    // Trim or grow back to the whole ring, from the largest slice down, so
    // the ring always closes exactly.
    let mut order: Vec<usize> = (0..data.len()).collect();
    order.sort_by_key(|i| std::cmp::Reverse(counts[*i]));
    let mut diff = segments as isize - assigned as isize;
    let mut at_slice = 0;
    while diff != 0 {
        let i = order[at_slice % order.len()];
        if diff > 0 {
            counts[i] += 1;
            diff -= 1;
        } else if counts[i] > 1 {
            counts[i] -= 1;
            diff += 1;
        }
        at_slice += 1;
    }

    // One start fraction for the sweep: the whole ring turns on inside
    // `seconds` plus the stagger the data's own length sets.
    let sweep = hold.max(0.05);
    let mut angle = -90.0_f64;
    let mut index = 0usize;
    let mut total_keys = 0usize;
    for (i, count) in counts.iter().enumerate() {
        let slice_degrees = *count as f64 / segments as f64 * 360.0;
        for _ in 0..*count {
            let step = slice_degrees / *count as f64;
            let mid = (angle + step / 2.0).to_radians();
            let chord = 2.0 * radius * (step / 2.0).to_radians().sin() + 2.0;
            let mut segment = node(
                &format!("{id}.ring.{index}"),
                at,
                NodeKind::Rect {
                    width: Length::px(chord),
                    height: Length::px(thickness),
                    fill: Some(Paint::Solid {
                        color: Rgba::hex(&color_of(i)),
                    }),
                    radius: (thickness * 0.2).min(4.0),
                },
            );
            segment.transform.position = Some(Point::new(
                centre.x + radius * mid.cos(),
                centre.y + radius * mid.sin(),
            ));
            segment.transform.rotation = mid.to_degrees() + 90.0;
            let start = stagger + sweep * total_keys as f64 / segments as f64;
            segment
                .tracks
                .insert("scale".into(), entrance(start, sweep * 0.2, &chart.curve));
            nodes.push(segment);
            angle += step;
            index += 1;
            total_keys += 1;
        }
    }

    // The centre: the whole, or whatever the author wants said in the
    // middle of the ring.
    if let Some(center) = &chart.center {
        let middle = text(
            &format!("{id}.center"),
            at,
            center,
            (radius * 0.38).clamp(18.0, 64.0),
            800,
            Rgba::hex(INK),
        );
        let mut middle = middle;
        middle.transform.position = Some(centre);
        middle.tracks.insert(
            "opacity".into(),
            entrance(stagger + sweep, fraction_of(span, 0.3), &Curve::EASE_OUT),
        );
        nodes.push(middle);
    }

    // The legend: a dot, a label and a value a slice, staggered like
    // everything else.
    let row = (legend_area.height / data.len() as f64)
        .min(label_size * 2.2)
        .max(label_size * 1.6);
    let legend_top = legend_area.y + (legend_area.height - row * data.len() as f64) / 2.0;
    for (i, point) in data.iter().enumerate() {
        let y = legend_top + row * (i as f64 + 0.5);
        let start = stagger + sweep + stagger * i as f64;
        let dot = (label_size * 0.5).clamp(8.0, 18.0);
        let mut swatch = node(
            &format!("{id}.legend.dot.{i}"),
            at,
            NodeKind::Ellipse {
                width: Length::px(dot),
                height: Length::px(dot),
                fill: Some(Paint::Solid {
                    color: Rgba::hex(&color_of(i)),
                }),
            },
        );
        swatch.transform.position = Some(Point::new(legend_area.x + dot, y));
        swatch
            .tracks
            .insert("opacity".into(), entrance(start, hold, &chart.curve));
        nodes.push(swatch);

        let share = if chart.show_values {
            format!(
                "{}  {}",
                point.label,
                chart
                    .format
                    .format(point.value / sum * 100.0)
                    .trim_end_matches('%'),
            )
        } else {
            point.label.clone()
        };
        let mut label = text(
            &format!("{id}.legend.label.{i}"),
            at,
            &share,
            label_size,
            600,
            Rgba::hex(INK),
        );
        label.transform.position = Some(Point::new(legend_area.x + dot * 2.4, y));
        if let NodeKind::Text(boxed) = &mut label.kind {
            boxed.align = TextAlign::Left;
            boxed.max_width = Some((legend_area.width - dot * 2.4).max(60.0));
        }
        label
            .tracks
            .insert("opacity".into(), entrance(start, hold, &chart.curve));
        nodes.push(label);
    }
}

/// The dot plot: one row a point, each row a stem grown from the plot's
/// left edge out to its own dot, the dot popped on the end of the stem.
/// Every dot sits on the one shared value axis, so a distribution, a
/// ranking or a lab's readings read by position the way a bar chart reads
/// by height - and unlike a bar chart, a dot plot's rows do not have to
/// start anywhere in particular, which is what makes it the shape for
/// values that share no zero.
#[allow(clippy::too_many_arguments)]
fn dot_chart(
    id: &str,
    chart: &Chart,
    data: &[Datum],
    color_of: &dyn Fn(usize) -> String,
    plot: Area,
    at: f64,
    span: f64,
    stagger: f64,
    hold: f64,
    label_size: f64,
    value_size: f64,
    nodes: &mut Vec<Node>,
) {
    // The labels take a gutter at the left; the axis takes what is left.
    let gutter = (label_size * 9.0).clamp(72.0, plot.width * 0.3);
    let axis = Area {
        x: plot.x + gutter,
        y: plot.y,
        width: (plot.width - gutter).max(80.0),
        height: plot.height,
    };
    let mut low = data
        .iter()
        .map(|point| point.value)
        .fold(f64::MAX, f64::min);
    let mut high = data
        .iter()
        .map(|point| point.value)
        .fold(f64::MIN, f64::max);
    if high <= low {
        high = low + 1.0;
    }
    // The axis letters the data's own span; zero is ruled in only when the
    // data crosses it, so a lab's readings at 90..98 do not cram against
    // the right edge of an axis that started at nothing.
    let head = (high - low) * 0.06;
    low -= head;
    high += head;
    let x_of = |value: f64| axis.x + (value - low) / (high - low) * axis.width;
    let row = axis.height / data.len() as f64;
    let stem = (row * 0.08).clamp(2.0, 5.0);
    let dot = (row * 0.42).clamp(7.0, 18.0);

    if chart.show_grid {
        for g in 1..=3 {
            let x = axis.x + axis.width * f64::from(g) / 4.0;
            let mut line = node(
                &format!("{id}.grid.{g}"),
                at,
                NodeKind::Rect {
                    width: Length::px(1.0),
                    height: Length::px(axis.height),
                    fill: Some(Paint::Solid {
                        color: hairline(0.16),
                    }),
                    radius: 0.0,
                },
            );
            line.transform.position = Some(Point::new(x, axis.y + axis.height / 2.0));
            line.tracks.insert(
                "opacity".into(),
                entrance(0.0, fraction_of(span, 0.35), &Curve::EASE_OUT),
            );
            nodes.push(line);
        }
    }

    // The zero rule: a vertical line wherever zero falls, so a crossing of
    // it reads as a crossing and not as a gap between quarters.
    if (0.0..=high).contains(&low) {
        let mut rule = node(
            &format!("{id}.axis"),
            at,
            NodeKind::Rect {
                width: Length::px(2.0),
                height: Length::px(axis.height),
                fill: Some(Paint::Solid {
                    color: hairline(0.5),
                }),
                radius: 0.0,
            },
        );
        rule.transform.position = Some(Point::new(x_of(0.0), axis.y + axis.height / 2.0));
        rule.tracks.insert(
            "opacity".into(),
            entrance(0.0, fraction_of(span, 0.35), &Curve::EASE_OUT),
        );
        nodes.push(rule);
    }

    for (i, point) in data.iter().enumerate() {
        let y = axis.y + row * (i as f64 + 0.5);
        let x = x_of(point.value);
        let start = stagger * (i + 1) as f64;

        // The stem: a guide from the axis's left edge to the datum, grown
        // out to it - a hairline, so the dot stays the thing the eye reads.
        let mut stick = node(
            &format!("{id}.stem.{i}"),
            at,
            NodeKind::Rect {
                width: Length::px((x - axis.x).max(1.0)),
                height: Length::px(stem),
                fill: Some(Paint::Solid {
                    color: hairline(0.35),
                }),
                radius: stem / 2.0,
            },
        );
        stick.transform.anchor = Some(Point::new(0.0, 0.5));
        stick.transform.position = Some(Point::new(axis.x, y));
        stick
            .tracks
            .insert("scale".into(), entrance(start, hold, &chart.curve));
        nodes.push(stick);

        let mut mark = node(
            &format!("{id}.dot.{i}"),
            at,
            NodeKind::Ellipse {
                width: Length::px(dot),
                height: Length::px(dot),
                fill: Some(Paint::Solid {
                    color: Rgba::hex(&color_of(i)),
                }),
            },
        );
        mark.transform.position = Some(Point::new(x, y));
        // Popped after its own stem has grown: the eye follows the stem
        // out and the datum lands on the end of it.
        mark.tracks.insert(
            "scale".into(),
            entrance(start + hold, hold * 0.6, &Curve::SPRING_POP),
        );
        nodes.push(mark);

        let mut label = text(
            &format!("{id}.label.{i}"),
            at,
            &point.label,
            label_size,
            500,
            Rgba::hex(MUTED),
        );
        label.transform.position = Some(Point::new(axis.x - label_size * 0.6, y));
        if let NodeKind::Text(boxed) = &mut label.kind {
            boxed.align = TextAlign::Left;
            boxed.max_width = Some(gutter);
        }
        label
            .tracks
            .insert("opacity".into(), entrance(start, hold, &Curve::EASE_OUT));
        nodes.push(label);

        if chart.show_values {
            // Lettered after the dot, unless the dot is near the axis's
            // right edge and its number would letter past it - then
            // before the dot, so the number never leaves the plot.
            let room = axis.x + axis.width - (x + dot * 0.7) >= value_size * 1.6;
            let (x_value, align) = if room {
                (x + dot * 0.7 + value_size * 0.4, TextAlign::Left)
            } else {
                (x - dot * 0.7 - value_size * 0.4, TextAlign::Right)
            };
            let mut value = text(
                &format!("{id}.value.{i}"),
                at,
                &chart.format.format(point.value),
                value_size,
                700,
                Rgba::hex(INK),
            );
            value.transform.position = Some(Point::new(x_value, y));
            if let NodeKind::Text(boxed) = &mut value.kind {
                boxed.align = align;
                boxed.max_width = Some((gutter * 1.6).max(90.0));
            }
            value.tracks.insert(
                "opacity".into(),
                entrance(start + hold, hold * 0.6, &chart.curve),
            );
            nodes.push(value);
        }
    }
}

/// The comparison: two columns a category, side by side on the one
/// baseline - `value` in the palette's first colour, `b` in its second -
/// so before and after, this run and that one, home and away read as
/// pairs. A legend between the letterhead and the plot names the two
/// sides, one at each end of the plot's own width, because a builder that
/// cannot measure letters cannot flow them either.
#[allow(clippy::too_many_arguments)]
fn compare_chart(
    id: &str,
    chart: &Chart,
    data: &[Datum],
    palette: &[String],
    plot: Area,
    at: f64,
    span: f64,
    stagger: f64,
    hold: f64,
    label_size: f64,
    value_size: f64,
    nodes: &mut Vec<Node>,
) {
    let a_color = palette[0].clone();
    let b_color = palette.get(1).cloned().unwrap_or_else(|| MUTED.into());
    let names = [
        chart.series.first().cloned().unwrap_or_else(|| "A".into()),
        chart.series.get(1).cloned().unwrap_or_else(|| "B".into()),
    ];

    // The legend takes a row under the letterhead; the columns take the
    // rest, exactly as the bar chart lays its own out below it.
    let legend_row = label_size * 2.0;
    let plot = Area {
        y: plot.y + legend_row,
        height: (plot.height - legend_row).max(80.0),
        ..plot
    };
    let label_row = label_size * 1.9;
    let plot_height = (plot.height - label_row).max(40.0);
    // One range for both sides, so a pair reads as a pair: the taller of
    // the two is the taller on screen by the data's own proportion.
    let mut low = 0.0_f64;
    let mut high = 0.0_f64;
    for point in data {
        low = low.min(point.value).min(point.b.unwrap_or(0.0));
        high = high.max(point.value).max(point.b.unwrap_or(0.0));
    }
    if high <= low {
        high = low + 1.0;
    }
    let range = high - low;
    let bottom = plot.y + plot_height;
    let baseline = bottom - (0.0 - low) / range * plot_height;
    let slot = plot.width / data.len() as f64;
    let bar_width = (slot * 0.3).min(plot_width_cap(plot.width));
    let pair_step = bar_width + bar_width * 0.16;

    if chart.show_grid && high > 0.0 {
        for g in 1..=3 {
            let y = baseline - (high * f64::from(g) / 4.0) / range * plot_height;
            if y < plot.y {
                continue;
            }
            let mut line = node(
                &format!("{id}.grid.{g}"),
                at,
                NodeKind::Rect {
                    width: Length::px(plot.width),
                    height: Length::px(1.0),
                    fill: Some(Paint::Solid {
                        color: hairline(0.16),
                    }),
                    radius: 0.0,
                },
            );
            line.transform.position = Some(Point::new(plot.x + plot.width / 2.0, y));
            line.tracks.insert(
                "opacity".into(),
                entrance(0.0, fraction_of(span, 0.35), &Curve::EASE_OUT),
            );
            nodes.push(line);
        }
    }

    let mut axis = node(
        &format!("{id}.axis"),
        at,
        NodeKind::Rect {
            width: Length::px(plot.width),
            height: Length::px(2.0),
            fill: Some(Paint::Solid {
                color: hairline(0.5),
            }),
            radius: 0.0,
        },
    );
    axis.transform.position = Some(Point::new(plot.x + plot.width / 2.0, baseline));
    axis.tracks.insert(
        "opacity".into(),
        entrance(0.0, fraction_of(span, 0.35), &Curve::EASE_OUT),
    );
    nodes.push(axis);

    // The legend, one side at each end of the plot: a dot in its series'
    // colour and its name beside it.
    for (side, (name, color)) in names.iter().zip([&a_color, &b_color]).enumerate() {
        let legend_y = plot.y - legend_row * 0.45;
        let dot_size = (label_size * 0.55).clamp(9.0, 18.0);
        let edge = if side == 0 {
            plot.x + dot_size
        } else {
            plot.x + plot.width - dot_size
        };
        let start = stagger;
        let mut swatch = node(
            &format!("{id}.legend.{side}"),
            at,
            NodeKind::Ellipse {
                width: Length::px(dot_size),
                height: Length::px(dot_size),
                fill: Some(Paint::Solid {
                    color: Rgba::hex(color),
                }),
            },
        );
        swatch.transform.position = Some(Point::new(edge, legend_y));
        swatch
            .tracks
            .insert("opacity".into(), entrance(start, hold, &Curve::EASE_OUT));
        nodes.push(swatch);

        let mut name_node = text(
            &format!("{id}.legend.name.{side}"),
            at,
            name,
            label_size,
            600,
            Rgba::hex(INK),
        );
        name_node.transform.position = Some(Point::new(
            if side == 0 {
                edge + dot_size * 1.4
            } else {
                edge - dot_size * 1.4
            },
            legend_y,
        ));
        if let NodeKind::Text(boxed) = &mut name_node.kind {
            boxed.align = if side == 0 {
                TextAlign::Left
            } else {
                TextAlign::Right
            };
            boxed.max_width = Some(plot.width * 0.3);
        }
        name_node
            .tracks
            .insert("opacity".into(), entrance(start, hold, &Curve::EASE_OUT));
        nodes.push(name_node);
    }

    for (i, point) in data.iter().enumerate() {
        let start = stagger * (i + 1) as f64;
        let centre_x = plot.x + slot * (i as f64 + 0.5);
        for (side, (value, color)) in [(point.value, &a_color), (point.b.unwrap_or(0.0), &b_color)]
            .into_iter()
            .enumerate()
        {
            let height = (value / range) * plot_height;
            let positive = value >= 0.0;
            let x = if side == 0 {
                centre_x - pair_step * 0.5
            } else {
                centre_x + pair_step * 0.5
            };
            let mut bar = node(
                &format!("{id}.{}.{}", ["a", "b"][side], i),
                at,
                NodeKind::Rect {
                    width: Length::px(bar_width),
                    height: Length::px(height.abs().max(1.0)),
                    fill: Some(Paint::Solid {
                        color: Rgba::hex(color),
                    }),
                    radius: (bar_width * 0.1).min(6.0),
                },
            );
            bar.transform.anchor = Some(Point::new(0.5, if positive { 1.0 } else { 0.0 }));
            bar.transform.position = Some(Point::new(x, baseline));
            bar.tracks
                .insert("scale".into(), entrance(start, hold, &chart.curve));
            nodes.push(bar);

            if chart.show_values {
                let above = if positive {
                    baseline - height - value_size * 0.7
                } else {
                    baseline + height.abs() + value_size * 0.7
                };
                let mut value_node = text(
                    &format!("{id}.{}.value.{i}", ["a", "b"][side]),
                    at,
                    &chart.format.format(value),
                    value_size * 0.85,
                    700,
                    Rgba::hex(INK),
                );
                value_node.transform.position = Some(Point::new(x, above));
                if let NodeKind::Text(boxed) = &mut value_node.kind {
                    boxed.max_width = Some(bar_width * 1.8);
                }
                value_node
                    .tracks
                    .insert("opacity".into(), entrance(start, hold, &chart.curve));
                nodes.push(value_node);
            }
        }

        let mut label = text(
            &format!("{id}.label.{i}"),
            at,
            &point.label,
            label_size,
            500,
            Rgba::hex(MUTED),
        );
        label.transform.position = Some(Point::new(centre_x, bottom + label_row * 0.5));
        if let NodeKind::Text(boxed) = &mut label.kind {
            boxed.max_width = Some(slot);
        }
        label
            .tracks
            .insert("opacity".into(), entrance(start, hold, &Curve::EASE_OUT));
        nodes.push(label);
    }
}

/// The progress rows: a track a category - the target, drawn the plot's
/// full width - and inside it a fill grown from the track's left edge, as
/// far toward the target as the value has come. The share letters at the
/// track's right end, through the chart's own format, so the number tells
/// the truth even when the fill has reached the end of its track: a fill
/// past its target letters the overshoot, and a fill of nothing draws
/// nothing rather than a sliver that reads as something.
#[allow(clippy::too_many_arguments)]
fn progress_chart(
    id: &str,
    chart: &Chart,
    data: &[Datum],
    color_of: &dyn Fn(usize) -> String,
    plot: Area,
    at: f64,
    span: f64,
    stagger: f64,
    hold: f64,
    label_size: f64,
    value_size: f64,
    nodes: &mut Vec<Node>,
) {
    let row = plot.height / data.len() as f64;
    let bar_h = (row * 0.34).clamp(8.0, 54.0);
    let label_room = (label_size * 1.6).min(row * 0.6);

    for (i, point) in data.iter().enumerate() {
        let start = stagger * (i + 1) as f64;
        let top = plot.y + row * i as f64;
        let track_y = top + label_room + (row - label_room).max(bar_h) * 0.5;
        let target = point.b.unwrap_or(1.0);
        let share = point.value / target;

        let mut label = text(
            &format!("{id}.label.{i}"),
            at,
            &point.label,
            label_size,
            500,
            Rgba::hex(MUTED),
        );
        label.transform.position = Some(Point::new(plot.x, top + label_size * 0.55));
        if let NodeKind::Text(boxed) = &mut label.kind {
            boxed.align = TextAlign::Left;
            boxed.max_width = Some(plot.width * 0.72);
        }
        label
            .tracks
            .insert("opacity".into(), entrance(start, hold, &Curve::EASE_OUT));
        nodes.push(label);

        // The track: the target itself, full width, quiet behind the fill.
        let mut track = node(
            &format!("{id}.track.{i}"),
            at,
            NodeKind::Rect {
                width: Length::px(plot.width),
                height: Length::px(bar_h),
                fill: Some(Paint::Solid {
                    color: hairline(0.14),
                }),
                radius: (bar_h * 0.24).min(6.0),
            },
        );
        track.transform.position = Some(Point::new(plot.x + plot.width / 2.0, track_y));
        track.tracks.insert(
            "opacity".into(),
            entrance(start, fraction_of(span, 0.35), &Curve::EASE_OUT),
        );
        nodes.push(track);

        // The fill: grown from the track's left edge, capped at the track
        // - the geometry stops where the track stops, and the share
        // letters carry the number past it.
        if share > 0.0 {
            let width = (share.min(1.0) * plot.width).max(1.0);
            let mut fill = node(
                &format!("{id}.fill.{i}"),
                at,
                NodeKind::Rect {
                    width: Length::px(width),
                    height: Length::px(bar_h),
                    fill: Some(Paint::Solid {
                        color: Rgba::hex(&color_of(i)),
                    }),
                    radius: (bar_h * 0.24).min(6.0),
                },
            );
            fill.transform.anchor = Some(Point::new(0.0, 0.5));
            fill.transform.position = Some(Point::new(plot.x, track_y));
            fill.tracks
                .insert("scale".into(), entrance(start, hold, &chart.curve));
            nodes.push(fill);
        }

        if chart.show_values {
            let mut share_node = text(
                &format!("{id}.share.{i}"),
                at,
                &chart.format.format(share * 100.0),
                value_size,
                700,
                Rgba::hex(INK),
            );
            share_node.transform.position = Some(Point::new(plot.x + plot.width, track_y));
            if let NodeKind::Text(boxed) = &mut share_node.kind {
                boxed.align = TextAlign::Right;
                boxed.max_width = Some(plot.width * 0.28);
            }
            share_node.tracks.insert(
                "opacity".into(),
                entrance(start + hold, fraction_of(span, 0.4), &Curve::EASE_OUT),
            );
            nodes.push(share_node);
        }
    }
}

/// Build a counter's nodes: the numbers it steps through, one text node a
/// step, each showing for its moment and handing the count on, and the
/// final number holding to the end of the comp.
///
/// No renderer animates a number, so the count is baked: `steps` text
/// nodes whose spans tile the count's seconds. Eased by the curve, the
/// digits read as one number climbing.
///
/// # Errors
///
/// Steps outside 8..=96, a count with no seconds to run in, or a number
/// that is not finite - each refused with the fix in the message.
pub fn counter_nodes(
    id: &str,
    counter: &Counter,
    x: f64,
    y: f64,
    at: f64,
    duration: f64,
) -> Result<Vec<Node>, MotionError> {
    if !counter.from.is_finite() || !counter.to.is_finite() {
        return Err(fail(
            "a counter counts finite numbers".into(),
            "nan and infinity are not numbers a viewer can read",
        ));
    }
    if counter.steps < 8 || counter.steps > 96 {
        return Err(fail(
            format!("{} steps is outside 8..=96", counter.steps),
            "a count is letters on a frame; more than 96 numbers is a blur",
        ));
    }
    let span = duration - at;
    if span < 0.1 {
        return Err(fail(
            format!("a counter starting at {at}s has no time to run in"),
            "start it earlier, or give the comp more length",
        ));
    }
    let seconds = counter.seconds.clamp(0.1, span);
    let step = seconds / f64::from(counter.steps);
    let mut nodes = Vec::new();
    for i in 0..=counter.steps {
        let progress = f64::from(i) / f64::from(counter.steps);
        let eased = counter.curve.apply(progress);
        let value = counter.from + (counter.to - counter.from) * eased;
        let mut number = text(
            &format!("{id}.value.{i}"),
            at + step * f64::from(i),
            &counter.format.format(value),
            counter.size,
            800,
            Rgba::hex(counter.color.as_deref().unwrap_or(INK)),
        );
        // The final number holds: its span runs to the end of the comp.
        number.duration = if i == counter.steps { 0.0 } else { step };
        number.transform.position = Some(Point::new(x, y));
        if let NodeKind::Text(boxed) = &mut number.kind {
            boxed.max_width = Some(1600.0);
        }
        nodes.push(number);
    }
    if let Some(label) = &counter.label {
        let label_gap = counter.size * 0.85;
        let mut under = text(
            &format!("{id}.label"),
            at,
            label,
            (counter.size * 0.2).clamp(14.0, 40.0),
            500,
            Rgba::hex(counter.label_color.as_deref().unwrap_or(MUTED)),
        );
        under.transform.position = Some(Point::new(x, y + label_gap));
        if let NodeKind::Text(boxed) = &mut under.kind {
            boxed.max_width = Some(1600.0);
        }
        under.tracks.insert(
            "opacity".into(),
            entrance(0.0, fraction_of(span, 0.4), &Curve::EASE_OUT),
        );
        nodes.push(under);
    }
    Ok(nodes)
}

fn fail(message: String, help: impl Into<String>) -> MotionError {
    MotionError::Engine {
        code: None,
        message,
        path: None,
        help: Some(help.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::comp::{Comp, FORMAT, Rgba};

    fn comp() -> Comp {
        Comp {
            format: FORMAT,
            id: "test.graphics".into(),
            name: "Graphics".into(),
            width: 1920,
            height: 1080,
            fps: 30.0,
            duration: 6.0,
            background: Rgba::new(0.0, 0.0, 0.0, 0.0),
            assets: Default::default(),
            curves: Default::default(),
            nodes: Vec::new(),
            comps: Default::default(),
            slots: Vec::new(),
        }
    }

    fn data() -> Vec<Datum> {
        vec![
            Datum {
                label: "Q1".into(),
                value: 96.0,
                b: None,
                color: None,
            },
            Datum {
                label: "Q2".into(),
                value: 74.0,
                b: None,
                color: None,
            },
            Datum {
                label: "Q3".into(),
                value: 118.0,
                b: None,
                color: None,
            },
            Datum {
                label: "Q4".into(),
                value: 132.0,
                b: None,
                color: None,
            },
        ]
    }

    /// The same readings with their second numbers: what a compare pairs
    /// and a progress fills toward.
    fn paired_data() -> Vec<Datum> {
        vec![
            Datum {
                label: "Q1".into(),
                value: 96.0,
                b: Some(88.0),
                color: None,
            },
            Datum {
                label: "Q2".into(),
                value: 74.0,
                b: Some(102.0),
                color: None,
            },
            Datum {
                label: "Q3".into(),
                value: 118.0,
                b: Some(104.0),
                color: None,
            },
            Datum {
                label: "Q4".into(),
                value: 132.0,
                b: Some(110.0),
                color: None,
            },
        ]
    }

    fn area() -> Area {
        Area {
            x: 160.0,
            y: 140.0,
            width: 1600.0,
            height: 800.0,
        }
    }

    #[test]
    fn a_number_formats_with_separators_decimals_and_units() {
        let plain = NumberFormat::default();
        assert_eq!(plain.format(1_800_000.0), "1,800,000");
        let money = NumberFormat {
            prefix: "$".into(),
            ..Default::default()
        };
        assert_eq!(money.format(-1800.6), "-$1,801");
        let pct = NumberFormat {
            decimals: 1,
            suffix: "%".into(),
            ..Default::default()
        };
        assert_eq!(pct.format(7.26), "7.3%");
        let compact = NumberFormat {
            compact: true,
            ..Default::default()
        };
        assert_eq!(compact.format(1_234_000.0), "1.2M");
        assert_eq!(compact.format(999.0), "999");
        let none = NumberFormat {
            separator: String::new(),
            ..Default::default()
        };
        assert_eq!(none.format(1_800_000.0), "1800000");
    }

    #[test]
    fn a_number_that_is_not_finite_draws_as_zero_rather_than_panicking() {
        assert_eq!(NumberFormat::default().format(f64::NAN), "0");
    }

    /// Every kind of chart must come back as a document that validates and
    /// that the sidecar engine can draw as it stands: no path nodes, no
    /// unsupported tracks, no markup.
    #[test]
    fn every_chart_kind_builds_validates_and_maps_onto_a_timeline() {
        for kind in [
            ChartKind::Bar,
            ChartKind::Line,
            ChartKind::Area,
            ChartKind::Donut,
            ChartKind::Dot,
            ChartKind::Compare,
            ChartKind::Progress,
        ] {
            let reads_b = matches!(kind, ChartKind::Compare | ChartKind::Progress);
            let chart = Chart {
                kind,
                title: Some("Quarterly review".into()),
                subtitle: Some("units shipped, thousands".into()),
                data: if reads_b { paired_data() } else { data() },
                series: if matches!(kind, ChartKind::Compare) {
                    vec!["this year".into(), "last year".into()]
                } else {
                    Vec::new()
                },
                center: Some("2026".into()),
                ..Chart::default()
            };
            let nodes = chart_nodes("g", &chart, area(), 0.0, 6.0).expect("the chart builds");
            let mut built = comp();
            built.nodes.extend(nodes);
            let problems = built.validate();
            assert!(problems.is_empty(), "{kind:?}: {problems:?}");
            let (document, warnings) = crate::geneva::Geneva::at("geneva")
                .timeline(&built, &crate::engine::BakeRequest::default());
            let layers = document["layers"].as_array().expect("layers");
            assert_eq!(
                layers.len(),
                built.nodes.len(),
                "{kind:?}: a node was skipped by the timeline"
            );
            assert!(warnings.is_empty(), "{kind:?}: {warnings:?}");
        }
    }

    /// The geometry is the data's: the tallest bar is the tallest on
    /// screen, whatever the labels say.
    #[test]
    fn bar_heights_follow_the_data_not_the_design() {
        let chart = Chart {
            data: data(),
            ..Chart::default()
        };
        let nodes = chart_nodes("g", &chart, area(), 0.0, 6.0).expect("builds");
        let heights: Vec<(String, f64)> = nodes
            .iter()
            .filter_map(|node| match &node.kind {
                NodeKind::Rect {
                    height: Length::Px(px),
                    ..
                } => Some((node.id.clone(), *px)),
                _ => None,
            })
            .filter(|(id, _)| id.starts_with("g.bar."))
            .collect();
        assert_eq!(heights.len(), 4, "{heights:?}");
        // 118 and 132 tower over 74, in the data's own proportions.
        assert!(heights[3].1 > heights[2].1, "{heights:?}");
        assert!(
            (heights[2].1 / heights[1].1 - 118.0 / 74.0).abs() < 0.01,
            "{heights:?}"
        );
    }

    /// A loss is a column below the rule, grown downward from it, with its
    /// value lettered underneath - not a silent flip of the fill.
    #[test]
    fn a_negative_value_draws_below_the_baseline() {
        let chart = Chart {
            data: vec![
                Datum {
                    label: "gain".into(),
                    value: 40.0,
                    b: None,
                    color: None,
                },
                Datum {
                    label: "loss".into(),
                    value: -20.0,
                    b: None,
                    color: None,
                },
            ],
            ..Chart::default()
        };
        let nodes = chart_nodes("g", &chart, area(), 0.0, 6.0).expect("builds");
        let loss = nodes
            .iter()
            .find(|node| node.id == "g.bar.1")
            .expect("the loss bar");
        assert_eq!(
            loss.transform.anchor,
            Some(Point::new(0.5, 0.0)),
            "a negative grows down from the rule"
        );
    }

    /// The data is checked before anything is drawn, and every refusal
    /// names the point that caused it.
    #[test]
    fn bad_data_is_refused_with_the_reason() {
        let empty = Chart::default();
        assert!(chart_nodes("g", &empty, area(), 0.0, 6.0).is_err());

        let nan = Chart {
            data: vec![Datum {
                label: "x".into(),
                value: f64::NAN,
                b: None,
                color: None,
            }],
            ..Chart::default()
        };
        assert!(chart_nodes("g", &nan, area(), 0.0, 6.0).is_err());

        let donut_negative = Chart {
            kind: ChartKind::Donut,
            data: vec![
                Datum {
                    label: "a".into(),
                    value: 10.0,
                    b: None,
                    color: None,
                },
                Datum {
                    label: "b".into(),
                    value: -4.0,
                    b: None,
                    color: None,
                },
            ],
            ..Chart::default()
        };
        assert!(chart_nodes("g", &donut_negative, area(), 0.0, 6.0).is_err());

        let tiny = Chart {
            data: data(),
            ..Chart::default()
        };
        assert!(
            chart_nodes(
                "g",
                &tiny,
                Area {
                    x: 0.0,
                    y: 0.0,
                    width: 40.0,
                    height: 40.0
                },
                0.0,
                6.0
            )
            .is_err()
        );
    }

    /// More points than the labels can carry is refused with the cap in
    /// the message, not drawn as noise.
    #[test]
    fn too_many_points_is_refused_with_the_cap() {
        let crowd = Chart {
            data: (0..60)
                .map(|i| Datum {
                    label: format!("p{i}"),
                    value: i as f64,
                    b: None,
                    color: None,
                })
                .collect(),
            ..Chart::default()
        };
        assert!(chart_nodes("g", &crowd, area(), 0.0, 6.0).is_err());
    }

    /// The same chart is the same nodes, twice, every time: nothing here
    /// consults anything but the data.
    #[test]
    fn the_same_chart_builds_identically_twice() {
        let chart = Chart {
            data: data(),
            ..Chart::default()
        };
        let a = chart_nodes("g", &chart, area(), 0.0, 6.0).expect("builds");
        let b = chart_nodes("g", &chart, area(), 0.0, 6.0).expect("builds");
        assert_eq!(a, b);

        // The paired kinds too: a compare's two sides and a progress's
        // shares come back the same twice - and the dot plot, one number
        // a point, is the same twice as well.
        for kind in [ChartKind::Compare, ChartKind::Progress, ChartKind::Dot] {
            let chart = Chart {
                kind,
                // The dot plot reads one number a point and refuses the
                // second; the paired kinds need it.
                data: if matches!(kind, ChartKind::Dot) {
                    data()
                } else {
                    paired_data()
                },
                // Only the compare reads series names; the others refuse
                // them, and refusing is part of being the same twice.
                series: if matches!(kind, ChartKind::Compare) {
                    vec!["this".into(), "that".into()]
                } else {
                    Vec::new()
                },
                ..Chart::default()
            };
            let a = chart_nodes("g", &chart, area(), 0.0, 6.0).expect("builds");
            let b = chart_nodes("g", &chart, area(), 0.0, 6.0).expect("builds");
            assert_eq!(a, b, "{kind:?}");
        }
    }

    /// A dot plot's dots sit where the data puts them: the bigger number
    /// is the dot further along the axis, in the data's own proportion.
    #[test]
    fn dot_positions_follow_the_data_not_the_design() {
        let chart = Chart {
            kind: ChartKind::Dot,
            data: data(),
            ..Chart::default()
        };
        let nodes = chart_nodes("g", &chart, area(), 0.0, 6.0).expect("builds");
        let xs: Vec<f64> = nodes
            .iter()
            .filter(|node| node.id.starts_with("g.dot."))
            .filter_map(|node| node.transform.position)
            .map(|point| point.x)
            .collect();
        assert_eq!(xs.len(), 4, "one dot a point");
        assert!(xs[3] > xs[2], "132 sits beyond 118");
        assert!(xs[1] < xs[0], "74 sits before 96");
        // And in the data's own proportion: the gap between two dots on
        // the screen is the gap between their numbers times the one scale
        // the whole axis shares.
        let scale = (xs[3] - xs[0]) / (132.0 - 96.0);
        assert!(
            ((xs[2] - xs[1]) - (118.0 - 74.0) * scale).abs() < 1e-9,
            "the axis is one scale: {xs:?}"
        );
        assert!(
            (xs[0] - xs[1] - (96.0 - 74.0) * scale).abs() < 1e-9,
            "{xs:?}"
        );
    }

    /// A compare draws two columns a category on the one range: both of
    /// Q1's sides, value's and b's, in the data's own proportions, in the
    /// palette's first two colours, with a legend that names them.
    #[test]
    fn a_compare_pairs_two_columns_a_category_on_one_range() {
        let chart = Chart {
            kind: ChartKind::Compare,
            data: paired_data(),
            series: vec!["this year".into(), "last year".into()],
            ..Chart::default()
        };
        let nodes = chart_nodes("g", &chart, area(), 0.0, 6.0).expect("builds");
        let height = |id: &str| -> f64 {
            nodes
                .iter()
                .find(|node| node.id == id)
                .and_then(|node| match &node.kind {
                    NodeKind::Rect {
                        height: Length::Px(px),
                        ..
                    } => Some(*px),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("{id}: a column"))
        };
        // Two sides a category, both lettered and both drawn.
        assert_eq!(height("g.a.0"), 96.0 / 132.0 * height("g.a.3"));
        assert_eq!(height("g.b.0"), 88.0 / 110.0 * height("g.b.3"));
        // The colours are the series': the palette's first two, so the
        // sides of one category never dress the same.
        let fill = |id: &str| -> String {
            nodes
                .iter()
                .find(|node| node.id == id)
                .and_then(|node| match &node.kind {
                    NodeKind::Rect {
                        fill: Some(Paint::Solid { color }),
                        ..
                    } => Some(format!("#{color:?}")),
                    _ => None,
                })
                .unwrap_or_default()
        };
        assert_ne!(fill("g.a.0"), fill("g.b.0"));
        // The legend names both sides, from the series the author gave.
        let legend: Vec<String> = nodes
            .iter()
            .filter(|node| node.id.starts_with("g.legend.name."))
            .filter_map(|node| match &node.kind {
                NodeKind::Text(words) => Some(words.text.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(legend, vec!["this year", "last year"]);
    }

    /// A progress row fills its track by its share of the target, and the
    /// share letters the truth even past a full track: the geometry caps
    /// at the track, the number does not.
    #[test]
    fn a_progress_fill_is_its_share_and_letters_the_truth_past_it() {
        let chart = Chart {
            kind: ChartKind::Progress,
            format: NumberFormat {
                suffix: "%".into(),
                decimals: 0,
                ..Default::default()
            },
            data: vec![
                Datum {
                    label: "halfway".into(),
                    value: 5.0,
                    b: Some(10.0),
                    color: None,
                },
                Datum {
                    label: "over".into(),
                    value: 13.0,
                    b: Some(10.0),
                    color: None,
                },
                Datum {
                    label: "nothing".into(),
                    value: 0.0,
                    b: Some(10.0),
                    color: None,
                },
            ],
            ..Chart::default()
        };
        let nodes = chart_nodes("g", &chart, area(), 0.0, 6.0).expect("builds");
        let width = |id: &str| -> f64 {
            nodes
                .iter()
                .find(|node| node.id == id)
                .and_then(|node| match &node.kind {
                    NodeKind::Rect {
                        width: Length::Px(px),
                        ..
                    } => Some(*px),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("{id}: a rect"))
        };
        let track = width("g.track.0");
        assert!(
            (width("g.fill.0") - track / 2.0).abs() < 1.5,
            "halfway is half the track"
        );
        assert!(
            (width("g.fill.1") - track).abs() < 1.5,
            "past the target the fill caps"
        );
        assert!(
            !nodes.iter().any(|node| node.id == "g.fill.2"),
            "a fill of nothing draws nothing, not a sliver that reads as something"
        );
        // The shares letter through the format: 50%, 130%, 0%.
        let shares: Vec<String> = nodes
            .iter()
            .filter(|node| node.id.starts_with("g.share."))
            .filter_map(|node| match &node.kind {
                NodeKind::Text(words) => Some(words.text.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(shares, vec!["50%", "130%", "0%"]);
    }

    /// The second number and the series names belong to the kinds that
    /// read them, and every misuse is refused by name - not dropped
    /// quietly somewhere below.
    #[test]
    fn the_second_number_and_series_are_refused_where_they_are_not_read() {
        // A bar that carries a b: one number a point is the bar's whole
        // vocabulary.
        let bar_with_b = Chart {
            data: vec![Datum {
                label: "x".into(),
                value: 3.0,
                b: Some(4.0),
                color: None,
            }],
            ..Chart::default()
        };
        assert!(chart_nodes("g", &bar_with_b, area(), 0.0, 6.0).is_err());

        // A donut that names series: it has no two sides to name.
        let donut_with_series = Chart {
            kind: ChartKind::Donut,
            series: vec!["a".into(), "b".into()],
            data: vec![Datum {
                label: "x".into(),
                value: 3.0,
                b: None,
                color: None,
            }],
            ..Chart::default()
        };
        assert!(chart_nodes("g", &donut_with_series, area(), 0.0, 6.0).is_err());

        // A compare missing its b on one point: the point is named.
        let compare_missing_b = Chart {
            kind: ChartKind::Compare,
            data: vec![
                Datum {
                    label: "whole".into(),
                    value: 3.0,
                    b: Some(4.0),
                    color: None,
                },
                Datum {
                    label: "half".into(),
                    value: 5.0,
                    b: None,
                    color: None,
                },
            ],
            ..Chart::default()
        };
        assert!(chart_nodes("g", &compare_missing_b, area(), 0.0, 6.0).is_err());

        // A progress toward nothing.
        let progress_to_zero = Chart {
            kind: ChartKind::Progress,
            data: vec![Datum {
                label: "x".into(),
                value: 3.0,
                b: Some(0.0),
                color: None,
            }],
            ..Chart::default()
        };
        assert!(chart_nodes("g", &progress_to_zero, area(), 0.0, 6.0).is_err());

        // A progress that fills below zero.
        let progress_negative = Chart {
            kind: ChartKind::Progress,
            data: vec![Datum {
                label: "x".into(),
                value: -3.0,
                b: Some(10.0),
                color: None,
            }],
            ..Chart::default()
        };
        assert!(chart_nodes("g", &progress_negative, area(), 0.0, 6.0).is_err());

        // A compare that colours one category: the colour is the series'.
        let compare_colored = Chart {
            kind: ChartKind::Compare,
            data: vec![Datum {
                label: "x".into(),
                value: 3.0,
                b: Some(4.0),
                color: Some("#ff0000".into()),
            }],
            ..Chart::default()
        };
        assert!(chart_nodes("g", &compare_colored, area(), 0.0, 6.0).is_err());

        // One series name is half a legend.
        let compare_half_named = Chart {
            kind: ChartKind::Compare,
            series: vec!["only".into()],
            data: vec![Datum {
                label: "x".into(),
                value: 3.0,
                b: Some(4.0),
                color: None,
            }],
            ..Chart::default()
        };
        assert!(chart_nodes("g", &compare_half_named, area(), 0.0, 6.0).is_err());
    }

    /// A counter steps through its numbers, and the last one holds to the
    /// end of the comp.
    #[test]
    fn a_counter_counts_and_holds() {
        let counter = Counter {
            from: 0.0,
            to: 180_000.0,
            label: Some("goals this season".into()),
            format: NumberFormat {
                suffix: " goals".into(),
                ..Default::default()
            },
            ..Counter::default()
        };
        let nodes = counter_nodes("c", &counter, 960.0, 500.0, 0.0, 6.0).expect("builds");
        assert_eq!(nodes.len(), 26, "24 steps and the label");
        let first = nodes.iter().find(|n| n.id == "c.value.0").expect("first");
        let last = nodes.iter().find(|n| n.id == "c.value.24").expect("last");
        assert_eq!(first.duration, counter.seconds / 24.0);
        assert_eq!(last.duration, 0.0, "the final number holds to the end");
        let problems = {
            let mut built = comp();
            built.nodes.extend(nodes.clone());
            built.validate()
        };
        assert!(problems.is_empty(), "{problems:?}");
        // The digits climb as numbers, not as strings: "77,528" would
        // sort after "103,892" in a dictionary and still be a climb.
        let numbers: Vec<f64> = nodes
            .iter()
            .filter(|n| n.id.starts_with("c.value."))
            .filter_map(|n| match &n.kind {
                NodeKind::Text(t) => {
                    let digits: String = t.text.chars().filter(|ch| ch.is_ascii_digit()).collect();
                    digits.parse::<f64>().ok()
                }
                _ => None,
            })
            .collect();
        assert_eq!(numbers.len(), 25, "24 steps and the landing");
        assert!(
            numbers.windows(2).all(|pair| pair[0] <= pair[1]),
            "{numbers:?}"
        );
        assert_eq!(numbers.last().copied(), Some(180_000.0));
    }

    /// Every data-graphics example - the documents `dump_graphics` writes
    /// and the repo carries - validates and maps onto a timeline with
    /// every node drawn and nothing warned about: the examples are the
    /// contract, in both directions.
    #[test]
    fn the_data_graphics_examples_are_valid_and_map_cleanly() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/comps");
        for name in [
            "data.chart.bars.json",
            "data.chart.line.json",
            "data.chart.donut.json",
            "data.chart.dot.json",
            "data.chart.compare.json",
            "data.chart.progress.json",
            "data.counter.json",
            "data.particles.json",
            "data.camera.json",
        ] {
            let text = std::fs::read_to_string(dir.join(name)).unwrap_or_else(|error| {
                panic!(
                    "{name}: {error} - run `cargo run -p concat-motion --example \
                     dump_graphics -- examples/comps` and carry the output"
                )
            });
            let comp: Comp = serde_json::from_str(&text).expect("the example parses");
            let problems = comp.validate();
            assert!(problems.is_empty(), "{name}: {problems:?}");
            let (document, warnings) = crate::geneva::Geneva::at("geneva")
                .timeline(&comp, &crate::engine::BakeRequest::default());
            let layers = document["layers"].as_array().expect("layers");
            assert_eq!(
                layers.len(),
                comp.nodes.len(),
                "{name}: a node was skipped by the timeline"
            );
            assert!(warnings.is_empty(), "{name}: {warnings:?}");
        }
    }

    #[test]
    fn a_counter_refuses_impossible_steps_and_numbers() {
        let nan = Counter {
            from: f64::NAN,
            to: 10.0,
            ..Counter::default()
        };
        assert!(counter_nodes("c", &nan, 0.0, 0.0, 0.0, 6.0).is_err());
        let crowded = Counter {
            steps: 500,
            ..Counter::default()
        };
        assert!(counter_nodes("c", &crowded, 0.0, 0.0, 0.0, 6.0).is_err());
        let late = Counter {
            ..Counter::default()
        };
        assert!(counter_nodes("c", &late, 0.0, 0.0, 6.0, 6.0).is_err());
    }
}
