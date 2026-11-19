//! Row helpers for the ridgeline: a stack of lines, each a past moment of the spectrum
//! drawn as a profile along a bent log frequency axis, bass on the left.
//! Pure functions with explicit output buffers, so a paint allocates nothing.

use super::params::RidgeParams;

/// Rounds halves toward positive infinity (JavaScript's `Math.round`): `-2.5` gives `-2`.
pub fn js_round(x: f64) -> f64 {
    (x + 0.5).floor()
}

/// Scratch buffers the row helpers reuse between calls.
#[derive(Default)]
pub struct RidgeScratch {
    /// The bell for the last `sigma` and radius `spread_row` ran with.
    bell: Vec<f32>,
    bell_sigma: f64,
    /// Secant of each segment, for `monotone_tangents`.
    secant: Vec<f32>,
    /// Slope at each point, for `resample_row`.
    slope: Vec<f32>,
}

/// Averages a frame's channels into one 0...1 level per band, bass first.
/// The row is zeroed unless `levels` holds `channels` runs of `out.len()`.
pub fn mix_bands(levels: &[f32], channels: usize, out: &mut [f32]) {
    let n = out.len();
    if channels == 0 || levels.len() != n * channels {
        out.fill(0.0);
        return;
    }
    let scale = 1.0 / channels as f64;
    for (k, mixed) in out.iter_mut().enumerate() {
        let sum: f64 = (0..channels).map(|c| f64::from(levels[c * n + k])).sum();
        *mixed = (sum * scale) as f32;
    }
}

/// Blends each point with its two neighbours; `amount` is the neighbours' total weight.
pub fn smooth_row(src: &[f32], out: &mut [f32], amount: f64) {
    let n = src.len();
    let side = amount / 2.0;
    let own = 1.0 - amount;
    for i in 0..n {
        let l = f64::from(src[i.saturating_sub(1)]);
        let r = f64::from(src[(i + 1).min(n - 1)]);
        out[i] = (own * f64::from(src[i]) + side * (l + r)) as f32;
    }
}

/// Widens every peak without lowering it: each point becomes the largest of itself
/// and its neighbours scaled by a bell of width `sigma` (in points). 0 is identity.
pub fn spread_row(src: &[f32], out: &mut [f32], sigma: f64, scratch: &mut RidgeScratch) {
    let n = src.len();
    if !(sigma > 0.0) {
        out[..n].copy_from_slice(src);
        return;
    }
    if n == 0 {
        return;
    }
    // Beyond three sigma the bell is under 1.2 % and changes nothing visible.
    let radius = (n - 1).min((sigma * 3.0).ceil() as usize);
    if sigma != scratch.bell_sigma || scratch.bell.len() != radius + 1 {
        scratch.bell = (0..=radius)
            .map(|d| (-((d * d) as f64) / (2.0 * sigma * sigma)).exp() as f32)
            .collect();
        scratch.bell_sigma = sigma;
    }
    for i in 0..n {
        let mut best = f64::from(src[i]);
        for d in 1..=radius {
            let k = f64::from(scratch.bell[d]);
            if i >= d {
                best = best.max(f64::from(src[i - d]) * k);
            }
            if i + d < n {
                best = best.max(f64::from(src[i + d]) * k);
            }
        }
        out[i] = best as f32;
    }
}

/// Scales every point by `1 + amount * noise[i]` (`noise` in -1...1), clamped at zero.
pub fn apply_grain(src: &[f32], out: &mut [f32], noise: &[f32], amount: f64) {
    if !(amount > 0.0) {
        out[..src.len()].copy_from_slice(src);
        return;
    }
    for ((out, &value), &noise) in out.iter_mut().zip(src).zip(noise) {
        let v = f64::from(value) * (1.0 + amount * f64::from(noise));
        *out = if v > 0.0 { v as f32 } else { 0.0 };
    }
}

/// The grain's random source: SplitMix64, seedable so a render can be repeated exactly.
pub struct RidgeRandom {
    state: u64,
}

impl RidgeRandom {
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Fills `noise` with uniform values in -1...1.
    pub fn reroll(&mut self, noise: &mut [f32]) {
        for value in noise {
            *value = (self.next() >> 40) as f32 / (1u32 << 23) as f32 - 1.0;
        }
    }
}

/// Per-point multiplier tapering both ends of a row to its baseline: a raised-cosine
/// ramp over the first and last `taper` fraction of the points.
pub fn edge_window(n: usize, taper: f64) -> Vec<f32> {
    let ramp = js_round(taper.max(0.0) * n as f64) as usize;
    (0..n)
        .map(|i| {
            let from_edge = i.min(n - 1 - i);
            if ramp == 0 || from_edge >= ramp {
                1.0
            } else {
                (0.5 - 0.5 * (std::f64::consts::PI * from_edge as f64 / ramp as f64).cos()) as f32
            }
        })
        .collect()
}

/// Fixed-size ring of past rows, newest first.
pub struct RidgeHistory {
    rows: usize,
    width: usize,
    slots: Vec<f32>,
    silence: Vec<f32>,
    /// Slot the next push writes; the newest row is the slot before it.
    head: usize,
}

impl RidgeHistory {
    pub fn new(rows: usize, width: usize) -> Self {
        let rows = rows.max(1);
        Self { rows, width, slots: vec![0.0; rows * width], silence: vec![0.0; width], head: 0 }
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    pub fn width(&self) -> usize {
        self.width
    }

    /// Pushes `row` as the newest, cut or zero-padded to the ring's width.
    pub fn push(&mut self, row: &[f32]) {
        let slot = &mut self.slots[self.head * self.width..(self.head + 1) * self.width];
        let m = row.len().min(slot.len());
        slot[..m].copy_from_slice(&row[..m]);
        slot[m..].fill(0.0);
        self.head = (self.head + 1) % self.rows;
    }

    /// Row `k` back from the newest (0 = newest); silence for rows not held.
    pub fn get(&self, k: usize) -> &[f32] {
        if k >= self.rows {
            return &self.silence;
        }
        let i = (self.head + self.rows - 1 - k) % self.rows;
        &self.slots[i * self.width..(i + 1) * self.width]
    }
}

/// Where one row of the stack sits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RidgeRowGeometry {
    /// Y of the row's resting line, in points from the top.
    pub baseline: f64,
    /// Points a full-scale point rises above the baseline.
    pub scale: f64,
    pub alpha: f64,
}

/// Where a row sits at `depth` (0 front, 1 back) in a frame `h` points tall.
/// A fractional depth places a row moving between slots.
pub fn ridge_row(depth: f64, h: f64, p: &RidgeParams) -> RidgeRowGeometry {
    let front = h * (1.0 - p.ridge_bottom);
    RidgeRowGeometry {
        baseline: front - depth * h * p.ridge_height,
        scale: h * p.ridge_peak * (1.0 + (p.ridge_depth_scale - 1.0) * depth),
        alpha: p.ridge_back_alpha
            + (p.ridge_alpha - p.ridge_back_alpha) * (1.0 - depth).max(0.0).powf(p.ridge_fade_curve),
    }
}

/// Slopes for a curve through `y` that never overshoots between neighbouring points
/// (Fritsch-Carlson monotone cubic interpolation).
fn monotone_tangents(y: &[f32], out: &mut [f32], secant: &mut Vec<f32>) {
    let n = y.len();
    if n == 0 {
        return;
    }
    if n == 1 {
        out[0] = 0.0;
        return;
    }
    secant.clear();
    secant.extend(y.windows(2).map(|w| (f64::from(w[1]) - f64::from(w[0])) as f32));
    let d = secant.as_slice();
    out[0] = d[0];
    out[n - 1] = d[n - 2];
    for k in 1..n - 1 {
        let (l, r) = (f64::from(d[k - 1]), f64::from(d[k]));
        out[k] = if l * r <= 0.0 { 0.0 } else { ((l + r) / 2.0) as f32 };
    }
    // Keep every segment monotone: flat segments get flat ends, and end slopes too
    // steep for the secant are scaled back onto the circle of radius 3.
    for k in 0..n - 1 {
        if d[k] == 0.0 {
            out[k] = 0.0;
            out[k + 1] = 0.0;
            continue;
        }
        let dk = f64::from(d[k]);
        let a = f64::from(out[k]) / dk;
        let b = f64::from(out[k + 1]) / dk;
        let r2 = a * a + b * b;
        if r2 > 9.0 {
            let t = 3.0 / r2.sqrt();
            out[k] = (t * a * dk) as f32;
            out[k + 1] = (t * b * dk) as f32;
        }
    }
}

/// Band position (0 to `bands - 1`) of each resampled point across the width. `curve`
/// bends the axis: 1 spreads the bands evenly, below 1 widens the low end.
pub fn axis_samples(bands: usize, curve: f64, out: &mut [f32]) {
    if out.is_empty() {
        return;
    }
    let last = out.len() - 1;
    let inverse = if curve > 0.0 { 1.0 / curve } else { 1.0 };
    let span = bands.saturating_sub(1) as f64;
    for (j, sample) in out.iter_mut().enumerate() {
        let x = if last > 0 { j as f64 / last as f64 } else { 0.0 };
        *sample = (x.powf(inverse) * span) as f32;
    }
}

/// Resamples a row along its monotone cubic at the band positions in `at`.
/// `out` must be as long as `at` or it is zeroed.
pub fn resample_row(src: &[f32], at: &[f32], out: &mut [f32], scratch: &mut RidgeScratch) {
    let n = src.len();
    if n == 0 || out.len() != at.len() {
        out.fill(0.0);
        return;
    }
    if n == 1 {
        out.fill(src[0]);
        return;
    }
    scratch.slope.resize(n, 0.0);
    monotone_tangents(src, &mut scratch.slope, &mut scratch.secant);
    let m = &scratch.slope;
    let top = (n - 1) as f64;
    for (value, &position) in out.iter_mut().zip(at) {
        let c = f64::from(position).clamp(0.0, top);
        let i = (c.floor() as usize).min(n - 2);
        let t = c - i as f64;
        let t2 = t * t;
        let t3 = t2 * t;
        *value = ((2.0 * t3 - 3.0 * t2 + 1.0) * f64::from(src[i])
            + (t3 - 2.0 * t2 + t) * f64::from(m[i])
            + (-2.0 * t3 + 3.0 * t2) * f64::from(src[i + 1])
            + (t3 - t2) * f64::from(m[i + 1])) as f32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_round_rounds_halves_up() {
        assert_eq!(js_round(2.5), 3.0);
        assert_eq!(js_round(-2.5), -2.0);
        assert_eq!(js_round(1.4), 1.0);
    }

    #[test]
    fn mixing_averages_the_channels() {
        let mut out = [0.0; 2];
        mix_bands(&[0.2, 0.4, 0.6, 0.8], 2, &mut out);
        assert!((out[0] - 0.4).abs() < 1e-6 && (out[1] - 0.6).abs() < 1e-6);
        mix_bands(&[0.2, 0.4, 0.6], 2, &mut out);
        assert_eq!(out, [0.0, 0.0]);
    }

    #[test]
    fn spreading_widens_a_peak_without_lowering_it() {
        let src = [0.0, 0.0, 1.0, 0.0, 0.0];
        let mut out = [0.0; 5];
        spread_row(&src, &mut out, 1.0, &mut RidgeScratch::default());
        assert_eq!(out[2], 1.0);
        assert!((out[1] - (-0.5f64).exp() as f32).abs() < 1e-6);
        assert_eq!(out[1], out[3]);
        assert!(out[0] > 0.0 && out[0] < out[1]);
    }

    #[test]
    fn resampling_passes_through_the_points_and_never_overshoots() {
        let src = [0.0, 0.0, 1.0, 0.2, 0.2, 0.0];
        let mut at = vec![0.0; 26];
        axis_samples(src.len(), 1.0, &mut at);
        let mut out = vec![0.0; 26];
        resample_row(&src, &at, &mut out, &mut RidgeScratch::default());
        for (k, &v) in src.iter().enumerate() {
            assert!((out[k * 5] - v).abs() < 1e-6);
        }
        assert!(out.iter().all(|&v| (0.0..=1.0).contains(&v)));
    }

    #[test]
    fn the_history_returns_rows_newest_first() {
        let mut history = RidgeHistory::new(3, 2);
        for i in 1..=4 {
            history.push(&[i as f32, 0.0]);
        }
        assert_eq!(history.get(0)[0], 4.0);
        assert_eq!(history.get(2)[0], 2.0);
        assert_eq!(history.get(3)[0], 0.0);
    }

    #[test]
    fn the_edge_window_tapers_both_ends() {
        let w = edge_window(100, 0.22);
        assert_eq!(w[0], 0.0);
        assert_eq!(w[99], 0.0);
        assert_eq!(w[50], 1.0);
        assert!(w[10] > 0.0 && w[10] < 1.0);
    }
}
