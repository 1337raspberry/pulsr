//! The analysis bands: how many, where they sit, and how long each one's level
//! trails the audio it measures.

/// Analysis bands per channel.
pub const BANDS: usize = 64;
/// Channels analysed: left, then right.
pub const CHANNELS: usize = 2;
/// Centre frequency of the lowest band, in Hz.
pub const FREQ_LOW_HZ: f64 = 50.0;
/// Centre frequency of the highest band, in Hz.
pub const FREQ_HIGH_HZ: f64 = 16_000.0;

/// Frames of band levels emitted per second of audio, independent of the display's
/// refresh rate: the paint loop reads frames by time and interpolates.
pub const FRAME_RATE: u32 = 240;

/// Sample rate the onset delays are computed at; they barely move with the rate.
const ONSET_REFERENCE_RATE: f64 = 48_000.0;

/// Width of each band in octaves.
pub fn octave_width(bands: usize) -> f64 {
    (FREQ_HIGH_HZ / FREQ_LOW_HZ).log2() / bands.saturating_sub(1).max(1) as f64
}

/// Centre frequency of every band, log-spaced from `FREQ_LOW_HZ` to `FREQ_HIGH_HZ`.
pub fn frequencies(bands: usize) -> Vec<f64> {
    let width = octave_width(bands);
    (0..bands)
        .map(|k| FREQ_LOW_HZ * 2f64.powf(k as f64 * width))
        .collect()
}

/// Seconds each band's level trails the audio, lowest band first: the time a band-pass
/// takes to swell to half amplitude (about 48 ms at 50 Hz, 2.5 ms at 1 kHz).
pub fn onset_delays(bands: usize) -> Vec<f64> {
    let width = octave_width(bands);
    frequencies(bands)
        .into_iter()
        .map(|f| {
            let w0 = std::f64::consts::TAU * f / ONSET_REFERENCE_RATE;
            let alpha = w0.sin() * (std::f64::consts::LN_2 / 2.0 * width * w0 / w0.sin()).sinh();
            // Envelope time constant in samples: r^n = e^(-n/τ), r² = a2/a0.
            let tau = -2.0 / ((1.0 - alpha) / (1.0 + alpha)).ln();
            tau * std::f64::consts::LN_2 / ONSET_REFERENCE_RATE
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frequencies_span_the_range() {
        let f = frequencies(BANDS);
        assert_eq!(f.len(), BANDS);
        assert!((f[0] - 50.0).abs() < 1e-9);
        assert!((f[BANDS - 1] - 16_000.0).abs() < 1e-6);
        assert!((f[1] - 54.794).abs() < 0.001);
    }

    #[test]
    fn onset_delays_match_the_reference_table() {
        let d = onset_delays(BANDS);
        assert!((d[0] - 0.048_177_58).abs() < 1e-6, "{}", d[0]);
        assert!((d[BANDS - 1] - 0.000_149_836_69).abs() < 1e-7, "{}", d[BANDS - 1]);
        assert!(d.windows(2).all(|w| w[1] < w[0]));
    }
}
