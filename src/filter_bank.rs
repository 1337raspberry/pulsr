//! Spectrum analysis over PCM. Per channel, each band is a band-pass biquad whose squared
//! output is low-passed into a power envelope and sampled in dBFS at the frame rate.
//! Arithmetic is f64: the narrow bass bands' poles are too close to the unit circle for f32.

use crate::bands;

/// Corner of the power-envelope low-pass, in Hz.
pub const ENVELOPE_LOWPASS_HZ: f64 = 20.0;
/// Q of the envelope low-pass.
pub const ENVELOPE_LOWPASS_Q: f64 = 0.707;
/// Lowest power the level expression sees, which puts silence at -100 dBFS.
pub const POWER_FLOOR: f64 = 1e-10;

/// Filter state below this is zeroed after a run, so silence does not decay into
/// slow subnormals.
const STATE_FLOOR: f64 = 1e-30;

pub struct FilterBank {
    bands: usize,
    sample_rate: f64,
    fps: u64,
    // Band-pass coefficients per band, normalised by a0. The numerator is (b0, 0, -b0).
    b0: Vec<f64>,
    a1: Vec<f64>,
    a2: Vec<f64>,
    // Per channel per band (transposed direct form II): band-pass delays, envelope
    // low-pass delays and the latest envelope value.
    s1: Vec<f64>,
    s2: Vec<f64>,
    t1: Vec<f64>,
    t2: Vec<f64>,
    envelope: Vec<f64>,
    // Envelope low-pass coefficients, the same for every band.
    lb0: f64,
    lb1: f64,
    lb2: f64,
    la1: f64,
    la2: f64,
    rate: u64,
    /// Advances by `fps` per sample; a frame is cut each time it reaches `rate`.
    phase: u64,
    samples_seen: u64,
    db: Vec<f32>,
}

impl FilterBank {
    pub fn new(bands: usize, fps: u32, sample_rate: f64) -> Self {
        let bands = bands.max(2);
        let states = bands::CHANNELS * bands;
        let width = bands::octave_width(bands);
        let mut b0 = Vec::with_capacity(bands);
        let mut a1 = Vec::with_capacity(bands);
        let mut a2 = Vec::with_capacity(bands);
        for centre in bands::frequencies(bands) {
            // A centre above Nyquist has no filter; hold it just under.
            let f = centre.min(sample_rate * 0.45);
            let w0 = std::f64::consts::TAU * f / sample_rate;
            let alpha = w0.sin() * (std::f64::consts::LN_2 / 2.0 * width * w0 / w0.sin()).sinh();
            let a0 = 1.0 + alpha;
            b0.push(alpha / a0);
            a1.push(-2.0 * w0.cos() / a0);
            a2.push((1.0 - alpha) / a0);
        }

        let w0 = std::f64::consts::TAU * ENVELOPE_LOWPASS_HZ / sample_rate;
        let alpha = w0.sin() / (2.0 * ENVELOPE_LOWPASS_Q);
        let a0 = 1.0 + alpha;
        let lb0 = (1.0 - w0.cos()) / 2.0 / a0;

        Self {
            bands,
            sample_rate,
            fps: u64::from(fps.max(1)),
            b0,
            a1,
            a2,
            s1: vec![0.0; states],
            s2: vec![0.0; states],
            t1: vec![0.0; states],
            t2: vec![0.0; states],
            envelope: vec![0.0; states],
            lb0,
            lb1: (1.0 - w0.cos()) / a0,
            lb2: lb0,
            la1: -2.0 * w0.cos() / a0,
            la2: (1.0 - alpha) / a0,
            rate: (sample_rate.round() as u64).max(1),
            phase: 0,
            samples_seen: 0,
            db: vec![0.0; states],
        }
    }

    pub fn sample_rate(&self) -> f64 {
        self.sample_rate
    }

    /// Seconds of audio processed so far.
    pub fn position(&self) -> f64 {
        self.samples_seen as f64 / self.sample_rate
    }

    /// Clears every filter's state. The position carries on.
    pub fn reset(&mut self) {
        for state in [&mut self.s1, &mut self.s2, &mut self.t1, &mut self.t2, &mut self.envelope] {
            state.fill(0.0);
        }
    }

    /// Calls `on_frame` for each frame the block completes with the position of its last
    /// sample and one dBFS level per band per channel (left bands, then right).
    pub fn process(&mut self, left: &[f32], right: &[f32], mut on_frame: impl FnMut(f64, &[f32])) {
        let count = left.len().min(right.len());
        let mut done = 0;
        while done < count {
            // Samples left before the next frame is cut.
            let until_cut = (self.rate - self.phase).div_ceil(self.fps) as usize;
            let n = until_cut.min(count - done);
            self.run(&left[done..done + n], 0);
            self.run(&right[done..done + n], 1);
            done += n;
            self.samples_seen += n as u64;
            self.phase += n as u64 * self.fps;
            if self.phase >= self.rate {
                self.phase -= self.rate;
                for (db, envelope) in self.db.iter_mut().zip(&self.envelope) {
                    *db = (10.0 * envelope.max(POWER_FLOOR).log10()) as f32;
                }
                on_frame(self.position(), &self.db);
            }
        }
    }

    fn run(&mut self, x: &[f32], channel: usize) {
        let base = channel * self.bands;
        let (lb0, lb1, lb2, la1, la2) = (self.lb0, self.lb1, self.lb2, self.la1, self.la2);
        for k in 0..self.bands {
            let (b, c1, c2) = (self.b0[k], self.a1[k], self.a2[k]);
            let i = base + k;
            let (mut bs1, mut bs2) = (self.s1[i], self.s2[i]);
            let (mut lt1, mut lt2) = (self.t1[i], self.t2[i]);
            let mut e = self.envelope[i];
            for &sample in x {
                // One NaN would stay in the filter's state from here on.
                let v = if sample.is_finite() { f64::from(sample) } else { 0.0 };
                let y = b * v + bs1;
                bs1 = bs2 - c1 * y;
                bs2 = -b * v - c2 * y;
                let p = y * y;
                e = lb0 * p + lt1;
                lt1 = lb1 * p - la1 * e + lt2;
                lt2 = lb2 * p - la2 * e;
            }
            if bs1.abs() < STATE_FLOOR && bs2.abs() < STATE_FLOOR {
                bs1 = 0.0;
                bs2 = 0.0;
            }
            if lt1.abs() < STATE_FLOOR && lt2.abs() < STATE_FLOOR {
                lt1 = 0.0;
                lt2 = 0.0;
            }
            self.s1[i] = bs1;
            self.s2[i] = bs2;
            self.t1[i] = lt1;
            self.t2[i] = lt2;
            self.envelope[i] = e;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bands::BANDS;

    struct Frame {
        pos: f64,
        db: Vec<f32>,
    }

    fn sine(hz: f64, amplitude: f64, seconds: f64, rate: f64) -> Vec<f32> {
        (0..(seconds * rate) as usize)
            .map(|i| (amplitude * (std::f64::consts::TAU * hz * i as f64 / rate).sin()) as f32)
            .collect()
    }

    fn feed(bank: &mut FilterBank, left: &[f32], right: &[f32], block: usize) -> Vec<Frame> {
        let mut frames = Vec::new();
        for (l, r) in left.chunks(block).zip(right.chunks(block)) {
            bank.process(l, r, |pos, db| frames.push(Frame { pos, db: db.to_vec() }));
        }
        frames
    }

    #[test]
    fn a_centred_sine_reads_its_power() {
        let centres = bands::frequencies(BANDS);
        for rate in [48_000.0, 44_100.0] {
            let probe = 32;
            // Amplitude 0.1 is -20 dBFS peak, 3.01 dB less as mean power.
            let left = sine(centres[probe], 0.1, 1.5, rate);
            let right = vec![0.0; left.len()];
            let frames = feed(&mut FilterBank::new(BANDS, 60, rate), &left, &right, left.len());
            assert_eq!(frames.len(), 90, "{rate} Hz");
            let last = &frames.last().unwrap().db;
            assert_eq!(last.len(), BANDS * 2);
            assert!((last[probe] + 23.01).abs() < 0.15, "{rate} Hz: {}", last[probe]);
            assert!(last[probe - 1] < last[probe] - 3.0 && last[probe + 1] < last[probe] - 3.0);

            // Twelve bands down, a two-pole band-pass of this Q has fallen
            // by 1 / (Q · |r - 1/r|), r being the frequency ratio.
            let width = bands::octave_width(BANDS);
            let q = 1.0 / (2.0 * (std::f64::consts::LN_2 / 2.0 * width).sinh());
            let ratio = 2f64.powf(12.0 * width);
            let skirt = (20.0 * (1.0 / (q * (ratio - 1.0 / ratio))).log10()) as f32;
            assert!((last[probe - 12] - (last[probe] + skirt)).abs() < 1.0);
            assert_eq!(last[BANDS + probe], -100.0, "the silent channel");

            // The same audio in odd-sized blocks gives the same frames.
            let chunked = feed(&mut FilterBank::new(BANDS, 60, rate), &left, &right, 257);
            assert_eq!(chunked.len(), frames.len());
            for (a, b) in frames.iter().zip(&chunked) {
                assert_eq!(a.pos, b.pos);
                assert_eq!(a.db, b.db);
            }
        }
    }

    #[test]
    fn the_frame_rate_is_independent_of_the_block_size() {
        let left = sine(440.0, 0.1, 1.0, 44_100.0);
        let frames = feed(&mut FilterBank::new(BANDS, 240, 44_100.0), &left, &left, 256);
        assert_eq!(frames.len(), 240);
        let last = frames.last().unwrap();
        assert!((last.pos - 1.0).abs() < 1e-9);
    }

    /// Half amplitude arrives at the onset delay plus the envelope low-pass's own lag.
    #[test]
    fn the_lowest_band_lags_by_its_onset_delay() {
        let rate = 48_000.0;
        let left = sine(bands::FREQ_LOW_HZ, 0.1, 1.5, rate);
        let frames = feed(&mut FilterBank::new(BANDS, 240, rate), &left, &left, 256);
        let steady = frames.last().unwrap().db[0];
        let half = frames.iter().find(|f| f.db[0] >= steady - 6.02).unwrap().pos;
        let table = bands::onset_delays(BANDS)[0];
        assert!(half > table && half < table + 0.06, "{half} vs {table}");
    }

    #[test]
    fn a_sample_that_is_not_a_number_is_heard_as_silence() {
        let rate = 48_000.0;
        let mut bank = FilterBank::new(BANDS, 240, rate);
        feed(&mut bank, &[f32::NAN, f32::INFINITY], &[f32::NEG_INFINITY, f32::NAN], 256);
        let tone = sine(1000.0, 0.5, 1.0, rate);
        let frames = feed(&mut bank, &tone, &tone, 256);
        let loudest = frames.last().unwrap().db.iter().copied().fold(f32::MIN, f32::max);
        assert!(loudest > -30.0, "{loudest}");
    }

    #[test]
    fn silence_after_a_signal_decays_to_exact_zero_state() {
        let rate = 48_000.0;
        let mut bank = FilterBank::new(BANDS, 240, rate);
        let tone = sine(100.0, 0.5, 0.2, rate);
        feed(&mut bank, &tone, &tone, 256);
        let silence = vec![0.0; (rate * 10.0) as usize];
        let frames = feed(&mut bank, &silence, &silence, 256);
        assert!(frames.last().unwrap().db.iter().all(|&d| d == -100.0));
        assert!(bank.s1.iter().chain(&bank.t1).all(|&s| s == 0.0));
    }

    #[test]
    fn the_analysis_is_cheap() {
        let rate = 48_000.0;
        let mut state: u64 = 88_172_645_463_325_252;
        let noise: Vec<f32> = (0..(rate * 10.0) as usize)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (state >> 40) as f32 / (1u32 << 23) as f32 - 1.0
            })
            .collect();
        let mut bank = FilterBank::new(BANDS, 240, rate);
        let started = std::time::Instant::now();
        let frames = feed(&mut bank, &noise, &noise, 256);
        let took = started.elapsed().as_secs_f64();
        assert_eq!(frames.len(), 2400);
        println!("10 s of audio analysed in {:.0} ms", took * 1000.0);
        assert!(took < 5.0);
    }
}
