//! Ridge layout, level curve, easing and sync values.
//! Every time-based value is in real time (milliseconds, or a factor per 60 Hz reference
//! frame), never in display frames, so the picture moves the same at any refresh rate.

#[derive(Clone, Debug, PartialEq)]
pub struct RidgeParams {
    /// Rows in the stack, the live front row included.
    pub ridge_rows: usize,
    /// Height of the stack (front to back baseline) as a fraction of the frame height.
    pub ridge_height: f64,
    /// Full-scale displacement on the front row as a fraction of the frame height.
    pub ridge_peak: f64,
    /// Peak scale of the back row relative to the front; 1 is flat.
    pub ridge_depth_scale: f64,
    /// Gap between the front baseline and the bottom edge as a fraction of the frame height.
    pub ridge_bottom: f64,
    /// Width of the ridge as a fraction of the frame width, centred.
    pub ridge_span: f64,
    /// Stroke width in points.
    pub ridge_line_width: f64,
    /// Points per band each row is resampled to along a monotone cubic (1...8).
    pub ridge_oversample: usize,
    /// Line alpha at the front row.
    pub ridge_alpha: f64,
    /// Line alpha at the back row.
    pub ridge_back_alpha: f64,
    /// Alpha fade from front to back: 1 is linear, below 1 stays bright further back.
    pub ridge_fade_curve: f64,
    /// Interval between history rows, in ms.
    pub ridge_row_ms: f64,
    /// Pixels the newest history row must have moved off the live row before it is drawn.
    pub ridge_emerge: f64,
    /// Sigma in points of the bell each peak is spread into, before `ridge_smooth`; 0 is off.
    pub ridge_spread: f64,
    /// Neighbour blend applied to each point, 0...1; 0 is identity.
    pub ridge_smooth: f64,
    /// Random texture as a fraction of each point's height, re-rolled per history row; 0 is off.
    pub ridge_grain: f64,
    /// Fraction of the ridge width over which each end tapers to the baseline.
    pub ridge_edge_taper: f64,
    /// Easing factor per 60 Hz reference frame while a point rises.
    pub ridge_attack: f64,
    /// Easing factor per 60 Hz reference frame while a point falls.
    pub ridge_decay: f64,
    /// Level-curve exponent; 1 is identity.
    pub ridge_gamma: f64,
    /// Level-curve threshold, 0...1; 0 is identity.
    pub ridge_floor_cut: f64,
    /// Level-curve multiplier; 1 is identity.
    pub ridge_gain: f64,
    /// Exponent on the log frequency axis: 1 is plain, below 1 gives the bass more width.
    pub ridge_axis_curve: f64,
}

impl Default for RidgeParams {
    fn default() -> Self {
        Self {
            ridge_rows: 51,
            ridge_height: 0.621,
            ridge_peak: 0.6,
            ridge_depth_scale: 0.394,
            ridge_bottom: 0.01,
            ridge_span: 1.0,
            ridge_line_width: 1.25,
            ridge_oversample: 5,
            ridge_alpha: 0.85,
            ridge_back_alpha: 0.0,
            ridge_fade_curve: 0.75,
            ridge_row_ms: 33.0,
            ridge_emerge: 1.0,
            ridge_spread: 1.0,
            ridge_smooth: 0.1,
            ridge_grain: 0.03,
            ridge_edge_taper: 0.22,
            ridge_attack: 0.67,
            ridge_decay: 0.5,
            ridge_gamma: 4.0,
            ridge_floor_cut: 0.6,
            ridge_gain: 1.15,
            ridge_axis_curve: 0.75,
        }
    }
}

/// The easing factors are given per frame of this length, in seconds.
pub const EASE_REFERENCE_DT: f64 = 1.0 / 60.0;

/// Share of the remaining distance a point moves in `dt` seconds:
/// `1 - (1 - ease) ^ (dt / reference)`, so the motion does not depend on the step size.
pub fn ease_alpha(ease: f64, dt: f64) -> f64 {
    if dt <= 0.0 {
        return 0.0;
    }
    1.0 - (1.0 - ease.clamp(0.0, 1.0)).powf(dt / EASE_REFERENCE_DT)
}

/// Seconds an easing factor takes to carry a point halfway to a new level.
pub fn ease_half_time(ease: f64) -> f64 {
    let rest = 1.0 - ease.clamp(0.0, 1.0);
    if rest <= 0.0 {
        return 0.0;
    }
    if rest >= 1.0 {
        return f64::INFINITY;
    }
    EASE_REFERENCE_DT * 0.5f64.ln() / rest.ln()
}

/// Seconds ahead of the audible position a paint reads its frame: the display delay,
/// half the paint period, half the analysis period and the attack easing's half time.
pub fn sync_lead(
    params: &RidgeParams,
    display_delay: f64,
    paint_period: f64,
    analysis_period: f64,
) -> f64 {
    display_delay + paint_period / 2.0 + analysis_period / 2.0 + ease_half_time(params.ridge_attack)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn easing_composes_across_steps() {
        let whole = ease_alpha(0.67, 1.0 / 60.0);
        assert!((whole - 0.67).abs() < 1e-12);
        let half = ease_alpha(0.67, 1.0 / 120.0);
        assert!(((1.0 - (1.0 - half) * (1.0 - half)) - whole).abs() < 1e-12);
        assert_eq!(ease_alpha(0.67, 0.0), 0.0);
    }

    #[test]
    fn the_lead_at_sixty_hertz_is_the_reference_lead() {
        let lead = sync_lead(&RidgeParams::default(), 1.0 / 60.0, 1.0 / 60.0, 1.0 / 60.0);
        assert!((lead - 0.045).abs() < 0.002, "{lead}");
        let fast = sync_lead(&RidgeParams::default(), 1.0 / 120.0, 1.0 / 120.0, 1.0 / 240.0);
        assert!(fast < 0.03, "{fast}");
    }
}
