//! The ridge in printable ASCII, one character to a cell: chosen by the line's height
//! in the cell where it is flat, by its slope where it is not. Rows are drawn back to
//! front, each clearing the cells under its line first.

use std::io::Write as _;

use super::cells::{CELL_DOTS, Paint};
use crate::ridge::DrawList;

/// How an ASCII mode chooses its characters.
#[derive(Clone, Copy, PartialEq)]
pub enum Look {
    /// Dashes with gaps that drift along the row; heavier characters at higher levels.
    Static,
    /// One fixed character for each stroke.
    Line,
}

/// Exponent from a cell's brightness to how far its colour stands out from the
/// backdrop: a character covers little of its cell.
const CURVE: f32 = 0.45;

/// A column whose line moves less than this many cells is flat, and less than `GENTLE`
/// a gentle slope; one that crosses more than `STEEP` is drawn upright.
const FLAT: f32 = 0.12;
const GENTLE: f32 = 0.45;
const STEEP: f32 = 1.7;
/// A crest or dip this far beyond both ends of its column, in cells, gets its own
/// character, unless the ends are `TURN_SPAN` or more apart.
const TURN: f32 = 0.22;
const TURN_SPAN: f32 = 1.2;

/// The level at which static draws its heaviest characters.
const HOT: f32 = 0.6;

/// How much static moves, 0...1: scales its drift, gaps and flicker.
const RESTLESS: f32 = 0.6;
/// Columns a second the dashes drift at `RESTLESS` 0 and 1, before each row's own
/// factor of 0.6 to 1.4.
const DRIFT: (f32, f32) = (1.5, 8.5);
/// About the share of flat cells left empty at level 0 and `RESTLESS` 1; fewer as the
/// level rises.
const GAPS: f32 = 0.5;
/// Flicker ticks a second.
const FLICKER_HZ: f64 = 7.0;

/// The shape of a row's line within one cell.
#[derive(Clone, Copy)]
enum Stroke {
    /// Flat, in the top, middle or bottom of the cell.
    Top,
    Middle,
    Bottom,
    /// Between the top and the middle, or the middle and the bottom; gentle slopes only.
    Upper,
    Lower,
    Rising,
    Falling,
    Upright,
    Crest,
    Dip,
}

impl Stroke {
    fn flat(self) -> bool {
        matches!(self, Self::Top | Self::Middle | Self::Bottom | Self::Upper | Self::Lower)
    }

    fn plain(self) -> u8 {
        match self {
            Self::Top => b'"',
            Self::Upper => b'\'',
            Self::Middle => b'-',
            Self::Lower => b'.',
            Self::Bottom => b'_',
            Self::Rising => b'/',
            Self::Falling => b'\\',
            Self::Upright => b'|',
            Self::Crest => b'^',
            Self::Dip => b'v',
        }
    }

    /// The characters static draws the stroke with, from level 0 to `HOT`.
    fn family(self) -> &'static [u8; 6] {
        match self {
            Self::Top => b"\"'\"^*#",
            Self::Upper => b"'`'\"*#",
            Self::Middle => b"--~=+#",
            Self::Lower => b".,._:=",
            Self::Bottom => b"__.,_=",
            Self::Rising => b"//////",
            Self::Falling => b"\\\\\\\\\\\\",
            Self::Upright => b"|:!|I#",
            Self::Crest => b"^^A*#@",
            Self::Dip => b"vvvVVV",
        }
    }
}

/// The strokes for a column whose line enters at `left`, leaves at `right` and reaches
/// from `top` to `bottom` between them, in cells from the top of the grid, into `out`
/// as (terminal line, stroke).
fn trace(left: f32, right: f32, top: f32, bottom: f32, out: &mut Vec<(i64, Stroke)>) {
    out.clear();
    let climb = left - right;
    let span = climb.abs();
    let middle = (left + right) / 2.0;
    let (upper, lower) = (left.min(right), left.max(right));
    if upper - top > TURN && span < TURN_SPAN {
        let line = top.floor() as i64;
        out.push((line, Stroke::Crest));
        out.extend((line + 1..=upper.floor() as i64).map(|line| (line, Stroke::Upright)));
        return;
    }
    if bottom - lower > TURN && span < TURN_SPAN {
        out.push((bottom.floor() as i64, Stroke::Dip));
        return;
    }
    let line = middle.floor() as i64;
    let within = middle - middle.floor();
    if span < FLAT {
        let stroke = if within < 0.3 {
            Stroke::Top
        } else if within < 0.7 {
            Stroke::Middle
        } else {
            Stroke::Bottom
        };
        out.push((line, stroke));
    } else if span < GENTLE {
        let stroke = match (within * 5.0) as u32 {
            0 => Stroke::Top,
            1 => Stroke::Upper,
            2 => Stroke::Middle,
            3 => Stroke::Lower,
            _ => Stroke::Bottom,
        };
        out.push((line, stroke));
    } else if lower.floor() - upper.floor() >= 2.0 || span > STEEP {
        out.extend((upper.floor() as i64..=lower.floor() as i64).map(|line| (line, Stroke::Upright)));
    } else {
        out.push((line, if climb > 0.0 { Stroke::Rising } else { Stroke::Falling }));
    }
}

/// A value in 0...1, the same for the same three numbers.
fn hash(a: u32, b: u32, c: u32) -> f32 {
    let mut n = a
        .wrapping_mul(374_761_393)
        .wrapping_add(b.wrapping_mul(668_265_263))
        .wrapping_add(c.wrapping_mul(1_442_695_041));
    n = (n ^ (n >> 13)).wrapping_mul(1_274_126_177);
    n ^= n >> 16;
    n as f32 / 4_294_967_296.0
}

/// Smooth noise in 0...1 along `x`.
fn noise(x: f64) -> f32 {
    let whole = x.floor();
    let t = (x - whole) as f32;
    let ease = t * t * (3.0 - 2.0 * t);
    let at = whole as i64 as u32;
    hash(at, 7, 0) * (1.0 - ease) + hash(at.wrapping_add(1), 7, 0) * ease
}

/// A grid of characters the size of the terminal lines drawn in.
pub struct Grid {
    cols: usize,
    lines: usize,
    /// A space where the cell is empty.
    glyphs: Vec<u8>,
    /// Each lit cell's brightness (its row's alpha, less static's flicker) and height
    /// above its row's baseline as a share of the row's rise, 0...1.
    brightness: Vec<f32>,
    levels: Vec<f32>,
    /// Per column, in dots: the top of the current row's erase, where its line enters
    /// and leaves, and the highest and lowest it reaches.
    top: Vec<f32>,
    left: Vec<f32>,
    right: Vec<f32>,
    low: Vec<f32>,
    high: Vec<f32>,
    strokes: Vec<(i64, Stroke)>,
}

impl Grid {
    pub fn new(cols: usize, lines: usize) -> Self {
        Self {
            cols,
            lines,
            glyphs: vec![b' '; cols * lines],
            brightness: vec![0.0; cols * lines],
            levels: vec![0.0; cols * lines],
            top: vec![0.0; cols],
            left: vec![0.0; cols],
            right: vec![0.0; cols],
            low: vec![0.0; cols],
            high: vec![0.0; cols],
            strokes: Vec::new(),
        }
    }

    /// Clears the grid and draws `list`, whose coordinates are dots. `now`, in seconds,
    /// moves static's drift and flicker.
    pub fn draw(&mut self, list: &DrawList, look: Look, now: f64) {
        self.glyphs.fill(b' ');
        let (across, down) = (CELL_DOTS.0 as f32, CELL_DOTS.1 as f32);
        let tick = (now * FLICKER_HZ).floor() as i64 as u32;
        let mut strokes = std::mem::take(&mut self.strokes);

        for row in &list.rows {
            let spans = &list.spans[row.spans.start as usize..row.spans.end as usize];
            if let Some(first) = spans.first() {
                self.top.fill(f32::INFINITY);
                for span in spans {
                    if !(span.x1 > span.x0) {
                        continue;
                    }
                    for column in columns(span.x0, span.x1, across, self.cols) {
                        let at = |x: f32| span.top0 + (x - span.x0) / (span.x1 - span.x0) * (span.top1 - span.top0);
                        let from = (column as f32 * across).max(span.x0);
                        let to = ((column + 1) as f32 * across).min(span.x1);
                        self.top[column] = self.top[column].min(at(from).min(at(to)));
                    }
                }
                for column in 0..self.cols {
                    let top = self.top[column];
                    if top < first.floor {
                        self.clear(column, top / down, first.floor / down);
                    }
                }
            }

            let segments = &list.segments[row.segments.start as usize..row.segments.end as usize];
            let Some(first) = segments.first() else { continue };
            let alpha = first.alpha;
            self.low.fill(f32::INFINITY);
            self.high.fill(f32::NEG_INFINITY);
            for segment in segments {
                let ([x1, y1], [x2, y2]) = (segment.p1, segment.p2);
                if !(x2 > x1) {
                    continue;
                }
                let slope = (y2 - y1) / (x2 - x1);
                for column in columns(x1, x2, across, self.cols) {
                    let from = (column as f32 * across).max(x1);
                    let to = ((column + 1) as f32 * across).min(x2);
                    let (a, b) = (y1 + (from - x1) * slope, y1 + (to - x1) * slope);
                    if self.low[column] > self.high[column] {
                        self.left[column] = a;
                    }
                    self.right[column] = b;
                    self.low[column] = self.low[column].min(a.min(b));
                    self.high[column] = self.high[column].max(a.max(b));
                }
            }

            let level =
                |y: f32| if row.rise > 0.0 { ((row.baseline - y * down) / row.rise).clamp(0.0, 1.0) } else { 0.0 };
            let id = row.id as u32;
            // Each row's dashes drift in its own direction at its own pace.
            let pace = DRIFT.0 + (DRIFT.1 - DRIFT.0) * RESTLESS * (0.6 + 0.8 * hash(id, 5, 0));
            let drift = (now * f64::from(pace) * if id & 1 == 1 { 1.0 } else { -1.0 }).floor() as i64;

            for column in 0..self.cols {
                let (low, high) = (self.low[column], self.high[column]);
                if !(low <= high) {
                    continue;
                }
                let (top, bottom) = (low / down, high / down);
                let (left, right) = (self.left[column] / down, self.right[column] / down);
                for line in
                    top.floor().max(0.0) as usize..=(bottom.floor().max(0.0) as usize).min(self.lines.saturating_sub(1))
                {
                    self.glyphs[line * self.cols + column] = b' ';
                }
                trace(left, right, top, bottom, &mut strokes);
                let heat = (level(top) / HOT).min(1.0);
                let along = (column as i64 + drift) as u32;
                let gap = look == Look::Static
                    && noise(f64::from(along) * 0.42 + f64::from(id % 4096) * 17.3)
                        < GAPS * (1.0 - heat * 3.0) * RESTLESS + 0.04;
                for &(line, stroke) in &strokes {
                    let y = match stroke {
                        Stroke::Upright => line as f32 + 0.5,
                        Stroke::Crest => top,
                        Stroke::Dip => bottom,
                        _ => (left + right) / 2.0,
                    };
                    match look {
                        Look::Static => {
                            if gap && stroke.flat() {
                                continue;
                            }
                            // On its turn a cell shows another character for one tick.
                            let turn = if hash(along, id, tick) < 0.25 * RESTLESS { tick } else { 0 };
                            let flick = hash(along, line as u32, id.wrapping_add(turn));
                            let family = stroke.family();
                            let pick =
                                ((heat * 0.75 + flick * (0.15 + 0.55 * RESTLESS)) * family.len() as f32) as usize;
                            let glyph = family[pick.min(family.len() - 1)];
                            self.put(column, line, glyph, alpha * (0.75 + 0.25 * flick), level(y));
                        }
                        Look::Line => self.put(column, line, stroke.plain(), alpha, level(y)),
                    }
                }
            }
        }
        self.strokes = strokes;
    }

    /// Empties the cells of `column` at least half covered by `from`...`to`, in cells.
    fn clear(&mut self, column: usize, from: f32, to: f32) {
        let from = from.max(0.0);
        let to = to.min(self.lines as f32);
        if !(to > from) {
            return;
        }
        for line in from.floor() as usize..to.ceil() as usize {
            if to.min(line as f32 + 1.0) - from.max(line as f32) >= 0.5 {
                self.glyphs[line * self.cols + column] = b' ';
            }
        }
    }

    fn put(&mut self, column: usize, line: i64, glyph: u8, brightness: f32, level: f32) {
        if line < 0 || line as usize >= self.lines {
            return;
        }
        let at = line as usize * self.cols + column;
        self.glyphs[at] = glyph;
        self.brightness[at] = brightness;
        self.levels[at] = level;
    }

    /// Appends the grid to `out` as lines of cells, leaving the cursor where it started.
    pub fn encode(&self, paint: &Paint<'_>, out: &mut Vec<u8>) {
        for line in 0..self.lines {
            if let Some([r, g, b]) = paint.background {
                let _ = write!(out, "\x1b[48;2;{r};{g};{b}m");
            }
            let mut colour = None;
            for at in line * self.cols..(line + 1) * self.cols {
                let glyph = self.glyphs[at];
                if glyph != b' ' {
                    let next = paint.colour(self.brightness[at], self.levels[at], CURVE);
                    if colour != Some(next) {
                        let [r, g, b] = next;
                        let _ = write!(out, "\x1b[38;2;{r};{g};{b}m");
                        colour = Some(next);
                    }
                }
                out.push(glyph);
            }
            out.extend_from_slice(b"\x1b[0m");
            if line + 1 < self.lines {
                out.extend_from_slice(b"\r\n");
            }
        }
        if self.lines > 1 {
            let _ = write!(out, "\x1b[{}A", self.lines - 1);
        }
        out.push(b'\r');
    }
}

/// The columns of `across` dots that `x0`...`x1` touches.
fn columns(x0: f32, x1: f32, across: f32, cols: usize) -> std::ops::Range<usize> {
    let first = (x0 / across).floor().max(0.0) as usize;
    let last = ((x1 / across).ceil().max(0.0) as usize).min(cols);
    first.min(last)..last
}

#[cfg(test)]
mod tests {
    use super::super::theme::Theme;
    use super::*;
    use crate::ridge::{EraseSpan, RowDraw, StrokeSegment};

    /// A row through `points` (in dots), resting on y 40 and rising 40 at full scale,
    /// erasing down to `floor` if given.
    fn push_row(list: &mut DrawList, points: &[[f32; 2]], floor: Option<f32>) {
        let spans = list.spans.len() as u32;
        let segments = list.segments.len() as u32;
        for pair in points.windows(2) {
            if let Some(floor) = floor {
                list.spans.push(EraseSpan {
                    x0: pair[0][0],
                    x1: pair[1][0],
                    top0: pair[0][1],
                    top1: pair[1][1],
                    floor,
                });
            }
            list.segments.push(StrokeSegment { p1: pair[0], p2: pair[1], alpha: 1.0 });
        }
        list.rows.push(RowDraw {
            spans: spans..list.spans.len() as u32,
            segments: segments..list.segments.len() as u32,
            baseline: 40.0,
            rise: 40.0,
            id: list.rows.len() as u64,
        });
    }

    fn text(grid: &Grid) -> Vec<String> {
        grid.glyphs.chunks(grid.cols).map(|line| String::from_utf8(line.to_vec()).unwrap()).collect()
    }

    fn drawn(list: &DrawList, look: Look, cols: usize, lines: usize) -> Vec<String> {
        let mut grid = Grid::new(cols, lines);
        grid.draw(list, look, 10.0);
        text(&grid)
    }

    #[test]
    fn a_flat_line_says_how_high_in_its_cell_it_lies() {
        // A cell is 4 dots tall: a flat row on its first dot, each middle dot and its last.
        for (y, glyph) in [(4.5, '"'), (5.5, '-'), (6.5, '-'), (7.5, '_')] {
            let mut list = DrawList::default();
            push_row(&mut list, &[[0.0, y], [8.0, y]], None);
            let lines = drawn(&list, Look::Line, 4, 3);
            assert_eq!(lines, ["    ".to_string(), glyph.to_string().repeat(4), "    ".to_string()], "{y}");
        }
    }

    #[test]
    fn a_slope_leans_and_a_steep_one_stands_upright() {
        // Half a line up in each of two columns, then four lines within one column,
        // then a crest inside the next.
        let mut list = DrawList::default();
        push_row(
            &mut list,
            &[[0.0, 39.5], [2.0, 37.5], [4.0, 35.5], [6.0, 19.5], [7.0, 17.0], [8.0, 19.5], [10.0, 19.5]],
            None,
        );
        let lines = drawn(&list, Look::Line, 5, 10);
        let column = |at: usize| lines.iter().map(|line| line.as_bytes()[at] as char).collect::<String>();
        assert_eq!(column(0), "         /");
        assert_eq!(column(1), "         /");
        assert_eq!(column(2), "    ||||| ");
        assert_eq!(column(3), "    ^     ");
        assert_eq!(column(4), "    _     ");
    }

    #[test]
    fn a_row_hides_what_is_behind_it_under_its_line() {
        let mut list = DrawList::default();
        // A flat row at the back, then two in front: one below it on the left, one
        // above it on the right.
        push_row(&mut list, &[[0.0, 21.5], [8.0, 21.5]], None);
        push_row(&mut list, &[[0.0, 33.5], [4.0, 33.5]], Some(33.5));
        push_row(&mut list, &[[4.0, 9.5], [8.0, 9.5]], Some(33.5));
        let lines = drawn(&list, Look::Line, 4, 10);
        assert_eq!(lines[5], "--  ");
        assert_eq!(lines[8], "--  ");
        assert_eq!(lines[2], "  --");
    }

    #[test]
    fn static_breaks_the_flats_and_keeps_the_slopes() {
        let mut list = DrawList::default();
        let mut points: Vec<[f32; 2]> = vec![[0.0, 39.5], [100.0, 39.5]];
        // A peak twenty columns wide and nine lines tall.
        points.extend([[120.0, 3.5], [140.0, 39.5], [240.0, 39.5]]);
        push_row(&mut list, &points, None);
        let lines = drawn(&list, Look::Static, 120, 10);
        assert!(lines.iter().all(|line| line.bytes().all(|glyph| (b' '..=b'~').contains(&glyph))));
        // Some of the flat cells are left out, none of the slope's.
        let flat = &lines[9][..50];
        assert!(flat.contains(' ') && flat.bytes().filter(|glyph| *glyph != b' ').count() > 25, "{flat:?}");
        for column in 51..69 {
            assert!(lines.iter().any(|line| line.as_bytes()[column] != b' '), "{column}");
        }
        // The same moment draws the same characters.
        assert_eq!(lines, drawn(&list, Look::Static, 120, 10));
    }

    #[test]
    fn cells_are_written_as_ascii_in_their_rows_colour() {
        let mut list = DrawList::default();
        push_row(&mut list, &[[0.0, 7.5], [4.0, 7.5]], None);
        list.segments[0].alpha = 0.25;
        let mut grid = Grid::new(3, 2);
        grid.draw(&list, Look::Line, 0.0);
        let theme = Theme::terminal();
        let paint =
            Paint { gradient: &theme.gradients[0], top_level: 1.0, full: 1.0, backdrop: [0; 3], background: None };
        let mut out = Vec::new();
        grid.encode(&paint, &mut out);
        let dim = (0.25f32.powf(CURVE) * 255.0 + 0.5) as u8;
        assert_eq!(
            String::from_utf8(out).unwrap(),
            format!("   \x1b[0m\r\n\x1b[38;2;{dim};{dim};{dim}m__ \x1b[0m\x1b[1A\r")
        );
    }
}
