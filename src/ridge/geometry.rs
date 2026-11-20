//! Turns a `RidgeFrame` into a draw list. Each row erases where its line rises above
//! its floor and strokes every segment unless its alpha is 0.

use std::ops::Range;

use super::{
    RidgeFrame,
    layout::{self, RidgeTarget, RowPlacement},
};

/// One trapezoid of a row's erase, in device pixels: `x0` to `x1`, between the row's
/// line on top and its floor.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EraseSpan {
    pub x0: f32,
    pub x1: f32,
    /// Top edge y at `x0` and `x1`.
    pub top0: f32,
    pub top1: f32,
    pub floor: f32,
}

/// One line segment `p1`-`p2` of a row's stroke.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StrokeSegment {
    pub p1: [f32; 2],
    pub p2: [f32; 2],
    pub alpha: f32,
}

/// The ranges of one row in a `DrawList`: its erase, then its stroke.
#[derive(Clone, Debug, PartialEq)]
pub struct RowDraw {
    pub spans: Range<u32>,
    pub segments: Range<u32>,
    /// Y of the row's resting line, and the pixels a full-scale point rises above it.
    pub baseline: f32,
    pub rise: f32,
    /// Identifies the row's levels; unchanged as the row moves back through the stack.
    /// A live row that stands still keeps `STANDING_ID`.
    pub id: u64,
}

/// The id of a live row that stands on the front baseline.
const STANDING_ID: u64 = u64::MAX;

/// Everything one frame of the ridge draws, rows back to front. Reused between frames.
#[derive(Default)]
pub struct DrawList {
    pub spans: Vec<EraseSpan>,
    pub segments: Vec<StrokeSegment>,
    pub rows: Vec<RowDraw>,
}

#[derive(Default)]
pub struct GeometryBuilder {
    placements: Vec<RowPlacement>,
    /// One row's points at a time.
    points: Vec<[f32; 2]>,
}

impl GeometryBuilder {
    pub fn build(&mut self, frame: Option<&RidgeFrame<'_>>, target: RidgeTarget, list: &mut DrawList) {
        list.spans.clear();
        list.segments.clear();
        list.rows.clear();
        let Some(frame) = frame else { return };
        let n = frame.edge.len();
        if n < 2 || frame.rows == 0 {
            return;
        }
        layout::place(frame, target, &mut self.placements);
        if self.placements.is_empty() {
            return;
        }
        let p = &frame.layout;
        let field_w = f64::from(target.pixel_width) * p.ridge_span;
        let field_x = (f64::from(target.pixel_width) - field_w) / 2.0;
        let step = field_w / (n - 1) as f64;
        self.points.resize(n, [0.0; 2]);

        for place in &self.placements {
            let values = frame.levels(place.row);
            for (i, point) in self.points.iter_mut().enumerate() {
                let level = values.get(i).copied().map_or(0.0, f64::from);
                let y = place.baseline - level * f64::from(frame.edge[i]) * place.rise;
                *point = [(field_x + i as f64 * step) as f32, y as f32];
            }
            let span_start = list.spans.len() as u32;
            if let Some(floor) = place.floor {
                erase(&self.points, floor, &mut list.spans);
            }
            let segment_start = list.segments.len() as u32;
            if place.alpha > 0.0 {
                stroke(&self.points, place.alpha as f32, &mut list.segments);
            }
            list.rows.push(RowDraw {
                spans: span_start..list.spans.len() as u32,
                segments: segment_start..list.segments.len() as u32,
                baseline: place.baseline as f32,
                rise: place.rise as f32,
                id: if place.row == 0 && !p.ridge_rising {
                    STANDING_ID
                } else {
                    frame.cuts.wrapping_sub(place.row as u64)
                },
            });
        }
    }
}

/// One span per segment with a point above `floor`, cut where the line crosses it.
fn erase(points: &[[f32; 2]], floor: f64, out: &mut Vec<EraseSpan>) {
    let f = floor as f32;
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let (ay, by) = (f64::from(a[1]), f64::from(b[1]));
        let (a_above, b_above) = (ay < floor, by < floor);
        if !a_above && !b_above {
            continue;
        }
        let mut span = EraseSpan { x0: a[0], x1: b[0], top0: a[1], top1: b[1], floor: f };
        if a_above != b_above {
            let (ax, bx) = (f64::from(a[0]), f64::from(b[0]));
            let t = (floor - ay) / (by - ay);
            let crossing = (ax + t * (bx - ax)) as f32;
            if a_above {
                span.x1 = crossing;
                span.top1 = f;
            } else {
                span.x0 = crossing;
                span.top0 = f;
            }
        }
        out.push(span);
    }
}

fn stroke(points: &[[f32; 2]], alpha: f32, out: &mut Vec<StrokeSegment>) {
    out.extend(points.windows(2).map(|pair| StrokeSegment { p1: pair[0], p2: pair[1], alpha }));
}

#[cfg(test)]
mod tests {
    use super::super::{FrameSource, Ridge, RidgeParams};
    use super::*;

    struct Tone;

    impl FrameSource for Tone {
        fn width(&self) -> usize {
            128
        }

        fn read(&self, _: f64, _: f64, out: &mut [f32]) -> bool {
            for (i, level) in out.iter_mut().enumerate() {
                *level = if i % 64 == 20 { 1.0 } else { 0.0 };
            }
            true
        }
    }

    #[test]
    fn a_row_keeps_its_id_as_it_moves_back() {
        for rising in [false, true] {
            let mut ridge = Ridge::new(RidgeParams { ridge_rising: rising, ..RidgeParams::default() }, 1);
            ridge.keep_moving = true;
            let target = RidgeTarget { pixel_width: 2560, pixel_height: 1440, scale: 2.0 };
            let mut builder = GeometryBuilder::default();
            let mut list = DrawList::default();
            let mut now = 5.0;
            let mut seen: Vec<(u64, Vec<u64>)> = Vec::new();
            for _ in 0..240 {
                now += 1.0 / 120.0;
                ridge.advance(now, Some(&Tone), 720.0);
                let frame = ridge.frame(now).unwrap();
                builder.build(Some(&frame), target, &mut list);
                seen.push((frame.cuts, list.rows.iter().map(|row| row.id).collect()));
            }
            let (first_cuts, first) = &seen[120];
            let (last_cuts, last) = seen.last().unwrap();
            assert!(last_cuts > first_cuts);
            // Back to front: the rows behind the live one are the same rows, further back.
            let behind = &first[first.len() - 4..first.len() - 1];
            assert!(last.windows(3).any(|ids| ids == behind), "{behind:?} in {last:?}");
            let live = (*first.last().unwrap(), *last.last().unwrap());
            if rising {
                // A rising live row becomes the next history row.
                assert_eq!(live, (*first_cuts, *last_cuts));
            } else {
                assert_eq!(live, (STANDING_ID, STANDING_ID));
            }
        }
    }

    #[test]
    fn a_frame_becomes_rows_of_spans_and_segments() {
        let mut ridge = Ridge::new(RidgeParams::default(), 1);
        let mut now = 5.0;
        for _ in 0..240 {
            now += 1.0 / 120.0;
            ridge.advance(now, Some(&Tone), 720.0);
        }
        let frame = ridge.frame(now).unwrap();
        let target = RidgeTarget { pixel_width: 2560, pixel_height: 1440, scale: 2.0 };
        let mut list = DrawList::default();
        GeometryBuilder::default().build(Some(&frame), target, &mut list);

        let fine = frame.edge.len();
        assert_eq!(fine, 63 * 5 + 1);
        // The live row and every history row short of the back slot.
        assert!(list.rows.len() >= 50 && list.rows.len() <= 51, "{}", list.rows.len());
        // The front row is last, strokes every segment and erases where the
        // peak stands.
        let front = list.rows.last().unwrap();
        assert_eq!(front.segments.len(), fine - 1);
        assert!(!front.spans.is_empty());
        let segments = &list.segments[front.segments.start as usize..front.segments.end as usize];
        assert_eq!(segments[0].p1[0], 0.0);
        assert_eq!(segments[fine - 2].p2[0], 2560.0);
        // Rows sit a whole number of pixels apart.
        let mut placements = Vec::new();
        layout::place(&frame, target, &mut placements);
        for pair in placements.windows(2) {
            let gap = pair[1].baseline - pair[0].baseline;
            assert!((gap - gap.round()).abs() < 1e-9 && gap > 0.0);
        }
        // Every span lies above its floor.
        assert!(list.spans.iter().all(|s| s.top0 <= s.floor && s.top1 <= s.floor && s.x0 <= s.x1));
    }
}
