//! Maps per-band dBFS levels to heights (0...1) against a running peak, after a
//! spectral tilt about the centre of the band range lifts the treble.

use crate::bands;

/// Floor for every level the mapper handles, in dBFS.
pub const DB_FLOOR: f32 = -90.0;
/// Width of the visible window below the running peak, in dB.
pub const DYNAMIC_RANGE_DB: f32 = 55.0;
/// Headroom above the running peak, in dB.
pub const PEAK_HEADROOM_DB: f32 = 2.0;
/// Exponent applied after normalisation, so quiet passages aren't flatlined.
pub const COMPRESSION: f32 = 0.6;
/// Running-peak seed for a fresh mapper, in dBFS.
pub const PEAK_SEED_DB: f32 = -20.0;
/// Running-peak release rate, in dB per second.
pub const PEAK_DECAY_DB_PER_SEC: f32 = 6.0;
/// Spectral tilt, in dB per octave.
pub const TILT_DB_PER_OCTAVE: f32 = 1.0;
/// Lowest value the running peak decays to, in dBFS.
pub const PEAK_FLOOR_DB: f32 = -40.0;

pub struct LevelMapper {
    peak_db: f32,
    decay_per_frame: f32,
    tilt_db_per_octave: f32,
    /// Per-value tilt offsets for the frame width last mapped.
    tilt: Vec<f32>,
    /// The tilted values of the frame being mapped, kept between frames.
    tilted: Vec<f32>,
}

impl LevelMapper {
    /// The peak's release is scaled by `fps` so it is the same per second at any rate.
    pub fn new(fps: u32) -> Self {
        Self {
            peak_db: PEAK_SEED_DB,
            decay_per_frame: PEAK_DECAY_DB_PER_SEC / fps.max(1) as f32,
            tilt_db_per_octave: TILT_DB_PER_OCTAVE,
            tilt: Vec::new(),
            tilted: Vec::new(),
        }
    }

    #[cfg(test)]
    pub fn peak_db(&self) -> f32 {
        self.peak_db
    }

    /// Maps one frame into `out`, advancing the running peak. The tilt is applied first;
    /// a band at the floor is silence and stays there.
    pub fn map(&mut self, db: &[f32], out: &mut [f32]) {
        if self.tilt.len() != db.len() {
            self.tilt = tilt_offsets(db.len(), self.tilt_db_per_octave);
        }
        self.tilted.clear();
        self.tilted.extend(db.iter().zip(&self.tilt).map(|(&d, &t)| {
            let d = sanitise_db(d);
            if d > DB_FLOOR { (d + t).max(DB_FLOOR) } else { d }
        }));
        let frame_max = self.tilted.iter().copied().fold(DB_FLOOR, f32::max);
        if frame_max > self.peak_db {
            self.peak_db = frame_max;
        } else {
            self.peak_db = (self.peak_db - self.decay_per_frame)
                .max(frame_max)
                .max(PEAK_FLOOR_DB);
        }
        let ceiling = self.peak_db + PEAK_HEADROOM_DB;
        let floor = (ceiling - DYNAMIC_RANGE_DB).max(DB_FLOOR);
        for (level, &d) in out.iter_mut().zip(&self.tilted) {
            *level = level_in_range(d, floor, ceiling);
        }
    }
}

fn sanitise_db(db: f32) -> f32 {
    if db.is_finite() {
        db.max(DB_FLOOR)
    } else if db == f32::INFINITY {
        0.0
    } else {
        DB_FLOOR
    }
}

/// A dBFS value as 0...1 within `[floor, ceiling]`, compressed. Degenerate ranges give 0.
fn level_in_range(db: f32, floor: f32, ceiling: f32) -> f32 {
    if !db.is_finite() || ceiling <= floor || db <= floor {
        return 0.0;
    }
    let t = (db.min(ceiling) - floor) / (ceiling - floor);
    t.powf(COMPRESSION)
}

/// `slope` dB per octave, zero at the centre of the band range. A width that is not
/// a whole number of channels, or a single band, gets no tilt.
fn tilt_offsets(len: usize, slope: f32) -> Vec<f32> {
    let n = len / bands::CHANNELS;
    if slope == 0.0 || n < 2 || !len.is_multiple_of(bands::CHANNELS) {
        return vec![0.0; len];
    }
    let span = (bands::FREQ_HIGH_HZ / bands::FREQ_LOW_HZ).log2() as f32;
    (0..len)
        .map(|i| {
            let k = (i % n) as f32 / (n - 1) as f32;
            slope * span * (k - 0.5)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_peak_jumps_up_and_relaxes_per_second() {
        let mut out = [0.0; 4];
        for fps in [60, 240] {
            let mut mapper = LevelMapper::new(fps);
            mapper.map(&[-10.0, -30.0, -10.0, -30.0], &mut out);
            let peak = mapper.peak_db();
            assert!(peak > PEAK_SEED_DB);
            for _ in 0..fps {
                mapper.map(&[-80.0; 4], &mut out);
            }
            assert!((mapper.peak_db() - (peak - PEAK_DECAY_DB_PER_SEC)).abs() < 0.01, "{fps} fps");
        }
    }

    #[test]
    fn levels_span_zero_to_one() {
        let mut mapper = LevelMapper::new(240);
        let mut out = [0.0; 4];
        mapper.map(&[-10.0, -100.0, -10.0, -100.0], &mut out);
        assert!(out[0] > 0.9 && out[0] <= 1.0);
        assert_eq!(out[1], 0.0);
    }
}
