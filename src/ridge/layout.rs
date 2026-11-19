//! Places the rows of a `RidgeFrame` on a target's pixel grid: the front row's stroke
//! has its upper edge on a pixel boundary and rows sit a whole number of pixels apart.
//! Between cuts the history stack moves `RidgeFrame::phase` of a slot, to the nearest whole pixel.

use super::{
    RidgeFrame,
    rows::{js_round, ridge_row},
};

/// A render target in whole device pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RidgeTarget {
    pub pixel_width: u32,
    pub pixel_height: u32,
    /// Device pixels per point.
    pub scale: f64,
}

impl RidgeTarget {
    /// Height in points.
    pub fn height(&self) -> f64 {
        f64::from(self.pixel_height) / self.scale
    }
}

/// Where one drawn row sits, in device pixels from the top left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RowPlacement {
    /// Stack row, numbered as `RidgeFrame::levels` numbers them.
    pub row: usize,
    /// Y of the row's resting line.
    pub baseline: f64,
    /// Pixels a full-scale point rises above the baseline.
    pub rise: f64,
    pub alpha: f64,
    /// Lowest y the row's erase reaches, a pixel below the stroke of the row behind;
    /// `None` for the farthest drawn row.
    pub floor: Option<f64>,
}

/// Placements for every row to draw, back to front, into `out`.
pub fn place(frame: &RidgeFrame<'_>, target: RidgeTarget, out: &mut Vec<RowPlacement>) {
    out.clear();
    let rows = frame.rows;
    if rows == 0 || target.pixel_width == 0 || target.pixel_height == 0 || !(target.scale > 0.0) {
        return;
    }
    let p = &frame.layout;
    let scale = target.scale;
    let h = target.height();
    let half_stroke = p.ridge_line_width * scale / 2.0;
    let front_y = ridge_row(0.0, h, p).baseline;
    let back_y = ridge_row(if rows > 1 { 1.0 } else { 0.0 }, h, p).baseline;
    let front = js_round(front_y * scale - half_stroke) + half_stroke;
    let pitch = if rows > 1 { js_round((front_y - back_y) / (rows - 1) as f64 * scale) } else { 0.0 };

    let mut append = |row: usize, depth: f64, baseline: f64| {
        let g = ridge_row(depth, h, p);
        out.push(RowPlacement { row, baseline, rise: g.scale * scale, alpha: g.alpha, floor: None });
    };

    // Front to back first. The live row stands still.
    append(0, 0.0, front);
    if rows > 1 {
        let span = (rows - 1) as f64;
        let phase = frame.phase.clamp(0.0, 1.0);
        let shift = js_round(phase * pitch);
        for j in 0..frame.history.rows() {
            let depth = (j as f64 + phase) / span;
            if depth > 1.0 {
                break;
            }
            if j == 0 && shift < p.ridge_emerge {
                continue;
            }
            append(j + 1, depth, front - j as f64 * pitch - shift);
        }
    }
    for i in 0..out.len().saturating_sub(1) {
        out[i].floor = Some(out[i].baseline.min(out[i + 1].baseline + half_stroke + 1.0));
    }
    out.reverse();
}

#[cfg(test)]
mod tests {
    use super::super::{FrameSource, Ridge, RidgeParams};
    use super::*;

    /// Every band at rest: a stack of flat rows.
    struct Flat;

    impl FrameSource for Flat {
        fn width(&self) -> usize {
            128
        }

        fn read(&self, _: f64, _: f64, out: &mut [f32]) -> bool {
            out.fill(0.0);
            true
        }
    }

    /// A 1100 by 640 point target at 2x: rows 16 px apart.
    const TARGET: RidgeTarget = RidgeTarget { pixel_width: 2200, pixel_height: 1280, scale: 2.0 };
    const PITCH: f64 = 16.0;

    /// How far the history stack stands above its slots, in pixels, on each of 240
    /// paints at 120 a second.
    fn shifts() -> Vec<f64> {
        let mut ridge = Ridge::new(RidgeParams::default(), 1);
        ridge.keep_moving = true;
        let mut placements = Vec::new();
        let mut out = Vec::new();
        for i in 1..=240 {
            let now = 50.0 + f64::from(i) / 120.0;
            assert!(ridge.advance(now, Some(&Flat), TARGET.height()));
            place(&ridge.frame(now).unwrap(), TARGET, &mut placements);
            // Back to front: the live row is last.
            let live = placements.last().unwrap();
            assert_eq!(live.row, 0);
            assert_eq!(live.baseline.fract(), 0.25, "the stroke's upper edge is on a pixel boundary");
            let deep = placements.iter().find(|placed| placed.row == 10).unwrap();
            out.push(live.baseline - 9.0 * PITCH - deep.baseline);
            // The rows keep their spacing wherever the stack stands.
            for pair in placements.windows(2).filter(|pair| pair[1].row > 1) {
                assert!((pair[1].baseline - pair[0].baseline - PITCH).abs() < 1e-9);
            }
        }
        out
    }

    /// Pixels the stack moved between two paints, across a cut or not.
    fn step(from: f64, to: f64) -> f64 {
        (to - from).rem_euclid(PITCH)
    }

    #[test]
    fn the_stack_glides_in_whole_pixels() {
        let shifts = shifts();
        assert!(shifts.iter().all(|shift| shift.fract() == 0.0));
        // 33 ms rows are a little under four paints, so most steps are 4 px
        // and now and then one is 5.
        let steps: Vec<f64> = shifts.windows(2).map(|pair| step(pair[0], pair[1])).collect();
        assert!(steps.iter().all(|&px| px == 4.0 || px == 5.0), "{steps:?}");
        let long = steps.iter().filter(|&&px| px == 5.0).count();
        assert!((8..=11).contains(&long), "{long}");
    }
}
