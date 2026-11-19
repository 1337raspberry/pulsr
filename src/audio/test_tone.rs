//! A silent synthetic stereo signal delivered in real time in place of a capture:
//! a kick twice a second, an off-beat noise tick and a slow sine sweep.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use super::PcmHandler;
use crate::clock;

const SAMPLE_RATE: f64 = 48_000.0;
const INTERVAL: Duration = Duration::from_millis(4);

pub struct TestTone {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl TestTone {
    pub fn start(mut handler: PcmHandler) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let thread = thread::spawn(move || {
            let mut generator = Generator::default();
            let start = clock::now();
            let mut produced: u64 = 0;
            let (mut left, mut right) = (Vec::new(), Vec::new());
            while !stopped.load(Ordering::Relaxed) {
                let due = ((clock::now() - start) * SAMPLE_RATE) as u64;
                let count = due.saturating_sub(produced).min(SAMPLE_RATE as u64 / 10) as usize;
                if count > 0 {
                    left.resize(count, 0.0);
                    right.resize(count, 0.0);
                    for i in 0..count {
                        let t = (produced + i as u64) as f64 / SAMPLE_RATE;
                        (left[i], right[i]) = generator.sample(t);
                    }
                    handler(&left, &right, SAMPLE_RATE, start + produced as f64 / SAMPLE_RATE);
                    produced += count as u64;
                }
                thread::sleep(INTERVAL);
            }
        });
        Self { stop, thread: Some(thread) }
    }
}

impl Drop for TestTone {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct Generator {
    sweep_left: f64,
    sweep_right: f64,
    noise_state: u64,
    last_noise: f32,
}

impl Default for Generator {
    fn default() -> Self {
        Self { sweep_left: 0.0, sweep_right: 0.0, noise_state: 0x9E37_79B9_7F4A_7C15, last_noise: 0.0 }
    }
}

impl Generator {
    /// Stateful: call once per sample, in order of `t` (seconds).
    fn sample(&mut self, t: f64) -> (f32, f32) {
        use std::f64::consts::TAU;

        // Kick: a sine falling from 140 Hz to 50 Hz under a fast decay.
        let beat = t.rem_euclid(0.5);
        let kick = 0.6
            * (-beat * 18.0).exp()
            * (TAU * (50.0 * beat + 90.0 / 40.0 * (1.0 - (-beat * 40.0).exp()))).sin();

        // Tick: differenced noise on the off-beat, so it sits in the treble.
        let off = (t + 0.25).rem_euclid(0.5);
        let noise = self.noise();
        let tick = f64::from(noise - self.last_noise) * 0.12 * (-off * 90.0).exp();
        self.last_noise = noise;

        // Sweep: 200 Hz to 3.2 kHz and back over eight seconds.
        let sweep = 0.5 + 0.5 * (TAU * t / 8.0).sin();
        self.sweep_left += TAU * 200.0 * 2f64.powf(4.0 * sweep) / SAMPLE_RATE;
        self.sweep_right += TAU * 200.0 * 2f64.powf(4.0 * (1.0 - sweep)) / SAMPLE_RATE;
        (
            (kick + tick + 0.1 * self.sweep_left.sin()) as f32,
            (kick + tick + 0.1 * self.sweep_right.sin()) as f32,
        )
    }

    /// Uniform noise in -1...1 (xorshift).
    fn noise(&mut self) -> f32 {
        self.noise_state ^= self.noise_state << 13;
        self.noise_state ^= self.noise_state >> 7;
        self.noise_state ^= self.noise_state << 17;
        (self.noise_state >> 40) as f32 / (1u32 << 23) as f32 - 1.0
    }
}
