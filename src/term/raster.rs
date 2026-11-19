//! A CPU rasteriser for the ridge's draw list onto a grid of dots: rows are painted
//! back to front, each clearing the dots under its line and then lighting the line's own.
//! A dot at least half covered counts as covered.

use crate::ridge::DrawList;

pub struct Canvas {
    pub width: usize,
    pub height: usize,
    /// Each dot's brightness, row by row from the top: 0 when off, else its row's alpha.
    pub pixels: Vec<f32>,
    /// Each lit dot's height above its row's baseline as a share of the row's rise, 0...1.
    pub levels: Vec<f32>,
    /// Per column: the top of the current row's erase.
    top: Vec<f32>,
    /// Per column: the highest and lowest the current row's line reaches.
    low: Vec<f32>,
    high: Vec<f32>,
}

impl Canvas {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            pixels: vec![0.0; width * height],
            levels: vec![0.0; width * height],
            top: vec![0.0; width],
            low: vec![0.0; width],
            high: vec![0.0; width],
        }
    }

    /// Clears the canvas and draws `list` with strokes `half_width` dots either side of a line.
    pub fn draw(&mut self, list: &DrawList, half_width: f32) {
        self.pixels.fill(0.0);
        for row in &list.rows {
            let spans = &list.spans[row.spans.start as usize..row.spans.end as usize];
            if let Some(first) = spans.first() {
                let floor = first.floor;
                self.top.fill(f32::INFINITY);
                for span in spans {
                    if !(span.x1 > span.x0) {
                        continue;
                    }
                    for column in columns(span.x0, span.x1, self.width) {
                        let x = (column as f32 + 0.5).clamp(span.x0, span.x1);
                        let t = (x - span.x0) / (span.x1 - span.x0);
                        let y = span.top0 + t * (span.top1 - span.top0);
                        self.top[column] = self.top[column].min(y);
                    }
                }
                for column in 0..self.width {
                    let top = self.top[column];
                    if top < floor {
                        self.cover(column, top, floor, 0.0, None);
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
                for column in columns(x1, x2, self.width) {
                    let left = (column as f32).max(x1);
                    let right = (column as f32 + 1.0).min(x2);
                    let (a, b) = (y1 + (left - x1) * slope, y1 + (right - x1) * slope);
                    self.low[column] = self.low[column].min(a.min(b));
                    self.high[column] = self.high[column].max(a.max(b));
                }
            }
            for column in 0..self.width {
                let (low, high) = (self.low[column], self.high[column]);
                if low <= high {
                    self.cover(column, low - half_width, high + half_width, alpha, Some((row.baseline, row.rise)));
                }
            }
        }
    }

    /// Sets each dot of `column` at least half covered by `from`...`to` to `value`.
    fn cover(&mut self, column: usize, from: f32, to: f32, value: f32, row: Option<(f32, f32)>) {
        let from = from.max(0.0);
        let to = to.min(self.height as f32);
        if !(to > from) {
            return;
        }
        let first = from.floor() as usize;
        let last = (to.ceil() as usize).min(self.height);
        for y in first..last {
            let share = to.min(y as f32 + 1.0) - from.max(y as f32);
            if share >= 0.5 {
                let at = y * self.width + column;
                self.pixels[at] = value;
                self.levels[at] = match row {
                    Some((baseline, rise)) if rise > 0.0 => ((baseline - (y as f32 + 0.5)) / rise).clamp(0.0, 1.0),
                    _ => 0.0,
                };
            }
        }
    }
}

/// The columns `x0`...`x1` touches.
fn columns(x0: f32, x1: f32, width: usize) -> std::ops::Range<usize> {
    let first = x0.floor().max(0.0) as usize;
    let last = (x1.ceil().max(0.0) as usize).min(width);
    first.min(last)..last
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ridge::{EraseSpan, RowDraw, StrokeSegment};

    /// A row whose line runs straight from `a` to `b`, erasing down to `floor` if given.
    fn push_row(list: &mut DrawList, a: [f32; 2], b: [f32; 2], alpha: f32, floor: Option<f32>) {
        let spans = list.spans.len() as u32;
        if let Some(floor) = floor {
            list.spans.push(EraseSpan { x0: a[0], x1: b[0], top0: a[1], top1: b[1], floor });
        }
        let segments = list.segments.len() as u32;
        list.segments.push(StrokeSegment { p1: a, p2: b, alpha });
        list.rows.push(RowDraw {
            spans: spans..list.spans.len() as u32,
            segments: segments..list.segments.len() as u32,
            // Every test row rests on y 10 and rises 10 at full scale.
            baseline: 10.0,
            rise: 10.0,
        });
    }

    fn lit(canvas: &Canvas, x: usize) -> Vec<usize> {
        (0..canvas.height).filter(|y| canvas.pixels[y * canvas.width + x] > 0.0).collect()
    }

    #[test]
    fn a_flat_line_is_one_dot_thick() {
        let mut list = DrawList::default();
        push_row(&mut list, [0.0, 4.6], [8.0, 4.6], 0.8, None);
        let mut canvas = Canvas::new(8, 10);
        canvas.draw(&list, 0.5);
        for x in 0..8 {
            assert_eq!(lit(&canvas, x), [4]);
            assert_eq!(canvas.pixels[4 * 8 + x], 0.8);
        }
    }

    #[test]
    fn a_steep_line_fills_every_dot_it_passes() {
        let mut list = DrawList::default();
        push_row(&mut list, [1.0, 1.2], [2.0, 8.8], 1.0, None);
        let mut canvas = Canvas::new(4, 10);
        canvas.draw(&list, 0.5);
        assert_eq!(lit(&canvas, 1), [1, 2, 3, 4, 5, 6, 7, 8]);
        // Each dot's level is its own height above the baseline at y 10.
        let level = |y: usize| canvas.levels[y * 4 + 1];
        assert!((level(1) - 0.85).abs() < 1e-6 && (level(8) - 0.15).abs() < 1e-6);
        assert!(lit(&canvas, 0).is_empty() && lit(&canvas, 2).is_empty());
    }

    #[test]
    fn a_row_hides_what_is_behind_it_under_its_line() {
        let mut list = DrawList::default();
        // A flat row at the back, then a row in front that rises over it
        // across the right half.
        push_row(&mut list, [0.0, 5.5], [8.0, 5.5], 1.0, None);
        push_row(&mut list, [0.0, 8.5], [4.0, 8.5], 1.0, Some(8.5));
        push_row(&mut list, [4.0, 2.5], [8.0, 2.5], 1.0, Some(8.5));
        let mut canvas = Canvas::new(8, 10);
        canvas.draw(&list, 0.5);
        assert_eq!(lit(&canvas, 1), [5, 8]);
        assert_eq!(lit(&canvas, 6), [2]);
    }

    #[test]
    fn lines_off_the_canvas_are_clipped() {
        let mut list = DrawList::default();
        push_row(&mut list, [-5.0, -3.0], [20.0, 30.0], 1.0, Some(40.0));
        let mut canvas = Canvas::new(6, 6);
        canvas.draw(&list, 0.5);
        assert!(canvas.pixels.iter().all(|value| (0.0..=1.0).contains(value)));
    }
}
