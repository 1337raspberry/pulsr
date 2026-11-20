//! The ridge's state over time: reads the frame source, shapes and eases the levels,
//! cuts history rows and decides whether the picture changed. Everything runs on time,
//! not display frames, so the picture is the same at any refresh rate.

mod geometry;
mod layout;
mod params;
mod rows;

pub use geometry::{DrawList, GeometryBuilder};
#[cfg(test)]
pub use geometry::{EraseSpan, StrokeSegment};
#[cfg(test)]
pub use geometry::RowDraw;
pub use layout::RidgeTarget;
pub use params::{RidgeParams, ease_alpha, sync_lead};
pub use rows::RidgeHistory;

use rows::{
    RidgeRandom, RidgeScratch, apply_grain, axis_samples, edge_window, mix_bands, resample_row,
    smooth_row, spread_row,
};

/// Where the ridge reads its levels from.
pub trait FrameSource {
    /// Values per frame: every band of every channel.
    fn width(&self) -> usize;

    /// Reads the levels (0...1; left bands, then right) for `lead_sec` seconds after `now`.
    /// False when there is no frame for that moment.
    fn read(&self, now: f64, lead_sec: f64, out: &mut [f32]) -> bool;
}

/// Channels per frame (left, right).
const CHANNELS: usize = 2;
/// Bands per channel assumed until a source says otherwise.
const DEFAULT_BANDS: usize = 64;
/// A point that rises less than this many points on the tallest row counts as flat.
const MIN_VISIBLE_HEIGHT: f64 = 0.5;
/// A decaying level this small is silence.
const SILENCE: f32 = 1e-6;

/// Longest stretch of time one sub-step covers, in seconds.
pub const MAX_STEP: f64 = 1.0 / 240.0;
/// A gap between two `advance` calls longer than this is a stall: the picture resumes
/// where it stopped instead of catching up.
pub const STALL: f64 = 0.25;

/// Everything one paint of the ridge reads, taken after an `advance`.
pub struct RidgeFrame<'a> {
    /// Rows in the stack, the live front row included.
    pub rows: usize,
    /// The live row, one level per fine-grid point.
    pub live: &'a [f32],
    /// Past rows, newest first.
    pub history: &'a RidgeHistory,
    /// Per-point edge multiplier (the taper window).
    pub edge: &'a [f32],
    /// The layout the rows are drawn with.
    pub layout: RidgeParams,
    /// Fraction of a row interval the stack has moved since the latest cut (0 to 1).
    pub phase: f64,
    /// History rows cut so far.
    pub cuts: u64,
}

impl RidgeFrame<'_> {
    /// Levels of stack row `k`: 0 is the live row, `k >= 1` history row `k - 1`.
    pub fn levels(&self, k: usize) -> &[f32] {
        if k == 0 { self.live } else { self.history.get(k - 1) }
    }
}

pub struct Ridge {
    pub params: RidgeParams,
    /// Seconds ahead of the audible position each step reads the source (`sync_lead`).
    pub lead_sec: f64,
    /// Never come to rest: the stack keeps scrolling through silence.
    pub keep_moving: bool,

    /// The eased level of each band, 0...1, bass first.
    levels: Vec<f32>,
    /// Tallest eased level after the latest step.
    peak: f32,

    // Per-band buffers: the frame as read, its channels mixed, the mix
    // after the level curve, and the widened and neighbour-blended copies.
    raw: Vec<f32>,
    target: Vec<f32>,
    shaped: Vec<f32>,
    spread: Vec<f32>,
    smoothed: Vec<f32>,

    // On the fine grid the eased row is resampled to. The grain is frozen into each
    // history row when it is cut, then re-rolled, so every row has its own texture.
    fine: Vec<f32>,
    axis: Vec<f32>,
    axis_curve: f64,
    taper: Vec<f32>,
    taper_amount: f64,
    grain: Vec<f32>,
    grained: Vec<f32>,
    history: Option<RidgeHistory>,
    random: RidgeRandom,
    scratch: RidgeScratch,

    /// Time of the previous `advance`.
    last_step_at: Option<f64>,
    /// Time the latest history row was cut.
    last_row_at: f64,
    /// History rows cut so far.
    cuts: u64,
    /// Since when the live line has been flat; `None` while it isn't.
    quiet_since: Option<f64>,
    /// At rest: every row flat and in its slot, no rows being cut.
    idle: bool,
    /// Row count of the last paint.
    painted_rows: usize,
    /// Rows the last `advance` prepared; 0 when there is nothing to draw.
    draw_rows: usize,
}

impl Ridge {
    pub fn new(params: RidgeParams, seed: u64) -> Self {
        let n = DEFAULT_BANDS;
        Self {
            params,
            lead_sec: 0.0,
            keep_moving: false,
            levels: vec![0.0; n],
            peak: 0.0,
            raw: vec![0.0; n * CHANNELS],
            target: vec![0.0; n],
            shaped: vec![0.0; n],
            spread: vec![0.0; n],
            smoothed: vec![0.0; n],
            fine: Vec::new(),
            axis: Vec::new(),
            axis_curve: f64::NAN,
            taper: Vec::new(),
            taper_amount: f64::NAN,
            grain: Vec::new(),
            grained: Vec::new(),
            history: None,
            random: RidgeRandom::new(seed),
            scratch: RidgeScratch::default(),
            last_step_at: None,
            last_row_at: 0.0,
            cuts: 0,
            quiet_since: None,
            idle: false,
            painted_rows: 0,
            draw_rows: 0,
        }
    }

    pub fn row_count(&self) -> usize {
        self.params.ridge_rows.max(1)
    }

    /// Seconds between history rows.
    pub fn row_period(&self) -> f64 {
        (self.params.ridge_row_ms / 1000.0).max(1e-3)
    }

    #[cfg(test)]
    pub fn levels(&self) -> &[f32] {
        &self.levels
    }

    #[cfg(test)]
    pub fn is_idle(&self) -> bool {
        self.idle
    }

    /// The level curve: zero at or below `ridge_floor_cut` with the rest rescaled, raised
    /// to `ridge_gamma`, times `ridge_gain`, clamped to 1.
    pub fn shape(level: f32, p: &RidgeParams) -> f32 {
        if !(p.ridge_floor_cut > 0.0 || p.ridge_gamma != 1.0 || p.ridge_gain != 1.0) {
            return level;
        }
        let mut v = f64::from(level);
        v = if v <= p.ridge_floor_cut {
            0.0
        } else {
            (v - p.ridge_floor_cut) / (1.0 - p.ridge_floor_cut)
        };
        if p.ridge_gamma != 1.0 {
            v = v.powf(p.ridge_gamma);
        }
        (v * p.ridge_gain).min(1.0) as f32
    }

    /// Whether a line peaking at `peak` rises under `MIN_VISIBLE_HEIGHT` on the tallest row.
    fn is_flat(&self, peak: f32, height: f64) -> bool {
        let rise = height * self.params.ridge_peak * self.params.ridge_depth_scale.max(1.0);
        f64::from(peak) * rise < MIN_VISIBLE_HEIGHT
    }

    /// Brings the ridge up to `now` (seconds on the source's clock). `height` is the frame
    /// height in points. Returns true when the picture must be repainted from `frame`.
    pub fn advance(&mut self, now: f64, source: Option<&dyn FrameSource>, height: f64) -> bool {
        self.size_buffers(source);
        let rows = self.row_count();
        let row_period = self.row_period();

        let prev = match self.last_step_at {
            Some(prev) if now >= prev && now - prev <= STALL => prev,
            Some(prev) => {
                // A stall, or the clock stepped back: carry the timers
                // across the gap so the picture resumes where it stopped.
                let gap = now - prev;
                self.last_row_at += gap;
                if let Some(quiet) = &mut self.quiet_since {
                    *quiet += gap;
                }
                now
            }
            None => {
                self.last_row_at = now;
                now
            }
        };
        self.last_step_at = Some(now);

        if self.idle && self.keep_moving {
            self.idle = false;
            self.last_row_at = now;
        }
        if self.idle {
            self.step(now, now - prev, source, height);
            if self.quiet_since.is_none() {
                // Something moved: the stack sets off from rest.
                self.idle = false;
                self.last_row_at = now;
            } else if rows == self.painted_rows {
                return false;
            }
        } else {
            let mut t = prev;
            loop {
                let next_cut = self.last_row_at + row_period;
                let mut to = (t + MAX_STEP).min(now);
                let cut = next_cut <= to;
                if cut {
                    to = next_cut.max(t);
                }
                self.step(to, to - t, source, height);
                t = to;
                if cut {
                    self.cut_row();
                    self.last_row_at = next_cut;
                    // Flat for longer than the stack takes to carry a row off the top means
                    // every row is flat. On a cut each row sits in its slot, so the stack
                    // rests here.
                    let limit = (rows + 1) as f64 * row_period;
                    if !self.keep_moving && self.quiet_since.is_some_and(|quiet| t - quiet > limit) {
                        self.idle = true;
                        break;
                    }
                }
                if t >= now {
                    break;
                }
            }
        }

        self.painted_rows = rows;
        self.draw_rows = 0;
        // One point has no interval to resample.
        if self.levels.len() < 2 {
            return true;
        }
        resample_row(&self.levels, &self.axis, &mut self.fine, &mut self.scratch);
        apply_grain(&self.fine, &mut self.grained, &self.grain, self.params.ridge_grain);
        self.draw_rows = rows;
        true
    }

    /// The state the last `advance` prepared; `now` is the time passed to it.
    /// `None` before the first paint or with fewer than two bands.
    pub fn frame(&self, now: f64) -> Option<RidgeFrame<'_>> {
        if self.draw_rows == 0 {
            return None;
        }
        let history = self.history.as_ref()?;
        let phase = if self.idle { 0.0 } else { ((now - self.last_row_at) / self.row_period()).clamp(0.0, 1.0) };
        Some(RidgeFrame {
            rows: self.draw_rows,
            live: &self.grained,
            history,
            edge: &self.taper,
            layout: self.params.clone(),
            phase,
            cuts: self.cuts,
        })
    }

    /// One sub-step: reads the source for time `t` and eases the levels over `dt` seconds.
    fn step(&mut self, t: f64, dt: f64, source: Option<&dyn FrameSource>, height: f64) {
        let have_frame = source.is_some_and(|source| source.read(t, self.lead_sec, &mut self.raw));
        if !have_frame && self.peak == 0.0 {
            // Silence in, silence held: nothing to compute.
            if self.quiet_since.is_none() {
                self.quiet_since = Some(t);
            }
            return;
        }
        if have_frame {
            mix_bands(&self.raw, CHANNELS, &mut self.target);
        } else {
            self.target.fill(0.0);
        }

        let p = &self.params;
        for (shaped, &level) in self.shaped.iter_mut().zip(&self.target) {
            *shaped = Self::shape(level, p);
        }
        // Spread and blend after the curve: a floor cut can leave one band standing as
        // a needle, which the spread widens and the blend softens.
        let mut input: &[f32] = &self.shaped;
        if p.ridge_spread > 0.0 {
            spread_row(input, &mut self.spread, p.ridge_spread, &mut self.scratch);
            input = &self.spread;
        }
        if p.ridge_smooth > 0.0 {
            smooth_row(input, &mut self.smoothed, p.ridge_smooth);
            input = &self.smoothed;
        }
        let alpha_attack = ease_alpha(p.ridge_attack, dt);
        let alpha_decay = ease_alpha(p.ridge_decay, dt);
        let mut peak = 0f32;
        for (eased, &level) in self.levels.iter_mut().zip(input) {
            let prev = f64::from(*eased);
            let level = f64::from(level);
            let alpha = if level > prev { alpha_attack } else { alpha_decay };
            let mut value = (prev + (level - prev) * alpha) as f32;
            if !have_frame && value < SILENCE {
                value = 0.0;
            }
            *eased = value;
            peak = peak.max(value);
        }
        self.peak = peak;

        if !self.is_flat(peak, height) {
            self.quiet_since = None;
        } else if self.quiet_since.is_none() {
            self.quiet_since = Some(t);
        }
    }

    fn cut_row(&mut self) {
        if self.levels.len() < 2 {
            return;
        }
        resample_row(&self.levels, &self.axis, &mut self.fine, &mut self.scratch);
        apply_grain(&self.fine, &mut self.grained, &self.grain, self.params.ridge_grain);
        if let Some(history) = &mut self.history {
            history.push(&self.grained);
            self.cuts += 1;
        }
        self.random.reroll(&mut self.grain);
    }

    /// Nothing is allocated unless the source's band count or the parameters changed.
    fn size_buffers(&mut self, source: Option<&dyn FrameSource>) {
        if let Some(width) = source.map(|source| source.width())
            && width != self.raw.len()
        {
            let points = width / CHANNELS;
            // The eased levels restart from silence.
            self.raw = vec![0.0; width];
            self.levels = vec![0.0; points];
            self.target = vec![0.0; points];
            self.shaped = vec![0.0; points];
            self.spread = vec![0.0; points];
            self.smoothed = vec![0.0; points];
            self.peak = 0.0;
        }
        let points = self.levels.len();
        if points < 2 {
            return;
        }
        let per_band = self.params.ridge_oversample.clamp(1, 8);
        let fine = (points - 1) * per_band + 1;
        let resized = self.fine.len() != fine;
        if resized {
            self.fine = vec![0.0; fine];
            self.grain = vec![0.0; fine];
            self.grained = vec![0.0; fine];
            self.axis = vec![0.0; fine];
            self.random.reroll(&mut self.grain);
        }
        let rows = self.row_count();
        if self.history.as_ref().is_none_or(|h| h.rows() != rows || h.width() != fine) {
            self.history = Some(RidgeHistory::new(rows, fine));
        }
        if resized || self.taper_amount != self.params.ridge_edge_taper {
            self.taper = edge_window(fine, self.params.ridge_edge_taper);
            self.taper_amount = self.params.ridge_edge_taper;
        }
        if resized || self.axis_curve != self.params.ridge_axis_curve {
            axis_samples(points, self.params.ridge_axis_curve, &mut self.axis);
            self.axis_curve = self.params.ridge_axis_curve;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A source that is a pure function of time: a few peaks that swell,
    /// fade and drift across the bands, with a hard onset twice a second.
    struct Synthetic;

    impl FrameSource for Synthetic {
        fn width(&self) -> usize {
            DEFAULT_BANDS * CHANNELS
        }

        fn read(&self, now: f64, lead_sec: f64, out: &mut [f32]) -> bool {
            let t = now + lead_sec;
            let n = DEFAULT_BANDS;
            for k in 0..n {
                let x = k as f64 / (n - 1) as f64;
                let sweep = 0.5 + 0.4 * (t * 1.3).sin();
                let bump = (-((x - sweep) * 9.0).powi(2)).exp();
                let beat = (-(t.rem_euclid(0.5)) * 9.0).exp();
                let kick = beat * (-(x * 6.0).powi(2)).exp();
                let level = (0.62 + 0.36 * bump.max(kick) * (0.75 + 0.25 * (t * 5.0).sin())) as f32;
                out[k] = level;
                out[n + k] = level;
            }
            true
        }
    }

    struct Silent;

    impl FrameSource for Silent {
        fn width(&self) -> usize {
            DEFAULT_BANDS * CHANNELS
        }

        fn read(&self, _: f64, _: f64, _: &mut [f32]) -> bool {
            false
        }
    }

    /// Run a ridge from time 100 for `seconds` at `fps` display frames a
    /// second, each frame's time jittered by up to `jitter` of a frame.
    fn run(fps: f64, seconds: f64, jitter: f64) -> Ridge {
        let mut ridge = Ridge::new(RidgeParams::default(), 7);
        ridge.lead_sec = 0.02;
        let start = 100.0;
        ridge.advance(start, Some(&Synthetic), 720.0);
        let frames = (seconds * fps).round() as usize;
        let mut noise: u64 = 0x1234_5678_9ABC_DEF1;
        for i in 1..=frames {
            noise ^= noise << 13;
            noise ^= noise >> 7;
            noise ^= noise << 17;
            let wobble = if i == frames { 0.0 } else { (noise >> 11) as f64 / (1u64 << 53) as f64 * jitter };
            ridge.advance(start + (i as f64 + wobble) / fps, Some(&Synthetic), 720.0);
        }
        ridge
    }

    /// Rows cut since a ridge that has not rested or stalled first advanced, at `start`.
    fn rows_cut(ridge: &Ridge, start: f64) -> u64 {
        ((ridge.last_row_at - start) / ridge.row_period()).round() as u64
    }

    fn worst_difference(a: &[f32], b: &[f32]) -> f32 {
        assert_eq!(a.len(), b.len());
        a.iter().zip(b).fold(0.0, |worst, (x, y)| worst.max((x - y).abs()))
    }

    #[test]
    fn the_picture_does_not_depend_on_the_refresh_rate() {
        let reference = run(240.0, 3.0, 0.0);
        assert_eq!(rows_cut(&reference, 100.0), 90);
        for fps in [24.0, 30.0, 50.0, 60.0, 75.0, 90.0, 120.0, 144.0, 165.0, 360.0, 480.0] {
            let ridge = run(fps, 3.0, 0.0);
            assert_eq!(rows_cut(&ridge, 100.0), 90, "{fps} Hz");
            let live = worst_difference(ridge.levels(), reference.levels());
            assert!(live < 0.01, "{fps} Hz: live levels differ by {live}");
            let (a, b) = (ridge.history.as_ref().unwrap(), reference.history.as_ref().unwrap());
            for k in [0, 1, 10, 40] {
                // Grain is seeded alike and re-rolled per row, so the rows
                // carry the same texture.
                let row = worst_difference(a.get(k), b.get(k));
                assert!(row < 0.01, "{fps} Hz: history row {k} differs by {row}");
            }
        }
    }

    #[test]
    fn the_picture_does_not_depend_on_frame_timing() {
        let reference = run(240.0, 3.0, 0.0);
        for fps in [60.0, 120.0, 144.0] {
            let ridge = run(fps, 3.0, 0.9);
            assert_eq!(rows_cut(&ridge, 100.0), rows_cut(&reference, 100.0), "{fps} Hz");
            let live = worst_difference(ridge.levels(), reference.levels());
            assert!(live < 0.01, "{fps} Hz: live levels differ by {live}");
        }
    }

    #[test]
    fn the_stack_glides_at_a_constant_speed() {
        for fps in [50.0, 60.0, 120.0, 144.0, 360.0] {
            let mut ridge = Ridge::new(RidgeParams::default(), 7);
            let start = 100.0;
            ridge.advance(start, Some(&Synthetic), 720.0);
            let period = ridge.row_period();
            for i in 1..600 {
                let now = start + i as f64 / fps;
                assert!(ridge.advance(now, Some(&Synthetic), 720.0));
                let travelled = rows_cut(&ridge, start) as f64 + ridge.frame(now).unwrap().phase;
                let expected = (now - start) / period;
                assert!((travelled - expected).abs() < 1e-6, "{fps} Hz at frame {i}");
            }
        }
    }

    #[test]
    fn a_ridge_kept_moving_scrolls_through_silence() {
        let mut ridge = Ridge::new(RidgeParams::default(), 7);
        ridge.keep_moving = true;
        let mut now = 100.0;
        for _ in 0..(5.0 * 120.0) as usize {
            now += 1.0 / 120.0;
            assert!(ridge.advance(now, Some(&Silent), 720.0));
        }
        assert!(!ridge.is_idle());
        assert!(rows_cut(&ridge, 100.0 + 1.0 / 120.0) > 145);
        // Let go, it comes to rest as usual.
        ridge.keep_moving = false;
        for _ in 0..(3.0 * 120.0) as usize {
            now += 1.0 / 120.0;
            ridge.advance(now, Some(&Silent), 720.0);
        }
        assert!(ridge.is_idle());
        // And sets off again when told to keep moving.
        ridge.keep_moving = true;
        now += 1.0 / 120.0;
        assert!(ridge.advance(now, Some(&Silent), 720.0));
        assert!(!ridge.is_idle());
    }

    #[test]
    fn silence_brings_the_ridge_to_rest_and_sound_wakes_it() {
        let mut ridge = Ridge::new(RidgeParams::default(), 7);
        let fps = 120.0;
        let mut now = 100.0;
        for _ in 0..120 {
            now += 1.0 / fps;
            assert!(ridge.advance(now, Some(&Synthetic), 720.0));
        }
        assert!(ridge.levels().iter().any(|&l| l > 0.1));

        let mut paints = 0;
        for _ in 0..(4.0 * fps) as usize {
            now += 1.0 / fps;
            if ridge.advance(now, Some(&Silent), 720.0) {
                paints += 1;
            }
        }
        assert!(ridge.is_idle());
        assert!(ridge.levels().iter().all(|&l| l == 0.0));
        // The line decays, then the stack carries its last row off the top.
        assert!(paints > (1.7 * fps) as usize && paints < (2.6 * fps) as usize, "{paints}");
        assert_eq!(ridge.frame(now).unwrap().phase, 0.0);

        // At rest nothing is painted, however long.
        now += 30.0;
        assert!(!ridge.advance(now, Some(&Silent), 720.0));
        assert!(ridge.is_idle());

        // Sound wakes it on the next frame.
        now += 1.0 / fps;
        assert!(ridge.advance(now, Some(&Synthetic), 720.0));
        assert!(!ridge.is_idle());
        now += 1.0 / fps;
        assert!(ridge.advance(now, Some(&Synthetic), 720.0));
        assert!(ridge.levels().iter().any(|&l| l > 0.0));
        assert!(ridge.frame(now).unwrap().phase > 0.0);
    }

    #[test]
    fn a_stall_resumes_without_catching_up() {
        let mut ridge = Ridge::new(RidgeParams::default(), 7);
        let mut now = 100.0;
        for _ in 0..60 {
            now += 1.0 / 60.0;
            ridge.advance(now, Some(&Synthetic), 720.0);
        }
        let newest = ridge.history.as_ref().unwrap().get(0).to_vec();
        let phase = ridge.frame(now).unwrap().phase;
        now += 20.0;
        ridge.advance(now, Some(&Synthetic), 720.0);
        // No row was cut across the gap.
        assert_eq!(ridge.history.as_ref().unwrap().get(0), newest);
        assert!((ridge.frame(now).unwrap().phase - phase).abs() < 1e-9);
    }

    #[test]
    fn the_level_curve_cuts_the_floor_and_clamps() {
        let p = RidgeParams::default();
        assert_eq!(Ridge::shape(0.59, &p), 0.0);
        assert!(Ridge::shape(0.6, &p) < 1e-20);
        assert_eq!(Ridge::shape(1.0, &p), 1.0);
        let mid = Ridge::shape(0.8, &p);
        assert!((mid - 0.0625 * 1.15).abs() < 1e-6, "{mid}");
    }
}
